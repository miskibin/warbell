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

// ── Trilithon / rocky landmark (baked shadow → pale cap, like biome_rocky) ───
const STONE_DARK: u32 = 0x6b6358; // shadowed foot / crevice wedge
const STONE_BODY: u32 = 0x8a7f70; // mid grey-brown body
const STONE_PALE: u32 = 0xb3a692; // sun-bleached top facet
const STONE_COOL: u32 = 0x7d7a72; // cooler accent lump
const STONE_A: u32 = 0x9a9aa3; // left upright — cool light grey
const STONE_B: u32 = 0x8a8f98; // lintel — mid grey
const STONE_C: u32 = 0xaab0b2; // right upright — pale warm grey
const STONE_CAP: u32 = 0x6b6862; // shaded underside
const LICHEN_ORANGE: u32 = 0xc88a3a;
const LICHEN_SAGE: u32 = 0x9aa56b;
const PEBBLE_WARM: u32 = 0x8a7a64;

// ── Frozen spire (angular ice chunks, biome_snow family) ─────────────────────
const ICE_BODY: u32 = 0xa9d2f0;
const ICE_RIME: u32 = 0xc4e3f6;
const ICE_PALE: u32 = 0xe6f4fc;
const ICE_DEEP: u32 = 0x5f93cc;
const ICE_RIM: u32 = 0x9cc3e0;
const FROST_ROCK: u32 = 0x66727f; // blue-grey rubble the crystals erupt from

// ── Sunken pyramid (banded sandstone facets, biome_desert family) ─────────────
const SAND_BODY: u32 = 0xd9c08a;
const SAND_LT: u32 = 0xe9d4a2;
const SAND_DK: u32 = 0xbb9c5e;
const SAND_SHADOW: u32 = 0x8c7340;
const SAND_DOOR: u32 = 0x2c2418;
const HOODOO_BASE: u32 = 0x9c7d59; // warm ochre sandstone (desert tiers)
const HOODOO_RUST: u32 = 0x8a5c3c;
const HOODOO_PALE: u32 = 0xc6a978;
const SNOW_CAP: u32 = 0xe8f2fa; // frost dusting on the ice-footing rocks
const SNOW_SHADE: u32 = 0xc8dff0;

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

/// A low-poly faceted lump (ico detail 0) — the angular chipped-stone look.
fn facet_at(r: f32, off: Vec3, squash: f32, c: u32) -> Mesh {
    facet_tinted(r, off, squash, lin(c))
}

/// Like [`facet_at`] but with an explicit linear RGBA (for baked shadow/highlight scales).
fn facet_tinted(r: f32, off: Vec3, squash: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        c,
    )
}

/// A faceted lump stretched on X/Y/Z and Z-tilted into a jagged block.
fn block_at(rx: f32, ry: f32, rz: f32, off: Vec3, tilt: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(1.0)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(rx, ry, rz))
            .rotated_by(Quat::from_rotation_z(tilt))
            .translated_by(off),
        lin(c),
    )
}

/// `block_at` with an extra yaw so fracture slabs can lean in any direction.
fn slab_at(rx: f32, ry: f32, rz: f32, off: Vec3, yaw: f32, tilt: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(1.0)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(rx, ry, rz))
            .rotated_by(Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt))
            .translated_by(off),
        lin(c),
    )
}

/// Centre height that grounds a Z-tilted slab's lowest point at y=0.
fn slab_ground(rx: f32, ry: f32, tilt: f32) -> f32 {
    ((rx * tilt.sin()).powi(2) + (ry * tilt.cos()).powi(2)).sqrt()
}

/// A flat lichen splotch pressed onto a stone surface.
fn lichen_at(r: f32, off: Vec3, c: u32) -> Mesh {
    facet_at(r, off, 0.24, c)
}

/// An upright cylinder whose centre sits at `cy` (a part rooted at y=0 uses `cy = h/2`).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(yv(cy)), lin(c))
}

/// An angular rock chunk: squashed icosphere, yawed + pitched (biome_snow `chunk_at`).
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

