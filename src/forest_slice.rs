//! Isolated, Blender-composed forest art slice (`FOREST_FORESTSLICE=1`).
//!
//! The editable Blender scene exports a small placement manifest. This module reuses the
//! game's real PBR meshes, materials, camera and post-processing, placing the composition
//! far from the campaign island so only the 48-unit forest is in the camera frustum.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::asset::{AssetId, RenderAssetUsages};
use bevy::gltf::{Gltf, GltfMaterial};
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::world_serialization::WorldAsset;
use serde::Deserialize;

const LAYOUT_PATH: &str = "assets/models/forest_slice/layout.json";
const SLICE_ASSET_DIR: &str = "assets/models/forest_slice";
const MASK_CUTOFF: f32 = 0.35;
const MASK_BYTE: f32 = 90.0;

#[derive(Deserialize)]
struct CameraPose {
    eye: [f32; 3],
    target: [f32; 3],
}

#[derive(Deserialize)]
struct Instance {
    model: String,
    position: [f32; 3],
    rotation_y: f32,
    scale: [f32; 3],
}

#[derive(Deserialize)]
struct Layout {
    schema: String,
    origin: [f32; 3],
    bounds: [f32; 2],
    camera: CameraPose,
    instances: Vec<Instance>,
}

#[derive(Deserialize)]
struct SliceExport {
    schema: String,
    #[serde(default)]
    name: Option<String>,
    material: crate::blenderenv::MaterialKind,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    triangles: usize,
}

#[derive(Resource)]
struct SliceModels {
    models: HashMap<String, (Handle<Mesh>, crate::blenderenv::MaterialKind)>,
    cutout_material: Handle<StandardMaterial>,
    opaque_material: Handle<StandardMaterial>,
}

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_FORESTSLICE").as_deref() == Ok("1"))
}

/// Built-in temporal AA smooths the remaining subpixel foliage and shadow
/// noise in the slice. Disable it for a matched sharpness comparison.
pub fn taa_enabled() -> bool {
    enabled() && std::env::var("FOREST_SLICE_TAA").as_deref() != Ok("0")
}

/// Diagnostic opt-out for screen-space contact shadows on the isolated slice.
/// The depth prepass remains active for DoF and other consumers.
pub fn contact_shadows_enabled() -> bool {
    !enabled() || std::env::var("FOREST_SLICE_CONTACT_SHADOWS").as_deref() != Ok("0")
}

pub fn warm_sun_enabled() -> bool {
    static WARM: OnceLock<bool> = OnceLock::new();
    *WARM.get_or_init(|| std::env::var("FOREST_SLICE_SUN_WARM").as_deref() == Ok("1"))
}

fn mips_enabled() -> bool {
    std::env::var("FOREST_SLICE_MIPS").as_deref() != Ok("0")
}

pub fn hdr_ibl_enabled() -> bool {
    enabled() && std::env::var("FOREST_SLICE_HDR_IBL").as_deref() != Ok("0")
}

pub fn look_knob(name: &str, fallback: f32, min: f32, max: f32) -> f32 {
    std::env::var(name).ok().and_then(|s| s.parse::<f32>().ok())
        .filter(|x| x.is_finite()).map(|x| x.clamp(min, max)).unwrap_or(fallback)
}

pub fn fog_range() -> (f32, f32) {
    let fallback = (60.0, 190.0);
    let Ok(value) = std::env::var("FOREST_SLICE_FOG") else { return fallback; };
    let parts: Vec<_> = value.split(',').map(|s| s.trim().parse::<f32>().ok()).collect();
    match parts.as_slice() {
        [Some(start), Some(end)] if start.is_finite() && end.is_finite()
            && *start >= 0.0 && *end > *start + 1.0 => (*start, *end),
        _ => fallback,
    }
}

