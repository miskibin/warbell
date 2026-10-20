//! Graphics quality presets — an **explicit** switch (set from Settings, top-right, or the
//! keyboard), cycling **High → Ultra → Low**. The STARTUP default is hardware-aware: on an
//! integrated GPU, virtual GPU, CPU renderer, or other weak adapter the game boots on Low;
//! discrete GPUs (the only ones that can afford the full Ultra cost) boot on Ultra. Manual
//! cycling is always available at runtime and is the only way to change the preset once the
//! game is running.
//!
//! - **High**: the hand-tuned default look. Carries the screen-space god-rays pass (`godrays.rs`) —
//!   cheap and reliable, so the *everyday* scene gets the light shafts, not just the showcase.
//! - **Ultra**: the demo / "prettiest possible" preset. Everything High has, plus SSAO + SMAA at
//!   their max levels, a 4096 shadow atlas, shadows pushed out to the fog line, and a bloom lift.
//!   (The old volumetric god-ray pass — `VolumetricLight` + `FogVolume` — was retired: it was
//!   imperceptible at our fog yet the frame's biggest cost (~13 ms) and blacked out the Atmosphere
//!   sky. The screen-space pass in `godrays.rs` replaces it on both High and Ultra.)
//! - **Low**: tuned for integrated GPUs (measured: ~4.3 ms SSAO + ~6-7 ms across 4 shadow
//!   cascades at 19 FPS on a typical iGPU). SSAO is **removed** entirely (the component is
//!   stripped from the camera — even the lowest-quality SSAO pass still walks the full-res depth
//!   buffer). Shadow cascades stop at 100 tiles (authored 150) — linear fog is already opaque
//!   there, so the far ground was invisible anyway, and this cuts one cascade's re-draw cost.
//!   SMAA Low and 1024 shadow atlas. Stays fully playable and legible; cycling to High/Ultra
//!   re-inserts SSAO at the preset's quality level.
//!
//! God-rays toggle on/off by inserting/removing the camera's `godrays::GodRays` component (same
//! mechanism as the DoF/outline post passes). The DoF blur, bloom intensity and cascade config are
//! snapshotted once at startup so other presets restore them exactly.
//!
//! `FOREST_QUALITY=ultra|high|low` picks the startup preset (screenshot harness / demo
//! recording — same idea as the other `FOREST_*` staging hooks). The env var wins over the
//! hardware-aware default.

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::light::cluster::GlobalClusterSettings;
use bevy::light::{CascadeShadowConfig, DirectionalLight, DirectionalLightShadowMap};
use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::effect_stack::{ChromaticAberration, Vignette};
use bevy::light::ShadowFilteringMethod;
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve};
use bevy::post_process::motion_blur::MotionBlur;
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

/// Anti-aliasing (SMAA preset), temporal AA, or off.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AaLevel {
    Off,
    Low,
    High,
    Ultra,
    /// Temporal AA + contrast-adaptive sharpening. The only mode that stabilises alpha-tested
    /// foliage (leaf / grass cards shimmer under SMAA) and denoises GTAO, so the realistic
    /// forest presets use it and turn SSAO on with it.
    Taa,
}

/// Screen-space ambient occlusion quality, or off (off strips the whole depth-walk pass).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AoLevel {
    Off,
    Medium,
    Ultra,
}

/// Procedural ground-shader detail lane (`terrain.wesl` `params2`): relief bump + the fine
/// noise/imprint octaves. `Low` is the cheap path (skips the expensive per-fragment layers).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TerrainDetail {
    Low,
    High,
    Ultra,
}

/// Forest undergrowth density (`realforest` density slices): how much of the floor clutter and
/// green carpet is drawn. The biggest forest-specific FPS lever after shadows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum Undergrowth {
    Low,
    #[default]
    Medium,
    High,
}

impl Undergrowth {
    /// How many of the three density slices are shown.
    pub fn slices(self) -> u8 {
        match self {
            Undergrowth::Low => 1,
            Undergrowth::Medium => 2,
            Undergrowth::High => 3,
        }
    }
}

/// **Render distance** — how far the world draws before it is culled / fogged out. `Normal` is the
/// tuned gameplay budget (byte-for-byte the pre-setting behaviour); `Far` / `Extreme` are
/// screenshot modes that multiply every distance cut by [`ViewDistance::scale`]: scatter/tree/prop
/// `VisibilityRange` culls (and the terrain far-LOD hand-off), the camera far plane, the distance
/// fog + atmospherics haze, the sun's shadow-cascade reach ([`ViewDistance::shadow_scale`]) and the
/// realforest cover streaming radius ([`ViewDistance::cover_scale`], kept lower — cover is the
/// memory-heavy layer). Applies live (no world rebuild).
/// `FOREST_VIEWDIST=normal|far|extreme` overrides it at startup (capture harness).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum ViewDistance {
    #[default]
    Normal,
    Far,
    Extreme,
}

impl ViewDistance {
    /// Multiplier on every view-cutting distance (culls, fog, far plane).
    pub fn scale(self) -> f32 {
        match self {
            ViewDistance::Normal => 1.0,
            ViewDistance::Far => 1.8,
            ViewDistance::Extreme => 3.0,
        }
    }

    /// Multiplier on the realforest cover/props STREAMING radius (merged chunk meshes resident on
    /// the GPU — area grows with its square, so it is kept below [`Self::scale`]).
    pub fn cover_scale(self) -> f32 {
        match self {
            ViewDistance::Normal => 1.0,
            ViewDistance::Far => 1.5,
            ViewDistance::Extreme => 2.2,
        }
    }

