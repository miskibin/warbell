//! **Siege** — the night-assault wave system, ported from the TS game's
//! `waveStore.ts` / `waveLogic.ts` / `WaveDirector.tsx` / `difficultyStore.ts`.
//!
//! The run is a loop of phases: **prep** (a free-roam "day" — the sun sweeps the sky as a
//! countdown) → **wave** (night falls and a warband marches on the keep from a ring around
//! it) → back to prep on a clear, or **victory** after the boss / **defeat** if the keep is
//! razed. Eight escalating waves, the last a lone giant boss; an `easy/normal/hard` preset
//! scales head-count, ork HP and the length of the day.
//!
//! This module owns the **pure decision core** (the wave table, difficulty presets, the
//! per-frame [`step_wave_director`] reducer and the [`spawn_point`] ring math) — no ECS, no
//! `Time`, no world writes — so wave progression is unit-tested in isolation, exactly like
//! the TS `waveLogic.ts`. The Bevy systems that feed it state and apply its actions live
//! lower in the file.

use bevy::prelude::*;

use crate::game_state::{AppState, Modal};
use crate::orks::{self, OrkVariant, WaveInvader};
use crate::player::{Health, HeroState, PendingHeroDamage};
use crate::projectile::{BoltSpawn, BoltSpawns};
use crate::steer;
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::worldmap::ground_at_world;

// ── Tuning (ported from waveStore.ts) ──────────────────────────────────────────────

/// Seconds the prep "day" lasts at Normal difficulty (the explore/rebuild breather). Each
/// difficulty's `prep_mul` multiplies this, so bumping the base lengthens the day on ALL of them
/// (this is the old 150s × 1.3 — a 30% longer day across the board).
pub const PREP_DURATION: f32 = 195.0;
/// A war-bell / HUD skip is ignored for this many seconds after a day begins, so a stale or
/// spam-pressed skip can't collapse the day to ~0s right after a wave→prep transition.
pub const MIN_PREP_SECONDS: f32 = 3.0;

/// One escalating assault wave. `variants` is sampled round-robin by spawn index; `hp_scale`
/// multiplies each ork's base HP; `count` orks spawn `spawn_interval` seconds apart.
pub struct WaveDef {
    pub count: u32,
    pub hp_scale: f32,
    pub variants: &'static [OrkVariant],
    pub spawn_interval: f32,
}

use OrkVariant::{Berserker, Grunt, Scout, Shaman};

/// The eight waves. Night 1 is an easy opener (grunts + a scout, base HP); counts and HP ramp
/// steeper each night (hp_scale ≈ 1.1·1.15^n); the final wave is a lone giant boss.
pub const WAVES: [WaveDef; 8] = [
    WaveDef { count: 6, hp_scale: 1.0, variants: &[Grunt, Grunt, Scout, Grunt], spawn_interval: 1.2 },
    WaveDef { count: 8, hp_scale: 1.18, variants: &[Grunt, Scout, Grunt, Berserker], spawn_interval: 1.1 },
    WaveDef { count: 12, hp_scale: 1.45, variants: &[Grunt, Scout, Berserker, Shaman], spawn_interval: 1.1 },
    WaveDef { count: 15, hp_scale: 1.67, variants: &[Grunt, Berserker, Scout, Shaman], spawn_interval: 1.0 },
    WaveDef { count: 18, hp_scale: 1.92, variants: &[Berserker, Scout, Grunt, Shaman], spawn_interval: 0.95 },
    WaveDef { count: 22, hp_scale: 2.21, variants: &[Berserker, Scout, Shaman, Grunt], spawn_interval: 0.85 },
    WaveDef { count: 26, hp_scale: 2.54, variants: &[Berserker, Shaman, Scout, Grunt], spawn_interval: 0.75 },
    WaveDef { count: 1, hp_scale: 14.0, variants: &[Berserker], spawn_interval: 0.5 }, // boss
];

/// Per-variant base HP for a wave (and camp) ork — the **full old-game** `orkConfig.ts` values
/// straight from core (grunt 254 / scout 136 / berserker 306 / shaman 201). The earlier ×0.35
/// rescale left orks far too soft against a hero that's already at full old-game power (25 base
/// dmg + weapons/crit/levels), so they're back to parity. Per-night growth still comes from
/// `hp_scale` below (1.1·1.15^n) → orks gain HP every night exactly as in the original.
pub fn base_hp(v: OrkVariant) -> f32 {
    tileworld_core::ork_config::ork_config(orks::core_variant(v)).hp as f32
}

// ── Difficulty (ported from difficultyStore.ts) ─────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

/// Per-difficulty handicaps. `count/hp/prep` scale the orks + day; `player_hp/keep_hp` scale the
/// hero's and castle's max HP at run start; `heirs_bonus` adds extra lives. Easy is tuned to be
/// genuinely beginner-friendly (fewer/softer orks, a much tougher keep + hero, spare heirs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffMods {
    pub count_mul: f32,
    pub hp_mul: f32,
    pub prep_mul: f32,
    pub player_hp_mul: f32,
    pub keep_hp_mul: f32,
    pub heirs_bonus: u32,
}

/// easy = fewer/softer orks, a long day, a beefy keep + hero and spare heirs · normal = the tuned
/// baseline · hard = more/tougher orks, a shorter day and a frailer keep.
pub fn mods_for(d: Difficulty) -> DiffMods {
    match d {
        Difficulty::Easy => DiffMods {
            count_mul: 0.7,
            hp_mul: 0.75,
            prep_mul: 1.4,
            player_hp_mul: 1.6,
            keep_hp_mul: 2.0,
            heirs_bonus: 2,
        },
        Difficulty::Normal => DiffMods {
            count_mul: 1.0,
            hp_mul: 1.0,
            prep_mul: 1.0,
            player_hp_mul: 1.0,
            keep_hp_mul: 1.0,
            heirs_bonus: 0,
        },
        Difficulty::Hard => DiffMods {
            count_mul: 1.25,
            hp_mul: 1.2,
            prep_mul: 0.8,
            player_hp_mul: 1.0,
            keep_hp_mul: 0.9,
            heirs_bonus: 0,
        },
    }
}

/// Orks in wave `i` after the difficulty count multiplier (min 1).
pub fn effective_count(i: usize, mods: DiffMods) -> u32 {
    ((WAVES[i].count as f32 * mods.count_mul).round() as u32).max(1)
}

// ── Pure director core (ported from waveLogic.ts) ───────────────────────────────────

/// The run's top-level phase. (No `Menu` — this scene boots straight into the first day.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GamePhase {
    Prep,
    Wave,
    Victory,
    Defeat,
}

/// Scratch state the reducer threads frame to frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WaveTimers {
    /// Elapsed time (s) the prep breather ends; 0 = not yet armed.
    pub prep_ends_at: f32,
    /// Earliest time (s) the next ork in this wave may spawn.
    pub next_spawn_at: f32,
    /// Running count of orks spawned this wave (drives variant rotation + ring placement).
    pub spawn_index: u32,
}

/// An action the director wants applied this frame (the side-effecting half lives in the ECS).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WaveAction {
    BeginWave { index: usize },
    SetPhase(GamePhase),
    Spawn { variant: OrkVariant, hp: f32, spawn_index: u32, wave_index: usize },
}

/// Everything the reducer needs this tick.
pub struct WaveStepInput {
    pub phase: GamePhase,
    /// 0-based current wave; -1 before the first wave starts.
    pub wave_index: i32,
    /// Orks spawned so far this wave.
    pub spawned: u32,
    /// Living wave invaders this frame.
    pub alive: u32,
    pub timers: WaveTimers,
    pub now: f32,
    /// Player rang the war bell / pressed skip — begin the night without waiting out prep.
    pub skip: bool,
    pub mods: DiffMods,
}

