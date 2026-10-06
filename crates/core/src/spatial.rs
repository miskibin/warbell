//! Reusable broad-phase point buckets. One flat link per point, no per-cell Vec allocations.
//! Candidates are a conservative square; callers keep their original distance/LOS/team rules.
use std::collections::HashMap;

pub struct PointGrid {
    cell_size: f64,
    heads: HashMap<(i32, i32), (usize, usize)>,
    next: Vec<usize>,
    bounds: (f64, f64, f64, f64),
}

impl Default for PointGrid {
    fn default() -> Self {
        Self::new(12.0)
    }
}

impl PointGrid {
    pub fn new(cell_size: f64) -> Self {
        assert!(cell_size.is_finite() && cell_size > 0.0);
        Self {
            cell_size,
            heads: HashMap::new(),
            next: Vec::new(),
            bounds: (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Indices correspond exactly to the supplied snapshot, including its tie-breaking rank.
    pub fn rebuild(&mut self, points: impl IntoIterator<Item = (f64, f64)>) {
        self.heads.clear();
        self.next.clear();
        self.bounds = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for (i, (x, z)) in points.into_iter().enumerate() {
            self.bounds.0 = self.bounds.0.min(x);
            self.bounds.1 = self.bounds.1.min(z);
            self.bounds.2 = self.bounds.2.max(x);
            self.bounds.3 = self.bounds.3.max(z);
            let cell = (
                (x / self.cell_size).floor() as i32,
                (z / self.cell_size).floor() as i32,
            );
            let entry = self.heads.entry(cell).or_insert((usize::MAX, 0));
            let prev = entry.0;
            entry.0 = i;
            entry.1 += 1;
            self.next.push(prev);
        }
    }

    pub fn candidates(&self, x: f64, z: f64, radius: f64) -> impl Iterator<Item = usize> + '_ {
        let lo_x = ((x - radius) / self.cell_size).floor() as i32;
        let hi_x = ((x + radius) / self.cell_size).floor() as i32;
        let lo_z = ((z - radius) / self.cell_size).floor() as i32;
        let hi_z = ((z + radius) / self.cell_size).floor() as i32;
        (lo_x..=hi_x).flat_map(move |cx| {
            (lo_z..=hi_z).flat_map(move |cz| {
                let mut cursor = self
                    .heads
                    .get(&(cx, cz))
                    .map_or(usize::MAX, |&(head, _)| head);
                std::iter::from_fn(move || {
                    if cursor == usize::MAX {
                        return None;
                    }
                    let i = cursor;
                    cursor = self.next[i];
                    Some(i)
                })
            })
        })
    }

    /// Dense battles and small sets are faster to scan contiguously than to chase bucket links.
    /// This fallback changes only the broad phase; callers must still apply exact rules.
    pub fn nearby_or_all(&self, x: f64, z: f64, radius: f64) -> impl Iterator<Item = usize> + '_ {
        let (min_x, min_z, max_x, max_z) = self.bounds;
        let compact = max_x - min_x <= radius * 2.0
            && max_z - min_z <= radius * 2.0
            && x + radius >= min_x
            && x - radius <= max_x
            && z + radius >= min_z
            && z - radius <= max_z;
        let mut full_scan = self.next.len() <= 96 || compact;
        if !full_scan {
            let mut count = 0;
            let lo_x = ((x - radius) / self.cell_size).floor() as i32;
            let hi_x = ((x + radius) / self.cell_size).floor() as i32;
            let lo_z = ((z - radius) / self.cell_size).floor() as i32;
            let hi_z = ((z + radius) / self.cell_size).floor() as i32;
            for cx in lo_x..=hi_x {
                for cz in lo_z..=hi_z {
                    count += self.heads.get(&(cx, cz)).map_or(0, |&(_, n)| n);
                }
            }
            full_scan = count * 2 > self.next.len();
        }
        (0..if full_scan { self.next.len() } else { 0 }).chain(
            self.candidates(x, z, radius)
                .take(if full_scan { 0 } else { usize::MAX }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_brute_force_at_boundaries_and_negative_coordinates() {
        let points: Vec<_> = (0..2000)
            .map(|i| {
                (
                    ((i * 37) % 601) as f64 - 300.0,
                    ((i * 83) % 599) as f64 - 300.0,
                )
            })
            .collect();
        let mut grid = PointGrid::default();
        grid.rebuild(points.iter().copied());
        for (x, z, r) in [
            (-12.0, 0.0, 0.0),
            (0.0, 0.0, 18.0),
            (-30.0, -30.0, 60.0),
            (12.0, 24.0, 12.0),
            (290.0, -290.0, 30.0),
        ] {
            let filter = |i: &usize| {
                let (px, pz) = points[*i];
                (px - x).hypot(pz - z) <= r
            };
            let mut actual: Vec<_> = grid.candidates(x, z, r).filter(filter).collect();
            actual.sort_unstable();
            let expected: Vec<_> = (0..points.len()).filter(filter).collect();
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn rebuild_removes_dead_and_moved_targets_and_reuses_capacity() {
        let mut grid = PointGrid::default();
        grid.rebuild([(0.0, 0.0), (1.0, 1.0), (300.0, 300.0)]);
        let capacity = (grid.heads.capacity(), grid.next.capacity());
        grid.rebuild([(100.0, 100.0)]);
        assert_eq!(grid.candidates(0.0, 0.0, 1.0).count(), 0);
        assert_eq!(
            grid.candidates(100.0, 100.0, 1.0).collect::<Vec<_>>(),
            vec![0]
        );
        grid.rebuild([]);
        assert_eq!(grid.candidates(100.0, 100.0, 1.0).count(), 0);
        assert_eq!(capacity, (grid.heads.capacity(), grid.next.capacity()));
    }

    #[test]
    fn dense_fallback_retains_all_indices_in_snapshot_order() {
        let mut grid = PointGrid::default();
        grid.rebuild((0..200).map(|i| ((i % 10) as f64, (i / 10) as f64)));
        assert_eq!(
            grid.nearby_or_all(5.0, 10.0, 30.0).collect::<Vec<_>>(),
            (0..200).collect::<Vec<_>>()
        );
    }
}
