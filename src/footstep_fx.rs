//! **Footstep feedback** — the visual twin of [`audio::footsteps`](crate::audio). Each footfall
//! (same `walk_phase` half-cycle gate the audio uses) kicks up a small puff of motes, tinted by
//! the surface underfoot (snow / stone / dirt). Stepping over the river swaps the puff for a
//! splash + an expanding ripple ring. Pure spawn-and-fade — the world finally answers your feet.
//!
//! Fades run **ungated** (like the combat sparks) so a frozen scene still settles its dust; the
//! emitter is gated on `Modal::None` so panels/pauses don't spawn steps.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::Biome;
use crate::player::Hero;
use crate::worldmap;
use crate::game_state::SimAppExt;

/// Downward pull on settling dust motes.
const GRAV: f32 = 6.0;
/// A drop shorter than this lands clean — no dust kick. (Below `movement::FALL_SAFE`, so even a
/// painless hop still puffs, but a flat-ground micro-bounce doesn't.)
const LAND_MIN_FALL: f32 = 0.6;

/// A kicked-up dust/splash mote — shrinks to nothing over its life (shared material, no per-mote
/// alpha, so thousands still batch).
#[derive(Component)]
struct Puff {
    vel: Vec3,
    life: f32,
    life0: f32,
    scale0: f32,
}

/// An expanding water ripple ring — grows + alpha-fades. Owns its material (cloned per spawn) so
/// the alpha fade is per-ring; the handle is freed on death to avoid leaking assets.
#[derive(Component)]
struct Ripple {
    life: f32,
    life0: f32,
    scale0: f32,
    scale1: f32,
    alpha0: f32,
    mat: Handle<StandardMaterial>,
}

/// Shared footstep-fx assets, built once. `puff` + `mortar` are crate-visible so
/// [`build_fx`](crate::build_fx) can reuse the mote pipeline for construction dust.
#[derive(Resource)]
pub(crate) struct FxAssets {
    pub(crate) puff: Handle<Mesh>,
    ring: Handle<Mesh>,
    dirt: Handle<StandardMaterial>,
    snow: Handle<StandardMaterial>,
    stone: Handle<StandardMaterial>,
    splash: Handle<StandardMaterial>,
    /// Pale stone/mortar dust — construction bursts (`build_fx`), not footfalls.
    pub(crate) mortar: Handle<StandardMaterial>,
}

pub struct FootstepFxPlugin;

impl Plugin for FootstepFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_sim_systems(emit)
            // Fades are visual — keep settling even while frozen.
            .add_systems(Update, (fade_puffs, fade_ripples));
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mk = |c: Color, a: f32| {
        materials.add(StandardMaterial {
            base_color: c.with_alpha(a),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        })
    };
    commands.insert_resource(FxAssets {
        puff: meshes.add(Sphere::new(0.12).mesh().ico(1).unwrap()),
        // Authored flat in XY (z=0) — laid into the ground plane with a -90° X tilt at spawn.
        ring: meshes.add(Annulus::new(0.5, 0.62).mesh().resolution(24).build()),
        dirt: mk(Color::srgb(0.62, 0.50, 0.34), 0.7),
        snow: mk(Color::srgb(0.95, 0.97, 1.0), 0.85),
        stone: mk(Color::srgb(0.62, 0.62, 0.66), 0.7),
        splash: mk(Color::srgb(0.70, 0.85, 1.0), 0.8),
        mortar: mk(Color::srgb(0.74, 0.68, 0.56), 0.75),
    });
}

/// Surface dust material under a world XZ — mirrors `audio::footsteps::surface_for`.
fn surf_mat(fx: &FxAssets, pos: Vec2) -> Handle<StandardMaterial> {
    match worldmap::biome_at_world(pos.x, pos.y) {
        Some(Biome::Snow) => fx.snow.clone(),
        Some(Biome::Rocky) => fx.stone.clone(),
        _ => fx.dirt.clone(),
    }
}

