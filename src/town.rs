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

use crate::succession::Lives;
use crate::villagers::{Guard, Townsfolk};

use crate::economy::Bank;
use crate::game_state::{AppState, Modal};
use crate::palette::lin;
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};

/// A flame entity tied to a burning plot (despawned when extinguished/collapsed).
#[derive(Component)]
struct Flame {
    idx: usize,
}

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

/// Tags a build-plot (foundation pad) entity. Plots are indexed via the `PlotSpots` resource.
#[derive(Component)]
pub struct BuildPlot;

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
            // Run AFTER economy's reset: its `bank.0.reset()` zeroes food/wood too, so the
            // START_WOOD grant must come last or it gets wiped (system-order race).
            .add_systems(
                OnExit(AppState::StartScreen),
                reset_town.after(crate::economy::reset_economy),
            )
            .add_systems(
                OnExit(AppState::GameOver),
                reset_town.after(crate::economy::reset_economy),
            )
            .add_systems(OnEnter(Modal::Build), spawn_build)
            .add_systems(OnExit(Modal::Build), despawn_build)
            .add_systems(Update, build_interact.run_if(in_state(Modal::Build)))
            .add_systems(
                Update,
                (auto_assign_workers, sync_staffed, release_orphan_workers)
                    .run_if(in_state(Modal::None)),
            )
            .add_systems(
                Update,
                (production_system, population_system).run_if(in_state(Modal::None)),
            )
            // Sim (gated): apply damage + repair only while playing.
            .add_systems(
                Update,
                (apply_building_damage, repair_system).run_if(in_state(Modal::None)),
            )
            // Rebuild building meshes to match a loaded `TownRes` (ungated; fires on a load).
            .add_systems(Update, restore_buildings)
            // VFX (ungated): flames flicker even when frozen.
            .add_systems(Update, flame_flicker)
            // Screenshot staging (ungated): no-op unless FOREST_TOWN / FOREST_PANEL=build set.
            .add_systems(Update, (stage_town_for_shot, open_build_for_shot));
    }
}

/// By day, staff each producer from the idle townsfolk reserve: pick the nearest standing guard
/// (an unemployed `Townsfolk`) and swap its `Guard` role for a `Worker` job — it downs its weapon
/// and walks to the field. Skipped during a wave: nobody gets pulled off the wall mid-assault, and
/// `muster_townsfolk` has already fired everyone back to guard duty at dusk.
#[allow(clippy::type_complexity)]
fn auto_assign_workers(
    town: Res<TownRes>,
    spots: Res<PlotSpots>,
    siege: Option<Res<crate::siege::Siege>>,
    mut commands: Commands,
    workers: Query<&Worker>,
    idle: Query<(Entity, &Transform), (With<Townsfolk>, With<Guard>, Without<Worker>)>,
) {
    if siege.is_some_and(|s| s.phase == crate::siege::GamePhase::Wave) {
        return; // night: defenders stay on the wall
    }
    for (idx, plot) in town.0.plots.iter().enumerate() {
        let Some(kind) = plot.kind else { continue };
        if !plot.is_built() || !kind.needs_worker() {
            continue;
        }
        if workers.iter().any(|w| w.idx == idx) {
            continue; // already has a worker assigned
        }
        let Some(spot) = spots.0.get(idx).copied() else { continue };
        // Nearest idle townsperson.
        let mut best: Option<(Entity, f32)> = None;
        for (e, tf) in &idle {
            let d = Vec2::new(tf.translation.x, tf.translation.z).distance(spot);
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((e, d));
            }
        }
        if let Some((e, _)) = best {
            // Off guard duty, onto the job (Guard → Worker; the two roles are exclusive).
            commands.entity(e).try_remove::<Guard>().try_insert(Worker { idx, at_post: false });
        }
    }
}

/// Each frame, mark a plot `staffed` iff a posted, visible worker is on it.
fn sync_staffed(mut town: ResMut<TownRes>, workers: Query<(&Worker, &Visibility)>) {
    let n = town.0.plots.len();
    let mut staffed = vec![false; n];
    for (w, vis) in &workers {
        if w.at_post && *vis != Visibility::Hidden && w.idx < n {
            staffed[w.idx] = true;
        }
    }
    for (i, plot) in town.0.plots.iter_mut().enumerate() {
        plot.staffed = staffed[i];
    }
}

