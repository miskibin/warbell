//! Port of src/world/frontier.ts — distance-from-castle gradient that grades the
//! map (loot tier, drop tier, day-threat toughness). 0 across the safe core,
//! smoothly -> 1 at the island rim.

use crate::tilemap::{CASTLE_CENTER_X, CASTLE_CENTER_Z, CASTLE_SAFE_R, ROWS};

/// Distance (tiles) from the castle at which the factor reaches 1.
/// 0.68 * ROWS ~= 103 on the 152-row map.
pub const RIM_DIST: f64 = ROWS as f64 * 0.68;

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// 0 inside the safe zone, smoothly -> 1 at RIM_DIST and beyond.
pub fn frontier_factor(x: f64, z: f64) -> f64 {
    let d = (x - CASTLE_CENTER_X).hypot(z - CASTLE_CENTER_Z);
    let t = ((d - CASTLE_SAFE_R) / (RIM_DIST - CASTLE_SAFE_R)).clamp(0.0, 1.0);
    smoothstep(t)
}

/// Loot quality band: 0 near, 1 mid, 2 rim (best).
pub fn gear_tier(factor: f64) -> u8 {
    if factor > 0.7 {
        2
    } else if factor > 0.4 {
        1
    } else {
        0
    }
}

/// Tiered loot pools indexed by gear_tier(). Top tier is the ONLY source of best gear.
fn gear_pool(tier: u8) -> &'static [&'static str] {
    match tier {
        0 => &["sword_iron", "leather_armor", "bread"],
        1 => &["axe", "stone_maul", "iron_armor", "potion"],
        _ => &["blade_frost", "dragon_plate", "sword_gold", "gold_armor"],
    }
}

/// Pick a loot id for a point's frontier `factor`. `roll` in [0,1).
pub fn roll_gear(factor: f64, roll: f64) -> &'static str {
    let pool = gear_pool(gear_tier(factor));
    let idx = ((roll * pool.len() as f64).floor() as usize).min(pool.len() - 1);
    pool[idx]
}

/// Deterministic [0,1) hash of a tile — stable loot per chest across reloads.
fn tile_hash(x: f64, z: f64) -> f64 {
    let s = (x * 127.1 + z * 311.7).sin() * 43758.5453;
    s - s.floor()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChestLoot {
    pub loot: Vec<&'static str>,
    pub gold: i64,
}

/// Loot ids + gold for a chest at (x,z): count + quality climb with distance.
pub fn chest_loot_for(x: f64, z: f64) -> ChestLoot {
    let f = frontier_factor(x, z);
    let h = tile_hash(x, z);
    let items = 1 + f.round() as i64; // 1 near, 2 at rim
    let mut loot = Vec::new();
    for i in 0..items {
        loot.push(roll_gear(f, (h + i as f64 * 0.37) % 1.0));
    }
    let gold = (15.0 + f * 60.0 + h * 20.0).round() as i64; // ~15-95 by distance
    ChestLoot { loot, gold }
}

#[cfg(test)]
mod tests {
    // Port of src/world/frontier.test.ts.
    use super::*;
    use crate::tilemap::{CASTLE_CENTER_X as CX, CASTLE_CENTER_Z as CZ, CASTLE_SAFE_R as SAFE};

    fn close(a: f64, b: f64, places: i32) -> bool {
        (a - b).abs() < 0.5 * 10f64.powi(-places)
    }

    #[test]
    fn factor_is_zero_at_centre_and_across_safe_zone() {
        assert_eq!(frontier_factor(CX, CZ), 0.0);
        assert_eq!(frontier_factor(CX + SAFE - 1.0, CZ), 0.0);
    }

    #[test]
    fn factor_is_one_at_or_beyond_rim() {
        assert!(close(frontier_factor(CX + RIM_DIST, CZ), 1.0, 5));
        assert_eq!(frontier_factor(CX + RIM_DIST + 50.0, CZ), 1.0);
    }

    #[test]
    fn factor_increases_monotonically_through_ramp() {
        let a = frontier_factor(CX + SAFE + 5.0, CZ);
        let b = frontier_factor(CX + SAFE + 15.0, CZ);
        let c = frontier_factor(CX + SAFE + 25.0, CZ);
        assert!(a > 0.0);
        assert!(b > a);
        assert!(c > b);
    }

    #[test]
    fn gear_tier_bands_factor() {
        assert_eq!(gear_tier(0.0), 0);
        assert_eq!(gear_tier(0.39), 0);
        assert_eq!(gear_tier(0.5), 1);
        assert_eq!(gear_tier(0.71), 2);
        assert_eq!(gear_tier(1.0), 2);
    }

    #[test]
    fn roll_gear_low_tier_near_castle() {
        let id = roll_gear(0.0, 0.5);
        assert!(["sword_iron", "leather_armor", "bread"].contains(&id));
    }

    #[test]
    fn roll_gear_top_tier_at_rim() {
        let id = roll_gear(1.0, 0.5);
        assert!(["blade_frost", "dragon_plate", "sword_gold", "gold_armor"].contains(&id));
    }

    #[test]
    fn roll_gear_is_deterministic() {
        assert_eq!(roll_gear(1.0, 0.42), roll_gear(1.0, 0.42));
    }

    #[test]
    fn chest_loot_fewer_lower_near_than_rim() {
        let near = chest_loot_for(CX + SAFE + 2.0, CZ);
        let rim = chest_loot_for(CX + RIM_DIST, CZ);
        assert!(rim.loot.len() >= near.loot.len());
        assert!(rim.gold > near.gold);
    }

    #[test]
    fn chest_loot_is_deterministic_per_tile() {
        assert_eq!(chest_loot_for(80.0, 40.0), chest_loot_for(80.0, 40.0));
    }
}
