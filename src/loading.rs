//! **Loading veil.** A full-screen branded cover (pulsing gold dots + the WARBELL wordmark) that
//! hides an unready frame. Two jobs:
//!
//! 1. **Boot** — the window opens a beat before fonts, the world mesh build, and the render
//!    pipelines are ready, so for ~1 s the player would stare at a blank clear-colour frame. The
//!    veil is on the *first* presented frame and holds until the UI fonts have loaded AND the world
//!    has been built ([`crate::biome::WorldReady`]), then fades to reveal the title screen.
//! 2. **In-process reset** — New Game / Restart / Play Again rebuild the world *in place* (no
//!    relaunch). The reset raises the veil and clears `WorldReady`; the veil holds over the
//!    despawn-and-rebuild and lifts once the fresh world lands. See `game_state::drive_fresh_run`.
//!
//! So the veil is **persistent + re-raisable**: one node spawned at `Startup` (never despawned),
//! driven by the [`Veil`] resource — `Display::None` while hidden, opaque while raised, fading in
//! between. It's intentionally font-free for its always-visible part (a solid veil + dot pulse from
//! plain `Node`s) so it renders on frame 0; the title text only appears once Cinzel loads.

use bevy::prelude::*;

use crate::biome::WorldReady;
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
/// Hard ceiling: reveal regardless after this long, so a missing/slow asset (or a stuck rebuild)
/// can't trap the player on the veil forever.
const MAX_WAIT: f32 = 8.0;
/// Dot pulse rate (rad/s) and per-dot phase offset.
const PULSE: f32 = 4.2;

/// Drives the persistent veil node. Raised opaque (`alpha = 1`) at boot and on every in-process
/// reset; reveals (fades to 0) once fonts + the world are ready, then sits dormant until re-raised.
/// `Default` is the dormant (hidden) state — the live resource is built in [`spawn_loading`].
#[derive(Resource, Default)]
pub(crate) struct Veil {
    /// Whether the veil is currently covering (counts down to a fade, then goes dormant).
    active: bool,
    /// Current opacity, 1 → 0 across the reveal fade.
    alpha: f32,
    /// `elapsed_secs` when this raise began (drives the `MAX_WAIT` safety cap).
    spawned: f32,
    /// Set once fonts + world are ready: the instant (now + [`REVEAL_HOLD`]) the fade begins.
    ready_at: Option<f32>,
    /// `FOREST_LOADTEST`: hold the veil up forever so a `FOREST_SHOT` can frame it.
    hold: bool,
}

impl Veil {
    /// Raise the veil opaque, restarting its readiness wait. Callers that raise it to cover a
    /// rebuild should also clear [`WorldReady`] so the veil holds until the fresh world lands.
    pub(crate) fn raise(&mut self, now: f32) {
        self.active = true;
        self.alpha = 1.0;
        self.ready_at = None;
        self.spawned = now;
    }
}

/// Root node of the veil.
#[derive(Component)]
struct VeilRoot;
/// A pulsing loading dot, by phase offset.
#[derive(Component)]
struct LoadingDot(f32);
/// A text element of the veil (title / subtitle) whose colour we re-tint each frame for the fade.
#[derive(Component)]
struct LoadingText(Color);

pub struct LoadingPlugin;

impl Plugin for LoadingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_loading).add_systems(Update, drive_veil);
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
    // The capture harnesses want a clean first frame — boot with the veil dormant (but still
    // spawned, so a later reset under the harness could raise it). `FOREST_LOADTEST` overrides.
    let active = hold || !capturing();
    commands.insert_resource(Veil {
        active,
        alpha: if active { 1.0 } else { 0.0 },
        spawned: time.elapsed_secs(),
        ready_at: None,
        hold,
    });

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
                // Hidden when dormant so it never eats clicks; `drive_veil` flips it on a raise.
                display: if active { Display::Flex } else { Display::None },
                ..default()
            },
            BackgroundColor(VEIL.with_alpha(if active { 1.0 } else { 0.0 })),
            GlobalZIndex(Z),
            VeilRoot,
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

/// Pulse the dots, decide when the cover can lift (fonts + world ready), fade, then go dormant.
/// Never despawns the node — it persists so a later reset can raise it again.
fn drive_veil(
    time: Res<Time>,
    assets: Res<AssetServer>,
    fonts: Res<UiFonts>,
    world_ready: Res<WorldReady>,
    mut veil: ResMut<Veil>,
    mut root_q: Query<(&mut Node, &mut BackgroundColor), With<VeilRoot>>,
    mut dots: Query<(&LoadingDot, &mut BackgroundColor), Without<VeilRoot>>,
    mut texts: Query<(&LoadingText, &mut TextColor)>,
) {
    let Ok((mut node, mut root_bg)) = root_q.single_mut() else { return };
    let now = time.elapsed_secs();
    let dt = time.delta_secs();

    if veil.active {
        if veil.ready_at.is_some() {
            // Armed — once the hold elapses, drain the fade; dormant at 0.
            if veil.ready_at.is_some_and(|t| now >= t) {
                veil.alpha = (veil.alpha - dt / FADE_DUR).max(0.0);
                if veil.alpha <= 0.0 {
                    veil.active = false;
                }
            }
        } else if !veil.hold {
            // Reveal once the fonts have loaded AND the world has been (re)built. A hard cap guards
            // a stuck asset or rebuild.
            let fonts_ready = [&fonts.display, &fonts.regular, &fonts.extrabold]
                .iter()
                .all(|f| assets.is_loaded_with_dependencies(f.id()));
            if (fonts_ready && world_ready.0) || now - veil.spawned > MAX_WAIT {
                veil.ready_at = Some(now + REVEAL_HOLD);
            }
        }
    }

    let alpha = veil.alpha;
    // Flex while there's anything to show; None when fully dormant so it never blocks input.
    node.display = if alpha > 0.0 { Display::Flex } else { Display::None };
    root_bg.0 = VEIL.with_alpha(alpha);
    for (dot, mut bg) in &mut dots {
        let pulse = 0.3 + 0.7 * (0.5 + 0.5 * (now * PULSE + dot.0).sin());
        bg.0 = GOLD.with_alpha(pulse * alpha);
    }
    for (t, mut tc) in &mut texts {
        tc.0 = t.0.with_alpha(alpha);
    }
}
