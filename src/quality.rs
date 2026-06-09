//! Graphics quality presets — an **explicit** Low/High switch (set from Settings, top-right, or
//! the keyboard). No automatic scaling: the player picks, and we apply.
//!
//! The F2 profiler showed the **volumetric god-ray pass** (`volumetric_lighting`) as the runaway
//! GPU cost — ~13 ms, more than the whole main scene pass — so **Low** drops it entirely and
//! eases the other fill-heavy ambiance passes (SSAO, SMAA, shadow-map resolution). **High** is the
//! hand-tuned default look. Everything here is atmosphere, not gameplay, so Low stays fully
//! playable and legible; it just sheds the expensive eye-candy.

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::light::{DirectionalLightShadowMap, VolumetricFog, VolumetricLight};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::prelude::*;

use crate::scene::Sun;

/// The active preset. `High` matches the scene's authored defaults.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GraphicsQuality {
    #[default]
    High,
    Low,
}

impl GraphicsQuality {
    pub fn toggled(self) -> Self {
        match self {
            GraphicsQuality::High => GraphicsQuality::Low,
            GraphicsQuality::Low => GraphicsQuality::High,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            GraphicsQuality::High => "High",
            GraphicsQuality::Low => "Low",
        }
    }
}

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GraphicsQuality>()
            // Applies once at startup (the resource counts as "changed" when added) and again on
            // every Settings toggle — never per-frame.
            .add_systems(Update, apply_quality.run_if(resource_changed::<GraphicsQuality>));
    }
}

fn apply_quality(
    quality: Res<GraphicsQuality>,
    mut commands: Commands,
    sun: Query<Entity, With<Sun>>,
    mut cam_fog: Query<&mut VolumetricFog>,
    mut ssao: Query<&mut ScreenSpaceAmbientOcclusion>,
    mut smaa: Query<&mut Smaa>,
    mut shadowmap: ResMut<DirectionalLightShadowMap>,
) {
    use ScreenSpaceAmbientOcclusionQualityLevel as Q;
    // (god-rays on?, volumetric step_count, SSAO quality, SMAA preset, shadow-map size)
    let (god_rays, vol_steps, ao, smaa_preset, shadow_size) = match *quality {
        GraphicsQuality::High => (true, 32u32, Q::Medium, SmaaPreset::High, 2048usize),
        GraphicsQuality::Low => (false, 32, Q::Low, SmaaPreset::Low, 1024),
    };

    // Volumetric god-rays: the big lever. The reliable on/off is the *sun's* `VolumetricLight`,
    // NOT the camera's `VolumetricFog` — Bevy's retained render world only drops the volumetric
    // pass when no `VolumetricLight` exists (its extractor never removes a stale `VolumetricFog`
    // from a view), so removing the camera component at runtime would leave the pass running.
    if let Ok(sun) = sun.single() {
        if god_rays {
            commands.entity(sun).insert(VolumetricLight);
        } else {
            commands.entity(sun).remove::<VolumetricLight>();
        }
    }
    if god_rays {
        for mut f in cam_fog.iter_mut() {
            f.step_count = vol_steps;
        }
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
