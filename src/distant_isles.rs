//! Distant background islands — faint hazed landmasses ringing the open sea.
//!
//! These exist purely to give the horizon something to read: small terraced low-poly
//! islands built with the **same recipe as the real island** (`worldmap::build_terrain_mesh`)
//! — elliptical falloff + noisy coast → per-tile height classes stepped into flat terraces,
//! biome colour baked into vertex `ATTRIBUTE_COLOR`, faceted per-quad normals — but stripped
//! of all detail: **no props, rivers, scatter, collision, or nav.** They sit out past the
//! ~190-unit fog wall (`scene.rs` Linear fog) so they're solid haze from the castle and only
//! resolve into soft, DoF-blurred silhouettes as the hero reaches the matching shore.
//!
//! There is deliberately **no white-triangle fallback**: every vertex is coloured and lit by
//! the shared white `StandardMaterial` (auto-batched, per the mesh-building contract), so the
//! islands pick up the sun, `DistanceFog` haze and `dof.rs` background blur for free — no
//! custom shader, no day/night special-casing.
//!
//! Spawned from `worldmap::build` and tagged [`crate::biome::BiomeEntity`] so the biome-swap
//! rebuild path (keys 1–5) despawns + recreates them with the rest of the world.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::lin;

/// Sea-surface Y (mirrors the private `worldmap::SEA_Y`). Island land starts just above it.
const SEA_Y: f32 = -0.4;
/// World units per terrain tile in an island's local grid. Coarser than the real map's 1.0 —
/// these are blurred and fog-bound, so finer tiling would only cost verts no one can see.
const TILE: f32 = 2.0;
/// World-Y per height class. Much bigger than the map's 0.5 so the islands rise as real relief
/// — they're tiny in footprint vs the main island, so they need exaggerated height to read as
/// landmasses (not flat reefs) from across the sea.
const STEP: f32 = 1.7;
/// How high the lowest land (the class-1 beach) sits above the sea. Must be a solid margin: the
/// sea is a *translucent* alpha-blended sheet, so any land at/below the waterline shows THROUGH
/// the blue water and reads as ghostly/transparent. Coast cliffs then drop only to `SEA_Y` (no
/// submerged geometry), exactly like the real island's coast.
const BEACH_LIFT: f32 = 0.8;
/// Number of distant islands (one palette each).
const COUNT: usize = 5;

// ── Biome palettes ───────────────────────────────────────────────────────────────
/// Silhouette tones for one island, low ground → peak. `snowcap` lets tall islands of this
/// palette wear a snow-white peak band (volcanic-looking green/grey peaks pushing into snow).
#[derive(Clone, Copy)]
struct Palette {
    lowland: u32,
    upland: u32,
    cliff: u32,
    beach: u32,
    snowcap: bool,
}

const PALETTES: [Palette; COUNT] = [
    // Forest — green hills, sandy shore, can wear snow caps when tall.
    Palette { lowland: 0x4a8136, upland: 0x6fb24c, cliff: 0x39562d, beach: 0xcdb079, snowcap: true },
    // Desert — dune tans, pale rock cliffs, no snow.
    Palette { lowland: 0xcdb079, upland: 0xe0c692, cliff: 0x9c8456, beach: 0xe2cd96, snowcap: false },
    // Rock — bare grey massif, snow-dusted peaks when tall.
    Palette { lowland: 0x756c62, upland: 0x968c80, cliff: 0x4f4942, beach: 0x9a9182, snowcap: true },
    // Snow — blue-grey body, near-white caps.
    Palette { lowland: 0xc9d6e8, upland: 0xe4ecf5, cliff: 0x7b8597, beach: 0xdfe7f0, snowcap: true },
    // Swamp — murky olive, no snow.
    Palette { lowland: 0x46512f, upland: 0x5d6a3e, cliff: 0x2f3722, beach: 0x6b6a44, snowcap: false },
];

/// Per-island authored shape/size/elevation — rolled deterministically in [`build`].
struct Isle {
    radius: f32,   // mean coast radius (world units)
    aspect: f32,   // ellipse stretch: ax = radius*aspect, az = radius/aspect
    mountain: f32, // 0 = flat sandbar … 1 = tall central peak
    palette: Palette,
    seed: u32,
}

