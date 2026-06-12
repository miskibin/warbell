//! Port of src/world/upgradeStore.ts — the click-driven upgrade tree.
//!
//! In the TS game each node owns an `apply()` closure that reaches into the
//! player/city/shop/castle/tower/weapon-unlock stores directly. Those cross-store
//! side effects are render/ECS concerns, so per the parity brief they are NOT run
//! here: instead every node carries a typed [`UpgradeEffect`] describing WHAT it
//! does, and the ECS layer (crate `tileworld_bevy`) enacts it against the live
//! Bevy resources. This keeps the core crate Bevy-free and unit-testable — the
//! catalog (ids, costs incl. stone, prereqs, branch, effect) plus the
//! purchase/affordability/gating rules are the parity surface.
//!
//! Cost scaling: the TS rescales every node's gold cost once at module load
//! (`Math.round(cost * 1.6 / 5) * 5`) so the tree isn't out-bought in a few
//! nights. The base costs below are the *pre-scale* numbers from the TS literal;
//! [`UpgradeNode::cost`] applies the same scale on read, so `cost()`, `can_buy`,
//! and the HUD all see the scaled value consistently (the TS mutated the array;
//! a pure accessor is the dep-free equivalent). Stone costs are NOT scaled (the
//! TS only scaled gold).

/// The four expansion charters (branches), mirroring the TS `UpgradeBranch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeBranch {
    Economy,
    Defense,
    Hero,
    Arsenal,
}

/// The typed effect a node applies when bought. The ECS layer matches on this to
/// mutate the right live resource — the dep-free stand-in for the TS `apply()`
/// closures (which called `bumpMaxHp`, `setWallsBuilt`, `unlockWeapon`, …).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UpgradeEffect {
    // ── Economy ───────────────────────────────────────────────────────────────
    // Food + population are owned by the town city-building layer (`town_store` /
    // `src/town.rs`) — build Farm/House plots there. The old `BuildHouses`/`Farm`
    // tree nodes that duplicated that have been removed.
    /// Bounty: ork-kill gold ×`mult` (sets `Player.bounty_mult`).
    Bounty(f64),
    /// Tax office: doubles the population tithe paid each cleared night (one-shot flag;
    /// the base tithe is `town_store::TITHE_GOLD_PER_POP` per villager).
    TaxOffice,
    /// Merchant Guild: shop price multiplier `mult` (e.g. 0.8 = −20%).
    MerchantGuild(f64),

    // ── Defense (structural — recorded in a shared Defenses resource) ──────────
    Walls,
    Gate,
    Towers,
    TowerMastery,
    KeepArchers,
    /// Reinforced keep: raises keep max HP + slow self-repair.
    ReinforceKeep,
    /// Town-guard arms tier: villagers hit harder / watch wider (bumps a tier).
    VillagerArmor,
    Ballista,
    HealingShrine,

    // ── Hero (player combat — sets fields on `Player`) ─────────────────────────
    /// +`n` max HP.
    MaxHp(f64),
    /// +`n` attack damage.
    AttackDamage(f64),
    /// Crit chance (sets `Player.crit_chance`).
    Crit(f64),
    /// Lifesteal: heal `n` HP per kill (sets `Player.lifesteal`).
    Lifesteal(f64),
    /// Move-speed multiplier (sets `Player.move_speed_mult`).
    MoveSpeed(f64),
    /// Cleave: splash fraction to nearby orks (sets `Player.cleave`).
    Cleave(f64),

    // ── Arsenal (unlock a shop weapon by id) ──────────────────────────────────
    UnlockWeapon(&'static str),
}

/// One upgrade node: id, branch, presentation (name/desc/icon), the base gold cost
/// (pre-scale), the stone cost (defense structures only), an optional prereq id,
/// and the typed effect. Mirrors the TS `UpgradeNode` minus the `apply()` closure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UpgradeNode {
    pub id: &'static str,
    pub branch: UpgradeBranch,
    pub name: &'static str,
    pub desc: &'static str,
    pub icon: &'static str,
    /// Base (pre-scale) gold cost. Use [`UpgradeNode::cost`] for the live value.
    pub base_cost: i64,
    /// Stone required alongside gold (0 = none). Not scaled.
    pub stone_cost: i64,
    /// Must be purchased before this node unlocks (None = a root node).
    pub prereq_id: Option<&'static str>,
    pub effect: UpgradeEffect,
}

