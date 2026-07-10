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
    /// Set while a warden rears back for a killing blow — eases a tension dolly-out (see below).
    crit_tension: Option<Res<'w, crate::boss::CritTension>>,
    /// The succession beat — when active, the camera is pulled to frame the peasant being possessed.
    succ: Res<'w, crate::succession::Succession>,
}

const SENS_X: f32 = 0.0035;
const SENS_Y: f32 = 0.0016;
const MIN_PITCH: f32 = 0.06;
const MAX_PITCH: f32 = 1.45;
const MIN_DIST: f32 = 1.9;
const MAX_DIST: f32 = 11.0;
const ZOOM_SENS: f32 = 0.55;
/// Witcher-style over-the-shoulder framing: the look target (and with it the whole orbit) is
/// shifted this far to the camera-right, so the hero stands left-of-centre and the view looks
/// past his sword shoulder instead of pinning him dead-centre like a turret. Fades out with the
/// first-person blend (FP aims through the eyes, no shoulder bias).
const SHOULDER_X: f32 = 0.42;
/// Minimum gap kept between the third-person eye and the terrain below it. When the orbit would sink
/// the camera into a hill/slope between the hero and the lens, the eye is lifted to ground + this so
/// it never punches under the world (the "camera dips below the hill" bug). Covers the 0.04 near
/// plane plus a little body so geometry doesn't poke into frame.
const CAM_GROUND_CLEAR: f32 = 0.5;
/// Extra follow distance pulled back (smoothly) while a warden winds up its killing blow — a slow
/// dolly-out that builds tension across the telegraph, then eases back in once the blow resolves.
const CRIT_ZOOM_OUT: f32 = 4.5;
/// Look-target height above the hero's feet. Aims at the knight's upper chest / shoulder line
/// (Witcher-style — drops the horizon lower in frame so more of the world reads over his
/// shoulder), NOT above the helm — an above-the-head target shoves the hero toward the bottom of
/// frame. (Tracks the `HERO_SCALE` bump; shoulder sits ≈1.02 world-units up.)
const EYE_H: f32 = 0.92;

/// First-person eye height above the hero's feet — sits right at the helm/eye line. (Scaled ×1.5
/// alongside the `HERO_SCALE` bump; an eye floating above the helm reads as "too tall" and drops the
/// sword-hand off the bottom of frame.) Original 0.74 × 1.35. Verify with `FOREST_FP`.
const FP_EYE_H: f32 = 1.32;
/// First-person forward eye offset (world units along the look direction). Now that the whole hero
/// is rendered in FP (no body cull), the eye must sit IN FRONT of the head — otherwise the camera
/// is behind the body centre and you stare at your own back/helm. Push it past the head radius so
/// you look OUT of the face, body behind the lens, weapon-arm projecting forward into view.
const FP_FWD_OFF: f32 = 0.05;
/// First-person look-pitch clamp (radians): how far you can crane up/down. Symmetric, unlike the
/// third-person `MIN/MAX_PITCH` (which is camera *elevation*, always tilting the view downward).
const FP_PITCH_LIMIT: f32 = 1.3;
/// First-person close-quarters FOV widen: up to this many degrees as the ringed foe closes from
/// [`FP_CLOSE_RANGE`] to point-blank. Enemies are taller than the 1.32 FP eye and stop at melee
/// range, so without this a big foe is one face filling the lens. Eased slowly (~3/s) so it reads
/// as breathing room, never a zoom pump; scaled by the FP blend so third person is untouched.
const FP_CLOSE_FOV_DEG: f32 = 8.0;
const FP_CLOSE_RANGE: f32 = 3.5;

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
    /// Low-passed look-ahead lead (world XZ), eased toward the hero's heading×moving so a direction
    /// change slides the framing instead of snapping. Kept here (not a new system `Local`) because the
    /// camera system is already at Bevy's 16-param ceiling.
    pub lead: Vec3,
    /// Smoothed follow anchor (world, eye-height) — eased toward the hero so the rig glides behind him
    /// instead of rigidly tracking his exact position every frame. Snaps on a big jump (spawn/load).
    pub anchor: Vec3,
    /// Smoothed combat dolly-out (world units) — eased toward the stance × threat-count target so
    /// a foe wandering across the tally line can't pump the zoom. Kept here (not a system `Local`)
    /// because `player_camera` sits at Bevy's 16-param ceiling.
    pub combat: f32,
}

