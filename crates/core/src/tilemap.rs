//! Port of src/world/tileMap.ts — map constants + the full procedural generator.
//!
//! The island is RESAMPLED from a 144×108 "base" map onto a 202×152 grid; every
//! new tile reads the base generation at `to_base(x,z)`, keeping the exact shape
//! just bigger. Generation runs in BASE space; anchors authored in base coords
//! convert via `from_base` (wilderness, scaled about centre) or `shift_to_centre`
//! (the castle, kept ABSOLUTE size).
//!
//! f64 throughout for JS-`number` parity. `f64::sin/cos/atan2/hypot` == JS
//! `Math.*` (IEEE libm). `f64::round` rounds half away from zero vs JS half
//! toward +inf, but every rounded coord here is positive in-map so it matches.

use crate::bridges::bridge_at;
use std::sync::OnceLock;

// ─── Map size + the 1.4× expansion transform ────────────────────────────────
pub const MAP_SCALE: f64 = 1.4;
pub const BASE_COLS: i32 = 144;
pub const BASE_ROWS: i32 = 108;
pub const COLS: i32 = 202; // ~= 144 * 1.4
pub const ROWS: i32 = 152; // ~= 108 * 1.4

pub const CENTER_X: f64 = COLS as f64 / 2.0; // 101
pub const CENTER_Z: f64 = ROWS as f64 / 2.0; // 76

const BASE_CENTER_X: f64 = BASE_COLS as f64 / 2.0; // 72
const BASE_CENTER_Z: f64 = BASE_ROWS as f64 / 2.0; // 54

/// Per-axis scale (COLS/ROWS rounded to keep CENTER integral).
pub const SCALE_X: f64 = COLS as f64 / BASE_COLS as f64;
pub const SCALE_Z: f64 = ROWS as f64 / BASE_ROWS as f64;

/// World-Y per height class (one class = half a tile-unit tall).
pub const GROUND_STEP: f64 = 0.5;

/// Castle sits at the true map centre.
pub const CASTLE_CENTER_X: f64 = CENTER_X;
pub const CASTLE_CENTER_Z: f64 = CENTER_Z;

/// New-space grass safe-zone radius = round(18 * SCALE_X) = 25 on the 202-wide
/// map. Hardcoded because `f64::round` isn't a const fn; the derivation is
/// pinned by `tests::castle_safe_r_matches_derivation`.
pub const CASTLE_SAFE_R: f64 = 25.0;

// BASE-space castle constants — used ONLY by terrain generation (runs in base
// coords). The resample maps the base safe-zone disc onto the new centre.
const BASE_CASTLE_CENTER_X: f64 = BASE_CENTER_X;
const BASE_CASTLE_CENTER_Z: f64 = BASE_CENTER_Z;
const BASE_CASTLE_SAFE_R: f64 = 18.0;

// ─── Biome / Tile ───────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Biome {
    Grass,
    Sand,
    Forest,
    Rock,
    Snow,
    Desert,
    Plains,
    Swamp,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub biome: Biome,
    pub height: i32,
}

// ─── Coordinate transforms ───────────────────────────────────────────────────
/// New grid coord → base/original-map coord. Generation samples the base map
/// here, so the bigger map is the original stretched (same shape).
fn to_base(x: f64, z: f64) -> (f64, f64) {
    (
        BASE_CENTER_X + (x - CENTER_X) / SCALE_X,
        BASE_CENTER_Z + (z - CENTER_Z) / SCALE_Z,
    )
}

/// Base/original WILDERNESS anchor coord → new grid coord (scaled about centre).
pub fn from_base(x: f64, z: f64) -> (f64, f64) {
    (
        CENTER_X + (x - BASE_CENTER_X) * SCALE_X,
        CENTER_Z + (z - BASE_CENTER_Z) * SCALE_Z,
    )
}

/// Base/original CASTLE-attached coord → new grid coord by pure translation.
pub fn shift_to_centre(x: f64, z: f64) -> (f64, f64) {
    (x + (CENTER_X - BASE_CENTER_X), z + (CENTER_Z - BASE_CENTER_Z))
}

// ─── Noise ───────────────────────────────────────────────────────────────────
fn noise_a(x: f64, z: f64) -> f64 {
    (x * 0.13 + 1.7).sin() * (z * 0.11 - 2.3).cos() + (x * 0.31 + z * 0.29 + 4.5).sin() * 0.5
}
fn noise_b(x: f64, z: f64) -> f64 {
    (x * 0.21 - 3.1).sin() * (z * 0.19 + 0.7).cos() + ((x + z) * 0.07 + 5.2).sin() * 0.4
}

