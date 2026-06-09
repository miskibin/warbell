# City-Building Economy — Design

**Date:** 2026-06-09
**Status:** Design (approved for spec → plan)
**Topic:** Add a city-building / town-economy layer to the castle-defense game.

## One-line

Turn the existing ambient town into a **living, working settlement you build and protect**:
by day you place producer buildings on plots and your villagers staff them to grow food,
resources, and population; by night the orks come to **burn the town**, and you defend and
rebuild. The town is both your economic engine and the stake you fight for.

## Why / fantasy

The game already has the bones of a town — wandering villagers, Districts that grow population,
a gold+stone economy, an upgrade tree, and a night siege — but the player only ever *plays the
knight*. The town is set-dressing. This feature makes the town a **system the player shapes**:
WHAT to build and WHERE, who works it, and whether it survives the night.

Two pillars (from brainstorming):

- **Economic engine** — buildings produce resources over time; resources fund the defense.
- **Living town to protect** — orks target and burn buildings; losing them hurts; you rebuild.

The day↔night siege rhythm already in `siege.rs` becomes the build↔defend rhythm of a
city-builder: **build/grow by day, defend the town by night.**

## Locked decisions (from brainstorming)

| Question | Decision |
|---|---|
| RTS flavor | **City-building**, not unit-command. Player stays the knight; no tactical/command camera. |
| City purpose | **Living town to protect** + **economic engine** (not freeform walls, not a troop-spam source). |
| Placement | **Pre-placed plots you unlock** — curated empty plots in the town/suburb zone; pick a plot, choose a building. Lowest terrain-tech risk. |
| Economy depth | **Flat producers + workers** — each building ticks ONE resource and needs ONE villager assigned; no hauling sim, no chains (yet). |
| Worker assignment | **Auto-assign** — building a producer auto-pulls the nearest idle villager. Manual override is a possible later pass. |
| Night threat | **Burn & repair** — orks attack nearest buildings, fire VFX, HP drops, production halts; HP 0 → rubble (rebuild from plot); saved-but-damaged buildings auto-repair by day. |
| Build/slice strategy | **Vertical slice first (A)** — Farm + House prove the whole loop end-to-end; building catalog fans out in pass 2. |

## Non-goals (explicitly out of scope)

- No production chains / refined goods (raw→refined). Flat producers only.
- No worker hauling / logistics / storage routing.
- No manual worker micromanagement in the slice (auto-assign only).
- No freeform terrain placement, grid-snapping, or terrain leveling. Plots only.
- No tactical/command camera or unit selection. The player still drives one knight.
- No new win/lose condition. Existing keep-falls / bloodline-ends conditions stand; burned
  buildings are a *setback*, not a game-over.

---

## Architecture

Follows the repo's split: **pure tested logic in `crates/core`, rendering/ECS in `src/`.**

### Core (`crates/core`, `f64`, zero-dep, unit-tested)

1. **Extend `resource_store::ResourceState`** — add `food: f64` and `wood: f64` alongside the
   existing `stone`, with `add_*` / `spend_*` mutators mirroring the stone ones (non-positive
   ignored; spend is all-or-nothing). Gold stays on `core::player::Player`. Existing stone tests
   stay green; add food/wood tests.

