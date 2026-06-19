//! **Knight hero model** — a Bevy port of the user's procedural three.js "Low-Poly Knight Studio"
//! (`knightBuilder.ts`): a finely-articulated knight (hips → torso → neck → head;
//! shoulder → elbow → hand; hip → knee → foot) in light steel plate with bronze trim, a gold
//! rampant-lion heater shield, and the equipped weapon baked into the right hand.
//!
//! This builds only the **meshes** (one merged, flat-shaded, vertex-coloured `Mesh` per joint,
//! against the shared white creature material); [`super`] spawns the actual joint *hierarchy* of
//! entities from them, and [`super::anim`] poses the joints. Authoring is in the TS units (the
//! knight stands ~1.85u tall before scale); `HERO_SCALE` brings it down to the orks' height.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;
use std::f32::consts::PI;

use crate::creature::{surf_code, Surf};
use crate::palette::lin;

// ── Palette (the customizer's defaults + the held-weapon tints, sRGB hex) ─────────────
const ARMOR: u32 = 0xa3adbb; // light steel plate (brighter than the original dark slate)
const TRIM: u32 = 0xb58a6f; // trimColor (bronze) — hilt/buckle/throat/greave trim
const SHIELD_BASE: u32 = 0x2d2f36;
const EMBLEM: u32 = 0xdbac42; // gold lion
const GLOW: u32 = 0xffecb3; // eye orbs
const SKIRT: u32 = 0x5a2b2c; // cloth: tassets, cape, arm/leg under-layers, shoulder sphere
const BLADE: u32 = 0xc0c6d0; // default iron blade
const HILT: u32 = 0x3a3a40;
const GRIP: u32 = 0x4e3b31;
const BELT: u32 = 0x3d2b20; // belt + pouch leather
const DARK: u32 = 0x121213; // visor slit + breath holes
const CORE: u32 = 0x3d3d3d; // sword ridge groove
const GOLD: u32 = 0xe8b84b; // golden blade gilding
const AXE_STEEL: u32 = 0xaab0bc;
const STONE: u32 = 0x8a8d92;
const FROST: u32 = 0xaad2f0;

/// Tip of the held weapon in **weapon-local** space (the sword cone's point), read by
/// `combat::hero_blade_trail` off the [`super::HeroWeapon`] global transform.
pub const WEAPON_TIP_LOCAL: Vec3 = Vec3::new(0.0, -0.90, 0.0);

// ── Mesh helpers (the orks/critters contract: primitives → tint → merge → flat-shade) ──
fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
/// Map a part colour to its surface family so the shader textures it appropriately — plate/blade as
/// brushed metal, the belt/grip/cloth as fabric/leather, eyes as a soft skin band. The code rides
/// in the vertex-colour alpha *per part*, so it survives the merge (one joint mesh, many surfaces).
fn surf_for(c: u32) -> Surf {
    match c {
        SKIRT | BELT | GRIP => Surf::Cloth,
        GLOW => Surf::Skin,
        _ => Surf::Metal,
    }
}
fn tinted(mut m: Mesh, c: u32) -> Mesh {
    let n = m.count_vertices();
    let mut col = lin(c);
    col[3] = surf_code(surf_for(c)); // surface family in alpha (the shader's per-surface texture)
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![col; n]);
    m
}
/// Merge a joint's parts into ONE flat-shaded, vertex-coloured mesh (duplicate FIRST — the flat
/// normals pass panics on an indexed mesh). Per-part surfaces are already baked into the alpha.
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn cuboid(w: f32, h: f32, d: f32) -> Mesh {
    Cuboid::new(w, h, d).mesh().build()
}
/// Tapered cylinder = `ConicalFrustum` (`rt==rb` → plain cylinder, `rt==0` → cone). Args mirror
/// three.js `CylinderGeometry(radiusTop, radiusBottom, height, radialSegments)`.
fn frustum(rt: f32, rb: f32, h: f32, res: u32) -> Mesh {
    ConicalFrustum { radius_top: rt, radius_bottom: rb, height: h }.mesh().resolution(res).build()
}
fn cone(r: f32, h: f32, res: u32) -> Mesh {
    Cone { radius: r, height: h }.mesh().resolution(res).build()
}
fn ball(r: f32) -> Mesh {
    Sphere::new(r).mesh().ico(1).unwrap()
}
/// Place a primitive: scale → rotate → translate (matching three.js' `T*R*S`), then tint.
fn part(mut m: Mesh, scale: Vec3, rot: Quat, off: Vec3, c: u32) -> Mesh {
    if scale != Vec3::ONE {
        m = m.scaled_by(scale);
    }
    if rot != Quat::IDENTITY {
        m = m.rotated_by(rot);
    }
    tinted(m.translated_by(off), c)
}
fn at(m: Mesh, off: Vec3, c: u32) -> Mesh {
    part(m, Vec3::ONE, Quat::IDENTITY, off, c)
}

