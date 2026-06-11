# Castle courtyard redesign — the most beautiful place on the map

**Date:** 2026-06-11 · **Status:** implemented

## Problem

The castle was the most visually flat place on the island: one tiled cobble/packed sheet
wall-to-wall (read as a repeating carpet), one shared cottage model for all 12 dwellings AND
every producer building, and almost no set dressing. Worse, all the rustic clutter (well,
cart, hay, hens, grass) was tagged `PreWalls` — buying Palisade Walls **deleted every sign of
life** and left bare paving. The first thing the player sees told no story.

## Decisions (user-locked)

- Ground: **paths + plaza**, not a full sheet (and not bare grass). Story: village bracing
  for siege + working stronghold. Houses: **archetypes + variation**.
- Courtyard must NOT be blank on day one — it's the player's first impression.
- **Upgrade-tree purchases plant real set pieces** in the castle (e.g. NPC armor → an actual
  armory), so a run's purchases can be read off the courtyard.

## Design

### Ground (`castle.rs`)

`PATH_RECTS`: a keep/bell/muster plaza + four gate paths; lawn everywhere between. Day one
the routes are trodden **packed earth** (`PreWalls`); buying Walls re-paves the same network
in **cobble** (`Walls`) — paving as progression. `worn_slab` now cuts the ragged rim into the
**geometry** (cells below the noise threshold aren't emitted, fully opaque). Alpha-Blend rims
rendered as a washed-out film at grazing angles; alpha-Mask fought the depth prepass (its
cutoff ignores vertex alpha → sky-coloured holes). Geometry can't lie.

### Permanent life (`castle.rs` + `castle_decor.rs`)

Corner clutter (wood yard, hay, cart, well), hens and lawn tufts re-tagged `Always`. New
day-one dressing in `castle_decor.rs`: notice board, water trough, market goods, two benches,
five lantern posts (lamp boxes share the window material → they glow at dusk for free).
House-gated dressing fills in as the town grows: kitchen gardens, woodpiles, laundry lines.

### Upgrade → set piece (`castle_decor.rs`)

`DecorGate` keys on live flags (`Defenses` / `EconomyState` / `Upgrades` / town houses), so
saves and `FOREST_DEFEND` staging both show the right dressing:

| Upgrade | Set piece |
|---|---|
| Town Guard Arms (tier 1) | Armory corner: spear rack, shields, leather stand |
| Veteran Guard (tier 2) | Steel extension: sword rail, iron stand |
| Unlock Battle Axe / Golden Blade | Display stands by the armory |
| Sharpened Blade | Grindstone at the muster yard |
| Tax Office | Counting booth (strongbox, coins, ledger, coin sign) |
| Bounty | Bounty board inside the north gate |
| Merchant Guild | Banner + goods at the merchant stall |
| Healing Shrine | A real shrine (candles + flicker light) |
| Reinforced Keep | Mason's scaffold + dressed stone at the keep wall |
| Tower Mastery | Standing fire baskets at the four wall corners (+ flicker lights) |

### House archetypes (`castle.rs`)

Four silhouettes over the 12 slots — hut (thatch, no chimney), cottage, two-story jettied
townhouse, longhouse — plus warm/weathered shingle alternation (`HouseRoof`/`HouseRoof2`).
Per-archetype accessors feed the blocker (`house_dims`), curfew shutters (`house_window`,
threaded through `shutters::spawn_house_shutters`) and chimney smoke (`house_chimney`).

### Producers (`town_meshes.rs`)

No more shared cottage. Farm = thatched barn + field + scarecrow; Woodcutter = open saw shed
over a sawpit + log yard; Mine = pit-head frame (windlass, bucket, ore crate, lantern) +
stone yard. Layout contract unchanged: structure on −X (blocked), yard on +X (walkable).

### New materials (`castle.rs` `M`)

`Thatch` (textured straw courses), `HouseRoof2` (weathered shingle), `Iron`, `Parchment`.

## Harness

`FOREST_TOWN=full` raises all 12 dwellings for archetype shots. `FOREST_DEFEND` now also
stages `reinforced` + `villager_arms_tier: 2` so a defended shot shows the full courtyard.
