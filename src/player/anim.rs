//! Hero limb animation, based on the user's three.js `updateKnightAnimation`
//! (low-poly-knight-studio): **idle / walk / run / jump /
//! defend / attack1 (overhead chop) / attack2 (horizontal slash) / attack3 (forward thrust) /
//! victory**. The studio builds each frame imperatively — reset every joint to rest, then a `switch`
//! case sets some — so we mirror that: [`rest`] seeds a full [`Pose`] table and each clip function
//! mutates the fields it touches. The per-frame system then writes the chosen pose onto the rig
//! joints.
//!
//! Game adaptations:
//! - **walk/jog/sprint** (the hero) are a procedural, foot-locked IK gait cut from the real ground
//!   speed (`Hero::gait_speed`; see [`footman_gait`]): the planted boot sweeps back exactly as fast
//!   as the body travels, the cadence follows from that ([`gait_phase_rate`]), and the style
//!   (walk → jog → sprint) is picked by speed. Other bipeds retain the shared studio walk/run clips.
//! - **clip changes** (take-off, landing, a swing starting, dash/charge) cross-fade from the last
//!   written pose instead of snapping ([`ClipBlend`]).
//! - **jump** — the studio faked the hop by sliding `hips.y`; here the **real jump physics own the
//!   height** (the root's world Y), so the studio `height` (0 launch/landing, 1 apex) is recovered
//!   from the hero's vertical speed and fed into the studio's exact airtime joint formulas.
//! - **attack** — our one-shot swing (`attack_t/ATTACK_DURATION`) is split into the studio's
//!   wind/strike/recovery phases (strike starts at `HIT_PHASE` so the blade meets the damage frame).
//! - **defend** — eased in/out by a smoothed `block_amt` instead of the studio's `time`-since-block.
//! On top sit our own layers: the Director's staged gestures (arms), the touchdown landing-squash,
//! and a slack keel-over on death. (First person doesn't use this rig at all — it is hidden and the
//! camera-parented `viewmodel` draws the hands.)

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use super::combat::{CHARGE_GRACE, CHARGE_THRESHOLD};
use super::{Hero, HeroHealth, HeroPart, Joint};

