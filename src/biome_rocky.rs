//! Rocky highlands biome (key 3) — arid grey-brown stone country. The scatter is
//! dominated by angular faceted BOULDERS (several sizes, layered/stacked chunks),
//! punctuated by tall banded HOODOO spires (the "tree" class, spacing-checked so they
//! never crowd), low dry SHRUB / dead-grass clumps (the first non-tree class → the
//! tree-too-close fallback) and flat scatters of scree PEBBLES. Ground cover dresses
//! the dirt with little pebbles + dry tufts. The horizon is a wide arc of tall craggy
//! grey peaks (no treeline); dust drifts on the wind. River is off.
//!
//! Self-contained: every mesh is built here from primitives, tinted into ATTRIBUTE_COLOR
//! and flat-shaded for the crisp low-poly facets the rest of the scene uses. Atmosphere
//! is neutral hazy daylight. The landmark is a dramatic two-pillar ROCK ARCH plus a
//! flat-topped MESA and a balanced-rock hoodoo, all on the land side (z < 0).
//!
//! Palette (muted arid stone, lifted from the TS rock-highland feel — grey-browns with
//! warm sand undertones and pale sun-bleached caps; banded ochre/rust for the hoodoos).

// The `landmarks()` ROCK ARCH set-piece + its `arch_pillar` helper below are authored biome
// content the world map doesn't place yet (it uses `ruins` landmarks instead). Kept per design;
// allow the resulting dead code until it's wired into a per-region pass.
#![allow(dead_code)]

use bevy::prelude::*;

use crate::biome::{Backdrop, Biome, BiomeConfig, BiomeEntity, GroundDetail, ParticleKind, PropClass};
use crate::palette::{lin, lin_scaled};

// ── Rocky palette ─────────────────────────────────────────────────────────────────
// Stone body tones: a cool-warm grey-brown stack so a boulder reads lit (dark foot →
// mid body → pale sun-bleached cap). Hoodoos add warmer banded ochre/rust strata.
const STONE_DARK: u32 = 0x6b6358; // shadowed lower stone
const STONE_BODY: u32 = 0x8a7f70; // mid grey-brown body
const STONE_PALE: u32 = 0xb3a692; // sun-bleached top facet
const STONE_COOL: u32 = 0x7d7a72; // cooler neutral accent lump

const HOODOO_BASE: u32 = 0x9c7d59; // warm ochre sandstone band
const HOODOO_RUST: u32 = 0x8a5c3c; // darker rust/iron band
const HOODOO_PALE: u32 = 0xc6a978; // pale wind-scoured band

const SHRUB_DRY: u32 = 0x7c7a3e; // olive-brown dry shrub body
const SHRUB_DRY_DARK: u32 = 0x5f5d2c; // shadowed shrub skirt
const SHRUB_DEAD: u32 = 0x9a8c52; // bleached dead-grass tips

// Crystal / geode cluster — saturated gem colours jutting from a small grey rock, the one
// splash of colour in the grey biome. Two themes (amethyst / teal) keyed off the variant.
const CRYSTAL_AMETHYST: u32 = 0x9b59d0; // amethyst body
const CRYSTAL_AMETHYST_DK: u32 = 0x6f3fa0; // shaded amethyst facet
const CRYSTAL_TEAL: u32 = 0x33c2ae; // teal body
const CRYSTAL_TEAL_DK: u32 = 0x2493a0; // shaded teal facet
const CRYSTAL_TIP: u32 = 0xe6dcf6; // pale near-white lit crystal tip

// Lichen — the only living colour creeping over the stone (rusty orange + pale green).
const LICHEN_ORANGE: u32 = 0xc88a3a; // rusty orange lichen crust
const LICHEN_GREEN: u32 = 0x8a9a4a; // pale lichen green

const PEBBLE_GREY: u32 = 0x9a9085; // scree pebble
const PEBBLE_WARM: u32 = 0x8a7a64; // warmer scree pebble
const DRYTUFT_BASE: u32 = 0x86813f; // dry ground tuft base
const DRYTUFT_TIP: u32 = 0xa89a55; // bleached tuft tip

