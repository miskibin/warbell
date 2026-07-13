//! Worker economy: producer buildings auto-claim idle workers, which then run a generic
//! walk→gather→carry-home→bank loop (a slimmed fork of the `lumberjack`/`miner` haul shape),
//! a farm food cycle, flee-from-enemies, and food-surplus population growth.
//!
//! Unlike the campaign trades, movement here is delegated to the shared RTS mover: a worker just
//! stamps a [`command::MoveTo`] goal and waits for it to clear (arrival is detected by distance OR
//! the mover removing the component). The body models are the campaign peasant (player) and the
//! desert-garbed rival worker (rival); they carry [`crate::scenes::SceneActor`] so the campaign
//! wander brain never grabs them, while `villager_drive`/`animate_biped` (if run in this mode) still
//! animate the legs off the [`Villager`] pose we keep in sync each frame.
//!
//! State machine (per worker, [`Haul`]): `Start` (pick a deposit / field, issue a MoveTo) →
//! `ToDeposit`/`Farm` (walk / tend) → `Gather` (a ~3s work timer at a REAL standing tree/boulder,
//! swinging the tool via `Villager::atk_anim`) → `Carry` (shoulder a load prop, MoveTo **its own
//! producer building** — the sawmill/quarry/mine/farm it's bonded to, NOT the Town Hall) → bank
//! into [`RtsBanks`] → back to `Start`.

use std::collections::HashSet;
use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::rts::command::{HarvestAt, MoveTo};
use crate::rts::{
    base_of, harvest_kind, in_skirmish, unit_hp, BuildingKind, Deposit, DepositKind, RtsBanks,
    RtsBuilding, RtsPop, RtsUnit, Side, UnitKind,
};
use crate::villagers::{Trade, Villager};

/// Seconds a worker spends gathering at a deposit before shouldering a load.
const GATHER_SECS: f32 = 3.0;
/// Tool-swing cadence (s) while gathering — stamps `Villager::atk_anim` so `villager_limbs` plays
/// the overhead chop/pick swing (the campaign lumberjack read).
const SWING_EVERY: f32 = 0.85;
/// How far short of the tree/boulder the worker stands to work it (chopping distance).
const WORK_STANDOFF: f32 = 0.8;
/// Seconds a farmer tends the field before carrying a food sack home.
const FARM_SECS: f32 = 10.0;
/// Arrival slop (world units) at a deposit / the hall drop-off.
const ARRIVE: f32 = 1.8;
/// A hostile armed enemy this close makes a worker drop its loop and run home.
const FLEE_R: f32 = 7.0;
/// A fled worker waits this long clear of enemies before resuming work.
const FLEE_CLEAR: f32 = 4.0;
/// Population-growth cadence (seconds) — one worker may be born per side each tick.
const GROWTH_TICK: f32 = 20.0;
/// Food a side must exceed to grow a new worker.
const FOOD_SPAWN_MIN: f64 = 15.0;
/// Food spent per new worker.
const FOOD_SPAWN_COST: f64 = 10.0;
/// Per-capita food burned per living unit per second.
const FOOD_DRAIN: f64 = 0.02;

pub struct RtsWorkersPlugin;

