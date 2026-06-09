//! Swamp / Bagno biome (key 5) — a murky wetland. Dark olive green-brown ground with a
//! high-variation wet mottle; dim, desaturated greenish daylight under DENSE green-grey
//! fog; a murky green river. The scatter is built from bespoke wetland props authored
//! right in this module (no shared `trees`/`props` reuse — the swamp wants its own
//! gnarled, drowned look):
//!
//!   * GNARLED MANGROVE / dead swamp tree (tree class) — a twisted dark trunk on a small
//!     stilt-root flare, a few drooping bare limbs, sparse dark-green canopy clumps and
//!     strands of hanging grey-green moss.
//!   * CYPRESS-KNEE / rotten stump variant — a knobbly mossy stump ringed by little
//!     cypress "knees" poking up out of the muck.
//!   * CATTAIL REED clump (first non-tree class → the tree spacing fallback) — green
//!     stalks fanning out, half topped with a brown seed head.
//!   * Swamp TOADSTOOLS / shelf fungus + a mossy boulder, as the small accent class.
//!
//! Ground cover: moss patches, reed sprigs, swamp mushrooms and flat lily-pad-ish discs.
//! Particle: drifting low Mist. Backdrop: a dark misty conifer treeline over low murky
//! hills (no ocean — the land arc fills most of the horizon). Landmark: a big hollow dead
//! tree on the land side with a knot of glowing greenish will-o'-wisp motes hovering over
//! the muck beside it.
//!
//! CONTRACT (mirrors `biome_forest.rs` + the mesh modules): every prop is ONE merged,
//! vertex-coloured mesh, base at y=0, built from `tinted` primitive parts merged via
//! `Mesh::merge` then `flat_shaded` for crisp low-poly facets. The scatter draws them all
//! against the shared white vertex-colour material, so colour lives in `ATTRIBUTE_COLOR`.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::{
    Backdrop, Biome, BiomeConfig, BiomeEntity, GroundDetail, ParticleKind, PropClass,
};
use crate::palette::lin;

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;
const FRAC_PI_2: f32 = std::f32::consts::FRAC_PI_2;

/// Mangroves are authored ~1.4u tall; scale up so they loom at eye level like the forest.
const TREE_SCALE: f32 = 1.7;

// ── Swamp palette (murky, desaturated — deep olive greens + rotting browns) ──────────
const MANGROVE_BARK: u32 = 0x3b2c20; // twisted near-black swamp-wood trunk
const MANGROVE_BARK_DK: u32 = 0x281d14; // shadowed underside / drooping limbs
const MANGROVE_ROOT: u32 = 0x322619; // stilt-root flare, a touch warmer
const CANOPY_DARK: u32 = 0x2a4a2c; // sparse deep swamp-green canopy clump
const CANOPY_MID: u32 = 0x37623a; // a slightly lighter canopy lobe
const HANGING_MOSS: u32 = 0x6f7e54; // grey-green Spanish-moss strands

const STUMP_BARK: u32 = 0x4a3826; // rotten cypress stump bark
const STUMP_TOP: u32 = 0x5f4a30; // damp cut-top wood
const STUMP_MOSS: u32 = 0x4d6e3a; // moss cushion on the stump
const KNEE_WOOD: u32 = 0x42301f; // cypress-knee root knob

const REED_STALK: u32 = 0x6b8a44; // marsh reed green (TS REED_MAT)
const REED_STALK_DK: u32 = 0x4f6c34; // darker reed (TS REED_DARK_MAT)
const CATTAIL_HEAD: u32 = 0x7a4a2a; // brown cattail seed head (TS CATTAIL_MAT)

const TOAD_STEM: u32 = 0xccc2a0; // pale toadstool stem
const TOAD_CAP: u32 = 0x7a5a34; // dull swamp-brown toadstool cap
const SHELF_FUNGUS: u32 = 0x9a7c4a; // ochre shelf fungus bracket
const SWAMP_ROCK: u32 = 0x57614e; // mossy grey-green boulder
const SWAMP_ROCK_MOSS: u32 = 0x47633a; // moss accent on the boulder

const LILY_PAD: u32 = 0x3a6a40; // murky lily-pad green
const LILY_PAD_EDGE: u32 = 0x294d2e; // darker pad rim
const MOSS_PATCH: u32 = 0x4a6a38; // ground moss carpet patch
const BOG_COTTON: u32 = 0xeef0e6; // fluffy white bog-cotton head
const SWAMP_FLOWER: u32 = 0xcdb6e6; // pale lilac marsh flower
const SWAMP_FLOWER_CORE: u32 = 0xe8d27a; // pale gold flower centre

