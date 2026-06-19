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
    // x=bump_strength, y=quality (0=Low,1=High,2=Ultra), z=macro_variety, w=reserved
    params2: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> forest: ForestParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var detail_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var detail_samp: sampler;

// 2D value-noise hash. The original `fract(sin(p.x*127.1 + p.y*311.7)*43758)` is effectively
// a 1D hash of the dot product `p·(127.1,311.7)`, so cells along that fixed diagonal get
// CORRELATED values — which the bump lighting reveals as straight parallel "ruts" across the
// ground. This is the sine-free Hoskins `hash12`: a genuine 2D hash with no directional
// correlation (and no large-coordinate `sin` precision breakdown).
fn ter_hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn ter_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let a = ter_hash(i);
    let b = ter_hash(i + vec2<f32>(1.0, 0.0));
    let c = ter_hash(i + vec2<f32>(0.0, 1.0));
    let d = ter_hash(i + vec2<f32>(1.0, 1.0));
    // Quintic (C2) interpolation, not the cubic smoothstep. The cubic still has a
    // discontinuous 2nd derivative at every cell edge, which reads as a faint axis-aligned
    // quilt — invisible-ish in albedo but GLARING once the noise drives a normal (the
    // bump), because shading amplifies the gradient kink. Quintic is C2 → smooth gradients,
    // no grid in the lit relief.
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    return mix(a, b, u.x) + (c - a) * u.y * (1.0 - u.x) + (d - b) * u.x * u.y;
}

// Value noise whose lattice is ROTATED off the world axes. Value noise is built on an
// integer grid, so its features line up with X/Z and stack into a visible square grid
// when several octaves share that alignment. Rotating each octave by a different non-axis
// angle scatters the cells so nothing reinforces into a lattice.
fn ter_noise_rot(p: vec2<f32>, c: f32, s: f32) -> f32 {
    return ter_noise(vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y));
}

// Pseudo micro-relief height in world XZ for the bump-shading gradient. Tuned to the
// VISIBLE clump scale (periods ~0.5–3 world units) — the very-fine albedo octaves
// (>5/unit) are deliberately left out: at gameplay distance they're sub-pixel and only
// alias/shimmer the normal. Ultra adds one finer octave for crisp near-camera grain.
fn terrain_h(p: vec2<f32>) -> f32 {
    // Each octave on its own rotated lattice (≈19°, 47°, 73°, 101°) so the relief never
    // stacks into an axis-aligned grid.
    var h = ter_noise_rot(p * 0.35, 0.946, 0.326) * 0.50   // broad mounds (~2.8u)
          + ter_noise_rot(p * 0.90, 0.682, 0.731) * 0.35   // clumps      (~1.1u)
          + ter_noise_rot(p * 2.20, 0.292, 0.956) * 0.22;  // fine clumps  (~0.45u)
    if forest.params2.y >= 2.0 {
        h += ter_noise_rot(p * 4.0, -0.191, 0.982) * 0.12; // ultra: near-camera grain
    }
    return h;
}

// Rock/erosion relief for the terrace WALLS, sampled in the face plane: `along` is the
// horizontal tangent down the wall (world X for Z-facing walls, world Z for X-facing) and
// `wy` is world Y. Features run VERTICALLY (erosion runnels): higher frequency along the
// horizontal axis than the vertical one, so the grooves streak top-to-bottom — the look that
// is NATURAL on a cliff (and the exact streak artifact we banned on flat ground).
fn rock_h(along: f32, wy: f32) -> f32 {
    return ter_noise(vec2<f32>(along * 2.2, wy * 0.7)) * 0.6
         + ter_noise(vec2<f32>(along * 5.0, wy * 1.6)) * 0.4;
}