/// A flat cairn stone — thin cuboid slab yawed about Y.
fn flat_stone(w: f32, h: f32, d: f32, off: Vec3, yaw: f32, c: u32) -> Mesh {
    tinted(
        Cuboid::new(w, h, d)
            .mesh()
            .build()
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(off),
        lin(c),
    )
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

/// One arch upright: two counter-leaning fracture slabs + stacked blocks + pale crown
/// (the `biome_rocky` leaning-slab crag recipe, not smooth cylinders).
fn crag_pillar(x: f32, total_h: f32) -> Vec<Mesh> {
    let r = 0.48;
    let (mx, my, mt) = (r * 1.22, total_h * 0.56, 0.28);
    let (cx, cy, ct) = (r * 1.02, total_h * 0.52, -0.34);
    vec![
        facet_at(r * 0.55, Vec3::new(x, r * 0.24, 0.06), 0.84, STONE_DARK),
        slab_at(
            mx,
            my,
            r * 0.92,
            Vec3::new(x - r * 0.12, slab_ground(mx, my, mt) + total_h * 0.20, 0.02),
            0.12,
            mt,
            STONE_BODY,
        ),
        slab_at(
            cx,
            cy,
            r * 0.85,
            Vec3::new(x + r * 0.22, slab_ground(cx, cy, ct) + total_h * 0.22, -r * 0.08),
            -0.18,
            ct,
            STONE_COOL,
        ),
        facet_tinted(r * 0.40, Vec3::new(x + r * 0.06, total_h * 0.50, r * 0.12), 0.86, lin_scaled(STONE_DARK, 0.85)),
        block_at(r * 0.76, total_h * 0.30, r * 0.68, Vec3::new(x + r * 0.06, total_h * 0.66, -0.04), -0.10, STONE_A),
        block_at(r * 0.68, total_h * 0.42, r * 0.62, Vec3::new(x, total_h * 0.48, 0.02), 0.03, STONE_BODY),
        block_at(r * 0.60, total_h * 0.26, r * 0.54, Vec3::new(x - r * 0.04, total_h * 0.90, 0.05), 0.08, STONE_C),
        facet_at(r * 0.44, Vec3::new(x + r * 0.05, total_h * 1.02, -0.02), 0.46, STONE_PALE),
        facet_tinted(r * 0.30, Vec3::new(x - r * 0.22, total_h * 0.76, r * 0.26), 0.48, lin_scaled(STONE_PALE, 0.94)),
        lichen_at(0.16, Vec3::new(x - r * 0.78, total_h * 0.30, r * 0.32), LICHEN_ORANGE),
        lichen_at(0.13, Vec3::new(x + r * 0.68, total_h * 0.56, -r * 0.18), LICHEN_SAGE),
    ]
}

/// A small loose crag cluster (two leaning slabs + cobbles) for the outer ring / talus.
fn mini_crag(cx: f32, cz: f32, scale: f32, yaw: f32) -> Vec<Mesh> {
    let r = 0.38 * scale;
    let (sx, sy, st) = (r * 1.15, r * 0.72, 0.32);
    vec![
        slab_at(
            sx,
            sy,
            r * 0.88,
            Vec3::new(cx - r * 0.08, slab_ground(sx, sy, st), cz),
            yaw,
            st,
            STONE_BODY,
        ),
        slab_at(
            r * 0.92,
            r * 0.58,
            r * 0.75,
            Vec3::new(cx + r * 0.42, slab_ground(r * 0.92, r * 0.58, -0.28), cz - r * 0.06),
            yaw + 0.6,
            -0.28,
            STONE_COOL,
        ),
        facet_at(r * 0.34, Vec3::new(cx + r * 0.12, r * 1.18, cz + 0.04), 0.44, STONE_PALE),
        facet_at(r * 0.22, Vec3::new(cx - r * 0.55, r * 0.20, cz + r * 0.35), 0.66, PEBBLE_WARM),
        facet_tinted(r * 0.18, Vec3::new(cx + r * 0.48, r * 0.16, cz - r * 0.32), 0.66, lin_scaled(STONE_DARK, 1.05)),
    ]
}

/// **The Standing Stones** — a natural rock arch: two fractured crag pillars bridged by a
/// thick faceted lintel (ported from `biome_rocky::landmarks`), three outer crag clusters,
/// and a talus skirt. No plinth. Base at y=0, ~3.2u to the lintel crown; opening faces ±Z.
pub fn build_trilithon_mesh() -> Mesh {
    const GAP: f32 = 2.0;
    const HALF: f32 = GAP * 0.5;
    const POST_H: f32 = 2.65;

    let mut parts: Vec<Mesh> = Vec::new();
    for p in crag_pillar(-HALF, POST_H) {
        parts.push(p);
    }
    for p in crag_pillar(HALF, POST_H) {
        parts.push(p);
    }

    // Lintel span — same layered block recipe as the rocky biome's rock arch.
    let span = GAP;
    let lintel_y = POST_H + 0.16;
    parts.push(block_at(
        span * 0.34,
        0.55,
        0.40,
        Vec3::new(-span * 0.28, lintel_y, 0.0),
        -0.10,
        STONE_B,
    ));
    parts.push(block_at(
        span * 0.34,
        0.55,
        0.40,
        Vec3::new(span * 0.28, lintel_y, 0.0),
        0.10,
        STONE_B,
    ));
    parts.push(block_at(span * 0.16, 0.50, 0.38, Vec3::new(0.0, lintel_y + 0.30, 0.0), 0.0, STONE_PALE));
    parts.push(block_at(span * 0.50, 0.22, 0.34, Vec3::new(0.0, lintel_y - 0.05, 0.0), 0.0, STONE_CAP));
    parts.push(facet_tinted(0.48, Vec3::new(-0.68, lintel_y - 0.42, 0.10), 0.76, lin_scaled(STONE_DARK, 0.9)));
    parts.push(facet_tinted(0.40, Vec3::new(0.88, lintel_y - 0.38, -0.08), 0.76, lin_scaled(STONE_BODY, 0.88)));
    parts.push(slab_at(0.52, 0.28, 0.42, Vec3::new(-0.35, 0.28, 0.55), 0.65, 0.16, STONE_COOL));
    parts.push(facet_at(0.34, Vec3::new(0.65, 0.22, -0.48), 0.62, PEBBLE_WARM));
    parts.push(facet_tinted(0.20, Vec3::new(0.08, 0.13, 0.82), 0.58, lin_scaled(STONE_BODY, 0.95)));

    // Three big outer crags (not five skinny duplicates).
    for (i, &(sc, yaw_off)) in [(1.0_f32, 0.0), (1.15, 1.1), (0.92, -0.8)].iter().enumerate() {
        let a = i as f32 * (TAU / 3.0) + 0.65;
        let (rx, rz) = (a.cos() * 2.85, a.sin() * 2.85);
        for p in mini_crag(rx, rz, sc, a + yaw_off) {
            parts.push(p);
        }
    }

    // Talus skirt under the arch.
    for (i, &(dx, dz)) in [
        (-1.35_f32, 0.70),
        (1.30, -0.60),
        (0.10, 0.95),
        (-0.85, -0.88),
        (1.55, 0.30),
        (-1.55, -0.25),
    ]
    .iter()
    .enumerate()
    {
        let r = 0.16 + (i % 3) as f32 * 0.05;
        parts.push(facet_at(
            r,
            Vec3::new(dx, r * 0.58, dz),
            0.66,
            if i % 2 == 0 { STONE_COOL } else { PEBBLE_WARM },
        ));
    }

    mottle(flat_shaded(merged(parts)), 0.62)
}

// ── Frozen spire (snow landmark) ──────────────────────────────────────────────

/// A slim hanging icicle rooted at `root` (tip stays above y=0).
fn icicle(r: f32, len: f32, root: Vec3) -> Mesh {
    tinted(
        Cone { radius: r, height: len }
            .mesh()
            .resolution(4)
            .build()
            .rotated_by(Quat::from_rotation_x(PI))
            .translated_by(root - yv(len * 0.5)),
        lin(ICE_PALE),
    )
}

/// **The Frozen Spire** — a banded ice hoodoo (wind-carved drums + protruding pale ledges)
/// erupting from a frost-boulder footing with snow dusting and icicles. No snow-dome plinth.
/// ~3.8u tall, base at y=0.
pub fn build_frozen_spire_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();

    // Frost-boulder footing — split tor + snow dusting (biome_snow boulder recipe).
    parts.push(chunk_at(0.32, yv(0.16), Vec3::new(1.2, 0.72, 1.05), 0.12, 0.08, 0, FROST_ROCK));
    parts.push(chunk_at(0.24, Vec3::new(0.10, 0.52, -0.05), Vec3::new(1.15, 0.88, 0.95), 0.75, -0.20, 0, FROST_ROCK));
    parts.push(facet_tinted(0.14, Vec3::new(-0.18, 0.50, 0.12), 0.48, lin_scaled(FROST_ROCK, 1.08)));
    parts.push(ball(0.24, yv(0.46), 0.44, SNOW_CAP));
    parts.push(ball(0.16, Vec3::new(0.18, 0.38, 0.14), 0.45, SNOW_CAP));
    parts.push(ball(0.13, Vec3::new(-0.20, 0.34, -0.14), 0.48, SNOW_SHADE));

    // Central spire — stacked angular ice chunks (detail 1) tapering to a crown shard.
    let bands: [(f32, f32, u32, f32); 6] = [
        (0.40, 0.48, ICE_DEEP, 0.00),
        (0.34, 0.44, ICE_BODY, 0.06),
        (0.28, 0.40, ICE_RIME, 0.12),
        (0.22, 0.36, ICE_BODY, 0.20),
        (0.17, 0.32, ICE_RIME, 0.28),
        (0.12, 0.28, ICE_PALE, 0.34),
    ];
    let mut cy = 0.10;
    for (i, &(r, h, c, drift)) in bands.iter().enumerate() {
        parts.push(chunk_at(
            r,
            Vec3::new(drift * 0.06, cy + h * 0.5, drift * 0.04),
            Vec3::new(1.0, h * 2.2, 0.92),
            drift,
            0.06,
            1,
            c,
        ));
        cy += h;
        if i == 1 || i == 3 {
            parts.push(facet_at(r * 0.62, Vec3::new(drift * 0.08, cy, drift * 0.05), 0.38, ICE_PALE));
        }
    }
    parts.push(chunk_at(
        0.14,
        Vec3::new(0.14, cy + 0.28, 0.05),
        Vec3::new(0.8, 1.5, 0.8),
        0.55,
        -0.14,
        1,
        ICE_PALE,
    ));
    // Bright sunlit facets on the windward flanks.
    for i in 0..3 {
        let a = i as f32 * (TAU / 3.0) + 0.25;
        parts.push(facet_at(0.14, Vec3::new(a.cos() * 0.34, cy * 0.55, a.sin() * 0.34), 0.42, ICE_PALE));
    }
    parts.push(icicle(0.022, 0.14, Vec3::new(0.34, 0.38, 0.06)));
    parts.push(icicle(0.016, 0.10, Vec3::new(-0.28, 0.34, -0.08)));

    // Flanking shards — leaning ice slabs + tip splinters.
    for i in 0..5 {
        let a = i as f32 * (TAU / 5.0) + 0.35;
        let (sx, sz) = (a.cos() * 0.82, a.sin() * 0.82);
        let h = 0.90 + (i % 3) as f32 * 0.30;
        let c = if i % 2 == 0 { ICE_BODY } else { ICE_DEEP };
        parts.push(slab_at(
            0.22,
            h,
            0.18,
            Vec3::new(sx, slab_ground(0.22, h, 0.30) + 0.02, sz),
            a,
            0.30,
            c,
        ));
        parts.push(chunk_at(
            0.10,
            Vec3::new(sx + a.cos() * 0.06, h * 0.92, sz + a.sin() * 0.06),
            Vec3::new(0.75, 1.15, 0.75),
            a + 0.35,
            -0.18,
            0,
            ICE_PALE,
        ));
    }

    // Shed ice chips at the foot.
    for &(dx, dz) in &[(0.55_f32, 0.42), (-0.48, -0.38), (0.62, -0.52), (-0.58, 0.45)] {
        parts.push(facet_at(0.08, Vec3::new(dx, 0.05, dz), 0.62, ICE_RIM));
    }

    mottle(flat_shaded(merged(parts)), 0.34)
}