/// Distance (tiles) from the castle centre — drives the flat safe-zone. Runs in
/// BASE space (generation only).
fn dist_from_castle(x: f64, z: f64) -> f64 {
    (x - BASE_CASTLE_CENTER_X).hypot(z - BASE_CASTLE_CENTER_Z)
}

// ─── Plateaus (climbable grass hills) ────────────────────────────────────────
struct Plateau {
    x: f64,
    z: f64,
    r: f64,
    peak: i32,
}
const PLATEAUS: [Plateau; 2] = [
    Plateau { x: 98.0, z: 72.0, r: 9.0, peak: 5 }, // SE grass belt
    Plateau { x: 52.0, z: 50.0, r: 7.0, peak: 4 }, // W grass belt
];

/// Plateau height class at (x,z): 0 = none, else 2..peak stepped by distance.
fn plateau_height_at(x: f64, z: f64) -> i32 {
    for p in PLATEAUS.iter() {
        let d = (x - p.x).hypot(z - p.z);
        if d >= p.r {
            continue;
        }
        let tiers = p.peak - 1;
        let cls = 2 + ((1.0 - d / p.r) * tiers as f64).floor() as i32;
        return cls.min(p.peak).max(2);
    }
    0
}

// ─── Island shape ─────────────────────────────────────────────────────────────
const ISLAND_RX: f64 = BASE_COLS as f64 / 2.0 - 1.0;
const ISLAND_RZ: f64 = BASE_ROWS as f64 / 2.0 - 1.0;
const ISLAND_EXP: f64 = 2.6;

fn is_land_shape(x: f64, z: f64) -> bool {
    let dx = (x - BASE_CENTER_X).abs() / ISLAND_RX;
    let dz = (z - BASE_CENTER_Z).abs() / ISLAND_RZ;
    let r = dx.powf(ISLAND_EXP) + dz.powf(ISLAND_EXP);
    let coast = noise_a(x, z) * 0.08;
    r + coast < 1.0
}

fn dist_from_coast(x: f64, z: f64) -> i32 {
    let mut min = 10;
    let dirs: [(f64, f64); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (1.0, 1.0),
        (1.0, -1.0),
        (-1.0, 1.0),
        (-1.0, -1.0),
    ];
    for (dx, dz) in dirs {
        for d in 1..=10 {
            if !is_land_shape(x + dx * d as f64, z + dz * d as f64) {
                if d < min {
                    min = d;
                }
                break;
            }
        }
    }
    min
}

// ─── Rivers + lakes ──────────────────────────────────────────────────────────
fn river_x(z: f64) -> f64 {
    40.0 + (z * 0.18).sin() * 5.0 + (z * 0.07 + 1.4).sin() * 3.0
}
fn river_z(x: f64) -> f64 {
    20.0 + (x * 0.13 + 0.7).sin() * 4.0
}

fn is_river_at(x: f64, z: f64) -> bool {
    if dist_from_castle(x, z) < BASE_CASTLE_SAFE_R {
        return false;
    }
    if in_mountain(x, z) {
        return false;
    }
    {
        let cx = river_x(z);
        let w = 0.75 + (z * 0.5).sin() * 0.2;
        if (x - cx).abs() < w {
            return true;
        }
    }
    if x > 46.0 && x < BASE_COLS as f64 - 10.0 {
        let cz = river_z(x);
        if (z - cz).abs() < 0.7 {
            return true;
        }
    }
    false
}

// One hand-placed lake SE of the castle.
const DELIBERATE_LAKE_X: f64 = 92.0;
const DELIBERATE_LAKE_Z: f64 = 80.0;
const DELIBERATE_LAKE_RX: f64 = 5.0;
const DELIBERATE_LAKE_RZ: f64 = 3.0;
fn is_deliberate_lake(x: f64, z: f64) -> bool {
    let dx = (x - DELIBERATE_LAKE_X) / DELIBERATE_LAKE_RX;
    let dz = (z - DELIBERATE_LAKE_Z) / DELIBERATE_LAKE_RZ;
    dx * dx + dz * dz < 1.0
}

pub fn get_river_x(z: f64) -> f64 {
    river_x(z)
}
pub fn get_river_z(x: f64) -> f64 {
    river_z(x)
}

// ─── Regions (biome blobs) ───────────────────────────────────────────────────
struct Region {
    x: f64,
    z: f64,
    r: f64,
    biome: Biome,
    /// centre height class for mountain biomes (rock/snow); None for flat.
    peak: Option<i32>,
    /// azimuth (radians) of the climbable ramp; None → faces the castle.
    ramp_ang: Option<f64>,
}

