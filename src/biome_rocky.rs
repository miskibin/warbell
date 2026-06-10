//! Rocky highlands biome (key 3) — dramatic craggy stone country. The scatter is
//! dominated by angular CRAG piles (big fractured slabs leaning into each other over dark
//! crevice wedges, sun-bleached top facets, ochre/sage lichen crusts, shed cobbles at the
//! foot — plus a stacked-flat-stone CAIRN variant), punctuated by a mixed "tree" class:
//! banded HOODOO spires with wind-carved ledges, gnarled wind-bent mountain PINES streaming
//! sparse tough foliage leeward, and a bleached splinter-topped SNAG (all spacing-checked so
//! they never crowd). Low dry SHRUB clumps (the first non-tree class → the tree-too-close
//! fallback) and directional SCREE spills fill between, with sparse crystal clusters as the
//! one colour accent. Ground cover dresses the dirt with pebbles, dry tufts, lichen litter
//! and tiny alpine wildflower specks. The horizon is a wide arc of tall craggy grey peaks
//! (no treeline); dust drifts on the wind. River is off.
//!
//! Self-contained: every mesh is built here from primitives, tinted into ATTRIBUTE_COLOR
//! and flat-shaded for the crisp low-poly facets the rest of the scene uses. Light is
//! BAKED into the vertex colours: every prop stacks dark shadowed feet/undersides, a mid
//! body and pale sun-bleached top facets so the hard facets read lit before the real sun
//! even hits them. Atmosphere is neutral hazy daylight. The landmark is a dramatic
//! two-pillar ROCK ARCH plus a flat-topped MESA (with a lone pine on its cap), a
//! balanced-rock hoodoo and a waymarker cairn, all on the land side (z < 0).
//!
//! Palette (muted arid stone, lifted from the TS rock-highland feel — grey-browns with
//! warm sand undertones and pale sun-bleached caps; banded ochre/rust for the hoodoos;
//! dusty conifer greens for the pines; pin-prick alpine flower colours kept tiny so the
//! biome stays grey overall).

// The `landmarks()` ROCK ARCH set-piece + its `arch_pillar` helper below are authored biome
// content the world map doesn't place yet (it uses `ruins` landmarks instead). Kept per design;
// allow the resulting dead code until it's wired into a per-region pass.
#![allow(dead_code)]

use bevy::prelude::*;

use crate::biome::{Backdrop, Biome, BiomeConfig, BiomeEntity, GroundDetail, ParticleKind, PropClass};
use crate::palette::{lin, lin_scaled};

// ── Rocky palette ─────────────────────────────────────────────────────────────────
// Stone body tones: a cool-warm grey-brown stack so a crag reads lit (dark foot →
// mid body → pale sun-bleached cap). Hoodoos add warmer banded ochre/rust strata.
const STONE_DARK: u32 = 0x6b6358; // shadowed lower stone / crevice
const STONE_BODY: u32 = 0x8a7f70; // mid grey-brown body
const STONE_PALE: u32 = 0xb3a692; // sun-bleached top facet
const STONE_COOL: u32 = 0x7d7a72; // cooler neutral accent lump

const HOODOO_BASE: u32 = 0x9c7d59; // warm ochre sandstone band
const HOODOO_RUST: u32 = 0x8a5c3c; // darker rust/iron band
const HOODOO_PALE: u32 = 0xc6a978; // pale wind-scoured band

const SHRUB_DRY: u32 = 0x7c7a3e; // olive-brown dry shrub body
const SHRUB_DRY_DARK: u32 = 0x5f5d2c; // shadowed shrub skirt
const SHRUB_DEAD: u32 = 0x9a8c52; // bleached dead-grass tips

// Mountain pine / snag — tough dusty conifer greens on weathered grey-brown wood.
const PINE_DARK: u32 = 0x46563a; // shadowed needle underside
const PINE_BODY: u32 = 0x5c7046; // mid dusty needle clump
const PINE_LIT: u32 = 0x7a8c54; // sunlit needle cap
const BARK_BODY: u32 = 0x6e6052; // weathered grey-brown bark
const BARK_DARK: u32 = 0x52463a; // bark shadow / root flare
const SNAG_BLEACH: u32 = 0xb5a88f; // sun-bleached dead wood splinter
const SNAG_GREY: u32 = 0x8d8273; // weathered snag trunk mid-tone

// Crystal / geode cluster — saturated gem colours jutting from a small grey rock, the one
// splash of colour in the grey biome. Two themes (amethyst / teal) keyed off the variant.
const CRYSTAL_AMETHYST: u32 = 0x9b59d0; // amethyst body
const CRYSTAL_AMETHYST_DK: u32 = 0x6f3fa0; // shaded amethyst facet
const CRYSTAL_TEAL: u32 = 0x33c2ae; // teal body
const CRYSTAL_TEAL_DK: u32 = 0x2493a0; // shaded teal facet
const CRYSTAL_TIP: u32 = 0xe6dcf6; // pale near-white lit crystal tip

// Lichen — the only living colour creeping over the stone (rusty orange + two greens).
const LICHEN_ORANGE: u32 = 0xc88a3a; // rusty orange lichen crust
const LICHEN_GREEN: u32 = 0x8a9a4a; // lichen green
const LICHEN_SAGE: u32 = 0x9aa56b; // paler sage lichen

const PEBBLE_GREY: u32 = 0x9a9085; // scree pebble
const PEBBLE_WARM: u32 = 0x8a7a64; // warmer scree pebble
const DRYTUFT_BASE: u32 = 0x86813f; // dry ground tuft base
const DRYTUFT_TIP: u32 = 0xa89a55; // bleached tuft tip

// Alpine wildflowers — tiny bright specks between the stones (heads are single squashed
// facets ~0.02u, so they read as pinpricks of colour, not flowerbeds).
const FLOWER_VIOLET: u32 = 0x9b7fd6;
const FLOWER_WHITE: u32 = 0xeae6da;
const FLOWER_GOLD: u32 = 0xd9b84e;
const STEM_GREEN: u32 = 0x6f7a44;

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

/// `block_at` with an extra yaw (about Y, applied after the Z-tilt) so big fracture slabs
/// can lean in ANY direction — the workhorse of the crag piles.
fn slab_at(rx: f32, ry: f32, rz: f32, off: Vec3, yaw: f32, tilt: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Sphere::new(1.0)
            .mesh()
            .ico(0)
            .unwrap()
            .scaled_by(Vec3::new(rx, ry, rz))
            .rotated_by(Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt))
            .translated_by(off),
        c,
    )
}

