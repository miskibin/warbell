//! **Shared quadruped rig + animator** — a faithful port of the studio `utils/quadruped/` (the
//! same source-of-truth pattern as [`crate::biped`], but a four-legged skeleton). Animals that have
//! a studio model (dog/wolf/horse/deer/camel/bear/polar bear) drive the studio `updateQuadruped-
//! Animation` clips (idle/walk/run/sit/lie/attack) off a small ECS contract:
//!
//! - [`QuadPart`] tags each joint entity (with a back-reference to its `root`).
//! - [`QuadDrive`] is the per-frame animation input a critter's AI fills.
//! - [`animate_quad`] reads every drive, builds one [`QuadPose`] per root, writes it on the joints.
//!
//! Joint tree (studio `buildQuadruped`): root → spine → {body mesh, chest, tail, back legs}; chest →
//! {neck → head, front legs}; each leg = shoulder/hip → elbow/knee → paw (all rotate on X). The
//! rig's **spawn pose is the idle stance** so the model viewer (no animator) shows a standing animal.

use std::collections::HashMap;
use std::f32::consts::PI;

use bevy::prelude::*;

use crate::creature::{surf, CreatureMaterial, Surf};
use crate::meshkit::tinted_hex as tinted;

// ── Species + config (port of `quadrupedSpecies.ts`) ──────────────────────────────────

/// A studio quadruped model. `PolarBear` reuses the bear's dimensions with a white coat (the studio
/// `resolveQuadrupedSpecies` maps it to `bear`; we keep it distinct for the colour).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuadSpecies {
    Dog,
    Wolf,
    Horse,
    Deer,
    Camel,
    Bear,
    PolarBear,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ears {
    Pointed,
    Round,
    Long,
}

/// How a hostile quadruped strikes — keys the attack clip in [`quad_pose`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AttackStyle {
    Bite,
    Swipe,
    Charge,
}

/// One species' geometry + gait + colour table (studio `QuadrupedSpeciesConfig` + the four
/// customization colours baked per species: fur/belly/accent/detail).
#[derive(Clone, Copy)]
pub struct QuadConfig {
    pub root_y: f32,
    body: Vec3, // w, h, l
    chest_z: f32,
    neck_h: f32,
    neck_angle: f32,
    neck_w: f32,
    head: Vec3,
    snout: Vec3,
    leg_upper: f32,
    leg_lower: f32,
    /// Leg box thickness (X/Z). Most species 0.07; bears are chunky (0.11).
    leg_width: f32,
    paw_w: f32,
    paw_h: f32,
    spread: f32,
    tail_h: f32,
    tail_angle: f32,
    tail_vis: bool,
    ears: Ears,
    antlers: bool,
    hump: bool,
    /// A bear-style hump rising over the FRONT shoulders (distinct from the camel's mid-back `hump`).
    shoulder_hump: bool,
    mane: bool,
    collar: bool,
    walk_speed: f32,
    run_speed: f32,
    walk_amp: f32,
    run_amp: f32,
    sit_root_y: f32,
    lie_root_y: f32,
    pub attack: AttackStyle,
    fur: u32,
    belly: u32,
    accent: u32,
    detail: u32,
}

