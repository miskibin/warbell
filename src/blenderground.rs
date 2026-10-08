//! Blender-baked, repeatable ground surfaces for the opt-in campaign art pass.
//!
//! The island height field, river cuts, roads, collision and navigation remain in `worldmap`.
//! These eight Blender-authored surface bakes are packed into three texture atlases at startup;
//! `terrain.wgsl` chooses and blends them per fragment. Packing avoids binding 24 separate
//! textures to every terrain material and keeps the existing terrain chunk/LOD system intact.

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat};

use crate::biome::{Biome, BiomeConfig};
use crate::blenderenv::Model;
use crate::terrain::{ForestExtension, ForestParams, TerrainMaterial};

const DIR: &str = "assets/models/blender_environment";
pub const SURFACES: [&str; 8] = [
    "grass", "forest", "rocky", "swamp", "desert", "snow", "path", "cobble",
];
pub const TILE_WORLD: f32 = 2.5;
const TILE_PX: u32 = 512;
const COLS: u32 = 4;
const ROWS: u32 = 2;

#[derive(Clone)]
pub struct GroundAtlases {
    pub albedo: Handle<Image>,
    pub normal: Handle<Image>,
    pub roughness: Handle<Image>,
}

static ATLASES: OnceLock<GroundAtlases> = OnceLock::new();
static SLICE_ATLASES: OnceLock<GroundAtlases> = OnceLock::new();
static PATH_MASK: OnceLock<(Handle<Image>, Vec4)> = OnceLock::new();

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("FOREST_BLENDERWORLD").as_deref() == Ok("1"))
}

pub struct BlenderGroundPlugin;

impl Plugin for BlenderGroundPlugin {
    fn build(&self, app: &mut App) {
        if enabled() {
            app.add_systems(Update, (sync_courtyard_paving, stream_meadow_infill));
        }
    }
}

pub const SLICE_CENTER: Vec2 = Vec2::new(500.0, 500.0);
pub const SLICE_SIZE: f32 = 48.0;
// Keep the authored 48u composition exact, then carry its path into the empty
// terrain fringe so the ground plate never ends directly behind the last trees.
const SLICE_GROUND_SIZE: f32 = 120.0;
const SLICE_PATH: [Vec2; 8] = [
    Vec2::new(21.0, 60.0), Vec2::new(9.0, 24.0), Vec2::new(6.0, 15.0),
    Vec2::new(1.0, 6.0), Vec2::new(-2.0, -4.0), Vec2::new(-1.0, -14.0),
    Vec2::new(-7.0, -24.0), Vec2::new(-19.0, -60.0),
];

/// Shared with the Blender layout export: local coordinates are relative to (500, 500).
/// The gentle undulation gives the slice a natural silhouette without lifting prop bases.
pub fn slice_height(x: f32, z: f32) -> f32 {
    0.06 * (0.22 * x).sin() * (0.15 * z).cos()
        + 0.03 * (0.63 * x + 0.40 * z).sin()
}

