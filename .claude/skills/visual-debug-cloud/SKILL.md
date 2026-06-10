---
name: visual-debug-cloud
description: Render and inspect game screenshots headlessly in a cloud container (no GPU, no display). Use when verifying any visual change — models, lighting, UI, biomes — from Claude Code on the web, or when the user asks to "see" the game from a remote session. Covers container setup (apt packages, xvfb, Mesa software Vulkan), the FOREST_SHOT capture workflow, staging env vars, and framing tips.
---

# Visual debugging in the cloud (headless screenshots)

The Bevy window cannot be captured externally, and a fresh cloud container has **no GPU and
no display server**. This skill renders real in-game screenshots anyway: Xvfb provides a
virtual X11 display, Mesa's **llvmpipe/lavapipe** provides a software Vulkan adapter, and the
game's built-in `FOREST_SHOT` harness (`src/capture.rs`) renders ~90 frames (so lighting/IBL
settle), saves a PNG, and exits.

## 1. One-time container setup (re-run every fresh container — they're ephemeral)

```bash
sudo apt-get update && sudo apt-get install -y \
  libwayland-dev libxkbcommon-dev libudev-dev libasound2-dev \
  xvfb mesa-vulkan-drivers libgl1-mesa-dri libegl1 libxkbcommon-x11-0
```

- Line 1 is Bevy's standard Linux build deps (already documented in CLAUDE.md — without them
  the *build* fails in a `wayland-client not found` build script).
- Line 2 is the headless-render stack: `xvfb` (virtual X display), `mesa-vulkan-drivers`
  (lavapipe — the CPU Vulkan driver wgpu picks up), `libgl1-mesa-dri`/`libegl1` (GL fallback),
  and **`libxkbcommon-x11-0`** — without this exact package winit panics at startup under
  X11 (`Failed loading libxkbcommon-x11.so.0`).

Then build once (first build recompiles Bevy at opt-level 3, ~3 min; later builds are fast):

```bash
cargo build
```

## 2. Taking a shot

```bash
FOREST_SHOT=/tmp/shot.png FOREST_TIME=0.25 \
  timeout 570 xvfb-run -a -s "-screen 0 1280x720x24" \
  ./target/debug/tileworld_bevy_forest >/dev/null 2>&1
```

Then **view the PNG with the Read tool** to actually inspect it.

Notes:
- Expect **~3–5 minutes per shot** on llvmpipe (the `software rendering ... very slow`
  warning and the X11 `XSETTINGS` warning are normal). Wrap in `timeout 570` so a hang
  can't eat the Bash tool's 10-minute ceiling.
- Run shots **sequentially**, not in parallel — llvmpipe saturates all cores.
- Run from the repo root so `assets/` resolves (or set `BEVY_ASSET_ROOT`).
- The HUD renders over the scene (the harness boots straight into Playing); ignore it.
- Sim time during a capture is short (frame `dt` is clamped), but wandering NPCs still
  drift a little — frame staged shots with some margin.

## 3. Staging the scene

All `FOREST_*` env hooks from CLAUDE.md work here; the load-bearing ones for visual checks:

| Var | Use |
|---|---|
| `FOREST_CAM="x,y,z,tx,ty,tz"` | camera at xyz looking at txtytz — THE framing tool |
| `FOREST_TIME=0.25` | fixed time-of-day (0.25 ≈ midday; without it a slow render drifts the sun) |
| `FOREST_HERO="x,z"` | drop the hero at a world XZ |
| `FOREST_EQUIP="axe,leather_armor"` | equip item ids so the hero shows that weapon/armor |
| `FOREST_ORKLINE="x,z"` | park one ork of each variant in an idle line (model close-ups) |
| `FOREST_WAVE=1` / `FOREST_DEFEND=1` | stage a night siege / arm defenses |
| `FOREST_BIOME` / `FOREST_PANEL` / `FOREST_MENU` | biome view / UI panels / start screen |

Useful anchors for framing (castle at world origin, 1 tile = 1 unit):
- Hero default spawn: just outside the **north gate** at ~`(0, -15)`, facing the castle
  (+Z). A good close-up: `FOREST_CAM="0.55,0.8,-13.85,0,0.45,-15"`. Mirror x (or use
  `-0.55`) to see the shield side.
- Walls run at `x=±17`, `z=±12` with gates at the axis midpoints (`castle::gate_centers`).
- Hero is small (~0.9u tall after `HERO_SCALE`); a camera ~1.5–2u away at y≈0.8–1.0
  looking at y≈0.45 fills the frame. Orks are ~1.3–1.6u; back off to 4–9u for a group.

## 4. Verification workflow for model/visual changes

1. `cargo check` + `cargo test -p tileworld_core` first — geometry mistakes (e.g. calling
   `compute_flat_normals` on an indexed mesh) panic at *runtime*, so a shot is also the
   crash test.
2. Take one **default-camera sanity shot** (no `FOREST_CAM`) to confirm the app boots and
   renders end-to-end.
3. Then **staged close-ups** of the thing you changed. Check: feet on the ground (base at
   y=0), no floating/orphaned parts, no obvious z-fighting (coplanar faces), colours read
   correctly, and silhouettes still match at gameplay camera distance.
4. Shots cost minutes — batch your look: one front-quarter close-up plus one pulled-back
   shot usually answers everything.
