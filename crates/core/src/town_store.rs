//! The town/settlement model — pure, tested, zero-dep. Drives the Bevy `town.rs`
//! city-building layer.
//!
//! **Population is a flow, not a stock.** Every peasant eats; every staffed Farm feeds.
//! The *net* food rate (production − upkeep) accumulates in a signed [`Town::growth`]
//! meter: cross +[`SETTLE_FOOD`] with a free house slot and a peasant **settles** in;
//! cross −[`SETTLE_FOOD`] and one **starves** away (no floor — chronic deficit can empty
//! the town). Food is therefore never banked: it is a daily balance you keep positive,
//! not a hoard. Houses (protected, inside the walls) set the population cap; producers
//! (Farm, Woodcutter — exposed on the outer plots) can burn at night.
//!
//! Numbers are tuned for forest's 60-HP combat units and the day/night siege cadence;
//! tweak the constants below (also exposed to the F1 debug panel).

use crate::resource_store::ResourceState;

/// Villagers a single House adds to the population cap.
pub const POP_PER_HOUSE: u32 = 2;
/// House build-slots inside the walls (the castle's interior dwellings).
pub const MAX_HOUSES: u32 = 12;
/// Houses a fresh town starts with (→ `START_POP` peasants at cap).
pub const START_HOUSES: u32 = 2;
/// Peasants a fresh town starts with (= `START_HOUSES * POP_PER_HOUSE`).
pub const START_POP: u32 = START_HOUSES * POP_PER_HOUSE;
/// Net food (production − upkeep) that must accumulate to settle one new peasant — or,
/// as a deficit, to starve one away. The `growth` meter is clamped to ±this.
pub const SETTLE_FOOD: f64 = 20.0;
/// Food eaten per peasant per second (upkeep). 4 peasants ≈ 0.16/s, so one staffed Farm
/// (0.5/s) comfortably feeds the starting town with a growing surplus.
pub const UPKEEP_PER_POP: f64 = 0.04;
/// Building HP healed per second while repairing (Prep phase).
pub const REPAIR_PER_SEC: f64 = 8.0;
/// Cost to raise one House (inside the walls). Houses don't burn, so this is the only gate
/// besides `MAX_HOUSES`.
pub const HOUSE_COST: Cost = Cost { wood: 6.0, stone: 4.0 };

/// What you can build on an outer plot — the **producers**. Houses are not plots (they sit
/// inside the walls; see [`Town::build_house`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BuildKind {
    Farm,
    /// Woodcutter — employs a woodcutter who fells real trees (the world layer banks wood per
    /// felled tree; there is NO passive wood trickle). Costs only stone so it can always be
    /// bootstrapped by mining, even once the starting wood stipend is spent (no chicken-and-egg).
    Lumber,
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
            BuildKind::Lumber => Cost { wood: 0.0, stone: 6.0 },
        }
    }

    /// `(resource, units per second)` of passive flow when staffed, or `None` for a producer
    /// whose yield is earned in the world instead (the Woodcutter: wood is banked per tree its
    /// worker actually fells — see the Bevy layer's `lumberjack.rs` / `verbs::fell_tree`).
    pub fn produces(self) -> Option<(Resource, f64)> {
        match self {
            BuildKind::Farm => Some((Resource::Food, 0.5)),
            BuildKind::Lumber => None,
        }
    }

    pub fn max_hp(self) -> f64 {
        match self {
            BuildKind::Farm => 60.0,
            BuildKind::Lumber => 55.0,
        }
    }

    /// Every producer needs a worker (kept as a method so the Bevy layer reads intent,
    /// and so a future non-staffed building type can opt out).
    pub fn needs_worker(self) -> bool {
        true
    }

    pub fn label(self) -> &'static str {
        match self {
            BuildKind::Farm => "Farm",
            BuildKind::Lumber => "Woodcutter",
        }
    }
}

/// A plot's lifecycle. `hp`/`burning` live inside `Built`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlotState {
    Empty,
    Built { hp: f64, burning: bool },
    Rubble,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

/// What a [`Town::population_tick`] did this step — drives the Bevy layer's body
/// spawn/despawn + the "a villager joined / left" feedback float.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopEvent {
    None,
    Grew,
    Starved,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Town {
    pub plots: Vec<Plot>,
    pub population: u32,
    /// Built houses inside the walls (0..=`MAX_HOUSES`). Sets the population cap.
    pub houses: u32,
    /// Signed net-food meter in food-units, clamped to ±[`SETTLE_FOOD`]. Positive →
    /// approaching a new settler; negative → approaching a starvation loss.
    pub growth: f64,
}

impl Town {
    /// A fresh town with `n` empty producer plots and the given starting population
    /// (houses default to 0 — the game uses [`Town::reset`] for its `START_*` state).
    pub fn new(n: usize, start_population: u32) -> Self {
        Self { plots: vec![Plot::empty(); n], population: start_population, houses: 0, growth: 0.0 }
    }

