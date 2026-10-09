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
    // The footman's bind pose IS the standing carry (arms already clear of the torso). Idle only
    // breathes — the old +0.1 / −0.4 elbow bend was compensating for a straight-down mesh.
    p.sh_l = Jp::r(e3(breath * 0.035, 0.0, c11 * 0.012 + w * 0.01));
    p.el_l = Jp::r(rx(-0.05 - breath * 0.02));
    p.sh_r = Jp::r(e3(breath * 0.035, 0.0, -c11 * 0.012 + w * 0.01));
    p.el_r = Jp::r(rx(-0.04 - breath * 0.015));
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
    /// Stance travel of the planted ankle, strike → toe-off, relative to the hips (m).
    sweep: f32,
    /// Share of `sweep` ahead of the hip at heel strike (runners land closer under the body).
    front: f32,
    /// Stance share of one leg cycle (walk > 0.5 = double support; run < 0.5 = flight).
    duty: f32,
    /// Plantar-flex at toe-off (rad, + = toe down) and the boot pitch at contact (− = heel first).
    toe_off: f32,
    strike: f32,
    /// 0 walk → 1 jog, and 0 jog → 1 sprint (style weights for the upper body / bob).
    run: f32,
    sprint: f32,
}

fn gait_at(v: f32) -> Gait {
    let run = smoothstep((v - 2.0) / 1.7);
    let sprint = smoothstep((v - 4.9) / 2.7);
    // Short creeping steps when barely moving, a full stride by a brisk walk.
    let walk_sweep = lerp(0.40, 0.9, smoothstep(v / 1.5));
    let k = |w: f32, j: f32, s: f32| lerp(lerp(w, j, run), s, sprint);
    Gait {
        sweep: k(walk_sweep, 0.78, 0.86),
        front: k(0.44, 0.36, 0.30),
        duty: k(0.6, 0.28, 0.22),
        toe_off: k(0.38, 0.55, 0.75),
        strike: k(-0.22, -0.08, 0.0),
        run,
        sprint,
    }
}

/// World units per footman model metre (rig scale × `HERO_SCALE`).
fn hero_model_scale() -> f32 {
    super::footman::leg_rig().scale * super::HERO_SCALE
}

/// `walk_phase` advance rate (rad/s) at a world ground speed — the foot-lock contract: a stance of
/// `duty` of the cycle must carry the planted ankle back through `sweep` at exactly the body's speed.
pub(crate) fn gait_phase_rate(world_speed: f32) -> f32 {
    let v = world_speed.max(0.0) / hero_model_scale();
    if v < 1e-4 {
        return 0.0;
    }
    let g = gait_at(v);
    std::f32::consts::TAU * v * g.duty / g.sweep
}

/// Where one leg's boot is at leg-cycle `u`: (flat-ankle z, clearance of the lowest boot point,
/// pitch, stance support weight 0..1). Only the pitch is meaningful in the swing.
fn leg_track(u: f32, g: &Gait) -> (f32, f32, f32, f32) {
    let z_strike = g.front * g.sweep;
    if u < g.duty {
        // Planted: the ankle slides back linearly = locked to the ground. Heel rocker settles the
        // boot flat after the strike, toe rocker peels the heel up into the push-off.
        let s = u / g.duty;
        let pitch = lerp(g.strike, 0.0, smoothstep(s / 0.18)) + g.toe_off * smoothstep((s - 0.68) / 0.32);
        (z_strike - s * g.sweep, 0.0, pitch, (PI * s).sin())
    } else {
        // Swing legs are posed by joint angle (see `swing_leg`), not by an ankle path; only the
        // boot pitch is used from here: pointed through the fold-up, levelled for the strike.
        let s = (u - g.duty) / (1.0 - g.duty);
        let pitch = lerp(g.toe_off, g.strike, smoothstep((s - 0.3) / 0.55));
        (0.0, 0.0, pitch, 0.0)
    }
}

