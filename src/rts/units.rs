//! RTS soldiers: barracks **training pipeline** (idle worker + cost → swordsman/archer) and the
//! Side-based **combat brains** (melee chase/strike, archer volleys). Everything reuses the shared
//! substrate — the campaign guard/archer bodies (`villagers::spawn_rts_militia` for the player,
//! `spawn_rival_soldier`/`spawn_rival_archer` for the rival), the shared `command::MoveTo` mover for
//! locomotion, the shared `player::Health` channel for damage, `dying::begin_dying` for the crumple,
//! and `blockers::wall_between` for archer line-of-sight.
//!
//! ## Training (Stronghold "convert a peasant" flow, spec §7)
//! A `TrainOrder` (from the HUD or the AI) spends the cost NOW and walks one worker of that side to
//! the barracks (`Converting`). It **prefers an idle worker**, but falls back to conscripting one off
//! its economy job (releasing its `Staffed` bond) when none is idle — otherwise a healthy economy,
//! which bonds every idle worker to a producer (`workers::claim_workers`), would leave zero idle
//! workers and the barracks would silently never train (the bug the RC playtest surfaced). On arrival
//! the worker BODY despawns (population is unchanged — a worker became a soldier) and the kind is
//! pushed onto the barracks `TrainQueue`; the queue driver births the soldier after `TRAIN_SECS`.
//!
//! ## Combat (both sides, symmetric)
//! `index_targets` snapshots every live RtsUnit + RtsBuilding into a shared `TargetIndex` once a
//! frame (so no combat system needs its own conflicting `&Transform` query). `acquire_targets` gives
//! any idle / attack-moving soldier the nearest hostile within `SIGHT`. `melee_brain` chases + strikes;
//! `archer_brain` kites to a standoff and volleys — the arrow's damage is scheduled directly (the
//! campaign arrow-impact system only hits the campaign hostile set, never RtsUnits) while a cosmetic
//! `ArrowSpawn` still flies so it *looks* right. Deaths route through `dying::begin_dying`; `soldier_death`
//! drops the fallen soldier's `RtsPop` (workers are handled by `workers::worker_death`).
//!
//! Every system is gated `in_skirmish` + `Playing` + `Modal::None` (all of this is world-sim).

use std::collections::HashMap;

use bevy::math::EulerRot;
use bevy::prelude::*;

use crate::dying::{begin_dying, Dying};
use crate::game_state::{AppState, Modal};
use crate::rts::command::{AttackTarget, MoveTo};
use crate::rts::workers::{Assigned, Staffed};
use crate::villagers::{Villager, BOW_SHOT_SECS};
use crate::{blockers, villagers};

use super::{
    base_of, building_def, in_skirmish, train_cost, unit_damage, unit_hp, BuildingKind, RtsBanks,
    RtsBuilding, RtsPop, RtsUnit, Side, TrainOrder, TrainQueue, UnitKind, TRAIN_QUEUE_DEPTH,
    TRAIN_SECS,
};

// ── tuning ────────────────────────────────────────────────────────────────────────────

/// A soldier auto-acquires the nearest hostile within this range (world units). Wide enough for an
/// RTS read — an idle soldier engages anything (unit OR building) that comes within ~18u, and a
/// defender at the base reacts before an attacker is on top of it. (Archers cap their standoff at
/// `ARCHER_HOLD+1.5` regardless, so widening acquisition doesn't stretch their kite.)
const SIGHT: f32 = 18.0;
/// Melee reach past a target's body / footprint edge. A touch generous so a CLUSTER of attackers can
/// all engage a single foe from a loose ring — instead of the rear rank jamming behind the front one,
/// unable to reach, and jittering.
const STRIKE_RANGE: f32 = 1.8;
/// Seconds between melee swings.
const MELEE_CD: f32 = 1.0;
/// A converting worker within this of its barracks has arrived (footprint 4 → edge ~2 + body).
const CONVERT_ARRIVE: f32 = 3.0;

/// Archer preferred stand-off from its target (inside `SIGHT`, so it can both see and hold there).
const ARCHER_HOLD: f32 = 9.0;
/// If a foe closes nearer than this, the archer kites back to `ARCHER_HOLD`.
const ARCHER_KITE: f32 = 3.5;
/// Full volley cycle length (draw + rest); the draw clip itself is `BOW_SHOT_SECS`.
const VOLLEY_CD: f32 = 2.2;

pub struct RtsUnitsPlugin;

impl Plugin for RtsUnitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TargetIndex>().init_resource::<ArrowHits>().add_systems(
            Update,
            (
                // training pipeline
                consume_train_orders,
                drive_converting,
                drive_train_queue,
                // combat
                index_targets,
                acquire_targets,
                melee_brain,
                archer_brain,
                watchtower_fire,
                resolve_arrow_hits,
                // presentation + death
                sync_soldier_pose,
                reap_units,
                soldier_death,
            )
                // Chained so `index_targets` precedes the acquire/brains that read `TargetIndex`.
                .chain()
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing))
                .run_if(in_state(Modal::None)),
        );
        if std::env::var("FOREST_PERFTEST").is_ok() && std::env::var("FOREST_PERF_ARMY").is_ok() {
            app.add_systems(Update, stage_perf_army.before(index_targets)
                .run_if(in_skirmish).run_if(in_state(AppState::Playing)).run_if(in_state(Modal::None)));
        }
    }
}

