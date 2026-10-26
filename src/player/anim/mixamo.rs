//! The hero's walk, run and dodge roll on Mixamo clips (`mixamo_clips`, baked by
//! `tools/mixamo_retarget/bake.py` from Mixamo's Walking / Dive Roll and "Running" cut on the
//! knight's own auto-rig).
//!
//! Same recipe as the sword combo ([`super::swordplay`]): the clips are rig-free targets (pelvis and
//! ankle positions in leg lengths, world-delta rotations, the limbs as directions), solved onto the
//! footman with two-bone IK, so his own proportions and armour sit on the motion. What is new here:
//!
//! - **run** is cut on the knight's OWN Mixamo auto-rig (the T-pose `footman_obj` export), so it is
//!   not solved at all: [`direct`] puts each Mixamo bone's turn straight on the matching joint —
//!   exactly the motion the Mixamo page shows on him.
//! - **walk / run** are in-place loops played at the speed the ground goes by: the planted boot sweeps
//!   back at `native speed × playback rate`, so choosing the rate = ground speed / native speed pins
//!   it to the ground (no skating). Both loops start on the left heel strike, so one phase drives
//!   either, and the two blend by speed.
//! - **jump** plays the clip's airborne stretch against the real vertical speed (physics owns the
//!   height; the clip only folds the limbs).
//! - **roll** plays the dive-roll's body shape (pelvis frame) through the somersault the root performs.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::mixamo_clips as mc;
use super::swordplay::{aim_leg, Frame};
use super::{aim_arm, e3, rest, rx, shield_gait_r, smoothstep, sword_rest_r, Jp, Pose, SHIELD_GAIT_T};

/// Model m/s: the walk (below `WALK_TO`) fades into the run by `RUN_FROM` (the knight's sprint is
/// ~6.5, so the fade only shows while he speeds up / slows down).
const WALK_TO: f32 = 2.3;
const RUN_FROM: f32 = 4.0;

/// 0 = the walk ... 1 = the run, at a ground speed of `v` model m/s.
fn sprint_weight(v: f32) -> f32 {
    smoothstep((v - WALK_TO) / (RUN_FROM - WALK_TO))
}

/// Mixamo clips own the run too unless `FOREST_RUN=curves` (then the walk fades into the original
/// hand-built run curves above a jog, as before).
pub(super) fn mixamo_run() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_RUN").map(|v| v != "curves").unwrap_or(true))
}

/// How much of the pose at `v` model m/s is the Mixamo run, which keeps its own height (its
/// flight), so `hero_anim`'s per-frame grounding stands aside by this much.
pub(super) fn owns_height(v: f32) -> f32 {
    if mixamo_run() { sprint_weight(v) } else { 0.0 }
}

/// Walk -> curve-run weight, for the `FOREST_RUN=curves` path.
pub(super) fn run_weight(v: f32) -> f32 {
    if mixamo_run() {
        return 0.0;
    }
    smoothstep((v - WALK_TO) / (3.4 - WALK_TO))
}

fn leg_len() -> f32 {
    let rig = super::super::footman::leg_rig();
    rig.knee.length() + rig.foot.length()
}

/// One locomotion loop and how it travels: the foot-lock numbers of a baked clip.
struct Loop {
    clip: &'static [[f32; 53]],
    /// Stance sweep speed (leg lengths / s at native playback) and the stride stretch on it.
    native: f32,
    stride: f32,
    /// Fraction of the cycle a boot is planted (left: from phase 0; right: from 0.5).
    duty: f32,
}

impl Loop {
    /// Leg lengths the planted boot sweeps back through in one stance.
    fn sweep(&self) -> f32 {
        self.native * self.stride * self.duty * self.clip.len() as f32 / mc::FPS
    }
}

/// The loops in play at `v` model m/s with their weights (walk, run), summing to 1.
fn mix(v: f32) -> [(Loop, f32); 2] {
    let wr = if mixamo_run() { sprint_weight(v) } else { 0.0 };
    [
        (Loop { clip: &mc::WALK, native: mc::WALK_SPEED, stride: 1.0, duty: mc::WALK_DUTY }, 1.0 - wr),
        (Loop { clip: &mc::RUN, native: mc::RUN_SPEED, stride: 1.0, duty: mc::RUN_DUTY }, wr),
    ]
}

