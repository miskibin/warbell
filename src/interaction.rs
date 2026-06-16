//! **Contextual interaction** — ports the 3js game's single-key `E` interact. Instead of global
//! hotkeys, the player walks up to a thing and presses **E**: near the **keep** → upgrades, near the
//! **merchant stall** → shop, near the **war bell** (prep only) → ring in the night, right after a
//! villager's jab → **talk back** (fires the offered comeback chain). The nearest
//! in-range interactable wins (the keep and bell zones overlap), and a screen-space "E" prompt names
//! it. Proximity only — no facing check, matching the original.
//!
//! Treasure **chests** join this E resolver too: near an unopened chest the prompt shows
//! **E "Open chest"** and a press emits `chest::OpenChest` (the actual loot + lid swing stays in
//! `chest.rs`). Other verbs keep their own keys: **F** forage/rescue (`verbs`/`villagers`),
//! **R** recruit (`villagers`), **I** satchel, **Q/Y/T** quick-use (`inventory`).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::audio::AudioCue;
use crate::combat_fx::HitFeedback;
use crate::game_state::Modal;
use crate::player::HeroState;
use crate::siege::{GamePhase, Siege};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};

/// Interaction ranges (world units), from the 3js `cityPlan`/`Shop` constants.
const KEEP_DIST: f32 = 4.2;
const BELL_DIST: f32 = 4.2;
const SHOP_DIST: f32 = 3.5;
/// Talk-back range: a bit over the villager-chatter trigger (`npc::NEAR_DIST` 7.0) so stepping
/// back half a pace during the jab doesn't lose the prompt.
const TALK_DIST: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum InteractKind {
    Upgrades,
    Shop,
    WarBell,
    /// A villager jabbed at the hero and a comeback is on offer (`director::OfferedReply`).
    TalkBack,
    /// Standing next to an unopened treasure chest — **E** opens it (the resolver emits
    /// `chest::OpenChest`; `chest.rs` does the actual loot + lid swing).
    Chest,
}
impl InteractKind {
    fn prompt(self) -> &'static str {
        match self {
            InteractKind::Upgrades => "Upgrades",
            InteractKind::Shop => "Shop",
            InteractKind::WarBell => "Ring the bell",
            InteractKind::TalkBack => "Talk back",
            InteractKind::Chest => "Open chest",
        }
    }
    /// The keycap shown on the prompt chip. Every contextual action — chests included — is on **E**.
    fn key_label(self) -> &'static str {
        "E"
    }
}

/// The interactable the hero is currently in range of (nearest wins), or `None`.
#[derive(Resource, Default)]
struct ActiveInteraction {
    kind: Option<InteractKind>,
    /// A player-facing "can't act yet" reason for the active interaction (red prompt + detail),
    /// or `None` when actionable. No current interaction gates on cost — building's afford check
    /// moved to the HUD-button build mode (`town::build_place`) — so this stays `None` today, but
    /// the prompt machinery is kept for the next gated interaction.
    blocked: Option<String>,
}

/// The chest read + open-request, bundled so `drive_interaction` stays under Bevy's 16-param cap.
#[derive(SystemParam)]
struct ChestIo<'w, 's> {
    chests: Query<'w, 's, (Entity, &'static crate::chest::Chest, &'static Transform)>,
    open: MessageWriter<'w, crate::chest::OpenChest>,
}

#[derive(Component)]
struct PromptRoot;
/// The bordered chip inside `PromptRoot` — recoloured red when the active interaction is blocked.
#[derive(Component)]
struct PromptChip;
#[derive(Component)]
struct PromptLabel;
/// The keycap glyph inside the chip — "E" for contextual actions, "F" for chest/world verbs.
#[derive(Component)]
struct PromptKey;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveInteraction>()
            .add_systems(Startup, setup_prompt)
            .add_systems(Update, drive_interaction.run_if(in_state(Modal::None)))
            .add_systems(Update, update_prompt);
    }
}

