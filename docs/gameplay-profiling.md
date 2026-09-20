# Recording real gameplay

From the repository folder, run:

```sh
python3 tools/profile_gameplay.py
```

This builds and launches an ordinary game session with recording enabled. Play normally: move
through the world, turn the camera, fight, and visit places that feel slow. Five to ten minutes
is a useful first recording. **F8** marks a slow moment (on a Mac you may need **Fn+F8**).
Close the game normally when finished. The recording stops automatically after 30 minutes,
without closing the game. Use `--seconds 600` for a ten-minute recording.

Each launch creates a new folder under `profiles/`. The report refreshes every 30 seconds,
including while the game is still running. Recordings are excluded from Git. No data is uploaded.

For native CPU stack samples on macOS:

```sh
python3 tools/profile_gameplay.py --stacks --seconds 600
```

The launcher samples the game process for five seconds after at least ten seconds of uninterrupted
active gameplay, then at most once every two minutes. These samples can identify engine/driver
CPU work outside the selected gameplay timers. They add overhead; the report labels/excludes
sampling intervals using approximate process/capture clock alignment. They are **not GPU samples**.

To analyze or refresh an existing recording:

```sh
python3 tools/analyze_gameplay.py profiles/SESSION/gameplay.jsonl --output profiles/SESSION/report.md --json profiles/SESSION/summary.json
```

## What is recorded

- Every frame's First-to-next-First wall interval, paired with that frame's gameplay state.
- First-to-Last main-app wall time and the recorder's own collection time.
- App state, focus, loading status, modal screen, siege phase, hero location.
- Every second: graphics settings, actual window resolution, actor/entity/asset counts.
- Inclusive wall time and call counts for A*, animal/ork AI, worker/guard/siege logic,
  quadruped/biped animation, and tree sway.
- Fresh render diagnostics supplied by Bevy, with CPU and GPU paths distinguished and sample age recorded.
- Process CPU usage and resident memory; GPU device/features, source revision and local patch.
- Manual markers, dropped records, capture termination status, and game warnings/errors.

`gameplay.jsonl` is written through a bounded background queue and flushed every second. If the
writer cannot keep up, records are dropped and counted instead of blocking gameplay. The capture
is capped at 128 MiB and two hours even if a longer duration is requested directly. A crash can
lose the final unflushed second or leave a partial line; the analyzer accepts the valid prefix and
marks an incomplete capture. The final frame without a following frame boundary is omitted.

## How to use the results

The default report includes only focused, loaded, active gameplay and excludes five seconds of
warmup after loading, pauses, or focus changes. Startup, menus, pause and background frames remain
in the raw recording. This avoids reporting a quiet menu or background throttling as play speed.

Start with frame p95/p99 and the worst-frame timeline, then examine the CPU system ranking and
render-pass data near those times. Compare actor/asset counts, location, phase and graphics
settings. Change one suspected bottleneck, repeat a similar route/fight with the same settings,
and compare distributions. Averages alone can hide severe stutters.

These timers are not complete CPU attribution: function bodies exclude deferred command work,
run conditions, asynchronous jobs and most engine systems. Timings are inclusive wall time;
callers can contain A* and worker threads can overlap. **Do not sum them into a CPU frame total.**
Main-app time excludes the render sub-app/presentation and is not a direct measure of GPU time.
Render diagnostic samples arrive asynchronously, may overlap, and are not matched to an exact
frame. Missing GPU measurements mean unavailable, not zero. Native stacks help find uninstrumented
CPU work; graphics-setting A/B tests help investigate GPU limits when hardware timings are absent.

The existing full-engine trace build remains available with `cargo run --features profiling` and
`tools/trace_summary.py`, but recompiles Bevy and produces much larger traces. The normal gameplay
recorder needs no extra Rust dependency or Bevy feature.

For custom launch integration, set `FOREST_PROFILE` to a new recording directory and optionally
`FOREST_PROFILE_SECONDS`. Reusing an existing `gameplay.jsonl` is refused instead of overwriting it.
The recorder is otherwise disabled; scoped instrumentation performs only an enabled-flag check.
