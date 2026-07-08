//! **Inventory, buffs & pickup toasts** — the reward sink the biome verbs fill and the
//! combat hot-paths read. Three thin resources wrap the test-gated core stores:
//! [`Inventory`] over `tileworld_core::inventory::Bag` (24-slot bag + weapon/armor equip),
//! [`Buffs`] over `buff_store::BuffStore` (resist/power/haste timers), [`Toasts`] over
//! `item_toast_store::ToastStack` (the "you picked up X" stack).
//!
//! What lives here: the **quick-bar** (Q eat food, Y/T use a resist/power item),
//! the shared [`try_grant`] pickup helper every verb calls, and the fresh-run reset. The
//! combat wiring (weapon bonus into the swing, armor + resist into incoming damage, power
//! into dealt, haste into move-speed) lives at each call site — `player/combat.rs`,
//! `player/health.rs`, `player/movement.rs` — reading these resources directly.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use tileworld_core::buff_store::{BuffKind, BuffStore};
use tileworld_core::inventory::{item_def, Bag, ConsumeEffect, ItemKind, QUICK_SLOTS};
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
use crate::game_state::SimAppExt;

/// A quick-slot fired this frame — HUD reads it to pop the matching cell. Index is the
/// quick-bar order: 0 = Q (food), 1 = Y, 2 = T.
#[derive(Message, Clone, Copy)]
pub struct QuickFlash(pub u8);

/// Letter shown for bindable slot `i` (0 = Y, 1 = T) — the key that uses/assigns it. (The
/// combat arts took Z/X/C, so the two quick-pots moved to Y/T.)
pub fn bind_slot_key(i: usize) -> char {
    ['Y', 'T'][i.min(1)]
}

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
        // `harvest_dmg` is the FLAT tree/ore damage, intentionally NOT `base_dmg` — harvesting is
        // decoupled from combat power so a maxed hero can't one-shot the forest (see `HeroSwing`).
        self.swings.write(crate::verbs::HeroSwing {
            origin,
            fwd,
            base_dmg,
            harvest_dmg: crate::verbs::HERO_HARVEST_DMG,
        });
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
/// Healing restores a little LESS than its listed value, and a *potion/elixir* heal also drains
/// stamina, so you can't freely potion-spam mid-fight — the same bar you block/art with pays for it.
/// **Eating food is exempt** (`eff.food`): snacking on bread/apples never costs stamina.
const HEAL_EFFECTIVENESS: f64 = 0.7; // restore 70% of the item's listed heal
const HEAL_STAMINA_COST: f32 = 35.0; // stamina spent per *potion* heal (~1/4 of the 150 base bar)

pub fn apply_consume(
    eff: &ConsumeEffect,
    player: &mut Player,
    buffs: &mut BuffStore,
    hero: &mut crate::player::HeroHealth,
    now: f64,
) {
    if eff.heal > 0.0 {
        player.heal(eff.heal * HEAL_EFFECTIVENESS);
        if !eff.food {
            // Potions/elixirs pay stamina; eating food does not.
            hero.stamina = (hero.stamina - HEAL_STAMINA_COST).max(0.0);
        }
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
            .init_resource::<InvPanelDirty>()
            .add_message::<QuickFlash>()
            .add_systems(Startup, (debug_seed, debug_equip, debug_bufftest))
            // Fresh run wipes bag, buffs and toasts (with the rest of progression).
            .add_systems(OnExit(AppState::StartScreen), reset_inventory)
            .add_systems(OnExit(AppState::GameOver), reset_inventory)
            // Every Continue path clears transient buffs/toasts (keyed off the load event).
            .add_systems(Update, clear_transients_on_load)
            // (Pause-menu Restart resets in-process via StartScreen → Playing — see
            // game_state::drive_fresh_run — so this OnExit(StartScreen) reset covers it; Load restores.)
            // Quick-bar (Q/Y/T) + open the satchel (Tab/I): only while playing with no panel up.
            .add_sim_systems((quickbar_input, open_inventory))
            // The satchel modal (freezes the world like the tree).
            .add_systems(OnEnter(Modal::Inventory), spawn_inventory_panel)
            .add_systems(OnExit(Modal::Inventory), despawn_inventory_panel)
            .add_systems(
                Update,
                (
                    (inv_panel_interact, inv_assign_input, inv_drop_input).chain(),
                    // Single rebuild after the action handlers: each handler that mutates the bag
                    // sets `InvPanelDirty` rather than despawning+rebuilding inline, so two actions
                    // landing the same frame can't double-despawn the panel (panic) or spawn it twice.
                    rebuild_inv_panel,
                    inv_tooltip,
                    close_inventory,
                )
                    .chain()
                    .run_if(in_state(Modal::Inventory)),
            );
    }
}