/// The merchant stall's world XZ — `castle::gate_centers()[0] + (2.5, -5.0)`, matching where
/// `villagers` builds the market.
fn shop_anchor() -> Vec2 {
    crate::castle::gate_centers()[0] + Vec2::new(2.5, -5.0)
}

#[allow(clippy::too_many_arguments)]
fn drive_interaction(
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<HeroState>,
    build_mode: Res<crate::town::BuildMode>,
    mut siege: ResMut<Siege>,
    mut active: ResMut<ActiveInteraction>,
    mut next_modal: ResMut<NextState<Modal>>,
    mut cues: MessageWriter<AudioCue>,
    mut feedback: ResMut<HitFeedback>,
    time: Res<Time>,
    mut offered: ResMut<crate::audio::director::OfferedReply>,
    mut voices: ResMut<crate::audio::director::VoiceManager>,
    mut chest_io: ChestIo,
) {
    let p = hero.pos;

    // While placing buildings, the build palette owns E + its own on-spot prompt — don't also
    // surface keep/shop/bell/chest prompts (they'd fight `town::build_place` for the key).
    if build_mode.active {
        active.kind = None;
        active.blocked = None;
        return;
    }

    // (kind, position, radius, available?)
    let mut candidates: Vec<(InteractKind, Vec2, f32, bool)> = vec![
        (InteractKind::Upgrades, Vec2::ZERO, KEEP_DIST, true),
        (InteractKind::Shop, shop_anchor(), SHOP_DIST, true),
        (InteractKind::WarBell, Vec2::new(0.0, 6.0), BELL_DIST, siege.phase == GamePhase::Prep),
    ];
    // A villager jab on offer: the prompt anchors where the speaker stood (expiry is handled by
    // `tick_chains`; here we only range-gate it).
    if let Some(offer) = offered.0 {
        let at = offer.pos.map_or(p, |v| Vec2::new(v.x, v.z));
        candidates.push((InteractKind::TalkBack, at, TALK_DIST, true));
    }
    // Nearest unopened treasure chest in reach — joins the E nearest-wins arbitration. We remember
    // the entity so an E press opens exactly the one the prompt named (chest.rs does the open).
    let mut nearest_chest: Option<(Entity, Vec2, f32)> = None;
    for (e, chest, tf) in &chest_io.chests {
        if chest.opened {
            continue;
        }
        let at = Vec2::new(tf.translation.x, tf.translation.z);
        let d = p.distance(at);
        if d < crate::chest::CHEST_INTERACT_DIST && nearest_chest.map_or(true, |(_, _, bd)| d < bd) {
            nearest_chest = Some((e, at, d));
        }
    }
    if let Some((_, at, _)) = nearest_chest {
        candidates.push((InteractKind::Chest, at, crate::chest::CHEST_INTERACT_DIST, true));
    }

    // Pick the nearest in-range, available interactable.
    let mut best: Option<(InteractKind, f32)> = None;
    for (kind, pos, radius, ok) in candidates {
        if !ok {
            continue;
        }
        let d = p.distance(pos);
        if d < radius && best.map_or(true, |(_, bd)| d < bd) {
            best = Some((kind, d));
        }
    }
    active.kind = best.map(|(k, _)| k);
    active.blocked = None; // no interaction carries a cost gate now (building moved to build mode)

    if let (true, Some(kind)) = (keys.just_pressed(KeyCode::KeyE), active.kind) {
        match kind {
            InteractKind::Upgrades => next_modal.set(Modal::UpgradeTree),
            InteractKind::Shop => next_modal.set(Modal::Shop),
            InteractKind::WarBell => {
                siege.request_prep_skip();
                cues.write(AudioCue::WarBell);
                feedback.trauma = (feedback.trauma + 0.3).min(1.0);
            }
            InteractKind::TalkBack => {
                if let Some(offer) = offered.0.take() {
                    voices.accept_reply(offer, time.elapsed_secs());
                }
            }
            // Ask chest.rs to open exactly the chest the resolver picked as nearest-in-range.
            InteractKind::Chest => {
                if let Some((e, _, _)) = nearest_chest {
                    chest_io.open.write(crate::chest::OpenChest(e));
                }
            }
        }
    }
}