/// The blend's shared stance: (duty, sweep in leg lengths). Cross-fading loops whose boots are planted
/// for different shares of the cycle would skate the boot wherever one is down and the other up, so
/// every loop is time-warped onto the weighted duty and stretched onto the weighted sweep: all of
/// them then plant, sweep and lift together, and one phase rate pins the boot for all.
fn shared(m: &[(Loop, f32); 2]) -> (f32, f32) {
    m.iter().fold((0.0, 0.0), |(d, s), (l, w)| (d + l.duty * w, s + l.sweep() * w))
}

/// Gait-phase advance (rad/s) at `v` model m/s: the foot-lock contract — the shared stance (`duty` of
/// the cycle) must carry the planted boot back through the shared sweep at exactly the body's speed.
pub(super) fn phase_rate(v: f32) -> f32 {
    let (duty, sweep) = shared(&mix(v));
    TAU * v * duty / (sweep * leg_len())
}

/// Cycle phase `u` of a loop planted for `duty` (per half-cycle: stance, then swing) warped from the
/// shared `common` duty.
pub(super) fn warp(u: f32, common: f32, duty: f32) -> f32 {
    let u = u.rem_euclid(1.0);
    let (base, s) = if u < 0.5 { (0.0, u) } else { (0.5, u - 0.5) };
    let s = if s < common { s * duty / common } else { duty + (s - common) * (0.5 - duty) / (0.5 - common).max(1e-4) };
    base + s
}

/// A looping clip at cycle phase `u` (0..1), Catmull-Rom between the baked keys.
pub(super) fn sample_loop(clip: &[[f32; 53]], u: f32) -> Frame {
    let n = clip.len() as isize;
    let x = u.rem_euclid(1.0) * n as f32;
    let i = x.floor() as isize;
    let k = |j: isize| Frame::of(&clip[j.rem_euclid(n) as usize]);
    Frame::cubic(&k(i - 1), &k(i), &k(i + 1), &k(i + 2), x - i as f32)
}

/// A one-shot clip at frame position `x` (clamped to its ends).
fn sample_once(clip: &[[f32; 53]], x: f32) -> Frame {
    let n = clip.len() as isize;
    let x = x.clamp(0.0, (n - 1) as f32);
    let i = (x.floor() as isize).min(n - 2);
    let k = |j: isize| Frame::of(&clip[j.clamp(0, n - 1) as usize]);
    Frame::cubic(&k(i - 1), &k(i), &k(i + 1), &k(i + 2), x - i as f32)
}

