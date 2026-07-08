//! Ground cover — grass tufts, ferns, red-cap mushrooms, flowers, clover, floor litter.
//!
//! CONTRACT: each returns ONE small merged, vertex-coloured `Mesh`, base at y=0,
//! against the shared white vertex-colour material (spawned `NotShadowCaster`). These
//! are scattered densely. See
//! `docs/specs/forest-biome-props-ground-cover-exact-bevy-rebuild.md`.
//!
//! All visual values (dims / colours / offsets / counts) come from the ground-cover
//! spec; the Rust mesh API (primitive `.mesh().build()`, `translated_by`/`rotated_by`/
//! `scaled_by`, `tinted` + `Mesh::merge`) comes from the verified-APIs doc §9
//! ("Custom Mesh + merging"). Every part is `tinted()` (gets a flat linear
//! `ATTRIBUTE_COLOR`) before merging so the parts share one attribute set and batch.
//!
//! **Merge rule (load-bearing):** `Mesh::merge` only carries indices when BOTH meshes are
//! indexed — so within ONE builder every part must share indexed-ness. The GRASS family is
//! all `blade()` (non-indexed, flat-shaded facets); the FLOWER / CLOVER / FERN / MUSHROOM /
//! LITTER families are all indexed primitives (`cyl_at` / `ball_at` / `petal` / cones / boxes).
//! Don't mix the two inside a single merged mesh or geometry silently corrupts/vanishes.
//!
//! Most builders take a `variant` so the scatterer can mix shapes/colours across a meadow —
//! see each family's `NUM_*_VARIANTS`.

use bevy::prelude::*;

use std::f32::consts::{FRAC_PI_2, TAU};

use crate::palette::lin;
use crate::meshkit::{merged, tinted};

// ── Grass palette ───────────────────────────────────────────────────────────────
// FADED + low-contrast: desaturated, sun-bleached greens so tufts read as soft washed-out foliage
// ("wypłowiałe roślinki") rather than crisp saturated spikes. Centred near the island grass colour
// (`worldmap::grass` = 0x6fb24c) but pulled toward grey so the blades blend into the turf; the
// narrow root→tip band keeps the silhouette gentle, not sharp. Pairs with the now-matte scatter
// material (no specular glint on the edges).
// Centred ON the island grass tone (`worldmap::grass` ≈ 0x6fb24c) with a NARROW root→tip band, so
// a tuft melts into the turf instead of standing off it as a bright spike. Pulling the tip down
// (was 0x8fba7c, a pale yellow-green that "popped") and the whole band toward the ground colour is
// what makes the grass read as part of the meadow — and the gentle value spread softens the hard
// flat-facet self-shading that read as sharp little shadows on every blade.
const TUFT_BASE: u32 = 0x5f8244; // blade root — darker, sinks into the turf
const TUFT_GREEN: u32 = 0x6f9a57; // blade mid — sits right on the ground green
const TUFT_TIP: u32 = 0x82ac6b; // blade tip — only a touch lighter than the ground (low-contrast)
const GRASS_DRY: u32 = 0x9c8c4a; // sun-dried straw blade (muted, no bright pop)
const SEED_HEAD: u32 = 0xbfae78; // pale seed-head spike on flowering grass (softened)

// ── Fern palette ─────────────────────────────────────────────────────────────────
const FERN_GREEN: u32 = 0x2f7e30; // deep fern frond green
const FERN_TIP: u32 = 0x46a047; // lighter frond tip
const FERN_STEM: u32 = 0x33621f; // fern central rachis (darker stalk)
const FERN_DEEP: u32 = 0x214f22; // shadowed bracken green (tall variant)
const FIDDLE_GREEN: u32 = 0x6fae4a; // bright young-frond / crozier green

// ── Mushroom palette ───────────────────────────────────────────────────────────
const MUSH_STEM: u32 = 0xf0e8d0; // mushroom pale stem (#f0e8d0)
const MUSH_RED: u32 = 0xc83838; // red amanita cap (#c83838)
const MUSH_BROWN: u32 = 0x8a5a3a; // brown cap variant (#8a5a3a)
const MUSH_DOT: u32 = 0xf8f6e8; // white cap speckles (#f8f6e8)

