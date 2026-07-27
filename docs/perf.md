# Performance log

FPS measurements from real runs (read off the **F2** stats overlay), tracked across
optimization passes so we can see whether a change actually moved the needle on the machines
that matter (weak/integrated GPUs — the strong ones are never the bottleneck).

**How to read this:** each row is one *run* on one machine at one graphics preset, in a named
scenario. We care most about the **worst realistic case** (a full night siege) and the
**default boot experience** on a modest GPU. Numbers are eyeballed off F2, so treat ±2 FPS as
noise.

> Fill this in from your own `cargo run` sessions. When you add an optimization, re-measure the
> SAME scenario+preset+machine row so the before/after is apples-to-apples, and note the commit.

## Test machines

| Tag | CPU | GPU | Notes |
|---|---|---|---|
| `igpu-strong-cpu` | strong desktop CPU | integrated GPU | GPU-bound — the representative "will it run on a laptop" case |
| _(add yours)_ | | | |

## Scenarios

- **boot-day** — fresh boot, standing in town during Prep, daytime, no siege (the default first impression).
- **siege** — full night wave assault (worst realistic case: many invaders + combat FX + shadows).
- **town-crowd** — large settlement, many villagers/guards milling (CPU-heavy on weak CPUs).

## Measurements

| Date | Commit | Machine | Preset | Scenario | FPS | ms/frame | Notes |
|---|---|---|---|---|---|---|---|
| 2026-06-18 | `396a25a` (pre-perf) | `igpu-strong-cpu` | Low | boot-day | 24 | 41.2 | baseline; `main_opaque_pass_3d` 15.7ms + sky/IBL ~6.7ms dominate |
| 2026-06-18 | `396a25a` (pre-perf) | `igpu-strong-cpu` | Ultra | boot-day | 14 | 70.5 | baseline; `ssao` 14.9ms + opaque 11.1ms + 4 shadow cascades |
| 2026-06-18 | `71138a0` | `igpu-strong-cpu` | Low | spawn(0,-15) cull OFF | 26 | 39.0 | `FOREST_NOCULL=1`; main_opaque 21.95 — camera NOT pinned (see caveat) |
| 2026-06-18 | `71138a0` | `igpu-strong-cpu` | Low | spawn(0,-15) cull ON | 37 | 27.3 | main_opaque **10.14** — ≈2× lower opaque, +42% FPS vs cull OFF |

**A/B caveat:** the two rows above used the same hero spawn but the follow-cam was free, so the
views weren't byte-identical — the Ultra pair from the same session was unreliable (Ultra-OFF
main_opaque 10.35 < Low-OFF 21.95, impossible for one view, proving the camera differed). The Low
pair is still a strong signal and matches theory (abrupt VisibilityRange can only cull, never
inflate — verified in bevy_camera source). To get a byte-identical A/B, use the **pinned protocol**
below.

### Pinned A/B protocol (PowerShell)

