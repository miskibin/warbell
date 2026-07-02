//! Screen-space **god rays** (radial light scattering) post pass — the crepuscular-ray effect the
//! original TS game had, and the reliable replacement for the volumetric god-ray path that never
//! worked (imperceptible at our fog settings, yet the frame's biggest cost, and it blacked out the
//! Atmosphere sky — see `quality.rs::ultra_fog`). This pass marches toward the sun's SCREEN
//! position, scattering the scene's own brightness into shafts along the treeline silhouette. It's
//! independent of fog and always visible when the sun is up and in/near frame.
//!
//! Same shape as [`crate::dof`] / [`crate::outline`] — a Bevy 0.19 `Core3d`-schedule render system
//! (not a `ViewNode`) — but with NO prepass binding (the light mask is scene luminance, not depth).
//! Pinned `.after(smaa).before(outline_pass)` → chain `tonemapping → smaa → godrays → outline → dof`.
//! Like the others it's a `post_process_write` ping-pong pass, so it MUST be ordered against the
//! other PostProcess ping-pong passes or the multithreaded executor races them and FLICKERS.

use bevy::{
    anti_alias::smaa::smaa,
    core_pipeline::{Core3d, Core3dSystems, FullscreenShader},
    light::DirectionalLight,
    prelude::*,
    render::{
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::ViewTarget,
        RenderApp, RenderStartup,
    },
};

use crate::scene::Sun;

const SHADER_ASSET_PATH: &str = "shaders/godrays.wgsl";

/// Per-camera god-rays settings (also the shader uniform). Field ORDER must match the WGSL
/// `Settings` struct exactly — both sides apply the same std140-ish layout, so identical
/// declaration order = identical offsets. `sun_screen` / `sun_color` / `fade` are driven each frame
/// by [`drive_godrays`]; the rest are the authored look (overridable via `FOREST_GODRAYS`).
#[derive(Component, Clone, Copy, ExtractComponent, ShaderType)]
pub struct GodRays {
    /// Ray tint — the live sun colour (linear RGB), driven per-frame.
    pub sun_color: Vec3,
    /// Overall additive strength.
    pub intensity: f32,
    /// Sun position in UV space [0,1], driven per-frame from the camera projection.
    pub sun_screen: Vec2,
    /// Per-step falloff along the ray (closer to 1 = longer shafts).
    pub decay: f32,
    /// March length toward the sun as a fraction of the screen.
    pub density: f32,
    /// Per-sample weight.
    pub weight: f32,
    /// Luminance above which a sample counts as "light" (sky/sun vs dark geometry).
    pub threshold: f32,
    /// March step count.
    pub num_samples: f32,
    /// 0..1 master gate (daylight × on-screen alignment), driven per-frame. 0 = no rays.
    pub fade: f32,
}

impl Default for GodRays {
    fn default() -> Self {
        Self {
            sun_color: Vec3::new(1.0, 0.83, 0.55), // a touch warmer/golden
            intensity: 1.15, // was 1.05 — the shafts should READ, not hint (1.3 smeared: verified)
            sun_screen: Vec2::new(0.5, 0.35),
            decay: 0.978, // longer shafts (was 0.974 → 0.96 — rays died too close to the sun)
            density: 0.85, // march further toward the sun so the shafts reach across the sky
            weight: 0.10,
            threshold: 0.60, // was 0.62; 0.50 fed mid-tones into the march → vertical smear curtains
            num_samples: 48.0,
            fade: 0.0, // starts off; the driver raises it when the sun is up and in frame
        }
    }
}

