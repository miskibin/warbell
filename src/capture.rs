//! Capture harness — the Bevy window can't be grabbed by external tools, so we render
//! to disk ourselves. Two modes, both read once at startup:
//!
//! - `FOREST_SHOT=<path.png>` — single screenshot: render ~90 frames (so lighting / IBL /
//!   prepasses settle), grab the window, save, exit.
//! - `FOREST_CLIP=<dir>` — frame-sequence capture for GIFs / video: after a short warm-up,
//!   save every frame as `<dir>/frame_00001.png …` for `FOREST_CLIP_FRAMES` frames, then exit.
//!   A clamped fixed timestep keeps the world motion smooth despite the per-frame PNG-encode
//!   stall, and an optional slow camera orbit (`FOREST_CLIP_ORBIT`) circles a point of
//!   interest. ffmpeg then stitches the sequence into a clip at `FOREST_CLIP_FPS`.
//!
//! Clip knobs (all optional, env, read at startup):
//! | `FOREST_CLIP_FRAMES` | saved frames (default 150) |
//! | `FOREST_CLIP_FPS`    | playback fps → fixed timestep + ffmpeg rate (default 30) |
//! | `FOREST_CLIP_WARMUP` | warm-up frames before the first save (default 30) |
//! | `FOREST_CLIP_ORBIT`  | `"cx,cy,cz,radius,height,deg_per_sec"` slow camera orbit around a point |
//! | `FOREST_CLIP_AUDIO`  | `0` = no `<dir>/audio.wav` (default: the game's own audio, frame-locked — `audio_capture.rs`) |

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::time::Duration;

pub struct CapturePlugin;

#[derive(Resource)]
pub(crate) struct ShotPath(pub(crate) String);

/// Set `true` by a follow-up shot sequence (e.g. `player::boom`'s `FOREST_BOOMSHOTS`) to keep the
/// app alive after the main `FOREST_SHOT` lands; `drive_shot` exits once it drops back to `false`.
#[derive(Resource, Default)]
pub(crate) struct ShotHold(pub(crate) bool);

#[derive(Resource, Default)]
struct ShotClock {
    frame: u32,
    shot: bool,
    /// Frame the screenshot was requested on — the readback safety cap counts from HERE, not from
    /// boot, or a long `FOREST_SHOT_WARMUP` (request at frame ≫ cap) exits the app the same frame
    /// as the request and the async readback lands on a despawned window ("Unknown window", no PNG).
    shot_at: u32,
    /// `FOREST_SHOT_STORIES=1`: after the main shot, one extra shot per snow story site (cameras
    /// from `stories_cold::shot_cams`), each saved next to the main PNG as `<name>.png`.
    seq: Option<Vec<(String, Transform)>>,
    seq_idx: usize,
    seq_frame: u32,
    seq_shot: bool,
    /// Consecutive frames [`crate::pipeline_sync::settled`] has held (the shot wants a few in a row:
    /// a pipeline queued this frame only shows up in the waiting count next frame).
    settled_frames: u32,
}

/// Longest the main shot waits for the render pipelines AFTER the frame / wall-clock floor (a cold
/// driver shader cache needs ~30 s here). A pipeline that never compiles can't hang the harness.
const PIPELINE_WAIT_SECS: f32 = 90.0;

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("FOREST_SHOT") {
            app.insert_resource(ShotPath(path))
                .init_resource::<ShotClock>()
                .init_resource::<ShotHold>()
                .add_systems(Update, drive_shot);
        } else if let Ok(dir) = std::env::var("FOREST_CLIP") {
            let cfg = clip_cfg(dir);
            // `FOREST_CLIP_FIXED=1`: every tick advances the sim by exactly one playback frame
            // (the default only caps the step, so quick ticks between PNG saves record unevenly
            // spaced frames) — for frame-exact animation reviews.
            // The clip audio capture (`audio_capture.rs`, on unless `FOREST_CLIP_AUDIO=0`) needs this
            // too: it renders exactly 1/fps of audio per saved frame, so the sim must advance exactly that.
            // So does a filmed cutscene (`FOREST_SCENE`), even silent: its soundtrack is mixed offline on
            // the scene clock, and a merely capped step ran that clock ~20% slow (a 144 s film drifted 28 s).
            if std::env::var("FOREST_CLIP_FIXED").is_ok()
                || std::env::var("FOREST_CLIP_AUDIO").as_deref() != Ok("0")
                || std::env::var("FOREST_SCENE").is_ok()
            {
                app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                    1.0 / cfg.fps as f32,
                )));
            }
            app.insert_resource(cfg)
                .init_resource::<ClipClock>()
                .init_resource::<ClipProgress>()
                .add_systems(Startup, clip_setup)
                .add_systems(Update, (clip_orbit, drive_clip).chain());
        }
        // FOREST_NOHUD=1: hide every UI node each frame (HUD, prompts, quick-bar) so a
        // staged shot shows only the world — for marketing/store captures.
        if std::env::var("FOREST_NOHUD").is_ok() || crate::controls::walk_mode() {
            app.add_systems(Update, hide_hud);
        }
    }
}

