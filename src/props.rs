//! Rocks + bushes — the forest-biome props that dress the scatter between trees.
//!
//! CONTRACT: each `build_*` returns ONE merged, vertex-coloured `Mesh`, base at y=0,
//! against the shared white vertex-colour material. `variant` in `0..NUM_*` selects a
//! shape so the scatter can vary props. See
//! `docs/specs/forest-biome-props-ground-cover-exact-bevy-rebuild.md`.
//!
//! Both props are built from faceted icospheres (`.mesh().ico(0|1)`) merged into a
//! single mesh. Rocks are flat-shaded angular boulders (chipped-stone look) in mossy
//! grey-greens; bushes are rounded shrubs of overlapping squashed balls in layered
//! greens (dark skirt → mid body → bright crown) so the foliage reads lush and 3D,
//! never a single flat sphere. Colours come from the TS spec; the Bevy mesh/merge
//! API comes from the verified-APIs doc §9.

use bevy::prelude::*;

use crate::palette::{lin, lin_scaled};
use crate::meshkit::{flat_shaded, merged, tinted};

// Bush greens (TS BUSH_MATS, forest-props spec lines 125-127 / 433-441):
//   dark #3a8a3a, mid #4aa84a, light #65bb55. Used as the three bush variants' base
//   tone; each variant fans out into its own dark/mid/light tier set around its base.
const BUSH_DARK: u32 = 0x3a8a3a;
const BUSH_MID: u32 = 0x4aa84a;
const BUSH_LIGHT: u32 = 0x65bb55;

// Rock tones. The TS rock is a flat light grey (#d3d3d3); the task asks for mossy
// grey-green low-poly boulders, so shift the stone toward a muted moss-grey and pair
// it with a slightly cooler/greener accent for the clustered lumps. A darker base +
// lighter top facet reads as a lit boulder.
const ROCK_STONE: u32 = 0x9aa090; // muted grey-green stone body
const ROCK_MOSS: u32 = 0x7d8a6a; // greener, mossier accent lump
const ROCK_LICHEN: u32 = 0x8a8f80; // neutral cool grey accent

/// Rock shape variants (different proportions / lump clusters).
pub const NUM_ROCK_VARIANTS: u32 = 3;
/// Bush colour/shape variants (TS has 3 — dark / mid / light green).
pub const NUM_BUSH_VARIANTS: u32 = 3;

// ─── Mesh helpers (verified 0.18 API, mirrors the working reference in
// D:/tileworld-bevy/crates/game/src/map_props.rs) ───────────────────────────────

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// A smooth-ish squashed icosphere blob (ico detail 1) for foliage — rounder, fuller
/// canopy lobes. `squash` < 1 flattens it vertically into a dome.
fn ball_at(r: f32, off: Vec3, squash: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(1)
            .unwrap()
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        c,
    )
}

/// A low-poly **faceted** lump: a 20-face icosahedron (ico detail 0) with hard
/// per-face normals — the angular "chipped stone" look the TS rocks use
/// (`IcosahedronGeometry(r, 0)` + `flatShading`), not a smooth round blob.
/// `duplicate_vertices()` MUST run before `compute_flat_normals()` (the latter panics
/// on an indexed mesh — see the verified-APIs doc §9).
fn facet_at(r: f32, off: Vec3, squash: f32, c: [f32; 4]) -> Mesh {
    facet_rot(r, off, Vec3::new(1.0, squash, 1.0), Quat::IDENTITY, c)
}

/// The general faceted lump: per-axis stretch + an arbitrary pre-rotation before the
/// translate. Rotating each lump differently keeps the 20 icosahedron facets from
/// lining up across a cluster — that repeated-orientation tell is what makes prop
/// rocks read as copy-pasted spheres.
fn facet_rot(r: f32, off: Vec3, stretch: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    let mut m = Sphere::new(r)
        .mesh()
        .ico(0)
        .unwrap()
        .scaled_by(stretch)
        .rotated_by(rot)
        .translated_by(off);
    m.duplicate_vertices();
    m.compute_flat_normals();
    tinted(m, c)
}

/// A draped moss patch: a very flat green disc-blob melted over a boulder's crown.
fn moss_drape(r: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    facet_rot(r, off, Vec3::new(1.25, 0.30, 1.1), Quat::from_rotation_y(0.7), c)
}

// ─── Rocks ──────────────────────────────────────────────────────────────────────

