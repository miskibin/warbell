# Bevy 0.18.1: polished static 3D scene — verified APIs (camera/HDR, post-FX, fog, SSAO/TAA, lights, custom Material + WGSL, meshes, instancing)

All API names below are verified against docs.rs/bevy/0.18.1 AND against a real compiling Bevy 0.18.1 project at `D:\tileworld-bevy` (the existing tileworld port — 302 tests green, uses all of these). File:line references point at that project as a working source of truth.

## 0. Cargo.toml — features

Verified working dep line (`D:\tileworld-bevy\crates\game\Cargo.toml:11`):
```toml
bevy = { version = "0.18.1", features = ["wav", "mp3"] }   # wav/mp3 are only for audio
bevy_egui = "0.39.1"                                        # optional: leva-style tweak panel
```

Key fact: **Bloom, DepthOfField, SSAO, TAA, SMAA, Tonemapping, DistanceFog, VolumetricFog, Atmosphere are all reachable with DEFAULT features** (72 of 161 default-on). They live in default sub-crates:
- Bloom / DoF / Tonemapping / MotionBlur → `bevy_core_pipeline` + `bevy_post_process` (default)
- SSAO / DistanceFog / VolumetricFog / Atmosphere / DirectionalLight → `bevy_pbr` (default)
- TAA / SMAA → `bevy_anti_alias` (default-on)
- `tonemapping_luts`, `smaa_luts`, `ktx2`, `zstd_rust` are default-on (needed for AgX/TonyMcMapface LUTs + KTX2 env maps)
- Opt-in (NOT default): `dlss` (NVIDIA-only, native-only), `experimental_pbr_pcss` (percentage-closer soft shadows). DoF/SSAO/TAA do NOT need any extra flag.

So a static showcase needs **no non-default features** unless you want DLSS or PCSS.

## 1. App + window + Camera3d (HDR, orbit-fixed, MSAA)

Window (`main.rs:64-83`):
```rust
use bevy::prelude::*;
App::new().add_plugins(
    DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "scene".into(),
            present_mode: bevy::window::PresentMode::AutoVsync,
            ..default()
        }),
        ..default()
    }),
).run();
```

Camera (verified pattern at `player_ctl.rs:178-189`, extended with the showcase components):
```rust
use bevy::prelude::*;
use bevy::render::view::Hdr;            // HDR is a MARKER component in 0.18 (NOT Camera{hdr:true})
use bevy::camera::Exposure;            // moved to bevy::camera in 0.18
use bevy::core_pipeline::tonemapping::Tonemapping;

commands.spawn((
    Camera3d::default(),
    // 0.18: projection is its own component, set via Projection::from(...).
    Projection::from(PerspectiveProjection { fov: 32f32.to_radians(), ..default() }),
    Transform::from_xyz(0.0, 18.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),  // fixed orbit pose
    Hdr,                                // REQUIRED for Bloom/Atmosphere; just insert the marker
    Exposure { ev100: Exposure::BLENDER }, // or Exposure::SUNLIGHT (15.0) / OVERCAST (12.0)
    Tonemapping::AgX,
    Msaa::Sample4,                      // 0.18: Msaa is a per-camera COMPONENT. Off / Sample2/4/8
));
```
CRITICAL 0.18 changes vs 0.15:
- `Camera { hdr: true }` is GONE → insert the `bevy::render::view::Hdr` marker component instead.
- `Msaa` is a **per-camera Component** (was a Resource in ≤0.16). Default is `Msaa::Sample4`.
- Projection is set with `Projection::from(PerspectiveProjection{..})`; read back with `if let Projection::Perspective(p) = ..` (`player_ctl.rs:401`).
- `Exposure` lives at `bevy::camera::Exposure` with one field `ev100: f32` and constants `SUNLIGHT=15.0`, `OVERCAST=12.0`, `INDOOR=7.0`, `BLENDER=9.7` (Default). Set via `Exposure { ev100: 12.0 }` or `Exposure::from_physical_camera(params)`.

