//! Ambient weather particles — a cheap CPU drift system. Each biome picks a
//! [`ParticleKind`]; the runner spawns a few hundred tiny instanced motes tagged
//! [`BiomeEntity`] + [`Particle`], and one `Update` system drifts + wraps them within a
//! box over the patch. Snow falls, pollen rises, dust + mist drift sideways, fireflies
//! bob (emissive → bloom). No GPU particle plumbing — just instanced spheres/discs.

use bevy::prelude::*;

use crate::biome::{BiomeEntity, ParticleKind};

/// Horizontal half-extent of the particle box around the origin (covers the patch).
const R: f32 = 26.0;

#[derive(Component)]
pub struct Particle {
    vel: Vec3,
    phase: f32,
    sway: f32,
    y_min: f32,
    y_max: f32,
}

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, drift);
    }
}

/// Tiny deterministic hash → [0,1) from an integer (per-instance variation).
fn h(n: u32) -> f32 {
    let mut t = n.wrapping_mul(0x6d2b_79f5).wrapping_add(0x9e37_79b9);
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}

/// Spawn the particle field for `kind`. No-op for [`ParticleKind::None`].
pub fn spawn(
    kind: ParticleKind,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    if kind == ParticleKind::None {
        return;
    }

    // Per-kind look + motion.
    let (count, radius, color, emissive, alpha, vel, sway, y_lo, y_hi, mist) = match kind {
        ParticleKind::Snow => (520u32, 0.07, Color::srgb(1.0, 1.0, 1.0), 0.0, 1.0, Vec3::new(0.0, -1.7, 0.0), 0.7, 0.0, 16.0, false),
        ParticleKind::Dust => (240, 0.06, Color::srgb(0.85, 0.74, 0.52), 0.0, 0.8, Vec3::new(0.9, 0.05, 0.4), 0.5, 0.15, 3.0, false),
        ParticleKind::Fireflies => (90, 0.08, Color::srgb(1.0, 0.95, 0.45), 7.0, 1.0, Vec3::ZERO, 0.9, 0.4, 2.2, false),
        ParticleKind::Pollen => (300, 0.045, Color::srgb(1.0, 0.96, 0.7), 0.6, 0.9, Vec3::new(0.1, 0.28, 0.05), 0.6, 0.2, 6.0, false),
        ParticleKind::Mist => (70, 3.2, Color::srgb(0.82, 0.86, 0.84), 0.0, 0.10, Vec3::new(0.35, 0.0, 0.2), 0.25, 0.25, 1.2, true),
        ParticleKind::None => unreachable!(),
    };

    let mesh = if mist {
        meshes.add(Circle::new(radius).mesh().build())
    } else {
        meshes.add(Sphere::new(radius).mesh().ico(1).unwrap())
    };
    let mat = materials.add(StandardMaterial {
        base_color: color.with_alpha(alpha),
        emissive: LinearRgba::from(color) * emissive,
        unlit: true,
        alpha_mode: if alpha < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        cull_mode: None,
        ..default()
    });

    for i in 0..count {
        let x = (h(i) * 2.0 - 1.0) * R;
        let z = (h(i + 7777) * 2.0 - 1.0) * R;
        let y = y_lo + h(i + 1234) * (y_hi - y_lo);
        let phase = h(i + 99) * std::f32::consts::TAU;
        // Per-instance velocity jitter so they don't move in lockstep.
        let jv = Vec3::new(
            (h(i + 11) - 0.5) * 0.4,
            (h(i + 22) - 0.5) * 0.15,
            (h(i + 33) - 0.5) * 0.4,
        );
        let mut tf = Transform::from_xyz(x, y, z).with_scale(Vec3::splat(0.7 + h(i + 5) * 0.6));
        if mist {
            // Lay the disc flat.
            tf.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        }
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            tf,
            Particle { vel: vel + jv, phase, sway, y_min: y_lo, y_max: y_hi },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
}

/// Drift + sway + wrap every particle within its box.
fn drift(time: Res<Time>, mut q: Query<(&Particle, &mut Transform)>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs_wrapped();
    for (p, mut tf) in &mut q {
        // Sinusoidal horizontal sway layered on the base velocity.
        let sx = (t * 1.3 + p.phase).sin() * p.sway;
        let sz = (t * 1.1 + p.phase * 1.7).cos() * p.sway;
        tf.translation.x += (p.vel.x + sx) * dt;
        tf.translation.y += p.vel.y * dt;
        tf.translation.z += (p.vel.z + sz) * dt;

        // Wrap vertically by travel direction; wrap horizontally within the box.
        if p.vel.y < 0.0 && tf.translation.y < p.y_min {
            tf.translation.y = p.y_max;
        } else if p.vel.y > 0.0 && tf.translation.y > p.y_max {
            tf.translation.y = p.y_min;
        }
        if tf.translation.x > R {
            tf.translation.x = -R;
        } else if tf.translation.x < -R {
            tf.translation.x = R;
        }
        if tf.translation.z > R {
            tf.translation.z = -R;
        } else if tf.translation.z < -R {
            tf.translation.z = R;
        }
    }
}