impl Plugin for RtsWorkersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, init_carry_assets.run_if(in_skirmish)).add_systems(
            Update,
            (
                spawn_starting_workers,
                claim_workers,
                worker_haul,
                worker_flee,
                population_growth,
                worker_death,
            )
                .run_if(in_skirmish)
                .run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

// ── Components ──────────────────────────────────────────────────────────────────────

/// On a worker: the producer building it's bonded to (drives which deposit kind it harvests).
#[derive(Component)]
pub struct Assigned {
    pub building: Entity,
}

/// On a producer building: the worker currently bonded to it (so it isn't re-claimed).
#[derive(Component)]
pub struct Staffed {
    pub worker: Entity,
}

/// A worker running for its own hall after sensing an armed enemy; cleared once safe + timed out.
#[derive(Component)]
struct Fleeing {
    until: f32,
}

/// The generic haul state machine carried by a bonded worker. `pub(crate)` so the training pipeline
/// (`units::consume_train_orders`) can strip it when it conscripts a bonded worker.
#[derive(Component)]
pub(crate) struct Haul {
    phase: Phase,
    timer: f32,
    /// Visible carried-load child mesh, despawned on banking / interruption.
    carry: Option<Entity>,
    /// Last position — used to derive a facing for the animation sync.
    last: Vec2,
}

impl Haul {
    fn new() -> Self {
        Haul { phase: Phase::Start, timer: 0.0, carry: None, last: Vec2::ZERO }
    }
}

#[derive(Clone, Copy)]
enum Phase {
    /// Decide the next trip: pick a deposit (or tend a field) and issue the MoveTo.
    Start,
    /// Walking to deposit `de` — specifically to `spot`, the standing tree/boulder chosen for this
    /// trip (so the worker chops AT a trunk, not at the grove's invisible anchor).
    ToDeposit { de: Entity, spot: Vec2 },
    /// Standing beside `spot` at deposit `de`, working the gather timer (tool swings play).
    Gather { de: Entity, spot: Vec2 },
    /// Tending the field beside the farm (food cycle, no deposit).
    Farm,
    /// Carrying a load back to the worker's own producer building (sawmill/quarry/mine/farm).
    Carry(CarryKind),
}

#[derive(Clone, Copy)]
enum CarryKind {
    Wood,
    Stone,
    Gold,
    Food,
}

/// Amount banked (and drawn from a deposit) per completed trip, per resource.
fn per_trip(kind: CarryKind) -> f64 {
    match kind {
        CarryKind::Wood => 8.0,
        CarryKind::Stone => 6.0,
        CarryKind::Gold => 4.0,
        CarryKind::Food => 7.0,
    }
}

fn carry_for(dk: DepositKind) -> CarryKind {
    match dk {
        DepositKind::Wood => CarryKind::Wood,
        DepositKind::Stone => CarryKind::Stone,
        DepositKind::Gold => CarryKind::Gold,
    }
}

// ── Carried-load prop assets ────────────────────────────────────────────────────────

/// Cached mesh+material for each carried load (shouldered log / stone lump / gold sack / food sack).
#[derive(Resource)]
struct CarryAssets {
    wood: (Handle<Mesh>, Handle<StandardMaterial>),
    stone: (Handle<Mesh>, Handle<StandardMaterial>),
    gold: (Handle<Mesh>, Handle<StandardMaterial>),
    food: (Handle<Mesh>, Handle<StandardMaterial>),
}

impl CarryAssets {
    fn get(&self, kind: CarryKind) -> (Handle<Mesh>, Handle<StandardMaterial>) {
        match kind {
            CarryKind::Wood => self.wood.clone(),
            CarryKind::Stone => self.stone.clone(),
            CarryKind::Gold => self.gold.clone(),
            CarryKind::Food => self.food.clone(),
        }
    }
}

fn init_carry_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mk = |m: &mut Assets<Mesh>, mat: &mut Assets<StandardMaterial>, d: (f32, f32, f32), c: Color| {
        (
            m.add(Cuboid::new(d.0, d.1, d.2)),
            mat.add(StandardMaterial { base_color: c, perceptual_roughness: 0.92, ..default() }),
        )
    };
    commands.insert_resource(CarryAssets {
        wood: mk(&mut meshes, &mut materials, (0.26, 0.26, 1.05), Color::srgb(0.42, 0.28, 0.15)),
        stone: mk(&mut meshes, &mut materials, (0.34, 0.30, 0.34), Color::srgb(0.60, 0.60, 0.66)),
        gold: mk(&mut meshes, &mut materials, (0.30, 0.30, 0.30), Color::srgb(0.85, 0.68, 0.25)),
        food: mk(&mut meshes, &mut materials, (0.34, 0.40, 0.30), Color::srgb(0.78, 0.66, 0.40)),
    });
}

