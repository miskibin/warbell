//! `MapGrid` — the `pathfinding::Grid` trait wired over the REAL ported map
//! (completes P1.4). The pathfinding port abstracts all world queries behind the
//! `Grid` trait so its own tests inject a hand-drawn grid; this is the production
//! implementation that bridges that trait to the actual tilemap/obstacles/
//! house-blocker modules.
//!
//! Each method forwards to the same chokepoint the TS `isWalkable`/`canStep`
//! consulted:
//!   standable     -> tilemap::standable        (land OR bridge deck)
//!   obstacle_tile -> obstacles::is_obstacle_tile
//!   wall_at       -> house_blockers::house_blocks_at  (continuous-space AABB)
//!   can_step      -> tilemap::can_step          (≤ 1 height-class change)
//!   cols / rows   -> tilemap consts

use crate::house_blockers::house_blocks_at;
use crate::obstacles::is_obstacle_tile;
use crate::pathfinding::Grid;
use crate::tilemap::{COLS, ROWS, can_step, standable};

/// Zero-sized adapter: all map state lives in the module-level caches the
/// forwarded functions read, so the grid itself carries no data.
#[derive(Debug, Clone, Copy, Default)]
pub struct MapGrid;

impl MapGrid {
    pub fn new() -> Self {
        MapGrid
    }
}

impl Grid for MapGrid {
    fn cols(&self) -> i32 {
        COLS
    }
    fn rows(&self) -> i32 {
        ROWS
    }
    fn standable(&self, x: i32, z: i32) -> bool {
        standable(x, z)
    }
    fn obstacle_tile(&self, x: i32, z: i32) -> bool {
        is_obstacle_tile(x, z)
    }
    fn wall_at(&self, x: f64, z: f64) -> bool {
        house_blocks_at(x, z)
    }
    fn can_step(&self, fx: i32, fz: i32, tx: i32, tz: i32) -> bool {
        can_step(fx, fz, tx, tz)
    }
}

#[cfg(test)]
mod tests {
    // Trait-wiring smoke test only — the reachability integration test
    // (tests/map_reachability.rs) already proves connectivity. This just proves
    // find_path runs over the REAL grid and returns a non-empty path from the
    // castle south-gate apron toward a nearby reachable tile.
    use super::*;
    use crate::obstacles::find_spawn_near;
    use crate::pathfinding::{PathPoint, find_path};
    use crate::tilemap::shift_to_centre;

    #[test]
    fn finds_a_path_on_the_real_map_near_the_south_gate() {
        let g = MapGrid::new();
        // Castle south-gate apron (base coords → re-centred map), snapped to a
        // standable, prop-free tile — the same start the integration test uses.
        let (sx, sz) = shift_to_centre(72.0, 64.0);
        let start = find_spawn_near(sx, sz, 8);
        // A nearby tile a few steps south, also snapped onto open ground.
        let goal = find_spawn_near(start.0, start.1 + 6.0, 8);

        let start_pt = PathPoint { x: start.0, z: start.1 };
        let goal_pt = PathPoint { x: goal.0, z: goal.1 };
        let path = find_path(&g, start_pt, goal_pt, 800);

        assert!(
            !path.is_empty(),
            "expected a non-empty path from south-gate apron {start:?} to {goal:?}"
        );
        // The path ends at the goal tile centre.
        let last = path.last().unwrap();
        assert_eq!(last.x, goal.0.floor() + 0.5);
        assert_eq!(last.z, goal.1.floor() + 0.5);
    }
}
