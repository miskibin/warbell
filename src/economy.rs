//! **Economy + upgrades.** Gold lives on [`crate::player::PlayerRes`]; this module holds the
//! **stone bank**, the **upgrade tree** ([`Upgrades`] over the test-gated
//! `tileworld_core::upgrade_store`), and the resources its effects target ([`Defenses`] flags
//! enacted as real structures in P4, [`EconomyState`] town flags). The tree UI is a `Modal`
//! panel (open with **U**); buying a node deducts gold+stone and enacts its typed effect.

use bevy::prelude::*;
use tileworld_core::inventory::item_def;
use tileworld_core::player::Player;
use tileworld_core::shop_catalog::{build_shop_items, discounted_price};
use tileworld_core::upgrade_store::{
    node_by_id, UpgradeBranch, UpgradeEffect, UpgradeState, UPGRADE_NODES,
};

use crate::game_state::{AppState, Modal};
use crate::inventory::{try_grant, Inventory, Toasts};
use crate::player::PlayerRes;
use crate::siege::KeepHp;

/// The crafting-resource bank — currently just stone. Wraps the parity-tested core store.
#[derive(Resource, Default)]
pub struct Bank(pub tileworld_core::resource_store::ResourceState);

/// Purchased upgrades (the gating/affordability core).
#[derive(Resource, Default)]
pub struct Upgrades(pub UpgradeState);

/// Structural-defense flags set by the Defense branch — turned into real auto-firing structures
/// in P4. (Reinforce is enacted immediately on `KeepHp`.)
#[derive(Resource, Default)]
pub struct Defenses {
    pub walls: bool,
    pub gate: bool,
    pub towers: bool,
    pub tower_mastery: bool,
    pub keep_archers: bool,
    pub reinforced: bool,
    pub ballista: bool,
    pub shrine: bool,
    /// 0 = unarmed townsfolk; each tier hits harder / watches wider (P5).
    pub villager_arms_tier: u32,
}

/// Town/economy flags. `houses` drives population growth (P5); `farm`/`tax_office` are read by
/// the siege wave-clear payout; `shop_discount`/`unlocked_weapons` feed the merchant (P3 shop).
#[derive(Resource)]
pub struct EconomyState {
    pub houses: u32,
    pub farm: bool,
    pub tax_office: bool,
    pub shop_discount: f32,
    pub unlocked_weapons: Vec<&'static str>,
}
impl Default for EconomyState {
    fn default() -> Self {
        Self { houses: 0, farm: false, tax_office: false, shop_discount: 1.0, unlocked_weapons: Vec::new() }
    }
}

/// Reinforced-Keep bonus (forest's keep is 1000 base; +400 → 1400, healed to full).
const REINFORCE_BONUS: f32 = 400.0;
/// Tax Office payout per cleared night.
pub const TAX_STIPEND: i64 = 25;
/// Granary bread harvested per cleared night.
pub const FARM_HARVEST: i64 = 3;

pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bank>()
            .init_resource::<Upgrades>()
            .init_resource::<Defenses>()
            .init_resource::<EconomyState>()
            // Fresh run wipes the economy (gold resets with PlayerRes).
            .add_systems(OnExit(AppState::StartScreen), reset_economy)
            .add_systems(OnExit(AppState::GameOver), reset_economy)
            // Open the tree with U (only while actually playing, no other panel open).
            .add_systems(Update, open_tree.run_if(in_state(Modal::None)))
            .add_systems(OnEnter(Modal::UpgradeTree), spawn_tree)
            .add_systems(OnExit(Modal::UpgradeTree), despawn_tree)
            .add_systems(Update, tree_interact.run_if(in_state(Modal::UpgradeTree)))
            // Merchant shop (open with T; buys land in the bag).
            .add_systems(Update, open_shop.run_if(in_state(Modal::None)))
            .add_systems(OnEnter(Modal::Shop), spawn_shop)
            .add_systems(OnExit(Modal::Shop), despawn_shop)
            .add_systems(Update, shop_interact.run_if(in_state(Modal::Shop)));
    }
}

