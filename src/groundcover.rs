//! Ground cover — grass tufts, ferns, red-cap mushrooms, flowers, clover.
//!
//! CONTRACT: each returns ONE small merged, vertex-coloured `Mesh`, base at y=0,
//! against the shared white vertex-colour material (spawned `NotShadowCaster`). These
//! are scattered densely. See
//! `docs/specs/forest-biome-props-ground-cover-exact-bevy-rebuild.md` + `CONTRACT.md`.
//!
//! All visual values (dims / colours / offsets / counts) come from the ground-cover
//! spec; the Rust mesh API (primitive `.mesh().build()`, `translated_by`/`rotated_by`/
//! `scaled_by`, `tinted` + `Mesh::merge`) comes from `CONTRACT.md` §"mesh-building
//! pattern" + the verified-APIs doc §9. Every part is `tinted()` (gets a flat linear
//! `ATTRIBUTE_COLOR`) before merging so the parts share one attribute set and batch.

use bevy::prelude::*;

use crate::palette::lin;

// ── Local ground-cover palette (exact hex from the spec) ───────────────────────
const TUFT_GREEN: u32 = 0x3aa044; // grass blade base (#3aa044)
const TUFT_TIP: u32 = 0x5fc060; // lighter blade tip for the two-tone clump
const FERN_GREEN: u32 = 0x2f7e30; // deep fern frond green
const FERN_TIP: u32 = 0x46a047; // lighter frond tip
const FERN_STEM: u32 = 0x33621f; // fern central rachis (darker stalk)

const MUSH_STEM: u32 = 0xf0e8d0; // mushroom pale stem (#f0e8d0)
const MUSH_RED: u32 = 0xc83838; // red amanita cap (#c83838)
const MUSH_BROWN: u32 = 0x8a5a3a; // brown cap variant (#8a5a3a)
const MUSH_DOT: u32 = 0xf8f6e8; // white cap speckles (#f8f6e8)

const FLOWER_STEM: u32 = 0x3a7a2a; // flower green stem (#3a7a2a)
const FLOWER_CENTER: u32 = 0xe8c84a; // yellow flower centre (#e8c84a)
const PETAL_PINK: u32 = 0xe88ad6; // variant 0 — pink (#e88ad6)
const PETAL_YELLOW: u32 = 0xe6c84a; // variant 1 — yellow (#e6c84a)
const PETAL_WHITE: u32 = 0xf2f0e4; // variant 2 — white

const CLOVER_GREEN: u32 = 0x4a8f3a; // clover leaf green

// ── Mesh helpers (verified 0.18 forms, mirrors CONTRACT §mesh-building) ────────

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// Tag every vertex of `m` with one flat linear colour (REQUIRED before merge — all
/// merged parts must carry the same attribute set, incl. `ATTRIBUTE_COLOR`).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge several tinted parts into ONE mesh (so identical props still batch into one
/// draw call). `Mesh::merge` returns `Result` in 0.18 — unwrap it.
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("ground-cover parts share attributes");
    }
    base
}

/// A cylinder whose centre sits at `cy` (so a stem of height `h` rooted at y=0 uses
/// `cy = h / 2`).
fn cyl_at(r: f32, h: f32, cy: f32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(6).build().translated_by(y(cy)), lin(c))
}

/// A small (optionally squashed) faceted icosphere centred at `off`. ico(0) keeps the
/// stylised low-poly facet count tiny — these props are scattered by the thousand.
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

/// A thin flat-shaded cone blade rooted at y≈0, leaned outward by `tilt` (about Z)
/// then yawed by `yaw` (about Y) so a clump of them fans out. Flat-shaded so the blade
/// reads as a crisp spike, not a soft round cone.
fn blade(yaw: f32, tilt: f32, h: f32, r: f32, c: u32) -> Mesh {
    let mut m = Cone { radius: r, height: h }
        .mesh()
        .build()
        .translated_by(y(h / 2.0))
        .rotated_by(Quat::from_rotation_z(tilt))
        .rotated_by(Quat::from_rotation_y(yaw));
    m.duplicate_vertices();
    m.compute_flat_normals();
    tinted(m, lin(c))
}

// ── Grass tuft ─────────────────────────────────────────────────────────────────

/// Grass tuft: 5 thin tapered cone blades fanned around the clump, ~0.26u tall, leaned
/// + yawed out so it reads as a spiky clump (port of `Scatter.tsx` PARTS.tuft — 5 cones,
/// radii 0.025→0.02, heights 0.26→0.17, exact offsets/rotations from the spec). Green
/// base (#3aa044) → lighter tip (#5fc060): the two taller central blades use the base
/// tone, the shorter outer blades the lighter tip tone, so the clump reads two-tone.
pub fn build_grass_tuft_mesh() -> Mesh {
    // (yaw, tilt, height, radius, colour) — spec blade table, with the tilt encoding
    // each blade's lean (the spec's combined x/z euler tilts folded into one Z lean).
    let specs = [
        (0.0_f32, 0.00_f32, 0.26_f32, 0.025_f32, TUFT_GREEN),
        (0.5, 0.22, 0.22, 0.022, TUFT_GREEN),
        (-0.4, -0.20, 0.20, 0.022, TUFT_TIP),
        (1.9, 0.15, 0.18, 0.020, TUFT_TIP),
        (-1.7, -0.18, 0.17, 0.020, TUFT_TIP),
    ];
    let parts = specs
        .iter()
        .map(|&(yaw, tilt, h, r, c)| blade(yaw, tilt, h, r, c))
        .collect();
    merged(parts)
}

// ── Fern ───────────────────────────────────────────────────────────────────────

