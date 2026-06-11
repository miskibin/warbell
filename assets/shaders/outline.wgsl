// Toon outline — a fullscreen post pass that reads the prepass depth + normal and darkens
// pixels on object/feature edges, so each low-poly object gets a crisp defined silhouette
// (the "every object pops" look). Depth discontinuities catch silhouettes + where objects
// overlap the ground; sharp normal breaks catch hard creases. Runs after tonemapping,
// BEFORE the depth-blur, so distant outlines soften naturally with the DoF.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var normal_texture: texture_2d<f32>;

struct Settings {
    thickness: f32,         // edge sample offset, in pixels
    depth_threshold: f32,   // relative depth jump that counts as a silhouette edge
    normal_threshold: f32,  // 1-dot(normal) break that counts as a crease edge
    strength: f32,          // how dark the outline goes (0..1)
    near: f32,              // camera near plane (depth → distance)
    sun_fade: f32,          // sun-gaze multiplier (1 = full; <1 when looking into the sun)
}
@group(0) @binding(4) var<uniform> settings: Settings;

// Eye-forward distance from the reverse-z prepass depth. Sky / cleared depth (0) → far.
fn linear_dist(coord: vec2<i32>) -> f32 {
    let d = textureLoad(depth_texture, coord, 0);
    if d <= 0.0 {
        return 1.0e5;
    }
    return settings.near / d;
}

fn world_normal(coord: vec2<i32>) -> vec3<f32> {
    let n = textureLoad(normal_texture, coord, 0).xyz;
    return normalize(n * 2.0 - vec3<f32>(1.0));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(screen_texture, texture_sampler, in.uv);
    let coord = vec2<i32>(in.position.xy);
    let dims = vec2<i32>(textureDimensions(screen_texture));
    let t = max(i32(round(settings.thickness)), 1);

    let dc = linear_dist(coord);
    let nc = world_normal(coord);

    var max_depth = 0.0;
    var max_normal = 0.0;
    let offs = array<vec2<i32>, 4>(vec2<i32>(t, 0), vec2<i32>(-t, 0), vec2<i32>(0, t), vec2<i32>(0, -t));
    for (var i = 0; i < 4; i = i + 1) {
        let c = clamp(coord + offs[i], vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
        let dn = linear_dist(c);
        max_depth = max(max_depth, abs(dn - dc) / max(dc, 0.1));
        max_normal = max(max_normal, 1.0 - dot(nc, world_normal(c)));
    }

    let depth_edge = smoothstep(settings.depth_threshold, settings.depth_threshold * 2.0, max_depth);
    let normal_edge = smoothstep(settings.normal_threshold, settings.normal_threshold + 0.25, max_normal);
    let edge = max(depth_edge, normal_edge);

    return vec4<f32>(color.rgb * (1.0 - edge * settings.strength * settings.sun_fade), color.a);
}
