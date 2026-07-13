//! The mirrored rival AI: a single paced *think tick* (fork of `rival.rs::rival_economy`'s
//! interval-disciplined decision shape) that spends **only** from the Rival bank under the exact same
//! rules as the player — no cheating, no resource grants, no reveal (there's no fog anyway).
//!
//! Each tick, in order:
//!  1. **Build** — walk a fixed build order and, when the next entry is affordable in the Rival bank,
//!     raise it at the first valid pre-authored slot ringing `RIVAL_BASE` (via [`build::try_place`],
//!     the same validate→spend→scaffold entry the player's ghost uses). At most one per tick.
//!  2. **Train** — once a Rival Barracks stands, alternate Swordsman/Archer while the army is below
//!     the current wave target, the bank affords the train cost, and > 4 workers remain hauling.
//!     Emitted as a [`TrainOrder`]; `units.rs` re-validates at consume time, so a duplicate is inert.
//!  3. **Muster** — a fresh soldier (one never seen before) gets a one-time attack-move to a point
//!     just outside the base toward the map centre, so idle troops don't stand inside buildings.
//!  4. **Attack wave** — when the living Rival army reaches the wave threshold (6, +2 each wave, cap
//!     14) and the launch cooldown has elapsed, order the **whole** army to attack-move the player's
//!     Town Hall, bump the threshold, and start a ~90 s cooldown. Soldiers fight to the death.
//!
//! Assignment (worker→producer claiming) and population growth are the shared automatic systems in
//! `workers.rs` — this AI never assigns a worker by hand.
//!
//! Gated `in_skirmish` + `Playing` + `Modal::None` like every other RTS sim system.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::dying::Dying;
use crate::game_state::{AppState, Modal};

use super::build::{self, RtsBuildAssets};
use super::{
    building_def, in_skirmish, train_cost, BuildingKind, Deposit, Order, RtsBanks, RtsBuilding,
    RtsOrder, RtsUnit, Side, TrainOrder, TrainQueue, UnitKind, PLAYER_BASE, RIVAL_BASE,
    TRAIN_QUEUE_DEPTH,
};

// ────────────────────────────────────────────────────────────── tuning

/// Seconds before the rival's *first* think (lets the match breathe: both economies open the same).
const START_DELAY: f32 = 8.0;
/// Think cadence — one decision pass every this many seconds (mirrors `rival_economy`'s discipline).
const THINK_INTERVAL: f32 = 5.0;
/// Stop building once the base holds this many AI-raised structures (Town Hall is pre-placed,
/// uncounted). Bigger-game scale — the rival raises a real town.
const BUILD_MAX: usize = 20;
/// Keep at least this many workers hauling — the AI won't train a soldier that would drop below it.
const MIN_WORKERS: usize = 7;
/// First attack wave launches at this army size; each launched wave raises it up to the cap. Scaled
/// up for the bigger-game armies.
const WAVE_START: u32 = 8;
const WAVE_STEP: u32 = 3;
const WAVE_CAP: u32 = 26;
/// Seconds a launched wave holds before the next one may go (so reinforcements don't dribble in).
const WAVE_COOLDOWN: f32 = 90.0;
/// How far outside the base (toward the map centre) fresh troops muster.
const MUSTER_OUT: f32 = 6.0;

// ────────────────────────────────────────────────────────────── state

/// The rival AI's entire mutable brain. One resource, ticked off `Time::elapsed_secs`.
#[derive(Resource)]
struct RtsAiState {
    /// Absolute time (s) of the next think pass.
    next_think: f32,
    /// Index into the build order (advances only on a successful placement).
    build_ix: usize,
    /// Swordsman/Archer alternation for training (persisted so trains alternate across ticks).
    train_archer: bool,
    /// Army size that triggers the next attack wave.
    wave_threshold: u32,
    /// Earliest absolute time (s) the next wave may launch.
    wave_cooldown_until: f32,
    /// Soldiers already mustered (so each is nudged out of the base exactly once).
    known_soldiers: HashSet<Entity>,
}

impl Default for RtsAiState {
    fn default() -> Self {
        RtsAiState {
            next_think: START_DELAY,
            build_ix: 0,
            train_archer: false,
            wave_threshold: WAVE_START,
            wave_cooldown_until: 0.0,
            known_soldiers: HashSet::new(),
        }
    }
}

// ────────────────────────────────────────────────────────────── plugin

pub struct RtsAiPlugin;

impl Plugin for RtsAiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RtsAiState>().add_systems(
            Update,
            rival_ai_think
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing))
                .run_if(in_state(Modal::None)),
        );
    }
}

// ────────────────────────────────────────────────────────────── build order + slots

/// The mirrored build order (spec §8). After the authored opening it falls to House/Farm filler.
fn build_kind(ix: usize) -> BuildingKind {
    use BuildingKind::*;
    const SEQ: [BuildingKind; 10] =
        [Farm, Sawmill, House, Quarry, House, GoldMine, Barracks, House, Farm, House];
    match SEQ.get(ix) {
        Some(k) => *k,
        None => {
            if ix % 2 == 0 {
                House
            } else {
                Farm
            }
        }
    }
}