pub struct WaveStepResult {
    pub actions: Vec<WaveAction>,
    pub timers: WaveTimers,
}

/// Advance the director one tick. Pure: returns the actions to apply and the next timers,
/// mutating nothing. Mirrors the TS `stepWaveDirector`:
///  - **prep**: arm a `PREP_DURATION × prep_mul` countdown, then begin the next wave + go Wave
///    (a skip is honored only once the day has run `MIN_PREP_SECONDS`).
///  - **wave**: spawn one ork per `spawn_interval` until the (difficulty-scaled) quota is met;
///    once fully spawned and cleared, go Victory (last wave) or Prep.
pub fn step_wave_director(input: &WaveStepInput) -> WaveStepResult {
    let mut timers = input.timers;
    let mut actions: Vec<WaveAction> = Vec::new();

    match input.phase {
        GamePhase::Prep => {
            let dur = PREP_DURATION * input.mods.prep_mul;
            if timers.prep_ends_at == 0.0 {
                timers.prep_ends_at = input.now + dur;
            }
            // Floor the skip: honored only once the day has run MIN_PREP_SECONDS (so a stale or
            // spam-pressed skip on the wave→prep transition frame can't collapse the day to ~0s).
            // Natural expiry is never floored. `prep_ends_at - dur` is when the day was armed.
            let skip_allowed =
                input.skip && input.now >= timers.prep_ends_at - dur + MIN_PREP_SECONDS;
            if skip_allowed || input.now >= timers.prep_ends_at {
                actions.push(WaveAction::BeginWave { index: (input.wave_index + 1) as usize });
                timers.spawn_index = 0;
                timers.next_spawn_at = input.now;
                timers.prep_ends_at = 0.0;
                actions.push(WaveAction::SetPhase(GamePhase::Wave));
            }
        }
        GamePhase::Wave => {
            let i = input.wave_index as usize;
            if i >= WAVES.len() {
                return WaveStepResult { actions, timers };
            }
            let def = &WAVES[i];
            let count = effective_count(i, input.mods);
            // Spawn on interval until the wave's quota is met.
            if input.spawned < count && input.now >= timers.next_spawn_at {
                let variant = def.variants[timers.spawn_index as usize % def.variants.len()];
                let hp = (base_hp(variant) * def.hp_scale * input.mods.hp_mul).round();
                actions.push(WaveAction::Spawn {
                    variant,
                    hp,
                    spawn_index: timers.spawn_index,
                    wave_index: i,
                });
                timers.spawn_index += 1;
                timers.next_spawn_at = input.now + def.spawn_interval;
            }
            // Wave cleared once everything has spawned and nothing is left alive.
            if input.spawned >= count && input.alive == 0 {
                let next =
                    if i >= WAVES.len() - 1 { GamePhase::Victory } else { GamePhase::Prep };
                actions.push(WaveAction::SetPhase(next));
            }
        }
        GamePhase::Victory | GamePhase::Defeat => {}
    }

    WaveStepResult { actions, timers }
}

/// Sky-as-countdown progress: 0 at the start of the prep day (full timer left) → 1 once it has
/// run out (or the bell skipped it). The day's length scales with difficulty, so it measures
/// against the effective duration. Read by the day/night driver to sweep the sun.
pub fn prep_progress(prep_seconds_left: f32, mods: DiffMods) -> f32 {
    let dur = PREP_DURATION * mods.prep_mul;
    let left = prep_seconds_left.clamp(0.0, dur);
    1.0 - left / dur
}

/// A spawn point on the ring around `keep` for the `i`-th spawn: marches outward along a
/// golden-angle ray and keeps the furthest **standable** tile (per the `standable` predicate),
/// capped at `max_ring`. Ring points can otherwise land in the sea where the ork strands the
/// wave. Mirrors the TS `spawnPointFor`.
pub fn spawn_point(i: u32, keep: Vec2, max_ring: f32, standable: impl Fn(f32, f32) -> bool) -> Vec2 {
    // Golden-angle spread so successive spawns don't stack.
    let a = i as f32 * 2.399_963_2;
    let dir = Vec2::new(a.cos(), a.sin());
    // March outward along the ray, keeping the furthest standable tile (capped at max_ring).
    let mut best = keep + dir * 6.0;
    let mut r = 8.0;
    while r <= max_ring {
        let p = keep + dir * r;
        if standable(p.x, p.y) {
            best = p;
        }
        r += 2.0;
    }
    best
}

/// Where a keep-marching invader actually paths: a standable point just inside the nearest gate.
/// The keep origin sits inside the solid keep box, so A* straight to it fails and the invader
/// wedges at the wall; stepping 4u in from the gate gap lands it in the courtyard, within batter
/// range of the keep.
fn keep_march_goal(from: Vec2) -> Vec2 {
    let gate = crate::castle::gate_centers()
        .into_iter()
        .min_by(|a, b| from.distance_squared(*a).total_cmp(&from.distance_squared(*b)))
        .unwrap_or(KEEP_POS);
    gate + (KEEP_POS - gate).normalize_or_zero() * 4.0
}

