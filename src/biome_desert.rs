//! Desert / Pustynia biome (key 4) — a hot sandy desert. Warm sand ground with
//! wind-ripple streaks, a bright hazy high-illuminance sun, pale warm sky, no river.
//!
//! Self-contained like the forest reference: this module builds ALL its own prop
//! meshes (saguaro + barrel cactus, dead bush/tumbleweed, bleached rocks + a bone/skull,
//! and ground cover: pebbles, dry grass, tiny succulents) inline, plus an OASIS
//! landmark (palms + reeds around a still blue-green pool) in `landmarks`.
//!
//! CONTRACT (mirrors trees.rs / props.rs / decor.rs): every prop is ONE merged,
//! vertex-coloured `Mesh`, base flush at y=0, built from primitives via `Mesh::merge`
//! with every part `tinted` (linear `ATTRIBUTE_COLOR`) BEFORE the merge, then
//! flat-shaded for the crisp low-poly facets the project wants. Props share the one
//! white vertex-colour material the scatter owns, so colour MUST live in the mesh.
//!
//! Land/dune split: low rolling dunes fill the `z < 0` half (`land_dir = -π/2`),
//! no ocean (a desert reaching to the hazy horizon), no treeline.

// The `landmarks()` OASIS set-piece + its mesh helpers/palette consts below are authored biome
// content the world map doesn't place yet (it uses `ruins` landmarks instead). Kept per design;
// allow the resulting dead code until it's wired into a per-region pass.
#![allow(dead_code)]

use bevy::prelude::*;

use crate::biome::{Backdrop, Biome, BiomeConfig, BiomeEntity, GroundDetail, ParticleKind, PropClass};
use crate::palette::lin;

const TAU: f32 = std::f32::consts::TAU;
const FRAC_PI_2: f32 = std::f32::consts::FRAC_PI_2;

// ── Desert palette (TS terrainDetail `sand` + dust + hand-picked prop tones) ──────
const SAND_GROUND: u32 = 0xdcc081; // warm sand base (TS sand `base`)
const SAND_DARK: u32 = 0xc2a566; // shadowed ripple trough (TS sand `dark`)
const SAND_LIGHT: u32 = 0xefd9a0; // sunlit ripple crest (TS sand `light`)

// Cacti — deep saturated desert green with a darker shaded side + ribbed highlight.
const CACTUS_DARK: u32 = 0x2f6b3a; // shadowed cactus flesh
const CACTUS_MID: u32 = 0x3e8f4a; // body green
const CACTUS_LIGHT: u32 = 0x5cb05e; // sunlit ribbed crest
const CACTUS_SPINE: u32 = 0xe8dcb0; // pale spine fleck
const FLOWER_RED: u32 = 0xd83a3a; // saguaro crown bloom (red)
const FLOWER_YELLOW: u32 = 0xe8c24a; // barrel crown bloom (yellow)
const FLOWER_PINK: u32 = 0xe06aa0; // barrel crown bloom (pink)

// Dead bush / tumbleweed — dry tan twigs.
const TWIG_TAN: u32 = 0xb79862; // dry twig
const TWIG_DARK: u32 = 0x8f7444; // shadowed twig

// Sun-bleached rocks — pale warm grey-tan stone.
const ROCK_BODY: u32 = 0xc7b58c; // bleached stone body
const ROCK_TOP: u32 = 0xe0d2ad; // sun-bleached top facet
const ROCK_SHADE: u32 = 0xa8966f; // shadowed side lump

// Bleached bone / skull — near-white ivory.
const BONE: u32 = 0xe8e2cf; // sun-bleached bone
const BONE_SHADE: u32 = 0xcfc7ae; // bone underside
const SOCKET: u32 = 0x3a3024; // dark eye socket

// Oasis — palm trunk rings, frond green, still water, reeds, dates.
const PALM_TRUNK: u32 = 0x8a6a40; // ringed palm trunk
const PALM_TRUNK_RING: u32 = 0x6f5230; // darker trunk ring band
const FROND_DARK: u32 = 0x2f7a3a; // drooping frond underside
const FROND_LIGHT: u32 = 0x49a352; // sunlit frond
const DATE: u32 = 0x6e4326; // date cluster
const OASIS_WATER: u32 = 0x2fae9a; // still blue-green pool
const REED_GREEN: u32 = 0x5f9a44; // oasis reed stalk

// Ground cover.
const PEBBLE: u32 = 0xbfae84; // desert pebble
const PEBBLE_DARK: u32 = 0xa3926a; // shadowed pebble
const DRY_GRASS: u32 = 0xb9a866; // sparse dry grass blade
const DRY_GRASS_TIP: u32 = 0xd6c684; // pale dry grass tip
const SUCC_GREEN: u32 = 0x6fa84e; // tiny succulent rosette
const SUCC_FLOWER: u32 = 0xe88ad0; // tiny succulent bloom
const DESERT_BLOOM_ORANGE: u32 = 0xe6862a; // brittlebush / desert-marigold orange
const DESERT_STEM: u32 = 0x7a8a3a; // dry grey-green bloom stem

