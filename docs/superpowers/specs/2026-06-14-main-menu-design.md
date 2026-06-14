# Main menu — design (2026-06-14)

## Goal

A "proper" main menu, reachable **at any time**, with a living static scene (slow orbiting
camera over the keep at dusk, embers + fireflies), a **Resume** path back into a running game,
and a minimal **Credits** panel. Today's `StartScreen` (`game_state.rs`) is the de-facto menu but
is boot-only (no return path), frames the world at the default camera pose, has no ambient
particles, and no credits.

## Architecture

New plugin **`src/mainmenu.rs` (`MainMenuPlugin`)** owns the menu *ambiance*; button/state wiring
stays in `game_state.rs` where the screens live. Keeps an already-large `game_state.rs` focused on
the state machine.

### `mainmenu.rs` owns
- **Dusk pin** — `OnEnter(StartScreen)` sets `SkyClock.t = 0.5` (dusk) and `paused = true` so
  `advance_sky` holds it; `OnExit(StartScreen)` clears `paused`. (`advance_sky` already honors
  `paused`.) Reaching the menu mid-run snaps the sky to dusk; on Resume it eases back over ~1s.
- **Orbit camera** — `menu_orbit`, `run_if(in_state(StartScreen))`, slowly circles the main
  `Camera3d` around the keep (origin) looking at it. Safe because `player_camera` is gated to
  `Modal::None` and never runs on StartScreen (verified). On Resume the follow-cam reclaims it.
- **Embers + fireflies** — self-contained emitter (NOT the hero-tied `particles.rs`): `OnEnter`
  spawns ~150 warm rising embers + ~44 emissive bobbing fireflies (bloom) in a box around the
  keep, tagged `MenuSceneEntity`; `menu_drift` animates them (StartScreen-gated); `OnExit`
  despawns the tag.
- **Credits overlay** — resource `CreditsOpen(bool)`; `sync_credits_overlay` spawns/despawns a
  modal card (mirrors the existing `ConfirmWipe` overlay pattern); close on Esc / Close button.
  Content: *WARBELL* · Game by miskibin · Built with Bevy 0.18 + Rust · short thanks line.

### `game_state.rs` changes
- Resource `RunInProgress(bool)`: set true `OnEnter(Playing)`, false `OnEnter(GameOver)`.
- **RESUME** button on StartScreen menu column, shown iff `RunInProgress` → `AppState::Playing`
  (world intact, no reset). Distinct from CONTINUE GAME (loads the disk save).
- **CREDITS** button on StartScreen → `CreditsOpen = true`.
- **MAIN MENU** button on the pause screen and the game-over screen → `AppState::StartScreen`
  (the run stays frozen in memory; nothing torn down on leaving Playing).

### Correctness fix this forces
"New Game" from StartScreen currently starts **in-process** (world already fresh at cold boot).
Once StartScreen is reachable mid-run, that path would resume a *dirty* world. Route New Game by
`RunInProgress`: false (cold boot) → in-process fresh; true (mid-session) → process relaunch (the
real reset). Touch points: `start_click`, `start_screen_input`, `confirm_input`.

## Testing
- Headless unit tests (like existing `game_state` tests): `RunInProgress` true after entering
  Playing / false after GameOver; New-Game routing picks relaunch when `RunInProgress`.
- Visual: `FOREST_MENU=1` screenshot for the orbit framing + particles + dusk.

## Deliberately out of scope
Full credits roster, a separate non-gameplay diorama, autosave-on-quit (Resume keeps the live run
instead), menu music change.