/// The gold-cost scale applied once to the whole tree in TS (`UPGRADE_COST_SCALE`).
pub const UPGRADE_COST_SCALE: f64 = 1.6;

/// JS `Math.round`: round half AWAY from zero toward +∞ for ties (`.5` → up).
/// All our values are positive, so this is just "round half up".
fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

impl UpgradeNode {
    /// The live gold cost: `round(base * 1.6 / 5) * 5` (the TS one-time rescale,
    /// applied on read so the data stays const).
    pub fn cost(&self) -> i64 {
        (js_round(self.base_cost as f64 * UPGRADE_COST_SCALE / 5.0) * 5.0) as i64
    }
}

/// Builder so the table reads like the TS object literals. (A const fn for a struct
/// with this many fields naturally takes one arg per field.)
#[allow(clippy::too_many_arguments)]
const fn node(
    id: &'static str,
    branch: UpgradeBranch,
    name: &'static str,
    desc: &'static str,
    icon: &'static str,
    base_cost: i64,
    stone_cost: i64,
    prereq_id: Option<&'static str>,
    effect: UpgradeEffect,
) -> UpgradeNode {
    UpgradeNode { id, branch, name, desc, icon, base_cost, stone_cost, prereq_id, effect }
}

use UpgradeBranch::*;
use UpgradeEffect::*;

/// The full upgrade tree (port of `UPGRADE_NODES`, same order). Base costs are the
/// pre-scale TS literals; stone costs match the TS `stoneCost`.
pub static UPGRADE_NODES: &[UpgradeNode] = &[
    // ── Economy: gold & trade ─────────────────────────────────────────────────
    // (Food/population growth lives in the town city-building layer — build Farm
    // and House plots in the suburb; the tree no longer duplicates them.)
    node("eco_bounty", Economy, "Bounty",
        "+50% gold from every ork you slay — reach the costly upgrades sooner.",
        "💰", 60, 0, None, Bounty(1.5)),
    node("eco_tax_office", Economy, "Tax Office",
        "Doubles the dawn tithe — every villager pays twice the gold each night you survive.",
        "🏛️", 75, 0, None, TaxOffice),
    node("eco_merchant_guild", Economy, "Merchant Guild",
        "−20% on everything the wandering merchant sells.",
        "⚖️", 70, 0, None, MerchantGuild(0.8)),

    // ── Defense: fortify the city ─────────────────────────────────────────────
    node("def_walls", Defense, "Palisade Walls",
        "Ring the town in timber walls — orks must funnel to the gates instead of swarming in.",
        "🧱", 50, 20, None, Walls),
    node("def_gate", Defense, "Gatehouse",
        "Fortified gates on all four walls: your folk pass freely while orks pile up outside.",
        "🚪", 35, 10, Some("def_walls"), Gate),
    node("def_towers", Defense, "Watchtowers",
        "Four corner towers that auto-fire arrows at any ork in range.",
        "🗼", 80, 25, Some("def_walls"), Towers),
    node("def_tower_mastery", Defense, "Tower Mastery",
        "Watchtowers fire faster, farther, and hit harder.",
        "🎯", 120, 0, Some("def_towers"), TowerMastery),
    node("def_keep_archers", Defense, "Keep Archers",
        "Bowmen on the keep roof rain arrows down on the courtyard all night.",
        "🏹", 100, 0, None, KeepArchers),
    node("def_reinforce", Defense, "Reinforced Keep",
        "Greatly raises keep HP, and the keep slowly self-repairs between waves.",
        "🏰", 130, 30, None, ReinforceKeep),
    node("def_armor_1", Defense, "Town Guard Arms",
        "Arm the townsfolk: guards hit far harder (16 dmg) and chase orks from farther.",
        "🪖", 40, 0, None, VillagerArmor),
    node("def_armor_2", Defense, "Veteran Guard",
        "Steel arms and drilling: guards hit harder still (23 dmg) and hold a wider watch.",
        "🛡️", 90, 0, Some("def_armor_1"), VillagerArmor),
    node("def_ballista", Defense, "Ballista",
        "A heavy bolt-thrower at the north gate: long range, big single hits.",
        "🎱", 110, 0, None, Ballista),
    node("def_shrine", Defense, "Healing Shrine",
        "A shrine that steadily heals the hero while you stay inside the walls.",
        "⛲", 95, 0, None, HealingShrine),

    // ── Hero: the player knight ───────────────────────────────────────────────
    node("hero_hp_1", Hero, "Vigor", "+18 max HP.", "❤️", 30, 0, None, MaxHp(18.0)),
    node("hero_hp_2", Hero, "Greater Vigor", "+35 max HP.", "💗", 70, 0, Some("hero_hp_1"), MaxHp(35.0)),
    node("hero_dmg_1", Hero, "Sharpened Blade", "+4 attack damage.", "⚔️", 30, 0, None, AttackDamage(4.0)),
    node("hero_dmg_2", Hero, "Honed Edge", "+7 attack damage.", "🗡️", 70, 0, Some("hero_dmg_1"), AttackDamage(7.0)),
    node("hero_crit", Hero, "Crit Strike",
        "14% chance for a swing to deal double damage.",
        "💥", 80, 0, Some("hero_dmg_1"), Crit(0.14)),
    node("hero_lifesteal", Hero, "Lifesteal",
        "Heal 7 HP every time you slay an ork.",
        "🩸", 90, 0, Some("hero_hp_1"), Lifesteal(7.0)),
    node("hero_swift", Hero, "Swift Boots", "Move 13% faster.", "👢", 60, 0, None, MoveSpeed(1.13)),
    node("hero_cleave", Hero, "Cleave",
        "Strikes splash 21% damage to orks beside your target.",
        "🌀", 110, 0, Some("hero_dmg_2"), Cleave(0.21)),

    // ── Arsenal: unlock shop weapons ──────────────────────────────────────────
    node("ars_axe", Arsenal, "Unlock Battle Axe",
        "Stocks the Battle Axe (+15 attack) at the wandering merchant.",
        "🪓", 50, 0, None, UnlockWeapon("axe")),
    node("ars_sword", Arsenal, "Unlock Golden Blade",
        "Stocks the Golden Blade (+21 attack) at the wandering merchant.",
        "🌟", 90, 0, Some("ars_axe"), UnlockWeapon("sword_gold")),
];

