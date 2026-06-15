//! Port of src/world/inventoryStore.ts — the loot model: one general-purpose BAG
//! plus two equip slots (weapon + armor), and the four DERIVED quick-slots (Q food
//! + z/x/c buffs) that surface the next matching bag item.
//!
//! The TS module is a global store with subscribe/notify and side effects
//! (healPlayer / applyBuff / SFX / pushItemToast). Per the parity brief those
//! cross-store + render/audio effects are DROPPED here so the core stays pure and
//! testable: consuming an item RETURNS the effect it would apply (a `ConsumeEffect`
//! describing the heal + buff) for the ECS layer to enact, and `add`/`place` return
//! whether the slot was taken (the ECS layer fires the pickup toast on success).
//! `BuffKind` is reused from `buff_store` (its TS counterpart).
//!
//! The bag insertion split is preserved: `place` is the silent path (used by both
//! real pickups and gear swaps/unequips returning a piece to the bag), and `add` is
//! `place` + a `true` return that signals "fire a pickup toast". Equipping reports
//! the resulting weapon-damage bonus / armor damage-mult so the game wires them
//! into the swing/damage paths.

use crate::buff_store::BuffKind;

/// What a consumable does when used: an optional heal and an optional timed buff.
/// `consume*` returns this so the ECS layer applies the real `heal`/`apply_buff`
/// (the TS called `healPlayer`/`applyBuff` directly; here those are caller effects).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConsumeEffect {
    /// HP to restore (0 = none).
    pub heal: f64,
    /// Timed buff to grant on use, if any: (kind, duration_ms, magnitude).
    pub buff: Option<(BuffKind, f64, f64)>,
}

/// Item category. `Token` = a key/quest item that just sits in the bag (the
/// Mercenary Contract spent to recruit a trader) — inert to eat/equip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Consumable,
    Weapon,
    Armor,
    Token,
}

/// Which quick-slot a consumable surfaces in: the Food slot (Q) or one of the
/// three buff slots (z/x/c). Mirrors the TS `QuickKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickKind {
    Food,
    Buff(BuffKind),
}

/// A static item definition (one entry in the `ITEM_DEFS` table).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemDef {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub kind: ItemKind,
    /// consumable: hp restored (0 = none).
    pub heal: f64,
    /// weapon: bonus added to base attack damage when equipped (0 = none).
    pub damage_bonus: f64,
    /// armor: fraction of incoming damage removed when worn (0.25 = −25%).
    pub defense: f64,
    /// consumables stack; weapons/armor don't.
    pub stackable: bool,
    /// consumable: timed buff granted on use — (kind, duration_ms, magnitude).
    pub buff: Option<(BuffKind, f64, f64)>,
    /// consumable: explicit quick-slot override; `None` defaults to the buff kind,
    /// else Food (resolved by `quick_of`).
    pub quick: Option<QuickKind>,
}

impl ItemDef {
    /// Compact builder so the static table reads like the TS object literals.
    const fn new(id: &'static str, name: &'static str, icon: &'static str, kind: ItemKind) -> Self {
        ItemDef {
            id,
            name,
            icon,
            kind,
            heal: 0.0,
            damage_bonus: 0.0,
            defense: 0.0,
            stackable: false,
            buff: None,
            quick: None,
        }
    }
    const fn heal(mut self, h: f64) -> Self {
        self.heal = h;
        self
    }
    const fn dmg(mut self, d: f64) -> Self {
        self.damage_bonus = d;
        self
    }
    const fn def(mut self, d: f64) -> Self {
        self.defense = d;
        self
    }
    const fn stack(mut self) -> Self {
        self.stackable = true;
        self
    }
    const fn buff(mut self, kind: BuffKind, duration_ms: f64, mag: f64) -> Self {
        self.buff = Some((kind, duration_ms, mag));
        self
    }
    const fn quick(mut self, q: QuickKind) -> Self {
        self.quick = Some(q);
        self
    }

    /// Which quick-slot this consumable feeds: explicit tag, else its buff kind,
    /// else Food (mirrors the TS `quickOf`).
    pub fn quick_of(&self) -> QuickKind {
        if let Some(q) = self.quick {
            q
        } else if let Some((k, _, _)) = self.buff {
            QuickKind::Buff(k)
        } else {
            QuickKind::Food
        }
    }

    /// One-line stat summary — the panel tooltip + pickup toast share it. Uses plain
    /// ASCII (`-` for the armor reduction, `,` between consumable parts) rather than the
    /// `−`/`·` typographic glyphs: this string is rendered in the Bevy HUD with the
    /// default embedded font, which has no U+2212/U+00B7 and would draw them as tofu.
    pub fn stat_line(&self) -> String {
        match self.kind {
            ItemKind::Weapon => format!("+{} attack", self.damage_bonus as i64),
            ItemKind::Armor => format!("-{}% damage taken", (self.defense * 100.0).round() as i64),
            ItemKind::Token => "Key item".to_string(),
            ItemKind::Consumable => {
                let mut parts: Vec<String> = Vec::new();
                if self.heal > 0.0 {
                    parts.push(format!("+{} HP", self.heal as i64));
                }
                if let Some((k, dur, _)) = self.buff {
                    parts.push(format!("{} {}s", k.label(), (dur / 1000.0).round() as i64));
                }
                if parts.is_empty() {
                    "No effect".to_string()
                } else {
                    parts.join(", ")
                }
            }
        }
    }
}

// ─── Procedural icon spec (data; rasterised in the game crate) ──────────────────
//
// The TS HUD drew item icons as EMOJI glyphs (`def.icon`). Bevy's default embedded
// font has no emoji coverage, so those render as blank "tofu". Instead of shipping a
// font/sprite-sheet, the icon is described HERE as a tiny deterministic shape recipe
// (`IconSpec`) — colours + a primitive — that the game crate rasterises into a 48²
// RGBA `Image` at startup (mirroring the project's "make the art procedurally, no
// asset files" ethos, same as `terrain_tex::detail_image`). Keeping the recipe in
// core (pure data) lets a unit test pin the mapping without a render device.

/// 0–255 RGB colour for an icon primitive (alpha is implied opaque where drawn).
pub type IconRgb = (u8, u8, u8);

