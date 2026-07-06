//! Homing magic bolts — the ork shaman's ranged spell. A bolt tracks the hero's
//! live position, deals damage on arrival (via `PendingHeroDamage`, so a raised
//! shield blocks it), and fizzles after a short lifetime or once it has flown
//! its full range. Ported from the original game's `projectileStore.ts`.
//!
//! Also home to the town archers' **arrows** — the friendly mirror of the bolt, but ballistic
//! (a real launch velocity + gravity arc, not homing): the archer brain (`villagers.rs`) pushes
//! an [`ArrowSpawn`] at the release frame of its draw clip, the shaft flies its arc, and on
//! impact damages hostile `Health` (orks / predators / rival soldiers) the same way a guard's
//! sword blow does. A missed shaft sticks in the turf for a beat, then fades.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::player::{spawn_burst, CombatFx, HeroState, PendingHeroDamage};

/// A bolt within this distance of its target counts as a hit.
pub(crate) const BOLT_HIT_RADIUS: f32 = 0.6;

/// Bolt flight speed (world units/sec).
const BOLT_SPEED: f32 = 9.0;
/// Seconds a bolt lives before it fizzles regardless of distance.
const BOLT_TTL: f32 = 3.0;
/// Distance a bolt may fly before fizzling short of a fleeing target.
const BOLT_MAX_RANGE: f32 = 16.0;

/// Outcome of advancing a bolt one frame.
#[derive(Debug, PartialEq)]
pub(crate) enum BoltStep {
    /// Still flying — new world position.
    Fly(Vec3),
    /// Reached the target — deal damage.
    Hit,
    /// Flew its full range without connecting — despawn.
    Fizzle,
}

/// Advance a bolt one frame toward `target`, moving `step` units. Returns the
/// outcome and the updated travelled distance.
pub(crate) fn advance_bolt(
    pos: Vec3,
    target: Vec3,
    step: f32,
    traveled: f32,
    max_range: f32,
) -> (BoltStep, f32) {
    let to = target - pos;
    let len = to.length();
    if len < BOLT_HIT_RADIUS {
        return (BoltStep::Hit, traveled);
    }
    let nt = traveled + step;
    if nt >= max_range {
        return (BoltStep::Fizzle, nt);
    }
    (BoltStep::Fly(pos + to / len.max(1e-6) * step), nt)
}

/// One bolt the shaman wants spawned this frame.
pub struct BoltSpawn {
    pub origin: Vec3,
    pub damage: f32,
}

/// Spawn queue — `orks.rs` pushes, `spawn_queued_bolts` drains. Mirrors the
/// `PendingHeroDamage` channel idiom (no `Commands` needed in the ork brain).
#[derive(Resource, Default)]
pub struct BoltSpawns(pub Vec<BoltSpawn>);

/// A live homing bolt flying at the hero.
#[derive(Component)]
pub(crate) struct Bolt {
    damage: f32,
    speed: f32,
    ttl: f32,
    traveled: f32,
    max_range: f32,
}

/// Shared bolt mesh + glowing purple material, built once.
#[derive(Resource)]
struct BoltAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

fn setup_bolt_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.16).mesh().ico(2).unwrap());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.61, 1.0),
        emissive: LinearRgba::rgb(2.4, 1.4, 4.0),
        unlit: true,
        ..default()
    });
    commands.insert_resource(BoltAssets { mesh, mat });
}

