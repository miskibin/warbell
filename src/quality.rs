//! Graphics quality presets — an **explicit** switch (set from Settings, top-right, or the
//! keyboard), cycling **High → Ultra → Low**. No automatic scaling: the player picks, we apply.
//!
//! - **High**: the hand-tuned default look. The volumetric god-ray pass is **off** — at the
//!   scene's subtle fog settings the shafts are imperceptible yet still the frame's biggest GPU
//!   cost (~13 ms, per the F2 profiler), so neither High nor Low pays for it.
//! - **Ultra**: the demo / "prettiest possible" preset. Everything High has, plus: the volumetric
//!   pass ON with a fog tune that makes the sun shafts clearly visible *without* killing the
//!   bokeh DoF, SSAO + SMAA at their max levels, a 4096 shadow atlas, shadows pushed out to the
//!   fog line, and a bloom lift. GPU cost is the volumetric pass + bigger shadow atlas — a
//!   deliberate "I have the GPU for it" choice. (There used to be a separate "God Rays" showcase
//!   preset; it blacked out the Atmosphere sky — see [`ultra_fog`] — and Ultra replaces it.)
//! - **Low**: same as High (no god-rays) plus eased SSAO / SMAA / shadow-map resolution for weak
//!   GPUs. Stays fully playable and legible.
//!
//! The reliable on/off for the volumetric pass is the **sun's `VolumetricLight`** (Bevy's retained
//! render world only tears the pass down when no `VolumetricLight` exists — its extractor never
//! removes a stale `VolumetricFog` from a view), so that's what we toggle. The DoF blur, bloom,
//! cascade config and the `FogVolume` tuning are snapshotted once at startup so other presets
//! restore them exactly.
//!
//! `FOREST_QUALITY=ultra|high|low` picks the startup preset (screenshot harness / demo
//! recording — same idea as the other `FOREST_*` staging hooks).

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::light::{
    CascadeShadowConfig, DirectionalLightShadowMap, FogVolume, VolumetricFog, VolumetricLight,
};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

use crate::scene::Sun;

/// The active preset. `High` matches the scene's authored defaults.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GraphicsQuality {
    High,
    #[default]
    Ultra,
    Low,
}

impl GraphicsQuality {
    /// Cycle order for the Settings button / F10.
    pub fn next(self) -> Self {
        match self {
            GraphicsQuality::High => GraphicsQuality::Ultra,
            GraphicsQuality::Ultra => GraphicsQuality::Low,
            GraphicsQuality::Low => GraphicsQuality::High,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            GraphicsQuality::High => "High",
            GraphicsQuality::Ultra => "Ultra",
            GraphicsQuality::Low => "Low",
        }
    }

    /// `FOREST_QUALITY` startup override (shot/clip staging), e.g. `FOREST_QUALITY=ultra`.
    fn from_env() -> Option<Self> {
        match std::env::var("FOREST_QUALITY").ok()?.trim().to_ascii_lowercase().as_str() {
            "ultra" => Some(GraphicsQuality::Ultra),
            "high" => Some(GraphicsQuality::High),
            "low" => Some(GraphicsQuality::Low),
            _ => None,
        }
    }
}

/// The render values captured at startup, so non-default presets restore them exactly instead of
/// hardcoding the scene's authored numbers here.
#[derive(Resource, Default)]
struct RenderDefaults {
    captured: bool,
    fog_volume: Option<FogVolume>,
    bloom_intensity: f32,
    cascades: Option<CascadeShadowConfig>,
}

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GraphicsQuality::from_env().unwrap_or_default())
            .init_resource::<RenderDefaults>()
            // Applies once at startup (the resource counts as "changed" when added) and again on
            // every Settings toggle — never per-frame.
            .add_systems(Update, apply_quality.run_if(resource_changed::<GraphicsQuality>));
    }
}

/// Ultra's shaft tune: visible sun shafts **without the black-sky veil**. The trap (which the
/// old "God Rays" showcase preset fell into, and why "god rays never worked"): sky pixels have no
/// depth, so the volumetric march runs far longer through the fog box than for geometry — any
/// appreciable extinction (density × (absorption + scattering)) multiplies the Atmosphere sky
/// toward black while the ground stays bright. Ultra keeps extinction near the authored
/// (imperceptible) level and buys shaft visibility through `light_intensity` + forward asymmetry
/// instead.
fn ultra_fog() -> FogVolume {
    // FOREST_ULTRAFOG="density,absorption,scattering,asymmetry,light_intensity" — live tuning
    // knob for the shot harness (no rebuild churn while hunting the sky/shaft balance).
    if let Ok(s) = std::env::var("FOREST_ULTRAFOG") {
        let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if v.len() == 5 {
            return FogVolume {
                density_factor: v[0],
                absorption: v[1],
                scattering: v[2],
                scattering_asymmetry: v[3],
                light_intensity: v[4],
                ..default()
            };
        }
    }
    FogVolume {
        density_factor: 0.008,
        absorption: 0.004,
        scattering: 0.5,
        scattering_asymmetry: 0.85,
        light_intensity: 5.5,
        ..default()
    }
}

