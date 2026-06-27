//! Landmark props — the five signature biome set-pieces (sunken pyramid, frozen spire,
//! standing-stone circle, giant dead tree, swamp sentinel) the world map plants one of in
//! each biome region. Built as single merged vertex-coloured meshes (one per landmark) so
//! they batch against the scene's shared white material; each model's BASE sits at y=0 and
//! the placement pass scales/seats it on FLAT ground.
//!
//! Build contract (per the verified-APIs doc §9): every part is a primitive
//! `tinted` with a flat linear `ATTRIBUTE_COLOR` BEFORE the merge (the scene's one white
//! `StandardMaterial` reads colour straight off the vertices), merged via `Mesh::merge`, then
//! `flat_shaded` (`duplicate_vertices` → `compute_flat_normals`) for the crisp low-poly facets
//! the rest of the scene uses. `duplicate_vertices` MUST precede `compute_flat_normals`.

use bevy::prelude::*;
use bevy::mesh::VertexAttributeValues;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use crate::palette::{lin, lin_scaled, DEAD_WOOD, DEAD_WOOD_DARK};

// ── Trilithon stone tints (mottled weathered granite greys) ──────────────────
const STONE_A: u32 = 0x9a9aa3; // left upright — cool light grey
const STONE_B: u32 = 0x8a8f98; // lintel — mid grey (the cross-beam reads darker)
const STONE_C: u32 = 0xaab0b2; // right upright — pale warm grey
const STONE_CAP: u32 = 0x6b6862; // shaded hewn caps / shadowed underside
const STONE_MOSS: u32 = 0x74803f; // subtle moss accent creeping up a base
const STONE_EARTH: u32 = 0x6a6358; // worn earthen ring the circle stands on

// ── Dead-tree wood tints ─────────────────────────────────────────────────────
const TREE_BARK: u32 = DEAD_WOOD; // 0x6e6258 weathered grey-brown
const TREE_BARK_DARK: u32 = DEAD_WOOD_DARK; // 0x4a4238 lower trunk / collar
const TREE_ROOT: u32 = 0x2f2418; // near-black wet root wood at the base

// ── Swamp sentinel tints (mossy grey-green, water-stained) ─────────────────────
const SWAMP_BARK: u32 = 0x5d6150; // grey-green mossy bark
const SWAMP_BARK_DARK: u32 = 0x3a3c30; // water-stained lower trunk
const SWAMP_ROOT: u32 = 0x241f16; // black sodden root wood
const SWAMP_MOSS: u32 = 0x6a7b3c; // hanging moss / knee caps

// ── Mesh helpers (verified 0.18 forms) ───────────────────────────────────────

/// Tag every vertex of a part with one flat linear colour. REQUIRED before merge —
/// all merged parts must carry the same attribute set, and the scene's shared white
/// `StandardMaterial` reads colour straight off `ATTRIBUTE_COLOR`.
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge a list of already-tinted parts into one mesh. `Mesh::merge` returns a
/// `Result` in 0.18 (all parts share POSITION/NORMAL/UV_0/COLOR, so it never fails).
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("parts share attributes");
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

fn yv(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// A box primitive, positioned so its CENTER lands at `center`.
fn box_at(x: f32, y: f32, z: f32, center: Vec3) -> Mesh {
    Cuboid::new(x, y, z).mesh().build().translated_by(center)
}

/// A faceted icosphere blob (ico detail 1), squashed on Y, centred at `center`.
fn ball(r: f32, center: Vec3, squash: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(1)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(center),
        lin(c),
    )
}

/// An upright cylinder centred at `center`. `res` keeps the low-poly facet look.
fn cyl(r: f32, h: f32, center: Vec3, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(center), lin(c))
}

/// A subdivided `a×b` plane facing +Y (built so it can be rotated into any box face). The
/// subdivisions give the flat face enough triangles for [`mottle`] to resolve into a grain.
fn plane(a: f32, b: f32, sub: u32) -> Mesh {
    Plane3d::new(Vec3::Y, Vec2::new(a * 0.5, b * 0.5)).mesh().subdivisions(sub).build()
}