fn e3(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn ease_out_cubic(t: f32) -> f32 {
    let c = t.clamp(0.0, 1.0);
    1.0 - (1.0 - c).powi(3)
}
fn smoothstep(t: f32) -> f32 {
    let c = t.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}

/// Shield rest pose — turned EDGE-ON along the forearm when not blocking (previs look); the defend
/// clip swings it face-forward. (Studio idle was face-out; the user wants it sideways at rest.)
const SHIELD_REST_T: Vec3 = Vec3::new(-0.07, -0.08, 0.13);
fn shield_rest_r() -> Quat {
    e3(0.12, -1.5, 0.0)
}
/// Walk/run — held edge-on, a touch closer to the body.
const SHIELD_GAIT_T: Vec3 = Vec3::new(-0.1, -0.05, 0.15);
fn shield_gait_r() -> Quat {
    e3(0.12, -1.45, 0.0)
}
/// Sword rest X-rotation — a relaxed forward-down carry (~22° below horizontal). 1.2 ("tip
/// up-forward at the ready") held the blade near-horizontal out of a hanging fist, so the grip
/// buried itself in the vambrace and the blade visibly pierced the wrist; ~1.95 runs the grip
/// through the fist at a natural angle (pommel up behind the wrist, blade clear of the arm)
/// without reaching the studio's full 2.2 straight-down (which risks ground-clipping on the walk
/// backswing). Shared by the rest pose AND the attacks' wind-start / recovery-end so idle⇄attack
/// stays smooth.
pub(crate) const SWORD_REST_X: f32 = 1.95;
pub(crate) fn sword_rest_r() -> Quat {
    e3(SWORD_REST_X, 0.3, 0.0)
}

/// Three.js footman clips store joint eulers where **identity is the baked carry**. The mesh is
/// pre-rotated by the inverse of the game rest, so a three.js euler `e` becomes `e * rest` here
/// (and the shield's small rest translation is swung with it) and the blade/board move the way
/// they did in the source model.
fn posed_sword(ex: f32, ey: f32, ez: f32) -> Jp {
    Jp::r(m3(ex, ey, ez) * sword_rest_r())
}
fn posed_shield(ex: f32, ey: f32, ez: f32) -> Jp {
    let e = m3(ex, ey, ez);
    Jp { t: Some(e * SHIELD_REST_T), r: e * shield_rest_r() }
}
/// A three.js euler carried across the export's X mirror: X turns survive, Y and Z flip.
fn m3(ex: f32, ey: f32, ez: f32) -> Quat {
    e3(ex, -ey, -ez)
}

/// Landing squash recovery time (a crouch that decays over this many seconds after touchdown).
const LAND_RECOVER: f32 = 0.20;

/// One joint's target: an optional local translation override (`Some` only for joints the studio
/// moves — hips always, the shield, the hips-joints in victory) + a local rotation (always applied).
#[derive(Clone, Copy)]
pub(crate) struct Jp {
    pub(crate) t: Option<Vec3>,
    pub(crate) r: Quat,
}
impl Jp {
    fn r(r: Quat) -> Self {
        Jp { t: None, r }
    }
    fn lerp(self, o: Jp, s: f32) -> Jp {
        let t = match (self.t, o.t) {
            (Some(a), Some(b)) => Some(a.lerp(b, s)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        Jp { t, r: self.r.slerp(o.r, s) }
    }
}

/// A full rig pose (the studio sets joints imperatively onto one of these each frame).
#[derive(Clone, Copy)]
pub(crate) struct Pose {
    hips: Jp,
    torso: Jp,
    head: Jp,
    sh_l: Jp,
    sh_r: Jp,
    el_l: Jp,
    el_r: Jp,
    hip_l: Jp,
    hip_r: Jp,
    knee_l: Jp,
    knee_r: Jp,
    foot_l: Jp,
    foot_r: Jp,
    shield: Jp,
    sword: Jp,
}

impl Pose {
    pub(crate) fn get(&self, j: Joint) -> Jp {
        match j {
            Joint::Hips => self.hips,
            Joint::Torso => self.torso,
            Joint::Head => self.head,
            Joint::ShoulderL => self.sh_l,
            Joint::ShoulderR => self.sh_r,
            Joint::ElbowL => self.el_l,
            Joint::ElbowR => self.el_r,
            Joint::HipL => self.hip_l,
            Joint::HipR => self.hip_r,
            Joint::KneeL => self.knee_l,
            Joint::KneeR => self.knee_r,
            Joint::FootL => self.foot_l,
            Joint::FootR => self.foot_r,
            Joint::Shield => self.shield,
            Joint::Sword => self.sword,
        }
    }
    fn lerp(&self, o: &Pose, s: f32) -> Pose {
        Pose {
            hips: self.hips.lerp(o.hips, s),
            torso: self.torso.lerp(o.torso, s),
            head: self.head.lerp(o.head, s),
            sh_l: self.sh_l.lerp(o.sh_l, s),
            sh_r: self.sh_r.lerp(o.sh_r, s),
            el_l: self.el_l.lerp(o.el_l, s),
            el_r: self.el_r.lerp(o.el_r, s),
            hip_l: self.hip_l.lerp(o.hip_l, s),
            hip_r: self.hip_r.lerp(o.hip_r, s),
            knee_l: self.knee_l.lerp(o.knee_l, s),
            knee_r: self.knee_r.lerp(o.knee_r, s),
            foot_l: self.foot_l.lerp(o.foot_l, s),
            foot_r: self.foot_r.lerp(o.foot_r, s),
            shield: self.shield.lerp(o.shield, s),
            sword: self.sword.lerp(o.sword, s),
        }
    }
}

/// The studio reset block (every clip starts here): hips at 1.05, all rotations zero, the shield at
/// its idle pose, the sword at its held rest.
fn rest() -> Pose {
    let id = Jp::r(Quat::IDENTITY);
    Pose {
        hips: Jp { t: Some(Vec3::new(0.0, 1.05, 0.0)), r: Quat::IDENTITY },
        torso: id,
        head: id,
        sh_l: id,
        sh_r: id,
        el_l: id,
        el_r: id,
        hip_l: id,
        hip_r: id,
        knee_l: id,
        knee_r: id,
        foot_l: id,
        foot_r: id,
        shield: Jp { t: Some(SHIELD_REST_T), r: shield_rest_r() },
        sword: Jp::r(sword_rest_r()),
    }
}

// ── Locomotion clips ───────────────────────────────────────────────────────────────────
/// A studio-rig biped (orks, townsfolk) at rest: the measured idle ([`idle_sample`]) toned down for
/// a rig without leg IK — the hips drift only a few cm (the knees take it up) while the trunk
/// sways and the head looks round as recorded.
fn idle_pose(t: f32) -> Pose {
    let s = idle_sample(t);
    let breath = (t * 2.2).sin();
    let c11 = (t * 1.1).cos();
    // The weight across the feet, −1..1.
    let w = (s.lat / 0.75).clamp(-1.0, 1.0);
    let mut p = rest();
    let pelvis = e3(0.0, 0.5 * s.yaw, s.roll);
    p.hips = Jp { t: Some(Vec3::new(w * 0.03, 1.05 + breath * 0.004 - w.abs() * 0.008, 0.0)), r: pelvis };
    let trunk = Quat::from_rotation_y(0.5 * s.sh_yaw) * e3(0.7 * s.lean + breath * 0.01, 0.0, -0.7 * s.side);
    p.torso = Jp::r(pelvis.inverse() * trunk);
    p.head = Jp::r(trunk.inverse() * Quat::from_rotation_y(0.7 * s.head_yaw) * rx(0.4 * s.head_pitch - breath * 0.015));
    // The footman's bind pose IS the standing carry (arms already clear of the torso). Idle only
    // breathes — the old +0.1 / −0.4 elbow bend was compensating for a straight-down mesh.
    p.sh_l = Jp::r(e3(breath * 0.035, 0.0, c11 * 0.012 + w * 0.01));
    p.el_l = Jp::r(rx(-0.05 - breath * 0.02));
    p.sh_r = Jp::r(e3(breath * 0.035, 0.0, -c11 * 0.012 + w * 0.01));
    p.el_r = Jp::r(rx(-0.04 - breath * 0.015));
    // Stance leg straightens, free leg softens at the knee as the weight rides across.
    p.hip_l = Jp::r(pelvis.inverse() * e3(0.0, 0.0, w.max(0.0) * 0.05));
    p.hip_r = Jp::r(pelvis.inverse() * e3(0.0, 0.0, w.min(0.0) * 0.05));
    p.knee_l = Jp::r(rx((-w).max(0.0) * 0.08));
    p.knee_r = Jp::r(rx(w.max(0.0) * 0.08));
    p
}

// ── The hero at rest: motion-captured ──────────────────────────────────────────────────
// A man standing at ease (CMU 139_02, 7.8 s, sampled at 10 Hz, each channel relative to his mean
// stance): the weight drifting from foot to foot, the hips rolling and turning with it, the trunk
// swaying over them and the head looking round now and then. Body frame = his planted feet.

/// Samples per second in the `IDLE_*` tables.
const IDLE_HZ: f32 = 10.0;
/// Over the last this-many seconds of each pass the sideways channels swing over to the mirror
/// image, which the next pass plays (so the loop runs 15.6 s before it repeats).
const IDLE_FADE: f32 = 1.5;
/// The hips over the right foot (+1 = one half foot-spacing), forward of the feet (m), and down (m).
const IDLE_LAT: [f32; 78] = [
    -0.023, -0.026, -0.037, -0.022, -0.011, -0.013, -0.022, -0.034, -0.035, -0.027, -0.004, 0.028, 0.052, 0.061,
    0.039, -0.022, -0.097, -0.165, -0.214, -0.235, -0.229, -0.194, -0.140, -0.076, -0.009, 0.057, 0.129, 0.079,
    -0.072, -0.270, -0.466, -0.582, -0.564, -0.438, -0.270, -0.103, 0.034, 0.134, 0.201, 0.239, 0.238, 0.171, 0.062,
    -0.056, -0.176, -0.294, -0.415, -0.490, -0.471, -0.358, -0.172, 0.038, 0.225, 0.354, 0.428, 0.472, 0.523, 0.613,
    0.718, 0.745, 0.629, 0.415, 0.194, 0.013, -0.116, -0.200, -0.249, -0.268, -0.250, -0.184, -0.070, 0.078, 0.207,
    0.284, 0.282, 0.218, 0.138, 0.056,
];
const IDLE_FWD: [f32; 78] = [
    0.008, 0.002, 0.002, -0.001, -0.001, 0.000, 0.001, 0.003, 0.006, 0.009, 0.012, 0.013, 0.012, 0.011, 0.009,
    0.004, 0.001, -0.001, 0.000, 0.002, 0.005, 0.010, 0.014, 0.018, 0.021, 0.023, 0.015, 0.006, 0.000, -0.004,
    -0.003, 0.002, 0.007, 0.007, 0.002, -0.005, -0.008, -0.009, -0.013, -0.017, -0.019, -0.021, -0.021, -0.016,
    -0.009, -0.003, 0.003, 0.006, 0.005, 0.002, -0.002, -0.002, 0.002, 0.007, 0.010, 0.008, 0.004, -0.001, -0.009,
    -0.012, -0.016, -0.025, -0.028, -0.026, -0.024, -0.019, -0.012, -0.006, 0.000, 0.002, 0.002, 0.002, 0.005,
    0.006, 0.007, 0.008, 0.008, 0.009,
];
const IDLE_DIP: [f32; 78] = [
    0.002, 0.004, 0.004, 0.004, 0.004, 0.004, 0.004, 0.004, 0.003, 0.003, 0.003, 0.002, 0.003, 0.003, 0.003, 0.004,
    0.005, 0.005, 0.005, 0.005, 0.004, 0.004, 0.004, 0.003, 0.002, 0.002, 0.002, 0.000, -0.002, -0.003, -0.004,
    -0.005, -0.005, -0.003, -0.001, 0.000, 0.001, 0.002, 0.001, 0.001, 0.001, 0.001, 0.000, -0.001, -0.002, -0.002,
    -0.002, -0.002, -0.003, -0.005, -0.006, -0.005, -0.003, -0.002, -0.003, -0.005, -0.007, -0.009, -0.009, -0.007,
    -0.008, -0.007, -0.004, 0.000, 0.001, 0.002, 0.002, 0.002, 0.002, 0.002, 0.000, -0.001, -0.001, 0.000, 0.000,
    0.000, 0.001, 0.002,
];
/// Pelvis roll (+ = right hip up) and yaw (+ = right hip back), deg.
const IDLE_ROLL: [f32; 78] = [
    0.7, 0.8, 0.2, 0.5, 0.6, 0.4, 0.4, 0.2, 0.0, -0.3, -0.5, -0.4, 0.3, 0.1, -0.3, 0.1, 0.0, -0.1, -0.2, -0.3, -0.8,
    -0.5, -0.4, -0.5, -0.4, -0.4, 0.6, 0.8, 1.6, 2.2, 1.8, 0.7, 0.3, 0.7, 0.9, 1.2, 0.7, 0.4, 0.3, 0.2, 0.0, 0.0,
    0.4, 0.6, 0.6, 0.2, -0.4, -0.7, -0.8, -1.3, -1.5, -1.2, -0.9, -0.8, -0.9, -1.5, -1.9, -2.4, -2.9, -2.9, -1.6,
    -0.2, 0.6, 1.3, 1.7, 2.0, 2.2, 2.2, 1.6, 0.8, 0.1, -0.4, -0.8, -0.9, -0.7, -0.3, 0.0, 0.5,
];
const IDLE_YAW: [f32; 78] = [
    4.4, 4.9, 4.2, 4.0, 3.8, 3.6, 3.7, 3.8, 3.6, 3.0, 2.0, 1.0, -0.2, -1.2, -1.3, -0.6, 0.5, 2.5, 4.4, 6.2, 7.2,
    7.2, 6.5, 5.1, 3.4, 1.6, 0.4, 0.2, 1.7, 4.2, 6.2, 6.6, 5.6, 3.9, 1.5, -0.4, -2.0, -3.2, -4.0, -4.7, -4.9, -4.0,
    -1.9, 0.6, 3.2, 5.2, 6.5, 6.7, 5.2, 2.2, -1.3, -4.3, -6.8, -7.8, -7.9, -8.0, -8.4, -9.2, -11.2, -11.6, -10.0,
    -7.4, -4.4, -1.7, 0.1, 0.9, 1.1, 1.4, 1.6, 0.9, -0.8, -3.3, -5.4, -6.3, -5.4, -3.2, -0.5, 2.1,
];
/// Trunk lean (+ = forward), side bend (+ = top right) and shoulder-line yaw (+ = right shoulder
/// back), deg.
const IDLE_LEAN: [f32; 78] = [
    -0.6, 0.1, 0.2, 0.5, 0.4, 0.1, -0.3, -0.8, -1.6, -2.5, -2.8, -2.8, -2.5, -2.0, -1.4, 0.0, 1.5, 1.8, 0.8, 0.1,
    -0.1, -0.4, -1.2, -2.0, -2.9, -3.5, -2.8, -1.6, -0.4, 0.7, 0.8, -0.2, -0.9, -0.4, 0.9, 2.2, 2.8, 2.6, 2.2, 1.8,
    1.4, 1.1, 0.3, -1.0, -2.1, -2.5, -2.7, -2.8, -2.6, -2.1, -1.7, -1.3, -1.0, -1.2, -1.7, -1.7, -1.3, -0.7, 0.1,
    0.3, 0.7, 2.6, 4.5, 5.2, 5.4, 5.0, 4.0, 3.0, 2.0, 1.3, 1.1, 1.0, 0.6, 0.4, 0.3, -0.2, -0.6, -0.5,
];
const IDLE_SIDE: [f32; 78] = [
    -3.4, -3.3, -3.0, -3.1, -3.2, -3.1, -3.2, -3.2, -2.6, -1.3, -0.3, 0.6, 1.7, 2.5, 2.9, 2.6, 1.7, 0.8, -0.3, -0.9,
    -1.2, -1.4, -1.6, -1.7, -1.7, -1.7, -1.5, 0.0, 1.0, 0.4, -0.5, -0.7, -0.8, -0.8, -0.6, -0.1, 0.5, 1.3, 2.2, 3.0,
    3.4, 3.3, 2.6, 1.4, -0.4, -2.2, -3.3, -3.5, -3.7, -3.8, -3.4, -1.8, -0.9, -0.2, 1.1, 2.6, 4.1, 5.6, 7.1, 7.3,
    6.4, 5.1, 3.5, 2.2, 1.1, 0.3, -0.2, -0.4, -0.3, -0.2, 0.0, 0.1, 0.1, -0.3, -1.0, -1.6, -2.4, -3.3,
];
const IDLE_SH_YAW: [f32; 78] = [
    6.2, 6.3, 6.3, 5.9, 5.3, 4.9, 5.0, 5.0, 4.8, 4.1, 2.9, 1.3, -0.4, -1.7, -2.4, -2.3, -1.6, 0.1, 2.4, 5.1, 7.0,
    7.6, 7.4, 6.2, 4.7, 3.2, 1.9, 1.7, 2.9, 3.8, 4.3, 3.9, 2.4, 0.9, -0.9, -2.3, -3.2, -3.8, -4.3, -4.7, -4.7, -3.6,
    -1.8, 0.3, 2.6, 4.5, 5.4, 5.4, 4.0, 1.4, -1.9, -4.9, -7.0, -7.4, -6.8, -6.6, -6.6, -7.5, -9.6, -10.3, -8.9,
    -6.8, -4.3, -1.9, -0.7, -0.6, -0.7, -0.5, 0.0, 0.0, -1.1, -3.0, -4.7, -5.3, -4.2, -1.8, 1.2, 4.1,
];
/// Gaze yaw (+ = looking right) and pitch (+ = looking down), deg.
const IDLE_HEAD_YAW: [f32; 78] = [
    34.1, 33.7, 33.5, 33.1, 32.5, 32.0, 31.4, 31.1, 24.7, 10.6, 0.4, -9.0, -18.7, -27.6, -33.7, -38.3, -40.7, -33.1,
    -13.4, 3.5, 13.9, 20.5, 23.2, 23.7, 23.2, 22.3, 21.3, 19.5, 12.4, 3.4, -1.1, -2.8, -5.0, -9.0, -13.9, -18.9,
    -22.4, -25.1, -27.2, -28.3, -24.3, -16.8, -10.8, -5.6, 5.3, 18.0, 23.7, 25.4, 25.6, 24.7, 16.7, -1.2, -8.7,
    -9.3, -11.6, -17.0, -20.8, -23.2, -26.1, -26.9, -25.0, -22.5, -19.0, -17.4, -18.4, -19.1, -16.1, -11.4, -7.3,
    -4.4, -2.0, 0.3, 2.6, 8.0, 17.1, 22.8, 27.2, 31.9,
];
const IDLE_HEAD_PITCH: [f32; 78] = [
    -9.9, -9.8, -9.7, -9.5, -10.1, -10.4, -10.6, -10.4, -6.5, 0.0, 2.5, 4.6, 6.1, 6.4, 6.5, 8.2, 9.6, 9.3, 9.5, 9.3,
    7.3, 2.4, -2.5, -6.2, -8.4, -9.6, -10.2, -10.0, -7.9, -4.9, -2.6, -1.2, 1.2, 6.1, 11.6, 15.8, 17.7, 18.1, 17.7,
    16.6, 14.0, 8.9, 3.5, -0.5, -5.1, -10.5, -14.0, -15.6, -16.0, -15.4, -12.0, -7.6, -6.5, -6.0, -1.3, 6.3, 8.9,
    9.0, 9.2, 8.5, 8.8, 9.6, 9.5, 10.1, 10.7, 9.1, 3.8, -1.5, -5.0, -5.8, -3.9, -2.1, -1.7, -3.6, -6.7, -8.7, -9.6,
    -9.8,
];

/// Uniform periodic Catmull-Rom through `v` at sample position `x`.
fn loop_curve(v: &[f32], x: f32) -> f32 {
    let n = v.len() as isize;
    let i = x.floor() as isize;
    let t = x - x.floor();
    let at = |j: isize| v[j.rem_euclid(n) as usize];
    let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    0.5 * (2.0 * p1 + (p2 - p0) * t + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t * t + (3.0 * (p1 - p2) + p3 - p0) * t * t * t)
}

/// One instant of the measured idle: `lat` in half foot-spacings, `fwd`/`dip` in metres, the rest
/// in radians, the sideways channels already turned for the pass (see [`idle_sample`]).
struct IdleSample {
    lat: f32,
    fwd: f32,
    dip: f32,
    roll: f32,
    yaw: f32,
    lean: f32,
    side: f32,
    sh_yaw: f32,
    head_yaw: f32,
    head_pitch: f32,
}

/// The measured idle at time `t` (s). Sideways channels play as recorded on even passes and as
/// their mirror image on odd ones, crossing over smoothly through the centre.
fn idle_sample(t: f32) -> IdleSample {
    let period = IDLE_LAT.len() as f32 / IDLE_HZ;
    let pass = (t / period).floor();
    let local = t - pass * period;
    let side = if (pass as i64).rem_euclid(2) == 0 { 1.0 } else { -1.0 };
    let flip = side * (PI * smoothstep((local - (period - IDLE_FADE)) / IDLE_FADE)).cos();
    let x = local * IDLE_HZ;
    let ch = |v: &[f32]| loop_curve(v, x);
    let deg = |v: &[f32]| loop_curve(v, x).to_radians();
    IdleSample {
        lat: flip * ch(&IDLE_LAT),
        fwd: ch(&IDLE_FWD),
        dip: ch(&IDLE_DIP),
        roll: flip * deg(&IDLE_ROLL),
        yaw: flip * deg(&IDLE_YAW),
        lean: deg(&IDLE_LEAN),
        side: flip * deg(&IDLE_SIDE),
        sh_yaw: flip * deg(&IDLE_SH_YAW),
        head_yaw: flip * deg(&IDLE_HEAD_YAW),
        head_pitch: deg(&IDLE_HEAD_PITCH),
    }
}

/// The footman hero standing at rest at time `t` (s): the measured idle above over the breathing,
/// the boots planted where the rest pose puts them (the legs are solved to them, so the hips can
/// drift and roll freely).
fn footman_idle_pose(t: f32) -> Pose {
    let rig = super::footman::leg_rig();
    let s = idle_sample(t);
    let breath = (t * 2.2).sin();

    let mut p = rest();
    // ── Pelvis: the weight rides across over the feet; the hips roll and turn with it ──
    let pelvis = e3(0.0, 0.6 * s.yaw, s.roll);
    let half = rig.hip.x.abs();
    let mut hips = Vec3::new(0.85 * half * s.lat, super::model::HIP_REST_Y + s.dip + 0.004 * breath, s.fwd);
    // Each boot stays where the rest pose stands it: flat, under its hip joint.
    let (upper, lower) = (rig.knee.y.hypot(rig.knee.z), rig.foot.y.hypot(rig.foot.z));
    let reach = 0.995 * (upper + lower);
    let foot_of = |rest_hip: Vec3| rest_hip + Vec3::new(0.0, super::model::HIP_REST_Y, 0.0) + Vec3::new(0.0, rig.knee.y + rig.foot.y, rig.knee.z + rig.foot.z);
    let hip_rest = [rig.hip, Vec3::new(-rig.hip.x, rig.hip.y, rig.hip.z)];
    // Never stretch a leg past its reach: the hips settle onto the straighter (loaded) leg and the
    // other one softens at the knee.
    let smin = |a: f32, b: f32| {
        let k = 0.01;
        let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
        lerp(b, a, h) - k * h * (1.0 - h)
    };
    for r in hip_rest {
        let h = pelvis * r;
        let d = foot_of(r) - (hips + h);
        let cap = foot_of(r).y + (reach * reach - d.x * d.x - d.z * d.z).max(0.0).sqrt() - h.y;
        hips.y = smin(hips.y, cap);
    }
    let leg = |r: Vec3| {
        let h = hips + pelvis * r;
        let d = foot_of(r) - h;
        // Abduct the leg plane onto the boot (as the gait does against its sway) and solve the
        // knee in it; the boot is turned back flat on the ground.
        let abduct = Quat::from_rotation_z((d.x / d.x.hypot(d.y)).clamp(-0.5, 0.5).asin());
        let (th, kn, _) = leg_ik(Vec2::ZERO, Vec2::new(d.z, -d.y.hypot(d.x)), 0.0, &rig);
        let flat = (abduct * rx(th + kn)).inverse();
        (Jp::r(pelvis.inverse() * abduct * rx(th)), Jp::r(rx(kn)), Jp::r(flat))
    };
    (p.hip_l, p.knee_l, p.foot_l) = leg(hip_rest[0]);
    (p.hip_r, p.knee_r, p.foot_r) = leg(hip_rest[1]);
    // The writer re-adds HIP_REST_Y − 1.05 (legacy 1.05-hip clips); store in that convention.
    p.hips = Jp { t: Some(hips + Vec3::Y * (1.05 - super::model::HIP_REST_Y)), r: pelvis };

    // ── Trunk and head: swaying over the hips, the chest rising with each breath; the gaze
    // wanders (a little less far, and less often down, than the subject's) ──
    let bend = e3(s.lean + 0.01 * breath, 0.0, -s.side);
    let trunk = Quat::from_rotation_y(0.6 * s.sh_yaw) * bend;
    p.torso = Jp::r(pelvis.inverse() * trunk);
    let gaze = Quat::from_rotation_y(0.75 * s.head_yaw) * rx(0.5 * s.head_pitch - 0.015 * breath);
    p.head = Jp::r(trunk.inverse() * gaze);

    // ── Arms: the bind pose IS the standing carry; they hang plumb as the trunk bends (and
    // breathe) ──
    let hang = bend.inverse().slerp(Quat::IDENTITY, 0.2);
    p.sh_l = Jp::r(hang * e3(breath * 0.035, 0.0, 0.012 * (t * 1.1).cos()));
    p.el_l = Jp::r(rx(-0.05 - breath * 0.02));
    p.sh_r = Jp::r(hang * e3(breath * 0.035, 0.0, -0.012 * (t * 1.1).cos()));
    p.el_r = Jp::r(rx(-0.04 - breath * 0.015));
    p
}

fn walk_pose(c: f32) -> Pose {
    let l = c;
    let r = c + PI;
    let torso_x = 0.05 + (c * 2.0).sin() * 0.02;
    let mut p = rest();
    // A confident, decisive march — more arm swing and torso commitment than the studio's restrained
    // values, while the legs carry a solid stride. 2026-07 humanization: real lateral weight
    // transfer (the pelvis rides over the planted foot and ROLLS with it), a torso that
    // counter-rolls, and a head that stays level — the counter-motions are what read as "person",
    // not "piston".
    p.hips = Jp {
        t: Some(Vec3::new(c.sin() * 0.032, 1.04 + (c * 2.0).sin() * 0.028, 0.0)),
        r: e3(0.0, c.cos() * 0.08, c.sin() * 0.045),
    };
    p.torso = Jp::r(e3(torso_x, -c.cos() * 0.11, -c.sin() * 0.035));
    p.head = Jp::r(e3(-torso_x * 0.5, c.cos() * 0.035, -c.sin() * 0.012));
    p.hip_l = Jp::r(rx(l.sin() * 0.6));
    p.hip_r = Jp::r(rx(r.sin() * 0.6));
    p.knee_l = Jp::r(rx((-l.cos()).max(0.0) * 1.1));
    p.knee_r = Jp::r(rx((-r.cos()).max(0.0) * 1.1));
    // Foot roll with a toe-off flick at the back of the stride (the +0.25 kick as the leg trails).
    p.foot_l = Jp::r(rx(-l.sin() * 0.6 + (-l.sin()).max(0.0) * 0.25));
    p.foot_r = Jp::r(rx(-r.sin() * 0.6 + (-r.sin()).max(0.0) * 0.25));
    // The footman's arms are already in the carry. These are the three.js walk deltas off that
    // bind (a few degrees of counter-swing), not the old straight-arm knight bends — those lifted
    // the blade into a lance and flared the shield.
    p.sh_l = Jp::r(m3(r.sin() * 0.11, 0.0, 0.04));
    p.el_l = Jp::r(rx(-0.30 + r.sin().min(0.0) * 0.08));
    p.sh_r = Jp::r(m3(l.sin() * 0.16, 0.0, -0.04));
    p.el_r = Jp::r(rx(-0.22 + l.sin().min(0.0) * 0.11));
    p
}

fn run_pose(c: f32) -> Pose {
    let l = c;
    let r = c + PI;
    let absin = c.sin().abs();
    let mut p = rest();
    // A purposeful armored run — bigger, more committed stride than the studio's restrained jog
    // (but not the old frantic sprint): real lean, driving arms, knees that lift.
    p.hips = Jp {
        t: Some(Vec3::new(c.sin() * 0.02, 1.0 + absin * 0.06, 0.0)),
        r: e3(0.0, c.cos() * 0.1, c.sin() * 0.04),
    };
    p.torso = Jp::r(e3(0.18 + absin * 0.04, -c.cos() * 0.12, -c.sin() * 0.035));
    p.head = Jp::r(e3(-0.1 - absin * 0.04, c.cos() * 0.04, -c.sin() * 0.01));
    p.hip_l = Jp::r(e3(l.sin() * 0.8, 0.0, l.sin().max(0.0) * 0.02));
    p.hip_r = Jp::r(e3(r.sin() * 0.8, 0.0, -r.sin().max(0.0) * 0.02));
    p.knee_l = Jp::r(rx((-l.cos()).max(0.0) * 1.3 + 0.12));
    p.knee_r = Jp::r(rx((-r.cos()).max(0.0) * 1.3 + 0.12));
    p.foot_l = Jp::r(rx(-l.sin() * 0.8 * 0.5 + 0.14));
    p.foot_r = Jp::r(rx(-r.sin() * 0.8 * 0.5 + 0.14));
    // Three.js run: elbows tuck, the blade rides point-up over the shoulder, the shield stays
    // edge-on against the forearm. Eulers are identity-at-carry, composed onto the game rest.
    p.sh_l = Jp::r(m3(r.sin() * 0.15, 0.0, 0.14));
    p.el_l = Jp::r(rx(-1.15 + r.sin().min(0.0) * 0.2));
    p.sh_r = Jp::r(m3(l.sin() * 0.12, 0.0, -0.14));
    p.el_r = Jp::r(rx(-1.25 + l.sin().min(0.0) * 0.15));
    p.sword = posed_sword(-2.046, -0.52, 0.119);
    p.shield = posed_shield(0.924, 0.415, -0.121);
    p
}

/// idle → (walk → run by `run`) → blended toward idle by `m` (moving_amt).
pub(crate) fn loco_pose(t: f32, wp: f32, m: f32, run: f32) -> Pose {
    let gait = walk_pose(wp).lerp(&run_pose(wp), run);
    idle_pose(t).lerp(&gait, m)
}

// ── Hero (footman) gait: foot-locked, speed-driven ─────────────────────────────────────────
// The old footman gait swept the planted boot only ±0.18 m while the body travelled ~2 m per
// step at the hero's base speed — the feet covered under a fifth of the ground the body did, which
// is the "moonwalk". Here the gait is cut FROM the ground speed: the stance foot sweeps back
// exactly as fast as the body moves forward (so it stays nailed to the ground), and the cadence is
// derived from that same contract ([`gait_phase_rate`]), so any speed — haste, swamp, roads, a
// heavy-strike creep — stays foot-locked with no retuning.
//
// Speed also picks the gait STYLE: a walk (long stance, double support, a vaulting hip) at low
// speed, a jog (short stance, a flight phase, knees that drive) at the hero's base speed, and a
// sprint (longer reach, high heel kick, deep lean, pumping arms) with Shift. NB the base speed
// (`movement::SPEED`, ≈4.5 model-m/s) is jogging pace for a ~1.9 m figure — a foot-locked walk there
// would need ~5 steps/s — so the base gait IS a jog; the walk shows at the slower speeds.
//
// Leg cycle `u` (0..1, left = walk_phase/2π, right half a cycle later): heel strike at `u = 0`
// (so the footstep SFX/dust, keyed on walk_phase crossing kπ, land on contacts), stance until
// `duty`, then the swing.

/// Shape of the gait at one ground speed (model metres / s).
#[derive(Clone, Copy)]
struct Gait {
    /// Stance travel of the planted boot, strike → toe-off, relative to the hips (m).
    sweep: f32,
    /// Stance share of one leg cycle (walk > 0.5 = double support; run < 0.5 = flight).
    duty: f32,
    /// The walk's stride amplitude (1 = the recorded stride; slow steps are shorter).
    walk_amp: f32,
    /// 0 walk → 1 jog, and 0 jog → 1 sprint (style weights for the upper body / bob).
    run: f32,
    sprint: f32,
}

fn gait_at(v: f32) -> Gait {
    let run = smoothstep((v - 2.0) / 1.7);
    let sprint = smoothstep((v - 4.4) / 1.9);
    // Short creeping steps when barely moving, the full recorded stride by a brisk walk.
    let walk_amp = lerp(0.6, 1.0, smoothstep(v / 1.6));
    Gait {
        // Both shapes come from their recorded curves: the natural contact sweep, and a duty
        // factor from a walk's ~0.64 through a jog's ~1/3 to a sprint's quarter.
        sweep: lerp(run_sweep(Style::walk(walk_amp)), run_sweep(Style::run(sprint)), run),
        duty: lerp(WALK.to, lerp(JOG.to, SPRINT.to, sprint), run),
        walk_amp,
        run,
        sprint,
    }
}

/// World units per footman model metre (rig scale × `HERO_SCALE`).
pub(crate) fn hero_model_scale() -> f32 {
    super::footman::leg_rig().scale * super::HERO_SCALE
}

/// `walk_phase` advance rate (rad/s) at a world ground speed — the foot-lock contract: a stance of
/// `duty` of the cycle must carry the planted ankle back through `sweep` at exactly the body's speed.
pub(crate) fn gait_phase_rate(world_speed: f32) -> f32 {
    if mixamo_gait() {
        let v = world_speed.max(0.0) / hero_model_scale();
        if v < 1e-4 {
            return 0.0;
        }
        // All gaits start on the left heel strike, so one phase drives any of them; with the curve
        // run (`FOREST_RUN=curves`) its rate is the Mixamo walk's below a jog and the curves' above
        // (each foot-locked on its own).
        return lerp(mixamo::phase_rate(v), gait_phase_rate_curves(world_speed), mixamo::run_weight(v));
    }
    gait_phase_rate_curves(world_speed)
}

/// [`gait_phase_rate`] of the recorded-curve gait engine.
fn gait_phase_rate_curves(world_speed: f32) -> f32 {
    let v = world_speed.max(0.0) / hero_model_scale();
    if v < 1e-4 {
        return 0.0;
    }
    let g = gait_at(v);
    std::f32::consts::TAU * v * g.duty / g.sweep
}

/// Ankle (z, height) for a boot whose flat-foot ankle is at `z`, lowest point `clear` above the
/// ground, pitched by `pitch` — rolling about the toe tip (toe down) or the heel (toe up), so a
/// rocker keeps its contact edge planted instead of sinking/skating.
fn boot_ankle(z: f32, clear: f32, pitch: f32, rig: &super::footman::LegRig) -> (f32, f32) {
    let (h, (s, c)) = (rig.ankle_height, pitch.sin_cos());
    if pitch >= 0.0 {
        (z + rig.toe + h * s - rig.toe * c, clear + h * c + rig.toe * s)
    } else {
        (z - rig.heel + h * s + rig.heel * c, clear + h * c - rig.heel * s)
    }
}

/// Two-bone sagittal IK for one leg. `hip` = hip joint (z, height), `ankle` = target. Returns the
/// (hip, knee, foot) X rotations that land the ankle there with the boot at `pitch` (relative to
/// the pelvis-independent leg frame).
fn leg_ik(hip: Vec2, ankle: Vec2, pitch: f32, rig: &super::footman::LegRig) -> (f32, f32, f32) {
    let (thigh, shin) = leg_dirs(hip, ankle, rig);
    leg_rot(thigh, shin, pitch, rig)
}

/// Forward angle from straight down toward +Z (the sagittal bone direction the gait works in).
fn fwd_angle(v: Vec2) -> f32 {
    v.x.atan2(-v.y)
}

/// The (thigh, shin) forward angles that land the ankle on `ankle` from the hip joint `hip`.
fn leg_dirs(hip: Vec2, ankle: Vec2, rig: &super::footman::LegRig) -> (f32, f32) {
    let upper = rig.knee.y.hypot(rig.knee.z);
    let lower = rig.foot.y.hypot(rig.foot.z);
    let d = ankle - hip;
    let dist = d.length().clamp((upper - lower).abs() + 1e-3, upper + lower - 1e-3);
    let open = ((upper * upper + dist * dist - lower * lower) / (2.0 * upper * dist)).clamp(-1.0, 1.0).acos();
    let thigh_dir = fwd_angle(d) + open; // knee ahead of the hip→ankle line
    let knee_pt = hip + upper * Vec2::new(thigh_dir.sin(), -thigh_dir.cos());
    (thigh_dir, fwd_angle(hip + d.normalize_or_zero() * dist - knee_pt))
}

/// Bone forward angles → the hip/knee/foot X rotations (`rx(θ)` turns a bone by −θ in that
/// angle), with the boot at world `pitch`.
fn leg_rot(thigh_dir: f32, shin_dir: f32, pitch: f32, rig: &super::footman::LegRig) -> (f32, f32, f32) {
    let thigh_rest = fwd_angle(Vec2::new(rig.knee.z, rig.knee.y));
    let shin_rest = fwd_angle(Vec2::new(rig.foot.z, rig.foot.y));
    let th = thigh_rest - thigh_dir;
    let kn = shin_rest - th - shin_dir;
    (th, kn, pitch - th - kn)
}

/// The hero's foot-locked locomotion at world ground speed `speed`, cycle phase `c`: the recorded
/// walk below a jog, the recorded run above it, cross-faded between ([`mocap_gait`]).
/// `armed` (0..1): the sword is in hand (1) or slung on the back (0); `shield_back` (0..1): the
/// shield is on the arm (0) or slung on the back for a sprint (1), when both arms run free.
fn footman_gait(c: f32, speed: f32, armed: f32, shield_back: f32) -> Pose {
    if mixamo_gait() {
        let w = mixamo::run_weight(speed.max(0.0) / hero_model_scale());
        if w >= 1.0 - 1e-3 {
            return footman_gait_curves(c, speed, armed, shield_back);
        }
        let walk = mixamo::gait(c, speed.max(0.0) / hero_model_scale(), armed, shield_back);
        if w <= 1e-3 {
            return walk;
        }
        return walk.lerp(&footman_gait_curves(c, speed, armed, shield_back), w);
    }
    footman_gait_curves(c, speed, armed, shield_back)
}

/// Mixamo clips drive the walk, the run and the dodge roll (the default; the jump stays on the
/// hand-built curves, and `FOREST_RUN=curves` puts the run back on them); `FOREST_GAIT=curves` brings back the curve walk and the procedural roll (A/B, and what
/// the curve tests exercise).
fn mixamo_gait() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_GAIT").map(|v| v != "curves").unwrap_or(true))
}

/// The recorded-curve gait engine (see the section below).
fn footman_gait_curves(c: f32, speed: f32, armed: f32, shield_back: f32) -> Pose {
    let g = gait_at(speed.max(0.0) / hero_model_scale());
    let (walk, run) = (Style::walk(g.walk_amp), Style::run(g.sprint));
    let play = |st: Style, pelvis| mocap_gait(c, &g, armed, shield_back, st, pelvis);
    if g.run <= 1e-3 {
        play(walk, style_pelvis(c, g.duty, walk))
    } else if g.run >= 1.0 - 1e-3 {
        play(run, style_pelvis(c, g.duty, run))
    } else {
        // Both styles stand on one pelvis, so the cross-fade can't swing the planted boots
        // sideways (two differently rolled pelves blended joint by joint did).
        let (a, b) = (style_pelvis(c, g.duty, walk), style_pelvis(c, g.duty, run));
        let pelvis = (lerp(a.0, b.0, g.run), a.1.slerp(b.1, g.run));
        play(walk, pelvis).lerp(&play(run, pelvis), g.run)
    }
}

// ── Run: motion-captured joint curves, foot-locked by a stance time-warp ──────────────────
// Redone from scratch (user, 2026-10), then rebuilt on real motion capture when the hand-keyed
// curves still read wrong. Instead of solving the legs from a foot path and keying
// the swing, every joint follows measured running curves (global thigh angle, knee flexion,
// ankle plantar-flexion over the gait cycle), so the stride has the shape of a real run: a soft
// landing just ahead of the hips, the knee loading through mid-stance, a full push-off, the heel
// folding up UNDER the body as the thigh drives forward, and the leg reaching out again before
// contact. Ground contact stays exact without bending the curves: during stance they play on a
// WARPED clock chosen so the planted boot's contact edge slides back at exactly the body's speed,
// and the hips ride at whatever height that planted leg puts them, so the bob (lowest at
// mid-stance, highest in flight) falls out of the leg instead of a sine. In flight the hips arc
// from toe-off to the next landing.

/// One style's curves over a leg cycle (fraction from initial contact; toe-off at `to`): global
/// thigh angle (deg forward of vertical), knee flexion (deg), ankle plantar-flexion (deg, minus =
/// dorsiflexion; only the swing uses it, the stance pitch is the boot rolling on the ground).
struct RunCurves {
    to: f32,
    p: &'static [f32],
    thigh: &'static [f32],
    knee: &'static [f32],
    ankle: &'static [f32],
    /// What a shortened stride closes toward: the thigh's mid-swing and the knee's straightest.
    thigh_mid: f32,
    knee_min: f32,
}

/// The motion-capture tables' sample points: 25 evenly over one cycle.
const RUN_P: [f32; 25] = [
    0.0, 0.04, 0.08, 0.12, 0.16, 0.20, 0.24, 0.28, 0.32, 0.36, 0.40, 0.44, 0.48, 0.52, 0.56, 0.60, 0.64, 0.68, 0.72,
    0.76, 0.80, 0.84, 0.88, 0.92, 0.96,
];

/// A run at 3.56 m/s, measured: the mean of three CMU motion-capture runs (subjects 09 and 35),
/// one right-leg cycle each, contact to contact. The real shape the hand-keyed tables missed: a
/// push-off with the thigh ~29 deg behind, the knee loading to ~47 in the stance and folding to
/// ~109 in the swing while the thigh is already forward (the heel tucks under the hips, it does
/// not kick out behind), and the drive to ~39.
const JOG: RunCurves = RunCurves {
    to: 0.322,
    p: &RUN_P,
    thigh_mid: 5.0,
    knee_min: 12.0,
    thigh: &[
        27.4, 27.3, 24.1, 17.8, 9.0, -0.2, -9.6, -18.8, -28.1, -29.2, -26.3, -21.8, -15.0, -7.3, 2.3, 14.2, 25.1,
        33.3, 37.7, 39.4, 38.9, 36.0, 32.4, 29.3, 27.1,
    ],
    knee: &[
        28.3, 39.7, 46.3, 47.3, 44.1, 38.3, 31.4, 22.3, 12.3, 18.1, 31.6, 46.8, 64.3, 80.9, 95.5, 105.9, 109.2,
        105.4, 96.0, 82.0, 65.2, 46.9, 30.2, 20.7, 19.4,
    ],
    ankle: &[
        -8.9, -15.3, -22.9, -30.1, -35.1, -32.4, -22.7, -0.5, 19.9, 27.7, 23.5, 23.5, 25.2, 24.0, 19.4, 18.1, 10.8,
        4.9, 0.2, -5.0, -9.5, -11.2, -11.1, -11.6, -9.8,
    ],
};

/// The hero's sprint (~4.7 m/s): the measured run morphed — the stance cut to a quarter of the
/// cycle, the thigh driving ~15 deg higher (to ~57), the push-off a little less far behind, the
/// stance knee stiffer and the swing fold ~10 % deeper, still peaking with the thigh forward.
const SPRINT: RunCurves = RunCurves {
    to: 0.25,
    p: &RUN_P,
    thigh_mid: 15.0,
    knee_min: 14.0,
    thigh: &[
        31.0, 29.4, 22.7, 11.1, -1.5, -12.6, -24.1, -27.1, -25.4, -22.2, -17.4, -11.3, -4.1, 5.7, 18.9, 31.9, 43.3,
        51.4, 56.3, 57.5, 54.8, 48.9, 42.2, 36.3, 32.1,
    ],
    knee: &[
        26.1, 37.9, 42.4, 40.1, 33.8, 25.5, 13.9, 15.6, 26.3, 39.8, 55.5, 72.8, 89.9, 105.5, 116.7, 119.9, 115.1,
        104.6, 90.2, 74.0, 56.9, 40.2, 26.7, 19.8, 19.8,
    ],
    ankle: &[
        -8.9, -17.4, -27.2, -34.8, -31.4, -13.7, 15.3, 26.8, 25.3, 23.1, 24.3, 25.1, 23.0, 19.1, 17.8, 11.1, 5.6,
        1.3, -3.3, -7.8, -10.6, -11.3, -11.3, -11.4, -9.6,
    ],
};

// The same runners' upper body over the right leg's cycle (0 = right contact, at `RUN_P`), deg,
// against the heading. Left-side curves are these half a cycle on, mirrored.
/// Pelvis yaw (+ = right hip back), obliquity (+ = right hip up), tilt (+ = top forward).
const RUN_PEL_YAW: [f32; 25] = [
    4.5, 5.8, 5.6, 4.2, 2.9, 3.2, 2.6, 0.5, -1.0, -1.8, -1.8, -2.7, -3.8, -5.3, -5.9, -5.0, -3.2, -3.1, -3.1, -1.6,
    0.3, 1.5, 1.9, 2.1, 3.3,
];
const RUN_PEL_OBL: [f32; 25] = [
    -2.6, -0.7, 2.5, 3.7, 4.0, 4.4, 4.1, 3.4, 3.2, 3.9, 4.9, 4.8, 3.4, 1.8, -0.8, -3.5, -3.7, -4.3, -4.2, -3.8,
    -3.1, -3.5, -4.5, -5.0, -4.2,
];
const RUN_PEL_TILT: [f32; 25] = [
    3.4, 2.3, 2.5, 3.2, 3.7, 4.3, 4.9, 5.1, 5.2, 5.6, 5.4, 4.6, 3.7, 2.7, 2.2, 2.9, 3.5, 4.0, 4.6, 5.0, 5.2, 5.4,
    5.6, 5.1, 4.0,
];
/// Shoulder-line yaw (+ = right shoulder back), trunk lean (+ = forward), side bend (+ = top right).
const RUN_SH_YAW: [f32; 25] = [
    10.0, 8.9, 7.9, 6.3, 3.2, -0.8, -4.2, -6.4, -8.1, -9.3, -9.6, -9.6, -9.9, -9.5, -8.4, -7.3, -4.9, -1.2, 2.6,
    5.4, 7.3, 8.8, 9.5, 9.6, 9.6,
];
const RUN_LEAN: [f32; 25] = [
    12.4, 12.9, 14.0, 14.8, 14.9, 14.5, 13.5, 12.4, 11.2, 10.6, 10.6, 10.8, 11.7, 12.5, 13.4, 14.4, 14.9, 14.8,
    14.1, 12.9, 11.8, 10.8, 10.6, 10.7, 10.9,
];
const RUN_SIDE: [f32; 25] = [
    2.4, 2.8, 3.4, 3.4, 3.0, 2.3, 1.8, 1.1, 0.5, -0.1, -0.5, -1.2, -2.0, -2.6, -3.1, -3.5, -3.3, -2.7, -2.1, -1.5,
    -0.8, -0.2, 0.3, 0.8, 1.6,
];
/// The right arm in the trunk frame: upper-arm flexion (+ = elbow ahead of the shoulder; it never
/// gets much ahead — a runner's arm swings from beside the body to well behind it), abduction
/// (+ = out), the elbow's bend (93-121 deg: the forearm rides forward and up), the forearm's turn
/// out of the sagittal plane (− = across toward the midline).
const RUN_ARM_FLEX: [f32; 25] = [
    -24.8, -17.7, -8.5, 0.1, 5.0, 6.9, 6.8, 5.8, 3.8, 1.9, -0.3, -2.7, -5.6, -8.4, -10.9, -14.2, -17.8, -21.1,
    -24.7, -28.3, -31.8, -34.4, -35.0, -33.7, -30.4,
];
const RUN_ARM_ABD: [f32; 25] = [
    6.1, 7.4, 8.9, 9.7, 10.1, 10.4, 11.1, 11.7, 12.2, 12.6, 12.4, 11.5, 10.7, 9.7, 7.8, 6.0, 5.6, 6.5, 7.2, 6.9,
    6.2, 5.5, 4.6, 4.1, 4.9,
];
const RUN_ARM_ELBOW: [f32; 25] = [
    100.8, 102.3, 103.5, 105.1, 108.8, 113.2, 117.0, 119.5, 121.0, 120.8, 118.9, 115.2, 109.9, 103.5, 97.6, 93.4,
    92.8, 94.4, 96.3, 97.4, 98.6, 100.2, 101.5, 102.5, 102.8,
];
const RUN_ARM_FORE_OUT: [f32; 25] = [
    -5.0, -8.0, -11.8, -14.7, -15.6, -15.8, -16.4, -17.5, -18.7, -20.0, -20.5, -19.7, -17.5, -14.9, -11.9, -9.1,
    -8.4, -9.5, -10.0, -8.9, -7.0, -5.3, -4.2, -3.8, -4.5,
];
/// Middle and half-range of [`RUN_ARM_FLEX`] (deg): the swing's centre and its ±1 reach.
const RUN_ARM_MID: f32 = -14.0;
const RUN_ARM_HALF: f32 = 21.0;

/// A walk at 1.54 m/s, measured: the mean of six CMU motion-capture cycles (subject 08), both legs
/// (the left mirrored), gait events by the heel / toe excursion from the pelvis (Zeni 2008, the
/// clips' floor drifts). This subject walks with a soft knee: its ~37 deg loading hump is flattened
/// to ~29 here (the rig's long shin would show it as a crouch).
const WALK: RunCurves = RunCurves {
    to: 0.65,
    p: &RUN_P,
    thigh: &[
        35.1, 35.2, 36.1, 35.7, 31.7, 26.2, 20.6, 15.4, 10.5, 5.1, -1.1, -8.7, -17.1, -22.0, -24.1, -17.9, -9.7,
        -0.8, 8.5, 16.8, 23.7, 29.1, 31.8, 32.3, 33.3,
    ],
    knee: &[
        17.5, 21.5, 24.7, 28.7, 29.6, 28.5, 27.0, 25.7, 24.6, 23.9, 23.1, 16.9, 8.2, 7.1, 11.6, 29.0, 46.2, 60.4,
        69.3, 72.0, 69.7, 62.3, 49.2, 33.0, 21.4,
    ],
    ankle: &[
        -22.5, -18.6, -7.0, -7.2, -10.8, -14.0, -16.7, -18.5, -21.5, -21.3, -18.9, -14.0, -8.7, -6.9, -2.1, 11.6,
        19.8, 15.8, 2.0, -7.5, -13.2, -19.7, -23.6, -22.7, -24.7,
    ],
    thigh_mid: 6.0,
    knee_min: 7.0,
};
const WALK_PEL_YAW: [f32; 25] = [
    -7.9, -8.0, -7.2, -6.0, -5.8, -5.4, -4.2, -2.9, -1.1, 0.8, 3.1, 5.4, 7.5, 8.1, 7.7, 6.6, 5.8, 5.8, 4.8, 3.6, 2.1,
    0.2, -1.9, -4.2, -6.5,
];
const WALK_PEL_OBL: [f32; 25] = [
    -5.7, -5.6, -4.2, -2.5, -0.5, 1.2, 1.7, 2.1, 3.0, 3.4, 3.8, 4.4, 5.3, 5.8, 5.0, 3.4, 1.5, -0.5, -1.6, -1.9,
    -2.5, -3.3, -3.6, -4.0, -4.8,
];
const WALK_PEL_TILT: [f32; 25] = [
    7.5, 7.1, 7.2, 6.4, 5.5, 5.1, 5.0, 4.9, 4.8, 5.0, 5.4, 6.5, 7.5, 7.3, 7.1, 6.9, 5.9, 5.2, 5.0, 5.0, 4.8, 4.9,
    5.2, 5.9, 7.1,
];
const WALK_SH_YAW: [f32; 25] = [
    0.3, 0.9, 1.7, 2.1, 1.3, -0.2, -1.0, -1.4, -1.4, -1.0, -0.4, 0.2, 0.2, -0.5, -1.3, -2.0, -1.9, -0.5, 0.7, 1.3,
    1.4, 1.3, 0.7, 0.1, -0.5,
];
const WALK_LEAN: [f32; 25] = [
    1.6, 1.5, 0.9, 0.8, 1.1, 1.2, 1.0, 0.6, 0.4, 0.5, 0.8, 1.0, 1.4, 1.6, 1.2, 0.8, 1.0, 1.2, 1.1, 0.8, 0.4, 0.4,
    0.7, 0.9, 1.1,
];
const WALK_SIDE: [f32; 25] = [
    -1.4, -0.3, 1.2, 2.6, 3.1, 3.1, 2.8, 2.6, 2.4, 2.3, 2.3, 2.1, 1.7, 0.9, -0.4, -2.0, -2.9, -3.1, -2.9, -2.7,
    -2.5, -2.3, -2.3, -2.2, -2.0,
];
const WALK_BOB: [f32; 25] = [
    -0.5, -1.3, -1.6, -1.5, -0.8, 0.1, 0.9, 1.3, 1.1, 1.0, 0.8, 0.5, -0.1, -0.9, -1.5, -1.6, -1.2, -0.4, 0.5, 1.2,
    1.3, 1.1, 0.9, 0.6, 0.3,
];
const WALK_ARM_FLEX: [f32; 25] = [
    -28.9, -30.4, -30.0, -27.5, -23.6, -17.6, -10.8, -4.5, 1.3, 7.4, 13.9, 19.6, 24.9, 27.4, 25.1, 19.8, 14.1, 9.2,
    4.5, -0.6, -5.5, -10.0, -14.6, -19.5, -24.4,
];
const WALK_ARM_ABD: [f32; 25] = [
    -7.4, -7.1, -5.8, -4.2, -3.4, -2.9, -2.1, -1.0, 0.5, 2.4, 4.4, 6.0, 7.8, 9.2, 9.7, 8.7, 7.1, 5.0, 3.1, 1.3,
    -0.5, -2.0, -3.5, -5.1, -6.7,
];
const WALK_ARM_ELBOW: [f32; 25] = [
    13.3, 12.2, 11.2, 11.0, 11.8, 12.1, 13.6, 17.4, 23.9, 30.8, 36.8, 42.0, 45.8, 49.3, 53.3, 56.7, 57.8, 55.1,
    48.8, 40.5, 31.3, 23.0, 17.2, 14.2, 13.3,
];
const WALK_ARM_FORE_OUT: [f32; 25] = [
    3.2, 3.8, 4.5, 5.7, 6.5, 7.0, 7.7, 7.6, 7.0, 5.6, 3.3, 1.0, -0.4, -0.6, -0.2, 0.5, 0.4, -0.5, -0.8, 0.1, 1.4,
    2.6, 3.1, 3.1, 2.7,
];

/// One style's recorded upper body and hip bob over the reference (right) leg's cycle (see the
/// `RUN_*` tables for the conventions), with the soldier's carriage laid on top.
struct Upper {
    pel_yaw: &'static [f32; 25],
    pel_obl: &'static [f32; 25],
    pel_tilt: &'static [f32; 25],
    sh_yaw: &'static [f32; 25],
    lean: &'static [f32; 25],
    side: &'static [f32; 25],
    bob: &'static [f32; 25],
    arm_flex: &'static [f32; 25],
    arm_abd: &'static [f32; 25],
    arm_elbow: &'static [f32; 25],
    arm_fore_out: &'static [f32; 25],
    /// Middle and half-range of `arm_flex` (deg): the swing's centre and its ±1 reach.
    arm_mid: f32,
    arm_half: f32,
    /// Scale of the recorded arm swing and elbow bend: the walk's subject swings his arms about
    /// half again as far as an average walker (and a man carrying arms swings less still).
    swing: f32,
    elbow: f32,
    /// Extra trunk lean and arm abduction (deg): an armoured man's carriage — a little more
    /// forward set than a walker in a T-shirt, elbows clear of the breastplate and tassets.
    lean_add: f32,
    abd_add: f32,
    /// Pelvis side-sway over the stance foot (m, peak).
    sway: f32,
}

const RUN_UPPER: Upper = Upper {
    pel_yaw: &RUN_PEL_YAW,
    pel_obl: &RUN_PEL_OBL,
    pel_tilt: &RUN_PEL_TILT,
    sh_yaw: &RUN_SH_YAW,
    lean: &RUN_LEAN,
    side: &RUN_SIDE,
    bob: &RUN_BOB,
    arm_flex: &RUN_ARM_FLEX,
    arm_abd: &RUN_ARM_ABD,
    arm_elbow: &RUN_ARM_ELBOW,
    arm_fore_out: &RUN_ARM_FORE_OUT,
    arm_mid: RUN_ARM_MID,
    arm_half: RUN_ARM_HALF,
    swing: 1.0,
    elbow: 1.0,
    lean_add: 0.0,
    abd_add: 0.0,
    sway: 0.01,
};

const WALK_UPPER: Upper = Upper {
    pel_yaw: &WALK_PEL_YAW,
    pel_obl: &WALK_PEL_OBL,
    pel_tilt: &WALK_PEL_TILT,
    sh_yaw: &WALK_SH_YAW,
    lean: &WALK_LEAN,
    side: &WALK_SIDE,
    bob: &WALK_BOB,
    arm_flex: &WALK_ARM_FLEX,
    arm_abd: &WALK_ARM_ABD,
    arm_elbow: &WALK_ARM_ELBOW,
    arm_fore_out: &WALK_ARM_FORE_OUT,
    arm_mid: -1.5,
    arm_half: 29.0,
    swing: 0.65,
    elbow: 0.7,
    lean_add: 3.0,
    abd_add: 12.0,
    sway: 0.02,
};

/// How far the swing leg is retargeted from the recorded joint angles to the ankle path (see
/// [`swing_retarget`]): a run's deep fold needs it most, a walk's shallow one a little.
const RUN_RETARGET: f32 = 0.6;
const WALK_RETARGET: f32 = 0.8;

/// A gait style the recorded-curve engine plays: two leg-curve sets blended by `mix` (jog →
/// sprint), the stride amplitude `amp` (< 1 closes the stride toward the curves' middle for
/// short slow steps), the swing retarget and the upper body.
#[derive(Clone, Copy)]
struct Style {
    a: &'static RunCurves,
    b: &'static RunCurves,
    mix: f32,
    amp: f32,
    retarget: f32,
    upper: &'static Upper,
}

impl Style {
    fn run(sprint: f32) -> Self {
        Style { a: &JOG, b: &SPRINT, mix: sprint, amp: 1.0, retarget: RUN_RETARGET, upper: &RUN_UPPER }
    }

    fn walk(amp: f32) -> Self {
        Style { a: &WALK, b: &WALK, mix: 0.0, amp, retarget: WALK_RETARGET, upper: &WALK_UPPER }
    }

    /// A walk: the stances overlap (double support) instead of being split by a flight.
    fn walking(&self) -> bool {
        self.a.to >= 0.5
    }

    /// The recorded clock (on the `a` curves' timing) at the reference leg's cycle `u` with
    /// stance share `duty`. A run warps each step (stance and flight separately, so a shorter
    /// sprint contact keeps the upper body in time with the feet); a walk, whose stances overlap,
    /// warps the leg's whole cycle.
    fn clock(&self, u: f32, duty: f32) -> f32 {
        let to = self.a.to;
        let u = u.rem_euclid(1.0);
        if self.walking() {
            let duty = duty.clamp(0.3, 0.9);
            return if u < duty { u * to / duty } else { to + (u - duty) * (1.0 - to) / (1.0 - duty) };
        }
        let duty = duty.clamp(0.05, 0.45);
        let (half, q) = if u < 0.5 { (0.0, u) } else { (0.5, u - 0.5) };
        half + if q < duty { q * to / duty } else { to + (q - duty) * (0.5 - to) / (0.5 - duty) }
    }
}

/// Shoulder and elbow rotations that lay an arm's upper arm along `upper` (torso frame) with the
/// forearm bent `bend` rad off it, turned toward `toward`. `bind` = (elbow from the shoulder, hand
/// from the elbow) at rest; the elbow hinges on its X axis and the shoulder carries the rest.
fn aim_arm(upper: Vec3, toward: Vec3, bend: f32, bind: (Vec3, Vec3)) -> (Quat, Quat) {
    let (u0, f0) = (bind.0.normalize(), bind.1.normalize());
    // The hinge angle (flexing = −) that opens the bind forearm to `bend` off the upper arm.
    let (a, b) = (u0.y * f0.y + u0.z * f0.z, u0.z * f0.y - u0.y * f0.z);
    let open = ((bend.cos() - u0.x * f0.x) / a.hypot(b).max(1e-5)).clamp(-1.0, 1.0).acos();
    let elbow = rx(b.atan2(a) - open);
    let frame = |u: Vec3, f: Vec3| {
        let n = u.cross(f).normalize_or_zero();
        Mat3::from_cols(u, n.cross(u), n)
    };
    let du = upper.normalize();
    let w = (toward - toward.dot(du) * du).normalize_or_zero();
    let to = frame(du, bend.cos() * du + bend.sin() * w);
    (Quat::from_mat3(&(to * frame(u0, elbow * f0).transpose())), elbow)
}

/// Periodic Catmull-Rom through keys `(p[i], v[i])` on a unit cycle (non-uniform spacing).
fn cycle_curve(p: &[f32], v: &[f32], x: f32) -> f32 {
    let n = p.len() as isize;
    let x = x.rem_euclid(1.0);
    let i = p.iter().rposition(|&k| k <= x).unwrap_or(0) as isize;
    // Key `j` of the endless cycle: wraps round, its position shifted by whole cycles.
    let key = |j: isize| {
        let w = j.rem_euclid(n) as usize;
        (p[w] + j.div_euclid(n) as f32, v[w])
    };
    let (pa, va) = key(i - 1);
    let (p0, v0) = key(i);
    let (p1, v1) = key(i + 1);
    let (pb, vb) = key(i + 2);
    let h = p1 - p0;
    let (m0, m1) = ((v1 - va) / (p1 - pa), (vb - v0) / (pb - p0));
    let t = ((x - p0) / h).clamp(0.0, 1.0);
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * v0 + (t3 - 2.0 * t2 + t) * h * m0 + (-2.0 * t3 + 3.0 * t2) * v1 + (t3 - t2) * h * m1
}

/// (thigh, knee flex, ankle) in radians for style `st`, at stance progress `s` (planted: 0 =
/// contact, 1 = toe-off) or swing progress `s` (0 = toe-off, 1 = the next contact).
fn run_angles(st: Style, stance: bool, s: f32) -> (f32, f32, f32) {
    // A shortened stride closes the thigh swing toward its middle and (less) the knee toward its
    // straightest, so the boot still clears the ground.
    let knee_amp = 0.5 + 0.5 * st.amp;
    let at = |c: &RunCurves| {
        let x = if stance { c.to * s } else { c.to + (1.0 - c.to) * s };
        let t = cycle_curve(c.p, c.thigh, x);
        let k = cycle_curve(c.p, c.knee, x);
        (c.thigh_mid + st.amp * (t - c.thigh_mid), c.knee_min + knee_amp * (k - c.knee_min), cycle_curve(c.p, c.ankle, x))
    };
    let (a, b) = (at(st.a), at(st.b));
    let m = st.mix;
    (lerp(a.0, b.0, m).to_radians(), lerp(a.1, b.1, m).to_radians(), lerp(a.2, b.2, m).to_radians())
}

/// A planted leg at (warped) stance progress `s`: (thigh, knee, boot pitch, flat-ankle z relative
/// to the hip joint, hip-joint height above the ground). The boot rolls from the landing pitch
/// (heel or mid-foot) to flat and up onto the toe for the push-off; the end pitches equal the
/// swing's (shin angle + ankle), so contact and lift-off hand over seamlessly.
fn run_stance(st: Style, s: f32, rig: &super::footman::LegRig) -> (f32, f32, f32, f32, f32) {
    let (t, k, _) = run_angles(st, true, s);
    let end_pitch = |e: f32| {
        let (t, k, a) = run_angles(st, true, e);
        -(t - k) + a
    };
    // A walker's heel peels late and fast (pre-swing), a runner's rolls up through the push-off.
    let rise = if st.walking() { ((s - 0.55) / 0.45).clamp(0.0, 1.0).powi(2) } else { smoothstep((s - 0.5) / 0.5) };
    let pitch = lerp(end_pitch(0.0), 0.0, smoothstep(s / 0.2)) + end_pitch(1.0) * rise;
    let ankle = leg_fk(t, t - k, rig);
    let (roll_z, ground_y) = boot_ankle(0.0, 0.0, pitch, rig);
    (t, k, pitch, ankle.x - roll_z, ground_y - ankle.y)
}

/// The ankle (z, height) relative to the hip joint for (thigh, shin) forward angles.
fn leg_fk(thigh: f32, shin: f32, rig: &super::footman::LegRig) -> Vec2 {
    let (upper, lower) = (rig.knee.y.hypot(rig.knee.z), rig.foot.y.hypot(rig.foot.z));
    Vec2::new(upper * thigh.sin() + lower * shin.sin(), -upper * thigh.cos() - lower * shin.cos())
}

/// A style's natural contact sweep (flat-ankle travel strike to toe-off, model m).
fn run_sweep(st: Style) -> f32 {
    let rig = super::footman::leg_rig();
    run_stance(st, 0.0, &rig).3 - run_stance(st, 1.0, &rig).3
}

/// The warped stance progress whose pose puts the planted boot at linear progress `s` of its
/// sweep, less `dz` (the hip joint's own fore-aft shift from the pelvis yaw). The flat-ankle z
/// falls monotonically through the stance, so a bisection is exact.
fn run_warp(st: Style, s: f32, dz: f32, rig: &super::footman::LegRig) -> f32 {
    let z0 = run_stance(st, 0.0, rig).3;
    let z1 = run_stance(st, 1.0, rig).3;
    let want = lerp(z0, z1, s.clamp(0.0, 1.0)) - dz;
    let (mut a, mut b) = (0.0_f32, 1.0_f32);
    for _ in 0..18 {
        let m = 0.5 * (a + b);
        if run_stance(st, m, rig).3 > want {
            a = m;
        } else {
            b = m;
        }
    }
    0.5 * (a + b)
}

/// The runners' measured hip bob (cm) over a step (0 = a landing; the cycle repeats it each
/// step): lowest a little after the landing as the knee loads, rising through the push-off,
/// cresting just after toe-off and falling into the next landing — ~8.5 cm in all.
const RUN_BOB: [f32; 25] = [
    0.0, -2.1, -3.8, -4.5, -3.9, -2.4, -0.4, 1.5, 3.0, 4.0, 4.0, 3.2, 1.3, -1.1, -3.0, -4.3, -4.4, -3.2, -1.4, 0.6,
    2.3, 3.6, 4.1, 3.7, 2.4,
];

/// Hip-joint height at gait cycle `u` with stance share `duty`: the measured bob from the leg's
/// own landing height, bent a little (zero at each landing) so it also meets the leg's own
/// toe-off height on this rig (its long rigid sabaton peels the hips higher than a person's
/// foot), so the landing and the lift-off both hand over to the swing curves exactly. (The old
/// flight arc matched the push-off's climb rate and threw the hips ~17 cm up.)
///
/// A walk instead pins the bob to the single stance's middle, where the planted leg stands at its
/// recorded angles: the people's legs are thigh = shin with a short foot, so on this rig the
/// recorded landing and toe-off heights disagree by several cm and matching them crouched the
/// whole stance. The swing hands over to the stance poses with a blend ([`mocap_gait`]).
fn run_hip_height(st: Style, duty: f32, u: f32, rig: &super::footman::LegRig) -> f32 {
    let own = |s: f32| run_stance(st, s, rig).4;
    let bob = |x: f32| 0.01 * (cycle_curve(&RUN_P, st.upper.bob, x) - st.upper.bob[0]);
    let x_to = st.a.to.rem_euclid(0.5);
    let x = st.clock(u, duty).rem_euclid(0.5);
    if st.walking() {
        let x_mid = 0.5 * (x_to + 0.5);
        return own(x_mid / st.a.to) + bob(x) - bob(x_mid);
    }
    let bend = |x: f32| (TAU * x).sin().powi(2);
    let (h0, h1) = (own(0.0), own(1.0));
    let fix = (h1 - h0 - bob(x_to)) / bend(x_to);
    h0 + bob(x) + fix * bend(x)
}

/// A swing leg's (thigh, shin) forward angles moved by `w` (0..1) from the recorded joint angles
/// toward the recorded ANKLE path: the people's legs are thigh = shin, this rig's shin is half
/// again its thigh, so the same knee fold fans the boot out far behind the hips (the heels-
/// kicking-back look); placing the ankle where a person's would be, scaled to this leg, tucks the
/// heel up under the body and carries the knee a little higher instead.
fn swing_retarget(thigh: f32, shin: f32, w: f32, rig: &super::footman::LegRig) -> (f32, f32) {
    if w <= 0.0 {
        return (thigh, shin);
    }
    let half = 0.5 * (rig.knee.y.hypot(rig.knee.z) + rig.foot.y.hypot(rig.foot.z));
    let ankle = half * Vec2::new(thigh.sin() + shin.sin(), -thigh.cos() - shin.cos());
    let (t, sh) = leg_dirs(Vec2::ZERO, ankle, rig);
    (lerp(thigh, t, w), lerp(shin, sh, w))
}

/// A style's measured pelvis at cycle phase `c`: (forward tilt, rotation) — tipped forward a
/// touch, the swing leg's hip carried forward with it, dropping on the swing side.
fn style_pelvis(c: f32, duty: f32, st: Style) -> (f32, Quat) {
    // The recorded upper body plays on the right leg's clock.
    let x_r = st.clock((c / TAU).rem_euclid(1.0) + 0.5, duty);
    let rec = |v: &[f32; 25]| cycle_curve(&RUN_P, v, x_r).to_radians();
    // A sprint twists the hips a little further.
    let tilt = rec(st.upper.pel_tilt);
    (tilt, e3(tilt, lerp(1.0, 1.2, st.mix) * rec(st.upper.pel_yaw), rec(st.upper.pel_obl)))
}

/// One recorded gait style played foot-locked: `c` cycle phase, `g` the gait, `st` the style,
/// `pelvis` its (tilt, rotation) from [`style_pelvis`] (or a blend of two styles').
/// `armed` (0..1): the sword is in hand (1) or slung on the back (0), an empty sword hand swings
/// as freely as the shield arm; `shield_back` (0..1) frees the shield arm the same way.
fn mocap_gait(c: f32, g: &Gait, armed: f32, shield_back: f32, st: Style, pelvis: (f32, Quat)) -> Pose {
    let rig = super::footman::leg_rig();
    let up = st.upper;
    let (sprint, duty) = (st.mix, g.duty);
    // A walk's stances overlap (double support), a run's are split by a flight; the walk also
    // carries the sword and shield the old way: hanging and held, not shouldered.
    let walk = st.walking();
    let r = if walk { 0.0 } else { 1.0 };
    let u_l = (c / TAU).rem_euclid(1.0);
    let u_r = (u_l + 0.5).fract();
    // Stance weight per leg (0 at contact / toe-off, 1 mid-stance) for the pelvis sway.
    let load = |u: f32| if u < duty { (PI * u / duty).sin() } else { 0.0 };
    let (kl, kr) = (load(u_l), load(u_r));
    // The recorded upper body plays on the right leg's clock; the left side half a cycle on.
    let x_r = st.clock(u_r, duty);
    let x_l = (x_r + 0.5).fract();
    let rec = |v: &[f32; 25], x: f32| cycle_curve(&RUN_P, v, x).to_radians();
    // A sprint twists the shoulders a little further.
    let twist = lerp(1.0, 1.2, sprint);

    let (tilt, pelvis) = pelvis;
    let hip_l = pelvis * rig.hip;
    let hip_r = pelvis * Vec3::new(-rig.hip.x, rig.hip.y, rig.hip.z);
    let hip_nominal = e3(tilt, 0.0, 0.0) * rig.hip;
    // The hips ride at the height the planted leg dictates (the hip joint sits at its pelvis
    // offset below the hips joint); in flight they follow the arc between the two stances. A walk
    // always has a leg down, so it holds the nominal joint height (no hand-over between the legs).
    let q = u_l.rem_euclid(0.5);
    let loaded = if q < duty && !walk { if u_l < 0.5 { hip_l } else { hip_r } } else { hip_nominal };
    let mut hips_y = run_hip_height(st, duty, u_l, &rig) - loaded.y;
    // The stance legs' poses: warped so each planted boot slides back at the body's speed.
    let planted = |off: Vec3, u: f32| run_stance(st, run_warp(st, u / duty, off.z - hip_nominal.z, &rig), &rig);
    if walk {
        // Never ask a planted leg for more reach than it has: the hips sink at the far ends of a
        // long stride instead of the boot peeling off the ground (soft-min keeps it smooth).
        let reach = 0.985 * (rig.knee.y.hypot(rig.knee.z) + rig.foot.y.hypot(rig.foot.z));
        let smin = |a: f32, b: f32| {
            let k = 0.03;
            let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
            lerp(b, a, h) - k * h * (1.0 - h)
        };
        for (off, u) in [(hip_l, u_l), (hip_r, u_r)] {
            if u < duty {
                let (t, k, _, _, own) = planted(off, u);
                let ankle = leg_fk(t, t - k, &rig);
                let cap = own + ankle.y + (reach * reach - ankle.x * ankle.x).max(0.0).sqrt() - off.y;
                hips_y = smin(hips_y, cap);
            }
        }
    }
    let sway = up.sway * (kr - kl);
    let hips_t = Vec3::new(sway, hips_y, 0.0);
    let mut p = rest();
    // The writer re-adds HIP_REST_Y − 1.05 (legacy 1.05-hip clips); store in that convention.
    p.hips = Jp { t: Some(hips_t + Vec3::Y * (1.05 - super::model::HIP_REST_Y)), r: pelvis };

    // ── Legs ──
    let leg = |off: Vec3, u: f32| {
        // The hip joint's height above the ground.
        let hold = hips_t.y + off.y;
        let (th, kn, ft) = if u < duty {
            let (t, k, pitch, _, own) = planted(off, u);
            // The hips ride the measured bob, not this pose's own height: same boot placement,
            // the knee gives or straightens the difference.
            leg_ik(Vec2::ZERO, leg_fk(t, t - k, &rig) + Vec2::Y * (own - hold), pitch, &rig)
        } else {
            let sw = (u - duty) / (1.0 - duty);
            let (t, k, a) = run_angles(st, false, sw);
            let (mut t, mut shin) = swing_retarget(t, t - k, st.retarget * smoothstep(sw / 0.3) * smoothstep((1.0 - sw) / 0.3), &rig);
            let mut pitch = -shin + a;
            if walk {
                // The walk's hips don't ride the leg's own toe-off / landing heights, so the swing
                // peels out of the stance pose it left and settles into the one it lands in.
                let stance_at = |u: f32| {
                    let (t, k, pitch, _, own) = planted(off, u);
                    let (th, sh) = leg_dirs(Vec2::ZERO, leg_fk(t, t - k, &rig) + Vec2::Y * (own - hold), &rig);
                    (th, sh, pitch)
                };
                let mix = |a: (f32, f32, f32), b: (f32, f32, f32), w: f32| (lerp(a.0, b.0, w), lerp(a.1, b.1, w), lerp(a.2, b.2, w));
                let lift = mix(stance_at(duty), (t, shin, pitch), smoothstep(sw / 0.3));
                (t, shin, pitch) = mix(lift, stance_at(0.0), smoothstep((sw - 0.8) / 0.2));
            }
            // Never through the ground: a boot the hips' height would sink is lifted onto it, the
            // knee folding a little more. A walker's swing skims a finger's width above it until
            // the heel comes down.
            let clear = if walk { 0.015 * smoothstep(sw / 0.15) * (1.0 - smoothstep((sw - 0.8) / 0.2)) } else { 0.0 };
            let ankle = leg_fk(t, shin, &rig);
            let floor = boot_ankle(0.0, clear, pitch, &rig).1 - hold;
            if ankle.y < floor { leg_ik(Vec2::ZERO, Vec2::new(ankle.x, floor), pitch, &rig) } else { leg_rot(t, shin, pitch, &rig) }
        };
        // Keep the boots on their track under the swaying pelvis: the sagittal IK ignores X, so
        // the sway (and the hip joint's own sideways shift as the pelvis rolls and yaws) would
        // carry a planted boot with it — abduct the leg against it so only the pelvis travels.
        let leg_len = hold.max(0.3);
        let drift = hips_t.x + off.x - rig.hip.x.abs() * off.x.signum();
        let abduct = Quat::from_rotation_z((-drift / leg_len).clamp(-0.5, 0.5).asin());
        (Jp::r(pelvis.inverse() * abduct * rx(th)), Jp::r(rx(kn)), Jp::r(rx(ft)))
    };
    (p.hip_l, p.knee_l, p.foot_l) = leg(hip_l, u_l);
    (p.hip_r, p.knee_r, p.foot_r) = leg(hip_r, u_r);

    // ── Trunk (measured): leaning into the speed, the shoulders counter-rotating the hips, a
    // little side bend over the stance; the head steady and level ──
    let lean = rec(up.lean, x_r) + up.lean_add.to_radians() + lerp(0.0, 0.045, sprint);
    let trunk = Quat::from_rotation_y(twist * rec(up.sh_yaw, x_r)) * e3(lean, 0.0, -rec(up.side, x_r));
    p.torso = Jp::r(pelvis.inverse() * trunk);
    p.head = Jp::r(trunk.inverse() * rx(0.04));

    // ── Arms (measured): a walker's arm swings near-straight from the shoulder, the elbow giving
    // more on the front swing; a runner's swings from beside the trunk to well behind it with the
    // elbow bent ~95-120 deg so the forearm rides forward and up, angled in toward the midline. A
    // sprint pumps wider and a touch tighter. ──
    let ab = super::footman::arm_bind();
    let pump = lerp(1.0, 1.35, sprint);
    let mid = up.arm_mid.to_radians();
    let free = |x: f32, side: f32, amp: f32, bind: (Vec3, Vec3)| {
        let flex = mid + lerp(0.0, 0.07, sprint) + up.swing * amp * pump * (rec(up.arm_flex, x) - mid);
        let abd = rec(up.arm_abd, x) + up.abd_add.to_radians();
        let out = rec(up.arm_fore_out, x);
        let upper = Vec3::new(side * abd.tan(), -1.0, flex.tan());
        aim_arm(upper, Vec3::new(side * out.sin(), 0.0, out.cos()), up.elbow * rec(up.arm_elbow, x) - 0.1 * sprint, bind)
    };
    // How far forward each arm is in its swing (+1 front, −1 back).
    let reach = |x: f32| (rec(up.arm_flex, x) - mid) / up.arm_half.to_radians();
    let (f_r, f_l) = (reach(x_r), reach(x_l));
    // Sword arm: free (the blade is slung), or with the sword in hand a shortened swing.
    let (sr, er) = free(x_r, 1.0, lerp(1.0, lerp(0.8, 0.7, r), armed), ab[0]);
    // Shield arm: a board on the forearm is carried, not pumped (small swing, elbow held); with the
    // shield slung on the back, the arm runs free like the other.
    let carry = if walk {
        // A relaxed half-swing with some give in the elbow (a dead-still arm read as stiff), the
        // elbow trailing the shoulder a beat.
        let f_late = reach(x_l - 0.04);
        (e3(0.04 - 0.22 * f_l, 0.0, 0.0), rx(lerp(-lerp(0.12, 0.45, 0.5 + 0.5 * f_late), -0.285, 0.2)))
    } else {
        let flex = -0.08 + 0.4 * lerp(0.56, 0.8, sprint) * f_l;
        let el_mid = lerp(0.55, 0.62, sprint);
        (e3(-flex, 0.22, -(0.07 - 0.088 * f_l.max(0.0))), rx(-(el_mid + 0.25 * lerp(0.22, 0.28, sprint) * f_l)))
    };
    let (sl_f, el_f) = free(x_l, -1.0, 1.0, ab[1]);
    let (sl, el) = (carry.0.slerp(sl_f, shield_back), carry.1.slerp(el_f, shield_back));
    // The shoulder girdle travels with its arm (forward + a hair up on the front swing, back on
    // the back swing) instead of the arm pivoting about a bolted-down socket.
    let girdle = |rest: Vec3, f: f32| rest + Vec3::new(0.0, lerp(0.006, 0.012, r) * f.max(0.0), lerp(0.015, 0.03, r) * f);
    p.sh_r = Jp { t: Some(girdle(rig.shoulder_r, f_r)), r: sr };
    p.el_r = Jp::r(er);
    p.sh_l = Jp { t: Some(girdle(rig.shoulder, f_l)), r: sl };
    p.el_l = Jp::r(el);
    // A running blade in hand rides shouldered (wrist against the pump) rather than fanning
    // behind; a walking one hangs at the held rest.
    let wrist = r * armed * 0.6 * lerp(0.56, 0.8, sprint) * f_r;
    p.sword = Jp::r(rx(wrist) * sword_rest_r().slerp(posed_sword(-2.046, -0.52, 0.119).r, r * armed));
    // The board stays upright, edge-on along the body: cancel the elbow's pitch on it.
    let (el_x, _, _) = el.to_euler(EulerRot::XYZ);
    let undo = rx(-el_x * lerp(0.4, 0.9, r));
    p.shield = Jp { t: Some(undo * SHIELD_GAIT_T), r: undo * shield_gait_r() };
    p
}

/// Preview/staging drivers (`FOREST_VIEW_ANIM`, `FOREST_ANIMTEST`, demos): play the gait at a
/// world ground speed exactly as movement would (foot-locked phase advance).
pub(crate) fn stage_gait(hero: &mut super::Hero, speed: f32, dt: f32) {
    hero.moving = true;
    hero.moving_amt = 1.0;
    hero.gait_speed = speed;
    hero.walk_phase += dt * gait_phase_rate(speed);
}

fn footman_loco_pose(t: f32, wp: f32, m: f32, speed: f32, armed: f32, shield_back: f32) -> Pose {
    footman_idle_pose(t).lerp(&footman_gait(wp, speed, armed, shield_back), m)
}

/// Footman combat-stance locomotion with two extra axes driven by `movement` —
/// `back` (0..1) cross-fades toward the gait played in REVERSE phase (a backpedal: the hero
/// steps backward while still facing the foe), and `twist` (radians) yaws the pelvis+legs
/// toward the movement while the torso/head counter-rotate to stay square on the target — the
/// classic lower-body-aims-along-movement / upper-body-faces-target split every lock-on game
/// uses, here as a differential yaw on the existing joints.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stance_loco_pose(t: f32, wp: f32, m: f32, speed: f32, back: f32, twist: f32, armed: f32, shield_back: f32) -> Pose {
    let mut p = footman_loco_pose(t, wp, m, speed, armed, shield_back);
    if back > 0.001 {
        // The same cycle run backward reads as stepping back; the mid-blend "gather step" as the
        // two phases cancel is exactly what a person does reversing direction.
        p = p.lerp(&footman_loco_pose(t, -wp, m, speed, armed, shield_back), back.clamp(0.0, 1.0));
    }
    if twist.abs() > 1e-3 {
        // Hips carry the legs AND the torso (rig: hips → torso, hips → hip_l/r), so yawing the
        // hips aims the whole lower body along the movement; the torso takes most of the counter
        // and the head the remainder, landing the eyes exactly back on the foe.
        p.hips.r = Quat::from_rotation_y(twist) * p.hips.r;
        p.torso.r = Quat::from_rotation_y(-twist * 0.85) * p.torso.r;
        p.head.r = Quat::from_rotation_y(-twist * 0.15) * p.head.r;
    }
    p
}

/// **Combat-guard overlay** — while the stance holds, the knight actually LOOKS ready to fight:
/// weight dropped into bent knees, torso crouched a touch forward, the shield raised into a
/// half-guard and the blade carried UP at the ready instead of trailing at rest (which also stops
/// the arms doing their casual walk-swing mid-fight). Blended over idle/walk/run by `amt`
/// (`hero.stance_amt`), so leaving combat melts back to the relaxed carry — and coming out of a
/// roll/attack flows straight into this ready pose. Attack/block/roll clips own their joints
/// wholesale past this point, so the overlay only colours locomotion.
fn guard_overlay(p: &mut Pose, amt: f32, moving: f32) {
    let a = amt.clamp(0.0, 1.0);
    if a <= 0.001 {
        return;
    }
    // Moving legs already have a solved support/recovery. Apply the extra guard crouch
    // only as movement settles; bending those knees again drove the supporting boot down.
    let lower_amt = a * (1.0 - moving.clamp(0.0, 1.0));
    if let Some(t) = p.hips.t {
        p.hips.t = Some(t - Vec3::new(0.0, 0.045 * lower_amt, 0.0));
    }
    p.hips.r = e3(0.05 * lower_amt, 0.0, 0.0) * p.hips.r;
    p.torso.r = e3(0.09 * a, 0.0, 0.0) * p.torso.r;
    // Eyes on the foe: the calm idle's wandering gaze settles straight ahead.
    p.head.r = p.head.r.slerp(Quat::IDENTITY, lower_amt);
    p.knee_l.r = p.knee_l.r * rx(0.22 * lower_amt);
    p.knee_r.r = p.knee_r.r * rx(0.20 * lower_amt);
    p.hip_l.r = p.hip_l.r * rx(-0.11 * lower_amt);
    p.hip_r.r = p.hip_r.r * rx(-0.10 * lower_amt);
    // Sword arm to a mid guard — forearm raised, blade angled up-forward at the ready.
    p.sh_r = p.sh_r.lerp(Jp::r(e3(-0.35, -0.1, 0.28)), a);
    p.el_r = p.el_r.lerp(Jp::r(rx(-1.15)), a);
    p.sword = p.sword.lerp(Jp::r(e3(1.15, 0.25, -0.1)), a);
    // Shield swings from the edge-on carry into a forward half-guard.
    p.sh_l = p.sh_l.lerp(Jp::r(e3(-0.45, 0.1, -0.35)), a);
    p.el_l = p.el_l.lerp(Jp::r(rx(-1.05)), a);
    p.shield = p.shield.lerp(Jp { t: Some(Vec3::new(-0.02, -0.02, 0.12)), r: e3(0.7, -0.8, 0.0) }, a);
}

fn blend_t(a: Option<Vec3>, b: Option<Vec3>, s: f32) -> Option<Vec3> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.lerp(y, s)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

/// Shield-block while still mobile: the torso/arms/shield brace by `block`, but the legs (and the
/// hip bob) only brace as the hero stands still (`1 - moving`) — so blocking *while walking* keeps
/// the legs striding instead of freezing them and sliding the body across the ground.
fn brace(loco: &Pose, d: &Pose, block: f32, moving: f32) -> Pose {
    let leg = block * (1.0 - moving);
    Pose {
        hips: Jp { t: blend_t(loco.hips.t, d.hips.t, leg), r: loco.hips.r.slerp(d.hips.r, block) },
        torso: loco.torso.lerp(d.torso, block),
        head: loco.head.lerp(d.head, block),
        sh_l: loco.sh_l.lerp(d.sh_l, block),
        sh_r: loco.sh_r.lerp(d.sh_r, block),
        el_l: loco.el_l.lerp(d.el_l, block),
        el_r: loco.el_r.lerp(d.el_r, block),
        hip_l: loco.hip_l.lerp(d.hip_l, leg),
        hip_r: loco.hip_r.lerp(d.hip_r, leg),
        knee_l: loco.knee_l.lerp(d.knee_l, leg),
        knee_r: loco.knee_r.lerp(d.knee_r, leg),
        foot_l: loco.foot_l.lerp(d.foot_l, leg),
        foot_r: loco.foot_r.lerp(d.foot_r, leg),
        shield: loco.shield.lerp(d.shield, block),
        sword: loco.sword.lerp(d.sword, block),
    }
}

/// An upper-body action (an attack) laid over locomotion legs: the action drives torso/head/arms/
/// sword/shield; the legs (and the hip bob) ease toward the walking/running gait by `leg`
/// (= moving_amt), so swinging mid-run keeps the legs striding (a "running attack") instead of
/// freezing into the attack's planted stance. Standing still → `leg≈0` → the full attack.
pub(crate) fn action_over_loco(action: &Pose, loco: &Pose, leg: f32) -> Pose {
    Pose {
        hips: Jp { t: blend_t(action.hips.t, loco.hips.t, leg), r: action.hips.r.slerp(loco.hips.r, leg) },
        torso: action.torso,
        head: action.head,
        sh_l: action.sh_l,
        sh_r: action.sh_r,
        el_l: action.el_l,
        el_r: action.el_r,
        hip_l: action.hip_l.lerp(loco.hip_l, leg),
        hip_r: action.hip_r.lerp(loco.hip_r, leg),
        knee_l: action.knee_l.lerp(loco.knee_l, leg),
        knee_r: action.knee_r.lerp(loco.knee_r, leg),
        foot_l: action.foot_l.lerp(loco.foot_l, leg),
        foot_r: action.foot_r.lerp(loco.foot_r, leg),
        shield: action.shield,
        sword: action.sword,
    }
}

/// A forward running leap (studio `jumpForward` feel) — blended over the plain vertical jump by how
/// fast the hero is moving. Now a real broad-jump arc: lead knee drives high at launch/apex then
/// **extends to plant** as you fall, while the trail leg scissors the other way — so the silhouette
/// changes through the jump instead of holding one stiff frame. `h` = apex closeness, `fall` ramps
/// 0→1 only on the descent (signed velocity), driving the landing reach.
fn leap_pose(vel_y: f32) -> Pose {
    let v = (vel_y / 6.5).clamp(-1.0, 1.0); // 6.5 = movement::JUMP_SPEED
    let h = (1.0 - v.abs()).clamp(0.0, 1.0);
    let fall = (-v).max(0.0); // 0 on the way up, 1 falling fast
    let mut p = rest();
    p.torso = Jp::r(e3(0.42 - fall * 0.2, 0.12, 0.0)); // deep forward dive, chest opens to spot the landing
    p.head = Jp::r(rx(-0.2 + fall * 0.3)); // chin tucks in flight, lifts to look down on descent
    // Lead (left) leg: knee tucks high through the climb, then drives down/forward to reach the ground.
    p.hip_l = Jp::r(rx(-0.95 + h * 0.2 + fall * 0.7));
    p.knee_l = Jp::r(rx(1.15 - fall * 0.95)); // tucked at apex, straightens to plant
    p.foot_l = Jp::r(rx(-0.35 + fall * 0.4));
    // Trail (right) leg: streams back hard at launch, sweeps forward under you as you fall (scissor).
    p.hip_r = Jp::r(rx(0.65 - fall * 0.55));
    p.knee_r = Jp::r(rx(0.3 + fall * 0.45));
    p.foot_r = Jp::r(rx(0.3 - fall * 0.2));
    // Arms pump the broad jump: lead arm reaches forward/up, trail arm drives back and opens on descent.
    p.sh_l = Jp::r(e3(-0.7 - h * 0.45, 0.0, -0.25));
    p.el_l = Jp::r(rx(-0.6));
    p.sh_r = Jp::r(e3(0.35 + fall * 0.25, 0.0, 0.3 + fall * 0.2));
    p.el_r = Jp::r(rx(-0.85 + h * 0.3));
    p
}

/// **Sand Dash** — the blink read as a swift, committed forward lunge with a flat sword swipe, so
/// the move *travels* instead of teleporting. `p` is 0→1 progress along the slide
/// (`hero.dash_t / movement::DASH_TIME`). A `lunge` envelope (0 at the ends, 1 mid-blink) drops the
/// hips and drives the legs into a speed-skater push; the sword arm whips a horizontal swipe across
/// the dash. The tail eases back toward locomotion in [`hero_anim`] so there's no snap on landing.
fn dash_pose(p: f32) -> Pose {
    let lunge = (p * PI).sin(); // deepest commitment mid-dash, settled at both ends
    let mut po = rest();
    po.hips = Jp { t: Some(Vec3::new(0.0, 1.0 - lunge * 0.13, lunge * 0.05)), r: e3(lunge * 0.12, 0.0, 0.0) };
    po.torso = Jp::r(e3(0.26 + lunge * 0.18, lerp(-0.32, 0.42, p), 0.0)); // pitch into the dash + twist through the swipe
    po.head = Jp::r(e3(-0.16, lerp(-0.16, 0.22, p), 0.0));
    // Lead (left) leg drives ahead, trail (right) leg streams back — a flat, low push.
    po.hip_l = Jp::r(rx(-0.72 - lunge * 0.22));
    po.knee_l = Jp::r(rx(0.5 + lunge * 0.3));
    po.foot_l = Jp::r(rx(-0.2));
    po.hip_r = Jp::r(rx(0.6 + lunge * 0.32));
    po.knee_r = Jp::r(rx(0.42));
    po.foot_r = Jp::r(rx(0.26));
    // Sword arm whips a flat horizontal swipe across the blink; off hand trails for balance.
    po.sh_r = Jp::r(e3(lerp(-0.2, -1.35, p), lerp(-0.7, 0.45, p), lerp(0.5, -0.3, p)));
    po.el_r = Jp::r(rx(lerp(-1.25, -0.3, p)));
    po.sword = Jp::r(e3(lerp(2.3, 2.5, p), lerp(0.7, -0.45, p), lerp(-0.55, 0.3, p)));
    po.sh_l = Jp::r(e3(-0.3 - lunge * 0.2, 0.0, -0.5));
    po.el_l = Jp::r(rx(-0.85));
    po.shield = Jp { t: Some(SHIELD_GAIT_T), r: shield_gait_r() };
    po
}

/// **Dodge roll** — the tucked-ball pose held through the somersault. The ROOT owns the actual
/// tumble (a full 2π pitch about the centre of mass, in `movement::player_move`); this just folds
/// the limbs into the tuck: chin down, knees hauled to the chest, arms wrapped in, shield/sword
/// pulled tight so nothing flails through the spin. Blended in/out by the roll envelope in
/// [`hero_anim`] so the stand-up flows back into locomotion.
fn roll_pose() -> Pose {
    let mut p = rest();
    p.hips = Jp { t: Some(Vec3::new(0.0, 0.74, 0.0)), r: rx(0.35) };
    p.torso = Jp::r(rx(1.0));
    p.head = Jp::r(rx(0.55)); // chin tucked
    p.hip_l = Jp::r(e3(-1.7, 0.0, 0.06));
    p.hip_r = Jp::r(e3(-1.75, 0.0, -0.06));
    p.knee_l = Jp::r(rx(2.15));
    p.knee_r = Jp::r(rx(2.2));
    p.foot_l = Jp::r(rx(0.5));
    p.foot_r = Jp::r(rx(0.5));
    p.sh_l = Jp::r(e3(-0.9, 0.0, -0.45));
    p.el_l = Jp::r(rx(-1.9));
    p.sh_r = Jp::r(e3(-0.85, 0.0, 0.45));
    p.el_r = Jp::r(rx(-1.8));
    p.shield = Jp { t: Some(SHIELD_GAIT_T), r: shield_gait_r() };
    p.sword = Jp::r(e3(2.4, 0.3, 0.0));
    p
}

// ── Jump (physics height → studio airtime formulas) ─────────────────────────────────────
// A standing hop, but no longer mirror-symmetric: signed velocity splits **rise** (legs trail from
// the push-off) → **apex** (knees tuck up, arms thrown high + a small torso twist for life) →
// **fall** (legs reach down to land, arms open for balance). The lead/trail legs differ so it never
// reads like a flat frontal frame.
fn jump_pose(vel_y: f32) -> Pose {
    let v = (vel_y / 6.5).clamp(-1.0, 1.0); // 6.5 = movement::JUMP_SPEED; signed: + rising, − falling
    let h = (1.0 - v.abs()).clamp(0.0, 1.0); // studio `height`: 0 at launch/landing, 1 at apex
    let rise = v.max(0.0); // 1 just off the ground, 0 by apex
    let fall = (-v).max(0.0); // 0 until apex, 1 dropping fast
    let mut p = rest(); // root owns real height → hips stay at rest
    p.torso = Jp::r(e3(0.3 - h * 0.45 + fall * 0.2, h * 0.18, h * 0.12)); // crunch + a touch of twist/roll at apex
    p.head = Jp::r(rx(-0.2 + h * 0.15 - fall * 0.2));
    // Lead (left) leg tucks higher at apex; trail (right) trails the push-off and reaches first to land.
    p.hip_l = Jp::r(rx(-0.4 + h * 0.95 - fall * 0.35));
    p.hip_r = Jp::r(rx(-0.6 + h * 0.45 + fall * 0.2));
    p.knee_l = Jp::r(rx(1.0 - h * 0.95 + rise * 0.2));
    p.knee_r = Jp::r(rx(1.0 - h * 0.5 - fall * 0.3));
    p.foot_l = Jp::r(rx(-0.5 + h * 0.6));
    p.foot_r = Jp::r(rx(-0.5 + h * 0.8 + fall * 0.2));
    // Arms thrown up at apex; on the way down the off arm sweeps wider to balance the descent.
    p.sh_l = Jp::r(e3(0.5 - h * 1.7 - rise * 0.2, 0.0, -h * 0.4));
    p.el_l = Jp::r(rx(-0.8 + h * 0.5));
    p.sh_r = Jp::r(e3(0.5 - h * 1.5, 0.0, h * 0.45 + fall * 0.35));
    p.el_r = Jp::r(rx(-0.85 + h * 0.5));
    p
}

// ── Defend (shield block) — full studio depth (ease=1) + idle sway; cross-faded by block_amt. ──
fn defend_pose(t: f32) -> Pose {
    // The three.js footman BLOCK: bladed stance, shield squared in front of the chest, the sword
    // drawn back at the hip with its point toward the foe.
    let b = (t * PI * 2.0 / 1.2).sin();
    let mut p = rest();
    p.hips = Jp { t: Some(Vec3::new(0.0, 1.035, 0.0)), r: m3(0.0, -0.25, 0.0) };
    p.torso = Jp::r(m3(0.1 + 0.02 * b, -0.15, 0.0));
    p.head = Jp::r(m3(0.0, 0.38, 0.0));
    p.hip_l = Jp::r(m3(-0.32, 0.25, 0.06));
    p.knee_l = Jp::r(rx(0.38));
    p.foot_l = Jp::r(rx(-0.06));
    p.hip_r = Jp::r(m3(0.28, 0.25, -0.06));
    p.knee_r = Jp::r(rx(0.3));
    p.foot_r = Jp::r(rx(-0.58));
    p.sh_l = Jp::r(m3(-1.15, 0.0, -0.2));
    p.el_l = Jp::r(rx(-1.05));
    p.shield = posed_shield(2.133, -0.344, 0.06);
    p.sh_r = Jp::r(m3(-0.35, 0.0, -0.25));
    p.el_r = Jp::r(rx(-1.15));
    p.sword = posed_sword(-0.09, 0.149, 0.254);
    p
}

// ── Attacks (the studio 3-phase wind/strike/recovery functions, verbatim lerp targets) ──
pub(crate) enum Phase {
    Wind,
    Strike,
    Recovery,
}

/// Split our one-shot swing progress `ap = attack_t/ATTACK_DURATION` into the studio phases, eased
/// like the studio's `getAttackPhase`. `WIND_END` aligns the strike start with combat's `HIT_PHASE`.
pub(crate) fn attack_phase(ap: f32) -> (Phase, f32) {
    const WIND_END: f32 = 0.30;
    const STRIKE_END: f32 = 0.55;
    if ap < WIND_END {
        (Phase::Wind, ease_out_cubic(ap / WIND_END))
    } else if ap < STRIKE_END {
        let t = (ap - WIND_END) / (STRIKE_END - WIND_END);
        (Phase::Strike, 1.0 - (1.0 - t).powf(2.5))
    } else {
        (Phase::Recovery, smoothstep((ap - STRIKE_END) / (1.0 - STRIKE_END)))
    }
}

/// The Heavy Strike at swing progress `ap`. It is only ever released out of the held
/// [`charge_stance`] — the blade already cocked overhead, i.e. the chop's Wind at 1 — so instead of
/// the shared [`attack_phase`] (which dropped the arm back to rest and re-raised it, then started
/// the chop exactly at the damage instant, with the blade still overhead) it opens with a short
/// gather (the coil deepens a touch and lets go: anticipation), then drives the chop so the blade
/// is mid-arc, coming down through the foe, at `HIT_PHASE`, and settles out unhurried.
fn heavy_strike_pose(ap: f32) -> Pose {
    const GATHER_END: f32 = 0.16;
    const CHOP_END: f32 = 0.44;
    if ap < GATHER_END {
        let u = ap / GATHER_END;
        heavy_chop(&Phase::Wind, 1.0 + 0.14 * (PI * u).sin() * (1.0 - 0.35 * u))
    } else if ap < CHOP_END {
        // Accelerate out of the coil, fastest just before contact, braking into the follow-through.
        let t = (ap - GATHER_END) / (CHOP_END - GATHER_END);
        heavy_chop(&Phase::Strike, smootherstep(t.powf(0.8)))
    } else {
        heavy_chop(&Phase::Recovery, smoothstep((ap - CHOP_END) / (1.0 - CHOP_END)))
    }
}

fn smootherstep(t: f32) -> f32 {
    let c = t.clamp(0.0, 1.0);
    c * c * c * (c * (c * 6.0 - 15.0) + 10.0)
}

pub(crate) fn attack_pose(variant: u8, phase: &Phase, p: f32) -> Pose {
    match variant {
        1 => horizontal_slash(phase, p),
        2 => forward_thrust(phase, p),
        v if v == super::combat::HEAVY_VARIANT => heavy_chop(phase, p),
        _ => overhead_chop(phase, p),
    }
}

/// The charged **Heavy Strike** — a true two-handed **overhead smash**: the wind-up hauls the blade
/// up over the head, the strike drives it down through the front with the whole body behind it, and
/// the recovery settles back to rest. The raise is modelled on the proven `overhead_chop` (attack1):
/// the arm lifts via a **moderate shoulder flex + a folded elbow** (NOT by cranking `sh_r` X toward
/// π — that swings the arm up the *back* arc, wrenching the shoulder backward like a backstroke), so
/// the blade is hauled up over the head the natural way. Both hands grip (off-hand drawn up to the
/// haft) and the legs coil into a load-crouch for the heavy. The cocked **Wind-end pose IS the held
/// [`charge_stance`]**, so the release flows seamlessly from raised-overhead into the downward chop —
/// one continuous "raise → smash".
fn heavy_chop(phase: &Phase, p: f32) -> Pose {
    let mut po = rest();
    match phase {
        Phase::Wind => {
            // Haul the blade up over the head the natural way (shoulder flex + folded elbow, à la
            // attack1), both hands to the haft, legs coiled into a load-crouch — wound up to smash down.
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(1.05, 0.97, p), lerp(0.0, -0.1, p))), r: e3(lerp(0.0, 0.06, p), lerp(0.0, -0.22, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.0, -0.16, p), lerp(0.0, -0.18, p), 0.0)); // coil away, ready to uncoil
            po.head = Jp::r(e3(lerp(0.0, -0.02, p), lerp(0.0, 0.18, p), 0.0)); // eyes stay on the target
            po.sh_r = Jp::r(e3(lerp(0.12, -2.9, p), 0.0, lerp(0.15, 0.05, p))); // raise up the FRONT arc (neg X) → arm overhead, leaning toward the foe (no back-wrench)
            po.el_r = Jp::r(rx(lerp(-0.4, -0.3, p))); // near-straight: arm + blade read as one raised line
            po.sword = Jp::r(e3(lerp(SWORD_REST_X, 2.3, p), lerp(0.3, 0.2, p), 0.0)); // counter-rotate: blade laid back over the head toward horizontal (natural cock, not a vertical flagpole)
            po.sh_l = Jp::r(e3(lerp(0.1, 0.45, p), lerp(0.0, 0.35, p), lerp(-0.15, -0.15, p))); // off hand drawn up to the haft (two-handed)
            po.el_l = Jp::r(rx(lerp(-0.5, -1.2, p)));
            po.shield = Jp { t: Some(SHIELD_REST_T), r: e3(lerp(0.15, 0.3, p), lerp(-1.5, -1.0, p), 0.0) };
            po.hip_l = Jp::r(rx(lerp(0.0, -0.2, p)));
            po.hip_r = Jp::r(rx(lerp(0.0, -0.25, p)));
            po.knee_l = Jp::r(rx(lerp(0.0, 0.3, p))); // braced load-crouch
            po.knee_r = Jp::r(rx(lerp(0.0, 0.32, p)));
        }
        Phase::Strike => {
            // Explosive downward chop — the raised arm unfolds and smashes through the front, body
            // drives in, stepping into a planted lunge.
            po.hips = Jp { t: Some(Vec3::new(0.0, 0.97 + (p * PI).sin() * 0.05, lerp(-0.1, 0.32, p))), r: e3(lerp(0.06, 0.18, p), lerp(-0.22, 0.16, p), 0.0) };
            po.torso = Jp::r(e3(lerp(-0.16, 0.42, p), lerp(-0.18, 0.12, p), 0.0));
            po.head = Jp::r(e3(lerp(-0.02, 0.14, p), lerp(0.18, -0.06, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(-2.9, -1.45, p), lerp(0.0, 0.1, p), lerp(0.05, -0.12, p))); // overhead-front → down-front (one forward arc)
            po.el_r = Jp::r(rx(lerp(-0.3, -0.3, p)));
            po.sword = Jp::r(e3(lerp(2.3, 2.75, p), lerp(0.2, -0.5, p), lerp(0.0, -0.3, p))); // blade whips down through the front
            po.sh_l = Jp::r(e3(lerp(0.45, -0.2, p), lerp(0.35, -0.1, p), lerp(-0.15, -0.4, p)));
            po.el_l = Jp::r(rx(lerp(-1.2, -0.85, p)));
            po.hip_l = Jp::r(rx(lerp(-0.2, 0.5, p)));
            po.knee_l = Jp::r(rx(lerp(0.3, 0.6, p))); // step into a planted lunge
            po.hip_r = Jp::r(rx(lerp(-0.25, -0.15, p)));
            po.knee_r = Jp::r(rx(lerp(0.32, 0.42, p)));
        }
        Phase::Recovery => {
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(0.97, 1.05, p), lerp(0.32, 0.0, p))), r: e3(lerp(0.18, 0.0, p), lerp(0.16, 0.0, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.42, 0.0, p), lerp(0.12, 0.0, p), 0.0));
            po.head = Jp::r(e3(lerp(0.14, 0.0, p), lerp(-0.06, 0.0, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(-1.45, 0.12, p), lerp(0.1, 0.0, p), lerp(-0.12, 0.15, p)));
            po.el_r = Jp::r(rx(lerp(-0.3, -0.4, p)));
            po.sword = Jp::r(e3(lerp(2.75, SWORD_REST_X, p), lerp(-0.5, 0.3, p), lerp(-0.3, 0.0, p)));
            po.sh_l = Jp::r(e3(lerp(-0.2, 0.1, p), lerp(-0.12, 0.0, p), lerp(-0.4, -0.15, p)));
            po.el_l = Jp::r(rx(lerp(-0.85, -0.5, p)));
            po.hip_l = Jp::r(rx(lerp(0.5, 0.0, p)));
            po.knee_l = Jp::r(rx(lerp(0.6, 0.0, p)));
            po.hip_r = Jp::r(rx(lerp(-0.15, 0.0, p)));
            po.knee_r = Jp::r(rx(lerp(0.42, 0.0, p)));
        }
    }
    po
}

