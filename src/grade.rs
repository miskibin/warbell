//! **Reactive colour grade** — a faithful port of the original 3js game's `ReactiveGrade`
//! (`gradeStore.ts` + `World.tsx`). The look is a *mature, cinematic* one, NOT a flat red tint:
//!   - a **radial vignette** whose screen *edges* quietly darken (black, never coloured), and
//!   - a **desaturation** that drains colour out of the frame,
//! both driven by the hero's HP ("dread" below 35% with a slow heartbeat throb) and by each
//! fresh hit (a sharp "wince" spike that snaps in and eases out over the 0.35 s hurt window).
//! At full health, untouched, there is **no vignette at all** — it only appears reactively.
//!
//! The vignette is a `bevy_ui` `BackgroundGradient` radial overlay (edge alpha = darkness); the
//! desaturation multiplies the camera's existing `ColorGrading.post_saturation` (so forest's
//! bespoke base grade is preserved at rest and only drains on damage).

use bevy::prelude::*;
use bevy::render::view::ColorGrading;

use crate::player::PlayerRes;

// Tunables — verbatim from the TS `gradeStore.ts` defaults.
const LOW_THRESHOLD: f32 = 0.35; // HP ratio below which the dread ramps in
const LOW_DARKEN: f32 = 0.16; // extra vignette darkness at 0 HP
const LOW_DESAT: f32 = 0.25; // saturation drained at 0 HP
const HEARTBEAT: f32 = 0.035; // low-HP edge throb amplitude (~0.9 Hz)
const WINCE_DARKEN: f32 = 0.13; // vignette spike on a fresh hit
const WINCE_DESAT: f32 = 0.16; // saturation dip on a fresh hit

#[derive(Component)]
struct Vignette;

pub struct GradePlugin;

impl Plugin for GradePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_vignette).add_systems(Update, drive_grade);
    }
}

fn spawn_vignette(mut commands: Commands) {
    commands.spawn((
        Vignette,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundGradient(vec![vignette_gradient(0.0)]),
        GlobalZIndex(-1),            // over the 3D scene, under the HUD
        bevy::ui::FocusPolicy::Pass, // never intercept clicks
    ));
}

/// The radial vignette: a clear core out to 60% of the way to the corner, ramping to a black
/// edge whose **alpha is `darkness`** (matches the TS `<Vignette offset={0.35}>` smoothstep —
/// the centre stays bright, only the edges darken).
fn vignette_gradient(darkness: f32) -> Gradient {
    let edge = Color::srgba(0.0, 0.0, 0.0, darkness.clamp(0.0, 1.0));
    Gradient::Radial(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::FarthestCorner,
        vec![
            ColorStop::new(Color::NONE, Val::Percent(35.0)),
            ColorStop::new(Color::NONE, Val::Percent(60.0)),
            ColorStop::new(edge, Val::Percent(100.0)),
        ],
    ))
}

/// Reactive fold (port of `World.tsx` ReactiveGrade): HP ratio + the decaying hurt pulse →
/// `(vignette darkness, saturation drain)`. `pulse` is the 0..1 charge that is 1.0 the instant of
/// a blow and bleeds to 0 across the 0.35 s `hurt_flash_until` window.
fn reactive(p: &tileworld_core::player::Player, now: f64) -> (f32, f32) {
    let ratio = if p.max_hp > 0.0 { (p.hp / p.max_hp) as f32 } else { 1.0 };
    let pulse = (((p.hurt_flash_until - now) / 0.35).clamp(0.0, 1.0)) as f32;
    let low = if ratio < LOW_THRESHOLD { (LOW_THRESHOLD - ratio) / LOW_THRESHOLD } else { 0.0 };
    let beat = if low > 0.0 { ((now as f32 * 5.5).sin() * 0.5 + 0.5) * low * HEARTBEAT } else { 0.0 };
    let darkness = (low * LOW_DARKEN + pulse * WINCE_DARKEN + beat).clamp(0.0, 0.97);
    let drain = low * LOW_DESAT + pulse * WINCE_DESAT; // ≥ 0; multiplies down the base saturation
    (darkness, drain)
}

fn drive_grade(
    time: Res<Time>,
    player: Option<Res<PlayerRes>>,
    mut cams: Query<&mut ColorGrading, With<Camera3d>>,
    mut overlays: Query<&mut BackgroundGradient, With<Vignette>>,
    mut base_sat: Local<Option<f32>>,
    mut last_drain: Local<f32>,
    mut last_darkness: Local<f32>,
) {
    let (darkness, drain) = match player.as_deref() {
        Some(p) => reactive(&p.0, time.elapsed_secs_f64()),
        None => (0.0, 0.0),
    };
    // Drain the camera's saturation toward grey on damage, restoring forest's base at rest
    // (captured once, so we never clobber the bespoke colour grade). Skip the write when the drain
    // is unchanged so we don't re-mark `ColorGrading` (and the gradient asset) dirty every frame at
    // rest — the overlay is invisible and static the vast majority of the time.
    if (drain - *last_drain).abs() > 1e-4 || base_sat.is_none() {
        *last_drain = drain;
        for mut cg in &mut cams {
            let base = *base_sat.get_or_insert(cg.global.post_saturation);
            cg.global.post_saturation = base * (1.0 - drain).max(0.0);
        }
    }
    // Debug: `FOREST_GRADETEST=1` forces a strong vignette so the harness can verify the radial
    // shape (dark edges, clear centre — NOT a full-screen fill).
    let darkness = if std::env::var("FOREST_GRADETEST").is_ok() { 0.6 } else { darkness };
    if (darkness - *last_darkness).abs() > 1e-4 {
        *last_darkness = darkness;
        for mut bg in &mut overlays {
            bg.0 = vec![vignette_gradient(darkness)];
        }
    }
}