/// Everything one preset sets. `None` = restore the startup snapshot (the authored look).
struct PresetVals {
    god_rays: bool,
    /// Volumetric raymarch steps (only applied when `god_rays`).
    steps: u32,
    ao: ScreenSpaceAmbientOcclusionQualityLevel,
    smaa: SmaaPreset,
    shadow_size: usize,
    /// God-ray `FogVolume` override; `None` restores the authored volume.
    fog: Option<FogVolume>,
    /// Bloom intensity override; `None` restores the authored value.
    bloom: Option<f32>,
    /// Cascade shadow far-distance override; `None` restores the authored config.
    cascade_far: Option<f32>,
}

fn preset(quality: GraphicsQuality) -> PresetVals {
    use ScreenSpaceAmbientOcclusionQualityLevel as Q;
    match quality {
        GraphicsQuality::High => PresetVals {
            god_rays: false,
            steps: 32,
            ao: Q::Medium,
            smaa: SmaaPreset::High,
            shadow_size: 2048,
            fog: None,
            bloom: None,
            cascade_far: None,
        },
        GraphicsQuality::Ultra => PresetVals {
            god_rays: true,
            // 64 steps: the showcase's 48 still shows faint banding on long horizon rays; Ultra
            // is the "I have the GPU" preset, so buy the smooth march.
            steps: 64,
            ao: Q::Ultra,
            smaa: SmaaPreset::Ultra,
            shadow_size: 4096,
            fog: Some(ultra_fog()),
            // 0.30 authored → 0.42: lifts the sun-disk halo and the shaft glow without tipping
            // emissives (torches, magma) into smear.
            bloom: Some(0.42),
            // Authored 150 → 190: the linear fog fully wins by ~190 tiles, so this carries tree
            // shadows all the way to the fog line — the far ground stops going flat. Only worth
            // it with the 4096 atlas (at 2048 the stretched cascades visibly pixelate).
            cascade_far: Some(190.0),
        },
        GraphicsQuality::Low => PresetVals {
            god_rays: false,
            steps: 32,
            ao: Q::Low,
            smaa: SmaaPreset::Low,
            shadow_size: 1024,
            fog: None,
            bloom: None,
            cascade_far: None,
        },
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
    mut bloom: Query<&mut Bloom>,
    mut cascades: Query<&mut CascadeShadowConfig>,
    mut ssao: Query<&mut ScreenSpaceAmbientOcclusion>,
    mut smaa: Query<&mut Smaa>,
    mut shadowmap: ResMut<DirectionalLightShadowMap>,
) {
    // Snapshot the authored render values the first time we run (before any preset is applied,
    // so the live components still hold the scene defaults — even when FOREST_QUALITY starts the
    // game on a non-default preset, this runs first within the same apply).
    if !defaults.captured {
        defaults.fog_volume = fog_vol.iter().next().map(|f| f.clone());
        defaults.bloom_intensity = bloom.iter().next().map(|b| b.intensity).unwrap_or(0.30);
        defaults.cascades = cascades.iter().next().map(|c| c.clone());
        defaults.captured = true;
    }

    let p = preset(*quality);

    // Volumetric pass on/off via the sun's VolumetricLight (the only reliable runtime switch).
    if let Ok(sun) = sun.single() {
        if p.god_rays {
            commands.entity(sun).insert(VolumetricLight);
        } else {
            commands.entity(sun).remove::<VolumetricLight>();
        }
    }
    if p.god_rays {
        for mut f in cam_fog.iter_mut() {
            f.step_count = p.steps;
        }
    }

    // FogVolume / Bloom / cascades: presets that override them get their tune; everything else
    // restores the captured authored values.
    for mut fv in fog_vol.iter_mut() {
        *fv = p.fog.clone().unwrap_or_else(|| defaults.fog_volume.clone().unwrap_or_default());
    }
    for mut b in bloom.iter_mut() {
        b.intensity = p.bloom.unwrap_or(defaults.bloom_intensity);
    }
    for mut c in cascades.iter_mut() {
        let Some(auth) = defaults.cascades.as_ref() else { continue };
        *c = match p.cascade_far {
            // Re-derive the split layout from the authored config with only the far bound moved:
            // the first cascade keeps its authored reach (near-shadow texel density unchanged),
            // the in-between splits re-space exponentially toward the new horizon.
            Some(far) => bevy::light::CascadeShadowConfigBuilder {
                num_cascades: auth.bounds.len(),
                minimum_distance: auth.minimum_distance,
                maximum_distance: far,
                first_cascade_far_bound: auth.bounds.first().copied().unwrap_or(12.0),
                overlap_proportion: auth.overlap_proportion,
            }
            .build(),
            None => auth.clone(),
        };
    }

    for mut s in ssao.iter_mut() {
        s.quality_level = p.ao;
    }
    for mut s in smaa.iter_mut() {
        s.preset = p.smaa;
    }
    // Guard the write so an unchanged size doesn't trigger a needless shadow-atlas rebuild.
    if shadowmap.size != p.shadow_size {
        shadowmap.size = p.shadow_size;
    }
}
