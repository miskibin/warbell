//! **City-building town economy.** Wraps the tested `tileworld_core::town_store::Town`
//! as a Resource and owns: pre-placed build plots, the live **Build mode** (a HUD-button
//! toggled placement state — see [`BuildMode`] — where every plot glows and walking onto one
//! and pressing **E** raises the chosen building), the production + population ticks, and the
//! night burn/repair. Build mode is a plain resource read under `Modal::None`, NOT a freeze
//! Modal, because the player walks the knight around while placing. Villagers
//! auto-staff producers (worker steering lives in `villagers.rs`); a fraction of
//! night invaders divert here to burn buildings (`orks.rs` pushes `PendingBuildingDamage`).
//!
//! Sim systems carry `.run_if(in_state(Modal::None))` per the freeze gate; VFX/render
//! stay ungated. Numbers live in `town_store` (test-gated).

use bevy::prelude::*;
use tileworld_core::town_store::{BuildKind, Cost, PopEvent, Town, HOUSE_COST, POP_PER_HOUSE};

use crate::castle::{Mats, VillageMats, M};
use crate::combat_fx::FloatReq;
use crate::player::HeroState;
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

/// A buildable thing in the unified Build palette. `House` raises a dwelling on the next free
/// courtyard slot (`castle::next_house_site`); `Producer` raises a yield building on an outer
/// plot. Kept as data (see [`BUILD_TYPES`]) so adding a building later is a single entry — the
/// foundation the future RTS expansion builds on (the placement layer stays decoupled from the
/// core `town.build` / `build_house` calls).
#[derive(Clone, Copy, PartialEq)]
pub enum BuildType {
    House,
    Producer(BuildKind),
}

/// The Build palette, in display order. **Add a building here** and the strip, glow rings, costs,
/// and placement all pick it up — no other edit needed.
pub const BUILD_TYPES: [BuildType; 4] = [
    BuildType::House,
    BuildType::Producer(BuildKind::Farm),
    BuildType::Producer(BuildKind::Lumber),
    BuildType::Producer(BuildKind::Mine),
];

impl BuildType {
    fn label(self) -> &'static str {
        match self {
            BuildType::House => "House",
            BuildType::Producer(k) => k.label(),
        }
    }
    /// One-line "what it does" for the strip's hint line.
    fn desc(self) -> &'static str {
        match self {
            BuildType::House => "Beds for 2 more peasants \u{2014} raises the town's population cap",
            BuildType::Producer(BuildKind::Farm) => "Grows food \u{2192} feeds the town so peasants settle in",
            BuildType::Producer(BuildKind::Lumber) => "Woodcutter \u{2192} fells real trees and hauls the logs home (needs a worker)",
            BuildType::Producer(BuildKind::Mine) => "Stone Miner \u{2192} mines real boulders and carts the stone home (needs a worker)",
        }
    }
    /// The tintable stat-bar game-icon that doubles as this building's pictogram.
    fn icon_id(self) -> &'static str {
        match self {
            BuildType::House => "stat:pop",
            BuildType::Producer(BuildKind::Farm) => "stat:food",
            BuildType::Producer(BuildKind::Lumber) => "stat:wood",
            BuildType::Producer(BuildKind::Mine) => "stat:stone",
        }
    }
    fn cost(self) -> Cost {
        match self {
            BuildType::House => HOUSE_COST,
            BuildType::Producer(k) => k.cost(),
        }
    }
    fn is_house(self) -> bool {
        matches!(self, BuildType::House)
    }
}

/// Where the selected building would land given the hero's position — the plot/slot a press of
/// **E** in build mode would raise it on. `None` = the hero isn't standing on a valid spot for
/// the selected type (so nothing happens, and the strip says "walk to a glowing spot").
enum BuildSpot {
    Plot(usize),
    House(Vec2),
}

/// Range (world units) the hero must be within to place on a plot / the house slot — mirrors the
/// old `interaction.rs` `BUILD_DIST` / `HOUSE_DIST`.
const BUILD_DIST: f32 = 3.0;
const HOUSE_DIST: f32 = 3.0;

