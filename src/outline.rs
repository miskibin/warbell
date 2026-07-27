//! Toon **outline** post-process — darkens object/feature edges so each low-poly object
//! reads with a crisp defined silhouette (the reference's "every object pops" look). A
//! fullscreen pass after tonemapping that samples the prepass depth + normal, detects
//! discontinuities (silhouettes via depth, hard creases via normals) and darkens them.
//!
//! Same shape as [`crate::dof`] — a Bevy 0.19 `Core3d`-schedule render system (not a `ViewNode`)
//! — with one extra binding: the prepass NORMAL texture (`ViewPrepassTextures::normal_view`).
//! Ordered BEFORE the DoF pass so distant outlines soften with the depth-of-field blur.

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

const SHADER_ASSET_PATH: &str = "shaders/outline.wgsl";
/// Also stale vs the camera's real `near: 0.04` (`scene::default_projection`) — but harmless
/// here, unlike `dof.rs`: the shader only uses it in the RATIO `abs(dn - dc) / dc`, where a
/// constant scale factor cancels. Deliberately NOT "fixed" to 0.04 — the only behavioural delta
/// would be the `max(dc, 0.1)` guard starting to bite on first-person geometry held at the lens,
/// i.e. re-tuning the FP weapon's outline for no gain. Left as-is on purpose.
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
/// cartoony (user feedback, twice — the band is wide and the floor low on purpose). Starts at
/// ~45° off-sun (cos 0.70), down to 15% strength when staring straight at it. Runs ungated
/// (pure view cosmetics, like the other render systems).
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
        let t = ((align - 0.70) / (0.95 - 0.70)).clamp(0.0, 1.0);
        let fade = 1.0 - 0.85 * (t * t * (3.0 - 2.0 * t)); // smoothstep ease
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
        // 0.19: render graph is ECS systems. Outline runs in the Core3d `PostProcess` stage, ordered
        // `.after(smaa)` and `.before(dof_pass)` → the pinned chain tonemapping → smaa → outline →
        // dof. Both orderings matter: `.after(smaa)` (SMAA is also a `post_process_write` ping-pong
        // pass in PostProcess — leaving it unordered vs ours races the executor and FLICKERS, see
        // dof.rs), and `.before(dof_pass)` so the darkened edges blur with the DoF instead of landing
        // crisp on already-blurred pixels. `ViewQuery` validation-skips when the view lacks `Outline`
        // (the Low preset strips it).
        render_app.add_systems(
            Core3d,
            outline_pass
                .in_set(Core3dSystems::PostProcess)
                .after(smaa)
                .before(crate::dof::dof_pass),
        );
    }
}

/// The toon-outline fullscreen post pass, as a Core3d render-schedule system (0.19's replacement
/// for the old `ViewNode`). Needs BOTH prepass textures (depth + normal); skips the frame if
/// either is missing.
pub(crate) fn outline_pass(
    view: ViewQuery<(
        &ViewTarget,
        &ViewPrepassTextures,
        &Outline,
        &DynamicUniformIndex<Outline>,
    )>,
    pipeline_res: Res<OutlinePipeline>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<Outline>>,
    mut ctx: RenderContext,
) {
    let (view_target, prepass, _settings, settings_index) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_res.pipeline_id) else {
        return;
    };
    let Some(settings_binding) = uniforms.uniforms().binding() else {
        return;
    };
    // Needs BOTH prepass textures; skip (pass-through) if either is missing this frame.
    let (Some(depth_view), Some(normal_view)) = (prepass.depth_view(), prepass.normal_view())
    else {
        return;
    };

    let post_process = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
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

    let mut render_pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("outline_pass"),
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
pub(crate) struct OutlinePipeline {
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
