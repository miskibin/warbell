# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A **Bevy 0.18** port of a TypeScript/three.js game, **"D: Tileworld"**, whose source lives at
`D:\tileworld\src\world` (the canonical reference for gameplay behaviour, tuning constants, and
model meshes — "the old game" / "the original"). This started as a static forest-scene viewer and
has grown into a near-parity playable game: a knight defends a central castle against night-wave
ork sieges, with combat, economy, an upgrade tree, inventory, villagers, succession, and five
biomes on one enlarged island.

> `README.md` and the `main.rs` module doc-comment now describe the actual game (both were
> truthed-up after an earlier era where they still claimed "no player, no gameplay, just the
> scene"). They're fine as entry points, but the per-subsystem **source of truth** is the parity
> roadmap, `docs/superpowers/specs/2026-06-07-tileworld-parity-port-roadmap.md` (numbers + reuse
> pointers). If you spot a code comment still describing the old static-viewer state, it's stale —
> fix it.

## Commands

```bash
cargo run                       # build + open the game window
cargo test                      # runs the crates/core unit tests (~268; the parity validation spec)
cargo test -p tileworld_core <name>   # a single test by substring
cargo check                     # type-check without the (slow) link
```

`bevy` deps compile at `opt-level = 3` even in dev (see `Cargo.toml`) — otherwise the scene runs
at single-digit FPS — while our own crates stay at low opt for fast rebuilds. The first build is
slow; incremental rebuilds of `src/` are fast.

**Multi-agent caveat:** the `CONTRACT*.md` files codify a workflow where several agents each edit
ONE module in parallel against a *shared* `target/` and must **not** run `cargo build/check/run`
(a concurrent build corrupts the shared dir; the integrator builds once at the end). That rule is
**only** for parallel-dispatch sessions. A normal single Claude session should build and verify.

### Git / agent workflow rules (non-negotiable)

- **NEVER `git reset --hard`** (or any destructive history/working-tree reset: `git clean -fd`,
  force-checkout over dirty files, `git checkout -- <path>` that discards work) **without explicit
  per-instance permission.** Blanket prior approval does not carry over — ask each time, naming what
  would be lost.
- **Push your code after you finish a feature.** Once the feature is complete and verified, commit
  and `git push` — don't leave finished work sitting only in the local working tree.
- **Other agents may be working in parallel.** If your changes overlap with another agent's
  in-flight work, you can simply **wait for them to finish** rather than racing or stomping their
  edits.
- **Push back on bad ideas — don't just agree.** If a request is wrong, risky, or has a clearly
  better alternative, say so plainly and explain why before (or instead of) implementing it. Honest
  technical disagreement is wanted, not reflexive compliance.

### Cloud / remote-container setup (Claude Code on the web, fresh containers)

A fresh Linux cloud container has the Rust toolchain but **not** the system libraries Bevy links
against, so the *first* `cargo check`/`build`/`run` panics in a build script with
`Package wayland-client was not found` (or `xkbcommon` / `alsa` / `libudev`). This is an
environment gap, **not** a code problem — don't "fix" it in `Cargo.toml`. Install the headers once
per container (the container is ephemeral, so this must run every fresh session):

```bash
sudo apt-get update && sudo apt-get install -y \
  libwayland-dev libxkbcommon-dev libudev-dev libasound2-dev
```

(These are Bevy's standard Linux deps: Wayland + XKB for windowing/input, ALSA for the `wav`
audio feature, libudev for gamepad enumeration.) After installing, `cargo check` succeeds (the
first build still recompiles all of `bevy` at `opt-level = 3`, ~2–3 min). On developer machines
(macOS / Windows) none of this applies. To make web sessions build without the manual step, put
the `apt-get` line in a **SessionStart hook** (see the `session-start-hook` skill).

### Screenshot harness (how to verify visuals — the Bevy window can't be captured externally)

```powershell
$env:FOREST_SHOT="shot.png"; cargo run      # renders ~90 frames so lighting/IBL settle, saves PNG, exits
```

Env hooks that stage a scene for a shot (combine with `FOREST_SHOT`), all read at startup:

| Var | Effect |
|---|---|
| `FOREST_SHOT=path.png` | capture-and-exit harness (`capture.rs`) |
| `FOREST_CAM`, `FOREST_TIME`/`FOREST_DAY`/`FOREST_NIGHT` | camera pose / time-of-day for the shot |
| `FOREST_HERO="x,z"` | drop the hero at a world XZ (e.g. deep in a biome region) to stage its reactive atmosphere/weather |
| `FOREST_BIOME` | boot straight into a given biome |
| `FOREST_WAVE` / `FOREST_DEFEND=1` | stage a night siege / arm all defenses + walls |
| `FOREST_ORKLINE="x,z"` | park one ork of each variant in an idle line at a world XZ (model close-ups) |
| `FOREST_MENU=1` | shoot the start screen |
| `FOREST_PANEL=tree\|inv` | seed + open the upgrade-tree / satchel panel for a shot |
| `FOREST_EQUIP="sword_gold,gold_armor"` | equip the listed item ids at startup so the hero model shows its weapon/armor |
| `FOREST_FOG="clear,full"` | override fog distances live (no rebuild) |
| `FOREST_AUDIOTEST` / `FOREST_GRADETEST` | isolate audio / reactive-grade for testing |
| `FOREST_FLOATTEST=1` | continuously stage sample floating combat numbers near the hero (style preview) |
| `BEVY_ASSET_ROOT` | point at this dir if running the binary from elsewhere (WGSL loads from `assets/shaders/`) |

## Architecture

### Two crates: pure logic vs. rendering

- **`crates/core` (`tileworld_core`)** — pure, deterministic, **zero-dep** game logic vendored
  from a ditched earlier port. Pathfinding (A*), wave director, upgrade/buff/resource/inventory
  stores, ork config, factions, RNG, shop catalog, etc. **`f64` throughout** to match JS `number`
  semantics for cross-language parity. This is the unit-tested validation spec — `cargo test` runs
  it. It has **no Bevy, no I/O, no rendering**. The Bevy front-end wraps these stores as Resources
  (e.g. `PlayerRes(core::Player)`, `Bank(core::ResourceState)`, `Inventory(core::Bag)`).
- **`tileworld_bevy_forest` (root `src/`)** — the Bevy app: rendering, ECS systems, input, the
  scene. Imports `tileworld_core` for all the numbers/logic that have to stay parity-correct.

When changing gameplay *numbers or rules*, prefer editing `crates/core` (and its tests) so parity
stays test-gated; `src/` should mostly *drive* core, not re-implement it.

### Plugin composition (`src/main.rs`)

Every `src/<feature>.rs` (and `src/player/`, `src/audio/`) is a self-contained `Plugin` that does
its own `Startup` spawn + `Update` systems. `main.rs` is just the assembly list — read it as the
table of contents (each plugin line has a one-line description of what it owns). Plugins are added
in several tuples because the `Plugins` trait maxes out at arity 15.

### The freeze-gate state machine (`src/game_state.rs`)

`AppState` = coarse mode (`StartScreen` / `Playing` / `Paused` / `GameOver`). `Modal` = a substate
that exists **only inside `Playing`** (`None` / `Shop` / `UpgradeTree` / `Inventory`). The entire
world-sim is gated on `run_if(in_state(Modal::None))`: opening any panel — or leaving `Playing` —
freezes the world but keeps rendering. This is the declarative port of the TS `isFrozen()`. Sim
systems carry the gate; render/camera/anim/audio/HUD systems stay ungated so the frozen world
still draws. New simulation systems **must** carry `.run_if(in_state(Modal::None))` or they'll run
through pauses/panels.

### World layout: biome viewer vs. the real island

Two related map systems share the ground pipeline in `biome.rs`:
- **`biome.rs` + `biome_<name>.rs`** — the biome *framework*. Each biome exposes `config()`
  (declarative ground/atmosphere/scatter/particles) + optional `landmarks()`. Keys **1–5** swap a
  single 32×32 biome patch at runtime (despawns everything tagged `BiomeEntity`, rebuilds).
- **`worldmap.rs`** — the actual playable island: a port of the TS `tileMap.ts` at base
  resolution scaled up by `MAP_SCALE = 1.5` (→ `COLS 216 × ROWS 162`). Elliptical island, five
  biome blobs, grass safe-zone (castle), four rivers + lake, coastal mountain ridges, rolling
  terraced knolls, plateaus. `classify` force-flattens grass under every town build plot (and
  chest/cover placement rejects `town::near_build_plot`) so nothing occupies a future building's
  spot. `worldmap::build` also seeds castle/camps/ore/chests placement. **Generation runs in base
  space, drawn over the enlarged grid**, so the island shape is identical, just denser.

### Pathfinding through the castle (`src/navgrid.rs`)

Wraps core's tested A* (`tileworld_core::pathfinding`) onto forest's world-space terrain. Walls
register impassable collision boxes; **gate gaps register none**, so A* threads the gates with no
explicit gate-targeting code. Night-wave invaders follow `InvaderPath` waypoints to the keep.

## Conventions that bite if you miss them

- **Coordinate frame is world-space with the castle at the ORIGIN** (a deliberate divergence from
  the TS game's grid-`-CENTER` origin — do NOT "fix" it). One tile = one world unit;
  `tile = floor(world + G)` where `GX/GZ` (in `worldmap.rs`) recentre the grid onto the origin.
- **Enemy combat numbers are at full old-game / core parity (NOT rescaled).** An earlier pass
  scaled ork HP/damage and wildlife damage *down* (~×0.35 HP, ~×0.5 dmg) for the smaller scene,
  but that left enemies far too soft against a hero already at full old-game power (core
  `PLAYER_BASE_DAMAGE` 25 + weapons/crit/levels). They're now read straight from core:
  `siege::base_hp` → `ork_config(v).hp` (grunt 254 / scout 136 / berserker 306 / shaman 201),
  `orks::ORK_DAMAGE`=24 (variant_melee derives the rest), `SHAMAN_BOLT_DAMAGE`=26, and
  `wildlife::predator_stats` bite damage → `animal_config(s).attack_damage` (wolf 12 … golem 28).
  Wildlife HP already came from core. Per-night HP growth is the `siege::WAVES` `hp_scale` table
  (1.1·1.15^n), unchanged. When porting a new TS/core combat number, use it **as-is** — don't
  reintroduce the old rescale.
- **Despawn races are pervasive** — many systems race to reap the same entity (corpses, kills,
  wave clears). Always use `try_despawn` (not `despawn`) and `try_insert` (not `insert`) on
  entities that combat/AI/HP-bar systems might have already removed. This is load-bearing; bare
  `despawn`/`insert` will panic intermittently.
- **Death is a fade, not a pop** (`src/dying.rs`): kills insert `Dying{since}` and crumple over
  ~1.4s. Every AI/targeting/count query must filter `Without<Dying>` so corpses aren't targeted or
  counted — but reward/clear logic fires once pre-fade.
- **Every voice/quote line carries its spoken text in a code comment.** When you wire up *any*
  spoken line (hero `voice.rs`, villager `audio/npc.rs`, ork `audio/ork.rs`, or future speakers),
  put the exact transcript next to where the clip is loaded / keyed, plus its trigger. The audio
  files are opaque `.ogg`s — these comments are our **only** record of what the game actually says,
  so we can retune *when/how often* lines play without re-listening to every clip. Keep them in
  sync when a clip is re-recorded; if a clip's text is unknown (older un-transcribed asset), say so
  explicitly (`[older clip — text not transcribed]`) rather than leaving it blank.
- **Mesh-building contract** (see `CONTRACT.md`, `CONTRACT2.md`): every prop mesh's base sits at
  `y = 0`; **colour lives in the mesh `ATTRIBUTE_COLOR`** (linear RGBA via `crate::palette::lin` /
  `lin_scaled`), because all props share one white `StandardMaterial` so the renderer auto-batches
  thousands of instances. Build parts as primitives, `tinted()` each (add COLOR) before
  `Mesh::merge`, and `duplicate_vertices()` + `compute_flat_normals()` for the crisp low-poly facet
  look (duplicate FIRST — `compute_flat_normals` panics on an indexed mesh). The verified Bevy
  0.18.1 API forms are in `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md`; the
  per-slice spec docs sometimes **guess wrong** about the Bevy API — trust the verified doc + real
  Bevy source under `C:\Users\skibi\.cargo\registry\src\index.crates.io-*\bevy_*-0.18.1\src`.
- **Determinism**: scatter/placement uses `mulberry32` (core `rng.rs`) seeded per-tile, so the
  world is reproducible. "Feels the same" parity, not byte-exact RNG/map.
- **Forest's divergences are canonical, not bugs**: `siege.rs`'s wave director is richer than
  core's `wave.rs` and is the one in use; `KEEP_MAX_HP = 1000` / 12-per-s repair; the bespoke
  `Atmosphere`/IBL lighting; the custom CoC bokeh `dof.rs` over a plain DoF. Don't "restore" these
  to match core/TS.

## Reference material

- `docs/superpowers/specs/2026-06-07-tileworld-parity-port-roadmap.md` — the P0–P6 roadmap + locked
  decisions + per-subsystem numbers. Read this before any gameplay-parity work.
- `docs/specs/` — extracted TS visual specs + the **verified** Bevy 0.18.1 API doc.
- `docs/superpowers/specs/*-design.md` — per-feature design docs (audio, hero, combat feedback,
  shaman spells, biome variety).
- `D:\tileworld\src\world` — the original TS game (canonical gameplay/tuning/mesh reference). Key
  files: `orkConfig.ts`, `Ork.tsx`, `projectileStore.ts`, `playerStore.ts`/`Character.tsx`,
  `combatStore.ts`, `tileMap.ts`.

## Controls (gameplay)

` (backquote) toggle free-roam fly-cam ↔ follow-cam · WASD move · LMB attack · RMB block ·
F1 debug egui tuning panel · F2 perf/state stats overlay · **E** contextual interact — walk up to a thing and a screen prompt names it:
near the **keep** → War Table (upgrades), near the **merchant stall** → shop, near the **war bell**
(prep only) → ring in the night (the unified resolver lives in `interaction.rs`; nearest in-range
wins, proximity-only/no-facing, ported from the 3js single-`E` scheme) · **I** Satchel ·
**Q/Z/X/C** quick-bar items · **F** open chest / forage / rescue · **R** recruit · **1–5** swap
biome patch · **P/Esc** pause. (`B` is a debug ring-the-bell fallback in `siege::siege_controls`.)
Fly-cam: Space/Ctrl up·down, Shift sprint, hold Right-Mouse to look.