/// One clip frame solved onto the footman: legs by two-bone IK onto the ankle targets (the boots go
/// where the clip puts them), trunk and head by the clip's own rotations, both arms by [`aim_arm`].
/// The sword and shield joints are left at their carries for the caller to dress.
fn solve(f: &Frame, ground: bool) -> Pose {
    let rig = super::super::footman::leg_rig();
    let tp = Frame::of(&mc::TPOSE);
    let leg_len = leg_len();
    let hip_rest = super::super::model::HIP_REST_Y;
    let mut p = rest();

    let pelvis = f.pelvis_r;
    let d = (f.pelvis - tp.pelvis) * leg_len;
    let mut hips = Vec3::new(0.0, hip_rest, 0.0) + d;

    let mirror = |v: Vec3, right: bool| if right { Vec3::new(-v.x, v.y, v.z) } else { v };
    let ankle_rest = |right: bool| Vec3::new(0.0, hip_rest, 0.0) + mirror(rig.hip + rig.knee + rig.foot, right);
    // On the ground a boot is never let through it: the clip's foot is shorter than the knight's
    // boot, so at toe-off the toe would sink; the ankle rides up by what the boot would dip.
    let clear = |i: usize| {
        let low = [Vec3::new(0.0, -rig.ankle_height, -rig.heel), Vec3::new(0.0, -rig.ankle_height, rig.toe)]
            .into_iter()
            .map(|p| (f.foot[i] * p).y)
            .fold(f32::MAX, f32::min);
        -low
    };
    let target = |i: usize| {
        let mut t = ankle_rest(i == 1) + (f.ankle[i] - tp.ankle[i]) * leg_len;
        if ground {
            t.y = t.y.max(clear(i));
        }
        t
    };
    // Never stretch a leg past its reach: the hips settle onto the straighter one.
    let reach = 0.995 * leg_len;
    for i in 0..2 {
        let h = pelvis * mirror(rig.hip, i == 1);
        let a = target(i);
        let (dx, dz) = (a.x - hips.x - h.x, a.z - hips.z - h.z);
        let cap = a.y + (reach * reach - dx * dx - dz * dz).max(0.0).sqrt() - h.y;
        let k = 0.02;
        let s = (0.5 + 0.5 * (cap - hips.y) / k).clamp(0.0, 1.0);
        hips.y = cap + (hips.y - cap) * s - k * s * (1.0 - s);
    }
    let leg = |i: usize| {
        let right = i == 1;
        let (hb, kb, fb) = (mirror(rig.hip, right), mirror(rig.knee, right), mirror(rig.foot, right));
        let (l1, l2) = (kb.length(), fb.length());
        let h = hips + pelvis * hb;
        let to = target(i) - h;
        let dist = to.length().clamp(1e-3, 0.999 * (l1 + l2));
        let dh = to.normalize_or(Vec3::NEG_Y);
        let ang = ((l1 * l1 + dist * dist - l2 * l2) / (2.0 * l1 * dist)).clamp(-1.0, 1.0).acos();
        let pole = (f.pole[i] - dh * f.pole[i].dot(dh)).normalize_or(Vec3::Z);
        let knee = h + l1 * (ang.cos() * dh + ang.sin() * pole);
        let ankle = h + dh * dist;
        let inv = pelvis.inverse();
        let (hq, kq) = aim_leg(inv * (knee - h), inv * (ankle - knee), (kb, fb));
        let fq = (pelvis * hq * kq).inverse() * f.foot[i];
        (Jp::r(hq), Jp::r(kq), Jp::r(fq))
    };
    (p.hip_l, p.knee_l, p.foot_l) = leg(0);
    (p.hip_r, p.knee_r, p.foot_r) = leg(1);
    p.hips = Jp { t: Some(hips + Vec3::Y * (1.05 - hip_rest)), r: pelvis };

    p.torso = Jp::r(pelvis.inverse() * f.chest);
    p.head = Jp::r(f.chest.inverse() * f.head);

    let ab = super::super::footman::arm_bind();
    let inv = f.chest.inverse();
    let arm = |upper: Vec3, fore: Vec3, bind: (Vec3, Vec3)| aim_arm(inv * upper, inv * fore, upper.angle_between(fore), bind);
    let (sr, er) = arm(f.r_upper, f.r_fore, ab[0]);
    p.sh_r = Jp::r(sr);
    p.el_r = Jp::r(er);
    let (sl, el) = arm(f.l_upper, f.l_fore, ab[1]);
    p.sh_l = Jp::r(sl);
    p.el_l = Jp::r(el);
    p.sword = Jp::r(sword_rest_r());
    p.shield = Jp { t: Some(SHIELD_GAIT_T), r: shield_gait_r() };
    p
}

/// Dress a solved clip pose with the knight's kit: the sword carried (hanging on a walk, shouldered
/// on a run) in the sword hand, the shield held edge-on on the shield arm — which swings only part of
/// the clip's way (a board on the forearm is carried, not pumped) unless it is slung (`shield_back`).
/// `run` 0..1 is the walk → run weight, `armed` whether the blade is in hand.
fn dress(mut p: Pose, run: f32, armed: f32, shield_back: f32) -> Pose {
    let carry_sh = e3(0.04, 0.0, 0.0).slerp(e3(0.08, 0.22, -0.07), run);
    let carry_el = rx(-0.26).slerp(rx(-0.6), run);
    let follow = 0.4 + 0.6 * shield_back;
    p.sh_l = Jp::r(carry_sh.slerp(p.sh_l.r, follow));
    p.el_l = Jp::r(carry_el.slerp(p.el_l.r, follow));
    p.sword = Jp::r(sword_rest_r().slerp(super::posed_sword(-2.046, -0.52, 0.119).r, run * armed));
    // The board stays upright, edge-on along the body: cancel the elbow's pitch on it.
    let (el_x, _, _) = p.el_l.r.to_euler(EulerRot::XYZ);
    let undo = rx(-el_x * (0.4 + 0.5 * run));
    p.shield = Jp { t: Some(undo * SHIELD_GAIT_T), r: undo * shield_gait_r() };
    p
}

