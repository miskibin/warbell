# FP Combat View-Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote first-person into a full combat mode: procedural weapon motion (bob/sway/breath), FP-framed attack + block poses, and a melee standoff + FOV widen so an ork no longer fills the frame.

**Architecture:** All view-model work stays inside the existing single-camera pipeline: the FP override block in `src/player/anim.rs::hero_anim` gains procedural offsets and per-phase attack targets (slerped by `fp.blend` over the TPS pose, exactly like the current static tucks). The melee standoff rides the existing `hero_guard_radius` keep-out, fed a new `HeroState::fp_amt` mirror of the FP blend.

**Tech Stack:** Rust / Bevy 0.19, no new dependencies. No `crates/core` changes, no save-data changes (everything here is transient view/pose state).

**Spec:** `docs/superpowers/specs/2026-07-03-fp-combat-viewmodel-design.md`

## Global Constraints

- **NO second `Camera3d`** — everything renders through the one camera (CLAUDE.md, documented triple failure).
- **The FP camera eye stays rigid** — bob/sway go on the ARM JOINTS only, never `fp_eye`. The 75% trauma/FOV-punch damping in FP stays untouched.
- **No `crates/core` edits.** No `SaveData` fields (all new state is transient).
- Near plane is `0.04` (`scene.rs::setup_camera`) — held meshes must stay > ~0.08u from the eye or they slice.
- `walk_phase` advances at `STEP_FREQ = 7.0` rad/s scaled by real speed (`movement.rs:626`) — use it directly as a radian cycle.
- New sim systems need `.run_if(in_state(Modal::None))`; pure render/anim systems stay ungated (`hero_anim` is ungated — keep it so).
- Build verification: `cargo check` after each task (fast); full `cargo run` captures in the verify steps. Confirm every capture's `Screenshot saved` log line; retry a junk frame once before debugging (CLAUDE.md).
- Git discipline: stage **explicit paths** and commit with `git commit -- <paths>` (parallel agents share the checkout; never `git add -A`).
- **Pose numbers are starting points.** Every pose/amplitude constant below was derived on paper; the verify steps render them and you WILL retune. Keep the structure, tune the numbers. Visual iteration (running captures + reading PNGs) is grunt work — per CLAUDE.md, delegate the capture-read-retune loop to an Opus subagent where practical.

## File Structure

- `src/player/anim.rs` — Tasks 1–2: FP procedural offsets, new FP rest/attack/block targets, `fp_attack_arm()`.
- `src/player/mod.rs` — Task 3: `HeroState::fp_amt` field + registering `publish_fp_state`.
- `src/player/camera.rs` — Task 3: `publish_fp_state` system, `FP_FOV_WIDEN`.
- `src/orks.rs` — Task 3: `FP_STANDOFF`, `hero_guard_radius` fp param, brain `atk_range`.
- `src/player/movement.rs`, `src/wildlife.rs`, `src/siege.rs` — Task 3: call-site updates.
- `src/player/combat.rs` — Task 4: `FOREST_SWINGTEST` staging hook.
- `CLAUDE.md` — Task 4: document the new env hook in the staging table.

---

### Task 1: Procedural FP weapon motion + retuned rest pose

**Files:**
- Modify: `src/player/anim.rs` (`hero_anim`, ~lines 871–1058)

**Interfaces:**
- Consumes: `Hero { walk_phase, moving_amt, run_amt, facing }`, `FirstPerson::blend` (already in the system).
- Produces: FP rest-pose targets fed by four procedural scalars `(fp_breath, fp_bob_v, fp_bob_l, fp_sway)`. Task 2 reuses the same scalars for its block targets, and replaces the `attack.is_none()` gates this task keeps.

- [ ] **Step 1: Add two Locals to `hero_anim`'s signature** (after `mut block_amt: Local<f32>`):

```rust
    // FP turn-sway state: last frame's view yaw + the low-passed yaw rate (rad/s).
    mut fp_prev_yaw: Local<f32>,
    mut fp_sway_amt: Local<f32>,
```

- [ ] **Step 2: Compute the procedural scalars once, right after `let fp_amt = fp.blend...` (line ~920):**

