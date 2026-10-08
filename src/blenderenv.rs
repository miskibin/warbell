//! Shared, opt-in Blender environment assets (`FOREST_BLENDERWORLD=1`).
//!
//! Every exported object is one Y-up indexed mesh using either the opaque surface atlas or
//! the cutout vegetation atlas. JSON mirrors the editable GLB's final attributes and lets the
//! phased world builder use shared handles synchronously; static props can also clone `source`
//! into existing chunk meshes. Gameplay roots, visibility, blockers and animations stay with
//! their original owners.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

const DIR: &str = "assets/models/blender_environment";
const ATLAS_CELLS: u32 = 4;
const CUTOUT_MASK_CUTOFF: f32 = 0.35;
const CUTOUT_ALPHA_THRESHOLD: u8 = 90; // ceil(CUTOUT_MASK_CUTOFF * 255)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaterialKind { Opaque, Cutout }

pub struct Model {
    pub name: String,
    pub mesh: Handle<Mesh>,
    pub mat: Handle<StandardMaterial>,
    /// Kept in CPU memory for biome chunk merging; all attributes include UV_0 and COLOR.
    pub source: Mesh,
    pub material_kind: MaterialKind,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    /// Largest true radial XZ extent, measured from vertices rather than an AABB corner.
    pub radius_xz: f32,
}

pub struct BlenderEnv {
    models: HashMap<String, Model>,
}

impl BlenderEnv {
    pub fn model(&self, name: &str) -> Option<&Model> { self.models.get(name) }
    pub fn len(&self) -> usize { self.models.len() }
}

static ASSETS: OnceLock<BlenderEnv> = OnceLock::new();

/// Campaign swaps stay exclusive to the whole-world experiment. The forest slice loads
/// the same assets for its isolated diorama without replacing remote campaign objects.
pub fn get() -> Option<&'static BlenderEnv> { enabled().then(|| ASSETS.get()).flatten() }

pub fn get_for_slice() -> Option<&'static BlenderEnv> { ASSETS.get() }

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_BLENDERWORLD").as_deref() == Ok("1"))
}

/// The forest slice shares the approved WORLD sky and grade without enabling any of the
/// campaign's model replacement call sites.
pub fn look_enabled() -> bool { enabled() || crate::forest_slice::enabled() }

#[derive(Deserialize)]
struct ExportBounds { min: [f32; 3], max: [f32; 3] }

#[derive(Deserialize)]
struct ExportMesh {
    schema: String,
    name: Option<String>,
    atlas: String,
    #[serde(default)]
    orm_atlas: Option<String>,
    #[serde(default)]
    emissive_atlas: Option<String>,
    material: MaterialKind,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    bounds: ExportBounds,
    triangles: usize,
}

impl ExportMesh {
    fn validate(&self, name: &str) {
        let n = self.positions.len();
        assert_eq!(self.schema, "warbell.blender_environment_mesh.v1", "{name}: schema");
        assert!(n > 0 && self.normals.len() == n && self.uvs.len() == n && self.colors.len() == n,
            "{name}: vertex attributes must have equal nonzero length");
        assert_eq!(self.indices.len(), 3 * self.triangles, "{name}: triangle count");
        assert!(self.indices.iter().all(|&i| (i as usize) < n), "{name}: bad index");
        assert!(self.positions.iter().flatten().all(|v| v.is_finite()), "{name}: nonfinite position");
        assert!(self.normals.iter().flatten().all(|v| v.is_finite()), "{name}: nonfinite normal");
        assert!(self.uvs.iter().flatten().all(|v| v.is_finite()), "{name}: nonfinite UV");
        assert!(self.colors.iter().flatten().all(|v| v.is_finite()), "{name}: nonfinite color");
        assert!(self.uvs.iter().flatten().all(|v| (0.0..=1.0).contains(v)), "{name}: UV outside atlas");
        assert!(self.bounds.min.iter().chain(self.bounds.max.iter()).all(|v| v.is_finite()),
            "{name}: nonfinite bounds");
        // Decks are authored with their walkable surface near local y=0; beams/stilts sit below.
        if !name.starts_with("bridge_") {
            assert!(self.bounds.min[1].abs() < 0.015, "{name}: base must be y=0");
        }
        let expected_atlas = match self.material {
            MaterialKind::Opaque => "surface_atlas.png",
            MaterialKind::Cutout => "vegetation_atlas.png",
        };
        assert_eq!(self.atlas, expected_atlas, "{name}: material/atlas mismatch");
        if let Some(path) = &self.orm_atlas {
            assert!(self.material == MaterialKind::Opaque && path == "surface_orm.png", "{name}: ORM atlas mismatch");
        }
        if let Some(path) = &self.emissive_atlas {
            assert!(self.material == MaterialKind::Opaque && path == "surface_emissive.png", "{name}: emissive atlas mismatch");
        }
    }

