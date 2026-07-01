//! **Quest system (Bevy layer)** — drives the tutorial chain from `tileworld_core::quest`.
//!
//! Owns four things:
//! 1. **Detection** — tiny systems turn engine facts (bank deltas, a built farm, an animal kill,
//!    opening the War Table, surviving a night) into [`QuestSignal`] messages. Each consults the
//!    active objective first, so they only emit the signal that could matter (no flooding).
//! 2. **Resolution** — `apply_quest_signals` feeds signals to [`QuestLog::record`], and on a
//!    completion grants the reward (gold/wood/stone/item), pushes a Notice, and rings a sting.
//! 3. **Tracker** — a persistent, clickable pill on the right-center edge (reusing the
//!    `hints.rs` toast look) showing the active quest + progress.
//! 4. **Explainer** — the `Modal::Quest` card (freeze gate) opened by **J** or the tracker.
//!
//! Quest progress is cross-run progression, so it rides the save (`savegame.rs` stores the
//! `QuestLog`; [`restore_quest_log`] reconciles it from `GameLoaded`).

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use tileworld_core::quest::{Objective, QuestLog, Reward, Signal, QUESTS};
use tileworld_core::town_store::BuildKind;

use crate::audio::AudioCue;
use crate::economy::Bank;
use crate::game_state::{AppState, Modal};
use crate::inventory::{try_grant, Inventory, Toasts};
use crate::player::PlayerRes;
use crate::savegame::GameLoaded;
use crate::siege::{GamePhase, Siege};
use crate::town::TownRes;
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::texture::UiTextures;
use crate::ui::theme::*;
use crate::ui::widgets::{self, border};
use crate::ui::IconAtlas;

/// The per-run quest state (wraps the parity-tested core log). Rides the save.
#[derive(Resource, Default)]
pub struct QuestLogRes(pub QuestLog);

/// Frame-to-frame gather baseline. `None` re-seeds the baseline next frame without scoring — used
/// at boot, on a fresh run, and after a load (so the starting stipend and the load's bank jump are
/// never mistaken for gathering).
#[derive(Resource, Default)]
struct QuestTracking {
    prev_wood: Option<f64>,
    prev_stone: Option<f64>,
}

/// A reported engine fact, drained by [`apply_quest_signals`]. Decouples the (many, tiny)
/// detectors from the single reward-granting path.
#[derive(Message)]
struct QuestSignal(Signal);

pub struct QuestPlugin;

impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestLogRes>()
            .init_resource::<QuestTracking>()
            .add_message::<QuestSignal>()
            .add_systems(Startup, setup_quest_root)
            // Fresh run → restart the chain (mirrors the economy/town resets).
            .add_systems(OnExit(AppState::StartScreen), reset_quests)
            .add_systems(OnExit(AppState::GameOver), reset_quests)
            // Opening the upgrade tree is a state transition, not a Modal::None event.
            .add_systems(OnEnter(Modal::UpgradeTree), detect_war_table)
            // Explainer card.
            .add_systems(OnEnter(Modal::Quest), spawn_quest_panel)
            .add_systems(OnExit(Modal::Quest), despawn_quest_panel)
            .add_systems(Update, quest_panel_input.run_if(in_state(Modal::Quest)))
            // Detection (world running only).
            .add_systems(
                Update,
                (detect_gather, detect_builds, detect_hunt, detect_survive)
                    .run_if(in_state(Modal::None)),
            )
            // Open the explainer (world running only).
            .add_systems(Update, quest_open_input.run_if(in_state(Modal::None)))
            // Resolution + restore run ungated: a signal emitted while a panel is open (the War
            // Table) must be processed before the 2-frame message buffer drops it.
            .add_systems(Update, (apply_quest_signals, restore_quest_log))
            // The tracker checks the modal state itself (hidden behind panels / on menus).
            .add_systems(Update, drive_tracker);
    }
}

// ── Reset / restore ─────────────────────────────────────────────────────────────────────