// ── Deterministic value noise (coast distortion + ridges) ────────────────────────
fn hash2(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x8da6_b343)
        ^ (z as u32).wrapping_mul(0xd816_3841);
    h = (h ^ (h >> 15)).wrapping_mul(0x2c1b_3c6d);
    h = (h ^ (h >> 12)).wrapping_mul(0x297a_2d39);
    (h ^ (h >> 15)) as f32 / 4_294_967_296.0
}

fn vnoise(x: f32, z: f32, seed: u32) -> f32 {
    let x0 = x.floor() as i32;
    let z0 = z.floor() as i32;
    let fx = x - x0 as f32;
    let fz = z - z0 as f32;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sz = fz * fz * (3.0 - 2.0 * fz);
    let n00 = hash2(x0, z0, seed);
    let n10 = hash2(x0 + 1, z0, seed);
    let n01 = hash2(x0, z0 + 1, seed);
    let n11 = hash2(x0 + 1, z0 + 1, seed);
    let nx0 = n00 + (n10 - n00) * sx;
    let nx1 = n01 + (n11 - n01) * sx;
    nx0 + (nx1 - nx0) * sz
}

/// 3-octave fractal value noise in [0,1].
fn fbm(x: f32, z: f32, seed: u32) -> f32 {
    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for o in 0..3u32 {
        sum += vnoise(x * freq, z * freq, seed.wrapping_add(o.wrapping_mul(131))) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

// ── Deterministic mulberry32 RNG (same recipe as camps.rs) ───────────────────────
fn next_u32(s: &mut u32) -> u32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    t ^ (t >> 14)
}
fn rng01(s: &mut u32) -> f32 {
    next_u32(s) as f32 / 4_294_967_296.0
}
fn rng_range(s: &mut u32, lo: f32, hi: f32) -> f32 {
    lo + rng01(s) * (hi - lo)
}

// ── Height field ─────────────────────────────────────────────────────────────────
/// Land height class at local tile-centre `(cx, cz)` (world units, island-local, pre-rotation),
/// or 0 for sea. Class 1 is the waterline beach ring; the peak is `peak_classes(isle)`.
fn class_at(isle: &Isle, cx: f32, cz: f32) -> i32 {
    let ax = isle.radius * isle.aspect;
    let az = isle.radius / isle.aspect;
    let nx = cx / ax;
    let nz = cz / az;
    let rr = (nx * nx + nz * nz).sqrt();

    // Distorted coast: low-freq noise pushes the ellipse outline in/out so no two read alike.
    let coast = (fbm(cx * 0.05, cz * 0.05, isle.seed) - 0.5) * 0.42;
    let edge = 1.0 + coast;
    if rr >= edge {
        return 0; // sea
    }

    let inland = ((edge - rr) / edge).clamp(0.0, 1.0); // 0 at coast → ~1 at centre
    let peak = peak_classes(isle);
    // mountain≈0 → high exponent → only the very centre lifts (flat island); mountain≈1 →
    // near-linear cone → a real central peak. Ridge noise breaks up the upper slopes.
    let exp = 2.3 - 1.4 * isle.mountain;
    let ridge = (fbm(cx * 0.09, cz * 0.09, isle.seed.wrapping_add(7)) - 0.5) * 0.22 * inland;
    let hv = inland.powf(exp) + ridge;
    let c = (hv * peak as f32).round() as i32;
    c.clamp(1, peak) // any land tile is at least a class-1 beach
}

/// Tallest height class this island reaches (drives both geometry and the colour ramp). Spans
/// flat sandbars (~3 classes) to dramatic peaks (~16) so the `mountain` roll gives real variety:
/// at STEP 1.7 that's roughly 4 to 27 world units of relief.
fn peak_classes(isle: &Isle) -> i32 {
    (3.0 + 13.0 * isle.mountain).round() as i32
}

/// World-Y of the top surface of a given land class. The class-1 beach sits [`BEACH_LIFT`]
/// above the sea so the island reads as solid land, not a half-submerged reef.
fn top_y(c: i32) -> f32 {
    SEA_Y + BEACH_LIFT + (c - 1) as f32 * STEP
}

// ── Colour ramp ──────────────────────────────────────────────────────────────────
fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        1.0,
    ]
}