fn slice_path_distance(point: Vec2) -> f32 {
    SLICE_PATH.windows(2).map(|pair| {
        let a = pair[0];
        let d = pair[1] - a;
        let t = ((point - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
        point.distance(a + d * t)
    }).fold(f32::INFINITY, f32::min)
}

fn slice_mask(images: &mut Assets<Image>) -> (Handle<Image>, Vec4) {
    const PX: u32 = 768; // 6.4 texels per world unit over the plate and its background fringe.
    let mut data = vec![0u8; (PX * PX * 4) as usize];
    for z in 0..PX {
        for x in 0..PX {
            let lx = -SLICE_GROUND_SIZE * 0.5
                + (x as f32 + 0.5) * SLICE_GROUND_SIZE / PX as f32;
            let lz = -SLICE_GROUND_SIZE * 0.5
                + (z as f32 + 0.5) * SLICE_GROUND_SIZE / PX as f32;
            // The authored centerline makes a gentle S. Vary its full width by
            // roughly ±0.35u and roughen the soil/grass boundary at a finer scale.
            let half_width_delta = 0.13 * (lz * 0.29).sin()
                + 0.045 * (lz * 1.31 + lx * 0.4).sin();
            let edge = 0.10 * (lz * 1.13 + lx * 0.29).sin()
                + 0.04 * (lz * 3.71 - lx * 1.17).sin();
            let d = slice_path_distance(Vec2::new(lx, lz)) + edge - half_width_delta;
            let t = ((d - 0.95) / 1.35).clamp(0.0, 1.0);
            let road = 1.0 - t * t * (3.0 - 2.0 * t);
            let i = ((z * PX + x) * 4) as usize;
            data[i] = (road * 255.0) as u8;
            data[i + 2] = 255; // Entire composed slice uses the forest habitat blend.
            data[i + 3] = 255;
        }
    }
    let mut image = Image::new(
        Extent3d { width: PX, height: PX, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    let min = SLICE_CENTER - Vec2::splat(SLICE_GROUND_SIZE * 0.5);
    (images.add(image), Vec4::new(min.x, min.y, 1.0 / SLICE_GROUND_SIZE, 1.0 / SLICE_GROUND_SIZE))
}

fn slice_atlases(images: &mut Assets<Image>) -> &'static GroundAtlases {
    SLICE_ATLASES.get_or_init(|| GroundAtlases {
        albedo: images.add(load_slice_atlas("albedo", TextureFormat::Rgba8UnormSrgb)),
        normal: images.add(load_slice_atlas("normal", TextureFormat::Rgba8Unorm)),
        roughness: images.add(load_slice_atlas("roughness", TextureFormat::Rgba8Unorm)),
    })
}

/// One composed 48u forest study at a remote origin. Registered by ForestSlicePlugin only;
/// it cannot replace the campaign ground or road topology in an ordinary game run.
pub fn spawn_forest_slice_ground(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<TerrainMaterial>>,
) {
    if std::env::var("FOREST_FORESTSLICE").as_deref() != Ok("1") { return; }
    let atlases = slice_atlases(&mut images);
    let (path_mask, path_region) = slice_mask(&mut images);
    let dummy = images.add(Image::new(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2,
        vec![255, 255, 255, 255], TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let rut = images.add(Image::new(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2,
        vec![0], TextureFormat::R8Unorm, RenderAssetUsages::default(),
    ));
    let material = mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            cull_mode: None,
            ..default()
        },
        extension: ForestExtension {
            params: ForestParams {
                params: Vec4::new(1.0, 0.0, 0.0, 1.0),
                params2: Vec4::new(1.0, 1.0, 0.0, 1.0),
                rut_region: Vec4::ZERO,
                surface: Vec4::new(1.0, 1.0 / TILE_WORLD, 0.0, 0.0),
                path_region,
            },
            detail: dummy,
            rut,
            surface_albedo: atlases.albedo.clone(),
            surface_normal: atlases.normal.clone(),
            surface_roughness: atlases.roughness.clone(),
            path_mask,
        },
    });
    const GRID: usize = 160; // 0.75u cells across a 120u backdrop (~51k tris).
    let width = GRID + 1;
    let mut positions = Vec::with_capacity(width * width);
    let mut normals = Vec::with_capacity(width * width);
    let mut uvs = Vec::with_capacity(width * width);
    let mut colors = Vec::with_capacity(width * width);
    let mut indices = Vec::with_capacity(GRID * GRID * 6);
    for iz in 0..=GRID {
        for ix in 0..=GRID {
            let x = -SLICE_GROUND_SIZE * 0.5 + ix as f32 * SLICE_GROUND_SIZE / GRID as f32;
            let z = -SLICE_GROUND_SIZE * 0.5 + iz as f32 * SLICE_GROUND_SIZE / GRID as f32;
            let dx = (slice_height(x + 0.05, z) - slice_height(x - 0.05, z)) * 10.0;
            let dz = (slice_height(x, z + 0.05) - slice_height(x, z - 0.05)) * 10.0;
            let n = Vec3::new(-dx, 1.0, -dz).normalize();
            positions.push([SLICE_CENTER.x + x, slice_height(x, z), SLICE_CENTER.y + z]);
            normals.push(n.to_array());
            uvs.push([ix as f32 / GRID as f32, iz as f32 / GRID as f32]);
            // Selector colour only: the Blender photos, not this paint, provide final RGB.
            colors.push([0.15, 0.31, 0.12, 0.0]);
        }
    }
    for iz in 0..GRID {
        for ix in 0..GRID {
            let a = (iz * width + ix) as u32;
            let b = a + 1;
            let c = a + width as u32;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::default()));
}

// The authored clumps in the ordinary biome scatter preserve their original counts and
// gameplay placement. At the walkable forest floor that leaves metre-wide gaps between
// bushes. Stream a separate, non-colliding meadow layer around the camera: one UV-bearing
// cutout mesh per 16u cell, rather than tens of thousands of permanent entities/blades.
const MEADOW_CELL: f32 = 16.0;
const MEADOW_NEAR_LOAD: f32 = 94.0;
const MEADOW_NEAR_KEEP: f32 = 108.0;
const MEADOW_FAR_LOAD: f32 = 140.0;
const MEADOW_FAR_KEEP: f32 = 156.0;
const MEADOW_NEAR_BUILDS_PER_FRAME: usize = 3;
const MEADOW_FAR_BUILDS_PER_FRAME: usize = 4;

#[derive(Component)]
struct MeadowInfillChunk;

#[derive(Default)]
struct MeadowStream {
    map_id: u8,
    // None means the cell has no eligible forest ground; avoid retrying it every frame.
    near: HashMap<(i32, i32), Option<(Entity, Handle<Mesh>)>>,
    far: HashMap<(i32, i32), Option<(Entity, Handle<Mesh>)>>,
}

fn meadow_rand(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as f32) / (u32::MAX as f32)
}

fn meadow_seed(cx: i32, cz: i32) -> u32 {
    let mut h = 0x8e37_79b9_u32 ^ (cx as u32).wrapping_mul(0x9e37_79b1);
    h ^= (cz as u32).wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    (h ^ (h >> 15)).max(1)
}

fn clear_meadow_stream(state: &mut MeadowStream, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
    for layer in [&mut state.near, &mut state.far] {
        for (_, resident) in layer.drain() {
            if let Some((entity, mesh)) = resident {
                commands.entity(entity).try_despawn();
                meshes.remove(mesh.id());
            }
        }
    }
}

fn evict_meadow_layer(
    layer: &mut HashMap<(i32, i32), Option<(Entity, Handle<Mesh>)>>,
    eye: Vec2,
    keep: f32,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    let expired: Vec<_> = layer.keys().copied()
        .filter(|key| meadow_center(*key).distance(eye) > keep)
        .collect();
    for key in expired {
        if let Some(Some((entity, mesh))) = layer.remove(&key) {
            commands.entity(entity).try_despawn();
            meshes.remove(mesh.id());
        }
    }
}

fn meadow_center(key: (i32, i32)) -> Vec2 {
    Vec2::new((key.0 as f32 + 0.5) * MEADOW_CELL,
              (key.1 as f32 + 0.5) * MEADOW_CELL)
}

fn stream_meadow_infill(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    ready: Option<Res<crate::biome::WorldReady>>,
    mut state: Local<MeadowStream>,
) {
    if std::env::var("FOREST_NOGRASS").is_ok()
        || std::env::var("FOREST_FORESTSLICE").as_deref() == Ok("1")
    {
        return;
    }
    let map_id = crate::worldmap::current_map_u8();
    // A capture rebuild can flip WorldReady false→true in ONE Update: the stream
    // never observes the false value, although the build wiped its BiomeEntity chunks.
    // The changed tick catches that completion and invalidates stale local handles.
    let ready_changed = ready.as_ref().is_some_and(|r| r.is_changed());
    if state.map_id != map_id || ready_changed || !ready.is_some_and(|r| r.0) || crate::worldmap::is_arena() {
        clear_meadow_stream(&mut state, &mut commands, &mut meshes);
        state.map_id = map_id;
        return;
    }
    let Some(camera) = cameras.iter().next() else { return; };
    let eye = camera.translation().xz();
    let base_x = (eye.x / MEADOW_CELL).floor() as i32;
    let base_z = (eye.y / MEADOW_CELL).floor() as i32;
    evict_meadow_layer(&mut state.near, eye, MEADOW_NEAR_KEEP, &mut commands, &mut meshes);
    evict_meadow_layer(&mut state.far, eye, MEADOW_FAR_KEEP, &mut commands, &mut meshes);
    let mut near_missing = Vec::new();
    let mut far_missing = Vec::new();
    for dz in -9..=9 {
        for dx in -9..=9 {
            let key = (base_x + dx, base_z + dz);
            let distance = meadow_center(key).distance(eye);
            if distance < MEADOW_NEAR_LOAD && !state.near.contains_key(&key) {
                near_missing.push((distance, key));
            }
            if distance > 50.0 && distance < MEADOW_FAR_LOAD && !state.far.contains_key(&key) {
                far_missing.push((distance, key));
            }
        }
    }
    near_missing.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Build the visible handoff ring first. Distant-LOD meshes near the camera
    // are fully culled, so filling them before the 70–100u horizon wastes warmup.
    far_missing.sort_by(|a, b| (a.0 - 84.0).abs().total_cmp(&(b.0 - 84.0).abs()));
    for (_, key) in near_missing.into_iter().take(MEADOW_NEAR_BUILDS_PER_FRAME) {
        let resident = build_meadow_chunk(key, false, &mut meshes, &mut commands);
        state.near.insert(key, resident);
    }
    for (_, key) in far_missing.into_iter().take(MEADOW_FAR_BUILDS_PER_FRAME) {
        let resident = build_meadow_chunk(key, true, &mut meshes, &mut commands);
        state.far.insert(key, resident);
    }
}

struct MeadowMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeadowMesh {
    fn new() -> Self {
        Self { positions: Vec::new(), normals: Vec::new(), uvs: Vec::new(), colors: Vec::new(), indices: Vec::new() }
    }

    fn append(&mut self, model: &Model, x: f32, y: f32, z: f32, scale: f32, yaw: f32, tint: f32) {
        let Some(VertexAttributeValues::Float32x3(pos)) = model.source.attribute(Mesh::ATTRIBUTE_POSITION) else { return; };
        let Some(VertexAttributeValues::Float32x3(nrm)) = model.source.attribute(Mesh::ATTRIBUTE_NORMAL) else { return; };
        let Some(VertexAttributeValues::Float32x2(uv)) = model.source.attribute(Mesh::ATTRIBUTE_UV_0) else { return; };
        let Some(VertexAttributeValues::Float32x4(col)) = model.source.attribute(Mesh::ATTRIBUTE_COLOR) else { return; };
        let Some(Indices::U32(indices)) = model.source.indices() else { return; };
        let first = self.positions.len() as u32;
        let (sin, cos) = yaw.sin_cos();
        for i in 0..pos.len() {
            let p = pos[i];
            let n = nrm[i];
            self.positions.push([x + (p[0] * cos - p[2] * sin) * scale,
                                 y + p[1] * scale,
                                 z + (p[0] * sin + p[2] * cos) * scale]);
            self.normals.push([n[0] * cos - n[2] * sin, n[1], n[0] * sin + n[2] * cos]);
            self.uvs.push(uv[i]);
            self.colors.push([col[i][0] * tint, col[i][1] * tint, col[i][2] * tint, col[i][3]]);
        }
        self.indices.extend(indices.iter().map(|idx| first + *idx));
    }

    fn finish(self) -> Option<Mesh> {
        if self.indices.is_empty() { return None; }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        Some(mesh)
    }
}

fn build_meadow_chunk(
    key: (i32, i32),
    far: bool,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) -> Option<(Entity, Handle<Mesh>)> {
    let env = crate::blenderenv::get()?;
    let ids = ["meadow_grass_a", "meadow_grass_b", "grass_a", "grass_c", "grass_d",
               "meadow_flowers_yellow", "meadow_flowers_white", "meadow_flowers_purple"];
    let models: Vec<&Model> = ids.iter().filter_map(|id| env.model(id)).collect();
    if models.len() != ids.len() { return None; }
    let mat = models[0].mat.clone();
    let mut mesh = MeadowMesh::new();
    let origin_x = key.0 as f32 * MEADOW_CELL;
    let origin_z = key.1 as f32 * MEADOW_CELL;
    let clearings = crate::poi::story_clearings();
    for iz in 0..16 {
        for ix in 0..16 {
            // Cell-local seeds make the far LOD an exact subset of the near geometry,
            // so the dither handoff does not swap one plant species for another.
            let mut seed = meadow_seed(key.0 * 16 + ix, key.1 * 16 + iz);
            let wx = origin_x + ix as f32 + 0.5 + (meadow_rand(&mut seed) - 0.5) * 0.52;
            let wz = origin_z + iz as f32 + 0.5 + (meadow_rand(&mut seed) - 0.5) * 0.52;
            if crate::worldmap::tile_biome_world(wx, wz) != Some(Biome::Forest)
                || crate::roads::near_road(wx, wz, 0.32)
                || crate::water::on_river(wx, wz)
                || crate::ruins::near_landmark_visual_footprint(wx, wz)
                || clearings.overlaps(wx, wz, 0.45)
                || crate::blockers::any_within(wx, wz, 0.28)
            { continue; }
            let Some(y) = crate::worldmap::ground_at_world(wx, wz) else { continue; };
            let Some(yx) = crate::worldmap::ground_at_world(wx + 0.45, wz) else { continue; };
            let Some(yz) = crate::worldmap::ground_at_world(wx, wz + 0.45) else { continue; };
            if (yx - y).abs() > 0.27 || (yz - y).abs() > 0.27 { continue; }
            let fertility = crate::biome::ground_patch(wx, wz).fertility;
            if meadow_rand(&mut seed) > 0.76 + 0.23 * fertility { continue; }
            let bloom = meadow_rand(&mut seed);
            let id = if bloom < 0.15 + 0.17 * fertility {
                         let petal = meadow_rand(&mut seed);
                         if petal < 0.55 { 5 } else if petal < 0.85 { 6 } else { 7 }
                     }
                     else if bloom < 0.33 { 2 + (meadow_rand(&mut seed) * 3.0) as usize }
                     else if meadow_rand(&mut seed) < 0.5 { 0 } else { 1 };
            let scale = if id == 0 || id == 1 { 1.12 + meadow_rand(&mut seed) * 0.30 }
                        else if id >= 5 { 1.00 + meadow_rand(&mut seed) * 0.30 }
                        else { 1.22 + meadow_rand(&mut seed) * 0.34 };
            let angle = meadow_rand(&mut seed) * std::f32::consts::TAU;
            let tint = 0.91 + meadow_rand(&mut seed) * 0.15;
            if far && meadow_rand(&mut seed) > 0.31 { continue; }
            mesh.append(models[id], wx - origin_x - MEADOW_CELL * 0.5, y + 0.012,
                        wz - origin_z - MEADOW_CELL * 0.5, scale, angle, tint);
            // A small, low tuft in a minority of gaps creates a grassy underlayer without
            // turning the scene into a repeated patch-grid or doubling every cell's triangles.
            if !far && id >= 5 && meadow_rand(&mut seed) < 0.35 {
                let ox = (meadow_rand(&mut seed) - 0.5) * 0.52;
                let oz = (meadow_rand(&mut seed) - 0.5) * 0.52;
                mesh.append(models[2 + (meadow_rand(&mut seed) * 3.0) as usize],
                            wx + ox - origin_x - MEADOW_CELL * 0.5, y + 0.012,
                            wz + oz - origin_z - MEADOW_CELL * 0.5, 1.05, angle + 0.8, tint);
            }
        }
    }
    let mesh = meshes.add(mesh.finish()?);
    let entity = commands.spawn((
        Mesh3d(mesh.clone()), MeshMaterial3d(mat),
        Transform::from_xyz(origin_x + MEADOW_CELL * 0.5, 0.0, origin_z + MEADOW_CELL * 0.5),
        bevy::light::NotShadowCaster,
        bevy::camera::visibility::VisibilityRange {
            start_margin: if far { 70.0..86.0 } else { 0.0..0.0 },
            end_margin: if far { 116.0..138.0 } else { 70.0..86.0 },
            // Both LOD meshes use the same chunk origin; AABB centres differ with density.
            use_aabb: false,
        },
        MeadowInfillChunk,
        crate::biome::GroundCoverChunk,
        crate::biome::BiomeEntity,
    )).id();
    Some((entity, mesh))
}

/// Cobble arrives with the built walls. New terrain materials also receive the current value
/// after a save/map rebuild, even when `Defenses` itself did not change that frame.
fn sync_courtyard_paving(
    defenses: Option<Res<crate::economy::Defenses>>,
    mut mats: ResMut<Assets<TerrainMaterial>>,
) {
    let paved = defenses.as_ref().is_some_and(|d| d.walls);
    let want = if paved { 1.0 } else { 0.0 };
    let stale: Vec<_> = mats
        .iter()
        .filter(|(_, m)| m.extension.params.surface.x > 0.5 && m.extension.params.surface.z != want)
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(mut mat) = mats.get_mut(id) {
            mat.extension.params.surface.z = want;
        }
    }
}

