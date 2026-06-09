# Save / Continue — dawn autosave + resume

**Date:** 2026-06-09
**Status:** approved, implementing
**Scope:** one-slot autosave at every dawn (cleared night); resume a run after defeat or after
quitting. Start screen and the game-over screen gain **Continue** / **New Game**.

## Goal (from the user)

> Game saving like the old game: after every night the game is saved. If the player fails he can
> restart after the last night. The game is saved, and on the start screen he can continue the old
> game or start a new one.

Decisions locked in brainstorming:
- **Autosave at dawn, one slot.** Overwritten automatically each night survived. No history, no
  manual mid-day save, no multiple slots.
- **Fidelity = progression + world flags.** Hero/economy/town progress AND world flags (looted
  treasure chests stay empty, rescued camps stay rescued, discovered landmarks stay discovered).
- **Serialization:** an *optional* `serde` feature on the zero-dep `tileworld_core` crate. `cargo
  test -p tileworld_core` stays dep-free (feature off). Forest enables it + adds `serde_json`.

## Key facts about the existing code (load-bearing)

- **The world is built once at `Startup` and is persistent.** Restarting a run (today's "Play
  Again") does **not** rebuild the island; the per-plugin `OnExit(GameOver)` / `OnExit(StartScreen)`
  reset systems only reset *resources* (and reap a few stale entities like building meshes/graves).
  So chest / landmark / camp entities live for the whole process. **There is no world rebuild to
  wait on** — a load just overwrites resources and marks the already-spawned entities.
- **Run-state lives in resources:** `PlayerRes(core::Player)`, `Bank(core::ResourceState)`,
  `Inventory(core::Bag)`, `TownRes(core::Town)`, `Upgrades(core::UpgradeState)`, `Defenses`,
  `EconomyState`, `KeepHp`, `Siege` (`wave_index`, `difficulty`), `Lives` (heirs),
  `RescuedCamps`, `Discoveries`.
- **The night-clear boundary** is the `Wave → Prep` phase transition in
  `step_wave_director` (siege.rs). At that instant no wave orks are alive and the hero is mid-map —
  a clean snapshot point.
- **Purchased upgrades enact effects** into `Player`, `KeepHp`, `Defenses`, `EconomyState` at
  purchase time (`economy::apply_effect`). `Player`/`KeepHp` are snapshotted wholesale, so their
  upgrade bonuses ride along. `Defenses` and the `EconomyState` flags (`houses`/`farm`/`tax_office`
  /`shop_discount`) are **also** snapshotted so walls/towers/etc. survive a load.
  `EconomyState.unlocked_weapons` is `Vec<&'static str>` (can't deserialize a `'static` borrow), so
  it is **re-derived** on load by scanning the restored purchased upgrades for `UnlockWeapon(id)`.
- **`UpgradeState.purchased` is `Vec<&'static str>`** — same `'static` problem. It is **not**
  serde-derived. The save stores the purchased **id strings** (`Vec<String>`); on load a new core
  helper `UpgradeState::restore(&[String])` resolves each id back to its `&'static` catalog id.
- **Town building meshes are spawned at build time, not reconciled from the resource.** Restoring a
  `Town` with `Built` plots therefore needs an explicit pass that (re)spawns a `BuildingMesh` for
  each built plot — reusing `town::spawn_building_mesh`.

## Architecture

A new self-contained plugin **`src/savegame.rs` (`SaveGamePlugin`)** owns the save file format, the
file I/O, *when* to save/load, and the start/game-over button wiring. Per-subsystem entity
reconciliation that needs a module's private helpers (town meshes) is done by a small system **in
that module**, triggered by a `GameLoaded` message.

### Save file

- Format: JSON via `serde_json` (human-readable, easy to debug).
- Location: `dirs`-free — `%APPDATA%\tileworld\save.json` on Windows
  (`std::env::var("APPDATA")`), `$XDG_DATA_HOME`/`~/.local/share/tileworld/save.json` elsewhere,
  with a `./tileworld-save.json` CWD fallback if no home dir is resolvable. One fixed file.
- `version: u32` header. On load, a mismatched/garbage/missing file is treated as **no save**
  (logged, not fatal). Bump `version` on any breaking format change.

### `SaveData` (serde `Serialize`/`Deserialize`, defined in savegame.rs)

```
version: u32
// run progress
difficulty: Difficulty          // siege enum, serde-derived
wave_index: i32                 // = nights cleared - 1 (the just-cleared night)
keep_hp: f32, keep_max: f32
heirs: u32
// core stores (serde via the core `serde` feature)
player: core::Player
bank:   core::ResourceState
bag:    core::Bag
town:   core::Town
// economy / defense (re-derive unlocked_weapons from `upgrades`)
upgrades: Vec<String>           // purchased node ids
defenses: Defenses              // forest resource, serde-derived
houses: u32, farm: bool, tax_office: bool, shop_discount: f32
// world flags
rescued_camps: Vec<bool>
discoveries_found: u32, discoveries_completed: bool
discovered_landmarks: Vec<bool> // indexed by LandmarkId
opened_chests: Vec<bool>        // indexed by ChestId; treasure chests only (caches respawn)
```

### core changes (behind `feature = "serde"`)

- `Cargo.toml`: `[features] serde = ["dep:serde"]`, optional `serde` dep with `derive`.
- `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]` on: `player::Player`,
  `resource_store::ResourceState`, `inventory::{Bag, Slot}`, `town_store::{Town, Plot, PlotState,
  BuildKind}`.
- `UpgradeState::restore(ids: &[String]) -> UpgradeState` (always compiled): resolves each id via
  `node_by_id` and records the `&'static` id; unknown ids skipped. Unit-tested (round-trips
  `purchased()`).

### Flow 1 — autosave at dawn

A savegame system tracks the previous `Siege.phase`. On a `Wave → Prep` edge with `wave_index >= 0`
(a real cleared night, not the initial Prep), it captures `SaveData` from all the resources +
the chest/landmark/camp flags, serializes, and writes the file. One slot, overwritten. Failure to
write is logged, never panics. (Capture runs after the keep's dawn-repair so the saved keep HP
matches what the player sees at dawn.)

### Flow 2 — Continue

- **Boot:** a `SaveExists(bool)` resource is set at startup from "file present and version-valid".
- **Start screen** (`game_state.rs`): show **Continue** (only when `SaveExists`) and **New Game**.
  **Game-over screen:** **Continue from last night** (only when `SaveExists` and the run was a
  Defeat, not a Victory) and **New Game**. Victory hides Continue.
- **New Game** = today's path, unchanged (the existing resets produce a fresh run).
- **Continue** = read the file into `PendingLoad(Option<SaveData>)`, then `set(AppState::Playing)`.
- **Apply:** the existing per-plugin resets run first (they default everything), then a savegame
  `apply_pending_load` system (Update, guarded by `PendingLoad.is_some()`) overwrites every resource
  from the snapshot, marks chest/landmark entities, sets `RescuedCamps`/`Discoveries`, repositions
  nothing (dawn hero-at-gate from the reset is fine), then emits a `GameLoaded` message and clears
  `PendingLoad`. It is idempotent and order-independent because all target resources and all world
  entities already exist (persistent world). `town::restore_buildings` listens for `GameLoaded` and
  reconciles `BuildingMesh` entities to the restored `TownRes` (despawn stale, spawn one per built
  plot via `spawn_building_mesh`).

### Entity keying (so flags re-apply to the right deterministic entity)

- `ChestId(usize)` component added at chest spawn (incrementing over the spawn loop). Only
  `cache == false` treasure chests are persisted; caches respawn on their own.
- `LandmarkId(usize)` component added at landmark spawn. `discovered_landmarks[id]` re-marks the
  `Landmark.discovered` field via a new `pub(crate)` setter.
- `RescuedCamps` (`done`/`seen` `Vec<bool>`) made `pub(crate)` (or given accessors) so savegame can
  snapshot/restore it.

## Testing

- **core (`cargo test -p tileworld_core`, the parity gate):** with `--features serde`, a round-trip
  test per derived struct (`Player`, `ResourceState`, `Bag`, `Town`) — serialize → deserialize →
  `assert_eq!`. `UpgradeState::restore` round-trips `purchased()`. Default (no feature) build still
  compiles and passes.
- **savegame:** a `SaveData` JSON round-trip unit test (serialize → deserialize → `assert_eq!` on a
  populated value). The capture/apply ECS systems are validated manually via a play-through
  (verification gate below) — they need the live `App` and are not unit-tested.

## Verification

`cargo test` (core gate) + `cargo check` (forest) green, then a manual play-through: survive night
1 → confirm `save.json` written; die → **Continue from last night** restores hero level/gold/stone,
upgrades (a built wall stays), a built farm's mesh reappears, a looted chest stays open, night
counter resumes at 2's prep. Quit and relaunch → start screen shows **Continue** and resumes the
same state.

## Out of scope (per the locked decisions)

Multiple save slots, save history / rollback to arbitrary nights, manual mid-day save, mid-combat
(mid-wave) snapshots, cloud sync.
