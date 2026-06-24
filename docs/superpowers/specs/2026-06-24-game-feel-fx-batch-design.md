# Game-feel FX batch — footprints, dust kicks, campfire embers

**Date:** 2026-06-24
**Status:** approved scope, ready to implement
**Author:** brainstormed with the user (Warbell)

## Goal

A small batch of cheap, high-feel particle/decal effects that make the world *answer* the
player — without measurably hurting performance. All three reuse the existing batched
mote / FIFO-capped ground-decal / spawn-and-fade pipeline, so the cost is negligible.

Explicitly **out of scope** (decided during brainstorming):
- **No combat-hit FX changes.** Outgoing hero hits already carry blood spray, slash glint,
  ground shockwave, blood splat, blade trail, hurt-flash, squash, screen-shake and hit-stop
  (`player/combat.rs`). Block-spark / hero-hit-contact ideas were considered and cut — the
  user judged combat feedback already good enough.
- **No ork footprints.** Hero-only footprints for now; ork siege prints can come later if the
  hero version lands well.

## The three effects

### 1. Hero footprint trail — new `src/footprints.rs`

Stamp a flat ground decal under each footfall so the hero leaves tracks.

- **Trigger:** the *same* footfall gate the dust puff already uses
  (`footstep_fx::emit`: `hero.walk_phase / PI` half-cycle edge, gated on `Modal::None`,
  only while `hero.moving && hero.on_ground`). Rather than duplicate the gate, a new
  `footprints` emit system runs the identical half-cycle detection (its own `Local<i64>`),
  reading the shared `Hero`.
- **L/R alternation:** half-cycle index parity picks the foot; offset the print sideways
  from `hero.pos` by `±FOOT_OFFSET` along the vector perpendicular to `hero.facing`
  (`right = (cos facing, -sin facing)`), and yaw the decal to `hero.facing` so the boot
  points along the heading.
- **Mesh:** one shared flat decal mesh — a small boot-ish shape (a rounded sole quad/oval,
  or a heel disc + ball disc merged), authored in XZ with normal up, base at `y=0`. Built
  once at startup. Same `depth_bias` trick as `aftermath.rs`
  (`GROUND_DECAL_DEPTH_BIAS`-style) so it doesn't z-fight the cobble courtyard slab.
- **Per-print fade:** each print owns an alpha-blended `StandardMaterial` clone (the proven
  `aftermath::BloodStain` pattern) so it can fade out solo; the handle is freed on despawn so
  clones never leak. A `Footprint { born, mat, hold, fade }` component drives the ramp.
- **Biome-aware look + persistence** (via `worldmap::biome_at_world`, exactly as
  `footstep_fx::surf_mat` already branches):
  | Biome | Tint | Hold (s) | Fade (s) | Notes |
  |---|---|---|---|---|
  | Snow | bright blue-white | long (~20) | long (~15) | reads as pressed snow; ambient snowfall visually "refills" the trail as it fades |
  | Swamp | dark wet brown | medium | medium | muddy churn |
  | Desert | pale tan | short | short | shallow, wind-scoured |
  | Forest / dirt (default) | faint dark | short | short | a soft scuff |
  | Rocky / grass | **none** | — | — | hard/elastic surface leaves no print (skip stamping) |
- **Bound:** a FIFO `VecDeque<Entity>` cap (~56 prints) reaps the oldest with `try_despawn`,
  mirroring `aftermath::MarkLog`. Combined with the fade, the trail stays a believable few
  strides long.
- **Lifecycle tags:** `BiomeEntity` (so a biome swap / world rebuild wipes them like any
  dressing) + `NotShadowCaster`.
- **Gating:** the emit system is gated on `Modal::None` (no stamping through a paused/panel
  frame); the fade system runs ungated so prints keep settling while frozen, matching
  `footstep_fx`.
- **Not saved.** Pure transient dressing (like footstep puffs / aftermath blood) — no save
  round-trip, no reset wiring needed beyond the `BiomeEntity` rebuild sweep.

### 2. Sprint / landing dust kicks — extend `src/footstep_fx.rs`

Reuse `footstep_fx::spawn_puffs` (the existing golden-angle mote burst) for two new beats.
No `movement.rs` edit — every signal is already on `Hero`.