/// The held **charge stance** while winding up a Heavy Strike (after the light swing, before
/// release). Completely reworked: instead of a stiff sword-out-to-the-side point, this is a proper
/// **two-handed overhead raise** — the blade hauled up high over the head, body coiled back onto the
/// rear foot, ready to smash straight down. It IS the wind-up phase of [`heavy_chop`] held at the
/// current charge (`frac` 0→1 deepens the coil), so the release flows seamlessly into the chop's
/// strike from exactly this cocked position — one continuous "raise → smash" motion. A small
/// `wobble` (a tiny tremble of effort, driven from wall-clock in [`hero_anim`]) keeps it alive
/// instead of frozen.
fn charge_stance(frac: f32, wobble: f32) -> Pose {
    let f = frac.clamp(0.0, 1.0);
    // Reuse the heavy chop's wind-up so the held coil and the strike are the same motion.
    let mut po = heavy_chop(&Phase::Wind, f);
    // Tremble of effort: a faint shake on the blade/arms + a breath at the torso, scaled by how
    // wound-up we are, so a full charge visibly strains.
    let tw = wobble * f;
    po.sword = Jp::r(po.sword.r * e3(tw * 0.05, tw * 0.07, 0.0));
    po.torso = Jp::r(po.torso.r * e3(tw * 0.03, 0.0, tw * 0.02));
    po.sh_r = Jp::r(po.sh_r.r * e3(0.0, 0.0, tw * 0.04));
    po
}

