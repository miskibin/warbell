//! Snow biome (key 2) — a crisp winter scene mirroring the Forest module's structure.
//!
//! All snow props are built **locally** in this file (self-contained — touches no other
//! module). Conifer pines stack off-axis green cone tiers (a zig-zag silhouette, three
//! variants incl. a tall slim spire) and drape snow PER TIER — a short skirt cone plus
//! squashed blobs piled on the rim, sunlit-white on top, ice-blue in shade. Bare birches
//! get a kinked two-segment trunk and a two-storey twig fan. The mound class is three
//! variants: a wind-sculpted drift, a frost-tipped frozen shrub, and a snow-capped stump
//! + fallen log. Boulders are angular tilted rock chunks over dark exposed footings,
//! draped with snow and hung with icicles under their overhangs. Plus the rare snowman
//! centrepiece. Ground cover: snow tufts with dry winter grass poking through, angular
//! ice glints, and litter (holly sprig / toppled pinecone / ice shards / frosted twig).
//! Every prop gets `bake_facet_shading`: down-facing facets darken and cool toward blue
//! (shadowed snow reads blue, not grey), up-facing facets brighten — baked light/shadow
//! contrast that realtime fill alone can't produce (see trees.rs for the rationale).
//! Particle: drifting snow. Backdrop: tall white-capped peaks over a dark conifer
//! treeline, land on one side, no ocean.
//!
//! Landmark: a frozen pond — a low-roughness pale-blue ice disc sitting just above y=0
//! (reflects the sky via IBL) with a frosted rim, ringed by snow-laden dead trees,
//! shoreline ice-shard clumps, a drift tongue, and an angular snow-capped stone cairn.
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

// Stumps / logs / winter grass — the warm accents poking through the snowfield.
const STUMP_WOOD: u32 = 0x584234; // weathered stump / log bark
const STUMP_CUT: u32 = 0xa8896a; // pale sawn cut-face ring
const GRASS_DRY: u32 = 0xb3a274; // dry straw-yellow winter grass

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

/// Bake painterly facet shading into the vertex colours (call AFTER `flat_shaded`, so the
/// per-face normals exist and every facet shades uniformly): down-facing facets darken AND
/// cool toward blue (shadowed snow reads blue, not grey), up-facing facets brighten toward
/// sunlit white, plus a mild base→crown lift. Gentler factors than the trees.rs bake and
/// clamped at 1.0 so the near-white snow tones never blow out / bloom.
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
            let f = (0.76 + 0.36 * up) * (0.90 + 0.16 * h);
            // The deeper the shadow, the bluer it gets (snow bounce light is sky-blue).
            let cool = (1.0 - f).max(0.0) * 0.35;
            c[0] = (c[0] * f).min(1.0);
            c[1] = (c[1] * (f + cool * 0.3)).min(1.0);
            c[2] = (c[2] * (f + cool)).min(1.0);
        }
    }
    m
}

/// The standard finish for every snow prop: flat-shade, then bake the facet shading.
fn shaded(m: Mesh) -> Mesh {
    bake_facet_shading(flat_shaded(m))
}

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// An upright cylinder rooted at y=0 (a part of height `h` uses centre `cy = h/2`).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), lin(c))
}

/// A cone with its base-circle centre at `at` (cones are centre-anchored, so lift by
/// `h/2`). `res` = radial sides.
fn cone_at(r: f32, h: f32, at: Vec3, res: u32, c: u32) -> Mesh {
    tinted(
        Cone { radius: r, height: h }
            .mesh()
            .resolution(res)
            .build()
            .translated_by(at + y(h * 0.5)),
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

/// An angular rock chunk: an icosphere squashed non-uniformly THEN tilted, so its facets
/// skew into a fractured-block read instead of a pebble. `detail` 0 (20 tris) or 1 (80).
fn chunk_at(r: f32, off: Vec3, scale: Vec3, yaw: f32, pitch: f32, detail: u32, c: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(detail)
            .expect("ico detail in range")
            .scaled_by(scale)
            .rotated_by(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch))
            .translated_by(off),
        lin(c),
    )
}

/// An icicle — a slim pale-blue cone hanging point-DOWN, its root (base disc) at `root`,
/// tip reaching `root.y - len`. Callers must hang them from ledges with `root.y > len`
/// so nothing pokes below y=0.
fn icicle(r: f32, len: f32, root: Vec3) -> Mesh {
    tinted(
        Cone { radius: r, height: len }
            .mesh()
            .resolution(4)
            .build()
            .rotated_by(Quat::from_rotation_x(std::f32::consts::PI)) // flip apex down
            .translated_by(root - y(len * 0.5)),
        lin(ICE_PALE),
    )
}

