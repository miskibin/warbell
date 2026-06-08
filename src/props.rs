//! Rocks + bushes — the forest-biome props that dress the scatter between trees.
//!
//! CONTRACT: each `build_*` returns ONE merged, vertex-coloured `Mesh`, base at y=0,
//! against the shared white vertex-colour material. `variant` in `0..NUM_*` selects a
//! shape so the scatter can vary props. See
//! `docs/specs/forest-biome-props-ground-cover-exact-bevy-rebuild.md` + `CONTRACT.md`.
//!
//! Both props are built from faceted icospheres (`.mesh().ico(0|1)`) merged into a
//! single mesh. Rocks are flat-shaded angular boulders (chipped-stone look) in mossy
//! grey-greens; bushes are rounded shrubs of overlapping squashed balls in layered
//! greens (dark skirt → mid body → bright crown) so the foliage reads lush and 3D,
//! never a single flat sphere. Colours come from the TS spec; the Bevy mesh/merge
//! API comes from the contract + the verified-APIs doc.

use bevy::prelude::*;

use crate::palette::{lin, lin_scaled};

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

/// Tag every vertex of `m` with a uniform linear RGBA colour (REQUIRED before merge —
/// all merged parts must share the same attribute set, incl. ATTRIBUTE_COLOR).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge several tinted parts into one mesh (all parts share POSITION/NORMAL/UV/COLOR).
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("prop parts share attributes");
    }
    base
}

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
/// on an indexed mesh — contract §"flat-shading helpers").
fn facet_at(r: f32, off: Vec3, squash: f32, c: [f32; 4]) -> Mesh {
    let mut m = Sphere::new(r)
        .mesh()
        .ico(0)
        .unwrap()
        .scaled_by(Vec3::new(1.0, squash, 1.0))
        .translated_by(off);
    m.duplicate_vertices();
    m.compute_flat_normals();
    tinted(m, c)
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
    match variant % NUM_ROCK_VARIANTS {
        // Variant 0 — a squat, wide boulder: one broad squashed lump + two side cobbles.
        0 => {
            let r = 0.34;
            merged(vec![
                // Body — wide & squashed; slightly darkened base tone.
                facet_at(r, y(r * 0.74), 0.78, lin_scaled(ROCK_STONE, 0.9)),
                // A bright top facet catching the light.
                facet_at(r * 0.5, y(r * 1.18), 0.7, lin_scaled(ROCK_STONE, 1.12)),
                // Two side cobbles leaning against the base.
                facet_at(r * 0.55, Vec3::new(r * 0.92, r * 0.4, r * 0.18), 0.85, lin(ROCK_MOSS)),
                facet_at(r * 0.46, Vec3::new(-r * 0.78, r * 0.34, -r * 0.28), 0.85, lin(ROCK_LICHEN)),
            ])
        }
        // Variant 1 — a taller, split crag: two stacked lumps + a high bright cap.
        1 => {
            let r = 0.3;
            merged(vec![
                // Lower block.
                facet_at(r, y(r * 0.82), 0.92, lin_scaled(ROCK_STONE, 0.88)),
                // Upper block offset to one side (the "split").
                facet_at(r * 0.72, Vec3::new(r * 0.34, r * 1.5, -r * 0.1), 0.95, lin(ROCK_LICHEN)),
                // A small mossy chip wedged in the cleft.
                facet_at(r * 0.4, Vec3::new(-r * 0.5, r * 0.7, r * 0.3), 0.85, lin(ROCK_MOSS)),
                // Bright cap on the peak.
                facet_at(r * 0.42, Vec3::new(r * 0.3, r * 2.05, -r * 0.05), 0.8, lin_scaled(ROCK_STONE, 1.14)),
            ])
        }
        // Variant 2 — a low scatter of cobbles: several small lumps spread flat.
        _ => {
            let r = 0.26;
            merged(vec![
                facet_at(r, y(r * 0.7), 0.74, lin_scaled(ROCK_STONE, 0.92)),
                facet_at(r * 0.78, Vec3::new(r * 1.05, r * 0.5, r * 0.25), 0.78, lin(ROCK_LICHEN)),
                facet_at(r * 0.66, Vec3::new(-r * 0.95, r * 0.42, -r * 0.4), 0.78, lin(ROCK_MOSS)),
                facet_at(r * 0.5, Vec3::new(r * 0.1, r * 0.4, -r * 1.0), 0.8, lin_scaled(ROCK_STONE, 1.06)),
                facet_at(r * 0.4, Vec3::new(-r * 0.2, r * 0.36, r * 0.95), 0.8, lin(ROCK_MOSS)),
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

    // Flat-shaded so the bush reads as crisp low-poly facets (like the TS game), not a
    // soft blob.
    flat_shaded(merged(vec![
        // ── Dark base skirt — wide low lobes that give the bush its grounded spread.
        // Mirrors the TS part radii/offsets (0.24 centre + 0.20/0.18/0.19 lobes),
        // squashed into domes and kept low so the silhouette is rounder than a tree.
        ball_at(0.24, y(0.17), 0.82, dark),
        ball_at(0.2, Vec3::new(0.2, 0.14, 0.05), 0.82, dark),
        ball_at(0.18, Vec3::new(-0.17, 0.13, 0.1), 0.82, dark),
        // ── Mid body — fills the centre of the mound.
        ball_at(0.21, y(0.27), 0.86, mid),
        ball_at(0.16, Vec3::new(0.13, 0.3, -0.13), 0.86, mid),
        ball_at(0.15, Vec3::new(-0.12, 0.28, -0.06), 0.86, mid),
        // ── Bright crown — catches the sun on top, a touch above the body.
        ball_at(0.16, y(0.38), 0.9, light),
        ball_at(0.12, Vec3::new(0.09, 0.42, 0.08), 0.9, light),
        ball_at(0.11, Vec3::new(-0.08, 0.41, -0.07), 0.9, light),
    ]))
}

/// Un-index + recompute per-face normals so a merged mesh shows hard, flat-shaded
/// facets (the crisp low-poly look) instead of soft smooth shading.
fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}