/// attack1 — diagonal overhead chop (studio `applyOverheadChop`).
fn overhead_chop(phase: &Phase, p: f32) -> Pose {
    let mut po = rest();
    match phase {
        Phase::Wind => {
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(1.05, 0.99, p), lerp(0.0, -0.1, p))), r: e3(lerp(0.0, 0.06, p), lerp(0.0, -0.25, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.0, -0.18, p), lerp(0.0, -0.15, p), 0.0));
            po.head = Jp::r(e3(0.0, lerp(0.0, 0.2, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(0.12, 0.35, p), lerp(0.0, -0.55, p), lerp(0.15, 0.45, p)));
            po.el_r = Jp::r(rx(lerp(-0.4, -1.75, p)));
            po.sword = Jp::r(e3(lerp(SWORD_REST_X, 0.55, p), lerp(0.3, 0.55, p), lerp(0.0, -0.45, p)));
            po.sh_l = Jp::r(e3(lerp(0.1, 0.05, p), lerp(0.0, 0.15, p), lerp(-0.15, -0.25, p)));
            po.el_l = Jp::r(rx(lerp(-0.5, -0.65, p)));
            po.shield = Jp { t: Some(Vec3::new(0.0, 0.0, lerp(0.14, 0.16, p))), r: e3(lerp(0.15, 0.25, p), lerp(-0.45, -0.35, p), lerp(0.1, 0.05, p)) };
            po.hip_l = Jp::r(rx(lerp(0.0, -0.2, p)));
            po.hip_r = Jp::r(rx(lerp(0.0, -0.25, p)));
        }
        Phase::Strike => {
            po.hips = Jp { t: Some(Vec3::new(0.0, 0.99 + (p * PI).sin() * 0.05, lerp(-0.1, 0.2, p))), r: e3(lerp(0.06, 0.14, p), lerp(-0.25, 0.15, p), 0.0) };
            po.torso = Jp::r(e3(lerp(-0.18, 0.26, p), lerp(-0.15, 0.1, p), 0.0));
            po.head = Jp::r(e3(lerp(0.0, 0.08, p), lerp(0.2, -0.05, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(0.35, -1.25, p), lerp(-0.55, 0.2, p), lerp(0.45, -0.15, p)));
            po.el_r = Jp::r(rx(lerp(-1.75, -0.35, p)));
            po.sword = Jp::r(e3(lerp(0.55, 2.7, p), lerp(0.55, -0.8, p), lerp(-0.45, -0.8, p)));
            po.sh_l = Jp::r(e3(lerp(0.05, -0.15, p), lerp(0.15, -0.1, p), lerp(-0.25, -0.35, p)));
            po.el_l = Jp::r(rx(lerp(-0.65, -0.85, p)));
            po.hip_l = Jp::r(rx(lerp(-0.2, 0.35, p)));
            po.knee_l = Jp::r(rx(lerp(0.0, 0.4, p)));
            po.hip_r = Jp::r(rx(lerp(-0.25, -0.1, p)));
            po.knee_r = Jp::r(rx(lerp(0.0, 0.3, p)));
        }
        Phase::Recovery => {
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(0.99, 1.05, p), lerp(0.2, 0.0, p))), r: e3(lerp(0.14, 0.0, p), lerp(0.15, 0.0, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.26, 0.0, p), lerp(0.1, 0.0, p), 0.0));
            po.head = Jp::r(e3(lerp(0.08, 0.0, p), lerp(-0.05, 0.0, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(-1.25, 0.12, p), lerp(0.2, 0.0, p), lerp(-0.15, 0.15, p)));
            po.el_r = Jp::r(rx(lerp(-0.35, -0.4, p)));
            po.sword = Jp::r(e3(lerp(2.7, SWORD_REST_X, p), lerp(-0.8, 0.3, p), lerp(-0.8, 0.0, p)));
            po.sh_l = Jp::r(e3(lerp(-0.15, 0.1, p), lerp(-0.1, 0.0, p), lerp(-0.35, -0.15, p)));
            po.el_l = Jp::r(rx(lerp(-0.85, -0.5, p)));
            po.hip_l = Jp::r(rx(lerp(0.35, 0.0, p)));
            po.knee_l = Jp::r(rx(lerp(0.4, 0.0, p)));
            po.hip_r = Jp::r(rx(lerp(-0.1, 0.0, p)));
            po.knee_r = Jp::r(rx(lerp(0.3, 0.0, p)));
        }
    }
    po
}

