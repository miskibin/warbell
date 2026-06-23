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
use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::light::{
    CascadeShadowConfig, DirectionalLight, DirectionalLightShadowMap, FogVolume, VolumetricFog,
    VolumetricLight,
};
use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use serde::{Deserialize, Serialize};

use crate::scene::Sun;
use crate::terrain::TerrainMaterial;

/// Which **preset chip** is lit in the Settings page. `High`/`Ultra`/`Low` are the canonical tunes;
/// `Custom` means the player has hand-tweaked at least one individual control, so no named preset
/// matches the live [`GraphicsSettings`]. `High` matches the scene's authored defaults.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum GraphicsQuality {
    High,
    #[default]
    Ultra,
    Low,
    /// Player-tuned mix — the live [`GraphicsSettings`] don't equal any named preset.
    Custom,
}

impl GraphicsQuality {
    /// Cycle order for the quick toggle / F10. Only steps through the *named* presets — `Custom`
    /// is reachable by tweaking an individual control, not by cycling — so from `Custom` it snaps
    /// back to `High`.
    pub fn next(self) -> Self {
        match self {
            GraphicsQuality::High => GraphicsQuality::Ultra,
            GraphicsQuality::Ultra => GraphicsQuality::Low,
            GraphicsQuality::Low => GraphicsQuality::High,
            GraphicsQuality::Custom => GraphicsQuality::High,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            GraphicsQuality::High => "High",
            GraphicsQuality::Ultra => "Ultra",
            GraphicsQuality::Low => "Low",
            GraphicsQuality::Custom => "Custom",
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

// ── Individual graphics settings — the per-control model behind the Settings page ──────────────
//
// Each field is one player-facing control. The named presets are just canned fills of this struct
// (`preset_settings`), and `apply_quality` translates these high-level choices into the actual
// render components/uniforms (SSAO, SMAA, shadow atlas, volumetric pass, terrain `params2`, …).
// Tweaking any one control flips the active preset to `Custom`; nothing here is render-API-typed,
// so the whole struct serialises cleanly to the on-disk graphics config.

/// Shadow fidelity: atlas size + cascade count + far reach, or fully off (biggest weak-GPU win).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ShadowLevel {
    Off,
    Low,
    Medium,
    High,
}

/// Anti-aliasing (SMAA preset), or off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AaLevel {
    Off,
    Low,
    High,
    Ultra,
}

/// Screen-space ambient occlusion quality, or off (off strips the whole depth-walk pass).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AoLevel {
    Off,
    Medium,
    Ultra,
}

/// Procedural ground-shader detail lane (`terrain.wgsl` `params2`): relief bump + the fine
/// noise/imprint octaves. `Low` is the cheap path (skips the expensive per-fragment layers).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TerrainDetail {
    Low,
    High,
    Ultra,
}

impl ShadowLevel {
    /// `(atlas_size, cascade_far)` for the on path; `None` when shadows are off.
    ///
    /// The cascade **count** is deliberately NOT varied between levels. Changing `num_cascades` on a
    /// live `CascadeShadowConfig` panics Bevy's `check_dir_light_mesh_visibility`: that system's
    /// thread-local parallel queues (`view_visible_entities_queue`) are only resized for worker
    /// threads that get a task on the current run, so when the count GROWS, a thread that ran last
    /// frame at the smaller size but is idle this frame keeps its stale length and the collection
    /// loop indexes past it (an out-of-bounds in bevy_light). So every level keeps the authored
    /// count and varies only atlas resolution + shadow reach (both safe to change at runtime).
    fn params(self) -> Option<(usize, f32)> {
        match self {
            ShadowLevel::Off => None,
            ShadowLevel::Low => Some((1024, 100.0)),
            ShadowLevel::Medium => Some((2048, 150.0)),
            ShadowLevel::High => Some((4096, 190.0)),
        }
    }
}

impl AaLevel {
    /// `Some(preset)` keeps the SMAA component; `None` removes it.
    fn smaa(self) -> Option<SmaaPreset> {
        match self {
            AaLevel::Off => None,
            AaLevel::Low => Some(SmaaPreset::Low),
            AaLevel::High => Some(SmaaPreset::High),
            AaLevel::Ultra => Some(SmaaPreset::Ultra),
        }
    }
}