/// Saguaros are authored ~1.35u tall; the scatter scales them up so they tower.
const CACTUS_SCALE: f32 = 1.6;

// ── Mesh helpers (verified 0.18 API; same recipe as trees.rs / decor.rs) ──────────

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// Tag every vertex of `m` with one flat linear colour (REQUIRED before merge — every
/// merged part must carry the same attribute set, incl. `ATTRIBUTE_COLOR`).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge several pre-`tinted` parts into ONE mesh so identical props batch into one
/// draw call. `Mesh::merge` returns `Result` in 0.18 — `.expect` on a mismatch.
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("desert parts share attributes");
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

/// An upright cylinder whose CENTRE sits at `cy` (a part of height `h` rooted at y=0
/// uses `cy = h/2`). `res` ≥ 3.
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), lin(c))
}

/// An upright cylinder centred at an arbitrary `center` (not just on the Y axis).
fn cyl_up_at(r: f32, h: f32, center: Vec3, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(6).build().translated_by(center), lin(c))
}

/// A flattened ellipsoid "paddle" (a prickly-pear pad): an ico-sphere stretched wide on
/// X / tall on Y / thin on Z, tilted about Z then yawed about Y, centred at `center`.
/// ico detail 1 so the silhouette reads as a smooth oval pad once flat-shaded.
fn pad(rx: f32, ry: f32, rz: f32, tilt: f32, yaw: f32, center: Vec3, c: u32) -> Mesh {
    tinted(
        Sphere::new(1.0)
            .mesh()
            .ico(1)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(rx, ry, rz))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(center),
        lin(c),
    )
}

// ─── Saguaro cactus (the TREE class) ──────────────────────────────────────────────

/// **Saguaro** — a tall ribbed green column with 1–2 upturned arms and a red crown
/// bloom, plus a few pale spine flecks. Built from low-resolution (6-sided) cylinders
/// so the column reads ribbed, with a darker shaded core and a brighter front rib for
/// volume, and a rounded green cap on top. Base flush at y=0; ~1.35u tall before the
/// scatter scales it. `variant` 0 = single arm, 1 = two arms (classic), 2 = young stub.
/// Pub so the standalone model viewer (`FOREST_VIEW=trees`) can show it as a choppable resource.
pub fn build_saguaro_mesh(variant: u32) -> Mesh {
    let v = variant % 3;
    let (trunk_h, trunk_r) = match v {
        0 => (1.30, 0.13),
        1 => (1.35, 0.14),
        _ => (0.95, 0.16), // young: shorter, fatter, no arms
    };

    let mut parts: Vec<Mesh> = Vec::new();

    // Trunk: a slightly tapering two-segment column (fatter foot) with TRUE rib ridges —
    // six thin vertical green ridges standing proud of the surface, alternating shadowed
    // and sunlit, so the column reads pleated from every side (the old build faked it
    // with two offset cylinders that only read from one angle).
    parts.push(cyl_up(trunk_r * 1.05, trunk_h * 0.45, trunk_h * 0.225, 7, CACTUS_MID));
    parts.push(cyl_up(trunk_r * 0.92, trunk_h * 0.60, trunk_h * 0.72, 7, CACTUS_MID));
    for i in 0..6 {
        let a = (i as f32 / 6.0) * TAU + 0.26;
        let c = if i % 2 == 0 { CACTUS_LIGHT } else { CACTUS_DARK };
        parts.push(tinted(
            Cuboid::new(0.030, trunk_h * 0.92, 0.030)
                .mesh()
                .build()
                .translated_by(Vec3::new(trunk_r * 0.92, trunk_h * 0.47, 0.0))
                .rotated_by(Quat::from_rotation_y(a)),
            lin(c),
        ));
    }
    // Rounded green cap so the trunk top isn't a flat disc.
    parts.push(ball_at(trunk_r * 0.92, y(trunk_h), 0.85, CACTUS_MID));

    // ── Arms: an upturned arm = a round elbow joint + an outward stub + an upright
    // forearm with its own short rib ridges + a rounded lit tip.
    let add_arm = |parts: &mut Vec<Mesh>, side: f32, attach_y: f32, arm_h: f32, arm_r: f32| {
        let elbow_len = 0.28;
        let elbow_x = side * (trunk_r + elbow_len * 0.5);
        // Ball joint where the elbow meets the trunk (hides the seam).
        parts.push(ball_at(arm_r * 1.1, Vec3::new(side * trunk_r * 0.9, attach_y, 0.0), 0.95, CACTUS_MID));
        // Elbow reaching outward (built upright, laid toward ±X by a 90° Z rotation).
        parts.push(tinted(
            Cylinder::new(arm_r, elbow_len)
                .mesh()
                .resolution(6)
                .build()
                .rotated_by(Quat::from_rotation_z(FRAC_PI_2))
                .translated_by(Vec3::new(elbow_x, attach_y, 0.0)),
            lin(CACTUS_DARK),
        ));
        let fore_x = side * (trunk_r + elbow_len);
        // Round outer elbow + upright forearm rising from it.
        parts.push(ball_at(arm_r * 1.05, Vec3::new(fore_x, attach_y, 0.0), 0.95, CACTUS_MID));
        parts.push(cyl_up_at(arm_r, arm_h, Vec3::new(fore_x, attach_y + arm_h * 0.5, 0.0), CACTUS_MID));
        // Two short rib ridges on the forearm (in + out faces).
        for s in [-1.0_f32, 1.0] {
            parts.push(tinted(
                Cuboid::new(0.022, arm_h * 0.85, 0.022)
                    .mesh()
                    .build()
                    .translated_by(Vec3::new(fore_x + s * arm_r * 0.9, attach_y + arm_h * 0.5, 0.02)),
                lin(if s > 0.0 { CACTUS_LIGHT } else { CACTUS_DARK }),
            ));
        }
        // Rounded lit tip cap + a bloom crowning the arm.
        parts.push(ball_at(arm_r * 0.95, Vec3::new(fore_x, attach_y + arm_h, 0.0), 0.85, CACTUS_LIGHT));
        parts.push(ball_at(arm_r * 0.42, Vec3::new(fore_x, attach_y + arm_h + 0.05, 0.0), 0.6, FLOWER_PINK));
    };

    match v {
        0 => add_arm(&mut parts, 1.0, trunk_h * 0.55, 0.55, 0.10),
        1 => {
            add_arm(&mut parts, 1.0, trunk_h * 0.58, 0.55, 0.10);
            add_arm(&mut parts, -1.0, trunk_h * 0.42, 0.42, 0.09);
        }
        _ => {}
    }

    // ── Red crown bloom on top (a flattened red ball + a tiny yellow core).
    parts.push(ball_at(trunk_r * 0.5, y(trunk_h + 0.06), 0.55, FLOWER_RED));
    parts.push(ball_at(trunk_r * 0.26, y(trunk_h + 0.12), 0.6, FLOWER_YELLOW));

    // ── Pale spine flecks spiralling up the trunk (tiny boxes) for texture.
    for i in 0..7 {
        let t = (i as f32 + 0.5) / 7.0;
        let a = i as f32 * 2.399_963_2; // golden angle spread
        parts.push(tinted(
            Cuboid::new(0.018, 0.018, 0.018)
                .mesh()
                .build()
                .translated_by(Vec3::new(a.cos() * trunk_r * 0.98, trunk_h * (0.15 + t * 0.7), a.sin() * trunk_r * 0.98)),
            lin(CACTUS_SPINE),
        ));
    }

    flat_shaded(merged(parts))
}

