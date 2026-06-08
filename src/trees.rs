//! Forest tree models — the Bevy rebuild of the TS forest's three scattered tree
//! variants (broadleaf "tree", birch, dead tree), built from the exact geometry in
//! `docs/specs/forest-tree-models-exact-bevy-rebuild-recipe.md`.
//!
//! CONTRACT: each `build_*` returns ONE merged, vertex-coloured `Mesh` with its base
//! at y=0 (trunk bottom on the ground), built from primitive parts via `Mesh::merge`
//! with every part carrying `ATTRIBUTE_COLOR` (linear). Rendered against the shared
//! white vertex-colour material in `scatter.rs`. See `CONTRACT.md` for the merge/tint
//! pattern + correct Bevy 0.18 mesh API.
//!
//! Geometry parity notes:
//! - The TS trunks/branches are tapered `CylinderGeometry(radiusTop, radiusBottom, …)`.
//!   Bevy's `Cylinder::new(radius, height)` has a SINGLE radius, so each tapered part
//!   uses the **average** of the TS top/bottom radii (per CONTRACT.md).
//! - Both three.js `CylinderGeometry` and Bevy `Cylinder` are centred on the origin
//!   (base at local y = −height/2), so translating a part by the TS *center* position
//!   (e.g. trunk `[0, 0.25, 0]` for a 0.5-tall trunk) lands the base on y=0 exactly.
//! - Foliage `IcosahedronGeometry(r, detail)` → `Sphere::new(r).mesh().ico(detail)`,
//!   translated to the TS layer centre. The detail level (0 vs 1) is carried over so
//!   birch reads rounder than the broadleaf.
//! - Branch rotations use the TS Euler `[x, y, z]` (radians) via
//!   `Quat::from_euler(EulerRot::XYZ, x, y, z)` before the translate.

use bevy::prelude::*;

use crate::palette::{
    lin, BIRCH_DARK, BIRCH_LIGHT, BIRCH_MARK, BIRCH_TRUNK, DEAD_WOOD, DEAD_WOOD_DARK, FOLIAGE_DARK,
    FOLIAGE_LIGHT, FOLIAGE_MID, TREE_TRUNK,
};

#[derive(Clone, Copy)]
pub enum TreeKind {
    Broadleaf,
    Birch,
    Dead,
}

/// Tag every vertex of `m` with a flat linear colour so it can be merged with other
/// coloured parts (all parts must carry `ATTRIBUTE_COLOR` before `Mesh::merge`).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge a non-empty list of pre-`tinted` parts into ONE mesh (so the renderer keeps
/// them in a single batch). All parts share POSITION/NORMAL/UV_0/COLOR, so the merge
/// always succeeds; `.expect` makes a mismatch loud rather than silent.
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("tree parts share attributes");
    }
    base
}

/// A tapered TS trunk/branch → a Bevy `Cylinder` using the AVERAGE of the top/bottom
/// radii, with 6 radial segments by default. `center` is the TS position (the part's
/// centre); the Bevy cylinder is centred too, so this lands the base correctly.
fn trunk_part(r_top: f32, r_bottom: f32, height: f32, segments: usize, center: Vec3) -> Mesh {
    let r = (r_top + r_bottom) * 0.5;
    Cylinder::new(r, height)
        .mesh()
        .resolution(segments as u32)
        .build()
        .translated_by(center)
}

/// A foliage blob → an icosphere of the given radius/detail, translated to `center`.
fn foliage(radius: f32, detail: u8, center: Vec3) -> Mesh {
    Sphere::new(radius)
        .mesh()
        .ico(detail as u32)
        .expect("ico detail in range")
        .translated_by(center)
}

pub fn build_tree_mesh(kind: TreeKind) -> Mesh {
    let m = match kind {
        TreeKind::Broadleaf => build_broadleaf(),
        TreeKind::Birch => build_birch(),
        TreeKind::Dead => build_dead(),
    };
    // Flat-shade so the foliage shows crisp icosphere facets (TS `flatShading: true`)
    // rather than soft smooth "blobs".
    flat_shaded(m)
}

/// Un-index + recompute per-face normals → hard flat-shaded facets.
fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

// ── Broadleaf "tree" — tapered trunk + 6 icosphere foliage layers (ico detail 1) ──
//
// Trunk: CylinderGeometry(0.09, 0.12, 0.5, 6) at [0,0.25,0], #5a3a22.
// Six foliage layers (dark/dark, mid/mid, light/light) per the spec, building a
// layered three-tone crown so it reads lush and 3D rather than a flat sphere.
fn build_broadleaf() -> Mesh {
    let trunk = tinted(
        trunk_part(0.09, 0.12, 0.5, 6, Vec3::new(0.0, 0.25, 0.0)),
        lin(TREE_TRUNK),
    );

    // Layer 1+2: dark base mass (#2f7a36)
    let l1 = tinted(foliage(0.46, 1, Vec3::new(0.0, 0.64, 0.0)), lin(FOLIAGE_DARK));
    let l2 = tinted(
        foliage(0.26, 1, Vec3::new(0.24, 0.6, 0.06)),
        lin(FOLIAGE_DARK),
    );
    // Layer 3+4: mid tone (#3a9442)
    let l3 = tinted(foliage(0.4, 1, Vec3::new(0.0, 0.86, 0.0)), lin(FOLIAGE_MID));
    let l4 = tinted(
        foliage(0.24, 1, Vec3::new(-0.22, 0.82, -0.08)),
        lin(FOLIAGE_MID),
    );
    // Layer 5+6: light crown cap + tip (#4cb358)
    let l5 = tinted(
        foliage(0.33, 1, Vec3::new(0.0, 1.06, 0.0)),
        lin(FOLIAGE_LIGHT),
    );
    let l6 = tinted(
        foliage(0.22, 1, Vec3::new(0.0, 1.24, 0.0)),
        lin(FOLIAGE_LIGHT),
    );

    merged(vec![trunk, l1, l2, l3, l4, l5, l6])
}

