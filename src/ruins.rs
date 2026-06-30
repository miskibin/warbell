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

use crate::palette::{lin, DEAD_WOOD, DEAD_WOOD_DARK};

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
#[allow(dead_code)]
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

/// A low-poly faceted lump (ico detail 0) — the angular chipped-stone look.
fn facet_at(r: f32, off: Vec3, squash: f32, c: u32) -> Mesh {
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

/// Stacked banded drums for one arch upright — wind-resistant pale ledges + lichen at the foot.
fn arch_upright(height: f32, r0: f32, bands: &[u32]) -> Vec<Mesh> {
    let drums = 5;
    let h = height / drums as f32;
    let mut parts = Vec::new();
    for i in 0..drums {
        let t = i as f32 / drums as f32;
        let r = r0 * (1.0 - t * 0.26);
        let c = bands[i % bands.len()];
        parts.push(cyl_up(r, h * 1.03, h * (i as f32 + 0.5), 8, c));
        if i == 2 || i == 4 {
            parts.push(facet_at(r * 1.12, yv(h * (i as f32 + 1.0)), 0.2, STONE_PALE));
        }
    }
    parts.push(lichen_at(r0 * 0.18, Vec3::new(r0 * 0.75, 0.45, 0.18), LICHEN_ORANGE));
    parts.push(lichen_at(r0 * 0.14, Vec3::new(-r0 * 0.7, 0.85, -0.12), LICHEN_SAGE));
    parts
}

/// **The Standing Stones** — a natural rock arch: two banded stone pillars bridged by a
/// thick faceted lintel, ringed by five leaning monolith slabs and foot-rubble. No plinth
/// or turf disc — the set-piece grows straight out of the ground like the rocky biome's
/// crags. Base at y=0, ~3u to the lintel crown; the opening faces ±Z.
pub fn build_trilithon_mesh() -> Mesh {
    const GAP: f32 = 2.0;
    const HALF: f32 = GAP * 0.5;
    const POST_H: f32 = 2.55;
    const POST_R: f32 = 0.38;

    let mut parts: Vec<Mesh> = Vec::new();

    let left_bands = [STONE_A, STONE_BODY, STONE_PALE, STONE_B, STONE_C];
    let right_bands = [STONE_C, STONE_COOL, STONE_PALE, STONE_BODY, STONE_A];
    for (sx, bands) in [(-HALF, &left_bands[..]), (HALF, &right_bands[..])] {
        for p in arch_upright(POST_H, POST_R, bands) {
            parts.push(p.translated_by(Vec3::new(sx, 0.0, 0.0)));
        }
        parts.push(facet_at(POST_R * 0.55, Vec3::new(sx, POST_R * 0.28, 0.05), 0.82, STONE_DARK));
    }

    let lintel_y = POST_H + 0.18;
    parts.push(block_at(
        GAP * 0.34,
        0.52,
        POST_R * 1.9,
        Vec3::new(-GAP * 0.27, lintel_y, 0.0),
        -0.10,
        STONE_B,
    ));
    parts.push(block_at(
        GAP * 0.34,
        0.52,
        POST_R * 1.9,
        Vec3::new(GAP * 0.27, lintel_y, 0.0),
        0.10,
        STONE_B,
    ));
    parts.push(block_at(GAP * 0.16, 0.48, POST_R * 1.75, Vec3::new(0.0, lintel_y + 0.32, 0.0), 0.0, STONE_PALE));
    parts.push(block_at(GAP * 0.48, 0.20, POST_R * 1.5, Vec3::new(0.0, lintel_y - 0.06, 0.0), 0.0, STONE_CAP));
    parts.push(facet_at(0.38, Vec3::new(-0.65, lintel_y - 0.42, 0.12), 0.78, STONE_DARK));
    parts.push(facet_at(0.32, Vec3::new(0.85, lintel_y - 0.38, -0.08), 0.78, STONE_BODY));

    let greys = [STONE_A, STONE_B, STONE_C, STONE_COOL, STONE_BODY];
    let ring_r = 3.2;
    for i in 0..5 {
        let a = i as f32 * (TAU / 5.0) + 0.55;
        let (rx, rz) = (a.cos() * ring_r, a.sin() * ring_r);
        let h = 1.45 + (i % 3) as f32 * 0.42;
        let w = 0.48 + (i % 2) as f32 * 0.10;
        let yaw = a + FRAC_PI_2;
        let tilt = if i % 2 == 0 { 0.22 } else { -0.18 };
        let g = greys[i];
        parts.push(slab_at(
            w,
            h,
            w * 0.82,
            Vec3::new(rx, slab_ground(w, h, tilt) + 0.04, rz),
            yaw,
            tilt,
            g,
        ));
        parts.push(facet_at(w * 0.42, Vec3::new(rx, slab_ground(w, h, tilt) + h * 0.92, rz), 0.5, STONE_PALE));
        if i % 3 == 0 {
            parts.push(lichen_at(0.11, Vec3::new(rx, 0.35, rz + 0.2), LICHEN_SAGE));
        }
    }

    for &(dx, dz, r) in &[
        (-1.4_f32, 0.75, 0.22),
        (1.35, -0.65, 0.20),
        (0.15, 1.05, 0.18),
        (-0.9, -0.95, 0.16),
        (1.6, 0.35, 0.14),
    ] {
        parts.push(facet_at(
            r,
            Vec3::new(dx, r * 0.55, dz),
            0.68,
            if dx < 0.0 { STONE_COOL } else { PEBBLE_WARM },
        ));
    }

    mottle(flat_shaded(merged(parts)), 0.55)
}

// ── Frozen spire (snow landmark) ──────────────────────────────────────────────

/// **The Frozen Spire** — a shattered ice outcrop: a tall central crystal stack of angular
/// chunks stepping up to a pale rime crown, ringed by five leaning flanking shards. Dark
/// frost-rock rubble at the foot grounds it (no snow-dome plinth). ~3.6u tall, base at y=0.
pub fn build_frozen_spire_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();

    // Frost-rock rubble the crystals erupt from — buried footing, not a blank mound.
    parts.push(chunk_at(0.34, yv(0.14), Vec3::new(1.35, 0.55, 1.1), 0.2, 0.0, 0, FROST_ROCK));
    parts.push(chunk_at(0.22, Vec3::new(0.55, 0.10, -0.35), Vec3::new(1.1, 0.7, 1.0), 1.4, 0.15, 0, FROST_ROCK));
    parts.push(chunk_at(0.18, Vec3::new(-0.48, 0.08, 0.42), Vec3::new(1.0, 0.65, 1.1), 2.0, -0.1, 0, FROST_ROCK));

    // Central spire — stacked angular ice chunks tapering upward with bright crown facets.
    let bands: [(f32, f32, f32, f32, u32); 5] = [
        (0.44, 0.55, 1.05, 0.0, ICE_DEEP),
        (0.38, 0.52, 0.98, 0.12, ICE_BODY),
        (0.32, 0.48, 0.92, 0.22, ICE_RIME),
        (0.26, 0.42, 0.88, 0.35, ICE_BODY),
        (0.20, 0.36, 0.82, 0.48, ICE_RIME),
    ];
    let mut cy = 0.12;
    for (i, &(r, h, squash, drift, c)) in bands.iter().enumerate() {
        parts.push(chunk_at(
            r,
            Vec3::new(drift * 0.08, cy + h * 0.5, drift * 0.05),
            Vec3::new(1.0, squash, 0.95),
            drift,
            0.08,
            0,
            c,
        ));
        cy += h;
        if i == 2 || i == 4 {
            parts.push(facet_at(r * 0.55, Vec3::new(drift * 0.1, cy, drift * 0.06), 0.42, ICE_PALE));
        }
    }
    // Pointed crown shard.
    parts.push(chunk_at(
        0.18,
        Vec3::new(0.12, cy + 0.42, 0.04),
        Vec3::new(0.75, 1.4, 0.75),
        0.6,
        -0.15,
        0,
        ICE_PALE,
    ));
    // Sunlit ridge facets standing proud of the body.
    for i in 0..3 {
        let a = i as f32 * (TAU / 3.0) + 0.3;
        parts.push(chunk_at(
            0.06,
            Vec3::new(a.cos() * 0.38, 1.55, a.sin() * 0.38),
            Vec3::new(0.35, 2.8, 0.35),
            a,
            0.0,
            0,
            ICE_PALE,
        ));
    }

    // Flanking shards — leaning ice chunks + smaller tip splinters.
    for i in 0..5 {
        let a = i as f32 * (TAU / 5.0) + 0.4;
        let (sx, sz) = (a.cos() * 0.78, a.sin() * 0.78);
        let h = 0.85 + (i % 3) as f32 * 0.32;
        let c = if i % 2 == 0 { ICE_BODY } else { ICE_DEEP };
        parts.push(chunk_at(
            0.22,
            Vec3::new(sx, h * 0.42, sz),
            Vec3::new(0.9, h, 0.85),
            a,
            0.32,
            0,
            c,
        ));
        parts.push(chunk_at(
            0.12,
            Vec3::new(sx + a.cos() * 0.08, h * 0.88, sz + a.sin() * 0.08),
            Vec3::new(0.7, 1.1, 0.7),
            a + 0.4,
            -0.2,
            0,
            ICE_PALE,
        ));
        if i % 2 == 0 {
            parts.push(tinted(
                Cone { radius: 0.018, height: 0.12 }
                    .mesh()
                    .resolution(4)
                    .build()
                    .rotated_by(Quat::from_rotation_x(PI))
                    .translated_by(Vec3::new(sx, h * 0.55, sz) - yv(0.06)),
                lin(ICE_RIM),
            ));
        }
    }

    mottle(flat_shaded(merged(parts)), 0.32)
}