    /// Multiplier on the sun's shadow-cascade far bound (cascade COUNT never changes at runtime, so
    /// this trades a little mid-range shadow texel density for shadowed far trees/ridges).
    pub fn shadow_scale(self) -> f32 {
        match self {
            ViewDistance::Normal => 1.0,
            ViewDistance::Far => 1.4,
            ViewDistance::Extreme => 2.0,
        }
    }

    fn from_env() -> Option<Self> {
        match std::env::var("FOREST_VIEWDIST").ok()?.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(ViewDistance::Normal),
            "far" => Some(ViewDistance::Far),
            "extreme" => Some(ViewDistance::Extreme),
            _ => None,
        }
    }
}

/// Live [`ViewDistance::scale`] / [`ViewDistance::cover_scale`] as f32 bits, readable from non-system
/// code (cover streaming, fog drivers). Seeded at plugin build, then mirrored by [`sync_view_distance`].
static VIEW_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000); // 1.0
static COVER_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000); // 1.0

/// The live render-distance multiplier (1.0 at `Normal`).
pub fn view_scale() -> f32 {
    f32::from_bits(VIEW_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

/// The live cover-streaming radius multiplier (1.0 at `Normal`).
pub fn cover_view_scale() -> f32 {
    f32::from_bits(COVER_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

fn store_view_distance(v: ViewDistance) {
    use std::sync::atomic::Ordering::Relaxed;
    VIEW_SCALE.store(v.scale().to_bits(), Relaxed);
    COVER_SCALE.store(v.cover_scale().to_bits(), Relaxed);
}

/// Mirror the settings' render distance into the atomics on any settings change.
fn sync_view_distance(settings: Res<GraphicsSettings>) {
    if view_scale() != settings.view_distance.scale() {
        info!("render distance -> {:?} (x{})", settings.view_distance, settings.view_distance.scale());
    }
    store_view_distance(settings.view_distance);
}

/// Margins below this (base units) are LOD hand-offs (tree LOD0->LOD1 at 30, lush near/far at 22)
/// or tiny-clutter culls (34) — kept as-is so a farther view doesn't also drag every LOD0 out.
const VIS_SCALE_MIN: f32 = 40.0;
/// Margins at/above this are "never cull" sentinels (1e6 / 1e30) — left alone.
const VIS_SCALE_MAX: f32 = 1.0e5;

/// Rescale one range from the multiplier currently baked in (`from`) to `to`. Every margin is
/// classified on its BASE value (`v / from`), so pairs that share a boundary (terrain fine-end /
/// coarse-start at 150) always move together and never open a gap.
fn rescale_range(r: &mut bevy::camera::visibility::VisibilityRange, from: f32, to: f32) {
    let f = |v: &mut f32| {
        let base = *v / from;
        if (VIS_SCALE_MIN..VIS_SCALE_MAX).contains(&base) {
            *v = base * to;
        }
    };
    f(&mut r.start_margin.start);
    f(&mut r.start_margin.end);
    f(&mut r.end_margin.start);
    f(&mut r.end_margin.end);
}

/// Scale every `VisibilityRange` in the world by the render distance — trees, kit cover/props,
/// legacy scatter, POIs / stories / town kit / vistas, the terrain far-LOD hand-off — without
/// touching any spawner. On a level change every live range is rescaled once; while the level is
/// above Normal, newly spawned ranges (streamed cover chunks, rebuilt worlds) are scaled as they
/// appear. At Normal with nothing baked in this returns before touching a single entity.
fn scale_visibility_ranges(
    mut applied: Local<Option<f32>>,
    mut ranges: Query<(&mut bevy::camera::visibility::VisibilityRange, Has<crate::biome::GroundCoverChunk>, Has<SmallDetail>)>,
) {
    let k = view_scale();
    let from = applied.unwrap_or(1.0);
    if k == 1.0 && from == 1.0 {
        *applied = Some(1.0);
        return;
    }
    // Ground cover (grass tufts / flowers / ferns — the bulk of the vertex work: ~0.4M indices
    // per 16u chunk) only stretches by `ground_factor`: blades are sub-pixel specks past ~60u, and
    // at the full x3 the Extreme level drew 9x the cover area (GPU 10.6 -> 21.9 ms in the village)
    // for grass nobody can see. Skinned people / animals ([`SmallDetail`]) likewise: a villager at
    // 480u is a few pixels, but still a full skinned draw in four passes. Trees, props and terrain
    // keep the full multiplier.
    let fac = |cover: bool, v: f32| if cover { ground_factor(v) } else { v };
    if from != k {
        for (mut r, cover, small) in &mut ranges {
            let cover = cover || small;
            // A range added since our last run was spawned at base scale (1.0), not `from`.
            let base_from = if r.is_added() { 1.0 } else { fac(cover, from) };
            rescale_range(&mut r, base_from, fac(cover, k));
        }
        *applied = Some(k);
        return;
    }
    for (mut r, cover, small) in &mut ranges {
        let cover = cover || small;
        if r.is_added() {
            rescale_range(&mut r, 1.0, fac(cover, k));
        }
    }
}

/// Marks a small-detail mesh (skinned townsfolk / animals, and the LOD1 meshes of impostor trees,
/// whose hand-off to the impostor must match `realforest::impostor`) whose `VisibilityRange` only takes
/// [`ground_factor`] of the render-distance multiplier, like ground cover.
#[derive(Component, Default)]
pub struct SmallDetail;

/// Ground-cover share of the render-distance multiplier: Far x1.24, Extreme x1.6 (vs x1.8 / x3).
pub(crate) fn ground_factor(k: f32) -> f32 {
    1.0 + (k - 1.0) * 0.3
}

/// The authored main-camera far plane (`scene::default_projection`).
const BASE_FAR: f32 = 230.0;

/// Push the main camera's far plane out with the render distance (Bevy's projection is
/// infinite-reverse-Z, so `far` only bounds frustum culling — no depth-precision cost). Only a
/// camera still on the authored far (or on the one we set) is touched; per-frame so an RTS restore
/// that re-inserts `default_projection` is caught. Idle at Normal once restored.
fn apply_view_far(mut applied: Local<Option<f32>>, mut cams: Query<&mut Projection, With<Camera3d>>) {
    let k = view_scale();
    let from = applied.unwrap_or(1.0);
    if k == 1.0 && from == 1.0 {
        *applied = Some(1.0);
        return;
    }
    let want = BASE_FAR * k;
    for mut proj in &mut cams {
        let Projection::Perspective(p) = proj.as_ref() else { continue };
        let ours = (p.far - BASE_FAR).abs() < 0.01 || (p.far - BASE_FAR * from).abs() < 0.01;
        if ours && p.far != want {
            if let Projection::Perspective(p) = proj.as_mut() {
                p.far = want;
            }
        }
    }
    *applied = Some(k);
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
            ShadowLevel::High => Some((4096, 165.0)),
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
            AaLevel::Taa => None,
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
    /// Per-object motion blur (`bevy_post_process`). OFF by default on every preset — it forces an
    /// always-on motion-vector prepass, so it's strictly opt-in. `#[serde(default)]` = `false`, so
    /// old saved configs (written before this field existed) load with it off.
    #[serde(default)]
    pub motion_blur: bool,
    /// Internal-3D render resolution as a fraction of the window (0.30–1.0). 1.0 = native. The
    /// dominant fragment-cost lever on weak GPUs (cost ≈ scale²); UI/post stay full-res.
    pub render_scale: f32,
    /// Forest undergrowth density. `#[serde(default)]` = Medium for configs saved before it existed.
    #[serde(default)]
    pub undergrowth: Undergrowth,
    /// Eye adaptation: histogram auto-exposure that nudges the authored exposure up to +-1 stop
    /// (deep forest shade <-> open glade) without flattening night into day.
    #[serde(default = "default_true")]
    pub eye_adaptation: bool,
    /// Dynamic resolution: when the GPU can't hold 60 fps, the 3D render scale dips (down to 60% of
    /// [`Self::render_scale`]) and climbs back as soon as there is headroom ([`DynRes`]). The UI and
    /// the post passes stay at full resolution. Does nothing on a GPU that keeps up.
    #[serde(default = "default_true")]
    pub dynamic_resolution: bool,
    /// Render distance (see [`ViewDistance`]). Not part of any preset — a preset fill keeps it.
    /// `#[serde(default)]` = Normal for configs saved before it existed.
    #[serde(default)]
    pub view_distance: ViewDistance,
}

fn default_true() -> bool {
    true
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
            antialias: AaLevel::Taa,
            // SSAO off: Bevy's GTAO temporal noise needs TAA to denoise; without it the AO pattern
            // crawls/flickers under camera motion (the "moving black patches on grass"). ContactShadows
            // on the sun/moon still give the contact-darkening read. Re-enableable via the menu.
            // With TAA on, GTAO is denoised temporally (no crawl) — it grounds the forest floor
            // clutter, trunks and props far better than contact shadows alone.
            ssao: AoLevel::Medium,
            terrain: TerrainDetail::High,
            bloom: true,
            depth_of_field: true,
            outline: false, // crisp toon edges off by default (user preference)
            god_rays: true, // screen-space light shafts (godrays.rs) — cheap, so High carries them too
            // A real-shutter camera blur (180 deg): what makes motion read as one continuous image
            // instead of a stack of razor-sharp frames. ~0.3 ms; the vector prepass is always on.
            motion_blur: true,
            render_scale: 1.0,
            undergrowth: Undergrowth::Medium,
            eye_adaptation: true,
            dynamic_resolution: true,
            view_distance: ViewDistance::Normal,
        },
        GraphicsQuality::Ultra => GraphicsSettings {
            shadows: ShadowLevel::High, // 4096 / 4 cascades / 190 reach
            antialias: AaLevel::Taa,
            ssao: AoLevel::Ultra,
            terrain: TerrainDetail::Ultra,
            bloom: true,
            depth_of_field: true,
            outline: false, // crisp toon edges off by default (user preference)
            god_rays: true,
            motion_blur: true,
            // Native: TAA does the edge-AA the old 2x SSAA bought at 4x the fragment cost.
            render_scale: 1.0,
            undergrowth: Undergrowth::High,
            eye_adaptation: true,
            dynamic_resolution: true,
            view_distance: ViewDistance::Normal,
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
            motion_blur: false,
            render_scale: 0.6,
            undergrowth: Undergrowth::Low,
            eye_adaptation: false,
            dynamic_resolution: true,
            view_distance: ViewDistance::Normal,
        },
        GraphicsQuality::Custom => preset_settings(GraphicsQuality::High),
    }
}

/// The eye-adaptation compensation curve. Bevy's auto exposure drives the frame's average
/// log-luminance `L` to the curve value: `correction = curve(L) - L`. This curve keeps the
/// correction inside +-`range` stops around the authored look's average `ref`: flat at `ref`
/// across `ref +- range` (full adaptation inside that band), slope 1 outside it (no further
/// correction), so a dim forest interior lifts and a bright glade settles a little while night
/// stays night. `FOREST_AE_REF` / `FOREST_AE_RANGE` tune it.
#[derive(Resource)]
pub struct EyeAdaptationCurve(pub Handle<AutoExposureCompensationCurve>);

fn build_eye_adaptation_curve(mut commands: Commands, mut curves: ResMut<Assets<AutoExposureCompensationCurve>>) {
    let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
    let (r, a) = (env("FOREST_AE_REF", -1.2), env("FOREST_AE_RANGE", 1.0));
    // `FOREST_AE=0` (or a degenerate range) = no curve = eye adaptation unavailable (A/B).
    if a < 0.05 || std::env::var("FOREST_AE").is_ok_and(|v| v == "0") {
        return;
    }
    let pts = [Vec2::new(-12.0, -12.0 + a), Vec2::new(r - a, r), Vec2::new(r + a, r), Vec2::new(8.0, 8.0 - a)];
    match AutoExposureCompensationCurve::from_curve(bevy::curve::cubic_splines::LinearSpline::new(pts)) {
        Ok(c) => {
            commands.insert_resource(EyeAdaptationCurve(curves.add(c)));
        }
        Err(e) => warn!("eye adaptation curve: {e}"),
    }
}

/// The render values captured at startup, so non-default presets restore them exactly instead of
/// hardcoding the scene's authored numbers here.
#[derive(Resource, Default)]
struct RenderDefaults {
    captured: bool,
    bloom_intensity: f32,
    cascades: Option<CascadeShadowConfig>,
}

/// Marker: the startup quality was chosen by `FOREST_QUALITY` env override **or** a saved graphics
/// config, so the hardware-detection system must not overwrite it.
#[derive(Resource)]
struct QualityLockedByEnv;

fn occlusion_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_OCCLUSION").map(|v| v != "0").unwrap_or(false))
}

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
        let mut settings = settings;
        env_overrides(&mut settings);
        store_view_distance(settings.view_distance);
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
            .add_plugins(bevy::post_process::auto_exposure::AutoExposurePlugin) // not in DefaultPlugins
            .add_systems(PreStartup, build_eye_adaptation_curve)
            // Weak-GPU stability: drop Bevy 0.19's GPU light-clustering readback (crash-prone under
            // device loss + a per-frame GPU→CPU stall). Runs before the first render extract.
            .add_systems(Startup, disable_gpu_clustering_on_weak_gpu)
            .add_systems(Update, debug_qswitch) // FOREST_QSWITCH=<preset>: flip preset at t≈4s (crash repro)
            .add_systems(Update, debug_mbtoggle) // FOREST_MBTOGGLE=1: flip motion-blur at t≈4s (crash repro)
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
            // Render distance: mirror into the atomics, then scale ranges / far plane. Both scalers
            // return immediately at Normal (nothing baked in), so the default costs nothing.
            .add_systems(
                Update,
                (
                    sync_view_distance.run_if(resource_changed::<GraphicsSettings>),
                    scale_visibility_ranges,
                    apply_view_far,
                )
                    .chain()
                    .after(fill_settings_from_preset),
            )
            // Render-scale follows the settings AND the window size; self-gates internally, so it
            // stays ungated (it must catch window resizes, not just setting toggles).
            .init_resource::<DynRes>()
            .add_systems(Update, (drive_dynamic_resolution, apply_render_scale).chain())
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

/// Debug repro: `FOREST_MBTOGGLE=1` flips the motion-blur setting once at ~4 s of wall-clock — the
/// runtime toggle the player hits in Settings — to reproduce the crash headlessly. Inert otherwise.
fn debug_mbtoggle(mut settings: ResMut<GraphicsSettings>, mut frame: Local<u32>, mut done: Local<bool>) {
    if *done {
        return;
    }
    if std::env::var("FOREST_MBTOGGLE").is_err() {
        *done = true;
        return;
    }
    *frame += 1;
    if *frame == 90 {
        settings.motion_blur = !settings.motion_blur;
        info!("FOREST_MBTOGGLE: motion_blur -> {} (runtime toggle)", settings.motion_blur);
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
        info.device_type,
        DeviceType::IntegratedGpu | DeviceType::VirtualGpu | DeviceType::Cpu
    );
    if weak {
        info!(
            "Integrated/virtual GPU detected (adapter: {:?}, type: {:?}) — starting on Low quality.",
            info.name,
            info.device_type
        );
        *quality = GraphicsQuality::Low;
    }
}

/// Bevy 0.19 added **GPU-driven light clustering**: a per-frame compute pass whose results are read
/// back to the CPU (`map_buffer_on_submit` → `get_mapped_range`) every frame. On weak integrated
/// GPUs that readback is doubly bad:
///
/// 1. **Performance** — it forces a hard GPU→CPU sync every frame, stalling the pipeline on exactly
///    the tiled/integrated GPUs least able to afford it (the reporter's "the game is very lag").
/// 2. **Stability** — the readback path is *not* crash-safe. If the device is ever lost (a driver
///    timeout under load — precisely what a struggling iGPU hits), the map callback panics on the
///    now-invalid staging buffer, poisons its `Mutex`, and the next frame's
///    `readback_data.lock().unwrap()` brings the whole process down. That is issue #67: an Intel
///    iGPU reports `DeviceLost`, then `bevy_pbr::cluster::gpu` panics with a `PoisonError`.
///
/// Falling back to **CPU clustering** (the pre-0.19 path, `gpu_clustering = None`) removes the
/// readback machinery entirely — Bevy's render error handler then rides out transient device errors
/// instead of hard-crashing. Our scene only ever has dozens of lights, so CPU clustering costs us
/// nothing. Applied to integrated / virtual / CPU adapters (same weak set as the quality default);
/// `FOREST_GPUCLUSTER=on|off` force-overrides either way (testing / a mis-classified adapter).
fn disable_gpu_clustering_on_weak_gpu(
    adapter_info: Option<Res<RenderAdapterInfo>>,
    cluster: Option<ResMut<GlobalClusterSettings>>,
) {
    let Some(mut cluster) = cluster else { return };
    // Device didn't support the GPU path in the first place — already on stable CPU clustering.
    if cluster.gpu_clustering.is_none() {
        return;
    }

    // Env escape hatch wins (headless testing, or a user on an adapter we classify wrong).
    let forced = std::env::var("FOREST_GPUCLUSTER").ok().and_then(|s| {
        match s.trim().to_ascii_lowercase().as_str() {
            "1" | "on" | "true" | "yes" => Some(true),
            "0" | "off" | "false" | "no" => Some(false),
            _ => None,
        }
    });

    let keep_gpu = match forced {
        Some(k) => k,
        None => {
            // Default: keep GPU clustering only on discrete (and unknown "Other") adapters; drop it
            // on the weak set that hits the readback stall + DeviceLost crash.
            let Some(info) = adapter_info.as_ref() else { return };
            use wgpu::DeviceType;
            !matches!(
                info.device_type,
                DeviceType::IntegratedGpu | DeviceType::VirtualGpu | DeviceType::Cpu
            )
        }
    };

    if !keep_gpu {
        let name = adapter_info.as_ref().map(|i| i.name.clone()).unwrap_or_default();
        info!(
            "Disabling GPU light clustering (adapter: {name:?}) — falling back to stable CPU \
             clustering to avoid the per-frame readback stall and the DeviceLost crash (issue #67)."
        );
        cluster.gpu_clustering = None;
    }
}

/// The camera's contact-shadow settings (`FOREST_CONTACT=steps,thickness,length` overrides).
fn contact_shadows(env: Option<&str>) -> ContactShadows {
    let v: Vec<f32> = env.map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect()).unwrap_or_default();
    if v.len() == 3 {
        return ContactShadows { linear_steps: v[0] as u32, thickness: v[1], length: v[2] };
    }
    ContactShadows::default()
}

