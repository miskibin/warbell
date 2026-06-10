//! **Stone miners work real boulders — the town's NPC stone income (the wood mirror is
//! `lumberjack.rs`).** The Stone Miner plot (`BuildKind::Mine`) has no passive trickle; its
//! worker walks out to an actual ore boulder, picks it apart (the boulder regrows later — see
//! `verbs::deplete_ore` / `regrow_ore`), loads a stone CART, hauls it back to the yard, and only
//! THERE is the stone banked. Lose the miner on the road and you lose the load.
//!
//! It is a near-exact mirror of the woodcutter, with two deliberate divergences:
//!   * **Ranges far** — ore lives in the Rocky biome blob (east), well outside the woodcutter's
//!     safe ring, so there is NO `WORK_R` cap; the miner takes long, riskier trips.
//!   * **Carries a cart** — a small loaded stone wagon trails the miner home ([`Carting`]) in
//!     place of the woodcutter's shouldered log.
//!
//! The flee/blacklist machinery is shared with the woodcutter: a hostile inside [`DANGER_R`]
//! sends the miner running home (`lumberjack::Fleeing` + `flee_steer`) and blacklists the spot in
//! the shared `lumberjack::DangerSpots`. Self-defence is the shared `villagers::FightBack`.

use bevy::prelude::*;

use tileworld_core::town_store::BuildKind;

use crate::economy::Bank;
use crate::lumberjack::{DangerSpots, Fleeing};
use crate::steer;
use crate::town::Worker;
use crate::verbs::{deplete_ore, DepletedOre, OreNode, TrunkShake};
use crate::villagers::{FightBack, Guard, Townsfolk, Villager};
use crate::worldmap;

/// …and no closer than this to any ork camp centre (a miner never picks ore in a warband's lap).
const CAMP_AVOID: f32 = 22.0;
/// A hostile (ork / predator) this near a miner triggers the flight home.
const DANGER_R: f32 = 12.0;
/// Boulders this near a remembered scare are off-limits while it lasts.
const DANGER_BLACKLIST_R: f32 = 16.0;
/// How long a scare keeps its ground blacklisted (s).
const DANGER_TTL: f32 = 120.0;
/// Pick damage per work swing — with ore's `ORE_HP` 354, ~7 swings (≈15s) to deplete a boulder.
const PICK_DMG: f64 = 50.0;
/// Seconds between work swings — matches the overhead-swing work loop in `villager_limbs` (~2.1s).
const PICK_CD: f32 = 2.1;
/// Swing reach BEYOND the boulder's own blocker radius (centre-to-centre gate =
/// `blocker_r + PICK_REACH`). Steering stops the body at `blocker_r + body_r (0.28)`, so this
/// leaves ~0.5u of slack while keeping the miner right at the rock face — a flat 2.4u gate
/// (the old `2.0 + ORE_COLLISION_RADIUS`) had him swinging at air ~1.5u short of small rocks.
const PICK_REACH: f32 = 0.85;
/// Don't rescan the whole ore set every frame when there's nothing eligible.
const RETRY_SECS: f32 = 3.0;
/// A flight home gives up after this long even if the walls weren't reached.
const FLEE_SECS: f32 = 9.0;
/// No steering progress toward the boulder for this long → wedged; abandon + blacklist briefly.
const STALL_SECS: f32 = 4.0;
/// Pick SFX is only audible this near the hero (the guards' small-earshot convention).
const SFX_EARSHOT: f32 = 16.0;
/// Close enough to the yard to dump the cart on the pile (matches `worker_steer`'s post reach).
const HAUL_REACH: f32 = 1.8;
/// A* node budget for the cross-island haul. `navgrid::NAV_MAX_NODES` (1400) is sized for the
/// ~40-tile invader run and EXHAUSTS on the ~100-tile castle→Rocky trip — `find_path` then
/// returns empty and the miner would beeline into the river. Unreachable goals drain the open
/// set and exit early, so the generous budget only costs while a real route is being found.
const NAV_NODES: u32 = 20_000;
/// Seconds between A* replans while marching (staggered per entity; the route is long + stable).
const REPLAN_SECS: f32 = 2.5;

