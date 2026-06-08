# Bigger map + ambient wildlife — design

Date: 2026-06-07. Target: `D:\tileworld-bevy-forest` (static Bevy 0.18 island viewer).

## Goal

1. Grow the island ×1.4 (more tiles, more land) and make the scatter denser.
2. Add 7 ambient wildlife species ported from the TS game, with procedural
   limb animation, biome-correct placement, ground-following and walkable wander.

This is a **viewer**: no player, no combat. Animals just make the world feel alive.

## Non-goals

HP / damage / death / respawn / combat, A* pathfinding, and the three monster
"animals" (scorpion, bog_croc, golem) are out of scope.

## 1. World growth (`worldmap.rs`, `scene.rs`, `biome.rs`)

- New const `MAP_SCALE = 1.4`. `COLS 144→202`, `ROWS 108→151`, `CX/CZ` recomputed
  from them.
- The generation stays in **base space**: `tiles()` calls
  `classify(ix / MAP_SCALE, iz / MAP_SCALE)` and `build_terrain_mesh` feeds
  `ground_color`/wall-noise the same base coords. Regions / rivers / coast / island
  math are therefore untouched → identical island shape, just sampled over more
  tiles. World units stay 1 tile = 1 unit, so the landmass is genuinely 1.4× wider.
- Camera default pose ×1.4 (`scene.rs`: y 44→62, z 80→112, look-target z scaled).
- Fog clear/full ×1.4 (`biome.rs`: 50/115 → 70/160) so view distance keeps pace.

## 2. Density (`biome.rs::scatter_region`)

- `SCATTER_DENSITY = 1.35` multiplies each main scatter class `chance`.
- `cover_per_tile` scaled ×1.5 (2→3) in the runner.
- One lever, applies to every biome + the grass frontier. Net props ≈ 2× (area) ×
  1.35 ≈ ~2.6×. Verify FPS on run; back `SCATTER_DENSITY` down if it stutters.

## 3. Walkability API (`worldmap.rs`)

Expose:
- `ground_at_world(wx, wz) -> Option<f32>` — terrain top Y, `None` on water/off-map.
- `walkable(wx, wz) -> bool` — land + the height-step is feasible vs neighbours
  (cliff faces Δheight ≥ 2 blocked, climbable slopes allowed — the TS `canStep` rule).

## 4. Creatures (`critters.rs` = models, `wildlife.rs` = systems)

Ported from the TS animal views: box-mesh quadrupeds animated procedurally (no
skeletons). Each animal is an **entity hierarchy** — a root + child part entities —
animated by ECS systems, mirroring `wind.rs`'s `Sway`.

- `critters.rs`: `build_<species>() -> CreatureSpec` = root scale + `Vec<PartSpec>`
  where `PartSpec { mesh, local: Transform, kind: PartKind }`. Parts are merged-tinted
  box meshes (`Cuboid`). 7 models: wolf, deer, boar, rabbit, polar_bear, elk, goat.
- Components: `Animal { species, mode, home, target, facing, speed, gait, timer, phase }`;
  part markers `Limb { side, pair, rest }`, `Head { rest }`, `Tail { rest }`, `Body { rest }`.
- Systems (`Update`):
  - `animal_brain`: wander ↔ graze state machine; integrate XZ toward `target`
    (re-roll target until `walkable`); set `facing` + `gait`; root `y = ground_at_world`
    with a small vertical bob while moving.
  - `animal_limbs`: overwrite each part `rotation = lean(sin(t·gait + phase)) · rest`
    — legs swing opposed, tail wags, head idle-turns. Frequencies/amplitudes from the
    TS views (legs `sin(t·12)·0.6`, etc).

## 5. Species, biomes, behaviour

| species | biome | mode | notes |
|---|---|---|---|
| deer | grass+forest | prey, herd | startles from camera |
| elk | forest | prey, herd | larger grazer |
| rabbit | grass | prey, skittish | tiny, fast, large flee radius |
| boar | forest/wilds | neutral wander | roots around |
| wolf | forest edge | loose predator | wander biased toward nearest prey, no kill |
| polar_bear | snow | slow wander | heavy slow gait |
| goat | rock highlands | prey, nimble | roams the terraces |

Startle: prey within `flee_r` of the **camera** flee directly away ~2s then resume.
Predators ignore the camera and drift toward the nearest prey. Herd species bias their
wander target toward herd-mates.

## 6. Spawning (`wildlife.rs`)

`WildlifePlugin` spawns in **combined world-map mode** only, tagging each root
`BiomeEntity` (the existing biome-switch despawn/rebuild then handles them). Placement:
per-species count via a deterministic mulberry32 `Rng`, rejection-sampled onto
biome-masked + `walkable` tiles; herds spawn as clusters. Target ~50 animals
(deer 10, elk 8, rabbit 10, boar 5, wolf 6, goat 7, bear 4) — tunable.

## 7. Verification

`cargo build` is the gate. Then `cargo run`, fly each biome, confirm animals walk /
graze / startle, sit flush on ground (not floating/sunk), and land in the right biome.
Use `capture.rs` for proof screenshots. No Rust test harness → run-verify.

## Tunables

`MAP_SCALE`, `SCATTER_DENSITY`, per-species populations, camera/fog distances.
