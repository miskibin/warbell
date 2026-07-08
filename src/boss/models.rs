//! **Biome-Warden boss models** — one distinct procedural creature per biome, built on the same
//! flat-shaded vertex-colour contract as `critters.rs` / `orks.rs` (build primitives → `tinted`
//! each → `merge` → `duplicate_vertices` + `compute_flat_normals`), against the shared white
//! `CreatureMaterial`. Bosses are humanoid hierarchies: a static **torso** plus articulated
//! **parts** (2 legs, 2 arms, a head) keyed by [`PartKind`] so `boss::boss_limbs` can sway them.
//! Built large (the spawn applies a ~2× root scale) so a warden towers over the warbands.
//!
//! 2026-07 polish pass: the first wardens were raw `Cuboid` stacks ("kanciaste kupy"). They're now
//! sculpted from the same richer primitive set the knight uses — [`lathe`] bodies-of-revolution,
//! tapered [`frustum`] limbs, chamfered [`slab`] plates and squashed [`blob`] spheres — so each
//! reads as a deliberate low-poly creature, not a box pile. Pivots, part kinds and overall
//! dimensions are UNCHANGED (the limb sway rig, hitboxes and root scales all still apply).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::MeshBuilder;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::biome::Biome;
use crate::creature::{surf, Surf};
use crate::critters::PartKind;
use crate::palette::lin;
use crate::meshkit::tinted;

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

// ── Mesh helpers (the knight-model contract: raw primitive → place → tint → merge) ─────
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
/// Place a raw primitive: scale → rotate → translate, then tint (mirrors the knight's `part`).
fn put(mut m: Mesh, scale: Vec3, rot: Quat, off: Vec3, c: u32) -> Mesh {
    if scale != Vec3::ONE {
        m = m.scaled_by(scale);
    }
    if rot != Quat::IDENTITY {
        m = m.rotated_by(rot);
    }
    tinted(m.translated_by(off), lin(c))
}
/// A squashed ico-sphere — the organic workhorse (bellies, knots, moss pads, skulls).
fn blob(r: f32, scale: Vec3, off: Vec3, c: u32) -> Mesh {
    put(Sphere::new(r).mesh().ico(2).unwrap(), scale, Quat::IDENTITY, off, c)
}
fn orb(r: f32, off: Vec3, c: u32) -> Mesh {
    blob(r, Vec3::ONE, off, c)
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    put(Cone { radius: r, height: h }.mesh().build(), Vec3::ONE, rot, off, c)
}
/// Tapered cylinder (limbs, trunks, horns) — origin-centred like the Bevy primitive.
fn frustum(rt: f32, rb: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    put(
        ConicalFrustum { radius_top: rt, radius_bottom: rb, height: h }.mesh().resolution(8).build(),
        Vec3::ONE,
        rot,
        off,
        c,
    )
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

/// Body of revolution from a `[radius, y]` profile (start at the axis on top, walk down, end at
/// the axis) — trunks, shrouds, domes. Same auto-outward triangulation as the knight's `lathe`.
fn lathe(profile: &[[f32; 2]], segs: u32) -> Mesh {
    let n = profile.len() as u32;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    for s in 0..=segs {
        let th = (s as f32 / segs as f32) * std::f32::consts::TAU;
        let (st, ct) = (th.sin(), th.cos());
        for p in profile {
            pos.push([p[0] * ct, p[1], p[0] * st]);
        }
    }
    let mut raw: Vec<[u32; 3]> = Vec::new();
    for s in 0..segs {
        for i in 0..n - 1 {
            let (a, b, c, d) = (s * n + i, (s + 1) * n + i, (s + 1) * n + i + 1, s * n + i + 1);
            raw.push([a, b, d]);
            raw.push([b, c, d]);
        }
    }
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx = Vec::new();
    for t in raw {
        let (va, vb, vc) = (g(t[0]), g(t[1]), g(t[2]));
        let nrm = (vb - va).cross(vc - va);
        let ctr = (va + vb + vc) / 3.0;
        if nrm.dot(Vec3::new(ctr.x, 0.0, ctr.z)) >= 0.0 {
            idx.extend(t);
        } else {
            idx.extend([t[0], t[2], t[1]]);
        }
    }
    let nn = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; nn]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; nn]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}