/// A box whose six faces are each subdivided into a `sub`×`sub` grid — so a [`mottle`] pass
/// stipples the large flat surfaces with weathered grain instead of one flat colour. Centred
/// at the origin; tint + place like [`box_at`]. (Bottom face included; it's cheap and a
/// toppled/partly-buried block can show it.)
fn tile_box(w: f32, h: f32, d: f32, sub: u32) -> Mesh {
    let (hw, hh, hd) = (w * 0.5, h * 0.5, d * 0.5);
    let faces = vec![
        plane(w, d, sub).translated_by(yv(hh)),                                            // +Y top
        plane(w, d, sub).rotated_by(Quat::from_rotation_x(PI)).translated_by(yv(-hh)),     // -Y bottom
        plane(w, h, sub).rotated_by(Quat::from_rotation_x(FRAC_PI_2)).translated_by(Vec3::new(0.0, 0.0, hd)), // +Z
        plane(w, h, sub).rotated_by(Quat::from_rotation_x(-FRAC_PI_2)).translated_by(Vec3::new(0.0, 0.0, -hd)), // -Z
        plane(h, d, sub).rotated_by(Quat::from_rotation_z(-FRAC_PI_2)).translated_by(Vec3::new(hw, 0.0, 0.0)), // +X
        plane(h, d, sub).rotated_by(Quat::from_rotation_z(FRAC_PI_2)).translated_by(Vec3::new(-hw, 0.0, 0.0)), // -X
    ];
    merged(faces)
}

/// A subdivided, tinted box centred at `center` (the grainy counterpart of [`box_at`]).
fn tbox(w: f32, h: f32, d: f32, center: Vec3, sub: u32, c: u32) -> Mesh {
    tinted(tile_box(w, h, d, sub).translated_by(center), lin(c))
}

/// Deterministic [0,1) value noise from a world position (same sin-hash family as the scatter).
fn hash3(x: f32, y: f32, z: f32) -> f32 {
    let v = (x * 127.1 + y * 311.7 + z * 74.7).sin() * 43758.5453;
    v - v.floor()
}

/// **Surface weathering** — the texture pass. Run LAST (after [`flat_shaded`], which has
/// de-indexed the mesh so vertices come in per-triangle triples). Jitters each facet's colour
/// by a position-seeded noise so big stone/sand faces read mottled and grainy rather than as
/// flat plastic — the project's stand-in for a texture, staying pure vertex-colour so the
/// landmark still batches against the shared white material. `amount` ≈ peak ± fraction.
fn mottle(mut m: Mesh, amount: f32) -> Mesh {
    let pos: Vec<[f32; 3]> = match m.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.clone(),
        _ => return m,
    };
    if let Some(VertexAttributeValues::Float32x4(cols)) = m.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for t in 0..(cols.len() / 3) {
            let i = t * 3;
            // Per-facet centroid → one shade for the whole triangle (keeps crisp flat facets).
            let cx = (pos[i][0] + pos[i + 1][0] + pos[i + 2][0]) / 3.0;
            let cy = (pos[i][1] + pos[i + 1][1] + pos[i + 2][1]) / 3.0;
            let cz = (pos[i][2] + pos[i + 1][2] + pos[i + 2][2]) / 3.0;
            // Two octaves: coarse blotches (weather staining over ~1.4u patches) + fine grain.
            let coarse = hash3((cx * 0.7).floor(), (cy * 0.7).floor(), (cz * 0.7).floor());
            let fine = hash3(cx * 2.3, cy * 2.3, cz * 2.3);
            let n = coarse * 0.6 + fine * 0.4;
            // Biased a touch dark (weathering darkens more than it lightens).
            let f = 1.0 + (n - 0.58) * amount;
            for k in 0..3 {
                for ch in 0..3 {
                    cols[i + k][ch] = (cols[i + k][ch] * f).clamp(0.0, 1.0);
                }
            }
        }
    }
    m
}