/// The plot/slot the hero is standing on for `kind`, or `None`. Producers pick the nearest free
/// outer plot in range; House the next courtyard slot if the hero is on it.
fn current_build_spot(kind: BuildType, pos: Vec2, town: &Town, spots: &PlotSpots) -> Option<BuildSpot> {
    match kind {
        BuildType::House => {
            let site = crate::castle::next_house_site(town.houses)?;
            (pos.distance(site) < HOUSE_DIST).then_some(BuildSpot::House(site))
        }
        BuildType::Producer(_) => {
            let mut best: Option<(usize, f32)> = None;
            for (idx, spot) in spots.0.iter().enumerate() {
                if town.plots.get(idx).is_some_and(|p| p.is_buildable()) {
                    let d = pos.distance(*spot);
                    if d < BUILD_DIST && best.map_or(true, |(_, bd)| d < bd) {
                        best = Some((idx, d));
                    }
                }
            }
            best.map(|(i, _)| BuildSpot::Plot(i))
        }
    }
}

/// Wood/stone still short of a cost, as a player-facing line — `None` if affordable.
fn cost_shortfall(cost: Cost, bank: &tileworld_core::resource_store::ResourceState) -> Option<String> {
    let nw = (cost.wood - bank.wood()).max(0.0).ceil() as i64;
    let ns = (cost.stone - bank.stone()).max(0.0).ceil() as i64;
    match (nw, ns) {
        (0, 0) => None,
        (w, 0) => Some(format!("need {w} more wood")),
        (0, s) => Some(format!("need {s} more stone")),
        (w, s) => Some(format!("need {w} wood + {s} stone")),
    }
}

/// **Build mode** — the live, world-unfrozen placement state. Toggled by the HUD `Build` button
/// (`hud.rs`). While `active`, every valid plot glows (`sync_build_rings`), a docked palette names
/// what to raise, and walking onto a lit spot + **E** raises it there (`build_place`). A plain
/// resource read under `Modal::None` (NOT a freeze-gated `Modal`) precisely because the player
/// walks the knight around while placing.
#[derive(Resource, Default)]
pub struct BuildMode {
    pub active: bool,
    /// Index into [`BUILD_TYPES`] — the currently selected building.
    pub sel: usize,
}

impl BuildMode {
    pub fn kind(&self) -> BuildType {
        BUILD_TYPES[self.sel.min(BUILD_TYPES.len() - 1)]
    }
}

/// **B** enters build mode and then cycles the selected building (House → Farm → Lumber → Mine →
/// off); the HUD `Build` button toggles it too, and a palette row click selects directly. Esc-exit
/// lives in `game_state::pause_toggle` (so Esc leaves build mode instead of pausing). Keyboard-first
/// on purpose: combat locks the mouse cursor (`player::camera`), so a click-only entry would be dead
/// at dawn — right after a night of fighting, the prime rebuild moment.
fn build_mode_keys(
    keys: Res<ButtonInput<KeyCode>>,
    siege: Option<Res<crate::siege::Siege>>,
    mut mode: ResMut<BuildMode>,
) {
    // Build mode is a daytime activity — the night assault force-exits it (you defend, not build).
    let prep = siege.map_or(true, |s| s.phase == crate::siege::GamePhase::Prep);
    if mode.active && !prep {
        mode.active = false;
        return;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        if !mode.active {
            if prep {
                mode.active = true;
                mode.sel = 0;
            }
        } else if mode.sel + 1 < BUILD_TYPES.len() {
            mode.sel += 1;
        } else {
            mode.active = false;
            mode.sel = 0;
        }
    }
}

pub struct TownPlugin;