// ── Flower palette ───────────────────────────────────────────────────────────────
const FLOWER_STEM: u32 = 0x3a7a2a; // flower green stem (#3a7a2a)
const FLOWER_LEAF: u32 = 0x4a8f34; // brighter stem leaf
const FLOWER_CENTER: u32 = 0xe8c84a; // yellow flower centre (#e8c84a)
const PETAL_PINK: u32 = 0xe88ad6; // pink
const PETAL_YELLOW: u32 = 0xe6c84a; // yellow buttercup
const PETAL_WHITE: u32 = 0xf2f0e4; // white daisy
const PETAL_RED: u32 = 0xd8413a; // poppy red
const PETAL_BLUE: u32 = 0x5878d8; // cornflower blue
const PETAL_PURPLE: u32 = 0xa861cc; // wild violet purple
const PETAL_ORANGE: u32 = 0xe8772e; // tulip orange-red
const BELL_BLUE: u32 = 0x6f7fe0; // hanging bluebell
const POPPY_CORE: u32 = 0x2a1a12; // dark poppy centre

/// How many flower colour/shape variants `build_flower_mesh` produces.
pub const NUM_FLOWER_VARIANTS: u32 = 9;

// ── Clover palette ───────────────────────────────────────────────────────────────
const CLOVER_GREEN: u32 = 0x4a8f3a; // clover leaf green
const CLOVER_DARK: u32 = 0x357a2c; // clover stalk / shadow
const CLOVER_PALE: u32 = 0x9fce7a; // pale chevron watermark on the leaflet
const CLOVER_FLOWER: u32 = 0xf2efe2; // white-clover bloom puff
const SORREL_GREEN: u32 = 0x6aa83a; // brighter broad-leaf / sorrel

/// How many clover/broadleaf variants `build_clover_mesh` produces.
pub const NUM_CLOVER_VARIANTS: u32 = 3;

/// How many grass-tuft shape variants `build_grass_tuft_mesh` produces.
pub const NUM_GRASS_VARIANTS: u32 = 4;

/// How many fern shape variants `build_fern_mesh` produces.
pub const NUM_FERN_VARIANTS: u32 = 3;

// ── Forest-floor litter (pinecones, acorns, pebbles, fallen leaves, twigs) ──────────
const PINECONE: u32 = 0x6e4a2c; // brown pinecone scales
const ACORN_NUT: u32 = 0x9a6536; // acorn nut body
const ACORN_CAP: u32 = 0x5a3a1f; // darker acorn cap
const LITTER_PEBBLE: u32 = 0x9a8f82; // small grey ground pebble
const LITTER_PEBBLE_DK: u32 = 0x7d7466; // shadowed pebble
const LEAF_RED: u32 = 0xc05a30; // fallen autumn leaf (rust)
const LEAF_GOLD: u32 = 0xd0a440; // fallen autumn leaf (gold)
const LEAF_BROWN: u32 = 0x8a6a3a; // fallen leaf (brown)
const TWIG_BARK: u32 = 0x6a4f33; // fallen twig bark

/// How many forest-floor litter variants `build_floor_litter_mesh` produces.
pub const NUM_LITTER_VARIANTS: u32 = 5;

// ── Mesh helpers (verified 0.18 forms, mirrors the verified-APIs doc §9) ───────

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// Linear-colour lerp (component-wise, alpha kept at `a`'s).
fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3]]
}

/// A cylinder whose centre sits at `cy` (so a stem of height `h` rooted at y=0 uses
/// `cy = h / 2`). INDEXED.
fn cyl_at(r: f32, h: f32, cy: f32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(6).build().translated_by(y(cy)), lin(c))
}

/// A small (optionally squashed) faceted icosphere centred at `off`. ico(0) keeps the
/// stylised low-poly facet count tiny — these props are scattered by the thousand. INDEXED.
fn ball_at(r: f32, off: Vec3, squash: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .unwrap()
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        lin(c),
    )
}

/// A thin flat-shaded cone blade rooted at y≈0, leaned outward by `tilt` (about Z) then
/// yawed by `yaw` (about Y) so a clump fans out. 4-sided, NON-INDEXED (flat facets) — the
/// crisp low-poly facet IS the look, and the default 32-segment cone was both soft and ~8×
/// the vertices.
///
/// Each blade carries a per-VERTEX gradient from `root` (dark, shadowed base) to `tip`
/// (sunlit point), keyed off the blade's local height BEFORE it's tilted/yawed. That
/// vertical value ramp is what gives a flat-scattered meadow real depth — every blade darkens
/// into the turf instead of reading as one flat green chip.
fn blade(yaw: f32, tilt: f32, h: f32, r: f32, root: u32, tip: u32) -> Mesh {
    let mut m = Cone { radius: r, height: h }.mesh().resolution(4).build().translated_by(y(h / 2.0));
    m.duplicate_vertices();
    m.compute_flat_normals();
    // Colour by height fraction (root→tip) while the blade is still upright, THEN lean it.
    let (lo, hi) = (lin(root), lin(tip));
    if let Some(pos) = m.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|p| p.as_float3()) {
        let cols: Vec<[f32; 4]> = pos.iter().map(|p| mix(lo, hi, (p[1] / h).clamp(0.0, 1.0))).collect();
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, cols);
    }
    m.rotated_by(Quat::from_rotation_z(tilt)).rotated_by(Quat::from_rotation_y(yaw))
}