/// The per-species config (studio `SPECIES` table dims; colours chosen to match our wildlife).
pub fn quad_config(s: QuadSpecies) -> QuadConfig {
    use AttackStyle::*;
    use Ears::*;
    match s {
        QuadSpecies::Dog => QuadConfig {
            root_y: 0.42, body: Vec3::new(0.26, 0.26, 0.55), chest_z: 0.25,
            neck_h: 0.25, neck_angle: 0.6, neck_w: 0.16, head: Vec3::new(0.2, 0.2, 0.22),
            snout: Vec3::new(0.12, 0.1, 0.15), leg_upper: 0.18, leg_lower: 0.18, leg_width: 0.07, paw_w: 0.09, paw_h: 0.05, spread: 0.11,
            tail_h: 0.25, tail_angle: 0.5, tail_vis: true, ears: Pointed, antlers: false, hump: false, shoulder_hump: false, mane: false, collar: true,
            walk_speed: 6.5, run_speed: 11.0, walk_amp: 1.0, run_amp: 1.5, sit_root_y: 0.25, lie_root_y: 0.12, attack: Bite,
            fur: 0x9a6b3f, belly: 0xc8a070, accent: 0xa02a26, detail: 0x201810, // accent = red collar
        },
        QuadSpecies::Wolf => QuadConfig {
            root_y: 0.48, body: Vec3::new(0.24, 0.22, 0.62), chest_z: 0.28,
            neck_h: 0.28, neck_angle: 0.55, neck_w: 0.14, head: Vec3::new(0.19, 0.18, 0.24),
            snout: Vec3::new(0.11, 0.09, 0.18), leg_upper: 0.2, leg_lower: 0.2, leg_width: 0.07, paw_w: 0.08, paw_h: 0.05, spread: 0.1,
            tail_h: 0.32, tail_angle: 0.45, tail_vis: true, ears: Pointed, antlers: false, hump: false, shoulder_hump: false, mane: false, collar: false,
            walk_speed: 6.8, run_speed: 12.0, walk_amp: 1.1, run_amp: 1.6, sit_root_y: 0.28, lie_root_y: 0.14, attack: Bite,
            fur: 0x6b6f78, belly: 0x8a8e96, accent: 0x494d55, detail: 0x141414,
        },
        QuadSpecies::Horse => QuadConfig {
            root_y: 0.68, body: Vec3::new(0.3, 0.28, 0.72), chest_z: 0.32,
            neck_h: 0.38, neck_angle: 0.45, neck_w: 0.18, head: Vec3::new(0.18, 0.2, 0.28),
            snout: Vec3::new(0.1, 0.08, 0.2), leg_upper: 0.28, leg_lower: 0.26, leg_width: 0.07, paw_w: 0.1, paw_h: 0.05, spread: 0.13,
            tail_h: 0.35, tail_angle: 0.3, tail_vis: true, ears: Long, antlers: false, hump: false, shoulder_hump: false, mane: true, collar: false,
            walk_speed: 5.5, run_speed: 9.5, walk_amp: 1.2, run_amp: 1.8, sit_root_y: 0.42, lie_root_y: 0.22, attack: Charge,
            fur: 0x6b4a2e, belly: 0x7a5a3a, accent: 0x241810, detail: 0x1a140e,
        },
        QuadSpecies::Deer => QuadConfig {
            root_y: 0.55, body: Vec3::new(0.2, 0.2, 0.58), chest_z: 0.26,
            neck_h: 0.22, neck_angle: 0.5, neck_w: 0.12, head: Vec3::new(0.14, 0.16, 0.2),
            snout: Vec3::new(0.08, 0.07, 0.12), leg_upper: 0.22, leg_lower: 0.22, leg_width: 0.07, paw_w: 0.06, paw_h: 0.04, spread: 0.09,
            tail_h: 0.12, tail_angle: 0.6, tail_vis: true, ears: Long, antlers: true, hump: false, shoulder_hump: false, mane: false, collar: false,
            walk_speed: 6.0, run_speed: 10.5, walk_amp: 0.9, run_amp: 1.4, sit_root_y: 0.32, lie_root_y: 0.16, attack: Charge,
            fur: 0xa9794a, belly: 0xd8c0a0, accent: 0x8a5a32, detail: 0x6b4a2a,
        },
        QuadSpecies::Camel => QuadConfig {
            root_y: 0.72, body: Vec3::new(0.28, 0.26, 0.7), chest_z: 0.3,
            neck_h: 0.42, neck_angle: 0.35, neck_w: 0.14, head: Vec3::new(0.16, 0.16, 0.22),
            snout: Vec3::new(0.09, 0.08, 0.14), leg_upper: 0.3, leg_lower: 0.28, leg_width: 0.07, paw_w: 0.1, paw_h: 0.05, spread: 0.12,
            tail_h: 0.15, tail_angle: 0.4, tail_vis: true, ears: Round, antlers: false, hump: true, shoulder_hump: false, mane: false, collar: false,
            walk_speed: 5.0, run_speed: 8.5, walk_amp: 0.85, run_amp: 1.2, sit_root_y: 0.48, lie_root_y: 0.28, attack: Bite,
            fur: 0xc2a06a, belly: 0xd8c098, accent: 0x8a6a40, detail: 0x4a3a22,
        },
        QuadSpecies::Bear => QuadConfig {
            // A bear reads as a big LOW barrel on short thick legs with a heavy forward head. Earlier
            // it was a tall box on thin gappy stilts ("incomprehensible") — now: long wide body, short
            // chunky legs (width 0.18), low stance, big snouted head.
            root_y: 0.52, body: Vec3::new(0.46, 0.42, 0.78), chest_z: 0.26,
            neck_h: 0.1, neck_angle: 0.45, neck_w: 0.26, head: Vec3::new(0.32, 0.27, 0.32),
            snout: Vec3::new(0.2, 0.14, 0.18), leg_upper: 0.16, leg_lower: 0.15, leg_width: 0.18, paw_w: 0.18, paw_h: 0.08, spread: 0.18,
            tail_h: 0.08, tail_angle: 0.3, tail_vis: true, ears: Round, antlers: false, hump: false, shoulder_hump: true, mane: false, collar: false,
            walk_speed: 5.5, run_speed: 9.0, walk_amp: 0.8, run_amp: 1.3, sit_root_y: 0.24, lie_root_y: 0.1, attack: Swipe,
            fur: 0x5a4632, belly: 0x6b5640, accent: 0x3a2c1e, detail: 0x141414,
        },
        QuadSpecies::PolarBear => QuadConfig {
            fur: 0xe8eaf0, belly: 0xd0d4dc, accent: 0xb8bcc8, detail: 0x2a2a30,
            ..quad_config(QuadSpecies::Bear)
        },
    }
}

// ── Joints ────────────────────────────────────────────────────────────────────────────

