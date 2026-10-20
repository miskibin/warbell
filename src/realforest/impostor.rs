//! **Far-tree impostors** — past [`IMP_NEAR`] a kit tree stops drawing its LOD1 mesh and becomes
//! one camera-facing quad that shows the tree as baked from N x N directions over the upper
//! hemisphere (`tools/blender_forest/impostors.py` → `impostor_{albedo,nd}.dds`, one array layer
//! per model). The shader (`impostor.wesl`) blends the 4 nearest views with per-view re-projection
//! + a depth-parallax step and lights the result with the stock PBR path, so a far tree keeps its
//! silhouette, turns with the view and shades with the sun.
//!
//! Cheap where the LOD1 meshes weren't: the quads are merged into one mesh per [`CHUNK`]-unit
//! region (a whole region of forest is one draw, and the CPU sees a few dozen entities instead of
//! 2-4 per tree), the swap distance and the far dissolve are evaluated per tree in the vertex /
//! fragment stages, and the far end fades out by dissolving leaf clumps instead of popping.
//!
//! A tree registers by carrying [`ImpostorTree`] (inserted by `Kit::attach_tree_with`); chunks
//! rebuild when a tree spawns, despawns, is felled (`verbs::Stump`) or regrows.
//! `FOREST_IMPOSTORS=0` keeps the old LOD1-to-180u trees (A/B).

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

pub type ImpostorMaterial = ExtendedMaterial<StandardMaterial, ImpostorExt>;

/// LOD1 → impostor hand-off (base units, stretched by the render distance's ground factor).
pub const IMP_NEAR: f32 = 62.0;
/// LOD0 → impostor cross-fade band (world units): over the last `IMP_FADE_IN` units of the LOD0
/// range its crown drops leaf clumps (`foliage::dissolve` on the canopy twin) while the impostor
/// gains them (`impostor.wesl`'s near keep, same constant there) — a clump-by-clump hand-off
/// instead of a whole-tree pop. Without a depth prepass (low presets) the crown can't dissolve,
/// so the two just overlap through the band.
pub const IMP_FADE_IN: f32 = 10.0;
/// Quads are pushed this many bounding radii toward the eye, so the ground at a tree's foot
/// doesn't cut its trunk when seen from above (the quad plane would dip into the slope).
const PUSH: f32 = 0.55;
/// The far dissolve spans the last fraction of the tree draw distance.
const FAR_FADE: f32 = 0.18;
/// Impostor draw distance as a multiple of the old LOD1 tree range (`TREE_FAR` x render distance).
/// A quad per tree in one merged mesh per chunk costs next to nothing far out, and past `TREE_FAR`
/// whole valleys and hillside forests used to stand bald — so the trees now reach the fog.
/// `FOREST_IMPFAR=<x>` overrides it (1 = the old range).
const FAR_MUL: f32 = 2.4;
/// Chunks nearer than this are always drawn (they cast the shadows onto what is on screen); farther
/// ones are culled against the view frustum's sides only — the camera's `far` (230u at Normal)
/// would otherwise frustum-cull every chunk past it.
const ALWAYS_R: f32 = 140.0;
/// Edge of one merged chunk (world units).
const CHUNK: f32 = 64.0;
const MAX_LAYERS: usize = 32;

/// Impostors wanted? (`FOREST_IMPOSTORS=0` keeps the old LOD1-to-the-horizon trees.) Read once.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_IMPOSTORS").map(|v| v != "0").unwrap_or(true))
}

/// The baked atlases + per-model sphere bounds (loaded with the kit).
pub struct Atlas {
    pub albedo: Handle<Image>,
    pub nd: Handle<Image>,
    pub frames: f32,
    pub cutoff: f32,
    /// Per layer: model-space sphere centre (xyz) + radius (w), model units.
    pub models: Vec<Vec4>,
    /// Kit model index (the `_lod0`) → atlas layer.
    pub layer_of: HashMap<usize, u8>,
}

