//! Port of src/world/obstacles.ts — procedural scatter (trees/rocks/…) + the
//! ork-camp anchors, RESERVED footprint set, collision/blocked-tile indices, and
//! spawn snapping.
//!
//! PARITY-CRITICAL: `generate` must pull the same mulberry32 `rng(2027)` sequence
//! as the TS in the same order — iterate `for z { for x {...} }`, one rand() per
//! non-reserved land tile for the roll, then the thinning branch (conditional
//! rands), then ALWAYS cx/cz/rot/scale/variant (5 rands, consumed even in the
//! cluster branch where they're unused), then the cluster count + 5 rands per
//! cluster item. The reachability gate test depends on this sequence.

use crate::city_plan::{is_inside_castle, snap_to_cardinal};
use crate::house_blockers::house_blocks_at;
use crate::landmarks::landmarks;
use crate::roads::is_road_tile;
use crate::tilemap::{
    Biome, COLS, ROWS, from_base, is_mountain_ramp_tile, shift_to_centre, standable, tile_at,
    tile_top_y,
};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObstacleKind {
    Tree,
    Birch,
    SnowPine,
    DeadTree,
    Bush,
    Rock,
    Boulder,
    Mushroom,
    Flower,
    Tuft,
    Cactus,
    IceShard,
    Bones,
    Reeds,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    pub kind: ObstacleKind,
    pub x: f64,
    pub z: f64,
    pub y: f64,
    pub radius: f64,
    pub scale: f64,
    pub rot: f64,
    pub variant: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CampSlot {
    pub x: i32,
    pub z: i32,
    pub biome: Biome,
}

// Authored in BASE coords; converted via from_base + round.
const BASE_CAMPS: [(i32, i32, Biome); 3] = [
    (74, 26, Biome::Snow),   // N — snow/desert frontier
    (104, 32, Biome::Desert), // NE — deep dunes
    (34, 72, Biome::Forest),  // SW — clearing in the wood
];

static ORK_CAMPS_CACHE: OnceLock<Vec<CampSlot>> = OnceLock::new();

pub fn ork_camps() -> &'static [CampSlot] {
    ORK_CAMPS_CACHE.get_or_init(|| {
        BASE_CAMPS
            .iter()
            .map(|&(x, z, biome)| {
                let (nx, nz) = from_base(x as f64, z as f64);
                CampSlot {
                    x: nx.round() as i32,
                    z: nz.round() as i32,
                    biome,
                }
            })
            .collect()
    })
}

// ─── RESERVED footprint set ──────────────────────────────────────────────────
static RESERVED: OnceLock<HashSet<i64>> = OnceLock::new();

fn reserved_key(x: i32, z: i32) -> i64 {
    z as i64 * COLS as i64 + x as i64
}

fn build_reserved() -> HashSet<i64> {
    let mut r: HashSet<i64> = HashSet::new();
    let add_box = |r: &mut HashSet<i64>, x0: i32, x1: i32, z0: i32, z1: i32| {
        for z in z0..=z1 {
            for x in x0..=x1 {
                r.insert(reserved_key(x, z));
            }
        }
    };
    // baseBox: authored in BASE coords, mapped via transform t, corners
    // floored/ceiled to whole tiles.
    let base_box = |r: &mut HashSet<i64>,
                        x0: i32,
                        x1: i32,
                        z0: i32,
                        z1: i32,
                        t: fn(f64, f64) -> (f64, f64)| {
        let (ax, az) = t(x0 as f64, z0 as f64);
        let (bx, bz) = t(x1 as f64, z1 as f64);
        let nx0 = ax.min(bx).floor() as i32;
        let nx1 = ax.max(bx).ceil() as i32;
        let nz0 = az.min(bz).floor() as i32;
        let nz1 = az.max(bz).ceil() as i32;
        add_box(r, nx0, nx1, nz0, nz1);
    };

    // 7×7 clearing around each ork camp (already in new space).
    for c in ork_camps() {
        add_box(&mut r, c.x - 3, c.x + 3, c.z - 3, c.z + 3);
    }
    // Northwest frontier hamlet — wilderness.
    base_box(&mut r, 62, 70, 28, 36, from_base);
    // Market stall just outside the south gate — castle-attached.
    base_box(&mut r, 65, 71, 68, 74, shift_to_centre);
    // NE desert caravan market.
    base_box(&mut r, 90, 102, 28, 38, from_base);
    // Biome signature landmarks — margin around each.
    for l in landmarks() {
        add_box(&mut r, l.x - l.r - 1, l.x + l.r + 1, l.z - l.r - 1, l.z + l.r + 1);
    }
    r
}

fn reserved() -> &'static HashSet<i64> {
    RESERVED.get_or_init(build_reserved)
}

