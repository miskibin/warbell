//! Port of src/world/waveLogic.ts (the pure assault-director reducer) plus the
//! WAVES table + PREP_DURATION from waveStore.ts. No clock, no stores — drive it
//! with explicit phase/wave/timers and apply the emitted actions.

use crate::ork_config::{OrkVariant, ork_config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePhase {
    Menu,
    Prep,
    Wave,
    Victory,
    Defeat,
}

/// The day is a free-roam window (2.5 min) before the night siege.
pub const PREP_DURATION: f64 = 150.0;

#[derive(Debug, Clone)]
pub struct WaveDef {
    pub count: i64,
    pub hp_scale: f64,
    pub variants: &'static [OrkVariant],
    pub spawn_interval: f64,
}

use OrkVariant::*;

/// Escalating assault waves; the final entry is the lone boss push.
pub fn waves() -> Vec<WaveDef> {
    vec![
        WaveDef { count: 6, hp_scale: 1.0, variants: &[Grunt, Grunt, Scout, Grunt], spawn_interval: 1.2 },
        WaveDef { count: 8, hp_scale: 1.18, variants: &[Grunt, Scout, Grunt, Berserker], spawn_interval: 1.1 },
        WaveDef { count: 12, hp_scale: 1.45, variants: &[Grunt, Scout, Berserker, Shaman], spawn_interval: 1.1 },
        WaveDef { count: 15, hp_scale: 1.67, variants: &[Grunt, Berserker, Scout, Shaman], spawn_interval: 1.0 },
        WaveDef { count: 18, hp_scale: 1.92, variants: &[Berserker, Scout, Grunt, Shaman], spawn_interval: 0.95 },
        WaveDef { count: 22, hp_scale: 2.21, variants: &[Berserker, Scout, Shaman, Grunt], spawn_interval: 0.85 },
        WaveDef { count: 26, hp_scale: 2.54, variants: &[Berserker, Shaman, Scout, Grunt], spawn_interval: 0.75 },
        WaveDef { count: 1, hp_scale: 14.0, variants: &[Berserker], spawn_interval: 0.5 }, // boss
    ]
}

/// Per-component scratch state threaded frame to frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveTimers {
    /// wall-clock (sec) the prep breather ends; 0 = not yet armed
    pub prep_ends_at: f64,
    /// earliest time (sec) the next ork in this wave may spawn
    pub next_spawn_at: f64,
    /// running count of orks spawned this wave (drives variant rotation + ring)
    pub spawn_index: i64,
}

impl Default for WaveTimers {
    fn default() -> Self {
        Self { prep_ends_at: 0.0, next_spawn_at: 0.0, spawn_index: 0 }
    }
}

/// Mirrors WaveProgress in waveStore.ts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveProgress {
    pub index: i64, // 0-based; -1 before the first wave
    pub total: i64,
    pub enemies_alive: i64,
    pub spawned: i64,
    pub prep_seconds_left: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WaveAction {
    BeginWave { index: i64 },
    SetPhase { phase: GamePhase },
    Spawn { variant: OrkVariant, hp: i64, spawn_index: i64, wave_index: i64 },
}

pub struct WaveStepInput {
    pub phase: GamePhase,
    pub wave: WaveProgress,
    pub timers: WaveTimers,
    pub now: f64,
    /// living wave orks this frame
    pub alive: i64,
    /// player pressed "Skip" — start the next wave now
    pub skip: bool,
}

pub struct WaveStepResult {
    pub actions: Vec<WaveAction>,
    pub timers: WaveTimers,
}