// ── Mesh helpers (mirror trees.rs / props.rs / decor.rs verbatim) ──────────────────

/// Tag every vertex of `m` with one flat linear colour (REQUIRED before merge — all
/// merged parts must carry the same attribute set, incl. ATTRIBUTE_COLOR).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge several pre-`tinted` parts into ONE mesh (so identical props batch into one
/// draw call). `Mesh::merge` returns `Result` in 0.18 — `.expect` on a mismatch.
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("rocky parts share attributes");
    }
    base
}

/// Un-index + recompute per-face normals → crisp flat-shaded low-poly facets.
/// `duplicate_vertices()` MUST run before `compute_flat_normals()` (the latter panics
/// on an indexed mesh). Call LAST, on the merged mesh.
fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// A low-poly **faceted** lump: a 20-face icosahedron (ico detail 0) with hard per-face
/// normals — the angular "chipped stone" look. Optionally squashed + tilted for irregular
/// boulders. Built then translated, so `off` is the lump centre.
fn facet_at(r: f32, off: Vec3, squash: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .unwrap()
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        c,
    )
}

/// A faceted lump that is also stretched on X/Z and given a small tilt (about Z) so the
/// boulder reads as a jagged angular block rather than a ball. `tilt` in radians.
fn block_at(rx: f32, ry: f32, rz: f32, off: Vec3, tilt: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Sphere::new(1.0)
            .mesh()
            .ico(0)
            .unwrap()
            .scaled_by(Vec3::new(rx, ry, rz))
            .rotated_by(Quat::from_rotation_z(tilt))
            .translated_by(off),
        c,
    )
}

/// An upright cylinder whose centre sits at `cy` (so a part of height `h` rooted at y=0
/// uses `cy = h/2`). `res` ≥ 3 (the Cylinder builder asserts resolution > 2).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: [f32; 4]) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), c)
}

// ── Boulders (dominant scatter class) ───────────────────────────────────────────
// Angular faceted rock chunks, layered/stacked. Three variants vary the silhouette:
// a squat wide slab pile, a tall split crag, and a low scatter of broken cobbles. All
// base flush at y=0. Authored ~0.4–0.7u; the scatter scales them 0.7–2.2 per instance,
// so they range from knee-high rubble to chest-high boulders.