/// Set when a satchel action changes the bag; `rebuild_inv_panel` consumes it once per frame.
#[derive(Resource, Default)]
struct InvPanelDirty(bool);

/// Screenshot hook: `FOREST_BUFFTEST=1` applies all three timed buffs at boot so a shot can frame
/// the buff HUD chips (icon + name + effect + countdown). No effect in normal play.
fn debug_bufftest(mut buffs: ResMut<Buffs>, time: Res<Time>) {
    if std::env::var("FOREST_BUFFTEST").is_err() {
        return;
    }
    let now = time.elapsed_secs() as f64;
    buffs.0.apply_buff(BuffKind::Power, 45_000.0, 1.4, now);
    buffs.0.apply_buff(BuffKind::Resist, 45_000.0, 0.6, now);
    buffs.0.apply_buff(BuffKind::Haste, 45_000.0, 1.3, now);
}

/// Screenshot hook: `FOREST_PANEL=inv` seeds a sample bag so the satchel + quick-bar render
/// with content under the capture harness. No effect in normal play.
fn debug_seed(mut inv: ResMut<Inventory>) {
    if std::env::var("FOREST_PANEL").ok().as_deref() == Some("inv") {
        for (id, n) in [("bread", 3), ("potion", 2), ("fur", 1), ("sword_iron", 1), ("leather_armor", 1), ("apple", 4)] {
            inv.0.add(id, n); // fur auto-binds Y (slot 0)
        }
        inv.0.set_quick_bind(1, "potion"); // demo a manual bind: a heal pinned to T (slot 1)
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

/// Clear transient buffs + pickup toasts whenever a save loads. `GameLoaded` fires once per load
/// from `apply_pending_load`, so this catches EVERY Continue path (game-over, pause-menu Load,
/// start-screen mid-run) — not just the ones that route through an `OnExit` reset. Without it a
/// shrine/potion buff active when you opened the pause menu carried into the loaded run. The bag
/// itself is restored from the save by `apply_pending_load`, so it's left untouched here.
fn clear_transients_on_load(
    mut ev: MessageReader<crate::savegame::GameLoaded>,
    mut buffs: ResMut<Buffs>,
    mut toasts: ResMut<Toasts>,
) {
    if ev.read().last().is_some() {
        buffs.0.reset();
        toasts.0.reset();
    }
}

/// Q eats the next food; Y/T use their bound item (or, when unbound, the next
/// resist/power item — the core `use_quick_slot` derives that fallback). On a
/// successful use it heals + applies the buff, blips a confirm, and emits a [`QuickFlash`]
/// so the HUD pops the cell. No-op when the slot is empty/exhausted.
fn quickbar_input(
    time: Res<Time>,
    mode: Res<PlayMode>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<Inventory>,
    mut buffs: ResMut<Buffs>,
    mut player: ResMut<PlayerRes>,
    mut hero: Query<&mut crate::player::HeroHealth>,
    mut cues: MessageWriter<AudioCue>,
    mut flash: MessageWriter<QuickFlash>,
) {
    if *mode != PlayMode::Play {
        return;
    }
    let (used, slot) = if keys.just_pressed(KeyCode::KeyQ) {
        (inv.0.eat_food(), 0u8)
    } else if keys.just_pressed(KeyCode::KeyY) {
        (inv.0.use_quick_slot(0), 1)
    } else if keys.just_pressed(KeyCode::KeyT) {
        (inv.0.use_quick_slot(1), 2)
    } else {
        (None, 0)
    };
    if let Some(eff) = used {
        let Ok(mut hh) = hero.single_mut() else { return };
        apply_consume(&eff, &mut player.0, &mut buffs.0, &mut hh, time.elapsed_secs() as f64);
        cues.write(AudioCue::UiSelect);
        flash.write(QuickFlash(slot));
    }
}

// ─── Satchel modal (I) ─────────────────────────────────────────────────────────────

#[derive(Component)]
struct InvUi;
/// A clickable bag cell, tagged with its bag-slot index (use/equip on click, assign on Y/T).
#[derive(Component)]
struct InvSlotButton(usize);
/// A clickable EQUIPPED card — click takes the piece off and returns it to the bag.
/// `true` = weapon slot, `false` = armor.
#[derive(Component)]
struct UnequipButton(bool);
/// The header ✕ — closes the satchel like Tab/I/Esc.
#[derive(Component)]
struct InvCloseBtn;
/// The floating item tooltip (persists across panel rebuilds; separate from [`InvUi`]).
#[derive(Component)]
struct InvTooltip;
#[derive(Component)]
struct InvTipName;
#[derive(Component)]
struct InvTipStat;
#[derive(Component)]
struct InvTipCompare;

/// Open the satchel with **Tab** or **I** (or the `FOREST_PANEL=inv` screenshot hook). Esc / Tab /
/// I close it (Esc via the shared `pause_toggle`, Tab/I via `close_inventory`), like the tree.
fn open_inventory(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<Modal>>,
    mut auto_done: Local<bool>,
) {
    let force = !*auto_done && std::env::var("FOREST_PANEL").ok().as_deref() == Some("inv");
    if force {
        *auto_done = true;
    }
    if keys.just_pressed(KeyCode::KeyI) || keys.just_pressed(KeyCode::Tab) || force {
        next.set(Modal::Inventory);
    }
}

/// Make **Tab** / **I** a toggle: pressed while the satchel is open, they close it (Esc is
/// handled centrally by `pause_toggle`). The open press can't re-fire here because state only
/// flips to `Inventory` next frame, by which point the key is no longer `just_pressed`.
fn close_inventory(
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<Modal>>,
    btns: Query<&Interaction, (With<InvCloseBtn>, Changed<Interaction>)>,
) {
    let x_clicked = btns.iter().any(|i| *i == Interaction::Pressed);
    if keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::KeyI) || x_clicked {
        next.set(Modal::None);
    }
}

fn spawn_inventory_panel(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<crate::ui::texture::UiTextures>,
) {
    // Fold any duplicate stackable cells together on open (cleans up gear from before it stacked).
    inv.0.coalesce();
    build_inv_panel(&mut commands, &inv.0, &fonts, &atlas, &tex, true);
    spawn_tooltip(&mut commands, &fonts);
}

#[allow(clippy::type_complexity)]
fn despawn_inventory_panel(
    mut commands: Commands,
    q: Query<Entity, Or<(With<InvUi>, With<InvTooltip>)>>,
) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

/// The floating tooltip shell — hidden until `inv_tooltip` parks it at the cursor over a hovered
/// item. Spawned once per open (outside [`InvUi`]) so it survives the panel's rebuild-on-action.
fn spawn_tooltip(commands: &mut Commands, fonts: &UiFonts) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-9999.0), // parked off-screen until shown
                top: Val::Px(0.0),
                max_width: Val::Px(240.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::axes(Val::Px(11.0), Val::Px(9.0)),
                border: border(1.0),
                border_radius: radius(R_BTN),
                display: Display::None,
                ..default()
            },
            BackgroundColor(rgba(20, 16, 11, 0.97)),
            BorderColor::all(GOLD_HAIRLINE),
            shadow_card(),
            GlobalZIndex(90),
            bevy::ui::FocusPolicy::Pass,
            InvTooltip,
        ))
        .with_children(|t| {
            t.spawn((label(&fonts.bold, "", 14.0, TEXT), InvTipName));
            t.spawn((label(&fonts.semibold, "", 12.0, GREEN), InvTipStat));
            t.spawn((label(&fonts.regular, "", 11.0, TEXT_DIM), InvTipCompare));
        });
}

