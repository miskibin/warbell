//! Opt-in Blender-authored trees for the first forest art/performance study.
//!
//! `FOREST_BLENDERTREES=1` loads five Y-up single-mesh exports and one shared RGBA atlas
//! synchronously at PreStartup. A tree stays one entity, one mesh and one material; placement,
//! blockers, chopping, wind and the other biomes remain owned by their existing systems.
//! JSON is the runtime mesh source because Bevy's glTF scene loading is asynchronous and would
//! add a child-entity tree to every scatter site. The matching GLBs are inspectable source assets.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat};
use serde::Deserialize;

use crate::trees::TreeKind;

const DIR: &str = "assets/models/blender_trees";
const NAMES: [&str; 5] = ["oak_a", "oak_b", "birch_a", "birch_b", "pine_a"];
const TINTS: [[f32; 3]; 5] = crate::trees::TREE_TINTS;
// Linear RGB, multiplied by the nearly neutral oak leaf texels. Bark is left intact.
const AUTUMN: [[f32; 3]; 5] = [
    [0.95, 0.43, 0.07],
    [0.80, 0.27, 0.035],
    [1.00, 0.62, 0.13],
    [0.82, 0.39, 0.055],
    [0.90, 0.51, 0.085],
];

#[derive(Deserialize)]
struct ExportMesh {
    schema: String,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    triangles: usize,
}

impl ExportMesh {
    fn validate(&self, name: &str) {
        let n = self.positions.len();
        assert_eq!(
            self.schema, "warbell.blender_tree_mesh.v1",
            "{name}: schema"
        );
        assert!(
            n > 0 && self.normals.len() == n && self.uvs.len() == n && self.colors.len() == n,
            "{name}: vertex attributes must have equal nonzero length"
        );
        assert_eq!(
            self.indices.len(),
            self.triangles * 3,
            "{name}: triangle count"
        );
        assert!(
            self.indices.iter().all(|&i| (i as usize) < n),
            "{name}: index out of range"
        );
        assert!(
            self.positions.iter().flatten().all(|v| v.is_finite()),
            "{name}: nonfinite position"
        );
    }

    fn mesh(&self, tint: [f32; 3], autumn: bool) -> Mesh {
        let mut colors = self.colors.clone();
        for (color, uv) in colors.iter_mut().zip(&self.uvs) {
            // Only foliage is recoloured. The bottom-right atlas quadrant is opaque bark.
            let leaf = !(uv[0] >= 0.5 && uv[1] >= 0.5);
            if leaf {
                if autumn {
                    let shade = (color[0] + color[1] + color[2]) / 3.0;
                    for i in 0..3 {
                        color[i] = shade * tint[i];
                    }
                } else {
                    for i in 0..3 {
                        color[i] *= tint[i];
                    }
                }
            }
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(self.indices.clone()));
        mesh
    }
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
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(rel))
}

fn atlas(path: &Path) -> Image {
    let raw = std::fs::read(path)
        .unwrap_or_else(|e| panic!("blender trees atlas {}: {e}", path.display()));
    let rgba = image::load_from_memory(&raw)
        .unwrap_or_else(|e| panic!("blender trees atlas decode {}: {e}", path.display()))
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    assert_eq!(width, 1024, "blender tree atlas width");
    assert_eq!(height, 1024, "blender tree atlas height");
    // Alpha-weighted sRGB box mips keep transparent black out of card edges. The alpha scale is
    // a near/deep-mip heuristic, not exact coverage preservation or linear-light filtering.
    // Stop at 4×4 so each atlas quadrant still has its own texels.
    let mut level = rgba.into_raw();
    let mut data = level.clone();
    let (mut w, mut h, mut mips) = (width, height, 1u32);
    while w > 4 || h > 4 {
        let nw = (w / 2).max(1);
        let nh = (h / 2).max(1);
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let (mut rgb, mut alpha) = ([0u32; 3], 0u32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (2 * x + dx).min(w - 1);
                    let sy = (2 * y + dy).min(h - 1);
                    let i = ((sy * w + sx) * 4) as usize;
                    let a = level[i + 3] as u32;
                    alpha += a;
                    for c in 0..3 {
                        rgb[c] += level[i + c] as u32 * a;
                    }
                }
                let o = ((y * nw + x) * 4) as usize;
                if alpha > 0 {
                    for c in 0..3 {
                        next[o + c] = (rgb[c] / alpha) as u8;
                    }
                }
                let alpha_scale = if mips <= 3 { 1.3 } else { 0.8 };
                // Bark owns the bottom-right quadrant and is fully opaque in the source.
                // Fading its alpha through deep mips would erase distant trunks under Mask.
                next[o + 3] = if x >= nw / 2 && y >= nh / 2 {
                    255
                } else {
                    ((alpha as f32 / 4.0 * alpha_scale).round() as u32).min(255) as u8
                };
            }
        }
        data.extend_from_slice(&next);
        level = next;
        w = nw;
        h = nh;
        mips += 1;
    }
    let mut img = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
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

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("FOREST_BLENDERTREES").as_deref() == Ok("1")
            || std::env::var("FOREST_BLENDERWORLD").as_deref() == Ok("1")
    })
}

