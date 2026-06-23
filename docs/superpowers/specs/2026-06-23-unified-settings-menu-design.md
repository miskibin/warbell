# Unified Settings Menu — design

**Date:** 2026-06-23
**Status:** approved, implementing
**Supersedes:** the standalone graphics Settings page (`ui/graphics_menu.rs`, commit `ed3471f`).

## Goal

Replace the standalone, centered-card graphics page with one **full-screen, top-tabbed** settings
menu (CS2-style), reached from a single **Settings** button in both the start screen and the pause
menu. Fold in three bugs the standalone page shipped with: the broken close `✕`, the render-scale
slider not applying, and option rows overflowing the card.

## Layout (user picked: top tabs)

Full-screen scrim + edge-to-edge card. Top to bottom:

- **Header row** — `SETTINGS` title + a working `✕` close button.
- **Tab bar** — `Graphics · Display · Audio · Controls` (segmented, gold-active).
- **Content pane** — only the active tab's rows are spawned (this is what kills the overflow: no
  single row list is ever wider/taller than the screen). Switching tabs despawns + respawns the pane.

## Tabs & contents

- **Graphics** — Preset (Low/High/Ultra/Custom) · Render scale (slider) · Shadows · Anti-aliasing ·
  Ambient occlusion · Terrain detail (segmented) · Bloom · Depth of field · Outline · God rays
  (checkboxes). All bind to the existing `GraphicsSettings`.
- **Display** — Window mode (Windowed/Fullscreen) · Resolution (Native + presets) · VSync. Bind to
  the existing `WindowSettings`. (Split out of Graphics so window/monitor ≠ visual fidelity.)
- **Audio** — Master · Music · SFX volume sliders (0–100%, default 100%) + Mute. NEW: see Audio
  wiring below.
- **Controls** — Camera (Third/First person, reuses `FirstPerson`) + a **read-only** keybind
  reference list. No live rebinding (explicitly out of scope).

## Data / resources (all persisted to the settings config, today `graphics.json`)

- `GraphicsSettings` (exists) — render pipeline.
- `WindowSettings` (exists) — mode/resolution/vsync.
- `AudioSettings` (exists; currently `muted`/`unfocused`) — **extend** with `master: f32`,
  `music: f32`, `sfx: f32`, all `0.0..=1.0`, default `1.0`.
- `FirstPerson` (exists) — camera mode.

### Audio wiring

`AudioConfig` already holds the *tuned* mix (`sfx_vol 0.6`, `music_vol 0.22`, `voice_vol`,
`narration_vol`, `ambience_vol`, …). The new sliders are **user multipliers**, NOT direct edits of
those tuned values: effective gain = `tuned × user_channel × master`. So 100% on every slider == the
authored balance, and the mix stays intact. The sfx/music systems already read `AudioConfig.*_vol`;
they additionally multiply by `AudioSettings.{master, sfx|music|voice}`. Mute = the existing
`muted` flag (forces 0). Persist `master/music/sfx`; reset on New Game is N/A (global pref, not
run-state).

## The three bug fixes

1. **`✕` glyph** — the Inter/bold font lacks U+2715, so it renders as tofu. Draw the X from two
   thin rotated bars (or use the Twemoji `✕` atlas entry) in `ui/widgets.rs::close_button` (or a
   local close button), so every panel using it gets a real ✕.
2. **Render scale not applying** — the slider emits its value on `DragEnd` (`is_final = true`), but
   it isn't committing/applying. Debug the commit path (`on_slider_change` is_final → write
   `GraphicsSettings.render_scale` → `apply_render_scale` inserts `MainPassResolutionOverride`). The
   override mechanism itself is verified working (`FOREST_RENDERSCALE`). **Acceptance: set render
   scale to 50%, capture, confirm the 3D is visibly half-res while the UI stays sharp.**
3. **Overflow** — resolved structurally by full-screen + per-tab panes.

## Menu wiring / cleanup

- `game_state::spawn_pause_screen` — **remove** the four scattered toggle buttons (View / Audio /
  Graphics / Fullscreen) and `pause_settings_sync`; add one **SETTINGS** button that sets the menu's
  open flag. Keep Resume / Save / Load / Restart / Main Menu.
- Start screen — the existing **Settings** button opens the same menu (already wired).
- `Esc` / `✕` closes and persists (already wired).
- The F10 quick-cycle + M mute keys stay as shortcuts.

## Out of scope (YAGNI)

Live key rebinding; restyling the pause/start *screens* themselves (they keep their look — only
their settings toggles move into the menu); mouse-sensitivity (no sensitivity system exists).

## Acceptance

- Menu opens full-screen from start + pause; tabs switch; no overflow.
- `✕` renders as a real cross and closes.
- Render scale visibly applies at 50% (captured).
- Audio sliders change loudness live; 100% == prior balance; settings persist across relaunch.
- Pause menu no longer shows the four scattered toggles.
- `cargo check` clean; existing graphics presets + the Low→High crash fix unaffected.
