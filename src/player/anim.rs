//! Hero limb animation, based on the user's three.js `updateKnightAnimation`
//! (low-poly-knight-studio): **idle / walk / run / jump /
//! defend / attack1 (overhead chop) / attack2 (horizontal slash) / attack3 (forward thrust) /
//! victory**. The studio builds each frame imperatively — reset every joint to rest, then a `switch`
//! case sets some — so we mirror that: [`rest`] seeds a full [`Pose`] table and each clip function
//! mutates the fields it touches. The per-frame system then writes the chosen pose onto the rig
//! joints.
//!
//! Game adaptations:
//! - **walk/run** read `walk_phase` for their `cycle` (gait locked to real movement speed, not
//!   wall-clock) and are cross-faded by `moving_amt` (idle→gait) and `run_amt` (walk→run).
//!   The hero's imported footman uses its measured leg lengths and grounded foot targets;
//!   other bipeds retain the shared studio clips.
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
        Jp { t: None, r: r.normalize() }
    }
    fn lerp(self, o: Jp, s: f32) -> Jp {
        let t = match (self.t, o.t) {
            (Some(a), Some(b)) => Some(a.lerp(b, s)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        Jp { t, r: self.r.slerp(o.r, s).normalize() }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum MotionClip {
    Locomotion,
    Airborne,
    Attack(u8),
    Charge,
    Roll,
    Dash,
    Victory,
}

#[derive(Clone, Copy)]
struct ReleasedContact {
    cycle: i32,
    anchor: Vec2,
    phase: f32,
    height: f32,
    rotation: Quat,
}

/// Per-hero transient state, reset with Hero::fresh on a new run. Capture the
/// actually displayed pose on a clip change, including changes during another fade.
#[derive(Default)]
pub(super) struct AnimationState {
    clip: Option<MotionClip>,
    displayed: Option<Pose>,
    from: Option<Pose>,
    elapsed: f32,
    was_air: bool,
    landing_elapsed: Option<f32>,
    leap_right: bool,
    block_weight: f32,
    contacts: [Option<(i32, Vec2)>; 2],
    released_contacts: [Option<ReleasedContact>; 2],
}

impl AnimationState {
    fn contact(&mut self, grounded: bool, dt: f32, phase: f32, run: f32) -> f32 {
        if !grounded && !self.was_air {
            // Keep the leg already reaching forward as the leap's lead leg.
            self.leap_right = foot_target(phase + PI, run).0 > foot_target(phase, run).0;
        }
        if grounded && self.was_air {
            self.landing_elapsed = Some(0.0);
        } else if let Some(elapsed) = &mut self.landing_elapsed {
            *elapsed += dt;
        }
        self.was_air = !grounded;
        if !grounded { return 0.0; }
        let Some(elapsed) = self.landing_elapsed else { return 0.0; };
        if elapsed >= LAND_RECOVER {
            self.landing_elapsed = None;
            return 0.0;
        }
        // Absorb the impact over a few frames, rather than snapping straight
        // into the deepest crouch on the first grounded frame.
        smoothstep(elapsed / 0.055) * (1.0 - elapsed / LAND_RECOVER).powi(2)
    }

    fn blend(&mut self, target: Pose, clip: MotionClip, dt: f32) -> Pose {
        if self.clip != Some(clip) {
            self.from = self.displayed;
            self.elapsed = 0.0;
            self.clip = Some(clip);
        } else {
            self.elapsed += dt;
        }
        let duration = match clip {
            MotionClip::Airborne => 0.10,
            MotionClip::Locomotion => 0.14,
            // Keep attacks responsive and preserve their authored damage frame.
            MotionClip::Attack(_) => 0.055,
            MotionClip::Dash => 0.025,
            MotionClip::Victory => 0.20,
            _ => 0.09,
        };
        let pose = if let Some(from) = self.from {
            from.lerp(&target, smoothstep(self.elapsed / duration))
        } else {
            target
        };
        if self.elapsed >= duration { self.from = None; }
        self.displayed = Some(pose);
        pose
    }

    fn plant_contacts(&mut self, p: &mut Pose, pos: Vec2, facing: f32, phase: f32, run: f32) {
        let scale = super::footman::leg_rig().world_scale;
        let heading = Quat::from_rotation_y(facing);
        let mut targets = [foot_transform(p, false), foot_transform(p, true)];
        for (i, right) in [false, true].into_iter().enumerate() {
            let phase = phase + if right { PI } else { 0.0 };
            let cycle = (phase / TAU).floor() as i32;
            if let Some(released) = self.released_contacts[i] {
                if released.cycle == cycle {
                    self.contacts[i] = None;
                    let offset = released.anchor - pos;
                    let mut from = heading.inverse() * Vec3::new(offset.x, 0.0, offset.y) / scale;
                    from.y = released.height;
                    let blend = smoothstep((phase - released.phase).abs() * 3.0);
                    targets[i].0 = from.lerp(targets[i].0, blend)
                        + Vec3::Y * (0.10 * (PI * blend).sin());
                    targets[i].1 = (heading.inverse() * released.rotation).slerp(targets[i].1, blend);
                    continue;
                }
            }
            if !foot_in_contact(phase, run) {
                self.contacts[i] = None;
                continue;
            }
            let (sole, rotation) = foot_transform(p, right);
            let world = heading * (sole * scale);
            let anchor = match self.contacts[i] {
                Some((last_cycle, anchor)) if last_cycle == cycle => anchor,
                _ => pos + Vec2::new(world.x, world.z),
            };
            let offset = anchor - pos;
            let mut target = heading.inverse() * Vec3::new(offset.x, 0.0, offset.y) / scale;
            target.y = sole.y;
            let rig = super::footman::leg_rig();
            let hip = p.hips.t.unwrap() + p.hips.r * leg_vectors(right).0;
            let horizontal = Vec2::new(target.x - hip.x, target.z - hip.z).length();
            let reach = rig.knee.length() + rig.foot.length() - 0.012;
            // A hard turn needs a fresh step. Keeping an unreachable anchor
            // folded the body down to the ankles while IK still slid the boot.
            if horizontal > reach * 0.72 {
                self.contacts[i] = None;
                self.released_contacts[i] = Some(ReleasedContact {
                    cycle, anchor, phase, height: target.y, rotation: heading * rotation,
                });
                targets[i] = (target, rotation);
                continue;
            }
            self.contacts[i] = Some((cycle, anchor));
            targets[i] = (target, rotation);
        }
        lower_pelvis_to_reach(p, &targets);
        for (right, (target, rotation)) in [false, true].into_iter().zip(targets) {
            plant_leg(p, right, target, rotation);
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

/// One foot's contact fraction and half-stroke, in imported joint units. The same
/// measurements drive BOTH the foot trajectory and the distance clock; changing a pose
/// must not silently change the amount of ground covered by a step.
fn gait_dimensions(run: f32) -> (f32, f32) {
    let run = run.clamp(0.0, 1.0);
    (lerp(0.58, 0.30, run), lerp(0.40, 0.44, run))
}

fn foot_in_contact(phase: f32, run: f32) -> bool {
    phase.rem_euclid(TAU) / TAU <= gait_dimensions(run).0
}

fn contact_phase(phase: f32, back: f32) -> Option<f32> {
    // Reversing blends two different contact schedules. Neither foot can be
    // treated as fully planted until the forward/backward gait has settled.
    if back <= 0.01 { Some(phase) }
    else if back >= 0.99 { Some(-phase) }
    else { None }
}

pub(crate) fn gait_phase_delta(distance: f32, run: f32) -> f32 {
    let (contact, stride) = gait_dimensions(run);
    distance.max(0.0) * TAU * contact / (2.0 * stride * super::footman::leg_rig().world_scale)
}

/// Phase zero is the left touchdown; PI is the right touchdown. During contact
/// the sole retracts linearly, exactly cancelling the root's forward displacement.
/// The recovery curve matches that velocity at both ends, with a lifted, rolling boot.
fn foot_target(phase: f32, run: f32) -> (f32, f32, f32) {
    let (contact, stride) = gait_dimensions(run);
    let u = phase.rem_euclid(TAU) / TAU;
    let offset = -0.10 * run;
    let heel = lerp(-0.16, -0.08, run);
    let toe = lerp(0.25, 0.38, run);
    if u <= contact {
        let support = u / contact;
        let pitch = heel * (1.0 - smoothstep(support / 0.22))
            + toe * smoothstep((support - 0.65) / 0.35);
        return (offset + stride * (1.0 - 2.0 * support), boot_clearance(pitch), pitch);
    }
    let s = (u - contact) / (1.0 - contact);
    let tangent = -2.0 * stride * (1.0 - contact) / contact;
    let z = (2.0 * s.powi(3) - 3.0 * s * s + 1.0) * -stride
        + (-2.0 * s.powi(3) + 3.0 * s * s) * stride
        + (2.0 * s.powi(3) - 3.0 * s * s + s) * tangent;
    // A runner folds the trailing leg early, clearing it before the body rises
    // into flight. Slow symmetric recovery left the leg overextended behind him.
    let recovery = s + lerp(0.12, 0.27, run) * (PI * s).sin();
    let pitch = lerp(toe, heel, smoothstep(s));
    let lift = lerp(boot_clearance(toe), boot_clearance(heel), smoothstep(s))
        + lerp(0.085, 0.28, run) * (PI * recovery).sin().powi(2);
    (z + offset, lift, pitch)
}

/// Roll from heel to toe while keeping the lowest edge of the imported boot
/// on the floor. The ankle rises naturally during push-off.
fn boot_clearance(pitch: f32) -> f32 {
    let rig = super::footman::leg_rig();
    if pitch >= 0.0 { rig.sole_front * pitch.sin() }
    else { rig.sole_back * -pitch.sin() }
}

fn leg_vectors(right: bool) -> (Vec3, Vec3, Vec3) {
    let rig = super::footman::leg_rig();
    let mirror = |v: Vec3| if right { Vec3::new(-v.x, v.y, v.z) } else { v };
    (mirror(rig.hip), mirror(rig.knee), mirror(rig.foot))
}

/// Two-bone IK with the knee pole facing forward. Solve in pelvis space so weight
/// transfer, pelvis rotation and landing compression cannot move a planted sole.
fn plant_leg(p: &mut Pose, right: bool, sole: Vec3, sole_rotation: Quat) {
    let rig = super::footman::leg_rig();
    let (hip, upper_rest, lower_rest) = leg_vectors(right);
    let pelvis = p.hips.t.unwrap() + Vec3::Y * (super::model::HIP_REST_Y - 1.05);
    let ankle = sole + sole_rotation * (Vec3::Y * rig.ankle_height);
    let target = p.hips.r.inverse() * (ankle - pelvis) - hip;
    let upper = upper_rest.length();
    let lower = lower_rest.length();
    let d = target.length().clamp((upper - lower).abs() + 0.001, upper + lower - 0.001);
    let along = target.normalize_or_zero();
    let pole = p.hips.r.inverse() * Vec3::Z;
    let bend_dir = (pole - along * pole.dot(along)).normalize_or_zero();
    let cos = ((upper * upper + d * d - lower * lower) / (2.0 * upper * d)).clamp(-1.0, 1.0);
    let knee = upper * (along * cos + bend_dir * (1.0 - cos * cos).max(0.0).sqrt());
    let thigh_r = Quat::from_rotation_arc(upper_rest.normalize(), knee.normalize());
    let shin_r = Quat::from_rotation_arc(lower_rest.normalize(), (along * d - knee).normalize());
    let joints = (
        Jp::r(thigh_r),
        Jp::r(thigh_r.inverse() * shin_r),
        Jp::r((p.hips.r * shin_r).inverse() * sole_rotation),
    );
    if right { (p.hip_r, p.knee_r, p.foot_r) = joints; }
    else { (p.hip_l, p.knee_l, p.foot_l) = joints; }
}

fn foot_transform(p: &Pose, right: bool) -> (Vec3, Quat) {
    let rig = super::footman::leg_rig();
    let (hip_offset, knee_offset, foot_offset) = leg_vectors(right);
    let (hip, knee, foot) = if right { (p.hip_r, p.knee_r, p.foot_r) }
        else { (p.hip_l, p.knee_l, p.foot_l) };
    let thigh_r = p.hips.r * hip.r;
    let shin_r = thigh_r * knee.r;
    let foot_r = shin_r * foot.r;
    let hips = p.hips.t.unwrap() + Vec3::Y * (super::model::HIP_REST_Y - 1.05);
    let ankle = hips + p.hips.r * hip_offset + thigh_r * knee_offset + shin_r * foot_offset;
    (ankle - foot_r * (Vec3::Y * rig.ankle_height), foot_r)
}

/// A change of stride or heading can move a planted target outside the current
/// pelvis height's reach. Absorb it with the body rather than stretching the leg
/// and letting IK silently slide the foot toward the hip.
fn lower_pelvis_to_reach(p: &mut Pose, targets: &[(Vec3, Quat); 2]) {
    let rig = super::footman::leg_rig();
    let reach = rig.knee.length() + rig.foot.length() - 0.012;
    let mut hips = p.hips.t.unwrap();
    let height_offset = super::model::HIP_REST_Y - 1.05;
    for (right, (sole, rotation)) in [false, true].into_iter().zip(targets) {
        let hip_offset = p.hips.r * leg_vectors(right).0;
        let ankle = *sole + *rotation * (Vec3::Y * rig.ankle_height);
        let dx = ankle.x - hips.x - hip_offset.x;
        let dz = ankle.z - hips.z - hip_offset.z;
        let drop = (reach * reach - dx * dx - dz * dz).max(0.0).sqrt();
        hips.y = hips.y.min(ankle.y - hip_offset.y + drop - height_offset);
    }
    p.hips.t = Some(hips);
}

fn landing_pose(p: &mut Pose, amount: f32) {
    let feet = [foot_transform(p, false), foot_transform(p, true)];
    p.hips.t = p.hips.t.map(|t| t - Vec3::Y * (0.075 * amount));
    p.torso.r *= rx(0.12 * amount);
    p.sh_l.r *= rx(0.10 * amount);
    p.sh_r.r *= rx(0.10 * amount);
    for (right, (mut sole, rotation)) in [false, true].into_iter().zip(feet) {
        // A blended descending pose can put a boot below the grounded root.
        // Clamp contact, then solve the crouch instead of adding knee rotations.
        sole.y = sole.y.max(0.0);
        let rotation = if sole.y < 0.025 { Quat::IDENTITY } else { rotation };
        plant_leg(p, right, sole, rotation);
    }
}

fn footman_gait(c: f32, run: f32) -> Pose {
    let run = run.clamp(0.0, 1.0);
    let mut p = rest();
    let swing = c.cos();
    // One body wave per step. Walking is highest over the supporting foot;
    // running compresses at mid-support and rises into flight. The previous
    // two separate sine lobes caused a second bounce during every single step.
    let half = c.rem_euclid(PI) / TAU;
    let contact = gait_dimensions(run).0;
    let walk_y = 0.94 - 0.025 * (2.0 * c).cos();
    let run_y = 0.88 - 0.035 * (TAU * (half - contact * 0.5) / 0.5).cos();
    let hips_y = lerp(walk_y, run_y, run);
    p.hips = Jp {
        t: Some(Vec3::new(-swing * 0.008, hips_y + 1.05 - super::model::HIP_REST_Y, 0.0)),
        r: e3(0.0, swing * 0.035, swing * 0.012),
    };
    let lean = lerp(0.035, 0.16, run);
    p.torso = Jp::r(e3(lean, -swing * lerp(0.045, 0.085, run), -swing * 0.015));
    p.head = Jp::r(e3(-lean * 0.7, swing * 0.025, swing * 0.009));
    // An equipped knight pumps the arms opposite the legs, with soft elbows and
    // the equipment kept close. Avoid the old rigid, shoulder-high sword carry.
    p.sh_r = Jp::r(e3(-swing * lerp(0.20, 0.38, run), 0.0, lerp(0.025, 0.06, run)));
    p.sh_l = Jp::r(e3(swing * lerp(0.14, 0.28, run), 0.0, -lerp(0.025, 0.06, run)));
    p.el_r = Jp::r(rx(-lerp(0.20, 0.78, run) - swing.min(0.0) * 0.08));
    p.el_l = Jp::r(rx(-lerp(0.25, 0.72, run) + swing.max(0.0) * 0.06));
    // Counter the bent elbow so the blade stays angled down beside the runner,
    // rather than flicking horizontally in front of the body on every step.
    p.sword = Jp::r(e3(lerp(1.75, 2.20, run) + swing * 0.18 * run, 0.3, -0.04 * run));
    p.shield = Jp { t: Some(SHIELD_GAIT_T), r: shield_gait_r() };
    let mut targets = [false, true].map(|right| {
        let (z, lift, pitch) = foot_target(c + if right { PI } else { 0.0 }, run);
        let (hip, knee, foot) = leg_vectors(right);
        (Vec3::new(hip.x + knee.x + foot.x, lift, z), rx(pitch))
    });
    // A recovery foot is free to lift higher. Lowering the whole body to reach
    // that airborne foot introduced extra dips in every running flight phase.
    let rig = super::footman::leg_rig();
    let reach = (rig.knee.length() + rig.foot.length()) * lerp(0.985, 0.94, run);
    let pelvis = p.hips.t.unwrap() + Vec3::Y * (super::model::HIP_REST_Y - 1.05);
    for (i, right) in [false, true].into_iter().enumerate() {
        if foot_in_contact(c + if right { PI } else { 0.0 }, run) { continue; }
        let hip = pelvis + p.hips.r * leg_vectors(right).0;
        let ankle_offset = targets[i].1 * (Vec3::Y * rig.ankle_height);
        let ankle = targets[i].0 + ankle_offset;
        let horizontal = Vec2::new(ankle.x - hip.x, ankle.z - hip.z).length_squared();
        let lowest_ankle = hip.y - (reach * reach - horizontal).max(0.0).sqrt();
        targets[i].0.y = targets[i].0.y.max(lowest_ankle - ankle_offset.y);
    }
    lower_pelvis_to_reach(&mut p, &targets);
    for (right, (sole, rotation)) in [false, true].into_iter().zip(targets) {
        plant_leg(&mut p, right, sole, rotation);
    }
    p
}

fn footman_loco_pose(t: f32, wp: f32, m: f32, run: f32) -> Pose {
    idle_pose(t).lerp(&footman_gait(wp, run), m)
}

/// Footman combat-stance locomotion with two extra axes driven by `movement` —
/// `back` (0..1) cross-fades toward the gait played in REVERSE phase (a backpedal: the hero
/// steps backward while still facing the foe), and `twist` (radians) yaws the pelvis+legs
/// toward the movement while the torso/head counter-rotate to stay square on the target — the
/// classic lower-body-aims-along-movement / upper-body-faces-target split every lock-on game
/// uses, here as a differential yaw on the existing joints.
pub(crate) fn stance_loco_pose(t: f32, wp: f32, m: f32, run: f32, back: f32, twist: f32) -> Pose {
    let mut p = footman_loco_pose(t, wp, m, run);
    if back > 0.001 {
        // The same cycle run backward reads as stepping back; the mid-blend "gather step" as the
        // two phases cancel is exactly what a person does reversing direction.
        p = p.lerp(&footman_loco_pose(t, -wp, m, run), back.clamp(0.0, 1.0));
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

pub fn hero_anim(
    time: Res<Time>,
    player: Res<super::PlayerRes>,
    dir: Res<crate::cinematic::DirectorState>,
    mut hero_q: Query<(&mut Hero, &HeroHealth)>,
    mut parts: Query<(&HeroPart, &mut Transform)>,
) {
    let Ok((mut hero, hh)) = hero_q.single_mut() else { return };
    let now = time.elapsed_secs();
    let dt = time.delta_secs();

    let (grounded, phase, run) = (hero.on_ground, hero.walk_phase, hero.run_amt);
    let landing = hero.animation.contact(grounded, dt, phase, run);

    // Slain: let the limbs go slack while the body keels over (root rotation owned by health.rs).
    if !player.0.is_alive() {
        hero.animation = AnimationState::default();
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
    hero.animation.block_weight += (block_target - hero.animation.block_weight) * (1.0 - (-dt * 10.0).exp());
    let block_amt = hero.animation.block_weight.clamp(0.0, 1.0);

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
            hero.run_amt.clamp(0.0, 1.0),
            hero.back_amt,
            hero.strafe_twist,
        );
        guard_overlay(&mut p, hero.stance_amt, moving);
        p
    };
    let (pose, clip) = if hero.victory {
        (victory_pose(now), MotionClip::Victory)
    } else if hero.roll_t >= 0.0 {
        // Dodge roll: fold into the tuck through the somersault's core, unfolding at both ends so
        // the dive-in / stand-up carry the transition (the root owns the actual tumble).
        let u = (hero.roll_t / super::movement::ROLL_TIME).clamp(0.0, 1.0);
        let w = smoothstep(u / 0.15) * smoothstep((1.0 - u) / 0.18);
        (loco.lerp(&roll_pose(), w), MotionClip::Roll)
    } else if hero.dash_t >= 0.0 {
        // Sand Dash slide: play the dash-swipe lunge, easing back into locomotion at the blink's tail.
        let p = (hero.dash_t / super::movement::DASH_TIME).clamp(0.0, 1.0);
        let tail = smoothstep((p - 0.7) / 0.3);
        (dash_pose(p).lerp(&loco, tail), MotionClip::Dash)
    } else if let Some((phase, p)) = &attack {
        let atk = attack_pose(hero.attack_variant, phase, *p);
        let p = if hero.on_ground && moving > 0.05 {
            action_over_loco(&atk, &loco, moving) // running / walking attack
        } else {
            atk
        };
        (p, MotionClip::Attack(hero.attack_variant))
    } else if hero.charge_t > CHARGE_GRACE && hero.on_ground {
        // Holding a Heavy Strike (the light swing has finished): coil into the overhead wind-up,
        // deepening as the bar fills. Layers over locomotion so you can creep while charging.
        let frac = (hero.charge_t / CHARGE_THRESHOLD).clamp(0.0, 1.0);
        let st = charge_stance(frac, (now * 22.0).sin());
        let p = if moving > 0.05 {
            action_over_loco(&st, &loco, moving)
        } else {
            st
        };
        (p, MotionClip::Charge)
    } else if !hero.on_ground {
        let j = jump_pose(hero.vel_y);
        let mut p = if moving > 0.05 {
            j.lerp(&leap_pose(hero.vel_y), moving) // running leap
        } else {
            j
        };
        if hero.animation.leap_right {
            std::mem::swap(&mut p.hip_l, &mut p.hip_r);
            std::mem::swap(&mut p.knee_l, &mut p.knee_r);
            std::mem::swap(&mut p.foot_l, &mut p.foot_r);
            std::mem::swap(&mut p.sh_l, &mut p.sh_r);
            std::mem::swap(&mut p.el_l, &mut p.el_r);
        }
        (p, MotionClip::Airborne)
    } else if block_amt > 0.001 {
        (brace(&loco, &defend_pose(now), block_amt, moving), MotionClip::Locomotion)
    } else {
        (loco, MotionClip::Locomotion)
    };
    let mut pose = hero.animation.blend(pose, clip, dt);
    if landing > 0.0 && attack.is_none() && grounded
        && matches!(clip, MotionClip::Locomotion) {
        landing_pose(&mut pose, landing);
    }
    // Keep the contact point through speed changes and turns too: interpolating
    // stride lengths alone moves an already-planted boot during walk/run blends.
    let contact_phase = contact_phase(phase, hero.back_amt);
    if grounded && moving > 0.95 && hero.animation.from.is_none()
        && contact_phase.is_some()
        && matches!(clip, MotionClip::Locomotion | MotionClip::Attack(_) | MotionClip::Charge) {
        let (pos, facing) = (hero.pos, hero.facing);
        hero.animation.plant_contacts(&mut pose, pos, facing, contact_phase.unwrap(), run);
    } else {
        hero.animation.contacts = [None, None];
        hero.animation.released_contacts = [None, None];
    }
    hero.animation.displayed = Some(pose);

    for (part, mut tf) in &mut parts {
        let jp = pose.get(part.joint);
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
    use super::super::{SPEED, SPRINT_MULT};

    #[test]
    fn gait_cadence_and_body_motion_stay_within_human_ranges() {
        let walk_steps = gait_phase_delta(SPEED, 0.0) / PI;
        let run_steps = gait_phase_delta(SPEED * SPRINT_MULT, 1.0) / PI;
        assert!((1.8..2.7).contains(&walk_steps), "walk cadence: {walk_steps}");
        assert!((2.6..3.7).contains(&run_steps), "run cadence: {run_steps}");
        assert!(run_steps > walk_steps);
        for run in [0.0, 1.0] {
            let mut heights = Vec::new();
            for frame in 0..120 {
                let p = footman_gait(TAU * frame as f32 / 120.0, run);
                heights.push(p.hips.t.unwrap().y + super::super::model::HIP_REST_Y - 1.05);
            }
            let min = heights.iter().copied().fold(f32::INFINITY, f32::min);
            let max = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            assert!(min > 0.83, "gait collapses into a crouch: {min}");
            assert!(max - min < 0.09, "gait bounces too far: {}", max - min);
            let peaks = (0..heights.len()).filter(|&i| {
                heights[i] > heights[(i + heights.len() - 1) % heights.len()] + 0.00001
                    && heights[i] > heights[(i + 1) % heights.len()] + 0.00001
            }).count();
            assert_eq!(peaks, 2, "one rise per step, without secondary bounces");
        }
    }

    #[test]
    fn reversing_direction_does_not_switch_planted_feet_mid_blend() {
        let phase = 0.2 * TAU;
        assert_eq!(contact_phase(phase, 0.0), Some(phase));
        assert_eq!(contact_phase(phase, 1.0), Some(-phase));
        for back in [0.1, 0.49, 0.5, 0.51, 0.9] {
            assert_eq!(contact_phase(phase, back), None);
        }
        let before = stance_loco_pose(1.0, phase, 1.0, 0.0, 0.499, 0.0);
        let after = stance_loco_pose(1.0, phase, 1.0, 0.0, 0.501, 0.0);
        for right in [false, true] {
            assert!(sole(&before, right).0.distance(sole(&after, right).0) < 0.003);
        }
    }

    #[test]
    fn sharp_sprint_turns_release_unreachable_contacts_without_collapsing_the_body() {
        let dt = 1.0 / 60.0;
        for turn in [PI * 0.5, PI] {
            let mut state = AnimationState::default();
            let mut phase = 0.0;
            let mut pos = Vec2::ZERO;
            let mut velocity = Vec2::Y * SPEED * SPRINT_MULT;
            let mut facing = 0.0;
            let mut run = 1.0;
            let desired = Vec2::new(turn.sin(), turn.cos()) * SPEED * SPRINT_MULT;
            let mut first = footman_gait(phase, run);
            state.plant_contacts(&mut first, pos, facing, phase, run);
            let mut previous_feet = [sole(&first, false).0, sole(&first, true).0];
            for _ in 0..120 {
                velocity = velocity.lerp(desired, (dt * 14.0_f32).min(1.0));
                facing = super::super::movement::lerp_angle(facing, turn, dt * 15.0);
                let speed = velocity.length();
                let target = ((speed - SPEED) / (SPEED * (SPRINT_MULT - 1.0))).clamp(0.0, 1.0);
                run += (target - run) * (1.0 - (-dt * 10.0).exp());
                pos += velocity * dt;
                phase += gait_phase_delta(speed * dt, run);
                let mut p = footman_gait(phase, run);
                state.plant_contacts(&mut p, pos, facing, phase, run);
                let height = p.hips.t.unwrap().y + super::super::model::HIP_REST_Y - 1.05;
                assert!(height > 0.55, "turn collapsed the pelvis: turn={turn}, height={height}");
                for (i, right) in [false, true].into_iter().enumerate() {
                    let foot = sole(&p, right).0;
                    if let Some(released) = state.released_contacts[i] {
                        let foot_phase = phase + if right { PI } else { 0.0 };
                        if released.cycle == (foot_phase / TAU).floor() as i32
                            && (foot_phase - released.phase).abs() < 0.001 {
                            assert!(foot.distance(previous_feet[i]) < 0.20,
                                "early toe-off snaps the foot at a sharp turn");
                            let mut held = footman_gait(phase, run);
                            state.plant_contacts(&mut held, pos, facing, phase, run);
                            assert_same_pose(&p, &held);
                        }
                    }
                    previous_feet[i] = foot;
                    if let Some((_, anchor)) = state.contacts[i] {
                        let world = Quat::from_rotation_y(facing)
                            * (sole(&p, right).0 * super::super::footman::leg_rig().world_scale);
                        assert!((pos + Vec2::new(world.x, world.z)).distance(anchor) < 0.003);
                    }
                }
            }
        }
    }

    fn sole(p: &Pose, right: bool) -> (Vec3, Vec3) {
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
        let sole = ankle + foot_rotation * Vec3::new(0.0, -rig.ankle_height, 0.0);
        (sole, foot_rotation * Vec3::Y)
    }

    #[test]
    fn supporting_feet_cancel_world_travel_at_walk_run_and_blended_speeds() {
        for run in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for speed in [0.8, 3.5, 6.125, 8.5] {
                for dt in [1.0 / 120.0, 1.0 / 30.0] {
                    let mut contacts: [Option<(i32, Vec3)>; 2] = [None, None];
                    let mut comparisons = 0;
                    let mut phase = 0.0;
                    let mut distance = 0.0;
                    for _ in 0..240 {
                        let p = footman_gait(phase, run);
                        for (i, right) in [false, true].into_iter().enumerate() {
                            let foot_phase = phase + if right { PI } else { 0.0 };
                            if !foot_in_contact(foot_phase, run) { continue; }
                            let (position, _) = sole(&p, right);
                            let world = Vec3::Z * distance + position * super::super::footman::leg_rig().world_scale;
                            let cycle = (foot_phase / TAU).floor() as i32;
                            if let Some((last_cycle, planted)) = contacts[i] {
                                if last_cycle == cycle {
                                    assert!(Vec2::new(world.x - planted.x, world.z - planted.z).length() < 0.003,
                                        "planted foot slides: run={run}, speed={speed}, dt={dt}, displacement={:?}", world - planted);
                                    comparisons += 1;
                                }
                            }
                            contacts[i] = Some((cycle, world));
                        }
                        distance += speed * dt;
                        phase += gait_phase_delta(speed * dt, run);
                    }
                    assert!(comparisons > 10, "must exercise sustained foot contact");
                }
            }
        }
    }

    #[test]
    fn foot_contact_and_recovery_have_continuous_position_and_velocity() {
        for run in [0.0, 0.5, 1.0] {
            let contact = gait_dimensions(run).0 * TAU;
            for boundary in [0.0, contact, TAU] {
                let eps = 0.0005;
                let a = foot_target(boundary - eps, run);
                let b = foot_target(boundary, run);
                let c = foot_target(boundary + eps, run);
                assert!((a.0 - c.0).abs() < 0.001);
                assert!((a.1 - c.1).abs() < 0.001);
                assert!(((b.0 - a.0) / eps - (c.0 - b.0) / eps).abs() < 0.01);
            }
        }
    }

    #[test]
    fn planted_feet_remain_fixed_while_speed_and_heading_change() {
        let dt = 1.0 / 120.0;
        let mut state = AnimationState::default();
        let mut phase = 0.0;
        let mut pos = Vec2::ZERO;
        let mut run = 0.0;
        let scale = super::super::footman::leg_rig().world_scale;
        for frame in 0..600 {
            let t = frame as f32 * dt;
            let speed = if t < 1.0 { SPEED } else if t < 3.0 { SPEED * SPRINT_MULT } else { SPEED };
            let target = ((speed - SPEED) / (SPEED * (SPRINT_MULT - 1.0))).clamp(0.0, 1.0);
            run += (target - run) * (1.0 - (-dt * 10.0).exp());
            let facing = if t < 2.0 { 0.0 } else { (t - 2.0).min(1.0) * 0.6 };
            pos += Vec2::new(facing.sin(), facing.cos()) * speed * dt;
            phase += gait_phase_delta(speed * dt, run);
            let mut p = footman_gait(phase, run);
            state.plant_contacts(&mut p, pos, facing, phase, run);
            for (i, right) in [false, true].into_iter().enumerate() {
                if let Some((_, anchor)) = state.contacts[i] {
                    let world = Quat::from_rotation_y(facing) * (sole(&p, right).0 * scale);
                    let actual = pos + Vec2::new(world.x, world.z);
                    assert!(actual.distance(anchor) < 0.003,
                        "contact moved through a speed/heading change: frame={frame}, run={run}, error={}", actual.distance(anchor));
                }
            }
        }
    }

    #[test]
    fn running_has_flight_and_a_longer_step_than_walking() {
        let walk_step = PI / gait_phase_delta(1.0, 0.0);
        let run_step = PI / gait_phase_delta(1.0, 1.0);
        assert!(run_step > walk_step * 1.5);
        // After left toe-off, before right touchdown, both boots recover.
        let phase = 0.44 * TAU;
        assert!(!foot_in_contact(phase, 1.0));
        assert!(!foot_in_contact(phase + PI, 1.0));
        assert!(foot_in_contact(phase, 0.0) || foot_in_contact(phase + PI, 0.0));
    }

    fn assert_same_pose(a: &Pose, b: &Pose) {
        for joint in [Joint::Hips, Joint::Torso, Joint::Head, Joint::ShoulderL, Joint::ShoulderR,
            Joint::ElbowL, Joint::ElbowR, Joint::HipL, Joint::HipR, Joint::KneeL,
            Joint::KneeR, Joint::FootL, Joint::FootR, Joint::Shield, Joint::Sword] {
            let delta = a.get(joint).r.inverse() * b.get(joint).r;
            assert!(delta.to_scaled_axis().length() < 0.001, "clip entry snaps at {joint:?}");
            if let (Some(a), Some(b)) = (a.get(joint).t, b.get(joint).t) {
                assert!(a.distance(b) < 0.0001);
            }
        }
    }

    #[test]
    fn takeoff_landing_and_interrupted_fades_start_at_the_displayed_pose() {
        for phase in [0.0, 0.7, PI, 4.8] {
            let dt = 1.0 / 60.0;
            let mut state = AnimationState::default();
            let loco = footman_gait(phase, 1.0);
            state.blend(loco, MotionClip::Locomotion, dt);
            state.contact(false, dt, phase, 1.0);
            let first_air = state.blend(leap_pose(6.5), MotionClip::Airborne, dt);
            assert_same_pose(&loco, &first_air);
            for i in 0..8 {
                state.blend(leap_pose(6.5 - i as f32 * 0.5), MotionClip::Airborne, dt);
            }
            let last_air = state.displayed.unwrap();
            state.contact(true, dt, phase, 1.0);
            let first_ground = state.blend(loco, MotionClip::Locomotion, dt);
            assert_same_pose(&last_air, &first_ground);
            let recovering = state.blend(loco, MotionClip::Locomotion, dt);
            let interrupted = state.blend(leap_pose(6.5), MotionClip::Airborne, dt);
            assert_same_pose(&recovering, &interrupted);
            for _ in 0..12 { state.blend(leap_pose(3.0), MotionClip::Airborne, dt); }
            assert_same_pose(&state.displayed.unwrap(), &leap_pose(3.0));
        }
    }

    #[test]
    fn leap_lead_follows_the_forward_leg_and_landing_compression_preserves_contacts() {
        let mut state = AnimationState::default();
        state.contact(false, 0.016, 0.0, 1.0);
        assert!(!state.leap_right);
        state.contact(true, 0.016, 0.0, 1.0);
        state.contact(false, 0.016, PI, 1.0);
        assert!(state.leap_right);
        for run in [0.0, 0.5, 1.0] {
            for frame in 0..120 {
                let mut p = footman_gait(TAU * frame as f32 / 120.0, run);
                let before = [sole(&p, false).0, sole(&p, true).0];
                landing_pose(&mut p, 1.0);
                for (right, original) in [false, true].into_iter().zip(before) {
                    let (after, _) = sole(&p, right);
                    assert!(after.distance(original) < 0.003, "landing moved a foot: run={run}, frame={frame}");
                    assert!(after.y >= -0.003);
                }
            }
        }
    }

    #[test]
    fn footman_supporting_boots_stay_grounded_through_walk_and_run() {
        for run in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let mut max_lift = 0.0_f32;
            for frame in 0..120 {
                let phase = std::f32::consts::TAU * frame as f32 / 120.0;
                let p = footman_gait(phase, run);
                for right in [false, true] {
                    let leg_phase = phase + if right { PI } else { 0.0 };
                    let (position, _) = sole(&p, right);
                    let rotation = foot_transform(&p, right).1;
                    let rig = super::super::footman::leg_rig();
                    let heel_y = (position + rotation * Vec3::new(0.0, 0.0, -rig.sole_back)).y;
                    let toe_y = (position + rotation * Vec3::new(0.0, 0.0, rig.sole_front)).y;
                    let lowest = heel_y.min(toe_y);
                    assert!(lowest >= -0.005, "boot penetrates ground: run={run}, phase={leg_phase}, y={lowest}");
                    if foot_in_contact(leg_phase, run) {
                        assert!(lowest.abs() <= 0.005, "supporting heel/toe floats: run={run}, phase={leg_phase}, y={lowest}");
                    }
                    max_lift = max_lift.max(position.y);
                }
            }
            assert!(max_lift > 0.05, "recovery foot must lift, not skate");
        }
    }

    #[test]
    fn footman_gait_is_periodic_and_blends_back_to_idle() {
        for run in [0.0, 0.5, 1.0] {
            let a = footman_gait(0.0, run);
            let b = footman_gait(std::f32::consts::TAU, run);
            for j in [Joint::Hips, Joint::HipL, Joint::HipR, Joint::KneeL, Joint::KneeR, Joint::FootL, Joint::FootR] {
                assert!(a.get(j).r.angle_between(b.get(j).r) < 0.001, "periodicity at {j:?}, run={run}");
            }
            let stopped = stance_loco_pose(2.0, 1.7, 0.0, run, 0.0, 0.0);
            let idle = idle_pose(2.0);
            assert!(stopped.hips.t.unwrap().distance(idle.hips.t.unwrap()) < 0.0001);
            assert!(stopped.knee_l.r.angle_between(idle.knee_l.r) < 0.001);
        }
    }

    #[test]
    fn combat_guard_preserves_moving_footman_support_and_level_soles() {
        for back in [0.0, 1.0] {
            for twist in [-0.7, 0.0, 0.7] {
                for frame in 0..120 {
                    let phase = std::f32::consts::TAU * frame as f32 / 120.0;
                    let mut p = stance_loco_pose(2.0, phase, 1.0, 0.0, back, twist);
                    guard_overlay(&mut p, 1.0, 1.0);
                    for right in [false, true] {
                        let (position, normal) = sole(&p, right);
                        assert!(position.y >= -0.025, "guard must not drive the moving boot into the ground");
                        let foot_phase = if back > 0.5 { -phase } else { phase } + if right { PI } else { 0.0 };
                        if foot_target(foot_phase, 0.0).1 < 0.00001 {
                            assert!(normal.dot(Vec3::Y) > 0.995, "guard must preserve level supporting soles");
                        }
                    }
                }
            }
        }
    }
}
