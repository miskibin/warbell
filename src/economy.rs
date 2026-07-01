//! **Economy + upgrades.** Gold lives on [`crate::player::PlayerRes`]; this module holds the
//! **stone bank**, the **upgrade tree** ([`Upgrades`] over the test-gated
//! `tileworld_core::upgrade_store`), and the resources its effects target ([`Defenses`] flags
//! enacted as real structures in P4, [`EconomyState`] town flags). The tree UI is a `Modal`
//! panel (open with **U**); buying a node deducts gold+stone and enacts its typed effect.

use bevy::prelude::*;
use tileworld_core::inventory::{item_def, sell_value};
use tileworld_core::player::Player;
use tileworld_core::shop_catalog::{build_shop_items, discounted_price};
use tileworld_core::upgrade_store::{node_by_id, UpgradeEffect, UpgradeState, UPGRADE_NODES};

use crate::game_state::{AppState, Modal};
use crate::inventory::{try_grant, Inventory, Toasts};
use crate::player::PlayerRes;
use crate::siege::KeepHp;
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

/// The crafting-resource bank — currently just stone. Wraps the parity-tested core store.
#[derive(Resource, Default)]
pub struct Bank(pub tileworld_core::resource_store::ResourceState);

/// Purchased upgrades (the gating/affordability core).
#[derive(Resource, Default)]
pub struct Upgrades(pub UpgradeState);

/// Structural-defense flags set by the Defense branch — turned into real auto-firing structures
/// in P4. (Reinforce is enacted immediately on `KeepHp`.)
#[derive(Resource, Default, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Flat +HP granted to every militia member by the guard-vigor upgrade line
    /// (`GuardHealth`). `#[serde(default)]` so pre-existing saves load (field absent → 0).
    #[serde(default)]
    pub guard_hp_bonus: f32,
}

/// Town/economy flags. `tax_office` is read by the siege wave-clear payout;
/// `shop_discount`/`unlocked_weapons` feed the merchant (P3 shop). (Food + population
/// now belong to the town city-building layer — `town_store` / `src/town.rs`.)
#[derive(Resource)]
pub struct EconomyState {
    pub tax_office: bool,
    pub shop_discount: f32,
    pub unlocked_weapons: Vec<&'static str>,
}
impl Default for EconomyState {
    fn default() -> Self {
        Self { tax_office: false, shop_discount: 1.0, unlocked_weapons: Vec::new() }
    }
}

/// Reinforced-Keep bonus (forest's keep is 1500 base; +400 → 1900, healed to full).
pub const REINFORCE_BONUS: f32 = 400.0;
// (The old flat Tax Office stipend is gone: dawn gold is now the population tithe —
// `town_store::Town::tithe`, paid in `siege::run_director` — and Tax Office doubles it.)

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
            // No OnExit(Paused) reset: pause-menu Restart resets **in-process** by routing through
            // StartScreen → Playing (see game_state::drive_fresh_run), so this OnExit(StartScreen)
            // reset already covers it; Load restores over it.
            // (The War Table tree panel itself lives in `tree_ui.rs` — TreeUiPlugin.)
            // Merchant shop (open with T; buys land in the bag).
            .add_systems(Update, open_shop.run_if(in_state(Modal::None)))
            .add_systems(OnEnter(Modal::Shop), spawn_shop)
            .add_systems(OnExit(Modal::Shop), despawn_shop)
            .add_systems(Update, (shop_interact, shop_close).run_if(in_state(Modal::Shop)));
        // Clip-only: auto-fill the War Table for a tree-showcase clip (ungated — runs while the
        // tree panel is up, which freezes the world).
        if std::env::var("FOREST_DEMO").ok().as_deref() == Some("tree") {
            app.add_systems(Update, demo_tree_fill);
        }
    }
}

