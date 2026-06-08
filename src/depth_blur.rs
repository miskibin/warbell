//! Custom **distance depth-blur** post-process — the reliable replacement for Bevy's
//! built-in `DepthOfField` (which silently no-ops alongside SSAO and only does a single
//! focal plane). A fullscreen pass after tonemapping reads the prepass depth, turns it
//! into a camera distance, and blurs the colour by an amount that is ZERO inside `clear`
//! tiles and ramps to `radius` px by `full` tiles — exactly the "sharp nearby, soft in
//! the distance" the three.js build had. Coexists fine with SSAO (it only *reads* depth).
//!
//! Built straight off the Bevy 0.18 `custom_post_processing` example (RenderStartup
//! pipeline init, `BindGroupLayoutDescriptor` in the cache, `FullscreenShader`), with one
//! extra binding: the prepass depth texture (`ViewPrepassTextures::depth_view`).

use bevy::{
    core_pipeline::{
        core_3d::graph::{Core3d, Node3d},
        prepass::ViewPrepassTextures,
        FullscreenShader,
    },
    ecs::query::QueryItem,
    prelude::*,
    render::{
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_graph::{
            NodeRunError, RenderGraphContext, RenderGraphExt, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice},
        view::ViewTarget,
        RenderApp, RenderStartup,
    },
};

const SHADER_ASSET_PATH: &str = "shaders/depth_blur.wgsl";
/// Camera near plane (Bevy `PerspectiveProjection` default) — used to turn reverse-z depth
/// into an eye-forward distance: `dist = near / ndc_depth`.
const NEAR: f32 = 0.1;

/// Per-camera depth-blur settings (also the shader uniform). Tiles for clear/full, pixels
/// for radius.
#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct DepthBlur {
    /// Tiles fully sharp around the camera.
    pub clear: f32,
    /// Tiles at which blur reaches `radius`.
    pub full: f32,
    /// Max blur radius, in pixels.
    pub radius: f32,
    /// Camera near plane (depth → distance).
    pub near: f32,
}

/// Build the camera's [`DepthBlur`] from `FOREST_BLUR="clear,full,radius"` (tiles,tiles,px),
/// else sensible defaults for the enlarged island.
pub fn settings_from_env() -> DepthBlur {
    let (clear, full, radius) = std::env::var("FOREST_BLUR")
        .ok()
        .and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            (v.len() == 3).then_some((v[0], v[1], v[2]))
        })
        .unwrap_or((60.0, 150.0, 18.0));
    DepthBlur { clear, full, radius, near: NEAR }
}

pub struct DepthBlurPlugin;

impl Plugin for DepthBlurPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<DepthBlur>::default(),
            UniformComponentPlugin::<DepthBlur>::default(),
        ));

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(RenderStartup, init_pipeline);
        render_app
            .add_render_graph_node::<ViewNodeRunner<DepthBlurNode>>(Core3d, DepthBlurLabel)
            .add_render_graph_edges(
                Core3d,
                (Node3d::Tonemapping, DepthBlurLabel, Node3d::EndMainPassPostProcessing),
            );
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct DepthBlurLabel;

#[derive(Default)]
struct DepthBlurNode;

impl ViewNode for DepthBlurNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewPrepassTextures,
        &'static DepthBlur,
        &'static DynamicUniformIndex<DepthBlur>,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, prepass, _settings, settings_index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipeline_res = world.resource::<DepthBlurPipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_res.pipeline_id) else {
            return Ok(());
        };
        let uniforms = world.resource::<ComponentUniforms<DepthBlur>>();
        let Some(settings_binding) = uniforms.uniforms().binding() else {
            return Ok(());
        };
        // No prepass depth this frame → nothing to key blur on; skip (pass-through).
        let Some(depth_view) = prepass.depth_view() else {
            return Ok(());
        };

        let post_process = view_target.post_process_write();
        let bind_group = render_context.render_device().create_bind_group(
            "depth_blur_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
            &BindGroupEntries::sequential((
                post_process.source,
                &pipeline_res.sampler,
                depth_view,
                settings_binding.clone(),
            )),
        );

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("depth_blur_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_render_pipeline(pipeline);
        render_pass.set_bind_group(0, &bind_group, &[settings_index.index()]);
        render_pass.draw(0..3, 0..1);
        Ok(())
    }
}

#[derive(Resource)]
struct DepthBlurPipeline {
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
        "depth_blur_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                uniform_buffer::<DepthBlur>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    let shader = asset_server.load(SHADER_ASSET_PATH);
    let vertex_state = fullscreen_shader.to_vertex_state();
    let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("depth_blur_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: vertex_state,
        fragment: Some(FragmentState {
            shader,
            // HDR camera → the post-process ping-pong textures are Rgba16Float.
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba16Float,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(DepthBlurPipeline { layout, sampler, pipeline_id });
}
