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
use bevy::window::{PrimaryWindow, WindowMode, WindowPosition};

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
    /// The quest explainer card (freeze gate, like `Tutorial`; opened by **J** or the tracker).
    Quest,
    /// "Warden slain" reward dialog (freezes the world; opened by `boss::reward_on_death`).
    BossReward,
}

/// Env var the relaunched child reads at startup to know what to boot into (see
/// [`apply_boot_intent`]): `"fresh:<difficulty>"` for a clean run. (A *Continue* no longer
/// relaunches — it resets in-process — so there is no load payload here anymore.)
const ENV_RESTART: &str = "WARBELL_RESTART";

/// What a mid-session "(re)enter a run" action wants the relaunched process to boot into.
/// Only **fresh runs** relaunch — a *Continue* now resets in-process (see [`begin_continue`]),
/// so there is no `Continue` variant here anymore.
#[derive(Clone, Copy)]
pub enum BootIntent {
    /// A clean run at this difficulty (the **full reset** — the persistent world is rebuilt fresh
    /// by the child's normal startup, so chopped trees / opened chests / built houses all return).
    Fresh(crate::siege::Difficulty),
}

/// Set by the fresh-run buttons (New Game / Play Again / pause Restart) to request a **true process
/// relaunch** — the only way to get an exactly-as-cold-launch reset, since the island is built once
/// at `Startup` and is otherwise persistent. [`do_process_restart`] consumes it: spawn
/// `current_exe` carrying the intent in [`ENV_RESTART`] (plus the live window geometry so the child
/// reopens *in place*), then exit. Start-screen entries skip this (the world is already fresh
/// moments after launch) and start in-process; *Continue* skips it too (resets in-process).
#[derive(Resource, Default)]
pub struct RestartProcess(pub Option<BootIntent>);

/// Env var carrying the parent window's geometry across a relaunch (`"x,y,w,h,fullscreen"`), so the
/// child reopens at the same place/size/mode instead of an OS-default spot — the relaunch then
/// reads as an in-place reload, not a stray new window. `x`/`y` are `-99999` when the position
/// isn't yet resolved. Parsed in `main.rs` before the window is created.
const ENV_WINGEO: &str = "WARBELL_WINGEO";

/// Flags an **in-process Continue** (resume last night / load last save) so [`clear_battlefield`]
/// sweeps the dead run's transient entities (invaders, marchers, bolts, corpses) on the next
/// `Playing` frame. The resource restore + hero revival ride existing wiring (`apply_pending_load`
/// + `OnExit(GameOver)` `reset_player`); this only triggers the entity sweep.
#[derive(Resource, Default)]
pub struct ContinueInPlace(pub bool);

/// The "Overwrite saved game?" confirm dialog. `Some(_)` = open (the bool, once "from pause", is now
/// vestigial — [`confirm_input`] decides in-process vs. relaunch from the live `AppState`). `None` =
/// closed. Every fresh-run button routes through this whenever a save exists, so a misclick can't
/// wipe a long run.
#[derive(Resource, Default)]
pub struct ConfirmWipe(pub Option<bool>);

/// True once a run is live (entered `Playing`) and not yet ended, so the start screen — reachable
/// mid-run via the pause/game-over **Main Menu** button — knows to offer **RESUME** (drop back into
/// the frozen run) and, crucially, to route **New Game** through a full process relaunch rather than
/// the cold-boot in-process start (which would resume a *dirty* world). Set true `OnEnter(Playing)`,
/// false `OnEnter(GameOver)`.
#[derive(Resource, Default)]
pub struct RunInProgress(pub bool);

pub struct GameStatePlugin;

