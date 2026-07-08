//! **Cinematic atmospherics** post pass — analytic height fog + sun in-scatter + drifting
//! fog-density noise + cloud light patches, in one fullscreen pass. This is the "advanced
//! post-processing" layer that closes the gap to the reference look: warm layered haze that
//! trees melt into, and big soft patches of cloud light wandering across the ground —
//! neither of which Bevy's per-material `DistanceFog` can do (no height term, no noise, and
//! nothing that modulates the *lit scene*).
//!
//! Same shape as [`crate::dof`] / [`crate::godrays`] — a Bevy 0.19 `Core3d`-schedule render
//! system, `post_process_write` ping-pong, binding the prepass depth like DoF. Pinned
//! `.after(smaa).before(godrays_pass)` → chain
//! `tonemapping → smaa → atmospherics → godrays → outline → dof`, so the god rays scatter
//! the already-hazed frame (haze brightens toward the sun, which feeds the shafts).
//!
//! Colours are NOT authored here: the driver reads the live [`DistanceFog`] colour +
//! directional in-scatter colour every frame, so all the existing mood systems (time-of-day,
//! biome tint, war-dusk surge) keep steering the haze for free.

use bevy::{
    anti_alias::smaa::smaa,
    core_pipeline::{prepass::ViewPrepassTextures, Core3d, Core3dSystems, FullscreenShader},
    pbr::DistanceFog,
    prelude::*,
    render::{
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::ViewTarget,
        RenderApp, RenderStartup,
    },
};

use crate::scene::Sun;

const SHADER_ASSET_PATH: &str = "shaders/atmospherics.wgsl";

/// Per-camera atmospherics settings (also the shader uniform). Field ORDER must match the
/// WGSL `Settings` struct exactly. `world_from_clip` / `cam_pos` / `sun_dir` / `fog_color` /
/// `glow_color` / `time` / `fade` are driven each frame by [`drive_atmospherics`]; the rest
/// are the authored look (overridable via `FOREST_ATMO` for screenshot-harness tuning).
#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
pub struct Atmospherics {
    /// Inverse view-projection — clip → world unprojection (driven per frame).
    pub world_from_clip: Mat4,
    /// Camera world position (driven per frame).
    pub cam_pos: Vec3,
    /// Base fog density per world unit at `base_height`.
    pub density: f32,
    /// Direction TO the sun (driven per frame).
    pub sun_dir: Vec3,
    /// How fast the fog thins with altitude (per world unit) — the "height" in height fog.
    pub height_falloff: f32,
    /// Base haze colour, linear (driven from the live `DistanceFog` colour).
    pub fog_color: Vec3,
    /// Sun-glow lobe exponent — LOW = a wide golden wash, high = a tight spot.
    pub inscatter_exp: f32,
    /// Sun in-scatter colour, linear (driven from `DistanceFog.directional_light_color`).
    pub glow_color: Vec3,
    /// Seconds since startup (driven per frame) — drifts the noise fields.
    pub time: f32,
    /// Cloud light-patch modulation depth (0 = off). 0.22 ≈ gentle broken-cloud light.
    pub cloud_strength: f32,
    /// Cloud noise frequency in world XZ (1/units) — 0.018 ≈ 55-unit blobs.
    pub cloud_scale: f32,
    /// Fog-density noise modulation (0 = uniform veil).
    pub noise_strength: f32,
    /// Fog-free radius around the camera (world units).
    pub fog_start: f32,
    /// Max fog opacity — geometry is never fully swallowed (the god rays + fog colour
    /// carry the rest of the distance read).
    pub fog_max: f32,
    /// 0..1 master gate (daylight curve), driven per frame.
    pub fade: f32,
    /// World Y where the fog is densest (sea level).
    pub base_height: f32,
    pub _pad: f32,
}

impl Default for Atmospherics {
    fn default() -> Self {
        Self {
            world_from_clip: Mat4::IDENTITY,
            cam_pos: Vec3::ZERO,
            // 0.010 (was 0.018 → 0.015 → 0.012): eased again on player feedback — forward
            // visibility kept reading short. The haze should layer the forest, not curtain the view.
            density: 0.006, // was 0.010 — 2026-07-08: thinner haze, more forward visibility
            sun_dir: Vec3::Y,
            height_falloff: 0.07,
            fog_color: Vec3::new(0.85, 0.80, 0.66),
            inscatter_exp: 4.0,
            glow_color: Vec3::new(1.0, 0.85, 0.6),
            time: 0.0,
            // 0.10 — the patches multiply the TONEMAPPED image, so even modest values read
            // strongly; 0.18+ turned the ground into muddy bands (verified via FOREST_ATMO A/B).
            cloud_strength: 0.10,
            cloud_scale: 0.018,
            noise_strength: 0.40,
            // 24 (was 13): the fog-free bubble around the camera pushed out so nearby props /
            // enemies / the hero himself never sit in haze — the veil starts past melee range.
            fog_start: 48.0, // was 24 — 2026-07-08: haze bubble pushed out, veil starts much farther
            fog_max: 0.62, // was 0.84 — distant terrain never fully buries, keeps visibility
            fade: 0.0, // starts off; the driver raises it with daylight
            base_height: 0.0,
            _pad: 0.0,
        }
    }
}