```rust
    // ── First-person viewmodel: procedural weapon motion ──
    // The FP arms have no clip of their own — without this they freeze at the static tuck and read
    // "glued to the camera". Three small joint-space layers (radians), all camera-untouched:
    // breath (idle heave), walk-bob (stride pump), turn-sway (weapon lags the view yaw).
    let (fp_breath, fp_bob_v, fp_bob_l, fp_sway) = if fp_amt > 0.0 {
        let breath = (now * 3.1).sin() * 0.018; // ~0.5 Hz idle heave
        // Vertical pump at 2× stride (one dip per footfall), light lateral sway at 1×; a touch
        // stronger at sprint. `walk_phase` is already a real-speed radian cycle.
        let stride = moving * (1.0 + 0.4 * hero.run_amt.clamp(0.0, 1.0));
        let bob_v = (hero.walk_phase * 2.0).sin() * 0.045 * stride;
        let bob_l = hero.walk_phase.sin() * 0.03 * stride;
        // Turn-sway: low-pass the view-yaw rate (hero.facing IS look_yaw in FP — the camera writes
        // it) and tilt the weapon OPPOSITE the turn, recovering as the rate settles.
        let yaw_rate = crate::steer::wrap_pi(hero.facing - *fp_prev_yaw) / dt.max(1e-3);
        *fp_sway_amt += (yaw_rate.clamp(-8.0, 8.0) - *fp_sway_amt) * (dt * 9.0).min(1.0);
        (breath, bob_v, bob_l, (*fp_sway_amt * -0.012).clamp(-0.07, 0.07))
    } else {
        *fp_sway_amt = 0.0;
        (0.0, 0.0, 0.0, 0.0)
    };
    *fp_prev_yaw = hero.facing;
```

