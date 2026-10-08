# Blender trees: feasibility protocol, 2026-10-08

Baseline: latest fetched origin/main `90f8d1b1b4b780b481046e76a82c3d931b64d60b`.
Machine: Intel i5-12400F, RTX 5060 Ti 16 GiB, driver 616.64, Windows.
Build: release, locked dependencies. Preserve a pristine baseline executable.

Scope: Blender-authored summer oak, birch and pine geometry/textures, opt-in.
Preserve placement, root entities, collision and gameplay. Preserve native nonforest
biome vegetation and dead/stump trees. This is a first art replacement experiment,
not a whole-game realism conversion or a claim about integrated GPUs.

Measurement: fixed camera, 1920x1080, explicit preset, no VSync, separate temporary
APPDATA per run. Existing main harness excludes 15 seconds after WorldReady before
collecting individual frame mean/p50/p95/p99. Run GPU workloads sequentially. Do
not count screenshot encoding or Blender rendering as game performance.

First collect the unmodified-main behavioral baseline with its natural day/night
cycle. For the matched comparison, run the SAME experimental executable with the
tree switch off/on and `FOREST_PERF_SKY=0.28` (day) or `0.75` (siege). This new hook
pins lighting without setting `SkyClock.paused`, which would also stop the siege
clock. Do not directly attribute differences between natural-sky and pinned-sky
runs to the trees. Captures use the existing screenshot-only sky freeze.

Measurement correction before final A/B: Bevy 0.19.1 `WinitSettings::game()` uses a
60 Hz `reactive_low_power` timer for unfocused windows, independently of VSync.
The main harness sets `window.focused=false`. Initial `main-natural-*` and `ab-*`
records therefore measure a capped background window and are NOT an uncapped FPS
baseline. `perftest.rs` now selects `WinitSettings::continuous()` only when both
PERFTEST and NOVSYNC are requested. Final decision uses only `uncapped-*` records,
from one binary with original-tree and Blender-tree modes. Normal gameplay is unchanged.
RTS is excluded per the user's clarification.

Scenarios and settings are in `tools/tree_study.py`. Validate camera framing before
freezing measured runs. Compare close forest, wider forest and 96-enemy siege, High;
repeat alternating baseline/candidate runs three times where stable. One Low close
forest pair provides a secondary quality check. Capture matching before/after shots
separately. Record binary SHA-256, environment, exit code and errors per run.

Decision gate fixed before measurements: candidate is promising only with clear
visual improvement at gameplay scale, no asset/shader errors, no gameplay changes,
and no more than 10% regression in median-of-run mean/p95 frame time or GPU pass sum
in representative forest cases. Report noise/ranges and p99 separately; inconclusive
near-threshold cases need more data. A failed gate keeps the option experimental.
The gate is an engineering screen; final aesthetic acceptance belongs to the user.

Risks to quantify: masked foliage overdraw and shadows, CPU entity/extraction cost,
loading time, asset size and memory. GPU pass sum is diagnostic, not an exclusive
GPU frame duration (nested/concurrent passes may overlap). Process memory in the
existing harness is diagnostic only. No extrapolation from this GPU to laptops.

## Reproduce on Windows

Use Python 3.10+ and Rust with the locked dependencies. From the repository root:

```powershell
cargo build --release --locked -j 4 --target-dir target
New-Item -ItemType Directory -Force target/tree-study/bin
Copy-Item target/release/tileworld_bevy_forest.exe target/tree-study/bin/trees.exe
python tools/tree_study_suite.py
```

The suite runs 20 timed sessions sequentially (three A/B pairs per High scene and
one Low pair), then six screenshot sessions. Leave the machine free of other heavy
work. Completed results are reused only if the binary, runtime assets and requested
mode match. Archive an earlier result directory explicitly before replacing it;
the suite refuses to silently overwrite mismatched or incomplete runs.

`python tools/tree_study_summary.py` refreshes `target/tree-study/summary.json`.
`tools/tree_study_report.py` is the dated report builder for this recorded study;
it also expects `uncapped-build.log` and `core-tests.log` in that directory. For a
new study, choose a new report destination and record fresh hardware/build details.
The report's GPU values are the mean of post-warmup five-second diagnostic samples
within each run, followed by the median across runs. Frame percentiles use individual
post-warmup frames, not those five-second samples.

No runs were discarded for being slow. The initial close-forest frame times varied
substantially while GPU times stayed stable; retain their full range and do not
attribute the median FPS difference entirely to the tree replacement. The stable
GPU regression already decides the predeclared performance gate, so extra trials
were not added to search for a passing result.
