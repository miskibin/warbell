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
/// MUST equal the main camera's real near plane (`scene::default_projection`, `near: 0.04`).
/// Bevy builds `perspective_infinite_reverse_rh`, so the prepass depth is exactly
/// `near / z` — the shader inverts it as `near / d`, and any mismatch scales EVERY
/// distance the DoF sees by `NEAR / near_actual`.
///
/// This was 0.1 (the Bevy default near) until 2026-07-27. `scene.rs` dropped the camera to
/// 0.04 on 2026-06-26 for the first-person weapon near-plane slice, and this const was never
/// followed — so DoF read every distance **2.5× too far**: with `range 64`/`far_ramp 200` the
/// intended "sharp to 74u, max blur at 274u" actually ran "sharp to ~30u, FULL 8px bokeh from
/// ~110u out". The island is ±185u wide, so the entire coast was permanently at max blur —
/// which is both the "blur too strong" complaints (patched twice by widening `range`, treating
/// the symptom) and the "map edges look like the meshes never loaded" report. It also fed the
/// silhouette bleed over mountains, since a ridge at 120u and the terrain behind it at 200u
/// both clamped to `coc = 1` and averaged into each other across the edge.
const NEAR: f32 = 0.04;

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
    // 2026-07-08 (user: blur too strong — supersedes the earlier range-60/ramp-130/radius-9 pass):
    // range 42→64 widens the fully-sharp band so blur starts much farther from the hero, far_ramp
    // 130→200 melts distance in later + gentler, max_radius 13→8 softens the far bokeh to a whisper.
    //
    // 2026-07-27 RETUNE — mandatory, not taste: the `NEAR` fix above removed a 2.5× distance
    // inflation, so the numbers above (chosen to fight it) would now leave almost no far blur at
    // all. Re-solved against TRUE distances, with the depth softening deliberately shifted onto
    // the cinematic haze pass (`atmospherics.rs`) instead of bokeh — haze reads as air, bokeh
    // past ~5px reads as a broken mip.
    //
    // Solved for BOTH focal modes, which the first cut missed: `drive_dof_focus` drives `focal` to
    // camera→hero (≈10-12u) in third person but PINS it to a fixed 28 in free-cam (`scene.rs:748`),
    // so a tune validated only on the gameplay view leaves overview framing (marketing shots, the
    // boot pose) near max bokeh. Measured on the first attempt: `far_ramp` 140 saturated by ~196u
    // and destroyed 55-82% of far-field edge energy — the far treeline went to mush where a
    // `FOREST_NOBLUR` A/B still resolved individual conifers. `far_ramp` 140→175 moves saturation
    // out to ~247u (i.e. only at the 230u far plane) and `max_radius` 6→4.5 caps the worst case at
    // a soften rather than a smear:
    // Then measured again, which corrected the radius too. Detail retention past ~200u turned out
    // to be FLOORED, not linear in radius: features out there are 1-2px hazed conifers, so 4.5px
    // and 6px annihilate them equally (far-coast retention 0.17 vs 0.22 — noise). Anything that
    // makes the coast read has to be ≈2-3px. So `far_ramp` 175→200, which pushes saturation to
    // 272u — beyond the 230u far plane, i.e. the disc NEVER reaches full size on screen — and
    // `max_radius` 4.5→3.5:
    //   third person (focal ≈12): crisp to 56u · 0.8px @100u · 1.6px @150u · 3.0px @230u
    //   free-cam     (focal  28): crisp to 72u · 1.4px @150u · 2.8px @230u
    // Depth now separates by TONE (haze) with bokeh only taking the hard edge off, which is the
    // "mature" read; the previous tune destroyed 55-82% of far-field edge energy. The terrain-LOD
    // hand-off ring (150→176u, `worldmap.rs:3145`) sits at ~1.4-2px — soft enough to hide the
    // swap, sharp enough that the coast no longer reads as unloaded geometry.
    let mut d =
        Dof { focal: 28.0, range: 44.0, far_ramp: 200.0, max_radius: 3.5, near: NEAR, debug_view: 0.0 };
    // `FOREST_DOF="range,far_ramp,max_radius"` — startup override, mirroring `FOREST_ATMO` /
    // `FOREST_GODRAYS`. Added 2026-07-27: the far-field look is a JOINT tune of bokeh against
    // haze, and haze was already env-tunable while DoF needed a 6-minute rebuild per guess — so
    // every past blur pass was one-shot-per-build guesswork (which is partly how the stale `NEAR`
    // survived three "fix the blur" rounds). `focal` is excluded on purpose: it's overwritten every
    // frame by `scene::drive_dof_focus` (use `FOREST_FOCAL` to pin it, or `FOREST_NOBLUR` to kill
    // the pass outright). A 4th value sets `debug_view` to paint the raw CoC.
    if let Ok(s) = std::env::var("FOREST_DOF") {
        // `collect::<Option<_>>`, NOT `filter_map(..ok())`: dropping an unparseable field silently
        // SHIFTED the rest ("44,x,200,3.5" became range=44, far_ramp=200, max_radius=3.5 with no
        // warning). A non-finite value is rejected for the same reason — `NaN` survives `clamp` and
        // rides straight into the shader uniform.
        let v: Vec<f32> = s
            .split(',')
            .map(|p| p.trim().parse::<f32>().ok().filter(|f| f.is_finite()))
            .collect::<Option<Vec<f32>>>()
            .unwrap_or_default();
        if v.len() == 3 || v.len() == 4 {
            d.range = v[0];
            d.far_ramp = v[1];
            d.max_radius = v[2];
            if let Some(&dbg) = v.get(3) {
                d.debug_view = dbg;
            }
        } else {
            warn!("FOREST_DOF needs 3 or 4 comma floats (range,far_ramp,max_radius[,debug]) — ignored");
        }
    }
    d
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