/// (Re)build the satchel panel: an equipped-gear column beside a 6-wide bag grid. Each occupied
/// cell is clickable (use/equip). Called on open and after every action. Ported from `InventoryPanel`.
fn build_inv_panel(
    commands: &mut Commands,
    bag: &Bag,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    tex: &crate::ui::texture::UiTextures,
    animate: bool, // pop-in on open only — action rebuilds must not re-play the entrance
) {
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
        let mut card_ec = root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(440.0),
                row_gap: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(26.0), Val::Px(22.0)),
                border: border(2.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
        ));
        if animate {
            card_ec.insert(anim(AnimKind::PopIn, 0.0, 0.26));
        }
        card_ec.with_children(|card| {
            widgets::chrome_layers(card, tex.linen.clone());
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
                h.spawn(label(&fonts.display, "SATCHEL", 17.0, GOLD));
                widgets::close_button(h, &fonts.bold, InvCloseBtn, false);
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
                        for (kind, val, id, is_weapon) in [
                            ("Weapon", &weapon, bag.equipped_id.as_deref(), true),
                            ("Armor", &armor, bag.equipped_armor_id.as_deref(), false),
                        ] {
                            let node = Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(10.0),
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                                border: border(1.0),
                                border_radius: radius(R_BTN),
                                ..default()
                            };
                            let text_col = |eq: &mut RelatedSpawnerCommands<ChildOf>,
                                            sub: &str,
                                            sub_col: Color| {
                                eq.spawn((
                                    Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(1.0),
                                        ..default()
                                    },
                                    children![
                                        label(&fonts.semibold, kind, 9.0, GREY),
                                        label(&fonts.semibold, val.clone(), 13.0, TEXT),
                                        label(&fonts.regular, sub.to_string(), 9.5, sub_col),
                                    ],
                                ));
                            };
                            if let Some(id) = id {
                                // Occupied: a real button — click takes the piece off.
                                eq.spawn((
                                    node,
                                    widgets::slot_paint(),
                                    crate::ui::focus::Focusable,
                                    UnequipButton(is_weapon),
                                ))
                                .with_children(|b| {
                                    if let Some(e) = atlas.get_tintable(id) {
                                        b.spawn(widgets::icon_tinted(e, 22.0, item_tint(id)));
                                    }
                                    text_col(b, "Click to unequip", rgba(255, 213, 140, 0.75));
                                });
                            } else {
                                eq.spawn((node, BackgroundColor(BTN_BG), BorderColor::all(BORDER_SOFT)))
                                    .with_children(|b| {
                                        text_col(b, "", Color::NONE);
                                    });
                            }
                        }
                    });

                    // Bag grid (6-wide wrap of 46px cells).
                    body.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|bagcol| {
                        // BAG header + a category legend so a glance tells gear from food.
                        bagcol
                            .spawn(Node {
                                width: Val::Px(311.0),
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                ..default()
                            })
                            .with_children(|hdr| {
                                hdr.spawn(label(&fonts.semibold, "BAG", 10.0, GREY));
                                hdr.spawn(Node {
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(10.0),
                                    ..default()
                                })
                                .with_children(|lg| {
                                    for (col, txt) in [(GOLD, "Wear"), (GREEN, "Use"), (TEXT_FAINT, "Key")] {
                                        lg.spawn(Node {
                                            flex_direction: FlexDirection::Row,
                                            align_items: AlignItems::Center,
                                            column_gap: Val::Px(4.0),
                                            ..default()
                                        })
                                        .with_children(|sw| {
                                            sw.spawn((
                                                Node { width: Val::Px(8.0), height: Val::Px(8.0), border_radius: radius(2.0), ..default() },
                                                BackgroundColor(col),
                                            ));
                                            sw.spawn(label(&fonts.semibold, txt, 9.0, GREY));
                                        });
                                    }
                                });
                            });
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
                                        crate::ui::focus::Focusable,
                                        InvSlotButton(i),
                                    ))
                                    .with_children(|cell| {
                                        // Category accent strip along the bottom edge: gold = wearable
                                        // gear, green = consumable, faint = key item (matches the legend).
                                        if let Some(k) = item_def(id).map(|d| d.kind) {
                                            cell.spawn((
                                                Node {
                                                    position_type: PositionType::Absolute,
                                                    left: Val::Px(0.0),
                                                    right: Val::Px(0.0),
                                                    bottom: Val::Px(0.0),
                                                    height: Val::Px(3.0),
                                                    ..default()
                                                },
                                                BackgroundColor(kind_accent_color(k)),
                                            ));
                                        }
                                        if let Some(entry) = atlas.get_tintable(id) {
                                            cell.spawn(widgets::icon_tinted(entry, 28.0, item_tint(id)));
                                        }
                                        if slot.count > 1 {
                                            cell.spawn((
                                                Node { position_type: PositionType::Absolute, right: Val::Px(2.0), bottom: Val::Px(0.0), ..default() },
                                                label(&fonts.extrabold, format!("{}", slot.count), 12.0, Color::WHITE),
                                                TextShadow { offset: Vec2::ZERO, color: rgba(0, 0, 0, 0.9) },
                                            ));
                                        }
                                        // Quick-bar badge: if this item is pinned to Y/T, stamp the
                                        // key in the corner so the binding is visible at a glance.
                                        if let Some(key) = bound_key_for(bag, id) {
                                            cell.spawn((
                                                Node {
                                                    position_type: PositionType::Absolute,
                                                    top: Val::Px(-3.0),
                                                    left: Val::Px(-3.0),
                                                    min_width: Val::Px(14.0),
                                                    align_items: AlignItems::Center,
                                                    justify_content: JustifyContent::Center,
                                                    padding: UiRect::axes(Val::Px(3.0), Val::Px(1.0)),
                                                    border_radius: radius(3.0),
                                                    ..default()
                                                },
                                                BackgroundColor(GOLD_DEEP),
                                                children![(
                                                    label(&fonts.extrabold, key.to_string(), 10.0, INK),
                                                )],
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

            card.spawn(label(
                &fonts.regular,
                "Tab/I or Esc to close  ·  click to use or equip  ·  hover: Y/T quick-slot, right-click or Del to drop",
                11.0,
                GREY,
            ));
        });
    });
}

