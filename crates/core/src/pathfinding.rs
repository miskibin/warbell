//! Port of src/world/pathfinding.ts — 8-directional A* over the tile grid.
//!
//! The TS version reaches into tileMap/obstacles/houseBlockers module-globals;
//! here those queries are abstracted behind the `Grid` trait so (a) tests inject
//! a tiny hand-drawn grid (mirroring the vitest `vi.mock`) and (b) the Bevy game
//! later implements `Grid` over the real tilemap. Returns world-space waypoints
//! at tile centres (x.5, z.5); empty vec if no path or already at the goal cell.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathPoint {
    pub x: f64,
    pub z: f64,
}

/// Everything A* needs to know about the world. Mirrors the chokepoint queries
/// the TS `isWalkable` made into tileMap/obstacles/houseBlockers.
pub trait Grid {
    fn cols(&self) -> i32;
    fn rows(&self) -> i32;
    /// Tile-level: in-bounds AND has a height class (land OR bridge deck).
    fn standable(&self, x: i32, z: i32) -> bool;
    /// Tile holds a collidable prop (tree/boulder/…) → impassable.
    fn obstacle_tile(&self, x: i32, z: i32) -> bool;
    /// Continuous-space house/wall AABB predicate (the TS `houseBlocksAt`).
    fn wall_at(&self, x: f64, z: f64) -> bool;
    /// Climb feasibility f→t: ≤ 1 height-class change (the TS `canStep`).
    fn can_step(&self, fx: i32, fz: i32, tx: i32, tz: i32) -> bool;
}

const NEIGHBORS: [(i32, i32); 8] =
    [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];

/// Can a walker occupy tile (cx,cz) at all — terrain standable + no prop/house.
fn is_walkable(g: &impl Grid, cx: i32, cz: i32) -> bool {
    if cx < 0 || cz < 0 || cx >= g.cols() || cz >= g.rows() {
        return false;
    }
    if g.wall_at(cx as f64 + 0.5, cz as f64 + 0.5) {
        return false;
    }
    if g.obstacle_tile(cx, cz) {
        return false;
    }
    g.standable(cx, cz)
}

/// Nearest walkable tile to (cx,cz), ring-searched outward.
fn nearest_walkable(g: &impl Grid, cx: i32, cz: i32, max_r: i32) -> Option<(i32, i32)> {
    if is_walkable(g, cx, cz) {
        return Some((cx, cz));
    }
    for r in 1..=max_r {
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dz.abs()) != r {
                    continue; // current ring only
                }
                if is_walkable(g, cx + dx, cz + dz) {
                    return Some((cx + dx, cz + dz));
                }
            }
        }
    }
    None
}

/// An entry on the A* open frontier. Ordered as a **min-heap** by `(f, key)` so the cheapest node
/// pops first, with the same deterministic tie-break the old linear scan used (lowest f, then lowest
/// key). `BinaryHeap` is a max-heap, so `Ord` is reversed below.
#[derive(Clone, Copy)]
struct OpenEntry {
    f: f64,
    g: f64,
    x: i32,
    z: i32,
    key: i64,
}

