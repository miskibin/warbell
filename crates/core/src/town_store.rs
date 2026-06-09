//! The town/settlement model — pure, tested, zero-dep. Drives the Bevy `town.rs`
//! city-building layer. Buildings sit on pre-placed plots; producers (staffed by
//! villagers) tick a resource into the bank; food grows population; orks burn
//! buildings (HP -> rubble) and survivors repair by day.
//!
//! Numbers are tuned for forest's 60-HP combat units and the day/night siege
//! cadence; tweak the constants below (also exposed to the F1 debug panel).

use crate::resource_store::ResourceState;

/// Villagers a single House adds to the population cap.
pub const POP_PER_HOUSE: u32 = 2;
/// Food spent to grow the population by one villager.
pub const FOOD_PER_VILLAGER: f64 = 20.0;
/// Food drained per villager per second (upkeep).
pub const UPKEEP_PER_POP: f64 = 0.04;
/// Building HP healed per second while repairing (Prep phase).
pub const REPAIR_PER_SEC: f64 = 8.0;

/// What you can build on a plot. (Pass 2 adds Lumber/Quarry/Market/Granary.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildKind {
    Farm,
    House,
}

/// A resource a producer yields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Food,
    Wood,
    Stone,
}

/// Build cost (gold is not spent on buildings in the slice).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cost {
    pub wood: f64,
    pub stone: f64,
}

impl BuildKind {
    pub fn cost(self) -> Cost {
        match self {
            BuildKind::Farm => Cost { wood: 8.0, stone: 0.0 },
            BuildKind::House => Cost { wood: 6.0, stone: 4.0 },
        }
    }

    /// `(resource, units per second)` when staffed, or `None` for non-producers.
    pub fn produces(self) -> Option<(Resource, f64)> {
        match self {
            BuildKind::Farm => Some((Resource::Food, 0.5)),
            BuildKind::House => None,
        }
    }

    pub fn max_hp(self) -> f64 {
        match self {
            BuildKind::Farm => 60.0,
            BuildKind::House => 50.0,
        }
    }

    pub fn needs_worker(self) -> bool {
        self.produces().is_some()
    }

    pub fn label(self) -> &'static str {
        match self {
            BuildKind::Farm => "Farm",
            BuildKind::House => "House",
        }
    }
}

/// A plot's lifecycle. `hp`/`burning` live inside `Built`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlotState {
    Empty,
    Built { hp: f64, burning: bool },
    Rubble,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plot {
    pub kind: Option<BuildKind>,
    pub state: PlotState,
    /// Synced from the Bevy layer each frame: is a live worker posted here?
    pub staffed: bool,
}

impl Plot {
    fn empty() -> Self {
        Self { kind: None, state: PlotState::Empty, staffed: false }
    }

    pub fn is_buildable(&self) -> bool {
        matches!(self.state, PlotState::Empty | PlotState::Rubble)
    }

