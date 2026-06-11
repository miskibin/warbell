//! Top-level run **state machine** + the **freeze gate**.
//!
//! `AppState` is the coarse mode (start screen / playing / paused / game-over). `Modal` is a
//! sub-state that exists **only while `Playing`** — opening a panel (shop / upgrade tree /
//! inventory) flips it off `None`. The whole world-sim is gated on `in_state(Modal::None)`,
//! so *any* panel (or leaving `Playing`) freezes the world. This is the declarative form of
//! the TS `isFrozen()` (`paused || shopOpen || treeOpen || inventoryOpen`).
//!
//! Because `in_state(S)` is **false when `State<S>` doesn't exist**, and `State<Modal>` only
//! exists inside `Playing`, the single condition `in_state(Modal::None)` is true *only* when
//! actually playing with no panel open — exactly the freeze gate. Sim systems carry it; the
//! render/camera/anim/audio/HUD systems stay ungated so the frozen world still draws.
//!
//! Panels are a sub-state **of** `Playing` (not sibling `AppState` variants) on purpose:
//! opening one does NOT fire `OnExit(Playing)`/`OnEnter(Playing)`, so it never wipes the run.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowMode};

use crate::quality::GraphicsQuality;
use crate::ui::anim::{anim, anim_btn, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::notice::Notice;
use crate::ui::settings::AudioSettings;
use crate::ui::theme::*;
use crate::ui::widgets;

/// Coarse run mode. Boots to `StartScreen` (unless a screenshot/demo env hook skips the menu).
#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AppState {
    #[default]
    StartScreen,
    Playing,
    Paused,
    GameOver,
}

/// Modal panel sub-state — exists only while `Playing`. `None` = no panel = world runs.
/// (`Shop`/`UpgradeTree`/`Inventory` are wired by their panels in P2–P3.)
#[derive(SubStates, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[source(AppState = AppState::Playing)]
#[allow(dead_code)]
pub enum Modal {
    #[default]
    None,
    Shop,
    UpgradeTree,
    Inventory,
    Tutorial,
    Build,
}

/// Set true by the pause menu's **Restart** / **Load last save** before it hands back to
/// `Playing`, so the per-plugin "fresh run" resets (normally keyed to leaving the start /
/// game-over screens) also fire on the `Paused → Playing` edge. A plain resume leaves it `false`,
/// so the world is untouched. Cleared again on `OnEnter(Playing)`.
#[derive(Resource, Default)]
pub struct RestartRequested(pub bool);

/// Run condition for the gated `OnExit(AppState::Paused)` reset systems (see [`RestartRequested`]).
pub fn restart_requested(flag: Res<RestartRequested>) -> bool {
    flag.0
}

/// The "Overwrite saved game?" confirm dialog. `Some(from_pause)` = open; `from_pause` records
/// whether the request came from the pause-menu **Restart** (so confirming sets [`RestartRequested`]
/// for the `Paused → Playing` resets) versus a start/game-over **New Game** (whose fresh-run resets
/// fire automatically on the screen's `OnExit`). `None` = closed. Every fresh-run button routes
/// through this whenever a save exists, so a misclick can't wipe a long run.
#[derive(Resource, Default)]
pub struct ConfirmWipe(pub Option<bool>);

pub struct GameStatePlugin;

impl Plugin for GameStatePlugin {
    fn build(&self, app: &mut App) {
        // Screenshot/demo hooks want a live world without the menu in the way.
        let boot = if skip_menu() { AppState::Playing } else { AppState::StartScreen };
        app.insert_state(boot)
            .add_sub_state::<Modal>()
            .init_resource::<RestartRequested>()
            .init_resource::<ConfirmWipe>()
            // Overwrite-confirm dialog: reconcile its overlay + resolve its input. Ungated so it
            // works over the start / game-over / pause screens alike.
            .add_systems(Update, (sync_confirm_overlay, confirm_input))
            .add_systems(Update, (pause_toggle, watch_end))
            // Clear the restart flag once we're back in Playing (after the gated OnExit(Paused)
            // resets have run during the same transition).
            .add_systems(OnEnter(AppState::Playing), clear_restart_flag)
            // Pause-menu buttons + live settings labels (only while the pause screen is up).
            .add_systems(
                Update,
                (pause_click, pause_settings_sync).run_if(in_state(AppState::Paused)),
            )
            // Minimal overlays (fleshed out + difficulty chooser in P0.6).
            .add_systems(OnEnter(AppState::StartScreen), spawn_start_screen)
            .add_systems(OnExit(AppState::StartScreen), despawn_screen::<StartScreenUi>)
            .add_systems(OnEnter(AppState::Paused), spawn_pause_screen)
            .add_systems(OnExit(AppState::Paused), despawn_screen::<PausedUi>)
            .add_systems(OnEnter(AppState::GameOver), spawn_gameover_screen)
            .add_systems(OnExit(AppState::GameOver), despawn_screen::<GameOverUi>)
            .add_systems(
                Update,
                (start_screen_input, cycle_difficulty, start_click, update_diff_seg)
                    .run_if(in_state(AppState::StartScreen)),
            )
            .add_systems(
                Update,
                (gameover_input, gameover_click).run_if(in_state(AppState::GameOver)),
            );
    }
}

