//! Main-menu **ambiance**: the living title scene behind `AppState::StartScreen`.
//!
//! The start screen itself (title, buttons, difficulty, legend) lives in `game_state.rs`. This
//! module owns the *world* dressing that makes the menu feel like a proper title screen:
//!
//! * **Dusk pin** — while on the start screen the sky clock is pinned to dusk (`SkyClock.t = 0.5`,
//!   `paused`), so embers/fireflies read against a warm-dark sky regardless of the live siege
//!   phase. Cleared on exit so time resumes in-game.
//! * **Orbit camera** — the main `Camera3d` slowly circles the keep (origin) looking inward. Only
//!   runs on `StartScreen`; the gameplay follow-cam (`player_camera`, gated to `Modal::None`)
//!   never runs here, so there's no fight, and it reclaims the camera the moment we re-enter play.
//! * **Embers + fireflies** — a self-contained CPU mote field (NOT the hero-tied `particles.rs`):
//!   warm embers rising off the keep + emissive fireflies bobbing (→ bloom). Spawned on enter,
//!   despawned on exit, animated by [`menu_drift`] while the menu is up.
//! * **Credits overlay** — a small modal card toggled by [`CreditsOpen`] (the start screen's
//!   CREDITS button flips it), mirroring the `ConfirmWipe` overlay pattern in `game_state.rs`.

use bevy::prelude::*;

use crate::game_state::AppState;
use crate::scene::SkyClock;
use crate::ui::anim::{anim, anim_btn, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreditsOpen>()
            .add_systems(
                OnEnter(AppState::StartScreen),
                (enter_menu_sky, spawn_menu_backdrop),
            )
            .add_systems(
                OnExit(AppState::StartScreen),
                (exit_menu_sky, despawn_menu_scene),
            )
            .add_systems(
                Update,
                menu_park_camera.run_if(in_state(AppState::StartScreen)),
            )
            // Credits overlay is reconciled ungated so its open/close survives state edges.
            .add_systems(Update, (sync_credits_overlay, credits_input));
    }
}

// ── Dusk pin ────────────────────────────────────────────────────────────────────────────

/// Golden-hour dusk: sun low in the west but still ABOVE the horizon, so the forest is warmly lit
/// (not black) while the sky stays warm-dark enough for the embers/fireflies to glow. Pure horizon
/// (`t = 0.5`, sun elevation ≈ 0 → `advance_sky` `day` ≈ 0.02) rendered the wider, un-buried menu
/// framing as near-black; this lifts the sun just enough to read.
const DUSK_T: f32 = 0.475;

/// Hold the sky at dusk while the menu is up. `advance_sky` honors `paused`, so pinning `t` here
/// keeps it from drifting toward the live siege-phase sun arc.
fn enter_menu_sky(mut clock: ResMut<SkyClock>) {
    clock.t = DUSK_T;
    clock.paused = true;
}

/// Let time resume the instant we leave the menu (Resume / New Game / Continue).
fn exit_menu_sky(mut clock: ResMut<SkyClock>) {
    clock.paused = false;
}

// ── Menu camera (parked at the sky, behind the static backdrop) ──────────────────────────────

/// Park the camera tilted UP at the empty dusk sky while the menu is up. The menu now draws a
/// pre-rendered [`spawn_menu_backdrop`] image over the whole screen, so the live 3D scene is never
/// seen — pointing the camera at bare sky frustum-culls the forest (and its shadow/SSAO/outline
/// work over geometry), which is the one GPU win we can take SAFELY. We deliberately do NOT cut the
/// post-FX stack per-screen: `quality::apply_quality` re-inserts the bloom/DoF/SSAO passes on any
/// settings change, and toggling them at runtime is a documented wgpu-validation crash (the
/// `debug_qswitch` crash-repro). The pose is fixed (no per-frame terrain snap); overwritten every
/// frame so nothing nudges it. Only runs on `StartScreen`, where no other system drives the camera.
fn menu_park_camera(mut cam: Query<&mut Transform, With<Camera3d>>) {
    let Some(mut tf) = cam.iter_mut().next() else { return };
    // Eye up high, looking up-and-out at the horizon sky — almost no geometry in frustum.
    *tf = Transform::from_xyz(0.0, 30.0, 0.0)
        .looking_to(Vec3::new(0.0, 0.5, -1.0).normalize(), Vec3::Y);
}

// ── Static backdrop ──────────────────────────────────────────────────────────────────────────

/// Everything spawned for the menu scene — despawned wholesale on exit.
#[derive(Component)]
struct MenuSceneEntity;