/// attack2 — horizontal slash (studio `applyHorizontalSlash`).
fn horizontal_slash(phase: &Phase, p: f32) -> Pose {
    let mut po = rest();
    match phase {
        Phase::Wind => {
            // Sink into a WIDE, LOW, planted power-stance (the reference silhouette): hips drop deep,
            // both legs splay out to the sides and bend hard, weight coiled onto both feet — loaded to
            // unleash a big horizontal sweep. The arms cock the blade back/out to the right.
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(1.05, 0.86, p), lerp(0.0, -0.04, p))), r: e3(0.0, lerp(0.0, -0.4, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.0, 0.05, p), lerp(0.0, -0.35, p), 0.0));
            po.head = Jp::r(e3(0.0, lerp(0.0, -0.3, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(0.12, -0.15, p), lerp(0.0, -0.65, p), lerp(0.15, 0.55, p)));
            po.el_r = Jp::r(rx(lerp(-0.4, -1.35, p)));
            po.sword = Jp::r(e3(lerp(SWORD_REST_X, 2.35, p), lerp(0.3, 0.75, p), lerp(0.0, -0.6, p)));
            po.sh_l = Jp::r(e3(lerp(0.1, 0.25, p), lerp(0.0, 0.35, p), lerp(-0.15, -0.1, p)));
            po.el_l = Jp::r(rx(lerp(-0.5, -0.4, p)));
            po.shield = Jp { t: Some(Vec3::new(0.0, 0.0, lerp(0.14, 0.18, p))), r: e3(lerp(0.15, 0.35, p), lerp(-0.45, -0.2, p), lerp(0.1, 0.15, p)) };
            // Wide planted legs: hips roll OUT on Z (splay), thighs sit back, knees fold deep, feet flatten.
            po.hip_l = Jp::r(e3(lerp(0.0, -0.12, p), 0.0, lerp(0.0, 0.4, p)));
            po.knee_l = Jp::r(rx(lerp(0.0, 0.7, p)));
            po.foot_l = Jp::r(rx(lerp(0.0, -0.25, p)));
            po.hip_r = Jp::r(e3(lerp(0.0, -0.1, p), 0.0, lerp(0.0, -0.5, p)));
            po.knee_r = Jp::r(rx(lerp(0.0, 0.78, p)));
            po.foot_r = Jp::r(rx(lerp(0.0, -0.3, p)));
        }
        Phase::Strike => {
            // Uncoil EXPLOSIVELY: drive up out of the deep stance (hips rise 0.86→1.0) as the splayed
            // legs sweep into a forward plant and the blade whips across the front.
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(0.86, 1.0, p) + (p * PI).sin() * 0.04, lerp(-0.04, 0.2, p))), r: e3(0.0, lerp(-0.4, 0.55, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.05, 0.12, p), lerp(-0.35, 0.55, p), lerp(0.0, 0.08, p)));
            po.head = Jp::r(e3(0.0, lerp(-0.3, 0.15, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(-0.15, -1.4, p), lerp(-0.65, 0.0, p), lerp(0.55, -0.4, p)));
            po.el_r = Jp::r(rx(lerp(-1.35, -0.25, p)));
            po.sword = Jp::r(e3(lerp(2.35, 2.45, p), lerp(0.75, 0.05, p), lerp(-0.6, 0.25, p)));
            po.sh_l = Jp::r(e3(lerp(0.25, -0.35, p), lerp(0.35, -0.45, p), lerp(-0.1, -0.4, p)));
            po.el_l = Jp::r(rx(lerp(-0.4, -0.75, p)));
            po.hip_l = Jp::r(e3(lerp(-0.12, 0.3, p), 0.0, lerp(0.4, 0.0, p))); // splay closes as the leg drives forward
            po.knee_l = Jp::r(rx(lerp(0.7, 0.25, p)));
            po.foot_l = Jp::r(rx(lerp(-0.25, 0.0, p)));
            po.hip_r = Jp::r(e3(lerp(-0.1, -0.15, p), 0.0, lerp(-0.5, 0.0, p)));
            po.knee_r = Jp::r(rx(lerp(0.78, 0.2, p)));
            po.foot_r = Jp::r(rx(lerp(-0.3, 0.0, p)));
        }
        Phase::Recovery => {
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(1.0, 1.05, p), lerp(0.2, 0.0, p))), r: e3(0.0, lerp(0.55, 0.0, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.12, 0.0, p), lerp(0.55, 0.0, p), lerp(0.08, 0.0, p)));
            po.head = Jp::r(e3(0.0, lerp(0.15, 0.0, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(-1.4, 0.12, p), lerp(0.0, 0.0, p), lerp(-0.4, 0.15, p)));
            po.el_r = Jp::r(rx(lerp(-0.25, -0.4, p)));
            po.sword = Jp::r(e3(lerp(2.45, SWORD_REST_X, p), lerp(0.05, 0.3, p), lerp(0.25, 0.0, p)));
            po.sh_l = Jp::r(e3(lerp(-0.35, 0.1, p), lerp(-0.45, 0.0, p), lerp(-0.4, -0.15, p)));
            po.el_l = Jp::r(rx(lerp(-0.75, -0.5, p)));
            po.hip_l = Jp::r(rx(lerp(0.3, 0.0, p)));
            po.knee_l = Jp::r(rx(lerp(0.25, 0.0, p)));
            po.hip_r = Jp::r(rx(lerp(-0.15, 0.0, p)));
            po.knee_r = Jp::r(rx(lerp(0.2, 0.0, p)));
        }
    }
    po
}