fn reset_economy(
    mut bank: ResMut<Bank>,
    mut up: ResMut<Upgrades>,
    mut def: ResMut<Defenses>,
    mut eco: ResMut<EconomyState>,
) {
    bank.0.reset();
    up.0.reset();
    *def = Defenses::default();
    *eco = EconomyState::default();
}

/// Enact a typed upgrade effect against the live resources (the dep-free stand-in for the TS
/// `apply()` closures). Hero effects hit `Player`; defense effects set flags; reinforce bumps
/// the keep now.
fn apply_effect(
    effect: UpgradeEffect,
    player: &mut Player,
    def: &mut Defenses,
    eco: &mut EconomyState,
    keep: &mut KeepHp,
) {
    use UpgradeEffect::*;
    match effect {
        BuildHouses(n) => eco.houses += n,
        Farm => eco.farm = true,
        Bounty(m) => player.bounty_mult = m,
        TaxOffice => eco.tax_office = true,
        MerchantGuild(m) => eco.shop_discount = m as f32,
        Walls => def.walls = true,
        Gate => def.gate = true,
        Towers => def.towers = true,
        TowerMastery => def.tower_mastery = true,
        KeepArchers => def.keep_archers = true,
        ReinforceKeep => {
            def.reinforced = true;
            keep.max += REINFORCE_BONUS;
            keep.hp = keep.max;
        }
        VillagerArmor => def.villager_arms_tier += 1,
        Ballista => def.ballista = true,
        HealingShrine => def.shrine = true,
        MaxHp(n) => player.bump_max_hp(n),
        AttackDamage(n) => player.bump_attack_damage(n),
        Crit(c) => player.crit_chance = c,
        Lifesteal(n) => player.lifesteal = n,
        MoveSpeed(m) => player.move_speed_mult = m,
        Cleave(c) => player.cleave = c,
        UnlockWeapon(id) => {
            if !eco.unlocked_weapons.contains(&id) {
                eco.unlocked_weapons.push(id);
            }
        }
    }
}

/// Try to buy node `id`: gate + deduct gold/stone + enact the effect. Returns true on success.
#[allow(clippy::too_many_arguments)]
fn try_purchase(
    id: &str,
    up: &mut Upgrades,
    player: &mut PlayerRes,
    bank: &mut Bank,
    def: &mut Defenses,
    eco: &mut EconomyState,
    keep: &mut KeepHp,
) -> bool {
    let Some(node) = node_by_id(id) else { return false };
    let gold = player.0.gold;
    let stone = bank.0.stone() as i64;
    if let Some(out) = up.0.purchase(node, gold, stone, false) {
        player.0.spend_gold(out.gold_cost, false);
        bank.0.spend_stone(out.stone_cost as f64);
        apply_effect(out.effect, &mut player.0, def, eco, keep);
        true
    } else {
        false
    }
}

// ── Tree UI (Modal panel) ─────────────────────────────────────────────────────────────

fn open_tree(
    keys: Res<ButtonInput<KeyCode>>,
    app: Res<State<AppState>>,
    mut next: ResMut<NextState<Modal>>,
    mut auto_done: Local<bool>,
) {
    // Screenshot hook: `FOREST_PANEL=tree` auto-opens the tree once so the harness can shoot it.
    let force = !*auto_done && std::env::var("FOREST_PANEL").ok().as_deref() == Some("tree");
    if force {
        *auto_done = true;
    }
    if *app.get() == AppState::Playing && (keys.just_pressed(KeyCode::KeyU) || force) {
        next.set(Modal::UpgradeTree);
    }
}

#[derive(Component)]
struct TreeUi;
#[derive(Component)]
struct TreeNodeButton(&'static str);
#[derive(Component)]
struct TreeHeader;

fn branch_title(b: UpgradeBranch) -> &'static str {
    match b {
        UpgradeBranch::Economy => "Prosperity",
        UpgradeBranch::Defense => "Bulwark",
        UpgradeBranch::Hero => "Champion",
        UpgradeBranch::Arsenal => "Armoury",
    }
}

fn node_label(node: &tileworld_core::upgrade_store::UpgradeNode) -> String {
    let stone = if node.stone_cost > 0 { format!(" +{}st", node.stone_cost) } else { String::new() };
    format!("{}\n{}g{}", node.name, node.cost(), stone)
}