// ── Sunken pyramid (desert landmark) ──────────────────────────────────────────

/// One complete ziggurat step: 8 perimeter slabs + centre fill + sunlit rim facets + front tread.
fn pyramid_tier(y: f32, w: f32, th: f32, body: u32, lit: u32) -> Vec<Mesh> {
    let mut p = Vec::new();
    if y > 0.01 {
        p.push(facet_at(w * 0.50, yv(y + 0.035), 0.28, SAND_SHADOW));
    }
    for i in 0..8 {
        let a = i as f32 * (TAU / 8.0) + 0.18;
        let (cx, cz) = (a.cos() * w * 0.36, a.sin() * w * 0.36);
        let tilt = if i % 2 == 0 { 0.05 } else { -0.04 };
        p.push(slab_at(w * 0.26, th, w * 0.22, Vec3::new(cx, y + th * 0.5, cz), a, tilt, body));
    }
    p.push(block_at(w * 0.40, th * 0.94, w * 0.38, yv(y + th * 0.48), 0.02, body));
    for i in 0..4 {
        let a = i as f32 * FRAC_PI_2 + FRAC_PI_4;
        let (cx, cz) = (a.cos() * w * 0.20, a.sin() * w * 0.20);
        p.push(block_at(w * 0.18, th * 0.82, w * 0.16, Vec3::new(cx, y + th * 0.46, cz), 0.04, body));
    }
    for i in 0..4 {
        let a = i as f32 * FRAC_PI_2 + 0.35;
        p.push(facet_at(
            w * 0.20,
            Vec3::new(a.cos() * w * 0.38, y + th - 0.025, a.sin() * w * 0.38),
            0.22,
            lit,
        ));
    }
    // Front staircase — three flat treads climbing the +Z face.
    for k in 0..3 {
        let t = k as f32 / 2.0;
        p.push(flat_stone(
            0.58 - t * 0.08,
            th * 0.07,
            0.38,
            Vec3::new(0.0, y + th * (0.32 + t * 0.22), w * 0.46 + k as f32 * 0.04),
            0.0,
            lit,
        ));
    }
    p
}