/// One animated joint of a quadruped. `root` points back at the [`QuadDrive`] entity.
#[derive(Component)]
pub struct QuadPart {
    pub joint: QuadJoint,
    pub root: Entity,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QuadJoint {
    Root,
    Spine,
    Chest,
    Neck,
    Head,
    Tail,
    FlShoulder,
    FlElbow,
    FlPaw,
    FrShoulder,
    FrElbow,
    FrPaw,
    BlHip,
    BlKnee,
    BlPaw,
    BrHip,
    BrKnee,
    BrPaw,
}

/// Per-frame animation inputs a critter's AI writes each tick; [`animate_quad`] turns it into a pose.
#[derive(Component)]
pub struct QuadDrive {
    pub species: QuadSpecies,
    /// 0..1 blend toward "moving" (idle ↔ gait).
    pub moving_amt: f32,
    /// 0..1 blend toward "running" (walk ↔ run).
    pub run_amt: f32,
    /// 0..1 eased blend toward the seated pose.
    pub sit_amt: f32,
    /// 0..1 eased blend toward the lying pose.
    pub lie_amt: f32,
    /// Gait clock (advanced by the AI at movement speed); `0` lets [`animate_quad`] use wall time.
    pub phase: f32,
    /// Mid-attack this frame.
    pub attacking: bool,
    /// 0..1 progress through the current strike.
    pub attack_t: f32,
}

impl QuadDrive {
    pub fn new(species: QuadSpecies) -> Self {
        QuadDrive {
            species,
            moving_amt: 0.0,
            run_amt: 0.0,
            sit_amt: 0.0,
            lie_amt: 0.0,
            phase: 0.0,
            attacking: false,
            attack_t: 0.0,
        }
    }
}

// ── Pose (port of `quadrupedAnimations.ts`) ────────────────────────────────────────────

/// A working pose table of joint angles (mostly X rotations, like the studio `QuadrupedPose`, plus
/// the few Y/Z axes the swipe/idle use). Assembled into per-joint quats by [`P::finish`].
#[derive(Clone, Copy)]
struct P {
    root_y: f32,
    root_z: f32,
    root_rx: f32,
    spine_rx: f32,
    spine_ry: f32,
    chest_rx: f32,
    neck_rx: f32,
    head_rx: f32,
    tail_rx: f32,
    tail_ry: f32,
    fl_sh_x: f32,
    fl_sh_z: f32,
    fl_el_x: f32,
    fl_pw_x: f32,
    fr_sh_x: f32,
    fr_sh_z: f32,
    fr_el_x: f32,
    fr_pw_x: f32,
    bl_hp_x: f32,
    bl_kn_x: f32,
    bl_pw_x: f32,
    br_hp_x: f32,
    br_kn_x: f32,
    br_pw_x: f32,
}

/// The resolved per-joint transforms one frame writes onto the rig.
struct QuadPose {
    root_t: Vec3,
    root: Quat,
    spine: Quat,
    chest: Quat,
    neck: Quat,
    head: Quat,
    tail: Quat,
    fl_sh: Quat,
    fl_el: Quat,
    fl_pw: Quat,
    fr_sh: Quat,
    fr_el: Quat,
    fr_pw: Quat,
    bl_hp: Quat,
    bl_kn: Quat,
    bl_pw: Quat,
    br_hp: Quat,
    br_kn: Quat,
    br_pw: Quat,
}

fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}

impl P {
    /// The studio `idle` pose (neck/head/tail at their rest angles, legs straight).
    fn idle(c: &QuadConfig) -> P {
        P {
            root_y: c.root_y, root_z: 0.0, root_rx: 0.0,
            spine_rx: 0.0, spine_ry: 0.0, chest_rx: 0.0,
            neck_rx: c.neck_angle, head_rx: -c.neck_angle, tail_rx: c.tail_angle, tail_ry: 0.0,
            fl_sh_x: 0.0, fl_sh_z: 0.0, fl_el_x: 0.0, fl_pw_x: 0.0,
            fr_sh_x: 0.0, fr_sh_z: 0.0, fr_el_x: 0.0, fr_pw_x: 0.0,
            bl_hp_x: 0.0, bl_kn_x: 0.0, bl_pw_x: 0.0,
            br_hp_x: 0.0, br_kn_x: 0.0, br_pw_x: 0.0,
        }
    }

    fn sit(c: &QuadConfig) -> P {
        P {
            root_y: c.sit_root_y, root_rx: -0.4,
            neck_rx: c.neck_angle + 0.2, head_rx: -c.neck_angle + 0.2, tail_rx: c.tail_angle + 0.7,
            fl_sh_x: 0.4, fr_sh_x: 0.4,
            bl_hp_x: -0.8, bl_kn_x: 1.2, bl_pw_x: -0.4,
            br_hp_x: -0.8, br_kn_x: 1.2, br_pw_x: -0.4,
            ..P::idle(c)
        }
    }

    fn lie(c: &QuadConfig) -> P {
        P {
            root_y: c.lie_root_y,
            neck_rx: c.neck_angle * 0.35, head_rx: -c.neck_angle * 0.35, tail_rx: c.tail_angle * 0.4,
            fl_sh_x: -1.2, fr_sh_x: -1.2,
            bl_hp_x: 1.2, bl_kn_x: -1.2,
            br_hp_x: 1.2, br_kn_x: -1.2,
            ..P::idle(c)
        }
    }