/// The drawable shape of an item icon — a handful of recognisable primitives that
/// stand in for the emoji's intent. `fg` is the main fill; `accent` a secondary
/// detail colour (stem/leaf/band/sheen). The game crate's rasteriser turns one of
/// these into pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconShape {
    /// Round fruit: a filled `fg` circle with a brown stem + an `accent` leaf (apple).
    Apple,
    /// Leafy sprigs: a few `fg` blades fanning up from a base (herb / forage).
    Herb,
    /// A cut of meat: a rounded `fg` slab with an `accent` bone nub (steak / jerky).
    Meat,
    /// A loaf/round food: a filled `fg` oval (bread / feast / generic food).
    Food,
    /// A potion flask: an `fg` rounded body with an `accent` cork/neck (potion).
    Potion,
    /// A bladed weapon: an `fg` blade triangle on an `accent` hilt (swords / axe / maul).
    Blade,
    /// A shield/plate: an `fg` heater-shield silhouette with an `accent` band (armor).
    Shield,
    /// A rolled scroll: a tan `fg` sheet with `accent` rolled ends (contract / token).
    Scroll,
    /// A tinted orb/charm: a glassy `fg` circle with an `accent` highlight (charm / venom / fur).
    Orb,
}

/// A complete procedural icon recipe: a primitive + its two colours. Deterministic
/// per item id (see `ItemDef::icon_spec`), so the rasterised atlas is stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconSpec {
    pub shape: IconShape,
    /// Primary fill colour (the body of the fruit/blade/shield/…).
    pub fg: IconRgb,
    /// Secondary detail colour (stem-leaf / hilt / band / cork / highlight).
    pub accent: IconRgb,
}

impl ItemDef {
    /// The procedural icon recipe for this item — the colour + shape that stands in
    /// for the emoji `icon`. A handful of items get a bespoke spec (apple, herb, the
    /// meats, the potion, the scroll); the rest group by `ItemKind` with a colour
    /// nudged toward the emoji's hue (gold blade vs iron blade, gilded plate vs iron,
    /// the three buff charms tinted per buff). Pure + deterministic so the rasterised
    /// atlas is stable and the mapping is unit-testable without a render device.
    pub fn icon_spec(&self) -> IconSpec {
        // A few id-specific recipes first (the bespoke icons the brief calls for).
        match self.id {
            "apple" => return IconSpec { shape: IconShape::Apple, fg: (210, 50, 45), accent: (70, 150, 55) },
            "marsh_herb" => return IconSpec { shape: IconShape::Herb, fg: (90, 165, 70), accent: (60, 120, 50) },
            "croc_steak" => return IconSpec { shape: IconShape::Meat, fg: (170, 70, 65), accent: (235, 220, 200) },
            "elk_jerky" => return IconSpec { shape: IconShape::Meat, fg: (140, 78, 50), accent: (235, 220, 200) },
            "potion" => return IconSpec { shape: IconShape::Potion, fg: (210, 60, 90), accent: (180, 140, 90) },
            "mercenary_contract" => return IconSpec { shape: IconShape::Scroll, fg: (215, 195, 150), accent: (150, 120, 80) },
            // The three buff charms — tinted by their buff hue (resist blue, power red,
            // haste cyan/green) so they read apart on the QuickBar.
            "fur" => return IconSpec { shape: IconShape::Orb, fg: (150, 120, 90), accent: (220, 200, 180) },
            "venom" => return IconSpec { shape: IconShape::Orb, fg: (110, 175, 70), accent: (210, 235, 180) },
            "goat_charm" => return IconSpec { shape: IconShape::Orb, fg: (215, 185, 80), accent: (250, 235, 170) },
            // The top-tier rim gear — distinct hues from the base swords/plate.
            "blade_frost" => return IconSpec { shape: IconShape::Blade, fg: (170, 210, 240), accent: (90, 110, 140) },
            "dragon_plate" => return IconSpec { shape: IconShape::Shield, fg: (70, 120, 80), accent: (40, 70, 50) },
            "sword_gold" => return IconSpec { shape: IconShape::Blade, fg: (235, 205, 90), accent: (150, 110, 50) },
            "gold_armor" => return IconSpec { shape: IconShape::Shield, fg: (220, 185, 90), accent: (150, 115, 50) },
            _ => {}
        }
        // Fallbacks grouped by kind.
        match self.kind {
            ItemKind::Weapon => IconSpec { shape: IconShape::Blade, fg: (190, 195, 205), accent: (120, 90, 55) },
            ItemKind::Armor => IconSpec { shape: IconShape::Shield, fg: (150, 155, 165), accent: (90, 95, 105) },
            ItemKind::Token => IconSpec { shape: IconShape::Scroll, fg: (215, 195, 150), accent: (150, 120, 80) },
            ItemKind::Consumable => {
                // A consumable with no bespoke spec: a buff item → tinted orb, a healer
                // → a loaf/food round.
                if self.buff.is_some() {
                    IconSpec { shape: IconShape::Orb, fg: (150, 130, 200), accent: (220, 210, 245) }
                } else {
                    IconSpec { shape: IconShape::Food, fg: (200, 160, 95), accent: (150, 110, 60) }
                }
            }
        }
    }
}

