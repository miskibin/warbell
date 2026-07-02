// **Cinematic atmospherics** — analytic height fog + sun in-scatter + drifting fog noise +
// cloud light patches, one fullscreen pass (see `src/atmospherics.rs`). Reads the prepass
// depth, reconstructs the world position, and layers a warm aerial haze OVER the graded
// image (runs post-tonemapping, before the god-rays scatter so the rays feed on the hazed
// frame). Sky pixels (cleared depth) are left untouched — the Atmosphere sky already carries
// its own gradient, and fogging it flattens the frame to a wall.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var depth_texture: texture_depth_2d;

struct Settings {
    world_from_clip: mat4x4<f32>, // inverse view-projection (driven per frame)
    cam_pos: vec3<f32>,           // camera world position
    density: f32,                 // base fog density per world unit
    sun_dir: vec3<f32>,           // direction TO the sun (driven per frame)
    height_falloff: f32,          // how fast fog thins with altitude (per unit)
    fog_color: vec3<f32>,         // base haze colour (live DistanceFog colour, linear)
    inscatter_exp: f32,           // pow() exponent of the sun-glow lobe (low = wide)
    glow_color: vec3<f32>,        // sun in-scatter colour (DistanceFog directional colour)
    time: f32,                    // seconds, for drifting noise
    cloud_strength: f32,          // cloud light-patch modulation depth (0 = off)
    cloud_scale: f32,             // cloud noise frequency in world XZ (1/units)
    noise_strength: f32,          // fog-density noise modulation (0 = uniform haze)
    fog_start: f32,               // fog-free radius around the camera (units)
    fog_max: f32,                 // max fog opacity (never fully swallow geometry)
    fade: f32,                    // 0..1 master gate (daylight), driven per frame
    base_height: f32,             // world Y where fog is densest
    _pad: f32,
}
@group(0) @binding(3) var<uniform> settings: Settings;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Bilinear value noise, [0,1].
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Two soft octaves — big drifting blobs, no gridding (second octave rotated + offset).
fn fbm(p: vec2<f32>) -> f32 {
    let r = mat2x2<f32>(vec2<f32>(0.8, 0.6), vec2<f32>(-0.6, 0.8));
    return 0.62 * vnoise(p) + 0.38 * vnoise(r * p * 2.3 + vec2<f32>(17.7, 9.2));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(screen_texture, texture_sampler, in.uv);
    if settings.fade <= 0.001 {
        return scene;
    }

    // Sample depth by UV PROPORTION, not by this pass's pixel coord: under the Ultra
    // render-scale (MainPassResolutionOverride 2.0) the prepass depth texture is a different
    // resolution than the post-process chain, and a raw pixel-coord load reads the wrong
    // quadrant of the scene (the fog then paints a shrunken ghost of the treeline).
    let dsize = vec2<f32>(textureDimensions(depth_texture));
    let d = textureLoad(depth_texture, vec2<i32>(in.uv * dsize), 0);
    if d <= 0.0 {
        return scene; // sky / cleared depth — leave the Atmosphere gradient alone
    }

    // Reverse-Z unprojection → exact world position of this pixel's surface.
    let ndc = vec4<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0, d, 1.0);
    let ws = settings.world_from_clip * ndc;
    let pos = ws.xyz / ws.w;

    let ray = pos - settings.cam_pos;
    let dist = length(ray);
    let ray_dir = ray / max(dist, 0.001);

    // ── Cloud light patches ── big drifting bright/shaded blobs across the ground (and
    // everything standing on it), as if broken cloud were crossing the sun. Multiplied into
    // the scene BEFORE the fog blend, so distance haze naturally swallows the far patches.
    var lit = scene.rgb;
    if settings.cloud_strength > 0.001 {
        let cp = pos.xz * settings.cloud_scale + vec2<f32>(0.017, 0.011) * settings.time;
        let cn = fbm(cp);
        // Wide soft ramp: most of the field near neutral, the deepest blobs shaded, the
        // clear gaps catching a touch of extra sun.
        let shade = mix(1.0 - settings.cloud_strength,
                        1.0 + 0.4 * settings.cloud_strength,
                        smoothstep(0.30, 0.78, cn));
        lit = scene.rgb * mix(1.0, shade, settings.fade);
    }

    // ── Height fog + sun in-scatter ──
    let run = max(dist - settings.fog_start, 0.0);
    if run <= 0.0 {
        return vec4<f32>(lit, scene.a);
    }

    // Analytic exponential height fog (Wenzel): integrate density e^(-falloff·y) along the
    // ray from the camera to the surface. `dy` near 0 (level ray) degenerates → limit 1.
    let k = settings.height_falloff;
    let cam_h = settings.cam_pos.y - settings.base_height;
    let dy = pos.y - settings.cam_pos.y;
    var height_int = 1.0;
    if abs(dy * k) > 0.001 {
        height_int = (1.0 - exp(-k * dy)) / (k * dy);
    }
    var amount = settings.density * exp(-k * cam_h) * height_int * run;

    // Drifting density noise so the haze reads as moving air, not a uniform veil.
    if settings.noise_strength > 0.001 {
        let np = pos.xz * 0.035 + vec2<f32>(-0.05, 0.03) * settings.time;
        amount *= 1.0 + (fbm(np) - 0.5) * 2.0 * settings.noise_strength;
    }

    let factor = min(1.0 - exp(-max(amount, 0.0)), settings.fog_max) * settings.fade;

    // Sun in-scatter: the haze brightens/warms looking toward the sun — the "trees melting
    // into light" read. Low exponent = a wide golden lobe, not a tight sun spot.
    let sun_amt = pow(max(dot(ray_dir, settings.sun_dir), 0.0), settings.inscatter_exp);
    let fog_col = mix(settings.fog_color, settings.glow_color, sun_amt);

    return vec4<f32>(mix(lit, fog_col, factor), scene.a);
}
