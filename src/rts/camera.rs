//! RTS isometric camera: reposes the ONE existing `Camera3d` (never a second camera —
//! CLAUDE.md hard rule) into an orthographic iso view with WASD/edge-pan + wheel zoom.
//!
//! Same "one camera, new pose branch" pattern as the campaign build-cam / fly-cam: nothing here
//! spawns or despawns a camera — it fetches the single existing `Camera3d` (via `single_mut`, the
//! way `player_camera`/`drive_dof_focus` do) and rewrites its `Transform`/`Projection` every frame.
//! The campaign camera drivers (`player::camera`, the menu orbit cam) are gated OFF in skirmish by
//! another agent, so in Skirmish this is the sole Playing-state writer of the camera transform.
//!
//! Two responsibilities:
//!  1. `rts_camera_boot` — the *configure-once* work, made idempotent/self-healing: swap the
//!     projection to iso `Orthographic` (once), and strip the passes that don't belong in a clean
//!     top-down view (DoF + god-rays, plus distance fog + atmospherics haze).
//!  2. `rts_drive_camera` — per-frame input (WASD pan, edge-pan, wheel zoom) → `RtsCamFocus`, then
//!     glide the camera into the fixed iso pose over the (terrain-riding) focus point.

use bevy::camera::ScalingMode;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::dof::Dof;
use crate::game_state::AppState;
use crate::godrays::GodRays;
use bevy::pbr::DistanceFog;

// ── fixed iso framing (spec §3) — never rotates in the POC (R is reserved for build rotation) ──
/// Iso yaw: the view spins 45° off the world axes so the arena reads as a diamond.
const ISO_YAW: f32 = std::f32::consts::FRAC_PI_4; // 45°
/// Iso elevation (camera looks down at ~35° below horizontal — a lower, more oblique iso tilt than
/// the old ~50° that read near-top-down; `FOREST_RTS_PITCH=<deg>` overrides for tuning). ≈
/// `35f32.to_radians()`, spelled as a literal because `to_radians` isn't a `const fn`.
const ISO_PITCH_DEFAULT: f32 = 0.610_865; // ≈ 35°

/// Iso pitch in radians, honouring `FOREST_RTS_PITCH=<degrees>` for live tuning.
fn iso_pitch() -> f32 {
    std::env::var("FOREST_RTS_PITCH")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .map(|deg| deg.to_radians())
        .unwrap_or(ISO_PITCH_DEFAULT)
}
/// Eye distance from the focus along the view axis. In an ORTHO projection this does **not** change
/// apparent size (only what falls inside near/far), so it's simply "far enough above the arena that
/// no geometry clips the eye plane" — the zoom is driven by the ortho viewport height instead.
const ISO_DIST: f32 = 90.0;

// ── zoom = vertical world-units of view (ortho `FixedVertical`) ──
const ZOOM_MIN: f32 = 10.0;
const ZOOM_MAX: f32 = 60.0;
/// Mid-range opening zoom (spec §3: "zoom mid-range").
const ZOOM_DEFAULT: f32 = 34.0;
/// World-units of vertical view added/removed per wheel notch.
const ZOOM_STEP: f32 = 3.0;

// ── panning ──
/// Ground pan speed (world-units/sec) at the default zoom; scaled by `zoom / ZOOM_DEFAULT` so a
/// zoomed-out view slides proportionally faster (covers more map per second).
const PAN_SPEED: f32 = 22.0;
/// Cursor within this many pixels of a window edge triggers an edge-pan that way.
const EDGE_PAN_PX: f32 = 24.0;
/// Slack past the arena radius the focus may roam, so the far (rival) base still frames.
const FOCUS_MARGIN: f32 = 8.0;
/// Camera glide rate (1/s) toward the target pose — a subtle ease so pans/zoom feel smooth, not
/// rigid (mirrors the campaign follow-cam's `GLIDE_RATE` feel).
const GLIDE_RATE: f32 = 12.0;
/// Above this jump (world units) the eye snaps instead of gliding — so the very first skirmish
/// frame doesn't sail the camera in from the boot overview pose.
const SNAP_DIST: f32 = 40.0;