// Lightly de-tiled detail-texture sample. The detail image is a Repeat texture sampled at
// `wp * scale`, wrapping every `1/scale` world units (~5.6u). Now that the texture is fine
// ISOTROPIC grain (no broad blobs, no directional streaks — see `terrain::detail_image`),
// that repeat is invisible on its own; a small low-frequency UV domain-warp finishes the job
// by bending any residual periodic line into a wavy one. A SINGLE sample — the earlier
// rotated second octave was what turned the texture's (then anisotropic) streaks into a
// diagonal crosshatch, so it's gone.
fn sample_detail(wp: vec2<f32>, scale: f32) -> vec3<f32> {
    let warp = vec2<f32>(ter_noise(wp * 0.06 + 4.0), ter_noise(wp * 0.06 + 19.0)) - 0.5;
    return textureSample(detail_tex, detail_samp, wp * scale + warp * 0.4).rgb;
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
    //     Each octave on its own rotated lattice (≈19°/47°/73°) so the value-noise cells
    //     don't line up into the axis-aligned brightness grid that read as square "tiles".
    let ter_m = ter_noise_rot(wp * 0.5, 0.946, 0.326) * 0.55
              + ter_noise_rot(wp * 1.7, 0.682, 0.731) * 0.30
              + ter_noise_rot(wp * 5.5, 0.292, 0.956) * 0.15;
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
    let det = sample_detail(wp, detail_scale) / max(mean, 0.01);
    let det_l = dot(det, vec3<f32>(0.2126, 0.7152, 0.0722));
    let det_c = mix(vec3<f32>(det_l), det, green);
    rgb *= mix(vec3<f32>(1.0), det_c, detail_strength * top_face);

    // (4) macro albedo-variety: broad worn-dirt scuffs + damp moss hollows so the field
    //     stops reading as one flat green. Very low-freq blobs, green-gated, strength from
    //     the preset (`macro_variety`). Value/hue stays subtle — this is variety, not camo.
    let variety = forest.params2.z;
    let worn = smoothstep(0.60, 0.88, ter_noise(wp * 0.020 + vec2<f32>(5.0, 9.0)));
    rgb = mix(rgb, rgb * vec3<f32>(0.82, 0.74, 0.55), worn * 0.30 * green * variety);
    let moss = smoothstep(0.62, 0.92, ter_noise(wp * 0.040 + vec2<f32>(19.0, 2.0)));
    rgb = mix(rgb, rgb * vec3<f32>(0.72, 0.92, 0.62), moss * 0.22 * green * variety);

    pbr_input.material.base_color = vec4<f32>(max(rgb, vec3<f32>(0.0)), pbr_input.material.base_color.a);

    // ── Normal perturbation (bump) — the cure for "flat". The ground geometry is dead-flat,
    //    so every fragment was lit at the same angle. Build a height field from the same
    //    noise the colour uses, take its world-XZ gradient (finite difference), and tilt the
    //    shading normal `pbr_input.N` by it. Now grass clumps catch the sun on one side and
    //    shadow on the other → relief. Weighted to near-flat ground (`topw`) so vertical
    //    cliff faces keep their true normal. SHADOWS still sample the flat `world_normal`
    //    (lighting uses `N`), so there are no fake self-shadow artifacts.
    // `bump` is a uniform, so this branch is coherent across the draw: when the preset
    // disables relief (Low → bump 0) the 4 height taps are skipped entirely — the relief is
    // free on weak GPUs while the cheap anti-grid colour fixes above still apply. `N` keeps
    // its default (flat `world_normal`) in that case.
    let bump = forest.params2.x;
    if bump > 0.001 {
        let gn = normalize(in.world_normal);
        let topw = smoothstep(0.35, 0.80, gn.y);
        // Small offset so the finite difference resolves the clump-scale octaves (a large
        // offset spans whole noise cells and cancels the gradient → flat). The RAW difference
        // (not the true 1/2e gradient) is deliberately kept: the true gradient of these
        // octaves is ~2, which tilts the normal ~70° and catches cool sky-ambient
        // (washed-out); the raw diff at this small offset stays a tame ~0.2 → gentle relief.
        let e = 0.18;
        let hx = terrain_h(wp + vec2<f32>(e, 0.0)) - terrain_h(wp - vec2<f32>(e, 0.0));
        let hz = terrain_h(wp + vec2<f32>(0.0, e)) - terrain_h(wp - vec2<f32>(0.0, e));
        var n2 = normalize(gn + vec3<f32>(-hx, 0.0, -hz) * bump * topw);

        // Side-face rock relief on the terrace WALLS. The top relief above is an XZ height
        // field — meaningless across a near-vertical face (XZ barely moves down a wall), so
        // walls got nothing and read as flat painted blocks. Give them their own relief in
        // the face plane (horizontal tangent + world Y) so the bevelled/sloped terrace faces
        // read as eroded rock/dirt. `sidew` is 1 on walls, 0 on tops; with `bump` it's free
        // on Low. Composes on top of `n2` so the bevel keeps a touch of the top relief.
        let sidew = 1.0 - smoothstep(0.30, 0.72, gn.y);
        if sidew > 0.001 {
            let wy = in.world_position.y;
            var along = wp.x;                  // Z-facing wall runs along world X
            var tang = vec3<f32>(1.0, 0.0, 0.0);
            if abs(gn.x) > abs(gn.z) {
                along = wp.y;                  // X-facing wall runs along world Z (wp = world.xz)
                tang = vec3<f32>(0.0, 0.0, 1.0);
            }
            let e2 = 0.18;
            let dha = rock_h(along + e2, wy) - rock_h(along - e2, wy);
            let dhy = rock_h(along, wy + e2) - rock_h(along, wy - e2);
            n2 = normalize(n2 - (tang * dha + vec3<f32>(0.0, 1.0, 0.0) * dhy) * bump * 0.7 * sidew);
            // Subtle crevice darkening so the relief reads in albedo too, not only in shading.
            let crev = rock_h(along, wy);
            let dk = mix(1.0, 0.82 + crev * 0.30, sidew);
            pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * dk, pbr_input.material.base_color.a);
        }
        pbr_input.N = n2;
    }

    // Ultra grass sheen: drop roughness a touch on green ground so the sun throws a lush,
    // slightly specular highlight across the new relief. Subtle; green-gated.
    if forest.params2.y >= 2.0 {
        let r0 = pbr_input.material.perceptual_roughness;
        pbr_input.material.perceptual_roughness = mix(r0, r0 * 0.82, green);
    }

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