/// Uploaded once and retained across in-process map rebuilds. The images keep their CPU copies:
/// a newly minted terrain material must be able to bind the same handles after a map switch.
pub fn atlases(images: &mut Assets<Image>) -> Option<&'static GroundAtlases> {
    if !enabled() {
        return None;
    }
    Some(ATLASES.get_or_init(|| GroundAtlases {
        albedo: images.add(load_atlas("albedo", TextureFormat::Rgba8UnormSrgb)),
        normal: images.add(load_atlas("normal", TextureFormat::Rgba8Unorm)),
        roughness: images.add(load_atlas("roughness", TextureFormat::Rgba8Unorm)),
    }))
}

/// Fragment-resolution approach road and castle-yard masks. The legacy road field still owns
/// geometry, speed, and avoidance; this only gives the authored packed-earth/cobble bakes the
/// same placement. R = road strength, G = castle yard strength, B = forest habitat blend.
pub fn path_mask(images: &mut Assets<Image>) -> Option<(Handle<Image>, Vec4)> {
    if !enabled() || crate::worldmap::is_arena() {
        return None;
    }
    Some(
        PATH_MASK
            .get_or_init(|| {
                const STEP: f32 = 0.5;
                let w = (crate::worldmap::COLS as f32 / STEP).ceil() as u32;
                let h = (crate::worldmap::ROWS as f32 / STEP).ceil() as u32;
                let min_x = -crate::worldmap::GX;
                let min_z = -crate::worldmap::GZ;
                let mut data = vec![0u8; (w * h * 4) as usize];
                for z in 0..h {
                    for x in 0..w {
                        let wx = min_x + (x as f32 + 0.5) * STEP;
                        let wz = min_z + (z as f32 + 0.5) * STEP;
                        let i = ((z * w + x) * 4) as usize;
                        data[i] =
                            (crate::roads::road_strength(wx, wz).clamp(0.0, 1.0) * 255.0) as u8;
                        data[i + 1] =
                            (crate::castle::yard_strength(wx, wz).clamp(0.0, 1.0) * 255.0) as u8;
                        data[i + 2] = if crate::worldmap::tile_biome_world(wx, wz)
                            == Some(Biome::Forest)
                        {
                            255
                        } else {
                            0
                        };
                        data[i + 3] = 255;
                    }
                }
                // Feather only the habitat channel over ~2 world units. The road/yard
                // masks retain their sharp gameplay-authored widths. A separable box
                // pass is cheap during the one-off map bake and avoids a tile-grid seam
                // where meadow turns into forest floor.
                let mut horizontal = vec![0u8; (w * h) as usize];
                for z in 0..h {
                    for x in 0..w {
                        let mut sum = 0u32;
                        let mut count = 0u32;
                        for dx in -4i32..=4 {
                            let sx = x as i32 + dx;
                            if sx >= 0 && sx < w as i32 {
                                sum += data[((z * w + sx as u32) * 4 + 2) as usize] as u32;
                                count += 1;
                            }
                        }
                        horizontal[(z * w + x) as usize] = (sum / count) as u8;
                    }
                }
                for z in 0..h {
                    for x in 0..w {
                        let mut sum = 0u32;
                        let mut count = 0u32;
                        for dz in -4i32..=4 {
                            let sz = z as i32 + dz;
                            if sz >= 0 && sz < h as i32 {
                                sum += horizontal[(sz as u32 * w + x) as usize] as u32;
                                count += 1;
                            }
                        }
                        data[((z * w + x) * 4 + 2) as usize] = (sum / count) as u8;
                    }
                }
                let mut img = Image::new(
                    Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    data,
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                );
                img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    mag_filter: ImageFilterMode::Linear,
                    min_filter: ImageFilterMode::Linear,
                    ..default()
                });
                (
                    images.add(img),
                    Vec4::new(
                        min_x,
                        min_z,
                        1.0 / (w as f32 * STEP),
                        1.0 / (h as f32 * STEP),
                    ),
                )
            })
            .clone(),
    )
}

