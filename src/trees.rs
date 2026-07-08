//! Forest tree models — the Bevy rebuild of the TS forest's three scattered tree
//! variants (broadleaf "tree", birch, dead tree), built from the exact geometry in
//! `docs/specs/forest-tree-models-exact-bevy-rebuild-recipe.md`.
//!
//! CONTRACT: each `build_*` returns ONE merged, vertex-coloured `Mesh` with its base
//! at y=0 (trunk bottom on the ground), built from primitive parts via `Mesh::merge`
//! with every part carrying `ATTRIBUTE_COLOR` (linear). Rendered against the shared
//! white vertex-colour material in `scatter.rs`. See the verified-APIs doc §9 for the
//! merge/tint pattern + correct Bevy 0.18 mesh API.
//!
//! Geometry parity notes:
//! - The TS trunks/branches are tapered `CylinderGeometry(radiusTop, radiusBottom, …)`.
//!   Bevy's `Cylinder::new(radius, height)` has a SINGLE radius, so each tapered part
//!   uses the **average** of the TS top/bottom radii.
//! - Both three.js `CylinderGeometry` and Bevy `Cylinder` are centred on the origin
//!   (base at local y = −height/2), so translating a part by the TS *center* position
//!   (e.g. trunk `[0, 0.25, 0]` for a 0.5-tall trunk) lands the base on y=0 exactly.
//! - Foliage `IcosahedronGeometry(r, detail)` → `Sphere::new(r).mesh().ico(detail)`,
//!   translated to the TS layer centre. The detail level (0 vs 1) is carried over so
//!   birch reads rounder than the broadleaf.
//! - Branch rotations use the TS Euler `[x, y, z]` (radians) via
//!   `Quat::from_euler(EulerRot::XYZ, x, y, z)` before the translate.

use bevy::prelude::*;

use crate::meshkit::{flat_shaded, merged, tinted};
use crate::palette::{
    lin, AUTUMN_DARK, AUTUMN_GOLD, AUTUMN_LIGHT, AUTUMN_MID, AUTUMN_OLIVE, AUTUMN_RED, BIRCH_DARK,
    BIRCH_LIGHT, BIRCH_MARK, BIRCH_TRUNK, CUT_WOOD, DEAD_WOOD, DEAD_WOOD_DARK, FOLIAGE_DARK,
    FOLIAGE_LIGHT, FOLIAGE_MID, TREE_TRUNK,
};

#[derive(Clone, Copy)]
pub enum TreeKind {
    Broadleaf,
    Birch,
    Dead,
    Pine,
    /// Tall columnar poplar/cypress — a slim flame silhouette that breaks the round-crown line.
    Poplar,
    /// Broadleaf-shaped crown in russet/orange/gold autumn foliage.
    Autumn,
    /// A waist-high sawn stump with a pale ringed cut face — ground-level forest detail.
    Stump,
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
        TreeKind::Poplar => build_poplar(),
        TreeKind::Autumn => build_autumn(),
        TreeKind::Stump => build_stump(),
    };
    // Flat-shade so the foliage shows crisp icosphere facets (TS `flatShading: true`)
    // rather than soft smooth "blobs"), then bake the painterly per-facet shading.
    bake_facet_shading(flat_shaded(m))
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
            // Lifted facet shading (was 0.74+0.42·up / 0.86+0.26·h): higher floor so the
            // shaded undersides/base no longer crush to near-black — the canopy reads bright
            // and airy like the reference, while keeping enough spread for facet definition.
            let f = (0.86 + 0.22 * up) * (0.90 + 0.16 * h);
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

/// The foliage tint spread: neutral / warm sun-dried / deep cool / sun-gilded / bright
/// spring. Hue+value jitter wide enough to read at gameplay distance but stay one family.
pub const TREE_TINTS: [[f32; 3]; 5] = [
    [1.0, 1.0, 1.0],
    [1.08, 1.04, 0.82],
    [0.80, 0.92, 0.86],
    [1.14, 1.05, 0.66], // golden early-autumn edge
    [0.90, 1.06, 0.74], // fresh yellow-green spring growth
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
    build_broadleaf_toned(FOLIAGE_DARK, FOLIAGE_MID, FOLIAGE_LIGHT)
}