/// A chamfered, optionally tapered box, origin-CENTRED (the knight's `tplate` re-based): `top_w`/
/// `top_d` scale the +Y face, `e` is the bevel inset. Every "box" on a warden goes through this so
/// edges catch light as deliberate facets instead of razor cuboid corners.
fn slab(w: f32, h: f32, d: f32, top_w: f32, top_d: f32, e: f32) -> Mesh {
    let (a, b, c) = (w * 0.5, h * 0.5, d * 0.5);
    let e = e.min(a * 0.49).min(b * 0.49).min(c * 0.49).max(0.001);
    let (ai, bi, ci) = (a - e, b - e, c - e);
    let mut pos: Vec<[f32; 3]> = vec![
        [a, -bi, -ci], [a, bi, -ci], [a, bi, ci], [a, -bi, ci],
        [-a, -bi, -ci], [-a, bi, -ci], [-a, bi, ci], [-a, -bi, ci],
        [-ai, b, -ci], [ai, b, -ci], [ai, b, ci], [-ai, b, ci],
        [-ai, -b, -ci], [ai, -b, -ci], [ai, -b, ci], [-ai, -b, ci],
        [-ai, -bi, c], [ai, -bi, c], [ai, bi, c], [-ai, bi, c],
        [-ai, -bi, -c], [ai, -bi, -c], [ai, bi, -c], [-ai, bi, -c],
    ];
    for vtx in pos.iter_mut() {
        let f = (vtx[1] + b) / h;
        vtx[0] *= 1.0 + (top_w - 1.0) * f;
        vtx[2] *= 1.0 + (top_d - 1.0) * f;
    }
    let edges = [
        [1, 2, 10, 9], [3, 0, 13, 14], [6, 5, 8, 11], [7, 4, 12, 15],
        [3, 2, 18, 17], [0, 1, 22, 21], [7, 6, 19, 16], [4, 5, 23, 20],
        [11, 10, 18, 19], [8, 9, 22, 23], [15, 14, 17, 16], [12, 13, 21, 20],
    ];
    let corners = [[2, 10, 18], [1, 9, 22], [3, 14, 17], [0, 13, 21], [6, 11, 19], [5, 8, 23], [7, 15, 16], [4, 12, 20]];
    let mut raw: Vec<[u32; 3]> = Vec::new();
    for f in 0..6u32 {
        let o = f * 4;
        raw.push([o, o + 1, o + 2]);
        raw.push([o, o + 2, o + 3]);
    }
    for q in edges {
        raw.push([q[0], q[1], q[2]]);
        raw.push([q[0], q[2], q[3]]);
    }
    for t in corners {
        raw.push(t);
    }
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx = Vec::new();
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
        // Gnarled trunk — waisted over the legs, bellied at the chest, knotted at the shoulders.
        put(
            lathe(&[[0.0, 1.84], [0.3, 1.8], [0.4, 1.6], [0.34, 1.28], [0.4, 0.95], [0.44, 0.6], [0.34, 0.36], [0.28, 0.26], [0.0, 0.24]], 10),
            Vec3::new(1.0, 1.0, 0.85),
            Quat::IDENTITY,
            Vec3::ZERO,
            BARK,
        ),
        // Pale heartwood scar down the face of the trunk.
        blob(0.42, v(0.6, 1.55, 0.42), v(0.0, 1.05, 0.2), WOOD),
        // Bark ridges hugging the trunk.
        frustum(0.045, 0.06, 1.25, v(-0.3, 1.05, 0.16), rx(0.06), DARK),
        frustum(0.045, 0.06, 1.3, v(0.27, 1.0, 0.18), rx(0.05), DARK),
        frustum(0.04, 0.055, 1.1, v(0.05, 1.15, -0.32), rx(-0.06), DARK),
        // Shoulder knots (burls) with moss caps.
        blob(0.27, v(1.0, 0.85, 0.9), v(-0.46, 1.68, -0.02), BARK),
        blob(0.27, v(1.0, 0.85, 0.9), v(0.46, 1.68, -0.02), BARK),
        blob(0.16, v(1.1, 0.5, 1.0), v(-0.48, 1.86, -0.02), LEAF),
        blob(0.15, v(1.1, 0.5, 1.0), v(0.48, 1.86, -0.02), LEAF2),
        // Moss tufts on the trunk.
        blob(0.12, v(1.2, 0.6, 0.9), v(-0.14, 1.44, 0.3), LEAF),
        blob(0.1, v(1.2, 0.6, 0.9), v(0.18, 1.14, 0.32), LEAF2),
    ]);
    let head = group(vec![
        // Bark skull — a rounded burl, not a crate. The face sits HIGH and FORWARD: the trunk
        // lathe reaches 1.84 under the 1.86 head pivot, so anything below head-local y≈0.05
        // drowns in the trunk top (the first pass had the eyes peeking over the rim).
        blob(0.3, v(0.95, 0.8, 0.85), v(0.0, 0.08, 0.02), BARK),
        blob(0.2, v(1.05, 0.45, 0.7), v(0.0, 0.24, 0.16), DARK), // heavy brow ledge
        orb(0.06, v(-0.12, 0.12, 0.24), EYE), // glowing knot-hole eyes
        orb(0.06, v(0.12, 0.12, 0.24), EYE),
        put(slab(0.28, 0.05, 0.06, 0.9, 1.0, 0.015), Vec3::ONE, Quat::IDENTITY, v(0.0, -0.02, 0.26), DARK), // mouth gash
        // Leafy crown — clustered canopy blobs riding above the brow.
        blob(0.3, v(1.15, 0.85, 1.05), v(0.0, 0.46, -0.04), LEAF),
        blob(0.22, v(1.0, 0.8, 0.95), v(-0.26, 0.38, 0.04), LEAF2),
        blob(0.22, v(1.0, 0.8, 0.95), v(0.26, 0.4, -0.06), LEAF2),
        blob(0.18, v(1.0, 0.85, 1.0), v(0.02, 0.4, 0.2), LEAF),
    ]);
    // Branch arms: a tapering bough that forks into twig fingers at the tip.
    let arm = || {
        group(vec![
            frustum(0.1, 0.13, 0.55, v(0.0, -0.28, 0.0), rx(0.1), BARK), // upper bough
            frustum(0.07, 0.095, 0.5, v(0.0, -0.73, 0.05), rx(0.14), BARK), // lower bough
            blob(0.11, v(1.0, 0.9, 1.0), v(0.0, -0.5, 0.03), DARK), // elbow burl
            cone(0.05, 0.3, v(-0.1, -1.02, 0.09), rx(0.3), WOOD), // twig fingers
            cone(0.055, 0.36, v(0.0, -1.05, 0.11), rx(0.2), WOOD),
            cone(0.05, 0.3, v(0.1, -1.02, 0.09), rx(0.3), WOOD),
            blob(0.1, v(1.2, 0.55, 1.0), v(-0.04, -0.5, 0.12), LEAF), // mossy elbow
        ])
    };
    let leg = || {
        group(vec![
            frustum(0.14, 0.18, 0.68, v(0.0, -0.34, 0.0), Quat::IDENTITY, BARK), // trunk leg
            blob(0.17, v(1.15, 0.5, 1.2), v(0.0, -0.66, 0.02), DARK), // root-ball ankle
            cone(0.07, 0.3, v(-0.14, -0.66, 0.16), rx(1.2), DARK), // splayed roots
            cone(0.07, 0.3, v(0.14, -0.66, 0.16), rx(1.2), DARK),
            cone(0.07, 0.3, v(0.0, -0.66, -0.18), rx(-1.2), DARK),
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
    const SCARF: u32 = 0x9e3a30; // knitted red scarf — the one warm thing on him
    let torso = group(vec![
        orb(0.6, v(0.0, 0.62, 0.0), SNOW),   // bottom ball
        orb(0.5, v(0.0, 1.45, 0.0), SNOW),   // middle ball
        blob(0.34, v(1.0, 0.9, 0.8), v(0.0, 0.52, 0.34), SHADE), // belly shadow
        // coal buttons following the belly curve
        orb(0.055, v(0.0, 1.26, 0.48), COAL),
        orb(0.055, v(0.0, 1.44, 0.48), COAL),
        // knitted scarf wrapped around the "neck" (the ball junction under the head — NOT at the
        // head pivot itself, which blindfolds the face) + a tail flapping down the chest
        put(
            Torus { minor_radius: 0.095, major_radius: 0.42 }.mesh().build(),
            Vec3::ONE,
            rx(0.06),
            v(0.0, 1.6, 0.0),
            SCARF,
        ),
        put(slab(0.17, 0.4, 0.07, 0.8, 1.0, 0.02), Vec3::ONE, xyz(0.12, 0.0, 0.08), v(0.2, 1.34, 0.44), SCARF),
        put(slab(0.14, 0.3, 0.06, 0.75, 1.0, 0.02), Vec3::ONE, xyz(0.18, 0.0, -0.06), v(0.24, 1.06, 0.48), SCARF),
        // icicle epaulettes
        cone(0.1, 0.4, v(-0.45, 1.7, 0.0), rz(2.6), ICE),
        cone(0.1, 0.4, v(0.45, 1.7, 0.0), rz(-2.6), ICE),
    ]);
    let head = group(vec![
        orb(0.36, v(0.0, 0.0, 0.0), SNOW),
        orb(0.055, v(-0.13, 0.08, 0.31), COAL), // coal eyes
        orb(0.055, v(0.13, 0.08, 0.31), COAL),
        cone(0.07, 0.34, v(0.0, -0.02, 0.4), rx(1.57), CARROT), // carrot nose
        orb(0.035, v(-0.16, -0.12, 0.3), COAL), // coal smile
        orb(0.035, v(-0.06, -0.17, 0.31), COAL),
        orb(0.035, v(0.06, -0.17, 0.31), COAL),
        orb(0.035, v(0.16, -0.12, 0.3), COAL),
        // icicle crown
        cone(0.06, 0.3, v(-0.16, 0.4, 0.0), rz(0.3), ICE),
        cone(0.07, 0.4, v(0.0, 0.44, 0.0), Quat::IDENTITY, ICE),
        cone(0.06, 0.3, v(0.16, 0.4, 0.0), rz(-0.3), ICE),
        cone(0.045, 0.22, v(0.0, 0.36, -0.18), rx(-0.4), ICE),
    ]);
    // Gnarled stick arms with twig hands.
    let arm = || {
        group(vec![
            frustum(0.035, 0.05, 0.95, v(0.0, -0.42, 0.0), Quat::IDENTITY, STICK),
            frustum(0.02, 0.028, 0.24, v(-0.09, -0.86, 0.0), rz(0.5), STICK), // twig fingers
            frustum(0.02, 0.028, 0.26, v(0.0, -0.92, 0.0), Quat::IDENTITY, STICK),
            frustum(0.02, 0.028, 0.24, v(0.09, -0.86, 0.0), rz(-0.5), STICK),
            frustum(0.018, 0.024, 0.18, v(0.05, -0.6, 0.05), rx(0.6), STICK), // snapped side twig
        ])
    };
    // Packed-snow feet (it shuffles) — rounded mounds, not bricks.
    let leg = || {
        group(vec![
            blob(0.2, v(0.85, 0.75, 1.15), v(0.0, -0.16, 0.05), SNOW),
            blob(0.19, v(0.85, 0.3, 1.15), v(0.0, -0.3, 0.07), SHADE),
        ])
    };
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
        // Chest boulder — a chamfered monolith that tapers to the waist.
        put(slab(1.06, 1.04, 0.78, 0.82, 0.85, 0.11), Vec3::ONE, Quat::IDENTITY, v(0.0, 1.12, 0.0), STONE),
        put(slab(0.8, 0.42, 0.62, 1.15, 1.05, 0.08), Vec3::ONE, Quat::IDENTITY, v(0.0, 0.52, 0.0), DARK), // waist block
        // Shoulder crags — tapered slabs jutting up and out.
        put(slab(0.52, 0.46, 0.46, 0.55, 0.6, 0.07), Vec3::ONE, xyz(0.15, 0.35, 0.35), v(-0.46, 1.6, -0.05), STONE),
        put(slab(0.48, 0.42, 0.44, 0.55, 0.6, 0.07), Vec3::ONE, xyz(-0.12, 0.3, -0.4), v(0.47, 1.57, -0.03), STONE),
        put(slab(0.78, 0.72, 0.26, 0.75, 0.8, 0.08), Vec3::ONE, xyz(0.15, 0.0, 0.1), v(0.0, 1.18, -0.42), DARK), // back slab
        // Moss growing in the shoulder cracks.
        blob(0.15, v(1.4, 0.5, 1.3), v(-0.48, 1.45, 0.02), MOSS),
        blob(0.14, v(1.4, 0.5, 1.3), v(0.48, 1.42, 0.02), MOSS),
        // Molten chest core — a glowing dome set into the rock, with radiating crack seams.
        put(
            lathe(&[[0.0, 0.1], [0.14, 0.07], [0.2, 0.0], [0.14, -0.04], [0.0, -0.05]], 10),
            Vec3::ONE,
            rx(1.5708),
            v(0.0, 1.12, 0.38),
            CORE,
        ),
        put(slab(0.035, 0.3, 0.03, 0.6, 1.0, 0.008), Vec3::ONE, rz(0.5), v(-0.24, 0.96, 0.355), CORE), // glowing crack seams (flush with the rock face)
        put(slab(0.035, 0.26, 0.03, 0.6, 1.0, 0.008), Vec3::ONE, rz(-2.1), v(0.22, 1.28, 0.355), CORE),
        put(slab(0.035, 0.22, 0.03, 0.6, 1.0, 0.008), Vec3::ONE, rz(2.5), v(0.2, 0.94, 0.355), CORE),
    ]);
    let head = group(vec![
        put(slab(0.6, 0.52, 0.52, 0.85, 0.85, 0.08), Vec3::ONE, Quat::IDENTITY, v(0.0, 0.0, 0.0), STONE),
        put(slab(0.58, 0.12, 0.1, 0.9, 1.0, 0.03), Vec3::ONE, rx(0.1), v(0.0, 0.16, 0.24), DARK), // brow ridge
        // One broken crag off the crown's corner (asymmetric — a chipped monolith, not horns).
        put(slab(0.24, 0.2, 0.2, 0.45, 0.5, 0.04), Vec3::ONE, xyz(0.1, 0.4, 0.55), v(-0.19, 0.28, -0.06), STONE),
        blob(0.11, v(1.3, 0.45, 1.2), v(0.1, 0.28, -0.1), MOSS),
        blob(0.065, v(1.0, 0.85, 0.4), v(-0.16, 0.02, 0.24), CORE), // molten eyes — glowing slits set into the face
        blob(0.065, v(1.0, 0.85, 0.4), v(0.16, 0.02, 0.24), CORE),
        put(slab(0.4, 0.06, 0.06, 0.9, 1.0, 0.015), Vec3::ONE, Quat::IDENTITY, v(0.0, -0.18, 0.26), DARK), // grim mouth
    ]);
    let arm = || {
        group(vec![
            put(slab(0.38, 0.8, 0.38, 0.8, 0.8, 0.07), Vec3::ONE, Quat::IDENTITY, v(0.0, -0.42, 0.0), DARK), // arm column
            put(slab(0.03, 0.42, 0.03, 0.8, 1.0, 0.008), Vec3::ONE, rz(0.06), v(-0.03, -0.44, 0.185), CORE), // cracked glow seam down the arm's front face (a corner placement pokes past the chamfer and floats)
            put(slab(0.5, 0.46, 0.5, 0.85, 0.85, 0.09), Vec3::ONE, xyz(0.1, 0.2, 0.0), v(0.0, -0.96, 0.03), STONE), // boulder fist
            put(slab(0.14, 0.12, 0.1, 0.8, 0.8, 0.03), Vec3::ONE, Quat::IDENTITY, v(-0.1, -0.8, 0.22), DARK), // knuckles
            put(slab(0.14, 0.12, 0.1, 0.8, 0.8, 0.03), Vec3::ONE, Quat::IDENTITY, v(0.1, -0.8, 0.22), DARK),
        ])
    };
    let leg = || {
        group(vec![
            put(slab(0.38, 0.72, 0.42, 0.85, 0.85, 0.07), Vec3::ONE, Quat::IDENTITY, v(0.0, -0.37, 0.0), DARK),
            put(slab(0.46, 0.22, 0.52, 0.8, 0.85, 0.06), Vec3::ONE, Quat::IDENTITY, v(0.0, -0.66, 0.04), STONE), // boulder foot
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
        // Wrapped shroud — broad at the shoulders, spiralling down to a wisp tail (it floats).
        put(
            lathe(&[[0.0, 1.9], [0.3, 1.84], [0.42, 1.62], [0.34, 1.3], [0.27, 1.0], [0.2, 0.68], [0.12, 0.38], [0.05, 0.12], [0.0, 0.02]], 10),
            Vec3::new(1.0, 1.0, 0.8),
            Quat::IDENTITY,
            Vec3::ZERO,
            WRAP,
        ),
        blob(0.3, v(0.85, 1.5, 0.45), v(0.0, 1.1, 0.14), DARK), // sun-shadowed chest hollow
        // Wrapped shoulder humps.
        blob(0.22, v(1.0, 0.8, 0.85), v(-0.36, 1.66, -0.03), WRAP),
        blob(0.22, v(1.0, 0.8, 0.85), v(0.36, 1.66, -0.03), WRAP),
        // Loose bandage ends fluttering off the shroud (thin, tucked to the silhouette).
        put(slab(0.09, 0.46, 0.025, 0.5, 1.0, 0.008), Vec3::ONE, xyz(0.22, 0.0, 0.3), v(-0.24, 0.72, 0.1), DARK),
        put(slab(0.08, 0.4, 0.025, 0.5, 1.0, 0.008), Vec3::ONE, xyz(-0.18, 0.0, -0.28), v(0.22, 0.58, -0.06), DARK),
    ]);
    let head = group(vec![
        blob(0.24, v(0.9, 1.0, 0.9), v(0.0, 0.02, 0.02), BONE), // skull dome
        put(slab(0.26, 0.16, 0.22, 0.75, 0.8, 0.04), Vec3::ONE, Quat::IDENTITY, v(0.0, -0.18, 0.12), BONE), // jaw
        blob(0.075, v(1.0, 1.1, 0.6), v(-0.11, 0.02, 0.2), SHADOW), // eye sockets
        blob(0.075, v(1.0, 1.1, 0.6), v(0.11, 0.02, 0.2), SHADOW),
        orb(0.04, v(-0.11, 0.02, 0.24), EYE), // glowing pinpoints
        orb(0.04, v(0.11, 0.02, 0.24), EYE),
        cone(0.035, 0.09, v(0.0, -0.07, 0.23), rx(1.2), SHADOW), // nasal cavity
        // Ragged hood draped over the cranium — SHADOW-dark so the pale skull face reads out of it.
        put(
            lathe(&[[0.0, 0.42], [0.24, 0.34], [0.32, 0.1], [0.34, -0.12], [0.3, -0.18], [0.0, -0.14]], 9),
            Vec3::new(1.0, 1.0, 0.95),
            rx(0.15),
            v(0.0, 0.1, -0.05),
            SHADOW,
        ),
        cone(0.1, 0.32, v(0.0, 0.42, -0.16), rx(-0.7), SHADOW), // hood peak folding back
    ]);
    // Sword arm: wrapped bone limb ending in a curved scimitar of bone (two angled segments,
    // swept FORWARD so the blade rides ahead of the shroud instead of scraping the ground).
    let arm_blade = || {
        group(vec![
            frustum(0.075, 0.1, 0.5, v(0.0, -0.26, 0.0), rx(0.05), WRAP), // wrapped upper arm
            frustum(0.05, 0.065, 0.36, v(0.0, -0.66, 0.02), rx(0.1), BONE), // bare bone forearm
            blob(0.08, v(1.0, 0.8, 1.0), v(0.0, -0.86, 0.05), WRAP), // wrapped grip fist
            // Curved bone scimitar sweeping down-FORWARD out of the fist. NB the sign: `rx(-a)`
            // tips the slab's −Y (blade) end toward +Z — a positive rx swings it down-BACK and
            // detaches it from the fist (the first pass's floating blade).
            put(slab(0.05, 0.46, 0.14, 0.7, 0.75, 0.015), Vec3::ONE, rx(-0.55), v(0.0, -1.08, 0.17), BONE), // blade root
            put(slab(0.04, 0.38, 0.1, 0.4, 0.5, 0.012), Vec3::ONE, rx(-1.0), v(0.0, -1.38, 0.45), BONE), // curved tip
        ])
    };
    // Off arm: wrapped limb ending in a bony claw.
    let arm = || {
        group(vec![
            frustum(0.075, 0.1, 0.55, v(0.0, -0.28, 0.0), rx(0.05), WRAP),
            frustum(0.05, 0.065, 0.42, v(0.0, -0.72, 0.02), rx(0.08), BONE),
            cone(0.04, 0.22, v(-0.07, -1.02, 0.02), rz(0.4), BONE), // bony claws
            cone(0.04, 0.24, v(0.0, -1.05, 0.04), rz(0.0), BONE),
            cone(0.04, 0.22, v(0.07, -1.02, 0.02), rz(-0.4), BONE),
        ])
    };
    // No real legs — the wraith floats; one small drifting wisp curl each for the limb rig,
    // tucked INTO the tapering tail (the pivots sit at ±0.18, outside the shroud's waist — an
    // uncorrected wisp floats beside it as debris). `sx` pulls the mesh back toward the axis.
    let leg = |sx: f32| group(vec![cone(0.08, 0.26, v(sx * -0.12, -0.06, 0.03), xyz(2.95, 0.0, 0.15), DARK)]);
    let parts = vec![
        BossPart { kind: PartKind::Leg(1.0), pivot: v(-0.18, 0.5, 0.0), mesh: leg(-1.0) },
        BossPart { kind: PartKind::Leg(-1.0), pivot: v(0.18, 0.5, 0.0), mesh: leg(1.0) },
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
        // Hunched, pot-bellied mass — stacked organic blobs instead of crates.
        blob(0.5, v(0.95, 0.95, 0.8), v(0.0, 0.95, 0.02), HIDE), // body mass
        blob(0.36, v(1.0, 0.85, 0.75), v(0.0, 0.8, 0.3), BELLY), // sagging belly
        blob(0.38, v(0.95, 0.75, 0.9), v(0.0, 1.44, -0.16), HIDE), // hunched upper back
        blob(0.2, v(1.2, 0.5, 1.1), v(0.0, 1.62, -0.14), DARK), // moss drape over the hunch
        // warty lumps
        orb(0.09, v(-0.3, 1.32, -0.28), WART),
        orb(0.1, v(0.28, 1.1, -0.26), WART),
        orb(0.08, v(0.34, 0.72, 0.2), WART),
        orb(0.07, v(-0.34, 0.86, 0.24), WART),
        // ragged skirt of bog-weed around the hips
        cone(0.09, 0.4, v(-0.22, 0.45, 0.1), xyz(3.0, 0.0, 0.25), DARK),
        cone(0.09, 0.44, v(0.1, 0.42, -0.14), xyz(-3.0, 0.0, -0.2), DARK),
        cone(0.08, 0.38, v(0.26, 0.45, 0.12), xyz(3.05, 0.0, -0.3), DARK),
    ]);
    let head = group(vec![
        blob(0.28, v(1.0, 0.85, 0.9), v(0.0, 0.02, 0.0), HIDE), // skull
        blob(0.22, v(1.15, 0.5, 0.85), v(0.0, -0.18, 0.14), BELLY), // wide croaking jaw
        // jagged teeth
        cone(0.045, 0.16, v(-0.16, -0.13, 0.26), rx(3.14159), TOOTH),
        cone(0.045, 0.18, v(-0.05, -0.15, 0.29), rx(3.14159), TOOTH),
        cone(0.045, 0.16, v(0.07, -0.13, 0.27), rx(3.14159), TOOTH),
        cone(0.045, 0.18, v(0.18, -0.14, 0.25), rx(3.14159), TOOTH),
        blob(0.06, v(1.6, 0.45, 0.6), v(-0.15, 0.06, 0.24), EYE), // narrowed eyes
        blob(0.06, v(1.6, 0.45, 0.6), v(0.15, 0.06, 0.24), EYE),
        blob(0.16, v(1.35, 0.45, 0.8), v(0.0, 0.17, 0.14), DARK), // heavy brow
        cone(0.08, 0.3, v(0.0, -0.1, 0.34), rx(1.5), WART), // hooked nose
        // lank weed-hair DRAPED off the crown (steep tilts so the strands hang, not spike up)
        frustum(0.02, 0.05, 0.46, v(-0.26, 0.06, -0.08), xyz(0.35, 0.0, 1.0), DARK),
        frustum(0.02, 0.055, 0.52, v(-0.05, 0.08, -0.24), rx(0.9), DARK),
        frustum(0.02, 0.05, 0.46, v(0.26, 0.06, -0.08), xyz(0.35, 0.0, -1.0), DARK),
        frustum(0.018, 0.045, 0.42, v(0.12, 0.1, -0.22), xyz(0.8, 0.0, -0.4), DARK),
        frustum(0.018, 0.045, 0.4, v(-0.14, 0.12, 0.16), xyz(-0.85, 0.0, 0.5), DARK),
    ]);
    // Long gangly arms, taloned — bent at the elbow so they curl, not hang like pipes.
    let arm = || {
        group(vec![
            frustum(0.075, 0.1, 0.6, v(0.0, -0.3, 0.0), rx(0.12), HIDE), // upper arm
            frustum(0.055, 0.07, 0.5, v(0.0, -0.76, 0.09), rx(0.3), HIDE), // forearm curling forward
            blob(0.09, v(1.0, 0.8, 1.0), v(0.0, -0.55, 0.03), DARK), // knobbly elbow
            blob(0.1, v(1.0, 0.75, 1.1), v(0.0, -1.0, 0.16), BELLY), // gnarled hand (follows the curled forearm)
            cone(0.035, 0.24, v(-0.08, -1.12, 0.21), rx(0.4), TOOTH), // talons
            cone(0.035, 0.26, v(0.0, -1.14, 0.22), rx(0.3), TOOTH),
            cone(0.035, 0.24, v(0.08, -1.12, 0.21), rx(0.4), TOOTH),
        ])
    };
    let leg = || {
        group(vec![
            frustum(0.11, 0.14, 0.5, v(0.0, -0.25, 0.0), Quat::IDENTITY, HIDE), // squat leg
            blob(0.16, v(1.0, 0.4, 1.4), v(0.0, -0.48, 0.1), DARK), // splayed webbed foot
            cone(0.035, 0.14, v(-0.1, -0.5, 0.28), rx(1.4), TOOTH), // claws
            cone(0.035, 0.14, v(0.0, -0.5, 0.32), rx(1.4), TOOTH),
            cone(0.035, 0.14, v(0.1, -0.5, 0.28), rx(1.4), TOOTH),
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