NOTE: **MSAA is mutually exclusive with SSAO and TAA** — both read the depth/normal prepass which MSAA would multisample. If you add SSAO or TAA you must set `Msaa::Off` (see §6) and rely on SMAA/TAA for AA instead.

## 2. Tonemapping (AgX / TonyMcMapface) + exposure

Import: `use bevy::core_pipeline::tonemapping::Tonemapping;`
Insert `Tonemapping::AgX` (cinematic, desaturated highlights — the film grade) or `Tonemapping::TonyMcMapface` (punchier, Bevy's recommended default with Bloom). Other variants: `AcesFitted`, `Reinhard`, `ReinhardLuminance`, `SomewhatBoringDisplayTransform`, `BlenderFilmic`, `None`.
Exposure is the separate `Exposure` component (§1) — `ev100` higher = darker image. AgX/TonyMcMapface need `tonemapping_luts` feature (default-on); without it they panic at startup for a missing LUT.
Verified mapping enum (`post_fx.rs:166-175`):
```rust
match self {
    TonemapChoice::AgX => Tonemapping::AgX,
    TonemapChoice::TonyMcMapface => Tonemapping::TonyMcMapface,
    TonemapChoice::AcesFitted => Tonemapping::AcesFitted,
    TonemapChoice::None => Tonemapping::None,
}
```

## 3. Bloom

Import (MOVED in 0.18): `use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};` (was `bevy::core_pipeline::bloom` in ≤0.17).
Requires the `Hdr` component on the camera (auto-required). **Not WebGL2-compatible.**
Fields: `intensity: f32` (default 0.15), `low_frequency_boost: f32`, `low_frequency_boost_curvature: f32`, `high_pass_frequency: f32` (default 1.0), `prefilter: BloomPrefilter` (has `.threshold`, `.threshold_softness`), `composite_mode: BloomCompositeMode` (`EnergyConserving` | `Additive`), `max_mip_dimension: u32`, `scale: Vec2`.
Presets (assoc. consts): `Bloom::NATURAL` (default look), `Bloom::ANAMORPHIC`, `Bloom::OLD_SCHOOL`, `Bloom::SCREEN_BLUR`.
Verified builder (`post_fx.rs:530-537`, ship defaults intensity 0.18 / low_freq 0.7 / threshold 0.55):
```rust
use bevy::post_process::bloom::Bloom;
let mut bloom = Bloom { intensity: 0.18, low_frequency_boost: 0.7, ..Bloom::NATURAL };
bloom.prefilter.threshold = 0.55;     // luminance threshold before a pixel blooms
commands.entity(cam).insert(bloom);
```

## 4. Depth of field

Import (MOVED in 0.18 to post_process): `use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};` (was `bevy::core_pipeline::dof` in ≤0.16). Plugin `DepthOfFieldPlugin` is in DefaultPlugins.
Struct is named `DepthOfField` (the old `DepthOfFieldSettings` name is gone). Fields: `mode: DepthOfFieldMode`, `focal_distance: f32` (meters), `sensor_height: f32` (default 0.01866 = Super-35), `aperture_f_stops: f32` (smaller = more blur), `max_circle_of_confusion_diameter: f32`, `max_depth: f32`.
`DepthOfFieldMode`: `Gaussian` (WebGL2-friendly) | `Bokeh` (needs R32 storage textures — desktop/WebGPU only).
Verified (`post_fx.rs:345-373, 506-508`):
```rust
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
DepthOfField {
    mode: DepthOfFieldMode::Gaussian,
    focal_distance: 30.0,             // world units to the in-focus plane
    aperture_f_stops: 3.1,            // smaller = stronger blur (bevy ex uses 0.125)
    max_depth: 100.0,                 // clamps infinite CoC for skybox/bg
    ..default()
}
```
For a STATIC scene just pick a fixed `focal_distance` to the hero subject; no per-frame driver needed.

## 5. Distance fog + atmospheric/volumetric fog

**DistanceFog** (per-camera component): `use bevy::pbr::{DistanceFog, FogFalloff};`
Fields: `color: Color`, `directional_light_color: Color` (sun-glow tint; `Color::NONE` disables), `directional_light_exponent: f32`, `falloff: FogFalloff`.
`FogFalloff`: `Linear { start, end }` | `Exponential { density }` | `ExponentialSquared { density }` (= three.js FogExp2) | `Atmospheric { extinction, inscattering }`.
Verified (`lighting.rs:140-144`, density 0.02):
```rust
use bevy::pbr::{DistanceFog, FogFalloff};
DistanceFog {
    color: Color::srgb_u8(0xd6, 0xc6, 0xa0),
    falloff: FogFalloff::ExponentialSquared { density: 0.02 },
    ..default()
}
```

**Volumetric fog / god-rays** (0.15+, present in 0.18): `use bevy::pbr::{VolumetricFog, FogVolume, VolumetricLight};` (also re-exported from `bevy::light`). Pattern:
- `VolumetricFog` component on the Camera3d (enables the effect / light shafts).
- `FogVolume` component on a separate entity = a bounding box (Transform-scaled) defining where fog renders.
- `VolumetricLight` component on a `DirectionalLight` with `shadows_enabled: true` to make it cast shafts.
```rust
commands.entity(cam).insert(VolumetricFog { ..default() });
commands.spawn((FogVolume::default(), Transform::from_scale(Vec3::splat(80.0))));
commands.entity(sun).insert(VolumetricLight);   // sun must have shadows_enabled
```

**Atmosphere (NEW headline feature in 0.18)** — procedural sky + aerial perspective, integrates with volumetric fog: `use bevy::pbr::Atmosphere;`. Constructor `Atmosphere::earthlike(scattering_medium_handle)` where the handle is an `Assets<bevy::pbr::ScatteringMedium>` asset. Requires the `Hdr` component (auto-inserts `AtmosphereSettings` + `Hdr`). Attach to the Camera3d. In 0.18 the atmosphere now applies orange/red sunlight near the horizon and affects object shading. For a static showcase this gives a free, beautiful sky + horizon glow:
```rust
commands.entity(cam).insert(Atmosphere::earthlike(media.add(ScatteringMedium::earthlike())));
```

## 6. SSAO + TAA (+ required setup)

**SSAO**: `use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};`. Add `ScreenSpaceAmbientOcclusionPlugin` (in DefaultPlugins). Fields: `quality_level` (`Low`|`Medium`|`High`|`Ultra`|`Custom{..}`), `constant_object_thickness: f32`. Auto-requires `DepthPrepass` + `NormalPrepass`. **Not on WebGL2/WebGPU.** Pairs best with TAA to cut noise.
**TAA**: `use bevy::anti_alias::taa::{TemporalAntiAliasing, TemporalAntiAliasPlugin};`. Single field `reset: bool`. Auto-inserts `TemporalJitter`, `MipBias`, `DepthPrepass`, `MotionVectorPrepass`.
**SMAA** (cheaper, WebGL2-safe alternative): `use bevy::anti_alias::smaa::{Smaa, SmaaPreset};` → `Smaa { preset: SmaaPreset::High }` (`post_fx.rs:315`).

REQUIRED: SSAO and TAA both need **`Msaa::Off`** on the camera. Verified attach block (`lighting.rs:111-135`):
```rust
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
commands.entity(cam).insert((
    Msaa::Off,                          // mandatory for SSAO/TAA
    ScreenSpaceAmbientOcclusion {
        quality_level: ScreenSpaceAmbientOcclusionQualityLevel::Medium,
        ..default()
    },
    DepthPrepass, NormalPrepass,        // SSAO #[require]s these; explicit = clearer
));
// optionally:  use bevy::anti_alias::taa::TemporalAntiAliasing;  cam.insert(TemporalAntiAliasing::default());
```
For AA when MSAA is off, use TAA (best quality, needs motion vectors — fine for static if the camera is still, but a still TAA frame can ghost on the first frames; set `reset:true` once) or SMAA (WebGL2-safe, no prepass).

## 7. DirectionalLight (shadows + cascades) + AmbientLight

DirectionalLight + cascade config (`map_render.rs:448-462`):
```rust
use bevy::pbr::{DirectionalLight, DirectionalLightShadowMap};
use bevy::light::CascadeShadowConfigBuilder;        // 0.18: in bevy::light
commands.spawn((
    DirectionalLight { shadows_enabled: true, illuminance: 10_000.0, ..default() },
    CascadeShadowConfigBuilder {
        num_cascades: 2,                // default 4; fewer = cheaper
        maximum_distance: 60.0,
        first_cascade_far_bound: 16.0,
        ..default()
    }.build(),
    Transform::from_xyz(40.0, 80.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
));
app.insert_resource(DirectionalLightShadowMap { size: 2048 });  // shadow map res (default 1024/2048)
```
`illuminance` is in lux; use `bevy::light::light_consts::lux::*` constants (e.g. `lux::AMBIENT_DAYLIGHT`, `lux::FULL_DAYLIGHT`) (`day_night.rs:277`). Optionally `ShadowFilteringMethod` (`Hardware2x2`|`Gaussian`|`Temporal`) as a camera component for softer shadow edges.

AmbientLight — **CHANGED in 0.18**: the scene-wide ambient is now the `GlobalAmbientLight` RESOURCE; `AmbientLight` is a per-camera Component override (`day_night.rs:133-139`):
```rust
app.insert_resource(GlobalAmbientLight {
    color: Color::srgb(0.9, 0.92, 1.0),
    brightness: 200.0,                 // lux-ish; tune to taste
    affects_lightmapped_meshes: true,
});
```

For best material quality, add image-based lighting via `GeneratedEnvironmentMapLight` (camera component) which GPU-prefilters a cubemap `Handle<Image>` into diffuse+specular IBL — full procedural-cubemap recipe at `lighting.rs:121-234` (`GeneratedEnvironmentMapLight { environment_map, intensity: 900.0, ..default() }`). `EnvironmentMapLight` is the alternative if you have pre-baked KTX2 cubemaps.

## 8. Custom terrain Material (ExtendedMaterial + WGSL)

The project uses **`ExtendedMaterial<StandardMaterial, MyExt>`** (keeps full PBR + shadows + fog, you only override the fragment to inject world-space color mottle on top of vertex colors). This is exactly the "procedural mottle + hue/value variation on vertex colors" ask.

Rust side (`terrain_material.rs:22-108`, verbatim shape):
```rust
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;                 // 0.18: ShaderRef is in bevy::shader

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct TerrainParams { pub kind: [Vec4; 6] }   // std140-friendly uniform

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct TerrainExtension {
    #[uniform(100)]                            // bindings start at 100 to dodge base mat's 0..99
    pub params: TerrainParams,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub detail: Handle<Image>,
}
impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef { "shaders/terrain.wgsl".into() }
    // also overridable: vertex_shader(), prepass_fragment_shader(), deferred_fragment_shader()
}

// register:
app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
// spawn:
commands.spawn((Mesh3d(mesh_h), MeshMaterial3d(mat_assets.add(ExtendedMaterial {
    base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, cull_mode: None, ..default() },
    extension: TerrainExtension { params, detail },
}))));
```

WGSL entry-point contract that 0.18 expects (verbatim from `assets/shaders/terrain.wgsl`):
```wgsl
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}
// extension bindings: group is the #{MATERIAL_BIND_GROUP} preprocessor token, NOT a literal number
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> terrain: TerrainParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var detail_tex: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var detail_samp: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let wp = in.world_position.xz;                 // WORLD-space so the look is continuous
    var rgb = pbr_input.material.base_color.rgb;    // vertex color already folded in here
    // (1) fine 3-octave value mottle:
    let m = ter_noise(wp*0.5)*0.55 + ter_noise(wp*1.7)*0.30 + ter_noise(wp*5.5)*0.15;
    rgb *= 0.80 + m*0.40;
    // (2) large-scale hue + value drift (cure for flat green):
    let big = ter_noise(wp*0.05)*0.6 + ter_noise(wp*0.14)*0.4;
    let hue = ter_noise(wp*0.028 + vec2(11.0,11.0));
    rgb += (big-0.5) * variation * vec3(0.22,0.14,-0.14);
    rgb *= 1.0 + (hue-0.5) * variation * 0.40;
    pbr_input.material.base_color = vec4(max(rgb, vec3(0.0)), pbr_input.material.base_color.a);
    // light it, then run the SAME post-lighting (fog/tonemap/deband) the default shader does:
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else { out.color = pbr_input.material.base_color; }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
```
WGSL gotchas confirmed: the bind group is `@group(#{MATERIAL_BIND_GROUP})` (a preprocessor token, expands to 2), bindings must start ≥100 to avoid StandardMaterial's, and you must call `main_pass_post_lighting_processing` yourself or fog/tonemap won't apply. The shader is loaded via the asset server from `assets/shaders/*.wgsl` (no manual `Shader` handle).
Per-vertex data (biome id) is threaded via `Mesh::ATTRIBUTE_UV_1` → read in WGSL as `in.uv_b`.

If you DON'T need to extend StandardMaterial (fully custom look), implement the plain `Material` trait instead (same `AsBindGroup` derive + `fragment_shader()`), register with `MaterialPlugin::<MyMaterial>::default()`, and override `Material::alpha_mode/opaque_render_method/...` as needed.

## 9. Custom Mesh (positions/normals/uvs/vertex-colors) + merging for trees

Build (`map_render.rs:17-19, 399-407`):
```rust
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};   // 0.18: Mesh primitives live in bevy::mesh
let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);   // Vec<[f32;3]>
mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL,   normals);     // Vec<[f32;3]>
mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR,    colors);      // Vec<[f32;4]> (RGBA, linear)
mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0,     uvs);         // Vec<[f32;2]>
mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1,     uvs_b);       // extra per-vertex data
mesh.insert_indices(Indices::U32(indices));
// flat-shading helpers:
// mesh.duplicate_vertices(); mesh.compute_flat_normals();  // NOTE: compute_flat_normals PANICS on an indexed mesh → duplicate_vertices() FIRST
// mesh.compute_normals();                                  // smooth normals (handles indexed)
```
ATTRIBUTE_COLOR auto-enables the `VERTEX_COLORS` shader-def — a white `StandardMaterial { base_color: Color::WHITE }` then shows the per-vertex colors with no shader work (`map_props.rs:99-111`).

Merging primitive meshes into one tree model (`map_props.rs:113-155`, verified):
```rust
// tag a primitive with a uniform color:
fn tinted(mut m: Mesh, c: [f32;4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
// merge parts that share the SAME attribute set into ONE mesh (so batching holds):
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().unwrap();
    for p in it { base.merge(&p).expect("parts share attributes"); }  // Mesh::merge → Result in 0.18
    base
}
// e.g. trunk(Cylinder) + canopy(Cone) → merged(vec![tinted(trunk, BROWN), tinted(canopy, GREEN)])
// build a primitive mesh + transform it: Cylinder::default().mesh().build().translated_by(off).rotated_by(quat)
```
`Mesh::merge` returns `Result` in 0.18 (must `.expect()`/`?`); all merged parts must carry the same attributes (give every part a COLOR via `tinted` before merging).

## 10. Instancing many low-poly meshes

For a static showcase the simplest path is best and what this project does: **spawn N entities that all share ONE `Handle<Mesh>` and ONE `Handle<StandardMaterial>`** (clone the handles — they're cheap Arc-like refs). Bevy's renderer **auto-batches/auto-instances** entities with the same mesh+material+pipeline into few draw calls (GPU instancing happens under the hood), so 18k props render fine (the project scatters ~18k this way). Mark static props `NotShadowCaster` (`bevy::pbr::NotShadowCaster`) to skip them in the shadow pass — big win.
```rust
let mesh_h = meshes.add(tree_mesh);              // build ONCE
let mat_h  = materials.add(white_vcolor_mat);    // build ONCE
for t in placements {
    commands.spawn((Mesh3d(mesh_h.clone()), MeshMaterial3d(mat_h.clone()),
                    Transform::from(t), bevy::pbr::NotShadowCaster));
}
```
Better paths only if you hit a CPU/draw bottleneck: (a) merge static props into a few big baked meshes (fewer entities to extract), or (b) a custom instanced-rendering pipeline (`RenderMeshInstances` / a manual instance buffer — see Bevy's `examples/shader/custom_shader_instancing.rs`). For a polished STATIC scene, sharing handles is almost always sufficient — don't reach for a custom instancing pipeline first.

---
### Source URLs (verify if needed)
- Bevy 0.18 release: https://bevy.org/news/bevy-0-18/
- 0.17→0.18 migration: https://bevy.org/learn/migration-guides/0-17-to-0-18/
- Bloom: https://docs.rs/bevy/0.18.1/bevy/post_process/bloom/struct.Bloom.html
- DoF: https://docs.rs/bevy/0.18.1/bevy/post_process/dof/index.html
- DistanceFog: https://docs.rs/bevy/0.18.1/bevy/pbr/struct.DistanceFog.html
- SSAO: https://docs.rs/bevy/0.18.1/bevy/pbr/struct.ScreenSpaceAmbientOcclusion.html
- TAA: https://docs.rs/bevy/0.18.1/bevy/anti_alias/taa/struct.TemporalAntiAliasing.html
- ExtendedMaterial: https://docs.rs/bevy/0.18.1/bevy/pbr/struct.ExtendedMaterial.html
- Atmosphere: https://docs.rs/bevy/0.18.1/bevy/pbr/struct.Atmosphere.html
- Hdr: https://docs.rs/bevy/0.18.1/bevy/render/view/struct.Hdr.html
- Exposure: https://docs.rs/bevy/0.18.1/bevy/camera/struct.Exposure.html
- features: https://docs.rs/crate/bevy/0.18.1/features
- Working reference project: D:\tileworld-bevy (Bevy 0.18.1, compiles, 302 tests green)

## Exact Constants
- Camera FOV: 32 degrees (telephoto-ish orbit) — `CAM_FOV_DEG` (player_ctl.rs)
- Exposure::ev100 presets: SUNLIGHT=15.0, OVERCAST=12.0, INDOOR=7.0, BLENDER=9.7 (Default)
- Bloom: intensity default 0.15 (project ships 0.18), high_pass_frequency default 1.0, low_frequency_boost ship 0.7, prefilter.threshold ship 0.55; base = Bloom::NATURAL
- DepthOfField: sensor_height default 0.01866 (Super-35), mode Gaussian (web) / Bokeh (desktop); project bokeh→f-stop maps ship bokeh_scale 7.0 → ~f/3.1, focus_range 70.0 world units
- DistanceFog: FogFalloff::ExponentialSquared { density: 0.02 } (FOG_DENSITY = 0.02), starting color #d6c6a0
- MotionBlur: shutter_angle 0.25, samples 2 (MOTION_BLUR_SHUTTER / MOTION_BLUR_SAMPLES)
- IBL: GeneratedEnvironmentMapLight intensity 900.0 (IBL_INTENSITY); cubemap 64px PoT square faces, Rgba16Float, 6 layers, Cube view
- DirectionalLight: shadows_enabled true, CascadeShadowConfigBuilder { num_cascades: 2, maximum_distance: 60.0, first_cascade_far_bound: 16.0 }; DirectionalLightShadowMap size 2048; illuminance via bevy::light::light_consts::lux
- SSAO: ScreenSpaceAmbientOcclusionQualityLevel::Medium; requires Msaa::Off + DepthPrepass + NormalPrepass
- SMAA: SmaaPreset::High
- ColorGrading (grade): global.exposure = brightness (ship -0.02), midtones.contrast = 1.0 + contrast (ship 0.12), global.post_saturation = 1.0 + base_saturation (ship 1.18)
- Material extension bindings: uniform 100, texture 101 (2d_array), sampler 102 (start at 100 to avoid StandardMaterial 0..99)
- Mesh attributes used: ATTRIBUTE_POSITION, ATTRIBUTE_NORMAL, ATTRIBUTE_COLOR (Float32x4 RGBA linear), ATTRIBUTE_UV_0, ATTRIBUTE_UV_1 (per-vertex aux data → in.uv_b in WGSL)
- Cargo features: NONE beyond default needed for bloom/dof/ssao/taa/smaa/fog/atmosphere/pbr; project adds only ["wav","mp3"] (audio). Opt-ins available: "dlss", "experimental_pbr_pcss". Default-on relevant: bevy_post_process, bevy_core_pipeline, bevy_pbr, bevy_anti_alias, tonemapping_luts, smaa_luts, ktx2, zstd_rust (72/161 features default)

## Bevy 0.18 Notes
Practical replication guide for a polished STATIC Bevy 0.18.1 scene, distilled from a known-compiling reference (D:\tileworld-bevy):

CAMERA STACK (one entity): `Camera3d::default()` + `Projection::from(PerspectiveProjection{fov,..})` + `Transform::looking_at` (fixed orbit pose) + `Hdr` (marker — REQUIRED for Bloom/Atmosphere, NOT `Camera{hdr:true}`) + `Exposure{ev100: 12.0}` + `Tonemapping::AgX` + then choose ONE AA path:
 - MSAA path: `Msaa::Sample4`, add Bloom + DoF + SMAA. Simplest, WebGL2-friendly. No SSAO/TAA.
 - Deferred-quality path: `Msaa::Off` + `ScreenSpaceAmbientOcclusion` + `DepthPrepass` + `NormalPrepass` + `TemporalAntiAliasing` (or `Smaa`). Best grounding, desktop-only (SSAO not on web).

POST-FX COMPONENTS (all on the camera entity): Bloom = `bevy::post_process::bloom::Bloom { intensity:0.18, low_frequency_boost:0.7, ..Bloom::NATURAL }` then set `.prefilter.threshold`. DoF = `bevy::post_process::dof::DepthOfField { mode: DepthOfFieldMode::Gaussian, focal_distance, aperture_f_stops, ..default() }` (Gaussian = WebGL2-safe; Bokeh = desktop). MotionBlur = `bevy::post_process::motion_blur::MotionBlur` if wanted. ColorGrading (`bevy::render::view::ColorGrading`) gives exposure/saturation/contrast (`.global.exposure`, `.global.post_saturation`, `.midtones.contrast`) — the BrightnessContrast/HueSaturation equivalent.

FOG: `bevy::pbr::DistanceFog { color, falloff: FogFalloff::ExponentialSquared{density:0.02} }` on the camera. For god-rays add `VolumetricFog` on camera + a `FogVolume` entity + `VolumetricLight` on the sun. NEW in 0.18: `Atmosphere::earthlike(medium_handle)` on the camera (needs `Hdr`) for a gorgeous procedural sky + horizon-tinted sun — strongly recommended for a static showcase.

LIGHTS: one `DirectionalLight { shadows_enabled:true, illuminance:10_000.0 }` + `CascadeShadowConfigBuilder{ num_cascades:2, maximum_distance:60.0, first_cascade_far_bound:16.0 }.build()` (from `bevy::light`) + `Transform::looking_at`. Set `DirectionalLightShadowMap{size:2048}` resource. Ambient = the `GlobalAmbientLight` RESOURCE (0.18 change — `AmbientLight` is now a per-camera component). For material richness add `GeneratedEnvironmentMapLight{environment_map, intensity:900.0}` (camera component) fed a procedural gradient cubemap (Rgba16Float, 6 PoT-square faces, cube view dimension) — full recipe at D:\tileworld-bevy\crates\game\src\lighting.rs:164-234.

CUSTOM TERRAIN MATERIAL: use `ExtendedMaterial<StandardMaterial, MyExt>` (keeps PBR/shadows/fog, override only the fragment). Rust: `#[derive(Asset, AsBindGroup, Clone, TypePath)]` on the extension, `#[uniform(100)]`/`#[texture(101,dimension="2d_array")]`/`#[sampler(102)]` bindings starting at 100, `impl MaterialExtension { fn fragment_shader() -> ShaderRef { "shaders/x.wgsl".into() } }`, register `MaterialPlugin::<ExtendedMaterial<StandardMaterial,MyExt>>::default()`, spawn with `MeshMaterial3d(mat)`. WGSL: import `bevy_pbr::pbr_fragment::pbr_input_from_standard_material` + `pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}` + `forward_io::{VertexOutput, FragmentOutput}`; entry `@fragment fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput`; bind with `@group(#{MATERIAL_BIND_GROUP}) @binding(100)`; mutate `pbr_input.material.base_color.rgb` with world-XZ noise mottle/hue drift, then `apply_pbr_lighting` + `main_pass_post_lighting_processing`. Full verbatim shader at D:\tileworld-bevy\assets\shaders\terrain.wgsl.

MESHES: `Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())` + `insert_attribute(Mesh::ATTRIBUTE_POSITION|NORMAL|COLOR|UV_0|UV_1, vec)` + `insert_indices(Indices::U32(..))` (from `bevy::mesh`). ATTRIBUTE_COLOR auto-enables vertex colors against a white StandardMaterial. For trees: build primitives via `Shape::default().mesh().build()`, position with `.translated_by`/`.rotated_by`, `tinted()` each (add a uniform COLOR attr), then `base.merge(&part)?` (returns Result in 0.18) into one mesh so batching holds. compute_flat_normals PANICS on indexed meshes — call duplicate_vertices() first.

INSTANCING: just spawn many entities sharing cloned `Mesh3d`/`MeshMaterial3d` handles — Bevy auto-batches/instances same mesh+material into few draw calls (18k props proven). Add `bevy::pbr::NotShadowCaster` to static props to skip the shadow pass. Only build a custom instancing pipeline if profiling demands it.

KEY 0.18 BREAKING CHANGES TO WATCH (vs 0.15): Msaa is a per-camera Component (not Resource); HDR is the `Hdr` marker component (not `Camera{hdr:true}`); ambient is the `GlobalAmbientLight` resource (AmbientLight is per-camera); Bloom moved to `bevy::post_process::bloom`; DoF moved to `bevy::post_process::dof` and renamed `DepthOfFieldSettings`→`DepthOfField`; `RenderTarget` is a separate component (not a Camera field); `Material::shadows_enabled`/`prepass_enabled` are now trait methods `enable_shadows()`/`enable_prepass()`; CascadeShadowConfigBuilder is in `bevy::light`; ShaderRef in `bevy::shader`. No non-default cargo features are required for any of bloom/dof/ssao/taa/smaa/fog/atmosphere/pbr — they're all default-on. dlss and experimental_pbr_pcss are the only opt-ins relevant here.