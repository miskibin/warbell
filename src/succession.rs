//! **Succession — the bloodline.** The hero is one of a line of heirs: when he falls, the blade
//! passes to the next (he respawns at a gate with all progression intact) and the town's
//! headcount drops by one — **an heir IS a townsperson**. There is exactly one number:
//! `town.population` is the source of truth, and [`Lives::heirs`] is a read-only mirror of it
//! ([`mirror_heirs`]) so the HUD/save never drift from the town. The hero falling with the town
//! empty ends the line — Defeat by *bloodline*, the second way to lose besides the keep falling.
//!
//! The pool grows however the town grows: food surplus (`town::population_system`), camp rescues,
//! mercenary recruits, the housing-gated dawn coming-of-age (`siege::run_director`), and the
//! castle larder (core `town_store`) that organically regrows an emptied town to its bedrock
//! pair — so the bloodline always has somewhere to come back from.

use bevy::prelude::*;

use crate::game_state::AppState;

/// The bloodline pool. `heirs` is a **read-only mirror** of `town.population` — never write it;
/// write the town headcount instead ([`mirror_heirs`] stomps any drift every frame).
#[derive(Resource)]
pub struct Lives {
    pub heirs: u32,
    /// Set when the hero falls with no townsperson left → the run is lost.
    pub defeat: bool,
}

impl Default for Lives {
    fn default() -> Self {
        Self { heirs: 0, defeat: false } // heirs filled from town.population by `mirror_heirs`
    }
}

pub struct SuccessionPlugin;

impl Plugin for SuccessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Lives>()
            .add_systems(OnExit(AppState::StartScreen), reset_lives)
            .add_systems(OnExit(AppState::GameOver), reset_lives)
            // `OnExit(GameOver)` fires on an in-process Continue (fresh runs relaunch instead);
            // `reset_lives` clears the defeat flag before `apply_pending_load` restores the save.
            // Ungated: the heir count shown in the HUD tracks the town even while frozen.
            .add_systems(Update, mirror_heirs)
            .add_systems(Update, watch_bloodline.run_if(in_state(AppState::Playing)));
    }
}

/// New run: clear the defeat flag. The heir count needs no seeding — it mirrors the town, and
/// `town::reset_town` owns the starting headcount (incl. the Easy spare-townsfolk handicap).
fn reset_lives(mut lives: ResMut<Lives>) {
    *lives = Lives::default();
}

/// Enforce heirs ≡ town.population every frame. Heirs and townsfolk are the SAME people; any
/// system that used to bump `heirs` separately now writes the town headcount and this sync
/// carries it into the HUD/save view.
fn mirror_heirs(town: Res<crate::town::TownRes>, mut lives: ResMut<Lives>) {
    if lives.heirs != town.0.population {
        lives.heirs = town.0.population;
    }
}

/// End the run once the bloodline is spent (hand off to the GameOver screen).
fn watch_bloodline(lives: Res<Lives>, mut next: ResMut<NextState<AppState>>) {
    if lives.defeat {
        next.set(AppState::GameOver);
    }
}