pub fn build_boulder_mesh(variant: u32) -> Mesh {
    let m = match variant % 3 {
        // 0 — squat wide slab: a broad tilted block + a stacked upper slab + side chunks.
        0 => {
            let r = 0.42;
            merged(vec![
                // Broad base slab, stretched flat and lightly tilted.
                block_at(r * 1.25, r * 0.62, r * 1.05, y(r * 0.5), 0.10, lin_scaled(STONE_BODY, 0.9)),
                // Stacked upper slab offset to one side (the "layering").
                block_at(r * 0.9, r * 0.5, r * 0.78, Vec3::new(r * 0.28, r * 1.05, -r * 0.12), -0.14, lin(STONE_COOL)),
                // Bright pale cap catching the sun.
                facet_at(r * 0.5, Vec3::new(r * 0.34, r * 1.55, -r * 0.1), 0.7, lin(STONE_PALE)),
                // Side chunks leaning on the base.
                facet_at(r * 0.55, Vec3::new(-r * 0.95, r * 0.45, r * 0.22), 0.8, lin_scaled(STONE_DARK, 1.05)),
                facet_at(r * 0.46, Vec3::new(r * 0.85, r * 0.4, -r * 0.55), 0.8, lin(STONE_COOL)),
            ])
        }
        // 1 — tall split crag: stacked blocks climbing up + a wedged chip + pale peak.
        1 => {
            let r = 0.4;
            merged(vec![
                // Lower block.
                block_at(r * 0.95, r * 0.8, r * 0.9, y(r * 0.7), 0.06, lin_scaled(STONE_DARK, 1.08)),
                // Middle block offset (the split).
                block_at(r * 0.78, r * 0.7, r * 0.72, Vec3::new(r * 0.3, r * 1.7, -r * 0.1), -0.12, lin(STONE_BODY)),
                // Upper block.
                block_at(r * 0.55, r * 0.6, r * 0.52, Vec3::new(r * 0.12, r * 2.55, r * 0.06), 0.1, lin(STONE_COOL)),
                // A small chip wedged in the cleft.
                facet_at(r * 0.34, Vec3::new(-r * 0.5, r * 1.3, r * 0.28), 0.85, lin_scaled(STONE_DARK, 1.0)),
                // Pale sun-bleached peak.
                facet_at(r * 0.4, Vec3::new(r * 0.14, r * 3.15, r * 0.02), 0.78, lin(STONE_PALE)),
            ])
        }
        // 2 — low scatter of broken cobbles spread flat (rubble field).
        _ => {
            let r = 0.34;
            merged(vec![
                block_at(r * 1.0, r * 0.5, r * 0.85, y(r * 0.46), 0.08, lin_scaled(STONE_BODY, 0.94)),
                facet_at(r * 0.78, Vec3::new(r * 1.05, r * 0.45, r * 0.28), 0.7, lin(STONE_COOL)),
                facet_at(r * 0.66, Vec3::new(-r * 0.98, r * 0.4, -r * 0.42), 0.7, lin_scaled(STONE_DARK, 1.05)),
                facet_at(r * 0.5, Vec3::new(r * 0.12, r * 0.4, -r * 1.05), 0.72, lin(STONE_PALE)),
                facet_at(r * 0.42, Vec3::new(-r * 0.22, r * 0.36, r * 1.0), 0.72, lin(STONE_BODY)),
            ])
        }
    };
    flat_shaded(m)
}

// ── Hoodoo / rock spire (the "tree" class — spacing-checked) ─────────────────────
// A tapered stack of stone drums in alternating ochre/rust/pale bands — a wind-carved
// column. Some carry a wider "balanced" cap rock perched on the narrow neck. Base at
// y=0. Authored ~1.4–1.8u tall; the scatter scales them up so they tower like landmarks.

pub fn build_hoodoo_mesh(variant: u32) -> Mesh {
    let m = match variant % 2 {
        // 0 — slender banded spire tapering to a point.
        0 => {
            // Stack of drums, each narrower than the last, alternating bands.
            let bands = [
                (0.30_f32, 0.34_f32, HOODOO_RUST),
                (0.27, 0.32, HOODOO_BASE),
                (0.24, 0.30, HOODOO_PALE),
                (0.20, 0.30, HOODOO_BASE),
                (0.16, 0.28, HOODOO_RUST),
                (0.12, 0.26, HOODOO_PALE),
            ];
            let mut parts = Vec::new();
            let mut cy = 0.0;
            for &(r, h, c) in &bands {
                parts.push(cyl_up(r, h, cy + h * 0.5, 8, lin(c)));
                cy += h;
            }
            // A small pointed cap.
            parts.push(tinted(
                Cone { radius: 0.12, height: 0.22 }.mesh().resolution(8).build().translated_by(y(cy + 0.11)),
                lin(HOODOO_RUST),
            ));
            merged(parts)
        }
        // 1 — balanced rock: a narrow stacked neck with a wide cap rock perched on top.
        _ => {
            let bands = [
                (0.26_f32, 0.40_f32, HOODOO_BASE),
                (0.20, 0.36, HOODOO_RUST),
                (0.15, 0.34, HOODOO_PALE),
                (0.11, 0.30, HOODOO_BASE), // narrow neck
            ];
            let mut parts = Vec::new();
            let mut cy = 0.0;
            for &(r, h, c) in &bands {
                parts.push(cyl_up(r, h, cy + h * 0.5, 8, lin(c)));
                cy += h;
            }
            // Wide balanced cap rock — a stretched faceted block overhanging the neck.
            parts.push(block_at(0.36, 0.20, 0.32, y(cy + 0.18), 0.06, lin_scaled(STONE_BODY, 1.05)));
            parts.push(facet_at(0.18, y(cy + 0.42), 0.7, lin(STONE_PALE)));
            merged(parts)
        }
    };
    flat_shaded(m)
}

