//! **Combat HUD** — ported from the 3js `PlayerHud` / `QuickBar` / `BuffBar` / `ItemToasts`.
//! Bottom-left: a level badge + gradient HP/XP/stamina bars. Bottom-centre: the gold/stone tally
//! over four quick-use slots (Q/Z/X/C). Above the vitals: buff pips. Top-left: pickup toasts.
//! All chrome comes from [`crate::ui`]; the bars/text bind to the live hero stores.

use bevy::prelude::*;
use tileworld_core::buff_store::BuffKind;
use tileworld_core::inventory::{item_def, QuickSlot};

use crate::player::{HeroHealth, PlayerRes};
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;
use crate::inventory::{Buffs, Inventory, Toasts};

#[derive(Component)]
struct HpFill;
#[derive(Component)]
struct HpText;
#[derive(Component)]
struct StaminaFill;
#[derive(Component)]
struct XpFill;
#[derive(Component)]
struct LevelText;
#[derive(Component)]
struct GoldText;
#[derive(Component)]
struct StoneText;
#[derive(Component)]
struct FoodText;
#[derive(Component)]
struct WoodText;
#[derive(Component)]
struct PopText;
/// Net daily food balance line in the town panel.
#[derive(Component)]
struct PopNetText;
/// The settle/starve meter fill (green toward a new peasant, red toward losing one).
#[derive(Component)]
struct PopBar;
/// One-line "what's happening" under the town panel bar.
#[derive(Component)]
struct PopHintText;

/// Which derived quick-slot a node belongs to.
#[derive(Clone, Copy, PartialEq)]
enum SlotKind {
    Food,
    Resist,
    Power,
    Haste,
}
impl SlotKind {
    fn key(self) -> char {
        match self {
            SlotKind::Food => 'Q',
            SlotKind::Resist => 'Z',
            SlotKind::Power => 'X',
            SlotKind::Haste => 'C',
        }
    }
}
#[derive(Component)]
struct QuickSlotIcon(SlotKind);
#[derive(Component)]
struct QuickSlotCount(SlotKind);
/// Container the buff pips are rebuilt into each frame.
#[derive(Component)]
struct BuffRoot;
#[derive(Component)]
struct BuffPip;
/// The toast column container (rows are cleared + respawned each frame).
#[derive(Component)]
struct ToastRoot;
#[derive(Component)]
struct ToastRow;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_hud, setup_inv_hud))
            .add_systems(Update, (update_hud, update_inv_hud, update_town_panel));
    }
}

const BAR_W: f32 = 240.0;

/// A full-size absolute fill quad with a vertical gradient (width driven live), tagged `marker`.
fn fill(top: Color, bot: Color, marker: impl Bundle) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        widgets::vgrad(top, bot),
        marker,
    )
}

/// A rounded bar-track node of the given height.
fn track(h: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(BAR_W),
            height: Val::Px(h),
            border: border(1.0),
            border_radius: radius(R_CELL),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(PANEL_HUD),
        BorderColor::all(BORDER_SOFT),
    )
}

fn setup_hud(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(18.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|root| {
            // Level badge.
            root.spawn((
                Node {
                    width: Val::Px(40.0),
                    height: Val::Px(40.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: border(1.0),
                    border_radius: radius(R_BTN),
                    ..default()
                },
                BackgroundColor(PANEL_HUD),
                BorderColor::all(rgba(255, 213, 140, 0.5)),
                shadow_hud(),
            ))
            .with_children(|b| {
                b.spawn((label(&fonts.extrabold, "1", 16.0, GOLD), LevelText));
            });
            // Bars column.
            root.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(3.0), ..default() })
                .with_children(|col| {
                    col.spawn(track(20.0)).with_children(|t| {
                        t.spawn(fill(HP_TOP, HP_BOT, HpFill));
                        t.spawn((
                            label(&fonts.bold, "100", 11.0, Color::WHITE),
                            TextShadow { offset: Vec2::ZERO, color: rgba(0, 0, 0, 0.7) },
                            HpText,
                        ));
                    });
                    col.spawn(track(10.0)).with_children(|t| {
                        t.spawn(fill(XP_TOP, XP_BOT, XpFill));
                    });
                    col.spawn(track(7.0)).with_children(|t| {
                        t.spawn(fill(STAM_TOP, STAM_BOT, StaminaFill));
                    });
                });
        });
}