// ── Equipped-armor tint ───────────────────────────────────────────────────────────────
/// Lerp an sRGB-hex colour `t` of the way toward `target` (byte space — the "feels the same" bar).
fn lerp_hex(c: u32, target: u32, t: f32) -> u32 {
    let ch = |x: u32, s: u32| ((x >> s) & 0xff) as f32;
    let mix = |a: f32, b: f32| (a + (b - a) * t).round().clamp(0.0, 255.0) as u32;
    (mix(ch(c, 16), ch(target, 16)) << 16) | (mix(ch(c, 8), ch(target, 8)) << 8) | mix(ch(c, 0), ch(target, 0))
}

/// The plate colour triple `(base, light, dark)` for the worn armor — `None` (bare) is the default
/// slate plate; a tier lerps light→white / dark→black for facet depth.
fn armor_palette(armor: Option<&str>) -> (u32, u32, u32) {
    let tint = match armor {
        Some("leather_armor") => 0x7a5230,
        Some("iron_armor") => 0xaeb4c0,
        Some("gold_armor") => 0xe8b84b,
        Some("dragon_plate") => 0x3a6a4a,
        _ => ARMOR,
    };
    (tint, lerp_hex(tint, 0xffffff, 0.25), lerp_hex(tint, 0x000000, 0.28))
}

// ── Held weapon (kept from the parity port: iron/axe/maul/gold/frost) ─────────────────
fn sword_parts(blade: u32, pommel: u32) -> Vec<Mesh> {
    let fuller = lerp_hex(blade, 0x000000, 0.35);
    vec![
        at(ball(0.05), v(0.0, 0.14, 0.0), pommel),
        at(frustum(0.03, 0.03, 0.14, 6), v(0.0, 0.06, 0.0), GRIP),
        at(cuboid(0.30, 0.06, 0.08), v(0.0, -0.04, 0.0), pommel),
        at(ball(0.035), v(-0.15, -0.04, 0.0), pommel),
        at(ball(0.035), v(0.15, -0.04, 0.0), pommel),
        at(cuboid(0.09, 0.80, 0.03), v(0.0, -0.47, 0.0), blade),
        at(cuboid(0.03, 0.64, 0.034), v(0.0, -0.42, 0.0), fuller),
        part(cone(0.05, 0.12, 6), Vec3::ONE, rx(PI), v(0.0, -0.90, 0.0), blade),
    ]
}

/// The held-weapon mesh for the equipped item, in weapon-local space (an unknown id → iron sword).
fn weapon_parts(weapon: Option<&str>) -> Vec<Mesh> {
    match weapon {
        Some("axe") => vec![
            at(frustum(0.028, 0.028, 0.8, 6), v(0.0, -0.12, 0.0), GRIP),
            at(frustum(0.034, 0.034, 0.05, 6), v(0.0, -0.27, 0.0), HILT),
            at(frustum(0.034, 0.034, 0.05, 6), v(0.0, 0.12, 0.0), HILT),
            at(ball(0.04), v(0.0, 0.3, 0.0), HILT),
            at(cuboid(0.26, 0.22, 0.05), v(0.13, -0.42, 0.0), AXE_STEEL),
            part(cone(0.11, 0.14, 6), Vec3::ONE, rz(-PI / 2.0), v(0.28, -0.42, 0.0), AXE_STEEL),
            part(cone(0.05, 0.10, 6), Vec3::ONE, rz(PI / 2.0), v(-0.04, -0.42, 0.0), AXE_STEEL),
        ],
        Some("sword_gold") => sword_parts(GOLD, GOLD),
        Some("blade_frost") => sword_parts(FROST, FROST),
        Some("stone_maul") => vec![
            at(frustum(0.035, 0.035, 0.95, 6), v(0.0, -0.1, 0.0), GRIP),
            at(frustum(0.042, 0.042, 0.06, 6), v(0.0, -0.32, 0.0), HILT),
            at(ball(0.045), v(0.0, 0.36, 0.0), HILT),
            at(cuboid(0.34, 0.26, 0.26), v(0.0, -0.6, 0.0), STONE),
            at(cuboid(0.36, 0.04, 0.27), v(0.0, -0.52, 0.0), HILT),
            at(cuboid(0.36, 0.04, 0.27), v(0.0, -0.68, 0.0), HILT),
            at(cuboid(0.06, 0.2, 0.2), v(0.19, -0.6, 0.0), STONE),
            at(cuboid(0.06, 0.2, 0.2), v(-0.19, -0.6, 0.0), STONE),
        ],
        _ => sword_parts(BLADE, HILT),
    }
}

