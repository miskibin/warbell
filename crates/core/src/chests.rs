//! Port of the `CHESTS` table + placement/loot rules in `src/world/World.tsx`
//! (lines 78-135 + the `CHESTS.map` block at 508-538) and the cache-respawn constant
//! in `src/world/Chest.tsx` (`CACHE_RESPAWN`).
//!
//! The TS authors 31 chests in BASE map coords (the original 144×108 layout): 16
//! one-shot TREASURE chests (unique hand-placed gear + deep-biome landmark rewards)
//! and 15 respawning CACHES (gold + food, refilled `CACHE_RESPAWN` s after opening). This
//! module holds that table and the resolution rules so the ECS layer just spawns what
//! `resolved_chests()` returns; the deterministic data + placement math are unit-tested
//! here, with no Bevy.
//!
//! Placement (TS `CHESTS.map`):
//!   - A *castle-adjacent* chest (BASE `x∈[55,89]` AND `z∈[41,67]`) TRANSLATES with the
//!     keep (`shift_to_centre`) and keeps its hand-authored loot.
//!   - Every *wilderness* chest SCALES out with its biome (`from_base`).
//!   - Loot: caches, token/teaching chests (any holding `mercenary_contract`), and
//!     castle-adjacent chests keep their hand-authored loot; the remaining one-shot
//!     TREASURE chests roll their gear by frontier distance (`frontier::chest_loot_for`).

use crate::frontier::{ChestLoot, chest_loot_for};
use crate::tilemap::{from_base, shift_to_centre};

/// A cache chest re-closes (becomes lootable again) this many seconds after being
/// opened — ~one in-game day (`Chest.tsx` `CACHE_RESPAWN = 150`). Treasure chests stay
/// opened forever.
pub const CACHE_RESPAWN: f64 = 150.0;

/// One authored chest entry in BASE map coords (the `CHESTS` array in World.tsx).
struct ChestSpec {
    /// BASE map x/z (the TS `pos[0]`/`pos[2]`; `pos[1]` is the on-ground y, unused —
    /// the ECS layer snaps to the terrain top).
    bx: f64,
    bz: f64,
    /// Yaw (the TS `rot`), passed through to the model.
    rot: f64,
    /// Hand-authored gold.
    gold: i64,
    /// Hand-authored loot ids.
    loot: &'static [&'static str],
    /// A respawning cache (vs. a one-shot treasure chest).
    cache: bool,
}