/// **Boulder** — a low-poly rock: a big faceted lump plus a few smaller ones
/// clustered around its base for an irregular, chipped silhouette. Base sits flush at
/// y=0 (lumps offset up by their radius). Mossy grey-green stone, with the body a
/// touch darker and the top facet brighter so it reads as lit. Three variants vary
/// the proportions (squat & wide / tall & split / a low scatter of cobbles).
///
/// Authoring scale is TS-ish: the TS rock is `Icosahedron(r=0.18)` then scaled
/// 0.85–1.30 by the scatter; these boulders read at ~0.3–0.45u so they sit between
/// the ground cover and the trees, then the scatter scales them per-instance.
pub fn build_rock_mesh(variant: u32) -> Mesh {
    let rot = |x: f32, yw: f32, z: f32| Quat::from_euler(EulerRot::XYZ, x, yw, z);
    match variant % NUM_ROCK_VARIANTS {
        // Variant 0 — a squat, wide boulder: a broad tilted body + bright crown facet,
        // two leaning side cobbles, a moss drape over the shoulder and grit at the foot.
        0 => {
            let r = 0.34;
            merged(vec![
                // Body — wide, squashed, rolled a few degrees so its facets sit unique.
                facet_rot(r, y(r * 0.72), Vec3::new(1.15, 0.78, 1.0), rot(0.12, 0.5, -0.08), lin_scaled(ROCK_STONE, 0.9)),
                // A bright top facet catching the light.
                facet_rot(r * 0.5, Vec3::new(-r * 0.1, r * 1.18, r * 0.06), Vec3::new(1.0, 0.7, 0.9), rot(-0.1, 1.7, 0.15), lin_scaled(ROCK_STONE, 1.12)),
                // Two side cobbles leaning against the base.
                facet_rot(r * 0.55, Vec3::new(r * 0.92, r * 0.38, r * 0.18), Vec3::new(1.0, 0.8, 0.9), rot(0.3, 2.6, 0.2), lin_scaled(ROCK_STONE, 0.97)),
                facet_rot(r * 0.46, Vec3::new(-r * 0.78, r * 0.32, -r * 0.28), Vec3::new(0.9, 0.85, 1.05), rot(-0.2, 4.0, -0.25), lin(ROCK_LICHEN)),
                // Moss melted over the sunward shoulder + a grounded skirt of grit.
                moss_drape(r * 0.55, Vec3::new(r * 0.28, r * 1.28, -r * 0.1), lin(ROCK_MOSS)),
                facet_at(r * 0.28, Vec3::new(-r * 0.5, r * 0.16, r * 0.75), 0.55, lin_scaled(ROCK_STONE, 0.84)),
            ])
        }
        // Variant 1 — a taller, split crag: stacked rotated blocks, a moss seam in the
        // cleft, a bright peak cap and a toppled chip at the foot.
        1 => {
            let r = 0.3;
            merged(vec![
                // Lower block, tilted into the hill.
                facet_rot(r, y(r * 0.8), Vec3::new(1.1, 0.95, 0.95), rot(0.1, 0.3, 0.14), lin_scaled(ROCK_STONE, 0.88)),
                // Upper block sheared to one side (the "split").
                facet_rot(r * 0.72, Vec3::new(r * 0.36, r * 1.52, -r * 0.1), Vec3::new(0.95, 1.1, 0.9), rot(-0.12, 1.9, -0.2), lin(ROCK_LICHEN)),
                // A mossy seam wedged in the cleft.
                facet_rot(r * 0.4, Vec3::new(-r * 0.5, r * 0.72, r * 0.3), Vec3::new(1.1, 0.7, 0.9), rot(0.4, 3.2, 0.0), lin(ROCK_MOSS)),
                // Bright cap on the peak.
                facet_rot(r * 0.42, Vec3::new(r * 0.32, r * 2.1, -r * 0.05), Vec3::new(0.9, 0.8, 0.85), rot(0.2, 5.1, -0.1), lin_scaled(ROCK_STONE, 1.14)),
                // A toppled chip resting at the foot.
                facet_rot(r * 0.3, Vec3::new(r * 0.85, r * 0.22, r * 0.55), Vec3::new(1.2, 0.55, 0.9), rot(0.0, 2.2, 0.5), lin_scaled(ROCK_STONE, 1.02)),
            ])
        }
        // Variant 2 — a low scatter of cobbles: several small rotated lumps spread flat,
        // moss creeping over the two biggest.
        _ => {
            let r = 0.26;
            merged(vec![
                facet_rot(r, y(r * 0.68), Vec3::new(1.2, 0.74, 1.0), rot(0.1, 0.9, -0.1), lin_scaled(ROCK_STONE, 0.92)),
                facet_rot(r * 0.78, Vec3::new(r * 1.05, r * 0.46, r * 0.25), Vec3::new(1.0, 0.78, 1.1), rot(-0.15, 2.0, 0.2), lin(ROCK_LICHEN)),
                facet_rot(r * 0.66, Vec3::new(-r * 0.95, r * 0.4, -r * 0.4), Vec3::new(0.95, 0.78, 0.9), rot(0.25, 3.4, 0.0), lin_scaled(ROCK_STONE, 1.0)),
                facet_rot(r * 0.5, Vec3::new(r * 0.1, r * 0.38, -r * 1.0), Vec3::new(1.1, 0.8, 0.95), rot(0.0, 4.6, -0.3), lin_scaled(ROCK_STONE, 1.06)),
                facet_rot(r * 0.4, Vec3::new(-r * 0.2, r * 0.34, r * 0.95), Vec3::new(0.9, 0.8, 1.0), rot(0.3, 5.6, 0.15), lin_scaled(ROCK_STONE, 0.95)),
                moss_drape(r * 0.5, y(r * 1.02), lin(ROCK_MOSS)),
                moss_drape(r * 0.34, Vec3::new(r * 1.05, r * 0.78, r * 0.25), lin(ROCK_MOSS)),
            ])
        }
    }
}