/// Replaces the visible mesh only, after the biome scatterer has selected a class/variant,
/// spacing, blocker and transform. The source placement and RNG sequence stay unchanged.
/// `vi` includes the scatterer's three subtle tint copies, so divide by three to recover the
/// authored class variant before choosing among the Blender silhouettes.
pub fn scatter_model(
    cfg: &BiomeConfig,
    cover: bool,
    class: usize,
    vi: usize,
) -> Option<&'static Model> {
    if !enabled() || crate::worldmap::is_arena() {
        return None;
    }
    let variant = vi / 3;
    let pick = |names: &'static [&'static str]| names[variant % names.len()];
    let name = if cfg.name == "Grass" {
        if !cover {
            return None;
        }
        match class {
            0 => pick(&["grass_a", "grass_b", "grass_c", "grass_d"]),
            1 => pick(&["clover_a", "clover_b", "clover_c"]),
            2 => pick(&["fern_a", "fern_b", "fern_c"]),
            3 => pick(&["flower_a", "flower_b", "flower_c", "flower_d", "flower_e", "flower_f", "flower_g", "flower_h", "flower_i"]),
            4 => pick(&["mushroom_a", "mushroom_b"]),
            _ => return None,
        }
    } else {
        match (cfg.biome, cover, class) {
            (Biome::Forest, false, 2) => ["shrub_a", "shrub_b", "shrub_c"][(variant / 3) % 3],
            (Biome::Forest, false, 3) => ["rock_a", "rock_b", "rock_c"][(variant / 3) % 3],
            (Biome::Forest, false, 4) => "stump_a",
            (Biome::Forest, true, 0) => pick(&["grass_a", "grass_b", "grass_c", "grass_d"]),
            (Biome::Forest, true, 1) => pick(&["clover_a", "clover_b", "clover_c"]),
            (Biome::Forest, true, 2) => pick(&["mushroom_a", "mushroom_b", "mushroom_c", "mushroom_d"]),
            (Biome::Forest, true, 3) => pick(&["flower_a", "flower_b", "flower_c", "flower_d", "flower_e", "flower_f", "flower_g", "flower_h", "flower_i"]),
            (Biome::Forest, true, 4) => pick(&["fern_a", "fern_b", "fern_c"]),
            (Biome::Forest, true, 5) => pick(&["litter_a", "litter_b", "litter_c", "litter_d", "litter_e"]),
            (Biome::Rocky, false, 0) => "dry_shrub_a",
            (Biome::Rocky, false, 1) => ["boulder_a", "boulder_b", "boulder_c", "rock_cairn_a"][variant % 4],
            (Biome::Rocky, false, 3) => pick(&["rock_scree_a", "rock_scree_b"]),
            (Biome::Rocky, true, 0) => pick(&["rock_a", "rock_b", "rock_c"]),
            (Biome::Rocky, true, 1) => "rock_dry_tuft_a",
            (Biome::Rocky, true, 2) => pick(&["rock_litter_a", "rock_litter_b", "rock_litter_c"]),
            (Biome::Snow, false, 1) if variant == 0 => "snowdrift_a",
            (Biome::Snow, false, 1) if variant == 1 => "snow_shrub_a",
            (Biome::Snow, false, 1) if variant == 2 => "snow_stump_log_a",
            (Biome::Snow, false, 2) => match variant {
                0 => "snow_boulder_a",
                1 => "snow_tor_a",
                _ => if vi % 3 == 1 { "snow_boulder_huddle_b" } else { "snow_boulder_huddle_a" },
            },
            (Biome::Snow, true, 0) => "snow_grass_a",
            (Biome::Snow, true, 1) => "ice_glint_a",
            (Biome::Snow, true, 2) => pick(&["winter_litter_a", "winter_litter_b", "winter_litter_c", "winter_litter_d"]),
            (Biome::Desert, false, 0) => "dry_shrub_a",
            (Biome::Desert, false, 2) => pick(&["barrel_cactus_a", "barrel_cactus_b"]),
            (Biome::Desert, false, 3) => pick(&["prickly_pear_a", "prickly_pear_b"]),
            (Biome::Desert, false, 4) if variant == 2 => "desert_skull_a",
            (Biome::Desert, false, 4) => ["desert_bleached_rock_a", "desert_bleached_rock_b"][variant % 2],
            (Biome::Desert, true, 0) => pick(&["desert_bleached_rock_a", "desert_bleached_rock_b"]),
            (Biome::Desert, true, 1) => "desert_grass_a",
            (Biome::Desert, true, 2) => "desert_succulent_a",
            (Biome::Desert, true, 3) => pick(&["desert_litter_a", "desert_litter_b", "desert_litter_c"]),
            (Biome::Swamp, false, 1) => "reed_a",
            (Biome::Swamp, false, 2) => match variant {
                0 => "mushroom_a",
                1 => "rock_a",
                2 => "mushroom_b",
                _ => "swamp_log_a",
            },
            (Biome::Swamp, true, 0) => "moss_patch_a",
            (Biome::Swamp, true, 1) => "reed_a",
            (Biome::Swamp, true, 2) => "mushroom_a",
            (Biome::Swamp, true, 3) => pick(&["flower_a", "flower_b"]),
            _ => return None,
        }
    };
    crate::blenderenv::get()?.model(name)
}