/// Index of the nearest guard within `engage` of `from`, if any — the guard an invader diverts
/// onto instead of marching the keep. Pure so the hero > guard > keep priority is unit-tested
/// without the ECS (the invader brain maps the index back to the guard entity).
pub fn nearest_guard_in_range(from: Vec2, guards: &[Vec2], engage: f32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, gp) in guards.iter().enumerate() {
        let d = from.distance(*gp);
        if d < engage && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

// Stuck-ork safety net (ported intent from waveLogic.ts): an invader caught in a steering
// local-minimum (oscillating round a prop / wall-corner, or ping-ponging between two equidistant
// gates) never reaches the keep, so the wave never clears. The OLD net keyed on raw movement —
// an oscillator that still inches >EPS each cycle reset the clock forever and was never reaped,
// and it only fired beyond 16u so a courtyard wedge hung too. This net is PROGRESS-based: reap any
// keep-marcher that hasn't gotten strictly closer to the keep within the timeout, at any range.
/// Reap a wedged invader that hasn't made `STUCK_TIMEOUT` seconds of progress toward the keep.
const STUCK_TIMEOUT: f32 = 20.0;
/// An invader must close at least this much distance-to-keep to count as "still progressing".
/// Lenient: a marching ork covers tens of units in `STUCK_TIMEOUT`, so only a truly wedged one
/// (≈zero net gain) trips it — near-zero false positives.
const STUCK_PROGRESS_EPS: f32 = 0.5;

/// Progress-based stuck test for a keep-marching invader. `closest`/`progress_at` are its best
/// (minimum) distance-to-keep so far and when that last improved; `dist_keep` is this frame's
/// distance; `engaged` is true when it's attacking or actively pursuing the hero/a guard (intended
/// behaviour, never "stuck"). Returns the updated `(closest, progress_at, reap)`: progress (or
/// engagement) resets the clock; a pure keep-march with no gain past the timeout reaps. Pure so the
/// "oscillator with net-zero progress still gets culled" guarantee is unit-tested without the ECS.
pub fn stuck_step(closest: f32, progress_at: f32, dist_keep: f32, engaged: bool, now: f32) -> (f32, f32, bool) {
    if engaged || dist_keep < closest - STUCK_PROGRESS_EPS {
        (closest.min(dist_keep), now, false)
    } else if now - progress_at >= STUCK_TIMEOUT {
        (closest, progress_at, true)
    } else {
        (closest, progress_at, false)
    }
}

// ════════════════════════════════════════════════════════════════════════════════════
// ECS layer — feeds the pure core above world state and applies its actions (the
// side-effecting half of the TS `WaveDirector.tsx`), plus the keep, the invader march AI,
// the phase HUD and the day/night binding lives in `scene.rs`.
// ════════════════════════════════════════════════════════════════════════════════════

/// The keep the horde marches on — the castle at the world origin.
pub const KEEP_POS: Vec2 = Vec2::ZERO;
/// Invaders enter from a ring this far out (golden-angle spread, on standable tiles).
const SPAWN_RING: f32 = 30.0;
/// An invader within this of the keep batters it. Generous: with no A* an invader can't thread
/// the gates, so it sieges the wall ring — this lets a horde bunched at the wall still do damage.
const KEEP_ATTACK_RANGE: f32 = 14.0;
/// Keep damage per invader hit (on the ork's normal strike cooldown).
const KEEP_DAMAGE: f32 = 9.0;
/// An invader spots + diverts onto a town-guard within this range (instead of marching the keep),
/// so guards actually intercept the horde rather than being ignored.
const GUARD_ENGAGE: f32 = 8.0;
/// Keep total HP — tuned so an unopposed early wave threatens it over the night and a late wave
/// razes it fast, but the hero thinning the horde saves it.
pub const KEEP_MAX_HP: f32 = 1000.0;
/// Keep self-repair during the prep breather (HP/s) — the day is a chance to recover.
const KEEP_REPAIR_RATE: f32 = 12.0;
/// On top of the slow continuous repair, the keep is shored up by this fraction of its MAX HP at
/// the dawn of each new day (the Wave→Prep clear) — a guaranteed bounce-back between sieges.
const KEEP_DAWN_REPAIR_FRAC: f32 = 0.2;
/// The night horde's warband tint (camps use both; invaders are uniformly this).
const INVADER_FACTION: orks::Faction = orks::Faction::Red;
/// How close an arsonist invader must be to batter a building.
const BUILDING_ATTACK_RANGE: f32 = 1.8;
/// Building damage per invader per second. FORGIVING slice tuning: a farm (60 HP)
/// survives ~12s of one undefended arsonist, so you can usually reach it in time.
const BUILDING_DPS: f32 = 5.0;

/// The keep's vitals. Razed (hp ≤ 0) during a wave → defeat.
#[derive(Resource)]
pub struct KeepHp {
    pub hp: f32,
    pub max: f32,
}
impl Default for KeepHp {
    fn default() -> Self {
        KeepHp { hp: KEEP_MAX_HP, max: KEEP_MAX_HP }
    }
}

/// The live siege state — the ECS mirror of the TS `waveStore`. `timers` + `skip_requested` are
/// the reducer's scratch; the rest is read by the HUD and the day/night driver.
#[derive(Resource)]
pub struct Siege {
    pub phase: GamePhase,
    /// 0-based current wave; -1 before the first night.
    pub wave_index: i32,
    pub spawned: u32,
    /// Whole-ish seconds left in the prep day (drives the sky countdown + HUD).
    pub prep_seconds_left: f32,
    pub difficulty: Difficulty,
    timers: WaveTimers,
    skip_requested: bool,
}
impl Default for Siege {
    fn default() -> Self {
        Siege {
            phase: GamePhase::Prep,
            wave_index: -1,
            spawned: 0,
            prep_seconds_left: PREP_DURATION,
            difficulty: Difficulty::Normal,
            timers: WaveTimers::default(),
            skip_requested: false,
        }
    }
}

impl Siege {
    /// Ring in the night early (war bell / **B**): the reducer floors it to `MIN_PREP_SECONDS`.
    pub fn request_prep_skip(&mut self) {
        self.skip_requested = true;
    }
}

/// All 8 variant×faction invader meshes, built once and clone-spawned per wave ork (the camps
/// build their own; this keeps the systems decoupled).
#[derive(Resource)]
struct InvaderArmory(orks::Armory);

/// Pause-aware siege clock. Accumulates frame time ONLY while the world runs (its advancing
/// system is gated on `Modal::None`), so it does **not** tick during a pause or an open panel.
/// The director's prep countdown + wave spawn timers are absolute stamps against THIS clock —
/// not `time.elapsed_secs()`, which keeps running through a pause and made the "night in 0:45"
/// countdown jump when you resumed (time passing during pauses).
#[derive(Resource, Default)]
pub struct GameTime(pub f32);

/// Advance the pause-aware [`GameTime`] (gated, so it freezes with the rest of the world).
/// Also held while the day/night cycle is paused (`SkyClock.paused` — the F1 panel's
/// "pause cycle" box or the `P` key): pausing the sun freezes the whole night timeline, so the
/// prep countdown stops and the next wave never comes until you resume.
fn advance_game_clock(
    time: Res<Time>,
    sky: Res<crate::scene::SkyClock>,
    mut clock: ResMut<GameTime>,
) {
    if sky.paused {
        return;
    }
    clock.0 += time.delta_secs();
}

pub struct SiegePlugin;

impl Plugin for SiegePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeepHp>()
            .init_resource::<Siege>()
            .init_resource::<GameTime>()
            .add_systems(Startup, (setup_invader_armory, setup_siege_hud))
            .add_systems(PostStartup, seed_demo_wave) // FOREST_WAVE screenshot hook only
            // Pause-aware clock — advances before the sim, frozen behind any panel / outside Playing.
            .add_systems(Update, advance_game_clock.run_if(in_state(Modal::None)))
            // Sim — frozen behind any panel / outside Playing.
            .add_systems(
                Update,
                (run_director, invader_brain, siege_controls, night_warning)
                    .after(advance_game_clock)
                    .run_if(in_state(Modal::None)),
            )
            // HUD keeps drawing while frozen.
            .add_systems(Update, update_siege_hud)
            // Fresh run: reset on leaving the start screen or game-over (NOT on un-pausing,
            // which is a Playing↔Paused transition and never touches these).
            .add_systems(OnExit(AppState::StartScreen), reset_siege)
            .add_systems(OnExit(AppState::GameOver), reset_siege)
            // Pause-menu Restart / Load also begins a fresh run (gated so a plain resume is inert).
            .add_systems(
                OnExit(AppState::Paused),
                reset_siege.run_if(crate::game_state::restart_requested),
            );
    }
}

/// Reset the siege to a fresh run (keeping the chosen difficulty): clear the field, rearm
/// the director, heal the keep. Runs when a new run begins (start-screen / game-over exit).
/// Fire the hero's "night is coming" line once, when the prep day has ~15 s left. Gated per
/// prep day by the *next* wave index so each night gets exactly one warning.
fn night_warning(
    siege: Res<Siege>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut warned_wave: Local<i32>,
) {
    if siege.phase == GamePhase::Prep
        && siege.prep_seconds_left > 0.0
        && siege.prep_seconds_left <= 15.0
    {
        let next_wave = siege.wave_index + 1;
        if *warned_wave != next_wave {
            *warned_wave = next_wave;
            speak.write(crate::audio::Speak::new(crate::audio::Concept::NightWarning));
        }
    }
}