2. **New `town_store` module** — the pure model of the settlement:

   ```text
   enum BuildKind { Farm, House, /* pass 2: Lumber, Quarry, Market, Granary */ }

   enum PlotState { Empty, Built { hp, burning }, Rubble }

   struct Plot {
       kind: Option<BuildKind>,   // None when Empty/Rubble
       state: PlotState,
       worker: Option<u32>,       // assigned villager id, None = unstaffed
   }

   struct Town {
       plots: Vec<Plot>,
       population: u32,           // total villagers (labor pool)
       food_accum: f64,           // progress toward the next villager
   }
   ```

   Pure functions / methods (all tested):
   - `BuildKind::cost()` → `{ wood, stone }`; `BuildKind::produces()` → `Option<(Resource, per_sec)>`;
     `BuildKind::max_hp()`; `BuildKind::needs_worker()` (House = false).
   - `Town::can_build(plot, kind, &bank, gold)` / `Town::build(...)`.
   - `Town::production_tick(dt, &mut bank)` — each `Built`, non-`burning`, **staffed** producer
     adds `produces * dt` to the bank.
   - `Town::population_tick(dt)` — food upkeep drain (`pop * UPKEEP`), surplus accrues to
     `food_accum`; when it crosses `FOOD_PER_VILLAGER` and a house slot is free, returns a
     "spawn villager" signal and increments `population`.
   - `pop_cap()` = `houses_built * POP_PER_HOUSE`.
   - `Plot::damage(amount)` / `Plot::repair(dt)` / collapse-to-`Rubble` at hp 0.

   This is the parity/validation surface — `cargo test -p tileworld_core` covers the loop math
   (build affordability, production accrual, upkeep, population growth, damage→rubble, repair).

### Bevy (`src/`)

New plugin **`src/town.rs`** (the assembly line in `main.rs` gets one more plugin entry). It
wraps the core `Town` as a Resource and owns the build/produce/burn/repair systems. It leans on
existing modules rather than re-implementing:

