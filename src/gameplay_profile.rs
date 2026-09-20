//! Opt-in gameplay capture: FOREST_PROFILE=<new directory>, FOREST_PROFILE_SECONDS=1800.
//! JSONL is written by a bounded background writer, never by the render/game thread.
//! Scope timings are inclusive wall times (nested/parallel spans must not be summed).
use std::{
    fs::{self, OpenOptions}, io::{BufWriter, Write}, path::PathBuf,
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc::{self, SyncSender}, Arc},
    thread::JoinHandle, time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use bevy::{diagnostic::DiagnosticsStore, prelude::*, render::renderer::{RenderAdapterInfo, RenderDevice}, window::PrimaryWindow};
use serde::Serialize;
use serde_json::{json, Value};

static ACTIVE: AtomicBool = AtomicBool::new(false);
const METRIC_COUNT: usize = 11;
static STATS: [Counters; METRIC_COUNT] = [const { Counters::new() }; METRIC_COUNT];
struct Counters { calls: AtomicU64, ns: AtomicU64, max_ns: AtomicU64 }
impl Counters {
    const fn new() -> Self { Self { calls: AtomicU64::new(0), ns: AtomicU64::new(0), max_ns: AtomicU64::new(0) } }
}
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum Metric { Pathfinding, QuadrupedAnimation, BipedAnimation, AnimalBrain, OrkBrain, WorkerSteer, GuardCombat, InvaderBrain, DirectorMarch, Wind, GroundSampling }
const NAMES: [&str; METRIC_COUNT] = ["pathfinding", "quadruped_animation", "biped_animation", "animal_brain", "ork_brain", "worker_steer", "guard_combat", "invader_brain", "director_march", "wind", "ground_sampling"];
pub struct Scope { start: Option<Instant>, metric: Metric }
#[inline]
pub fn scope(metric: Metric) -> Scope {
    Scope { start: ACTIVE.load(Ordering::Relaxed).then(Instant::now), metric }
}
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            let s = &STATS[self.metric as usize];
            s.ns.fetch_add(ns, Ordering::Relaxed);
            s.max_ns.fetch_max(ns, Ordering::Relaxed);
            s.calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Serialize)]