/// The full item table (port of `ITEM_DEFS`). Kept as a slice of definitions;
/// `item_def(id)` resolves an id, mirroring the TS `ITEM_DEFS[id]` lookup.
pub static ITEM_DEFS: &[ItemDef] = &[
    ItemDef::new("bread", "Bread", "🍞", ItemKind::Consumable).heal(15.0).stack(),
    ItemDef::new("potion", "Health Potion", "🧪", ItemKind::Consumable).heal(40.0).stack(),
    ItemDef::new("feast", "Tavern Feast", "🍖", ItemKind::Consumable).heal(100.0).stack(),
    // Gear stacks too: duplicate weapons/armor collapse into one bag cell with a count instead of
    // each taking its own slot (equipping pulls one off the stack; the spares can be sold/dropped).
    ItemDef::new("sword_iron", "Iron Sword", "⚔️", ItemKind::Weapon).dmg(11.0).stack(),
    ItemDef::new("sword_gold", "Golden Blade", "🗡️", ItemKind::Weapon).dmg(21.0).stack(),
    ItemDef::new("axe", "Battle Axe", "🪓", ItemKind::Weapon).dmg(15.0).stack(),
    // ─── Biome creature drops ─────────────────────────────────────
    ItemDef::new("fur", "Thick Fur", "🧥", ItemKind::Consumable)
        .stack()
        .buff(BuffKind::Resist, 12000.0, 0.6),
    ItemDef::new("venom", "Venom Vial", "🧫", ItemKind::Consumable)
        .stack()
        .buff(BuffKind::Power, 12000.0, 1.4),
    ItemDef::new("goat_charm", "Goat Charm", "🔔", ItemKind::Consumable)
        .stack()
        .buff(BuffKind::Haste, 12000.0, 1.3),
    ItemDef::new("croc_steak", "Croc Steak", "🥩", ItemKind::Consumable).heal(70.0).stack(),
    ItemDef::new("elk_jerky", "Elk Jerky", "🍖", ItemKind::Consumable).heal(35.0).stack(),
    // Foraged in the swamp — heal + resist poultice, tagged Food (the resist is a
    // bonus on eating; the tag decides where it shows, not what it does).
    ItemDef::new("marsh_herb", "Marsh Herb", "🌿", ItemKind::Consumable)
        .heal(30.0)
        .stack()
        .buff(BuffKind::Resist, 14000.0, 0.6)
        .quick(QuickKind::Food),
    // Foraged in the western forest — a quick snack heal.
    ItemDef::new("apple", "Forest Apple", "🍎", ItemKind::Consumable).heal(18.0).stack(),
    ItemDef::new("stone_maul", "Stone Maul", "🔨", ItemKind::Weapon).dmg(18.0).stack(),
    // ─── Wearable armor ───────────────────────────────────────────
    ItemDef::new("leather_armor", "Leather Armor", "🦺", ItemKind::Armor).def(0.11).stack(),
    ItemDef::new("iron_armor", "Iron Cuirass", "🛡️", ItemKind::Armor).def(0.2).stack(),
    ItemDef::new("gold_armor", "Gilded Plate", "👑", ItemKind::Armor).def(0.28).stack(),
    // ─── Rim-only top tier (frontier gradient) ────────────────────
    ItemDef::new("blade_frost", "Frostfang Greatsword", "🗡️", ItemKind::Weapon).dmg(34.0).stack(),
    ItemDef::new("dragon_plate", "Dragonscale Plate", "🐉", ItemKind::Armor).def(0.42).stack(),
    // ─── Key items (tokens) ───────────────────────────────────────
    ItemDef::new("mercenary_contract", "Mercenary Contract", "📜", ItemKind::Token).stack(),
];

/// Resolve an item id to its definition (the TS `ITEM_DEFS[id]`).
pub fn item_def(id: &str) -> Option<&'static ItemDef> {
    ITEM_DEFS.iter().find(|d| d.id == id)
}

/// Gold the merchant pays to BUY this item back from the hero (its sell value). Scales with the
/// piece's stats but stays well under the shop's selling price, so buy-low/sell-high is never an
/// exploit. Tokens (key/quest items) are not sellable → 0. Drives the shop's SELL list.
pub fn sell_value(id: &str) -> i64 {
    let Some(d) = item_def(id) else { return 0 };
    match d.kind {
        ItemKind::Weapon => 4 + (d.damage_bonus * 1.5).round() as i64,
        ItemKind::Armor => 4 + (d.defense * 60.0).round() as i64,
        ItemKind::Consumable => 1 + (d.heal / 8.0).round() as i64 + if d.buff.is_some() { 3 } else { 0 },
        ItemKind::Token => 0,
    }
}

/// General-purpose bag capacity (the TS `BAG_SIZE`). Generous so pickups rarely
/// bounce.
pub const BAG_SIZE: usize = 24;

/// Number of player-assignable quick-slots (the **Z / X / C** keys). The **Q** food
/// slot is separate and always derives the next food, so it is not counted here.
pub const QUICK_SLOTS: usize = 3;

/// The buff kind a bindable quick-slot defaults to — drives both `auto_bind` targeting
/// and the empty-slot ghost icon: slot 0 = Z (Resist), 1 = X (Power), 2 = C (Haste).
pub fn quick_default_kind(slot: usize) -> Option<BuffKind> {
    match slot {
        0 => Some(BuffKind::Resist),
        1 => Some(BuffKind::Power),
        2 => Some(BuffKind::Haste),
        _ => None,
    }
}

/// One bag cell: `None` = empty, else an item id + a stack count.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Slot {
    pub item_id: Option<String>,
    pub count: i64,
}

impl Slot {
    fn empty() -> Self {
        Slot { item_id: None, count: 0 }
    }
    fn clear(&mut self) {
        self.item_id = None;
        self.count = 0;
    }
}

/// A derived quick-slot view (the next bag item feeding Q / z / x / c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSlot {
    pub item_id: String,
    pub count: i64,
}

/// The full inventory: the bag + two equip slots + the cached equip-derived
/// combat values the game reads (weapon bonus, armor damage-mult). Port of the TS
/// `InventoryState`; the `open` modal flag is the ECS `AppState::Inventory`, so it
/// is NOT stored here.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bag {
    pub bag: Vec<Slot>,
    /// equipped weapon's damage bonus (0 = fists/starter).
    pub weapon_bonus: f64,
    pub equipped_id: Option<String>,
    /// equipped armor id (None = bare); drives the model re-skin in the render layer.
    pub equipped_armor_id: Option<String>,
    /// incoming-damage multiplier from worn armor (1 = none, 0.6 = −40%).
    pub armor_damage_mult: f64,
    /// The three player-assignable quick-slots **Z / X / C** (Q is the fixed food
    /// slot, handled by `eat_food`). Each holds the item id pinned to that slot, or
    /// `None` for an unbound slot. A bind is by item *id* (not bag position), so it
    /// survives stacks moving and persists at count 0 (the slot greys but keeps its
    /// item — Diablo-belt style). New pickups `auto_bind` into an empty default slot,
    /// so a new player gets the old derived layout (resist→Z, power→X, haste→C) with
    /// zero setup. See [`Bag::set_quick_bind`] / [`Bag::use_quick_slot`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub quick_binds: [Option<String>; QUICK_SLOTS],
}

impl Default for Bag {
    fn default() -> Self {
        Bag {
            bag: (0..BAG_SIZE).map(|_| Slot::empty()).collect(),
            weapon_bonus: 0.0,
            equipped_id: None,
            equipped_armor_id: None,
            armor_damage_mult: 1.0,
            quick_binds: [None, None, None],
        }
    }
}

impl Bag {
    pub fn new() -> Self {
        Self::default()
    }

    // ─── Bag management ──────────────────────────────────────────────

