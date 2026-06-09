# City-Building Town Economy — Implementation Plan (Vertical Slice)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a city-building layer — place producer buildings on pre-placed plots, villagers auto-staff them, food grows population, and orks burn buildings at night (burn & repair) — shipped as a vertical slice (Farm + House) that proves every integration seam.

**Architecture:** Pure tested loop math lives in `crates/core` (extend `resource_store` with food/wood; new `town_store` module). A new Bevy plugin `src/town.rs` wraps the core `Town` as a Resource and owns plots, the build Modal, production/population ticks, and the night burn/repair. It reuses the existing contextual-`E` interaction, the villager bodies (for workers), the siege phase machine, and the HUD.

**Tech Stack:** Rust, Bevy 0.18, `tileworld_core` (f64, zero-dep, unit-tested). Core verified by `cargo test -p tileworld_core`; Bevy systems verified by `cargo check` + `FOREST_SHOT` screenshots + manual play (this repo does **not** unit-test the Bevy layer — only core).

**Spec:** `docs/superpowers/specs/2026-06-09-city-building-economy-design.md`

**Branch:** `feat/town-economy` (already created; spec already committed).

---

## File Structure

| File | Create/Modify | Responsibility |
|---|---|---|
| `crates/core/src/resource_store.rs` | Modify | Add `food` + `wood` to `ResourceState` (mirror `stone`). |
| `crates/core/src/town_store.rs` | **Create** | Pure tested settlement model: `BuildKind`, `PlotState`, `Plot`, `Town`, build/produce/population/damage/repair math. |
| `crates/core/src/lib.rs` | Modify | Register `pub mod town_store;`. |
| `src/town.rs` | **Create** | Bevy plugin: `TownRes` wrapper, plot seeding, build Modal, production/population ticks, burn/repair, fire VFX, `Worker` component, `PendingBuildingDamage`. |
| `src/main.rs` | Modify | Register `town::TownPlugin`. |
| `src/game_state.rs` | Modify | Add `Modal::Build` variant. |
| `src/interaction.rs` | Modify | Add `InteractKind::Build` + a `BuildTarget` resource for the active plot. |
| `src/worldmap.rs` | Modify | Call `town::populate_plots(...)` from `build`. |
| `src/villagers.rs` | Modify | `worker_steer` system: move `Worker`-tagged villagers to their plot, set `staffed`. |
| `src/orks.rs` | Modify | In `invader_brain`, divert a fraction of invaders to the nearest built plot; push damage to `PendingBuildingDamage`. |
| `src/hud.rs` | Modify | Add `FoodText` + `WoodText` counters to the resource row. |
| `src/capture.rs` | Modify | `FOREST_TOWN` env hook to stage a built/burning town for screenshots. |

**Decomposition note:** worker *movement* stays in `villagers.rs` (it needs the private `Villager` fields); everything else town-related lives in `town.rs`. The core `Town` only carries a `staffed: bool` per plot (synced from Bevy each frame), so it stays zero-dep and fully testable.

---

## Phase 1 — Core: food + wood resources (TDD)

### Task 1: Extend `ResourceState` with food + wood