// Will-o'-wisp glow (sickly green; sRGB → linear in the emissive so it blooms).
const WISP_GLOW: Color = Color::srgb(0.45, 1.0, 0.55);
const WISP_EMISSIVE: f32 = 55.0;

// Glowing mushroom cluster — pale cool stems (on the shared white mat) under
// bioluminescent caps (their own emissive mat so they bloom, like the wisps).
const GLOWMUSH_STEM: u32 = 0xcfe8e0; // pale cool stem
const GLOWMUSH_GLOW: Color = Color::srgb(0.35, 1.0, 0.82); // teal-green cap glow
const GLOWMUSH_EMISSIVE: f32 = 26.0;
/// Local layout of one cluster: (dx, dz, scale) per mushroom. Shared by the stem + cap
/// builders so caps land exactly on their stems.
const GLOWMUSH_SPOTS: [(f32, f32, f32); 5] = [
    (0.0, 0.0, 1.3),
    (0.16, 0.06, 0.9),
    (-0.13, 0.10, 0.8),
    (0.05, -0.14, 0.7),
    (-0.08, -0.05, 0.55),
];

// ── Mesh helpers (verbatim from trees.rs / decor.rs) ─────────────────────────────────

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// Tag every vertex with one flat linear colour (REQUIRED before merge — all merged
/// parts must carry the same attribute set, incl. `ATTRIBUTE_COLOR`).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge pre-`tinted` parts into ONE mesh so identical props batch into one draw call.
/// `Mesh::merge` returns `Result` in 0.18 — `.expect` on an attribute mismatch.
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("swamp parts share attributes");
    }
    base
}

/// Un-index + recompute per-face normals → crisp flat-shaded facets. MUST be called LAST,
/// on the merged mesh (`compute_flat_normals` panics on an indexed mesh, so dup first).
fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// A faceted icosphere blob (ico detail 0), optionally squashed, centred at `off`.
fn ball_at(r: f32, off: Vec3, squash: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        lin(c),
    )
}

/// An upright cylinder whose centre sits at `cy` (a part of height `h` rooted at y=0 uses
/// `cy = h/2`). `res` ≥ 3 (the Cylinder builder asserts resolution > 2).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), lin(c))
}

// ── Prop builders ────────────────────────────────────────────────────────────────────

