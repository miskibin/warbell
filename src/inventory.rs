//! **Inventory, buffs & pickup toasts** — the reward sink the biome verbs fill and the
//! combat hot-paths read. Three thin resources wrap the test-gated core stores:
//! [`Inventory`] over `tileworld_core::inventory::Bag` (24-slot bag + weapon/armor equip),
//! [`Buffs`] over `buff_store::BuffStore` (resist/power/haste timers), [`Toasts`] over
//! `item_toast_store::ToastStack` (the "you picked up X" stack).
//!
//! What lives here: the **quick-bar** (Q eat food, Z/X/C use a resist/power/haste item),
//! the shared [`try_grant`] pickup helper every verb calls, and the fresh-run reset. The
//! combat wiring (weapon bonus into the swing, armor + resist into incoming damage, power
//! into dealt, haste into move-speed) lives at each call site — `player/combat.rs`,
//! `player/health.rs`, `player/movement.rs` — reading these resources directly.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use tileworld_core::buff_store::{BuffKind, BuffStore};
use tileworld_core::inventory::{item_def, Bag, ConsumeEffect};
use tileworld_core::item_toast_store::ToastStack;
use tileworld_core::player::Player;

use crate::audio::AudioCue;
use crate::game_state::{AppState, Modal};
use crate::player::{PlayMode, PlayerRes};
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

/// The hero's 24-slot bag + two equip slots (the combat-read weapon bonus / armor mult live
/// inside). Filled by ore/forage/chest/hunt pickups and the shop; drained by the quick-bar.
#[derive(Resource, Default)]
pub struct Inventory(pub Bag);

/// Active timed buffs (resist/power/haste). Lazy-expiry: the multipliers read `now` each
/// frame, so no tick system is needed — just a reset on a new run.
#[derive(Resource, Default)]
pub struct Buffs(pub BuffStore);

/// The pickup-toast stack (HUD reads it; [`try_grant`] pushes to it).
#[derive(Resource, Default)]
pub struct Toasts(pub ToastStack);

/// The combat read-mods + swing publisher bundled into one `SystemParam`, so the swing system
/// (already at Bevy's 16-param ceiling) spends a single slot on the equipped-weapon bonus, the
/// power buff and the [`crate::verbs::HeroSwing`] broadcast that ore/dummies read.
#[derive(SystemParam)]
pub struct CombatMods<'w> {
    inv: Res<'w, Inventory>,
    buffs: Res<'w, Buffs>,
    swings: MessageWriter<'w, crate::verbs::HeroSwing>,
    kills: MessageWriter<'w, crate::verbs::AnimalKilled>,
}

impl CombatMods<'_> {
    /// Equipped weapon's flat damage bonus (0 = fists).
    pub fn weapon_bonus(&self) -> f64 {
        self.inv.0.weapon_bonus()
    }
    /// Outgoing-damage multiplier from an active Power buff (1.0 = none).
    pub fn power_mult(&self, now: f64) -> f64 {
        self.buffs.0.damage_dealt_mult(now)
    }
    /// Broadcast this swing's cone so the biome verbs (ore mining, training dummies) can react
    /// to the same blow. `base_dmg` is the non-crit damage (ore/dummies don't crit).
    pub fn publish_swing(&mut self, origin: Vec2, fwd: Vec2, base_dmg: f32) {
        self.swings.write(crate::verbs::HeroSwing { origin, fwd, base_dmg });
    }
    /// Announce a slain wild animal so the verbs layer rolls + spawns its loot drops.
    pub fn publish_animal_kill(&mut self, at: Vec3, species: crate::critters::Species) {
        self.kills.write(crate::verbs::AnimalKilled { at, species });
    }
}

/// Drop `count` of item `id` into the bag and fire a pickup toast on success. Returns whether
/// the bag accepted it — `false` means the bag was full, so the caller can leave the source
/// (forage plant / ground drop) intact to retry. The single insertion path every verb uses.
pub fn try_grant(bag: &mut Bag, toasts: &mut ToastStack, id: &str, count: i64, now: f64) -> bool {
    if bag.add(id, count) {
        toasts.push(id, count, now);
        true
    } else {
        false
    }
}

