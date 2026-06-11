//! **Contextual interaction** — ports the 3js game's single-key `E` interact. Instead of global
//! hotkeys, the player walks up to a thing and presses **E**: near the **keep** → upgrades, near the
//! **merchant stall** → shop, near the **war bell** (prep only) → ring in the night, right after a
//! villager's jab → **talk back** (fires the offered comeback chain). The nearest
//! in-range interactable wins (the keep and bell zones overlap), and a screen-space "E" prompt names
//! it. Proximity only — no facing check, matching the original.
//!
//! Other verbs keep their dedicated keys: **F** chest/forage (`verbs`), **R** recruit (`villagers`),
//! **I** satchel (`inventory`), **Q/Z/X/C** quick-use (`inventory`).

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
const BUILD_DIST: f32 = 3.0;
/// Talk-back range: a bit over the villager-chatter trigger (`npc::NEAR_DIST` 7.0) so stepping
/// back half a pace during the jab doesn't lose the prompt.
const TALK_DIST: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum InteractKind {
    Upgrades,
    Shop,
    WarBell,
    Build,
    /// A villager jabbed at the hero and a comeback is on offer (`director::OfferedReply`).
    TalkBack,
}
impl InteractKind {
    fn prompt(self) -> &'static str {
        match self {
            InteractKind::Upgrades => "Upgrades",
            InteractKind::Shop => "Shop",
            InteractKind::WarBell => "Ring the bell",
            InteractKind::Build => "Build",
            InteractKind::TalkBack => "Talk back",
        }
    }
}

/// The interactable the hero is currently in range of (nearest wins), or `None`.
#[derive(Resource, Default)]
struct ActiveInteraction(Option<InteractKind>);

#[derive(Component)]
struct PromptRoot;
#[derive(Component)]
struct PromptLabel;

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
    mut siege: ResMut<Siege>,
    mut active: ResMut<ActiveInteraction>,
    mut next_modal: ResMut<NextState<Modal>>,
    mut cues: MessageWriter<AudioCue>,
    mut feedback: ResMut<HitFeedback>,
    plot_spots: Res<crate::town::PlotSpots>,
    town: Res<crate::town::TownRes>,
    mut build_target: ResMut<crate::town::BuildTarget>,
    time: Res<Time>,
    mut offered: ResMut<crate::audio::director::OfferedReply>,
    mut voices: ResMut<crate::audio::director::VoiceManager>,
) {
    let p = hero.pos;

    // Nearest buildable (Empty/Rubble) plot the hero is standing on.
    let mut nearest_plot: Option<(usize, f32)> = None;
    for (idx, spot) in plot_spots.0.iter().enumerate() {
        if town.0.plots.get(idx).map_or(false, |pl| pl.is_buildable()) {
            let d = p.distance(*spot);
            if d < BUILD_DIST && nearest_plot.map_or(true, |(_, bd)| d < bd) {
                nearest_plot = Some((idx, d));
            }
        }
    }
    build_target.0 = nearest_plot.map(|(i, _)| i);

    // (kind, position, radius, available?)
    let mut candidates: Vec<(InteractKind, Vec2, f32, bool)> = vec![
        (InteractKind::Upgrades, Vec2::ZERO, KEEP_DIST, true),
        (InteractKind::Shop, shop_anchor(), SHOP_DIST, true),
        (InteractKind::WarBell, Vec2::new(0.0, 6.0), BELL_DIST, siege.phase == GamePhase::Prep),
    ];
    if let Some((idx, _)) = nearest_plot {
        candidates.push((InteractKind::Build, plot_spots.0[idx], BUILD_DIST, true));
    }
    // A villager jab on offer: the prompt anchors where the speaker stood (expiry is handled by
    // `tick_chains`; here we only range-gate it).
    if let Some(offer) = offered.0 {
        let at = offer.pos.map_or(p, |v| Vec2::new(v.x, v.z));
        candidates.push((InteractKind::TalkBack, at, TALK_DIST, true));
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
    active.0 = best.map(|(k, _)| k);

    if let (true, Some(kind)) = (keys.just_pressed(KeyCode::KeyE), active.0) {
        match kind {
            InteractKind::Upgrades => next_modal.set(Modal::UpgradeTree),
            InteractKind::Shop => next_modal.set(Modal::Shop),
            InteractKind::WarBell => {
                siege.request_prep_skip();
                cues.write(AudioCue::WarBell);
                feedback.trauma = (feedback.trauma + 0.3).min(1.0);
            }
            InteractKind::Build => next_modal.set(Modal::Build),
            InteractKind::TalkBack => {
                if let Some(offer) = offered.0.take() {
                    voices.accept_reply(offer, time.elapsed_secs());
                }
            }
        }
    }
}

fn setup_prompt(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            PromptRoot,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(110.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-90.0)),
                width: Val::Px(180.0),
                display: Display::None,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(8.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                border: border(1.0),
                border_radius: radius(R_CARD),
                ..default()
            },
            BackgroundColor(PANEL_HUD),
            BorderColor::all(rgba(255, 213, 140, 0.5)),
            shadow_hud(),
            bevy::ui::FocusPolicy::Pass,
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
                k.spawn(label(&fonts.extrabold, "E", 12.0, rgba(255, 224, 170, 0.92)));
            });
            p.spawn((label(&fonts.bold, "Upgrades", 14.0, GOLD), PromptLabel));
        });
}

/// Show the prompt for the active interactable, but only while actually playing with no panel open.
fn update_prompt(
    active: Res<ActiveInteraction>,
    modal: Option<Res<State<Modal>>>,
    mut root_q: Query<&mut Node, With<PromptRoot>>,
    mut label_q: Query<&mut Text, With<PromptLabel>>,
) {
    let playing = modal.map_or(false, |m| *m.get() == Modal::None);
    let kind = if playing { active.0 } else { None };
    if let Ok(mut node) = root_q.single_mut() {
        node.display = if kind.is_some() { Display::Flex } else { Display::None };
    }
    if let (Some(k), Ok(mut t)) = (kind, label_q.single_mut()) {
        if t.as_str() != k.prompt() {
            **t = k.prompt().to_string();
        }
    }
}
