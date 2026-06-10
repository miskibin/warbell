//! **Succession — the bloodline.** The hero is one of a line of heirs: when he falls, the blade
//! passes to the next (he respawns at a gate with all progression intact). Run out of heirs and
//! the run ends — Defeat by *bloodline*, the second way to lose besides the keep falling. The
//! pool starts at [`STARTING_HEIRS`] and grows by one each **dawn** (a cleared wave), so holding
//! the line buys you lives.
//!
//! This is the headline P5 mechanic. Guard combat, camp rescue, the muster yard and
//! district→population growth are the deferred long-tail (tracked in the roadmap).

use bevy::prelude::*;

use crate::game_state::AppState;

/// Heirs the bloodline starts a run with (TS `STARTING_HEIRS`).
pub const STARTING_HEIRS: u32 = 3;

/// The bloodline pool: how many heirs remain, and whether the line has ended.
#[derive(Resource)]
pub struct Lives {
    pub heirs: u32,
    /// Set when the hero falls with no heir left → the run is lost.
    pub defeat: bool,
}

impl Default for Lives {
    fn default() -> Self {
        Self { heirs: STARTING_HEIRS, defeat: false }
    }
}

pub struct SuccessionPlugin;

impl Plugin for SuccessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Lives>()
            .add_systems(OnExit(AppState::StartScreen), reset_lives)
            .add_systems(OnExit(AppState::GameOver), reset_lives)
            .add_systems(
                OnExit(AppState::Paused),
                reset_lives.run_if(crate::game_state::restart_requested),
            )
            .add_systems(Update, watch_bloodline.run_if(in_state(AppState::Playing)));
    }
}

fn reset_lives(mut lives: ResMut<Lives>, siege: Option<Res<crate::siege::Siege>>) {
    *lives = Lives::default();
    // Difficulty handicap: Easy grants spare heirs so a beginner's run survives a few falls.
    let diff = siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);
    lives.heirs += crate::siege::mods_for(diff).heirs_bonus;
}

/// End the run once the bloodline is spent (hand off to the GameOver screen).
fn watch_bloodline(lives: Res<Lives>, mut next: ResMut<NextState<AppState>>) {
    if lives.defeat {
        next.set(AppState::GameOver);
    }
}
