# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

**"Warbell"** (renamed from "D: Tileworld", June 2026) — a **Bevy 0.19** game (upgraded from 0.18 in
June 2026). This started as a static forest-scene viewer and
has grown into a full playable game: a knight defends a central castle against night-wave
ork sieges, with combat, economy, an upgrade tree, inventory, villagers, succession, and five
biomes on one enlarged island. (It began life as a port of an old TypeScript/three.js game;
that original is **obsolete and gone as a reference** — this repo + `crates/core` are the sole
source of truth. Do not look for or cite "the old game".)

> `README.md` and the `main.rs` module doc-comment now describe the actual game (both were
> truthed-up after an earlier era where they still claimed "no player, no gameplay, just the
> scene"). They're fine as entry points, but the per-subsystem **source of truth** is the parity
> roadmap, `docs/superpowers/specs/2026-06-07-tileworld-parity-port-roadmap.md` (numbers + reuse
> pointers). If you spot a code comment still describing the old static-viewer state, it's stale —
> fix it.

## Models / subagents

**Main session runs Fable** — best model, worth it for the reasoning-heavy work (architecture,
gameplay logic, tricky edits). Fable is expensive, so **don't burn it on grunt work.** Delegate
token-heavy, low-judgement tasks to an **Opus subagent** (cheaper) via the `Agent` tool with
`model: "opus"`:

- **Codebase exploration** — broad "where is X / how does Y work" fan-out reads (use the `Explore`
  agent with `model: "opus"`).
- **Visual debugging** — capture-harness runs (`FOREST_SHOT`/`FOREST_CLIP`), reading back
  screenshots, iterating on a shot until it looks right.
- Any bulk, mechanical, or high-token-low-insight task (log sifting, wide grep sweeps, repetitive
  edits).

Rule of thumb: **Fable decides, Opus fetches.** If the task is mostly "read a lot / look at a lot"
and little judgement, hand it to an Opus subagent and keep the conclusion, not the file dumps.

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

**Multi-agent caveat:** in a *parallel-dispatch* session (several agents each editing ONE module
against a *shared* `target/`), agents must **not** run `cargo build/check/run` — a concurrent build
corrupts the shared dir; the integrator builds once at the end. That rule is **only** for parallel
dispatch. A normal single Claude session should build and verify.

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
$env:FOREST_SHOT="shot.png"; cargo run      # warms up (≥240 frames AND ≥10s so cold pipelines/shaders/IBL settle; FOREST_SHOT_WARMUP=<secs> raises the floor), saves PNG, exits
```

**For player-perspective / game-feel shots use `FOREST_TPS=1`, NOT a hand-placed `FOREST_CAM`.**
`FOREST_TPS` boots the real over-the-shoulder **gameplay** camera (`player::camera`), so the frame
matches what a player actually sees — no guessing fly-cam coordinates (which reliably produces
useless god/cloud-cam angles). Just drop the hero with `FOREST_HERO="x,z"`; tune the view with
`FOREST_TPS_AZ`/`_PITCH` (radians) + `_DIST` (units). Pair with `FOREST_DEMO=explore` to film a real
**walking** shot (footsteps / dust kicks / anim) through the follow-cam. Reserve
`FOREST_CAM` for deliberate god/overview framing only.

**A HIGH god-cam to "see the whole island" washes out to flat white — don't fight it.** Three
things compound at altitude and none is your bug: atmospheric **haze** fogs distant terrain to the
sky colour (`FOREST_FOG="clear,full"`, bigger = thinner, only nibbles — it does NOT clear a
whole-island view); the **cloud layer** sits ~y90 so a camera above it shoots cloud-tops; and
**midday bloom** (`FOREST_TIME≈0.25–0.35`) blows the bright/snow areas white. To actually see
terrain, frame **low + oblique** (camera height **~15–40**, close, looking near-flat along the
ground — the orbit examples below use *height 14* on purpose); a near-top-down at y≈25 over one
feature reads crisply. To verify a **spread-out** feature (rivers, biome layout), take **several
low close shots at the known biome/feature coords** (below), one per region — NOT one doomed
overview. For a true island-wide frame use the **orbit clip** (`FOREST_CLIP_ORBIT`, low height)
and pick a frame; a single high static cam can't do it cleanly.

```powershell
# player-perspective still (hero framed by the real follow-cam):
$env:FOREST_SHOT="shot.png"; $env:FOREST_TPS="1"; $env:FOREST_HERO="-18,24"; cargo run
# real gameplay walk clip (the demo drives the hero; the follow-cam films it):
$env:FOREST_CLIP="target/clips/walk"; $env:FOREST_TPS="1"; $env:FOREST_TPS_PITCH="0.6"; $env:FOREST_DEMO="explore"; cargo run
```

For **GIFs / video** (itch.io promo, motion bugs) use the clip mode instead — it saves a numbered
PNG per frame to a dir, then ffmpeg stitches it. A clamped fixed timestep keeps motion smooth
despite the per-frame encode stall; `FOREST_CLIP_ORBIT` slowly circles a point. `siege_clip_refill`
keeps a `FOREST_WAVE` assault topped up so a long siege actually films a battle.

```powershell
# island/biome flyover (orbit "cx,cy,cz,radius,height,deg_per_sec")
$env:FOREST_CLIP="target/clips/desert"; $env:FOREST_CLIP_ORBIT="71,1.5,-46,22,14,7"; $env:FOREST_TIME="0.24"; cargo run
# sustained night siege
$env:FOREST_CLIP="target/clips/siege"; $env:FOREST_WAVE="1"; $env:FOREST_DEFEND="1"; $env:FOREST_TOWN="1"; $env:FOREST_CAM="0,15,30,0,2,-8"; cargo run
# stitch (per-clip mp4 + gif): ffmpeg -framerate 30 -i frame_%05d.png -pix_fmt yuv420p out.mp4
```
Biome region centres (world XZ, at `MAP_SCALE` 2.6): snow (-82,-53) · desert (71,-46) · rock (78,5) ·
forest (-71,46) · swamp (0,67). These scale with `MAP_SCALE` — code with a hand-authored world coord
should route it through `worldmap::world22` (rescales 2.2-era coords) instead of baking the scale in.
Clip knobs: `FOREST_CLIP_FRAMES` (150) · `FOREST_CLIP_FPS` (30) · `FOREST_CLIP_WARMUP` (30).

Env hooks that stage a scene for a shot (combine with `FOREST_SHOT` **or** `FOREST_CLIP`), all read at startup:

| Var | Effect |
|---|---|
| `FOREST_SHOT=path.png` | single-shot capture-and-exit harness (`capture.rs`) |
| `FOREST_CLIP=dir` (+`_FRAMES`/`_FPS`/`_WARMUP`/`_ORBIT`) | frame-sequence recorder → ffmpeg GIF/video (`capture.rs`) |
| `FOREST_CAM`, `FOREST_TIME`/`FOREST_DAY`/`FOREST_NIGHT` | camera pose / time-of-day for the shot |
| `FOREST_HERO="x,z"` | drop the hero at a world XZ (e.g. deep in a biome region) to stage its reactive atmosphere/weather |
| `FOREST_BIOME` | boot straight into a given biome |
| `FOREST_WAVE` / `FOREST_DEFEND=1` | stage a night siege / arm all defenses + walls |
| `FOREST_MUSTER=1` | rally the whole town into the **war party** at boot (as if pressing `K`) so a shot frames the muster; pair with `FOREST_DEMO=work` (stages a 14-pop town) for a full host (`villagers.rs::stage_muster`) |
| `FOREST_ORKLINE="x,z"` | park one ork of each variant in an idle line at a world XZ (model close-ups) |
| `FOREST_ARCHERS=<n>` | retrain the whole standing militia as **longbow archers** at boot (`villagers.rs::stage_archers`); numeric `n ≥ 2` also raises `town.population` to `n` so a whole squad grows in to retrain. Pair with `FOREST_MUSTER`+`FOREST_HERO` to park a volleying rank anywhere, `FOREST_ORKLINE` for live targets, or `FOREST_WAVE` for a defended siege. (`FOREST_VIEW=peasant:archer` + `FOREST_VIEW_ANIM=bow` previews the model / draw-loose clip in the viewer.) |
| `FOREST_CAGETEST="x,z"` | park the prisoner-cage rescue's before/after states side by side at a world XZ: a CLOSED cage of real seated peasant captives + an OPENED emptied one 5.5u further +X (`camps::spawn_cage`); both doors face +X, so frame from the east. Film the actual door-swing + walk-out with `FOREST_DEMO=rescue` + `FOREST_CLIP` instead |
| `FOREST_BREACH=1` | auto-break the Hold gate on the first sim frame so a shot/clip films the woken garrison + the Warlord boss without a keypress (`ork_fortress::stage_breach`); pair with `FOREST_HERO`/`FOREST_CAM` inside the walls |
| `FOREST_RIVAL=<n>` | instantly raise `n` buildings in the **rival stronghold** (the desert AI opponent, `rival.rs`) so a shot frames a grown rival town instead of waiting out its economy (default: fill the bailey); the rival keep/walls/garrison spawn regardless. Frame it at `rival::RIVAL_CENTRE` ≈ world `(78, -104)` at MAP_SCALE 2.6 (NE desert) |
| `FOREST_TREELINE="x,z"` | park one of each `TreeKind` (broadleaf/birch/pine/poplar/autumn/dead/stump) in a 2× row at a world XZ (tree-model close-ups, `trees.rs`) |
| `FOREST_FISHLINE="x,z"` | park one of each fish variety (silver/blue/gold) frozen mid-leap in a lit row at a world XZ (fish-model close-ups, `fish.rs`) |
| `FOREST_MENU=1` | shoot the start screen |
| `FOREST_FP=1` | boot straight into first-person (forces Play so the follow-cam eye-view can be captured; `player/camera.rs`) |
| `FOREST_TPS=1` (+`_AZ`/`_PITCH` rad, `_DIST` units) | boot Play + **third-person** real follow-cam so a shot/clip frames the world like actual gameplay (NOT a god-cam `FOREST_CAM`); place the hero with `FOREST_HERO`, film a walk with `FOREST_DEMO=explore` (`player/mod.rs`, `player/camera.rs`) |
| `FOREST_LOADTEST=1` | hold the boot loading veil up (even under a capture) so it can be shot (`loading.rs`); pair with `FOREST_SHOT`+`FOREST_MENU=1` |
| `FOREST_PANEL=tree\|inv` | seed + open the upgrade-tree / satchel panel for a shot |
| `FOREST_EQUIP="sword_gold,gold_armor"` | equip the listed item ids at startup so the hero model shows its weapon/armor |
| `FOREST_FOG="clear,full"` | override fog distances live (no rebuild) |
| `FOREST_QUALITY=ultra\|high\|low` | startup graphics preset (`quality.rs`); `ultra` = demo showcase (visible god rays + maxed AA/AO/shadows) |
| `FOREST_AUDIOTEST` / `FOREST_GRADETEST` | isolate audio / reactive-grade for testing |
| `FOREST_FLOATTEST=1` | continuously stage sample floating combat numbers near the hero (style preview) |
| `FOREST_FLAGTEST=1` | park one cloth banner in open air at `(0, 6, -22)` to frame the flutter in isolation (`banner.rs`). NB the cloth streams along world ≈`(0.9, 0, -0.43)` — shoot from a spot perpendicular to that or it reads edge-on |
| `FOREST_BELLTEST=1` | re-toll the war bell on a ~12s loop (swing + clapper + SFX) so a shot/clip frames the ring without a keypress (`castle::swing_bell`); the bell stands at `castle::BELL_POS` (4.5, 7.5) |
| `FOREST_ROLLTEST=1` | re-arm the hero's **Alt dodge-roll** (forward, along the facing) on a ~1.8s loop so a `FOREST_TPS` shot/clip frames the somersault without a keypress (`player/movement.rs::player_roll`); skips the pointer-lock/stamina gates |
| `FOREST_SWINGTEST=1` | re-arm the hero's **attack chain** on a ~1.15s loop (inside the combo window, so it steps chop → slash → thrust; every 4th swing is the Heavy) with no keypress, skipping the pointer-lock gate, so a `FOREST_FP`/`FOREST_TPS` clip frames every swing variant (`player/combat.rs::swing_test`). Pair with `FOREST_FPDBG=1`, which samples fast (~0.12s) under this hook, to probe the FP viewmodel through the swings |
| `FOREST_IMMORTAL=1` | the hero takes hits with full juice (floats/flash/shake) but can't drop below 1 HP, so a filmed melee never trips the **succession beat** — which slow-mos the world and swings the camera to the nearest townsperson, hijacking a combat clip's framing (`player/health.rs::apply_hero_damage`) |
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
freezes the world but keeps rendering. Sim
systems carry the gate; render/camera/anim/audio/HUD systems stay ungated so the frozen world
still draws. New simulation systems **must** carry `.run_if(in_state(Modal::None))` or they'll run
through pauses/panels.

### World layout: biome viewer vs. the real island

Two related map systems share the ground pipeline in `biome.rs`:
- **`biome.rs` + `biome_<name>.rs`** — the biome *framework*. Each biome exposes `config()`
  (declarative ground/atmosphere/scatter/particles) + optional `landmarks()`. Keys **1–5** swap a
  single 32×32 biome patch at runtime (despawns everything tagged `BiomeEntity`, rebuilds).
- **`worldmap.rs`** — the actual playable island: generated at base
  resolution scaled up by `MAP_SCALE = 2.2` (→ `COLS 316 × ROWS 360`). Elliptical island, five
  biome blobs, grass safe-zone (castle), four rivers + lake, coastal mountain ridges, rolling
  terraced knolls, plateaus. `classify` force-flattens grass under every town build plot (and
  chest/cover placement rejects `town::near_build_plot`) so nothing occupies a future building's
  spot. `worldmap::build` also seeds castle/camps/ore/chests placement. **Generation runs in base
  space, drawn over the enlarged grid**, so the island shape is identical, just denser.

### Pathfinding through the castle (`src/navgrid.rs`)

Wraps core's tested A* (`tileworld_core::pathfinding`) onto forest's world-space terrain. Walls
register impassable collision boxes; **gate gaps register none**, so A* threads the gates with no
explicit gate-targeting code. Night-wave invaders follow `InvaderPath` waypoints to the keep.

### Save / load (`src/savegame.rs`) — the one-slot snapshot

A save is a **logic snapshot, not an ECS dump**: the world is built once at `Startup` and is
persistent within a process, so we serialize the run-state *resources* (hero / economy / town /
upgrades / keep / heirs / night) plus a few world flags (looted chests, rescued camps, discovered
landmarks) to one JSON slot, and on load overwrite those resources + mark the already-spawned
entities. Two write triggers, **both Prep-only**:

- **Dawn autosave** — `autosave_on_dawn` fires on the `Wave → Prep` edge (a cleared night).
- **Manual save** — the pause-menu **Save Game** button sends `RequestSave`; `manual_save` writes
  while in `Prep` (greyed/refused during a siege). Allowed on day 1 (`wave_index == -1`).

Both build the snapshot from one shared `SaveCtx` SystemParam (`snapshot()`), so there's a single
field list to keep in sync. Loading: `begin_continue` drops the file into `PendingLoad`,
`apply_pending_load` writes it back over the live resources the next `Playing` frame and emits
`GameLoaded`, then the entity-owning modules reconcile from that message (`town.rs` rebuilds
meshes, chests re-open, landmarks re-mark, **bosses despawn already-slain wardens**). Restore always
forces `phase = Prep` at the saved night — which is *why* saving mid-`Wave` is forbidden (a mid-night
`wave_index` would resume one night too late, skipping the fight).

**Invariant — when you add anything a player earns/changes across a run, it MUST round-trip the
save, or it's silently lost on Continue.** Concretely: put it in core stores where you can (those
already serialize via the core `serde` feature and ride `Player`/`Bag`/`Town`/`ResourceState`), and:

1. add the field to `SaveData` (bump `SAVE_VERSION` only on a *breaking* shape change — additive
   fields use `#[serde(default)]` so old saves still load);
2. read it in `SaveCtx::snapshot()` and write it back in `apply_pending_load`;
3. if it lives on **entities** (not a resource), reconcile it from the `GameLoaded` message in the
   owning module — and read the value off the carried `SaveData`, never live `PlayerRes`/etc., which
   `apply_pending_load` may write the same frame in undefined order.

Things deliberately **not** saved (fine — derived/transient): timed `Buffs`, pickup `Toasts`, the
battlefield (invaders/bolts/corpses — swept on Continue), warden *levels* (re-level from 1), and
warden *kills* as such (the permanent boon flag on `Player` is the record — `boss::despawn_slain_wardens`
reads it to drop a beaten warden). `Lives.heirs` mirrors `town.population`, so it's saved via `Town`.

## Conventions that bite if you miss them

- **Coordinate frame is world-space with the castle at the ORIGIN** (deliberate — do NOT "fix"
  it). One tile = one world unit;
  `tile = floor(world + G)` where `GX/GZ` (in `worldmap.rs`) recentre the grid onto the origin.
- **Enemy combat numbers are at full core parity (NOT rescaled).** An earlier pass
  scaled ork HP/damage and wildlife damage *down* (~×0.35 HP, ~×0.5 dmg) for the smaller scene,
  but that left enemies far too soft against a hero already at full core power (core
  `PLAYER_BASE_DAMAGE` 25 + weapons/crit/levels). They're now read straight from core:
  `siege::base_hp` → `ork_config(v).hp` (grunt 254 / scout 136 / berserker 306 / shaman 201),
  `orks::ORK_DAMAGE`=24 (variant_melee derives the rest), `SHAMAN_BOLT_DAMAGE`=26, and
  `wildlife::predator_stats` bite damage → `animal_config(s).attack_damage` (wolf 12 … golem 28).
  Wildlife HP already came from core. Per-night HP growth is the `siege::WAVES` `hp_scale` table
  (1.1·1.15^n), unchanged. When wiring a new combat number from core, use it **as-is** — don't
  reintroduce the old rescale.
- **Despawn races are pervasive** — many systems race to reap the same entity (corpses, kills,
  wave clears). Always use `try_despawn` (not `despawn`) and `try_insert` (not `insert`) on
  entities that combat/AI/HP-bar systems might have already removed. This is load-bearing; bare
  `despawn`/`insert` will panic intermittently.
- **Death is a fade, not a pop** (`src/dying.rs`): kills insert `Dying{since}` and crumple over
  ~1.4s. Every AI/targeting/count query must filter `Without<Dying>` so corpses aren't targeted or
  counted — but reward/clear logic fires once pre-fade.
- **Anything a player earns or changes over a run has TWO obligations — persist it AND reset it.**
  Add any new run-state (resource, flag, or earned entity) to *both* paths or it breaks silently:
  1. **Persist** — round-trip it through the save (`src/savegame.rs`): `SaveData` +
     `SaveCtx::snapshot()` + `apply_pending_load` (reconcile from the `GameLoaded` message if it
     lives on entities). Miss this and it's lost on Continue — exactly how a run's level/items, and
     later the quest chain, went missing. See the **Save / load** section for the checklist + the
     deliberately-unsaved list.
  2. **Reset** — make sure **New Game** wipes it back to its fresh-boot value (resources via the
     `OnExit(StartScreen)`/`OnExit(GameOver)` reset systems or the in-process world rebuild;
     world entities via the rebuild's despawn). Miss this and a new run inherits the old run's state.
  Default rule of thumb: prefer holding it in a core store (those already serialize) and a resource
  that a reset system clears, so both obligations are one edit each.
- **Every voice/quote line carries its spoken text in a code comment.** When you wire up *any*
  spoken line (hero `voice.rs`, villager `audio/npc.rs`, ork `audio/ork.rs`, or future speakers),
  put the exact transcript next to where the clip is loaded / keyed, plus its trigger. The audio
  files are opaque `.ogg`s — these comments are our **only** record of what the game actually says,
  so we can retune *when/how often* lines play without re-listening to every clip. Keep them in
  sync when a clip is re-recorded; if a clip's text is unknown (older un-transcribed asset), say so
  explicitly (`[older clip — text not transcribed]`) rather than leaving it blank.
- **Mesh-building contract** (verified API forms in the doc cited below, §9): every prop mesh's
  base sits at
  `y = 0`; **colour lives in the mesh `ATTRIBUTE_COLOR`** (linear RGBA via `crate::palette::lin` /
  `lin_scaled`), because all props share one white `StandardMaterial` so the renderer auto-batches
  thousands of instances. Build parts as primitives, `tinted()` each (add COLOR) before
  `Mesh::merge`, and `duplicate_vertices()` + `compute_flat_normals()` for the crisp low-poly facet
  look (duplicate FIRST — `compute_flat_normals` panics on an indexed mesh). The verified Bevy
  0.18.1 API forms are in `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md` (the
  mesh-building API is unchanged in 0.19, so it still applies); the per-slice spec docs sometimes
  **guess wrong** about the Bevy API — trust the verified doc + the real Bevy source, now under
  `C:\Users\skibi\.cargo\registry\src\index.crates.io-*\bevy_*-0.19.0\src`.
- **Determinism**: scatter/placement uses `mulberry32` (core `rng.rs`) seeded per-tile, so the
  world is reproducible. "Feels the same" parity, not byte-exact RNG/map.
- **Forest's divergences are canonical, not bugs**: `siege.rs`'s wave director is richer than
  core's `wave.rs` and is the one in use; `KEEP_MAX_HP = 1500` / 12-per-s repair; the bespoke
  `Atmosphere`/IBL lighting; the custom CoC bokeh `dof.rs` over a plain DoF. Don't "restore" these
  to match core.
- **The render pipeline is SINGLE-CAMERA — do NOT add a second `Camera3d`.** The "proper" Bevy
  two-camera / `RenderLayers` first-person view-model pattern (the `first_person_view_model` example)
  was tried and **fails three ways here** (June 2026): (1) a 2nd `Camera3d` makes every
  `single()`/`Single<Camera3d>` query in the codebase ambiguous (`player_camera`, `drive_dof_focus`,
  bloom/dof/godrays/outline/grade/atmosphere…), so the main camera silently stops being driven and
  freezes at the boot overview pose; (2) a 2nd HDR+`Tonemapping` camera corrupts the main camera's
  output — Bevy issue #17530 — washing the world to flat sky; (3) the heavy prepass+post stack
  (Depth/Normal/MotionVector prepass + SSAO + DOF + godrays + outline + bloom) throws a hard **wgpu
  Validation Error → the app quits**. Don't re-attempt it. Anything "in front of the lens" (a FP
  weapon/arms view-model, a HUD-in-world overlay) must live in the ONE camera. The working FP
  view-model knobs: `fp_keep` in `player/mod.rs::spawn_hero_meshes` picks which limb meshes survive
  FP (hide the **upper-arm** meshes — they balloon at the eye — but KEEP the forearm so the weapon
  has a hand and doesn't levitate); `camera::fp_body_visibility` applies it; the FP arm/sword/shield
  poses live in `anim::hero_anim` (July 2026 rework): the arms are **always viewmodel-driven in FP**
  (the eye sits AT the chest, so third-person clips orbit the lens itself — never let them play on
  the FP arms), a `fp_ready` weight keeps the gear in a low carry at the frame edges out of combat
  and draws it up when a threat is near / attacking / blocking, and the whole arm chains are
  **handedness-MIRRORED** in FP (the studio rig renders its "R" joints on the viewer's left;
  translations flip X, rotations conjugate `(x,-y,-z)`) so the sword reads bottom-right / shield
  bottom-left. The FP wrist/shield angles were **solved from `FOREST_FPDBG=1` camera-space probes,
  not eyeballed** — pose-space intuition is useless through the tilted FP hand frame, so tune
  against the probe vectors (NB: FPDBG needs `FOREST_SHOT` too — without the shot harness the app
  idles on the start screen and the probes read the menu camera). The eye sits at
  `FP_EYE_H`/`FP_FWD_OFF` in `player/camera.rs`; and the main-camera **near-plane**
  (`scene.rs::setup_camera`, `near: 0.04`) is lowered so the close-held weapon doesn't slice the
  near-plane (that slicing was the walk-time "flicker"). July 2026 FP-combat polish knobs:
  per-variant swing shaping is `anim::fp_swing` (wind/punch endpoints + `sw_roll` edge-roll about
  the blade axis + `arc` mid-strike crescent). **The FP eye sits ~0.2u from the shoulder pivot, so
  any real shoulder-Y sweep parks the forearm ON the lens (whole frames black out) — cross-frame
  travel must live in the WRIST**, with `sw_x` compensating the tilted hand frame (a big wrist yaw
  alone reads as a rising poke). Swings also drive a small per-variant **camera swing-sway**
  (`anim::fp_cam_sway` → `FirstPerson::sway`, applied post-`look_at` in `player_camera`; a lean,
  never a shake) and an FP **close-quarters FOV widen** (`camera::FP_CLOSE_FOV_DEG` eased off the
  ringed foe's distance) buys back a melee-range ork's silhouette. Enemy **HP bars clamp into a
  view cone above the eye** (`combat_fx::HP_BAR_CONE_SLOPE` — the bar slides down toward the chest
  and pulls toward a close camera, shrinking) so a towering foe's bar stays on screen in FP melee.
  NB: FP melee inherently puts the enemy in your face — the widen softens it, but no view-model
  trick removes it; third-person is the design's combat view.
- **Capture-harness flakes — confirm the `Screenshot saved` log line, and retry before debugging.**
  For `FOREST_CLIP`, set `FOREST_CLIP_WARMUP=70`+ — the default 30 records the tail of the boot
  **loading veil** (`loading.rs`, lifts ~1.4s of sim time): the first ~50 saved frames render
  dimmed/black *behind an intact HUD* in every scene, which reads exactly like a full-frame
  rendering bug and is invariant to any code change (a whole session was once lost chasing it).
  A `FOREST_SHOT` run can emit a junk frame that is NOT a code bug: a **black** frame (cold pipeline,
  see the bevy-0.19 note) or an **overview/god-cam** frame (the follow-cam hadn't engaged yet under
  `FOREST_FP`/`FOREST_TPS`). And if the run **crashed** (e.g. a wgpu validation error) no new PNG is
  written, so you'll `Read` the **stale** previous file and misread it as "my change did nothing".
  Always grep the run output for `Screenshot saved` / `error[` / `panic` / `Validation` before
  trusting the image; if a frame looks wrong, re-run once before diagnosing it.

## Reference material

- `docs/superpowers/specs/2026-06-07-tileworld-parity-port-roadmap.md` — the P0–P6 roadmap + locked
  decisions + per-subsystem numbers. Read this before any gameplay-parity work.
- `docs/specs/` — extracted visual specs + the **verified** Bevy 0.18.1 API doc.
- `docs/superpowers/specs/*-design.md` — per-feature design docs (audio, hero, combat feedback,
  shaman spells, biome variety).

