//! Ambient weather particles — a cheap CPU drift system. The active biome (the one the hero
//! stands in) picks a [`ParticleKind`]; [`update_weather`] spawns a few hundred tiny instanced
//! motes tagged [`Particle`] in a box that FOLLOWS the hero, and [`drift`] drifts + wraps them
//! within that moving box. Snow falls, pollen rises, dust drifts sideways, fireflies bob
//! (emissive → bloom). Crossing a biome edge **fades** the field's alpha in/out (so it eases in
//! rather than popping at full strength) and only despawns the old field once it's faded away.
//! No GPU particle plumbing — just instanced spheres.

use bevy::prelude::*;

use crate::biome::{BiomeAmbiences, BiomeEntity, ParticleKind};
use crate::player::HeroState;

/// Where the weather box is centred (the hero's world XZ), updated each frame so the field
/// follows the player across the map.
#[derive(Resource, Default)]
struct WeatherCenter(Vec2);

/// The weather field currently in the world + its fade ramp, so crossing a biome edge eases
/// the alpha in/out instead of popping the full mote count instantly.
#[derive(Resource, Default)]
struct Weather {
    /// The kind currently spawned (`None` = no field in the world right now).
    spawned: Option<ParticleKind>,
    /// Its shared material — alpha driven by `fade` each frame.
    mat: Option<Handle<StandardMaterial>>,
    /// The kind's intended max alpha (the ramp scales toward this).
    full_alpha: f32,
    /// 0 = invisible, 1 = fully faded in.
    fade: f32,
}

/// Horizontal half-extent of the particle box around the hero.
const R: f32 = 26.0;
/// How fast weather fades in/out when the hero crosses a biome edge (exponential, per second).
/// Gentle so snow/dust eases in over ~2–3 s rather than appearing all at once.
const WEATHER_FADE: f32 = 0.7;

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
        app.init_resource::<WeatherCenter>()
            .init_resource::<Weather>()
            // Picking/respawning the field is sim-ish (spawns entities, follows the hero), so it
            // freezes with panels; the drift itself is visual and keeps animating ungated.
            .add_systems(
                Update,
                (
                    update_weather.run_if(in_state(crate::game_state::Modal::None)),
                    drift,
                ),
            );
    }
}

/// Pick the weather for the biome the hero stands in and ease it in/out: ramp the field's alpha
/// toward full while the hero is in its biome, ramp it to zero (then despawn) when they leave,
/// and spawn the new biome's field once the old one has faded away. Also tracks the hero XZ so
/// [`drift`] can wrap the motes within a box that follows the player.
fn update_weather(
    mut commands: Commands,
    time: Res<Time>,
    hero: Option<Res<HeroState>>,
    ambiences: Option<Res<BiomeAmbiences>>,
    mode: Res<crate::player::PlayMode>,
    fly_cam: Query<&Transform, With<crate::controls::FlyCam>>,
    mut weather: ResMut<Weather>,
    mut center: ResMut<WeatherCenter>,
    existing: Query<Entity, With<Particle>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(hero), Some(ambiences)) = (hero, ambiences) else { return };
    // The weather box follows the hero in Play, but the free-roam fly-cam in FreeRoam — otherwise
    // flying off to frame a trailer shot leaves the motes parked back at the (stationary) hero and
    // the scene reads empty. Sample the biome + centre the box at whichever the view is tracking.
    let focus = match *mode {
        crate::player::PlayMode::FreeRoam => {
            fly_cam.single().map(|t| Vec3::new(t.translation.x, t.translation.y, t.translation.z)).unwrap_or(Vec3::new(hero.pos.x, hero.y, hero.pos.y))
        }
        crate::player::PlayMode::Play => Vec3::new(hero.pos.x, hero.y, hero.pos.y),
    };
    center.0 = Vec2::new(focus.x, focus.z);
    let desired = ambiences.sample(crate::worldmap::biome_at_world(focus.x, focus.z)).particle;
    // Exponential approach, stable across frame rates.
    let k = 1.0 - (-time.delta_secs() * WEATHER_FADE).exp();

    match weather.spawned {
        // The right field is already up → ease it in to full.
        Some(cur) if cur == desired => weather.fade += (1.0 - weather.fade) * k,
        // A field is up but the hero left its biome (or wants a different one) → fade it out,
        // and once it's invisible, despawn the motes and clear the slot.
        Some(_) => {
            weather.fade += (0.0 - weather.fade) * k;
            if weather.fade <= 0.02 {
                for e in &existing {
                    commands.entity(e).try_despawn();
                }
                weather.spawned = None;
                weather.mat = None;
                weather.fade = 0.0;
            }
        }
        // Nothing up → spawn the desired field invisible, to fade in over the next frames.
        None => {
            if desired != ParticleKind::None {
                let (mat, full_alpha) = spawn(desired, &mut commands, &mut meshes, &mut materials, focus);
                weather.spawned = Some(desired);
                weather.mat = Some(mat);
                weather.full_alpha = full_alpha;
                weather.fade = 0.0;
            }
        }
    }

    // Drive the shared material's alpha from the fade ramp (one material → one write).
    if let Some(handle) = weather.mat.clone() {
        if let Some(m) = materials.get_mut(&handle) {
            m.base_color = m.base_color.with_alpha(weather.full_alpha * weather.fade);
        }
    }
}

