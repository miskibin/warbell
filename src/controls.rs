//! Free-look fly camera so the scene can be explored.
//!
//! - **WASD** — move (relative to look direction)
//! - **Space / Left-Ctrl** — up / down
//! - **Left-Shift** — sprint
//! - **Hold Right-Mouse** — look around (cursor is locked + hidden while held)
//!
//! The capture harness still works: with `FOREST_SHOT` set there's no input, so the
//! camera holds its initial pose for the screenshot.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

const SENSITIVITY: f32 = 0.0022;
const MOVE_SPEED: f32 = 9.0;
const SPRINT_MULT: f32 = 3.5;
/// Don't let the camera sink below the canopy floor.
const MIN_Y: f32 = 0.4;

/// Cinematic smoothing rates (1/sec). Higher = snappier, lower = more floaty/filmic.
/// The smoothed value chases its target by `1 - exp(-rate*dt)` each frame (frame-rate
/// independent), so raw mouse jitter and instant key on/off become eased ramps — the slow
/// glide that reads as a tripod/gimbal move in a trailer rather than a twitchy debug fly.
/// Look lags the mouse so flicks settle smoothly; move builds/sheds momentum so dolly
/// starts and stops ease instead of popping.
const LOOK_SMOOTH: f32 = 11.0;
const MOVE_SMOOTH: f32 = 4.5;

/// Marks the camera as fly-controllable. Mouse/keys drive the `target_*`/desired velocity;
/// the live `yaw`/`pitch`/`vel` chase those targets with damping for cinematic motion.
#[derive(Component)]
pub struct FlyCam {
    /// Smoothed (rendered) yaw/pitch — what the transform actually uses.
    pub yaw: f32,
    pub pitch: f32,
    /// Where the mouse wants the look to be; `yaw`/`pitch` ease toward these.
    pub target_yaw: f32,
    pub target_pitch: f32,
    /// Smoothed world-space velocity (momentum), eased toward the keyboard's desired velocity.
    pub vel: Vec3,
}

impl FlyCam {
    pub fn new(yaw: f32, pitch: f32) -> Self {
        FlyCam { yaw, pitch, target_yaw: yaw, target_pitch: pitch, vel: Vec3::ZERO }
    }
}

pub struct ControlsPlugin;

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, fly_camera);
    }
}

fn fly_camera(
    time: Res<Time>,
    mode: Res<crate::player::PlayMode>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    egui_wants: Res<crate::debug_panel::EguiWantsPointer>,
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
    // Exponential smoothing factors, frame-rate independent. Clamped to [0,1] so a long
    // frame (e.g. the per-frame stall during clip capture) can't overshoot past the target.
    let look_a = (1.0 - (-LOOK_SMOOTH * dt).exp()).clamp(0.0, 1.0);
    let move_a = (1.0 - (-MOVE_SMOOTH * dt).exp()).clamp(0.0, 1.0);

    for (mut tf, mut cam) in &mut cam_q {
        // Mouse drives the *target* look; the rendered yaw/pitch lag behind it so flicks and
        // jitter resolve into a smooth glide.
        if looking {
            let d = motion.delta;
            cam.target_yaw -= d.x * SENSITIVITY;
            cam.target_pitch = (cam.target_pitch - d.y * SENSITIVITY).clamp(-1.54, 1.54);
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

        // Desired velocity from keys; the live velocity eases toward it so dolly moves ramp up
        // and coast to a stop instead of popping on/off — the momentum that reads as a gimbal.
        let speed = if keys.pressed(KeyCode::ShiftLeft) { MOVE_SPEED * SPRINT_MULT } else { MOVE_SPEED };
        let desired = dir.normalize_or_zero() * speed;
        let vel = cam.vel + (desired - cam.vel) * move_a;
        cam.vel = vel;
        tf.translation += vel * dt;
        if tf.translation.y < MIN_Y {
            tf.translation.y = MIN_Y;
            if cam.vel.y < 0.0 {
                cam.vel.y = 0.0;
            }
        }
    }
}
