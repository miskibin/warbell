//! Snow biome (key 2) — a crisp winter scene mirroring the Forest module's structure.
//!
//! All snow props are built **locally** in this file (self-contained — touches no other
//! module): snow-laden conifer pines (brown stub trunk + stacked dark-green cone tiers,
//! each boughed with a smaller WHITE snow cone), bare snowy birch (white trunk, a few
//! bare twigs, a dab of snow), low snow-dusted shrubs / mounds (the first non-tree
//! fallback class), and frost boulders (blue-grey rock with a white snow cap). Ground
//! cover is tiny snow tufts + ice glints. Particle: drifting snow. Backdrop: tall
//! white-capped peaks over a dark conifer treeline, land on one side, no ocean.
//!
//! Landmark: a frozen pond — a low-roughness pale-blue ice disc sitting just above y=0
//! (reflects the sky via IBL) ringed by a couple of snow-laden dead trees + a small
//! rock cairn.
//!
//! CONTRACT (mirrors trees.rs / props.rs / decor.rs): every prop is ONE merged,
//! vertex-coloured `Mesh` with its base at y=0, tinted into `ATTRIBUTE_COLOR` (the
//! scatter draws them against one shared white material), then flat-shaded for crisp
//! low-poly facets. Two public fns with the exact framework signatures.

// The `landmarks()` FROZEN POND set-piece + its helpers/consts below are authored biome content
// the world map doesn't place yet (it uses `ruins` landmarks instead). Kept per design; allow
// the resulting dead code until it's wired into a per-region pass.
#![allow(dead_code)]

use bevy::prelude::*;

use crate::biome::{Backdrop, Biome, BiomeConfig, BiomeEntity, GroundDetail, ParticleKind, PropClass};

// ── Snow palette (hex lifted from the TS game's snow biome) ─────────────────────
// Ground: snow `#eef3f8` / `#eaf1f7`, fog `#cdd8e8` — kept blue-grey, NOT pure white.
const SNOW_GROUND: u32 = 0xdfe8f2; // cold blue-white ground base (snow with blue shadow)
const SNOW_GROUND_DARK: u32 = 0xc2d0e0; // shadowed snow trough
const SNOW_GROUND_LIGHT: u32 = 0xf4f8fc; // sunlit drift crest

// Conifer foliage: TS snowpine dark `#35614a`, mid `#427a5a`. Deep saturated winter green.
const PINE_DARK: u32 = 0x2c5240; // shadowed lower boughs
const PINE_MID: u32 = 0x35614a; // body tier
const PINE_LIGHT: u32 = 0x427a5a; // sunlit upper tier
const PINE_TRUNK: u32 = 0x4a3526; // brown stub trunk

// Snow that sits ON the props (boughs / caps / dustings). Slightly blue so it reads as
// snow-in-shade against the bright white ground, with a brighter highlight cap.
const SNOW_CAP: u32 = 0xeaf2fb; // snow on boughs / mounds
const SNOW_CAP_HI: u32 = 0xfbfdff; // bright sunlit snow highlight
const SNOW_SHADE: u32 = 0xc9d8ea; // bluish snow underside / shadow

// Birch: pale trunk (snow trunk family) + dark bark marks + bare grey-brown twigs.
const BIRCH_TRUNK: u32 = 0xe6ebef; // pale birch bark
const BIRCH_MARK: u32 = 0x55524c; // dark bark scar
const BIRCH_TWIG: u32 = 0x7a6f63; // bare grey-brown twig

// Frost boulders: blue-grey rock (snow chest dark `#8b97a3` family) + a snow cap.
const ROCK_BODY: u32 = 0x7e8b99; // blue-grey frost rock
const ROCK_DARK: u32 = 0x66727f; // shadowed rock base
const ROCK_LIGHT: u32 = 0x97a3b0; // lit rock facet

// Snowman (bałwan) — bright stacked snow body + coal face/buttons, carrot nose, twig
// arms (reuse BIRCH_TWIG), a red knitted scarf and a dark bucket hat.
const SNOWMAN_BODY: u32 = 0xfbfdff; // bright packed-snow body (= SNOW_CAP_HI, kept explicit)
const COAL: u32 = 0x1d1d22; // coal eyes / mouth / buttons
const CARROT: u32 = 0xe8721f; // carrot nose
const SCARF: u32 = 0xc0392b; // red knitted scarf
const HAT: u32 = 0x2a2a30; // dark bucket hat

