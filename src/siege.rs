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
use crate::worldmap::ground_at_world;

// ── Tuning (ported from waveStore.ts) ──────────────────────────────────────────────

/// Seconds the prep "day" lasts at Normal difficulty (the explore/rebuild breather).
pub const PREP_DURATION: f32 = 150.0;
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

/// Per-variant base HP for a wave ork, in this scene's combat units (the camp ork is a flat
/// 60 HP; these keep the original `orkConfig.ts` *ratios* — scout frail, berserker beefy —
/// anchored so a Night-1 grunt matches a camp grunt). Scaled by `hp_scale` × the difficulty
/// `hp_mul` at spawn.
pub fn base_hp(v: OrkVariant) -> f32 {
    match v {
        OrkVariant::Grunt => 60.0,
        OrkVariant::Scout => 32.0,
        OrkVariant::Berserker => 72.0,
        OrkVariant::Shaman => 47.0,
    }
}

// ── Difficulty (ported from difficultyStore.ts) ─────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

/// Multipliers applied to wave count / ork HP / prep duration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffMods {
    pub count_mul: f32,
    pub hp_mul: f32,
    pub prep_mul: f32,
}

/// easy = fewer/softer orks + a longer day · normal = the tuned baseline · hard = the reverse.
pub fn mods_for(d: Difficulty) -> DiffMods {
    match d {
        Difficulty::Easy => DiffMods { count_mul: 0.8, hp_mul: 0.85, prep_mul: 1.25 },
        Difficulty::Normal => DiffMods { count_mul: 1.0, hp_mul: 1.0, prep_mul: 1.0 },
        Difficulty::Hard => DiffMods { count_mul: 1.25, hp_mul: 1.2, prep_mul: 0.8 },
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
const KEEP_DAMAGE: f32 = 6.0;
/// Keep total HP — tuned so an unopposed early wave threatens it over the night and a late wave
/// razes it fast, but the hero thinning the horde saves it.
pub const KEEP_MAX_HP: f32 = 1000.0;
/// Keep self-repair during the prep breather (HP/s) — the day is a chance to recover.
const KEEP_REPAIR_RATE: f32 = 12.0;
/// The night horde's warband tint (camps use both; invaders are uniformly this).
const INVADER_FACTION: orks::Faction = orks::Faction::Red;

// Stuck-ork safety net (ported from waveLogic.ts): an invader knocked onto an isolated tile the
// steerer can't leave never reaches the keep, so the wave never clears. Reap any that has sat
// essentially still, far from the keep, past the timeout.
const STUCK_TIMEOUT: f32 = 20.0;
const STUCK_MOVE_EPS: f32 = 0.6;
const STUCK_SAFE_RANGE: f32 = 16.0;

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

pub struct SiegePlugin;

impl Plugin for SiegePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeepHp>()
            .init_resource::<Siege>()
            .add_systems(Startup, (setup_invader_armory, setup_siege_hud))
            .add_systems(PostStartup, seed_demo_wave) // FOREST_WAVE screenshot hook only
            // Sim — frozen behind any panel / outside Playing.
            .add_systems(
                Update,
                (run_director, invader_brain, siege_controls).run_if(in_state(Modal::None)),
            )
            // HUD keeps drawing while frozen.
            .add_systems(Update, update_siege_hud)
            // Fresh run: reset on leaving the start screen or game-over (NOT on un-pausing,
            // which is a Playing↔Paused transition and never touches these).
            .add_systems(OnExit(AppState::StartScreen), reset_siege)
            .add_systems(OnExit(AppState::GameOver), reset_siege);
    }
}

/// Reset the siege to a fresh run (keeping the chosen difficulty): clear the field, rearm
/// the director, heal the keep. Runs when a new run begins (start-screen / game-over exit).
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
    commands.insert_resource(InvaderArmory(orks::Armory::new(&mut meshes, mat)));
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
    let p = spawn_point(ring_index, KEEP_POS, SPAWN_RING, |x, z| ground_at_world(x, z).is_some());
    let seed = ring_index.wrapping_mul(0x9e37_79b1) ^ (ring_index + 1);
    let e = armory.spawn(commands, variant, INVADER_FACTION, KEEP_POS, p, seed);
    commands.entity(e).insert((WaveInvader { last_pos: p, idle_since: now }, Health { hp, max: hp }));
}