/// Advance the director one tick. Pure: returns actions + next timers, mutating
/// nothing (the input timers are cloned).
pub fn step_wave_director(input: &WaveStepInput) -> WaveStepResult {
    let mut timers = input.timers; // copy — never mutate the input
    let mut actions: Vec<WaveAction> = Vec::new();
    let now = input.now;
    let wave = &input.wave;

    if input.phase == GamePhase::Prep {
        if timers.prep_ends_at == 0.0 {
            timers.prep_ends_at = now + PREP_DURATION;
        }
        if input.skip || now >= timers.prep_ends_at {
            actions.push(WaveAction::BeginWave { index: wave.index + 1 });
            timers.spawn_index = 0;
            timers.next_spawn_at = now;
            timers.prep_ends_at = 0.0;
            actions.push(WaveAction::SetPhase { phase: GamePhase::Wave });
        }
        return WaveStepResult { actions, timers };
    }

    if input.phase == GamePhase::Wave {
        let table = waves();
        let def = if wave.index >= 0 && (wave.index as usize) < table.len() {
            &table[wave.index as usize]
        } else {
            return WaveStepResult { actions, timers };
        };
        // Spawn on interval until the wave's quota is met.
        if wave.spawned < def.count && now >= timers.next_spawn_at {
            let variant = def.variants[(timers.spawn_index as usize) % def.variants.len()];
            let hp = (ork_config(variant).hp * def.hp_scale).round() as i64;
            actions.push(WaveAction::Spawn {
                variant,
                hp,
                spawn_index: timers.spawn_index,
                wave_index: wave.index,
            });
            timers.spawn_index += 1;
            timers.next_spawn_at = now + def.spawn_interval;
        }
        // Wave cleared once everything has spawned and nothing is left alive.
        if wave.spawned >= def.count && input.alive == 0 {
            let phase = if wave.index as usize >= table.len() - 1 {
                GamePhase::Victory
            } else {
                GamePhase::Prep
            };
            actions.push(WaveAction::SetPhase { phase });
        }
    }

    WaveStepResult { actions, timers }
}

#[cfg(test)]
mod tests {
    // Port of src/world/waveLogic.test.ts.
    use super::*;

    fn timers() -> WaveTimers {
        WaveTimers::default()
    }
    fn wave(index: i64, spawned: i64) -> WaveProgress {
        WaveProgress {
            index,
            total: waves().len() as i64,
            enemies_alive: 0,
            spawned,
            prep_seconds_left: 0.0,
        }
    }
    fn step(phase: GamePhase, w: WaveProgress, t: WaveTimers, now: f64, alive: i64) -> WaveStepResult {
        step_wave_director(&WaveStepInput { phase, wave: w, timers: t, now, alive, skip: false })
    }

    // --- prep phase ---
    #[test]
    fn arms_prep_countdown_on_first_tick_without_acting() {
        let r = step(GamePhase::Prep, wave(-1, 0), timers(), 100.0, 0);
        assert_eq!(r.actions, vec![]);
        assert_eq!(r.timers.prep_ends_at, 100.0 + PREP_DURATION);
    }

    #[test]
    fn waits_while_countdown_running() {
        let t = WaveTimers { prep_ends_at: 112.0, ..timers() };
        let r = step(GamePhase::Prep, wave(-1, 0), t, 111.0, 0);
        assert_eq!(r.actions, vec![]);
        assert_eq!(r.timers.prep_ends_at, 112.0);
    }

    #[test]
    fn begins_next_wave_when_timer_elapses() {
        let t = WaveTimers { prep_ends_at: 112.0, ..timers() };
        let r = step(GamePhase::Prep, wave(-1, 0), t, 112.0, 0);
        assert_eq!(
            r.actions,
            vec![
                WaveAction::BeginWave { index: 0 },
                WaveAction::SetPhase { phase: GamePhase::Wave },
            ]
        );
        assert_eq!(r.timers, WaveTimers { prep_ends_at: 0.0, next_spawn_at: 112.0, spawn_index: 0 });
    }

    // --- wave phase: spawning ---
    #[test]
    fn spawns_one_ork_when_interval_up() {
        let r = step(GamePhase::Wave, wave(0, 0), timers(), 0.0, 0);
        assert_eq!(
            r.actions,
            vec![WaveAction::Spawn { variant: OrkVariant::Grunt, hp: 254, spawn_index: 0, wave_index: 0 }]
        );
        assert_eq!(r.timers.spawn_index, 1);
        assert_eq!(r.timers.next_spawn_at, waves()[0].spawn_interval);
    }

    #[test]
    fn holds_fire_until_interval_passed() {
        let t = WaveTimers { next_spawn_at: 1.6, spawn_index: 1, ..timers() };
        assert_eq!(step(GamePhase::Wave, wave(0, 1), t, 1.0, 1).actions.len(), 0);
        assert_eq!(step(GamePhase::Wave, wave(0, 1), t, 1.6, 1).actions.len(), 1);
    }

