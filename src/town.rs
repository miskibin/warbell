//! **City-building town economy.** Wraps the tested `tileworld_core::town_store::Town`
//! as a Resource and owns: pre-placed build plots, the `Modal::Build` construction
//! menu, the production + population ticks, and the night burn/repair. Villagers
//! auto-staff producers (worker steering lives in `villagers.rs`); a fraction of
//! night invaders divert here to burn buildings (`orks.rs` pushes `PendingBuildingDamage`).
//!
//! Sim systems carry `.run_if(in_state(Modal::None))` per the freeze gate; VFX/render
//! stay ungated. Numbers live in `town_store` (test-gated).

use bevy::prelude::*;
use tileworld_core::town_store::{BuildKind, PopEvent, Town, POP_PER_HOUSE};

use crate::castle::{Mats, VillageMats, M};
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
        t.reset(); // the founding house + the larder pair it shelters (2 peasants)
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
            // (Pause-menu Restart / Load relaunch the process now — see game_state::RestartProcess.)
            .add_systems(OnEnter(Modal::Build), spawn_build)
            .add_systems(OnExit(Modal::Build), despawn_build)
            .add_systems(
                Update,
                (build_interact, build_hover_ghost, build_hover_hint).run_if(in_state(Modal::Build)),
            )
            // Self-explanation (ungated visuals): the gold ring marks WHICH plot the build
            // menu will use; the timber pad marks where the NEXT house will rise.
            .add_systems(Update, (sync_plot_highlight, sync_house_site_pad))
            // Trailer Director (F1 → "Build stronghold"): live, real-time construction timelapse.
            .add_systems(Update, director_build_timelapse.run_if(in_state(Modal::None)))
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
        // Clip-only: raise the town one plot at a time for a construction timelapse.
        if std::env::var("FOREST_DEMO").ok().as_deref() == Some("build") {
            app.add_systems(Update, demo_build_timelapse.run_if(in_state(Modal::None)));
        }
        // Clip-only: instant working village for the peasants-at-work scene.
        if std::env::var("FOREST_DEMO").ok().as_deref() == Some("work") {
            app.add_systems(Update, demo_work_setup);
        }
    }
}