static ASSETS: OnceLock<BlenderTrees> = OnceLock::new();

/// Only the tree/world experiments replace the existing campaign forest. The isolated
/// slice can load these same meshes without changing campaign tree call sites.
pub fn get() -> Option<&'static BlenderTrees> {
    enabled().then(|| ASSETS.get()).flatten()
}

pub fn get_for_slice() -> Option<&'static BlenderTrees> {
    ASSETS.get()
}

#[derive(Clone, Copy)]
pub enum Source {
    Forest,
    Meadow,
    Rts,
    Treeline,
}

pub struct TreeVisual {
    pub mesh: Handle<Mesh>,
    pub mat: Handle<StandardMaterial>,
    /// Poplar uses the narrower birch crown while keeping its tall, slim silhouette.
    pub shape: Vec3,
}

pub struct BlenderTrees {
    // Indexed model (oak A/B, birch A/B, pine A), then original foliage tint variant.
    green: Vec<Vec<Handle<Mesh>>>,
    autumn: Vec<Vec<Handle<Mesh>>>,
    mat: Handle<StandardMaterial>,
    /// Green-grade atlas used only by the isolated forest composition. The original
    /// tree-study/campaign material and frozen atlas remain byte-for-byte unchanged.
    slice_mat: Option<Handle<StandardMaterial>>,
    forest: AtomicUsize,
    meadow: AtomicUsize,
    rts: AtomicUsize,
    treeline: AtomicUsize,
}

impl BlenderTrees {
    /// Exact authored variant for the isolated forest study; no tint hash or gameplay
    /// counter is involved, so the layout JSON names map one-to-one onto Blender meshes.
    pub fn model(&self, name: &str) -> Option<TreeVisual> {
        let index = NAMES.iter().position(|candidate| *candidate == name)?;
        Some(TreeVisual {
            mesh: self.green[index][0].clone(),
            mat: self.slice_mat.as_ref().unwrap_or(&self.mat).clone(),
            shape: Vec3::ONE,
        })
    }

    pub fn pick(
        &self,
        kind: TreeKind,
        x: f32,
        z: f32,
        tint: usize,
        source: Source,
    ) -> Option<TreeVisual> {
        let hash = x.to_bits().wrapping_mul(0x9e37_79b1)
            ^ z.to_bits().rotate_left(13).wrapping_mul(0x85eb_ca6b);
        let variant = (hash as usize) & 1;
        let (mesh, shape) = match kind {
            TreeKind::Broadleaf => (&self.green[variant][tint % 5], Vec3::ONE),
            TreeKind::Birch => (&self.green[2 + variant][tint % 5], Vec3::ONE),
            TreeKind::Pine => (&self.green[4][tint % 5], Vec3::ONE),
            TreeKind::Poplar => (
                &self.green[2 + variant][tint % 5],
                Vec3::new(0.72, 1.23, 0.72),
            ),
            TreeKind::Autumn => (&self.autumn[variant][tint % 5], Vec3::ONE),
            TreeKind::Dead | TreeKind::Stump => return None,
        };
        match source {
            Source::Forest => &self.forest,
            Source::Meadow => &self.meadow,
            Source::Rts => &self.rts,
            Source::Treeline => &self.treeline,
        }
        .fetch_add(1, Ordering::Relaxed);
        Some(TreeVisual {
            mesh: mesh.clone(),
            mat: self.mat.clone(),
            shape,
        })
    }

