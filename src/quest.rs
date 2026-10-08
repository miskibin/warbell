//! Contextual campaign guidance: one need at a time, actual outcomes, and optional help.
//! The first three days are untimed; the bell always lets the player choose their own approach.
//! Legacy QuestLog stays in saves for compatibility; new runs use the additive Campaign snapshot.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use tileworld_core::campaign::{Campaign, Objective};
use tileworld_core::quest::QuestLog;
use tileworld_core::town_store::BuildKind;
use crate::audio::AudioCue;
use crate::game_state::{AppState, Modal, SimAppExt};
use crate::savegame::GameLoaded;
use crate::siege::{GamePhase, Siege};
use crate::town::TownRes;
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::texture::UiTextures;
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

#[derive(Resource, Default)]
pub struct QuestLogRes(pub QuestLog);
#[derive(Resource, Default)]
pub struct CampaignRes(pub Campaign);

pub struct QuestPlugin;
impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestLogRes>()
            .init_resource::<CampaignRes>()
            .add_systems(Startup, setup_quest_root)
            .add_systems(OnExit(AppState::StartScreen), reset_quests.run_if(crate::rts::in_campaign).run_if(crate::game_state::fresh_run_reset))
            .add_systems(OnExit(AppState::GameOver), reset_quests.run_if(crate::rts::in_campaign))
            .add_systems(OnEnter(Modal::Quest), spawn_quest_panel.run_if(crate::rts::in_campaign))
            .add_systems(OnExit(Modal::Quest), despawn_quest_panel.run_if(crate::rts::in_campaign))
            .add_systems(Update, quest_panel_input.run_if(in_state(Modal::Quest)).run_if(crate::rts::in_campaign))
            .add_sim_systems((observe_campaign.run_if(|ready: Res<crate::biome::WorldReady>| ready.0), quest_open_input).chain().after(crate::camps::respawn_warbands).after(crate::siege::run_director).run_if(crate::rts::in_campaign))
            .add_systems(Update, restore_quest_log.after(crate::savegame::RestoreRunSet).before(crate::game_state::SimulationSet).run_if(crate::rts::in_campaign))
            .add_systems(Update, drive_tracker.run_if(crate::rts::in_campaign));
    }
}

fn reset_quests(mut legacy: ResMut<QuestLogRes>, mut campaign: ResMut<CampaignRes>) {
    legacy.0 = QuestLog::default();
    campaign.0 = Campaign::default();
}

fn restore_quest_log(
    mut ev: MessageReader<GameLoaded>,
    mut legacy: ResMut<QuestLogRes>,
) {
    let Some(GameLoaded(data)) = ev.read().last() else { return };
    legacy.0 = data.quest.clone().unwrap_or_default();
    // Positional legacy quests never map onto new lessons. Existing runs retain their freedom.
    // Campaign is restored synchronously in apply_pending_load, before any camp/director step.
}

pub fn lesson_night(siege: &Siege) -> usize {
    if siege.phase == GamePhase::Wave { siege.wave_index.max(0) as usize }
    else { (siege.wave_index + 1).max(0) as usize }
}