pub fn tree_model(biome: Biome, class: usize, vi: usize) -> Option<&'static Model> {
    if !enabled() || crate::worldmap::is_arena() {
        return None;
    }
    let variant = vi / 3;
    let name = match (biome, class) {
        // Forest class 0 has five living kinds × five tints, followed by one Dead
        // source variant (each expanded into three PROP_TINTS during upload).
        // The tree-study loader intentionally leaves Dead native; world mode covers it.
        (Biome::Forest, 0) if variant == 25 => "dead_tree_a",
        (Biome::Snow, 0) => match variant {
            0 => "snow_pine_a",
            1 => "snow_pine_b",
            2 => "snow_pine_c",
            3 => "snow_birch_a",
            _ => return None,
        },
        (Biome::Desert, 1) => ["cactus_a", "cactus_b", "cactus_c"][variant % 3],
        (Biome::Swamp, 0) => match variant {
            0 => "swamp_tree_a",
            1 => "swamp_tree_b",
            2 => "swamp_tree_c",
            3 => "swamp_cypress_stump_a",
            4 => "swamp_cypress_stump_b",
            _ => return None,
        },
        (Biome::Rocky, 2) => match variant {
            0 | 1 => "rock_spire_a",
            2 => "rock_windpine_a",
            3 => "rock_windpine_b",
            4 => "dead_tree_a",
            _ => return None,
        },
        _ => return None,
    };
    crate::blenderenv::get()?.model(name)
}