/// Top-surface colour for a tile at class `c` (peak `peak`), with a tiny per-tile jitter so the
/// terraces don't read as flat paint. The lowest class is a sandy/icy beach; tall snow-capped
/// palettes whiten the upper third.
fn top_color(isle: &Isle, c: i32, peak: i32, cx: f32, cz: f32) -> [f32; 4] {
    let p = isle.palette;
    let low = lin(p.lowland);
    let up = lin(p.upland);
    let t = if peak > 1 { (c - 1) as f32 / (peak - 1) as f32 } else { 0.0 };
    let mut col = lerp4(low, up, t * t * (3.0 - 2.0 * t)); // smoothstep low→up
    // Waterline beach ring.
    if c <= 1 {
        col = lerp4(col, lin(p.beach), 0.7);
    }
    // Snow-cap the upper slopes of tall snowcap palettes.
    if p.snowcap && isle.mountain > 0.5 && t > 0.7 {
        let k = ((t - 0.7) / 0.3).clamp(0.0, 1.0);
        col = lerp4(col, lin(0xf4f9ff), k);
    }
    // ±5% per-tile value jitter.
    let j = 0.95 + 0.10 * hash2((cx * 3.0) as i32, (cz * 3.0) as i32, isle.seed ^ 0x51);
    [col[0] * j, col[1] * j, col[2] * j, 1.0]
}

