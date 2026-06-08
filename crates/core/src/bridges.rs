//! Port of src/world/bridges.ts — a mutable registry of bridge spans.
//!
//! The TS module keeps a module-global mutable `Vec`; the Rust mirror uses a
//! process-global `Mutex<Vec<BridgeSpan>>` (NOT OnceLock — the list mutates via
//! register/reset). `bridge_at` is the rectangle-along-axis test consulted by
//! `tilemap::height_class_at` so bridge decks count as standable height-1.

use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BridgeSpan {
    pub from_x: f64,
    pub from_z: f64,
    pub to_x: f64,
    pub to_z: f64,
    pub width: f64,
    pub y: f64,
}

static BRIDGES: Mutex<Vec<BridgeSpan>> = Mutex::new(Vec::new());

/// Register a span, deduping by endpoints (a re-register with the same
/// endpoints replaces the existing entry, in case width/y changed).
pub fn register_bridge(span: BridgeSpan) {
    let mut bridges = BRIDGES.lock().unwrap();
    for b in bridges.iter_mut() {
        if b.from_x == span.from_x
            && b.from_z == span.from_z
            && b.to_x == span.to_x
            && b.to_z == span.to_z
        {
            *b = span;
            return;
        }
    }
    bridges.push(span);
}

pub fn reset_bridges() {
    BRIDGES.lock().unwrap().clear();
}

pub fn get_bridges() -> Vec<BridgeSpan> {
    BRIDGES.lock().unwrap().clone()
}

/// Returns the bridge span the (x, z) point lies on, or None. A bridge is a
/// rectangle aligned with its from→to axis: along ∈ [-0.4, len+0.4] (the 0.4
/// overhang lets the approach edges count) and |perp| ≤ width/2.
pub fn bridge_at(x: f64, z: f64) -> Option<BridgeSpan> {
    let bridges = BRIDGES.lock().unwrap();
    for b in bridges.iter() {
        let dx = b.to_x - b.from_x;
        let dz = b.to_z - b.from_z;
        let len = dx.hypot(dz);
        if len < 0.001 {
            continue;
        }
        let ux = dx / len;
        let uz = dz / len;
        let px = x - b.from_x;
        let pz = z - b.from_z;
        let along = px * ux + pz * uz;
        if along < -0.4 || along > len + 0.4 {
            continue;
        }
        let perp = px * -uz + pz * ux;
        if perp.abs() > b.width / 2.0 {
            continue;
        }
        return Some(*b);
    }
    None
}