/// **Gnarled mangrove / dead swamp tree** — the tree class. A twisted dark trunk lifting
/// off a low knot of stilt roots, a few drooping bare limbs, a couple of sparse dark-green
/// canopy clumps and several strands of hanging grey-green moss dangling off the limbs.
/// Authored ~1.4u tall, base flush at y=0. Two variants vary lean / canopy fullness.
fn build_mangrove_mesh(variant: u32) -> Mesh {
    let lean = if variant == 0 { 0.10_f32 } else { -0.14 };
    let mut parts: Vec<Mesh> = Vec::new();

    // ── Stilt-root flare — three short angled prop-roots splaying out at the base, so the
    // trunk reads as standing up out of the muck on legs.
    for i in 0..3 {
        let a = (i as f32 / 3.0) * TAU + 0.4;
        let root = Cylinder::new(0.05, 0.34)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(0.17))
            // splay outward: lean then yaw around the base
            .rotated_by(Quat::from_rotation_z(0.66))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(Vec3::new(a.cos() * 0.10, 0.02, a.sin() * 0.10));
        parts.push(tinted(root, lin(MANGROVE_ROOT)));
    }
    // A low knotty boss where the roots gather.
    parts.push(ball_at(0.15, y(0.16), 0.85, MANGROVE_ROOT));

    // ── Trunk — a leaning column in two stacked cylinders for a slight crook.
    let lower = Cylinder::new(0.10, 0.62)
        .mesh()
        .resolution(6)
        .build()
        .translated_by(y(0.31))
        .rotated_by(Quat::from_rotation_z(lean))
        .translated_by(y(0.14)); // lift so the leaned base still clears y=0
    parts.push(tinted(lower, lin(MANGROVE_BARK)));
    // Upper trunk crooks back the other way for a gnarled silhouette.
    let upper_base = Quat::from_rotation_z(lean) * Vec3::new(0.0, 0.62, 0.0) + y(0.14);
    let upper = Cylinder::new(0.075, 0.5)
        .mesh()
        .resolution(6)
        .build()
        .translated_by(y(0.25))
        .rotated_by(Quat::from_rotation_z(-lean * 1.6))
        .translated_by(upper_base);
    parts.push(tinted(upper, lin(MANGROVE_BARK)));

    // Crown anchor point (top of the upper trunk) — limbs & canopy hang off it.
    let crown = Quat::from_rotation_z(lean) * Vec3::new(0.0, 0.62, 0.0)
        + Quat::from_rotation_z(-lean * 1.6) * Vec3::new(0.0, 0.5, 0.0)
        + y(0.14);

    // ── A few drooping bare limbs radiating from the crown (thin dark cylinders, angled
    // out then sagging). Record each tip so moss can hang off it.
    let mut limb_tips: Vec<Vec3> = Vec::new();
    let limbs = [
        (0.5_f32, 0.9_f32, 0.34_f32), // (yaw, droop, length)
        (2.4, 0.7, 0.30),
        (4.3, 1.0, 0.28),
        (3.4, 0.55, 0.24),
    ];
    for (i, &(yaw, droop, len)) in limbs.iter().enumerate() {
        // Build along +Y, tilt past horizontal so it droops (FRAC_PI_2 - 0.2 + droop),
        // yaw out, then translate to the crown.
        let tilt = Quat::from_rotation_z(FRAC_PI_2 - 0.2 + droop);
        let spin = Quat::from_rotation_y(yaw);
        let limb = Cylinder::new(0.026, len)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(tilt)
            .rotated_by(spin)
            .translated_by(crown);
        let c = if i % 2 == 0 { MANGROVE_BARK } else { MANGROVE_BARK_DK };
        parts.push(tinted(limb, lin(c)));
        // The far tip of the limb in world space.
        let tip = crown + spin * (tilt * Vec3::new(0.0, len, 0.0));
        limb_tips.push(tip);
    }

    // ── Sparse canopy — a couple of dark-green clumps up near the crown (drowned, thin).
    parts.push(ball_at(0.26, crown + y(0.06), 0.8, CANOPY_DARK));
    parts.push(ball_at(0.18, crown + Vec3::new(0.16, 0.14, -0.08), 0.8, CANOPY_MID));
    if variant == 0 {
        parts.push(ball_at(0.16, crown + Vec3::new(-0.18, 0.04, 0.10), 0.8, CANOPY_DARK));
    }

    // ── Hanging moss — thin grey-green strands dangling straight down off the limb tips.
    for (i, &tip) in limb_tips.iter().enumerate() {
        if i == 3 {
            continue; // leave one limb bare for an asymmetric look
        }
        let hang = 0.22 + (i as f32) * 0.04;
        // A thin tapered cone pointing DOWN (rotate the upright cone PI about X).
        let strand = Cone { radius: 0.018, height: hang }
            .mesh()
            .build()
            .translated_by(y(hang * 0.5))
            .rotated_by(Quat::from_rotation_x(PI))
            .translated_by(tip);
        parts.push(tinted(strand, lin(HANGING_MOSS)));
        // A little wispy blob at the bottom of the strand.
        parts.push(ball_at(0.05, tip - y(hang), 0.6, HANGING_MOSS));
    }

    flat_shaded(merged(parts))
}

/// **Cypress-knee stump** — the second tree-class variant: a knobbly mossy rotten stump
/// ringed by a few little cypress "knees" (root knobs poking up out of the muck). Base at
/// y=0, ~0.4u tall.
fn build_cypress_stump_mesh() -> Mesh {
    let r = 0.32;
    let h = 0.36;
    let mut parts = vec![
        // Bark drum.
        cyl_up(r, h, h * 0.5, 9, STUMP_BARK),
        ball_at(r * 1.05, y(0.06), 0.45, STUMP_BARK), // splayed muddy foot
        // Damp irregular cut top.
        cyl_up(r * 0.94, 0.06, h + 0.01, 9, STUMP_TOP),
        // A moss cushion slumped over one side of the rim.
        ball_at(r * 0.7, Vec3::new(r * 0.45, h - 0.02, 0.0), 0.55, STUMP_MOSS),
        ball_at(r * 0.4, Vec3::new(-r * 0.4, h + 0.02, r * 0.3), 0.6, STUMP_MOSS),
    ];
    // A ring of little cypress knees rising out of the muck around the stump.
    let knees = 5;
    for i in 0..knees {
        let a = (i as f32 / knees as f32) * TAU + 0.3;
        let dist = r + 0.18 + (i % 2) as f32 * 0.10;
        let kh = 0.16 + (i % 3) as f32 * 0.06;
        let kx = a.cos() * dist;
        let kz = a.sin() * dist;
        // A short fat cone, point up, leaning slightly toward the stump.
        let knee = Cone { radius: 0.07, height: kh }
            .mesh()
            .build()
            .translated_by(y(kh * 0.5))
            .rotated_by(Quat::from_rotation_z(0.12))
            .rotated_by(Quat::from_rotation_y(a + FRAC_PI_2))
            .translated_by(Vec3::new(kx, 0.0, kz));
        parts.push(tinted(knee, lin(KNEE_WOOD)));
    }
    flat_shaded(merged(parts))
}

