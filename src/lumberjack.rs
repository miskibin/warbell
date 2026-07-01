//! **Woodcutters work the woods for real — and the woods are the town's ONLY wood income.**
//! The Lumber plot has no passive trickle (core `BuildKind::produces` → `None` for Lumber);
//! instead its worker walks out to an actual scattered tree, chops it down (the tree topples
//! and regrows later), shoulders the log ([`Hauling`]), carries it back to the yard, and only
//! THERE the wood is banked. Lose the woodcutter on the road and you lose the log.
//!
//! Two hard rails keep this from feeding the orks an endless villager buffet:
//!   * **Safe ground only** — a tree is workable only inside [`WORK_R`] of the castle and at
//!     least [`LIVE_CAMP_AVOID`] from every *occupied* ork camp (and never inside a camp
//!     clearing), so a woodcutter never wanders into a standing warband on its own. A camp whose
//!     warband you've wiped stops pushing the cutter away — its grove opens up for cutting.
//!   * **Threat sense** — any ork or predator inside [`DANGER_R`] sends the woodcutter (working
//!     or hauling) running home ([`Fleeing`]) and blacklists that ground for [`DANGER_TTL`]
//!     seconds ([`DangerSpots`]), so it does NOT trudge straight back under the same shaman's
//!     bolt.
//!
//! While a woodcutter is actually at a tree it counts as `at_post` (the plot reads as staffed
//! in the build UI). If it's caught anyway, the shared villager self-defence
//! (`villagers::FightBack`) takes over — it drops the flight and fights, then resumes the haul.

use bevy::prelude::*;

use tileworld_core::town_store::BuildKind;

use crate::economy::Bank;
use crate::steer;
use crate::town::Worker;
use crate::villagers::{FightBack, Guard, Townsfolk, Villager};

