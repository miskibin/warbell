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
/// Progress-bar track size (px) and the sweeping highlight's width (px).
const BAR_W: f32 = 240.0;
const BAR_H: f32 = 5.0;
const FILL_W: f32 = 84.0;
/// Seconds for the highlight to scan across the track and back (ping-pong — stays inside the track,
/// so no overflow clipping needed, and reads as a clean indeterminate loader).
const SWEEP_PERIOD: f32 = 1.5;

/// Loading-screen flavour lines — one is shown per load (boot + every in-process reset), rotated so
/// repeat loads feel fresh. Kept short; each names a real mechanic so it doubles as a play tip. Edit
/// freely; order is irrelevant (the shown line advances by one each raise, seeded per boot).
const TIPS: &[&str] = &[
    "Ring the war bell at dusk to march the whole town at your side.",
    "Raise a shield with the right hand — it turns a berserker's charge.",
    "Build farms and lumber camps by day; they feed the long nights.",
    "Shamans hurl fire from afar. Close the distance or lose the keep.",
    "Spend the day's gold at the War Table — upgrades carry the nights.",
    "Repair the keep walls in daylight; by night they only crumble.",
    "Forage at dawn. A hungry hero swings slow.",
    "Every fallen heir is replaced, but the town remembers its dead.",
    "Break the gate of Gnashfang Hold to end the Warlord's reign.",
    "The Ashlands burn hotter — its orks hit harder than the home isle's.",
];

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
    /// Index into [`TIPS`] of the line currently shown. Advances one step on each rising edge
    /// (dormant → raised), so every load shows a fresh tip; seeded per boot in [`spawn_loading`].
    tip: usize,
}

impl Veil {
    /// Raise the veil opaque, restarting its readiness wait. Callers that raise it to cover a
    /// rebuild should also clear [`WorldReady`] so the veil holds until the fresh world lands.
    /// `raise` is called every frame a reset is pending, so the tip only advances on the rising
    /// edge (when it was dormant) — otherwise it would flicker through the pool during the wait.
    pub(crate) fn raise(&mut self, now: f32) {
        if !self.active {
            self.tip = (self.tip + 1) % TIPS.len();
        }
        self.active = true;
        self.alpha = 1.0;
        self.ready_at = None;
        self.spawned = now;
    }
}

/// Root node of the veil.
#[derive(Component)]
struct VeilRoot;
/// A text element of the veil (title / tip) whose colour we re-tint each frame for the fade.
#[derive(Component)]
struct LoadingText(Color);
/// The rotating-tip line — its *content* is swapped from [`TIPS`] when the shown index changes
/// (its colour fade is handled by [`LoadingText`], which it also carries).
#[derive(Component)]
struct TipText;
/// The indeterminate progress-bar track (the dim rail).
#[derive(Component)]
struct ProgressTrack;
/// The sweeping highlight inside the track — its `left` is animated each frame.
#[derive(Component)]
struct ProgressFill;

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
    // Seed which tip shows first off a wall-clock nibble so successive boots vary (the index then
    // advances on each in-process raise). Logic-irrelevant, so non-deterministic is fine.
    let tip = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0)
        % TIPS.len();
    commands.insert_resource(Veil {
        active,
        alpha: if active { 1.0 } else { 0.0 },
        spawned: time.elapsed_secs(),
        ready_at: None,
        hold,
        tip,
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
            // Rotating flavour line (its content set from `TIPS[tip]`, re-rolled per load).
            root.spawn((label(&fonts.regular, TIPS[tip], 14.0, TEXT_DIM), LoadingText(TEXT_DIM), TipText));
            // Indeterminate progress bar — a dim rail with a gold highlight that scans across it.
            // Font-free, so it draws from frame 0; the sweep animates during the warmup + fade.
            root.spawn((
                Node {
                    width: Val::Px(BAR_W),
                    height: Val::Px(BAR_H),
                    margin: UiRect::top(Val::Px(10.0)),
                    border_radius: radius(BAR_H * 0.5),
                    ..default()
                },
                BackgroundColor(GOLD.with_alpha(0.14)),
                ProgressTrack,
            ))
            .with_children(|track| {
                track.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(FILL_W),
                        height: Val::Percent(100.0),
                        border_radius: radius(BAR_H * 0.5),
                        ..default()
                    },
                    BackgroundColor(GOLD),
                    ProgressFill,
                ));
            });
        });
}

/// Sweep the progress highlight, rotate the tip, decide when the cover can lift (fonts + world
/// ready), fade, then go dormant. Never despawns the node — it persists so a later reset can raise
/// it again.
#[allow(clippy::too_many_arguments)]
fn drive_veil(
    time: Res<Time>,
    assets: Res<AssetServer>,
    fonts: Res<UiFonts>,
    world_ready: Res<WorldReady>,
    mut veil: ResMut<Veil>,
    mut root_q: Query<(&mut Node, &mut BackgroundColor), With<VeilRoot>>,
    mut fill_q: Query<(&mut Node, &mut BackgroundColor), (With<ProgressFill>, Without<VeilRoot>, Without<ProgressTrack>)>,
    mut track_q: Query<&mut BackgroundColor, (With<ProgressTrack>, Without<VeilRoot>, Without<ProgressFill>)>,
    mut texts: Query<(&LoadingText, &mut TextColor)>,
    mut tip_q: Query<&mut Text, With<TipText>>,
    // Last tip index applied to the text node — so we only rewrite (and re-lay-out) it when it
    // actually changes, never per-frame.
    mut last_tip: Local<usize>,
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

    // Scan the highlight across the rail and back (ping-pong, so it stays inside the track), fading
    // with the veil. It only visibly moves during the warmup + fade — the build freeze pins it
    // mid-rail, which reads as a paused loader rather than a hang.
    let tri = 1.0 - (2.0 * (now / SWEEP_PERIOD).fract() - 1.0).abs();
    let left = tri * (BAR_W - FILL_W);
    if let Ok((mut fill, mut fill_bg)) = fill_q.single_mut() {
        fill.left = Val::Px(left);
        fill_bg.0 = GOLD.with_alpha(alpha);
    }
    if let Ok(mut track_bg) = track_q.single_mut() {
        track_bg.0 = GOLD.with_alpha(0.14 * alpha);
    }

    // Swap the tip text only when the chosen line changed (re-layout is not free).
    if *last_tip != veil.tip {
        *last_tip = veil.tip;
        if let Ok(mut text) = tip_q.single_mut() {
            *text = Text::new(TIPS[veil.tip]);
        }
    }

    for (t, mut tc) in &mut texts {
        tc.0 = t.0.with_alpha(alpha);
    }
}