/// The run's bone turns ([`mc::RUN_BONES`]) at loop phase `u`, slerped between frames.
fn run_bones(u: f32) -> [Quat; 13] {
    let n = mc::RUN_BONES.len();
    let x = u.rem_euclid(1.0) * n as f32;
    let (i, s) = (x.floor() as usize % n, x.fract());
    let q = |r: &[f32; 52], b: usize| Quat::from_xyzw(r[4 * b], r[4 * b + 1], r[4 * b + 2], r[4 * b + 3]).normalize();
    std::array::from_fn(|b| q(&mc::RUN_BONES[i], b).slerp(q(&mc::RUN_BONES[(i + 1) % n], b), s))
}

/// A shoulder's world turn in the T-pose the knight was exported (and auto-rigged) in: the arm
/// swung from hanging to straight out sideways (`footman::export::footman_obj`).
fn tpose_shoulder(right: bool) -> Quat {
    let (el, hand) = super::super::footman::arm_bind()[if right { 0 } else { 1 }];
    let out = if right { Vec3::X } else { Vec3::NEG_X };
    Quat::from_rotation_arc((el + hand).normalize_or(Vec3::NEG_Y), out)
}

/// A pose straight off the Mixamo bones: every joint takes its bone's world turn from the bind
/// pose (the knight's own T-pose, so bone and joint start aligned), turned into the joint's local
/// rotation through the rig's hierarchy; the hips ride the clip's pelvis travel. No IK, no
/// retiming — the clip as Mixamo plays it on him.
fn direct(f: &Frame, b: &[Quat; 13]) -> Pose {
    let mut p = direct_raw(f, b);
    if let Some(t) = &mut p.hips.t {
        t.y += run_floor();
    }
    p
}

/// The run's hips lift (model m) that puts its lowest boot point over the whole loop on the
/// ground: the clip's own height, flight and all, set once against the knight's real legs (the
/// Mixamo hips bone needn't sit where the footman's does).
fn run_floor() -> f32 {
    static FLOOR: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *FLOOR.get_or_init(|| {
        let n = 96;
        let low = (0..n)
            .map(|i| {
                let u = i as f32 / n as f32;
                let p = direct_raw(&sample_loop(&mc::RUN, u), &run_bones(u));
                let [a, b] = super::LegChain::of_pose(&p).boot_lows();
                a.min(b)
            })
            .fold(f32::MAX, f32::min);
        -low
    })
}

fn direct_raw(f: &Frame, b: &[Quat; 13]) -> Pose {
    let [hips, chest, head, th_l, sn_l, ft_l, th_r, sn_r, ft_r, ua_l, fa_l, ua_r, fa_r] = *b;
    let tp = Frame::of(&mc::TPOSE);
    let hip_rest = super::super::model::HIP_REST_Y;
    let mut p = rest();
    let t = Vec3::new(0.0, hip_rest, 0.0) + (f.pelvis - tp.pelvis) * leg_len();
    p.hips = Jp { t: Some(t + Vec3::Y * (1.05 - hip_rest)), r: hips };
    p.torso = Jp::r(hips.inverse() * chest);
    p.head = Jp::r(chest.inverse() * head);
    p.hip_l = Jp::r(hips.inverse() * th_l);
    p.knee_l = Jp::r(th_l.inverse() * sn_l);
    p.foot_l = Jp::r(sn_l.inverse() * ft_l);
    p.hip_r = Jp::r(hips.inverse() * th_r);
    p.knee_r = Jp::r(th_r.inverse() * sn_r);
    p.foot_r = Jp::r(sn_r.inverse() * ft_r);
    let (tl, tr) = (tpose_shoulder(false), tpose_shoulder(true));
    let (sl, sr) = (ua_l * tl, ua_r * tr);
    p.sh_l = Jp::r(chest.inverse() * sl);
    p.el_l = Jp::r(sl.inverse() * fa_l * tl);
    p.sh_r = Jp::r(chest.inverse() * sr);
    p.el_r = Jp::r(sr.inverse() * fa_r * tr);
    p.sword = Jp::r(sword_rest_r());
    p.shield = Jp { t: Some(SHIELD_GAIT_T), r: shield_gait_r() };
    p
}

