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
    Pine,
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

/// A conifer cone tier sitting with its base at `base_y` (cones are centre-anchored, so
/// lift by `h/2 + base_y`). `res` = radial sides.
fn cone_at(radius: f32, height: f32, base_y: f32, res: usize, center_xz: Vec3) -> Mesh {
    Cone { radius, height }
        .mesh()
        .resolution(res as u32)
        .build()
        .translated_by(Vec3::new(center_xz.x, height * 0.5 + base_y, center_xz.z))
}

pub fn build_tree_mesh(kind: TreeKind) -> Mesh {
    let m = match kind {
        TreeKind::Broadleaf => build_broadleaf(),
        TreeKind::Birch => build_birch(),
        TreeKind::Dead => build_dead(),
        TreeKind::Pine => build_pine(),
    };
    // Flat-shade so the foliage shows crisp icosphere facets (TS `flatShading: true`)
    // rather than soft smooth "blobs"), then bake the painterly per-facet shading.
    bake_facet_shading(flat_shaded(m))
}

/// Un-index + recompute per-face normals → hard flat-shaded facets.
fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// Bake painterly shading into the vertex colours (call AFTER `flat_shaded`, so the
/// per-face normals exist and every facet shades uniformly): facets angled up toward the
/// sky lighten, facets angled down darken (canopy undersides read self-shadowed), plus a
/// vertical gradient from a dark grounded skirt to a lit crown. This is what gives each
/// low-poly facet its own distinct value — realtime lighting alone can't produce it,
/// since a constant ambient/IBL fill lights the whole blob evenly and washes it flat.
fn bake_facet_shading(mut m: Mesh) -> Mesh {
    use bevy::mesh::VertexAttributeValues as V;
    let Some(V::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { return m };
    let (mut y_min, mut y_max) = (f32::MAX, f32::MIN);
    for p in pos {
        y_min = y_min.min(p[1]);
        y_max = y_max.max(p[1]);
    }
    let span = (y_max - y_min).max(1e-4);
    let ys: Vec<f32> = pos.iter().map(|p| p[1]).collect();
    let Some(V::Float32x3(ns)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else { return m };
    let nys: Vec<f32> = ns.iter().map(|n| n[1]).collect();
    if let Some(V::Float32x4(cols)) = m.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for (c, (ny, y)) in cols.iter_mut().zip(nys.iter().zip(&ys)) {
            let up = ny * 0.5 + 0.5; // 0 facing down … 1 facing up
            let h = (y - y_min) / span; // 0 base … 1 crown
            let f = (0.74 + 0.42 * up) * (0.86 + 0.26 * h);
            c[0] *= f;
            c[1] *= f;
            c[2] *= f;
        }
    }
    m
}

/// Multiply every vertex colour by an rgb tint — per-variant tree variety. Each tinted
/// copy is its own mesh asset (instances of it still batch), so a forest stops being one
/// identical green repeated thousands of times.
pub fn tint_mesh(mut m: Mesh, tint: [f32; 3]) -> Mesh {
    use bevy::mesh::VertexAttributeValues as V;
    if let Some(V::Float32x4(cols)) = m.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for c in cols.iter_mut() {
            c[0] *= tint[0];
            c[1] *= tint[1];
            c[2] *= tint[2];
        }
    }
    m
}

/// The foliage tint spread: neutral / warm sun-dried / deep cool green. Hue+value jitter
/// wide enough to read at gameplay distance but stay one family.
pub const TREE_TINTS: [[f32; 3]; 3] = [
    [1.0, 1.0, 1.0],
    [1.08, 1.04, 0.82],
    [0.80, 0.92, 0.86],
];

/// A root flare: short fat cylinders leaning out from the trunk foot, base sunk to y≈0,
/// so the bole reads grounded and gnarled instead of a pole stuck in the lawn.
fn root_flare(trunk_r: f32, n: usize, len: f32, color: [f32; 4], phase: f32) -> Vec<Mesh> {
    (0..n)
        .map(|i| {
            let a = phase + (i as f32 / n as f32) * std::f32::consts::TAU;
            let m = Cylinder::new(trunk_r * 0.42, len)
                .mesh()
                .resolution(5)
                .build()
                .translated_by(Vec3::new(0.0, len * 0.5, 0.0))
                // Lean well out from vertical so the root crawls along the ground...
                .rotated_by(Quat::from_rotation_z(1.05))
                .rotated_by(Quat::from_rotation_y(a))
                // ...rooted right at the trunk foot.
                .translated_by(Vec3::new(a.cos() * trunk_r * 0.55, 0.02, -a.sin() * trunk_r * 0.55));
            tinted(m, color)
        })
        .collect()
}

