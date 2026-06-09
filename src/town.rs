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
use crate::palette::lin;

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
            .init_resource::<PlotSpots>()
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

/// Stores the world-XZ centre of every seeded plot (index = plot idx).
#[derive(Resource, Default)]
pub struct PlotSpots(pub Vec<Vec2>);

/// Fixed plot offsets from the castle origin — corners of the grass safe-zone,
/// OUTSIDE the ~17×12 castle wall footprint (in_footprint: |x|<=18.6 && |z|<=13.6)
/// and OFF the cardinal (N/S/E/W) gate lanes, all within ~21 world units so they
/// sit on forced-flat grass.
const PLOT_OFFSETS: [Vec2; PLOT_COUNT] = [
    Vec2::new(10.0, 15.5),   // NE — north of wall, off gate lane
    Vec2::new(-10.0, 15.5),  // NW — north of wall, off gate lane
    Vec2::new(10.0, -15.5),  // SE — south of wall, off gate lane
    Vec2::new(-10.0, -15.5), // SW — south of wall, off gate lane
    Vec2::new(20.0, 8.0),    // E  — east of wall, off gate lane
    Vec2::new(-20.0, 8.0),   // W  — west of wall, off gate lane
    Vec2::new(20.0, -8.0),   // E  — east of wall, off gate lane
    Vec2::new(-20.0, -8.0),  // W  — west of wall, off gate lane
];

/// Seed the build-plot entities + their foundation pads. Called from `worldmap::build`
/// after the castle so the safe-zone ground is final.
pub fn populate_plots(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    let pad = meshes.add(plot_pad_mesh());
    let mut spots = Vec::with_capacity(PLOT_COUNT);
    for (idx, off) in PLOT_OFFSETS.iter().enumerate() {
        let y = crate::worldmap::ground_at_world(off.x, off.y).unwrap_or(0.0);
        spots.push(*off);
        commands.spawn((
            Mesh3d(pad.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(off.x, y + 0.02, off.y),
            crate::biome::BiomeEntity,
            BuildPlot { idx },
        ));
    }
    commands.insert_resource(PlotSpots(spots));
}

/// A low foundation pad (flat-shaded, vertex-coloured per the mesh contract).
fn plot_pad_mesh() -> Mesh {
    let mut m = tinted(Cuboid::new(3.4, 0.12, 3.4).mesh().build(), lin(0x6b5a44));
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// Tag every vertex with a uniform linear RGBA colour (mesh-contract helper, mirrors props.rs).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