**Files:**
- Modify: `crates/core/src/resource_store.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `crates/core/src/resource_store.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p tileworld_core resource_store`
Expected: FAIL — `no method named food`/`add_food`/`spend_food`/`wood`/... on `ResourceState`.

- [ ] **Step 3: Implement food + wood (mirror stone)**

In `crates/core/src/resource_store.rs`, add fields and mutators:

```rust
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
```

Add these methods inside `impl ResourceState` (alongside the stone ones):

```rust
    pub fn food(&self) -> f64 {
        self.food
    }
    pub fn wood(&self) -> f64 {
        self.wood
    }

    /// Add food; non-positive ignored (mirrors `add_stone`).
    pub fn add_food(&mut self, n: f64) {
        if n <= 0.0 {
            return;
        }
        self.food += n;
    }
    pub fn add_wood(&mut self, n: f64) {
        if n <= 0.0 {
            return;
        }
        self.wood += n;
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
```

Update `reset` to zero all three:

```rust
    pub fn reset(&mut self) {
        self.stone = 0.0;
        self.food = 0.0;
        self.wood = 0.0;
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p tileworld_core resource_store`
Expected: PASS (all stone tests stay green; new food/wood tests pass).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/resource_store.rs
git commit -m "core: add food + wood to ResourceState"
```

---

## Phase 2 — Core: the `town_store` settlement model (TDD)

### Task 2: Building kinds, costs, and plot/town types

**Files:**
- Create: `crates/core/src/town_store.rs`
- Modify: `crates/core/src/lib.rs`

- [ ] **Step 1: Register the module**

In `crates/core/src/lib.rs`, add (alphabetical, after `pub mod tilemap;`):

```rust
pub mod town_store;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/core/src/town_store.rs` with the full module below — types, impls, and tests together (TDD here means writing the tested behaviour in one focused module; the tests drive the impl). Write the **tests first** mentally, then the impl that satisfies them. The complete file:

```rust
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
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p tileworld_core town_store`
Expected: PASS (all 12 town_store tests).

- [ ] **Step 4: Run the whole core suite (no regressions)**

Run: `cargo test -p tileworld_core`
Expected: PASS (the existing ~268 + the new resource/town tests).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/town_store.rs crates/core/src/lib.rs
git commit -m "core: town_store settlement model (build/produce/population/burn/repair)"
```

---

## Phase 3 — Bevy: Modal::Build state + TownRes + plugin skeleton

### Task 3: Add the `Build` modal substate

**Files:**
- Modify: `src/game_state.rs:39-45`

- [ ] **Step 1: Add the `Build` variant**

In `src/game_state.rs`, extend the `Modal` enum:

```rust
#[derive(SubStates, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[source(AppState = AppState::Playing)]
#[allow(dead_code)]
pub enum Modal {
    #[default]
    None,
    Shop,
    UpgradeTree,
    Inventory,
    Build,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: PASS (the existing `modal_substate_gates_only_while_playing` test still compiles; no behaviour change).

- [ ] **Step 3: Commit**

```bash
git add src/game_state.rs
git commit -m "game_state: add Modal::Build substate"
```

### Task 4: Create `src/town.rs` skeleton + register the plugin

**Files:**
- Create: `src/town.rs`
- Modify: `src/main.rs:73-149`

- [ ] **Step 1: Create the plugin skeleton**

Create `src/town.rs`:

```rust
//! **City-building town economy.** Wraps the tested `tileworld_core::town_store::Town`
//! as a Resource and owns: pre-placed build plots, the `Modal::Build` construction
//! menu, the production + population ticks, and the night burn/repair. Villagers
//! auto-staff producers (worker steering lives in `villagers.rs`); a fraction of
//! night invaders divert here to burn buildings (`orks.rs` pushes `PendingBuildingDamage`).
//!
//! Sim systems carry `.run_if(in_state(Modal::None))` per the freeze gate; VFX/render
//! stay ungated. Numbers live in `town_store` (test-gated).

use bevy::prelude::*;
use tileworld_core::town_store::{BuildKind, Town};

use crate::economy::Bank;
use crate::game_state::{AppState, Modal};

/// Number of build plots seeded around the castle.
pub const PLOT_COUNT: usize = 8;
/// Starting wood so the player can build on day one.
const START_WOOD: f64 = 16.0;

/// The settlement model (parity-tested core) as a Bevy Resource.
#[derive(Resource)]
pub struct TownRes(pub Town);

impl Default for TownRes {
    fn default() -> Self {
        Self(Town::new(PLOT_COUNT, 0))
    }
}

/// Marks a build-plot entity; `idx` indexes `TownRes.0.plots`.
#[derive(Component)]
pub struct BuildPlot {
    pub idx: usize,
}

/// The building mesh sitting on a plot (despawned on collapse/rebuild).
#[derive(Component)]
pub struct BuildingMesh {
    pub idx: usize,
}

/// Tags a villager assigned to staff a plot (set by town auto-assign, read by
/// `villagers::worker_steer`). `at_post` flips true once it reaches the building.
#[derive(Component)]
pub struct Worker {
    pub idx: usize,
    pub at_post: bool,
}

/// Damage night invaders deal to buildings this frame: `(plot_idx, damage)`.
/// `orks::invader_brain` pushes; `apply_building_damage` drains.
#[derive(Resource, Default)]
pub struct PendingBuildingDamage(pub Vec<(usize, f32)>);

/// Which buildable plot the hero is standing on (set by `interaction.rs`), so the
/// Build panel knows where to build. `None` when not on a buildable plot.
#[derive(Resource, Default)]
pub struct BuildTarget(pub Option<usize>);

pub struct TownPlugin;

impl Plugin for TownPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TownRes>()
            .init_resource::<PendingBuildingDamage>()
            .init_resource::<BuildTarget>()
            .add_systems(OnExit(AppState::StartScreen), reset_town)
            .add_systems(OnExit(AppState::GameOver), reset_town);
        // Plot seeding, build Modal, ticks, and burn/repair are added in later tasks.
    }
}

/// New run: clear the town and seed starting wood. Mirrors `economy::reset_economy`.
fn reset_town(mut town: ResMut<TownRes>, mut bank: ResMut<Bank>) {
    town.0.reset(0);
    bank.0.add_wood(START_WOOD);
}

/// World-space centre of plot `idx` (set when plots are seeded).
pub fn plot_world(idx: usize, plots: &[Vec2]) -> Vec2 {
    plots.get(idx).copied().unwrap_or(Vec2::ZERO)
}
```

- [ ] **Step 2: Register the module + plugin**

In `src/main.rs`, add `mod town;` (alphabetical, after `mod terrain;`):

```rust
mod terrain;
mod town;
mod training_dummies;
```

Add `town::TownPlugin` to the third `add_plugins((...))` tuple (it has room; if any tuple is at arity 15, start a new `.add_plugins((...))` call):

```rust
            interaction::InteractionPlugin, // contextual E (keep→upgrades, merchant→shop, bell→night)
            town::TownPlugin,               // city-building: plots, build menu, economy, burn/repair
            debug_stats::DebugStatsPlugin, // read-only perf/state telemetry overlay (toggle: F2)
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: PASS. Warnings about unused `BuildPlot`/`Worker`/`plot_world`/`BuildingMesh` are fine (wired in later tasks).

- [ ] **Step 4: Commit**

```bash
git add src/town.rs src/main.rs
git commit -m "town: plugin skeleton (TownRes, plot/worker/damage types, reset)"
```

---

## Phase 4 — Bevy: seed + render build plots

### Task 5: Seed plot entities around the castle

**Files:**
- Modify: `src/town.rs`
- Modify: `src/worldmap.rs` (call site in `build`)

Plots sit at fixed diagonal offsets around the castle — in the flat grass safe-zone (`SAFE_R = 18.0`), off the cardinal gate-approach lanes, so a building never blocks the A* path to a gate. Diagonal placement makes "off-lane" automatic.

- [ ] **Step 1: Add plot positions + seeding to `town.rs`**

Add to `src/town.rs`:

```rust
use crate::palette::lin;

/// Stores the world-XZ centre of every seeded plot (index = plot idx).
#[derive(Resource, Default)]
pub struct PlotSpots(pub Vec<Vec2>);

/// Fixed plot offsets from the castle origin — two diagonal rings, off the
/// cardinal gate lanes, inside the grass safe-zone (|offset| < ~16).
const PLOT_OFFSETS: [Vec2; PLOT_COUNT] = [
    Vec2::new(8.0, 8.0),
    Vec2::new(-8.0, 8.0),
    Vec2::new(8.0, -8.0),
    Vec2::new(-8.0, -8.0),
    Vec2::new(13.0, 5.0),
    Vec2::new(-13.0, 5.0),
    Vec2::new(13.0, -5.0),
    Vec2::new(-13.0, -5.0),
];

/// Seed the build-plot entities + their foundation pads. Called from `worldmap::build`
/// after the castle so the safe-zone ground is final.
pub fn populate_plots(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    let pad = meshes.add(plot_pad_mesh());
    let mut spots = Vec::with_capacity(PLOT_COUNT);
    for (idx, off) in PLOT_OFFSETS.iter().enumerate() {
        let y = crate::worldmap::ground_at_world(off.x, off.y).unwrap_or(0.0);
        spots.push(*off);
        commands.spawn((
            Mesh3d(pad.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(off.x, y + 0.02, off.y),
            crate::biome::BiomeEntity,
            BuildPlot { idx },
        ));
    }
    commands.insert_resource(PlotSpots(spots));
}

/// A low foundation pad (flat-shaded, vertex-coloured per the mesh contract).
fn plot_pad_mesh() -> Mesh {
    use bevy::mesh::MeshBuilder;
    let mut m = Cuboid::new(3.4, 0.12, 3.4).mesh().build();
    crate::props::tinted(&mut m, lin(0x6b5a44)); // dusty foundation brown
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}
```

> Note: confirm the exact tint helper name in `src/props.rs` (the mesh contract uses `crate::palette::lin` for the colour; the COLOR-attribute helper may be `tinted`/`tint`/`with_color`). If `props::tinted` doesn't exist, use the equivalent helper the other prop builders call — grep `ATTRIBUTE_COLOR` in `src/` to find it.

- [ ] **Step 2: Wire the seeding into `worldmap::build`**

In `src/worldmap.rs`, find where ore is seeded (`crate::verbs::populate_ore(commands, meshes, std_mats);`, ~line 543) and add right after it:

```rust
    crate::town::populate_plots(commands, meshes, std_mats);
```

- [ ] **Step 3: Add the system that registers PlotSpots default**

`populate_plots` inserts `PlotSpots`, but add a default so systems that read it before `build` runs don't panic. In `TownPlugin::build`, add:

```rust
            .init_resource::<PlotSpots>()
```

- [ ] **Step 4: Build + screenshot to verify plots render**

Run: `cargo check`
Expected: PASS.

Run (PowerShell): `$env:FOREST_SHOT="shot_plots.png"; $env:FOREST_CAM="overhead"; cargo run`
Expected: PNG shows 8 foundation pads in a diagonal pattern around the castle, on flat grass, none on the gate approaches. (If `FOREST_CAM=overhead` isn't a valid pose, omit it and use the default follow-cam; the pads should be visible near the castle.)

- [ ] **Step 5: Commit**

```bash
git add src/town.rs src/worldmap.rs
git commit -m "town: seed 8 build plots in the castle safe-zone (off gate lanes)"
```

---

## Phase 5 — Bevy: build interaction + Modal::Build panel

### Task 6: `InteractKind::Build` + set `BuildTarget` when on a buildable plot

**Files:**
- Modify: `src/interaction.rs`

- [ ] **Step 1: Add the `Build` interact kind**

In `src/interaction.rs`, extend `InteractKind` (line 26) and its `prompt` (line 32):

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum InteractKind {
    Upgrades,
    Shop,
    WarBell,
    Build,
}
impl InteractKind {
    fn prompt(self) -> &'static str {
        match self {
            InteractKind::Upgrades => "Upgrades",
            InteractKind::Shop => "Shop",
            InteractKind::WarBell => "Ring the bell",
            InteractKind::Build => "Build",
        }
    }
}
```

Add a distance constant near the others (line 21):

```rust
const BUILD_DIST: f32 = 3.0;
```

- [ ] **Step 2: Add a Build candidate to `drive_interaction`**

In `drive_interaction` (line 69), the candidate array is fixed-size; instead compute the nearest buildable plot separately and add it as a candidate. Add these params to the system signature:

```rust
    plot_spots: Res<crate::town::PlotSpots>,
    town: Res<crate::town::TownRes>,
    mut build_target: ResMut<crate::town::BuildTarget>,
```

Inside the system, after `let p = hero.pos;`, find the nearest buildable plot in range:

```rust
    // Nearest buildable (Empty/Rubble) plot the hero is standing on.
    let mut nearest_plot: Option<(usize, f32)> = None;
    for (idx, spot) in plot_spots.0.iter().enumerate() {
        if town.0.plots.get(idx).map_or(false, |pl| pl.is_buildable()) {
            let d = p.distance(*spot);
            if d < BUILD_DIST && nearest_plot.map_or(true, |(_, bd)| d < bd) {
                nearest_plot = Some((idx, d));
            }
        }
    }
    build_target.0 = nearest_plot.map(|(i, _)| i);
```

Change the `candidates` array to a `Vec` so the Build option can be appended conditionally:

```rust
    let mut candidates: Vec<(InteractKind, Vec2, f32, bool)> = vec![
        (InteractKind::Upgrades, Vec2::ZERO, KEEP_DIST, true),
        (InteractKind::Shop, shop_anchor(), SHOP_DIST, true),
        (InteractKind::WarBell, Vec2::new(0.0, 6.0), BELL_DIST, siege.phase == GamePhase::Prep),
    ];
    if let Some((idx, _)) = nearest_plot {
        candidates.push((InteractKind::Build, plot_spots.0[idx], BUILD_DIST, true));
    }
```

Add the match arm in the E-press handler:

```rust
            InteractKind::Build => next_modal.set(Modal::Build),
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/interaction.rs
git commit -m "interaction: E on a buildable plot opens Modal::Build (sets BuildTarget)"
```

### Task 7: The Build Modal panel (spawn/despawn/click → build)

**Files:**
- Modify: `src/town.rs`

Mirror the `spawn_tree`/`despawn_tree`/`tree_interact` lifecycle from `economy.rs` (registered on `OnEnter(Modal::UpgradeTree)`/`OnExit`/`Update.run_if(in_state(...))`).

- [ ] **Step 1: Add panel markers, spawn, despawn, and click systems to `town.rs`**

Add to `src/town.rs` (imports: pull in the UI kit helpers the tree panel uses — `crate::ui::fonts::{label, UiFonts}`, `crate::ui::theme::*`, `crate::ui::widgets`, and `crate::ui::anim::{anim, AnimKind}`):

```rust
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets;

#[derive(Component)]
struct BuildUi;
#[derive(Component)]
struct BuildOption(BuildKind);

const MENU: [BuildKind; 2] = [BuildKind::Farm, BuildKind::House];

fn spawn_build(mut commands: Commands, fonts: Res<UiFonts>, bank: Res<Bank>, town: Res<TownRes>, target: Res<BuildTarget>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                margin: UiRect::new(Val::Px(-180.0), Val::Auto, Val::Px(-140.0), Val::Auto),
                width: Val::Px(360.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                padding: UiRect::all(Val::Px(20.0)),
                border: widgets::border(1.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            GlobalZIndex(60),
            BuildUi,
            anim(AnimKind::PopIn, 0.0, 0.22),
        ))
        .with_children(|root| {
            root.spawn(label(&fonts.extrabold, "BUILD", 20.0, GOLD));
            let buildable = target.0.is_some();
            if !buildable {
                root.spawn(label(&fonts.regular, "Stand on an empty plot to build.", 13.0, GREY));
            }
            for kind in MENU {
                let c = kind.cost();
                let afford = town.0.can_afford(kind, &bank.0) && buildable;
                let col = if afford { Color::WHITE } else { TEXT_FAINT };
                root.spawn((
                    Button,
                    Interaction::default(),
                    Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(9.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_CARD),
                        ..default()
                    },
                    BorderColor::all(if afford { GOLD_DEEP } else { BORDER_SOFT }),
                    BuildOption(kind),
                ))
                .with_children(|b| {
                    let need = kind.needs_worker();
                    let name = if need { format!("{}  (needs worker)", kind.label()) } else { kind.label().to_string() };
                    b.spawn(label(&fonts.semibold, &name, 14.0, col));
                    b.spawn(label(&fonts.semibold, &format!("Wood {}  Stone {}", c.wood as i64, c.stone as i64), 13.0, col));
                });
            }
            root.spawn(label(&fonts.regular, "Esc to close", 11.0, GREY));
        });
}

fn despawn_build(mut commands: Commands, q: Query<Entity, With<BuildUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

#[allow(clippy::too_many_arguments)]
fn build_interact(
    q: Query<(&Interaction, &BuildOption), Changed<Interaction>>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    target: Res<BuildTarget>,
    mut next_modal: ResMut<NextState<Modal>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spots: Res<PlotSpots>,
    existing: Query<(Entity, &BuildingMesh)>,
) {
    for (interaction, opt) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(idx) = target.0 else { continue };
        if town.0.build(idx, opt.0, &mut bank.0) {
            // Rebuild-on-rubble: clear any stale mesh first.
            for (e, bm) in &existing {
                if bm.idx == idx {
                    commands.entity(e).try_despawn();
                }
            }
            spawn_building_mesh(&mut commands, &mut meshes, &mut materials, idx, opt.0, &spots);
            next_modal.set(Modal::None); // close after a successful build
        }
    }
}
```

- [ ] **Step 2: Add the building mesh builder**

Add to `src/town.rs` (simple slice meshes — a hut for Farm, a house box for House; polish is pass 2):

```rust
fn spawn_building_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    idx: usize,
    kind: BuildKind,
    spots: &PlotSpots,
) {
    let pos = spots.0.get(idx).copied().unwrap_or(Vec2::ZERO);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    commands.spawn((
        Mesh3d(meshes.add(building_mesh(kind))),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, y, pos.y),
        crate::biome::BiomeEntity,
        BuildingMesh { idx },
    ));
}

fn building_mesh(kind: BuildKind) -> Mesh {
    use bevy::mesh::MeshBuilder;
    let (body_col, roof_col, h) = match kind {
        BuildKind::Farm => (0xb9975a, 0x7a4a2a, 1.4),  // straw walls, brown thatch
        BuildKind::House => (0xcdbfa6, 0x8a3a2a, 2.0),  // plaster walls, red roof
    };
    let mut walls = Cuboid::new(2.2, h, 2.2).mesh().build();
    crate::props::tinted(&mut walls, lin(body_col));
    // shift walls up so the base sits at y=0
    translate_mesh(&mut walls, Vec3::new(0.0, h * 0.5, 0.0));
    let mut roof = Cuboid::new(2.6, 0.5, 2.6).mesh().build();
    crate::props::tinted(&mut roof, lin(roof_col));
    translate_mesh(&mut roof, Vec3::new(0.0, h + 0.25, 0.0));
    let mut m = walls;
    m.merge(&roof);
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

fn translate_mesh(m: &mut Mesh, by: Vec3) {
    use bevy::mesh::VertexAttributeValues;
    if let Some(VertexAttributeValues::Float32x3(ps)) = m.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        for p in ps.iter_mut() {
            p[0] += by.x;
            p[1] += by.y;
            p[2] += by.z;
        }
    }
}
```

> Note: confirm `Mesh::merge` returns `()` vs `Result` in this Bevy version (the mesh contract / verified-API doc). If `merge` consumes/returns differently, follow the form used in `src/villagers.rs` or `src/castle.rs` where parts are merged. `translate_mesh` is a local helper because the contract requires base-at-`y=0`; if a shared translate helper exists in `props.rs`, use it instead.

- [ ] **Step 3: Register the panel systems in `TownPlugin::build`**

```rust
            .add_systems(OnEnter(Modal::Build), spawn_build)
            .add_systems(OnExit(Modal::Build), despawn_build)
            .add_systems(Update, build_interact.run_if(in_state(Modal::Build)))
```

- [ ] **Step 4: Build + manual verify**

Run: `cargo check`
Expected: PASS.

Run: `cargo run` — walk the knight onto a plot pad; the `E` prompt should read "Build"; press E; the BUILD panel opens; click Farm; wood drops by 8, the panel closes, and a hut appears on the plot. Stand off a plot → opening shows "Stand on an empty plot to build."

- [ ] **Step 5: Commit**

```bash
git add src/town.rs
git commit -m "town: Modal::Build panel — construct Farm/House on the active plot"
```

---

## Phase 6 — Bevy: workers + production/population ticks

### Task 8: Auto-assign + steer workers; sync `staffed`

**Files:**
- Modify: `src/town.rs` (auto-assign — picks an idle villager)
- Modify: `src/villagers.rs` (worker steering — needs private `Villager` fields)

- [ ] **Step 1: Auto-assign system in `town.rs`**

Add to `src/town.rs`. It finds producer plots that are Built, need a worker, and have no live `Worker` entity yet, then tags the nearest idle villager. Villager position is read via a public accessor we add to `villagers.rs` in Step 2.

```rust
use crate::villagers::{Villager, Guard};

#[allow(clippy::type_complexity)]
fn auto_assign_workers(
    town: Res<TownRes>,
    spots: Res<PlotSpots>,
    mut commands: Commands,
    workers: Query<&Worker>,
    idle: Query<(Entity, &Transform), (With<Villager>, Without<Guard>, Without<Worker>)>,
) {
    for (idx, plot) in town.0.plots.iter().enumerate() {
        let Some(kind) = plot.kind else { continue };
        if !plot.is_built() || !kind.needs_worker() {
            continue;
        }
        if workers.iter().any(|w| w.idx == idx) {
            continue; // already has a worker assigned
        }
        let Some(spot) = spots.0.get(idx).copied() else { continue };
        // Nearest idle (or any) unassigned villager.
        let mut best: Option<(Entity, f32)> = None;
        for (e, tf) in &idle {
            let d = Vec2::new(tf.translation.x, tf.translation.z).distance(spot);
            if best.map_or(true, |(_, bd)| d < bd) {
                best = Some((e, d));
            }
        }
        if let Some((e, _)) = best {
            commands.entity(e).try_insert(Worker { idx, at_post: false });
        }
    }
}

/// Each frame, mark a plot `staffed` iff a posted, visible worker is on it.
fn sync_staffed(
    mut town: ResMut<TownRes>,
    workers: Query<(&Worker, &Visibility)>,
) {
    let n = town.0.plots.len();
    let mut staffed = vec![false; n];
    for (w, vis) in &workers {
        if w.at_post && *vis != Visibility::Hidden && w.idx < n {
            staffed[w.idx] = true;
        }
    }
    for (i, plot) in town.0.plots.iter_mut().enumerate() {
        plot.staffed = staffed[i];
    }
}

/// Drop the `Worker` tag when its plot is gone (collapsed to rubble), so the
/// villager rejoins the idle pool and auto-assign re-staffs survivors.
fn release_orphan_workers(
    town: Res<TownRes>,
    mut commands: Commands,
    workers: Query<(Entity, &Worker)>,
) {
    for (e, w) in &workers {
        let gone = town.0.plots.get(w.idx).map_or(true, |p| !p.is_built());
        if gone {
            commands.entity(e).try_remove::<Worker>();
        }
    }
}
```

- [ ] **Step 2: Worker steering in `villagers.rs`**

In `src/villagers.rs`, add a system that moves `Worker`-tagged villagers toward their plot (it can touch the private `Villager` fields). Add near `villager_brain`:

```rust
/// Steer assigned workers to their building, then hold post (sets `at_post`).
/// Lives here because it pokes the private `Villager` fields. Workers inherit
/// `townsfolk_curfew` (no `Guard`), so they flee at night automatically.
fn worker_steer(
    time: Res<Time>,
    spots: Res<crate::town::PlotSpots>,
    mut q: Query<(&mut crate::town::Worker, &mut Villager, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    for (mut worker, mut v, mut tf) in &mut q {
        let Some(post) = spots.0.get(worker.idx).copied() else { continue };
        // Stand just outside the building footprint.
        let to = post - v.pos;
        let dist = to.length();
        if dist < 1.6 {
            worker.at_post = true;
            v.moving = false;
        } else {
            worker.at_post = false;
            v.target = post;
            let cur_y = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
            if let Some(s) = steer::advance(v.pos, v.facing, v.target, v.speed * dt, v.body_r, cur_y, VIL_MAX_TURN * dt) {
                v.facing = s.facing;
                v.pos = s.pos;
                v.moving = s.moving;
            }
        }
        let gy = worldmap::ground_at_world(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}
```

Exclude workers from the ambient `villager_brain` so the two don't fight over the transform — change its query filter (line 358) to add `Without<crate::town::Worker>`:

```rust
    mut q: Query<(&mut Villager, &mut Transform, Has<Kid>), (Without<Guard>, Without<Pilgrim>, Without<crate::town::Worker>)>,
```

Register `worker_steer` in `VillagersPlugin::build` alongside the other gated systems (in the `.run_if(in_state(Modal::None))` tuple):

```rust
                    worker_steer,
```

- [ ] **Step 3: Register the town worker systems**

In `TownPlugin::build`, add to a gated `Update` tuple:

```rust
            .add_systems(
                Update,
                (auto_assign_workers, sync_staffed, release_orphan_workers)
                    .run_if(in_state(Modal::None)),
            )
```

- [ ] **Step 4: Build + manual verify**

Run: `cargo check` → PASS.
Run: `cargo run` — build a Farm; a villager should walk over and stand at it. (Food production is verified after the next task wires the HUD.)

- [ ] **Step 5: Commit**

```bash
git add src/town.rs src/villagers.rs
git commit -m "town: auto-assign + steer worker villagers; sync staffed"
```

### Task 9: Production + population ticks; HUD food/wood

**Files:**
- Modify: `src/town.rs` (ticks + villager spawn on growth)
- Modify: `src/hud.rs` (food + wood counters)

- [ ] **Step 1: Production + population tick systems in `town.rs`**

```rust
use crate::succession::Lives;

/// Staffed producers add their yield; runs only while playing (Modal::None).
fn production_system(time: Res<Time>, mut town: ResMut<TownRes>, mut bank: ResMut<Bank>) {
    let dt = time.delta_secs() as f64;
    town.0.production_tick(dt, &mut bank.0);
}

/// Food upkeep + growth; on growth, spawn a villager body and grow the bloodline
/// (keeps the existing house→heir tie).
fn population_system(
    time: Res<Time>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut lives: ResMut<Lives>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs() as f64;
    if town.0.population_tick(dt, &mut bank.0) {
        lives.heirs += 1;
        // A new townsperson appears at the castle courtyard (reuses the villager rig).
        let seed = 0x70b1_0000u32.wrapping_add(town.0.population.wrapping_mul(101));
        crate::villagers::spawn_courtyard_guard(&mut commands, &mut meshes, &mut materials, seed);
    }
}
```

> Note: `spawn_courtyard_guard` spawns an armed townsperson that defends at night. For the slice this is the simplest population body and it reuses an existing public spawner. If you'd rather grow *unarmed* workers, add a `spawn_townsperson` helper in `villagers.rs` mirroring `spawn_courtyard_guard` but with a peasant `Kind`. Either is fine for the slice; document the choice.

Register both (gated):

```rust
            .add_systems(
                Update,
                (production_system, population_system).run_if(in_state(Modal::None)),
            )
```

- [ ] **Step 2: Add food + wood to the HUD**

In `src/hud.rs`, add marker components (near `GoldText`/`StoneText`, line ~29):

```rust
#[derive(Component)]
struct FoodText;
#[derive(Component)]
struct WoodText;
```

In the resource row (line ~203, where `GoldText`/`StoneText` are spawned), add two more:

```rust
        r.spawn((label(&fonts.extrabold, "Food 0", 13.0, rgb(150, 220, 130)), FoodText));
        r.spawn((label(&fonts.extrabold, "Wood 0", 13.0, rgb(190, 150, 100)), WoodText));
```

In `update_hud`, add query params (mirroring `stone_txt`, being careful with the `Without<...>` disjointness the file already uses):

```rust
    mut food_txt: Query<&mut Text, (With<FoodText>, Without<GoldText>, Without<StoneText>, Without<WoodText>)>,
    mut wood_txt: Query<&mut Text, (With<WoodText>, Without<GoldText>, Without<StoneText>, Without<FoodText>)>,
```

And the update body (after the stone update):

```rust
    if let Ok(mut t) = food_txt.single_mut() {
        **t = format!("Food {}", bank.0.food() as i64);
    }
    if let Ok(mut t) = wood_txt.single_mut() {
        **t = format!("Wood {}", bank.0.wood() as i64);
    }
```

> Note: the existing `gold_txt`/`stone_txt` queries carry `Without<...>` filters for mutual disjointness — add `Without<FoodText>`/`Without<WoodText>` to **those** existing queries too, or the borrow checker will reject overlapping `&mut Text` access. Match the pattern already in the signature exactly.

- [ ] **Step 3: Build + manual verify the full economy loop**

Run: `cargo check` → PASS.
Run: `cargo run` — HUD shows Food/Wood. Build a Farm, a worker posts up, **Food counts up**. Build a House (cap rises), let food accumulate to 20 → a new villager appears and the heir count rises.

- [ ] **Step 4: Commit**

```bash
git add src/town.rs src/hud.rs
git commit -m "town: production + population ticks; HUD food/wood counters"
```

---

## Phase 7 — Bevy: night burn & repair

### Task 10: Divert invaders to burn buildings

**Files:**
- Modify: `src/orks.rs` (`invader_brain`)

A deterministic fraction of invaders (by entity-id parity) target the nearest built plot instead of the keep; on contact they push damage to `PendingBuildingDamage`.

- [ ] **Step 1: Add building-target params to `invader_brain`**

In `src/orks.rs`, add to the `invader_brain` signature:

```rust
    town: Res<crate::town::TownRes>,
    plot_spots: Res<crate::town::PlotSpots>,
    mut building_dmg: ResMut<crate::town::PendingBuildingDamage>,
```

Add a tuning const near the other ork combat consts:

```rust
/// How close an invader must be to batter a building.
const BUILDING_ATTACK_RANGE: f32 = 1.8;
/// Building damage per invader per second. FORGIVING slice tuning: a farm (60 HP)
/// survives ~12s of one undefended arsonist, so you can usually reach it in time.
const BUILDING_DPS: f32 = 5.0;
```

- [ ] **Step 2: Insert the building-diversion branch**

In `invader_brain`, the target decision is around line 663 (`let target = if let Some((_, gp)) = guard_tgt { ... } else { KEEP_POS }`). A fraction of invaders prefer a building when the hero/guards aren't the priority. Replace the final `else { KEEP_POS }` branch with a building check:

```rust
    // FORGIVING slice tuning: only ~1/3 of the warband (by id) are arsonists; the
    // rest still rush the keep. They make for the nearest standing building.
    // The keep's existing defenses (towers/archers/ballista in `defenses.rs`)
    // already auto-target ANY WaveInvader, so arsonists get shot on approach —
    // no extra wiring needed for defenses to help save the town.
    let arsonist = (e.to_bits() % 3) == 0;
    let building_goal: Option<(usize, Vec2)> = if arsonist {
        let mut best: Option<(usize, f32)> = None;
        for (idx, spot) in plot_spots.0.iter().enumerate() {
            if town.0.plots.get(idx).map_or(false, |p| p.is_built()) {
                let d = o.pos.distance(*spot);
                if best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((idx, d));
                }
            }
        }
        best.map(|(i, _)| (i, plot_spots.0[i]))
    } else {
        None
    };

    let target = if let Some((_, gp)) = guard_tgt {
        gp
    } else if chase_hero {
        hero.pos
    } else if let Some((bidx, bpos)) = building_goal {
        // Batter the building when in range; else march toward it.
        if o.pos.distance(bpos) < BUILDING_ATTACK_RANGE {
            building_dmg.0.push((bidx, BUILDING_DPS * dt));
        }
        bpos
    } else {
        KEEP_POS
    };
```

> Note: `dt` here is the frame delta already computed in `invader_brain` (the system reads `time.delta_secs()`; reuse that local). `e` is the invader entity from the query tuple. If the pathing block (lines 720-746) keys off `KEEP_POS`/`keep_march_goal`, the diverted invader will A* toward `target` the same way it does the keep — buildings are off the gate lanes but inside the safe-zone, so they're reachable. Verify the diverted invaders actually path to and reach a building during a wave.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check` → PASS.

- [ ] **Step 4: Commit**

```bash
git add src/orks.rs
git commit -m "orks: arsonist invaders divert to burn the nearest building"
```

### Task 11: Apply building damage, fire VFX, collapse to rubble, repair

**Files:**
- Modify: `src/town.rs`

- [ ] **Step 1: Drain `PendingBuildingDamage` → apply, ignite, collapse**

Add to `src/town.rs`:

```rust
/// A flame entity tied to a burning plot (despawned when extinguished/collapsed).
#[derive(Component)]
struct Flame {
    idx: usize,
}

#[allow(clippy::too_many_arguments)]
fn apply_building_damage(
    mut town: ResMut<TownRes>,
    mut pending: ResMut<PendingBuildingDamage>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spots: Res<PlotSpots>,
    buildings: Query<(Entity, &BuildingMesh)>,
    flames: Query<(Entity, &Flame)>,
) {
    for (idx, dmg) in pending.0.drain(..) {
        let was_built = town.0.plots.get(idx).map_or(false, |p| p.is_built());
        town.0.damage(idx, dmg as f64);
        if !was_built {
            continue;
        }
        let now_rubble = town.0.plots.get(idx).map_or(false, |p| matches!(p.state, tileworld_core::town_store::PlotState::Rubble));
        if now_rubble {
            // Collapse: drop the building mesh + its flames, leave the bare plot (rubble).
            for (e, bm) in &buildings {
                if bm.idx == idx {
                    commands.entity(e).try_despawn();
                }
            }
            for (e, f) in &flames {
                if f.idx == idx {
                    commands.entity(e).try_despawn();
                }
            }
        } else {
            // Still standing + burning: ensure a flame is showing.
            let has_flame = flames.iter().any(|(_, f)| f.idx == idx);
            if !has_flame {
                spawn_flame(&mut commands, &mut meshes, &mut materials, idx, &spots);
            }
        }
    }
}

fn spawn_flame(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    idx: usize,
    spots: &PlotSpots,
) {
    let pos = spots.0.get(idx).copied().unwrap_or(Vec2::ZERO);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.45, 0.1),
        emissive: LinearRgba::rgb(6.0, 2.0, 0.3),
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(0.6).mesh().build())),
        MeshMaterial3d(mat),
        Transform::from_xyz(pos.x, y + 1.6, pos.y),
        crate::biome::BiomeEntity,
        Flame { idx },
        PointLight { color: Color::srgb(1.0, 0.5, 0.2), intensity: 60_000.0, range: 10.0, ..default() },
    ));
}
```

> Note: confirm `Sphere::new(..).mesh().build()` is the right primitive-mesh form for this Bevy version (verified-API doc). If `PointLight` as a child-less sibling is awkward, drop the light and keep the emissive sphere — the slice just needs a visible flame.

- [ ] **Step 2: Flame flicker + repair systems**

```rust
/// Bob/scale the flames so they read as fire (ungated — VFX runs while frozen).
fn flame_flicker(time: Res<Time>, mut q: Query<(&mut Transform, &Flame)>) {
    let t = time.elapsed_secs_wrapped();
    for (mut tf, f) in &mut q {
        let s = 0.8 + (t * 9.0 + f.idx as f32).sin() * 0.18;
        tf.scale = Vec3::splat(s);
    }
}

/// Repair damaged survivors during Prep; extinguish flames once a plot is full HP.
fn repair_system(
    time: Res<Time>,
    siege: Option<Res<crate::siege::Siege>>,
    mut town: ResMut<TownRes>,
    mut commands: Commands,
    flames: Query<(Entity, &Flame)>,
) {
    let prep = siege.map_or(false, |s| s.phase == crate::siege::GamePhase::Prep);
    if !prep {
        return;
    }
    town.0.repair(time.delta_secs() as f64);
    // Despawn flames whose plot is no longer burning.
    for (e, f) in &flames {
        let burning = town.0.plots.get(f.idx).map_or(false, |p| matches!(p.state, tileworld_core::town_store::PlotState::Built { burning: true, .. }));
        if !burning {
            commands.entity(e).try_despawn();
        }
    }
}
```

Register them in `TownPlugin::build`:

```rust
            // Sim (gated): apply damage + repair only while playing.
            .add_systems(
                Update,
                (apply_building_damage, repair_system).run_if(in_state(Modal::None)),
            )
            // VFX (ungated): flames flicker even when frozen.
            .add_systems(Update, flame_flicker)
```

- [ ] **Step 3: Build + manual verify the burn loop**

Run: `cargo check` → PASS.
Run (PowerShell): `$env:FOREST_WAVE="1"; cargo run` — during the wave, ~half the invaders peel off toward buildings; a battered building shows flames and stops producing; if it drops to 0 it collapses (mesh gone, plot is rubble). Kill the attackers to save it; at dawn (Prep) survivors heal and flames go out. Walk onto a rubble plot → E → rebuild.

- [ ] **Step 4: Commit**

```bash
git add src/town.rs
git commit -m "town: burn buildings (fire VFX + collapse to rubble) and repair by day"
```

---

## Phase 8 — Screenshot hook + final verification

### Task 12: `FOREST_TOWN` capture hook

**Files:**
- Modify: `src/town.rs`

- [ ] **Step 1: Stage a built/burning town on demand**

Add a `Startup`-ish system that, when `FOREST_TOWN` is set, pre-builds a couple of plots (and ignites one when `FOREST_TOWN=burn`). It must run after plots are seeded — gate it to run once the `PlotSpots` resource is populated.

```rust
fn stage_town_for_shot(
    mut done: Local<bool>,
    spots: Res<PlotSpots>,
    mut town: ResMut<TownRes>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *done || spots.0.is_empty() {
        return;
    }
    let Ok(mode) = std::env::var("FOREST_TOWN") else { *done = true; return };
    *done = true;
    bank.0.add_wood(100.0);
    bank.0.add_stone(100.0);
    town.0.build(0, BuildKind::Farm, &mut bank.0);
    town.0.build(1, BuildKind::House, &mut bank.0);
    town.0.build(2, BuildKind::Farm, &mut bank.0);
    for idx in [0usize, 1, 2] {
        spawn_building_mesh(&mut commands, &mut meshes, &mut materials, idx, town.0.plots[idx].kind.unwrap(), &spots);
    }
    if mode == "burn" {
        town.0.damage(0, 20.0);
        spawn_flame(&mut commands, &mut meshes, &mut materials, 0, &spots);
    }
}
```

Register it ungated (so it works on the screenshot boot path):

```rust
            .add_systems(Update, stage_town_for_shot)
```

- [ ] **Step 2: Capture the shots**

Run (PowerShell):
```powershell
$env:FOREST_SHOT="shot_town.png"; $env:FOREST_TOWN="1"; cargo run
$env:FOREST_SHOT="shot_burn.png"; $env:FOREST_TOWN="burn"; cargo run
```
Expected: `shot_town.png` shows built farms + a house on the plots; `shot_burn.png` shows one ablaze.

- [ ] **Step 3: Commit**

```bash
git add src/town.rs
git commit -m "town: FOREST_TOWN screenshot hook (staged built/burning town)"
```

### Task 13: Full verification pass

- [ ] **Step 1: Core tests green**

Run: `cargo test -p tileworld_core`
Expected: PASS (existing suite + new resource_store + town_store tests).

- [ ] **Step 2: Full build clean**

Run: `cargo check`
Expected: PASS with no errors. Resolve any warnings you introduced (unused imports, dead code) or justify them.

- [ ] **Step 3: Manual end-to-end play test**

Run: `cargo run` and walk the full loop:
1. Day: build a Farm (wood drops), a worker posts up, Food climbs.
2. Build a House; let Food hit 20 → a villager spawns, heir count +1.
3. Ring the bell / wait for night: arsonist orks burn a building (flames, production halts).
4. Defend: kill attackers before a building collapses; let one collapse → rubble.
5. Dawn: survivors repair, flames out; rebuild the rubble plot.

Confirm no panics through the despawn-heavy burn/collapse/wave-clear races (the repo's known hazard — all building/flame despawns use `try_despawn`/`try_insert`/`try_remove`).

- [ ] **Step 4: Update docs**

Add the new controls/loop to `CLAUDE.md` (the Controls section): **E** on a plot → Build menu; mention food/wood resources and the burn/repair loop. Keep it one or two lines, consistent with the existing terse style.

```bash
git add CLAUDE.md
git commit -m "docs: note the city-building build menu + food/wood in CLAUDE.md"
```

- [ ] **Step 5: Finish the branch**

Invoke the `superpowers:finishing-a-development-branch` skill to choose merge/PR/cleanup.

---

## Self-Review notes (for the executor)

- **Spec coverage:** every spec section maps to a task — data model (Tasks 1–2), plots & build (Tasks 5–7), workers & production (Tasks 8–9), population loop (Task 9), night burn & repair (Tasks 10–11), HUD (Task 9), capture hook (Task 12). Building catalog beyond Farm+House is explicitly **pass 2** (out of this slice, per the spec).
- **Type consistency:** core method names used downstream — `Town::build`, `production_tick`, `population_tick`, `damage`, `repair`, `pop_cap`, `is_buildable`, `is_built`; resource accessors `food()/wood()/add_food/spend_food/...`; Bevy types `TownRes`, `PlotSpots`, `BuildPlot`, `BuildingMesh`, `Worker{idx,at_post}`, `PendingBuildingDamage`, `BuildTarget`. These names are used identically across tasks.
- **Open questions resolved in-plan:** starting plots = 8, all pre-unlocked (slice); upkeep = flat `UPKEEP_PER_POP`; producers do **not** register collision (off-lane in safe-zone); auto-repair is free-over-time during Prep. Revisit in pass 2 if balance needs it.
- **Burn feel = FORGIVING (decided):** slow burn (`BUILDING_DPS = 5.0`), only ~1/3 arsonists, and the keep's existing defenses already shoot arsonists for free. You can save most buildings; a loss reads as a mistake, not bad luck. This is the slice's most important tuning — the whole "is it fun?" verdict rides on the town-burns-while-you-fight moment not feeling unfair. Dial tension UP (more arsonists / higher DPS) only after the loop proves fun. Numbers sit in `town.rs`/`town_store` consts + the F1 panel for fast iteration.
- **API caveats flagged inline** (confirm against the verified Bevy doc / real source): the mesh tint helper name (`props::tinted`), `Mesh::merge` return shape, `Sphere/Cuboid …mesh().build()` forms, and the HUD query disjointness (`Without<...>` filters). These are the spots most likely to differ from the guessed form — check `src/villagers.rs`/`src/castle.rs`/`src/props.rs` and `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md`.