/// Pub so `town::reset_town` can order its starting-wood grant *after* this — otherwise
/// `bank.0.reset()` here (which now zeroes food/wood too) races and wipes the grant.
pub fn reset_economy(
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
        GuardHealth(n) => def.guard_hp_bonus += n as f32,
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
pub(crate) fn try_purchase(
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

/// Demo hook (`FOREST_DEMO=tree`): seed resources, open the War Table, then buy one node per few
/// frames off the clip's frame-locked clock so a clip films the tree filling in (nodes flip to the
/// owned tan as `tree_interact` recolours them). Clip-only; never wired in real play.
#[allow(clippy::too_many_arguments)]
fn demo_tree_fill(
    prog: Option<Res<crate::capture::ClipProgress>>,
    app: Res<State<AppState>>,
    mut next: ResMut<NextState<Modal>>,
    mut up: ResMut<Upgrades>,
    mut player: ResMut<PlayerRes>,
    mut bank: ResMut<Bank>,
    mut def: ResMut<Defenses>,
    mut eco: ResMut<EconomyState>,
    mut keep: ResMut<KeepHp>,
    mut primed: Local<bool>,
    mut last: Local<i32>,
) {
    const STEP: u32 = 13; // recorded frames between buys
    if !*primed {
        *primed = true;
        *last = -1;
        player.0.add_gold(8000);
        bank.0.add_stone(800.0);
        if *app.get() == AppState::Playing {
            next.set(Modal::UpgradeTree);
        }
    }
    let Some(prog) = prog.as_ref() else { return };
    if !prog.recording {
        return;
    }
    let step = (prog.frame / STEP) as i32;
    if step <= *last {
        return;
    }
    *last = step;
    // Buy the next affordable, prereq-met node (one per step) so the panel fills gradually.
    for node in UPGRADE_NODES.iter() {
        if !up.0.is_purchased(node.id)
            && up.0.can_buy(node, player.0.gold, bank.0.stone() as i64, false)
        {
            try_purchase(node.id, &mut up, &mut player, &mut bank, &mut def, &mut eco, &mut keep);
            break;
        }
    }
}

// ── Merchant shop (Modal panel) ────────────────────────────────────────────────────────

#[derive(Component)]
struct ShopUi;
#[derive(Component)]
struct ShopItemButton(&'static str);
/// A SELL row — click to sell one of this bag item back to the merchant for gold.
#[derive(Component)]
struct ShopSellButton(String);
#[derive(Component)]
struct ShopHeader;
/// The header ✕ — leaves the shop like T/Esc.
#[derive(Component)]
struct ShopCloseBtn;

// The shop opens via the contextual **E** at the merchant stall (see `interaction.rs`); this system
// only keeps the `FOREST_PANEL=shop` screenshot hook alive.
fn open_shop(mut next: ResMut<NextState<Modal>>, mut auto_done: Local<bool>) {
    let force = !*auto_done && std::env::var("FOREST_PANEL").ok().as_deref() == Some("shop");
    if force {
        *auto_done = true;
    }
    if force {
        next.set(Modal::Shop);
    }
}

fn spawn_shop(
    mut commands: Commands,
    eco: Res<EconomyState>,
    inv: Res<Inventory>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<crate::ui::texture::UiTextures>,
) {
    build_shop_panel(&mut commands, &eco, &inv, &fonts, &atlas, &tex);
}

/// (Re)build the merchant panel: a BUY list (catalog) above a SELL list (the bag's sellable items).
/// Called on open and after every buy/sell so the SELL list tracks the bag. Mirrors the satchel's
/// rebuild-on-action.
fn build_shop_panel(
    commands: &mut Commands,
    eco: &EconomyState,
    inv: &Inventory,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    tex: &crate::ui::texture::UiTextures,
) {
    let discount = eco.shop_discount as f64;
    let items = build_shop_items(&eco.unlocked_weapons);
    // Distinct sellable bag items (skip key items — `sell_value` 0) with their stack counts.
    let mut sellable: Vec<(String, i64, i64)> = Vec::new(); // (id, count, unit_price)
    for slot in inv.0.bag.iter() {
        let Some(id) = slot.item_id.as_deref() else { continue };
        let value = sell_value(id);
        if value <= 0 {
            continue;
        }
        if let Some(row) = sellable.iter_mut().find(|(rid, _, _)| rid == id) {
            row.1 += slot.count;
        } else {
            sellable.push((id.to_string(), slot.count, value));
        }
    }
    commands.spawn((widgets::scrim(60), ShopUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(380.0),
                row_gap: Val::Px(10.0),
                padding: UiRect::axes(Val::Px(26.0), Val::Px(22.0)),
                border: border(2.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            anim(AnimKind::PopIn, 0.0, 0.26),
        ))
        .with_children(|card| {
            widgets::chrome_layers(card, tex.linen.clone());
            // Header.
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::bottom(Val::Px(8.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            })
            .insert(BorderColor::all(BORDER_SOFT))
            .with_children(|h| {
                h.spawn(label(&fonts.display, "WANDERING MERCHANT", 16.0, GOLD));
                h.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|right| {
                    right.spawn((label(&fonts.bold, "Gold 0", 13.0, GOLD), ShopHeader));
                    widgets::close_button(right, &fonts.bold, ShopCloseBtn, false);
                });
            });
            // ── BUY: the catalog. ──
            card.spawn(label(&fonts.semibold, "BUY", 10.0, GREY));
            for item in items {
                let price = discounted_price(item.price, discount);
                let stat = item_def(item.id).map(|d| d.stat_line()).unwrap_or_default();
                card.spawn((
                    Button,
                    Interaction::default(),
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        width: Val::Px(360.0),
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
                        border: border(1.0),
                        border_radius: radius(R_BTN),
                        ..default()
                    },
                    BackgroundColor(BTN_BG),
                    BorderColor::all(BORDER_SOFT),
                    crate::ui::focus::Focusable,
                    ShopItemButton(item.id),
                ))
                .with_children(|b| {
                    if let Some(entry) = atlas.get_tintable(item.id) {
                        b.spawn(widgets::icon_tinted(entry, 24.0, crate::inventory::item_tint(item.id)));
                    }
                    b.spawn((
                        Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, ..default() },
                        children![
                            label(&fonts.semibold, item.name, 14.0, TEXT),
                            label(&fonts.regular, stat, 11.0, GREY),
                        ],
                    ));
                    b.spawn(label(&fonts.bold, format!("{price}g"), 14.0, GOLD));
                });
            }
            // ── SELL: the bag's sellable items (gear you don't want → gold). ──
            if !sellable.is_empty() {
                card.spawn((
                    Node { padding: UiRect::top(Val::Px(4.0)), ..default() },
                    children![label(&fonts.semibold, "SELL FROM BAG", 10.0, GREY)],
                ));
                for (id, count, value) in sellable {
                    let name = item_def(&id).map(|d| d.name).unwrap_or("?");
                    card.spawn((
                        Button,
                        Interaction::default(),
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(12.0),
                            width: Val::Px(360.0),
                            padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                            border: border(1.0),
                            border_radius: radius(R_BTN),
                            ..default()
                        },
                        BackgroundColor(BTN_BG),
                        BorderColor::all(BORDER_SOFT),
                        crate::ui::focus::Focusable,
                        ShopSellButton(id.clone()),
                    ))
                    .with_children(|b| {
                        if let Some(entry) = atlas.get_tintable(&id) {
                            b.spawn(widgets::icon_tinted(entry, 22.0, crate::inventory::item_tint(&id)));
                        }
                        b.spawn((
                            Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, ..default() },
                            children![
                                label(&fonts.semibold, format!("{name}  x{count}"), 13.0, TEXT),
                                label(&fonts.regular, "click to sell one", 10.0, GREY),
                            ],
                        ));
                        b.spawn(label(&fonts.bold, format!("+{value}g"), 13.0, GREEN));
                    });
                }
            }
            // Close hint.
            card.spawn(label(&fonts.regular, "T or Esc to leave  ·  click to buy or sell", 11.0, GREY));
        });
    });
}