/// Drop the `Worker` tag when its plot is gone (collapsed to rubble), so the
/// villager rejoins the idle pool and auto-assign re-staffs survivors.
fn release_orphan_workers(town: Res<TownRes>, mut commands: Commands, workers: Query<(Entity, &Worker)>) {
    for (e, w) in &workers {
        let gone = town.0.plots.get(w.idx).map_or(true, |p| !p.is_built());
        if gone {
            commands.entity(e).try_remove::<Worker>();
        }
    }
}

/// Staffed producers add their yield; runs only while playing (Modal::None).
fn production_system(time: Res<Time>, mut town: ResMut<TownRes>, mut bank: ResMut<Bank>) {
    let dt = time.delta_secs() as f64;
    town.0.production_tick(dt, &mut bank.0);
}

/// Food upkeep + growth; on growth, add one townsperson to the `Townsfolk` pool and grow the
/// bloodline (keeps the house→heir tie). The new body joins as a standing guard (the idle reserve),
/// so it defends at night and `auto_assign_workers` can post it to a producer by day — the
/// food→population→workforce-and-militia loop.
#[allow(clippy::too_many_arguments)]
fn population_system(
    time: Res<Time>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut lives: ResMut<Lives>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
) {
    let dt = time.delta_secs() as f64;
    if town.0.population_tick(dt, &mut bank.0) {
        lives.heirs += 1;
        let seed = 0x70b1_0000u32.wrapping_add(town.0.population.wrapping_mul(101));
        crate::villagers::spawn_courtyard_guard(&mut commands, &mut meshes, &mut materials, seed);
        // Make the food→population link visible: a float over the town when a villager is born.
        floats.0.push(crate::combat_fx::FloatReq {
            world: Vec3::new(0.0, 6.5, 5.0),
            text: "\u{1f331} A villager joins your town!".into(),
            color: Color::srgb(0.55, 1.0, 0.6),
            scale: 1.25,
        });
    }
}

/// New run: clear the town and seed starting wood. Mirrors `economy::reset_economy`.
fn reset_town(
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    stale: Query<Entity, Or<(With<BuildingMesh>, With<Flame>)>>,
) {
    town.0.reset(0);
    bank.0.add_wood(START_WOOD);
    // The world map isn't rebuilt on restart, so reap last run's building meshes +
    // flames here (the empty plot pads persist; TownRes is now all-Empty, so the
    // scene must match). Mirrors how succession_fx reaps graves on a new run.
    for e in &stale {
        commands.entity(e).try_despawn();
    }
}

/// On a loaded game (`GameLoaded`), rebuild building meshes to match the restored `TownRes`:
/// reap every current building mesh + flame, then spawn one per built plot. The plot pads
/// persist (built once at startup), so only the buildings on them need reconciling — the mirror
/// of `reset_town`'s reap, but for an arbitrary saved layout instead of an empty one.
fn restore_buildings(
    mut ev: MessageReader<crate::savegame::GameLoaded>,
    town: Res<TownRes>,
    spots: Res<PlotSpots>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    stale: Query<Entity, Or<(With<BuildingMesh>, With<Flame>)>>,
) {
    if ev.read().count() == 0 {
        return;
    }
    for e in &stale {
        commands.entity(e).try_despawn();
    }
    for (idx, plot) in town.0.plots.iter().enumerate() {
        if let (true, Some(kind)) = (plot.is_built(), plot.kind) {
            spawn_building_mesh(&mut commands, &mut meshes, &mut materials, idx, kind, &spots);
        }
    }
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
    for off in PLOT_OFFSETS.iter() {
        let y = crate::worldmap::ground_at_world(off.x, off.y).unwrap_or(0.0);
        spots.push(*off);
        commands.spawn((
            Mesh3d(pad.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(off.x, y + 0.02, off.y),
            crate::biome::BiomeEntity,
            BuildPlot,
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

// ── Damage, fire VFX, and repair ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn apply_building_damage(
    mut town: ResMut<TownRes>,
    mut pending: ResMut<PendingBuildingDamage>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spots: Res<PlotSpots>,
    buildings: Query<(Entity, &BuildingMesh)>,
    flames: Query<(Entity, &Flame)>,
) {
    for (idx, dmg) in pending.0.drain(..) {
        let was_built = town.0.plots.get(idx).map_or(false, |p| p.is_built());
        town.0.damage(idx, dmg as f64);
        if !was_built {
            continue;
        }
        let now_rubble = town.0.plots.get(idx).map_or(false, |p| {
            matches!(p.state, tileworld_core::town_store::PlotState::Rubble)
        });
        if now_rubble {
            // Collapse: drop the building mesh + its flames, leave the bare plot (rubble).
            for (e, bm) in &buildings {
                if bm.idx == idx {
                    commands.entity(e).try_despawn();
                }
            }
            for (e, f) in &flames {
                if f.idx == idx {
                    commands.entity(e).try_despawn();
                }
            }
        } else {
            // Still standing + burning: ensure a flame is showing.
            let has_flame = flames.iter().any(|(_, f)| f.idx == idx);
            if !has_flame {
                spawn_flame(&mut commands, &mut meshes, &mut materials, idx, &spots);
            }
        }
    }
}

fn spawn_flame(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    idx: usize,
    spots: &PlotSpots,
) {
    let pos = spots.0.get(idx).copied().unwrap_or(Vec2::ZERO);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.45, 0.1),
        emissive: LinearRgba::rgb(6.0, 2.0, 0.3),
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(0.6).mesh().ico(1).unwrap())),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, y + 1.6, pos.y),
        crate::biome::BiomeEntity,
        Flame { idx },
        PointLight {
            color: Color::srgb(1.0, 0.5, 0.2),
            intensity: 60_000.0,
            range: 10.0,
            ..default()
        },
    ));
}

