# In-process world reset (no relaunch) — design

**Date:** 2026-06-16
**Status:** approved (brainstorming) → ready for implementation plan

## Problem

New Game / Restart / Play Again currently **relaunch the process** (`game_state::do_process_restart`
spawns a fresh `current_exe` carrying window geometry + a `WARBELL_RESTART` boot intent, then exits).
The window visibly closes and reopens — bad UX (taskbar flash, second-window race, OS-default spawn
position fought with `WARBELL_WINGEO`).

The relaunch exists because the island and everything destructible (terrain, trees, ore, treasure
chests, town buildings, rescue camps, the Blight fortress, the hero) is built once by ~33 scattered
`Startup` systems with a fixed seed, and there is **no in-process path to regenerate a *fresh*
world**. Continue already resets in-process — but only because it *keeps* existing geometry and just
rolls run-state resources back to the saved dawn + sweeps the battlefield.

A fresh New Game must regenerate world **geometry** (re-grow chopped trees, close opened chests,
delete built houses, restore looted ore), not just reset resources. The only clean way to do that
in-process is **despawn the live world and re-run the build logic** — the fixed seed makes the
rebuild identical to a cold boot.

## Goals

- New Game / Restart / Play Again reset the **entire** run in-process, keeping the same window.
- The fresh world is identical to a cold-boot world (fixed seed → deterministic).
- A loading veil covers the rebuild hitch. (The veil is currently broken — fix it and reuse it.)
- Remove the relaunch machinery once in-process reset works.

## Non-goals

- Changing Continue/Load (already in-process and correct).
- Multi-slot saves, async/streamed world-gen, or splitting the rebuild across frames (one-frame
  rebuild behind the veil is acceptable).
- Reworking the AppState/Modal state machine shape (rejected Approach 2 below).

## Approaches considered

1. **Despawn-and-rebuild via a re-runnable `WorldBuild` schedule (CHOSEN).** World-spawning systems
   move from `Startup` into a custom `WorldBuild` schedule; boot runs it once, reset re-runs it.
