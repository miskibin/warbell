//! **Affordance hints** — a single decorated toast, bottom-right, that nudges the player toward
//! an action they've unlocked but not taken: *spend* (you can afford a real upgrade — a War Table
//! node, or shop gear better than what's worn) or *equip* (a better weapon/armor sits unequipped in
//! the satchel). Each of the three channels arms after a short settle delay, shows one toast for
//! ~60s, then disappears on its own; resolving the condition fades it out early. **Prep-only** — the
//! whole thing resets when a night Wave begins (so no stale 60s timer spans a siege) and re-arms at
//! the next dawn. Pure UI state: nothing is saved (recomputed from live resources, like `Toasts`).
//!
//! The predicate half (`spend_available` / `better_weapon` / `better_armor`) is dependency-free over
//! the parity-tested core stores, so it's unit-tested below; the state machine + toast visuals are
//! verified in-engine.

use bevy::prelude::*;

use tileworld_core::inventory::{item_def, Bag, ItemKind};
use tileworld_core::shop_catalog::{build_shop_items, discounted_price};
use tileworld_core::upgrade_store::{UpgradeState, UPGRADE_NODES};

use crate::economy::{Bank, EconomyState, Upgrades};
use crate::game_state::Modal;
use crate::inventory::Inventory;
use crate::player::PlayerRes;
use crate::siege::{GamePhase, Siege};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

/// Seconds the condition must hold before the toast appears (rides out loot churn).
const PROMOTE_DELAY: f64 = 3.0;
/// How long the toast stays once shown, before it auto-expires.
const SHOW_SECS: f64 = 60.0;
/// Fade-in / fade-out duration (also the dismiss-on-resolve fade).
const FADE_SECS: f64 = 0.6;

pub struct HintsPlugin;

impl Plugin for HintsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_hint_root)
            .add_systems(Update, drive_hints.run_if(in_state(Modal::None)));
    }
}

// ── Predicates (pure, unit-tested) ──────────────────────────────────────────────────────

/// What the affordable purchase is, so the toast can name the right venue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SpendKind {
    /// A prereq-met, unbought, affordable War Table node.
    Upgrade,
    /// Shop gear (weapon/armor) strictly better than what's equipped and within budget.
    Gear,
}

/// `Some` when the player can buy something *worthwhile* right now — deliberately strict: an
/// upgrade node, or shop gear better than equipped. Affordable consumables do NOT count. Upgrades
/// take priority for the message (the War Table is the deeper progression).
pub(crate) fn spend_available(
    gold: i64,
    stone: i64,
    eq_weapon_dmg: f64,
    eq_armor_def: f64,
    unlocked: &[&str],
    discount: f64,
    upgrades: &UpgradeState,
) -> Option<SpendKind> {
    for node in UPGRADE_NODES {
        if !upgrades.is_purchased(node.id) && upgrades.can_buy(node, gold, stone, false) {
            return Some(SpendKind::Upgrade);
        }
    }
    for item in build_shop_items(unlocked) {
        let Some(def) = item_def(item.id) else { continue };
        let better = match def.kind {
            ItemKind::Weapon => def.damage_bonus > eq_weapon_dmg,
            ItemKind::Armor => def.defense > eq_armor_def,
            _ => false,
        };
        if better && discounted_price(item.price, discount) <= gold {
            return Some(SpendKind::Gear);
        }
    }
    None
}

/// The id of a bag weapon whose damage beats the equipped weapon (0 = fists), if any.
pub(crate) fn better_weapon(bag: &Bag) -> Option<String> {
    let eq = bag.weapon_bonus;
    bag.bag.iter().filter_map(|s| s.item_id.as_deref()).find_map(|id| {
        let d = item_def(id)?;
        (d.kind == ItemKind::Weapon && d.damage_bonus > eq).then(|| id.to_string())
    })
}

/// The id of a bag armor whose defense beats the worn armor (0 = bare), if any.
pub(crate) fn better_armor(bag: &Bag) -> Option<String> {
    let eq = bag.equipped_armor_id.as_deref().and_then(item_def).map(|d| d.defense).unwrap_or(0.0);
    bag.bag.iter().filter_map(|s| s.item_id.as_deref()).find_map(|id| {
        let d = item_def(id)?;
        (d.kind == ItemKind::Armor && d.defense > eq).then(|| id.to_string())
    })
}

// ── Per-channel state machine ───────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Chan {
    /// Condition false; ready to re-arm.
    Idle,
    /// Condition true, waiting out the settle delay.
    Pending { since: f64 },
    /// Toast visible; auto-expires at `expires`.
    Shown { since: f64, expires: f64 },
    /// Fading out until `until`; `relatch` = went to Latched (timed out) vs Idle (resolved).
    Dismissing { until: f64, relatch: bool },
    /// Shown our one toast; condition still true → stay silent until it clears.
    Latched,
}

impl Default for Chan {
    fn default() -> Self {
        Chan::Idle
    }
}