/// Keep the old class's visual footprint even if an authored asset was made at a different
/// studio scale. The blocker itself remains derived from the original class as before.
pub fn fit_radius(model: &Model, old_radius: f32) -> f32 {
    // Use the furthest actual vertex, not the diagonal of an axis-aligned box.
    // Card/branch assets rarely reach both box extrema together; the box diagonal
    // made their visual radius look much larger on paper and shrank every clump.
    let new_radius = model.radius_xz.max(0.05);
    (old_radius / new_radius).clamp(0.28, 3.5)
}

/// Recover subtle per-instance hue variety from the old three-tint scatter without
/// multiplying materials or draw calls. The tint is baked into the mesh before its
/// chunk is merged; all instances still sample the same Blender atlas.
pub fn tinted_source(model: &Model, cfg: &BiomeConfig, cover: bool, class: usize, vi: usize) -> Mesh {
    let subtle = match vi % 3 {
        0 => [1.0, 1.0, 1.0],
        1 => [1.045, 1.025, 0.96],
        _ => [0.93, 1.025, 1.035],
    };
    let base = if cfg.biome == Biome::Forest && !cover && class == 2 {
        // The native bush class had dark, middle and bright foliage families,
        // each with three colour variants. Preserve that family distinction.
        match (vi / 3) / 3 {
            0 => [0.82, 0.94, 0.82],
            1 => [1.0, 1.0, 1.0],
            _ => [1.09, 1.07, 0.98],
        }
    } else {
        [1.0, 1.0, 1.0]
    };
    crate::trees::tint_mesh(
        model.source.clone(),
        [base[0] * subtle[0], base[1] * subtle[1], base[2] * subtle[2]],
    )
}