/// Enact a consumable's returned effect against the live hero: heal + grant its timed buff.
pub fn apply_consume(eff: &ConsumeEffect, player: &mut Player, buffs: &mut BuffStore, now: f64) {
    if eff.heal > 0.0 {
        player.heal(eff.heal);
    }
    if let Some((kind, duration_ms, mag)) = eff.buff {
        buffs.apply_buff(kind, duration_ms, mag, now);
    }
}

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inventory>()
            .init_resource::<Buffs>()
            .init_resource::<Toasts>()
            .add_systems(Startup, (debug_seed, debug_equip))
            // Fresh run wipes bag, buffs and toasts (with the rest of progression).
            .add_systems(OnExit(AppState::StartScreen), reset_inventory)
            .add_systems(OnExit(AppState::GameOver), reset_inventory)
            // Pause-menu Restart / Load also begins a fresh run (gated; see game_state).
            .add_systems(
                OnExit(AppState::Paused),
                reset_inventory.run_if(crate::game_state::restart_requested),
            )
            // Quick-bar (Q/Z/X/C) + open the satchel (I): only while playing with no panel up.
            .add_systems(Update, (quickbar_input, open_inventory).run_if(in_state(Modal::None)))
            // The satchel modal (freezes the world like the tree).
            .add_systems(OnEnter(Modal::Inventory), spawn_inventory_panel)
            .add_systems(OnExit(Modal::Inventory), despawn_inventory_panel)
            .add_systems(Update, inv_panel_interact.run_if(in_state(Modal::Inventory)));
    }
}

/// Screenshot hook: `FOREST_PANEL=inv` seeds a sample bag so the satchel + quick-bar render
/// with content under the capture harness. No effect in normal play.
fn debug_seed(mut inv: ResMut<Inventory>) {
    if std::env::var("FOREST_PANEL").ok().as_deref() == Some("inv") {
        for (id, n) in [("bread", 3), ("potion", 2), ("fur", 1), ("sword_iron", 1), ("leather_armor", 1), ("apple", 4)] {
            inv.0.add(id, n);
        }
    }
}

/// Screenshot/staging hook: `FOREST_EQUIP="sword_gold,gold_armor"` equips the listed gear at
/// startup (each id is dropped in the bag then equipped) so a shot can frame the hero with its
/// weapon/armor reflected on the model. Runs before `spawn_hero` (Startup < PostStartup), so the
/// knight builds already-geared. No effect in normal play.
fn debug_equip(mut inv: ResMut<Inventory>) {
    let Ok(list) = std::env::var("FOREST_EQUIP") else { return };
    for id in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !inv.0.add(id, 1) {
            continue;
        }
        if let Some(i) = inv.0.bag.iter().position(|s| s.item_id.as_deref() == Some(id)) {
            inv.0.activate_bag_item(i);
        }
    }
}

fn reset_inventory(mut inv: ResMut<Inventory>, mut buffs: ResMut<Buffs>, mut toasts: ResMut<Toasts>) {
    inv.0.reset();
    buffs.0.reset();
    toasts.0.reset();
}

/// Q eats the next food; Z/X/C use the next resist/power/haste item. Each surfaces the next
/// matching bag item (the core `eat_food`/`activate_buff` derive the quick-slot), heals +
/// applies its buff, and blips a confirm. No-op when the matching slot is empty.
fn quickbar_input(
    time: Res<Time>,
    mode: Res<PlayMode>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<Inventory>,
    mut buffs: ResMut<Buffs>,
    mut player: ResMut<PlayerRes>,
    mut cues: MessageWriter<AudioCue>,
) {
    if *mode != PlayMode::Play {
        return;
    }
    let used = if keys.just_pressed(KeyCode::KeyQ) {
        inv.0.eat_food()
    } else if keys.just_pressed(KeyCode::KeyZ) {
        inv.0.activate_buff(BuffKind::Resist)
    } else if keys.just_pressed(KeyCode::KeyX) {
        inv.0.activate_buff(BuffKind::Power)
    } else if keys.just_pressed(KeyCode::KeyC) {
        inv.0.activate_buff(BuffKind::Haste)
    } else {
        None
    };
    if let Some(eff) = used {
        apply_consume(&eff, &mut player.0, &mut buffs.0, time.elapsed_secs() as f64);
        cues.write(AudioCue::UiSelect);
    }
}

// ─── Satchel modal (I) ─────────────────────────────────────────────────────────────

#[derive(Component)]
struct InvUi;
/// A clickable bag row, tagged with its bag-slot index (use/equip on click).
#[derive(Component)]
struct InvSlotButton(usize);

/// Open the satchel with **I** (or the `FOREST_PANEL=inv` screenshot hook). Esc closes it
/// (via the shared `pause_toggle`), like the upgrade tree.
fn open_inventory(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<Modal>>,
    mut auto_done: Local<bool>,
) {
    let force = !*auto_done && std::env::var("FOREST_PANEL").ok().as_deref() == Some("inv");
    if force {
        *auto_done = true;
    }
    if keys.just_pressed(KeyCode::KeyI) || force {
        next.set(Modal::Inventory);
    }
}

fn spawn_inventory_panel(
    mut commands: Commands,
    inv: Res<Inventory>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
) {
    build_inv_panel(&mut commands, &inv.0, &fonts, &atlas);
}