impl PartialEq for OpenEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f == other.f && self.key == other.key
    }
}
impl Eq for OpenEntry {}
impl Ord for OpenEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse both comparisons: BinaryHeap yields the GREATEST element, but we want the
        // smallest (f, key). `f` is finite here (g + Euclidean h), so `partial_cmp` never None.
        other
            .f
            .partial_cmp(&self.f)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.key.cmp(&self.key))
    }
}
impl PartialOrd for OpenEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A* path on the tile grid. `max_nodes` bounds the search (default 800 in TS).
pub fn find_path(g: &impl Grid, start: PathPoint, goal: PathPoint, max_nodes: u32) -> Vec<PathPoint> {
    let cols = g.cols() as i64;
    let sx0 = start.x.floor() as i32;
    let sz0 = start.z.floor() as i32;
    let gx0 = goal.x.floor() as i32;
    let gz0 = goal.z.floor() as i32;
    if sx0 == gx0 && sz0 == gz0 {
        return vec![];
    }
    let (sx, sz) = match nearest_walkable(g, sx0, sz0, 5) {
        Some(p) => p,
        None => return vec![],
    };
    let (gx, gz) = match nearest_walkable(g, gx0, gz0, 5) {
        Some(p) => p,
        None => return vec![],
    };
    if sx == gx && sz == gz {
        return vec![];
    }

    let key = |x: i32, z: i32| -> i64 { z as i64 * cols + x as i64 };
    let h = |x: i32, z: i32| -> f64 { ((x - gx) as f64).hypot((z - gz) as f64) };

    // Binary-heap open set instead of a linear scan: popping the cheapest node is O(log n) rather
    // than O(n), so a long or unreachable search (which can grow the frontier into the thousands
    // before hitting `max_nodes`) no longer costs O(n²). `best_g` holds the cheapest known cost to
    // each node; the heap may carry stale duplicates (no decrease-key), so a pop whose `g` is worse
    // than `best_g` (or already closed) is skipped — textbook lazy-deletion A*.
    let mut open: BinaryHeap<OpenEntry> = BinaryHeap::new();
    let mut best_g: HashMap<i64, f64> = HashMap::new();
    let mut closed: HashSet<i64> = HashSet::new();
    let mut came_from: HashMap<i64, i64> = HashMap::new();

    let start_key = key(sx, sz);
    open.push(OpenEntry { f: h(sx, sz), g: 0.0, x: sx, z: sz, key: start_key });
    best_g.insert(start_key, 0.0);

    let mut visited: u32 = 0;
    while let Some(best) = open.pop() {
        let best_key = best.key;
        // Stale heap entry — a cheaper route to this node was found (or it's already expanded)
        // after this copy was pushed. Skip without spending budget.
        if closed.contains(&best_key) || best.g > *best_g.get(&best_key).unwrap_or(&f64::INFINITY) {
            continue;
        }
        if visited >= max_nodes {
            break;
        }
        visited += 1;
        closed.insert(best_key);

        let (cx, cz, gscore) = (best.x, best.z, best.g);
        if cx == gx && cz == gz {
            // Reconstruct: goal down to (but excluding) start, then reverse.
            let mut path: Vec<PathPoint> = Vec::new();
            let mut k = best_key;
            while k != start_key {
                let px = (k % cols) as f64 + 0.5;
                let pz = (k / cols) as f64 + 0.5;
                path.push(PathPoint { x: px, z: pz });
                match came_from.get(&k) {
                    Some(&prev) => k = prev,
                    None => break,
                }
            }
            path.reverse();
            return path;
        }

        for (dx, dz) in NEIGHBORS {
            let nx = cx + dx;
            let nz = cz + dz;
            if nx < 0 || nz < 0 || nx >= g.cols() || nz >= g.rows() {
                continue;
            }
            let nk = key(nx, nz);
            if closed.contains(&nk) {
                continue;
            }
            if !is_walkable(g, nx, nz) {
                continue;
            }
            // Edge-midpoint wall check: reject a step crossing a thin wall AABB
            // that both tile centres miss. Gate gaps register no blocker.
            let mid_x = (cx + nx) as f64 / 2.0 + 0.5;
            let mid_z = (cz + nz) as f64 / 2.0 + 0.5;
            if g.wall_at(mid_x, mid_z) {
                continue;
            }
            // Climb gate: Δ ≥ 2 face is a cliff → route around.
            if !g.can_step(cx, cz, nx, nz) {
                continue;
            }
            // Corner-cut prevention: both orthogonal cells must be walkable AND
            // a legal step.
            if dx != 0 && dz != 0 {
                if !is_walkable(g, cx + dx, cz) || !is_walkable(g, cx, cz + dz) {
                    continue;
                }
                if !g.can_step(cx, cz, cx + dx, cz) || !g.can_step(cx, cz, cx, cz + dz) {
                    continue;
                }
            }
            let step = if dx != 0 && dz != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
            let ng = gscore + step;
            let better = match best_g.get(&nk) {
                Some(&existing) => ng < existing,
                None => true,
            };
            if better {
                best_g.insert(nk, ng);
                came_from.insert(nk, best_key);
                open.push(OpenEntry { f: ng + h(nx, nz), g: ng, x: nx, z: nz, key: nk });
            }
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    // Port of src/world/pathfinding.test.ts — same hand-drawn-grid legend:
    //   .  walkable ground (h0)   #  cliff (h>=2, impassable)
    //   ~  water (absent tile)    B  bridge over water (walkable)
    //   O  obstacle prop (blocked)
    use super::*;

    struct MockGrid {
        cols: i32,
        rows: i32,
        heights: HashMap<(i32, i32), i32>,
        bridges: HashSet<(i32, i32)>,
        obstacles: HashSet<(i32, i32)>,
        wall: Option<Box<dyn Fn(f64, f64) -> bool>>,
    }

    impl MockGrid {
        fn new() -> Self {
            Self {
                cols: 30,
                rows: 30,
                heights: HashMap::new(),
                bridges: HashSet::new(),
                obstacles: HashSet::new(),
                wall: None,
            }
        }
        fn h_class(&self, x: i32, z: i32) -> Option<i32> {
            if let Some(&v) = self.heights.get(&(x, z)) {
                Some(v)
            } else if self.bridges.contains(&(x, z)) {
                Some(1)
            } else {
                None
            }
        }
        fn set_map(&mut self, rows: &[&str]) {
            self.heights.clear();
            self.bridges.clear();
            self.obstacles.clear();
            for (z, row) in rows.iter().enumerate() {
                for (x, ch) in row.chars().enumerate() {
                    let at = (x as i32, z as i32);
                    match ch {
                        '.' => {
                            self.heights.insert(at, 0);
                        }
                        '#' => {
                            self.heights.insert(at, 3);
                        }
                        'O' => {
                            self.heights.insert(at, 0);
                            self.obstacles.insert(at);
                        }
                        'B' => {
                            self.bridges.insert(at);
                        }
                        _ => {} // '~' water → leave absent
                    }
                }
            }
        }
    }

    impl Grid for MockGrid {
        fn cols(&self) -> i32 {
            self.cols
        }
        fn rows(&self) -> i32 {
            self.rows
        }
        fn standable(&self, x: i32, z: i32) -> bool {
            if x < 0 || z < 0 || x >= self.cols || z >= self.rows {
                return false;
            }
            self.h_class(x, z).is_some()
        }
        fn obstacle_tile(&self, x: i32, z: i32) -> bool {
            self.obstacles.contains(&(x, z))
        }
        fn wall_at(&self, x: f64, z: f64) -> bool {
            self.wall.as_ref().map(|w| w(x, z)).unwrap_or(false)
        }
        fn can_step(&self, fx: i32, fz: i32, tx: i32, tz: i32) -> bool {
            match (self.h_class(fx, fz), self.h_class(tx, tz)) {
                (Some(fc), Some(tc)) => (tc - fc).abs() <= 1,
                _ => false,
            }
        }
    }

    fn p(x: f64, z: f64) -> PathPoint {
        PathPoint { x, z }
    }
    fn has(path: &[PathPoint], x: i32, z: i32) -> bool {
        path.iter().any(|q| q.x == x as f64 + 0.5 && q.z == z as f64 + 0.5)
    }

    #[test]
    fn walks_a_straight_line() {
        let mut g = MockGrid::new();
        g.set_map(&["......", "......", "......"]);
        let path = find_path(&g, p(0.0, 0.0), p(5.0, 0.0), 800);
        assert_eq!(path.len(), 5);
        assert_eq!(*path.last().unwrap(), p(5.5, 0.5));
        assert!(!has(&path, 0, 0)); // start excluded
    }

    #[test]
    fn uses_diagonals() {
        let mut g = MockGrid::new();
        g.set_map(&["....", "....", "....", "...."]);
        let path = find_path(&g, p(0.0, 0.0), p(3.0, 3.0), 800);
        assert_eq!(path.len(), 3); // three diagonal steps, not six
        assert_eq!(*path.last().unwrap(), p(3.5, 3.5));
    }

    #[test]
    fn routes_around_a_cliff_wall() {
        let mut g = MockGrid::new();
        g.set_map(&[".....", ".###.", "....."]);
        let path = find_path(&g, p(0.0, 1.0), p(4.0, 1.0), 800);
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), p(4.5, 1.5));
        for c in [1, 2, 3] {
            assert!(!has(&path, c, 1));
        }
    }

    #[test]
    fn routes_around_a_blocking_prop() {
        let mut g = MockGrid::new();
        g.set_map(&["...", ".O.", "..."]);
        let path = find_path(&g, p(0.0, 1.0), p(2.0, 1.0), 800);
        assert!(!path.is_empty());
        assert!(!has(&path, 1, 1));
    }

    #[test]
    fn returns_empty_when_goal_walled_off() {
        let mut g = MockGrid::new();
        g.set_map(&[".....", ".###.", ".#.#.", ".###.", "....."]);
        assert!(find_path(&g, p(0.0, 0.0), p(2.0, 2.0), 800).is_empty());
    }

    #[test]
    fn returns_empty_at_node_budget() {
        let mut g = MockGrid::new();
        g.set_map(&[
            "..........", "..........", "..........", "..........", "..........",
        ]);
        assert!(find_path(&g, p(0.0, 0.0), p(9.0, 4.0), 3).is_empty());
    }

    #[test]
    fn returns_empty_when_start_equals_goal_cell() {
        let mut g = MockGrid::new();
        g.set_map(&["...", "...", "..."]);
        assert!(find_path(&g, p(1.0, 1.0), p(1.0, 1.0), 800).is_empty());
    }

    #[test]
    fn routes_around_thin_boundary_wall_to_gate_gap() {
        let mut g = MockGrid::new();
        g.set_map(&[".......", ".......", "......."]);
        g.wall = Some(Box::new(|_x: f64, z: f64| {
            (z - 1.0).abs() <= 0.3 && (0.0..5.0).contains(&_x)
        }));
        let path = find_path(&g, p(2.0, 0.0), p(2.0, 2.0), 800);
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), p(2.5, 2.5));
        assert!(has(&path, 5, 1) || has(&path, 6, 1)); // detour through gate
        // must NOT punch straight across the wall at x=2
        assert!(!(has(&path, 2, 0) && has(&path, 2, 2) && path.len() == 2));
    }

    #[test]
    fn crosses_thin_wall_through_gate_gap() {
        let mut g = MockGrid::new();
        g.set_map(&[".......", ".......", "......."]);
        g.wall = Some(Box::new(|_x: f64, z: f64| {
            (z - 1.0).abs() <= 0.3 && (0.0..5.0).contains(&_x)
        }));
        // start/goal aligned on the gate column (x=5) → straight through.
        let path = find_path(&g, p(5.0, 0.0), p(5.0, 2.0), 800);
        assert_eq!(path.len(), 2);
    }

    #[test]
    fn bridge_tile_is_walkable_water() {
        let mut g = MockGrid::new();
        g.set_map(&["~~~~~", "..B..", "~~~~~"]);
        let crossed = find_path(&g, p(0.0, 1.0), p(4.0, 1.0), 800);
        assert!(!crossed.is_empty());
        assert!(has(&crossed, 2, 1)); // uses the bridge span

        // Same layout without the bridge: the water gap is impassable.
        g.set_map(&["~~~~~", "..~..", "~~~~~"]);
        assert!(find_path(&g, p(0.0, 1.0), p(4.0, 1.0), 800).is_empty());
    }

    /// Fill an `n×n` MockGrid with open height-0 ground (bypasses the string `set_map`).
    fn open_grid(n: i32) -> MockGrid {
        let mut g = MockGrid::new();
        g.cols = n;
        g.rows = n;
        for x in 0..n {
            for z in 0..n {
                g.heights.insert((x, z), 0);
            }
        }
        g
    }

    #[test]
    fn solves_large_open_grid() {
        // A big open grid grows the frontier into the thousands of open nodes — the war-party-at-the-
        // ork-base case. Under the old linear-scan open set this was O(n²); the binary heap keeps it
        // quick. Assert the optimal corner-to-corner route (pure diagonals on an open grid).
        let n = 120;
        let g = open_grid(n);
        let path = find_path(&g, p(0.0, 0.0), p((n - 1) as f64, (n - 1) as f64), 100_000);
        assert_eq!(*path.last().unwrap(), p(n as f64 - 0.5, n as f64 - 0.5));
        assert_eq!(path.len(), (n - 1) as usize);
    }

    #[test]
    fn unreachable_goal_on_large_grid_terminates() {
        // Goal sealed behind a full-height cliff wall (the worst case behind the spike: orks behind
        // the Hold walls). A* must drain the reachable frontier and return empty — the point is that
        // it TERMINATES cheaply instead of blowing up O(n²) over a huge open set.
        let n = 80;
        let mut g = open_grid(n);
        for z in 0..n {
            g.heights.insert((40, z), 3); // an impassable cliff column splitting the grid in two
        }
        let path = find_path(&g, p(1.0, 1.0), p((n - 1) as f64, (n - 1) as f64), 10_000);
        assert!(path.is_empty());
    }
}