fn reset_siege(
    mut siege: ResMut<Siege>,
    mut keep: ResMut<KeepHp>,
    invaders: Query<Entity, With<WaveInvader>>,
    mut commands: Commands,
) {
    for e in &invaders {
        commands.entity(e).try_despawn();
    }
    let diff = siege.difficulty;
    *siege = Siege { difficulty: diff, ..Siege::default() };
    // Re-derive the keep's max from base × the difficulty handicap (so switching difficulty between
    // runs takes effect, and the Easy keep is genuinely tougher).
    keep.max = KEEP_MAX_HP * mods_for(diff).keep_hp_mul;
    keep.hp = keep.max;
}

fn setup_invader_armory(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.insert_resource(InvaderArmory(orks::Armory::new(&mut meshes, &mut materials, mat)));
}

/// Invader spawn footing: **flat** land only (height class 1, Y≈0). The world-30 spawn ring clips
/// the inner edge of two low plateaus (base (52,50) and (90,36)); a terrace-top spawn strands the
/// ork on the elevation — it mills there instead of marching the keep ("orks stuck on elevations")
/// — so the ring keeps only flat tiles, which near the castle are all in the keep's walkable
/// component. (Heights are discrete 0.5·(class-1) steps, so `y <= 0.05` ⟺ class 1.)
fn spawn_footing(x: f32, z: f32) -> bool {
    ground_at_world(x, z).is_some_and(|y| y <= 0.05)
}

/// Spawn one invader at `ring_index` on the spawn ring, with `hp` HP — home-anchored at the keep
/// (unused by its brain) and tagged [`WaveInvader`] + [`Health`] in the same command flush (so
/// the hero's `ensure_combat_health` never overwrites the scaled HP).
fn spawn_invader(
    commands: &mut Commands,
    armory: &orks::Armory,
    variant: OrkVariant,
    hp: f32,
    ring_index: u32,
    now: f32,
) {
    let p = spawn_point(ring_index, KEEP_POS, SPAWN_RING, spawn_footing);
    let seed = ring_index.wrapping_mul(0x9e37_79b1) ^ (ring_index + 1);
    let e = armory.spawn(commands, variant, INVADER_FACTION, KEEP_POS, p, seed);
    commands.entity(e).insert((
        WaveInvader { closest: p.distance(KEEP_POS), progress_at: now },
        crate::navgrid::NavPath::default(),
        Health { hp, max: hp },
    ));
}

/// The assault director: counts down prep, spawns the wave on an interval, advances to the next
/// wave / victory on a clear, and trips defeat if the keep is razed. Decision logic is the pure
/// [`step_wave_director`]; this feeds it world state and applies its actions.
#[allow(clippy::too_many_arguments)]
fn run_director(
    time: Res<Time>,
    game: Res<GameTime>,
    mut siege: ResMut<Siege>,
    mut keep: ResMut<KeepHp>,
    mut player: ResMut<crate::player::PlayerRes>,
    eco: Res<crate::economy::EconomyState>,
    mut town: ResMut<crate::town::TownRes>,
    armory: Option<Res<InvaderArmory>>,
    invaders: Query<Entity, With<WaveInvader>>,
    alive_invaders: Query<(), (With<WaveInvader>, Without<crate::dying::Dying>)>,
    mut commands: Commands,
) {
    let now = game.0; // pause-aware clock (NOT elapsed_secs, which ticks through pauses)
    let dt = time.delta_secs();

    // Slow keep self-repair across the prep breather.
    if siege.phase == GamePhase::Prep && keep.hp < keep.max {
        keep.hp = (keep.hp + KEEP_REPAIR_RATE * dt).min(keep.max);
    }

    // Keep razed → defeat: clear the field and freeze.
    if siege.phase == GamePhase::Wave && keep.hp <= 0.0 {
        siege.phase = GamePhase::Defeat;
        for e in &invaders {
            commands.entity(e).try_despawn();
        }
        siege.prep_seconds_left = 0.0;
        return;
    }
    if matches!(siege.phase, GamePhase::Victory | GamePhase::Defeat) {
        return; // end states are frozen (reset with R)
    }

    let alive = alive_invaders.iter().count() as u32; // fading corpses don't count → wave clears
    let mods = mods_for(siege.difficulty);
    let res = step_wave_director(&WaveStepInput {
        phase: siege.phase,
        wave_index: siege.wave_index,
        spawned: siege.spawned,
        alive,
        timers: siege.timers,
        now,
        skip: siege.skip_requested,
        mods,
    });
    siege.timers = res.timers;
    siege.skip_requested = false;

    for action in res.actions {
        match action {
            WaveAction::BeginWave { index } => {
                siege.wave_index = index as i32;
                siege.spawned = 0;
            }
            WaveAction::SetPhase(p) => {
                siege.phase = p;
                // Wave→Prep is a clear: shore up the keep + pay the Tax Office stipend.
                if p == GamePhase::Prep {
                    // Dawn repair: +20% of max HP, guaranteed each new day (on top of the slow
                    // continuous prep repair above).
                    keep.hp = (keep.hp + keep.max * KEEP_DAWN_REPAIR_FRAC).min(keep.max);
                    if eco.tax_office {
                        player.0.add_gold(crate::economy::TAX_STIPEND);
                    }
                    // Dawn: a youth comes of age — heirs ARE townsfolk (one headcount), so the
                    // bloodline grows by growing the town. Gated by housing: people aren't
                    // conjured from nothing — no roof, no new townsperson.
                    if town.0.population < town.0.pop_cap() {
                        town.0.population += 1;
                    }
                }
                if p == GamePhase::Victory {
                    for e in &invaders {
                        commands.entity(e).try_despawn();
                    }
                }
            }
            WaveAction::Spawn { variant, hp, spawn_index, wave_index } => {
                if let Some(arm) = armory.as_deref() {
                    // Offset the ring index per wave so successive nights don't reuse the same arc.
                    let ring_index = spawn_index + wave_index as u32 * 7;
                    spawn_invader(&mut commands, &arm.0, variant, hp, ring_index, now);
                    siege.spawned += 1;
                }
            }
        }
    }

    siege.prep_seconds_left = if siege.phase == GamePhase::Prep && siege.timers.prep_ends_at > 0.0 {
        (siege.timers.prep_ends_at - now).max(0.0)
    } else {
        0.0
    };
}