/// A cylinder primitive whose BASE sits at the local origin, then optionally pitched
/// about Z and yawed about Y, then translated to its attach point. Order: lift the
/// base-centred cylinder so its base is at origin → rotate (so it swings about its
/// base) → translate to where it attaches. `resolution` keeps the low-poly facet look.
fn limb(radius: f32, height: f32, resolution: u32, pitch_z: f32, yaw_y: f32, attach: Vec3) -> Mesh {
    let rot = Quat::from_rotation_y(yaw_y) * Quat::from_rotation_z(pitch_z);
    Cylinder::new(radius, height)
        .mesh()
        .resolution(resolution)
        .build()
        .translated_by(Vec3::new(0.0, height * 0.5, 0.0)) // base → origin
        .rotated_by(rot)
        .translated_by(attach)
}

// ── Standing stones (rocky landmark) ──────────────────────────────────────────

/// **The Standing Stones** — a clean Stonehenge-style trilithon arch (two tapered
/// uprights + a lintel) standing inside an outer ring of five rough leaning monoliths,
/// all on a low worn earthen circle. Mottled weathered greys with a moss accent; base at
/// y=0, ~3u to the lintel. Walk through the arch (the opening faces ±Z; the ring leaves
/// that line clear).
pub fn build_trilithon_mesh() -> Mesh {
    const POST_W: f32 = 0.7; // upright width (X)
    const POST_D: f32 = 0.7; // upright depth (Z)
    const POST_H: f32 = 2.6; // upright bulk height
    const GAP: f32 = 2.0; // centre-to-centre spacing of the two uprights
    const HALF: f32 = GAP * 0.5; // ±1.0 in X
    const LINTEL_H: f32 = 0.45; // cross-beam thickness
    const PLINTH_H: f32 = 0.14; // low platform under both posts

    let mut parts: Vec<Mesh> = Vec::new();

    // Worn earthen ring the whole circle stands on — a low wide disc, grounds it.
    parts.push(cyl(3.9, 0.12, yv(0.06), 20, STONE_EARTH));

    // Low turf/earth plinth the uprights stand on — base at y=0.
    parts.push(tinted(
        box_at(GAP + POST_W + 0.5, PLINTH_H, POST_D + 0.5, Vec3::new(0.0, PLINTH_H * 0.5, 0.0)),
        lin(STONE_CAP),
    ));

    // The two uprights. Left = STONE_A, right = STONE_C (distinct greys). Each is a
    // tall box bulk sitting on the plinth + a narrower, darker hewn cap on top, so
    // the silhouette reads tapered/chiselled. Posts rise from y=PLINTH_H.
    for (sx, body_hex) in [(-HALF, STONE_A), (HALF, STONE_C)] {
        let base_y = PLINTH_H;
        // Slight per-stone brightness jitter so even the two uprights differ subtly.
        let v = if sx < 0.0 { 0.97 } else { 1.03 };
        parts.push(tinted(
            tile_box(POST_W, POST_H, POST_D, 4).translated_by(Vec3::new(sx, base_y + POST_H * 0.5, 0.0)),
            lin_scaled(body_hex, v),
        ));
        // Hewn cap — narrower + darker, capping the upright just under the lintel.
        parts.push(tinted(
            box_at(POST_W * 0.82, 0.18, POST_D * 0.82, Vec3::new(sx, base_y + POST_H - 0.02, 0.0)),
            lin(STONE_CAP),
        ));
    }

    // Horizontal lintel spanning both uprights' tops, with a darker shadowed underside.
    let lintel_w = GAP + POST_W + 0.3;
    let lintel_y = PLINTH_H + POST_H + LINTEL_H * 0.5 - 0.02;
    parts.push(tinted(tile_box(lintel_w, LINTEL_H, POST_D, 5).translated_by(Vec3::new(0.0, lintel_y, 0.0)), lin(STONE_B)));
    parts.push(tinted(
        box_at(lintel_w * 0.98, 0.06, POST_D * 0.7, Vec3::new(0.0, PLINTH_H + POST_H + 0.02, 0.0)),
        lin(STONE_CAP),
    ));

    // Moss creeping up the base of the left upright (thin slab on the +Z face).
    parts.push(tinted(
        box_at(POST_W * 0.6, 0.7, 0.04, Vec3::new(-HALF, PLINTH_H + 0.45, POST_D * 0.5 + 0.02)),
        lin(STONE_MOSS),
    ));

    // ── Outer ring: five rough leaning monoliths around the arch. Each is a tapered
    // bulk + a hewn cap, faced toward the centre and tilted a touch so the circle reads
    // weathered, not stamped. Angles dodge the ±Z entrance line so the arch stays walkable.
    let greys = [STONE_A, STONE_B, STONE_C, STONE_CAP, STONE_A];
    let ring_r = 3.35;
    for i in 0..5 {
        let a = i as f32 * (TAU / 5.0) + 0.55;
        let (rx, rz) = (a.cos() * ring_r, a.sin() * ring_r);
        let h = 1.5 + (i % 3) as f32 * 0.45;
        let w = 0.52 + (i % 2) as f32 * 0.12;
        let yaw = a + FRAC_PI_2; // flat face toward centre
        let tilt = if i % 2 == 0 { 0.06 } else { -0.05 };
        let lean = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt);
        let g = greys[i];
        // Tapered bulk (base at origin → lean → seat on the ring).
        parts.push(tinted(
            tile_box(w, h, w * 0.85, 4)
                .translated_by(yv(h * 0.5))
                .rotated_by(lean)
                .translated_by(Vec3::new(rx, 0.1, rz)),
            lin_scaled(g, 0.96 + (i as f32) * 0.02),
        ));
        // Hewn cap.
        parts.push(tinted(
            Cuboid::new(w * 0.8, 0.16, w * 0.7)
                .mesh()
                .build()
                .translated_by(yv(h - 0.04))
                .rotated_by(lean)
                .translated_by(Vec3::new(rx, 0.1, rz)),
            lin(STONE_CAP),
        ));
    }

    mottle(flat_shaded(merged(parts)), 0.6)
}