(If `crate::steer::wrap_pi` isn't visible from `anim.rs`, check how `movement.rs` imports it — `use crate::steer;` at module top — and mirror that.)

- [ ] **Step 3: Replace the three static FP rest targets in the per-joint loop (lines ~1002–1037) with the animated Skyrim-ready pose.** Same match arms, same gates for now (Task 2 reworks the gates); only the `target` values change:

Right arm (`Joint::ShoulderR | Joint::ElbowR` arm, line ~1012):
```rust
                    // Skyrim-style ready: hilt low bottom-right, blade angled up-and-inward (not
                    // laid flat) — alive via breath/bob/sway instead of frozen.
                    let target = if elbow {
                        rx(-1.35 + fp_bob_v * 0.6)
                    } else {
                        e3(-0.28 + fp_breath + fp_bob_v, 0.12 + fp_sway + fp_bob_l, 0.20)
                    };
                    rot = rot.slerp(target, fp_amt);
```

Sword pivot (`Joint::Sword`, line ~1022):
```rust
                if attack.is_none() && block_amt < 0.5 && fp_amt > 0.0 {
                    // Blade diagonal up-inward toward frame centre (ready stance). X well under the
                    // old 2.6 lay-flat but above ~1.1 (a vertical flagpole) — tune against a shot.
                    rot = rot.slerp(e3(1.45 + fp_bob_v * 0.5, -0.35 + fp_sway, 0.10), fp_amt);
                }
```

Left arm (`Joint::ShoulderL | Joint::ElbowL` arm, line ~1034):
```rust
                    let target = if elbow {
                        rx(-1.70 + fp_bob_v * 0.6)
                    } else {
                        e3(-0.55 + fp_breath + fp_bob_v, -0.12 + fp_sway - fp_bob_l, -0.20)
                    };
                    rot = rot.slerp(target, fp_amt);
```

- [ ] **Step 4: Add an FP shield-tilt override** so the shield's top edge reads bottom-left instead of pure edge-on. New match arm next to `Joint::Sword` in the same `match part.joint`:

```rust
            Joint::Shield => {
                // FP rest: tilt the edge-on rest shield a touch open so its top edge reads in the
                // bottom-left corner (pure edge-on is an invisible line at eye height). Blocking /
                // attacks own it otherwise. (Task 2 replaces the block half of this gate.)
                if attack.is_none() && block_amt < 0.5 && fp_amt > 0.0 {
                    rot = rot.slerp(e3(0.25, -1.15, 0.0), fp_amt);
                }
            }
```

- [ ] **Step 5: `cargo check`** — expect clean.

- [ ] **Step 6: Visual verify (stills + walk clip).** Confirm `Screenshot saved` in each run's output:

```powershell
$env:FOREST_SHOT="target/fp_rest.png"; $env:FOREST_FP="1"; $env:FOREST_HERO="-18,24"; cargo run
# walk clip — bob/sway in motion through the real FP eye:
$env:FOREST_CLIP="target/clips/fp_walk"; $env:FOREST_FP="1"; $env:FOREST_DEMO="explore"; cargo run
```

Check: sword hilt visible bottom-right with blade angled up-inward (not flat, not flagpole, not clipping the near plane); shield top edge bottom-left; arms visibly pump on the walk frames; nothing pops. Retune the constants above (amplitudes ±50%, pose angles ±0.3 rad) until it reads — this loop is delegable to an Opus subagent.

- [ ] **Step 7: Commit**

```bash
git add src/player/anim.rs
git commit -m "FP viewmodel: procedural breath/bob/turn-sway + Skyrim ready rest pose" -- src/player/anim.rs
```

---

### Task 2: FP attack swing + FP block pose

**Files:**
- Modify: `src/player/anim.rs`

**Interfaces:**
- Consumes: Task 1's `(fp_breath, fp_bob_v, fp_bob_l, fp_sway)` scalars and rest targets; existing `attack_phase` / `Phase` / `attack: Option<(Phase, f32)>` / `block_amt`.
- Produces: `fn fp_attack_arm(variant: u8, phase: &Phase, p: f32) -> (Quat, Quat, Quat)` (sh_r, el_r, sword local rotations). The `attack.is_none()` gates on ALL FP overrides are removed — after this task the FP layer is always live in FP.

- [ ] **Step 1: Add `fp_attack_arm` near `attack_pose` (~line 540):**

```rust
/// First-person sword-arm targets per attack phase. The TPS clips carry their arc in the torso +
/// shoulder (authored for over-the-shoulder framing) — at eye scale that throws the arm across the
/// whole lens. In FP the arc lives in the FOREARM + wrist instead: wind-up pulls the hilt out of
/// frame, the strike sweeps the blade through the LOWER HALF of the frame, recovery settles back
/// to the ready tuck. Returns (sh_r, el_r, sword) local rotations, slerped by the FP blend over
/// the TPS attack pose in `hero_anim`. Angles hand-tuned against `FOREST_SWINGTEST` clips.
pub(crate) fn fp_attack_arm(variant: u8, phase: &Phase, p: f32) -> (Quat, Quat, Quat) {
    // FP ready-tuck baseline (keep in sync with the rest targets in `hero_anim`):
    const SH: (f32, f32, f32) = (-0.28, 0.12, 0.20);
    const EL: f32 = -1.35;
    const SW: (f32, f32, f32) = (1.45, -0.35, 0.10);
    match variant {
        // Horizontal slash — cock to the right edge, sweep across the lower third, settle back.
        1 => match phase {
            Phase::Wind => (
                e3(lerp(SH.0, -0.40, p), lerp(SH.1, 0.60, p), SH.2),
                rx(lerp(EL, -1.10, p)),
                e3(lerp(SW.0, 1.70, p), lerp(SW.1, -1.00, p), lerp(SW.2, -0.25, p)),
            ),
            Phase::Strike => (
                e3(-0.40, lerp(0.60, -0.55, p), SH.2),
                rx(lerp(-1.10, -1.30, p)),
                e3(lerp(1.70, 1.55, p), lerp(-1.00, 0.60, p), lerp(-0.25, 0.20, p)),
            ),
            Phase::Recovery => (
                e3(lerp(-0.40, SH.0, p), lerp(-0.55, SH.1, p), SH.2),
                rx(lerp(-1.30, EL, p)),
                e3(lerp(1.55, SW.0, p), lerp(0.60, SW.1, p), lerp(0.20, SW.2, p)),
            ),
        },
        // Forward thrust — hilt back to the hip, drive straight at frame centre, pull back.
        2 => match phase {
            Phase::Wind => (
                e3(lerp(SH.0, 0.10, p), lerp(SH.1, 0.25, p), SH.2),
                rx(lerp(EL, -1.65, p)),
                e3(lerp(SW.0, 1.60, p), lerp(SW.1, -0.10, p), SW.2),
            ),
            Phase::Strike => (
                e3(lerp(0.10, -0.75, p), lerp(0.25, 0.05, p), SH.2),
                rx(lerp(-1.65, -0.45, p)),
                e3(lerp(1.60, 1.35, p), lerp(-0.10, 0.0, p), lerp(SW.2, 0.0, p)),
            ),
            Phase::Recovery => (
                e3(lerp(-0.75, SH.0, p), lerp(0.05, SH.1, p), SH.2),
                rx(lerp(-0.45, EL, p)),
                e3(lerp(1.35, SW.0, p), lerp(0.0, SW.1, p), lerp(0.0, SW.2, p)),
            ),
        },
        // Overhead / heavy chop — raise top-right, cut down through lower-centre. The heavy shares
        // the shape; its longer wind + charge_stance already read through the damped torso.
        _ => match phase {
            Phase::Wind => (
                e3(lerp(SH.0, -1.05, p), lerp(SH.1, 0.30, p), lerp(SH.2, 0.10, p)),
                rx(lerp(EL, -0.85, p)),
                e3(lerp(SW.0, 2.05, p), lerp(SW.1, -0.15, p), SW.2),
            ),
            Phase::Strike => (
                e3(lerp(-1.05, -0.15, p), lerp(0.30, 0.0, p), 0.10),
                rx(lerp(-0.85, -1.25, p)),
                e3(lerp(2.05, 1.75, p), lerp(-0.15, 0.15, p), lerp(SW.2, -0.10, p)),
            ),
            Phase::Recovery => (
                e3(lerp(-0.15, SH.0, p), lerp(0.0, SH.1, p), lerp(0.10, SH.2, p)),
                rx(lerp(-1.25, EL, p)),
                e3(lerp(1.75, SW.0, p), lerp(0.15, SW.1, p), lerp(-0.10, SW.2, p)),
            ),
        },
    }
}
```

- [ ] **Step 2: Rework the per-joint FP overrides in `hero_anim` — remove every `attack.is_none()` gate.** The whole `match part.joint` block (lines ~1002–1039 post-Task-1) becomes:

```rust
        match part.joint {
            Joint::ShoulderR | Joint::ElbowR => {
                let elbow = part.joint == Joint::ElbowR;
                if let Some((Some((sh, el)), _)) = gesture {
                    rot = if elbow { el } else { sh };
                } else if fp_amt > 0.0 {
                    let target = if let Some((phase, p)) = &attack {
                        // FP-framed swing (see `fp_attack_arm`) instead of the raw TPS arc.
                        let (sh, el, _) = fp_attack_arm(hero.attack_variant, phase, *p);
                        if elbow { el } else { sh }
                    } else if elbow {
                        rx(-1.35 + fp_bob_v * 0.6)
                    } else {
                        e3(-0.28 + fp_breath + fp_bob_v, 0.12 + fp_sway + fp_bob_l, 0.20)
                    };
                    rot = rot.slerp(target, fp_amt);
                }
            }
            Joint::Sword => {
                if fp_amt > 0.0 {
                    let target = if let Some((phase, p)) = &attack {
                        let (_, _, sw) = fp_attack_arm(hero.attack_variant, phase, *p);
                        sw
                    } else {
                        // Ready tuck (blade up-inward); the block lowers it slightly out of the
                        // shield's way.
                        let ready = e3(1.45 + fp_bob_v * 0.5, -0.35 + fp_sway, 0.10);
                        let blocked = e3(1.85, -0.55, 0.10);
                        ready.slerp(blocked, block_amt)
                    };
                    rot = rot.slerp(target, fp_amt);
                }
            }
            Joint::ShoulderL | Joint::ElbowL => {
                let elbow = part.joint == Joint::ElbowL;
                if let Some((_, Some((sh, el)))) = gesture {
                    rot = if elbow { el } else { sh };
                } else if fp_amt > 0.0 {
                    // Rest tuck ⇄ FP block raise (shield up-centre, top edge just below the
                    // eye-line, ~⅓ of the frame — readable "guarding", not a plywood wall).
                    // During an attack the off-arm just holds the tuck (the TPS off-arm sweep
                    // reads as noise at eye scale).
                    let restt = if elbow {
                        rx(-1.70 + fp_bob_v * 0.6)
                    } else {
                        e3(-0.55 + fp_breath + fp_bob_v, -0.12 + fp_sway - fp_bob_l, -0.20)
                    };
                    let blockt = if elbow { rx(-1.35) } else { e3(-0.90, -0.05, -0.10) };
                    rot = rot.slerp(restt.slerp(blockt, block_amt), fp_amt);
                }
            }
            Joint::Shield => {
                if fp_amt > 0.0 {
                    // Rest: edge-on tilted a touch open. Block: face-on, tilted slightly back so
                    // the rim (not the flat) catches the eye-line.
                    let restt = e3(0.25, -1.15, 0.0);
                    let blockt = e3(PI / 2.0 - 0.25, 0.0, 0.0);
                    rot = rot.slerp(restt.slerp(blockt, block_amt), fp_amt);
                }
            }
            // FP damps the big TPS body-english during a swing: the torso/head cranking is what
            // threw the whole arm across the lens (the clips are authored for over-the-shoulder
            // framing). Legs keep locomotion untouched.
            Joint::Torso | Joint::Head => {
                if attack.is_some() && fp_amt > 0.0 {
                    rot = rot.slerp(Quat::IDENTITY, fp_amt * 0.8);
                }
            }
            _ => {}
        }
```

- [ ] **Step 3: Damp the hips' attack drive in FP** — the strike shoves `hips.z` +0.32 forward (toward the near plane). Right after `tf.rotation = rot;` (before the landing-squash block), add:

```rust
        // FP: damp the swing's hip drive/rotation — the forward shove pushes the chest into the
        // near plane and the yaw-crank swings the whole viewmodel sideways.
        if part.joint == Joint::Hips && attack.is_some() && fp_amt > 0.0 {
            tf.translation.z *= 1.0 - 0.7 * fp_amt;
            tf.rotation = tf.rotation.slerp(Quat::IDENTITY, fp_amt * 0.8);
        }
```

- [ ] **Step 4: `cargo check`** — expect clean. (`gesture` destructuring and `attack` borrow patterns are unchanged; only gates moved.)

- [ ] **Step 5: Interim visual sanity (manual swing).** Full swing verification needs Task 4's `FOREST_SWINGTEST`; here just confirm nothing broke in TPS and the FP block reads:

```powershell
$env:FOREST_SHOT="target/fp_block.png"; $env:FOREST_FP="1"; $env:FOREST_EQUIP="sword_gold"; cargo run
# TPS regression: attacks/block must look EXACTLY as before (fp_amt = 0 short-circuits all of it)
$env:FOREST_SHOT="target/tps_check.png"; $env:FOREST_TPS="1"; $env:FOREST_HERO="-18,24"; cargo run
```

(For the block still: `FOREST_FP` boots Play; the block pose itself is verified properly in Task 4's clip pass — this still just catches gross breakage.)

- [ ] **Step 6: Commit**

```bash
git add src/player/anim.rs
git commit -m "FP viewmodel: FP-framed attack swings + raised block pose, damped body-english" -- src/player/anim.rs
```

---

### Task 3: Melee standoff + FP FOV widen

**Files:**
- Modify: `src/player/mod.rs` (`HeroState`, plugin registration)
- Modify: `src/player/camera.rs` (`publish_fp_state`, `FP_FOV_WIDEN`)
- Modify: `src/orks.rs` (`FP_STANDOFF`, `hero_guard_radius`, brain `atk_range`, lunge-clamp call site)
- Modify: `src/player/movement.rs` (shove call site)
- Modify: `src/wildlife.rs` (call site — explicitly excluded from FP standoff)
- Modify: `src/siege.rs` (`at_hero` range)

**Interfaces:**
- Consumes: `FirstPerson::blend` (camera), `HeroState` (already read by orks/siege/wildlife/movement).
- Produces: `HeroState::fp_amt: f32` (0..1, mirrored each frame); `orks::FP_STANDOFF: f32 = 0.5`; new signature `orks::hero_guard_radius(hero_pos: Vec2, hero_facing: f32, blocking: bool, fp_amt: f32, attacker: Vec2) -> f32`.

- [ ] **Step 1: Add the field to `HeroState`** (`src/player/mod.rs:324`):

```rust
    /// First-person blend (0 = third-person … 1 = FP), mirrored from `FirstPerson::blend` each
    /// frame by `camera::publish_fp_state` — read by the ork keep-out + attack-range so melee
    /// holds a readable distance from the FP eye instead of filling the lens.
    pub fp_amt: f32,
```

- [ ] **Step 2: Add the publisher system** in `src/player/camera.rs` (next to `fp_body_visibility`):

```rust
/// Mirror the FP blend into [`HeroState`] for cross-module readers (the ork keep-out/attack-range
/// in `orks`/`siege`). A separate tiny system because `player_camera` sits at Bevy's 16-param
/// ceiling and `HeroState`'s other writers (`movement`, `block`) have no `FirstPerson` need.
pub fn publish_fp_state(fp: Res<FirstPerson>, mut state: ResMut<super::HeroState>) {
    state.fp_amt = fp.blend.clamp(0.0, 1.0);
}
```

Register it in `PlayerPlugin::build` in the same `Update` tuple as `fp_body_visibility` (grep `fp_body_visibility` in `src/player/mod.rs` for the registration line) as `camera::publish_fp_state`. It's render-side mirroring like `fp_body_visibility` — same gating (none).

- [ ] **Step 3: `FP_STANDOFF` + `hero_guard_radius` in `src/orks.rs`** (~line 597, after `SHIELD_REACH`):

```rust
/// Extra standoff a melee ork keeps in FIRST PERSON (scaled by the FP blend): the TPS keep-out
/// parks a grunt's centre ~0.86u from the hero's centre — fine over the shoulder, but the FP eye
/// sits AT the hero's centre, so the scale-1.35 mesh fills the lens. Held this much further out —
/// with `ORK_ATTACK_RANGE` extended by the same amount (see `ork_brain`/`siege`) so it can still
/// strike — the fight stays readable. Orks only: wildlife's bite-stop (~1.2u) would need its own
/// matching extension, deliberately not done in this pass.
pub(crate) const FP_STANDOFF: f32 = 0.5;
```

Change `hero_guard_radius` (line ~605):

```rust
pub(crate) fn hero_guard_radius(
    hero_pos: Vec2,
    hero_facing: f32,
    blocking: bool,
    fp_amt: f32,
    attacker: Vec2,
) -> f32 {
    let base = HERO_GUARD_R + FP_STANDOFF * fp_amt.clamp(0.0, 1.0);
    if !blocking {
        return base;
    }
    let to = attacker - hero_pos;
    let d = to.length();
    if d < 1e-4 {
        return base;
    }
    // cos(bearing): +1 dead ahead of the hero's facing, 0 abeam, <0 behind.
    let fwd = Vec2::new(hero_facing.sin(), hero_facing.cos());
    let aim = (to.x * fwd.x + to.y * fwd.y) / d;
    base + SHIELD_REACH * aim.max(0.0)
}
```

- [ ] **Step 4: Update the three call sites:**

`src/orks.rs` ~539 (lunge clamp):
```rust
            let keep = o.body_r + hero_guard_radius(hero.pos, hero.facing, hero.blocking, hero.fp_amt, o.pos);
```

`src/player/movement.rs` ~563 (body shove):
```rust
        let guard = crate::orks::hero_guard_radius(hero.pos, hero.facing, blocking, state.fp_amt, o.pos);
```

`src/wildlife.rs` ~483 (render lunge clamp) — pass `0.0` explicitly:
```rust
            // fp_amt 0.0: wildlife keeps the TPS keep-out (FP standoff is orks-only this pass —
            // extending it here without also extending the predators' bite-stop would hold the
            // rendered snout off a body that still presses to the old line).
            let guard = crate::orks::hero_guard_radius(hero.pos, hero.facing, hero.blocking, 0.0, a.pos);
```

- [ ] **Step 5: Extend the ork attack-entry range by the same standoff.**

`src/orks.rs` ~356 (`ork_brain`):
```rust
        let atk_range =
            if o.shaman { SHAMAN_CAST_RANGE } else { ORK_ATTACK_RANGE + FP_STANDOFF * hero.fp_amt };
```
(Line ~394's rival-brawl check keeps plain `ORK_ATTACK_RANGE` — ork vs ork, no hero eye involved.)

`src/siege.rs` ~1047–1049 — hero check only (`at_guard` is ork-vs-guard, unchanged):
```rust
        let atk_range = if o.shaman { orks::SHAMAN_CAST_RANGE } else { orks::ORK_ATTACK_RANGE };
        // In FP the keep-out holds melee further off the eye — extend the hero strike range to
        // match or the shove parks them permanently out of their own reach (melee would die).
        let fp_reach = if o.shaman { 0.0 } else { orks::FP_STANDOFF * hero.fp_amt };
        let at_hero = chase_hero && hold_pt.is_none() && o.pos.distance(hero.pos) < atk_range + fp_reach;
        let at_guard = guard_tgt.is_some_and(|(_, gp)| o.pos.distance(gp) < atk_range);
```

Sanity: grunt min centre-distance in FP = `body_r 0.36 + 0.5 + 0.5` = 1.36 < extended range 2.0 → strikes land. Player `ATTACK_RANGE` 1.8 > 1.36 → hero still reaches. `melee_ring::RING_R` 2.9 untouched.

- [ ] **Step 6: FP FOV widen** in `src/player/camera.rs`. Constant next to `SPRINT_FOV_DEG` (~line 130):

```rust
/// Extra base-FOV widen (degrees) at full first-person — optically pulls near melee back so a
/// pressed-in ork subtends less of the frame. Blended by `fp.blend` (seamless with the toggle).
const FP_FOV_WIDEN: f32 = 10.0;
```

And in the FOV block (~line 444), `fpb` is already in scope:
```rust
        p.fov = base + (kick + speed_fov + combat_fov + fpb * FP_FOV_WIDEN).to_radians();
```

- [ ] **Step 7: `cargo check`** — expect clean (the signature change surfaces any call site missed; there are exactly three).

- [ ] **Step 8: Run the core tests** (nothing here touches core, this is the cheap regression gate): `cargo test` — expect all ~268 pass.

- [ ] **Step 9: Visual verify — FP siege standoff:**

```powershell
$env:FOREST_CLIP="target/clips/fp_siege"; $env:FOREST_CLIP_FRAMES="240"; $env:FOREST_FP="1"; $env:FOREST_WAVE="1"; $env:FOREST_DEFEND="1"; cargo run
```

Check frames: orks hold visibly off the eye (whole silhouette readable, not a torso wall), still swing and land hits (hit flash + damage), TPS unchanged (`FOREST_TPS` + `FOREST_WAVE` spot-check). Tune `FP_STANDOFF` (0.4–0.7) and `FP_FOV_WIDEN` (8–14) if the frame still chokes.

- [ ] **Step 10: Commit**

```bash
git add src/orks.rs src/siege.rs src/wildlife.rs src/player/mod.rs src/player/camera.rs src/player/movement.rs
git commit -m "FP melee readability: fp-scaled ork standoff + matched attack range + FOV widen" -- src/orks.rs src/siege.rs src/wildlife.rs src/player/mod.rs src/player/camera.rs src/player/movement.rs
```

---

### Task 4: FOREST_SWINGTEST hook + final verification pass

**Files:**
- Modify: `src/player/combat.rs` (`player_attack`)
- Modify: `CLAUDE.md` (env-hook table row)

**Interfaces:**
- Consumes: `begin_swing(hero, heavy, aim, now, fp)` (combat.rs:119), `ATTACK_DURATION`.
- Produces: `FOREST_SWINGTEST=1` — loops swings (3 light variants + a Heavy every 4th) on a ~1.6s timer, no keypress/pointer-lock, same pattern as `FOREST_ROLLTEST`.

- [ ] **Step 1: Add two Locals to `player_attack`'s signature** (before `mut hero_q`):

```rust
    // FOREST_SWINGTEST state: next fire time + swing counter (cycles the clip variants).
    mut next_test_swing: Local<f32>,
    mut test_swing_n: Local<u32>,
```

- [ ] **Step 2: Add the hook after the early-return guard block (after line ~641, `let can_act = ...`):**

```rust
    // Debug/capture hook: `FOREST_SWINGTEST=1` fires a swing every ~1.6s — cycling the three light
    // clips, a Heavy every 4th — skipping the pointer-lock/press gates (same pattern as
    // `FOREST_ROLLTEST`), so a `FOREST_FP`/`FOREST_TPS` clip can film every swing shape unattended.
    if std::env::var("FOREST_SWINGTEST").is_ok() && !hero.attacking && now_s >= *next_test_swing {
        *next_test_swing = now_s + 1.6;
        let heavy = *test_swing_n % 4 == 3;
        begin_swing(&mut hero, heavy, None, now_s, true);
        if !heavy {
            // Force the variant cycle (begin_swing's combo chain would need timed presses).
            hero.attack_variant = (*test_swing_n % 3) as u8;
            hero.attack_dur = ATTACK_DURATION;
        }
        *test_swing_n += 1;
        cues.write(AudioCue::HeroGruntSwing);
    }
```

- [ ] **Step 3: Add the CLAUDE.md staging-table row** (in the env-hook table, next to `FOREST_ROLLTEST`):

```markdown
| `FOREST_SWINGTEST=1` | loop hero attack swings (~1.6s apart, cycling overhead/slash/thrust + a Heavy every 4th; skips input gates) so a `FOREST_FP`/`FOREST_TPS` shot/clip frames every swing shape (`player/combat.rs::player_attack`) |
```

- [ ] **Step 4: `cargo check`** — expect clean.

- [ ] **Step 5: Film + retune the FP swing set (the point of this whole plan — budget real iteration here, delegable to an Opus subagent):**

```powershell
$env:FOREST_CLIP="target/clips/fp_swings"; $env:FOREST_CLIP_FRAMES="300"; $env:FOREST_FP="1"; $env:FOREST_SWINGTEST="1"; $env:FOREST_HERO="-18,24"; cargo run
```

Read the frames per swing: thrust drives to frame centre; slash sweeps the lower third; chop cuts top-right → lower-centre; heavy reads as raise → smash; blade never slices the near plane; recovery settles into the ready tuck with no pop. Retune `fp_attack_arm` angles until each clears. Then the TPS regression clip (swings must be pixel-identical to pre-branch):

```powershell
$env:FOREST_CLIP="target/clips/tps_swings"; $env:FOREST_CLIP_FRAMES="300"; $env:FOREST_TPS="1"; $env:FOREST_SWINGTEST="1"; $env:FOREST_HERO="-18,24"; cargo run
```

- [ ] **Step 6: Full-loop FP combat proof** — siege in FP with everything on:

```powershell
$env:FOREST_CLIP="target/clips/fp_combat"; $env:FOREST_CLIP_FRAMES="360"; $env:FOREST_FP="1"; $env:FOREST_WAVE="1"; $env:FOREST_DEFEND="1"; $env:FOREST_SWINGTEST="1"; cargo run
```

Check the combined read: standoff + FOV + swings + block together; orks visible and hittable; hits land both ways.

- [ ] **Step 7: `cargo test`** one last time — expect all pass.

- [ ] **Step 8: Commit + push (feature complete):**

```bash
git add src/player/combat.rs CLAUDE.md
git commit -m "FOREST_SWINGTEST staging hook + doc row; FP combat viewmodel verified" -- src/player/combat.rs CLAUDE.md
git push
```

---

## Self-review notes

- Spec coverage: §1 → Task 1; §2 → Task 2; §3 → Task 3; §4 (harness + scope) → Task 4 + per-task verify steps. No gaps.
- The spec's "files touched" listed `src/player/mod.rs` for pose-spawn changes — not needed (no mesh changes; `fp_keep`/`fp_hide` untouched), `mod.rs` is touched only for `HeroState`.
- Type consistency: `fp_attack_arm` returns `(Quat, Quat, Quat)` and is consumed with matching destructuring in Task 2; `hero_guard_radius`'s new 5-param signature matches all three call sites in Task 3.
- All pose constants flagged as starting points with explicit tuning ranges — deliberate, not placeholders: the mechanism is complete, the numbers are visual-iteration inputs.
