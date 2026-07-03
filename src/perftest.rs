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
}

#[derive(Resource, Default)]
struct PerfClock {
    start: Option<f32>,
    last: f32,
}

impl Plugin for PerftestPlugin {
    fn build(&self, app: &mut App) {
        // FOREST_SPIKEWATCH=1: log every individual frame whose REAL delta exceeds 50ms, with the
        // wall-clock gap since the previous spike — pinpoints periodic stutter (e.g. "every ~10s")
        // that the 5s-smoothed PERF line averages away. Independent of FOREST_PERFTEST (checked
        // BEFORE the early-return below) so it also works in a real, non-auto-exiting play session.
        if std::env::var("FOREST_SPIKEWATCH").is_ok() {
            app.add_systems(Update, perf_spike_watch);
        }
        let Ok(raw) = std::env::var("FOREST_PERFTEST") else { return };
        let duration = raw.trim().parse::<f32>().unwrap_or(600.0).max(10.0);
        let speed = std::env::var("FOREST_PERFSPEED")
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
            .unwrap_or(1.0)
            .clamp(0.25, 16.0);
        app.insert_resource(PerfCfg { duration, speed })
            .init_resource::<PerfClock>()
            // FrameTime/EntityCount/SystemInformation diagnostics are all registered by `debug_stats`
            // (always present); we just read them here.
            .add_systems(Startup, perf_setup)
            .add_systems(Update, (perf_tick, perf_exit, perf_keep_hero_alive));
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

/// See `FOREST_SPIKEWATCH` above.
fn perf_spike_watch(time: Res<Time<Real>>, mut last_spike: Local<f32>) {
    let dt = time.delta_secs();
    if dt < 0.05 {
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
    let r = 78.0_f32;
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
fn perf_exit(time: Res<Time<Real>>, cfg: Res<PerfCfg>, mut exit: MessageWriter<AppExit>) {
    if time.elapsed_secs() >= cfg.duration {
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
    let meshes = world.resource::<Assets<Mesh>>().len();
    let mats = world.resource::<Assets<StandardMaterial>>().len();
    let imgs = world.resource::<Assets<Image>>().len();
    let (atlas_keys, atlas_pages) = world.get_resource::<FontAtlasSet>().map_or((0, 0), |fa| {
        (fa.len(), fa.values().map(|v| v.len()).sum::<usize>())
    });

    info!(
        "PERF t={elapsed:>4.0} fps={fps:>4.0} ms={ms:>5.1} ent={ent:>6.0} mesh={meshes:>5} mat={mats:>4} img={imgs:>5} atlas={atlas_keys}k/{atlas_pages}p rss={rss:>5.0}MB"
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