/// Bob/scale the flames so they read as fire (ungated — VFX runs while frozen).
fn flame_flicker(time: Res<Time>, mut q: Query<(&mut Transform, &Flame)>) {
    let t = time.elapsed_secs_wrapped();
    for (mut tf, f) in &mut q {
        let s = 0.8 + (t * 9.0 + f.idx as f32).sin() * 0.18;
        tf.scale = Vec3::splat(s);
    }
}

/// Repair damaged survivors during Prep; extinguish flames once a plot is full HP.
fn repair_system(
    time: Res<Time>,
    siege: Option<Res<crate::siege::Siege>>,
    mut town: ResMut<TownRes>,
    mut commands: Commands,
    flames: Query<(Entity, &Flame)>,
) {
    let prep = siege.map_or(false, |s| s.phase == crate::siege::GamePhase::Prep);
    if !prep {
        return;
    }
    town.0.repair(time.delta_secs() as f64);
    // Despawn flames whose plot is no longer burning.
    for (e, f) in &flames {
        let burning = town.0.plots.get(f.idx).map_or(false, |p| {
            matches!(p.state, tileworld_core::town_store::PlotState::Built { burning: true, .. })
        });
        if !burning {
            commands.entity(e).try_despawn();
        }
    }
}

/// Screenshot-staging hook: pre-builds plots 0/1/2 and optionally ignites plot 0.
/// Runs once per launch when `FOREST_TOWN` is set (any value builds; `"burn"` also ignites).
/// Waits until `PlotSpots` is populated (post worldmap build) via the early-return guard.
/// Screenshot hook: `FOREST_PANEL=build` pops the Build menu (with a plot target + resources
/// staged) so the construction copy can be captured. No-op otherwise.
fn open_build_for_shot(
    app: Res<State<AppState>>,
    mut next: ResMut<NextState<Modal>>,
    mut target: ResMut<BuildTarget>,
    mut bank: ResMut<Bank>,
    mut done: Local<bool>,
) {
    if *done || std::env::var("FOREST_PANEL").ok().as_deref() != Some("build") {
        return;
    }
    if *app.get() == AppState::Playing {
        *done = true;
        target.0 = Some(0); // pretend we're standing on a plot so the rows read as buildable
        bank.0.add_wood(50.0);
        bank.0.add_stone(50.0);
        next.set(Modal::Build);
    }
}

fn stage_town_for_shot(
    mut done: Local<bool>,
    spots: Res<PlotSpots>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *done || spots.0.is_empty() {
        return;
    }
    let Ok(mode) = std::env::var("FOREST_TOWN") else { *done = true; return };
    *done = true;
    bank.0.add_wood(100.0);
    bank.0.add_stone(100.0);
    town.0.build(0, BuildKind::Farm, &mut bank.0);
    town.0.build(1, BuildKind::House, &mut bank.0);
    town.0.build(2, BuildKind::Farm, &mut bank.0);
    town.0.build(3, BuildKind::Lumber, &mut bank.0);
    for idx in [0usize, 1, 2, 3] {
        if let Some(kind) = town.0.plots[idx].kind {
            spawn_building_mesh(&mut commands, &mut meshes, &mut materials, idx, kind, &spots);
        }
    }
    if mode == "burn" {
        town.0.damage(0, 20.0);
        spawn_flame(&mut commands, &mut meshes, &mut materials, 0, &spots);
    }
}

// ── Modal::Build panel ────────────────────────────────────────────────────────────────────

#[derive(Component)]
struct BuildUi;