    /// Drop an item into the bag with NO pickup toast — the shared insertion path
    /// (real pickups AND gear swaps/unequips returning a piece). Returns false if
    /// there's no room (unknown id also returns false). Mirrors `placeInBag`.
    pub fn place(&mut self, item_id: &str, count: i64) -> bool {
        if count <= 0 {
            return false; // adding nothing (or a negative) must not occupy a slot or shrink a stack
        }
        let Some(def) = item_def(item_id) else {
            return false;
        };
        if def.stackable
            && let Some(existing) = self.bag.iter_mut().find(|s| s.item_id.as_deref() == Some(item_id))
        {
            existing.count += count;
            return true;
        }
        let Some(empty) = self.bag.iter_mut().find(|s| s.item_id.is_none()) else {
            return false;
        };
        empty.item_id = Some(item_id.to_string());
        empty.count = count;
        true
    }

    /// Pick up an item: add it to the bag. Returns true when the slot was taken —
    /// the ECS layer fires the pickup toast on `true` (the TS `addItem` did the
    /// toast inline; here that side effect is the caller's). Mirrors `addItem`.
    ///
    /// On a successful pickup the item also `auto_bind`s into an empty default
    /// quick-slot (gear swap-returns go through `place`, which never auto-binds), so
    /// every pickup path — forage, chest, hunt, shop — keeps the quick-bar populated
    /// with no extra call sites.
    pub fn add(&mut self, item_id: &str, count: i64) -> bool {
        let ok = self.place(item_id, count);
        if ok {
            self.auto_bind(item_id);
        }
        ok
    }

    /// Merge separate slots holding the same stackable item into one (folding later copies into the
    /// first, clearing the emptied slots). New pickups already stack via `place`; this fixes a bag
    /// that accumulated duplicate gear in distinct cells *before* gear became stackable. Idempotent.
    pub fn coalesce(&mut self) {
        let mut first: Vec<(String, usize)> = Vec::new(); // (id, slot of its first stack)
        for i in 0..self.bag.len() {
            let Some(id) = self.bag[i].item_id.clone() else { continue };
            if !item_def(&id).map(|d| d.stackable).unwrap_or(false) {
                continue;
            }
            if let Some(&(_, f)) = first.iter().find(|(sid, _)| *sid == id) {
                let n = self.bag[i].count;
                self.bag[f].count += n;
                self.bag[i].clear();
            } else {
                first.push((id, i));
            }
        }
    }

    /// Total count of an item across the bag.
    pub fn count_item(&self, item_id: &str) -> i64 {
        self.bag
            .iter()
            .filter(|s| s.item_id.as_deref() == Some(item_id))
            .map(|s| s.count)
            .sum()
    }

    /// True if the player holds at least one of `item_id`.
    pub fn has_item(&self, item_id: &str) -> bool {
        self.count_item(item_id) > 0
    }

    /// True if the bag could accept EVERY id in `ids` at once — accounts for empty
    /// slots plus stackable merges. Lets a chest avoid granting loot it has no room
    /// for. Mirrors `bagHasRoomFor`.
    pub fn has_room_for(&self, ids: &[&str]) -> bool {
        let mut free = self.bag.iter().filter(|s| s.item_id.is_none()).count() as i64;
        let mut stacks: Vec<String> = self
            .bag
            .iter()
            .filter_map(|s| s.item_id.clone())
            .filter(|id| item_def(id).map(|d| d.stackable).unwrap_or(false))
            .collect();
        for id in ids {
            let Some(def) = item_def(id) else { continue };
            if def.stackable && stacks.iter().any(|s| s == id) {
                continue; // merges into an existing stack
            }
            if free <= 0 {
                return false;
            }
            free -= 1;
            if def.stackable {
                stacks.push(id.to_string()); // a later same-id stacks on this new slot
            }
        }
        true
    }

    /// Remove `count` of `item_id`. Returns false (and changes nothing) if the
    /// player doesn't hold that many. Mirrors `consumeItem` (used to spend a token).
    pub fn consume_item(&mut self, item_id: &str, count: i64) -> bool {
        if self.count_item(item_id) < count {
            return false;
        }
        let mut remaining = count;
        for s in &mut self.bag {
            if remaining <= 0 {
                break;
            }
            if s.item_id.as_deref() != Some(item_id) {
                continue;
            }
            let take = s.count.min(remaining);
            s.count -= take;
            remaining -= take;
            if s.count <= 0 {
                s.clear();
            }
        }
        true
    }

    // ─── Quick-slots (derived views into the bag) ────────────────────

    /// Bag index of the next consumable feeding `kind`, or None. Mirrors `quickIndex`.
    fn quick_index(&self, kind: QuickKind) -> Option<usize> {
        self.bag.iter().position(|s| {
            let Some(id) = s.item_id.as_deref() else {
                return false;
            };
            match item_def(id) {
                Some(def) => def.kind == ItemKind::Consumable && def.quick_of() == kind,
                None => false,
            }
        })
    }

    /// Apply a consumable in bag slot `i`: returns the `ConsumeEffect` (heal+buff)
    /// for the caller to enact, then decrements the slot. None if `i` is empty or
    /// not a consumable. Mirrors `consumeConsumableAt` (effects returned, not run).
    fn consume_at(&mut self, i: usize) -> Option<ConsumeEffect> {
        let slot = self.bag.get(i)?;
        let id = slot.item_id.clone()?;
        let def = item_def(&id)?;
        if def.kind != ItemKind::Consumable {
            return None;
        }
        let effect = ConsumeEffect { heal: def.heal, buff: def.buff };
        let slot = &mut self.bag[i];
        slot.count -= 1;
        if slot.count <= 0 {
            slot.clear();
        }
        Some(effect)
    }

    /// Q: eat the next food. Returns its effect (heal+buff) for the caller, or None
    /// if no food is held. Mirrors `eatFood`.
    pub fn eat_food(&mut self) -> Option<ConsumeEffect> {
        let i = self.quick_index(QuickKind::Food)?;
        self.consume_at(i)
    }

    /// z/x/c: use the next item feeding the given buff slot. Returns its effect or
    /// None. Mirrors `activateBuff`.
    pub fn activate_buff(&mut self, kind: BuffKind) -> Option<ConsumeEffect> {
        let i = self.quick_index(QuickKind::Buff(kind))?;
        self.consume_at(i)
    }

    fn quick_slot(&self, kind: QuickKind) -> Option<QuickSlot> {
        let i = self.quick_index(kind)?;
        let slot = &self.bag[i];
        Some(QuickSlot {
            item_id: slot.item_id.clone().unwrap(),
            count: slot.count,
        })
    }

    /// The food Q would eat next (id + count), or None. Mirrors `getFoodSlot`.
    pub fn food_slot(&self) -> Option<QuickSlot> {
        self.quick_slot(QuickKind::Food)
    }