// ─── Bushes ───────────────────────────────────────────────────────────────────────

/// **Bush** — a rounded shrub built from a cluster of overlapping squashed icospheres
/// in three layered green tiers (dark skirt → mid body → bright crown), shorter and
/// rounder than the trees, base flush at y=0. The TS bush is 4 `ico0` blobs at radii
/// 0.24/0.20/0.18/0.19 (forest-props spec lines 116-119 / 442-446); enriched here into
/// tiered tones so the canopy reads 3D and lush, not a single flat sphere (free — the
/// shared mesh is drawn many times by the scatter).
///
/// `variant` picks the bush's base green (TS BUSH_MATS): 0 dark `#3a8a3a`, 1 mid
/// `#4aa84a`, 2 light `#65bb55`. Each variant fans its base into a slightly darker
/// skirt and a slightly brighter crown so all three read as distinct but coherent.
pub fn build_bush_mesh(variant: u32) -> Mesh {
    // Base tone per variant; the skirt/crown tiers are brightness-scaled from it so the
    // tier gradient holds for every colour.
    let base = match variant % NUM_BUSH_VARIANTS {
        0 => BUSH_DARK,
        1 => BUSH_MID,
        _ => BUSH_LIGHT,
    };
    let dark = lin_scaled(base, 0.78); // grounded skirt
    let mid = lin(base); // body
    let light = lin_scaled(base, 1.16); // sunlit crown

    let mut parts = vec![
        // ── Dark base skirt — wide low lobes that give the bush its grounded spread.
        // Mirrors the TS part radii/offsets (0.24 centre + 0.20/0.18/0.19 lobes),
        // squashed into domes and kept low so the silhouette is rounder than a tree.
        ball_at(0.24, y(0.17), 0.82, dark),
        ball_at(0.2, Vec3::new(0.2, 0.14, 0.05), 0.82, dark),
        ball_at(0.18, Vec3::new(-0.17, 0.13, 0.1), 0.82, dark),
        ball_at(0.15, Vec3::new(0.02, 0.12, -0.19), 0.82, dark),
        // ── Mid body — fills the centre of the mound.
        ball_at(0.21, y(0.27), 0.86, mid),
        ball_at(0.16, Vec3::new(0.13, 0.3, -0.13), 0.86, mid),
        ball_at(0.15, Vec3::new(-0.12, 0.28, -0.06), 0.86, mid),
        // ── Bright crown — catches the sun on top, a touch above the body.
        ball_at(0.16, y(0.38), 0.9, light),
        ball_at(0.12, Vec3::new(0.09, 0.42, 0.08), 0.9, light),
        ball_at(0.11, Vec3::new(-0.08, 0.41, -0.07), 0.9, light),
    ];
    // A couple of woody twig stubs poking through the canopy — shrubs aren't solid.
    for (a, tilt, len) in [(0.7_f32, 0.5_f32, 0.14_f32), (3.6, -0.6, 0.12)] {
        parts.push(tinted(
            Cylinder::new(0.012, len)
                .mesh()
                .resolution(4)
                .build()
                .translated_by(y(len * 0.5))
                .rotated_by(Quat::from_rotation_z(tilt))
                .rotated_by(Quat::from_rotation_y(a))
                .translated_by(y(0.40)),
            lin(0x6b4a2c),
        ));
    }
    // Per-variant fruiting accent: the dark bush carries red berries, the mid one white
    // blossom dots, the light one stays plain — three shrubs, three reads.
    let accent = match variant % NUM_BUSH_VARIANTS {
        0 => Some(lin(0xc83a3a)), // holly-red berries
        1 => Some(lin(0xf2efe0)), // white blossom
        _ => None,
    };
    if let Some(acc) = accent {
        for (i, &(dx, dy, dz)) in [
            (0.14_f32, 0.40_f32, 0.06_f32),
            (-0.10, 0.42, -0.09),
            (0.03, 0.46, 0.12),
            (0.20, 0.32, -0.10),
            (-0.18, 0.34, 0.08),
        ]
        .iter()
        .enumerate()
        {
            parts.push(ball_at(0.022 + (i % 2) as f32 * 0.006, Vec3::new(dx, dy, dz), 1.0, acc));
        }
    }
    // Flat-shaded so the bush reads as crisp low-poly facets (like the TS game), not a
    // soft blob.
    flat_shaded(merged(parts))
}
