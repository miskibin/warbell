//! Ambient daytime life — butterflies near the ground in green country. Pure visual charm: no
//! simulation, no save/reset (like the weather motes and fireflies, it's transient atmosphere). A
//! small self-contained plugin that spawns a fixed pool once the hero exists, then each frame moves
//! it to follow the hero, flaps wings, and toggles visibility by time-of-day + biome.
//!
//! Design (deliberately SUBTLE — they should be barely-noticed life, not neon confetti):
//! - Few, small, soft-pastel butterflies (not bright saturated cards).
//! - Each holds a small drifting *territory* near the hero and **flutters in place** within it —
//!   gentle local jitter, NOT the wide screen-crossing sweeps of the first attempt.
//! - A real butterfly silhouette: a tiny dark body with paired fore + hind wings that clap.
//!
//! Gating:
//! - **Day only** (`scene::night_of` from the siege clock) — butterflies sleep at night.
//! - Shown only over **green country** (grass frontier + forest).
//! - Movement/animation are visual, so they keep running ungated; only the spawn-follow read of the
//!   hero is gated to `Modal::None` so it doesn't chase a frozen world through panels.

use bevy::prelude::*;

use crate::player::HeroState;
use crate::siege::GameTime;

/// How many butterflies wander near the hero. Kept low — a few drifting specks, not a swarm.
const BUTTERFLY_COUNT: usize = 7;
/// Max horizontal distance (world units) a butterfly's territory drifts from the hero. Each holds a
/// scattered spot in this radius and flutters locally around it — they don't all cluster underfoot.
const TERRITORY_R: f32 = 11.0;
/// Below this `night_of` value it's "day" — life is out.
const DAY_MAX: f32 = 0.35;

#[derive(Component)]
struct Butterfly {
    /// Per-instance phase so each wanders/flaps out of step.
    phase: f32,
    /// Stable scattered territory radius (3..TERRITORY_R), so each keeps its own patch of air.
    home_r: f32,
}

/// A flapping wing child: `side` (−1 left / +1 right) and its rest rotation, about which the flap
/// opens/closes. `rate`/`phase` drive the flap speed.
#[derive(Component)]
struct Wing {
    side: f32,
    base: Quat,
    rate: f32,
    phase: f32,
}

/// Marks the spawn done so the run-once setup doesn't re-fire.
#[derive(Resource, Default)]
struct AmbientSpawned(bool);

pub struct AmbientLifePlugin;

impl Plugin for AmbientLifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AmbientSpawned>().add_systems(
            Update,
            (
                spawn_once.run_if(in_state(crate::game_state::Modal::None)),
                fly_butterflies,
                flap_wings,
            ),
        );
    }
}

/// True when the siege clock reads daytime.
fn is_day(gt: &GameTime) -> bool {
    crate::scene::night_of(gt.0) < DAY_MAX
}

/// A small butterfly: a tiny dark body with paired fore + hind wings that flap together about the
/// body centreline. Spawned parked offscreen + hidden until the first `fly_` update places it.
#[allow(clippy::too_many_arguments)]
fn spawn_butterfly(
    commands: &mut Commands,
    forewing: &Handle<Mesh>,
    hindwing: &Handle<Mesh>,
    body: &Handle<Mesh>,
    wing_mat: &Handle<StandardMaterial>,
    body_mat: &Handle<StandardMaterial>,
    phase: f32,
    home_r: f32,
) {
    // Flap a hair off the per-instance phase + a small rate jitter so they don't beat in unison.
    let rate = 11.0 + (phase * 1.7).sin() * 2.5;
    commands
        .spawn((
            Transform::from_xyz(0.0, -100.0, 0.0), // parked offscreen until first fly_ update
            Visibility::Hidden,
            Butterfly { phase, home_r },
            bevy::light::NotShadowCaster,
        ))
        .with_children(|p| {
            // Slim dark body along the travel axis (Z), so the wings read as flanking a thorax.
            p.spawn((
                Mesh3d(body.clone()),
                MeshMaterial3d(body_mat.clone()),
                Transform::from_xyz(0.0, 0.0, 0.0),
                bevy::light::NotShadowCaster,
            ));
            for side in [-1.0_f32, 1.0] {
                // Wings hinge at the body centreline, tilted up into a shallow V at rest.
                let base = Quat::from_rotation_z(side * 0.4);
                // Forewing — larger, set slightly forward.
                p.spawn((
                    Mesh3d(forewing.clone()),
                    MeshMaterial3d(wing_mat.clone()),
                    Transform {
                        translation: Vec3::new(side * 0.05, 0.0, 0.04),
                        rotation: base,
                        ..default()
                    },
                    Wing { side, base, rate, phase },
                    bevy::light::NotShadowCaster,
                ));
                // Hindwing — smaller, set back.
                p.spawn((
                    Mesh3d(hindwing.clone()),
                    MeshMaterial3d(wing_mat.clone()),
                    Transform {
                        translation: Vec3::new(side * 0.045, -0.01, -0.05),
                        rotation: base,
                        ..default()
                    },
                    Wing { side, base, rate, phase },
                    bevy::light::NotShadowCaster,
                ));
            }
        });
}