/// Night-wave invader AI: march on the keep from the ring (no leash), intercept the hero if he
/// strays into range, batter the keep (or the hero) on the strike cooldown, and reap if stuck
/// far out. Reuses the camp ork's tuning + steering; distinct from the leashed [`orks::ork_brain`].
#[allow(clippy::too_many_arguments)]
fn invader_brain(
    time: Res<Time>,
    game: Res<GameTime>,
    hero: Res<HeroState>,
    siege: Res<Siege>,
    mut keep: ResMut<KeepHp>,
    mut pending: ResMut<PendingHeroDamage>,
    mut guard_dmg: ResMut<crate::villagers::NpcDamage>,
    mut bolts: ResMut<BoltSpawns>,
    mut commands: Commands,
    town: Res<crate::town::TownRes>,
    plot_spots: Res<crate::town::PlotSpots>,
    mut building_dmg: ResMut<crate::town::PendingBuildingDamage>,
    guards: Query<
        (Entity, &Transform),
        (With<crate::villagers::Guard>, Without<WaveInvader>, Without<crate::dying::Dying>),
    >,
    mut q: Query<
        (
            Entity,
            &mut orks::Ork,
            &mut WaveInvader,
            &mut crate::navgrid::NavPath,
            &mut Transform,
            &Health,
        ),
        Without<crate::dying::Dying>,
    >,
) {
    if siege.phase != GamePhase::Wave {
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let now = game.0; // pause-aware clock for logic (replan throttle, stuck-net)
    let rnow = time.elapsed_secs(); // raw clock for visuals/corpse-fade (matches ork_limbs & dying.rs)

    // Living guards this frame (the dying are already filtered out — death is permanent now).
    let live_guards: Vec<(Entity, Vec2)> = guards
        .iter()
        .map(|(e, tf)| (e, Vec2::new(tf.translation.x, tf.translation.z)))
        .collect();
    let guard_positions: Vec<Vec2> = live_guards.iter().map(|(_, p)| *p).collect();

    for (e, mut o, mut inv, mut path, mut tf, hp) in &mut q {
        o.atk_cd -= dt;
        // Berserker frenzy: faster march + quicker strikes under 40% HP (incl. the boss).
        let frenzied = o.variant == OrkVariant::Berserker && hp.hp < hp.max * 0.4;
        let dist_keep = o.pos.distance(KEEP_POS);
        // Target priority: invaders treat the town-guards LIKE the hero — they fight whichever of
        // the two threats is nearer, rather than tunnelling on the hero and walking past the
        // guards. Only with neither hero nor guard in range do they press on to the keep.
        let hero_d = if hero.alive { o.pos.distance(hero.pos) } else { f32::INFINITY };
        let see_hero = hero.alive && hero_d < orks::ORK_SIGHT;
        let guard_near = nearest_guard_in_range(o.pos, &guard_positions, GUARD_ENGAGE);
        let guard_d = guard_near.map_or(f32::INFINITY, |i| o.pos.distance(guard_positions[i]));
        // Take a guard when one is in range and either no hero is near or the guard is the closer.
        let guard_tgt: Option<(Entity, Vec2)> = if guard_near.is_some() && (!see_hero || guard_d <= hero_d) {
            guard_near.map(|i| live_guards[i])
        } else {
            None
        };
        let chase_hero = see_hero && guard_tgt.is_none();
        // FORGIVING slice tuning: only ~1/3 of the warband (by id) are arsonists; the
        // rest still rush the keep. They make for the nearest standing building. The
        // keep's existing defenses (towers/archers/ballista) already auto-target ANY
        // WaveInvader, so arsonists get shot on approach — no extra wiring needed.
        let arsonist = (e.to_bits() % 3) == 0;
        let building_goal: Option<(usize, Vec2)> = if arsonist {
            let mut best: Option<(usize, f32)> = None;
            for (idx, spot) in plot_spots.0.iter().enumerate() {
                if town.0.plots.get(idx).map_or(false, |p| p.is_built()) {
                    let d = o.pos.distance(*spot);
                    if best.map_or(true, |(_, bd)| d < bd) {
                        best = Some((idx, d));
                    }
                }
            }
            best.map(|(i, _)| (i, plot_spots.0[i]))
        } else {
            None
        };
        let target = if let Some((_, gp)) = guard_tgt {
            gp
        } else if chase_hero {
            hero.pos
        } else if let Some((bidx, bpos)) = building_goal {
            // Batter the building when in range; else march toward it.
            if o.pos.distance(bpos) < BUILDING_ATTACK_RANGE {
                building_dmg.0.push((bidx, BUILDING_DPS * dt));
            }
            bpos
        } else {
            KEEP_POS
        };
        let atk_range = if o.shaman { orks::SHAMAN_CAST_RANGE } else { orks::ORK_ATTACK_RANGE };
        let at_hero = chase_hero && o.pos.distance(hero.pos) < atk_range;
        let at_guard = guard_tgt.is_some_and(|(_, gp)| o.pos.distance(gp) < atk_range);
        let at_keep = !chase_hero && guard_tgt.is_none() && dist_keep <= KEEP_ATTACK_RANGE;
        // Chasing a target (hero/guard) uses cheap direct steering; only the keep march paths A*.
        // Arsonists with a building goal also steer directly toward `target` (= the building, in
        // the open safe-zone outside the walls) instead of running the keep A* — otherwise they'd
        // ignore the building and march the keep.
        let chase_direct = chase_hero || guard_tgt.is_some() || building_goal.is_some();

        if at_hero || at_guard || at_keep {
            o.moving = false;
            // Turn to face the target at a capped rate.
            let to = target - o.pos;
            if to.length_squared() > 1e-4 {
                let want = to.x.atan2(to.y);
                let turn = (orks::ORK_MAX_TURN * 2.0 * dt).abs();
                o.facing += steer::wrap_pi(want - o.facing).clamp(-turn, turn);
            }
            if o.atk_cd <= 0.0 {
                let frenzy_cd = if frenzied { 0.6 } else { 1.0 };
                o.atk_anim = rnow; // play the club-chop / staff-jab (keep, guard, or hero)
                if at_hero {
                    if o.shaman {
                        o.atk_cd = orks::SHAMAN_CAST_CD;
                        let gy = ground_at_world(o.pos.x, o.pos.y).unwrap_or(0.0);
                        bolts.0.push(BoltSpawn {
                            origin: Vec3::new(o.pos.x, gy + 1.4, o.pos.y),
                            damage: orks::SHAMAN_BOLT_DAMAGE,
                        });
                    } else {
                        o.atk_cd = orks::ORK_ATTACK_CD * frenzy_cd;
                        pending.0 += orks::variant_melee(o.variant);
                    }
                } else if at_guard {
                    // Trading blows with a town-guard (armour blunts the hit).
                    o.atk_cd =
                        if o.shaman { orks::SHAMAN_CAST_CD } else { orks::ORK_ATTACK_CD * frenzy_cd };
                    if let Some((ge, _)) = guard_tgt {
                        let dmg = orks::variant_melee(o.variant) * crate::villagers::GUARD_ARMOR_MULT;
                        guard_dmg.0.push(crate::villagers::NpcHit {
                            victim: ge,
                            amount: dmg,
                            attacker: Some(e),
                        });
                    }
                } else {
                    // Hammering the keep.
                    o.atk_cd =
                        if o.shaman { orks::SHAMAN_CAST_CD } else { orks::ORK_ATTACK_CD * frenzy_cd };
                    keep.hp = (keep.hp - KEEP_DAMAGE).max(0.0);
                }
            }
        } else {
            // Pick the immediate step target. Marching the KEEP follows an A* route through the
            // gates (replanned on a throttle); chasing the hero/guard stays cheap direct steering
            // (they're close and move every frame, so pathing them is churn).
            let step_target = if chase_direct {
                target
            } else {
                // A* the keep march to a STANDABLE point just inside the nearest gate — the keep
                // origin sits inside the solid keep box, so pathing straight to it fails and the
                // invader wedges at the wall ("just there, can't approach"). The gate-interior
                // goal threads them through the gap into batter range.
                let keep_goal = keep_march_goal(o.pos);
                if path.cursor >= path.waypoints.len()
                    || now >= path.next_replan
                    || path.goal_cached.distance(keep_goal) > 2.0
                {
                    path.waypoints = crate::navgrid::path_to(o.pos, keep_goal);
                    path.cursor = 0;
                    path.goal_cached = keep_goal;
                    // Stagger replans across the horde so they don't all path on one frame.
                    path.next_replan = now + 0.75 + (e.to_bits() % 16) as f32 * 0.05;
                }
                while path.cursor < path.waypoints.len()
                    && o.pos.distance(path.waypoints[path.cursor]) < 1.2
                {
                    path.cursor += 1;
                }
                // Next waypoint, or the gate-interior aim if there's no route (fallback — the
                // stuck-net still reaps a wedged invader).
                path.waypoints.get(path.cursor).copied().unwrap_or(keep_goal)
            };

            // March toward the step target, steering around props/cliffs (faster than a patrol).
            let cur_y = ground_at_world(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
            let speed = o.speed * 1.4 * if frenzied { 1.4 } else { 1.0 };
            match steer::advance(o.pos, o.facing, step_target, speed * dt, o.body_r, cur_y, orks::ORK_MAX_TURN * 1.6 * dt) {
                Some(s) => {
                    o.facing = s.facing;
                    o.pos = s.pos;
                    o.moving = s.moving;
                }
                None => o.moving = false,
            }
        }

        // Stuck-ork safety net — progress-based: a keep-marcher that hasn't gotten closer to the
        // keep within the timeout is wedged (oscillating round a prop/wall, or gate-flip thrash)
        // and fades out, so the wave can't hang. See `stuck_step`.
        // `engaged` resets the clock for legitimate non-keep-progress behaviour, but ONLY when it's
        // real activity — *attacking* something (incl. battering an off-keep building) or *actually
        // advancing* toward a hero/guard/building this frame. A wedged ork that merely INTENDS to
        // reach a building/hero but is frozen (steering boxed in → `!o.moving`) must still time out;
        // keying `engaged` on intent alone left a frozen arsonist immune forever, hanging the wave.
        let at_building =
            building_goal.is_some_and(|(_, bp)| o.pos.distance(bp) < BUILDING_ATTACK_RANGE);
        let attacking = at_hero || at_guard || at_keep || at_building;
        let pursuing = chase_hero || guard_tgt.is_some() || building_goal.is_some();
        let engaged = attacking || (pursuing && o.moving);
        let (closest, progress_at, reap) = stuck_step(inv.closest, inv.progress_at, dist_keep, engaged, now);
        inv.closest = closest;
        inv.progress_at = progress_at;
        if reap {
            crate::dying::begin_dying(&mut commands, e, rnow); // a wedged invader fades out
            continue;
        }

        orks::apply_knockback(&mut o, dt);

        let gy = ground_at_world(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
        tf.translation = Vec3::new(o.pos.x, gy, o.pos.y);
        // Springy recoil-wobble on a blow taken — same as the camp orks.
        tf.rotation = Quat::from_rotation_y(o.facing) * Quat::from_rotation_x(orks::recoil_tilt(o.hit_recoil, rnow));
    }
}

/// Keyboard (prep only): **B** rings the war bell (summon the night early), **G** cycles
/// difficulty. Restart is handled by the game-over screen (Enter → `OnExit(GameOver)` reset),
/// so there is no `R` keybind. Gated to `Playing` (no panel) by the plugin's run condition.
fn siege_controls(keys: Res<ButtonInput<KeyCode>>, mut siege: ResMut<Siege>) {
    if keys.just_pressed(KeyCode::KeyB) && siege.phase == GamePhase::Prep {
        siege.skip_requested = true; // floored to MIN_PREP_SECONDS inside the reducer
    }
    if keys.just_pressed(KeyCode::KeyG) && siege.phase == GamePhase::Prep {
        siege.difficulty = match siege.difficulty {
            Difficulty::Easy => Difficulty::Normal,
            Difficulty::Normal => Difficulty::Hard,
            Difficulty::Hard => Difficulty::Easy,
        };
    }
}

// ── Objective banner (top-centre), ported from the 3js `Objective` ───────────────────

#[derive(Component)]
struct KeepHpFill;
#[derive(Component)]
struct PhaseFill;
#[derive(Component)]
struct PhaseText;
#[derive(Component)]
struct SubText;
#[derive(Component)]
struct HeirText;

fn setup_siege_hud(mut commands: Commands, fonts: Res<UiFonts>) {
    // Full-width wrapper centres the banner card horizontally.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|wrap| {
            wrap.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(5.0),
                    min_width: Val::Px(260.0),
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                    border: border(1.0),
                    border_radius: radius(R_CARD),
                    ..default()
                },
                BackgroundColor(PANEL_HUD),
                BorderColor::all(BORDER_SOFT),
                shadow_hud(),
                anim(AnimKind::SlideDown, 0.0, 0.36),
            ))
            .with_children(|card| {
                card.spawn((label(&fonts.bold, "PREPARE", 12.0, GOLD), PhaseText));
                card.spawn((label(&fonts.semibold, "", 12.0, TEXT_DIM), SubText));
                // Phase progress bar (prep day drains / wave horde remaining).
                card.spawn((
                    Node { width: Val::Px(180.0), height: Val::Px(6.0), border_radius: radius(3.0), overflow: Overflow::clip(), ..default() },
                    BackgroundColor(rgba(0, 0, 0, 0.45)),
                ))
                .with_children(|t| {
                    t.spawn((
                        Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                        BackgroundColor(GOLD_DEEP),
                        PhaseFill,
                    ));
                });
                // Keep HP row.
                card.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() })
                    .with_children(|row| {
                        row.spawn(label(&fonts.semibold, "KEEP", 10.0, GREY));
                        row.spawn((
                            Node { width: Val::Px(160.0), height: Val::Px(8.0), border: border(1.0), border_radius: radius(4.0), overflow: Overflow::clip(), ..default() },
                            BackgroundColor(rgba(0, 0, 0, 0.45)),
                            BorderColor::all(rgba(255, 255, 255, 0.25)),
                        ))
                        .with_children(|t| {
                            t.spawn((
                                Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                                widgets::vgrad(rgb(111, 208, 255), rgb(42, 143, 214)),
                                KeepHpFill,
                            ));
                        });
                    });
                card.spawn((label(&fonts.bold, "", 11.0, rgb(159, 211, 160)), HeirText));
            });
        });
}

