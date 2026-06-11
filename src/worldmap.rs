//! The **world map** — a Bevy port of the TS game's island (`src/world/tileMap.ts`),
//! generated in the original BASE space (144×108): an elliptical island with a noisy
//! coast, five biome blobs (snow NW, desert NE, rock E, forest SW, swamp S), a grass
//! centre safe-zone (the castle spot), a grass frontier with scattered forest clumps and
//! rolling terraced knolls, a beach ring backed by patchy coastal mountain ridges, four
//! carved rivers + one lake, and **terraced** stepped heights (flat tile-tops + cliff
//! faces; snow peak 10, rock peak 9). South of the old coast the grid extends into the
//! **Blight** — the walkable ork-fortress mire (`TB::Blight`, shape owned by
//! `ork_fortress.rs`) that gameplay treats as swamp (poison + slow).
//!
//! `build` seeds the full playable island: the ground mesh plus ork-camp / castle / ore / chest
//! placement and wildlife (single-biome views, keys 1–5, have no island layout, so none of these).
//! Biome boundaries get **smooth colour blending** (the ground mesh's per-vertex colour is
//! a soft distance-weighted mix of the biome palettes), while the discrete classification
//! still drives heights + which biome's props scatter on each tile.

use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::biome::{
    scatter_region, AtmoSample, Backdrop, Biome, BiomeAmbience, BiomeAmbiences, BiomeConfig,
    GroundDetail, ParticleKind, PropClass,
};
use crate::groundcover as gc;
use crate::palette::lin;
use crate::terrain::TerrainMaterial;
use crate::water::{WaterExt, WaterMaterial, WaterParams};

// ── Map dimensions ───────────────────────────────────────────────────────────────
/// Map enlargement vs the original base island (more tiles → more land + props).
pub const MAP_SCALE: f32 = 1.5;
// The GRID is the enlarged resolution; GENERATION still runs in *base* space — the grid
// loop samples `classify(ix / MAP_SCALE, …)`, so the island shape is identical, just
// drawn over more tiles. `CX/CZ` stay the BASE centre used by all the generation math;
// `GX/GZ` are the GRID centre used for world placement + tile-cache indexing.
pub const COLS: i32 = 216; // 144 * 1.5
/// Rows: the original island (108 base rows → 162) PLUS the southern **Blight** extension —
/// the walkable ork-fortress landmass (`ork_fortress::blight_class_base`). World z runs
/// −81 … +165; everything north of +81 is the original map, untouched.
pub const ROWS: i32 = 246;
const CX: f32 = 72.0; // base COLS/2 — generation centre
const CZ: f32 = 54.0;
/// Grid centre (enlarged) — world placement recentres the map onto the origin here.
pub const GX: f32 = COLS as f32 / 2.0;
/// NOT `ROWS/2`: the castle stays at the origin, so GZ is pinned to the ORIGINAL map's
/// half-height (162/2) and the Blight extension grows `ROWS` southward only.
pub const GZ: f32 = 81.0;
const ISLAND_RX: f32 = 71.0;
const ISLAND_RZ: f32 = 53.0;
const ISLAND_EXP: f32 = 2.6;
pub const SAFE_R: f32 = 18.0; // castle safe-zone radius (forced flat grass)
pub const GROUND_STEP: f32 = 0.5; // world-Y per height class
const SEA_Y: f32 = -0.4;
/// Colour-blend half-width (tiles) at biome edges.
const BLEND: f32 = 4.5;

/// Shared daytime atmosphere: (sky, fog_density, sun_color, sun_illuminance,
/// ambient_color, ambient_brightness, sun_pos). Light fog so the big island reads across.
pub const ATMOSPHERE: (u32, f32, u32, f32, u32, f32, Vec3) =
    (0xb4d2ec, 0.0060, 0xffedc7, 11_000.0, 0xe6edf5, 165.0, Vec3::new(80.0, 110.0, 40.0));

// ── Biome palette (sRGB hex) for the blended ground colour ──────────────────────
const COL_GRASS: u32 = 0x6fb24c;
/// Meadow macro-patch tones mottled into the grass base (see `ground_color`).
const COL_GRASS_DARK: u32 = 0x4f8c38; // lush shaded patches
const COL_GRASS_DRY: u32 = 0x8fa953; // dry, sun-bleached patches
const COL_GRASS_GOLD: u32 = 0xa8b048; // broad sun-gilded meadow sweeps
const COL_SAND: u32 = 0xcdb079;
const COL_FOREST: u32 = 0x5d9e44;
const COL_ROCK: u32 = 0x8d847a;
const COL_SNOW: u32 = 0xe4ecf5;
const COL_DESERT: u32 = 0xddc189;
const COL_SWAMP: u32 = 0x55613a;
/// The Blight: trampled ork mud (interior mottle adds churned-black, ash and a sickly
/// warp-green tinge — see `biome_col_at`).
const COL_BLIGHT: u32 = 0x4d3e2a;
const COL_BLIGHT_DARK: u32 = 0x332817;
const COL_BLIGHT_ASH: u32 = 0x6f695c;
const COL_BLIGHT_GREEN: u32 = 0x59653a;
/// Snow-interior mottle (see `biome_col_at`): cool drift-shadow troughs + wind-polished
/// bright crests, so the snowfield reads as wind-shaped drifts instead of one cream sheet.
const COL_SNOW_SHADE: u32 = 0xc9d6e8;
const COL_SNOW_BRIGHT: u32 = 0xf4f9ff;
/// Forest-floor mottle: dark moist loam under the canopy + dry leaf-litter patches.
const COL_FOREST_DARK: u32 = 0x4a8136;
const COL_FOREST_DRY: u32 = 0x79a24c;
/// Exposed soil on grass-biome cliff faces (lip lighter, base darker).
const COL_DIRT: u32 = 0x6b4f30;
/// Snow-biome cliff faces: a bright snow lip over exposed blue-grey rock — the old
/// "darken the snow colour" walls read as bare foam blocks in the same cream as the tops.
const SNOW_CLIFF_LIP: u32 = 0xe9f0f9;
const SNOW_CLIFF_ROCK: u32 = 0x7b8597;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TB {
    Grass,
    Sand,
    Forest,
    Rock,
    Snow,
    Desert,
    Swamp,
    /// The ork-blighted mire around Gnashfang Hold (south extension). Maps to
    /// [`Biome::Swamp`] for gameplay (poison, slow, ambience) but draws its own ground.
    Blight,
}

struct Region {
    x: f32,
    z: f32,
    r: f32,
    biome: TB,
    /// centre height class for mountain biomes (0 = flat biome).
    peak: i32,
}