/// Pickup toasts (top-left) + the quick-bar (bottom-centre) + the buff pips (bottom-left).
fn setup_inv_hud(mut commands: Commands, fonts: Res<UiFonts>) {
    // Top-left pickup-toast column.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            top: Val::Px(18.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            ..default()
        },
        bevy::ui::FocusPolicy::Pass,
        ToastRoot,
    ));
    // Buff pips, bottom-left above the vitals.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(80.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            ..default()
        },
        bevy::ui::FocusPolicy::Pass,
        BuffRoot,
    ));
    // Top-right TOWN panel: headcount, daily food balance, and a settle/starve meter — so the
    // food→population loop is legible at a glance (why peasants arrive or leave, and what to do).
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(18.0),
                right: Val::Px(18.0),
                width: Val::Px(186.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                border: border(2.0),
                border_radius: radius(R_BTN),
                ..default()
            },
            BackgroundColor(rgba(20, 22, 28, 0.72)),
            BorderColor::all(rgba(0, 0, 0, 0.5)),
            bevy::ui::FocusPolicy::Pass,
        ))
        .with_children(|col| {
            col.spawn((label(&fonts.extrabold, "\u{1f465} Town  4 / 4", 15.0, rgb(235, 224, 180)), PopText));
            col.spawn((label(&fonts.semibold, "Food balanced", 12.0, rgb(170, 178, 190)), PopNetText));
            // Settle/starve meter: a dark track with a fill that grows green toward the next
            // peasant, or red toward losing one.
            col.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(7.0),
                    border_radius: radius(4.0),
                    ..default()
                },
                BackgroundColor(rgba(0, 0, 0, 0.45)),
            ))
            .with_children(|track| {
                track.spawn((
                    Node { width: Val::Percent(0.0), height: Val::Percent(100.0), border_radius: radius(4.0), ..default() },
                    BackgroundColor(GREEN),
                    PopBar,
                ));
            });
            col.spawn((label(&fonts.regular, "", 11.0, rgb(150, 158, 170)), PopHintText));
        });
    // Bottom-centre quick-bar: gold/stone tally over a row of four slots.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(14.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|col| {
            col.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(14.0), ..default() })
                .with_children(|r| {
                    r.spawn((label(&fonts.extrabold, "Gold 30", 13.0, GOLD), GoldText));
                    r.spawn((label(&fonts.extrabold, "Stone 0", 13.0, STONE), StoneText));
                    r.spawn((label(&fonts.extrabold, "Wood 0", 13.0, rgb(190, 150, 100)), WoodText));
                });
            col.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(5.0)),
                    border: border(2.0),
                    border_radius: radius(R_BTN),
                    ..default()
                },
                BackgroundColor(rgba(20, 22, 28, 0.72)),
                BorderColor::all(rgba(0, 0, 0, 0.5)),
            ))
            .with_children(|row| {
                for kind in [SlotKind::Food, SlotKind::Resist, SlotKind::Power, SlotKind::Haste] {
                    row.spawn((
                        Node {
                            width: Val::Px(52.0),
                            height: Val::Px(52.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: border(2.0),
                            border_radius: radius(R_SLOT),
                            ..default()
                        },
                        widgets::slot_paint(),
                    ))
                    .with_children(|slot| {
                        // Key (top-left).
                        slot.spawn((
                            Node { position_type: PositionType::Absolute, top: Val::Px(1.0), left: Val::Px(4.0), ..default() },
                            label(&fonts.extrabold, kind.key().to_string(), 10.0, rgba(230, 236, 246, 0.8)),
                        ));
                        // Icon (centre).
                        slot.spawn((
                            Node { width: Val::Px(30.0), height: Val::Px(30.0), display: Display::None, ..default() },
                            ImageNode::new(Handle::default()),
                            QuickSlotIcon(kind),
                        ));
                        // Count (bottom-right).
                        slot.spawn((
                            Node { position_type: PositionType::Absolute, right: Val::Px(3.0), bottom: Val::Px(1.0), ..default() },
                            label(&fonts.extrabold, "", 13.0, Color::WHITE),
                            TextShadow { offset: Vec2::ZERO, color: rgba(0, 0, 0, 0.9) },
                            QuickSlotCount(kind),
                        ));
                    });
                }
            });
        });
}

fn slot_for(inv: &Inventory, kind: SlotKind) -> Option<QuickSlot> {
    match kind {
        SlotKind::Food => inv.0.food_slot(),
        SlotKind::Resist => inv.0.buff_slot(BuffKind::Resist),
        SlotKind::Power => inv.0.buff_slot(BuffKind::Power),
        SlotKind::Haste => inv.0.buff_slot(BuffKind::Haste),
    }
}

fn buff_icon_key(kind: BuffKind) -> &'static str {
    match kind {
        BuffKind::Resist => "buff:resist",
        BuffKind::Power => "buff:power",
        BuffKind::Haste => "buff:haste",
    }
}

