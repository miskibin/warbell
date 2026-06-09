//! **City-building town economy.** Wraps the tested `tileworld_core::town_store::Town`
//! as a Resource and owns: pre-placed build plots, the `Modal::Build` construction
//! menu, the production + population ticks, and the night burn/repair. Villagers
//! auto-staff producers (worker steering lives in `villagers.rs`); a fraction of
//! night invaders divert here to burn buildings (`orks.rs` pushes `PendingBuildingDamage`).
//!
//! Sim systems carry `.run_if(in_state(Modal::None))` per the freeze gate; VFX/render
//! stay ungated. Numbers live in `town_store` (test-gated).

use bevy::prelude::*;
use tileworld_core::town_store::{BuildKind, Cost, PopEvent, Town, HOUSE_COST};

use crate::castle::{Mats, VillageMats, M};
use crate::succession::Lives;
use crate::villagers::{Guard, Townsfolk};

use crate::economy::Bank;
use crate::game_state::{AppState, Modal};
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
        let mut t = Town::new(PLOT_COUNT, 0);
        t.reset(); // start with the founding houses + peasants (4 in 2 houses)
        Self(t)
    }
}

/// Tags a build-plot (the construction-site placeholder) entity, carrying its plot index so it can
/// be hidden once something is built there (and shown again on an empty/rubble plot).
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
                (auto_assign_workers, sync_staffed, release_orphan_workers, sync_plot_visibility)
                    .run_if(in_state(Modal::None)),
            )
            .add_systems(
                Update,
                (production_system, population_system, sync_population_bodies)
                    .run_if(in_state(Modal::None)),
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

/// Advance the food→population flow. A surplus settles a new peasant (grow the bloodline +
/// a green float); a sustained deficit starves one away (a red float). Bodies aren't touched
/// here — [`sync_population_bodies`] reconciles them to `town.population` next frame.
fn population_system(
    time: Res<Time>,
    mut town: ResMut<TownRes>,
    mut lives: ResMut<Lives>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
) {
    let dt = time.delta_secs() as f64;
    match town.0.population_tick(dt) {
        PopEvent::Grew => {
            lives.heirs += 1; // a new household → a new heir (keeps the population→bloodline tie)
            floats.0.push(crate::combat_fx::FloatReq {
                world: Vec3::new(0.0, 6.5, 5.0),
                text: "\u{1f331} A peasant settles in your town!".into(),
                color: Color::srgb(0.55, 1.0, 0.6),
                scale: 1.25,
            });
        }
        PopEvent::Starved => {
            floats.0.push(crate::combat_fx::FloatReq {
                world: Vec3::new(0.0, 6.5, 5.0),
                text: "\u{1f342} A peasant left \u{2014} not enough food".into(),
                color: Color::srgb(1.0, 0.5, 0.4),
                scale: 1.25,
            });
        }
        PopEvent::None => {}
    }
}

/// Keep the visible `Townsfolk` bodies matched to `town.population` — the single source of truth
/// for the town's headcount (grown by food, lost to starvation, added by rescue/recruit). Moves at
/// most one body per frame toward the target, so growth, starvation, a fresh run (jump to 4), and a
/// loaded save all converge without racing the deferred-command flush. Despawns prefer an idle
/// guard over a posted worker. Pilgrims and kids aren't `Townsfolk`, so they're never touched.
#[allow(clippy::type_complexity)]
fn sync_population_bodies(
    town: Res<TownRes>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    folk: Query<Entity, With<Townsfolk>>,
    idle_guards: Query<Entity, (With<Townsfolk>, With<Guard>, Without<Worker>)>,
    mut next_seed: Local<u32>,
) {
    let want = town.0.population as i64;
    let have = folk.iter().count() as i64;
    if have < want {
        *next_seed = next_seed.wrapping_add(1);
        let seed = 0xb0d1_0000u32.wrapping_add(next_seed.wrapping_mul(2654435761));
        crate::villagers::spawn_courtyard_guard(&mut commands, &mut meshes, &mut materials, seed);
    } else if have > want {
        // Prefer culling a standing guard; fall back to any townsperson.
        let victim = idle_guards.iter().next().or_else(|| folk.iter().next());
        if let Some(e) = victim {
            commands.entity(e).try_despawn();
        }
    }
}

/// New run: clear the town and seed starting wood. Mirrors `economy::reset_economy`.
fn reset_town(
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    stale: Query<Entity, Or<(With<BuildingMesh>, With<Flame>)>>,
) {
    town.0.reset();
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
    mats: Option<Res<VillageMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    stale: Query<Entity, Or<(With<BuildingMesh>, With<Flame>)>>,
) {
    if ev.read().count() == 0 {
        return;
    }
    let Some(mats) = mats else { return };
    for e in &stale {
        commands.entity(e).try_despawn();
    }
    for (idx, plot) in town.0.plots.iter().enumerate() {
        if let (true, Some(kind)) = (plot.is_built(), plot.kind) {
            spawn_building(&mut commands, &mut meshes, &mats.0, idx, kind, &spots);
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
pub fn populate_plots(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Mats) {
    let mut spots = Vec::with_capacity(PLOT_COUNT);
    for (idx, off) in PLOT_OFFSETS.iter().enumerate() {
        spots.push(*off);
        spawn_textured(commands, meshes, mats, BuildPlot { idx }, crate::town_meshes::plot_parts(), *off);
    }
    commands.insert_resource(PlotSpots(spots));
}

/// Hide a plot's construction-site placeholder once a building stands on it (and show it again on
/// an empty/rubble plot) — so you never see the timber frame poking through a finished building.
fn sync_plot_visibility(town: Res<TownRes>, mut q: Query<(&BuildPlot, &mut Visibility)>) {
    for (plot, mut vis) in &mut q {
        let built = town.0.plots.get(plot.idx).is_some_and(|p| p.is_built());
        *vis = if built { Visibility::Hidden } else { Visibility::Visible };
    }
}

/// Spawn a textured multi-material structure (one child mesh per `(Mesh, M)` part, sharing the
/// keep's [`VillageMats`]) under a parent carrying `tag` + a ground-snapped transform at `pos`.
/// `despawn`ing the parent reaps every part. Returns the parent entity.
fn spawn_textured(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &Mats,
    tag: impl Bundle,
    parts: Vec<(Mesh, M)>,
    pos: Vec2,
) -> Entity {
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let parent = commands
        .spawn((Transform::from_xyz(pos.x, y, pos.y), Visibility::Visible, crate::biome::BiomeEntity, tag))
        .id();
    commands.entity(parent).with_children(|p| {
        for (mesh, m) in parts {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.get(m)), Transform::default()));
        }
    });
    parent
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
    mats: Option<Res<VillageMats>>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *done || spots.0.is_empty() {
        return;
    }
    let Some(mats) = mats else { return };
    let Ok(mode) = std::env::var("FOREST_TOWN") else { *done = true; return };
    *done = true;
    bank.0.add_wood(100.0);
    bank.0.add_stone(100.0);
    town.0.build(0, BuildKind::Farm, &mut bank.0);
    town.0.build(2, BuildKind::Farm, &mut bank.0);
    town.0.build(3, BuildKind::Lumber, &mut bank.0);
    town.0.build_house(&mut bank.0); // raise one extra dwelling (castle reveals it)
    for idx in [0usize, 2, 3] {
        if let Some(kind) = town.0.plots[idx].kind {
            spawn_building(&mut commands, &mut meshes, &mats.0, idx, kind, &spots);
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

/// A row in the Build menu. Producers go on the outer plot you're standing on; a House is
/// raised inside the walls (a protected dwelling that lifts the population cap) and needs no plot.
#[derive(Clone, Copy)]
enum BuildItem {
    Producer(BuildKind),
    House,
}

#[derive(Component)]
struct BuildOption(BuildItem);

const MENU: [BuildItem; 3] =
    [BuildItem::Producer(BuildKind::Farm), BuildItem::House, BuildItem::Producer(BuildKind::Lumber)];

impl BuildItem {
    fn label(self) -> &'static str {
        match self {
            BuildItem::Producer(k) => k.label(),
            BuildItem::House => "House",
        }
    }

    fn cost(self) -> Cost {
        match self {
            BuildItem::Producer(k) => k.cost(),
            BuildItem::House => HOUSE_COST,
        }
    }

    /// One-line "what it does", so players see that a Farm *feeds the town and grows population*
    /// and a House *makes room for more people*, not just abstract stats.
    fn desc(self) -> &'static str {
        match self {
            BuildItem::Producer(BuildKind::Farm) => "Grows food \u{2192} feeds the town so peasants settle in",
            BuildItem::Producer(BuildKind::Lumber) => "Woodcutter \u{2192} produces wood (needs a worker)",
            BuildItem::House => "Home in the walls \u{2192} +2 people your town can hold",
        }
    }

    /// Whether this item can be built right now. Producers need an empty plot under the hero;
    /// a House just needs the resources and a free slot inside the walls.
    fn affordable(self, town: &Town, bank: &tileworld_core::resource_store::ResourceState, on_plot: bool) -> bool {
        match self {
            BuildItem::Producer(k) => on_plot && town.can_afford(k, bank),
            BuildItem::House => town.can_build_house(bank),
        }
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
            let on_plot = target.0.is_some();
            if !on_plot {
                root.spawn(label(&fonts.regular, "Stand on an empty plot to build a producer.", 13.0, GREY));
            }
            for item in MENU {
                let c = item.cost();
                let afford = item.affordable(&town.0, &bank.0, on_plot);
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
                    BuildOption(item),
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
                        l.spawn(label(&fonts.semibold, item.label(), 14.0, col));
                        l.spawn(label(&fonts.regular, item.desc(), 11.0, desc_col));
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
    mats: Option<Res<VillageMats>>,
    mut next_modal: ResMut<NextState<Modal>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    spots: Res<PlotSpots>,
    existing: Query<(Entity, &BuildingMesh)>,
) {
    let Some(mats) = mats else { return };
    for (interaction, opt) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match opt.0 {
            // A House goes up inside the walls (no plot): the castle reveals the next dwelling
            // and the population cap lifts. `sync_castle` shows the mesh from `town.houses`.
            BuildItem::House => {
                if town.0.build_house(&mut bank.0) {
                    next_modal.set(Modal::None);
                }
            }
            // A producer goes on the empty plot the hero is standing on.
            BuildItem::Producer(kind) => {
                let Some(idx) = target.0 else { continue };
                if town.0.build(idx, kind, &mut bank.0) {
                    // Rebuild-on-rubble: clear any stale mesh first.
                    for (e, bm) in &existing {
                        if bm.idx == idx {
                            commands.entity(e).try_despawn();
                        }
                    }
                    spawn_building(&mut commands, &mut meshes, &mats.0, idx, kind, &spots);
                    next_modal.set(Modal::None); // close after a successful build
                }
            }
        }
    }
}

fn spawn_building(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &Mats,
    idx: usize,
    kind: BuildKind,
    spots: &PlotSpots,
) {
    let pos = spots.0.get(idx).copied().unwrap_or(Vec2::ZERO);
    spawn_textured(commands, meshes, mats, BuildingMesh { idx }, building_parts(kind), pos);
    // Solid cottage: register a collision box over the dwelling (the −X side of the plot) so the
    // hero + orks route around it. (The crop field / log yard on the +X side stays walkable.)
    crate::blockers::add_box(pos.x - 0.95, pos.y, 1.05, 0.95);
}

/// The textured parts for a producer — both are a shared plaster cottage (matching the keep's
/// houses) plus the trade's own yard. Live in `town_meshes`.
fn building_parts(kind: BuildKind) -> Vec<(Mesh, M)> {
    match kind {
        BuildKind::Farm => crate::town_meshes::farm_parts(),
        BuildKind::Lumber => crate::town_meshes::woodcutter_parts(),
    }
}
