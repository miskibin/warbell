# Stone Miner building — design

**Date:** 2026-06-10
**Status:** approved, ready for implementation plan

## Summary

Add a third producer building, **Stone Miner** (`BuildKind::Mine`). Its worker roams out
to a real ore boulder in the Rocky biome, picks it apart, loads a stone **cart**, hauls the
cart home, and banks the stone on arrival. It is a near-exact mirror of the existing
**Woodcutter** (`BuildKind::Lumber` / `src/lumberjack.rs`), with two deliberate divergences:

1. **Ranges far** — no 45u safe-zone cap; the ore it works lives in the Rocky biome blob (east),
   well outside the woodcutter's `WORK_R`.
2. **Carries a cart** — a small loaded stone wagon shown only on the trip home, in place of the
   woodcutter's shouldered log.

To stop NPC mining from permanently stripping the map, **ore boulders now regrow** (slowly) and
there are **more** of them.

## Locked decisions

- **Stone source:** real Rocky-biome `OreNode` boulders (not new near-castle rocks), with more
  boulders + a slow respawn so a working miner doesn't deplete the map.
- **Cart:** visual-only loaded cart mesh while hauling (mirror of the woodcutter's carried log) —
  *not* a persistent parked vehicle the miner shuttles to.
- **Range/safety:** range far (drop the `WORK_R` cap) but keep the woodcutter's flee rules —
  threat-sense → run home + blacklist the spot, avoid ork camps.
- **Build cost:** wood only (~6), the mirror-image of the woodcutter's stone-only cost. Clean
  economic loop: wood → stone → wood.

## Accepted trade-off

The Rocky blob is well past the 45u safe ring, so the miner's round trip is long. During a night
wave — or past wandering predators / camp orks — it will sometimes flee and lose a load. Stone
income from the miner is therefore lumpier and slower than wood from the woodcutter. This is the
intended feel of "real boulders, range far, same flee rules," not a bug to design out.

## Components & changes

### 1. Core — `crates/core/src/town_store.rs` (test-gated)

Add `BuildKind::Mine` and a new arm to each of the four exhaustive `match self` methods:

| method        | `Mine` arm                                   | rationale                                            |
|---------------|----------------------------------------------|------------------------------------------------------|
| `cost()`      | `Cost { wood: 6.0, stone: 0.0 }`             | wood-only; mirror of Lumber's stone-only             |
| `produces()`  | `None`                                        | yield earned in the world, banked on haul (like Lumber) |
| `max_hp()`    | `55.0`                                        | same as Lumber                                       |
| `label()`     | `"Stone Miner"`                               | menu/HUD label                                       |

`needs_worker()` already returns `true` for all kinds — no change.

**Test:** `mine_costs_wood_only_and_has_no_passive_stone` — mirror of
`woodcutter_costs_stone_only_and_has_no_passive_wood`: building deducts 6 wood / 0 stone, and a
staffed Mine banks no passive stone over a `production_tick`.

**Save compatibility:** `BuildKind` already derives serde `Serialize`/`Deserialize`, so a saved
`Mine` plot round-trips automatically — no `savegame.rs` change. (Optionally extend
`crates/core/tests/serde_roundtrip.rs` to build a `Mine` plot for coverage.)

### 2. Ore becomes renewable — `src/verbs.rs`

Today `mine_ore` calls `try_despawn` on shatter, so a boulder is gone forever. NPC mining would
permanently strip the 18 boulders. Make ore regrow, mirroring the tree `Stump` → `regrow_trees`
pattern:

- `OreNode` gains `blocker_r: f32`, stored at spawn (`= (0.55 * scale).min(0.95)`), so a regrown
  node can re-register its blocker.
- New `DepletedOre { regrow_at: f32 }` component (mirror of `Stump`).
- New shared helper `deplete_ore(commands, e, now)`: hide the node (`Visibility::Hidden`), insert
  `DepletedOre { regrow_at: now + ORE_REGROW }`, lift its blocker (`blockers::remove_at`).
  - `mine_ore` (hero path) calls `deplete_ore` **instead of** `try_despawn`. Banking, the float,
    chip FX, audio cues, and the `FirstStone` voice line stay on the hero path unchanged.
- New `regrow_ore` system (gated `Modal::None`): for each `DepletedOre` past `regrow_at`, reset
  `ore.hp = ore.max_hp`, show, re-add the blocker at `tf.translation` with `blocker_r`, insert
  `RegrowPop`, remove `DepletedOre`.
- `const ORE_REGROW: f32 = 360.0;` — slow (~6 min).
- `ORE_COUNT: 18 → 28` — more boulders.

Both the hero's `mine_ore` and the miner's `pick_work` share `deplete_ore`, so regrow + blocker
bookkeeping is identical regardless of who lands the last blow. Targeting on both paths filters
`ore.hp > 0` / `Without<DepletedOre>`, so a boulder finished by one actor is dropped by the other
(mirror of the woodcutter's "tree felled by someone else" handling).

### 3. New module — `src/miner.rs` (mirror of `src/lumberjack.rs`)

Components:
- `MineJob { ore: Entity, atk_cd: f32, stall: f32 }` — walk to the boulder, pick on cooldown.
- `Carting { amount: f64, cart: Option<Entity>, stall: f32 }` — mirror of `Hauling`; carry stone
  home, bank on arrival.

Reuse from `lumberjack.rs` (bump these to `pub(crate)`):
- `Fleeing` component + `flee_steer` system (already runs any villager with `Fleeing` + `Worker`).
- `DangerSpots` resource (shared blacklist of remembered scares).

Systems (all gated `run_if(in_state(Modal::None))`):
- `mine_danger` — a hostile inside `DANGER_R` of a `MineJob`/`Carting` worker → `Fleeing` + push
  the spot to `DangerSpots`. (Mirror of `lumber_danger`.)
- `assign_ore` — hand each idle **Mine**-plot worker the nearest boulder that is: alive
  (`ore.hp > 0`, `Without<DepletedOre>`), not in/near an ork camp (`CAMP_AVOID`, `in_clearing`),
  and not blacklisted. **No `WORK_R` cap.** Throttled like `assign_tree`. Skips workers whose plot
  kind isn't `Some(BuildKind::Mine)`.
- `pick_work` — A*-when-far / direct-steer-when-near march to the boulder; swing the pick on the
  cooldown. On a non-final blow: chip FX + `TrunkShake` + `OreChip` cue (reuse existing). On the
  final blow: call `deplete_ore`, then swap `MineJob` → `Carting { amount: ore.stone_reward }`.
  Stall handling + blacklist-on-wedge as in `chop_work`.
- `cart_home` — march to the worker's plot spot; on arrival bank `amount` stone (`bank.0.add_stone`)
  + a "+N stone" float, despawn the cart mesh, drop `Carting`. (Mirror of `haul_home`.)
- `attach_cart` — on a fresh `Carting`, spawn + cache the cart child mesh (see §4).
- `shed_cart_at_muster` — a `Carting` worker mustered to `Guard` at dusk banks the stone on the
  spot and despawns the cart (mirror of `shed_log_at_muster`), so a load isn't stranded on a
  soldier's back all night.

Tuning constants:
- `PICK_DMG: f64 = 50.0` — with `ORE_HP = 354`, ~7 swings ≈ 15s per boulder.
- `PICK_CD: f32 = 2.1` — matches the overhead-swing work loop.
- `PICK_REACH: f32 = 2.0` (+ `ORE_COLLISION_RADIUS` on the distance check).
- `DANGER_R`, `DANGER_BLACKLIST_R`, `DANGER_TTL`, `CAMP_AVOID`, `STALL_SECS`, `RETRY_SECS`,
  `SFX_EARSHOT`, `HAUL_REACH` — same values as `lumberjack.rs`.

`MinerPlugin` registered in `src/main.rs` immediately after `LumberjackPlugin`.

### 4. Cart mesh — `attach_cart`

A small wagon, built once and cached (mirror of `attach_log`): a wood box bed + two dark wheels +
a heap of grey stone on top. Spawned as a child of the miner trailing behind at local
≈ `(0.0, 0.0, -0.7)` (wheels at ground; the villager root rides at `gy + bob`). Shown only while
`Carting`; despawned on delivery. Low-poly clipping on slopes is acceptable (logs clip too).

### 5. Building mesh — `src/town_meshes.rs`

`mine_parts()` = the shared `cottage()` (−X side) + a new `stone_yard(cx)` (+X side), mirror of
`woodcutter_parts` / `log_yard`:
- a stack of cut stone blocks (`M::Stone` / `M::DarkStone` / `M::LightStone`),
- a parked empty handcart frame (timber + wheels),
- a leaning pick (a `M::Beam` haft + a small `M::Stone`/`M::Bronze` head).

Update the module doc-comment's building list to include the Stone Miner.

### 6. Worker look + animation — `src/villagers.rs`

- `Trade::Miner` added to the `Trade` enum.
- `Held::Pick` added to `Held`, with a pickaxe mesh in the held-tool builder (mirror of the axe ~L1248).
- Miner outfit: a vest torso piece (mirror of the woodcutter ~L1227).
- `Kind::Worker { trade: Trade::Miner, .. } => Held::Pick` (~L1212).
- Work animation: extend the overhead-chop stroke to the miner —
  `let chopping = matches!(role, Some(Role::Working(Trade::Woodcutter | Trade::Miner)));`
- Plot-kind → role map (~L1575): `Some(BuildKind::Mine) => Role::Working(Trade::Miner)`.

`Role::Working(Trade::Miner)` is a new `Role` value; `reskin_townsfolk` rebuilds the body on role
change automatically (derived `PartialEq`/`Eq`).

### 7. Build menu — `src/town.rs`

- Add `BuildItem::Producer(BuildKind::Mine)` to the `MENU` array.
- `building_parts`: `BuildKind::Mine => crate::town_meshes::mine_parts()`.
- `BuildItem::desc()` arm: `"Stone Miner \u{2192} mines real boulders and carts the stone home (needs a worker)"`.
- `auto_assign_workers` farm-short release: also `try_remove::<crate::miner::MineJob>()` alongside
  the existing `try_remove::<crate::lumberjack::ChopJob>()`, so a non-farm worker freed for the
  fields drops a mining job too.

## Conventions to honour (from CLAUDE.md)

- `try_despawn` / `try_insert` / `try_remove` everywhere combat/AI/HP systems might race the entity.
- Every new voice/SFX cue keeps its spoken-text comment (no new voice lines planned; reuse
  `OreChip` for picks).
- Mesh parts share the white `StandardMaterial` via the `M` palette; build with `bx`/`bake`.
- New sim systems carry `.run_if(in_state(Modal::None))`.
- Stage explicit paths on commit (never `git add -A`); single-session build + verify is expected.

## Verification

- `cargo test -p tileworld_core` — new `mine_*` test passes alongside the existing town suite.
- `cargo run` — build a Stone Miner on an empty plot (wood only), confirm a peasant walks to the
  Rocky biome, picks a boulder, returns with a visible stone cart, and stone is banked on arrival;
  confirm boulders regrow after ~6 min and the hero's own mining still works.
- Optional screenshot (`FOREST_TOWN=...` / a staged Carting miner) for the cart + building look.

## Out of scope

- No upgrade-tree node for the miner (uses the existing build menu only).
- No new ore variants, gem hues, or audio lines.
- No change to the hero's mining feel beyond boulders now regrowing.