    /// The item the given buff key would use next, or None. Mirrors `getBuffSlot`.
    pub fn buff_slot(&self, kind: BuffKind) -> Option<QuickSlot> {
        self.quick_slot(QuickKind::Buff(kind))
    }

    // ─── Bindable quick-slots (Z / X / C) ────────────────────────────
    //
    // Q stays a pure derived view (`food_slot` / `eat_food`); these three slots add a
    // thin layer of *player intent* on top of the bag: a slot can be pinned to any
    // consumable id, and that pin persists across stack movement and depletion. An
    // unbound slot transparently falls back to the old derived-by-kind behaviour, so
    // the feature is invisible until a player chooses to use it.

    /// The item id pinned to bindable slot `slot` (0 = Z, 1 = X, 2 = C), or None.
    pub fn quick_bind(&self, slot: usize) -> Option<&str> {
        self.quick_binds.get(slot).and_then(|b| b.as_deref())
    }

    /// Pin consumable `id` to bindable slot `slot`. Only consumables (food *or* buff)
    /// may be bound. A bind is unique: if `id` is already pinned to a different slot,
    /// that slot is cleared first (so dragging an item onto a new key moves it rather
    /// than duplicating). Returns false (no change) for a bad slot or a non-consumable.
    pub fn set_quick_bind(&mut self, slot: usize, id: &str) -> bool {
        if slot >= QUICK_SLOTS {
            return false;
        }
        match item_def(id) {
            Some(d) if d.kind == ItemKind::Consumable => {}
            _ => return false,
        }
        for b in &mut self.quick_binds {
            if b.as_deref() == Some(id) {
                *b = None;
            }
        }
        self.quick_binds[slot] = Some(id.to_string());
        true
    }

    /// Clear bindable slot `slot` back to unbound (reverts to derived-by-kind).
    pub fn clear_quick_bind(&mut self, slot: usize) {
        if let Some(b) = self.quick_binds.get_mut(slot) {
            *b = None;
        }
    }

    /// Auto-pin a freshly added consumable to its default slot — but only when that
    /// slot is still empty and the item isn't already pinned somewhere. This gives a
    /// new player the legacy derived layout (resist→Z, power→X, haste→C) with zero
    /// setup while never stomping a manual bind. Food items target Q (the fixed slot),
    /// so they are skipped here. Called from `add` (the pickup path).
    pub fn auto_bind(&mut self, id: &str) {
        let Some(def) = item_def(id) else { return };
        if def.kind != ItemKind::Consumable {
            return;
        }
        let QuickKind::Buff(kind) = def.quick_of() else { return };
        let slot = match kind {
            BuffKind::Resist => 0,
            BuffKind::Power => 1,
            BuffKind::Haste => 2,
        };
        if self.quick_binds.iter().any(|b| b.as_deref() == Some(id)) {
            return; // already pinned to some slot — don't move it
        }
        if self.quick_binds[slot].is_none() {
            self.quick_binds[slot] = Some(id.to_string());
        }
    }

    /// Use bindable slot `slot` (Z / X / C): consume one of its pinned item and return
    /// the effect, or None when the slot is exhausted. An *unbound* slot falls back to
    /// the legacy "next item of the default kind" path, so the keys work before a
    /// player has set anything up.
    pub fn use_quick_slot(&mut self, slot: usize) -> Option<ConsumeEffect> {
        if slot >= QUICK_SLOTS {
            return None;
        }
        if let Some(id) = self.quick_binds[slot].clone() {
            let i = self.bag.iter().position(|s| s.item_id.as_deref() == Some(id.as_str()))?;
            return self.consume_at(i);
        }
        let kind = quick_default_kind(slot)?;
        self.activate_buff(kind)
    }

    /// The id + count a bindable slot shows on the HUD: the pinned id (count may be 0 →
    /// render greyed) when bound, else the derived next item of the default kind, else
    /// None → the caller draws the empty-slot ghost.
    pub fn quick_view(&self, slot: usize) -> Option<QuickSlot> {
        if slot >= QUICK_SLOTS {
            return None;
        }
        if let Some(id) = self.quick_binds[slot].as_deref() {
            return Some(QuickSlot { item_id: id.to_string(), count: self.count_item(id) });
        }
        let kind = quick_default_kind(slot)?;
        self.buff_slot(kind)
    }

    // ─── Equipping (driven by panel clicks) ──────────────────────────

    /// Equip the weapon/armor in bag slot `i`, swapping any current piece back into
    /// the bag. Mirrors `equipFromBag`.
    fn equip_from_bag(&mut self, i: usize, kind: ItemKind) {
        let Some(slot) = self.bag.get(i) else { return };
        let Some(id) = slot.item_id.clone() else {
            return;
        };
        let Some(def) = item_def(&id) else { return };
        // Free the bag slot first (gear is non-stackable), so it can receive the
        // previously equipped piece on a swap.
        let slot = &mut self.bag[i];
        slot.count -= 1;
        if slot.count <= 0 {
            slot.clear();
        }
        match kind {
            ItemKind::Weapon => {
                if let Some(prev) = self.equipped_id.take() {
                    self.place(&prev, 1);
                }
                self.equipped_id = Some(id);
                self.weapon_bonus = def.damage_bonus;
            }
            ItemKind::Armor => {
                if let Some(prev) = self.equipped_armor_id.take() {
                    self.place(&prev, 1);
                }
                self.equipped_armor_id = Some(id);
                self.armor_damage_mult = 1.0 - def.defense;
            }
            _ => {}
        }
    }

    /// Click a bag slot: eat a consumable (returns its effect), equip a
    /// weapon/armor (returns None), or no-op a token. Mirrors `activateBagItem`.
    pub fn activate_bag_item(&mut self, i: usize) -> Option<ConsumeEffect> {
        // `id` is owned + `def` is `&'static`, so neither borrows `self` — the
        // `&mut self` consume/equip calls below are free of a borrow conflict.
        let id = self.bag.get(i)?.item_id.clone()?;
        let def = item_def(&id)?;
        match def.kind {
            ItemKind::Consumable => self.consume_at(i),
            ItemKind::Weapon => {
                self.equip_from_bag(i, ItemKind::Weapon);
                None
            }
            ItemKind::Armor => {
                self.equip_from_bag(i, ItemKind::Armor);
                None
            }
            ItemKind::Token => None, // inert
        }
    }

    /// Take the equipped weapon off and return it to the bag (no-op if bag full).
    /// Mirrors `unequipWeapon`.
    pub fn unequip_weapon(&mut self) {
        let Some(id) = self.equipped_id.clone() else {
            return;
        };
        if !self.place(&id, 1) {
            return; // bag full → keep it equipped
        }
        self.equipped_id = None;
        self.weapon_bonus = 0.0;
    }

