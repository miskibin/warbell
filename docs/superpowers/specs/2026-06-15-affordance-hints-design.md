# Affordance Hints — design

**Date:** 2026-06-15
**Status:** approved design, pre-implementation

## Goal

When the player is sitting on spendable wealth, or carrying gear strictly better than
what's worn, surface a calm "you can do X here" cue during the daytime **Prep** phase. It
nudges toward an action the player has unlocked but hasn't taken — and stays silent during
the night siege so it never distracts mid-fight.

This is a quality-of-life affordance, not a tutorial. It points at existing verbs (visit the
merchant, open the War Table, equip from the satchel); it does not teach controls.

## User-facing behaviour

- **Sitting on enough gold to buy a real upgrade** → the gold stat on the HUD glows softly
  (a slow pulse), and a one-time top-centre notice names the action: e.g. *"Plenty of gold —
  the merchant has a better blade (E)"* or *"You can afford an upgrade at the War Table (E)"*.
  The glow persists until the player either spends down below the threshold or buys the thing.
- **A better weapon/armor sitting unequipped in the bag** → a top-centre notice: *"Better
  blade in your satchel — Tab to equip"* / *"Better armor in your satchel — Tab to equip"*.
  No standing glow (there is no satchel element on the HUD to attach one to); instead the
  notice re-fires on a cooldown while the better gear remains unequipped.
- **All hints are Prep-only.** During a Wave (night siege) the glow is forced off and no
  notices fire. An unresolved hint resumes the following dawn.

## Architecture

New self-contained module `src/hints.rs` exposing `HintsPlugin`, added to the plugin list in
`main.rs`. It is **read-only over game state** — it never mutates `Player`, `Bank`,
`Inventory`, etc. Its only outputs are:

1. pushing strings into the existing `Notice` queue (`src/ui/notice.rs`), and
2. publishing a small `Hints` resource that the HUD reads to drive the glow.

This keeps a clean boundary: `hints.rs` decides *what* should glow; `hud.rs` keeps ownership
of its own entities and decides *how* to render the glow.

### Channels

Three independent hint channels, each a small state machine:

| Channel | Condition true when… | Output |
|---|---|---|
| `Spend` | gold (and/or stone) can buy something *worthwhile* (see below) | glow gold stat (+ stone stat if the affordable target is stone-gated) **and** a notice |
| `BetterWeapon` | the bag holds a weapon whose `damage_bonus` > the equipped weapon's | notice only |
| `BetterArmor` | the bag holds armor whose `defense` > the equipped armor's | notice only |

**"Worthwhile" (Spend) is upgrade-only**, deliberately strict to maximise signal:

- a shop catalog item (`build_shop_items(&eco.unlocked_weapons)`) that is **gear** (Weapon or
  Armor) AND strictly better than what's currently equipped (weapon `damage_bonus` >
  equipped weapon bonus, or armor `defense` > equipped armor defense), whose
  `discounted_price(item.price, eco.shop_discount)` ≤ player gold; **or**
- any `UPGRADE_NODES` entry that is not yet purchased and `up.0.can_buy(node, gold, stone,
  false)` returns true (prereqs met + affordable).

Affordable **consumables** (bread/potion/etc.) do **not** trip the Spend hint.

If the cheapest affordable target is a stone-gated upgrade node, the stone stat glows too;
otherwise only gold glows.

### State machine (per channel)

States: `Idle → Pending → Active`.

- **Idle**: condition false. Nothing shown.
- Condition becomes true → **Pending**, record `pending_since = now`.
- **Pending** held for `PROMOTE_DELAY` (~4.0s) while condition stays true → **Active**.
  (The delay rides out loot churn and mid-action pickups so the hint doesn't flicker.)
  If the condition goes false during Pending, drop straight back to Idle.
- On entering **Active**: push the channel's notice once.
- **Active**:
  - `Spend`: set the relevant glow flag(s) in `Hints`; it stays on.
  - `BetterWeapon` / `BetterArmor`: re-push the notice every `RENUDGE_INTERVAL` (~50s) while
    still Active (these have no standing glow, so a periodic repeat keeps them from being
    forgotten).
- Condition goes false at any point → **Idle**, clear glow flags.

Tunables (constants at the top of `hints.rs`): `PROMOTE_DELAY = 4.0`, `RENUDGE_INTERVAL =
50.0`, plus the glow pulse rate/amplitude used by the HUD.

### Prep gating

The hint-evaluation system carries `.run_if(in_state(Modal::None))` and additionally early-
returns unless `siege.phase == GamePhase::Prep`. When not in Prep it forces all glow flags
off and suppresses notices, but **does not reset the per-channel state** — so a hint that was
Active at dusk is still Active at the next dawn and re-shows without re-waiting the promote
delay.

State lives in a `Local` (or in the `Hints` resource) on the hint system — process-lifetime
only.

### The glow

`Hints` resource:

```rust
#[derive(Resource, Default)]
pub struct Hints { pub glow_gold: bool, pub glow_stone: bool }
```

`hud.rs`'s existing per-frame stat-update system gains a `Res<Hints>` and a `Res<Time>` read.
When `glow_gold` is set, it applies a soft pulse to the gold icon + number — a sine-driven
tint toward bright gold and a slight scale (e.g. `1.0 + 0.06 * pulse`). Same for stone. When
the flag is clear it renders the normal static colour/scale. Reuse `ui::anim` easing if it
fits cleanly; otherwise a direct `(time * RATE).sin()` is fine. The gold/stone marker
components (`GoldText`/`StoneText`) and their icon entities already exist in `hud.rs`; the
glow logic stays inside `hud.rs`.

## Data flow

```
hints::evaluate (Update, Modal::None, Prep-only)
  reads: PlayerRes, Bank, Inventory, EconomyState, Upgrades, Siege, Time
  writes: Notice (push), Hints (glow flags)

hud::update_stats (Update, ungated)
  reads: Hints, Time  (added)
  effect: pulses gold/stone icon+number when flagged
```

## Error / edge handling

- Empty bag / fists equipped (no weapon): equipped bonus is 0, so any bag weapon trips
  `BetterWeapon` — correct (a starting player should be told to equip the sword they're
  carrying). Same for armor.
- Nothing left to buy (everything affordable already owned / no better gear in shop): Spend
  condition is false → no nudge. The hint goes quiet on its own.
- Multiple better weapons in the bag: still one channel, one nudge (we don't enumerate).
- Resolving by selling rather than equipping also clears the gear hint (condition recheck).

## Save / load

**Nothing is persisted.** Hint state is pure derived UI state, recomputed from live resources
every frame — the same category as `Toasts` and `Buffs` (see `savegame.rs` deliberately-not-
saved list). On Continue the conditions re-evaluate from the loaded resources and the right
hints re-arm after the promote delay. No `SaveData` change.

## Testing

The conditions are thin wrappers over already-tested core stores, but the *worthwhile* /
*better-gear* predicates are worth a couple of pure unit checks if we factor them into free
functions in `hints.rs` (no Bevy types): given gold/stone + equipped ids + bag, assert
Spend/BetterWeapon/BetterArmor fire correctly. The state-machine timing and glow are verified
in-engine via the screenshot/clip harness (stage Prep with seeded gold + a spare `sword_gold`
in the bag, confirm the gold stat pulses and the notice reads correctly).

## Out of scope (YAGNI)

- Food / population / wood-specific hints.
- A new satchel button on the HUD.
- A settings toggle to disable hints.
- Any sound cue.
- Teaching controls beyond naming the key in the notice text.