// Winter ground litter — holly (dark leaves + red berries), frosted pinecone, ice shard.
const HOLLY_LEAF: u32 = 0x2f6b3a; // dark holly-leaf green
const HOLLY_BERRY: u32 = 0xcc2a2a; // bright red holly berry
const PINECONE_BROWN: u32 = 0x6e5038; // brown pinecone scales

// Frozen-pond ice (FrozenSpire family): pale crystal blue.
const ICE_PALE: u32 = 0xbfe0f4; // pale ice surface tint
const ICE_RIM: u32 = 0x9cc3e0; // darker frosted rim
const CAIRN_STONE: u32 = 0x8893a0; // pond-side cairn stone

// Authoring → world scale: trees are built ~1.4u tall; scale up so they tower.
const TREE_SCALE: f32 = 1.7;

const FRAC_PI_2: f32 = std::f32::consts::FRAC_PI_2;

// ── Mesh helpers (identical recipe to trees.rs / props.rs / decor.rs) ───────────

fn lin(c: u32) -> [f32; 4] {
    crate::palette::lin(c)
}

/// Tag every vertex of `m` with a flat linear colour (REQUIRED before merge — all parts
/// must carry the same attribute set incl. `ATTRIBUTE_COLOR`).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge a non-empty list of pre-`tinted` parts into ONE mesh (renderer batches them).
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut b = it.next().expect("part");
    for p in it {
        b.merge(&p).expect("attrs");
    }
    b
}

/// Un-index + recompute per-face normals → crisp flat-shaded facets. MUST be called LAST
/// on the merged mesh (`compute_flat_normals` panics on an indexed mesh).
fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// An upright cylinder rooted at y=0 (a part of height `h` uses centre `cy = h/2`).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), lin(c))
}