/// Spawn the pool once the hero (and so the world) exists.
fn spawn_once(
    mut commands: Commands,
    mut done: ResMut<AmbientSpawned>,
    hero: Option<Res<HeroState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if done.0 || hero.is_none() {
        return;
    }
    done.0 = true;

    // Small wing quads + a slim body. Forewing slightly larger than the hindwing.
    let forewing = meshes.add(Rectangle::new(0.11, 0.13));
    let hindwing = meshes.add(Rectangle::new(0.085, 0.085));
    let body = meshes.add(Cuboid::new(0.022, 0.022, 0.16));

    // Soft, DESATURATED pastels — gentle flecks against the grass, not bright confetti.
    let wing_cols = [0xf1ead4, 0xeadfa6, 0xd9e0ea, 0xe7cbb0, 0xddd6e6, 0xd4e0c6];
    // One shared dark body material for every butterfly.
    let body_mat = mats.add(StandardMaterial {
        base_color: crate::palette::srgb(0x2b2620),
        unlit: true,
        ..default()
    });
    for i in 0..BUTTERFLY_COUNT {
        let c = crate::palette::srgb(wing_cols[i % wing_cols.len()]);
        let wing_mat = mats.add(StandardMaterial {
            base_color: c,
            unlit: true,
            cull_mode: None, // two-sided so a wing shows from either face
            ..default()
        });
        let phase = i as f32 * 2.39996; // golden-angle spread
        // Stable scattered territory radius from the phase: 3..TERRITORY_R.
        let home_r = 3.0 + (phase * 1.3).sin().abs() * (TERRITORY_R - 3.0);
        spawn_butterfly(&mut commands, &forewing, &hindwing, &body, &wing_mat, &body_mat, phase, home_r);
    }
}

/// Drift each butterfly's small territory slowly around the hero and flutter it locally within that
/// patch — low to the ground, gentle bob. Hide them at night / off green country.
fn fly_butterflies(
    time: Res<Time>,
    hero: Option<Res<HeroState>>,
    gt: Option<Res<GameTime>>,
    mut q: Query<(&Butterfly, &mut Transform, &mut Visibility)>,
) {
    let (Some(hero), Some(gt)) = (hero, gt) else { return };
    let t = time.elapsed_secs_wrapped();
    let hx = hero.pos.x;
    let hz = hero.pos.y;
    let here_green = crate::worldmap::is_grass_world(hx, hz)
        || crate::worldmap::tile_biome_world(hx, hz) == Some(crate::biome::Biome::Forest);
    let show = is_day(&gt) && here_green && hero.alive;

    for (b, mut tf, mut vis) in &mut q {
        if !show {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Visible;
        let p = b.phase;
        // The territory: a scattered spot that orbits the hero VERY slowly (it drifts, doesn't dart).
        let home_ang = t * 0.05 + p;
        let cx = hx + home_ang.cos() * b.home_r;
        let cz = hz + home_ang.sin() * b.home_r;
        // Local flutter: small erratic jitter so it reads as fluttering in place, not zipping across
        // the field. Amplitudes are ~1 unit, NOT the old 14-unit sweeps.
        let fx = (t * 1.6 + p * 2.0).sin() * 0.7 + (t * 0.9 + p).cos() * 0.35;
        let fz = (t * 1.4 + p * 3.1).cos() * 0.7 + (t * 1.1 + p * 1.5).sin() * 0.35;
        let x = cx + fx;
        let z = cz + fz;
        let ground = crate::worldmap::ground_at_world(x, z).unwrap_or(hero.y);
        // Low over the grass with a soft bob.
        let y = ground + 0.5 + (t * 2.4 + p).sin() * 0.16 + (t * 3.3 + p * 2.0).sin() * 0.08;
        let prev = tf.translation;
        tf.translation = Vec3::new(x, y, z);
        // Face the direction of travel (yaw only), so the wings lead.
        let d = tf.translation - prev;
        if d.x.abs() + d.z.abs() > 1e-4 {
            tf.rotation = Quat::from_rotation_y(d.x.atan2(d.z));
        }
    }
}

/// Flap every wing about its rest rotation: open/close around the hinge at the wing's rate.
fn flap_wings(time: Res<Time>, mut q: Query<(&Wing, &mut Transform)>) {
    let t = time.elapsed_secs_wrapped();
    for (w, mut tf) in &mut q {
        // Bias the clap upward (rest at a shallow V, snapping up toward a vertical clap).
        let flap = (t * w.rate + w.phase).sin() * 0.55 + 0.35;
        tf.rotation = w.base * Quat::from_rotation_z(w.side * flap);
    }
}