    /// Re-init for a new run: empty plots, the starting houses + peasants, zeroed meter.
    pub fn reset(&mut self) {
        for p in &mut self.plots {
            *p = Plot::empty();
        }
        self.population = START_POP;
        self.houses = START_HOUSES;
        self.growth = 0.0;
    }

    pub fn pop_cap(&self) -> u32 {
        self.houses * POP_PER_HOUSE
    }

    // ── Houses (interior, protected — a count, not a plot) ─────────────────────────

    pub fn can_build_house(&self, bank: &ResourceState) -> bool {
        self.houses < MAX_HOUSES && bank.wood() >= HOUSE_COST.wood && bank.stone() >= HOUSE_COST.stone
    }

    /// Raise one House (spending wood+stone atomically). Returns true on success.
    pub fn build_house(&mut self, bank: &mut ResourceState) -> bool {
        if !self.can_build_house(bank) {
            return false;
        }
        bank.spend_wood(HOUSE_COST.wood);
        bank.spend_stone(HOUSE_COST.stone);
        self.houses += 1;
        true
    }

    // ── Producer plots (outer, burnable) ───────────────────────────────────────────

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

    // ── Food economy (flow) ────────────────────────────────────────────────────────

    /// Food produced per second by every staffed, non-burning Farm.
    pub fn food_rate(&self) -> f64 {
        self.plots
            .iter()
            .filter_map(|p| match (p.state, p.kind, p.staffed) {
                (PlotState::Built { burning: false, .. }, Some(k), true) => match k.produces() {
                    Some((Resource::Food, r)) => Some(r),
                    _ => None,
                },
                _ => None,
            })
            .sum()
    }

    /// Food eaten per second by the whole population.
    pub fn upkeep_rate(&self) -> f64 {
        self.population as f64 * UPKEEP_PER_POP
    }

    /// Net daily food balance (production − upkeep). Positive grows the town, negative
    /// starves it. The HUD shows this so the player can see why population moves.
    pub fn net_food(&self) -> f64 {
        self.food_rate() - self.upkeep_rate()
    }

    /// Progress of the settle/starve meter as a signed fraction in [-1, 1]
    /// (+1 = a peasant about to arrive, -1 = one about to leave). For the HUD bar.
    pub fn growth_fraction(&self) -> f64 {
        self.growth / SETTLE_FOOD
    }

    /// Advance the population by the net-food flow. Surplus + a free house slot settles a
    /// peasant; a sustained deficit starves one (no floor). At most one change per call.
    pub fn population_tick(&mut self, dt: f64) -> PopEvent {
        self.growth += self.net_food() * dt;
        if self.growth >= SETTLE_FOOD && self.population < self.pop_cap() {
            self.population += 1;
            self.growth -= SETTLE_FOOD;
            return PopEvent::Grew;
        }
        if self.growth <= -SETTLE_FOOD && self.population > 0 {
            self.population -= 1;
            self.growth += SETTLE_FOOD;
            return PopEvent::Starved;
        }
        // Capped surplus / spent town: park the meter at the rail (bar full / empty).
        self.growth = self.growth.clamp(-SETTLE_FOOD, SETTLE_FOOD);
        PopEvent::None
    }