Boot into a fixed fly-cam so the frame is identical across runs (don't touch mouse/keyboard):

```powershell
# culling ON (current default), fixed outward view over the island:
$env:FOREST_FREEROAM="1"; $env:FOREST_CAM="0,30,60,0,0,-30"; cargo run
# culling OFF, SAME view:
$env:FOREST_NOCULL="1"; $env:FOREST_FREEROAM="1"; $env:FOREST_CAM="0,30,60,0,0,-30"; cargo run
# cleanup so normal play isn't staged:
Remove-Item Env:FOREST_FREEROAM,Env:FOREST_CAM,Env:FOREST_NOCULL -ErrorAction SilentlyContinue
```

Read `main_opaque_pass_3d` from F2 in each — identical view, so the delta is purely the culling.

**GOTCHA — `FOREST_PERFTEST` does NOT imply `FOREST_NOVSYNC`, and the saved config ships
`vsync: true`.** Without `FOREST_NOVSYNC=1` every run reports a vsync-pinned frame time and any A/B
is meaningless (all rows land on the refresh interval). Always pass both. Also note the *resolved*
preset comes from `%APPDATA%\tileworld\graphics.json`, not from a default — on an Ultra config the
main pass runs at `render_scale 2.0` (3840×2160), a 4× fragment load, which makes such a run a
*sensitive* test for per-fragment costs and a *misleading* one for anything else.

**Read the GPU pass timers, not frame time, for small deltas.** On a discrete GPU this scene is
CPU-bound (GPU Σ ≈3.6 ms inside a ≈10.7 ms frame), and frame-time sd is ±0.7-0.9 ms — so frame ms
cannot resolve anything under ~1 ms, while the pass timers are stable to ±0.01 ms. A useful
self-check that the pinned view really is identical across runs: `main_opaque_pass_3d` should match
to ~0.001 ms between two runs that don't touch opaque geometry.

### 2026-07-27 — visual-fidelity pass (DoF `NEAR` fix, terrain LOD dither, haze, leaf transmission)

`RTX 5060 Ti` (DiscreteGpu, Vulkan 610.74), Ultra + `render_scale 2.0`, pinned
`FOREST_CAM="0,30,60,0,0,-30"` + `FOREST_FREEROAM=1`, `FOREST_PERFTEST=60 FOREST_NOVSYNC=1`,
8 steady samples after discarding 20 s of warmup. **Not** representative of `igpu-strong-cpu`.

| run | frame ms mean [min-max] sd | GPU Σ | `main_opaque_pass_3d` | `bin_unpacking` |
|---|---|---|---|---|
| all changes in | 10.72 [9.6-11.8] sd 0.72 | 3.630 | 2.400 | 0.345 |
| `FOREST_LEAFTRANS=0` | 10.68 [9.4-11.8] sd 0.71 | 3.504 | 2.321 | 0.305 |
| `FOREST_NOBLUR=1` | 10.50 [9.5-12.3] sd 0.87 | 3.607 | 2.401 | 0.325 |

- **Tree translucency material costs a real but small +0.126 ms GPU (+3.5%)** — `main_opaque_pass_3d`
  +0.079 ms (+3.3%, the per-fragment `DIFFUSE_TRANSMISSION` branch) and `bin_unpacking` +0.040 ms
  (+11.6%, the extra material bin). Ranges do not overlap, so this is signal, not noise. Invisible
  in frame time *here* only because the GPU isn't the bottleneck — on the fragment-bound iGPU row
  it lands directly on the frame, and it scales with tree pixel coverage (this pose is moderate,
  not worst-case).
- **DoF at the reduced radius: not measurable** (GPU Σ −0.022 ms, ranges touch). Caveat on what that
  isolates: `FOREST_NOBLUR` only zeroes `max_radius`, so the pass still dispatches and still does a
  full-res read/write — it measures the radius-dependent *sampling* cost, not the pass's existence.
  The custom post chain (dof/outline/godrays/atmospherics) is not individually instrumented.
- **No leaks** over 60 s in all three runs: entities 11872, meshes 4248, materials 235, images 143,
  font atlas 14k/14p — all flat. (`rss=4MB` is a mis-scaled diagnostic, not a real reading.)

## Terrain far-LOD (July 2026, with the MAP_SCALE 2.2 → 2.6 bump)

The terrain sheets were the last full-res-everywhere geometry: chunked (48-tile blocks) and
frustum-culled, but every on-screen chunk drew 1 quad/tile + terrace walls + marching-squares
river banks regardless of distance. `worldmap::build_terrain_chunk_coarse` now builds a second,
stride-4 **coarse drape** per chunk (~1/16th the vertices; no walls, no river cuts — the channel
just dips under the always-drawn water plane; short perimeter skirts hide LOD seams). The two
meshes swap via `VisibilityRange` at **150–176u** (camera→chunk-AABB, `use_aabb`; `worldmap.rs`
`TERRAIN_LOD`/`TERRAIN_LOD_BAND` — this doc said 110–136 until 2026-07-27, from before the radius
was pushed out for MAP_SCALE 2.6) with a dithered crossfade — unlike the scatter culls this band is
deliberately NON-abrupt: terrain is huge and a hard swap pops its silhouette; only the ring of
chunks currently inside the band pays the per-fragment discard. The coarse mesh is
`NotShadowCaster` (cascades end ~150 anyway).

**The crossfade was not actually wired until 2026-07-27.** `terrain.wgsl` overrides
`fragment_shader()` and so replaces `bevy_pbr::pbr.wgsl` wholesale, but it never called
`visibility_range_dither` — while the depth prepass, which is *not* overridden, fell through to
`pbr_prepass.wgsl`, which does dither. So in the 150–176u ring both LODs drew fully in the colour
pass against a prepass depth that was a 4×4 checkerboard of the two, and `depth_compare:
GreaterEqual` resolved to a per-pixel nearest-of-both — an interpenetrating union in which the
coarse drape's straight ramp over a mesa tier punched through the full-res cliff walls ("terrain
showing through the mountains"). Any future terrain fragment shader MUST keep that dither call
first, guarded by `#ifdef VISIBILITY_RANGE_DITHER`.

This is what pays for MAP_SCALE 2.6 (tiles ∝ scale², ~1.4× vs 2.2): beyond ~136u only ~1/16-density
terrain draws, so the full-res vertex load now tracks the LOD radius, not the island size.
`FOREST_NOCULL=1` disables the LOD together with the scatter culls (same A/B protocol above; the
whole-island map-shot recipe already sets it).

## GPU pass breakdown (reference, from baseline F2)

Captured on `igpu-strong-cpu`, boot-day, to know what each pass costs and what to target.

**Low (24 FPS, Σ listed 29.4ms):**

| Pass | ms |
|---|---|
| main_opaque_pass_3d | 15.69 |
| atmosphere_luts | 2.51 |
| lightprobe_irradiance_map | 1.89 |
| render_sky | 1.83 |
| ui | 1.70 |
| smaa | 1.31 |
| tonemapping | 1.22 |
| shadow cascades (×2) | ~1.5 |
| lightprobe_radiance_map | 0.43 |
| upscaling | 0.39 |

**Ultra (14 FPS, Σ listed 47.4ms):**

| Pass | ms |
|---|---|
| ssao | 14.86 |
| main_opaque_pass_3d | 11.05 |
| early prepass | 3.46 |
| shadow cascades (×4) | ~8 |
| atmosphere_luts | 1.36 |
| render_sky | 1.27 |
| smaa | 1.23 |
| bloom | 1.09 |
| lightprobe_irradiance_map | 1.06 |
| volumetric_lighting | 0.72 |

Note: on Low the frame (41ms) exceeds the summed GPU passes (29ms) by ~12ms — unexplained gap
(present/vsync, or the iGPU's shared memory bandwidth making the listed passes undercount real
GPU time). Worth investigating but not the primary lever.