/// **The Sunken Pyramid** — a weathered stepped sandstone ziggurat: five solid tiers (8-slab
/// rings + infill per step), banded ochre/rust courses, a doorwayed summit cluster, half-buried
/// sand pebbles at the foot, and a toppled obelisk. No blank platform. ~3.7u tall, base at y=0.
pub fn build_sunken_pyramid_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();

    // Half-buried sand — scattered pebbles + low drifts hugging the lowest course.
    for &(dx, dz, r, c) in &[
        (1.55_f32, 1.40, 0.30, SAND_DK),
        (-1.65, 1.25, 0.26, SAND_BODY),
        (-1.35, -1.55, 0.28, SAND_DK),
        (1.45, -1.30, 0.24, SAND_BODY),
        (0.85, 1.65, 0.20, SAND_DK),
        (-1.75, -0.55, 0.22, SAND_BODY),
    ] {
        parts.push(facet_at(r, Vec3::new(dx, r * 0.48, dz), 0.54, c));
    }
    for &(dx, dz, yaw) in &[(1.4_f32, 1.2, 0.5), (-1.5, -1.1, -0.7)] {
        parts.push(slab_at(0.85, 0.10, 0.55, Vec3::new(dx, 0.06, dz), yaw, 0.06, SAND_DK));
    }

    let tiers = 5;
    let (w0, w1, th) = (3.15_f32, 0.95_f32, 0.38_f32);
    let bodies = [HOODOO_RUST, SAND_BODY, HOODOO_BASE, SAND_DK, HOODOO_PALE];
    let mut y_base = 0.0_f32;
    let mut top_w = w0;
    for i in 0..tiers {
        let t = i as f32 / (tiers - 1) as f32;
        let w = w0 + (w1 - w0) * t;
        for p in pyramid_tier(y_base, w, th, bodies[i], SAND_LT) {
            parts.push(p);
        }
        y_base += th;
        top_w = w;
    }

    // Summit temple — layered cap blocks + dark doorway recess on the +Z face.
    let tw = top_w * 0.88;
    let tht = 0.52;
    parts.push(block_at(tw * 0.44, tht, tw * 0.40, yv(y_base + tht * 0.5), 0.03, HOODOO_BASE));
    parts.push(slab_at(tw * 0.52, 0.14, tw * 0.48, yv(y_base + tht + 0.06), 0.25, 0.02, HOODOO_RUST));
    parts.push(facet_at(tw * 0.18, Vec3::new(0.0, y_base + tht * 0.40, tw * 0.47), 0.52, SAND_DOOR));
    parts.push(facet_at(tw * 0.14, Vec3::new(tw * 0.32, y_base + tht * 0.72, 0.0), 0.24, SAND_LT));
    parts.push(facet_at(tw * 0.12, Vec3::new(-tw * 0.30, y_base + tht * 0.68, -0.02), 0.24, SAND_LT));

    // Toppled obelisk — fallen slab shaft + pyramidion chip half-buried in sand.
    parts.push(slab_at(0.28, 1.50, 0.24, Vec3::new(2.15, 0.85, 0.48), 0.45, 1.18, HOODOO_RUST));
    parts.push(chunk_at(
        0.20,
        Vec3::new(2.58, 0.24, 0.55),
        Vec3::new(0.9, 0.75, 0.9),
        -0.6,
        0.2,
        0,
        HOODOO_PALE,
    ));
    parts.push(facet_at(0.26, Vec3::new(-2.15, 0.14, 0.92), 0.64, SAND_BODY));
    parts.push(facet_at(0.20, Vec3::new(-2.40, 0.11, -0.62), 0.64, SAND_DK));
    parts.push(facet_at(0.16, Vec3::new(0.80, 0.09, -1.48), 0.58, SAND_DK));

    mottle(flat_shaded(merged(parts)), 0.60)
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