impl AoLevel {
    fn level(self) -> Option<ScreenSpaceAmbientOcclusionQualityLevel> {
        use ScreenSpaceAmbientOcclusionQualityLevel as Q;
        match self {
            AoLevel::Off => None,
            AoLevel::Medium => Some(Q::Medium),
            AoLevel::Ultra => Some(Q::Ultra),
        }
    }
}

impl TerrainDetail {
    /// `(ground_bump, ground_quality_lane, ground_variety)` pushed to `TerrainMaterial::params2`.
    fn params(self) -> (f32, f32, f32) {
        match self {
            TerrainDetail::Low => (0.0, 0.0, 0.7),
            TerrainDetail::High => (1.0, 1.0, 1.0),
            TerrainDetail::Ultra => (1.3, 2.0, 1.0),
        }
    }
}

/// The full per-control graphics state. The Settings page binds each field to one widget; the
/// presets are canned fills; [`apply_quality`] reads it and reconfigures the renderer.
#[derive(Resource, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct GraphicsSettings {
    pub shadows: ShadowLevel,
    pub antialias: AaLevel,
    pub ssao: AoLevel,
    pub terrain: TerrainDetail,
    pub bloom: bool,
    pub depth_of_field: bool,
    pub outline: bool,
    pub god_rays: bool,
    /// Internal-3D render resolution as a fraction of the window (0.30–1.0). 1.0 = native. The
    /// dominant fragment-cost lever on weak GPUs (cost ≈ scale²); UI/post stay full-res.
    pub render_scale: f32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        preset_settings(GraphicsQuality::Ultra)
    }
}

/// Canonical per-preset fill of [`GraphicsSettings`]. `Custom` has no canonical fill (it *is* the
/// live struct), so it falls back to `High` if ever asked.
pub fn preset_settings(quality: GraphicsQuality) -> GraphicsSettings {
    match quality {
        GraphicsQuality::High => GraphicsSettings {
            shadows: ShadowLevel::Medium, // 2048 / 3 cascades / 150 reach (authored look)
            antialias: AaLevel::High,
            ssao: AoLevel::Medium,
            terrain: TerrainDetail::High,
            bloom: true,
            depth_of_field: true,
            outline: true,
            god_rays: false,
            render_scale: 1.0,
        },
        GraphicsQuality::Ultra => GraphicsSettings {
            shadows: ShadowLevel::High, // 4096 / 4 cascades / 190 reach
            antialias: AaLevel::Ultra,
            ssao: AoLevel::Ultra,
            terrain: TerrainDetail::Ultra,
            bloom: true,
            depth_of_field: true,
            outline: true,
            god_rays: true,
            render_scale: 1.0,
        },
        // Low: tuned for integrated GPUs — SSAO/bloom/DoF/outline off (each strips a whole pass),
        // 2 small shadow cascades, the cheap terrain lane, and a 0.6 render-scale (the big
        // fragment-cost cut on the fragment-bound iGPU this preset targets).
        GraphicsQuality::Low => GraphicsSettings {
            shadows: ShadowLevel::Low, // 1024 / 2 cascades / 100 reach
            antialias: AaLevel::Low,
            ssao: AoLevel::Off,
            terrain: TerrainDetail::Low,
            bloom: false,
            depth_of_field: false,
            outline: false,
            god_rays: false,
            render_scale: 0.6,
        },
        GraphicsQuality::Custom => preset_settings(GraphicsQuality::High),
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

/// Marker: the startup quality was chosen by `FOREST_QUALITY` env override **or** a saved graphics
/// config, so the hardware-detection system must not overwrite it.
#[derive(Resource)]
struct QualityLockedByEnv;

pub struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        let env_q = GraphicsQuality::from_env();
        let cfg = load_config();

        // Quality precedence: env override > saved config > Default. Hardware-detect downgrades to
        // Low only when NEITHER env nor a saved config already chose.
        let quality = env_q.or_else(|| cfg.as_ref().map(|c| c.quality)).unwrap_or_default();
        app.insert_resource(quality);
        if env_q.is_some() || cfg.is_some() {
            app.insert_resource(QualityLockedByEnv);
        }

        // Per-control settings: an env override fully defines the look (preset fill); otherwise the
        // saved config wins, else the preset's canonical fill. Window settings come from the config.
        let settings = if env_q.is_some() {
            preset_settings(quality)
        } else {
            cfg.as_ref().map(|c| c.settings.clone()).unwrap_or_else(|| preset_settings(quality))
        };
        let mut window = cfg.as_ref().map(|c| c.window.clone()).unwrap_or_default();
        // FOREST_NOVSYNC (uncapped perf testing, set in main.rs) must win — else `apply_window_settings`
        // would re-vsync the window on frame 1.
        if std::env::var("FOREST_NOVSYNC").is_ok() {
            window.vsync = false;
        }
        app.insert_resource(settings).insert_resource(window);

        app.init_resource::<RenderDefaults>()
            // Hardware-aware default: runs at Startup, after the render plugin's `finish()` has
            // inserted RenderAdapterInfo into the main world. Overwrites the default only when no
            // env override / saved config locked the preset and the adapter is a weak device type.
            .add_systems(Startup, detect_adapter_quality)
            .add_systems(Update, debug_qswitch) // FOREST_QSWITCH=<preset>: flip preset at t≈4s (crash repro)
            .add_systems(
                Update,
                (
                    // A named-preset change fills the per-control settings…
                    fill_settings_from_preset.run_if(resource_changed::<GraphicsQuality>),
                    // …and any settings change (preset fill OR a single-control tweak) reconfigures
                    // the renderer. Ordered after the fill so a preset click applies the same frame.
                    apply_quality
                        .run_if(resource_changed::<GraphicsSettings>)
                        .after(fill_settings_from_preset),
                ),
            )
            // Render-scale follows the settings AND the window size; self-gates internally, so it
            // stays ungated (it must catch window resizes, not just setting toggles).
            .add_systems(Update, apply_render_scale)
            // Window mode / vsync / resolution → primary window. Self-gated.
            .add_systems(Update, apply_window_settings);
    }
}

