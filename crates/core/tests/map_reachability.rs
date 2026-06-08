//! Integration port of src/world/mapReachability.test.ts — the player must be
//! able to walk from the castle out to every biome foot and each mountain summit
//! on the REAL procedural map.
//!
//! Rust runs tests in PARALLEL threads sharing process globals, and the bridge
//! registry is a mutable global. So this is ONE test fn: it does the setup
//! (reset_bridges + register the road bridges) and the flood-fill ONCE, then
//! asserts ALL targets in a loop. Splitting per-target would race the registry.

use tileworld_core::bridges::{BridgeSpan, register_bridge, reset_bridges};
use tileworld_core::obstacles::{find_spawn_near, is_obstacle_tile};
use tileworld_core::roads::get_road_bridges;
use tileworld_core::tilemap::{COLS, ROWS, can_step, from_base, shift_to_centre, standable};
use std::collections::HashSet;
use std::collections::VecDeque;

fn walkable(cx: i32, cz: i32) -> bool {
    if cx < 0 || cz < 0 || cx >= COLS || cz >= ROWS {
        return false;
    }
    if is_obstacle_tile(cx, cz) {
        return false;
    }
    standable(cx, cz)
}

/// Flood the walkable component using the exact pathfinding rules: standable +
/// prop-free tiles, can_step climb gate, no diagonal corner-cutting past a
/// cliff/gap.
fn flood_from(sx: i32, sz: i32) -> HashSet<i64> {
    let mut seen: HashSet<i64> = HashSet::new();
    seen.insert(sz as i64 * COLS as i64 + sx as i64);
    let mut q: VecDeque<(i32, i32)> = VecDeque::new();
    q.push_back((sx, sz));
    let neighbors: [(i32, i32); 8] = [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ];
    while let Some((cx, cz)) = q.pop_front() {
        for (dx, dz) in neighbors {
            let nx = cx + dx;
            let nz = cz + dz;
            let k = nz as i64 * COLS as i64 + nx as i64;
            if seen.contains(&k) || !walkable(nx, nz) || !can_step(cx, cz, nx, nz) {
                continue;
            }
            if dx != 0
                && dz != 0
                && (!can_step(cx, cz, cx + dx, cz) || !can_step(cx, cz, cx, cz + dz))
            {
                continue;
            }
            seen.insert(k);
            q.push_back((nx, nz));
        }
    }
    seen
}

#[test]
fn castle_reaches_every_biome_and_summit() {
    // Register the computed road bridges so river crossings count (the Bridge
    // components do this on mount in the live game).
    reset_bridges();
    for b in get_road_bridges() {
        register_bridge(BridgeSpan {
            from_x: b.from_x,
            from_z: b.from_z,
            to_x: b.to_x,
            to_z: b.to_z,
            width: 3.0,
            y: 1.0,
        });
    }

    // Castle south-gate apron (base coords → re-centred map).
    let (start_x, start_z) = shift_to_centre(72.0, 64.0);
    let start = find_spawn_near(start_x, start_z, 8);
    let reachable = flood_from(start.0.floor() as i32, start.1.floor() as i32);

    // TARGETS are base-map biome coords; scale onto the enlarged map.
    let targets: [(&str, (f64, f64)); 7] = [
        ("snow massif (NW) foot", (38.0, 34.0)),
        ("desert (NE)", (104.0, 28.0)),
        ("forest (SW)", (40.0, 76.0)),
        ("swamp (S)", (72.0, 84.0)),
        ("rock range (E) foot", (110.0, 66.0)),
        ("snow massif (NW) summit", (26.0, 24.0)),
        ("rock range (E) summit", (122.0, 58.0)),
    ];

    for (name, (tx, tz)) in targets {
        let (bx, bz) = from_base(tx, tz);
        let goal = find_spawn_near(bx, bz, 8);
        let key = goal.1.floor() as i64 * COLS as i64 + goal.0.floor() as i64;
        assert!(
            reachable.contains(&key),
            "castle → {name} should be reachable"
        );
    }
}
