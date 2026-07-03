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
    mesh_view_bindings::{globals, lights, view},
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
// R8 shore-distance field: 0 = land, 1 = ≥ SHORE_MAX tiles offshore (linear-filtered). Samples
// OUTSIDE the field's UV [0,1] are forced to "deep" in the fragment (see `in_field`) — the island
// touches the texture edge at its widest latitude, so relying on clamp-to-edge there smeared a
// false shallow band across the open sea.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var shore_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var shore_samp: sampler;
// Bog mask (R8, same region mapping as the shore field): 1 over the swamp's carved standing
// pools — the vivid river palette swaps to still dark-olive murk there and the foam collar dies
// (a bog doesn't lap). Baked alongside the shore field in `worldmap::bake_shore_distance`.
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var bog_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var bog_samp: sampler;

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

    var alpha = pbr_input.material.base_color.a;
    var rough = pbr_input.material.perceptual_roughness;

    // Distance to the nearest land tile, in tiles (0 on land, SHORE_MAX+ offshore).
    let uv = (p - water.region.xy) * water.region.zw;
    // Outside the baked shore field = open ocean → force DEEP. Clamp-to-edge otherwise smears the
    // texture's border column across the whole off-texture sea: where the island nears the texture
    // edge (its widest latitude, z≈0) that column holds a small shore distance, so the clamp painted
    // a bogus shallow+foam BAND straight across the open sea E↔W ("the white stripe through the map").
    let in_field = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    let shore = select(SHORE_MAX, textureSample(shore_tex, shore_samp, uv).r * SHORE_MAX, in_field);
    let bog = select(0.0, textureSample(bog_tex, bog_samp, uv).r, in_field);
    let fx = water.sky_tint.w;

    // Authored stylized palette (linear RGB): vivid turquoise lapping the banks,
    // rich saturated blue out in the channel / open sea. Authored here rather than
    // derived from the (muted) base colour — scaling that washed the whole river
    // toward grey. Lighting still applies, so it dims at night like everything else.
    // The first water texel beside land already reads ~1 (chamfer step + half-texel
    // sampling), so shift the field half a tile back toward the bank — otherwise the
    // waterline effects start one tile out and the foam collar evaporates.
    let shore_d = max(shore - 0.5, 0.0);

    let depth_t = 1.0 - exp(-shore_d * 0.7);
    let shallow_col = vec3<f32>(0.05, 0.38, 0.40);
    let deep_col = vec3<f32>(0.012, 0.09, 0.32);
    var color = mix(pbr_input.material.base_color.rgb,
        mix(shallow_col, deep_col, depth_t), fx);
    alpha = mix(alpha, mix(0.82, 0.96, depth_t), fx);
    // Bog murk: the swamp pools (and the marsh's river stretches) are STILL dark-olive water,
    // not the vivid turquoise channel — and near-opaque (you don't see the carved floor through
    // bog water). Cooler + more saturated than the first cut, which sat so close to the mud
    // tone that daytime pools read as wet dirt instead of standing water.
    let murk = mix(vec3<f32>(0.055, 0.115, 0.095), vec3<f32>(0.022, 0.048, 0.042), depth_t);
    color = mix(color, murk, bog * fx);
    alpha = mix(alpha, 0.95, bog * fx);
    // A touch glossier over the bog: the flat sky-sheen highlight is what sells "still water,
    // not mud" by day.
    rough = mix(rough, 0.10, bog * 0.6);

    // Crest/trough banding: light gathers on the swell tops so the surface reads
    // as moving even where there's no glint.
    color *= 1.0 + s.x * 0.20;

    // Soft waterline: fade out right where the sheet meets the bank so the shoreline
    // is a wash, not a hard z-cut.
    alpha *= mix(1.0, smoothstep(0.0, 0.15, shore), fx);

    // Foam collar on every shore: a crisp noise-dappled band over the shallows plus
    // a bright line hugging the waterline, breathing with the swell phase. The hard
    // smoothstep threshold keeps it readable low-poly dapples, not grey smudge.
    let n = vnoise(p * 1.9 + vec2<f32>(t * 0.25, -t * 0.20));
    let foam_w = 0.45 + 0.2 * sin(t * 0.9 + n * 6.2831) + s.x * 0.15;
    let band = 1.0 - smoothstep(0.0, max(foam_w, 0.001), shore_d + (n - 0.5) * 0.5);
    let line = 1.0 - smoothstep(0.0, 0.3, shore_d);
    let foam = smoothstep(0.55, 0.85, band * (0.30 + 0.70 * n) + line * 0.6) * fx * (1.0 - bog * 0.94);
    color = mix(color, vec3<f32>(0.95, 0.97, 0.98), foam);
    alpha = max(alpha, min(foam * 1.2, 0.97));
    rough = mix(rough, 0.85, foam);

    // Grazing-angle fresnel: tint toward the sky and tighten the roughness so the
    // distance mirrors the Atmosphere; head-on stays the soft readable sheet.
    let fres = pow(1.0 - clamp(dot(pbr_input.N, pbr_input.V), 0.0, 1.0), 5.0)
        * water.params.w;
    rough = mix(rough, 0.16, fres);

    // Day factor from the brightest directional light. Bevy premultiplies ONLY
    // illuminance into `.color` (no exposure), so these are lux-scale: the day sun
    // peaks ≈13k, while the night pair — 3800-lux moon + the sun's 800-lux
    // readability floor — tops out ≈2.7k. The vivid authored palette is a DAYLIGHT
    // look — under the blue-leaning night grade its cyan luminance survives while
    // the terrain crushes, so the lake would glow radioactive at midnight. Dim it
    // toward a dark moonlit sheet instead.
    var sun_lum = 0.0;
    let nd = min(lights.n_directional_lights, 4u);
    for (var i = 0u; i < nd; i = i + 1u) {
        let c = lights.directional_lights[i].color.rgb;
        sun_lum = max(sun_lum, dot(c, vec3<f32>(0.299, 0.587, 0.114)));
    }
    let day_f = smoothstep(3500.0, 9000.0, sun_lum);
    color = mix(color, water.sky_tint.rgb, fres * 0.35 * mix(0.4, 1.0, day_f));
    color *= mix(0.22, 1.0, day_f);

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