impl Default for OrbitCam {
    fn default() -> Self {
        OrbitCam {
            // Boot the orbit looking the way the hero faces (over his shoulder, at the castle):
            // view dir = -(sin az, cos az) so az = facing + π. The old hand-tuned 0.85π matched
            // the retired north-gate spawn; deriving from `spawn_point` keeps the opening frame
            // on the keep if the spawn ever moves again.
            azimuth: crate::player::spawn_point().1 + std::f32::consts::PI,
            // Witcher-style default pose: flatter pitch (near-level, looking along the world
            // rather than down at the hero's back) and a closer follow so he reads large in frame.
            pitch: 0.26,
            dist: 3.3,
            locked: false,
            lead: Vec3::ZERO,
            anchor: Vec3::ZERO,
            combat: 0.0,
        }
    }
}

/// Camera-glide ease rate (1/s) for the follow anchor — subtle (flows behind him, not laggy).
const GLIDE_RATE: f32 = 13.0;
/// Separate, much slower ease rate (1/s) for the anchor's **vertical** component. Terrain is
/// terraced (`worldmap::GROUND_STEP` = 0.5), so `hero.y` snaps a half-unit at every tile edge the
/// hero walks up/down. Easing Y at the full `GLIDE_RATE` tracks those snaps near-1:1 and the camera
/// lurches behind him; a slow vertical glide averages the steps into a smooth float. XZ stays at
/// `GLIDE_RATE` so the horizontal follow is still snappy. Only used while grounded — a real jump/fall
/// keeps the fast rate so the camera doesn't lag a genuine vertical move (see `player_camera`).
const VERT_GLIDE_RATE: f32 = 4.0;
/// Sprint "speed feel": how far the camera dollies back (world units) + how much the FOV widens
/// (degrees) at full run, eased by `hero.run_amt`.
const SPRINT_DOLLY: f32 = 0.38;
const SPRINT_FOV_DEG: f32 = 5.0;

// ── Combat framing (the stance's camera) ──
// The cardinal rule (GDC "50 Camera Mistakes"): the camera must NEVER fight the mouse — azimuth
// stays 100% player-owned even in combat (auto-rotation + strafe steering is the classic spin
// feedback loop). Instead the stance gets two gentle, non-rotational biases: the LOOK-TARGET
// slides toward the ringed foe so both fighters share the frame, and the rig DOLLIES BACK a
// touch, more as the scrap grows (the Witcher-mod "camera preset by enemy count" pattern).
/// How far the framing slides toward the foe at full stance (world units; cf. the 0.42 shoulder).
const COMBAT_LEAD: f32 = 0.55;
/// Dolly-out (world units) by threat tier: a duel / a pair / a mob.
const COMBAT_DOLLY_DUEL: f32 = 0.35;
const COMBAT_DOLLY_GROUP: f32 = 0.9;
const COMBAT_DOLLY_MOB: f32 = 1.3;
/// FOV widen (degrees) per world-unit of combat dolly — a slight wide-angle as the fight grows.
const COMBAT_FOV_PER_UNIT: f32 = 2.0;

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
    /// FP swing camera-sway (screen-space radians: x = pitch(+up), y = yaw(+left), z = roll),
    /// written by `anim::hero_anim` from the attack envelopes and applied here on top of the
    /// settled FP pose — one smooth lean per cut (anticipation pulls opposite, the strike rides
    /// the blade), NOT a shake. Zeroed outside FP.
    pub sway: Vec3,
    /// Smoothed FP close-quarters FOV widen (degrees). A `WORLD_BUMP`-scaled ork at melee range
    /// fills the whole first-person frame (face-in-lens); this eases a modest wide-angle in as
    /// the ringed foe closes, buying back its silhouette + HP bar. See `FP_CLOSE_FOV_DEG`.
    pub close_fov: f32,
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

