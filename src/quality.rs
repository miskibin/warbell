//! Graphics quality presets — an **explicit** switch (set from Settings, top-right, or the
//! keyboard), cycling **High → Ultra → Low**. The STARTUP default is hardware-aware: on an
//! integrated GPU, virtual GPU, CPU renderer, or other weak adapter the game boots on Low;
//! discrete GPUs (the only ones that can afford the full Ultra cost) boot on Ultra. Manual
//! cycling is always available at runtime and is the only way to change the preset once the
//! game is running.
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
//! - **Low**: tuned for integrated GPUs (measured: ~4.3 ms SSAO + ~6-7 ms across 4 shadow
//!   cascades at 19 FPS on a typical iGPU). SSAO is **removed** entirely (the component is
//!   stripped from the camera — even the lowest-quality SSAO pass still walks the full-res depth
//!   buffer). Shadow cascades stop at 100 tiles (authored 150) — linear fog is already opaque
//!   there, so the far ground was invisible anyway, and this cuts one cascade's re-draw cost.
//!   SMAA Low and 1024 shadow atlas. Stays fully playable and legible; cycling to High/Ultra
//!   re-inserts SSAO at the preset's quality level.
//!
//! The reliable on/off for the volumetric pass is the **sun's `VolumetricLight`** (Bevy's retained
//! render world only tears the pass down when no `VolumetricLight` exists — its extractor never
//! removes a stale `VolumetricFog` from a view), so that's what we toggle. The DoF blur, bloom,
//! cascade config and the `FogVolume` tuning are snapshotted once at startup so other presets
//! restore them exactly.
//!
//! `FOREST_QUALITY=ultra|high|low` picks the startup preset (screenshot harness / demo
//! recording — same idea as the other `FOREST_*` staging hooks). The env var wins over the
//! hardware-aware default.

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::light::{
    CascadeShadowConfig, DirectionalLightShadowMap, FogVolume, VolumetricFog, VolumetricLight,
};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;

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

/// Marker: the startup quality was set by `FOREST_QUALITY` env override and must not be
/// overwritten by the hardware-detection system.
#[derive(Resource)]
struct QualityLockedByEnv;

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        // If the env var is set, lock in that preset and mark it so the adapter-detection
        // startup system skips overwriting it.
        if let Some(q) = GraphicsQuality::from_env() {
            app.insert_resource(q).insert_resource(QualityLockedByEnv);
        } else {
            app.insert_resource(GraphicsQuality::default());
        }

        app.init_resource::<RenderDefaults>()
            // Hardware-aware default: runs at Startup, after the render plugin's `finish()` has
            // inserted RenderAdapterInfo into the main world. Overwrites the default only when no
            // env override was given and the adapter is a weak device type.
            .add_systems(Startup, detect_adapter_quality)
            // Applies once at startup (the resource counts as "changed" when added) and again on
            // every Settings toggle — never per-frame.
            .add_systems(Update, apply_quality.run_if(resource_changed::<GraphicsQuality>));
    }
}