const REGIONS: [Region; 6] = [
    Region { x: 26.0, z: 24.0, r: 31.0, biome: TB::Snow, peak: 10 }, // NW snow massif
    Region { x: 112.0, z: 28.0, r: 34.0, biome: TB::Desert, peak: 0 }, // NE dunes
    // E rock range — pulled in toward the castle (122→116: the mine country starts just past
    // the safe-zone fray instead of a 33-unit trek) and LOWERED (peak 15→9) so most faces are
    // 1-class terraces the nav-grid can climb; less coast-clipping on the far side too.
    Region { x: 116.0, z: 57.0, r: 28.0, biome: TB::Rock, peak: 9 },
    Region { x: 32.0, z: 80.0, r: 34.0, biome: TB::Forest, peak: 0 }, // SW forest
    Region { x: 72.0, z: 92.0, r: 32.0, biome: TB::Swamp, peak: 0 }, // S swamp
    // Eastern marsh arm: fills the open grass strip that ran between the S swamp and the
    // rocky mine range (world x +30…+66, z +15…+60), so the marsh laps right up to the
    // foot of the mines instead of leaving a grass corridor. Rock's mountain priority in
    // `region_at` auto-clips the east edge (the mines stay rock), and the castle safe-ring
    // forces grass at the centre, so this only eats the in-between grass.
    Region { x: 102.0, z: 78.0, r: 27.0, biome: TB::Swamp, peak: 0 },
];

struct Plateau {
    x: f32,
    z: f32,
    r: f32,
    peak: i32,
}
const PLATEAUS: [Plateau; 4] = [
    Plateau { x: 98.0, z: 72.0, r: 9.0, peak: 5 },
    Plateau { x: 52.0, z: 50.0, r: 7.0, peak: 4 },
    Plateau { x: 90.0, z: 36.0, r: 7.0, peak: 5 }, // NE mesa on the desert fringe
    Plateau { x: 20.0, z: 62.0, r: 7.0, peak: 4 }, // W forest highland
];

const DELIBERATE_LAKE: (f32, f32, f32, f32) = (92.0, 80.0, 5.0, 3.0); // x,z,rx,rz

// ── Procedural generation (ported from tileMap.ts, base space) ──────────────────
fn noise_a(x: f32, z: f32) -> f32 {
    (x * 0.13 + 1.7).sin() * (z * 0.11 - 2.3).cos() + (x * 0.31 + z * 0.29 + 4.5).sin() * 0.5
}
fn noise_b(x: f32, z: f32) -> f32 {
    (x * 0.21 - 3.1).sin() * (z * 0.19 + 0.7).cos() + ((x + z) * 0.07 + 5.2).sin() * 0.4
}

fn is_land_shape(x: f32, z: f32) -> bool {
    let dx = (x - CX).abs() / ISLAND_RX;
    let dz = (z - CZ).abs() / ISLAND_RZ;
    let r = dx.powf(ISLAND_EXP) + dz.powf(ISLAND_EXP);
    let coast = noise_a(x, z) * 0.08;
    r + coast < 1.0
}

fn dist_from_coast(x: f32, z: f32) -> i32 {
    let mut min = 10;
    const DIRS: [(f32, f32); 8] =
        [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)];
    for (dx, dz) in DIRS {
        for d in 1..=10 {
            if !is_land_shape(x + dx * d as f32, z + dz * d as f32) {
                if d < min {
                    min = d;
                }
                break;
            }
        }
    }
    min
}

fn dist_from_castle(x: f32, z: f32) -> f32 {
    (x - CX).hypot(z - CZ)
}

fn river_x(z: f32) -> f32 {
    40.0 + (z * 0.18).sin() * 5.0 + (z * 0.07 + 1.4).sin() * 3.0
}
fn river_z(x: f32) -> f32 {
    20.0 + (x * 0.13 + 0.7).sin() * 4.0
}
/// Southern stream: rises near the castle safe-zone and winds south through the forest to the
/// coast (the safe-zone test in `is_river` clips its head).
fn river_x2(z: f32) -> f32 {
    56.0 + (z * 0.15 + 2.0).sin() * 4.0 + (z * 0.06).sin() * 2.5
}
/// Southern crossways river: spans the island below the castle through forest + swamp.
fn river_z2(x: f32) -> f32 {
    86.0 + (x * 0.11 + 1.0).sin() * 4.0
}
fn in_mountain(x: f32, z: f32) -> bool {
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    REGIONS.iter().any(|r| r.peak > 0 && (x - r.x).hypot(z - r.z) + wob < r.r + 2.0)
}
fn is_river(x: f32, z: f32) -> bool {
    if dist_from_castle(x, z) < SAFE_R {
        return false;
    }
    if in_mountain(x, z) {
        return false;
    }
    let cx = river_x(z);
    let w = 0.75 + (z * 0.5).sin() * 0.2;
    if (x - cx).abs() < w {
        return true;
    }
    // Northern crossways branch (base-space bound — the old `COLS - 10` was grid-space and
    // never clipped anything; the coast does the clipping anyway).
    if x > 46.0 && x < 134.0 {
        let cz = river_z(x);
        if (z - cz).abs() < 0.7 {
            return true;
        }
    }
    // Southern stream toward the south coast.
    if z > 58.0 && (x - river_x2(z)).abs() < 0.7 {
        return true;
    }
    // Southern crossways river through forest + swamp.
    if x > 14.0 && x < 126.0 && (z - river_z2(x)).abs() < 0.75 {
        return true;
    }
    false
}
fn is_lake(x: f32, z: f32) -> bool {
    let dx = (x - DELIBERATE_LAKE.0) / DELIBERATE_LAKE.2;
    let dz = (z - DELIBERATE_LAKE.1) / DELIBERATE_LAKE.3;
    dx * dx + dz * dz < 1.0
}

fn edge_fray(x: f32, z: f32) -> f32 {
    (x * 0.5 + z * 0.35 + 1.3).sin() * 1.1
        + (x * 0.9 - z * 0.82 + 4.0).sin() * 1.6
        + (x * 1.5 + z * 1.3 + 2.2).sin() * 1.0
}

fn region_at(x: f32, z: f32) -> Option<usize> {
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    let mut best: Option<usize> = None;
    let mut best_edge = f32::INFINITY;
    for (i, reg) in REGIONS.iter().enumerate() {
        let fray = if reg.peak > 0 { 0.0 } else { edge_fray(x, z) };
        let d = (x - reg.x).hypot(z - reg.z) + wob + fray;
        let edge = d - reg.r;
        if edge < 0.0 && edge < best_edge {
            best_edge = edge;
            best = Some(i);
        }
    }
    best
}

