//! Port of src/world/roads.ts — grid road network (dirt + auto-bridges).
//!
//! Roads are polylines of integer tile waypoints (every consecutive pair
//! axis-aligned). Authored on the base 144×108 map then scaled via from_base +
//! round. Water crossings auto-emit bridges that bracket the run with the land
//! tiles on either side. Built once and cached.

use crate::tilemap::{COLS, ROWS, from_base, is_land};
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadBridge {
    pub from_x: f64,
    pub from_z: f64,
    pub to_x: f64,
    pub to_z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirtTile {
    pub x: i32,
    pub z: i32,
}

// Waypoints in BASE grid coords. Every consecutive pair is axis-aligned.
const BASE_ROUTES: &[&[(i32, i32)]] = &[
    // North gate → northern trunk
    &[(72, 44), (72, 26)],
    &[(72, 26), (74, 26)],
    &[(72, 30), (60, 30)],
    &[(72, 30), (90, 30), (90, 42)],
    // South gate → southern trunk
    &[(72, 64), (72, 84)],
    &[(72, 72), (44, 72), (44, 64)],
    // West gate → SW forest
    &[(58, 54), (42, 54), (42, 66), (36, 66)],
    // East gate → desert/rock foot
    &[(86, 54), (86, 46), (92, 46), (92, 44)],
    // Spurs to landmarks
    &[(72, 66), (68, 66), (68, 71)],
    &[(72, 32), (66, 32)],
];

fn routes() -> Vec<Vec<(i32, i32)>> {
    BASE_ROUTES
        .iter()
        .map(|route| {
            route
                .iter()
                .map(|&(x, z)| {
                    let (nx, nz) = from_base(x as f64, z as f64);
                    (nx.round() as i32, nz.round() as i32)
                })
                .collect()
        })
        .collect()
}

struct RoadData {
    dirt: Vec<DirtTile>,
    bridges: Vec<RoadBridge>,
    tiles: HashSet<i64>,
}

static CACHED: OnceLock<RoadData> = OnceLock::new();

fn key_of(x: i32, z: i32) -> i64 {
    z as i64 * COLS as i64 + x as i64
}

/// Math.sign — TS `lineTiles` step direction.
fn sign(n: i32) -> i32 {
    if n > 0 {
        1
    } else if n < 0 {
        -1
    } else {
        0
    }
}

/// Inclusive tile list between two axis-aligned points.
fn line_tiles(ax: i32, az: i32, bx: i32, bz: i32) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    let dx = sign(bx - ax);
    let dz = sign(bz - az);
    let mut x = ax;
    let mut z = az;
    out.push((x, z));
    let mut guard = 0;
    while (x != bx || z != bz) && {
        guard += 1;
        guard
    } < 500
    {
        x += dx;
        z += dz;
        out.push((x, z));
    }
    out
}

fn build() -> RoadData {
    let mut tiles: HashSet<i64> = HashSet::new();
    let mut dirt_set: HashSet<i64> = HashSet::new();
    let mut dirt_order: Vec<i64> = Vec::new();
    let mut bridges: Vec<RoadBridge> = Vec::new();

    for route in routes() {
        for i in 0..route.len().saturating_sub(1) {
            let (ax, az) = route[i];
            let (bx, bz) = route[i + 1];
            let seg = line_tiles(ax, az, bx, bz);
            let mut run_start: i64 = -1; // index in seg of first water tile of run
            for j in 0..seg.len() {
                let (x, z) = seg[j];
                if x < 0 || z < 0 || x >= COLS || z >= ROWS {
                    continue;
                }
                tiles.insert(key_of(x, z));
                if is_land(x, z) {
                    if run_start >= 0 {
                        let a = seg[(run_start - 1).max(0) as usize];
                        let b = seg[j];
                        bridges.push(RoadBridge {
                            from_x: a.0 as f64 + 0.5,
                            from_z: a.1 as f64 + 0.5,
                            to_x: b.0 as f64 + 0.5,
                            to_z: b.1 as f64 + 0.5,
                        });
                        run_start = -1;
                    }
                    let k = key_of(x, z);
                    if dirt_set.insert(k) {
                        dirt_order.push(k);
                    }
                } else if run_start < 0 {
                    run_start = j as i64;
                }
            }
            if run_start >= 0 {
                let a = seg[(run_start - 1).max(0) as usize];
                let b = seg[seg.len() - 1];
                bridges.push(RoadBridge {
                    from_x: a.0 as f64 + 0.5,
                    from_z: a.1 as f64 + 0.5,
                    to_x: b.0 as f64 + 0.5,
                    to_z: b.1 as f64 + 0.5,
                });
            }
        }
    }

    let dirt: Vec<DirtTile> = dirt_order
        .iter()
        .map(|&k| DirtTile {
            x: (k % COLS as i64) as i32,
            z: (k / COLS as i64) as i32,
        })
        .collect();
    RoadData { dirt, bridges, tiles }
}

fn data() -> &'static RoadData {
    CACHED.get_or_init(build)
}

pub fn get_road_dirt() -> &'static [DirtTile] {
    &data().dirt
}

pub fn get_road_bridges() -> &'static [RoadBridge] {
    &data().bridges
}

/// True if a road tile occupies (cx, cz).
pub fn is_road_tile(cx: i32, cz: i32) -> bool {
    data().tiles.contains(&key_of(cx, cz))
}