// ── Snow-laden conifer pine ──────────────────────────────────────────────────────
//
// A brown trunk (with a root flare) + a stack of dark→light green cone tiers, every tier
// nudged off-axis so the silhouette zig-zags instead of reading as a perfect kebab. Snow
// is DRAPED per tier: a short wide skirt cone over the rim plus 3 squashed blobs piled on
// the boughs — sunlit-white / body / ice-blue shaded lobes. Crown carries a green spike
// + a white snow tip; a drift banks against the trunk. `bake_facet_shading` then darkens
// the bough undersides. Variants: 0 = broad + heavy load, 1 = standard + light dusting,
// 2 = tall slim 5-tier spire. ~1.7u tall before TREE_SCALE.
fn build_pine_mesh(variant: u32) -> Mesh {
    // (tier count, base tier radius, base tier height, heavy snow load?)
    let (tiers_n, r0, h0, heavy) = match variant % 3 {
        0 => (4, 0.58, 0.50, true),
        1 => (4, 0.53, 0.48, false),
        _ => (5, 0.43, 0.42, true),
    };

    let mut parts = vec![
        // Trunk poking out of the snow + a wider root flare at the base.
        cyl_up(0.075, 0.36, 0.18, 6, PINE_TRUNK),
        cyl_up(0.105, 0.10, 0.05, 6, PINE_TRUNK),
    ];

    let tau = std::f32::consts::TAU;
    let mut base_y = 0.22;
    let mut top = base_y;
    for k in 0..tiers_n {
        let t = k as f32 / (tiers_n - 1) as f32;
        let r = r0 * (1.0 - 0.55 * t);
        let h = h0 * (1.0 - 0.16 * t);
        // Off-axis nudge, strongest low, fading toward the crown → a zig-zag silhouette.
        let wob = 0.055 * (1.0 - t * 0.7);
        let at = Vec3::new(
            (k as f32 * 2.3 + variant as f32 * 1.7).sin() * wob,
            base_y,
            (k as f32 * 1.6 + variant as f32).cos() * wob,
        );
        // Green bough tier: shadowed dark low boughs → lit upper tiers.
        let green = if t < 0.34 {
            PINE_DARK
        } else if t < 0.75 {
            PINE_MID
        } else {
            PINE_LIGHT
        };
        parts.push(cone_at(r, h, at, 7, green));
        // Snow skirt draped over the tier's shoulders: a hair wider at the rim, much
        // shorter, so its hem drapes over the green tier's boughs.
        let skirt_h = if heavy { h * 0.40 } else { h * 0.30 };
        parts.push(cone_at(r * 1.05, skirt_h, at + y(0.015), 7, if heavy { SNOW_CAP } else { SNOW_SHADE }));
        // Three snow blobs piled around the rim — sunlit / body / ice-blue shaded lobes
        // walking around the tree per tier so no two tiers load the same side.
        let blob_r = r * if heavy { 0.34 } else { 0.26 };
        for (j, tone) in [SNOW_CAP_HI, SNOW_CAP, SNOW_SHADE].into_iter().enumerate() {
            let a = j as f32 / 3.0 * tau + k as f32 * 0.9 + variant as f32 * 0.5;
            let p = at + Vec3::new(a.cos() * r * 0.62, h * 0.26, a.sin() * r * 0.62);
            parts.push(ball_at(blob_r, p, 0.5, tone));
        }
        base_y += h * 0.58;
        top = at.y + h;
    }

    // Crown: a green spike + a white snow tip wrapping it + a clinging dab just below.
    parts.push(cone_at(0.13, 0.26, y(top - 0.10), 7, PINE_LIGHT));
    parts.push(cone_at(0.10, 0.20, y(top + 0.06), 6, SNOW_CAP_HI));
    parts.push(ball_at(0.085, Vec3::new(0.06, top - 0.04, -0.04), 0.55, SNOW_CAP));
    // Drift banked against the trunk base (bright lee side / blue windward side).
    parts.push(ball_at(0.16, Vec3::new(0.10, 0.05, 0.06), 0.4, SNOW_CAP));
    parts.push(ball_at(0.13, Vec3::new(-0.13, 0.04, -0.05), 0.4, SNOW_SHADE));

    shaded(merged(parts))
}