/// Terraced coastal ridge height class (≥2) at base `(x, z)`, or 0 for no hill. A low-frequency
/// mask picks which stretches of coast get a mountain ring (the rest stays open beach/plain).
/// Height rises MONOTONICALLY toward the sea — tallest just behind the beach, tapering to flat
/// inland — so the inland face is a climbable terraced ramp (≤1 class per step for the
/// nav-grid) while the seaward face drops to the beach as a sheer cliff: the ridge tops are
/// reachable from one side only.
fn coast_hill_class(x: f32, z: f32) -> i32 {
    let d = dist_from_coast(x, z) as f32;
    if !(2.0..=7.0).contains(&d) {
        return 0;
    }
    let mask = (x * 0.045 + 1.7).sin() + (z * 0.05 - 0.6).cos() + noise_a(x * 0.5, z * 0.5) * 0.25;
    if mask < 0.3 {
        return 0;
    }
    let band = ((7.0 - d) / 4.5).clamp(0.0, 1.0); // 0 inland (d=7) → 1 at the coast side
    let h = 1.0 + mask.min(1.6) * 5.0 * band + noise_b(x * 1.1 + 5.0, z * 1.1) * 0.7;
    let cls = h.round().clamp(1.0, 9.0) as i32;
    if cls >= 2 { cls } else { 0 }
}

/// Gentle terraced inland hills (height class 1..=`max`) layered over every flat part of the
/// island — grassy knolls on the frontier, dunes in the desert, wooded rises in the forest —
/// so the middle of the map isn't one flat sheet. Nested thresholds mean a hill always climbs
/// through each class ring in turn, keeping every face a 1-class step the nav-grid can walk;
/// inside `SAFE_R + 8` it stays flat so the castle approaches and siege lanes stay open.
fn inland_hills(x: f32, z: f32, dc: f32, max: i32) -> i32 {
    if dc <= SAFE_R + 8.0 {
        return 1;
    }
    let roll = noise_a(x * 0.3 + 9.0, z * 0.3 - 5.0) + noise_b(x * 0.16 - 4.0, z * 0.16 + 8.0) * 0.6;
    let h = if roll > 1.9 {
        4
    } else if roll > 1.45 {
        3
    } else if roll > 0.95 {
        2
    } else {
        1
    };
    h.min(max.max(1))
}

fn plateau_height(x: f32, z: f32) -> i32 {
    for p in &PLATEAUS {
        let d = (x - p.x).hypot(z - p.z);
        if d >= p.r {
            continue;
        }
        let tiers = (p.peak - 1) as f32;
        let cls = 2 + ((1.0 - d / p.r) * tiers).floor() as i32;
        return cls.clamp(2, p.peak);
    }
    0
}

// Half-width of the castle-facing ramp corridor up each peak region (base tiles). Widened
// 1.7→3.0 so the rock range / snow massif climb is a broad avenue, not a one-tile stair.
const RAMP_HALF_TILES: f32 = 3.0;
fn ramp_class(x: f32, z: f32, reg: &Region) -> Option<i32> {
    if reg.peak <= 0 {
        return None;
    }
    let dx = x - reg.x;
    let dz = z - reg.z;
    let dc = dx.hypot(dz);
    if dc >= reg.r {
        return None;
    }
    let ramp_ang = (CZ - reg.z).atan2(CX - reg.x);
    let mut da = (dz.atan2(dx) - ramp_ang) % std::f32::consts::TAU;
    if da < -std::f32::consts::PI {
        da += std::f32::consts::TAU;
    }
    if da > std::f32::consts::PI {
        da -= std::f32::consts::TAU;
    }
    let half_ang = (RAMP_HALF_TILES / dc.max(1.5)).min(std::f32::consts::PI);
    if da.abs() >= half_ang {
        return None;
    }
    let span = (reg.peak - 2).max(1) as f32;
    let step_len = reg.r / span;
    let cls = 2 + ((reg.r - dc) / step_len).floor() as i32;
    Some(cls.clamp(2, reg.peak))
}

fn mountain_height(x: f32, z: f32, reg: &Region) -> i32 {
    if let Some(rc) = ramp_class(x, z, reg) {
        return rc;
    }
    let dc = (x - reg.x).hypot(z - reg.z);
    let peak = reg.peak;
    let t = (1.0 - dc / reg.r).max(0.0);
    // Noise weight kept LOW (was 0.35 + t·0.95): noise_b swings ±1.4, so the old weight stamped
    // frequent ≥2-class jumps between neighbour tiles — sheer cliff pockets `can_step` refuses,
    // which walled off most of the range. Now slopes are mostly 1-class walkable terraces.
    let h = (peak as f32 * t * t + noise_b(x, z) * (0.2 + t * 0.4)).round() as i32;
    h.clamp(1, peak)
}

fn classify(x: f32, z: f32) -> Option<(TB, i32)> {
    // The Blight: the walkable ork-fortress landmass south of the old coast (shape + heights
    // live in `ork_fortress.rs`). Checked BEFORE the island shape — it deliberately extends
    // past the coast — and it overrides the south-swamp fringe so the trampled mud runs
    // continuous from the marsh into Gnashfang Hold.
    if let Some(h) = crate::ork_fortress::blight_class_base(x, z) {
        return Some((TB::Blight, h));
    }
    if !is_land_shape(x, z) {
        return None;
    }
    // Town build plots must stay flat, empty grass — a terrain step or biome props under a
    // plot would collide with the building constructed on it. Plots are authored in world
    // space; this runs in base space, so convert (world = base·MAP_SCALE − G).
    if crate::town::near_build_plot(x * MAP_SCALE - GX, z * MAP_SCALE - GZ) {
        return Some((TB::Grass, 1));
    }
    let dc = dist_from_castle(x, z);
    if dc < SAFE_R + edge_fray(x, z).max(-4.0) {
        return Some((TB::Grass, 1));
    }
    if is_river(x, z) {
        return None;
    }
    if is_lake(x, z) {
        return None;
    }
    let d = dist_from_coast(x, z) as f32;
    let beach_w =
        1.0 + (1.0 + (x * 0.6 + z * 0.42 + 2.1).sin() * 0.8 + (x * 1.25 - z * 0.95 + 0.4).sin() * 0.6).max(0.0);
    if d <= beach_w {
        return Some((TB::Sand, 1));
    }
    // Coastal mountain ridges around the island rim (skip inside the peak regions — the snow
    // massif / rock range own their heights there). Tall ridges read as bare rock; lower
    // rises keep the local flat-region biome (sandy bluffs in the desert, wooded coastal
    // hills in the forest, …) or grass.
    if !in_mountain(x, z) {
        let ch = coast_hill_class(x, z);
        if ch > 0 {
            let b = match region_at(x, z) {
                Some(ri) if REGIONS[ri].peak == 0 => REGIONS[ri].biome,
                _ if ch >= 4 => TB::Rock,
                _ => TB::Grass,
            };
            return Some((b, ch));
        }
    }
    let ph = plateau_height(x, z);
    if ph > 0 {
        // A plateau inside a flat biome blob keeps that biome (desert mesa, forest highland);
        // out on the frontier it's a grass plateau.
        let b = match region_at(x, z) {
            Some(ri) if REGIONS[ri].peak == 0 => REGIONS[ri].biome,
            _ => TB::Grass,
        };
        return Some((b, ph));
    }
    if let Some(ri) = region_at(x, z) {
        let reg = &REGIONS[ri];
        if reg.biome == TB::Swamp && dc < SAFE_R {
            return Some((TB::Grass, 1));
        }
        if reg.peak > 0 {
            return Some((reg.biome, mountain_height(x, z, reg)));
        }
        // Flat biomes still get the inland-hills field: dunes in the desert, wooded rises in
        // the forest; the swamp stays marsh-flat.
        let max = if reg.biome == TB::Swamp { 1 } else { 3 };
        return Some((reg.biome, inland_hills(x, z, dc, max)));
    }
    let h = inland_hills(x, z, dc, 4);
    let forest_n = noise_a(x, z) * noise_b(x + 7.0, z - 3.0);
    if forest_n > 0.35 {
        return Some((TB::Forest, h));
    }
    Some((TB::Grass, h))
}