/// Fern: a low spray of several angled fronds radiating from the base, deep green,
/// ~0.3u tall. Each frond is a thin flattened box (a leaf blade) tilted up + outward;
/// they fan around the clump in a low rosette. A short darker central stalk anchors it.
pub fn build_fern_mesh() -> Mesh {
    const FROND_LEN: f32 = 0.30;
    let mut parts = vec![
        // Short central rachis (a thin upright box) so the fronds read as rooted.
        tinted(
            Cuboid::new(0.018, 0.10, 0.018).mesh().build().translated_by(y(0.05)),
            lin(FERN_STEM),
        ),
    ];
    // 6 fronds fanned around the clump: a thin flattened box, pivoted at the base, laid
    // out almost flat (low spray) with a slight upward lift, alternating two green tones.
    for i in 0..6 {
        let yaw = (i as f32 / 6.0) * std::f32::consts::TAU;
        let lift = if i % 2 == 0 { 0.62 } else { 0.50 }; // radians from horizontal
        let c = if i % 2 == 0 { FERN_GREEN } else { FERN_TIP };
        // Build a thin flat leaf along +Y (length FROND_LEN), shift so its base is at the
        // origin, tilt it down toward horizontal (about X), then yaw it around the clump.
        let frond = Cuboid::new(0.05, FROND_LEN, 0.012)
            .mesh()
            .build()
            .translated_by(y(FROND_LEN * 0.5))
            // tilt away from vertical: PI/2 - lift leans it toward the ground (low spray).
            .rotated_by(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - lift))
            .rotated_by(Quat::from_rotation_y(yaw))
            // lift the whole frond a touch so it sprays from ~0.05 above ground, base ≥ 0.
            .translated_by(y(0.05));
        parts.push(tinted(frond, lin(c)));
    }
    merged(parts)
}

// ── Mushroom (red amanita) ───────────────────────────────────────────────────

/// Red-cap amanita: a pale white stem + a domed cap (squashed half-ball) + (red
/// variant) a few tiny white speckle boxes on the cap. `variant`: even = red cap
/// (#c83838) with white spots, odd = brown cap (#8a5a3a) with no spots. Two sizes via
/// the variant too — variant ≥ 2 builds a slightly larger mushroom. ~0.15u tall.
pub fn build_mushroom_mesh(variant: u32) -> Mesh {
    // Size: variants 0/1 are small, 2/3 a touch bigger (the spec's 2-size cluster).
    let s = if variant >= 2 { 1.25 } else { 1.0 };
    // Cap colour: even = red amanita, odd = brown.
    let red = variant % 2 == 0;
    let cap = if red { MUSH_RED } else { MUSH_BROWN };

    let stem_h = 0.10 * s;
    let cap_y = stem_h; // cap sits at the top of the stem
    let cap_r = 0.09 * s;

    let mut parts = vec![
        // Pale stem (slightly tapered look approximated with a thin cylinder).
        cyl_at(0.034 * s, stem_h, stem_h * 0.5, MUSH_STEM),
        // Domed cap: a squashed half-ball resting on the stem.
        ball_at(cap_r, y(cap_y), 0.62, cap),
    ];
    // White speckles only on the red amanita cap (a few tiny boxes near the crown).
    if red {
        for &(dx, dz) in &[(0.045_f32, 0.02_f32), (-0.035, -0.04), (0.01, 0.05)] {
            let spot = Cuboid::new(0.020 * s, 0.014 * s, 0.020 * s)
                .mesh()
                .build()
                .translated_by(Vec3::new(dx * s, cap_y + 0.045 * s, dz * s));
            parts.push(tinted(spot, lin(MUSH_DOT)));
        }
    }
    merged(parts)
}

// ── Flower ───────────────────────────────────────────────────────────────────

/// Flower: a thin green stem + a small bright petal head — a ring of 5 petal balls
/// around a yellow centre. `variant`: 0 pink, 1 yellow, 2 white (anything else wraps).
/// ~0.18u tall.
pub fn build_flower_mesh(variant: u32) -> Mesh {
    let petal = match variant % 3 {
        0 => PETAL_PINK,
        1 => PETAL_YELLOW,
        _ => PETAL_WHITE,
    };
    const HEAD_Y: f32 = 0.16;
    const RING_R: f32 = 0.045;
    let mut parts = vec![
        // Thin green stem (a slender cone from the ground up to the bloom).
        tinted(
            Cone { radius: 0.010, height: HEAD_Y }.mesh().build().translated_by(y(HEAD_Y * 0.5)),
            lin(FLOWER_STEM),
        ),
        // Yellow centre disc (small squashed ball at the bloom).
        ball_at(0.024, y(HEAD_Y), 0.7, FLOWER_CENTER),
    ];
    // Ring of 5 petals around the centre (small flattened balls).
    for i in 0..5 {
        let a = (i as f32 / 5.0) * std::f32::consts::TAU;
        parts.push(ball_at(
            0.030,
            Vec3::new(a.cos() * RING_R, HEAD_Y, a.sin() * RING_R),
            0.55,
            petal,
        ));
    }
    merged(parts)
}

// ── Clover ────────────────────────────────────────────────────────────────────

/// Clover: a tiny tri-leaf clump — 3 small flattened green discs in a triangle, very
/// low to the ground (~0.06u), each on a stub. Base at y=0.
pub fn build_clover_mesh() -> Mesh {
    const LEAF_Y: f32 = 0.05;
    const RING_R: f32 = 0.04;
    let mut parts = Vec::new();
    for i in 0..3 {
        let a = (i as f32 / 3.0) * std::f32::consts::TAU;
        let off = Vec3::new(a.cos() * RING_R, LEAF_Y, a.sin() * RING_R);
        // Leaf: a small flattened (very squashed) ball — a low rounded disc.
        parts.push(ball_at(0.035, off, 0.30, CLOVER_GREEN));
    }
    merged(parts)
}
