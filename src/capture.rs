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

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::time::Duration;

pub struct CapturePlugin;

#[derive(Resource)]
struct ShotPath(String);

#[derive(Resource, Default)]
struct ShotClock(u32);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("FOREST_SHOT") {
            app.insert_resource(ShotPath(path))
                .init_resource::<ShotClock>()
                .add_systems(Update, drive_shot);
        } else if let Ok(dir) = std::env::var("FOREST_CLIP") {
            app.insert_resource(clip_cfg(dir))
                .init_resource::<ClipClock>()
                .init_resource::<ClipProgress>()
                .add_systems(Startup, clip_setup)
                .add_systems(Update, (clip_orbit, drive_clip).chain());
        }
        // FOREST_NOHUD=1: hide every UI node each frame (HUD, prompts, quick-bar) so a
        // staged shot shows only the world — for marketing/store captures.
        if std::env::var("FOREST_NOHUD").is_ok() {
            app.add_systems(Update, hide_hud);
        }
    }
}

fn hide_hud(mut nodes: Query<&mut Visibility, With<Node>>) {
    for mut vis in &mut nodes {
        *vis = Visibility::Hidden;
    }
}

fn drive_shot(
    mut clock: ResMut<ShotClock>,
    path: Res<ShotPath>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    clock.0 += 1;
    if clock.0 == 90 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.0.clone()));
    }
    if clock.0 > 120 {
        exit.write(AppExit::Success);
    }
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
}

#[derive(Resource, Default)]
struct ClipClock {
    /// total ticks elapsed (warm-up included)
    frame: u32,
    /// frames written to disk
    saved: u32,
    /// tick the last frame was written — start of the flush tail
    done_at: Option<u32>,
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
    (v.len() == 6).then(|| Orbit {
        center: Vec3::new(v[0], v[1], v[2]),
        radius: v[3],
        height: v[4],
        speed: v[5],
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
fn clip_orbit(cfg: Res<ClipCfg>, clock: Res<ClipClock>, mut cam: Query<&mut Transform, With<Camera3d>>) {
    let Some(o) = cfg.orbit else { return };
    let t = clock.frame.saturating_sub(cfg.warmup) as f32 / cfg.fps as f32;
    let ang = (o.speed * t).to_radians();
    let pos = Vec3::new(o.center.x + o.radius * ang.cos(), o.height, o.center.z + o.radius * ang.sin());
    for mut tf in &mut cam {
        *tf = Transform::from_translation(pos).looking_at(o.center, Vec3::Y);
    }
}

fn drive_clip(
    cfg: Res<ClipCfg>,
    mut clock: ResMut<ClipClock>,
    mut prog: ResMut<ClipProgress>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    prog.recording = clock.frame >= cfg.warmup;
    prog.frame = clock.saved;
    // Flush tail: all frames written → wait a few ticks for the async disk writes to land, exit.
    if let Some(done) = clock.done_at {
        if clock.frame >= done + 15 {
            exit.write(AppExit::Success);
        }
        clock.frame += 1;
        return;
    }

    if clock.frame >= cfg.warmup && clock.saved < cfg.frames {
        clock.saved += 1;
        let path = format!("{}/frame_{:05}.png", cfg.dir, clock.saved);
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        if clock.saved >= cfg.frames {
            clock.done_at = Some(clock.frame);
        }
    }
    clock.frame += 1;
}
