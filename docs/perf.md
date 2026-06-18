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
| | | | | | | | _(after #1 VisibilityRange culling — re-measure boot-day Low/Ultra here)_ |
| | | | | | | | _(after #2 atmosphere/IBL throttle)_ |
| | | | | | | | _(after #3 High shadow cascades 4→3)_ |
| | | | | | | | _(after #5 CPU spatial grid / anim gating — measure `siege`)_ |

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