    /// Forest config class 0 is five living kinds × five tints, then Dead; upload_classes
    /// expands each source mesh into three PROP_TINTS. Class 1 is old-growth Broadleaf.
    pub fn pick_forest(&self, class: usize, vi: usize, x: f32, z: f32) -> Option<TreeVisual> {
        let kind = match class {
            0 => match vi / 15 {
                0 => TreeKind::Broadleaf,
                1 => TreeKind::Birch,
                2 => TreeKind::Pine,
                3 => TreeKind::Poplar,
                4 => TreeKind::Autumn,
                _ => return None,
            },
            1 => TreeKind::Broadleaf,
            _ => return None,
        };
        self.pick(kind, x, z, (vi / 3) % 5, Source::Forest)
    }
}

pub struct BlenderTreesPlugin;

impl Plugin for BlenderTreesPlugin {
    fn build(&self, app: &mut App) {
        if enabled() || crate::forest_slice::enabled() {
            app.add_systems(PreStartup, build_assets);
            app.add_systems(Update, log_counts);
        }
    }
}

fn build_assets(
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let t0 = std::time::Instant::now();
    let texture = images.add(atlas(&resolve(&format!("{DIR}/tree_atlas.png"))));
    let material = StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(texture),
        alpha_mode: AlphaMode::Mask(0.35),
        perceptual_roughness: 0.9,
        reflectance: 0.12,
        metallic: 0.0,
        diffuse_transmission: 0.25,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let slice_mat = crate::forest_slice::enabled().then(|| {
        let path = resolve("assets/models/forest_slice/tree_atlas_forest.png");
        let texture = images.add(atlas(&path));
        mats.add(StandardMaterial {
            base_color_texture: Some(texture),
            ..material.clone()
        })
    });
    let mat = mats.add(material);
    let mut green = Vec::new();
    let mut autumn = Vec::new();
    let (mut triangles, mut vertices) = (0usize, 0usize);
    for (model_idx, name) in NAMES.into_iter().enumerate() {
        let path = resolve(&format!("{DIR}/{name}.json"));
        let json = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("blender tree {}: {e}", path.display()));
        let export: ExportMesh = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("blender tree parse {}: {e}", path.display()));
        export.validate(name);
        triangles += export.triangles;
        vertices += export.positions.len();
        green.push(
            TINTS
                .into_iter()
                .map(|tint| meshes.add(export.mesh(tint, false)))
                .collect(),
        );
        if model_idx < 2 {
            autumn.push(
                AUTUMN
                    .into_iter()
                    .map(|tint| meshes.add(export.mesh(tint, true)))
                    .collect(),
            );
        }
        info!(
            "blender trees: {name} {} vertices {} triangles",
            export.positions.len(),
            export.triangles
        );
    }
    info!(
        "blender trees: 5 source models, {vertices} source vertices, {triangles} source triangles, one 1024px atlas, built in {:?}",
        t0.elapsed()
    );
    ASSETS
        .set(BlenderTrees {
            green,
            autumn,
            mat,
            slice_mat,
            forest: AtomicUsize::new(0),
            meadow: AtomicUsize::new(0),
            rts: AtomicUsize::new(0),
            treeline: AtomicUsize::new(0),
        })
        .unwrap_or_else(|_| panic!("blender tree assets initialized twice"));
}

fn log_counts(ready: Option<Res<crate::biome::WorldReady>>, mut frames: Local<u8>) {
    if *frames >= 3 || !ready.is_some_and(|r| r.0) {
        return;
    }
    *frames += 1;
    if *frames == 3 {
        if let Some(a) = get() {
            info!(
                "blender trees replacements: forest={}, meadow={}, rts={}, treeline={}; dead/stump, orchard and nonforest biome trees retain native meshes",
                a.forest.load(Ordering::Relaxed),
                a.meadow.load(Ordering::Relaxed),
                a.rts.load(Ordering::Relaxed),
                a.treeline.load(Ordering::Relaxed)
            );
        }
    }
}