fn layout() -> &'static Layout {
    static LAYOUT: OnceLock<Layout> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        let root = std::env::var("BEVY_ASSET_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let path = root.join(LAYOUT_PATH);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("forest slice layout {}: {e}", path.display()));
        let manifest: Layout = serde_json::from_str(&source)
            .unwrap_or_else(|e| panic!("forest slice layout JSON {}: {e}", path.display()));
        assert_eq!(
            manifest.schema, "warbell.forest_slice.v1",
            "forest slice schema"
        );
        assert!(
            manifest
                .origin
                .iter()
                .chain(manifest.bounds.iter())
                .chain(manifest.camera.eye.iter())
                .chain(manifest.camera.target.iter())
                .all(|v| v.is_finite()),
            "forest slice non-finite origin/bounds/camera"
        );
        assert!(
            manifest.bounds.iter().all(|v| *v > 0.0 && *v <= 128.0),
            "forest slice bounds must be a compact art study"
        );
        assert!(
            !manifest.instances.is_empty(),
            "forest slice layout has no instances"
        );
        for (index, instance) in manifest.instances.iter().enumerate() {
            assert!(
                !instance.model.is_empty(),
                "forest slice instance {index}: empty model"
            );
            assert!(
                instance
                    .position
                    .iter()
                    .chain(instance.scale.iter())
                    .all(|v| v.is_finite())
                    && instance.rotation_y.is_finite(),
                "forest slice instance {index}: non-finite transform"
            );
            assert!(
                instance.scale.iter().all(|v| *v > 0.0),
                "forest slice instance {index}: non-positive scale"
            );
        }
        manifest
    })
}

pub fn origin() -> Vec3 {
    Vec3::from_array(layout().origin)
}

/// Slice-only preload list: the shared environment loader can skip unrelated campaign
/// architecture JSON, leaving the paused whole-world experiment out of this preview.
pub fn environment_model_names() -> HashSet<String> {
    layout()
        .instances
        .iter()
        .filter(|i| {
            !i.model.starts_with("tree:")
                && !i.model.starts_with("slice:")
                && !i.model.starts_with("gltf:")
        })
        .map(|i| i.model.clone())
        .collect()
}

fn slice_model_names() -> HashSet<String> {
    layout()
        .instances
        .iter()
        .filter_map(|i| i.model.strip_prefix("slice:"))
        .map(str::to_owned)
        .collect()
}

/// The manifest's camera is relative to its remote origin. `FOREST_CAM` may still override
/// this in `scene::env_cam`, which is useful for matched close-up and overview captures.
pub fn camera_transform() -> Option<Transform> {
    if !enabled() {
        return None;
    }
    let manifest = layout();
    let eye = origin() + Vec3::from_array(manifest.camera.eye);
    let target = origin() + Vec3::from_array(manifest.camera.target);
    assert!(
        eye.distance_squared(target) > 0.01,
        "forest slice camera eye equals target"
    );
    Some(Transform::from_translation(eye).looking_at(target, Vec3::Y))
}

/// Match the existing bokeh focus plane to the Blender-composed camera target.
pub fn camera_focus_distance() -> Option<f32> {
    enabled().then(|| {
        let camera = &layout().camera;
        Vec3::from_array(camera.eye).distance(Vec3::from_array(camera.target))
    })
}

