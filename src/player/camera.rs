//! Third-person camera + the free-roam debug toggle. Ported from `MouseLookCamera.tsx`:
//! an over-the-shoulder orbit (azimuth / pitch / dist) with pointer-lock — click to lock the
//! cursor (mouse then rotates the view), Esc to release, wheel to zoom.
//!
//! The backtick key flips [`PlayMode`]: in **FreeRoam** this system yields and
//! `controls::fly_camera` drives the camera instead (for debugging).

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::controls::FlyCam;
use crate::game_state::{AppState, Modal};

use super::{Hero, PlayMode};

/// The "is the cursor interactive / where should the camera frame" inputs, bundled to keep
/// `player_camera` under Bevy's 16-param system cap. Build mode flips the camera up over the town
/// and frees the cursor (`build_mode.active`).
#[derive(SystemParam)]
pub struct CamGate<'w> {
    app: Res<'w, State<AppState>>,
    modal: Option<Res<'w, State<Modal>>>,
    egui_wants: Res<'w, crate::debug_panel::EguiWantsPointer>,
    build_mode: Res<'w, crate::town::BuildMode>,
}

const SENS_X: f32 = 0.0035;
const SENS_Y: f32 = 0.0016;
const MIN_PITCH: f32 = 0.12;
const MAX_PITCH: f32 = 1.45;
const MIN_DIST: f32 = 3.5;
const MAX_DIST: f32 = 16.0;
const ZOOM_SENS: f32 = 0.7;
/// Look-target height above the hero's feet (roughly the helm).
const EYE_H: f32 = 1.0;

/// Build-mode camera pose — eye + look-at, framing the WHOLE settlement (the castle at the origin)
/// centred above the bottom build palette. The look-target is pushed forward (z = 5, toward the
/// camera) so the town sits high enough that the near plots clear the palette. Verified against a
/// staged capture; tune here if the framing drifts.
const BUILD_CAM_EYE: Vec3 = Vec3::new(0.0, 30.0, 22.0);
const BUILD_CAM_LOOK: Vec3 = Vec3::new(0.0, 0.0, 5.0);

#[derive(Resource)]
pub struct OrbitCam {
    pub azimuth: f32,
    pub pitch: f32,
    pub dist: f32,
    pub locked: bool,
}

impl Default for OrbitCam {
    fn default() -> Self {
        OrbitCam { azimuth: std::f32::consts::PI * 0.85, pitch: 0.5, dist: 7.0, locked: false }
    }
}

/// Backtick toggles Play ↔ FreeRoam. Leaving Play frees the cursor and syncs the fly-cam's
/// yaw/pitch to the current view so it doesn't snap when it takes over.
pub fn toggle_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<PlayMode>,
    mut orbit: ResMut<OrbitCam>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cam_q: Query<(&Transform, &mut FlyCam)>,
) {
    if !keys.just_pressed(KeyCode::Backquote) {
        return;
    }
    *mode = match *mode {
        PlayMode::Play => PlayMode::FreeRoam,
        PlayMode::FreeRoam => PlayMode::Play,
    };
    if *mode == PlayMode::FreeRoam {
        if let Ok(mut cur) = cursor_q.single_mut() {
            cur.grab_mode = CursorGrabMode::None;
            cur.visible = true;
        }
        orbit.locked = false;
        if let Ok((tf, mut fly)) = cam_q.single_mut() {
            let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
            fly.yaw = yaw;
            fly.pitch = pitch;
            // Sync smoothing targets + clear momentum so the fly-cam takes over at rest
            // instead of easing toward a stale target or drifting from leftover velocity.
            fly.target_yaw = yaw;
            fly.target_pitch = pitch;
            fly.vel = Vec3::ZERO;
        }
    }
}

