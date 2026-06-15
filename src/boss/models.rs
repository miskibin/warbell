//! **Biome-Warden boss models** — one distinct procedural creature per biome, built on the same
//! flat-shaded vertex-colour contract as `critters.rs` / `orks.rs` (build primitives → `tinted`
//! each → `merge` → `duplicate_vertices` + `compute_flat_normals`), against the shared white
//! `CreatureMaterial`. Bosses are humanoid hierarchies: a static **torso** plus articulated
//! **parts** (2 legs, 2 arms, a head) keyed by [`PartKind`] so `boss::boss_limbs` can sway them.
//! Built large (the spawn applies a ~2× root scale) so a warden towers over the warbands.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::Biome;
use crate::creature::{surf, Surf};
use crate::critters::PartKind;
use crate::palette::lin;

/// One articulated boss part (pivot local to the root; mesh built in part-local space).
pub struct BossPart {
    pub kind: PartKind,
    pub pivot: Vec3,
    pub mesh: Mesh,
}

/// A built boss: the static torso + its articulated parts.
pub struct BossSpec {
    pub torso: Mesh,
    pub parts: Vec<BossPart>,
}

// ── Mesh helpers (local copies of the props/critters contract) ───────────────────────
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("boss parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), lin(c))
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), lin(c))
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), lin(c))
}
fn orb(r: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Sphere::new(r).mesh().ico(2).unwrap().translated_by(off), lin(c))
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

// ── Dispatch ─────────────────────────────────────────────────────────────────────────

/// Build the warden model for `biome`, surface-tagged so the shader textures it.
pub fn build(biome: Biome) -> BossSpec {
    let (spec, sf) = match biome {
        Biome::Forest => (treant(), Surf::Skin),
        Biome::Snow => (balwan(), Surf::Skin),
        Biome::Rocky => (golem(), Surf::Stone),
        Biome::Desert => (revenant(), Surf::Cloth),
        Biome::Swamp => (hag(), Surf::Scale),
    };
    BossSpec {
        torso: surf(spec.torso, sf),
        parts: spec.parts.into_iter().map(|p| BossPart { mesh: surf(p.mesh, sf), ..p }).collect(),
    }
}

/// The spawn-time root scale for each warden (multiplies the in-mesh ~1.7u height → a towering boss).
pub fn root_scale(biome: Biome) -> f32 {
    match biome {
        Biome::Rocky => 1.47, // the golem is the bulkiest (30% smaller)
        Biome::Snow => 1.4,
        _ => 1.33,
    }
}

/// Display name shown in the reward dialog + boss health bar.
pub fn name(biome: Biome) -> &'static str {
    match biome {
        Biome::Forest => "Old Bramblewood, the Treant",
        Biome::Snow => "Bałwan, the Frostgiant",
        Biome::Rocky => "Karngor, the Stone Golem",
        Biome::Desert => "The Sand Revenant",
        Biome::Swamp => "Mabworm, the Bog Hag",
    }
}

