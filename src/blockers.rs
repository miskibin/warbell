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
//!   — towers, houses, the keep, walls, and the camp tents/cage/fire/banner — each sized
//!   to its ACTUAL footprint and (for camp props) its real rotation, so a long thin tilted tent
//!   gets a thin tilted box, not a fat square. (Filling a rectangle with floor-snapped circles,
//!   the old approach, ballooned a ~1.9-wide tower into a ~4.5-wide collision.)
//!
//! Circles are bucketed by their centre tile; [`is_blocked`] scans only the query point's own
//! tile + its 8 neighbours, so every circle radius MUST stay ≤ 1.0 (a larger one could reach a
//! point two tiles from its centre and be missed). Boxes are ALSO tile-bucketed (every tile each
//! box's circumscribing radius overlaps, computed once at insert time) — the old comment here
//! claimed "only a few dozen [boxes], so the linear scan is cheap", but on the enlarged island
//! (castle + fortress + rival stronghold walls, ~12 houses + producer plots, 5 camps' tents/
//! cages/fires/banners, landmarks) that grew into the hundreds-to-low-thousands. A single
//! pathfinding search explores up to tens of thousands of nodes, each checking several neighbours
//! against `is_blocked`/`wall_at` — a per-call O(all boxes) scan there measured as a **multi-second
//! real freeze** (`cargo run --features profiling` + `tools/trace_summary.py` pinned it: a single
//! `miner::assign_ore` A* call, NOT a burst of several, cost 2.6+ seconds). Bucketing turns each
//! query into an O(boxes-near-this-tile) lookup like circles already had.
//!
//! Lifecycle: [`reset`] at the top of every (re)build, [`add`]/[`add_box`]/[`add_obb`] during
//! scatter/castle/camps, [`is_blocked`] per mover step.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Circle obstacles: `(centre_x, centre_z, radius)`, bucketed by centre tile.
static CIRCLES: LazyLock<RwLock<HashMap<(i32, i32), Vec<(f32, f32, f32)>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Oriented-box obstacles: `[cx, cz, hw, hd, cos_yaw, sin_yaw]`, bucketed by every tile the box's
/// circumscribing radius overlaps (so a box is found from ANY tile it actually covers, including
/// off-centre corners of a rotated box) — one entry duplicated across its covered tiles, same
/// trade-off circles already make. An axis-aligned box is just `cos=1, sin=0`. The point test
/// rotates the query into the box's local frame.
static BOX_BUCKETS: LazyLock<RwLock<HashMap<(i32, i32), Vec<[f32; 6]>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn tile(wx: f32, wz: f32) -> (i32, i32) {
    (wx.floor() as i32, wz.floor() as i32)
}

/// Every tile coordinate a box (centred `cx,cz`, half-extents `hw,hd`) can possibly overlap —
/// its rotation-agnostic circumscribing radius, so this over-covers a rotated box slightly rather
/// than under-covering it (the exact OBB test at query time still rejects false candidates).
fn box_tile_range(cx: f32, cz: f32, hw: f32, hd: f32) -> impl Iterator<Item = (i32, i32)> {
    let diag = (hw * hw + hd * hd).sqrt();
    let (tx0, tz0) = tile(cx - diag, cz - diag);
    let (tx1, tz1) = tile(cx + diag, cz + diag);
    (tx0..=tx1).flat_map(move |tx| (tz0..=tz1).map(move |tz| (tx, tz)))
}