/// Facts are state-based, so an early farm, early rescue or early purchase counts later too.
/// A built farm alone doesn't teach the work loop: a person must actually produce food there.
#[allow(clippy::too_many_arguments)]
pub(crate) fn observe_campaign(
    time: Res<Time>,
    siege: Res<Siege>,
    hero: Res<crate::player::HeroState>,
    town: Res<TownRes>,
    rescued: Res<crate::villagers::RescuedCamps>,
    up: Res<crate::economy::Upgrades>,
    def: Res<crate::economy::Defenses>,
    rallied: Query<(), (With<crate::villagers::Rallied>, Without<crate::dying::Dying>)>,
    mut campaign: ResMut<CampaignRes>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    mut previous: Local<Option<(GamePhase, i32)>>,
) {
    let last = previous.replace((siege.phase, siege.wave_index));
    let c = &mut campaign.0;
    let was = (c.rescued, c.returned, c.farm_worked, c.defense_bought);
    c.rallied |= !rallied.is_empty();
    c.rescued |= rescued.done.iter().any(|done| *done);
    c.returned |= c.rescued && crate::castle::in_footprint(hero.pos.x, hero.pos.y) && rallied.is_empty();
    c.farm_worked |= town.0.food_rate() > 0.0;
    c.defense_bought |= !up.0.purchased().is_empty();
    c.ranged_prepared = def.keep_archers || def.towers || def.ballista;
    let night = lesson_night(&siege);
    c.shaman_cleared = c.raid_day == night as i32
        && crate::camps::raid_target(night as i32).is_some_and(|i| c.cleared_camps.get(i).copied().unwrap_or(false));
    let now = time.elapsed_secs_f64();
    if siege.phase == GamePhase::Prep && last.is_some_and(|(phase, _)| phase == GamePhase::Wave) {
        let farms = town.0.plots.iter().filter(|p| p.kind == Some(BuildKind::Farm) && p.is_built()).count();
        if siege.wave_index == 1 {
            notice.push(if farms > 0 { "Dawn: the fields still stand. Stand down with K and your people return to work." }
                else { "Dawn: the fields were lost. Walk to the ruins and press E to rebuild, or B to raise a farm." }, now);
        }
    }
    if c.guided && siege.phase == GamePhase::Prep {
        if c.briefed_day != night as i32 {
            c.briefed_day = night as i32;
            match night {
                0 => notice.push("Captives need help. Bring your militia — K. Follow the gold marker on the compass.", now),
                1 => notice.push("Torch raiders are coming for the farms. Buy an upgrade at the keep and choose where to defend.", now),
                2 => notice.push("Shamans join tonight's attack. Break their marked camp, or prepare ranged defenses.", now),
                _ => {},
            }
        }
        if !was.0 && c.rescued { notice.push("They're free. Return to the keep and stand down with K so your people can work.", now); }
        if !was.1 && c.returned { notice.push("You're back at the keep. Build a farm and let a villager start working — B.", now); }
        if !was.2 && c.farm_worked { notice.push("Your people are working the farm. Ring the war bell when you're ready for night.", now); }
        if !was.3 && c.defense_bought && night == 1 { notice.push("Upgrade bought. Choose where to meet tonight's torch raid.", now); }
    }
}

/// Exactly one navigational goal, shared by tracker and compass; no hunt for an unmarked camp.
/// Later days retain the raid opportunity, without step-by-step instruction.
pub fn guidance_target(c: &Campaign, siege: &Siege, town: &TownRes, spots: &crate::town::PlotSpots) -> Option<(Vec2, &'static str)> {
    let night = lesson_night(siege);
    let wave = siege.phase == GamePhase::Wave;
    let objective = c.objective(night, wave);
    match objective {
        Objective::Rally => crate::camps::intro_target().and_then(crate::camps::camp_centre).map(|p| (p, "Captives")),
        Objective::Rescue => crate::camps::intro_target().and_then(crate::camps::camp_centre).map(|p| (p, "Captives")),
        Objective::ReturnHome | Objective::PrepareDefense => Some((Vec2::ZERO, "Keep")),
        Objective::CallNight | Objective::SurviveRitual => Some((crate::castle::BELL_POS, if wave { "Keep" } else { "War bell" })),
        Objective::FeedPeople | Objective::DefendFarm => town.0.plots.iter().enumerate()
            .find(|(_, p)| p.kind == Some(BuildKind::Farm))
            .and_then(|(i, _)| spots.0.get(i)).copied()
            .map(|p| (p, "Farm")).or(Some((Vec2::ZERO, "Build a farm"))),
        Objective::HoldKeep => Some((Vec2::ZERO, "Keep")),
        Objective::DisruptRitual | Objective::FreePlay if !wave => {
            let target = crate::camps::raid_target(night as i32)?;
            if c.raid_day == night as i32 && c.cleared_camps.get(target).copied().unwrap_or(false) { return None; }
            crate::camps::camp_centre(target).map(|p| (p, "Raid camp"))
        }
        _ => None,
    }
}