const REGIONS: [Region; 5] = [
    Region { x: 26.0, z: 24.0, r: 26.0, biome: Biome::Snow, peak: Some(9), ramp_ang: None },
    Region { x: 112.0, z: 28.0, r: 34.0, biome: Biome::Desert, peak: None, ramp_ang: None },
    Region { x: 122.0, z: 58.0, r: 22.0, biome: Biome::Rock, peak: Some(15), ramp_ang: None },
    Region { x: 32.0, z: 80.0, r: 34.0, biome: Biome::Forest, peak: None, ramp_ang: None },
    Region { x: 72.0, z: 92.0, r: 32.0, biome: Biome::Swamp, peak: None, ramp_ang: None },
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionInfo {
    pub x: f64,
    pub z: f64,
    pub r: f64,
}

/// The biome blob for `biome` in NEW space (centre + radius), or None.
pub fn region_by_biome(biome: Biome) -> Option<RegionInfo> {
    let r = REGIONS.iter().find(|reg| reg.biome == biome)?;
    let (x, z) = from_base(r.x, r.z);
    Some(RegionInfo {
        x,
        z,
        r: r.r * SCALE_X,
    })
}

// ─── scatterInRegion ─────────────────────────────────────────────────────────
const SCATTER_INNER: f64 = 0.55;
const SCATTER_OUTER: f64 = 0.95;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScatterPoint {
    pub x: f64,
    pub z: f64,
    pub seed: f64,
}

/// `n` deterministic golden-angle scatter points on a biome's outer-rim annulus
/// (0.55·r .. 0.95·r). Keeps only points whose tile resolves to `biome`.
pub fn scatter_in_region(biome: Biome, n: i32) -> Vec<ScatterPoint> {
    let reg = match region_by_biome(biome) {
        Some(r) => r,
        None => return vec![],
    };
    const GOLDEN: f64 = 2.39996323;
    let i2 = SCATTER_INNER * SCATTER_INNER;
    let span = SCATTER_OUTER * SCATTER_OUTER - i2;
    let mut pts: Vec<ScatterPoint> = Vec::new();
    for i in 0..n {
        let rad = reg.r * (i2 + ((i as f64 + 0.5) / n as f64) * span).sqrt();
        let ang = i as f64 * GOLDEN + reg.x;
        pts.push(ScatterPoint {
            x: reg.x + ang.cos() * rad,
            z: reg.z + ang.sin() * rad,
            seed: (i as f64 * 0.6180339 + 0.13) % 1.0,
        });
    }
    pts.into_iter()
        .filter(|p| {
            tile_at(p.x.floor() as i32, p.z.floor() as i32)
                .map(|t| t.biome == biome)
                .unwrap_or(false)
        })
        .collect()
}

// ─── Mountain masking / edge fray / ramps ────────────────────────────────────
fn in_mountain(x: f64, z: f64) -> bool {
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    for reg in REGIONS.iter() {
        if reg.peak.is_none() {
            continue;
        }
        if (x - reg.x).hypot(z - reg.z) + wob < reg.r + 2.0 {
            return true;
        }
    }
    false
}

fn edge_fray(x: f64, z: f64) -> f64 {
    (x * 0.5 + z * 0.35 + 1.3).sin() * 1.1
        + (x * 0.9 - z * 0.82 + 4.0).sin() * 1.6
        + (x * 1.5 + z * 1.3 + 2.2).sin() * 1.0
}

fn region_at(x: f64, z: f64) -> Option<&'static Region> {
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    let mut best: Option<&'static Region> = None;
    let mut best_edge = f64::INFINITY;
    for reg in REGIONS.iter() {
        let fray = if reg.peak.is_none() { edge_fray(x, z) } else { 0.0 };
        let d = (x - reg.x).hypot(z - reg.z) + wob + fray;
        let edge = d - reg.r;
        if edge < 0.0 && edge < best_edge {
            best_edge = edge;
            best = Some(reg);
        }
    }
    best
}

const RAMP_HALF_TILES: f64 = 1.7;