/// Clear all blockers — call once before rebuilding the scene.
pub fn reset() {
    CIRCLES.write().unwrap().clear();
    BOX_BUCKETS.write().unwrap().clear();
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

/// Remove circular obstacles centred within ~0.2 units of `(wx, wz)`. Used when a tree is
/// felled so its trunk blocker doesn't linger as an invisible nub where the tree stood.
pub fn remove_at(wx: f32, wz: f32) {
    if let Some(bucket) = CIRCLES.write().unwrap().get_mut(&tile(wx, wz)) {
        bucket.retain(|&(cx, cz, _)| {
            let (ex, ez) = (wx - cx, wz - cz);
            ex * ex + ez * ez > 0.04 // keep anything more than 0.2 units away
        });
    }
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
    let b = [cx, cz, hw, hd, yaw.cos(), yaw.sin()];
    let mut buckets = BOX_BUCKETS.write().unwrap();
    for t in box_tile_range(cx, cz, hw, hd) {
        buckets.entry(t).or_default().push(b);
    }
}

/// Remove every oriented-box obstacle whose centre lies within `eps` world-units of `(cx, cz)`.
/// Used to **swing the fortress gate open**: dropping the gate's OBB clears the wall-gap so A*
/// (and the sallying ork column) can path straight through it. Pair with [`add_obb`] to
/// re-register the box when the gate shuts again. A box is duplicated across every tile its
/// circumscribing radius overlaps (see [`box_tile_range`]), so this must sweep every bucket —
/// every caller discards the return value, so no count is tracked.
pub fn remove_box_near(cx: f32, cz: f32, eps: f32) {
    let mut buckets = BOX_BUCKETS.write().unwrap();
    for bucket in buckets.values_mut() {
        bucket.retain(|b| (b[0] - cx).hypot(b[1] - cz) > eps);
    }
}

/// True if any obstacle lies within `margin` world-units of `(wx, wz)` — a clearance test for
/// placing standout props (apple trees) that must not crowd existing trunks/structures. `margin`
/// of `0` is equivalent to [`is_blocked`]. Scans the neighbourhood widened by `margin` so circles
/// up to `margin` tiles away are still caught.
pub fn any_within(wx: f32, wz: f32, margin: f32) -> bool {
    {
        let map = CIRCLES.read().unwrap();
        let (tx, tz) = tile(wx, wz);
        let reach = 1 + margin.max(0.0).ceil() as i32; // circles ≤1.0 + margin may span extra tiles
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                if let Some(bucket) = map.get(&(tx + dx, tz + dz)) {
                    for &(cx, cz, r) in bucket {
                        let (ex, ez) = (wx - cx, wz - cz);
                        let rr = r + margin;
                        if ex * ex + ez * ez < rr * rr {
                            return true;
                        }
                    }
                }
            }
        }
    }
    let buckets = BOX_BUCKETS.read().unwrap();
    let (tx, tz) = tile(wx, wz);
    // Boxes are pre-registered under every tile they cover (see `box_tile_range`), so the query's
    // own tile normally already has any overlapping box — the ±1 neighbourhood is just the same
    // rounding-edge safety margin circles use, cheap since a tile's box bucket is small.
    let reach = 1 + margin.max(0.0).ceil() as i32;
    for dx in -reach..=reach {
        for dz in -reach..=reach {
            if let Some(bucket) = buckets.get(&(tx + dx, tz + dz)) {
                for b in bucket {
                    let (ex, ez) = (wx - b[0], wz - b[1]);
                    let (cos, sin) = (b[4], b[5]);
                    let lx = cos * ex - sin * ez;
                    let lz = sin * ex + cos * ez;
                    if lx.abs() <= b[2] + margin && lz.abs() <= b[3] + margin {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// A small outward vector from nearby circular blockers (trees/cacti), weighted by how close the
/// body is to their collision shell. This is intentionally circle-only: walls and buildings already
/// slide well via axis-separated movement, while trunks are the snaggy case that benefits from a
/// subtle steer assist.
pub fn circle_repulsion(wx: f32, wz: f32, body_r: f32, sense_r: f32) -> (f32, f32) {
    let map = CIRCLES.read().unwrap();
    let (tx, tz) = tile(wx, wz);
    let sense = sense_r.max(0.0);
    let reach = 1 + (body_r + sense).ceil() as i32;
    let mut out_x = 0.0;
    let mut out_z = 0.0;
    for dx in -reach..=reach {
        for dz in -reach..=reach {
            if let Some(bucket) = map.get(&(tx + dx, tz + dz)) {
                for &(cx, cz, r) in bucket {
                    let (ex, ez) = (wx - cx, wz - cz);
                    let d2 = ex * ex + ez * ez;
                    if d2 <= 1e-6 {
                        continue;
                    }
                    let d = d2.sqrt();
                    let influence = r + body_r + sense;
                    if d < influence {
                        let w = ((influence - d) / sense.max(0.001)).clamp(0.0, 1.0);
                        out_x += (ex / d) * w;
                        out_z += (ez / d) * w;
                    }
                }
            }
        }
    }
    (out_x, out_z)
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
    let buckets = BOX_BUCKETS.read().unwrap();
    let (tx, tz) = tile(wx, wz);
    for dx in -1..=1 {
        for dz in -1..=1 {
            if let Some(bucket) = buckets.get(&(tx + dx, tz + dz)) {
                for b in bucket {
                    let (ex, ez) = (wx - b[0], wz - b[1]);
                    let (cos, sin) = (b[4], b[5]);
                    // Rotate the query into the box's local frame (inverse Y-rotation), then AABB-test.
                    let lx = cos * ex - sin * ez;
                    let lz = sin * ex + cos * ez;
                    if lx.abs() <= b[2] && lz.abs() <= b[3] {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// True if `(wx, wz)` lies inside any solid **box** obstacle (walls / towers / buildings / camp
/// structures) — the circle obstacles (tree trunks, clutter) are ignored. Backs [`wall_between`].
fn box_at(wx: f32, wz: f32) -> bool {
    let buckets = BOX_BUCKETS.read().unwrap();
    let (tx, tz) = tile(wx, wz);
    for dx in -1..=1 {
        for dz in -1..=1 {
            if let Some(bucket) = buckets.get(&(tx + dx, tz + dz)) {
                for b in bucket {
                    let (ex, ez) = (wx - b[0], wz - b[1]);
                    let (cos, sin) = (b[4], b[5]);
                    let lx = cos * ex - sin * ez;
                    let lz = sin * ex + cos * ez;
                    if lx.abs() <= b[2] && lz.abs() <= b[3] {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// True if a solid **box** obstacle — a wall, tower, building or camp structure — sits on the
/// straight line between `(ax, az)` and `(bx, bz)`: a melee/attack **line-of-sight** test. Combat
/// (hero swings, ork clubs, shaman/predator strikes) calls this so a blow can't land *through* a
/// wall — the movement layer already stops bodies clipping walls, but attack targeting was pure
/// distance and ignored them (orks clubbing the hero across a wall, and vice-versa).
///
/// Circles (tree trunks, ground clutter) are deliberately NOT tested — you can fight across a
/// sapling. Open gates register no box (the gate swing removes it, see [`remove_box_near`]), so
/// LOS threads a gate exactly like A* does. Samples box occupancy every ~0.25u along the segment —
/// finer than the thinnest registered wall (≥0.8u across) — so no wall is stepped over. The two
/// endpoints are skipped: an attacker or victim standing flush against a wall must still fight
/// along it rather than block itself.
pub fn wall_between(ax: f32, az: f32, bx: f32, bz: f32) -> bool {
    let (dx, dz) = (bx - ax, bz - az);
    let len = (dx * dx + dz * dz).sqrt();
    if len < 1e-3 {
        return false;
    }
    const STEP: f32 = 0.25;
    let n = (len / STEP).ceil().max(1.0) as i32;
    for i in 1..n {
        let t = i as f32 / n as f32;
        if box_at(ax + dx * t, az + dz * t) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The blocker store is a set of process-global statics, so tests that `reset()`/`add_box`
    /// must not run concurrently or they clobber each other's fixtures. Serialize them on one lock.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// A wall between attacker and target blocks the attack line-of-sight ([`wall_between`]),
    /// while a clear diagonal past the wall's end does not, and endpoints flush against the wall
    /// don't self-block. Guards the "orks club you through the wall" fix.
    #[test]
    fn wall_between_blocks_los_but_not_open_paths() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        // A thin wall spanning x∈[-2,2], centred on the z-axis at z=0 (0.4u thick across).
        add_box(0.0, 0.0, 2.0, 0.2);
        // Attacker south of the wall, target north of it, on the same x — line crosses the wall.
        assert!(wall_between(0.0, -1.5, 0.0, 1.5), "a wall on the line must block LOS");
        // Both on the same (south) side, no wall between — clear.
        assert!(!wall_between(-1.0, -1.5, 1.0, -1.5), "same-side attack must be clear");
        // Past the wall's end (x>2), the line misses the box — clear.
        assert!(!wall_between(3.0, -1.5, 3.0, 1.5), "a line past the wall end is clear");
        // Flush endpoints: standing on the wall face isn't a self-block for a short reach along it.
        assert!(!wall_between(-1.0, 0.25, -1.0, 0.6), "endpoints at the wall don't self-block");
        reset();
    }

    /// A blocker raised on top of a stationary hero (a build-mode producer building, or a
    /// War-Table wall/tower/ballista) can leave his CENTRE in the "penetration shell": outside the
    /// box itself, yet within `PLAYER_R` of a face so his body overlaps it. The hero's un-stick
    /// path (`player::movement`) keys off this distinction — `is_blocked` (centre strictly inside)
    /// reads FALSE in the shell, so the escape must test `any_within(.., PLAYER_R)`, which reads
    /// TRUE. Guards the "stuck inside a just-built structure" fix.
    #[test]
    fn penetration_shell_reads_overlapping_but_not_inside() {
        let _g = TEST_LOCK.lock().unwrap();
        const PLAYER_R: f32 = 0.22; // mirror player::movement::PLAYER_R
        reset();
        // A box spanning x∈[-1,1], z∈[-1,1] centred at the origin.
        add_box(0.0, 0.0, 1.0, 1.0);
        // 0.1 east of the east face (x=1.0): centre is OUTSIDE the box, but the 0.22 body overlaps.
        assert!(!is_blocked(1.1, 0.0), "shell centre must read outside the box");
        assert!(any_within(1.1, 0.0, PLAYER_R), "shell centre must read as body-overlapping");
        // A hero at rest sits ≥ PLAYER_R from the face (collision never lets him penetrate): both
        // read false there, so the broadened escape can't be abused to clip through a wall in play.
        assert!(!is_blocked(1.0 + PLAYER_R + 0.01, 0.0));
        assert!(!any_within(1.0 + PLAYER_R + 0.01, 0.0, PLAYER_R));
        reset();
    }

    #[test]
    fn circle_repulsion_points_away_from_nearby_trunks_only() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        add(0.0, 0.0, 0.5);
        add_box(2.0, 0.0, 0.5, 0.5);

        let (x, z) = circle_repulsion(0.75, 0.0, 0.22, 0.45);
        assert!(x > 0.0, "near a trunk on the east side should push east");
        assert!(z.abs() < 0.01, "a symmetric trunk contact should not add sideways noise");

        let (far_x, far_z) = circle_repulsion(2.0, 0.0, 0.22, 0.45);
        assert_eq!((far_x, far_z), (0.0, 0.0), "box blockers are ignored by the tree assist");
        reset();
    }
}