fn slice_path(name: &str) -> PathBuf {
    let root = std::env::var("BEVY_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    root.join(SLICE_ASSET_DIR).join(name)
}

/// CC0 Poly Haven sky, baked to Bevy's six-face RGBA16F order for this art study.
pub fn sky_cubemap() -> Image {
    const FACE: u32 = 512;
    let path = slice_path("sky_cube_rgba16f.bin");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("forest slice sky cube {}: {e}", path.display()));
    assert_eq!(
        bytes.len(),
        (FACE * FACE * 6 * 8) as usize,
        "forest slice sky cube byte size"
    );
    let mut image = Image::new(
        Extent3d {
            width: FACE,
            height: FACE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

#[derive(Clone, Copy)]
enum SliceMap {
    Albedo,
    Normal,
    Orm,
}

fn mask_coverage(data: &[u8], width: u32, x_cell: u32, y_cell: u32, scale: f32) -> f32 {
    let cell = width / 4;
    let mut kept = 0u32;
    for y in y_cell * cell..(y_cell + 1) * cell {
        for x in x_cell * cell..(x_cell + 1) * cell {
            let a = data[((y * width + x) * 4 + 3) as usize] as f32;
            if (a * scale).round().min(255.0) >= MASK_BYTE {
                kept += 1;
            }
        }
    }
    kept as f32 / (cell * cell) as f32
}

fn preserve_mask_coverage(data: &mut [u8], width: u32, goals: &[f32; 16]) {
    let cell = width / 4;
    for cy in 0..4u32 {
        for cx in 0..4u32 {
            let goal = goals[(cy * 4 + cx) as usize];
            let current = mask_coverage(data, width, cx, cy, 1.0);
            if (current - goal).abs() <= 0.5 / (cell * cell) as f32 {
                continue;
            }
            let (mut lo, mut hi) = if current < goal {
                (1.0f32, 64.0f32)
            } else {
                (0.0f32, 1.0f32)
            };
            for _ in 0..16 {
                let mid = (lo + hi) * 0.5;
                if mask_coverage(data, width, cx, cy, mid) < goal {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let lo_err = (mask_coverage(data, width, cx, cy, lo) - goal).abs();
            let hi_err = (mask_coverage(data, width, cx, cy, hi) - goal).abs();
            let scale = if lo_err <= hi_err { lo } else { hi };
            for y in cy * cell..(cy + 1) * cell {
                for x in cx * cell..(cx + 1) * cell {
                    let a = &mut data[((y * width + x) * 4 + 3) as usize];
                    *a = (*a as f32 * scale).round().min(255.0) as u8;
                }
            }
        }
    }
}

fn slice_map(name: &str, kind: SliceMap) -> Image {
    let path = slice_path(name);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("forest slice texture {}: {e}", path.display()));
    let rgba = image::load_from_memory(&bytes)
        .unwrap_or_else(|e| panic!("forest slice texture decode {}: {e}", path.display()))
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    assert_eq!(
        (width, height),
        (2048, 2048),
        "forest slice PBR atlas must be 2048px"
    );
    let mut level = rgba.into_raw();
    let mut data = level.clone();
    let mut w = width;
    let mut mips = 1u32;
    let coverage = matches!(kind, SliceMap::Albedo).then(|| {
        std::array::from_fn(|i| mask_coverage(&level, width, (i as u32) % 4, (i as u32) / 4, 1.0))
    });
    // 4×4 atlas cells stay at least 16×16 px. Albedo averages alpha-weighted sRGB
    // colours and restores each cell's mask coverage; normal/ORM stay linear data.
    while w > 64 {
        let nw = w / 2;
        let mut next = vec![0u8; (nw * nw * 4) as usize];
        for y in 0..nw {
            for x in 0..nw {
                let out = ((y * nw + x) * 4) as usize;
                let samples = [
                    ((2 * y * w + 2 * x) * 4) as usize,
                    ((2 * y * w + 2 * x + 1) * 4) as usize,
                    (((2 * y + 1) * w + 2 * x) * 4) as usize,
                    (((2 * y + 1) * w + 2 * x + 1) * 4) as usize,
                ];
                match kind {
                    SliceMap::Albedo => {
                        let alpha: u32 = samples.iter().map(|&i| level[i + 3] as u32).sum();
                        for c in 0..3 {
                            next[out + c] = if alpha == 0 {
                                0
                            } else {
                                (samples
                                    .iter()
                                    .map(|&i| level[i + c] as u32 * level[i + 3] as u32)
                                    .sum::<u32>()
                                    / alpha) as u8
                            };
                        }
                        next[out + 3] = (alpha as f32 / 4.0).round() as u8;
                    }
                    SliceMap::Normal => {
                        let mut n = Vec3::ZERO;
                        for &i in &samples {
                            n += Vec3::new(
                                level[i] as f32,
                                level[i + 1] as f32,
                                level[i + 2] as f32,
                            ) * (2.0 / 255.0)
                                - Vec3::ONE;
                        }
                        let n = n.normalize_or_zero();
                        for c in 0..3 {
                            next[out + c] = ((n[c] * 0.5 + 0.5) * 255.0).round() as u8;
                        }
                        next[out + 3] = 255;
                    }
                    SliceMap::Orm => {
                        for c in 0..3 {
                            next[out + c] =
                                (samples.iter().map(|&i| level[i + c] as u32).sum::<u32>() / 4)
                                    as u8;
                        }
                        next[out + 3] = 255;
                    }
                }
            }
        }
        if let Some(goals) = &coverage {
            let mut output = next.clone();
            preserve_mask_coverage(&mut output, nw, goals);
            data.extend_from_slice(&output);
        } else {
            data.extend_from_slice(&next);
        }
        level = next;
        w = nw;
        mips += 1;
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        if matches!(kind, SliceMap::Albedo) {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = mips;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

#[derive(Clone, Copy)]
enum GltfMipKind {
    Albedo(Option<f32>),
    Normal,
    Orm,
}

#[derive(Resource)]
struct GltfMipSources {
    pending: Vec<Handle<Gltf>>,
    processed: HashSet<AssetId<Image>>,
    updated: usize,
    total_levels: u32,
    skipped: usize,
    max_coverage_error: f32,
}

fn srgb_linear(byte: u8) -> f32 {
    let x = byte as f32 / 255.0;
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

fn linear_srgb(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let encoded = if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    (encoded * 255.0).round() as u8
}

fn alpha_coverage(rgba: &[u8], cutoff: f32, scale: f32) -> f32 {
    let threshold = cutoff.clamp(0.0, 1.0) * 255.0;
    let count = rgba.chunks_exact(4).filter(|p| (p[3] as f32 * scale).min(255.0) >= threshold).count();
    count as f32 / (rgba.len() / 4) as f32
}

fn restore_alpha_coverage(rgba: &mut [u8], cutoff: f32, target: f32) {
    let pixels = rgba.len() / 4;
    if pixels == 0 || (alpha_coverage(rgba, cutoff, 1.0) - target).abs() <= 0.5 / pixels as f32 {
        return;
    }
    let (mut low, mut high) = (0.0f32, 64.0f32);
    for _ in 0..16 {
        let mid = (low + high) * 0.5;
        if alpha_coverage(rgba, cutoff, mid) < target { low = mid; } else { high = mid; }
    }
    let scale = if (alpha_coverage(rgba, cutoff, low) - target).abs()
        <= (alpha_coverage(rgba, cutoff, high) - target).abs() { low } else { high };
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = (pixel[3] as f32 * scale).round().min(255.0) as u8;
    }
}

/// Build the missing mip chain for a single Bevy-loaded glTF PNG. Blender's glTF sampler
/// requests trilinear mips, but Bevy 0.19's PNG path creates exactly one level. Work from
/// unscaled alpha at each level so coverage correction cannot compound into solid cards.
fn add_gltf_mips(image: &mut Image, kind: GltfMipKind) -> Option<(u32, f32)> {
    let w = image.texture_descriptor.size.width as usize;
    let h = image.texture_descriptor.size.height as usize;
    if image.texture_descriptor.mip_level_count != 1
        || image.texture_descriptor.size.depth_or_array_layers != 1
        || w.min(h) <= 8
    { return None; }
    let expected_format = match kind {
        GltfMipKind::Albedo(_) => TextureFormat::Rgba8UnormSrgb,
        GltfMipKind::Normal | GltfMipKind::Orm => TextureFormat::Rgba8Unorm,
    };
    if image.texture_descriptor.format != expected_format { return None; }
    let mut previous = image.data.as_ref()?.clone();
    if previous.len() != w * h * 4 { return None; }
    let target = if let GltfMipKind::Albedo(Some(cutoff)) = kind {
        Some((cutoff, alpha_coverage(&previous, cutoff, 1.0)))
    } else { None };
    let mut packed = previous.clone();
    let (mut width, mut height, mut levels) = (w, h, 1u32);
    let srgb_table = SRGB_TO_LINEAR.get_or_init(|| std::array::from_fn(|i| srgb_linear(i as u8)));
    let mut max_coverage_error = 0.0f32;
    while width.min(height) > 8 {
        let nw = (width / 2).max(1);
        let nh = (height / 2).max(1);
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let samples = [
                    (2 * y * width + 2 * x) * 4,
                    (2 * y * width + (2 * x + 1).min(width - 1)) * 4,
                    (((2 * y + 1).min(height - 1)) * width + 2 * x) * 4,
                    (((2 * y + 1).min(height - 1)) * width + (2 * x + 1).min(width - 1)) * 4,
                ];
                let out = (y * nw + x) * 4;
                match kind {
                    GltfMipKind::Albedo(mask) => {
                        let alpha_sum: u32 = samples.iter().map(|&i| previous[i + 3] as u32).sum();
                        next[out + 3] = (alpha_sum as f32 / 4.0).round() as u8;
                        for c in 0..3 {
                            let weighted = samples.iter().map(|&i| {
                                let weight = if mask.is_some() { previous[i + 3] as f32 / 255.0 } else { 1.0 };
                                srgb_table[previous[i + c] as usize] * weight
                            }).sum::<f32>();
                            let weight_sum = if mask.is_some() { alpha_sum as f32 / 255.0 } else { 4.0 };
                            next[out + c] = linear_srgb(if weight_sum > 0.0 { weighted / weight_sum } else { 0.0 });
                        }
                    }
                    GltfMipKind::Normal => {
                        let mut normal = Vec3::ZERO;
                        for &i in &samples {
                            normal += Vec3::new(previous[i] as f32, previous[i + 1] as f32, previous[i + 2] as f32)
                                * (2.0 / 255.0) - Vec3::ONE;
                        }
                        let normal = normal.try_normalize().unwrap_or(Vec3::Z);
                        for c in 0..3 {
                            next[out + c] = ((normal[c] * 0.5 + 0.5) * 255.0).round() as u8;
                        }
                        next[out + 3] = 255;
                    }
                    GltfMipKind::Orm => {
                        for c in 0..4 {
                            next[out + c] = (samples.iter().map(|&i| previous[i + c] as u32).sum::<u32>() as f32 / 4.0).round() as u8;
                        }
                    }
                }
            }
        }
        if let Some((cutoff, coverage)) = target {
            let mut corrected = next.clone();
            restore_alpha_coverage(&mut corrected, cutoff, coverage);
            max_coverage_error = max_coverage_error.max((alpha_coverage(&corrected, cutoff, 1.0) - coverage).abs());
            packed.extend_from_slice(&corrected);
        } else {
            packed.extend_from_slice(&next);
        }
        previous = next;
        width = nw;
        height = nh;
        levels += 1;
    }
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(packed);
    Some((levels, max_coverage_error))
}

static SRGB_TO_LINEAR: OnceLock<[f32; 256]> = OnceLock::new();

fn process_gltf_image(
    handle: Option<&Handle<Image>>,
    kind: GltfMipKind,
    images: &mut Assets<Image>,
    sources: &mut GltfMipSources,
) -> bool {
    let Some(handle) = handle else { return true; };
    if sources.processed.contains(&handle.id()) { return true; }
    let Some(mut image) = images.get_mut(handle) else { return false; };
    if let Some((levels, coverage_error)) = add_gltf_mips(&mut image, kind) {
        sources.updated += 1;
        sources.total_levels += levels;
        sources.max_coverage_error = sources.max_coverage_error.max(coverage_error);
    } else {
        sources.skipped += 1;
    }
    sources.processed.insert(handle.id());
    true
}

fn generate_gltf_mips(
    mut sources: ResMut<GltfMipSources>,
    gltfs: Res<Assets<Gltf>>,
    materials: Res<Assets<GltfMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if sources.pending.is_empty() { return; }
    let mut remaining = Vec::new();
    for root in std::mem::take(&mut sources.pending) {
        let Some(gltf) = gltfs.get(&root) else { remaining.push(root); continue; };
        let mut ready = true;
        for material_handle in &gltf.materials {
            let Some(material) = materials.get(material_handle) else { ready = false; continue; };
            let cutoff = match material.alpha_mode { AlphaMode::Mask(cutoff) => Some(cutoff), _ => None };
            for (handle, kind) in [
                (material.base_color_texture.as_ref(), GltfMipKind::Albedo(cutoff)),
                (material.normal_map_texture.as_ref(), GltfMipKind::Normal),
                (material.metallic_roughness_texture.as_ref(), GltfMipKind::Orm),
                (material.occlusion_texture.as_ref(), GltfMipKind::Orm),
            ] {
                ready &= process_gltf_image(handle, kind, &mut images, &mut sources);
            }
        }
        if !ready { remaining.push(root); }
    }
    sources.pending = remaining;
    if sources.pending.is_empty() {
        info!("forest slice glTF mips: {} images processed, {} generated ({} total mip levels), {} skipped, max alpha-mask coverage deviation {:.3}",
            sources.processed.len(), sources.updated, sources.total_levels, sources.skipped,
            sources.max_coverage_error);
    }
}

fn load_slice_models(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let names = slice_model_names();
    if names.is_empty() {
        return;
    }
    let albedo = images.add(slice_map("hero_tree_atlas.png", SliceMap::Albedo));
    let normal = images.add(slice_map("hero_tree_normal.png", SliceMap::Normal));
    let orm = images.add(slice_map("hero_tree_orm.png", SliceMap::Orm));
    let common = StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(albedo),
        normal_map_texture: Some(normal),
        metallic_roughness_texture: Some(orm.clone()),
        occlusion_texture: Some(orm),
        perceptual_roughness: 1.0,
        metallic: 1.0,
        reflectance: 0.12,
        diffuse_transmission: 0.08,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let opaque_material = materials.add(StandardMaterial {
        diffuse_transmission: 0.0,
        ..common.clone()
    });
    let cutout_material = materials.add(StandardMaterial {
        alpha_mode: AlphaMode::Mask(MASK_CUTOFF),
        ..common
    });
    let mut models = HashMap::new();
    let mut triangles = 0usize;
    for name in names {
        assert!(
            !name.is_empty() && !name.contains('/') && !name.contains('\\'),
            "invalid slice model ID {name}"
        );
        let path = slice_path(&format!("{name}.json"));
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("forest slice model {}: {e}", path.display()));
        let export: SliceExport = serde_json::from_str(&source)
            .unwrap_or_else(|e| panic!("forest slice model parse {}: {e}", path.display()));
        assert_eq!(
            export.schema, "warbell.forest_slice_tree.v1",
            "{name}: schema"
        );
        if let Some(export_name) = &export.name {
            assert_eq!(export_name, &name, "{name}: model name");
        }
        let n = export.positions.len();
        assert!(
            n > 0
                && export.normals.len() == n
                && export.tangents.len() == n
                && export.uvs.len() == n
                && export.colors.len() == n,
            "{name}: vertex attributes"
        );
        assert_eq!(
            export.indices.len(),
            export.triangles * 3,
            "{name}: triangle count"
        );
        assert!(
            export.indices.iter().all(|&i| (i as usize) < n),
            "{name}: bad index"
        );
        assert!(
            export
                .positions
                .iter()
                .flatten()
                .chain(export.normals.iter().flatten())
                .chain(export.tangents.iter().flatten())
                .chain(export.uvs.iter().flatten())
                .chain(export.colors.iter().flatten())
                .all(|x| x.is_finite()),
            "{name}: nonfinite vertex"
        );
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, export.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, export.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, export.tangents);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, export.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, export.colors);
        mesh.insert_indices(Indices::U32(export.indices));
        triangles += export.triangles;
        models.insert(name, (meshes.add(mesh), export.material));
    }
    info!(
        "forest slice PBR kit: {} shared-atlas models, {} source triangles",
        models.len(),
        triangles
    );
    commands.insert_resource(SliceModels {
        models,
        cutout_material,
        opaque_material,
    });
}

pub struct ForestSlicePlugin;

impl Plugin for ForestSlicePlugin {
    fn build(&self, app: &mut App) {
        if enabled() {
            app.add_systems(PreStartup, load_slice_models);
            app.add_systems(
                Startup,
                (crate::blenderground::spawn_forest_slice_ground, spawn_slice),
            );
            if mips_enabled() {
                app.add_systems(Update, generate_gltf_mips);
            }
        }
    }
}

fn spawn_slice(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    slice_models: Option<Res<SliceModels>>,
) {
    let manifest = layout();
    let env = crate::blenderenv::get_for_slice().expect("forest slice Blender environment loader");
    let trees = crate::blendertrees::get_for_slice().expect("forest slice Blender tree loader");
    let origin = origin();
    let mut tree_count = 0usize;
    let mut cutout_count = 0usize;
    let mut hero_count = 0usize;
    let mut gltf_count = 0usize;
    let mut gltf_scenes: HashMap<String, Handle<WorldAsset>> = HashMap::new();
    let mut gltf_roots: Vec<Handle<Gltf>> = Vec::new();
    for (index, instance) in manifest.instances.iter().enumerate() {
        let transform = Transform::from_translation(origin + Vec3::from_array(instance.position))
            .with_rotation(Quat::from_rotation_y(instance.rotation_y))
            .with_scale(Vec3::from_array(instance.scale));
        if let Some(name) = instance.model.strip_prefix("tree:") {
            let tree = trees.model(name).unwrap_or_else(|| {
                panic!("forest slice instance {index}: unknown Blender tree {name}")
            });
            commands.spawn((Mesh3d(tree.mesh), MeshMaterial3d(tree.mat), transform));
            tree_count += 1;
        } else if let Some(name) = instance.model.strip_prefix("gltf:") {
            assert!(
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                "forest slice instance {index}: invalid glTF model ID {name}"
            );
            let path = format!("cc0/{name}.glb");
            assert!(
                slice_path(&path).exists(),
                "forest slice instance {index}: missing glTF {path}"
            );
            let scene = gltf_scenes.entry(name.to_owned()).or_insert_with(|| {
                if mips_enabled() {
                    gltf_roots.push(asset_server.load(format!("models/forest_slice/{path}")));
                }
                asset_server.load(
                    GltfAssetLabel::Scene(0).from_asset(format!("models/forest_slice/{path}")),
                )
            });
            commands.spawn((WorldAssetRoot(scene.clone()), transform));
            gltf_count += 1;
        } else if let Some(name) = instance.model.strip_prefix("slice:") {
            let kit = slice_models
                .as_deref()
                .expect("forest slice PBR kit loader");
            let (mesh, kind) = kit.models.get(name).unwrap_or_else(|| {
                panic!("forest slice instance {index}: unknown PBR model {name}")
            });
            let mat = match kind {
                crate::blenderenv::MaterialKind::Opaque => &kit.opaque_material,
                crate::blenderenv::MaterialKind::Cutout => &kit.cutout_material,
            };
            let mut entity =
                commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), transform));
            // Trees and shrubs cast real leaf shadows. Thousands of very thin grass and
            // flower cards would add aliasing and shadow-map cost without useful depth.
            if name.starts_with("hero_grass_") || name.starts_with("hero_flower_") {
                entity.insert(NotShadowCaster);
            }
            hero_count += 1;
        } else {
            let model = env.model(&instance.model).unwrap_or_else(|| {
                panic!(
                    "forest slice instance {index}: unknown Blender environment model {}",
                    instance.model
                )
            });
            let mut entity = commands.spawn((
                Mesh3d(model.mesh.clone()),
                MeshMaterial3d(model.mat.clone()),
                transform,
            ));
            if model.material_kind == crate::blenderenv::MaterialKind::Cutout {
                // Let broad shrubs and ferns cast grounding foliage shadows; suppress
                // only the many thin grass/flower cards to avoid dark aliasing.
                if !instance.model.starts_with("shrub_") && !instance.model.starts_with("fern_") {
                    entity.insert(NotShadowCaster);
                }
                cutout_count += 1;
            }
        }
    }
    info!(
        "forest slice: {} authored instances ({} background trees, {} PBR JSON props, {} glTF scenes from {} sources, {} environment cutouts), {}x{}u at {:?}",
        manifest.instances.len(),
        tree_count,
        hero_count,
        gltf_count,
        gltf_scenes.len(),
        cutout_count,
        manifest.bounds[0],
        manifest.bounds[1],
        origin
    );
    if mips_enabled() {
        commands.insert_resource(GltfMipSources {
            pending: gltf_roots,
            processed: HashSet::new(),
            updated: 0,
            total_levels: 0,
            skipped: 0,
            max_coverage_error: 0.0,
        });
    }
}