/// **Cattail reed clump** — the FIRST non-tree class (so it is the tree-spacing fallback).
/// A fan of tall thin green stalks leaning out, roughly half topped with a brown cattail
/// seed head. Base at y=0, ~0.8–1.0u tall so it reads against the water. Two variants vary
/// the stalk count / height.
fn build_reed_clump_mesh(variant: u32) -> Mesh {
    let count = if variant == 0 { 7 } else { 9 };
    let mut parts: Vec<Mesh> = Vec::new();
    for i in 0..count {
        let a = (i as f32 / count as f32) * TAU;
        let foot = 0.11;
        let bx = a.cos() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let bz = a.sin() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let h = 0.72 + ((i * 7) % 5) as f32 * 0.08 + variant as f32 * 0.10;
        let tilt = 0.05 + (i % 4) as f32 * 0.05;
        let foot_off = Vec3::new(bx, 0.0, bz);
        let lean = Quat::from_rotation_y(a) * Quat::from_rotation_z(tilt);
        let stalk_c = if i % 2 == 0 { REED_STALK } else { REED_STALK_DK };
        // Slender flat-shaded cone leaning out: build upright, lean (Z) then yaw (Y), shift.
        let stalk = Cone { radius: 0.020, height: h }
            .mesh()
            .build()
            .translated_by(y(h / 2.0))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(foot_off);
        parts.push(tinted(stalk, lin(stalk_c)));

        // Half the stalks carry a brown cattail seed head near the leaned-out tip.
        if i % 2 == 0 {
            let tip = lean * Vec3::new(0.0, h * 0.88, 0.0) + foot_off;
            let head = Cylinder::new(0.034, 0.18)
                .mesh()
                .resolution(7)
                .build()
                .rotated_by(lean)
                .translated_by(tip);
            parts.push(tinted(head, lin(CATTAIL_HEAD)));
        }
    }
    flat_shaded(merged(parts))
}

/// **Swamp accents** — the small accent class. Variants: a cluster of dull toadstools, a
/// mossy boulder with shelf-fungus brackets, and a tight toadstool trio. Base at y=0.
fn build_accent_mesh(variant: u32) -> Mesh {
    match variant % 3 {
        // 0 — a loose cluster of dull swamp toadstools of mixed size.
        0 => {
            let mut parts: Vec<Mesh> = Vec::new();
            let spots = [
                (0.0_f32, 0.0_f32, 1.25_f32),
                (0.13, 0.05, 0.9),
                (-0.10, 0.08, 0.7),
                (0.04, -0.12, 0.6),
            ];
            for &(dx, dz, s) in &spots {
                let stem_h = 0.11 * s;
                let cap_r = 0.085 * s;
                parts.push(
                    cyl_up(0.03 * s, stem_h, stem_h * 0.5, 6, TOAD_STEM)
                        .translated_by(Vec3::new(dx, 0.0, dz)),
                );
                parts.push(ball_at(cap_r, Vec3::new(dx, stem_h, dz), 0.6, TOAD_CAP));
            }
            flat_shaded(merged(parts))
        }
        // 1 — a mossy boulder with a couple of shelf-fungus brackets on its shaded side.
        1 => {
            let mut parts = vec![
                ball_at(0.30, y(0.20), 0.82, SWAMP_ROCK),
                ball_at(0.17, Vec3::new(0.22, 0.14, 0.06), 0.85, SWAMP_ROCK),
                ball_at(0.14, Vec3::new(-0.18, 0.12, -0.10), 0.85, SWAMP_ROCK),
                // Moss cap catching what little light there is.
                ball_at(0.20, y(0.34), 0.55, SWAMP_ROCK_MOSS),
            ];
            // Two flat shelf-fungus brackets jutting off one side (squashed thin discs).
            for &(hy, hr, dx) in &[(0.18_f32, 0.14_f32, 0.28_f32), (0.30, 0.10, 0.24)] {
                let shelf = Cylinder::new(hr, 0.03)
                    .mesh()
                    .resolution(10)
                    .build()
                    .scaled_by(Vec3::new(1.0, 1.0, 0.6))
                    .translated_by(Vec3::new(dx, hy, 0.0));
                parts.push(tinted(shelf, lin(SHELF_FUNGUS)));
            }
            flat_shaded(merged(parts))
        }
        // 2 — a tight toadstool trio (taller, paler stems).
        _ => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..3 {
                let a = (i as f32 / 3.0) * TAU;
                let dx = a.cos() * 0.07;
                let dz = a.sin() * 0.07;
                let s = 1.0 + (i % 2) as f32 * 0.3;
                let stem_h = 0.13 * s;
                parts.push(
                    cyl_up(0.028 * s, stem_h, stem_h * 0.5, 6, TOAD_STEM)
                        .translated_by(Vec3::new(dx, 0.0, dz)),
                );
                parts.push(ball_at(0.075 * s, Vec3::new(dx, stem_h, dz), 0.55, TOAD_CAP));
            }
            flat_shaded(merged(parts))
        }
    }
}