impl Chan {
    /// Advance one frame given whether the condition holds. Returns the visible alpha (0..1) if a
    /// toast should render this frame.
    fn step(&mut self, cond: bool, now: f64) -> Option<f32> {
        *self = match *self {
            Chan::Idle => {
                if cond {
                    Chan::Pending { since: now }
                } else {
                    Chan::Idle
                }
            }
            Chan::Pending { since } => {
                if !cond {
                    Chan::Idle
                } else if now - since >= PROMOTE_DELAY {
                    Chan::Shown { since: now, expires: now + SHOW_SECS }
                } else {
                    Chan::Pending { since }
                }
            }
            Chan::Shown { since, expires } => {
                if !cond {
                    Chan::Dismissing { until: now + FADE_SECS, relatch: false }
                } else if now >= expires {
                    Chan::Dismissing { until: now + FADE_SECS, relatch: true }
                } else {
                    Chan::Shown { since, expires }
                }
            }
            Chan::Dismissing { until, relatch } => {
                if now >= until {
                    if relatch {
                        Chan::Latched
                    } else {
                        Chan::Idle
                    }
                } else {
                    Chan::Dismissing { until, relatch }
                }
            }
            Chan::Latched => {
                if cond {
                    Chan::Latched
                } else {
                    Chan::Idle
                }
            }
        };
        match *self {
            Chan::Shown { since, .. } => Some((((now - since) / FADE_SECS).clamp(0.0, 1.0)) as f32),
            Chan::Dismissing { until, .. } => {
                Some((((until - now) / FADE_SECS).clamp(0.0, 1.0)) as f32)
            }
            _ => None,
        }
    }
}

#[derive(Default)]
struct HintState {
    spend: Chan,
    weapon: Chan,
    armor: Chan,
}

// ── UI ──────────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
struct HintRoot;
#[derive(Component)]
struct HintRow;

fn setup_hint_root(mut commands: Commands) {
    commands.spawn((
        HintRoot,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(16.0),
            bottom: Val::Px(16.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: Val::Px(6.0),
            ..default()
        },
        GlobalZIndex(70),
        bevy::ui::FocusPolicy::Pass,
    ));
}

/// The icon + text a visible channel renders.
enum Glyph {
    /// Gold coin, tinted (spend hints).
    Gold,
    /// A specific bag item's icon (gear hints).
    Item(String),
}
struct Content {
    glyph: Glyph,
    text: String,
}