/// The authored chest table — a faithful copy of `World.tsx`'s `CHESTS` (BASE coords).
const CHESTS: &[ChestSpec] = &[
    // ---- Treasure (one-shot): unique gear ----
    ChestSpec { bx: 59.0, bz: 59.0, rot: 0.3, gold: 6, loot: &["sword_iron"], cache: false },
    ChestSpec { bx: 104.0, bz: 80.0, rot: -1.2, gold: 14, loot: &["feast", "mercenary_contract"], cache: false },
    // by the NE trader market — teaches recruiting
    ChestSpec { bx: 96.0, bz: 39.0, rot: 2.6, gold: 10, loot: &["mercenary_contract"], cache: false },
    ChestSpec { bx: 34.0, bz: 30.0, rot: 0.5, gold: 8, loot: &["fur", "leather_armor"], cache: false },
    ChestSpec { bx: 120.0, bz: 58.0, rot: -1.4, gold: 8, loot: &["goat_charm"], cache: false },
    ChestSpec { bx: 24.0, bz: 56.0, rot: 0.9, gold: 10, loot: &["stone_maul", "iron_armor"], cache: false },
    ChestSpec { bx: 18.0, bz: 54.0, rot: 0.6, gold: 30, loot: &["gold_armor"], cache: false },
    // Deep-biome reward chests beside each biome's signature landmark (one-shot).
    ChestSpec { bx: 33.0, bz: 30.0, rot: 0.7, gold: 18, loot: &["feast", "gold_armor"], cache: false }, // snow spire
    ChestSpec { bx: 122.0, bz: 25.0, rot: -0.8, gold: 16, loot: &["venom", "iron_armor"], cache: false }, // desert pyramid
    ChestSpec { bx: 116.0, bz: 82.0, rot: 1.5, gold: 16, loot: &["stone_maul"], cache: false }, // stone circle
    ChestSpec { bx: 73.0, bz: 98.0, rot: 2.1, gold: 14, loot: &["croc_steak", "potion"], cache: false }, // swamp tree
    ChestSpec { bx: 23.0, bz: 86.0, rot: -1.1, gold: 14, loot: &["elk_jerky", "goat_charm"], cache: false }, // forest shrine

    // ---- Caches (respawning): gold + food, rung around the map edge ----
    ChestSpec { bx: 50.0, bz: 22.0, rot: -0.5, gold: 8, loot: &["potion"], cache: true }, // N gap (snow↔desert)
    ChestSpec { bx: 116.0, bz: 24.0, rot: -0.6, gold: 8, loot: &["venom"], cache: true }, // NE desert rim
    ChestSpec { bx: 122.0, bz: 66.0, rot: 0.4, gold: 12, loot: &["potion"], cache: true }, // E rock rim
    ChestSpec { bx: 84.0, bz: 92.0, rot: 1.3, gold: 8, loot: &["croc_steak"], cache: true }, // SE swamp rim
    ChestSpec { bx: 60.0, bz: 94.0, rot: -0.9, gold: 12, loot: &["feast"], cache: true }, // S swamp rim
    ChestSpec { bx: 30.0, bz: 86.0, rot: 1.6, gold: 9, loot: &["bread"], cache: true }, // SW forest rim
    ChestSpec { bx: 20.0, bz: 62.0, rot: 2.0, gold: 8, loot: &["elk_jerky"], cache: true }, // W forest/coast rim

    // ---- Extra density: more grass-belt + biome-approach caches + deep treasure ----
    ChestSpec { bx: 88.0, bz: 46.0, rot: 0.2, gold: 10, loot: &["bread"], cache: true }, // E grass belt
    ChestSpec { bx: 56.0, bz: 64.0, rot: -0.7, gold: 10, loot: &["potion"], cache: true }, // SW grass belt
    ChestSpec { bx: 92.0, bz: 58.0, rot: 1.1, gold: 9, loot: &["bread"], cache: true }, // E grass belt
    ChestSpec { bx: 60.0, bz: 46.0, rot: 2.4, gold: 9, loot: &["potion"], cache: true }, // W grass belt
    ChestSpec { bx: 36.0, bz: 40.0, rot: 0.5, gold: 11, loot: &["feast"], cache: true }, // snow approach
    ChestSpec { bx: 106.0, bz: 42.0, rot: -1.1, gold: 10, loot: &["venom"], cache: true }, // desert approach
    ChestSpec { bx: 108.0, bz: 64.0, rot: 0.9, gold: 12, loot: &["potion"], cache: true }, // rock approach
    ChestSpec { bx: 42.0, bz: 84.0, rot: -0.4, gold: 9, loot: &["croc_steak"], cache: true }, // forest/swamp rim
    // Deep-biome treasure (gear rolled by frontier distance — chest_loot_for).
    ChestSpec { bx: 96.0, bz: 70.0, rot: 1.4, gold: 14, loot: &["stone_maul"], cache: false }, // deep rock
    ChestSpec { bx: 28.0, bz: 74.0, rot: -1.3, gold: 12, loot: &["fur"], cache: false }, // deep forest
    ChestSpec { bx: 116.0, bz: 32.0, rot: 0.7, gold: 14, loot: &["venom"], cache: false }, // deep desert
    ChestSpec { bx: 78.0, bz: 96.0, rot: 2.0, gold: 12, loot: &["croc_steak"], cache: false }, // deep swamp
];

/// A chest resolved onto the live (enlarged) map: re-centred position, final loot +
/// gold (hand-authored or frontier-rolled), the model yaw, and whether it's a cache.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedChest {
    pub x: f64,
    pub z: f64,
    pub rot: f64,
    pub gold: i64,
    pub loot: Vec<&'static str>,
    pub cache: bool,
}