#[allow(clippy::type_complexity)]
fn update_siege_hud(
    siege: Res<Siege>,
    keep: Res<KeepHp>,
    lives: Res<crate::succession::Lives>,
    invaders: Query<&WaveInvader, Without<crate::dying::Dying>>,
    mut keep_q: Query<&mut Node, (With<KeepHpFill>, Without<PhaseFill>)>,
    mut phase_q: Query<(&mut Node, &mut BackgroundColor), (With<PhaseFill>, Without<KeepHpFill>)>,
    mut ptext: Query<&mut Text, (With<PhaseText>, Without<SubText>, Without<HeirText>)>,
    mut stext: Query<&mut Text, (With<SubText>, Without<HeirText>)>,
    mut htext: Query<&mut Text, With<HeirText>>,
) {
    if let Ok(mut n) = keep_q.single_mut() {
        n.width = Val::Percent((keep.hp / keep.max * 100.0).clamp(0.0, 100.0));
    }
    let total = WAVES.len();
    let (label_s, sub_s) = match siege.phase {
        GamePhase::Prep => {
            let night = (siege.wave_index + 2).max(1);
            let secs = siege.prep_seconds_left.max(0.0) as i64;
            (format!("PREPARE — NIGHT {night} / {total}"), format!("{}:{:02} until nightfall", secs / 60, secs % 60))
        }
        GamePhase::Wave => {
            let night = (siege.wave_index + 1).max(1);
            let alive = invaders.iter().count();
            (format!("NIGHT {night} / {total}"), format!("{alive} orks remain"))
        }
        GamePhase::Victory => ("VICTORY".into(), String::new()),
        GamePhase::Defeat => ("THE KEEP HAS FALLEN".into(), String::new()),
    };
    if let Ok(mut t) = ptext.single_mut() {
        **t = label_s;
    }
    if let Ok(mut t) = stext.single_mut() {
        **t = sub_s;
    }
    if let Ok(mut t) = htext.single_mut() {
        **t = format!("{} heir{} in reserve", lives.heirs, if lives.heirs == 1 { "" } else { "s" });
    }

    let Ok((mut n, mut col)) = phase_q.single_mut() else { return };
    match siege.phase {
        GamePhase::Prep => {
            let p = prep_progress(siege.prep_seconds_left, mods_for(siege.difficulty));
            n.width = Val::Percent(((1.0 - p) * 100.0).clamp(0.0, 100.0)); // full day → drains to night
            col.0 = GOLD_DEEP;
        }
        GamePhase::Wave => {
            let count = effective_count(siege.wave_index.max(0) as usize, mods_for(siege.difficulty));
            let alive = invaders.iter().count() as u32;
            n.width = Val::Percent((alive as f32 / count as f32 * 100.0).clamp(0.0, 100.0));
            col.0 = RED;
        }
        GamePhase::Victory => {
            n.width = Val::Percent(100.0);
            col.0 = rgb(102, 217, 102);
        }
        GamePhase::Defeat => {
            n.width = Val::Percent(100.0);
            col.0 = rgb(77, 77, 77);
        }
    }
}

