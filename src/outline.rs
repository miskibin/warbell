//! Toon **outline** post-process — darkens object/feature edges so each low-poly object
//! reads with a crisp defined silhouette (the reference's "every object pops" look). A
//! fullscreen pass after tonemapping that samples the prepass depth + normal, detects
//! discontinuities (silhouettes via depth, hard creases via normals) and darkens them.
//!
//! Built on the same Bevy 0.18 `custom_post_processing` pattern as [`crate::depth_blur`],
//! with one extra binding: the prepass NORMAL texture (`ViewPrepassTextures::normal_view`).
//! Runs BEFORE the depth-blur so distant outlines soften with the DoF.

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

const SHADER_ASSET_PATH: &str = "shaders/outline.wgsl";
const NEAR: f32 = 0.1;

/// Per-camera outline settings (also the shader uniform).
#[derive(Component, Default, Clone, Copy, ExtractComponent, ShaderType)]
pub struct Outline {
    /// Edge sample offset, in pixels (line thickness).
    pub thickness: f32,
    /// Relative depth jump that counts as a silhouette edge.
    pub depth_threshold: f32,
    /// `1 - dot(normal)` break that counts as a crease edge.
    pub normal_threshold: f32,
    /// How dark the outline goes (0 = off, 1 = black).
    pub strength: f32,
    /// Camera near plane (depth → distance).
    pub near: f32,
    /// Sun-gaze multiplier on `strength`, driven per-frame by [`fade_outline_toward_sun`]
    /// (1 = full outlines; eases down when the camera looks into the sun, where backlit
    /// silhouettes + full-strength edges read cartoony). Not a hand-tuned knob.
    pub sun_fade: f32,
}

/// A SUBTLE default: silhouette-only (a high `normal_threshold` suppresses the per-facet
/// crease lines that read cel-shaded), low strength — objects are gently defined against
/// what's behind them, not cartoon-outlined. Crank it (or lower the crease sens) in the F1
/// panel if you want a bolder toon look; set strength 0 to disable entirely.
pub fn default_outline() -> Outline {
    Outline {
        thickness: 1.2,
        depth_threshold: 0.06,
        normal_threshold: 1.3,
        strength: 0.15,
        near: NEAR,
        sun_fade: 1.0,
    }
}

/// Ease the outline off as the camera turns into the sun: against the bright sky every prop is
/// already a high-contrast backlit silhouette, and stacking the full edge-darkening on top reads
/// cartoony (user feedback). No effect below ~37° off-sun (cos 0.80), eases to 30% strength when
/// staring straight at it. Runs ungated (pure view cosmetics, like the other render systems).
fn fade_outline_toward_sun(
    sun: Query<&GlobalTransform, With<crate::scene::Sun>>,
    mut cams: Query<(&GlobalTransform, &mut Outline)>,
) {
    let Ok(sun_tf) = sun.single() else { return };
    // The day/night cycle parks the sun at `sun_dir * 120` looking at the origin, so its
    // translation IS the direction to the sun.
    let to_sun = sun_tf.translation().normalize_or_zero();
    for (cam_tf, mut o) in cams.iter_mut() {
        let fwd = cam_tf.rotation() * Vec3::NEG_Z;
        let align = fwd.dot(to_sun).max(0.0);
        let t = ((align - 0.80) / (0.97 - 0.80)).clamp(0.0, 1.0);
        let fade = 1.0 - 0.7 * (t * t * (3.0 - 2.0 * t)); // smoothstep ease
        if (o.sun_fade - fade).abs() > 1e-3 {
            o.sun_fade = fade;
        }
    }
}

pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ExtractComponentPlugin::<Outline>::default(),
            UniformComponentPlugin::<Outline>::default(),
        ))
        .add_systems(Update, fade_outline_toward_sun);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(RenderStartup, init_pipeline);
        render_app
            .add_render_graph_node::<ViewNodeRunner<OutlineNode>>(Core3d, OutlineLabel)
            // Tonemapping → Outline → DoF: outline must write its darkened edges BEFORE the
            // DoF pass reads the screen, so out-of-focus outlines blur with everything else
            // (otherwise the order is undefined and a crisp outline can land on blurred pixels).
            // DofPlugin owns the DoF → EndMainPassPostProcessing edge.
            .add_render_graph_edges(
                Core3d,
                (Node3d::Tonemapping, OutlineLabel, crate::dof::DofLabel),
            );
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct OutlineLabel;

#[derive(Default)]
struct OutlineNode;

impl ViewNode for OutlineNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static ViewPrepassTextures,
        &'static Outline,
        &'static DynamicUniformIndex<Outline>,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        (view_target, prepass, _settings, settings_index): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let pipeline_res = world.resource::<OutlinePipeline>();
        let pipeline_cache = world.resource::<PipelineCache>();
        let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_res.pipeline_id) else {
            return Ok(());
        };
        let uniforms = world.resource::<ComponentUniforms<Outline>>();
        let Some(settings_binding) = uniforms.uniforms().binding() else {
            return Ok(());
        };
        // Needs BOTH prepass textures; skip (pass-through) if either is missing this frame.
        let (Some(depth_view), Some(normal_view)) = (prepass.depth_view(), prepass.normal_view())
        else {
            return Ok(());
        };

        let post_process = view_target.post_process_write();
        let bind_group = render_context.render_device().create_bind_group(
            "outline_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
            &BindGroupEntries::sequential((
                post_process.source,
                &pipeline_res.sampler,
                depth_view,
                normal_view,
                settings_binding.clone(),
            )),
        );

        let mut render_pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("outline_pass"),
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
struct OutlinePipeline {
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
        "outline_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
                texture_2d(TextureSampleType::Float { filterable: true }),
                uniform_buffer::<Outline>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor::default());
    let shader = asset_server.load(SHADER_ASSET_PATH);
    let vertex_state = fullscreen_shader.to_vertex_state();
    let pipeline_id = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("outline_pipeline".into()),
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
    commands.insert_resource(OutlinePipeline { layout, sampler, pipeline_id });
}