/// Opt-in late-game staging, using the same bodies/combat/movement as trained troops.
/// Each side stays within the real population ceiling; never registered behavior without perf flags.
#[allow(clippy::too_many_arguments)]
fn stage_perf_army(
    time: Res<Time>,
    world_ready: Res<crate::biome::WorldReady>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    mut pop: ResMut<RtsPop>,
    mut halls: Query<(&RtsBuilding, &mut crate::player::Health)>,
    mut staged: Local<bool>,
) {
    if *staged || !world_ready.0 || std::env::var("FOREST_PERFTEST").is_err() { return; }
    let Some(count) = std::env::var("FOREST_PERF_ARMY").ok().and_then(|s| s.parse::<usize>().ok()) else { return };
    if !halls.iter().any(|(b, _)| b.kind == BuildingKind::TownHall) { return; }
    *staged = true;
    // Protect bases only in this opt-in workload, so a won battle cannot silently turn
    // the remaining measurement into a frozen game-over screen. Unit stats stay native.
    for (b, mut hp) in &mut halls {
        if b.kind == BuildingKind::TownHall { hp.hp = 1_000_000.0; hp.max = 1_000_000.0; }
    }
    info!("PERF_ARMY bases protected; unit stats unchanged");
    let mut complete = true;
    for side in [Side::Player, Side::Rival] {
        let request = count.min(super::POP_HARD_CAP.saturating_sub(pop.0[side.ix()].count) as usize);
        let centre = base_of(side) * 0.45;
        let mut made = 0;
        for slot in 0..1024 {
            if made == request { break; }
            let ring = slot as f32 * 2.399_963;
            let radius = 2.0 + (slot as f32).sqrt() * 1.8;
            let p = centre + Vec2::new(ring.cos(), ring.sin()) * radius;
            if crate::steer::footing(p.x, p.y).is_none() || blockers::is_blocked(p.x, p.y) { continue; }
            let kind = if made % 3 == 0 { UnitKind::Archer } else { UnitKind::Swordsman };
            let e = spawn_soldier(&mut commands, &mut meshes, &mut mats, side, kind, p, made as u32);
            commands.entity(e).try_insert((MoveTo { goal: base_of(side.foe()), fight: true },
                crate::navgrid::NavPath::staggered(time.elapsed_secs(), e)));
            made += 1;
        }
        pop.0[side.ix()].count += made as u32;
        complete &= made == request;
        info!("PERF_ARMY side={side:?} requested={request} spawned={made}");
    }
    commands.insert_resource(crate::perftest::PerfArmyReady(complete));
    if !complete { error!("PERF_ARMY incomplete: no performance summary will be accepted"); }
}

// ── components ──────────────────────────────────────────────────────────────────────────

/// A worker walking to a barracks to be trained into `kind`. Its economy bond is stripped when it's
/// conscripted; [`drive_converting`] re-strips any re-claim each frame, then on arrival despawns the
/// body and enqueues the soldier. `pub(crate)` so `workers::claim_workers` can exclude it — and so
/// the HUD can show a conversion that's still WALKING as an already-queued slot (the click has to
/// register in the queue instantly, not only once the peasant reaches the door).
#[derive(Component)]
pub(crate) struct Converting {
    pub(crate) barracks: Entity,
    pub(crate) kind: UnitKind,
}

/// A swordsman's swing cooldown (seconds until the next strike is allowed).
#[derive(Component, Default)]
struct Melee {
    cd: f32,
}

/// An archer's volley state machine.
#[derive(Component, Default)]
struct Volley {
    /// Seconds until the next draw may begin.
    cd: f32,
    /// Currently mid-draw.
    drawing: bool,
    /// `elapsed_secs` the current draw began.
    draw_start: f32,
    /// The shaft has left the string this draw (guards the multi-frame release window).
    loosed: bool,
}

/// Stashed attack-move goal — when an attack-moving soldier engages, its `MoveTo` is removed and the
/// original march goal parked here; on the target's death the brain re-issues `MoveTo{goal, fight:true}`
/// so the advance resumes.
#[derive(Component)]
struct ResumeMove {
    goal: Vec2,
}

// ── shared per-frame target index ────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Tgt {
    side: Side,
    pos: Vec3,
    /// Footprint half-extent (buildings) or `0` (units) — melee reach is measured to the edge.
    half: f32,
}

/// Every live RtsUnit + RtsBuilding, rebuilt each frame by [`index_targets`]. Shared so the acquire
/// and both combat brains read positions from a Resource instead of each holding a conflicting
/// `&Transform` query against the brains' `&mut Transform`.
#[derive(Resource, Default)]
struct TargetIndex(HashMap<Entity, Tgt>, Vec<(Entity, Tgt)>, tileworld_core::spatial::PointGrid);

