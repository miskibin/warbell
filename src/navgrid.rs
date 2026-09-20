//! **Forest nav-grid** — wires core's tested 8-direction A* (`tileworld_core::pathfinding`) to
//! forest's world-space terrain + blockers so night-wave invaders route to the keep **through the
//! gates** instead of smearing along the walls. Walls register collision boxes (impassable); gate
//! gaps register none (open) → A* threads them with no explicit gate-targeting code.
//!
//! Coordinate frame: forest is world-space `f32` with the castle at the origin; one tile = one
//! world unit; `tile = floor(world + G)`. We map a forest tile ↔ a core `PathPoint` (tile+0.5),
//! and the edge-midpoint `wall_at` test (in core) is what opens gates while blocking walls.

use bevy::prelude::*;
use tileworld_core::pathfinding::{find_path, Grid, PathPoint};

use crate::blockers;
use crate::worldmap::{COLS, GROUND_STEP, GX, GZ, ROWS};

/// A* node budget for the invader keep-march. The spawn ring is 30 tiles out, but on the enlarged
/// map an invader spawned across one of the four rivers must detour to a bridge, so the explored
/// set can far exceed the straight-line tile count. 1400 truncated those detours to an *empty*
/// path (idle invader); 6000 covered the worst river detour at MAP_SCALE 2.2, re-sized to 8400
/// for the 2.6 bump (budget ∝ tile count ∝ MAP_SCALE²). Since the keep is always reachable, A*
/// terminates as soon as it's found — the larger budget only costs on a (never-occurring for
/// invaders) unreachable goal, where the open set drains to the cap.
pub const NAV_MAX_NODES: u32 = 8400;

/// World centre of tile `(ix, iz)` in forest world-space (castle at origin).
#[inline]
fn tile_world_centre(ix: i32, iz: i32) -> (f32, f32) {
    (ix as f32 - GX + 0.5, iz as f32 - GZ + 0.5)
}

/// Forest's terrain + blocker set, viewed as a pathfinding `Grid`. Tile-centre heights come
/// from `worldmap::tile_centre_ground` — a per-map baked flat array (terrain is static per
/// map), so A*'s many re-reads of the same tiles are single array loads. This replaced a
/// per-`find_path` `RefCell<HashMap>` memo (which re-derived every tile once per search —
/// measured at ~208ms for one long-haul stone-miner search before any caching existed).
/// Blockers/bridges stay live queries: walls ARE built and razed mid-run.
#[derive(Default)]
pub struct ForestGrid;

impl ForestGrid {
    fn height_at(&self, ix: i32, iz: i32) -> Option<f32> {
        crate::worldmap::tile_centre_ground(ix, iz)
    }
}

impl Grid for ForestGrid {
    fn cols(&self) -> i32 {
        COLS
    }
    fn rows(&self) -> i32 {
        ROWS
    }
    fn standable(&self, ix: i32, iz: i32) -> bool {
        // Land (not water / off-map) OR a bridge deck spanning the river — so A* (and the night
        // invaders) can cross the water at a crossing instead of routing all the way around.
        if self.height_at(ix, iz).is_some() {
            return true;
        }
        let (wx, wz) = tile_world_centre(ix, iz);
        crate::bridges::is_on_bridge(wx, wz)
    }
    fn obstacle_tile(&self, ix: i32, iz: i32) -> bool {
        let (wx, wz) = tile_world_centre(ix, iz);
        blockers::is_blocked(wx, wz) // a prop / keep / wall box sits on this tile centre
    }
    fn wall_at(&self, px: f64, pz: f64) -> bool {
        // Core passes continuous coords in ITS grid space (tile+0.5); convert back to forest
        // world. This edge-midpoint test rejects steps crossing a wall while leaving gaps open.
        blockers::is_blocked(px as f32 - GX, pz as f32 - GZ)
    }
    fn can_step(&self, fx: i32, fz: i32, tx: i32, tz: i32) -> bool {
        // Effective walk height: terrain, or a bridge deck over the river. Without the deck
        // fallback a bridge tile is `standable` but no step INTO it ever passes (its terrain
        // height is `None`), so A* could never actually use a crossing.
        let eff = |ix: i32, iz: i32| {
            self.height_at(ix, iz).or_else(|| {
                let (wx, wz) = tile_world_centre(ix, iz);
                crate::bridges::deck_y_at(wx, wz)
            })
        };
        match (eff(fx, fz), eff(tx, tz)) {
            (Some(fy), Some(ty)) => (ty - fy).abs() <= GROUND_STEP + 0.1, // ≤1 height class
            _ => false,
        }
    }
}

/// Forest world XZ → a core `PathPoint` (`find_path` floors internally).
fn world_to_pathpoint(wx: f32, wz: f32) -> PathPoint {
    PathPoint { x: (wx + GX) as f64, z: (wz + GZ) as f64 }
}

/// A* waypoints from `from` to `to` in forest world-space (empty if no route / already there).
pub fn path_to(from: Vec2, to: Vec2) -> Vec<Vec2> {
    path_to_budget(from, to, NAV_MAX_NODES)
}

/// [`path_to`] with an explicit node budget — the default [`NAV_MAX_NODES`] is sized for the
/// ~40-tile invader run to the keep and **exhausts (→ empty) on cross-island trips** like the
/// stone miner's castle→Rocky haul (~100 tiles + river detours). On an unreachable goal A*
/// drains the open set and exits early, so a generous budget only costs when a route exists.
pub fn path_to_budget(from: Vec2, to: Vec2, max_nodes: u32) -> Vec<Vec2> {
    let _profile = crate::gameplay_profile::scope(crate::gameplay_profile::Metric::Pathfinding);
    find_path(&ForestGrid::default(), world_to_pathpoint(from.x, from.y), world_to_pathpoint(to.x, to.y), max_nodes)
        .into_iter()
        .map(|p| Vec2::new(p.x as f32 - GX, p.z as f32 - GZ))
        .collect()
}

/// A cached A* route (followed + smoothed by `steer::advance`) — the keep-march of a wave
/// invader, or a freed captive's march home to the courtyard. Both thread the gates this way.
#[derive(Component, Default)]
pub struct NavPath {
    pub waypoints: Vec<Vec2>,
    pub cursor: usize,
    /// Game-time at which to recompute (throttled + staggered per invader).
    pub next_replan: f32,
    /// The goal the cached path was computed for (replan if it moves).
    pub goal_cached: Vec2,
    /// An empty result must wait for its retry deadline, unlike an exhausted successful route.
    failed: bool,
}

impl NavPath {
    /// `periodic` retains the keep/worker march's timed refresh; guards instead refresh a live
    /// route only when its goal moves. Failed searches always retry after the staggered deadline.
    pub fn needs_replan(&self, now: f32, goal: Vec2, periodic: bool) -> bool {
        tileworld_core::pathfinding::should_replan_path(
            self.failed && self.waypoints.is_empty(),
            self.cursor >= self.waypoints.len(),
            now >= self.next_replan,
            self.goal_cached.distance_squared(goal) > 4.0,
            periodic,
        )
    }

    pub fn set_route(&mut self, waypoints: Vec<Vec2>, goal: Vec2, retry_at: f32) {
        self.failed = waypoints.is_empty();
        self.waypoints = waypoints;
        self.cursor = 0;
        self.goal_cached = goal;
        self.next_replan = retry_at;
    }

    /// Entering direct-steer range abandons the cached route, not a failed search to retry.
    pub fn clear_route(&mut self) {
        self.waypoints.clear();
        self.cursor = 0;
        self.failed = false;
    }
}