/// The quick-bar key (Y/T) this item id is pinned to, if any — drives the cell badge. Only the two
/// bound slots the HUD/keys expose are scanned: core keeps a vestigial 3rd slot (Haste) for save
/// compat, but it has no key/cell, so a stale bind there must NOT render a phantom badge.
fn bound_key_for(bag: &Bag, id: &str) -> Option<char> {
    (0..QUICK_SLOTS.min(2)).find(|&i| bag.quick_bind(i) == Some(id)).map(bind_slot_key)
}

/// Cell accent colour by category — the bag legend's key: wearable gear (weapon/armor) reads
/// gold, consumables green, key items faint. Lets a glance separate what you equip from what
/// you use.
fn kind_accent_color(kind: ItemKind) -> Color {
    match kind {
        ItemKind::Weapon | ItemKind::Armor => GOLD,
        ItemKind::Consumable => GREEN,
        ItemKind::Token => TEXT_FAINT,
    }
}

/// Per-item display tint for the item icons (satchel grid, quick-bar, pickup toast, shop rows).
/// The shipped game-icons are white monochrome silhouettes, so untinted EVERY item reads as the
/// same colourless shape; we ink each with its core `IconSpec` primary hue (red apple, gold blade,
/// frost-blue greatsword, tan fur, green venom…) so the bag is readable at a glance. Channels are
/// lifted off the floor so a dark recipe channel still shows as a hue, not black, on the dark slot.
/// Passed through [`widgets::icon_tinted`], which leaves full-colour Twemoji rasters untouched, so
/// this only ever recolours the monochrome source.
pub fn item_tint(id: &str) -> Color {
    let (r, g, b) = item_def(id).map(|d| d.icon_spec().fg).unwrap_or((225, 225, 225));
    let ch = |c: u8| (c as f32 / 255.0).mul_add(0.7, 0.3);
    Color::srgb(ch(r), ch(g), ch(b))
}

