# Szaman ranged caster + healer — design

**Date:** 2026-06-07
**Status:** approved, ready for plan

## Goal

Port the original D: Tileworld shaman behaviour into the Bevy forest scene. Orks
already chase + club the hero (`orks.rs` `Hunt`/`Attack`, damage queued onto
`PendingHeroDamage`). The shaman currently clubs identically to a grunt — wrong.
It must instead be a **ranged caster** that lobs homing magic bolts at the hero
and **heals wounded warband allies**.

## Reference (original game)

`D:\tileworld\src\world\{orkConfig.ts, Ork.tsx, projectileStore.ts}`:

- `shaman` config: `ranged: true`, `rangedRange: 12`, `aggro: 15`, `melee: 11`
  (= preferred cast distance), `damage: 26` (bolt), `attackDuration: 0.6`,
  `attackCooldown: 2.1`, `speed: 1.8`, `healAmount: 24`, `healCooldown: 5`,
  `healRange: 8`.
- Shaman never melees. When target within `rangedRange`, mid-cast (anim phase
  0.55) it `spawnBolt(...)` from the staff orb (`y + 1.7`) at the target.
- Bolt (`projectileStore.ts`): homes the target's **live** position, `speed 9`,
  `ttl 3s`, hit radius `0.6`, fizzles past `maxRange` (= `rangedRange + 4`),
  deals damage on arrival, shield-blockable (origin passed so the block cone
  faces the bolt).
- Heal: on its own timer, finds nearest wounded ally within `healRange`, restores
  `healAmount`, resets timer by `healCooldown`. Independent of attacking.

## Scope (scaled to the Bevy scene)

The Bevy scene is simpler than the original: hero-only aggro, no rival camps /
keep marching / defenders, flat `ORK_HP = 60`, a single `blocking` flag (not a
directional cone). The port targets that scene.

**In scope**

1. Shaman casts homing bolts at the hero from range (replaces its melee).
2. Shaman heals wounded warband allies on a timer.
3. Glowing bolt FX + green heal motes (reuse the existing spark system).

**Out of scope** (absent from the Bevy scene): rival-ork bolt targeting,
keep-marching, defender targeting, floating damage/heal text, directional shield
cone (the existing flat `blocking` mitigation applies for free).

## Architecture

### New module `src/projectile.rs` (mirrors `projectileStore.ts`)

A self-contained, reusable projectile unit. Communicates with ork AI through a
queue resource — the same "pending channel" idiom as `PendingHeroDamage`, so the
ork brain needs no `Commands` or asset handles.

- `BoltSpawn { origin: Vec3, damage: f32 }` and
  `BoltSpawns(Vec<BoltSpawn>)` resource. Producers push; the projectile plugin
  drains it each frame and spawns bolt entities.
- `Bolt` component: `{ damage, speed, ttl, traveled, max_range }`. Homes the
  hero's live position (`HeroState.pos`, `+1.0` y). On arrival (`dist < 0.6`):
  `PendingHeroDamage += damage`, despawn, impact spark. On `ttl <= 0` or
  `traveled >= max_range`: despawn (fizzle).
- `BoltAssets` resource: one emissive purple orb mesh + `unlit` material built
  once at startup (orb colour `0xc89cff`, matching the staff orb).
- Systems: `setup_bolt_assets` (Startup), `spawn_queued_bolts` (drain queue),
  `step_bolts` (home + move + hit/fizzle). Registered by a `ProjectilePlugin`.

Damage routes through `PendingHeroDamage`, so the existing block mitigation
(`health::apply_hero_damage`, `BLOCK_MITIGATION`) applies with no extra work.

### `orks.rs` changes — branch on `Ork.shaman: bool`

- Add `shaman: bool` to `Ork` (already in `Stats`), and a `heal_cd: f32` timer.
- Shaman skips melee. The `Hunt → Attack` mode threshold uses
  `SHAMAN_CAST_RANGE` (stand off) instead of `ORK_ATTACK_RANGE`.
- `Attack` branch: a shaman pushes a `BoltSpawn` (damage `SHAMAN_BOLT_DAMAGE`,
  origin at staff-orb height) on `SHAMAN_CAST_CD`, instead of
  `pending += ORK_DAMAGE`.
- New `shaman_heal` system: for each shaman whose `heal_cd <= 0`, scan ork
  `Health` (from `player::combat::Health`, already `pub`), find the nearest
  wounded ally within `SHAMAN_HEAL_RANGE`, restore `SHAMAN_HEAL_AMOUNT` (clamped
  to `max`), reset `heal_cd`, emit a green spark burst at the ally.
- Light cast pose: raise the staff (right) arm while a shaman is in `Attack`, so
  the cast reads visually (`ork_limbs`).

### FX

Extend `CombatFx` (in `player/combat.rs`, already `pub(crate)`) with a green
`heal` material and a `pub(crate)` heal-burst helper; reuse the existing `Spark`
component + `update_sparks` system for both bolt impacts and heal motes. No new
FX system.

## Constants (scaled from the original)

| Name | Value | Origin |
|------|-------|--------|
| `SHAMAN_CAST_RANGE` | `8.0` | rangedRange 12, scaled to Bevy sight 9 |
| `SHAMAN_CAST_CD` | `2.1` | attackCooldown 2.1 |
| `SHAMAN_BOLT_DAMAGE` | `12.0` | bolt 26 > club 24 → keep bolt > Bevy club 8 |
| bolt `speed` | `9.0` | speed 9 |
| bolt `ttl` | `3.0` | ttl 3 |
| bolt hit radius | `0.6` | HIT_RADIUS 0.6 |
| bolt `max_range` | `16.0` | rangedRange + 4 |
| `SHAMAN_HEAL_RANGE` | `8.0` | healRange 8 |
| `SHAMAN_HEAL_AMOUNT` | `20.0` | healAmount 24, vs Bevy max 60 |
| `SHAMAN_HEAL_CD` | `5.0` | healCooldown 5 |

## Data flow

```
ork_brain (shaman in Attack, cd<=0)
  └─> BoltSpawns.push(origin, dmg)
        └─> spawn_queued_bolts → Bolt entity (orb mesh)
              └─> step_bolts homes HeroState.pos
                    └─ on arrival: PendingHeroDamage += dmg
                          └─> health::apply_hero_damage (block mitigates)

shaman_heal (per shaman, heal_cd<=0)
  └─> nearest wounded ork Health within range → hp += amount → green burst
```

## Testing / verification

- Unit: `step_bolts` homes toward a moving target and fizzles past `max_range`
  (pure-ish; factor the homing step so it is testable without a full `World`).
- Manual (screenshot harness / run): spawn near a camp, confirm the shaman holds
  distance and lobs visible purple bolts, the hero takes damage (mitigated when
  blocking), and a wounded grunt regains HP with a green mote.
```
