//! Custom **CoC bokeh depth-of-field** post pass — the player-focused DoF the old game had.
//! Bevy's built-in `DepthOfField` silently no-ops in this pipeline (verified: even f/1.0 with
//! SSAO removed produces no blur), so this is a fullscreen pass that reads the prepass depth
//! and blurs by a circle-of-confusion around a focal plane (driven onto the player by
//! `scene::drive_dof_focus`). On Bevy 0.19 the render graph is ECS systems, so this is a system
//! added to the `Core3d` schedule (`Core3dSystems::PostProcess`, after tonemapping) rather than a
//! `ViewNode` — see `dof_pass`. `outline.rs` is the same shape and orders itself before this.

use bevy::{
    anti_alias::smaa::smaa,
    core_pipeline::{prepass::ViewPrepassTextures, Core3d, Core3dSystems, FullscreenShader},
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

const SHADER_ASSET_PATH: &str = "shaders/dof.wgsl";
const NEAR: f32 = 0.1;

/// Per-camera bokeh-DoF settings (also the shader uniform). `focal` is overwritten each frame
/// by `scene::drive_dof_focus` (camera→player distance / a fixed mid-ground plane in free-cam).
#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct Dof {
    /// Focus distance (tiles).
    pub focal: f32,
    /// Half-width of the fully-sharp focus band (tiles).
    pub range: f32,
    /// Distance (tiles) over which the FAR blur ramps from sharp to max — large = gradual
    /// (the farther a thing is, the blurrier it gets, instead of clamping to a flat max).
    pub far_ramp: f32,
    /// Maximum blur radius (pixels).
    pub max_radius: f32,
    /// Camera near plane (depth → distance).
    pub near: f32,
    /// Debug: >0.5 paints the raw CoC (blur factor) as grayscale instead of blurring — white
    /// = "DoF thinks this is fully out of focus". Lets you see if a washed-out region is DoF.
    pub debug_view: f32,
}

/// A tasteful default; tunable live in the Debug panel.
pub fn default_dof() -> Dof {
    // 2026-07 readability pass (user feedback: too much blur near the hero): range 30→42 widens
    // the fully-sharp band so everything the player actually fights/loots reads crisp, far_ramp
    // 90→130 makes the distance melt come on later and more gradually, and max_radius 20→13
    // keeps the far bokeh a gentle cinematic soften instead of a smear.
    // 2026-07 follow-up (user: torches blur too soon / too hard): range 42→60 pushes the far-blur
    // ONSET out from focal+42 (~70 tiles) to focal+60 (~88), so mid-distance torches/braziers stay
    // sharp; max_radius 13→9 softens the remaining far bokeh so lit points read as flames, not smears.
    Dof { focal: 28.0, range: 60.0, far_ramp: 130.0, max_radius: 9.0, near: NEAR, debug_view: 0.0 }
}

pub struct DofPlugin;

impl Plugin for DofPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<Dof>::default(),
            UniformComponentPlugin::<Dof>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(RenderStartup, init_pipeline);
        // 0.19: the render graph is now ECS systems. The bokeh DoF runs in the Core3d `PostProcess`
        // stage and MUST be ordered `.after(smaa)` — not just after tonemapping. SMAA is ALSO a
        // `post_process_write` ping-pong pass sitting in PostProcess `.after(tonemapping)`, so
        // without this the multithreaded executor runs smaa/outline/dof in a varying order each
        // frame; the ping-pong's final buffer then alternates blurred/sharp → a real-time deblur
        // FLICKER (the old render graph pinned this with explicit edges). Pinned chain:
        // tonemapping → smaa → outline → dof → upscaling (DoF is the final blur, as before).
        // `outline.rs` orders itself `.before(dof_pass)` so its edges blur with the DoF.
        // ASSUMES SMAA is the camera's AA (added unconditionally in `scene.rs`; quality.rs only swaps
        // its preset). If you ever switch to FXAA/CAS/TAA, re-pin against that pass — `.after(smaa)`
        // silently becomes a no-op (no flicker error) when no `smaa` system runs, and the race returns.
        render_app.add_systems(Core3d, dof_pass.in_set(Core3dSystems::PostProcess).after(smaa));
    }
}

/// The bokeh-DoF fullscreen post pass, as a Core3d render-schedule system (0.19's replacement for
/// the old `ViewNode`). `ViewQuery` fetches the components of the view currently being rendered;
/// it validation-skips the whole system when the view lacks `Dof` (the Low graphics preset strips
/// the component), so no explicit "is DoF on?" guard is needed.
pub(crate) fn dof_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &Dof,
        &DynamicUniformIndex<Dof>,
    )>,
    pipeline_res: Res<DofPipeline>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Dof>>,
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
        "dof_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline_res.sampler,
            depth_view,
            settings_binding.clone(),
        )),
    );

    let mut render_pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("dof_pass"),
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
pub(crate) struct DofPipeline {
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
        "dof_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                uniform_buffer::<Dof>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    let shader = asset_server.load(SHADER_ASSET_PATH);
    let vertex_state = fullscreen_shader.to_vertex_state();
    let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("dof_pipeline".into()),
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
    commands.insert_resource(DofPipeline { layout, sampler, pipeline_id });
}