fn is_reserved(x: i32, z: i32) -> bool {
    if is_inside_castle(x as f64, z as f64) {
        return true;
    }
    if is_road_tile(x, z) {
        return true;
    }
    if is_mountain_ramp_tile(x, z) {
        return true;
    }
    reserved().contains(&reserved_key(x, z))
}

// ─── mulberry32 PRNG ─────────────────────────────────────────────────────────
struct Rng {
    s: u32,
}
impl Rng {
    fn new(seed: u32) -> Self {
        Rng { s: seed }
    }
    fn next(&mut self) -> f64 {
        // s = (s + 0x6d2b79f5) >>> 0
        self.s = self.s.wrapping_add(0x6d2b79f5);
        let mut t = self.s;
        // t = Math.imul(t ^ (t >>> 15), t | 1)
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        // t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        // ((t ^ (t >>> 14)) >>> 0) / 4294967296
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }
}

fn radius_by_kind(kind: ObstacleKind) -> f64 {
    match kind {
        ObstacleKind::Tree => 0.12,
        ObstacleKind::Birch => 0.1,
        ObstacleKind::SnowPine => 0.12,
        ObstacleKind::DeadTree => 0.09,
        ObstacleKind::Bush => 0.0,
        ObstacleKind::Rock => 0.0,
        ObstacleKind::Boulder => 0.34,
        ObstacleKind::Mushroom => 0.0,
        ObstacleKind::Flower => 0.0,
        ObstacleKind::Tuft => 0.0,
        ObstacleKind::Cactus => 0.18,
        ObstacleKind::IceShard => 0.0,
        ObstacleKind::Bones => 0.0,
        ObstacleKind::Reeds => 0.0,
    }
}

// ─── Per-biome cumulative roll tables ────────────────────────────────────────
#[derive(Clone, Copy)]
struct Roll {
    kind: ObstacleKind,
    until: f64,
    cluster_min: Option<i32>,
    cluster_max: Option<i32>,
}
const fn roll(kind: ObstacleKind, until: f64) -> Roll {
    Roll { kind, until, cluster_min: None, cluster_max: None }
}
const fn cluster(kind: ObstacleKind, until: f64, cmin: i32, cmax: i32) -> Roll {
    Roll { kind, until, cluster_min: Some(cmin), cluster_max: Some(cmax) }
}

use ObstacleKind::*;

// Grass: the original ground is a DENSE carpet of grass-blade tufts with red
// mushrooms + colourful flowers scattered for life. Widen the tuft band to nearly
// fill the table (it now claims ~0.22..0.95 of the roll) and bump the cluster count
// (3..6 per tile) so tufts thickly carpet the lawn; mushrooms + flowers get a modest
// density lift too. Tree/bush/rock bands are unchanged so the silhouette mix holds.
const GRASS_ROLLS: [Roll; 8] = [
    roll(Tree, 0.05),
    roll(Birch, 0.08),
    roll(Bush, 0.12),
    roll(Rock, 0.14),
    roll(Boulder, 0.15),
    cluster(Mushroom, 0.20, 1, 3),
    cluster(Flower, 0.28, 2, 5),
    cluster(Tuft, 0.95, 3, 6),
];
// Forest floor: same dense-tuft treatment under the canopy. Widen the tuft band and
// raise its cluster count so the forest ground carpets like the meadow.
const FOREST_ROLLS: [Roll; 7] = [
    roll(Tree, 0.34),
    roll(Birch, 0.48),
    roll(DeadTree, 0.52),
    roll(Bush, 0.64),
    cluster(Mushroom, 0.72, 2, 4),
    cluster(Flower, 0.78, 1, 3),
    cluster(Tuft, 0.98, 3, 6),
];
const SAND_ROLLS: [Roll; 3] = [roll(Rock, 0.04), roll(Bones, 0.05), roll(Tuft, 0.1)];
const ROCK_ROLLS: [Roll; 5] = [
    roll(Boulder, 0.16),
    roll(Rock, 0.4),
    roll(DeadTree, 0.45),
    roll(Bush, 0.48),
    roll(Tuft, 0.55),
];
const SNOW_ROLLS: [Roll; 5] = [
    roll(SnowPine, 0.18),
    cluster(IceShard, 0.28, 1, 2),
    roll(Rock, 0.33),
    roll(Boulder, 0.35),
    roll(DeadTree, 0.37),
];
const DESERT_ROLLS: [Roll; 4] = [
    roll(Cactus, 0.07),
    cluster(Bones, 0.13, 1, 2),
    roll(Rock, 0.17),
    roll(DeadTree, 0.18),
];
const PLAINS_ROLLS: [Roll; 4] = [
    cluster(Flower, 0.18, 2, 4),
    // Plains share the grass meadow look — carpet them with tufts too.
    cluster(Tuft, 0.90, 3, 6),
    roll(Rock, 0.93),
    roll(Tree, 0.95),
];
const SWAMP_ROLLS: [Roll; 5] = [
    roll(DeadTree, 0.14),
    cluster(Reeds, 0.34, 2, 3),
    cluster(Mushroom, 0.48, 1, 3),
    roll(Bush, 0.54),
    cluster(Tuft, 0.66, 1, 2),
];