/// Centre height that grounds a Z-tilted slab's lowest point exactly at y=0 — the support
/// function of the scaled ico "ellipsoid" in -Y: `sqrt((rx·sin t)² + (ry·cos t)²)`. (The
/// extra yaw in `slab_at` is about Y so it never changes the vertical extent.)
fn slab_ground(rx: f32, ry: f32, tilt: f32) -> f32 {
    ((rx * tilt.sin()).powi(2) + (ry * tilt.cos()).powi(2)).sqrt()
}

/// A flat lichen splotch — a strongly squashed facet lump pressed onto a stone surface.
fn lichen_at(r: f32, off: Vec3, c: u32) -> Mesh {
    facet_at(r, off, 0.24, lin(c))
}

/// A flat cairn stone — a thin cuboid slab yawed about Y, centre at `off`.
fn flat_stone(w: f32, h: f32, d: f32, off: Vec3, yaw: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Cuboid::new(w, h, d)
            .mesh()
            .build()
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(off),
        c,
    )
}

/// An upright cylinder whose centre sits at `cy` (so a part of height `h` rooted at y=0
/// uses `cy = h/2`). `res` ≥ 3 (the Cylinder builder asserts resolution > 2).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: [f32; 4]) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), c)
}

/// One thin leaning blade — a skinny **res-4** cone (8 tris, vs 64 at default resolution)
/// rooted at `foot`, tilted about Z then yawed about Y. Returns `(mesh, tip_position)` so
/// a flower head / seed head can be perched exactly on the tip. Shared by the dry-shrub
/// spikes, dry tufts, wildflowers and scree accents.
fn blade(r: f32, h: f32, yaw: f32, tilt: f32, foot: Vec3, c: [f32; 4]) -> (Mesh, Vec3) {
    let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt);
    let m = tinted(
        Cone { radius: r, height: h }
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(h * 0.5))
            .rotated_by(rot)
            .translated_by(foot),
        c,
    );
    (m, foot + rot * y(h))
}

// ── Crags / boulders (dominant scatter class) ─────────────────────────────────────
// Angular fractured rock, with the lighting BAKED in: dark crevice wedges and shadowed
// feet, mid grey-brown bodies, pale sun-bleached facets on every raised edge, plus
// ochre/sage lichen crusts. Four variants: a leaning slab crag, a tall split crag, a
// cobble spill (with grass + a flower speck between the stones) and a stacked-stone
// CAIRN. All base flush at y=0 (leaning slabs are grounded via `slab_ground`). Authored
// ~0.4–0.7u; the scatter scales 0.7–2.2, knee-high rubble → chest-high crags.