/// A short branch reaching from the trunk up into the canopy: a thin cylinder rotated by
/// `(tilt about Z, then yaw about Y)` with its BASE pivoted at `base` (not its centre).
fn branch_part(r: f32, len: f32, tilt: f32, yaw: f32, base: Vec3, color: [f32; 4]) -> Mesh {
    let m = Cylinder::new(r, len)
        .mesh()
        .resolution(5)
        .build()
        .translated_by(Vec3::new(0.0, len * 0.5, 0.0))
        .rotated_by(Quat::from_rotation_z(tilt))
        .rotated_by(Quat::from_rotation_y(yaw))
        .translated_by(base);
    tinted(m, color)
}

// ── Broadleaf "tree" — flared tapered trunk + branches + a 9-blob three-tone crown ──
//
// Upgraded from the original 6-layer spec build: the trunk is now two stacked tapering
// segments with a root flare and two visible limbs climbing into the foliage, and the
// crown is nine icospheres (dark base mass → mid body → light cap) pushed asymmetric so
// the silhouette reads grown, not stacked. Same palette + ~1.5u height as the original.
fn build_broadleaf() -> Mesh {
    let bark = lin(TREE_TRUNK);
    let mut parts = vec![
        // Two-segment tapering bole (thicker foot, slimmer upper) + root flare.
        tinted(trunk_part(0.10, 0.13, 0.34, 7, Vec3::new(0.0, 0.17, 0.0)), bark),
        tinted(trunk_part(0.075, 0.10, 0.34, 7, Vec3::new(0.015, 0.48, 0.01)), bark),
    ];
    parts.extend(root_flare(0.13, 4, 0.16, bark, 0.45));
    // Two limbs forking off the upper bole into the canopy mass.
    parts.push(branch_part(0.045, 0.34, 0.65, 0.4, Vec3::new(0.03, 0.52, 0.0), bark));
    parts.push(branch_part(0.04, 0.30, -0.75, 2.6, Vec3::new(-0.02, 0.46, 0.02), bark));

    // Crown: dark grounded mass → mid body → sunlit cap, with off-axis side lobes so no
    // two silhouettes line up. (radius, centre, tone)
    let blobs: [(f32, Vec3, u32); 9] = [
        (0.46, Vec3::new(0.0, 0.66, 0.0), FOLIAGE_DARK),
        (0.27, Vec3::new(0.27, 0.60, 0.10), FOLIAGE_DARK),
        (0.25, Vec3::new(-0.20, 0.58, -0.18), FOLIAGE_DARK),
        (0.40, Vec3::new(0.02, 0.88, 0.0), FOLIAGE_MID),
        (0.25, Vec3::new(-0.26, 0.84, 0.10), FOLIAGE_MID),
        (0.23, Vec3::new(0.22, 0.92, -0.16), FOLIAGE_MID),
        (0.32, Vec3::new(0.0, 1.08, 0.02), FOLIAGE_LIGHT),
        (0.20, Vec3::new(0.16, 1.18, 0.10), FOLIAGE_LIGHT),
        (0.21, Vec3::new(-0.06, 1.26, -0.06), FOLIAGE_LIGHT),
    ];
    for (r, c, tone) in blobs {
        parts.push(tinted(foliage(r, 1, c), lin(tone)));
    }
    merged(parts)
}

