// Wind-sway VERTEX override for `ExtendedMaterial<StandardMaterial, WindExt>` (src/foliage_wind.rs).
//
// Displaces ground-cover blades (grass tufts / flowers / ferns / reed sprigs) by a
// height-weighted sin/cos wander keyed on world XZ, so the meadow breathes in the wind. Only
// the VERTEX stage is overridden — the FRAGMENT stays the default StandardMaterial PBR, so
// lighting / IBL / fog / tonemap all still apply, and the cover keeps auto-batching.
//
// Why a baked height in COLOR.a instead of `position.y`: the scatter merges each chunk's cover
// into one mesh and bakes the *world* transform into the vertices, so `position.y` carries the
// TERRAIN elevation, not the blade's height above its planted base. `biome.rs` (upload_classes,
// cover path) bakes each vertex's terrain-independent local height into COLOR.a instead; the
// opaque prop output ignores alpha, so RGB/colour is untouched.
//
// Frequencies match the CPU tree sway (`wind.rs`): X = sin(t*1.5)+0.4*sin(t*3.1),
// Z = cos(t*1.2), per-blade phase = worldpos.x*0.7 + worldpos.z*0.55 — so grass and trees
// ripple coherently. Main-pass ONLY (no prepass twin): the prepass keeps the undisplaced cover
// depth, so watch for depth-silhouette shimmer on swaying blades (verified acceptable at the
// default amplitude; cover is NotShadowCaster + small).

#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    skinning,
    morph::{morph_position, morph_normal, morph_tangent},
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
}

struct WindParams {
    // x = master sway amplitude (world units per unit of blade height), y = gust depth,
    // z = gust frequency (rad/s), w = unused. Time = `globals.time` (in the main-pass view bind
    // group). There is deliberately NO prepass twin: globals is absent from the prepass view
    // layout, and feeding time via the material uniform every frame re-specialized every cover
    // mesh → a 10× CPU regression. See src/foliage_wind.rs.
    params: vec4<f32>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> wind: WindParams;

#ifdef MORPH_TARGETS
// The instance_index parameter must match vertex_in.instance_index. This is a work around for a wgpu dx12 bug.
// See https://github.com/gfx-rs/naga/issues/2416
fn morph_vertex(vertex_in: Vertex, instance_index: u32) -> Vertex {
    var vertex = vertex_in;
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;

    let weight_count = bevy_pbr::morph::layer_count(instance_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {
        let weight = bevy_pbr::morph::weight_at(i, instance_index);
        if weight == 0.0 {
            continue;
        }
        vertex.position += weight * morph_position(vertex_index, i, instance_index);
#ifdef VERTEX_NORMALS
        vertex.normal += weight * morph_normal(vertex_index, i, instance_index);
#endif
#ifdef VERTEX_TANGENTS
        vertex.tangent += vec4(weight * morph_tangent(vertex_index, i, instance_index), 0.0);
#endif
    }
    return vertex;
}
#endif

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;

#ifdef MORPH_TARGETS
    var vertex = morph_vertex(vertex_no_morph, vertex_no_morph.instance_index);
#else
    var vertex = vertex_no_morph;
#endif

    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);

#ifdef SKINNED
    var world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex_no_morph.instance_index
    );
#else
    var world_from_local = mesh_world_from_local;
#endif

#ifdef VERTEX_NORMALS
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_no_morph.instance_index
    );
#endif
#endif

#ifdef VERTEX_POSITIONS
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));

    // ── wind sway ── (see the file header + foliage_wind_prepass.wgsl, which must match)
#ifdef VERTEX_COLORS
    let bend = vertex.color.a; // local blade height baked by biome.rs (0 = planted base)
#else
    let bend = 0.0;
#endif
    let wt = globals.time;
    let phase = out.world_position.x * 0.7 + out.world_position.z * 0.55;
    // Slow field-wide gust envelope layered over the per-blade wander so the whole meadow surges.
    let gust = 1.0 + wind.params.y * sin(wt * wind.params.z + out.world_position.x * 0.03 + out.world_position.z * 0.02);
    let amp = bend * wind.params.x * gust;
    out.world_position.x += (sin(wt * 1.5 + phase) + 0.4 * sin(wt * 3.1 + phase * 1.7)) * amp;
    out.world_position.z += cos(wt * 1.2 + phase * 1.1) * amp;

    out.position = position_world_to_clip(out.world_position.xyz);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif

#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex_no_morph.instance_index
    );
#endif

#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, mesh_world_from_local[3]);
#endif

    return out;
}