pub fn build_boulder_mesh(variant: u32) -> Mesh {
    let m = match variant % 4 {
        // 0 — leaning slab crag: two big fractured slabs leaning into each other over a
        // dark crevice wedge; pale facets on the raised edges; cobbles shed at the foot.
        0 => {
            let r = 0.42;
            let (mx, my, mt) = (r * 1.3, r * 0.75, 0.30); // main slab dims + lean
            let (cx, cy, ct) = (r * 0.95, r * 0.62, -0.38); // counter-slab leaning back
            merged(vec![
                slab_at(mx, my, r * 0.95, Vec3::new(-r * 0.15, slab_ground(mx, my, mt), 0.0), 0.2, mt, lin_scaled(STONE_BODY, 0.92)),
                slab_at(cx, cy, r * 0.8, Vec3::new(r * 0.6, slab_ground(cx, cy, ct), -r * 0.1), -0.5, ct, lin(STONE_COOL)),
                // Dark wedge jammed in the crevice where the slabs meet (baked shadow).
                facet_at(r * 0.4, Vec3::new(r * 0.18, r * 0.55, r * 0.12), 0.9, lin_scaled(STONE_DARK, 0.85)),
                // Sun-bleached facets riding the raised slab edges.
                facet_at(r * 0.4, Vec3::new(r * 0.55, r * 1.45, 0.05), 0.45, lin(STONE_PALE)),
                facet_at(r * 0.28, Vec3::new(r * 0.25, r * 1.2, -r * 0.35), 0.45, lin_scaled(STONE_PALE, 0.95)),
                // Lichen crusts — orange on the sunny top, sage low on the flank.
                lichen_at(r * 0.26, Vec3::new(-r * 0.3, r * 1.5, r * 0.2), LICHEN_ORANGE),
                lichen_at(r * 0.2, Vec3::new(-r * 0.95, r * 0.55, r * 0.3), LICHEN_SAGE),
                // Shed cobbles at the foot.
                facet_at(r * 0.3, Vec3::new(-r * 1.3, r * 0.21, -r * 0.45), 0.7, lin_scaled(STONE_DARK, 1.05)),
                facet_at(r * 0.24, Vec3::new(r * 1.3, r * 0.17, r * 0.55), 0.7, lin(PEBBLE_WARM)),
            ])
        }
        // 1 — tall split crag: stacked blocks climbing up, a flank slab propped against
        // the base, a dark cleft chip, lit ledges and a pale sun-bleached peak.
        1 => {
            let r = 0.4;
            let (fx, fy, ft) = (r * 0.85, r * 0.4, 0.5); // propped flank slab
            merged(vec![
                // Lower block (grounded: vertical extent ≈ 0.85r at this tilt).
                block_at(r * 1.05, r * 0.85, r * 0.95, y(r * 0.86), 0.06, lin_scaled(STONE_DARK, 1.08)),
                // Middle block offset (the split), then the upper block.
                block_at(r * 0.8, r * 0.72, r * 0.75, Vec3::new(r * 0.3, r * 1.75, -r * 0.1), -0.12, lin(STONE_BODY)),
                block_at(r * 0.58, r * 0.62, r * 0.55, Vec3::new(r * 0.1, r * 2.6, r * 0.05), 0.10, lin(STONE_COOL)),
                // Fracture slab propped against the lower block.
                slab_at(fx, fy, r * 0.6, Vec3::new(-r * 0.8, slab_ground(fx, fy, ft), r * 0.2), 0.7, ft, lin_scaled(STONE_BODY, 0.9)),
                // Dark chip wedged in the cleft (baked crevice shadow).
                facet_at(r * 0.32, Vec3::new(-r * 0.45, r * 1.35, r * 0.3), 0.85, lin_scaled(STONE_DARK, 0.85)),
                // Lit ledge facet on the mid block + pale sun-bleached peak.
                facet_at(r * 0.26, Vec3::new(r * 0.6, r * 2.25, -r * 0.15), 0.5, lin_scaled(STONE_PALE, 0.94)),
                facet_at(r * 0.42, Vec3::new(r * 0.12, r * 3.2, 0.0), 0.75, lin(STONE_PALE)),
                // Lichen — orange on the mid shoulder, sage low.
                lichen_at(r * 0.18, Vec3::new(-r * 0.1, r * 2.2, r * 0.45), LICHEN_ORANGE),
                lichen_at(r * 0.16, Vec3::new(r * 0.85, r * 0.9, r * 0.35), LICHEN_GREEN),
                // A cobble shed at the foot.
                facet_at(r * 0.26, Vec3::new(r * 1.1, r * 0.19, r * 0.5), 0.7, lin(PEBBLE_WARM)),
            ])
        }
        // 2 — cobble spill: a low central slab with broken cobbles drifted around it,
        // dry grass + one gold alpine speck poking up between the stones.
        2 => {
            let r = 0.34;
            let (sx, sy, st) = (r * 1.1, r * 0.45, 0.10);
            let mut parts = vec![
                slab_at(sx, sy, r * 0.8, Vec3::new(0.0, slab_ground(sx, sy, st), 0.0), 0.3, st, lin_scaled(STONE_BODY, 0.94)),
                // Lit fleck on the central slab's crown.
                facet_at(r * 0.3, Vec3::new(r * 0.2, r * 0.78, 0.0), 0.4, lin(STONE_PALE)),
                // The cobble drift, mixed tones, thinning outward.
                facet_at(r * 0.78, Vec3::new(r * 1.1, r * 0.4, r * 0.3), 0.7, lin(STONE_COOL)),
                facet_at(r * 0.66, Vec3::new(-r * 1.0, r * 0.34, -r * 0.45), 0.7, lin_scaled(STONE_DARK, 1.05)),
                facet_at(r * 0.5, Vec3::new(r * 0.12, r * 0.27, -r * 1.05), 0.72, lin(STONE_PALE)),
                facet_at(r * 0.44, Vec3::new(-r * 0.25, r * 0.24, r * 1.05), 0.72, lin(PEBBLE_WARM)),
                facet_at(r * 0.36, Vec3::new(r * 1.6, r * 0.2, -r * 0.5), 0.7, lin_scaled(PEBBLE_GREY, 0.94)),
                facet_at(r * 0.3, Vec3::new(-r * 1.55, r * 0.17, r * 0.35), 0.7, lin(PEBBLE_GREY)),
                // Sage lichen on the big cool cobble.
                lichen_at(r * 0.2, Vec3::new(r * 1.15, r * 0.85, r * 0.35), LICHEN_SAGE),
            ];
            // Dry grass + a gold flower speck in the gaps.
            let (g1, _) = blade(0.013, 0.17, 0.8, 0.3, Vec3::new(-r * 0.6, 0.02, -r * 0.9), lin(DRYTUFT_BASE));
            let (g2, _) = blade(0.012, 0.14, 3.9, 0.36, Vec3::new(-r * 0.5, 0.02, -r * 0.8), lin(DRYTUFT_TIP));
            let (stem, tip) = blade(0.008, 0.10, 2.0, 0.25, Vec3::new(r * 0.7, 0.01, -r * 0.55), lin(STEM_GREEN));
            parts.push(g1);
            parts.push(g2);
            parts.push(stem);
            parts.push(facet_at(0.018, tip, 0.75, lin(FLOWER_GOLD)));
            merged(parts)
        }
        // 3 — CAIRN: a hand-stacked tower of flat stones (thin yawed cuboids) tapering
        // up to a pale top stone + a pebble topper; lichen on the weathered courses.
        _ => {
            // (w, d, h, yaw, x-jog, z-jog, colour) per course, bottom → top.
            let stones = [
                (0.46_f32, 0.40_f32, 0.100_f32, 0.15_f32, 0.000_f32, 0.000_f32, lin_scaled(STONE_BODY, 0.92)),
                (0.40, 0.35, 0.095, -0.35, 0.020, -0.015, lin(STONE_COOL)),
                (0.35, 0.30, 0.090, 0.55, -0.025, 0.020, lin(STONE_BODY)),
                (0.28, 0.25, 0.085, -0.20, 0.015, 0.015, lin_scaled(STONE_DARK, 1.12)),
                (0.22, 0.19, 0.080, 0.95, -0.010, -0.020, lin_scaled(STONE_BODY, 1.04)),
                (0.16, 0.13, 0.075, 0.35, 0.020, 0.010, lin(STONE_PALE)),
            ];
            let mut parts = Vec::new();
            let mut cy = 0.0;
            for &(w, d, h, yaw, jx, jz, c) in &stones {
                parts.push(flat_stone(w, h, d, Vec3::new(jx, cy + h * 0.5, jz), yaw, c));
                cy += h;
            }
            // Pebble topper + lichen on the courses + two stones dropped at the foot.
            parts.push(facet_at(0.05, Vec3::new(0.02, cy + 0.035, 0.01), 0.7, lin(PEBBLE_WARM)));
            parts.push(lichen_at(0.055, Vec3::new(0.17, 0.20, 0.12), LICHEN_ORANGE));
            parts.push(lichen_at(0.05, Vec3::new(-0.20, 0.07, 0.06), LICHEN_SAGE));
            parts.push(facet_at(0.05, Vec3::new(0.30, 0.031, 0.14), 0.6, lin(PEBBLE_GREY)));
            parts.push(facet_at(0.04, Vec3::new(-0.27, 0.025, -0.12), 0.6, lin(PEBBLE_WARM)));
            merged(parts)
        }
    };
    flat_shaded(m)
}

// ── Hoodoo / rock spire (part of the "tree" class — spacing-checked) ──────────────
// A tapered stack of stone drums in alternating ochre/rust/pale bands, now wind-carved:
// the drums drift sideways as they climb, hard pale bands resist erosion and jut out as
// protruding LEDGE rims, a dark plinth shadows the foot, and fallen band-rubble + lichen
// dress the base. Base at y=0. Authored ~1.5–1.9u; the scatter scales them up to tower.