/// On each gait half-cycle (a footfall), kick up a puff — or a splash + ripple over the river.
/// Also two non-footfall beats: a fatter **landing** burst on touchdown after a real drop, and
/// **sprint** footfalls that throw more, faster dust trailing behind the heading.
fn emit(
    mut last_half: Local<i64>,
    mut last_ground: Local<bool>,
    mut commands: Commands,
    fx: Option<Res<FxAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    hero_q: Query<&Hero>,
) {
    let (Some(fx), Ok(hero)) = (fx, hero_q.single()) else {
        return;
    };

    // ── Landing kick: on the airborne→grounded edge, a drop above the threshold thumps up dust,
    // scaled by fall height. (Runs before the footfall gate so a jump that lands while standing
    // still still puffs.) At startup `air_takeoff_y == y`, so the fall is 0 and nothing fires.
    if hero.on_ground && !*last_ground {
        let fall = hero.air_takeoff_y - hero.y;
        if fall > LAND_MIN_FALL {
            let k = ((fall - LAND_MIN_FALL) / 3.0).clamp(0.0, 1.0);
            let mat = surf_mat(&fx, hero.pos);
            let feet = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
            spawn_puffs(&mut commands, &fx.puff, &mat, feet, 7 + (k * 9.0) as u32, 1.4 + k * 1.8, 0.9 + k * 0.4, 0.0);
        }
    }
    *last_ground = hero.on_ground;

    let half = (hero.walk_phase / PI).floor() as i64;
    if !(hero.moving && hero.on_ground) {
        *last_half = half; // idle/airborne: stay current so resuming doesn't fire a stale puff
        return;
    }
    if half == *last_half {
        return;
    }
    *last_half = half;

    let p = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
    if worldmap::is_river_world(hero.pos.x, hero.pos.y) {
        spawn_puffs(&mut commands, &fx.puff, &fx.splash, p, 6, 1.6, 0.9, 0.0);
        let mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.9, 1.0).with_alpha(0.5),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        commands.spawn((
            Mesh3d(fx.ring.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(p + Vec3::Y * 0.04)
                .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                .with_scale(Vec3::splat(0.3)),
            Ripple { life: 0.55, life0: 0.55, scale0: 0.3, scale1: 2.6, alpha0: 0.5, mat },
            NotShadowCaster,
        ));
    } else {
        let mat = surf_mat(&fx, hero.pos);
        // Sprinting (run_amt → 1) throws more, faster dust, kicked back behind the heading so the
        // trail reads as churned-up sprint. A walk (run_amt ≈ 0) is unchanged (5 motes at 1.1).
        let run = hero.run_amt.clamp(0.0, 1.0);
        // Kick the dust back along the actual TRAVEL (velocity), not the facing — in the combat
        // stance the two differ (body squared to the foe while strafing), and a facing-keyed kick
        // would trail dust sideways off a backpedal.
        let fwd = hero.vel.normalize_or_zero();
        let kick = p - Vec3::new(fwd.x, 0.0, fwd.y) * (0.22 * run);
        // Toned down (was 5 + run*5 motes @ 0.8 + run*0.3): a walk kicked up too much dust. Now
        // 3 motes @ 0.65 for a walk, ramping to 6 @ 0.9 at a full sprint.
        spawn_puffs(&mut commands, &fx.puff, &mat, kick, 3 + (run * 3.0) as u32, 1.1 + run * 1.6, 0.65 + run * 0.25, 0.0);
    }
}

/// Fling `n` motes outward + up from `at` (golden-angle spread), each shrinking over its life.
/// `radius` offsets each mote's start along its own fling direction — 0 for footfalls (open
/// ground under the hero), the footprint half-width for construction dust (motes born at a
/// building's centre die unseen inside its mesh).
/// Crate-visible: `build_fx` borrows it for construction dust ([`fade_puffs`] settles those too).
pub(crate) fn spawn_puffs(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    at: Vec3,
    n: u32,
    spd: f32,
    scale0: f32,
    radius: f32,
) {
    for i in 0..n {
        let a = i as f32 * 2.399_963_2; // golden angle
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let mag = 0.5 + (i % 5) as f32 * 0.12;
        let vel = dir * spd * mag + Vec3::Y * (0.6 + (i % 3) as f32 * 0.25);
        let life = 0.4 + (i % 3) as f32 * 0.06;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(at + dir * radius + Vec3::Y * 0.08)
                .with_scale(Vec3::splat(scale0)),
            Puff { vel, life, life0: life, scale0 },
            NotShadowCaster,
        ));
    }
}

/// Drift + settle each puff, shrink to zero, reap at end of life.
fn fade_puffs(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Puff, &mut Transform)>) {
    let dt = time.delta_secs();
    for (e, mut pf, mut tf) in &mut q {
        pf.life -= dt;
        if pf.life <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        pf.vel.y -= GRAV * dt;
        let v = pf.vel;
        tf.translation += v * dt;
        let t = (pf.life / pf.life0).clamp(0.0, 1.0);
        tf.scale = Vec3::splat(pf.scale0 * t);
    }
}

/// Grow each ripple outward, fade its (owned) material's alpha, free the material + reap at end.
fn fade_ripples(
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Ripple, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut r, mut tf) in &mut q {
        r.life -= dt;
        let t = (r.life / r.life0).clamp(0.0, 1.0); // 1 → 0
        let grow = 1.0 - t;
        tf.scale = Vec3::splat(r.scale0 + (r.scale1 - r.scale0) * grow);
        if let Some(mut m) = materials.get_mut(&r.mat) {
            let c = m.base_color;
            m.base_color = c.with_alpha(r.alpha0 * t);
        }
        if r.life <= 0.0 {
            materials.remove(&r.mat);
            commands.entity(e).try_despawn();
        }
    }
}