/// A single flower petal: a 4-sided cone widened + flattened into a thin blade, laid pointing
/// radially outward from the bloom centre at angle `a`, lifted by `cup` (0 = flat ray, →
/// FRAC_PI_2 = upright cupped), its base at `ring` from centre and `head_y` high. INDEXED
/// (smooth) so it merges with the rest of the flower.
fn petal(a: f32, ring: f32, head_y: f32, len: f32, width: f32, cup: f32, c: u32) -> Mesh {
    let m = Cone { radius: width, height: len }
        .mesh()
        .resolution(4)
        .build()
        .scaled_by(Vec3::new(1.3, 1.0, 0.30)) // widen + flatten → a petal blade, not a spike
        .translated_by(y(len * 0.5))
        .rotated_by(Quat::from_rotation_x(FRAC_PI_2 - cup)) // lay outward, cup up
        .rotated_by(Quat::from_rotation_y(a))
        .translated_by(Vec3::new(a.cos() * ring, head_y, a.sin() * ring));
    tinted(m, lin(c))
}

/// A thin upright flower stem (slender 5-sided cone), optionally leaned by `lean` about Z.
/// INDEXED.
fn stem(head_y: f32, lean: f32, c: u32) -> Mesh {
    tinted(
        Cone { radius: 0.010, height: head_y }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(head_y * 0.5))
            .rotated_by(Quat::from_rotation_z(lean)),
        lin(c),
    )
}

/// A hanging bell flower (bluebell): a small 6-sided cone with its wide mouth opening
/// downward, attached near `off`. INDEXED.
fn bell(off: Vec3, size: f32, c: u32) -> Mesh {
    tinted(Cone { radius: size, height: size * 1.7 }.mesh().resolution(6).build().translated_by(off), lin(c))
}

// ── Grass tuft ─────────────────────────────────────────────────────────────────