pub fn build_hoodoo_mesh(variant: u32) -> Mesh {
    let m = match variant % 2 {
        // 0 — slender banded spire: 7 drifting drums tapering to a pointed cap, with two
        // ledge rims where the hard pale bands overhang.
        0 => {
            // (radius, height, band colour, sideways drift of the drum centre).
            let bands = [
                (0.31_f32, 0.32_f32, HOODOO_RUST, 0.000_f32),
                (0.28, 0.30, HOODOO_BASE, 0.015),
                (0.25, 0.28, HOODOO_PALE, 0.030),
                (0.21, 0.28, HOODOO_BASE, 0.050),
                (0.17, 0.26, HOODOO_RUST, 0.065),
                (0.13, 0.24, HOODOO_PALE, 0.085),
                (0.10, 0.22, HOODOO_BASE, 0.100),
            ];
            // Dark shadowed plinth hugging the foot (baked contact shadow).
            let mut parts = vec![cyl_up(0.36, 0.10, 0.05, 8, lin_scaled(HOODOO_RUST, 0.82))];
            let mut cy = 0.0;
            for (i, &(r, h, c, drift)) in bands.iter().enumerate() {
                parts.push(tinted(
                    Cylinder::new(r, h)
                        .mesh()
                        .resolution(8)
                        .build()
                        .translated_by(Vec3::new(drift, cy + h * 0.5, drift * 0.4)),
                    lin(c),
                ));
                cy += h;
                if i == 2 || i == 4 {
                    // Hard band resists the wind → a pale protruding ledge rim at the joint.
                    parts.push(facet_at(r * 1.18, Vec3::new(drift, cy, drift * 0.4), 0.22, lin(STONE_PALE)));
                }
            }
            // Pointed cap, leaning with the drift.
            parts.push(tinted(
                Cone { radius: 0.10, height: 0.22 }
                    .mesh()
                    .resolution(8)
                    .build()
                    .translated_by(Vec3::new(0.10, cy + 0.11, 0.04)),
                lin(HOODOO_RUST),
            ));
            // Lichen streaks on the shaded base + fallen band-rubble at the foot.
            parts.push(lichen_at(0.12, Vec3::new(-0.26, 0.30, 0.08), LICHEN_ORANGE));
            parts.push(lichen_at(0.09, Vec3::new(-0.22, 0.62, -0.12), LICHEN_SAGE));
            parts.push(facet_at(0.10, Vec3::new(0.40, 0.07, 0.16), 0.7, lin(PEBBLE_WARM)));
            parts.push(facet_at(0.08, Vec3::new(-0.36, 0.055, -0.20), 0.7, lin_scaled(HOODOO_RUST, 0.9)));
            merged(parts)
        }
        // 1 — balanced rock: a drifting banded neck (its top band darkened — it lives in
        // the cap's shadow) carrying a two-slab layered cap rock with a lit pale crown;
        // a dark squashed facet just under the cap bakes the overhang shadow in.
        _ => {
            let bands = [
                (0.27_f32, 0.38_f32, lin(HOODOO_BASE), 0.000_f32),
                (0.21, 0.34, lin(HOODOO_RUST), 0.020),
                (0.16, 0.32, lin(HOODOO_PALE), 0.040),
                (0.115, 0.28, lin_scaled(HOODOO_BASE, 0.78), 0.050), // shadowed narrow neck
            ];
            let mut parts = Vec::new();
            let mut cy = 0.0;
            for &(r, h, c, drift) in &bands {
                parts.push(tinted(
                    Cylinder::new(r, h)
                        .mesh()
                        .resolution(8)
                        .build()
                        .translated_by(Vec3::new(drift, cy + h * 0.5, drift * 0.3)),
                    c,
                ));
                cy += h;
            }
            // Baked contact shadow where the cap overhangs the neck.
            parts.push(facet_at(0.18, Vec3::new(0.05, cy + 0.03, 0.0), 0.3, lin_scaled(STONE_DARK, 0.88)));
            // The layered cap: main slab + smaller offset top slab + pale lit crown.
            parts.push(slab_at(0.40, 0.21, 0.34, Vec3::new(0.06, cy + 0.20, 0.0), 0.3, 0.07, lin_scaled(STONE_BODY, 1.05)));
            parts.push(slab_at(0.26, 0.14, 0.22, Vec3::new(0.10, cy + 0.42, -0.04), -0.4, -0.06, lin(STONE_COOL)));
            parts.push(facet_at(0.16, Vec3::new(0.10, cy + 0.55, -0.03), 0.55, lin(STONE_PALE)));
            // Lichen clinging to the cap rim + rubble and sage at the foot.
            parts.push(lichen_at(0.10, Vec3::new(0.40, cy + 0.24, 0.12), LICHEN_ORANGE));
            parts.push(lichen_at(0.10, Vec3::new(-0.22, 0.26, 0.10), LICHEN_SAGE));
            parts.push(facet_at(0.10, Vec3::new(0.36, 0.07, -0.14), 0.7, lin(PEBBLE_WARM)));
            parts.push(facet_at(0.08, Vec3::new(-0.32, 0.055, 0.18), 0.7, lin_scaled(STONE_BODY, 0.95)));
            merged(parts)
        }
    };
    flat_shaded(m)
}

// ── Wind-bent mountain pine (part of the "tree" class) ────────────────────────────
// A gnarled timberline conifer: three chained trunk segments leaning progressively
// downwind, a dark root flare, snapped branch stubs on the windward side, and a sparse
// flag of tough foliage streaming leeward — dark shadowed underside clumps, mid dusty
// bodies, small sunlit caps on top. `variant` mirrors the wind direction (and the odd
// variant carries one extra clump near the crown). Base at y=0, ~1.5u authored.

pub fn build_windpine_mesh(variant: u32) -> Mesh {
    // Wind blows toward +X for even variants, mirrored for odd.
    let sweep: f32 = if variant % 2 == 0 { 1.0 } else { -1.0 };
    let mut parts = vec![
        // Dark root flare gripping the stone.
        facet_at(0.10, y(0.06), 0.66, lin(BARK_DARK)),
        facet_at(0.06, Vec3::new(0.09 * sweep, 0.045, 0.04), 0.66, lin_scaled(BARK_DARK, 0.9)),
        facet_at(0.055, Vec3::new(-0.08 * sweep, 0.04, -0.05), 0.66, lin(BARK_BODY)),
    ];

    // Trunk: three chained segments, each leaning further downwind (twisted-bole look).
    let segs = [(0.085_f32, 0.55_f32, 0.16_f32), (0.065, 0.45, 0.34), (0.05, 0.40, 0.55)];
    let mut foot = y(0.02);
    for (i, &(r, len, lean)) in segs.iter().enumerate() {
        let rot = Quat::from_rotation_z(-lean * sweep);
        let mid = foot + rot * y(len * 0.5);
        let c = if i == 0 { lin_scaled(BARK_BODY, 0.92) } else { lin(BARK_BODY) };
        parts.push(tinted(
            Cylinder::new(r, len).mesh().resolution(6).build().rotated_by(rot).translated_by(mid),
            c,
        ));
        foot += rot * y(len);
    }
    let t = foot; // crown anchor ≈ (0.45·sweep, 1.33, 0)
    let at = |dx: f32, dy: f32, dz: f32| t + Vec3::new(dx * sweep, dy, dz);

    // Snapped branch stubs on the windward side (the wind strips that flank bare).
    let (s1, _) = blade(0.022, 0.16, 0.0, 1.30 * sweep, Vec3::new(0.10 * sweep, 0.62, 0.01), lin(BARK_DARK));
    let (s2, _) = blade(0.018, 0.12, 0.0, 1.45 * sweep, Vec3::new(0.20 * sweep, 0.91, -0.02), lin(SNAG_GREY));
    parts.push(s1);
    parts.push(s2);

    // Foliage flag streaming leeward: dark undersides → mid bodies → sunlit caps.
    parts.push(facet_at(0.16, at(0.02, -0.02, 0.0), 0.6, lin(PINE_DARK)));
    parts.push(facet_at(0.13, at(0.24, -0.05, 0.09), 0.6, lin(PINE_DARK)));
    parts.push(facet_at(0.12, at(-0.13, -0.07, -0.08), 0.6, lin(PINE_DARK)));
    parts.push(facet_at(0.15, at(0.10, 0.08, 0.04), 0.66, lin(PINE_BODY)));
    parts.push(facet_at(0.12, at(0.32, 0.02, -0.06), 0.62, lin(PINE_BODY)));
    parts.push(facet_at(0.11, at(-0.05, 0.10, 0.09), 0.64, lin(PINE_BODY)));
    parts.push(facet_at(0.10, at(0.13, 0.19, 0.0), 0.5, lin(PINE_LIT)));
    parts.push(facet_at(0.08, at(0.31, 0.13, -0.03), 0.5, lin(PINE_LIT)));
    // A lower clump trailing off the mid-trunk, with its own lit cap.
    parts.push(facet_at(0.11, Vec3::new(0.30 * sweep, 0.88, 0.06), 0.6, lin(PINE_DARK)));
    parts.push(facet_at(0.075, Vec3::new(0.34 * sweep, 0.99, 0.05), 0.5, lin(PINE_LIT)));
    if variant % 2 == 1 {
        // The mirrored tree is a touch fuller at the crown.
        parts.push(facet_at(0.09, at(0.44, 0.06, 0.02), 0.55, lin(PINE_BODY)));
    }
    flat_shaded(merged(parts))
}

