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
            .add_systems(Startup, debug_seed)
            // Fresh run wipes bag, buffs and toasts (with the rest of progression).
            .add_systems(OnExit(AppState::StartScreen), reset_inventory)
            .add_systems(OnExit(AppState::GameOver), reset_inventory)
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

fn spawn_inventory_panel(mut commands: Commands, inv: Res<Inventory>, atlas: Res<crate::icons::IconAtlas>) {
    build_inv_panel(&mut commands, &inv.0, &atlas);
}

fn despawn_inventory_panel(mut commands: Commands, q: Query<Entity, With<InvUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// (Re)build the satchel panel from the current bag: an equipped-gear header + one clickable
/// row per occupied slot (name × count + its stat line). Called on open and after every action.
fn build_inv_panel(commands: &mut Commands, bag: &Bag, atlas: &crate::icons::IconAtlas) {
    let equip_line = {
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
        format!("Weapon: {weapon}      Armor: {armor}")
    };

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.82)),
            GlobalZIndex(60),
            InvUi,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("Satchel"),
                TextFont { font_size: 34.0, ..default() },
                TextColor(Color::srgb(0.95, 0.88, 0.6)),
            ));
            root.spawn((
                Text::new(equip_line),
                TextFont { font_size: 18.0, ..default() },
                TextColor(Color::srgb(0.8, 0.9, 1.0)),
            ));
            root.spawn((
                Text::new("I / Esc to close  ·  click an item to use or equip"),
                TextFont { font_size: 14.0, ..default() },
                TextColor(Color::srgba(0.8, 0.8, 0.85, 0.7)),
            ));

            let mut any = false;
            for (i, slot) in bag.bag.iter().enumerate() {
                let Some(id) = slot.item_id.as_deref() else { continue };
                any = true;
                let def = item_def(id);
                let name = def.map(|d| d.name).unwrap_or(id);
                let stat = def.map(|d| d.stat_line()).unwrap_or_default();
                root.spawn((
                    Button,
                    Interaction::default(),
                    Node {
                        width: Val::Px(360.0),
                        padding: UiRect::all(Val::Px(5.0)),
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.18, 0.18, 0.21)),
                    InvSlotButton(i),
                ))
                .with_children(|b| {
                    if let Some(icon) = atlas.get(id) {
                        b.spawn((
                            Node { width: Val::Px(22.0), height: Val::Px(22.0), margin: UiRect::right(Val::Px(8.0)), ..default() },
                            ImageNode::new(icon),
                        ));
                    }
                    b.spawn((
                        Text::new(format!("{name}  x{}", slot.count)),
                        TextFont { font_size: 15.0, ..default() },
                        TextColor(Color::WHITE),
                    ));
                    b.spawn((
                        Text::new(stat),
                        TextFont { font_size: 13.0, ..default() },
                        TextColor(Color::srgb(0.7, 0.78, 0.7)),
                    ));
                });
            }
            if !any {
                root.spawn((
                    Text::new("(empty — go forage, mine and hunt)"),
                    TextFont { font_size: 16.0, ..default() },
                    TextColor(Color::srgba(0.8, 0.8, 0.85, 0.6)),
                ));
            }
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
    atlas: Res<crate::icons::IconAtlas>,
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
        build_inv_panel(&mut commands, &inv.0, &atlas);
    }
}