struct FrameSample {
    #[serde(rename = "type")] kind: &'static str,
    frame: u64, t_s: f64, frame_ms: f64, app_ms: f64, recorder_ms: f64,
    app_state: String, modal: Option<String>, focused: bool, world_ready: bool,
    phase: Option<String>, hero: Option<[f32; 2]>,
}
enum Packet { Frame(FrameSample), Json(Value) }
#[derive(Resource)]
struct Capture {
    tx: Option<SyncSender<Packet>>, worker: Option<JoinHandle<()>>,
    dropped: Arc<AtomicU64>, failed: Arc<AtomicBool>,
    start: Instant, frame_start: Instant, frame: u64, pending: Option<FrameSample>,
    last_sample: Instant, duration: Duration, stopped: bool,
}
impl Capture {
    fn send(&self, packet: Packet) {
        if let Some(tx) = &self.tx {
            if tx.try_send(packet).is_err() { self.dropped.fetch_add(1, Ordering::Relaxed); }
        }
    }
    fn stop(&mut self, reason: &str) {
        if self.stopped { return; }
        self.send(Packet::Json(json!({"type":"stop", "reason":reason, "t_s":self.start.elapsed().as_secs_f64()})));
        self.tx.take();
        self.stopped = true;
        ACTIVE.store(false, Ordering::Relaxed);
        info!("Gameplay capture stopped ({reason}); game continues normally");
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop("game_exit");
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

pub struct GameplayProfilePlugin;
impl Plugin for GameplayProfilePlugin {
    fn build(&self, app: &mut App) {
        let Ok(dir) = std::env::var("FOREST_PROFILE") else { return };
        let duration = std::env::var("FOREST_PROFILE_SECONDS").ok()
            .and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite())
            .unwrap_or(1800.0).clamp(1.0, 7200.0);
        match open_capture(PathBuf::from(dir), duration) {
            Ok(capture) => {
                for stats in &STATS {
                    stats.calls.store(0, Ordering::Relaxed);
                    stats.ns.store(0, Ordering::Relaxed);
                    stats.max_ns.store(0, Ordering::Relaxed);
                }
                ACTIVE.store(true, Ordering::Relaxed);
                app.insert_resource(capture)
                    .add_systems(Startup, metadata)
                    .add_systems(First, begin_frame)
                    .add_systems(Last, end_frame);
            }
            Err(e) => error!("Could not start gameplay capture: {e}"),
        }
    }
}

fn open_capture(dir: PathBuf, duration: f64) -> std::io::Result<Capture> {
    fs::create_dir_all(&dir)?;
    let path = dir.join("gameplay.jsonl");
    // Never overwrite an earlier recording, even if a launch path was accidentally reused.
    let file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    let (tx, rx) = mpsc::sync_channel::<Packet>(4096);
    let dropped = Arc::new(AtomicU64::new(0));
    let failed = Arc::new(AtomicBool::new(false));
    let dropped_writer = dropped.clone();
    let failed_writer = failed.clone();
    let worker = std::thread::Builder::new().name("gameplay-profile-writer".into()).spawn(move || {
        let result = (|| -> std::io::Result<()> {
            let mut out = BufWriter::with_capacity(64 * 1024, file);
            let mut last_flush = Instant::now();
            let mut bytes = 0_u64;
            loop {
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(packet) => {
                        let encoded = match packet {
                            Packet::Frame(frame) => serde_json::to_vec(&frame),
                            Packet::Json(value) => serde_json::to_vec(&value),
                        }.map_err(std::io::Error::other)?;
                        bytes += encoded.len() as u64 + 1;
                        out.write_all(&encoded)?;
                        out.write_all(b"\n")?;
                        if bytes >= 128 * 1024 * 1024 {
                            writeln!(out, "{}", json!({"type":"end","reason":"size_limit","dropped_records":dropped_writer.load(Ordering::Relaxed)}))?;
                            out.flush()?;
                            return Ok(());
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        writeln!(out, "{}", json!({"type":"end","reason":"channel_closed","dropped_records":dropped_writer.load(Ordering::Relaxed)}))?;
                        out.flush()?;
                        return Ok(());
                    }
                }
                if last_flush.elapsed() >= Duration::from_secs(1) { out.flush()?; last_flush = Instant::now(); }
            }
        })();
        if let Err(e) = result { eprintln!("Gameplay profile writer failed: {e}"); }
        failed_writer.store(true, Ordering::Relaxed);
    })?;
    let now = Instant::now();
    info!("Recording gameplay to {} ({}s limit; F8 marks a slow moment)", path.display(), duration);
    Ok(Capture { tx: Some(tx), worker: Some(worker), dropped, failed,
        start: now, frame_start: now, frame: 0, pending: None, last_sample: now,
        duration: Duration::from_secs_f64(duration), stopped: false })
}

fn metadata(world: &mut World) {
    let capture = world.resource::<Capture>();
    let adapter = world.get_resource::<RenderAdapterInfo>().map(|a| format!("{:?}", a.0));
    let features = world.get_resource::<RenderDevice>().map(|d| format!("{:?}", d.features()));
    let env: std::collections::BTreeMap<_, _> = std::env::vars()
        .filter(|(key, _)| key.starts_with("FOREST_") && key != "FOREST_RC" && key != "FOREST_PROFILE")
        .collect();
    capture.send(Packet::Json(json!({"type":"session","schema":1,"version":env!("CARGO_PKG_VERSION"),
        "platform":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "unix_start_s":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64() - capture.start.elapsed().as_secs_f64(),
        "duration_limit_s":capture.duration.as_secs_f64(),"adapter":adapter,"device_features":features,
        "environment":env,"cpu_scope_semantics":"inclusive wall time, potentially nested/parallel; do not sum",
        "entity_count_semantics":"live_archetype_rows",
        "frame_semantics":"First-to-next-First interval paired with previous frame state; app_ms is First-to-Last and excludes render sub-app/presentation",
        "render_semantics":"latest asynchronous diagnostic sample, freshness in age_ms; not aligned to frame; do not sum nested passes"})));
}

fn begin_frame(mut capture: ResMut<Capture>) {
    if capture.stopped { return; }
    let now = Instant::now();
    if let Some(mut previous) = capture.pending.take() {
        previous.frame_ms = now.duration_since(capture.frame_start).as_secs_f64() * 1000.0;
        capture.send(Packet::Frame(previous));
    }
    if capture.failed.load(Ordering::Relaxed) { capture.stop("writer_finished_or_failed"); return; }
    if now.duration_since(capture.start) >= capture.duration { capture.stop("duration_limit"); return; }
    capture.frame_start = now;
    capture.frame += 1;
}

fn end_frame(world: &mut World) {
    if world.resource::<Capture>().stopped { return; }
    let overhead_start = Instant::now();
    let capture = world.resource::<Capture>();
    let t_s = capture.start.elapsed().as_secs_f64();
    let app_ms = capture.frame_start.elapsed().as_secs_f64() * 1000.0;
    let frame = capture.frame;
    let do_sample = capture.last_sample.elapsed() >= Duration::from_secs(1);
    let app_state = world.get_resource::<State<crate::game_state::AppState>>()
        .map(|s| format!("{:?}", s.get())).unwrap_or_default();
    let modal = world.get_resource::<State<crate::game_state::Modal>>().map(|s| format!("{:?}", s.get()));
    let world_ready = world.get_resource::<crate::biome::WorldReady>().is_some_and(|r| r.0);
    let phase = world.get_resource::<crate::siege::Siege>().map(|s| format!("{:?}", s.phase));
    let window = world.query_filtered::<&Window, With<PrimaryWindow>>().iter(world).next();
    let focused = window.is_some_and(|w| w.focused);
    let window_info = window.map(|w| json!({"width":w.physical_width(),"height":w.physical_height(),"scale_factor":w.scale_factor(),"present_mode":format!("{:?}",w.present_mode)}));
    let hero = world.query::<&crate::player::Hero>().iter(world).next().map(|h| [h.pos.x,h.pos.y]);
    let marker = world.get_resource::<ButtonInput<KeyCode>>().is_some_and(|k| k.just_pressed(KeyCode::F8));
    if marker {
        world.resource::<Capture>().send(Packet::Json(json!({"type":"marker","t_s":t_s,"frame":frame,"label":"player marked slow moment"})));
    }
    if do_sample {
        let render: Vec<_> = world.get_resource::<DiagnosticsStore>().map(|d| d.iter().filter_map(|d| {
            let path = d.path().as_str();
            if !path.starts_with("render/") { return None; }
            let measurement = d.measurement()?;
            let age_ms = measurement.time.elapsed().as_secs_f64() * 1000.0;
            if age_ms > 500.0 { return None; }
            Some(json!({"path":path,"value":measurement.value,"age_ms":age_ms}))
        }).collect()).unwrap_or_default();
        let cpu: Vec<_> = STATS.iter().zip(NAMES).map(|(s,name)| json!({"name":name,
            "calls":s.calls.load(Ordering::Relaxed),"total_ms":s.ns.load(Ordering::Relaxed) as f64 / 1e6,
            "max_ms":s.max_ns.load(Ordering::Relaxed) as f64 / 1e6})).collect();
        let animals = world.query_filtered::<Entity, With<crate::wildlife::Animal>>().iter(world).count();
        let invaders = world.query_filtered::<Entity, With<crate::orks::WaveInvader>>().iter(world).count();
        let villagers = world.query_filtered::<Entity, With<crate::villagers::Villager>>().iter(world).count();
        let trees = world.query_filtered::<Entity, With<crate::wind::Sway>>().iter(world).count();
        let sample = json!({"type":"sample","t_s":t_s,"frame":frame,
            "app_state":app_state,"modal":modal,"focused":focused,"world_ready":world_ready,"phase":phase,
            "hero":hero,"quality":world.get_resource::<crate::quality::GraphicsQuality>(),
            "settings":world.get_resource::<crate::quality::GraphicsSettings>(),"window":window_info,
            "counts":{"entities":live_entities(world),"allocated_entity_indices":world.entities().len(),"animals":animals,"invaders":invaders,"villagers":villagers,"trees":trees,
                "meshes":world.get_resource::<Assets<Mesh>>().map(|a| a.len()),
                "materials":world.get_resource::<Assets<StandardMaterial>>().map(|a| a.len()),
                "images":world.get_resource::<Assets<Image>>().map(|a| a.len())},
            "cpu":cpu,"render":render});
        let mut capture = world.resource_mut::<Capture>();
        capture.send(Packet::Json(sample));
        capture.last_sample = Instant::now();
    }
    let recorder_ms = overhead_start.elapsed().as_secs_f64() * 1000.0;
    world.resource_mut::<Capture>().pending = Some(FrameSample { kind:"frame",frame,t_s,
        frame_ms:0.0,app_ms,recorder_ms,app_state,modal,focused,world_ready,phase,hero });
}

fn live_entities(world: &World) -> u64 {
    world.archetypes().iter().map(|a| u64::from(a.len())).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("warbell-profile-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()))
    }
    #[test]
    fn entity_count_tracks_spawn_and_despawn_not_allocator_capacity() {
        let mut world = World::new();
        let baseline = live_entities(&world);
        let entities: Vec<_> = (0..100).map(|_| world.spawn_empty().id()).collect();
        assert_eq!(live_entities(&world), baseline + 100);
        for entity in entities { world.despawn(entity); }
        assert_eq!(live_entities(&world), baseline);
        assert!(u64::from(world.entities().len()) > baseline, "allocator retains index slots after despawn");
    }
    #[test]
    fn writer_flushes_on_exit_and_never_overwrites_capture() {
        let dir = temp_dir();
        let mut capture = open_capture(dir.clone(), 1.0).unwrap();
        capture.send(Packet::Json(json!({"type":"session","schema":1})));
        capture.send(Packet::Json(json!({"type":"marker","label":"test"})));
        capture.stop("test_done");
        drop(capture);
        let path = dir.join("gameplay.jsonl");
        let original = fs::read_to_string(&path).unwrap();
        let rows: Vec<Value> = original.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(rows[0]["type"], "session");
        assert_eq!(rows[1]["label"], "test");
        assert_eq!(rows.last().unwrap()["type"], "end");
        assert_eq!(rows.last().unwrap()["dropped_records"], 0);
        assert!(open_capture(dir.clone(), 1.0).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn full_queue_drops_without_blocking_game_thread() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let now = Instant::now();
        let mut capture = Capture { tx:Some(tx),worker:None,dropped:Arc::new(AtomicU64::new(0)),failed:Arc::new(AtomicBool::new(false)),start:now,frame_start:now,frame:0,pending:None,last_sample:now,duration:Duration::from_secs(1),stopped:false };
        capture.send(Packet::Json(json!({"n":1})));
        capture.send(Packet::Json(json!({"n":2})));
        assert_eq!(capture.dropped.load(Ordering::Relaxed),1);
        capture.stop("test_done");
        assert!(capture.tx.is_none());
    }
    #[test]
    fn headless_frame_capture_keeps_state_and_missing_gpu_data_explicit() {
        let dir = temp_dir();
        let mut capture = open_capture(dir.clone(), 10.0).unwrap();
        capture.last_sample = Instant::now() - Duration::from_secs(2);
        let mut app = App::new();
        app.insert_resource(capture)
            .insert_resource(State::new(crate::game_state::AppState::Playing))
            .insert_resource(State::new(crate::game_state::Modal::None))
            .insert_resource(crate::biome::WorldReady(true))
            .add_systems(Startup, metadata)
            .add_systems(First, begin_frame)
            .add_systems(Last, end_frame);
        app.update();
        app.update();
        drop(app);
        let rows: Vec<Value> = fs::read_to_string(dir.join("gameplay.jsonl")).unwrap()
            .lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let frame = rows.iter().find(|r| r["type"] == "frame").unwrap();
        assert_eq!(frame["app_state"], "Playing");
        assert_eq!(frame["world_ready"], true);
        assert_eq!(frame["focused"], false, "no window cannot count as focused play");
        assert!(frame["frame_ms"].as_f64().unwrap() > 0.0);
        let sample = rows.iter().find(|r| r["type"] == "sample").unwrap();
        assert_eq!(sample["render"], json!([]));
        fs::remove_dir_all(dir).unwrap();
    }
}