fn spawn_queued_bolts(
    mut commands: Commands,
    assets: Res<BoltAssets>,
    mut spawns: ResMut<BoltSpawns>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
) {
    for s in spawns.0.drain(..) {
        // The staff-release crack (`warp-cast.ogg`), spatial at the shaman's hands.
        cues.write(crate::audio::AudioCue::WarpCast(s.origin));
        commands.spawn((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.mat.clone()),
            Transform::from_translation(s.origin),
            Bolt {
                damage: s.damage,
                speed: BOLT_SPEED,
                ttl: BOLT_TTL,
                traveled: 0.0,
                max_range: BOLT_MAX_RANGE,
            },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
}

fn step_bolts(
    time: Res<Time>,
    hero: Res<HeroState>,
    siege: Res<crate::siege::Siege>,
    fx: Option<Res<CombatFx>>,
    mut pending: ResMut<PendingHeroDamage>,
    mut marks: MessageWriter<crate::aftermath::BattleMark>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Bolt, &mut Transform)>,
    mut prev_phase: Local<Option<crate::siege::GamePhase>>,
) {
    let dt = time.delta_secs().min(0.05);
    let target = Vec3::new(hero.pos.x, hero.y + 1.0, hero.pos.y);
    // A bolt still in flight when a night wave CLEARS must not deal damage into the won daytime —
    // sweep every live bolt on that Wave→day edge only (the keep/invaders clear on the same edge).
    // This must NOT fire every daytime frame: wilderness camp shamans cast during the day too, and
    // despawning their bolts the instant they spawned made those bolts invisible ("szamani strzelają
    // niewidzialnymi kulami"). Daytime bolts now fly normally and expire on their own TTL/range.
    let cur_phase = siege.phase;
    let was = prev_phase.replace(cur_phase);
    let wave_cleared =
        was == Some(crate::siege::GamePhase::Wave) && cur_phase != crate::siege::GamePhase::Wave;
    for (e, mut b, mut tf) in &mut q {
        b.ttl -= dt;
        if !hero.alive || b.ttl <= 0.0 || wave_cleared {
            commands.entity(e).try_despawn();
            continue;
        }
        let (out, traveled) =
            advance_bolt(tf.translation, target, b.speed * dt, b.traveled, b.max_range);
        b.traveled = traveled;
        match out {
            BoltStep::Fly(p) => tf.translation = p,
            BoltStep::Hit => {
                pending.0 += b.damage;
                // Bolt travel direction (bolt → hero, XZ) so the burst shoves the camera along it.
                pending.1 = (Vec2::new(target.x, target.z)
                    - Vec2::new(tf.translation.x, tf.translation.z))
                .normalize_or_zero();
                if let Some(fx) = &fx {
                    spawn_burst(&mut commands, fx, tf.translation, false);
                }
                // Leave a scorch on the turf where the bolt burst (aftermath.rs).
                marks.write(crate::aftermath::BattleMark { at: tf.translation });
                commands.entity(e).try_despawn();
            }
            BoltStep::Fizzle => commands.entity(e).try_despawn(),
        }
    }
}

// ── Arrows (the town archers' ballistic shafts) ─────────────────────────────────────────

/// Arrow muzzle speed (world units/sec) — quick enough to lead-shoot a shambling ork, slow enough
/// that the arc and the shaft itself read in flight.
pub(crate) const ARROW_SPEED: f32 = 26.0;
/// Arrow gravity — gamey-light (real 9.8 at this speed reads near-flat), so a long shot lofts into
/// a visible archer's arc.
pub(crate) const ARROW_GRAV: f32 = 12.0;
/// Within this of a hostile's chest, the shaft connects.
const ARROW_HIT_RADIUS: f32 = 0.8;
/// Seconds a missed shaft stays stuck in the turf before it fades.
const ARROW_STICK_SECS: f32 = 2.4;
/// Hard flight cap so a shaft launched off a cliff edge can't rain forever.
const ARROW_TTL: f32 = 5.0;

/// The launch velocity that carries a shaft from `from` to `aim` in `dist/speed` seconds under
/// `grav` — flat and fast up close, a visible loft at range. Pure, unit-tested below.
pub(crate) fn arrow_launch(from: Vec3, aim: Vec3, speed: f32, grav: f32) -> Vec3 {
    let d = aim - from;
    let t = (d.length() / speed.max(1e-3)).clamp(0.12, 1.4);
    Vec3::new(d.x / t, d.y / t + 0.5 * grav * t, d.z / t)
}

/// One arrow an archer looses this frame — pushed by the archer brain at the draw clip's release
/// moment (`villagers::guard_combat`), drained by [`spawn_queued_arrows`]. Same channel idiom as
/// [`BoltSpawns`].
pub struct ArrowSpawn {
    /// Bow position at release (world; roughly the archer's chest, a step toward the target).
    pub from: Vec3,
    /// Where the shaft is aimed (the target's chest at release — a lead is not needed at militia
    /// ranges, misses on a sidestepping foe are honest archery).
    pub aim: Vec3,
    /// The foe it was loosed at — used for the tight hit test while it flies.
    pub target: Entity,
    /// The archer, so a struck beast enrages at the right assailant.
    pub shooter: Entity,
    pub damage: f32,
    /// Loosed by the RIVAL's desert bowmen (`rival.rs`): the shaft carries crimson fletchings and
    /// hits the OTHER side — the hero (via `PendingHeroDamage`, so a raised shield blocks it like a
    /// shaman bolt) and the player's townsfolk (via the `NpcDamage` channel, like a rival blade) —
    /// instead of the friendly arrows' ork/predator/rival hostile set.
    pub rival: bool,
}

#[derive(Resource, Default)]
pub struct ArrowSpawns(pub Vec<ArrowSpawn>);

/// A shaft in flight (or stuck in the turf while `stuck_at >= 0`).
#[derive(Component)]
struct Arrow {
    vel: Vec3,
    damage: f32,
    target: Entity,
    shooter: Entity,
    ttl: f32,
    /// `elapsed_secs` when it hit the ground; `< 0` while still flying.
    stuck_at: f32,
    /// A rival bowman's shaft — hits the hero/townsfolk instead of the hostile set (see [`ArrowSpawn::rival`]).
    rival: bool,
}

/// Shared arrow mesh (one merged flat-shaded shaft, vertex-coloured) + plain white material,
/// built once. Authored with the POINT toward -Z so `Transform::looking_to(vel)` flies it
/// point-first.
#[derive(Resource)]
struct ArrowAssets {
    mesh: Handle<Mesh>,
    /// The rival bowmen's shaft — same build, crimson vanes (their faction dye).
    mesh_rival: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

fn setup_arrow_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    use crate::palette::lin;
    let tint = |mut m: Mesh, c: u32| -> Mesh {
        let n = m.count_vertices();
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(c); n]);
        m
    };
    // Same kit palette as the archer model (`peasant_model.rs`): yew shaft, iron point, faction-dyed
    // vanes (militia blue FLETCH / the rival's crimson DESERT_FLETCH — keep in sync with that file).
    let build = |vane: u32| -> Mesh {
        let mut m = tint(Cuboid::new(0.045, 0.045, 0.68).mesh().build(), 0x4f3c26); // shaft
        let head = tint(
            Cone { radius: 0.045, height: 0.13 }
                .mesh()
                .build()
                .rotated_by(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)) // +Y point → -Z
                .translated_by(Vec3::new(0.0, 0.0, -0.38)),
            0xcfd3dc,
        );
        let vane_a = tint(Cuboid::new(0.13, 0.02, 0.16).mesh().build().translated_by(Vec3::new(0.0, 0.0, 0.27)), vane);
        let vane_b = tint(Cuboid::new(0.02, 0.13, 0.16).mesh().build().translated_by(Vec3::new(0.0, 0.0, 0.27)), vane);
        for part in [head, vane_a, vane_b] {
            m.merge(&part).expect("arrow parts share attributes");
        }
        m.duplicate_vertices();
        m.compute_flat_normals();
        m
    };
    commands.insert_resource(ArrowAssets {
        mesh: meshes.add(build(0x3f5f9e)),
        mesh_rival: meshes.add(build(0xc23a2e)),
        mat: materials.add(StandardMaterial::default()), // white; colour rides the vertices
    });
}