/// Drive quick-slot icons/counts, rebuild the buff pips, and rebuild the pickup-toast rows.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_inv_hud(
    time: Res<Time>,
    inv: Res<Inventory>,
    buffs: Res<Buffs>,
    atlas: Res<IconAtlas>,
    fonts: Res<UiFonts>,
    mut toasts: ResMut<Toasts>,
    mut commands: Commands,
    mut icon_q: Query<(&QuickSlotIcon, &mut Node, &mut ImageNode)>,
    mut count_q: Query<(&QuickSlotCount, &mut Text)>,
    buff_root_q: Query<Entity, With<BuffRoot>>,
    pips_q: Query<Entity, With<BuffPip>>,
    toast_root_q: Query<Entity, With<ToastRoot>>,
    rows_q: Query<Entity, With<ToastRow>>,
) {
    let now = time.elapsed_secs() as f64;

    // Quick-slot icons.
    for (slot, mut node, mut img) in &mut icon_q {
        match slot_for(&inv, slot.0).and_then(|s| atlas.get(&s.item_id)) {
            Some(handle) => {
                img.image = handle;
                node.display = Display::Flex;
            }
            None => node.display = Display::None,
        }
    }
    // Quick-slot counts ("" if empty/single).
    for (slot, mut text) in &mut count_q {
        **text = match slot_for(&inv, slot.0) {
            Some(s) if s.count > 1 => format!("{}", s.count),
            _ => String::new(),
        };
    }

    // ── Buff pips: rebuild one column [icon + seconds] per active buff. ──
    for e in &pips_q {
        commands.entity(e).try_despawn();
    }
    if let Ok(root) = buff_root_q.single() {
        commands.entity(root).with_children(|bar| {
            for a in buffs.0.active_buffs(now) {
                bar.spawn((
                    BuffPip,
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(3.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                        border_radius: radius(R_BTN),
                        ..default()
                    },
                    BackgroundColor(rgba(20, 18, 24, 0.55)),
                ))
                .with_children(|pip| {
                    if let Some(h) = atlas.get(buff_icon_key(a.kind)) {
                        pip.spawn(widgets::icon(h, 18.0));
                    }
                    pip.spawn(label(&fonts.bold, format!("{:.0}s", a.remain), 10.0, GOLD));
                });
            }
        });
    }

    // ── Toasts: dismiss stale, rebuild one [icon + text] card per live toast. ──
    let expired: Vec<i64> =
        toasts.0.toasts().iter().filter(|t| now - t.born >= 4.0).map(|t| t.id).collect();
    for id in expired {
        toasts.0.remove(id);
    }
    for e in &rows_q {
        commands.entity(e).try_despawn();
    }
    if let Ok(root) = toast_root_q.single() {
        commands.entity(root).with_children(|col| {
            for tt in toasts.0.toasts() {
                let name = item_def(&tt.item_id).map(|d| d.name).unwrap_or(tt.item_id.as_str());
                col.spawn((
                    ToastRow,
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                        border: UiRect::left(Val::Px(3.0)),
                        border_radius: radius(7.0),
                        ..default()
                    },
                    BackgroundColor(rgba(20, 22, 28, 0.9)),
                    BorderColor::all(GREEN),
                    shadow_hud(),
                    anim(AnimKind::ToastIn, 0.0, 0.18),
                ))
                .with_children(|row| {
                    if let Some(h) = atlas.get(&tt.item_id) {
                        row.spawn(widgets::icon(h, 26.0));
                    }
                    row.spawn(label(&fonts.extrabold, format!("+{} {}", tt.count, name), 14.0, rgb(242, 244, 250)));
                });
            }
        });
    }
}