/// attack3 — forward thrust (studio `applyForwardThrust`).
fn forward_thrust(phase: &Phase, p: f32) -> Pose {
    let mut po = rest();
    match phase {
        Phase::Wind => {
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(1.05, 0.97, p), lerp(0.0, -0.05, p))), r: e3(lerp(0.0, 0.08, p), lerp(0.0, -0.15, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.0, 0.1, p), lerp(0.0, -0.1, p), 0.0));
            po.head = Jp::r(e3(0.0, lerp(0.0, 0.1, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(0.12, -0.35, p), lerp(0.0, -0.25, p), lerp(0.15, 0.3, p)));
            po.el_r = Jp::r(rx(lerp(-0.4, -1.45, p)));
            po.sword = Jp::r(e3(lerp(SWORD_REST_X, 2.4, p), lerp(0.3, 0.2, p), lerp(0.0, 0.3, p)));
            po.sh_l = Jp::r(e3(lerp(0.1, -0.2, p), lerp(0.0, 0.25, p), lerp(-0.15, -0.3, p)));
            po.el_l = Jp::r(rx(lerp(-0.5, -0.7, p)));
            po.shield = Jp { t: Some(Vec3::new(0.0, 0.0, lerp(0.14, 0.12, p))), r: e3(lerp(0.15, PI / 2.0, p), lerp(-0.45, -0.1, p), lerp(0.1, 0.0, p)) };
            po.hip_l = Jp::r(rx(lerp(0.0, -0.25, p)));
            po.hip_r = Jp::r(rx(lerp(0.0, -0.3, p)));
            po.knee_l = Jp::r(rx(lerp(0.0, 0.35, p)));
            po.knee_r = Jp::r(rx(lerp(0.0, 0.4, p)));
        }
        Phase::Strike => {
            po.hips = Jp { t: Some(Vec3::new(0.0, 0.97, lerp(-0.05, 0.3, p))), r: e3(lerp(0.08, 0.12, p), lerp(-0.15, 0.05, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.1, 0.2, p), lerp(-0.1, 0.05, p), 0.0));
            po.head = Jp::r(e3(lerp(0.0, 0.05, p), lerp(0.1, -0.05, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(-0.35, -1.55, p), lerp(-0.25, 0.05, p), lerp(0.3, 0.05, p)));
            po.el_r = Jp::r(rx(lerp(-1.45, -0.1, p)));
            po.sword = Jp::r(e3(lerp(2.4, 2.7, p), lerp(0.2, 0.8, p), lerp(0.3, 0.4, p)));
            po.sh_l = Jp::r(e3(lerp(-0.2, -0.55, p), lerp(0.25, 0.1, p), lerp(-0.3, -0.45, p)));
            po.el_l = Jp::r(rx(lerp(-0.7, -0.85, p)));
            po.hip_l = Jp::r(rx(lerp(-0.25, 0.45, p)));
            po.knee_l = Jp::r(rx(lerp(0.35, 0.15, p)));
            po.hip_r = Jp::r(rx(lerp(-0.3, 0.1, p)));
            po.knee_r = Jp::r(rx(lerp(0.4, 0.1, p)));
        }
        Phase::Recovery => {
            po.hips = Jp { t: Some(Vec3::new(0.0, lerp(0.97, 1.05, p), lerp(0.3, 0.0, p))), r: e3(lerp(0.12, 0.0, p), lerp(0.05, 0.0, p), 0.0) };
            po.torso = Jp::r(e3(lerp(0.2, 0.0, p), lerp(0.05, 0.0, p), 0.0));
            po.head = Jp::r(e3(lerp(0.05, 0.0, p), lerp(-0.05, 0.0, p), 0.0));
            po.sh_r = Jp::r(e3(lerp(-1.55, 0.12, p), lerp(0.05, 0.0, p), lerp(0.05, 0.15, p)));
            po.el_r = Jp::r(rx(lerp(-0.1, -0.4, p)));
            po.sword = Jp::r(e3(lerp(2.7, SWORD_REST_X, p), lerp(0.8, 0.3, p), lerp(0.4, 0.0, p)));
            po.sh_l = Jp::r(e3(lerp(-0.55, 0.1, p), lerp(0.1, 0.0, p), lerp(-0.45, -0.15, p)));
            po.el_l = Jp::r(rx(lerp(-0.85, -0.5, p)));
            po.hip_l = Jp::r(rx(lerp(0.45, 0.0, p)));
            po.knee_l = Jp::r(rx(lerp(0.15, 0.0, p)));
            po.hip_r = Jp::r(rx(lerp(0.1, 0.0, p)));
            po.knee_r = Jp::r(rx(lerp(0.1, 0.0, p)));
        }
    }
    po
}

// ── Victory (studio `victory`) ──────────────────────────────────────────────────────────
fn victory_pose(t: f32) -> Pose {
    let s = (t * 1.5).sin();
    let mut p = rest();
    p.hips = Jp { t: Some(Vec3::new(0.0, 1.07 + (t * 3.5).sin() * 0.02, 0.0)), r: e3(0.0, 0.25 * s, 0.0) };
    p.torso = Jp::r(e3(-0.12, 0.05 * s, 0.0));
    p.head = Jp::r(e3(-0.25, 0.25 * s, 0.0));
    p.sh_l = Jp::r(e3(0.1, 0.2, -0.3));
    p.el_l = Jp::r(rx(-0.3));
    p.sh_r = Jp::r(e3(2.8, 0.0, -0.1)); // sword thrust skyward
    p.el_r = Jp::r(Quat::IDENTITY);
    p.sword = Jp::r(e3(0.15, 0.3, 0.0));
    // Wide stance (studio overrides the hip-joint X positions).
    p.hip_l = Jp { t: Some(Vec3::new(-0.22, -0.05, 0.0)), r: e3(0.0, 0.0, -0.15) };
    p.hip_r = Jp { t: Some(Vec3::new(0.22, -0.05, 0.0)), r: e3(0.0, 0.0, 0.15) };
    p
}

/// Seated rest pose (studio `applySeatedPose` / `SEATED`) — for a mob roosting on a stump: hips
/// dropped + tipped back, thighs forward with a deep knee bend, feet tucked, hands low. Shared by
/// the biped animator (orcs sitting on camp stumps); the caller positions the root on the stump.
pub(crate) fn sit_pose() -> Pose {
    let mut p = rest();
    p.hips = Jp { t: Some(Vec3::new(0.0, 0.6725, -0.08)), r: e3(0.10, 0.0, 0.0) };
    p.torso = Jp::r(e3(0.20, 0.0, 0.0));
    p.head = Jp::r(e3(-0.10, 0.0, 0.0));
    p.hip_l = Jp::r(e3(-0.78, 0.38, -0.18));
    p.hip_r = Jp::r(e3(-0.78, -0.38, 0.18));
    p.knee_l = Jp::r(rx(1.95));
    p.knee_r = Jp::r(rx(1.95));
    p.foot_l = Jp::r(rx(-0.55));
    p.foot_r = Jp::r(rx(-0.55));
    p.sh_l = Jp::r(e3(0.15, -0.15, -0.45));
    p.el_l = Jp::r(rx(-0.9));
    p.sh_r = Jp::r(e3(0.1, 0.2, 0.3));
    p.el_r = Jp::r(rx(-1.0));
    p.shield = Jp { t: Some(Vec3::new(0.0, -0.05, 0.1)), r: e3(0.85, -0.25, 0.12) };
    p.sword = Jp::r(e3(2.5, 0.25, 0.2));
    p
}

/// A posted town worker's repetitive two-handed tool stroke (a Warbell flavour clip, not a studio
/// one): both arms swing together on X over a fixed elbow grip, legs planted with a tiny
/// weight-shift, and a small head nod toward the work. `hoe` = a quick forward farmer stroke; else a
/// slower overhead chop/pick (woodcutter/miner). `t` is the worker's phase-desynced clock.
pub(crate) fn work_pose(t: f32, hoe: bool) -> Pose {
    let mut p = rest();
    let (arm, nod_rate) = if hoe {
        (0.6 + 0.7 * (t * 4.5).sin(), 4.5) // quick hoe, ~1.4s
    } else {
        (-0.2 + 1.3 * (0.5 - 0.5 * (t * 3.0).cos()), 3.0) // overhead → down chop/pick, ~2.1s
    };
    // Both arms drive the stroke together; a fixed elbow bend so they read as gripping the haft.
    p.sh_l = Jp::r(e3(arm, 0.0, -0.12));
    p.sh_r = Jp::r(e3(arm, 0.0, 0.12));
    p.el_l = Jp::r(rx(-0.7));
    p.el_r = Jp::r(rx(-0.7));
    p.head = Jp::r(rx((t * nod_rate).sin() * 0.06));
    // A subtle planted weight-shift so they aren't board-stiff while working.
    let sway = (t * 0.8).sin() * 0.02;
    p.hip_l = Jp::r(rx(sway));
    p.hip_r = Jp::r(rx(-sway));
    p
}

/// The bow shot's release moment as a fraction of the whole clip — the arrow entity must leave the
/// string exactly when the string hand snaps open, so the archer brain (`villagers::guard_combat`)
/// times its `ArrowSpawn` off this same constant.
pub(crate) const BOW_RELEASE_P: f32 = 0.60;

/// A Warbell flavour clip (not a studio one): the archer's **draw-and-loose**. One shot, `p` 0..1:
/// the bow arm levels at the target while the string hand reaches to the string (draw), pulls to
/// the cheek and holds a steady aiming beat (a faint tremble of effort), the string hand SNAPS open
/// at [`BOW_RELEASE_P`] with a small whole-body recoil, then everything settles back to the carry.
/// The body blades side-on (hips + torso yaw toward the string side, head counter-yawed onto the
/// target) — the root still faces the target, so the silhouette reads as a braced archer, not a
/// squared-up peasant. The off-hand `Shield` pivot carries the BOW (stave authored +Y,
/// string at -Z): this clip turns it upright into the draw; the `Sword` pivot's nocked arrow is
/// levelled at the target through the aim.
pub(crate) fn bow_pose(t: f32, p: f32) -> Pose {
    let p = p.clamp(0.0, 1.0);
    let draw = smoothstep(p / 0.40); // reach + pull to the cheek
    let loose = smoothstep((p - BOW_RELEASE_P) / 0.05); // the string hand snaps open
    let settle = smoothstep((p - 0.70) / 0.30); // ease the whole pose back to rest
    // A faint aiming tremble while at full draw (gone once loosed).
    let trem = (t * 21.0).sin() * 0.012 * draw * (1.0 - loose);

    let mut po = rest();
    // Blade the body: hips + torso yaw toward the string side, head counter-yawed onto the target,
    // weight settled into a staggered stance (lead/left foot toward the foe).
    po.hips = Jp {
        t: Some(Vec3::new(0.0, lerp(1.05, 1.01, draw), 0.0)),
        r: e3(0.0, 0.42 * draw, 0.0),
    };
    po.torso = Jp::r(e3(-0.05 * draw, 0.30 * draw, 0.04 * draw));
    po.head = Jp::r(e3(0.02 * draw + trem, -0.62 * draw, 0.0));
    po.hip_l = Jp::r(e3(-0.22 * draw, 0.12 * draw, -0.05 * draw));
    po.knee_l = Jp::r(rx(0.14 * draw));
    po.foot_l = Jp::r(rx(-0.06 * draw));
    po.hip_r = Jp::r(e3(0.14 * draw, -0.1 * draw, 0.06 * draw));
    po.knee_r = Jp::r(rx(0.22 * draw));

    // Bow arm: levels straight out at the target (compensating the torso yaw), elbow near-locked.
    po.sh_l = Jp::r(e3(lerp(0.1, -1.42, draw) + trem, lerp(0.0, 0.34, draw), lerp(-0.15, -0.06, draw)));
    po.el_l = Jp::r(rx(lerp(-0.5, -0.1, draw)));
    // The bow itself: from the at-ease carry (stave through the fist, tipped forward-up — see
    // `toolwork::bow_carry_r`) to UPRIGHT in the draw. With the arm raised forward (hand-local
    // −Y ≈ world-forward, +Z ≈ world-up), pitching the mesh +X by +π/2 stands the stave (mesh +Y)
    // vertical and turns the string (mesh −Z) back at the cheek. Both are pure X turns, so the
    // stave stays in the fist the whole way.
    po.shield = Jp {
        t: Some(toolwork::BOW_CARRY_T.lerp(Vec3::new(0.0, -0.02, 0.05), draw)),
        r: toolwork::bow_carry_r().slerp(e3(1.55, 0.0, 0.0), draw),
    };
    // String hand: reaches forward with the nock, hauls straight back to the cheek (the elbow
    // folding to a right angle at shoulder height, shoulder drawn back around the yawed torso),
    // then SNAPS open past the release point.
    let pull = draw; // reach and pull share the envelope; the reach reads in the elbow unfolding
    po.sh_r = Jp::r(e3(
        lerp(0.12, -1.14, pull) + 0.14 * loose,
        lerp(0.0, -0.66, pull) - 0.45 * loose,
        lerp(0.15, 0.26, pull) + 0.12 * loose,
    ));
    po.el_r = Jp::r(rx(lerp(-0.4, -1.7, pull) + 0.95 * loose));
    // The nocked arrow lies level along the draw (pointing at the target), and drops with the hand
    // after the loose — the "next shaft" carried down at ease.
    po.sword = Jp::r(Jp::r(sword_rest_r()).r.slerp(e3(1.5, 0.15, 0.0), draw * (1.0 - loose)));

    // Recoil: a small whole-body give the instant the string lets go.
    if loose > 0.0 && settle < 1.0 {
        let k = loose * (1.0 - settle);
        po.torso = Jp::r(po.torso.r * e3(-0.05 * k, 0.03 * k, 0.0));
        po.sh_l = Jp::r(po.sh_l.r * e3(0.0, 0.06 * k, 0.04 * k));
    }

    // Settle everything back to the rest carry after the loose (the bow back to its at-ease grip).
    let mut out = rest();
    out.shield = Jp { t: Some(toolwork::BOW_CARRY_T), r: toolwork::bow_carry_r() };
    if settle > 0.0 {
        return po.lerp(&out, settle);
    }
    po
}

/// A worker hauling a load home (a log / a handcart): both arms raise forward with a fixed elbow
/// bend, gripping the load level in front of the chest. The legs come from locomotion (the worker
/// walks the load home), so the caller layers this over the gait with `action_over_loco`.
pub(crate) fn carry_pose() -> Pose {
    let mut p = rest();
    p.sh_l = Jp::r(e3(-0.55, 0.0, -0.12)); // upper arm raised forward
    p.sh_r = Jp::r(e3(-0.55, 0.0, 0.12));
    p.el_l = Jp::r(rx(-0.6)); // forearm up, gripping the load level
    p.el_r = Jp::r(rx(-0.6));
    p
}

/// The caged-captive clip family (huddle / hug-and-rock / rub arms / slump, the feet-planted
/// stand-up, gripping the bars, and the breath / look / flinch / talk overlays) — a child module so
/// it can build [`Pose`]s field-by-field like the clips above; `crate::captives` drives it.
pub(crate) mod captive;

/// Village-life clips (bench sit / stand-up, chores, social beats, carries) — `crate::village_life`.
pub(crate) mod village;

/// The orks' own animator (planted IK stance, foot-locked waddle, wound-up club blows that land on
/// the damage instant) — `crate::orks` drives it, `crate::biped::animate_biped` poses with it.
pub(crate) mod ork;

/// The light sword combo, retargeted from keyframed clips (see the module docs).
mod mixamo;
mod mixamo_clips;
mod swordplay;
mod ual_sword;

/// Townsfolk tools held for real: two-handed work strokes solved onto the haft, shoulder carries,
/// the slung tool + load grips while hauling, the guard's buckler and the archer's bow at ease.
mod toolwork;
pub(crate) use toolwork::{biped_tool_pose, next_blow_in, HeldTool, CART_GRIP, CART_SHAFT_PITCH, LOG_CRADLE};

/// Which clip family [`hero_anim`] is playing — a change triggers the cross-fade.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Clip {
    #[default]
    Loco,
    Air,
    Attack(u8),
    Charge,
    Dash,
    Roll,
    Victory,
    /// In the saddle (`crate::horse`).
    Ride,
}

#[derive(Default)]
pub struct ClipBlend {
    clip: Clip,
    last: Option<Pose>,
    /// (pose to fade from, start time, duration).
    from: Option<(Pose, f32, f32)>,
    /// The airborne clip leads with the right leg (mirrored) this jump.
    lead_right: bool,
    /// The light swing playing last frame (its combo step, duration), to start its recovery when it ends.
    swinging: Option<(u8, f32)>,
    /// A finished light swing settling back to the guard: (combo step, when its blade made contact).
    recover: Option<(u8, f32)>,
}

/// Draw/sheathe bookkeeping. Drawing for a fight that's coming (a foe ringed, blows traded)
/// plays the over-the-shoulder reach; an action straight from the slung carry (a swing, a block)
/// draws instantly so combat never waits on a flourish. Sheathing waits until things have been
/// calm for a moment (sooner when sprinting away).
#[derive(Default)]
pub struct Sheath {
    drawn: bool,
    /// Smoothed 0..1 of `drawn`, for the gait's arm carry.
    armed: f32,
    /// Progress (s) of a draw/sheathe reach in flight, and whether it ends drawn.
    reach_t: Option<f32>,
    to_drawn: bool,
    calm_for: f32,
}

/// One draw / sheathe: both hands go back at once (sword hilt over the right shoulder, shield rim at
/// the left flank) and come forward with the kit.
const SHEATHE_REACH: f32 = 0.7;
/// Calm this long before the kit goes away — long enough that a foe drifting in and out of range
/// doesn't have the hero fidgeting with his weapons.
const SHEATHE_DELAY: f32 = 3.0;
/// Shift in a fight: put the weapons away at once so the hero can run.
const SHEATHE_DELAY_SPRINT: f32 = 0.1;
/// A ringed foe within this (world u) counts as a fight on its own — the kit comes out.
const DRAW_RANGE: f32 = 4.5;
/// The shield hand's reach to the slung board's rim: the upper arm back (~35 deg) and a little out,
/// turned in so the bent forearm points behind the flank — the hand lands at the left side of the
/// back at about belt height (tuned against the viewer's `draw` loop).
fn shield_reach_sh() -> Quat {
    e3(0.6, 1.0, -0.3)
}
const SHIELD_REACH_EL: f32 = -1.05;

impl Sheath {
    fn update(&mut self, dt: f32, want: bool, action: bool, sprinting: bool) {
        self.calm_for = if want { 0.0 } else { self.calm_for + dt };
        if want && action && (!self.drawn || self.reach_t.is_some()) {
            // Straight into a swing/block: the blade is simply in hand.
            self.drawn = true;
            self.reach_t = None;
        } else if self.reach_t.is_none() {
            let delay = if sprinting { SHEATHE_DELAY_SPRINT } else { SHEATHE_DELAY };
            if want && !self.drawn {
                (self.reach_t, self.to_drawn) = (Some(0.0), true);
            } else if !want && self.drawn && self.calm_for > delay {
                (self.reach_t, self.to_drawn) = (Some(0.0), false);
            }
        } else if want && !self.to_drawn {
            // Trouble mid-sheathe: keep the blade out.
            self.drawn = true;
            self.reach_t = None;
        }
        if let Some(t) = self.reach_t.as_mut() {
            *t += dt;
            if *t >= 0.5 * SHEATHE_REACH {
                self.drawn = self.to_drawn; // hand at the hilt: swap
            }
            if *t >= SHEATHE_REACH {
                self.reach_t = None;
            }
        }
        let target = if self.drawn { 1.0 } else { 0.0 };
        self.armed += (target - self.armed) * (dt * 8.0).min(1.0);
    }

    /// 0→1→0 weight of the reach in flight.
    fn reach(&self) -> Option<f32> {
        self.reach_t.map(|t| (PI * (t / SHEATHE_REACH).clamp(0.0, 1.0)).sin())
    }

    /// The shield's hand-off mid-reach: `None` when it simply sits (on the arm while drawn, on the
    /// back otherwise); `Some(s)` while it travels, `s` = 0 on the back … 1 in the hand. The left
    /// hand meets the rim at the flank around the middle of the reach; the board peels off the back
    /// (draw) or settles onto it (sheathe) over a window around that meeting.
    fn shield_transit(&self) -> Option<f32> {
        let t = self.reach_t? / SHEATHE_REACH;
        let s = smoothstep((t - 0.38) / 0.3);
        Some(if self.to_drawn { s } else { 1.0 - s })
    }
}

/// Sword shoulder on its way from `from` (w = 0) to the slung hilt over the right shoulder
/// (w = 1), always up the front: a De Casteljau curve through an arm-raised-forward waypoint.
fn sword_reach_arc(from: Quat, w: f32) -> Quat {
    let (mid, hilt) = (e3(-1.7, -0.2, 0.3), e3(-3.1, -0.4, 0.3));
    from.slerp(mid, w).slerp(mid.slerp(hilt, w), w)
}

