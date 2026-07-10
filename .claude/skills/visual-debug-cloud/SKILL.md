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
BEVY_ASSET_ROOT=$PWD \
  FOREST_SHOT=/tmp/shot.png FOREST_TIME=0.25 \
  timeout 570 xvfb-run -a -s "-screen 0 1280x720x24" \
  ./target/debug/tileworld_bevy_forest >/dev/null 2>&1
```

Then **view the PNG with the Read tool** to actually inspect it.

Notes:
- **ALWAYS set `BEVY_ASSET_ROOT=$PWD` (repo root).** Running the binary directly,
  Bevy resolves its asset root from the *executable's* dir (`target/debug/assets/`), NOT the
  CWD — so `assets/` 404s **even when you `cd` to the repo root**. When it 404s, the
  disk-loaded WGSL shaders fail silently: meshes using custom materials (the **ground**
  shader and every creature/**hero** mesh, which use `creature.wgsl`) **don't render at all**,
  while props on the built-in `StandardMaterial` still show. Symptom: flat untextured ground +
  an invisible hero, with `Path not found: .../target/debug/assets/...` errors in the log. This
  is the #1 way to waste a 5-min shot. Verify a run is clean with
  `grep -c "Path not found" log` → must be `0`.
- Expect **~3–5 minutes per shot** on llvmpipe (the `software rendering ... very slow`
  warning and the X11 `XSETTINGS` warning are normal). Wrap in `timeout 570` so a hang
  can't eat the Bash tool's 10-minute ceiling.
- **FOREST_CLIP: set `FOREST_CLIP_WARMUP=70`+ — the default 30 records the boot LOADING
  VEIL.** The veil (a full-screen dark backdrop, `loading.rs`) lifts ~1.4s of *sim* time
  after boot, so with the default 30-frame warmup the first ~50 saved frames are dimmed
  grey / pure black *behind an intact HUD*, in every scene. This is spectacularly
  misleading when you're debugging a visual: it reads as "some mesh/overlay blacks out my
  frames at t≈1.1–1.4s" and it is invariant to any code you change (an entire debugging
  session was once lost to chasing it as a viewmodel bug). It is NOT a game bug — real
  players never see it; the SHOT harness warms ≥240 frames precisely to skip it.
- Run shots **sequentially**, not in parallel — llvmpipe saturates all cores.
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
- Hero default spawn: `player::spawn_point()` = **`(-26, 18)`**, facing the castle at the
  origin (bearing ≈ `(0.82, 0, -0.57)` — mostly +X, a bit −Z). `FOREST_HERO` moves the hero
  but KEEPS this boot bearing, so anything staged "in front" (an orkline for an FP shot)
  must sit along `hero + d·(0.82, -0.57)` — NOT simply at `z+d`. (An earlier version of this
  note claimed spawn ≈ `(0,-15)`; that's stale — staging placed off it lands out of frame.)
- Walls run at `x=±17`, `z=±12` with gates at the axis midpoints (`castle::gate_centers`).
- Hero is small (~0.9u tall after `HERO_SCALE`); a camera ~1.5–2u away at y≈0.8–1.0
  looking at y≈0.45 fills the frame. Orks are ~1.3–1.6u; back off to 4–9u for a group.

### Whole-island MAP shot — the working recipe (top-down, clouds/haze/cull OFF)

A clean top-down map of the *entire* island DOES work — but only if you kill the three things
that wash a naive god-cam out (haze, clouds, distance-cull) and shoot **near-vertical**, not
oblique. The oblique god-cam fails because its line of sight runs off to the infinite hazed
horizon; a near-straight-down cam sees only ground a few hundred units away, so turning fog off
actually clears it. Proven recipe (this is the "zajebisty" island map):

```bash
BEVY_ASSET_ROOT=$PWD \
  FOREST_SHOT=/tmp/island.png \
  FOREST_CAM="0,300,60,0,0,28" \   # near-vertical over map centre; small z-offset avoids the straight-down gimbal
  FOREST_NOCLOUDS=1 \              # MANDATORY: the y42-80 cloud layer otherwise blankets a top-down
  FOREST_NOCULL=1 \               # MANDATORY: else distant trees(>~90u)/props(>75u)/cover(>55u) are culled → bare map
  FOREST_NOHUD=1 \
  FOREST_QUALITY=low \            # no bloom/DoF/godrays → flat, crisp, un-blown map
  FOREST_FOG="4000,9000" \        # fog effectively off; only works BECAUSE the view is near-vertical
  FOREST_TIME=0.28 \
  timeout 570 xvfb-run -a -s "-screen 0 1600x1600x24" \
  ./target/debug/tileworld_bevy_forest >/tmp/island.log 2>&1
```

- **Framing knobs**: `height ~300` + look-at `(0,0,28)` frames the whole island incl. the southern
  Blight; drop to `height ~60` over a biome centre (near-vertical, e.g. `FOREST_CAM="-70,64,41,-70,1,37"`)
  for a zoomed **region** map (same clouds/cull/fog flags). Keep the camera↔look-at horizontal
  offset SMALL (steep) — the moment it tilts oblique the far side hazes to grey again.
- **The four flags are the whole trick.** Miss `FOREST_NOCLOUDS` → white cloud-tops in frame;
  miss `FOREST_NOCULL` → only the patch under the look-point has trees/rocks, rest reads bald;
  miss `FOREST_QUALITY=low` → midday bloom blows out snow/sand; miss the fat `FOREST_FOG` → distant
  terrain greys out. All four are capture-only — none changes the game.
- Pale grey blocks at the frame corners over sea = the **distant backdrop isles** (`distant_isles.rs`)
  on the horizon, not terrain — ignore them (or `FOREST_NOISLES=1` to drop them).

### Wide / OBLIQUE overview shots — a high god-cam WASHES OUT; don't fight it

(For a straight-down MAP use the recipe just above. This is about *oblique* high cams.) A static
oblique `FOREST_CAM` raised up to "see the whole island" reliably renders a flat white/pale
frame — three things compound at altitude and none is fixed by your change:
- **Atmospheric haze** over the long view distance fogs distant terrain to the sky colour.
  `FOREST_FOG="clear,full"` (bigger numbers = thinner) only nibbles at it — it does **not**
  clear a whole-island view.
- **The cloud layer** sits ~y90; a camera above it shoots cloud-tops, not ground.
- **Midday bloom** (`FOREST_TIME≈0.25–0.35`) blows out the bright/snow areas to white.

So to actually SEE terrain:
- Frame **low and oblique** — camera height **~15–40u**, close to the subject, looking nearly
  along the ground (the orbit-flyover examples in CLAUDE.md use *height 14* for a reason). A
  near-top-down at y≈25 over a single feature reads crisply.
- To verify a **spread-out** feature (rivers across the map, biome placement), take **several
  low close shots at the known coords** — one per region — rather than one doomed overview.
  Biome/feature world-XZ anchors are in CLAUDE.md.
- If you genuinely need the island-wide framing (promo), use the **orbit clip**
  (`FOREST_CLIP_ORBIT="cx,cy,cz,radius,height,deg"` with a *low* height) and pick a frame —
  a single high static cam won't do it cleanly.
- Always check the run log for `Screenshot saved` before trusting the PNG (cold-pipeline black
  frame / crash → stale file), and re-run once if a frame looks wrong.

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