/// Grass tuft — `variant` (mod [`NUM_GRASS_VARIANTS`]) picks the clump shape so a meadow mixes
/// lush spires, low turf, seeding stalks and dry wild tufts. Every blade runs a dark-root→
/// bright-tip gradient and the clumps are tiered in height, so the carpet reads layered and
/// self-shading instead of a flat green sprinkle. All-blade (non-indexed), ~0.12–0.40u tall.
pub fn build_grass_tuft_mesh(variant: u32) -> Mesh {
    match variant % NUM_GRASS_VARIANTS {
        // 0 — lush meadow tuft: three height tiers (the carpet's body).
        0 => {
            let mut parts = Vec::with_capacity(13);
            for i in 0..4 {
                let yaw = (i as f32 / 4.0) * TAU + 0.25;
                let tilt = 0.06 + (i % 2) as f32 * 0.06;
                parts.push(blade(yaw, tilt, 0.36 - (i % 2) as f32 * 0.04, 0.026, TUFT_BASE, TUFT_TIP));
            }
            for i in 0..4 {
                let yaw = (i as f32 / 4.0) * TAU + 0.9;
                let tilt = 0.18 + (i % 3) as f32 * 0.06;
                parts.push(blade(yaw, tilt, 0.24 + (i % 2) as f32 * 0.03, 0.022, TUFT_BASE, TUFT_GREEN));
            }
            for i in 0..5 {
                let yaw = (i as f32 / 5.0) * TAU + 1.7;
                let tilt = 0.34 + (i % 3) as f32 * 0.07;
                parts.push(blade(yaw, tilt, 0.15 + (i % 2) as f32 * 0.04, 0.018, TUFT_GREEN, TUFT_TIP));
            }
            merged(parts)
        }
        // 1 — short turf clump: low, dense, rounded — trodden lawn / filler between props.
        1 => {
            let mut parts = Vec::with_capacity(9);
            for i in 0..9 {
                let yaw = (i as f32 / 9.0) * TAU + 0.4;
                let tilt = 0.30 + (i % 4) as f32 * 0.06;
                parts.push(blade(yaw, tilt, 0.11 + (i % 3) as f32 * 0.025, 0.020, TUFT_BASE, TUFT_GREEN));
            }
            merged(parts)
        }
        // 2 — flowering grass: a medium fan + two slender stalks each topped with a pale seed spike.
        2 => {
            let mut parts = Vec::with_capacity(10);
            for i in 0..6 {
                let yaw = (i as f32 / 6.0) * TAU;
                let tilt = 0.16 + (i % 3) as f32 * 0.07;
                parts.push(blade(yaw, tilt, 0.22 + (i % 2) as f32 * 0.05, 0.020, TUFT_BASE, TUFT_GREEN));
            }
            for (j, (yaw, lean)) in [(0.7_f32, 0.10_f32), (3.6, 0.16)].into_iter().enumerate() {
                let h = 0.40 + j as f32 * 0.05;
                // Stalk: a single-colour upright blade (stays non-indexed with the rest).
                parts.push(blade(yaw, lean, h, 0.012, FLOWER_STEM, TUFT_GREEN));
                // Seed head: a short fat blade at the stalk tip, fading green→straw.
                let tip = Quat::from_rotation_y(yaw) * (Quat::from_rotation_z(lean) * y(h));
                parts.push(blade(yaw, lean, 0.085, 0.026, GRASS_DRY, SEED_HEAD).translated_by(tip));
            }
            merged(parts)
        }
        // 3 — wild dry tuft: a broad splayed fan with a couple of sun-dried straw blades mixed in.
        _ => {
            let mut parts = Vec::with_capacity(11);
            for i in 0..11 {
                let yaw = (i as f32 / 11.0) * TAU + 0.2;
                let tilt = 0.22 + (i % 4) as f32 * 0.08;
                let (root, tip) = if i % 5 == 0 { (GRASS_DRY, SEED_HEAD) } else { (TUFT_BASE, TUFT_TIP) };
                parts.push(blade(yaw, tilt, 0.20 + (i % 3) as f32 * 0.06, 0.020, root, tip));
            }
            merged(parts)
        }
    }
}

// ── Fern ───────────────────────────────────────────────────────────────────────

/// A single tapering frond: a 4-sided cone squashed flat (z) + widened (x), base-pivoted,
/// tilted toward horizontal by `lift`, yawed around the rosette, rooted at `base_y`. INDEXED.
fn frond(len: f32, yaw: f32, lift: f32, base_y: f32, c: u32) -> Mesh {
    let m = Cone { radius: 0.045, height: len }
        .mesh()
        .resolution(4)
        .build()
        .scaled_by(Vec3::new(1.6, 1.0, 0.30))
        .translated_by(y(len * 0.5))
        .rotated_by(Quat::from_rotation_x(FRAC_PI_2 - lift))
        .rotated_by(Quat::from_rotation_y(yaw))
        .translated_by(y(base_y));
    tinted(m, lin(c))
}

/// A young fern crozier (fiddlehead): a knobbly coil of shrinking balls curling inward at the
/// top, rooted at `base`, yawed to `yaw`. INDEXED.
fn crozier(base: Vec3, h: f32, yaw: f32, c: u32) -> Mesh {
    let rot = Quat::from_rotation_y(yaw);
    let parts: Vec<Mesh> = (0..5)
        .map(|k| {
            let t = k as f32 / 4.0;
            let r = 0.030 * (1.0 - t * 0.6);
            let local = Vec3::new(t * t * 0.10, t * h, 0.0); // curls over toward the front as it rises
            ball_at(r, base + rot * local, 0.9, c)
        })
        .collect();
    merged(parts)
}

