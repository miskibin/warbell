//! **City-building town economy.** Wraps the tested `tileworld_core::town_store::Town`
//! as a Resource and owns: pre-placed build plots, the `Modal::Build` construction
//! menu, the production + population ticks, and the night burn/repair. Villagers
//! auto-staff producers (worker steering lives in `villagers.rs`); a fraction of
//! night invaders divert here to burn buildings (`orks.rs` pushes `PendingBuildingDamage`).
//!
//! Sim systems carry `.run_if(in_state(Modal::None))` per the freeze gate; VFX/render
//! stay ungated. Numbers live in `town_store` (test-gated).

use bevy::prelude::*;
use tileworld_core::town_store::{BuildKind, Town};

use crate::economy::Bank;
use crate::game_state::{AppState, Modal};

/// Number of build plots seeded around the castle.
pub const PLOT_COUNT: usize = 8;
/// Starting wood so the player can build on day one.
const START_WOOD: f64 = 16.0;

/// The settlement model (parity-tested core) as a Bevy Resource.
#[derive(Resource)]
pub struct TownRes(pub Town);

impl Default for TownRes {
    fn default() -> Self {
        Self(Town::new(PLOT_COUNT, 0))
    }
}

/// Marks a build-plot entity; `idx` indexes `TownRes.0.plots`.
#[derive(Component)]
pub struct BuildPlot {
    pub idx: usize,
}

/// The building mesh sitting on a plot (despawned on collapse/rebuild).
#[derive(Component)]
pub struct BuildingMesh {
    pub idx: usize,
}

/// Tags a villager assigned to staff a plot (set by town auto-assign, read by
/// `villagers::worker_steer`). `at_post` flips true once it reaches the building.
#[derive(Component)]
pub struct Worker {
    pub idx: usize,
    pub at_post: bool,
}

/// Damage night invaders deal to buildings this frame: `(plot_idx, damage)`.
/// `orks::invader_brain` pushes; `apply_building_damage` drains.
#[derive(Resource, Default)]
pub struct PendingBuildingDamage(pub Vec<(usize, f32)>);

/// Which buildable plot the hero is standing on (set by `interaction.rs`), so the
/// Build panel knows where to build. `None` when not on a buildable plot.
#[derive(Resource, Default)]
pub struct BuildTarget(pub Option<usize>);

pub struct TownPlugin;

impl Plugin for TownPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TownRes>()
            .init_resource::<PendingBuildingDamage>()
            .init_resource::<BuildTarget>()
            .add_systems(OnExit(AppState::StartScreen), reset_town)
            .add_systems(OnExit(AppState::GameOver), reset_town);
        // Plot seeding, build Modal, ticks, and burn/repair are added in later tasks.
    }
}

/// New run: clear the town and seed starting wood. Mirrors `economy::reset_economy`.
fn reset_town(mut town: ResMut<TownRes>, mut bank: ResMut<Bank>) {
    town.0.reset(0);
    bank.0.add_wood(START_WOOD);
}

/// World-space centre of plot `idx` (set when plots are seeded).
pub fn plot_world(idx: usize, plots: &[Vec2]) -> Vec2 {
    plots.get(idx).copied().unwrap_or(Vec2::ZERO)
}