/// True when an env hook wants to boot straight into a running world (screenshots / demos).
/// `FOREST_MENU=1` overrides — it keeps the start screen up so the harness can shoot it.
fn skip_menu() -> bool {
    if std::env::var("FOREST_MENU").is_ok() {
        return false;
    }
    std::env::var("FOREST_SHOT").is_ok()
        || std::env::var("FOREST_CLIP").is_ok()
        || std::env::var("FOREST_WAVE").is_ok()
        || std::env::var("FOREST_BIOME").is_ok()
}

// ── Transitions ──────────────────────────────────────────────────────────────────────

/// Esc: close the open panel if any, else toggle pause. (Pointer-lock release is reconciled
/// in `player::camera` — both may fire on the same Esc, which is the desired "free the cursor
/// when paused" behaviour.)
fn pause_toggle(
    keys: Res<ButtonInput<KeyCode>>,
    app: Res<State<AppState>>,
    modal: Option<Res<State<Modal>>>,
    mut next_app: ResMut<NextState<AppState>>,
    mut next_modal: ResMut<NextState<Modal>>,
    confirm: Res<ConfirmWipe>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    // While the overwrite dialog is up, Esc belongs to it (cancel), not the pause toggle.
    if confirm.0.is_some() {
        return;
    }
    match app.get() {
        AppState::Playing => {
            let no_panel = modal.map_or(true, |m| *m.get() == Modal::None);
            if no_panel {
                next_app.set(AppState::Paused);
            } else {
                next_modal.set(Modal::None); // close the panel
            }
        }
        AppState::Paused => next_app.set(AppState::Playing),
        _ => {}
    }
}

/// Hand off to the GameOver screen when the siege reaches an end state.
fn watch_end(
    siege: Option<Res<crate::siege::Siege>>,
    app: Res<State<AppState>>,
    mut next_app: ResMut<NextState<AppState>>,
) {
    if *app.get() != AppState::Playing {
        return;
    }
    if let Some(s) = siege {
        if matches!(s.phase, crate::siege::GamePhase::Victory | crate::siege::GamePhase::Defeat) {
            next_app.set(AppState::GameOver);
        }
    }
}

fn start_screen_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
) {
    // While the overwrite dialog is up, it owns the keyboard (see `confirm_input`).
    if confirm.0.is_some() {
        return;
    }
    // C resumes the saved run; Enter/Space starts a fresh one (confirming first if it'd overwrite).
    if save.0 && keys.just_pressed(KeyCode::KeyC) {
        pending.0 = crate::savegame::load_save();
        next_app.set(AppState::Playing);
    } else if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        if save.0 {
            confirm.0 = Some(false); // ask before wiping the existing run
        } else {
            pending.0 = None;
            next_app.set(AppState::Playing);
        }
    }
}

/// On the start screen, **G** cycles the difficulty (reusing the siege bind so it doesn't clash
/// with the dev `1-5` biome keys). The segmented control reflects it via [`update_diff_seg`].
fn cycle_difficulty(keys: Res<ButtonInput<KeyCode>>, siege: Option<ResMut<crate::siege::Siege>>) {
    if !keys.just_pressed(KeyCode::KeyG) {
        return;
    }
    let Some(mut siege) = siege else { return };
    use crate::siege::Difficulty::*;
    siege.difficulty = match siege.difficulty {
        Easy => Normal,
        Normal => Hard,
        Hard => Easy,
    };
}

fn diff_name(d: crate::siege::Difficulty) -> &'static str {
    use crate::siege::Difficulty::*;
    match d {
        Easy => "Easy",
        Normal => "Normal",
        Hard => "Hard",
    }
}

const DIFFS: [crate::siege::Difficulty; 3] = [
    crate::siege::Difficulty::Easy,
    crate::siege::Difficulty::Normal,
    crate::siege::Difficulty::Hard,
];

