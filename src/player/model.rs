//! **Knight hero model** — box-mesh humanoid ported 1:1 from the TS `Character.tsx` mesh
//! tree (plate armour, visored helm, an iron sword baked into the right arm, a cross
//! shield on its own pivot). Built exactly like `orks.rs` / `critters.rs`: each articulated
//! part is ONE merged, flat-shaded, vertex-coloured `Mesh` against the shared white hero
//! material; feet rest at `y = 0` so the root, placed on the ground, plants the knight.
//!
//! Authoring is in TS units (the TS root group is `scale 0.5`, knight ~1.25u tall before
//! scale); the spawn applies `HERO_SCALE` so the knight stands the same height as the orks.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::palette::lin;

use super::HeroLimb;

// ── Palette (sRGB hex, matches Character.tsx) ────────────────────────────────────────
const ARMOR: u32 = 0xd6d8df;
const ARMOR_LIGHT: u32 = 0xe6e8ed;
const ARMOR_DARK: u32 = 0x9aa0aa;
const VISOR: u32 = 0x1a1a22;
const BELT: u32 = 0x3a2a1a;
const BLADE: u32 = 0xc0c6d0;
const HILT: u32 = 0x3a3a40;
const GRIP: u32 = 0x5a3a22;
const SHIELD_FACE: u32 = 0xa8b8d0;
const SHIELD_RIM: u32 = 0x6a3a22;
const SHIELD_EMBLEM: u32 = 0xd3b14c;

/// Shield rest pose (own pivot, decoupled from the left arm): slung on the left flank,
/// decorated face out (−X). Block (M3) swings it across the front.
pub const SHIELD_REST_POS: Vec3 = Vec3::new(-0.3, 0.62, 0.06);
pub fn shield_rest_rot() -> Quat {
    Quat::from_euler(EulerRot::XYZ, 0.04, -1.3, 0.05)
}
/// Block pose: shield swung across the front (face +Z), braced high.
pub const SHIELD_BLOCK_POS: Vec3 = Vec3::new(-0.12, 0.82, 0.5);
pub fn shield_block_rot() -> Quat {
    Quat::from_euler(EulerRot::XYZ, -0.12, 0.05, -0.05)
}

// ── Articulated part + spec ──────────────────────────────────────────────────────────
pub struct HeroPartDef {
    pub limb: HeroLimb,
    pub pivot: Vec3,
    /// Rest orientation of the part (identity for limbs; the shield rests rotated).
    pub rest: Quat,
    pub mesh: Mesh,
}

pub struct KnightSpec {
    pub torso: Mesh,
    pub parts: Vec<HeroPartDef>,
}

// ── Mesh helpers (local copies of the orks/critters contract) ────────────────────────
fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn tinted(mut m: Mesh, c: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(c); n]);
    m
}
/// Merge + hard flat-shade — the crisp low-poly facets the TS models use.
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("hero parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn cyl(r: f32, h: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(8).build().translated_by(off), c)
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), c)
}
fn sphere(r: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Sphere::new(r).mesh().ico(1).unwrap().translated_by(off), c)
}
/// Bake a sub-group (built in its own local space) into the parent: rotate about the group
/// origin, then translate to the group's offset — matches three.js `<group rotation pos>`.
fn baked(m: Mesh, rot: Quat, off: Vec3) -> Mesh {
    m.rotated_by(rot).translated_by(off)
}

