# Second Map — Volcanic Ashlands (design)

**Date:** 2026-06-17
**Status:** approved design, ready for implementation plan
**Scope of first cut:** working prototype (boot into map 2 from the menu, see a clearly
different world, fight a siege). Polish staged after.

## 1. Why

One fixed island gets stale on repeat runs. Add a **second selectable map** that feels
maximally different for minimal added code/complexity. The whole world is procedural from
module-level consts in `worldmap.rs` (`REGIONS`, `COL_*` palette, `ATMOSPHERE`, river sine
funcs, `is_land_shape`), with no "map ID" concept. The lever: parameterize that const set
behind a small data table and pick one at boot. All generation math is reused unchanged.

### The four chosen axes (locked in brainstorming)

- **Theme:** *Volcanic Ashlands* — ash-grey + ember-red. Reskin the existing 5 biomes
  (rock→charcoal, forest→burnt-dead, snow→ash, desert→grey dune, swamp→sulfur).
- **New signature biome:** a **Lava field** (the one real new-content item).
- **Layout:** **mirror + reseed** — flip the sample X and shift the noise phase so the
  coastline, rivers, and biome positions genuinely differ. Same generation engine.
- **Polish:** working prototype first; explicit non-goals below.

## 2. Goals / Non-goals

### Goals (v1)

1. Start screen "New Game" offers a 2-way map choice (Home Island / Volcanic Ashlands).
2. Picking Ashlands boots a world that reads as a distinct place: new palette, new
   atmosphere, mirrored+reseeded layout, and a lava biome with a damage floor.
3. The choice round-trips the save (Continue resumes the correct map) and resets correctly
   on a fresh run.
4. The home island is **byte-for-byte unchanged** when map 0 is selected.

### Non-goals (explicitly deferred)

- New enemy mix, new win condition, wave rebalance (the "hardest" axis — dropped).
- Flowing/emissive magma pools rendered as a tinted water plane (v1 fakes magma as baked
  bright-orange ground mottle).
- New prop **models** beyond recolors of existing meshes.
- Map-2-unique landmarks / a relocated ork fortress (Ashlands reuses Gnashfang Hold as-is).
- More than two maps (the data table is built to extend, but only two ship).

## 3. Architecture crux (verified facts)

- **New Game rebuilds in-process** (no exe relaunch): `game_state::drive_fresh_run` routes
  StartScreen→Playing, sets `biome::PendingBuild(true)`, and `biome::apply_build` despawns
  all `BiomeEntity` and re-runs `worldmap::build`. (An earlier era relaunched the exe; the
  code no longer does — confirmed in `loading.rs` / `game_state.rs`.)
- **`worldmap::build` reads `tiles()`** — a process-global `OnceLock<Vec<Option<(TB,i32)>>>`
  (`TILES`). Built once, cached for the process lifetime. So an in-process rebuild redraws
  the **same grid**. This single cache is the *only* structural blocker to swapping maps.

Everything else that differs between maps is plain data read by `classify` / `ground_color`
/ the atmosphere wiring.

## 4. Data model — `MapDef`