// ── Tracker (right-center pill) ───────────────────────────────────────────────────────────

#[derive(Component)]
struct QuestRoot;
/// The persistent, clickable tracker pill. Spawned once; its children (icon/text/bar) are rebuilt
/// when the situation changes, while the button entity lives so clicks register across frames.
#[derive(Component)]
struct QuestCard;

fn setup_quest_root(mut commands: Commands) {
    commands
        .spawn((
            QuestRoot,
            crate::game_state::CampaignOnly,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                // Full-height column pinned to the right edge → the card sits vertically centred.
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            GlobalZIndex(68),
            FocusPolicy::Pass,
        ))
        .with_children(|root| {
            root.spawn((
                QuestCard,
                Button,
                Interaction::default(),
                Node {
                    display: Display::None, // shown by drive_tracker once a quest is active
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(11.0),
                    max_width: Val::Px(264.0),
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(11.0)),
                    border: border(1.5),
                    border_radius: radius(R_CARD),
                    ..default()
                },
                BackgroundColor(rgba(26, 21, 15, 0.92)),
                BorderColor::all(GOLD.with_alpha(0.5)),
                shadow_card(),
            ));
        });
}

/// Update the tracker for the current situation. Hidden behind panels and after the first cycles.
fn drive_tracker(
    time: Res<Time>,
    campaign: Res<CampaignRes>,
    siege: Res<Siege>,
    modal: Option<Res<State<Modal>>>,
    atlas: Res<IconAtlas>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
    mut shown: Local<String>,
    mut card_q: Query<
        (Entity, &mut Node, &mut BorderColor, Option<&Children>),
        With<QuestCard>,
    >,
) {
    let Ok((card, mut node, mut bcol, children)) = card_q.single_mut() else { return };

    // Visible only while actually playing with no panel up and a quest still active.
    let playing = modal.as_ref().map_or(false, |m| *m.get() == Modal::None);
    let night = lesson_night(&siege);
    let wave = siege.phase == GamePhase::Wave;
    let q = campaign.0.lesson(night, wave);
    if !playing || campaign.0.objective(night, wave) == Objective::FreePlay {
        if node.display != Display::None {
            node.display = Display::None;
            if let Some(children) = children {
                for &c in children {
                    commands.entity(c).try_despawn();
                }
            }
        }
        return;
    }

    let became_visible = node.display == Display::None;
    node.display = Display::Flex;
    // Slow gold pulse on the border (matches the hints toast).
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 3.0).sin();
    *bcol = BorderColor::all(GOLD.with_alpha(0.45 + 0.4 * pulse));

    if !became_visible && *shown == q.id { return; }
    *shown = q.id.to_string();

    // Rebuild children (cheap — a few nodes; the button shell persists for click detection).
    if let Some(children) = children {
        for &c in children {
            commands.entity(c).try_despawn();
        }
    }

    commands.entity(card).with_children(|row| {
        if let Some(entry) = atlas.get_tintable(q.icon) {
            row.spawn(widgets::icon_tinted(entry, 26.0, GOLD));
        }
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|col| {
            col.spawn(label(&fonts.bold, q.title, 14.0, GOLD));
            col.spawn(label(&fonts.regular, q.why, 12.0, TEXT));
            col.spawn(label(&fonts.semibold, q.action, 12.0, TEXT_DIM));
            col.spawn(label(&fonts.regular, "J — details · H — controls", 10.0, GREY));
        });
    });
}

// ── Explainer card (Modal::Quest) ─────────────────────────────────────────────────────────

#[derive(Component)]
struct QuestPanelUi;
#[derive(Component)]
struct QuestCloseBtn;

