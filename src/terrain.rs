//! Biome ground: a flat plane lit by the vision-shader `ExtendedMaterial`
//! (`assets/shaders/terrain.wgsl`) plus a procedurally-generated detail texture (port
//! of `terrainDetail.ts`). One big plane covers far past the 32×32 populated patch so
//! the ground reads to the fog horizon. The base colour, detail-ramp and shader params
//! all come from the active [`BiomeConfig`], so every biome reuses this one material.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;

use crate::biome::GroundDetail;

/// Side length of the populated patch (tiles == world units), centred on the origin so it
/// spans `[-HALF, HALF]`. Used by the (kept, unwired) `decor` charm's standalone placement.
#[allow(dead_code)] // referenced only by `decor`, which is authored but not wired into the map
pub const FOREST: f32 = 32.0;
#[allow(dead_code)] // ditto
pub const HALF: f32 = FOREST / 2.0;

const TERRAIN_SHADER: &str = "shaders/terrain.wgsl";

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, ForestExtension>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct ForestParams {
    /// x=detailScale, y=detailStrength, z=variation, w=meanLuminance
    pub params: Vec4,
    /// x=bump_strength (normal-perturbation amount), y=quality (0=Low, 1=High, 2=Ultra),
    /// z=macro_variety (worn-dirt/moss patch strength), w=reserved. Set per-preset by
    /// `quality::apply_quality` (startup + every Settings toggle), so the ground relief
    /// reacts live to the graphics preset. `make_material` seeds the High defaults.
    pub params2: Vec4,
    /// World→UV mapping for the wheel-rut mask (`roads::bake_rut_mask`): `xy` = world min
    /// corner, `zw` = 1/extent. `zw == 0` disables the lookup (single-biome viewer ground /
    /// ground test have no roads).
    pub rut_region: Vec4,
}

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct ForestExtension {
    #[uniform(100)]
    pub params: ForestParams,
    #[texture(101)]
    #[sampler(102)]
    pub detail: Handle<Image>,
    /// Fragment-resolution cart-wheel-rut mask (R8) — twin grooves beside artery centrelines,
    /// far below the terrain's 1u vertex-colour resolution, so they must be a texture.
    #[texture(103)]
    #[sampler(104)]
    pub rut: Handle<Image>,
}

impl MaterialExtension for ForestExtension {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }
}

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        // Only registers the material — the biome runner spawns the ground per switch.
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
    }
}

/// Build a terrain `ExtendedMaterial` for a detail spec + roughness (the detail texture
/// is baked here). Used by the single-biome ground AND the world map's per-wedge ground.
/// `rut` carries the world-map wheel-rut mask (`worldmap` bakes it once per build); `None`
/// (viewer / ground test — no roads) binds a 1×1 dummy and zeroes the region, which disables
/// the shader lookup.
pub fn make_material(
    detail: &GroundDetail,
    roughness: f32,
    rut: Option<(Handle<Image>, Vec4)>,
    images: &mut Assets<Image>,
    mats: &mut Assets<TerrainMaterial>,
) -> Handle<TerrainMaterial> {
    let (detail_img, mean) = detail_image(detail);
    let detail_h = images.add(detail_img);
    let (rut_h, rut_region) = rut.unwrap_or_else(|| {
        let img = Image::new(
            bevy::render::render_resource::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            bevy::render::render_resource::TextureDimension::D2,
            vec![0u8],
            bevy::render::render_resource::TextureFormat::R8Unorm,
            // MAIN_WORLD too: RENDER_WORLD-only drops the CPU copy after the first extract,
            // and a material minted on a later in-process world rebuild then binds a handle
            // whose GPU image can no longer be (re)prepared — the terrain draw never becomes
            // ready and the ground renders untextured/absent. Keeping the main-world copy
            // (a few hundred KB at most across these masks) keeps the handle re-preparable forever.
            bevy::asset::RenderAssetUsages::default(),
        );
        (images.add(img), Vec4::ZERO)
    });
    mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE, // vertex colour carries the hue
            perceptual_roughness: roughness,
            cull_mode: None,
            ..default()
        },
        extension: ForestExtension {
            params: ForestParams {
                params: Vec4::new(detail.scale, detail.strength, detail.variation, mean.max(0.01)),
                // High-preset defaults; apply_quality overrides on the first Update frame
                // (after the Startup world build) and on every preset toggle.
                params2: Vec4::new(1.0, 1.0, 1.0, 0.0),
                rut_region,
            },
            detail: detail_h,
            rut: rut_h,
        },
    })
}


// ── Detail texture (port of terrainDetail.ts; parameterised per biome) ───────────

