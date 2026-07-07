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
    // Wheel-rut mask world→UV mapping: xy = world min corner, zw = 1/extent (0 = disabled).
    rut_region: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> forest: ForestParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var detail_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var detail_samp: sampler;
// Cart-wheel-rut mask (R8, `roads::bake_rut_mask`): twin grooves beside artery centrelines.
// Fragment-resolution because ~0.5u grooves are far below the 1u vertex-colour grid.
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var rut_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var rut_samp: sampler;

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

// Distance from point `p` to the line segment a→b (analytic capsule SDF core).
fn seg_dist(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-5), 0.0, 1.0);
    return length(pa - ba * h);
}

// Scattered fallen-twig field in world XZ, baked into the floor texture so the ground reads
// littered EVERYWHERE, not only under a 3D litter prop. One short twig per cell on a ~1.25u
// lattice, each with an INDEPENDENT random position / angle / length — localised segments at
// every orientation, NOT a world-spanning anisotropic streak (the banned flat-ground artifact
// was a single global directional lattice tiling into diagonal bands; random per-cell sticks
// never reinforce into one). Scans the 3×3 neighbourhood so a twig straddling a cell border
// still draws.
//
// A clean SDF segment reads as a flat painted DASH — geometric, "immature", out of place on
// the noisy ground. So this is deliberately organic: the query point is DOMAIN-WARPED (a low
// + high freq pair) so each stick bends and frays instead of running dead straight; the width
// TAPERS to thin ends like a real twig (not a constant-width dash); the edge is softened by a
// noisy band so it's ragged, not vector-crisp. Returns coverage `x` plus an along-stick tone
// `y` (0..1) so the caller can vary the bark colour down its length.
fn twig_field(wp: vec2<f32>, seedoff: f32, base_w: f32) -> vec2<f32> {
    // Bend (low-freq) + fray (high-freq) domain warp.
    let warp = (vec2<f32>(ter_noise(wp * 0.8 + seedoff + 8.0), ter_noise(wp * 0.8 + seedoff + 30.0)) - 0.5) * 1.7
             + (vec2<f32>(ter_noise(wp * 2.6 + seedoff + 3.0), ter_noise(wp * 2.6 + seedoff + 21.0)) - 0.5) * 0.7;
    let q = wp + warp * 0.10;
    let cell = 1.25;
    let ci = floor(q / cell);
    var best_d = 1e9;
    var best_h = 0.0;
    var best_seed = 0.0;
    for (var dy = -1; dy <= 1; dy += 1) {
        for (var dx = -1; dx <= 1; dx += 1) {
            let c = ci + vec2<f32>(f32(dx), f32(dy));
            let r1 = ter_hash(c + seedoff);
            // ~45% of cells carry a twig — sparse, like a real forest floor.
            if (r1 < 0.45) {
                let r2 = ter_hash(c + seedoff + 17.3);
                let r3 = ter_hash(c + seedoff + 41.7);
                let r4 = ter_hash(c + seedoff + 71.1);
                let center = (c + vec2<f32>(r2, r3)) * cell;
                let ang = r4 * 6.28318;
                let dir = vec2<f32>(cos(ang), sin(ang));
                let len = (0.20 + r1 * 0.55) * cell;   // ~0.25–0.8u sticks
                let a = center - dir * len * 0.5;
                let ba = dir * len;
                let pa = q - a;
                let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-5), 0.0, 1.0);
                let d = length(pa - ba * h);
                if (d < best_d) {
                    best_d = d;
                    best_h = h;
                    best_seed = r2;
                }
            }
        }
    }
    // Taper: full width mid-stick, thinning to a point at both ends.
    let w = base_w * (0.30 + 0.70 * (1.0 - pow(abs(best_h * 2.0 - 1.0), 1.7)));
    // Ragged, noisy soft edge (not a crisp vector line).
    let edge = 0.008 + 0.014 * ter_noise(wp * 9.0 + seedoff);
    let cov = 1.0 - smoothstep(w, w + edge, best_d);
    let tone = ter_noise(vec2<f32>(best_h * 4.0, best_seed * 13.0));
    return vec2<f32>(cov, tone);
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

    // Cliff-face weight. Terrain cliff-wall vertices carry a NEGATIVE alpha "cliffness"
    // (`worldmap::build_terrain_chunk::cliff_wall`); flat ground stays 0 and marsh wetness
    // stays positive, so `-a` cleanly isolates the crag faces for the rock layers below.
    let cliff = clamp(-pbr_input.material.base_color.a, 0.0, 1.0);

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
    // Coarse mottle (one cheap low-freq octave) always; the two FINER octaves only on High/Ultra.
    // `params2.y` (ground quality lane: Low 0 / High 1 / Ultra 2) is a UNIFORM, so this branch is
    // coherent across the whole draw — on Low the GPU genuinely skips the two extra noise evals.
    // The fine octaves are high-freq grain that's sub-perceptible on the weak GPUs Low targets, but
    // they (and the detail tap below) are the bulk of the per-fragment ground cost there.
    var ter_m = ter_noise_rot(wp * 0.5, 0.946, 0.326) * 0.55;
    if forest.params2.y >= 1.0 {
        ter_m += ter_noise_rot(wp * 1.7, 0.682, 0.731) * 0.30
               + ter_noise_rot(wp * 5.5, 0.292, 0.956) * 0.15;
    }
    rgb *= 0.83 + ter_m * 0.34;

    // (2) large-scale analytic hue + value drift — the soft cloudy patches (cure for
    //     flat green): broad areas drift warm yellow-green ↔ cool deep-green + lighten.
    //     The hue push is gated to green ground; the value drift applies everywhere.
    let ter_big = ter_noise(wp * 0.05) * 0.6 + ter_noise(wp * 0.14) * 0.4;
    let ter_hue = ter_noise(wp * 0.028 + vec2<f32>(11.0, 11.0));
    rgb += (ter_big - 0.5) * variation * vec3<f32>(0.10, 0.09, -0.05) * green;
    rgb *= 1.0 + (ter_hue - 0.5) * variation * 0.40;

    // (2b) cart-wheel ruts (map-character overhaul pass 2): twin darker grooves pressed either
    // side of every road artery, from the baked fragment-resolution mask. A textureSample must
    // stay in UNIFORM control flow, so sample unconditionally (the 1×1 dummy + zero region make
    // this a no-op on roadless grounds); a broken-wear noise keeps the grooves from reading as
    // two ruler lines.
    let rut_uv = (wp - forest.rut_region.xy) * forest.rut_region.zw;
    let rut_in = f32(forest.rut_region.z > 0.0
        && rut_uv.x > 0.0 && rut_uv.x < 1.0 && rut_uv.y > 0.0 && rut_uv.y < 1.0);
    let rut = textureSample(rut_tex, rut_samp, rut_uv).r * rut_in;
    let rut_wear = 0.65 + 0.35 * ter_noise(wp * 0.9 + vec2<f32>(31.0, 7.0));
    // 0.55: tuned by A/B shots — 0.22 and even 0.38 drowned under the ±0.17 ground mottle +
    // midday haze/bloom (verification: "ruts invisible"); a 1.0 debug pass proved the sampling
    // chain and read as mud trenches. 0.55·wear ≈ 0.36–0.55 darkening keeps two legible worn
    // grooves without going black.
    rgb *= 1.0 - 0.55 * rut * rut_wear;

    // ── Cliff-rock albedo (the "tekstura urwiska") on flagged crag faces: warped
    //    near-horizontal SEDIMENTARY STRATA + soft-edged FRACTURE PLATES over a
    //    stone-greyed base. Greying keeps a whisper of the vertex-colour biome hue
    //    (sandstone / granite / snow-blue), so desert mesas and snow crags still differ.
    //    Cheap (4 noise evals, no texture tap) and gated per-vertex, so it runs at every
    //    quality tier — the geometry facets need the albedo to read as rock even on Low.
    if cliff > 0.004 {
        let wy = in.world_position.y;
        // Strata bands: high frequency in Y, very low along XZ, undulated by a broad
        // world-space warp so the layers never run ruler-straight.
        let cwarp = (ter_noise(wp * 0.30) - 0.5) * 2.6;
        let strata = ter_noise(vec2<f32>(wy * 1.5 + cwarp, dot(wp, vec2<f32>(0.35, 0.27))));
        // Fracture plates: coarse noise quantised to a few tones with softened edges —
        // the chiseled-slab patchwork of a real crag face.
        let pl = ter_noise(vec2<f32>(dot(wp, vec2<f32>(0.85, 0.62)) + 13.0, wy * 0.85)) * 3.0;
        let plate = (floor(pl) + smoothstep(0.30, 0.70, fract(pl))) / 3.0;
        let clum = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
        let stone = mix(rgb, vec3<f32>(clum) * vec3<f32>(1.03, 1.0, 0.95), 0.55);
        let rockc = stone * (0.80 + strata * 0.30) * (0.82 + plate * 0.30);
        rgb = mix(rgb, rockc, cliff * 0.88);
    }

    // (3)+(4) only on High/Ultra — same coherent `params2.y` lane as the fine mottle above.
    // (3) is a TEXTURE tap + a domain-warp noise pair (the single most expensive ground op) and
    // (4) is two more low-freq noise evals; both are skipped wholesale on Low, where the weak GPU
    // this preset targets can't afford them and the subtle imprint/scuff variety reads as flat
    // green anyway. The base colour + coarse mottle + drift keep the ground from looking bald.
    if forest.params2.y >= 1.0 {
        // (3) soft grass detail imprint on up-facing fragments, normalised by mean luminance.
        //     Off-grass the imprint collapses to its luminance so snow keeps the blade-scale
        //     value texture without picking up the green cast.
        // Soft slope fade, NOT a hard `step(0.5, …)`. The chamfered terrace lip rolls its
        // normal from ~0.92 (top edge) to ~0.37 (base) across ONE facet, so a hard step cut
        // the grass imprint along a line mid-triangle → a crisp green "wedge" on the cliff
        // lip. `smoothstep` fades the imprint down the lip instead (band matches `topw`
        // above), so the lip keeps grass but melts into the dirt wall with no triangle.
        let top_face = smoothstep(0.35, 0.80, in.world_normal.y);
        let det = sample_detail(wp, detail_scale) / max(mean, 0.01);
        let det_l = dot(det, vec3<f32>(0.2126, 0.7152, 0.0722));
        let det_c = mix(vec3<f32>(det_l), det, green);
        rgb *= mix(vec3<f32>(1.0), det_c, detail_strength * top_face);

        // (4) macro albedo-variety: broad low-freq patches so the field reads as a living meadow
        //     of mottled greens — worn dirt, damp moss, sun-dried gold, lush deep-green — instead
        //     of one flat tone. All green-gated + preset-scaled (`macro_variety`); each on its own
        //     noise offset/frequency so they overlap into organic patches, not a regular quilt.
        //     Kept under ~0.4 mix so it's varied, not camo.
        let variety = forest.params2.z;
        // Worn sun-bleached dirt scuffs (bald spots / trodden paths) — warm + desaturated.
        // Lower thresholds widen each patch and stronger mixes raise the contrast between
        // them, so the field reads as a genuinely varied meadow rather than one flat tone.
        // (Toned down 2026-07-03, player: "ta biała tekstura gruntu mi się nie podoba" — the
        // broad ~50u worn patches + pale soil blotches below washed whole meadows/forest floors
        // toward chalky white under bloom. Rarer, smaller, warmer.)
        let worn = smoothstep(0.66, 0.90, ter_noise(wp * 0.020 + vec2<f32>(5.0, 9.0)));
        rgb = mix(rgb, rgb * vec3<f32>(0.80, 0.70, 0.52), worn * 0.20 * green * variety);
        // Damp moss hollows — cool, rich, slightly darker green.
        let moss = smoothstep(0.52, 0.86, ter_noise(wp * 0.040 + vec2<f32>(19.0, 2.0)));
        rgb = mix(rgb, rgb * vec3<f32>(0.66, 0.90, 0.56), moss * 0.44 * green * variety);
        // Sun-dried golden sweeps — drier grass catching the light (a brighter warm push).
        let dry = smoothstep(0.70, 0.93, ter_noise(wp * 0.015 + vec2<f32>(33.0, 7.0)));
        rgb = mix(rgb, rgb * vec3<f32>(1.06, 1.02, 0.80), dry * 0.10 * green * variety);
        // Lush well-watered patches — deep saturated green.
        let lush = smoothstep(0.56, 0.89, ter_noise(wp * 0.030 + vec2<f32>(2.0, 27.0)));
        rgb = mix(rgb, rgb * vec3<f32>(0.70, 1.03, 0.66), lush * 0.42 * green * variety);

        // ── Forest-floor debris baked into the texture (the "patyki + szare placki"): scattered
        //    twigs and bare-earth / lichen patches, so the floor reads littered EVERYWHERE, not
        //    only where a 3D litter prop happens to sit. Top-faces only (`topd`) + green-gated so
        //    it never smears onto cliff walls / snow / sand; preset-scaled by `variety`.
        let topd = smoothstep(0.35, 0.80, in.world_normal.y);
        let debris_w = topd * green * variety;
        // (a) Bare-earth / lichen blotches — the "szare placki": broad low-freq blobs drifting
        //     the green toward a desaturated grey-tan soil. One main blob field thresholded so
        //     patches are clearly readable, broken up by a finer octave so their edges are ragged
        //     (organic), not clean ovals; a faint grain keeps them from looking painted.
        // Rarer + a warm humus BROWN, not the old grey-tan (0.45,0.44,0.41) that read as bald
        // white patches in the meadow and the forest floor.
        let soil = smoothstep(0.68, 0.84, ter_noise(wp * 0.06 + vec2<f32>(61.0, 13.0)));
        let soil_edge = soil * (0.70 + 0.30 * ter_noise(wp * 0.28 + vec2<f32>(7.0, 51.0)));
        let earth = vec3<f32>(0.34, 0.27, 0.19) * (0.90 + ter_m * 0.20);
        rgb = mix(rgb, earth, soil_edge * 0.48 * debris_w);
        // (b) Scattered twigs (organic — warped/tapered/soft, see `twig_field`): a brown-bark
        //     pass + a sparser, greyer driftwood pass (offset seed so they don't coincide).
        //     Bark colour drifts dark↔mid down each stick (`.y`) so it's not a flat fill.
        let tb = twig_field(wp, 0.0, 0.024);
        let bark = mix(vec3<f32>(0.28, 0.19, 0.11), vec3<f32>(0.45, 0.34, 0.21), tb.y);
        rgb = mix(rgb, bark, tb.x * 0.66 * debris_w);
        let tg = twig_field(wp, 123.4, 0.018);
        let drift = mix(vec3<f32>(0.38, 0.35, 0.30), vec3<f32>(0.55, 0.52, 0.46), tg.y);
        rgb = mix(rgb, drift, tg.x * 0.48 * debris_w);
    }

    // Clamp alpha ≥ 0 on write-back: the negative cliffness flag has served its purpose and
    // must not leak into lighting/wetness (the wet gate below reads this same channel).
    pbr_input.material.base_color = vec4<f32>(max(rgb, vec3<f32>(0.0)), max(pbr_input.material.base_color.a, 0.0));

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
        var n2 = normalize(gn + vec3<f32>(-hx, 0.0, -hz) * bump * topw * 1.35);

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
            // Flagged crag faces get a stronger relief push — the displaced facet geometry
            // plus deeper runnels is what sells the rock; plain dirt banks keep the old 0.7.
            n2 = normalize(n2 - (tang * dha + vec3<f32>(0.0, 1.0, 0.0) * dhy) * bump * (0.7 + cliff * 0.5) * sidew);
            // Subtle crevice darkening so the relief reads in albedo too, not only in shading.
            let crev = rock_h(along, wy);
            let dk = mix(1.0, 0.82 + crev * 0.30, sidew);
            pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * dk, pbr_input.material.base_color.a);
        }

        // ── Cavity ambient occlusion — the depth cue normal-tilt alone CAN'T give, and the
        //    single biggest reason a flat ground "fakes 3D". A tilted shading normal still
        //    catches ambient from every direction, so under the bright sky-ambient here the
        //    raised clumps read shaded-on-one-side but never DEEP. Baking soft self-shadow
        //    into the hollows of the SAME height field (so albedo + relief agree) carves the
        //    grooves the eye reads as real geometry — exactly what gives a photo-relief
        //    root/dirt texture its depth, but procedural (no baked map, biome-agnostic).
        //    Top faces only (`topw`); value-only so it's safe on grass/dirt/snow with no hue
        //    cast; entirely inside the `bump` branch so it's free on Low (relief off there).
        let h0 = terrain_h(wp);
        // (a) broad mound AO: darken the low ground BETWEEN clumps. Wide range + a low floor
        //     so hollows go genuinely deep (~0.46×), not a faint tint.
        let mound = smoothstep(0.10, 0.82, h0);
        // (b) crease network: three rotated octaves (≈36°/53°/73°) so the grooves are dense
        //     and read at both the clump scale and crisp near-camera detail, never lining up
        //     into an axis grid.
        let crease = ter_noise_rot(wp * 1.7, 0.81, 0.59) * 0.34
                   + ter_noise_rot(wp * 3.3, 0.60, 0.80) * 0.40
                   + ter_noise_rot(wp * 6.7, 0.29, 0.957) * 0.26;
        let groove = smoothstep(0.32, 0.70, crease);
        // Deepest where a broad hollow AND a fine groove coincide; strong sun-kissed crowns on
        // the mound tops so the relief pops both ways (bright peaks ↔ dark pits).
        let ao = mix(0.56, 1.0, mound) * mix(0.72, 1.0, groove);
        let crown = 1.0 + smoothstep(0.62, 1.0, h0) * 0.20;
        let shade = mix(1.0, ao * crown, topw);
        pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * shade, pbr_input.material.base_color.a);

        pbr_input.N = n2;
    }

    // ── Wet ground (marsh) — the vertex-colour ALPHA carries a per-vertex wetness (0 = dry … 1 =
    //    standing bog), which `worldmap::ground_color` feathers over the biome BLEND band. Lower the
    //    roughness toward a damp sheen by it, so the marsh catches a broad specular highlight AND the
    //    wet look blends smoothly across the swamp↔grass boundary instead of switching per material
    //    sheet (the tile-square "kwadraty" a player saw where the swamp begins). Every terrain sheet
    //    runs this identically, so a shared boundary vertex resolves the SAME roughness from either
    //    side — no seam. A no-op (wet≈0) on all dry ground, so grass/sand/snow are unaffected.
    let wet = clamp(pbr_input.material.base_color.a, 0.0, 1.0);
    if wet > 0.001 {
        let rw = pbr_input.material.perceptual_roughness;
        pbr_input.material.perceptual_roughness = mix(rw, 0.40, wet);
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
