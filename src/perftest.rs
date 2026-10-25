//! Headless **performance instrumentation** (`FOREST_PERFTEST=<seconds>`). The game's degradation
//! shows up only after ~50 min of play, which is the signature of a *leak* (unbounded growth), not
//! a steady-state cost — so this logs the quantities that would grow without bound, on a fixed
//! real-time cadence, then auto-exits. Never active without the env var; zero cost in normal play.
//!
//! Every 5 s it logs one `PERF` line — frame time, total entity count, Mesh/StandardMaterial/Image
//! asset counts, font-atlas counts (distinct font+size keys / total atlas pages — the thing the old
//! cosmic-text font-atlas leak grew), and process RSS — followed by a **per-archetype entity
//! histogram** (top archetypes by entity count, tagged with their game-specific component names) so
//! a leaking *entity type* names itself instead of just showing up as a rising total.
//!
//! Pair with `FOREST_WAVE=1 FOREST_DEFEND=1 FOREST_TOWN=1` to drive a sustained siege: `siege.rs`'s
//! `siege_clip_refill` runs under `FOREST_PERFTEST` too, holding a full horde under constant fire
//! from the auto-firing defenses, so combat churn (kills → corpses/floats/particles/audio, spawns)
//! exercises the leak-prone systems with no input needed. `FOREST_PERFSPEED=<mul>` runs the sim
//! faster (Time<Virtual> relative speed) to compress wall-clock.

