//! Port of src/world/oreStore.ts — static, destructible stone nodes (ore
//! boulders) the player mines by hitting. No AI, no movement: HP + a hurt flash.
//!
//! The TS module holds `ore: OreState[]` + `nextId` as module-globals with free
//! functions over them; here they live in an `OreField` struct so tests use a
//! fresh instance (parallel-safe). `damageOre` mutated an OreState directly in TS
//! → it's a method on `Ore`. There is no subscribe/notify on this store. `f64`
//! for JS-`number` parity; `create` snaps y to the real tile top via `tilemap`.

use crate::tilemap::{tile_at, tile_top_y};

/// Ore is the one renewable exception: it does not respawn. Each node is a long
/// dig (high HP) so building stone is a real time sink.
pub const ORE_HP: f64 = 500.0;
pub const ORE_STONE: f64 = 8.0;

pub const ORE_COLLISION_RADIUS: f64 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ore {
    pub id: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub hp: f64,
    pub max_hp: f64,
    pub hurt_flash_until: f64,
    pub seed: f64,
    pub collision_radius: f64,
    /// 0..3 visual variant (vein colour / shape)
    pub variant: i32,
    /// stone granted to the player when this node shatters
    pub stone_reward: f64,
}

impl Ore {
    /// Apply `amount` damage at time `now` (seconds). Returns true if the node
    /// shatters on this hit. A hit on an already-dead node is a no-op → false.
    pub fn damage(&mut self, amount: f64, now: f64) -> bool {
        if self.hp <= 0.0 {
            return false;
        }
        self.hp = (self.hp - amount).max(0.0);
        self.hurt_flash_until = now + 0.18;
        self.hp <= 0.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct OreField {
    ore: Vec<Ore>,
    next_id: i64,
}

impl OreField {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a full-HP node at (x,z); y snaps to the tile top (1 over void).
    pub fn create(&mut self, x: f64, z: f64, seed: f64) -> Ore {
        let fx = x.floor() as i32;
        let fz = z.floor() as i32;
        let y = if tile_at(fx, fz).is_some() {
            tile_top_y(fx, fz)
        } else {
            1.0
        };
        let o = Ore {
            id: self.next_id,
            x,
            y,
            z,
            hp: ORE_HP,
            max_hp: ORE_HP,
            hurt_flash_until: 0.0,
            seed,
            collision_radius: ORE_COLLISION_RADIUS,
            variant: ((seed * 4.0).floor() as i32).rem_euclid(4),
            stone_reward: ORE_STONE,
        };
        self.next_id += 1;
        self.ore.push(o);
        o
    }

    pub fn reset(&mut self) {
        self.ore.clear();
        self.next_id = 0;
    }

    /// Every node, alive or shattered.
    pub fn all(&self) -> &[Ore] {
        &self.ore
    }

    /// Only the still-standing nodes (hp > 0).
    pub fn alive(&self) -> Vec<Ore> {
        self.ore.iter().copied().filter(|o| o.hp > 0.0).collect()
    }

    /// Mutable handle to the node with `id`, if any (so callers can `damage` it).
    pub fn get_mut(&mut self, id: i64) -> Option<&mut Ore> {
        self.ore.iter_mut().find(|o| o.id == id)
    }

    /// Player-vs-ore blocking check: true if a live node overlaps the query disc.
    pub fn collides_at(&self, x: f64, z: f64, r: f64) -> bool {
        for o in &self.ore {
            if o.hp <= 0.0 {
                continue;
            }
            let dx = x - o.x;
            let dz = z - o.z;
            let rsum = r + o.collision_radius;
            if dx * dx + dz * dz < rsum * rsum {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    // Port of src/world/oreStore.test.ts. `damageOre(o, ...)` mutated the node
    // directly in TS; here it's `Ore::damage` operating on the returned copy. The
    // field's collision/alive queries are re-checked by mutating the node inside
    // the field via `get_mut`.
    use super::*;

    #[test]
    fn create_registers_a_full_hp_node() {
        let mut f = OreField::new();
        let o = f.create(110.5, 66.5, 0.5);
        assert_eq!(f.all().len(), 1);
        assert_eq!(f.alive().len(), 1);
        assert_eq!(o.hp, o.max_hp);
        assert!(o.stone_reward > 0.0);
    }

    #[test]
    fn damage_returns_false_until_shatter_true_on_lethal_hit() {
        let mut f = OreField::new();
        let o = f.create(110.5, 66.5, 0.5);
        let mut node = o;
        let half = (node.max_hp / 2.0).ceil();
        assert!(!node.damage(half, 1.0)); // still standing
        assert!(node.hurt_flash_until > 1.0); // flash stamped
        assert!(node.damage(node.max_hp, 2.0)); // shattered
        assert_eq!(node.hp, 0.0);
    }

    #[test]
    fn shattered_node_drops_from_alive_and_stops_colliding() {
        let mut f = OreField::new();
        let id = f.create(110.5, 66.5, 0.5).id;
        assert!(f.collides_at(110.5, 66.5, 0.2));
        f.get_mut(id).unwrap().damage(ORE_HP, 1.0);
        assert_eq!(f.alive().len(), 0);
        assert!(!f.collides_at(110.5, 66.5, 0.2));
    }

    #[test]
    fn collides_at_only_blocks_within_radii() {
        let mut f = OreField::new();
        f.create(110.5, 66.5, 0.5);
        assert!(f.collides_at(110.5, 66.5, 0.2)); // on top
        assert!(!f.collides_at(115.0, 66.5, 0.2)); // far away
    }

    #[test]
    fn damage_on_dead_node_is_noop_returning_false() {
        let mut f = OreField::new();
        let mut node = f.create(110.5, 66.5, 0.5);
        node.damage(node.max_hp, 1.0);
        assert!(!node.damage(999.0, 2.0));
    }

    #[test]
    fn reset_clears_the_field() {
        let mut f = OreField::new();
        f.create(110.5, 66.5, 0.5);
        f.reset();
        assert_eq!(f.all().len(), 0);
    }
}