// ── Ground-cover builders (small, flat dressing) ─────────────────────────────────────

/// A low moss patch — a clump of very squashed green balls hugging the ground (~0.06u).
fn build_moss_patch_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    let lobes = [
        (0.0_f32, 0.0_f32, 0.13_f32),
        (0.12, 0.04, 0.10),
        (-0.10, 0.08, 0.09),
        (0.05, -0.11, 0.08),
    ];
    for &(dx, dz, r) in &lobes {
        parts.push(ball_at(r, Vec3::new(dx, 0.03, dz), 0.28, MOSS_PATCH));
    }
    flat_shaded(merged(parts))
}

/// A small reed sprig — 4 short blades fanned out, for sub-cell cover near the water.
fn build_reed_sprig_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    for i in 0..4 {
        let a = (i as f32 / 4.0) * TAU;
        let h = 0.30 + (i % 2) as f32 * 0.08;
        let blade = Cone { radius: 0.016, height: h }
            .mesh()
            .build()
            .translated_by(y(h / 2.0))
            .rotated_by(Quat::from_rotation_z(0.14))
            .rotated_by(Quat::from_rotation_y(a));
        let c = if i % 2 == 0 { REED_STALK } else { REED_STALK_DK };
        parts.push(tinted(blade, lin(c)));
    }
    flat_shaded(merged(parts))
}

/// A single small swamp mushroom (pale stem + dull brown cap), ~0.14u tall.
fn build_swamp_mushroom_mesh() -> Mesh {
    let stem_h = 0.09;
    flat_shaded(merged(vec![
        cyl_up(0.026, stem_h, stem_h * 0.5, 6, TOAD_STEM),
        ball_at(0.07, y(stem_h), 0.55, TOAD_CAP),
    ]))
}

/// **Swamp ground accent** (cover). `variant`: 0 = bog cotton (green stems topped with
/// fluffy white heads), 1 = a pale lilac marsh flower (petal ring + gold core). The soft
/// pale touches that lift the murky floor. Base at y=0, ~0.14–0.24u tall.
fn build_swamp_cover_extra_mesh(variant: u32) -> Mesh {
    match variant % 2 {
        // Bog cotton — three green stems each tipped with a fluffy white head.
        0 => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..3 {
                let a = (i as f32 / 3.0) * TAU;
                let (bx, bz) = (a.cos() * 0.04, a.sin() * 0.04);
                let h = 0.18 + (i % 2) as f32 * 0.05;
                parts.push(cyl_up(0.010, h, h * 0.5, 5, REED_STALK).translated_by(Vec3::new(bx, 0.0, bz)));
                parts.push(ball_at(0.04, Vec3::new(bx, h, bz), 0.85, BOG_COTTON));
            }
            flat_shaded(merged(parts))
        }
        // Pale lilac marsh flower — a slim stem, gold core, ring of pale petals.
        _ => {
            let head_y = 0.14;
            let mut parts = vec![
                tinted(
                    Cone { radius: 0.009, height: head_y }.mesh().build().translated_by(y(head_y * 0.5)),
                    lin(REED_STALK_DK),
                ),
                ball_at(0.016, y(head_y), 0.7, SWAMP_FLOWER_CORE),
            ];
            for i in 0..5 {
                let a = (i as f32 / 5.0) * TAU;
                parts.push(ball_at(0.026, Vec3::new(a.cos() * 0.038, head_y, a.sin() * 0.038), 0.5, SWAMP_FLOWER));
            }
            flat_shaded(merged(parts))
        }
    }
}