/// The draw/sheathe reach folded over `pose` at weight `w` (0 = the clip, 1 = hands at the slung
/// hilt and the shield rim).
fn reach_overlay(pose: Pose, w: f32) -> Pose {
    let mut p = pose;
    // Fist at the slung grip just under the crossguard (tuned against the viewer). That hilt pose
    // is a ~179 deg turn of the shoulder from a hanging arm, so a straight slerp to it is a coin
    // toss between raising the arm up the FRONT and wrenching it BACK and up behind the body:
    // the sign of a near-zero quaternion dot picked the route, flipped by whichever way the gait
    // had the arm swinging (and could flip mid-reach as the stride went on) — the playtest's
    // "sometimes he twists the arm backward when sheathing". Route it through an arm-raised-
    // forward waypoint instead (a quadratic slerp curve: every leg of it is well under 180 deg,
    // so each is unambiguous), so the hand always travels up the front and over the shoulder.
    p.sh_r.r = sword_reach_arc(p.sh_r.r, w);
    p.el_r = p.el_r.lerp(Jp::r(rx(-2.0)), w);
    p.sh_l = p.sh_l.lerp(Jp::r(shield_reach_sh()), w);
    p.el_l = p.el_l.lerp(Jp::r(rx(SHIELD_REACH_EL)), w);
    p.head = Jp::r(p.head.r * e3(0.0, -0.25 * w, 0.0)); // a glance at the hand
    p.torso = Jp::r(p.torso.r * e3(-0.05 * w, -0.1 * w, 0.0));
    p
}

/// The slung shield's pose (torso frame) part-way through its hand-off, `s` = 0 at the back `slot`
/// … 1 on the arm (`hand`). It swings round the LEFT flank on a cylinder about the spine — bearing
/// and radius interpolated, with a little bulge — instead of cutting straight through the body.
fn shield_handoff(slot: &Transform, hand: &Transform, s: f32) -> Transform {
    let (a, b) = (slot.translation, hand.translation);
    // Bearing about +Y, 0 = forward (+Z), −π/2 = left (−X); the back slot reads as −π so the sweep
    // runs back → left → front.
    let bearing = |v: Vec3| v.x.atan2(v.z);
    let mut ba = bearing(a);
    let bb = bearing(b);
    if ba > bb + PI {
        ba -= TAU;
    } else if ba < bb - PI {
        ba += TAU;
    }
    let (ra, rb) = (Vec2::new(a.x, a.z).length(), Vec2::new(b.x, b.z).length());
    let ang = lerp(ba, bb, s);
    let r = lerp(ra, rb, s) + 0.06 * (PI * s).sin();
    Transform {
        translation: Vec3::new(r * ang.sin(), lerp(a.y, b.y, s), r * ang.cos()),
        rotation: slot.rotation.slerp(hand.rotation, s),
        scale: Vec3::ONE,
    }
}

/// Swap the two legs of a pose (the clips here are sagittal X turns, so a straight swap mirrors).
fn mirror_legs(p: &Pose) -> Pose {
    let mut m = *p;
    (m.hip_l, m.hip_r) = (p.hip_r, p.hip_l);
    (m.knee_l, m.knee_r) = (p.knee_r, p.knee_l);
    (m.foot_l, m.foot_r) = (p.foot_r, p.foot_l);
    m
}

/// Largest hips shift (model m) the grounding pass will make.
const GROUND_FIX_MAX: f32 = 0.12;

/// The hips + both leg chains as finally posed, for a forward-kinematics read of the boots.
#[derive(Default)]
struct LegChain {
    hips: Transform,
    /// (hip, knee, foot) rotations, left then right.
    leg: [[Quat; 3]; 2],
}

impl LegChain {
    fn take(&mut self, j: Joint, tf: &Transform) {
        match j {
            Joint::Hips => self.hips = *tf,
            Joint::HipL => self.leg[0][0] = tf.rotation,
            Joint::KneeL => self.leg[0][1] = tf.rotation,
            Joint::FootL => self.leg[0][2] = tf.rotation,
            Joint::HipR => self.leg[1][0] = tf.rotation,
            Joint::KneeR => self.leg[1][1] = tf.rotation,
            Joint::FootR => self.leg[1][2] = tf.rotation,
            _ => {}
        }
    }

    /// Where the boots really are: for each, the height (rig space, root origin on the ground) of its
    /// lowest heel / toe point. The one nearest the ground is the planted one — a swinging toe can
    /// dip a finger below it, and following that would float the boot that carries the weight.
    fn boot_height(&self) -> f32 {
        let [a, b] = self.boot_lows();
        if a.abs() < b.abs() { a } else { b }
    }

    /// Each boot's lowest heel / toe point (rig space, root origin on the ground), left then right.
    fn boot_lows(&self) -> [f32; 2] {
        let rig = super::footman::leg_rig();
        let mut lows = [0.0; 2];
        for (i, [hip, knee, foot]) in self.leg.iter().enumerate() {
            let mirror = |v: Vec3| if i == 1 { Vec3::new(-v.x, v.y, v.z) } else { v };
            let hip_r = self.hips.rotation * *hip;
            let knee_r = hip_r * *knee;
            let foot_r = knee_r * *foot;
            let ankle = self.hips.translation + self.hips.rotation * mirror(rig.hip) + hip_r * mirror(rig.knee) + knee_r * mirror(rig.foot);
            let h = rig.ankle_height;
            let low = [Vec3::new(0.0, -h, -rig.heel), Vec3::new(0.0, -h, rig.toe)]
                .into_iter()
                .map(|p| (ankle + foot_r * p).y)
                .fold(f32::MAX, f32::min);
            lows[i] = low;
        }
        lows
    }

    /// The chain of a pose as `hero_anim` writes it (the hips at the footman's own rest height).
    fn of_pose(p: &Pose) -> LegChain {
        let t = p.hips.t.unwrap_or(Vec3::new(0.0, 1.05, 0.0));
        LegChain {
            hips: Transform::from_translation(t + Vec3::Y * (super::model::HIP_REST_Y - 1.05)).with_rotation(p.hips.r),
            leg: [[p.hip_l.r, p.knee_l.r, p.foot_l.r], [p.hip_r.r, p.knee_r.r, p.foot_r.r]],
        }
    }
}