// ── Spawning worker bodies ──────────────────────────────────────────────────────────

/// Spawn one worker body for `side` at `pos`: the campaign peasant for the player, the desert
/// worker for the rival, plus the RTS unit bundle. Both bodies carry `SceneActor` to keep the
/// campaign wander brain off them.
pub(crate) fn spawn_worker_body(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    side: Side,
    pos: Vec2,
    seed: u32,
) -> Entity {
    // A generic labourer look; the trade only picks the outfit/tool, not behaviour.
    let trade = Trade::Farmer;
    let e = match side {
        Side::Player => {
            crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, pos, 0.0, Some(trade), seed)
        }
        Side::Rival => {
            crate::villagers::spawn_rival_worker(commands, meshes, creature_mats, trade, pos, pos, seed)
        }
    };
    let hp = unit_hp(UnitKind::Worker);
    commands
        .entity(e)
        .insert((
            RtsUnit { kind: UnitKind::Worker },
            side,
            crate::player::Health { hp, max: hp },
            crate::scenes::SceneActor,
            crate::navgrid::NavPath::default(),
        ))
        // The spawn helpers tag bodies `BiomeEntity`; drop it so a campaign biome-swap (keys 1-5,
        // if ever wired in this mode) can't despawn a live RTS unit.
        .remove::<crate::biome::BiomeEntity>();
    e
}

/// Once both Town Halls stand, drop 3 idle workers beside each and seed the population count.
/// One-shot (waits until it can find a hall for *both* sides so neither is missed).
fn spawn_starting_workers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    mut pop: ResMut<RtsPop>,
    halls: Query<(&RtsBuilding, &Side, &Transform)>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let mut player = None;
    let mut rival = None;
    for (b, s, t) in &halls {
        if b.kind == BuildingKind::TownHall && b.built {
            let p = Vec2::new(t.translation.x, t.translation.z);
            match s {
                Side::Player => player = Some(p),
                Side::Rival => rival = Some(p),
            }
        }
    }
    let (Some(pp), Some(rp)) = (player, rival) else { return };
    *done = true;
    for (side, hall) in [(Side::Player, pp), (Side::Rival, rp)] {
        for k in 0..3u32 {
            let ang = k as f32 / 3.0 * TAU;
            let pos = hall + Vec2::new(ang.cos(), ang.sin()) * 3.5;
            let seed = 0x51_0000 ^ (side.ix() as u32 * 97 + k * 13 + 1);
            spawn_worker_body(&mut commands, &mut meshes, &mut creature_mats, side, pos, seed);
            pop.0[side.ix()].count += 1;
        }
    }
}

// ── Claim ───────────────────────────────────────────────────────────────────────────