// ── Bare snowy birch ───────────────────────────────────────────────────────────
//
// A kinked two-segment pale trunk (lower upright + upper segment leaning a touch) + four
// peeling-bark scar boxes + a two-storey fan of bare twigs from the crook — long mains
// with forking twiglets — snow dabs caught in the crooks, a frost ledge clinging to the
// trunk's lee side, and a drift collar at the base. No foliage (winter-bare). ~1.3u tall.
fn build_birch_mesh() -> Mesh {
    let kink = 0.10; // upper-trunk lean (radians)
    let mut parts = vec![
        // Lower trunk + the slightly leaning upper segment → a gentle kink, not a pole.
        cyl_up(0.060, 0.62, 0.31, 6, BIRCH_TRUNK),
        tinted(
            Cylinder::new(0.048, 0.55)
                .mesh()
                .resolution(6)
                .build()
                .translated_by(y(0.275))
                .rotated_by(Quat::from_rotation_z(kink))
                .translated_by(y(0.58)),
            lin(BIRCH_TRUNK),
        ),
    ];

    // Peeling-bark scar boxes wrapped at varying heights and alternating sides.
    let marks = [
        (0.060_f32, 0.70_f32, 0.0_f32, 0.05_f32, 0.10_f32),
        (-0.060, 0.42, 0.03, 0.04, 0.08),
        (0.052, 0.94, -0.02, 0.04, 0.07),
        (-0.048, 0.20, -0.03, 0.035, 0.09),
    ];
    for (mx, my, mz, mh, md) in marks {
        parts.push(tinted(
            Cuboid::new(0.008, mh, md).mesh().build().translated_by(Vec3::new(mx, my, mz)),
            lin(BIRCH_MARK),
        ));
    }

    // Two-storey twig fan from the crook (top of the leaning segment): six slim mains
    // alternating length/pitch; every third forks a twiglet at its elbow; every other
    // catches a snow dab where it meets the trunk.
    let crook = Vec3::new(-0.05, 1.06, 0.0);
    let twigs = [
        (0.0_f32, 0.55_f32, 0.42_f32),
        (1.1, 0.75, 0.34),
        (2.2, 0.45, 0.38),
        (3.3, 0.85, 0.30),
        (4.4, 0.60, 0.35),
        (5.4, 0.40, 0.28),
    ];
    for (i, (yaw, tilt, len)) in twigs.into_iter().enumerate() {
        let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt);
        let twig = Cone { radius: 0.016, height: len }
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(rot)
            .translated_by(crook);
        parts.push(tinted(twig, lin(BIRCH_TWIG)));
        if i % 3 == 0 {
            // A forking twiglet splayed off this main's elbow.
            let elbow = crook + rot * y(len * 0.6);
            let frot = rot * Quat::from_rotation_z(0.55);
            let f = Cone { radius: 0.010, height: len * 0.45 }
                .mesh()
                .resolution(4)
                .build()
                .translated_by(y(len * 0.225))
                .rotated_by(frot)
                .translated_by(elbow);
            parts.push(tinted(f, lin(BIRCH_TWIG)));
        }
        if i % 2 == 0 {
            // Snow dab caught where the twig leaves the trunk.
            parts.push(ball_at(0.05, crook + rot * y(len * 0.22), 0.55, SNOW_CAP_HI));
        }
    }

    // Snow in the main crook, a frost ledge on the trunk's lee side, drift collar at the
    // base (bright + blue-shaded lobes).
    parts.push(ball_at(0.09, crook + y(0.02), 0.5, SNOW_CAP_HI));
    parts.push(ball_at(0.045, Vec3::new(0.058, 0.55, 0.01), 0.4, SNOW_CAP));
    parts.push(ball_at(0.14, Vec3::new(0.06, 0.05, 0.02), 0.42, SNOW_CAP));
    parts.push(ball_at(0.10, Vec3::new(-0.10, 0.04, -0.05), 0.42, SNOW_SHADE));

    shaded(merged(parts))
}

// ── Snow shrub / drift / stump (the first non-tree fallback class) ───────────────
//
// Three low winter set-dressing variants, all base-flush and ≤ ~0.45u tall:
//   0 — wind-sculpted drift: a tapering line of snow lumps kicking into a tail, bright
//       wind-packed crest on top, blue-shadowed windward skirt (reads directional);
//   1 — frozen shrub: a dark evergreen tuft buried to its shoulders, frost-tipped bare
//       twigs poking through, snow banked around it;
//   2 — snow-covered stump + fallen log: sawn cut-face ring, snow capping both.
// `pub(crate)`: the world map also banks variant-0 drifts against snow-terrace cliff
// bases (see `worldmap::build`'s drift pass), beyond this biome's own scatter.
pub(crate) fn build_mound_mesh(variant: u32) -> Mesh {
    match variant % 3 {
        // Wind-sculpted drift.
        0 => shaded(merged(vec![
            // Head → tail lumps along +X, the tail kicking into -Z.
            ball_at(0.28, Vec3::new(-0.12, 0.15, 0.0), 0.62, SNOW_CAP),
            ball_at(0.22, Vec3::new(0.14, 0.12, -0.05), 0.60, SNOW_CAP),
            ball_at(0.15, Vec3::new(0.36, 0.09, -0.10), 0.58, SNOW_CAP),
            ball_at(0.09, Vec3::new(0.52, 0.05, -0.14), 0.55, SNOW_CAP),
            // Bright wind-packed crest.
            ball_at(0.18, Vec3::new(-0.10, 0.25, 0.0), 0.6, SNOW_CAP_HI),
            ball_at(0.12, Vec3::new(0.14, 0.19, -0.05), 0.6, SNOW_CAP_HI),
            // Blue-shadowed windward skirt.
            ball_at(0.22, Vec3::new(-0.26, 0.09, 0.06), 0.5, SNOW_SHADE),
            ball_at(0.14, Vec3::new(0.05, 0.07, 0.12), 0.5, SNOW_SHADE),
        ])),
        // Frozen shrub with frost tips.
        1 => {
            let mut parts = vec![
                // Snow banked around the buried shrub.
                ball_at(0.26, y(0.10), 0.52, SNOW_CAP),
                ball_at(0.18, Vec3::new(0.20, 0.08, 0.10), 0.5, SNOW_SHADE),
                ball_at(0.16, Vec3::new(-0.18, 0.08, -0.08), 0.5, SNOW_CAP_HI),
                // Dark evergreen tuft poking through.
                ball_at(0.19, y(0.22), 0.75, PINE_DARK),
                ball_at(0.13, Vec3::new(0.10, 0.30, -0.06), 0.75, PINE_MID),
            ];
            // Bare twigs leaning out of the tuft, each tipped with a frost crystal.
            let tau = std::f32::consts::TAU;
            for i in 0..4 {
                let a = i as f32 / 4.0 * tau + 0.6;
                let rot = Quat::from_rotation_y(a) * Quat::from_rotation_z(0.55);
                let len = 0.20 + (i % 2) as f32 * 0.06;
                let root = Vec3::new(a.cos() * 0.06, 0.24, a.sin() * 0.06);
                let twig = Cone { radius: 0.012, height: len }
                    .mesh()
                    .resolution(4)
                    .build()
                    .translated_by(y(len * 0.5))
                    .rotated_by(rot)
                    .translated_by(root);
                parts.push(tinted(twig, lin(BIRCH_TWIG)));
                parts.push(ball_at(0.030, root + rot * y(len), 0.8, SNOW_CAP_HI));
            }
            // Frost dabs sitting on the evergreen crown.
            parts.push(ball_at(0.09, y(0.35), 0.5, SNOW_CAP_HI));
            parts.push(ball_at(0.06, Vec3::new(0.11, 0.36, -0.06), 0.5, SNOW_CAP));
            shaded(merged(parts))
        }
        // Snow-covered stump + fallen log.
        _ => {
            let mut parts = vec![
                // Stump: bark drum + root flare + pale sawn cut-face ring + a snow cap.
                cyl_up(0.125, 0.24, 0.12, 7, STUMP_WOOD),
                cyl_up(0.155, 0.06, 0.03, 7, STUMP_WOOD),
                cyl_up(0.110, 0.035, 0.255, 7, STUMP_CUT),
                ball_at(0.105, y(0.295), 0.45, SNOW_CAP_HI),
                // Snow banked against the stump's foot.
                ball_at(0.13, Vec3::new(0.14, 0.05, 0.08), 0.45, SNOW_CAP),
            ];
            // Fallen log alongside (lying cylinder, yawed) with a snow ridge on top.
            let lyaw = 0.5_f32;
            let log = Cylinder::new(0.075, 0.52)
                .mesh()
                .resolution(7)
                .build()
                .rotated_by(Quat::from_rotation_z(FRAC_PI_2)) // lie the axis along X
                .rotated_by(Quat::from_rotation_y(lyaw))
                .translated_by(Vec3::new(0.05, 0.075, -0.30));
            parts.push(tinted(log, lin(STUMP_WOOD)));
            // Three snow lumps along the log's upper flank (bright in the middle).
            let along = Vec3::new(lyaw.cos(), 0.0, -lyaw.sin());
            for i in 0..3 {
                let t = i as f32 - 1.0;
                parts.push(ball_at(
                    0.07 - (t * t) * 0.015,
                    Vec3::new(0.05, 0.145, -0.30) + along * (t * 0.16),
                    0.5,
                    if i == 1 { SNOW_CAP_HI } else { SNOW_CAP },
                ));
            }
            shaded(merged(parts))
        }
    }
}