/// A flat lily-pad-ish disc — a darker rim under a green top, lying flat on the muck.
/// The `Circle` mesh lies in the XY plane (normal +Z); rotate −90° about X to lie flat.
fn build_lily_disc_mesh() -> Mesh {
    let flat = |m: Mesh| -> Mesh { m.rotated_by(Quat::from_rotation_x(-FRAC_PI_2)) };
    let pad_r = 0.24;
    flat_shaded(merged(vec![
        tinted(
            flat(Circle::new(pad_r).mesh().resolution(12).build()).translated_by(y(0.004)),
            lin(LILY_PAD_EDGE),
        ),
        tinted(
            flat(Circle::new(pad_r * 0.88).mesh().resolution(12).build()).translated_by(y(0.012)),
            lin(LILY_PAD),
        ),
    ]))
}

/// **Big hollow dead swamp tree** (landmark) — a wide hollow trunk (an arc of thick bark
/// slabs leaving a dark gap), a jagged broken top, a couple of stubbed limbs and moss.
/// Base at y=0, ~3u tall, authored at full scale (the landmark spawns it un-scaled).
fn build_hollow_dead_tree_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    let trunk_h = 2.6;
    let radius = 0.55;
    // Four thick bark slabs bowed around the trunk axis, leaving a gap (the hollow).
    let slabs = [
        (0.9_f32, 1.0_f32), // (yaw centre, width factor)
        (2.3, 0.9),
        (3.9, 1.0),
        (5.0, 0.8),
    ];
    for &(yaw, wf) in &slabs {
        let slab = Cuboid::new(0.34 * wf, trunk_h, 0.30)
            .mesh()
            .build()
            .translated_by(y(trunk_h * 0.5))
            .translated_by(Vec3::new(0.0, 0.0, radius)) // push out onto the trunk ring
            .rotated_by(Quat::from_rotation_y(yaw));
        parts.push(tinted(slab, lin(MANGROVE_BARK)));
    }
    // Flared mossy base so it sits in the muck convincingly.
    parts.push(ball_at(radius * 1.4, y(0.18), 0.4, MANGROVE_ROOT));
    parts.push(ball_at(radius * 1.1, Vec3::new(radius, 0.16, radius * 0.4), 0.5, MANGROVE_ROOT));

    // Jagged broken top — tall thin shards of differing height around the rim.
    for i in 0..5 {
        let a = (i as f32 / 5.0) * TAU + 0.5;
        let sh = 0.5 + (i % 3) as f32 * 0.35;
        let shard = Cone { radius: 0.18, height: sh }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(sh * 0.5))
            .translated_by(Vec3::new(a.cos() * radius * 0.7, trunk_h, a.sin() * radius * 0.7));
        parts.push(tinted(shard, lin(MANGROVE_BARK_DK)));
    }
    // Two stubbed broken limbs jutting out partway up.
    for &(yaw, hgt, len) in &[(0.6_f32, 1.7_f32, 0.9_f32), (3.6, 2.0, 0.7)] {
        let limb = Cylinder::new(0.10, len)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(Quat::from_rotation_z(FRAC_PI_2 - 0.3))
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(y(hgt));
        parts.push(tinted(limb, lin(MANGROVE_BARK)));
    }
    // Moss clinging up one side.
    parts.push(ball_at(0.32, Vec3::new(-radius * 0.6, 1.0, radius * 0.3), 0.7, STUMP_MOSS));
    parts.push(ball_at(0.24, Vec3::new(-radius * 0.5, 1.8, radius * 0.2), 0.7, STUMP_MOSS));

    flat_shaded(merged(parts))
}

// ── Glowing mushroom cluster (a swamp landmark accent, split by material) ─────────────
//
// A cluster of 5 mushrooms of mixed size. The STEMS are a separate pale vertex-coloured
// mesh (rides the shared white mat); the CAPS are a separate mesh carrying NO colour
// attribute, so an emissive glow material lights them up and feeds bloom. The two meshes
// share `GLOWMUSH_SPOTS`, so a cap sits exactly atop each stem when spawned at one
// transform. Base flush at y=0.

