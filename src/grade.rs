//! **Reactive colour grade** — a fullscreen red overlay that breathes with the hero's state:
//! it deepens as HP drops (a low-health "dread" with a faint heartbeat) and pulses red on each
//! hit (the wince). A render-agnostic `bevy_ui` overlay (no post-process node), so it layers
//! under the HUD panels but over the world. Ported in spirit from the TS `reactive_grade`.

use bevy::prelude::*;

use crate::combat_fx::HitFeedback;
use crate::player::PlayerRes;

#[derive(Component)]
struct Vignette;

pub struct GradePlugin;

impl Plugin for GradePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_vignette).add_systems(Update, drive_vignette);
    }
}

fn spawn_vignette(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.6, 0.0, 0.0, 0.0)),
        GlobalZIndex(5), // under the menus/panels (z≥50), over the world
        bevy::ui::FocusPolicy::Pass, // never intercept clicks
        Vignette,
    ));
}

fn drive_vignette(
    time: Res<Time>,
    player: Res<PlayerRes>,
    fb: Res<HitFeedback>,
    mut q: Query<&mut BackgroundColor, With<Vignette>>,
) {
    let p = &player.0;
    let ratio = if p.max_hp > 0.0 { (p.hp / p.max_hp) as f32 } else { 1.0 };
    let now = time.elapsed_secs();

    let mut a = 0.0;
    // Low-HP dread + a faint heartbeat once you're badly hurt.
    if ratio < 0.35 {
        let dread = (0.35 - ratio) / 0.35;
        a += dread * 0.22;
        a += dread * (now * 5.5).sin().max(0.0) * 0.06;
    }
    // Hit wince — the `HitFeedback.flash` channel the damage path already drives.
    a += fb.flash.clamp(0.0, 1.0) * 0.3;

    if let Ok(mut bg) = q.single_mut() {
        bg.0 = Color::srgba(0.6, 0.0, 0.0, a.min(0.5));
    }
}