impl Plugin for GameStatePlugin {
    fn build(&self, app: &mut App) {
        // Screenshot/demo hooks want a live world without the menu in the way.
        let boot = if skip_menu() { AppState::Playing } else { AppState::StartScreen };
        app.insert_state(boot)
            .add_sub_state::<Modal>()
            .init_resource::<RestartProcess>()
            .init_resource::<ConfirmWipe>()
            .init_resource::<ContinueInPlace>()
            .init_resource::<RunInProgress>()
            // Honor a relaunch intent from the parent process (fresh run), then drive any pending
            // relaunch to completion. Both ungated (work over every screen).
            .add_systems(Startup, apply_boot_intent)
            .add_systems(Update, do_process_restart)
            // Sweep the dead run's battlefield the frame an in-process Continue lands in Playing.
            .add_systems(Update, clear_battlefield.run_if(in_state(AppState::Playing)))
            // Overwrite-confirm dialog: reconcile its overlay + resolve its input. Ungated so it
            // works over the start / game-over / pause screens alike.
            .add_systems(Update, (sync_confirm_overlay, confirm_input))
            .add_systems(Update, (pause_toggle, watch_end))
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
            // Track whether a run is live, so the (now mid-run-reachable) start screen offers
            // RESUME and routes New Game correctly. See [`RunInProgress`].
            .add_systems(OnEnter(AppState::Playing), mark_run_active)
            .add_systems(OnEnter(AppState::GameOver), clear_run_active)
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

// ── Process relaunch (the "full reset", carried across a fresh process) ──────────────────

/// Decode a difficulty token written by [`diff_name`] (the inverse used on the relaunch handoff).
fn parse_diff(s: &str) -> crate::siege::Difficulty {
    use crate::siege::Difficulty::*;
    match s {
        "Easy" => Easy,
        "Hard" => Hard,
        _ => Normal,
    }
}

/// Child-side: if the parent relaunched us with an intent in [`ENV_RESTART`], skip the menu and
/// boot straight into the requested run. We DON'T short-circuit the StartScreen→Playing transition
/// — letting it fire normally runs every `OnExit(StartScreen)` fresh-run reset (so the difficulty
/// handicaps apply), and `apply_pending_load` (Update) overwrites them afterwards for a Continue.
fn apply_boot_intent(
    mut siege: ResMut<crate::siege::Siege>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    mut next_app: ResMut<NextState<AppState>>,
) {
    let Ok(intent) = std::env::var(ENV_RESTART) else { return };
    // Only fresh runs relaunch now (Continue resets in-process), so the sole payload is `fresh:…`.
    let Some(diff) = intent.strip_prefix("fresh:") else { return };
    siege.difficulty = parse_diff(diff);
    pending.0 = None; // a clean run, never a load
    next_app.set(AppState::Playing);
}

/// Parent-side: when a fresh-run relaunch is requested, spawn a fresh copy of ourselves carrying
/// the boot intent (+ this window's geometry so the child reopens in place), then exit. The new
/// process rebuilds the entire world from scratch — the one reliable "exactly as if the app were
/// started again" reset.
///
/// Takes the intent (`req.0.take()`) so this fires **exactly once**: `AppExit` may not tear the
/// process down for a frame or two (GPU/window teardown latency on Windows), and re-running with
/// the intent still set would spawn a *second* child → a stray extra window. Draining it first
/// makes every later frame a no-op.
fn do_process_restart(
    mut req: ResMut<RestartProcess>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(intent) = req.0.take() else { return };
    let payload = match intent {
        BootIntent::Fresh(d) => format!("fresh:{}", diff_name(d)),
    };
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            error!("full reset: current_exe() failed, cannot relaunch: {e}");
            return;
        }
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.env(ENV_RESTART, payload);
    if let Ok(w) = windows.single() {
        cmd.env(ENV_WINGEO, window_geometry(w));
    }
    match cmd.spawn() {
        Ok(_) => {
            exit.write(AppExit::Success); // child is up; tear this process down
        }
        Err(e) => error!("full reset: relaunch failed: {e}"),
    }
}

/// Encode a window's geometry as `"x,y,w,h,fullscreen"` for the relaunch handoff ([`ENV_WINGEO`]).
/// Position is `-99999,-99999` when winit hasn't resolved an explicit one yet (child then centres).
fn window_geometry(w: &Window) -> String {
    let (x, y) = match w.position {
        WindowPosition::At(p) => (p.x, p.y),
        _ => (-99999, -99999),
    };
    let fs = u8::from(!matches!(w.mode, WindowMode::Windowed));
    format!("{x},{y},{},{},{fs}", w.resolution.physical_width(), w.resolution.physical_height())
}

/// Shared by every **Continue / Load last save** entry point (game-over, the C key, pause→Load):
/// load the save into [`PendingLoad`], flag the [`clear_battlefield`] sweep, and switch to
/// `Playing` — all in-process, so no new window spawns. A missing/unreadable save is a no-op (the
/// Continue button is only shown when a save exists, but this guards the file-vanished race).
fn begin_continue(
    pending: &mut crate::savegame::PendingLoad,
    next_app: &mut NextState<AppState>,
    cont: &mut ContinueInPlace,
) {
    let Some(data) = crate::savegame::load_save() else { return };
    pending.0 = Some(data);
    cont.0 = true;
    next_app.set(AppState::Playing);
}

/// On an in-process Continue, despawn the dead run's transient combat entities so dawn reads clean:
/// night-wave invaders, cinematic marchers, shaman bolts in flight, and fading corpses. Geometry
/// (chopped trees / opened chests / built houses) and camp orks are deliberately left as-is. The
/// hero is revived by `OnExit(GameOver)`'s `reset_player`; run-state is restored by
/// `apply_pending_load`; the sky eases back to dawn on its own once the siege phase is `Prep`.
fn clear_battlefield(
    mut cont: ResMut<ContinueInPlace>,
    mut commands: Commands,
    transient: Query<
        Entity,
        Or<(
            With<crate::orks::WaveInvader>,
            With<crate::cinematic::DirectorMarcher>,
            With<crate::projectile::Bolt>,
            With<crate::dying::Dying>,
        )>,
    >,
) {
    if !cont.0 {
        return;
    }
    cont.0 = false;
    for e in &transient {
        commands.entity(e).try_despawn();
    }
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

/// A run just went live — remember it so the start screen (reachable mid-run) offers RESUME and
/// routes New Game through a relaunch instead of a dirty in-process start.
fn mark_run_active(mut run: ResMut<RunInProgress>) {
    run.0 = true;
}

/// The run ended (Victory/Defeat) — the start screen reached from game-over has nothing live to
/// RESUME, and New Game from there is a fresh relaunch as before.
fn clear_run_active(mut run: ResMut<RunInProgress>) {
    run.0 = false;
}

/// Start a fresh run from the start screen, picking the *correct* reset path: at cold boot the
/// world is already fresh, so go in-process; if a run is already live (the menu was reached
/// mid-session), only a full process relaunch truly rebuilds the world (the in-process path would
/// resume the dirty world). Mirrors the routing in `confirm_input`.
fn begin_new_game(
    run_active: bool,
    cur_diff: crate::siege::Difficulty,
    pending: &mut crate::savegame::PendingLoad,
    next_app: &mut NextState<AppState>,
    restart_proc: &mut RestartProcess,
) {
    if run_active {
        restart_proc.0 = Some(BootIntent::Fresh(cur_diff)); // full reset via relaunch
    } else {
        pending.0 = None; // cold boot: world already fresh, start in-process
        next_app.set(AppState::Playing);
    }
}

fn start_screen_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    run: Res<RunInProgress>,
    siege: Option<Res<crate::siege::Siege>>,
    mut restart_proc: ResMut<RestartProcess>,
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
            let cur_diff = current_difficulty(siege.as_deref());
            begin_new_game(run.0, cur_diff, &mut pending, &mut next_app, &mut restart_proc);
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

/// The live siege difficulty, or `Normal` when no siege resource exists yet (start screen /
/// between runs). Centralises the missing-resource fallback used to seed `BootIntent::Fresh`.
/// Call with `siege.as_deref()` so the `Option<Res<Siege>>` isn't consumed.
fn current_difficulty(siege: Option<&crate::siege::Siege>) -> crate::siege::Difficulty {
    siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal)
}

fn gameover_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut restart_proc: ResMut<RestartProcess>,
    siege: Option<Res<crate::siege::Siege>>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    mut next_app: ResMut<NextState<AppState>>,
    mut cont: ResMut<ContinueInPlace>,
) {
    if confirm.0.is_some() {
        return; // dialog owns the keyboard
    }
    let cur_diff = current_difficulty(siege.as_deref());
    let defeat = !matches!(siege.as_deref().map(|s| s.phase), Some(crate::siege::GamePhase::Victory));
    // C resumes last night in-process (defeat + save only); Enter starts a fresh run (confirming
    // first if it'd overwrite), which relaunches the process for a clean world.
    if defeat && save.0 && keys.just_pressed(KeyCode::KeyC) {
        begin_continue(&mut pending, &mut next_app, &mut cont);
    } else if keys.just_pressed(KeyCode::Enter) {
        if save.0 {
            confirm.0 = Some(false);
        } else {
            restart_proc.0 = Some(BootIntent::Fresh(cur_diff));
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
/// The "Resume" button on the start screen — drops back into the live frozen run. Only spawned
/// when [`RunInProgress`] (i.e. the menu was reached mid-run via a Main Menu button).
#[derive(Component)]
struct StartResumeButton;
/// The "Credits" button on the start screen — opens the credits overlay ([`mainmenu::CreditsOpen`]).
#[derive(Component)]
struct CreditsButton;
/// The "Main Menu" button on the pause screen — returns to the start screen, run kept live.
#[derive(Component)]
struct PauseMenuBtn;
/// The "Main Menu" button on the game-over screen — returns to the start screen.
#[derive(Component)]
struct GameOverMenuBtn;
/// A difficulty segment (click to select).
#[derive(Component)]
struct SegButton(crate::siege::Difficulty);
// ── Pause-menu buttons ──
#[derive(Component)]
struct PauseResumeBtn;
#[derive(Component)]
struct PauseSaveBtn;
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
    run: Res<RunInProgress>,
) {
    let cur = current_difficulty(siege.as_deref());
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
                // Resume — only when a run is live (menu reached mid-game via Main Menu). The
                // primary action then: drops straight back into the frozen run, world intact.
                if run.0 {
                    m.spawn((
                        Node {
                            padding: UiRect::axes(Val::Px(44.0), Val::Px(13.0)),
                            border: widgets::border(1.0),
                            border_radius: radius(11.0),
                            ..default()
                        },
                        widgets::btn_primary_paint(),
                        StartResumeButton,
                        anim_btn(AnimKind::Rise, 0.32, 0.7),
                    ))
                    .with_children(|b| {
                        b.spawn(label(&fonts.extrabold, "RESUME", 19.0, INK));
                    });
                }
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
                // How to Play — secondary, but right in the menu column so new players find the
                // guide (and the Stronghold/RTS explanation) before their first night.
                m.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(24.0), Val::Px(9.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_BTN),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    Button,
                    Interaction::default(),
                    BackgroundColor(BTN_BG),
                    BorderColor::all(GOLD_HAIRLINE),
                    crate::ui::anim::Hoverable {
                        rest_bg: BTN_BG,
                        hover_bg: BTN_BG_HOVER,
                        rest_border: GOLD_HAIRLINE,
                        hover_border: GOLD_NOTCH,
                        lift: 2.0,
                    },
                    UiTransform::IDENTITY,
                    crate::tutorial::StartHelpButton,
                    anim_btn(AnimKind::Rise, 0.38, 0.7),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.bold, "HOW TO PLAY", 14.0, GOLD));
                    b.spawn(label(&fonts.semibold, "H", 11.0, KICKER));
                });
                // Credits — secondary, opens the credits overlay (mainmenu::CreditsOpen).
                m.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(24.0), Val::Px(9.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_BTN),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    Button,
                    Interaction::default(),
                    BackgroundColor(BTN_BG),
                    BorderColor::all(GOLD_HAIRLINE),
                    crate::ui::anim::Hoverable {
                        rest_bg: BTN_BG,
                        hover_bg: BTN_BG_HOVER,
                        rest_border: GOLD_HAIRLINE,
                        hover_border: GOLD_NOTCH,
                        lift: 2.0,
                    },
                    UiTransform::IDENTITY,
                    CreditsButton,
                    anim_btn(AnimKind::Rise, 0.4, 0.7),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.bold, "CREDITS", 14.0, GOLD));
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
    siege: Res<crate::siege::Siege>,
    audio: Res<AudioSettings>,
    quality: Res<GraphicsQuality>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let has_save = save.0;
    // Manual save is a day-only action (a mid-siege snapshot would resume in the wrong place).
    let can_save = matches!(siege.phase, crate::siege::GamePhase::Prep);
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
            // Save the current run on demand. Disabled (dim) during a night assault.
            if can_save {
                pause_btn(c, &fonts.extrabold, "SAVE GAME", PauseSaveBtn, (), 0.11);
            } else {
                c.spawn((
                    Node {
                        width: Val::Px(264.0),
                        padding: UiRect::axes(Val::Px(18.0), Val::Px(10.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: widgets::border(1.0),
                        border_radius: radius(R_BTN),
                        ..default()
                    },
                    BackgroundColor(rgba(196, 144, 62, 0.16)),
                    BorderColor::all(rgba(244, 204, 132, 0.22)),
                    anim(AnimKind::PopIn, 0.11, 0.28),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.bold, "SAVE GAME  (by day only)", 14.0, GREY));
                });
            }
            if has_save {
                pause_btn(c, &fonts.extrabold, "LOAD LAST SAVE", PauseLoadBtn, (), 0.12);
            }
            pause_btn(c, &fonts.extrabold, "RESTART", PauseRestartBtn, (), 0.14);
            // Back to the title screen — the run stays frozen in memory, so RESUME there returns
            // to exactly this point.
            pause_btn(c, &fonts.extrabold, "MAIN MENU", PauseMenuBtn, (), 0.16);

            c.spawn((
                label(&fonts.regular, "Esc to resume", 13.0, GREY),
                Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
            ));
        });
    });
}