#[derive(Component)]
struct BuildOption(BuildKind);

const MENU: [BuildKind; 3] = [BuildKind::Farm, BuildKind::House, BuildKind::Lumber];

/// One-line "what it does" shown under each building in the Build menu — so players see that
/// a Farm *feeds the town and grows population*, not just that it "needs a worker".
fn build_desc(kind: BuildKind) -> &'static str {
    match kind {
        BuildKind::Farm => "Grows food \u{2192} new villagers join your town",
        BuildKind::House => "Housing \u{2192} +2 population your town can hold",
        BuildKind::Lumber => "Woodcutter \u{2192} produces wood (needs a worker)",
    }
}

fn spawn_build(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    bank: Res<Bank>,
    town: Res<TownRes>,
    target: Res<BuildTarget>,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                margin: UiRect::new(Val::Px(-180.0), Val::Auto, Val::Px(-140.0), Val::Auto),
                width: Val::Px(360.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(20.0)),
                border: border(1.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            GlobalZIndex(60),
            BuildUi,
            anim(AnimKind::PopIn, 0.0, 0.22),
        ))
        .with_children(|root| {
            root.spawn(label(&fonts.extrabold, "BUILD", 20.0, GOLD));
            let buildable = target.0.is_some();
            if !buildable {
                root.spawn(label(&fonts.regular, "Stand on an empty plot to build.", 13.0, GREY));
            }
            for kind in MENU {
                let c = kind.cost();
                let afford = town.0.can_afford(kind, &bank.0) && buildable;
                let col = if afford { Color::WHITE } else { TEXT_FAINT };
                root.spawn((
                    Button,
                    Interaction::default(),
                    Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(9.0)),
                        border: border(1.0),
                        border_radius: radius(R_CARD),
                        ..default()
                    },
                    BorderColor::all(if afford { GOLD_DEEP } else { BORDER_SOFT }),
                    BuildOption(kind),
                ))
                .with_children(|b| {
                    // Left column: name + a plain-language line on what the building does.
                    let desc_col = if afford { GREY } else { TEXT_FAINT };
                    b.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|l| {
                        l.spawn(label(&fonts.semibold, kind.label(), 14.0, col));
                        l.spawn(label(&fonts.regular, build_desc(kind), 11.0, desc_col));
                    });
                    // Right: cost (only the resources actually needed).
                    let mut cost = String::new();
                    if c.wood > 0.0 {
                        cost.push_str(&format!("{} wood", c.wood as i64));
                    }
                    if c.stone > 0.0 {
                        if !cost.is_empty() {
                            cost.push_str("  ");
                        }
                        cost.push_str(&format!("{} stone", c.stone as i64));
                    }
                    b.spawn(label(&fonts.semibold, &cost, 13.0, col));
                });
            }
            root.spawn(label(&fonts.regular, "Esc to close", 11.0, GREY));
        });
}

fn despawn_build(mut commands: Commands, q: Query<Entity, With<BuildUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

#[allow(clippy::too_many_arguments)]
fn build_interact(
    q: Query<(&Interaction, &BuildOption), Changed<Interaction>>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    target: Res<BuildTarget>,
    mut next_modal: ResMut<NextState<Modal>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spots: Res<PlotSpots>,
    existing: Query<(Entity, &BuildingMesh)>,
) {
    for (interaction, opt) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(idx) = target.0 else { continue };
        let kind = opt.0;
        if town.0.build(idx, kind, &mut bank.0) {
            // Rebuild-on-rubble: clear any stale mesh first.
            for (e, bm) in &existing {
                if bm.idx == idx {
                    commands.entity(e).try_despawn();
                }
            }
            spawn_building_mesh(&mut commands, &mut meshes, &mut materials, idx, kind, &spots);
            next_modal.set(Modal::None); // close after a successful build
        }
    }
}

fn spawn_building_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    idx: usize,
    kind: BuildKind,
    spots: &PlotSpots,
) {
    let pos = spots.0.get(idx).copied().unwrap_or(Vec2::ZERO);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(building_mesh(kind))),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, y, pos.y),
        crate::biome::BiomeEntity,
        BuildingMesh { idx },
    ));
}

fn building_mesh(kind: BuildKind) -> Mesh {
    // Proper low-poly models (foundation/walls/gable-roof/door/window for the house, a
    // thatched hut + tilled field for the farm) live in `town_meshes`.
    match kind {
        BuildKind::Farm => crate::town_meshes::farm(),
        BuildKind::House => crate::town_meshes::house(),
        BuildKind::Lumber => crate::town_meshes::woodcutter(),
    }
}
