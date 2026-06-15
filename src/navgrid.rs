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
use crate::worldmap::{ground_at_world, COLS, GROUND_STEP, GX, GZ, ROWS};

/// A* node budget for the invader keep-march. The spawn ring is 30 tiles out, but on the enlarged
/// (259×295) map an invader spawned across one of the four rivers must detour to a bridge, so the
/// explored set can far exceed the straight-line tile count. 1400 truncated those detours to an
/// *empty* path (idle invader); 6000 covers the worst river detour. Since the keep is always
/// reachable, A* terminates as soon as it's found — the larger budget only costs on a (never-
/// occurring for invaders) unreachable goal, where the open set drains to the cap.
pub const NAV_MAX_NODES: u32 = 6000;

/// World centre of tile `(ix, iz)` in forest world-space (castle at origin).
#[inline]
fn tile_world_centre(ix: i32, iz: i32) -> (f32, f32) {
    (ix as f32 - GX + 0.5, iz as f32 - GZ + 0.5)
}

/// Forest's terrain + blocker set, viewed as a pathfinding `Grid`.
pub struct ForestGrid;

impl Grid for ForestGrid {
    fn cols(&self) -> i32 {
        COLS
    }
    fn rows(&self) -> i32 {
        ROWS
    }
    fn standable(&self, ix: i32, iz: i32) -> bool {
        let (wx, wz) = tile_world_centre(ix, iz);
        // Land (not water / off-map) OR a bridge deck spanning the river — so A* (and the night
        // invaders) can cross the water at a crossing instead of routing all the way around.
        ground_at_world(wx, wz).is_some() || crate::bridges::is_on_bridge(wx, wz)
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
        let (fwx, fwz) = tile_world_centre(fx, fz);
        let (twx, twz) = tile_world_centre(tx, tz);
        // Effective walk height: terrain, or a bridge deck over the river. Without the deck
        // fallback a bridge tile is `standable` but no step INTO it ever passes (its terrain
        // height is `None`), so A* could never actually use a crossing.
        let eff = |wx: f32, wz: f32| ground_at_world(wx, wz).or_else(|| crate::bridges::deck_y_at(wx, wz));
        match (eff(fwx, fwz), eff(twx, twz)) {
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
    find_path(&ForestGrid, world_to_pathpoint(from.x, from.y), world_to_pathpoint(to.x, to.y), max_nodes)
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
}