/// Fern — `variant` (mod [`NUM_FERN_VARIANTS`]) picks the shape: a full ground rosette, a young
/// fiddlehead with curled croziers, or tall shadowed bracken. INDEXED, ~0.22–0.36u tall.
pub fn build_fern_mesh(variant: u32) -> Mesh {
    match variant % NUM_FERN_VARIANTS {
        // 0 — full rosette: a low outer spray + a steeper inner ring around a short rachis.
        0 => {
            let mut parts = vec![tinted(
                Cuboid::new(0.018, 0.10, 0.018).mesh().build().translated_by(y(0.05)),
                lin(FERN_STEM),
            )];
            for i in 0..5 {
                let yaw = (i as f32 / 5.0) * TAU;
                let c = if i % 2 == 0 { FERN_GREEN } else { FERN_TIP };
                parts.push(frond(0.30, yaw, 0.50 + (i % 2) as f32 * 0.10, 0.05, c));
            }
            for i in 0..3 {
                let yaw = (i as f32 / 3.0) * TAU + 0.6;
                parts.push(frond(0.20, yaw, 0.95, 0.05, FERN_TIP));
            }
            merged(parts)
        }
        // 1 — fiddlehead: a couple of opening fronds + two curled croziers rising from the heart.
        1 => {
            let mut parts = vec![tinted(
                Cuboid::new(0.016, 0.07, 0.016).mesh().build().translated_by(y(0.035)),
                lin(FERN_STEM),
            )];
            for i in 0..3 {
                let yaw = (i as f32 / 3.0) * TAU + 0.3;
                parts.push(frond(0.22, yaw, 0.65, 0.04, FIDDLE_GREEN));
            }
            parts.push(crozier(y(0.03), 0.26, 0.8, FIDDLE_GREEN));
            parts.push(crozier(Vec3::new(0.04, 0.03, -0.02), 0.20, 3.7, FERN_TIP));
            merged(parts)
        }
        // 2 — tall bracken: fewer, taller, steeper fronds in a deep shadowed green.
        _ => {
            let mut parts = vec![tinted(
                Cuboid::new(0.020, 0.16, 0.020).mesh().build().translated_by(y(0.08)),
                lin(FERN_STEM),
            )];
            for i in 0..5 {
                let yaw = (i as f32 / 5.0) * TAU + 0.4;
                let c = if i % 2 == 0 { FERN_DEEP } else { FERN_GREEN };
                parts.push(frond(0.34, yaw, 1.05 + (i % 2) as f32 * 0.08, 0.07, c));
            }
            merged(parts)
        }
    }
}

// ── Mushroom (red amanita) ───────────────────────────────────────────────────

/// Red-cap amanita: a pale white stem + a domed cap (squashed half-ball) + (red variant) a few
/// tiny white speckle boxes on the cap. `variant`: even = red cap with white spots, odd = brown
/// cap with no spots; variant ≥ 2 builds a slightly larger mushroom (the spec's 2-size cluster).
/// INDEXED, ~0.15u tall.
pub fn build_mushroom_mesh(variant: u32) -> Mesh {
    let s = if variant >= 2 { 1.25 } else { 1.0 };
    let red = variant % 2 == 0;
    let cap = if red { MUSH_RED } else { MUSH_BROWN };

    let stem_h = 0.10 * s;
    let cap_y = stem_h;
    let cap_r = 0.09 * s;

    let mut parts = vec![
        // Pale stem with a skirt bulge at the foot (amanita volva) so it roots visibly.
        cyl_at(0.030 * s, stem_h, stem_h * 0.55, MUSH_STEM),
        ball_at(0.042 * s, y(0.02 * s), 0.7, MUSH_STEM),
        // Pale gill plate tucked under the cap rim (a wide squashed disc).
        ball_at(cap_r * 0.88, y(cap_y - 0.008 * s), 0.22, MUSH_STEM),
        // Domed cap overhanging the gills.
        ball_at(cap_r, y(cap_y), 0.62, cap),
    ];
    if red {
        for &(dx, dz, dy) in &[
            (0.045_f32, 0.02_f32, 0.040_f32),
            (-0.035, -0.04, 0.042),
            (0.01, 0.05, 0.048),
            (-0.05, 0.025, 0.034),
            (0.02, -0.055, 0.036),
        ] {
            let spot = Cuboid::new(0.018 * s, 0.012 * s, 0.018 * s)
                .mesh()
                .build()
                .translated_by(Vec3::new(dx * s, cap_y + dy * s, dz * s));
            parts.push(tinted(spot, lin(MUSH_DOT)));
        }
        // A baby cap budding by the big one — amanitas grow in pairs.
        parts.push(cyl_at(0.018 * s, 0.05 * s, 0.025 * s, MUSH_STEM).translated_by(Vec3::new(0.085 * s, 0.0, 0.04 * s)));
        parts.push(ball_at(0.045 * s, Vec3::new(0.085 * s, 0.05 * s, 0.04 * s), 0.62, cap));
    }
    merged(parts)
}

// ── Flower ───────────────────────────────────────────────────────────────────