/// Debug repro: `FOREST_QSWITCH=<high|ultra|low>` flips the preset once at ~4 s of wall-clock, to
/// reproduce a runtime preset-change crash (e.g. Low→High re-inserting all the post passes + shadow
/// atlas resize). Inert without the env var.
fn debug_qswitch(
    time: Res<Time>,
    mut quality: ResMut<GraphicsQuality>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(target) = std::env::var("FOREST_QSWITCH").ok().and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
        "high" => Some(GraphicsQuality::High),
        "ultra" => Some(GraphicsQuality::Ultra),
        "low" => Some(GraphicsQuality::Low),
        _ => None,
    }) else {
        *done = true;
        return;
    };
    if time.elapsed_secs() >= 4.0 {
        info!("FOREST_QSWITCH: switching preset to {:?}", target);
        *quality = target;
        *done = true;
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

/// When the active **named** preset changes (chip click / F10 / hardware default / config load),
/// overwrite the live [`GraphicsSettings`] with that preset's canned fill. `Custom` is skipped — it
/// *is* the player's hand-tuned struct, so overwriting would undo their tweak. Runs on a
/// `GraphicsQuality` change; `apply_quality` (gated on a `GraphicsSettings` change) then reacts.
fn fill_settings_from_preset(quality: Res<GraphicsQuality>, mut settings: ResMut<GraphicsSettings>) {
    if *quality == GraphicsQuality::Custom {
        return;
    }
    let want = preset_settings(*quality);
    if *settings != want {
        *settings = want;
    }
}

/// Reconfigure the whole renderer from the live [`GraphicsSettings`]. Runs whenever the settings
/// resource changes (a preset fill OR a single-control tweak), never per-frame. Translates the
/// high-level per-control choices into the actual components/uniforms, inserting/removing whole
/// passes so an "off" setting truly skips its GPU cost (not just runs it at zero strength).
#[allow(clippy::too_many_arguments)]
fn apply_quality(
    settings: Res<GraphicsSettings>,
    mut defaults: ResMut<RenderDefaults>,
    mut commands: Commands,
    sun: Query<Entity, With<Sun>>,
    mut sun_light: Query<&mut DirectionalLight, With<Sun>>,
    cam: Query<Entity, With<Camera3d>>,
    mut cam_fog: Query<&mut VolumetricFog>,
    mut fog_vol: Query<&mut FogVolume>,
    bloom: Query<&Bloom>,
    mut cascades: Query<&mut CascadeShadowConfig>,
    mut shadowmap: ResMut<DirectionalLightShadowMap>,
    mut terrain_mats: ResMut<Assets<TerrainMaterial>>,
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

    let s = &*settings;
    let god = s.god_rays;

    // Volumetric pass on/off via the sun's VolumetricLight (the only reliable runtime switch).
    if let Ok(sun) = sun.single() {
        if god {
            commands.entity(sun).insert(VolumetricLight);
        } else {
            commands.entity(sun).remove::<VolumetricLight>();
        }
    }
    if god {
        // 64-step march — the smooth horizon rays the Ultra/god-rays look wants.
        for mut f in cam_fog.iter_mut() {
            f.step_count = 64;
        }
    }

    // FogVolume: the visible-shaft tune when god-rays are on, else the authored (imperceptible) volume.
    let fog = if god { Some(ultra_fog()) } else { None };
    for mut fv in fog_vol.iter_mut() {
        *fv = fog.clone().unwrap_or_else(|| defaults.fog_volume.clone().unwrap_or_default());
    }

    // Shadows: `Off` disables the sun's shadow casting entirely (the biggest weak-GPU win — no
    // cascade passes at all); otherwise size + cascade count + reach come from the level. Toggling
    // `DirectionalLight::shadows_enabled` is the runtime switch; the cascade/atlas config is only
    // re-derived when shadows are on.
    let shadow = s.shadows.params();
    if let Ok(mut dl) = sun_light.single_mut() {
        let want = shadow.is_some();
        if dl.shadow_maps_enabled != want {
            dl.shadow_maps_enabled = want;
        }
    }
    if let Some((size, far)) = shadow {
        for mut c in cascades.iter_mut() {
            let Some(auth) = defaults.cascades.as_ref() else { continue };
            // Re-derive the split layout from the authored config with only the far bound moved: the
            // first cascade keeps its authored near reach (texel density unchanged), the in-between
            // splits re-space exponentially toward the new horizon. `num_cascades` is ALWAYS the
            // authored count — see `ShadowLevel::params` for why changing it at runtime crashes.
            *c = bevy::light::CascadeShadowConfigBuilder {
                num_cascades: auth.bounds.len(),
                minimum_distance: auth.minimum_distance,
                maximum_distance: far,
                first_cascade_far_bound: auth.bounds.first().copied().unwrap_or(12.0),
                overlap_proportion: auth.overlap_proportion,
            }
            .build();
        }
        // Guard the write so an unchanged size doesn't trigger a needless shadow-atlas rebuild.
        if shadowmap.size != size {
            shadowmap.size = size;
        }
    }

    // Derived per-camera pass needs. The normal prepass exists only to feed SSAO + the outline; the
    // depth prepass feeds those plus the bokeh DoF (and contact shadows read it). When a consumer is
    // off we strip the prepass it fed, so the whole pass is skipped, not just run unused.
    let ao = s.ssao.level();
    let smaa = s.antialias.smaa();
    let normal_prepass = ao.is_some() || s.outline;
    let needs_depth = s.depth_of_field || ao.is_some() || s.outline;
    // Keep the Ultra (god-rays) bloom lift; otherwise the authored intensity.
    let bloom_intensity = if god { 0.42 } else { defaults.bloom_intensity };

    // Per-camera component toggles. We act on every camera carrying Camera3d so flycam + follow-cam
    // are both covered (scene.rs only spawns one camera, but this is robust to future additions).
    for cam_e in cam.iter() {
        let mut e = commands.entity(cam_e);

        // SSAO: `Off` removes the component entirely (saves the full depth-buffer walk cost on
        // iGPUs); otherwise insert it at the chosen quality level.
        match ao {
            Some(level) => {
                e.insert(ScreenSpaceAmbientOcclusion { quality_level: level, ..default() });
            }
            None => {
                e.remove::<ScreenSpaceAmbientOcclusion>();
            }
        }

        // Toon outline: removing the `Outline` component makes `OutlineNode`'s ViewQuery stop
        // matching this view, so the fullscreen edge pass is skipped.
        if s.outline {
            e.insert(crate::outline::default_outline());
        } else {
            e.remove::<crate::outline::Outline>();
        }

        // Normal prepass: dead weight when nothing consumes it (SSAO + outline both off), so drop it.
        if normal_prepass {
            e.insert(NormalPrepass);
        } else {
            e.remove::<NormalPrepass>();
        }

        // Bloom: removing the component skips the mip down/upsample chain entirely (a fixed iGPU
        // cost regardless of intensity).
        if s.bloom {
            e.insert(Bloom { intensity: bloom_intensity, ..Bloom::NATURAL });
        } else {
            e.remove::<Bloom>();
        }

        // Bokeh DoF: removing the `Dof` component stops DofNode's ViewQuery matching → pass skipped.
        if s.depth_of_field {
            e.insert(crate::dof::default_dof());
        } else {
            e.remove::<crate::dof::Dof>();
        }

        // SMAA: `Off` removes the component so the resolve pass is skipped; otherwise (re)insert at
        // the chosen preset. Managed per-camera (rather than mutating an always-present component) so
        // "off" is a real pass removal.
        match smaa {
            Some(preset) => {
                e.insert(Smaa { preset });
            }
            None => {
                e.remove::<Smaa>();
            }
        }

        // Depth prepass + contact shadows ride one gate: contact shadows (0.19) read the prepass
        // depth, so they must share the exact on/off as the prepass. Stripped together when nothing
        // (DoF/SSAO/outline) needs depth — that's the ~3 ms `early prepass` in the F2 profiler.
        if needs_depth {
            e.insert((DepthPrepass, ContactShadows::default()));
        } else {
            e.remove::<DepthPrepass>();
            e.remove::<ContactShadows>();
        }
    }

    // Ground relief: push the terrain-detail level's bump / quality-lane / variety into every
    // `TerrainMaterial`'s `params2`. FOREST_GROUNDLOD=N force-overrides the quality lane on ANY
    // setting (an A/B knob to isolate the terrain-shader cost).
    let (bump, ground_q, variety) = s.terrain.params();
    let q_override = std::env::var("FOREST_GROUNDLOD").ok().and_then(|v| v.trim().parse::<f32>().ok());
    for (_, m) in terrain_mats.iter_mut() {
        let q = q_override.unwrap_or(ground_q);
        m.extension.params.params2 = Vec4::new(bump, q, variety, 0.0);
    }
}

/// The effective render-scale: the `FOREST_RENDERSCALE` env override wins (A/B tuning on the target
/// machine), else the player's [`GraphicsSettings::render_scale`]. On a weak GPU the main 3D pass
/// (rasterising the whole scene) IS the frame — a Radeon 840M iGPU spends ~30 ms in
/// `main_opaque_pass_3d` alone at native res — so dropping below 1.0 is the dominant fragment lever.
fn render_scale_for(settings: &GraphicsSettings) -> f32 {
    if let Some(v) =
        std::env::var("FOREST_RENDERSCALE").ok().and_then(|s| s.trim().parse::<f32>().ok())
    {
        return v.clamp(0.3, 1.0);
    }
    settings.render_scale.clamp(0.3, 1.0)
}

/// Drive Bevy's [`MainPassResolutionOverride`] from the window size × the render-scale: the
/// opaque/transparent/prepass render at the lower resolution (Bevy upscales the result; the cheap
/// post passes — SMAA/tonemapping/UI — stay full-res), cutting the dominant fragment cost by ~scale²
/// on fragment-bound GPUs. Self-gating via `Local` so it only touches the camera when the scale or
/// window size actually changes (re-inserting every frame would mark the camera `Changed`).
fn apply_render_scale(
    settings: Res<GraphicsSettings>,
    windows: Query<&Window>,
    cam: Query<Entity, With<Camera3d>>,
    mut commands: Commands,
    mut last: Local<Option<UVec2>>,
) {
    let scale = render_scale_for(&settings);
    let Ok(win) = windows.single() else {
        return;
    };
    let want = (scale < 0.999).then(|| {
        UVec2::new(
            (win.physical_width() as f32 * scale).round().max(64.0) as u32,
            (win.physical_height() as f32 * scale).round().max(64.0) as u32,
        )
    });
    if *last == want {
        return;
    }
    *last = want;
    for cam_e in cam.iter() {
        match want {
            Some(res) => {
                commands.entity(cam_e).insert(MainPassResolutionOverride(res));
            }
            None => {
                commands.entity(cam_e).remove::<MainPassResolutionOverride>();
            }
        }
    }
}

// ── Window settings (display mode / vsync / resolution) + on-disk graphics config ──────────────
//
// These sit alongside the render-pipeline `GraphicsSettings` but act on the OS Window rather than
// the render graph, so they get their own resource + apply system. All persist with the rest of the
// graphics config so the player's choices survive a relaunch.

/// Player window preferences. `resolution: None` = follow the desktop/native size (no override).
#[derive(Resource, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct WindowSettings {
    pub fullscreen: bool,
    pub vsync: bool,
    /// Explicit window resolution `[w, h]` in physical pixels, or `None` for native (no override).
    pub resolution: Option<[u32; 2]>,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { fullscreen: false, vsync: true, resolution: None }
    }
}

