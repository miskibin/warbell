//! Graphics quality presets — an **explicit** switch (set from Settings, top-right, or the
//! keyboard), cycling **High → Low → God Rays**. No automatic scaling: the player picks, we apply.
//!
//! - **High**: the hand-tuned default look. The volumetric god-ray pass is **off** — at the
//!   scene's subtle fog settings the shafts are imperceptible yet still the frame's biggest GPU
//!   cost (~13 ms, per the F2 profiler), so neither High nor Low pays for it.
//! - **Low**: same (no god-rays) plus eased SSAO / SMAA / shadow-map resolution for weak GPUs.
//!   Stays fully playable and legible.
//! - **God Rays**: a showcase mode — the only preset that runs the volumetric pass — and it makes
//!   the shafts *unmistakable*: it kills the depth-of-field blur that smears them and cranks the
//!   volumetric scattering, so the beams read crisply toward the sun.
//!
//! The reliable on/off for the volumetric pass is the **sun's `VolumetricLight`** (Bevy's retained
//! render world only tears the pass down when no `VolumetricLight` exists — its extractor never
//! removes a stale `VolumetricFog` from a view), so that's what we toggle. The DoF blur and the
//! `FogVolume` tuning are snapshotted once at startup so non-showcase presets restore them exactly.

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::light::{DirectionalLightShadowMap, FogVolume, VolumetricFog, VolumetricLight};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::prelude::*;

use crate::dof::Dof;
use crate::scene::Sun;

/// The active preset. `High` matches the scene's authored defaults.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GraphicsQuality {
    #[default]
    High,
    Low,
    GodRays,
}

impl GraphicsQuality {
    /// Cycle order for the Settings button / F10.
    pub fn next(self) -> Self {
        match self {
            GraphicsQuality::High => GraphicsQuality::Low,
            GraphicsQuality::Low => GraphicsQuality::GodRays,
            GraphicsQuality::GodRays => GraphicsQuality::High,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            GraphicsQuality::High => "High",
            GraphicsQuality::Low => "Low",
            GraphicsQuality::GodRays => "God Rays",
        }
    }
}

/// The render values captured at startup, so non-showcase presets restore them exactly instead of
/// hardcoding the scene's defaults here.
#[derive(Resource, Default)]
struct RenderDefaults {
    captured: bool,
    fog_volume: Option<FogVolume>,
    dof_max_radius: f32,
}

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GraphicsQuality>()
            .init_resource::<RenderDefaults>()
            // Applies once at startup (the resource counts as "changed" when added) and again on
            // every Settings toggle — never per-frame.
            .add_systems(Update, apply_quality.run_if(resource_changed::<GraphicsQuality>));
    }
}

/// The `FogVolume` tuning that makes shafts clearly visible without blacking out the sky. The fog
/// box is huge (~320 u), so opacity = density × path-length — density and absorption must stay
/// *low* or the long horizon ray goes opaque. Visibility comes from high scattering + strong
/// forward asymmetry (concentrate the glow into beams aimed at the sun) + a brightness boost.
fn godrays_fog() -> FogVolume {
    FogVolume {
        density_factor: 0.03,
        absorption: 0.02,
        scattering: 0.95,
        scattering_asymmetry: 0.92,
        light_intensity: 3.0,
        ..default()
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_quality(
    quality: Res<GraphicsQuality>,
    mut defaults: ResMut<RenderDefaults>,
    mut commands: Commands,
    sun: Query<Entity, With<Sun>>,
    mut cam_fog: Query<&mut VolumetricFog>,
    mut fog_vol: Query<&mut FogVolume>,
    mut dof: Query<&mut Dof>,
    mut ssao: Query<&mut ScreenSpaceAmbientOcclusion>,
    mut smaa: Query<&mut Smaa>,
    mut shadowmap: ResMut<DirectionalLightShadowMap>,
) {
    use ScreenSpaceAmbientOcclusionQualityLevel as Q;

    // Snapshot the authored render values the first time we run (preset is High, so the live
    // components still hold the scene defaults).
    if !defaults.captured {
        defaults.fog_volume = fog_vol.iter().next().map(|f| f.clone());
        defaults.dof_max_radius = dof.iter().next().map(|d| d.max_radius).unwrap_or(18.0);
        defaults.captured = true;
    }

    // (god-rays on?, step_count, SSAO, SMAA, shadow size, showcase god-rays?)
    let (god_rays, steps, ao, smaa_preset, shadow_size, showcase) = match *quality {
        GraphicsQuality::High => (false, 32u32, Q::Medium, SmaaPreset::High, 2048usize, false),
        GraphicsQuality::Low => (false, 32, Q::Low, SmaaPreset::Low, 1024, false),
        GraphicsQuality::GodRays => (true, 48, Q::Medium, SmaaPreset::High, 2048, true),
    };

    // Volumetric pass on/off via the sun's VolumetricLight (the only reliable runtime switch).
    if let Ok(sun) = sun.single() {
        if god_rays {
            commands.entity(sun).insert(VolumetricLight);
        } else {
            commands.entity(sun).remove::<VolumetricLight>();
        }
    }
    if god_rays {
        for mut f in cam_fog.iter_mut() {
            f.step_count = steps;
        }
    }

    // FogVolume + DoF: showcase tunes them for visible shafts (and kills the blur that smears
    // them); every other preset restores the captured defaults.
    for mut fv in fog_vol.iter_mut() {
        *fv = if showcase { godrays_fog() } else { defaults.fog_volume.clone().unwrap_or_default() };
    }
    for mut d in dof.iter_mut() {
        d.max_radius = if showcase { 0.0 } else { defaults.dof_max_radius };
    }

    for mut s in ssao.iter_mut() {
        s.quality_level = ao;
    }
    for mut s in smaa.iter_mut() {
        s.preset = smaa_preset;
    }
    // Guard the write so an unchanged size doesn't trigger a needless shadow-atlas rebuild.
    if shadowmap.size != shadow_size {
        shadowmap.size = shadow_size;
    }
}