// ── Per-joint geometry (each in that joint's LOCAL space; `a/al/ad` = worn-armor triple) ──
fn hips_mesh(a: u32) -> Mesh {
    group(vec![
        at(frustum(0.19, 0.15, 0.15, 6), v(0.0, -0.04, 0.0), a),
        at(frustum(0.24, 0.22, 0.11, 6), v(0.0, 0.04, 0.0), BELT),
        at(cuboid(0.08, 0.06, 0.03), v(0.0, 0.04, 0.23), TRIM),
        part(cuboid(0.08, 0.12, 0.06), Vec3::ONE, xyz(0.1, 0.0, -0.1), v(0.2, 0.01, 0.05), BELT),
        part(cuboid(0.15, 0.2, 0.04), Vec3::ONE, xyz(0.1, 0.0, -0.15), v(0.12, -0.1, 0.05), SKIRT),
        part(cuboid(0.15, 0.2, 0.04), Vec3::ONE, xyz(0.1, 0.0, 0.15), v(-0.12, -0.1, 0.05), SKIRT),
        part(cuboid(0.28, 0.35, 0.03), Vec3::ONE, rx(-0.1), v(0.0, -0.15, -0.14), SKIRT),
    ])
}

/// A natural human breastplate (chest tapering to the waist).
fn torso_mesh(a: u32, al: u32) -> Mesh {
    group(vec![
        part(frustum(0.26, 0.17, 0.46, 6), v(1.15, 1.0, 0.75), Quat::IDENTITY, v(0.0, 0.13, 0.0), a),
        part(frustum(0.18, 0.22, 0.06, 6), v(1.15, 1.0, 0.85), Quat::IDENTITY, v(0.0, 0.33, 0.0), TRIM),
        at(cuboid(0.35, 0.03, 0.06), v(0.0, 0.2, 0.185), TRIM),
        at(cuboid(0.30, 0.03, 0.06), v(0.0, 0.1, 0.175), TRIM),
        // a subtle lit breastplate ridge so the worn-tier highlight reads
        at(cuboid(0.05, 0.30, 0.012), v(0.0, 0.16, 0.205), al),
    ])
}

fn neck_mesh(a: u32) -> Mesh {
    group(vec![
        at(frustum(0.12, 0.14, 0.1, 8), v(0.0, -0.04, 0.0), a),
        at(cuboid(0.16, 0.1, 0.14), v(0.0, -0.01, 0.06), TRIM),
    ])
}

/// Simplified, human-sized helm: a clean rounded bowl + dome cap, one eye slit with glowing eyes
/// and a single bronze brow band. (No visor band / vertical trim / breath holes — kept clean, but
/// at a natural head-to-body ratio rather than an oversized stylized skull.)
fn head_mesh(a: u32, al: u32) -> Mesh {
    group(vec![
        at(frustum(0.15, 0.16, 0.22, 8), v(0.0, 0.05, 0.0), a), // bowl
        at(ball(0.155), v(0.0, 0.16, 0.0), al),                 // dome cap
        at(cuboid(0.22, 0.025, 0.025), v(0.0, 0.075, 0.155), DARK), // eye slit
        at(cuboid(0.24, 0.02, 0.018), v(0.0, 0.12, 0.16), TRIM), // brow band
        at(cuboid(0.035, 0.018, 0.012), v(-0.05, 0.075, 0.16), GLOW), // eye orbs
        at(cuboid(0.035, 0.018, 0.012), v(0.05, 0.075, 0.16), GLOW),
    ])
}