// ── Dry shrub / dead-grass clump (first non-tree class → tree fallback) ──────────
// A low olive-brown scrub: a dark squashed skirt of foliage lumps with a few bleached
// dead-grass spikes poking out. Base at y=0, ~0.35u tall. Two variants vary fullness.

pub fn build_dry_shrub_mesh(variant: u32) -> Mesh {
    let mut parts = vec![
        // Dark grounded skirt.
        facet_at(0.20, y(0.13), 0.7, lin(SHRUB_DRY_DARK)),
        facet_at(0.16, Vec3::new(0.15, 0.11, 0.05), 0.7, lin(SHRUB_DRY_DARK)),
        facet_at(0.14, Vec3::new(-0.13, 0.10, 0.09), 0.7, lin(SHRUB_DRY_DARK)),
        // Olive body.
        facet_at(0.17, y(0.22), 0.78, lin(SHRUB_DRY)),
        facet_at(0.13, Vec3::new(0.10, 0.25, -0.10), 0.78, lin(SHRUB_DRY)),
    ];
    if variant % 2 == 0 {
        parts.push(facet_at(0.11, y(0.31), 0.82, lin_scaled(SHRUB_DRY, 1.1)));
    }
    // A few bleached dead-grass spikes (thin cones) leaning out of the clump.
    let spikes = if variant % 2 == 0 { 5 } else { 4 };
    for i in 0..spikes {
        let a = (i as f32 / spikes as f32) * std::f32::consts::TAU + 0.4;
        let h = 0.24 + (i % 3) as f32 * 0.05;
        let tilt = 0.22 + (i % 2) as f32 * 0.10;
        let foot = 0.10;
        let spike = Cone { radius: 0.012, height: h }
            .mesh()
            .build()
            .translated_by(y(h / 2.0))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(Vec3::new(a.cos() * foot, 0.06, a.sin() * foot));
        parts.push(tinted(spike, lin(SHRUB_DEAD)));
    }
    flat_shaded(merged(parts))
}

// ── Crystal / geode cluster (scatter — the one colour accent) ────────────────────
// A small grey rock base with a fan of 6-sided crystal shards (hex prism + pointed cap)
// jutting up at mixed tilts/sizes, in saturated amethyst or teal with a pale lit tip.
// Vertex-colour only (no emissive) so it still batches on the shared material. Base at
// y=0, ~0.6u tall. `variant` switches the gem theme.

/// One crystal shard: a 6-sided prism capped with a 6-sided point, tilted + yawed off the
/// base, in `body`/`tip` linear colours. Returned as one merged (2-part) mesh.
fn crystal_shard(r: f32, h: f32, tilt: f32, yaw: f32, base: Vec3, body: [f32; 4], tip: [f32; 4]) -> Mesh {
    let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt);
    let prism = tinted(
        Cylinder::new(r, h).mesh().resolution(6).build().translated_by(y(h * 0.5)).rotated_by(rot).translated_by(base),
        body,
    );
    let cap_h = r * 2.4;
    let cap = tinted(
        Cone { radius: r * 1.02, height: cap_h }
            .mesh()
            .resolution(6)
            .build()
            .translated_by(y(h + cap_h * 0.5))
            .rotated_by(rot)
            .translated_by(base),
        tip,
    );
    merged(vec![prism, cap])
}

