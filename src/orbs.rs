//! **Reward orbs** — the gold/XP motes that burst off a slain ork, hang for a beat, then
//! accelerate into the hero and bank on contact (the racing HUD counter is the payoff).
//! Wraps the test-gated `tileworld_core::orb` integration; combat queues a [`RewardBurst`]
//! (the `BoltSpawns` idiom) and this module spawns + steps + grants the orbs.

use bevy::prelude::*;
use tileworld_core::orb::{self, Orb, OrbKind, OrbStep, PlayerPose};

use crate::game_state::Modal;
use crate::player::{HeroState, PlayerRes};

/// A reward queued by combat on a kill — split into gold + xp orbs by [`spawn_queued_orbs`].
pub struct RewardBurst {
    pub at: Vec3,
    pub gold: i64,
    pub xp: i64,
}

/// Pending reward bursts (drained each frame). Combat pushes; this module consumes.
#[derive(Resource, Default)]
pub struct RewardBursts(pub Vec<RewardBurst>);

#[derive(Component)]
struct RewardOrb(Orb);

#[derive(Resource)]
struct OrbAssets {
    mesh: Handle<Mesh>,
    gold: Handle<StandardMaterial>,
    xp: Handle<StandardMaterial>,
}

/// Deterministic mulberry32 driving the burst spread (no parity need — "feels the same").
#[derive(Resource)]
struct OrbRng(u32);
impl Default for OrbRng {
    fn default() -> Self {
        OrbRng(0x9e37_79b9)
    }
}
impl OrbRng {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }
}

pub struct OrbsPlugin;

impl Plugin for OrbsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RewardBursts>()
            .init_resource::<OrbRng>()
            .add_systems(Startup, setup_orb_assets)
            .add_systems(
                Update,
                (spawn_queued_orbs, step_reward_orbs)
                    .chain()
                    .run_if(in_state(Modal::None)),
            );
    }
}

fn setup_orb_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.13).mesh().ico(2).unwrap());
    let gold = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.82, 0.25),
        emissive: LinearRgba::rgb(3.0, 2.0, 0.3),
        unlit: true,
        ..default()
    });
    let xp = materials.add(StandardMaterial {
        base_color: Color::srgb(0.42, 0.8, 1.0),
        emissive: LinearRgba::rgb(0.6, 1.6, 3.0),
        unlit: true,
        ..default()
    });
    commands.insert_resource(OrbAssets { mesh, gold, xp });
}

/// Drain queued bursts → spawn gold + xp orb entities (count split, no value lost).
fn spawn_queued_orbs(
    mut bursts: ResMut<RewardBursts>,
    mut rng: ResMut<OrbRng>,
    assets: Option<Res<OrbAssets>>,
    mut commands: Commands,
) {
    let Some(assets) = assets else {
        bursts.0.clear();
        return;
    };
    for b in bursts.0.drain(..) {
        let (x, y, z) = (b.at.x as f64, b.at.y as f64, b.at.z as f64);
        // Gold splits into 2–4 motes (~one per 4 gold); xp into 4.
        let gold_count = ((b.gold as f64 / 4.0).round() as i64).clamp(2, 4);
        let mut r = || rng.unit();
        let golds = orb::spawn_orbs(OrbKind::Gold, x, y, z, gold_count, b.gold, &mut r);
        let xps = orb::spawn_orbs(OrbKind::Xp, x, y, z, 4, b.xp, &mut r);
        for o in golds.into_iter().chain(xps) {
            let mat = if o.kind == OrbKind::Gold { assets.gold.clone() } else { assets.xp.clone() };
            commands.spawn((
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(mat),
                Transform::from_xyz(o.x as f32, o.y as f32, o.z as f32),
                RewardOrb(o),
                bevy::light::NotShadowCaster,
            ));
        }
    }
}

/// Advance each orb toward the hero; bank its value (gold→purse, xp→bar) on contact.
fn step_reward_orbs(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut player: ResMut<PlayerRes>,
    mut q: Query<(Entity, &mut RewardOrb, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs() as f64;
    if dt <= 0.0 {
        return; // world fully paused (panel/menu) — orbs hold. Hit-stop only slows dt, not zeroes it.
    }
    let pose = PlayerPose { x: hero.pos.x as f64, y: (hero.y + 1.0) as f64, z: hero.pos.y as f64 };
    for (e, mut ro, mut tf) in &mut q {
        match orb::step_orb(&mut ro.0, &pose, dt) {
            OrbStep::Collected => {
                match ro.0.kind {
                    OrbKind::Gold => player.0.add_gold(ro.0.value),
                    OrbKind::Xp => player.0.add_xp(ro.0.value),
                }
                commands.entity(e).despawn();
            }
            OrbStep::Flying => {
                tf.translation = Vec3::new(ro.0.x as f32, ro.0.y as f32, ro.0.z as f32);
            }
        }
    }
}
