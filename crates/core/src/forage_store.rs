//! Port of src/world/forageStore.ts — the shared forage-field factory. Marsh
//! herbs (herbStore.ts) and forest apples (appleStore.ts) are both thin
//! instances of this ONE store: `herb_store()` / `apple_store()` are just
//! `ForageStore::new(90)` constructors, so only the factory is ported (matching
//! the TS, where the herb/apple modules carry no logic of their own).
//!
//! No subscribe/notify on this store (the TS one has none either — the view reads
//! the field off React state). `now` is already an explicit elapsed-seconds arg
//! in the TS API, so it carries over directly. `create` snaps y to the real tile
//! top via `tilemap` (1 over a null tile).

use crate::tilemap::{tile_at, tile_top_y};

/// Default respawn delay (seconds) — what herb/apple fields are built with.
pub const DEFAULT_RESPAWN: f64 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Forage {
    pub id: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub seed: f64,
    pub collected: bool,
    /// elapsed-seconds timestamp of the last collect; drives respawn. 0 = never.
    pub collected_at: f64,
}

#[derive(Debug, Clone)]
pub struct ForageStore {
    items: Vec<Forage>,
    next_id: i64,
    /// the respawn delay this field was built with (seconds)
    respawn: f64,
}

impl ForageStore {
    /// Build an independent forage field with the given respawn delay (seconds).
    pub fn new(respawn: f64) -> Self {
        Self { items: Vec::new(), next_id: 0, respawn }
    }

    pub fn respawn(&self) -> f64 {
        self.respawn
    }

    /// Register a plant at (x,z); y snaps to the tile top (1 over null tile).
    pub fn create(&mut self, x: f64, z: f64, seed: f64) -> Forage {
        let fx = x.floor() as i32;
        let fz = z.floor() as i32;
        let y = if tile_at(fx, fz).is_some() {
            tile_top_y(fx, fz)
        } else {
            1.0
        };
        let item = Forage {
            id: self.next_id,
            x,
            y,
            z,
            seed,
            collected: false,
            collected_at: 0.0,
        };
        self.next_id += 1;
        self.items.push(item);
        item
    }

    /// Clear the field + id counter (new game / unmount).
    pub fn reset(&mut self) {
        self.items.clear();
        self.next_id = 0;
    }

    /// Every plant, collected or not.
    pub fn all(&self) -> &[Forage] {
        &self.items
    }

    /// Only the still-gatherable plants.
    pub fn active(&self) -> Vec<Forage> {
        self.items.iter().copied().filter(|i| !i.collected).collect()
    }

    /// Mark the plant with `id` foraged at time `now` (elapsed seconds). Returns
    /// false if it was already taken (or unknown).
    pub fn collect(&mut self, id: i64, now: f64) -> bool {
        match self.items.iter_mut().find(|i| i.id == id) {
            Some(item) if !item.collected => {
                item.collected = true;
                item.collected_at = now;
                true
            }
            _ => false,
        }
    }

    /// Revive any collected plant whose respawn delay has elapsed by `now`
    /// (elapsed seconds). Returns true if at least one regrew.
    pub fn tick(&mut self, now: f64) -> bool {
        let mut revived = false;
        for i in &mut self.items {
            if i.collected && now - i.collected_at >= self.respawn {
                i.collected = false;
                revived = true;
            }
        }
        revived
    }

    /// Look up a plant by id (e.g. to read its `collected_at`).
    pub fn get(&self, id: i64) -> Option<&Forage> {
        self.items.iter().find(|i| i.id == id)
    }
}

/// Marsh-herb field (herbStore.ts) — a forage field with the default respawn.
pub fn herb_store() -> ForageStore {
    ForageStore::new(DEFAULT_RESPAWN)
}

/// Forest-apple field (appleStore.ts) — same factory, default respawn.
pub fn apple_store() -> ForageStore {
    ForageStore::new(DEFAULT_RESPAWN)
}

#[cfg(test)]
mod tests {
    // Port of src/world/forageStore.test.ts + herbStore.test.ts. `collect`
    // operates by id here (TS passed the live object); behaviour is identical.
    // appleStore.test.ts does not exist — the apple field is the same factory and
    // is covered by these factory tests, mirroring the TS comment that "neither
    // [herb nor apple] needs its own near-identical test".
    use super::*;

    // ── forageStore factory ───────────────────────────────────────────────────
    #[test]
    fn create_registers_uncollected_plant_in_all_and_active() {
        let mut s = ForageStore::new(DEFAULT_RESPAWN);
        let a = s.create(72.0, 84.0, 0.5);
        assert!(!a.collected);
        assert_eq!(s.all().len(), 1);
        assert_eq!(s.active().len(), 1);
    }