fn despawn_shop(mut commands: Commands, q: Query<Entity, With<ShopUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// The header ✕ — click leaves the shop (same as T/Esc).
fn shop_close(
    mut next: ResMut<NextState<Modal>>,
    btns: Query<&Interaction, (With<ShopCloseBtn>, Changed<Interaction>)>,
) {
    if btns.iter().any(|i| *i == Interaction::Pressed) {
        next.set(Modal::None);
    }
}

/// Per-frame: recolour each BUY line by affordability + update the gold header. On a click, BUY the
/// catalog item (deduct discounted gold + drop into the bag, blocked when full) or SELL one of a bag
/// item back for its `sell_value`; either rebuilds the panel so the SELL list tracks the bag.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn shop_interact(
    time: Res<Time>,
    eco: Res<EconomyState>,
    mut player: ResMut<PlayerRes>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut acts: MessageReader<crate::ui::focus::FocusActivate>,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<crate::ui::texture::UiTextures>,
    mut buy_buttons: Query<(Entity, &Interaction, &ShopItemButton, &mut BackgroundColor)>,
    sell_buttons: Query<(Entity, &Interaction, &ShopSellButton)>,
    panel: Query<Entity, With<ShopUi>>,
    mut header: Query<&mut Text, With<ShopHeader>>,
) {
    let discount = eco.shop_discount as f64;
    let now = time.elapsed_secs() as f64;
    let keyed: Vec<Entity> = acts.read().map(|a| a.0).collect();
    let mut acted = false;
    // Built once per tick — the catalog is tiny but this system runs every frame the shop is open
    // and both the BUY loop and the recolour loop scan it (was rebuilt per-button before).
    let items = build_shop_items(&eco.unlocked_weapons);

    // BUY: a click or Enter/E focus activation (one per press).
    for (e, interaction, btn, _) in &buy_buttons {
        if *interaction == Interaction::Pressed || keyed.contains(&e) {
            if let Some(item) = items.iter().find(|i| i.id == btn.0) {
                let price = discounted_price(item.price, discount);
                // Need the gold AND room in the bag; otherwise no-op (TS refunds a full bag).
                if player.0.gold >= price && inv.0.has_room_for(&[item.id]) {
                    player.0.spend_gold(price, false);
                    try_grant(&mut inv.0, &mut toasts.0, item.id, 1, now);
                    cues.write(crate::audio::AudioCue::ShopBuy);
                    acted = true;
                }
            }
            break;
        }
    }

    // SELL: one of the clicked bag item back for gold (only if no buy this frame).
    if !acted {
        let sell_id = sell_buttons
            .iter()
            .find(|(e, i, _)| **i == Interaction::Pressed || keyed.contains(e))
            .map(|(_, _, b)| b.0.clone());
        if let Some(id) = sell_id {
            let value = sell_value(&id);
            if value > 0 && inv.0.consume_item(&id, 1) {
                player.0.add_gold(value);
                cues.write(crate::audio::AudioCue::Gold); // coin chime for the payout
                acted = true;
            }
        }
    }

    // A transaction changed the bag → rebuild so the SELL list reflects it. (BUY lines re-colour
    // next frame.)
    if acted {
        for e in &panel {
            commands.entity(e).despawn();
        }
        build_shop_panel(&mut commands, &eco, &inv, &fonts, &atlas, &tex);
        return;
    }

    // Recolour BUY lines: affordable = gold, too dear = grey.
    let gold = player.0.gold;
    for (_, _, btn, mut bg) in &mut buy_buttons {
        let price = items
            .iter()
            .find(|i| i.id == btn.0)
            .map(|i| discounted_price(i.price, discount))
            .unwrap_or(i64::MAX);
        bg.0 = if gold >= price {
            BTN_BG_HOVER // affordable — brighter
        } else {
            rgba(255, 255, 255, 0.015) // too dear — sunken
        };
    }

    if let Ok(mut t) = header.single_mut() {
        **t = format!("Gold {gold}");
    }
}