/// The pre-rendered menu backdrop: a golden-hour look across the forest to the keep — the war-bell
/// tower and torch-lit keep silhouetted against the low sun's god rays, over baked DoF + haze. It
/// covers the whole screen
/// as a UI image, so the live 3D world is never seen on the menu — that's what lets
/// [`menu_park_camera`] aim at bare sky and skip drawing the forest. `GlobalZIndex(-100)` keeps it
/// BEHIND the start-screen title/buttons (`game_state.rs`, default z) but in front of the 3D pass
/// (all UI draws over 3D). Tagged [`MenuSceneEntity`] so `despawn_menu_scene` clears it on exit.
fn spawn_menu_backdrop(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        ImageNode::new(asset_server.load("ui/menu_backdrop.png")),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        GlobalZIndex(-100),
        MenuSceneEntity,
    ));
}

fn despawn_menu_scene(mut commands: Commands, q: Query<Entity, With<MenuSceneEntity>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

// ── Credits overlay ─────────────────────────────────────────────────────────────────────────

/// `true` = the credits card is open. The start screen's CREDITS button flips it on; Esc / the
/// Close button flips it off.
#[derive(Resource, Default)]
pub struct CreditsOpen(pub bool);

#[derive(Component)]
struct CreditsUi;
#[derive(Component)]
struct CreditsCloseBtn;

/// Spawn / despawn the credits overlay to match [`CreditsOpen`] (mirrors `sync_confirm_overlay`).
fn sync_credits_overlay(
    mut commands: Commands,
    open: Res<CreditsOpen>,
    fonts: Res<UiFonts>,
    existing: Query<Entity, With<CreditsUi>>,
) {
    let want = open.0;
    let have = !existing.is_empty();
    if want && !have {
        spawn_credits(&mut commands, &fonts);
    } else if !want && have {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

/// The credits card — a centred scrim above the start screen (z 90).
fn spawn_credits(commands: &mut Commands, fonts: &UiFonts) {
    commands
        .spawn((
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
            GlobalZIndex(90),
            CreditsUi,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(54.0), Val::Px(34.0)),
                    border: widgets::border(1.0),
                    border_radius: radius(R_PANEL),
                    ..default()
                },
                widgets::card_paint(),
                anim(AnimKind::PopIn, 0.0, 0.26),
            ))
            .with_children(|c| {
                c.spawn((
                    label(&fonts.display, "WARBELL", 40.0, rgb(244, 228, 188)),
                    Node { margin: UiRect::bottom(Val::Px(2.0)), ..default() },
                ));
                c.spawn(label(&fonts.semibold, "A knight's last stand.", 14.0, TEXT_DIM));
                // Gold rule.
                c.spawn((
                    Node {
                        width: Val::Px(220.0),
                        height: Val::Px(1.0),
                        margin: UiRect::vertical(Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(rgba(224, 168, 74, 0.6)),
                ));
                c.spawn(label(&fonts.semibold, "GAME BY", 11.0, KICKER));
                c.spawn(label(&fonts.bold, "miskibin", 20.0, GOLD));
                c.spawn((
                    label(&fonts.semibold, "BUILT WITH", 11.0, KICKER),
                    Node { margin: UiRect::top(Val::Px(8.0)), ..default() },
                ));
                c.spawn(label(&fonts.bold, "Bevy 0.18  ·  Rust", 16.0, TEXT));
                c.spawn((
                    label(&fonts.regular, "Thanks for playing — now go hold the keep.", 13.0, TEXT_DIM),
                    Node { margin: UiRect::top(Val::Px(10.0)), ..default() },
                ));
                // Close.
                c.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(34.0), Val::Px(11.0)),
                        margin: UiRect::top(Val::Px(14.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_BTN),
                        ..default()
                    },
                    widgets::btn_primary_paint(),
                    CreditsCloseBtn,
                    anim_btn(AnimKind::PopIn, 0.05, 0.26),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.extrabold, "CLOSE", 16.0, INK));
                });
                c.spawn((
                    label(&fonts.regular, "Esc to close", 12.0, GREY),
                    Node { margin: UiRect::top(Val::Px(4.0)), ..default() },
                ));
            });
        });
}

/// Close the credits on Esc or the Close button. No-ops while the card is shut.
fn credits_input(
    keys: Res<ButtonInput<KeyCode>>,
    q: Query<&Interaction, (Changed<Interaction>, With<CreditsCloseBtn>)>,
    mut open: ResMut<CreditsOpen>,
) {
    if !open.0 {
        return;
    }
    let mut close = keys.just_pressed(KeyCode::Escape);
    for interaction in &q {
        if *interaction == Interaction::Pressed {
            close = true;
        }
    }
    if close {
        open.0 = false;
    }
}