use bevy::diagnostic::{
    DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
    SystemInformationDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::text::FontAtlasSet;

pub struct PerftestPlugin;

#[derive(Resource)]
struct PerfCfg {
    duration: f32,
    speed: f32,
    frozen: bool,
    army: bool,
}

#[derive(Resource, Default)]
struct PerfClock {
    start: Option<f32>,
    last: f32,
}

#[derive(Resource, Default)]
struct PerfFrames(Vec<f64>);

#[derive(Resource, Default)]
pub(crate) struct PerfArmyReady(pub bool);

impl Plugin for PerftestPlugin {
    fn build(&self, app: &mut App) {
        // FOREST_SPIKEWATCH=1: log every individual frame whose REAL delta exceeds 50ms, with the
        // wall-clock gap since the previous spike — pinpoints periodic stutter (e.g. "every ~10s")
        // that the 5s-smoothed PERF line averages away. Independent of FOREST_PERFTEST (checked
        // BEFORE the early-return below) so it also works in a real, non-auto-exiting play session.
        if std::env::var("FOREST_SPIKEWATCH").is_ok() {
            app.add_systems(Update, perf_spike_watch);
        }
        // FOREST_MESHDIAG=1: once a second, log how many meshes (and vertex MB) went through Bevy's
        // mesh upload — streaming churn shows up here as `allocate_and_free_meshes` time.
        if std::env::var("FOREST_MESHDIAG").is_ok() {
            if let Some(ra) = app.get_sub_app_mut(bevy::render::RenderApp) {
                ra.add_systems(bevy::render::Render, mesh_upload_diag.in_set(bevy::render::RenderSystems::ExtractCommands));
            }
        }
        if std::env::var("FOREST_MEMREPORT").is_ok() || std::env::var("FOREST_PERFTEST").is_ok() {
            app.init_resource::<MemReportState>().add_systems(Update, mem_report);
        }
        let Ok(raw) = std::env::var("FOREST_PERFTEST") else { return };
        let duration = raw.trim().parse::<f32>().unwrap_or(600.0).max(20.0);
        let speed = std::env::var("FOREST_PERFSPEED")
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .unwrap_or(1.0)
            .clamp(0.25, 16.0);
        let frozen = std::env::var("FOREST_PERFFREEZE").is_ok();
        let army = std::env::var("FOREST_PERF_ARMY").is_ok();
        let cpu = CpuTimes::default();
        if let Some(ra) = app.get_sub_app_mut(bevy::render::RenderApp) {
            use bevy::render::{Render, RenderSystems};
            ra.insert_resource(cpu.clone()).add_systems(
                Render,
                (
                    cpu_render_begin.in_set(RenderSystems::ExtractCommands),
                    cpu_render_end.in_set(RenderSystems::PostCleanup),
                ),
            );
        }
        app.insert_resource(PerfCfg { duration, speed, frozen, army })
            .init_resource::<PerfClock>()
            .init_resource::<PerfFrames>()
            .init_resource::<PerfArmyReady>()
            .insert_resource(cpu)
            .add_systems(First, cpu_main_begin)
            .add_systems(Last, cpu_main_end)
            // FrameTime/EntityCount/SystemInformation diagnostics are all registered by `debug_stats`
            // (always present); we just read them here.
            .add_systems(Startup, perf_setup)
            .add_systems(Update, (perf_tick, perf_sample.before(perf_exit), perf_exit, perf_keep_hero_alive, perf_state_watch));
        // Material churn: every `Assets<M>::get_mut` marks the material modified, which re-prepares
        // its bind group on the render thread and re-specializes every entity drawing it. Counted
        // per type (events/s + distinct assets) so a system touching materials every frame shows up.
        app.init_resource::<MatChurn>().add_systems(
            Last,
            (
                mat_churn::<StandardMaterial>,
                mat_churn::<crate::creature::CreatureMaterial>,
                mat_churn::<crate::realforest::foliage::FoliageMaterial>,
                mat_churn::<crate::terrain::TerrainMaterial>,
                mat_churn::<crate::realforest::impostor::ImpostorMaterial>,
                mat_churn::<crate::water::WaterMaterial>,
                mat_churn::<crate::combat_fx::HpBarMaterial>,
                mat_churn::<crate::combat_marks::MarkMaterial>,
                mat_churn::<crate::realforest::reskin::ReskinMaterial>,
                mat_churn::<crate::alpine::snowkit::SnowKitMaterial>,
                mat_churn::<crate::vista::FallsMaterial>,
                mat_churn::<crate::clouds::CloudSkyMaterial>,
                mat_churn_report,
            )
                .chain(),
        );
        // FOREST_PERFROAM=1: also drive the hero on a wide circuit so the follow-cam streams every
        // biome — exercises the position-reactive systems (groundcover/atmosphere/weather/footsteps)
        // that an idle-hero test leaves dormant. Gated to Modal::None like the rest of the sim.
        if std::env::var("FOREST_PERFROAM").is_ok() {
            app.add_systems(Update, perf_roam.run_if(in_state(crate::game_state::Modal::None)));
        }
        // FOREST_PERFPANELS=1: open and close every freeze-gate panel on a fast loop — a classic
        // over-time leak is UI a panel forgets to despawn on close. Runs ungated (it must fire while
        // a panel is open to close it again).
        if std::env::var("FOREST_PERFPANELS").is_ok() {
            app.add_systems(Update, perf_panels);
        }
        // FOREST_PERFFREEZE=1: hold a panel open so the whole world-sim freezes (gated on Modal::None)
        // while rendering keeps running — isolates render-pipeline CPU (visibility/extraction of all
        // ~10k entities) from the sim systems (AI / wind sway / separation / particles).
        if std::env::var("FOREST_PERFFREEZE").is_ok() {
            app.add_systems(Update, perf_freeze);
        }
        // FOREST_NOTREES=1: despawn every swaying tree (the scatter spawns them chunked over many
        // frames, so this runs every frame to catch them as they appear) — measures the frame-time
        // ceiling of merging trees out of the per-entity render path before building the chunk-merge.
        if std::env::var("FOREST_NOTREES").is_ok() {
            app.add_systems(Update, perf_despawn_trees);
        }
    }
}

/// Logs `AppState`/`Modal` every 5s under `FOREST_PERFTEST` — most of the sim (including
/// `animal_brain`/`ork_brain`) is gated on `Modal::None`, and if something leaves a panel open
/// with no input to close it, the whole sim silently never runs while render/PERF numbers still
/// look plausible. Caught exactly that class of bug once already (`skip_menu` not covering
/// `FOREST_PERFTEST`, so the harness sat at the main menu) — cheap enough to just leave running.
fn perf_state_watch(
    time: Res<Time<Real>>,
    app_state: Res<State<crate::game_state::AppState>>,
    modal: Option<Res<State<crate::game_state::Modal>>>,
    mut last: Local<f32>,
) {
    let now = time.elapsed_secs();
    if now - *last < 5.0 {
        return;
    }
    *last = now;
    info!(
        "STATE_DBG t={now:.1} app={:?} modal={}",
        app_state.get(),
        modal.map_or("?".to_string(), |m| format!("{:?}", m.get()))
    );
}

/// See `FOREST_SPIKEWATCH` above.
fn perf_spike_watch(time: Res<Time<Real>>, mut last_spike: Local<f32>) {
    // `FOREST_SPIKEWATCH=<ms>` sets the threshold (a bare `1` = the old 50 ms).
    static LIMIT: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    let limit = *LIMIT.get_or_init(|| {
        std::env::var("FOREST_SPIKEWATCH").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| *v >= 5.0).unwrap_or(50.0) / 1000.0
    });
    let dt = time.delta_secs();
    if dt < limit {
        return;
    }
    let now = time.elapsed_secs();
    let gap = if *last_spike > 0.0 { now - *last_spike } else { 0.0 };
    info!("SPIKE t={now:>7.2} dt={:.1}ms gap_since_last={gap:>6.2}s", dt * 1000.0);
    *last_spike = now;
}

