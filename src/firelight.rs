//! **Dynamic fire-light** — a pooled, flickering [`PointLight`] for diegetic flame.
//!
//! Camp campfires (`camps.rs`) and castle/gate torches (`castle.rs`) were emissive-only: the
//! flame mesh glowed but cast no light, so a night camp out in a dark biome read as a faint
//! orange speck instead of a pool you can navigate toward. This module adds one warm point
//! light per fire, riding a [`FireLight`] marker that [`flicker_fire_lights`] animates: the
//! intensity jitters on a per-source phase (so neighbouring fires don't pulse in lockstep) and
//! **ramps with nightfall** — a faint ember by day, full pool after dark — so the lights cost
//! nothing visually in daylight and make the camps + keep gates read from across the map at
//! night.
//!
//! Spawn-side wiring lives with each fire's owner ([`campfire_light`] for camps, [`torch_light`]
//! for the castle), so those plugins keep owning their own entities (and the lights inherit the
//! fire's `BiomeEntity` / `CastlePart` lifecycle — camp lights rebuild on a biome swap, gate
//! torches light up only once their gate is built). This plugin owns just the shared flicker.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::scene::{self, SkyClock};

/// Warm fire-light tint (orange), matching the emissive flame colour the lights sit inside.
pub const FIRE_COLOR: Color = Color::srgb(1.0, 0.55, 0.22);

/// Marks a [`PointLight`] as a flickering fire. `base` is its peak (full-night) intensity;
/// `phase` desyncs the flicker between sources.
#[derive(Component)]
pub struct FireLight {
    pub phase: f32,
    pub base: f32,
    /// `elapsed_secs` at which this fire next spits an ember ([`spawn_embers`]). 0 = ready now;
    /// each emit reschedules it with a phase-jittered interval so neighbouring fires don't puff in
    /// lockstep.
    pub next_ember: f32,
}

/// A campfire's point light + flicker marker (big warm pool — camps sit in open dark biomes).
/// Returned as a bundle so the caller can fold it straight into the flame's spawn tuple.
pub fn campfire_light(phase: f32) -> (PointLight, FireLight) {
    const BASE: f32 = 85_000.0;
    (
        PointLight {
            color: FIRE_COLOR,
            intensity: BASE,
            range: 16.0,
            radius: 0.3,
            shadow_maps_enabled: false, // many fires on the map — keep them cheap
            ..default()
        },
        FireLight { phase, base: BASE, next_ember: 0.0 },
    )
}

/// A wall/gate torch's point light + flicker marker (smaller, tighter pool than a campfire).
pub fn torch_light(phase: f32) -> (PointLight, FireLight) {
    const BASE: f32 = 17_000.0;
    (
        PointLight {
            color: FIRE_COLOR,
            intensity: BASE,
            range: 8.0,
            radius: 0.12,
            shadow_maps_enabled: false,
            ..default()
        },
        FireLight { phase, base: BASE, next_ember: 0.0 },
    )
}

/// A war-torch carried by a marching ork (`orks.rs`): the tightest, cheapest pool of the family —
/// there can be a dozen live at once mid-siege, and its job is to paint the BEARER and his
/// neighbours so the horde reads as figures at night instead of black cutouts.
pub fn held_torch_light(phase: f32) -> (PointLight, FireLight) {
    const BASE: f32 = 9_500.0;
    (
        PointLight {
            color: FIRE_COLOR,
            intensity: BASE,
            range: 6.5,
            radius: 0.08,
            shadow_maps_enabled: false,
            ..default()
        },
        FireLight { phase, base: BASE, next_ember: 0.0 },
    )
}

/// A gate war-brazier (`castle.rs`): a campfire-class pool aimed at the gate APPROACH — the siege
/// kill-zone — so the night melee happens in warm light instead of the moon-dark mush it read as
/// on capture footage. Between torch and campfire in size.
pub fn brazier_light(phase: f32) -> (PointLight, FireLight) {
    const BASE: f32 = 36_000.0;
    (
        PointLight {
            color: FIRE_COLOR,
            intensity: BASE,
            range: 14.0,
            radius: 0.25,
            shadow_maps_enabled: false,
            ..default()
        },
        FireLight { phase, base: BASE, next_ember: 0.0 },
    )
}

pub struct FireLightPlugin;

impl Plugin for FireLightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_embers)
            // Ungated — lighting keeps breathing while a panel freezes the sim (like the flame mesh
            // flicker it accompanies). The ember *drift* rides ungated too; only the *spawn* is
            // gated so a paused night doesn't accrue motes.
            .add_systems(Update, (flicker_fire_lights, drift_embers))
            .add_systems(
                Update,
                spawn_embers.run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

// ── Rising embers ────────────────────────────────────────────────────────────────────────────────
// Every fire spits hot embers/sparks that float up into the smoke column, flicker and wink out —
// the diegetic twin of the point-light pool, so a camp reads as a living fire by day and night.

/// A single rising ember mote: floats up (no gravity — embers rise), shrinks + flickers over its
/// short life, then despawns. Shared material, so thousands still batch.
#[derive(Component)]
pub(crate) struct Ember {
    vel: Vec3,
    life: f32,
    life0: f32,
    scale0: f32,
}

/// Shared ember mesh + warm-emissive material, built once.
#[derive(Resource)]
struct EmberAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

fn setup_embers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Hot ember: a tiny soft round sprite (camera-facing), additive, HDR orange so the bloom kisses
    // it into a glowing speck.
    let mesh = meshes.add(crate::particles::soft_disc_mesh().scaled_by(Vec3::splat(0.135)));
    let mat = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::rgb(14.0, 4.8, 0.85)),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(EmberAssets { mesh, mat });
}