/// Trees are only worked this close to the castle origin (the safe heart of the island)…
const WORK_R: f32 = 45.0;
/// …and no closer than this to a *live* ork camp's centre. Sized to clear the warband's leash
/// (`orks` `ORK_LEASH` 16) PLUS the cutter's own threat sense ([`DANGER_R`] 12), so a working
/// cutter sits outside any leashed camp ork's reach — otherwise it'd be scared off (and blacklist
/// its own grove) by orks it can never actually escape. A *cleared* camp stops pushing the cutter
/// away entirely (see [`assign_tree`]), so wiping a warband opens its grove for cutting.
const LIVE_CAMP_AVOID: f32 = 30.0;
/// A hostile (ork / predator) this near a woodcutter triggers the flight home.
const DANGER_R: f32 = 12.0;
/// Trees this near a remembered scare are off-limits while it lasts.
const DANGER_BLACKLIST_R: f32 = 16.0;
/// How long a scare keeps its ground blacklisted (s). Kept short: a real threat that lingers
/// re-trips the scare and re-blacklists, but a transient predator/ork fly-by shouldn't sideline a
/// whole grove for minutes (the old 120s did, which read as the woodcutter "refusing to work").
const DANGER_TTL: f32 = 45.0;
/// Axe damage per work swing — scaled with TREE_HP's ×3 bump (TREE_HP 165 → ~5 swings ≈ 10s a
/// tree), so the town's wood income keeps its old pace even though the hero now needs 3× the hits.
const CHOP_DMG: f64 = 36.0;
/// Seconds between work swings — matches the overhead chop loop in `villager_limbs` (~2.1s).
const CHOP_CD: f32 = 2.1;
/// Pad past the trunk surface (its blocker radius + the cutter's body radius) at which the axe
/// can land. Small on purpose — the cutter should stand AT the bark, not an axe-handle off it —
/// but it must leave a little slop over where the trunk blocker parks the body, or "arrived"
/// never trips and the cutter just orbits the tree.
const CHOP_REACH_PAD: f32 = 0.45;
/// Don't rescan the whole tree set every frame when there's nothing eligible.
const RETRY_SECS: f32 = 3.0;
/// A flight home gives up after this long even if the walls weren't reached.
const FLEE_SECS: f32 = 9.0;
/// A hauler making no real progress home for this long (across however many replans) is treated
/// as truly wedged: it banks the log on the spot and frees up, rather than looping replans forever
/// in a dead corner. Generous vs [`STALL_SECS`] so a normal jam (one replan past a prop) recovers
/// without ever tripping this.
const HAUL_GIVEUP_SECS: f32 = 12.0;
/// Reaching this close to the castle origin counts as "safe — stop running".
const FLEE_HOME_R: f32 = 14.0;
/// No steering progress for this long → wedged; [`haul_home`] uses it to force a replan on the
/// way back to the yard.
const STALL_SECS: f32 = 4.0;
/// No steering progress toward the tree for this long while FOLLOWING the A* route → the route
/// has a local wedge (a courtyard corner, a gate lip). Re-path from here rather than abandon —
/// the goal is A*-reachable (assignment checks), so the wedge is on the road, not at the tree.
const STALL_REPLAN_SECS: f32 = 1.5;
/// Total time genuinely wedged (NOT reset by a replan, only by real forward progress). Past this
/// the cutter abandons the job and briefly shuns the spot — a reactive trap A* keeps routing back
/// into. Generous so a normal jam (one replan past a prop) recovers without ever tripping it.
const WEDGE_GIVEUP_SECS: f32 = 12.0;
/// How many nearest candidate trees `assign_tree` A*-probes for reachability before giving up —
/// bounds the pathfinding cost while still skipping past a cluster of walled-off trees.
const REACH_CHECK_K: usize = 8;
/// Inside this ring a worker just direct-steers, so [`pick_nearest_reachable`] takes the target
/// without paying for an A* reachability probe.
pub(crate) const CLOSE_RING: f32 = 6.0;
/// Time allowed inside the direct-steer ring (≤6u of the tree) without actually reaching it.
/// A reachable tree is reached from 6u out in ~3–4s; a tree up a terrace lip keeps the cutter
/// wall-following along the cliff face — `moving` stays true, so [`STALL_SECS`] never trips and
/// he paces under the unreachable tree forever. This caps that: bail + blacklist, pick another.
const CLOSE_GIVEUP_SECS: f32 = 8.0;
/// Chop SFX is only audible this near the hero (the guards' small-earshot convention).
const SFX_EARSHOT: f32 = 16.0;
/// Close enough to the yard to dump the log on the pile (matches `worker_steer`'s post reach).
const HAUL_REACH: f32 = 1.8;

/// The tree a woodcutter is working: walk to it, swing on the cooldown, fell it, pick the next.
#[derive(Component)]
pub struct ChopJob {
    tree: Entity,
    atk_cd: f32,
    /// Seconds without steering progress since the last replan — re-path at [`STALL_REPLAN_SECS`].
    stall: f32,
    /// Total time wedged, reset only by real forward progress — abandon at [`WEDGE_GIVEUP_SECS`].
    stuck: f32,
    /// Seconds spent inside the direct-steer ring without reaching the tree — catches cliff
    /// pacing under a tree up a terrace lip; bail at [`CLOSE_GIVEUP_SECS`].
    close: f32,
}

/// A felled log on the woodcutter's shoulder: walk it back to the Lumber yard — the wood is
/// banked only on arrival ([`haul_home`]). `log` is the visible carried-log child mesh.
#[derive(Component)]
pub struct Hauling {
    amount: f64,
    log: Option<Entity>,
    /// Seconds without steering progress on the way home — force a replan past [`STALL_SECS`].
    stall: f32,
    /// Total time genuinely wedged, NOT reset by a replan (only by real forward progress). Past
    /// [`HAUL_GIVEUP_SECS`] the hauler dumps the log where it stands and frees up — a worker
    /// pinned in a corner A* can't route out of would otherwise loop replans forever and read as
    /// "stuck holding wood".
    stuck: f32,
}

/// A working NPC running for the castle after its threat sense fired (or it was attacked and
/// the brawl broke off). Removed on reaching home ground or after [`FLEE_SECS`].
#[derive(Component)]
pub struct Fleeing {
    pub(crate) until: f32,
}

