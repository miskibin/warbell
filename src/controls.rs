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

/// Marks the camera as fly-controllable + stores its yaw/pitch (kept in sync so mouse
/// look and the transform never disagree).
#[derive(Component)]
pub struct FlyCam {
    pub yaw: f32,
    pub pitch: f32,
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
    mut cam_q: Query<(&mut Transform, &mut FlyCam)>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    // The hero follow-cam owns the view in Play mode; the fly-cam only drives in FreeRoam.
    if *mode != crate::player::PlayMode::FreeRoam {
        return;
    }
    // Grab/hide the cursor only while the right button is held, restore on release.
    if let Ok(mut cursor) = cursor_q.single_mut() {
        if buttons.just_pressed(MouseButton::Right) {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        if buttons.just_released(MouseButton::Right) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    let looking = buttons.pressed(MouseButton::Right);
    let dt = time.delta_secs();

    for (mut tf, mut cam) in &mut cam_q {
        if looking {
            let d = motion.delta;
            cam.yaw -= d.x * SENSITIVITY;
            cam.pitch = (cam.pitch - d.y * SENSITIVITY).clamp(-1.54, 1.54);
            tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
        }

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

        let speed = if keys.pressed(KeyCode::ShiftLeft) { MOVE_SPEED * SPRINT_MULT } else { MOVE_SPEED };
        tf.translation += dir.normalize_or_zero() * speed * dt;
        if tf.translation.y < MIN_Y {
            tf.translation.y = MIN_Y;
        }
    }
}
