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
const BUTTERFLY_COUNT: usize = 2;
/// How fast a butterfly fades in/out (alpha units per second) as it enters/leaves green daytime
/// country. Low → a soft ~1.5s dissolve, never a pop.
const FADE_RATE: f32 = 0.7;
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
    /// Current opacity 0..1, eased toward 1 when it should be out (day + green country) and 0 when
    /// not, so it dissolves in/out instead of popping. Drives the wing/body material alpha.
    fade: f32,
    /// This butterfly's own [wing, hind, body] materials, so its alpha can be faded independently.
    mats: [Handle<StandardMaterial>; 3],
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
    hind_mat: &Handle<StandardMaterial>,
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
            Butterfly {
                phase,
                home_r,
                fade: 0.0, // dissolves in on its first day over green country
                mats: [wing_mat.clone(), hind_mat.clone(), body_mat.clone()],
            },
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
            use core::f32::consts::FRAC_PI_2;
            for side in [-1.0_f32, 1.0] {
                // Lay each wing FLAT (ellipse faces +Z → rotate -90° about X so it lies horizontal,
                // face up), then YAW it about vertical so the four wings splay into a butterfly
                // X-silhouette — forewing points forward-out, hindwing back-out — instead of merging
                // into one oval. The flap raises them into a V about the body's fore-aft axis
                // (`flap_wings`), so from above you read the wing FACES, never an edge-on sliver.
                let flat = Quat::from_rotation_x(-FRAC_PI_2);
                let base_fore = Quat::from_rotation_y(side * 0.5) * flat; // forward-out
                let base_hind = Quat::from_rotation_y(side * -0.55) * flat; // back-out
                // Forewing — larger, set forward + outward.
                p.spawn((
                    Mesh3d(forewing.clone()),
                    MeshMaterial3d(wing_mat.clone()),
                    Transform {
                        translation: Vec3::new(side * 0.11, 0.0, 0.075),
                        rotation: base_fore,
                        ..default()
                    },
                    Wing { side, base: base_fore, rate, phase },
                    bevy::light::NotShadowCaster,
                ));
                // Hindwing — smaller, darker, set back + outward.
                p.spawn((
                    Mesh3d(hindwing.clone()),
                    MeshMaterial3d(hind_mat.clone()),
                    Transform {
                        translation: Vec3::new(side * 0.09, -0.006, -0.08),
                        rotation: base_hind,
                        ..default()
                    },
                    Wing { side, base: base_hind, rate, phase },
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

    // ROUNDED wings (ellipses, not paper rectangles) — a wide forewing + a smaller hindwing read as
    // a butterfly silhouette instead of a flat scrap. `Ellipse::new(half_w, half_h)`.
    let forewing = meshes.add(Ellipse::new(0.10, 0.060)); // wide, shallow forewing (~30% smaller)
    let hindwing = meshes.add(Ellipse::new(0.060, 0.072)); // rounder, longer hindwing
    let body = meshes.add(Cuboid::new(0.014, 0.014, 0.11));

    // Saturated, recognisable butterfly colours — NOT washed-out pastels (which read as flat white
    // under the unlit midday sun). Monarch orange, sulphur yellow, sky-blue, cabbage-white, dusky
    // rose, pale green.
    let wing_cols = [0xd9772b, 0xe6b829, 0x4f86c6, 0xe8e8ea, 0xcf6f86, 0x8fb45a];
    for i in 0..BUTTERFLY_COUNT {
        // Per-instance body material so each butterfly's alpha can fade independently. `Blend` so
        // the dissolve in/out actually shows (see `fade_butterflies`); start fully transparent.
        let body_mat = mats.add(StandardMaterial {
            base_color: crate::palette::srgb(0x241f19).with_alpha(0.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let c = crate::palette::srgb(wing_cols[i % wing_cols.len()]).with_alpha(0.0);
        let wing_mat = mats.add(StandardMaterial {
            base_color: c,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None, // two-sided so a wing shows from either face
            ..default()
        });
        // Hindwing a shade darker than the forewing → a hint of two-tone depth, less flat.
        let h = crate::palette::lin_scaled(wing_cols[i % wing_cols.len()], 0.7);
        let hc = Color::linear_rgba(h[0], h[1], h[2], 0.0);
        let hind_mat = mats.add(StandardMaterial {
            base_color: hc,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        let phase = i as f32 * 2.39996; // golden-angle spread
        // Stable scattered territory radius from the phase: 3..TERRITORY_R.
        let home_r = 3.0 + (phase * 1.3).sin().abs() * (TERRITORY_R - 3.0);
        spawn_butterfly(&mut commands, &forewing, &hindwing, &body, &wing_mat, &hind_mat, &body_mat, phase, home_r);
    }
}

/// Drift each butterfly's small territory slowly around the hero and flutter it locally within that
/// patch — low to the ground, gentle bob. Hide them at night / off green country.
fn fly_butterflies(
    time: Res<Time>,
    hero: Option<Res<HeroState>>,
    gt: Option<Res<GameTime>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Butterfly, &mut Transform, &mut Visibility)>,
) {
    let (Some(hero), Some(gt)) = (hero, gt) else { return };
    let t = time.elapsed_secs_wrapped();
    let dt = time.delta_secs();
    let hx = hero.pos.x;
    let hz = hero.pos.y;
    let here_green = crate::worldmap::is_grass_world(hx, hz)
        || crate::worldmap::tile_biome_world(hx, hz) == Some(crate::biome::Biome::Forest);
    let show = is_day(&gt) && here_green && hero.alive;

    for (mut b, mut tf, mut vis) in &mut q {
        // Ease opacity toward the target (1 out / 0 gone) so they dissolve in and out, never pop.
        let target = if show { 1.0 } else { 0.0 };
        b.fade += (target - b.fade).clamp(-FADE_RATE * dt, FADE_RATE * dt);
        // Push the eased alpha into this butterfly's own wing/hind/body materials.
        for h in &b.mats {
            if let Some(mut m) = mats.get_mut(h) {
                // AssetMut only marks the material modified when we write it.
                if m.base_color.alpha() != b.fade {
                    m.base_color.set_alpha(b.fade);
                }
            }
        }
        if b.fade <= 0.001 {
            // Fully gone — hide to skip drawing, and don't bother moving it.
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
        // Raise the flat wing into a shallow V about the body's fore-aft axis (parent-space Z), so
        // it claps between ~9° (nearly flat, full face) and ~40° up — gentle flutter, never edge-on.
        let osc = (t * w.rate + w.phase).sin() * 0.5 + 0.5; // 0..1
        let dihedral = 0.35 + osc * 0.4; // ~20°..43° up — clear butterfly V, never edge-on
        tf.rotation = Quat::from_rotation_z(w.side * dihedral) * w.base;
    }
}
