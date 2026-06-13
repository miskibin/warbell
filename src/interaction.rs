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
/// Range around the next dwelling slot's construction pad (`castle::next_house_site`).
const HOUSE_DIST: f32 = 3.0;
/// Talk-back range: a bit over the villager-chatter trigger (`npc::NEAR_DIST` 7.0) so stepping
/// back half a pace during the jab doesn't lose the prompt.
const TALK_DIST: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum InteractKind {
    Upgrades,
    Shop,
    WarBell,
    Build,
    /// Standing at the next dwelling slot's construction pad inside the walls — E raises the
    /// house right there (houses left the plot Build menu: a building must rise where you
    /// stand, never somewhere else).
    RaiseHouse,
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
            InteractKind::RaiseHouse => "Raise house",
            InteractKind::TalkBack => "Talk back",
        }
    }
}

/// The interactable the hero is currently in range of (nearest wins), or `None`.
#[derive(Resource, Default)]
struct ActiveInteraction {
    kind: Option<InteractKind>,
    /// When the active interaction can't be acted on *right now* (e.g. Raise house with too
    /// little wood/stone), the player-facing reason — `None` means actionable. Drives the
    /// prompt's red "can't afford" state + the shortfall line, so the player sees *why* before
    /// pressing E (not a silent no-op).
    blocked: Option<String>,
}

/// Wood/stone the player is still short of for a House, as a prompt line — `None` if affordable.
fn house_shortfall(bank: &tileworld_core::resource_store::ResourceState) -> Option<String> {
    use tileworld_core::town_store::HOUSE_COST;
    let nw = (HOUSE_COST.wood - bank.wood()).max(0.0).ceil() as i64;
    let ns = (HOUSE_COST.stone - bank.stone()).max(0.0).ceil() as i64;
    match (nw, ns) {
        (0, 0) => None,
        (w, 0) => Some(format!("need {w} more wood")),
        (0, s) => Some(format!("need {s} more stone")),
        (w, s) => Some(format!("need {w} wood + {s} stone")),
    }
}

#[derive(Component)]
struct PromptRoot;
/// The bordered chip inside `PromptRoot` — recoloured red when the active interaction is blocked.
#[derive(Component)]
struct PromptChip;
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
    mut town: ResMut<crate::town::TownRes>,
    mut bank: ResMut<crate::economy::Bank>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
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
    // The next free dwelling slot inside the walls (its pad is visible in the world): E here
    // raises a House on the spot. Anchored to the site so the prompt and the building agree.
    let house_site = crate::castle::next_house_site(town.0.houses);
    if let Some(site) = house_site {
        candidates.push((InteractKind::RaiseHouse, site, HOUSE_DIST, true));
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
    active.kind = best.map(|(k, _)| k);
    // Surface affordability on the prompt itself (red + shortfall) so the player knows *before*
    // pressing E — only Raise house carries a cost gate today.
    active.blocked = match active.kind {
        Some(InteractKind::RaiseHouse) => house_shortfall(&bank.0),
        _ => None,
    };

    if let (true, Some(kind)) = (keys.just_pressed(KeyCode::KeyE), active.kind) {
        match kind {
            InteractKind::Upgrades => next_modal.set(Modal::UpgradeTree),
            InteractKind::Shop => next_modal.set(Modal::Shop),
            InteractKind::WarBell => {
                siege.request_prep_skip();
                cues.write(AudioCue::WarBell);
                feedback.trauma = (feedback.trauma + 0.3).min(1.0);
            }
            InteractKind::Build => next_modal.set(Modal::Build),
            InteractKind::RaiseHouse => {
                // Build right here, right now — and ALWAYS answer the press: a float names
                // either the new beds or exactly what's missing (no silent no-op).
                let site = house_site.unwrap_or(p);
                let y = crate::worldmap::ground_at_world(site.x, site.y).unwrap_or(0.0);
                use tileworld_core::town_store::POP_PER_HOUSE;
                if town.0.build_house(&mut bank.0) {
                    cues.write(AudioCue::UiSelect);
                    floats.0.push(crate::combat_fx::FloatReq {
                        world: Vec3::new(site.x, y + 3.0, site.y),
                        text: format!("\u{1f3e0} House raised \u{2014} beds for {POP_PER_HOUSE} more"),
                        color: Color::srgb(0.55, 1.0, 0.6),
                        scale: 1.25,
                    });
                } else {
                    // Can't afford — name exactly what's short (the prompt already warned in red).
                    let why = house_shortfall(&bank.0).unwrap_or_else(|| "need more resources".into());
                    floats.0.push(crate::combat_fx::FloatReq {
                        world: Vec3::new(site.x, y + 3.0, site.y),
                        text: format!("Can't raise house \u{2014} {why}"),
                        color: Color::srgb(1.0, 0.4, 0.35),
                        scale: 1.1,
                    });
                }
            }
            InteractKind::TalkBack => {
                if let Some(offer) = offered.0.take() {
                    voices.accept_reply(offer, time.elapsed_secs());
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
                    k.spawn(label(&fonts.extrabold, "E", 12.0, rgba(255, 224, 170, 0.92)));
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
    mut label_q: Query<(&mut Text, &mut TextColor), With<PromptLabel>>,
) {
    let playing = modal.map_or(false, |m| *m.get() == Modal::None);
    let kind = if playing { active.kind } else { None };
    if let Ok(mut node) = root_q.single_mut() {
        node.display = if kind.is_some() { Display::Flex } else { Display::None };
    }
    let Some(k) = kind else { return };
    let blocked = active.blocked.as_deref();

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