/// True for a castle-adjacent chest (BASE `x∈[55,89]` AND `z∈[41,67]`) — the TS
/// `castleish` test. These translate with the keep and keep their hand-authored loot.
fn is_castleish(bx: f64, bz: f64) -> bool {
    (55.0..=89.0).contains(&bx) && (41.0..=67.0).contains(&bz)
}

/// Resolve the whole `CHESTS` table onto the live map (positions re-centred + loot
/// chosen), mirroring the TS `CHESTS.map` block. Caches, `mercenary_contract`-teaching
/// chests, and castle-adjacent chests keep their authored loot; the remaining one-shot
/// treasure chests roll gear by frontier distance.
pub fn resolved_chests() -> Vec<ResolvedChest> {
    CHESTS
        .iter()
        .map(|c| {
            let castleish = is_castleish(c.bx, c.bz);
            let (x, z) = if castleish {
                shift_to_centre(c.bx, c.bz)
            } else {
                from_base(c.bx, c.bz)
            };
            // Token/teaching chests (any holding a mercenary contract) keep authored loot.
            let is_token = c.loot.contains(&"mercenary_contract");
            let (loot, gold): (Vec<&'static str>, i64) =
                if c.cache || is_token || castleish {
                    (c.loot.to_vec(), c.gold)
                } else {
                    let ChestLoot { loot, gold } = chest_loot_for(x, z);
                    (loot, gold)
                };
            ResolvedChest { x, z, rot: c.rot, gold, loot, cache: c.cache }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_respawn_matches_ts() {
        // Chest.tsx `CACHE_RESPAWN = 150`.
        assert_eq!(CACHE_RESPAWN, 150.0);
    }

    #[test]
    fn table_has_the_full_authored_set() {
        // World.tsx CHESTS: 16 one-shot treasure + 15 respawning caches = 31 total.
        let all = resolved_chests();
        assert_eq!(all.len(), 31, "the full CHESTS table is ported");
        let caches = all.iter().filter(|c| c.cache).count();
        let treasure = all.iter().filter(|c| !c.cache).count();
        assert_eq!(caches, 15, "fifteen respawning caches");
        assert_eq!(treasure, 16, "sixteen one-shot treasure chests");
    }

    #[test]
    fn unique_gear_chests_are_placed_with_authored_loot() {
        let all = resolved_chests();
        // The starter Iron Sword (castle-adjacent: kept authored).
        assert!(
            all.iter().any(|c| c.loot == vec!["sword_iron"] && !c.cache),
            "the starter iron-sword chest is present"
        );
        // The Gilded Plate treasure (far W rim — authored, one-shot).
        assert!(
            all.iter().any(|c| c.loot.contains(&"gold_armor")),
            "a gold-armor treasure chest is present"
        );
        // The recruiting-teach chest by the NE market (mercenary_contract token).
        assert!(
            all.iter().any(|c| c.loot == vec!["mercenary_contract"]),
            "the recruit-teaching contract chest is present"
        );
    }

    #[test]
    fn caches_keep_authored_food_or_gold_loot() {
        let all = resolved_chests();
        // Every cache keeps its hand-authored loot + a small gold gift (the daily
        // food/gold trickle); none gets frontier-rolled gear.
        for c in all.iter().filter(|c| c.cache) {
            assert!(c.gold > 0, "a cache hands out gold");
            assert!(!c.loot.is_empty(), "a cache hands out a consumable");
        }
    }

    #[test]
    fn castle_adjacent_chest_translates_with_the_keep() {
        // The starter chest (BASE 59,59) is castle-adjacent → shift_to_centre, NOT
        // scaled out. shift_to_centre(59,59) translates by (CENTER−BASE_CENTER).
        let (sx, sz) = shift_to_centre(59.0, 59.0);
        let starter = resolved_chests()
            .into_iter()
            .find(|c| c.loot == vec!["sword_iron"])
            .expect("starter chest");
        assert!((starter.x - sx).abs() < 1e-9 && (starter.z - sz).abs() < 1e-9);
    }

    #[test]
    fn resolution_is_deterministic() {
        assert_eq!(resolved_chests(), resolved_chests());
    }
}