    fn mesh(&self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD);
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.clone());
        m.insert_indices(Indices::U32(self.indices.clone()));
        m
    }
}

fn resolve(rel: &str) -> PathBuf {
    let mut roots = Vec::new();
    if let Ok(root) = std::env::var("BEVY_ASSET_ROOT") { roots.push(PathBuf::from(root)); }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() { roots.push(dir.to_path_buf()); }
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    roots.push(PathBuf::new());
    roots.into_iter().map(|root| root.join(rel)).find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(rel))
}

fn cell_mask_coverage(level: &[u8], width: u32, height: u32, cell_x: u32, cell_y: u32, scale: f32) -> f32 {
    let cell_w = width / ATLAS_CELLS;
    let cell_h = height / ATLAS_CELLS;
    let mut kept = 0u32;
    for y in cell_y * cell_h..(cell_y + 1) * cell_h {
        for x in cell_x * cell_w..(cell_x + 1) * cell_w {
            let alpha = level[((y * width + x) * 4 + 3) as usize] as f32;
            if (alpha * scale).round().min(255.0) >= CUTOUT_ALPHA_THRESHOLD as f32 {
                kept += 1;
            }
        }
    }
    kept as f32 / (cell_w * cell_h) as f32
}

fn preserve_cell_mask_coverage(level: &mut [u8], width: u32, height: u32, target: &[f32; 16]) {
    let cell_w = width / ATLAS_CELLS;
    let cell_h = height / ATLAS_CELLS;
    for cell_y in 0..ATLAS_CELLS {
        for cell_x in 0..ATLAS_CELLS {
            let goal = target[(cell_y * ATLAS_CELLS + cell_x) as usize];
            // Alpha-mask coverage rises monotonically with scale. The final 8x8-texel
            // cells cannot reproduce every base ratio, so choose the closer side of
            // the threshold. A finite cap also limits amplification of faint fringes.
            let current = cell_mask_coverage(level, width, height, cell_x, cell_y, 1.0);
            if (current - goal).abs() <= 0.5 / (cell_w * cell_h) as f32 { continue; }
            let (mut low, mut high) = if current < goal { (1.0f32, 64.0f32) } else { (0.0f32, 1.0f32) };
            for _ in 0..16 {
                let middle = (low + high) * 0.5;
                if cell_mask_coverage(level, width, height, cell_x, cell_y, middle) < goal {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let low_error = (cell_mask_coverage(level, width, height, cell_x, cell_y, low) - goal).abs();
            let high_error = (cell_mask_coverage(level, width, height, cell_x, cell_y, high) - goal).abs();
            let scale = if low_error < high_error { low }
                else if high_error < low_error { high }
                else if (low - 1.0).abs() <= (high - 1.0).abs() { low } else { high };
            for y in cell_y * cell_h..(cell_y + 1) * cell_h {
                for x in cell_x * cell_w..(cell_x + 1) * cell_w {
                    let alpha = &mut level[((y * width + x) * 4 + 3) as usize];
                    *alpha = (*alpha as f32 * scale).round().min(255.0) as u8;
                }
            }
        }
    }
}

fn atlas(path: &Path, cutout: bool, srgb: bool) -> Image {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("Blender environment atlas {}: {e}", path.display()));
    let rgba = image::load_from_memory(&bytes)
        .unwrap_or_else(|e| panic!("Blender environment atlas decode {}: {e}", path.display()))
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    assert!(width.is_power_of_two() && height.is_power_of_two() && width >= 64 && height >= 64,
        "Blender environment atlas must have power-of-two dimensions >=64");
    let mut level = rgba.into_raw();
    let mut data = level.clone();
    let (mut w, mut h, mut mips) = (width, height, 1u32);
    let base_coverage = cutout.then(|| std::array::from_fn(|i| {
        cell_mask_coverage(&level, width, height, (i as u32) % ATLAS_CELLS, (i as u32) / ATLAS_CELLS, 1.0)
    }));
    // Stop at 32px so each 4x4 atlas cell retains an 8x8 footprint. RGB remains
    // alpha-weighted in sRGB; only cutout alpha is corrected for mask coverage.
    while w > 32 || h > 32 {
        let nw = (w / 2).max(1);
        let nh = (h / 2).max(1);
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let (mut rgb, mut alpha) = ([0u32; 3], 0u32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (2*x + dx).min(w-1);
                    let sy = (2*y + dy).min(h-1);
                    let i = ((sy*w + sx)*4) as usize;
                    let a = level[i+3] as u32;
                    alpha += a;
                    for c in 0..3 { rgb[c] += level[i+c] as u32 * a; }
                }
                let o = ((y*nw + x)*4) as usize;
                if alpha > 0 { for c in 0..3 { next[o+c] = (rgb[c]/alpha) as u8; } }
                next[o+3] = if cutout { (alpha as f32 / 4.0).round() as u8 } else { 255 };
            }
        }
        if let Some(target) = &base_coverage {
            let mut output = next.clone();
            preserve_cell_mask_coverage(&mut output, nw, nh, target);
            data.extend_from_slice(&output);
        } else {
            data.extend_from_slice(&next);
        }
        // Derive later levels from the unscaled alpha, avoiding cumulative fringe
        // amplification; colour weights use that same unmodified alpha.
        level = next;
        w = nw;
        h = nh;
        mips += 1;
    }
    let mut img = Image::new_uninit(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = mips;
    img.data = Some(data);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    img
}