/// A cone tier sitting with its base at `base_y` (cones are centre-anchored, so lift by
/// `h/2 + base_y`). `res` = radial sides.
fn cone_at(r: f32, h: f32, base_y: f32, res: u32, c: u32) -> Mesh {
    tinted(
        Cone { radius: r, height: h }
            .mesh()
            .resolution(res)
            .build()
            .translated_by(y(h * 0.5 + base_y)),
        lin(c),
    )
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

// ── Snow-laden conifer pine ──────────────────────────────────────────────────────
//
// A brown stub trunk + 3 stacked dark→light green cone tiers (wide low, narrow high),
// each capped with a smaller WHITE snow cone sitting on its boughs, plus a snow tip on
// the crown. ~1.4u tall before TREE_SCALE. Two snow-load variants (heavier vs lighter).
fn build_pine_mesh(snowy: bool) -> Mesh {
    let mut parts = vec![
        // Stub trunk poking out of the snow.
        cyl_up(0.07, 0.30, 0.15, 6, PINE_TRUNK),
    ];

    // Three green cone tiers + a white snow cone capping each bough ring. Tiers shrink
    // and rise; the snow cone is shorter & wider so it reads as snow piled on the boughs.
    // (base_y, tier_radius, tier_height, green-tone)
    let tiers = [
        (0.22, 0.55, 0.62, PINE_DARK),
        (0.58, 0.44, 0.56, PINE_MID),
        (0.94, 0.32, 0.48, PINE_LIGHT),
    ];
    for (base_y, r, h, green) in tiers {
        // The dark-green bough tier.
        parts.push(cone_at(r, h, base_y, 7, green));
        // A WHITE snow cone resting on the boughs: a hair wider at the rim, much shorter,
        // sitting at the same base so its skirt drapes over the green tier's shoulders.
        let snow_c = if snowy { SNOW_CAP_HI } else { SNOW_CAP };
        parts.push(cone_at(r * 1.04, h * 0.42, base_y + 0.015, 7, snow_c));
        // A faint blue snow-shade lobe under the snow skirt for depth (one side).
        parts.push(ball_at(r * 0.34, Vec3::new(r * 0.5, base_y + h * 0.12, 0.0), 0.45, SNOW_SHADE));
    }

    // Snow-capped crown tip (a small white cone on the very top).
    parts.push(cone_at(0.16, 0.30, 1.30, 7, SNOW_CAP_HI));
    // A couple of clinging snow dabs on lower boughs for irregularity.
    parts.push(ball_at(0.12, Vec3::new(-0.30, 0.40, 0.10), 0.5, SNOW_CAP));
    parts.push(ball_at(0.10, Vec3::new(0.24, 0.74, -0.12), 0.5, SNOW_CAP));

    flat_shaded(merged(parts))
}

// ── Bare snowy birch ───────────────────────────────────────────────────────────
//
// A pale white trunk + 2 dark bark marks + a few bare grey-brown twigs fanning from the
// top + a dab of snow caught in the crook. No foliage (winter-bare). ~1.3u tall.
fn build_birch_mesh() -> Mesh {
    let mut parts = vec![
        // Pale trunk (slightly tapered look via a slim cylinder).
        cyl_up(0.055, 1.05, 0.525, 6, BIRCH_TRUNK),
        // Two dark bark-mark boxes (peeling-bark stripes).
        tinted(
            Cuboid::new(0.006, 0.05, 0.10)
                .mesh()
                .build()
                .translated_by(Vec3::new(0.055, 0.70, 0.0)),
            lin(BIRCH_MARK),
        ),
        tinted(
            Cuboid::new(0.006, 0.04, 0.08)
                .mesh()
                .build()
                .translated_by(Vec3::new(-0.055, 0.42, 0.03)),
            lin(BIRCH_MARK),
        ),
    ];

    // A fan of bare twigs from the upper trunk: slim cones leaning out, alternating two
    // lengths. Build upright, lean (Z), yaw around the trunk, then lift to the branch crook.
    let twigs = [
        (0.0_f32, 0.5_f32, 0.40_f32),
        (1.3, 0.7, 0.34),
        (2.6, 0.45, 0.36),
        (3.9, 0.8, 0.30),
        (5.2, 0.55, 0.33),
    ];
    for (yaw, tilt, len) in twigs {
        let twig = Cone { radius: 0.016, height: len }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(y(0.92));
        parts.push(tinted(twig, lin(BIRCH_TWIG)));
    }

    // Dabs of snow: one on the crook, one near the trunk base.
    parts.push(ball_at(0.10, y(0.98), 0.45, SNOW_CAP_HI));
    parts.push(ball_at(0.13, Vec3::new(0.05, 0.06, 0.0), 0.4, SNOW_CAP));

    flat_shaded(merged(parts))
}

// ── Snow-dusted shrub / mound (the first non-tree fallback class) ────────────────
//
// A low cluster of white-blue snow blobs — a buried shrub or a wind-packed drift. Base
// flush at y=0; ~0.4u tall. Three tiers (blue-shade skirt → snow body → bright crown) so
// it reads 3D, never a flat sphere. Two variants vary the lump count.
fn build_mound_mesh(variant: u32) -> Mesh {
    let mut parts = vec![
        // Bluish snow-shade skirt (grounded spread).
        ball_at(0.30, y(0.13), 0.62, SNOW_SHADE),
        ball_at(0.22, Vec3::new(0.22, 0.11, 0.06), 0.62, SNOW_SHADE),
        ball_at(0.20, Vec3::new(-0.20, 0.10, 0.10), 0.62, SNOW_SHADE),
        // Snow body.
        ball_at(0.24, y(0.22), 0.66, SNOW_CAP),
        ball_at(0.17, Vec3::new(0.15, 0.24, -0.13), 0.66, SNOW_CAP),
        // Bright sunlit crown.
        ball_at(0.18, y(0.32), 0.7, SNOW_CAP_HI),
        ball_at(0.12, Vec3::new(-0.10, 0.35, 0.07), 0.7, SNOW_CAP_HI),
    ];
    if variant % 2 == 1 {
        // A second hump beside the first → a longer drift.
        parts.push(ball_at(0.20, Vec3::new(0.36, 0.14, -0.10), 0.62, SNOW_CAP));
        parts.push(ball_at(0.14, Vec3::new(0.40, 0.24, -0.12), 0.7, SNOW_CAP_HI));
    }
    flat_shaded(merged(parts))
}

// ── Frost boulder ────────────────────────────────────────────────────────────────
//
// A faceted blue-grey rock (dark base + lit facet) topped with a WHITE snow cap, plus a
// side cobble. Base flush at y=0. Two variants vary the proportions.
fn build_boulder_mesh(variant: u32) -> Mesh {
    match variant % 2 {
        0 => {
            let r = 0.34;
            flat_shaded(merged(vec![
                // Body — wide squashed lump, darker base tone.
                ball_at(r, y(r * 0.74), 0.78, ROCK_DARK),
                // Lit facet catching the light.
                ball_at(r * 0.6, Vec3::new(r * 0.12, r * 1.0, -r * 0.1), 0.72, ROCK_BODY),
                // Side cobble.
                ball_at(r * 0.5, Vec3::new(-r * 0.85, r * 0.4, r * 0.2), 0.82, ROCK_LIGHT),
                // White snow cap draped over the crown.
                ball_at(r * 0.78, y(r * 1.26), 0.5, SNOW_CAP_HI),
                ball_at(r * 0.42, Vec3::new(r * 0.4, r * 1.05, r * 0.2), 0.5, SNOW_CAP),
            ]))
        }
        _ => {
            let r = 0.30;
            flat_shaded(merged(vec![
                // Lower block.
                ball_at(r, y(r * 0.82), 0.9, ROCK_DARK),
                // Upper split block.
                ball_at(r * 0.72, Vec3::new(r * 0.3, r * 1.55, -r * 0.1), 0.9, ROCK_BODY),
                // Lit chip.
                ball_at(r * 0.4, Vec3::new(-r * 0.55, r * 0.7, r * 0.3), 0.85, ROCK_LIGHT),
                // Snow cap on the peak + a side dab.
                ball_at(r * 0.64, Vec3::new(r * 0.25, r * 2.0, -r * 0.08), 0.46, SNOW_CAP_HI),
                ball_at(r * 0.36, Vec3::new(-r * 0.5, r * 0.95, r * 0.3), 0.46, SNOW_CAP),
            ]))
        }
    }
}

// ── Snowman (bałwan) ──────────────────────────────────────────────────────────────
//
// The charming centrepiece: three stacked bright-snow balls (big → mid → head), a coal
// face (two eyes + an arc-of-coal smile), an orange carrot nose, two bare twig arms
// fanning up-and-out, a red scarf wound at the neck with a hanging tail, and a dark
// bucket hat. Base flush at y=0, ~1.15u tall (incl. hat) before the scatter scales it.
// Built facing +Z; the scatter gives it a random yaw. Single merged vertex-coloured mesh.
fn build_snowman_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();

    // ── Body: three stacked near-round snow balls (lightly squashed so they read packed).
    // Centres chosen so each ball overlaps the one below (no gap), bottom resting on y=0.
    let bottom_cy = 0.285;
    parts.push(ball_at(0.30, y(bottom_cy), 0.95, SNOWMAN_BODY));
    let mid_cy = 0.58;
    parts.push(ball_at(0.225, y(mid_cy), 0.96, SNOWMAN_BODY));
    let head_cy = 0.84;
    parts.push(ball_at(0.165, y(head_cy), 0.97, SNOWMAN_BODY));

    // ── Face (on the +Z front of the head): two coal eyes + a carrot nose + a coal smile.
    let face_z = 0.135;
    parts.push(ball_at(0.028, Vec3::new(0.06, head_cy + 0.03, face_z), 0.9, COAL));
    parts.push(ball_at(0.028, Vec3::new(-0.06, head_cy + 0.03, face_z), 0.9, COAL));
    // Carrot nose — a slim cone pointing +Z (built apex-up, tipped 90° about X).
    parts.push(tinted(
        Cone { radius: 0.034, height: 0.18 }
            .mesh()
            .resolution(6)
            .build()
            .rotated_by(Quat::from_rotation_x(FRAC_PI_2))
            .translated_by(Vec3::new(0.0, head_cy - 0.01, face_z + 0.02)),
        lin(CARROT),
    ));
    // Coal smile — five small coal dots in a downward arc below the nose.
    for i in 0..5 {
        let t = (i as f32 / 4.0) * 2.0 - 1.0; // -1..1 across the mouth
        let mx = t * 0.085;
        let my = head_cy - 0.075 - (1.0 - t * t) * 0.018; // dip toward the centre → a smile
        parts.push(ball_at(0.016, Vec3::new(mx, my, face_z + 0.005), 0.9, COAL));
    }

    // ── Coal buttons down the front of the mid ball.
    for &(by, bz) in &[(mid_cy + 0.06, 0.205_f32), (mid_cy - 0.04, 0.215), (mid_cy - 0.13, 0.20)] {
        parts.push(ball_at(0.022, Vec3::new(0.0, by, bz), 0.9, COAL));
    }

    // ── Twig arms — a bare twig fanning up-and-out from each side of the mid ball, each
    // with a small forked branchlet. Build along +Y, lean out (Z) ±, lift to shoulder.
    for side in [1.0_f32, -1.0] {
        let shoulder = Vec3::new(side * 0.20, mid_cy + 0.04, 0.0);
        let lean = Quat::from_rotation_z(side * -1.15); // splay outward + up
        let arm = Cylinder::new(0.016, 0.40)
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(0.20))
            .rotated_by(lean)
            .translated_by(shoulder);
        parts.push(tinted(arm, lin(BIRCH_TWIG)));
        // A short forked branchlet near the arm's tip.
        let tip = shoulder + lean * Vec3::new(0.0, 0.40, 0.0);
        let fork = Cylinder::new(0.011, 0.16)
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(0.08))
            .rotated_by(Quat::from_rotation_z(side * -0.5))
            .translated_by(tip);
        parts.push(tinted(fork, lin(BIRCH_TWIG)));
    }

    // ── Red scarf — a drum wound at the neck (between head and mid ball) + a hanging tail.
    let neck = 0.735;
    parts.push(cyl_up(0.155, 0.075, neck, 10, SCARF));
    // Tail — a thin red box hanging down the front-left of the body.
    parts.push(tinted(
        Cuboid::new(0.07, 0.26, 0.03)
            .mesh()
            .build()
            .translated_by(Vec3::new(0.10, neck - 0.14, 0.135)),
        lin(SCARF),
    ));

    // ── Dark bucket hat on the crown: a wide thin brim + a tapered crown drum.
    let head_top = head_cy + 0.165 * 0.97;
    parts.push(cyl_up(0.205, 0.035, head_top - 0.01, 12, HAT)); // brim
    parts.push(cyl_up(0.135, 0.20, head_top + 0.10, 10, HAT)); // crown

    flat_shaded(merged(parts))
}