// Autumn broadleaf: the broadleaf bole + nine-blob crown, but the crown is dappled across
// the full turn (deep russet & a lingering olive in the shaded base, burnt orange & amber
// through the body, sunlit gold on the cap) instead of one flat orange. The mixed hues read
// as leaf clusters turning at different rates — and these are real warm tones the green
// TREE_TINTS can't reach by multiply. Same 9 icospheres as the broadleaf, so no mesh cost.
fn build_autumn() -> Mesh {
    let bark = lin(TREE_TRUNK);
    // Same tall slim bole + high clustered crown as the green broadleaf (they're one species,
    // just turned), so an autumn tree stands shoulder-to-shoulder with the greens.
    let mut parts = vec![
        tinted(trunk_part(0.075, 0.10, 0.5, 7, Vec3::new(0.0, 0.25, 0.0)), bark),
        tinted(trunk_part(0.058, 0.072, 0.5, 7, Vec3::new(0.02, 0.70, 0.01)), bark),
    ];
    parts.extend(root_flare(0.10, 4, 0.13, bark, 0.45));
    parts.push(branch_part(0.035, 0.30, 0.7, 0.4, Vec3::new(0.03, 0.78, 0.0), bark));
    parts.push(branch_part(0.032, 0.26, -0.8, 2.6, Vec3::new(-0.02, 0.74, 0.02), bark));

    // (radius, centre, tone) — dark/red/olive shaded base → orange/amber body → gold cap.
    let blobs: [(f32, Vec3, u32); 8] = [
        (0.34, Vec3::new(0.0, 1.06, 0.0), AUTUMN_DARK),
        (0.27, Vec3::new(0.28, 1.00, 0.08), AUTUMN_RED),
        (0.26, Vec3::new(-0.24, 1.04, -0.10), AUTUMN_OLIVE),
        (0.30, Vec3::new(0.04, 1.30, 0.04), AUTUMN_MID),
        (0.23, Vec3::new(-0.22, 1.26, 0.12), AUTUMN_RED),
        (0.22, Vec3::new(0.22, 1.30, -0.12), AUTUMN_LIGHT),
        (0.24, Vec3::new(-0.02, 1.52, -0.02), AUTUMN_MID),
        (0.18, Vec3::new(0.14, 1.56, 0.10), AUTUMN_GOLD),
    ];
    for (r, c, tone) in blobs {
        parts.push(tinted(foliage(r, 1, c), lin(tone)));
    }
    merged(parts)
}