impl Plugin for TownPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TownRes>()
            .init_resource::<PendingBuildingDamage>()
            .init_resource::<BuildMode>()
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
            // Build mode (live — world stays unfrozen so the hero walks while placing): spawn /
            // despawn the palette on toggle, drive its rows, glow every valid plot, and place on E.
            .add_systems(
                Update,
                (build_mode_keys, build_strip_input, build_strip_update, build_place)
                    .run_if(in_state(Modal::None)),
            )
            // Ungated, but self-gated on `Modal::None` inside: the palette + glow rings must be
            // REAPED when play is left or a panel opens (a `Modal::None` run-condition would just
            // stop running and strand the strip over the pause / game-over screen).
            .add_systems(Update, (sync_build_strip, sync_build_rings))
            // The timber pad marks where the NEXT house will rise (visible even outside build mode).
            .add_systems(Update, sync_house_site_pad)
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
    mut build_mode: ResMut<BuildMode>,
    siege: Option<Res<crate::siege::Siege>>,
    mut commands: Commands,
    stale: Query<Entity, Or<(With<BuildingMesh>, With<Flame>)>>,
) {
    town.0.reset();
    *build_mode = BuildMode::default(); // never resume a new run with build mode stuck on (in-process Continue)
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
    mut fa: Local<Option<FlameAssets>>,
    spots: Res<PlotSpots>,
    buildings: Query<(Entity, &BuildingMesh)>,
    flames: Query<(Entity, &Flame)>,
) {
    if pending.0.is_empty() {
        return;
    }
    // Plots that already show a flame. Tracked across the drain loop so multiple damage events to
    // the SAME plot in one frame (every arsonist pushes damage every frame) don't each spawn a
    // flame — `commands` aren't flushed mid-system, so the `flames` query alone can't see a flame
    // queued earlier this frame, and the building used to sprout a stack of overlapping flames.
    let mut burning: std::collections::HashSet<usize> = flames.iter().map(|(_, f)| f.idx).collect();
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
            burning.remove(&idx);
        } else if burning.insert(idx) {
            // Newly burning this frame (insert returns true) → show exactly one flame.
            let assets = fa.get_or_insert_with(|| flame_assets(&mut meshes, &mut materials));
            spawn_flame(&mut commands, assets, idx, &spots);
        }
    }
}

/// Shared flame mesh + material — built once and cloned per flame. Per-flame `meshes.add` /
/// `materials.add` used to mint a UNIQUE mesh+material for every burning plot, which broke the
/// renderer's instanced batching (each flame = its own draw call) and churned the asset stores.
/// One handle pair keeps every flame in a single batch.
#[derive(Clone)]
struct FlameAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

fn flame_assets(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> FlameAssets {
    FlameAssets {
        mesh: meshes.add(Sphere::new(0.6).mesh().ico(1).unwrap()),
        mat: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.45, 0.1),
            emissive: LinearRgba::rgb(6.0, 2.0, 0.3),
            ..default()
        }),
    }
}