pub fn player_camera(
    mode: Res<PlayMode>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut orbit: ResMut<OrbitCam>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
    hero_q: Query<&Hero>,
    mut cam_q: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<Hero>)>,
    time: Res<Time>,
    feedback: Option<Res<crate::combat_fx::HitFeedback>>,
    mut base_fov: Local<Option<f32>>,
    gate: CamGate,
    mut build_blend: Local<f32>,
) {
    if *mode != PlayMode::Play {
        return;
    }
    let Ok(hero) = hero_q.single() else { return };
    let Ok((mut cam_tf, mut cam_proj)) = cam_q.single_mut() else { return };

    // Cursor only locks while actually playing with no panel up; a modal/menu frees it so its
    // buttons are clickable (and a button-click can't re-grab the view). The debug panel
    // (egui_wants) also blocks the grab so clicking a slider never locks + rotates the view.
    // Build mode is non-interactive too: the cursor stays FREE so the player clicks the palette +
    // plots (and combat/arts already gate on `orbit.locked`, so freeing it disables attacking).
    let interactive = *gate.app.get() == AppState::Playing
        && gate.modal.as_ref().map_or(true, |m| *m.get() == Modal::None)
        && !gate.egui_wants.0
        && !gate.build_mode.active;
    if let Ok(mut cur) = cursor_q.single_mut() {
        if interactive {
            if buttons.just_pressed(MouseButton::Left) && !orbit.locked {
                cur.grab_mode = CursorGrabMode::Locked;
                cur.visible = false;
                orbit.locked = true;
            }
            if keys.just_pressed(KeyCode::Escape) && orbit.locked {
                cur.grab_mode = CursorGrabMode::None;
                cur.visible = true;
                orbit.locked = false;
            }
        } else if orbit.locked {
            cur.grab_mode = CursorGrabMode::None;
            cur.visible = true;
            orbit.locked = false;
        }
    }

    if orbit.locked {
        let d = motion.delta;
        orbit.azimuth -= d.x * SENS_X;
        orbit.pitch = (orbit.pitch + d.y * SENS_Y).clamp(MIN_PITCH, MAX_PITCH);
    }

    let s = scroll.delta.y;
    if s != 0.0 {
        orbit.dist = (orbit.dist - s * ZOOM_SENS).clamp(MIN_DIST, MAX_DIST);
    }

    let follow_target = Vec3::new(hero.pos.x, hero.y + EYE_H, hero.pos.y);
    let (a, p, r) = (orbit.azimuth, orbit.pitch, orbit.dist);
    let follow_eye =
        follow_target + Vec3::new(a.sin() * p.cos() * r, p.sin() * r, a.cos() * p.cos() * r);

    // Build mode eases the camera up over the settlement (centred on the castle/origin) so EVERY plot
    // is visible — it doesn't matter which way the hero was facing. Blend back on exit.
    let want = if gate.build_mode.active { 1.0 } else { 0.0 };
    *build_blend += (want - *build_blend) * (1.0 - (-time.delta_secs() * 7.0).exp());
    let blend = (*build_blend).clamp(0.0, 1.0);
    let eye = follow_eye.lerp(BUILD_CAM_EYE, blend);
    let look = follow_target.lerp(BUILD_CAM_LOOK, blend);
    cam_tf.translation = eye;
    cam_tf.look_at(look, Vec3::Y);

    // Trauma-based screen shake + FOV punch layered on the settled pose (fed by combat_fx on hits).
    if let Some(fb) = feedback {
        let s = crate::combat_fx::SHAKE_MAX * fb.trauma * fb.trauma;
        if s > 0.0 {
            let t = time.elapsed_secs();
            let jitter = Vec3::new((t * 47.0).sin(), (t * 59.0).sin(), (t * 41.0).sin());
            cam_tf.translation += jitter * s;
        }
        // FOV punch: widen the lens off the rest FOV (captured once) by the decaying kick.
        if let Projection::Perspective(p) = &mut *cam_proj {
            let base = *base_fov.get_or_insert(p.fov);
            p.fov = base + fb.fov_kick.to_radians();
        }
    }
}