// ── Ground cover: snow tuft + ice glint ─────────────────────────────────────────

/// A tiny snow tuft — a low cluster of three white-blue specks (wind-packed snow nub).
fn build_snow_tuft_mesh() -> Mesh {
    flat_shaded(merged(vec![
        ball_at(0.05, y(0.03), 0.5, SNOW_CAP),
        ball_at(0.04, Vec3::new(0.05, 0.03, 0.02), 0.5, SNOW_CAP_HI),
        ball_at(0.035, Vec3::new(-0.04, 0.025, -0.03), 0.5, SNOW_SHADE),
    ]))
}

/// An ice glint — a tiny flat pale-blue ice shard catching the light (a low faceted disc
/// nub). Sits essentially flat at y≈0.
fn build_ice_glint_mesh() -> Mesh {
    flat_shaded(merged(vec![
        ball_at(0.06, y(0.012), 0.18, ICE_PALE),
        ball_at(0.035, Vec3::new(0.04, 0.02, 0.0), 0.3, SNOW_CAP_HI),
    ]))
}

/// Winter ground litter (cover). `variant`: 0 = a holly sprig (dark leaves + red berries +
/// a snow dab), 1 = a snow-capped pinecone, 2 = a tiny pale-blue ice-shard cluster. Very
/// low (≤0.13u), base at y=0 — the colourful little touches that dress the snowfield.
fn build_winter_litter_mesh(variant: u32) -> Mesh {
    let tau = std::f32::consts::TAU;
    match variant % 3 {
        // Holly sprig — four dark flattened leaves around a few red berries + a snow dab.
        0 => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..4 {
                let a = (i as f32 / 4.0) * tau;
                parts.push(ball_at(0.05, Vec3::new(a.cos() * 0.06, 0.03, a.sin() * 0.06), 0.28, HOLLY_LEAF));
            }
            parts.push(ball_at(0.022, y(0.05), 0.9, HOLLY_BERRY));
            parts.push(ball_at(0.020, Vec3::new(0.03, 0.05, 0.0), 0.9, HOLLY_BERRY));
            parts.push(ball_at(0.020, Vec3::new(-0.02, 0.05, 0.025), 0.9, HOLLY_BERRY));
            parts.push(ball_at(0.04, Vec3::new(0.05, 0.02, -0.04), 0.4, SNOW_CAP_HI));
            flat_shaded(merged(parts))
        }
        // Snow-capped pinecone — a stack of brown balls with a bright snow tip.
        1 => flat_shaded(merged(vec![
            ball_at(0.045, y(0.04), 1.15, PINECONE_BROWN),
            ball_at(0.036, y(0.085), 1.15, PINECONE_BROWN),
            ball_at(0.026, y(0.115), 1.1, SNOW_CAP_HI),
        ])),
        // Ice-shard cluster — a few small pale-blue pointed cones poking up.
        _ => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..3 {
                let a = (i as f32 / 3.0) * tau + 0.4;
                let h = 0.10 + (i % 2) as f32 * 0.05;
                let shard = Cone { radius: 0.022, height: h }
                    .mesh()
                    .resolution(5)
                    .build()
                    .translated_by(y(h * 0.5))
                    .rotated_by(Quat::from_rotation_z(0.2 * (i as f32 - 1.0)))
                    .translated_by(Vec3::new(a.cos() * 0.04, 0.0, a.sin() * 0.04));
                parts.push(tinted(shard, lin(ICE_PALE)));
            }
            parts.push(ball_at(0.03, y(0.02), 0.4, SNOW_CAP_HI));
            flat_shaded(merged(parts))
        }
    }
}