// ── Sunken pyramid (desert landmark) ──────────────────────────────────────────

/// **The Sunken Pyramid** — a weathered stepped sandstone ziggurat built from faceted
/// blocks (corner drums + sunlit cap facets per tier), a front stair of flat slabs, a
/// doorwayed summit cluster, and half-buried sand-drift rubble at the foot — no blank
/// platform plinth. ~3.6u tall, base at y=0.
pub fn build_sunken_pyramid_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();

    // Half-buried sand drifts — low angled slabs + pebbles hugging the base.
    for &(dx, dz, yaw) in &[
        (1.75_f32, 1.55, 0.4),
        (-1.85, 1.4, -0.6),
        (-1.5, -1.75, 1.2),
        (1.65, -1.45, -0.3),
    ] {
        parts.push(slab_at(1.0, 0.14, 0.75, Vec3::new(dx, 0.08, dz), yaw, 0.08, SAND_DK));
    }
    for &(dx, dz) in &[(1.55_f32, 1.45), (-1.65, 1.25), (-1.4, -1.65), (1.5, -1.35)] {
        parts.push(facet_at(0.28, Vec3::new(dx, 0.11, dz), 0.52, SAND_DK));
    }

    let tiers = 5;
    let (w0, w1, th) = (3.2_f32, 1.0_f32, 0.38_f32);
    let mut y_base = 0.0_f32;
    let mut top_w = w0;
    for i in 0..tiers {
        let t = i as f32 / (tiers - 1) as f32;
        let w = w0 + (w1 - w0) * t;
        let body_c = if i % 2 == 0 { SAND_BODY } else { SAND_DK };

        if i > 0 {
            parts.push(facet_at(w * 0.48, yv(y_base + 0.04), 0.32, SAND_SHADOW));
        }

        let corners = [(w * 0.42, w * 0.42), (-w * 0.42, w * 0.42), (-w * 0.42, -w * 0.42), (w * 0.42, -w * 0.42)];
        for (ci, &(cx, cz)) in corners.iter().enumerate() {
            let tilt = if ci % 2 == 0 { 0.05 } else { -0.04 };
            parts.push(block_at(w * 0.22, th, w * 0.20, Vec3::new(cx, y_base + th * 0.5, cz), tilt, body_c));
        }
        parts.push(facet_at(w * 0.36, yv(y_base + th - 0.02), 0.26, SAND_LT));
        parts.push(slab_at(
            0.55,
            th * 0.85,
            0.16,
            Vec3::new(0.0, y_base + th * 0.45, w * 0.48),
            0.0,
            0.0,
            SAND_LT,
        ));
        y_base += th;
        top_w = w;
    }

    let tw = top_w * 0.9;
    let tht = 0.55;
    parts.push(block_at(tw * 0.45, tht, tw * 0.42, yv(y_base + tht * 0.5), 0.04, SAND_BODY));
    parts.push(slab_at(tw * 0.55, 0.14, tw * 0.50, yv(y_base + tht + 0.06), 0.3, 0.02, SAND_DK));
    parts.push(facet_at(tw * 0.20, Vec3::new(0.0, y_base + tht * 0.42, tw * 0.48), 0.55, SAND_DOOR));

    parts.push(slab_at(0.30, 1.55, 0.26, Vec3::new(2.2, 0.88, 0.48), 0.5, 1.22, SAND_DK));
    parts.push(facet_at(0.24, Vec3::new(2.62, 0.26, 0.58), 0.68, SAND_LT));
    parts.push(facet_at(0.28, Vec3::new(-2.2, 0.15, 0.95), 0.65, SAND_BODY));
    parts.push(facet_at(0.22, Vec3::new(-2.45, 0.12, -0.65), 0.65, SAND_DK));
    parts.push(facet_at(0.18, Vec3::new(0.85, 0.10, -1.55), 0.6, SAND_DK));

    mottle(flat_shaded(merged(parts)), 0.58)
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
