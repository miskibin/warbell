# Quest System — Design (2026-06-16)

## What this is

A **guided tutorial quest chain**: a linear sequence of objectives that teaches a new
player the core day loop (gather → build → feed → hunt → upgrade → survive) by *doing it*,
not by reading a help panel. It complements the existing passive teaching layers:

- `tutorial.rs` — the **H** "How to Play" reference panel (read on demand).
- `hints.rs` — bottom-right affordance toasts ("you can afford an upgrade").

Quests are the **active** layer: one objective at a time, an always-on tracker, and a
click-out card that visually explains the mechanic. The data model is built so real
(non-tutorial) quests can be added later, but the first shipment is onboarding only.

Quest *titles* are plain and imperative ("Gather Wood"). The motivational framing lives in
the body copy ("Timber is the bones of every building…"), per the request — the player is
told **why**, not "press LMB".

## The chain (linear, 7 quests)

| # | id | Title | Objective | Reward |
|---|----|-------|-----------|--------|
| 0 | `gather_wood` | Gather Wood | gather ≥12 wood (chopped) | +5 gold |
| 1 | `build_farm` | Build a Farm | a Farm is built | +6 wood |
| 2 | `gather_stone` | Gather Stone | gather ≥8 stone (mined) | +5 gold |
| 3 | `build_house` | Build a House | houses increase | +12 gold |
| 4 | `hunt_food` | Hunt for Food | kill 1 wild animal | 2× bread |
| 5 | `war_table` | Open the War Table | open the upgrade tree once | +6 stone, +5 gold |
| 6 | `survive_night` | Survive the Night | survive night one | +25 gold |

Targets/rewards are **data** and tunable. They're sized so each quest leaves enough to do
the next build (the starting 16-wood stipend covers the farm; gather targets + rewards bridge
the house's 12 wood + 8 stone). When the chain finishes, the tracker hides and a one-time
"Your hold stands" notice fires.

**Out-of-order is honored.** "Build" / "open" / "survive" objectives complete from current
world state the moment they become active (e.g. a farm built early auto-completes the farm
quest when the chain reaches it). Gather objectives count wood/stone *gained while active*
(frame-to-frame bank increases), so the starting stipend never counts and spending never
un-progresses.

## Architecture

### `crates/core/src/quest.rs` (pure, tested)

Bevy-free, serde-gated, unit-tested.

- `Objective` enum: `GatherWood(f64)`, `GatherStone(f64)`, `BuildFarm`, `BuildHouse`,
  `HuntAnimal(u32)`, `OpenWarTable`, `SurviveNight`. Helpers `target()` and `is_metered()`.
- `Signal` enum: the abstract engine facts the Bevy layer reports —
  `WoodGained(f64)`, `StoneGained(f64)`, `FarmBuilt`, `HouseBuilt`, `AnimalHunted`,
  `WarTableOpened`, `NightSurvived`.
- `Reward { gold, wood, stone, item: Option<(&'static str, i64)> }`.
- `QuestDef { id, title, why, explain, action, icon, objective, reward }` + a static
  `QUESTS` table (the 7 above).
- `QuestLog { active: usize, progress: f64 }` (the per-run state; serde-able, rides the save).
  - `record(Signal) -> Option<usize>`: if the signal matches the active objective, advance
    progress; returns the just-completed quest index when it tips over (so the caller grants
    the reward + celebrates). Non-matching signals are ignored — this is what makes the Bevy
    layer free to fire signals liberally.
  - `current()`, `is_complete()`, `fraction()` for the UI.

Core knows nothing of Bevy `Modal`/`Town`; objectives are abstract and the src layer maps
engine events onto `Signal`s.

### `src/quest.rs` (Bevy plugin)

Wraps `QuestLog` as `QuestLogRes`; owns detection, the tracker, the explain panel, and reward
granting.

- **`QuestSignal(Signal)`** message decouples detection from reward-granting. Tiny detector
  systems emit it; one `apply_quest_signals` system consumes it, calls `record`, and on a
  completion grants the reward (gold/wood/stone via `PlayerRes`/`Bank`, item via `try_grant`),
  pushes a Notice, and plays `AudioCue::LevelUp`.
- **Detectors** (all consult `QuestLog` so they only emit the relevant signal — no flooding):
  - `detect_gather` (Modal::None): wood/stone bank deltas → `WoodGained`/`StoneGained`.
    Baseline (`QuestTracking.prev_*`) is `None`-seeded so the starting stipend and load-jumps
    don't count.
  - `detect_builds` (Modal::None): reads `TownRes` — a built Farm → `FarmBuilt`; houses past
    `START_HOUSES` → `HouseBuilt`.
  - `detect_hunt` (Modal::None): drains `verbs::AnimalKilled` → `AnimalHunted`.
  - `detect_war_table` (`OnEnter(Modal::UpgradeTree)`): → `WarTableOpened`.
  - `detect_survive` (Modal::None): the `Wave→Prep` dawn edge → `NightSurvived` (same edge as
    the autosave).
  - `apply_quest_signals` runs **ungated** so a signal emitted while a panel is open (war
    table) is processed before the 2-frame message buffer drops it.

### UI

- **Tracker** — reuses the `hints.rs` toast card look (dark pill, pulsing gold border,
  `shadow_card`), positioned **right-center** (a full-height absolute column at the right
  edge, `justify_content: center`) so it clears the top resource HUD and the bottom-right
  hints. Shows the active quest's icon + plain title; metered quests add a `x / y` counter +
  thin bar, binary quests show the action hint. The card is a `Button` — click it (or press
  **J**) to open the explainer. Rebuilt each frame; hidden when a panel is open, on menus, or
  when the chain is complete.
- **Explain card** — `Modal::Quest` sub-state (freeze gate, like the H panel but compact &
  centered, ~380px). Title, the "why" body, a "HOW" line with the action, and a reward
  preview. Closes on **J** / **Esc** / ✕.

### Save (`src/savegame.rs`)

Quest progress is cross-run progression → it **must** round-trip the save (CLAUDE.md
invariant).

- `SaveData` gains `#[serde(default)] pub quest: QuestLog` (additive — old saves load and
  default to the start of the chain; no `SAVE_VERSION` bump).
- `SaveCtx` reads `QuestLogRes`; `snapshot()` writes `quest`.
- Restore rides the existing `GameLoaded` message: `quest.rs::restore_quest_log` overwrites
  `QuestLogRes` from the snapshot and re-seeds the gather baseline (so the load's bank jump
  isn't mistaken for gathering). Kept out of `apply_pending_load` to stay clear of its
  system-param ceiling and to follow the "owning module reconciles from GameLoaded" pattern.
- Reset on a fresh run (`OnExit(StartScreen)` / `OnExit(GameOver)`): the chain restarts.

## Keys / state touched

- New key **J** — open the quest explainer (free; verified against the existing bind list).
- New `Modal::Quest` variant in `game_state.rs` (freeze gate, Esc-to-close handled by the
  existing `pause_toggle`).
- New `QuestPlugin` registered in `main.rs`.

## Deliberately out of scope (now)

- Multiple simultaneous / optional quests (the data model allows it; the UI is single-active).
- Real (non-tutorial) quest content, branching, NPC givers.
- A merchant quest — the merchant flow isn't wired yet, so it's omitted; "Hunt for Food"
  takes its slot.
