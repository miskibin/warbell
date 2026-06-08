//! Port of src/world/landmarks.ts — the five biome signature-landmark anchors.
//!
//! SCOPE: only the `LANDMARKS` data (with `r`) is ported — that's all the
//! obstacles RESERVED box needs. The runtime blocker-registration fn
//! (`registerLandmarkBlockers` / `footprintHitsCorridor`) is a render concern
//! and is intentionally NOT ported (see report).

use crate::tilemap::from_base;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandmarkSlot {
    pub x: i32,
    pub z: i32,
    /// half-extent in tiles (footprint reserved from scatter)
    pub r: i32,
}

// Authored in BASE coords; converted to the enlarged map via from_base + round.
const BASE_LANDMARKS: [(i32, i32, i32); 5] = [
    (26, 24, 2),   // FrozenSpire — snow summit
    (122, 22, 3),  // SunkenPyramid — desert far NE
    (118, 82, 2),  // StandingStones — SE rock frontier
    (72, 100, 1),  // GiantDeadTree — swamp far S
    (22, 88, 2),   // RuinedShrine — forest far SW
];

static LANDMARKS_CACHE: OnceLock<Vec<LandmarkSlot>> = OnceLock::new();

pub fn landmarks() -> &'static [LandmarkSlot] {
    LANDMARKS_CACHE.get_or_init(|| {
        BASE_LANDMARKS
            .iter()
            .map(|&(x, z, r)| {
                let (nx, nz) = from_base(x as f64, z as f64);
                LandmarkSlot {
                    x: nx.round() as i32,
                    z: nz.round() as i32,
                    r,
                }
            })
            .collect()
    })
}
