# FP Combat View-Model — Design

**Date:** 2026-07-03
**Goal:** Promote first-person from an exploration novelty to a full combat mode, Skyrim/Vermintide
game-feel. Two reported problems: (1) sword/shield move unnaturally (stiff/glued, bad base pose,
attack swing reads wrong), (2) a melee ork closes to ~0.86u from the eye and fills the whole frame.

**Hard constraints (unchanged):**
- Single-camera pipeline — NO second `Camera3d` (see CLAUDE.md; the 2-camera FP view-model fails
  three documented ways). Everything happens on the one camera + the hero's own limb meshes.
- FP camera eye stays rigid (no camera bob) — the existing 75% trauma/FOV-punch damping in FP is a
  deliberate motion-sickness guard and stays.
- No `crates/core` changes. No save-data impact (nothing persistent added).

**Current state (diagnosis, from code):**
- FP view-model = forearms + hands + sword + shield + legs; head/torso/upper-arms hidden via
  `fp_hide` (`player/mod.rs::spawn_hero_meshes`, `camera.rs::fp_body_visibility`).
- FP arm poses are *static* tuck targets slerped over the TPS locomotion pose by `fp_amt`
  (`anim.rs::hero_anim` ~1002–1039). No idle sway, no walk-bob, no turn-lag → "glued" feel.
- Every FP override is gated on `attack.is_none()`: an attack switches the override OFF and plays
  the raw TPS whole-body swing clip, authored for over-the-shoulder framing → the arm crosses the
  whole lens, hard pose discontinuity on start/end.
- Enemy keep-out is mode-agnostic: min center distance = `o.body_r + hero_guard_radius` ≈ 0.86u for
  a grunt (`orks.rs` ~585–637, applied in `movement.rs` ~558–564). FP eye sits at hero center
  + `FP_FWD_OFF 0.05` → scale-1.35 ork mesh at ~0.8u fills the frame.
- No FP-specific FOV; no reticle (decision: keep no reticle — the 60° hit cone forgives).

## 1. Procedural weapon motion (idle + locomotion)

In the FP override block in `anim.rs::hero_anim`, replace the static targets with
*target + procedural offset* before the `fp_amt` slerp:

- **Idle breathing** — slow sine (~0.5 Hz, ±0.015 rad) on both shoulders' pitch.
- **Walk-bob** — driven by the existing `walk_phase` / `moving_amt`: vertical bob at 2× stride
  frequency + small lateral figure-8 at 1×, amplitude × `moving_amt` (and modestly by sprint).
  Applied to the arm joints only — **never the camera**.
- **Turn-sway** — low-pass the frame-to-frame `look_yaw` delta; tilt/lag the weapon+shield up to
  ~0.06 rad opposite the turn, recovering over ~0.2 s. This is the main "not glued" ingredient.
- **Re-tuned base pose** — sword hilt visible bottom-right with the blade angled up-and-inward
  (ready stance), shield bottom-left with its top edge visible. Respect the near-plane (0.04):
  keep mesh surfaces > ~0.08u from the eye.

## 2. FP attack + block poses

Remove the `attack.is_none()` gate; the FP override stays active during attacks and slerps the
sword-arm chain toward FP-authored per-phase targets, reusing the existing `attack_phase` timing
(WIND_END 0.30 / STRIKE_END 0.55, `ATTACK_DURATION` 0.45 s):

- **forward_thrust** — straight jab toward frame center.
- **horizontal_slash** — cut across the lower third of the frame.
- **overhead_chop / heavy_chop** — top-right down to lower-center; heavy = bigger wind-up + pause.
- Torso/hip rotations damped by `fp_amt` during attacks (they're what threw the arm across the
  lens); legs keep locomotion.
- Clamp forward extension so the blade never crosses the near plane.
- **Block:** FP variant of `defend_pose` — shield raised up-center covering ~⅓ of the frame,
  clearly readable as "guarding"; the existing block-priority over the FP rest pose stays.
- Director gestures keep their existing priority over FP overrides.

## 3. Melee readability (FP standoff + FOV)

- **FP standoff:** add `fp.blend × FP_GUARD_EXTRA` (~0.5) to `hero_guard_radius` (`orks.rs`) so a
  grunt holds at ~1.4u instead of 0.86u. Add the *same* term to the `ORK_ATTACK_RANGE` entry check
  (and the siege reuse of it) so orks can still attack from the new distance — otherwise the
  keep-out pushes them permanently out of their own reach and melee dies. Player `ATTACK_RANGE`
  1.8 still reaches; no change. The visual `lunge_clear_of_hero` clamp inherits the bigger guard
  automatically.
- **FOV:** widen the FP base FOV by ~10°, blended by `fp.blend` (applied around the existing
  `base_fov` capture in `camera.rs`), so the same ork subtends less of the frame.
- No reticle (decided against).

## 4. Verification + scope

- Stills: `FOREST_FP=1` + `FOREST_SHOT` (base pose, block pose). Confirm the `Screenshot saved`
  log line; retry once before debugging a bad frame.
- Walk feel: `FOREST_CLIP` + `FOREST_FP=1` + `FOREST_DEMO=explore` (bob/sway in motion).
- Attacks: new `FOREST_SWINGTEST=1` staging hook — loop attacks (cycling clip families) on a timer
  like `FOREST_ROLLTEST`, skipping input gates, so a clip can film every swing in FP.
- Standoff: `FOREST_ORKLINE`/`FOREST_WAVE` + walk into melee in FP; verify the ork holds ~1.4u and
  still lands attacks.
- Files touched: `src/anim.rs`, `src/player/camera.rs`, `src/player/mod.rs` (if pose spawn needs
  it), `src/orks.rs`, `src/siege.rs` (attack-range reuse), `src/combat.rs` (block/attack glue),
  plus the `FOREST_SWINGTEST` hook. No core, no save changes.

## Out of scope

- Dedicated hand-authored FP animation clips (Chivalry-style full-screen arcs).
- Any second camera / RenderLayers view-model.
- Camera bob or added screen-shake in FP.
- Reticle/crosshair.