fn gameover_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    siege: Option<Res<crate::siege::Siege>>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
) {
    if confirm.0.is_some() {
        return; // dialog owns the keyboard
    }
    let defeat = !matches!(siege.as_deref().map(|s| s.phase), Some(crate::siege::GamePhase::Victory));
    // C resumes last night (defeat + save only); Enter starts a fresh run (confirming first if it'd
    // overwrite). The per-plugin OnExit(GameOver) resets rebuild a fresh run on confirm.
    if defeat && save.0 && keys.just_pressed(KeyCode::KeyC) {
        pending.0 = crate::savegame::load_save();
        next_app.set(AppState::Playing);
    } else if keys.just_pressed(KeyCode::Enter) {
        if save.0 {
            confirm.0 = Some(false);
        } else {
            pending.0 = None;
            next_app.set(AppState::Playing);
        }
    }
}

// ── Screens (cinematic start / pause / game-over), ported from the 3js HUD ──────────────

#[derive(Component)]
struct StartScreenUi;
#[derive(Component)]
struct PausedUi;
#[derive(Component)]
struct GameOverUi;
/// The "Play" / "New Game" button on the start screen (always a fresh run).
#[derive(Component)]
struct StartPlayButton;
/// The "Continue" button on the start screen (loads the save). Only spawned when a save exists.
#[derive(Component)]
struct StartContinueButton;
/// A difficulty segment (click to select).
#[derive(Component)]
struct SegButton(crate::siege::Difficulty);
// ── Pause-menu buttons ──
#[derive(Component)]
struct PauseResumeBtn;
#[derive(Component)]
struct PauseLoadBtn;
#[derive(Component)]
struct PauseRestartBtn;
#[derive(Component)]
struct PauseAudioBtn;
#[derive(Component)]
struct PauseGfxBtn;
#[derive(Component)]
struct PauseFsBtn;
#[derive(Component)]
struct PauseAudioLabel;
#[derive(Component)]
struct PauseGfxLabel;
#[derive(Component)]
struct PauseFsLabel;
/// The "Play again" / "New Game" button on the game-over screen (a fresh run).
#[derive(Component)]
struct AgainButton;
/// The "Continue from last night" button on the game-over screen (loads the save).
#[derive(Component)]
struct GameOverContinueButton;
// ── Overwrite-confirm dialog ──
#[derive(Component)]
struct ConfirmUi;
#[derive(Component)]
struct ConfirmOkBtn;
#[derive(Component)]
struct ConfirmCancelBtn;

/// A centred full-screen scrim card root (pause / game-over).
fn modal_root(z: i32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            row_gap: Val::Px(16.0),
            ..default()
        },
        BackgroundColor(SCRIM),
        GlobalZIndex(z),
    )
}