// ── Frost boulder ────────────────────────────────────────────────────────────────
//
// Angular fractured stone, not pebbles: non-uniformly squashed + tilted icosphere chunks
// stacked over a DARK exposed footing (the shadowed stone under the snowline), draped
// with squashed snow blobs (bright crown / body / ice-blue shade lobes) and hung with
// icicles under the overhang lips. Base flush at y=0. Three silhouettes:
//   0 — broad tilted slab with a +X overhang (two icicles) and a side cobble;
//   1 — tall split tor: two counter-tilted blocks, one long icicle under the upper lip;
//   2 — low huddle of three cobbles bridged by a snow blanket.
fn build_boulder_mesh(variant: u32) -> Mesh {
    match variant % 3 {
        0 => {
            let mut parts = vec![
                // Dark buried under-stone (the exposed shadowed footing).
                chunk_at(0.30, y(0.12), Vec3::new(1.35, 0.55, 1.1), 0.2, 0.0, 0, ROCK_DARK),
                // Main tilted slab — the tilt opens an overhang lip on the +X side.
                chunk_at(0.34, y(0.30), Vec3::new(1.25, 0.72, 1.0), 0.45, 0.18, 1, ROCK_BODY),
                // Lit crown facet + a side cobble huddled against the slab.
                chunk_at(0.20, Vec3::new(-0.06, 0.46, -0.05), Vec3::new(1.2, 0.6, 1.0), 0.9, -0.1, 0, ROCK_LIGHT),
                chunk_at(0.15, Vec3::new(-0.34, 0.12, 0.18), Vec3::new(1.1, 0.85, 1.0), 1.4, 0.2, 0, ROCK_BODY),
                // Snow draped over the crown — bright top + body + ice-blue shade lobes.
                ball_at(0.22, y(0.52), 0.45, SNOW_CAP_HI),
                ball_at(0.15, Vec3::new(0.16, 0.46, 0.12), 0.45, SNOW_CAP),
                ball_at(0.13, Vec3::new(-0.18, 0.44, -0.14), 0.45, SNOW_SHADE),
                ball_at(0.08, Vec3::new(-0.36, 0.22, 0.18), 0.5, SNOW_CAP),
            ];
            // Icicles hanging off the overhang lip (+X side); tips stay well above y=0.
            parts.push(icicle(0.022, 0.13, Vec3::new(0.40, 0.26, 0.04)));
            parts.push(icicle(0.016, 0.09, Vec3::new(0.36, 0.24, -0.10)));
            shaded(merged(parts))
        }
        1 => {
            let mut parts = vec![
                // Lower block (dark) + counter-tilted upper block → the split-tor profile.
                chunk_at(0.28, y(0.24), Vec3::new(1.15, 0.95, 1.05), 0.1, 0.1, 1, ROCK_DARK),
                chunk_at(0.21, Vec3::new(0.08, 0.58, -0.04), Vec3::new(1.2, 0.85, 0.95), 0.8, -0.22, 1, ROCK_BODY),
                // Lit chip wedged in the split.
                chunk_at(0.12, Vec3::new(-0.16, 0.50, 0.10), Vec3::new(1.0, 0.7, 1.1), 1.9, 0.15, 0, ROCK_LIGHT),
                // Snow: bright peak cap + a dab on each shoulder + a shade lobe low.
                ball_at(0.14, Vec3::new(0.08, 0.74, -0.04), 0.45, SNOW_CAP_HI),
                ball_at(0.10, Vec3::new(-0.14, 0.56, 0.10), 0.45, SNOW_CAP),
                ball_at(0.09, Vec3::new(0.20, 0.40, 0.14), 0.45, SNOW_SHADE),
            ];
            // One long icicle under the upper block's lip.
            parts.push(icicle(0.020, 0.16, Vec3::new(0.26, 0.46, -0.06)));
            shaded(merged(parts))
        }
        _ => shaded(merged(vec![
            // Three huddled cobbles, a snow blanket bridging their tops.
            chunk_at(0.26, Vec3::new(-0.08, 0.16, 0.02), Vec3::new(1.2, 0.7, 1.0), 0.3, 0.1, 1, ROCK_BODY),
            chunk_at(0.17, Vec3::new(0.24, 0.12, -0.12), Vec3::new(1.1, 0.75, 1.0), 1.1, -0.1, 0, ROCK_DARK),
            chunk_at(0.13, Vec3::new(0.12, 0.10, 0.22), Vec3::new(1.0, 0.8, 1.1), 2.0, 0.12, 0, ROCK_LIGHT),
            // Snow blanket draped across the tops + a shade lobe in the crevice.
            ball_at(0.19, Vec3::new(-0.06, 0.30, 0.02), 0.42, SNOW_CAP_HI),
            ball_at(0.13, Vec3::new(0.20, 0.22, -0.10), 0.42, SNOW_CAP),
            ball_at(0.10, Vec3::new(0.10, 0.18, 0.18), 0.45, SNOW_CAP),
            ball_at(0.09, Vec3::new(0.04, 0.16, -0.18), 0.5, SNOW_SHADE),
        ])),
    }
}