/// Tiny deterministic hash → [0,1) from an integer (per-instance variation).
fn h(n: u32) -> f32 {
    let mut t = n.wrapping_mul(0x6d2b_79f5).wrapping_add(0x9e37_79b9);
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}

/// Spawn the particle field for `kind`, centred on `center` (the hero). Returns the field's
/// shared material + its intended max alpha, so [`update_weather`] can fade it in/out. The caller
/// guarantees `kind != None`.
fn spawn(
    kind: ParticleKind,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    center: Vec3,
) -> (Handle<StandardMaterial>, f32) {
    // Per-kind look + motion.
    let (count, radius, color, emissive, alpha, vel, sway, y_lo, y_hi) = match kind {
        ParticleKind::Snow => (520u32, 0.07, Color::srgb(1.0, 1.0, 1.0), 0.0, 1.0, Vec3::new(0.0, -1.7, 0.0), 0.7, 0.0, 16.0),
        // Desert: a dense, fast, ground-to-overhead sheet of blown sand streaming sideways — with
        // the gust in `drift` it surges and lulls so the desert reads as genuinely windy.
        ParticleKind::Dust => (420, 0.06, Color::srgb(0.85, 0.74, 0.52), 0.0, 0.8, Vec3::new(2.6, 0.06, 1.0), 0.9, 0.05, 5.5),
        ParticleKind::Fireflies => (32, 0.08, Color::srgb(1.0, 0.95, 0.45), 7.0, 1.0, Vec3::ZERO, 0.9, 0.4, 2.2),
        ParticleKind::Pollen => (150, 0.045, Color::srgb(1.0, 0.96, 0.7), 0.6, 0.9, Vec3::new(0.1, 0.28, 0.05), 0.6, 0.2, 6.0),
        // Mist is unused (the flat-disc look read as hard-edged shards); kept as a soft mote
        // preset in case a biome wants it. Small sphere like the others, not a flat disc.
        ParticleKind::Mist => (90, 0.16, Color::srgb(0.82, 0.86, 0.84), 0.0, 0.16, Vec3::new(0.35, 0.0, 0.2), 0.25, 0.2, 2.4),
        ParticleKind::None => unreachable!("spawn is only called for an active weather kind"),
    };

    let mesh = meshes.add(Sphere::new(radius).mesh().ico(1).unwrap());
    // Always Blend so the fade ramp can drive alpha (even snow, whose full alpha is 1.0).
    let mat = materials.add(StandardMaterial {
        base_color: color.with_alpha(0.0), // starts invisible; the fade ramp brings it in
        emissive: LinearRgba::from(color) * emissive,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    for i in 0..count {
        let x = center.x + (h(i) * 2.0 - 1.0) * R;
        let z = center.z + (h(i + 7777) * 2.0 - 1.0) * R;
        let y = y_lo + h(i + 1234) * (y_hi - y_lo);
        let phase = h(i + 99) * std::f32::consts::TAU;
        // Per-instance velocity jitter so they don't move in lockstep.
        let jv = Vec3::new(
            (h(i + 11) - 0.5) * 0.4,
            (h(i + 22) - 0.5) * 0.15,
            (h(i + 33) - 0.5) * 0.4,
        );
        let tf = Transform::from_xyz(x, y, z).with_scale(Vec3::splat(0.7 + h(i + 5) * 0.6));
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            tf,
            Particle { vel: vel + jv, phase, sway, y_min: y_lo, y_max: y_hi },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
    (mat, alpha)
}

/// Drift + sway + wrap every particle within the box that follows the hero.
fn drift(time: Res<Time>, center: Res<WeatherCenter>, mut q: Query<(&Particle, &mut Transform)>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs_wrapped();
    let (cx, cz) = (center.0.x, center.0.y);
    // A slow, layered gust on the *directional* wind (≈0.15× lull → ≈1.85× surge) so a steady
    // drift becomes gusty — the desert dust (big horizontal vel) heaves and slackens like real
    // wind. Snow/pollen/fireflies have little directional vel, so they're barely touched.
    let gust = 1.0 + 0.55 * (t * 0.27).sin() + 0.3 * (t * 0.11 + 1.3).sin();
    for (p, mut tf) in &mut q {
        // Sinusoidal horizontal sway layered on the gusting base velocity.
        let sx = (t * 1.3 + p.phase).sin() * p.sway;
        let sz = (t * 1.1 + p.phase * 1.7).cos() * p.sway;
        tf.translation.x += (p.vel.x * gust + sx) * dt;
        tf.translation.y += p.vel.y * dt;
        tf.translation.z += (p.vel.z * gust + sz) * dt;

        // Wrap vertically by travel direction; wrap horizontally within the moving box.
        if p.vel.y < 0.0 && tf.translation.y < p.y_min {
            tf.translation.y = p.y_max;
        } else if p.vel.y > 0.0 && tf.translation.y > p.y_max {
            tf.translation.y = p.y_min;
        }
        if tf.translation.x > cx + R {
            tf.translation.x = cx - R;
        } else if tf.translation.x < cx - R {
            tf.translation.x = cx + R;
        }
        if tf.translation.z > cz + R {
            tf.translation.z = cz - R;
        } else if tf.translation.z < cz - R {
            tf.translation.z = cz + R;
        }
    }
}