/// The cinematic start screen: a live scene behind a left-heavy scrim, with the title block,
/// Play button, difficulty selector (lower-left) and a controls legend (lower-right).
fn spawn_start_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    siege: Option<Res<crate::siege::Siege>>,
    mut save: ResMut<crate::savegame::SaveExists>,
) {
    let cur = siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);
    // Re-check the file here: Bevy runs the initial `OnEnter(StartScreen)` *before* `Startup`
    // (where `detect_existing_save` sets the flag), so reading `save.0` directly would miss an
    // existing save on a fresh launch. Recompute + write it back so the flag is right from frame 0.
    let has_save = crate::savegame::load_save().is_some();
    save.0 = has_save;

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // Left-heavy cinematic scrim so the title reads while the scene stays bright at right.
            BackgroundGradient(vec![Gradient::Linear(LinearGradient::new(
                std::f32::consts::FRAC_PI_2, // → right
                vec![
                    ColorStop::new(rgba(6, 10, 20, 0.78), Val::Percent(0.0)),
                    ColorStop::new(rgba(6, 10, 20, 0.32), Val::Percent(40.0)),
                    ColorStop::new(rgba(6, 10, 20, 0.0), Val::Percent(72.0)),
                ],
            ))]),
            GlobalZIndex(50),
            StartScreenUi,
        ))
        .with_children(|root| {
            // ── Lower-left menu column ──
            root.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(72.0),
                bottom: Val::Px(80.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: Val::Px(12.0),
                ..default()
            })
            .with_children(|m| {
                m.spawn((label(&fonts.display, "DEFEND THE KEEP", 13.0, KICKER), anim(AnimKind::Rise, 0.06, 0.6)));
                // The title — Cinzel roman capitals, gold-lit.
                m.spawn((
                    Node { flex_direction: FlexDirection::Column, ..default() },
                    anim(AnimKind::Rise, 0.12, 0.7),
                ))
                .with_children(|t| {
                    t.spawn((
                        label(&fonts.display, "WARBELL", 88.0, rgb(244, 228, 188)),
                        TextShadow { offset: Vec2::new(0.0, 6.0), color: rgba(0, 0, 0, 0.65) },
                    ));
                    // Gold rule under the title, like a charter heading.
                    t.spawn((
                        Node { width: Val::Px(330.0), height: Val::Px(2.0), margin: UiRect::top(Val::Px(2.0)), ..default() },
                        BackgroundColor(rgba(224, 168, 74, 0.75)),
                    ));
                });
                m.spawn((label(&fonts.regular, "A knight's last stand.", 17.0, TEXT_DIM), anim(AnimKind::Rise, 0.2, 0.7)));
                // Quick "how to play" — the whole loop in two lines, so a new player isn't lost.
                m.spawn((
                    label(&fonts.semibold, "DAY  —  loot chests, gather gold & stone, buy upgrades", 13.5, rgb(214, 224, 240)),
                    anim(AnimKind::Rise, 0.24, 0.7),
                ));
                m.spawn((
                    label(&fonts.semibold, "NIGHT  —  orks besiege the keep; hold the walls", 13.5, rgb(214, 224, 240)),
                    anim(AnimKind::Rise, 0.27, 0.7),
                ));
                // Divider.
                m.spawn((
                    Node { width: Val::Px(220.0), height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(6.0)), ..default() },
                    BackgroundColor(rgba(199, 155, 106, 0.6)),
                    anim(AnimKind::Rise, 0.3, 0.7),
                ));
                // Continue button — always shown; the primary action when resuming. Dim + inert
                // when there's no save yet (so the menu reads as "New Game / Continue Game").
                if has_save {
                    m.spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(44.0), Val::Px(13.0)),
                            border: widgets::border(1.0),
                            border_radius: radius(11.0),
                            ..default()
                        },
                        widgets::btn_primary_paint(),
                        StartContinueButton,
                        anim_btn(AnimKind::Rise, 0.34, 0.7),
                    ))
                    .with_children(|b| {
                        b.spawn(label(&fonts.extrabold, "CONTINUE GAME", 19.0, INK));
                    });
                } else {
                    // Disabled: no Button/Interaction/Hoverable, just a dim card.
                    m.spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(44.0), Val::Px(13.0)),
                            border: widgets::border(1.0),
                            border_radius: radius(11.0),
                            ..default()
                        },
                        BackgroundColor(rgba(196, 144, 62, 0.16)),
                        BorderColor::all(rgba(244, 204, 132, 0.22)),
                        anim(AnimKind::Rise, 0.34, 0.7),
                    ))
                    .with_children(|b| {
                        b.spawn(label(&fonts.extrabold, "CONTINUE GAME", 19.0, GREY));
                    });
                }
                // New Game — a fresh run (confirms first if it'd overwrite a save).
                m.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(44.0), Val::Px(13.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(11.0),
                        ..default()
                    },
                    widgets::btn_primary_paint(),
                    StartPlayButton,
                    anim_btn(AnimKind::Rise, 0.36, 0.7),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.extrabold, "NEW GAME", 19.0, INK));
                });
                // Difficulty selector.
                m.spawn((
                    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(7.0), ..default() },
                    anim(AnimKind::Rise, 0.34, 0.7),
                ))
                .with_children(|d| {
                    d.spawn(label(&fonts.semibold, "DIFFICULTY", 11.0, KICKER));
                    d.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            padding: UiRect::all(Val::Px(3.0)),
                            border: widgets::border(1.0),
                            border_radius: radius(10.0),
                            ..default()
                        },
                        BackgroundColor(rgba(24, 19, 13, 0.72)),
                        BorderColor::all(BORDER_SOFT),
                    ))
                    .with_children(|seg| {
                        for d in DIFFS {
                            let on = d == cur;
                            seg.spawn((
                                Button,
                                Interaction::default(),
                                Node {
                                    padding: UiRect::axes(Val::Px(20.0), Val::Px(7.0)),
                                    border_radius: radius(7.0),
                                    ..default()
                                },
                                BackgroundColor(if on { GOLD_DEEP } else { Color::NONE }),
                                BorderColor::all(Color::NONE),
                                SegButton(d),
                            ))
                            .with_children(|b| {
                                b.spawn(label(&fonts.semibold, diff_name(d), 13.0, if on { INK } else { TEXT_FAINT }));
                            });
                        }
                    });
                });
            });

            // ── Lower-right controls legend ──
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(36.0),
                    bottom: Val::Px(36.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::End,
                    row_gap: Val::Px(6.0),
                    ..default()
                },
                anim(AnimKind::Rise, 0.52, 0.7),
            ))
            .with_children(|legend| {
                let rows: &[(&[&str], &str)] = &[
                    (&["W", "A", "S", "D"], "Move"),
                    (&["LMB"], "Attack"),
                    (&["RMB"], "Block"),
                    (&["E"], "Interact"),
                    (&["F"], "Loot"),
                    (&["I"], "Satchel"),
                    (&["R"], "Recruit"),
                    (&["H"], "Help"),
                    (&["Esc"], "Pause"),
                ];
                for (keys, desc) in rows {
                    legend
                        .spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() })
                        .with_children(|row| {
                            row.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(3.0), ..default() })
                                .with_children(|kc| {
                                    for k in keys.iter() {
                                        kc.spawn((
                                            Node {
                                                padding: UiRect::axes(Val::Px(7.0), Val::Px(3.0)),
                                                border: widgets::border(1.0),
                                                border_radius: radius(5.0),
                                                ..default()
                                            },
                                            widgets::keycap_paint(),
                                        ))
                                        .with_children(|c| {
                                            c.spawn(label(&fonts.bold, *k, 11.0, rgb(233, 238, 251)));
                                        });
                                    }
                                });
                            row.spawn(label(&fonts.regular, *desc, 12.0, TEXT_FAINT));
                        });
                }
            });
        });
}