/// Resolve a node id to its definition (the TS `UPGRADE_NODES.find`).
pub fn node_by_id(id: &str) -> Option<&'static UpgradeNode> {
    UPGRADE_NODES.iter().find(|n| n.id == id)
}

/// The set of purchased upgrade ids + the gating/affordability rules. Mirrors the
/// TS `purchasedIds` Set plus `isPurchased` / `canBuy` / `purchase`, but the EFFECT
/// is NOT applied here (the ECS layer reads [`PurchaseOutcome::effect`] and enacts
/// it). A plain struct (no global) so tests run a fresh instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpgradeState {
    purchased: Vec<&'static str>,
}

/// What a successful [`UpgradeState::purchase`] yields: the gold + stone to deduct
/// and the effect for the caller to enact. (`canBuy` already verified affordability,
/// so the caller deducts unconditionally — matching the TS `payCosts`/`spendGold`.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PurchaseOutcome {
    pub gold_cost: i64,
    pub stone_cost: i64,
    pub effect: UpgradeEffect,
}

impl UpgradeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_purchased(&self, id: &str) -> bool {
        self.purchased.contains(&id)
    }

    /// Live view of purchased ids (the HUD greys out owned nodes).
    pub fn purchased(&self) -> &[&'static str] {
        &self.purchased
    }

    /// Whether a node is buyable now: not owned, prereq met, and affordable in
    /// gold + stone. `unlimited` (the debug cheat) skips the affordability check
    /// (but never the owned/prereq gate). Mirrors `canBuy`.
    pub fn can_buy(&self, node: &UpgradeNode, gold: i64, stone: i64, unlimited: bool) -> bool {
        if self.is_purchased(node.id) {
            return false;
        }
        if let Some(req) = node.prereq_id
            && !self.is_purchased(req)
        {
            return false;
        }
        if unlimited {
            return true;
        }
        if node.stone_cost > 0 && stone < node.stone_cost {
            return false;
        }
        gold >= node.cost()
    }

    /// Attempt to purchase `node`. On success marks it owned and returns the costs
    /// to deduct + the effect to enact; returns None when not buyable. Mirrors
    /// `purchase` (minus running the effect — the caller does that).
    pub fn purchase(
        &mut self,
        node: &UpgradeNode,
        gold: i64,
        stone: i64,
        unlimited: bool,
    ) -> Option<PurchaseOutcome> {
        if !self.can_buy(node, gold, stone, unlimited) {
            return None;
        }
        self.purchased.push(node.id);
        Some(PurchaseOutcome {
            gold_cost: node.cost(),
            stone_cost: node.stone_cost,
            effect: node.effect,
        })
    }

    /// Wipe purchase records for a fresh run. Mirrors `resetUpgrades` (the Merchant
    /// Guild discount lives in the shop catalog state, reset there).
    pub fn reset(&mut self) {
        self.purchased.clear();
    }

    /// Rebuild a state from saved purchased ids (a loaded game). Each id is resolved
    /// back to its `&'static` catalog id via [`node_by_id`]; unknown ids (a node
    /// removed since the save) are skipped. This is the load counterpart of
    /// [`purchased`](Self::purchased) — `purchased` can't be serde-`Deserialize`d
    /// because the ids are `&'static str`, so the save stores owned `String`s and
    /// this re-interns them. The caller re-enacts the effects against live state.
    pub fn restore<S: AsRef<str>>(ids: &[S]) -> Self {
        let mut purchased = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(node) = node_by_id(id.as_ref())
                && !purchased.contains(&node.id)
            {
                purchased.push(node.id);
            }
        }
        Self { purchased }
    }
}