fn reset_quests(mut log: ResMut<QuestLogRes>, mut track: ResMut<QuestTracking>) {
    log.0 = QuestLog::default();
    track.prev_wood = None;
    track.prev_stone = None;
}

/// Reconcile quest progress from a loaded snapshot (the owning-module half of the save; the
/// `QuestLog` itself is carried on `GameLoaded`). Re-seeds the gather baseline so the load's bank
/// jump isn't scored as gathering.
fn restore_quest_log(
    mut ev: MessageReader<GameLoaded>,
    mut log: ResMut<QuestLogRes>,
    mut track: ResMut<QuestTracking>,
) {
    let Some(GameLoaded(data)) = ev.read().last() else { return };
    // A save from before the quest system carries no quest data (`None`). That player already had a
    // running town, so they're past onboarding — mark the chain complete rather than restart the
    // tutorial on every load. A present log (any new save) restores verbatim.
    log.0 = match &data.quest {
        Some(q) => q.clone(),
        None => QuestLog { active: QUESTS.len(), progress: 0.0 },
    };
    track.prev_wood = None;
    track.prev_stone = None;
}

// ── Detection ───────────────────────────────────────────────────────────────────────────

/// Score wood/stone *gained* (positive bank deltas) while the matching gather quest is active.
fn detect_gather(
    log: Res<QuestLogRes>,
    bank: Res<Bank>,
    mut track: ResMut<QuestTracking>,
    mut sigs: MessageWriter<QuestSignal>,
) {
    let wood = bank.0.wood();
    let stone = bank.0.stone();
    let obj = log.0.current().map(|q| q.objective);
    if let Some(prev) = track.prev_wood {
        let d = wood - prev;
        if d > 0.0 && matches!(obj, Some(Objective::GatherWood(_))) {
            sigs.write(QuestSignal(Signal::WoodGained(d)));
        }
    }
    if let Some(prev) = track.prev_stone {
        let d = stone - prev;
        if d > 0.0 && matches!(obj, Some(Objective::GatherStone(_))) {
            sigs.write(QuestSignal(Signal::StoneGained(d)));
        }
    }
    track.prev_wood = Some(wood);
    track.prev_stone = Some(stone);
}

/// Complete the build objectives. **Producers** (Farm/Woodcutter/Mine) are read from town *state*,
/// so one raised *early* (build mode is open all through Prep) still completes its quest the moment
/// the chain reaches it — never a soft-lock on an already-occupied plot. A **House** is read from
/// the actual [`PlayerBuilt`](crate::town::PlayerBuilt) action instead (difficulty-proof: a bonus
/// starting house on Easy would trip a start-vs-current count, but not a real build event).
fn detect_builds(
    log: Res<QuestLogRes>,
    town: Res<TownRes>,
    mut built: MessageReader<crate::town::PlayerBuilt>,
    mut sigs: MessageWriter<QuestSignal>,
) {
    // Drain every frame (even off-objective) so the reader can't back up past its buffer.
    let house_raised = built.read().any(|b| b.0.is_none());
    let has_producer = |k: BuildKind| town.0.plots.iter().any(|p| p.is_built() && p.kind == Some(k));
    let Some(q) = log.0.current() else { return };
    match q.objective {
        Objective::BuildFarm if has_producer(BuildKind::Farm) => {
            sigs.write(QuestSignal(Signal::FarmBuilt));
        }
        Objective::BuildLumber if has_producer(BuildKind::Lumber) => {
            sigs.write(QuestSignal(Signal::LumberBuilt));
        }
        Objective::BuildMine if has_producer(BuildKind::Mine) => {
            sigs.write(QuestSignal(Signal::MineBuilt));
        }
        Objective::BuildHouse if house_raised => {
            sigs.write(QuestSignal(Signal::HouseBuilt));
        }
        _ => {}
    }
}