// ── Build (default = bare iron sword + cross shield) ─────────────────────────────────
pub fn build_knight() -> KnightSpec {
    // Static torso: legs are articulated; belt + body + breastplate are baked in here.
    let torso = group(vec![
        bx(0.42, 0.08, 0.22, v(0.0, 0.4, 0.0), BELT), // belt
        bx(0.42, 0.46, 0.26, v(0.0, 0.66, 0.0), ARMOR), // body
        bx(0.32, 0.32, 0.02, v(0.0, 0.70, 0.135), ARMOR_LIGHT), // breastplate
    ]);

    // Head: helm + visor slit + crest.
    let head = group(vec![
        bx(0.32, 0.3, 0.32, v(0.0, 0.0, 0.0), ARMOR_LIGHT),
        bx(0.24, 0.06, 0.01, v(0.0, -0.01, 0.165), VISOR),
        bx(0.34, 0.06, 0.34, v(0.0, 0.18, 0.0), ARMOR_DARK),
    ]);

    // Right arm (sword hand): shoulder + upper + cuff, with the iron sword's parts baked
    // individually at the hand so the blade swings with the arm. Sword sits at arm-local
    // (0,-0.5,0.06) rotated x=-π/2 so it extends FORWARD (+Z). CRITICAL: every part is an
    // indexed primitive merged by the SINGLE outer group() — do NOT pre-`group()` a sub-part
    // and re-merge it (flat-shading makes it non-indexed → merge corrupts the geometry, which
    // is what hid the sword). Matches the ork-club build in `orks.rs`.
    let sw_rot = rx(-std::f32::consts::FRAC_PI_2);
    let sw_off = v(0.0, -0.5, 0.06);
    let mut arm_r_parts = vec![
        bx(0.18, 0.1, 0.28, v(0.0, -0.02, 0.0), ARMOR_LIGHT), // shoulder
        bx(0.12, 0.42, 0.22, v(0.0, -0.21, 0.0), ARMOR), // upper
        bx(0.13, 0.08, 0.23, v(0.0, -0.45, 0.0), ARMOR_DARK), // cuff
    ];
    for part in [
        sphere(0.05, v(0.0, 0.14, 0.0), HILT), // pommel
        cyl(0.03, 0.14, v(0.0, 0.06, 0.0), GRIP), // grip
        bx(0.28, 0.06, 0.08, v(0.0, -0.04, 0.0), HILT), // guard
        bx(0.09, 0.78, 0.03, v(0.0, -0.46, 0.0), BLADE), // blade (a touch longer/wider to read)
        cone(0.05, 0.12, v(0.0, -0.9, 0.0), rx(std::f32::consts::PI), BLADE), // tip
    ] {
        arm_r_parts.push(baked(part, sw_rot, sw_off));
    }
    let arm_r = group(arm_r_parts);

    // Left arm (shield hand): shoulder + upper + cuff (shield is a separate part).
    let arm_l = group(vec![
        bx(0.18, 0.1, 0.28, v(0.0, -0.02, 0.0), ARMOR_LIGHT),
        bx(0.12, 0.42, 0.22, v(0.0, -0.21, 0.0), ARMOR),
        bx(0.13, 0.08, 0.23, v(0.0, -0.45, 0.0), ARMOR_DARK),
    ]);

    // Shield (own pivot): heater plate + raised rim + recessed field + gold cross emblem.
    let shield = group(vec![
        bx(0.42, 0.58, 0.05, v(0.0, 0.0, 0.0), SHIELD_FACE), // plate
        bx(0.46, 0.62, 0.014, v(0.0, 0.0, 0.028), SHIELD_RIM), // rim
        bx(0.34, 0.5, 0.014, v(0.0, 0.0, 0.034), SHIELD_FACE), // inset field
        bx(0.07, 0.4, 0.014, v(0.0, 0.03, 0.04), SHIELD_EMBLEM), // cross vertical
        bx(0.3, 0.07, 0.014, v(0.0, 0.1, 0.04), SHIELD_EMBLEM), // cross horizontal
    ]);

    // Legs (built top-at-hip so the pivot sits at the hip; foot rests at root y≈0).
    let leg = || group(vec![bx(0.16, 0.36, 0.18, v(0.0, -0.18, 0.0), ARMOR_DARK)]);

    let parts = vec![
        HeroPartDef { limb: HeroLimb::LegR, pivot: v(0.1, 0.36, 0.0), rest: Quat::IDENTITY, mesh: leg() },
        HeroPartDef { limb: HeroLimb::LegL, pivot: v(-0.1, 0.36, 0.0), rest: Quat::IDENTITY, mesh: leg() },
        HeroPartDef { limb: HeroLimb::ArmR, pivot: v(0.27, 0.87, 0.0), rest: Quat::IDENTITY, mesh: arm_r },
        HeroPartDef { limb: HeroLimb::ArmL, pivot: v(-0.27, 0.87, 0.0), rest: Quat::IDENTITY, mesh: arm_l },
        HeroPartDef { limb: HeroLimb::Head, pivot: v(0.0, 1.04, 0.0), rest: Quat::IDENTITY, mesh: head },
        HeroPartDef { limb: HeroLimb::Shield, pivot: SHIELD_REST_POS, rest: shield_rest_rot(), mesh: shield },
    ];

    KnightSpec { torso, parts }
}