fn spawn_flame(commands: &mut Commands, fa: &FlameAssets, idx: usize, spots: &PlotSpots) {
    let pos = spots.0.get(idx).copied().unwrap_or(Vec2::ZERO);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    commands.spawn((
        Mesh3d(fa.mesh.clone()),
        MeshMaterial3d(fa.mat.clone()),
        Transform::from_xyz(pos.x, y + 1.6, pos.y),
        crate::biome::BiomeEntity,
        // A glowing emissive blob — it should never cast a shadow (a flame casting a hard sphere
        // shadow looks wrong anyway), so keep it out of every cascade's shadow pass.
        bevy::light::NotShadowCaster,
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
    mut mode: ResMut<BuildMode>,
    mut bank: ResMut<Bank>,
    mut done: Local<bool>,
) {
    if *done || std::env::var("FOREST_PANEL").ok().as_deref() != Some("build") {
        return;
    }
    if *app.get() == AppState::Playing {
        *done = true;
        bank.0.add_wood(50.0);
        bank.0.add_stone(50.0);
        mode.active = true; // pop the build palette + glow rings for the capture
        mode.sel = 1; // Farm selected → the outer producer-plot rings light up (not just the house slot)
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
        let fa = flame_assets(&mut meshes, &mut materials);
        spawn_flame(&mut commands, &fa, 0, &spots);
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

// ── Build mode: the docked palette (what to raise) ──────────────────────────────────────────

#[derive(Component)]
struct BuildUi;

/// A row in the unified Build palette — its index into [`BUILD_TYPES`] (House + the producers).
#[derive(Component)]
struct BuildOption(usize);

/// The hint line under the palette — `build_strip_update` swaps it between the selected building's
/// placement state ("walk to a glowing spot" / "press E here") and its afford shortfall.
#[derive(Component)]
struct BuildHint;

/// Spawn / despawn the docked Build palette to match [`BuildMode::active`] (an edge check, so the
/// strip is built once on entering build mode and reaped on leaving — not rebuilt per frame, which
/// would reset the rows' `Interaction` and eat clicks).
fn sync_build_strip(
    mode: Res<BuildMode>,
    modal: Option<Res<State<Modal>>>,
    existing: Query<Entity, With<BuildUi>>,
    fonts: Res<UiFonts>,
    icons: Res<crate::ui::icons::IconAtlas>,
    mut commands: Commands,
) {
    // `Modal::None` only exists inside `Playing` with no panel, so this one check means "show the
    // palette only while actually playing" — pausing / a panel / game-over reaps it.
    let show = mode.active && modal.map_or(false, |m| *m.get() == Modal::None);
    let shown = !existing.is_empty();
    if show && !shown {
        spawn_build_strip(&mut commands, &fonts, &icons);
    } else if !show && shown {
        for e in &existing {
            commands.entity(e).try_despawn();
        }
    }
}

/// Build the docked palette: a hint line + one row per [`BUILD_TYPES`] entry (icon + name + cost).
/// Docked bottom-centre above the quick-bar so the glowing plots stay visible behind it.
fn spawn_build_strip(commands: &mut Commands, fonts: &UiFonts, icons: &crate::ui::icons::IconAtlas) {
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
                    width: Val::Px(284.0),
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
                card.spawn((
                    Node { max_width: Val::Px(268.0), ..default() },
                    label(&fonts.regular, "Walk onto a glowing spot \u{2014} press E to raise it.", 11.0, GREY),
                    BuildHint,
                ));
                for (i, item) in BUILD_TYPES.iter().enumerate() {
                    let c = item.cost();
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
                        BorderColor::all(BORDER_SOFT),
                        BuildOption(i),
                    ))
                    .with_children(|b| {
                        if let Some(entry) = icons.get_tintable(item.icon_id()) {
                            b.spawn(widgets::icon_tinted(entry, 20.0, GOLD));
                        }
                        b.spawn(label(&fonts.semibold, item.label(), 13.0, Color::WHITE));
                        b.spawn(Node { flex_grow: 1.0, ..default() }); // cost hugs the right edge
                        for (key, amount) in [("stat:wood", c.wood), ("stat:stone", c.stone)] {
                            if amount <= 0.0 {
                                continue;
                            }
                            if let Some(entry) = icons.get_tintable(key) {
                                b.spawn(widgets::icon_tinted(entry, 11.0, Color::WHITE));
                            }
                            b.spawn(label(&fonts.semibold, format!("{}", amount as i64), 11.0, Color::WHITE));
                        }
                    });
                }
            });
        });
}

/// A palette row clicked → select that building (drives which spots glow + what E raises).
fn build_strip_input(mut mode: ResMut<BuildMode>, q: Query<(&Interaction, &BuildOption), Changed<Interaction>>) {
    for (interaction, opt) in &q {
        if *interaction == Interaction::Pressed {
            mode.sel = opt.0;
        }
    }
}

/// Each frame: highlight the selected row's border + rewrite the hint line to the selected
/// building's placement state ("walk to a glowing spot" / "press E here") or its afford shortfall.
/// (Rows are built once; only their highlight + the hint change live, so clicks aren't eaten.)
fn build_strip_update(
    mode: Res<BuildMode>,
    town: Res<TownRes>,
    bank: Res<Bank>,
    hero: Res<HeroState>,
    spots: Res<PlotSpots>,
    mut rows: Query<(&BuildOption, &mut BorderColor)>,
    mut hint: Query<&mut Text, With<BuildHint>>,
) {
    if !mode.active {
        return;
    }
    for (opt, mut bc) in &mut rows {
        *bc = BorderColor::all(if opt.0 == mode.sel { GOLD } else { BORDER_SOFT });
    }
    let kind = mode.kind();
    let on_spot = current_build_spot(kind, hero.pos, &town.0, &spots).is_some();
    let msg = match cost_shortfall(kind.cost(), &bank.0) {
        Some(short) => format!("{} \u{2014} {short}", kind.label()),
        None if on_spot => format!("Press E to raise the {} here", kind.label()),
        None => format!("{} \u{00b7} walk to a glowing spot, E to raise", kind.desc()),
    };
    if let Ok(mut text) = hint.single_mut() {
        if text.0 != msg {
            text.0 = msg;
        }
    }
}