fn hide_hud(
    mut nodes: Query<&mut Visibility, With<Node>>,
    menu: Option<Res<crate::ui::graphics_menu::GraphicsMenuOpen>>,
) {
    // The forest walk's Settings panel (key O) must stay usable: pause the hiding while it's up
    // (the HUD nodes stay hidden from before; only the freshly spawned panel shows).
    if menu.is_some_and(|m| m.0) {
        return;
    }
    for mut vis in &mut nodes {
        *vis = Visibility::Hidden;
    }
}

fn drive_shot(
    mut clock: ResMut<ShotClock>,
    // `Time<Real>` (true wall-clock), NOT the default `Res<Time>` (virtual): a hit-stop slowmo or a
    // debug pause scales/freezes virtual time, which must not stall the warm-up gate below.
    time: Res<Time<Real>>,
    path: Res<ShotPath>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    mut cams: Query<(&mut Transform, Option<&mut crate::controls::FlyCam>), With<Camera3d>>,
    hold: Res<ShotHold>,
    mut sky: Option<ResMut<crate::scene::SkyClock>>,
) {
    clock.frame += 1;
    // Take the shot once the scene has actually WARMED: ≥240 rendered frames AND ≥10 s wall-clock
    // (raised from 120/6 s — user report: shaders sometimes still hadn't finished compiling at
    // grab time, so shots captured with missing materials). `FOREST_SHOT_WARMUP=<secs>` raises
    // the wall-clock floor further for pathological cases. The wall-clock floor is load-bearing
    // on a COLD run — the first frame stalls several seconds compiling every render pipeline
    // (and the IBL / atmosphere LUTs take a few frames to settle), and a bare frame count
    // elapses while the GPU is still warming, so the grab races an unfinished scene and captures
    // BLACK or shader-less patches. (On a warm run the extra latency is fine for a verification
    // shot.) Spawn the screenshot exactly once. We exit on file-existence below, so FIRST ensure
    // the parent dir exists AND delete any stale PNG already at this path — otherwise a leftover
    // from a prior run trips the exit guard on this very frame (before the async readback lands)
    // and the harness exits leaving the OLD image in place.
    let min_secs = std::env::var("FOREST_SHOT_WARMUP")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0)
        .max(10.0);
    // ...and once every render pipeline the frame needs has compiled (`pipeline_sync::settled`): on
    // a cold shader cache the frame/clock floor above is long gone before the post stack, terrain
    // and foliage pipelines land, and the shot would catch black / missing geometry.
    // `FOREST_PIPEWAIT=0` skips the wait (A/B: film the warm-up itself).
    let wait = std::env::var("FOREST_PIPEWAIT").map(|v| v != "0").unwrap_or(true);
    clock.settled_frames = if crate::pipeline_sync::settled() || !wait { clock.settled_frames + 1 } else { 0 };
    let settled = clock.settled_frames >= 10;
    let capped = time.elapsed_secs() >= min_secs + PIPELINE_WAIT_SECS;
    if !clock.shot && clock.frame >= 240 && time.elapsed_secs() >= min_secs && (settled || capped) {
        if !settled {
            warn!(
                "capture: shooting with pipelines still compiling ({} waiting, {} visible meshes unready) after {:.0}s",
                crate::pipeline_sync::waiting_pipelines(),
                crate::pipeline_sync::unready_meshes(),
                time.elapsed_secs()
            );
        }
        if let Some(parent) = std::path::Path::new(&path.0).parent().filter(|p| !p.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&path.0);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.0.clone()));
        clock.shot = true;
        clock.shot_at = clock.frame;
    }
    // The shot is an async GPU readback: `save_to_disk` (and the PNG write) only fire once the
    // `ScreenshotCaptured` observer lands, a few frames AFTER the request. So once requested, keep
    // rendering until the (freshly-written) file exists on disk, or a hard safety cap (counted
    // from the REQUEST frame — see `shot_at`), THEN exit — a fixed-frame exit would race the
    // readback and write no PNG.
    if clock.shot && (std::path::Path::new(&path.0).exists() || clock.frame > clock.shot_at + 1800) {
        let seq_wanted = std::env::var("FOREST_SHOT_STORIES").is_ok() || std::env::var("FOREST_SHOT_CAMS").is_ok();
        if seq_wanted && drive_story_seq(&mut clock, &path.0, &mut commands, &mut cams, sky.as_deref_mut()) {
            return;
        }
        if hold.0 {
            return;
        }
        exit.write(AppExit::Success);
    }
}