// ── Birch — tall pale trunk, banded bark marks all the way up, a side limb, airy crown ──
//
// Upgraded from the 2-mark/4-blob original: the trunk keeps its pale tapered column but
// now carries five staggered peeling-bark bands (alternating sides, varied widths — the
// classic birch "ladder"), a slim limb lifting a satellite leaf puff clear of the crown,
// and a six-blob crown (rounder ico-0 masses, dark under / light over) that drifts off
// axis for an airy, open silhouette. Same palette, slightly taller (~1.35u).
fn build_birch() -> Mesh {
    let mut parts = vec![tinted(
        trunk_part(0.055, 0.075, 0.86, 7, Vec3::new(0.0, 0.43, 0.0)),
        lin(BIRCH_TRUNK),
    )];
    // Shallow root flare keeps the slim pole grounded.
    parts.extend(root_flare(0.075, 3, 0.10, lin(BIRCH_TRUNK), 1.1));

    // Five peeling-bark bands hugging the trunk surface, alternating faces + heights.
    // (y, yaw, w, h) — thin boxes just proud of the bark.
    let marks: [(f32, f32, f32, f32); 5] = [
        (0.18, 0.3, 0.085, 0.030),
        (0.34, 2.4, 0.070, 0.040),
        (0.50, 4.4, 0.080, 0.026),
        (0.63, 1.4, 0.065, 0.036),
        (0.76, 3.5, 0.060, 0.024),
    ];
    for (my, yaw, w, h) in marks {
        let r_here = 0.075 - (my / 0.86) * 0.02; // follow the taper
        parts.push(tinted(
            Cuboid::new(0.006, h, w)
                .mesh()
                .build()
                .translated_by(Vec3::new(r_here, 0.0, 0.0))
                .rotated_by(Quat::from_rotation_y(yaw))
                .translated_by(Vec3::new(0.0, my, 0.0)),
            lin(BIRCH_MARK),
        ));
    }

    // A slim limb carrying its own small leaf puff out beside the crown.
    parts.push(branch_part(0.025, 0.30, 0.95, 0.9, Vec3::new(0.02, 0.62, 0.0), lin(BIRCH_TRUNK)));
    parts.push(tinted(foliage(0.16, 0, Vec3::new(0.27, 0.80, -0.22)), lin(BIRCH_LIGHT)));

    // Airy six-blob crown: dark base masses, light top puffs, drifting off the axis.
    let blobs: [(f32, Vec3, u32); 6] = [
        (0.32, Vec3::new(0.0, 0.98, 0.0), BIRCH_DARK),
        (0.22, Vec3::new(0.20, 1.06, 0.10), BIRCH_LIGHT),
        (0.23, Vec3::new(-0.18, 1.02, -0.10), BIRCH_DARK),
        (0.18, Vec3::new(-0.10, 1.18, 0.12), BIRCH_LIGHT),
        (0.17, Vec3::new(0.07, 1.24, -0.08), BIRCH_LIGHT),
        (0.13, Vec3::new(0.0, 1.34, 0.02), BIRCH_LIGHT),
    ];
    for (r, c, tone) in blobs {
        parts.push(tinted(foliage(r, 0, c), lin(tone)));
    }
    merged(parts)
}