// ─── Barrel cactus (a non-tree prop) ──────────────────────────────────────────────

/// **Barrel cactus** — a squat ribbed green dome with a yellow/pink crown flower ring.
/// A wide low 8-sided cylinder (ribbed) + a domed lit top + a dark shaded skirt + pale
/// spine flecks + a ring of bloom balls. Base at y=0, ~0.4u tall. `variant` picks bloom.
fn build_barrel_mesh(variant: u32) -> Mesh {
    let r = 0.30;
    let h = 0.34;
    let bloom = if variant % 2 == 0 { FLOWER_YELLOW } else { FLOWER_PINK };

    let mut parts = vec![
        cyl_up(r, h, h * 0.5, 8, CACTUS_MID),
        ball_at(r * 0.98, y(h), 0.5, CACTUS_LIGHT),  // domed lit top
        ball_at(r * 0.9, y(h * 0.28), 0.5, CACTUS_DARK), // dark shaded skirt
    ];

    // Eight TRUE vertical rib ridges standing proud of the barrel, alternating lit and
    // shadowed, so the pleats read from every angle.
    for i in 0..8 {
        let a = (i as f32 / 8.0) * TAU + 0.2;
        let c = if i % 2 == 0 { CACTUS_LIGHT } else { CACTUS_DARK };
        parts.push(tinted(
            Cuboid::new(0.026, h * 0.88, 0.026)
                .mesh()
                .build()
                .translated_by(Vec3::new(r * 0.95, h * 0.48, 0.0))
                .rotated_by(Quat::from_rotation_y(a)),
            lin(c),
        ));
    }

    // Pale spine flecks around the upper ribs.
    for i in 0..8 {
        let a = (i as f32 / 8.0) * TAU;
        parts.push(tinted(
            Cuboid::new(0.018, 0.05, 0.018)
                .mesh()
                .build()
                .rotated_by(Quat::from_rotation_y(a))
                .translated_by(Vec3::new(a.cos() * r * 0.98, h * 0.62, a.sin() * r * 0.98)),
            lin(CACTUS_SPINE),
        ));
    }

    // Crown bloom — a centre + a ring of small bloom balls.
    parts.push(ball_at(0.05, y(h + 0.05), 0.6, bloom));
    for i in 0..5 {
        let a = (i as f32 / 5.0) * TAU;
        parts.push(ball_at(0.045, Vec3::new(a.cos() * 0.1, h + 0.04, a.sin() * 0.1), 0.55, bloom));
    }

    flat_shaded(merged(parts))
}