/// `FOREST_SHOT_CAMS="name:x,y,z,tx,ty,tz|name2:…"` — extra fixed-camera shots after the main one
/// (eye → look-at target, world units), each saved next to the main PNG as `<name>.png`.
fn parse_shot_cams(spec: &str) -> Vec<(String, Transform)> {
    spec.split('|')
        .filter_map(|item| {
            let (name, nums) = item.split_once(':')?;
            let v: Vec<f32> = nums.split(',').filter_map(|t| t.trim().parse().ok()).collect();
            (v.len() == 6).then(|| {
                let tf = Transform::from_xyz(v[0], v[1], v[2]).looking_at(Vec3::new(v[3], v[4], v[5]), Vec3::Y);
                (name.trim().to_string(), tf)
            })
        })
        .collect()
}

/// One extra shot per story site (or per `FOREST_SHOT_CAMS` entry) after the main shot: park the camera (and the fly-cam's eased
/// yaw/pitch, so it doesn't swing back), let ~45 frames settle, grab, wait for the PNG, next.
/// Returns `true` while the sequence still runs.
fn drive_story_seq(
    clock: &mut ShotClock,
    main: &str,
    commands: &mut Commands,
    cams: &mut Query<(&mut Transform, Option<&mut crate::controls::FlyCam>), With<Camera3d>>,
    sky: Option<&mut crate::scene::SkyClock>,
) -> bool {
    if clock.seq.is_none() {
        // Both set: the (filtered, see `stories_cold::shot_cams`) story cams first, then the fixed ones.
        // `FOREST_SHOT_STORIES` is a comma list of sets: `sites` (desert / forest), `wild` (swamp /
        // rocky), `landmarks` (the five landmarks + the Hold), anything else = the snow set. `FOREST_SHOT_ONLY=a,b` keeps only cams whose name
        // contains one of the substrings (one launch for a handful of sites).
        let mut seq: Vec<(String, Transform)> = Vec::new();
        if let Ok(sets) = std::env::var("FOREST_SHOT_STORIES") {
            for set in sets.split(',').map(str::trim) {
                seq.extend(match set {
                    "sites" => crate::sites_warm::shot_cams(),
                    "wild" => crate::wild_sites::shot_cams(),
                    "landmarks" => crate::ruins::shot_cams().into_iter().chain(crate::ork_fortress::shot_cams()).collect(),
                    _ => crate::stories_cold::shot_cams(),
                });
            }
            if let Ok(only) = std::env::var("FOREST_SHOT_ONLY") {
                let keys: Vec<&str> = only.split(',').map(str::trim).filter(|k| !k.is_empty()).collect();
                seq.retain(|(n, _)| keys.iter().any(|k| n.contains(k)));
            }
        }
        if let Ok(spec) = std::env::var("FOREST_SHOT_CAMS") {
            seq.extend(parse_shot_cams(&spec));
        }
        clock.seq = Some(seq);
    }
    let dir = std::path::Path::new(main).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let list = clock.seq.clone().unwrap_or_default();
    let Some((name, tf)) = list.get(clock.seq_idx) else { return false };
    let out = dir.join(format!("{name}.png"));
    if clock.seq_frame == 0 {
        let _ = std::fs::remove_file(&out);
        // `name@0.46` in FOREST_SHOT_CAMS: shoot this view at that time of day (one launch can
        // cover noon and golden hour). The PNG keeps the full name.
        if let (Some(t), Some(sky)) = (name.split_once('@').and_then(|(_, t)| t.parse::<f32>().ok()), sky) {
            sky.t = t.rem_euclid(1.0);
        }
        for (mut t, fly) in cams.iter_mut() {
            *t = *tf;
            if let Some(mut f) = fly {
                let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
                (f.yaw, f.pitch, f.target_yaw, f.target_pitch, f.vel) = (yaw, pitch, yaw, pitch, Vec3::ZERO);
            }
        }
    }
    clock.seq_frame += 1;
    // A new view can bring new pipeline keys into frame — wait for them too (capped, ~30 s).
    if !clock.seq_shot && clock.seq_frame >= 45
        && (crate::pipeline_sync::settled() || clock.seq_frame >= 1800 || std::env::var("FOREST_PIPEWAIT").is_ok_and(|v| v == "0")) {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.clone()));
        clock.seq_shot = true;
    }
    if clock.seq_shot && (out.exists() || clock.seq_frame > 900) {
        info!("story shot {name} saved: {}", out.display());
        clock.seq_idx += 1;
        clock.seq_frame = 0;
        clock.seq_shot = false;
    }
    true
}

