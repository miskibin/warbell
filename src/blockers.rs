//! Shared **obstacle set** — the world-space solids a mover must route around (tree trunk /
//! cactus / wall / building …). The scatter ([`crate::biome::scatter_region`]), the castle
//! and the camps register obstacles; wildlife, orks, villagers and the player all read it so
//! they slide around solids instead of clipping through them.
//!
//! Two obstacle shapes, picked to fit the thing:
//! * **Circles** (`add`) for round/point props — a tree blocks only its trunk (you walk under
//!   the canopy). Small clutter (bushes, small rocks, barrel cacti, ground cover) registers
//!   nothing, so you walk through it freely.
//! * **Oriented boxes** (`add_box` axis-aligned, `add_obb` rotated) for rectangular structures
//!   — towers, houses, the keep, farm, walls, and the camp tents/cage/fire/banner — each sized
//!   to its ACTUAL footprint and (for camp props) its real rotation, so a long thin tilted tent
//!   gets a thin tilted box, not a fat square. (Filling a rectangle with floor-snapped circles,
//!   the old approach, ballooned a ~1.9-wide tower into a ~4.5-wide collision.)
//!
//! Circles are bucketed by their centre tile; [`is_blocked`] scans only the query point's own
//! tile + its 8 neighbours, so every circle radius MUST stay ≤ 1.0 (a larger one could reach a
//! point two tiles from its centre and be missed). Boxes have no such bound — they're held in a
//! flat list and tested directly (there are only a few dozen, so the linear scan is cheap).
//!
//! Lifecycle: [`reset`] at the top of every (re)build, [`add`]/[`add_box`]/[`add_obb`] during
//! scatter/castle/camps, [`is_blocked`] per mover step.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Circle obstacles: `(centre_x, centre_z, radius)`, bucketed by centre tile.
static CIRCLES: LazyLock<RwLock<HashMap<(i32, i32), Vec<(f32, f32, f32)>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Oriented-box obstacles: `[cx, cz, hw, hd, cos_yaw, sin_yaw]`. An axis-aligned box is just
/// `cos=1, sin=0`. The point test rotates the query into the box's local frame.
static BOXES: LazyLock<RwLock<Vec<[f32; 6]>>> = LazyLock::new(|| RwLock::new(Vec::new()));

fn tile(wx: f32, wz: f32) -> (i32, i32) {
    (wx.floor() as i32, wz.floor() as i32)
}

/// Clear all blockers — call once before rebuilding the scene.
pub fn reset() {
    CIRCLES.write().unwrap().clear();
    BOXES.write().unwrap().clear();
}

/// Mark a solid circular obstacle of `radius` (world units) centred at `(wx, wz)`. A radius
/// ≤ 0 registers nothing. Keep `radius ≤ 1.0` — the neighbour-only [`is_blocked`] scan
/// assumes it (use [`add_box`] for anything bigger than a trunk).
pub fn add(wx: f32, wz: f32, radius: f32) {
    if radius <= 0.0 {
        return;
    }
    CIRCLES.write().unwrap().entry(tile(wx, wz)).or_default().push((wx, wz, radius));
}

/// Mark a solid **axis-aligned** box centred at `(cx, cz)` with half-extents `(hw, hd)` (spans
/// `cx ± hw` by `cz ± hd`). For rectangular structures — sized to the real footprint (+ a small
/// body margin), no radius bound.
pub fn add_box(cx: f32, cz: f32, hw: f32, hd: f32) {
    add_obb(cx, cz, hw, hd, 0.0);
}

/// Mark a solid **oriented** box centred at `(cx, cz)`, half-extents `(hw, hd)` in its local
/// frame, rotated `yaw` radians about +Y (matching `Quat::from_rotation_y(yaw)` on the mesh).
/// For rotated rectangular props (camp tents/cage) so the collision hugs the real silhouette.
pub fn add_obb(cx: f32, cz: f32, hw: f32, hd: f32, yaw: f32) {
    if hw <= 0.0 || hd <= 0.0 {
        return;
    }
    BOXES.write().unwrap().push([cx, cz, hw, hd, yaw.cos(), yaw.sin()]);
}

/// True if `(wx, wz)` lies inside any solid obstacle (a circle or an oriented box).
pub fn is_blocked(wx: f32, wz: f32) -> bool {
    {
        let map = CIRCLES.read().unwrap();
        let (tx, tz) = tile(wx, wz);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(bucket) = map.get(&(tx + dx, tz + dz)) {
                    for &(cx, cz, r) in bucket {
                        let (ex, ez) = (wx - cx, wz - cz);
                        if ex * ex + ez * ez < r * r {
                            return true;
                        }
                    }
                }
            }
        }
    }
    let boxes = BOXES.read().unwrap();
    boxes.iter().any(|b| {
        let (ex, ez) = (wx - b[0], wz - b[1]);
        let (cos, sin) = (b[4], b[5]);
        // Rotate the query into the box's local frame (inverse Y-rotation), then AABB-test.
        let lx = cos * ex - sin * ez;
        let lz = sin * ex + cos * ez;
        lx.abs() <= b[2] && lz.abs() <= b[3]
    })
}