- **Plots** are entities tagged `BuildPlot` carrying the core `Plot` index, seeded by
  `worldmap::build` (alongside the existing castle/camp/ore seeding) at flat, walkable suburb
  positions that are **off the invader lanes** (so a building doesn't wall off the gate path).
- **Build interaction** extends `interaction.rs`'s nearest-in-range `E` resolver with a new
  `InteractKind::Build` (when the hero stands on an `Empty`/`Rubble` plot) → opens a new
  `Modal::Build` panel.
- **Build menu** is a new `Modal::Build` variant in `game_state.rs` + a panel built like the
  existing upgrade-tree/shop Modals (`economy.rs` UI is the template): a list of buildings with
  cost + "needs worker" badge, greyed if unaffordable.
- **Worker villagers** reuse the `villagers.rs` `Villager` bodies. Auto-assign: when a producer
  is built (or a worker dies/flees), a system pulls the nearest idle non-guard villager, marks it
  `Worker { plot }`, and steers it to the building; its limb anim plays the existing work/idle.
- **Night burn** extends the `orks.rs` `WaveInvader` AI: a tunable fraction of invaders pick a
  `BuildPlot` target instead of the keep, path to it (via `navgrid`), and attack on contact →
  `Plot::damage`, spawn fire particles (`particles.rs`), and (if it collapses) swap the building
  mesh for a rubble/scorch prop. Workers flee under the existing `townsfolk_curfew`.
- **Repair** runs during `GamePhase::Prep`: damaged survivors tick `Plot::repair`; rubble stays
  until the player rebuilds it from the plot.

All simulation systems carry `.run_if(in_state(Modal::None))` per the freeze-gate convention;
render/anim/fire-VFX systems stay ungated so a frozen world still draws.

---

## The core loop

```
DAY (Prep)                          NIGHT (Wave)
──────────                          ───────────
walk to plot → E → build menu       some orks peel off to nearest building
  pay wood/stone                    attack → Burning, HP↓, production halts
producer auto-pulls a villager      worker flees (curfew)
staffed producer ticks resource     hero / keep defenses kill attackers to save it
food surplus → +1 villager          HP 0 → Rubble
  (up to pop cap = houses)          ───────────
bigger pop → more producers staffed DAWN: wave cleared → +1 heir (existing)
                                    survivors auto-repair, rubble awaits rebuild
```

The tension: **food feeds population, population staffs producers, producers fund defense and
more building — but every building you place is another thing the orks can burn.** Spreading out
vs. clustering, growing fast vs. defending what you have.

---

## MVP scope (the vertical slice)

Ships the full loop with the minimum content needed to exercise every integration seam:

**In the slice:**
- Resources: **food** + **wood** added to `resource_store` (+ existing stone/gold). HUD shows them.
- Buildings: **Farm** (produces food, needs 1 worker, burnable) and **House** (raises pop cap,
  no worker, burnable).
- ~6–10 pre-placed plots in the suburb zone.
- Auto-assign one worker per Farm; worker flees at night.
- Food→population growth (capped by houses); keeps the existing `houses→heir` succession tie.
- Build Modal + `E` plot interaction + plot state prompt (Empty / Built / Burning / Rubble).
- Night: invaders target buildings, burn, collapse to rubble; survivors auto-repair by day.
- Core tests for the whole loop math.

**Pass 2 (designed, not built in the slice):**
- More producers: **Lumber camp** (wood), **Quarry** (stone), **Market** (gold), **Granary**
  (food cap/upkeep buffer), plus flavor (Well, Watchpost).
- Manual worker reassignment panel (if balance needs it).
- Fire *spread* between adjacent buildings (deferred from the "burn & repair" decision).
- Plot *unlocking* progression (start with a few plots, unlock more via upgrades/expansion).

---

## Integration points (where this touches existing systems)

| System | Touch |
|---|---|
| `siege.rs` / `orks.rs` `WaveInvader` AI | New target branch: a fraction of invaders path to a `BuildPlot` and attack it instead of the keep. |
| `villagers.rs` `townsfolk_curfew` | Already hides non-guard villagers at night — workers inherit this (they flee, production halts). |
| `villagers.rs` population / `economy.rs` `EconomyState.houses` | Population becomes a real labor pool; `houses` becomes the pop cap. Keep `houses→heir`. |
| `succession.rs` `Lives` | Unchanged tie: a built House still grants an heir (population growth ≈ household growth). |
| `interaction.rs` | New `InteractKind::Build` in the nearest-in-range `E` resolver. |
| `game_state.rs` `Modal` | New `Modal::Build` substate (freezes the world like other panels). |
| `navgrid.rs` | Plots sit off the invader lanes so buildings never block the gate path; built producers may register a small collision box (route-around), gaps preserved. |
| `hud.rs` | Resource readout gains food/wood; plot/building state prompts. |
| `worldmap::build` | Seeds the `BuildPlot` markers (flat, walkable, off-lane). |
| `particles.rs` / mesh props | Fire VFX on burning buildings; rubble/scorch prop on collapse. |
| `capture.rs` env hooks | Add a `FOREST_TOWN=1` style hook to stage a built/burning town for screenshots. |

## Risks & mitigations

- **Siege seam is the headline risk** — orks attacking buildings touches invader AI, navgrid,
  and despawn-race-prone burn/collapse logic. *Mitigation:* the vertical slice builds exactly
  this seam first, on one building type. Use `try_despawn`/`try_insert` throughout (repo
  convention — burning/collapse races with combat/wave-clear reaping).
- **Plots blocking the gate path** — a building on an invader lane could wall off A\*.
  *Mitigation:* seed plots only off-lane; validate reachability after seeding.
- **Balance (food/upkeep/pop numbers)** — flat-producer economies wobble easily.
  *Mitigation:* numbers live in `town_store` constants, test-gated; tune via the F1 debug panel.
- **Worker churn at night** — workers fleeing/returning each night must not leak entities or
  strand producers unstaffed. *Mitigation:* auto-assign re-runs each dawn; worker link is the
  core `Plot.worker` id, re-resolved on spawn.

## Testing

- **Core:** `cargo test -p tileworld_core` — build affordability, production accrual, food
  upkeep/population growth, pop cap, damage→rubble, repair, edge cases (unstaffed = no output,
  unaffordable = no-op).
- **Visual:** `FOREST_SHOT` + a town-staging env hook to verify built town, burning state, and
  rubble render correctly.
- **Manual:** play a day→night→dawn cycle; confirm build→staff→produce→grow, then
  burn→defend→rubble→rebuild→repair all fire and the world stays stable through despawn races.

## Open questions (resolve during planning, not blocking)

1. Exact starting plot count and whether early plots are pre-unlocked or gated.
2. Food upkeep curve — flat per-capita vs. scaling; the threshold for the first new villager.
3. Whether a built producer registers collision (route-around) or is walkable.
4. Auto-repair: free-over-time vs. wood-fed.
