# Cinematic atmospherics — height fog + cloud light patches + filmic grade (2026-07-02)

Goal: close the gap to the reference mock (warm layered haze, wide soft god rays, patchy
cloud light on the ground, filmic desaturated grade, readable DOF) as the DEFAULT High+ look.

## Approach (approved: variant A)

One new fullscreen post pass (`src/atmospherics.rs` + `assets/shaders/atmospherics.wgsl`),
same Core3d `PostProcess` ping-pong shape as `dof.rs`/`godrays.rs`, pinned
`tonemapping → smaa → **atmospherics** → godrays → outline → dof` so god rays scatter the
already-hazed image and cloud patches sit under the rays.

The pass reads prepass depth, reconstructs world position (uniform carries
`world_from_clip`), and does three things:

1. **Height fog / aerial perspective** — analytic exponential height fog
   (density falls off with altitude, integrates along the view ray) plus a distance term,
   with sun in-scatter: fog colour lerps toward a warm glow colour by
   `pow(dot(ray, sun), exp)`. Colours are reused from the live `DistanceFog`
   (already time-of-day + biome + war-dusk driven) so all existing mood systems keep working.
2. **Cloud light patches** — scrolling 2-octave value noise in world XZ multiplies scene
   brightness (soft ±, big ~60-unit features) on geometry pixels, faded by daylight.
   Post-based: no touching the light/shadow systems.
3. **Fog noise** — the same noise field modulates fog density ±, so the haze reads as
   drifting atmosphere, not a uniform veil.

Gating: rides the same High/Ultra gate as god rays (`quality.rs::apply_quality`,
component insert/remove; the pass's ViewQuery skips itself when absent). Low unchanged.
Driver (`drive_atmospherics`) mirrors `drive_godrays`: daylight gate, sun dir, colours from
`DistanceFog`, `FOREST_ATMO` env override for screenshot-harness tuning without rebuild.

## Retuned existing knobs

- god rays: wider/stronger/longer (intensity, weight, decay up; threshold down; on-screen
  fade window widened so shafts survive a sun nearer the frame edge).
- grade: `post_saturation` 1.1 → ~0.95 (filmic), `midtones.contrast` 1.5 → ~1.2,
  slight warm `temperature`; night/hit reactive grade untouched.
- DOF: sharp band narrowed + shorter far ramp so background/foreground actually melt
  (range 75/far_ramp 130 meant no visible blur in normal play).
- DistanceFog: warmer in-scatter (`directional_light_exponent` 12 → lower = wider glow).

## Rejected

- Bevy `VolumetricFog`/`FogVolume` return — retired for cause (blacked out Atmosphere sky,
  frame cost, imperceptible shafts at our fog settings). Screen-space stays.
- Real cloud-shadow cookies on the directional light — Bevy has no directional light
  textures; post-space patches give the look for a fraction of the complexity.

## Verify

`FOREST_SHOT` + `FOREST_TPS=1` at the forest path (windmill framing, morning sun in frame),
compare against the mock; iterate via `FOREST_ATMO`/`FOREST_GODRAYS` before hardcoding.
