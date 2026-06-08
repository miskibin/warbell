// Basic stylized water — a deliberately simple FRAGMENT override of
// `ExtendedMaterial<StandardMaterial, WaterExt>`. Builds the standard PBR input, gives
// the normal a single GENTLE broad wave (so the surface has a hint of life without the
// busy ripple/sun-glint/fresnel look), then runs the normal PBR lighting + post so fog
// and tonemap still apply. Matte-ish roughness (set on the Rust side) keeps it a soft
// flat sheet rather than a sharp mirror.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::globals,
}

struct WaterParams {
    // x = ripple amplitude (normal tilt), y = wave frequency, z = scroll speed, w = unused
    params: vec4<f32>,
    sky_tint: vec4<f32>, // unused in the basic shader (kept for binding compatibility)
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water: WaterParams;

// Two broad slow waves with analytic gradients → a smooth, calm surface normal.
// Returns vec3(height, dH/dx, dH/dz).
fn water_surface(p: vec2<f32>, t: f32) -> vec3<f32> {
    let freq = water.params.y;
    let speed = water.params.z;
    var h = 0.0;
    var gx = 0.0;
    var gz = 0.0;
    {
        let d = vec2<f32>(0.80, 0.60);
        let f = freq;
        let a = 0.6;
        let ph = dot(p, d) * f + t * speed;
        h += sin(ph) * a;
        let c = cos(ph) * a * f;
        gx += c * d.x; gz += c * d.y;
    }
    {
        let d = vec2<f32>(-0.50, 0.86);
        let f = freq * 1.6;
        let a = 0.35;
        let ph = dot(p, d) * f + t * speed * 1.3;
        h += sin(ph) * a;
        let c = cos(ph) * a * f;
        gx += c * d.x; gz += c * d.y;
    }
    return vec3<f32>(h, gx, gz);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let amp = water.params.x;
    let t = globals.time;
    let p = in.world_position.xz;

    // One gentle wave normal — just enough motion, no busy ripple field.
    let s = water_surface(p, t);
    let ripple_n = normalize(vec3<f32>(-s.y * amp, 1.0, -s.z * amp));
    pbr_input.N = normalize(pbr_input.N + ripple_n - vec3<f32>(0.0, 1.0, 0.0));
    pbr_input.world_normal = pbr_input.N;

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