    /// Take the worn armor off and return it to the bag (no-op if bag full).
    /// Mirrors `unequipArmor`.
    pub fn unequip_armor(&mut self) {
        let Some(id) = self.equipped_armor_id.clone() else {
            return;
        };
        if !self.place(&id, 1) {
            return;
        }
        self.equipped_armor_id = None;
        self.armor_damage_mult = 1.0;
    }

    /// Equipped weapon's damage bonus (read by the swing path). Mirrors `getWeaponBonus`.
    pub fn weapon_bonus(&self) -> f64 {
        self.weapon_bonus
    }

    /// Incoming-damage multiplier from worn armor (read by `damage`). Mirrors
    /// `getArmorDamageMult`.
    pub fn armor_damage_mult(&self) -> f64 {
        self.armor_damage_mult
    }

    /// Full wipe to a fresh run. Mirrors `resetInventory` (the `open` flag is the
    /// ECS state, not stored here).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    // Port of src/world/inventoryStore.test.ts. The TS spied healPlayer/applyBuff
    // and asserted they fired; here `consume*` RETURNS the effect, so the same
    // assertions check the returned `ConsumeEffect` instead. The pickup-toast tests
    // become assertions on the `add`/`place` return value (toast fires on `true`),
    // and the open-flag test moves to the ECS layer (it's `AppState::Inventory`).
    use super::*;

    fn idx_of(bag: &Bag, id: &str) -> usize {
        bag.bag
            .iter()
            .position(|s| s.item_id.as_deref() == Some(id))
            .expect("item should be in the bag")
    }

    // ── consume_item (bag) ──
    #[test]
    fn consume_item_decrements_a_stack_and_reports_success() {
        let mut b = Bag::new();
        b.add("mercenary_contract", 2);
        assert!(b.consume_item("mercenary_contract", 1));
        assert!(b.has_item("mercenary_contract")); // one left
    }

    #[test]
    fn consume_item_frees_the_slot_when_last_consumed() {
        let mut b = Bag::new();
        b.add("mercenary_contract", 1);
        assert!(b.consume_item("mercenary_contract", 1));
        assert!(!b.has_item("mercenary_contract"));
        assert!(b.bag.iter().all(|s| s.item_id.is_none()));
    }

    #[test]
    fn consume_item_returns_false_when_absent() {
        let mut b = Bag::new();
        assert!(!b.consume_item("mercenary_contract", 1));
    }

    // ── eat_food (Q quick-slot) ──
    #[test]
    fn eat_food_eats_next_food_heals_and_decrements() {
        let mut b = Bag::new();
        b.add("bread", 2);
        let eff = b.eat_food().expect("ate bread");
        assert_eq!(eff.heal, 15.0);
        assert_eq!(
            b.food_slot(),
            Some(QuickSlot { item_id: "bread".into(), count: 1 })
        );
    }

    #[test]
    fn eat_food_auto_surfaces_next_food_once_a_stack_is_depleted() {
        let mut b = Bag::new();
        b.add("apple", 1);
        b.add("feast", 1);
        assert_eq!(b.food_slot().unwrap().item_id, "apple"); // first food
        b.eat_food(); // apple gone
        assert_eq!(
            b.food_slot(),
            Some(QuickSlot { item_id: "feast".into(), count: 1 })
        );
    }

    #[test]
    fn eat_food_also_applies_buff_on_dual_heal_buff_marsh_herb() {
        let mut b = Bag::new();
        b.add("marsh_herb", 1);
        let eff = b.eat_food().expect("ate marsh herb");
        assert_eq!(eff.heal, 30.0);
        let (kind, _, _) = eff.buff.expect("marsh herb grants a buff");
        assert_eq!(kind, BuffKind::Resist);
    }

    #[test]
    fn eat_food_returns_none_when_no_food_held() {
        let mut b = Bag::new();
        b.add("fur", 1); // a buff item, not food
        assert!(b.eat_food().is_none());
        assert!(b.food_slot().is_none());
    }

    // ── activate_buff (z/x/c) ──
    #[test]
    fn activate_buff_uses_matching_item_for_requested_kind() {
        let mut b = Bag::new();
        b.add("fur", 1); // resist
        let eff = b.activate_buff(BuffKind::Resist).expect("used fur");
        assert_eq!(eff.buff.unwrap().0, BuffKind::Resist);
        assert!(b.buff_slot(BuffKind::Resist).is_none());
    }

    #[test]
    fn activate_buff_does_not_pull_food_tagged_dual_item() {
        let mut b = Bag::new();
        b.add("marsh_herb", 1); // heals + resist, but tagged Food
        assert!(b.buff_slot(BuffKind::Resist).is_none());
        assert!(b.activate_buff(BuffKind::Resist).is_none());
        assert_eq!(b.food_slot().unwrap().item_id, "marsh_herb"); // belongs to food
    }

    #[test]
    fn activate_buff_returns_none_when_no_item_of_kind_held() {
        let mut b = Bag::new();
        assert!(b.activate_buff(BuffKind::Power).is_none());
    }

    // ── activate_bag_item (panel click) ──
    #[test]
    fn activate_bag_item_equips_a_weapon_and_removes_it_from_the_bag() {
        let mut b = Bag::new();
        b.add("sword_iron", 1);
        let i = idx_of(&b, "sword_iron");
        b.activate_bag_item(i);
        assert_eq!(b.equipped_id.as_deref(), Some("sword_iron"));
        assert_eq!(b.weapon_bonus(), 11.0);
        assert!(!b.has_item("sword_iron")); // moved into the equip slot
    }

    #[test]
    fn activate_bag_item_swaps_previously_equipped_weapon_back() {
        let mut b = Bag::new();
        b.add("sword_iron", 1);
        b.add("axe", 1);
        b.activate_bag_item(idx_of(&b, "sword_iron"));
        b.activate_bag_item(idx_of(&b, "axe"));
        assert_eq!(b.equipped_id.as_deref(), Some("axe"));
        assert!(b.has_item("sword_iron")); // returned to the bag
    }

    #[test]
    fn activate_bag_item_equips_armor_and_applies_damage_reduction() {
        let mut b = Bag::new();
        b.add("leather_armor", 1);
        b.activate_bag_item(idx_of(&b, "leather_armor"));
        assert_eq!(b.equipped_armor_id.as_deref(), Some("leather_armor"));
        assert!((b.armor_damage_mult() - 0.89).abs() < 1e-9); // 1 - 0.11
    }