fn resolve(rel: &str) -> PathBuf {
    let mut roots = Vec::new();
    if let Ok(root) = std::env::var("BEVY_ASSET_ROOT") {
        roots.push(PathBuf::from(root));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    roots.push(PathBuf::new());
    roots
        .into_iter()
        .map(|root| root.join(rel))
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(rel))
}

fn read_tile(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path)
        .unwrap_or_else(|e| panic!("Blender ground texture {}: {e}", path.display()));
    let rgba = image::load_from_memory(&raw)
        .unwrap_or_else(|e| panic!("Blender ground decode {}: {e}", path.display()))
        .into_rgba8();
    assert_eq!(
        rgba.dimensions(),
        (TILE_PX, TILE_PX),
        "Blender ground {} must be {TILE_PX}x{TILE_PX}",
        path.display()
    );
    rgba.into_raw()
}

fn halve_tile(src: &[u8], width: u32) -> Vec<u8> {
    let nw = width / 2;
    let mut dst = vec![0; (nw * nw * 4) as usize];
    for y in 0..nw {
        for x in 0..nw {
            let d = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                let mut sum = 0u32;
                for dy in 0..2 {
                    for dx in 0..2 {
                        sum += src[(((y * 2 + dy) * width + x * 2 + dx) * 4) as usize + c] as u32;
                    }
                }
                dst[d + c] = (sum / 4) as u8;
            }
        }
    }
    dst
}