/// Click a bag row → use the consumable (heal + buff) or equip the gear, then rebuild the panel
/// so the slots re-index. Mirrors the tree's click-to-buy.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn inv_panel_interact(
    time: Res<Time>,
    mut inv: ResMut<Inventory>,
    mut buffs: ResMut<Buffs>,
    mut player: ResMut<PlayerRes>,
    mut hero: Query<&mut crate::player::HeroHealth>,
    mut cues: MessageWriter<AudioCue>,
    mut dirty: ResMut<InvPanelDirty>,
    mut acts: MessageReader<crate::ui::focus::FocusActivate>,
    buttons: Query<(&Interaction, &InvSlotButton), Changed<Interaction>>,
    slots: Query<&InvSlotButton>,
    unequips: Query<(&Interaction, &UnequipButton), Changed<Interaction>>,
    unequip_all: Query<&UnequipButton>,
) {
    // A real click, or Enter/E on the focused cell (see ui::focus) — one shared use path.
    let keyed_acts: Vec<Entity> = acts.read().map(|a| a.0).collect();
    let clicked = buttons
        .iter()
        .find(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, b)| b.0);
    let keyed = keyed_acts.iter().find_map(|e| slots.get(*e).ok()).map(|b| b.0);
    let unequip = unequips
        .iter()
        .find(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, b)| b.0)
        .or_else(|| keyed_acts.iter().find_map(|e| unequip_all.get(*e).ok()).map(|b| b.0));
    let mut acted = false;
    if let Some(slot) = clicked.or(keyed) {
        if let Some(eff) = inv.0.activate_bag_item(slot) {
            if let Ok(mut hh) = hero.single_mut() {
                apply_consume(&eff, &mut player.0, &mut buffs.0, &mut hh, time.elapsed_secs() as f64);
            }
        }
        cues.write(AudioCue::UiSelect);
        acted = true;
    } else if let Some(is_weapon) = unequip {
        if is_weapon {
            inv.0.unequip_weapon();
        } else {
            inv.0.unequip_armor();
        }
        cues.write(AudioCue::UiSelect);
        acted = true;
    }
    if acted {
        dirty.0 = true;
    }
}