fn index_targets(
    mut idx: ResMut<TargetIndex>,
    units: Query<(Entity, &Side, &Transform), (With<RtsUnit>, Without<Dying>)>,
    bldgs: Query<(Entity, &Side, &RtsBuilding, &Transform), (Without<RtsUnit>, Without<Dying>)>,
) {
    idx.0.clear();
    for (e, s, t) in &units {
        idx.0.insert(e, Tgt { side: *s, pos: t.translation, half: 0.0 });
    }
    for (e, s, b, t) in &bldgs {
        let half = building_def(b.kind).footprint as f32 * 0.5;
        idx.0.insert(e, Tgt { side: *s, pos: t.translation, half });
    }
    let idx = &mut *idx;
    idx.1.clear();
    idx.1.extend(idx.0.iter().map(|(&e, &t)| (e, t)));
    idx.2.rebuild(idx.1.iter().map(|(_, t)| (t.pos.x as f64, t.pos.z as f64)));
}

// ── training: order → convert an idle worker ─────────────────────────────────────────────

#[allow(clippy::type_complexity)]
fn consume_train_orders(
    mut orders: MessageReader<TrainOrder>,
    mut commands: Commands,
    mut banks: ResMut<RtsBanks>,
    barracks: Query<(&RtsBuilding, &Side, &Transform, &TrainQueue)>,
    converting: Query<&Converting>,
    // Include BONDED workers (Option<&Assigned>) so training can conscript one when none is idle —
    // a healthy economy bonds every idle worker to a producer, so an idle-only pick silently starved.
    workers: Query<
        (Entity, &RtsUnit, &Side, &Transform, Option<&Assigned>),
        (Without<Converting>, Without<Dying>),
    >,
) {
    // Bevy applies `Converting`/spend as DEFERRED commands, so the `workers` query and `converting`
    // count don't update between orders in this same frame. Track picks + per-barracks commits
    // locally so a BATCH of orders (RC/AI/rapid HUD clicks) converts N DISTINCT workers, not the
    // same nearest one N times (the bug that made batched training yield ~1 soldier).
    let mut picked: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    let mut committed: HashMap<Entity, usize> = HashMap::new();
    for ord in orders.read() {
        let Ok((b, side, btf, tq)) = barracks.get(ord.building) else { continue };
        if b.kind != BuildingKind::Barracks || !b.built {
            continue;
        }
        // Queue depth counts the FIFO, workers already walking in, AND anything committed this frame.
        let inflight = converting.iter().filter(|c| c.barracks == ord.building).count();
        let this_frame = *committed.get(&ord.building).unwrap_or(&0);
        if tq.queue.len() + inflight + this_frame >= TRAIN_QUEUE_DEPTH {
            continue;
        }
        let bpos = Vec2::new(btf.translation.x, btf.translation.z);
        // Pick the nearest worker of this side NOT already picked this frame, **preferring an idle
        // one** (no Assigned): sort by (is_bonded, distance) so any idle worker outranks every bonded
        // one, and only if there are none idle do we conscript the nearest bonded worker off its job.
        let pick = workers
            .iter()
            .filter(|(e, u, s, _, _)| u.kind == UnitKind::Worker && *s == side && !picked.contains(e))
            .min_by(|a, c| {
                let key = |w: &(Entity, &RtsUnit, &Side, &Transform, Option<&Assigned>)| {
                    let d = Vec2::new(w.3.translation.x, w.3.translation.z).distance_squared(bpos);
                    (w.4.is_some(), d) // idle (false) sorts before bonded (true)
                };
                let (ab, ad) = key(a);
                let (cb, cd) = key(c);
                ab.cmp(&cb).then(ad.partial_cmp(&cd).unwrap_or(std::cmp::Ordering::Equal))
            })
            .map(|(e, _, _, _, assigned)| (e, assigned.map(|a| a.building)));
        let Some((we, bonded_to)) = pick else { continue };
        // Spend all-or-nothing NOW (on enqueue); bail without converting if short.
        if !banks.side_mut(*side).spend(&train_cost(ord.kind)) {
            continue;
        }
        // Conscript: release any economy bond (both ends) so the producer re-claims a fresh worker
        // and `drive_converting` isn't fighting the haul loop over this body.
        if let Some(bldg) = bonded_to {
            commands.entity(bldg).try_remove::<Staffed>();
            commands.entity(we).try_remove::<Assigned>();
            commands.entity(we).try_remove::<crate::rts::workers::Haul>();
        }
        commands.entity(we).try_insert((
            Converting { barracks: ord.building, kind: ord.kind },
            MoveTo { goal: bpos, fight: false },
            crate::navgrid::NavPath::default(),
        ));
        picked.insert(we);
        *committed.entry(ord.building).or_insert(0) += 1;
    }
}

