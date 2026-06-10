# D: Tileworld — Bevy

A **Bevy 0.18** port of the TypeScript/three.js game **"D: Tileworld"**. A knight defends a
central castle against night-wave ork sieges across a five-biome island: real-time combat,
an economy, an upgrade tree, inventory, villagers, a bloodline succession loop, and wildlife —
at near-parity with the original game, wrapped in forest's own (better) lighting and
post-processing.

> This started as a static forest-scene viewer and grew into the full game. If you find a doc
> claiming "no player, no gameplay, just the scene", it predates the port — trust the code and
> `docs/superpowers/specs/`.

## Run

```bash
cargo run        # build + open the game window
cargo test       # run the crates/core parity tests (the validation spec)
cargo check      # type-check without the (slow) link
```

On a fresh Linux box you need Bevy's system libraries once before the first build:

```bash
sudo apt-get update && sudo apt-get install -y \
  libwayland-dev libxkbcommon-dev libudev-dev libasound2-dev
```

(macOS / Windows need none of this.) The first build is slow — `bevy` compiles at
`opt-level = 3` even in dev so the scene isn't single-digit FPS — but incremental rebuilds of
`src/` are fast.

## Controls

- **WASD / arrows** move · **Space** jump · **Shift** sprint · **LMB** attack · **RMB** block
- **E** contextual interact — walk up to the keep (upgrades), a merchant stall (shop), or the
  war bell (ring in the night) and a prompt names it
- **I** satchel · **Q/Z/X/C** quick-bar items · **F** open chest / forage / rescue · **R** recruit
- **` (backquote)** toggle free-roam fly-cam ↔ follow-cam · **P / Esc** pause
- **F1** debug egui tuning panel · **F2** perf/state stats overlay · **1–5** swap biome patch
- Fly-cam: **Space / Ctrl** up·down, **Shift** sprint, hold **Right-Mouse** to look

## Architecture

Two crates:

- **`crates/core` (`tileworld_core`)** — pure, deterministic, zero-dependency game logic
  (`f64` throughout to match JS `number` semantics). Pathfinding (A*), the wave director,
  upgrade / buff / resource / inventory stores, ork & animal config, factions, RNG, the shop
  catalog. This is the unit-tested validation spec — `cargo test` runs it. No Bevy, no I/O,
  no rendering.
- **`tileworld_bevy_forest` (root `src/`)** — the Bevy app: rendering, ECS systems, input, the
  scene. It imports `tileworld_core` for all the numbers/logic that must stay parity-correct,
  wrapping core's stores as Resources (`PlayerRes`, `Bank`, `Inventory`, …).

Each `src/<feature>.rs` is a self-contained `Plugin`; **`main.rs` is the assembly list** — read
it as the table of contents (every plugin line has a one-line description of what it owns). The
whole world-sim is gated behind a freeze-gate state machine (`game_state.rs`): opening any
panel or leaving `Playing` freezes the sim but keeps rendering.

See **`CLAUDE.md`** for the full conventions (coordinate frame, despawn-race rules, mesh-building
contract, combat-number parity) and **`docs/superpowers/specs/`** for the per-subsystem roadmap
and design docs.

## Screenshot harness

The Bevy window can't be captured externally, so visuals are verified via a render-and-exit
harness:

```bash
# PowerShell
$env:FOREST_SHOT="shot.png"; cargo run
# bash
FOREST_SHOT=shot.png cargo run
```

It renders ~90 frames so lighting/IBL settle, saves the PNG, and exits. Stage the shot with
env vars read at startup: `FOREST_CAM` / `FOREST_TIME` (camera pose / time-of-day),
`FOREST_BIOME` (boot into a biome), `FOREST_WAVE` / `FOREST_DEFEND=1` (stage a night siege),
`FOREST_MENU=1` (start screen), `FOREST_PANEL=tree|inv|shop` (open a UI panel),
`FOREST_EQUIP="sword_gold,gold_armor"` (equip items on the hero). `BEVY_ASSET_ROOT` points at
this dir if you run the binary from elsewhere (WGSL loads from `assets/shaders/`).


<img width="1905" height="992" alt="image" src="https://github.com/user-attachments/assets/9b80b27f-688b-4f4d-b13d-7919d156d598" />
<img width="1907" height="999" alt="image" src="https://github.com/user-attachments/assets/b699a1cc-7df4-47c3-ab31-1b5a8fd670c6" />
<img width="1914" height="1005" alt="image" src="https://github.com/user-attachments/assets/3d6e62b8-6159-4055-9e25-6a1dc361f8e3" />
<img width="1913" height="1004" alt="image" src="https://github.com/user-attachments/assets/09f16620-9098-4f3d-9eda-9a4e4694cad7" />
<img width="1919" height="1010" alt="image" src="https://github.com/user-attachments/assets/3078fe5a-720f-44f7-80d3-8ffcf24e2da7" />
<img width="1914" height="1004" alt="image" src="https://github.com/user-attachments/assets/c76da516-d9ee-4776-9507-b6a5b3ca9b78" />
<img width="1909" height="1010" alt="image" src="https://github.com/user-attachments/assets/445809c9-5443-4e2c-857f-f8b3f228a9bc" />

