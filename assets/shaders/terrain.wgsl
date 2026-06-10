// Forest ground vision shader — a forest-only port of the TS `vision.ts`
// `onBeforeCompile` fragment injection, wired as
// `ExtendedMaterial<StandardMaterial, TerrainExtension>`.
//
// The base StandardMaterial carries the forest grass base colour (mesh
// ATTRIBUTE_COLOR) + roughness. THIS shader runs the original's three world-space
// layers on the base colour BEFORE lighting:
//   (1) a fine 3-octave value mottle,
//   (2) a large-scale analytic hue + value drift (cure for flat green),
//   (3) the grass detail-texture imprint on up-facing fragments, normalised by the
//       texture's mean luminance.
// Params (detailScale, detailStrength, variation, mean) come in one vec4 uniform.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    forward_io::{VertexOutput, FragmentOutput},
}

struct ForestParams {
    // x=detailScale, y=detailStrength, z=variation, w=meanLuminance
    params: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> forest: ForestParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var detail_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var detail_samp: sampler;

// Value noise (vision.ts terHash / terNoise) in world XZ.
fn ter_hash(p: vec2<f32>) -> f32 {
    return fract(sin(p.x * 127.1 + p.y * 311.7) * 43758.5453);
}

fn ter_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let a = ter_hash(i);
    let b = ter_hash(i + vec2<f32>(1.0, 0.0));
    let c = ter_hash(i + vec2<f32>(0.0, 1.0));
    let d = ter_hash(i + vec2<f32>(1.0, 1.0));
    let u = f * f * (3.0 - 2.0 * f);
    return mix(a, b, u.x) + (c - a) * u.y * (1.0 - u.x) + (d - b) * u.x * u.y;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let wp = in.world_position.xz;
    let detail_scale = forest.params.x;
    let detail_strength = forest.params.y;
    let variation = forest.params.z;
    let mean = forest.params.w;

    var rgb = pbr_input.material.base_color.rgb;

    // How green-dominant the base ground is (1 = grass/forest floor, 0 = snow / sand /
    // dirt cliff). The world island runs ONE of these materials over every biome's
    // vertex colours, so the hue-bearing grass layers below must fade out where the
    // ground isn't green — the warm yellow-green drift + green blade imprint were
    // reading as dirt smears on snowfields and cliff walls. Value-only layers stay.
    let green = clamp((rgb.g - max(rgb.r, rgb.b)) * 6.0 + 0.30, 0.0, 1.0);

    // The original vision.ts ground texture — a SUBTLE low-opacity gradient, three layers:
    // (1) fine value mottle (three octaves), amplitude softened a touch from the original.
    let ter_m = ter_noise(wp * 0.5) * 0.55 + ter_noise(wp * 1.7) * 0.30 + ter_noise(wp * 5.5) * 0.15;
    rgb *= 0.85 + ter_m * 0.30;

    // (2) large-scale analytic hue + value drift — the soft cloudy patches (cure for
    //     flat green): broad areas drift warm yellow-green ↔ cool deep-green + lighten.
    //     The hue push is gated to green ground; the value drift applies everywhere.
    let ter_big = ter_noise(wp * 0.05) * 0.6 + ter_noise(wp * 0.14) * 0.4;
    let ter_hue = ter_noise(wp * 0.028 + vec2<f32>(11.0, 11.0));
    rgb += (ter_big - 0.5) * variation * vec3<f32>(0.22, 0.14, -0.14) * green;
    rgb *= 1.0 + (ter_hue - 0.5) * variation * 0.40;

    // (3) soft grass detail imprint on up-facing fragments, normalised by mean luminance.
    //     Off-grass the imprint collapses to its luminance so snow keeps the blade-scale
    //     value texture without picking up the green cast.
    let top_face = step(0.5, in.world_normal.y);
    let det = textureSample(detail_tex, detail_samp, wp * detail_scale).rgb / max(mean, 0.01);
    let det_l = dot(det, vec3<f32>(0.2126, 0.7152, 0.0722));
    let det_c = mix(vec3<f32>(det_l), det, green);
    rgb *= mix(vec3<f32>(1.0), det_c, detail_strength * top_face);

    pbr_input.material.base_color = vec4<f32>(max(rgb, vec3<f32>(0.0)), pbr_input.material.base_color.a);

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
