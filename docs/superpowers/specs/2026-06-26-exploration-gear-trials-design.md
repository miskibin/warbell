# Design — Exploration, Earned Gear & Pacing

**Date:** 2026-06-26
**Status:** approved-in-dialogue, pending written review
**Author:** brainstormed with miskibin

## Problem

Late-game the run is **short and shallow**:

- The spine is a tiny combat checklist (5 wardens → breach Gnashfang Hold → Warlord). You can be
  Warlord-ready by ~night 3.
- **No reason to explore** except finding wardens. Biome loot is finite/deterministic; once looted
  a biome is dead.
- **Gear arrives opaquely** — best gear was random rolls (`frontier::roll_gear`) from chests,
  animal drops, and *walking into a landmark's smoke*. The landmark grab in particular is
  unsatisfying: proximity → loot in bag, no act, no reason.
- **Town growth is trivial/automatic** (~20 s per villager with one farm), so clearing ork camps in
  daylight has no point — population just appears on its own.

A separate, already-implemented **balance batch** (hero level-curve soft-cap, decoupled harvest
damage, escalating house cost) curbs raw power. *This* spec addresses the **content/pacing** layer:
give the map earned, legible reasons to explore.

## Goals

1. **Gear is earned, deep in a biome, and CLEAR to obtain** — no random rolls, no proximity-grab.
2. **Exploration has a per-biome reward loop** that interleaves with the warden hunt.
3. **Daytime sorties matter** — clearing ork camps becomes the main way the town grows.

Non-goals (this pass): touching the shop/Arsenal upgrade path; reworking wardens; the optional
hard "clear all camps → Warlord" gate (deferred — see Open Questions).

## Design

Three coordinated changes. Each biome ends up with **three earned reasons to go there**: the
**warden** (an *art/boon*, existing), the **landmark trial** (a *gear* piece, new), and the **ork
camp** (a *batch of villagers*, retuned).

### 1. Gear = a "Hold the Rune" trial at each landmark

The five biome landmarks already exist as discoverable POIs with a **beacon** (a will-o'-wisp
column visible from afar — the "where" is already solved) and a repeatable **shrine** buff
(`src/landmarks.rs`). We layer a one-time skill trial that gates a **named, biome-signature** gear
piece. The trial reuses the game's own verbs — holding ground against a horde — so it reads as a
*mini-siege*, on-theme for a castle-defender.

**Landmark → gear mapping** (top-tier, biome-flavoured; these pieces are findable ONLY here):

| Biome  | Landmark           | Gear (item id)         | Stat            |
|--------|--------------------|------------------------|-----------------|
| Snow   | The Frozen Spire   | `blade_frost` Frostfang| +34 damage ❄    |
| Swamp  | The Mire Sentinel  | `dragon_plate`         | 42% armor 🐉    |
| Desert | The Sunken Pyramid | `gold_armor` Gilded Plate | 28% armor    |
| Forest | The Hollow Oak     | `sword_gold` Golden Blade | +21 damage   |
| Rocky  | The Standing Stones| `stone_maul`           | +18 damage      |

**Landmark lifecycle (F is context-sensitive):**

1. **Undiscovered** → walk within `DISCOVER_R` → *discovered*: announce + lore + small gold +
   beacon snuffed (as today) **but NO gear/relic granted**. The cache is "sealed".
2. **Discovered, gear unclaimed** → press **F** within `SHRINE_R` → **start the Hold-the-Rune
   trial** (instead of praying).
3. **Gear claimed** → press **F** → pray at the shrine (the existing repeatable buff).

**The trial (Hold the Rune):**

- A rune circle of radius `RUNE_R = 5.0` lights at the landmark; a **hold meter** fills 0→1 over
  `HOLD_SECS = 35` while the hero stands inside it, and **drains at `DRAIN_MULT = 1.5×`** while he
  steps outside (so you must defend the spot, not kite forever).
- Enemies spawn from a ring around the landmark — **reuse the siege spawn-ring + invader AI**
  (`siege.rs`), flavoured as awakened guardians. Steady trickle: `SPAWN_INTERVAL ≈ 2.0 s`,
  `MAX_ALIVE ≈ 7`, scaled by gear tier (the Frostfang/Dragonscale trials are hardest).
- **Win:** meter reaches 1.0 → remaining guardians despawn, the named gear is granted
  (`try_grant` + a named reward float + voice line), `gear_claimed = true`; F now prays.
- **Fail / abort** (no permanent loss, retry anytime): hero downed, OR hero leaves
  `ABORT_R = 18.0` of the landmark, OR night falls (a real siege starts). On abort: despawn the
  trial's guardians, reset the meter.