    fn lerp(&self, o: &P, t: f32) -> P {
        let l = |a: f32, b: f32| a + (b - a) * t;
        P {
            root_y: l(self.root_y, o.root_y), root_z: l(self.root_z, o.root_z), root_rx: l(self.root_rx, o.root_rx),
            spine_rx: l(self.spine_rx, o.spine_rx), spine_ry: l(self.spine_ry, o.spine_ry), chest_rx: l(self.chest_rx, o.chest_rx),
            neck_rx: l(self.neck_rx, o.neck_rx), head_rx: l(self.head_rx, o.head_rx), tail_rx: l(self.tail_rx, o.tail_rx), tail_ry: l(self.tail_ry, o.tail_ry),
            fl_sh_x: l(self.fl_sh_x, o.fl_sh_x), fl_sh_z: l(self.fl_sh_z, o.fl_sh_z), fl_el_x: l(self.fl_el_x, o.fl_el_x), fl_pw_x: l(self.fl_pw_x, o.fl_pw_x),
            fr_sh_x: l(self.fr_sh_x, o.fr_sh_x), fr_sh_z: l(self.fr_sh_z, o.fr_sh_z), fr_el_x: l(self.fr_el_x, o.fr_el_x), fr_pw_x: l(self.fr_pw_x, o.fr_pw_x),
            bl_hp_x: l(self.bl_hp_x, o.bl_hp_x), bl_kn_x: l(self.bl_kn_x, o.bl_kn_x), bl_pw_x: l(self.bl_pw_x, o.bl_pw_x),
            br_hp_x: l(self.br_hp_x, o.br_hp_x), br_kn_x: l(self.br_kn_x, o.br_kn_x), br_pw_x: l(self.br_pw_x, o.br_pw_x),
        }
    }

    fn finish(&self) -> QuadPose {
        let yx = |y: f32, x: f32| Quat::from_rotation_y(y) * rx(x);
        QuadPose {
            root_t: Vec3::new(0.0, self.root_y, self.root_z),
            root: rx(self.root_rx),
            spine: yx(self.spine_ry, self.spine_rx),
            chest: rx(self.chest_rx),
            neck: rx(self.neck_rx),
            head: rx(self.head_rx),
            tail: yx(self.tail_ry, self.tail_rx),
            fl_sh: Quat::from_rotation_z(self.fl_sh_z) * rx(self.fl_sh_x),
            fl_el: rx(self.fl_el_x),
            fl_pw: rx(self.fl_pw_x),
            fr_sh: Quat::from_rotation_z(self.fr_sh_z) * rx(self.fr_sh_x),
            fr_el: rx(self.fr_el_x),
            fr_pw: rx(self.fr_pw_x),
            bl_hp: rx(self.bl_hp_x),
            bl_kn: rx(self.bl_kn_x),
            bl_pw: rx(self.bl_pw_x),
            br_hp: rx(self.br_hp_x),
            br_kn: rx(self.br_kn_x),
            br_pw: rx(self.br_pw_x),
        }
    }
}

impl QuadPose {
    fn get(&self, j: QuadJoint) -> (Quat, Option<Vec3>) {
        use QuadJoint::*;
        match j {
            Root => (self.root, Some(self.root_t)),
            Spine => (self.spine, None),
            Chest => (self.chest, None),
            Neck => (self.neck, None),
            Head => (self.head, None),
            Tail => (self.tail, None),
            FlShoulder => (self.fl_sh, None),
            FlElbow => (self.fl_el, None),
            FlPaw => (self.fl_pw, None),
            FrShoulder => (self.fr_sh, None),
            FrElbow => (self.fr_el, None),
            FrPaw => (self.fr_pw, None),
            BlHip => (self.bl_hp, None),
            BlKnee => (self.bl_kn, None),
            BlPaw => (self.bl_pw, None),
            BrHip => (self.br_hp, None),
            BrKnee => (self.br_kn, None),
            BrPaw => (self.br_pw, None),
        }
    }
}