#[cfg(test)]
mod tests {
    // Mirrors the upgradeStore behaviour the brief calls the parity gate: cost
    // scaling, prerequisite gating, gold+stone affordability, and the typed effect.
    use super::*;

    #[test]
    fn every_node_id_is_unique() {
        let mut ids: Vec<&str> = UPGRADE_NODES.iter().map(|n| n.id).collect();
        ids.sort_unstable();
        let len = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), len, "duplicate upgrade id");
    }

    #[test]
    fn every_prereq_resolves_to_a_real_node() {
        for n in UPGRADE_NODES {
            if let Some(req) = n.prereq_id {
                assert!(node_by_id(req).is_some(), "{} has dangling prereq {req}", n.id);
            }
        }
    }

    #[test]
    fn cost_applies_the_ts_scale_and_rounds_to_five() {
        // round(75 * 1.6 / 5) * 5 = round(24) * 5 = 24 * 5 = 120.
        assert_eq!(node_by_id("eco_tax_office").unwrap().cost(), 120);
        // round(50 * 1.6 / 5) * 5 = round(16) * 5 = 80.
        assert_eq!(node_by_id("def_walls").unwrap().cost(), 80);
        // round(130 * 1.6 / 5) * 5 = round(41.6) * 5 = 42 * 5 = 210.
        assert_eq!(node_by_id("def_reinforce").unwrap().cost(), 210);
        // round(60 * 1.6 / 5) * 5 = round(19.2) * 5 = 19 * 5 = 95.
        assert_eq!(node_by_id("eco_bounty").unwrap().cost(), 95);
    }

    #[test]
    fn stone_costs_are_not_scaled_and_match_ts() {
        assert_eq!(node_by_id("def_walls").unwrap().stone_cost, 20);
        assert_eq!(node_by_id("def_gate").unwrap().stone_cost, 10);
        assert_eq!(node_by_id("def_towers").unwrap().stone_cost, 25);
        assert_eq!(node_by_id("def_reinforce").unwrap().stone_cost, 30);
        // Non-defense nodes need no stone.
        assert_eq!(node_by_id("hero_hp_1").unwrap().stone_cost, 0);
        assert_eq!(node_by_id("eco_bounty").unwrap().stone_cost, 0);
    }

    #[test]
    fn cannot_buy_a_node_whose_prereq_is_unmet() {
        let st = UpgradeState::new();
        let gate = node_by_id("def_gate").unwrap();
        // Plenty of gold + stone, but walls aren't built yet.
        assert!(!st.can_buy(gate, 9999, 9999, false));
    }

    #[test]
    fn buying_a_prereq_unlocks_the_child() {
        let mut st = UpgradeState::new();
        let walls = node_by_id("def_walls").unwrap();
        let gate = node_by_id("def_gate").unwrap();
        assert!(st.purchase(walls, 9999, 9999, false).is_some());
        assert!(st.can_buy(gate, 9999, 9999, false));
    }

    #[test]
    fn cannot_afford_without_enough_gold() {
        let st = UpgradeState::new();
        let n = node_by_id("hero_hp_1").unwrap(); // cost 50 after scale
        assert_eq!(n.cost(), 50);
        assert!(!st.can_buy(n, 49, 0, false));
        assert!(st.can_buy(n, 50, 0, false));
    }

    #[test]
    fn defense_node_needs_both_gold_and_stone() {
        let st = UpgradeState::new();
        let walls = node_by_id("def_walls").unwrap(); // 80 gold + 20 stone
        assert!(!st.can_buy(walls, 80, 19, false), "short on stone");
        assert!(!st.can_buy(walls, 79, 20, false), "short on gold");
        assert!(st.can_buy(walls, 80, 20, false));
    }

    #[test]
    fn unlimited_money_buys_anything_unlocked_for_free() {
        let st = UpgradeState::new();
        let walls = node_by_id("def_walls").unwrap();
        assert!(st.can_buy(walls, 0, 0, true)); // free under the cheat
        // …but still respects the prereq gate.
        let gate = node_by_id("def_gate").unwrap();
        assert!(!st.can_buy(gate, 0, 0, true));
    }

    #[test]
    fn purchase_returns_the_costs_and_effect_then_marks_owned() {
        let mut st = UpgradeState::new();
        let walls = node_by_id("def_walls").unwrap();
        let out = st.purchase(walls, 100, 50, false).expect("buyable");
        assert_eq!(out.gold_cost, 80);
        assert_eq!(out.stone_cost, 20);
        assert_eq!(out.effect, UpgradeEffect::Walls);
        assert!(st.is_purchased("def_walls"));
        // Can't buy the same node twice.
        assert!(st.purchase(walls, 100, 50, false).is_none());
    }

    #[test]
    fn hero_effects_carry_the_ts_magnitudes() {
        assert_eq!(node_by_id("hero_hp_1").unwrap().effect, UpgradeEffect::MaxHp(18.0));
        assert_eq!(node_by_id("hero_dmg_2").unwrap().effect, UpgradeEffect::AttackDamage(7.0));
        assert_eq!(node_by_id("hero_crit").unwrap().effect, UpgradeEffect::Crit(0.14));
        assert_eq!(node_by_id("hero_lifesteal").unwrap().effect, UpgradeEffect::Lifesteal(7.0));
        assert_eq!(node_by_id("hero_swift").unwrap().effect, UpgradeEffect::MoveSpeed(1.13));
        assert_eq!(node_by_id("hero_cleave").unwrap().effect, UpgradeEffect::Cleave(0.21));
        assert_eq!(node_by_id("eco_bounty").unwrap().effect, UpgradeEffect::Bounty(1.5));
    }

    #[test]
    fn arsenal_nodes_unlock_the_right_weapon_ids() {
        assert_eq!(node_by_id("ars_axe").unwrap().effect, UpgradeEffect::UnlockWeapon("axe"));
        assert_eq!(
            node_by_id("ars_sword").unwrap().effect,
            UpgradeEffect::UnlockWeapon("sword_gold")
        );
    }

    #[test]
    fn reset_clears_purchase_records() {
        let mut st = UpgradeState::new();
        st.purchase(node_by_id("hero_hp_1").unwrap(), 9999, 0, false);
        assert!(st.is_purchased("hero_hp_1"));
        st.reset();
        assert!(!st.is_purchased("hero_hp_1"));
        assert!(st.purchased().is_empty());
    }

    #[test]
    fn restore_round_trips_purchased_ids_and_skips_unknown() {
        let mut st = UpgradeState::new();
        st.purchase(node_by_id("hero_hp_1").unwrap(), 9999, 0, false);
        st.purchase(node_by_id("def_walls").unwrap(), 9999, 9999, false);
        // Save = the owned ids as Strings (what the save file holds).
        let saved: Vec<String> = st.purchased().iter().map(|s| s.to_string()).collect();
        let mut restored = UpgradeState::restore(&saved);
        assert_eq!(restored, st);
        // A stale id from an older save is ignored, not fatal.
        restored = UpgradeState::restore(&["hero_hp_1".to_string(), "no_such_node".to_string()]);
        assert!(restored.is_purchased("hero_hp_1"));
        assert_eq!(restored.purchased().len(), 1);
    }
}