// ── A snow-laden bare dead tree for the pond ring ────────────────────────────────
//
// A grey-brown bare trunk + a few up-angled broken branches, each carrying a snow dab.
// Base at y=0. ~1.3u tall.
fn build_dead_snow_tree_mesh() -> Mesh {
    let mut parts = vec![cyl_up(0.07, 1.10, 0.55, 6, BIRCH_TWIG)];

    // Four angled bare branches with a snow dab near each tip.
    let branches = [
        (0.0_f32, -0.8_f32, 0.46_f32, Vec3::new(0.18, 0.80, 0.06)),
        (1.6, 0.7, 0.40, Vec3::new(-0.16, 0.92, -0.04)),
        (3.0, 0.5, 0.34, Vec3::new(0.06, 1.05, 0.12)),
        (4.5, -0.55, 0.30, Vec3::new(-0.10, 1.10, -0.10)),
    ];
    for (yaw, tilt, len, tip) in branches {
        let m = Cone { radius: 0.022, height: len }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(tip);
        parts.push(tinted(m, lin(BIRCH_TWIG)));
        // Snow dab clinging to the branch.
        parts.push(ball_at(0.07, tip + y(len * 0.18), 0.5, SNOW_CAP_HI));
    }
    // Snow piled at the base.
    parts.push(ball_at(0.16, y(0.06), 0.4, SNOW_CAP));
    parts.push(ball_at(0.11, Vec3::new(0.18, 0.05, 0.05), 0.4, SNOW_CAP_HI));

    flat_shaded(merged(parts))
}