/// The broadleaf body shared by the green `Broadleaf` and the `Autumn` variant — only the
/// three crown tones differ. (dark base mass → mid body → light sunlit cap)
fn build_broadleaf_toned(dark: u32, mid: u32, light: u32) -> Mesh {
    let bark = lin(TREE_TRUNK);
    // Reference-style broadleaf: a TALL slim bole carries a high, bumpy clustered crown that
    // floats clear of the ground (you can see most of the trunk), instead of a fat short pole
    // wearing a ground-hugging ball. Trunk visible for ~half the height, then a clump of
    // faceted leaf masses (ico detail 1 — crisp low-poly facets, as in the reference).
    let mut parts = vec![
        // Two-segment slim tapering bole, reaching ~0.95u before the crown.
        tinted(trunk_part(0.075, 0.10, 0.5, 7, Vec3::new(0.0, 0.25, 0.0)), bark),
        tinted(trunk_part(0.058, 0.072, 0.5, 7, Vec3::new(0.02, 0.70, 0.01)), bark),
    ];
    parts.extend(root_flare(0.10, 4, 0.13, bark, 0.45));
    // Two limbs forking off the upper bole up into the clustered crown.
    parts.push(branch_part(0.035, 0.30, 0.7, 0.4, Vec3::new(0.03, 0.78, 0.0), bark));
    parts.push(branch_part(0.032, 0.26, -0.8, 2.6, Vec3::new(-0.02, 0.74, 0.02), bark));

    // Clustered bumpy crown sitting high on the bole: a knot of faceted leaf masses,
    // dark shaded base/back → mid body → light sunlit crown, pushed off-axis so the
    // silhouette reads as several leaf clumps, not one smooth ball. (radius, centre, tone)
    let blobs: [(f32, Vec3, u32); 8] = [
        (0.34, Vec3::new(0.0, 1.06, 0.0), dark),
        (0.27, Vec3::new(0.28, 1.00, 0.08), dark),
        (0.26, Vec3::new(-0.24, 1.04, -0.10), mid),
        (0.30, Vec3::new(0.04, 1.30, 0.04), mid),
        (0.23, Vec3::new(-0.22, 1.26, 0.12), mid),
        (0.22, Vec3::new(0.22, 1.30, -0.12), light),
        (0.24, Vec3::new(-0.02, 1.52, -0.02), light),
        (0.18, Vec3::new(0.14, 1.56, 0.10), light),
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
    parts.push(tinted(foliage(0.16, 1, Vec3::new(0.27, 0.80, -0.22)), lin(BIRCH_LIGHT)));

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
        parts.push(tinted(foliage(r, 1, c), lin(tone)));
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
        // Lower bole (stout) + kinked upper bole leaning off plumb. The upper segment is
        // only a touch slimmer than the lower top and overlaps it by ~0.14u, so the bole
        // reads as ONE continuous trunk — the old r0.05 upper stub pinched off the fat base
        // and looked snapped clean through (the reported "tree does not connect").
        tinted(trunk_part(0.075, 0.105, 0.6, 6, Vec3::new(0.0, 0.30, 0.0)), wood),
        tinted(
            Cylinder::new(0.072, 0.58)
                .mesh()
                .resolution(6)
                .build()
                .rotated_by(Quat::from_rotation_z(-0.14))
                .translated_by(Vec3::new(0.045, 0.78, 0.0)),
            wood,
        ),
        // Shattered spike topping the snag (a narrow cone, off the lean axis).
        tinted(
            Cone { radius: 0.05, height: 0.22 }
                .mesh()
                .resolution(5)
                .build()
                .rotated_by(Quat::from_rotation_z(-0.2))
                .translated_by(Vec3::new(0.1, 1.06, 0.0)),
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

// ── Pine / spruce conifer — slim trunk + 8 slender drooping cone tiers + a sharp leader ──
//
// A strong pointed silhouette (the layered spruces in the reference). Snow-FREE (unlike the
// snow biome's snow-laden pine): a lush dark→light green spruce. Eight narrow overlapping
// tiers from a wide skirt to a sharp point, each tier's base overhanging the one above so the
// boughs read as drooping serrated layers. ~2.1u tall (towers well over the broadleaf).
fn build_pine() -> Mesh {
    // 2026-07, third rework, matched to the player's reference render: the pine is NOT stacked
    // cones at all — it's whorls of INDIVIDUAL flat feather-boughs around a chunky faceted bark
    // trunk, with air between the blades. Each bough is a hand-built tent-ridged blade (pointed
    // root and tip, widest ~40% out, top face lit green / underside shadow green), pitched
    // gently downward and scattered around the ring with yaw/length jitter. A small hanging
    // pine cone finishes it.
    let bark = lin(TREE_TRUNK);
    let bark_dark = lin(0x5a3c24);
    let mut seed: u32 = 0xC0FFEE;

    // Chunky visible trunk: two ruffled tapered segments (irregular bark chunks) + a slim
    // hidden spine carrying the upper whorls, over the shared root flare.
    let mut parts = vec![
        tinted(ruffled(trunk_part(0.13, 0.20, 0.5, 7, Vec3::new(0.0, 0.25, 0.0)), 3, 0.26), bark),
        tinted(ruffled(trunk_part(0.09, 0.13, 0.6, 7, Vec3::new(0.0, 0.78, 0.0)), 11, 0.20), bark_dark),
        tinted(trunk_part(0.045, 0.085, 1.1, 6, Vec3::new(0.0, 1.55, 0.0)), bark),
    ];
    parts.extend(root_flare(0.19, 4, 0.15, bark, 1.0));

    // Whorls bottom→top: (y, bough length, half-width, count, downward pitch). DENSE — eight
    // closely-spaced rings with wide leafy blades, so the trunk only shows at the base and the
    // crown reads full with just slivers of air between the feathers (the reference look).
    let whorls: [(f32, f32, f32, usize, f32); 8] = [
        (0.55, 0.95, 0.200, 9, 0.40),
        (0.78, 0.88, 0.190, 8, 0.36),
        (1.00, 0.78, 0.175, 8, 0.32),
        (1.22, 0.68, 0.160, 7, 0.28),
        (1.44, 0.57, 0.145, 7, 0.25),
        (1.64, 0.46, 0.130, 6, 0.22),
        (1.83, 0.35, 0.110, 5, 0.18),
        (2.00, 0.24, 0.090, 4, 0.15),
    ];
    for (wi, (y, len, w, n, pitch)) in whorls.into_iter().enumerate() {
        let ring_yaw = prand(&mut seed) * std::f32::consts::TAU;
        for i in 0..n {
            let yaw = ring_yaw
                + i as f32 / n as f32 * std::f32::consts::TAU
                + (prand(&mut seed) - 0.5) * 0.45;
            let l = len * (0.9 + prand(&mut seed) * 0.22);
            let p = pitch + (prand(&mut seed) - 0.5) * 0.10;
            // Alternate the top tone slightly so neighbouring blades never read as clones.
            // Deep spruce greens — the facet-shading bake brightens every up-facing blade, so
            // these are authored ~a shade darker than the target render tone.
            let top = if (i + wi) % 2 == 0 { 0x4f7d3c } else { 0x467238 };
            let bough = pine_bough(l, w, lin(top), lin(0x315427), &mut seed)
                .rotated_by(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(p))
                .translated_by(Vec3::new(0.0, y, 0.0));
            parts.push(bough);
        }
    }

    // Leader: a sharp faceted tip spike capping the spire.
    parts.push(tinted(cone_at(0.075, 0.40, 2.10, 6, Vec3::ZERO), lin(0x4f7d3c)));

    // The hanging pine cone (the reference's charming accent): a small squashed brown ico.
    let cone_fruit = Sphere::new(0.075)
        .mesh()
        .ico(1)
        .expect("ico detail in range")
        .scaled_by(Vec3::new(0.8, 1.35, 0.8))
        .translated_by(Vec3::new(0.26, 1.34, 0.12));
    parts.push(tinted(cone_fruit, lin(0x6e4a2a)));

    merged(parts)
}

/// One flat pine bough: a tent-ridged feather blade along +Z — pointed at the root and tip,
/// widest ~40% out, a low spine ridge on top, a slight tip sag. Built as a triangle soup
/// (positions duplicated per face) so the top faces carry the lit tone and the underside the
/// shadow tone in one mesh; `flat_shaded` recomputes the real facet normals later.
fn pine_bough(len: f32, w: f32, top: [f32; 4], under: [f32; 4], seed: &mut u32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::PrimitiveTopology;

    let sag = -(0.03 + prand(seed) * 0.06) * len;
    let ww = w * (0.85 + prand(seed) * 0.35);
    let v0 = [0.0, 0.02, 0.0]; // root, at the trunk axis
    let vl = [-ww, -0.02, 0.42 * len];
    let vr = [ww, -0.02, 0.42 * len];
    let vs = [0.0, 0.055, 0.40 * len]; // spine ridge
    let vt = [0.0, sag, len]; // tip
    // Top faces (CCW from above) then the flat underside (CCW from below).
    let tris: [([f32; 3], [f32; 3], [f32; 3], [f32; 4]); 6] = [
        (v0, vl, vs, top),
        (v0, vs, vr, top),
        (vl, vt, vs, top),
        (vs, vt, vr, top),
        (v0, vr, vt, under),
        (v0, vt, vl, under),
    ];
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(18);
    let mut col: Vec<[f32; 4]> = Vec::with_capacity(18);
    for (a, b, c, tone) in tris {
        pos.extend([a, b, c]);
        col.extend([tone; 3]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        // MUST be indexed: `Mesh::merge` into the (indexed) trunk primitives appends vertices
        // but only copies INDICES — an unindexed soup merges in silently invisible (verified:
        // the whole crown vanished, leaving a bare trunk).
        .with_inserted_indices(bevy::mesh::Indices::U32((0..n as u32).collect()))
}

/// Tiny deterministic rng ([0,1)) for the bough jitter (mulberry32 step).
fn prand(s: &mut u32) -> f32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    (t ^ (t >> 14)) as f32 / 4_294_967_296.0
}

/// Organic "ruffle" for conifer boughs: deterministically jitter a mesh's vertices, scaled by
/// each vertex's radial distance from the Y axis, so the apex/centre stay pinned while the rim
/// breaks into an irregular hand-modelled silhouette (a clean `Cone` reads as a mechanical
/// "choinka" — player feedback, twice). The offset is keyed on the QUANTISED VERTEX POSITION
/// (not the vertex index), so coincident vertices shared between the side fan and the base cap
/// move identically and the mesh stays watertight. Runs before `flat_shaded`, which then turns
/// the ragged geometry into crisp uneven facets.
fn ruffled(mut m: Mesh, seed: u32, amp: f32) -> Mesh {
    use bevy::mesh::VertexAttributeValues;
    if let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in pos.iter_mut() {
            let r = (p[0] * p[0] + p[2] * p[2]).sqrt();
            if r < 1e-4 {
                continue;
            }
            let k = amp * r;
            let mut s = seed
                ^ (((p[0] * 512.0).round() as i32 as u32).wrapping_mul(0x9E37_79B9))
                ^ (((p[1] * 512.0).round() as i32 as u32).wrapping_mul(0x85EB_CA6B))
                ^ (((p[2] * 512.0).round() as i32 as u32).wrapping_mul(0xC2B2_AE35));
            let mut rr = || {
                s = s.wrapping_add(0x6d2b_79f5);
                let mut t = s;
                t = (t ^ (t >> 15)).wrapping_mul(t | 1);
                t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
                (t ^ (t >> 14)) as f32 / 4_294_967_296.0
            };
            p[0] += (rr() - 0.5) * 2.0 * k;
            p[2] += (rr() - 0.5) * 2.0 * k;
            p[1] += (rr() - 0.5) * 1.6 * k;
        }
    }
    m
}

// ── Poplar / cypress — slim columnar flame: tall narrow stack of foliage blobs ──────────
//
// A strong vertical silhouette the round-crown trees lack (the tall trees in the reference
// image). A barely-visible slim trunk carries ~7 overlapping icospheres stacked on the
// spire axis: narrow at the foot, bellying out low-middle, tapering back to a point — a
// Lombardy-poplar/cypress flame. Reads green by default; a gold TREE_TINT turns it into the
// yellow poplar from the reference. ~1.95u tall, only ~0.5u wide.
fn build_poplar() -> Mesh {
    let bark = lin(TREE_TRUNK);
    // Slim trunk, mostly buried in the column; shallow flare so it grips the ground.
    let mut parts = vec![tinted(trunk_part(0.05, 0.07, 0.5, 6, Vec3::new(0.0, 0.25, 0.0)), bark)];
    parts.extend(root_flare(0.07, 3, 0.09, bark, 0.6));

    // Stacked foliage blobs forming the flame. (centre-y, radius, tone) — slight x/z drift
    // per blob (via the index) keeps the column from being a dead-straight pole of spheres.
    let column: [(f32, f32, u32); 7] = [
        (0.34, 0.22, FOLIAGE_DARK),
        (0.58, 0.27, FOLIAGE_DARK),
        (0.82, 0.28, FOLIAGE_MID),
        (1.06, 0.26, FOLIAGE_MID),
        (1.30, 0.22, FOLIAGE_LIGHT),
        (1.54, 0.17, FOLIAGE_LIGHT),
        (1.76, 0.11, FOLIAGE_LIGHT),
    ];
    for (i, (y, r, tone)) in column.iter().enumerate() {
        let drift = (i as f32) * 1.7; // cheap per-blob angle for a tiny off-axis nudge
        let c = Vec3::new(drift.cos() * 0.03, *y, drift.sin() * 0.03);
        parts.push(tinted(foliage(*r, 1, c), lin(*tone)));
    }
    merged(parts)
}

// ── Stump — a waist-high sawn snag with a pale ringed cut face + flared roots ────────────
//
// Ground-level forest detail (the cut stumps in the reference image): a stout tapering bole
// capped by a slightly-proud pale heartwood disc, with root flares gripping the turf. No
// foliage — scatters as walkable decor, not a tree. ~0.3u tall.
fn build_stump() -> Mesh {
    let bark = lin(TREE_TRUNK);
    let cut = lin(CUT_WOOD);
    let h = 0.26;
    let mut parts = vec![
        // Stout tapering bole.
        tinted(trunk_part(0.16, 0.20, h, 8, Vec3::new(0.0, h * 0.5, 0.0)), bark),
        // Pale sawn face: a thin disc sitting just proud of the bole top.
        tinted(
            Cylinder::new(0.165, 0.03)
                .mesh()
                .resolution(8)
                .build()
                .translated_by(Vec3::new(0.0, h + 0.005, 0.0)),
            cut,
        ),
    ];
    parts.extend(root_flare(0.20, 4, 0.16, bark, 0.3));
    merged(parts)
}

/// Per-instance trunk/foliage collision radius (UNIT scale) for a scattered tree, derived from
/// its own mesh silhouette so the blocker matches the KIND: a wide low-canopy broadleaf or a
/// skirted pine stops you near its leaves (you sink in only a little), while a slim poplar or an
/// airy birch lets you walk right up to the trunk. Sampled over a LOW height band (below an airy
/// crown) so a tall tree's high canopy — which you walk *under* — never inflates the footprint.
/// Computed once at scene build; the registered blocker stays a single circle, so there is no
/// per-frame cost (collision queries are unchanged).
pub fn silhouette_block_radius(mesh: &Mesh) -> f32 {
    // Knee-to-chest band of the unit-space mesh: catches a wide conifer skirt, the trunk, and the
    // underside of the broadleaf's high clustered crown, but stays below the airy birch crown
    // (~0.98) you walk under. Raised from 0.60 once the broadleaf crown floated high on a tall bole.
    const LO: f32 = 0.05;
    const HI: f32 = 0.85;
    let Some(pos) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|p| p.as_float3()) else {
        return 0.20; // no positions → fall back to the old flat trunk radius
    };
    let mut radii: Vec<f32> = pos
        .iter()
        .filter(|p| p[1] >= LO && p[1] <= HI)
        .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
        .collect();
    if radii.is_empty() {
        return 0.20;
    }
    radii.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // 85th-percentile reach — tracks the bulk of the low silhouette while a few stray twig /
    // branch / fallen-limb / root-flare verts can't blow the footprint up.
    let p = radii[((radii.len() - 1) * 85 / 100).min(radii.len() - 1)];
    // Pull in a touch (the hero may sink slightly into leaves — "a bit, but minimally") and clamp:
    // never below a slim-trunk bump, never past the blockers neighbour-scan bound (≤1.0, leaving
    // headroom for the per-instance scale-up).
    (p * 0.85).clamp(0.13, 0.85)
}

/// Debug screenshot hook: `FOREST_TREELINE="x,z"` parks one of every `TreeKind` in a row at
/// the given world XZ for model close-ups (mirrors `FOREST_ORKLINE`). Spawned at 2× against
/// the same white vertex-colour material the scatter uses.
pub struct TreeDebugPlugin;
impl Plugin for TreeDebugPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("FOREST_TREELINE").is_ok() {
            app.add_systems(Startup, spawn_treeline);
        }
    }
}