    #[test]
    fn activate_bag_item_eats_a_consumable_when_clicked() {
        let mut b = Bag::new();
        b.add("bread", 1);
        let eff = b.activate_bag_item(idx_of(&b, "bread")).expect("ate bread");
        assert_eq!(eff.heal, 15.0);
        assert!(!b.has_item("bread"));
    }

    #[test]
    fn activate_bag_item_leaves_a_token_inert() {
        let mut b = Bag::new();
        b.add("mercenary_contract", 1);
        let i = idx_of(&b, "mercenary_contract");
        assert!(b.activate_bag_item(i).is_none());
        assert_eq!(b.bag[i].count, 1);
        assert!(b.equipped_id.is_none());
    }

    // ── unequip returns gear to the bag ──
    #[test]
    fn unequip_weapon_clears_the_bonus_and_returns_it() {
        let mut b = Bag::new();
        b.add("sword_iron", 1);
        b.activate_bag_item(idx_of(&b, "sword_iron"));
        b.unequip_weapon();
        assert!(b.equipped_id.is_none());
        assert_eq!(b.weapon_bonus(), 0.0);
        assert!(b.has_item("sword_iron"));
    }

    #[test]
    fn unequip_armor_restores_full_damage_taken() {
        let mut b = Bag::new();
        b.add("leather_armor", 1);
        b.activate_bag_item(idx_of(&b, "leather_armor"));
        b.unequip_armor();
        assert!(b.equipped_armor_id.is_none());
        assert_eq!(b.armor_damage_mult(), 1.0);
        assert!(b.has_item("leather_armor"));
    }

    // ── pickup signalling (the TS toast tests) ──
    #[test]
    fn add_signals_a_toast_on_a_genuine_pickup() {
        let mut b = Bag::new();
        assert!(b.add("bread", 1)); // true → the ECS layer fires the toast
    }

    #[test]
    fn place_returns_a_gear_to_the_bag_silently() {
        // The TS "no toast on swap/return" invariant: equipping then unequipping a
        // weapon routes the return through `place` (silent), so only the original
        // `add` would have toasted. Here we assert the bag count is restored without
        // a second toast-signalling `add` ever being called.
        let mut b = Bag::new();
        assert!(b.add("sword_iron", 1)); // one genuine pickup
        b.activate_bag_item(idx_of(&b, "sword_iron")); // equip (uses place to swap)
        b.unequip_weapon(); // returns via place — silent
        assert_eq!(b.count_item("sword_iron"), 1);
        assert!(b.equipped_id.is_none());
    }

    // ── has_room_for ──
    #[test]
    fn has_room_for_merges_stackables_and_counts_free_slots() {
        // A stackable already present needs no new slot — even both copies merge into the stack.
        let mut b = Bag::new();
        b.place("bread", 1);
        assert!(b.has_room_for(&["bread", "bread"]));

        // Every slot occupied by a non-mergeable item → a brand-new id has nowhere to go, but an id
        // whose stack is already present still fits (merge, no slot needed).
        let mut full = Bag::new();
        for s in full.bag.iter_mut() {
            s.item_id = Some("leather_armor".to_string());
            s.count = 1;
        }
        assert!(!full.has_room_for(&["sword_iron"]));
        assert!(full.has_room_for(&["leather_armor"]));
    }

    #[test]
    fn coalesce_merges_duplicate_stackable_slots() {
        let mut b = Bag::new();
        // Two separate dragon_plate cells (as a legacy bag would have before gear stacked).
        b.bag[0].item_id = Some("dragon_plate".to_string());
        b.bag[0].count = 1;
        b.bag[3].item_id = Some("dragon_plate".to_string());
        b.bag[3].count = 1;
        b.bag[5].item_id = Some("bread".to_string());
        b.bag[5].count = 2;
        b.coalesce();
        assert_eq!(b.count_item("dragon_plate"), 2);
        // Collapsed into a single cell.
        assert_eq!(b.bag.iter().filter(|s| s.item_id.as_deref() == Some("dragon_plate")).count(), 1);
        assert_eq!(b.count_item("bread"), 2); // untouched single stack
    }

    // ── stat lines ──
    #[test]
    fn stat_line_formats_per_kind() {
        assert_eq!(item_def("sword_iron").unwrap().stat_line(), "+11 attack");
        assert_eq!(item_def("leather_armor").unwrap().stat_line(), "-11% damage taken");
        assert_eq!(item_def("bread").unwrap().stat_line(), "+15 HP");
        assert_eq!(item_def("mercenary_contract").unwrap().stat_line(), "Key item");
        assert_eq!(item_def("fur").unwrap().stat_line(), "Resist 12s");
        assert_eq!(item_def("marsh_herb").unwrap().stat_line(), "+30 HP, Resist 14s");
    }

    // ── icon spec (the procedural-icon recipe the game crate rasterises) ──
    #[test]
    fn icon_spec_maps_bespoke_items_and_falls_back_by_kind() {
        // Bespoke recipes: the apple is a red round fruit with a green-leaf accent,
        // the herb a green sprig, the contract a tan scroll.
        let apple = item_def("apple").unwrap().icon_spec();
        assert_eq!(apple.shape, IconShape::Apple);
        assert!(apple.fg.0 > apple.fg.1 && apple.fg.0 > apple.fg.2, "apple body reads red");
        assert!(apple.accent.1 > apple.accent.0, "apple leaf reads green");
        assert_eq!(item_def("marsh_herb").unwrap().icon_spec().shape, IconShape::Herb);
        assert_eq!(item_def("croc_steak").unwrap().icon_spec().shape, IconShape::Meat);
        assert_eq!(item_def("potion").unwrap().icon_spec().shape, IconShape::Potion);
        assert_eq!(item_def("mercenary_contract").unwrap().icon_spec().shape, IconShape::Scroll);

        // Kind fallbacks: an un-bespoke weapon → a blade, armor → a shield, a plain
        // healer → a food round, a buff item → a tinted orb.
        assert_eq!(item_def("axe").unwrap().icon_spec().shape, IconShape::Blade);
        assert_eq!(item_def("leather_armor").unwrap().icon_spec().shape, IconShape::Shield);
        assert_eq!(item_def("bread").unwrap().icon_spec().shape, IconShape::Food);
        // `fur` is a buff consumable → an orb (bespoke), but the generic-buff fallback
        // also yields an orb; assert the kind-level path via a buff item without a
        // bespoke entry would be Orb too. (All buff items here are bespoke, so check
        // the bespoke ones read as orbs.)
        assert_eq!(item_def("fur").unwrap().icon_spec().shape, IconShape::Orb);

        // Every defined item resolves to *some* spec (no panic / missing arm).
        for d in ITEM_DEFS {
            let _ = d.icon_spec();
        }
    }

