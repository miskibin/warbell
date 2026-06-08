# Tileworld Forest — Bevy

A single **static** 3D scene that recreates the TS tileworld game's **forest biome**
(32×32) in **Bevy 0.18**, with the same ground, models and post-processing — or better.
No player, no day/night, no gameplay: just the scene.

## Run

```bash
cargo run                      # opens the scene in a window
```

**Fly camera:** **WASD** move · **Space / Left-Ctrl** up·down · **Left-Shift** sprint ·
**hold Right-Mouse** to look (cursor locks while held).

Screenshot harness (renders a few frames, saves a PNG, exits):

```bash
# PowerShell
$env:FOREST_SHOT="shot.png"; cargo run
# bash
FOREST_SHOT=shot.png cargo run
```

`BEVY_ASSET_ROOT` can point at this dir if you run the binary from elsewhere (the
terrain WGSL is loaded from `assets/shaders/`).

## What's ported (1:1 from the TS game) + what's better

**Ground** (`terrain.rs` + `assets/shaders/terrain.wgsl`)
- The `vision.ts` terrain shader as an `ExtendedMaterial<StandardMaterial, _>`: a fine
  3-octave world-space value mottle, a large-scale analytic hue/value drift, and a
  procedural **grass detail texture** (port of `terrainDetail.ts` grass spec) imprinted
  on up-facing fragments. Same constants as the forest biome (detailScale 0.18,
  strength 0.65, variation 0.6, grass green `#6cb14a`).

**Models** (`trees.rs`, `props.rs`, `groundcover.rs`, `ruins.rs`)
- Built from the exact TS geometry specs: broadleaf (6 layered icosphere foliage tiers),
  birch (pale trunk + bark marks), dead tree (angled branch stubs); mossy faceted rocks;
  layered-green bushes; and a dense ground carpet of grass tufts, ferns, red-cap
  mushrooms, flowers and clover. Plus a standing-stone trilithon + giant dead tree as
  background landmarks. Every model is one merged, vertex-coloured mesh; instances share
  handles so the renderer auto-batches the thousands of props.

**Placement** (`scatter.rs`)
- Deterministic (mulberry32) scatter over the 32×32 patch — per-tile tree/bush/rock rolls
  plus a 4-per-tile ground-cover pass, with position jitter, scale + rotation variation.

**Post-processing** (`scene.rs`) — at least as good as the game:
- **AgX** tonemapping + exposure + a saturation `ColorGrading` (recovers the TS richness)
- **Bloom**, **Bokeh depth-of-field** (background blur), **distance fog** to the horizon
- **SSAO** + **SMAA**, gradient-cubemap **IBL**, a shadowed warm directional sun
- **Atmosphere** — Bevy 0.18's procedural sky: real blue sky, sun disk and horizon glow
  (this is the bit that's *better* than the original's flat sky dome).

## Layout

```
src/
  main.rs         plugin wiring
  scene.rs        camera + lights + post-processing + procedural sky
  terrain.rs      forest ground mesh + vision-shader material + grass detail texture
  trees.rs        broadleaf / birch / dead tree meshes
  props.rs        rocks + bushes
  groundcover.rs  grass tufts / ferns / mushrooms / flowers / clover
  ruins.rs        standing-stone trilithon + giant dead tree
  scatter.rs      deterministic placement over the 32×32 patch
  capture.rs      FOREST_SHOT screenshot harness
  palette.rs      colour helpers + the forest palette
assets/shaders/terrain.wgsl   the vision shader (WGSL)
docs/specs/                    the extracted TS visual + Bevy-API specs
CONTRACT.md                    the model-builder contract
```