pub fn build_crystal_mesh(variant: u32) -> Mesh {
    // Theme: amethyst (even) vs teal (odd), each with a darker shaded accent shard.
    let (body, body_dk) = if variant % 2 == 0 {
        (lin(CRYSTAL_AMETHYST), lin(CRYSTAL_AMETHYST_DK))
    } else {
        (lin(CRYSTAL_TEAL), lin(CRYSTAL_TEAL_DK))
    };
    let tip = lin(CRYSTAL_TIP);

    let mut parts = vec![
        // Small grey host rock the crystals erupt from.
        facet_at(0.22, y(0.10), 0.62, lin(STONE_DARK)),
        facet_at(0.15, Vec3::new(0.13, 0.09, 0.05), 0.7, lin(STONE_BODY)),
        facet_at(0.12, Vec3::new(-0.12, 0.07, -0.06), 0.7, lin_scaled(STONE_DARK, 1.05)),
    ];

    // A tall central shard + four shorter ones leaning out around it.
    parts.push(crystal_shard(0.06, 0.42, 0.0, 0.0, y(0.14), body, tip));
    let shards = [
        (0.05_f32, 0.30_f32, 0.40_f32, 0.6_f32, Vec3::new(0.12, 0.12, 0.04), body_dk),
        (0.045, 0.26, -0.42, 2.1, Vec3::new(-0.13, 0.11, 0.05), body),
        (0.04, 0.22, 0.34, 3.6, Vec3::new(0.05, 0.10, -0.13), body_dk),
        (0.038, 0.20, -0.30, 5.0, Vec3::new(-0.06, 0.10, -0.10), body),
    ];
    for (r, h, tilt, yaw, base, c) in shards {
        parts.push(crystal_shard(r, h, tilt, yaw, base, c, tip));
    }

    flat_shaded(merged(parts))
}

// ── Scree pebble cluster (scatter) ───────────────────────────────────────────────
// A flat spread of several tiny faceted stones — broken rock litter. Base at y=0,
// very low. Two variants vary the count / spread.

pub fn build_scree_mesh(variant: u32) -> Mesh {
    let mut parts = vec![
        block_at(0.14, 0.07, 0.11, y(0.06), 0.12, lin(PEBBLE_GREY)),
        facet_at(0.09, Vec3::new(0.18, 0.05, 0.06), 0.6, lin(PEBBLE_WARM)),
        facet_at(0.08, Vec3::new(-0.15, 0.045, 0.12), 0.6, lin_scaled(PEBBLE_GREY, 0.9)),
        facet_at(0.07, Vec3::new(0.05, 0.04, -0.17), 0.6, lin(PEBBLE_WARM)),
    ];
    if variant % 2 == 0 {
        parts.push(facet_at(0.06, Vec3::new(-0.10, 0.035, -0.14), 0.6, lin(PEBBLE_GREY)));
        parts.push(facet_at(0.055, Vec3::new(0.16, 0.03, -0.05), 0.6, lin_scaled(PEBBLE_WARM, 1.05)));
    }
    flat_shaded(merged(parts))
}

// ── Rocky ground litter (cover) ──────────────────────────────────────────────────
// The little colour accents on the stony floor. `variant`: 0 = a small stone crusted
// with rusty-orange + pale-green lichen, 1 = a tiny crystal sprinkle (a grey nub with a
// couple of small amethyst/teal shards). Very low (≤0.12u), base at y=0.
fn build_rocky_litter_mesh(variant: u32) -> Mesh {
    match variant % 2 {
        // Lichen-crusted stone — a grey facet nub with flat lichen splotches on top.
        0 => flat_shaded(merged(vec![
            facet_at(0.08, y(0.05), 0.6, lin(STONE_BODY)),
            facet_at(0.05, Vec3::new(0.06, 0.04, 0.02), 0.6, lin_scaled(STONE_DARK, 1.05)),
            facet_at(0.045, Vec3::new(0.0, 0.085, 0.0), 0.22, lin(LICHEN_ORANGE)),
            facet_at(0.035, Vec3::new(0.05, 0.072, 0.03), 0.22, lin(LICHEN_GREEN)),
            facet_at(0.028, Vec3::new(-0.04, 0.066, -0.03), 0.22, lin(LICHEN_ORANGE)),
        ])),
        // Mini crystal sprinkle — small amethyst + teal shards on a dark stone nub.
        _ => {
            let mut parts = vec![facet_at(0.06, y(0.04), 0.6, lin(STONE_DARK))];
            parts.push(crystal_shard(0.025, 0.10, 0.0, 0.0, y(0.06), lin(CRYSTAL_AMETHYST), lin(CRYSTAL_TIP)));
            parts.push(crystal_shard(0.02, 0.08, 0.4, 2.0, Vec3::new(0.04, 0.05, 0.02), lin(CRYSTAL_TEAL), lin(CRYSTAL_TIP)));
            flat_shaded(merged(parts))
        }
    }
}