/// Natural pauldron + bicep — human shoulder width.
fn shoulder_mesh(sign: f32, a: u32) -> Mesh {
    group(vec![
        at(ball(0.08), v(0.0, 0.0, 0.0), SKIRT),
        part(ball(0.16), v(1.0, 0.8, 1.0), rz(sign * PI / 8.0), v(0.0, 0.05, 0.0), a),
        part(frustum(0.165, 0.165, 0.04, 6), Vec3::ONE, rz(sign * PI / 8.0), v(0.0, 0.01, 0.0), TRIM),
        part(cuboid(0.15, 0.1, 0.15), Vec3::ONE, rz(sign * PI / 12.0), v(sign * 0.03, -0.05, 0.0), a),
        at(cuboid(0.04, 0.03, 0.16), v(0.0, 0.08, 0.0), TRIM),
        at(frustum(0.12, 0.095, 0.22, 5), v(0.0, -0.14, 0.0), a),
        at(frustum(0.08, 0.08, 0.26, 4), v(0.0, -0.14, 0.0), SKIRT),
    ])
}

fn elbow_mesh(a: u32) -> Mesh {
    group(vec![
        part(cuboid(0.11, 0.11, 0.11), Vec3::ONE, xyz(0.7, 0.7, 0.2), Vec3::ZERO, TRIM),
        at(frustum(0.105, 0.12, 0.24, 6), v(0.0, -0.12, 0.0), a),
        at(frustum(0.11, 0.125, 0.04, 6), v(0.0, -0.16, 0.0), TRIM),
        at(cuboid(0.09, 0.09, 0.10), v(0.0, -0.24, 0.0), a),
    ])
}

fn hip_mesh(a: u32) -> Mesh {
    group(vec![
        at(frustum(0.10, 0.085, 0.36, 4), v(0.0, -0.18, 0.0), SKIRT),
        at(frustum(0.15, 0.115, 0.32, 6), v(0.0, -0.18, 0.0), a),
        at(frustum(0.155, 0.135, 0.04, 6), v(0.0, -0.04, 0.0), TRIM),
    ])
}

fn knee_mesh(a: u32) -> Mesh {
    group(vec![
        part(cuboid(0.12, 0.12, 0.12), Vec3::ONE, xyz(0.7, 0.0, 0.2), Vec3::ZERO, TRIM),
        part(frustum(0.105, 0.13, 0.35, 6), v(1.1, 1.0, 0.9), Quat::IDENTITY, v(0.0, -0.18, 0.0), a),
        part(frustum(0.11, 0.135, 0.03, 6), v(1.1, 1.0, 0.9), Quat::IDENTITY, v(0.0, -0.06, 0.0), TRIM),
        part(frustum(0.13, 0.132, 0.03, 6), v(1.105, 1.0, 0.905), Quat::IDENTITY, v(0.0, -0.32, 0.0), TRIM),
    ])
}

/// A proper boot: leather ankle cuff + upper, a dark sole, and a steel toe cap (reads as a real
/// shoe rather than the old cone-toe). Textured by colour (BELT → leather/cloth, sole/toe metal).
fn foot_mesh(a: u32) -> Mesh {
    group(vec![
        at(frustum(0.085, 0.075, 0.10, 6), v(0.0, 0.015, -0.01), BELT), // ankle cuff
        at(cuboid(0.10, 0.09, 0.17), v(0.0, -0.035, 0.03), BELT),       // leather upper
        at(cuboid(0.11, 0.025, 0.21), v(0.0, -0.07, 0.04), HILT),       // sole
        at(cuboid(0.095, 0.05, 0.06), v(0.0, -0.055, 0.12), a),         // steel toe cap
        at(cuboid(0.07, 0.05, 0.06), v(0.0, -0.05, -0.07), HILT),       // heel
    ])
}

fn shield_mesh() -> Mesh {
    group(vec![
        at(cuboid(0.52, 0.6, 0.05), v(0.0, 0.08, 0.0), SHIELD_BASE),
        part(cuboid(0.36, 0.36, 0.05), Vec3::ONE, rz(PI / 4.0), v(0.0, -0.28, 0.0), SHIELD_BASE),
        at(cuboid(0.58, 0.66, 0.03), v(0.0, 0.06, -0.012), TRIM),
        part(cuboid(0.40, 0.40, 0.03), Vec3::ONE, rz(PI / 4.0), v(0.0, -0.30, -0.012), TRIM),
    ])
}