/// Load `impostors.json` + the two packed arrays. `by_name` maps kit model names to indices.
pub fn load_atlas(images: &mut Assets<Image>, by_name: &HashMap<String, usize>) -> Result<Atlas, String> {
    let path = super::load::asset_path("forest_pbr/impostors.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let meta: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut models = Vec::new();
    let mut layer_of = HashMap::new();
    for (layer, m) in meta["models"].as_array().ok_or("impostors.json: no models")?.iter().enumerate() {
        let name = m["name"].as_str().ok_or("impostors.json: model without name")?;
        let c = &m["center"];
        let f = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
        models.push(Vec4::new(f(&c[0]), f(&c[1]), f(&c[2]), f(&m["radius"])));
        if let Some(&idx) = by_name.get(&format!("{name}_lod0")) {
            layer_of.insert(idx, layer as u8);
        }
    }
    if models.len() > MAX_LAYERS {
        return Err(format!("impostors.json: {} models > {MAX_LAYERS}", models.len()));
    }
    let sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    let mut arr = |name: &str| -> Result<Handle<Image>, String> {
        let rel = format!("forest_pbr/impostor_{name}.dds");
        if !super::load::asset_path(&rel).exists() {
            return Err(format!("{rel} missing (PACK_ONLY=impostor pack_forest_textures)"));
        }
        let mut img = super::load::texture_array(&format!("forest_pbr/impostor_{name}.dds"), &[], name == "albedo")?;
        img.sampler = sampler.clone();
        Ok(images.add(img))
    };
    Ok(Atlas {
        albedo: arr("albedo")?,
        nd: arr("nd")?,
        frames: meta["frames"].as_f64().unwrap_or(8.0) as f32,
        cutoff: meta["cutoff"].as_f64().unwrap_or(0.42) as f32,
        models,
        layer_of,
    })
}

/// A tree drawn as an impostor past [`IMP_NEAR`]: its atlas layer + snow dusting.
#[derive(Component, Clone, Copy)]
pub struct ImpostorTree {
    pub layer: u8,
    pub snow: f32,
}

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct ImpostorParams {
    /// x frames per side, y alpha cutoff, z impostor start distance, w quad push (radii).
    pub cfg: Vec4,
    /// x far-fade start, y far end, z crown tint strength, w parallax on/off.
    pub far: Vec4,
    pub models: [Vec4; MAX_LAYERS],
}

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct ImpostorExt {
    #[uniform(100)]
    pub params: ImpostorParams,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub albedo: Handle<Image>,
    #[texture(103, dimension = "2d_array")]
    #[sampler(104)]
    pub nd: Handle<Image>,
}

impl MaterialExtension for ImpostorExt {
    fn vertex_shader() -> ShaderRef {
        "shaders/impostor.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/impostor.wesl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/impostor_prepass.wesl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/impostor_prepass.wesl".into()
    }
    /// Main pass: EQUAL depth test against the prepass's alpha-tested depth, no depth write, no
    /// discard in the shader (`impostor.wesl`), so only visible texels are shaded and early-Z
    /// stays on — the same trick as the foliage cards (`foliage.rs`).
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        let main = descriptor.label.as_deref().is_some_and(|l| l.ends_with("opaque_mesh_pipeline"));
        if main && key.mesh_key.contains(bevy::pbr::MeshPipelineKey::DEPTH_PREPASS) {
            if let Some(ds) = descriptor.depth_stencil.as_mut() {
                ds.depth_compare = Some(bevy::render::render_resource::CompareFunction::Equal);
                ds.depth_write_enabled = Some(false);
            }
        } else if main {
            // No depth prepass (low presets strip it when no DoF/AO/outline/god-rays/TAA needs it):
            // nothing alpha-tested the quad, so the fragment stage must discard itself or the
            // whole card draws as an opaque slab.
            if let Some(f) = descriptor.fragment.as_mut() {
                f.shader_defs.push("IMPOSTOR_ALPHA_TEST".into());
            }
        }
        Ok(())
    }
}

#[derive(Resource)]
struct ImpostorRes {
    material: Handle<ImpostorMaterial>,
}

#[derive(Default)]
struct Chunk {
    trees: Vec<Entity>,
    entity: Option<Entity>,
    /// World bounds of the merged quads (for [`cull_chunks`]).
    bounds: Option<bevy::camera::primitives::Aabb>,
}

#[derive(Resource, Default)]
struct Chunks {
    by_key: HashMap<(i32, i32), Chunk>,
    of: HashMap<Entity, (i32, i32)>,
    dirty: HashSet<(i32, i32)>,
}

pub struct ImpostorPlugin;