/// Screenshot hook: with `FOREST_WAVE` set, pre-spawn the whole Night-1 horde at the ring and
/// jump straight into the wave, so a `FOREST_SHOT` frame shows the night assault.
fn seed_demo_wave(mut siege: ResMut<Siege>, armory: Option<Res<InvaderArmory>>, mut commands: Commands) {
    if std::env::var("FOREST_WAVE").is_err() {
        return;
    }
    let Some(arm) = armory.as_deref() else { return };
    let wave_index = 0usize;
    let mods = mods_for(siege.difficulty);
    let count = effective_count(wave_index, mods);
    let def = &WAVES[wave_index];
    for k in 0..count {
        let variant = def.variants[k as usize % def.variants.len()];
        let hp = (base_hp(variant) * def.hp_scale * mods.hp_mul).round();
        spawn_invader(&mut commands, &arm.0, variant, hp, k + wave_index as u32 * 7, 0.0);
    }
    siege.phase = GamePhase::Wave;
    siege.wave_index = wave_index as i32;
    siege.spawned = count;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normal() -> DiffMods {
        mods_for(Difficulty::Normal)
    }

    #[test]
    fn difficulty_presets() {
        // Normal is the unscaled baseline (every multiplier 1.0, no spare heirs).
        let n = mods_for(Difficulty::Normal);
        assert_eq!(
            n,
            DiffMods { count_mul: 1.0, hp_mul: 1.0, prep_mul: 1.0, player_hp_mul: 1.0, keep_hp_mul: 1.0, heirs_bonus: 0 }
        );
        // Easy softens the orks AND buffs the hero/keep/heirs so a beginner can survive.
        let e = mods_for(Difficulty::Easy);
        assert!(e.count_mul < 1.0 && e.hp_mul < 1.0 && e.prep_mul > 1.0);
        assert!(e.player_hp_mul > 1.0 && e.keep_hp_mul > 1.0 && e.heirs_bonus > 0);
        // Hard does the reverse (more/tougher orks, shorter day, frailer keep).
        let h = mods_for(Difficulty::Hard);
        assert!(h.count_mul > 1.0 && h.hp_mul > 1.0 && h.prep_mul < 1.0 && h.keep_hp_mul < 1.0);
    }

    #[test]
    fn effective_count_scales_and_floors_at_one() {
        assert_eq!(effective_count(0, normal()), 6);
        assert_eq!(effective_count(0, mods_for(Difficulty::Hard)), 8); // round(6·1.25=7.5)=8
        let boss = WAVES.len() - 1; // count 1; easy round(0.8)=1 floored, never 0
        assert_eq!(effective_count(boss, mods_for(Difficulty::Easy)), 1);
    }

    #[test]
    fn wave_table_has_eight_waves_with_boss_last() {
        assert_eq!(WAVES.len(), 8);
        let boss = &WAVES[7];
        assert_eq!(boss.count, 1);
        assert_eq!(boss.hp_scale, 14.0);
        assert_eq!(boss.variants, &[OrkVariant::Berserker]);
    }

    #[test]
    fn prep_arms_timer_then_begins_wave_at_expiry() {
        let r1 = step_wave_director(&WaveStepInput {
            phase: GamePhase::Prep, wave_index: -1, spawned: 0, alive: 0,
            timers: WaveTimers::default(), now: 0.0, skip: false, mods: normal(),
        });
        assert!(r1.actions.is_empty(), "no transition on the arming frame");
        assert_eq!(r1.timers.prep_ends_at, PREP_DURATION); // 0 + PREP_DURATION·1 (normal prep_mul)

        let r2 = step_wave_director(&WaveStepInput {
            phase: GamePhase::Prep, wave_index: -1, spawned: 0, alive: 0,
            timers: r1.timers, now: PREP_DURATION, skip: false, mods: normal(),
        });
        assert_eq!(
            r2.actions,
            vec![WaveAction::BeginWave { index: 0 }, WaveAction::SetPhase(GamePhase::Wave)]
        );
        assert_eq!(r2.timers.prep_ends_at, 0.0);
        assert_eq!(r2.timers.spawn_index, 0);
    }

    #[test]
    fn prep_skip_ignored_before_min_seconds_then_honored() {
        let armed = WaveTimers { prep_ends_at: PREP_DURATION, next_spawn_at: 0.0, spawn_index: 0 };
        let early = step_wave_director(&WaveStepInput {
            phase: GamePhase::Prep, wave_index: -1, spawned: 0, alive: 0,
            timers: armed, now: 1.0, skip: true, mods: normal(),
        });
        assert!(early.actions.is_empty(), "skip floored for the first MIN_PREP_SECONDS");

        let ok = step_wave_director(&WaveStepInput {
            phase: GamePhase::Prep, wave_index: -1, spawned: 0, alive: 0,
            timers: armed, now: 5.0, skip: true, mods: normal(),
        });
        assert_eq!(ok.actions[0], WaveAction::BeginWave { index: 0 });
    }

    #[test]
    fn wave_spawns_on_interval_round_robin_variant() {
        let r = step_wave_director(&WaveStepInput {
            phase: GamePhase::Wave, wave_index: 0, spawned: 0, alive: 0,
            timers: WaveTimers { prep_ends_at: 0.0, next_spawn_at: 0.0, spawn_index: 0 },
            now: 0.0, skip: false, mods: normal(),
        });
        match r.actions[0] {
            WaveAction::Spawn { variant, spawn_index, wave_index, .. } => {
                assert_eq!(variant, WAVES[0].variants[0]);
                assert_eq!(spawn_index, 0);
                assert_eq!(wave_index, 0);
            }
            ref a => panic!("expected Spawn, got {a:?}"),
        }
        assert_eq!(r.timers.spawn_index, 1);
        assert_eq!(r.timers.next_spawn_at, WAVES[0].spawn_interval);
    }

    #[test]
    fn wave_does_not_spawn_before_interval_elapses() {
        let r = step_wave_director(&WaveStepInput {
            phase: GamePhase::Wave, wave_index: 0, spawned: 1, alive: 1,
            timers: WaveTimers { prep_ends_at: 0.0, next_spawn_at: 5.0, spawn_index: 1 },
            now: 1.0, skip: false, mods: normal(),
        });
        assert!(r.actions.is_empty());
    }

    #[test]
    fn wave_clear_returns_to_prep_midgame() {
        let count = effective_count(0, normal());
        let r = step_wave_director(&WaveStepInput {
            phase: GamePhase::Wave, wave_index: 0, spawned: count, alive: 0,
            timers: WaveTimers { prep_ends_at: 0.0, next_spawn_at: 0.0, spawn_index: count },
            now: 99.0, skip: false, mods: normal(),
        });
        assert_eq!(r.actions, vec![WaveAction::SetPhase(GamePhase::Prep)]);
    }

    #[test]
    fn last_wave_clear_is_victory() {
        let last = WAVES.len() - 1;
        let count = effective_count(last, normal());
        let r = step_wave_director(&WaveStepInput {
            phase: GamePhase::Wave, wave_index: last as i32, spawned: count, alive: 0,
            timers: WaveTimers { prep_ends_at: 0.0, next_spawn_at: 0.0, spawn_index: count },
            now: 99.0, skip: false, mods: normal(),
        });
        assert_eq!(r.actions, vec![WaveAction::SetPhase(GamePhase::Victory)]);
    }

    #[test]
    fn spawn_hp_scales_with_wave_and_difficulty() {
        let r = step_wave_director(&WaveStepInput {
            phase: GamePhase::Wave, wave_index: 0, spawned: 0, alive: 0,
            timers: WaveTimers::default(), now: 0.0, skip: false, mods: normal(),
        });
        if let WaveAction::Spawn { hp, variant, .. } = r.actions[0] {
            assert_eq!(hp, (base_hp(variant) * WAVES[0].hp_scale).round());
        } else {
            panic!("expected a Spawn action");
        }
    }

    #[test]
    fn boss_hp_is_base_times_scale() {
        let last = WAVES.len() - 1;
        let r = step_wave_director(&WaveStepInput {
            phase: GamePhase::Wave, wave_index: last as i32, spawned: 0, alive: 0,
            timers: WaveTimers::default(), now: 0.0, skip: false, mods: normal(),
        });
        if let WaveAction::Spawn { hp, variant, .. } = r.actions[0] {
            assert_eq!(variant, OrkVariant::Berserker);
            assert_eq!(hp, (base_hp(OrkVariant::Berserker) * 14.0).round());
        } else {
            panic!("expected a Spawn action");
        }
    }

    #[test]
    fn prep_progress_runs_zero_to_one() {
        let m = normal();
        assert_eq!(prep_progress(PREP_DURATION, m), 0.0); // full day left → sun at dawn
        assert_eq!(prep_progress(0.0, m), 1.0); // none left → sun at dusk
        assert!((prep_progress(PREP_DURATION / 2.0, m) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn spawn_point_marches_to_furthest_standable_tile() {
        let keep = Vec2::ZERO;
        let all = spawn_point(0, keep, 30.0, |_, _| true);
        assert!(all.length() > 6.0 && all.length() <= 31.0, "reaches out toward the ring");

        let blocked = spawn_point(0, keep, 30.0, |x, z| Vec2::new(x, z).length() <= 8.0);
        assert!(blocked.length() <= 9.0, "stays on the last standable tile when the sea blocks the ray");
    }

    #[test]
    fn spawn_points_spread_by_golden_angle() {
        let keep = Vec2::ZERO;
        let a = spawn_point(0, keep, 30.0, |_, _| true);
        let b = spawn_point(1, keep, 30.0, |_, _| true);
        assert!(a.distance(b) > 1.0, "successive spawns don't stack");
    }

    #[test]
    fn invader_diverts_onto_the_nearest_guard_in_range() {
        let from = Vec2::new(0.0, 0.0);
        let guards = [Vec2::new(3.0, 0.0), Vec2::new(6.0, 0.0)];
        // Both inside GUARD_ENGAGE (8) → pick the closer (index 0).
        assert_eq!(nearest_guard_in_range(from, &guards, GUARD_ENGAGE), Some(0));
        // Closer guard now further than the second → pick index 1.
        let guards2 = [Vec2::new(7.5, 0.0), Vec2::new(2.0, 0.0)];
        assert_eq!(nearest_guard_in_range(from, &guards2, GUARD_ENGAGE), Some(1));
    }

    #[test]
    fn keep_march_goal_lands_inside_batter_range() {
        // From any spawn-ring bearing, the gate-interior goal sits within KEEP_ATTACK_RANGE of the
        // keep (so reaching it = battering it) and off the keep origin (a valid, standable A* goal).
        for i in 0..16 {
            let a = i as f32 * 0.4;
            let from = Vec2::new(a.cos(), a.sin()) * SPAWN_RING;
            let g = keep_march_goal(from);
            assert!(g.distance(KEEP_POS) <= KEEP_ATTACK_RANGE, "goal {g:?} out of batter range");
            assert!(g.distance(KEEP_POS) > 2.0, "goal must clear the keep box");
        }
    }

    #[test]
    fn stuck_step_resets_clock_on_progress() {
        // Got 1u closer (> EPS) → progress: closest drops, clock resets to now, no reap.
        let (c, t, reap) = stuck_step(10.0, 0.0, 9.0, false, 5.0);
        assert_eq!((c, t, reap), (9.0, 5.0, false));
    }

    #[test]
    fn stuck_step_reaps_a_net_zero_oscillator() {
        // The regression the old movement-based net missed: an ork that keeps moving (so the old
        // idle clock kept resetting) but never beats its closest approach. No gain past timeout → reap.
        let closest = 10.0;
        // Wobbling just shy of `closest` — never < closest - EPS, so never counts as progress.
        let (_, _, before) = stuck_step(closest, 0.0, 10.2, false, STUCK_TIMEOUT - 1.0);
        assert!(!before, "not yet timed out");
        let (_, _, after) = stuck_step(closest, 0.0, 10.2, false, STUCK_TIMEOUT);
        assert!(after, "net-zero progress past the timeout must reap");
    }

    #[test]
    fn stuck_step_never_reaps_while_engaged() {
        // Chasing the hero far from the keep (dist_keep growing) past the timeout must NOT reap —
        // engagement is intended behaviour, not stuck. Clock resets even with no keep-ward gain.
        let (c, t, reap) = stuck_step(8.0, 0.0, 25.0, true, STUCK_TIMEOUT + 10.0);
        assert_eq!((c, t, reap), (8.0, STUCK_TIMEOUT + 10.0, false));
    }

    #[test]
    fn invaders_spawn_on_flat_keep_connected_ground() {
        // Regression for "orks stuck on elevations": the world-30 spawn ring clips the inner
        // edge of two low plateaus (base (52,50) r7 and (90,36) r7), so the old `is_some()`
        // footing dropped invaders onto a terrace top where they milled instead of marching the
        // keep. `spawn_footing` keeps only flat (class-1, Y≈0) ring tiles — all in the keep's
        // walkable component. Sweep every golden-angle bearing and assert none land elevated.
        for i in 0..400u32 {
            let p = spawn_point(i, KEEP_POS, SPAWN_RING, spawn_footing);
            let y = crate::worldmap::ground_at_world(p.x, p.y).expect("spawn lands on land");
            assert!(y <= 0.05, "spawn #{i} at ({:.1},{:.1}) elevated Y={y:.2}", p.x, p.y);
        }
    }

    #[test]
    fn invader_ignores_guards_outside_engage_range() {
        let from = Vec2::ZERO;
        let far = [Vec2::new(9.0, 0.0), Vec2::new(0.0, 12.0)]; // both > GUARD_ENGAGE (8)
        assert_eq!(nearest_guard_in_range(from, &far, GUARD_ENGAGE), None);
        // No guards at all → march the keep.
        assert_eq!(nearest_guard_in_range(from, &[], GUARD_ENGAGE), None);
    }
}