/// Reads the wgpu adapter type at startup and downgrades the quality preset to `Low` for
/// integrated GPUs, virtual GPUs, CPU renderers, and other weak adapters, **unless** the
/// `FOREST_QUALITY` env var already locked the preset. Discrete GPUs (and "Other" —
/// conservative: unknown adapters might be discrete) keep the default (Ultra).
fn detect_adapter_quality(
    adapter_info: Option<Res<RenderAdapterInfo>>,
    locked: Option<Res<QualityLockedByEnv>>,
    mut quality: ResMut<GraphicsQuality>,
) {
    // Env override wins; do nothing.
    if locked.is_some() {
        return;
    }
    let Some(info) = adapter_info else {
        // Renderer not yet initialized (shouldn't happen at Startup, but be safe).
        return;
    };

    use wgpu::DeviceType;
    let weak = matches!(
        info.0.device_type,
        DeviceType::IntegratedGpu | DeviceType::VirtualGpu | DeviceType::Cpu
    );
    if weak {
        info!(
            "Integrated/virtual GPU detected (adapter: {:?}, type: {:?}) — starting on Low quality.",
            info.0.name,
            info.0.device_type
        );
        *quality = GraphicsQuality::Low;
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
    /// `Some(level)` inserts / updates the SSAO component on the camera;
    /// `None` removes it entirely (Low preset — even the lowest SSAO quality still walks the
    /// full-res depth buffer; iGPUs measure ~4.3 ms just for that pass).
    ao: Option<ScreenSpaceAmbientOcclusionQualityLevel>,
    smaa: SmaaPreset,
    shadow_size: usize,
    /// God-ray `FogVolume` override; `None` restores the authored volume.
    fog: Option<FogVolume>,
    /// Bloom intensity override; `None` restores the authored value.
    bloom: Option<f32>,
    /// Cascade shadow far-distance override; `None` restores the authored config.
    cascade_far: Option<f32>,
    /// Whether the camera carries the prepass NORMAL texture. Only two passes consume normals:
    /// SSAO (depth+normal) and the toon outline (depth+normal). When BOTH are off (Low), the
    /// normal prepass is dead weight, so we strip `NormalPrepass`. **Depth is NOT optional**: the
    /// bokeh DoF (`dof.rs`) and the outline (when on) both read prepass depth, and DoF is active
    /// on every preset — so `DepthPrepass` always stays.
    normal_prepass: bool,
    /// Whether the toon outline post pass runs. `false` removes the `Outline` component from the
    /// camera so `OutlineNode`'s ViewQuery no longer matches the view (the pass is skipped). Low
    /// drops it: it's a subtle 0.15-strength cosmetic, and dropping it is what frees the normal
    /// prepass above (outline is the only non-SSAO normal consumer).
    outline: bool,
    /// Whether the bloom pass runs. Low removes the `Bloom` component entirely — the down/upsample
    /// mip chain is a fixed ~1.5 ms cost on an iGPU regardless of intensity, so intensity 0 would
    /// NOT save it; only dropping the component skips the passes.
    bloom_on: bool,
    /// Whether the bokeh DoF runs. Low removes the `Dof` component (its ViewQuery stops matching,
    /// so the pass is skipped) AND — since nothing else then needs prepass depth (SSAO + outline
    /// are already off on Low) — lets [`apply_quality`] strip `DepthPrepass` too (~3 ms).
    dof_on: bool,
    /// Override the shadow cascade COUNT. Fewer cascades = fewer per-cascade shadow re-draw passes
    /// (each is its own GPU pass in the F2 profiler). `None` keeps the authored count. Only honoured
    /// on the rebuild path (alongside `cascade_far`).
    cascades_count: Option<usize>,
}

fn preset(quality: GraphicsQuality) -> PresetVals {
    use ScreenSpaceAmbientOcclusionQualityLevel as Q;
    match quality {
        GraphicsQuality::High => PresetVals {
            god_rays: false,
            steps: 32,
            ao: Some(Q::Medium),
            smaa: SmaaPreset::High,
            shadow_size: 2048,
            fog: None,
            bloom: None,
            cascade_far: None,
            normal_prepass: true, // SSAO consumes normals
            outline: true,
            bloom_on: true,
            dof_on: true,
            cascades_count: None,
        },
        GraphicsQuality::Ultra => PresetVals {
            god_rays: true,
            // 64 steps: the showcase's 48 still shows faint banding on long horizon rays; Ultra
            // is the "I have the GPU" preset, so buy the smooth march.
            steps: 64,
            ao: Some(Q::Ultra),
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
            normal_prepass: true, // SSAO + outline both consume normals
            outline: true,
            bloom_on: true,
            dof_on: true,
            cascades_count: None,
        },
        // Low: tuned for integrated GPUs. Key savings vs High:
        //   • SSAO removed (None): ~4.3 ms saved — the depth-buffer walk happens regardless of
        //     the quality level, so even Q::Low still hurts on an iGPU.
        //   • cascade_far 100 (vs 150 authored): shadows stop where the fog is already opaque,
        //     cutting the farthest cascade's redraw. Saves ~2-3 ms of the ~6-7 ms cascade total.
        //   • 1024 shadow atlas, SMAA Low.
        //   • Toon outline OFF + `NormalPrepass` stripped: the only two normal-prepass consumers
        //     are SSAO (already off) and the outline; with both gone the normal prepass is pure
        //     waste, so we drop the component. The outline is a subtle 0.15-strength cosmetic and
        //     isn't worth the normal write + fullscreen edge pass on an iGPU.
        //   • Bloom OFF: the mip down/upsample chain is a fixed ~1.5 ms on an iGPU.
        //   • DoF OFF + `DepthPrepass` stripped: with SSAO, outline AND DoF all off, NOTHING
        //     consumes prepass depth anymore, so the whole early depth prepass (~3 ms) goes too.
        //   • 2 shadow cascades (vs authored 4): halves the per-cascade shadow re-draw passes —
        //     each cascade is its own GPU pass. 2 still covers near + mid where shadows read.
        // Net vs the old Low: ~no-bloom (1.5) + no-prepass/DoF (~3) + 2 fewer cascade passes (~3),
        // plus the freed VRAM (prepass depth texture + bloom mips + 2 shadow maps) eases the
        // integrated-GPU memory pressure that was forcing it to thrash.
        // Manual cycling Low → High/Ultra re-inserts SSAO + outline + normal prepass + bloom + DoF.
        GraphicsQuality::Low => PresetVals {
            god_rays: false,
            steps: 32,
            ao: None,
            smaa: SmaaPreset::Low,
            shadow_size: 1024,
            fog: None,
            bloom: None,
            cascade_far: Some(100.0),
            normal_prepass: false,
            outline: false,
            bloom_on: false,
            dof_on: false,
            cascades_count: Some(2),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_quality(
    quality: Res<GraphicsQuality>,
    mut defaults: ResMut<RenderDefaults>,
    mut commands: Commands,
    sun: Query<Entity, With<Sun>>,
    cam: Query<Entity, With<Camera3d>>,
    mut cam_fog: Query<&mut VolumetricFog>,
    mut fog_vol: Query<&mut FogVolume>,
    bloom: Query<&Bloom>,
    mut cascades: Query<&mut CascadeShadowConfig>,
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
    for mut c in cascades.iter_mut() {
        let Some(auth) = defaults.cascades.as_ref() else { continue };
        *c = match p.cascade_far {
            // Re-derive the split layout from the authored config with only the far bound moved:
            // the first cascade keeps its authored reach (near-shadow texel density unchanged),
            // the in-between splits re-space exponentially toward the new horizon.
            Some(far) => bevy::light::CascadeShadowConfigBuilder {
                num_cascades: p.cascades_count.unwrap_or(auth.bounds.len()),
                minimum_distance: auth.minimum_distance,
                maximum_distance: far,
                first_cascade_far_bound: auth.bounds.first().copied().unwrap_or(12.0),
                overlap_proportion: auth.overlap_proportion,
            }
            .build(),
            None => auth.clone(),
        };
    }

    // Per-camera component toggles (SSAO / outline / normal prepass). We act on every camera
    // carrying Camera3d so flycam + follow-cam are both covered (scene.rs only spawns one camera,
    // but this is robust to future additions).
    for cam_e in cam.iter() {
        let mut e = commands.entity(cam_e);

        // SSAO: Low removes the component entirely (saves the full depth-buffer walk cost on
        // iGPUs); High/Ultra insert it with the preset's quality level.
        match p.ao {
            Some(level) => {
                e.insert(ScreenSpaceAmbientOcclusion { quality_level: level, ..default() });
            }
            None => {
                e.remove::<ScreenSpaceAmbientOcclusion>();
            }
        }

        // Toon outline: removing the `Outline` component makes `OutlineNode`'s ViewQuery stop
        // matching this view, so the fullscreen edge pass is skipped (Low). Re-inserting restores
        // it. (The per-frame `fade_outline_toward_sun` system only mutates existing `Outline`s, so
        // dropping the component just parks it until the preset puts it back.)
        if p.outline {
            e.insert(crate::outline::default_outline());
        } else {
            e.remove::<crate::outline::Outline>();
        }

        // Normal prepass: the only consumers are SSAO and the outline. When both are off (Low) the
        // normal prepass is dead weight, so drop `NormalPrepass`. DEPTH is never dropped — the
        // bokeh DoF reads prepass depth on every preset. `NormalPrepass` is `#[require]`d by SSAO,
        // so when SSAO is present it would be re-added anyway; we manage it explicitly so the Low
        // (no-SSAO, no-outline) case actually strips it.
        if p.normal_prepass {
            e.insert(NormalPrepass);
        } else {
            e.remove::<NormalPrepass>();
        }

        // Bloom: Low removes the component so the mip down/upsample chain is skipped entirely
        // (a fixed iGPU cost regardless of intensity); High/Ultra (re)insert it at their intensity.
        if p.bloom_on {
            e.insert(Bloom {
                intensity: p.bloom.unwrap_or(defaults.bloom_intensity),
                ..Bloom::NATURAL
            });
        } else {
            e.remove::<Bloom>();
        }

        // Bokeh DoF: Low removes the `Dof` component (DofNode's ViewQuery stops matching → pass
        // skipped); High/Ultra restore it.
        if p.dof_on {
            e.insert(crate::dof::default_dof());
        } else {
            e.remove::<crate::dof::Dof>();
        }

        // Depth prepass: needed ONLY by DoF, SSAO, or the outline. When all three are off (Low),
        // strip it — that's the ~3 ms `early prepass` in the F2 profiler. (SSAO `#[require]`s it,
        // so when SSAO is on it'd be re-added regardless; we manage it explicitly for the Low case.)
        let needs_depth = p.dof_on || p.ao.is_some() || p.outline;
        if needs_depth {
            e.insert(DepthPrepass);
        } else {
            e.remove::<DepthPrepass>();
        }
    }

    for mut s in smaa.iter_mut() {
        s.preset = p.smaa;
    }
    // Guard the write so an unchanged size doesn't trigger a needless shadow-atlas rebuild.
    if shadowmap.size != p.shadow_size {
        shadowmap.size = p.shadow_size;
    }
}