fn perf_despawn_trees(
    mut commands: Commands,
    trees: Query<Entity, With<crate::wind::Sway>>,
    mut total: Local<u32>,
) {
    let mut n = 0u32;
    for e in &trees {
        commands.entity(e).try_despawn();
        n += 1;
    }
    if n > 0 {
        *total += n;
        info!("PERFTEST FOREST_NOTREES: despawned {n} trees (total {})", *total);
    }
}

fn perf_freeze(
    app_state: Res<State<crate::game_state::AppState>>,
    mut next: ResMut<NextState<crate::game_state::Modal>>,
) {
    if *app_state.get() == crate::game_state::AppState::Playing {
        next.set(crate::game_state::Modal::Inventory);
    }
}

/// Cycle through the panels (open → close → next) so their `OnEnter`/`OnExit` spawn/despawn pairs run
/// hundreds of times — if any leaks UI nodes on close, the entity count climbs.
fn perf_panels(
    time: Res<Time<Real>>,
    app_state: Res<State<crate::game_state::AppState>>,
    mut next: ResMut<NextState<crate::game_state::Modal>>,
    mut last: Local<f32>,
    mut step: Local<u32>,
) {
    use crate::game_state::{AppState, Modal};
    if *app_state.get() != AppState::Playing {
        return;
    }
    let t = time.elapsed_secs();
    if t - *last < 1.0 {
        return;
    }
    *last = t;
    const SEQ: [Modal; 12] = [
        Modal::UpgradeTree, Modal::None, Modal::Inventory, Modal::None, Modal::Shop, Modal::None,
        Modal::Build, Modal::None, Modal::Quest, Modal::None, Modal::Tutorial, Modal::None,
    ];
    next.set(SEQ[(*step as usize) % SEQ.len()]);
    *step = step.wrapping_add(1);
}

/// Sweep the hero around a wide circuit through all five biome regions (centres sit ~60–90 out from
/// the origin). Mirrors `demo::explore_drive`'s hero-write pattern (pos + ground-sampled y + facing
/// + moving), so locomotion/footstep/biome-detection all react as in real roaming.
fn perf_roam(time: Res<Time>, mut hero_q: Query<&mut crate::player::Hero>) {
    let Ok(mut hero) = hero_q.single_mut() else {
        return;
    };
    let t = time.elapsed_secs();
    let a = t * 0.16; // ~40 s of game time per lap (×PERFSPEED faster in wall-clock)
    let prev_a = (t - time.delta_secs()) * 0.16;
    // 78u at MAP_SCALE 2.6 (threads every biome's rim) — tracks the map scale so it still does.
    let r = 78.0_f32 * crate::worldmap::MAP_SCALE / 2.6;
    let pos = Vec2::new(r * a.cos(), r * a.sin());
    let prev = Vec2::new(r * prev_a.cos(), r * prev_a.sin());
    let dir = (pos - prev).normalize_or_zero();
    hero.pos = pos;
    hero.y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(hero.y);
    if dir != Vec2::ZERO {
        hero.facing = dir.x.atan2(dir.y);
    }
    hero.moving = true;
    hero.moving_amt = 1.0;
}

/// Keep the (input-less) hero from dying in the headless siege — otherwise he's swarmed, falls, and
/// a Game-Over freeze ends the run. The auto-firing defenses do the killing; the hero just tanks.
fn perf_keep_hero_alive(player: Option<ResMut<crate::player::PlayerRes>>) {
    if let Some(mut p) = player {
        p.0.hp = p.0.max_hp;
    }
}

/// Untraced CPU cost per frame of each thread, for A/B work where frame time can't resolve it
/// (vsync/driver caps pin frame time, and the `profiling` trace adds its own overhead): `main` is the
/// main-world schedule (`First` → `Last`), `render` the render-world `Render` schedule
/// (`ExtractCommands` → `PostCleanup`, which includes command encoding + queue submit). With
/// pipelined rendering the two overlap, so the achievable frame ≈ max(main, render + extract).
#[derive(Resource, Clone, Default)]
struct CpuTimes(std::sync::Arc<std::sync::Mutex<CpuAcc>>);

#[derive(Default)]
struct CpuAcc {
    /// Since the last PERF line (drained by `perf_tick`).
    win_main: Vec<f64>,
    win_render: Vec<f64>,
    /// Post-warmup samples for the summary (cleared until gameplay frames start counting).
    all_main: Vec<f64>,
    all_render: Vec<f64>,
}

impl CpuAcc {
    fn push_main(&mut self, ms: f64) {
        self.win_main.push(ms);
        self.all_main.push(ms);
    }
    fn push_render(&mut self, ms: f64) {
        self.win_render.push(ms);
        self.all_render.push(ms);
    }
}

#[derive(Resource)]
struct RenderStart(std::time::Instant);

