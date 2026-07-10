//! Hero limb animation — a faithful 1:1 port of the user's three.js `updateKnightAnimation`
//! (low-poly-knight-studio). Every studio clip is reproduced verbatim: **idle / walk / run / jump /
//! defend / attack1 (overhead chop) / attack2 (horizontal slash) / attack3 (forward thrust) /
//! victory**. The studio builds each frame imperatively — reset every joint to rest, then a `switch`
//! case sets some — so we mirror that: [`rest`] seeds a full [`Pose`] table and each clip function
//! mutates the fields it touches. The per-frame system then writes the chosen pose onto the rig
//! joints.
//!
//! Game adaptations (kept minimal so the *poses* stay verbatim):
//! - **walk/run** read `walk_phase` for their `cycle` (gait locked to real movement speed, not
//!   wall-clock) and are cross-faded by `moving_amt` (idle→gait) and `run_amt` (walk→run).
//! - **jump** — the studio faked the hop by sliding `hips.y`; here the **real jump physics own the
//!   height** (the root's world Y), so the studio `height` (0 launch/landing, 1 apex) is recovered
//!   from the hero's vertical speed and fed into the studio's exact airtime joint formulas.
//! - **attack** — our one-shot swing (`attack_t/ATTACK_DURATION`) is split into the studio's
//!   wind/strike/recovery phases (strike starts at `HIT_PHASE` so the blade meets the damage frame).
//! - **defend** — eased in/out by a smoothed `block_amt` instead of the studio's `time`-since-block.
//! On top sit our own layers: the Director's staged gestures (arms), the first-person viewmodel
//! raise, the touchdown landing-squash, and a slack keel-over on death.

use std::f32::consts::PI;

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
fn idle_pose(t: f32) -> Pose {
    let breath = (t * 2.2).sin();
    let s11 = (t * 1.1).sin();
    let c11 = (t * 1.1).cos();
    // Slow weight shift (≈14s full cycle): a person at rest settles onto one hip, then the other.
    // The pelvis slides + rolls a touch, the torso counter-rolls to keep the head over the feet,
    // and the legs take up the slack — kills the "statue with a breathing chest" read.
    let w = (t * 0.45).sin();
    let mut p = rest();
    p.hips = Jp {
        t: Some(Vec3::new(w * 0.03, 1.05 + breath * 0.015 - w.abs() * 0.008, 0.0)),
        r: e3(0.0, s11 * 0.02, w * 0.035),
    };
    p.torso = Jp::r(e3(breath * 0.01, -s11 * 0.012, -w * 0.028));
    p.head = Jp::r(e3(-breath * 0.015, s11 * 0.04, s11 * 0.006 - w * 0.012));
    p.sh_l = Jp::r(e3(breath * 0.05 + 0.1, 0.0, -0.15 + c11 * 0.02 + w * 0.02));
    p.el_l = Jp::r(rx(-0.5 - breath * 0.03));
    p.sh_r = Jp::r(e3(breath * 0.05 + 0.12, 0.0, 0.15 - c11 * 0.02 + w * 0.02));
    p.el_r = Jp::r(rx(-0.4 - breath * 0.02));
    // Stance leg straightens, free leg softens at the knee as the weight rides across.
    p.hip_l = Jp::r(e3(0.0, 0.0, w.max(0.0) * 0.05));
    p.hip_r = Jp::r(e3(0.0, 0.0, w.min(0.0) * 0.05));
    p.knee_l = Jp::r(rx((-w).max(0.0) * 0.08));
    p.knee_r = Jp::r(rx(w.max(0.0) * 0.08));
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
    // Arms: relaxed pendulum swing with a soft elbow that folds deeper on the forward swing
    // (negative shoulder-X = forward; the `sin` term goes negative with it, bending the elbow) —
    // a straight arm swinging from the shoulder is the classic robot tell.
    p.sh_l = Jp::r(e3(r.sin() * 0.42 + 0.05, 0.0, -0.36));
    p.el_l = Jp::r(rx(-0.42 + r.sin() * 0.3));
    p.sh_r = Jp::r(e3(l.sin() * 0.55 + 0.1, 0.0, 0.15 + c.cos() * 0.02));
    p.el_r = Jp::r(rx(-0.32 + l.sin() * 0.32));
    p.shield = Jp { t: Some(SHIELD_GAIT_T), r: shield_gait_r() };
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
    p.sh_l = Jp::r(e3(r.sin() * 0.38 + 0.08, 0.0, -0.36));
    p.el_l = Jp::r(rx(-0.65 + r.sin() * 0.14));
    p.sh_r = Jp::r(e3(l.sin() * 0.62 + 0.15, 0.0, 0.18));
    p.el_r = Jp::r(rx(-0.6 + l.sin() * 0.22));
    p.shield = Jp { t: Some(SHIELD_GAIT_T), r: shield_gait_r() };
    p
}