fn drive_converting(
    mut commands: Commands,
    mut queues: Query<&mut TrainQueue>,
    bstate: Query<(&RtsBuilding, &Transform)>,
    mut workers: Query<
        (Entity, &Converting, &Transform, Option<&Assigned>, Option<&mut MoveTo>),
        (With<RtsUnit>, Without<Dying>),
    >,
) {
    for (we, conv, wtf, assigned, moveto) in &mut workers {
        // A producer may have re-claimed this idle-looking worker; scrub the bond so it keeps walking.
        if let Some(a) = assigned {
            commands.entity(a.building).try_remove::<Staffed>();
            commands.entity(we).try_remove::<Assigned>();
        }
        // Barracks gone (killed mid-walk) → drop the walk, leave the worker idle (POC: no refund).
        let Ok((_, btf)) = bstate.get(conv.barracks) else {
            commands.entity(we).try_remove::<(Converting, MoveTo)>();
            continue;
        };
        let bpos = Vec2::new(btf.translation.x, btf.translation.z);
        let wpos = Vec2::new(wtf.translation.x, wtf.translation.z);
        if wpos.distance(bpos) <= CONVERT_ARRIVE {
            // Arrived: enqueue the soldier + despawn the worker body. Population is UNCHANGED — a
            // worker became a soldier (spec §7), so this is a plain try_despawn (no Dying → no
            // worker_death pop-decrement).
            if let Ok(mut q) = queues.get_mut(conv.barracks) {
                if q.queue.len() < TRAIN_QUEUE_DEPTH {
                    q.queue.push(conv.kind);
                }
            }
            commands.entity(we).try_despawn();
            continue;
        }
        // Keep the goal pinned to the barracks (recovers if worker_flee stole the MoveTo).
        match moveto {
            Some(mut m) => m.goal = bpos,
            None => {
                commands.entity(we).try_insert((
                    MoveTo { goal: bpos, fight: false },
                    crate::navgrid::NavPath::default(),
                ));
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn drive_train_queue(
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    focus: Res<crate::rts::camera::RtsCamFocus>,
    mut q: Query<(&RtsBuilding, &Side, &Transform, &mut TrainQueue)>,
) {
    let dt = time.delta_secs();
    for (b, side, btf, mut tq) in &mut q {
        if b.kind != BuildingKind::Barracks || !b.built {
            continue;
        }
        if tq.queue.is_empty() {
            tq.progress = 0.0;
            continue;
        }
        tq.progress += dt;
        if tq.progress < TRAIN_SECS {
            continue;
        }
        tq.progress = 0.0;
        let kind = tq.queue.remove(0);
        let bpos = Vec2::new(btf.translation.x, btf.translation.z);
        let half = building_def(b.kind).footprint as f32 * 0.5;
        // Muster on the friendly side (toward own base).
        let mut dir = (base_of(*side) - bpos).normalize_or_zero();
        if dir == Vec2::ZERO {
            dir = Vec2::new(1.0, 0.0);
        }
        let pos = bpos + dir * (half + 1.2);
        let seed = 0x50_1D_00 ^ (side.ix() as u32 * 131 + (time.elapsed_secs() * 1000.0) as u32);
        spawn_soldier(&mut commands, &mut meshes, &mut creature_mats, *side, kind, pos, seed);
        if *side == Side::Player && focus.in_earshot(pos) {
            // "Troops ready!" — reuse the LevelUp fanfare sting (synth-baked, always audible) so a
            // finished unit is unmistakably HEARD, not the near-silent UiSelect blip it was before.
            cues.write(crate::audio::AudioCue::LevelUp);
        }
    }
}

/// Spawn one soldier body of `kind` for `side` at `pos`, with the RTS bundle + combat state. Reuses
/// the campaign guard/archer bodies (player) or the desert rival bodies (rival); spawn hygiene mirrors
/// `workers::spawn_worker_body` exactly (SceneActor to keep the wander brain off, BiomeEntity stripped).
fn spawn_soldier(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    side: Side,
    kind: UnitKind,
    pos: Vec2,
    seed: u32,
) -> Entity {
    let archer = kind == UnitKind::Archer;
    let e = match side {
        Side::Player => villagers::spawn_rts_militia(commands, meshes, creature_mats, archer, pos, seed),
        Side::Rival if archer => {
            villagers::spawn_rival_archer(commands, meshes, creature_mats, pos, pos, seed)
        }
        Side::Rival => villagers::spawn_rival_soldier(commands, meshes, creature_mats, pos, pos, seed),
    };
    let hp = unit_hp(kind);
    commands
        .entity(e)
        .insert((
            RtsUnit { kind },
            side,
            crate::player::Health { hp, max: hp },
            crate::scenes::SceneActor,
            crate::navgrid::NavPath::default(),
            crate::rts::RtsSpawned,
        ))
        .remove::<crate::biome::BiomeEntity>();
    if archer {
        commands.entity(e).insert(Volley::default());
    } else {
        commands.entity(e).insert(Melee::default());
    }
    e
}

// ── combat: acquisition ──────────────────────────────────────────────────────────────────

/// Give any soldier without an explicit `AttackTarget` the nearest hostile within `SIGHT` — provided
/// it's idle (no `MoveTo`) or attack-moving (`MoveTo{fight:true}`). Units over buildings; archers need
/// clear LOS. On acquiring during an attack-move, stash the march goal in `ResumeMove` and drop `MoveTo`.
#[allow(clippy::type_complexity)]
fn acquire_targets(
    mut commands: Commands,
    idx: Res<TargetIndex>,
    mut speak: MessageWriter<crate::audio::Speak>,
    focus: Res<crate::rts::camera::RtsCamFocus>,
    seekers: Query<
        (Entity, &Side, &RtsUnit, &Transform, Option<&MoveTo>),
        (Without<AttackTarget>, Without<Converting>, Without<Dying>),
    >,
) {
    for (e, side, unit, tf, moveto) in &seekers {
        if unit.kind == UnitKind::Worker {
            continue; // workers never fight
        }
        // Only idle or attack-moving units auto-engage (a plain Move keeps its head down).
        match moveto {
            Some(m) if !m.fight => continue,
            _ => {}
        }
        let from = Vec2::new(tf.translation.x, tf.translation.z);
        let is_archer = unit.kind == UnitKind::Archer;

        let mut best_unit: Option<(Entity, f32, usize)> = None;
        let mut best_bld: Option<(Entity, f32, usize)> = None;
        // Small margin makes this broad phase conservative for f32 distance rounding at a cell edge.
        for rank in idx.2.nearby_or_all(from.x as f64, from.y as f64, SIGHT as f64 + 0.001) {
            let (te, t) = idx.1[rank];
            if t.side == *side {
                continue; // friendly
            }
            let tp = Vec2::new(t.pos.x, t.pos.z);
            let d = from.distance(tp);
            if d > SIGHT {
                continue;
            }
            // Archers won't shoot through a wall — require LOS to acquire.
            if is_archer && blockers::wall_between(from.x, from.y, tp.x, tp.y) {
                continue;
            }
            let slot = if t.half > 0.0 { &mut best_bld } else { &mut best_unit };
            let better = match slot {
                Some((_, bd, br)) => d < *bd || (d == *bd && rank < *br),
                None => true,
            };
            if better {
                *slot = Some((te, d, rank));
            }
        }
        let Some((target, _, _)) = best_unit.or(best_bld) else { continue };
        // Stash an attack-move's march goal so it resumes after the kill.
        if let Some(m) = moveto {
            if m.fight {
                commands.entity(e).try_insert(ResumeMove { goal: m.goal });
            }
        }
        // Enemy war-bark as a rival soldier locks on (fires once per engagement — Without<AttackTarget>
        // gates re-barks; the director spaces overlapping ones). Only if on-screen.
        if *side == Side::Rival && focus.in_earshot(from) {
            speak.write(crate::audio::Speak::at(crate::audio::Concept::RivalSpot, tf.translation));
        }
        commands.entity(e).try_insert(AttackTarget(target));
        commands.entity(e).try_remove::<MoveTo>();
    }
}

// ── combat: melee ────────────────────────────────────────────────────────────────────────

/// Swordsmen chase their `AttackTarget` and strike in reach. Damage is accumulated then applied to a
/// separate `Health` query (disjoint from the attacker `Transform`/`Villager` access). On target death,
/// resume an attack-move or fall idle (re-acquisition then picks the next foe).
#[allow(clippy::type_complexity)]
fn melee_brain(
    time: Res<Time>,
    mut commands: Commands,
    idx: Res<TargetIndex>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    focus: Res<crate::rts::camera::RtsCamFocus>,
    mut atk: Query<
        (
            Entity,
            &mut Transform,
            &mut Villager,
            &mut Melee,
            &AttackTarget,
            Option<&ResumeMove>,
            Option<&mut MoveTo>,
        ),
        Without<Dying>,
    >,
    mut healths: Query<&mut crate::player::Health>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let mut dealt: Vec<(Entity, f32)> = Vec::new();

    for (e, mut tf, mut vil, mut melee, target, resume, moveto) in &mut atk {
        melee.cd -= dt;
        let Some(t) = idx.0.get(&target.0).copied() else {
            // Target gone: resume the march or fall idle.
            if let Some(rm) = resume {
                commands.entity(e).try_insert(MoveTo { goal: rm.goal, fight: true });
                commands.entity(e).try_remove::<ResumeMove>();
            }
            commands.entity(e).try_remove::<AttackTarget>();
            continue;
        };
        let from = Vec2::new(tf.translation.x, tf.translation.z);
        let tp = Vec2::new(t.pos.x, t.pos.z);
        let edge = from.distance(tp) - t.half;

        if edge <= STRIKE_RANGE {
            // In reach: stop, face, swing on cadence.
            commands.entity(e).try_remove::<MoveTo>();
            face(&mut tf, &mut vil, tp);
            if melee.cd <= 0.0 {
                melee.cd = MELEE_CD;
                vil.atk_anim = now; // villager_drive plays the overhead swing
                if focus.in_earshot(from) {
                    cues.write(crate::audio::AudioCue::GuardStrike(tf.translation)); // swing+thud
                }
                dealt.push((target.0, unit_damage(UnitKind::Swordsman)));
            }
        } else {
            // Chase: cheaply steer the goal toward the target (mover handles staggered A* replans).
            match moveto {
                Some(mut m) => m.goal = tp,
                None => {
                    commands.entity(e).try_insert((
                        MoveTo { goal: tp, fight: false },
                        crate::navgrid::NavPath::default(),
                    ));
                }
            }
        }
    }

    for (te, dmg) in dealt {
        if let Ok(mut h) = healths.get_mut(te) {
            if h.hp > 0.0 {
                h.hp -= dmg;
            }
        }
    }
}

// ── combat: archer ───────────────────────────────────────────────────────────────────────

/// Archers hold at `ARCHER_HOLD`, kite if crowded, and volley on a `VOLLEY_CD` cycle with clear LOS.
/// The shaft's damage is scheduled directly (`ArrowHits`) — the campaign arrow-impact system can't see
/// RtsUnits — while a cosmetic `ArrowSpawn` still flies so the loose reads.
#[allow(clippy::type_complexity)]
fn archer_brain(
    time: Res<Time>,
    mut commands: Commands,
    idx: Res<TargetIndex>,
    mut arrows: ResMut<crate::projectile::ArrowSpawns>,
    mut hits: ResMut<ArrowHits>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    focus: Res<crate::rts::camera::RtsCamFocus>,
    mut atk: Query<
        (
            Entity,
            &mut Transform,
            &mut Villager,
            &mut Volley,
            &AttackTarget,
            Option<&ResumeMove>,
            Option<&mut MoveTo>,
        ),
        Without<Dying>,
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let release_p = crate::player::anim::BOW_RELEASE_P;

    for (e, mut tf, mut vil, mut volley, target, resume, moveto) in &mut atk {
        volley.cd -= dt;
        let Some(t) = idx.0.get(&target.0).copied() else {
            if let Some(rm) = resume {
                commands.entity(e).try_insert(MoveTo { goal: rm.goal, fight: true });
                commands.entity(e).try_remove::<ResumeMove>();
            }
            commands.entity(e).try_remove::<AttackTarget>();
            volley.drawing = false;
            vil.atk_anim = 0.0;
            continue;
        };
        let from = Vec2::new(tf.translation.x, tf.translation.z);
        let tp = Vec2::new(t.pos.x, t.pos.z);
        let d = from.distance(tp);
        // Direction from target back toward the archer — the line it holds / retreats along.
        let back = (from - tp).normalize_or_zero();
        let reposition = if d > SIGHT.min(ARCHER_HOLD + 1.5) {
            Some(tp + back * ARCHER_HOLD) // too far → close to the standoff ring
        } else if d < ARCHER_KITE {
            Some(tp + back * ARCHER_HOLD) // too close → kite back
        } else {
            None
        };

        if let Some(goal) = reposition {
            // Walk to the ring; cancel any in-progress draw.
            volley.drawing = false;
            vil.atk_anim = 0.0;
            match moveto {
                Some(mut m) => m.goal = goal,
                None => {
                    commands.entity(e).try_insert((
                        MoveTo { goal, fight: false },
                        crate::navgrid::NavPath::default(),
                    ));
                }
            }
            continue;
        }

        // In the band: plant, face, run the volley cycle (LOS required to loose).
        commands.entity(e).try_remove::<MoveTo>();
        face(&mut tf, &mut vil, tp);
        let los = !blockers::wall_between(from.x, from.y, tp.x, tp.y);

        if volley.drawing {
            let p = (now - volley.draw_start) / BOW_SHOT_SECS;
            if !volley.loosed && p >= release_p && los {
                volley.loosed = true;
                // Loose: chest height, half a step toward the foe; aim at the target's chest.
                let dir3 = Vec3::new(tp.x - from.x, 0.0, tp.y - from.y).normalize_or_zero();
                let bow = Vec3::new(from.x, tf.translation.y + 1.3, from.y) + dir3 * 0.45;
                if focus.in_earshot(from) {
                    cues.write(crate::audio::AudioCue::BowShot(bow)); // bowstring snap
                }
                let aim = Vec3::new(t.pos.x, t.pos.y + 1.0, t.pos.z);
                arrows.0.push(crate::projectile::ArrowSpawn {
                    from: bow,
                    aim,
                    target: target.0,
                    shooter: e,
                    damage: unit_damage(UnitKind::Archer),
                    // Always false: keeps the campaign impact system from routing at a (nonexistent)
                    // skirmish hero/townsfolk; our own ArrowHits does the real damage.
                    rival: false,
                });
                // Schedule the real damage at the shaft's flight time.
                let flight = (d / crate::projectile::ARROW_SPEED).clamp(0.1, 1.5);
                hits.0.push(ArrowHit { target: target.0, damage: unit_damage(UnitKind::Archer), at: now + flight });
            }
            if p >= 1.0 {
                volley.drawing = false;
                volley.cd = (VOLLEY_CD - BOW_SHOT_SECS).max(0.0);
            }
        } else if volley.cd <= 0.0 && los {
            volley.drawing = true;
            volley.draw_start = now;
            volley.loosed = false;
            vil.atk_anim = now; // villager_drive plays the draw-and-loose clip off this
        }
    }
}

// ── Watchtower defence ─────────────────────────────────────────────────────────────────

/// Range a watchtower can loose at (world units) and its shot cadence + damage.
const TOWER_RANGE: f32 = 13.0;
const TOWER_CD: f32 = 1.5;
const TOWER_DMG: f32 = 15.0;

/// Per-tower shot cooldown.
#[derive(Component)]
struct TowerCd(f32);

/// Every completed Watchtower looses at the nearest enemy UNIT in range on a cadence: direct damage
/// (no ballistic sim needed for a tower) + a cosmetic arrow from the crown for feedback.
#[allow(clippy::type_complexity)]
fn watchtower_fire(
    time: Res<Time>,
    mut commands: Commands,
    idx: Res<TargetIndex>,
    mut arrows: ResMut<crate::projectile::ArrowSpawns>,
    mut healths: Query<&mut crate::player::Health, Without<Dying>>,
    mut towers: Query<(Entity, &RtsBuilding, &Side, &Transform, Option<&mut TowerCd>), Without<Dying>>,
) {
    let dt = time.delta_secs();
    for (e, b, side, tf, cd) in &mut towers {
        if b.kind != BuildingKind::Watchtower || !b.built {
            continue;
        }
        // Tick / seed the cooldown; only fire on the tick it elapses.
        let ready = match cd {
            Some(mut c) => {
                c.0 -= dt;
                if c.0 <= 0.0 {
                    c.0 = TOWER_CD;
                    true
                } else {
                    false
                }
            }
            None => {
                commands.entity(e).try_insert(TowerCd(TOWER_CD));
                false
            }
        };
        if !ready {
            continue;
        }
        // Nearest enemy UNIT (half == 0) in range.
        let from = Vec2::new(tf.translation.x, tf.translation.z);
        let mut best: Option<(Entity, Vec3, f32, usize)> = None;
        for rank in idx.2.nearby_or_all(from.x as f64, from.y as f64, TOWER_RANGE as f64 + 0.001) {
            let (te, t) = idx.1[rank];
            if t.side == *side || t.half > 0.0 {
                continue; // friendly, or a building
            }
            let d = from.distance(Vec2::new(t.pos.x, t.pos.z));
            if d <= TOWER_RANGE && best.is_none_or(|bb| d < bb.2 || (d == bb.2 && rank < bb.3)) {
                best = Some((te, t.pos, d, rank));
            }
        }
        if let Some((te, tpos, _, _)) = best {
            if let Ok(mut hp) = healths.get_mut(te) {
                if hp.hp > 0.0 {
                    hp.hp -= TOWER_DMG;
                }
            }
            // Cosmetic shaft from the crown (~3.6u up); damage 0 so it doesn't double-hit.
            let bow = Vec3::new(tf.translation.x, tf.translation.y + 3.6, tf.translation.z);
            arrows.0.push(crate::projectile::ArrowSpawn {
                from: bow,
                aim: Vec3::new(tpos.x, tpos.y + 1.0, tpos.z),
                target: te,
                shooter: e,
                damage: 0.0,
                rival: false,
            });
        }
    }
}

/// A pending arrow impact: apply `damage` to `target` at `at` (scheduled at loose, LOS already checked).
struct ArrowHit {
    target: Entity,
    damage: f32,
    at: f32,
}

#[derive(Resource, Default)]
struct ArrowHits(Vec<ArrowHit>);

fn resolve_arrow_hits(
    time: Res<Time>,
    mut hits: ResMut<ArrowHits>,
    mut healths: Query<&mut crate::player::Health, Without<Dying>>,
) {
    let now = time.elapsed_secs();
    hits.0.retain(|h| {
        if now < h.at {
            return true; // still in flight
        }
        if let Ok(mut hp) = healths.get_mut(h.target) {
            if hp.hp > 0.0 {
                hp.hp -= h.damage;
            }
        }
        false
    });
}

// ── presentation + death ─────────────────────────────────────────────────────────────────

/// Keep each soldier's `Villager` pose in step with its mover-driven transform so `villager_drive`
/// walks the legs (workers do the same in `worker_haul`). `atk_anim` is stamped by the brains, not here.
#[allow(clippy::type_complexity)]
fn sync_soldier_pose(
    mut q: Query<
        (&RtsUnit, &Transform, &mut Villager, Has<MoveTo>),
        (With<RtsUnit>, Without<Dying>),
    >,
) {
    for (unit, tf, mut vil, moving) in &mut q {
        if unit.kind == UnitKind::Worker {
            continue; // workers are synced by worker_haul
        }
        vil.pos = Vec2::new(tf.translation.x, tf.translation.z);
        vil.facing = tf.rotation.to_euler(EulerRot::YXZ).0;
        vil.moving = moving;
    }
}

/// Any RtsUnit (worker or soldier) whose Health has run out begins the shared crumple. Workers get
/// their pop-decrement from `workers::worker_death`; soldiers from [`soldier_death`] — both keyed on
/// `Added<Dying>`, so this reaper only needs to start the fade.
fn reap_units(
    mut commands: Commands,
    time: Res<Time>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    focus: Res<crate::rts::camera::RtsCamFocus>,
    q: Query<(Entity, &crate::player::Health, &Transform), (With<RtsUnit>, Without<Dying>)>,
) {
    let now = time.elapsed_secs();
    for (e, hp, tf) in &q {
        if hp.hp <= 0.0 {
            if focus.in_earshot(Vec2::new(tf.translation.x, tf.translation.z)) {
                cues.write(crate::audio::AudioCue::Impact { kill: true, crit: false }); // kill thud
            }
            begin_dying(&mut commands, e, now);
        }
    }
}

/// A soldier's death drops its side's population (workers are handled by `workers::worker_death`, so
/// the `!= Worker` guard keeps the count from being double-decremented).
fn soldier_death(
    mut pop: ResMut<RtsPop>,
    mut speak: MessageWriter<crate::audio::Speak>,
    focus: Res<crate::rts::camera::RtsCamFocus>,
    dead: Query<(&RtsUnit, &Side, &Transform), Added<Dying>>,
) {
    for (unit, side, tf) in &dead {
        if unit.kind == UnitKind::Worker {
            continue;
        }
        let ps = &mut pop.0[side.ix()];
        ps.count = ps.count.saturating_sub(1);
        // A rival soldier's death cry (player militia has no voiced death line). Only if on-screen.
        if *side == Side::Rival && focus.in_earshot(Vec2::new(tf.translation.x, tf.translation.z)) {
            speak.write(crate::audio::Speak::at(crate::audio::Concept::RivalDeath, tf.translation));
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────────────────

/// Face `target` (world XZ) with an instant pivot, keeping the `Villager` pose in step.
fn face(tf: &mut Transform, vil: &mut Villager, target: Vec2) {
    let from = Vec2::new(tf.translation.x, tf.translation.z);
    let to = target - from;
    if to.length_squared() > 1e-4 {
        let yaw = to.x.atan2(to.y);
        tf.rotation = Quat::from_rotation_y(yaw);
        vil.facing = yaw;
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn acquisition_preserves_hostility_death_and_attack_move_rules() {
        let mut world = World::new();
        world.init_resource::<TargetIndex>();
        world.init_resource::<Messages<crate::audio::Speak>>();
        world.init_resource::<crate::rts::camera::RtsCamFocus>();
        let march = Vec2::new(40.0, 0.0);
        let soldier = world.spawn((Side::Player, RtsUnit { kind: UnitKind::Swordsman },
            Transform::default(), MoveTo { goal: march, fight: true })).id();
        let plain_move = world.spawn((Side::Player, RtsUnit { kind: UnitKind::Swordsman },
            Transform::default(), MoveTo { goal: march, fight: false })).id();
        let worker = world.spawn((Side::Player, RtsUnit { kind: UnitKind::Worker },
            Transform::default())).id();
        // A living hostile unit outranks a closer hostile building, friendly and corpse.
        world.spawn((Side::Rival, RtsBuilding { kind: BuildingKind::TownHall, built: true },
            Transform::from_xyz(1.0, 0.0, 0.0)));
        world.spawn((Side::Player, RtsUnit { kind: UnitKind::Worker },
            Transform::from_xyz(0.5, 0.0, 0.0)));
        world.spawn((Side::Rival, RtsUnit { kind: UnitKind::Worker },
            Transform::from_xyz(0.2, 0.0, 0.0),
            Dying { since: 0.0, dir: Vec2::ZERO, power: 1.0 }));
        let hostile = world.spawn((Side::Rival, RtsUnit { kind: UnitKind::Worker },
            Transform::from_xyz(17.0, 0.0, 0.0))).id();
        // Force the spatial branch with targets beyond the exact sight boundary.
        for i in 0..128 {
            world.spawn((Side::Rival, RtsUnit { kind: UnitKind::Worker },
                Transform::from_xyz(100.0 + i as f32 * 20.0, 0.0, 100.0)));
        }
        world.run_system_once(index_targets).unwrap();
        world.run_system_once(acquire_targets).unwrap();
        assert_eq!(world.get::<AttackTarget>(soldier).unwrap().0, hostile);
        assert_eq!(world.get::<ResumeMove>(soldier).unwrap().goal, march);
        assert!(world.get::<MoveTo>(soldier).is_none());
        assert!(world.get::<AttackTarget>(plain_move).is_none());
        assert!(world.get::<MoveTo>(plain_move).is_some());
        assert!(world.get::<AttackTarget>(worker).is_none());
    }
}
