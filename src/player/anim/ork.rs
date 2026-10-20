//! **Ork body language** on the shared biped rig — the orks' own animator, instead of the townsfolk /
//! hero studio clips they used to borrow (an upright march and a sword chop read as a man in an ork
//! suit). Everything here is built from a few physical rules rather than canned angles:
//!
//! - **Feet are planted.** Legs are solved with two-bone IK to ankle targets on the ground ([`leg`]),
//!   so the stance can widen, the hips drop and roll, and nothing slides or sinks. The gait is
//!   foot-locked: the stance foot sweeps back exactly as far as the body travels (the cadence follows
//!   the measured ground speed, [`OrkMotion::track`]).
//! - **Heavy, hunched, wide.** Knees always soft, toes out, pelvis tipped forward, trunk hunched over
//!   it, head thrust forward and up to look ahead; the weight rolls from foot to foot in the idle and
//!   the walk waddles over each stance leg ([`Stance`]). Lean builds (scout/shaman) carry less of it.
//! - **Arms by hand targets.** Hands are placed in the chest frame (or the rig frame for the blows) and
//!   the arm is solved with the elbow pointed where an arm would put it ([`arm`]); the club is laid
//!   along a wanted direction by forearm twist + wrist deviation, within a hammer grip's range
//!   ([`grip`]) — so the club hangs from the fist at ease, stands up in the guard, and whips
//!   through the blow without the fist ever bending backwards.
//! - **Blows land on the hit.** The brains report how long until the next blow lands
//!   (`Ork::strike_in`: the swing cooldown, or the closing time while charging) and stamp the instant
//!   it lands (`Ork::atk_anim`, unchanged: damage is dealt there). The wind-up plays *before* the
//!   stamp, timed off the countdown, so the downswing meets the target exactly at the damage instant;
//!   anticipation → strike → follow-through → recovery ([`ork_pose`]). An unforeseen blow (no
//!   countdown, e.g. a scripted stamp) still plays its downswing, just after the stamp.
//!
//! Cost: one pose per visible ork per frame — a handful of IK solves and pose blends, no allocation.
//! The blow keyframes are solved once per body build and cached ([`keys`]).

use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

use bevy::prelude::*;

use super::mixamo_clips as mc;
use super::swordplay::Frame;
use super::{action_over_loco, e3, lerp, rest, rx, sit_pose, smoothstep, Jp, Pose};
use crate::biped::BipedDrive;

/// Rig segment lengths (`biped::spawn_biped`): thigh, shin, upper arm, forearm.
const THIGH: f32 = 0.40;
const SHIN: f32 = 0.45;
const UPPER: f32 = 0.30;
const FORE: f32 = 0.25;
/// Ankle-joint height above the rig floor at rest (hips 1.05 − 0.05 − thigh − shin).
const ANKLE_Y: f32 = 0.15;

/// Seconds of the downswing: cocked → the blow landing. The countdown below this plays it.
pub(crate) const SWING_T: f32 = 0.14;
/// Seconds before a blow lands that the wind-up starts (guard → cocked over `WIND_T − SWING_T`).
const WIND_T: f32 = 0.55;
/// `Ork::strike_in` for an ork in a fight but not about to swing (prowling the melee ring).
pub(crate) const STRIKE_HOLD: f32 = 9.0;
/// How fast an abandoned wind-up (the target stepped out of reach, a stagger) settles back
/// (swing units / s).
const WIND_DECAY: f32 = 3.0;
/// A blow the countdown did not foresee plays its remaining downswing after the stamp at this
/// many seconds per swing unit.
const LATE_SWING: f32 = 0.1;
/// Impact → the end of the follow-through (s).
const FOLLOW_T: f32 = 0.16;
/// Follow-through → back on guard (s).
const RECOVER_T: f32 = 0.42;
/// Stance share of a leg cycle: a walk (double support) and a charge (a flight phase).
const WALK_DUTY: f32 = 0.62;
const RUN_DUTY: f32 = 0.42;

/// The blows an ork has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OrkBlow {
    /// Two-handed-strength overhead club smash.
    Smash,
    /// Flat backhand-to-forehand swipe across the front.
    Swipe,
    /// The shaman's cast: staff hauled overhead, then levelled at the target.
    Cast,
}

/// Body build + carriage of one ork kind. `sdx`/`hdx` must match the rig it was spawned with
/// (`orks::rig_dims`); `id` keys the cached blow keyframes ([`keys`]), so it must be unique per
/// distinct build.
#[derive(Clone, Copy)]
pub(crate) struct OrkBuild {
    pub(crate) id: usize,
    pub(crate) sdx: f32,
    pub(crate) hdx: f32,
    /// 0 lean and light .. 1 heavy: stance width, hunch, toe-out, how deep the knees sit.
    pub(crate) heavy: f32,
    /// Carries the shaman's staff (casts instead of clubbing).
    pub(crate) staff: bool,
    /// Chance a club blow is a swipe rather than a smash.
    pub(crate) swipe: f32,
}

/// Distinct builds [`keys`] caches (`OrkBuild::id` < this).
pub(crate) const BUILD_SLOTS: usize = 8;

/// The ork animator's per-ork state, advanced from the AI by [`OrkMotion::track`] (in
/// `orks::ork_drive`) and read by [`ork_pose`] (in `biped::animate_biped`).
#[derive(Component, Clone, Copy)]
pub(crate) struct OrkMotion {
    pub(crate) build: OrkBuild,
    /// Wind-up of the coming blow: 0 guard .. 1 cocked .. 2 landing.
    wind: f32,
    /// `atk_anim` of the last blow seen (0 = none yet).
    stamp: f32,
    /// Seconds since that blow landed (∞ = none).
    since: f32,
    /// The wind-up it had reached when it landed (2 = on time; less = the rest plays late).
    from: f32,
    /// The blow being played out, and the one being wound up.
    swing: OrkBlow,
    next: OrkBlow,
    /// 0 at ease .. 1 in a fight (guard stance, club up), eased.
    engaged: f32,
    /// Ground speed (rig units / s), eased, and the ground covered per gait cycle at it.
    speed: f32,
    stride: f32,
    last: Vec2,
    fresh: bool,
    seed: u32,
}

impl OrkMotion {
    pub(crate) fn new(build: OrkBuild, seed: u32) -> Self {
        let mut m = OrkMotion {
            build,
            wind: 0.0,
            stamp: 0.0,
            since: f32::INFINITY,
            from: 2.0,
            swing: OrkBlow::Smash,
            next: OrkBlow::Smash,
            engaged: 0.0,
            speed: 0.0,
            stride: 0.0,
            last: Vec2::ZERO,
            fresh: true,
            seed: seed | 1,
        };
        m.next = m.pick();
        m.swing = m.next;
        m
    }

    /// The next blow: a staff casts; a club smashes or (by the build's odds) swipes.
    fn pick(&mut self) -> OrkBlow {
        if self.build.staff {
            return OrkBlow::Cast;
        }
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        if ((self.seed >> 8) & 0xffff) as f32 / 65_536.0 < self.build.swipe {
            OrkBlow::Swipe
        } else {
            OrkBlow::Smash
        }
    }

    /// Force the blow being wound up (the viewer's `smash` / `swipe` previews).
    pub(crate) fn set_next(&mut self, b: OrkBlow) {
        self.next = b;
    }