/// The boulder a miner is working: walk to it, swing on the cooldown, deplete it, pick the next.
#[derive(Component)]
pub struct MineJob {
    ore: Entity,
    atk_cd: f32,
    /// Seconds without steering progress (wedged on a river/prop) — bail at [`STALL_SECS`].
    stall: f32,
}

/// A loaded stone cart trailing the miner: haul it back to the Stone Miner yard — the stone is
/// banked only on arrival ([`cart_home`]). `cart` is the visible carried-cart child mesh.
#[derive(Component)]
pub struct Carting {
    amount: f64,
    cart: Option<Entity>,
    /// Seconds without steering progress on the way home — force a replan past [`STALL_SECS`].
    stall: f32,
}

pub struct MinerPlugin;

impl Plugin for MinerPlugin {
    fn build(&self, app: &mut App) {
        // `DangerSpots` + `flee_steer` are owned/registered by `LumberjackPlugin`; `init_resource`
        // is idempotent and `flee_steer` already drives every `Fleeing` villager (miners included).
        app.init_resource::<DangerSpots>().add_systems(
            Update,
            (mine_danger, assign_ore, pick_work, cart_home, attach_cart, shed_cart_at_muster)
                .run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

/// Threat sense: an ork or a predator prowling inside [`DANGER_R`] of a working (or carting) miner
/// sends it running home and blacklists the spot. A scared carter keeps the load — the flight ends
/// at the walls and [`cart_home`] finishes the delivery. (Mirror of `lumberjack::lumber_danger`;
/// scare-expiry is owned there, so this one only adds.)
#[allow(clippy::type_complexity)]
fn mine_danger(
    time: Res<Time>,
    mut danger: ResMut<DangerSpots>,
    mut commands: Commands,
    workers: Query<
        (Entity, &Villager),
        (
            With<Worker>,
            Or<(With<MineJob>, With<Carting>)>,
            Without<Fleeing>,
            Without<crate::dying::Dying>,
        ),
    >,
    orks: Query<&crate::orks::Ork, Without<crate::dying::Dying>>,
    animals: Query<&crate::wildlife::Animal, Without<crate::dying::Dying>>,
) {
    let now = time.elapsed_secs();
    if workers.is_empty() {
        return;
    }
    let mut hostiles: Vec<Vec2> = orks.iter().map(|o| o.pos).collect();
    hostiles.extend(
        animals.iter().filter(|a| crate::wildlife::is_hostile_species(a.species)).map(|a| a.pos),
    );
    for (e, v) in &workers {
        if let Some(hp) = hostiles.iter().find(|h| h.distance(v.pos) < DANGER_R) {
            commands.entity(e).try_remove::<MineJob>().try_insert(Fleeing { until: now + FLEE_SECS });
            danger.0.push((*hp, now + DANGER_TTL));
        }
    }
}

/// Hand each idle Stone-Miner-plot worker the nearest workable boulder (alive, safe ground, not
/// blacklisted). NO `WORK_R` cap — ore is far, in the Rocky biome. Throttled to [`RETRY_SECS`].
#[allow(clippy::type_complexity)]
fn assign_ore(
    time: Res<Time>,
    town: Res<crate::town::TownRes>,
    danger: Res<DangerSpots>,
    mut retry_at: Local<f32>,
    mut commands: Commands,
    workers: Query<
        (Entity, &Worker, &Villager),
        (
            With<Townsfolk>,
            Without<MineJob>,
            Without<Carting>,
            Without<Fleeing>,
            Without<FightBack>,
            Without<crate::dying::Dying>,
        ),
    >,
    ores: Query<(Entity, &OreNode, &Transform), Without<DepletedOre>>,
) {
    let now = time.elapsed_secs();
    if now < *retry_at {
        return;
    }
    *retry_at = now + RETRY_SECS;
    let camps: Vec<Vec2> = crate::camps::cage_positions().iter().map(|(_, c)| *c).collect();
    for (e, worker, v) in &workers {
        if town.0.plots.get(worker.idx).and_then(|p| p.kind) != Some(BuildKind::Mine) {
            continue; // farmers/woodcutters keep their own jobs — only miners roam to ore
        }
        let mut best: Option<(Entity, f32)> = None;
        for (oe, node, otf) in &ores {
            if node.ore.hp <= 0.0 {
                continue;
            }
            let op = Vec2::new(otf.translation.x, otf.translation.z);
            if crate::camps::in_clearing(op.x, op.y)
                || camps.iter().any(|c| c.distance(op) < CAMP_AVOID)
                || danger.0.iter().any(|(d, _)| d.distance(op) < DANGER_BLACKLIST_R)
            {
                continue;
            }
            let d = v.pos.distance(op);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((oe, d));
            }
        }
        if let Some((oe, _)) = best {
            commands.entity(e).try_insert(MineJob { ore: oe, atk_cd: 0.0, stall: 0.0 });
        }
    }
}

/// Walk the miner to its boulder and swing the pick on the cooldown; the depleting blow shatters
/// the boulder (regrow scheduled via [`deplete_ore`]) and loads the cart ([`Carting`] — NO stone
/// is banked here; that happens back at the yard in [`cart_home`]). At the boulder it counts
/// `at_post` and the overhead-swing work loop in `villager_limbs` plays.
#[allow(clippy::type_complexity)]
fn pick_work(
    time: Res<Time>,
    mut commands: Commands,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut danger: ResMut<DangerSpots>,
    hero: Res<crate::player::HeroState>,
    mut workers: Query<
        (Entity, &mut MineJob, &mut Worker, &mut Villager, &mut Transform, &mut crate::navgrid::NavPath),
        (Without<Carting>, Without<Fleeing>, Without<FightBack>, Without<crate::dying::Dying>),
    >,
    mut ores: Query<(&mut OreNode, &Transform), (Without<DepletedOre>, Without<Worker>)>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (self_e, mut job, mut worker, mut v, mut tf, mut path) in &mut workers {
        let Ok((mut node, otf)) = ores.get_mut(job.ore) else {
            // Depleted (regrowing) or gone — back to the pool; `assign_ore` hands out the next one.
            commands.entity(self_e).try_remove::<MineJob>();
            continue;
        };
        if node.ore.hp <= 0.0 {
            // Finished this frame by the hero (or us) before `DepletedOre` was visible — let go.
            commands.entity(self_e).try_remove::<MineJob>();
            continue;
        }
        let op = Vec2::new(otf.translation.x, otf.translation.z);
        let d = v.pos.distance(op);
        job.atk_cd -= dt;
        if d <= node.blocker_r + PICK_REACH {
            // At the boulder: face it, plant the feet, swing on the cooldown.
            worker.at_post = true;
            v.moving = false;
            job.stall = 0.0;
            let to = op - v.pos;
            if to.length_squared() > 1e-4 {
                v.facing = to.x.atan2(to.y);
            }
            if job.atk_cd <= 0.0 {
                job.atk_cd = PICK_CD;
                if hero.pos.distance(v.pos) < SFX_EARSHOT {
                    cues.write(crate::audio::AudioCue::OreChip);
                }
                let dir = (op - v.pos).normalize_or_zero();
                let reward = node.ore.stone_reward;
                if node.ore.damage(PICK_DMG, now as f64) {
                    // Depleted — but no stone yet: load the cart and haul it home. Clear
                    // `at_post` or `villager_limbs` keeps the pick stroke going on the walk.
                    worker.at_post = false;
                    deplete_ore(&mut commands, job.ore, otf.translation, now);
                    commands
                        .entity(self_e)
                        .try_remove::<MineJob>()
                        .try_insert(Carting { amount: reward, cart: None, stall: 0.0 });
                } else {
                    // The boulder judders under the pick (its rest yaw is restored after the shake).
                    commands.entity(job.ore).try_insert(TrunkShake::new(now, dir));
                }
            }
        } else {
            // March to the boulder: A* when far (thread the gates/river), direct steer when close.
            // Replan is TIME-throttled only — `cursor >= len` must NOT trigger one, or an empty
            // (failed) path re-runs the full A* every frame.
            worker.at_post = false;
            let step_target = if d > 6.0 {
                if now >= path.next_replan || path.goal_cached.distance(op) > 2.0 {
                    path.waypoints = crate::navgrid::path_to_budget(v.pos, op, NAV_NODES);
                    path.cursor = 0;
                    path.goal_cached = op;
                    path.next_replan = now + REPLAN_SECS + (self_e.to_bits() % 16) as f32 * 0.1;
                    if path.waypoints.is_empty() {
                        // No route even at the long-haul budget (boulder walled off / across
                        // unbridged water) — shun the spot and let `assign_ore` pick another,
                        // instead of beelining into whatever blocked the route.
                        danger.0.push((op, now + 45.0));
                        commands.entity(self_e).try_remove::<MineJob>();
                        continue;
                    }
                }
                while path.cursor < path.waypoints.len()
                    && v.pos.distance(path.waypoints[path.cursor]) < 1.2
                {
                    path.cursor += 1;
                }
                path.waypoints.get(path.cursor).copied().unwrap_or(op)
            } else {
                path.waypoints.clear();
                path.cursor = 0;
                op
            };
            let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
            match steer::advance(v.pos, v.facing, step_target, v.speed * dt, v.body_r, cur_y, 3.0 * dt) {
                Some(s) if s.moving => {
                    v.pos = s.pos;
                    v.facing = s.facing;
                    v.moving = true;
                    job.stall = 0.0;
                }
                _ => {
                    v.moving = false;
                    job.stall += dt;
                    if job.stall > STALL_SECS {
                        // Wedged (river bend, prop knot) — abandon this boulder and shun the spot
                        // briefly so the next pick is a different one.
                        danger.0.push((op, now + 45.0));
                        commands.entity(self_e).try_remove::<MineJob>();
                    }
                }
            }
        }
        // Ground-follow + bob (this system owns the transform while the job is on).
        let gy = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Cart the load back to the worker's own plot (the Stone Miner yard) and bank the stone THERE —
/// the delivery, not the dig, is what pays. Same A*-when-far march as [`pick_work`]; a wedged
/// carter just forces a replan (home is always reachable, unlike an arbitrary boulder).
#[allow(clippy::type_complexity)]
fn cart_home(
    time: Res<Time>,
    spots: Res<crate::town::PlotSpots>,
    mut commands: Commands,
    mut bank: ResMut<Bank>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut carters: Query<
        (Entity, &mut Carting, &Worker, &mut Villager, &mut Transform, &mut crate::navgrid::NavPath),
        (Without<Fleeing>, Without<FightBack>, Without<crate::dying::Dying>),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (self_e, mut carting, worker, mut v, mut tf, mut path) in &mut carters {
        let Some(yard) = spots.0.get(worker.idx).copied() else { continue };
        let d = v.pos.distance(yard);
        if d <= HAUL_REACH {
            // Cart on the pile — NOW the stone lands in the stock.
            bank.0.add_stone(carting.amount);
            floats.0.push(crate::combat_fx::FloatReq {
                world: Vec3::new(v.pos.x, tf.translation.y + 1.6, v.pos.y),
                text: format!("+{} stone", carting.amount as i64),
                color: Color::srgb(0.82, 0.82, 0.88),
                scale: 1.1,
            });
            if let Some(cart) = carting.cart {
                commands.entity(cart).try_despawn();
            }
            commands.entity(self_e).try_remove::<Carting>();
            v.moving = false;
            continue;
        }
        // March home: A* when far, direct steer when close (same shape as pick_work's march —
        // time-throttled replan, long-haul budget). Home is always reachable, so an empty path
        // (start pocketed by props) just falls back to direct steer until the next replan.
        let step_target = if d > 6.0 {
            if now >= path.next_replan || path.goal_cached.distance(yard) > 2.0 {
                path.waypoints = crate::navgrid::path_to_budget(v.pos, yard, NAV_NODES);
                path.cursor = 0;
                path.goal_cached = yard;
                path.next_replan = now + REPLAN_SECS + (self_e.to_bits() % 16) as f32 * 0.1;
            }
            while path.cursor < path.waypoints.len()
                && v.pos.distance(path.waypoints[path.cursor]) < 1.2
            {
                path.cursor += 1;
            }
            path.waypoints.get(path.cursor).copied().unwrap_or(yard)
        } else {
            path.waypoints.clear();
            path.cursor = 0;
            yard
        };
        let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        match steer::advance(v.pos, v.facing, step_target, v.speed * dt, v.body_r, cur_y, 3.0 * dt) {
            Some(s) if s.moving => {
                v.pos = s.pos;
                v.facing = s.facing;
                v.moving = true;
                carting.stall = 0.0;
            }
            _ => {
                v.moving = false;
                carting.stall += dt;
                if carting.stall > STALL_SECS {
                    // Wedged on the way home — drop the cached route and replan now.
                    path.waypoints.clear();
                    path.cursor = 0;
                    path.next_replan = now;
                    carting.stall = 0.0;
                }
            }
        }
        // Ground-follow + bob (this system owns the transform while the haul is on).
        let gy = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// The cart mesh/material set, built once and cached (every cart looks the same).
#[derive(Clone)]
struct CartAssets {
    bed_mesh: Handle<Mesh>,
    wheel_mesh: Handle<Mesh>,
    stone_mesh: Handle<Mesh>,
    wood_mat: Handle<StandardMaterial>,
    dark_mat: Handle<StandardMaterial>,
    stone_mat: Handle<StandardMaterial>,
}

impl CartAssets {
    fn build(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        Self {
            bed_mesh: meshes.add(Cuboid::new(0.6, 0.16, 0.84)),
            wheel_mesh: meshes.add(Cuboid::new(0.08, 0.4, 0.4)),
            stone_mesh: meshes.add(Cuboid::new(0.22, 0.2, 0.22)),
            wood_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.42, 0.28, 0.15),
                perceptual_roughness: 0.95,
                ..default()
            }),
            dark_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.18, 0.16, 0.14),
                perceptual_roughness: 0.9,
                ..default()
            }),
            stone_mat: materials.add(StandardMaterial {
                base_color: Color::srgb(0.6, 0.6, 0.66),
                perceptual_roughness: 0.95,
                ..default()
            }),
        }
    }
}

