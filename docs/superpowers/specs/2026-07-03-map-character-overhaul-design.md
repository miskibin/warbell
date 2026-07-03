# Map Character Overhaul — Design

**Date:** 2026-07-03
**Status:** Approved (user), implementing pass-by-pass
**Problem statement (user):** the map is bland. Mountains too subtle and too few; roads
visually indistinguishable from the surrounding biome; biomes feel too small once the warden
glade / rival plateau / fortress apron are subtracted; the swamp is flat and samey ("some flat
surfaces"); big empty areas. The map must gain character — beautiful places worth staring at,
better terrain shaping, every square metre purposeful. Not a tweak; a real overhaul.

## User decisions (locked)

- **Feel:** Witcher 3 (POI density, 40-second rule) + RDR2 (terrain drama, vistas). NOT
  realism/KCD-simulation — though we borrow KCD's "roadside grammar" props where cheap.
- **Roads must always be easy to navigate.** No POI may block a road (today an ork camp can
  land literally on one — bug to fix).
- **Size:** slightly larger — `MAP_SCALE` 2.0 → **2.2** (~317×361 tiles). The main budget goes
  into density/character, not area.
- **Mountains:** full drama — impassable cliff walls, entries via authored passes/canyons,
  walkable ridgelines. Terrain channels movement (and ork paths) through specific gaps.
- **Swamp identity:** dead trees standing in water + fog, boardwalk roads over water, sunken
  ruins/wrecks. (Wisps/glowing accents kept as night dressing — the authored-but-unwired
  `biome_swamp::landmarks()` content finally gets plugged in.)

## Root causes found in code (2026-07-03 exploration)

1. **Mountains are deliberately castrated:** `worldmap.rs::terrace_inland` force-lowers every
   inland step to ≤1 height class (0.5u) because `navgrid::can_step` rejects |Δy| > 0.5+0.1;
   test `inland_terrain_is_climbable` enforces it. Rock peak 18 → only 8.5u over a 68u radius
   (~12% grade). Cliffs are only allowed on the coastal band (`dist_from_coast ≤ 7`).
   `mountain_height` noise was also deliberately cut to keep terraces climbable.
2. **Roads are only a vertex-colour tint** (`ground_color()` blend toward `road_dirt`), no
   edge/verge/ruts/geometry; nearly invisible on dark rocky ground. The road *graph*
   (`roads.rs`: trunks to all 5 biomes + ring + fortress/rival/landmark/camp spurs +
   capillaries) is good — readability is the failure, not topology.
3. **Placement ignores roads:** scatter rejects `on_road`, but camps/chests/ruins placement
   does not — hence camp-on-the-road.
4. **Swamp max height class 2** (0.5u of micro-relief = flat), standing pools were removed
   (they rendered as ugly white plates — a shading bug, not a concept failure), and the
   authored swamp landmarks (wisps, glowing mushrooms, hollow tree) are dead code — never
   placed on the world map. `ParticleKind::Fireflies` also unused.
5. **Biome interiors are eaten by claims:** warden glade r16, rival flat r30, fortress apron
   58×50 — the slight scale bump plus larger region radii restores breathing room.

## Research principles applied (Witcher 3 / RDR2 / Ghost of Tsushima / Disney)

- **40-second rule** (CDPR): something interesting every ~40s of travel along routes — our
  density metric. POIs paced along roads, not scattered uniformly.
- **Weenies:** *Flags* = skyline-breaking far-visible markers (smoke columns, tall silhouettes,
  towers, bird flocks); *Breadcrumbs* = ground-level markers (shrines, cairns) that pull the
  last stretch. Sightline hygiene: clear scatter around a flag or it pulls nobody.
- **Contrast makes drama:** tall reads tall only next to flat. Mountains get flat meadow
  aprons; the island keeps deliberate calm zones between dense ones.
- **Compression → release:** narrow canyon/gate opening onto a wide vista is the recipe for
  "a place you stare at".
- **A road reads as a road via its edge,** not its fill: verge band, wheel ruts, width tiers,
  signposted junctions, roadside furniture at intervals.
- **Biome = one-screenshot test:** unique silhouette prop + colour grade + ground pattern +
  one signature flag landmark each.

## The plan — six passes, each independently shippable

Order rationale: scale first so every later pass tunes against final geometry; terrain second
because roads/POIs/vistas sit on it.

### Pass 0 — Scale 2.2 + biome breathing room (small)
`MAP_SCALE` 2.0→2.2; biome `REGIONS` radii +10–15%; retune scatter density if perf demands
(`SCATTER_DENSITY`/`COVER_DENSITY` are the levers); fix stale COLS/ROWS doc comments. Verify:
orbit clip + perf sanity (scatter chunk-merge holds; ground shader is the fragment-bound risk).

### Pass 1 — Terrain drama (the foundation, biggest)
- **`CLIFF_ZONES` region data exempting areas from `terrace_inland`** (the coastal band is
  precedent). Inside a zone, multi-class jumps stand as sheer cliff walls — skirt-wall geometry
  already renders any height, and `navgrid::can_step` already treats >1-class steps as walls,
  so NPC blocking comes for free. The climbability test gets scoped to non-cliff-zone tiles;
  instead a new test asserts every zone's *pass corridors* are climbable end-to-end.
- **Passes as the only entrances:** authored ramp corridors (existing `ramp_class` mechanism)
  through the cliff walls. Roads, camp placement, and invader/wave paths route through them.
- **Rock biome → tiered mesa:** higher peak, flat shelves + vertical faces between them.
- **Snow massif → sharper peak + a walkable ridgeline** with the sea on one side (scenic
  route with breadcrumb cairns).
- **Canyon at the rock/desert border:** narrow floor road (compression) opening onto the
  desert (release).
- **Waterfall:** a river drops off the rock highland cliff into the lake — signature flag
  (motion + sound) for one asset.
- **Contrast aprons:** flat meadow bands at mountain feet.

### Pass 2 — Roads that read as roads
- Vertex-colour upgrade: road core + **verge band** (edged transition, not a smear) + **wheel
  ruts** (two darker parallel stripes) on arteries; stronger per-biome tint contrast (light
  packed track on dark rock).
- **Signposts at every road-graph junction** pointing at named destinations.
- Roadside furniture on a spacing rule: mile cairns, wayside shrines, short fence runs,
  wrecked carts.
- **Placement respects roads:** camps/chests/ruins reject positions within road clearance
  (`near_road`-style test). Fixes camp-on-the-road.
- Swamp roads become boardwalks in Pass 3.

### Pass 3 — Swamp overhaul
- **Hummock micro-relief** (class-1/2 islets) + **standing murky water between them** —
  pools return, but rendered like the murky green `river_color` water, not the old white
  plates.
- **Road through swamp = wooden boardwalk on stilts** — the road becomes an OBJECT; the
  "can't see the road" complaint dies by construction.
- Dead trees standing IN the water; sunken tower ruin; boat wreck; stilt huts.
- Wire the dead content: `landmarks()` wisps + glowing mushrooms, `Fireflies` particle, low
  ground mist. Night swamp = bioluminescent attraction, not a void.

### Pass 4 — POI/vignette pass (40-second rule) + flags
- Micro-POI set: wayside shrines, a gallows on the hill approaching Gnashfang Hold, burned
  cabin, fresh grave, standing stones, ruined watchtower, shepherd's hut — placed **along
  roads at ~40s walking intervals**, never blocking them.
- Fields + fences around town build plots (cheapest kill for "empty").
- **Flags:** smoke columns above camps/fortress (particles only), crows circling the
  fortress, and the five `ruins.rs` biome landmarks raised/scaled up + a scatter-clear radius
  so each is visible from its road.

### Pass 5 — Vista pass
- 3–4 authored overlooks: flattened cliff-edge shelf + framing prop (lone tree / ruined arch)
  + composed foreground/midground/background view.
- Canyon framing checked as a compression→release shot.
- Castle spire as the always-visible orientation anchor (raise it / clear sightlines).

## Constraints & invariants

- `navgrid::can_step` |Δy| ≤ 0.6 stays the walkability law — cliffs are walls, passes are
  ramps. Every region must remain reachable via at least one pass (test).
- Invader waves + camp spurs must path through passes; no wave spawn may be walled off.
- Roads stay unobstructed: clearance check for every placed POI/prop.
- Determinism: all new placement uses seeded `mulberry32`, same as existing scatter.
- Perf: new props ride the existing chunk-merge + shared white material batching; tree-class
  (individual entity) additions kept modest. Density levers back off if the 2.2 map stutters.
- Save/load: no new run-state expected (all world-gen); if any POI becomes lootable/
  discoverable it must round-trip `SaveData` per the savegame checklist.
- Each pass: verify with capture harness (orbit clips per biome, `FOREST_TPS` walk clip along
  the main road for the 40s-rule check), then commit + push.

## Verification of the whole

Final acceptance: a `FOREST_TPS` + `FOREST_DEMO=explore` walk clip along the castle→rock road
and castle→swamp road each showing (a) the road reading clearly, (b) ≥1 POI/flag per ~40s,
(c) at least one framed vista; plus per-biome orbit clips showing distinct identity.