    pub fn is_built(&self) -> bool {
        matches!(self.state, PlotState::Built { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Town {
    pub plots: Vec<Plot>,
    pub population: u32,
}

impl Town {
    /// A fresh town with `n` empty plots and a starting population.
    pub fn new(n: usize, start_population: u32) -> Self {
        Self { plots: vec![Plot::empty(); n], population: start_population }
    }

    /// Re-init to empty plots + the given starting population (new run).
    pub fn reset(&mut self, start_population: u32) {
        for p in &mut self.plots {
            *p = Plot::empty();
        }
        self.population = start_population;
    }

    pub fn houses_built(&self) -> u32 {
        self.plots
            .iter()
            .filter(|p| p.is_built() && p.kind == Some(BuildKind::House))
            .count() as u32
    }

    pub fn pop_cap(&self) -> u32 {
        self.houses_built() * POP_PER_HOUSE
    }

    pub fn can_afford(&self, kind: BuildKind, bank: &ResourceState) -> bool {
        let c = kind.cost();
        bank.wood() >= c.wood && bank.stone() >= c.stone
    }

    /// Build `kind` on a buildable plot, spending wood+stone atomically.
    /// Returns true on success.
    pub fn build(&mut self, idx: usize, kind: BuildKind, bank: &mut ResourceState) -> bool {
        let Some(plot) = self.plots.get(idx) else { return false };
        if !plot.is_buildable() || !self.can_afford(kind, bank) {
            return false;
        }
        let c = kind.cost();
        // Affordability already checked, so both spends succeed.
        bank.spend_wood(c.wood);
        bank.spend_stone(c.stone);
        self.plots[idx] = Plot {
            kind: Some(kind),
            state: PlotState::Built { hp: kind.max_hp(), burning: false },
            staffed: false,
        };
        true
    }

    /// Each staffed, non-burning producer adds its yield to the bank.
    pub fn production_tick(&mut self, dt: f64, bank: &mut ResourceState) {
        for plot in &self.plots {
            let PlotState::Built { burning, .. } = plot.state else { continue };
            if burning || !plot.staffed {
                continue;
            }
            let Some(kind) = plot.kind else { continue };
            let Some((res, rate)) = kind.produces() else { continue };
            let amount = rate * dt;
            match res {
                Resource::Food => bank.add_food(amount),
                Resource::Wood => bank.add_wood(amount),
                Resource::Stone => bank.add_stone(amount),
            }
        }
    }

    /// Drain upkeep, then grow population by one if food + a free house slot allow.
    /// Returns `true` when a villager was added (the Bevy layer then spawns a body).
    ///
    /// Upkeep uses all-or-nothing `spend_food`, so when the bank can't cover the
    /// full upkeep it is silently skipped — there is deliberately NO starvation
    /// consequence in this slice (no attrition / building shutdown). If starvation
    /// feedback is added later, this returns a shortfall signal instead.
    pub fn population_tick(&mut self, dt: f64, bank: &mut ResourceState) -> bool {
        let upkeep = self.population as f64 * UPKEEP_PER_POP * dt;
        bank.spend_food(upkeep); // spend_food no-ops when short (all-or-nothing)
        if self.population < self.pop_cap() && bank.spend_food(FOOD_PER_VILLAGER) {
            self.population += 1;
            return true;
        }
        false
    }

    /// Apply `amount` damage to a built plot, igniting it. Collapses to Rubble at <= 0.
    pub fn damage(&mut self, idx: usize, amount: f64) {
        let Some(plot) = self.plots.get_mut(idx) else { return };
        if let PlotState::Built { hp, burning } = &mut plot.state {
            *hp -= amount;
            *burning = true;
            if *hp <= 0.0 {
                plot.state = PlotState::Rubble;
                plot.kind = None;
                plot.staffed = false;
            }
        }
    }

    /// Heal built (damaged) plots toward max; clear `burning` once full. Day-only.
    pub fn repair(&mut self, dt: f64) {
        for plot in &mut self.plots {
            if let PlotState::Built { hp, burning } = &mut plot.state {
                if let Some(kind) = plot.kind {
                    let max = kind.max_hp();
                    if *hp < max {
                        *hp = (*hp + REPAIR_PER_SEC * dt).min(max);
                    }
                    if *hp >= max {
                        *burning = false;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank_with(wood: f64, stone: f64, food: f64) -> ResourceState {
        let mut b = ResourceState::new();
        b.add_wood(wood);
        b.add_stone(stone);
        b.add_food(food);
        b
    }

    #[test]
    fn new_town_has_empty_plots() {
        let t = Town::new(4, 3);
        assert_eq!(t.plots.len(), 4);
        assert!(t.plots.iter().all(|p| p.state == PlotState::Empty));
        assert_eq!(t.population, 3);
        assert_eq!(t.pop_cap(), 0);
    }

    #[test]
    fn build_deducts_cost_and_sets_built() {
        let mut t = Town::new(2, 0);
        let mut bank = bank_with(20.0, 10.0, 0.0);
        assert!(t.build(0, BuildKind::Farm, &mut bank));
        assert!(t.plots[0].is_built());
        assert_eq!(bank.wood(), 12.0); // 20 - 8
        assert_eq!(bank.stone(), 10.0); // farm costs no stone
    }

    #[test]
    fn build_fails_when_unaffordable_and_changes_nothing() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(2.0, 0.0, 0.0);
        assert!(!t.build(0, BuildKind::Farm, &mut bank));
        assert_eq!(t.plots[0].state, PlotState::Empty);
        assert_eq!(bank.wood(), 2.0);
    }

    #[test]
    fn cannot_build_on_occupied_plot() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 50.0, 0.0);
        assert!(t.build(0, BuildKind::Farm, &mut bank));
        assert!(!t.build(0, BuildKind::House, &mut bank)); // already built
    }

    #[test]
    fn house_raises_pop_cap() {
        let mut t = Town::new(2, 0);
        let mut bank = bank_with(50.0, 50.0, 0.0);
        t.build(0, BuildKind::House, &mut bank);
        assert_eq!(t.pop_cap(), POP_PER_HOUSE);
    }

    #[test]
    fn staffed_farm_produces_food_unstaffed_does_not() {
        let mut t = Town::new(2, 0);
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.build(1, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true; // plot 1 stays unstaffed
        let before = bank.food();
        t.production_tick(2.0, &mut bank); // 2s at 0.5/s = +1 food from one farm
        assert!((bank.food() - (before + 1.0)).abs() < 1e-9);
    }

    #[test]
    fn burning_farm_halts_production() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true;
        t.damage(0, 5.0); // ignites
        let before = bank.food();
        t.production_tick(5.0, &mut bank);
        assert_eq!(bank.food(), before); // no output while burning
    }

    #[test]
    fn population_grows_when_food_and_cap_allow() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 50.0, FOOD_PER_VILLAGER + 5.0);
        t.build(0, BuildKind::House, &mut bank); // cap = 2
        let grew = t.population_tick(0.0, &mut bank); // dt 0 => no upkeep drain
        assert!(grew);
        assert_eq!(t.population, 1);
        assert!(bank.food() < FOOD_PER_VILLAGER); // a villager's food was spent
    }

    #[test]
    fn population_capped_by_houses() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 50.0, 1000.0);
        t.build(0, BuildKind::House, &mut bank); // cap = 2
        assert!(t.population_tick(0.0, &mut bank));
        assert!(t.population_tick(0.0, &mut bank));
        assert!(!t.population_tick(0.0, &mut bank)); // hit cap of 2
        assert_eq!(t.population, 2);
    }

    #[test]
    fn damage_collapses_to_rubble_at_zero() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.damage(0, 1000.0);
        assert_eq!(t.plots[0].state, PlotState::Rubble);
        assert_eq!(t.plots[0].kind, None);
        assert!(t.plots[0].is_buildable()); // can rebuild on rubble
    }

    #[test]
    fn repair_heals_and_extinguishes() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.damage(0, 30.0); // hp 30, burning
        t.repair(100.0); // heals to max, clears burning
        assert_eq!(t.plots[0].state, PlotState::Built { hp: 60.0, burning: false });
    }

    #[test]
    fn rebuild_on_rubble_succeeds() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 50.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.damage(0, 1000.0); // -> rubble
        assert!(t.build(0, BuildKind::House, &mut bank));
        assert_eq!(t.plots[0].kind, Some(BuildKind::House));
    }
}