    /// Advance from the AI once per frame (after the brains): `atk_anim` is the instant the last
    /// blow landed, `strike_in` the seconds until the next lands (∞ = not fighting), `pos` the
    /// ground position and `scale` the root scale (world units per rig unit). Also advances the
    /// gait phase `d.walk_phase` at the cadence that keeps the planted foot locked to the ground.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn track(&mut self, d: &mut BipedDrive, atk_anim: f32, strike_in: f32, pos: Vec2, scale: f32, now: f32, dt: f32) {
        if self.fresh {
            // Desynchronise a warband's footfalls (they'd otherwise all start the cycle at 0).
            self.fresh = false;
            self.last = pos;
            d.walk_phase = (self.seed % 628) as f32 * 0.01;
        }
        if dt > 0.0 {
            let v = ((pos - self.last).length() / dt / scale.max(0.01)).min(8.0);
            self.speed += (v - self.speed) * (dt * 8.0).min(1.0);
        }
        self.last = pos;
        // Walk → charge by ground speed (a patrol walks; a charge, or a horde's forced march, runs).
        // Cadence from speed: the stance sweep a heavy ork comfortably reaches (a short, choppy
        // stride; longer at a charge) → cycles per second. Clamped, so a near-standstill prowl
        // shuffles with short steps instead of freezing mid-stride.
        d.run_amt = ss(1.9, 2.8, self.speed);
        let run = d.run_amt;
        let cadence = if mixamo_gait() {
            mx_cadence(self.speed, run)
        } else {
            let duty = lerp(WALK_DUTY, RUN_DUTY, run);
            (TAU * self.speed * duty / lerp(0.66, 0.82, run)).clamp(4.0, 13.0)
        };
        self.stride = self.speed * TAU / cadence;
        d.walk_phase += cadence * dt;
        if d.walk_phase > TAU * 64.0 {
            d.walk_phase -= TAU * 64.0;
        }

        let fight = if strike_in.is_finite() { 1.0 } else { 0.0 };
        self.engaged += (fight - self.engaged) * (dt * 4.0).min(1.0);

        // A new blow landed: what was wound up is what lands; pick the next.
        if atk_anim > 0.0 && atk_anim != self.stamp {
            self.stamp = atk_anim;
            self.from = self.wind;
            self.swing = self.next;
            self.next = self.pick();
            self.wind = 0.0;
        }
        self.since = if self.stamp > 0.0 { (now - self.stamp).max(0.0) } else { f32::INFINITY };
        // Anticipation follows the countdown up exactly (so the downswing lands on the stamp);
        // an abandoned one settles back.
        let w = wind_at(strike_in);
        self.wind = if w >= self.wind { w } else { (self.wind - WIND_DECAY * dt).max(w) };
    }
}

/// Wind-up for a blow landing in `strike_in` s: 0 (guard) until [`WIND_T`], 1 (cocked) at
/// [`SWING_T`], 2 at the instant it lands.
fn wind_at(strike_in: f32) -> f32 {
    if strike_in >= WIND_T {
        0.0
    } else if strike_in > SWING_T {
        (WIND_T - strike_in) / (WIND_T - SWING_T)
    } else {
        2.0 - strike_in.max(0.0) / SWING_T
    }
}

/// The body's forward step into a blow (world units, applied to the root by the camp brain so it
/// can be kept clear of the hero): rides in over the downswing, fully in at the instant the blow
/// lands, then eases back out.
pub(crate) fn strike_lunge(atk_anim: f32, strike_in: f32, now: f32) -> f32 {
    const IN: f32 = 0.2;
    const OUT: f32 = 0.45;
    let pre = 1.0 - (strike_in / IN).clamp(0.0, 1.0);
    let post = if atk_anim > 0.0 && now >= atk_anim { 1.0 - ((now - atk_anim) / OUT).clamp(0.0, 1.0) } else { 0.0 };
    0.22 * smoothstep(pre.max(post))
}

// ── Solvers ─────────────────────────────────────────────────────────────────────────────

/// Two-bone chain `a`,`b` reaching `len`: (angle between bone `a` and the root→target line,
/// bend at the middle joint).
fn two_bone(a: f32, b: f32, len: f32) -> (f32, f32) {
    let l = len.clamp((a - b).abs() + 1e-3, (a + b) * 0.999);
    let cos_a = ((a * a + l * l - b * b) / (2.0 * a * l)).clamp(-1.0, 1.0);
    let cos_g = ((a * a + b * b - l * l) / (2.0 * a * b)).clamp(-1.0, 1.0);
    (cos_a.acos(), PI - cos_g.acos())
}

/// A frame whose −Y runs along `dn` and whose +Z leans toward `side` (where the knee / elbow points).
fn hinge_frame(dn: Vec3, side: Vec3) -> Quat {
    let h = side.cross(dn).try_normalize().unwrap_or(Vec3::X);
    let y = -dn;
    Quat::from_mat3(&Mat3::from_cols(h, y, h.cross(y)))
}

/// One leg: hip at `side`·`hdx` on the pelvis, ankle on `ankle` (rig frame), knee toward
/// `knee_dir`, the foot turned to `foot_w` (rig frame). Returns the (hip, knee, foot) joints.
fn leg(hips_t: Vec3, pelvis: Quat, hdx: f32, side: f32, ankle: Vec3, knee_dir: Vec3, foot_w: Quat) -> (Jp, Jp, Jp) {
    let hip = hips_t + pelvis * Vec3::new(side * hdx, -0.05, 0.0);
    let d = ankle - hip;
    let (alpha, bend) = two_bone(THIGH, SHIN, d.length());
    let thigh = hinge_frame(d.try_normalize().unwrap_or(Vec3::NEG_Y), knee_dir) * rx(-alpha);
    let shin = thigh * rx(bend);
    (Jp::r(pelvis.inverse() * thigh), Jp::r(rx(bend)), Jp::r(shin.inverse() * foot_w))
}

/// One arm: shoulder at `side`·`sdx` on the chest (`ct`/`cr`), wrist on `hand` (rig frame), elbow
/// toward `elbow`. Returns (shoulder, elbow, the hand's rig-frame rotation). The hand's thumb side
/// (+Z) faces away from the elbow, as a fist hangs at rest.
fn arm(ct: Vec3, cr: Quat, sdx: f32, side: f32, hand: Vec3, elbow: Vec3) -> (Quat, Quat, Quat) {
    let sh = ct + cr * Vec3::new(side * sdx, 0.27, 0.01);
    let d = hand - sh;
    let (alpha, bend) = two_bone(UPPER, FORE, d.length());
    let upper = hinge_frame(d.try_normalize().unwrap_or(Vec3::NEG_Y), elbow) * rx(-alpha) * Quat::from_rotation_y(PI);
    let el = rx(-bend);
    (cr.inverse() * upper, el, upper * el)
}

/// Lay a club / staff (+Y of the `Sword` joint, grip at the origin) along rig-frame `dir` from a
/// hand at `hand_w`: the forearm turns about its own axis (pronation / supination, ≤ 75°) to bring
/// `dir` round to the thumb side, then the wrist deviates to set its angle off the forearm (a
/// hammer grip's range). Returns (the forearm twist, to append to the elbow; the grip rotation).
fn grip(hand_w: Quat, dir: Vec3) -> (Quat, Quat) {
    const LO: f32 = 0.85;
    const HI: f32 = 2.8;
    let l = hand_w.inverse() * dir.normalize_or_zero();
    // Near the forearm's own line any twist will do — fade it out there (it is ill-conditioned).
    let tw = l.x.atan2(l.z).clamp(-1.3, 1.3) * ss(0.1, 0.4, l.x.hypot(l.z));
    let twist = Quat::from_rotation_y(tw);
    let l2 = twist.inverse() * l;
    let mut th = l2.z.atan2(l2.y).rem_euclid(TAU);
    if !(LO..=HI).contains(&th) {
        th = if th < (HI + LO + TAU) * 0.5 { HI } else { LO };
    }
    let ph = l2.x.clamp(-1.0, 1.0).asin().clamp(-0.4, 0.4);
    (twist, rx(th) * Quat::from_rotation_z(-ph))
}

