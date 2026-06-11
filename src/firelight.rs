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
}

/// A campfire's point light + flicker marker (big warm pool — camps sit in open dark biomes).
/// Returned as a bundle so the caller can fold it straight into the flame's spawn tuple.
pub fn campfire_light(phase: f32) -> (PointLight, FireLight) {
    const BASE: f32 = 52_000.0;
    (
        PointLight {
            color: FIRE_COLOR,
            intensity: BASE,
            range: 16.0,
            radius: 0.3,
            shadows_enabled: false, // many fires on the map — keep them cheap
            ..default()
        },
        FireLight { phase, base: BASE },
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
            shadows_enabled: false,
            ..default()
        },
        FireLight { phase, base: BASE },
    )
}

pub struct FireLightPlugin;

impl Plugin for FireLightPlugin {
    fn build(&self, app: &mut App) {
        // Ungated — lighting keeps breathing while a panel freezes the sim (like the flame mesh
        // flicker it accompanies).
        app.add_systems(Update, flicker_fire_lights);
    }
}

/// Jitter each fire light's intensity and ramp it with nightfall.
fn flicker_fire_lights(
    time: Res<Time>,
    clock: Option<Res<SkyClock>>,
    mut q: Query<(&FireLight, &mut PointLight)>,
) {
    let t = time.elapsed_secs();
    // Faint by day, full after dark — the "reads from a distance at night" the lights are for.
    // No clock yet (e.g. start screen, before the world is built) → treat the fire as lit.
    let night = clock.map(|c| scene::night_of(c.t)).unwrap_or(1.0);
    let ramp = 0.12 + 0.88 * night;
    for (fl, mut light) in &mut q {
        // Two desynced waves → a restless, non-periodic flicker (mirrors `camps::flicker_flames`).
        let flick = 1.0
            + (t * 7.3 + fl.phase).sin() * 0.12
            + (t * 15.0 + fl.phase * 1.7).sin() * 0.06;
        light.intensity = fl.base * ramp * flick;
    }
}