    #[test]
    fn sell_value_is_under_buy_price_and_zero_for_tokens() {
        assert_eq!(sell_value("mercenary_contract"), 0); // key item — not sellable
        assert!(sell_value("bread") < 4); // shop sells bread for 4
        assert!(sell_value("potion") < 12);
        assert!(sell_value("feast") < 28);
        assert!(sell_value("axe") < 45);
        assert!(sell_value("sword_gold") < 80);
        assert!(sell_value("sword_iron") > 0); // unbuyable gear still has a sell-back value
        assert!(sell_value("fur") > 0); // a buff consumable is worth a little
        assert_eq!(sell_value("nope"), 0); // unknown id
    }

    #[test]
    fn reset_wipes_to_a_fresh_bag() {
        let mut b = Bag::new();
        b.add("sword_iron", 1);
        b.activate_bag_item(idx_of(&b, "sword_iron"));
        b.add("bread", 5);
        b.add("fur", 1); // pins Z
        b.reset();
        assert!(b.bag.iter().all(|s| s.item_id.is_none()));
        assert!(b.equipped_id.is_none());
        assert_eq!(b.weapon_bonus(), 0.0);
        assert_eq!(b.armor_damage_mult(), 1.0);
        assert_eq!(b.quick_binds, [None, None, None]);
    }

    // ── Bindable quick-slots (Z / X / C) ──
    #[test]
    fn auto_bind_pins_buff_items_to_their_default_slot_on_pickup() {
        let mut b = Bag::new();
        b.add("fur", 1); // resist → Z (slot 0)
        b.add("venom", 1); // power → X (slot 1)
        b.add("goat_charm", 1); // haste → C (slot 2)
        assert_eq!(b.quick_bind(0), Some("fur"));
        assert_eq!(b.quick_bind(1), Some("venom"));
        assert_eq!(b.quick_bind(2), Some("goat_charm"));
    }

    #[test]
    fn auto_bind_ignores_food_items_food_belongs_to_q() {
        let mut b = Bag::new();
        b.add("bread", 1);
        b.add("marsh_herb", 1); // heals + resist, but tagged Food → Q, not Z
        assert_eq!(b.quick_binds, [None, None, None]);
    }

    #[test]
    fn auto_bind_never_overrides_a_manual_bind() {
        let mut b = Bag::new();
        assert!(b.set_quick_bind(0, "potion")); // pin a heal to Z by hand
        b.add("fur", 1); // resist would target Z, but it's taken
        assert_eq!(b.quick_bind(0), Some("potion"));
        // fur found no empty default slot, so it's left unbound (Q/derive still works).
        assert!(b.quick_binds.iter().all(|x| x.as_deref() != Some("fur")));
    }

    #[test]
    fn set_quick_bind_is_unique_rebinding_moves_it() {
        let mut b = Bag::new();
        assert!(b.set_quick_bind(0, "fur"));
        assert!(b.set_quick_bind(1, "fur")); // same id to a second slot
        assert_eq!(b.quick_bind(0), None); // moved off the first
        assert_eq!(b.quick_bind(1), Some("fur"));
    }

    #[test]
    fn set_quick_bind_rejects_non_consumables() {
        let mut b = Bag::new();
        assert!(!b.set_quick_bind(0, "sword_iron"));
        assert!(!b.set_quick_bind(0, "leather_armor"));
        assert!(!b.set_quick_bind(0, "mercenary_contract"));
        assert!(!b.set_quick_bind(9, "fur")); // out-of-range slot
        assert_eq!(b.quick_binds, [None, None, None]);
    }

    #[test]
    fn use_quick_slot_consumes_the_pinned_item() {
        let mut b = Bag::new();
        b.add("fur", 2); // auto-pins Z
        let eff = b.use_quick_slot(0).expect("used fur");
        assert_eq!(eff.buff.unwrap().0, BuffKind::Resist);
        assert_eq!(b.count_item("fur"), 1);
    }

    #[test]
    fn use_quick_slot_keeps_the_bind_when_the_stack_is_exhausted() {
        let mut b = Bag::new();
        b.add("fur", 1); // auto-pins Z
        assert!(b.use_quick_slot(0).is_some()); // last fur consumed
        assert_eq!(b.quick_bind(0), Some("fur")); // bind survives at count 0
        assert!(b.use_quick_slot(0).is_none()); // nothing left to use → no-op
    }

    #[test]
    fn use_quick_slot_can_fire_a_pinned_food_item() {
        let mut b = Bag::new();
        b.add("bread", 1); // food: auto-bind skips it
        assert!(b.set_quick_bind(1, "bread")); // but a player can pin it to X
        let eff = b.use_quick_slot(1).expect("ate bread from X");
        assert_eq!(eff.heal, 15.0);
        assert!(!b.has_item("bread"));
    }

    #[test]
    fn use_quick_slot_unbound_falls_back_to_derived_kind() {
        let mut b = Bag::new();
        b.clear_quick_bind(0); // explicitly unbound Z
        b.place("fur", 1); // bypass `add`'s auto-bind so the slot stays unbound
        assert_eq!(b.quick_bind(0), None);
        let eff = b.use_quick_slot(0).expect("derived resist item");
        assert_eq!(eff.buff.unwrap().0, BuffKind::Resist);
    }

    #[test]
    fn quick_view_reports_pinned_id_and_count_for_the_hud() {
        let mut b = Bag::new();
        b.add("fur", 3); // pins Z, count 3
        assert_eq!(b.quick_view(0), Some(QuickSlot { item_id: "fur".into(), count: 3 }));
        // Depleted but still pinned → count 0 (HUD greys it, keeps the icon).
        b.consume_item("fur", 3);
        assert_eq!(b.quick_view(0), Some(QuickSlot { item_id: "fur".into(), count: 0 }));
        // Truly unbound with nothing of its kind → None (HUD draws the ghost).
        assert_eq!(b.quick_view(1), None);
    }

    #[test]
    fn quick_default_kind_maps_zxc_and_rejects_others() {
        assert_eq!(quick_default_kind(0), Some(BuffKind::Resist));
        assert_eq!(quick_default_kind(1), Some(BuffKind::Power));
        assert_eq!(quick_default_kind(2), Some(BuffKind::Haste));
        assert_eq!(quick_default_kind(3), None);
    }
}
