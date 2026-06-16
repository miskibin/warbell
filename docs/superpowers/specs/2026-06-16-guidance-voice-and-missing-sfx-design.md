# Guidance voice (narrator + hero), boss VO, and missing-SFX list — design

**Date:** 2026-06-16
**Why:** Two problems. (1) Players get lost — they don't know what to do next, what's wrong
(starving? broke? walls down?), or *why* they need a farm / woodcutter / mine / houses. (2) The
combat/economy/boss soundscape has gaps (bosses sound like ordinary orks; building and economy
events are silent). This doc is the shopping list for a **third voice actor (the Narrator)**, a
batch of **new hero lines** (mechanic hints + boss-approach musings), and the **missing SFX** to
record.

It is a *spec*, not an implementation. The new combat SFX (sword-hit/​block) shipped separately;
everything here is "record these, then wire them."

---

## How voice currently works (so these slot in cleanly)

- **Catalog:** `src/audio/lines.rs` — every line is one `Line { id, speaker, concept, text, … }`
  in the `LINES` array. `text` is the subtitle **and** our only in-code record of the quote
  (CLAUDE.md rule). Lines are gated per-line by `floor` (min seconds between replays) and `once`
  (once per run).
- **Speakers:** `Speaker::{Hero, Villager, Ork}` today. Routing comes from `SPEAKERS` and the
  clip dir is chosen in `director.rs:303`: `Hero→"hero"`, `Villager→"npc"`, `Ork→"ork"`, loaded
  from `audio/vo/<dir>/<id>.ogg`.
- **Triggers:** small `detect_*` systems watch game state and emit a `Concept`; the director
  resolves candidates, applies barge-in priority + the shared hero-line window, and plays one.
- **On-screen notices already exist** (`crate::ui::notice::Notice`) — e.g. `boss_proximity`
  (`src/boss/mod.rs:512`) pushes *"Something massive stirs in the {biome}…"* at range 28, and a
  warden death pushes *"{name} is slain!"*. **Voice should pair with these notices, not replace
  them** — text for the deaf/skim, voice for flavor.

### To add the Narrator (new third actor)

