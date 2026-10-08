// Procedural creature surface texturing — a StandardMaterial fragment EXTENSION.
// Reads vertex-colour rgb as hue and vertex-colour alpha as a SURFACE CODE, samples cheap
// model-space value-noise, and subtly perturbs base colour / roughness / normal per surface
// family. PBR lighting (sun/IBL/fog/exposure) stays exact. Output is forced opaque so the
// repurposed alpha never leaks into transparency.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    mesh_functions,
    mesh_view_bindings::view,
    forward_io::{VertexOutput, FragmentOutput},
}

struct CreatureParams {
    params: vec4<f32>,
    // x = triplanar scale, y = height bump, z = roughness multiplier, w = mode
    // (0 = the procedural surf families below, 1 = sampled triplanar PBR).
    pbr: vec4<f32>,
};
// Bevy injects the material bind-group index; hardcoding @group(2) breaks on pipeline changes.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> creature: CreatureParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var ft_alb: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var ft_alb_s: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var ft_mr: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var ft_mr_s: sampler;

// Object-space normal. Bevy writes `world_normal = normalize(inverse_transpose(M) * objectNormal)`,
// so `transpose(M) * world_normal` is parallel to the raw attribute — including on the footman's
// non-uniformly scaled parts (pauldrons, straps). A pure inverse would skew those weights.
fn ft_obj_normal(m: mat3x3<f32>, n_world: vec3<f32>) -> vec3<f32> {
    return normalize(transpose(m) * n_world);
}

fn ft_tri_w(n: vec3<f32>) -> vec3<f32> {
    let w = pow(abs(n), vec3<f32>(4.0));
    return w / max(w.x + w.y + w.z, 1e-5);
}

fn ft_tri(t: texture_2d<f32>, s: sampler, p: vec3<f32>, n: vec3<f32>, sc: f32) -> vec4<f32> {
    let w = ft_tri_w(n);
    let q = p * sc;
    return textureSample(t, s, q.zy) * w.x
        + textureSample(t, s, q.xz + vec2<f32>(0.37, 0.37)) * w.y
        + textureSample(t, s, q.xy + vec2<f32>(0.71, 0.71)) * w.z;
}

// Screen-space height bump, the same construction as the three.js `tpPerturb`.
fn ft_perturb(sp: vec3<f32>, n: vec3<f32>, d_h: vec2<f32>) -> vec3<f32> {
    let sx = normalize(dpdx(sp));
    let sy = normalize(dpdy(sp));
    let r1 = cross(sy, n);
    let r2 = cross(n, sx);
    let det = dot(sx, r1);
    let g = sign(det) * (d_h.x * r1 + d_h.y * r2);
    return normalize(abs(det) * n - g);
}

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * 0.3183099 + vec3(0.1, 0.2, 0.3));
    let r = q + dot(q, q.yzx + 19.19);
    return fract((r.x + r.y) * r.z);
}