/// Push [`WindowSettings`] onto the primary window: display mode, present mode (vsync) and an
/// optional explicit resolution. Self-gated on a settings change so it doesn't fight a manual window
/// resize every frame.
fn apply_window_settings(
    settings: Res<WindowSettings>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut last: Local<Option<WindowSettings>>,
) {
    if last.as_ref() == Some(&*settings) {
        return;
    }
    *last = Some(settings.clone());
    let Ok(mut win) = windows.single_mut() else { return };

    let want_mode = if settings.fullscreen {
        bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
    } else {
        bevy::window::WindowMode::Windowed
    };
    if win.mode != want_mode {
        win.mode = want_mode;
    }

    let want_present = if settings.vsync {
        bevy::window::PresentMode::AutoVsync
    } else {
        bevy::window::PresentMode::AutoNoVsync
    };
    if win.present_mode != want_present {
        win.present_mode = want_present;
    }

    // An explicit resolution only applies in windowed mode (borderless fullscreen tracks the desktop).
    if let Some([w, h]) = settings.resolution {
        if !settings.fullscreen
            && (win.resolution.physical_width() != w || win.resolution.physical_height() != h)
        {
            win.resolution.set_physical_resolution(w, h);
        }
    }
}

/// Player audio preferences persisted with the rest of the settings (0..=1 volume multipliers +
/// mute). The live resource is `ui::settings::AudioSettings`; this is its serialisable subset
/// (`unfocused` is transient and not stored). Kept here so the whole Settings menu writes one file.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct AudioPrefs {
    pub master: f32,
    pub music: f32,
    pub sfx: f32,
    pub muted: bool,
}

