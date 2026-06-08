// Distance-based depth blur — a fake depth-of-field that runs as a fullscreen post pass.
// Reads the scene depth (prepass), turns it into a camera distance, and blurs the colour
// by an amount that is ZERO inside `clear` tiles and ramps to `radius` px by `full` tiles.
// This is the controllable "see clearly nearby, blur the distance" the built-in DoF
// couldn't give us (single focal plane + it silently no-ops with SSAO).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;

struct Settings {
    clear: f32,   // tiles fully sharp around the camera
    full: f32,    // tiles at which blur reaches `radius`
    radius: f32,  // max blur radius, in pixels
    near: f32,    // camera near plane (for depth → distance)
}
@group(0) @binding(3) var<uniform> settings: Settings;

const TAPS: i32 = 24;
const GOLDEN_ANGLE: f32 = 2.39996323;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(screen_texture));
    let coord = vec2<i32>(in.position.xy);

    // Bevy uses infinite reverse-z perspective: clip_from_view[3][2] == near, so the
    // eye-forward distance is simply near / ndc_depth. Sky / cleared depth == 0 → treat
    // as maximally far.
    let depth = textureLoad(depth_texture, coord, 0);
    var dist: f32;
    if depth <= 0.0 {
        dist = settings.full * 4.0;
    } else {
        dist = settings.near / depth;
    }

    // Blur ramp: 0 within `clear`, eased up to full by `full`.
    let t = clamp((dist - settings.clear) / max(settings.full - settings.clear, 0.001), 0.0, 1.0);
    let blur_px = t * t * settings.radius;

    let center = textureSample(screen_texture, texture_sampler, in.uv);
    if blur_px < 0.5 {
        return center;
    }

    // Sunflower-disc blur, radius scaled per-pixel by the distance ramp.
    let texel = 1.0 / dims;
    var acc = center.rgb;
    var total = 1.0;
    for (var i = 0; i < TAPS; i = i + 1) {
        let fi = f32(i) + 1.0;
        let ang = fi * GOLDEN_ANGLE;
        let rad = sqrt(fi / f32(TAPS)) * blur_px;
        let off = vec2<f32>(cos(ang), sin(ang)) * rad * texel;
        acc += textureSample(screen_texture, texture_sampler, in.uv + off).rgb;
        total += 1.0;
    }
    return vec4<f32>(acc / total, center.a);
}