// ── Forest: Treant ("wood man") ────────────────────────────────────────────────────────
fn treant() -> BossSpec {
    const BARK: u32 = 0x5a4232;
    const DARK: u32 = 0x3e2c1f;
    const WOOD: u32 = 0x7a6347;
    const LEAF: u32 = 0x3f7a2c;
    const LEAF2: u32 = 0x568f38;
    const EYE: u32 = 0xc8e85a;
    let torso = group(vec![
        bx(0.7, 1.5, 0.55, v(0.0, 1.05, 0.0), BARK),
        bx(0.5, 1.4, 0.2, v(0.0, 1.05, 0.24), WOOD), // pale heartwood face plate
        bx(0.16, 1.2, 0.08, v(-0.22, 1.0, 0.22), DARK), // bark seams
        bx(0.16, 1.2, 0.08, v(0.2, 1.0, 0.22), DARK),
        bxr(0.3, 0.7, 0.25, v(-0.42, 1.7, -0.04), rz(0.5), BARK), // shoulder knot
        bxr(0.3, 0.7, 0.25, v(0.42, 1.7, -0.04), rz(-0.5), BARK),
        bx(0.2, 0.18, 0.1, v(-0.12, 1.5, 0.27), LEAF), // moss tufts on the trunk
        bx(0.18, 0.16, 0.1, v(0.16, 1.2, 0.27), LEAF2),
    ]);
    let head = group(vec![
        bx(0.5, 0.46, 0.46, v(0.0, 0.0, 0.0), BARK),
        bx(0.4, 0.1, 0.3, v(0.0, 0.13, 0.18), DARK), // heavy brow
        bx(0.1, 0.1, 0.04, v(-0.13, 0.0, 0.22), EYE), // glowing eyes
        bx(0.1, 0.1, 0.04, v(0.13, 0.0, 0.22), EYE),
        bx(0.34, 0.06, 0.05, v(0.0, -0.16, 0.2), DARK), // mouth gash
        // leaf crown
        cone(0.34, 0.5, v(0.0, 0.42, 0.0), Quat::IDENTITY, LEAF),
        cone(0.22, 0.36, v(-0.28, 0.36, 0.0), rz(0.5), LEAF2),
        cone(0.22, 0.36, v(0.28, 0.36, 0.0), rz(-0.5), LEAF2),
        cone(0.2, 0.34, v(0.0, 0.34, 0.28), rx(-0.5), LEAF),
    ]);
    // Branch arms: a long limb that forks into twig fingers at the tip.
    let arm = || {
        group(vec![
            bxr(0.2, 1.0, 0.2, v(0.0, -0.45, 0.0), xyz(0.1, 0.0, 0.0), BARK),
            cone(0.06, 0.3, v(-0.1, -0.95, 0.04), rx(0.3), WOOD), // twig fingers
            cone(0.06, 0.34, v(0.0, -0.98, 0.06), rx(0.2), WOOD),
            cone(0.06, 0.3, v(0.1, -0.95, 0.04), rx(0.3), WOOD),
            bx(0.16, 0.14, 0.1, v(-0.06, -0.5, 0.12), LEAF), // mossy elbow
        ])
    };
    let leg = || {
        group(vec![
            bx(0.3, 0.7, 0.3, v(0.0, -0.35, 0.0), BARK),
            cone(0.08, 0.3, v(-0.14, -0.66, 0.16), rx(1.2), DARK), // splayed roots
            cone(0.08, 0.3, v(0.14, -0.66, 0.16), rx(1.2), DARK),
            cone(0.08, 0.3, v(0.0, -0.66, -0.18), rx(-1.2), DARK),
        ])
    };
    let parts = vec![
        BossPart { kind: PartKind::Leg(1.0), pivot: v(-0.26, 0.72, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Leg(-1.0), pivot: v(0.26, 0.72, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Arm(1.0), pivot: v(0.6, 1.78, 0.0), mesh: arm() },
        BossPart { kind: PartKind::Arm(-1.0), pivot: v(-0.6, 1.78, 0.0), mesh: arm() },
        BossPart { kind: PartKind::Head, pivot: v(0.0, 1.86, 0.06), mesh: head },
    ];
    BossSpec { torso, parts }
}

// ── Snow: Bałwan (three-stack frost giant) ─────────────────────────────────────────────
fn balwan() -> BossSpec {
    const SNOW: u32 = 0xeef4fb;
    const SHADE: u32 = 0xc3d2e4;
    const ICE: u32 = 0x9ad0ff;
    const COAL: u32 = 0x1a1a1a;
    const CARROT: u32 = 0xe8862c;
    const STICK: u32 = 0x5a3f25;
    let torso = group(vec![
        orb(0.6, v(0.0, 0.62, 0.0), SNOW),   // bottom ball
        orb(0.5, v(0.0, 1.45, 0.0), SNOW),   // middle ball
        orb(0.34, v(0.0, 0.5, 0.34), SHADE), // belly shadow
        // three coal buttons up the middle
        bx(0.1, 0.1, 0.05, v(0.0, 1.35, 0.46), COAL),
        bx(0.1, 0.1, 0.05, v(0.0, 1.55, 0.42), COAL),
        // icicle epaulettes
        cone(0.1, 0.4, v(-0.45, 1.7, 0.0), rz(2.6), ICE),
        cone(0.1, 0.4, v(0.45, 1.7, 0.0), rz(-2.6), ICE),
    ]);
    let head = group(vec![
        orb(0.36, v(0.0, 0.0, 0.0), SNOW),
        bx(0.1, 0.1, 0.05, v(-0.13, 0.08, 0.3), COAL), // coal eyes
        bx(0.1, 0.1, 0.05, v(0.13, 0.08, 0.3), COAL),
        cone(0.07, 0.34, v(0.0, -0.02, 0.34), rx(1.57), CARROT), // carrot nose
        bx(0.05, 0.05, 0.04, v(-0.16, -0.12, 0.28), COAL), // coal smile
        bx(0.05, 0.05, 0.04, v(-0.06, -0.16, 0.3), COAL),
        bx(0.05, 0.05, 0.04, v(0.06, -0.16, 0.3), COAL),
        bx(0.05, 0.05, 0.04, v(0.16, -0.12, 0.28), COAL),
        // icicle crown
        cone(0.06, 0.3, v(-0.16, 0.34, 0.0), rz(0.3), ICE),
        cone(0.07, 0.36, v(0.0, 0.36, 0.0), Quat::IDENTITY, ICE),
        cone(0.06, 0.3, v(0.16, 0.34, 0.0), rz(-0.3), ICE),
    ]);
    // Stick arms with twig hands.
    let arm = || {
        group(vec![
            bxr(0.08, 0.95, 0.08, v(0.0, -0.42, 0.0), xyz(0.0, 0.0, 0.0), STICK),
            cone(0.04, 0.22, v(-0.1, -0.86, 0.0), rz(0.5), STICK),
            cone(0.04, 0.22, v(0.0, -0.9, 0.0), Quat::IDENTITY, STICK),
            cone(0.04, 0.22, v(0.1, -0.86, 0.0), rz(-0.5), STICK),
        ])
    };
    // Stubby snow feet (it shuffles).
    let leg = || group(vec![bx(0.26, 0.34, 0.34, v(0.0, -0.16, 0.04), SNOW), bx(0.26, 0.08, 0.34, v(0.0, -0.3, 0.06), SHADE)]);
    let parts = vec![
        BossPart { kind: PartKind::Leg(1.0), pivot: v(-0.22, 0.32, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Leg(-1.0), pivot: v(0.22, 0.32, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Arm(1.0), pivot: v(0.5, 1.5, 0.0), mesh: arm() },
        BossPart { kind: PartKind::Arm(-1.0), pivot: v(-0.5, 1.5, 0.0), mesh: arm() },
        BossPart { kind: PartKind::Head, pivot: v(0.0, 1.78, 0.0), mesh: head },
    ];
    BossSpec { torso, parts }
}

// ── Rocky: Stone Golem ─────────────────────────────────────────────────────────────────
fn golem() -> BossSpec {
    const STONE: u32 = 0x82838c;
    const DARK: u32 = 0x595a62;
    const MOSS: u32 = 0x5a6a3a;
    const CORE: u32 = 0xff8a3a; // molten orange heart
    let torso = group(vec![
        bx(1.0, 1.0, 0.74, v(0.0, 1.1, 0.0), STONE),
        bxr(0.5, 0.4, 0.42, v(-0.42, 1.55, -0.06), xyz(0.2, 0.4, 0.2), STONE), // shoulder crags
        bxr(0.46, 0.36, 0.4, v(0.44, 1.52, -0.04), xyz(-0.2, 0.3, -0.25), STONE),
        bxr(0.72, 0.7, 0.24, v(0.0, 1.16, -0.4), xyz(0.15, 0.0, 0.1), DARK), // back slab
        bx(0.24, 0.16, 0.28, v(-0.48, 1.42, 0.0), MOSS), // shoulder moss
        bx(0.24, 0.16, 0.28, v(0.48, 1.42, 0.0), MOSS),
        bx(0.3, 0.34, 0.08, v(0.0, 1.1, 0.4), CORE), // molten chest core
        bx(0.05, 0.3, 0.04, v(-0.28, 0.92, 0.37), CORE), // glowing seams
        bx(0.26, 0.05, 0.04, v(0.24, 1.32, 0.37), CORE),
    ]);
    let head = group(vec![
        bx(0.58, 0.5, 0.5, v(0.0, 0.0, 0.0), STONE),
        bx(0.56, 0.1, 0.08, v(0.0, 0.16, 0.24), DARK), // brow ridge
        cone(0.1, 0.24, v(-0.18, 0.32, -0.04), rz(0.3), STONE), // crown crags
        cone(0.08, 0.18, v(0.14, 0.33, 0.02), rz(-0.25), STONE),
        bx(0.18, 0.12, 0.06, v(0.06, 0.3, -0.12), MOSS),
        bx(0.12, 0.12, 0.05, v(-0.16, 0.02, 0.25), CORE), // molten eyes
        bx(0.12, 0.12, 0.05, v(0.16, 0.02, 0.25), CORE),
        bx(0.42, 0.06, 0.05, v(0.0, -0.18, 0.25), DARK), // grim mouth
    ]);
    let arm = || {
        group(vec![
            bx(0.36, 0.84, 0.36, v(0.0, -0.42, 0.0), DARK),
            bx(0.04, 0.5, 0.04, v(-0.18, -0.4, 0.1), CORE), // cracked glow seam
            bx(0.46, 0.42, 0.46, v(0.0, -0.96, 0.03), STONE), // boulder fist
            bx(0.14, 0.12, 0.08, v(-0.1, -0.82, 0.22), DARK), // knuckles
            bx(0.14, 0.12, 0.08, v(0.1, -0.82, 0.22), DARK),
        ])
    };
    let leg = || {
        group(vec![
            bx(0.36, 0.74, 0.4, v(0.0, -0.37, 0.0), DARK),
            bx(0.42, 0.18, 0.46, v(0.0, -0.66, 0.03), STONE), // boulder foot
        ])
    };
    let parts = vec![
        BossPart { kind: PartKind::Leg(1.0), pivot: v(-0.3, 0.74, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Leg(-1.0), pivot: v(0.3, 0.74, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Arm(1.0), pivot: v(0.66, 1.6, 0.0), mesh: arm() },
        BossPart { kind: PartKind::Arm(-1.0), pivot: v(-0.66, 1.6, 0.0), mesh: arm() },
        BossPart { kind: PartKind::Head, pivot: v(0.0, 1.78, 0.06), mesh: head },
    ];
    BossSpec { torso, parts }
}

// ── Desert: Sand Revenant ───────────────────────────────────────────────────────────────
fn revenant() -> BossSpec {
    const WRAP: u32 = 0xcdb486; // sun-bleached wraps
    const DARK: u32 = 0x9c8559;
    const BONE: u32 = 0xe8dcc0;
    const SHADOW: u32 = 0x5a4a30;
    const EYE: u32 = 0x46c8d8; // pale spectral cyan
    let torso = group(vec![
        bx(0.6, 1.2, 0.4, v(0.0, 1.1, 0.0), WRAP),
        bx(0.5, 1.0, 0.18, v(0.0, 1.05, 0.2), DARK), // wrap shadow
        // ribcage hints through the wraps
        bx(0.5, 0.04, 0.04, v(0.0, 1.35, 0.21), BONE),
        bx(0.5, 0.04, 0.04, v(0.0, 1.2, 0.22), BONE),
        bx(0.5, 0.04, 0.04, v(0.0, 1.05, 0.21), BONE),
        bxr(0.4, 0.5, 0.3, v(-0.34, 1.62, -0.04), rz(0.4), WRAP), // shoulders
        bxr(0.4, 0.5, 0.3, v(0.34, 1.62, -0.04), rz(-0.4), WRAP),
        // tattered hem (tapers to a wraith tail instead of feet)
        bxr(0.3, 0.6, 0.2, v(-0.14, 0.4, 0.0), rz(0.2), DARK),
        bxr(0.3, 0.7, 0.2, v(0.14, 0.32, 0.0), rz(-0.15), WRAP),
        cone(0.18, 0.5, v(0.0, 0.1, 0.0), rx(3.14159), DARK), // wisp tail tip
    ]);
    let head = group(vec![
        bx(0.4, 0.42, 0.4, v(0.0, 0.0, 0.0), BONE), // skull
        bx(0.26, 0.16, 0.2, v(0.0, -0.16, 0.16), BONE), // jaw
        bx(0.12, 0.14, 0.06, v(-0.12, 0.02, 0.2), SHADOW), // eye sockets
        bx(0.12, 0.14, 0.06, v(0.12, 0.02, 0.2), SHADOW),
        bx(0.07, 0.08, 0.04, v(-0.12, 0.02, 0.22), EYE), // glowing pinpoints
        bx(0.07, 0.08, 0.04, v(0.12, 0.02, 0.22), EYE),
        bx(0.06, 0.1, 0.05, v(0.0, -0.06, 0.22), SHADOW), // nasal cavity
        // ragged hood
        bxr(0.48, 0.3, 0.4, v(0.0, 0.22, -0.06), rx(0.2), DARK),
        cone(0.1, 0.3, v(0.0, 0.36, -0.1), rx(-0.3), DARK),
    ]);
    // Skeletal arms ending in a curved scimitar of bone.
    let arm_blade = || {
        group(vec![
            bxr(0.12, 0.9, 0.12, v(0.0, -0.4, 0.0), Quat::IDENTITY, WRAP),
            bx(0.1, 0.3, 0.1, v(0.0, -0.85, 0.0), BONE), // forearm bone
            bxr(0.06, 0.7, 0.14, v(0.0, -1.2, 0.18), rx(0.4), BONE), // bone blade
        ])
    };
    let arm = || {
        group(vec![
            bxr(0.12, 0.9, 0.12, v(0.0, -0.4, 0.0), Quat::IDENTITY, WRAP),
            bx(0.1, 0.3, 0.1, v(0.0, -0.85, 0.0), BONE),
            cone(0.05, 0.2, v(-0.06, -1.05, 0.0), rz(0.4), BONE), // bony claw
            cone(0.05, 0.2, v(0.06, -1.05, 0.0), rz(-0.4), BONE),
        ])
    };
    // No real legs — wraith floats; give two short hovering wisp stubs for the limb rig.
    let leg = || group(vec![cone(0.16, 0.4, v(0.0, -0.2, 0.0), rx(3.14159), DARK)]);
    let parts = vec![
        BossPart { kind: PartKind::Leg(1.0), pivot: v(-0.18, 0.5, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Leg(-1.0), pivot: v(0.18, 0.5, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Arm(1.0), pivot: v(0.5, 1.7, 0.0), mesh: arm_blade() },
        BossPart { kind: PartKind::Arm(-1.0), pivot: v(-0.5, 1.7, 0.0), mesh: arm() },
        BossPart { kind: PartKind::Head, pivot: v(0.0, 1.82, 0.04), mesh: head },
    ];
    BossSpec { torso, parts }
}

// ── Swamp: Bog Hag ──────────────────────────────────────────────────────────────────────
fn hag() -> BossSpec {
    const HIDE: u32 = 0x4f6a3a;
    const DARK: u32 = 0x35471f;
    const BELLY: u32 = 0x8a9a52;
    const TOOTH: u32 = 0xd8d0b0;
    const WART: u32 = 0x6a7a40;
    const EYE: u32 = 0xd8b020;
    let torso = group(vec![
        // hunched, pot-bellied
        bx(0.9, 0.9, 0.7, v(0.0, 0.95, 0.0), HIDE),
        bx(0.7, 0.6, 0.3, v(0.0, 0.82, 0.34), BELLY), // sagging belly
        bxr(0.7, 0.6, 0.4, v(0.0, 1.42, -0.18), rx(0.5), HIDE), // hunched upper back
        bx(0.16, 0.14, 0.14, v(-0.3, 1.3, -0.3), WART), // warty lumps
        bx(0.18, 0.16, 0.16, v(0.28, 1.1, -0.28), WART),
        bx(0.14, 0.12, 0.12, v(0.34, 0.7, 0.2), WART),
        // moss drape over the shoulders
        bx(0.6, 0.1, 0.5, v(0.0, 1.55, -0.1), DARK),
    ]);
    let head = group(vec![
        bx(0.56, 0.44, 0.5, v(0.0, 0.0, 0.0), HIDE),
        bx(0.5, 0.2, 0.34, v(0.0, -0.2, 0.18), BELLY), // big jaw
        // jagged teeth
        cone(0.05, 0.16, v(-0.16, -0.14, 0.28), rx(3.14159), TOOTH),
        cone(0.05, 0.18, v(-0.04, -0.16, 0.3), rx(3.14159), TOOTH),
        cone(0.05, 0.16, v(0.08, -0.14, 0.28), rx(3.14159), TOOTH),
        cone(0.05, 0.18, v(0.2, -0.15, 0.27), rx(3.14159), TOOTH),
        bx(0.14, 0.04, 0.04, v(-0.18, 0.02, 0.26), EYE), // narrow eyes
        bx(0.14, 0.04, 0.04, v(0.18, 0.02, 0.26), EYE),
        bx(0.36, 0.12, 0.1, v(0.0, 0.16, 0.18), DARK), // heavy brow
        cone(0.1, 0.3, v(0.0, -0.12, 0.36), rx(1.4), WART), // hooked nose
        // wild hair / weeds
        cone(0.05, 0.4, v(-0.2, 0.3, -0.1), rz(0.6), DARK),
        cone(0.05, 0.44, v(0.0, 0.34, -0.12), Quat::IDENTITY, DARK),
        cone(0.05, 0.4, v(0.2, 0.3, -0.1), rz(-0.6), DARK),
    ]);
    // Long gangly arms, taloned.
    let arm = || {
        group(vec![
            bxr(0.16, 1.05, 0.16, v(0.0, -0.48, 0.0), xyz(0.1, 0.0, 0.0), HIDE),
            bx(0.14, 0.18, 0.14, v(0.0, -1.0, 0.0), BELLY), // gnarled hand
            cone(0.04, 0.24, v(-0.08, -1.12, 0.06), rx(0.4), TOOTH), // talons
            cone(0.04, 0.26, v(0.0, -1.14, 0.06), rx(0.3), TOOTH),
            cone(0.04, 0.24, v(0.08, -1.12, 0.06), rx(0.4), TOOTH),
        ])
    };
    let leg = || {
        group(vec![
            bx(0.28, 0.5, 0.3, v(0.0, -0.25, 0.0), HIDE),
            bx(0.3, 0.1, 0.4, v(0.0, -0.48, 0.08), DARK), // splayed webbed foot
            cone(0.04, 0.14, v(-0.1, -0.5, 0.26), rx(1.4), TOOTH), // claws
            cone(0.04, 0.14, v(0.1, -0.5, 0.26), rx(1.4), TOOTH),
        ])
    };
    let parts = vec![
        BossPart { kind: PartKind::Leg(1.0), pivot: v(-0.26, 0.5, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Leg(-1.0), pivot: v(0.26, 0.5, 0.0), mesh: leg() },
        BossPart { kind: PartKind::Arm(1.0), pivot: v(0.56, 1.5, 0.06), mesh: arm() },
        BossPart { kind: PartKind::Arm(-1.0), pivot: v(-0.56, 1.5, 0.06), mesh: arm() },
        BossPart { kind: PartKind::Head, pivot: v(0.0, 1.62, 0.1), mesh: head },
    ];
    BossSpec { torso, parts }
}
