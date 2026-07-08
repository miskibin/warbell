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
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;

use crate::ui::anim::{anim, anim_btn, AnimKind};
use crate::ui::fonts::{label, UiFonts, FONT_BODY, FONT_DISPLAY, FONT_LABEL};
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

/// Register `Update` systems that belong to the world-sim — the ones that MUST freeze when a panel
/// opens or `Playing` is left. Exactly equivalent to
/// `app.add_systems(Update, systems.run_if(in_state(Modal::None)))`, but it names the intent and
/// makes the freeze-gate impossible to forget: CLAUDE.md requires every new sim system to carry
/// that gate or it silently runs through pauses/panels. Keep plain `add_systems` for the ungated
/// render / VFX / HUD systems that must keep drawing while the world is frozen.
pub trait SimAppExt {
    fn add_sim_systems<M>(
        &mut self,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self;
}

impl SimAppExt for App {
    fn add_sim_systems<M>(
        &mut self,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.add_systems(Update, systems.run_if(in_state(Modal::None)))
    }
}

/// Set by the fresh-run buttons (New Game / Play Again / pause Restart) to request a **full
/// in-process reset** at the carried difficulty — no relaunch, the window stays. [`drive_fresh_run`]
/// consumes it: route through `StartScreen → Playing` (whose `OnExit(StartScreen)` runs every
/// `reset_*` system — hero / economy / town / upgrades / siege / lives / quests), re-arm the world
/// rebuild (`biome::PendingBuild`, which despawns all `BiomeEntity` and re-runs `worldmap::build`),
/// sweep the battlefield, and hold the loading veil over the hitch. A cold-boot New Game skips this
/// (the world is already fresh); *Continue* skips it too (resumes the saved world in place).
#[derive(Resource, Default)]
pub struct FreshRunPending(pub Option<crate::siege::Difficulty>);

/// Flags an **in-process Continue** (resume last night / load last save) so [`clear_battlefield`]
/// sweeps the dead run's transient entities (invaders, marchers, bolts, corpses) on the next
/// `Playing` frame. The resource restore + hero revival ride existing wiring (`apply_pending_load`
/// + `OnExit(GameOver)` `reset_player`); this only triggers the entity sweep.
#[derive(Resource, Default)]
pub struct ContinueInPlace(pub bool);

/// The "Overwrite saved game?" confirm dialog. `Some(_)` = open (the bool, once "from pause", is now
/// vestigial). `None` = closed. Every fresh-run button routes through this whenever a save exists,
/// so a misclick can't wipe a long run; on confirm it starts the fresh run (in-process).
#[derive(Resource, Default)]
pub struct ConfirmWipe(pub Option<bool>);

/// True once a run is live (entered `Playing`) and not yet ended, so the start screen — reachable
/// mid-run via the pause/game-over **Main Menu** button — knows to offer **RESUME** (drop back into
/// the frozen run) and, crucially, to route **New Game** through a full in-process reset
/// ([`FreshRunPending`]) rather than the cold-boot direct start (which would resume a *dirty*
/// world). Set true `OnEnter(Playing)`, false `OnEnter(GameOver)`.
#[derive(Resource, Default)]
pub struct RunInProgress(pub bool);

pub struct GameStatePlugin;

impl Plugin for GameStatePlugin {
    fn build(&self, app: &mut App) {
        // Screenshot/demo hooks want a live world without the menu in the way.
        let boot = if skip_menu() { AppState::Playing } else { AppState::StartScreen };
        app.insert_state(boot)
            .add_sub_state::<Modal>()
            .init_resource::<FreshRunPending>()
            .init_resource::<ConfirmWipe>()
            .init_resource::<ContinueInPlace>()
            .init_resource::<RunInProgress>()
            // Drive a pending in-process fresh run to completion (ungated — works over every screen).
            .add_systems(Update, drive_fresh_run)
            // Sweep the dead run's battlefield the frame an in-process Continue lands in Playing.
            .add_systems(Update, clear_battlefield.run_if(in_state(AppState::Playing)))
            // Overwrite-confirm dialog: reconcile its overlay + resolve its input. Ungated so it
            // works over the start / game-over / pause screens alike.
            .add_systems(Update, (sync_confirm_overlay, confirm_input))
            .add_systems(Update, (pause_toggle, watch_end))
            // Pause-menu buttons + live settings labels (only while the pause screen is up).
            .add_systems(
                Update,
                pause_click.run_if(in_state(AppState::Paused)),
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
                (start_screen_input, cycle_difficulty, start_click, update_diff_seg, cycle_map, update_map_seg)
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
        // The whole point of the headless perf harness (`perftest.rs`) is measuring LIVE gameplay —
        // sitting at the start screen (nothing simulating) silently produces meaningless numbers that
        // still look plausible (entity/mesh counts come from the unconditional Startup world-build).
        // Previously this only worked by accident when a run happened to also set FOREST_WAVE.
        || std::env::var("FOREST_PERFTEST").is_ok()
}

// ── In-process fresh run (the "full reset", no relaunch) ─────────────────────────────────

/// Drive a [`FreshRunPending`] request to a clean, in-process new run — the window never closes.
///
/// The trick: every fresh-run path is funnelled through the `StartScreen → Playing` transition,
/// whose `OnExit(StartScreen)` already runs the full suite of `reset_*` systems (hero, economy,
/// inventory, town, upgrades, siege, lives, quests, graves). So we only have to (1) get to the
/// start screen, (2) re-arm the world-geometry rebuild + sweep the battlefield, then (3) drop into
/// `Playing` — and hold the loading veil over the whole thing so the brief menu flash + rebuild
/// hitch are invisible.
///
/// Runs over **every** screen (game-over Play Again, pause Restart, mid-run New Game). Idempotent:
/// it re-raises the veil each frame it's pending, and clears the request the frame it lands.
fn drive_fresh_run(
    mut req: ResMut<FreshRunPending>,
    app: Res<State<AppState>>,
    mut next_app: ResMut<NextState<AppState>>,
    mut veil: ResMut<crate::loading::Veil>,
    mut world_ready: ResMut<crate::biome::WorldReady>,
    mut pending_build: ResMut<crate::biome::PendingBuild>,
    mut pending_load: ResMut<crate::savegame::PendingLoad>,
    mut cont: ResMut<ContinueInPlace>,
    siege: Option<ResMut<crate::siege::Siege>>,
    time: Res<Time>,
    // Set once the rebuild is armed, so we don't re-arm (restart the chunked build) every frame
    // while waiting for it to finish.
    mut armed: Local<bool>,
) {
    let Some(diff) = req.0 else {
        *armed = false;
        return;
    };
    // Keep the cover up across the hop + the chunked rebuild (hides the start-screen flash).
    veil.raise(time.elapsed_secs());

    if *app.get() != AppState::StartScreen {
        // Hop to the title first; its OnExit is what runs the fresh-run resets. Stay pending.
        next_app.set(AppState::StartScreen);
        return;
    }

    // On the start screen: arm the rebuild ONCE. The world build (`biome::drive_build`) then runs
    // chunked over many frames HERE in StartScreen — sim is off, so nothing runs on the half-built
    // world, and the loading veil animates over it. The build is deterministic + run-state-blind,
    // so building before the OnExit(StartScreen) resets is fine (it's how cold boot already works).
    if !*armed {
        if let Some(mut siege) = siege {
            siege.difficulty = diff; // reset_siege preserves difficulty across its wipe
        }
        pending_load.0 = None; // a fresh run, never a load — don't let a stale save overwrite the reset
        world_ready.0 = false; // hold the veil until the rebuilt world lands
        pending_build.0 = true; // re-run the world build (despawn all BiomeEntity + rebuild fresh)
        cont.0 = true; // clear_battlefield sweeps invaders / marchers / bolts / corpses once Playing
        *armed = true;
    }

    // Drop into Playing only once the rebuilt world has fully landed — firing OnExit(StartScreen)'s
    // `reset_*` suite, then starting the sim on a complete world.
    if world_ready.0 {
        next_app.set(AppState::Playing);
        req.0 = None;
        *armed = false;
    }
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
    gfx_menu: Res<crate::ui::graphics_menu::GraphicsMenuOpen>,
    mut build_mode: ResMut<crate::town::BuildMode>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    // While the graphics Settings page is open, Esc closes IT (its own handler), not the pause.
    if gfx_menu.0 {
        return;
    }
    // While the overwrite dialog is up, Esc belongs to it (cancel), not the pause toggle.
    if confirm.0.is_some() {
        return;
    }
    // Build mode is a live placement state (not a panel) — Esc leaves it instead of pausing.
    if build_mode.active {
        build_mode.active = false;
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
/// routes New Game through an in-process reset instead of a dirty direct start.
fn mark_run_active(mut run: ResMut<RunInProgress>) {
    run.0 = true;
}

/// The run ended (Victory/Defeat) — the start screen reached from game-over has nothing live to
/// RESUME; New Game from there is a fresh in-process reset.
fn clear_run_active(mut run: ResMut<RunInProgress>) {
    run.0 = false;
}

/// Start a fresh run from the start screen, picking the *correct* reset path: at cold boot the
/// world is already fresh, so drop straight into `Playing`; if a run is already live (the menu was
/// reached mid-session), request a full **in-process** reset via [`FreshRunPending`] (the in-process
/// world is dirty, so it must be rebuilt). Mirrors the routing in `confirm_input`.
fn begin_new_game(
    run_active: bool,
    cur_diff: crate::siege::Difficulty,
    map_changed: bool,
    pending: &mut crate::savegame::PendingLoad,
    next_app: &mut NextState<AppState>,
    fresh: &mut FreshRunPending,
) {
    if run_active || map_changed {
        // Mid-run (dirty world) OR the player picked a different map than the one currently built:
        // a full in-process reset, so `drive_fresh_run` rebuilds the world from the chosen `ActiveMap`.
        fresh.0 = Some(cur_diff);
    } else {
        pending.0 = None; // cold boot, same map: world already fresh, start in-process
        next_app.set(AppState::Playing);
    }
}

/// Whether the picked map (the [`crate::worldmap::ActiveMap`] resource) differs from the world
/// currently built — if so a New Game must rebuild even on a cold boot.
fn map_changed(active: &crate::worldmap::ActiveMap) -> bool {
    active.0 as u8 != crate::worldmap::current_map_u8()
}

fn start_screen_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    run: Res<RunInProgress>,
    siege: Option<Res<crate::siege::Siege>>,
    mut fresh: ResMut<FreshRunPending>,
    active_map: Res<crate::worldmap::ActiveMap>,
    gfx_menu: Res<crate::ui::graphics_menu::GraphicsMenuOpen>,
) {
    // While the graphics Settings page is open over the title, it owns the keyboard (Esc closes it).
    if gfx_menu.0 {
        return;
    }
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
            begin_new_game(run.0, cur_diff, map_changed(&active_map), &mut pending, &mut next_app, &mut fresh);
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
/// between runs). Centralises the missing-resource fallback used to seed a fresh-run request.
/// Call with `siege.as_deref()` so the `Option<Res<Siege>>` isn't consumed.
fn current_difficulty(siege: Option<&crate::siege::Siege>) -> crate::siege::Difficulty {
    siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal)
}

fn gameover_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut fresh: ResMut<FreshRunPending>,
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
    // first if it'd overwrite) — a full in-process reset, no relaunch.
    if defeat && save.0 && keys.just_pressed(KeyCode::KeyC) {
        begin_continue(&mut pending, &mut next_app, &mut cont);
    } else if keys.just_pressed(KeyCode::Enter) {
        if save.0 {
            confirm.0 = Some(false);
        } else {
            fresh.0 = Some(cur_diff); // full in-process reset (drive_fresh_run rebuilds the world)
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
/// The "Settings" button on the start screen — opens the graphics Settings page.
#[derive(Component)]
struct StartSettingsButton;
/// The "Credits" button on the start screen — opens the credits overlay ([`mainmenu::CreditsOpen`]).
#[derive(Component)]
struct CreditsButton;
/// The "Quit" button on the start screen — closes the game (sends `AppExit::Success`).
#[derive(Component)]
struct QuitButton;
/// The "Main Menu" button on the pause screen — returns to the start screen, run kept live.
#[derive(Component)]
struct PauseMenuBtn;
/// The "Main Menu" button on the game-over screen — returns to the start screen.
#[derive(Component)]
struct GameOverMenuBtn;
/// A difficulty segment (click to select).
#[derive(Component)]
struct SegButton(crate::siege::Difficulty);
/// A map segment on the start screen (click to choose which world a New Game generates).
#[derive(Component)]
struct MapSeg(crate::worldmap::MapId);
// ── Pause-menu buttons ──
#[derive(Component)]
struct PauseResumeBtn;
#[derive(Component)]
struct PauseSaveBtn;
#[derive(Component)]
struct PauseLoadBtn;
#[derive(Component)]
struct PauseRestartBtn;
/// The single **Settings** button on the pause screen — opens the tabbed Settings menu.
#[derive(Component)]
struct PauseGfxBtn;
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
    active_map: Res<crate::worldmap::ActiveMap>,
) {
    let cur = current_difficulty(siege.as_deref());
    let cur_map = active_map.0;
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
                // Map selector — which world a New Game builds (Green Isle / Ashlands). Mirrors the
                // difficulty segmented control; the live choice lives in the `ActiveMap` resource.
                m.spawn((
                    Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(7.0), ..default() },
                    anim(AnimKind::Rise, 0.35, 0.7),
                ))
                .with_children(|d| {
                    d.spawn(label(&fonts.semibold, "MAP", 11.0, KICKER));
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
                        for mp in MAPS {
                            let on = mp == cur_map;
                            seg.spawn((
                                Button,
                                Interaction::default(),
                                Node {
                                    padding: UiRect::axes(Val::Px(20.0), Val::Px(7.0)),
                                    border_radius: radius(7.0),
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(6.0),
                                    ..default()
                                },
                                BackgroundColor(if on { GOLD_DEEP } else { Color::NONE }),
                                BorderColor::all(Color::NONE),
                                MapSeg(mp),
                            ))
                            .with_children(|b| {
                                b.spawn(label(&fonts.semibold, map_name(mp), 13.0, if on { INK } else { TEXT_FAINT }));
                                // Flag a not-yet-finished map so players know what they're picking.
                                if !map_ready(mp) {
                                    b.spawn(label(&fonts.semibold, "(not ready)", 10.0, if on { INK } else { TEXT_FAINT }));
                                }
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
                // Settings — secondary, opens the full graphics Settings page.
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
                    StartSettingsButton,
                    anim_btn(AnimKind::Rise, 0.39, 0.7),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.bold, "SETTINGS", 14.0, GOLD));
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
                // Quit — closes the game entirely (sends AppExit). Secondary, bottom of the column.
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
                    QuitButton,
                    anim_btn(AnimKind::Rise, 0.42, 0.7),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.bold, "QUIT", 14.0, GOLD));
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
                    (&["RMB"], "Block / Parry"),
                    (&["Alt"], "Dodge roll"),
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

/// A full-width pause-menu button (primary-blue paint), with `text` and an optional extra bundle
/// on its label (`()` for the simple buttons).
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
        b.spawn((label(font, text, FONT_LABEL, INK), label_extra));
    });
}

fn spawn_pause_screen(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    save: Res<crate::savegame::SaveExists>,
    siege: Res<crate::siege::Siege>,
) {
    let has_save = save.0;
    // Manual save is a day-only action (a mid-siege snapshot would resume in the wrong place).
    let can_save = matches!(siege.phase, crate::siege::GamePhase::Prep);

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
                label(&fonts.display, "PAUSED", FONT_DISPLAY, GOLD),
                Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() },
            ));