/// Despawn-and-rebuild the satchel panel once per frame if any action handler dirtied the bag.
/// Centralised so concurrent actions can't race the despawn (see `InvPanelDirty`).
#[allow(clippy::too_many_arguments)]
fn rebuild_inv_panel(
    mut dirty: ResMut<InvPanelDirty>,
    mut commands: Commands,
    inv: Res<Inventory>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<crate::ui::texture::UiTextures>,
    panel: Query<Entity, With<InvUi>>,
) {
    if !dirty.0 {
        return;
    }
    dirty.0 = false;
    for e in &panel {
        commands.entity(e).try_despawn();
    }
    build_inv_panel(&mut commands, &inv.0, &fonts, &atlas, &tex, false);
}

/// The bag index currently under the cursor (hovered or pressed), if any. Shared by the
/// tooltip and the assign-key handler so both react to the same cell.
fn hovered_slot(buttons: &Query<(&Interaction, &InvSlotButton)>) -> Option<usize> {
    buttons
        .iter()
        .find(|(i, _)| !matches!(**i, Interaction::None))
        .map(|(_, b)| b.0)
}

/// Hover a bag item and press **Y / T** to pin it to that quick-slot (consumables only).
/// Rebuilds the panel so the new key badge shows. The same physical keys *use* a slot in
/// `Modal::None`; here, inside the satchel, they *assign* — no conflict (different modal).
#[allow(clippy::too_many_arguments)]
fn inv_assign_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<Inventory>,
    mut cues: MessageWriter<AudioCue>,
    mut dirty: ResMut<InvPanelDirty>,
    buttons: Query<(&Interaction, &InvSlotButton)>,
) {
    let slot = if keys.just_pressed(KeyCode::KeyY) {
        0
    } else if keys.just_pressed(KeyCode::KeyT) {
        1
    } else {
        return;
    };
    let Some(idx) = hovered_slot(&buttons) else { return };
    let Some(id) = inv.0.bag.get(idx).and_then(|s| s.item_id.clone()) else { return };
    if !inv.0.set_quick_bind(slot, &id) {
        return; // not a consumable — nothing to assign
    }
    cues.write(AudioCue::UiSelect);
    dirty.0 = true;
}