    /// Each staffed, non-burning producer banks its passive **material** yield (wood/stone).
    /// Food is a flow (see [`Town::food_rate`]) and is deliberately NOT banked. Note: with the
    /// current building set this banks nothing — the Woodcutter has no passive flow (its wood
    /// comes from real felled trees) — but the mechanism stays for future quarry-like producers.
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
                Resource::Food => {} // a flow, not banked
                Resource::Wood => bank.add_wood(amount),
                Resource::Stone => bank.add_stone(amount),
            }
        }
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
        assert_eq!(t.pop_cap(), 0); // no houses yet
    }

    #[test]
    fn reset_seeds_starting_houses_and_population() {
        let mut t = Town::new(8, 0);
        t.reset();
        assert_eq!(t.houses, START_HOUSES);
        assert_eq!(t.population, START_POP);
        assert_eq!(t.pop_cap(), START_POP); // starts exactly at cap
        assert_eq!(t.growth, 0.0);
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
        assert!(!t.build(0, BuildKind::Lumber, &mut bank)); // already built
    }

    #[test]
    fn house_raises_pop_cap_and_costs_resources() {
        let mut t = Town::new(0, 0);
        let mut bank = bank_with(50.0, 50.0, 0.0);
        assert!(t.build_house(&mut bank));
        assert_eq!(t.houses, 1);
        assert_eq!(t.pop_cap(), POP_PER_HOUSE);
        assert_eq!(bank.wood(), 44.0); // 50 - 6
        assert_eq!(bank.stone(), 46.0); // 50 - 4
    }

    #[test]
    fn houses_cap_at_max() {
        let mut t = Town::new(0, 0);
        let mut bank = bank_with(1000.0, 1000.0, 0.0);
        for _ in 0..MAX_HOUSES {
            assert!(t.build_house(&mut bank));
        }
        assert_eq!(t.houses, MAX_HOUSES);
        assert!(!t.build_house(&mut bank)); // no more slots
    }

    #[test]
    fn food_rate_sums_staffed_unburnt_farms_only() {
        let mut t = Town::new(3, 0);
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.build(1, BuildKind::Farm, &mut bank);
        t.build(2, BuildKind::Lumber, &mut bank);
        t.plots[0].staffed = true; // farm 0 staffed
        t.plots[1].staffed = true; // farm 1 staffed
        t.plots[2].staffed = true; // woodcutter doesn't feed
        assert!((t.food_rate() - 1.0).abs() < 1e-9); // two farms × 0.5
        t.damage(0, 5.0); // ignite farm 0 → drops out
        assert!((t.food_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn net_food_is_production_minus_upkeep() {
        let mut t = Town::new(1, 4); // 4 peasants → upkeep 0.16/s
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true;
        // 0.5 produced − 4 × 0.04 upkeep = 0.34.
        assert!((t.net_food() - 0.34).abs() < 1e-9);
    }

    #[test]
    fn woodcutter_costs_stone_only_and_has_no_passive_wood() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(0.0, 10.0, 0.0); // no wood, some stone
        assert!(t.build(0, BuildKind::Lumber, &mut bank));
        assert_eq!(bank.stone(), 4.0); // 10 - 6
        t.plots[0].staffed = true;
        let before = bank.wood();
        // No trickle: wood is banked per tree the woodcutter actually fells (world layer).
        t.production_tick(60.0, &mut bank);
        assert_eq!(bank.wood(), before);
    }

    #[test]
    fn farm_food_is_not_banked() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true;
        let before = bank.food();
        t.production_tick(5.0, &mut bank);
        assert_eq!(bank.food(), before); // food is a flow (food_rate), never hoarded
    }

    #[test]
    fn population_settles_when_surplus_and_a_house_is_free() {
        let mut t = Town::new(1, 0);
        t.houses = 1; // cap = 2, population 0 → upkeep 0
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true; // +0.5/s, no upkeep
        // 0.5 × 40s = 20 = SETTLE_FOOD → one peasant settles.
        assert_eq!(t.population_tick(40.0), PopEvent::Grew);
        assert_eq!(t.population, 1);
    }

    #[test]
    fn population_is_capped_by_houses() {
        let mut t = Town::new(1, 0);
        t.houses = 1; // cap = 2
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true;
        // dt=60 clears SETTLE_FOOD even after upkeep kicks in past the first peasant.
        assert_eq!(t.population_tick(60.0), PopEvent::Grew); // → 1
        assert_eq!(t.population_tick(60.0), PopEvent::Grew); // → 2 (cap)
        assert_eq!(t.population_tick(60.0), PopEvent::None); // capped
        assert_eq!(t.population, 2);
    }

    #[test]
    fn population_starves_on_a_food_deficit() {
        let mut t = Town::new(0, 2); // 2 peasants, no farms → net −0.08/s
        // −0.08 × 250s = −20 = −SETTLE_FOOD → one starves.
        assert_eq!(t.population_tick(250.0), PopEvent::Starved);
        assert_eq!(t.population, 1);
    }

    #[test]
    fn starvation_has_no_floor_but_stops_at_zero() {
        let mut t = Town::new(0, 1); // 1 peasant, no food
        assert_eq!(t.population_tick(600.0), PopEvent::Starved);
        assert_eq!(t.population, 0);
        // Empty town, nothing left to lose.
        assert_eq!(t.population_tick(600.0), PopEvent::None);
        assert_eq!(t.population, 0);
    }

    #[test]
    fn growth_fraction_tracks_the_meter() {
        let mut t = Town::new(1, 0);
        t.houses = 1;
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true;
        t.population_tick(20.0); // +0.5 × 20 = +10 = half of SETTLE_FOOD
        assert!((t.growth_fraction() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn burning_farm_halts_food() {
        let mut t = Town::new(1, 0);
        let mut bank = bank_with(50.0, 0.0, 0.0);
        t.build(0, BuildKind::Farm, &mut bank);
        t.plots[0].staffed = true;
        assert!((t.food_rate() - 0.5).abs() < 1e-9);
        t.damage(0, 5.0); // ignites
        assert_eq!(t.food_rate(), 0.0); // no output while burning
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
        assert!(t.build(0, BuildKind::Lumber, &mut bank));
        assert_eq!(t.plots[0].kind, Some(BuildKind::Lumber));
    }
}