fn audio_label(muted: bool) -> String {
    format!("Audio: {}", if muted { "Off" } else { "On" })
}
fn fs_label(fullscreen: bool) -> String {
    format!("Fullscreen: {}", if fullscreen { "On" } else { "Off" })
}

/// A full-width pause-menu button (primary-blue paint), with `text` and an optional extra bundle
/// on its label (a `PauseXLabel` marker for the live-syncing settings rows; `()` otherwise).
fn pause_btn<M: Component, L: Bundle>(
    p: &mut RelatedSpawnerCommands<ChildOf>,
    font: &Handle<Font>,
    text: &str,
    marker: M,
    label_extra: L,
    delay: f32,
) {
    p.spawn((
        Node {
            width: Val::Px(264.0),
            padding: UiRect::axes(Val::Px(18.0), Val::Px(10.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: widgets::border(1.0),
            border_radius: radius(R_BTN),
            ..default()
        },
        widgets::btn_primary_paint(),
        marker,
        anim_btn(AnimKind::PopIn, delay, 0.28),
    ))
    .with_children(|b| {
        b.spawn((label(font, text, 16.0, INK), label_extra));
    });
}

fn spawn_pause_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    save: Res<crate::savegame::SaveExists>,
    audio: Res<AudioSettings>,
    quality: Res<GraphicsQuality>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let has_save = save.0;
    let audio_txt = audio_label(audio.muted);
    let gfx_txt = format!("Graphics: {}", quality.label());
    let fs_on = windows.single().map(|w| !matches!(w.mode, WindowMode::Windowed)).unwrap_or(false);
    let fs_txt = fs_label(fs_on);

    commands.spawn((modal_root(50), PausedUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(9.0),
                padding: UiRect::axes(Val::Px(40.0), Val::Px(28.0)),
                border: widgets::border(1.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            anim(AnimKind::PopIn, 0.0, 0.26),
        ))
        .with_children(|c| {
            c.spawn((
                label(&fonts.display, "PAUSED", 34.0, GOLD),
                Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() },
            ));

            pause_btn(c, &fonts.extrabold, "RESUME", PauseResumeBtn, (), 0.04);

            // ── Settings (toggle in place; labels live-sync via pause_settings_sync) ──
            c.spawn((
                label(&fonts.semibold, "SETTINGS", 11.0, KICKER),
                Node { margin: UiRect::top(Val::Px(8.0)), ..default() },
            ));
            pause_btn(c, &fonts.bold, &audio_txt, PauseAudioBtn, PauseAudioLabel, 0.06);
            pause_btn(c, &fonts.bold, &gfx_txt, PauseGfxBtn, PauseGfxLabel, 0.08);
            pause_btn(c, &fonts.bold, &fs_txt, PauseFsBtn, PauseFsLabel, 0.10);

            // ── Run controls ──
            c.spawn((
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(1.0),
                    margin: UiRect::vertical(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(BORDER_SOFT),
            ));
            if has_save {
                pause_btn(c, &fonts.extrabold, "LOAD LAST SAVE", PauseLoadBtn, (), 0.12);
            }
            pause_btn(c, &fonts.extrabold, "RESTART", PauseRestartBtn, (), 0.14);

            c.spawn((
                label(&fonts.regular, "Esc to resume", 13.0, GREY),
                Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
            ));
        });
    });
}

