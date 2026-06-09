//! JSON round-trip tests for the save-relevant stores. The whole file compiles to
//! nothing unless the `serde` feature is on, so the default parity-gate run
//! (`cargo test -p tileworld_core`) stays dependency-free. Run with:
//! `cargo test -p tileworld_core --features serde`.
#![cfg(feature = "serde")]

use tileworld_core::inventory::Bag;
use tileworld_core::player::Player;
use tileworld_core::resource_store::ResourceState;
use tileworld_core::town_store::{BuildKind, Town};

/// serialize → deserialize → equal.
fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_string(value).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

#[test]
fn player_round_trips() {
    let mut p = Player::default();
    p.gold = 1234;
    p.level = 7;
    p.xp = 88;
    p.max_hp = 250.0;
    p.crit_chance = 0.2;
    assert_eq!(round_trip(&p), p);
}

#[test]
fn resource_state_round_trips() {
    let mut r = ResourceState::new();
    r.add_stone(40.0);
    r.add_wood(13.5);
    r.add_food(7.0);
    assert_eq!(round_trip(&r), r);
}

#[test]
fn bag_round_trips_with_gear_and_stacks() {
    let mut b = Bag::new();
    b.add("bread", 3);
    b.add("sword_iron", 1);
    // Equip the sword so the derived fields (equipped_id, weapon_bonus) are non-default.
    let i = b
        .bag
        .iter()
        .position(|s| s.item_id.as_deref() == Some("sword_iron"))
        .unwrap();
    b.activate_bag_item(i);
    assert_eq!(b.equipped_id.as_deref(), Some("sword_iron"));
    assert_eq!(round_trip(&b), b);
}

#[test]
fn town_round_trips_with_built_and_rubble_plots() {
    let mut t = Town::new(4, 2);
    let mut bank = ResourceState::new();
    bank.add_wood(50.0);
    bank.add_stone(50.0);
    t.build(0, BuildKind::Farm, &mut bank);
    t.build(1, BuildKind::Lumber, &mut bank);
    t.build_house(&mut bank); // exercise the houses count too
    t.damage(0, 5.0); // ignite plot 0 (Built { burning: true })
    t.damage(2, 1.0); // plot 2 was empty → no-op, stays Empty
    assert_eq!(round_trip(&t), t);
}
