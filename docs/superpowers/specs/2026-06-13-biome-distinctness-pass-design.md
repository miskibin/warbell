# Biome distinctness pass — design

**Date:** 2026-06-13
**Goal:** Each biome should read as an unmistakably different place. Strengthen the
per-region atmosphere tint, wire fog thickness, sweep weather particles, and — the
headline — give the **ork castle (the Blight) its own red-ember atmosphere** instead of
silently inheriting swamp's grey-green mood.

Locked decisions (from brainstorming):
- Lever = **strengthen atmosphere + particles**. No new full-frame post-FX color-grade layer.
- **Daytime only** — the tuned uniform moonlit night stays; biome mood blends in by `day`.
- Ork weather = **ash/ember motes** (new `ParticleKind::Ash`).
- Swamp/Blight fog = **strong** (fog wall ~45–120 tiles vs the open island's 85–190).

## Root causes being fixed

1. **The Blight has no atmosphere of its own.** `worldmap::tile_biome_world` collapses
   `TB::Blight → Biome::Swamp` (`worldmap.rs:480`), so the ork seat of power inherits swamp's
   atmosphere sample. Deliberate for *gameplay* (poison/slow/wildlife/forage), but it means the
   ork capital looks like a swamp. We diverge the **mood only**, leaving gameplay as swamp.
2. **`fog_density` is dead data.** Each biome authors `fog_density` (swamp `0.030`, the thickest)
   but nothing reads it — `biome.rs` sets one fixed `FogFalloff::Linear { 85, 190 }` for the
   whole map. So "swamp isn't foggy" is literal: its authored fog never applies.
3. **The tint is timid.** `BIOME_TINT_W = 0.7` and the authored atmosphere values sit close
   together, so neighbouring biomes read nearly the same in daylight.

## Mechanism changes

### 1. `AtmoSample` carries fog thickness (`biome.rs`)
Add `pub fog_density: f32` to `AtmoSample`. `from_config` sets it from `c.fog_density`;
`from_raw` takes it as a new arg (the island base uses `ATMOSPHERE.1`, currently `_fog`).

### 2. Surface the Blight as its own ambience (`biome.rs` + `worldmap.rs` + `ork_fortress.rs`)
- `BiomeAmbiences` gains `pub blight: BiomeAmbience`.
- New method `BiomeAmbiences::sample_world(wx, wz) -> BiomeAmbience`: returns `self.blight`
  when `crate::ork_fortress::in_blight_world(wx, wz)`, else `self.sample(biome_at_world(wx, wz))`.
  This is the single world-space ambience lookup; the biome-enum `sample` stays for callers that
  already have a `Biome`.
- New `ork_fortress::blight_ambience() -> BiomeAmbience` returns the red-ember atmosphere + `Ash`.
- `worldmap::build` populates `blight: crate::ork_fortress::blight_ambience()` when it inserts
  `BiomeAmbiences`.

### 3. Consumers use the world-space lookup (`scene.rs`, `particles.rs`)
- `scene::track_biome_atmo` → `ambiences.sample_world(hero.pos.x, hero.pos.y).atmo`.
- `particles::update_weather` → `ambiences.sample_world(focus.x, focus.z).particle`.
Crossing into the Blight now eases the mood toward red-ember and swaps the weather to ash —
while standing in the open swamp still reads swamp.

### 4. Wire fog thickness, by region, daytime-gated (`scene.rs::advance_sky`)
- Add `fog_density` to the eased `SmoothBiomeAtmo` (lerp like the other scalars).
- Map density → fog distances:
  `t = clamp((d - 0.009) / (0.036 - 0.009), 0, 1)`,
  `start = 85 - 43·t`, `end = 190 - 75·t`.
  Forest `d=0.009` → `(85,190)` (unchanged baseline); swamp `d=0.034` → `(~45,120)`;
  Blight `d=0.036` → `(~42,115)`.
- Blend toward baseline by `(1 - day)` so night returns to the standard distances (keeps siege
  readable). Apply as `fog.falloff = Linear { start, end }` each frame.
- **Honor `FOREST_FOG`:** when that env override is set, skip the dynamic pull (the manual
  distances win, as today).

### 5. Stronger tint (`scene.rs`)
`BIOME_TINT_W: 0.7 → 0.82`.

### 6. New `ParticleKind::Ash` (`biome.rs` enum + `particles.rs` spawn table)
Dark embery motes with a faint warm glow and a slight *upward* drift (embers rise), modest
count. Tuned next to the existing Dust/Snow entries.

### 7. Swamp mist = soft fog-bank cards, NOT motes (`particles.rs`)
The mote-sphere `Mist` preset read as cheap floating dots. **Reactive volumetric fog was tried
first and abandoned** — the scene's `FogVolume` exists only as the medium for the separate
screen-space god-ray pass (the camera's `VolumetricFog` runs `ambient_intensity: 0`), so driving
its `density_factor`/`fog_color` produces no visible haze (proven: forcing `density 0.6` magenta
rendered nothing). Instead `ParticleKind::Mist` now spawns a handful (~34) of BIG soft-alpha,
camera-facing cards (a procedural radial-alpha sprite on a unit quad) hovering low over the mire,
drifting slowly — rolling ground-fog banks. Reuses the weather fade/despawn lifecycle and the
`Particle` drift box; a new `billboard` flag makes `drift` yaw the cards to face the camera
(upright). Renders on **every** preset, unlike the Ultra-only volumetric pass.

## Authored atmosphere targets

Final values dialed in via the `FOREST_SHOT` / `FOREST_HERO="x,z"` harness; these are the
starting points (sRGB hex).

| Biome | sky | sun_color | sun_illum | ambient_color | ambient_br | fog_density | particle |
|---|---|---|---|---|---|---|---|
| Forest | `0xaed3e6` | `0xffe9bb` | 10500 | `0xd8ecd6` | 85 | 0.009 | **Pollen** (was None) |
| Snow | `0xd4e6fb` | `0xfaf0e6` | **12000** | `0xb6d2f7` | **128** | 0.013 | Snow |
| Swamp | `0x76857a` | `0xb6c499` | **6400** | `0x97aa8e` | **64** | **0.034** | **Mist** = fog-bank cards (was None) |
| Rocky | `0xccc8be` | `0xffe6b8` | 11000 | `0xe4ddca` | 88 | 0.012 | Dust |
| Desert | `0xf2e0a8` | `0xfff0c4` | **15500** | `0xffeec2` | **136** | 0.013 | Dust |
| **Blight** | `0x5a2418` | `0xa8895a` | 6000 | `0x6a6048` | 58 | **0.036** | **Ash** |

The Blight's sooty-red `sky` is what reddens the horizon: `advance_sky` lerps the daytime
fog color toward the region's `sky` by the tint weight, so the fog band facing out over the
mire glows blood-red, the sun dims sickly-bronze, and the fill goes ash-green.

## Files touched
- `src/biome.rs` — `AtmoSample.fog_density`; `BiomeAmbiences.blight` + `sample_world`; `ParticleKind::Ash`.
- `src/ork_fortress.rs` — `blight_ambience()`.
- `src/worldmap.rs` — populate `blight` slot.
- `src/scene.rs` — `sample_world` in `track_biome_atmo`; fog-density pull + `BIOME_TINT_W`.
- `src/particles.rs` — `sample_world`; `Ash` spawn tuning; verify Mist/Pollen read.
- `src/biome_{forest,snow,swamp,rocky,desert}.rs` — pushed atmosphere/particle values.

## Out of scope (YAGNI)
- No `Biome::Blight` enum variant — gameplay stays swamp; only the visual ambience diverges.
- No night-time biome distinctness; no full-frame per-biome color grading.
- No directional/azimuthal sky gradient — the red horizon comes for free from fog-toward-sky.

## Verification
Screenshot each region with `FOREST_HERO="x,z"` (centres: swamp `0,57`; Blight near `12,103`;
desert `60,-39`; snow `-69,-45`; rock `66,4`; forest `-60,39`) + `FOREST_SHOT`. Confirm: red
horizon + ash over the Blight, soupy fog + mist in swamp, and five clearly different daytime
moods. `cargo check` + a normal `cargo run` smoke test.