fn cpu_main_begin(mut commands: Commands) {
    // Same marker type as the render side — each world holds its own copy.
    commands.insert_resource(RenderStart(std::time::Instant::now()));
}

fn cpu_main_end(start: Option<Res<RenderStart>>, times: Res<CpuTimes>) {
    if let Some(s) = start {
        times.0.lock().unwrap().push_main(s.0.elapsed().as_secs_f64() * 1000.0);
    }
}

fn cpu_render_begin(mut commands: Commands) {
    commands.insert_resource(RenderStart(std::time::Instant::now()));
}

fn cpu_render_end(start: Option<Res<RenderStart>>, times: Res<CpuTimes>) {
    if let Some(s) = start {
        times.0.lock().unwrap().push_render(s.0.elapsed().as_secs_f64() * 1000.0);
    }
}

fn mean_p95(v: &mut [f64]) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    v.sort_by(f64::total_cmp);
    (v.iter().sum::<f64>() / v.len() as f64, v[((v.len() - 1) as f64 * 0.95).ceil() as usize])
}

fn perf_setup(cfg: Res<PerfCfg>, mut vtime: ResMut<Time<Virtual>>) {
    if (cfg.speed - 1.0).abs() > f32::EPSILON {
        vtime.set_relative_speed(cfg.speed);
    }
    info!(
        "PERFTEST start: duration={}s sim_speed=x{} — one PERF line every 5s, then exits",
        cfg.duration as u32, cfg.speed
    );
}

/// Hard stop after the wall-clock budget so an unattended run always terminates.
fn perf_sample(time: Res<Time<Real>>, app: Res<State<crate::game_state::AppState>>,
    modal: Option<Res<State<crate::game_state::Modal>>>, cfg: Res<PerfCfg>, mut frames: ResMut<PerfFrames>,
    ready: Res<crate::biome::WorldReady>, army_ready: Res<PerfArmyReady>, mut ready_at: Local<Option<f32>>,
    cpu: Res<CpuTimes>) {
    // Exclude shader/asset warmup and frozen menus. Percentiles describe actual gameplay frames.
    let clear_cpu = || {
        let mut c = cpu.0.lock().unwrap();
        c.all_main.clear();
        c.all_render.clear();
    };
    if !ready.0 || (cfg.army && !army_ready.0) {
        *ready_at = None;
        clear_cpu();
        return;
    }
    let now = time.elapsed_secs();
    let start = *ready_at.get_or_insert_with(|| { info!("PERF_READY t={now:.3}"); now });
    if now - start >= 15.0 && *app.get() == crate::game_state::AppState::Playing
        && (cfg.frozen || modal.is_some_and(|m| *m.get() == crate::game_state::Modal::None)) {
        frames.0.push(time.delta_secs_f64() * 1000.0);
    } else {
        clear_cpu();
    }
}

fn perf_exit(time: Res<Time<Real>>, cfg: Res<PerfCfg>, mut frames: ResMut<PerfFrames>, mut exit: MessageWriter<AppExit>,
    app: Res<State<crate::game_state::AppState>>, modal: Option<Res<State<crate::game_state::Modal>>>, cpu: Res<CpuTimes>) {
    if time.elapsed_secs() >= cfg.duration {
        if frames.0.is_empty() || *app.get() != crate::game_state::AppState::Playing
            || (!cfg.frozen && !modal.is_some_and(|m| *m.get() == crate::game_state::Modal::None)) {
            error!("PERF_SUMMARY INVALID: no post-warmup gameplay or workload ended/froze before deadline");
            exit.write(AppExit::error());
            return;
        }
        frames.0.sort_by(f64::total_cmp);
        let n = frames.0.len();
        let mode = if cfg.frozen { "render_frozen" } else { "gameplay" };
        let percentile = |p: f64| frames.0[((n - 1) as f64 * p).ceil() as usize];
        info!("PERF_SUMMARY mode={mode} frames={n} mean_ms={:.3} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} max_ms={:.3}",
            frames.0.iter().sum::<f64>() / n as f64, percentile(0.50), percentile(0.95), percentile(0.99), frames.0[n-1]);
        {
            let mut c = cpu.0.lock().unwrap();
            let (mm, mp) = mean_p95(&mut c.all_main);
            let (rm, rp) = mean_p95(&mut c.all_render);
            info!("PERF_CPU main_mean_ms={mm:.3} main_p95_ms={mp:.3} render_mean_ms={rm:.3} render_p95_ms={rp:.3}");
        }
        info!("PERFTEST done ({:.0}s) — exiting", time.elapsed_secs());
        exit.write(AppExit::Success);
    }
}