// ── config() ─────────────────────────────────────────────────────────────────────

pub fn config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Snow,
        name: "Snow",

        ground_color: SNOW_GROUND,
        ground_roughness: 0.82,
        detail: GroundDetail {
            // Subtle, low strength so the snowfield reads broad & smooth but not dead-flat:
            // a faint blue-grey shadow drift over a bright base.
            scale: 0.14,
            strength: 0.22,
            variation: 0.42,
            seed: 4.0,
            dark: SNOW_GROUND_DARK,
            base: SNOW_GROUND,
            light: SNOW_GROUND_LIGHT,
            grain: 0.30,
            streak: 0.22,
        },

        // Cool bright winter daylight; slightly higher ambient (snow bounces a lot of
        // fill light) + denser cool fog so distant peaks fade into a pale haze.
        sky: 0xcedef0,
        fog_density: 0.013,
        sun_color: 0xfff4e0,
        sun_illuminance: 11_500.0,
        ambient_color: 0xdbe7f5,
        ambient_brightness: 120.0,
        sun_pos: Vec3::new(18.0, 42.0, 12.0),

        seed: 4127,
        tree_min_dist: 2.9,
        classes: vec![
            // Trees: 78% snow-laden conifer (two snow loads) / 22% bare snowy birch.
            PropClass {
                variants: vec![
                    (build_pine_mesh(false), 0.46),
                    (build_pine_mesh(true), 0.32),
                    (build_birch_mesh(), 0.22),
                ],
                chance: 0.072,
                scale: (0.85 * TREE_SCALE, 1.25 * TREE_SCALE),
                tree: true,
                block_radius: 0.0,
            },
            // Snow shrub / mound — FIRST non-tree class (the tree-too-close fallback).
            PropClass {
                variants: vec![(build_mound_mesh(0), 1.0), (build_mound_mesh(1), 1.0)],
                chance: 0.055,
                scale: (0.8, 1.45),
                tree: false,
                block_radius: 0.0,
            },
            // Frost boulders.
            PropClass {
                variants: vec![(build_boulder_mesh(0), 1.0), (build_boulder_mesh(1), 1.0)],
                chance: 0.028,
                scale: (0.6, 1.5),
                tree: false,
                block_radius: 0.28, // big frost boulders block; small ones walk-through
            },
            // Snowman (bałwan) — a rare charming centrepiece, sprinkled sparsely (~10 per
            // patch). Kept LAST so it never becomes the tree-too-close fallback.
            PropClass {
                variants: vec![(build_snowman_mesh(), 1.0)],
                chance: 0.007,
                scale: (0.9, 1.2),
                tree: false,
                block_radius: 0.3, // a snowman is a solid body-sized figure — don't walk through
            },
        ],
        cover: vec![
            // Snow tufts everywhere; sparser ice glints.
            PropClass {
                variants: vec![(build_snow_tuft_mesh(), 1.0)],
                chance: 0.34,
                scale: (0.55, 1.1),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_ice_glint_mesh(), 1.0)],
                chance: 0.10,
                scale: (0.6, 1.2),
                tree: false,
                block_radius: 0.0,
            },
            // Winter litter — holly berries, snow-capped pinecones, ice shards.
            PropClass {
                variants: (0..3).map(|v| (build_winter_litter_mesh(v), 1.0)).collect(),
                chance: 0.09,
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover_per_tile: 2,

        river: false,
        river_color: 0x2f8fd6,
        backdrop: Backdrop {
            // Land arc faces -z (the camera-facing far side); tall white-capped peaks over
            // a dark conifer treeline. No ocean (frozen interior).
            land_dir: -FRAC_PI_2,
            land_arc: std::f32::consts::FRAC_PI_2,
            ocean: false,
            ocean_color: 0x4a6f8e,
            hill_body: 0x9fb0c2, // blue-grey snowy massif body
            hill_cap: 0xf2f7fc,  // near-white peak caps
            hill_foot: 0x7e90a4, // shadowed lower slopes
            treeline: true,
            treeline_dark: 0x223f30, // deep conifer band
            treeline_mid: 0x2c5240,
            hill_h: (44.0, 92.0), // tall peaks
        },
        particle: ParticleKind::Snow,
    }
}

