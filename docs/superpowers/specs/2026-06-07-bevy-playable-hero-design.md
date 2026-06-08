# Playable Hero — Bevy forest scene (design)

- **Date:** 2026-06-07
- **Status:** implemented (M1–M3), compiles clean; visual verification in progress

## Implementation notes (as built)

- All three milestones landed in one pass. The hero lives under `src/player/`
  (`mod`/`model`/`movement`/`camera`/`combat`/`block`/`health`/`anim`) + `src/hud.rs`.
- **Concurrency win:** combat targeting lives entirely on the player side. `Health` is
  attached to orks/wildlife by `combat::ensure_combat_health` (they're `pub` queryable
  types), and the swing scans their `GlobalTransform` — so M2 touched **zero** shared files.
  Only M3's ork *aggro* needed `orks.rs` edits (additive: `Hunt`/`Attack` modes + `atk_cd`).
- **Ork → hero damage** flows through a `PendingHeroDamage(f32)` resource (orks `+=`,
  `health::apply_hero_damage` drains) — the store-mediated channel, no events/messages.
- **Hero → world:** `HeroState` resource (pos/facing/alive) written by movement, read by ork
  AI. `alive=false` while down so orks disengage the corpse.
- Death = respawn at the north gate after 1.6 s (no succession, per scope).
- HUD = two `bevy_ui` rectangles (HP + block-stamina), no text.
- Wildlife flee reuses the existing camera-startle (the follow-cam trails the hero), so
  `wildlife.rs` was left untouched.
- Knife-edge Bevy-0.18 gotchas hit: `EventReader`→ gone (used `AccumulatedMouseScroll`);
  `pub` systems can't name private param types across modules (made FX types `pub(crate)`).