/// The RTS camera's ground focus + zoom. Panning moves `pos`; the wheel adjusts `zoom`; the drive
/// system parks the iso camera over `pos` at ground height. Starts over the player's base (spec §3).
#[derive(Resource)]
pub struct RtsCamFocus {
    /// World XZ the camera looks at (castle-at-origin frame).
    pub pos: Vec2,
    /// Vertical extent of the view in world units (ortho `FixedVertical`); clamped to
    /// `[ZOOM_MIN, ZOOM_MAX]`.
    pub zoom: f32,
}

impl Default for RtsCamFocus {
    fn default() -> Self {
        // Capture aid: `FOREST_RTS_CAM="x,z[,zoom]"` opens the camera elsewhere (e.g. framing the
        // rival base for a harness shot). Normal play always opens over the player's base.
        if let Ok(s) = std::env::var("FOREST_RTS_CAM") {
            let p: Vec<f32> = s.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if p.len() >= 2 {
                let zoom = p.get(2).copied().unwrap_or(ZOOM_DEFAULT).clamp(ZOOM_MIN, ZOOM_MAX);
                return RtsCamFocus { pos: Vec2::new(p[0], p[1]), zoom };
            }
        }
        RtsCamFocus { pos: super::PLAYER_BASE, zoom: ZOOM_DEFAULT }
    }
}

pub struct RtsCameraPlugin;

impl Plugin for RtsCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RtsCamFocus>().add_systems(
            Update,
            (rts_camera_boot, rts_drive_camera)
                .chain() // boot swaps to ortho first, so drive sees the ortho to apply zoom
                .run_if(super::in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

/// The *configure-once* half, kept idempotent so it's safe to run every frame (all work is guarded).
///
/// - **Projection swap**: the camera boots `Perspective` (`scene::setup_camera`); the first time
///   this runs in skirmish it becomes iso `Orthographic`. Once orthographic we never touch the
///   variant again — the design forbids runtime projection toggling and the drive system only
///   mutates the ortho's `viewport_height` for zoom.
/// - **Post-stack safety**: DoF and god-rays are perspective-tuned and wrong for a top-down view.
///   Both passes `ViewQuery`-skip when their component is absent (exactly how `quality::apply_quality`
///   disables them), so removing the component is the clean off-switch — and the SAFE direction:
///   *removing* a post pass from a live view never crashes; it's *re-inserting* one that trips the
///   documented wgpu validation crash (CLAUDE.md / menu-static-backdrop). If a graphics-settings
///   change makes `apply_quality` re-add them, this strips them again next frame (self-healing).
fn rts_camera_boot(
    mut commands: Commands,
    mut cam: Query<
        (
            Entity,
            &mut Projection,
            Has<Dof>,
            Has<GodRays>,
            Has<DistanceFog>,
            Has<crate::atmospherics::Atmospherics>,
        ),
        With<Camera3d>,
    >,
) {
    let Ok((e, mut proj, has_dof, has_rays, has_fog, has_atmo)) = cam.single_mut() else {
        return;
    };

    if matches!(*proj, Projection::Perspective(_)) {
        *proj = Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height: ZOOM_DEFAULT },
            ..OrthographicProjection::default_3d()
        });
    }

    // Strip the perspective/atmosphere passes that muddy a top-down RTS read: DoF + god-rays (as
    // before) plus the distance **fog** and the analytic atmospherics haze — the player asked for a
    // clean, crisp skirmish view. Removing a post/fog component from a live view is the SAFE
    // direction (re-inserting is what trips the wgpu crash); this system is self-healing so it
    // re-strips whatever `apply_quality` re-adds. Killing `DistanceFog` also silences the
    // atmospherics driver (its query requires `&DistanceFog`).
    if has_dof || has_rays || has_fog || has_atmo {
        let mut ec = commands.entity(e);
        if has_dof {
            ec.remove::<Dof>();
        }
        if has_rays {
            ec.remove::<GodRays>();
        }
        if has_fog {
            ec.remove::<DistanceFog>();
        }
        if has_atmo {
            ec.remove::<crate::atmospherics::Atmospherics>();
        }
    }
}