fn spawn_queued_arrows(
    mut commands: Commands,
    assets: Res<ArrowAssets>,
    mut spawns: ResMut<ArrowSpawns>,
) {
    for s in spawns.0.drain(..) {
        let vel = arrow_launch(s.from, s.aim, ARROW_SPEED, ARROW_GRAV);
        let mesh = if s.rival { assets.mesh_rival.clone() } else { assets.mesh.clone() };
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(assets.mat.clone()),
            Transform::from_translation(s.from).looking_to(vel.normalize_or_zero(), Vec3::Y),
            Arrow { vel, damage: s.damage, target: s.target, shooter: s.shooter, ttl: ARROW_TTL, stuck_at: -1.0, rival: s.rival },
            BiomeEntity,
        ));
    }
}

/// Fly every arrow along its ballistic arc, point-first: hit the aimed foe (or any hostile the
/// shaft happens to pass through), damage its `Health` exactly like a guard's sword blow — kill →
/// death-fade toppling along the shaft's line, struck beast → enrage at the archer — or stick in
/// the turf on a miss and fade after a beat. A RIVAL bowman's shaft flies the same arc but hits
/// the other side: the hero (blockable `PendingHeroDamage`, like a shaman bolt) and the player's
/// townsfolk (the `NpcDamage` channel, like a rival blade).
#[allow(clippy::type_complexity)]
fn step_arrows(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut pending: ResMut<PendingHeroDamage>,
    mut npc_dmg: ResMut<crate::villagers::NpcDamage>,
    mut commands: Commands,
    mut kills: MessageWriter<crate::verbs::AnimalKilled>,
    mut arrows: Query<(Entity, &mut Arrow, &mut Transform)>,
    mut hostiles: Query<
        (Entity, &Transform, &mut crate::player::Health, Option<&crate::wildlife::Animal>),
        (
            // The same hostile set the guard melee engages (`villagers::guard_combat`).
            Or<(
                With<crate::orks::Ork>,
                With<crate::wildlife::Animal>,
                With<crate::warlord::Warlord>,
                With<crate::rival::RivalSoldier>,
            )>,
            Without<Arrow>,
            Without<crate::dying::Dying>,
        ),
    >,
    // The RIVAL arrows' targets — the player's town pool (guards + workers; damage goes through
    // the NpcDamage channel, so their NpcHp/fight-back plumbing reacts like to any rival blade).
    folk: Query<
        (Entity, &Transform),
        (With<crate::villagers::Townsfolk>, Without<Arrow>, Without<crate::dying::Dying>),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    for (arrow_e, mut a, mut tf) in &mut arrows {
        a.ttl -= dt;
        if a.ttl <= 0.0 || (a.stuck_at >= 0.0 && now - a.stuck_at > ARROW_STICK_SECS) {
            commands.entity(arrow_e).try_despawn();
            continue;
        }
        if a.stuck_at >= 0.0 {
            continue; // planted in the turf, waiting out its fade timer
        }
        a.vel.y -= ARROW_GRAV * dt;
        tf.translation += a.vel * dt;
        let dir = a.vel.normalize_or_zero();
        tf.look_to(dir, Vec3::Y);

        // A rival shaft tests OUR side (hero chest + townsfolk) and never the hostile set — the
        // rival doesn't fight the orks, and its own garrison must not catch friendly fire.
        if a.rival {
            let mut struck = false;
            let hero_chest = Vec3::new(hero.pos.x, hero.y + 1.0, hero.pos.y);
            if hero.alive && hero_chest.distance(tf.translation) < ARROW_HIT_RADIUS {
                pending.0 += a.damage;
                pending.1 = Vec2::new(dir.x, dir.z).normalize_or_zero(); // directional hit-shake
                struck = true;
            }
            if !struck {
                // Aimed townsperson gets the full radius; any other a slightly tighter one.
                let mut hit: Option<Entity> = None;
                if let Ok((te, ttf)) = folk.get(a.target) {
                    if (ttf.translation + Vec3::Y).distance(tf.translation) < ARROW_HIT_RADIUS {
                        hit = Some(te);
                    }
                }
                if hit.is_none() {
                    for (fe, ftf) in folk.iter() {
                        if (ftf.translation + Vec3::Y).distance(tf.translation) < ARROW_HIT_RADIUS * 0.8 {
                            hit = Some(fe);
                            break;
                        }
                    }
                }
                if let Some(fe) = hit {
                    // `attacker: Some(shooter)` so a struck worker fights back against the bowman
                    // like it does any rival blade (`npc_fight_back`).
                    npc_dmg.0.push(crate::villagers::NpcHit { victim: fe, amount: a.damage, attacker: Some(a.shooter) });
                    struck = true;
                }
            }
            if struck {
                commands.entity(arrow_e).try_despawn();
                continue;
            }
            // Miss → fall through to the shared ground-stick below.
        } else {
            // Hit test: the aimed target gets the full radius; any other hostile the shaft passes
            // through connects on a slightly tighter one (a volley into a horde lands *somewhere*).
            let mut hit: Option<Entity> = None;
            if let Ok((te, ttf, ..)) = hostiles.get(a.target) {
                if (ttf.translation + Vec3::Y).distance(tf.translation) < ARROW_HIT_RADIUS {
                    hit = Some(te);
                }
            }
            if hit.is_none() {
                for (he, htf, ..) in hostiles.iter() {
                    if (htf.translation + Vec3::Y).distance(tf.translation) < ARROW_HIT_RADIUS * 0.8 {
                        hit = Some(he);
                        break;
                    }
                }
            }
            if let Some(he) = hit {
                if let Ok((_, htf, mut hp, animal)) = hostiles.get_mut(he) {
                    if hp.hp > 0.0 {
                        hp.hp -= a.damage;
                        // Light struck-feedback (no camera punch — it's not the hero's blow): the
                        // target blinks + squashes so a landed shaft visibly *thuds* home.
                        commands.entity(he).try_insert(crate::combat_fx::HurtFlash::new(now, 0.4));
                        commands.entity(he).try_insert(crate::combat_fx::HitSquash::new(now, 0.09, false));
                        if hp.hp <= 0.0 {
                            // Topple the kill along the shaft's line — it falls the way it was shot.
                            crate::dying::begin_dying_struck(&mut commands, he, now, Vec2::new(dir.x, dir.z), false);
                            if let Some(an) = animal {
                                kills.write(crate::verbs::AnimalKilled { at: htf.translation, species: an.species });
                            }
                        } else if animal.is_some() {
                            commands.entity(he).try_insert(crate::wildlife::Struck { by: Some(a.shooter) });
                        }
                    }
                }
                commands.entity(arrow_e).try_despawn();
                continue;
            }
        }

        // Ground: plant the shaft point-first where it lands and let it fade out.
        let gy = crate::worldmap::ground_at_world(tf.translation.x, tf.translation.z).unwrap_or(f32::MIN);
        if tf.translation.y <= gy + 0.06 {
            tf.translation.y = gy + 0.06;
            a.stuck_at = now;
        }
    }
}

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoltSpawns>()
            .init_resource::<ArrowSpawns>()
            .add_systems(Startup, (setup_bolt_assets, setup_arrow_assets))
            .add_systems(
                Update,
                (spawn_queued_bolts, step_bolts, spawn_queued_arrows, step_arrows)
                    .chain()
                    .run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homes_closer_each_step() {
        let (out, tr) = advance_bolt(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 1.0, 0.0, 40.0);
        assert_eq!(out, BoltStep::Fly(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(tr, 1.0);
    }

    #[test]
    fn hits_when_within_radius() {
        let (out, _) = advance_bolt(Vec3::ZERO, Vec3::new(0.3, 0.0, 0.0), 1.0, 0.0, 40.0);
        assert_eq!(out, BoltStep::Hit);
    }

    #[test]
    fn fizzles_past_max_range() {
        let (out, tr) = advance_bolt(Vec3::ZERO, Vec3::new(50.0, 0.0, 0.0), 2.0, 39.0, 40.0);
        assert_eq!(out, BoltStep::Fizzle);
        assert_eq!(tr, 41.0);
    }

    /// Integrating the launch velocity under the same gravity must land the shaft on the aim
    /// point (the arc is exact for the unclamped flight time).
    #[test]
    fn arrow_arc_lands_on_aim() {
        let from = Vec3::new(0.0, 1.4, 0.0);
        let aim = Vec3::new(10.0, 1.0, 6.0);
        let vel = arrow_launch(from, aim, ARROW_SPEED, ARROW_GRAV);
        let t = (aim - from).length() / ARROW_SPEED;
        // Closed-form position at time t: p = from + vel·t − ½·g·t² ŷ.
        let p = from + vel * t - Vec3::Y * 0.5 * ARROW_GRAV * t * t;
        assert!(p.distance(aim) < 1e-4, "landed {p:?}, wanted {aim:?}");
    }

    /// A long shot must actually LOFT — the launch pitch rises above the straight line to the aim.
    #[test]
    fn arrow_long_shot_lofts() {
        let from = Vec3::new(0.0, 1.4, 0.0);
        let aim = Vec3::new(20.0, 1.4, 0.0);
        let vel = arrow_launch(from, aim, ARROW_SPEED, ARROW_GRAV);
        assert!(vel.y > 0.5, "expected an upward loft, got vel.y = {}", vel.y);
    }
}
