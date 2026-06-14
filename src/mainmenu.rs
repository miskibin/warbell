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
                (enter_menu_sky, spawn_menu_particles),
            )
            .add_systems(
                OnExit(AppState::StartScreen),
                (exit_menu_sky, despawn_menu_scene),
            )
            .add_systems(
                Update,
                (menu_orbit, menu_drift).run_if(in_state(AppState::StartScreen)),
            )
            // Credits overlay is reconciled ungated so its open/close survives state edges.
            .add_systems(Update, (sync_credits_overlay, credits_input));
    }
}

// ── Dusk pin ────────────────────────────────────────────────────────────────────────────

/// Dusk: sun at the west horizon — a warm-dark sky that lets the embers/fireflies glow.
const DUSK_T: f32 = 0.5;

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

// ── Menu camera (a slow drift over the forest) ──────────────────────────────────────────────

/// World XZ of the forest biome region (see `CLAUDE.md` biome centres). The menu frames *this*,
/// not the whole island — a tight, tree-filled dusk shot reads far better than a map-like overview.
const SCENE_CENTER: Vec3 = Vec3::new(-60.0, 0.0, 39.0);

// A fixed, player-eye-level shot looking into the forest — no drift, no orbit.
const CAM_OFFSET: Vec3 = Vec3::new(24.0, 2.2, 18.0); // from SCENE_CENTER; y ≈ a standing player's eye
const CAM_LOOK_Y: f32 = 3.5; // look level-to-slightly-up into the trees

/// Hold the camera at a static, player-height pose over the forest. Overwrites the transform every
/// frame so nothing nudges it; only runs on `StartScreen`, where no other system drives the camera.
fn menu_orbit(mut cam: Query<&mut Transform, With<Camera3d>>) {
    let Some(mut tf) = cam.iter_mut().next() else { return };
    let pos = SCENE_CENTER + CAM_OFFSET;
    let look = SCENE_CENTER + Vec3::new(0.0, CAM_LOOK_Y, 0.0);
    *tf = Transform::from_translation(pos).looking_at(look, Vec3::Y);
}

// ── Embers + fireflies ──────────────────────────────────────────────────────────────────────

/// Everything spawned for the menu scene (motes) — despawned wholesale on exit.
#[derive(Component)]
struct MenuSceneEntity;

/// A drifting menu mote. Embers rise + recycle; fireflies (`twinkle`) bob laterally and pulse.
#[derive(Component)]
struct MenuMote {
    vel: Vec3,
    phase: f32,
    sway: f32,
    y_min: f32,
    y_max: f32,
    base_scale: f32,
    twinkle: bool,
}

/// Half-extent of the mote box around [`SCENE_CENTER`] — sized to keep the field in the forest
/// camera's frame.
const BOX_R: f32 = 26.0;

/// Tiny deterministic hash → [0,1) for per-instance variation (no RNG dependency).
fn h(n: u32) -> f32 {
    let mut t = n.wrapping_mul(0x6d2b_79f5).wrapping_add(0x9e37_79b9);
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}