/// A completed producer building with no worker claims the nearest idle worker of its side.
fn claim_workers(
    mut commands: Commands,
    buildings: Query<(Entity, &RtsBuilding, &Side, &Transform), Without<Staffed>>,
    workers: Query<
        (Entity, &RtsUnit, &Side, &Transform),
        // Exclude Converting workers (walking to a barracks to be trained) — claiming one to a
        // producer would tug-of-war its MoveTo against `drive_converting` and it'd never arrive.
        (Without<Assigned>, Without<crate::dying::Dying>, Without<Fleeing>, Without<crate::rts::units::Converting>),
    >,
) {
    let idle: Vec<(Entity, Side, Vec2)> = workers
        .iter()
        .filter(|(_, u, _, _)| u.kind == UnitKind::Worker)
        .map(|(e, _, s, t)| (e, *s, Vec2::new(t.translation.x, t.translation.z)))
        .collect();
    if idle.is_empty() {
        return;
    }
    let mut taken: HashSet<Entity> = HashSet::new();
    for (be, b, bs, bt) in &buildings {
        if !b.built {
            continue;
        }
        let is_producer = harvest_kind(b.kind).is_some() || b.kind == BuildingKind::Farm;
        if !is_producer {
            continue;
        }
        let bp = Vec2::new(bt.translation.x, bt.translation.z);
        let pick = idle
            .iter()
            .filter(|(e, s, _)| s == bs && !taken.contains(e))
            .min_by(|a, c| {
                a.2.distance_squared(bp)
                    .partial_cmp(&c.2.distance_squared(bp))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(e, _, _)| *e);
        if let Some(we) = pick {
            taken.insert(we);
            commands.entity(we).try_insert((Assigned { building: be }, Haul::new()));
            commands.entity(be).try_insert(Staffed { worker: we });
        }
    }
}

// ── Haul loop ─────────────────────────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
fn worker_haul(
    time: Res<Time>,
    mut commands: Commands,
    mut banks: ResMut<RtsBanks>,
    assets: Res<CarryAssets>,
    buildings: Query<(&RtsBuilding, &Side, &Transform, Has<crate::dying::Dying>)>,
    mut deposits: Query<(Entity, &mut Deposit, &Transform)>,
    dep_vis: Query<&crate::rts::deposits::DepositVisuals>,
    part_tf: Query<&Transform>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut workers: Query<
        (
            Entity,
            &Side,
            &mut Haul,
            &Assigned,
            &Transform,
            Has<MoveTo>,
            Option<&HarvestAt>,
            Option<&mut Villager>,
        ),
        (Without<Fleeing>, Without<crate::dying::Dying>),
    >,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    // Snapshot of live (non-spent) deposits for the nearest-search; owned, so it doesn't hold a
    // borrow while we later `get_mut` a deposit to draw from it.
    let dep_list: Vec<(Entity, DepositKind, Vec2)> = deposits
        .iter()
        .filter(|(_, d, _)| d.remaining > 0.0)
        .map(|(e, d, t)| (e, d.kind, Vec2::new(t.translation.x, t.translation.z)))
        .collect();

    for (we, side, mut haul, assigned, wtf, has_moveto, harvest, mut vil) in &mut workers {
        let wpos = Vec2::new(wtf.translation.x, wtf.translation.z);

        // Keep the Villager pose in sync with the mover-driven transform so the biped animator (if
        // it runs in this mode) walks the legs; otherwise this is a harmless no-op and the body
        // glides (accepted POC fallback).
        if let Some(v) = vil.as_mut() {
            if has_moveto {
                let d = wpos - haul.last;
                if d.length_squared() > 1e-5 {
                    v.facing = d.x.atan2(d.y);
                }
            }
            v.pos = wpos;
            v.moving = has_moveto;
        }
        haul.last = wpos;

        // Building gone or dying → return to the idle pool.
        let Ok((b, _bside, btf, dying)) = buildings.get(assigned.building) else {
            drop_carry(&mut commands, &mut haul);
            commands.entity(we).try_remove::<(Assigned, Haul)>();
            continue;
        };
        if dying {
            drop_carry(&mut commands, &mut haul);
            commands.entity(we).try_remove::<(Assigned, Haul)>();
            commands.entity(assigned.building).try_remove::<Staffed>();
            continue;
        }
        let b_kind = b.kind;
        // Loads bank at the worker's OWN producer building (sawmill/quarry/mine/farm) — the whole
        // point of building one near the resource. (Fleeing still runs for `base_of`.)
        let dropoff = Vec2::new(btf.translation.x, btf.translation.z);
        let dk = harvest_kind(b_kind);

        match haul.phase {
            Phase::Start => {
                if b_kind == BuildingKind::Farm {
                    // Tend a spot beside the farm (deterministic per worker).
                    let a = (we.to_bits() % 360) as f32 * TAU / 360.0;
                    let spot = dropoff + Vec2::new(a.cos(), a.sin()) * 2.5;
                    commands.entity(we).try_insert(MoveTo { goal: spot, fight: false });
                    haul.phase = Phase::Farm;
                    haul.timer = FARM_SECS;
                } else if let Some(dk) = dk {
                    // Honour a player redirect (HarvestAt) first, if it points at a compatible,
                    // non-spent deposit; consume it either way.
                    let mut target = None;
                    if let Some(ha) = harvest {
                        if let Ok((_, d, t)) = deposits.get(ha.0) {
                            if d.kind == dk && d.remaining > 0.0 {
                                target = Some((ha.0, Vec2::new(t.translation.x, t.translation.z)));
                            }
                        }
                        commands.entity(we).try_remove::<HarvestAt>();
                    }
                    let target = target.or_else(|| nearest(&dep_list, dk, wpos));
                    if let Some((de, dpos)) = target {
                        // Work a REAL standing tree/boulder of the site, not the invisible anchor:
                        // walk to a chopping stand-off beside the nearest standing part.
                        let spot = dep_vis
                            .get(de)
                            .ok()
                            .and_then(|v| crate::rts::deposits::nearest_standing_part(v, &part_tf, wpos))
                            .unwrap_or(dpos);
                        let stand = spot + (wpos - spot).normalize_or_zero() * WORK_STANDOFF;
                        commands.entity(we).try_insert(MoveTo { goal: stand, fight: false });
                        haul.phase = Phase::ToDeposit { de, spot };
                    } else {
                        // Nothing left to harvest → idle (unassign).
                        commands.entity(we).try_remove::<(Assigned, Haul)>();
                        commands.entity(assigned.building).try_remove::<Staffed>();
                    }
                } else {
                    // Bonded to a non-producer somehow → release.
                    commands.entity(we).try_remove::<(Assigned, Haul)>();
                    commands.entity(assigned.building).try_remove::<Staffed>();
                }
            }
            Phase::ToDeposit { de, spot } => match deposits.get(de) {
                Ok((_, d, _)) if d.remaining > 0.0 => {
                    if !has_moveto || wpos.distance(spot) <= ARRIVE {
                        commands.entity(we).try_remove::<MoveTo>();
                        haul.phase = Phase::Gather { de, spot };
                        haul.timer = GATHER_SECS;
                    }
                }
                _ => {
                    // Deposit spent / gone before arrival — re-plan.
                    commands.entity(we).try_remove::<MoveTo>();
                    haul.phase = Phase::Start;
                }
            },
            Phase::Gather { de, spot } => {
                // Face the trunk and swing the tool on a cadence — `villager_limbs` plays the
                // overhead chop off `atk_anim`, so the work reads like the campaign lumberjack.
                if let Some(v) = vil.as_mut() {
                    let d = spot - wpos;
                    if d.length_squared() > 1e-4 {
                        v.facing = d.x.atan2(d.y);
                    }
                    let prev = haul.timer;
                    if (prev / SWING_EVERY).ceil() != ((prev - dt) / SWING_EVERY).ceil() {
                        v.atk_anim = now;
                        // Tool sound per swing: axe on wood, pick-chip on stone/gold.
                        match dk {
                            Some(DepositKind::Wood) => {
                                cues.write(crate::audio::AudioCue::WoodChop);
                            }
                            Some(DepositKind::Stone | DepositKind::Gold) => {
                                cues.write(crate::audio::AudioCue::OreChip);
                            }
                            None => {}
                        }
                    }
                }
                haul.timer -= dt;
                if haul.timer <= 0.0 {
                    // `dk` is Some here (Gather only entered for producers).
                    let kind = dk.map(carry_for).unwrap_or(CarryKind::Food);
                    if let Ok((_, mut d, _)) = deposits.get_mut(de) {
                        let got = crate::rts::deposits::take(&mut d, per_trip(kind));
                        if got > 0.0 {
                            let child = spawn_carry(&mut commands, &assets, kind, we);
                            haul.carry = Some(child);
                            commands.entity(we).try_insert(MoveTo { goal: dropoff, fight: false });
                            haul.phase = Phase::Carry(kind);
                        } else {
                            haul.phase = Phase::Start; // depleted mid-gather
                        }
                    } else {
                        haul.phase = Phase::Start;
                    }
                }
            }
            Phase::Farm => {
                // Field-work swings (a hoe rhythm, slower than the chop).
                if let Some(v) = vil.as_mut() {
                    let prev = haul.timer;
                    if (prev / 1.4).ceil() != ((prev - dt) / 1.4).ceil() {
                        v.atk_anim = now;
                    }
                }
                haul.timer -= dt;
                if haul.timer <= 0.0 {
                    let child = spawn_carry(&mut commands, &assets, CarryKind::Food, we);
                    haul.carry = Some(child);
                    cues.write(crate::audio::AudioCue::Forage); // gathered a food sack
                    commands.entity(we).try_insert(MoveTo { goal: dropoff, fight: false });
                    haul.phase = Phase::Carry(CarryKind::Food);
                }
            }
            Phase::Carry(kind) => {
                if !has_moveto || wpos.distance(dropoff) <= ARRIVE {
                    let amt = per_trip(kind);
                    let bank = banks.side_mut(*side);
                    match kind {
                        CarryKind::Wood => bank.wood += amt,
                        CarryKind::Stone => bank.stone += amt,
                        CarryKind::Gold => bank.gold += amt,
                        CarryKind::Food => bank.food += amt,
                    }
                    drop_carry(&mut commands, &mut haul);
                    commands.entity(we).try_remove::<MoveTo>();
                    haul.phase = Phase::Start;
                }
            }
        }
    }
}

fn drop_carry(commands: &mut Commands, haul: &mut Haul) {
    if let Some(c) = haul.carry.take() {
        commands.entity(c).try_despawn();
    }
}

fn spawn_carry(commands: &mut Commands, assets: &CarryAssets, kind: CarryKind, parent: Entity) -> Entity {
    let (mesh, mat) = assets.get(kind);
    // Carried across the chest / shouldered (worker root scale ≈0.81; child in local space).
    let child = commands
        .spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_xyz(0.0, 1.05, 0.4)))
        .id();
    commands.entity(parent).add_child(child);
    child
}