/// By day, staff each producer from the idle townsfolk reserve: pick the nearest standing guard
/// (an unemployed `Townsfolk`) and swap its `Guard` role for a `Worker` job — it downs its weapon
/// and walks to the field. **Farms are staffed first** (food → population is what keeps the town
/// alive), so a thin workforce — e.g. the larder-regrown pair after a massacre night — goes to
/// the fields before any other producer sees a worker; if every hand is already employed
/// elsewhere, one is pulled off a non-farm plot. Skipped during a wave: nobody
/// gets pulled off the wall mid-assault, and `muster_townsfolk` has already fired everyone back to
/// guard duty at dusk.
#[allow(clippy::type_complexity)]
fn auto_assign_workers(
    town: Res<TownRes>,
    spots: Res<PlotSpots>,
    siege: Option<Res<crate::siege::Siege>>,
    mut commands: Commands,
    workers: Query<(Entity, &Worker)>,
    idle: Query<(Entity, &Transform), (With<Townsfolk>, With<Guard>, Without<Worker>)>,
) {
    if siege.is_some_and(|s| s.phase == crate::siege::GamePhase::Wave) {
        return; // night: defenders stay on the wall
    }
    // Visit plots farm-first (stable sort keeps index order within each group).
    let mut order: Vec<usize> = (0..town.0.plots.len()).collect();
    order.sort_by_key(|&i| !matches!(town.0.plots[i].kind, Some(BuildKind::Farm)));
    let mut claimed: Vec<Entity> = Vec::new(); // picked this frame (commands are deferred)
    let mut farm_short = false; // a built farm went unstaffed for want of hands
    for idx in order {
        let plot = &town.0.plots[idx];
        let Some(kind) = plot.kind else { continue };
        if !plot.is_built() || !kind.needs_worker() {
            continue;
        }
        if workers.iter().any(|(_, w)| w.idx == idx) {
            continue; // already has a worker assigned
        }
        let Some(spot) = spots.0.get(idx).copied() else { continue };
        // Nearest idle townsperson not already claimed this frame.
        let mut best: Option<(Entity, f32)> = None;
        for (e, tf) in &idle {
            if claimed.contains(&e) {
                continue;
            }
            let d = Vec2::new(tf.translation.x, tf.translation.z).distance(spot);
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((e, d));
            }
        }
        if let Some((e, _)) = best {
            // Off guard duty, onto the job (Guard → Worker; the two roles are exclusive).
            claimed.push(e);
            commands.entity(e).try_remove::<Guard>().try_insert(Worker { idx, at_post: false });
        } else if kind == BuildKind::Farm {
            farm_short = true;
        }
    }
    // A built farm went unstaffed with nobody idle → free ONE non-farm worker (wood can wait;
    // an unfed town starves). `rearm_townsfolk` re-arms it next frame and the farm-first
    // ordering above walks it to the field — converges without thrash once farms are staffed.
    if farm_short {
        if let Some((e, _)) = workers
            .iter()
            .find(|(_, w)| town.0.plots.get(w.idx).is_some_and(|p| !matches!(p.kind, Some(BuildKind::Farm))))
        {
            commands
                .entity(e)
                .try_remove::<Worker>()
                .try_remove::<crate::lumberjack::ChopJob>()
                .try_remove::<crate::miner::MineJob>();
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
    siege: Option<Res<crate::siege::Siege>>,
    mut town: ResMut<TownRes>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
) {
    // Growing a new peasant is a daytime thing: while the night wave is on, the food→population
    // flow pauses entirely — losses to the horde can't be replaced until dawn.
    if siege.is_some_and(|s| s.phase == crate::siege::GamePhase::Wave) {
        return;
    }
    let dt = time.delta_secs() as f64;
    match town.0.population_tick(dt) {
        PopEvent::Grew => {
            // (Heirs need no bump: `Lives.heirs` mirrors `town.population` — one headcount.)
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
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    // The dying are already subtracted from `population` (see `villagers::npc_damage_apply`) —
    // counting their still-fading bodies would make this reaper cull a second, living villager.
    // `Without<SceneActor>` so a screenshot-staged body (e.g. FOREST_VILLINE's guard) isn't
    // counted against the live population or culled by the over-count branch — the scene owns it.
    folk: Query<Entity, (With<Townsfolk>, Without<crate::dying::Dying>, Without<crate::scenes::SceneActor>)>,
    idle_guards: Query<
        Entity,
        (With<Townsfolk>, With<Guard>, Without<Worker>, Without<crate::dying::Dying>, Without<crate::scenes::SceneActor>),
    >,
    mut next_seed: Local<u32>,
) {
    let want = town.0.population as i64;
    let have = folk.iter().count() as i64;
    if have < want {
        *next_seed = next_seed.wrapping_add(1);
        let seed = 0xb0d1_0000u32.wrapping_add(next_seed.wrapping_mul(2654435761));
        crate::villagers::spawn_courtyard_guard(&mut commands, &mut meshes, &mut creature_mats, seed);
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
    siege: Option<Res<crate::siege::Siege>>,
    mut commands: Commands,
    stale: Query<Entity, Or<(With<BuildingMesh>, With<Flame>)>>,
) {
    town.0.reset();
    // Difficulty handicap: Easy seeds spare townsfolk — heirs ARE the headcount now, so the
    // old "spare heirs" grant lands here. They arrive housed (a free house per pair), but only
    // the larder pair eats free: keeping the spares fed means building farms.
    let diff = siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);
    let bonus = crate::siege::mods_for(diff).heirs_bonus;
    town.0.population += bonus;
    town.0.houses += bonus.div_ceil(POP_PER_HOUSE);
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

/// World-XZ radius around each plot centre that must stay clear so a future building has room:
/// `worldmap::classify` forces flat grass here, and chest / ground-cover placement rejects it.
pub const PLOT_CLEAR_R: f32 = 3.4;

/// Is `(wx, wz)` inside the clear zone of any town build plot?
pub fn near_build_plot(wx: f32, wz: f32) -> bool {
    PLOT_OFFSETS.iter().any(|o| (wx - o.x).hypot(wz - o.y) < PLOT_CLEAR_R)
}

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
    town.0.build(1, BuildKind::Mine, &mut bank.0);
    town.0.build(2, BuildKind::Farm, &mut bank.0);
    town.0.build(3, BuildKind::Lumber, &mut bank.0);
    town.0.build_house(&mut bank.0); // raise one extra dwelling (castle reveals it)
    // `FOREST_TOWN=full`: raise every dwelling slot so a shot shows all twelve house
    // archetypes (and the house-gated dressing — laundry, gardens, woodpiles).
    if mode == "full" {
        bank.0.add_wood(600.0);
        bank.0.add_stone(600.0);
        for _ in 0..11 {
            town.0.build_house(&mut bank.0);
        }
    }
    for idx in [0usize, 1, 2, 3] {
        if let Some(kind) = town.0.plots[idx].kind {
            spawn_building(&mut commands, &mut meshes, &mats.0, idx, kind, &spots);
        }
    }
    if mode == "burn" {
        town.0.damage(0, 20.0);
        spawn_flame(&mut commands, &mut meshes, &mut materials, 0, &spots);
    }
}

/// Demo hook (`FOREST_DEMO=work`): instantly stand up a working village — several lumber + mine
/// yards (so woodcutters walk out and fell real trees, miners cart stone) plus farms and houses
/// for the population that staffs them. The warm-up lets the workers reach their jobs before
/// recording. Clip-only; never wired in real play.
#[allow(clippy::too_many_arguments)]
fn demo_work_setup(
    mut done: Local<bool>,
    spots: Res<PlotSpots>,
    mats: Option<Res<VillageMats>>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if *done || spots.0.is_empty() {
        return;
    }
    let Some(mats) = mats else { return };
    *done = true;
    bank.0.add_wood(4000.0);
    bank.0.add_stone(4000.0);
    const PLAN: [BuildKind; 8] = [
        BuildKind::Lumber, BuildKind::Mine, BuildKind::Lumber, BuildKind::Farm,
        BuildKind::Mine, BuildKind::Lumber, BuildKind::Farm, BuildKind::Mine,
    ];
    for (i, k) in PLAN.iter().enumerate() {
        raise_plot(i, *k, &mut town, &mut bank, &mut commands, &mut meshes, &mats.0, &spots);
    }
    for _ in 0..6 {
        town.0.build_house(&mut bank.0); // dwellings raise the pop cap → more workers staff the yards
    }
    // The workforce: producers staff from the idle `Townsfolk` (guard) pool — without bodies the
    // yards stand empty and the clip shows a village where nobody works. The HEADCOUNT is the
    // source of truth (`sync_population_bodies` reconciles bodies to it and culls strays — direct
    // guard spawns get reaped right back to `population`), so raise the number and let the sync
    // grow the bodies; auto-assign then staffs farms first, lumber/mine after.
    town.0.population = 14;
}

/// Raise one producer building on plot `i` (build + spawn its mesh).
#[allow(clippy::too_many_arguments)]
fn raise_plot(
    i: usize,
    kind: BuildKind,
    town: &mut TownRes,
    bank: &mut Bank,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &Mats,
    spots: &PlotSpots,
) {
    if i >= spots.0.len() {
        return;
    }
    town.0.build(i, kind, &mut bank.0);
    if town.0.plots[i].kind.is_some() {
        spawn_building(commands, meshes, mats, i, kind, spots);
    }
}

/// Demo hook (`FOREST_DEMO=build`): raise the whole stronghold one piece at a time for a
/// construction timelapse — palisade walls, gate, watchtowers + defences (the `castle.rs` parts
/// reveal off the live `Defenses` flags), interleaved with producer plots and dwellings. Steps off
/// the clip's frame-locked clock and waits for recording so the warm-up doesn't burn the sequence.
/// Clip-only; never wired in real play.
#[allow(clippy::too_many_arguments)]
fn demo_build_timelapse(
    prog: Option<Res<crate::capture::ClipProgress>>,
    spots: Res<PlotSpots>,
    mats: Option<Res<VillageMats>>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut def: ResMut<crate::economy::Defenses>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last: Local<i32>,
    mut primed: Local<bool>,
) {
    const STEP: u32 = 24; // recorded frames between actions (~0.8s at the 30fps playback rate)
    if spots.0.is_empty() {
        return;
    }
    let Some(mats) = mats else { return };
    let Some(prog) = prog.as_ref() else { return };
    if !prog.recording {
        return;
    }
    if !*primed {
        *primed = true;
        *last = -1;
        bank.0.add_wood(4000.0);
        bank.0.add_stone(4000.0);
    }
    let step = (prog.frame / STEP) as i32;
    if step <= *last {
        return;
    }
    let mat = &mats.0;
    for s in (*last + 1)..=step {
        build_step(s, &mut town, &mut bank, &mut def, &mut commands, &mut meshes, mat, &spots);
    }
    *last = step;
}

/// One step (0..=16) of the stronghold construction timelapse, shared by the clip demo
/// ([`demo_build_timelapse`]) and the live Director ([`director_build_timelapse`]): defence flags
/// flip (castle.rs reveals the parts off `Defenses`), producer plots + houses spawn their meshes.
#[allow(clippy::too_many_arguments)]
fn build_step(
    s: i32,
    town: &mut TownRes,
    bank: &mut Bank,
    def: &mut crate::economy::Defenses,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Mats,
    spots: &PlotSpots,
) {
    match s {
        0 => def.walls = true, // palisade goes up (the bare yard gives way to the courtyard)
        1 => raise_plot(0, BuildKind::Farm, town, bank, commands, meshes, mat, spots),
        2 => {
            town.0.build_house(&mut bank.0);
        }
        3 => def.gate = true,
        4 => raise_plot(1, BuildKind::Lumber, town, bank, commands, meshes, mat, spots),
        5 => {
            town.0.build_house(&mut bank.0);
        }
        6 => def.towers = true, // four corner watchtowers
        7 => raise_plot(2, BuildKind::Mine, town, bank, commands, meshes, mat, spots),
        8 => raise_plot(3, BuildKind::Farm, town, bank, commands, meshes, mat, spots),
        9 => {
            def.tower_mastery = true;
            def.keep_archers = true; // archers man the keep roof
        }
        10 => {
            town.0.build_house(&mut bank.0);
        }
        11 => raise_plot(4, BuildKind::Lumber, town, bank, commands, meshes, mat, spots),
        12 => def.ballista = true, // ballista north of the gate
        13 => raise_plot(5, BuildKind::Mine, town, bank, commands, meshes, mat, spots),
        14 => raise_plot(6, BuildKind::Farm, town, bank, commands, meshes, mat, spots),
        15 => {
            def.shrine = true;
            town.0.build_house(&mut bank.0);
        }
        16 => raise_plot(7, BuildKind::Lumber, town, bank, commands, meshes, mat, spots),
        _ => {}
    }
}

/// Live Director build timelapse (F1 → "Build stronghold"): the same 17-step reveal as the clip
/// demo, but stepped off REAL time so the user films it with the free-cam. One-shot per toggle —
/// flip `build_run` off and on to re-prime (it does not despawn what's already up).
#[allow(clippy::too_many_arguments)]
pub fn director_build_timelapse(
    state: Res<crate::cinematic::DirectorState>,
    spots: Res<PlotSpots>,
    mats: Option<Res<VillageMats>>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut def: ResMut<crate::economy::Defenses>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    mut last: Local<i32>,
    mut primed: Local<bool>,
    mut acc: Local<f32>,
) {
    const SECS_PER_STEP: f32 = 0.35; // 1.5× slower than the old 0.233 — each pop+dust gets room to read
    if !state.build_run {
        if *primed {
            *primed = false;
            *last = -1;
            *acc = 0.0;
        }
        return;
    }
    if spots.0.is_empty() {
        return;
    }
    let Some(mats) = mats else { return };
    if !*primed {
        *primed = true;
        *last = -1;
        // Start the step clock in the red: a camera-setting grace before the first piece rises.
        *acc = -crate::cinematic::PRE_ROLL;
        bank.0.add_wood(4000.0);
        bank.0.add_stone(4000.0);
    }
    *acc += time.delta_secs();
    if *acc < 0.0 {
        return; // still in the pre-roll
    }
    let step = ((*acc / SECS_PER_STEP) as i32).min(16);
    if step <= *last {
        return;
    }
    let mat = &mats.0;
    for s in (*last + 1)..=step {
        build_step(s, &mut town, &mut bank, &mut def, &mut commands, &mut meshes, mat, &spots);
    }
    *last = step;
}

// ── Modal::Build panel ────────────────────────────────────────────────────────────────────

#[derive(Component)]
struct BuildUi;

/// A row in the Build menu — producers only. The menu is strictly "what to raise on THIS
/// plot": Houses left it (they used to appear inside the walls while the player stood on an
/// outer plot — a building rising somewhere other than where you chose read as a bug). A
/// House is now raised on its own marked site in the courtyard via the on-site **E** prompt.
#[derive(Component)]
struct BuildOption(BuildKind);

const MENU: [BuildKind; 3] = [BuildKind::Farm, BuildKind::Lumber, BuildKind::Mine];

/// One-line "what it does", so players see that a Farm *feeds the town and grows population*,
/// not just abstract stats. Shown in the hint line while the row is hovered.
fn build_desc(kind: BuildKind) -> &'static str {
    match kind {
        BuildKind::Farm => "Grows food \u{2192} feeds the town so peasants settle in",
        BuildKind::Lumber => "Woodcutter \u{2192} fells real trees and hauls the logs home (needs a worker)",
        BuildKind::Mine => "Stone Miner \u{2192} mines real boulders and carts the stone home (needs a worker)",
    }
}

/// The building's pictogram in the [`IconAtlas`] (the tintable stat-bar game-icons double as
/// build icons: a Farm makes food, a Woodcutter wood, a Miner stone).
fn build_icon_id(kind: BuildKind) -> &'static str {
    match kind {
        BuildKind::Farm => "stat:food",
        BuildKind::Lumber => "stat:wood",
        BuildKind::Mine => "stat:stone",
    }
}

/// The hint line's resting text (no row hovered) — covers the two things the old footer said.
const HINT_IDLE: &str = "Houses rise at the timber pad in the walls \u{b7} Esc to close";

/// The one-line caption above the build rows; `build_hover_hint` swaps it to the hovered
/// row's description (Anno-style tooltip) and back.
#[derive(Component)]
struct BuildHint;

fn spawn_build(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    bank: Res<Bank>,
    town: Res<TownRes>,
    target: Res<BuildTarget>,
    icons: Res<crate::ui::icons::IconAtlas>,
) {
    // Docked bottom-centre, just above the quick-bar — a slim icon column, not a screen-filling
    // modal: the plot (gold ring) and the hover ghost stay visible behind it while choosing.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(96.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            bevy::ui::FocusPolicy::Pass,
            GlobalZIndex(60),
            BuildUi,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Px(248.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(5.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    border: border(1.0),
                    border_radius: radius(R_PANEL),
                    ..default()
                },
                widgets::card_paint(),
                anim(AnimKind::PopIn, 0.0, 0.18),
            ))
            .with_children(|card| {
                let on_plot = target.0.is_some();
                card.spawn((
                    Node { max_width: Val::Px(232.0), ..default() },
                    label(
                        &fonts.regular,
                        if on_plot { HINT_IDLE } else { "Stand on an empty plot to build." },
                        11.0,
                        GREY,
                    ),
                    BuildHint,
                ));
                for item in MENU {
                    let c = item.cost();
                    let afford = on_plot && town.0.can_afford(item, &bank.0);
                    let col = if afford { Color::WHITE } else { TEXT_FAINT };
                    let tint = if afford { GOLD } else { TEXT_FAINT };
                    card.spawn((
                        Button,
                        Interaction::default(),
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(8.0),
                            padding: UiRect::axes(Val::Px(9.0), Val::Px(6.0)),
                            border: border(1.0),
                            border_radius: radius(R_CARD),
                            ..default()
                        },
                        BackgroundColor(BTN_BG),
                        BorderColor::all(if afford { GOLD_DEEP } else { BORDER_SOFT }),
                        BuildOption(item),
                    ))
                    .with_children(|b| {
                        if let Some(entry) = icons.get_tintable(build_icon_id(item)) {
                            b.spawn(widgets::icon_tinted(entry, 20.0, tint));
                        }
                        b.spawn(label(&fonts.semibold, item.label(), 13.0, col));
                        b.spawn(Node { flex_grow: 1.0, ..default() }); // cost hugs the right edge
                        for (key, amount) in [("stat:wood", c.wood), ("stat:stone", c.stone)] {
                            if amount <= 0.0 {
                                continue;
                            }
                            if let Some(entry) = icons.get_tintable(key) {
                                b.spawn(widgets::icon_tinted(entry, 11.0, col));
                            }
                            b.spawn(label(&fonts.semibold, format!("{}", amount as i64), 11.0, col));
                        }
                    });
                }
            });
        });
}