/// Pale stems for the glowmush cluster (shared white vertex-colour mat).
fn build_glowmush_stems_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    for &(dx, dz, s) in &GLOWMUSH_SPOTS {
        let sh = 0.14 * s;
        parts.push(cyl_up(0.03 * s, sh, sh * 0.5, 6, GLOWMUSH_STEM).translated_by(Vec3::new(dx, 0.0, dz)));
    }
    flat_shaded(merged(parts))
}

/// Glowing caps for the glowmush cluster — domed squashed blobs with NO colour attribute
/// (the emissive material owns the colour). Built to match the stem layout/heights.
fn build_glowmush_caps_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    for &(dx, dz, s) in &GLOWMUSH_SPOTS {
        let sh = 0.14 * s;
        parts.push(
            Sphere::new(0.085 * s)
                .mesh()
                .ico(0)
                .expect("ico detail in range")
                .scaled_by(Vec3::new(1.0, 0.6, 1.0))
                .translated_by(Vec3::new(dx, sh, dz)),
        );
    }
    // Merge raw (no ATTRIBUTE_COLOR on any part) then flat-shade for crisp facets.
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one cap");
    for p in it {
        base.merge(&p).expect("glowmush caps share attributes");
    }
    flat_shaded(base)
}

// ── BiomeConfig ──────────────────────────────────────────────────────────────────────

pub fn config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Swamp,
        name: "Swamp",

        // Dark olive green-brown wet ground. Low roughness so the dim sun throws a damp
        // specular sheen across the muck — reads as standing bog-water rather than dry dirt.
        ground_color: 0x49543a,
        ground_roughness: 0.25,
        detail: GroundDetail {
            scale: 0.16,
            strength: 0.55,
            variation: 0.95, // high → blotchy wet/dry mottle
            seed: 11.0,
            dark: 0x2c3522, // muddy shadow pools
            base: 0x49543a, // olive base
            light: 0x687a4a, // mossy highlights
            grain: 0.7,
            streak: 0.6,
        },

        // Dim, desaturated greenish daylight under dense green-grey fog.
        sky: 0x8a968a,
        fog_density: 0.030,
        sun_color: 0xc7d0b0,
        sun_illuminance: 7_200.0,
        ambient_color: 0xaebaa6,
        ambient_brightness: 70.0,
        sun_pos: Vec3::new(12.0, 30.0, 14.0),

        seed: 5005,
        tree_min_dist: 2.6,
        classes: vec![
            // Trees: 70% gnarled mangrove (2 variants) / 30% cypress-knee stump.
            PropClass {
                variants: vec![
                    (build_mangrove_mesh(0), 0.42),
                    (build_mangrove_mesh(1), 0.28),
                    (build_cypress_stump_mesh(), 0.30),
                ],
                chance: 0.085,
                scale: (0.85 * TREE_SCALE, 1.25 * TREE_SCALE),
                tree: true,
                block_radius: 0.0,
            },
            // Cattail reed clumps — FIRST non-tree class → the tree-spacing fallback.
            PropClass {
                variants: vec![
                    (build_reed_clump_mesh(0), 1.0),
                    (build_reed_clump_mesh(1), 1.0),
                ],
                chance: 0.06,
                scale: (0.85, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            // Toadstools / shelf fungus / mossy rock accents.
            PropClass {
                variants: (0..3).map(|v| (build_accent_mesh(v), 1.0)).collect(),
                chance: 0.04,
                scale: (0.7, 1.5),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover: vec![
            PropClass {
                variants: vec![(build_moss_patch_mesh(), 1.0)],
                chance: 0.34,
                scale: (0.7, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_reed_sprig_mesh(), 1.0)],
                chance: 0.18,
                scale: (0.7, 1.2),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_swamp_mushroom_mesh(), 1.0)],
                chance: 0.12,
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_lily_disc_mesh(), 1.0)],
                chance: 0.10,
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
            // Soft pale floor accents — bog cotton + lilac marsh flowers.
            PropClass {
                variants: (0..2).map(|v| (build_swamp_cover_extra_mesh(v), 1.0)).collect(),
                chance: 0.10,
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover_per_tile: 2,

        river: true,
        river_color: 0x3f5a44, // murky green swamp water
        backdrop: Backdrop {
            land_dir: -FRAC_PI_2,
            land_arc: PI * 0.62, // land wraps most of the horizon
            ocean: false,
            ocean_color: 0x2f5a4a,
            // Low murky hills (desaturated grey-greens).
            hill_body: 0x5a6452,
            hill_cap: 0x76806a,
            hill_foot: 0x474f40,
            // Dark misty conifer treeline ringing the marsh.
            treeline: true,
            treeline_dark: 0x223526,
            treeline_mid: 0x2e4530,
            hill_h: (26.0, 58.0),
        },
        // No weather: the flat-disc Mist read as hard-edged translucent shards from a low
        // camera. Left as `None` until a soft volumetric-ish swamp haze exists.
        particle: ParticleKind::None,
    }
}