fn ss(e0: f32, e1: f32, x: f32) -> f32 {
    smoothstep((x - e0) / (e1 - e0))
}

// ── Carriage ────────────────────────────────────────────────────────────────────────────

/// The standing frame a build carries: stance width (beyond the hip joints), toe-out, hip height,
/// trunk hunch and pelvic tilt.
#[derive(Clone, Copy)]
struct Stance {
    width: f32,
    toe: f32,
    hy: f32,
    lean: f32,
    tilt: f32,
}

impl Stance {
    fn of(heavy: f32) -> Self {
        Stance { width: 0.06 + 0.04 * heavy, toe: 0.2 + 0.1 * heavy, hy: 0.97 - 0.04 * heavy, lean: 0.14 + 0.1 * heavy, tilt: 0.07 }
    }
}

/// Plant one foot: `x` beyond the stance width, `z` fore/aft, `yaw` extra toe-out, `lift` of the
/// ankle and `pitch` of the sole (+ heel up / − toes up).
#[allow(clippy::too_many_arguments)]
fn foot(p: &mut Pose, b: &OrkBuild, st: &Stance, hips_t: Vec3, pelvis: Quat, side: f32, x: f32, z: f32, yaw: f32, lift: f32, pitch: f32) {
    let yaw = side * (st.toe + yaw);
    let ankle = Vec3::new(side * (b.hdx + st.width + x), ANKLE_Y + lift, z);
    let (h, k, f) = leg(hips_t, pelvis, b.hdx, side, ankle, Quat::from_rotation_y(yaw * 0.7) * Vec3::Z, Quat::from_rotation_y(yaw) * rx(pitch));
    if side < 0.0 {
        (p.hip_l, p.knee_l, p.foot_l) = (h, k, f);
    } else {
        (p.hip_r, p.knee_r, p.foot_r) = (h, k, f);
    }
}

/// The chest frame (torso joint position, rig-frame rotation).
fn chest(hips_t: Vec3, pelvis: Quat, trunk: Quat) -> (Vec3, Quat) {
    (hips_t + pelvis * Vec3::new(0.0, 0.15, 0.0), pelvis * trunk)
}

/// The head held to a rig-frame gaze (yaw, pitch) whatever the trunk does — the eyes stay on the
/// way ahead / the target.
fn look(cr: Quat, yaw: f32, pitch: f32) -> Jp {
    Jp::r(cr.inverse() * Quat::from_rotation_y(yaw) * rx(pitch))
}

/// Pose the left (off-hand) arm.
fn arm_l(p: &mut Pose, b: &OrkBuild, ct: Vec3, cr: Quat, hand: Vec3, elbow: Vec3) {
    let (s, e, _) = arm(ct, cr, b.sdx, -1.0, hand, elbow);
    (p.sh_l, p.el_l) = (Jp::r(s), Jp::r(e));
}

/// Pose the right (weapon) arm and lay the weapon along `dir`.
fn arm_r(p: &mut Pose, b: &OrkBuild, ct: Vec3, cr: Quat, hand: Vec3, elbow: Vec3, dir: Vec3) {
    let (s, e, hw) = arm(ct, cr, b.sdx, 1.0, hand, elbow);
    let (twist, g) = grip(hw, dir);
    (p.sh_r, p.el_r, p.sword) = (Jp::r(s), Jp::r(e * twist), Jp::r(g));
}

/// Same, the arm aimed by direction from the shoulder + elbow bend (rig frame) — exact control of
/// how far a striking arm is thrown out.
#[allow(clippy::too_many_arguments)]
fn arm_r_dir(p: &mut Pose, b: &OrkBuild, ct: Vec3, cr: Quat, dir: Vec3, bend: f32, elbow: Vec3, weapon: Vec3) {
    let reach = (UPPER * UPPER + FORE * FORE + 2.0 * UPPER * FORE * bend.cos()).sqrt();
    let sh = ct + cr * Vec3::new(b.sdx, 0.27, 0.01);
    arm_r(p, b, ct, cr, sh + dir.normalize() * reach, elbow, weapon);
}

/// Chest-frame arm targets: (left hand, left elbow, right hand, right elbow, weapon direction).
/// `x` of the hands is relative to the shoulder spread.
struct Arms {
    l: Vec3,
    le: Vec3,
    r: Vec3,
    re: Vec3,
    w: Vec3,
}

/// At ease: the off arm hangs heavy off the lats; the club hangs from the right fist (the staff
/// stood upright beside it).
fn arms_ease(staff: bool) -> Arms {
    if staff {
        Arms { l: Vec3::new(-0.109, -0.24, 0.19), le: Vec3::new(-0.3, -0.25, -0.92), r: Vec3::new(0.14, -0.03, 0.24), re: Vec3::new(0.5, -0.34, -0.8), w: Vec3::new(0.0, 0.99, -0.16) }
    } else {
        // The club is carried low in the fist at the right hip (forearm forward, elbow back), its
        // head canted out and forward at hip height — not dangling by the shin — clear of the
        // leg's swing and of the skinned body's heavy deltoid (propped on the shoulder, its head
        // sank through it: `club_clears_the_shoulder`).
        Arms { l: Vec3::new(-0.109, -0.22, 0.187), le: Vec3::new(-0.29, -0.29, -0.91), r: Vec3::new(0.06, -0.08, 0.2), re: Vec3::new(0.35, -0.2, -0.92), w: Vec3::new(0.44, -0.33, 0.83) }
    }
}

/// On guard: off fist up in front, club stood up before the right shoulder (staff held out).
fn arms_guard(staff: bool) -> Arms {
    if staff {
        Arms { l: Vec3::new(-0.087, 0.18, 0.29), le: Vec3::new(-0.62, -0.75, -0.23), r: Vec3::new(-0.06, 0.19, 0.42), re: Vec3::new(0.76, -0.65, -0.01), w: Vec3::new(-0.13, 0.99, -0.05) }
    } else {
        Arms { l: Vec3::new(-0.087, 0.244, 0.254), le: Vec3::new(-0.62, -0.77, -0.15), r: Vec3::new(0.037, 0.237, 0.352), re: Vec3::new(0.7, -0.71, 0.09), w: Vec3::new(-0.2, 0.94, -0.26) }
    }
}

/// Pose both arms from chest-frame targets (`calm` → `fight` by `e`), plus a per-frame offset of
/// the off hand (`swing`, chest frame) and the weapon hand (`bob`).
#[allow(clippy::too_many_arguments)]
fn arms_at(p: &mut Pose, b: &OrkBuild, ct: Vec3, cr: Quat, e: f32, swing: Vec3, bob: Vec3) {
    let (c, f) = (arms_ease(b.staff), arms_guard(b.staff));
    let x = |v: Vec3, s: f32| Vec3::new(v.x + s * b.sdx, v.y, v.z);
    let at = |v: Vec3| ct + cr * v;
    let l = x(c.l, -1.0).lerp(x(f.l, -1.0), e) + swing;
    let r = x(c.r, 1.0).lerp(x(f.r, 1.0), e) + bob;
    arm_l(p, b, ct, cr, at(l), cr * c.le.lerp(f.le, e));
    arm_r(p, b, ct, cr, at(r), cr * c.re.lerp(f.re, e), cr * c.w.lerp(f.w, e));
}

