//! Click-order feedback — a Stronghold-style marker that pops + fades at the spot the player just
//! ordered troops to (green ring for a move/harvest, red for an attack/attack-move). Gives the "yes,
//! I heard you, here" confirmation the RTS was missing.
//!
//! `command.rs` writes an [`OrderMark`] for PLAYER orders only (AI orders don't mark). A short-lived
//! ring entity spawns at the ground point and animates for ~0.6s, then despawns (and frees its own
//! material so a long game doesn't leak one per click).

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::game_state::AppState;
use crate::rts::in_skirmish;

/// A player order landed here — spawn a marker. `attack` picks red vs green.
#[derive(Message)]
pub struct OrderMark {
    pub at: Vec2,
    pub attack: bool,
}

#[derive(Component)]
struct ClickMarker {
    spawned: f32,
    mat: Handle<StandardMaterial>,
}

/// Marker lifetime (seconds).
const DUR: f32 = 0.6;

pub struct RtsOrderMarkPlugin;

impl Plugin for RtsOrderMarkPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OrderMark>().add_systems(
            Update,
            (spawn_marks, animate_marks)
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

fn spawn_marks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut mesh: Local<Option<Handle<Mesh>>>,
    mut msgs: MessageReader<OrderMark>,
) {
    let ring = mesh
        .get_or_insert_with(|| meshes.add(Annulus::new(0.5, 0.82).mesh().resolution(28).build()))
        .clone();
    let now = time.elapsed_secs();
    for ev in msgs.read() {
        let col = if ev.attack { Color::srgb(1.0, 0.32, 0.26) } else { Color::srgb(0.4, 1.0, 0.52) };
        let mat = materials.add(StandardMaterial {
            base_color: col.with_alpha(0.9),
            emissive: LinearRgba::from(col) * 0.7,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        let y = crate::worldmap::ground_at_world(ev.at.x, ev.at.y).unwrap_or(0.0) + 0.08;
        commands.spawn((
            Mesh3d(ring.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(ev.at.x, y, ev.at.y)
                .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                .with_scale(Vec3::splat(1.5)),
            ClickMarker { spawned: now, mat },
        ));
    }
}

fn animate_marks(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &ClickMarker)>,
) {
    let now = time.elapsed_secs();
    for (e, mut tf, m) in &mut q {
        let t = (now - m.spawned) / DUR;
        if t >= 1.0 {
            materials.remove(&m.mat); // free the per-marker material
            commands.entity(e).try_despawn();
            continue;
        }
        // Snap in (scale 1.5 → 0.85 over the first third), hold, and fade alpha out over the life.
        let scale = 1.5 - 0.65 * (t * 3.0).min(1.0);
        tf.scale = Vec3::splat(scale);
        if let Some(mut mat) = materials.get_mut(&m.mat) {
            let a = (1.0 - t) * 0.9;
            let c = mat.base_color;
            mat.base_color = c.with_alpha(a);
        }
    }
}