/// The authored default, with a `FOREST_GODRAYS="intensity,decay,density,weight,threshold,samples"`
/// startup override for the screenshot harness (no rebuild while hunting the look).
pub fn default_godrays() -> GodRays {
    let mut g = GodRays::default();
    if let Ok(s) = std::env::var("FOREST_GODRAYS") {
        let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if v.len() == 6 {
            g.intensity = v[0];
            g.decay = v[1];
            g.density = v[2];
            g.weight = v[3];
            g.threshold = v[4];
            g.num_samples = v[5];
        }
    }
    g
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub struct GodRaysPlugin;

impl Plugin for GodRaysPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<GodRays>::default(),
            UniformComponentPlugin::<GodRays>::default(),
        ))
        .add_systems(Update, drive_godrays);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(RenderStartup, init_pipeline);
        // PostProcess stage, pinned against the other ping-pong passes (see module doc + dof.rs):
        // tonemapping → smaa → godrays → outline → dof.
        render_app.add_systems(
            Core3d,
            godrays_pass
                .in_set(Core3dSystems::PostProcess)
                .after(smaa)
                .before(crate::outline::outline_pass),
        );
    }
}

/// Project the sun direction onto the screen and gate the rays. Runs in the MAIN app (mutates the
/// `GodRays` component before extract). The sun is a directional light parked at `dir * 120` looking
/// at the origin (see `scene.rs`), so its translation IS the direction to the sun.
fn drive_godrays(
    sun: Query<(&GlobalTransform, &DirectionalLight), With<Sun>>,
    mut cams: Query<(&GlobalTransform, &Camera, &mut GodRays)>,
) {
    let Ok((sun_tf, sun_light)) = sun.single() else {
        return;
    };
    let dir = sun_tf.translation().normalize_or_zero();
    // Rays only in daylight — fade out as the sun drops to/below the horizon (no night god rays).
    let daylight = smoothstep(-0.05, 0.20, dir.y);
    let lin = sun_light.color.to_linear();
    let sun_color = Vec3::new(lin.red, lin.green, lin.blue);

    for (cam_tf, cam, mut gr) in &mut cams {
        let fwd = (cam_tf.rotation() * Vec3::NEG_Z).normalize_or_zero();
        let align = fwd.dot(dir);
        // On-screen gate: full when the sun is roughly in front, easing to 0 as it moves to the
        // side/behind (a generous band so shafts still streak in from a just-off-screen sun).
        // Widened (was 0.20..0.60): the cinematic look leans on the rays, so they should hold
        // almost until the sun leaves the side of the frame instead of dying at a half-turn.
        let onscreen = smoothstep(0.05, 0.50, align);
        gr.fade = daylight * onscreen;
        gr.sun_color = sun_color;

        // Project a far point along the sun direction to its screen UV. Guarded by `fade` above, so
        // a behind-camera projection (garbage UV) is harmless — the rays are already gated off.
        let world = cam_tf.translation() + dir * 100_000.0;
        if let Some(ndc) = cam.world_to_ndc(cam_tf, world) {
            gr.sun_screen = Vec2::new(ndc.x * 0.5 + 0.5, ndc.y * -0.5 + 0.5);
        }
    }
}

/// The god-rays fullscreen post pass, as a Core3d render-schedule system. `ViewQuery` fetches the
/// current view's components and validation-skips the whole system when the view lacks `GodRays`
/// (High/Low presets without god-rays strip the component), so no explicit on/off guard is needed.
pub(crate) fn godrays_pass(
    view: ViewQuery<(&ViewTarget, &GodRays, &DynamicUniformIndex<GodRays>)>,
    pipeline_res: Res<GodRaysPipeline>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GodRays>>,
    mut ctx: RenderContext,
) {
    let (view_target, _settings, settings_index) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_res.pipeline_id) else {
        return;
    };
    let Some(settings_binding) = uniforms.uniforms().binding() else {
        return;
    };

    let post_process = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        "godrays_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline_res.sampler,
            settings_binding.clone(),
        )),
    );

    let mut render_pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("godrays_pass"),
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
pub(crate) struct GodRaysPipeline {
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
        "godrays_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<GodRays>(true),
            ),
        ),
    );
    // Linear + clamp: the radial march samples between texels, so linear filtering smooths the
    // shafts (Nearest bands them).
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    let shader = asset_server.load(SHADER_ASSET_PATH);
    let vertex_state = fullscreen_shader.to_vertex_state();
    let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("godrays_pipeline".into()),
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
    commands.insert_resource(GodRaysPipeline { layout, sampler, pipeline_id });
}