/// Each wild-animal kill (drained even when off-quest so the reader can't back up).
fn detect_hunt(
    log: Res<QuestLogRes>,
    mut kills: MessageReader<crate::verbs::AnimalKilled>,
    mut sigs: MessageWriter<QuestSignal>,
) {
    let hunting = matches!(log.0.current().map(|q| q.objective), Some(Objective::HuntAnimal(_)));
    let n = kills.read().count();
    if hunting {
        for _ in 0..n {
            sigs.write(QuestSignal(Signal::AnimalHunted));
        }
    }
}

/// The `Wave → Prep` dawn edge — a survived night (same edge the autosave fires on).
fn detect_survive(
    mut prev: Local<Option<GamePhase>>,
    siege: Res<Siege>,
    mut sigs: MessageWriter<QuestSignal>,
) {
    let phase = siege.phase;
    let was = prev.replace(phase);
    if was == Some(GamePhase::Wave) && phase == GamePhase::Prep {
        sigs.write(QuestSignal(Signal::NightSurvived));
    }
}

/// Opening the War Table (runs on the `OnEnter(Modal::UpgradeTree)` transition).
fn detect_war_table(mut sigs: MessageWriter<QuestSignal>) {
    sigs.write(QuestSignal(Signal::WarTableOpened));
}

// ── Resolution ──────────────────────────────────────────────────────────────────────────

/// Drain reported signals into the log; on each completion grant the reward + celebrate.
#[allow(clippy::too_many_arguments)]
fn apply_quest_signals(
    mut sigs: MessageReader<QuestSignal>,
    mut log: ResMut<QuestLogRes>,
    mut player: ResMut<PlayerRes>,
    mut bank: ResMut<Bank>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    mut cues: MessageWriter<AudioCue>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    for QuestSignal(sig) in sigs.read() {
        let Some(done) = log.0.record(*sig) else { continue };
        let q = &QUESTS[done];
        grant_reward(q.reward, &mut player, &mut bank, &mut inv, &mut toasts, now);
        notice.push(format!("Quest complete — {}", q.title), now);
        cues.write(AudioCue::LevelUp);
        if log.0.is_complete() {
            notice.push("Your hold stands. The keep is yours to defend.", now);
        }
    }
}

/// Pay out a quest reward into the live resources / satchel.
fn grant_reward(
    r: Reward,
    player: &mut PlayerRes,
    bank: &mut Bank,
    inv: &mut Inventory,
    toasts: &mut Toasts,
    now: f64,
) {
    if r.gold != 0 {
        player.0.add_gold(r.gold);
    }
    if r.wood > 0.0 {
        bank.0.add_wood(r.wood);
    }
    if r.stone > 0.0 {
        bank.0.add_stone(r.stone);
    }
    if let Some((id, n)) = r.item {
        try_grant(&mut inv.0, &mut toasts.0, id, n, now);
    }
}

// ── Tracker (right-center pill) ───────────────────────────────────────────────────────────

#[derive(Component)]
struct QuestRoot;
/// The persistent, clickable tracker pill. Spawned once; its children (icon/text/bar) are rebuilt
/// each frame, but the button entity itself lives so clicks register across frames.
#[derive(Component)]
struct QuestCard;

fn setup_quest_root(mut commands: Commands) {
    commands
        .spawn((
            QuestRoot,
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

/// Rebuild the tracker's content each frame for the active quest. Hidden (collapsed) behind any
/// panel, on the menus, and once the chain is complete.
fn drive_tracker(
    time: Res<Time>,
    log: Res<QuestLogRes>,
    modal: Option<Res<State<Modal>>>,
    atlas: Res<IconAtlas>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
    mut card_q: Query<
        (Entity, &mut Node, &mut BorderColor, Option<&Children>),
        With<QuestCard>,
    >,
) {
    let Ok((card, mut node, mut bcol, children)) = card_q.single_mut() else { return };

    // Visible only while actually playing with no panel up and a quest still active.
    let playing = modal.as_ref().map_or(false, |m| *m.get() == Modal::None);
    let Some(q) = log.0.current().filter(|_| playing) else {
        if node.display != Display::None {
            node.display = Display::None;
            if let Some(children) = children {
                for &c in children {
                    commands.entity(c).try_despawn();
                }
            }
        }
        return;
    };

    node.display = Display::Flex;
    // Slow gold pulse on the border (matches the hints toast).
    let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 3.0).sin();
    *bcol = BorderColor::all(GOLD.with_alpha(0.45 + 0.4 * pulse));

    // Rebuild children (cheap — a few nodes; the button shell persists for click detection).
    if let Some(children) = children {
        for &c in children {
            commands.entity(c).try_despawn();
        }
    }
    let obj = q.objective;
    let frac = log.0.fraction() as f32;
    let progress = log.0.progress;
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
            if obj.is_metered() {
                col.spawn(label(
                    &fonts.semibold,
                    format!("{} / {}", progress.floor() as i64, obj.target() as i64),
                    12.0,
                    TEXT,
                ));
                tracker_bar(col, frac);
            } else {
                col.spawn(label(&fonts.regular, q.action, 12.0, TEXT_DIM));
            }
            col.spawn(label(&fonts.regular, "J — what's this?", 10.0, GREY));
        });
    });
}