/// Standing at ease: the weight drifts from foot to foot (the hips ride over the loaded leg and
/// roll, the free knee softens), the chest heaves, the head looks round.
fn idle(t: f32, b: &OrkBuild) -> Pose {
    let st = Stance::of(b.heavy);
    let breath = (t * 1.9).sin();
    // The weight across the feet: −1 on the left foot .. +1 on the right.
    let w = 0.85 * (t * 0.37).sin() + 0.15 * (t * 0.91 + 1.3).sin();
    let mut p = rest();
    let pelvis = e3(st.tilt, -0.08 * w, -0.05 * w);
    let hips_t = Vec3::new(0.06 * w, st.hy + 0.02 - 0.014 * w.abs() + 0.005 * breath, 0.0);
    p.hips = Jp { t: Some(hips_t), r: pelvis };
    foot(&mut p, b, &st, hips_t, pelvis, -1.0, 0.0, 0.05, 0.0, 0.0, 0.0);
    foot(&mut p, b, &st, hips_t, pelvis, 1.0, 0.0, -0.04, 0.05, 0.0, 0.0);
    let trunk = e3(st.lean + 0.03 * breath, 0.06 * w, 0.07 * w);
    p.torso = Jp::r(trunk);
    let (ct, cr) = chest(hips_t, pelvis, trunk);
    p.head = look(cr, 0.3 * (t * 0.29).sin() * (t * 0.13).sin(), 0.04 + 0.05 * (t * 0.23).sin());
    arms_at(&mut p, b, ct, cr, 0.0, Vec3::new(0.0, 0.01 * breath, 0.0), Vec3::new(0.0, 0.012 * breath, 0.0));
    p
}

/// The left foot forward, the right turned out behind — bladed for a right-handed blow.
const GUARD_FEET: [(f32, f32, f32, f32); 2] = [(-1.0, 0.0, 0.22, -0.12), (1.0, 0.03, -0.2, 0.28)];

/// On guard: bladed stance, hips sunk, chest coiled a little toward the club side, fast breathing.
fn guard(t: f32, b: &OrkBuild) -> Pose {
    let st = Stance::of(b.heavy);
    let breath = (t * 2.6).sin();
    let mut p = rest();
    let pelvis = e3(st.tilt + 0.04, 0.22, 0.0);
    let hips_t = Vec3::new(0.0, st.hy - 0.06 + 0.005 * breath, -0.02);
    p.hips = Jp { t: Some(hips_t), r: pelvis };
    for (side, x, z, yaw) in GUARD_FEET {
        foot(&mut p, b, &st, hips_t, pelvis, side, x, z, yaw, 0.0, 0.0);
    }
    let trunk = e3(st.lean + 0.06 + 0.02 * breath, 0.12, 0.0);
    p.torso = Jp::r(trunk);
    let (ct, cr) = chest(hips_t, pelvis, trunk);
    p.head = look(cr, 0.0, 0.05);
    arms_at(&mut p, b, ct, cr, 1.0, Vec3::new(0.0, 0.01 * breath, 0.0), Vec3::new(0.0, 0.01 * breath, 0.0));
    p
}

/// Walk ⇄ charge (`run`) at gait phase `c`, foot-locked: the stance foot sweeps `stride`·duty back
/// under the hips. The pelvis rides over each stance leg, rolls and swings; the trunk counter-
/// twists and hunches further into a charge. Arms per [`arms_at`] (at ease ⇄ guard by `e`).
fn gait(c: f32, run: f32, stride: f32, e: f32, b: &OrkBuild) -> Pose {
    let st = Stance::of(b.heavy);
    let duty = lerp(WALK_DUTY, RUN_DUTY, run);
    let sweep = (stride * duty).min(0.9);
    let lift = lerp(0.1, 0.2, run);
    let mut p = rest();
    // Left leg cycle (heel strike at 0, so footfalls land on walk_phase = kπ); right half a cycle on.
    let ul = (c / TAU).rem_euclid(1.0);
    let bob = (TAU * 2.0 * (ul - duty * 0.5)).cos(); // +1 at each mid-stance
    let sway = -(TAU * (ul - duty * 0.5)).cos(); // −1 over the left foot
    let hips_t = Vec3::new(0.045 * sway * (1.0 - 0.5 * run), st.hy - 0.06 * run + lerp(0.022, 0.03, run) * bob, 0.0);
    let yaw = 0.11 * (TAU * ul).cos() * (1.0 + 0.3 * b.heavy);
    let pelvis = e3(st.tilt + 0.08 * run, yaw, 0.05 * sway);
    p.hips = Jp { t: Some(hips_t), r: pelvis };
    for (side, u) in [(-1.0, ul), (1.0, (ul + 0.5).rem_euclid(1.0))] {
        let (z, y, pitch) = if u < duty {
            // Stance: heel strike (toes up) → flat → heel off onto the ball.
            let k = u / duty;
            (sweep * (0.5 - k), 0.06 * ss(0.7, 1.0, k), -0.25 * (1.0 - ss(0.0, 0.25, k)) + 0.45 * ss(0.7, 1.0, k))
        } else {
            // Swing: toe-off → knee up → reach for the next heel strike.
            let k = (u - duty) / (1.0 - duty);
            let z = sweep * (-0.5 + 0.5 * (1.0 - (PI * k).cos()));
            (z, lift * (PI * k).sin().max(0.0).powf(1.5) + 0.06 * (1.0 - ss(0.0, 0.3, k)), lerp(0.6, -0.25, ss(0.1, 0.9, k)))
        };
        foot(&mut p, b, &st, hips_t, pelvis, side, -0.15 * st.width, z, -0.2 * st.toe, y, pitch);
    }
    let trunk = e3(st.lean + lerp(0.08, 0.3, run) + 0.02 * bob, -1.6 * yaw, -0.05 * sway);
    p.torso = Jp::r(trunk);
    let (ct, cr) = chest(hips_t, pelvis, trunk);
    p.head = look(cr, 0.0, 0.08);
    // The off arm swings against its leg (further at a charge, and only at ease — on guard it holds
    // up); the carried club bobs with the step.
    let sw = (TAU * ul).cos() * (1.0 - e);
    let swing = Vec3::new(0.0, 0.06 * (-sw).max(0.0), -0.22 * sw * (1.0 + 0.4 * run) + 0.08 * (1.0 - e) + 0.12 * run);
    arms_at(&mut p, b, ct, cr, e, swing, Vec3::new(0.0, 0.015 * bob, 0.0));
    p
}

// ── Mixamo gait ─────────────────────────────────────────────────────────────────────────

/// The orks walk and charge on Mixamo's "Orc Walk" / "Mutant Run" (`mixamo_clips`, baked by
/// `tools/mixamo_retarget/bake.py`) unless `FOREST_ORK_GAIT=curves` (the hand-built [`gait`]).
pub(crate) fn mixamo_gait() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_ORK_GAIT").map(|v| v != "curves").unwrap_or(true))
}

/// Rig leg length (hip → ankle at full stretch): the clips' unit.
const LEG: f32 = THIGH + SHIN;

/// A baked loop: frames, the planted ankle's sweep speed at native rate (legs / s), the share of
/// the cycle a foot is down.
struct MxLoop {
    clip: &'static [[f32; 53]],
    native: f32,
    duty: f32,
}

