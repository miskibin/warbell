//! Port of src/world/cityPlan.ts — castle layout constants.
//!
//! SCOPE: only the pieces the obstacles port + reachability test need are ported
//! here — `snap_to_cardinal`, `is_inside_castle`, and `CASTLE_BOUNDS`. The
//! purely-visual slot tables (HOUSE_SLOTS, WALL_SLOTS, GATE_SLOTS, TOWER_SLOTS,
//! FARM_SLOT and their `face_center`/`door_in_front` helpers) are render-only and
//! are intentionally NOT ported (no pure-logic consumer; see report).

use crate::tilemap::{CENTER_X, CENTER_Z};

/// Wall perimeter (also the footprint reserved from scatter). Half-extents
/// 13 × 9 about the map centre.
pub const CASTLE_MIN_X: f64 = CENTER_X - 13.0;
pub const CASTLE_MAX_X: f64 = CENTER_X + 13.0;
pub const CASTLE_MIN_Z: f64 = CENTER_Z - 9.0;
pub const CASTLE_MAX_Z: f64 = CENTER_Z + 9.0;

const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;

/// Snap any angle to the nearest cardinal (0, 90, 180, 270°), normalised to
/// [0, 2π).
pub fn snap_to_cardinal(a: f64) -> f64 {
    let snapped = (a / HALF_PI).round() * HALF_PI;
    let two_pi = std::f64::consts::PI * 2.0;
    ((snapped % two_pi) + two_pi) % two_pi
}

/// True if a grid tile lies within the castle wall perimeter.
pub fn is_inside_castle(x: f64, z: f64) -> bool {
    (CASTLE_MIN_X..=CASTLE_MAX_X).contains(&x) && (CASTLE_MIN_Z..=CASTLE_MAX_Z).contains(&z)
}