fn spawn_tree(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.82)),
            GlobalZIndex(60),
            TreeUi,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("War Table"),
                TextFont { font_size: 34.0, ..default() },
                TextColor(Color::srgb(0.95, 0.88, 0.6)),
            ));
            root.spawn((
                Text::new(""),
                TextFont { font_size: 20.0, ..default() },
                TextColor(Color::srgb(0.9, 0.9, 0.95)),
                TreeHeader,
            ));
            root.spawn((
                Text::new("U / Esc to close"),
                TextFont { font_size: 15.0, ..default() },
                TextColor(Color::srgba(0.8, 0.8, 0.85, 0.7)),
            ));
            // Four branch columns.
            root.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(12.0), ..default() })
                .with_children(|cols| {
                    for branch in
                        [UpgradeBranch::Economy, UpgradeBranch::Defense, UpgradeBranch::Hero, UpgradeBranch::Arsenal]
                    {
                        cols.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(5.0),
                            width: Val::Px(170.0),
                            ..default()
                        })
                        .with_children(|col| {
                            col.spawn((
                                Text::new(branch_title(branch)),
                                TextFont { font_size: 17.0, ..default() },
                                TextColor(Color::srgb(0.7, 0.8, 1.0)),
                            ));
                            for node in UPGRADE_NODES.iter().filter(|n| n.branch == branch) {
                                col.spawn((
                                    Button,
                                    Interaction::default(),
                                    Node {
                                        width: Val::Percent(100.0),
                                        padding: UiRect::all(Val::Px(5.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.18, 0.18, 0.2)),
                                    TreeNodeButton(node.id),
                                ))
                                .with_children(|b| {
                                    b.spawn((
                                        Text::new(node_label(node)),
                                        TextFont { font_size: 13.0, ..default() },
                                        TextColor(Color::WHITE),
                                    ));
                                });
                            }
                        });
                    }
                });
        });
}

fn despawn_tree(mut commands: Commands, q: Query<Entity, With<TreeUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Per-frame: colour each node by state (owned / buyable / locked-or-poor), update the
/// gold/stone header, and on a click buy the node.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn tree_interact(
    mut up: ResMut<Upgrades>,
    mut player: ResMut<PlayerRes>,
    mut bank: ResMut<Bank>,
    mut def: ResMut<Defenses>,
    mut eco: ResMut<EconomyState>,
    mut keep: ResMut<KeepHp>,
    mut buttons: Query<(&Interaction, &TreeNodeButton, &mut BackgroundColor)>,
    mut header: Query<&mut Text, With<TreeHeader>>,
) {
    let gold = player.0.gold;
    let stone = bank.0.stone() as i64;

    // Handle a click first (one buy; the node becomes owned so a held press is harmless).
    for (interaction, btn, _) in &buttons {
        if *interaction == Interaction::Pressed {
            try_purchase(
                btn.0,
                &mut up,
                &mut player,
                &mut bank,
                &mut def,
                &mut eco,
                &mut keep,
            );
            break;
        }
    }

    // Re-colour every node by its current state.
    for (_, btn, mut bg) in &mut buttons {
        let Some(node) = node_by_id(btn.0) else { continue };
        bg.0 = if up.0.is_purchased(btn.0) {
            Color::srgb(0.22, 0.45, 0.25) // owned
        } else if up.0.can_buy(node, gold, stone, false) {
            Color::srgb(0.5, 0.42, 0.16) // buyable
        } else {
            Color::srgb(0.16, 0.16, 0.19) // locked / can't afford
        };
    }

    if let Ok(mut t) = header.single_mut() {
        **t = format!("Gold {gold}   Stone {stone}");
    }
}

// ── Merchant shop (Modal panel) ────────────────────────────────────────────────────────