// ── Frozen spire (snow landmark) ──────────────────────────────────────────────

/// **The Frozen Spire** — a tall tapered ice crystal (a 6-sided prism stepping in to a
/// pale rime upper, capped by a pointed cone) ringed by five leaning flanking shards on a
/// frosted snow mound. Pale blues with bright sunlit facets; ~3.6u tall, base at y=0.
pub fn build_frozen_spire_mesh() -> Mesh {
    const ICE: u32 = 0xa9d2f0; // crystal body blue
    const RIME: u32 = 0xc4e3f6; // upper, frostier blue
    const PALE: u32 = 0xe6f4fc; // sunlit tip / facet
    const DEEP: u32 = 0x5f93cc; // shadowed shard
    const SNOW: u32 = 0xeef6fc; // mound

    let mut parts: Vec<Mesh> = Vec::new();

    // Frosted snow mound (wide squashed dome) the crystals erupt from.
    parts.push(ball(1.5, yv(0.06), 0.22, SNOW));
    parts.push(ball(0.9, Vec3::new(0.7, 0.05, -0.4), 0.3, SNOW));

    // Central crystal: a stepped 6-sided prism narrowing as it rises, then a cone tip.
    let bh = 2.3;
    parts.push(cyl(0.46, bh * 0.5, yv(0.2 + bh * 0.25), 6, ICE));
    parts.push(cyl(0.34, bh * 0.5, yv(0.2 + bh * 0.75), 6, RIME));
    parts.push(tinted(
        Cone { radius: 0.34, height: 1.0 }.mesh().resolution(6).build().translated_by(yv(0.2 + bh + 0.5)),
        lin(PALE),
    ));
    // Three bright sunlit facet ridges standing proud of the body (catch the light).
    for i in 0..3 {
        let a = i as f32 * (TAU / 3.0) + 0.3;
        parts.push(tinted(
            Cuboid::new(0.05, bh * 0.8, 0.05)
                .mesh()
                .build()
                .translated_by(Vec3::new(0.42, 0.2 + bh * 0.45, 0.0))
                .rotated_by(Quat::from_rotation_y(a)),
            lin(PALE),
        ));
    }

    // ── Flanking shards: five leaning tapered crystals (body prism + cone tip) around
    // the base, two-tone so the cluster reads as a shattered outcrop.
    let mut shard = |parts: &mut Vec<Mesh>, base: Vec3, h: f32, r: f32, lean: Quat, c: u32| {
        let body = Cylinder::new(r, h * 0.7)
            .mesh()
            .resolution(6)
            .build()
            .translated_by(yv(h * 0.35))
            .rotated_by(lean)
            .translated_by(base);
        parts.push(tinted(body, lin(c)));
        let tip = Cone { radius: r, height: h * 0.4 }
            .mesh()
            .resolution(6)
            .build()
            .translated_by(yv(h * 0.7 + h * 0.2))
            .rotated_by(lean)
            .translated_by(base);
        parts.push(tinted(tip, lin(PALE)));
    };
    for i in 0..5 {
        let a = i as f32 * (TAU / 5.0) + 0.4;
        let (sx, sz) = (a.cos() * 0.72, a.sin() * 0.72);
        let h = 1.0 + (i % 3) as f32 * 0.34;
        let lean = Quat::from_rotation_y(a) * Quat::from_rotation_z(0.30);
        shard(&mut parts, Vec3::new(sx, 0.12, sz), h, 0.2, lean, if i % 2 == 0 { ICE } else { DEEP });
    }

    mottle(flat_shaded(merged(parts)), 0.28)
}