// ── Snowman (bałwan) ──────────────────────────────────────────────────────────────
//
// The charming centrepiece: three stacked bright-snow balls (big → mid → head), a coal
// face (two eyes + an arc-of-coal smile), an orange carrot nose, two bare twig arms
// fanning up-and-out, a red scarf wound at the neck with a hanging tail, a dark bucket
// hat with a red knitted band, and a drift skirt banked against his base (hides the
// ball/ground seam). Base flush at y=0, ~1.15u tall (incl. hat) before the scatter
// scales it. Built facing +Z; the scatter gives it a random yaw. Single merged mesh.
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

    // ── Drift skirt banked against the base (bright lee lobe / blue windward lobe).
    parts.push(ball_at(0.20, Vec3::new(0.16, 0.05, 0.14), 0.45, SNOW_CAP));
    parts.push(ball_at(0.17, Vec3::new(-0.20, 0.04, -0.08), 0.45, SNOW_SHADE));

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

    // ── Dark bucket hat on the crown: a wide thin brim + a tapered crown drum + a red
    // knitted band wrapping the crown's base (matches the scarf).
    let head_top = head_cy + 0.165 * 0.97;
    parts.push(cyl_up(0.205, 0.035, head_top - 0.01, 12, HAT)); // brim
    parts.push(cyl_up(0.135, 0.20, head_top + 0.10, 10, HAT)); // crown
    parts.push(cyl_up(0.142, 0.045, head_top + 0.035, 10, SCARF)); // band

    shaded(merged(parts))
}

// ── Ground cover: snow tuft + ice glint ─────────────────────────────────────────

/// A tiny snow tuft — wind-packed snow specks with three dry straw-yellow winter grass
/// blades poking through (the sparse grass surviving under the snow), the tallest blade
/// carrying a frost crystal at its tip.
fn build_snow_tuft_mesh() -> Mesh {
    let mut parts = vec![
        ball_at(0.05, y(0.03), 0.5, SNOW_CAP),
        ball_at(0.04, Vec3::new(0.05, 0.03, 0.02), 0.5, SNOW_CAP_HI),
        ball_at(0.035, Vec3::new(-0.04, 0.025, -0.03), 0.5, SNOW_SHADE),
    ];
    let blades = [(0.4_f32, 0.30_f32, 0.11_f32), (2.5, -0.40, 0.14), (4.6, 0.25, 0.09)];
    for (i, (yaw, tilt, len)) in blades.into_iter().enumerate() {
        let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt);
        let at = Vec3::new(yaw.cos() * 0.03, 0.0, yaw.sin() * 0.03);
        let b = Cone { radius: 0.010, height: len }
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(rot)
            .translated_by(at);
        parts.push(tinted(b, lin(GRASS_DRY)));
        if i == 1 {
            // Frost crystal clinging to the tallest blade's tip.
            parts.push(ball_at(0.016, at + rot * y(len), 0.9, SNOW_CAP_HI));
        }
    }
    shaded(merged(parts))
}