/// A/B staging knobs on top of any preset: `FOREST_AA=off|low|high|ultra|taa`,
/// `FOREST_SSAO=off|medium|ultra`. (Read in `apply_quality`: `FOREST_SHADOWFILTER=gaussian|temporal|
/// hardware`, `FOREST_CONTACT=off`; in `scene.rs`: `FOREST_SHADOWBIAS=depth,normal`; in
/// `realforest`: `FOREST_NONORMAL=set[,set..]` drops those kit sets' normal maps.)
fn env_overrides(settings: &mut GraphicsSettings) {
    match std::env::var("FOREST_AA").ok().as_deref() {
        Some("off") => settings.antialias = AaLevel::Off,
        Some("low") => settings.antialias = AaLevel::Low,
        Some("high") => settings.antialias = AaLevel::High,
        Some("ultra") => settings.antialias = AaLevel::Ultra,
        Some("taa") => settings.antialias = AaLevel::Taa,
        _ => {}
    }
    match std::env::var("FOREST_TERRAIN").ok().as_deref() {
        Some("low") => settings.terrain = TerrainDetail::Low,
        Some("high") => settings.terrain = TerrainDetail::High,
        Some("ultra") => settings.terrain = TerrainDetail::Ultra,
        _ => {}
    }
    match std::env::var("FOREST_SSAO").ok().as_deref() {
        Some("off") => settings.ssao = AoLevel::Off,
        Some("medium") => settings.ssao = AoLevel::Medium,
        Some("ultra") => settings.ssao = AoLevel::Ultra,
        _ => {}
    }
    if let Some(v) = ViewDistance::from_env() {
        settings.view_distance = v;
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
    let mut want = preset_settings(*quality);
    env_overrides(&mut want);
    want.view_distance = settings.view_distance; // render distance isn't part of a preset
    want.dynamic_resolution = settings.dynamic_resolution; // nor is dynamic resolution
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
    mut sun_light: Query<&mut DirectionalLight, With<Sun>>,
    cam: Query<Entity, With<Camera3d>>,
    bloom: Query<&Bloom>,
    mut cascades: Query<&mut CascadeShadowConfig>,
    mut shadowmap: ResMut<DirectionalLightShadowMap>,
    mut terrain_mats: ResMut<Assets<TerrainMaterial>>,
    ae_curve: Option<Res<EyeAdaptationCurve>>,
) {
    // Snapshot the authored render values the first time we run (before any preset is applied,
    // so the live components still hold the scene defaults — even when FOREST_QUALITY starts the
    // game on a non-default preset, this runs first within the same apply).
    if !defaults.captured {
        defaults.bloom_intensity = bloom.iter().next().map(|b| b.intensity).unwrap_or(0.30);
        defaults.cascades = cascades.iter().next().map(|c| c.clone());
        defaults.captured = true;
    }

    let s = &*settings;
    let god = s.god_rays; // screen-space god-rays toggled as a camera component in the per-camera loop below

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
                // Render distance stretches only the far bound (same cascade count — see above).
                // `FOREST_SHADOWFAR=<u>` pins the horizon for perf A/Bs (the far cascade holds most casters).
                maximum_distance: std::env::var("FOREST_SHADOWFAR").ok().and_then(|v| v.parse().ok())
                    .unwrap_or(far * settings.view_distance.shadow_scale()),
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
    let taa = s.antialias == AaLevel::Taa;
    let normal_prepass = ao.is_some() || s.outline;
    // god_rays counts: the atmospherics pass (gated with it) reads the prepass depth.
    let needs_depth = s.depth_of_field || ao.is_some() || s.outline || god || taa;
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
                // Assumed object thickness 1.0 (default 0.25): with TAA resolving it fully, the default
                // occluded ~17% of the forest's (large) ambient term and the whole wood went dark.
                // Measured: 1.0 keeps the contact grounding at ~3% overall darkening.
                let thickness = std::env::var("FOREST_AO_THICK").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
                e.insert(ScreenSpaceAmbientOcclusion { quality_level: level, constant_object_thickness: thickness, ..default() });
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

        // Screen-space god rays: same component-toggle mechanism — removing `GodRays` makes the
        // pass's ViewQuery stop matching, so the fullscreen scatter pass is skipped (High/Ultra on,
        // Low off). The driver (`godrays::drive_godrays`) gates it further to daylight + sun-in-frame.
        if god {
            e.insert(crate::godrays::default_godrays());
        } else {
            e.remove::<crate::godrays::GodRays>();
        }

        // Cinematic atmospherics (height fog + in-scatter + cloud light patches): rides the
        // god-rays gate — the same "premium look" tier — via the same component-toggle mechanism.
        if god {
            e.insert(crate::atmospherics::default_atmospherics());
        } else {
            e.remove::<crate::atmospherics::Atmospherics>();
        }

        // Water screen-space reflections (`water_ssr.rs`): same premium tier, same toggle.
        if god {
            e.insert(crate::water_ssr::default_water_ssr());
        } else {
            e.remove::<crate::water_ssr::WaterSsr>();
        }

        // Cinematic lens: a static edge vignette only. Chromatic aberration is OFF by default (user
        // preference) — the component is never inserted, so `postfx::drive_chromatic` finds nothing
        // and there's no resting fringe NOR hit-spike fringe. The vignette rides the existing
        // effect-stack pass (no new render-graph node) — a cheap premium-preset dressing, gated like
        // god-rays (High/Ultra on, Low off). Film grain is a separate UI overlay (`postfx`).
        if god {
            e.insert(crate::postfx::default_vignette());
        } else {
            e.remove::<Vignette>();
        }
        // Chromatic aberration is disabled everywhere; ensure no stale component lingers.
        e.remove::<ChromaticAberration>();

        // Motion blur: opt-in (off by default). Toggle ONLY the blur effect here — the
        // motion-vector prepass that feeds it lives on the camera from spawn and is never toggled,
        // because adding it to a live view crashes wgpu (the velocity texture isn't reallocated, so
        // the bg-motion-vectors pipeline mismatches the pass). MotionBlur safely reads the
        // always-present texture, same as the outline/DoF effect toggles. Shutter > 0.5 over-blurs
        // past a true 24fps shutter on purpose — a clearly-visible artistic blur when enabled.
        if s.motion_blur {
            // 180 deg shutter (film standard): natural smear on camera turns and fast movers.
            e.insert(MotionBlur { shutter_angle: 0.5, samples: 3 });
        } else {
            e.remove::<MotionBlur>();
        }

        // Eye adaptation (see `EyeAdaptationCurve`): limited +-1-stop histogram auto-exposure.
        match (s.eye_adaptation, ae_curve.as_ref()) {
            (true, Some(curve)) => {
                e.insert(AutoExposure {
                    range: -10.0..=6.0,
                    filter: 0.25..=0.85,
                    speed_brighten: 1.2,
                    speed_darken: 0.9,
                    exponential_transition_distance: 1.0,
                    compensation_curve: curve.0.clone(),
                    ..default()
                });
            }
            _ => {
                e.remove::<AutoExposure>();
            }
        }

        // Shadow filtering: with TAA the temporal filter resolves soft, stable penumbrae (leaf
        // shadows no longer crawl); otherwise the 9-tap Gaussian.
        e.insert(match std::env::var("FOREST_SHADOWFILTER").ok().as_deref() {
            // A/B knob (flicker hunts): force one filter regardless of the AA level.
            Some("gaussian") => ShadowFilteringMethod::Gaussian,
            Some("temporal") => ShadowFilteringMethod::Temporal,
            Some("hardware") => ShadowFilteringMethod::Hardware2x2,
            _ if taa => ShadowFilteringMethod::Temporal,
            _ => ShadowFilteringMethod::Gaussian,
        });

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

        // TAA + CAS: the jitter and mip bias TAA requires must leave with it, or the image keeps
        // sub-pixel shaking with nothing resolving it.
        if taa {
            // No CAS after TAA any more: even at 0.2 the sharpener re-crunched the alpha-tested leaf
            // and grass edges TAA had just softened — the "over-sharpened" read of the mid-ground.
            e.insert(TemporalAntiAliasing::default());
            e.remove::<ContrastAdaptiveSharpening>();
        } else {
            e.remove::<(TemporalAntiAliasing, TemporalJitter, MipBias, ContrastAdaptiveSharpening)>();
        }

        // Depth prepass + contact shadows ride one gate: contact shadows (0.19) read the prepass
        // depth, so they must share the exact on/off as the prepass. Stripped together when nothing
        // (DoF/SSAO/outline) needs depth — that's the ~3 ms `early prepass` in the F2 profiler.
        // `FOREST_CONTACT=off`: A/B knob, keep the depth prepass but drop the contact shadows;
        // `FOREST_CONTACT=steps,thickness,length` overrides the settings.
        let contact_env = std::env::var("FOREST_CONTACT").ok();
        let contact = contact_env.as_deref() != Some("off");
        if needs_depth && contact {
            e.insert((DepthPrepass, contact_shadows(contact_env.as_deref())));
        } else if needs_depth {
            e.insert(DepthPrepass);
            e.remove::<ContactShadows>();
        } else {
            e.remove::<DepthPrepass>();
            e.remove::<ContactShadows>();
        }
        // GPU two-phase occlusion culling (the HZB test every AAA engine runs): meshes whose bounds
        // sit wholly behind last frame's depth skip their vertex work in every later pass. Needs
        // the depth prepass. `FOREST_OCCLUSION=0` turns it off (A/B).
        if needs_depth && occlusion_enabled() {
            e.insert(bevy::render::occlusion_culling::OcclusionCulling);
        } else {
            e.remove::<bevy::render::occlusion_culling::OcclusionCulling>();
        }
    }

    // Ground relief: push the terrain-detail level's bump / quality-lane / variety into every
    // `TerrainMaterial`'s `params2`. FOREST_GROUNDLOD=N force-overrides the quality lane on ANY
    // setting (an A/B knob to isolate the terrain-shader cost).
    let (bump, ground_q, variety) = s.terrain.params();
    let q_override = std::env::var("FOREST_GROUNDLOD").ok().and_then(|v| v.trim().parse::<f32>().ok());
    let want = Vec4::new(bump, q_override.unwrap_or(ground_q), variety, 0.0);
    // Touch ONLY the terrain materials whose value actually differs. This whole system runs on ANY
    // settings change (toggling shadows, bloom, a resolution tweak…), and `Assets::iter_mut` queues
    // an `AssetEvent::Modified` for **every** asset it walks whether or not anything is stored — so
    // the old unconditional loop re-uploaded + re-bound every terrain chunk on settings that have
    // nothing to do with the ground. Collect the stale ids read-only first, then `get_mut` just
    // those (which marks only them modified). Steady state: an empty list, zero GPU work.
    let stale: Vec<AssetId<TerrainMaterial>> = terrain_mats
        .iter()
        .filter(|(_, m)| m.extension.params.params2 != want)
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(mut m) = terrain_mats.get_mut(id) {
            m.extension.params.params2 = want;
        }
    }
}