// ── Ground cover meshes ──────────────────────────────────────────────────────────

/// A single tiny ground pebble (a small flat faceted stone), base at y=0.
fn build_cover_pebble_mesh() -> Mesh {
    flat_shaded(merged(vec![
        block_at(0.08, 0.05, 0.07, y(0.045), 0.1, lin(PEBBLE_GREY)),
        facet_at(0.045, Vec3::new(0.07, 0.03, 0.02), 0.6, lin(PEBBLE_WARM)),
    ]))
}

/// A small dry-grass tuft — a fan of bleached cone blades, base at y=0, ~0.2u tall.
fn build_cover_drytuft_mesh() -> Mesh {
    let specs = [
        (0.0_f32, 0.00_f32, 0.20_f32, DRYTUFT_BASE),
        (0.6, 0.20, 0.16, DRYTUFT_TIP),
        (-0.5, -0.18, 0.15, DRYTUFT_TIP),
        (1.9, 0.14, 0.13, DRYTUFT_BASE),
    ];
    let parts = specs
        .iter()
        .map(|&(yaw, tilt, h, c)| {
            let mut m = Cone { radius: 0.016, height: h }
                .mesh()
                .build()
                .translated_by(y(h / 2.0))
                .rotated_by(Quat::from_rotation_z(tilt))
                .rotated_by(Quat::from_rotation_y(yaw));
            m.duplicate_vertices();
            m.compute_flat_normals();
            tinted(m, lin(c))
        })
        .collect();
    merged(parts)
}

// ── Config ───────────────────────────────────────────────────────────────────────