/// Default chip border (gold hairline) — `update_prompt` flips to `RED_BORDER` when blocked.
const PROMPT_BORDER: Color = rgba(255, 213, 140, 0.5);

fn setup_prompt(mut commands: Commands, fonts: Res<UiFonts>) {
    // A full-width centring band so the chip can grow with its text (the shortfall line can be
    // long) and stay centred — the chip used to be fixed-width and centred by a margin hack.
    commands
        .spawn((
            PromptRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(110.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                display: Display::None,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                ..default()
            },
            bevy::ui::FocusPolicy::Pass,
        ))
        .with_children(|root| {
            root.spawn((
                PromptChip,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                    border: border(1.0),
                    border_radius: radius(R_CARD),
                    ..default()
                },
                BackgroundColor(PANEL_HUD),
                BorderColor::all(PROMPT_BORDER),
                shadow_hud(),
            ))
            .with_children(|p| {
                // Keycap "E".
                p.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                        border: border(1.0),
                        border_radius: radius(5.0),
                        ..default()
                    },
                    widgets::keycap_paint(),
                ))
                .with_children(|k| {
                    k.spawn((label(&fonts.extrabold, "E", 12.0, rgba(255, 224, 170, 0.92)), PromptKey));
                });
                p.spawn((label(&fonts.bold, "Upgrades", 14.0, GOLD), PromptLabel));
            });
        });
}

/// Show the prompt for the active interactable, but only while actually playing with no panel
/// open. A *blocked* interaction (can't afford it) recolours the whole chip red and appends the
/// shortfall ("Raise house — need 4 wood + 2 stone") so the press is never a silent no-op.
fn update_prompt(
    active: Res<ActiveInteraction>,
    modal: Option<Res<State<Modal>>>,
    mut root_q: Query<&mut Node, With<PromptRoot>>,
    mut chip_q: Query<(&mut BorderColor, &mut BackgroundColor), With<PromptChip>>,
    mut label_q: Query<(&mut Text, &mut TextColor), (With<PromptLabel>, Without<PromptKey>)>,
    mut key_q: Query<&mut Text, (With<PromptKey>, Without<PromptLabel>)>,
) {
    let playing = modal.map_or(false, |m| *m.get() == Modal::None);
    let kind = if playing { active.kind } else { None };
    if let Ok(mut node) = root_q.single_mut() {
        node.display = if kind.is_some() { Display::Flex } else { Display::None };
    }
    let Some(k) = kind else { return };
    let blocked = active.blocked.as_deref();

    // Name the key on the chip (E for contextual, F for chest) — only rewrite on a change.
    if let Ok(mut kt) = key_q.single_mut() {
        if kt.as_str() != k.key_label() {
            **kt = k.key_label().to_string();
        }
    }

    if let Ok((mut t, mut col)) = label_q.single_mut() {
        let want = match blocked {
            Some(detail) => format!("{}  \u{2014}  {detail}", k.prompt()),
            None => k.prompt().to_string(),
        };
        if t.as_str() != want {
            **t = want;
        }
        col.0 = if blocked.is_some() { RED_HI } else { GOLD };
    }
    if let Ok((mut bc, mut bg)) = chip_q.single_mut() {
        if blocked.is_some() {
            *bc = BorderColor::all(RED_BORDER);
            bg.0 = rgba(64, 18, 18, 0.88); // red-tinted HUD chrome — reads "can't"
        } else {
            *bc = BorderColor::all(PROMPT_BORDER);
            bg.0 = PANEL_HUD;
        }
    }
}
