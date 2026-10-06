//! Frozen broad-phase benchmark, including index rebuild cost. Both selectors must agree exactly.
//! rustc -O tools/endgame_target_bench.rs -o target/target-bench.exe
#[path = "../crates/core/src/spatial.rs"]
mod spatial;
use spatial::PointGrid;
use std::{hint::black_box, time::Instant};

#[derive(Clone, Copy)]
struct Target {
    x: f64,
    z: f64,
    side: bool,
    building: bool,
}
#[inline(never)]
fn nearest(points: &[Target], from: Target, ids: impl Iterator<Item = usize>) -> usize {
    let mut unit: Option<(f64, usize)> = None;
    let mut building: Option<(f64, usize)> = None;
    for i in ids {
        let t = points[i];
        if t.side == from.side {
            continue;
        }
        let d = (t.x - from.x).hypot(t.z - from.z);
        if d > 18.0 {
            continue;
        }
        let slot = if t.building { &mut building } else { &mut unit };
        if slot.is_none_or(|(bd, bi)| d < bd || (d == bd && i < bi)) {
            *slot = Some((d, i));
        }
    }
    unit.or(building).map_or(usize::MAX, |(_, i)| i)
}
fn main() {
    for crowded in [false, true] {
        for n in [32, 128, 512, 1024] {
            let points: Vec<_> = (0..n)
                .map(|i| {
                    let scale = if crowded { 0.1 } else { 1.0 };
                    Target {
                        x: (((i * 37) % 299) as f64 - 149.0) * scale,
                        z: (((i * 83) % 293) as f64 - 146.0) * scale,
                        side: i % 2 == 0,
                        building: i % 5 == 0,
                    }
                })
                .collect();
            let mut grid = PointGrid::default();
            grid.rebuild(points.iter().map(|t| (t.x, t.z)));
            for &p in &points {
                assert_eq!(
                    nearest(&points, p, 0..n),
                    nearest(&points, p, grid.nearby_or_all(p.x, p.z, 18.001))
                );
            }
            for indexed in [false, true] {
                let start = Instant::now();
                let mut checksum = 0usize;
                for _ in 0..32 {
                    if indexed {
                        grid.rebuild(points.iter().map(|t| (t.x, t.z)));
                    }
                    for &p in black_box(&points) {
                        if indexed {
                            checksum ^= nearest(&points, p, grid.nearby_or_all(p.x, p.z, 18.001));
                        } else {
                            checksum ^= nearest(&points, p, 0..n);
                        }
                    }
                }
                println!(
                    "crowded={crowded} n={n} indexed={indexed} frames=32 ms={:.3} checksum={}",
                    start.elapsed().as_secs_f64() * 1000.0,
                    black_box(checksum)
                );
            }
        }
    }
}