fn spawn_treeline(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(s) = std::env::var("FOREST_TREELINE") else { return };
    let p: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    if p.len() != 2 {
        return;
    }
    let mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.62,
        reflectance: 0.5,
        ..default()
    });
    let kinds = [
        TreeKind::Broadleaf,
        TreeKind::Birch,
        TreeKind::Pine,
        TreeKind::Poplar,
        TreeKind::Autumn,
        TreeKind::Dead,
        TreeKind::Stump,
    ];
    for (i, k) in kinds.iter().enumerate() {
        let x = p[0] + i as f32 * 3.0 - (kinds.len() as f32 - 1.0) * 1.5;
        let z = p[1];
        let y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
        commands.spawn((
            Mesh3d(meshes.add(build_tree_mesh(*k))),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(Vec3::new(x, y, z)).with_scale(Vec3::splat(2.0)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radius(k: TreeKind) -> f32 {
        silhouette_block_radius(&build_tree_mesh(k))
    }

    /// The whole point of the per-kind footprint: the wide-skirted conifer blocks widest, while
    /// the airy birch (slim pale trunk, crown floating above the sample band) lets you walk right
    /// up to it. Guards against a regression back to one flat radius for all kinds.
    #[test]
    fn block_radius_tracks_silhouette_per_kind() {
        let broadleaf = radius(TreeKind::Broadleaf);
        let autumn = radius(TreeKind::Autumn);
        let pine = radius(TreeKind::Pine);
        let poplar = radius(TreeKind::Poplar);
        let birch = radius(TreeKind::Birch);
        eprintln!(
            "tree block radii — broadleaf {broadleaf:.3} autumn {autumn:.3} pine {pine:.3} \
             poplar {poplar:.3} birch {birch:.3}"
        );

        // Every kind stays within the registrable single-circle bound.
        for v in [broadleaf, autumn, pine, poplar, birch] {
            assert!((0.13..=0.85).contains(&v), "radius {v} out of range");
        }
        // The wide low conifer skirt blocks wider than any slim silhouette.
        assert!(pine > birch + 0.04, "pine {pine} ≯ birch {birch}");
        assert!(pine > poplar, "pine {pine} ≯ poplar {poplar}");
        // The airy birch stays a slim silhouette — its pale trunk plus the one low side leaf-puff
        // (the high crown floats above the sample band), so it blocks far narrower than the conifer.
        assert!(birch <= 0.32, "birch {birch} should stay slim");
        assert!(pine > birch + 0.08, "pine {pine} should dwarf birch {birch}");
    }
}