/// The effective render-scale: the `FOREST_RENDERSCALE` env override wins (A/B tuning on the target
/// machine), else the player's [`GraphicsSettings::render_scale`]. On a weak GPU the main 3D pass
/// (rasterising the whole scene) IS the frame — a Radeon 840M iGPU spends ~30 ms in
/// `main_opaque_pass_3d` alone at native res — so dropping below 1.0 is the dominant fragment lever.
///
/// NEVER above 1.0: Bevy's `MainPassResolutionOverride` "must be smaller than the camera's viewport
/// size" — there is no supersample-and-downsample behind it. At 2.0 the main pass rasterised 4x the
/// pixels into a window-sized target (tragic frame times), its depth no longer matched the post
/// passes' UVs (black, flickering blotches in the water reflections) and the frame read blurred /
/// over-sharpened. Clamp 0.3..1.0 (an old saved 2.0 lands on 1.0).
fn render_scale_for(settings: &GraphicsSettings) -> f32 {
    if let Some(v) = env_render_scale() {
        return v;
    }
    settings.render_scale.clamp(0.3, 1.0)
}

fn env_render_scale() -> Option<f32> {
    std::env::var("FOREST_RENDERSCALE").ok().and_then(|s| s.trim().parse::<f32>().ok()).map(|v| v.clamp(0.3, 1.0))
}

