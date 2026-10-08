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
//! The buckets themselves are a **dense tile grid**, not a `HashMap`. A siege frame asks
//! `is_blocked` hundreds of thousands of times (every A* neighbour, every steering sample) and
//! almost all of those tiles are empty. Hashing `(tx, tz)` on every one of them measured ~200 ns
//! per query; an array index of the same 3×3 neighbourhood is ~60 ns (≈3.5× on an 8k-circle /
//! 700-box set). Tiles outside the island grid fall through to a tiny overflow map so a blocker
//! placed off the map is still solid. [`BlockView`] holds the read lock once for a whole path
//! search or steering fan — `std::sync::RwLock` is not re-entrant, so a view must not call back
//! into the locking wrappers.
//!
//! Lifecycle: [`reset`] at the top of every (re)build, [`add`]/[`add_box`]/[`add_obb`] during
//! scatter/castle/camps, [`is_blocked`] per mover step.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// World-tile origin of the dense index. The island lives well inside this (world XZ is a few
/// hundred units around the origin); anything outside uses [`TileGrid::overflow`].
const GRID_ORIGIN: i32 = -512;
const GRID_DIM: i32 = 1024;
const EMPTY: u32 = u32::MAX;

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

/// One shape family (circles or boxes), indexed by world tile.
struct TileGrid<T> {
    /// `cells[z * DIM + x] = bucket index`, or [`EMPTY`].
    cells: Vec<u32>,
    buckets: Vec<Vec<T>>,
    /// Cell indices that have a bucket, so [`TileGrid::reset`] doesn't walk the whole island.
    occupied: Vec<u32>,
    /// Tiles outside [`GRID_ORIGIN`].. — kept correct, just not the hot path.
    overflow: HashMap<(i32, i32), Vec<T>>,
}

impl<T> TileGrid<T> {
    fn new() -> Self {
        Self {
            cells: vec![EMPTY; (GRID_DIM * GRID_DIM) as usize],
            buckets: Vec::new(),
            occupied: Vec::new(),
            overflow: HashMap::new(),
        }
    }

    fn reset(&mut self) {
        for &i in &self.occupied {
            self.cells[i as usize] = EMPTY;
        }
        self.occupied.clear();
        self.buckets.clear();
        self.overflow.clear();
    }

    fn index(tx: i32, tz: i32) -> Option<usize> {
        let x = tx.checked_sub(GRID_ORIGIN)?;
        let z = tz.checked_sub(GRID_ORIGIN)?;
        if (0..GRID_DIM).contains(&x) && (0..GRID_DIM).contains(&z) {
            Some(z as usize * GRID_DIM as usize + x as usize)
        } else {
            None
        }
    }

    fn push(&mut self, tx: i32, tz: i32, item: T) {
        let Some(i) = Self::index(tx, tz) else {
            self.overflow.entry((tx, tz)).or_default().push(item);
            return;
        };
        let slot = self.cells[i];
        if slot == EMPTY {
            let id = self.buckets.len() as u32;
            self.cells[i] = id;
            self.occupied.push(i as u32);
            self.buckets.push(vec![item]);
        } else {
            self.buckets[slot as usize].push(item);
        }
    }

    fn bucket(&self, tx: i32, tz: i32) -> Option<&[T]> {
        let Some(i) = Self::index(tx, tz) else {
            return self.overflow.get(&(tx, tz)).map(Vec::as_slice);
        };
        let slot = self.cells[i];
        if slot == EMPTY { None } else { Some(self.buckets[slot as usize].as_slice()) }
    }

    fn bucket_mut(&mut self, tx: i32, tz: i32) -> Option<&mut Vec<T>> {
        let Some(i) = Self::index(tx, tz) else {
            return self.overflow.get_mut(&(tx, tz));
        };
        let slot = self.cells[i];
        if slot == EMPTY { None } else { Some(&mut self.buckets[slot as usize]) }
    }

    fn retain(&mut self, mut f: impl FnMut(&T) -> bool) {
        for b in &mut self.buckets {
            b.retain(&mut f);
        }
        for b in self.overflow.values_mut() {
            b.retain(&mut f);
        }
    }
}

struct Store {
    circles: TileGrid<(f32, f32, f32)>,
    boxes: TileGrid<[f32; 6]>,
    // Placement-only footprints: walkable foliage still needs visible breathing room.
    visual: TileGrid<(f32, f32, f32)>,
}

impl Store {
    fn new() -> Self {
        Self { circles: TileGrid::new(), boxes: TileGrid::new(), visual: TileGrid::new() }
    }
}

