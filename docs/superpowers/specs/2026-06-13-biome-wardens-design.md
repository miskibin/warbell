# Biome Wardens — per-biome bosses (design)

**Date:** 2026-06-13
**Status:** Approved design, pre-plan
**Feature name (in-game):** *Biome Wardens*

## Summary

Five world bosses, one roaming each of the five biome blobs. Each has a distinct,
polished procedural model (~2.2× ork scale). A boss is **passive until the hero strikes it
first** — the player can scout, forage, and walk past unmolested. The first strike aggros it;
it then chases and fights to the death. Each boss can be **killed only once** and its death
grants a **permanent combat boon** (a new active fighting move *or* a passive that juices the
hero's swing), surfaced through a reward dialog.

Bosses **level up every dawn** (level = nights survived + 1), growing HP and damage and
**healing to full** each morning. Killing one sooner is easier; ignoring one makes it brutal
by the late game. All five are huntable from the start, but even a level-1 boss out-stats a
bare hero — the player wants a weapon and a few upgrades first. This makes bosses **mid-game
content**.

## Locked decisions

| Decision | Choice |
|---|---|
| Where you fight | **World boss** roaming its own biome region |
| Leveling | **+1 level each dawn; grows HP & damage; heals to full each morning.** Wounds do NOT persist between attempts |
| Reward mix | **3 new active moves + 2 passive boons** |
| Engagement | **Roams, passive until the hero lands the first hit** |
| Availability / difficulty | **All 5 open from start; level-1 needs some gear** (mid-game) |
| Boss moveset | **One signature telegraphed attack each**, on top of a big melee |
| Discovery | **No marker — stumble on them.** One-time proximity notice when first near a living boss |
| Reward UI | New `Modal::BossReward` dialog (freezes world, PopIn) |
| Persistence | Boss-kill / boon state persists **within a run only** (matches existing in-window reset model) |

## Roster

| Biome | Boss | Model concept | Signature attack (boss → hero) | Reward (hero gains) | Type |
|---|---|---|---|---|---|
| Rocky | **Stone Golem** | slab torso, boulder fists, mossy cracks | **Stomp** → radial shockwave ring, knockback | **Ground Slam** — leap + slam, radial AoE + knockback | move |
| Desert | **Sand Revenant** | sun-bleached wraith/djinn, tattered wraps, sand-trail | **Sand Burst** — frontal cone, brief slow / vision-obscure | **Sand Dash** — dash forward through enemies, slashing | move |
| Forest | **Treant** ("wood man") | bark torso, branch arms, leaf crown | **Root-Snare** — delayed roots erupt at hero's position, root/slow | **Bramble Sweep** — 360° spin-cleave, hits all around | move |
| Snow | **Bałwan** | three-stack frost giant, coal eyes, icicle crown | **Ice-Shard Volley** — 3 bolts (reuse shaman-bolt path) | **Frostbite** — every hit slows; crit briefly freezes | passive |
| Swamp | **Bog Hag** | hunched troll/hag, moss, rotten teeth | **Poison Cloud** — lingering AoE puddle | **Venom** — hits apply poison DoT + small lifesteal | passive |

## Mechanics detail

### Leveling

- Boss `level` starts at **1** at game start and increments **+1 on each dawn** (day/night
  transition). The leveling system hooks the existing night→day transition.
- On each dawn: `level += 1`, then `hp = max_hp(level)` (full heal at the new, higher tier).
- Scaling (starting points, all tunable):
  - `max_hp(level)   = BASE_HP   * HP_GROWTH^(level-1)`   — `BASE_HP ≈ 1400`, `HP_GROWTH ≈ 1.16`
  - `melee_dmg(level)= BASE_MELEE* DMG_GROWTH^(level-1)`  — `BASE_MELEE ≈ 40`, `DMG_GROWTH ≈ 1.12`
  - signature attack damage ≈ `melee_dmg * 1.4`
- Sanity: by ~night 6 (level ~6) HP ≈ 3000, melee ≈ 70 — a hard fight that demands mid-game
  gear, not impossible. Re-tune after playtest.

### Engagement & AI

- One boss spawned per biome at startup, placed near its biome region center (jittered onto
  walkable ground; respect `town::near_build_plot` / water rejection like other placement).
- **Roam:** slow wander within its biome blob while un-aggroed.
- **Passive until struck:** ignores the hero (no aggro, no attack) until it takes its first
  hit from the hero. First hit flips it to `Hostile` for the rest of its life.
- **Hostile:** chases the hero (reuse A*/`navgrid` like invaders), melee in range, fires its
  signature attack on a cooldown when in mid-range.
- Uses the shared `Dying` fade on death (filter `Without<Dying>` in all targeting/counts).

### Combat integration

- Boss is a fightable entity = `Boss` component + `Health` component (attached like orks via
  the `ensure_combat_health` pattern, but with boss HP from `max_hp(level)`), `Transform` /
  `GlobalTransform`, child mesh parts.
- Hero swing already cone-scans orks + wildlife; extend the scan to include `Boss` so the
  existing damage/crit/FX/floating-number path "just works" on bosses.
- **Boss health bar:** a large, named boss bar (top-of-screen) shown while a boss is Hostile,
  distinct from the small floating enemy bars. Hidden when no boss engaged.

### Signature attacks (boss → hero)

Each on its own cooldown, telegraphed (windup tell before it lands):
- **Golem Stomp** — radial shockwave ring; reuse `spawn_shockwave` FX; knockback + damage in radius.
- **Sand Burst** — frontal cone; brief hero slow / vision-obscure.
- **Treant Root-Snare** — roots erupt at the hero's *current* position after a short delay
  (dodgeable); root/slow on hit.
- **Bałwan Ice-Shard Volley** — 3 projectiles; reuse the shaman-bolt projectile path.
- **Bog Hag Poison Cloud** — lingering ground AoE puddle that damages while stood in.

### Discovery

- No persistent waypoint/marker.
- The first time the hero comes within ~25u of a *living* boss, push a one-time `Notice`
  ("Something massive moves in the <biome>…"). Tracked per-boss so it fires once each.

## Rewards (hero-side)

### Active moves (3) — `src/player/arts.rs`

Each guarded on an unlock flag; dedicated key + cooldown. Reuse the existing combat cone,
crit roll, FX helpers (`spawn_blood`/`spawn_slash`/`spawn_burst`/`spawn_shockwave`),
`HitFeedback`, `FloatQueue`, audio cues.

- **Ground Slam** (from Golem) — keypress → hero leaps and slams; radial AoE damage +
  knockback around the landing point. CD ≈ 6s.
- **Sand Dash** (from Revenant) — keypress → hero dashes forward ~4u, passing through and
  damaging enemies along the line, slash at the end. CD ≈ 4s.
- **Bramble Sweep** (from Treant) — keypress → 360° spin-cleave hitting all enemies in radius.
  CD ≈ 5s.

**Key bindings:** free keys are scarce (see Controls in CLAUDE.md). Resolve at implementation —
likely three dedicated keys (candidates G / V / T) or a small "weapon-arts" set. A move is
only usable once its boss is slain; show the binding in the reward dialog and/or a small arts
HUD. This is a tuning detail, not an architectural one.

### Passives (2) — extend the hero swing in `combat.rs`

- **Frostbite** (from Bałwan) — every hero hit applies a `Slowed` component to the struck
  enemy (move/attack-speed −X% for ~2s); a crit applies a short (~1s) freeze (stun).
- **Venom** (from Bog Hag) — every hero hit applies/stacks a `Poisoned` component (DoT over
  ~4s); the hero heals a small fraction of poison damage dealt (lifesteal flavor).

New `Slowed` / `Poisoned` components + tick systems (filter `Without<Dying>`; use
`try_insert`/`try_despawn` per the despawn-race convention). These also apply to bosses.

### Reward moment

1. Boss death fires a `BossDefeated { biome, boon_id, boon_name, boon_desc }` message.
2. A system grants the boon (sets the `Player` flag / unlock) and sets `Modal::BossReward`.
3. `OnEnter(Modal::BossReward)` spawns a PopIn dialog (reuse `widgets` + `anim` kit):
   "⚔ WARDEN SLAIN" → boon name + description (+ key binding if a move) → **Continue** button.
4. Continue / Esc → `Modal::None`, world unfreezes.

The world freezes for the read (the Modal substate gates all sim systems on
`in_state(Modal::None)`), honoring the moment.

## Architecture

New module `src/boss/`:

- **`boss/mod.rs`** — `BossPlugin`; `Boss` component (biome, level, roam/aggro state,
  signature cooldown, pos/facing); startup spawn (one per biome region); roam + aggro-on-hit
  brain; per-dawn leveling system (hooks day/night transition); signature-attack systems;
  death → `BossDefeated` message + grant boon + open `Modal::BossReward`; proximity-notice
  system; boss health-bar system.
- **`boss/models.rs`** — five procedural boss models following the `critters.rs` mesh
  contract: build parts from primitives, `tinted()` (bake linear RGBA into `ATTRIBUTE_COLOR`),
  `merge`, `duplicate_vertices()` + `compute_flat_normals()`, `surf()` codes (bark / stone /
  ice / cloth / bone). Larger scale, distinct silhouettes. One shared white material for
  batching, as with other creatures.
- **`boss/reward_ui.rs`** — the `Modal::BossReward` dialog (spawn/despawn/continue), modeled
  on the existing confirm/pause modal pattern.

Touch points:

- **`src/player/arts.rs`** (new) — the three active moves + their inputs / cooldowns,
  registered in `PlayerPlugin`. Guarded on `Player` unlock flags.
- **`src/player/combat.rs`** — include `Boss` in the hero cone-scan; apply Frostbite/Venom
  on-hit when the flags are set; add `Slowed`/`Poisoned` components + tick systems.
- **`crates/core` `Player`** — add boon flags: `has_ground_slam`, `has_sand_dash`,
  `has_bramble_sweep`, `frostbite`, `venom`. Granted directly on boss kill (does NOT route
  through the upgrade store). Keeps boon state in the parity-tested player struct.
- **`src/game_state.rs`** — add `Modal::BossReward` variant (+ its `OnEnter`/`OnExit` wiring).
- **`src/main.rs`** — register `BossPlugin` in a plugin tuple.

### Reuse (keeps scope sane)

`Dying` fade, `Health` + damage path, crit roll & combat FX helpers, shaman-bolt projectile
(ranged signatures), `spawn_shockwave` (slam + golem stomp), `navgrid` A* (chase), the Modal
substate + UI `widgets`/`anim`/`Notice` kit, `critters.rs` mesh contract (models).

### Net-new (the heavy bits)

5 procedural boss models · 3 hero active moves · 2 passive status systems (`Slowed`/`Poisoned`)
· 5 boss signature attacks · the leveling + roam/aggro brain · the reward dialog · the boss
health bar.

## Conventions to honor

- `try_despawn` / `try_insert` on any combat/AI/HP-bar-touched entity (despawn races).
- All targeting / counting queries filter `Without<Dying>`.
- New sim systems carry `.run_if(in_state(Modal::None))` so they freeze during panels/pauses.
- Combat numbers wired from core are used **as-is** (no rescale).
- Mesh contract: base at `y=0`, color in `ATTRIBUTE_COLOR`, `duplicate_vertices()` **before**
  `compute_flat_normals()`.
- Every voice/quote line (if bosses get barks) carries its spoken transcript in a code comment.
- Determinism: placement via `mulberry32` seeded per-tile/biome.

## Out of scope (YAGNI)

- Boss-kill persistence across separate runs (matches existing reset model — within-run only).
- A War Table "Wardens" roster panel (discovery is stumble-only; could add later).
- Multi-phase boss fights / full unique movesets (one signature each is the agreed depth).
- Biome state changes on kill (e.g. "cleansed" biome).
- Key rebinding system for the new moves.