/// Hover a bag item and **right-click** (or press **Delete**) to throw one away — the quick way to
/// clear gear you don't want when no merchant is near. Rebuilds the panel so the cell updates.
/// Selling for gold lives at the merchant shop; this just discards. (RMB is free here: the only
/// other RMB users — fly-cam look, hero block — don't run while a panel freezes the world / in Play
/// mode.)
#[allow(clippy::too_many_arguments)]
fn inv_drop_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut inv: ResMut<Inventory>,
    mut cues: MessageWriter<AudioCue>,
    mut dirty: ResMut<InvPanelDirty>,
    buttons: Query<(&Interaction, &InvSlotButton)>,
) {
    if !(mouse.just_pressed(MouseButton::Right) || keys.just_pressed(KeyCode::Delete)) {
        return;
    }
    let Some(idx) = hovered_slot(&buttons) else { return };
    let Some(id) = inv.0.bag.get(idx).and_then(|s| s.item_id.clone()) else { return };
    if !inv.0.consume_item(&id, 1) {
        return;
    }
    cues.write(AudioCue::UiSelect);
    dirty.0 = true;
}

/// Park the floating tooltip at the cursor over the hovered bag item, filling name + stat +
/// an action line (equip delta for gear, the assign hint for consumables). Hidden when nothing
/// is hovered.
#[allow(clippy::type_complexity)]
fn inv_tooltip(
    inv: Res<Inventory>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Query<(&Interaction, &InvSlotButton)>,
    mut tip: Query<&mut Node, With<InvTooltip>>,
    mut name_q: Query<&mut Text, (With<InvTipName>, Without<InvTipStat>, Without<InvTipCompare>)>,
    mut stat_q: Query<&mut Text, (With<InvTipStat>, Without<InvTipName>, Without<InvTipCompare>)>,
    mut cmp_q: Query<&mut Text, (With<InvTipCompare>, Without<InvTipName>, Without<InvTipStat>)>,
) {
    let Ok(mut node) = tip.single_mut() else { return };
    let hovered = hovered_slot(&buttons)
        .and_then(|i| inv.0.bag.get(i))
        .and_then(|s| s.item_id.as_deref())
        .and_then(item_def);
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());

    let Some((def, pos)) = hovered.zip(cursor) else {
        node.display = Display::None;
        node.left = Val::Px(-9999.0);
        return;
    };

    if let Ok(mut t) = name_q.single_mut() {
        **t = def.name.to_string();
    }
    if let Ok(mut t) = stat_q.single_mut() {
        **t = def.stat_line();
    }
    if let Ok(mut t) = cmp_q.single_mut() {
        **t = match def.kind {
            ItemKind::Consumable => "Y / T  set quick-slot".to_string(),
            ItemKind::Weapon => {
                let cur = inv.0.weapon_bonus() as i64;
                format!("Equip: +{} atk  (current +{})", def.damage_bonus as i64, cur)
            }
            ItemKind::Armor => {
                let cur = ((1.0 - inv.0.armor_damage_mult()) * 100.0).round() as i64;
                format!("Equip: -{}% dmg taken  (current -{}%)", (def.defense * 100.0).round() as i64, cur)
            }
            ItemKind::Token => String::new(),
        };
    }
    node.display = Display::Flex;
    node.left = Val::Px(pos.x + 16.0);
    node.top = Val::Px(pos.y + 16.0);
}
