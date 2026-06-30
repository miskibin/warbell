# Organic Road Network — design

**Date:** 2026-06-30
**Status:** approved, implementing

## Goal

Replace the current four straight gate-stub roads with a **map-wide network of natural,
curving dirt paths** that connect the castle to every interesting place on the island. Paths
wander like real trails, **nothing grows on them**, the player can choose to travel mainly by
road, and the player moves **a little faster** on a road. Crossing a river auto-spawns a
**bridge**. The whole thing must be **perf-safe** — heavy work runs once at startup, every
runtime consumer is an O(1) lookup.

This supersedes the `roads.rs` draft (straight `dist_to_segment` spokes from `castle::gate_centers()`,
24 units long, consumed only by `worldmap::ground_color`).

## Topology (decided: "B + a little C")

Hub-and-ring, organic curves, lightly branched — not chaotic.

- **Nodes** (world-space, castle = origin):
  - castle (0,0)
  - 5 biome centres: snow (−69,−45) · desert (60,−39) · rock (66,4) · forest (−60,39) · swamp (0,57)
  - ork fortress (south of swamp) and rival stronghold (NE desert) — read from their modules' centre consts.
- **Edges:**
  - **Trunks** — castle → each biome centre; castle → fortress; a branch to rival.
  - **Ring** — links *adjacent* biome centres around the island so the player can circle without
    backtracking through the centre.
  - **Spurs** — a small, **capped** number (≤6) of short branches off a passing trunk to nearby
    minor POIs (camps / chests). The capped spur count is what keeps it organic-not-chaotic; the
    main organic feel comes from the wander, not branch density.

## Curve generation (decided: jittered-waypoint Catmull-Rom)

Each edge `A→B` becomes a smooth wandering polyline:

1. Place `k` intermediate waypoints evenly along the straight `A→B` (k scales with length).
2. Jitter each waypoint perpendicular/laterally using seeded `mulberry32` (core `rng.rs`) keyed
   by edge index — fully deterministic, reproducible every boot.
3. **Sanity-nudge**: if a jittered waypoint lands on deep water or steep slope, pull it back
   toward the straight line until acceptable (bounded iterations; give up gracefully). Paths may
   still cross *rivers* (narrow) — that's intentional, it makes bridges.
4. Smooth the endpoints + waypoints into a Catmull-Rom curve, emit a dense centreline polyline.

Tunables (top of `roads.rs`): waypoint density, jitter amplitude, road half-width, edge-falloff,
spur cap, spur max-length.

## The baked field (the perf core)

`RoadField` — a `Resource` holding a 2D grid covering the island AABB:

- One `f32` per cell = road strength `[0,1]` (1 = packed centre, soft falloff to 0 at edge).
- Cell size ~0.5–1.0 world units (≈300×300 cells → ~0.4–1.4 MB; trivial).
- Built **once** at `Startup`, after `worldmap::build`, by rasterising every curve: walk each
  centreline in small steps, stamp a radial brush with `max`-blend into nearby cells.
- A `sample(wx,wz) -> f32` does a bilinear lookup → smooth, cheap, allocation-free.

Every runtime consumer samples the field; **no consumer re-walks splines**:

| Consumer | Where | Cost |
|---|---|---|
| Ground tint (existing) | `worldmap::ground_color` keeps calling `roads::road_strength`, now backed by the field | per-vertex, gen-time only |
| **Nothing grows** | trees / groundcover (and rocks/decor) reject an instance if `sample > GROW_CUTOFF` (~0.45) | one lookup per scatter point, gen-time |
| **Little faster** | `player::movement` multiplies speed by ~1.12 when `sample(hero) > ROAD_CUTOFF` | 1 lookup/frame |
| **AI bias** (villagers / war party / orks prefer roads) | `navgrid` lowers per-cell A* step cost where `sample > ROAD_CUTOFF` | 1 lookup/cell, baked into cost |
| **Bridges** | where a centreline crosses a river tile, emit a bridge via `bridges.rs` | gen-time |

`roads::road_strength(wx,wz)` is kept as the public query (so `ground_color` is unchanged) but
now just delegates to `RoadField::sample`. The old `dist_to_segment` spoke math is removed.

## River / bridge coordination (parallel-work hazard)

Rivers are being reworked by another agent right now (`worldmap.rs` mid-edit). The bridge step
depends on the **final** river query. So:

- The road network + field + nothing-grows + faster-movement + AI-bias ship **independently** of
  rivers — they don't need the river query.
- Bridge emission is an **isolated, last** step that calls whatever "is this world point a river?"
  query the river rework settles on. If that query isn't stable yet, land everything else and wire
  bridges when rivers settle. Do **not** edit the river generation; only read its query.
- Likewise avoid stomping the in-flight `player/movement.rs` edit: the speed bonus is a small
  additive change at the speed-application site.

## Determinism, save/load, reset

The field is **pure derived data** — a deterministic function of the fixed world seed + map id.
It is rebuilt at every `Startup` / in-process world rebuild, identical each time. It is **not**
run-state: the player neither earns nor changes it. Therefore:

- **No save** needed (nothing to persist — it regenerates).
- **No reset** needed (the world rebuild already reconstructs it).

This sidesteps the persist+reset obligation in CLAUDE.md by construction. (If a *future* feature
lets the player carve/alter roads, that delta would need both — out of scope here.)

## Module / plugin shape

- `src/roads.rs` becomes a `RoadsPlugin`:
  - data: node list, edge list, curve builder, `RoadField` + sampler.
  - `Startup` system `build_road_field` ordered **after** `worldmap::build`, inserting `RoadField`.
  - a later `Startup`/post step for bridge emission (river-query dependent, isolated).
- Consumers add a single field sample at their existing accept/reject/speed/cost sites.
- Keep all forest-specific world geometry in `src/` (not `crates/core`) — it's not parity logic.

## Testing

Pure helpers get unit tests (no Bevy needed): Catmull-Rom interpolation continuity, waypoint
jitter determinism (same seed → same polyline), field stamp + bilinear sample monotonicity
(strength 1 at centre, decreasing outward, 0 far away), and that every node is reachable in the
edge graph. Visual verification via `FOREST_SHOT` top-down over the island + a `FOREST_TPS`
walking shot down a trunk road.

## Out of scope (YAGNI)

Player-editable/upgradeable roads; road-side props (mileposts, lamps); seasonal mud/snow cover on
roads; per-biome road material variation. The field design leaves room for these later.