// 512²: the 256 texture's grain went soft and its repeat read as a visible grid at
// gameplay zoom; doubling the lattice resolution (with an extra macro octave below)
// keeps the blades crisp and pushes the repetition below notice. Still a one-off
// CPU bake per biome (≈0.26 Mpx).
const DETAIL_PX: u32 = 512;

fn hash2(ix: f32, iy: f32, seed: f32) -> f32 {
    let d = ix * 127.1 + iy * 311.7 + seed * 74.7;
    (d.sin() * 43758.547).fract().abs()
}

/// Periodic value noise on an `nx × ny` lattice, seamless over `[0,1)²`.
fn value_noise(u: f32, v: f32, nx: i32, ny: i32, seed: f32) -> f32 {
    let x = u * nx as f32;
    let y = v * ny as f32;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - ix as f32;
    let fy = y - iy as f32;
    let h = |gx: i32, gy: i32| hash2(gx.rem_euclid(nx) as f32, gy.rem_euclid(ny) as f32, seed);
    let a = h(ix, iy);
    let b = h(ix + 1, iy);
    let c = h(ix, iy + 1);
    let d = h(ix + 1, iy + 1);
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let top = a + (b - a) * ux;
    let bot = c + (d - c) * ux;
    top + (bot - top) * uy
}

fn hex_srgb_f(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0]
}

/// Build a 256² seamless detail texture (sRGB, Repeat) from a [`GroundDetail`] ramp,
/// returning it + its mean luminance (the shader divides by mean to keep brightness
/// neutral). Same noise recipe as the TS `terrainDetail` generator.
pub(crate) fn detail_image(d: &GroundDetail) -> (Image, f32) {
    let n = DETAIL_PX as usize;
    let mut data = vec![0u8; n * n * 4];
    let dark = hex_srgb_f(d.dark);
    let base = hex_srgb_f(d.base);
    let light = hex_srgb_f(d.light);
    let seed = d.seed;
    let mut lum_sum = 0.0f64;
    for py in 0..n {
        for px in 0..n {
            let u = px as f32 / n as f32;
            let v = py as f32 / n as f32;
            let macro_ = value_noise(u, v, 3, 3, seed + 53.0); // broad worn/lush drift
            let patch = value_noise(u, v, 7, 7, seed);
            let mid = value_noise(u, v, 18, 18, seed + 11.0);
            let mid2 = value_noise(u, v, 37, 37, seed + 71.0); // clump break-up
            let grain = value_noise(u, v, 96, 96, seed + 23.0);
            // ISOTROPIC by design. The old recipe added strong vertical "blade" streaks
            // (64×7 / 23×5 anisotropic lattices) — those read as directional DIAGONAL bands
            // across the ground once the shader de-tiles the texture by warping its UVs, and
            // the effect was worse in the higher-`streak` biomes (swamp/blight). The blade
            // character belongs to the 3D ground-cover tufts, not the floor texture, so the
            // texture is now pure non-directional grain. Weighted toward the HIGHER-frequency
            // octaves (mid/mid2/grain) and away from the broad macro/patch blobs: broad
            // low-freq content read as a trackable repeating MACRO pattern when tiled, whereas
            // fine grain repeats invisibly. Sums to 1.0 (mean-neutral). `d.streak` is now
            // unused by the bake (kept on the config struct for the spec's sake).
            let _ = d.streak;
            let t = macro_ * 0.10 + patch * 0.24 + mid * 0.26 + mid2 * 0.14 + grain * 0.26;
            let t = t.clamp(0.0, 1.0);
            let col = if t < 0.5 {
                let s = t * 2.0;
                [
                    dark[0] + (base[0] - dark[0]) * s,
                    dark[1] + (base[1] - dark[1]) * s,
                    dark[2] + (base[2] - dark[2]) * s,
                ]
            } else {
                let s = (t - 0.5) * 2.0;
                [
                    base[0] + (light[0] - base[0]) * s,
                    base[1] + (light[1] - base[1]) * s,
                    base[2] + (light[2] - base[2]) * s,
                ]
            };
            let sp = 0.9 + grain * 0.2 * (0.5 + d.grain);
            let r = (col[0] * sp).min(1.0);
            let g = (col[1] * sp).min(1.0);
            let b = (col[2] * sp).min(1.0);
            lum_sum += (0.299 * r + 0.587 * g + 0.114 * b) as f64;
            let i = (py * n + px) * 4;
            data[i] = (r * 255.0).round() as u8;
            data[i + 1] = (g * 255.0).round() as u8;
            data[i + 2] = (b * 255.0).round() as u8;
            data[i + 3] = 255;
        }
    }
    let mean = (lum_sum / (n * n) as f64) as f32;
    let mut img = Image::new(
        Extent3d { width: DETAIL_PX, height: DETAIL_PX, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = repeat_sampler();
    (img, mean)
}

fn repeat_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    })
}