// ── landmarks() — the frozen pond ────────────────────────────────────────────────

/// A frozen pond: a pale-blue low-roughness ice disc sitting just above y=0 (reflects the
/// sky via IBL), ringed by a darker frosted rim, a couple of snow-laden dead trees, and a
/// small rock cairn. All entities tagged `BiomeEntity` so a biome switch wipes them.
pub fn landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Shared white vertex-colour material for the snowy set-pieces (matches scatter).
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });

    // The pond sits to the LAND side (z < 0) so nothing covers it; offset a touch off
    // centre so it doesn't sit dead-ahead of the camera.
    let pond = Vec3::new(-3.5, 0.0, -7.0);
    let pond_r = 4.2_f32;

    // ── Frozen ice disc — low roughness so it mirrors the sky/IBL. A `Circle` mesh lies
    // in XY (normal +Z); rotate -90° about X to lie flat on the ground plane. Sit it just
    // above y=0 (the opaque ground plane is at y=0) to avoid z-fighting.
    let ice_mat = materials.add(StandardMaterial {
        base_color: crate::palette::srgb(ICE_PALE),
        perceptual_roughness: 0.08,
        metallic: 0.0,
        reflectance: 0.6,
        ..default()
    });
    let ice_disc = Circle::new(pond_r)
        .mesh()
        .resolution(48)
        .build()
        .rotated_by(Quat::from_rotation_x(-FRAC_PI_2));
    commands.spawn((
        Mesh3d(meshes.add(ice_disc)),
        MeshMaterial3d(ice_mat),
        Transform::from_translation(pond + y(0.05)),
        BiomeEntity,
    ));

    // Frosted rim ring — a slightly larger, darker disc a hair LOWER than the ice so it
    // peeks out as a frozen shoreline lip (uses the shared vertex-colour material).
    let rim = tinted(
        Circle::new(pond_r * 1.12)
            .mesh()
            .resolution(48)
            .build()
            .rotated_by(Quat::from_rotation_x(-FRAC_PI_2)),
        lin(ICE_RIM),
    );
    commands.spawn((
        Mesh3d(meshes.add(flat_shaded(rim))),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(pond + y(0.025)),
        BiomeEntity,
    ));

    // ── Ring of snow-laden dead trees around the pond ──
    let dead_tree = meshes.add(build_dead_snow_tree_mesh());
    let tree_angles = [0.7_f32, 2.5, 4.1, 5.4];
    let tree_scales = [1.6_f32, 1.3, 1.5, 1.2];
    for (i, &a) in tree_angles.iter().enumerate() {
        let rr = pond_r * 1.22;
        let tx = pond.x + a.cos() * rr;
        let tz = pond.z + a.sin() * rr;
        commands.spawn((
            Mesh3d(dead_tree.clone()),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: Vec3::new(tx, 0.0, tz),
                rotation: Quat::from_rotation_y(a * 1.7),
                scale: Vec3::splat(tree_scales[i]),
            },
            BiomeEntity,
        ));
    }

    // ── A small rock cairn beside the pond (stacked frost-rock balls, snow-capped) ──
    let cairn = {
        let parts = vec![
            ball_at(0.34, y(0.26), 0.78, ROCK_DARK),
            ball_at(0.28, y(0.62), 0.82, CAIRN_STONE),
            ball_at(0.22, y(0.92), 0.86, ROCK_LIGHT),
            ball_at(0.16, y(1.14), 0.9, CAIRN_STONE),
            // Snow cap on the top stone + a dab on the shoulder.
            ball_at(0.15, y(1.28), 0.5, SNOW_CAP_HI),
            ball_at(0.12, Vec3::new(0.18, 0.70, 0.06), 0.5, SNOW_CAP),
        ];
        flat_shaded(merged(parts))
    };
    let cairn_pos = pond + Vec3::new(pond_r * 0.85, 0.0, pond_r * 0.55);
    commands.spawn((
        Mesh3d(meshes.add(cairn)),
        MeshMaterial3d(mat),
        Transform {
            translation: Vec3::new(cairn_pos.x, 0.0, cairn_pos.z),
            rotation: Quat::from_rotation_y(0.6),
            scale: Vec3::splat(1.4),
        },
        BiomeEntity,
    ));
}