/// Remembered scares: `(world XZ, expires_at)`. Trees near one are skipped while it lasts —
/// the cap on the "ork farms the same woodcutter forever" loop. Shared with the stone miner
/// (`miner.rs`), which pushes/reads the same blacklist (the two trades work disjoint ground, so
/// one shared list is harmless and keeps a single source of remembered danger).
#[derive(Resource, Default)]
pub(crate) struct DangerSpots(pub(crate) Vec<(Vec2, f32)>);

pub struct LumberjackPlugin;

impl Plugin for LumberjackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DangerSpots>().add_systems(
            Update,
            (lumber_danger, assign_tree, chop_work, haul_home, attach_log, shed_log_at_muster, flee_steer)
                .run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

/// Threat sense: an ork or a predator prowling inside [`DANGER_R`] of a working (or hauling)
/// woodcutter sends it running home and blacklists the spot. Also expires old scares. A scared
/// hauler keeps the log — the flight ends at the walls and [`haul_home`] finishes the delivery.
#[allow(clippy::type_complexity)]
fn lumber_danger(
    time: Res<Time>,
    mut danger: ResMut<DangerSpots>,
    mut commands: Commands,
    workers: Query<
        (Entity, &Villager),
        (
            With<Worker>,
            Or<(With<ChopJob>, With<Hauling>)>,
            Without<Fleeing>,
            Without<crate::dying::Dying>,
        ),
    >,
    orks: Query<&crate::orks::Ork, Without<crate::dying::Dying>>,
    animals: Query<&crate::wildlife::Animal, Without<crate::dying::Dying>>,
) {
    let now = time.elapsed_secs();
    danger.0.retain(|(_, until)| now < *until);
    if workers.is_empty() {
        return;
    }
    let mut hostiles: Vec<Vec2> = orks.iter().map(|o| o.pos).collect();
    hostiles.extend(
        animals.iter().filter(|a| crate::wildlife::is_hostile_species(a.species)).map(|a| a.pos),
    );
    for (e, v) in &workers {
        if let Some(hp) = hostiles.iter().find(|h| h.distance(v.pos) < DANGER_R) {
            commands.entity(e).try_remove::<ChopJob>().try_insert(Fleeing { until: now + FLEE_SECS });
            danger.0.push((*hp, now + DANGER_TTL));
        }
    }
}

/// From a distance-tagged candidate list `(entity, pos, dist-from-worker)`, pick the nearest one
/// the worker can actually WALK to: sort ascending by distance, then probe up to `k` candidates and
/// return the first that's either inside the close direct-steer ring ([`CLOSE_RING`]) or
/// `reachable` (an A* path exists). Picking the Euclidean-nearest blindly is what marched workers at
/// targets across a stream / up a cliff, where they wedged, abandoned, and re-picked the next
/// equally-unreachable target — reading as "standing still". The probe is capped because A* over
/// the island isn't free. Shared by `assign_tree`/`assign_ore`.
pub(crate) fn pick_nearest_reachable(
    cands: &mut [(Entity, Vec2, f32)],
    k: usize,
    reachable: impl Fn(Vec2) -> bool,
) -> Option<Entity> {
    cands.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    cands
        .iter()
        .take(k)
        .find(|(_, p, d)| *d <= CLOSE_RING || reachable(*p))
        .map(|(e, _, _)| *e)
}

/// Hand each idle Lumber-plot worker the nearest workable tree (safe ground, not blacklisted).
/// Throttled to every [`RETRY_SECS`] — the tree set is large and assignment isn't urgent.
#[allow(clippy::type_complexity)]
fn assign_tree(
    time: Res<Time>,
    town: Res<crate::town::TownRes>,
    danger: Res<DangerSpots>,
    mut retry_at: Local<f32>,
    mut commands: Commands,
    workers: Query<
        (Entity, &Worker, &Villager),
        (
            With<Townsfolk>,
            Without<ChopJob>,
            Without<Hauling>,
            Without<Fleeing>,
            Without<FightBack>,
            Without<crate::dying::Dying>,
        ),
    >,
    trees: Query<(Entity, &Transform), (With<crate::verbs::ChopTree>, Without<crate::verbs::Stump>)>,
    orks: Query<&crate::orks::Ork, (Without<crate::orks::WaveInvader>, Without<crate::dying::Dying>)>,
) {
    let now = time.elapsed_secs();
    if now < *retry_at {
        return;
    }
    *retry_at = now + RETRY_SECS;
    // Only camps that STILL have a standing warband push the cutter away. A camp ork is
    // home-anchored to its camp centre (`orks::Ork::home`), so a centre with no living ork homed
    // to it is a cleared camp — its grove is safe to cut (matches the player's intuition: wipe the
    // warband and the woodcutter starts working that wood).
    let live_camps: Vec<Vec2> = crate::camps::cage_positions()
        .iter()
        .map(|(_, c)| *c)
        .filter(|c| orks.iter().any(|o| o.home().distance(*c) < 1.0))
        .collect();
    for (e, worker, v) in &workers {
        if town.0.plots.get(worker.idx).and_then(|p| p.kind) != Some(BuildKind::Lumber) {
            continue; // farmers keep their field mime — only woodcutters roam
        }
        // Gather every eligible tree (safe ground, not blacklisted), nearest first.
        let mut cands: Vec<(Entity, Vec2, f32)> = trees
            .iter()
            .filter_map(|(te, tf)| {
                let tp = Vec2::new(tf.translation.x, tf.translation.z);
                if tp.length() > WORK_R
                    || crate::camps::in_clearing(tp.x, tp.y)
                    || live_camps.iter().any(|c| c.distance(tp) < LIVE_CAMP_AVOID)
                    || danger.0.iter().any(|(d, _)| d.distance(tp) < DANGER_BLACKLIST_R)
                {
                    return None;
                }
                Some((te, tp, v.pos.distance(tp)))
            })
            .collect();
        let from = v.pos;
        let chosen = pick_nearest_reachable(&mut cands, REACH_CHECK_K, |tp| {
            !crate::navgrid::path_to(from, tp).is_empty()
        });
        if let Some(te) = chosen {
            commands
                .entity(e)
                .try_insert(ChopJob { tree: te, atk_cd: 0.0, stall: 0.0, stuck: 0.0, close: 0.0 });
        }
    }
}

/// Walk the woodcutter to its tree and swing the axe on the cooldown; the last blow topples the
/// tree and shoulders the log ([`Hauling`] — NO wood is banked here; that happens back at the
/// yard in [`haul_home`]). At the tree it counts `at_post` and the overhead-chop work loop in
/// `villager_limbs` plays for free.
#[allow(clippy::type_complexity)]
fn chop_work(
    time: Res<Time>,
    mut commands: Commands,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut danger: ResMut<DangerSpots>,
    hero: Res<crate::player::HeroState>,
    tree_fx: Option<Res<crate::verbs::TreeFx>>,
    mut workers: Query<
        (Entity, &mut ChopJob, &mut Worker, &mut Villager, &mut Transform, &mut crate::navgrid::NavPath),
        (Without<Hauling>, Without<Fleeing>, Without<FightBack>, Without<crate::dying::Dying>),
    >,
    mut trees: Query<
        (&mut crate::verbs::ChopTree, &Transform),
        (Without<crate::verbs::Stump>, Without<Worker>),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (self_e, mut job, mut worker, mut v, mut tf, mut path) in &mut workers {
        let Ok((mut tree, ttf)) = trees.get_mut(job.tree) else {
            // Felled (stumped) or gone — back to the pool; `assign_tree` hands out the next one.
            commands.entity(self_e).try_remove::<ChopJob>();
            continue;
        };
        // Already dropped to 0 HP by another cutter (or the hero) THIS frame — the `Stump` insert is
        // deferred, so the tree is still queryable here. Bail before swinging, or this worker fells
        // the same tree a second time and banks a second unearned log (mirrors `miner::pick_work`).
        if tree.felled() {
            commands.entity(self_e).try_remove::<ChopJob>();
            continue;
        }
        let tp = Vec2::new(ttf.translation.x, ttf.translation.z);
        let d = v.pos.distance(tp);
        // Reach is sized to THIS tree's trunk (thin sapling vs fat bole) so the cutter steps right
        // up to the bark either way, instead of a one-size-fits-all arm's length off the trunk.
        let reach = tree.trunk_r() + v.body_r + CHOP_REACH_PAD;
        job.atk_cd -= dt;
        if d <= reach {
            // At the tree: face it, plant the feet, swing on the cooldown.
            worker.at_post = true;
            v.moving = false;
            job.stall = 0.0;
            job.close = 0.0;
            let to = tp - v.pos;
            if to.length_squared() > 1e-4 {
                v.facing = to.x.atan2(to.y);
            }
            if job.atk_cd <= 0.0 {
                job.atk_cd = CHOP_CD;
                if hero.pos.distance(v.pos) < SFX_EARSHOT {
                    cues.write(crate::audio::AudioCue::WoodChop);
                }
                let dir = (tp - v.pos).normalize_or_zero();
                if tree.work_chop(CHOP_DMG) {
                    // Timber — but no wood yet: shoulder the log and carry it home. Clear
                    // `at_post` or `villager_limbs` keeps the chop stroke going on the walk.
                    worker.at_post = false;
                    crate::verbs::topple_tree(&mut commands, job.tree, ttf.translation, dir, now);
                    commands
                        .entity(self_e)
                        .try_remove::<ChopJob>()
                        .try_insert(Hauling {
                            amount: crate::verbs::TREE_WOOD,
                            log: None,
                            stall: 0.0,
                            stuck: 0.0,
                        });
                } else {
                    // The same chop juice the hero's swings get: the trunk shudders under the
                    // axe and sheds chips/leaves (visual-only, so no earshot gate).
                    commands
                        .entity(job.tree)
                        .try_insert(crate::verbs::TrunkShake::new(now, dir));
                    if let Some(fxa) = &tree_fx {
                        crate::verbs::chop_burst(&mut commands, fxa, ttf.translation, dir);
                    }
                }
            }
        } else {
            // March to the tree: A* when far (thread the gates/river), direct steer when close.
            worker.at_post = false;
            let step_target = if d > 6.0 {
                job.close = 0.0;
                if path.cursor >= path.waypoints.len()
                    || now >= path.next_replan
                    || path.goal_cached.distance(tp) > 2.0
                {
                    path.waypoints = crate::navgrid::path_to(v.pos, tp);
                    path.cursor = 0;
                    path.goal_cached = tp;
                    path.next_replan = now + 1.0 + (self_e.to_bits() % 16) as f32 * 0.05;
                }
                while path.cursor < path.waypoints.len()
                    && v.pos.distance(path.waypoints[path.cursor]) < 1.2
                {
                    path.cursor += 1;
                }
                path.waypoints.get(path.cursor).copied().unwrap_or(tp)
            } else {
                // Direct steer at close range — but a tree up a terrace lip is in reach on the
                // flat (XZ) and unreachable on foot: the steering fan wall-follows the cliff
                // face, `moving` stays true, and the stall never fires. Cap the time spent
                // this close without arriving; on the cap, shun the tree and pick another.
                job.close += dt;
                if job.close > CLOSE_GIVEUP_SECS {
                    danger.0.push((tp, now + 45.0));
                    commands.entity(self_e).try_remove::<ChopJob>();
                    continue;
                }
                path.waypoints.clear();
                path.cursor = 0;
                tp
            };
            let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
            let advanced = steer::advance(
                v.pos,
                v.facing,
                step_target,
                v.speed * dt,
                v.body_r,
                cur_y,
                3.0 * dt,
            );
            // Apply the step on ANY result — crucially the PIVOT case (`Some` with `moving:false`),
            // where the steerer turned toward an opening but couldn't step yet. Dropping that
            // pivot-facing (the old `Some(s) if s.moving` arm did) froze the cutter facing a blocked
            // heading so it could never rotate to round a wall/building corner — THE courtyard-wedge
            // "standing still" bug. `villager_brain`/`guard_combat` already apply facing on any Some.
            match advanced {
                Some(s) => {
                    v.facing = s.facing;
                    v.pos = s.pos; // == old pos when !moving, so a pivot doesn't teleport
                    v.moving = s.moving;
                }
                None => v.moving = false,
            }
            if v.moving {
                job.stall = 0.0;
                job.stuck = 0.0; // real forward progress clears the wedge clock
            } else {
                job.stall += dt;
                job.stuck += dt;
                if job.stuck > WEDGE_GIVEUP_SECS {
                    // Wedged for a long stretch despite pivots + replans — a genuine trap. Abandon
                    // + briefly shun so the repick differs.
                    danger.0.push((tp, now + 30.0));
                    commands.entity(self_e).try_remove::<ChopJob>();
                } else if job.stall > STALL_REPLAN_SECS {
                    // Re-path from here (bridges now steerable) — the wedge is on the route, not at
                    // the (A*-reachable) tree. Mirrors the hauler's replan-on-stall recovery.
                    path.waypoints.clear();
                    path.cursor = 0;
                    path.next_replan = now;
                    job.stall = 0.0;
                }
            }
        }
        // Ground-follow + bob (this system owns the transform while the job is on).
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Carry the felled log back to the worker's own plot (the Lumber yard) and bank the wood
/// THERE — the delivery, not the chop, is what pays. Same A*-when-far march as [`chop_work`];
/// a wedged hauler just forces a replan (home is always reachable, unlike an arbitrary tree).
#[allow(clippy::type_complexity)]
fn haul_home(
    time: Res<Time>,
    spots: Res<crate::town::PlotSpots>,
    mut commands: Commands,
    mut bank: ResMut<Bank>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut haulers: Query<
        (Entity, &mut Hauling, &Worker, &mut Villager, &mut Transform, &mut crate::navgrid::NavPath),
        (Without<Fleeing>, Without<FightBack>, Without<crate::dying::Dying>),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (self_e, mut hauling, worker, mut v, mut tf, mut path) in &mut haulers {
        let Some(yard) = spots.0.get(worker.idx).copied() else { continue };
        let d = v.pos.distance(yard);
        if d <= HAUL_REACH {
            // Log on the pile — NOW the wood lands in the stock.
            bank.0.add_wood(hauling.amount);
            floats.0.push(crate::combat_fx::FloatReq {
                world: Vec3::new(v.pos.x, tf.translation.y + 1.6, v.pos.y),
                text: format!("+{} wood", hauling.amount as i64),
                color: Color::srgb(0.78, 0.62, 0.36),
                scale: 1.1,
            });
            if let Some(log) = hauling.log {
                commands.entity(log).try_despawn();
            }
            commands.entity(self_e).try_remove::<Hauling>();
            v.moving = false;
            continue;
        }
        // March home: A* when far, direct steer when close (same shape as chop_work's march).
        let step_target = if d > 6.0 {
            if path.cursor >= path.waypoints.len()
                || now >= path.next_replan
                || path.goal_cached.distance(yard) > 2.0
            {
                path.waypoints = crate::navgrid::path_to(v.pos, yard);
                path.cursor = 0;
                path.goal_cached = yard;
                path.next_replan = now + 1.0 + (self_e.to_bits() % 16) as f32 * 0.05;
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
        let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        // Apply the step on ANY result, including a PIVOT (`Some`, `moving:false`) so the hauler can
        // turn toward an opening instead of freezing facing a wall — see `chop_work`.
        match steer::advance(v.pos, v.facing, step_target, v.speed * dt, v.body_r, cur_y, 3.0 * dt) {
            Some(s) => {
                v.facing = s.facing;
                v.pos = s.pos;
                v.moving = s.moving;
            }
            None => v.moving = false,
        }
        if v.moving {
            hauling.stall = 0.0;
            hauling.stuck = 0.0;
        } else {
            hauling.stall += dt;
            hauling.stuck += dt;
            if hauling.stuck > HAUL_GIVEUP_SECS {
                // Truly wedged — replanning hasn't moved it for a long while. Dump the log
                // where it stands (bank the wood; don't strand the town's only income) and
                // free the worker so `assign_tree` hands it a fresh job, instead of looping
                // replans forever in a corner A* can't escape.
                bank.0.add_wood(hauling.amount);
                floats.0.push(crate::combat_fx::FloatReq {
                    world: Vec3::new(v.pos.x, tf.translation.y + 1.6, v.pos.y),
                    text: format!("+{} wood", hauling.amount as i64),
                    color: Color::srgb(0.78, 0.62, 0.36),
                    scale: 1.1,
                });
                if let Some(log) = hauling.log {
                    commands.entity(log).try_despawn();
                }
                commands.entity(self_e).try_remove::<Hauling>();
                v.moving = false;
                continue;
            }
            if hauling.stall > STALL_SECS {
                // Wedged on the way home — drop the cached route and replan now.
                path.waypoints.clear();
                path.cursor = 0;
                path.next_replan = now;
                hauling.stall = 0.0;
            }
        }
        // Ground-follow + bob (this system owns the transform while the haul is on).
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Put the visible log on a fresh hauler's shoulder: a small brown cuboid child riding above
/// the pack. Mesh/material are built once and cached (every log looks the same).
fn attach_log(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
    mut fresh: Query<(Entity, &mut Hauling), Added<Hauling>>,
) {
    for (e, mut hauling) in &mut fresh {
        if hauling.log.is_some() {
            continue;
        }
        let (mesh, mat) = cache
            .get_or_insert_with(|| {
                (
                    meshes.add(Cuboid::new(0.26, 0.26, 1.05)),
                    materials.add(StandardMaterial {
                        base_color: Color::srgb(0.42, 0.28, 0.15),
                        perceptual_roughness: 0.95,
                        ..default()
                    }),
                )
            })
            .clone();
        let mut log = Entity::PLACEHOLDER;
        commands.entity(e).with_children(|p| {
            log = p
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(mat),
                    // Carried level across the chest in both arms (matches the biped `carry_pose`).
                    // Child of the root (scale ~0.6); rotated so the log lies across the body (X).
                    Transform::from_xyz(0.0, 1.05, 0.46)
                        .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(0.05)),
                ))
                .id();
        });
        hauling.log = Some(log);
    }
}

/// A hauler mustered to guard duty at dusk dumps the log where it stands: the wood is banked
/// on the spot (don't strand the town's only wood income on a soldier's back all night) and
/// the carried-log mesh goes away before the sword comes out.
fn shed_log_at_muster(
    mut commands: Commands,
    mut bank: ResMut<Bank>,
    mustered: Query<(Entity, &Hauling), With<Guard>>,
) {
    for (e, hauling) in &mustered {
        bank.0.add_wood(hauling.amount);
        if let Some(log) = hauling.log {
            commands.entity(log).try_despawn();
        }
        commands.entity(e).try_remove::<Hauling>();
    }
}

/// Run a fleeing NPC home (toward the castle origin), then stand down. A dusk-mustered guard
/// sheds the flag instead — the guard brain owns it from there.
#[allow(clippy::type_complexity)]
fn flee_steer(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<
        (Entity, &Fleeing, &mut Villager, &mut Transform, Option<&mut Worker>, Has<Guard>),
        Without<crate::dying::Dying>,
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (e, flee, mut v, mut tf, worker, is_guard) in &mut q {
        if is_guard || now > flee.until || v.pos.length() < FLEE_HOME_R {
            commands.entity(e).try_remove::<Fleeing>();
            continue;
        }
        if let Some(mut w) = worker {
            w.at_post = false;
        }
        let cur_y = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        // Adrenaline sprint for the walls.
        match steer::advance(v.pos, v.facing, Vec2::ZERO, v.speed * 1.7 * dt, v.body_r, cur_y, 4.5 * dt) {
            Some(s) => {
                v.pos = s.pos;
                v.facing = s.facing;
                v.moving = s.moving;
            }
            None => v.moving = false,
        }
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}