static STORE: LazyLock<RwLock<Store>> = LazyLock::new(|| RwLock::new(Store::new()));

fn write<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.write().unwrap();
    f(&mut guard)
}

/// A read-locked view of every blocker. Cheap to create once per path search or steering
/// step; each [`BlockView::is_blocked`] after that is an array lookup, not another lock.
pub struct BlockView {
    guard: std::sync::RwLockReadGuard<'static, Store>,
}

impl BlockView {
    /// True if `(wx, wz)` lies inside any solid obstacle (a circle or an oriented box).
    pub fn is_blocked(&self, wx: f32, wz: f32) -> bool {
        let (tx, tz) = tile(wx, wz);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(bucket) = self.guard.circles.bucket(tx + dx, tz + dz) {
                    for &(cx, cz, r) in bucket {
                        let (ex, ez) = (wx - cx, wz - cz);
                        if ex * ex + ez * ez < r * r {
                            return true;
                        }
                    }
                }
            }
        }
        self.box_at(wx, wz)
    }

    /// True if any obstacle lies within `margin` world-units of `(wx, wz)`. `margin` of `0` is
    /// equivalent to [`BlockView::is_blocked`].
    pub fn any_within(&self, wx: f32, wz: f32, margin: f32) -> bool {
        let (tx, tz) = tile(wx, wz);
        let reach = 1 + margin.max(0.0).ceil() as i32;
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                if let Some(bucket) = self.guard.circles.bucket(tx + dx, tz + dz) {
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
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                if let Some(bucket) = self.guard.boxes.bucket(tx + dx, tz + dz) {
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

    /// Placement clearance includes walk-through props without changing movement or LOS.
    pub fn any_visual_within(&self, wx: f32, wz: f32, margin: f32) -> bool {
        if self.any_within(wx, wz, margin) { return true; }
        let margin = margin.max(0.0);
        let (tx, tz) = tile(wx, wz);
        let reach = margin.ceil() as i32;
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                if let Some(bucket) = self.guard.visual.bucket(tx + dx, tz + dz) {
                    for &(cx, cz, radius) in bucket {
                        if (wx - cx).hypot(wz - cz) < radius + margin { return true; }
                    }
                }
            }
        }
        false
    }

    /// A small outward vector from nearby circular blockers, weighted by how close the body is
    /// to their collision shell. Circle-only: walls already slide via axis-separated movement.
    pub fn circle_repulsion(&self, wx: f32, wz: f32, body_r: f32, sense_r: f32) -> (f32, f32) {
        let (tx, tz) = tile(wx, wz);
        let sense = sense_r.max(0.0);
        let reach = 1 + (body_r + sense).ceil() as i32;
        let mut out_x = 0.0;
        let mut out_z = 0.0;
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                if let Some(bucket) = self.guard.circles.bucket(tx + dx, tz + dz) {
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

    fn box_at(&self, wx: f32, wz: f32) -> bool {
        let (tx, tz) = tile(wx, wz);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(bucket) = self.guard.boxes.bucket(tx + dx, tz + dz) {
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

    /// [`wall_between`] against an already-held view, so a multi-sample line of sight takes
    /// one lock instead of one per 0.25u step.
    pub fn wall_between(&self, ax: f32, az: f32, bx: f32, bz: f32) -> bool {
        let (dx, dz) = (bx - ax, bz - az);
        let len = (dx * dx + dz * dz).sqrt();
        if len < 1e-3 {
            return false;
        }
        const STEP: f32 = 0.25;
        let n = (len / STEP).ceil().max(1.0) as i32;
        for i in 1..n {
            let t = i as f32 / n as f32;
            if self.box_at(ax + dx * t, az + dz * t) {
                return true;
            }
        }
        false
    }
}

/// Lock the blocker set for a batch of queries. Drop the view before adding or removing
/// blockers on this thread — the lock is not re-entrant.
pub fn read() -> BlockView {
    BlockView { guard: STORE.read().unwrap() }
}

/// Clear all blockers — call once before rebuilding the scene.
pub fn reset() {
    write(|s| {
        s.circles.reset();
        s.boxes.reset();
        s.visual.reset();
    });
}

/// Mark a solid circular obstacle of `radius` (world units) centred at `(wx, wz)`. A radius
/// ≤ 0 registers nothing. Keep `radius ≤ 1.0` — the neighbour-only [`is_blocked`] scan
/// assumes it (use [`add_box`] for anything bigger than a trunk).
pub fn add(wx: f32, wz: f32, radius: f32) {
    if radius <= 0.0 {
        return;
    }
    let (tx, tz) = tile(wx, wz);
    write(|s| s.circles.push(tx, tz, (wx, wz, radius)));
}

/// Reserve a whole visible footprint for later placement. Unlike a solid circle, this may
/// have any radius: it is indexed in every covered tile. Movement and LOS ignore it.
pub fn reserve_visual(wx: f32, wz: f32, radius: f32) {
    if !radius.is_finite() || radius <= 0.0 { return; }
    let (tx0, tz0) = tile(wx - radius, wz - radius);
    let (tx1, tz1) = tile(wx + radius, wz + radius);
    write(|s| {
        for tx in tx0..=tx1 {
            for tz in tz0..=tz1 { s.visual.push(tx, tz, (wx, wz, radius)); }
        }
    });
}

/// Remove circular obstacles centred within ~0.2 units of `(wx, wz)`. Used when a tree is
/// felled so its trunk blocker doesn't linger as an invisible nub where the tree stood.
pub fn remove_at(wx: f32, wz: f32) {
    let (tx, tz) = tile(wx, wz);
    write(|s| {
        if let Some(bucket) = s.circles.bucket_mut(tx, tz) {
            bucket.retain(|&(cx, cz, _)| {
                let (ex, ez) = (wx - cx, wz - cz);
                ex * ex + ez * ez > 0.04 // keep anything more than 0.2 units away
            });
        }
    });
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
    write(|s| {
        for (tx, tz) in box_tile_range(cx, cz, hw, hd) {
            s.boxes.push(tx, tz, b);
        }
    });
}

/// Remove every oriented-box obstacle whose centre lies within `eps` world-units of `(cx, cz)`.
/// Used to **swing the fortress gate open**: dropping the gate's OBB clears the wall-gap so A*
/// (and the sallying ork column) can path straight through it. Pair with [`add_obb`] to
/// re-register the box when the gate shuts again. A box is duplicated across every tile its
/// circumscribing radius overlaps (see [`box_tile_range`]), so this sweeps every occupied bucket.
pub fn remove_box_near(cx: f32, cz: f32, eps: f32) {
    write(|s| {
        s.boxes.retain(|b| (b[0] - cx).hypot(b[1] - cz) > eps);
    });
}

/// True if any obstacle lies within `margin` world-units of `(wx, wz)` — a clearance test for
/// placing standout props (apple trees) that must not crowd existing trunks/structures. `margin`
/// of `0` is equivalent to [`is_blocked`]. Scans the neighbourhood widened by `margin` so circles
/// up to `margin` tiles away are still caught.
pub fn any_within(wx: f32, wz: f32, margin: f32) -> bool {
    read().any_within(wx, wz, margin)
}

/// Clearance against both actual solids and placement-only visual footprints.
pub fn any_visual_within(wx: f32, wz: f32, margin: f32) -> bool {
    read().any_visual_within(wx, wz, margin)
}

/// A small outward vector from nearby circular blockers (trees/cacti), weighted by how close the
/// body is to their collision shell. This is intentionally circle-only: walls and buildings already
/// slide well via axis-separated movement, while trunks are the snaggy case that benefits from a
/// subtle steer assist.
pub fn circle_repulsion(wx: f32, wz: f32, body_r: f32, sense_r: f32) -> (f32, f32) {
    read().circle_repulsion(wx, wz, body_r, sense_r)
}

/// True if `(wx, wz)` lies inside any solid obstacle (a circle or an oriented box).
pub fn is_blocked(wx: f32, wz: f32) -> bool {
    read().is_blocked(wx, wz)
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
    read().wall_between(ax, az, bx, bz)
}

/// The blocker store is a set of process-global statics, so tests that `reset()`/`add_box` must not
/// run concurrently or they clobber each other's fixtures. Every such test — here and in `steer`,
/// whose stepping gate consults this store — serializes on this one lock.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_visual_props_reserve_space_without_blocking_movement() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        reserve_visual(-1.2, 0.4, 2.8);
        assert!(any_visual_within(1.5, 0.4, 0.0));
        assert!(any_visual_within(2.0, 0.4, 0.5));
        assert!(!any_visual_within(2.2, 0.4, 0.5));
        assert!(!is_blocked(-1.2, 0.4));
        assert!(!wall_between(-2.0, 0.4, 2.0, 0.4));
        add_box(6.0, 0.0, 0.6, 0.4);
        assert!(any_visual_within(6.0, 0.0, 0.0));
        reset();
        assert!(!any_visual_within(-1.2, 0.4, 0.5));
    }

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

    /// The dense grid (and its off-map overflow) must answer the same point tests as a flat
    /// scan of the registered shapes — including rotated boxes, tile-boundary circles, a
    /// remove, and a blocker far outside the island.
    #[test]
    fn dense_grid_matches_a_flat_scan() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();

        struct Rng(u32);
        impl Rng {
            fn f(&mut self) -> f32 {
                self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
                (self.0 >> 8) as f32 / 16_777_216.0
            }
            fn range(&mut self, a: f32, b: f32) -> f32 {
                a + (b - a) * self.f()
            }
        }
        let mut rng = Rng(0xB10C);
        let mut circles: Vec<(f32, f32, f32)> = Vec::new();
        let mut boxes: Vec<[f32; 6]> = Vec::new();

        for _ in 0..240 {
            let c = (rng.range(-80.0, 80.0), rng.range(-90.0, 120.0), rng.range(0.12, 0.85));
            add(c.0, c.1, c.2);
            circles.push(c);
        }
        // Trunk whose centre sits on a tile boundary — the neighbour scan has to see it.
        add(-0.2, 0.4, 0.7);
        circles.push((-0.2, 0.4, 0.7));
        // Off the island grid entirely (overflow map, not the dense array).
        add(900.0, -900.0, 0.6);
        circles.push((900.0, -900.0, 0.6));

        for _ in 0..60 {
            let yaw = rng.range(-1.5, 1.5);
            let b = [rng.range(-30.0, 30.0), rng.range(-30.0, 30.0), rng.range(0.3, 2.4), rng.range(0.2, 1.2), yaw.cos(), yaw.sin()];
            add_obb(b[0], b[1], b[2], b[3], yaw);
            boxes.push(b);
        }

        fn flat_blocked(circles: &[(f32, f32, f32)], boxes: &[[f32; 6]], x: f32, z: f32) -> bool {
            for &(cx, cz, r) in circles {
                let (ex, ez) = (x - cx, z - cz);
                if ex * ex + ez * ez < r * r {
                    return true;
                }
            }
            for b in boxes {
                let (ex, ez) = (x - b[0], z - b[1]);
                let lx = b[4] * ex - b[5] * ez;
                let lz = b[5] * ex + b[4] * ez;
                if lx.abs() <= b[2] && lz.abs() <= b[3] {
                    return true;
                }
            }
            false
        }
        fn flat_within(circles: &[(f32, f32, f32)], boxes: &[[f32; 6]], x: f32, z: f32, margin: f32) -> bool {
            for &(cx, cz, r) in circles {
                let (ex, ez) = (x - cx, z - cz);
                let rr = r + margin;
                if ex * ex + ez * ez < rr * rr {
                    return true;
                }
            }
            for b in boxes {
                let (ex, ez) = (x - b[0], z - b[1]);
                let lx = b[4] * ex - b[5] * ez;
                let lz = b[5] * ex + b[4] * ez;
                if lx.abs() <= b[2] + margin && lz.abs() <= b[3] + margin {
                    return true;
                }
            }
            false
        }

        let view = read();
        for _ in 0..1500 {
            let (x, z) = (rng.range(-100.0, 100.0), rng.range(-110.0, 140.0));
            assert_eq!(view.is_blocked(x, z), flat_blocked(&circles, &boxes, x, z), "blocked mismatch at ({x}, {z})");
            assert_eq!(view.any_within(x, z, 0.35), flat_within(&circles, &boxes, x, z, 0.35), "margin mismatch at ({x}, {z})");
        }
        assert!(view.is_blocked(900.0, -900.0), "off-map trunk centre must still be solid");
        assert!(!view.is_blocked(900.0, -897.0), "off-map trunk must not fill the neighbourhood");
        drop(view);

        remove_at(-0.2, 0.4);
        circles.retain(|&(cx, cz, _)| (cx + 0.2) * (cx + 0.2) + (cz - 0.4) * (cz - 0.4) > 0.04);
        assert_eq!(is_blocked(-0.2, 0.4), flat_blocked(&circles, &boxes, -0.2, 0.4));

        let (bx, bz) = (boxes[0][0], boxes[0][1]);
        remove_box_near(bx, bz, 0.05);
        boxes.retain(|b| (b[0] - bx).hypot(b[1] - bz) > 0.05);
        assert_eq!(is_blocked(bx, bz), flat_blocked(&circles, &boxes, bx, bz), "removed box must leave no ghost");
        reset();
    }
}
