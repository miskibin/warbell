//! Minimal combat HUD — an HP bar over a thinner block-stamina bar, bottom-left. Plain
//! `bevy_ui` rectangles bound to the hero's `HeroHealth`; no text, no chrome.

use bevy::prelude::*;
use tileworld_core::buff_store::BuffKind;
use tileworld_core::inventory::{item_def, QuickSlot};

use crate::inventory::{Buffs, Inventory, Toasts};
use crate::player::{HeroHealth, PlayerRes};

#[derive(Component)]
struct HpFill;
#[derive(Component)]
struct StaminaFill;
#[derive(Component)]
struct XpFill;
#[derive(Component)]
struct ResourceText;
/// Pickup-toast column (top-right).
#[derive(Component)]
struct ToastText;
/// Quick-bar + active-buff line (bottom-centre).
#[derive(Component)]
struct QuickText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_hud, setup_inv_hud))
            .add_systems(Update, (update_hud, update_inv_hud));
    }
}

fn setup_hud(mut commands: Commands) {
    let track_bg = Color::srgba(0.0, 0.0, 0.0, 0.55);
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(18.0),
            width: Val::Px(240.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|root| {
            // Level + gold + stone readout (numeric).
            root.spawn((
                Text::new("Lv 1   Gold 30   Stone 0"),
                TextFont { font_size: 18.0, ..default() },
                TextColor(Color::srgb(0.96, 0.86, 0.45)),
                ResourceText,
            ));
            // HP track + fill.
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(16.0),
                    padding: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(track_bg),
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(Color::srgb(0.85, 0.22, 0.22)),
                    HpFill,
                ));
            });
            // Block-stamina track + fill (thinner).
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(9.0),
                    padding: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(track_bg),
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(Color::srgb(0.92, 0.78, 0.30)),
                    StaminaFill,
                ));
            });
            // XP track + fill (thin, blue — fills toward the next level).
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(7.0),
                    padding: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(track_bg),
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(Color::srgb(0.42, 0.7, 1.0)),
                    XpFill,
                ));
            });
        });
}

/// Pickup toasts (top-right) + the quick-bar/buff line (bottom-centre).
fn setup_inv_hud(mut commands: Commands) {
    // Top-right pickup toasts.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            right: Val::Px(18.0),
            top: Val::Px(18.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextFont { font_size: 18.0, ..default() },
                TextColor(Color::srgb(0.95, 0.86, 0.5)),
                ToastText,
            ));
        });
    // Bottom-centre quick-bar.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(16.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextFont { font_size: 16.0, ..default() },
                TextColor(Color::srgb(0.88, 0.9, 0.95)),
                QuickText,
            ));
        });
}

/// Render the pickup-toast stack (auto-dismissing past 4s) + the four quick-slots and any
/// active buff timers. Polls the inventory/buff/toast resources each frame.
#[allow(clippy::type_complexity)]
fn update_inv_hud(
    time: Res<Time>,
    inv: Res<Inventory>,
    buffs: Res<Buffs>,
    mut toasts: ResMut<Toasts>,
    mut toast_q: Query<&mut Text, (With<ToastText>, Without<QuickText>)>,
    mut quick_q: Query<&mut Text, (With<QuickText>, Without<ToastText>)>,
) {
    let now = time.elapsed_secs() as f64;

    // Auto-dismiss toasts older than 4s.
    let expired: Vec<i64> =
        toasts.0.toasts().iter().filter(|t| now - t.born >= 4.0).map(|t| t.id).collect();
    for id in expired {
        toasts.0.remove(id);
    }
    if let Ok(mut t) = toast_q.single_mut() {
        **t = toasts
            .0
            .toasts()
            .iter()
            .map(|tt| {
                let name = item_def(&tt.item_id).map(|d| d.name).unwrap_or(tt.item_id.as_str());
                format!("+{} {}", tt.count, name)
            })
            .collect::<Vec<_>>()
            .join("\n");
    }

    if let Ok(mut q) = quick_q.single_mut() {
        let label = |s: Option<QuickSlot>| {
            s.map(|q| {
                let name = item_def(&q.item_id).map(|d| d.name).unwrap_or(q.item_id.as_str());
                format!("{} x{}", name, q.count)
            })
            .unwrap_or_else(|| "-".into())
        };
        let mut line = format!(
            "[Q] {}    [Z] {}    [X] {}    [C] {}",
            label(inv.0.food_slot()),
            label(inv.0.buff_slot(BuffKind::Resist)),
            label(inv.0.buff_slot(BuffKind::Power)),
            label(inv.0.buff_slot(BuffKind::Haste)),
        );
        let active = buffs.0.active_buffs(now);
        if !active.is_empty() {
            let bits: Vec<String> =
                active.iter().map(|a| format!("{} {:.0}s", a.kind.label(), a.remain)).collect();
            line.push('\n');
            line.push_str(&bits.join("    "));
        }
        **q = line;
    }
}

#[allow(clippy::type_complexity)]
fn update_hud(
    player: Res<PlayerRes>,
    bank: Res<crate::economy::Bank>,
    lives: Res<crate::succession::Lives>,
    hero_q: Query<&HeroHealth>,
    mut hp_q: Query<&mut Node, (With<HpFill>, Without<StaminaFill>, Without<XpFill>)>,
    mut st_q: Query<&mut Node, (With<StaminaFill>, Without<HpFill>, Without<XpFill>)>,
    mut xp_q: Query<&mut Node, (With<XpFill>, Without<HpFill>, Without<StaminaFill>)>,
    mut txt_q: Query<&mut Text, With<ResourceText>>,
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
    if let Ok(mut t) = txt_q.single_mut() {
        **t = format!(
            "Lv {}   Gold {}   Stone {}   Heirs {}",
            p.level,
            p.gold,
            bank.0.stone() as i64,
            lives.heirs
        );
    }
}