// ─── Dead bush / tumbleweed (the FIRST non-tree class — also the tree fallback) ──

/// **Dead bush / tumbleweed** — a tan twiggy blob: a low ball of crossed thin twigs
/// over a darker base lump, dry and airy. Base at y=0, ~0.35u across. `variant` 0 =
/// sprawly bush, 1 = rounder tumbleweed.
fn build_dead_bush_mesh(variant: u32) -> Mesh {
    let mut parts: Vec<Mesh> = vec![ball_at(0.10, y(0.07), 0.6, TWIG_DARK)];

    let n = if variant % 2 == 0 { 9 } else { 12 };
    let spread = if variant % 2 == 0 { 0.34 } else { 0.28 };
    for i in 0..n {
        let a = (i as f32 / n as f32) * TAU + (i % 3) as f32 * 0.4;
        let tilt = 0.5 + (i % 4) as f32 * 0.18;
        let len = spread * (0.7 + (i % 3) as f32 * 0.22);
        let c = if i % 3 == 0 { TWIG_DARK } else { TWIG_TAN };
        // Thin twig: build upright, lean out (Z) then yaw around the blob (Y).
        let twig = Cylinder::new(0.012, len)
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(y(0.05));
        parts.push(tinted(twig, lin(c)));
    }
    flat_shaded(merged(parts))
}

// ─── Prickly-pear / opuntia cactus (a non-tree class) ────────────────────────────

/// **Prickly-pear (opuntia)** — a clump of flat green paddle pads sprouting off one
/// another, studded with pale spine flecks and topped with a few red "tuna" fruit and a
/// yellow bloom. A wholly different cactus silhouette from the columnar saguaro. Base at
/// y=0, ~0.7u tall before the scatter scales it. `variant` varies the pad count / lean.
fn build_prickly_pear_mesh(variant: u32) -> Mesh {
    let flip = if variant % 2 == 0 { 1.0_f32 } else { -1.0 };
    let mut parts: Vec<Mesh> = Vec::new();

    // Base pad — a broad upright paddle resting on the ground.
    parts.push(pad(0.24, 0.30, 0.07, 0.0, 0.25, y(0.29), CACTUS_MID));
    // Two pads growing off its upper shoulders, tilted out and yawed apart.
    parts.push(pad(0.20, 0.26, 0.06, flip * 0.55, 0.5, Vec3::new(-flip * 0.20, 0.50, 0.05), CACTUS_DARK));
    parts.push(pad(0.19, 0.25, 0.06, -flip * 0.50, -0.4, Vec3::new(flip * 0.21, 0.54, -0.04), CACTUS_LIGHT));
    // A smaller crowning pad (variant 0 only) for a fuller clump.
    if variant % 2 == 0 {
        parts.push(pad(0.14, 0.18, 0.05, 0.2, 0.8, Vec3::new(0.02, 0.74, 0.02), CACTUS_MID));
    }

    // Pale spine flecks dotted over the front faces of the pads (tiny boxes on a spiral).
    let pad_faces = [
        (Vec3::new(0.0, 0.29, 0.0), 0.20_f32, 0.27_f32),
        (Vec3::new(-flip * 0.20, 0.50, 0.05), 0.16, 0.22),
        (Vec3::new(flip * 0.21, 0.54, -0.04), 0.15, 0.21),
    ];
    for (centre, sx, sy) in pad_faces {
        for i in 0..5 {
            let a = i as f32 * 2.399_963_2; // golden angle
            let t = (i as f32 + 0.5) / 5.0;
            let off = Vec3::new(a.cos() * sx * (1.0 - t) * 0.8, (t - 0.5) * sy * 1.4, 0.075);
            parts.push(tinted(
                Cuboid::new(0.016, 0.016, 0.016).mesh().build().translated_by(centre + off),
                lin(CACTUS_SPINE),
            ));
        }
    }

    // A few red "tuna" fruit perched on the top rims + one yellow bloom.
    parts.push(ball_at(0.05, Vec3::new(-flip * 0.20, 0.74, 0.04), 0.7, FLOWER_RED));
    parts.push(ball_at(0.045, Vec3::new(flip * 0.21, 0.78, -0.04), 0.7, FLOWER_RED));
    parts.push(ball_at(0.038, Vec3::new(0.0, 0.84, 0.02), 0.7, FLOWER_YELLOW));

    flat_shaded(merged(parts))
}

// ─── Sun-bleached rocks + the odd bone/skull (a non-tree class) ──────────────────