/// Swap the hint line to the hovered row's plain-language description, and back to the resting
/// text when the pointer leaves — the rows themselves stay icon + name + cost only.
fn build_hover_hint(
    btns: Query<(&Interaction, &BuildOption)>,
    target: Res<BuildTarget>,
    mut hint: Query<&mut Text, With<BuildHint>>,
) {
    let Ok(mut text) = hint.single_mut() else { return };
    let hovered = btns
        .iter()
        .find_map(|(i, o)| (*i == Interaction::Hovered).then_some(o.0));
    let new = match hovered {
        Some(kind) => build_desc(kind),
        None if target.0.is_none() => "Stand on an empty plot to build.",
        None => HINT_IDLE,
    };
    if text.0 != new {
        text.0 = new.to_string();
    }
}

fn despawn_build(mut commands: Commands, q: Query<Entity, Or<(With<BuildUi>, With<BuildGhost>)>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ── Self-explanation visuals: plot ring, ghost preview, house site pad ─────────────────

/// The pulsing gold ring laid on the plot the build menu targets — so "press E to build"
/// and the menu's choice are visibly anchored to ONE spot in the world.
#[derive(Component)]
struct PlotHighlight;

/// A translucent preview of the hovered menu row, standing on the target plot — you see the
/// building where it will rise *before* you click.
#[derive(Component)]
struct BuildGhost;

/// The construction-site pad marking where the NEXT house will rise inside the walls
/// (`castle::next_house_site`); the on-site **E** prompt raises it right there.
#[derive(Component)]
struct HouseSitePad;

/// Keep the gold ring on the targeted plot: visible while the hero stands on a buildable plot
/// (`BuildTarget`) in open play or with the Build menu up; hidden otherwise. Lazy-spawned on
/// first run (not biome-tagged — it's a permanent FX entity, just repositioned).
fn sync_plot_highlight(
    time: Res<Time>,
    target: Res<BuildTarget>,
    spots: Res<PlotSpots>,
    app: Option<Res<State<AppState>>>,
    modal: Option<Res<State<Modal>>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<PlotHighlight>>,
) {
    let Ok((mut tf, mut vis)) = q.single_mut() else {
        // One ring, built once: flat gold annulus a hair above the pad (same planar-flash
        // recipe as the combat rings — unlit + emissive so it reads day and night).
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.86, 0.5, 0.55),
            emissive: LinearRgba::rgb(1.6, 1.1, 0.4),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Annulus::new(2.45, 2.8).mesh().resolution(48).build())),
            MeshMaterial3d(mat),
            Transform::from_xyz(0.0, -10.0, 0.0)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Visibility::Hidden,
            PlotHighlight,
        ));
        return;
    };
    let playing = app.is_some_and(|s| *s.get() == AppState::Playing);
    let shown = modal.is_some_and(|m| matches!(*m.get(), Modal::None | Modal::Build));
    let spot = target.0.and_then(|i| spots.0.get(i).copied());
    match (playing && shown, spot) {
        (true, Some(p)) => {
            let y = crate::worldmap::ground_at_world(p.x, p.y).unwrap_or(0.0);
            tf.translation = Vec3::new(p.x, y + 0.06, p.y);
            // A slow breath, not a strobe — enough motion to say "this one".
            let pulse = 1.0 + (time.elapsed_secs() * 2.6).sin() * 0.04;
            tf.scale = Vec3::splat(pulse);
            *vis = Visibility::Visible;
        }
        _ => *vis = Visibility::Hidden,
    }
}