            pause_btn(c, &fonts.extrabold, "RESUME", PauseResumeBtn, (), 0.04);

            // One Settings button → opens the full tabbed Settings menu over the pause screen.
            pause_btn(c, &fonts.extrabold, "SETTINGS", PauseGfxBtn, (), 0.07);

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
                label(&fonts.regular, "Esc to resume", FONT_BODY, GREY),
                Node { margin: UiRect::top(Val::Px(6.0)), ..default() },
            ));
        });
    });
}

/// Handle the pause-menu buttons: resume, the three in-place settings toggles, and the two run
/// actions. **Load last save** resumes in-process; **Restart** requests a full in-process reset via
/// [`FreshRunPending`] (confirming first if it'd overwrite a save) — the window stays put.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn pause_click(
    q: Query<
        (
            &Interaction,
            Option<&PauseResumeBtn>,
            Option<&PauseLoadBtn>,
            Option<&PauseRestartBtn>,
            Option<&PauseGfxBtn>,
            Option<&PauseSaveBtn>,
            Option<&PauseMenuBtn>,
        ),
        Changed<Interaction>,
    >,
    mut next_app: ResMut<NextState<AppState>>,
    mut fresh: ResMut<FreshRunPending>,
    siege: Option<Res<crate::siege::Siege>>,
    mut gfx_menu: ResMut<crate::ui::graphics_menu::GraphicsMenuOpen>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    mut cont_req: ResMut<ContinueInPlace>,
    mut save_req: MessageWriter<crate::savegame::RequestSave>,
) {
    if confirm.0.is_some() {
        return; // dialog owns input
    }
    let cur_diff = current_difficulty(siege.as_deref());
    for (interaction, resume, load, restart_b, gfx_b, save_b, menu_b) in &q {
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
                fresh.0 = Some(cur_diff); // full in-process reset (drive_fresh_run rebuilds the world)
            }
        }
        if gfx_b.is_some() {
            gfx_menu.0 = true; // open the full tabbed Settings menu over the pause screen
        }
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
            Option<&MapSeg>,
            Option<&QuitButton>,
            Option<&StartSettingsButton>,
        ),
        Changed<Interaction>,
    >,
    mut next_app: ResMut<NextState<AppState>>,
    mut pending: ResMut<crate::savegame::PendingLoad>,
    siege: Option<ResMut<crate::siege::Siege>>,
    save: Res<crate::savegame::SaveExists>,
    mut confirm: ResMut<ConfirmWipe>,
    run: Res<RunInProgress>,
    mut fresh: ResMut<FreshRunPending>,
    mut credits: ResMut<crate::mainmenu::CreditsOpen>,
    mut active_map: ResMut<crate::worldmap::ActiveMap>,
    mut gfx_menu: ResMut<crate::ui::graphics_menu::GraphicsMenuOpen>,
    mut exit: MessageWriter<AppExit>,
) {
    if confirm.0.is_some() {
        return; // dialog up — the scrim already blocks these, but be explicit
    }
    let mut siege = siege;
    let cur_diff = current_difficulty(siege.as_deref());
    for (interaction, play, cont, resume, cred, seg, mapseg, quit, settings_b) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if quit.is_some() {
            exit.write(AppExit::Success);
        }
        if settings_b.is_some() {
            gfx_menu.0 = true; // open the graphics Settings page over the start screen
        }
        // Pick a map segment first, so a New Game in the same click batch sees the new choice.
        if let Some(ms) = mapseg {
            active_map.0 = ms.0;
        }
        if play.is_some() {
            if save.0 {
                confirm.0 = Some(false); // New Game would overwrite — confirm first
            } else {
                begin_new_game(run.0, cur_diff, map_changed(&active_map), &mut pending, &mut next_app, &mut fresh);
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

/// The maps offered on the start screen (order = segment order).
const MAPS: [crate::worldmap::MapId; 2] =
    [crate::worldmap::MapId::Home, crate::worldmap::MapId::Ashlands];
fn map_name(m: crate::worldmap::MapId) -> &'static str {
    match m {
        crate::worldmap::MapId::Home => "Green Isle",
        crate::worldmap::MapId::Ashlands => "Ashlands",
    }
}
/// Whether a map is finished enough to ship without a caveat. Ashlands is a playable prototype
/// (ground/atmosphere reskinned, but tree foliage etc. not yet charred), so the menu flags it.
fn map_ready(m: crate::worldmap::MapId) -> bool {
    !matches!(m, crate::worldmap::MapId::Ashlands)
}

/// On the start screen, **M** cycles which map a New Game builds. The segmented control reflects it.
fn cycle_map(keys: Res<ButtonInput<KeyCode>>, mut active: ResMut<crate::worldmap::ActiveMap>) {
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    active.0 = match active.0 {
        crate::worldmap::MapId::Home => crate::worldmap::MapId::Ashlands,
        crate::worldmap::MapId::Ashlands => crate::worldmap::MapId::Home,
    };
}

/// Recolour the map segments to match the live [`crate::worldmap::ActiveMap`] (M key or click).
fn update_map_seg(
    active: Res<crate::worldmap::ActiveMap>,
    mut q: Query<(&MapSeg, &mut BackgroundColor, &Children)>,
    mut text_q: Query<&mut TextColor>,
) {
    for (seg, mut bg, children) in &mut q {
        let on = seg.0 == active.0;
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
    mut fresh: ResMut<FreshRunPending>,
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
                fresh.0 = Some(cur_diff); // full in-process reset (drive_fresh_run rebuilds the world)
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
/// Esc = cancel). On overwrite: delete the save, mark it gone, and start a fresh run. From a
/// cold-boot start screen that's a direct in-process start (world is already fresh); mid-session it
/// requests a full in-process reset via [`FreshRunPending`]. Ungated; no-ops while closed.
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
    mut fresh: ResMut<FreshRunPending>,
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
            // Cold-boot start screen — the world is already fresh, so start in-process directly.
            // OnExit(StartScreen) runs the fresh-run resets.
            pending.0 = None;
            next_app.set(AppState::Playing);
        } else {
            // Mid-session New Game / Restart (incl. a start screen reached mid-run) → full in-process
            // reset, since the in-process world is dirty (drive_fresh_run rebuilds it).
            let cur_diff = current_difficulty(siege.as_deref());
            fresh.0 = Some(cur_diff);
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

    /// A headless app wired with just enough to exercise [`drive_fresh_run`]: the state machine,
    /// the resources it reads/writes, and the system itself. No rendering.
    fn fresh_run_app(state: AppState) -> App {
        let mut app = booted(state);
        app.init_resource::<FreshRunPending>()
            .init_resource::<ContinueInPlace>()
            .init_resource::<crate::loading::Veil>()
            .insert_resource(crate::biome::WorldReady(true))
            .insert_resource(crate::biome::PendingBuild(false))
            .init_resource::<crate::savegame::PendingLoad>()
            .add_systems(Update, drive_fresh_run);
        app
    }

    /// From the start screen, a `FreshRunPending` request arms the in-process reset (world rebuild +
    /// veil hold + battlefield sweep) but STAYS pending until the chunked build finishes — only then
    /// does it drain the request and queue `Playing` (so `OnExit(StartScreen)` runs every `reset_*`
    /// on a complete world). This holds the sim off the half-built world during the rebuild.
    #[test]
    fn drive_fresh_run_arms_rebuild_then_waits_for_world() {
        use crate::siege::Difficulty;
        let mut app = fresh_run_app(AppState::StartScreen);
        app.world_mut().resource_mut::<FreshRunPending>().0 = Some(Difficulty::Hard);
        app.update();

        // Armed, but waiting: build re-armed, WorldReady cleared, still pending, not yet Playing.
        {
            let w = app.world();
            assert!(w.resource::<crate::biome::PendingBuild>().0, "world rebuild armed");
            assert!(!w.resource::<crate::biome::WorldReady>().0, "WorldReady cleared so the veil holds");
            assert!(w.resource::<ContinueInPlace>().0, "battlefield sweep flagged");
            assert!(w.resource::<FreshRunPending>().0.is_some(), "stays pending until the world is built");
            assert!(
                !matches!(w.resource::<NextState<AppState>>(), NextState::Pending(AppState::Playing)),
                "not Playing until the rebuilt world lands"
            );
        }

        // Simulate the chunked build finishing.
        app.world_mut().resource_mut::<crate::biome::WorldReady>().0 = true;
        app.update();

        let w = app.world();
        assert!(w.resource::<FreshRunPending>().0.is_none(), "request drained once the world is up");
        assert!(
            matches!(w.resource::<NextState<AppState>>(), NextState::Pending(AppState::Playing)),
            "Playing queued so OnExit(StartScreen) runs the resets"
        );
    }

    /// From elsewhere (pause Restart / game-over Play Again) the request first hops to the start
    /// screen — its `OnExit` is what runs the resets — and stays pending until it lands there, only
    /// arming the rebuild on the start-screen frame, then waiting for the build before `Playing`.
    #[test]
    fn drive_fresh_run_hops_through_start_screen_when_paused() {
        use crate::siege::Difficulty;
        let mut app = fresh_run_app(AppState::Paused);
        app.world_mut().resource_mut::<FreshRunPending>().0 = Some(Difficulty::Normal);

        app.update(); // Paused frame: queue the StartScreen hop, stay pending, don't arm yet
        assert!(app.world().resource::<FreshRunPending>().0.is_some(), "still pending mid-hop");
        assert!(!app.world().resource::<crate::biome::PendingBuild>().0, "not armed off the title");

        app.update(); // StateTransition lands StartScreen, then drive arms the rebuild (still pending)
        assert_eq!(app.world().resource::<State<AppState>>().get(), &AppState::StartScreen);
        assert!(app.world().resource::<crate::biome::PendingBuild>().0, "rebuild armed on the title");
        assert!(app.world().resource::<FreshRunPending>().0.is_some(), "still pending until the world is built");

        // Simulate the chunked build finishing → the request drains.
        app.world_mut().resource_mut::<crate::biome::WorldReady>().0 = true;
        app.update();
        assert!(app.world().resource::<FreshRunPending>().0.is_none(), "request drained once the world is up");
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

    /// New Game routing: from a cold-boot start screen (`run_active = false`) the world is already
    /// fresh, so we start **in-process** (queue `Playing`, no rebuild request). Once a run is live
    /// (`run_active = true`, e.g. the menu was reached mid-session) the in-process world is dirty, so
    /// `begin_new_game` requests a full in-process reset via [`FreshRunPending`] (rebuilt by
    /// `drive_fresh_run`) instead of queueing `Playing` directly.
    #[test]
    fn new_game_routes_in_process_cold_and_rebuild_midrun() {
        use crate::siege::Difficulty;

        // Cold boot → straight to Playing: a transition is queued, no fresh-run rebuild requested.
        let mut pending = crate::savegame::PendingLoad::default();
        let mut next = NextState::<AppState>::default();
        let mut fresh = FreshRunPending::default();
        begin_new_game(false, Difficulty::Normal, false, &mut pending, &mut next, &mut fresh);
        assert!(fresh.0.is_none(), "cold boot needs no rebuild");
        assert!(matches!(next, NextState::Pending(AppState::Playing)), "cold boot queues Playing");

        // Mid-run → in-process rebuild: a fresh-run request is set, no direct Playing transition
        // (drive_fresh_run routes it through StartScreen → Playing to fire the resets + rebuild).
        let mut pending = crate::savegame::PendingLoad::default();
        let mut next = NextState::<AppState>::default();
        let mut fresh = FreshRunPending::default();
        begin_new_game(true, Difficulty::Normal, false, &mut pending, &mut next, &mut fresh);
        assert!(
            matches!(fresh.0, Some(Difficulty::Normal)),
            "mid-run New Game must request an in-process reset at the chosen difficulty"
        );
        assert!(matches!(next, NextState::Unchanged), "mid-run does not transition Playing directly");
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
        let corpse = app.world_mut().spawn(Dying { since: 0.0, dir: Vec2::ZERO, power: 1.0 }).id();
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