A small table holding only the per-map variables. Lives in a new `src/worldmap/maps.rs`
(or a section of `worldmap.rs` — implementer's call; keep it one file region).

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MapId { Home, Ashlands }   // u8 on the wire (0 = Home, default)

struct MapDef {
    regions: &'static [Region],   // biome blobs: pos / radius / biome (TB) / peak height
    palette: Palette,             // the full COL_* set, incl. lava colours
    atmosphere: Atmosphere,       // (sky, fog, sun_col, sun_lux, amb_col, amb_b, sun_pos)
    noise_phase: f32,             // added inside noise_a/noise_b → reseeds coast/rivers/hills
    mirror: bool,                 // flip sample X (COLS-1-ix) → mirrored silhouette
    has_lava: bool,               // whether TB::Lava regions/ground/scatter are active
}

const MAP_HOME: MapDef = /* today's island values, verbatim — regression anchor */;
const MAP_ASHLANDS: MapDef = /* volcanic palette + atmosphere + lava region, mirror=true */;
```

`Palette` is a plain struct of the `COL_*` hex values currently top-of-file in `worldmap.rs`
(grass/sand/forest/rock/snow/desert/swamp/blight + their mottle tones) **plus** new lava
fields (`lava_basalt`, `lava_seam`, `lava_seam_hot`). `MAP_HOME.palette` is the existing
values exactly.

`classify`, `ground_color`, `biome_col*`, the region/mountain helpers, and the atmosphere
insert all switch from reading module consts to reading `active_map().<field>`. This is a
mechanical edit fully contained to `worldmap.rs`.

### Mirror + reseed details

- **Mirror** is applied at the single sampling site in `tiles()`:
  `classify(sample_x / MAP_SCALE, iz / MAP_SCALE)` where `sample_x = if mirror { COLS-1-ix }
  else { ix }`. Because `region_at`, rivers, and coast all evaluate at the passed base coord,
  the whole world mirrors consistently with one change. The castle stays at the origin
  (mirror is about the base centre `CX`, which is the island centre), so safe-zone/town
  plots are unaffected.
- **Reseed** adds `active_map().noise_phase` inside `noise_a`/`noise_b` (e.g.
  `(x*0.13 + 1.7 + phase).sin()`). Home uses `phase = 0.0` → identical output. Ashlands uses
  a non-zero phase → different coast fray, river winding, inland-hill placement.

## 5. Selection flow

1. **`ActiveMap(MapId)` resource** (new, in `worldmap.rs` or `game_state.rs`), default
   `Home`. This is the single source of truth a Bevy system can read.
2. **Menu:** the start-screen New Game control gains a 2-way map toggle (segmented
   button / two buttons, matching `ui/` kit + `ui/focus.rs` arrow-nav). It writes the choice
   into `ActiveMap`. (Mid-run New Game from the pause menu also reads/sets it.)
3. **Hand-off to the pure-fn world:** `biome::apply_build` reads `ActiveMap` and calls
   `worldmap::set_active_map(am.0)` exactly once, immediately before `worldmap::build(...)`.
   `set_active_map` stores the id in a process global the pure functions read (see §6). The
   StartScreen→Playing rebuild route guarantees this runs before the grid is sampled.

## 6. The tile cache (only structural change)

Replace the single `OnceLock` with a per-map cache:

```rust
static ACTIVE: AtomicU8 = AtomicU8::new(0);                 // current MapId
static TILES: OnceLock<Mutex<HashMap<u8, Arc<Vec<Tile>>>>> = OnceLock::new();

pub fn set_active_map(id: MapId) { ACTIVE.store(id as u8, Ordering::Relaxed); }
fn active_map() -> &'static MapDef { match ACTIVE.load(..) { 1 => &MAP_ASHLANDS, _ => &MAP_HOME } }

fn tiles() -> Arc<Vec<Tile>> {
    let id = ACTIVE.load(..);
    let map = TILES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = map.lock().unwrap();
    g.entry(id).or_insert_with(|| Arc::new(build_grid_for_active())).clone()
}
```

- `build_grid_for_active()` is today's `tiles()` body (the `classify` loop + `terrace_inland`).
- `tile_at` and callers take the `Arc` clone (cheap). Map switch = one regen (~76k tiles),
  one-time, fully covered by the loading veil. Each map's grid is memoized, so switching back
  is instant.
- **All other modules keep calling `tile_at` / `ground_at_world` unchanged** — the global
  swap is invisible to them.

## 7. Lava biome (Blight pattern, MVP)

The new biome reuses the **proven `TB::Blight` recipe** (its own ground draw + scatter, but a
mapped gameplay `Biome` so no new gameplay code).

- **`TB::Lava`** variant, present only when `active_map().has_lava`. One `Region` in
  `MAP_ASHLANDS.regions` is `TB::Lava`.
- **Gameplay mapping:** `TB::Lava → Biome::Swamp` in `tile_biome_world` — reuses the existing
  poison + slow damage-floor as "standing in lava burns". **Zero new gameplay/movement code.**
  (Decision locked: reuse Swamp DoT, not a distinct burn.)
- **Ground:** its own mesh sheet, like swamp/blight —
  `build_terrain_mesh(|tb| tb == TB::Lava)` with a basalt `GroundDetail` (dark, hard grain).
- **Magma look (v1):** glowing seams **baked as bright-orange mottle** in `biome_col_at` for
  `TB::Lava` (high-freq cracks in `lava_seam` / `lava_seam_hot` over `lava_basalt`). No new
  water plane in v1.
- **Scatter:** obsidian spires = existing rock-biome prop meshes recolored black via the
  Ashlands palette; ember particles via `ParticleKind` (reuse an existing kind, or add one
  `Ember` variant if none fits — small).
- **Blend:** lava blends into its neighbours over the standard `BLEND` band like any region.

## 8. Save + reset (the two run-state obligations)

- **Persist:** add `map_id: u8` to `SaveData` with `#[serde(default)]` (0 = Home) — additive,
  **no `SAVE_VERSION` bump**, old saves load as Home. `SaveCtx::snapshot()` writes
  `ACTIVE`/`ActiveMap`; `apply_pending_load` sets the `ActiveMap` resource **and** calls
  `worldmap::set_active_map(map_id)` before the world is used, so Continue rebuilds the right
  grid. (Map is a world-shape fact, applied like the other resource overwrites.)
- **Reset:** the menu owns `ActiveMap`; cold boot defaults to `Home`; New Game sets it before
  the rebuild; `set_active_map` is re-pushed every build via §5.3. No stale carry-over.

## 9. File-by-file change list (v1)

| File | Change |
|---|---|
| `src/worldmap.rs` | Extract `Palette`/`Atmosphere`/`MapDef`; add `MapId`, `ACTIVE`, `set_active_map`, `active_map`; per-map `tiles()` cache; mirror+phase in sampling/noise; `TB::Lava` (classify, `biome_col_at`, `tile_biome_world`→Swamp, ground sheet, scatter); switch consts→`active_map()`. |
| `src/biome.rs` | `apply_build` reads `ActiveMap`, calls `set_active_map` before `worldmap::build`. |
| `src/mainmenu.rs` (+ `game_state.rs`) | Map toggle on New Game; write `ActiveMap`. |
| `src/savegame.rs` | `map_id` field (+default) in `SaveData`; snapshot + apply. |
| new resource | `ActiveMap(MapId)` (worldmap or game_state). |

No changes to combat, waves, economy, town, AI, nav, audio for v1.

## 10. Risks / watch-items

- **`OnceLock`→`Mutex<HashMap>`**: any code path holding a `&'static Vec` from the old
  `tiles()` must move to the `Arc`. Grep all `tiles()` / `tile_at` users.
- **Mirror correctness**: build plots, the castle safe-zone, and bridges are authored in
  world space; verify they still land on flat grass after the mirror (they key off
  `CX`/castle distance, which is mirror-invariant, but screenshot-check).
- **Ashlands atmosphere vs volumetric fog**: per the ultra-graphics note, heavy fog can black
  the Atmosphere sky — keep Ashlands fog moderate, tune via `FOREST_FOG`.
- **Determinism**: reseed only shifts noise phase; placement RNG (`mulberry32` per-tile) is
  unchanged, so each map stays reproducible.

## 11. Verification

- `cargo test -p tileworld_core` unaffected (pure logic untouched).
- New unit test: `MAP_HOME` grid == the pre-refactor grid for a sample of tiles (regression
  anchor — home island unchanged).
- New unit test: save round-trips `map_id`; default-load yields `Home`.
- Screenshots via the harness: `FOREST_MAP=2` (debug boot hook) + `FOREST_SHOT` for the
  Ashlands island, the lava biome close-up, and a night siege.

## 12. Staging (within the prototype)

1. **Seam + reskin + mirror** — `MapDef` table, per-map cache, `ActiveMap`, menu toggle,
   palette/atmosphere/mirror/reseed, save+reset. Map 2 = recolored mirrored island, no lava.
   (This alone is a playable, clearly-different second map.)
2. **Lava biome** — `TB::Lava`, ground sheet, magma mottle, ember particles, obsidian
   recolors, DoT mapping.

Each stage is independently shippable and screenshot-verifiable.