#[derive(Component)]
struct ShopUi;
#[derive(Component)]
struct ShopItemButton(&'static str);
#[derive(Component)]
struct ShopHeader;

fn open_shop(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<Modal>>,
    mut auto_done: Local<bool>,
) {
    // Screenshot hook: `FOREST_PANEL=shop` opens the merchant once for the harness.
    let force = !*auto_done && std::env::var("FOREST_PANEL").ok().as_deref() == Some("shop");
    if force {
        *auto_done = true;
    }
    if keys.just_pressed(KeyCode::KeyT) || force {
        next.set(Modal::Shop);
    }
}

fn spawn_shop(mut commands: Commands, eco: Res<EconomyState>, atlas: Res<crate::icons::IconAtlas>) {
    let discount = eco.shop_discount as f64;
    let items = build_shop_items(&eco.unlocked_weapons);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.82)),
            GlobalZIndex(60),
            ShopUi,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("Merchant"),
                TextFont { font_size: 34.0, ..default() },
                TextColor(Color::srgb(0.95, 0.88, 0.6)),
            ));
            root.spawn((
                Text::new(""),
                TextFont { font_size: 20.0, ..default() },
                TextColor(Color::srgb(0.9, 0.9, 0.95)),
                ShopHeader,
            ));
            root.spawn((
                Text::new("T / Esc to close  ·  click to buy"),
                TextFont { font_size: 14.0, ..default() },
                TextColor(Color::srgba(0.8, 0.8, 0.85, 0.7)),
            ));
            for item in items {
                let price = discounted_price(item.price, discount);
                let stat = item_def(item.id).map(|d| d.stat_line()).unwrap_or_default();
                root.spawn((
                    Button,
                    Interaction::default(),
                    Node {
                        width: Val::Px(360.0),
                        padding: UiRect::all(Val::Px(6.0)),
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.18, 0.18, 0.21)),
                    ShopItemButton(item.id),
                ))
                .with_children(|b| {
                    if let Some(icon) = atlas.get(item.id) {
                        b.spawn((
                            Node { width: Val::Px(22.0), height: Val::Px(22.0), margin: UiRect::right(Val::Px(8.0)), ..default() },
                            ImageNode::new(icon),
                        ));
                    }
                    b.spawn((
                        Text::new(format!("{}   {}", item.name, stat)),
                        TextFont { font_size: 15.0, ..default() },
                        TextColor(Color::WHITE),
                    ));
                    b.spawn((
                        Text::new(format!("{price}g")),
                        TextFont { font_size: 15.0, ..default() },
                        TextColor(Color::srgb(0.96, 0.86, 0.45)),
                    ));
                });
            }
        });
}

fn despawn_shop(mut commands: Commands, q: Query<Entity, With<ShopUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Per-frame: recolour each line by affordability + update the gold header; on a click buy the
/// item (deduct discounted gold + drop into the bag, blocked when the bag is full).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn shop_interact(
    time: Res<Time>,
    eco: Res<EconomyState>,
    mut player: ResMut<PlayerRes>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut buttons: Query<(&Interaction, &ShopItemButton, &mut BackgroundColor)>,
    mut header: Query<&mut Text, With<ShopHeader>>,
) {
    let discount = eco.shop_discount as f64;
    let now = time.elapsed_secs() as f64;

    // Handle a click first (one buy per press).
    for (interaction, btn, _) in &buttons {
        if *interaction == Interaction::Pressed {
            if let Some(item) = build_shop_items(&eco.unlocked_weapons).iter().find(|i| i.id == btn.0) {
                let price = discounted_price(item.price, discount);
                // Need the gold AND room in the bag; otherwise no-op (TS refunds a full bag).
                if player.0.gold >= price && inv.0.has_room_for(&[item.id]) {
                    player.0.spend_gold(price, false);
                    try_grant(&mut inv.0, &mut toasts.0, item.id, 1, now);
                }
            }
            break;
        }
    }

    // Recolour lines: affordable = gold, too dear = grey.
    let gold = player.0.gold;
    for (_, btn, mut bg) in &mut buttons {
        let price = build_shop_items(&eco.unlocked_weapons)
            .iter()
            .find(|i| i.id == btn.0)
            .map(|i| discounted_price(i.price, discount))
            .unwrap_or(i64::MAX);
        bg.0 = if gold >= price {
            Color::srgb(0.5, 0.42, 0.16)
        } else {
            Color::srgb(0.16, 0.16, 0.19)
        };
    }

    if let Ok(mut t) = header.single_mut() {
        **t = format!("Gold {gold}");
    }
}
