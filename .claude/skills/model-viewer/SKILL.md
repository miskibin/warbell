---
name: model-viewer
description: Inspect a SINGLE character/prop model in isolation (no game world) via the in-binary FOREST_VIEW standalone viewer — render it large, unoccluded, on a clean 3-point-lit stage, and screenshot/turntable it headlessly. Use when iterating on or visually verifying one model (the hero knight, an ork, a prop, etc.), reworking a mesh, or when the user asks to "see / inspect / debug just the model". Much faster than a full-game capture (~25s vs ~6min — it skips world generation).
---

# Single-model viewer (FOREST_VIEW)

Inspecting a model inside the full game is slow and awkward: the world build takes minutes, the
character is small and half-hidden behind props/HUD, and you have to guess `FOREST_CAM` to frame
it. The **viewer mode** renders ONE model in a minimal app — no world, no gameplay, no HUD — large
and centred on a neutral, evenly-lit stage. It reuses the real in-crate mesh builders + the shared
`CreatureMaterial` + the same capture harness, so what you see matches the game's shading.

Lives in `src/viewer.rs`; `main()` branches into it when `FOREST_VIEW` is set (see `src/main.rs`).

## Headless container setup (cloud / no display)

Same as the `visual-debug-cloud` skill — do this once per fresh container:

```bash
sudo apt-get update && sudo apt-get install -y \
  libwayland-dev libxkbcommon-dev libudev-dev libasound2-dev \
  xvfb mesa-vulkan-drivers libvulkan1 vulkan-tools
cargo build            # first build is slow (compiles bevy); rebuilds are fast
```

## Render a model

`FOREST_VIEW=<model>` picks the model; combine with the normal `FOREST_SHOT` / `FOREST_CLIP`
capture vars. Always set `BEVY_ASSET_ROOT` to the repo root so the shaders load.

```bash
export BEVY_ASSET_ROOT="$PWD"
# Single still (1920×1080 PNG):
FOREST_VIEW=hero FOREST_SHOT=/tmp/hero.png \
  xvfb-run -a -s "-screen 0 1280x720x24" ./target/debug/tileworld_bevy_forest
```

~25s, because there is no world generation. Then `Read` the PNG.

### Models

- `FOREST_VIEW=hero` (default / any unknown value) — the player knight in **rest pose**.
  `FOREST_EQUIP="weapon_id,armor_id"` swaps gear (e.g. `FOREST_EQUIP="sword_gold,gold_armor"`).
- New models slot into the `match` in `spawn_model()` in `src/viewer.rs` — add an arm like
  `"ork" => { … build + spawn … }` calling that model's builder against the creature material.

### Framing

- Default camera frames a ~1.8u-tall model standing at the origin (chest-height look-at).
- Override with `FOREST_CAM="ex,ey,ez,tx,ty,tz"` (eye + look-at, same format as the game).
  A front-left eye (negative X) shows the shield side; the sword is on +X.

### Turntable / motion clip

The reused `CapturePlugin` honours `FOREST_CLIP` + `FOREST_CLIP_ORBIT`, which orbits the camera
around a point — i.e. a turntable:

```bash
mkdir -p /tmp/turn
FOREST_VIEW=hero FOREST_CLIP=/tmp/turn FOREST_CLIP_FRAMES=120 FOREST_CLIP_FPS=30 \
  FOREST_CLIP_ORBIT="0,0.9,0,3.0,1.1,90" \
  xvfb-run -a -s "-screen 0 1280x720x24" ./target/debug/tileworld_bevy_forest
# orbit = "cx,cy,cz,radius,height,deg_per_sec"; 90°/s × 4s = full 360°.
# Read a few frames, or stitch: ffmpeg -framerate 30 -i /tmp/turn/frame_%05d.png -pix_fmt yuv420p turn.mp4
```

## Notes / limits

- The viewer shows the model's **rest/spawn pose** (no animator runs here). To inspect the hero's
  *animations*, use the in-game `FOREST_ANIMTEST=walk|block` staging hook with the normal
  `FOREST_SHOT`/`FOREST_CLIP` (that path runs the real animator).
- Lighting here is a neutral 3-point rig, NOT the game's atmosphere/IBL — geometry, proportions
  and per-surface texture (the `surf` codes) read true, but final in-game tone differs. For a
  lighting/atmosphere check, capture in the real game (`visual-debug-cloud`).
- The viewer is a debug tool: keep model-building logic in the shared builders so both the game and
  the viewer render the exact same mesh.
```