impl Default for AudioPrefs {
    fn default() -> Self {
        Self { master: 1.0, music: 1.0, sfx: 1.0, muted: false }
    }
}

/// Everything the Settings menu persists, so the player's choices survive a relaunch.
#[derive(Serialize, Deserialize)]
struct GraphicsConfig {
    quality: GraphicsQuality,
    settings: GraphicsSettings,
    #[serde(default)]
    window: WindowSettings,
    #[serde(default)]
    audio: AudioPrefs,
}

/// `graphics.json` next to the save file (same OS data-dir resolution as `savegame::save_path`).
fn config_path() -> std::path::PathBuf {
    use std::path::PathBuf;
    let dir = if let Ok(appdata) = std::env::var("APPDATA") {
        Some(PathBuf::from(appdata).join("tileworld"))
    } else if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        Some(PathBuf::from(xdg).join("tileworld"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(PathBuf::from(home).join(".local/share/tileworld"))
    } else {
        None
    };
    match dir {
        Some(d) => d.join("graphics.json"),
        None => PathBuf::from("tileworld-graphics.json"),
    }
}

/// Load the saved graphics config (None = missing / unreadable / unparseable — just use defaults).
fn load_config() -> Option<GraphicsConfig> {
    let text = std::fs::read_to_string(config_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// The saved audio preferences (or defaults). Lets `SettingsPlugin` seed `AudioSettings` at startup
/// from the same one config file the Settings menu writes.
pub fn load_audio_prefs() -> AudioPrefs {
    load_config().map(|c| c.audio).unwrap_or_default()
}

/// Persist the current settings. Best-effort: a write failure is logged, never fatal. Call at natural
/// commit points (Settings-menu close, preset click) rather than on every slider tick.
pub fn save_graphics_config(
    quality: &GraphicsQuality,
    settings: &GraphicsSettings,
    window: &WindowSettings,
    audio: &AudioPrefs,
) {
    let cfg = GraphicsConfig {
        quality: *quality,
        settings: settings.clone(),
        window: window.clone(),
        audio: *audio,
    };
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&cfg) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                warn!("failed to write graphics config {path:?}: {e}");
            }
        }
        Err(e) => warn!("failed to serialise graphics config: {e}"),
    }
}