pub struct BlenderEnvPlugin;

impl Plugin for BlenderEnvPlugin {
    fn build(&self, app: &mut App) {
        if enabled() || crate::forest_slice::enabled() { app.add_systems(PreStartup, build_assets); }
    }
}

fn build_assets(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let t0 = std::time::Instant::now();
    let surface = images.add(atlas(&resolve(&format!("{DIR}/surface_atlas.png")), false, true));
    let vegetation = images.add(atlas(&resolve(&format!("{DIR}/vegetation_atlas.png")), true, true));
    let orm_path = resolve(&format!("{DIR}/surface_orm.png"));
    let emissive_path = resolve(&format!("{DIR}/surface_emissive.png"));
    let orm = orm_path.exists().then(|| images.add(atlas(&orm_path, false, false)));
    let emissive = emissive_path.exists().then(|| images.add(atlas(&emissive_path, false, true)));
    let opaque_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE, base_color_texture: Some(surface),
        metallic_roughness_texture: orm.clone(),
        occlusion_texture: orm.clone(),
        emissive_texture: emissive.clone(),
        emissive: if emissive.is_some() { LinearRgba::rgb(0.35, 0.35, 0.35) } else { LinearRgba::BLACK },
        perceptual_roughness: if orm.is_some() { 1.0 } else { 0.88 },
        metallic: if orm.is_some() { 1.0 } else { 0.0 }, ..default()
    });
    let cutout_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE, base_color_texture: Some(vegetation),
        alpha_mode: AlphaMode::Mask(CUTOUT_MASK_CUTOFF), double_sided: true, cull_mode: None,
        perceptual_roughness: 0.9, metallic: 0.0, diffuse_transmission: 0.25,
        ..default()
    });
    let dir = resolve(DIR);
    let mut models = HashMap::new();
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Blender environment directory {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    if crate::forest_slice::enabled() && !enabled() {
        let used = crate::forest_slice::environment_model_names();
        entries.retain(|p| p.file_stem().is_some_and(|name| used.contains(&name.to_string_lossy().to_string())));
    }
    entries.sort();
    assert!(!entries.is_empty(), "FOREST_BLENDERWORLD=1 but no environment JSON assets found");
    let mut total_triangles = 0usize;
    for path in entries {
        let stem = path.file_stem().expect("JSON filename stem").to_string_lossy().to_string();
        let json = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Blender environment {}: {e}", path.display()));
        let export: ExportMesh = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("Blender environment parse {}: {e}", path.display()));
        export.validate(&stem);
        if export.orm_atlas.is_some() { assert!(orm.is_some(), "{stem}: missing surface_orm.png"); }
        if export.emissive_atlas.is_some() { assert!(emissive.is_some(), "{stem}: missing surface_emissive.png"); }
        if let Some(name) = &export.name { assert_eq!(name, &stem, "{}: name/file mismatch", path.display()); }
        total_triangles += export.triangles;
        let material_kind = export.material;
        let mat = match material_kind { MaterialKind::Opaque => opaque_mat.clone(), MaterialKind::Cutout => cutout_mat.clone() };
        let radius_xz = export.positions.iter().map(|p| p[0].hypot(p[2])).fold(0.0_f32, f32::max);
        let source = export.mesh();
        let mesh = meshes.add(source.clone());
        let model = Model {
            name: stem.clone(), mesh, mat, source, material_kind,
            bounds_min: Vec3::from_array(export.bounds.min),
            bounds_max: Vec3::from_array(export.bounds.max),
            radius_xz,
        };
        assert!(models.insert(stem, model).is_none(), "duplicate Blender environment model");
    }
    info!("Blender environment: {} models, {} source triangles, {} shared textures, built in {:?}",
        models.len(), total_triangles, 2 + orm.is_some() as usize + emissive.is_some() as usize, t0.elapsed());
    ASSETS.set(BlenderEnv { models }).unwrap_or_else(|_| panic!("Blender environment assets initialized twice"));
}
