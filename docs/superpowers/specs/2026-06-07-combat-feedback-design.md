# Combat feedback (floating numbers + HP bars + hurt-flash + hero hit feedback) — design

**Date:** 2026-06-07
**Status:** approved, implementing

## Goal

Port the original game's combat juice so the player can *see* combat: floating
damage numbers, ork health bars, ork hurt-flash, and hero hit feedback (screen
shake + red flash). The Bevy port currently has none of this — damage is applied
silently (the user couldn't tell orks were hitting them).

## Reference (original game)

`D:\tileworld\src\world\{fxStore.ts, FloatingText.tsx, Ork.tsx, playerStore.ts, Character.tsx}`:

- **Floating text** (`spawnFloat(text,color,x,y,z,scale)`): billboard text, rises
  ~1.3u and fades `1-k²` over `FLOAT_LIFETIME 1.1s`, pop-in overshoot, slight
  horizontal drift. Hero hit = red `-N` (`#ff5a4a`), block = `BLOCK` (`#bcd4ff`),
  ork hit = white `N` (`#ffffff`), kill = `☠` (`#9be38a`).
- **Ork HP bar** (`Ork.tsx`): billboard quad at `y 2.6`, **only when `hp<maxHp`**,
  dark bg + fg red `#d63a3a` (orange `#ffaa20` during hurt-flash), fg width scales
  with `hp/max`, left-anchored.
- **Hurt-flash**: struck ork's skin emissive flashes white, `hurtFlashUntil`.
- **Shake** (`fxStore`): trauma 0..1, offset `∝ MAX_SHAKE·trauma²`, decay 2.4/s;
  `addShake(0.22)` on a normal hit, `0.5` on death.

## Architecture

New module **`src/combat_fx.rs`** owns all four effects and attaches to orks
externally (same philosophy as `combat.rs`'s `Health`). Hit-sites only push to
queues / set resources / insert a marker component — they do no rendering.

### 1. Floating damage numbers — screen-space `bevy_ui` text

3D text in Bevy is painful; the existing HUD already uses `bevy_ui` `Node`s, so
floating numbers are UI `Text` projected to screen each frame.

- `FloatReq { world: Vec3, text: String, color: Color, scale: f32 }` and
  `FloatQueue(Vec<FloatReq>)` resource. Producers push; `spawn_floats` drains →
  one absolute-positioned UI `Text` node per request, tagged
  `FloatText { anchor: Vec3, born: f32 }`.
- `drive_floats`: each frame, project `anchor + Vec3::Y*rise*k` via
  `Camera::world_to_viewport`; set node `left`/`top`; fade `TextColor` alpha
  `1-k²`; pop-in overshoot scale via `TextFont.font_size`; despawn at
  `k>=1` (`FLOAT_LIFE 1.1s`). Behind-camera / off-screen → `Display::None`.

### 2. Ork HP bars — billboard follower entities

A follower entity (not an ork child — avoids parent counter-rotation) keeps the
billboard math trivial and despawns itself when its ork is gone.

- `HpBarAssets` resource: a unit quad mesh + dark-bg / red-fg / orange-fg unlit
  materials, built once.
- `ensure_hp_bars`: for each ork lacking a bar, spawn a follower
  `HpBar { ork: Entity }` carrying bg + fg child quads (mirrors
  `ensure_combat_health`).
- `drive_hp_bars`: position at the ork head (`y ≈ 2.6·scale`), face the camera,
  set fg `Transform.scale.x = hp/max` + left-anchor offset, `Visibility` only
  when `hp < max`, fg material orange while hurt-flashing else red. If the ork
  entity is gone → despawn the bar.

### 3. Ork hurt-flash — per-ork material

Orks currently share one material for batching, but there are only dozens of
orks, so a per-ork material clone is negligible.

- `Armory::spawn` clones the base material per ork and stores `OrkSkin(Handle)`
  on the root (and uses it for the ork's meshes).
- On a hero hit, `player_attack` inserts `HurtFlash { until }` on the struck ork.
- `ork_flash`: drives that ork's material `emissive` white → 0 over ~0.12s;
  removes `HurtFlash` when elapsed. The HP bar reads `HurtFlash` for its orange
  tint.

### 4. Hero hit feedback — shake + red flash

- `HitFeedback { flash: f32, trauma: f32 }` resource, bumped in
  `apply_hero_damage` (`flash=0.35`, `trauma += 0.22`, more on death).
- A full-screen red UI `Node` (spawned at startup); `drive_hit_flash` sets its
  `BackgroundColor` alpha from a decaying `flash`.
- Screen shake: `drive_shake` decays `trauma`; `player_camera` adds a
  `trauma²`-scaled offset to the final camera translation (small edit there, so
  the offset lands after the camera is positioned — correct ordering).

### Wiring

- `CombatFxPlugin` registers `FloatQueue`, `HitFeedback`, `HpBarAssets` (Startup),
  the red-flash node (Startup), and the drive/ensure systems (Update). Added to
  `main.rs`.
- Hit-sites that change: `combat::player_attack` (push ork/animal/kill floats +
  insert `HurtFlash`), `health::apply_hero_damage` (push hero `-N`/`BLOCK` float +
  set `HitFeedback`), `orks::Armory::spawn` (per-ork material + `OrkSkin`),
  `player/camera.rs::player_camera` (apply shake offset).

## Constants (ported, scaled)

| Name | Value | Origin |
|------|-------|--------|
| `FLOAT_LIFE` | `1.1` | FLOAT_LIFETIME 1.1 |
| float rise | `1.3` | rise 1.3u |
| hero-hit colour | `#ff5a4a` | playerStore |
| block colour | `#bcd4ff` | playerStore |
| ork-hit colour | `#ffffff` | Character |
| kill glyph/colour | `☠` / `#9be38a` | Character |
| HP bar height | `2.6·scale` | Ork.tsx y 2.6 |
| fg red / orange | `#d63a3a` / `#ffaa20` | Ork.tsx |
| hurt-flash dur | `0.12` s | — |
| shake decay | `2.4`/s | fxStore TRAUMA_DECAY |
| shake on hit / death | `+0.22` / `+0.5` | playerStore |
| flash alpha on hit | `0.35` | — |

## Out of scope

Crits (no crit system in the port), FOV punch (shake covers the feel), gold/XP
floats (no economy in the port).

## Verification

- `cargo build` clean; app boots 120 frames without panic (screenshot harness).
- Manual: fight a camp — red `-N` + screen flash/shake when hit, white `N` + ork
  hurt-flash + shrinking HP bar when striking an ork, `☠` on kill.