/// Handle the pause-menu buttons: resume, the three in-place settings toggles, and the two
/// run actions (Load last save / Restart) that both re-run the fresh-start resets via
/// [`RestartRequested`] before handing control back to `Playing`.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn pause_click(
    q: Query<
        (
            &Interaction,
            Option<&PauseResumeBtn>,
            Option<&PauseLoadBtn>,
            Option<&PauseRestartBtn>,
            Option<&PauseAudioBtn>,
            Option<&PauseGfxBtn>,
            Option<&PauseFsBtn>,
        ),
        Changed<Interaction>,
    >,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    mut restart: ResMut<RestartRequested>,
    mut audio: ResMut<AudioSettings>,
    mut quality: ResMut<GraphicsQuality>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
) {
    if confirm.0.is_some() {
        return; // dialog owns input
    }
    let now = time.elapsed_secs_f64();
    for (interaction, resume, load, restart_b, audio_b, gfx_b, fs_b) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if resume.is_some() {
            next_app.set(AppState::Playing);
        }
        if load.is_some() {
            // Clear the in-progress field/entities (restart resets), then the loaded snapshot
            // overwrites the resources on the next Playing frame — identical to Continue.
            pending.0 = crate::savegame::load_save();
            restart.0 = true;
            next_app.set(AppState::Playing);
        }
        if restart_b.is_some() {
            if save.0 {
                confirm.0 = Some(true); // Restart wipes the save too — confirm (from_pause = true)
            } else {
                pending.0 = None; // fresh run (keeps the chosen difficulty)
                restart.0 = true;
                next_app.set(AppState::Playing);
            }
        }
        if audio_b.is_some() {
            crate::ui::settings::toggle_mute(&mut audio, &mut notice, now);
        }
        if gfx_b.is_some() {
            crate::ui::settings::toggle_quality(&mut quality, &mut notice, now);
        }
        if fs_b.is_some() {
            crate::ui::settings::toggle_fullscreen(&mut windows, &mut notice, now);
        }
    }
}

/// Keep the three pause settings buttons' labels matching live state (they can also be flipped by
/// the M / F10 / F11 keys while paused).
#[allow(clippy::type_complexity)]
fn pause_settings_sync(
    audio: Res<AudioSettings>,
    quality: Res<GraphicsQuality>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut audio_l: Query<&mut Text, (With<PauseAudioLabel>, Without<PauseGfxLabel>, Without<PauseFsLabel>)>,
    mut gfx_l: Query<&mut Text, (With<PauseGfxLabel>, Without<PauseAudioLabel>, Without<PauseFsLabel>)>,
    mut fs_l: Query<&mut Text, (With<PauseFsLabel>, Without<PauseAudioLabel>, Without<PauseGfxLabel>)>,
) {
    if let Ok(mut t) = audio_l.single_mut() {
        **t = audio_label(audio.muted);
    }
    if let Ok(mut t) = gfx_l.single_mut() {
        **t = format!("Graphics: {}", quality.label());
    }
    if let Ok(mut t) = fs_l.single_mut() {
        let on = windows.single().map(|w| !matches!(w.mode, WindowMode::Windowed)).unwrap_or(false);
        **t = fs_label(on);
    }
}

fn clear_restart_flag(mut restart: ResMut<RestartRequested>) {
    restart.0 = false;
}