/// Handle the pause-menu buttons: resume, the three in-place settings toggles, and the two run
/// actions. **Load last save** resumes in-process (no new window); **Restart** requests a full
/// process relaunch via [`RestartProcess`] (confirming first if it'd overwrite a save) for a clean
/// world.
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
            Option<&PauseSaveBtn>,
            Option<&PauseMenuBtn>,
        ),
        Changed<Interaction>,
    >,
    mut next_app: ResMut<NextState<AppState>>,
    mut restart_proc: ResMut<RestartProcess>,
    siege: Option<Res<crate::siege::Siege>>,
    mut audio: ResMut<AudioSettings>,
    mut quality: ResMut<GraphicsQuality>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    mut cont_req: ResMut<ContinueInPlace>,
    mut save_req: MessageWriter<crate::savegame::RequestSave>,
) {
    if confirm.0.is_some() {
        return; // dialog owns input
    }
    let now = time.elapsed_secs_f64();
    let cur_diff = current_difficulty(siege.as_deref());
    for (interaction, resume, load, restart_b, audio_b, gfx_b, fs_b, save_b, menu_b) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if resume.is_some() {
            next_app.set(AppState::Playing);
        }
        if menu_b.is_some() {
            next_app.set(AppState::StartScreen); // run kept live; RESUME on the title returns here
        }
        if save_b.is_some() {
            // `manual_save` (savegame.rs) does the write + the "Game saved" notice.
            save_req.write(crate::savegame::RequestSave);
        }
        if load.is_some() {
            // Resume the save in-process: the battlefield sweep clears the live wave and
            // `apply_pending_load` rolls run-state back to the saved dawn (geometry persists).
            begin_continue(&mut pending, &mut next_app, &mut cont_req);
        }
        if restart_b.is_some() {
            if save.0 {
                confirm.0 = Some(true); // Restart wipes the save too — confirm (from_pause = true)
            } else {
                restart_proc.0 = Some(BootIntent::Fresh(cur_diff)); // full reset via relaunch
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
        // Back to the title screen (secondary).
        root.spawn((
            Node {
                padding: UiRect::axes(Val::Px(22.0), Val::Px(9.0)),
                border: widgets::border(1.0),
                border_radius: radius(R_BTN),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
            Button,
            Interaction::default(),
            BackgroundColor(BTN_BG),
            BorderColor::all(GOLD_HAIRLINE),
            crate::ui::anim::Hoverable {
                rest_bg: BTN_BG,
                hover_bg: BTN_BG_HOVER,
                rest_border: GOLD_HAIRLINE,
                hover_border: GOLD_NOTCH,
                lift: 2.0,
            },
            UiTransform::IDENTITY,
            GameOverMenuBtn,
            anim_btn(AnimKind::FloatUp, 0.5, 0.5),
        ))
        .with_children(|b| {
            b.spawn(label(&fonts.bold, "MAIN MENU", 13.0, GOLD));
        });
    });
}

/// Click "Resume" / "Continue" / "New Game" / "Credits" / a difficulty segment on the start screen.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn start_click(
    q: Query<
        (
            &Interaction,
            Option<&StartPlayButton>,
            Option<&StartContinueButton>,
            Option<&StartResumeButton>,
            Option<&CreditsButton>,
            Option<&SegButton>,
        ),
        Changed<Interaction>,
    >,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    siege: Option<ResMut<crate::siege::Siege>>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    run: Res<RunInProgress>,
    mut restart_proc: ResMut<RestartProcess>,
    mut credits: ResMut<crate::mainmenu::CreditsOpen>,
) {
    if confirm.0.is_some() {
        return; // dialog up — the scrim already blocks these, but be explicit
    }
    let mut siege = siege;
    let cur_diff = current_difficulty(siege.as_deref());
    for (interaction, play, cont, resume, cred, seg) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if play.is_some() {
            if save.0 {
                confirm.0 = Some(false); // New Game would overwrite — confirm first
            } else {
                begin_new_game(run.0, cur_diff, &mut pending, &mut next_app, &mut restart_proc);
            }
        }
        if resume.is_some() {
            next_app.set(AppState::Playing); // back into the live frozen run, world intact
        }
        if cont.is_some() && save.0 {
            pending.0 = crate::savegame::load_save();
            next_app.set(AppState::Playing);
        }
        if cred.is_some() {
            credits.0 = true;
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
    let cur = current_difficulty(siege.as_deref());
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
        (
            &Interaction,
            Option<&AgainButton>,
            Option<&GameOverContinueButton>,
            Option<&GameOverMenuBtn>,
        ),
        Changed<Interaction>,
    >,
    mut restart_proc: ResMut<RestartProcess>,
    siege: Option<Res<crate::siege::Siege>>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    mut next_app: ResMut<NextState<AppState>>,
    mut cont_req: ResMut<ContinueInPlace>,
) {
    if confirm.0.is_some() {
        return;
    }
    let cur_diff = current_difficulty(siege.as_deref());
    for (interaction, again, cont, menu_b) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if menu_b.is_some() {
            next_app.set(AppState::StartScreen); // run already ended; title offers New/Continue
        }
        if again.is_some() {
            if save.0 {
                confirm.0 = Some(false); // would overwrite — confirm first
            } else {
                restart_proc.0 = Some(BootIntent::Fresh(cur_diff)); // full reset via relaunch
            }
        }
        if cont.is_some() {
            begin_continue(&mut pending, &mut next_app, &mut cont_req); // resume last night in-process
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
/// Esc = cancel). On overwrite: delete the save, mark it gone, and start a fresh run. From the
/// start screen that's in-process (world is already fresh); mid-session it's a full process
/// relaunch (see [`RestartProcess`]). Ungated; no-ops while the dialog is closed.
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
    mut restart_proc: ResMut<RestartProcess>,
    app: Res<State<AppState>>,
    siege: Option<Res<crate::siege::Siege>>,
    run: Res<RunInProgress>,
    mut was_open: Local<bool>,
) {
    // Swallow input on the frame the dialog opens, so the very Enter/Space that opened it (from
    // New Game) doesn't also confirm it. (System order vs. the openers is otherwise undefined.)
    let opened_this_frame = confirm.0.is_some() && !*was_open;
    *was_open = confirm.0.is_some();

    if confirm.0.is_none() {
        return;
    }
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
        confirm.0 = None;
        if *app.get() == AppState::StartScreen && !run.0 {
            // Cold-boot start screen — the world is already fresh, so start in-process (relaunching
            // here would only reload identical assets). OnExit(StartScreen) runs the fresh-run resets.
            pending.0 = None;
            next_app.set(AppState::Playing);
        } else {
            // Mid-session New Game / Restart (incl. a start screen reached mid-run) → full reset via
            // a clean process relaunch, since the in-process world is dirty.
            let cur_diff = current_difficulty(siege.as_deref());
            restart_proc.0 = Some(BootIntent::Fresh(cur_diff));
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
    use bevy::window::WindowResolution;

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

    /// The relaunch geometry handoff encodes position / physical size / fullscreen, and degrades to
    /// the `-99999` sentinel when winit hasn't resolved a position yet.
    #[test]
    fn window_geometry_encodes_pos_size_and_mode() {
        let mut w = Window::default();
        w.position = WindowPosition::At(IVec2::new(100, 50));
        w.resolution = WindowResolution::new(1280, 720);
        w.mode = WindowMode::Windowed;
        assert_eq!(window_geometry(&w), "100,50,1280,720,0");

        w.mode = WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current);
        assert_eq!(window_geometry(&w), "100,50,1280,720,1", "fullscreen flag set");

        w.position = WindowPosition::Automatic;
        assert_eq!(window_geometry(&w), "-99999,-99999,1280,720,1", "unresolved position sentinel");
    }

    /// New Game routing: from a cold-boot start screen (`run_active = false`) the world is already
    /// fresh, so we start **in-process** (queue `Playing`, no relaunch). Once a run is live
    /// (`run_active = true`, e.g. the menu was reached mid-session), only a full process relaunch
    /// truly rebuilds the world — so `begin_new_game` must request one instead.
    #[test]
    fn new_game_routes_in_process_cold_and_relaunch_midrun() {
        use crate::siege::Difficulty;

        // Cold boot → in-process: a Playing transition is queued, no relaunch requested.
        let mut pending = crate::savegame::PendingLoad::default();
        let mut next = NextState::<AppState>::default();
        let mut restart = RestartProcess::default();
        begin_new_game(false, Difficulty::Normal, &mut pending, &mut next, &mut restart);
        assert!(restart.0.is_none(), "cold boot must not relaunch");
        assert!(matches!(next, NextState::Pending(AppState::Playing)), "cold boot queues Playing");

        // Mid-run → relaunch: a fresh-run intent is requested, no in-process Playing transition.
        let mut pending = crate::savegame::PendingLoad::default();
        let mut next = NextState::<AppState>::default();
        let mut restart = RestartProcess::default();
        begin_new_game(true, Difficulty::Normal, &mut pending, &mut next, &mut restart);
        assert!(
            matches!(restart.0, Some(BootIntent::Fresh(Difficulty::Normal))),
            "mid-run New Game must relaunch for a clean world"
        );
        assert!(matches!(next, NextState::Unchanged), "mid-run does not start in-process");
    }

    /// `clear_battlefield` sweeps the dead run's transient entities (invaders / marchers / corpses)
    /// only when the Continue flag is set, leaves unrelated entities alone, and resets the flag so
    /// it fires exactly once per Continue.
    #[test]
    fn clear_battlefield_sweeps_transients_only_when_flagged() {
        use crate::cinematic::DirectorMarcher;
        use crate::dying::Dying;
        use crate::orks::WaveInvader;

        let mut app = App::new();
        app.init_resource::<ContinueInPlace>().add_systems(Update, clear_battlefield);

        // Flag clear ⇒ a wave invader survives (the sweep no-ops).
        let early = app.world_mut().spawn(WaveInvader { closest: 1.0, progress_at: 0.0 }).id();
        app.update();
        assert!(app.world().entities().contains(early), "no sweep while the flag is clear");

        // Flag set ⇒ every transient is reaped, an unrelated entity is kept, the flag resets.
        let marcher = app.world_mut().spawn(DirectorMarcher).id();
        let corpse = app.world_mut().spawn(Dying { since: 0.0 }).id();
        let bystander = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<ContinueInPlace>().0 = true;
        app.update();

        assert!(!app.world().entities().contains(early), "wave invader swept");
        assert!(!app.world().entities().contains(marcher), "cinematic marcher swept");
        assert!(!app.world().entities().contains(corpse), "fading corpse swept");
        assert!(app.world().entities().contains(bystander), "unrelated entity kept");
        assert!(!app.world().resource::<ContinueInPlace>().0, "flag reset after one sweep");
    }
}