/// Flower — `variant` (mod [`NUM_FLOWER_VARIANTS`]) picks colour + shape, so a meadow reads as a
/// mix of daisies, buttercups, pink cosmos, poppies, cornflowers, violets, an oxeye daisy, a
/// tulip and drooping bluebells. Petals are real flattened blades (not blobs) for crisp blooms,
/// each on a leafed stem. INDEXED, ~0.16–0.30u tall.
pub fn build_flower_mesh(variant: u32) -> Mesh {
    // Two small leaf blades partway up a stem (so it reads as a plant, not a lollipop).
    let leaves = |head_y: f32| -> [Mesh; 2] {
        [
            ball_at(0.030, Vec3::new(0.035 * 0.62, head_y * 0.35, 0.035 * 0.78), 0.28, FLOWER_LEAF),
            ball_at(0.028, Vec3::new(-0.035 * 0.86, head_y * 0.55, -0.035 * 0.51), 0.28, FLOWER_LEAF),
        ]
    };
    // A ring of `n` petals (optionally a second offset ring for a fuller bloom).
    let ring = |parts: &mut Vec<Mesh>, n: u32, ring_r: f32, head_y: f32, len: f32, w: f32, cup: f32, c: u32, phase: f32| {
        for i in 0..n {
            let a = (i as f32 / n as f32) * TAU + phase;
            parts.push(petal(a, ring_r, head_y, len, w, cup, c));
        }
    };

    match variant % NUM_FLOWER_VARIANTS {
        // 0 — white daisy: a ring of pointed white rays around a yellow eye.
        0 => {
            let h = 0.17;
            let mut parts = vec![stem(h, 0.0, FLOWER_STEM), ball_at(0.024, y(h), 0.7, FLOWER_CENTER)];
            parts.extend(leaves(h));
            ring(&mut parts, 9, 0.030, h + 0.004, 0.072, 0.020, 0.30, PETAL_WHITE, 0.0);
            merged(parts)
        }
        // 1 — buttercup: five broad rounded yellow petals, low and shiny.
        1 => {
            let h = 0.16;
            let mut parts = vec![stem(h, 0.05, FLOWER_STEM), ball_at(0.020, y(h), 0.8, POPPY_CORE)];
            parts.extend(leaves(h));
            ring(&mut parts, 5, 0.026, h + 0.004, 0.060, 0.034, 0.45, PETAL_YELLOW, 0.0);
            merged(parts)
        }
        // 2 — pink cosmos: eight slender pink petals.
        2 => {
            let h = 0.18;
            let mut parts = vec![stem(h, 0.0, FLOWER_STEM), ball_at(0.022, y(h), 0.7, FLOWER_CENTER)];
            parts.extend(leaves(h));
            ring(&mut parts, 8, 0.030, h + 0.004, 0.068, 0.024, 0.35, PETAL_PINK, 0.0);
            merged(parts)
        }
        // 3 — poppy: five broad cupped red petals, dark core, taller stem.
        3 => {
            let h = 0.21;
            let mut parts = vec![stem(h, 0.06, FLOWER_STEM), ball_at(0.026, y(h), 0.7, POPPY_CORE)];
            parts.extend(leaves(h));
            ring(&mut parts, 5, 0.028, h + 0.002, 0.066, 0.044, 0.62, PETAL_RED, 0.0);
            merged(parts)
        }
        // 4 — cornflower: many thin blue rays in two offset rings (fringed look).
        4 => {
            let h = 0.19;
            let mut parts = vec![stem(h, 0.0, FLOWER_STEM), ball_at(0.018, y(h), 0.7, POPPY_CORE)];
            parts.extend(leaves(h));
            ring(&mut parts, 7, 0.030, h + 0.004, 0.058, 0.016, 0.40, PETAL_BLUE, 0.0);
            ring(&mut parts, 7, 0.022, h + 0.020, 0.044, 0.014, 0.70, PETAL_BLUE, 0.45);
            merged(parts)
        }
        // 5 — violet: five small purple petals, low to the ground.
        5 => {
            let h = 0.13;
            let mut parts = vec![stem(h, 0.10, FLOWER_STEM), ball_at(0.016, y(h), 0.8, FLOWER_CENTER)];
            parts.extend(leaves(h));
            ring(&mut parts, 5, 0.024, h + 0.002, 0.050, 0.030, 0.50, PETAL_PURPLE, 0.0);
            merged(parts)
        }
        // 6 — oxeye daisy: tall, many thin white rays.
        6 => {
            let h = 0.27;
            let mut parts = vec![stem(h, 0.0, FLOWER_STEM), ball_at(0.026, y(h), 0.7, FLOWER_CENTER)];
            parts.extend(leaves(h));
            ring(&mut parts, 12, 0.032, h + 0.004, 0.080, 0.015, 0.28, PETAL_WHITE, 0.0);
            merged(parts)
        }
        // 7 — tulip: three/four upright orange petals forming a closed cup, broad leaves.
        7 => {
            let h = 0.20;
            let mut parts = vec![
                stem(h, 0.0, FLOWER_STEM),
                // Big sheath leaves clasping the lower stem.
                ball_at(0.050, Vec3::new(0.02, h * 0.30, 0.0), 0.18, FLOWER_LEAF),
                ball_at(0.046, Vec3::new(-0.02, h * 0.45, 0.0), 0.18, FLOWER_LEAF),
            ];
            ring(&mut parts, 4, 0.014, h + 0.010, 0.075, 0.030, 1.15, PETAL_ORANGE, 0.0);
            ring(&mut parts, 3, 0.010, h + 0.030, 0.060, 0.026, 1.30, PETAL_RED, 0.5);
            merged(parts)
        }
        // 8 — bluebell: a tall drooping stem hung with a few blue bells opening downward.
        _ => {
            let h = 0.26;
            let mut parts = vec![stem(h, 0.18, FLOWER_STEM)];
            parts.extend(leaves(h));
            // Three bells hanging off the upper, leaning stem at descending heights.
            for k in 0..3 {
                let t = k as f32;
                let off = Vec3::new(0.030 + t * 0.018, h - 0.05 - t * 0.05, t * 0.010 - 0.01);
                parts.push(bell(off, 0.028, BELL_BLUE));
            }
            merged(parts)
        }
    }
}