/// The assault director: counts down prep, spawns the wave on an interval, advances to the next
/// wave / victory on a clear, and trips defeat if the keep is razed. Decision logic is the pure
/// [`step_wave_director`]; this feeds it world state and applies its actions.
#[allow(clippy::too_many_arguments)]
fn run_director(
    time: Res<Time>,
    mut siege: ResMut<Siege>,
    mut keep: ResMut<KeepHp>,
    mut player: ResMut<crate::player::PlayerRes>,
    eco: Res<crate::economy::EconomyState>,
    mut inv: ResMut<crate::inventory::Inventory>,
    mut toasts: ResMut<crate::inventory::Toasts>,
    mut lives: ResMut<crate::succession::Lives>,
    armory: Option<Res<InvaderArmory>>,
    invaders: Query<Entity, With<WaveInvader>>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
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

    let alive = invaders.iter().count() as u32;
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
                // Wave→Prep is a clear: pay the Tax Office stipend + harvest the Granary's bread.
                if p == GamePhase::Prep {
                    if eco.tax_office {
                        player.0.add_gold(crate::economy::TAX_STIPEND);
                    }
                    if eco.farm {
                        crate::inventory::try_grant(
                            &mut inv.0,
                            &mut toasts.0,
                            "bread",
                            crate::economy::FARM_HARVEST,
                            now as f64,
                        );
                    }
                    lives.heirs += 1; // dawn: a new heir comes of age
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
    hero: Res<HeroState>,
    siege: Res<Siege>,
    mut keep: ResMut<KeepHp>,
    mut pending: ResMut<PendingHeroDamage>,
    mut bolts: ResMut<BoltSpawns>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut orks::Ork, &mut WaveInvader, &mut Transform, &Health)>,
) {
    if siege.phase != GamePhase::Wave {
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();

    for (e, mut o, mut inv, mut tf, hp) in &mut q {
        o.atk_cd -= dt;
        // Berserker frenzy: faster march + quicker strikes under 40% HP (incl. the boss).
        let frenzied = o.variant == OrkVariant::Berserker && hp.hp < hp.max * 0.4;
        let dist_keep = o.pos.distance(KEEP_POS);
        // No leash: head for the keep, but turn on the hero if he blocks the way.
        let see_hero = hero.alive && o.pos.distance(hero.pos) < orks::ORK_SIGHT;
        let target = if see_hero { hero.pos } else { KEEP_POS };
        let atk_range = if o.shaman { orks::SHAMAN_CAST_RANGE } else { orks::ORK_ATTACK_RANGE };
        let at_hero = see_hero && o.pos.distance(hero.pos) < atk_range;
        let at_keep = !see_hero && dist_keep <= KEEP_ATTACK_RANGE;

        if at_hero || at_keep {
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
                } else {
                    // Hammering the keep.
                    o.atk_cd =
                        if o.shaman { orks::SHAMAN_CAST_CD } else { orks::ORK_ATTACK_CD * frenzy_cd };
                    keep.hp = (keep.hp - KEEP_DAMAGE).max(0.0);
                }
            }
        } else {
            // March toward the target, steering around props/cliffs (faster than a patrol).
            let cur_y = ground_at_world(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
            let speed = o.speed * 1.4 * if frenzied { 1.4 } else { 1.0 };
            match steer::advance(o.pos, o.facing, target, speed * dt, o.body_r, cur_y, orks::ORK_MAX_TURN * 1.6 * dt) {
                Some(s) => {
                    o.facing = s.facing;
                    o.pos = s.pos;
                    o.moving = s.moving;
                }
                None => o.moving = false,
            }
        }

        // Stuck-ork safety net — movement resets the idle clock; idle + far + timed-out → reap.
        if o.pos.distance(inv.last_pos) > STUCK_MOVE_EPS {
            inv.last_pos = o.pos;
            inv.idle_since = now;
        } else if dist_keep > STUCK_SAFE_RANGE && now - inv.idle_since >= STUCK_TIMEOUT {
            commands.entity(e).try_despawn();
            continue;
        }

        orks::apply_knockback(&mut o, dt);

        let gy = ground_at_world(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
        tf.translation = Vec3::new(o.pos.x, gy, o.pos.y);
        tf.rotation = Quat::from_rotation_y(o.facing);
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

// ── Phase HUD (chrome-less bars, matching `hud.rs`) ──────────────────────────────────

#[derive(Component)]
struct KeepHpFill;
#[derive(Component)]
struct PhaseFill;

fn setup_siege_hud(mut commands: Commands) {
    let track_bg = Color::srgba(0.0, 0.0, 0.0, 0.55);
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(14.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-150.0)), // centre the 300px block
            width: Val::Px(300.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|root| {
            // Keep HP (crimson).
            root.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(14.0), padding: UiRect::all(Val::Px(2.0)), ..default() },
                BackgroundColor(track_bg),
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(Color::srgb(0.80, 0.30, 0.26)),
                    KeepHpFill,
                ));
            });
            // Phase bar: prep = amber day-countdown; wave = crimson horde-remaining.
            root.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(8.0), padding: UiRect::all(Val::Px(2.0)), ..default() },
                BackgroundColor(track_bg),
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(Color::srgb(0.95, 0.75, 0.30)),
                    PhaseFill,
                ));
            });
        });
}