// ── Sunken pyramid (desert landmark) ──────────────────────────────────────────

/// **The Sunken Pyramid** — a weathered stepped sandstone ziggurat: five receding tiers
/// (each with a sunlit top lip + a recessed shadow reveal under its step), a central front
/// staircase, and a doorwayed summit temple. Half-buried in a low sand drift, with a
/// toppled obelisk and rubble beside it. ~3.6u tall, base at y=0.
pub fn build_sunken_pyramid_mesh() -> Mesh {
    const SAND: u32 = 0xd9c08a; // sandstone body
    const SAND_LT: u32 = 0xe9d4a2; // sunlit ridge / stair tread
    const SAND_DK: u32 = 0xbb9c5e; // shaded course / drift
    const SHADOW: u32 = 0x8c7340; // recessed reveal under each step
    const DOOR: u32 = 0x2c2418; // dark temple doorway

    let mut parts: Vec<Mesh> = Vec::new();

    // Half-buried sand drift skirt — a low wide platform + corner dunes, hides the seam
    // and reads as the pyramid sunk into the dunes.
    parts.push(tbox(4.4, 0.18, 4.4, yv(0.09), 5, SAND_DK));
    for &(dx, dz) in &[(1.9_f32, 1.7_f32), (-2.0, 1.5), (-1.6, -1.9), (1.8, -1.6)] {
        parts.push(ball(0.9, Vec3::new(dx, 0.06, dz), 0.28, SAND_DK));
    }

    // ── Receding tiers.
    let tiers = 5;
    let (w0, w1, th) = (3.4_f32, 1.1_f32, 0.40_f32);
    let mut y = 0.14;
    let mut top_w = w0;
    for i in 0..tiers {
        let t = i as f32 / (tiers - 1) as f32;
        let w = w0 + (w1 - w0) * t;
        // Recessed shadow reveal sitting on the wider course below (skip the first, which
        // sits on the drift, not a step).
        if i > 0 {
            parts.push(tinted(box_at(w + 0.06, 0.07, w + 0.06, yv(y + 0.035)), lin(SHADOW)));
        }
        // Tier body, alternating course tone for a weathered banding (grainy faces).
        parts.push(tbox(w, th, w, yv(y + th * 0.5), 5, if i % 2 == 0 { SAND } else { SAND_DK }));
        // Sunlit top lip.
        parts.push(tinted(box_at(w, 0.05, w, yv(y + th - 0.02)), lin(SAND_LT)));
        // Central front staircase tread on the +Z face.
        parts.push(tinted(box_at(0.66, th * 0.9, 0.18, Vec3::new(0.0, y + th * 0.45, w * 0.5 + 0.03)), lin(SAND_LT)));
        y += th;
        top_w = w;
    }

    // ── Summit temple with a dark doorway facing +Z and a flat overhanging roof.
    let tw = top_w * 0.92;
    let tht = 0.58;
    parts.push(tbox(tw, tht, tw, yv(y + tht * 0.5), 4, SAND));
    parts.push(tbox(tw * 1.16, 0.12, tw * 1.16, yv(y + tht + 0.05), 2, SAND_DK));
    parts.push(tinted(box_at(tw * 0.34, tht * 0.7, 0.14, Vec3::new(0.0, y + tht * 0.4, tw * 0.5)), lin(DOOR)));

    // ── Toppled obelisk half-sunk in the sand beside the base + a couple of rubble blocks.
    let obelisk = {
        let shaft = box_at(0.34, 1.7, 0.34, yv(0.85));
        let pyramidion = Cone { radius: 0.26, height: 0.4 }.mesh().resolution(4).build().translated_by(yv(1.9));
        let mut m = shaft;
        m.merge(&pyramidion).expect("obelisk parts share attributes");
        tinted(
            m.rotated_by(Quat::from_rotation_z(1.25)).translated_by(Vec3::new(2.25, 0.18, 0.5)),
            lin(SAND_DK),
        )
    };
    parts.push(obelisk);
    parts.push(tbox(0.5, 0.32, 0.5, Vec3::new(-2.3, 0.16, 1.0), 1, SAND));
    parts.push(tbox(0.4, 0.26, 0.6, Vec3::new(-2.5, 0.13, -0.7), 1, SAND_DK));

    mottle(flat_shaded(merged(parts)), 0.6)
}