/// In build mode, **E** raises the selected building on the plot/slot the hero stands on. Producers
/// spawn their mesh here; a House just bumps the core count (the courtyard reveals the dwelling).
/// A float always answers the press — the new beds, or exactly what's short (never a silent no-op).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn build_place(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<BuildMode>,
    siege: Option<Res<crate::siege::Siege>>,
    hero: Res<HeroState>,
    spots: Res<PlotSpots>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mats: Option<Res<VillageMats>>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    existing: Query<(Entity, &BuildingMesh)>,
) {
    if !mode.active || !keys.just_pressed(KeyCode::KeyE) {
        return;
    }
    // No raising buildings mid-assault — build mode is a daytime activity (`build_mode_keys` also
    // force-exits at dusk; this guards the in-frame race where E lands the same tick night falls).
    if siege.is_some_and(|s| s.phase != crate::siege::GamePhase::Prep) {
        return;
    }
    let Some(mats) = mats else { return };
    let kind = mode.kind();
    let Some(spot) = current_build_spot(kind, hero.pos, &town.0, &spots) else { return };
    match (kind, spot) {
        (BuildType::Producer(k), BuildSpot::Plot(idx)) => {
            if town.0.build(idx, k, &mut bank.0) {
                for (e, bm) in &existing {
                    if bm.idx == idx {
                        commands.entity(e).try_despawn(); // rebuild-on-rubble: clear the stale mesh
                    }
                }
                spawn_building(&mut commands, &mut meshes, &mats.0, idx, k, &spots);
                cues.write(crate::audio::AudioCue::UiSelect);
            } else {
                let at = spots.0.get(idx).copied().unwrap_or(hero.pos);
                push_cant_afford(&mut floats, k.cost(), &bank.0, k.label(), at);
            }
        }
        (BuildType::House, BuildSpot::House(site)) => {
            if town.0.build_house(&mut bank.0) {
                let y = crate::worldmap::ground_at_world(site.x, site.y).unwrap_or(0.0);
                cues.write(crate::audio::AudioCue::UiSelect);
                floats.0.push(FloatReq {
                    world: Vec3::new(site.x, y + 3.0, site.y),
                    text: format!("\u{1f3e0} House raised \u{2014} beds for {POP_PER_HOUSE} more"),
                    color: Color::srgb(0.55, 1.0, 0.6),
                    scale: 1.25,
                });
            } else {
                push_cant_afford(&mut floats, HOUSE_COST, &bank.0, "House", site);
            }
        }
        _ => {}
    }
}

/// Red "can't afford" float naming exactly what's short — mirrors the strip hint so a press is
/// never a silent no-op.
fn push_cant_afford(
    floats: &mut crate::combat_fx::FloatQueue,
    cost: Cost,
    bank: &tileworld_core::resource_store::ResourceState,
    label: &str,
    at: Vec2,
) {
    let why = cost_shortfall(cost, bank).unwrap_or_else(|| "need more resources".into());
    let y = crate::worldmap::ground_at_world(at.x, at.y).unwrap_or(0.0);
    floats.0.push(FloatReq {
        world: Vec3::new(at.x, y + 3.0, at.y),
        text: format!("Can't raise {label} \u{2014} {why}"),
        color: Color::srgb(1.0, 0.4, 0.35),
        scale: 1.1,
    });
}

// ── Build-mode world visuals: glow rings + the house site pad ──────────────────────────

/// A gold ring laid flat on a buildable spot, lit during build mode so "where can I build?" is
/// answered visibly. One per outer plot + one for the courtyard house slot; `sync_build_rings`
/// toggles each by the selected building. Not biome-tagged — permanent FX, just repositioned.
#[derive(Component)]
struct BuildRing(BuildRingKind);