fn spawn_menu_particles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Warm embers — orange, gently glowing, rising and recycling.
    let ember_col = Color::srgb(1.0, 0.5, 0.18);
    let ember_mesh = meshes.add(Sphere::new(0.075).mesh().ico(1).unwrap());
    let ember_mat = materials.add(StandardMaterial {
        base_color: ember_col.with_alpha(0.9),
        emissive: LinearRgba::from(ember_col) * 3.2,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    // Fireflies — warm yellow-green, glowing (bloom), bobbing + twinkling.
    let fly_col = Color::srgb(0.92, 1.0, 0.5);
    let fly_mesh = meshes.add(Sphere::new(0.1).mesh().ico(1).unwrap());
    let fly_mat = materials.add(StandardMaterial {
        base_color: fly_col.with_alpha(1.0),
        emissive: LinearRgba::from(fly_col) * 5.5,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    const EMBER_N: u32 = 150;
    for i in 0..EMBER_N {
        let x = SCENE_CENTER.x + (h(i) * 2.0 - 1.0) * BOX_R;
        let z = SCENE_CENTER.z + (h(i + 7777) * 2.0 - 1.0) * BOX_R;
        let y = h(i + 1234) * 22.0;
        let scale = 0.6 + h(i + 5) * 0.7;
        let vel = Vec3::new(
            (h(i + 11) - 0.5) * 0.5,
            0.7 + h(i + 22) * 0.8, // rise
            (h(i + 33) - 0.5) * 0.5,
        );
        commands.spawn((
            Mesh3d(ember_mesh.clone()),
            MeshMaterial3d(ember_mat.clone()),
            Transform::from_xyz(x, y, z).with_scale(Vec3::splat(scale)),
            MenuMote {
                vel,
                phase: h(i + 99) * std::f32::consts::TAU,
                sway: 0.6,
                y_min: 0.0,
                y_max: 22.0,
                base_scale: scale,
                twinkle: false,
            },
            bevy::light::NotShadowCaster,
            MenuSceneEntity,
        ));
    }

    const FLY_N: u32 = 32;
    for i in 0..FLY_N {
        let x = SCENE_CENTER.x + (h(i + 311) * 2.0 - 1.0) * BOX_R;
        let z = SCENE_CENTER.z + (h(i + 911) * 2.0 - 1.0) * BOX_R;
        let y = 1.5 + h(i + 555) * 5.5;
        let scale = 0.7 + h(i + 71) * 0.6;
        let vel = Vec3::new((h(i + 17) - 0.5) * 0.6, 0.0, (h(i + 29) - 0.5) * 0.6);
        commands.spawn((
            Mesh3d(fly_mesh.clone()),
            MeshMaterial3d(fly_mat.clone()),
            Transform::from_xyz(x, y, z).with_scale(Vec3::splat(scale)),
            MenuMote {
                vel,
                phase: h(i + 99) * std::f32::consts::TAU,
                sway: 0.9,
                y_min: 1.2,
                y_max: 7.5,
                base_scale: scale,
                twinkle: true,
            },
            bevy::light::NotShadowCaster,
            MenuSceneEntity,
        ));
    }
}

/// Drift + sway + wrap every menu mote within the box around the keep; twinkle the fireflies.
fn menu_drift(time: Res<Time>, mut q: Query<(&MenuMote, &mut Transform), With<MenuSceneEntity>>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs_wrapped();
    for (m, mut tf) in &mut q {
        tf.translation += m.vel * dt;
        tf.translation.x += (t + m.phase).sin() * m.sway * dt;
        tf.translation.z += (t * 0.8 + m.phase).cos() * m.sway * dt;
        // Vertical recycle within the mote's band.
        if tf.translation.y > m.y_max {
            tf.translation.y = m.y_min;
        } else if tf.translation.y < m.y_min {
            tf.translation.y = m.y_max;
        }
        // Horizontal wrap within the box (keeps the field centred on the forest forever).
        if tf.translation.x > SCENE_CENTER.x + BOX_R {
            tf.translation.x -= 2.0 * BOX_R;
        } else if tf.translation.x < SCENE_CENTER.x - BOX_R {
            tf.translation.x += 2.0 * BOX_R;
        }
        if tf.translation.z > SCENE_CENTER.z + BOX_R {
            tf.translation.z -= 2.0 * BOX_R;
        } else if tf.translation.z < SCENE_CENTER.z - BOX_R {
            tf.translation.z += 2.0 * BOX_R;
        }
        if m.twinkle {
            let pulse = 0.6 + 0.4 * ((t * 3.0 + m.phase).sin() * 0.5 + 0.5);
            tf.scale = Vec3::splat(m.base_scale * pulse);
        }
    }
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