fn nearest(list: &[(Entity, DepositKind, Vec2)], kind: DepositKind, from: Vec2) -> Option<(Entity, Vec2)> {
    list.iter()
        .filter(|(_, k, _)| *k == kind)
        .min_by(|a, b| {
            a.2.distance_squared(from)
                .partial_cmp(&b.2.distance_squared(from))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(e, _, p)| (*e, *p))
}

// ── Flee ────────────────────────────────────────────────────────────────────────────

/// Workers within [`FLEE_R`] of a hostile armed unit (the other side's Swordsman/Archer) drop the
/// haul loop and run home; they resume once clear for ~[`FLEE_CLEAR`]s. (Armed units arrive in
/// wave 3, so this is wired but inert until then.)
#[allow(clippy::type_complexity)]
fn worker_flee(
    time: Res<Time>,
    mut commands: Commands,
    units: Query<(&RtsUnit, &Side, &Transform), Without<crate::dying::Dying>>,
    mut workers: Query<
        (Entity, &RtsUnit, &Side, &Transform, Option<&mut Haul>, Option<&Fleeing>),
        Without<crate::dying::Dying>,
    >,
) {
    let now = time.elapsed_secs();
    // Positions of every armed unit, by side.
    let armed: Vec<(Side, Vec2)> = units
        .iter()
        .filter(|(u, _, _)| matches!(u.kind, UnitKind::Swordsman | UnitKind::Archer))
        .map(|(_, s, t)| (*s, Vec2::new(t.translation.x, t.translation.z)))
        .collect();

    for (we, u, side, wtf, haul, fleeing) in &mut workers {
        if u.kind != UnitKind::Worker {
            continue;
        }
        let wpos = Vec2::new(wtf.translation.x, wtf.translation.z);
        let foe = side.foe();
        let threatened = armed.iter().any(|(s, p)| *s == foe && p.distance(wpos) < FLEE_R);

        if threatened {
            // Enter / refresh the flight home.
            commands.entity(we).try_insert(Fleeing { until: now + FLEE_CLEAR });
            commands.entity(we).try_insert(MoveTo { goal: base_of(*side), fight: false });
            if let Some(mut h) = haul {
                if let Some(c) = h.carry.take() {
                    commands.entity(c).try_despawn();
                }
                h.phase = Phase::Start; // re-plan on resume
            }
        } else if let Some(f) = fleeing {
            if now >= f.until {
                commands.entity(we).try_remove::<Fleeing>();
            }
        }
    }
}

// ── Population growth + upkeep ────────────────────────────────────────────────────────

/// Per-capita food drain every frame + a food-surplus growth tick every [`GROWTH_TICK`]s: if a
/// side has food to spare and room under its cap, a new worker walks out of the hall.
fn population_growth(
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    mut banks: ResMut<RtsBanks>,
    mut pop: ResMut<RtsPop>,
    mut speak: MessageWriter<crate::audio::Speak>,
    halls: Query<(&RtsBuilding, &Side, &Transform)>,
    mut acc: Local<f32>,
) {
    let dt = time.delta_secs();
    for side in [Side::Player, Side::Rival] {
        let count = pop.0[side.ix()].count as f64;
        let bank = banks.side_mut(side);
        bank.food = (bank.food - FOOD_DRAIN * count * dt as f64).max(0.0);
    }

    *acc += dt;
    if *acc < GROWTH_TICK {
        return;
    }
    *acc -= GROWTH_TICK;

    let mut hall_pos = [None, None];
    for (b, s, t) in &halls {
        if b.kind == BuildingKind::TownHall && b.built {
            hall_pos[s.ix()] = Some(Vec2::new(t.translation.x, t.translation.z));
        }
    }
    for side in [Side::Player, Side::Rival] {
        let Some(pos) = hall_pos[side.ix()] else { continue };
        let ps = pop.0[side.ix()];
        if banks.side(side).food > FOOD_SPAWN_MIN && ps.count < ps.cap {
            let seed = 0x9e_0000 ^ (side.ix() as u32 * 131 + ps.count * 17 + 3);
            let out = pos + Vec2::new(2.0, 2.0);
            spawn_worker_body(&mut commands, &mut meshes, &mut creature_mats, side, out, seed);
            pop.0[side.ix()].count += 1;
            banks.side_mut(side).food -= FOOD_SPAWN_COST;
            if side == Side::Player {
                // "A new pair of hands!" — villager birth line at the hall.
                let at = Vec3::new(out.x, 1.0, out.y);
                speak.write(crate::audio::Speak::at(crate::audio::Concept::VillagerBorn, at));
            }
        }
    }
}

/// A worker dying frees its bonded building and drops the side's population count. (Soldier deaths
/// are owned by `units.rs`; guarded on `Worker` here so the count isn't double-decremented.)
fn worker_death(
    mut commands: Commands,
    mut pop: ResMut<RtsPop>,
    dead: Query<(&RtsUnit, &Side, Option<&Assigned>), Added<crate::dying::Dying>>,
) {
    for (u, side, assigned) in &dead {
        if u.kind != UnitKind::Worker {
            continue;
        }
        let ps = &mut pop.0[side.ix()];
        ps.count = ps.count.saturating_sub(1);
        if let Some(a) = assigned {
            commands.entity(a.building).try_remove::<Staffed>();
        }
    }
}