#[allow(clippy::type_complexity)]
/// Drive the top-right TOWN panel: headcount vs cap, the daily food balance (production − upkeep),
/// the settle/starve meter, and a one-line hint on what's happening / what to do. Makes the
/// food→population loop legible so the player understands why peasants arrive or leave.
fn update_town_panel(
    town: Res<crate::town::TownRes>,
    mut q_pop: Query<&mut Text, (With<PopText>, Without<PopNetText>, Without<PopHintText>)>,
    mut q_net: Query<(&mut Text, &mut TextColor), (With<PopNetText>, Without<PopText>, Without<PopHintText>)>,
    mut q_hint: Query<(&mut Text, &mut TextColor), (With<PopHintText>, Without<PopText>, Without<PopNetText>)>,
    mut q_bar: Query<(&mut Node, &mut BackgroundColor), With<PopBar>>,
) {
    let t = &town.0;
    let (pop, cap) = (t.population, t.pop_cap());
    let net = t.net_food();
    let frac = t.growth_fraction();

    let green = Color::srgb(0.45, 0.92, 0.5);
    let red = Color::srgb(1.0, 0.46, 0.38);
    let grey = Color::srgb(0.66, 0.70, 0.75);
    let amber = Color::srgb(1.0, 0.80, 0.36);

    if let Ok(mut text) = q_pop.single_mut() {
        **text = format!("\u{1f465} Town  {pop} / {cap}");
    }
    if let Ok((mut text, mut col)) = q_net.single_mut() {
        if net.abs() < 0.01 {
            **text = "Food: balanced".into();
            col.0 = grey;
        } else if net > 0.0 {
            **text = format!("Food: +{:.2}/s surplus", net);
            col.0 = green;
        } else {
            **text = format!("Food: {:.2}/s short", net);
            col.0 = red;
        }
    }
    if let Ok((mut node, mut col)) = q_bar.single_mut() {
        node.width = Val::Percent((frac.abs() * 100.0).clamp(0.0, 100.0) as f32);
        col.0 = if frac >= 0.0 { green } else { red };
    }
    if let Ok((mut text, mut col)) = q_hint.single_mut() {
        let (msg, c) = if net < -0.001 {
            ("\u{2193} losing a peasant \u{2014} build & staff a Farm", red)
        } else if pop >= cap && net > 0.001 {
            ("full \u{2014} build a House for room", amber)
        } else if net > 0.001 && pop < cap {
            ("\u{2191} a peasant is settling in", green)
        } else {
            ("steady", grey)
        };
        **text = msg.into();
        col.0 = c;
    }
}

fn update_hud(
    player: Res<PlayerRes>,
    bank: Res<crate::economy::Bank>,
    hero_q: Query<&HeroHealth>,
    mut hp_q: Query<&mut Node, (With<HpFill>, Without<StaminaFill>, Without<XpFill>)>,
    mut st_q: Query<&mut Node, (With<StaminaFill>, Without<HpFill>, Without<XpFill>)>,
    mut xp_q: Query<&mut Node, (With<XpFill>, Without<HpFill>, Without<StaminaFill>)>,
    mut hp_txt: Query<&mut Text, (With<HpText>, Without<LevelText>, Without<GoldText>, Without<StoneText>, Without<FoodText>, Without<WoodText>)>,
    mut lvl_txt: Query<&mut Text, (With<LevelText>, Without<HpText>, Without<GoldText>, Without<StoneText>, Without<FoodText>, Without<WoodText>)>,
    mut gold_txt: Query<&mut Text, (With<GoldText>, Without<HpText>, Without<LevelText>, Without<StoneText>, Without<FoodText>, Without<WoodText>)>,
    mut stone_txt: Query<&mut Text, (With<StoneText>, Without<HpText>, Without<LevelText>, Without<GoldText>, Without<FoodText>, Without<WoodText>)>,
    mut food_txt: Query<&mut Text, (With<FoodText>, Without<HpText>, Without<LevelText>, Without<GoldText>, Without<StoneText>, Without<WoodText>)>,
    mut wood_txt: Query<&mut Text, (With<WoodText>, Without<HpText>, Without<LevelText>, Without<GoldText>, Without<StoneText>, Without<FoodText>)>,
) {
    let Ok(hh) = hero_q.single() else { return };
    let p = &player.0;
    let hp = (p.hp / p.max_hp * 100.0).clamp(0.0, 100.0) as f32;
    let st = (hh.stamina / hh.stamina_max * 100.0).clamp(0.0, 100.0);
    let xp = if p.xp_to_next > 0 {
        (p.xp as f32 / p.xp_to_next as f32 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    if let Ok(mut n) = hp_q.single_mut() {
        n.width = Val::Percent(hp);
    }
    if let Ok(mut n) = st_q.single_mut() {
        n.width = Val::Percent(st);
    }
    if let Ok(mut n) = xp_q.single_mut() {
        n.width = Val::Percent(xp);
    }
    if let Ok(mut t) = hp_txt.single_mut() {
        **t = format!("{}", p.hp.max(0.0) as i64);
    }
    if let Ok(mut t) = lvl_txt.single_mut() {
        **t = format!("{}", p.level);
    }
    if let Ok(mut t) = gold_txt.single_mut() {
        **t = format!("Gold {}", p.gold);
    }
    if let Ok(mut t) = stone_txt.single_mut() {
        **t = format!("Stone {}", bank.0.stone() as i64);
    }
    if let Ok(mut t) = food_txt.single_mut() {
        **t = format!("Food {}", bank.0.food() as i64);
    }
    if let Ok(mut t) = wood_txt.single_mut() {
        **t = format!("Wood {}", bank.0.wood() as i64);
    }
}