/// The Mixamo walk / run at gait phase `c` (rad; the left heel strikes at 0 and the right at
/// pi) and ground speed `v` (model m/s), dressed with the knight's kit. With `FOREST_RUN=curves` only
/// the walk (the caller fades it into the run curves with [`run_weight`]).
pub(super) fn gait(c: f32, v: f32, armed: f32, shield_back: f32) -> Pose {
    let u = c / TAU;
    let m = mix(v);
    let (duty, sweep) = shared(&m);
    let [(walk, ww), (run, wr)] = &m;
    let at = |l: &Loop| warp(u, duty, l.duty);
    let frame = |l: &Loop| stretch(sample_loop(l.clip, at(l)), l.stride * sweep / l.sweep());
    // The walk is solved onto the rig; the run is the knight's own clip, taken bone for bone.
    let p = if *wr < 1e-3 {
        solve(&frame(walk), true)
    } else {
        let r = direct(&frame(run), &run_bones(at(run)));
        if *ww < 1e-3 {
            r
        } else {
            // Blending two differently built poses joint by joint can dip a boot: lift it back out.
            let mut p = solve(&frame(walk), true).lerp(&r, *wr);
            let [a, b] = super::LegChain::of_pose(&p).boot_lows();
            if let Some(t) = &mut p.hips.t {
                t.y += (-a.min(b)).max(0.0);
            }
            p
        }
    };
    dress(p, *wr, armed, shield_back)
}

/// A frame with its forward travel (pelvis + ankles, off the T-pose) lengthened by `k`.
pub(super) fn stretch(mut f: Frame, k: f32) -> Frame {
    let tp = Frame::of(&mc::TPOSE);
    f.pelvis.z = tp.pelvis.z + (f.pelvis.z - tp.pelvis.z) * k;
    for i in 0..2 {
        f.ankle[i].z = tp.ankle[i].z + (f.ankle[i].z - tp.ankle[i].z) * k;
    }
    f
}

/// The somersault's limbs at progress `u` (0..1) through the roll: the dive, the tuck, the landing.
/// The root owns the actual tumble and the pelvis turn, so the hips only sit low over its pivot.
pub(super) fn roll(u: f32) -> Pose {
    let f = sample_once(&mc::ROLL, u.clamp(0.0, 1.0) * (mc::ROLL.len() - 1) as f32);
    let mut p = dress(solve(&f, false), 0.5, 1.0, 0.0);
    p.hips = Jp { t: Some(Vec3::new(0.0, 0.74, 0.0)), r: Quat::IDENTITY };
    p
}

/// Astride the horse: the seat posed in Blender on Henry S's horse (feet in the stirrups, reins in
/// hand), baked in the pelvis frame. The hips sit at the clips' authored 1.05 (the footman's
/// `HIP_REST_Y` once `hero_anim` shifts them) — `crate::horse` puts that point on the saddle.
pub(super) fn ride() -> Pose {
    let f = sample_once(&mc::RIDE, 0.0);
    let mut p = dress(solve(&f, false), 0.0, 0.0, 1.0);
    p.hips = Jp { t: Some(Vec3::new(0.0, 1.05, 0.0)), r: Quat::IDENTITY };
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Shift sprint plays the run clip at exactly its own rate, as on the Mixamo page (re-tune
    /// `SPRINT_MULT` when the run clip is swapped).
    #[test]
    fn sprint_plays_the_run_at_native_rate() {
        let v = crate::player::SPEED * crate::player::SPRINT_MULT / super::super::hero_model_scale();
        let native = TAU * mc::FPS / mc::RUN.len() as f32;
        let k = phase_rate(v) / native;
        assert!((k - 1.0).abs() < 0.01, "sprint plays the run at {k}x");
    }
}