fn rolls_for(biome: Biome) -> &'static [Roll] {
    match biome {
        Biome::Grass => &GRASS_ROLLS,
        Biome::Forest => &FOREST_ROLLS,
        Biome::Sand => &SAND_ROLLS,
        Biome::Rock => &ROCK_ROLLS,
        Biome::Snow => &SNOW_ROLLS,
        Biome::Desert => &DESERT_ROLLS,
        Biome::Plains => &PLAINS_ROLLS,
        Biome::Swamp => &SWAMP_ROLLS,
    }
}

// ─── Obstacle cache ──────────────────────────────────────────────────────────
static OBSTACLES: OnceLock<Vec<Obstacle>> = OnceLock::new();

pub fn get_obstacles() -> &'static [Obstacle] {
    OBSTACLES.get_or_init(generate)
}

fn push(out: &mut Vec<Obstacle>, kind: ObstacleKind, x: f64, z: f64, scale: f64, rot: f64, variant: i32) {
    let base_tile = tile_at(x.floor() as i32, z.floor() as i32);
    let y = if base_tile.is_some() {
        tile_top_y(x.floor() as i32, z.floor() as i32)
    } else {
        1.0
    };
    out.push(Obstacle {
        kind,
        x,
        z,
        y,
        radius: radius_by_kind(kind),
        scale,
        rot,
        variant,
    });
}

fn generate() -> Vec<Obstacle> {
    let mut rand = Rng::new(2027);
    let mut out: Vec<Obstacle> = Vec::new();
    for z in 0..ROWS {
        for x in 0..COLS {
            let tile = match tile_at(x, z) {
                Some(t) => t,
                None => continue,
            };
            if is_reserved(x, z) {
                continue;
            }

            let rolls = rolls_for(tile.biome);
            let r = rand.next();
            let mut picked: Option<&Roll> = None;
            for roll in rolls {
                if r < roll.until {
                    picked = Some(roll);
                    break;
                }
            }
            let picked = match picked {
                Some(p) => p,
                None => continue,
            };

            // Thinning (same conditional rand order as TS).
            if matches!(picked.kind, Tree | Birch | SnowPine) {
                if rand.next() < 0.65 {
                    continue;
                }
                if tile.biome == Biome::Forest && rand.next() < 0.15 {
                    continue;
                }
            } else if radius_by_kind(picked.kind) > 0.0 && rand.next() < 0.3 {
                continue;
            }

            // ALWAYS pulled (even in the cluster branch where they're unused).
            let cx = x as f64 + 0.5 + (rand.next() - 0.5) * 0.4;
            let cz = z as f64 + 0.5 + (rand.next() - 0.5) * 0.4;
            let rot = snap_to_cardinal(rand.next() * std::f64::consts::PI * 2.0);
            let scale = 0.85 + rand.next() * 0.45;
            let variant = (rand.next() * 4.0).floor() as i32;

            if let Some(cmin) = picked.cluster_min {
                // A spec with `cluster_min` but no `cluster_max` is malformed data; fall back to a
                // single-value span (cmin) instead of panicking in a release build.
                let cmax = picked.cluster_max.unwrap_or(cmin);
                debug_assert!(cmax >= cmin, "cluster_max < cluster_min for {:?}", picked.kind);
                // For valid specs (cmin <= cmax) this span is `cmax - cmin + 1` and the
                // RNG draw + result are unchanged. The `.max(1)` only guards a malformed
                // span (cmax < cmin) from producing a negative range; the final `.max(cmin)`
                // keeps `count >= cmin` regardless.
                let span = (cmax - cmin + 1).max(1);
                let count = (cmin + (rand.next() * span as f64).floor() as i32).max(cmin);
                for _ in 0..count {
                    let px = x as f64 + rand.next();
                    let pz = z as f64 + rand.next();
                    let sc = 0.7 + rand.next() * 0.5;
                    let rt = snap_to_cardinal(rand.next() * std::f64::consts::PI * 2.0);
                    let vr = (rand.next() * 4.0).floor() as i32;
                    push(&mut out, picked.kind, px, pz, sc, rt, vr);
                }
            } else {
                push(&mut out, picked.kind, cx, cz, scale, rot, variant);
            }
        }
    }
    out
}