impl MxLoop {
    /// Legs the planted ankle sweeps back through in one stance.
    fn sweep(&self) -> f32 {
        self.native * self.duty * self.clip.len() as f32 / mc::FPS
    }
    /// Native cycle rate (rad / s).
    fn rate(&self) -> f32 {
        TAU * mc::FPS / self.clip.len() as f32
    }
}

fn mx_loops() -> [MxLoop; 2] {
    [
        MxLoop { clip: &mc::ORC_WALK, native: mc::ORC_WALK_SPEED, duty: mc::ORC_WALK_DUTY },
        MxLoop { clip: &mc::ORC_RUN, native: mc::ORC_RUN_SPEED, duty: mc::ORC_RUN_DUTY },
    ]
}

/// The walk ⇄ charge blend's shared (duty, sweep in legs, native rate) at run weight `run`: both
/// loops are time-warped onto one duty and stretched onto one sweep, so one phase rate pins the
/// planted foot for both (as the hero's `mixamo::gait`).
fn mx_shared(run: f32) -> (f32, f32, f32) {
    let [w, r] = mx_loops();
    (lerp(w.duty, r.duty, run), lerp(w.sweep(), r.sweep(), run), lerp(w.rate(), r.rate(), run))
}

/// Gait-phase rate (rad / s) at `speed` rig units / s: foot-locked, but never slower than half the
/// clips' own cadence — a prowl at a crawl takes short steps ([`mx_gait`] shortens the sweep to
/// match) instead of a slow-motion stride.
fn mx_cadence(speed: f32, run: f32) -> f32 {
    let (duty, sweep, native) = mx_shared(run);
    (TAU * speed * duty / (sweep * LEG)).clamp(0.5 * native, 2.0 * native)
}

/// The lowest each loop's ankles reach above their T-pose height (legs): the planted height.
fn mx_floor(i: usize) -> [f32; 2] {
    static FLOOR: [OnceLock<[f32; 2]>; 2] = [const { OnceLock::new() }; 2];
    *FLOOR[i].get_or_init(|| {
        let tp = Frame::of(&mc::TPOSE);
        let clip = mx_loops()[i].clip;
        let lo = |k: usize| clip.iter().map(|r| Frame::of(r).ankle[k].y - tp.ankle[k].y).fold(f32::MAX, f32::min);
        [lo(0), lo(1)]
    })
}

/// Walk ⇄ charge on the Mixamo loops at gait phase `c` (left heel strike at 0): the clips own the
/// legs (two-bone IK onto their ankle paths, knees where theirs point), the pelvis's bob / sway /
/// roll and the trunk; the ork keeps its own carriage on top — wider, toed-out feet, the pelvis
/// tipped and the trunk hunched by build — and its own arms ([`arms_at`]: the club and the off
/// fist), which the blows take over. `stride` is the ground covered per cycle ([`OrkMotion`]).
fn mx_gait(c: f32, run: f32, stride: f32, e: f32, b: &OrkBuild) -> Pose {
    let st = Stance::of(b.heavy);
    let tp = Frame::of(&mc::TPOSE);
    let (duty, sweep, _) = mx_shared(run);
    // The planted foot must sweep back exactly what the body covers while it is down.
    let k = (stride * duty / (sweep * LEG)).clamp(0.2, 1.8);
    let u = c / TAU;
    let loops = mx_loops();
    let ws = [1.0 - run, run];
    let mut f: Option<(Frame, f32)> = None;
    let mut floor = [0.0f32; 2];
    for (i, l) in loops.iter().enumerate() {
        let w = ws[i];
        if w < 1e-3 {
            continue;
        }
        let fl = mx_floor(i);
        floor = [floor[0] + fl[0] * w, floor[1] + fl[1] * w];
        let fr = super::mixamo::sample_loop(l.clip, super::mixamo::warp(u, duty, l.duty));
        let fr = super::mixamo::stretch(fr, k * sweep / l.sweep());
        f = Some(match f {
            None => (fr, w),
            Some((acc, wa)) => (acc.lerp(&fr, w / (wa + w)), wa + w),
        });
    }
    let Some((f, wsum)) = f else { return gait(c, run, stride, e, b) };
    let floor = floor.map(|x| x / wsum);

    let mut p = rest();
    let pelvis = e3(st.tilt, 0.0, 0.0) * f.pelvis_r;
    // Half the build's crouch on top of the clip's own hip travel (the orc walk already sits low).
    let hips_t = Vec3::new(0.0, 1.05 - 0.5 * (1.05 - st.hy), 0.0) + (f.pelvis - tp.pelvis) * LEG;
    p.hips = Jp { t: Some(hips_t), r: pelvis };
    for (i, side) in [(0usize, -1.0f32), (1, 1.0)] {
        let d = f.ankle[i] - tp.ankle[i];
        let lift = (d.y - floor[i]).max(0.0) * LEG;
        let ankle = Vec3::new(side * (b.hdx + st.width) + d.x * LEG, ANKLE_Y + lift, d.z * LEG);
        let toe = Quat::from_rotation_y(side * st.toe);
        let (h, kn, ft) = leg(hips_t, pelvis, b.hdx, side, ankle, toe * f.pole[i], toe * f.foot[i]);
        if side < 0.0 {
            (p.hip_l, p.knee_l, p.foot_l) = (h, kn, ft);
        } else {
            (p.hip_r, p.knee_r, p.foot_r) = (h, kn, ft);
        }
    }
    // The build's hunch on top of the walk. "Mutant Run" folds the trunk near level, which laid the
    // guard's stood-up club out flat ahead like a lance: a charge keeps two thirds of its bend.
    let chest_w = e3(st.tilt + lerp(0.6, 0.15, run) * st.lean, 0.0, 0.0) * f.chest.slerp(Quat::IDENTITY, 0.35 * run);
    let trunk = pelvis.inverse() * chest_w;
    p.torso = Jp::r(trunk);
    let (ct, cr) = chest(hips_t, pelvis, trunk);
    p.head = look(cr, 0.0, 0.08);
    // The off arm swings against its leg at ease (on guard it holds up); the club bobs with the step.
    let ul = u.rem_euclid(1.0);
    let sw = (TAU * ul).cos() * (1.0 - e);
    let bob = (TAU * 2.0 * (ul - duty * 0.5)).cos();
    let swing = Vec3::new(0.0, 0.06 * (-sw).max(0.0), -0.22 * sw * (1.0 + 0.4 * run) + 0.08 * (1.0 - e) + 0.12 * run);
    arms_at(&mut p, b, ct, cr, e, swing, Vec3::new(0.0, 0.015 * bob, 0.0));
    p
}

// ── Blows ───────────────────────────────────────────────────────────────────────────────

/// One blow keyframe: hips (position, pelvis), trunk, gaze, the feet (front: z, lift, pitch; back:
/// lift, pitch), and the weapon arm either to a hand position or by direction + bend.
struct Key {
    hips: Vec3,
    pelvis: Quat,
    trunk: Quat,
    gaze: (f32, f32),
    front: (f32, f32, f32),
    back: (f32, f32),
    hand: Reach,
    elbow: Vec3,
    weapon: Vec3,
    off: Vec3,
    off_elbow: Vec3,
}

enum Reach {
    To(Vec3),
    Dir(Vec3, f32),
}

