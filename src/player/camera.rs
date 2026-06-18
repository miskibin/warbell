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

use super::{Hero, HeroLimb, HeroPart, PlayMode};

/// The "is the cursor interactive / where should the camera frame" inputs, bundled to keep
/// `player_camera` under Bevy's 16-param system cap. Build mode flips the camera up over the town
/// and frees the cursor (`build_mode.active`).
#[derive(SystemParam)]
pub struct CamGate<'w> {
    app: Res<'w, State<AppState>>,
    modal: Option<Res<'w, State<Modal>>>,
    egui_wants: Res<'w, crate::debug_panel::EguiWantsPointer>,
    build_mode: Res<'w, crate::town::BuildMode>,
    /// Set while a warden rears back for a killing blow — eases a tension dolly-out (see below).
    crit_tension: Option<Res<'w, crate::boss::CritTension>>,
}

const SENS_X: f32 = 0.0035;
const SENS_Y: f32 = 0.0016;
const MIN_PITCH: f32 = 0.12;
const MAX_PITCH: f32 = 1.45;
const MIN_DIST: f32 = 2.3;
const MAX_DIST: f32 = 11.0;
const ZOOM_SENS: f32 = 0.55;
/// Extra follow distance pulled back (smoothly) while a warden winds up its killing blow — a slow
/// dolly-out that builds tension across the telegraph, then eases back in once the blow resolves.
const CRIT_ZOOM_OUT: f32 = 4.5;
/// Look-target height above the hero's feet. The knight is only ~0.765u tall (1.25u TS ×
/// `HERO_SCALE`), so this aims at his chest, NOT above the helm — an above-the-head target shoves
/// the (already small) hero toward the bottom of frame and makes him read as a distant speck.
/// Framing the torso keeps him centred and prominent.
const EYE_H: f32 = 0.55;

/// First-person eye height above the hero's feet. The knight stands ~0.765u tall (1.25u TS ×
/// `HERO_SCALE`), so this sits right at the helm/eye line — earlier it was 1.3u, which floated the
/// camera a half-unit ABOVE the knight's own head (felt "too tall" and dropped the sword-hand,
/// at ~0.3u, clean off the bottom of frame). Verified against a staged `FOREST_FP` shot.
const FP_EYE_H: f32 = 0.74;
/// First-person forward eye offset (world units along the look direction). Eyes sit at the FRONT
/// of the head, not the body centre — this nudges the viewpoint toward what you're aiming at so a
/// tree/ork at swing reach (`verbs::SWING_RANGE` = 1.9u) reads as reachable instead of "hug it".
/// Kept SMALL: the sword hand only projects ~0.3u in front of the body, so too large an offset
/// puts the eye on top of (or past) the weapon and it falls out of the forward frustum. The eye
/// must stay BEHIND the sword-hand for the viewmodel to read. Slightly NEGATIVE: the eye sits a
/// touch behind the body centre (the head is hidden in FP, so nothing clips) so the raised
/// sword-arm has room to project forward into the lens instead of straddling it.
const FP_FWD_OFF: f32 = -0.08;
/// First-person look-pitch clamp (radians): how far you can crane up/down. Symmetric, unlike the
/// third-person `MIN/MAX_PITCH` (which is camera *elevation*, always tilting the view downward).
const FP_PITCH_LIMIT: f32 = 1.3;

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
        OrbitCam { azimuth: std::f32::consts::PI * 0.85, pitch: 0.42, dist: 4.2, locked: false }
    }
}