/// **Dynamic resolution** (the console / Unreal staple): the live fraction of the player's render
/// scale the 3D passes draw at. Driven from the GPU's own per-pass timestamps (`RenderDiagnosticsPlugin`,
/// the same numbers the F2 overlay shows), so it only reacts when the GPU is the bottleneck — a
/// CPU-bound frame (the usual case on a discrete card) never loses resolution. Cost ≈ scale², so a
/// step is sized by √(budget / gpu). `MainPassResolutionOverride` is a viewport inside full-size
/// targets: changing it reallocates nothing, so steps are cheap and can come every half second.
/// `FOREST_DYNRES=0` turns it off; it also stays off under a capture or a `FOREST_RENDERSCALE` override.
#[derive(Resource)]
pub struct DynRes {
    pub factor: f32,
    timer: f32,
}

impl Default for DynRes {
    fn default() -> Self {
        Self { factor: 1.0, timer: 0.0 }
    }
}

/// GPU frame budget (ms): 60 fps less a margin for the passes the timers don't see (present, UI
/// composite) and for timer noise.
const DYNRES_BUDGET_MS: f64 = 14.5;
/// Lowest fraction of the player's render scale it may drop to, and the absolute floor.
const DYNRES_MIN_FACTOR: f32 = 0.6;
const DYNRES_MIN_SCALE: f32 = 0.4;
/// Re-evaluate this often (seconds).
const DYNRES_PERIOD: f32 = 0.5;

