//! **Loading screen.** The window opens a beat before the fonts, the world mesh build, and the
//! render pipelines are ready, so for ~1 s the player stared at a blank clear-colour frame. This
//! plugin spawns a full-screen branded veil at `Startup` — so it's on the *first* presented frame —
//! and holds it (pulsing gold dots + the WARBELL wordmark once the display font loads) until the UI
//! fonts have loaded AND the island has been built, then fades it out to reveal the title screen.
//!
//! It's intentionally font-free for its always-visible part (a solid veil + dot pulse drawn from
//! plain `Node`s): those render on frame 0, while the title text only appears once Cinzel loads.

use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::{radius, rgb, GOLD, TEXT_DIM};

/// Veil colour — warm near-black, same family as the panel chrome (`theme::PANEL`).
const VEIL: Color = rgb(15, 11, 7);
/// Above EVERYTHING — the title scrim is z 50, the HUD lower; nothing else claims this high.
const Z: i32 = 10_000;
/// How long the fully-branded screen lingers AFTER everything is ready, before it fades — gives the
/// wordmark a beat on screen instead of flashing past.
const REVEAL_HOLD: f32 = 0.4;
/// Reveal fade length (seconds).
const FADE_DUR: f32 = 0.45;
/// Hard ceiling: reveal regardless after this long, so a missing/slow asset can't trap the player
/// on the loading screen forever.
const MAX_WAIT: f32 = 8.0;
/// Dot pulse rate (rad/s) and per-dot phase offset.
const PULSE: f32 = 4.2;

/// Root of the loading veil. `fade` rides 1→0 once `ready_at` elapses; the veil despawns at 0.
#[derive(Component)]
struct LoadingScreen {
    spawned: f32,
    /// Set once fonts + world are ready: the instant (now + [`REVEAL_HOLD`]) at which the fade begins.
    ready_at: Option<f32>,
    fade: f32,
    /// `FOREST_LOADTEST` staging shot: hold the veil up forever so a `FOREST_SHOT` can frame it
    /// (it normally fades away long before the capture harness's warm-up frames finish).
    hold: bool,
}

/// A pulsing loading dot, by phase offset.
#[derive(Component)]
struct LoadingDot(f32);

/// A text element of the veil (title / subtitle) whose colour we re-tint each frame for the fade.
#[derive(Component)]
struct LoadingText(Color);

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_loading).add_systems(Update, drive_loading);
    }
}

/// True for the headless capture harnesses (`FOREST_SHOT` / `FOREST_CLIP`) — they boot straight
/// into a live world and want a clean frame from the start, so the veil would only intrude.
fn capturing() -> bool {
    std::env::var("FOREST_SHOT").is_ok() || std::env::var("FOREST_CLIP").is_ok()
}

/// `FOREST_LOADTEST=1` — keep the veil up (even under a capture) so it can be screenshotted.
fn load_test() -> bool {
    std::env::var("FOREST_LOADTEST").is_ok()
}

fn spawn_loading(mut commands: Commands, time: Res<Time>, fonts: Res<UiFonts>) {
    let hold = load_test();
    if capturing() && !hold {
        return;
    }
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(22.0),
                ..default()
            },
            BackgroundColor(VEIL),
            GlobalZIndex(Z),
            LoadingScreen { spawned: time.elapsed_secs(), ready_at: None, fade: 1.0, hold },
        ))
        .with_children(|root| {
            // Wordmark (Cinzel) — blank until the display font loads, which is ~when we reveal.
            root.spawn((label(&fonts.display, "WARBELL", 72.0, GOLD), LoadingText(GOLD)));
            root.spawn((
                label(&fonts.regular, "Lighting the braziers…", 14.0, TEXT_DIM),
                LoadingText(TEXT_DIM),
            ));
            // Three pulsing dots — the font-free indicator that shows from frame 0.
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            })
            .with_children(|row| {
                for i in 0..3 {
                    row.spawn((
                        Node { width: Val::Px(11.0), height: Val::Px(11.0), border_radius: radius(6.0), ..default() },
                        BackgroundColor(GOLD),
                        LoadingDot(i as f32 * 0.55),
                    ));
                }
            });
        });
}

/// Pulse the dots, decide when everything is ready, fade the veil, then despawn it.
fn drive_loading(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    fonts: Res<UiFonts>,
    world: Query<(), With<BiomeEntity>>,
    mut root_q: Query<(Entity, &mut LoadingScreen, &mut BackgroundColor), Without<LoadingDot>>,
    mut dots: Query<(&LoadingDot, &mut BackgroundColor), Without<LoadingScreen>>,
    mut texts: Query<(&LoadingText, &mut TextColor)>,
) {
    let Ok((root, mut ls, mut root_bg)) = root_q.single_mut() else { return };
    let now = time.elapsed_secs();
    let dt = time.delta_secs();

    if ls.ready_at.is_some() {
        // Already armed — count down the hold, then drain the fade.
        if ls.ready_at.is_some_and(|t| now >= t) {
            ls.fade = (ls.fade - dt / FADE_DUR).max(0.0);
        }
    } else if !ls.hold {
        // Ready once the fonts have loaded AND the island has been built (so the scene is there
        // behind the half-transparent title scrim when we reveal). A hard cap guards a stuck asset.
        let fonts_ready = [&fonts.display, &fonts.regular, &fonts.extrabold]
            .iter()
            .all(|f| assets.is_loaded_with_dependencies(f.id()));
        let world_ready = !world.is_empty();
        if (fonts_ready && world_ready) || now - ls.spawned > MAX_WAIT {
            ls.ready_at = Some(now + REVEAL_HOLD);
        }
    }

    let fade = ls.fade;
    root_bg.0 = VEIL.with_alpha(fade);
    for (dot, mut bg) in &mut dots {
        let pulse = 0.3 + 0.7 * (0.5 + 0.5 * (now * PULSE + dot.0).sin());
        bg.0 = GOLD.with_alpha(pulse * fade);
    }
    for (t, mut tc) in &mut texts {
        tc.0 = t.0.with_alpha(fade);
    }

    if ls.ready_at.is_some() && fade <= 0.0 {
        commands.entity(root).try_despawn();
    }
}