fn despawn_inventory_panel(mut commands: Commands, q: Query<Entity, With<InvUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// (Re)build the satchel panel: an equipped-gear column beside a 6-wide bag grid. Each occupied
/// cell is clickable (use/equip). Called on open and after every action. Ported from `InventoryPanel`.
fn build_inv_panel(commands: &mut Commands, bag: &Bag, fonts: &UiFonts, atlas: &IconAtlas) {
    let weapon = bag
        .equipped_id
        .as_deref()
        .and_then(item_def)
        .map(|d| format!("{} (+{} atk)", d.name, d.damage_bonus as i64))
        .unwrap_or_else(|| "fists".into());
    let armor = bag
        .equipped_armor_id
        .as_deref()
        .and_then(item_def)
        .map(|d| format!("{} (-{}% dmg)", d.name, (d.defense * 100.0).round() as i64))
        .unwrap_or_else(|| "none".into());

    commands.spawn((widgets::scrim(60), InvUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(440.0),
                row_gap: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(26.0), Val::Px(22.0)),
                border: border(1.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            anim(AnimKind::PopIn, 0.0, 0.26),
        ))
        .with_children(|card| {
            // Header.
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::bottom(Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            })
            .insert(BorderColor::all(BORDER_SOFT))
            .with_children(|h| {
                h.spawn(label(&fonts.bold, "SATCHEL", 18.0, TEXT));
            });

            // Body: equipment column + bag grid.
            card.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(18.0), ..default() })
                .with_children(|body| {
                    // Equipment.
                    body.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        min_width: Val::Px(170.0),
                        ..default()
                    })
                    .with_children(|eq| {
                        eq.spawn(label(&fonts.semibold, "EQUIPPED", 10.0, GREY));
                        for (kind, val) in [("Weapon", &weapon), ("Armor", &armor)] {
                            eq.spawn((
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(1.0),
                                    padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
                                    border: border(1.0),
                                    border_radius: radius(R_BTN),
                                    ..default()
                                },
                                BackgroundColor(BTN_BG),
                                BorderColor::all(BORDER_SOFT),
                                children![
                                    label(&fonts.semibold, kind, 9.0, GREY),
                                    label(&fonts.semibold, val.clone(), 13.0, TEXT),
                                ],
                            ));
                        }
                    });

                    // Bag grid (6-wide wrap of 46px cells).
                    body.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|bagcol| {
                        bagcol.spawn(label(&fonts.semibold, "BAG", 10.0, GREY));
                        bagcol
                            .spawn(Node {
                                width: Val::Px(311.0),
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: Val::Px(5.0),
                                row_gap: Val::Px(5.0),
                                ..default()
                            })
                            .with_children(|grid| {
                                let mut any = false;
                                for (i, slot) in bag.bag.iter().enumerate() {
                                    let Some(id) = slot.item_id.as_deref() else { continue };
                                    any = true;
                                    grid.spawn((
                                        Node {
                                            width: Val::Px(46.0),
                                            height: Val::Px(46.0),
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::Center,
                                            border: border(1.0),
                                            border_radius: radius(R_CELL),
                                            ..default()
                                        },
                                        widgets::slot_paint(),
                                        InvSlotButton(i),
                                    ))
                                    .with_children(|cell| {
                                        if let Some(handle) = atlas.get(id) {
                                            cell.spawn(widgets::icon(handle, 28.0));
                                        }
                                        if slot.count > 1 {
                                            cell.spawn((
                                                Node { position_type: PositionType::Absolute, right: Val::Px(2.0), bottom: Val::Px(0.0), ..default() },
                                                label(&fonts.extrabold, format!("{}", slot.count), 12.0, Color::WHITE),
                                                TextShadow { offset: Vec2::ZERO, color: rgba(0, 0, 0, 0.9) },
                                            ));
                                        }
                                    });
                                }
                                if !any {
                                    grid.spawn(label(&fonts.regular, "Empty — forage, mine and hunt.", 13.0, GREY));
                                }
                            });
                    });
                });

            card.spawn(label(&fonts.regular, "I or Esc to close  ·  click an item to use or equip", 11.0, GREY));
        });
    });
}

/// Click a bag row → use the consumable (heal + buff) or equip the gear, then rebuild the panel
/// so the slots re-index. Mirrors the tree's click-to-buy.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn inv_panel_interact(
    time: Res<Time>,
    mut inv: ResMut<Inventory>,
    mut buffs: ResMut<Buffs>,
    mut player: ResMut<PlayerRes>,
    mut cues: MessageWriter<AudioCue>,
    mut commands: Commands,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    buttons: Query<(&Interaction, &InvSlotButton), Changed<Interaction>>,
    panel: Query<Entity, With<InvUi>>,
) {
    let mut acted = false;
    for (interaction, btn) in &buttons {
        if *interaction == Interaction::Pressed {
            if let Some(eff) = inv.0.activate_bag_item(btn.0) {
                apply_consume(&eff, &mut player.0, &mut buffs.0, time.elapsed_secs() as f64);
            }
            cues.write(AudioCue::UiSelect);
            acted = true;
            break;
        }
    }
    if acted {
        for e in &panel {
            commands.entity(e).despawn();
        }
        build_inv_panel(&mut commands, &inv.0, &fonts, &atlas);
    }
}