fn lion_mesh() -> Mesh {
    let b: [([f32; 3], [f32; 3], f32); 17] = [
        ([0.10, 0.06, 0.012], [-0.02, -0.05, 0.0], PI / 4.0),
        ([0.08, 0.14, 0.012], [0.01, 0.02, 0.0], 0.5),
        ([0.11, 0.09, 0.015], [0.04, 0.08, 0.002], 0.2),
        ([0.06, 0.06, 0.018], [0.06, 0.14, 0.003], -0.1),
        ([0.04, 0.025, 0.014], [0.09, 0.15, 0.002], 0.0),
        ([0.03, 0.012, 0.014], [0.09, 0.12, 0.002], -0.3),
        ([0.03, 0.08, 0.012], [0.08, 0.05, 0.002], -0.9),
        ([0.035, 0.035, 0.014], [0.12, 0.08, 0.002], 0.4),
        ([0.024, 0.08, 0.012], [0.06, -0.02, 0.001], -1.4),
        ([0.03, 0.03, 0.014], [0.10, -0.03, 0.001], 0.0),
        ([0.035, 0.08, 0.012], [-0.06, -0.10, 0.002], 0.5),
        ([0.04, 0.024, 0.014], [-0.09, -0.14, 0.002], 0.0),
        ([0.03, 0.07, 0.012], [-0.01, -0.11, 0.001], -0.7),
        ([0.035, 0.02, 0.014], [0.02, -0.14, 0.001], 0.0),
        ([0.08, 0.02, 0.012], [-0.08, -0.04, 0.001], -0.8),
        ([0.06, 0.018, 0.012], [-0.11, 0.01, 0.001], 0.8),
        ([0.03, 0.03, 0.014], [-0.1, 0.05, 0.001], 0.4),
    ];
    group(b.iter().map(|(s, p, r)| part(cuboid(s[0], s[1], s[2]), Vec3::ONE, rz(*r), v(p[0], p[1], p[2]), EMBLEM)).collect())
}

// ── The full build (one mesh per joint + the held weapon) ─────────────────────────────
/// Every joint mesh + the held-weapon mesh & its hand-local transform. [`super::spawn_hero_meshes`]
/// spawns the joint hierarchy from this; an equip change rebuilds it (`super::reskin_hero`).
pub struct KnightMeshes {
    pub hips: Mesh,
    pub torso: Mesh,
    pub neck: Mesh,
    pub head: Mesh,
    pub shoulder_l: Mesh,
    pub shoulder_r: Mesh,
    pub elbow_l: Mesh,
    pub elbow_r: Mesh,
    pub hip_l: Mesh,
    pub hip_r: Mesh,
    pub knee_l: Mesh,
    pub knee_r: Mesh,
    pub foot_l: Mesh,
    pub foot_r: Mesh,
    pub shield: Mesh,
    pub lion: Mesh,
    pub weapon: Mesh,
    pub weapon_xf: Transform,
}

/// Build all knight meshes reflecting the equipped gear: the held weapon swaps geometry and the
/// worn armor recolours the plate (bare = default slate). Re-called by `super::reskin_hero`.
pub fn build_knight(weapon: Option<&str>, armor: Option<&str>) -> KnightMeshes {
    let (a, al, _ad) = armor_palette(armor);
    KnightMeshes {
        hips: hips_mesh(a),
        torso: torso_mesh(a, al),
        neck: neck_mesh(a),
        head: head_mesh(a, al),
        shoulder_l: shoulder_mesh(-1.0, a),
        shoulder_r: shoulder_mesh(1.0, a),
        elbow_l: elbow_mesh(a),
        elbow_r: elbow_mesh(a),
        hip_l: hip_mesh(a),
        hip_r: hip_mesh(a),
        knee_l: knee_mesh(a),
        knee_r: knee_mesh(a),
        foot_l: foot_mesh(a),
        foot_r: foot_mesh(a),
        shield: shield_mesh(),
        lion: lion_mesh(),
        weapon: group(weapon_parts(weapon)),
        // Mount at the right hand: blade rotated forward (+Z) and dropped into the grip.
        weapon_xf: Transform { translation: v(0.0, -0.04, 0.04), rotation: rx(-PI / 2.0), ..default() },
    }
}