/// Climbable-ramp height class at (x,z) for mountain region `reg`, or None.
fn ramp_class(x: f64, z: f64, reg: &Region) -> Option<i32> {
    let peak = reg.peak?;
    let dx = x - reg.x;
    let dz = z - reg.z;
    let dc = dx.hypot(dz);
    if dc >= reg.r {
        return None;
    }
    let ramp_ang = reg
        .ramp_ang
        .unwrap_or_else(|| (BASE_CASTLE_CENTER_Z - reg.z).atan2(BASE_CASTLE_CENTER_X - reg.x));
    let mut da = (dz.atan2(dx) - ramp_ang) % (std::f64::consts::PI * 2.0);
    if da < -std::f64::consts::PI {
        da += std::f64::consts::PI * 2.0;
    }
    if da > std::f64::consts::PI {
        da -= std::f64::consts::PI * 2.0;
    }
    let half_ang = std::f64::consts::PI.min(RAMP_HALF_TILES / 1.5_f64.max(dc));
    if da.abs() >= half_ang {
        return None;
    }
    let span = 1.max(peak - 2);
    let step_len = reg.r / span as f64;
    let cls = 2 + ((reg.r - dc) / step_len).floor() as i32;
    Some(cls.max(2).min(peak))
}

/// True if (x,z) (NEW-space tile) lies in any mountain's ramp corridor.
pub fn is_mountain_ramp_tile(x: i32, z: i32) -> bool {
    let (bx, bz) = to_base(x as f64, z as f64);
    for reg in REGIONS.iter() {
        if ramp_class(bx, bz, reg).is_some() {
            return true;
        }
    }
    false
}

/// Mountain height at (x,z): the ramp staircase on the corridor, else a
/// quadratic profile with t-scaled noise (steep core, gentle apron).
fn mountain_height(x: f64, z: f64, reg: &Region) -> i32 {
    if let Some(rc) = ramp_class(x, z, reg) {
        return rc;
    }
    let dc = (x - reg.x).hypot(z - reg.z);
    let peak = reg.peak.unwrap_or(6);
    let t = (1.0 - dc / reg.r).max(0.0);
    let h = (peak as f64 * t * t + noise_b(x, z) * (0.35 + t * 0.95)).round() as i32;
    h.max(1).min(peak)
}

// ─── classifyBiome ───────────────────────────────────────────────────────────
fn classify_biome(x: f64, z: f64) -> Option<Tile> {
    if !is_land_shape(x, z) {
        return None;
    }

    let dc = dist_from_castle(x, z);
    if dc < BASE_CASTLE_SAFE_R + (-4.0_f64).max(edge_fray(x, z)) {
        return Some(Tile { biome: Biome::Grass, height: 1 });
    }

    if is_river_at(x, z) {
        return None;
    }

    let d = dist_from_coast(x, z);
    if is_deliberate_lake(x, z) {
        return None;
    }

    let beach_w = 1.0
        + (0.0_f64).max(
            1.0 + (x * 0.6 + z * 0.42 + 2.1).sin() * 0.8 + (x * 1.25 - z * 0.95 + 0.4).sin() * 0.6,
        );
    if (d as f64) <= beach_w {
        return Some(Tile { biome: Biome::Sand, height: 1 });
    }

    let ph = plateau_height_at(x, z);
    if ph != 0 {
        return Some(Tile { biome: Biome::Grass, height: ph });
    }

    if let Some(reg) = region_at(x, z) {
        if reg.biome == Biome::Swamp && dc < BASE_CASTLE_SAFE_R {
            return Some(Tile { biome: Biome::Grass, height: 1 });
        }
        if reg.peak.is_some() {
            return Some(Tile {
                biome: reg.biome,
                height: mountain_height(x, z, reg),
            });
        }
        return Some(Tile { biome: reg.biome, height: 1 });
    }

    let forest_n = noise_a(x, z) * noise_b(x + 7.0, z - 3.0);
    if forest_n > 0.5 {
        return Some(Tile { biome: Biome::Forest, height: 1 });
    }

    Some(Tile { biome: Biome::Grass, height: 1 })
}

// ─── Tile cache ──────────────────────────────────────────────────────────────
static TILES: OnceLock<Vec<Vec<Option<Tile>>>> = OnceLock::new();

fn ensure_tiles() -> &'static Vec<Vec<Option<Tile>>> {
    TILES.get_or_init(|| {
        let mut rows: Vec<Vec<Option<Tile>>> = Vec::with_capacity(ROWS as usize);
        for z in 0..ROWS {
            let mut row: Vec<Option<Tile>> = Vec::with_capacity(COLS as usize);
            for x in 0..COLS {
                let (bx, bz) = to_base(x as f64, z as f64);
                let t = classify_biome(bx, bz);
                match t {
                    None => row.push(None),
                    Some(t) => {
                        // Height re-sampled at the base GRID tile this falls in.
                        let q = classify_biome(bx.round(), bz.round());
                        let height = q.map(|q| q.height).unwrap_or(t.height);
                        row.push(Some(Tile { biome: t.biome, height }));
                    }
                }
            }
            rows.push(row);
        }
        rows
    })
}