#[allow(clippy::type_complexity)]
fn update_siege_hud(
    siege: Res<Siege>,
    keep: Res<KeepHp>,
    invaders: Query<&WaveInvader>,
    mut keep_q: Query<&mut Node, (With<KeepHpFill>, Without<PhaseFill>)>,
    mut phase_q: Query<(&mut Node, &mut BackgroundColor), (With<PhaseFill>, Without<KeepHpFill>)>,
) {
    if let Ok(mut n) = keep_q.single_mut() {
        n.width = Val::Percent((keep.hp / keep.max * 100.0).clamp(0.0, 100.0));
    }
    let Ok((mut n, mut col)) = phase_q.single_mut() else { return };
    match siege.phase {
        GamePhase::Prep => {
            let p = prep_progress(siege.prep_seconds_left, mods_for(siege.difficulty));
            n.width = Val::Percent(((1.0 - p) * 100.0).clamp(0.0, 100.0)); // full day → drains to night
            col.0 = Color::srgb(0.95, 0.75, 0.30);
        }
        GamePhase::Wave => {
            let count = effective_count(siege.wave_index.max(0) as usize, mods_for(siege.difficulty));
            let alive = invaders.iter().count() as u32;
            n.width = Val::Percent((alive as f32 / count as f32 * 100.0).clamp(0.0, 100.0));
            col.0 = Color::srgb(0.85, 0.22, 0.22);
        }
        GamePhase::Victory => {
            n.width = Val::Percent(100.0);
            col.0 = Color::srgb(0.40, 0.85, 0.40);
        }
        GamePhase::Defeat => {
            n.width = Val::Percent(100.0);
            col.0 = Color::srgb(0.30, 0.30, 0.30);
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
    fn difficulty_presets_match_original() {
        assert_eq!(mods_for(Difficulty::Easy), DiffMods { count_mul: 0.8, hp_mul: 0.85, prep_mul: 1.25 });
        assert_eq!(mods_for(Difficulty::Normal), DiffMods { count_mul: 1.0, hp_mul: 1.0, prep_mul: 1.0 });
        assert_eq!(mods_for(Difficulty::Hard), DiffMods { count_mul: 1.25, hp_mul: 1.2, prep_mul: 0.8 });
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
        assert_eq!(r1.timers.prep_ends_at, PREP_DURATION); // 0 + 150·1

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
}