// ── Tile cache ──────────────────────────────────────────────────────────────────
static TILES: OnceLock<Vec<Option<(TB, i32)>>> = OnceLock::new();
fn tiles() -> &'static Vec<Option<(TB, i32)>> {
    TILES.get_or_init(|| {
        let mut v = Vec::with_capacity((COLS * ROWS) as usize);
        for iz in 0..ROWS {
            for ix in 0..COLS {
                // Sample generation in BASE space so the island shape is unchanged.
                v.push(classify(ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE));
            }
        }
        v
    })
}
fn tile_at(ix: i32, iz: i32) -> Option<(TB, i32)> {
    if ix < 0 || iz < 0 || ix >= COLS || iz >= ROWS {
        return None;
    }
    tiles()[(iz * COLS + ix) as usize]
}

// World ↔ tile helpers (world is the enlarged tile-space recentred on the origin).
fn tile_biome_world(wx: f32, wz: f32) -> Option<Biome> {
    let t = tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32)?;
    match t.0 {
        TB::Forest => Some(Biome::Forest),
        TB::Snow => Some(Biome::Snow),
        TB::Rock => Some(Biome::Rocky),
        TB::Desert => Some(Biome::Desert),
        // The Blight IS swamp to gameplay: poison + slow (`player::movement`), swamp
        // ambience/weather, swamp wildlife/forage. Only the ground + scatter differ.
        TB::Swamp | TB::Blight => Some(Biome::Swamp),
        _ => None,
    }
}
pub fn is_grass_world(wx: f32, wz: f32) -> bool {
    matches!(tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32).map(|t| t.0), Some(TB::Grass))
}
fn tile_top_y_world(wx: f32, wz: f32) -> f32 {
    match tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32) {
        Some((_, h)) => (h - 1) as f32 * GROUND_STEP,
        None => 0.0,
    }
}

// ── Public sampling API (wildlife placement + ground-following) ──────────────────
/// Terrain top Y at world `(x, z)`; `None` over water / off the island. Wildlife uses
/// this to sit creatures flush on the ground and to reject water/off-map wander steps.
pub fn ground_at_world(wx: f32, wz: f32) -> Option<f32> {
    tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32).map(|(_, h)| (h - 1) as f32 * GROUND_STEP)
}
/// Biome at world `(x, z)` (`None` = grass / sand / water) — wildlife biome placement.
pub fn biome_at_world(wx: f32, wz: f32) -> Option<Biome> {
    tile_biome_world(wx, wz)
}

/// Is world `(x, z)` over the carved river channel (where the sea plane shows through the
/// terrain)? The real combined-map river — `bridges` scans this to span the actual water.
///
/// Must sample in the SAME space the tiles are classified in: `tile_at` builds each tile from
/// `classify(ix / MAP_SCALE, …)`, so the river that actually carves water lives at base coord
/// `(world + G) / MAP_SCALE`. Feeding the raw `world + G` (missing the `/ MAP_SCALE`) read the
/// formula 1.4× off — bridges spanned dry land far from the real river, and the horizontal
/// river branch produced absurdly long decks.
pub fn is_river_world(wx: f32, wz: f32) -> bool {
    is_river((wx + GX) / MAP_SCALE, (wz + GZ) / MAP_SCALE)
}