// ── Weathered snag (part of the "tree" class) ─────────────────────────────────────
// A long-dead tree: a tilted weathered-grey bole rising from a dark root flare to a
// splintered crown (a tall bleached shard + a shorter dark one), one bare near-horizontal
// branch, a snapped stub, and lichen creeping up the shaded side. Base at y=0, ~1.4u.

pub fn build_snag_mesh() -> Mesh {
    let rot = Quat::from_rotation_z(0.06);
    let mut parts = vec![
        // Dark root flare.
        facet_at(0.11, y(0.06), 0.6, lin(BARK_DARK)),
        facet_at(0.07, Vec3::new(0.10, 0.045, 0.05), 0.6, lin_scaled(BARK_DARK, 0.9)),
        facet_at(0.06, Vec3::new(-0.09, 0.04, -0.06), 0.6, lin(SNAG_GREY)),
        // Slightly tilted weathered bole.
        tinted(
            Cylinder::new(0.085, 0.95).mesh().resolution(7).build().rotated_by(rot).translated_by(rot * y(0.475) + y(0.012)),
            lin(SNAG_GREY),
        ),
    ];
    let top = rot * y(0.95) + y(0.012); // ≈ (-0.057, 0.96)
    // Splintered crown: tall bleached shard + shorter dark shard beside it.
    parts.push(tinted(
        Cone { radius: 0.06, height: 0.48 }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(0.24))
            .rotated_by(Quat::from_rotation_z(0.14))
            .translated_by(top),
        lin(SNAG_BLEACH),
    ));
    parts.push(tinted(
        Cone { radius: 0.045, height: 0.26 }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(0.13))
            .rotated_by(Quat::from_rotation_z(-0.34))
            .translated_by(top + Vec3::new(0.03, -0.04, 0.02)),
        lin_scaled(BARK_DARK, 1.05),
    ));
    // One bare near-horizontal branch + a snapped stub lower on the other side.
    parts.push(tinted(
        Cylinder::new(0.02, 0.42)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(0.21))
            .rotated_by(Quat::from_rotation_y(0.5) * Quat::from_rotation_z(1.22))
            .translated_by(Vec3::new(-0.02, 0.68, 0.0)),
        lin_scaled(SNAG_BLEACH, 0.92),
    ));
    let (stub, _) = blade(0.028, 0.14, 3.6, 1.35, Vec3::new(0.02, 0.48, 0.02), lin(BARK_DARK));
    parts.push(stub);
    // Lichen creeping up the shaded (-X) side.
    parts.push(lichen_at(0.055, Vec3::new(-0.085, 0.34, 0.03), LICHEN_GREEN));
    parts.push(lichen_at(0.045, Vec3::new(-0.075, 0.58, -0.02), LICHEN_ORANGE));
    flat_shaded(merged(parts))
}

// ── Dry shrub / dead-grass clump (first non-tree class → tree fallback) ──────────
// A low olive-brown scrub with the light baked in as three tone layers: a dark grounded
// skirt → olive mid body → a sunlit crown lump. Bleached dead-grass spikes (cheap res-4
// cones) lean out of the clump; the odd variant perches bright seed heads on its two
// tallest spikes. Base at y=0, ~0.38u tall.

pub fn build_dry_shrub_mesh(variant: u32) -> Mesh {
    let mut parts = vec![
        // Dark grounded skirt.
        facet_at(0.21, y(0.13), 0.62, lin(SHRUB_DRY_DARK)),
        facet_at(0.16, Vec3::new(0.16, 0.10, 0.05), 0.62, lin(SHRUB_DRY_DARK)),
        facet_at(0.14, Vec3::new(-0.14, 0.09, 0.09), 0.62, lin(SHRUB_DRY_DARK)),
        // Olive mid body.
        facet_at(0.17, y(0.23), 0.78, lin(SHRUB_DRY)),
        facet_at(0.13, Vec3::new(0.11, 0.25, -0.10), 0.78, lin(SHRUB_DRY)),
        facet_at(0.11, Vec3::new(-0.10, 0.24, -0.06), 0.74, lin_scaled(SHRUB_DRY, 0.92)),
        // Sunlit crown.
        facet_at(0.11, y(0.33), 0.8, lin_scaled(SHRUB_DRY, 1.16)),
    ];
    if variant % 2 == 0 {
        parts.push(facet_at(0.08, Vec3::new(0.07, 0.36, -0.04), 0.8, lin_scaled(SHRUB_DRY, 1.24)));
    }
    // Bleached dead-grass spikes leaning out of the clump, alternating two tones.
    let spikes = if variant % 2 == 0 { 6 } else { 5 };
    for i in 0..spikes {
        let a = (i as f32 / spikes as f32) * std::f32::consts::TAU + 0.4;
        let h = 0.26 + (i % 3) as f32 * 0.05;
        let tilt = 0.24 + (i % 2) as f32 * 0.12;
        let foot = Vec3::new(a.cos() * 0.10, 0.05, a.sin() * 0.10);
        let c = if i % 2 == 0 { lin(SHRUB_DEAD) } else { lin_scaled(SHRUB_DEAD, 0.86) };
        let (m, tip) = blade(0.012, h, a, tilt, foot, c);
        parts.push(m);
        if variant % 2 == 1 && i < 2 {
            // Bright seed heads catching the light on the tallest spikes.
            parts.push(facet_at(0.016, tip, 0.8, lin_scaled(SHRUB_DEAD, 1.2)));
        }
    }
    flat_shaded(merged(parts))
}