/// First-person sub-mode of [`PlayMode::Play`] (you still drive the knight). Toggled by the HUD
/// eye button / **V** key. `blend` eases the third⇄first transition (a smooth dolly-in, never a
/// hard cut). `pitch` is the FP look-pitch, kept SEPARATE from [`OrbitCam::pitch`] (which is camera
/// elevation and always tilts the view down); `azimuth` is shared with the orbit so your heading
/// survives the toggle. Transient — not saved (a view preference, like the debug panel).
#[derive(Resource, Default)]
pub struct FirstPerson {
    pub active: bool,
    pub blend: f32,
    pub pitch: f32,
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

/// **V** flips first-person on/off (the HUD eye button writes the same flag). Ungated like the
/// other view toggles so it works through pauses/panels; the pose change only takes effect in
/// `player_camera`, which runs only in `PlayMode::Play`.
pub fn toggle_first_person(
    keys: Res<ButtonInput<KeyCode>>,
    mut fp: ResMut<FirstPerson>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    time: Res<Time>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        fp.active = !fp.active;
        notice.push(if fp.active { "First person" } else { "Third person" }, time.elapsed_secs_f64());
    }
}

/// First-person **viewmodel** visibility. The head would fill the lens, so in FP we hide the
/// head + torso + legs but KEEP the arms, the shield, and the weapon (nested under the sword arm) —
/// so a swing shows your sword and a block shows your shield. Restores everything in third person.
/// Driven off `fp.blend` (the eased toggle), one frame behind the camera — imperceptible.
pub fn fp_body_visibility(
    fp: Res<FirstPerson>,
    hero_q: Query<&Children, With<Hero>>,
    mut vis_q: Query<(&mut Visibility, Option<&HeroPart>)>,
) {
    let fp_on = fp.blend > 0.5;
    let Ok(children) = hero_q.single() else { return };
    for &c in children {
        let Ok((mut vis, part)) = vis_q.get_mut(c) else { continue };
        // Keep the arms + shield (the weapon inherits the sword arm); drop the torso/head/legs.
        let keep = matches!(
            part.map(|p| p.limb),
            Some(HeroLimb::ArmR | HeroLimb::ArmL | HeroLimb::Shield)
        );
        let want = if fp_on && !keep { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
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
    mut fp: ResMut<FirstPerson>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut hero_q: Query<&mut Hero>,
    mut cam_q: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<Hero>)>,
    time: Res<Time>,
    feedback: Option<Res<crate::combat_fx::HitFeedback>>,
    mut base_fov: Local<Option<f32>>,
    gate: CamGate,
    mut build_blend: Local<f32>,
    mut tension_blend: Local<f32>,
) {
    if *mode != PlayMode::Play {
        return;
    }
    let Ok(mut hero) = hero_q.single_mut() else { return };
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
        // Vertical mouse drives the FP look-pitch in first person (crane up/down), the orbit
        // elevation otherwise. `-= d.y` so moving the mouse up looks up (non-inverted).
        if fp.active {
            fp.pitch = (fp.pitch - d.y * SENS_Y).clamp(-FP_PITCH_LIMIT, FP_PITCH_LIMIT);
        } else {
            orbit.pitch = (orbit.pitch + d.y * SENS_Y).clamp(MIN_PITCH, MAX_PITCH);
        }
    }

    let s = scroll.delta.y;
    if s != 0.0 {
        orbit.dist = (orbit.dist - s * ZOOM_SENS).clamp(MIN_DIST, MAX_DIST);
    }

    // Tension dolly: ease a 0→1 blend toward "any warden is winding up a crit", slow enough to read
    // as a deliberate pull-back across the ~1.2s telegraph (and a smooth ease-in on release).
    let want_tension = if gate.crit_tension.as_ref().is_some_and(|t| t.0) { 1.0 } else { 0.0 };
    *tension_blend += (want_tension - *tension_blend) * (1.0 - (-time.delta_secs() * 3.2).exp());
    let r_tension = (*tension_blend).clamp(0.0, 1.0) * CRIT_ZOOM_OUT;

    let follow_target = Vec3::new(hero.pos.x, hero.y + EYE_H, hero.pos.y);
    let (a, p, r) = (orbit.azimuth, orbit.pitch, orbit.dist + r_tension);
    let follow_eye =
        follow_target + Vec3::new(a.sin() * p.cos() * r, p.sin() * r, a.cos() * p.cos() * r);

    // ── First-person pose ──
    // The orbit's *view* heading (camera-forward yaw) is `azimuth + π` — the look-yaw FP must use so
    // exiting FP doesn't snap your heading. Forward is built from that yaw + the FP look-pitch.
    let look_yaw = a + std::f32::consts::PI;
    let (fpy_sin, fpy_cos) = look_yaw.sin_cos();
    let (fpp_sin, fpp_cos) = fp.pitch.sin_cos();
    // Eye sits at head height, nudged forward along the (horizontal) look direction.
    let fp_eye = Vec3::new(
        hero.pos.x + fpy_sin * FP_FWD_OFF,
        hero.y + FP_EYE_H,
        hero.pos.y + fpy_cos * FP_FWD_OFF,
    );
    let fp_fwd = Vec3::new(fpy_sin * fpp_cos, fpp_sin, fpy_cos * fpp_cos);
    let fp_look = fp_eye + fp_fwd;

    // Ease the FP blend (smooth dolly-in, no hard cut). The third⇄FP pose is resolved first, then
    // build mode (if active) lerps that overhead — so build framing always wins over FP.
    let want_fp = if fp.active { 1.0 } else { 0.0 };
    fp.blend += (want_fp - fp.blend) * (1.0 - (-time.delta_secs() * 9.0).exp());
    let fpb = fp.blend.clamp(0.0, 1.0);

    // In FP the body owns no facing — the view does (so attacks/arts fire where you look). Movement
    // skips its facing-steer while `fp.active`, so this is the sole writer; coupling on `active` (not
    // `blend`) keeps it consistent through the transition.
    if fp.active {
        hero.facing = look_yaw;
    }
    // (Body parts are hidden/shown by `fp_body_visibility` — a first-person viewmodel: arms +
    // weapon + shield stay, head/torso/legs go, so the head never fills the lens.)

    // Build mode eases the camera up over the settlement (centred on the castle/origin) so EVERY plot
    // is visible — it doesn't matter which way the hero was facing. Blend back on exit.
    let want = if gate.build_mode.active { 1.0 } else { 0.0 };
    *build_blend += (want - *build_blend) * (1.0 - (-time.delta_secs() * 7.0).exp());
    let blend = (*build_blend).clamp(0.0, 1.0);
    let base_eye = follow_eye.lerp(fp_eye, fpb);
    let base_look = follow_target.lerp(fp_look, fpb);
    let eye = base_eye.lerp(BUILD_CAM_EYE, blend);
    let look = base_look.lerp(BUILD_CAM_LOOK, blend);
    cam_tf.translation = eye;
    cam_tf.look_at(look, Vec3::Y);

    // Trauma-based screen shake + FOV punch layered on the settled pose (fed by combat_fx on hits).
    // Damped by ~75% in first person: at eye-scale the raw high-frequency jitter would fill the
    // whole view (a motion-sickness + photosensitive-oscillation hazard) — damped, not removed, so
    // a hit still reads as a hit. `damp` scales with the FP blend so the transition is seamless.
    if let Some(fb) = feedback {
        let damp = 1.0 - 0.75 * fpb;
        let s = crate::combat_fx::SHAKE_MAX * fb.trauma * fb.trauma * damp;
        if s > 0.0 {
            let t = time.elapsed_secs();
            let jitter = Vec3::new((t * 47.0).sin(), (t * 59.0).sin(), (t * 41.0).sin());
            cam_tf.translation += jitter * s;
        }
        // FOV punch: widen the lens off the rest FOV (captured once) by the decaying kick.
        if let Projection::Perspective(p) = &mut *cam_proj {
            let base = *base_fov.get_or_insert(p.fov);
            p.fov = base + fb.fov_kick.to_radians() * damp;
        }
    }
}