// ── Blended ground colour ───────────────────────────────────────────────────────
fn lin3(hex: u32) -> [f32; 3] {
    let l = lin(hex);
    [l[0], l[1], l[2]]
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn biome_col(b: TB) -> [f32; 3] {
    lin3(match b {
        TB::Grass => COL_GRASS,
        TB::Sand => COL_SAND,
        TB::Forest => COL_FOREST,
        TB::Rock => COL_ROCK,
        TB::Snow => COL_SNOW,
        TB::Desert => COL_DESERT,
        TB::Swamp => COL_SWAMP,
        TB::Blight => COL_BLIGHT,
    })
}

/// Biome interior colour at base-space (x,z). The region blend used to mix toward one
/// flat `biome_col` per blob, which left big biome interiors (especially the snowfield)
/// a single unbroken tone — only the grass frontier had macro-patches. Snow and forest
/// now carry their own interior mottle; the other biomes keep their flat base (their
/// scatter is dense enough to break the ground up).
fn biome_col_at(b: TB, x: f32, z: f32) -> [f32; 3] {
    let base = biome_col(b);
    match b {
        TB::Snow => {
            // Broad cool drift-shadow troughs + tighter wind-streak crests (sastrugi).
            let drift = smoothstep(0.15, 1.25, noise_a(x * 3.2 - 7.0, z * 3.2 + 19.0));
            let streak = smoothstep(0.55, 1.35, noise_b(x * 7.5 + 3.0, z * 2.4 - 13.0));
            let col = mix3(base, lin3(COL_SNOW_SHADE), drift * 0.50);
            mix3(col, lin3(COL_SNOW_BRIGHT), streak * 0.55)
        }
        TB::Forest => {
            // Dark moist loam patches under the canopy + dry leaf-litter speckle.
            let moist = smoothstep(0.20, 1.30, noise_a(x * 3.6 + 13.0, z * 3.6 - 5.0));
            let dry = smoothstep(0.35, 1.40, noise_b(x * 8.0 - 23.0, z * 8.0 + 7.0));
            let col = mix3(base, lin3(COL_FOREST_DARK), moist * 0.50);
            mix3(col, lin3(COL_FOREST_DRY), dry * 0.35)
        }
        TB::Blight => {
            // Churned-black trample troughs, dead-ash patches, and a sickly warp-green
            // seep — the mud should read beaten flat by ten thousand ork feet. Weights
            // run HOT (first pass read as one flat tan sheet from gameplay height).
            let churn = smoothstep(0.10, 1.10, noise_a(x * 4.1 + 11.0, z * 4.1 - 5.0));
            let ash = smoothstep(0.50, 1.25, noise_b(x * 2.6 - 9.0, z * 2.6 + 17.0));
            let seep = smoothstep(0.40, 1.20, noise_a(x * 1.8 + 31.0, z * 1.8 + 3.0));
            let col = mix3(base, lin3(COL_BLIGHT_DARK), churn * 0.70);
            let col = mix3(col, lin3(COL_BLIGHT_ASH), ash * 0.45);
            mix3(col, lin3(COL_BLIGHT_GREEN), seep * 0.40)
        }
        _ => base,
    }
}

/// Smooth blended ground colour at tile-space (x,z): grass base, each biome blob mixed in
/// over a soft `BLEND` band at its edge, plus a sandy coast fade.
fn ground_color(x: f32, z: f32) -> [f32; 4] {
    let mut col = lin3(COL_GRASS);
    // Meadow macro-patches on the grass base (before the biome-region blends, so biome
    // interiors keep their own colour): two noise octaves mottle the green between a
    // darker lush tone and a drier warm one. Open grass stops being one flat neon sheet —
    // the patchiness is what makes the ground read as a living meadow at camera distance.
    let p1 = noise_a(x * 4.0 + 31.0, z * 4.0 - 17.0); // ~12-world-tile patches
    let p2 = noise_b(x * 9.0 - 11.0, z * 9.0 + 23.0); // ~4-tile speckle
    let p3 = noise_a(x * 1.6 - 71.0, z * 1.6 + 59.0); // ~30-tile golden sweeps
    col = mix3(col, lin3(COL_GRASS_DARK), smoothstep(0.1, 1.3, p1) * 0.55);
    col = mix3(col, lin3(COL_GRASS_DRY), smoothstep(0.25, 1.4, p2) * 0.40);
    col = mix3(col, lin3(COL_GRASS_GOLD), smoothstep(0.55, 1.5, p3) * 0.45);
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    for reg in &REGIONS {
        let fray = if reg.peak > 0 { 0.0 } else { edge_fray(x, z) };
        let d = (x - reg.x).hypot(z - reg.z) + wob + fray;
        let edge = reg.r - d; // >0 inside
        let w = smoothstep(-BLEND, BLEND, edge);
        col = mix3(col, biome_col_at(reg.biome, x, z), w);
    }
    // The Blight blends in by its own shape (it's not a `Region` — the landmass lives in
    // `ork_fortress.rs`): the south-swamp green smears into the trampled mud over `BLEND`.
    let blight_w = smoothstep(-BLEND, BLEND, crate::ork_fortress::blight_edge_base(x, z));
    if blight_w > 0.0 {
        col = mix3(col, biome_col_at(TB::Blight, x, z), blight_w);
    }
    // Sandy coast fade — damped inside the Blight: `dist_from_coast` only knows the OLD
    // island shape, so without the damp the whole southern landmass would tint to beach.
    let dco = dist_from_coast(x, z) as f32;
    col = mix3(col, lin3(COL_SAND), smoothstep(3.5, 0.5, dco) * 0.85 * (1.0 - blight_w));
    // Universal mottle — value jitter + a slow warm/cool hue wander over the *blended*
    // colour, so biome interiors (forest, sand, snow…) get texture too, not just the open
    // grass. Multiplicative and small: it breaks the flat fill without recolouring a biome.
    let m1 = noise_a(x * 2.2 - 53.0, z * 2.2 + 41.0); // ~20-tile broad drift
    let m2 = noise_b(x * 14.0 + 7.0, z * 14.0 - 29.0); // ~3-tile speckle
    let v = 1.0 + 0.10 * m1 + 0.06 * m2;
    let warm = 0.05 * noise_a(x * 1.4 + 91.0, z * 1.4 - 77.0); // +red/−blue ↔ −red/+blue
    col = [
        (col[0] * v * (1.0 + warm)).clamp(0.0, 1.0),
        (col[1] * v).clamp(0.0, 1.0),
        (col[2] * v * (1.0 - warm)).clamp(0.0, 1.0),
    ];
    // Worn dirt approach-paths baked straight into the ground (not raised geometry — they're
    // just a brown blend in the terrain, like the original game's roads).
    let strength = crate::roads::road_strength(x - GX, z - GZ);
    if strength > 0.0 {
        col = mix3(col, lin3(ROAD_DIRT), strength * 0.85);
    }
    [col[0], col[1], col[2], 1.0]
}

/// Trampled-earth tint blended into the ground along the gate approach-roads.
const ROAD_DIRT: u32 = 0x8a6d44;

// ── Shore-distance field (water shader foam / shallows) ─────────────────────────
/// Max encoded shore distance, in tiles — keep in sync with `SHORE_MAX` in
/// `assets/shaders/water.wgsl`.
const SHORE_MAX: f32 = 8.0;

/// Bake an R8 distance-to-land field over the island (1 texel = 1 world unit, with a
/// sea margin all round): 0 on land ramping to 1 at ≥ [`SHORE_MAX`] tiles offshore.
/// The water shader samples it for the shore foam collar + shallow→deep gradient —
/// the terrain has **no underwater geometry** (cliff walls stop at the waterline), so
/// scene depth can't measure shallowness; this baked field is the only source.
/// Returns the image plus the world→UV mapping (`xy` = min corner, `zw` = 1/extent).
fn bake_shore_distance(images: &mut Assets<Image>) -> (Handle<Image>, Vec4) {
    const W: usize = 256; // covers world x ∈ [-128, 128] (island is ±108)
    const H: usize = 352; // covers world z ∈ [-176, 176] (island ±81 + the Blight to ~+160)
    let min_x = -(W as f32) / 2.0;
    let min_z = -(H as f32) / 2.0;
    let idx = |x: usize, z: usize| z * W + x;

    // Land mask → two-pass 8-neighbour chamfer distance transform (≈Euclidean).
    let mut d = vec![f32::INFINITY; W * H];
    for z in 0..H {
        for x in 0..W {
            let wx = min_x + x as f32 + 0.5;
            let wz = min_z + z as f32 + 0.5;
            if ground_at_world(wx, wz).is_some() {
                d[idx(x, z)] = 0.0;
            }
        }
    }
    const DIAG: f32 = 1.414;
    for z in 0..H {
        for x in 0..W {
            let mut v = d[idx(x, z)];
            if x > 0 { v = v.min(d[idx(x - 1, z)] + 1.0); }
            if z > 0 { v = v.min(d[idx(x, z - 1)] + 1.0); }
            if x > 0 && z > 0 { v = v.min(d[idx(x - 1, z - 1)] + DIAG); }
            if x + 1 < W && z > 0 { v = v.min(d[idx(x + 1, z - 1)] + DIAG); }
            d[idx(x, z)] = v;
        }
    }
    for z in (0..H).rev() {
        for x in (0..W).rev() {
            let mut v = d[idx(x, z)];
            if x + 1 < W { v = v.min(d[idx(x + 1, z)] + 1.0); }
            if z + 1 < H { v = v.min(d[idx(x, z + 1)] + 1.0); }
            if x + 1 < W && z + 1 < H { v = v.min(d[idx(x + 1, z + 1)] + DIAG); }
            if x > 0 && z + 1 < H { v = v.min(d[idx(x - 1, z + 1)] + DIAG); }
            d[idx(x, z)] = v;
        }
    }

    let data: Vec<u8> =
        d.iter().map(|v| ((v / SHORE_MAX).clamp(0.0, 1.0) * 255.0) as u8).collect();
    let mut img = Image::new(
        Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Linear filtering smooths the 1-texel bands; the default clamp-to-edge address
    // mode makes off-texture samples read the border (open sea = max distance).
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    let region = Vec4::new(min_x, min_z, 1.0 / W as f32, 1.0 / H as f32);
    (images.add(img), region)
}

// ── Build ────────────────────────────────────────────────────────────────────────
#[allow(clippy::too_many_arguments)]
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
    terrain_mats: &mut Assets<TerrainMaterial>,
    water_mats: &mut Assets<WaterMaterial>,
) {
    // ── Terraced ground mesh (blended vertex colours, per-face normals) — two sheets:
    //    the island proper on the grass detail texture, and the Blight on its own
    //    trampled-mud detail so the ork ground reads filthy instead of lawn-grained. ──
    let ground = build_terrain_mesh(|tb| tb != TB::Blight);
    let grass_detail = GroundDetail {
        scale: 0.18,
        strength: 0.40,
        variation: 0.70,
        seed: 1.0,
        dark: 0x356b28,
        base: 0x5d9e44,
        light: 0x95d162,
        grain: 0.55,
        streak: 0.5,
    };
    let ground_mat = crate::terrain::make_material(&grass_detail, 0.95, images, terrain_mats);
    commands.spawn((
        Mesh3d(meshes.add(ground)),
        MeshMaterial3d(ground_mat),
        Transform::default(),
        crate::biome::BiomeEntity,
    ));
    let blight_ground = build_terrain_mesh(|tb| tb == TB::Blight);
    let blight_detail = GroundDetail {
        scale: 0.30,
        strength: 0.62,
        variation: 0.85,
        seed: 7.0,
        dark: 0x241a10,
        base: 0x4d3e2a,
        light: 0x6e5f46,
        grain: 0.95,
        streak: 0.65,
    };
    let blight_mat = crate::terrain::make_material(&blight_detail, 0.97, images, terrain_mats);
    commands.spawn((
        Mesh3d(meshes.add(blight_ground)),
        MeshMaterial3d(blight_mat),
        Transform::default(),
        crate::biome::BiomeEntity,
    ));

    // ── Sea (big animated water plane under everything; shows at coast/rivers/lake) ──
    let sea_mesh = meshes.add(Plane3d::default().mesh().size(900.0, 900.0).subdivisions(8).build());
    let (shore_tex, shore_region) = bake_shore_distance(images);
    let sea = water_mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(0x2f as f32 / 255.0, 0x6f as f32 / 255.0, 0xae as f32 / 255.0, 0.9),
            perceptual_roughness: 0.3,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        },
        extension: WaterExt {
            params: WaterParams {
                // x amp, y freq, z scroll, w fresnel strength.
                params: Vec4::new(0.16, 0.45, 0.4, 0.85),
                // rgb sky tint at grazing angles, w shore-fx strength (foam collar +
                // shallow→deep gradient in water.wgsl).
                sky_tint: Vec4::new(0.70, 0.82, 0.93, 1.0),
                region: shore_region,
            },
            shore: Some(shore_tex),
        },
    });
    commands.spawn((
        Mesh3d(sea_mesh),
        MeshMaterial3d(sea),
        Transform::from_xyz(0.0, SEA_Y, 0.0),
        crate::biome::BiomeEntity,
    ));

    // ── Background sailboats on the open water around the island ──
    // Island shape is authored in BASE space (CX/CZ, ISLAND_R*); convert to world:
    // world = base*MAP_SCALE - G. Centre ≈ origin; radii scale by MAP_SCALE.
    let isle_c = Vec2::new(CX * MAP_SCALE - GX, CZ * MAP_SCALE - GZ);
    let isle_r = Vec2::new(ISLAND_RX * MAP_SCALE, ISLAND_RZ * MAP_SCALE);
    crate::boats::spawn_boats_island(commands, meshes, std_mats, isle_c, isle_r, SEA_Y);

    // ── Plan the ork camps BEFORE scatter so their clearings can be reserved. ──
    crate::camps::plan();

    // ── Scatter: each biome's props on its tiles (height-aware), + grass cover ──
    let lo = -GX;
    let hi = GX; // square covers the whole grid; off-map tiles mask out
    // Capture each biome's atmosphere + weather while we already have its config in hand, so
    // the hero-region transition system can lerp toward it without rebuilding configs per frame.
    let mut ambiences: Vec<(Biome, BiomeAmbience)> = Vec::new();
    for biome in [Biome::Forest, Biome::Snow, Biome::Rocky, Biome::Desert, Biome::Swamp] {
        let cfg = config_for(biome);
        ambiences.push((
            biome,
            BiomeAmbience { atmo: AtmoSample::from_config(&cfg), particle: cfg.particle },
        ));
        scatter_region(
            &cfg,
            commands,
            meshes,
            std_mats,
            lo,
            hi,
            false,
            &move |x, z| {
                tile_biome_world(x, z) == Some(biome)
                    && !crate::camps::in_clearing(x, z)
                    && !crate::bridges::near_bridge(x, z, 1.0)
                    // Keep swamp scatter off the fortress gate road, and out of the deep
                    // Blight (the Blight tiles read as Swamp; ork_fortress scatters its own
                    // dead-wood there — swamp props only bleed into the northern blend band).
                    && !crate::ork_fortress::on_gate_approach(x, z)
                    && !(crate::ork_fortress::in_blight_world(x, z) && z > 86.0)
            },
            &|x, z| tile_top_y_world(x, z),
        );
    }
    // Island-wide base ambience (grass / sand / water): the shared daytime atmosphere, no weather.
    let (sky, _fog, sun_c, sun_i, amb_c, amb_b, _sun_p) = ATMOSPHERE;
    commands.insert_resource(BiomeAmbiences {
        base: BiomeAmbience {
            atmo: AtmoSample::from_raw(sky, sun_c, sun_i, amb_c, amb_b),
            particle: ParticleKind::None,
        },
        list: ambiences,
    });
    // ── Snow drifts banked against terrace walls: where a snow tile abuts a higher
    // neighbour, pile a wind-drift mound into the corner so the cliff faces rise out of
    // drifted snow instead of standing naked on a flat field. Deterministic per tile.
    {
        let drift_mat = std_mats.add(StandardMaterial {
            base_color: Color::WHITE, // vertex colour carries the hue (mesh contract)
            perceptual_roughness: 0.62,
            reflectance: 0.5,
            ..default()
        });
        let drift_meshes: Vec<Handle<Mesh>> =
            (0..3).map(|v| meshes.add(crate::biome_snow::build_mound_mesh(v))).collect();
        for iz in 0..ROWS {
            for ix in 0..COLS {
                let Some((TB::Snow, h)) = tile_at(ix, iz) else { continue };
                let mut rng =
                    tileworld_core::rng::Mulberry32::new((iz * COLS + ix) as u32 ^ 0x5eed_d81f);
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let Some((_, nh)) = tile_at(ix + dx, iz + dz) else { continue };
                    if nh <= h || rng.next() > 0.35 {
                        continue;
                    }
                    // Pull the mound off the shared edge into THIS (lower) tile so it
                    // leans against the wall base.
                    let wx = ix as f32 - GX + 0.5 - dx as f32 * 0.32
                        + (rng.next() as f32 - 0.5) * 0.3;
                    let wz = iz as f32 - GZ + 0.5 - dz as f32 * 0.32
                        + (rng.next() as f32 - 0.5) * 0.3;
                    if crate::bridges::near_bridge(wx, wz, 0.5) {
                        continue; // keep drift mounds off the plank decks
                    }
                    let v = (rng.next() * 3.0) as u32;
                    commands.spawn((
                        Mesh3d(drift_meshes[(v % 3) as usize].clone()),
                        MeshMaterial3d(drift_mat.clone()),
                        Transform {
                            translation: Vec3::new(wx, (h - 1) as f32 * GROUND_STEP, wz),
                            rotation: Quat::from_rotation_y(rng.next() as f32 * std::f32::consts::TAU),
                            scale: Vec3::splat(0.9 + rng.next() as f32 * 0.7),
                        },
                        crate::biome::BiomeEntity,
                    ));
                }
            }
        }
    }

    // Grass frontier cover (tufts/clover/flowers) on grass tiles.
    let grass_cfg = grass_config();
    scatter_region(
        &grass_cfg,
        commands,
        meshes,
        std_mats,
        lo,
        hi,
        false,
        &|x, z| {
            is_grass_world(x, z)
                && !crate::castle::in_footprint(x, z)
                && !crate::camps::in_clearing(x, z)
                && !crate::town::near_build_plot(x, z)
                && !crate::bridges::near_bridge(x, z, 1.0)
        },
        &|x, z| tile_top_y_world(x, z),
    );

    // ── Central castle (fully built) on the flat grass centre ──
    // Returns the shared textured material set so the town plots/buildings match the keep.
    let village_mats = crate::castle::build(commands, meshes, images, std_mats);

    // ── Ork camps: tents/fire/banner/cage + a patrolling warband (registers blockers). ──
    crate::camps::build(commands, meshes, images, std_mats);

    // ── Castle townsfolk: ambient villagers milling the courtyard + gates. ──
    crate::villagers::populate(commands, meshes, std_mats);

    // ── Training dummies: practice pells in the keep courtyard (hit-feedback only). ──
    crate::training_dummies::populate(commands, meshes, std_mats);

    // ── Ambient wildlife — biome-placed animals that wander/graze/startle ──
    crate::wildlife::populate(commands, meshes, std_mats);

    // ── Biome verbs: mineable ore (rock), forage (swamp herbs / forest apples), chests ──
    crate::verbs::populate_ore(commands, meshes, std_mats);
    crate::town::populate_plots(commands, meshes, &village_mats);
    crate::verbs::populate_forage(commands, meshes, std_mats);
    crate::verbs::populate_chests(commands, meshes, std_mats);

    // ── Castle defenses: tower/archer/ballista fire emitters (upgrade-gated at runtime) ──
    crate::defenses::populate_defenders(commands, meshes, std_mats);

    // ── Signature landmarks: one per biome region (frozen spire, pyramid, trilithon, …) ──
    crate::ruins::populate_landmarks(commands, meshes, std_mats);

    // ── Biome vignettes: one mute set-piece story per region (abandoned camp, lost caravan,
    //    collapsed watchtower, …) — discoverable POIs + pilgrim destinations. After the landmarks
    //    so each routes around the ruin already planted in its biome.
    crate::vignettes::populate_vignettes(commands, meshes, std_mats);

    // ── Gnashfang Hold + the Blight: the ork fortress and its walkable poisoned landmass
    //    south of the old coast (camp props, patrols, watchtowers; see `ork_fortress.rs`).
    crate::ork_fortress::build(commands, meshes, images, std_mats);

    // ── River bridges: plank decks at the real river crossings (also nav-grid walkable). ──
    crate::bridges::populate(commands, meshes, std_mats);
    // (Roads are NOT spawned as geometry — they're baked into the ground colour by
    //  `ground_color` via `roads::road_strength`, so they read as a brown blend in the terrain.)

    // No distant horizon hills — open ocean fading into fog reads cleaner.
}

