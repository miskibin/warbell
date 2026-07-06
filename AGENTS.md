# AGENTS.md

`CLAUDE.md` is the primary, detailed guide for this repo (architecture, conventions, the
`FOREST_*` capture harness, git rules). Read it first. The `.claude/skills/` directory holds
task-specific SOPs (`visual-debug-cloud`, `model-viewer`, `trailer-maker`, `studio-port`,
`release`). This file only adds the durable, non-obvious notes for running in a Cursor Cloud VM.

## Cursor Cloud specific instructions

**What this is:** "Warbell" — a single Bevy 0.19 / Rust game crate (`tileworld_bevy_forest`, root
`src/`) plus a pure-logic workspace member `crates/core` (`tileworld_core`). One product, no
backend/services.

**Standard commands** (see `README.md` / `CLAUDE.md`):
- `cargo build` — build the game (first build recompiles Bevy at `opt-level=3`, ~10 min; later
  `src/` rebuilds are fast).
- `cargo test -p tileworld_core` — the parity unit tests (~306 + integration tests). This is the
  test suite; `src/` has no tests.
- `cargo check` — the lint/type-check (no clippy/rustfmt gate in CI; CI only builds on tag).

**Toolchain gotcha:** the crate is `edition = "2024"`, which needs Rust ≥ 1.85. The base VM's
`rustup` default was an older pin (1.83), which fails with `feature 'edition2024' is required`.
The environment is set to `rustup default stable` (currently 1.96); the update script re-asserts
this each session.

**Running / visual testing is headless (no GPU, no display).** The Bevy window can't be captured
externally, so use the built-in render-and-exit harness, NOT a normal `cargo run`:
- Required system libs (Bevy build deps + software-render stack) are baked into the VM snapshot:
  `libwayland-dev libxkbcommon-dev libudev-dev libasound2-dev xvfb mesa-vulkan-drivers
  libgl1-mesa-dri libegl1 libxkbcommon-x11-0`. If a fresh image ever lacks them, the build fails
  with `wayland-client not found` / winit panics with `libxkbcommon-x11.so.0`.
- Take a shot through Xvfb + lavapipe (software Vulkan):
  ```bash
  BEVY_ASSET_ROOT=$PWD FOREST_SHOT=/tmp/shot.png FOREST_TIME=0.28 \
    timeout 570 xvfb-run -a -s "-screen 0 1280x720x24" \
    ./target/debug/tileworld_bevy_forest
  ```
- **Always set `BEVY_ASSET_ROOT=$PWD`** or custom-material meshes (ground + hero) silently fail to
  render. Verify a clean run: `grep -c "Path not found" <log>` must be `0`, and confirm the
  `Screenshot saved` log line before trusting the PNG. Each shot takes ~1 min on llvmpipe; run
  shots sequentially. The `software rendering ... very slow` warning is expected.
- Stage scenes with the `FOREST_*` env vars (full table in `CLAUDE.md`), e.g.
  `FOREST_WAVE=1 FOREST_DEFEND=1 FOREST_NIGHT=1` stages a fortified night siege,
  `FOREST_CAM="x,y,z,tx,ty,tz"` frames it. See the `visual-debug-cloud` skill for the full
  workflow and `FOREST_CLIP` for stitched video.

**Marketing site (`site/`)** is plain static HTML/CSS/JS with no build step — preview it with
`python3 -m http.server 8099` run from `site/`, then open `http://localhost:8099/`. Its images
in `site/screenshots/*.png` are real in-engine captures from the `FOREST_SHOT` harness; use
`FOREST_QUALITY=ultra` for the showcase look and `FOREST_NOHUD=1` for clean, HUD-free marketing
frames. Reliable framings: `FOREST_TPS=1 FOREST_HERO="x,z"` for character-in-world / biome shots,
`FOREST_CAM` for deliberate overviews. Note `FOREST_PANEL=<panel>` also opens the egui F1 debug
window (shared env check), so a UI-panel shot needs cropping. The VM has **no ImageMagick or
Pillow** — crop/resize captures with `ffmpeg -vf "crop=w:h:x:y"` instead.
