# Lush Foliage & Ambient Life — design

**Date:** 2026-06-28
**Goal:** Make the world read prettier and more alive ("more nature") without reworking the
already-good tree/biome models. First batch of three independent, low-risk wins covering the
**ground**, the **air**, and the **sky**:

1. **Grass wind** — the merged grass carpet bends in the wind (vertex shader).
2. **Falling leaves + butterflies** — drifting leaves tie the canopy to the air; butterflies add
   daytime life near the ground.
3. **Bird flock** — V-silhouettes glide across the sky for sense-of-scale.

Deferred from this batch (chosen earlier, explicitly out of scope here): **denser undergrowth**
clustering under tree bases. Revisit after this lands.

## Context — what already exists (do not rebuild)

- **Trees** (`trees.rs`): 7 kinds, facet-baked painterly shading, per-instance tint variety, root
  flares. Already good. **Trees already sway** (`wind.rs`: per-entity base-pivot lean, distance-gated
  at 70u, freqs 1.5 / 3.1 / 1.2).
- **Ground cover** (`groundcover.rs`): grass tufts / ferns / mushrooms / flowers / clover. **Baked
  into merged static per-chunk meshes** (`biome.rs::merge_props` → `spawn_chunks`), one cover mesh
  per chunk against the shared white vertex-colour `StandardMaterial`, `NotShadowCaster`, culled at
  80u (`VisibilityRange`). This is why grass can't use the per-entity `Sway` trick — there are no
  per-blade entities.
- **Weather particles** (`particles.rs`): a CPU drift system that follows the hero. One
  `ParticleKind` per active biome (snow / dust / fireflies / pollen / ash / mist), faded in/out at
  biome edges. `drift` translates motes, gusts the directional ones, and yaws `billboard` cards to
  face the camera. Clean to extend.
- **Custom WGSL is in-scope**: the project already ships `assets/shaders/water.wgsl` and
  `terrain.wgsl`, so a grass vertex shader fits the existing pattern.

---

## Component 1 — Grass wind (vertex-shader bend)

**Approach (decided):** a dedicated material for cover chunks ONLY, with a vertex shader that bends
each vertex horizontally by a per-vertex **bend weight** (0 at the blade base, 1 at the tip).
Props and trees keep the plain shared white material.

### Material
- New `GrassWindExt` extension → `ExtendedMaterial<StandardMaterial, GrassWindExt>`, registered by a
  small `GrassWindPlugin` (mirrors `WaterPlugin`: plugin only registers the material; `biome.rs`
  uses the handle for cover chunks).
- The base `StandardMaterial` half stays identical to the current cover material (white base, vertex
  colours supply the painterly tones) so the look is unchanged when wind = 0.
- Uniforms: wind `strength` (radians-equivalent XZ offset scale), and time comes from the engine
  globals already bound in Bevy's mesh view bind group (no custom time uniform needed).

### Vertex bend weight (the load-bearing detail)
- Encode the bend weight in vertex **`COLOR.alpha`**. The opaque pass ignores vertex alpha for
  blending, so the channel is free and survives the `merge`/upload path (unlike UV_0, which the
  comment in `merge_props` notes is stripped).
- **Bake it per-prop in LOCAL space, BEFORE the transform is baked into the chunk** (props have base
  at y=0, tip at y=`h`): `alpha = clamp((y - y_min) / (y_max - y_min), 0, 1)` computed over that one
  prop's verts. Computing it after the chunk transform bake would be wrong — a chunk on a hill has
  high world-y everywhere, so every vertex would read as "tip".
- Do the baking in the cover assembly path (a helper applied to each cover `PendingProp` before
  transform-bake + merge), so no individual `groundcover.rs` builder needs editing.
- Consequence: ferns/flowers/mushrooms in the same cover bucket also get a height-proportional bend.
  That's fine (they should sway a little too); keep `strength` modest so short props barely move.

### Shader wind function
- Displace world XZ by `weight * strength * windfn(world_xz, t)`, reusing `wind.rs`'s frequencies so
  grass and canopies move coherently:
  - `dx = (sin(t*1.5 + p) + 0.4*sin(t*3.1 + p*1.7)) * AMP`
  - `dz = cos(t*1.2 + p*1.1) * AMP`
  - phase `p = world.x*0.7 + world.z*0.55` (the same hash `wind.rs::sway_for` uses).
- Apply in the vertex shader after world-position is computed; only the XZ offset, no Y, so blades
  shear rather than lift.

### Wiring & cost
- `biome.rs::spawn_chunks`: the **cover** chunk uses the grass-wind material handle; the **props**
  chunk keeps the plain material. One extra material/batch for cover (the codebase already accepts
  this split pattern for ork props, water, etc.).