- **When startable:** only in `Modal::None` during **Prep/day** — not during an actual night Wave
  (don't stack two hordes). Trial guardians are tagged so they're swept on abort/win and never
  counted by the siege director.

**HUD:** while a trial is active, show a hold-progress bar + "Hold the Spire!" prompt (reuse the UI
kit; the meter can be a simple top-or-overhead bar). Out of scope to make it fancy — legibility
first.

**Tunables (locked as starting values, expect playtest tuning):** `RUNE_R 5.0`, `HOLD_SECS 35`,
`DRAIN_MULT 1.5`, `SPAWN_INTERVAL 2.0`, `MAX_ALIVE 7`, `ABORT_R 18.0`.

### 2. Remove the random/opaque gear sources

So the landmark trials are the *only* path to top-tier wearables:

- **Landmark discovery** — drop the `frontier::roll_gear` relic (keep the small gold + lore).
- **Chests** (`crates/core/src/chests.rs` / frontier-rolled) — no wearable gear; keep consumables,
  materials, gold. (Hand-authored castle-adjacent starter chests may keep their fixed low-tier
  pieces — they're the tutorial ramp, not "random".)
- **Animal drops** (`src/verbs.rs` frontier gear bonus + golem `stone_maul`/`iron_armor`) — drop the
  wearable rolls; keep meat/hides/materials.

The shop/Arsenal upgrade path (buy `axe`, `sword_gold` for gold) is **left untouched** — it is an
explicit, chosen, gold-gated progression, not an opaque drop. (Minor redundancy: `sword_gold` is
both a Forest-landmark reward and Arsenal-buyable; acceptable — it's mid-tier and the buy is a
deliberate gold sink.)

### 3. Slow organic town growth; make camps the population engine

- **Slow the settle flow** so villagers don't just appear: raise `SETTLE_FOOD 20 → 45`
  (`crates/core/src/town_store.rs`). One farm now settles a peasant in ~45–90 s instead of ~20 s.
- **Camps free a cage of captives:** camp rescue grants **+3 population** instead of +1
  (`src/villagers.rs::camp_rescue`; the code already has `cage_positions()` for multiple cages).
  Five camps × +3 = +15 — the main army source.
- Net effect: a daytime loop — sortie into a biome, clear the camp warband, free a batch of
  villagers → bigger army/economy. This also slows the gold-tithe snowball (fewer peasants),
  reinforcing the balance batch.

## Persistence & reset (the load-bearing invariant)

Per `CLAUDE.md`, anything earned across a run must **persist** AND **reset**:

- **New run-state:** per-landmark `gear_claimed` (5 flags) and (optional) the live trial state.
  - Landmarks already serialize "discovered" via the `GameLoaded` reconcile in the landmark module;
    add `gear_claimed` alongside it. The *active* trial state is transient — **not** saved (like the
    battlefield); a save can only be written in Prep, and a trial aborts when night falls, so no
    trial is ever mid-flight at save time.
  - Add the claimed-set to `SaveData` (`#[serde(default)]`, additive — no `SAVE_VERSION` bump),
    write it in `SaveCtx::snapshot()`, restore in `apply_pending_load`, reconcile the per-landmark
    `gear_claimed` from the carried `SaveData` in the landmark module's `GameLoaded` handler.
- **Reset:** New Game already rebuilds the world (beacons/landmarks respawn fresh with
  `discovered=false`); ensure `gear_claimed` resets to false there too. `SETTLE_FOOD` / camp +3 are
  constants (no per-run state). Camp `population += 3` already round-trips via `Town`.

## Files touched

- `crates/core/src/town_store.rs` — `SETTLE_FOOD` 20→45.
- `src/villagers.rs` — `camp_rescue` +1 → +3.
- `src/landmarks.rs` — gear mapping per biome; sealed-cache discovery (drop `roll_gear`); the
  Hold-the-Rune trial state machine + spawn/meter/win-lose systems; F context-switch; save reconcile
  of `gear_claimed`.
- `src/siege.rs` — expose the spawn-ring helper for trial reuse (or a thin shared spawner).
- `src/savegame.rs` — `SaveData` claimed-set + snapshot/restore.
- `src/verbs.rs`, `crates/core/src/chests.rs`, `crates/core/src/animal.rs` — strip wearable gear
  from chest/animal rolls.
- A small HUD element for the hold meter (`src/ui/` or within `landmarks.rs`).

## Phasing (implement in this order; each verifiable on its own)

1. **Economy retune** (small, low risk): `SETTLE_FOOD` 45 + camp +3. Core test + a quick run.
2. **Gear relocation** (no new systems): assign named gear to landmarks granted *on discovery*
   (temporary — before the trial exists), strip random gear from chests/animals/landmark-roll.
   Verifies the gear ladder end-to-end.
3. **Hold-the-Rune trial** (the big piece): convert the discovery-grant into the F-triggered trial;
   add the state machine, spawner, meter, HUD, win/lose, save flag.

## Open questions (deferred, not blocking)

- **Hard map-gate on the Warlord** (Hold un-breachable until all 5 camps cleared) — the user raised
  it; deferred to a follow-up so this pass stays scoped.
- Trial enemy flavour: reuse ork invader meshes vs. bespoke "guardian" models — start with reused
  ork variants; reskin later if it reads wrong.
- Per-biome trial difficulty curve (which landmark is hardest) — start flat-ish, tune in playtest.