    #[test]
    fn collect_takes_a_plant_once_and_drops_from_active() {
        let mut s = ForageStore::new(DEFAULT_RESPAWN);
        let a = s.create(1.0, 2.0, 0.1);
        assert!(s.collect(a.id, 0.0));
        assert!(s.get(a.id).unwrap().collected);
        assert_eq!(s.active().len(), 0);
        assert_eq!(s.all().len(), 1);
    }

    #[test]
    fn a_plant_cannot_be_foraged_twice() {
        let mut s = ForageStore::new(DEFAULT_RESPAWN);
        let a = s.create(1.0, 2.0, 0.1);
        s.collect(a.id, 0.0);
        assert!(!s.collect(a.id, 0.0));
    }

    #[test]
    fn reset_clears_field_and_restarts_id_counter() {
        let mut s = ForageStore::new(DEFAULT_RESPAWN);
        s.create(1.0, 2.0, 0.1);
        s.reset();
        assert_eq!(s.all().len(), 0);
        assert_eq!(s.create(3.0, 4.0, 0.2).id, 0);
    }

    #[test]
    fn two_instances_keep_independent_state() {
        let mut a = ForageStore::new(DEFAULT_RESPAWN);
        let mut b = ForageStore::new(DEFAULT_RESPAWN);
        let pa = a.create(0.0, 0.0, 0.0);
        a.collect(pa.id, 0.0);
        b.create(0.0, 0.0, 0.0);
        assert_eq!(a.active().len(), 0);
        assert_eq!(b.active().len(), 1);
    }

    #[test]
    fn y_falls_back_to_1_over_a_null_tile() {
        let mut s = ForageStore::new(DEFAULT_RESPAWN);
        assert_eq!(s.create(-50.0, -50.0, 0.0).y, 1.0);
    }

    #[test]
    fn collect_stamps_time_tick_before_delay_keeps_collected() {
        let mut s = ForageStore::new(90.0);
        let a = s.create(1.0, 2.0, 0.1);
        s.collect(a.id, 100.0);
        assert_eq!(s.get(a.id).unwrap().collected_at, 100.0);
        s.tick(189.0); // 89s elapsed, still under 90
        assert!(s.get(a.id).unwrap().collected);
        assert_eq!(s.active().len(), 0);
    }

    #[test]
    fn tick_respawns_a_plant_once_delay_elapsed() {
        let mut s = ForageStore::new(90.0);
        let a = s.create(1.0, 2.0, 0.1);
        s.collect(a.id, 100.0);
        let revived = s.tick(190.0); // exactly 90s later
        assert!(revived);
        assert!(!s.get(a.id).unwrap().collected);
        assert_eq!(s.active().len(), 1);
    }

    #[test]
    fn tick_returns_false_when_nothing_ready() {
        let mut s = ForageStore::new(90.0);
        let a = s.create(1.0, 2.0, 0.1);
        s.collect(a.id, 100.0);
        assert!(!s.tick(150.0));
    }

    #[test]
    fn a_respawned_plant_can_be_foraged_again() {
        let mut s = ForageStore::new(90.0);
        let a = s.create(1.0, 2.0, 0.1);
        s.collect(a.id, 100.0);
        s.tick(200.0);
        assert!(s.collect(a.id, 300.0));
        assert!(s.get(a.id).unwrap().collected);
    }

    #[test]
    fn respawn_delay_is_configurable_per_store() {
        let mut s = ForageStore::new(10.0);
        let a = s.create(1.0, 2.0, 0.1);
        s.collect(a.id, 0.0);
        assert!(!s.tick(9.0));
        assert!(s.tick(10.0));
        assert!(!s.get(a.id).unwrap().collected);
    }

    // ── herbStore instance (herbStore.test.ts) ────────────────────────────────
    #[test]
    fn herb_create_registers_uncollected_plant() {
        let mut h = herb_store();
        let plant = h.create(72.0, 84.0, 0.5);
        assert_eq!(h.all().len(), 1);
        assert_eq!(h.active().len(), 1);
        assert!(!plant.collected);
    }

    #[test]
    fn herb_collect_takes_once_and_drops_from_active() {
        let mut h = herb_store();
        let plant = h.create(72.0, 84.0, 0.5);
        assert!(h.collect(plant.id, 0.0));
        assert!(h.get(plant.id).unwrap().collected);
        assert_eq!(h.active().len(), 0);
        assert_eq!(h.all().len(), 1);
    }

    #[test]
    fn herb_cannot_be_foraged_twice() {
        let mut h = herb_store();
        let plant = h.create(72.0, 84.0, 0.5);
        h.collect(plant.id, 0.0);
        assert!(!h.collect(plant.id, 0.0));
    }

    #[test]
    fn reset_herbs_clears_field() {
        let mut h = herb_store();
        h.create(72.0, 84.0, 0.5);
        h.reset();
        assert_eq!(h.all().len(), 0);
    }
}
