# Affordance Hints — design

**Date:** 2026-06-15
**Status:** approved design, pre-implementation

## Goal

When the player is sitting on spendable wealth, or carrying gear strictly better than
what's worn, surface a calm "you can do X here" cue during the daytime **Prep** phase. It
nudges toward an action the player has unlocked but hasn't taken — visit the merchant, open
the War Table, equip from the satchel — then gets out of the way on its own.

This is a quality-of-life affordance, not a tutorial. It points at existing verbs; it does
not teach controls.

## User-facing behaviour

- A single **decorated toast** appears in the **bottom-right** corner naming an available
  action, e.g. *"Plenty of gold — the merchant has a better blade (E)"*, *"You can afford an
  upgrade at the War Table (E)"*, or *"Better blade in your satchel — Tab to equip"*.
- It is fancier than a plain pickup toast: gold border, a soft pulsing glow, an icon (coin
  for spend, sword/shield for gear), one line of text. Pops in, holds, fades out.
- It **auto-expires after ~60s** and disappears, even if the condition still holds — it will
  not nag forever. It does not re-show for the same condition until that condition has gone
  away and come back (you spent the gold / equipped the gear, then later re-qualified).
- Resolving the condition (spending below the threshold, equipping/selling the gear) makes
  the toast **fade out early**.
- **Prep-only.** No hint toasts during a Wave (night siege). They re-evaluate fresh at the
  next dawn.

No HUD glow, no new stat decoration — the toast is the whole feature.

## Why bottom-right

Current HUD occupancy (so the hint never overlaps an existing element):

| Zone | Owner |
|---|---|
| top-left | stat bar (gold/stone/wood/pop/food) + pickup-toast column |
| top-center | `Notice` queue (system pills) |
| bottom-left | level badge + HP/XP/stamina bars + buff pips |
| bottom-center | quick-slot bar (Q/Z/X/C) |
| **bottom-right** | **free → hint toasts live here** |

The hint toasts get their own anchored column at bottom-right and stack upward if more than
one channel is active, exactly like the existing notice/toast columns.

## Architecture

New self-contained module `src/hints.rs` exposing `HintsPlugin`, added to the plugin list in
`main.rs`. It is **read-only over game state** and owns its own UI entities (the bottom-right
column + toast rows). It does **not** touch `hud.rs`, the `Notice` queue, or any other
module's entities — clean boundary, no shared element to overlap.

- Reads: `PlayerRes`, `Bank`, `Inventory`, `EconomyState`, `Upgrades`, `Siege`, `Time`,
  `IconAtlas`, `UiFonts`.
- Writes: only its own hint-toast entities.

### Channels

Three independent hint channels, each a small state machine producing at most one toast:

| Channel | Condition true when… | Toast text (example) |
|---|---|---|
| `Spend` | gold (and/or stone) can buy something *worthwhile* (see below) | "You can afford an upgrade at the War Table (E)" / "The merchant has a better blade (E)" |
| `BetterWeapon` | bag holds a weapon whose `damage_bonus` > the equipped weapon's | "Better blade in your satchel — Tab to equip" |
| `BetterArmor` | bag holds armor whose `defense` > the equipped armor's | "Better armor in your satchel — Tab to equip" |

**"Worthwhile" (Spend) is upgrade-only**, deliberately strict to maximise signal:

- a shop catalog item (`build_shop_items(&eco.unlocked_weapons)`) that is **gear** (Weapon or
  Armor) AND strictly better than what's currently equipped (weapon `damage_bonus` >
  equipped weapon bonus, or armor `defense` > equipped armor defense), whose
  `discounted_price(item.price, eco.shop_discount)` ≤ player gold; **or**
- any `UPGRADE_NODES` entry not yet purchased where `up.0.can_buy(node, gold, stone, false)`
  is true (prereqs met + affordable).

Affordable **consumables** (bread/potion/etc.) do **not** trip the Spend hint. The toast text
prefers the War-Table phrasing if a node is the affordable target, else the merchant phrasing.

### State machine (per channel)

States: `Idle → Pending → Shown → Latched`.

- **Idle**: condition false, no toast. Re-arm allowed.
- Condition becomes true → **Pending**, record `pending_since = now`.
- **Pending** held for `PROMOTE_DELAY` (~3.0s) while condition stays true → spawn the toast,
  set `expires_at = now + SHOW_SECS` (~60.0), enter **Shown**. (The delay rides out loot
  churn / mid-action pickups so the toast doesn't flicker in.) If the condition goes false
  during Pending → straight back to Idle.
- **Shown**: toast visible (with a pop-in, a gentle border pulse, and a fade-out over the
  last ~0.6s before `expires_at`).
  - reaching `expires_at` → despawn toast, enter **Latched**.
  - condition goes false before expiry → despawn toast (early fade), → **Idle**.
- **Latched**: condition still true but we've already shown our one toast; stay silent. When
  the condition finally goes false → **Idle** (re-arming for a future trigger).

Tunables (constants at top of `hints.rs`): `PROMOTE_DELAY = 3.0`, `SHOW_SECS = 60.0`,
`FADE_SECS = 0.6`, plus pulse rate/amplitude.

### Prep gating

The hint system carries `.run_if(in_state(Modal::None))` and early-returns unless
`siege.phase == GamePhase::Prep`. On leaving Prep it **despawns any visible hint toasts and
resets all channels to `Idle`** — so an interrupted hint does not carry a stale 60s timer
across a siege; at the next dawn the conditions re-evaluate and re-arm cleanly through the
normal promote delay.

Channel state lives in a `Local` on the hint system (process-lifetime only).

## Data flow

```
hints::drive (Update, run_if Modal::None, early-return unless Prep)
  reads:  PlayerRes, Bank, Inventory, EconomyState, Upgrades, Siege, Time, IconAtlas, UiFonts
  writes: its own bottom-right hint-toast entities (spawn / despawn / fade)
```

No other system changes. `main.rs` gains one `HintsPlugin` line.

## Error / edge handling

- Empty bag / fists equipped: equipped bonus is 0, so a carried starter weapon trips
  `BetterWeapon` — correct (tell the player to equip the sword they're holding). Same armor.
- Nothing left to buy / no better gear: condition false → no toast. Goes quiet by itself.
- Multiple better weapons in the bag → still one `BetterWeapon` toast (we don't enumerate).
- Resolving by selling rather than equipping also clears the gear hint (condition recheck).
- Two or three channels active at once → up to three toasts stacked in the bottom-right
  column; each runs its own timer independently.

## Save / load

**Nothing is persisted.** Hint state is pure derived UI state recomputed from live resources
— same category as `Toasts` and `Buffs` (see `savegame.rs` deliberately-not-saved list). On
Continue the conditions re-evaluate from the loaded resources and re-arm after the promote
delay. No `SaveData` change.

## Testing

Factor the predicates into pure free functions in `hints.rs` (no Bevy types) and unit-test
them: given gold/stone + equipped ids + bag + shop/upgrade inputs, assert
Spend / BetterWeapon / BetterArmor fire correctly (including the consumable-doesn't-count and
fists-equipped cases). The state-machine timing + toast visuals are verified in-engine via
the screenshot/clip harness: stage Prep with seeded gold and a spare `sword_gold` in the bag,
confirm the decorated toast appears bottom-right, reads correctly, and fades after the
timeout.

## Out of scope (YAGNI)

- HUD stat glow (cut — the decorated toast replaces it).
- Food / population / wood-specific hints.
- A new satchel button on the HUD.
- A settings toggle to disable hints.
- Any sound cue.
- Teaching controls beyond naming the key in the toast text.