fn key_pose(k: &Key, b: &OrkBuild) -> Pose {
    let st = Stance::of(b.heavy);
    let mut p = rest();
    let hips_t = k.hips + Vec3::Y * st.hy;
    p.hips = Jp { t: Some(hips_t), r: k.pelvis };
    let [(fs, fx, _, fy), (bs, bx, bz, by)] = GUARD_FEET;
    foot(&mut p, b, &st, hips_t, k.pelvis, fs, fx, k.front.0, fy, k.front.1, k.front.2);
    foot(&mut p, b, &st, hips_t, k.pelvis, bs, bx, bz, by, k.back.0, k.back.1);
    p.torso = Jp::r(k.trunk);
    let (ct, cr) = chest(hips_t, k.pelvis, k.trunk);
    p.head = look(cr, k.gaze.0, k.gaze.1);
    match k.hand {
        Reach::To(h) => arm_r(&mut p, b, ct, cr, h, k.elbow, k.weapon),
        Reach::Dir(d, bend) => arm_r_dir(&mut p, b, ct, cr, d, bend, k.elbow, k.weapon),
    }
    arm_l(&mut p, b, ct, cr, k.off, k.off_elbow);
    p
}

/// Keyframes (cocked, landing, follow-through) of a blow, rig frame; `hips.y` is relative to the
/// build's stance height.
fn blow_keys(blow: OrkBlow) -> [Key; 3] {
    let v = Vec3::new;
    match blow {
        // Overhead smash: weight rocks back and the front foot lifts as the club is hauled up behind
        // the head; the foot stamps down and the chest folds onto the blow as it lands at chest
        // height out in front; the club carries on down toward the ground.
        OrkBlow::Smash => [
            Key {
                hips: v(0.0, -0.03, -0.08),
                pelvis: e3(0.0, 0.3, 0.0),
                trunk: e3(-0.12, 0.32, 0.08),
                gaze: (-0.08, -0.05),
                front: (0.12, 0.07, -0.2),
                back: (0.0, 0.0),
                hand: Reach::To(v(0.3, 1.88, -0.12)),
                elbow: v(0.35, 0.55, 0.75),
                weapon: v(-0.15, -0.5, -0.85),
                off: v(-0.18, 1.3, 0.55),
                off_elbow: v(-0.6, -0.6, 0.0),
            },
            Key {
                hips: v(0.0, -0.12, 0.1),
                pelvis: e3(0.15, -0.1, 0.0),
                trunk: e3(0.42, -0.25, -0.04),
                gaze: (0.05, 0.25),
                front: (0.34, 0.0, 0.0),
                back: (0.04, 0.3),
                hand: Reach::Dir(v(-0.2, -0.5, 1.0), 0.3),
                elbow: v(0.5, -1.0, 0.0),
                weapon: v(-0.1, -0.02, 1.0),
                off: v(-0.32, 0.98, 0.05),
                off_elbow: v(-0.4, -0.2, -1.0),
            },
            Key {
                hips: v(0.0, -0.15, 0.12),
                pelvis: e3(0.2, -0.2, 0.0),
                trunk: e3(0.6, -0.35, -0.06),
                gaze: (0.1, 0.35),
                front: (0.34, 0.0, 0.0),
                back: (0.05, 0.35),
                hand: Reach::Dir(v(-0.3, -0.9, 0.45), 0.35),
                elbow: v(0.8, 0.0, -0.4),
                weapon: v(-0.25, -0.6, 0.75),
                off: v(-0.34, 0.92, -0.02),
                off_elbow: v(-0.4, 0.0, -1.0),
            },
        ],
        // Swipe: coiled hard to the right with the club cocked back at shoulder height, uncoiling
        // through a flat blow across the front, wrapping round to the left.
        OrkBlow::Swipe => [
            Key {
                hips: v(0.02, -0.05, -0.06),
                pelvis: e3(0.04, 0.4, -0.03),
                trunk: e3(0.12, 0.45, 0.1),
                gaze: (-0.1, 0.05),
                front: (0.22, 0.0, 0.0),
                back: (0.0, 0.0),
                hand: Reach::To(v(0.62, 1.38, -0.32)),
                elbow: v(0.4, -0.7, 0.4),
                weapon: v(0.25, 0.55, -0.8),
                off: v(-0.15, 1.2, 0.5),
                off_elbow: v(-0.6, -0.6, 0.0),
            },
            Key {
                hips: v(-0.02, -0.1, 0.06),
                pelvis: e3(0.08, -0.15, 0.0),
                trunk: e3(0.28, -0.3, -0.05),
                gaze: (0.05, 0.15),
                front: (0.26, 0.0, 0.0),
                back: (0.04, 0.3),
                hand: Reach::To(v(0.2, 1.2, 0.72)),
                elbow: v(0.4, -0.85, -0.3),
                weapon: v(-0.8, 0.1, 0.6),
                off: v(-0.4, 1.02, -0.05),
                off_elbow: v(-0.3, -0.3, -1.0),
            },
            Key {
                hips: v(-0.04, -0.12, 0.06),
                pelvis: e3(0.1, -0.35, 0.0),
                trunk: e3(0.35, -0.45, -0.06),
                gaze: (0.15, 0.2),
                front: (0.26, 0.0, 0.0),
                back: (0.05, 0.35),
                hand: Reach::Dir(v(-0.9, -0.35, 0.45), 0.4),
                elbow: v(0.2, -1.0, -0.2),
                weapon: v(-0.88, -0.21, -0.42),
                off: v(-0.42, 0.98, -0.12),
                off_elbow: v(-0.3, -0.3, -1.0),
            },
        ],
        // Cast: the staff hauled up overhead, the off hand drawn back; then the orb levelled at the
        // target in a forward thrust with the off hand flung after it (the bolt leaves here).
        OrkBlow::Cast => [
            Key {
                hips: v(0.0, -0.02, -0.06),
                pelvis: e3(-0.02, 0.15, 0.0),
                trunk: e3(-0.15, 0.1, 0.05),
                gaze: (-0.05, -0.2),
                front: (0.22, 0.0, 0.0),
                back: (0.0, 0.0),
                hand: Reach::To(v(0.28, 1.86, 0.08)),
                elbow: v(0.6, 0.0, -0.6),
                weapon: v(0.0, 1.0, -0.15),
                off: v(-0.42, 1.22, -0.08),
                off_elbow: v(-0.5, -0.6, -0.4),
            },
            Key {
                hips: v(0.0, -0.1, 0.08),
                pelvis: e3(0.12, -0.05, 0.0),
                trunk: e3(0.35, -0.15, 0.0),
                gaze: (0.0, -0.1),
                front: (0.3, 0.0, 0.0),
                back: (0.03, 0.25),
                hand: Reach::Dir(v(-0.15, 0.05, 1.0), 0.25),
                elbow: v(0.4, -1.0, 0.0),
                weapon: v(-0.05, 0.42, 0.9),
                off: v(-0.14, 1.32, 0.62),
                off_elbow: v(-0.6, -0.6, -0.3),
            },
            Key {
                hips: v(0.0, -0.1, 0.07),
                pelvis: e3(0.12, -0.08, 0.0),
                trunk: e3(0.38, -0.18, 0.0),
                gaze: (0.05, -0.05),
                front: (0.3, 0.0, 0.0),
                back: (0.03, 0.25),
                hand: Reach::Dir(v(-0.1, -0.1, 1.0), 0.35),
                elbow: v(0.4, -1.0, 0.0),
                weapon: v(-0.05, 0.45, 0.89),
                off: v(-0.2, 1.22, 0.55),
                off_elbow: v(-0.6, -0.6, -0.3),
            },
        ],
    }
}

/// The solved keyframes of one build, per blow (indexed by `OrkBlow as usize`).
type Keys = [[Pose; 3]; 3];