/// Pre-authored candidate offsets (world units) from `RIVAL_BASE`: three rings (r 6 / 8.5 / 11) of six
/// slots each, staggered, near→far. The AI takes the first that [`build::placement_valid`] accepts;
/// it never brute-force searches. Mirrors the plausible fan of a player's town around its hall.
fn candidate_offsets() -> Vec<Vec2> {
    let mut out = Vec::with_capacity(18);
    for (r, phase) in [(6.0f32, 0.0f32), (8.5, 0.5), (11.0, 1.0)] {
        for k in 0..6u32 {
            let a = (k as f32 + phase) / 6.0 * std::f32::consts::TAU;
            out.push(Vec2::new(r * a.cos(), r * a.sin()));
        }
    }
    out
}

// ────────────────────────────────────────────────────────────── think tick

#[allow(clippy::too_many_arguments)]
fn rival_ai_think(
    time: Res<Time>,
    mut state: ResMut<RtsAiState>,
    mut commands: Commands,
    assets: Option<Res<RtsBuildAssets>>,
    mut banks: ResMut<RtsBanks>,
    deposits_q: Query<&Transform, With<Deposit>>,
    buildings_q: Query<(Entity, &RtsBuilding, &Side, Option<&TrainQueue>)>,
    units_q: Query<(Entity, &RtsUnit, &Side), Without<Dying>>,
    mut trains: MessageWriter<TrainOrder>,
    mut orders: MessageWriter<RtsOrder>,
) {
    let now = time.elapsed_secs();
    if now < state.next_think {
        return;
    }
    state.next_think = now + THINK_INTERVAL;

    // Living Rival roster (Dying already filtered by the query).
    let mut soldiers: Vec<Entity> = Vec::new();
    let mut workers = 0usize;
    for (e, u, side) in &units_q {
        if *side != Side::Rival {
            continue;
        }
        match u.kind {
            UnitKind::Worker => workers += 1,
            UnitKind::Swordsman | UnitKind::Archer => soldiers.push(e),
        }
    }
    let army = soldiers.len() as u32;

    // ── 1. BUILD ── one structure per tick, at the first valid authored slot, if affordable.
    if let Some(assets) = assets.as_deref() {
        if state.build_ix < BUILD_MAX {
            let kind = build_kind(state.build_ix);
            let cost = building_def(kind).cost;
            if banks.side(Side::Rival).can_afford(&cost) {
                let deposits: Vec<Vec2> = deposits_q
                    .iter()
                    .map(|t| Vec2::new(t.translation.x, t.translation.z))
                    .collect();
                for (i, off) in candidate_offsets().into_iter().enumerate() {
                    let pos = RIVAL_BASE + off;
                    if build::try_place(
                        &mut commands,
                        assets,
                        &mut banks,
                        &deposits,
                        kind,
                        Side::Rival,
                        pos,
                        (i as u32) % 4,
                    ) {
                        state.build_ix += 1;
                        break;
                    }
                }
            }
        }
    }

    // ── 2. TRAIN ── while the army is under the wave target, workers can spare one, and it's afforded.
    if army < state.wave_threshold && workers > MIN_WORKERS {
        // A built Rival barracks with a train queue that isn't already full.
        let barracks = buildings_q.iter().find(|(_, b, s, tq)| {
            b.kind == BuildingKind::Barracks
                && b.built
                && **s == Side::Rival
                && tq.is_some_and(|q| q.queue.len() < TRAIN_QUEUE_DEPTH)
        });
        if let Some((be, _, _, _)) = barracks {
            let kind = if state.train_archer { UnitKind::Archer } else { UnitKind::Swordsman };
            if banks.side(Side::Rival).can_afford(&train_cost(kind)) {
                trains.write(TrainOrder { building: be, kind });
                state.train_archer = !state.train_archer;
            }
        }
    }

    // ── 3. MUSTER ── nudge each never-seen soldier out of the base once, toward the map centre.
    let current: HashSet<Entity> = soldiers.iter().copied().collect();
    state.known_soldiers.retain(|e| current.contains(e));
    let to_centre = (Vec2::ZERO - RIVAL_BASE).normalize_or_zero();
    let muster = RIVAL_BASE + to_centre * MUSTER_OUT;
    for &e in &soldiers {
        if state.known_soldiers.insert(e) {
            orders.write(RtsOrder { units: vec![e], order: Order::AttackMove(muster) });
        }
    }

    // ── 4. ATTACK WAVE ── whole army marches on the player's hall; overrides any muster this tick.
    if army >= state.wave_threshold && now >= state.wave_cooldown_until && !soldiers.is_empty() {
        orders.write(RtsOrder { units: soldiers, order: Order::AttackMove(PLAYER_BASE) });
        state.wave_threshold = (state.wave_threshold + WAVE_STEP).min(WAVE_CAP);
        state.wave_cooldown_until = now + WAVE_COOLDOWN;
    }
}