pub fn hero_anim(
    time: Res<Time>,
    player: Res<super::PlayerRes>,
    dir: Res<crate::cinematic::DirectorState>,
    mut hero_q: Query<(&mut Hero, &HeroHealth)>,
    mut parts: Query<(&HeroPart, &mut Transform)>,
    // Edge-detect touchdown (was airborne, now grounded) to stamp a short landing-squash window.
    mut was_air: Local<bool>,
    mut land_at: Local<f32>,
    // Smoothed block weight (0 = open, 1 = full defend) so the brace eases in/out.
    mut block_amt: Local<f32>,
    // Clip cross-fade: the last pose written, the clip it came from, and an in-flight fade.
    mut blend: Local<ClipBlend>,
    // Sword in hand vs. slung on the back (+ the draw/sheathe reach in flight).
    mut sheath: Local<Sheath>,
    // Smoothed hips lift/drop that puts the lowest boot point on the ground (grounding pass).
    mut ground_adj: Local<f32>,
    // First person looks out of this rig: what is slung on the back must not enter the frame.
    fp: Res<super::camera::FirstPerson>,
    mut sword_vis: Query<(&HeroPart, &mut Visibility), (Without<super::BackSword>, Without<super::BackShield>)>,
    mut back_vis: Query<&mut Visibility, (With<super::BackSword>, Without<HeroPart>, Without<super::BackShield>)>,
    // The shield goes with the sword: on the arm in a fight, slung on the back otherwise. This
    // holder is the slung board, and carries it to / from the hand during a draw or sheathe.
    mut back_shield: Query<
        (&super::BackShield, &mut Transform, &mut Visibility),
        (Without<HeroPart>, Without<super::BackSword>),
    >,
) {
    let Ok((mut hero_mut, hh)) = hero_q.single_mut() else { return };
    let now = time.elapsed_secs();
    let dt = time.delta_secs();

    // Touchdown edge → arm the landing squash (before the early-returns so it's always stamped).
    if *was_air && hero_mut.on_ground {
        *land_at = now;
    }
    *was_air = !hero_mut.on_ground;

    // Slain: let the limbs go slack while the body keels over (root rotation owned by health.rs).
    if !player.0.is_alive() {
        for (part, mut tf) in &mut parts {
            tf.rotation = match part.joint {
                Joint::Hips => {
                    tf.translation = Vec3::new(0.0, super::model::HIP_REST_Y, 0.0);
                    Quat::IDENTITY
                }
                Joint::ShoulderL => e3(0.2, 0.0, -0.2),
                Joint::ShoulderR => e3(0.2, 0.0, 0.2),
                Joint::ElbowL | Joint::ElbowR => rx(-0.3),
                Joint::Shield => shield_rest_r(),
                Joint::Sword => sword_rest_r(),
                _ => Quat::IDENTITY,
            };
        }
        return;
    }

    // Ease the block weight toward its target each frame (≈0.15s settle, ~the studio 0.22 ENTER).
    let block_target = if hh.blocking { 1.0 } else { 0.0 };
    *block_amt += (block_target - *block_amt) * (dt * 10.0).min(1.0);
    let block_amt = block_amt.clamp(0.0, 1.0);

    // ── Sword AND shield: drawn for a fight, slung on the back otherwise ──
    let action = hero_mut.attacking || hh.blocking || hero_mut.charge_t >= 0.0 || hero_mut.dash_t >= 0.0;
    // A fight = blows traded lately, or a foe right on top of the hero. (Not merely a hostile
    // somewhere in the soft-lock's 7 u arc: one drifting at the edge of it had the kit coming out
    // and going away every few seconds.)
    let foe_close = hero_mut.soft_pos.is_some_and(|tp| tp.distance(hero_mut.pos) < DRAW_RANGE);
    let fight = foe_close || now < hero_mut.combat_until;
    // Holding Shift in a fight (no swing / block under way) puts the weapons away so the hero can
    // run: no sprinting with sword and shield drawn.
    // In the saddle the hands are on the reins: everything stays slung.
    let want_drawn = !hero_mut.mounted && (action || hero_mut.victory || (fight && !hero_mut.sprint_held));
    sheath.update(dt, want_drawn, action, hero_mut.sprint_held);
    hero_mut.weapons_drawn = sheath.drawn;
    hero_mut.kit_reach = sheath.reach_t.is_some();
    let hero: &Hero = &hero_mut;
    let transit = sheath.shield_transit();
    // First person, sprinting: nothing of the kit may be in the frame, in the hand or slung (the
    // trunk, arms and weapons all swing past the eye at a run).
    // Nor through a draw / sheathe reach: the hands swing over the shoulder past the eye, and the
    // forearms are hidden for it (`camera::fp_body_visibility`) — a blade with no hand would float.
    let fp_sprint = fp.blend > 0.5 && (hero_mut.gait_speed > 3.0 || hero_mut.run_amt > 0.3 || hero_mut.kit_reach);
    for (part, mut vis) in &mut sword_vis {
        if part.joint == Joint::Sword {
            vis.set_if_neq(if sheath.drawn && !fp_sprint { Visibility::Inherited } else { Visibility::Hidden });
        } else if part.joint == Joint::Shield {
            let in_hand = sheath.drawn && transit.is_none() && !fp_sprint;
            vis.set_if_neq(if in_hand { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
    // A sprint leans the trunk far forward, which swings the slung sword and board up past the eye
    // (a dark slab and a blade across the frame): in first person they stay hidden.
    let in_fp = fp.blend > 0.5;
    for mut vis in &mut back_vis {
        vis.set_if_neq(if sheath.drawn || in_fp { Visibility::Hidden } else { Visibility::Inherited });
    }
    let armed = sheath.armed;

    let attack = hero.attacking;
    let gesture = dir.gesture.map(|g| gesture_pose(g, now - dir.gesture_start));

    // Pick the active clip. Actions now LAYER over locomotion so combined moves read right: swinging
    // while running keeps the legs striding (a running attack), and a jump taken at speed becomes a
    // forward leap. (Priority: victory › attack › jump › block-blended locomotion.)
    let moving = hero.moving_amt.clamp(0.0, 1.0);

    // Combat stance feeds two extra locomotion axes (backpedal blend + pelvis-vs-torso twist);
    // both are 0 out of the stance, where this reduces exactly to the footman locomotion. The
    // guard overlay then colours ALL stance locomotion (idle/walk/run) into the ready-to-fight
    // carry — knees bent, shield up, blade at the ready.
    let loco = {
        let mut p = stance_loco_pose(
            now,
            hero.walk_phase,
            moving,
            hero.gait_speed,
            hero.back_amt,
            hero.strafe_twist,
            armed,
            1.0 - armed,
        );
        guard_overlay(&mut p, hero.stance_amt, moving);
        p
    };
    // ── Clip cross-fade ── Each clip below is continuous within itself, but switching between
    // them (stride → jump at take-off, jump → stride on landing, a swing starting mid-run, the
    // dash/charge entering) used to SNAP the whole rig in one frame. On a clip change, freeze the
    // last written pose and ease from it into the new clip over a short, per-transition window.
    let clip = if hero.mounted {
        Clip::Ride
    } else if hero.victory {
        Clip::Victory
    } else if hero.roll_t >= 0.0 {
        Clip::Roll
    } else if hero.dash_t >= 0.0 {
        Clip::Dash
    } else if hero.attacking {
        Clip::Attack(hero.attack_variant)
    } else if hero.charge_t > CHARGE_GRACE && hero.on_ground {
        Clip::Charge
    } else if !hero.on_ground {
        Clip::Air
    } else {
        Clip::Loco
    };
    if clip == Clip::Air && blend.clip != Clip::Air {
        // Take-off: the leg in its swing (not the planted one) drives up into the leap.
        let g = gait_at(hero.gait_speed / hero_model_scale());
        let u_l = (hero.walk_phase / std::f32::consts::TAU).rem_euclid(1.0);
        blend.lead_right = moving > 0.05 && u_l < g.duty;
    }
    let pose = if hero.mounted {
        // Astride the horse: the seat posed in Blender, the trunk pitched by the ride's sway and the
        // head held level against it (the horse seats the hips and owns the bounce).
        let mut p = mixamo::ride();
        p.torso.r *= rx(hero.ride_lean);
        p.head.r *= rx(-0.7 * hero.ride_lean);
        p
    } else if hero.victory {
        victory_pose(now)
    } else if hero.roll_t >= 0.0 {
        // Dodge roll: fold into the tuck through the somersault's core, unfolding at both ends so
        // the dive-in / stand-up carry the transition (the root owns the actual tumble).
        let u = (hero.roll_t / super::movement::ROLL_TIME).clamp(0.0, 1.0);
        let w = smoothstep(u / 0.15) * smoothstep((1.0 - u) / 0.18);
        // The dive-roll clip stretches out in flight; half of the tight tuck keeps the dodge compact.
        let tuck = if mixamo_gait() { roll_pose().lerp(&mixamo::roll(u), 0.5) } else { roll_pose() };
        loco.lerp(&tuck, w)
    } else if hero.dash_t >= 0.0 {
        // Sand Dash slide: play the dash-swipe lunge, easing back into locomotion at the blink's tail.
        let p = (hero.dash_t / super::movement::DASH_TIME).clamp(0.0, 1.0);
        let tail = smoothstep((p - 0.7) / 0.3);
        dash_pose(p).lerp(&loco, tail)
    } else if attack {
        let ap = (hero.attack_t / hero.attack_dur).clamp(0.0, 1.0);
        let atk = if hero.attack_variant < super::combat::HEAVY_VARIANT {
            swordplay::strike_pose(hero.attack_variant, ap, super::combat::HIT_PHASE, hero.attack_dur)
        } else {
            heavy_strike_pose(ap)
        };
        if hero.on_ground && moving > 0.05 {
            action_over_loco(&atk, &loco, moving) // running / walking attack
        } else {
            atk
        }
    } else if hero.charge_t > CHARGE_GRACE && hero.on_ground {
        // Holding a Heavy Strike (the light swing has finished): coil into the overhead wind-up,
        // deepening as the bar fills. Layers over locomotion so you can creep while charging.
        let frac = (hero.charge_t / CHARGE_THRESHOLD).clamp(0.0, 1.0);
        let st = charge_stance(frac, (now * 22.0).sin());
        if moving > 0.05 {
            action_over_loco(&st, &loco, moving)
        } else {
            st
        }
    } else if !hero.on_ground {
        let j = jump_pose(hero.vel_y);
        let j = if moving > 0.05 {
            j.lerp(&leap_pose(hero.vel_y), moving) // running leap
        } else {
            j
        };
        // Lead with whichever leg was swinging forward at take-off, so a running jump continues
        // the stride instead of swapping legs mid-air.
        if blend.lead_right { mirror_legs(&j) } else { j }
    } else if block_amt > 0.001 {
        brace(&loco, &defend_pose(now), block_amt, moving)
    } else if let Some((variant, ended)) = blend.recover {
        // A light swing the chain stopped at settles back through its own recovery; walking off
        // hands the legs (and soon the whole body) straight back to locomotion.
        let (rec, w) = swordplay::recover_pose(variant, now - ended);
        loco.lerp(&action_over_loco(&rec, &loco, moving), w * (1.0 - moving))
    } else {
        loco
    };
    // Recovery bookkeeping: a light swing that just ended starts its recovery; anything else
    // taking over (another swing, a block, a roll, a jump…) or the clip running out ends it.
    let light = hero.attacking && hero.attack_variant < super::combat::HEAVY_VARIANT;
    if let (Some((v, dur)), false) = (blend.swinging, hero.attacking) {
        // The recovery runs on the step's own timeline from its blade contact (the strike already
        // ran on into it inside the swing): stamp when that contact was.
        blend.recover = Some((v, now - (1.0 - super::combat::HIT_PHASE) * dur));
    }
    blend.swinging = light.then_some((hero.attack_variant, hero.attack_dur));
    let busy = hero.attacking || hero.victory || hero.roll_t >= 0.0 || hero.dash_t >= 0.0 || !hero.on_ground
        || hero.charge_t > CHARGE_GRACE || block_amt > 0.001;
    if let Some((v, ended)) = blend.recover {
        if (busy && now > ended) || now - ended > swordplay::recover_len(v) {
            blend.recover = None;
        }
    }

    if clip != blend.clip {
        let dur = match (blend.clip, clip) {
            // A light step eases in over most of its wind-up, so the clip takes over by the
            // contact: a step pressed mid-recovery (or from the guard) starts from wherever the
            // body is instead of snapping to the clip's first key (the last step's follow-through
            // crouch) in four frames. Straight chains start where the last step ended anyway.
            (_, Clip::Attack(v)) if v < super::combat::HEAVY_VARIANT => {
                hero.attack_dur * super::combat::HIT_PHASE * 0.8
            }
            (_, Clip::Attack(_)) | (_, Clip::Dash) => 0.07, // actions must stay snappy
            (Clip::Air, Clip::Loco) => 0.09,                 // landing (the squash carries the rest)
            (Clip::Loco, Clip::Air) => 0.12,                 // take-off into the jump/leap
            (_, Clip::Victory) => 0.3,
            (_, Clip::Ride) | (Clip::Ride, _) => 0.3, // swung up into / down out of the saddle
            _ => 0.14,
        };
        if let Some(last) = blend.last {
            blend.from = Some((last, now, dur));
        }
        blend.clip = clip;
    }
    let pose = match blend.from {
        Some((from, start, dur)) if now - start < dur => from.lerp(&pose, smoothstep((now - start) / dur)),
        _ => {
            blend.from = None;
            pose
        }
    };
    blend.last = Some(pose);
    // The draw/sheathe reach, both hands at once: the sword hand up over the right shoulder to the
    // slung hilt (the swap happens at the top, in `Sheath::update`), the shield hand back and down
    // to the board's rim at the left flank — a natural reach behind the hip, not a wrench over the
    // head. The board itself travels between back and arm (`shield_handoff`, below).
    let pose = match sheath.reach() {
        Some(w) => reach_overlay(pose, w),
        None => pose,
    };

    // Landing squash: a quick crouch the instant the feet hit, easing back over `LAND_RECOVER`.
    let landing = if *land_at <= 0.0 {
        0.0 // no touchdown yet (fresh boot) — don't play an unearned landing crouch
    } else {
        let u = (1.0 - (now - *land_at) / LAND_RECOVER).clamp(0.0, 1.0);
        u * u
    };

    // Final shield-arm joint transforms, for the slung board's hand-off (taken after every overlay).
    let (mut sh_l_tf, mut el_l_tf, mut shield_tf) = (Transform::IDENTITY, Transform::IDENTITY, Transform::IDENTITY);
    // The final hips / leg joints, to find where the boots really end up (see the grounding pass).
    let mut legs = LegChain::default();
    for (part, mut tf) in &mut parts {
        let mut jp = pose.get(part.joint);
        // Only the gait slides the shoulder girdle; every other clip leaves the shoulders' `t`
        // unset, so pin them back to the bind offset instead of keeping the last stride's.
        if jp.t.is_none() && matches!(part.joint, Joint::ShoulderL | Joint::ShoulderR) {
            let rig = super::footman::leg_rig();
            jp.t = Some(if part.joint == Joint::ShoulderL { rig.shoulder } else { rig.shoulder_r });
        }
        if let Some(t) = jp.t {
            // Clips were authored when the hips sat at y = 1.05. The footman's hips are at
            // `HIP_REST_Y` (0.98); shift every absolute hip height by the same delta so crouches
            // keep their depth and the feet stay on the ground.
            tf.translation = if part.joint == Joint::Hips {
                Vec3::new(t.x, t.y + (super::model::HIP_REST_Y - 1.05), t.z)
            } else {
                t
            };
        }
        let mut rot = jp.r;

        // Arm overrides: the Director's staged gesture wins on the arms; otherwise the combat /
        // locomotion clip already in `rot` plays. (First person hides this rig entirely — see
        // `camera::fp_body_visibility` / `viewmodel` — so nothing here is FP-aware.)
        match part.joint {
            Joint::ShoulderR | Joint::ElbowR => {
                if let Some((Some((sh, el)), _)) = gesture {
                    rot = if part.joint == Joint::ElbowR { el } else { sh };
                }
            }
            Joint::ShoulderL | Joint::ElbowL => {
                if let Some((_, Some((sh, el)))) = gesture {
                    rot = if part.joint == Joint::ElbowL { el } else { sh };
                }
            }
            _ => {}
        }
        tf.rotation = rot;

        // Landing squash folded over the locomotion pose right after touchdown (studio positive-knee
        // crouch: hips dip, knees bend, thighs settle back, feet flatten, torso leans in).
        if landing > 0.0 && !attack && hero.on_ground && !hero.mounted {
            match part.joint {
                Joint::Hips => tf.translation.y -= 0.12 * landing,
                Joint::KneeL | Joint::KneeR => tf.rotation *= rx(0.9 * landing),
                Joint::HipL | Joint::HipR => tf.rotation *= rx(-0.35 * landing),
                Joint::FootL | Joint::FootR => tf.rotation *= rx(0.4 * landing),
                Joint::Torso => tf.rotation *= rx(0.25 * landing),
                // Arms throw down to absorb the impact, then spring back as `landing` decays.
                Joint::ShoulderL | Joint::ShoulderR => tf.rotation *= rx(0.3 * landing),
                Joint::ElbowL | Joint::ElbowR => tf.rotation *= rx(-0.25 * landing),
                _ => {}
            }
        }
        match part.joint {
            Joint::ShoulderL => sh_l_tf = *tf,
            Joint::ElbowL => el_l_tf = *tf,
            Joint::Shield => shield_tf = *tf,
            _ => {}
        }
        legs.take(part.joint, &tf);
    }

    // ── Grounding pass ── Every clip above is authored against its own idea of the ground, and the
    // overlays (guard crouch, landing squash, block, a swing's lunge) bend the legs without moving
    // the hips to match, so the knight kept ending up standing a hand's breadth in the air. Whatever
    // the pose, while he is on the ground the lowest boot point is brought onto it: the hips ride
    // the difference (low-passed, so a change of the planted boot doesn't pop).
    // Not the Mixamo run, though: it has real flight (both boots up), which this pulled back down
    // into a squat; it is grounded once, as a whole loop, in `mixamo::direct`.
    // Nor in the saddle: the boots hang in the stirrups, the horse seats the hips.
    let grounded = hero.on_ground && hero.roll_t < 0.0 && hero.dash_t < 0.0 && !hero.mounted;
    let flight = if matches!(clip, Clip::Loco) && mixamo_gait() { mixamo::owns_height(hero.gait_speed / hero_model_scale()) * moving } else { 0.0 };
    let want = if grounded { (-legs.boot_height()).clamp(-GROUND_FIX_MAX, GROUND_FIX_MAX) * (1.0 - flight) } else { 0.0 };
    let k = (dt * if grounded { 40.0 } else { 10.0 }).min(1.0);
    *ground_adj += (want - *ground_adj) * k;
    if ground_adj.abs() > 1e-4 {
        for (part, mut tf) in &mut parts {
            if part.joint == Joint::Hips {
                tf.translation.y += *ground_adj;
            }
        }
    }

    // The slung board: at its back slot, or mid hand-off on its way between the back and the hand
    // (the hand board's pose in the torso frame: shoulder → elbow → fist → shield joint). At the
    // end of a draw it lands exactly on the hand board, so the swap to it is invisible.
    for (slung, mut tf, mut vis) in &mut back_shield {
        let (pose_tf, shown) = match transit {
            Some(s) => {
                let hand = sh_l_tf
                    .mul_transform(el_l_tf)
                    .mul_transform(Transform::from_translation(super::footman::hand_l_bind()))
                    .mul_transform(shield_tf);
                (shield_handoff(&slung.slot, &hand, s), true)
            }
            None => (slung.slot, !sheath.drawn),
        };
        if *tf != pose_tf {
            *tf = pose_tf;
        }
        vis.set_if_neq(if shown && !in_fp { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Staged-gesture arm poses (Director). Returns `(right, left)`, each `Some((shoulder, elbow))` or
/// `None` to leave that arm on its normal animation. `ph` = seconds since the gesture began. Right
/// is the sword arm, left the shield arm. Rough by design — eyeball + nudge against a capture.
fn gesture_pose(g: crate::cinematic::HeroGesture, ph: f32) -> (Option<(Quat, Quat)>, Option<(Quat, Quat)>) {
    use crate::cinematic::HeroGesture::*;
    let raise = (ph / 0.45).clamp(0.0, 1.0);
    let e = raise * raise * (3.0 - 2.0 * raise); // smoothstep
    match g {
        Wave => (Some((e3(-2.55 * e, 0.0, 0.20 + (ph * 5.0).sin() * 0.45 * e), rx(-0.5))), None),
        Salute => (Some((e3(-2.6 * e, 0.0, 0.85 * e), rx(-0.2))), None),
        Point => (Some((e3(-1.6 * e, 0.0, 0.05), rx(-0.1))), None),
        ArmsCrossed => (
            Some((e3(-1.15 * e, 0.0, 0.85 * e), rx(-1.4))),
            Some((e3(-1.15 * e, 0.0, -0.85 * e), rx(-1.4))),
        ),
        Cheer => {
            let pump = (ph * 4.0).sin() * 0.15;
            (
                Some((e3((-2.75 + pump) * e, 0.0, -0.35 * e), rx(-0.3))),
                Some((e3((-2.75 + pump) * e, 0.0, 0.35 * e), rx(-0.3))),
            )
        }
        // A looping chop — the "at work" gesture (villager-staging cinematics).
        Work => {
            let chop = ((ph.max(0.0) * 1.3).fract() * PI).sin();
            (Some((e3(-1.2 - chop * 0.5, 0.0, 0.1), rx(-0.6 + chop * 0.3))), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forward kinematics of one posed leg (rig space, ground at y = 0): (ankle, boot rotation).
    fn ankle(p: &Pose, right: bool) -> (Vec3, Quat) {
        let rig = super::super::footman::leg_rig();
        let (hip, knee, foot) = if right {
            (p.hip_r, p.knee_r, p.foot_r)
        } else {
            (p.hip_l, p.knee_l, p.foot_l)
        };
        let mirror = |v: Vec3| if right { Vec3::new(-v.x, v.y, v.z) } else { v };
        let hip_rotation = p.hips.r * hip.r;
        let knee_rotation = hip_rotation * knee.r;
        let foot_rotation = knee_rotation * foot.r;
        let hips = p.hips.t.unwrap() + Vec3::Y * (super::super::model::HIP_REST_Y - 1.05);
        let ankle = hips + p.hips.r * mirror(rig.hip)
            + hip_rotation * mirror(rig.knee) + knee_rotation * mirror(rig.foot);
        (ankle, foot_rotation)
    }

    /// The boot's heel and toe contact points.
    fn heel_toe(p: &Pose, right: bool) -> (Vec3, Vec3) {
        let rig = super::super::footman::leg_rig();
        let (a, r) = ankle(p, right);
        let h = rig.ankle_height;
        (a + r * Vec3::new(0.0, -h, -rig.heel), a + r * Vec3::new(0.0, -h, rig.toe))
    }

    #[test]
    #[ignore]
    fn dump_run_cycle() {
        let rig = super::super::footman::leg_rig();
        for (e, h) in super::super::footman::arm_bind() {
            println!("arm bind: upper {e:?} fore {h:?} elbow flex {:.1} deg", e.angle_between(h).to_degrees());
        }
        println!("rig thigh {} shin {} ankle_h {} toe {} heel {} hip {:?} scale {}", rig.knee.length(), rig.foot.length(), rig.ankle_height, rig.toe, rig.heel, rig.hip, hero_model_scale());
        let speeds: Vec<f32> = std::env::var("DUMP_V").map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or(vec![4.4, 5.04]);
        for v in speeds {
            let g = gait_at(v / hero_model_scale());
            println!("v={v} run={} sprint={} duty={} sweep={} steps/s={}", g.run, g.sprint, g.duty, g.sweep, gait_phase_rate_curves(v) / PI);
            for i in 0..40 {
                let ph = TAU * i as f32 / 40.0;
                let p = footman_gait_curves(ph, v, 0.0, 1.0);
                let (lh, lt) = heel_toe(&p, false);
                let (rh, rt) = heel_toe(&p, true);
                let ang = |hip: &Jp, knee: &Jp| {
                    let h = p.hips.r * hip.r;
                    let th = h * Vec3::NEG_Y;
                    let sh = h * knee.r * Vec3::NEG_Y;
                    (th.z.atan2(-th.y).to_degrees(), th.angle_between(sh).to_degrees())
                };
                let arm = |sh: &Jp, el: &Jp, bind: (Vec3, Vec3)| {
                    let t = p.hips.r * p.torso.r * sh.r;
                    let up = t * bind.0;
                    let fore = t * el.r * bind.1;
                    (up.z.atan2(-up.y).to_degrees(), fore.z.atan2(-fore.y).to_degrees())
                };
                let ab = super::super::footman::arm_bind();
                let (ru, rf) = arm(&p.sh_r, &p.el_r, ab[0]);
                let (lu, lf) = arm(&p.sh_l, &p.el_l, ab[1]);
                if i % 4 == 0 { println!("   arms: R upper {ru:.0} fore {rf:.0} | L upper {lu:.0} fore {lf:.0}"); }
                let (tl, kl) = ang(&p.hip_l, &p.knee_l);
                let (tr, kr) = ang(&p.hip_r, &p.knee_r);
                println!("u={:.3} hips_y={:.3} L low={:.3} z={:.3} th={tl:.0} kn={kl:.0}  R low={:.3} z={:.3} th={tr:.0} kn={kr:.0}", i as f32 / 40.0, p.hips.t.unwrap().y, lh.y.min(lt.y), lh.z, rh.y.min(rt.y), rh.z);
            }
        }
    }

    /// The measured idle drifts the hips from foot to foot; the boots must stay put, flat on the
    /// ground, with no knee locked past straight or sunk into a crouch — through the mirrored
    /// second pass and both cross-overs too.
    #[test]
    fn idle_keeps_the_boots_planted() {
        let rest = [heel_toe(&footman_idle_pose(0.0), false), heel_toe(&footman_idle_pose(0.0), true)];
        let mut knee_max = 0.0_f32;
        for i in 0..800 {
            let p = footman_idle_pose(i as f32 * 0.04);
            for (side, right) in [(0, false), (1, true)] {
                let (h, t) = heel_toe(&p, right);
                for (a, b) in [(h, rest[side].0), (t, rest[side].1)] {
                    assert!(a.y.abs() < 0.012, "idle boot off the ground: t={}, y={}", i as f32 * 0.04, a.y);
                    assert!(a.xz().distance(b.xz()) < 0.012, "idle boot slides: t={}, {a} vs {b}", i as f32 * 0.04);
                }
            }
            knee_max = knee_max.max(p.knee_l.r.angle_between(Quat::IDENTITY)).max(p.knee_r.r.angle_between(Quat::IDENTITY));
        }
        assert!(knee_max.to_degrees() < 30.0, "idle knee sinks to {} deg", knee_max.to_degrees());
    }

    /// World ground speeds spanning a creep, a walk, the base jog and the sprint.
    const SPEEDS: [f32; 7] = [0.5, 1.4, 2.4, 3.5, 4.4, 6.125, 7.0];

    #[test]
    fn footman_boots_stay_grounded_through_every_gait() {
        for v in SPEEDS {
            let g = gait_at(v / hero_model_scale());
            let mut max_lift = 0.0_f32;
            for frame in 0..240 {
                let phase = std::f32::consts::TAU * frame as f32 / 240.0;
                let p = footman_gait_curves(phase, v, 1.0, 0.0);
                for right in [false, true] {
                    let u = (phase / std::f32::consts::TAU + if right { 0.5 } else { 0.0 }).rem_euclid(1.0);
                    let (heel, toe) = heel_toe(&p, right);
                    let low = heel.y.min(toe.y);
                    assert!(low >= -0.02, "boot penetrates ground: v={v}, u={u}, y={low}");
                    if u < g.duty {
                        assert!(low.abs() <= 0.02, "planted boot floats: v={v}, u={u}, y={low}");
                    }
                    max_lift = max_lift.max(low);
                }
            }
            assert!(max_lift > 0.04, "recovery foot must lift, not skate (v={v})");
        }
    }

    /// The Mixamo gait: whichever boot carries the weight stays on the ground and does not slide as
    /// the body travels (the playback rate is the foot-lock contract), and the swing foot lifts.
    #[test]
    fn mixamo_gait_boots_planted_and_locked() {
        assert!(mixamo_gait(), "Mixamo is the default gait");
        let mut fails = Vec::new();
        for v in SPEEDS {
            let vm = v / hero_model_scale();
            let rate = gait_phase_rate(v);
            let dt = 1.0 / 240.0;
            let (mut worst_slip, mut worst_low, mut max_lift) = (0.0_f32, 0.0_f32, 0.0_f32);
            let (mut slip_sum, mut slip_n) = (0.0_f32, 0);
            for frame in 0..300 {
                let phase = std::f32::consts::TAU * frame as f32 / 300.0;
                let a = footman_gait(phase, v, 1.0, 0.0);
                let b = footman_gait(phase + rate * dt, v, 1.0, 0.0);
                for right in [false, true] {
                    let (ha, ta) = heel_toe(&a, right);
                    let (hb, tb) = heel_toe(&b, right);
                    let low = ha.y.min(ta.y);
                    worst_low = worst_low.min(low);
                    max_lift = max_lift.max(low);
                    if ha.y.max(ta.y) < 0.02 && hb.y.max(tb.y) < 0.02 {
                        let (pa, pb) = if ha.y <= ta.y { (ha, hb) } else { (ta, tb) };
                        let slip = ((pb.z - pa.z) / dt + vm).abs() / vm.max(0.5);
                        worst_slip = worst_slip.max(slip);
                        slip_sum += slip;
                        slip_n += 1;
                    }
                }
            }
            println!("mixamo v={v} vm={vm:.2} rate={rate:.2} worst_low={worst_low:.3} max_lift={max_lift:.3} slip max {worst_slip:.2} mean {:.2} over {slip_n}", slip_sum / slip_n.max(1) as f32);
            fails.push((v, worst_low, max_lift, slip_sum / slip_n.max(1) as f32));
        }
        for (v, low, lift, slip) in fails {
            assert!(low > -0.04, "boot sinks into the ground: v={v}, y={low}");
            assert!(lift > 0.03, "swing boot must lift: v={v}");
            assert!(slip < 0.2, "planted boot skates: v={v}, mean slip {slip}");
        }
    }

    /// The moonwalk regression: while a boot is planted, its contact edge must stay put on the
    /// ground as the body travels — the rig-space sweep cancels the root's real speed.
    #[test]
    fn planted_boot_does_not_skate() {
        for v in SPEEDS {
            let vm = v / hero_model_scale();
            let g = gait_at(vm);
            let rate = gait_phase_rate_curves(v);
            let dt = 1.0 / 240.0;
            for frame in 0..200 {
                let phase = std::f32::consts::TAU * frame as f32 / 200.0;
                for right in [false, true] {
                    let u = (phase / std::f32::consts::TAU + if right { 0.5 } else { 0.0 }).rem_euclid(1.0);
                    let u2 = u + rate * dt / std::f32::consts::TAU;
                    if u < 0.02 || u2 > g.duty - 0.02 {
                        continue; // the touchdown / lift-off frames themselves
                    }
                    let a = heel_toe(&footman_gait_curves(phase, v, 1.0, 0.0), right);
                    let b = heel_toe(&footman_gait_curves(phase + rate * dt, v, 1.0, 0.0), right);
                    // Whichever edge carries the weight (the lower one; both when flat).
                    let (pa, pb) = if a.0.y <= a.1.y { (a.0, b.0) } else { (a.1, b.1) };
                    let slip = (pb.z - pa.z) / dt + vm;
                    assert!(slip.abs() < 0.06 * vm + 0.05, "planted boot skates: v={v}, u={u}, slip={slip} m/s of {vm}");
                    // …nor shuffle sideways with the pelvis sway (the "wobbly on its legs" read).
                    let side = (pb.x - pa.x) / dt;
                    assert!(side.abs() < 0.08, "planted boot shuffles sideways: v={v}, u={u}, {side} m/s");
                }
            }
        }
    }

    /// Swing-leg joint angles stay inside human running ranges (the ankle-path IK once swung the
    /// sprinting thigh to 136° and folded the knee to 154°). These read the hip joint's local axis,
    /// a few degrees off the hip→knee line: the recorded walk's ~36° thigh reads ~44° here.
    #[test]
    fn swing_joint_angles_stay_human() {
        for (v, hip_max, knee_max) in [(1.4_f32, 46.0_f32, 75.0_f32), (3.5, 55.0, 115.0), (6.125, 75.0, 132.0)] {
            let (mut hi, mut kn) = (0.0_f32, 0.0_f32);
            for i in 0..200 {
                let p = footman_gait_curves(std::f32::consts::TAU * i as f32 / 200.0, v, 0.0, 1.0);
                let hip = p.hips.r * p.hip_l.r;
                let thigh = hip * Vec3::NEG_Y;
                let shin = hip * p.knee_l.r * Vec3::NEG_Y;
                hi = hi.max(thigh.z.atan2(-thigh.y).to_degrees());
                kn = kn.max(thigh.angle_between(shin).to_degrees());
            }
            assert!(hi < hip_max && hi > 0.5 * hip_max, "v={v}: peak hip flexion {hi}°");
            assert!(kn < knee_max && kn > 0.5 * knee_max, "v={v}: peak knee flexion {kn}°");
        }
    }

    #[test]
    fn sword_draws_for_a_fight_and_slings_when_calm() {
        let dt = 1.0 / 60.0;
        let mut sh = Sheath::default();
        assert!(!sh.drawn);
        // A swing from the slung carry: in hand at once, no reach flourish.
        sh.update(dt, true, true, false);
        assert!(sh.drawn && sh.reach().is_none());
        // Calm: stays out through the delay, then the reach slings it at its midpoint.
        let mut t = 0.0;
        while sh.drawn {
            sh.update(dt, false, false, false);
            t += dt;
            assert!(t < SHEATHE_DELAY + SHEATHE_REACH, "never sheathed");
        }
        assert!(t > SHEATHE_DELAY, "sheathed mid-fight");
        // A foe ringed (no action yet): drawn via the reach.
        for _ in 0..60 {
            sh.update(dt, false, false, false);
        }
        sh.update(dt, true, false, false);
        assert!(!sh.drawn && sh.reach().is_some());
        for _ in 0..60 {
            sh.update(dt, true, false, false);
        }
        assert!(sh.drawn && sh.reach().is_none());
        // Sprinting away slings it sooner.
        let mut t = 0.0;
        while sh.drawn {
            sh.update(dt, false, false, true);
            t += dt;
        }
        assert!(t < SHEATHE_DELAY);
    }

    #[test]
    fn shield_goes_with_the_sword() {
        let dt = 1.0 / 60.0;
        let mut sh = Sheath::default();
        assert!(sh.shield_transit().is_none() && !sh.drawn, "starts slung");
        // A fight: one reach brings both out; the board travels back → hand without jumping.
        let mut last = 0.0_f32;
        let mut seen = false;
        for _ in 0..120 {
            sh.update(dt, true, false, false);
            if let Some(s) = sh.shield_transit() {
                assert!(s >= last - 1e-6 && s - last < 0.2, "board jumped {last} → {s}");
                last = s;
                seen = true;
            }
        }
        assert!(seen && sh.drawn && sh.shield_transit().is_none(), "drawn with the shield on the arm");
        assert!(last > 0.99, "the hand-off ended short of the hand ({last})");
        // Calm: both go away together, the board settling back onto the back.
        let mut last = 1.0_f32;
        for _ in 0..((SHEATHE_DELAY + SHEATHE_REACH + 0.2) / dt) as usize {
            sh.update(dt, false, false, false);
            if let Some(s) = sh.shield_transit() {
                assert!(s <= last + 1e-6, "board went back toward the hand");
                last = s;
            }
        }
        assert!(!sh.drawn && sh.shield_transit().is_none() && last < 0.01, "sheathed with the shield on the back");
    }

    #[test]
    fn shield_handoff_swings_round_the_left_flank() {
        let slot = Transform::from_translation(Vec3::new(0.0, 0.2, -0.2));
        let hand = Transform::from_translation(Vec3::new(-0.3, -0.1, 0.12));
        assert!(shield_handoff(&slot, &hand, 0.0).translation.distance(slot.translation) < 1e-4);
        assert!(shield_handoff(&slot, &hand, 1.0).translation.distance(hand.translation) < 1e-4);
        for i in 1..10 {
            let t = shield_handoff(&slot, &hand, i as f32 / 10.0).translation;
            assert!(t.x < 0.0, "passed the right side / the spine at {i}: {t}");
            assert!(Vec2::new(t.x, t.z).length() > 0.19, "cut through the body at {i}: {t}");
        }
    }

    #[test]
    fn cadence_reads_human() {
        let steps = |v: f32| gait_phase_rate_curves(v) / PI; // two steps per 2π
        let walk = steps(super::super::SPEED);
        assert!((1.8..2.6).contains(&walk), "walk cadence {walk}");
        let jog = steps(3.2);
        assert!((2.6..3.6).contains(&jog) && jog > walk, "jog cadence {jog}");
        let sprint = steps(super::super::SPEED * super::super::SPRINT_MULT);
        assert!((3.0..4.6).contains(&sprint) && sprint > jog, "sprint cadence {sprint}");
        assert_eq!(gait_phase_rate_curves(0.0), 0.0);
    }

    #[test]
    fn footman_gait_is_periodic_and_blends_back_to_idle() {
        for v in SPEEDS {
            let a = footman_gait_curves(0.0, v, 1.0, 0.0);
            let b = footman_gait_curves(std::f32::consts::TAU, v, 1.0, 0.0);
            for j in [Joint::Hips, Joint::Torso, Joint::ShoulderR, Joint::HipL, Joint::HipR, Joint::KneeL, Joint::KneeR, Joint::FootL, Joint::FootR] {
                let d = a.get(j).r.angle_between(b.get(j).r);
                assert!(d < 0.002, "gait not periodic at v={v}: joint {} off by {d}", j as u8);
            }
            let stopped = stance_loco_pose(2.0, 1.7, 0.0, v, 0.0, 0.0, 1.0, 0.0);
            let idle = footman_idle_pose(2.0);
            assert!(stopped.hips.t.unwrap().distance(idle.hips.t.unwrap()) < 0.0001);
            assert!(stopped.knee_l.r.angle_between(idle.knee_l.r) < 0.001);
        }
    }

    /// Opposite arm to the forward leg — the old gait swung the sword arm WITH the same-side leg.
    #[test]
    fn arms_counter_swing_the_legs() {
        for v in SPEEDS {
            // Left heel strike: left boot out front → right (sword) arm forward (negative X).
            let p = footman_gait_curves(0.0, v, 1.0, 0.0);
            assert!(heel_toe(&p, false).0.z > heel_toe(&p, true).0.z);
            // Where each shoulder puts its elbow (the bind upper arm, turned).
            let [r, l] = super::super::footman::arm_bind();
            assert!((p.sh_r.r * r.0).z > (p.sh_l.r * l.0).z, "sword arm must lead with the left leg (v={v})");
        }
    }

    #[test]
    fn combat_guard_preserves_moving_footman_support() {
        for back in [0.0, 1.0] {
            for twist in [-0.7, 0.0, 0.7] {
                for frame in 0..120 {
                    let phase = std::f32::consts::TAU * frame as f32 / 120.0;
                    let mut p = stance_loco_pose(2.0, phase, 1.0, 2.6, back, twist, 1.0, 0.0);
                    guard_overlay(&mut p, 1.0, 1.0);
                    for right in [false, true] {
                        let (heel, toe) = heel_toe(&p, right);
                        assert!(heel.y.min(toe.y) >= -0.02, "guard must not drive the moving boot into the ground");
                    }
                }
            }
        }
    }

    /// Regression (playtest: "sometimes when he sheathes the sword he twists the arm backward"):
    /// the draw/sheathe reach must carry the sword arm UP THE FRONT to the slung hilt from every
    /// pose it can start from — whatever gait phase, speed, carry or backpedal. The hilt target is
    /// ~180 deg of shoulder pitch away from a hanging arm, so a plain shortest-arc slerp picked the
    /// front or the BACK route depending on which way the gait had the arm swinging that frame.
    #[test]
    fn sheathe_reach_raises_the_sword_arm_up_the_front() {
        let bind = super::super::footman::arm_bind()[0];
        // Sagittal pitch of the right upper arm in the torso frame: 0 = hanging, +90 = forward,
        // ±180 = overhead, −90 = swung back behind the body.
        let pitch = |p: &Pose| {
            let up = p.hips.r * p.torso.r * p.sh_r.r * bind.0;
            up.z.atan2(-up.y).to_degrees()
        };
        let mut starts = Vec::new();
        for i in 0..24 {
            let wp = TAU * i as f32 / 24.0;
            for (m, speed) in [(0.0, 0.0), (0.5, 0.8), (1.0, 1.4), (1.0, 5.0)] {
                for armed in [0.0, 1.0] {
                    for back in [0.0, 1.0] {
                        starts.push(stance_loco_pose(i as f32 * 0.37, wp, m, speed, back, 0.0, armed, 1.0 - armed));
                    }
                }
            }
        }
        for vy in [-6.0, -2.0, 0.0, 3.0, 6.5] {
            starts.push(jump_pose(vy));
        }
        let mut worst = f32::INFINITY;
        for (k, start) in starts.iter().enumerate() {
            let mut prev = pitch(start);
            let start_pitch = prev;
            let mut unwrapped = prev;
            let mut lowest = prev;
            for s in 1..=60 {
                let a = pitch(&reach_overlay(*start, s as f32 / 60.0));
                let mut d = a - prev;
                if d > 180.0 {
                    d -= 360.0;
                } else if d < -180.0 {
                    d += 360.0;
                }
                unwrapped += d;
                prev = a;
                lowest = lowest.min(unwrapped);
            }
            worst = worst.min(lowest);
            assert!(
                unwrapped > 120.0 && lowest > start_pitch.min(-60.0) - 15.0, // a sprint's arm may start well back; it must not swing further back
                "start #{k}: the sword arm went the back way (ended at {unwrapped:.0} deg, lowest {lowest:.0})"
            );
        }
        println!("lowest arm pitch over every reach: {worst:.0} deg");
    }
}