/// First-person body visibility. FP renders the **whole hero** (so the hands don't flicker) except
/// meshes flagged `fp_hide` — just the head, which would otherwise fill the lens as a black blob.
/// Restores everything in third person. Driven off `fp.blend` (the eased toggle), one frame behind.
pub fn fp_body_visibility(fp: Res<FirstPerson>, mut vis_q: Query<(&mut Visibility, &super::HeroMesh)>) {
    let fp_on = fp.blend > 0.5;
    for (mut vis, mesh) in &mut vis_q {
        let want = if fp_on && mesh.fp_hide { Visibility::Hidden } else { Visibility::Inherited };
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

    // Look-ahead lead: shift the framing slightly in the hero's heading while moving, so the camera
    // shows more of where you're going. Subtle — keeps him near centre. The target is low-passed
    // (lead_smooth) so a direction change (e.g. starting a diagonal) eases sideways instead of
    // snapping — an instant facing read jerks the camera the moment you press a strafe key.
    // In the COMBAT STANCE the lead slides toward the ringed foe instead of the heading, so both
    // fighters share the frame — a framing bias only, never a rotation (see the combat constants).
    let heading_lead =
        Vec3::new(hero.facing.sin(), 0.0, hero.facing.cos()) * (0.22 * hero.moving_amt.clamp(0.0, 1.0));
    let lead_target = match hero.soft_pos {
        Some(tp) if hero.stance_amt > 0.01 => {
            let to = Vec3::new(tp.x - hero.pos.x, 0.0, tp.y - hero.pos.y);
            heading_lead.lerp(to.clamp_length_max(1.0) * COMBAT_LEAD, hero.stance_amt.clamp(0.0, 1.0))
        }
        _ => heading_lead,
    };
    let new_lead = orbit.lead + (lead_target - orbit.lead) * (1.0 - (-time.delta_secs() * 4.5).exp());
    orbit.lead = new_lead;
    // Camera glide: ease the followed anchor toward the hero. Snap on a big jump (spawn/respawn/load)
    // so it doesn't sail in from the old position.
    let raw_anchor = Vec3::new(hero.pos.x, hero.y + EYE_H, hero.pos.y);
    if orbit.anchor.distance(raw_anchor) > 5.0 {
        orbit.anchor = raw_anchor;
    } else {
        // Decouple vertical from horizontal: XZ glides fast, Y glides slow so terraced ground steps
        // (`hero.y` snaps 0.5u per tile edge) float by instead of jerking the camera. Airborne, Y
        // tracks at the full rate so a real jump/fall isn't laggy. See `VERT_GLIDE_RATE`.
        let kxz = 1.0 - (-time.delta_secs() * GLIDE_RATE).exp();
        let y_rate = if hero.on_ground { VERT_GLIDE_RATE } else { GLIDE_RATE };
        let ky = 1.0 - (-time.delta_secs() * y_rate).exp();
        orbit.anchor.x += (raw_anchor.x - orbit.anchor.x) * kxz;
        orbit.anchor.z += (raw_anchor.z - orbit.anchor.z) * kxz;
        orbit.anchor.y += (raw_anchor.y - orbit.anchor.y) * ky;
    }
    // Witcher-style shoulder offset: slide the whole framing to the camera-right (right =
    // `(cos az, 0, -sin az)` for this orbit parameterisation) so the hero stands left-of-centre
    // and the view runs past his sword shoulder. The shifted target feeds BOTH the eye and the
    // look-at, so the orbit still circles the hero — the rig just rides offset. First person
    // needs no explicit fade: `fp_eye`/`fp_look` are built from the raw hero position, so the
    // FP blend below washes the offset out on its own.
    let cam_right = Vec3::new(orbit.azimuth.cos(), 0.0, -orbit.azimuth.sin());
    let follow_target = orbit.anchor + orbit.lead + cam_right * SHOULDER_X;
    // Speed feel: sprinting dollies the camera back a touch (eased by `run_amt`).
    let r_speed = hero.run_amt.clamp(0.0, 1.0) * SPRINT_DOLLY;
    // Combat dolly: the stance eases the camera back — a touch for a duel, more as the scrap
    // grows. Smoothed on the orbit so a foe crossing the threat-tally line can't pump the zoom.
    let want_combat = hero.stance_amt.clamp(0.0, 1.0)
        * match hero.threats {
            0 | 1 => COMBAT_DOLLY_DUEL,
            2 => COMBAT_DOLLY_GROUP,
            _ => COMBAT_DOLLY_MOB,
        };
    orbit.combat += (want_combat - orbit.combat) * (1.0 - (-time.delta_secs() * 2.5).exp());
    let (a, p, r) = (orbit.azimuth, orbit.pitch, orbit.dist + r_tension + r_speed + orbit.combat);
    let mut follow_eye =
        follow_target + Vec3::new(a.sin() * p.cos() * r, p.sin() * r, a.cos() * p.cos() * r);
    // Terrain clamp: never let the follow eye sink below the ground under it (a slope/ridge between
    // hero and camera). `smooth_surface_y` is continuous so the lift glides instead of popping; only
    // the third-person eye is clamped (FP rides at head height; build cam is far overhead). `None` —
    // over water / off the island — leaves the eye free.
    if let Some(g) = crate::worldmap::ground_at_world(follow_eye.x, follow_eye.z) {
        follow_eye.y = follow_eye.y.max(g + CAM_GROUND_CLEAR);
    }

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
    let mut base_eye = follow_eye.lerp(fp_eye, fpb);
    let mut base_look = follow_target.lerp(fp_look, fpb);
    // Succession cinematic: pull the camera to orbit the peasant being possessed (reusing the
    // current azimuth/pitch, pulled back a touch, so it reads as a deliberate move, not a cut).
    // Eased in/out by `cam_blend`; because the risen hero ends up AT `steal_pos`, blending back out
    // hands control smoothly to the normal follow.
    if gate.succ.active && gate.succ.cam_blend > 0.001 {
        let ct = Vec3::new(gate.succ.steal_pos.x, gate.succ.steal_pos.y + EYE_H, gate.succ.steal_pos.z);
        let (ca, cp, cr) = (orbit.azimuth, (orbit.pitch + 0.18).min(MAX_PITCH), orbit.dist + 1.8);
        let ce = ct + Vec3::new(ca.sin() * cp.cos() * cr, cp.sin() * cr, ca.cos() * cp.cos() * cr);
        let k = gate.succ.cam_blend.clamp(0.0, 1.0);
        base_eye = base_eye.lerp(ce, k);
        base_look = base_look.lerp(ct, k);
    }
    let eye = base_eye.lerp(BUILD_CAM_EYE, blend);
    let look = base_look.lerp(BUILD_CAM_LOOK, blend);
    cam_tf.translation = eye;
    cam_tf.look_at(look, Vec3::Y);

    // FP swing sway: the small smooth camera lean `anim::hero_anim` derives from the attack
    // envelopes (anticipation ↔ strike). Composed AFTER look_at so it tilts the settled view;
    // purely cosmetic — `hero.facing`/aim still come from the un-swayed look_yaw above.
    if fpb > 0.0 && fp.sway != Vec3::ZERO {
        let s = fp.sway * fpb;
        cam_tf.rotation *= Quat::from_euler(EulerRot::YXZ, s.y, s.x, s.z);
    }

    // Trauma-based screen shake + FOV punch layered on the settled pose (fed by combat_fx on hits).
    // Damped by ~75% in first person: at eye-scale the raw high-frequency jitter would fill the
    // whole view (a motion-sickness + photosensitive-oscillation hazard) — damped, not removed, so
    // a hit still reads as a hit. `damp` scales with the FP blend so the transition is seamless.
    let damp = 1.0 - 0.75 * fpb;
    if let Some(fb) = &feedback {
        let s = crate::combat_fx::SHAKE_MAX * fb.trauma * fb.trauma * damp;
        if s > 0.0 {
            let t = time.elapsed_secs();
            let chaos = Vec3::new((t * 47.0).sin(), (t * 59.0).sin(), (t * 41.0).sin());
            // Directional bias: a hit site sets `shake_dir` (world XZ) so the camera kicks ALONG
            // it — a swing's `-fwd` reads as recoil. Mix mostly the directed push with some chaos
            // so it still feels organic, not a clean slide. Zero dir → the old unbiased shake.
            let jitter = if fb.shake_dir.length_squared() > 1e-5 {
                let push = Vec3::new(fb.shake_dir.x, 0.0, fb.shake_dir.y) * (t * 53.0).sin();
                push * 0.7 + chaos * 0.5
            } else {
                chaos
            };
            cam_tf.translation += jitter * s;
        }
    }
    // FP close-quarters widen: ramp by how close the ringed foe stands (soft_pos already tracks
    // the engaged hostile). Slow ease both ways so a foe crossing the range can't pump the FOV.
    let want_close = if fp.active {
        hero.soft_pos.map_or(0.0, |tp| {
            ((FP_CLOSE_RANGE - tp.distance(hero.pos)) / FP_CLOSE_RANGE).clamp(0.0, 1.0)
        })
    } else {
        0.0
    };
    fp.close_fov +=
        (want_close * FP_CLOSE_FOV_DEG - fp.close_fov) * (1.0 - (-time.delta_secs() * 3.0).exp());

    // FOV = rest (captured once) + decaying combat punch (damped in FP) + a gentle sprint widen for a
    // sense of speed.
    if let Projection::Perspective(p) = &mut *cam_proj {
        let base = *base_fov.get_or_insert(p.fov);
        let kick = feedback.as_deref().map_or(0.0, |fb| fb.fov_kick) * damp;
        let speed_fov = hero.run_amt.clamp(0.0, 1.0) * SPRINT_FOV_DEG;
        // A slight wide-angle as the combat dolly pulls back, so a mob fight reads the arena.
        let combat_fov = orbit.combat * COMBAT_FOV_PER_UNIT;
        p.fov = base + (kick + speed_fov + combat_fov + fp.close_fov * fpb).to_radians();
    }
}