/// A swing leg's (thigh, knee-flex) forward angles at swing progress `s`, keyed on running /
/// walking gait-lab joint curves (Novacheck '98: jog hip flexion peaks ≈45° late in the swing and
/// the knee folds to ≈90–95° just after toe-off; a sprint ≈65–70° / ≈120°; a walk ≈28° / ≈55°),
/// with the ends pinned to the stance IK at toe-off and strike so the hand-off is seamless.
/// (An ankle-path IK here let the thigh fly up past horizontal whenever the boot rose under the
/// hip — 136° of hip flexion in the sprint.)
fn swing_leg(s: f32, g: &Gait, off: (f32, f32), strike: (f32, f32)) -> (f32, f32) {
    let k = |w: f32, j: f32, sp: f32| lerp(lerp(w, j, g.run), sp, g.sprint).to_radians();
    let thigh = [off.0, k(8.0, 5.0, 12.0), k(22.0, 30.0, 48.0), k(28.0, 46.0, 68.0), strike.0];
    let knee = [off.1, k(58.0, 88.0, 118.0), k(48.0, 92.0, 115.0), k(16.0, 50.0, 58.0), strike.1];
    let ends = |v: &[f32; 5]| ((v[1] - v[0]) / 0.25, (v[4] - v[3]) / 0.25);
    let (ts, te) = ends(&thigh);
    let (ks, ke) = ends(&knee);
    (keyed(&thigh, s, ts, te), keyed(&knee, s, ks, ke))
}