1. `Speaker::Narrator` in the enum + a `SPEAKERS` row: `spatial: false` (head-locked, like the
   hero — he's the voice of God, not a guy standing in the square), `name: Some("…")` or `None`,
   its own `gain`, no pitch jitter.
2. `director.rs` dir map: `Speaker::Narrator => "narrator"` → clips at `audio/vo/narrator/<id>.ogg`.
3. New `Concept`s (below) + `detect_*` systems reading town/economy/boss state. All sim-side
   triggers must carry `.run_if(in_state(Modal::None))` (CLAUDE.md freeze-gate rule) and a
   generous `floor` so the narrator advises, never nags.
4. **Nothing here needs saving** — all transient reactions to live state (no `SaveData` changes).

**Tone for the Narrator:** a melodramatic medieval chronicler / doom-prophet who explains a
mechanic by wildly over-dramatizing the stakes, then undercutting it. Trailer-voice gravity with
a dry punchline. He is the game *telling you what's wrong and what to build*, in costume. Keep
each line short enough to read as a subtitle (~1 breath).

---

## PART 1 — Voice lines to record

Format: **`concept`** — *trigger* — then candidate lines (record 2–3 per concept so they rotate;
`floor` keeps them from repeating). IDs are suggestions matching the `<id>.ogg` convention.

### 1A. NARRATOR — food & starvation (the headline "our population is starving")

- **`FoodStarving`** — *net food < 0 and population is shrinking (the starvation path in
  `town_store`).* This is the literal example the user asked for.
  - "Behold — the granaries echo, the stew is mostly rumor, and your people are **starving**.
    A farm, hero. Build a farm, before the eulogies start."
  - "Your subjects gnaw their own belts. STARVATION stalks the realm! …Honestly, one little farm
    would fix this."
  - "The town grows thin and the gravedigger grows busy. Feed them — raise a farm and staff it."
- **`FoodTight`** — *surplus near zero while population is at/over what the farms feed.*
  - "The larder grows nervous. More mouths arrive each dawn; the turnips, traitorously, do not.
    Another farm would settle everyone's nerves."
- **`FoodSurplus`** — *healthy food surplus AND a free house slot (room to grow).*
  - "The harvest overflows, bellies are full, morale is suspiciously high. Build them a **house** —
    give these well-fed souls room to make you *more* taxpayers."

### 1B. NARRATOR — why each building exists (first-time + low-resource nudges)

- **`NeedWood`** — *wood near zero and the player tried to build/upgrade, or it's day 1 and no
  woodcutter exists.*
  - "Your timber stores: one sad, splintered plank. Walls are not built of good intentions —
    raise a **woodcutter** and let the forest pay its taxes."
  - "No wood, no walls, no future. The trees are RIGHT THERE, mocking you. Put someone on an axe."
- **`NeedStone`** — *stone near zero / blocked from a stone upgrade.*
  - "Stone runs short, and a wall of optimism stops precisely no orks. You need a **miner**. The
    mountain is enormous and, frankly, just sitting there."
- **`NeedHouses`** — *population at cap with a food surplus (growth wasted).*
  - "Your people are stacked three to a cot and multiplying like a problem. Build **houses**,
    hero, before they revolt over the blanket arrangements."
- **`WhyFarm` / `WhyWoodcutter` / `WhyMine`** — *first time the build menu is opened, one-shot
  each (`once: true`), pure tutorial:*
  - Farm: "A **farm** feeds your people. Hungry people leave, or worse, *stay* and complain."
  - Woodcutter: "A **woodcutter** turns trees into the wood you spend on houses and walls."
  - Mine: "A **miner** chips stone from the hills — the stone your fortifications drink by the ton."

### 1C. NARRATOR — economy, defense & "what do I do next" nudges

- **`GoldHoard`** — *large unspent gold pile.*
  - "Your coffers groan with gold while your blade stays dull and your walls stay low. The War
    Table is *right there*, you magnificent miser."
- **`NeverUpgraded`** — *several nights survived, upgrade tree still untouched.*
  - "You have slain much and learned nothing. The War Table gathers dust. Even a hero may sharpen
    more than his sword."
- **`WallsLow`** — *no walls bought by the time the first sieges bite.*
  - "Your fortifications are a stern look and a knee-high fence. The horde finds this *delightful*.
    Buy **walls**."
- **`PrepDawdling`** — *prep phase, lots of daylight burned with little done.*
  - "Daylight bleeds away while you admire the view. Loot, build, arm — the night rewards no
    dawdlers."
- **`BellWaiting`** — *prep, the player is clearly ready and lingering.*
  - "When your courage at last outpaces your common sense — ring the **war bell**. The horde is
    RSVP'd and impatient."
- **`KeepFailing`** — *keep HP falling fast mid-wave* (urgent; high priority, pairs with the
  existing keep-hurt hero line).
  - "The keep WEEPS mortar! Should it fall, so falls everything — and the narration takes a very
    dark turn."

### 1D. NARRATOR — population events (pair with the on-screen count change)

- **`PopGrew`** — *a peasant settled.*
  - "A new soul joins the realm! Another mouth, another back, another name you will absolutely
    never learn."
- **`PopLost`** — *a peasant starved / was slain.*
  - "We are diminished. One fewer voice at the well tonight. Light a candle — or, better, fix the
    food."
- **`TownEmpty`** — *population back down to the larder floor.*
  - "The realm is now two stubborn souls and a chicken. Rebuild, hero, before the chicken declares
    itself regent."

### 1E. NARRATOR — boss "you learned X" (pairs with the existing "{name} is slain!" notice)

- **`WardenSlain(biome)`** — *warden death (`reward_on_death`, `src/boss/mod.rs:601`).* Names per
  biome: Old Bramblewood (Forest→Bramble Sweep), Bałwan the Frostgiant (Snow→Frostbite), Karngor
  the Stone Golem (Rocky→Ground Slam), The Sand Revenant (Desert→Sand Dash), Mabworm the Bog Hag
  (Swamp→Venom).
  - "The {warden} **falls!** Its power seeps into your bones — you have learned **{boon}**.
    Tell no one it was mostly luck."
  - (Generic fallback) "A warden lies broken, and you are the stronger for it. The island notices.
    The island always notices."

### 1F. HERO — new mechanic-hint musings (in-character, low priority, prep-phase)

These are the user's *"maybe I should give people more houses to live"* register — the hero
reasoning aloud, quieter and more personal than the narrator's grandstanding. New `Concept`s
(or fold into existing prep musings), `floor` ~300, priority 5, `Speaker::Hero` (`audio/vo/hero/`).

- **`HintHouses`** — *pop near cap / crowded:* "Folk are packed in tight down there. I should
  raise more houses — give them room to grow."
- **`HintFarm`** — *food low:* "Stomachs are growling louder than the orks out there. We need a
  farm before someone faints on the wall."
- **`HintWood`** — *wood low:* "Walls eat timber, and we're nearly dry. Best put someone on the
  trees."
- **`HintStone`** — *stone low:* "Stone's thin. The hills are full of it — time someone started
  digging."
- **`HintUpgrade`** — *gold high, tree untouched:* "Coin's piling up. I should spend it at the War
  Table before I get sentimental about it."
- **`HintBell`** — *prep, ready:* "Daylight won't hold. When I'm set, the bell calls the night."

### 1G. HERO — boss-approach musings (the user's *"I feel in my bones…"* request)

*Trigger:* hook `boss_proximity` (`src/boss/mod.rs:512`, NOTICE_RANGE 28) — alongside the existing
notice, emit `Concept::NearWarden(biome)`. `once: true` per warden per run (or a long `floor`),
priority ~8 so it lands but yields to combat warnings. Each teases that warden's **boon** without
naming UI ("learn something" / grow stronger), per the requested style.

- **Generic:** "Something vast sleeps out here. I feel it in my bones — put that beast down, and
  I'd walk away the stronger for it."
- **Forest — Old Bramblewood (→ Bramble Sweep):** "That old wooden thing among the trees… beat it,
  and I think my blade would learn a new sweep. I can almost feel it."
- **Snow — Bałwan, the Frostgiant (→ Frostbite):** "Whatever stirs in the ice up here — kill it,
  and I swear the cold itself would start fighting on my side."
- **Rocky — Karngor, the Stone Golem (→ Ground Slam):** "A mountain that walks. Break it, and
  maybe I'd learn to bring the mountain down myself."
- **Desert — The Sand Revenant (→ Sand Dash):** "That dead thing moves like the wind off the
  dunes. Put it down, and perhaps I would too."
- **Swamp — Mabworm, the Bog Hag (→ Venom):** "The bog-hag's brewed every poison there is. Best
  her, and I'd turn that venom loose on the horde."

> Optional polish: a separate `WardenLevelUp` narrator/hero nudge at dawn — wardens gain a level
> every dawn (`boss_levelup`), so they get *harder the longer you wait*. A line like the narrator's
> "The beasts of this island grew bolder overnight — slay them young, hero" teaches the
> kill-early incentive that's currently invisible to the player.

---

## PART 2 — Missing SFX to record

Derived from the authoritative `AudioCue` enum (`src/audio/mod.rs`) — every event with a sound is
a variant there; these are real events with **no** dedicated cue. Priority P0→P2. "Reuse?" notes
an existing clip that could stand in cheaply until a bespoke one is recorded.

### P0 — Boss / warden audio (today wardens sound like ordinary orks)

Wardens currently borrow `OrkRoar` on a crit telegraph and `Slam` on impact — five distinct,
named, building-sized bosses share a grunt with a foot-soldier. Record:

- **Warden aggro roar** — one per warden ideally (5: treant groan, frost-giant bellow, stone-golem
  grind, revenant shriek, bog-hag cackle); at minimum one big shared "boss roar" distinct from
  `ork-roar`. *Fires when the warden turns hostile (first hit).*
- **Crit-telegraph wind-up** — a rising, dread "it's about to do the big one" cue during the
  1.2 s telegraph (so the player knows to block/dodge by **ear**, not just the visual). Currently
  reuses `OrkRoar`.
- **Signature-attack cues** — the radial **shock** burst (Forest/Rocky/Swamp) and the **volley**
  release (Snow ice shards / Desert sand). Distinct from melee.
- **Warden footfall** — heavy, slow stomp as it roams/chases (sells the size). Spatial.
- **Warden-slain fanfare** — a bigger, rarer victory sting than the normal `LevelUp` so a boss
  kill feels like a boss kill. Pairs with the boon-reward dialog.
- *(Stretch)* **boss combat-music shift** when a warden aggros (`music.rs` already swaps
  bed↔combat — add a boss layer).

### P1 — Town / building lifecycle (currently silent)

- **Construction complete** — a satisfying "raised!" thunk/hammer-flourish when a farm/house/
  woodcutter/mine finishes. *Reuse?* none good — record one.
- **Building on fire** — a fire-crackle loop on a burning plot. *Reuse?* `campfire-loop.ogg` could
  stand in.
- **Building collapses to rubble** — a wood/stone crash when a plot hits 0 HP. *Reuse?* layer
  `tree-fall.ogg` + `wood-crack.ogg`.
- **Peasant settled / house occupied** — a small warm chime on population growth.
- **Peasant lost (starved/slain)** — a brief sombre cue (sells the loss the `PopLost` voice line
  reacts to).

### P1 — Defenses & projectiles (only the *magical* bolt exists)

`WarpCast` covers shaman/tower magic, but the *physical* defenses are silent:

- **Bow twang + arrow impact** — for `def_keep_archers` and town-guard archers.
- **Ballista fire + bolt impact** — heavier than a bow, for `def_ballista`.
- **Gate open/close** + **wall breach** (stone crumble when a wall section is destroyed) — distinct
  from generic building damage.

### P1 — Economy / UI

- **Upgrade purchased at the War Table** — currently likely reuses `ShopBuy`; a beefier "power
  acquired" sting for **ability/boon unlocks** (cleave, crit, and the five warden boons) would sell
  permanent progression.
- **Action denied / can't afford** — a short negative blip. Today `menu-select` is the only UI
  sound, so a failed purchase is silent and reads as a bug.

### P2 — Combat & movement polish

- **Passive-boon on-hit tells** — Frostbite has no "freeze/ice-crackle" and Venom has no "poison
  sizzle"; the player can't *hear* their passive warden boons working. Short per-hit accents.
- **Guard / shield break** — a distinct "your guard is broken" crack when block-stamina is
  exhausted (different from a successful `Block`). Important readable feedback.
- **Biome footstep surfaces** — `Footstep` only has Dirt/Snow/Stone; **desert sand** (soft,
  granular) and **swamp mud / shallow-water wade** (wet squelch) currently fall back to Dirt across
  two of the five biomes. Add a `Surface::Sand` and `Surface::Mud`.

---

## Suggested record order

1. **Narrator food/build/economy nudge set (1A–1C)** + matching **hero hints (1F)** — this is the
   actual fix for "players are lost," and it's pure data+trigger work once the clips exist.
2. **Boss VO + boss SFX (1E, 1G, P0)** — makes the five wardens feel like events, not big orks,
   and teaches *why* to hunt them.
3. **Building/economy SFX (P1)** — makes the town read as alive and gives the economy feedback.
4. **Polish (P2).**