// ── Per-biome placement (combined world map) ─────────────────────────────────────

/// Plant one signature landmark in each biome region of the combined map. Reject-samples a
/// clear, on-land tile of the target biome (away from camps/castle) and — crucially — seats
/// it on the FLATTEST footprint it can find, never on a slope or with an edge over the sea, so
/// the set-piece sits cleanly on the ground instead of clipping through a terrace. Mirrors the
/// wildlife/ore placement; called from `worldmap::build`.
pub fn populate_landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    use crate::biome::{Biome, BiomeEntity};
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.92, ..default() });
    // (biome, mesh, scale, (box_hw, box_hd), footprint_radius) — one landmark each. The solid is an
    // oriented BOX (not a ≤1.0 circle) so wide set-pieces — the trilithon's standing stones, the
    // pyramid's broad base — actually block instead of letting you walk through their edges. The
    // half-extents (mesh-local, world-scaled below) hug each silhouette; footprint_radius is the
    // flatness-probe reach so the whole base lands on level ground.
    let specs: [(Biome, Mesh, f32, (f32, f32), f32); 5] = [
        (Biome::Snow, build_frozen_spire_mesh(), 1.4, (0.7, 0.7), 1.0),
        (Biome::Desert, build_sunken_pyramid_mesh(), 1.3, (1.15, 1.15), 2.0),
        (Biome::Rocky, build_trilithon_mesh(), 1.3, (1.25, 0.55), 2.0), // wide arch, thin depth
        (Biome::Forest, build_giant_dead_tree_mesh(), 1.2, (0.55, 0.55), 0.7),
        (Biome::Swamp, build_swamp_sentinel_mesh(), 1.1, (0.65, 0.65), 0.8),
    ];
    let mut rng: u32 = 0x1a2b_3c4d;
    for (biome, mesh, scale, (box_hw, box_hd), foot_r) in specs {
        let handle = meshes.add(mesh);
        let probe_r = foot_r * scale;
        // Best-of: keep the flattest candidate seen; take the first perfectly level one.
        let mut best: Option<(f32, f32, f32, f32, f32)> = None; // (spread, x, z, y, yaw)
        for _ in 0..4000 {
            let x = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GX + 6.0, crate::worldmap::GX - 6.0);
            let z = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GZ + 6.0, crate::worldmap::GZ - 6.0);
            if crate::worldmap::biome_at_world(x, z) != Some(biome)
                || crate::worldmap::ground_at_world(x, z).is_none()
                || crate::blockers::is_blocked(x, z)
                || crate::camps::in_clearing(x, z)
                || crate::castle::in_footprint(x, z)
                || crate::rival::near_fort(x, z)
            {
                continue;
            }
            // Reject any footprint that runs off-land / over water, then grade by flatness.
            let Some(spread) = footprint_spread(x, z, probe_r) else { continue };
            let y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
            let yaw = crate::wildlife::rng_range(&mut rng, 0.0, TAU);
            if best.map_or(true, |b| spread < b.0) {
                best = Some((spread, x, z, y, yaw));
            }
            if spread <= 0.01 {
                break; // dead flat — done
            }
        }
        if let Some((spread, x, z, y, yaw)) = best {
            let id = commands
                .spawn((
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(x, y, z)
                        .with_rotation(Quat::from_rotation_y(yaw))
                        .with_scale(Vec3::splat(scale)),
                    BiomeEntity,
                ))
                .id();
            // Solid oriented box, scaled into world units + turned to match the placed mesh yaw.
            crate::blockers::add_obb(x, z, box_hw * scale, box_hd * scale, yaw);
            crate::landmarks::attach(commands, id, biome, Vec3::new(x, y, z), meshes, materials);
            if spread > crate::worldmap::GROUND_STEP {
                info!("landmark {:?}: best spot still uneven (spread {:.2})", biome, spread);
            }
        } else {
            info!("landmark: no spot found for {:?}", biome);
        }
    }
}