// ── Crystal / geode cluster (scatter — the one colour accent) ────────────────────
// A small grey rock base (now with a pale lit crown facet + a sage lichen fleck) with a
// fan of 6-sided crystal shards (hex prism + pointed cap) jutting up at mixed tilts and
// sizes — a tall centre shard, four leaners, and two small chips at the foot — in
// saturated amethyst or teal with a pale lit tip. Vertex-colour only (no emissive) so it
// still batches on the shared material. Base at y=0, ~0.6u tall. `variant` = gem theme.

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
        // Grey host rock the crystals erupt from — dark base, lit crown, lichen fleck.
        facet_at(0.22, y(0.14), 0.62, lin(STONE_DARK)),
        facet_at(0.15, Vec3::new(0.13, 0.10, 0.05), 0.7, lin(STONE_BODY)),
        facet_at(0.12, Vec3::new(-0.12, 0.085, -0.06), 0.7, lin_scaled(STONE_DARK, 1.05)),
        facet_at(0.10, Vec3::new(0.02, 0.24, -0.02), 0.45, lin(STONE_PALE)),
        lichen_at(0.05, Vec3::new(-0.15, 0.14, 0.04), LICHEN_SAGE),
    ];

    // A tall central shard + four leaners + two small chips low at the foot.
    parts.push(crystal_shard(0.06, 0.42, 0.0, 0.0, y(0.16), body, tip));
    let shards = [
        (0.05_f32, 0.30_f32, 0.40_f32, 0.6_f32, Vec3::new(0.12, 0.13, 0.04), body_dk),
        (0.045, 0.26, -0.42, 2.1, Vec3::new(-0.13, 0.12, 0.05), body),
        (0.04, 0.22, 0.34, 3.6, Vec3::new(0.05, 0.11, -0.13), body_dk),
        (0.038, 0.20, -0.30, 5.0, Vec3::new(-0.06, 0.11, -0.10), body),
        (0.030, 0.14, 0.55, 1.3, Vec3::new(0.18, 0.05, -0.07), body),
        (0.028, 0.12, -0.50, 4.2, Vec3::new(-0.16, 0.045, -0.10), body_dk),
    ];
    for (r, h, tilt, yaw, base, c) in shards {
        parts.push(crystal_shard(r, h, tilt, yaw, base, c, tip));
    }

    flat_shaded(merged(parts))
}

// ── Scree spill (scatter) ────────────────────────────────────────────────────────
// A directional drift of broken-rock litter: one anchor cobble (with a pale lit cap and
// a sage lichen fleck) trailing a spill of mixed grey/warm/pale pebbles that thins along
// X. Variant 0 grows dry grass + a gold alpine speck in the gaps; variant 1 swaps in a
// pale chip, an orange lichen dot and a violet speck. Base at y=0, very low.

pub fn build_scree_mesh(variant: u32) -> Mesh {
    let mut parts = vec![
        // Anchor cobble, grounded (vertical extent ≈ 0.086 at this tilt).
        block_at(0.15, 0.085, 0.12, y(0.088), 0.1, lin(PEBBLE_GREY)),
        facet_at(0.07, Vec3::new(0.02, 0.15, 0.0), 0.4, lin(STONE_PALE)),
        lichen_at(0.05, Vec3::new(0.12, 0.08, 0.05), LICHEN_SAGE),
        // The drift, thinning toward +X.
        facet_at(0.09, Vec3::new(0.22, 0.055, 0.09), 0.6, lin(PEBBLE_WARM)),
        facet_at(0.075, Vec3::new(-0.19, 0.046, 0.11), 0.6, lin_scaled(PEBBLE_GREY, 0.9)),
        facet_at(0.07, Vec3::new(0.34, 0.043, -0.04), 0.6, lin(STONE_COOL)),
        facet_at(0.06, Vec3::new(-0.31, 0.037, -0.09), 0.6, lin_scaled(PEBBLE_WARM, 1.05)),
        facet_at(0.055, Vec3::new(0.11, 0.034, -0.16), 0.6, lin(PEBBLE_GREY)),
        facet_at(0.05, Vec3::new(-0.07, 0.031, 0.18), 0.6, lin(STONE_PALE)),
        facet_at(0.045, Vec3::new(0.45, 0.028, 0.03), 0.6, lin_scaled(PEBBLE_GREY, 0.94)),
    ];
    if variant % 2 == 0 {
        // Dry grass + a gold alpine speck poking between the stones.
        let (g1, _) = blade(0.013, 0.18, 0.7, 0.30, Vec3::new(-0.13, 0.02, -0.05), lin(DRYTUFT_BASE));
        let (g2, _) = blade(0.012, 0.15, 3.6, 0.35, Vec3::new(-0.11, 0.02, -0.03), lin(DRYTUFT_TIP));
        let (stem, tipp) = blade(0.008, 0.09, 1.8, 0.25, Vec3::new(0.25, 0.01, 0.14), lin(STEM_GREEN));
        parts.push(g1);
        parts.push(g2);
        parts.push(stem);
        parts.push(facet_at(0.018, tipp, 0.75, lin(FLOWER_GOLD)));
    } else {
        // A pale chip + an orange lichen dot + a violet speck.
        parts.push(facet_at(0.05, Vec3::new(0.20, 0.032, -0.13), 0.6, lin_scaled(STONE_PALE, 0.96)));
        parts.push(lichen_at(0.035, Vec3::new(0.23, 0.085, 0.10), LICHEN_ORANGE));
        let (stem, tipp) = blade(0.008, 0.10, 4.6, 0.30, Vec3::new(-0.24, 0.01, 0.12), lin(STEM_GREEN));
        parts.push(stem);
        parts.push(facet_at(0.018, tipp, 0.75, lin(FLOWER_VIOLET)));
    }
    flat_shaded(merged(parts))
}