/// An ice glint — a low angular ice slab (tilted squashed chunk) with two tiny shard
/// spikes leaning off it and a bright sparkle nub. Sits essentially flat at y≈0.
fn build_ice_glint_mesh() -> Mesh {
    let mut parts = vec![
        chunk_at(0.06, y(0.015), Vec3::new(1.3, 0.3, 1.0), 0.7, 0.0, 0, ICE_PALE),
        ball_at(0.022, Vec3::new(0.045, 0.025, 0.01), 0.6, SNOW_CAP_HI),
    ];
    for (a, h) in [(0.9_f32, 0.07_f32), (3.8, 0.05)] {
        let shard = Cone { radius: 0.014, height: h }
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(h * 0.5))
            .rotated_by(Quat::from_rotation_y(a) * Quat::from_rotation_z(0.25))
            .translated_by(Vec3::new(a.cos() * 0.035, 0.0, a.sin() * 0.035));
        parts.push(tinted(shard, lin(ICE_RIM)));
    }
    shaded(merged(parts))
}

/// Winter ground litter (cover). `variant`: 0 = a holly sprig (angular dark leaves + red
/// berries + a snow dab), 1 = a toppled snow-dusted pinecone with a ring of scales, 2 = a
/// pale-blue ice-shard cluster (one shard catching the light), 3 = a frosted fallen twig.
/// Very low (≤0.15u), base at y=0 — the colourful little touches dressing the snowfield.
fn build_winter_litter_mesh(variant: u32) -> Mesh {
    let tau = std::f32::consts::TAU;
    match variant % 4 {
        // Holly sprig — five angular tip-tilted leaves fanned around four berries.
        0 => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..5 {
                let a = i as f32 / 5.0 * tau;
                parts.push(chunk_at(
                    0.045,
                    Vec3::new(a.cos() * 0.06, 0.032, a.sin() * 0.06),
                    Vec3::new(1.5, 0.30, 0.8), // long flat leaf blade
                    -a,
                    0.25, // tips tilt up out of the snow
                    0,
                    HOLLY_LEAF,
                ));
            }
            for (bx, bz) in [(0.0_f32, 0.0_f32), (0.032, -0.01), (-0.02, 0.026), (0.005, -0.034)] {
                parts.push(ball_at(0.020, Vec3::new(bx, 0.055, bz), 0.9, HOLLY_BERRY));
            }
            parts.push(ball_at(0.04, Vec3::new(0.055, 0.02, -0.045), 0.4, SNOW_CAP_HI));
            shaded(merged(parts))
        }
        // Toppled pinecone — a fat lying cone (tip +X) ringed by scale-balls at the butt,
        // snow piled along its upper flank.
        1 => {
            let mut parts = vec![tinted(
                Cone { radius: 0.045, height: 0.13 }
                    .mesh()
                    .resolution(6)
                    .build()
                    .rotated_by(Quat::from_rotation_z(-FRAC_PI_2)) // tip the apex to +X
                    .translated_by(y(0.045)),
                lin(PINECONE_BROWN),
            )];
            for i in 0..5 {
                let a = i as f32 / 5.0 * tau + 0.3;
                parts.push(ball_at(
                    0.020,
                    Vec3::new(-0.035, 0.045 + a.sin() * 0.034, a.cos() * 0.034),
                    0.9,
                    PINECONE_BROWN,
                ));
            }
            parts.push(ball_at(0.030, Vec3::new(0.0, 0.085, 0.0), 0.5, SNOW_CAP_HI));
            parts.push(ball_at(0.024, Vec3::new(-0.055, 0.06, 0.01), 0.6, SNOW_CAP));
            shaded(merged(parts))
        }
        // Ice-shard cluster — four leaning pointed cones, one bright (catching the sun).
        2 => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..4 {
                let a = i as f32 / 4.0 * tau + 0.4;
                let h = 0.09 + (i % 2) as f32 * 0.06;
                let shard = Cone { radius: 0.020, height: h }
                    .mesh()
                    .resolution(4)
                    .build()
                    .translated_by(y(h * 0.5))
                    .rotated_by(Quat::from_rotation_y(a) * Quat::from_rotation_z(0.22))
                    .translated_by(Vec3::new(a.cos() * 0.04, 0.0, a.sin() * 0.04));
                parts.push(tinted(shard, lin(if i == 0 { SNOW_CAP_HI } else { ICE_PALE })));
            }
            parts.push(ball_at(0.035, y(0.02), 0.4, SNOW_CAP_HI));
            shaded(merged(parts))
        }
        // Frosted fallen twig — a thin lying branch with one stub fork, snow dusted along
        // its top.
        _ => {
            let lyaw = 0.7_f32;
            let dir = Vec3::new(lyaw.cos(), 0.0, -lyaw.sin());
            let mut parts = vec![tinted(
                Cylinder::new(0.013, 0.26)
                    .mesh()
                    .resolution(5)
                    .build()
                    .rotated_by(Quat::from_rotation_z(FRAC_PI_2)) // lie the axis flat
                    .rotated_by(Quat::from_rotation_y(lyaw))
                    .translated_by(y(0.013)),
                lin(BIRCH_TWIG),
            )];
            let fork = Cone { radius: 0.010, height: 0.09 }
                .mesh()
                .resolution(4)
                .build()
                .translated_by(y(0.045))
                .rotated_by(Quat::from_rotation_y(lyaw + 0.5) * Quat::from_rotation_z(1.05))
                .translated_by(dir * 0.05 + y(0.012));
            parts.push(tinted(fork, lin(BIRCH_TWIG)));
            for t in [-0.08_f32, 0.0, 0.09] {
                parts.push(ball_at(0.016, dir * t + y(0.028), 0.6, SNOW_CAP_HI));
            }
            shaded(merged(parts))
        }
    }
}

