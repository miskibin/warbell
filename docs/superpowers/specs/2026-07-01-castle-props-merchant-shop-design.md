# Design — Mature castle props + a real merchant shop

**Date:** 2026-07-01
**Status:** approved (brainstorm)

## Problem

The props in the castle bailey that come from `villagers.rs` — the **market/shop stall**, the
**well**, and the **woodpile** gathering spots — look crude and flat next to the rest of the
courtyard. Root cause: they are built as one **merged vertex-coloured mesh on a plain white
`StandardMaterial`** (no procedural wood-grain / stone texture) with minimal geometry. The
"mature" courtyard set dressing in `castle_decor.rs` looks good because it uses the shared,
procedurally-**textured** `Mats`/`M` pipeline (plank `Wood`, `Beam`, `Stone`, shingle roofs…) plus
flat-normal facets.

On top of that, the **shop** itself is a tiny open stall (a 1.8-wide counter, four thin posts, a
short awning, two crates). It does not read as a shop — a player can walk past the merchant
interaction without noticing there is a shop there.

The user's reference screenshot is the **woodpile** gather-spot (stacked flat-brown boxes with a
villager clustered at it) — representative of the whole crude family.

## Goals

1. Rebuild the shop stall, well, and woodpile on the **textured `Mats` pipeline** so they match the
   maturity of the `castle_decor.rs` props.
2. Redo the shop as a **bigger, unmistakable roofed merchant shopfront** with a hanging shop sign,
   goods on display, and a **stationary merchant NPC** behind the counter.
3. No regressions to the shop **interaction**, collision, gather-spots, save/load, or `cargo test`.

Out of scope: the courtyard `market_parts` goods pile is **removed** (consolidated into the new
shop), not restyled. No changes to `crates/core`, the shop *panel* UI (`economy.rs`), or shop
economics.

## Approach

### Wiring — hand the textured `Mats` to the village props

`villagers::populate` currently receives only `commands, meshes, std_mats, creature_mats` and makes
its own white material. The textured material set is already available at build time as
`BuildState.village_mats` (set at worldmap phase 13, `castle::build`), and phase 19
(`town::populate_plots`) already threads it in. Do the same for phase 15:

- `worldmap::build_step` step 15: pass `state.village_mats.as_ref().expect(...)` into `populate`
  (castle phase 13 runs before villager phase 15, so it is always `Some`).
- `villagers::populate` gains a `mats: &crate::castle::Mats` parameter.
- The shop / well / woodpile props spawn **one entity per `M` slot** (like `castle_decor::build`),
  using `crate::castle::{bake, bx, gable, cyl, taper, log_x, flat, M}`. The old merged-white-mesh
  helpers (`market_stall_mesh`, `well_mesh`, `woodpile_mesh`, and the local `group/bx/tinted/
  tilt_slat` used only by them) are removed if nothing else uses them (the villager *bodies* keep
  their own creature-material pipeline — untouched).

The villager `mat` (white) currently also dresses nothing else critical; verify no other prop
relies on it before deleting. (`well`/`woodpile`/`stall` are its only users per grep — confirm at
implementation time.)

### Shop → roofed merchant shopfront

Keep the anchor **unchanged**: `south_gate + (2.5, -5.0)` (where `interaction::shop_anchor()` and
the gather-spot both point). Ground-snap `y` as today.

Form: **open-front booth with a low back wall + goods shelf** (merchant visible from the front,
still reads as a built structure). Parts (textured `M` slots):

- Plank **counter** (`Wood`) ~3.2 wide × 2.0 deep with a `Beam` top lip; the open browsing side
  faces the player's approach (toward the gate / −Z-ish, matching the current stall facing).
- Four **thick corner posts** (`Beam`), taller than today.
- **Shingle gable roof** (`HouseRoof`) spanning the booth — the primary "this is a shop building"
  read.
- **Striped awning** slats along the open front under the eave (`Banner` red / `Plaster` cream),
  a richer version of today's `tilt_slat`.
- **Low back wall** (`Plaster` with `Beam` framing) behind the merchant, carrying a **shelf**
  (`Wood`) of small goods (jars/bolts).
- **Hanging shop sign** on a front bracket arm: a `Wood` board + a `Gold` coin/goods glyph — the
  unmistakable shop signal. Plus a small `Banner` pennant on a post.