// ─── Collision index ─────────────────────────────────────────────────────────
static COLLISION_GRID: OnceLock<HashMap<i64, Vec<Obstacle>>> = OnceLock::new();

fn collision_grid() -> &'static HashMap<i64, Vec<Obstacle>> {
    COLLISION_GRID.get_or_init(|| {
        let mut g: HashMap<i64, Vec<Obstacle>> = HashMap::new();
        for o in get_obstacles() {
            if o.radius <= 0.0 {
                continue;
            }
            let key = o.z.floor() as i64 * COLS as i64 + o.x.floor() as i64;
            g.entry(key).or_default().push(*o);
        }
        g
    })
}

pub fn obstacle_collides_at(x: f64, z: f64, r: f64) -> bool {
    let grid = collision_grid();
    let cx = x.floor() as i64;
    let cz = z.floor() as i64;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let key = (cz + dz) * COLS as i64 + (cx + dx);
            if let Some(cell) = grid.get(&key) {
                for o in cell {
                    let ox = x - o.x;
                    let oz = z - o.z;
                    let rsum = r + o.radius;
                    if ox * ox + oz * oz < rsum * rsum {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ─── Blocked-tile index ──────────────────────────────────────────────────────
static BLOCKED_TILES: OnceLock<HashSet<i64>> = OnceLock::new();

fn blocked_tiles() -> &'static HashSet<i64> {
    BLOCKED_TILES.get_or_init(|| {
        let mut s: HashSet<i64> = HashSet::new();
        for o in get_obstacles() {
            if o.radius > 0.0 {
                s.insert(o.z.floor() as i64 * COLS as i64 + o.x.floor() as i64);
            }
        }
        s
    })
}

/// True if a collidable obstacle sits in tile (cx, cz).
pub fn is_obstacle_tile(cx: i32, cz: i32) -> bool {
    blocked_tiles().contains(&(cz as i64 * COLS as i64 + cx as i64))
}

/// Nearest standable, obstacle-free, unblocked tile center to (x,z), ring search.
/// Falls back to the rounded input if nothing is found.
pub fn find_spawn_near(x: f64, z: f64, max_r: i32) -> (f64, f64) {
    let ox = x.floor() as i32;
    let oz = z.floor() as i32;
    for r in 0..=max_r {
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dz.abs()) != r {
                    continue;
                }
                let cx = ox + dx;
                let cz = oz + dz;
                if standable(cx, cz)
                    && !is_obstacle_tile(cx, cz)
                    && !house_blocks_at(cx as f64 + 0.5, cz as f64 + 0.5)
                {
                    return (cx as f64 + 0.5, cz as f64 + 0.5);
                }
            }
        }
    }
    (ox as f64 + 0.5, oz as f64 + 0.5)
}

#[cfg(test)]
mod tests {
    // Port of src/world/campPlacement.test.ts — every tile under each camp's
    // 5×5 prop spread must be standable AND the same height class as the centre,
    // so the camp sits flush (no floating tents).
    use super::*;
    use crate::tilemap::standable;

    const FOOTPRINT: i32 = 2;

    #[test]
    fn camps_sit_on_flat_standable_ground() {
        for camp in ork_camps() {
            let center = tile_at(camp.x, camp.z);
            assert!(center.is_some(), "camp centre must be land");
            let h = center.unwrap().height;
            for dz in -FOOTPRINT..=FOOTPRINT {
                for dx in -FOOTPRINT..=FOOTPRINT {
                    let x = camp.x + dx;
                    let z = camp.z + dz;
                    assert!(
                        standable(x, z),
                        "tile ({x},{z}) under camp must be standable"
                    );
                    let t = tile_at(x, z);
                    assert_eq!(
                        t.unwrap().height,
                        h,
                        "tile ({x},{z}) height must equal camp centre height {h}"
                    );
                }
            }
        }
    }

    // Golden parity: lock the mulberry32 `rng(2027)` sequence to the exact values
    // the TS produces (computed from the original `obstacles.ts` rng in Node).
    // The whole scatter layout — and thus the reachability gate — rides on this
    // PRNG matching bit-for-bit.
    #[test]
    fn rng_2027_matches_ts_sequence() {
        let mut r = Rng::new(2027);
        let expected = [
            0.98798907129094_f64,
            0.5169636213686317,
            0.19619796960614622,
            0.41229676525108516,
            0.3498533582314849,
        ];
        for (i, e) in expected.iter().enumerate() {
            let got = r.next();
            assert!(
                (got - e).abs() < 1e-15,
                "rng draw {i}: got {got}, expected {e}"
            );
        }
    }
}