#[derive(Clone, Copy)]
enum BuildRingKind {
    Plot(usize),
    House,
}

/// The construction-site pad marking where the NEXT house will rise inside the walls
/// (`castle::next_house_site`); visible even outside build mode. Lazy-spawned + self-healing:
/// a biome swap reaps it (`BiomeEntity`), we respawn next frame.
#[derive(Component)]
struct HouseSitePad;

/// Lazy-spawn one ring per plot + a house ring, then each frame light the rings that match the
/// selected building: every free outer plot for a producer, the next courtyard slot for a House.
/// The ring the hero stands on swells a touch so "this one" reads. Hidden entirely when build mode
/// is off.
#[allow(clippy::too_many_arguments)]
fn sync_build_rings(
    mode: Res<BuildMode>,
    modal: Option<Res<State<Modal>>>,
    town: Res<TownRes>,
    spots: Res<PlotSpots>,
    hero: Option<Res<HeroState>>,
    time: Res<Time>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rings: Query<(&BuildRing, &mut Transform, &mut Visibility)>,
    mut spawned: Local<bool>,
    mut prev_shown: Local<bool>,
) {
    // One-time lazy spawn once the plots exist — a flat gold annulus per spot, sharing one mesh +
    // material (same unlit-emissive planar-flash recipe as the combat rings, so it reads day/night).
    if !*spawned {
        if spots.0.is_empty() {
            return;
        }
        *spawned = true;
        let mesh = meshes.add(Annulus::new(2.45, 2.8).mesh().resolution(48).build());
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.86, 0.5, 0.55),
            emissive: LinearRgba::rgb(1.6, 1.1, 0.4),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        for idx in 0..spots.0.len() {
            spawn_build_ring(&mut commands, mesh.clone(), mat.clone(), BuildRingKind::Plot(idx));
        }
        spawn_build_ring(&mut commands, mesh, mat, BuildRingKind::House);
        return; // rings show from next frame
    }
    // Self-gate (build mode is off ~all the time): when not shown, hide every ring once on the
    // falling edge then idle — no per-frame work while build mode is closed / not playing.
    let show = mode.active && modal.map_or(false, |m| *m.get() == Modal::None);
    if !show {
        if *prev_shown {
            *prev_shown = false;
            for (_, _, mut vis) in &mut rings {
                *vis = Visibility::Hidden;
            }
        }
        return;
    }
    *prev_shown = true;
    let kind = mode.kind();
    let pulse = 1.0 + (time.elapsed_secs() * 2.6).sin() * 0.05;
    let hero_pos = hero.map(|h| h.pos);
    for (ring, mut tf, mut vis) in &mut rings {
        let (spot, ring_on) = match ring.0 {
            BuildRingKind::Plot(idx) => {
                let free = town.0.plots.get(idx).is_some_and(|p| p.is_buildable());
                (spots.0.get(idx).copied(), !kind.is_house() && free)
            }
            BuildRingKind::House => {
                let site = crate::castle::next_house_site(town.0.houses);
                (site, kind.is_house() && site.is_some())
            }
        };
        match (ring_on, spot) {
            (true, Some(p)) => {
                let y = crate::worldmap::ground_at_world(p.x, p.y).unwrap_or(0.0);
                let near = hero_pos.is_some_and(|hp| hp.distance(p) < BUILD_DIST.max(HOUSE_DIST));
                tf.translation = Vec3::new(p.x, y + 0.06, p.y);
                tf.scale = Vec3::splat(pulse * if near { 1.12 } else { 1.0 });
                *vis = Visibility::Visible;
            }
            _ => *vis = Visibility::Hidden,
        }
    }
}

/// Spawn one parked, hidden build ring (placed + revealed by [`sync_build_rings`]).
fn spawn_build_ring(commands: &mut Commands, mesh: Handle<Mesh>, mat: Handle<StandardMaterial>, kind: BuildRingKind) {
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        // Parked underground until the sync places it; laid flat like the combat rings.
        Transform::from_xyz(0.0, -100.0, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Visibility::Hidden,
        BuildRing(kind),
    ));
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