/// Blow keyframes for `b`, solved on first use and cached per [`OrkBuild::id`] (they are static per
/// build — only the live idle / guard / gait poses depend on time).
fn keys(b: &OrkBuild) -> &'static Keys {
    static CACHE: [OnceLock<Keys>; BUILD_SLOTS] = [const { OnceLock::new() }; BUILD_SLOTS];
    CACHE[b.id.min(BUILD_SLOTS - 1)].get_or_init(|| {
        [OrkBlow::Smash, OrkBlow::Swipe, OrkBlow::Cast].map(|blow| blow_keys(blow).map(|k| key_pose(&k, b)))
    })
}

/// A blow at swing state `s` from `base`: 0..1 wind-up (eased) → cocked, 1..2 the downswing
/// (accelerating into the target), 2..3 follow-through.
fn blow_at(k: &[Pose; 3], s: f32, base: &Pose) -> Pose {
    if s <= 1.0 {
        base.lerp(&k[0], smoothstep(s))
    } else if s <= 2.0 {
        let u = s - 1.0;
        k[0].lerp(&k[1], u * u)
    } else {
        k[1].lerp(&k[2], (s - 2.0).min(1.0))
    }
}

/// The ork's pose this frame from its drive (gait / seated) and its [`OrkMotion`]: stance (at ease ⇄
/// on guard) or gait, then the blow — wound up off the countdown, landing on the stamp, followed
/// through and recovered back into whatever comes next (the next wind-up in a flurry).
pub(crate) fn ork_pose(d: &BipedDrive, m: &OrkMotion, now: f32) -> Pose {
    if d.sitting {
        return sit_pose();
    }
    let b = &m.build;
    let t = now + 2.5 * d.phase;
    let moving = d.moving_amt.clamp(0.0, 1.0);
    let e = m.engaged.clamp(0.0, 1.0);
    let still = (moving < 0.999).then(|| {
        if e <= 0.001 {
            idle(t, b)
        } else if e >= 0.999 {
            guard(t, b)
        } else {
            idle(t, b).lerp(&guard(t, b), e)
        }
    });
    let walk = |d: &BipedDrive| {
        let run = d.run_amt.clamp(0.0, 1.0);
        if mixamo_gait() {
            mx_gait(d.walk_phase, run, m.stride, e, b)
        } else {
            gait(d.walk_phase, run, m.stride, e, b)
        }
    };
    let base = match (still, moving > 0.001) {
        (Some(s), false) => s,
        (Some(s), true) => s.lerp(&walk(d), moving),
        (None, _) => walk(d),
    };

    let ks = keys(b);
    let pre = (m.wind > 0.001).then(|| blow_at(&ks[m.next as usize], m.wind, &base));
    // The blow that landed: its downswing (if it landed before it was played), follow-through, and
    // the recovery blending back into the stance / the next wind-up.
    let post = m.since.is_finite().then(|| {
        let late = (2.0 - m.from).max(0.0) * LATE_SWING;
        let s = if m.since < late { m.from + m.since / LATE_SWING } else { 2.0 + super::ease_out_cubic((m.since - late) / FOLLOW_T) };
        (s, smoothstep((m.since - late - 0.7 * FOLLOW_T) / RECOVER_T))
    });
    let act = match post {
        Some((s, r)) if r < 1.0 => {
            let landed = blow_at(&ks[m.swing as usize], s, &base);
            landed.lerp(pre.as_ref().unwrap_or(&base), r)
        }
        _ => match pre {
            Some(p) => p,
            None => return base,
        },
    };
    // Mid-stride (a charge with the club cocked) the legs keep running under the swing.
    if moving > 0.05 {
        action_over_loco(&act, &base, moving)
    } else {
        act
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAVY: OrkBuild = OrkBuild { id: 0, sdx: 0.25, hdx: 0.11, heavy: 1.0, staff: false, swipe: 0.3 };
    const SHAMAN: OrkBuild = OrkBuild { id: 3, sdx: 0.222, hdx: 0.1, heavy: 0.45, staff: true, swipe: 0.0 };

    /// Rig-frame (ankle_l, ankle_r, hand_r, weapon +Y) of a pose — the `spawn_biped` chain.
    fn fk(p: &Pose, b: &OrkBuild) -> (Vec3, Vec3, Vec3, Vec3) {
        let x = |jp: Jp, t: Vec3| Transform { translation: jp.t.unwrap_or(t), rotation: jp.r, ..default() }.to_matrix();
        let hips = x(p.hips, Vec3::new(0.0, 1.05, 0.0));
        let ankle = |hip: Jp, knee: Jp, foot: Jp, s: f32| {
            (hips * x(hip, Vec3::new(s * b.hdx, -0.05, 0.0)) * x(knee, Vec3::new(0.0, -0.4, 0.0)) * x(foot, Vec3::new(0.0, -0.45, 0.0))).transform_point3(Vec3::ZERO)
        };
        let hand = hips
            * x(p.torso, Vec3::new(0.0, 0.15, 0.0))
            * x(p.sh_r, Vec3::new(b.sdx, 0.27, 0.01))
            * x(p.el_r, Vec3::new(0.0, -0.3, 0.0))
            * Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0));
        let weapon = hand * x(p.sword, Vec3::ZERO);
        (ankle(p.hip_l, p.knee_l, p.foot_l, -1.0), ankle(p.hip_r, p.knee_r, p.foot_r, 1.0), hand.transform_point3(Vec3::ZERO), weapon.transform_vector3(Vec3::Y).normalize())
    }

    /// Standing poses keep both soles on the ground (the IK reaches) — no floating, no sinking.
    #[test]
    fn standing_feet_are_planted() {
        for b in [HEAVY, SHAMAN] {
            for p in [idle(0.0, &b), idle(7.3, &b), guard(1.0, &b)] {
                let (l, r, _, _) = fk(&p, &b);
                assert!((l.y - ANKLE_Y).abs() < 2e-3 && (r.y - ANKLE_Y).abs() < 2e-3, "{l} {r}");
            }
            for k in keys(&b).iter().flatten() {
                let (_, r, _, _) = fk(k, &b);
                assert!(r.y >= ANKLE_Y - 2e-3, "back foot sinks: {r}");
            }
        }
    }

    /// The carried club never runs through its own shoulder (the skinned body's deltoid + the
    /// trapezius beside the neck): at ease, through the walk and the charge, on guard. Measured
    /// from the club head (grip + 0.13..0.53 along the haft, r ≈ 0.09 with the spikes' roots) and
    /// the upper haft to the deltoid (r ≈ 0.08 round the upper arm's first 0.12) and the trapezius.
    #[test]
    fn club_clears_the_shoulder() {
        let b = HEAVY;
        let x = |jp: Jp, t: Vec3| Transform { translation: jp.t.unwrap_or(t), rotation: jp.r, ..default() }.to_matrix();
        let seg = |p: Vec3, a: Vec3, bb: Vec3| {
            let ab = bb - a;
            let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            p.distance(a + ab * t)
        };
        // Closest approach between two segments, sampled (plenty for a test).
        let segseg = |a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3| (0..=24).map(|i| seg(a0.lerp(a1, i as f32 / 24.0), b0, b1)).fold(f32::INFINITY, f32::min);
        let mut poses: Vec<(String, Pose)> = vec![("idle0".into(), idle(0.0, &b)), ("idle7".into(), idle(7.3, &b)), ("guard".into(), guard(1.0, &b))];
        for i in 0..8 {
            let c = i as f32 / 8.0 * TAU;
            poses.push((format!("walk{i}"), gait(c, 0.0, 0.5, 0.0, &b)));
            poses.push((format!("run{i}"), gait(c, 1.0, 0.9, 0.0, &b)));
        }
        for blow in [OrkBlow::Smash, OrkBlow::Swipe] {
            for (i, k) in keys(&b)[blow as usize].iter().enumerate() {
                poses.push((format!("{blow:?}{i}"), k.clone()));
            }
        }
        let mut worst = f32::INFINITY;
        for (name, p) in &poses {
            let hips = x(p.hips, Vec3::new(0.0, 1.05, 0.0));
            let chest = hips * x(p.torso, Vec3::new(0.0, 0.15, 0.0));
            let sh = chest * x(p.sh_r, Vec3::new(b.sdx, 0.27, 0.01));
            let hand = sh * x(p.el_r, Vec3::new(0.0, -0.3, 0.0)) * Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0));
            let club = hand * x(p.sword, Vec3::ZERO);
            let at = |d: f32| club.transform_point3(Vec3::Y * d);
            let delt = (sh.transform_point3(Vec3::new(0.0, -0.01, 0.0)), sh.transform_point3(Vec3::new(0.0, -0.12, 0.0)));
            let trap = (chest.transform_point3(Vec3::new(0.06, 0.3, -0.02)), chest.transform_point3(Vec3::new(b.sdx - 0.05, 0.3, -0.02)));
            let head = segseg(at(0.16), at(0.5), delt.0, delt.1) - 0.08 - 0.09;
            let haft = segseg(at(0.08), at(0.16), delt.0, delt.1) - 0.08 - 0.045;
            let trap_c = segseg(at(0.1), at(0.5), trap.0, trap.1) - 0.05 - 0.09;
            let m = head.min(haft).min(trap_c);
            eprintln!("{name}: head {head:.3} haft {haft:.3} trap {trap_c:.3}");
            worst = worst.min(m);
        }
        assert!(worst > -0.005, "club runs through the shoulder by {:.3}", -worst);
    }

    /// Every authored club / staff direction is actually reached by the grip solver.
    #[test]
    fn weapons_point_where_authored() {
        for (b, blows) in [(HEAVY, [OrkBlow::Smash, OrkBlow::Swipe]), (SHAMAN, [OrkBlow::Cast, OrkBlow::Cast])] {
            for blow in blows {
                for (k, p) in blow_keys(blow).iter().zip(keys(&b)[blow as usize].iter()) {
                    let (_, _, _, w) = fk(p, &b);
                    assert!(w.angle_between(k.weapon.normalize()) < 0.25, "{blow:?}: {w} vs {}", k.weapon);
                }
            }
        }
        // At ease the club hangs down beside the leg, head forward; on guard it stands up.
        let (_, _, _, w) = fk(&idle(0.0, &HEAVY), &HEAVY);
        assert!(w.y < -0.3 && w.z > 0.2 && w.x > 0.1, "{w}");
        let (_, _, _, w) = fk(&guard(0.0, &HEAVY), &HEAVY);
        assert!(w.y > 0.8, "{w}");
    }

    /// The timing contract: the downswing is complete (swing state 2) exactly when the countdown
    /// hits zero — the instant the brain stamps `atk_anim` and deals the damage.
    #[test]
    fn blow_lands_on_the_stamp() {
        assert_eq!(wind_at(WIND_T + 0.01), 0.0);
        assert!((wind_at(SWING_T) - 1.0).abs() < 1e-5);
        assert!((wind_at(0.0) - 2.0).abs() < 1e-5);
        let mut m = OrkMotion::new(HEAVY, 7);
        let mut d = BipedDrive::default();
        let dt = 1.0 / 60.0;
        let mut now = 10.0;
        let mut cd = 0.6;
        while cd > 0.0 {
            m.track(&mut d, 0.0, cd, Vec2::ZERO, 1.0, now, dt);
            now += dt;
            cd -= dt;
        }
        assert!(m.wind > 1.85, "wound up to {}", m.wind);
        // The stamp frame: the blow lands from (nearly) the full downswing — no late swing left.
        m.track(&mut d, now, ORK_CD, Vec2::ZERO, 1.0, now, dt);
        assert!(m.since == 0.0 && (2.0 - m.from) * LATE_SWING < 0.02);
        assert_eq!(m.wind, 0.0);
    }
    const ORK_CD: f32 = 1.1;

    /// Closing in with the club cocked (a charge): the arms hold the wind-up while the legs keep
    /// running under it.
    #[test]
    fn charge_keeps_striding() {
        let mut m = OrkMotion::new(HEAVY, 7);
        let mut d = BipedDrive { moving_amt: 1.0, ..default() };
        let dt = 1.0 / 24.0;
        let mut pos = Vec2::ZERO;
        let mut knees = Vec::new();
        for i in 0..200 {
            pos.y += 2.6 * dt;
            d.moving_amt = 1.0;
            m.track(&mut d, 0.0, SWING_T, pos, 1.0, i as f32 * dt, dt);
            if i >= 190 {
                let p = ork_pose(&d, &m, i as f32 * dt);
                knees.push((d.walk_phase, p.knee_l.r, p.hips.t));
            }
        }
        assert!(m.wind > 0.99, "cocked: {}", m.wind);
        for w in knees.windows(2) {
            assert!(w[0].1.angle_between(w[1].1) > 1e-3 || w[0].2 != w[1].2, "frozen legs: {:?}", knees);
        }
    }

    /// The gait is foot-locked: over a second of walking, the ground the planted foot sweeps back
    /// (gait cycles advanced × stride) equals the ground the body covered, and the stance sole
    /// stays on the ground.
    #[test]
    fn gait_stance_foot_tracks_the_ground() {
        let mut m = OrkMotion::new(HEAVY, 3);
        let mut d = BipedDrive { moving_amt: 1.0, ..default() };
        let (dt, speed, scale) = (1.0 / 60.0, 1.6, 0.92);
        let mut pos = Vec2::ZERO;
        for i in 0..120 {
            pos.y += speed * dt;
            m.track(&mut d, 0.0, f32::INFINITY, pos, scale, i as f32 * dt, dt);
        }
        let c0 = d.walk_phase;
        for i in 0..60 {
            pos.y += speed * dt;
            m.track(&mut d, 0.0, f32::INFINITY, pos, scale, 2.0 + i as f32 * dt, dt);
        }
        let swept = (d.walk_phase - c0) / TAU * m.stride;
        assert!((swept - speed / scale).abs() < 0.03, "{swept} vs {}", speed / scale);
        if mixamo_gait() {
            // The Mixamo walk plays its own stride, within the stretch it allows.
            let (duty, sweep, _) = mx_shared(0.0);
            let k = m.stride * duty / (sweep * LEG);
            assert!((0.5..1.5).contains(&k), "stride stretched x{k}");
            for u in [0.1, 0.25, 0.4] {
                let (a, _, _, _) = fk(&mx_gait(TAU * u * duty, 0.0, m.stride, 0.0, &HEAVY), &HEAVY);
                assert!((a.y - ANKLE_Y).abs() < 0.015, "mid-stance sole off the ground at {u}: {a}");
            }
        } else {
            assert!(m.stride * WALK_DUTY < 0.9, "stance sweep clamped: {}", m.stride * WALK_DUTY);
            // Mid-stance (left leg) the sole is on the ground.
            let (a, _, _, _) = fk(&gait(TAU * 0.4 * WALK_DUTY, 0.0, m.stride, 0.0, &HEAVY), &HEAVY);
            assert!((a.y - ANKLE_Y).abs() < 0.01, "{a}");
        }
    }
}
