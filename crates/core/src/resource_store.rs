//! Port of src/world/resourceStore.ts — the crafting-resource bank (currently
//! just `stone`, mined from ore boulders and spent on defense upgrades).
//!
//! The TS module is a global store with subscribe/notify; that listener fan-out
//! is HUD-only and becomes Bevy change-detection later, so it is SKIPPED here.
//! Ported as a plain `ResourceState` struct + mutators so tests construct a fresh
//! instance (no global → parallel-safe). `f64` for JS-`number` parity.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceState {
    pub stone: f64,
    pub food: f64,
    pub wood: f64,
}

impl Default for ResourceState {
    fn default() -> Self {
        Self { stone: 0.0, food: 0.0, wood: 0.0 }
    }
}

impl ResourceState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stone(&self) -> f64 {
        self.stone
    }

    pub fn food(&self) -> f64 {
        self.food
    }

    pub fn wood(&self) -> f64 {
        self.wood
    }

    /// Add stone; non-positive amounts are ignored (mirrors `addStone`).
    pub fn add_stone(&mut self, n: f64) {
        if n <= 0.0 {
            return;
        }
        self.stone += n;
    }

    /// Add food; non-positive ignored (mirrors `add_stone`).
    pub fn add_food(&mut self, n: f64) {
        if n <= 0.0 {
            return;
        }
        self.food += n;
    }

    /// Add wood; non-positive ignored (mirrors `add_stone`).
    pub fn add_wood(&mut self, n: f64) {
        if n <= 0.0 {
            return;
        }
        self.wood += n;
    }

    /// Spend stone if affordable. All-or-nothing: returns false and changes
    /// nothing when short. `n <= 0` is a no-op that returns true (mirrors the TS).
    pub fn spend_stone(&mut self, n: f64) -> bool {
        if n <= 0.0 {
            return true;
        }
        if self.stone < n {
            return false;
        }
        self.stone -= n;
        true
    }

    /// Spend food if affordable; all-or-nothing (mirrors `spend_stone`).
    pub fn spend_food(&mut self, n: f64) -> bool {
        if n <= 0.0 {
            return true;
        }
        if self.food < n {
            return false;
        }
        self.food -= n;
        true
    }

    /// Spend wood if affordable; all-or-nothing (mirrors `spend_stone`).
    pub fn spend_wood(&mut self, n: f64) -> bool {
        if n <= 0.0 {
            return true;
        }
        if self.wood < n {
            return false;
        }
        self.wood -= n;
        true
    }

    /// Zero the bank (new game).
    pub fn reset(&mut self) {
        self.stone = 0.0;
        self.food = 0.0;
        self.wood = 0.0;
    }
}

#[cfg(test)]
mod tests {
    // Port of src/world/resourceStore.test.ts (the subscribe/notify test is
    // dropped with the listener machinery; the mutator behaviour is the parity
    // surface).
    use super::*;

    #[test]
    fn starts_empty() {
        assert_eq!(ResourceState::new().stone(), 0.0);
    }

    #[test]
    fn add_stone_accumulates_non_positive_ignored() {
        let mut r = ResourceState::new();
        r.add_stone(4.0);
        r.add_stone(6.0);
        assert_eq!(r.stone(), 10.0);
        r.add_stone(0.0);
        r.add_stone(-5.0);
        assert_eq!(r.stone(), 10.0);
    }

    #[test]
    fn spend_stone_deducts_when_affordable_and_returns_true() {
        let mut r = ResourceState::new();
        r.add_stone(30.0);
        assert!(r.spend_stone(20.0));
        assert_eq!(r.stone(), 10.0);
    }

    #[test]
    fn spend_stone_is_all_or_nothing() {
        let mut r = ResourceState::new();
        r.add_stone(5.0);
        assert!(!r.spend_stone(20.0));
        assert_eq!(r.stone(), 5.0);
    }

    #[test]
    fn reset_zeroes_the_bank() {
        let mut r = ResourceState::new();
        r.add_stone(99.0);
        r.reset();
        assert_eq!(r.stone(), 0.0);
    }

    #[test]
    fn food_and_wood_start_empty() {
        let r = ResourceState::new();
        assert_eq!(r.food(), 0.0);
        assert_eq!(r.wood(), 0.0);
    }

    #[test]
    fn add_food_wood_accumulate_non_positive_ignored() {
        let mut r = ResourceState::new();
        r.add_food(4.0);
        r.add_food(6.0);
        r.add_wood(3.0);
        assert_eq!(r.food(), 10.0);
        assert_eq!(r.wood(), 3.0);
        r.add_food(-5.0);
        r.add_wood(0.0);
        assert_eq!(r.food(), 10.0);
        assert_eq!(r.wood(), 3.0);
    }

    #[test]
    fn spend_food_wood_all_or_nothing() {
        let mut r = ResourceState::new();
        r.add_food(5.0);
        r.add_wood(5.0);
        assert!(!r.spend_food(20.0));
        assert!(r.spend_wood(5.0));
        assert_eq!(r.food(), 5.0);
        assert_eq!(r.wood(), 0.0);
    }

    #[test]
    fn reset_zeroes_all_resources() {
        let mut r = ResourceState::new();
        r.add_stone(9.0);
        r.add_food(9.0);
        r.add_wood(9.0);
        r.reset();
        assert_eq!(r.food(), 0.0);
        assert_eq!(r.wood(), 0.0);
        assert_eq!(r.stone(), 0.0);
    }
}
