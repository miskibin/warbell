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

// Fixed 32-tap disc — the original count and cost. A higher / dynamic count tanked weak GPUs:
// most of a player-focused frame is max-blur background, so every extra tap is paid fullscreen,
// and a variable loop bound also blocks the compiler from unrolling. Temporal stability comes
// from the cheap Karis luma weight below + the smaller max radius, NOT brute tap count.
const TAPS: i32 = 32;
const GOLDEN_ANGLE: f32 = 2.39996323;

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Interleaved gradient noise → [0,1). Keyed on the *screen pixel* only (no time term), so the
// rotation it drives is stable frame-to-frame: a still camera is pixel-identical every frame
// (no flicker when standing), while neighbouring pixels get decorrelated disc orientations.
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// Eye-forward distance from reverse-z prepass depth. Sky / cleared depth → very far.
fn dist_at(uv: vec2<f32>, depth_size: vec2<f32>) -> f32 {
    // Ultra can render the prepass depth at a different resolution from this
    // post-process target. Map by UV proportion instead of reusing screen pixels.
    let coord = clamp(vec2<i32>(uv * depth_size), vec2<i32>(0), vec2<i32>(depth_size) - vec2<i32>(1));
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
    let depth_size = vec2<f32>(textureDimensions(depth_texture));
    let center = textureSample(screen_texture, texture_sampler, in.uv);

    let c = coc_of(dist_at(in.uv, depth_size));
    if settings.debug_view > 0.5 {
        return vec4<f32>(c, c, c, 1.0); // white = fully out-of-focus per DoF; black = sharp band
    }
    let blur_px = c * settings.max_radius;
    if blur_px < 0.5 {
        return center;
    }

    let texel = 1.0 / dims;
    // Depth-aware sunflower-disc gather. Each tap is weighted by its own CoC (so a sharp,
    // in-focus tap barely bleeds into a blurred pixel) AND by a Karis luma term `1/(1+luma)`:
    // that down-weights bright outlier pixels so a hot speck on the distant horizon can't pop
    // in and out of the sparse disc as the camera moves (the classic bokeh sparkle). The luma
    // term cancels on flat regions (numerator + denominator scale together), so it only shifts
    // the result where there's contrast — exactly where the sparkle lives.
    let cw = c / (1.0 + luma(center.rgb));
    var acc = center.rgb * cw;
    var total = cw;
    // Per-pixel disc rotation: golden-angle taps give uniform coverage, but a *fixed* orientation
    // means every pixel shares the same under-sampling blind spots — so a bright snow tip pops in
    // and out of the sparse disc in a coherent patch as the scene slides past (motion sparkle).
    // Rotating the whole disc by an IGN-hashed per-pixel angle decorrelates those blind spots into
    // fine spatial noise the eye averages out. Screen-keyed only → a still frame stays flicker-free.
    let rot = ign(in.position.xy) * 6.28318530718;
    let cr = cos(rot);
    let sr = sin(rot);
    for (var i = 0; i < TAPS; i = i + 1) {
        let fi = f32(i) + 1.0;
        let ang = fi * GOLDEN_ANGLE;
        let rad = sqrt(fi / f32(TAPS)) * blur_px;
        let ca = cos(ang);
        let sa = sin(ang);
        // (ca,sa) rotated by the per-pixel angle (cr,sr).
        let off = vec2<f32>(ca * cr - sa * sr, ca * sr + sa * cr) * rad;
        let s = textureSample(screen_texture, texture_sampler, in.uv + off * texel).rgb;
        let w = max(coc_of(dist_at(in.uv + off * texel, depth_size)), 0.02) / (1.0 + luma(s));
        acc += s * w;
        total += w;
    }
    return vec4<f32>(acc / total, center.a);
}