fn dynres_allowed(settings: &GraphicsSettings) -> bool {
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let env_ok = *ENV.get_or_init(|| {
        std::env::var("FOREST_DYNRES").map(|v| v != "0").unwrap_or(true)
            && std::env::var_os("FOREST_SHOT").is_none()
            && std::env::var_os("FOREST_CLIP").is_none()
    });
    env_ok && settings.dynamic_resolution && env_render_scale().is_none()
}

/// Sum of the top-level GPU pass timers (ms), or `None` when the GPU can't timestamp.
fn gpu_frame_ms(diag: &bevy::diagnostic::DiagnosticsStore) -> Option<f64> {
    let mut sum = 0.0;
    let mut any = false;
    for d in diag.iter() {
        let p = d.path().as_str();
        let Some(name) = p.strip_prefix("render/").and_then(|n| n.strip_suffix("/elapsed_gpu")) else { continue };
        if name.contains('/') || d.measurement().is_none_or(|m| m.time.elapsed().as_millis() > 500) {
            continue; // nested spans are inside their parent's time; stale = didn't run lately
        }
        if let Some(ms) = d.smoothed() {
            sum += ms;
            any = true;
        }
    }
    any.then_some(sum)
}

fn drive_dynamic_resolution(
    time: Res<Time<Real>>,
    settings: Res<GraphicsSettings>,
    diag: Option<Res<bevy::diagnostic::DiagnosticsStore>>,
    mut dyn_res: ResMut<DynRes>,
) {
    if !dynres_allowed(&settings) {
        if dyn_res.factor != 1.0 {
            dyn_res.factor = 1.0;
        }
        return;
    }
    dyn_res.timer += time.delta_secs();
    if dyn_res.timer < DYNRES_PERIOD {
        return;
    }
    dyn_res.timer = 0.0;
    let Some(gpu) = diag.as_deref().and_then(gpu_frame_ms) else { return };
    let base = settings.render_scale.clamp(0.3, 1.0);
    let min = (DYNRES_MIN_FACTOR).max(DYNRES_MIN_SCALE / base).min(1.0);
    let f = dyn_res.factor;
    // Timers measure the CURRENT scale; scale cost ~ factor², so the factor that would fit is
    // f·√(budget/gpu). Drop at once (a hitch is worse than a softer frame), climb in small steps.
    let fit = f * (DYNRES_BUDGET_MS / gpu.max(0.1)).sqrt() as f32;
    let next = if gpu > DYNRES_BUDGET_MS {
        fit.max(f - 0.15)
    } else if gpu < DYNRES_BUDGET_MS * 0.75 {
        (f + 0.05).min(fit)
    } else {
        f
    };
    let next = (next.clamp(min, 1.0) * 20.0).round() / 20.0; // 5% steps: no churn on timer noise
    if (next - f).abs() > 1e-3 {
        dyn_res.factor = next;
    }
}