/// **J** or a click on the tracker opens the explainer (no-op once the chain is complete).
fn quest_open_input(
    keys: Res<ButtonInput<KeyCode>>,
    campaign: Res<CampaignRes>,
    siege: Res<Siege>,
    card: Query<&Interaction, (With<QuestCard>, Changed<Interaction>)>,
    mut next: ResMut<NextState<Modal>>,
    mut cues: MessageWriter<AudioCue>,
    mut auto_done: Local<bool>,
) {
    if campaign.0.objective(lesson_night(&siege), siege.phase == GamePhase::Wave) == Objective::FreePlay {
        return;
    }
    // Screenshot hook: `FOREST_PANEL=quest` opens the explainer once under the capture harness.
    if !*auto_done && std::env::var("FOREST_PANEL").ok().as_deref() == Some("quest") {
        *auto_done = true;
        next.set(Modal::Quest);
        return;
    }
    let clicked = card.iter().any(|i| *i == Interaction::Pressed);
    if clicked || keys.just_pressed(KeyCode::KeyJ) {
        cues.write(AudioCue::UiSelect);
        next.set(Modal::Quest);
    }
}

/// **J** / **Esc** / ✕ close the explainer (Esc also goes through `game_state::pause_toggle`).
fn quest_panel_input(
    keys: Res<ButtonInput<KeyCode>>,
    close: Query<&Interaction, (With<QuestCloseBtn>, Changed<Interaction>)>,
    mut next: ResMut<NextState<Modal>>,
) {
    let x = close.iter().any(|i| *i == Interaction::Pressed);
    if x || keys.just_pressed(KeyCode::KeyJ) || keys.just_pressed(KeyCode::Escape) {
        next.set(Modal::None);
    }
}

fn despawn_quest_panel(mut commands: Commands, q: Query<Entity, With<QuestPanelUi>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

fn spawn_quest_panel(
    mut commands: Commands,
    campaign: Res<CampaignRes>,
    siege: Res<Siege>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<UiTextures>,
    assets: Res<AssetServer>,
) {
    // Guard: the panel only opens with an active quest, but bail cleanly if the chain finished
    // between the keypress and this OnEnter.
    let q = campaign.0.lesson(lesson_night(&siege), siege.phase == GamePhase::Wave);

    commands.spawn((widgets::scrim(60), FocusPolicy::Block, QuestPanelUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(420.0),
                row_gap: Val::Px(14.0),
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

            // Header: kicker + title + close.
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::bottom(Val::Px(9.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            })
            .insert(BorderColor::all(BORDER_SOFT))
            .with_children(|h| {
                h.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() })
                    .with_children(|t| {
                        t.spawn(label(&fonts.display, "YOUR NEXT MOVE", 12.0, KICKER));
                        t.spawn(label(&fonts.display, q.title, 22.0, GOLD));
                    });
                h.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|right| {
                    right.spawn(label(&fonts.semibold, "J / Esc", 11.0, GREY));
                    widgets::close_button(right, &fonts.bold, QuestCloseBtn, false);
                });
            });

            // Why (the motivational body) — big icon beside it.
            card.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(14.0),
                ..default()
            })
            .with_children(|row| {
                if let Some(entry) = atlas.get_tintable(q.icon) {
                    row.spawn(widgets::icon_tinted(entry, 44.0, GOLD));
                }
                row.spawn((
                    Node { flex_grow: 1.0, ..default() },
                    children![label(&fonts.semibold, q.why, 14.5, TEXT)],
                ));
            });

            // A real in-game screenshot of the action, when one's been captured — shows the player
            // exactly what to look for (the build palette + a glowing plot), not just words.
            if let Some(shot) = q.shot {
                card.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(207.0), // 16:9 against the ~368px inner card width
                        border: border(1.0),
                        border_radius: radius(R_CARD),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BorderColor::all(BORDER_SOFT),
                    ImageNode::new(assets.load(shot)),
                ));
            }

            // How (the explanation + the action keycap line).
            panel_section(card, &fonts, "HOW", |c| {
                c.spawn(label(&fonts.regular, q.explain, 13.0, TEXT_DIM));
                c.spawn((
                    Node { margin: UiRect::top(Val::Px(2.0)), ..default() },
                    children![label(&fonts.bold, q.action, 13.5, GOLD)],
                ));
            });

            card.spawn(label(&fonts.regular, "Other approaches work too. Ring the bell whenever you are ready.", 11.0, GREY));
        });
    });
}