/// A thin gold progress bar for metered quests.
fn tracker_bar(p: &mut RelatedSpawnerCommands<ChildOf>, frac: f32) {
    p.spawn((
        Node {
            width: Val::Px(150.0),
            height: Val::Px(7.0),
            border: border(1.0),
            border_radius: radius(R_SLOT),
            overflow: Overflow::clip(),
            margin: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(SLOT_BG),
        BorderColor::all(SLOT_BORDER),
    ))
    .with_children(|track| {
        track.spawn((
            Node {
                width: Val::Percent(frac.clamp(0.0, 1.0) * 100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            widgets::vgrad(GOLD, GOLD_DEEP),
        ));
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
    log: Res<QuestLogRes>,
    card: Query<&Interaction, (With<QuestCard>, Changed<Interaction>)>,
    mut next: ResMut<NextState<Modal>>,
    mut cues: MessageWriter<AudioCue>,
    mut auto_done: Local<bool>,
) {
    if log.0.is_complete() {
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
    log: Res<QuestLogRes>,
    fonts: Res<UiFonts>,
    atlas: Res<IconAtlas>,
    tex: Res<UiTextures>,
    assets: Res<AssetServer>,
) {
    // Guard: the panel only opens with an active quest, but bail cleanly if the chain finished
    // between the keypress and this OnEnter.
    let Some(q) = log.0.current() else {
        commands.spawn(QuestPanelUi); // empty marker so OnExit has something to despawn
        return;
    };

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
                        t.spawn(label(&fonts.display, "QUEST", 12.0, KICKER));
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

            // Reward preview.
            panel_section(card, &fonts, "REWARD", |c| {
                reward_chips(c, &fonts, &atlas, q.reward);
            });

            card.spawn(label(&fonts.regular, "Finish it to claim the reward.", 11.0, GREY));
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

/// Render a reward as icon+amount chips (gold / wood / stone / item).
fn reward_chips(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    fonts: &UiFonts,
    atlas: &IconAtlas,
    r: Reward,
) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(14.0),
        flex_wrap: FlexWrap::Wrap,
        row_gap: Val::Px(6.0),
        ..default()
    })
    .with_children(|row| {
        let mut chip = |icon: &str, text: String| {
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|c| {
                if let Some(entry) = atlas.get_tintable(icon) {
                    c.spawn(widgets::icon_tinted(entry, 18.0, GOLD));
                }
                c.spawn(label(&fonts.bold, text, 13.5, GOLD));
            });
        };
        if r.gold != 0 {
            chip("sym:gold", format!("{} gold", r.gold));
        }
        if r.wood > 0.0 {
            chip("stat:wood", format!("{} wood", r.wood as i64));
        }
        if r.stone > 0.0 {
            chip("stat:stone", format!("{} stone", r.stone as i64));
        }
        if let Some((id, n)) = r.item {
            let name = tileworld_core::inventory::item_def(id).map(|d| d.name).unwrap_or(id);
            chip("stat:food", format!("{name} ×{n}"));
        }
    });
}