/// Smooth curve through five evenly spaced keys at s = 0, ¼, ½, ¾, 1 (Catmull-Rom tangents
/// inside, the given end slopes in per-s units).
fn keyed(k: &[f32; 5], s: f32, t_start: f32, t_end: f32) -> f32 {
    const H: f32 = 0.25;
    let s = s.clamp(0.0, 1.0);
    let i = ((s / H) as usize).min(3);
    let tan = |j: usize| match j {
        0 => t_start,
        4 => t_end,
        _ => (k[j + 1] - k[j - 1]) / (2.0 * H),
    };
    let t = (s - i as f32 * H) / H;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * k[i]
        + (t3 - 2.0 * t2 + t) * H * tan(i)
        + (-2.0 * t3 + 3.0 * t2) * k[i + 1]
        + (t3 - t2) * H * tan(i + 1)
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

/// The hero's foot-locked locomotion at world ground speed `speed`, cycle phase `c`.
/// `armed` (0..1): the sword is in hand (1) or slung on the back (0) — an empty sword hand pumps
/// as freely as the shield arm.
fn footman_gait(c: f32, speed: f32, armed: f32) -> Pose {
    let rig = super::footman::leg_rig();
    let g = gait_at(speed.max(0.0) / hero_model_scale());
    let (run, sprint) = (g.run, g.sprint);
    let u_l = (c / std::f32::consts::TAU).rem_euclid(1.0);
    let u_r = (u_l + 0.5).fract();
    let (zl, cl, pl, kl) = leg_track(u_l, &g);
    let (zr, cr, pr, kr) = leg_track(u_r, &g);
    let al = boot_ankle(zl, cl, pl, &rig);
    let ar = boot_ankle(zr, cr, pr, &rig);
    // The stance targets a swing leg hands off from (toe-off) and to (strike).
    let at = |u: f32| {
        let (z, c, p, _) = leg_track(u, &g);
        boot_ankle(z, c, p, &rig)
    };
    let (a_off, a_strike) = (at(g.duty - 1e-4), at(0.0));
    let swing_s = |u: f32| (u >= g.duty).then(|| (u - g.duty) / (1.0 - g.duty));
    // +1 when the LEFT boot is out front (its strike), −1 at the right strike.
    let swing = (c).cos();

    // ── Pelvis ──
    // Vertical bob, once per step — the main carrier of weight. A walk vaults UP over the planted
    // leg (highest at single-leg mid-stance, lowest in double support); a run sinks INTO the
    // stance (lowest mid-stance, loaded knee) and peaks in the flight phase. Amplitudes follow
    // human centre-of-mass excursion scaled to this ~1.9 m body: ~4-5 cm peak-to-peak walking,
    // ~8-9 cm jogging, a little flatter in a full sprint.
    let step = (2.0 * u_l).fract();
    let crest = (TAU * (step - (g.duty + 0.5 * run))).cos(); // +1 at the top of the bob
    let bob = lerp(0.022, lerp(0.045, 0.038, sprint), run) * crest;
    let base = super::model::HIP_REST_Y - lerp(0.012, lerp(0.035, 0.05, sprint), run);
    let sway = lerp(0.022, 0.012, run) * (kr - kl); // over the stance foot (left leg is −X)
    // Hips work in all three planes: they yaw with the stride (the forward leg's hip swings
    // forward), DROP on the swing-leg side while the other leg carries the weight, and tip
    // forward a touch as the body loads into each stance.
    let pel_yaw = lerp(0.10, lerp(0.12, 0.14, sprint), run) * swing; // + swings the left hip forward
    let pel_drop = lerp(0.045, 0.075, run) * (kr - kl); // − = right (+X) hip low: left leg loaded
    let pelvis = e3(run * (0.06 - 0.03 * crest), pel_yaw, pel_drop);
    // Never ask a planted leg for more reach than it has: the hips sink at the far ends of a
    // stride instead of the boot peeling off the ground (soft-min keeps it smooth).
    let reach = 0.985 * (rig.knee.y.hypot(rig.knee.z) + rig.foot.y.hypot(rig.foot.z));
    let hip_l = pelvis * rig.hip;
    let hip_r = pelvis * Vec3::new(-rig.hip.x, rig.hip.y, rig.hip.z);
    let cap = |a: (f32, f32), off: Vec3| {
        let dz = a.0 - off.z;
        a.1 + (reach * reach - dz * dz).max(0.0).sqrt() - off.y
    };
    let smin = |a: f32, b: f32| {
        let k = 0.03;
        let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
        lerp(b, a, h) - k * h * (1.0 - h)
    };
    // Planted legs cap the hips directly; a swing leg only through the stance it is leaving /
    // about to land in, phased in so the hips glide rather than jump at contact.
    let leg_cap = |u: f32, a: (f32, f32), off: Vec3| match swing_s(u) {
        None => cap(a, off),
        Some(s) => (cap(a_off, off) + smoothstep(s / 0.3)).min(cap(a_strike, off) + 1.0 - smoothstep((s - 0.5) / 0.5)),
    };
    let hips_y = smin(smin(base + bob, leg_cap(u_l, al, hip_l)), leg_cap(u_r, ar, hip_r));
    let hips_t = Vec3::new(sway, hips_y, 0.0);

    let mut p = rest();
    // The writer re-adds HIP_REST_Y − 1.05 (legacy 1.05-hip clips); store in that convention.
    p.hips = Jp { t: Some(hips_t + Vec3::Y * (1.05 - super::model::HIP_REST_Y)), r: pelvis };
    let leg = |off: Vec3, u: f32, a: (f32, f32), pitch: f32| {
        let hip = Vec2::new(hips_t.z + off.z, hips_t.y + off.y);
        let (th, kn, ft) = match swing_s(u) {
            None => leg_ik(hip, Vec2::new(a.0, a.1), pitch, &rig),
            Some(s) => {
                let dirs = |t: (f32, f32)| {
                    let (thigh, shin) = leg_dirs(hip, Vec2::new(t.0, t.1), &rig);
                    (thigh, thigh - shin)
                };
                let (thigh, flex) = swing_leg(s, &g, dirs(a_off), dirs(a_strike));
                leg_rot(thigh, thigh - flex, pitch, &rig)
            }
        };
        // Legs live in the pelvis frame: undo the pelvis turn so the leg plane stays on the travel
        // line and a planted boot never twists or skates with the hip swing. The sagittal IK
        // ignores X, so the pelvis's side-to-side sway would carry the boots with it (a planted
        // foot shuffling ±2 cm sideways every step reads as wobbly on the legs): abduct the leg
        // against the sway so the feet keep their track and only the pelvis travels.
        let leg_len = (hip.y - a.1).max(0.3);
        let abduct = Quat::from_rotation_z((-(hips_t.x) / leg_len).clamp(-0.5, 0.5).asin());
        (Jp::r(pelvis.inverse() * abduct * rx(th)), Jp::r(rx(kn)), Jp::r(rx(ft)))
    };
    (p.hip_l, p.knee_l, p.foot_l) = leg(hip_l, u_l, al, pl);
    (p.hip_r, p.knee_r, p.foot_r) = leg(hip_r, u_r, ar, pr);

    // ── Trunk: lean into the speed, shoulders counter-rotate the pelvis, head stays on the road ──
    let lean = lerp(0.04, lerp(0.15, 0.28, sprint), run) + run * 0.035 * (1.0 - crest) * 0.5;
    // Overlapping action: nothing moves in lock-step. The pelvis leads, the shoulders follow it a
    // beat later, the arms later still, the elbows and the head last — identical in-phase sines on
    // every joint are what read as stiff / robotic.
    let shoulder = lerp(0.07, lerp(0.13, 0.17, sprint), run) * (c - 0.2).cos(); // + = right shoulder forward
    // The trunk soaks up each landing a moment after the low point (and the head counters most
    // of it so the gaze stays steady).
    let absorb = lerp(0.015, 0.03, run) * -(TAU * (step - (g.duty + 0.5 * run)) - 0.7).cos();
    // The trunk takes back most of the pelvic drop so the shoulders stay nearly level.
    p.torso = Jp::r(
        Quat::from_rotation_y(-pel_yaw - shoulder)
            * e3(lean - lerp(0.0, 0.06, run) + absorb, 0.0, -0.75 * pel_drop),
    );
    let head_yaw = lerp(0.07, lerp(0.13, 0.17, sprint), run) * (c - 0.45).cos();
    p.head = Jp::r(e3(-lean * 0.75 - 0.7 * absorb, head_yaw * 0.85, -0.25 * pel_drop));

    // ── Arms: opposite arm to the forward leg (right/sword arm forward when the left boot is out
    // front). Sprint-coaching rules of thumb: the swing comes from a relaxed shoulder with the
    // elbows kept close to the body; the hand rises to about chin height just inside the
    // shoulder, drifting toward (never across) the midline, and drops past the hip behind; the
    // elbow is NOT locked at 90° — it closes to ~70° in front and opens past 90° behind, trailing
    // the shoulder a beat (that lag is what reads as loose rather than robotic). A walk swings
    // near-straight. ──
    let amp = lerp(0.26, lerp(0.52, 0.64, sprint), run);
    let back_bias = lerp(0.04, 0.18, run);
    let flex_front = lerp(0.45, lerp(1.75, 1.95, sprint), run);
    let flex_back = lerp(0.12, lerp(1.2, 1.1, sprint), run);
    let tuck = lerp(0.0, -0.07, run); // + = away from the body
    let cross = 0.35 * run; // forward-swing drift toward the midline
    // `scale` trims the pump; `inward` is how far the back swing tucks the arm toward the spine
    // (the bent arm otherwise flings the fist out past the hip); `elbow_fix` holds the elbow at a
    // steady carry (the shield arm: a big board flapping through a full pump reads as broken).
    let arm = |fwd: f32, fwd_late: f32, side: f32, scale: f32, inward: f32, elbow_fix: f32| {
        let z = tuck - cross * fwd.max(0.0) - inward * run * (-fwd).max(0.0);
        let sh = e3(-scale * amp * fwd + back_bias, -0.2 * run * side, side * z);
        let pump = -lerp(flex_back, flex_front, 0.5 + 0.5 * fwd_late);
        let el = rx(lerp(pump, -0.5 * (flex_back + flex_front), elbow_fix));
        (sh, el)
    };
    // The arm trails the leg phase a little; the forearm trails the upper arm.
    let arm_swing = (c - 0.3).cos();
    let late = (c - 0.3 - lerp(0.35, 0.45, run)).cos();
    let (sr, er) = arm(arm_swing, late, 1.0, lerp(1.0, 0.8, armed), 0.4, 0.0);
    let (sl, el) = arm(-arm_swing, -late, -1.0, lerp(0.85, 0.6, run), 0.1, lerp(0.2, 0.6, run));
    // The shoulder girdle travels with its arm (forward + a hair up on the front swing, back on
    // the back swing) instead of the arm pivoting about a bolted-down socket.
    let girdle = |rest: Vec3, fwd: f32| {
        rest + Vec3::new(0.0, lerp(0.006, 0.014, run) * fwd.max(0.0), lerp(0.015, 0.03, run) * fwd)
    };
    p.sh_r = Jp { t: Some(girdle(rig.shoulder_r, arm_swing)), r: sr };
    p.el_r = Jp::r(er);
    p.sh_l = Jp { t: Some(girdle(rig.shoulder, -arm_swing)), r: sl };
    p.el_l = Jp::r(el);
    // With the blade in hand, the wrist rides the pump against it so the sword stays shouldered
    // near-upright instead of fanning behind the head.
    let wrist = armed * run * 0.85 * (lerp(1.0, 0.8, armed) * amp * arm_swing + (flex_front - flex_back) * 0.5 * late);
    p.sword = Jp::r(rx(wrist) * sword_rest_r().slerp(posed_sword(-2.046, -0.52, 0.119).r, run));
    p.shield = rest().shield.lerp(posed_shield(0.924, 0.415, -0.121), run);
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

fn footman_loco_pose(t: f32, wp: f32, m: f32, speed: f32, armed: f32) -> Pose {
    idle_pose(t).lerp(&footman_gait(wp, speed, armed), m)
}

/// Footman combat-stance locomotion with two extra axes driven by `movement` —
/// `back` (0..1) cross-fades toward the gait played in REVERSE phase (a backpedal: the hero
/// steps backward while still facing the foe), and `twist` (radians) yaws the pelvis+legs
/// toward the movement while the torso/head counter-rotate to stay square on the target — the
/// classic lower-body-aims-along-movement / upper-body-faces-target split every lock-on game
/// uses, here as a differential yaw on the existing joints.
pub(crate) fn stance_loco_pose(t: f32, wp: f32, m: f32, speed: f32, back: f32, twist: f32, armed: f32) -> Pose {
    let mut p = footman_loco_pose(t, wp, m, speed, armed);
    if back > 0.001 {
        // The same cycle run backward reads as stepping back; the mid-blend "gather step" as the
        // two phases cancel is exactly what a person does reversing direction.
        p = p.lerp(&footman_loco_pose(t, -wp, m, speed, armed), back.clamp(0.0, 1.0));
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
}

#[derive(Default)]
pub struct ClipBlend {
    clip: Clip,
    last: Option<Pose>,
    /// (pose to fade from, start time, duration).
    from: Option<(Pose, f32, f32)>,
    /// The airborne clip leads with the right leg (mirrored) this jump.
    lead_right: bool,
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

const SHEATHE_REACH: f32 = 0.55;
const SHEATHE_DELAY: f32 = 1.6;
const SHEATHE_DELAY_SPRINT: f32 = 0.4;

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
}

/// Swap the two legs of a pose (the clips here are sagittal X turns, so a straight swap mirrors).
fn mirror_legs(p: &Pose) -> Pose {
    let mut m = *p;
    (m.hip_l, m.hip_r) = (p.hip_r, p.hip_l);
    (m.knee_l, m.knee_r) = (p.knee_r, p.knee_l);
    (m.foot_l, m.foot_r) = (p.foot_r, p.foot_l);
    m
}

pub fn hero_anim(
    time: Res<Time>,
    player: Res<super::PlayerRes>,
    dir: Res<crate::cinematic::DirectorState>,
    hero_q: Query<(&Hero, &HeroHealth)>,
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
    mut sword_vis: Query<(&HeroPart, &mut Visibility), Without<super::BackSword>>,
    mut back_vis: Query<&mut Visibility, (With<super::BackSword>, Without<HeroPart>)>,
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

    // ── Sword: drawn for a fight, slung on the back otherwise ──
    let action = hero.attacking || hh.blocking || hero.charge_t >= 0.0 || hero.dash_t >= 0.0;
    let want_drawn = action || hero.victory || hero.soft_pos.is_some() || now < hero.combat_until;
    let sprinting = hero.gait_speed > super::SPEED * 1.3;
    sheath.update(dt, want_drawn, action, sprinting);
    for (part, mut vis) in &mut sword_vis {
        if part.joint == Joint::Sword {
            vis.set_if_neq(if sheath.drawn { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
    for mut vis in &mut back_vis {
        vis.set_if_neq(if sheath.drawn { Visibility::Hidden } else { Visibility::Inherited });
    }
    let armed = sheath.armed;

    let attack = hero.attacking.then(|| attack_phase((hero.attack_t / hero.attack_dur).clamp(0.0, 1.0)));
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
        );
        guard_overlay(&mut p, hero.stance_amt, moving);
        p
    };
    // ── Clip cross-fade ── Each clip below is continuous within itself, but switching between
    // them (stride → jump at take-off, jump → stride on landing, a swing starting mid-run, the
    // dash/charge entering) used to SNAP the whole rig in one frame. On a clip change, freeze the
    // last written pose and ease from it into the new clip over a short, per-transition window.
    let clip = if hero.victory {
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
    } else {
        loco
    };

    if clip != blend.clip {
        let dur = match (blend.clip, clip) {
            (_, Clip::Attack(_)) | (_, Clip::Dash) => 0.07, // actions must stay snappy
            (Clip::Air, Clip::Loco) => 0.09,                 // landing (the squash carries the rest)
            (Clip::Loco, Clip::Air) => 0.12,                 // take-off into the jump/leap
            (_, Clip::Victory) => 0.3,
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
    // The draw/sheathe reach: the sword hand goes up over the right shoulder to the slung hilt
    // and back (the swap happens at the top, in `Sheath::update`).
    let pose = match sheath.reach() {
        Some(w) => {
            let mut p = pose;
            // Fist at the slung grip just under the crossguard (tuned against the viewer).
            p.sh_r = p.sh_r.lerp(Jp::r(e3(-3.1, -0.4, 0.3)), w);
            p.el_r = p.el_r.lerp(Jp::r(rx(-2.0)), w);
            p.head = Jp::r(p.head.r * e3(0.0, -0.25 * w, 0.0)); // a glance at the hand
            p.torso = Jp::r(p.torso.r * e3(-0.05 * w, -0.15 * w, 0.0));
            p
        }
        None => pose,
    };

    // Landing squash: a quick crouch the instant the feet hit, easing back over `LAND_RECOVER`.
    let landing = if *land_at <= 0.0 {
        0.0 // no touchdown yet (fresh boot) — don't play an unearned landing crouch
    } else {
        let u = (1.0 - (now - *land_at) / LAND_RECOVER).clamp(0.0, 1.0);
        u * u
    };

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

    /// World ground speeds spanning a creep, a walk, the base jog and the sprint.
    const SPEEDS: [f32; 7] = [0.5, 1.4, 2.4, 3.5, 4.4, 6.125, 7.0];

    #[test]
    fn footman_boots_stay_grounded_through_every_gait() {
        for v in SPEEDS {
            let g = gait_at(v / hero_model_scale());
            let mut max_lift = 0.0_f32;
            for frame in 0..240 {
                let phase = std::f32::consts::TAU * frame as f32 / 240.0;
                let p = footman_gait(phase, v, 1.0);
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

    /// The moonwalk regression: while a boot is planted, its contact edge must stay put on the
    /// ground as the body travels — the rig-space sweep cancels the root's real speed.
    #[test]
    fn planted_boot_does_not_skate() {
        for v in SPEEDS {
            let vm = v / hero_model_scale();
            let g = gait_at(vm);
            let rate = gait_phase_rate(v);
            let dt = 1.0 / 240.0;
            for frame in 0..200 {
                let phase = std::f32::consts::TAU * frame as f32 / 200.0;
                for right in [false, true] {
                    let u = (phase / std::f32::consts::TAU + if right { 0.5 } else { 0.0 }).rem_euclid(1.0);
                    let u2 = u + rate * dt / std::f32::consts::TAU;
                    if u < 0.02 || u2 > g.duty - 0.02 {
                        continue; // the touchdown / lift-off frames themselves
                    }
                    let a = heel_toe(&footman_gait(phase, v, 1.0), right);
                    let b = heel_toe(&footman_gait(phase + rate * dt, v, 1.0), right);
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
    /// sprinting thigh to 136° and folded the knee to 154°).
    #[test]
    fn swing_joint_angles_stay_human() {
        for (v, hip_max, knee_max) in [(1.4_f32, 40.0_f32, 75.0_f32), (3.5, 55.0, 105.0), (6.125, 75.0, 132.0)] {
            let (mut hi, mut kn) = (0.0_f32, 0.0_f32);
            for i in 0..200 {
                let p = footman_gait(std::f32::consts::TAU * i as f32 / 200.0, v, 0.0);
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
    fn cadence_reads_human() {
        let steps = |v: f32| gait_phase_rate(v) / PI; // two steps per 2π
        let walk = steps(super::super::SPEED);
        assert!((1.8..2.6).contains(&walk), "walk cadence {walk}");
        let run = steps(super::super::SPEED * super::super::SPRINT_MULT);
        assert!((2.6..3.6).contains(&run) && run > walk, "run cadence {run}");
        let sprint = steps(6.125); // a hasted / downhill-road top speed
        assert!((3.2..4.5).contains(&sprint), "sprint cadence {sprint}");
        assert_eq!(gait_phase_rate(0.0), 0.0);
    }

    #[test]
    fn footman_gait_is_periodic_and_blends_back_to_idle() {
        for v in SPEEDS {
            let a = footman_gait(0.0, v, 1.0);
            let b = footman_gait(std::f32::consts::TAU, v, 1.0);
            for j in [Joint::Hips, Joint::Torso, Joint::ShoulderR, Joint::HipL, Joint::HipR, Joint::KneeL, Joint::KneeR, Joint::FootL, Joint::FootR] {
                let d = a.get(j).r.angle_between(b.get(j).r);
                assert!(d < 0.002, "gait not periodic at v={v}: joint {} off by {d}", j as u8);
            }
            let stopped = stance_loco_pose(2.0, 1.7, 0.0, v, 0.0, 0.0, 1.0);
            let idle = idle_pose(2.0);
            assert!(stopped.hips.t.unwrap().distance(idle.hips.t.unwrap()) < 0.0001);
            assert!(stopped.knee_l.r.angle_between(idle.knee_l.r) < 0.001);
        }
    }

    /// Opposite arm to the forward leg — the old gait swung the sword arm WITH the same-side leg.
    #[test]
    fn arms_counter_swing_the_legs() {
        for v in SPEEDS {
            // Left heel strike: left boot out front → right (sword) arm forward (negative X).
            let p = footman_gait(0.0, v, 1.0);
            assert!(heel_toe(&p, false).0.z > heel_toe(&p, true).0.z);
            let fwd = |q: Quat| (q * Vec3::NEG_Y).z;
            assert!(fwd(p.sh_r.r) > fwd(p.sh_l.r), "sword arm must lead with the left leg (v={v})");
        }
    }

    #[test]
    fn combat_guard_preserves_moving_footman_support() {
        for back in [0.0, 1.0] {
            for twist in [-0.7, 0.0, 0.7] {
                for frame in 0..120 {
                    let phase = std::f32::consts::TAU * frame as f32 / 120.0;
                    let mut p = stance_loco_pose(2.0, phase, 1.0, 2.6, back, twist, 1.0);
                    guard_overlay(&mut p, 1.0, 1.0);
                    for right in [false, true] {
                        let (heel, toe) = heel_toe(&p, right);
                        assert!(heel.y.min(toe.y) >= -0.02, "guard must not drive the moving boot into the ground");
                    }
                }
            }
        }
    }
}
