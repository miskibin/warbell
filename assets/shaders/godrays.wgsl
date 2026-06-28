// Screen-space **god rays** (radial light scattering) — the crepuscular-ray pass the original TS
// game shipped (GPU Gems 3, Ch. 13). For each pixel we march N samples toward the sun's SCREEN
// position, accumulating the scene's own brightness (sky/sun = bright, trees = dark) with a
// per-step exponential decay, then ADD the result back, tinted by the sun colour. The bright sky
// streaks into shafts that are naturally occluded by the treeline silhouette — no geometry, no
// occlusion buffer, no dependence on volumetric fog (which is why this works where the volumetric
// pass didn't). Runs in PostProcess after tonemapping, so it scatters the already-graded image.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct Settings {
    sun_color: vec3<f32>,   // ray tint (the live sun DirectionalLight colour, linear)
    intensity: f32,         // overall additive strength
    sun_screen: vec2<f32>,  // sun position in UV space [0,1] (driven each frame)
    decay: f32,             // per-step falloff along the ray (e.g. 0.96)
    density: f32,           // march length toward the sun as a fraction of the screen (e.g. 0.7)
    weight: f32,            // per-sample weight
    threshold: f32,         // luminance above which a sample counts as "light" (sky/sun)
    num_samples: f32,       // march steps (e.g. 48)
    fade: f32,              // 0..1 master gate: daylight × on-screen alignment (0 = no rays)
}
@group(0) @binding(2) var<uniform> settings: Settings;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(screen_texture, texture_sampler, in.uv);

    let strength = settings.intensity * settings.fade;
    if strength <= 0.0001 {
        return scene; // night / sun off-screen — pass the frame straight through
    }

    let steps = max(settings.num_samples, 1.0);
    let n = i32(steps);
    // Step vector from this pixel toward the sun's screen position, scaled so the whole march
    // covers `density` of the distance to the sun.
    let delta = (settings.sun_screen - in.uv) * (settings.density / steps);

    // Per-pixel jitter of the march START, by a fraction of one step. Without it every pixel
    // samples at the SAME discrete phase, so the finite step count shows up as visible banding
    // (the "low-res stripes"). Interleaved-gradient-noise dither spreads that into high-frequency
    // noise the soft rays hide — so we get smooth shafts at a LOW sample count (= cheap) instead of
    // having to brute-force more steps. Cost: a few ALU ops, no extra texture taps.
    let ign = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
    var coord = in.uv + delta * ign;
    var decay = 1.0;
    var accum = vec3<f32>(0.0);
    for (var i = 0; i < n; i = i + 1) {
        coord += delta;
        // Outside the frame contributes nothing — avoids smearing the clamped edge pixels into a
        // bright border when the sun sits near/over the screen edge.
        if coord.x < 0.0 || coord.x > 1.0 || coord.y < 0.0 || coord.y > 1.0 {
            decay *= settings.decay;
            continue;
        }
        let s = textureSample(screen_texture, texture_sampler, coord).rgb;
        let mask = smoothstep(settings.threshold, settings.threshold + 0.25, luminance(s));
        accum += s * mask * decay * settings.weight;
        decay *= settings.decay;
    }

    // Radial SHAFT structure — the difference between a flat sun-glow and crepuscular rays. Break
    // the smooth fan into soft beams + gaps by an angular function of the direction from the sun
    // (non-harmonic frequencies so it doesn't read as a regular pinwheel). The beams rotate with
    // the sun's screen position, so they stay anchored to the light like real god rays.
    let dir = in.uv - settings.sun_screen;
    let ang = atan2(dir.y, dir.x);
    let beam = 0.72 + 0.28 * (0.6 * sin(ang * 9.0) + 0.4 * sin(ang * 17.0 + 1.7));
    accum *= beam;

    accum *= strength * settings.sun_color;
    return vec4<f32>(scene.rgb + accum, scene.a);
}