// ── Landmarks ────────────────────────────────────────────────────────────────────────

/// Big hollow dead tree on the land side + a knot of glowing greenish will-o'-wisp motes
/// hovering over the muck beside it. Every spawn is tagged [`BiomeEntity`] so the biome
/// switch wipes it. The motes are unlit emissive spheres so they glow against the dim
/// swamp light and feed bloom; they hover at a fixed ~1u height (no animation system — the
/// module is self-contained and registers no plugin).
pub fn landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Shared white vertex-colour material for the dead-tree set-piece.
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });

    // The hollow dead tree, planted on the land side (z < 0).
    commands.spawn((
        Mesh3d(meshes.add(build_hollow_dead_tree_mesh())),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(-9.0, 0.0, -11.0).with_rotation(Quat::from_rotation_y(0.5)),
        BiomeEntity,
    ));

    // ── Will-o'-wisp motes — small unlit emissive greenish spheres hovering ~1u up in a
    // loose knot near the dead tree, over the muck on the land side.
    let wisp_mesh = meshes.add(Sphere::new(0.07).mesh().ico(1).expect("ico detail in range"));
    let wisp_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 1.0, 0.6),
        emissive: LinearRgba::from(WISP_GLOW) * WISP_EMISSIVE,
        unlit: true,
        ..default()
    });

    // A small deterministic spread of motes around two clusters near the dead tree / water.
    let centres = [Vec3::new(-6.5, 0.0, -7.5), Vec3::new(-11.5, 0.0, -9.0)];
    // (cluster index, dx, dz, height)
    let motes = [
        (0usize, 0.0_f32, 0.0_f32, 1.00_f32),
        (0, 1.4, -0.8, 1.30),
        (0, -1.1, 1.0, 0.85),
        (0, 0.8, 1.5, 1.15),
        (1, 0.0, 0.0, 1.10),
        (1, -1.3, -0.6, 0.90),
        (1, 1.2, 0.9, 1.25),
    ];
    for &(ci, dx, dz, hy) in &motes {
        let pos = centres[ci] + Vec3::new(dx, hy, dz);
        commands.spawn((
            Mesh3d(wisp_mesh.clone()),
            MeshMaterial3d(wisp_mat.clone()),
            Transform::from_translation(pos),
            NotShadowCaster,
            BiomeEntity,
        ));
    }

    // ── Glowing mushroom clusters — pale stems (shared white mat) under bioluminescent
    // caps (emissive mat → bloom). Spread across the patch with a local Mulberry32 RNG,
    // skipping the river column and the open centre framing. ~14 clusters.
    let glow_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.40, 0.95, 0.82),
        emissive: LinearRgba::from(GLOWMUSH_GLOW) * GLOWMUSH_EMISSIVE,
        unlit: true,
        ..default()
    });
    let stems = meshes.add(build_glowmush_stems_mesh());
    let caps = meshes.add(build_glowmush_caps_mesh());

    let mut seed = 0x51ed_2a17_u32;
    let mut next = || {
        seed = seed.wrapping_add(0x6d2b_79f5);
        let mut t = seed;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    };
    let mut placed = 0;
    for _ in 0..160 {
        if placed >= 14 {
            break;
        }
        let x = -14.0 + next() * 28.0;
        let z = -14.0 + next() * 28.0;
        // Skip the river band and the open framing in front of the camera.
        if crate::water::on_river(x, z) || (x * x + z * z) < 9.0 {
            continue;
        }
        let tf = Transform {
            translation: Vec3::new(x, 0.0, z),
            rotation: Quat::from_rotation_y(next() * TAU),
            scale: Vec3::splat(0.85 + next() * 0.7),
        };
        commands.spawn((Mesh3d(stems.clone()), MeshMaterial3d(mat.clone()), tf, BiomeEntity));
        commands.spawn((
            Mesh3d(caps.clone()),
            MeshMaterial3d(glow_mat.clone()),
            tf,
            NotShadowCaster,
            BiomeEntity,
        ));
        placed += 1;
    }
}
