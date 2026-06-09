// Custom circle-of-confusion (CoC) **bokeh depth-of-field** — the system the old game used
// (player-focused DoF), done as a fullscreen post pass because Bevy's built-in DepthOfField
// silently no-ops in this pipeline. Reads the prepass depth, computes a CoC from a focal
// plane (driven onto the player) with a sharp focus band, and blurs BOTH the foreground and
// background that fall outside it. A depth-aware gather weights each tap by its own CoC so a
// sharp subject doesn't bleed into the blurred distance (the classic single-pass halo).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;

struct Settings {
    focal: f32,       // focus distance (tiles) — driven onto the player each frame
    range: f32,       // half-width of the fully-sharp focus band (tiles)
    far_ramp: f32,    // tiles over which FAR blur ramps to max (large = gradual)
    max_radius: f32,  // maximum blur radius (px)
    near: f32,        // camera near plane (reverse-z depth → distance)
    debug_view: f32,  // >0.5 → output raw CoC as grayscale (debug), don't blur
}
@group(0) @binding(3) var<uniform> settings: Settings;

const TAPS: i32 = 32;
const GOLDEN_ANGLE: f32 = 2.39996323;

// Eye-forward distance from reverse-z prepass depth. Sky / cleared depth → very far.
fn dist_at(coord: vec2<i32>) -> f32 {
    let d = textureLoad(depth_texture, coord, 0);
    if d <= 0.0 {
        return 1.0e5;
    }
    return settings.near / d;
}

// Circle of confusion: 0 inside the sharp band [focal±range], then ramps to 1. The FAR side
// ramps GRADUALLY over `far_ramp` tiles (so distance keeps getting blurrier instead of
// clamping to a flat max); the NEAR/foreground side ramps quicker (less depth to work with).
fn coc_of(dist: f32) -> f32 {
    let d = abs(dist - settings.focal) - settings.range;
    if d <= 0.0 {
        return 0.0;
    }
    if dist >= settings.focal {
        return clamp(d / max(settings.far_ramp, 0.001), 0.0, 1.0);
    }
    return clamp(d / max(settings.range, 0.001), 0.0, 1.0);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(screen_texture));
    let coord = vec2<i32>(in.position.xy);
    let center = textureSample(screen_texture, texture_sampler, in.uv);

    let c = coc_of(dist_at(coord));
    if settings.debug_view > 0.5 {
        return vec4<f32>(c, c, c, 1.0); // white = fully out-of-focus per DoF; black = sharp band
    }
    let blur_px = c * settings.max_radius;
    if blur_px < 0.5 {
        return center;
    }

    let texel = 1.0 / dims;
    let max_c = vec2<i32>(dims) - vec2<i32>(1, 1);
    // Depth-aware sunflower-disc gather. Each tap is weighted by its own CoC, so sharp
    // (in-focus) taps barely bleed into a blurred pixel, while blurred taps blend smoothly.
    var acc = center.rgb * c;
    var total = c;
    for (var i = 0; i < TAPS; i = i + 1) {
        let fi = f32(i) + 1.0;
        let ang = fi * GOLDEN_ANGLE;
        let rad = sqrt(fi / f32(TAPS)) * blur_px;
        let off = vec2<f32>(cos(ang), sin(ang)) * rad;
        let tap_coord = clamp(coord + vec2<i32>(off), vec2<i32>(0, 0), max_c);
        let w = max(coc_of(dist_at(tap_coord)), 0.02);
        acc += textureSample(screen_texture, texture_sampler, in.uv + off * texel).rgb * w;
        total += w;
    }
    return vec4<f32>(acc / total, center.a);
}
