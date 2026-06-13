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
    forward_io::{VertexOutput, FragmentOutput},
}

struct CreatureParams { params: vec4<f32> };
// Bevy injects the material bind-group index; hardcoding @group(2) breaks on pipeline changes.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> creature: CreatureParams;

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
    let s2 = max(dot(m[0], m[0]), 1e-5);
    let origin = model[3].xyz;
    let obj = (transpose(m) * (in.world_position.xyz - origin)) / s2;

    let surf = in.color.a;        // surface code (see Surf::surf_code)
    let strength = creature.params.x;
    let relief = creature.params.y;
    let spec_lift = creature.params.z;

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
        lum = 1.0 + n * strength * 0.4;
        rough_adj = -0.25; // ~0.45, matches the original hero plate
        pbr_input.material.metallic = 0.3; // gentle sheen; high metallic mirrors the bright sky white
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

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