/// While the Build menu is up, hovering a row stands a translucent ghost of that building on
/// the targeted plot. Rebuilt only when the hovered row changes; reaped with the panel.
fn build_hover_ghost(
    target: Res<BuildTarget>,
    spots: Res<PlotSpots>,
    btns: Query<(&Interaction, &BuildOption)>,
    ghosts: Query<Entity, With<BuildGhost>>,
    mut current: Local<Option<BuildKind>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hovered = btns
        .iter()
        .find_map(|(i, o)| (*i == Interaction::Hovered).then_some(o.0));
    // `current` is a Local and survives the panel closing (which reaps the ghost), so a bare
    // equality check would skip respawning the same row next time — require a live ghost too.
    if hovered == *current && (hovered.is_none() || !ghosts.is_empty()) {
        return;
    }
    *current = hovered;
    for e in &ghosts {
        commands.entity(e).try_despawn();
    }
    let (Some(kind), Some(idx)) = (hovered, target.0) else { return };
    let Some(pos) = spots.0.get(idx).copied() else { return };
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    // One shared see-through material for every part — a hologram, not a finished building.
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.92, 1.0, 0.38),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let parent = commands
        .spawn((Transform::from_xyz(pos.x, y, pos.y), Visibility::Visible, BuildGhost))
        .id();
    commands.entity(parent).with_children(|p| {
        for (mesh, _) in building_parts(kind) {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat.clone()), Transform::default()));
        }
    });
}

