//! Graphics quality presets — an **explicit** Low/High switch (set from Settings, top-right, or
//! the keyboard). No automatic scaling: the player picks, and we apply.
//!
//! The F2 profiler showed the **volumetric god-ray pass** (`volumetric_lighting`) as the runaway
//! GPU cost — ~13 ms, more than the whole main scene pass — so **Low** drops it entirely and
//! eases the other fill-heavy ambiance passes (SSAO, SMAA, shadow-map resolution). **High** is the
//! hand-tuned default look. Everything here is atmosphere, not gameplay, so Low stays fully
//! playable and legible; it just sheds the expensive eye-candy.

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::light::{DirectionalLightShadowMap, VolumetricFog};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::prelude::*;

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
    mut cam: Query<(Entity, Option<&mut VolumetricFog>), With<Camera3d>>,
    mut ssao: Query<&mut ScreenSpaceAmbientOcclusion>,
    mut smaa: Query<&mut Smaa>,
    mut shadowmap: ResMut<DirectionalLightShadowMap>,
) {
    use ScreenSpaceAmbientOcclusionQualityLevel as Q;
    // (volumetric step_count or None = off, SSAO quality, SMAA preset, shadow-map size)
    let (vol_steps, ao, smaa_preset, shadow_size) = match *quality {
        GraphicsQuality::High => (Some(32u32), Q::Medium, SmaaPreset::High, 2048usize),
        GraphicsQuality::Low => (None, Q::Low, SmaaPreset::Low, 1024),
    };

    // Volumetric fog: the big lever. Add/remove the component so the whole GPU pass appears or
    // disappears, rather than just running it cheaply.
    if let Ok((entity, vfog)) = cam.single_mut() {
        match (vol_steps, vfog) {
            (Some(n), Some(mut v)) => v.step_count = n,
            (Some(n), None) => {
                commands.entity(entity).insert(VolumetricFog {
                    ambient_intensity: 0.0,
                    jitter: 0.0,
                    step_count: n,
                    ..default()
                });
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<VolumetricFog>();
            }
            (None, None) => {}
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