/// Terrain height spread (max−min world-Y) over a landmark's footprint: the centre plus two
/// rings of eight samples out to `radius`. Returns `None` if ANY sample is off-land/over water
/// (so a landmark never plants with part of its base over a cliff edge or the sea). A spread of
/// 0 means dead-flat ground; `GROUND_STEP` (0.5) is one terrace step.
fn footprint_spread(x: f32, z: f32, radius: f32) -> Option<f32> {
    let mut lo = crate::worldmap::ground_at_world(x, z)?;
    let mut hi = lo;
    for i in 0..8 {
        let a = i as f32 * FRAC_PI_4;
        let (c, s) = (a.cos(), a.sin());
        for m in [radius, radius * 0.55] {
            let g = crate::worldmap::ground_at_world(x + c * m, z + s * m)?;
            lo = lo.min(g);
            hi = hi.max(g);
        }
    }
    Some(hi - lo)
}

// ── Dead trees (forest + swamp landmarks) ─────────────────────────────────────────

/// **The Hollow Oak** — a tall bare gnarled dead tree, ~5u to the branch tips, base at y=0.
pub fn build_giant_dead_tree_mesh() -> Mesh {
    mottle(flat_shaded(merged(dead_tree_parts(TREE_BARK, TREE_BARK_DARK, TREE_ROOT, None))), 0.4)
}

/// **The Mire Sentinel** — the swamp's drowned cousin of the dead tree: mossy grey-green,
/// water-stained bark, hanging moss strands off the limbs and a ring of knob-tipped cypress
/// "knees" round its sodden base. Base at y=0.
pub fn build_swamp_sentinel_mesh() -> Mesh {
    mottle(flat_shaded(merged(dead_tree_parts(SWAMP_BARK, SWAMP_BARK_DARK, SWAMP_ROOT, Some(SWAMP_MOSS)))), 0.44)
}