/// **Bleached rock / bone** — `variant` 0/1 are pale faceted boulders (a body lump +
/// a bright top facet + a shaded side cobble); `variant` 2 is a bleached bone/skull on
/// the sand (an ivory cranium with two dark sockets + a snout + a couple of rib bones).
/// Base flush at y=0.
fn build_bleached_mesh(variant: u32) -> Mesh {
    match variant % 3 {
        // Squat bleached boulder.
        0 => {
            let r = 0.30;
            flat_shaded(merged(vec![
                ball_at(r, y(r * 0.7), 0.78, ROCK_BODY),
                ball_at(r * 0.5, y(r * 1.16), 0.7, ROCK_TOP),
                ball_at(r * 0.55, Vec3::new(r * 0.9, r * 0.38, r * 0.15), 0.82, ROCK_SHADE),
                ball_at(r * 0.44, Vec3::new(-r * 0.8, r * 0.34, -r * 0.3), 0.82, ROCK_BODY),
            ]))
        }
        // Taller split crag.
        1 => {
            let r = 0.26;
            flat_shaded(merged(vec![
                ball_at(r, y(r * 0.82), 0.92, ROCK_BODY),
                ball_at(r * 0.7, Vec3::new(r * 0.32, r * 1.5, -r * 0.1), 0.95, ROCK_SHADE),
                ball_at(r * 0.4, Vec3::new(-r * 0.5, r * 0.7, r * 0.3), 0.85, ROCK_BODY),
                ball_at(r * 0.42, Vec3::new(r * 0.3, r * 2.0, -r * 0.05), 0.8, ROCK_TOP),
            ]))
        }
        // Bleached skull + scattered rib bones lying on the sand.
        _ => {
            let mut parts = vec![
                ball_at(0.16, y(0.11), 0.85, BONE),                    // cranium dome
                ball_at(0.10, Vec3::new(0.0, 0.07, 0.16), 0.8, BONE_SHADE), // snout/jaw
                ball_at(0.035, Vec3::new(0.06, 0.13, 0.12), 0.7, SOCKET),   // eye sockets
                ball_at(0.035, Vec3::new(-0.06, 0.13, 0.12), 0.7, SOCKET),
            ];
            // A couple of rib bones lying flat (thin cylinders laid on the ground).
            for (k, &(rx, rz, ry)) in
                [(0.26_f32, 0.05_f32, 0.5_f32), (0.30, -0.12, -0.7)].iter().enumerate()
            {
                let len = 0.30 - k as f32 * 0.04;
                parts.push(tinted(
                    Cylinder::new(0.022, len)
                        .mesh()
                        .resolution(5)
                        .build()
                        .rotated_by(Quat::from_rotation_z(FRAC_PI_2))
                        .rotated_by(Quat::from_rotation_y(ry))
                        .translated_by(Vec3::new(rx, 0.022, rz)),
                    lin(BONE),
                ));
            }
            flat_shaded(merged(parts))
        }
    }
}

// ─── Ground cover ─────────────────────────────────────────────────────────────────

/// **Desert pebble** — a tiny cluster of 2–3 flat faceted stones. Base at y=0.
fn build_pebble_mesh(variant: u32) -> Mesh {
    let parts = if variant % 2 == 0 {
        vec![
            ball_at(0.06, y(0.025), 0.45, PEBBLE),
            ball_at(0.04, Vec3::new(0.08, 0.018, 0.03), 0.45, PEBBLE_DARK),
        ]
    } else {
        vec![
            ball_at(0.05, y(0.022), 0.4, PEBBLE_DARK),
            ball_at(0.045, Vec3::new(-0.06, 0.02, 0.04), 0.45, PEBBLE),
            ball_at(0.035, Vec3::new(0.05, 0.016, -0.05), 0.45, PEBBLE),
        ]
    };
    flat_shaded(merged(parts))
}