// Smooth 3D value noise in [0,1].
fn vnoise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let c000 = hash3(i + vec3(0.0, 0.0, 0.0));
    let c100 = hash3(i + vec3(1.0, 0.0, 0.0));
    let c010 = hash3(i + vec3(0.0, 1.0, 0.0));
    let c110 = hash3(i + vec3(1.0, 1.0, 0.0));
    let c001 = hash3(i + vec3(0.0, 0.0, 1.0));
    let c101 = hash3(i + vec3(1.0, 0.0, 1.0));
    let c011 = hash3(i + vec3(0.0, 1.0, 1.0));
    let c111 = hash3(i + vec3(1.0, 1.0, 1.0));
    let x00 = mix(c000, c100, u.x);
    let x10 = mix(c010, c110, u.x);
    let x01 = mix(c001, c101, u.x);
    let x11 = mix(c011, c111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Model-space coordinate locked to this (possibly animated) part: exact for the rigs'
    // rotation + uniform-scale instance transforms, no 4x4 inverse.
    let model = mesh_functions::get_world_from_local(in.instance_index);
    let m = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let origin = model[3].xyz;
    // Per-axis unscale → true model-space coord even under NON-uniform instance scale (e.g. the
    // hero's stocky rig scale): divide each local axis by its own length² (uniform scale ⇒ identical
    // to the old single-divide, so creatures are unaffected). Stops the texture warping/artifacts.
    let lp = transpose(m) * (in.world_position.xyz - origin);
    let obj = vec3<f32>(
        lp.x / max(dot(m[0], m[0]), 1e-5),
        lp.y / max(dot(m[1], m[1]), 1e-5),
        lp.z / max(dot(m[2], m[2]), 1e-5),
    );

#ifdef VERTEX_COLORS
    let surf = in.color.a;        // surface code (see Surf::surf_code)
#else
    let surf = 1.0;               // uncoloured mesh: the neutral skin bucket
#endif
    let strength = creature.params.x;
    let relief = creature.params.y;
    let spec_lift = creature.params.z;

    // Footman (and any future textured hero part): real triplanar albedo / roughness / height,
    // multiplied onto the material tint. Orks, villagers and wildlife stay on the procedural
    // surf families below (`pbr.w == 0`) and never sample these textures.
    if (creature.pbr.w > 0.5) {
        let on = ft_obj_normal(m, pbr_input.N);
        let sc = creature.pbr.x;
        let alb = ft_tri(ft_alb, ft_alb_s, obj, on, sc);
        let mr = ft_tri(ft_mr, ft_mr_s, obj, on, sc);
        let rgb = pbr_input.material.base_color.rgb * alb.rgb;
        pbr_input.material.base_color = vec4<f32>(max(rgb, vec3<f32>(0.0)), 1.0);
        pbr_input.material.perceptual_roughness = clamp(mr.g * creature.pbr.z, 0.05, 1.0);
        let d_h = vec2<f32>(dpdx(mr.r), dpdy(mr.r)) * creature.pbr.y;
        pbr_input.N = ft_perturb(in.world_position.xyz, pbr_input.N, d_h);
    } else {

    // Decode the surface family from the alpha bucket (see Surf::surf_code).
    // 0.07 Skin · 0.21 Fur · 0.36 Scale · 0.50 Stone · 0.64 Metal · 0.79 Cloth · 0.93 Bone
    var lum = 1.0;
    var rough_adj = 0.0;
    var rgb = pbr_input.material.base_color.rgb;

    if (surf < 0.14 || surf > 0.965) {
        // Skin / hide — soft low-freq mottle + faint pores. Also the UNTAGGED default
        // (vertex alpha 1.0 from `lin()`), so any un-surfed mesh reads as neutral skin.
        let n = vnoise(obj * 9.0) - 0.5;
        let pore = (vnoise(obj * 38.0) - 0.5) * 0.4;
        lum = 1.0 + (n + pore) * strength;
        rough_adj = 0.05;
    } else if (surf < 0.28) {
        // Fur — streaks stretched along the part's long axis (Y in part space).
        let p = obj * vec3<f32>(26.0, 7.0, 26.0);
        let f = (vnoise(p) - 0.5) + (vnoise(p * 2.3) - 0.5) * 0.5;
        lum = 1.0 + f * strength * 1.4;
        rough_adj = 0.12;
    } else if (surf < 0.43) {
        // Scale — cellular: quantise position into cells, darken cell edges.
        let cell = floor(obj * 16.0);
        let r = hash3(cell);
        let edge = fract(obj.x * 16.0) * fract(obj.y * 16.0);
        lum = 1.0 + (r - 0.5) * strength * 1.2 - (1.0 - smoothstep(0.05, 0.2, edge)) * strength;
        rough_adj = -0.05;
    } else if (surf < 0.57) {
        // Stone — broadband mottle + sparse bright speckle, rougher.
        let n = (vnoise(obj * 8.0) - 0.5) + (vnoise(obj * 22.0) - 0.5) * 0.5;
        let spk = step(0.92, vnoise(obj * 40.0));
        lum = 1.0 + n * strength * 1.3 + spk * strength * 2.0;
        rough_adj = 0.18;
    } else if (surf < 0.71) {
        // Metal — reflective plate: raise metallic so the reflection tints to the base hue
        // (gold/steel) instead of mirroring the white sky; keep roughness moderate so it reads
        // as a soft sheen, not a blown-out mirror.
        let n = vnoise(obj * 30.0) - 0.5;
        lum = 1.0 + n * strength * 0.55; // a touch more grain so plate reads textured, not plastic
        rough_adj = -0.18;
        pbr_input.material.metallic = clamp(spec_lift, 0.0, 1.0); // per-material sheen (params.z); hero stays matte
    } else if (surf < 0.86) {
        // Cloth — fine weave grain.
        let weave = (sin(obj.x * 120.0) * sin(obj.y * 120.0)) * 0.5;
        lum = 1.0 + weave * strength * 0.6;
        rough_adj = 0.10;
    } else {
        // Bone — fine grain, slightly polished.
        let n = vnoise(obj * 24.0) - 0.5;
        lum = 1.0 + n * strength * 0.7;
        rough_adj = -0.05;
    }

    rgb = rgb * lum;
    pbr_input.material.base_color = vec4<f32>(max(rgb, vec3<f32>(0.0)), 1.0); // force opaque — alpha was the surf code
    pbr_input.material.perceptual_roughness =
        clamp(pbr_input.material.perceptual_roughness + rough_adj, 0.05, 1.0);

    // Micro-relief: perturb the normal slightly by the noise gradient (cheap finite diff).
    if (relief > 0.0) {
        let e = 0.02;
        let dx = vnoise(obj * 18.0 + vec3<f32>(e, 0.0, 0.0)) - vnoise(obj * 18.0 - vec3<f32>(e, 0.0, 0.0));
        let dz = vnoise(obj * 18.0 + vec3<f32>(0.0, 0.0, e)) - vnoise(obj * 18.0 - vec3<f32>(0.0, 0.0, e));
        pbr_input.N = normalize(pbr_input.N + (m * vec3<f32>(dx, 0.0, dz)) * relief * 0.5);
    }
    } // procedural surf (`pbr.w == 0`)

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
        // Camera-space key light for the first-person viewmodel (params.w > 0 opts in): the world's
        // sun/IBL knows nothing about the lens, so facing the sun (or standing in shade) would
        // silhouette the hands and weapon. This lights them from up-left-front of the camera at a
        // fixed strength, exposure-scaled like every other light, so they always read.
        let fill = creature.params.w;
        if (fill > 0.0) {
            let cam_right = view.world_from_view[0].xyz;
            let cam_up = view.world_from_view[1].xyz;
            let cam_back = view.world_from_view[2].xyz;
            let to_key = normalize(-0.55 * cam_right + 0.75 * cam_up + 0.6 * cam_back);
            let half_lambert = clamp(dot(normalize(pbr_input.N), to_key) * 0.5 + 0.5, 0.0, 1.0);
            let key = fill * view.exposure * half_lambert * half_lambert;
            out.color = vec4<f32>(out.color.rgb + pbr_input.material.base_color.rgb * key, out.color.a);
        }
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