// ── Mesh ─────────────────────────────────────────────────────────────────────────
/// Build one island's terraced mesh in local space (centred at origin, Y already in world
/// terms). Same flat-shaded quad recipe as `worldmap::build_terrain_mesh`: a top quad per land
/// tile plus cliff walls down to lower neighbours / the waterline at the coast.
fn build_mesh(isle: &Isle) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let mut quad = |p: [[f32; 3]; 4], n: [f32; 3], c: [[f32; 4]; 4]| {
        let b = positions.len() as u32;
        for k in 0..4 {
            positions.push(p[k]);
            normals.push(n);
            colors.push(c[k]);
        }
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    };

    let peak = peak_classes(isle);
    // Tile half-extent: cover the distorted coast (radius * aspect + noise slack).
    let reach = isle.radius * isle.aspect.max(1.0 / isle.aspect) * 1.5;
    let n = (reach / TILE).ceil() as i32 + 2;
    let center = |ix: i32| ix as f32 * TILE + TILE * 0.5;
    let cls = |ix: i32, iz: i32| class_at(isle, center(ix), center(iz));

    const NB: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

    for iz in -n..n {
        for ix in -n..n {
            let c = cls(ix, iz);
            if c == 0 {
                continue;
            }
            let x0 = ix as f32 * TILE;
            let z0 = iz as f32 * TILE;
            let x1 = x0 + TILE;
            let z1 = z0 + TILE;
            let top = top_y(c);
            let cx = center(ix);
            let cz = center(iz);
            let tc = top_color(isle, c, peak, cx, cz);

            // Top quad (flat, single colour per tile — blurred at this distance anyway).
            quad(
                [[x0, top, z0], [x1, top, z0], [x1, top, z1], [x0, top, z1]],
                [0.0, 1.0, 0.0],
                [tc, tc, tc, tc],
            );

            // Cliff walls down to each lower neighbour (or the waterline at the coast).
            let cliff = lin(isle.palette.cliff);
            let wall_top = [cliff[0] * 0.95, cliff[1] * 0.95, cliff[2] * 0.95, 1.0];
            let wall_bot = [cliff[0] * 0.62, cliff[1] * 0.60, cliff[2] * 0.58, 1.0];
            let wc = [wall_bot, wall_bot, wall_top, wall_top];
            for (dx, dz) in NB {
                let nc = cls(ix + dx, iz + dz);
                // Inland steps drop to the lower neighbour; coast tiles drop to the waterline
                // (SEA_Y) and no further — nothing goes below the translucent sea sheet.
                let nh = if nc >= 1 { top_y(nc) } else { SEA_Y };
                if top <= nh + 1e-4 {
                    continue;
                }
                let (e0, e1, nrm): ([f32; 2], [f32; 2], [f32; 3]) = match (dx, dz) {
                    (1, 0) => ([x1, z0], [x1, z1], [1.0, 0.0, 0.0]),
                    (-1, 0) => ([x0, z1], [x0, z0], [-1.0, 0.0, 0.0]),
                    (0, 1) => ([x1, z1], [x0, z1], [0.0, 0.0, 1.0]),
                    _ => ([x0, z0], [x1, z0], [0.0, 0.0, -1.0]),
                };
                quad(
                    [[e0[0], nh, e0[1]], [e1[0], nh, e1[1]], [e1[0], top, e1[1]], [e0[0], top, e0[1]]],
                    nrm,
                    wc,
                );
            }
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ── Spawn ────────────────────────────────────────────────────────────────────────
/// Spawn the ring of distant islands. Called once from `worldmap::build`.
pub fn build(commands: &mut Commands, meshes: &mut Assets<Mesh>, std_mats: &mut Assets<StandardMaterial>) {
    // One shared white material — vertex colours carry the look, so all islands auto-batch.
    // DOUBLE-SIDED: these islands are tall (peaks ~9-11 units) but are viewed from the hero at
    // sea level, i.e. from BELOW their up-facing top surfaces. With normal back-face culling
    // those tops are culled and you see straight through to the sea — a hollow ring. Rendering
    // both faces (and flipping the normal for back-faces via `double_sided`) keeps them solid
    // from any angle. Cheap: only 5 small, usually-fogged meshes.
    let mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.93,
        reflectance: 0.15,
        double_sided: true,
        cull_mode: None,
        ..default()
    });

    let mut s: u32 = 0x1515_d1e5;
    for i in 0..COUNT {
        let isle = Isle {
            radius: rng_range(&mut s, 20.0, 58.0),
            aspect: rng_range(&mut s, 0.78, 1.28),
            mountain: rng01(&mut s).powf(0.85),
            palette: PALETTES[i],
            seed: 0x9e37 + i as u32 * 0x101,
        };

        // Random ring scatter: fan the islands evenly around the origin with jitter (keeps them
        // non-overlapping for free), pushed as FAR out as still reads. The hero can only ever
        // glimpse them from a far shore — the camera far plane is 230 and fog goes solid by ~190
        // (both camera-relative) — so the near edge sits ~210-250 units off the origin: invisible
        // from the castle, fading in only as the hero reaches a coast facing them. Our island
        // reaches ~106×80, so this leaves a wide band of open sea between us and them.
        let base_angle = TAU * (i as f32 / COUNT as f32) + rng_range(&mut s, -0.38, 0.38);
        let mut dist = 200.0 + isle.radius + rng_range(&mut s, 10.0, 50.0);
        let yaw = rng_range(&mut s, 0.0, TAU);

        // Nudge outward if the island (centre or its near edge) would land on the southern
        // Blight landmass, which reaches well past the original coast (z up to ~165).
        for _ in 0..6 {
            let cx = dist * base_angle.cos();
            let cz = dist * base_angle.sin();
            let near = dist - isle.radius;
            let nx = near * base_angle.cos();
            let nz = near * base_angle.sin();
            if !crate::ork_fortress::in_blight_world(cx, cz)
                && !crate::ork_fortress::in_blight_world(nx, nz)
            {
                break;
            }
            dist += 24.0;
        }

        let cx = dist * base_angle.cos();
        let cz = dist * base_angle.sin();
        let mesh = meshes.add(build_mesh(&isle));
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(cx, 0.0, cz).with_rotation(Quat::from_rotation_y(yaw)),
            NotShadowCaster, // their shadows are fogged out anyway — skip the shadow pass
            BiomeEntity,
        ));
    }
}
