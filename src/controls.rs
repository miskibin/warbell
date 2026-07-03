//! Free-look fly camera. WASD move · Space/Ctrl up·down · Shift sprint · RMB look.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

const SENSITIVITY: f32 = 0.0022;
const MOVE_SPEED: f32 = 22.0;
const SPRINT_MULT: f32 = 3.5;
/// Don't let the camera sink below the canopy floor.
const MIN_Y: f32 = 0.4;

/// Smoothing rates (1/sec). High = snappy, low = floaty.
const LOOK_SMOOTH: f32 = 18.0;
const MOVE_SMOOTH: f32 = 12.0;

// ── Cinematic free-roam (toggle with C) ──
/// A slow, heavily-damped fly for filming/screenshots: crawl speed + long glide-out so pans and
/// dollies read as deliberate camera moves instead of debug darting. Mouse sensitivity is also
/// scaled down so a look eases around. Toggled by [`CinematicCam`]; only in FreeRoam.
const CINE_MOVE_SPEED: f32 = 5.0;
const CINE_LOOK_SMOOTH: f32 = 3.5;
const CINE_MOVE_SMOOTH: f32 = 2.2;
const CINE_SENS_MULT: f32 = 0.45;

/// Cinematic free-roam toggle (C key). Off = the normal snappy debug fly; on = the slow, floaty
/// filming glide. Only meaningful in [`crate::player::PlayMode::FreeRoam`].
#[derive(Resource, Default)]
pub struct CinematicCam(pub bool);

/// Marks the camera as fly-controllable. Mouse/keys drive the `target_*`/desired velocity;
/// the live `yaw`/`pitch`/`vel` chase those targets with damping for cinematic motion.
#[derive(Component)]
pub struct FlyCam {
    /// Smoothed (rendered) yaw/pitch — what the transform actually uses.
    pub yaw: f32,
    pub pitch: f32,
    /// Where the mouse wants the look to be; the rendered `yaw`/`pitch` ease toward these.
    pub target_yaw: f32,
    pub target_pitch: f32,
    /// Smoothed world-space velocity (momentum); eased toward the keyboard's desired velocity.
    pub vel: Vec3,
}

impl FlyCam {
    pub fn new(yaw: f32, pitch: f32) -> Self {
        FlyCam {
            yaw,
            pitch,
            target_yaw: yaw,
            target_pitch: pitch,
            vel: Vec3::ZERO,
        }
    }
}

pub struct ControlsPlugin;

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CinematicCam>()
            .add_systems(Update, (toggle_cinematic, fly_camera).chain());
    }
}

/// **C** flips the cinematic free-roam glide on/off (only in FreeRoam — in Play, C is unused here).
fn toggle_cinematic(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<crate::player::PlayMode>,
    mut cine: ResMut<CinematicCam>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    time: Res<Time>,
) {
    if *mode != crate::player::PlayMode::FreeRoam || !keys.just_pressed(KeyCode::KeyC) {
        return;
    }
    cine.0 = !cine.0;
    notice.push(
        if cine.0 { "Cinematic camera" } else { "Free camera" },
        time.elapsed_secs_f64(),
    );
}

fn fly_camera(
    time: Res<Time>,
    mode: Res<crate::player::PlayMode>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    egui_wants: Res<crate::debug_panel::EguiWantsPointer>,
    cine: Res<CinematicCam>,
    mut cam_q: Query<(&mut Transform, &mut FlyCam)>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    // The hero follow-cam owns the view in Play mode; the fly-cam only drives in FreeRoam.
    if *mode != crate::player::PlayMode::FreeRoam {
        return;
    }
    // Don't grab the cursor / look around while the debug panel owns the pointer (so dragging
    // a slider can't rotate the view).
    let over_ui = egui_wants.0;
    // Grab/hide the cursor only while the right button is held, restore on release.
    if let Ok(mut cursor) = cursor_q.single_mut() {
        if buttons.just_pressed(MouseButton::Right) && !over_ui {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        if buttons.just_released(MouseButton::Right) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    let looking = buttons.pressed(MouseButton::Right) && !over_ui;
    let dt = time.delta_secs();
    // Cinematic mode swaps in the slow, floaty constants (crawl speed + long glide, lighter mouse).
    let (look_smooth, move_smooth, move_speed, sens) = if cine.0 {
        (CINE_LOOK_SMOOTH, CINE_MOVE_SMOOTH, CINE_MOVE_SPEED, SENSITIVITY * CINE_SENS_MULT)
    } else {
        (LOOK_SMOOTH, MOVE_SMOOTH, MOVE_SPEED, SENSITIVITY)
    };
    // Exponential smoothing factors, frame-rate independent. Clamped to [0,1] so a long
    // frame (e.g. the per-frame stall during clip capture) can't overshoot past the target.
    let look_a = (1.0 - (-look_smooth * dt).exp()).clamp(0.0, 1.0);
    let move_a = (1.0 - (-move_smooth * dt).exp()).clamp(0.0, 1.0);

    for (mut tf, mut cam) in &mut cam_q {
        // Mouse drives the *target* look; the rendered yaw/pitch lag behind it so flicks and
        // jitter resolve into a smooth glide.
        if looking {
            let d = motion.delta;
            cam.target_yaw -= d.x * sens;
            cam.target_pitch = (cam.target_pitch - d.y * sens).clamp(-1.54, 1.54);
        }
        cam.yaw += (cam.target_yaw - cam.yaw) * look_a;
        cam.pitch += (cam.target_pitch - cam.pitch) * look_a;
        tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);

        let forward = tf.forward();
        let right = tf.right();
        let mut dir = Vec3::ZERO;
        if keys.pressed(KeyCode::KeyW) {
            dir += *forward;
        }
        if keys.pressed(KeyCode::KeyS) {
            dir -= *forward;
        }
        if keys.pressed(KeyCode::KeyD) {
            dir += *right;
        }
        if keys.pressed(KeyCode::KeyA) {
            dir -= *right;
        }
        if keys.pressed(KeyCode::Space) {
            dir += Vec3::Y;
        }
        if keys.pressed(KeyCode::ControlLeft) {
            dir -= Vec3::Y;
        }

        let speed = if keys.pressed(KeyCode::ShiftLeft) { move_speed * SPRINT_MULT } else { move_speed };
        let desired = dir.normalize_or_zero() * speed;
        let new_vel = cam.vel + (desired - cam.vel) * move_a;
        cam.vel = new_vel;
        tf.translation += cam.vel * dt;
        if tf.translation.y < MIN_Y {
            tf.translation.y = MIN_Y;
            if cam.vel.y < 0.0 {
                cam.vel.y = 0.0;
            }
        }
    }
}
