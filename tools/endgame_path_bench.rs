//! Standalone frozen CPU benchmark: rustc -O tools/endgame_path_bench.rs -o target/path-bench.exe
//! Run before and after the pathfinding change; identical route checksums are required.
#[cfg(not(legacy))]
#[path = "../crates/core/src/pathfinding.rs"]
#[allow(dead_code)]
mod pathfinding;
#[cfg(legacy)]
#[path = "../target/legacy_pathfinding.rs"]
mod pathfinding;
#[cfg(not(reusable))]
use pathfinding::find_path;
use pathfinding::{Grid, PathPoint};
use std::{hint::black_box, time::Instant};

struct Arena {
    mode: u8,
}
impl Grid for Arena {
    fn cols(&self) -> i32 {
        160
    }
    fn rows(&self) -> i32 {
        160
    }
    fn standable(&self, _: i32, _: i32) -> bool {
        true
    }
    fn obstacle_tile(&self, x: i32, z: i32) -> bool {
        match self.mode {
            1 => (x == 50 && z < 115) || (x == 100 && z > 45),
            2 => x == 80,
            _ => false,
        }
    }
    fn wall_at(&self, _: f64, _: f64) -> bool {
        false
    }
    fn can_step(&self, _: i32, _: i32, _: i32, _: i32) -> bool {
        true
    }
}
fn main() {
    for mode in 0..3 {
        let grid = Arena { mode };
        #[cfg(reusable)]
        let mut work = pathfinding::PathWorkspace::default();
        #[allow(unused_mut)]
        let mut run = || {
            let mut checksum = 0u64;
            for i in 0..32 {
                let start = PathPoint {
                    x: 2.5,
                    z: i as f64 + 2.5,
                };
                let goal = PathPoint {
                    x: 155.5,
                    z: 150.5 - i as f64,
                };
                let budget = if mode == 1 { 30_000 } else { 8400 };
                #[cfg(not(reusable))]
                let route = find_path(&grid, start, goal, budget);
                #[cfg(reusable)]
                let route =
                    pathfinding::find_path_with_workspace(&grid, start, goal, budget, &mut work);
                for p in black_box(&route) {
                    checksum = checksum
                        .wrapping_mul(31)
                        .wrapping_add((p.x * 2.0) as u64 * 320 + (p.z * 2.0) as u64);
                }
            }
            black_box(checksum)
        };
        run();
        for sample in 0..5 {
            let start = Instant::now();
            let checksum = run();
            println!(
                "mode={mode} sample={sample} batch=32 ms={:.3} checksum={checksum}",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