/// **Dry grass tuft** — a sparse fan of 4 thin pale-tan cone blades, drier + shorter
/// than the forest grass. Base at y=0, ~0.22u tall.
fn build_dry_grass_mesh() -> Mesh {
    let specs = [
        (0.0_f32, 0.00_f32, 0.22_f32, DRY_GRASS),
        (0.7, 0.24, 0.18, DRY_GRASS_TIP),
        (-0.6, -0.22, 0.17, DRY_GRASS_TIP),
        (2.0, 0.16, 0.15, DRY_GRASS),
    ];
    let parts = specs
        .iter()
        .map(|&(yaw, tilt, h, c)| {
            // 4-sided blades: the crisp facet IS the look, and dry grass is scattered
            // by the thousand (the default 32-segment cone was ~8× the vertices).
            let mut m = Cone { radius: 0.018, height: h }
                .mesh()
                .resolution(4)
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

/// **Tiny succulent** — a low rosette of small green leaf balls with a tiny pink bloom.
/// Base at y=0, ~0.1u tall.
fn build_succulent_mesh() -> Mesh {
    let mut parts = vec![ball_at(0.045, y(0.03), 0.5, SUCC_GREEN)];
    for i in 0..5 {
        let a = (i as f32 / 5.0) * TAU;
        parts.push(ball_at(0.03, Vec3::new(a.cos() * 0.05, 0.028, a.sin() * 0.05), 0.4, SUCC_GREEN));
    }
    parts.push(ball_at(0.018, y(0.07), 0.7, SUCC_FLOWER)); // tiny bloom on top
    flat_shaded(merged(parts))
}

/// **Desert ground litter** (cover) — the little splashes of life on the sand. `variant`:
/// 0 = a brittlebush cluster of small orange blooms on dry stems, 1 = a single yellow
/// desert poppy, 2 = a couple of bleached crossed twigs. Very low (≤0.12u), base at y=0.
fn build_desert_litter_mesh(variant: u32) -> Mesh {
    match variant % 3 {
        // Brittlebush — a small cluster of orange blooms on short grey-green stems.
        0 => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..4 {
                let a = (i as f32 / 4.0) * TAU;
                let (bx, bz) = (a.cos() * 0.05, a.sin() * 0.05);
                let h = 0.08 + (i % 2) as f32 * 0.03;
                parts.push(cyl_up_at(0.008, h, Vec3::new(bx, h * 0.5, bz), DESERT_STEM));
                parts.push(ball_at(0.028, Vec3::new(bx, h, bz), 0.6, DESERT_BLOOM_ORANGE));
                parts.push(ball_at(0.012, Vec3::new(bx, h + 0.012, bz), 0.7, FLOWER_YELLOW));
            }
            flat_shaded(merged(parts))
        }
        // Yellow desert poppy — a single small flower: yellow petal ring + a red centre.
        1 => {
            let head_y = 0.10;
            let mut parts = vec![
                tinted(
                    Cone { radius: 0.008, height: head_y }
                        .mesh()
                        .resolution(5)
                        .build()
                        .translated_by(y(head_y * 0.5)),
                    lin(DESERT_STEM),
                ),
                ball_at(0.018, y(head_y), 0.7, FLOWER_RED),
            ];
            for i in 0..6 {
                let a = (i as f32 / 6.0) * TAU;
                parts.push(ball_at(0.024, Vec3::new(a.cos() * 0.035, head_y, a.sin() * 0.035), 0.45, FLOWER_YELLOW));
            }
            flat_shaded(merged(parts))
        }
        // Bleached crossed twigs lying on the sand.
        _ => {
            let twig = |len: f32, rot_y: f32, c: u32| -> Mesh {
                tinted(
                    Cylinder::new(0.01, len)
                        .mesh()
                        .resolution(4)
                        .build()
                        .rotated_by(Quat::from_rotation_z(FRAC_PI_2))
                        .rotated_by(Quat::from_rotation_y(rot_y))
                        .translated_by(y(0.012)),
                    lin(c),
                )
            };
            flat_shaded(merged(vec![
                twig(0.14, 0.3, TWIG_TAN),
                twig(0.11, 1.4, TWIG_DARK),
                ball_at(0.02, y(0.015), 0.5, TWIG_DARK),
            ]))
        }
    }
}

// ── Config ────────────────────────────────────────────────────────────────────────

pub fn config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Desert,
        name: "Desert",

        ground_color: SAND_GROUND,
        ground_roughness: 0.96,
        detail: GroundDetail {
            scale: 0.20,
            strength: 0.32,
            variation: 0.45,
            seed: 4.0,
            dark: SAND_DARK,
            base: SAND_GROUND,
            light: SAND_LIGHT,
            grain: 0.40,
            // Strong streak → wind ripples across the sand.
            streak: 0.85,
        },

        // Blazing hot hazy desert: hot pale-amber sky, fierce sun, warm amber ambient.
        sky: 0xf2e0a8,
        fog_density: 0.013, // shimmering heat haze
        sun_color: 0xfff0c4, // hot white-gold
        sun_illuminance: 15_500.0,
        ambient_color: 0xffeec2,
        ambient_brightness: 136.0,
        sun_pos: Vec3::new(20.0, 38.0, 6.0),

        seed: 4044,
        tree_min_dist: 3.4,
        classes: vec![
            // First non-tree class = the tumbleweed/dead-bush (also the tree fallback).
            PropClass {
                variants: (0..2).map(|v| (build_dead_bush_mesh(v), 1.0)).collect(),
                chance: 0.045,
                scale: (0.7, 1.25),
                tree: false,
                block_radius: 0.0,
            },
            // Saguaro cactus — the tree class (spacing-checked, gets wind sway).
            PropClass {
                variants: vec![
                    (build_saguaro_mesh(1), 0.5), // classic two-arm
                    (build_saguaro_mesh(0), 0.3), // single arm
                    (build_saguaro_mesh(2), 0.2), // stubby young
                ],
                chance: 0.035,
                scale: (0.8 * CACTUS_SCALE, 1.15 * CACTUS_SCALE),
                tree: true,
                block_radius: 0.0,
            },
            // Barrel cactus — squat ribbed dome with a crown bloom.
            PropClass {
                variants: (0..2).map(|v| (build_barrel_mesh(v), 1.0)).collect(),
                chance: 0.03,
                scale: (0.8, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            // Prickly-pear / opuntia — clumps of flat green pads with red fruit.
            PropClass {
                variants: (0..2).map(|v| (build_prickly_pear_mesh(v), 1.0)).collect(),
                chance: 0.018,
                scale: (0.8, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            // Bleached rocks + the odd bone/skull.
            PropClass {
                variants: vec![
                    (build_bleached_mesh(0), 1.0),
                    (build_bleached_mesh(1), 1.0),
                    (build_bleached_mesh(2), 0.6),
                ],
                chance: 0.035,
                scale: (0.6, 1.4),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover: vec![
            PropClass {
                variants: (0..2).map(|v| (build_pebble_mesh(v), 1.0)).collect(),
                chance: 0.30,
                scale: (0.7, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_dry_grass_mesh(), 1.0)],
                chance: 0.12,
                scale: (0.6, 1.1),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_succulent_mesh(), 1.0)],
                chance: 0.05,
                scale: (0.7, 1.2),
                tree: false,
                block_radius: 0.0,
            },
            // Desert litter — brittlebush blooms, desert poppies, bleached twigs.
            PropClass {
                variants: (0..3).map(|v| (build_desert_litter_mesh(v), 1.0)).collect(),
                chance: 0.10,
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover_per_tile: 2,

        river: false,
        river_color: OASIS_WATER,
        backdrop: Backdrop {
            land_dir: -FRAC_PI_2,
            land_arc: FRAC_PI_2,
            // No ocean — desert reaching to the hazy horizon.
            ocean: false,
            ocean_color: 0x2f6fae,
            // Low rolling dunes in sandy tones.
            hill_body: 0xd6c08a,
            hill_cap: 0xeed9a8,
            hill_foot: 0xbfa672,
            treeline: false,
            treeline_dark: 0x7a6b3a,
            treeline_mid: 0x8c7c45,
            // Short dunes (not tall mountains).
            hill_h: (16.0, 34.0),
        },
        particle: ParticleKind::Dust,
    }
}

// ── Landmark: a small OASIS — palms + reeds around a still blue-green pool ──────────

/// 2–3 palms + reeds around a still blue-green pool, on the land side (`z < 0`). Every
/// spawn is tagged [`BiomeEntity`] so a biome switch wipes it.
pub fn landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Shared white vertex-colour material for the props (same recipe as the scatter).
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    // Still-water material — low roughness so it reflects the sky via IBL, tinted
    // blue-green. Spawned just above y=0 so the opaque ground plane doesn't hide it.
    let water_mat = materials.add(StandardMaterial {
        base_color: crate::palette::srgb(OASIS_WATER),
        perceptual_roughness: 0.08,
        metallic: 0.0,
        reflectance: 0.5,
        ..default()
    });

    // Oasis centre on the land side, off to one side of the framing.
    let cx = -7.0_f32;
    let cz = -9.0_f32;
    let pool_r = 3.2_f32;

    // ── Still pool — a low-roughness disc just above the ground. The `Circle` mesh
    // lies in the XY plane (normal +Z), so rotate −90° about X to lie flat on XZ.
    commands.spawn((
        Mesh3d(meshes.add(
            Circle::new(pool_r)
                .mesh()
                .resolution(40)
                .build()
                .rotated_by(Quat::from_rotation_x(-FRAC_PI_2)),
        )),
        MeshMaterial3d(water_mat),
        Transform::from_xyz(cx, 0.04, cz),
        BiomeEntity,
    ));

    // A damp dark-sand rim ring just outside the water (a thin flat tinted disc).
    commands.spawn((
        Mesh3d(meshes.add(tinted_circle(pool_r * 1.22, 40, SAND_DARK))),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(cx, 0.02, cz),
        bevy::light::NotShadowCaster,
        BiomeEntity,
    ));

    // ── Palms around the pool — 3 at angles round the rim, leaning outward.
    let palm = meshes.add(build_palm_mesh());
    for (i, &a) in [0.6_f32, 2.4, 4.3].iter().enumerate() {
        let pr = pool_r + 0.6;
        let px = cx + a.cos() * pr;
        let pz = cz + a.sin() * pr;
        let lean = 0.10 + (i % 2) as f32 * 0.06;
        let s = 1.0 + (i % 3) as f32 * 0.12;
        commands.spawn((
            Mesh3d(palm.clone()),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: Vec3::new(px, 0.0, pz),
                rotation: Quat::from_rotation_y(a + FRAC_PI_2) * Quat::from_rotation_z(lean),
                scale: Vec3::splat(s),
            },
            BiomeEntity,
        ));
    }

    // ── Reeds clustered at the water's edge.
    let reed = meshes.add(build_reed_clump_mesh());
    for &a in [1.4_f32, 3.3, 5.1, 5.6].iter() {
        let rr = pool_r + 0.15;
        let rx = cx + a.cos() * rr;
        let rz = cz + a.sin() * rr;
        commands.spawn((
            Mesh3d(reed.clone()),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: Vec3::new(rx, 0.0, rz),
                rotation: Quat::from_rotation_y(a),
                scale: Vec3::splat(0.9 + (a * 7.0).sin().abs() * 0.3),
            },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
}

/// A flat tinted disc lying on the XZ plane (for the oasis damp-sand rim).
fn tinted_circle(r: f32, res: u32, c: u32) -> Mesh {
    tinted(
        Circle::new(r).mesh().resolution(res).build().rotated_by(Quat::from_rotation_x(-FRAC_PI_2)),
        lin(c),
    )
}

/// **Palm tree** — a tall ringed trunk (a stack of tapering cylinders banded with
/// darker rings) bending slightly, topped with a crown of drooping fronds (leaned-out
/// flattened boxes) + a small date cluster. Base at y=0, ~2.2u tall.
fn build_palm_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    let trunk_h = 2.2_f32;
    let seg = 8u32;
    let seg_h = trunk_h / seg as f32;
    let mut top_x = 0.0_f32;
    for i in 0..seg {
        let t = i as f32 / seg as f32;
        let r = 0.13 - t * 0.045; // taper
        let bend = t * t * 0.5; // upper trunk leans in +X
        let cy = seg_h * (i as f32 + 0.5);
        parts.push(cyl_up_at(r, seg_h * 1.02, Vec3::new(bend, cy, 0.0), PALM_TRUNK));
        // Darker ring band between segments (a thin slightly-wider disc).
        parts.push(tinted(
            Cylinder::new(r * 1.12, seg_h * 0.18)
                .mesh()
                .resolution(8)
                .build()
                .translated_by(Vec3::new(bend, seg_h * (i as f32 + 1.0), 0.0)),
            lin(PALM_TRUNK_RING),
        ));
        top_x = bend;
    }
    let top = Vec3::new(top_x, trunk_h, 0.0);

    // ── Crown: two tiers of drooping fronds radiating out + down from the trunk top.
    // Each frond is a squashed 4-sided cone — broad at the crown, tapering to a hanging
    // tip — so the leaf reads pointed and ribbed, not a stiff rectangle.
    let frond = |len: f32, a: f32, droop: f32, c: u32| -> Mesh {
        let m = Cone { radius: 0.10, height: len }
            .mesh()
            .resolution(4)
            .build()
            .scaled_by(Vec3::new(1.8, 1.0, 0.35))
            .translated_by(y(len * 0.5))
            .rotated_by(Quat::from_rotation_x(FRAC_PI_2 + droop))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(top);
        tinted(m, lin(c))
    };
    // Lower tier: eight long fronds drooping well past horizontal.
    for i in 0..8 {
        let a = (i as f32 / 8.0) * TAU;
        let droop = if i % 2 == 0 { 0.60 } else { 0.80 };
        let c = if i % 2 == 0 { FROND_LIGHT } else { FROND_DARK };
        parts.push(frond(1.15, a, droop, c));
    }
    // Upper tier: five shorter fronds lifted nearer the vertical, offset between the
    // lower tier's gaps, crowning the head.
    for i in 0..5 {
        let a = (i as f32 / 5.0) * TAU + 0.4;
        parts.push(frond(0.75, a, 0.25, FROND_LIGHT));
    }
    // Crown core lump hiding the frond roots.
    parts.push(ball_at(0.14, top, 0.8, FROND_DARK));
    // A small cluster of dates under the crown.
    for i in 0..4 {
        let a = (i as f32 / 4.0) * TAU;
        parts.push(ball_at(0.035, top + Vec3::new(a.cos() * 0.1, -0.12, a.sin() * 0.1), 0.9, DATE));
    }

    flat_shaded(merged(parts))
}

/// **Oasis reed clump** — a fan of tall slender green cone stalks for the water's edge.
/// Base at y=0, ~0.8u tall.
fn build_reed_clump_mesh() -> Mesh {
    let count = 7;
    let mut parts: Vec<Mesh> = Vec::new();
    for i in 0..count {
        let a = (i as f32 / count as f32) * TAU;
        let foot = 0.10;
        let bx = a.cos() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let bz = a.sin() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let h = 0.6 + ((i * 7) % 5) as f32 * 0.08;
        let tilt = 0.06 + (i % 4) as f32 * 0.05;
        let stalk = Cone { radius: 0.022, height: h }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(h / 2.0))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(Vec3::new(bx, 0.0, bz));
        parts.push(tinted(stalk, lin(REED_GREEN)));
    }
    flat_shaded(merged(parts))
}
