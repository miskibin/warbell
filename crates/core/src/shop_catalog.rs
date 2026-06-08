//! Port of src/world/shopCatalog.ts + the price/discount half of shopStore.ts —
//! the merchant's goods + the Merchant-Guild discount math.
//!
//! The TS `ShopItem.apply()` closures (spend gold → add to bag, refund on a full
//! bag) are ECS/store side effects, so per the parity brief they are NOT run here.
//! This module is the pure data + price math: the static consumable catalog, the
//! weapon catalog the Arsenal branch unlocks, and `discounted_price`. The ECS
//! layer builds the live item list (`build_shop_items`) from the unlocked-weapon
//! set, shows `discounted_price`, and does the buy (deduct gold + `Bag::add`).

/// One purchasable line in the shop (the TS `ShopItem` minus its `apply` closure).
/// `price` is the BASE (pre-discount) price; the discount is applied at display +
/// charge time via [`discounted_price`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShopItem {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub price: i64,
}

/// The base consumables, always for sale (port of `SHOP_ITEMS`).
pub static SHOP_ITEMS: &[ShopItem] = &[
    ShopItem { id: "bread", name: "Bread", icon: "🍞", price: 4 },
    ShopItem { id: "potion", name: "Health Potion", icon: "🧪", price: 12 },
    ShopItem { id: "feast", name: "Tavern Feast", icon: "🍖", price: 28 },
];

/// Weapons the Arsenal upgrade branch can unlock for sale (port of `WEAPON_CATALOG`).
/// Added to the live list once their id is in the unlocked set.
pub static WEAPON_CATALOG: &[ShopItem] = &[
    ShopItem { id: "axe", name: "Battle Axe", icon: "🪓", price: 45 },
    ShopItem { id: "sword_gold", name: "Golden Blade", icon: "🗡️", price: 80 },
];

/// The base consumables plus any weapons whose id appears in `unlocked` (the TS
/// `buildShopItems` reading `getUnlockedWeapons()`). Order matches the TS: the
/// three consumables, then unlocked weapons in catalog order.
pub fn build_shop_items(unlocked: &[&str]) -> Vec<ShopItem> {
    let mut items: Vec<ShopItem> = SHOP_ITEMS.to_vec();
    for w in WEAPON_CATALOG {
        if unlocked.contains(&w.id) {
            items.push(*w);
        }
    }
    items
}

/// The Merchant-Guild discount: a flat multiplier on every shop price. 1.0 = full
/// price; 0.8 once the guild is purchased. Mirrors the `shopDiscount` module flag
/// in shopStore.ts (here it lives on the shop state the ECS layer owns).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShopDiscount(pub f64);

impl Default for ShopDiscount {
    fn default() -> Self {
        ShopDiscount(1.0)
    }
}

impl ShopDiscount {
    pub fn full() -> Self {
        ShopDiscount(1.0)
    }
    pub fn mult(&self) -> f64 {
        self.0
    }
    pub fn is_discounted(&self) -> bool {
        self.0 < 1.0
    }
    /// Apply the Merchant-Guild discount and round to whole gold (the TS
    /// `discountedPrice`: `Math.round(price * shopDiscount)`).
    pub fn price_of(&self, base_price: i64) -> i64 {
        ((base_price as f64 * self.0) + 0.5).floor() as i64
    }
    pub fn reset(&mut self) {
        self.0 = 1.0;
    }
}

/// Standalone discount math (the TS `discountedPrice`) for callers holding a raw
/// multiplier rather than a `ShopDiscount`.
pub fn discounted_price(base_price: i64, discount_mult: f64) -> i64 {
    ((base_price as f64 * discount_mult) + 0.5).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_catalog_has_the_three_consumables() {
        assert_eq!(SHOP_ITEMS.len(), 3);
        assert_eq!(SHOP_ITEMS[0].id, "bread");
        assert_eq!(SHOP_ITEMS[1].id, "potion");
        assert_eq!(SHOP_ITEMS[2].id, "feast");
    }

    #[test]
    fn prices_match_ts() {
        assert_eq!(SHOP_ITEMS[0].price, 4);
        assert_eq!(SHOP_ITEMS[1].price, 12);
        assert_eq!(SHOP_ITEMS[2].price, 28);
        let axe = WEAPON_CATALOG.iter().find(|w| w.id == "axe").unwrap();
        let blade = WEAPON_CATALOG.iter().find(|w| w.id == "sword_gold").unwrap();
        assert_eq!(axe.price, 45);
        assert_eq!(blade.price, 80);
    }

    #[test]
    fn build_shop_items_only_includes_unlocked_weapons() {
        // Nothing unlocked → just the consumables.
        assert_eq!(build_shop_items(&[]).len(), 3);
        // Unlock the axe → four items, axe appended last.
        let with_axe = build_shop_items(&["axe"]);
        assert_eq!(with_axe.len(), 4);
        assert_eq!(with_axe[3].id, "axe");
        // Both unlocked → five, catalog order (axe then blade).
        let both = build_shop_items(&["sword_gold", "axe"]);
        assert_eq!(both.len(), 5);
        assert_eq!(both[3].id, "axe");
        assert_eq!(both[4].id, "sword_gold");
    }

    #[test]
    fn discount_defaults_to_full_price() {
        let d = ShopDiscount::default();
        assert_eq!(d.mult(), 1.0);
        assert!(!d.is_discounted());
        assert_eq!(d.price_of(28), 28);
    }

    #[test]
    fn merchant_guild_discount_rounds_like_js() {
        let d = ShopDiscount(0.8);
        assert!(d.is_discounted());
        // 4 * 0.8 = 3.2 → round 3
        assert_eq!(d.price_of(4), 3);
        // 12 * 0.8 = 9.6 → round 10
        assert_eq!(d.price_of(12), 10);
        // 28 * 0.8 = 22.4 → round 22
        assert_eq!(d.price_of(28), 22);
        // 45 * 0.8 = 36.0 → 36
        assert_eq!(d.price_of(45), 36);
    }

    #[test]
    fn discount_reset_restores_full_price() {
        let mut d = ShopDiscount(0.8);
        d.reset();
        assert_eq!(d.mult(), 1.0);
    }

    #[test]
    fn standalone_discounted_price_matches() {
        assert_eq!(discounted_price(12, 0.8), 10);
        assert_eq!(discounted_price(28, 1.0), 28);
    }
}
