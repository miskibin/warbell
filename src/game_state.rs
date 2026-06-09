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

use bevy::prelude::*;

use crate::ui::anim::{anim, anim_btn, AnimKind};
use crate::ui::fonts::{label, UiFonts};
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

pub struct GameStatePlugin;

impl Plugin for GameStatePlugin {
    fn build(&self, app: &mut App) {
        // Screenshot/demo hooks want a live world without the menu in the way.
        let boot = if skip_menu() { AppState::Playing } else { AppState::StartScreen };
        app.insert_state(boot)
            .add_sub_state::<Modal>()
            .add_systems(Update, (pause_toggle, watch_end))
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
) {
    if !keys.just_pressed(KeyCode::Escape) {
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

fn start_screen_input(keys: Res<ButtonInput<KeyCode>>, mut next_app: ResMut<NextState<AppState>>) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        next_app.set(AppState::Playing);
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

fn gameover_input(keys: Res<ButtonInput<KeyCode>>, mut next_app: ResMut<NextState<AppState>>) {
    // Enter restarts: the per-plugin OnExit(GameOver) resets (siege/keep/hero) rebuild a fresh run.
    if keys.just_pressed(KeyCode::Enter) {
        next_app.set(AppState::Playing);
    }
}

// ── Screens (cinematic start / pause / game-over), ported from the 3js HUD ──────────────

#[derive(Component)]
struct StartScreenUi;
#[derive(Component)]
struct PausedUi;
#[derive(Component)]
struct GameOverUi;
/// The "Play" button on the start screen.
#[derive(Component)]
struct StartPlayButton;
/// A difficulty segment (click to select).
#[derive(Component)]
struct SegButton(crate::siege::Difficulty);
/// The "Play again" button on the game-over screen.
#[derive(Component)]
struct AgainButton;

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
) {
    let cur = siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);

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
                m.spawn((label(&fonts.bold, "DEFEND THE KEEP", 12.0, KICKER), anim(AnimKind::Rise, 0.06, 0.6)));
                // Two-line title.
                m.spawn((
                    Node { flex_direction: FlexDirection::Column, ..default() },
                    anim(AnimKind::Rise, 0.12, 0.7),
                ))
                .with_children(|t| {
                    for line in ["TILE", "WORLD"] {
                        t.spawn((
                            label(&fonts.extrabold, line, 84.0, rgb(238, 244, 255)),
                            TextShadow { offset: Vec2::new(0.0, 6.0), color: rgba(0, 0, 0, 0.6) },
                        ));
                    }
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
                // Play button.
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
                    b.spawn(label(&fonts.extrabold, "PLAY", 19.0, Color::WHITE));
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
                        BackgroundColor(rgba(14, 20, 34, 0.72)),
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
                                b.spawn(label(&fonts.semibold, diff_name(d), 13.0, if on { Color::WHITE } else { TEXT_FAINT }));
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

fn spawn_pause_screen(mut commands: Commands, fonts: Res<UiFonts>) {
    commands.spawn((modal_root(50), PausedUi)).with_children(|root| {
        root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                padding: UiRect::axes(Val::Px(40.0), Val::Px(28.0)),
                border: widgets::border(1.0),
                border_radius: radius(R_PANEL),
                ..default()
            },
            widgets::card_paint(),
            anim(AnimKind::PopIn, 0.0, 0.26),
        ))
        .with_children(|c| {
            c.spawn(label(&fonts.extrabold, "PAUSED", 40.0, TEXT));
            c.spawn(label(&fonts.regular, "Esc to resume", 16.0, GREY));
        });
    });
}

fn spawn_gameover_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    siege: Option<Res<crate::siege::Siege>>,
    player: Option<Res<crate::player::PlayerRes>>,
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

    commands.spawn((modal_root(50), GameOverUi)).with_children(|root| {
        root.spawn((
            label(&fonts.extrabold, title, 60.0, col),
            TextShadow { offset: Vec2::new(0.0, 4.0), color: rgba(0, 0, 0, 0.8) },
            anim(AnimKind::PopIn, 0.0, 0.6),
        ));
        if !stats.is_empty() {
            root.spawn((label(&fonts.semibold, stats, 16.0, GOLD), anim(AnimKind::FloatUp, 0.2, 0.5)));
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
            b.spawn(label(&fonts.extrabold, "PLAY AGAIN", 15.0, Color::WHITE));
        });
    });
}

/// Click "Play" / a difficulty segment on the start screen.
#[allow(clippy::type_complexity)]
fn start_click(
    q: Query<(&Interaction, Option<&StartPlayButton>, Option<&SegButton>), Changed<Interaction>>,
    mut next_app: ResMut<NextState<AppState>>,
    siege: Option<ResMut<crate::siege::Siege>>,
) {
    let mut siege = siege;
    for (interaction, play, seg) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if play.is_some() {
            next_app.set(AppState::Playing);
        }
        if let Some(seg) = seg {
            if let Some(s) = siege.as_deref_mut() {
                s.difficulty = seg.0;
            }
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

/// Click "Play again" on the game-over screen.
fn gameover_click(
    q: Query<&Interaction, (Changed<Interaction>, With<AgainButton>)>,
    mut next_app: ResMut<NextState<AppState>>,
) {
    for interaction in &q {
        if *interaction == Interaction::Pressed {
            next_app.set(AppState::Playing);
        }
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