/// Exclusive so it can walk the archetype table; gated to a 5 s cadence off `PerfClock`.
fn perf_tick(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let log_now = {
        let mut c = world.resource_mut::<PerfClock>();
        match c.start {
            None => {
                c.start = Some(now);
                c.last = now;
                false
            }
            Some(_) if now - c.last >= 5.0 => {
                c.last = now;
                true
            }
            _ => false,
        }
    };
    if !log_now {
        return;
    }
    let elapsed = now - world.resource::<PerfClock>().start.unwrap_or(now);

    // ── scalar metrics ───────────────────────────────────────────────────────────────
    let (fps, ms, ent, rss) = {
        let d = world.resource::<DiagnosticsStore>();
        let fps = d.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|x| x.smoothed()).unwrap_or(0.0);
        let ms =
            d.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|x| x.smoothed()).unwrap_or(0.0);
        let ent =
            d.get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT).and_then(|x| x.value()).unwrap_or(0.0);
        let rss = d
            .get(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE)
            .and_then(|x| x.value())
            .unwrap_or(0.0);
        (fps, ms, ent, rss)
    };
    // Draw-count shape: Mesh3d entities, how many the camera can see, how many feed the shadow
    // cascades (each cascade view re-bins every caster inside it — the render-thread cost driver).
    let (m3d, m3d_vis, casters) = {
        let mut q = world.query::<(&Mesh3d, &ViewVisibility, &InheritedVisibility, Has<bevy::light::NotShadowCaster>)>();
        let mut t = (0u32, 0u32, 0u32);
        for (_, vv, iv, nsc) in q.iter(world) {
            t.0 += 1;
            if vv.get() {
                t.1 += 1;
            }
            if iv.get() && !nsc {
                t.2 += 1;
            }
        }
        t
    };
    let cascades: Vec<String> = {
        let mut q = world.query::<&bevy::camera::visibility::CascadesVisibleEntities>();
        q.iter(world)
            .flat_map(|c| c.entities.values().map(|v| v.iter().map(|e| e.entities.len().to_string()).collect::<Vec<_>>().join("/")))
            .collect()
    };
    // Who fills the farthest cascade: tag each caster by its game components (or engine-only shape).
    {
        let far: Vec<Entity> = {
            let mut q = world.query::<&bevy::camera::visibility::CascadesVisibleEntities>();
            q.iter(world).flat_map(|c| c.entities.values().filter_map(|v| v.last().map(|l| l.entities.clone()))).flatten().collect()
        };
        let cam = world.query_filtered::<&GlobalTransform, With<Camera3d>>().iter(world).next().map(|t| t.translation()).unwrap_or_default();
        // signature → (count, Σ distance to the camera, max distance, one sample position)
        let mut hist: std::collections::HashMap<String, (u32, f32, f32, Vec3)> = Default::default();
        let comps = world.components();
        for e in far {
            let Ok(er) = world.get_entity(e) else { continue };
            let mut names: Vec<String> = er.archetype().components().iter()
                .filter_map(|id| comps.get_name(*id).map(|n| n.to_string()))
                .filter(|n| n.contains("tileworld") || n.contains("MeshMaterial3d") || n.contains("VisibilityRange"))
                .map(|n| short(&n.replace("MeshMaterial3d<", "M<")))
                .collect();
            // The range band says which tier a caster belongs to (props / static LOD / tree LOD …).
            if let Some(r) = er.get::<bevy::camera::visibility::VisibilityRange>() {
                names.push(format!("[{:.0}..{:.0}]", r.start_margin.end, r.end_margin.end));
            }
            if let Some(p) = er.get::<ChildOf>() {
                if let Ok(pr) = world.get_entity(p.parent()) {
                    let tag: Vec<String> = pr.archetype().components().iter()
                        .filter_map(|id| comps.get_name(*id).map(|n| n.to_string()))
                        .filter(|n| n.contains("tileworld"))
                        .map(|n| short(&n))
                        .collect();
                    names.push(format!("parent({})", tag.join(",")));
                }
            }
            let p = er.get::<GlobalTransform>().map(|t| t.translation()).unwrap_or_default();
            let d = p.distance(cam);
            let h = hist.entry(names.join(" ")).or_insert((0, 0.0, 0.0, p));
            h.0 += 1;
            h.1 += d;
            h.2 = h.2.max(d);
        }
        let mut rows: Vec<_> = hist.into_iter().collect();
        rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        for (sig, (n, sum, max, p)) in rows.iter().take(14) {
            info!("   farcascade {n:>5} avg {:>4.0}u max {:>4.0}u e.g.({:.0},{:.0})  {}", sum / *n as f32, max, p.x, p.z, sig.chars().take(140).collect::<String>());
        }
    }
    info!("PERF draws: mesh3d={m3d} cam_visible={m3d_vis} shadow_casters(visible-tree)={casters} cascades=[{}]", cascades.join(" | "));
    let meshes = world.resource::<Assets<Mesh>>().len();
    let mats = world.resource::<Assets<StandardMaterial>>().len();
    let creature_mats = world.resource::<Assets<crate::creature::CreatureMaterial>>().len();
    let imgs = world.resource::<Assets<Image>>().len();
    let (atlas_keys, atlas_pages) = world.get_resource::<FontAtlasSet>().map_or((0, 0), |fa| {
        (fa.len(), fa.values().map(|v| v.len()).sum::<usize>())
    });

    let (cpu_main, cpu_render) = world.get_resource::<CpuTimes>().map_or((0.0, 0.0), |c| {
        let mut c = c.0.lock().unwrap();
        let m = mean_p95(&mut c.win_main).0;
        let r = mean_p95(&mut c.win_render).0;
        c.win_main.clear();
        c.win_render.clear();
        (m, r)
    });
    info!(
        "PERF t={elapsed:>4.0} fps={fps:>4.0} ms={ms:>5.1} cpu_main={cpu_main:>5.2} cpu_render={cpu_render:>5.2} ent={ent:>6.0} mesh={meshes:>5} mat={mats:>4} creature_mat={creature_mats} img={imgs:>5} atlas={atlas_keys}k/{atlas_pages}p rss={rss:>5.2}GiB"
    );

    // ── per-archetype histogram (top by entity count, tagged with game components) ──────
    let comps = world.components();
    let mut rows: Vec<(u32, String)> = world
        .archetypes()
        .iter()
        .filter(|a| a.len() >= 8) // a real leak grows large; skip the long tail of singletons
        .map(|a| {
            let game: Vec<String> = a
                .components()
                .iter()
                .filter_map(|id| comps.get_name(*id).map(|n| n.to_string()))
                .filter(|n| n.contains("tileworld")) // our crate's markers identify the entity type
                .map(|n| short(&n))
                .collect();
            let sig = if game.is_empty() {
                // No game marker (pure-engine entity, e.g. a mesh instance) — fall back to a hint.
                format!("[engine x{} comps]", a.components().len())
            } else {
                game.join(" ")
            };
            (a.len(), sig)
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for (n, sig) in rows.iter().take(12) {
        info!("   arch {n:>6}  {}", sig.chars().take(180).collect::<String>());
    }

    // ── GPU pass breakdown (where the frame time actually goes) ─────────────────────────
    // Reads RenderDiagnosticsPlugin's per-pass `render/<pass>/elapsed_gpu` (needs TIMESTAMP_QUERY;
    // falls back to nothing on backends without it). If Σ passes ≈ frame_ms → GPU-bound (cut/cheapen
    // passes); if Σ ≪ frame_ms → CPU-bound (systems/extraction/entity count).
    let mut passes: Vec<(String, f64)> = {
        let d = world.resource::<DiagnosticsStore>();
        d.iter()
            .filter_map(|diag| {
                let p = diag.path().as_str();
                let name = p.strip_prefix("render/")?.strip_suffix("/elapsed_gpu")?;
                if diag.measurement().is_none_or(|m| m.time.elapsed().as_millis() > 500) {
                    return None; // skip stale passes (a node that only ran during warmup)
                }
                let ms = diag.smoothed().filter(|m| *m > 0.0)?;
                Some((name.trim_end_matches('/').replace('/', "·"), ms))
            })
            .collect()
    };
    passes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let gpu_total: f64 = passes.iter().map(|p| p.1).sum();
    let top = passes
        .iter()
        .take(12)
        .map(|(n, ms)| format!("{n}={ms:.2}"))
        .collect::<Vec<_>>()
        .join(" ");
    info!("  GPU Σ={gpu_total:.2}ms  {top}");
}

/// `tileworld_bevy_forest::combat_fx::FloatText` -> `FloatText` (last path segment, sans generics).
fn short(full: &str) -> String {
    let base = full.rsplit("::").next().unwrap_or(full);
    base.split('<').next().unwrap_or(base).trim().to_string()
}

/// `FOREST_MEMREPORT=1` (or any `FOREST_PERFTEST` run): ONE detailed memory breakdown, ~12 s after
/// the world is up — where the resident CPU-side asset bytes live. Meshes/images whose data was
/// handed to the GPU (`RENDER_WORLD`-only) report as "extracted" (0 CPU bytes); everything else is
/// a CPU copy kept alive in `Assets<T>`. Retained mesh bytes are attributed to the game components
/// of the first entity that renders them, so the owning system names itself.
pub(crate) fn mem_report(world: &mut World) {
    use std::collections::HashMap;
    let ready = world.resource::<crate::biome::WorldReady>().0;
    let now = world.resource::<Time<Real>>().elapsed_secs();
    let state = {
        let mut s = world.resource_mut::<MemReportState>();
        if !ready {
            s.ready_at = None;
            return;
        }
        let at = *s.ready_at.get_or_insert(now);
        if s.done || now - at < 12.0 {
            return;
        }
        s.done = true;
        at
    };
    let _ = state;
    // mesh id → (entity count, first entity)
    let mut users: HashMap<AssetId<Mesh>, (u32, Entity)> = HashMap::new();
    let mut q = world.query::<(Entity, &Mesh3d)>();
    for (e, m) in q.iter(world) {
        let u = users.entry(m.0.id()).or_insert((0, e));
        u.0 += 1;
    }
    let mut mat_users: HashMap<AssetId<StandardMaterial>, u32> = HashMap::new();
    let mut mq = world.query::<&MeshMaterial3d<StandardMaterial>>();
    let mut std_ents = 0u32;
    for m in mq.iter(world) {
        *mat_users.entry(m.0.id()).or_default() += 1;
        std_ents += 1;
    }
    let sig_of = |world: &World, e: Entity| -> String {
        let Ok(er) = world.get_entity(e) else { return "?".into() };
        let comps = world.components();
        let mut v: Vec<String> = er
            .archetype()
            .components()
            .iter()
            .filter_map(|id| comps.get_name(*id).map(|n| n.to_string()))
            .filter(|n| n.contains("tileworld"))
            .map(|n| short(&n))
            .collect();
        // Fall back to the parent's markers (glTF/kit children carry none of their own).
        if v.is_empty() {
            if let Some(p) = er.get::<ChildOf>() {
                if let Ok(pr) = world.get_entity(p.parent()) {
                    v = pr
                        .archetype()
                        .components()
                        .iter()
                        .filter_map(|id| comps.get_name(*id).map(|n| n.to_string()))
                        .filter(|n| n.contains("tileworld"))
                        .map(|n| format!("^{}", short(&n)))
                        .collect();
                }
            }
        }
        if v.is_empty() { "[engine]".into() } else { v.join(" ") }
    };
    let meshes = world.resource::<Assets<Mesh>>();
    let (mut n_ret, mut n_ext, mut b_ret, mut v_ret) = (0usize, 0usize, 0usize, 0usize);
    let mut rows: Vec<(usize, usize, AssetId<Mesh>)> = Vec::new();
    let mut by_owner: HashMap<String, (usize, usize, usize)> = HashMap::new(); // bytes, meshes, verts
    let mut orphan = (0usize, 0usize);
    for (id, m) in meshes.iter() {
        let Ok(attrs) = m.try_attributes() else {
            n_ext += 1;
            continue;
        };
        let mut bytes = 0usize;
        let mut verts = 0usize;
        for (a, vals) in attrs {
            bytes += vals.len() * a.format.size() as usize;
            verts = verts.max(vals.len());
        }
        if let Ok(Some(ix)) = m.try_indices_option() {
            bytes += match ix {
                bevy::mesh::Indices::U16(v) => v.len() * 2,
                bevy::mesh::Indices::U32(v) => v.len() * 4,
            };
        }
        n_ret += 1;
        b_ret += bytes;
        v_ret += verts;
        rows.push((bytes, verts, id));
        match users.get(&id) {
            Some(&(_, e)) => {
                let o = by_owner.entry(sig_of(world, e)).or_default();
                o.0 += bytes;
                o.1 += 1;
                o.2 += verts;
            }
            None => {
                orphan.0 += bytes;
                orphan.1 += 1;
            }
        }
    }
    let mib = |b: usize| b as f64 / (1024.0 * 1024.0);
    info!(
        "MEM meshes: total={} cpu_retained={} ({:.1} MiB, {} verts) extracted(gpu-only)={} orphan_retained={} ({:.1} MiB) entities_with_mesh={}",
        n_ret + n_ext, n_ret, mib(b_ret), v_ret, n_ext, orphan.1, mib(orphan.0), users.values().map(|u| u.0).sum::<u32>()
    );
    let mut owners: Vec<_> = by_owner.into_iter().collect();
    owners.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (sig, (b, n, v)) in owners.iter().take(20) {
        info!("MEM   mesh-owner {:>8.1} MiB  {n:>5} meshes {v:>8} verts  {}", mib(*b), sig.chars().take(140).collect::<String>());
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for (b, v, id) in rows.iter().take(12) {
        let (n, owner) = users.get(id).map_or((0, "<no entity>".to_string()), |&(n, e)| (n, sig_of(world, e)));
        info!("MEM   mesh {:>7.2} MiB {v:>7} verts x{n:<4} {}", mib(*b), owner.chars().take(120).collect::<String>());
    }
    let images = world.resource::<Assets<Image>>();
    let (mut ni, mut ni_ret, mut bi) = (0usize, 0usize, 0usize);
    let mut irows: Vec<(usize, String)> = Vec::new();
    for (_id, im) in images.iter() {
        ni += 1;
        let Some(d) = im.data.as_ref() else { continue };
        if d.is_empty() {
            continue;
        }
        ni_ret += 1;
        bi += d.len();
        let td = &im.texture_descriptor;
        irows.push((
            d.len(),
            format!(
                "{}x{}x{} {:?} mips={} usage={:?}",
                td.size.width, td.size.height, td.size.depth_or_array_layers, td.format, td.mip_level_count, im.asset_usage
            ),
        ));
    }
    info!("MEM images: total={ni} cpu_retained={ni_ret} ({:.1} MiB)", mib(bi));
    irows.sort_by(|a, b| b.0.cmp(&a.0));
    for (b, d) in irows.iter().take(15) {
        info!("MEM   image {:>7.2} MiB {d}", mib(*b));
    }
    let n_mats = world.resource::<Assets<StandardMaterial>>().len();
    info!(
        "MEM std_materials: assets={n_mats} used_distinct={} entities={std_ents} (singletons={})",
        mat_users.len(),
        mat_users.values().filter(|n| **n == 1).count()
    );
}

#[derive(Resource, Default)]
pub(crate) struct MemReportState {
    ready_at: Option<f32>,
    done: bool,
}

/// `FOREST_MESHDIAG`: per-second totals of the meshes extracted (added/modified) and removed.
fn mesh_upload_diag(
    extracted: Res<bevy::render::render_asset::ExtractedAssets<bevy::render::mesh::RenderMesh>>,
    mut acc: Local<Option<(std::time::Instant, u32, usize, usize, usize, usize)>>,
) {
    let (t0, frames, n, verts, bytes, removed) = acc.get_or_insert((std::time::Instant::now(), 0, 0, 0, 0, 0));
    *frames += 1;
    *n += extracted.extracted.len();
    *removed += extracted.removed.len();
    for (_, m) in &extracted.extracted {
        *verts += m.count_vertices();
        *bytes += m.get_vertex_buffer_size();
    }
    if t0.elapsed().as_secs_f32() >= 1.0 {
        info!(
            "MESHDIAG frames={frames} uploaded={n} verts={:.0}k vbuf={:.1}MB removed={removed}",
            *verts as f32 / 1000.0,
            *bytes as f32 / 1.0e6
        );
        *acc = None;
    }
}

/// Per material type: (modified events, distinct assets modified) since the last report.
#[derive(Resource, Default)]
struct MatChurn {
    by_type: std::collections::BTreeMap<&'static str, (u32, bevy::platform::collections::HashSet<bevy::asset::UntypedAssetId>)>,
    last: f32,
}

fn mat_churn<M: Asset>(mut events: MessageReader<AssetEvent<M>>, mut churn: ResMut<MatChurn>) {
    let name = std::any::type_name::<M>().rsplit("::").next().unwrap_or("?");
    for ev in events.read() {
        if let AssetEvent::Modified { id } = ev {
            let e = churn.by_type.entry(name).or_default();
            e.0 += 1;
            e.1.insert(id.untyped());
        }
    }
}

fn mat_churn_report(
    time: Res<Time<Real>>,
    mut churn: ResMut<MatChurn>,
    std_mats: Res<Assets<StandardMaterial>>,
    users: Query<(&MeshMaterial3d<StandardMaterial>, Option<&Name>, Option<&ChildOf>)>,
    names: Query<&Name>,
) {
    let now = time.elapsed_secs();
    let dt = now - churn.last;
    if dt < 5.0 {
        return;
    }
    churn.last = now;
    // Name the churned StandardMaterials: how many meshes draw each, its colours, and a user's name.
    if let Some((_, ids)) = churn.by_type.get("StandardMaterial") {
        for id in ids.iter().take(12) {
            let Ok(tid) = id.try_typed::<StandardMaterial>() else { continue };
            let mut n = 0;
            let mut who = String::new();
            for (m, nm, parent) in &users {
                if m.0.id() == tid {
                    n += 1;
                    if who.is_empty() {
                        who = nm.map(|x| x.to_string()).or_else(|| parent.and_then(|p| names.get(p.parent()).ok()).map(|x| format!("child of {x}"))).unwrap_or_default();
                    }
                }
            }
            let desc = std_mats.get(tid).map(|m| format!("base {:?} emissive {:?}", m.base_color.to_srgba(), m.emissive)).unwrap_or_default();
            info!("MATCHURN   std {id:?}: {n} users e.g. '{who}' {desc}");
        }
    }
    let line: Vec<String> = churn
        .by_type
        .iter()
        .filter(|(_, (n, _))| *n > 0)
        .map(|(t, (n, ids))| format!("{t} {:.0}/s over {} assets", *n as f32 / dt, ids.len()))
        .collect();
    info!("MATCHURN {}", if line.is_empty() { "none".into() } else { line.join(" | ") });
    churn.by_type.clear();
}
