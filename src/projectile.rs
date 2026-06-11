//! Homing magic bolts — the ork shaman's ranged spell. A bolt tracks the hero's
//! live position, deals damage on arrival (via `PendingHeroDamage`, so a raised
//! shield blocks it), and fizzles after a short lifetime or once it has flown
//! its full range. Ported from the original game's `projectileStore.ts`.

use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::player::{spawn_burst, CombatFx, HeroState, PendingHeroDamage};

/// A bolt within this distance of its target counts as a hit.
pub(crate) const BOLT_HIT_RADIUS: f32 = 0.6;

/// Bolt flight speed (world units/sec).
const BOLT_SPEED: f32 = 9.0;
/// Seconds a bolt lives before it fizzles regardless of distance.
const BOLT_TTL: f32 = 3.0;
/// Distance a bolt may fly before fizzling short of a fleeing target.
const BOLT_MAX_RANGE: f32 = 16.0;

/// Outcome of advancing a bolt one frame.
#[derive(Debug, PartialEq)]
pub(crate) enum BoltStep {
    /// Still flying — new world position.
    Fly(Vec3),
    /// Reached the target — deal damage.
    Hit,
    /// Flew its full range without connecting — despawn.
    Fizzle,
}

/// Advance a bolt one frame toward `target`, moving `step` units. Returns the
/// outcome and the updated travelled distance.
pub(crate) fn advance_bolt(
    pos: Vec3,
    target: Vec3,
    step: f32,
    traveled: f32,
    max_range: f32,
) -> (BoltStep, f32) {
    let to = target - pos;
    let len = to.length();
    if len < BOLT_HIT_RADIUS {
        return (BoltStep::Hit, traveled);
    }
    let nt = traveled + step;
    if nt >= max_range {
        return (BoltStep::Fizzle, nt);
    }
    (BoltStep::Fly(pos + to / len.max(1e-6) * step), nt)
}

/// One bolt the shaman wants spawned this frame.
pub struct BoltSpawn {
    pub origin: Vec3,
    pub damage: f32,
}

/// Spawn queue — `orks.rs` pushes, `spawn_queued_bolts` drains. Mirrors the
/// `PendingHeroDamage` channel idiom (no `Commands` needed in the ork brain).
#[derive(Resource, Default)]
pub struct BoltSpawns(pub Vec<BoltSpawn>);

/// A live homing bolt flying at the hero.
#[derive(Component)]
struct Bolt {
    damage: f32,
    speed: f32,
    ttl: f32,
    traveled: f32,
    max_range: f32,
}

/// Shared bolt mesh + glowing purple material, built once.
#[derive(Resource)]
struct BoltAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

fn setup_bolt_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.16).mesh().ico(2).unwrap());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.61, 1.0),
        emissive: LinearRgba::rgb(2.4, 1.4, 4.0),
        unlit: true,
        ..default()
    });
    commands.insert_resource(BoltAssets { mesh, mat });
}

fn spawn_queued_bolts(
    mut commands: Commands,
    assets: Res<BoltAssets>,
    mut spawns: ResMut<BoltSpawns>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
) {
    for s in spawns.0.drain(..) {
        // The staff-release crack (`warp-cast.ogg`), spatial at the shaman's hands.
        cues.write(crate::audio::AudioCue::WarpCast(s.origin));
        commands.spawn((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.mat.clone()),
            Transform::from_translation(s.origin),
            Bolt {
                damage: s.damage,
                speed: BOLT_SPEED,
                ttl: BOLT_TTL,
                traveled: 0.0,
                max_range: BOLT_MAX_RANGE,
            },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
}

fn step_bolts(
    time: Res<Time>,
    hero: Res<HeroState>,
    fx: Option<Res<CombatFx>>,
    mut pending: ResMut<PendingHeroDamage>,
    mut marks: MessageWriter<crate::aftermath::BattleMark>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Bolt, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    let target = Vec3::new(hero.pos.x, hero.y + 1.0, hero.pos.y);
    for (e, mut b, mut tf) in &mut q {
        b.ttl -= dt;
        if !hero.alive || b.ttl <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let (out, traveled) =
            advance_bolt(tf.translation, target, b.speed * dt, b.traveled, b.max_range);
        b.traveled = traveled;
        match out {
            BoltStep::Fly(p) => tf.translation = p,
            BoltStep::Hit => {
                pending.0 += b.damage;
                if let Some(fx) = &fx {
                    spawn_burst(&mut commands, fx, tf.translation, false);
                }
                // Leave a scorch on the turf where the bolt burst (aftermath.rs).
                marks.write(crate::aftermath::BattleMark { at: tf.translation });
                commands.entity(e).despawn();
            }
            BoltStep::Fizzle => commands.entity(e).despawn(),
        }
    }
}

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoltSpawns>()
            .add_systems(Startup, setup_bolt_assets)
            .add_systems(
                Update,
                (spawn_queued_bolts, step_bolts)
                    .chain()
                    .run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homes_closer_each_step() {
        let (out, tr) = advance_bolt(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 1.0, 0.0, 40.0);
        assert_eq!(out, BoltStep::Fly(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(tr, 1.0);
    }

    #[test]
    fn hits_when_within_radius() {
        let (out, _) = advance_bolt(Vec3::ZERO, Vec3::new(0.3, 0.0, 0.0), 1.0, 0.0, 40.0);
        assert_eq!(out, BoltStep::Hit);
    }

    #[test]
    fn fizzles_past_max_range() {
        let (out, tr) = advance_bolt(Vec3::ZERO, Vec3::new(50.0, 0.0, 0.0), 2.0, 39.0, 40.0);
        assert_eq!(out, BoltStep::Fizzle);
        assert_eq!(tr, 41.0);
    }
}