fn spawn_gameover_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    siege: Option<Res<crate::siege::Siege>>,
    player: Option<Res<crate::player::PlayerRes>>,
    save: Res<crate::savegame::SaveExists>,
) {
    let won = matches!(siege.as_deref().map(|s| s.phase), Some(crate::siege::GamePhase::Victory));
    let (title, col) = if won {
        ("VICTORY", rgb(255, 231, 154))
    } else {
        ("THE KEEP HAS FALLEN", rgb(255, 106, 90))
    };
    let stats = player
        .map(|p| format!("Level {}     Gold {}", p.0.level, p.0.gold))
        .unwrap_or_default();
    // On a defeat with a save, offer to resume last night; a victory ends the saga (no Continue).
    let can_continue = !won && save.0;

    commands.spawn((modal_root(50), GameOverUi)).with_children(|root| {
        root.spawn((
            label(&fonts.display, title, 58.0, col),
            TextShadow { offset: Vec2::new(0.0, 4.0), color: rgba(0, 0, 0, 0.8) },
            anim(AnimKind::PopIn, 0.0, 0.6),
        ));
        if !stats.is_empty() {
            root.spawn((label(&fonts.semibold, stats, 16.0, GOLD), anim(AnimKind::FloatUp, 0.2, 0.5)));
        }
        // Continue from last night (defeat + save only) — the primary "don't lose progress" action.
        if can_continue {
            root.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(26.0), Val::Px(11.0)),
                    border: widgets::border(1.0),
                    border_radius: radius(R_CARD),
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
                widgets::btn_primary_paint(),
                GameOverContinueButton,
                anim_btn(AnimKind::FloatUp, 0.35, 0.5),
            ))
            .with_children(|b| {
                b.spawn(label(&fonts.extrabold, "CONTINUE FROM LAST NIGHT", 15.0, INK));
            });
        }
        root.spawn((
            Node {
                padding: UiRect::axes(Val::Px(26.0), Val::Px(11.0)),
                border: widgets::border(1.0),
                border_radius: radius(R_CARD),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
            widgets::btn_primary_paint(),
            AgainButton,
            anim_btn(AnimKind::FloatUp, 0.4, 0.5),
        ))
        .with_children(|b| {
            let txt = if can_continue { "NEW GAME" } else { "PLAY AGAIN" };
            b.spawn(label(&fonts.extrabold, txt, 15.0, INK));
        });
    });
}

/// Click "Continue" / "New Game" / a difficulty segment on the start screen.
#[allow(clippy::type_complexity)]
fn start_click(
    q: Query<
        (
            &Interaction,
            Option<&StartPlayButton>,
            Option<&StartContinueButton>,
            Option<&SegButton>,
        ),
        Changed<Interaction>,
    >,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    siege: Option<ResMut<crate::siege::Siege>>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
) {
    if confirm.0.is_some() {
        return; // dialog up — the scrim already blocks these, but be explicit
    }
    let mut siege = siege;
    for (interaction, play, cont, seg) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if play.is_some() {
            if save.0 {
                confirm.0 = Some(false); // New Game would overwrite — confirm first
            } else {
                pending.0 = None; // New Game: a fresh run, never a load.
                next_app.set(AppState::Playing);
            }
        }
        if cont.is_some() && save.0 {
            pending.0 = crate::savegame::load_save();
            next_app.set(AppState::Playing);
        }
        if let Some(seg) = seg
            && let Some(s) = siege.as_deref_mut()
        {
            s.difficulty = seg.0;
        }
    }
}

/// Recolour the difficulty segments to match the live selection (G key or click).
fn update_diff_seg(
    siege: Option<Res<crate::siege::Siege>>,
    mut q: Query<(&SegButton, &mut BackgroundColor, &Children)>,
    mut text_q: Query<&mut TextColor>,
) {
    let cur = siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);
    for (seg, mut bg, children) in &mut q {
        let on = seg.0 == cur;
        bg.0 = if on { GOLD_DEEP } else { Color::NONE };
        for child in children.iter() {
            if let Ok(mut tc) = text_q.get_mut(child) {
                tc.0 = if on { Color::WHITE } else { TEXT_FAINT };
            }
        }
    }
}

/// Click "New Game" / "Continue from last night" on the game-over screen.
#[allow(clippy::type_complexity)]
fn gameover_click(
    q: Query<
        (&Interaction, Option<&AgainButton>, Option<&GameOverContinueButton>),
        Changed<Interaction>,
    >,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
) {
    if confirm.0.is_some() {
        return;
    }
    for (interaction, again, cont) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if again.is_some() {
            if save.0 {
                confirm.0 = Some(false); // would overwrite — confirm first
            } else {
                pending.0 = None; // fresh run
                next_app.set(AppState::Playing);
            }
        }
        if cont.is_some() {
            pending.0 = crate::savegame::load_save();
            next_app.set(AppState::Playing);
        }
    }
}

// ── Overwrite-confirm dialog ────────────────────────────────────────────────────────────