pub fn config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Rocky,
        name: "Rocky",

        ground_color: 0x8d847a,
        ground_roughness: 1.0,
        // Higher strength + grain for a stony, broken-dirt look; warm-grey ramp.
        detail: GroundDetail {
            scale: 0.22,
            strength: 0.55,
            variation: 0.58,
            seed: 7.0,
            dark: 0x5f574e,
            base: 0x8a8076,
            light: 0xbab09e,
            grain: 0.72,
            streak: 0.30,
        },

        // Neutral hazy daylight.
        sky: 0xc6cbd0,
        fog_density: 0.010,
        sun_color: 0xffe9c4,
        sun_illuminance: 10_800.0,
        ambient_color: 0xe6e2d8,
        ambient_brightness: 88.0,
        sun_pos: Vec3::new(18.0, 38.0, 12.0),

        seed: 4002,
        tree_min_dist: 3.2, // hoodoos are tall — keep them well spaced
        classes: vec![
            // Dry shrub — the FIRST non-tree class (the tree-too-close fallback). 2 variants.
            PropClass {
                variants: vec![
                    (build_dry_shrub_mesh(0), 1.0),
                    (build_dry_shrub_mesh(1), 1.0),
                ],
                chance: 0.045,
                scale: (0.8, 1.35),
                tree: false,
                block_radius: 0.0,
            },
            // Boulders — the DOMINANT class. 3 variants, big scale range.
            PropClass {
                variants: vec![
                    (build_boulder_mesh(0), 1.0),
                    (build_boulder_mesh(1), 0.85),
                    (build_boulder_mesh(2), 1.1),
                ],
                chance: 0.085,
                scale: (0.7, 2.2),
                tree: false,
                block_radius: 0.3, // dominant boulders — big ones block, scree-sized walk-through
            },
            // Hoodoo spires — the "tree" class (spacing-checked, no sway harm). 2 variants.
            PropClass {
                variants: vec![
                    (build_hoodoo_mesh(0), 1.0),
                    (build_hoodoo_mesh(1), 0.8),
                ],
                chance: 0.022,
                scale: (0.9, 1.8),
                tree: true,
                block_radius: 0.0,
            },
            // Scree pebble clusters. 2 variants.
            PropClass {
                variants: vec![
                    (build_scree_mesh(0), 1.0),
                    (build_scree_mesh(1), 1.0),
                ],
                chance: 0.03,
                scale: (0.7, 1.5),
                tree: false,
                block_radius: 0.0,
            },
            // Crystal / geode clusters — the lone colour accent (amethyst / teal). Sparse.
            PropClass {
                variants: vec![
                    (build_crystal_mesh(0), 1.0),
                    (build_crystal_mesh(1), 0.8),
                ],
                chance: 0.014,
                scale: (0.7, 1.5),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover: vec![
            PropClass {
                variants: vec![(build_cover_pebble_mesh(), 1.0)],
                chance: 0.30,
                scale: (0.6, 1.3),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_cover_drytuft_mesh(), 1.0)],
                chance: 0.22,
                scale: (0.7, 1.2),
                tree: false,
                block_radius: 0.0,
            },
            // Rocky litter — lichen-crusted stones + tiny crystal sprinkles (colour).
            PropClass {
                variants: (0..2).map(|v| (build_rocky_litter_mesh(v), 1.0)).collect(),
                chance: 0.10,
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover_per_tile: 2,

        river: false,
        river_color: 0x3f7fae,
        backdrop: Backdrop {
            // Wide land arc of tall craggy peaks; no treeline (bare stone country).
            land_dir: 0.0,
            land_arc: std::f32::consts::PI * 0.72,
            ocean: false,
            ocean_color: 0x2f6fae,
            hill_body: 0x8a8076,
            hill_cap: 0xc0b6a4,
            hill_foot: 0x6f665c,
            treeline: false,
            treeline_dark: 0x4a5a40,
            treeline_mid: 0x586a4a,
            hill_h: (58.0, 118.0),
        },
        particle: ParticleKind::Dust,
    }
}

// ── Landmark: a dramatic ROCK ARCH + a flat-topped MESA + a balanced hoodoo ───────

/// Build one stacked-drum banded column rooted at y=0, returned as a finished mesh.
/// Used for the arch pillars (so the lintel can bridge two of them).
fn arch_pillar(height: f32, r0: f32) -> Mesh {
    // Drums getting slightly narrower toward the top, alternating bands.
    let drums = 6;
    let mut parts = Vec::new();
    let h = height / drums as f32;
    let bands = [HOODOO_BASE, HOODOO_RUST, HOODOO_PALE];
    for i in 0..drums {
        let t = i as f32 / drums as f32;
        let r = r0 * (1.0 - t * 0.28);
        let c = bands[i % bands.len()];
        parts.push(cyl_up(r, h * 1.04, h * (i as f32 + 0.5), 10, lin(c)));
    }
    merged(parts)
}