// ── Forest-floor litter ──────────────────────────────────────────────────────────

/// Tiny forest-floor litter that makes the ground feel lived-in. `variant` (mod
/// [`NUM_LITTER_VARIANTS`]): 0 = pinecone, 1 = acorn, 2 = pebble cluster, 3 = fallen autumn
/// leaves, 4 = a couple of crossed twigs. All very low (≤0.12u), base flush at y=0. INDEXED.
pub fn build_floor_litter_mesh(variant: u32) -> Mesh {
    match variant % NUM_LITTER_VARIANTS {
        // Pinecone — three stacked squashed brown balls tapering to a tip.
        0 => merged(vec![
            ball_at(0.045, y(0.04), 1.15, PINECONE),
            ball_at(0.036, y(0.085), 1.15, PINECONE),
            ball_at(0.024, y(0.115), 1.1, PINECONE),
        ]),
        // Acorn — a rounded nut with a darker textured cap + a tiny stalk.
        1 => merged(vec![
            ball_at(0.040, y(0.035), 1.05, ACORN_NUT),
            ball_at(0.044, y(0.066), 0.55, ACORN_CAP),
            tinted(
                Cylinder::new(0.008, 0.03).mesh().resolution(5).build().translated_by(y(0.092)),
                lin(ACORN_CAP),
            ),
        ]),
        // Pebble cluster — two or three small grey stones.
        2 => merged(vec![
            ball_at(0.050, y(0.028), 0.55, LITTER_PEBBLE),
            ball_at(0.036, Vec3::new(0.06, 0.020, 0.03), 0.5, LITTER_PEBBLE_DK),
            ball_at(0.030, Vec3::new(-0.05, 0.018, -0.04), 0.5, LITTER_PEBBLE),
        ]),
        // Fallen leaves — a few flat tinted discs lying on the ground, lightly overlapping.
        3 => {
            let leaf = |r: f32, off: Vec3, c: u32| -> Mesh {
                tinted(
                    Circle::new(r)
                        .mesh()
                        .resolution(6)
                        .build()
                        .rotated_by(Quat::from_rotation_x(-FRAC_PI_2))
                        .translated_by(off),
                    lin(c),
                )
            };
            merged(vec![
                leaf(0.06, y(0.004), LEAF_RED),
                leaf(0.055, Vec3::new(0.07, 0.008, 0.02), LEAF_GOLD),
                leaf(0.05, Vec3::new(-0.05, 0.012, 0.05), LEAF_BROWN),
            ])
        }
        // Twigs — two slender bark cylinders crossed on the ground, plus a stub.
        _ => {
            let twig = |len: f32, off: Vec3, yaw: f32, tilt: f32| -> Mesh {
                tinted(
                    Cylinder::new(0.008, len)
                        .mesh()
                        .resolution(5)
                        .build()
                        .rotated_by(Quat::from_rotation_z(FRAC_PI_2 - tilt)) // lay nearly flat
                        .rotated_by(Quat::from_rotation_y(yaw))
                        .translated_by(off),
                    lin(TWIG_BARK),
                )
            };
            merged(vec![
                twig(0.16, y(0.012), 0.4, 0.06),
                twig(0.12, Vec3::new(0.01, 0.020, 0.02), 1.9, 0.10),
                twig(0.06, Vec3::new(-0.05, 0.010, -0.03), 2.7, 0.0),
            ])
        }
    }
}