// ── Birch — pale tapered trunk + 2 dark bark-mark boxes + 4 rounder foliage (ico 0) ──
//
// Trunk: CylinderGeometry(0.06, 0.075, 0.8, 6) at [0,0.4,0], #ece8d8.
// Marks: thin dark boxes suggesting peeling-bark stripes.
// Foliage: 4 detail-0 icospheres (rounder than the broadleaf), dark/light alternating.
fn build_birch() -> Mesh {
    let trunk = tinted(
        trunk_part(0.06, 0.075, 0.8, 6, Vec3::new(0.0, 0.4, 0.0)),
        lin(BIRCH_TRUNK),
    );

    // Two dark bark-mark boxes (BoxGeometry(w,h,d) at center positions).
    let mark1 = tinted(
        Cuboid::new(0.005, 0.04, 0.08)
            .mesh()
            .build()
            .translated_by(Vec3::new(0.075, 0.55, 0.0)),
        lin(BIRCH_MARK),
    );
    let mark2 = tinted(
        Cuboid::new(0.005, 0.03, 0.06)
            .mesh()
            .build()
            .translated_by(Vec3::new(-0.075, 0.32, 0.02)),
        lin(BIRCH_MARK),
    );

    // Four foliage masses (rounder, detail 0): dark base, light bump, dark bump, light tip.
    let f1 = tinted(foliage(0.34, 0, Vec3::new(0.0, 0.95, 0.0)), lin(BIRCH_DARK));
    let f2 = tinted(
        foliage(0.22, 0, Vec3::new(0.18, 1.05, 0.1)),
        lin(BIRCH_LIGHT),
    );
    let f3 = tinted(
        foliage(0.24, 0, Vec3::new(-0.16, 1.0, -0.1)),
        lin(BIRCH_DARK),
    );
    let f4 = tinted(
        foliage(0.18, 0, Vec3::new(0.05, 1.18, 0.0)),
        lin(BIRCH_LIGHT),
    );

    merged(vec![trunk, mark1, mark2, f1, f2, f3, f4])
}

// ── Dead tree — bare tapered trunk + 4 angled broken-branch cylinders, no foliage ──
//
// Trunk: CylinderGeometry(0.06, 0.095, 0.9, 6) at [0,0.45,0], #6e6258.
// Branches: tapered (avg-radius) cylinders, 5 segments, each rotated by the TS Euler
// then translated to its TS centre. Two darker (#4a4238), two trunk-tone (#6e6258).
fn build_dead() -> Mesh {
    let trunk = tinted(
        trunk_part(0.06, 0.095, 0.9, 6, Vec3::new(0.0, 0.45, 0.0)),
        lin(DEAD_WOOD),
    );

    // Each branch: build the avg-radius cylinder at the origin, rotate by the TS Euler
    // (XYZ radians), then translate to the TS centre position.
    let branch = |r_top: f32,
                  r_bottom: f32,
                  height: f32,
                  rot: Vec3,
                  center: Vec3,
                  color: [f32; 4]|
     -> Mesh {
        let r = (r_top + r_bottom) * 0.5;
        let m = Cylinder::new(r, height)
            .mesh()
            .resolution(5)
            .build()
            .rotated_by(Quat::from_euler(EulerRot::XYZ, rot.x, rot.y, rot.z))
            .translated_by(center);
        tinted(m, color)
    };

    // Branch 1 (upper right): rot z −0.8, darker.
    let b1 = branch(
        0.025,
        0.04,
        0.42,
        Vec3::new(0.0, 0.0, -0.8),
        Vec3::new(0.2, 0.7, 0.08),
        lin(DEAD_WOOD_DARK),
    );
    // Branch 2 (upper left): rot z 0.7, darker.
    let b2 = branch(
        0.022,
        0.035,
        0.36,
        Vec3::new(0.0, 0.0, 0.7),
        Vec3::new(-0.17, 0.82, -0.04),
        lin(DEAD_WOOD_DARK),
    );
    // Branch 3 (mid upper right): rot [0.4, 0, 0.2], trunk tone.
    let b3 = branch(
        0.018,
        0.028,
        0.3,
        Vec3::new(0.4, 0.0, 0.2),
        Vec3::new(0.06, 1.0, 0.13),
        lin(DEAD_WOOD),
    );
    // Branch 4 (mid upper left): rot [−0.3, 0, −0.4], trunk tone.
    let b4 = branch(
        0.016,
        0.024,
        0.26,
        Vec3::new(-0.3, 0.0, -0.4),
        Vec3::new(-0.08, 1.05, -0.1),
        lin(DEAD_WOOD),
    );

    merged(vec![trunk, b1, b2, b3, b4])
}