// ── clip mode ──────────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct ClipCfg {
    dir: String,
    frames: u32,
    warmup: u32,
    fps: u32,
    orbit: Option<Orbit>,
}

#[derive(Clone, Copy)]
struct Orbit {
    center: Vec3,
    radius: f32,
    height: f32,
    /// degrees per second
    speed: f32,
    /// optional 7th value: starting angle in degrees (0 = the +X side of `center`)
    start: f32,
}

#[derive(Resource, Default)]
struct ClipClock {
    /// total ticks elapsed (warm-up included)
    frame: u32,
    /// frames written to disk
    saved: u32,
    /// tick the last frame was written — start of the flush tail
    done_at: Option<u32>,
    /// consecutive ticks with every render pipeline compiled (see `drive_shot`)
    settled_frames: u32,
    /// warm-up passed; recording (latched)
    started: bool,
    /// flush tail: every frame up to this index is confirmed on disk
    flushed: u32,
}

/// Read by the demo director (`demo.rs` / `town.rs`) so scripted timelines (the hero walk, the
/// build sequence, caption cues) start only once recording begins — the warm-up frames render
/// (shaders compile, lighting / IBL / the world sim settle) without burning the scripted action.
#[derive(Resource, Default)]
pub struct ClipProgress {
    /// false during warm-up, true once frames are being saved
    pub recording: bool,
    /// count of frames written so far (0 during warm-up) — a frame-locked clock for scripts
    pub frame: u32,
}

fn clip_cfg(dir: String) -> ClipCfg {
    let num = |k: &str, d: f32| {
        std::env::var(k).ok().and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(d)
    };
    ClipCfg {
        dir,
        frames: num("FOREST_CLIP_FRAMES", 150.0).max(1.0) as u32,
        warmup: num("FOREST_CLIP_WARMUP", 30.0).max(0.0) as u32,
        fps: num("FOREST_CLIP_FPS", 30.0).max(1.0) as u32,
        orbit: std::env::var("FOREST_CLIP_ORBIT").ok().and_then(parse_orbit),
    }
}

fn parse_orbit(s: String) -> Option<Orbit> {
    let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    (v.len() == 6 || v.len() == 7).then(|| Orbit {
        center: Vec3::new(v[0], v[1], v[2]),
        radius: v[3],
        height: v[4],
        speed: v[5],
        start: v.get(6).copied().unwrap_or(0.0),
    })
}

fn clip_setup(cfg: Res<ClipCfg>, mut vtime: ResMut<Time<Virtual>>) {
    let _ = std::fs::create_dir_all(&cfg.dir);
    // Clamp the per-tick delta to exactly one playback frame. Encoding a PNG every frame makes a
    // tick take far longer than 1/fps of wall-clock, so without this the world would fast-forward
    // in big jumps between saved frames. Clamped, every tick advances the sim by ≤1/fps, so the
    // recorded motion plays back as smooth real-time when ffmpeg assembles at FOREST_CLIP_FPS.
    vtime.set_max_delta(Duration::from_secs_f32(1.0 / cfg.fps as f32));
}