fn load_atlas(channel: &str, format: TextureFormat) -> Image {
    load_atlas_impl(channel, format, false)
}

fn load_slice_atlas(channel: &str, format: TextureFormat) -> Image {
    load_atlas_impl(channel, format, true)
}

fn load_atlas_impl(channel: &str, format: TextureFormat, slice_path: bool) -> Image {
    let mut tiles: Vec<Vec<u8>> = SURFACES
        .iter()
        .map(|name| {
            let rel = if slice_path && *name == "path" {
                format!("assets/models/forest_slice/ground_path_{channel}.png")
            } else {
                format!("{DIR}/ground_{name}_{channel}.png")
            };
            let path = resolve(&rel);
            read_tile(&path)
        })
        .collect();
    let mut data = Vec::new();
    let mut size = TILE_PX;
    let mut mips = 0;
    loop {
        let (w, h) = (size * COLS, size * ROWS);
        let mut atlas = vec![0u8; (w * h * 4) as usize];
        for (i, tile) in tiles.iter().enumerate() {
            let ox = i as u32 % COLS * size;
            let oy = i as u32 / COLS * size;
            for y in 0..size {
                let src = (y * size * 4) as usize;
                let dst = (((oy + y) * w + ox) * 4) as usize;
                atlas[dst..dst + (size * 4) as usize]
                    .copy_from_slice(&tile[src..src + (size * 4) as usize]);
            }
        }
        data.extend_from_slice(&atlas);
        mips += 1;
        if size == 4 {
            break;
        } // keep each atlas cell distinct in the deepest mip
        tiles = tiles.iter().map(|tile| halve_tile(tile, size)).collect();
        size /= 2;
    }
    let mut img = Image::new_uninit(
        Extent3d {
            width: TILE_PX * COLS,
            height: TILE_PX * ROWS,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::default(),
    );
    img.texture_descriptor.mip_level_count = mips;
    img.data = Some(data);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    img
}