fn config_for(b: Biome) -> BiomeConfig {
    match b {
        Biome::Forest => crate::biome_forest::config(),
        Biome::Snow => crate::biome_snow::config(),
        Biome::Rocky => crate::biome_rocky::config(),
        Biome::Desert => crate::biome_desert::config(),
        Biome::Swamp => crate::biome_swamp::config(),
    }
}

/// A cover-only pseudo-config for the open grass frontier (no trees/rocks).
fn grass_config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Forest,
        name: "Grass",
        ground_color: COL_GRASS,
        ground_roughness: 0.95,
        detail: GroundDetail {
            scale: 0.18, strength: 0.4, variation: 0.7, seed: 1.0,
            dark: 0x356b28, base: 0x5d9e44, light: 0x95d162, grain: 0.55, streak: 0.5,
        },
        sky: 0xb4d2ec, fog_density: 0.0035, sun_color: 0xffedc7, sun_illuminance: 11_000.0,
        ambient_color: 0xe6edf5, ambient_brightness: 95.0, sun_pos: Vec3::new(80.0, 110.0, 40.0),
        seed: 7777,
        tree_min_dist: 2.0,
        classes: vec![],
        cover: vec![
            PropClass { variants: vec![(gc::build_grass_tuft_mesh(), 1.0)], chance: 0.26, scale: (0.45, 0.8), tree: false, block_radius: 0.0 },
            PropClass { variants: vec![(gc::build_clover_mesh(), 1.0)], chance: 0.30, scale: (0.7, 1.2), tree: false, block_radius: 0.0 },
            PropClass { variants: vec![(gc::build_fern_mesh(), 1.0)], chance: 0.10, scale: (0.5, 0.85), tree: false, block_radius: 0.0 },
            PropClass {
                variants: (0..3).map(|v| (gc::build_flower_mesh(v), 1.0)).collect(),
                chance: 0.12,
                scale: (0.8, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: (0..2).map(|v| (gc::build_mushroom_mesh(v), 1.0)).collect(),
                chance: 0.05,
                scale: (0.6, 1.0),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover_per_tile: 2,
        river: false,
        river_color: 0x2f8fd6,
        backdrop: Backdrop {
            land_dir: 0.0, land_arc: std::f32::consts::PI, ocean: false, ocean_color: 0x2f6fae,
            hill_body: 0x8f9aa0, hill_cap: 0xb8c2c6, hill_foot: 0x7a8890,
            treeline: false, treeline_dark: 0x2c4a34, treeline_mid: 0x365c3e, hill_h: (40.0, 90.0),
        },
        particle: ParticleKind::None,
    }
}

// ── Terraced terrain mesh ─────────────────────────────────────────────────────────
/// Build the terraced ground mesh over every tile `keep` accepts. Called twice from
/// `build`: once for everything but the Blight (grass detail texture), once for the
/// Blight alone (its own trampled-mud detail) — two sheets, identical recipe.
fn build_terrain_mesh(keep: impl Fn(TB) -> bool) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let quad =
        |p: [[f32; 3]; 4], n: [f32; 3], c: [[f32; 4]; 4], idx: &mut Vec<u32>, pos: &mut Vec<[f32; 3]>, nrm: &mut Vec<[f32; 3]>, col: &mut Vec<[f32; 4]>| {
            let b = pos.len() as u32;
            for k in 0..4 {
                pos.push(p[k]);
                nrm.push(n);
                col.push(c[k]);
            }
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        };

    const NB: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

    for iz in 0..ROWS {
        for ix in 0..COLS {
            let Some((tb, h)) = tile_at(ix, iz) else { continue };
            if !keep(tb) {
                continue;
            }
            let top = (h - 1) as f32 * GROUND_STEP;
            let wx = ix as f32 - GX;
            let wz = iz as f32 - GZ;

            // Top quad (per-corner blended colour). Colour samples in BASE space.
            let c = |cx: f32, cz: f32| ground_color(cx / MAP_SCALE, cz / MAP_SCALE);
            quad(
                [[wx, top, wz], [wx + 1.0, top, wz], [wx + 1.0, top, wz + 1.0], [wx, top, wz + 1.0]],
                [0.0, 1.0, 0.0],
                [c(ix as f32, iz as f32), c(ix as f32 + 1.0, iz as f32), c(ix as f32 + 1.0, iz as f32 + 1.0), c(ix as f32, iz as f32 + 1.0)],
                &mut indices, &mut positions, &mut normals, &mut colors,
            );

            // Walls down to lower neighbours / water. Grass-biome cliffs show exposed
            // dirt (lighter soil at the lip, darker toward the base, slight per-tile
            // jitter); every other biome just darkens its own top colour.
            let top_col = ground_color((ix as f32 + 0.5) / MAP_SCALE, (iz as f32 + 0.5) / MAP_SCALE);
            let (wall_top, wall_bot) = if tb == TB::Grass {
                let j = 0.82 + 0.32 * (noise_b(ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE) * 0.5 + 0.5);
                let d = lin3(COL_DIRT);
                let top = [d[0] * j, d[1] * j, d[2] * j, 1.0];
                // Base lifted off the old 0.55× — in cliff shadow at low sun that
                // multiplied down to pure black pits; shaded soil, not holes.
                let bot = [d[0] * j * 0.68, d[1] * j * 0.64, d[2] * j * 0.60, 1.0];
                (top, bot)
            } else if tb == TB::Snow {
                // Bright snow lip overhanging exposed blue-grey rock: the warm/cool value
                // split that makes the terraces read as carved drifts over stone instead
                // of foam blocks in the same cream as the snowfield tops.
                let lip = lin3(SNOW_CLIFF_LIP);
                let rock = lin3(SNOW_CLIFF_ROCK);
                ([lip[0], lip[1], lip[2], 1.0], [rock[0], rock[1], rock[2], 1.0])
            } else {
                // Graded lip→base (was a flat 0.5× that went black at dusk): the lip keeps
                // most of the top tone so the edge reads, the base darkens but stays lit.
                let top = [top_col[0] * 0.80, top_col[1] * 0.78, top_col[2] * 0.76, 1.0];
                let bot = [top_col[0] * 0.58, top_col[1] * 0.56, top_col[2] * 0.54, 1.0];
                (top, bot)
            };
            // Wall quad corner order is [bottom, bottom, top, top].
            let wc = [wall_bot, wall_bot, wall_top, wall_top];
            for (dx, dz) in NB {
                let nh_top = match tile_at(ix + dx, iz + dz) {
                    Some((_, nh)) => (nh - 1) as f32 * GROUND_STEP,
                    None => SEA_Y, // coast / river / lake bank
                };
                if top <= nh_top + 1e-4 {
                    continue;
                }
                // Shared edge between this tile and the neighbour, vertical nh_top..top.
                let (e0, e1, n): ([f32; 2], [f32; 2], [f32; 3]) = match (dx, dz) {
                    (1, 0) => ([wx + 1.0, wz], [wx + 1.0, wz + 1.0], [1.0, 0.0, 0.0]),
                    (-1, 0) => ([wx, wz + 1.0], [wx, wz], [-1.0, 0.0, 0.0]),
                    (0, 1) => ([wx + 1.0, wz + 1.0], [wx, wz + 1.0], [0.0, 0.0, 1.0]),
                    _ => ([wx, wz], [wx + 1.0, wz], [0.0, 0.0, -1.0]),
                };
                quad(
                    [[e0[0], nh_top, e0[1]], [e1[0], nh_top, e1[1]], [e1[0], top, e1[1]], [e0[0], top, e0[1]]],
                    n,
                    wc,
                    &mut indices, &mut positions, &mut normals, &mut colors,
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