/// Optional cinematic move: circle `center` at a fixed radius/height, `speed` deg/s. Driven off
/// the saved-frame index (not wall time) so the path is deterministic. The fly-cam is idle under
/// capture (no input), so writing the transform here doesn't fight it.
///
/// `FOREST_CLIP_ORBIT_REL=1` makes the centre's y and the camera height offsets above the terrain
/// at the centre (no guessing a biome's ground level), and keeps the eye ≥3u above the ground
/// under it so a low orbit never dips into a hillside.
fn clip_orbit(
    cfg: Res<ClipCfg>,
    clock: Res<ClipClock>,
    mut cam: Query<(&mut Transform, Option<&mut crate::controls::FlyCam>), With<Camera3d>>,
) {
    let Some(o) = cfg.orbit else { return };
    let t = clock.frame.saturating_sub(cfg.warmup) as f32 / cfg.fps as f32;
    let ang = (o.start + o.speed * t).to_radians();
    let (mut center, mut height) = (o.center, o.height);
    let mut pos = Vec3::new(o.center.x + o.radius * ang.cos(), 0.0, o.center.z + o.radius * ang.sin());
    if std::env::var("FOREST_CLIP_ORBIT_REL").is_ok() {
        let g = crate::worldmap::ground_at_world(center.x, center.z).unwrap_or(0.0);
        center.y += g;
        height += g;
        if let Some(under) = crate::worldmap::ground_at_world(pos.x, pos.z) {
            height = height.max(under + 3.0);
        }
    }
    pos.y = height;
    for (mut tf, fly) in &mut cam {
        *tf = Transform::from_translation(pos).looking_at(center, Vec3::Y);
        // The FreeRoam fly-cam rebuilds the rotation from its own yaw/pitch every frame (unordered
        // vs this system) — hand it the orbit's look, or the orbit only moves the eye while the view
        // stays locked on the boot overview heading.
        if let Some(mut fly) = fly {
            let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
            fly.yaw = yaw;
            fly.pitch = pitch;
            fly.target_yaw = yaw;
            fly.target_pitch = pitch;
            fly.vel = Vec3::ZERO;
        }
    }
}

fn drive_clip(
    cfg: Res<ClipCfg>,
    mut clock: ResMut<ClipClock>,
    mut prog: ResMut<ClipProgress>,
    time: Res<Time<Real>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    // Warm-up = the frame count AND the same >= 10 s wall-clock floor as `drive_shot`. A bare
    // frame count elapses in well under a second on a fast preset (SMAA instead of TAA runs ~3x the
    // frame rate during the load): every screenshot was requested before the window rendered at
    // all, none ever landed, and the harness exited with no frames and no error.
    // ...and, like `drive_shot`, only once every render pipeline has compiled: without it a cold
    // shader cache recorded the whole clip as a black world behind a live HUD.
    clock.settled_frames = if crate::pipeline_sync::settled() { clock.settled_frames + 1 } else { 0 };
    let settled = clock.settled_frames >= 10 || time.elapsed_secs() >= 10.0 + PIPELINE_WAIT_SECS;
    if !clock.started && clock.frame >= cfg.warmup && time.elapsed_secs() >= 10.0 && settled {
        clock.started = true;
    }
    let warm = clock.started;
    prog.recording = warm;
    prog.frame = clock.saved;
    // Flush tail: all frames requested → keep rendering until EVERY one is on disk, then exit. The
    // async readbacks run on the shared async-compute pool, which the world streaming can starve
    // in a heavy scene: they land out of order and late, and exiting on just the LAST frame
    // dropped whole runs of mid-clip frames (a 10-frame hole → a stutter + audio drift). Safety cap.
    if let Some(done) = clock.done_at {
        while clock.flushed < cfg.frames
            && std::path::Path::new(&format!("{}/frame_{:05}.png", cfg.dir, clock.flushed + 1)).exists()
        {
            clock.flushed += 1;
        }
        if (clock.frame >= done + 15 && clock.flushed >= cfg.frames) || clock.frame >= done + 3600 {
            if clock.flushed < cfg.frames {
                warn!("clip: exiting with frame {} still unsaved", clock.flushed + 1);
            }
            exit.write(AppExit::Success);
        }
        clock.frame += 1;
        return;
    }

    if warm && clock.saved < cfg.frames {
        clock.saved += 1;
        let path = format!("{}/frame_{:05}.png", cfg.dir, clock.saved);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        if clock.saved >= cfg.frames {
            clock.done_at = Some(clock.frame);
        }
    }
    clock.frame += 1;
}