- **Sprint kicks:** in the existing `emit`, scale the per-footfall puff by `hero.run_amt`
  (the smooth 0..1 sprint blend): a sprinting footfall throws **more** motes, **faster**, and
  biased to trail *behind* the heading (kick-back), versus the gentle walk puff. Walk feel is
  unchanged at `run_amt ≈ 0`.
- **Landing kicks:** a new check (own `Local<bool>` tracking the `on_ground` edge) detects a
  touchdown (`!prev && hero.on_ground`). Estimate fall height from `hero.air_takeoff_y -
  hero.y` at the edge; above a small threshold, spawn a fatter **radial** ground burst
  (count/speed scaled by fall height, clamped) — the landing thump kicking up dust. Surface
  tint via the existing `surf_mat`. Reuses the same `spawn_puffs`/`fade_puffs` path.
- Cost: a few extra motes on sprint footfalls + one burst per landing. Negligible.

### 3. Campfire / brazier embers — extend `src/firelight.rs`

Rising ember motes off every fire at night.

- **Sources:** the `FireLight` entities already mark every campfire (`camps.rs`) and castle /
  gate torch (`castle.rs`) and carry a `base` intensity + `phase`. Reuse their world
  `GlobalTransform` as emitter points — no new spawn wiring in the fire owners.
- **Night-gated + throttled:** sample `scene::night_of(SkyClock.t)` (the same nightfall ramp
  the flicker uses). Only emit after dusk; throttle so each fire spits an ember every so often
  (a per-system accumulator + a cheap per-fire hash off `phase`/index so they don't all puff
  in lockstep), keeping the live ember count low.
- **Ember mote:** a tiny warm-emissive sphere (so bloom kisses it), spawned just above the
  flame, given a slow upward velocity + slight sideways drift, **no gravity** (embers rise),
  flickering/shrinking over a short life then despawned. A dedicated lightweight
  `Ember { vel, life, life0 }` + its updater, kept self-contained in `firelight.rs` with one
  shared mesh + one shared emissive material built at startup (no per-ember material — heeds
  the font-atlas-leak lesson: shared material, fade by shrink).
- **Lifecycle:** `NotShadowCaster`. Embers are short-lived and self-reaping; bound is the
  throttle × life, so no FIFO cap needed. Tagging `BiomeEntity` is optional (they vanish in
  well under a second anyway).
- **Ungated** drift/fade (lighting/atmosphere keeps breathing through a panel, like the fire
  flicker itself); the *spawn* can be gated on `Modal::None` so a paused night doesn't accrue.

## Wiring

- `main.rs`: add `mod footprints;` + `.add_plugins(footprints::FootprintPlugin)`. Embers ride
  the existing `FireLightPlugin`; sprint/land kicks ride the existing `FootstepFxPlugin`.
- New file: `src/footprints.rs`.
- Touched: `src/footstep_fx.rs` (sprint scaling + land burst), `src/firelight.rs` (embers),
  `src/main.rs` (footprint plugin).

## Performance notes

- Everything is shared-mesh + shared-or-FIFO-capped-clone + spawn-and-fade, the same pattern
  the existing weather/footstep/aftermath systems already run at thousands of instances.
- The one place that mints per-instance materials (footprint fade clones) is FIFO-capped at
  ~56 live and frees each handle on despawn — bounded, matching `aftermath` blood stains.
- No per-frame unique font sizes / materials anywhere (the documented atlas-leak trap).

## Testing / verification

Pure rendering dressing (no `crates/core` logic), so no unit tests. Verify visually with the
screenshot harness:
- Footprints: `FOREST_HERO` into snow/swamp + an `FOREST_ANIMTEST=walk` or a short clip to
  see a trail; confirm prints alternate L/R, face the heading, tint per biome, fade, and never
  exceed the cap.
- Land/sprint kicks: a clip of a sprint + a jump-down off a ledge.
- Embers: a night camp / gate-torch shot (`FOREST_NIGHT`, a camp biome) — embers rising,
  bloom-lit, not in lockstep.
- Build once (single session) and eyeball; no parallel-dispatch constraint here.