fn ease_out_cubic(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

/// The studio `applyQuadrupedGait`, accumulated onto `p` (additive). `run` blends the walk→run
/// speed/amp/leg-phase; `amp` is already scaled by moving_amt.
fn apply_gait(p: &mut P, c: &QuadConfig, time: f32, run: f32, amp: f32) {
    let speed = c.walk_speed + (c.run_speed - c.walk_speed) * run;
    let phase = time * speed;
    let s2 = (phase * 2.0).sin();
    let c2 = (phase * 2.0).cos();
    p.root_y += s2 * 0.02 * amp;
    p.spine_rx += s2 * 0.05 * amp;
    p.chest_rx += c2 * 0.05 * amp;
    p.neck_rx += (phase * 2.0 + PI).sin() * 0.05 * amp;
    p.tail_rx += s2 * 0.1 * amp;
    p.tail_ry += phase.sin() * 0.2 * amp;

    // Diagonal pairs; running offsets the back-leg phases (studio: br +0.2π, bl +1.2π vs +π walk).
    let off = run * PI * 0.2;
    let fl = phase;
    let fr = phase + PI;
    let bl = phase + PI + off;
    let br = phase + off;
    p.fl_sh_x += fl.sin() * 0.5 * amp;
    p.fl_el_x += (fl - PI / 2.0).sin().max(0.0) * 0.8 * amp;
    p.fr_sh_x += fr.sin() * 0.5 * amp;
    p.fr_el_x += (fr - PI / 2.0).sin().max(0.0) * 0.8 * amp;
    p.bl_hp_x += bl.sin() * 0.5 * amp;
    p.bl_kn_x += (bl + PI / 2.0).sin().max(0.0) * 0.8 * amp;
    p.br_hp_x += br.sin() * 0.5 * amp;
    p.br_kn_x += (br + PI / 2.0).sin().max(0.0) * 0.8 * amp;
}

/// Studio three-phase attack progress (wind → strike → recovery), driven by an explicit 0..1 cycle.
fn attack_phase(cycle: f32, wind: f32, strike: f32) -> (u8, f32) {
    if cycle < wind {
        (0, ease_out_cubic(cycle / wind))
    } else if cycle < wind + strike {
        let t = (cycle - wind) / strike;
        (1, 1.0 - (1.0 - t).powf(2.5))
    } else {
        let span = (1.0 - wind - strike).max(0.0001);
        let t = ((cycle - wind - strike) / span).clamp(0.0, 1.0);
        (2, t * t * (3.0 - 2.0 * t))
    }
}

fn lerpf(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Apply the species' attack clip (bite/swipe/charge) over the current pose.
fn apply_attack(p: &mut P, c: &QuadConfig, t: f32) {
    let a = c.neck_angle;
    match c.attack {
        AttackStyle::Bite => {
            let (ph, pr) = attack_phase(t, 0.3, 0.2);
            match ph {
                0 => {
                    p.root_z -= lerpf(0.0, 0.1, pr);
                    p.root_y -= lerpf(0.0, 0.05, pr);
                    p.spine_rx -= lerpf(0.0, 0.1, pr);
                    p.neck_rx -= lerpf(0.0, 0.2, pr);
                    p.tail_rx += lerpf(0.0, 0.2, pr);
                }
                1 => {
                    p.root_z = lerpf(-0.1, 0.12, pr);
                    p.root_y = lerpf(c.root_y - 0.05, c.root_y + 0.03, pr);
                    p.spine_rx = lerpf(-0.1, 0.2, pr);
                    p.neck_rx = lerpf(a - 0.2, a + 0.4, pr);
                    p.head_rx = lerpf(-a - 0.2, -a + 0.4, pr);
                    p.fl_sh_x -= lerpf(0.0, 0.4, pr);
                    p.fr_sh_x -= lerpf(0.0, 0.4, pr);
                }
                _ => {
                    p.root_z = lerpf(0.12, 0.0, pr);
                    p.root_y = lerpf(c.root_y + 0.03, c.root_y, pr);
                    p.spine_rx = lerpf(0.2, 0.0, pr);
                    p.neck_rx = lerpf(a + 0.4, a, pr);
                    p.head_rx = lerpf(-a + 0.4, -a, pr);
                    p.fl_sh_x = lerpf(-0.4, 0.0, pr);
                    p.fr_sh_x = lerpf(-0.4, 0.0, pr);
                }
            }
        }
        AttackStyle::Swipe => {
            let (ph, pr) = attack_phase(t, 0.35, 0.22);
            match ph {
                0 => {
                    p.spine_ry = lerpf(0.0, -0.3, pr);
                    p.fr_sh_x = lerpf(0.0, -0.5, pr);
                    p.fr_sh_z = lerpf(0.0, 0.4, pr);
                }
                1 => {
                    p.spine_ry = lerpf(-0.3, 0.35, pr);
                    p.fr_sh_x = lerpf(-0.5, 0.8, pr);
                    p.fr_sh_z = lerpf(0.4, -0.5, pr);
                    p.fl_sh_x = lerpf(0.0, 0.3, pr);
                    p.root_z = lerpf(0.0, 0.12, pr);
                }
                _ => {
                    p.spine_ry = lerpf(0.35, 0.0, pr);
                    p.fr_sh_x = lerpf(0.8, 0.0, pr);
                    p.fr_sh_z = lerpf(-0.5, 0.0, pr);
                    p.fl_sh_x = lerpf(0.3, 0.0, pr);
                    p.root_z = lerpf(0.12, 0.0, pr);
                }
            }
        }
        AttackStyle::Charge => {
            let (ph, pr) = attack_phase(t, 0.25, 0.3);
            match ph {
                0 => {
                    p.root_z -= lerpf(0.0, 0.15, pr);
                    p.root_y -= lerpf(0.0, 0.08, pr);
                    p.neck_rx -= lerpf(0.0, 0.15, pr);
                }
                1 => {
                    p.root_z = lerpf(-0.15, 0.2, pr);
                    p.root_y = lerpf(c.root_y - 0.08, c.root_y, pr);
                    p.neck_rx = lerpf(a - 0.15, a + 0.1, pr);
                    p.head_rx = lerpf(-a + 0.15, -a - 0.1, pr);
                    apply_gait(p, c, t * 2.0, 1.0, c.run_amp * 0.6);
                }
                _ => {
                    p.root_z = lerpf(0.2, 0.0, pr);
                    p.neck_rx = lerpf(a + 0.1, a, pr);
                    p.head_rx = lerpf(-a - 0.1, -a, pr);
                }
            }
        }
    }
}

/// Select + compose the studio clips for one quadruped from its drive.
fn quad_pose(c: &QuadConfig, d: &QuadDrive, now: f32) -> P {
    let rest = d.sit_amt.max(d.lie_amt);
    if rest > 0.001 {
        // Resting: blend idle → sit/lie (sit wins if both set). Add a faint breathe.
        let target = if d.sit_amt >= d.lie_amt { P::sit(c) } else { P::lie(c) };
        let mut p = P::idle(c).lerp(&target, ease_out_cubic(rest.min(1.0)));
        p.spine_rx += (now * 2.0).sin() * 0.02 * rest;
        if c.tail_vis {
            p.tail_ry += (now * 3.0).sin() * 0.15 * rest;
        }
        return p;
    }

    let mut p = P::idle(c);
    let move_amt = d.moving_amt.clamp(0.0, 1.0);
    // Idle breathe + tail wag, fading out as it starts moving.
    let still = 1.0 - move_amt;
    p.spine_rx += (now * 2.0).sin() * 0.02 * still;
    p.chest_rx += (now * 2.0).sin() * 0.02 * still;
    if c.tail_vis {
        p.tail_ry += (now * 3.0).sin() * 0.15 * still;
    }
    if move_amt > 0.001 {
        let t = if d.phase != 0.0 { d.phase } else { now };
        apply_gait(&mut p, c, t, d.run_amt.clamp(0.0, 1.0), move_amt);
    }
    if d.attacking {
        apply_attack(&mut p, c, d.attack_t.clamp(0.0, 1.0));
    }
    p
}

/// Pose every quadruped: one [`QuadPose`] per root, written onto its [`QuadPart`]s. Ungated (like
/// the hero/biped animators) so a frozen/paused world still draws its animals posed.
pub fn animate_quad(
    time: Res<Time>,
    drives: Query<(Entity, &QuadDrive)>,
    mut parts: Query<(&QuadPart, &mut Transform)>,
    // Reused across frames so a herd's worth of animals doesn't heap-alloc a fresh map every frame.
    mut poses: Local<HashMap<Entity, QuadPose>>,
) {
    let now = time.elapsed_secs();
    poses.clear();
    poses.extend(drives.iter().map(|(e, d)| (e, quad_pose(&quad_config(d.species), d, now).finish())));
    for (part, mut tf) in &mut parts {
        if let Some(pose) = poses.get(&part.root) {
            let (rot, t) = pose.get(part.joint);
            if let Some(t) = t {
                tf.translation = t;
            }
            tf.rotation = rot;
        }
    }
}

// ── Meshes (port of `buildQuadruped`) ──────────────────────────────────────────────────

/// Eye colour, shared by every species (a dark near-black box reads as an eye on the fur head).
const EYE: u32 = 0x141414;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
/// A bevelled box (24 verts + 12 edge bevels + 8 corner tris), origin-centred — the same softened
/// low-poly silhouette the hero uses (`player::model::chamfer_box`), kept local here so animals read
/// as the same family instead of raw sharp cuboids. `e` = chamfer inset (auto-clamped to the box).
fn chamfer_box(w: f32, h: f32, d: f32, e: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (a, b, c) = (w * 0.5, h * 0.5, d * 0.5);
    let e = e.min(a * 0.49).min(b * 0.49).min(c * 0.49).max(0.001);
    let (ai, bi, ci) = (a - e, b - e, c - e);
    let pos: Vec<[f32; 3]> = vec![
        [a, -bi, -ci], [a, bi, -ci], [a, bi, ci], [a, -bi, ci],
        [-a, -bi, -ci], [-a, bi, -ci], [-a, bi, ci], [-a, -bi, ci],
        [-ai, b, -ci], [ai, b, -ci], [ai, b, ci], [-ai, b, ci],
        [-ai, -b, -ci], [ai, -b, -ci], [ai, -b, ci], [-ai, -b, ci],
        [-ai, -bi, c], [ai, -bi, c], [ai, bi, c], [-ai, bi, c],
        [-ai, -bi, -c], [ai, -bi, -c], [ai, bi, -c], [-ai, bi, -c],
    ];
    let mut raw: Vec<[u32; 3]> = Vec::new();
    let mut quad = |a: u32, b: u32, c: u32, d: u32| {
        raw.push([a, b, c]);
        raw.push([a, c, d]);
    };
    for f in 0..6u32 {
        let o = f * 4;
        quad(o, o + 1, o + 2, o + 3);
    }
    let edges = [
        [1, 2, 10, 9], [3, 0, 13, 14], [6, 5, 8, 11], [7, 4, 12, 15],
        [3, 2, 18, 17], [0, 1, 22, 21], [7, 6, 19, 16], [4, 5, 23, 20],
        [11, 10, 18, 19], [8, 9, 22, 23], [15, 14, 17, 16], [12, 13, 21, 20],
    ];
    for q in edges {
        quad(q[0], q[1], q[2], q[3]);
    }
    for t in [[2, 10, 18], [1, 9, 22], [3, 14, 17], [0, 13, 21], [6, 11, 19], [5, 8, 23], [7, 15, 16], [4, 12, 20]] {
        raw.push(t);
    }
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx: Vec<u32> = Vec::new();
    for t in raw {
        let (va, vb, vc) = (g(t[0]), g(t[1]), g(t[2]));
        let nrm = (vb - va).cross(vc - va);
        let ctr = (va + vb + vc) / 3.0;
        if nrm.dot(ctr) >= 0.0 {
            idx.extend(t);
        } else {
            idx.extend([t[0], t[2], t[1]]);
        }
    }
    let n = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}
/// Auto chamfer for a box of the given dims — a quarter of the smallest side, capped so big slabs
/// stay crisp and thin legs still bevel.
fn cham(w: f32, h: f32, d: f32) -> f32 {
    (w.min(h).min(d) * 0.26).clamp(0.012, 0.05)
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(chamfer_box(w, h, d, cham(w, h, d)).translated_by(off), c)
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(chamfer_box(w, h, d, cham(w, h, d)).rotated_by(rot).translated_by(off), c)
}
/// Merge + flat-shade + tag the whole mesh with `Fur` so the creature shader textures it.
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("quad parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    surf(base, Surf::Fur)
}

/// A per-joint mesh set for one quadruped (the legs share one upper/lower/paw mesh across all four).
pub struct QuadMeshes {
    spine: Mesh,
    neck: Mesh,
    head: Mesh,
    tail: Option<Mesh>,
    leg_upper: Mesh,
    leg_lower: Mesh,
    paw: Mesh,
}

/// Pre-uploaded handles for a [`QuadMeshes`] set.
#[derive(Clone)]
pub struct QuadHandles {
    spine: Handle<Mesh>,
    neck: Handle<Mesh>,
    head: Handle<Mesh>,
    tail: Option<Handle<Mesh>>,
    leg_upper: Handle<Mesh>,
    leg_lower: Handle<Mesh>,
    paw: Handle<Mesh>,
}

impl QuadMeshes {
    pub fn upload(self, meshes: &mut Assets<Mesh>) -> QuadHandles {
        QuadHandles {
            spine: meshes.add(self.spine),
            neck: meshes.add(self.neck),
            head: meshes.add(self.head),
            tail: self.tail.map(|m| meshes.add(m)),
            leg_upper: meshes.add(self.leg_upper),
            leg_lower: meshes.add(self.leg_lower),
            paw: meshes.add(self.paw),
        }
    }
}

/// Build the studio per-joint meshes for `species` (each in joint-local space).
pub fn quad_meshes(species: QuadSpecies) -> QuadMeshes {
    let c = quad_config(species);
    let (bw, bh, bl) = (c.body.x, c.body.y, c.body.z);

    // Spine: body box + belly slab + optional hump.
    let mut spine = vec![
        bx(bw, bh, bl, v(0.0, 0.0, 0.0), c.fur),
        bx(bw - 0.02, 0.05, bl - 0.02, v(0.0, -bh / 2.0 + 0.025, 0.0), c.belly),
    ];
    if c.hump {
        spine.push(bx(0.22, 0.18, 0.28, v(0.0, bh / 2.0 + 0.04, -0.05), c.fur));
    }
    if c.shoulder_hump {
        // A bear's muscular shoulder hump: a low WIDE rise sunk into the back over the front shoulders
        // (was a tall narrow box that poked up behind the head and read as a separate lump).
        spine.push(bx(bw * 0.9, 0.16, bl * 0.4, v(0.0, bh / 2.0 + 0.01, bl * 0.22), c.fur));
    }
    let spine = group(spine);

    // Neck (built in neck-local: mesh sits above the joint) + optional mane / collar.
    let mut neck = vec![bx(c.neck_w, c.neck_h, c.neck_w, v(0.0, c.neck_h / 2.0, 0.0), c.fur)];
    if c.mane {
        neck.push(bx(0.06, c.neck_h * 0.9, 0.04, v(0.0, c.neck_h * 0.45, -c.neck_w / 2.0 - 0.02), c.accent));
    }
    if c.collar {
        neck.push(bx(c.neck_w + 0.02, 0.05, c.neck_w + 0.02, v(0.0, c.neck_h * 0.75, 0.0), c.accent));
    }
    let neck = group(neck);

    // Head: skull + snout + nose + ears + optional antlers (head-local).
    let mut head = vec![bx(c.head.x, c.head.y, c.head.z, v(0.0, 0.0, c.head.z * 0.2), c.fur)];
    let (sw, sh, sl) = (c.snout.x, c.snout.y, c.snout.z);
    head.push(bx(sw, sh, sl, v(0.0, -sh * 0.4, c.head.z * 0.5 + sl / 2.0), c.belly));
    head.push(bx(0.06, 0.04, 0.04, v(0.0, -sh * 0.4 + sh * 0.15, c.head.z * 0.5 + sl), c.detail)); // nose
    // Eyes: a dark box on each side of the upper face (the studio quad builder omits these; our old
    // critters had them). Sized + spread by the head so every species reads with a face.
    let (ew, eh, ex, ez, ey) = (c.head.x * 0.22, c.head.y * 0.16, c.head.x * 0.33, c.head.z * 0.5 + 0.03, c.head.y * 0.12);
    head.push(bx(ew, eh, 0.03, v(ex, ey, ez), EYE));
    head.push(bx(ew, eh, 0.03, v(-ex, ey, ez), EYE));
    add_ears(&mut head, &c);
    if c.antlers {
        add_antlers(&mut head, &c);
    }
    let head = group(head);

    // Tail (tail-local: above the joint).
    let tail = c.tail_vis.then(|| group(vec![bx(0.05, c.tail_h, 0.05, v(0.0, c.tail_h / 2.0, 0.0), c.fur)]));

    // Legs (joint-local, hanging down from each joint); `leg_width` thickens the bear's limbs.
    let lw = c.leg_width;
    let leg_upper = group(vec![bx(lw, c.leg_upper, lw, v(0.0, -c.leg_upper / 2.0, 0.0), c.fur)]);
    let leg_lower = group(vec![bx(lw, c.leg_lower, lw, v(0.0, -c.leg_lower / 2.0, 0.0), c.fur)]);
    let paw = group(vec![bx(c.paw_w, c.paw_h, c.paw_w + 0.02, v(0.0, -c.paw_h / 2.0, 0.02), c.detail)]);

    QuadMeshes { spine, neck, head, tail, leg_upper, leg_lower, paw }
}

fn add_ears(head: &mut Vec<Mesh>, c: &QuadConfig) {
    let mat = if c.ears == Ears::Round { c.detail } else { c.fur };
    let hw = c.head.x / 2.0;
    match c.ears {
        Ears::Long => {
            head.push(bxr(0.04, 0.14, 0.03, v(hw + 0.02, c.head.y * 0.35, -0.04), Quat::from_rotation_z(-0.15), mat));
            head.push(bxr(0.04, 0.14, 0.03, v(-hw - 0.02, c.head.y * 0.35, -0.04), Quat::from_rotation_z(0.15), mat));
        }
        Ears::Round => {
            head.push(bxr(0.06, 0.1, 0.04, v(hw - 0.02, c.head.y * 0.4, -0.05), rx(0.2), mat));
            head.push(bxr(0.06, 0.1, 0.04, v(-hw + 0.02, c.head.y * 0.4, -0.05), rx(0.2), mat));
        }
        Ears::Pointed => {
            head.push(bxr(0.06, 0.1, 0.04, v(hw - 0.02, c.head.y * 0.4, -0.05), Quat::from_euler(EulerRot::XYZ, 0.2, 0.0, -0.2), mat));
            head.push(bxr(0.06, 0.1, 0.04, v(-hw + 0.02, c.head.y * 0.4, -0.05), Quat::from_euler(EulerRot::XYZ, 0.2, 0.0, 0.2), mat));
        }
    }
}

fn add_antlers(head: &mut Vec<Mesh>, c: &QuadConfig) {
    let mut branch = |x: f32, rot_z: f32| {
        head.push(bxr(0.03, 0.22, 0.03, v(x, 0.22, -0.02), Quat::from_rotation_z(rot_z), c.detail));
        head.push(bxr(0.025, 0.1, 0.025, v(x + rot_z * 0.06, 0.28, 0.02), Quat::from_rotation_z(rot_z * 0.8), c.detail));
    };
    branch(0.07, -0.25);
    branch(-0.07, 0.25);
}

// ── Spawn ──────────────────────────────────────────────────────────────────────────────

/// Spawn the studio quadruped skeleton for a critter, from pre-uploaded `h` handles, tagging every
/// joint [`QuadPart`] so [`animate_quad`] poses it from the root's [`QuadDrive`]. Joints spawn in the
/// idle stance, so a viewer with no animator still shows a standing animal. Built under the caller's
/// `root` entity (which carries the world transform + `QuadDrive`).
pub fn spawn_quad(commands: &mut Commands, root: Entity, mat: &Handle<CreatureMaterial>, species: QuadSpecies, h: QuadHandles) {
    use QuadJoint::*;
    let c = quad_config(species);
    let idle = P::idle(&c).finish();
    // Spawn a joint: transform-only + QuadPart tag, with the mesh placed directly on it.
    let joint = |commands: &mut Commands, parent: Entity, j: QuadJoint, pos: Vec3, mesh: Option<Handle<Mesh>>| -> Entity {
        let (rot, _) = idle.get(j);
        let mut ec = commands.spawn((Transform { translation: pos, rotation: rot, ..default() }, Visibility::Visible, QuadPart { joint: j, root }));
        if let Some(m) = mesh {
            ec.insert((Mesh3d(m), MeshMaterial3d(mat.clone())));
        }
        let e = ec.id();
        commands.entity(parent).add_child(e);
        e
    };

    let root_j = joint(commands, root, Root, Vec3::new(0.0, c.root_y, 0.0), None);
    let spine = joint(commands, root_j, Spine, Vec3::ZERO, Some(h.spine));
    let chest = joint(commands, spine, Chest, Vec3::new(0.0, 0.0, c.chest_z), None);
    let neck = joint(commands, chest, Neck, Vec3::new(0.0, c.body.y * 0.15, c.chest_z * 0.35), Some(h.neck));
    joint(commands, neck, Head, Vec3::new(0.0, c.neck_h, 0.0), Some(h.head));
    if let Some(tail) = h.tail {
        joint(commands, spine, Tail, Vec3::new(0.0, c.body.y * 0.3, -c.body.z / 2.0 + 0.05), Some(tail));
    }

    let leg_y = -c.body.y / 2.0 + 0.05;
    let front_z = c.chest_z * 0.35;
    let back_z = -c.body.z / 2.0 + 0.15;
    // Front legs parent to the chest; back legs to the spine (studio `addLegs`).
    let mut leg = |parent: Entity, sh: QuadJoint, el: QuadJoint, pw: QuadJoint, x: f32, z: f32| {
        let s = joint(commands, parent, sh, Vec3::new(x, leg_y, z), Some(h.leg_upper.clone()));
        let e = joint(commands, s, el, Vec3::new(0.0, -c.leg_upper, 0.0), Some(h.leg_lower.clone()));
        joint(commands, e, pw, Vec3::new(0.0, -c.leg_lower, 0.0), Some(h.paw.clone()));
    };
    leg(chest, FlShoulder, FlElbow, FlPaw, c.spread, front_z);
    leg(chest, FrShoulder, FrElbow, FrPaw, -c.spread, front_z);
    leg(spine, BlHip, BlKnee, BlPaw, c.spread, back_z);
    leg(spine, BrHip, BrKnee, BrPaw, -c.spread, back_z);
}

pub struct QuadrupedPlugin;

impl Plugin for QuadrupedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, animate_quad);
    }
}