// ── Clover ────────────────────────────────────────────────────────────────────

/// Three trefoil leaflets (rounded flat balls on short stalks, each with a pale chevron
/// watermark) at 120° around the origin — the shared base of the clover/broadleaf variants.
fn trefoil(parts: &mut Vec<Mesh>, leaf_r: f32, leaf_y: f32, ring: f32, leaf_c: u32) {
    for i in 0..3 {
        let a = (i as f32 / 3.0) * TAU + 0.3;
        let off = Vec3::new(a.cos() * ring, leaf_y, a.sin() * ring);
        // Short stalk under the leaflet.
        parts.push(cyl_at(0.006, leaf_y, leaf_y * 0.5, CLOVER_DARK).translated_by(Vec3::new(off.x * 0.5, 0.0, off.z * 0.5)));
        // Leaflet: a low rounded flat ball.
        parts.push(ball_at(leaf_r, off, 0.26, leaf_c));
        // Pale chevron watermark, a touch above + inward of the leaflet centre.
        parts.push(ball_at(leaf_r * 0.42, off + Vec3::new(-off.x * 0.25, 0.010, -off.z * 0.25), 0.18, CLOVER_PALE));
    }
}

/// Clover / broadleaf patch — `variant` (mod [`NUM_CLOVER_VARIANTS`]): 0 = a trefoil shamrock,
/// 1 = white clover (trefoil + a little white bloom puff on a stalk), 2 = a brighter broadleaf
/// (sorrel) clump of larger flat leaves. Low and living, INDEXED, ~0.06–0.12u tall.
pub fn build_clover_mesh(variant: u32) -> Mesh {
    match variant % NUM_CLOVER_VARIANTS {
        // 0 — classic three-leaf shamrock.
        0 => {
            let mut parts = Vec::new();
            trefoil(&mut parts, 0.040, 0.050, 0.045, CLOVER_GREEN);
            merged(parts)
        }
        // 1 — white clover: trefoil + a small white bloom puff (cluster of balls) on a thin stalk.
        1 => {
            let mut parts = Vec::new();
            trefoil(&mut parts, 0.036, 0.046, 0.042, CLOVER_GREEN);
            let head = y(0.105);
            parts.push(cyl_at(0.006, 0.10, 0.05, CLOVER_DARK));
            for &(dx, dz, dy) in &[(0.0_f32, 0.0_f32, 0.0_f32), (0.018, 0.0, 0.006), (-0.014, 0.012, 0.008), (0.006, -0.016, 0.004)] {
                parts.push(ball_at(0.016, head + Vec3::new(dx, dy, dz), 0.85, CLOVER_FLOWER));
            }
            merged(parts)
        }
        // 2 — broadleaf / sorrel: four larger, brighter flat leaves splayed low.
        _ => {
            let mut parts = Vec::new();
            for i in 0..4 {
                let a = (i as f32 / 4.0) * TAU + 0.5;
                let ring = 0.050;
                let off = Vec3::new(a.cos() * ring, 0.042 + (i % 2) as f32 * 0.012, a.sin() * ring);
                parts.push(cyl_at(0.006, off.y, off.y * 0.5, CLOVER_DARK).translated_by(Vec3::new(off.x * 0.45, 0.0, off.z * 0.45)));
                // Elongated flat leaf (wider along its outward axis).
                let leaf = Sphere::new(0.044)
                    .mesh()
                    .ico(0)
                    .unwrap()
                    .scaled_by(Vec3::new(1.4, 0.22, 0.95))
                    .rotated_by(Quat::from_rotation_y(a))
                    .translated_by(off);
                parts.push(tinted(leaf, lin(SORREL_GREEN)));
            }
            merged(parts)
        }
    }
}
