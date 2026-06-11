//! **Economy + upgrades.** Gold lives on [`crate::player::PlayerRes`]; this module holds the
//! **stone bank**, the **upgrade tree** ([`Upgrades`] over the test-gated
//! `tileworld_core::upgrade_store`), and the resources its effects target ([`Defenses`] flags
//! enacted as real structures in P4, [`EconomyState`] town flags). The tree UI is a `Modal`
//! panel (open with **U**); buying a node deducts gold+stone and enacts its typed effect.

use bevy::prelude::*;
use tileworld_core::inventory::item_def;
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

/// Reinforced-Keep bonus (forest's keep is 1000 base; +400 → 1400, healed to full).
const REINFORCE_BONUS: f32 = 400.0;
/// Tax Office payout per cleared night.
pub const TAX_STIPEND: i64 = 25;

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
            // Pause-menu Restart / Load also begins a fresh run (gated; see game_state).
            .add_systems(
                OnExit(AppState::Paused),
                reset_economy.run_if(crate::game_state::restart_requested),
            )
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
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<crate::ui::texture::UiTextures>,
) {
    let discount = eco.shop_discount as f64;
    let items = build_shop_items(&eco.unlocked_weapons);
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
            // Item rows.
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
                    if let Some(handle) = atlas.get(item.id) {
                        b.spawn(widgets::icon(handle, 24.0));
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
            // Close hint.
            card.spawn(label(&fonts.regular, "T or Esc to leave  ·  click to buy", 11.0, GREY));
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

/// Per-frame: recolour each line by affordability + update the gold header; on a click buy the
/// item (deduct discounted gold + drop into the bag, blocked when the bag is full).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn shop_interact(
    time: Res<Time>,
    eco: Res<EconomyState>,
    mut player: ResMut<PlayerRes>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut acts: MessageReader<crate::ui::focus::FocusActivate>,
    mut buttons: Query<(Entity, &Interaction, &ShopItemButton, &mut BackgroundColor)>,
    mut header: Query<&mut Text, With<ShopHeader>>,
) {
    let discount = eco.shop_discount as f64;
    let now = time.elapsed_secs() as f64;

    // Handle a click or an Enter/E focus activation first (one buy per press).
    let keyed: Vec<Entity> = acts.read().map(|a| a.0).collect();
    for (e, interaction, btn, _) in &buttons {
        if *interaction == Interaction::Pressed || keyed.contains(&e) {
            if let Some(item) = build_shop_items(&eco.unlocked_weapons).iter().find(|i| i.id == btn.0) {
                let price = discounted_price(item.price, discount);
                // Need the gold AND room in the bag; otherwise no-op (TS refunds a full bag).
                if player.0.gold >= price && inv.0.has_room_for(&[item.id]) {
                    player.0.spend_gold(price, false);
                    try_grant(&mut inv.0, &mut toasts.0, item.id, 1, now);
                    cues.write(crate::audio::AudioCue::ShopBuy);
                }
            }
            break;
        }
    }

    // Recolour lines: affordable = gold, too dear = grey.
    let gold = player.0.gold;
    for (_, _, btn, mut bg) in &mut buttons {
        let price = build_shop_items(&eco.unlocked_weapons)
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