/// idle → (walk → run by `run`) → blended toward idle by `m` (moving_amt).
pub(crate) fn loco_pose(t: f32, wp: f32, m: f32, run: f32) -> Pose {
    let gait = walk_pose(wp).lerp(&run_pose(wp), run);
    idle_pose(t).lerp(&gait, m)
}

/// Combat-stance locomotion: [`loco_pose`] with two extra axes driven by `movement` —
/// `back` (0..1) cross-fades toward the gait played in REVERSE phase (a backpedal: the hero
/// steps backward while still facing the foe), and `twist` (radians) yaws the pelvis+legs
/// toward the movement while the torso/head counter-rotate to stay square on the target — the
/// classic lower-body-aims-along-movement / upper-body-faces-target split every lock-on game
/// uses, here as a differential yaw on the existing joints.
pub(crate) fn stance_loco_pose(t: f32, wp: f32, m: f32, run: f32, back: f32, twist: f32) -> Pose {
    let mut p = loco_pose(t, wp, m, run);
    if back > 0.001 {
        // The same cycle run backward reads as stepping back; the mid-blend "gather step" as the
        // two phases cancel is exactly what a person does reversing direction.
        p = p.lerp(&loco_pose(t, -wp, m, run), back.clamp(0.0, 1.0));
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
fn guard_overlay(p: &mut Pose, amt: f32) {
    let a = amt.clamp(0.0, 1.0);
    if a <= 0.001 {
        return;
    }
    // Weight drops: hips sink + tip forward slightly, knees take the bend, thighs sit back.
    if let Some(t) = p.hips.t {
        p.hips.t = Some(t - Vec3::new(0.0, 0.045 * a, 0.0));
    }
    p.hips.r = e3(0.05 * a, 0.0, 0.0) * p.hips.r;
    p.torso.r = e3(0.09 * a, 0.0, 0.0) * p.torso.r;
    p.knee_l.r = p.knee_l.r * rx(0.22 * a);
    p.knee_r.r = p.knee_r.r * rx(0.20 * a);
    p.hip_l.r = p.hip_l.r * rx(-0.11 * a);
    p.hip_r.r = p.hip_r.r * rx(-0.10 * a);
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
    let hold = (t * 5.5).sin() * 0.012;
    let mut p = rest();
    p.hips = Jp { t: Some(Vec3::new(0.0, 0.96 + hold, 0.05)), r: e3(0.05, 0.15, 0.0) };
    p.torso = Jp::r(e3(0.1, -0.05, 0.0));
    p.head = Jp::r(e3(-0.08, -0.1, 0.0));
    p.hip_l = Jp::r(e3(-0.35, 0.1, -0.08));
    p.knee_l = Jp::r(rx(0.45));
    p.foot_l = Jp::r(rx(-0.15));
    p.hip_r = Jp::r(e3(-0.2, -0.1, 0.15));
    p.knee_r = Jp::r(rx(0.25));
    p.foot_r = Jp::r(rx(-0.1));
    p.sh_l = Jp::r(e3(-0.6, 0.0, -0.4));
    p.el_l = Jp::r(rx(-0.8));
    // Shield braced flat in front (studio blockPos/blockRot).
    p.shield = Jp { t: Some(Vec3::new(0.0, 0.0, 0.1)), r: e3(PI / 2.0, 0.0, 0.0) };
    p.sh_r = Jp::r(e3(0.15, 0.1, 0.25));
    p.el_r = Jp::r(rx(-0.5));
    p.sword = Jp::r(e3(2.4, 0.0, 0.0));
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
    // The bow itself: from the at-ease carry along the forearm to UPRIGHT in the draw. With the
    // arm raised forward (hand-local −Y ≈ world-forward, +Z ≈ world-up), pitching the mesh +X by
    // +π/2 stands the stave (mesh +Y) vertical and turns the string (mesh −Z) back at the cheek.
    po.shield = Jp {
        t: Some(Vec3::new(0.0, -0.02, 0.05)),
        r: Jp::r(e3(0.12, -1.5, 0.0)).r.slerp(e3(1.55, 0.0, 0.0), draw),
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

    // Settle everything back to the rest carry after the loose.
    let out = rest();
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

/// Per-variant FP viewmodel swing shaping (rig-space radians, pre-mirror). The third-person attack
/// clips can't play on the FP arms (they orbit the lens — see the sword-arm note in [`hero_anim`]),
/// so each attack variant gets its own compact camera-framed envelope instead of one shared
/// wind→punch: the overhead chop cocks the blade high and drives it DOWN the frame, the horizontal
/// slash whips it ACROSS, the thrust holds the blade level and punches the whole arm forward, and
/// the Heavy is the chop writ large. Each `(back, fwd)` pair scales the wind-up coil and the strike
/// punch on one joint axis.
struct FpSwing {
    /// Shoulder X — raise on the cock / drive on the punch.
    sh_x: (f32, f32),
    /// Shoulder Y — lateral pull / cross-frame sweep.
    sh_y: (f32, f32),
    /// Elbow fold on the cock / extension on the punch.
    el: (f32, f32),
    /// Wrist X — blade tips up-back / sweeps down-forward.
    sw_x: (f32, f32),
    /// Wrist Y — blade cocks out / whips across the frame.
    sw_y: (f32, f32),
    /// EDGE ROLL about the BLADE'S OWN AXIS (mesh +Y), applied as a right-multiplied local
    /// `Quat::from_rotation_y` — NOT a wrist-euler Z term, which in XYZ order deflects the blade
    /// sideways off its line (the first cut of this feature pointed the thrust's blade vertical).
    /// Pronate on the cock / turn the edge over through the cut; the thrust corkscrews.
    sw_roll: (f32, f32),
    /// Mid-STRIKE perpendicular bulge on the shoulder `(X, Y)`, scaled by `sin(π·strike-p)` — 0 at
    /// both strike endpoints, peaking mid-cut. Bows the blade's path into a crescent instead of a
    /// straight endpoint-to-endpoint lerp (the old "toporny"/stick-swing read).
    arc: (f32, f32),
}

fn fp_swing(variant: u8) -> FpSwing {
    match variant {
        // attack2 — horizontal slash: a falling cross-cut — the blade cocks out to the sword side
        // and sweeps across the MID frame. (Probe history: an all-shoulder sweep marched the
        // forearm through the lens; an all-wrist one read as a rising poke — the travel is now
        // SPLIT between them, with sw_x keeping the blade on the level-to-down band.)
        1 => FpSwing {
            // Cross-travel lives almost entirely in the WRIST (sw_y): the FP eye sits ~0.2u from
            // the shoulder, so ANY real shoulder-Y sweep (0.9 originally, still at 0.6) parks the
            // forearm ON the lens — whole frames blacked out mid-slash. The hilt now holds
            // low-right; the blade whips across, sw_x riding it down onto the level band (the big
            // wrist yaw alone reads as a rising poke through the tilted hand frame).
            // BOTH sh_x endpoints DROP (positive): the ready base holds the fist high near the
            // face (-0.70 raise / -1.05 fold), so any cross-cut through it passes the hand
            // straight THROUGH the lens — the mid-slash full-frame blackout every earlier cut
            // of this table reproduced. The whole slash now runs at WAIST height (wind included;
            // the classic FP belly-cut), the blade doing the cross-travel in the WRIST (sw_y)
            // while sh_y.0 cocks the wind outward-right.
            sh_x: (0.35, 0.55),
            sh_y: (0.20, 0.25),
            el: (0.10, 0.55),
            sw_x: (-0.10, 0.45),
            sw_y: (0.15, 1.15),
            sw_roll: (-0.35, 0.60), // edge cocked over on the wind, turned through the cross-cut
            arc: (0.0, 0.0),
        },
        // attack3 — forward thrust: the wind folds the elbow deep (hilt drawn to the ribs, blade
        // kept LEVEL at the target), the strike EXTENDS the whole arm at the frame centre — the
        // punch reads in the reach, so the wrist barely moves (X≈2.56 is the probe-solved
        // blade-level-forward angle for the raised FP arm; sw_x holds it there through the punch).
        2 => FpSwing {
            // sh_x.1 NEGATIVE: the punch RAISES the extending arm to chest line, so the blade
            // reaches across the lower-centre frame instead of vanishing under the bottom edge
            // as the unfolding elbow drops the hand. sw_x.0 pitches the cocked blade forward,
            // countering the deep elbow fold that otherwise stands it vertical at the frame edge.
            sh_x: (-0.10, -0.10),
            sh_y: (-0.10, 0.25),
            el: (-0.60, 1.05),
            sw_x: (0.55, 0.0),
            sw_y: (0.15, 0.0),
            sw_roll: (0.10, 0.90), // corkscrew: a quarter-turn about the blade axis through the punch
            arc: (0.0, 0.0),       // a thrust IS the straight line — no crescent
        },
        // Heavy Strike — the overhead chop writ large: hauled higher, smashed hard down the middle
        // (this shape is also what the held charge coils into). sw_x.1 stops at +0.55: the probe
        // showed +1.05 swung the blade PAST straight-down into pointing back at the camera.
        v if v == super::combat::HEAVY_VARIANT => FpSwing {
            // Drive capped (sh_x/sw_x .1) so the hilt lands in the LOWER FRAME, not past its
            // bottom edge — a smash that exits the screen entirely reads as nothing at all.
            sh_x: (-0.50, 0.32),
            // sh_y.1 pulls the extended smash toward frame CENTRE (safe from the lens here —
            // the arm is near-straight by then), so the blow lands down the middle, not along
            // the right edge.
            sh_y: (-0.05, 0.28),
            el: (-0.30, 0.90),
            sw_x: (-0.95, 0.45),
            sw_y: (0.05, 0.20),
            sw_roll: (-0.20, 0.45),
            arc: (0.0, 0.40), // the smash bows outward mid-drop — an axe-arc, not an elevator
        },
        // attack1 — overhead chop: blade cocked high over the shoulder, driven DOWN the frame —
        // the Heavy's arc at a lighter scale (the first cut played the whole chop below the
        // bottom frame edge; raising the shoulder and capping sw_x keeps the sweep IN frame).
        _ => FpSwing {
            sh_x: (-0.50, 0.30),
            sh_y: (-0.05, 0.22), // slight centre-pull at extension (see the Heavy note)
            el: (-0.32, 0.80),
            sw_x: (-0.85, 0.55),
            sw_y: (0.10, 0.25),
            sw_roll: (-0.12, 0.35),
            arc: (0.0, 0.30), // the downcut bows toward the sword side mid-strike
        },
    }
}

/// Per-variant FP **camera swing-sway** (screen-space radians: x = pitch(+up), y = yaw(+left),
/// z = roll). `.0` leans through the wind-up — a small anticipation pull OPPOSITE the cut — and
/// `.1` rides the strike WITH the blade. Written onto [`super::FirstPerson::sway`] scaled by the
/// same wind/punch envelopes as the arms, applied by `player::camera` after `look_at`, so the view
/// itself commits to every cut (the missing half of the old "toporne" stick-swings). Deliberately
/// small (≤~3°) and one smooth arc per swing — a lean, never a shake (motion-sickness guard); aim
/// is unaffected (`hero.facing` comes from the un-swayed yaw).
fn fp_cam_sway(variant: u8) -> (Vec3, Vec3) {
    match variant {
        // horizontal slash: gather right, then sweep left across the frame with a matching roll.
        1 => (Vec3::new(0.008, -0.020, 0.014), Vec3::new(-0.006, 0.032, -0.026)),
        // thrust: a breath back, then a forward nod into the punch.
        2 => (Vec3::new(-0.006, -0.006, 0.006), Vec3::new(0.012, 0.006, -0.008)),
        // Heavy: the chop writ large — rise with the overhead haul, drop hard with the smash.
        v if v == super::combat::HEAVY_VARIANT => {
            (Vec3::new(0.028, 0.0, -0.010), Vec3::new(-0.050, 0.006, 0.016))
        }
        // overhead chop: rise with the cock, dip with the blow.
        _ => (Vec3::new(0.016, -0.006, -0.008), Vec3::new(-0.034, 0.008, 0.012)),
    }
}

pub fn hero_anim(
    time: Res<Time>,
    player: Res<super::PlayerRes>,
    dir: Res<crate::cinematic::DirectorState>,
    // `ResMut`: hero_anim WRITES `fp.sway` (the FP camera swing-sway target) each frame.
    mut fp: ResMut<super::FirstPerson>,
    hero_q: Query<(&Hero, &HeroHealth)>,
    mut parts: Query<(&HeroPart, &mut Transform)>,
    // Edge-detect touchdown (was airborne, now grounded) to stamp a short landing-squash window.
    mut was_air: Local<bool>,
    mut land_at: Local<f32>,
    // Smoothed block weight (0 = open, 1 = full defend) so the brace eases in/out.
    mut block_amt: Local<f32>,
    // FP turn-sway state: last frame's view yaw + the low-passed yaw rate (rad/s).
    mut fp_prev_yaw: Local<f32>,
    mut fp_sway_amt: Local<f32>,
    // Smoothed FP "weapon drawn" weight (0 = calm low carry at the frame edges, 1 = combat-ready).
    mut fp_ready: Local<f32>,
) {
    let Ok((hero, hh)) = hero_q.single() else { return };
    let now = time.elapsed_secs();
    let dt = time.delta_secs();

    // Touchdown edge → arm the landing squash (before the early-returns so it's always stamped).
    if *was_air && hero.on_ground {
        *land_at = now;
    }
    *was_air = !hero.on_ground;

    // Slain: let the limbs go slack while the body keels over (root rotation owned by health.rs).
    if !player.0.is_alive() {
        fp.sway = Vec3::ZERO; // no stale FP swing-lean on the death camera
        for (part, mut tf) in &mut parts {
            tf.rotation = match part.joint {
                Joint::Hips => {
                    tf.translation = Vec3::new(0.0, 1.05, 0.0);
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

    let attack = hero.attacking.then(|| attack_phase((hero.attack_t / hero.attack_dur).clamp(0.0, 1.0)));
    let gesture = dir.gesture.map(|g| gesture_pose(g, now - dir.gesture_start));
    let fp_amt = fp.blend.clamp(0.0, 1.0);

    // Pick the active clip. Actions now LAYER over locomotion so combined moves read right: swinging
    // while running keeps the legs striding (a running attack), and a jump taken at speed becomes a
    // forward leap. (Priority: victory › attack › jump › block-blended locomotion.)
    let moving = hero.moving_amt.clamp(0.0, 1.0);

    // ── First-person viewmodel: procedural weapon motion ──
    // The FP arms have no clip of their own — without this they freeze at the static tuck and read
    // "glued to the camera". Three small joint-space layers (radians), all camera-untouched (the FP
    // eye stays rigid — motion-sickness guard): breath (idle heave), walk-bob (stride pump),
    // turn-sway (weapon lags the view yaw).
    // FP weapon-ready weight: out of combat the arms settle into a calm low carry (gear barely
    // peeking into the bottom frame corners); a hostile nearby, a swing, a charge, or a raised
    // guard DRAWS them up into the ready stance. Quick raise (~0.15s) so an ambush reads
    // instantly; slow lower (~0.5s + the 6s `combat_until` linger) so it never pumps mid-fight.
    let fp_want_ready = hero.attacking
        || hero.charge_t > CHARGE_GRACE
        || hh.blocking
        || hero.threats > 0
        || now < hero.combat_until;
    let fp_ready_rate = if fp_want_ready { 12.0 } else { 3.0 };
    *fp_ready += ((if fp_want_ready { 1.0 } else { 0.0 }) - *fp_ready) * (dt * fp_ready_rate).min(1.0);
    let fp_ready = fp_ready.clamp(0.0, 1.0);

    // FP attack envelope, shared by the sword arm and wrist: `back` coils through the wind (and
    // through a held Heavy-Strike charge), `fwd` punches through the strike and eases out across
    // the recovery. The *direction* of the coil/punch comes from the per-variant [`fp_swing`]
    // table, so the FP combo reads as three distinct cuts (chop / slash / thrust) like the
    // third-person clips, instead of one repeated generic poke.
    let (fp_atk_back, fp_atk_fwd) = if hero.charge_t > CHARGE_GRACE && attack.is_none() {
        (1.0, 0.0)
    } else {
        match &attack {
            Some((Phase::Wind, p)) => (*p, 0.0),
            Some((Phase::Strike, p)) => (1.0 - *p, *p),
            Some((Phase::Recovery, p)) => (0.0, 1.0 - *p),
            None => (0.0, 0.0),
        }
    };
    // Mid-strike crescent envelope: 0 at both strike endpoints, peaking mid-cut — feeds the
    // per-variant `arc` bulge so the blade's path bows instead of lerping straight.
    let fp_atk_arc = match &attack {
        Some((Phase::Strike, p)) => (*p * PI).sin(),
        _ => 0.0,
    };
    // A held charge coils into the Heavy's shape even before the release stamps the variant.
    let fp_variant = if hero.charge_t > CHARGE_GRACE && attack.is_none() {
        super::combat::HEAVY_VARIANT
    } else {
        hero.attack_variant
    };
    let fp_sw = fp_swing(fp_variant);
    // FP camera swing-sway target — screen-space (NOT mirrored, unlike the rig targets below),
    // ridden by `player::camera`. Zeroed outside FP so re-entering never inherits a stale lean.
    fp.sway = if fp_amt > 0.0 {
        let (wind, strike) = fp_cam_sway(fp_variant);
        (wind * fp_atk_back + strike * fp_atk_fwd) * fp_amt
    } else {
        Vec3::ZERO
    };

    let (fp_breath, fp_bob_v, fp_bob_l, fp_sway) = if fp_amt > 0.0 {
        let breath = (now * 3.1).sin() * 0.018; // ~0.5 Hz idle heave
        // Vertical pump at 2× stride (one dip per footfall), light lateral sway at 1×; a touch
        // stronger at sprint. `walk_phase` is already a real-speed radian cycle.
        // Amplitudes deliberately tiny: the FP camera is rigid, so at eye scale hundredths of a
        // radian already read clearly — bigger flails the weapon across the lens. No sprint boost
        // for the same reason (the run's own cadence via `walk_phase` is speed-up enough).
        let stride = moving;
        let bob_v = (hero.walk_phase * 2.0).sin() * 0.022 * stride;
        let bob_l = hero.walk_phase.sin() * 0.015 * stride;
        // Turn-sway: low-pass the view-yaw rate (hero.facing IS look_yaw in FP — the camera writes
        // it) and tilt the weapon OPPOSITE the turn, recovering as the rate settles.
        let yaw_rate = crate::steer::wrap_pi(hero.facing - *fp_prev_yaw) / dt.max(1e-3);
        *fp_sway_amt += (yaw_rate.clamp(-8.0, 8.0) - *fp_sway_amt) * (dt * 9.0).min(1.0);
        (breath, bob_v, bob_l, (*fp_sway_amt * -0.012).clamp(-0.07, 0.07))
    } else {
        *fp_sway_amt = 0.0;
        (0.0, 0.0, 0.0, 0.0)
    };
    *fp_prev_yaw = hero.facing;
    // Combat stance feeds two extra locomotion axes (backpedal blend + pelvis-vs-torso twist);
    // both are 0 out of the stance, where this reduces exactly to the plain `loco_pose`. The
    // guard overlay then colours ALL stance locomotion (idle/walk/run) into the ready-to-fight
    // carry — knees bent, shield up, blade at the ready.
    let loco = {
        let mut p = stance_loco_pose(
            now,
            hero.walk_phase,
            moving,
            hero.run_amt.clamp(0.0, 1.0),
            hero.back_amt,
            hero.strafe_twist,
        );
        guard_overlay(&mut p, hero.stance_amt);
        p
    };
    let pose = if hero.victory {
        victory_pose(now)
    } else if hero.roll_t >= 0.0 {
        // Dodge roll: fold into the tuck through the somersault's core, unfolding at both ends so
        // the dive-in / stand-up carry the transition (the root owns the actual tumble).
        let u = (hero.roll_t / super::movement::ROLL_TIME).clamp(0.0, 1.0);
        let w = smoothstep(u / 0.15) * smoothstep((1.0 - u) / 0.18);
        loco.lerp(&roll_pose(), w)
    } else if hero.dash_t >= 0.0 {
        // Sand Dash slide: play the dash-swipe lunge, easing back into locomotion at the blink's tail.
        let p = (hero.dash_t / super::movement::DASH_TIME).clamp(0.0, 1.0);
        let tail = smoothstep((p - 0.7) / 0.3);
        dash_pose(p).lerp(&loco, tail)
    } else if let Some((phase, p)) = &attack {
        let atk = attack_pose(hero.attack_variant, phase, *p);
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
        if moving > 0.05 {
            j.lerp(&leap_pose(hero.vel_y), moving) // running leap
        } else {
            j
        }
    } else if block_amt > 0.001 {
        brace(&loco, &defend_pose(now), block_amt, moving)
    } else {
        loco
    };

    // Landing squash: a quick crouch the instant the feet hit, easing back over `LAND_RECOVER`.
    let landing = if *land_at <= 0.0 {
        0.0 // no touchdown yet (fresh boot) — don't play an unearned landing crouch
    } else {
        let u = (1.0 - (now - *land_at) / LAND_RECOVER).clamp(0.0, 1.0);
        u * u
    };

    for (part, mut tf) in &mut parts {
        let jp = pose.get(part.joint);
        if let Some(t) = jp.t {
            tf.translation = t;
        }
        let mut rot = jp.r;

        // Arm overrides: the Director's staged gesture wins on the arms; otherwise the first-person
        // viewmodel carries the arms (low at rest, raised when `fp_ready`). All FP targets here are
        // authored in RIG space — the handedness mirror below flips the finished chains onto the
        // correct screen sides. (Combat/locomotion already in `rot`.)
        match part.joint {
            Joint::ShoulderR | Joint::ElbowR => {
                let elbow = part.joint == Joint::ElbowR;
                if let Some((Some((sh, el)), _)) = gesture {
                    rot = if elbow { el } else { sh };
                } else if fp_amt > 0.0 {
                    // First-person sword ARM. The eye sits AT the chest, so the third-person
                    // clips (whose swings/braces orbit the chest) land ON the lens — the arm is
                    // therefore ALWAYS viewmodel-driven in FP. Two carries blended by `fp_ready`:
                    // out of combat a calm LOW carry (arm hanging easy, the resting blade's tip
                    // just grazing the bottom-right corner); in combat the Skyrim-style READY.
                    // An attack layers a compact wind→punch envelope on top (the blade's sweep
                    // itself plays on the Sword joint below). Breath/bob/sway keep it alive.
                    let (back, fwd) = (fp_atk_back, fp_atk_fwd);
                    // While the shield is braced the sword arm DROPS back toward the low carry —
                    // at full ready its forearm hangs right at the lens edge (elbow z≈-0.12 in
                    // camera space) and paints a full-height dark band down the right of the
                    // blocking frame. The blade is tucked away during a block anyway.
                    let fp_ready = fp_ready * (1.0 - 0.65 * block_amt);
                    let target = if elbow {
                        rx(lerp(-0.35, -1.05, fp_ready) + fp_sw.el.0 * back + fp_sw.el.1 * fwd + fp_bob_v * 0.6)
                    } else {
                        // `arc` bows the strike's path mid-cut (crescent), on top of the
                        // endpoint envelope — see the `FpSwing::arc` note.
                        e3(
                            lerp(0.10, -0.70, fp_ready) + fp_sw.sh_x.0 * back + fp_sw.sh_x.1 * fwd
                                + fp_sw.arc.0 * fp_atk_arc
                                + fp_breath
                                + fp_bob_v,
                            -(fp_sway + fp_bob_l) * lerp(0.5, 1.0, fp_ready) - 0.35 * fp_ready
                                + fp_sw.sh_y.0 * back
                                + fp_sw.sh_y.1 * fwd
                                + fp_sw.arc.1 * fp_atk_arc,
                            lerp(0.12, 0.10, fp_ready),
                        )
                    };
                    rot = rot.slerp(target, fp_amt);
                }
            }
            Joint::Sword => {
                // FP wrist — fully viewmodel-owned in first person: the FP arm tilts the HAND
                // frame so far back that EVERY pose-space wrist angle (rest 1.95, the clips'
                // sweeps) points the blade up at / behind the lens. These angles are SOLVED from
                // the FPDBG camera-space probes (reconstruct the hand basis, invert the desired
                // camera-space blade line back to joint-local Euler), not eyeballed:
                // - ready: blade up-forward from the bottom-right hilt toward frame centre
                // - attack: the envelope cocks it back-right through the wind, sweeps it across
                //   toward centre-down through the strike (the arm punch carries the hilt)
                // - guard: tucked down-forward-right, out of frame — the shield is the story.
                if fp_amt > 0.0 {
                    // Low carry — the out-of-combat grip. The old code left the wrist entirely to
                    // the third-person pose below `fp_ready` = 0, and through the tilted FP hand
                    // frame the rest/gait angles read as a rigid rod jutting up across the frame
                    // ("nie trzyma miecza" bug) — so the wrist is now owned in FP at ALL times:
                    // relaxed here (blade angled easy down-forward out of the frame's way, riding
                    // the walk bob), drawn up into `combat` as `fp_ready` rises.
                    let carry = e3(3.02 + fp_bob_v * 0.4, -0.30 - fp_sway * 0.5, -0.44);
                    // `sw_roll` turns the EDGE about the blade's own axis through the cut
                    // (pronated on the cock, rolled over through the strike; the thrust
                    // corkscrews). Right-multiplied local +Y spin — the blade runs along mesh
                    // +Y, so this rolls it in place without deflecting its line.
                    let edge_roll = fp_sw.sw_roll.0 * fp_atk_back + fp_sw.sw_roll.1 * fp_atk_fwd;
                    // Ready base: yaw −0.20 (not the old −0.60) angles the blade as a DIAGONAL
                    // across the lower-right — the −0.60 carry pointed it almost dead along the
                    // view axis, which on screen read as a disembodied floating rod ("jakies
                    // nienaturalne"): a sliver of near-axial blade with the hand below frame.
                    let combat = e3(
                        2.70 + fp_sw.sw_x.0 * fp_atk_back + fp_sw.sw_x.1 * fp_atk_fwd + fp_bob_v * 0.5,
                        -0.20 - fp_sway + fp_sw.sw_y.0 * fp_atk_back + fp_sw.sw_y.1 * fp_atk_fwd,
                        -0.44,
                    ) * Quat::from_rotation_y(edge_roll);
                    let ready_w = if attack.is_some() { 1.0 } else { fp_ready };
                    let target = carry.slerp(combat, ready_w).slerp(e3(-2.42, -0.60, -0.33), block_amt);
                    rot = rot.slerp(target, fp_amt);
                }
            }
            Joint::ShoulderL | Joint::ElbowL => {
                let elbow = part.joint == Joint::ElbowL;
                if let Some((_, Some((sh, el)))) = gesture {
                    rot = if elbow { el } else { sh };
                } else if fp_amt > 0.0 {
                    // First-person shield ARM (always viewmodel-driven in FP — see the sword arm
                    // note): it STAYS in the low carry even at combat-ready — the shield rides
                    // edge-on beside the thigh, out of frame bar a sliver in the bottom-left
                    // corner (raising it at ready laid the plate ALONG the lifted forearm, whose
                    // far end crossed the lens as a wall). Only a raised guard (`block_amt`)
                    // BRACES it up into the lower-left of frame, where the FP Shield override
                    // below turns the plate's face to the camera.
                    let target = if elbow {
                        rx(lerp(-0.55, -1.20, block_amt) + fp_bob_v * 0.6)
                    } else {
                        e3(
                            lerp(0.10, -0.75, block_amt) + fp_breath + fp_bob_v,
                            -(fp_sway - fp_bob_l) * 0.5,
                            lerp(-0.12, -0.02, block_amt),
                        )
                    };
                    rot = rot.slerp(target, fp_amt);
                }
            }
            Joint::Shield => {
                // FP: the shield joint is FULLY viewmodel-owned, like the sword wrist. The
                // third-person attack clips write the shield EVERY swing (the slash lays the
                // plate flat, the thrust braces it forward) — and in FP the shield hand hangs
                // right at the lens, so those writes swept the dark plate ACROSS the camera:
                // the "whole frame blacks out mid-swing" bug that survived every arm retune.
                // Out of block, pin it to the edge-on rest carry instead.
                if fp_amt > 0.0 && block_amt <= 0.001 {
                    rot = rot.slerp(shield_rest_r(), fp_amt);
                    tf.translation = tf.translation.lerp(SHIELD_REST_T, fp_amt);
                }
                // FP block: turn the plate's FACE to the camera, low in frame — the pose-space
                // defend brace (rx(π/2)) shows its BACK through the FP hand frame. Solved from
                // the FPDBG probes like the sword wrist.
                if fp_amt > 0.0 && block_amt > 0.001 {
                    let k = fp_amt * block_amt;
                    // Z carries a π roll — the other solution branch held the heater point-UP.
                    // X tips the plate's face UP into the skylight (probe: face.y moves ≈ +1.1
                    // per +rad of X here; -0.85 lands ≈0.4) so the brace catches light and reads
                    // as a dimensional plate, not a flat dark wall. (A stronger -0.70 tilt +
                    // higher mount was tried: the plate rode up into the frame centre and ate
                    // half the view — this framing keeps it low with the rim/boss just in shot.)
                    rot = rot.slerp(e3(-0.85, -0.02, -2.97), k);
                    // Push the braced plate OUT of the lens. The defend pose's (0,0,0.1) mount
                    // left the plate ~0.38u from the FP eye — it filled half the frame as one
                    // featureless slab (the "odwrócona tarcza" read: too close to show its rim
                    // or boss). Probe-solved hand-frame push: -Y runs along the raised forearm
                    // AWAY from the camera, -Z drops it toward the lower frame edge — lands the
                    // centre ≈(-0.27,-0.38,-0.6) in camera space: lower-left frame, plate in view.
                    tf.translation = tf.translation.lerp(Vec3::new(0.0, -0.28, -0.18), k);
                }
            }
            _ => {}
        }

        // ── FP handedness mirror ──
        // The studio port left the rig handedness-MIRRORED: the joint named ShoulderR renders on
        // the viewer's LEFT through the FP eye (three.js +Z-toward-viewer vs Bevy -Z-forward).
        // Invisible in third person behind the low-poly silhouette, glaring at eye height ("holds
        // the sword left-handed"). In first person, mirror the whole arm chains + held gear across
        // the body plane — translations flip X, rotations conjugate by the reflection (keep x,
        // negate y/z) — so the sword reads bottom-RIGHT / shield bottom-LEFT like every FP melee
        // game, and EVERY authored pose (attack sweeps, the block brace, the charge coil) stays
        // correct-handed with no FP-specific re-authoring. (The earlier translation-only
        // anchor-swap left the un-mirrored rotations sweeping attacks/blocks the wrong way —
        // the "FP is completely broken" bug.) Third person keeps the rig as authored; sword and
        // heater shield are x-symmetric meshes, so the un-mirrored geometry doesn't tell.
        match part.joint {
            Joint::ShoulderR | Joint::ShoulderL | Joint::ElbowR | Joint::ElbowL | Joint::Sword | Joint::Shield => {
                if fp_amt > 0.0 {
                    let mirrored = Quat::from_xyzw(rot.x, -rot.y, -rot.z, rot.w);
                    rot = rot.slerp(mirrored, fp_amt);
                }
                // Anchors: the shoulders are never written by poses (they spawn at ±SHOULDER_DX),
                // so cross-fade them to the opposite side from their homes — unconditionally, so
                // leaving FP restores them. The shield's per-pose local offset was freshly written
                // above, so a plain X flip mirrors it (identity at fp_amt = 0).
                let home = super::model::SHOULDER_DX;
                match part.joint {
                    Joint::ShoulderR => tf.translation.x = lerp(home, -home, fp_amt),
                    Joint::ShoulderL => tf.translation.x = lerp(-home, home, fp_amt),
                    Joint::Shield => tf.translation.x = lerp(tf.translation.x, -tf.translation.x, fp_amt),
                    _ => {}
                }
            }
            _ => {}
        }
        tf.rotation = rot;

        // FP: the camera eye is rigid (no head-bob by design), so any hips/torso bob/lean/twist
        // shakes the ENTIRE viewmodel across the lens — the run gait's torso pump alone swung the
        // sword arm from frame-right to frame-LEFT ("macha mieczem" bug), and the loco hips-bob
        // read as flailing. Flatten the trunk back to its rest carry in FP (the arm targets above
        // are LOCAL to the torso, so a stable trunk keeps them pinned to the frame corners); the
        // small controlled per-joint bob terms carry the walk feel instead. Also damps the attack
        // clips' hip/torso drive (their forward shove pushes the chest into the near plane).
        if fp_amt > 0.0 {
            match part.joint {
                Joint::Hips => {
                    tf.translation = tf.translation.lerp(Vec3::new(0.0, 1.05, 0.0), fp_amt);
                    tf.rotation = tf.rotation.slerp(Quat::IDENTITY, fp_amt * 0.8);
                }
                Joint::Torso => {
                    tf.rotation = tf.rotation.slerp(Quat::IDENTITY, fp_amt * 0.9);
                }
                _ => {}
            }
        }

        // Landing squash folded over the locomotion pose right after touchdown (studio positive-knee
        // crouch: hips dip, knees bend, thighs settle back, feet flatten, torso leans in).
        if landing > 0.0 && attack.is_none() && hero.on_ground {
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