2. **`InRun` wrapper state, build `OnEnter` / despawn `OnExit`.** Idiomatic state-scoping, but forces
   a large AppState refactor (Pause + panels must become sub-states that don't exit the run) and
   `StateScoped` on thousands of entities. More invasive, same risk class. **Rejected.**
3. **Per-subsystem reverse operations** (regrow trees, close chests, delete buildings…). No schedule
   refactor, but fragile and high-maintenance — every future destructible needs a matching reset, and
   a miss means "not actually fresh." **Rejected.**

## Design (Approach 1)

### a. `WorldBuild` schedule — single source of truth for world generation

Define a custom schedule label `WorldBuild`. Every world-building system currently in `Startup` that
spawns run-tied geometry/entities is registered in `WorldBuild` instead (mechanical change:
`add_systems(Startup, build_x)` → `add_systems(WorldBuild, build_x)`, **preserving existing
`.after()`/`.chain()` ordering** — worldmap must build before consumers that read it, etc.).

One tiny `Startup` system runs the schedule once at boot: `world.run_schedule(WorldBuild)`.

**Classification (world vs. chrome) — to be finalized per-system in the plan.** Rough split of the
33 `Startup` registrations:

- **World (→ `WorldBuild`):** `scene`, `worldmap`/terrain, `trees`, `defenses`, `town` buildings,
  `ork_fortress`, `orbs`, `banner`, `siege` spawns, `villagers`, chests, landmarks, `nightsky`,
  `grade`, player spawn, `interaction` markers, `aftermath`/`combat_fx`/`footstep_fx`/`projectile`/
  `succession_fx` (verify: some of these may just init resources/pools — those can stay if they're
  idempotent and not tied to a run).
- **Chrome / persistent (stay in `Startup`):** `hud`, `ui/icons`, `ui/focus`, `ui/notice`,
  `ui/settings`, `quality`, `loading`, `subtitles`, `hints`, `capture`, `savegame` (detect-existing),
  `game_state` (boot routing — simplified once relaunch is removed).

Each system is classified explicitly in the plan with a one-line reason.

### b. Despawn predicate — keep-list

Persistent entities are **few and enumerable**; world entities are **many**. So the reset keeps a
small allow-list and despawns the rest, rather than tagging every world entity.

On reset, despawn every **root** entity (no `ChildOf`) **except**:
- entities with a `Camera` (3D + any UI camera),
- entities with a UI `Node` (the persistent HUD/tracker roots — `NoticeRoot`, `QuestRoot`,
  `HintRoot`, `ToastRoot`, `TipRoot`, `PromptRoot`, `BuffRoot`, `BossBarRoot`, the HUD root, etc.;
  state-driven menu UI re-spawns itself via `OnEnter` and isn't present mid-run anyway),
- entities marked with a new `Persistent` component — the explicit escape hatch for non-UI things
  that must survive (audio sinks, gizmo/config entities, the loading veil).

Children despawn with their roots. This deletes the 3D world (terrain, trees, props, hero, orks,
chests, buildings, lights, particles) and leaves chrome + cameras intact.

### c. `rebuild_world` reset routine

A `RebuildWorld` request (resource flag or message). When set, an exclusive system on the next frame:

1. Reset run-state **resources** to fresh-boot defaults — reuse the existing
   `OnExit(StartScreen)`/`OnExit(GameOver)` reset logic where it already exists (economy, quests,
   player, etc.); for the rest set defaults directly. Resources in scope: `Siege` (wave_index = -1,
   phase = Prep, chosen `Difficulty`), `KeepHp`, `PlayerRes`, `Bank`, `Inventory`, `TownRes`,
   `Upgrades`, `Defenses`, `EconomyState`, `Lives`, `RescuedCamps`, `BlightCaptives`, `Discoveries`,
   `QuestLogRes`. (Cross-check against `SaveCtx::snapshot()`'s field list — the save snapshot is the
   authoritative inventory of run-state, so every field it reads must be reset here.)
2. Despawn per (b).
3. `world.run_schedule(WorldBuild)` to regenerate the world.

All behind the veil (raised the frame before, faded after).

### d. Loading veil — fix + make re-raisable

The veil (`src/loading.rs`) is currently a one-shot: it spawns at `Startup`, fades, and **despawns**
— so it can't cover a later reset, and its ready-check keys off `BiomeEntity` (which the playable
island may not set, making boot reveal rely on the 8 s `MAX_WAIT` fallback).

Changes:
- Drive the veil from a `Veil { active, fade }` resource (or equivalent) that any code can raise,
  instead of spawning/despawning a one-shot entity. The veil node persists (hidden when faded out)
  and is marked `Persistent` so the reset despawn never removes it.
- Fix the boot ready-signal: reveal when "world built + fonts loaded" using a robust signal (e.g. a
  `WorldBuilt` flag set at the end of the `WorldBuild` run, instead of probing `BiomeEntity`).
- Reset path raises the veil → rebuild runs next frame → veil fades out.

### e. Delete the relaunch path

Once (a)–(d) work, remove `RestartProcess`, `BootIntent`, `ENV_RESTART`, `ENV_WINGEO`,
`do_process_restart`, `apply_boot_intent`, `window_geometry`, and the `main.rs` window-geometry
parse. Route New Game (`begin_new_game`'s run-active branch), pause **Restart** (`pause_click`), and
game-over **Play Again** (`gameover_click`/`gameover_input`) through `RebuildWorld`. Cold-boot New
Game (world already fresh) can also just use `RebuildWorld` for one uniform path, or keep its current
in-process start. Continue/Load stays exactly as-is.

### f. CLAUDE.md

Update the save/reset convention so the two-sided invariant names the real symbols once they exist:
persist via `savegame.rs`, **and** reset via the reset systems / `WorldBuild` rebuild. (The rule was
strengthened in this change; finalize the symbol names in the plan's last step.)

## Risks & mitigations

- **System classification.** Getting the world-vs-chrome split wrong → either a duplicated chrome
  entity (if a UI system lands in `WorldBuild`) or a leaked/un-rebuilt world entity. *Mitigation:*
  enumerate every one of the 33 systems in the plan with a reason; verify by doing a reset and
  diffing entity counts before/after against a cold boot.
- **Idempotency of re-running build systems.** `Local` state persists across schedule runs; resource
  re-inserts; ordering. *Mitigation:* audit each `WorldBuild` system for run-once `Local`s and
  cross-system resource reads; rely on resource resets in step (c.1) happening before the rebuild.
- **Asset growth.** Despawned entities' mesh/material handles aren't auto-freed; small memory growth
  per reset. *Mitigation:* acceptable for a player-initiated action; note it. Revisit only if resets
  are spammed.
- **Keep-list completeness.** A missed persistent entity gets deleted on reset. *Mitigation:* the set
  is small and well-known; the `Persistent` marker is the explicit opt-in; test that audio/cameras/
  HUD survive a reset.
- **Determinism drift.** If any build system reads wall-clock/`Time` or a non-seeded RNG, the rebuilt
  world differs from boot. *Mitigation:* the world is already fixed-seed (`mulberry32`); audit for
  time/RNG reads during classification.

## Verification

- Manual: New Game / Restart / Play Again — window stays, veil covers the hitch, fresh world (trees
  back, chests closed, no built houses, hero at start, day 1).
- Continue still works (resumes the saved world unchanged).
- Entity-count / state diff: a reset-produced world matches a cold-boot world (spot-check key
  subsystems: tree count, chest count/closed, town plots empty, siege wave_index = -1).
- Veil: shows on boot and reveals correctly (no 8 s hang); shows on reset and fades.
- `cargo test` (core parity unchanged); `cargo run` smoke.