/// Deterministic hash → [0,1) from a float seed (per-fire / per-emit jitter without RNG state).
fn hash01(x: f32) -> f32 {
    let v = (x * 12.9898).sin() * 43758.547;
    v - v.floor()
}

/// Each fire emits rising sparks/embers whenever its `next_ember` deadline passes, then reschedules
/// with a phase-jittered interval (so fires don't pulse in unison).
fn spawn_embers(
    time: Res<Time>,
    clock: Option<Res<SkyClock>>,
    assets: Option<Res<EmberAssets>>,
    mut commands: Commands,
    mut fires: Query<(&mut FireLight, &GlobalTransform)>,
) {
    let Some(assets) = assets else { return };
    if clock.is_none() {
        return;
    }
    let night = clock.map(|c| scene::night_of(c.t)).unwrap_or(0.0);
    let now = time.elapsed_secs();
    for (mut fl, gt) in &mut fires {
        if now < fl.next_ember {
            continue;
        }
        // Campfires (`base >= 60_000`) throw a rich stream of sparks both day and night; smaller
        // torches spit more gently by day.
        let is_campfire = fl.base >= 60_000.0;
        if !is_campfire && night < 0.15 {
            continue;
        }
        let interval = if is_campfire {
            (0.08 - 0.03 * night) * (0.6 + 0.8 * hash01(fl.phase + now))
        } else {
            (0.7 - 0.4 * night) * (0.6 + 0.8 * hash01(fl.phase + now))
        };
        fl.next_ember = now + interval.max(0.04);

        let count = if is_campfire { 3 } else { 1 };
        for k in 0..count {
            let j = fl.phase + now + k as f32 * 19.3;
            let base = gt.translation()
                + Vec3::new((hash01(j + 2.1) - 0.5) * 0.55, 0.22 + hash01(j + 4.7) * 0.55, (hash01(j + 6.3) - 0.5) * 0.55);
            let drift = Vec3::new((hash01(j) - 0.5) * 0.95, 0.0, (hash01(j + 3.1) - 0.5) * 0.95);
            let up = 1.15 + hash01(j + 7.7) * 1.45;
            let sc0 = 0.52 + hash01(j + 1.3) * 0.72;
            let life0 = 1.8 + hash01(j + 9.1) * 0.9;
            commands.spawn((
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.mat.clone()),
                Transform::from_translation(base).with_scale(Vec3::splat(sc0)),
                Ember { vel: drift + Vec3::Y * up, life: life0, life0, scale0: sc0 },
                crate::particles::FaceCamera,
                NotShadowCaster,
            ));
        }
    }
}

/// Float each ember up, decelerate the sideways drift, flicker + shrink over its life, then reap.
fn drift_embers(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Ember, &mut Transform)>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (e, mut em, mut tf) in &mut q {
        em.life -= dt;
        if em.life <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        tf.translation += em.vel * dt;
        // Bleed the sideways drift but keep a touch of buoyancy so it keeps rising as it fades.
        em.vel.x *= 1.0 - 1.1 * dt;
        em.vel.z *= 1.0 - 1.1 * dt;
        em.vel.y += 0.30 * dt;
        let k = em.life / em.life0;
        let flick = 0.80 + 0.20 * (t * 26.0 + em.scale0 * 30.0).sin();
        tf.scale = Vec3::splat(em.scale0 * k * flick);
    }
}

/// Jitter each fire light's intensity and ramp it with nightfall.
fn flicker_fire_lights(
    time: Res<Time>,
    clock: Option<Res<SkyClock>>,
    mut dark_at: Local<Option<f32>>,
    mut q: Query<(&FireLight, &mut PointLight)>,
) {
    let t = time.elapsed_secs();
    // Warm pool around campfires by day too so drums, logs, and trampled earth around the firepit
    // catch a diegetic orange glow; full intensity after dark.
    let night = clock.map(|c| scene::night_of(c.t)).unwrap_or(1.0);
    let ramp = 0.58 + 0.42 * night;
    // Nightfall flare: the moment full dark lands, every fire ROARS up (~+55%) and settles back
    // over a few seconds — the torches answering the night, part of the nightfall beat. The
    // timestamp arms on the day→dark edge and clears when day returns.
    if night > 0.92 {
        if dark_at.is_none() {
            *dark_at = Some(t);
        }
    } else if night < 0.5 {
        *dark_at = None;
    }
    let flare = dark_at.map_or(0.0, |t0| {
        let dt = t - t0;
        ((dt / 0.4).min(1.0)) * (-dt / 2.8).exp() // quick catch, slow settle
    });
    for (fl, mut light) in &mut q {
        // Two desynced waves → a restless, non-periodic flicker (mirrors `camps::flicker_flames`).
        let flick = 1.0
            + (t * 7.3 + fl.phase).sin() * 0.12
            + (t * 15.0 + fl.phase * 1.7).sin() * 0.06;
        light.intensity = fl.base * ramp * flick * (1.0 + 0.55 * flare);
    }
}