// ── Rocky ground litter (cover) ──────────────────────────────────────────────────
// The little colour accents on the stony floor. `variant`: 0 = a stone crusted with
// rusty-orange + green/sage lichen and a pale lit fleck, 1 = a tiny crystal sprinkle
// (small amethyst/teal shards on a dark nub), 2 = an alpine wildflower tuft — three thin
// stems with violet/white/gold speck heads between two mini pebbles. ≤0.12u, base y=0.
fn build_rocky_litter_mesh(variant: u32) -> Mesh {
    match variant % 3 {
        // Lichen-crusted stone — grey facet nub, lit crown fleck, three lichen splotches.
        0 => flat_shaded(merged(vec![
            facet_at(0.08, y(0.05), 0.6, lin(STONE_BODY)),
            facet_at(0.05, Vec3::new(0.06, 0.04, 0.02), 0.6, lin_scaled(STONE_DARK, 1.05)),
            facet_at(0.035, Vec3::new(-0.02, 0.092, -0.01), 0.35, lin(STONE_PALE)),
            facet_at(0.045, Vec3::new(0.0, 0.085, 0.0), 0.22, lin(LICHEN_ORANGE)),
            facet_at(0.035, Vec3::new(0.05, 0.072, 0.03), 0.22, lin(LICHEN_GREEN)),
            facet_at(0.028, Vec3::new(-0.04, 0.066, -0.03), 0.22, lin(LICHEN_SAGE)),
        ])),
        // Mini crystal sprinkle — amethyst + teal shards on a dark stone nub.
        1 => {
            let mut parts = vec![facet_at(0.06, y(0.04), 0.6, lin(STONE_DARK))];
            parts.push(crystal_shard(0.025, 0.10, 0.0, 0.0, y(0.06), lin(CRYSTAL_AMETHYST), lin(CRYSTAL_TIP)));
            parts.push(crystal_shard(0.02, 0.08, 0.4, 2.0, Vec3::new(0.04, 0.05, 0.02), lin(CRYSTAL_TEAL), lin(CRYSTAL_TIP)));
            parts.push(crystal_shard(0.016, 0.06, -0.45, 4.1, Vec3::new(-0.035, 0.045, -0.02), lin(CRYSTAL_AMETHYST_DK), lin(CRYSTAL_TIP)));
            flat_shaded(merged(parts))
        }
        // Alpine wildflower tuft — bright pinprick heads on thin leaning stems.
        _ => {
            let mut parts = vec![
                facet_at(0.035, y(0.022), 0.6, lin(PEBBLE_GREY)),
                facet_at(0.028, Vec3::new(0.05, 0.018, 0.02), 0.6, lin(PEBBLE_WARM)),
            ];
            let flowers = [
                (0.4_f32, 0.25_f32, 0.095_f32, FLOWER_VIOLET),
                (2.5, 0.35, 0.080, FLOWER_WHITE),
                (4.6, 0.30, 0.070, FLOWER_GOLD),
            ];
            for &(yaw, tilt, h, c) in &flowers {
                let foot = Vec3::new(yaw.cos() * 0.02, 0.01, yaw.sin() * 0.02);
                let (stem, tipp) = blade(0.007, h, yaw, tilt, foot, lin(STEM_GREEN));
                parts.push(stem);
                parts.push(facet_at(0.018, tipp, 0.75, lin(c)));
            }
            flat_shaded(merged(parts))
        }
    }
}

// ── Ground cover meshes ──────────────────────────────────────────────────────────

/// A single tiny ground pebble pair with the light baked in — grey block + warm chip,
/// a pale lit fleck on the crown and an orange lichen dot on the shaded side. Base y=0.
fn build_cover_pebble_mesh() -> Mesh {
    flat_shaded(merged(vec![
        block_at(0.085, 0.052, 0.07, y(0.054), 0.1, lin(PEBBLE_GREY)),
        facet_at(0.05, Vec3::new(0.075, 0.032, 0.02), 0.6, lin(PEBBLE_WARM)),
        facet_at(0.035, Vec3::new(0.01, 0.09, 0.0), 0.4, lin(STONE_PALE)),
        facet_at(0.02, Vec3::new(-0.055, 0.045, 0.025), 0.25, lin(LICHEN_ORANGE)),
    ]))
}