impl Plugin for ImpostorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ImpostorMaterial>::default());
        if !enabled() {
            return;
        }
        app.init_resource::<Chunks>().add_systems(Startup, setup).add_systems(
            PostUpdate,
            (track_trees, rebuild_chunks, sync_params, cull_chunks)
                .chain()
                .after(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// Distances for the current render-distance multiplier: (impostor start, far fade start, far end).
fn distances() -> (f32, f32, f32) {
    let k = crate::quality::view_scale();
    let near = IMP_NEAR * crate::quality::ground_factor(k) - IMP_FADE_IN;
    let far = super::TREE_FAR * k * far_mul();
    (near, far * (1.0 - FAR_FADE), far)
}

fn far_mul() -> f32 {
    static V: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("FOREST_IMPFAR").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(FAR_MUL))
}

fn params(atlas: &Atlas) -> ImpostorParams {
    let (near, far0, far1) = distances();
    let mut models = [Vec4::new(0.0, 1.0, 0.0, 1.0); MAX_LAYERS];
    for (slot, m) in models.iter_mut().zip(&atlas.models) {
        *slot = *m;
    }
    let parallax = if std::env::var("FOREST_IMPPARALLAX").is_ok_and(|v| v == "0") { 0.0 } else { 1.0 };
    let push = std::env::var("FOREST_IMPPUSH").ok().and_then(|v| v.parse().ok()).unwrap_or(PUSH);
    ImpostorParams {
        cfg: Vec4::new(atlas.frames, atlas.cutoff, near, push),
        far: Vec4::new(far0, far1, 0.55, parallax),
        models,
    }
}

fn setup(mut commands: Commands, mut mats: ResMut<Assets<ImpostorMaterial>>) {
    let Some(atlas) = super::get().and_then(|k| k.impostors.as_ref()) else { return };
    let material = mats.add(ExtendedMaterial {
        base: StandardMaterial {
            alpha_mode: AlphaMode::Mask(atlas.cutoff),
            cull_mode: None,
            perceptual_roughness: 0.66,
            reflectance: 0.3,
            ..default()
        },
        extension: ImpostorExt { params: params(atlas), albedo: atlas.albedo.clone(), nd: atlas.nd.clone() },
    });
    commands.insert_resource(ImpostorRes { material });
}

fn key(p: Vec3) -> (i32, i32) {
    ((p.x / CHUNK).floor() as i32, (p.z / CHUNK).floor() as i32)
}

/// File new trees into chunks, and dirty the chunks of trees that despawned / were felled /
/// regrew / changed visibility.
fn track_trees(
    mut chunks: ResMut<Chunks>,
    added: Query<(Entity, &GlobalTransform), Added<ImpostorTree>>,
    changed: Query<Entity, (With<ImpostorTree>, Or<(Changed<Visibility>, Added<crate::verbs::Stump>)>)>,
    mut removed: RemovedComponents<ImpostorTree>,
    mut regrown: RemovedComponents<crate::verbs::Stump>,
) {
    let chunks = &mut *chunks;
    for e in removed.read() {
        if let Some(k) = chunks.of.remove(&e) {
            if let Some(c) = chunks.by_key.get_mut(&k) {
                c.trees.retain(|t| *t != e);
            }
            chunks.dirty.insert(k);
        }
    }
    for (e, gt) in &added {
        let k = key(gt.translation());
        chunks.by_key.entry(k).or_default().trees.push(e);
        chunks.of.insert(e, k);
        chunks.dirty.insert(k);
    }
    for e in changed.iter().chain(regrown.read()) {
        if let Some(&k) = chunks.of.get(&e) {
            chunks.dirty.insert(k);
        }
    }
}

fn rebuild_chunks(
    mut commands: Commands,
    mut chunks: ResMut<Chunks>,
    res: Option<Res<ImpostorRes>>,
    mut meshes: ResMut<Assets<Mesh>>,
    trees: Query<(&GlobalTransform, &ImpostorTree, &Visibility, Option<&crate::wind::Sway>, Has<crate::verbs::Stump>)>,
) {
    let Some(res) = res else { return };
    let Some(atlas) = super::get().and_then(|k| k.impostors.as_ref()) else { return };
    if chunks.dirty.is_empty() {
        return;
    }
    let dirty: Vec<_> = chunks.dirty.drain().collect();
    for k in dirty {
        let Some(chunk) = chunks.by_key.get_mut(&k) else { continue };
        let (mut pos, mut uv0, mut uv1, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for &e in &chunk.trees {
            let Ok((gt, imp, vis, sway, stump)) = trees.get(e) else { continue };
            if stump || *vis == Visibility::Hidden {
                continue;
            }
            let (scale, rot, base) = gt.to_scale_rotation_translation();
            // Sway rewrites the near trees' rotation as lean * base yaw; the rest yaw is what the
            // LOD meshes show at the hand-off distance.
            let yaw = sway.map(|s| s.base).unwrap_or(rot).to_euler(EulerRot::YXZ).0;
            let s = scale.x * super::TREE_MODEL_SCALE;
            let m = atlas.models[imp.layer as usize];
            let rs = m.w * s;
            let cw = Quat::from_rotation_y(yaw) * (m.truncate() * s);
            let i0 = pos.len() as u32;
            // The vertex stage turns the quad to the eye and pushes it out: bound the whole sphere.
            lo = lo.min(base + cw - Vec3::splat(rs * (1.0 + PUSH)));
            hi = hi.max(base + cw + Vec3::splat(rs * (1.0 + PUSH)));
            for (cu, cv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                pos.push((base + cw + Vec3::new(cu * rs, cv * rs, cu * rs)).to_array());
                uv0.push([cu, cv]);
                uv1.push([s, yaw]);
                col.push([imp.layer as f32, imp.snow, 0.0, 0.0]);
            }
            idx.extend_from_slice(&[i0, i0 + 1, i0 + 2, i0, i0 + 2, i0 + 3]);
        }
        if idx.is_empty() {
            if let Some(ent) = chunk.entity.take() {
                commands.entity(ent).try_despawn();
            }
            if chunk.trees.is_empty() {
                chunks.by_key.remove(&k);
            }
            continue;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv0);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        mesh.insert_indices(Indices::U32(idx));
        let handle = meshes.add(mesh);
        chunk.bounds = Some(bevy::camera::primitives::Aabb::from_min_max(lo, hi));
        match chunk.entity {
            Some(ent) => {
                commands.entity(ent).try_insert(Mesh3d(handle));
            }
            None => {
                chunk.entity = Some(
                    commands
                        .spawn((
                            Name::new("tree impostors"),
                            Mesh3d(handle),
                            MeshMaterial3d(res.material.clone()),
                            Transform::IDENTITY,
                            Visibility::default(),
                            bevy::camera::visibility::NoFrustumCulling,
                        ))
                        .id(),
                );
            }
        }
    }
}

/// Cull the chunks ourselves (they carry `NoFrustumCulling`): near ones always draw, far ones only
/// when their bounds meet the view frustum's sides — ignoring its far plane, which sits well inside
/// the impostors' reach.
fn cull_chunks(
    chunks: Res<Chunks>,
    cam: Query<(&GlobalTransform, &bevy::camera::primitives::Frustum), With<Camera3d>>,
    mut vis: Query<&mut Visibility>,
) {
    let Ok((cam_gt, frustum)) = cam.single() else { return };
    let eye = cam_gt.translation();
    for chunk in chunks.by_key.values() {
        let (Some(e), Some(b)) = (chunk.entity, chunk.bounds) else { continue };
        let Ok(mut v) = vis.get_mut(e) else { continue };
        let c = Vec3::from(b.center);
        let near = (eye - c).abs() - Vec3::from(b.half_extents);
        let show = near.max(Vec3::ZERO).length() < ALWAYS_R || frustum.intersects_obb(&b, &bevy::math::Affine3A::IDENTITY, true, false);
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

/// Follow the render-distance setting (rare: one material write per change).
fn sync_params(mut applied: Local<f32>, res: Option<Res<ImpostorRes>>, mut mats: ResMut<Assets<ImpostorMaterial>>) {
    let k = crate::quality::view_scale();
    if *applied == k {
        return;
    }
    let (Some(res), Some(atlas)) = (res, super::get().and_then(|k| k.impostors.as_ref())) else { return };
    if let Some(mut m) = mats.get_mut(&res.material) {
        m.extension.params = params(atlas);
        *applied = k;
    }
}
