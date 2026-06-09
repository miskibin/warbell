//! Animated water material + the river-centerline placement queries.
//!
//! The surface is an `ExtendedMaterial<StandardMaterial, WaterExt>` whose FRAGMENT shader
//! (`assets/shaders/water.wgsl`) perturbs the lighting normal with scrolling ripple waves:
//! because the base StandardMaterial has tiny roughness + high reflectance, that moving
//! normal makes it reflect the Atmosphere sky (via IBL) and slide the sun glint across the
//! surface — i.e. real-looking water. `WaterPlugin` only registers the material; `worldmap`
//! builds the actual river/lake geometry with it.
//!
//! Exposes [`on_river`] / [`river_bank_t`] (per CONTRACT2.md) — the sine-centerline queries
//! the scatter (and the kept `decor` charm) use to keep props out of the water + dress banks.

use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

// ── River geometry constants (shared by the on_river / river_bank_t helpers) ──

/// Sideways swing amplitude of the centerline (world units): `x = AMP * sin(z*FREQ)`.
const RIVER_AMP: f32 = 6.0;
/// Spatial frequency of the centerline sine (radians per world unit along Z).
const RIVER_FREQ: f32 = 0.12;
/// Half the water-surface width: a tile is "on the river" within this of the centerline.
const HALF_WIDTH: f32 = 2.4;
/// How far past the water edge the bank ramp (`river_bank_t` 0→1) extends.
const BANK_RAMP: f32 = 3.0;
/// The river runs the full depth of the scene (well past the 32×32 patch) so it reads to
/// the fog horizon at both ends.
const Z_MIN: f32 = -90.0;
const Z_MAX: f32 = 90.0;

const WATER_SHADER: &str = "shaders/water.wgsl";

// ── Centerline math (one source of truth for mesh + queries) ────────────────────

/// World-space X of the river centerline at depth `z`.
#[inline]
fn centerline_x(z: f32) -> f32 {
    RIVER_AMP * (z * RIVER_FREQ).sin()
}

/// Unit tangent of the centerline in the XZ plane at depth `z` (advancing +Z).
#[inline]
fn centerline_tangent(z: f32) -> Vec2 {
    // d/dz of (centerline_x(z), z) = (AMP*FREQ*cos(z*FREQ), 1).
    let dx = RIVER_AMP * RIVER_FREQ * (z * RIVER_FREQ).cos();
    Vec2::new(dx, 1.0).normalize()
}

/// Unit normal (in XZ) pointing to the river's left/right edges at depth `z`.
#[inline]
fn centerline_normal(z: f32) -> Vec2 {
    let t = centerline_tangent(z);
    Vec2::new(t.y, -t.x) // perpendicular in XZ
}

/// Perpendicular distance from world point `(x,z)` to the centerline. Uses the local
/// normal at the point's own `z`, which is accurate because the curve is gentle
/// (|slope| ≤ AMP*FREQ = 0.72) — good enough for placement masking + banks.
#[inline]
fn dist_to_centerline(x: f32, z: f32) -> f32 {
    let n = centerline_normal(z);
    // Offset from the centerline point at this z is purely in X (same-z projection); its
    // perpendicular distance is that offset dotted with the unit normal's X component.
    ((x - centerline_x(z)) * n.x).abs()
}

/// True where the river water surface is (within the half-width of the centerline).
/// Scatter calls this to avoid spawning props in the water.
pub fn on_river(x: f32, z: f32) -> bool {
    if z < Z_MIN || z > Z_MAX {
        return false;
    }
    dist_to_centerline(x, z) <= HALF_WIDTH
}

/// 0 at the centerline, ramping to 1 a few units past the bank — decor uses this to
/// place wet-shore dressing (reeds, pebbles) along the edges and fade it out inland.
/// Returns 1.0 well away from the river (fully "dry land").
#[allow(dead_code)] // used only by the (kept, unwired) `decor` charm's bank dressing
pub fn river_bank_t(x: f32, z: f32) -> f32 {
    if z < Z_MIN || z > Z_MAX {
        return 1.0;
    }
    let d = dist_to_centerline(x, z);
    if d <= HALF_WIDTH {
        0.0
    } else {
        ((d - HALF_WIDTH) / BANK_RAMP).clamp(0.0, 1.0)
    }
}

// ── Material ────────────────────────────────────────────────────────────────────

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct WaterParams {
    /// x=amplitude, y=wave frequency, z=scroll speed, w=fresnel strength.
    pub params: Vec4,
    /// rgb = sky/fresnel tint added at grazing angles (a unused).
    pub sky_tint: Vec4,
}

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct WaterExt {
    #[uniform(100)]
    pub params: WaterParams,
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }
    // alpha_mode() left as None → inherits the base StandardMaterial's AlphaMode::Blend,
    // so the river is the translucent reflective sheet we configure below.
}

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        // Registers the water material; the world map builds the river/lake geometry with it.
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
    }
}
