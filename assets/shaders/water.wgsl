// Stylized water — FRAGMENT override of `ExtendedMaterial<StandardMaterial, WaterExt>`.
// Builds the standard PBR input, then layers:
//   1. a multi-octave wave normal (two broad swells + mid chop + fine ripples that fade
//      with distance so the horizon never shimmers),
//   2. a shore-distance field baked by `worldmap::build` (the terrain has NO underwater
//      geometry — cliff walls stop at the waterline — so prepass depth can't measure
//      shallowness; the baked field gives distance-to-land directly): a shallow→deep
//      colour/opacity gradient, a soft waterline, and an animated foam collar hugging
//      every coast, river bank and lake edge,
//   3. grazing-angle fresnel that tints toward the sky and sharpens roughness, so the
//      surface mirrors the Atmosphere sky at a distance but stays readable up close.
// Everything still runs the normal PBR lighting + post, so sun glints, IBL, fog and
// tonemap all apply. On Ultra the lifted bloom catches the sharpened sun glints —
// the sparkle is free.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{globals, view},
}

struct WaterParams {
    // x = ripple amplitude (normal tilt), y = wave frequency, z = scroll speed,
    // w = fresnel strength (0 = off)
    params: vec4<f32>,
    // rgb = sky tint blended in at grazing angles, w = shore-fx strength (foam/gradient)
    sky_tint: vec4<f32>,
    // Shore-distance texture mapping: xy = world-space min corner, zw = 1 / world extent.
    region: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water: WaterParams;
// R8 shore-distance field: 0 = land, 1 = ≥ SHORE_MAX tiles offshore (linear-filtered,
// clamp-to-edge — the border texels are open sea, so off-texture samples read "deep").
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var shore_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var shore_samp: sampler;

// Max encoded shore distance in tiles — keep in sync with `worldmap::SHORE_MAX`.
const SHORE_MAX: f32 = 8.0;

// ── Tiny value noise (foam break-up) ────────────────────────────────────────────
fn hash21(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// ── Wave field ──────────────────────────────────────────────────────────────────
// Sum of directional sine waves with analytic gradients. `fine` (0..1) scales the
// high-frequency octaves — distance-faded by the caller so far water stays calm.
// Returns vec3(height, dH/dx, dH/dz).
fn water_surface(p: vec2<f32>, t: f32, fine: f32) -> vec3<f32> {
    let freq = water.params.y;
    let speed = water.params.z;
    var h = 0.0;
    var gx = 0.0;
    var gz = 0.0;
    // Two broad slow swells (the original calm surface).
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
    // Mid chop — visible texture inside a screenshot's middle ground.
    {
        let d = vec2<f32>(0.31, -0.95);
        let f = freq * 3.1;
        let a = 0.16 * fine;
        let ph = dot(p, d) * f + t * speed * 1.9;
        h += sin(ph) * a;
        let c = cos(ph) * a * f;
        gx += c * d.x; gz += c * d.y;
    }
    {
        let d = vec2<f32>(-0.91, -0.42);
        let f = freq * 4.3;
        let a = 0.11 * fine;
        let ph = dot(p, d) * f + t * speed * 2.4;
        h += sin(ph) * a;
        let c = cos(ph) * a * f;
        gx += c * d.x; gz += c * d.y;
    }
    // Fine ripples — the layer that makes sun glints slide and sparkle up close.
    {
        let d = vec2<f32>(0.59, 0.81);
        let f = freq * 7.7;
        let a = 0.05 * fine;
        let ph = dot(p, d) * f + t * speed * 3.4;
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

    // High-frequency octaves fade out by ~90 units so the far sea reads as calm
    // swells instead of aliased shimmer against the fog line.
    let cam_dist = length(view.world_position.xyz - in.world_position.xyz);
    let fine = clamp(1.0 - cam_dist / 90.0, 0.0, 1.0);

    let s = water_surface(p, t, fine);
    let ripple_n = normalize(vec3<f32>(-s.y * amp, 1.0, -s.z * amp));
    pbr_input.N = normalize(pbr_input.N + ripple_n - vec3<f32>(0.0, 1.0, 0.0));
    pbr_input.world_normal = pbr_input.N;

    var color = pbr_input.material.base_color.rgb;
    var alpha = pbr_input.material.base_color.a;
    var rough = pbr_input.material.perceptual_roughness;

    // Subtle crest/trough banding: light gathers on the swell tops so the surface
    // reads as moving even where there's no glint.
    color *= 1.0 + s.x * 0.10;

    // Distance to the nearest land tile, in tiles (0 on land, SHORE_MAX+ offshore).
    let uv = (p - water.region.xy) * water.region.zw;
    let shore = textureSample(shore_tex, shore_samp, uv).r * SHORE_MAX;
    let fx = water.sky_tint.w;

    // Shallow → deep gradient: bright washed turquoise lapping the banks, dark navy
    // out in the channel / open sea. Opacity follows — shallows stay translucent.
    let depth_t = 1.0 - exp(-shore * 0.45);
    let shallow_col = color * vec3<f32>(1.55, 1.65, 1.25);
    let deep_col = color * vec3<f32>(0.30, 0.40, 0.58);
    color = mix(color, mix(shallow_col, deep_col, depth_t), fx);
    alpha = mix(alpha, mix(0.55, 0.95, depth_t), fx);

    // Soft waterline: fade out right where the sheet meets the bank so the shoreline
    // is a wash, not a hard z-cut.
    alpha *= mix(1.0, smoothstep(0.0, 0.25, shore), fx);

    // Animated foam collar on every shore: a noise-wobbled band over the shallows
    // plus a brighter line hugging the waterline, breathing with the swell phase.
    let n = vnoise(p * 1.9 + vec2<f32>(t * 0.25, -t * 0.20));
    let foam_w = 0.85 + 0.35 * sin(t * 0.9 + n * 6.2831) + s.x * 0.20;
    let band = 1.0 - smoothstep(0.0, max(foam_w, 0.001), shore + (n - 0.5) * 0.7);
    let line = 1.0 - smoothstep(0.0, 0.35, shore);
    let foam = clamp(band * (0.40 + 0.60 * n) + line * 0.75, 0.0, 1.0) * fx;
    color = mix(color, vec3<f32>(0.92, 0.96, 0.97), foam * 0.85);
    alpha = max(alpha, foam * 0.85);
    rough = mix(rough, 0.85, foam);

    // Grazing-angle fresnel: tint toward the sky and tighten the roughness so the
    // distance mirrors the Atmosphere; head-on stays the soft readable sheet.
    let fres = pow(1.0 - clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0), 5.0)
        * water.params.w;
    color = mix(color, water.sky_tint.rgb, fres * 0.55);
    rough = mix(rough, 0.10, fres);

    pbr_input.material.base_color = vec4<f32>(color, alpha);
    pbr_input.material.perceptual_roughness = rough;

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