/// Shared dead-tree build: a dark flared root collar grounding a tapered three-segment
/// gnarled trunk, shallow flared roots, and four thick bare branches (each sprouting a
/// forked twig) high up, plus a dark hollow knot. `moss = Some(c)` makes it the swamp
/// sentinel — hanging moss strands off each branch, a moss saddle on the trunk, and a ring
/// of cypress knees at the base.
fn dead_tree_parts(bark: u32, bark_dark: u32, root: u32, moss: Option<u32>) -> Vec<Mesh> {
    let mut parts: Vec<Mesh> = Vec::new();

    // Dark flared root collar / stump the trunk grows out of (base at y=0).
    parts.push(tinted(limb(0.46, 0.55, 8, 0.0, 0.0, Vec3::ZERO), lin(bark_dark)));

    // Shallow flared roots splaying from the base — five around the trunk, laid nearly flat.
    for i in 0..5 {
        let yaw = i as f32 * TAU / 5.0 + 0.3;
        let len = 0.55 + (i as f32 * 0.07);
        parts.push(tinted(limb(0.13, len, 5, FRAC_PI_2 * 1.05, yaw, Vec3::new(0.0, 0.16, 0.0)), lin(root)));
    }

    // ── Trunk: three stacked tapered segments, each leaning a touch more for a gnarl.
    let seg_specs: [(f32, f32, f32, f32); 3] = [
        (1.5, 0.31, 0.05, 0.0),  // lower (darker)
        (1.4, 0.23, -0.08, 0.5), // mid
        (1.2, 0.15, 0.13, 0.3),  // upper (thinnest)
    ];
    let trunk_base_y = 0.4;
    let mut tip = Vec3::new(0.0, trunk_base_y, 0.0);
    let mut accum = Quat::IDENTITY;
    for (idx, &(h, r, lean_z, lean_yaw)) in seg_specs.iter().enumerate() {
        accum = accum * Quat::from_rotation_y(lean_yaw) * Quat::from_rotation_z(lean_z);
        let seg = Cylinder::new(r, h)
            .mesh()
            .resolution(6)
            .build()
            .translated_by(Vec3::new(0.0, h * 0.5, 0.0))
            .rotated_by(accum)
            .translated_by(tip);
        let hex = if idx == 0 { bark_dark } else { bark };
        parts.push(tinted(seg, lin(hex)));
        tip += accum * Vec3::new(0.0, h, 0.0);
    }

    // Dark hollow knot on the lower trunk (a recessed eye in the bark).
    parts.push(ball(0.16, Vec3::new(0.18, 1.4, 0.18), 0.85, root));

    // ── Branches: thick bare limbs forking outward/upward high on the trunk.
    let branches: [(f32, f32, f32, f32, f32); 4] = [
        (2.4, 0.4, 0.85, 1.05, 0.115),
        (2.9, 2.5, 0.75, 0.95, 0.10),
        (3.3, 4.4, 0.95, 0.90, 0.09),
        (3.6, 1.4, 0.6, 0.75, 0.075),
    ];
    for &(ay, yaw, pitch, len, r) in branches.iter() {
        let attach = Vec3::new(0.0, ay, 0.0);
        parts.push(tinted(limb(r, len, 5, -pitch, yaw, attach), lin(bark)));
        let branch_rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(-pitch);
        let tip_pt = attach + branch_rot * Vec3::new(0.0, len, 0.0);
        let twig_rot = branch_rot * Quat::from_rotation_z(0.7);
        let twig = Cylinder::new(r * 0.5, len * 0.5)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(Vec3::new(0.0, len * 0.25, 0.0))
            .rotated_by(twig_rot)
            .translated_by(tip_pt);
        parts.push(tinted(twig, lin(bark)));
        // Swamp: a strand of moss hanging straight down off the branch tip.
        if let Some(mc) = moss {
            parts.push(tinted(box_at(0.05, 0.55, 0.05, tip_pt + yv(-0.28)), lin(mc)));
        }
    }

    // ── Swamp extras: a moss saddle on the trunk + a ring of cypress knees at the base.
    if let Some(mc) = moss {
        parts.push(ball(0.34, yv(1.0), 0.5, mc));
        for i in 0..5 {
            let a = i as f32 * TAU / 5.0 + 0.7;
            let (kx, kz) = (a.cos() * 0.78, a.sin() * 0.78);
            parts.push(cyl(0.09, 0.32, Vec3::new(kx, 0.16, kz), 5, bark_dark));
            parts.push(ball(0.1, Vec3::new(kx, 0.34, kz), 0.8, mc));
        }
    }

    parts
}