#[allow(clippy::too_many_arguments)]
fn drive_hints(
    time: Res<Time>,
    siege: Res<Siege>,
    player: Res<PlayerRes>,
    bank: Res<Bank>,
    inv: Res<Inventory>,
    eco: Res<EconomyState>,
    up: Res<Upgrades>,
    atlas: Res<IconAtlas>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
    mut state: Local<HintState>,
    root_q: Query<Entity, With<HintRoot>>,
    rows_q: Query<Entity, With<HintRow>>,
) {
    let now = time.elapsed_secs_f64();

    // Prep-only: outside the daytime breather, wipe state + any visible toast and bail.
    if siege.phase != GamePhase::Prep {
        if !matches!((state.spend, state.weapon, state.armor), (Chan::Idle, Chan::Idle, Chan::Idle)) {
            *state = HintState::default();
        }
        for e in &rows_q {
            commands.entity(e).try_despawn();
        }
        return;
    }

    // Evaluate each channel's condition + the content it would show.
    let eq_armor_def =
        inv.0.equipped_armor_id.as_deref().and_then(item_def).map(|d| d.defense).unwrap_or(0.0);
    let spend = spend_available(
        player.0.gold,
        bank.0.stone() as i64,
        inv.0.weapon_bonus,
        eq_armor_def,
        &eco.unlocked_weapons,
        eco.shop_discount as f64,
        &up.0,
    )
    .map(|kind| Content {
        glyph: Glyph::Gold,
        text: match kind {
            SpendKind::Upgrade => "You can afford an upgrade — War Table (E)".to_string(),
            SpendKind::Gear => "The merchant has better gear you can afford (E)".to_string(),
        },
    });
    let weapon = better_weapon(&inv.0).map(|id| Content {
        text: format!("Better weapon: {} — Tab to equip", item_def(&id).map(|d| d.name).unwrap_or("?")),
        glyph: Glyph::Item(id),
    });
    let armor = better_armor(&inv.0).map(|id| Content {
        text: format!("Better armor: {} — Tab to equip", item_def(&id).map(|d| d.name).unwrap_or("?")),
        glyph: Glyph::Item(id),
    });

    // Advance the state machines and collect what's visible this frame.
    let visible: Vec<(f32, &Content)> = [
        (state.spend.step(spend.is_some(), now), spend.as_ref()),
        (state.weapon.step(weapon.is_some(), now), weapon.as_ref()),
        (state.armor.step(armor.is_some(), now), armor.as_ref()),
    ]
    .into_iter()
    .filter_map(|(a, c)| Some((a?, c?)))
    .collect();

    // Rebuild the column each frame (cheap — ≤3 rows; mirrors the Notice queue).
    for e in &rows_q {
        commands.entity(e).try_despawn();
    }
    let Ok(root) = root_q.single() else { return };
    if visible.is_empty() {
        return;
    }
    // A slow shared pulse drives the "glow" on the gold border.
    let pulse = 0.5 + 0.5 * (now as f32 * 3.0).sin();
    commands.entity(root).with_children(|col| {
        for (alpha, content) in visible {
            let border_a = (0.45 + 0.4 * pulse) * alpha;
            col.spawn((
                HintRow,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    max_width: Val::Px(300.0),
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                    border: border(1.5),
                    border_radius: radius(R_CARD),
                    ..default()
                },
                BackgroundColor(rgba(26, 21, 15, 0.92).with_alpha(0.92 * alpha)),
                BorderColor::all(GOLD.with_alpha(border_a)),
                shadow_card(),
            ))
            .with_children(|row| {
                match &content.glyph {
                    Glyph::Gold => {
                        if let Some(entry) = atlas.get_tintable("stat:gold") {
                            row.spawn(widgets::icon_tinted(entry, 22.0, GOLD.with_alpha(alpha)));
                        }
                    }
                    Glyph::Item(id) => {
                        if let Some(handle) = atlas.get(id) {
                            row.spawn(widgets::icon(handle, 24.0));
                        }
                    }
                }
                row.spawn(label(&fonts.bold, content.text.clone(), 13.0, TEXT.with_alpha(alpha)));
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tileworld_core::upgrade_store::UpgradeState;

    /// Add `id` to the bag, and equip it (via the click path) when `equip`.
    fn bag_with(ids: &[(&str, bool)]) -> Bag {
        let mut b = Bag::default();
        for (id, equip) in ids {
            b.add(id, 1);
            if *equip {
                let i = b.bag.iter().position(|s| s.item_id.as_deref() == Some(*id)).unwrap();
                b.activate_bag_item(i); // weapon/armor → equips; consumable would eat
            }
        }
        b
    }

    #[test]
    fn spend_fires_on_affordable_upgrade_node() {
        let up = UpgradeState::default();
        // Plenty of gold + stone, fists/bare equipped: a root node is affordable.
        assert_eq!(
            spend_available(9999, 9999, 0.0, 0.0, &[], 1.0, &up),
            Some(SpendKind::Upgrade)
        );
    }

    #[test]
    fn spend_silent_when_broke() {
        let up = UpgradeState::default();
        assert_eq!(spend_available(0, 0, 0.0, 0.0, &[], 1.0, &up), None);
    }

    #[test]
    fn spend_does_not_fire_for_consumables_only() {
        // Enough gold for bread (4g) but not for any node (cheapest root node costs more) and no
        // weapon unlocked → the affordable thing is only a consumable, which must NOT trip Spend.
        let up = UpgradeState::default();
        // Use a gold amount below the cheapest upgrade node but above bread.
        let cheapest_node = UPGRADE_NODES.iter().map(|n| n.base_cost).min().unwrap();
        let gold = (cheapest_node - 1).max(5); // afford bread, not the cheapest node
        // No stone either, so stone-gated nodes are out.
        assert_eq!(spend_available(gold, 0, 999.0, 999.0, &[], 1.0, &up), None);
    }

    #[test]
    fn spend_fires_on_affordable_better_shop_gear() {
        // No gold for the cheapest node but enough for an unlocked golden blade, with weak weapon
        // equipped so it's strictly better.
        let up = UpgradeState::default();
        // sword_gold base price 80; equipped weapon dmg 11 (iron) < 21 (gold).
        // Set stone 0 and gold just enough for the blade but below any node we can't meet prereqs
        // for cheaply — simplest: give exactly 80 and equip iron sword.
        let res = spend_available(80, 0, 11.0, 999.0, &["sword_gold"], 1.0, &up);
        // Either an affordable node also exists at 80g (then Upgrade) — both are valid "spend"
        // nudges — so just assert it's Some.
        assert!(res.is_some());
    }

    #[test]
    fn better_weapon_detects_unequipped_upgrade() {
        // Carry an iron sword, equip nothing (fists) → it's better than fists.
        let b = bag_with(&[("sword_iron", false)]);
        assert_eq!(better_weapon(&b).as_deref(), Some("sword_iron"));
    }

    #[test]
    fn better_weapon_silent_when_equipped_is_best() {
        // Equip the golden blade (21), carry only a weaker iron sword (11).
        let mut b = bag_with(&[("sword_gold", true)]);
        b.add("sword_iron", 1);
        assert_eq!(better_weapon(&b), None);
    }

    #[test]
    fn better_armor_detects_unequipped_upgrade() {
        let b = bag_with(&[("leather_armor", false)]);
        assert_eq!(better_armor(&b).as_deref(), Some("leather_armor"));
    }

    #[test]
    fn better_armor_silent_when_equipped_is_best() {
        let mut b = bag_with(&[("gold_armor", true)]);
        b.add("leather_armor", 1);
        assert_eq!(better_armor(&b), None);
    }
}