/// The authored default, with a
/// `FOREST_ATMO="density,falloff,inscatter_exp,fog_start,fog_max,noise,cloud_strength,cloud_scale"`
/// startup override for the screenshot harness (no rebuild while hunting the look).
pub fn default_atmospherics() -> Atmospherics {
    let mut a = Atmospherics::default();
    if let Ok(s) = std::env::var("FOREST_ATMO") {
        let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if v.len() == 8 {
            a.density = v[0];
            a.height_falloff = v[1];
            a.inscatter_exp = v[2];
            a.fog_start = v[3];
            a.fog_max = v[4];
            a.noise_strength = v[5];
            a.cloud_strength = v[6];
            a.cloud_scale = v[7];
        }
    }
    a
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Master on/off for the cinematic haze pass (F1 debug panel). `true` by default. When `false`
/// — or while build mode is active — [`drive_atmospherics`] forces `fade = 0`, and the shader
/// early-returns the untouched frame (see the `fade <= 0.001` guard in `atmospherics.wgsl`).
#[derive(Resource)]
pub struct AtmosphericsEnabled(pub bool);

impl Default for AtmosphericsEnabled {
    fn default() -> Self {
        Self(true)
    }
}

pub struct AtmosphericsPlugin;

impl Plugin for AtmosphericsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<Atmospherics>::default(),
            UniformComponentPlugin::<Atmospherics>::default(),
        ))
        .init_resource::<AtmosphericsEnabled>()
        .add_systems(Update, drive_atmospherics);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(RenderStartup, init_pipeline);
        // PostProcess stage, pinned against the other ping-pong passes (see module doc):
        // tonemapping → smaa → atmospherics → godrays → outline → dof.
        render_app.add_systems(
            Core3d,
            atmospherics_pass
                .in_set(Core3dSystems::PostProcess)
                .after(smaa)
                .before(crate::godrays::godrays_pass),
        );
    }
}

/// Feed the uniform: camera matrices, sun direction, the live `DistanceFog` colours (so the
/// haze inherits time-of-day / biome / war-dusk mood), and the daylight gate. Runs in the
/// MAIN app (mutates the component before extract), like `godrays::drive_godrays`.
fn drive_atmospherics(
    time: Res<Time>,
    enabled: Res<AtmosphericsEnabled>,
    build_mode: Res<crate::town::BuildMode>,
    sun: Query<&GlobalTransform, With<Sun>>,
    mut cams: Query<(&GlobalTransform, &Camera, &DistanceFog, &mut Atmospherics)>,
) {
    let Ok(sun_tf) = sun.single() else {
        return;
    };
    let dir = sun_tf.translation().normalize_or_zero();
    // Full haze by day, eased down to a thin floor at night — the fog COLOUR already turns
    // navy after dark (DistanceFog), so the residual keeps night depth without a grey veil.
    let daylight = smoothstep(-0.05, 0.20, dir.y);
    // Kill the haze entirely when toggled off in the panel, or while placing buildings — the
    // build palette wants a clean, unhazed read of the plots. fade=0 makes the shader no-op.
    let fade = if !enabled.0 || build_mode.active { 0.0 } else { 0.30 + 0.70 * daylight };

    for (cam_tf, cam, fog, mut atmo) in &mut cams {
        let view = cam_tf.to_matrix();
        atmo.world_from_clip = view * cam.clip_from_view().inverse();
        atmo.cam_pos = cam_tf.translation();
        atmo.sun_dir = dir;
        let f = fog.color.to_linear();
        atmo.fog_color = Vec3::new(f.red, f.green, f.blue);
        let g = fog.directional_light_color.to_linear();
        atmo.glow_color = Vec3::new(g.red, g.green, g.blue);
        atmo.time = time.elapsed_secs();
        atmo.fade = fade;
    }
}

/// The atmospherics fullscreen post pass, as a Core3d render-schedule system. `ViewQuery`
/// validation-skips the whole system when the view lacks `Atmospherics` (the Low preset
/// strips the component), so no explicit on/off guard is needed.
pub(crate) fn atmospherics_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &Atmospherics,
        &DynamicUniformIndex<Atmospherics>,
    )>,
    pipeline_res: Res<AtmosphericsPipeline>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Atmospherics>>,
    mut ctx: RenderContext,
) {
    let (view_target, prepass, _settings, settings_index) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_res.pipeline_id) else {
        return;
    };
    let Some(settings_binding) = uniforms.uniforms().binding() else {
        return;
    };
    let Some(depth_view) = prepass.depth_view() else {
        return;
    };

    let post_process = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        "atmospherics_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline_res.sampler,
            depth_view,
            settings_binding.clone(),
        )),
    );

    let mut render_pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("atmospherics_pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        ..default()
    });

    render_pass.set_pipeline(pipeline);
    render_pass.set_bind_group(0, &bind_group, &[settings_index.index()]);
    render_pass.draw(0..3, 0..1);
}

#[derive(Resource)]
pub(crate) struct AtmosphericsPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

fn init_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    asset_server: Res<AssetServer>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "atmospherics_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                uniform_buffer::<Atmospherics>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    let shader = asset_server.load(SHADER_ASSET_PATH);
    let vertex_state = fullscreen_shader.to_vertex_state();
    let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("atmospherics_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: vertex_state,
        fragment: Some(FragmentState {
            shader,
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(AtmosphericsPipeline { layout, sampler, pipeline_id });
}