/// A dry-grass tuft — six lean res-4 blades fanned around the root in alternating
/// base/bleached tones, plus one taller wind-bent blade carrying a bright seed head.
/// Base at y=0, ~0.24u tall, ~80 tris (the old default-res cones cost 64 tris EACH).
fn build_cover_drytuft_mesh() -> Mesh {
    let specs = [
        (0.0_f32, 0.18_f32, 0.21_f32, DRYTUFT_BASE),
        (1.05, -0.24, 0.16, DRYTUFT_TIP),
        (2.10, 0.28, 0.14, DRYTUFT_BASE),
        (3.20, -0.20, 0.17, DRYTUFT_TIP),
        (4.20, 0.26, 0.13, DRYTUFT_BASE),
        (5.20, -0.30, 0.15, DRYTUFT_TIP),
    ];
    let mut parts = Vec::new();
    for &(yaw, tilt, h, c) in &specs {
        let (m, _) = blade(0.015, h, yaw, tilt, y(0.005), lin(c));
        parts.push(m);
    }
    // The seed-head blade, bowing with the wind, its tip catching the light. (Foot a
    // touch higher: at tilt 0.5 the base rim swings ~0.0065 below the root point.)
    let (m, tipp) = blade(0.013, 0.24, 2.7, 0.5, y(0.008), lin(DRYTUFT_BASE));
    parts.push(m);
    parts.push(facet_at(0.016, tipp, 0.8, lin_scaled(DRYTUFT_TIP, 1.15)));
    flat_shaded(merged(parts))
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
        tree_min_dist: 3.2, // hoodoos/pines are tall — keep them well spaced
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
            // Crags — the DOMINANT class. 4 variants (slab crag / split crag / cobble
            // spill / rare cairn), big scale range.
            PropClass {
                variants: vec![
                    (build_boulder_mesh(0), 1.0),
                    (build_boulder_mesh(1), 0.85),
                    (build_boulder_mesh(2), 1.05),
                    (build_boulder_mesh(3), 0.3), // cairns stay a rare waymarker treat
                ],
                chance: 0.085,
                scale: (0.7, 2.2),
                tree: false,
                block_radius: 0.3, // dominant crags — big ones block, scree-sized walk-through
            },
            // The "tree" class (spacing-checked, no sway harm): hoodoos + wind-bent
            // mountain pines (mirrored pair) + a bleached snag. 5 variants.
            PropClass {
                variants: vec![
                    (build_hoodoo_mesh(0), 1.0),
                    (build_hoodoo_mesh(1), 0.75),
                    (build_windpine_mesh(0), 0.9),
                    (build_windpine_mesh(1), 0.9),
                    (build_snag_mesh(), 0.45),
                ],
                chance: 0.024,
                scale: (0.9, 1.8),
                tree: true,
                block_radius: 0.0,
            },
            // Scree spills. 2 variants (grass+gold speck / lichen+violet speck).
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
            // Rocky litter — lichen stones, crystal sprinkles + alpine wildflower tufts.
            PropClass {
                variants: (0..3).map(|v| (build_rocky_litter_mesh(v), 1.0)).collect(),
                chance: 0.11,
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
/// Used for the arch pillars (so the lintel can bridge two of them). Hard pale bands
/// now jut out as ledge rims partway up, and lichen crusts the foot.
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
        if i == 2 || i == 4 {
            // Wind-resistant band → a pale protruding ledge rim at the joint.
            parts.push(facet_at(r * 1.14, y(h * (i as f32 + 1.0)), 0.2, lin(STONE_PALE)));
        }
    }
    // Lichen crusting the sheltered foot.
    parts.push(lichen_at(0.16, Vec3::new(r0 * 0.8, 0.5, 0.2), LICHEN_ORANGE));
    parts.push(lichen_at(0.13, Vec3::new(-r0 * 0.75, 0.9, -0.15), LICHEN_SAGE));
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
    // a natural arch span (two stretched blocks meeting at a high centre), with dark
    // eroded chunks hanging under the span and rubble fallen onto the ground beneath it.
    let lintel = flat_shaded(merged(vec![
        // Left half rising to centre.
        block_at(span * 0.34, 0.55, pillar_r * 0.95, Vec3::new(-span * 0.28, pillar_h + 0.2, 0.0), -0.10, lin(HOODOO_RUST)),
        // Right half.
        block_at(span * 0.34, 0.55, pillar_r * 0.95, Vec3::new(span * 0.28, pillar_h + 0.2, 0.0), 0.10, lin(HOODOO_RUST)),
        // Pale keystone crowning the join.
        block_at(span * 0.16, 0.5, pillar_r * 0.92, Vec3::new(0.0, pillar_h + 0.5, 0.0), 0.0, lin(HOODOO_PALE)),
        // Warm underside band so the span reads layered.
        block_at(span * 0.5, 0.22, pillar_r * 0.8, Vec3::new(0.0, pillar_h - 0.05, 0.0), 0.0, lin(HOODOO_BASE)),
        // Dark eroded chunks clinging under the span (baked shadow weight).
        facet_at(0.5, Vec3::new(-0.7, pillar_h - 0.45, 0.1), 0.8, lin_scaled(STONE_DARK, 0.9)),
        facet_at(0.4, Vec3::new(0.9, pillar_h - 0.4, -0.1), 0.8, lin_scaled(HOODOO_RUST, 0.85)),
        // Rubble fallen from the span onto the ground under the arch.
        slab_at(0.55, 0.28, 0.45, Vec3::new(-0.4, 0.30, 0.6), 0.7, 0.18, lin(STONE_COOL)),
        facet_at(0.35, Vec3::new(0.7, 0.24, -0.5), 0.65, lin_scaled(STONE_BODY, 0.95)),
        facet_at(0.22, Vec3::new(0.1, 0.14, 0.9), 0.6, lin(PEBBLE_WARM)),
    ]));
    commands.spawn((
        Mesh3d(meshes.add(lintel)),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(arch_cx, 0.0, arch_cz),
        BiomeEntity,
    ));

    // ── MESA — a wide flat-topped butte: a broad banded drum stack with a pale caprock
    // (its overhang shadow baked in as a dark band beneath), protruding ledge slabs at
    // the rim, lichen crusts on the cap, and a talus skirt of fallen boulders around its
    // foot. To the right, on land. ──
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
            // Dark band tucked under the caprock overhang (baked contact shadow).
            cyl_up(2.42, 0.12, 4.56, 14, lin_scaled(STONE_DARK, 0.85)),
            // Hard pale caprock (a wider thin slab on top — the erosion-resistant cap).
            cyl_up(2.5, 0.45, 4.83, 16, lin(STONE_PALE)),
        ];
        // Protruding ledge slabs around the cap rim.
        for i in 0..4 {
            let a = i as f32 * 1.7 + 0.5;
            parts.push(slab_at(0.55, 0.16, 0.4, Vec3::new(a.cos() * 2.4, 4.66, a.sin() * 2.4), a, 0.08, lin_scaled(STONE_PALE, 0.97)));
        }
        // Lichen crusts spreading over the sunny cap.
        parts.push(lichen_at(0.22, Vec3::new(1.4, 5.06, 1.6), LICHEN_ORANGE));
        parts.push(lichen_at(0.18, Vec3::new(-1.8, 5.05, 0.9), LICHEN_GREEN));
        // Talus boulders strewn around the base, mixed tones.
        for i in 0..7 {
            let a = (i as f32 / 7.0) * std::f32::consts::TAU + 0.3;
            let rr = 3.1 + (i % 3) as f32 * 0.35;
            let off = Vec3::new(a.cos() * rr, 0.0, a.sin() * rr);
            let s = 0.6 + (i % 3) as f32 * 0.2;
            let c = match i % 3 {
                0 => lin(STONE_COOL),
                1 => lin_scaled(STONE_DARK, 1.05),
                _ => lin(PEBBLE_WARM),
            };
            parts.push(block_at(0.6 * s, 0.34 * s, 0.5 * s, off + y(0.35 * s), 0.12, c));
        }
        flat_shaded(merged(parts))
    };
    commands.spawn((
        Mesh3d(meshes.add(mesa)),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(mesa_cx, 0.0, mesa_cz).with_rotation(Quat::from_rotation_y(0.4)),
        BiomeEntity,
    ));
    // A lone wind-bent pine on the mesa cap + another rooted in the talus below — the
    // classic "tree on the butte" silhouette. (Cap top sits at y ≈ 5.06.)
    let pine0 = meshes.add(build_windpine_mesh(0));
    let pine1 = meshes.add(build_windpine_mesh(1));
    commands.spawn((
        Mesh3d(pine0),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(mesa_cx + 0.6, 5.06, mesa_cz - 0.5)
            .with_scale(Vec3::splat(1.25))
            .with_rotation(Quat::from_rotation_y(2.0)),
        BiomeEntity,
    ));
    commands.spawn((
        Mesh3d(pine1),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(mesa_cx - 3.9, 0.0, mesa_cz + 1.2)
            .with_scale(Vec3::splat(1.5))
            .with_rotation(Quat::from_rotation_y(0.9)),
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

    // A big waymarker CAIRN on the route between the set-pieces.
    let cairn = meshes.add(build_boulder_mesh(3));
    commands.spawn((
        Mesh3d(cairn),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(4.0, 0.0, -10.5)
            .with_scale(Vec3::splat(1.9))
            .with_rotation(Quat::from_rotation_y(0.7)),
        BiomeEntity,
    ));

    // A couple of big loose crags flanking the arch foot to ground the scene.
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