// ── Dead tree — gnarled leaning snag: kinked trunk, root flare, 6 branches, deadfall ──
//
// Upgraded from the straight-pole original: the bole is two segments with a visible kink
// (the upper segment leans), flared roots grip the ground, six broken branches (two now
// carry short forked twig tips) claw at the sky, the top ends in a shattered-spike cone,
// and one fallen limb lies in the grass at the foot. Same two-tone dead-wood palette.
fn build_dead() -> Mesh {
    let wood = lin(DEAD_WOOD);
    let dark = lin(DEAD_WOOD_DARK);

    let mut parts = vec![
        // Lower bole (stout) + kinked upper bole leaning off plumb.
        tinted(trunk_part(0.065, 0.10, 0.5, 6, Vec3::new(0.0, 0.25, 0.0)), wood),
        tinted(
            Cylinder::new(0.05, 0.48)
                .mesh()
                .resolution(6)
                .build()
                .rotated_by(Quat::from_rotation_z(-0.16))
                .translated_by(Vec3::new(0.055, 0.71, 0.0)),
            wood,
        ),
        // Shattered spike topping the snag (a narrow cone, off the lean axis).
        tinted(
            Cone { radius: 0.04, height: 0.22 }
                .mesh()
                .resolution(5)
                .build()
                .rotated_by(Quat::from_rotation_z(-0.2))
                .translated_by(Vec3::new(0.12, 1.02, 0.0)),
            dark,
        ),
    ];
    parts.extend(root_flare(0.10, 4, 0.15, dark, 0.2));

    // Clawing branches: (r, len, tilt, yaw, base, tone). Tilts past ±1.2 lay them near
    // horizontal — a dead canopy's reach, not living lift.
    let limbs: [(f32, f32, f32, f32, Vec3, [f32; 4]); 6] = [
        (0.030, 0.42, 1.05, 0.2, Vec3::new(0.06, 0.66, 0.02), dark),
        (0.026, 0.36, -1.15, 0.5, Vec3::new(-0.03, 0.78, -0.02), dark),
        (0.022, 0.30, 0.85, 2.3, Vec3::new(0.04, 0.88, 0.04), wood),
        (0.020, 0.26, -0.95, 3.9, Vec3::new(0.02, 0.94, -0.03), wood),
        (0.018, 0.22, 1.25, 5.1, Vec3::new(0.05, 0.58, -0.04), wood),
        (0.016, 0.18, -0.7, 1.5, Vec3::new(0.08, 1.0, 0.02), dark),
    ];
    for (r, len, tilt, yaw, base, tone) in limbs {
        parts.push(branch_part(r, len, tilt, yaw, base, tone));
    }
    // Forked twig tips on the two big limbs (short thin stubs off the limb ends).
    parts.push(branch_part(0.014, 0.16, 1.5, 0.4, Vec3::new(0.40, 0.85, 0.10), dark));
    parts.push(branch_part(0.012, 0.13, -1.5, 0.7, Vec3::new(-0.30, 0.92, -0.10), dark));

    // A fallen limb rotting in the grass by the foot.
    parts.push(tinted(
        Cylinder::new(0.028, 0.4)
            .mesh()
            .resolution(5)
            .build()
            .rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
            .rotated_by(Quat::from_rotation_y(0.7))
            .translated_by(Vec3::new(0.28, 0.03, 0.22)),
        dark,
    ));

    merged(parts)
}

// ── Pine / spruce conifer — short brown trunk + 3 stacked green cone tiers + a tip ──
//
// The forest had no conifer (only the broadleaf/birch/dead broad-crowns), so this adds a
// strong new pointed silhouette. Snow-FREE (unlike the snow biome's snow-laden pine): a
// lush dark→light green spruce. Wide low tier → narrow high tier, each base overlapping
// the one below so the boughs layer. ~1.65u tall (towers a touch over the broadleaf).
fn build_pine() -> Mesh {
    let bark = lin(TREE_TRUNK);
    // Stub trunk under the boughs + a shallow root flare gripping the ground.
    let mut parts = vec![tinted(trunk_part(0.07, 0.095, 0.42, 6, Vec3::new(0.0, 0.21, 0.0)), bark)];
    parts.extend(root_flare(0.095, 3, 0.12, bark, 0.8));

    // Five overlapping bough tiers, dark shadowed skirt → sunlit crown, each nudged a
    // touch off the spire axis and yawed so the faceted cones never align — the jitter is
    // what turns "stacked party hats" into a grown spruce. 8 sides for crisper facets.
    // (base_y, radius, height, xz-nudge, yaw, tone)
    let tiers: [(f32, f32, f32, Vec3, f32, u32); 5] = [
        (0.26, 0.54, 0.52, Vec3::new(0.02, 0.0, -0.02), 0.0, FOLIAGE_DARK),
        (0.52, 0.46, 0.50, Vec3::new(-0.03, 0.0, 0.02), 0.4, FOLIAGE_DARK),
        (0.78, 0.38, 0.48, Vec3::new(0.02, 0.0, 0.03), 0.8, FOLIAGE_MID),
        (1.04, 0.30, 0.44, Vec3::new(-0.02, 0.0, -0.02), 1.2, FOLIAGE_MID),
        (1.28, 0.22, 0.38, Vec3::new(0.01, 0.0, 0.01), 1.6, FOLIAGE_LIGHT),
    ];
    for (base_y, r, h, nudge, yaw, c) in tiers {
        let m = Cone { radius: r, height: h }
            .mesh()
            .resolution(8)
            .build()
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(Vec3::new(nudge.x, h * 0.5 + base_y, nudge.z));
        parts.push(tinted(m, lin(c)));
    }
    // The sunlit leader spike capping the spire (~1.78u total).
    parts.push(tinted(cone_at(0.12, 0.30, 1.48, 7, Vec3::ZERO), lin(FOLIAGE_LIGHT)));

    merged(parts)
}