- **Goods on/around the counter**: `Wood` crates, a hooped **barrel** (`taper` + `Beam` hoops),
  `Plaster` grain sacks, a `Gold` coin-scale / coin stack, a `Banner` bolt of cloth — reusing the
  motifs already in `castle_decor::market_parts` / `guild_goods_parts`.
- **Collision**: resize the existing `blockers::add_box` at the anchor to the new counter footprint
  (roughly half-extents matching the ~3.2 × 2.0 counter, minus the open front so the hero can step
  up to browse). Posts/roof overhang do not need separate boxes.

### Merchant NPC

One **stationary** biped standing **behind the counter**, facing the open browsing side:

- Spawn via the existing `villagers::spawn` (as the pilgrims/`FOREST_VILLINE` line do) with
  `speed 0`, `wander_r 0`, a merchant-ish `Kind::Peasant { tunic: <apron/dyed>, hat: true }`.
- Insert `SceneActor` + a fixed `Transform` (facing the approach) so the wander/gather brain leaves
  it planted (same pattern as the `FOREST_VILLINE` stand-still actors).
- **Not** added to `town.population` (ambient flavour, like the pilgrims and kids) → **not saved**,
  matching the deliberately-unsaved ambient NPC set. No blocker (the counter blocks; the merchant
  stands in the sheltered back).
- Night curfew: it may hide with the other ambient NPCs at night (acceptable / consistent with
  gate-folk & traders) — confirm it is caught by the same ambient-curfew filter or is intentionally
  exempt; either is fine, no siege interaction required.

### Well + woodpile maturity

Rebuild both in the textured `Mats` idiom (mirror `castle_decor`'s already-textured
`woodpile_parts`):

- **Well**: `Stone`/`HouseStone` curb, `Slit` (dark water) surface, `Beam` posts + crossbar, an
  optional small shingle cap, a `Wood` bucket with an `Iron` band + a rope line. More facets/detail
  than the current 6-box mesh. Blocker + gather-spot position unchanged.
- **Woodpile**: textured `Wood`/`Beam` `log_x` stack (alternating hues) between two `Beam` end
  stakes, plus a **chopping block** with an `Iron` axe head sunk in it and a few bark chips. Blocker
  + gather-spot position unchanged.

### Remove the old courtyard market pile

Delete the `set(market_parts(), (5.3, 0.0, -2.9), …, DecorGate::Always, …)` call in
`castle_decor::build` and its `market_parts()` fn (now unused). Its collision box is only ever
registered when the piece is *shown*, so removing the spawn removes the box cleanly — no
`blockers` bookkeeping needed. Confirm no other code references `market_parts`.

## Data / state impact

- **Save/load:** none. All three props are static `BiomeEntity` world geometry rebuilt every world
  build; the merchant is ambient (unsaved, like pilgrims). No `SaveData` / `SaveCtx` / reset work.
- **Core (`tileworld_core`):** untouched — `cargo test` (~268) must stay green with no core edits.
- **Interaction:** `interaction::shop_anchor()` and `SHOP_DIST` unchanged; the `E` "Shop" prompt and
  the `Modal::Shop` panel are untouched — only the world geometry at the anchor changes.

## Testing / verification

- `cargo check` + `cargo test` (core parity spec) green.
- Visual (screenshot harness, per CLAUDE.md — confirm the `Screenshot saved` log line, retry on a
  black/god-cam frame):
  - Shop: `FOREST_SHOT` + `FOREST_TPS=1` + `FOREST_HERO="<shop anchor xz>"` → confirm roof + hanging
    sign + merchant + goods read clearly as a shop; a low-oblique `FOREST_CAM` for a wider frame.
  - Well + woodpile: low-oblique `FOREST_CAM` near each gather spot to confirm the textured
    upgrade.
- Manual: walk to the shop, confirm the `E` "Shop" prompt still fires at range and opens the panel;
  confirm the hero routes around the counter (blocker) and cannot walk through the merchant.

## Risks / notes

- Multi-material spawn means more entities than one merged mesh, but they auto-batch on the shared
  `Mats` (same as every `castle_decor` piece) — no perf concern.
- Keep the shop **facing** and **anchor** aligned with the interaction range or the `E` prompt could
  land behind the counter; verify facing in the screenshot pass.
- The awning/roof must not clip the hanging sign or the merchant's head — tune heights in the
  screenshot loop.