/// Keep one construction-site pad standing on the next free dwelling slot inside the walls,
/// so "where do houses go?" has a visible answer in the world (the on-site E raises it there).
/// Lazy-spawned and self-healing: a biome swap reaps it (`BiomeEntity`), we respawn next frame.
fn sync_house_site_pad(
    town: Res<TownRes>,
    mats: Option<Res<VillageMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<HouseSitePad>>,
) {
    let site = crate::castle::next_house_site(town.0.houses);
    if let Ok((mut tf, mut vis)) = q.single_mut() {
        match site {
            Some(p) => {
                let y = crate::worldmap::ground_at_world(p.x, p.y).unwrap_or(0.0);
                tf.translation = Vec3::new(p.x, y, p.y);
                *vis = Visibility::Visible;
            }
            None => *vis = Visibility::Hidden, // all twelve dwellings stand
        }
    } else if let (Some(p), Some(mats)) = (site, mats) {
        spawn_textured(&mut commands, &mut meshes, &mats.0, HouseSitePad, crate::town_meshes::house_site_parts(), p);
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
        // The producer goes on the empty plot the hero is standing on — exactly the one the
        // gold ring marks (`sync_plot_highlight`).
        let kind = opt.0;
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

fn spawn_building(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &Mats,
    idx: usize,
    kind: BuildKind,
    spots: &PlotSpots,
) {
    let pos = spots.0.get(idx).copied().unwrap_or(Vec2::ZERO);
    let parent = spawn_textured(commands, meshes, mats, BuildingMesh { idx }, building_parts(kind), pos);
    // Construction feedback: the fresh building pops up out of its plot on a kick of dust
    // (build_fx). Re-insert the parent transform pre-shrunk so it never flashes full-size.
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let pop = crate::build_fx::BuildPop::pop();
    commands
        .entity(parent)
        .try_insert((Transform::from_xyz(pos.x, y, pos.y).with_scale(pop.scale0()), pop));
    commands.spawn(crate::build_fx::DustBurst::building(Vec3::new(pos.x, y, pos.y)));
    // Solid structure: register a collision box over the trade's building (the −X side of the
    // plot) so the hero + orks route around it. (The working yard on the +X side stays walkable.)
    crate::blockers::add_box(pos.x - 0.95, pos.y, 1.05, 0.95);
}

/// The textured parts for a producer — each trade has its own structure (barn / saw shed /
/// pit-head) plus its working yard. Live in `town_meshes`.
fn building_parts(kind: BuildKind) -> Vec<(Mesh, M)> {
    match kind {
        BuildKind::Farm => crate::town_meshes::farm_parts(),
        BuildKind::Lumber => crate::town_meshes::woodcutter_parts(),
        BuildKind::Mine => crate::town_meshes::mine_parts(),
    }
}