/// Drive Bevy's [`MainPassResolutionOverride`] from the window size × the render-scale: the
/// opaque/transparent/prepass render at the scaled resolution (Bevy resamples the result to the
/// window; the cheap post passes — SMAA/tonemapping/UI — stay full-res). Below 1.0 this cuts the
/// dominant fragment cost by ~scale² on fragment-bound GPUs; above 1.0 it supersamples (SSAA) for
/// true geometric edge-AA. Self-gating via `Local` so it only touches the camera when the scale or
/// window size actually changes (re-inserting every frame would mark the camera `Changed`).
fn apply_render_scale(
    settings: Res<GraphicsSettings>,
    windows: Query<&Window>,
    cam: Query<Entity, With<Camera3d>>,
    mut commands: Commands,
    mut last: Local<Option<UVec2>>,
    dyn_res: Res<DynRes>,
) {
    let scale = render_scale_for(&settings) * dyn_res.factor;
    let Ok(win) = windows.single() else {
        return;
    };
    // Override whenever the scale deviates from native (either direction); ==1.0 removes it.
    let want = ((scale - 1.0).abs() > 0.001).then(|| {
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

/// `graphics.json` next to the save slots (same OS data-dir resolution as `savegame::save_dir`).
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::visibility::VisibilityRange;

    fn vr(s: std::ops::Range<f32>, e: std::ops::Range<f32>) -> VisibilityRange {
        VisibilityRange { start_margin: s, end_margin: e, use_aabb: true }
    }

    #[test]
    fn view_distance_scaling_keeps_lods_and_pairs() {
        // Terrain fine/coarse hand-off at 150..176 moves together; "never cull" stays put.
        let mut fine = vr(0.0..0.0, 150.0..176.0);
        let mut coarse = vr(150.0..176.0, 1.0e30..1.0e30);
        // Tree LOD0 (30±2) keeps its hand-off; the LOD1 cull at 180 scales.
        let mut lod0 = vr(0.0..0.0, 28.0..32.0);
        let mut lod1 = vr(28.0..32.0, 180.0..180.0);
        for r in [&mut fine, &mut coarse, &mut lod0, &mut lod1] {
            rescale_range(r, 1.0, 3.0);
        }
        assert_eq!(fine.end_margin, 450.0..528.0);
        assert_eq!(coarse.start_margin, 450.0..528.0);
        assert_eq!(coarse.end_margin, 1.0e30..1.0e30);
        assert_eq!(lod0.end_margin, 28.0..32.0);
        assert_eq!(lod1.start_margin, 28.0..32.0);
        assert_eq!(lod1.end_margin, 540.0..540.0);
        // Extreme -> Far -> Normal round-trips back to the authored values.
        rescale_range(&mut lod1, 3.0, 1.8);
        rescale_range(&mut lod1, 1.8, 1.0);
        assert!((lod1.end_margin.start - 180.0).abs() < 1e-3);
        assert_eq!(ViewDistance::Normal.scale(), 1.0);
        assert_eq!(ViewDistance::Normal.cover_scale(), 1.0);
    }
}
