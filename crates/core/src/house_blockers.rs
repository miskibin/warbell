//! Port of src/world/houseBlockers.ts — a mutable registry of axis-aligned
//! rectangular footprints (houses / walls / towers) that pathfinding and
//! movement treat as solid.
//!
//! Process-global `Mutex<Vec<HouseBlocker>>` mirroring the TS module-global Vec.
//! In a headless run nothing registers a blocker, so `house_blocks_at` returns
//! false — matching the TS test setup (the obstacles port's `find_spawn_near`
//! footprint check is consequently a no-op headlessly, as in TS).

use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub struct HouseBlocker {
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
    /// which component registered this (so resets are scoped, not global)
    pub owner: String,
}

static BLOCKERS: Mutex<Vec<HouseBlocker>> = Mutex::new(Vec::new());

/// Register a footprint owned by `owner`. Deduped by (bounds, owner).
pub fn register_house_blocker(min_x: f64, min_z: f64, max_x: f64, max_z: f64, owner: &str) {
    let mut blockers = BLOCKERS.lock().unwrap();
    for e in blockers.iter() {
        if e.min_x == min_x
            && e.min_z == min_z
            && e.max_x == max_x
            && e.max_z == max_z
            && e.owner == owner
        {
            return;
        }
    }
    blockers.push(HouseBlocker {
        min_x,
        min_z,
        max_x,
        max_z,
        owner: owner.to_string(),
    });
}

/// Clear blockers. `None` clears everything; `Some(owner)` clears only that
/// owner's entries — so two independent components don't wipe each other's
/// footprints on unmount.
pub fn reset_house_blockers(owner: Option<&str>) {
    let mut blockers = BLOCKERS.lock().unwrap();
    match owner {
        None => blockers.clear(),
        Some(o) => blockers.retain(|b| b.owner != o),
    }
}

pub fn house_blocks_at(x: f64, z: f64) -> bool {
    let blockers = BLOCKERS.lock().unwrap();
    blockers
        .iter()
        .any(|b| x >= b.min_x && x <= b.max_x && z >= b.min_z && z <= b.max_z)
}

/// True if a registered blocker sits between the two points. Samples the segment
/// interior (endpoints skipped, so an attacker/target flush against a structure
/// doesn't self-block).
pub fn wall_between(ax: f64, az: f64, bx: f64, bz: f64) -> bool {
    let dx = bx - ax;
    let dz = bz - az;
    let len = dx.hypot(dz);
    if len < 0.001 {
        return false;
    }
    let steps = (2.0_f64).max((len / 0.25).ceil()) as i64;
    for i in 1..steps {
        let t = i as f64 / steps as f64;
        if house_blocks_at(ax + dx * t, az + dz * t) {
            return true;
        }
    }
    false
}

pub fn get_house_blockers() -> Vec<HouseBlocker> {
    BLOCKERS.lock().unwrap().clone()
}
