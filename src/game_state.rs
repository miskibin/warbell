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
                (start_screen_input, cycle_difficulty).run_if(in_state(AppState::StartScreen)),
            )
            .add_systems(Update, gameover_input.run_if(in_state(AppState::GameOver)));
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
/// with the dev `1-5` biome keys), updating the on-screen label.
fn cycle_difficulty(
    keys: Res<ButtonInput<KeyCode>>,
    siege: Option<ResMut<crate::siege::Siege>>,
    mut q: Query<&mut Text, With<DifficultyText>>,
) {
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
    if let Ok(mut t) = q.single_mut() {
        **t = difficulty_line(siege.difficulty);
    }
}

fn difficulty_line(d: crate::siege::Difficulty) -> String {
    use crate::siege::Difficulty::*;
    let name = match d {
        Easy => "Easy",
        Normal => "Normal",
        Hard => "Hard",
    };
    format!("Difficulty: {name}   (G to change)")
}

fn gameover_input(keys: Res<ButtonInput<KeyCode>>, mut next_app: ResMut<NextState<AppState>>) {
    // Enter restarts: the per-plugin OnExit(GameOver) resets (siege/keep/hero) rebuild a fresh run.
    if keys.just_pressed(KeyCode::Enter) {
        next_app.set(AppState::Playing);
    }
}

// ── Minimal overlays (P0.6 adds difficulty chooser + styling) ──────────────────────────

#[derive(Component)]
struct StartScreenUi;
#[derive(Component)]
struct DifficultyText;
#[derive(Component)]
struct PausedUi;
#[derive(Component)]
struct GameOverUi;

fn overlay_root(dim: f32) -> impl Bundle {
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
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, dim)),
        GlobalZIndex(50),
    )
}

fn title_text(s: &str, size: f32) -> impl Bundle {
    (Text::new(s), TextFont { font_size: size, ..default() }, TextColor(Color::WHITE))
}

fn spawn_start_screen(mut commands: Commands, siege: Option<Res<crate::siege::Siege>>) {
    let diff = siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);
    commands.spawn((overlay_root(0.55), StartScreenUi)).with_children(|p| {
        p.spawn(title_text("TILEWORLD", 72.0));
        p.spawn(title_text("Press Enter to begin", 28.0));
        p.spawn((
            Text::new(difficulty_line(diff)),
            TextFont { font_size: 22.0, ..default() },
            TextColor(Color::srgb(0.8, 0.85, 0.95)),
            DifficultyText,
        ));
    });
}

fn spawn_pause_screen(mut commands: Commands) {
    commands.spawn((overlay_root(0.5), PausedUi)).with_children(|p| {
        p.spawn(title_text("Paused", 56.0));
        p.spawn(title_text("Esc to resume", 24.0));
    });
}

fn spawn_gameover_screen(mut commands: Commands, siege: Option<Res<crate::siege::Siege>>) {
    let won = matches!(siege.map(|s| s.phase), Some(crate::siege::GamePhase::Victory));
    let (title, col) = if won {
        ("Victory", Color::srgb(0.45, 0.9, 0.45))
    } else {
        ("The keep has fallen", Color::srgb(0.9, 0.4, 0.36))
    };
    commands.spawn((overlay_root(0.6), GameOverUi)).with_children(|p| {
        p.spawn((Text::new(title), TextFont { font_size: 60.0, ..default() }, TextColor(col)));
        p.spawn(title_text("Press Enter to play again", 24.0));
    });
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