pub fn landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.92,
        ..default()
    });

    // ── ROCK ARCH — two banded pillars + a thick spanning lintel block. ──
    // Centred at (-10, -13) on the land side, span along X.
    let pillar_h = 4.6;
    let pillar_r = 0.85;
    let span = 4.2; // distance between the two pillar centres
    let arch_cx = -10.0;
    let arch_cz = -13.0;

    let pillar_mesh = meshes.add(flat_shaded(arch_pillar(pillar_h, pillar_r)));
    for sx in [-span * 0.5, span * 0.5] {
        commands.spawn((
            Mesh3d(pillar_mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(arch_cx + sx, 0.0, arch_cz),
            BiomeEntity,
        ));
    }
    // Lintel — a long thick faceted block spanning the tops, sagging slightly to read as
    // a natural arch span (two stretched blocks meeting at a high centre).
    let lintel = flat_shaded(merged(vec![
        // Left half rising to centre.
        block_at(span * 0.34, 0.55, pillar_r * 0.95, Vec3::new(-span * 0.28, pillar_h + 0.2, 0.0), -0.10, lin(HOODOO_RUST)),
        // Right half.
        block_at(span * 0.34, 0.55, pillar_r * 0.95, Vec3::new(span * 0.28, pillar_h + 0.2, 0.0), 0.10, lin(HOODOO_RUST)),
        // Pale keystone crowning the join.
        block_at(span * 0.16, 0.5, pillar_r * 0.92, Vec3::new(0.0, pillar_h + 0.5, 0.0), 0.0, lin(HOODOO_PALE)),
        // Warm underside band so the span reads layered.
        block_at(span * 0.5, 0.22, pillar_r * 0.8, Vec3::new(0.0, pillar_h - 0.05, 0.0), 0.0, lin(HOODOO_BASE)),
    ]));
    commands.spawn((
        Mesh3d(meshes.add(lintel)),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(arch_cx, 0.0, arch_cz),
        BiomeEntity,
    ));

    // ── MESA — a wide flat-topped butte: a broad banded drum stack with a pale caprock
    // and a talus skirt of fallen boulders around its foot. To the right, on land. ──
    let mesa_cx = 11.0;
    let mesa_cz = -14.0;
    let mesa = {
        let mut parts = vec![
            // Broad sloping talus foot.
            cyl_up(3.4, 1.2, 0.6, 14, lin_scaled(STONE_DARK, 1.02)),
            // Main banded body.
            cyl_up(2.7, 1.4, 1.9, 14, lin(STONE_BODY)),
            cyl_up(2.45, 1.1, 3.15, 14, lin(HOODOO_BASE)),
            cyl_up(2.3, 0.9, 4.15, 14, lin_scaled(STONE_BODY, 1.04)),
            // Hard pale caprock (a wider thin slab on top — the erosion-resistant cap).
            cyl_up(2.5, 0.45, 4.83, 16, lin(STONE_PALE)),
        ];
        // Talus boulders strewn around the base.
        for i in 0..7 {
            let a = (i as f32 / 7.0) * std::f32::consts::TAU + 0.3;
            let rr = 3.1 + (i % 3) as f32 * 0.35;
            let off = Vec3::new(a.cos() * rr, 0.0, a.sin() * rr);
            let s = 0.6 + (i % 3) as f32 * 0.2;
            parts.push(block_at(0.6 * s, 0.34 * s, 0.5 * s, off + y(0.3 * s), 0.12, lin(STONE_COOL)));
        }
        flat_shaded(merged(parts))
    };
    commands.spawn((
        Mesh3d(meshes.add(mesa)),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(mesa_cx, 0.0, mesa_cz).with_rotation(Quat::from_rotation_y(0.4)),
        BiomeEntity,
    ));

    // ── A tall balanced-rock HOODOO standing alone between the arch and mesa, scaled up
    // from the scatter variant so it reads as a third set-piece. ──
    let hoodoo = meshes.add(build_hoodoo_mesh(1));
    commands.spawn((
        Mesh3d(hoodoo),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(0.5, 0.0, -16.5)
            .with_scale(Vec3::splat(2.6))
            .with_rotation(Quat::from_rotation_y(1.1)),
        BiomeEntity,
    ));

    // A couple of big loose boulders flanking the arch foot to ground the scene.
    let big_boulder = meshes.add(build_boulder_mesh(0));
    for (bx, bz, s, ry) in [(-13.5_f32, -10.5_f32, 1.8_f32, 0.6_f32), (-6.5, -10.0, 2.1, 2.3)] {
        commands.spawn((
            Mesh3d(big_boulder.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(bx, 0.0, bz)
                .with_scale(Vec3::splat(s))
                .with_rotation(Quat::from_rotation_y(ry)),
            BiomeEntity,
        ));
    }
}