/// Put the visible loaded cart behind a fresh carter: a little plank wagon on two wheels with a
/// heap of grey stone, trailing the miner (−Z, the model's back). Mirror of `lumberjack::attach_log`.
fn attach_cart(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Option<CartAssets>>,
    mut fresh: Query<(Entity, &mut Carting), Added<Carting>>,
) {
    for (e, mut carting) in &mut fresh {
        if carting.cart.is_some() {
            continue;
        }
        let a = cache.get_or_insert_with(|| CartAssets::build(&mut meshes, &mut materials)).clone();
        let mut cart = Entity::PLACEHOLDER;
        commands.entity(e).with_children(|p| {
            cart = p
                .spawn((Transform::from_xyz(0.0, 0.0, -0.72), Visibility::Visible))
                .with_children(|c| {
                    c.spawn((
                        Mesh3d(a.bed_mesh.clone()),
                        MeshMaterial3d(a.wood_mat.clone()),
                        Transform::from_xyz(0.0, 0.34, 0.0),
                    ));
                    for wx in [-0.4_f32, 0.4] {
                        c.spawn((
                            Mesh3d(a.wheel_mesh.clone()),
                            MeshMaterial3d(a.dark_mat.clone()),
                            Transform::from_xyz(wx, 0.2, 0.0),
                        ));
                    }
                    // A small heap of stone riding the bed.
                    for (sx, sy, sz) in [(0.0, 0.52, 0.0), (-0.13, 0.6, -0.12), (0.14, 0.58, 0.1)] {
                        c.spawn((
                            Mesh3d(a.stone_mesh.clone()),
                            MeshMaterial3d(a.stone_mat.clone()),
                            Transform::from_xyz(sx, sy, sz),
                        ));
                    }
                })
                .id();
        });
        carting.cart = Some(cart);
    }
}

/// A carter mustered to guard duty at dusk dumps the load where it stands: the stone is banked on
/// the spot (don't strand a load on a soldier's back all night) and the cart mesh goes away before
/// the sword comes out. Mirror of `lumberjack::shed_log_at_muster`.
fn shed_cart_at_muster(
    mut commands: Commands,
    mut bank: ResMut<Bank>,
    mustered: Query<(Entity, &Carting), With<Guard>>,
) {
    for (e, carting) in &mustered {
        bank.0.add_stone(carting.amount);
        if let Some(cart) = carting.cart {
            commands.entity(cart).try_despawn();
        }
        commands.entity(e).try_remove::<Carting>();
    }
}