pub fn build_tiles() -> &'static Vec<Vec<Option<Tile>>> {
    ensure_tiles()
}

pub fn tile_at(x: i32, z: i32) -> Option<Tile> {
    if x < 0 || z < 0 || x >= COLS || z >= ROWS {
        return None;
    }
    ensure_tiles()[z as usize][x as usize]
}

pub fn is_land(x: i32, z: i32) -> bool {
    tile_at(x, z).is_some()
}

/// World-Y of the walkable top of tile (x,z). Base ground (height 1) at y=1;
/// water / off-map → 0.
pub fn tile_top_y(x: i32, z: i32) -> f64 {
    match tile_at(x, z) {
        None => 0.0,
        Some(t) => 1.0 + (t.height - 1) as f64 * GROUND_STEP,
    }
}

/// Height class at a tile center, treating a bridge span as class 1. None = not
/// standable (open water / off-map). Bridges are LIVE (queried, never cached).
fn height_class_at(cx: i32, cz: i32) -> Option<i32> {
    if let Some(t) = tile_at(cx, cz) {
        return Some(t.height);
    }
    if bridge_at(cx as f64 + 0.5, cz as f64 + 0.5).is_some() {
        return Some(1);
    }
    None
}

/// True if an entity can stand on tile (cx,cz): any land height or bridge deck.
pub fn standable(cx: i32, cz: i32) -> bool {
    if cx < 0 || cz < 0 || cx >= COLS || cz >= ROWS {
        return false;
    }
    height_class_at(cx, cz).is_some()
}

/// Shared climb rule: a step is allowed when target is standable and the
/// height-class difference is ≤ 1 (Δ ≥ 2 face is a cliff).
pub fn can_step(fx: i32, fz: i32, tx: i32, tz: i32) -> bool {
    let tc = match height_class_at(tx, tz) {
        Some(c) => c,
        None => return false,
    };
    let fc = match height_class_at(fx, fz) {
        Some(c) => c,
        None => return false,
    };
    (tc - fc).abs() <= 1
}

/// Player movement rule — like can_step but one-directional: any DROP allowed,
/// climbing limited to one class.
pub fn can_step_or_drop(fx: i32, fz: i32, tx: i32, tz: i32) -> bool {
    let tc = match height_class_at(tx, tz) {
        Some(c) => c,
        None => return false,
    };
    let fc = match height_class_at(fx, fz) {
        Some(c) => c,
        None => return false,
    };
    tc - fc <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn castle_safe_r_matches_derivation() {
        assert_eq!(CASTLE_SAFE_R, (18.0 * SCALE_X).round());
    }

    #[test]
    fn centre_is_integral() {
        assert_eq!(CENTER_X, 101.0);
        assert_eq!(CENTER_Z, 76.0);
    }

    // ─── Port of src/world/scatterInRegion.test.ts ───────────────────────────
    const INNER: f64 = 0.55;
    const OUTER: f64 = 0.95;

    #[test]
    fn scatters_every_point_inside_outer_rim_annulus() {
        let reg = region_by_biome(Biome::Forest).unwrap();
        let pts = scatter_in_region(Biome::Forest, 5);
        assert!(!pts.is_empty());
        for p in &pts {
            let d = (p.x - reg.x).hypot(p.z - reg.z);
            assert!(d >= reg.r * INNER - 1e-6);
            assert!(d <= reg.r * OUTER + 1e-6);
        }
    }

    #[test]
    fn keeps_swamp_points_on_swamp_tiles_only() {
        let pts = scatter_in_region(Biome::Swamp, 40);
        assert!(pts.len() >= 10);
        for p in &pts {
            assert_eq!(
                tile_at(p.x.floor() as i32, p.z.floor() as i32).map(|t| t.biome),
                Some(Biome::Swamp)
            );
        }
    }

    #[test]
    fn keeps_forest_points_on_forest_tiles_only() {
        let pts = scatter_in_region(Biome::Forest, 26);
        assert!(pts.len() >= 10);
        for p in &pts {
            assert_eq!(
                tile_at(p.x.floor() as i32, p.z.floor() as i32).map(|t| t.biome),
                Some(Biome::Forest)
            );
        }
    }

    #[test]
    fn scatter_is_deterministic_across_calls() {
        let a = scatter_in_region(Biome::Swamp, 6);
        let b = scatter_in_region(Biome::Swamp, 6);
        assert_eq!(a, b);
    }

    #[test]
    fn scatter_empty_for_biome_with_no_region() {
        assert_eq!(scatter_in_region(Biome::Grass, 5), vec![]);
    }
}