    #[test]
    fn rotates_variant_pool_by_spawn_index() {
        // Wave 2 (index 1) pool is [grunt, scout, grunt, berserker]; index 1 -> scout.
        let t = WaveTimers { spawn_index: 1, ..timers() };
        let r = step(GamePhase::Wave, wave(1, 1), t, 5.0, 1);
        match &r.actions[0] {
            WaveAction::Spawn { variant, .. } => assert_eq!(*variant, OrkVariant::Scout),
            other => panic!("expected spawn, got {other:?}"),
        }
    }

    #[test]
    fn scales_hp_by_wave_hpscale() {
        // Wave 3 (index 2) hpScale 1.45, grunt base 254 -> round(368.3) = 368.
        let r = step(GamePhase::Wave, wave(2, 0), timers(), 0.0, 0);
        match &r.actions[0] {
            WaveAction::Spawn { variant, hp, .. } => {
                assert_eq!(*variant, OrkVariant::Grunt);
                assert_eq!(*hp, 368);
            }
            other => panic!("expected spawn, got {other:?}"),
        }
    }

    #[test]
    fn stops_spawning_once_quota_met() {
        let full = waves()[0].count;
        let r = step(GamePhase::Wave, wave(0, full), timers(), 99.0, 3);
        assert!(!r.actions.iter().any(|a| matches!(a, WaveAction::Spawn { .. })));
    }

    // --- wave phase: clearing ---
    #[test]
    fn does_not_advance_while_enemies_remain() {
        let full = waves()[0].count;
        let t = WaveTimers { spawn_index: full, ..timers() };
        let r = step(GamePhase::Wave, wave(0, full), t, 99.0, 2);
        assert_eq!(r.actions, vec![]);
    }

    #[test]
    fn returns_to_prep_after_non_final_wave_cleared() {
        let full = waves()[0].count;
        let t = WaveTimers { spawn_index: full, ..timers() };
        let r = step(GamePhase::Wave, wave(0, full), t, 99.0, 0);
        assert_eq!(r.actions, vec![WaveAction::SetPhase { phase: GamePhase::Prep }]);
    }

    #[test]
    fn declares_victory_after_final_wave_cleared() {
        let last = (waves().len() - 1) as i64;
        let t = WaveTimers { spawn_index: 1, ..timers() };
        let r = step(GamePhase::Wave, wave(last, waves()[last as usize].count), t, 99.0, 0);
        assert_eq!(r.actions, vec![WaveAction::SetPhase { phase: GamePhase::Victory }]);
    }

    // --- boss wave ---
    #[test]
    fn spawns_lone_high_hp_berserker() {
        let last = (waves().len() - 1) as i64;
        let r = step(GamePhase::Wave, wave(last, 0), timers(), 0.0, 0);
        // boss: berserker base 306 * hpScale 14 = 4284
        assert!(r.actions.contains(&WaveAction::Spawn {
            variant: OrkVariant::Berserker,
            hp: 4284,
            spawn_index: 0,
            wave_index: last,
        }));
    }

    // --- inert states ---
    #[test]
    fn does_nothing_for_out_of_range_index() {
        assert_eq!(step(GamePhase::Wave, wave(99, 0), timers(), 0.0, 0).actions, vec![]);
    }

    #[test]
    fn does_nothing_in_menu_victory_defeat() {
        for phase in [GamePhase::Menu, GamePhase::Victory, GamePhase::Defeat] {
            assert_eq!(step(phase, wave(0, 0), timers(), 5.0, 0).actions, vec![]);
        }
    }

    #[test]
    fn never_mutates_input_timers() {
        let t = timers();
        step(GamePhase::Wave, wave(0, 0), t, 0.0, 0);
        assert_eq!(t, WaveTimers { prep_ends_at: 0.0, next_spawn_at: 0.0, spawn_index: 0 });
    }

    // --- table shape (from waveStore.test.ts essentials) ---
    #[test]
    fn table_has_eight_waves_and_boss_last() {
        let w = waves();
        assert_eq!(w.len(), 8);
        assert_eq!(w[7].count, 1);
        assert_eq!(w[7].hp_scale, 14.0);
        // boss hpScale is the max
        assert!(w.iter().all(|d| d.hp_scale <= w[7].hp_scale));
    }
}