// ── A snow-laden bare dead tree for the pond ring ────────────────────────────────
//
// A kinked grey two-segment trunk snapped off in a splintered tip spike, a dark hollow
// scar low on the bole, five up-angled broken branches each loaded with two snow dabs,
// one icicle hanging from the lowest branch's elbow, and a drift banked around the
// roots. Base at y=0. ~1.35u tall.
fn build_dead_snow_tree_mesh() -> Mesh {
    let mut parts = vec![
        cyl_up(0.075, 0.66, 0.33, 6, BIRCH_TWIG),
        // Leaning upper segment + the splintered snapped-off tip.
        tinted(
            Cylinder::new(0.058, 0.46)
                .mesh()
                .resolution(6)
                .build()
                .translated_by(y(0.23))
                .rotated_by(Quat::from_rotation_z(-0.14))
                .translated_by(y(0.62)),
            lin(BIRCH_TWIG),
        ),
        tinted(
            Cone { radius: 0.055, height: 0.22 }
                .mesh()
                .resolution(5)
                .build()
                .translated_by(y(0.11))
                .rotated_by(Quat::from_rotation_z(-0.14))
                .translated_by(Vec3::new(0.064, 1.07, 0.0)),
            lin(BIRCH_TWIG),
        ),
        // Dark hollow scar low on the bole.
        tinted(
            Cuboid::new(0.012, 0.10, 0.06).mesh().build().translated_by(Vec3::new(0.072, 0.30, 0.01)),
            lin(BIRCH_MARK),
        ),
    ];

    // Five angled broken branches, each loaded with two snow dabs along its upper side.
    let branches = [
        (0.0_f32, -0.85_f32, 0.50_f32, Vec3::new(0.07, 0.74, 0.02)),
        (1.5, 0.70, 0.42, Vec3::new(-0.05, 0.88, -0.02)),
        (2.9, 0.52, 0.36, Vec3::new(0.05, 0.98, 0.03)),
        (4.3, -0.60, 0.32, Vec3::new(0.02, 1.06, -0.03)),
        (5.5, 0.78, 0.26, Vec3::new(0.06, 0.55, 0.0)),
    ];
    for (i, (byaw, tilt, len, root)) in branches.into_iter().enumerate() {
        let rot = Quat::from_rotation_y(byaw) * Quat::from_rotation_z(tilt);
        let m = Cone { radius: 0.024, height: len }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(rot)
            .translated_by(root);
        parts.push(tinted(m, lin(BIRCH_TWIG)));
        parts.push(ball_at(0.06, root + rot * y(len * 0.45), 0.5, SNOW_CAP_HI));
        parts.push(ball_at(0.045, root + rot * y(len * 0.8), 0.5, SNOW_CAP));
        if i == 4 {
            // An icicle hanging from the lowest branch's elbow (tip stays well above y=0).
            parts.push(icicle(0.016, 0.12, root + rot * y(len * 0.5) - y(0.02)));
        }
    }

    // Drift banked around the roots + snow caught on the snapped tip.
    parts.push(ball_at(0.18, Vec3::new(0.06, 0.06, 0.02), 0.42, SNOW_CAP));
    parts.push(ball_at(0.12, Vec3::new(-0.16, 0.05, -0.06), 0.42, SNOW_SHADE));
    parts.push(ball_at(0.07, Vec3::new(0.066, 1.27, 0.0), 0.5, SNOW_CAP_HI));

    shaded(merged(parts))
}

// ── config() ─────────────────────────────────────────────────────────────────────