/// Spawn / despawn the confirm overlay to match [`ConfirmWipe`]. Runs every frame (ungated) but
/// acts only on a mismatch, so it cleans the overlay up even across the state transition that
/// confirming triggers.
fn sync_confirm_overlay(
    mut commands: Commands,
    confirm: Res<ConfirmWipe>,
    fonts: Res<UiFonts>,
    existing: Query<Entity, With<ConfirmUi>>,
) {
    let want = confirm.0.is_some();
    let have = !existing.is_empty();
    if want && !have {
        spawn_confirm(&mut commands, &fonts);
    } else if !want && have {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

/// The "Overwrite saved game?" card — a danger **OVERWRITE** + a **CANCEL**, over a click-blocking
/// scrim above every other screen (z 100).
fn spawn_confirm(commands: &mut Commands, fonts: &UiFonts) {
    commands.spawn((modal_root(100), ConfirmUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(14.0),
                padding: UiRect::axes(Val::Px(40.0), Val::Px(28.0)),
                border: widgets::border(1.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            anim(AnimKind::PopIn, 0.0, 0.26),
        ))
        .with_children(|c| {
            c.spawn(label(&fonts.display, "OVERWRITE SAVED GAME?", 22.0, TEXT));
            c.spawn(label(
                &fonts.regular,
                "This deletes your current run. It can't be undone.",
                14.0,
                TEXT_DIM,
            ));
            c.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(12.0),
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(28.0), Val::Px(11.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_BTN),
                        ..default()
                    },
                    widgets::btn_danger_paint(),
                    ConfirmOkBtn,
                    anim_btn(AnimKind::PopIn, 0.05, 0.26),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.extrabold, "OVERWRITE", 16.0, Color::WHITE));
                });
                row.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(28.0), Val::Px(11.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_BTN),
                        ..default()
                    },
                    widgets::btn_primary_paint(),
                    ConfirmCancelBtn,
                    anim_btn(AnimKind::PopIn, 0.05, 0.26),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.extrabold, "CANCEL", 16.0, INK));
                });
            });
            c.spawn((
                label(&fonts.regular, "Enter to overwrite · Esc to cancel", 12.0, GREY),
                Node { margin: UiRect::top(Val::Px(2.0)), ..default() },
            ));
        });
    });
}

/// Resolve the confirm dialog from its buttons or the keyboard (Enter/Space = overwrite,
/// Esc = cancel). On overwrite: delete the save, mark it gone, and start a fresh run — replaying
/// what the originating button would have done (`from_pause` ⇒ fire the `Paused → Playing` resets
/// via [`RestartRequested`]). Ungated; no-ops while the dialog is closed.
#[allow(clippy::type_complexity)]
fn confirm_input(
    keys: Res<ButtonInput<KeyCode>>,
    q: Query<
        (&Interaction, Option<&ConfirmOkBtn>, Option<&ConfirmCancelBtn>),
        Changed<Interaction>,
    >,
    mut confirm: ResMut<ConfirmWipe>,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    mut save: ResMut<crate::savegame::SaveExists>,
    mut restart: ResMut<RestartRequested>,
    mut was_open: Local<bool>,
) {
    // Swallow input on the frame the dialog opens, so the very Enter/Space that opened it (from
    // New Game) doesn't also confirm it. (System order vs. the openers is otherwise undefined.)
    let opened_this_frame = confirm.0.is_some() && !*was_open;
    *was_open = confirm.0.is_some();

    let Some(from_pause) = confirm.0 else { return };
    if opened_this_frame {
        return;
    }

    let mut ok = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
    let mut cancel = keys.just_pressed(KeyCode::Escape);
    for (interaction, ok_b, cancel_b) in &q {
        if *interaction == Interaction::Pressed {
            ok |= ok_b.is_some();
            cancel |= cancel_b.is_some();
        }
    }

    if cancel {
        confirm.0 = None; // overlay despawns on the next sync
        return;
    }
    if ok {
        crate::savegame::delete_save();
        save.0 = false;
        pending.0 = None; // a fresh run, never a load
        if from_pause {
            restart.0 = true; // fire the Paused → Playing fresh-run resets
        }
        next_app.set(AppState::Playing);
        confirm.0 = None;
    }
}

fn despawn_screen<T: Component>(mut commands: Commands, q: Query<Entity, With<T>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn booted(state: AppState) -> App {
        let mut app = App::new();
        // StatesPlugin provides the StateTransition schedule (DefaultPlugins bundles it).
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_state(state)
            .add_sub_state::<Modal>();
        app.update(); // run the initial StateTransition
        app
    }

    #[test]
    fn modal_substate_gates_only_while_playing() {
        // Playing ⇒ the Modal sub-state exists and defaults to None ⇒ the freeze gate is OPEN.
        let playing = booted(AppState::Playing);
        assert_eq!(playing.world().resource::<State<Modal>>().get(), &Modal::None);
        // Not playing ⇒ no Modal resource ⇒ `in_state(Modal::None)` is false ⇒ world frozen.
        let menu = booted(AppState::StartScreen);
        assert!(menu.world().get_resource::<State<Modal>>().is_none());
    }
}