- **Target repo:** `D:\tileworld-bevy-forest` (standalone Bevy 0.18.1 forest scene)
- **Source of behaviour:** the TS game `D:\tileworld\src\world\` — `Character.tsx`,
  `MouseLookCamera.tsx`, `useKeyboard.ts`, `blockStore.ts`, `playerStore.ts`.

## Goal

Turn the static forest showcase scene into a **playable** one: a knight hero the user
drives in third-person — walk / run / jump with terrain grounding + collision, a swung
sword that kills camp orks and wildlife, a shield block, hero HP that can run out (death →
respawn), and a minimal HP/stamina HUD. The existing **free-roam fly camera is kept as a
debug toggle**.

This is a faithful port of the TS hero's *feel* (same constants where they transfer), not a
port of the whole TS game.

## Decisions (locked with user)

- **Scope:** full hero port — locomotion + swing combat + shield block + fall damage +
  hero HP/death. (FX/footsteps/VO are nice-to-have, not required for milestone completion.)
- **Model:** build a dedicated knight model via the `model-smith` skill (not a reskinned
  villager).
- **Camera:** port the TS over-the-shoulder orbit + pointer-lock (click → lock, mouse
  rotates, Esc → release, wheel → zoom).
- **Combat:** two-way. Orks get aggro/chase/attack AI + HP; the hero has HP, takes damage,
  can die → **respawn at a castle gate**. Wildlife stays passive (flees, can be killed).
- **HUD:** minimal — an HP bar + a block-stamina bar, nothing else (`bevy_ui`, no new deps).

## Out of scope (this pass)

XP / gold / leveling, inventory + equipment swaps, weapon variants beyond a single sword,
the "Blade Passes" succession death, biome voice lines, the full economy. Wildlife stays
non-aggressive. No new runtime point lights (recompile-stutter ban) — combat FX use the
existing particle/emissive paths only.

## Architecture

A new decomposed `player/` module (small focused systems, matching how the scene is already
split across `orks` / `wildlife` / `steer` / `camps`), plus minimal additive edits to a few
shared files.

### New `player/` module

| File | Responsibility |
|------|----------------|
| `player/mod.rs` | `PlayerPlugin`; spawns the hero at a castle gate; owns the core components + resources; registers all sub-systems in the right order. |
| `player/model.rs` | Knight mesh (model-smith): torso / 2 legs / 2 arms / head / sword / shield as child parts tagged `HeroPart { kind }` (reuses the `critters::PartKind` rig like `orks.rs`). One shared vertex-colour material. Registered in the inspect harness. |
| `player/movement.rs` | Keyboard → camera-relative move vector; axis-separated collision via `steer::step_clear` / `can_stand` + `blockers::is_blocked`; ground via `worldmap::ground_at_world`; gravity / jump / fall-damage; sprint; smooth turn-to-facing. |
| `player/camera.rs` | Third-person orbit (azimuth / pitch / dist) + pointer-lock; wheel zoom; `PlayMode` toggle that hands control to / from the existing `controls::FlyCam`. |
| `player/combat.rs` | LMB → timed swing; cone scan over orks + wildlife within range; apply damage / knockback / kill + particle FX. |
| `player/block.rs` | RMB → shield raise; stamina drain / regen / lock; sets a "blocking + facing" state that `health.rs` reads to mitigate incoming hits. |
| `player/health.rs` | `damage_hero` (block-aware, mitigates frontal hits); death → brief lie-down → respawn at a castle gate with full HP. |
| `player/anim.rs` | Limb drivers on `HeroPart` children — walk / idle leg+arm swing, jump pose, attack-swing arm, shield raise, body bob/lean — same `sin`-driver approach as `ork_limbs`. |

### New top-level file

- `hud.rs` — `HudPlugin`: a `bevy_ui` HP bar + block-stamina bar bound to `HeroHealth`.

### Touched shared files (ADDITIVE, coordinate with concurrent agent)

- `orks.rs` — add `Hunt` / `Attack` modes to `OrkMode`; add `hp` + `hurt_flash` to `Ork`;
  aggro when the hero is within sight, chase toward the hero via `steer::advance`, attack on
  contact (queues hero damage), leash back home past a max-chase radius; die / despawn at
  `hp <= 0`.
- `wildlife.rs` — flee from the hero when near (passive); take swing damage → die.
- `controls.rs` — gate `fly_camera` to run only in `PlayMode::FreeRoam`.
- `main.rs` — `mod player; mod hud;` + register `PlayerPlugin`, `HudPlugin`.

> Fallback if `orks.rs` churn collides with the concurrent agent: the hero-combat
> *targeting* (taking damage from a swing) can be attached from the player module via a
> marker component + an externally-added system, leaving `orks.rs` to own only the
> aggro-movement fields. Decide at plan time based on what the other agent has touched.

## Data model

ECS-native (mirrors the TS module-store split: hot per-frame state on components, discrete
shared state in resources/events).

- `#[derive(Component)] Hero` — `vel_y`, `on_ground`, `facing`, `air_takeoff_y`,
  `walk_phase`, `moving_amt`, attack timers (`attacking`, `attack_t`, `hit_dealt`).
- `#[derive(Component)] HeroHealth` — `hp`, `max`, `stamina`, `block_locked`,
  `regen_pause`, `blocking`, `dead_since: Option<f32>`.
- `#[derive(Component)] HeroPart { kind: PartKind }` — animated child limbs.
- `#[derive(Resource)] PlayMode { Play, FreeRoam }` — gates camera + fly-cam + hero input.
- `#[derive(Resource)] HeroState` — world `pos: Vec2`, `y`, `facing`, `alive` — written at
  the end of `movement.rs`, read by ork AI (a resource read, not a cross-entity query, so
  system ordering stays simple).
- `#[derive(Event)] HeroHit { amount: f32, from: Vec2 }` — orks queue hits; `health.rs`
  drains them once per frame and applies block mitigation. (Mirrors TS store-mediated
  combat: no collision events, damage flows through one channel.)

## Data flow

```
keyboard/mouse ─▶ movement / combat / block (mutate Hero, HeroHealth, Transform)
movement ─▶ writes HeroState resource (pos/facing/alive)
ork_brain ─▶ reads HeroState → aggro/chase; on contact sends HeroHit event
health ─▶ drains HeroHit events, applies block mitigation, handles death/respawn
combat ─▶ reads ork/wildlife Transform+health, applies damage/kill
hud ─▶ reads HeroHealth → bar widths
PlayMode ─▶ gates camera.rs vs controls::fly_camera; freezes hero in FreeRoam
```

## Ported behaviour + constants (from TS, tuned to scene scale)