/// A small framed sub-card with a gold small-caps title; `f` fills the body.
fn panel_section(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    title: &str,
    f: impl FnOnce(&mut RelatedSpawnerCommands<ChildOf>),
) {
    p.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(14.0)),
            border: border(1.0),
            border_radius: radius(R_CARD),
            ..default()
        },
        BackgroundColor(rgba(146, 122, 86, 0.07)),
        BorderColor::all(BORDER_SOFT),
    ))
    .with_children(|c| {
        c.spawn(label(&fonts.display, title, 11.0, rgb(216, 178, 114)));
        f(c);
    });
}


#[cfg(test)]
mod tests {
    use super::*;
    use tileworld_core::town_store::{Plot, PlotState};

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<Siege>()
            .init_resource::<crate::player::HeroState>()
            .init_resource::<TownRes>()
            .init_resource::<crate::villagers::RescuedCamps>()
            .init_resource::<crate::economy::Upgrades>()
            .init_resource::<crate::economy::Defenses>()
            .init_resource::<CampaignRes>()
            .init_resource::<crate::ui::notice::Notice>()
            .add_systems(Update, observe_campaign);
        app
    }

    #[test]
    fn farm_lesson_requires_actual_food_flow_and_purchase_requires_owned_upgrade() {
        let mut app = app();
        app.world_mut().resource_mut::<TownRes>().0.plots = vec![Plot {
            kind: Some(BuildKind::Farm),
            state: PlotState::Built { hp: BuildKind::Farm.max_hp(), burning: false },
            staffed: false,
        }];
        app.update();
        assert!(!app.world().resource::<CampaignRes>().0.farm_worked);
        assert!(!app.world().resource::<CampaignRes>().0.defense_bought);
        app.world_mut().resource_mut::<TownRes>().0.plots[0].staffed = true;
        app.world_mut().resource_mut::<crate::economy::Upgrades>().0 =
            tileworld_core::upgrade_store::UpgradeState::restore(&["hero_hp_1"]);
        app.update();
        let c = &app.world().resource::<CampaignRes>().0;
        assert!(c.farm_worked);
        assert!(c.defense_bought);
        assert!(!c.ranged_prepared, "hero upgrade is no substitute for actual ranged support");
    }

    #[test]
    fn rescue_done_early_counts_but_return_requires_coming_home() {
        let mut app = app();
        app.world_mut().resource_mut::<crate::villagers::RescuedCamps>().done = vec![true];
        app.world_mut().resource_mut::<crate::player::HeroState>().pos = Vec2::new(80.0, 30.0);
        app.update();
        assert!(app.world().resource::<CampaignRes>().0.rescued);
        assert!(!app.world().resource::<CampaignRes>().0.returned);
        app.world_mut().resource_mut::<crate::player::HeroState>().pos = Vec2::ZERO;
        app.update();
        assert!(app.world().resource::<CampaignRes>().0.returned);
    }

    #[test]
    fn fresh_run_resets_guidance_and_raid_outcomes() {
        let mut app = App::new();
        app.insert_resource(QuestLogRes(QuestLog { active: 7, progress: 0.0 }))
            .insert_resource(CampaignRes(Campaign { rescued: true, defense_bought: true,
                raid_day: 2, cleared_camps: vec![true], ..Default::default() }))
            .add_systems(Update, reset_quests);
        app.update();
        assert_eq!(app.world().resource::<CampaignRes>().0, Campaign::default());
        assert_eq!(app.world().resource::<QuestLogRes>().0, QuestLog::default());
    }
}