/// Per-frame: fold WASD/edge-pan/wheel input into `RtsCamFocus`, then glide the single camera into
/// the fixed iso pose looking at the focus point on the ground.
fn rts_drive_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut focus: ResMut<RtsCamFocus>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
    // Smoothed (eye, look) so the pose glides toward its target instead of snapping — also averages
    // out the half-unit terrace steps in the focus's ground height. `None` until the first frame.
    mut smoothed: Local<Option<(Vec3, Vec3)>>,
) {
    let Ok((mut tf, mut proj)) = cam.single_mut() else {
        return;
    };
    let dt = time.delta_secs();

    // ── zoom (wheel): wheel-up (positive) zooms IN = a smaller vertical view ──
    if scroll.delta.y != 0.0 {
        focus.zoom = (focus.zoom - scroll.delta.y * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    // Ground-plane pan basis, derived from the fixed iso yaw. The eye sits at
    // `focus + ISO_DIST·(sin y·cos p, sin p, cos y·cos p)`, so the horizontal eye→focus direction
    // ("into the screen") is `-(sin y, cos y)`; `right` is its ground perpendicular.
    let (sy, cy) = ISO_YAW.sin_cos();
    let forward = Vec2::new(-sy, -cy);
    let right = Vec2::new(cy, -sy);

    let mut pan = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        pan += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        pan -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        pan += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        pan -= right;
    }

    // Edge-pan: cursor near a window border nudges the focus that way (Stronghold-style). Cursor
    // origin is top-left, so a small `y` is the TOP of the screen → push the view forward/away.
    if let Ok(win) = windows.single() {
        if let Some(c) = win.cursor_position() {
            let (w, h) = (win.width(), win.height());
            if c.x < EDGE_PAN_PX {
                pan -= right;
            }
            if c.x > w - EDGE_PAN_PX {
                pan += right;
            }
            if c.y < EDGE_PAN_PX {
                pan += forward;
            }
            if c.y > h - EDGE_PAN_PX {
                pan -= forward;
            }
        }
    }

    if pan != Vec2::ZERO {
        // Speed scales with zoom; normalise so a diagonal (WASD + edge combined) isn't faster.
        let speed = PAN_SPEED * (focus.zoom / ZOOM_DEFAULT);
        focus.pos += pan.normalize() * speed * dt;
    }
    // Keep the focus inside the arena ellipse (+ margin) — a generous box clamp is plenty here.
    let lim = super::ARENA_RADIUS + FOCUS_MARGIN;
    focus.pos = focus.pos.clamp(Vec2::splat(-lim), Vec2::splat(lim));

    // ── target iso pose over the terrain-riding focus ──
    let gy = crate::worldmap::ground_at_world(focus.pos.x, focus.pos.y).unwrap_or(0.0);
    let look = Vec3::new(focus.pos.x, gy, focus.pos.y);
    let (ps, pc) = iso_pitch().sin_cos();
    let target_eye = look + Vec3::new(sy * pc, ps, cy * pc) * ISO_DIST;

    // Glide toward the target (snap on the first frame / a big jump so it doesn't ease in from the
    // boot overview pose). Smoothing the look point too kills the terrace-step wobble in `gy`.
    let (eye, look_s) = match *smoothed {
        Some((pe, pl)) if pe.distance(target_eye) < SNAP_DIST => {
            let k = 1.0 - (-dt * GLIDE_RATE).exp();
            (pe + (target_eye - pe) * k, pl + (look - pl) * k)
        }
        _ => (target_eye, look),
    };
    *smoothed = Some((eye, look_s));
    tf.translation = eye;
    tf.look_at(look_s, Vec3::Y);

    // Apply the current zoom to the ortho viewport height (once the boot swap has landed).
    if let Projection::Orthographic(o) = &mut *proj {
        o.scaling_mode = ScalingMode::FixedVertical { viewport_height: focus.zoom };
    }
}