pub fn config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Snow,
        name: "Snow",

        ground_color: SNOW_GROUND,
        ground_roughness: 0.82,
        detail: GroundDetail {
            // A touch stronger than the original near-flat setting: faint blue-grey
            // shadow drift + wind streaks so the field reads broad but not dead-flat.
            scale: 0.14,
            strength: 0.30,
            variation: 0.42,
            seed: 4.0,
            dark: SNOW_GROUND_DARK,
            base: SNOW_GROUND,
            light: SNOW_GROUND_LIGHT,
            grain: 0.30,
            streak: 0.32,
        },

        // Bright winter daylight built on the warm/cool split that sells stylized snow:
        // a decisively COOL blue ambient (so shadowed snow goes blue, not grey-cream)
        // under a slightly warmer sun, + denser cool fog so peaks fade into pale haze.
        sky: 0xd4e6fb,
        fog_density: 0.013,
        sun_color: 0xfaf0e6, // near-white cold glare off the snow
        sun_illuminance: 12_000.0,
        ambient_color: 0xb6d2f7, // decisively cold-blue shadow fill
        ambient_brightness: 128.0,
        sun_pos: Vec3::new(18.0, 42.0, 12.0),

        seed: 4127,
        tree_min_dist: 2.9,
        classes: vec![
            // Trees: 78% snow-laden conifer (heavy / light / tall-spire) / 22% bare birch.
            PropClass {
                variants: vec![
                    (build_pine_mesh(0), 0.30), // broad, heavy snow load
                    (build_pine_mesh(1), 0.26), // standard, light dusting
                    (build_pine_mesh(2), 0.22), // tall slim 5-tier spire
                    (build_birch_mesh(), 0.22),
                ],
                chance: 0.072,
                scale: (0.85 * TREE_SCALE, 1.25 * TREE_SCALE),
                tree: true,
                block_radius: 0.0,
            },
            // Snow drift / frozen shrub / stump+log — FIRST non-tree class (the
            // tree-too-close fallback).
            PropClass {
                variants: (0..3).map(|v| (build_mound_mesh(v), 1.0)).collect(),
                chance: 0.07,
                scale: (0.8, 1.45),
                tree: false,
                block_radius: 0.0,
            },
            // Frost boulders (slab / split tor / low huddle).
            PropClass {
                variants: (0..3).map(|v| (build_boulder_mesh(v), 1.0)).collect(),
                chance: 0.034,
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
            // Snow tufts (with winter grass) everywhere; sparser ice glints.
            PropClass {
                variants: vec![(build_snow_tuft_mesh(), 1.0)],
                chance: 0.34,
                scale: (0.55, 1.1),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_ice_glint_mesh(), 1.0)],
                chance: 0.13,
                scale: (0.6, 1.2),
                tree: false,
                block_radius: 0.0,
            },
            // Winter litter — holly, toppled pinecone, ice shards, frosted twig.
            PropClass {
                variants: (0..4).map(|v| (build_winter_litter_mesh(v), 1.0)).collect(),
                chance: 0.13,
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
/// sky via IBL), ringed by a darker frosted rim, snow-laden dead trees, shoreline
/// ice-shard clumps + a drift tongue, and an angular snow-capped stone cairn. All
/// entities tagged `BiomeEntity` so a biome switch wipes them.
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
    // peeks out as a frozen shoreline lip (uses the shared vertex-colour material; plain
    // flat_shaded — the bake is meaningless on a flat ground disc).
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

    // ── Shoreline dressing: ice-shard clumps cracked up through the rim ice + a
    // wind-sculpted drift tongue spilling onto the shore (reuses the scatter builders).
    let shard_clump = meshes.add(build_winter_litter_mesh(2));
    for (i, &a) in [1.2_f32, 3.3, 5.0].iter().enumerate() {
        let rr = pond_r * (0.88 + 0.06 * i as f32);
        commands.spawn((
            Mesh3d(shard_clump.clone()),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: pond + Vec3::new(a.cos() * rr, 0.05, a.sin() * rr),
                rotation: Quat::from_rotation_y(a * 2.1),
                scale: Vec3::splat(1.6 - 0.2 * i as f32),
            },
            BiomeEntity,
        ));
    }
    commands.spawn((
        Mesh3d(meshes.add(build_mound_mesh(0))),
        MeshMaterial3d(mat.clone()),
        Transform {
            translation: pond + Vec3::new(-pond_r * 0.7, 0.0, pond_r * 0.95),
            rotation: Quat::from_rotation_y(2.3),
            scale: Vec3::splat(1.5),
        },
        BiomeEntity,
    ));

    // ── A small rock cairn beside the pond — four stacked ANGULAR frost stones (tilted
    // chunks, dark footing → lit upper), a bright snow cap on the top stone, dabs on the
    // shoulders, and an icicle under the second stone's lip.
    let cairn = {
        let mut parts = vec![
            chunk_at(0.34, y(0.24), Vec3::new(1.2, 0.8, 1.05), 0.2, 0.08, 1, ROCK_DARK),
            chunk_at(0.27, Vec3::new(0.03, 0.60, -0.02), Vec3::new(1.15, 0.75, 1.0), 1.1, -0.12, 1, CAIRN_STONE),
            chunk_at(0.21, Vec3::new(-0.02, 0.88, 0.03), Vec3::new(1.1, 0.7, 1.0), 2.0, 0.10, 0, ROCK_LIGHT),
            chunk_at(0.15, Vec3::new(0.02, 1.10, 0.0), Vec3::new(1.05, 0.8, 1.0), 2.8, -0.08, 0, CAIRN_STONE),
            // Bright snow cap on the top stone + dabs on the shoulders below.
            ball_at(0.14, y(1.26), 0.5, SNOW_CAP_HI),
            ball_at(0.11, Vec3::new(0.17, 0.72, 0.05), 0.5, SNOW_CAP),
            ball_at(0.09, Vec3::new(-0.15, 0.96, -0.06), 0.5, SNOW_SHADE),
        ];
        parts.push(icicle(0.018, 0.14, Vec3::new(0.30, 0.52, 0.05)));
        shaded(merged(parts))
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