Authoring note: the forest scene is ~1 tile = 1 world unit with `GROUND_STEP = 0.5` per
height class and `steer::MAX_STEP = 0.6`. TS grid-space constants transfer directly; the
hero model is authored at TS proportions (TS knight `scale 0.5`) then scaled to sit right
next to the orks (`BASE_SCALE 0.7`) — final scale tuned by eye + the inspect harness.

- **Movement:** `SPEED 3.5` u/s, `SPRINT_MULT 1.75`, `TURN_RATE 12` (lerp-to-facing),
  camera-relative WASD, axis-separated so you slide along walls. Climb rule = `can_stand`
  (one terrace class); you may walk off any height (gravity carries you down).
- **Jump / gravity / fall:** `GRAVITY 20` u/s², `JUMP_SPEED 6.5`; `FALL_SAFE 1.1`,
  `FALL_DMG_PER_UNIT 16`, `FALL_DMG_MAX 45`; ground from `ground_at_world` (terrace top).
- **Collision radius:** `PLAYER_RADIUS 0.22` against props (`blockers`) + creatures.
- **Camera:** orbit `SENS_X 0.0035`, `SENS_Y 0.0014`; pitch clamp `[0.18, π/2 − 0.07]`;
  zoom `dist` default ~8, clamp tuned for this small map (TS `MIN_DIST 8 / MAX_DIST 150`
  is map-wide; cap lower here). Pointer-lock on canvas click, release on Esc.
- **Combat:** `ATTACK_DURATION 0.45`, `ATTACK_RANGE 1.8`, `ATTACK_CONE_DOT 0.5` (60° front
  cone); damage a flat hero value (no level scaling); knockback on non-kill.
- **Block:** stamina drain on hold / regen after a delay / lock at empty until a recover
  threshold (port `blockStore` constants); frontal hits within the block cone are mitigated.
- **Ork AI (new):** sight radius → `Hunt` (chase via `steer::advance` toward `HeroState`),
  contact → `Attack` (windup → hit → cooldown, sends `HeroHit`), leash radius → back to
  `Patrol`/home. HP per variant (grunt < berserker, etc.); `hurt_flash` on hit.

## Milestones (each independently runnable)

1. **Locomotion** — knight model + `movement` + `camera` + `PlayMode` toggle. Verify: run
   → walk/run/jump, mouse-look orbit, backtick toggles the fly-cam, hero grounds on terrain
   and can't walk through trees/cliffs/water.
2. **Offense** — `combat` + ork HP/hit-flash/death + wildlife flee/die. Verify: hero swing
   kills orks and animals; particles + knockback read right.
3. **Two-way** — ork aggro/chase/attack AI + hero HP/`block`/`health` death→respawn + HUD.
   Verify: an ork camp fights back; blocking mitigates; HP can hit 0 → respawn at a gate;
   HUD bars track.

## Concurrency / file ownership

A second agent is editing this codebase concurrently (likely around villagers/orks). Rules
for this work:

- **Own outright:** everything under `player/`, plus `hud.rs`. No coordination needed.
- **Shared, edit defensively:** `orks.rs`, `wildlife.rs`, `controls.rs`, `main.rs` — re-read
  each file immediately before editing, keep all changes additive, never `git add -A`.
- **Builds:** verify against a **separate `CARGO_TARGET_DIR`** so the other agent's
  in-progress (possibly non-compiling) code doesn't block my build, and so we don't fight
  over the shared target dir / running-exe lock.
- **Git:** the repo has no commits yet; hold commits until the user confirms, to avoid
  tangling with the other agent's integration.

## Defaults assumed

- Toggle key **backtick `` ` ``**, Play ↔ FreeRoam; scene **starts in Play**.
- Hero spawns at a **castle gate**, facing into the courtyard.
- `FOREST_SHOT` capture: hero spawns at rest, the existing static capture camera is
  unchanged, so screenshots are unaffected.

## Verification

- `model-smith` inspect harness for the knight model (no floating/sunken parts, base on
  y=0, sane bounds).
- Manual run per milestone (this scene has no automated gameplay tests; "verify" = run it).
- Build via a private `CARGO_TARGET_DIR`; treat a clean build (no `^error`) as the gate.