- GPU vertex-only work on meshes already culled at 80u → negligible. No new CPU per-frame system.
- `FOREST_NOGRASS` and the existing `VisibilityRange` cull keep working unchanged.

### Risks
- If `COLOR.alpha` turns out to be read somewhere for cover (it should not be — opaque), fall back to
  a spare UV channel re-added post-merge. Verify during implementation.
- Keep `AMP` small (start ≈ 0.05–0.08 world units at the tip) — over-bending reads as underwater
  kelp, not a breeze.

---

## Component 2 — Falling leaves + butterflies

### Falling leaves — `ParticleKind::Leaves` (in `particles.rs`)
- Add `Leaves` to `ParticleKind` and the `spawn` match: small **tumbling quads** (a `Rectangle`
  mesh, double-sided, unlit) in warm leaf tones (reuse the `AUTUMN_*` / `FOLIAGE_*` palette range,
  per-instance varied), slow **downward + sideways** drift (`vel ≈ (0.4, -0.6, 0.2)`), spawned in a
  low band over the canopy height.
- Add a `tumble: bool` flag to `Particle`. In `drift`, tumbling particles also spin (rotate about a
  per-instance tilted axis driven by `t + phase`) so leaves flutter rather than slide flat. This is
  the only `drift` change; existing kinds pass `tumble: false`.
- Biome gating: surface `Leaves` for the green biomes (forest + grass frontier) by day via the
  existing `BiomeAmbiences` picker — i.e. forest's day ambience becomes leaves (or pollen→leaves;
  decide at impl which reads better; only one mote kind is active at a time).

### Butterflies — in the new `ambient_life.rs` plugin (Component 3), NOT weather
- Butterflies are *creatures*, not precipitation, and should coexist with whatever weather mote is
  active — so they live in the new plugin, not the single-kind weather field.
- ~10–14 small colored wing-pairs (two angled quads sharing an edge, per-instance hue), **unlit**,
  bobbing near the ground (y ≈ 0.3–1.5) in a hero-following box, present in green biomes by day.
- Motion: gentle wander (sinusoidal XZ drift + bob) plus a fast wing-flap (open/close the two quads)
  for life. Cheap; no shadow.

---

## Component 3 — `ambient_life.rs` plugin (butterflies + bird flock)

A new self-contained plugin (mirrors `decor.rs`: own `Startup`/`Update`, deterministic, hero/day
gated). Owns butterflies (above) and the bird flock.

### Bird flock
- ~6 dark **V-silhouettes** (two thin angled quads/boxes per bird forming a shallow V), nearly
  black against the sky, gliding **high above** the hero (y ≈ hero + 20–35u).
- Fly as a loose flock along a slow heading; gentle wing-flap (rock the two wing halves) and a small
  per-bird bob. When a bird exits the hero-following box on the lee side, **re-loop** it to the
  windward edge with a fresh lateral offset (same wrap idea as the weather box).
- Day-gated; optionally not always present (e.g. a flock crosses periodically) — start with
  always-on, tune frequency after seeing it. `NotShadowCaster`, unlit dark material.

### Gating shared by both
- Active only in `AppState::Playing` and `Modal::None` (don't animate through pauses/panels —
  follows the freeze-gate convention).
- Biome-aware: butterflies in green biomes; birds anywhere outdoors (sky reads everywhere). Reuse the
  hero XZ / weather-center pattern for the follow box.

---

## What this is NOT (scope guard)

- No new tree/creature models, no terrain changes, no lighting/atmosphere rework (that was a separate
  ranked option, deferred).
- No undergrowth densification (deferred).
- No save/reset obligations: all three are **transient ambient** (like existing weather motes and
  fireflies) — nothing a player earns/changes — so they do NOT touch `savegame.rs`. They are tagged
  for the biome-swap despawn/rebuild where appropriate (`BiomeEntity`) like the weather field.

## Testing / verification

- `crates/core` unit tests unaffected (pure rendering/feel work).
- Visual verify via the screenshot/clip harness:
  - Grass wind: a `FOREST_CLIP` of a grass meadow (compare static vs swaying); confirm tree+grass
    move coherently.
  - Leaves/butterflies: `FOREST_BIOME` forest, day, `FOREST_TPS` walk clip.
  - Birds: a `FOREST_CLIP` with the sky in frame.
  - Always confirm the `Screenshot saved` / no-`Validation` log line (capture-flake rule).

## Files touched

- New: `assets/shaders/grass_wind.wgsl`, `src/grass_wind.rs` (material + plugin), `src/ambient_life.rs`
  (butterflies + birds).
- Edited: `src/particles.rs` (`Leaves` kind + `tumble`), `src/biome.rs` (cover bend-weight bake +
  grass material on cover chunks; `BiomeAmbiences` leaves gating), `src/main.rs` (add the two new
  plugins).
