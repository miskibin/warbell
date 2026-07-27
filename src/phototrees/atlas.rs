//! Foliage atlas — drawn on the CPU at startup, deterministic. Vendored from the sibling
//! `bevy-world-editor` project's `src/foliage.rs`, then trimmed for this game.
//!
//! One **2048²** RGBA atlas, four **1024²** species quadrants (pine, spruce, broadleaf, birch).
//! Each quadrant: a photographic leaf/needle SPRIG cutout in the top `LEAF_H` rows (alpha-masked
//! cards sample it), a 16px transparent guard band, then an 80px **opaque procedural bark strip**
//! along the bottom.
//!
//! **The bark strip is why a whole tree renders from ONE material.** Trunk tubes are UV'd into it
//! via [`bark_uv`], leaf cards into [`leaf_uv`], so trunk + canopy merge into a single mesh with a
//! single material — which is what lets these trees keep this game's one-entity-per-tree,
//! one-shared-material batching contract instead of needing 4 entities each. Upstream only did
//! this for its far LOD2 impostor; here it's used at full detail.
//!
//! Divergence from upstream, deliberate: upstream loads photographic CC0 bark JPGs (~38MB,
//! gitignored, fetched by a script) for the near tiers and only uses this procedural strip far
//! away. We use the procedural strip at ALL distances and ship no bark files. That costs less than
//! it sounds — upstream's bark normal maps are a silent no-op, because its tube meshes carry no
//! `ATTRIBUTE_TANGENT` and `pbr_fragment.wgsl` gates normal mapping behind `#ifdef VERTEX_TANGENTS`.
//! What's actually lost is photographic plate/fissure detail on pine and broadleaf.
//!
//! The leaf sprig PNGs ARE shipped (`assets/textures/leaves/`, ~649KB): they are the dominant
//! realism cue and are MIT-licensed (dgreenheck/ez-tree) — see the attribution in `mod.rs`.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::treegen::noise::vnoise;
use super::treegen::rng::Rng;
use super::treegen::tree::{Species, ALL_SPECIES};

/// The atlas sampler. Upstream took this from its `texload` module (not vendored — the rest of it
/// exists to load the bark/ground JPGs we deliberately don't ship). **Repeat addressing is not
/// optional**: `mesh::tube` emits an unbounded V coordinate (`end * len * 0.35`, which reaches ~7
/// on a tall trunk) so the bark strip can tile up the tube. Under Bevy's default `ClampToEdge` the
/// trunk instead smears one row of pixels along its whole length.
fn repeat_sampler() -> bevy::image::ImageSampler {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    })
}

/// Resolve an asset-relative path, trying each plausible root and returning the first that exists.
///
/// This loads through the `image` crate directly rather than Bevy's `AssetServer` (the atlas has to
/// be composited on the CPU at `PreStartup`, before any async load could complete), so it does NOT
/// inherit Bevy's asset-root resolution and has to reproduce it.
///
/// **A naive CWD-relative path is a trap, and it bit immediately.** Bevy resolves against
/// `CARGO_MANIFEST_DIR`, so the game finds its shaders and audio no matter where it is launched
/// from — but a bare relative path here silently misses whenever the working directory isn't the
/// repo root, the atlas falls back to procedural blobs, and the ONLY symptom is one `warn!` line.
/// That is exactly what an installed MSI would hit: it nests `assets/` beside the `.exe`, and
/// Explorer/shortcut launches set an arbitrary CWD. Hence the exe directory is checked explicitly.
fn resolve(rel: &str) -> std::path::PathBuf {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    // 1. Explicit override — the documented escape hatch for running the binary from elsewhere.
    if let Ok(root) = std::env::var("BEVY_ASSET_ROOT") {
        roots.push(root.into());
    }
    // 2. Beside the executable — the shipped/MSI layout (`assets/` next to the exe).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    // 3. The crate root, baked at compile time — what Bevy itself uses, so `cargo run` works from
    //    any subdirectory (the case that actually broke).
    roots.push(env!("CARGO_MANIFEST_DIR").into());
    // 4. Plain CWD, last.
    roots.push(std::path::PathBuf::new());

    for root in &roots {
        let p = root.join(rel);
        if p.exists() {
            return p;
        }
    }
    // Nothing found — hand back the CWD-relative form so the caller's warning names the path a
    // human would recognise.
    std::path::PathBuf::from(rel)
}

pub const ATLAS: u32 = 2048;
const Q: u32 = 1024;
/// Leaf region height inside a quadrant; below it sits the bark strip.
pub const LEAF_H: u32 = 928;
pub const BARK_Y0: u32 = 944;

/// Quadrant origin for a species (pine, spruce, broadleaf, birch → 2×2).
pub fn quad_origin(sp: Species) -> (u32, u32) {
    match sp {
        Species::Pine => (0, 0),
        Species::Spruce => (Q, 0),
        Species::Broadleaf => (0, Q),
        Species::Birch => (Q, Q),
    }
}

/// UV rect (u0, v0, u1, v1) of a species' LEAF region.
pub fn leaf_uv(sp: Species) -> (f32, f32, f32, f32) {
    let (qx, qy) = quad_origin(sp);
    let a = ATLAS as f32;
    (qx as f32 / a, qy as f32 / a, (qx + Q) as f32 / a, (qy + LEAF_H) as f32 / a)
}

/// UV rect of a species' BARK strip.
pub fn bark_uv(sp: Species) -> (f32, f32, f32, f32) {
    let (qx, qy) = quad_origin(sp);
    let a = ATLAS as f32;
    (qx as f32 / a, (qy + BARK_Y0) as f32 / a, (qx + Q) as f32 / a, (qy + Q) as f32 / a)
}

struct Canvas {
    px: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Canvas { px: vec![0u8; (ATLAS * ATLAS * 4) as usize] }
    }

    #[inline]
    fn blend(&mut self, x: i32, y: i32, c: [f32; 3], a: f32) {
        if x < 0 || y < 0 || x >= ATLAS as i32 || y >= ATLAS as i32 || a <= 0.0 {
            return;
        }
        let i = ((y as u32 * ATLAS + x as u32) * 4) as usize;
        let da = self.px[i + 3] as f32 / 255.0;
        let oa = a + da * (1.0 - a);
        if oa <= 0.0 {
            return;
        }
        for c_i in 0..3 {
            let dst = self.px[i + c_i] as f32 / 255.0;
            let out = (c[c_i] * a + dst * da * (1.0 - a)) / oa;
            self.px[i + c_i] = (out * 255.0) as u8;
        }
        self.px[i + 3] = (oa * 255.0) as u8;
    }

    /// Rotated soft-edged ellipse with a darker midrib — one leaf.
    fn leaf(&mut self, cx: f32, cy: f32, ang: f32, len: f32, wid: f32, col: [f32; 3]) {
        let (s, c) = ang.sin_cos();
        let r = len.max(wid) + 2.0;
        let (x0, x1) = ((cx - r) as i32, (cx + r) as i32);
        let (y0, y1) = ((cy - r) as i32, (cy + r) as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let u = c * dx + s * dy; // along the leaf
                let v = -s * dx + c * dy; // across
                let d = (u / len).powi(2) + (v / wid).powi(2);
                if d < 1.0 {
                    let edge = ((1.0 - d) * 4.0).clamp(0.0, 1.0);
                    // Midrib + slight base-to-tip darkening for depth.
                    let rib = 1.0 - 0.35 * (1.0 - (v.abs() / (wid * 0.14)).clamp(0.0, 1.0));
                    let shade = rib * (0.82 + 0.18 * (u / len + 1.0) * 0.5);
                    self.blend(x, y, [col[0] * shade, col[1] * shade, col[2] * shade], edge);
                }
            }
        }
    }

    /// Thin anti-aliased line — one needle.
    fn needle(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, col: [f32; 3]) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let steps = (len * 1.5) as i32;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let px = x0 + dx * t;
            let py = y0 + dy * t;
            let shade = 0.8 + 0.2 * t; // tips lighter
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let d = ((ox * ox + oy * oy) as f32).sqrt();
                    let a = (w - d + 0.5).clamp(0.0, 1.0) * 0.9;
                    self.blend(
                        px as i32 + ox,
                        py as i32 + oy,
                        [col[0] * shade, col[1] * shade, col[2] * shade],
                        a,
                    );
                }
            }
        }
    }
}

fn srgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

fn jitter(rng: &mut Rng, col: [f32; 3], amt: f32) -> [f32; 3] {
    let j = 1.0 + rng.signed() * amt;
    [
        (col[0] * j).clamp(0.0, 1.0),
        (col[1] * (1.0 + rng.signed() * amt)).clamp(0.0, 1.0),
        (col[2] * j * 0.9).clamp(0.0, 1.0),
    ]
}

/// Draw a broadleaf-style cluster: many overlapping oval leaves fanning from the centre.
fn cluster_leaves(
    cv: &mut Canvas,
    qx: u32,
    qy: u32,
    rng: &mut Rng,
    n: u32,
    len_range: (f32, f32),
    base: [f32; 3],
) {
    let cx = qx as f32 + Q as f32 / 2.0;
    let cy = qy as f32 + LEAF_H as f32 / 2.0;
    for _ in 0..n {
        // Position biased outward — hollow-ish middle reads as a real leaf mass.
        let ang = rng.range(0.0, std::f32::consts::TAU);
        let rad = rng.f32().sqrt() * (Q as f32 * 0.44);
        let lx = cx + ang.cos() * rad;
        let ly = cy + ang.sin() * rad * (LEAF_H as f32 / Q as f32);
        let len = rng.range(len_range.0, len_range.1);
        // Leaves point loosely away from the cluster centre.
        let la = ang + rng.signed() * 0.9;
        cv.leaf(lx, ly, la, len, len * rng.range(0.42, 0.6), jitter(rng, base, 0.16));
    }
}

/// Draw a conifer frond: twig stems fanning upward, needle pairs along each.
fn cluster_needles(
    cv: &mut Canvas,
    qx: u32,
    qy: u32,
    rng: &mut Rng,
    stems: u32,
    needle_len: (f32, f32),
    spread: f32,
    base: [f32; 3],
) {
    let cx = qx as f32 + Q as f32 / 2.0;
    let cy = qy as f32 + LEAF_H as f32 * 0.92;
    let twig = srgb(96, 74, 52);
    for s in 0..stems {
        let sa = -std::f32::consts::FRAC_PI_2
            + (s as f32 / (stems - 1).max(1) as f32 - 0.5) * spread
            + rng.signed() * 0.1;
        let slen = LEAF_H as f32 * rng.range(0.62, 0.85);
        let (sx1, sy1) = (cx + sa.cos() * slen, cy + sa.sin() * slen);
        cv.needle(cx, cy, sx1, sy1, 1.6, twig);
        let n_needles = (slen / 4.0) as u32;
        for i in 0..n_needles {
            let t = 0.12 + 0.88 * i as f32 / n_needles as f32;
            let bx = cx + (sx1 - cx) * t;
            let by = cy + (sy1 - cy) * t;
            for side in [-1.0f32, 1.0] {
                let na = sa + side * rng.range(0.55, 0.95);
                let nl = rng.range(needle_len.0, needle_len.1) * (1.0 - t * 0.35);
                cv.needle(
                    bx,
                    by,
                    bx + na.cos() * nl,
                    by + na.sin() * nl,
                    1.0,
                    jitter(rng, base, 0.12),
                );
            }
        }
    }
}

/// Opaque bark strip: vertical (or horizontal for birch) noise banding.
fn bark_strip(cv: &mut Canvas, qx: u32, qy: u32, sp: Species, rng: &mut Rng) {
    for y in BARK_Y0..Q {
        for x in 0..Q {
            let fx = x as f32;
            let fy = y as f32;
            let col = match sp {
                Species::Birch => {
                    // White bark, dark horizontal lenticel dashes.
                    let band = vnoise(fx * 0.11, fy * 0.5, 77);
                    let dash = vnoise(fx * 0.35, fy * 0.06, 12);
                    if band > 0.72 && dash > 0.55 {
                        srgb(40, 36, 32)
                    } else {
                        let v = 0.86 + 0.10 * vnoise(fx * 0.05, fy * 0.05, 5);
                        [v, v, v * 0.97]
                    }
                }
                Species::Pine => {
                    let plate = vnoise(fx * 0.06, fy * 0.02, 31);
                    let crack = vnoise(fx * 0.30, fy * 0.08, 44);
                    let v = 0.45 + plate * 0.5 - (crack > 0.7) as i32 as f32 * 0.3;
                    [0.42 * v + 0.18, 0.27 * v + 0.09, 0.18 * v + 0.05]
                }
                Species::Spruce => {
                    let v = 0.4 + 0.4 * vnoise(fx * 0.10, fy * 0.04, 90);
                    [0.30 * v + 0.10, 0.22 * v + 0.08, 0.16 * v + 0.06]
                }
                Species::Broadleaf => {
                    let v = 0.5 + 0.4 * vnoise(fx * 0.07, fy * 0.03, 61);
                    [0.38 * v + 0.20, 0.33 * v + 0.18, 0.28 * v + 0.16]
                }
            };
            let _ = rng;
            cv.blend((qx + x) as i32, (qy + y) as i32, col, 1.0);
        }
    }
}

/// The photographic sprig source for a species (EZ-Tree leaf textures, MIT — see
/// assets/textures/MANIFEST.md). Spruce reuses the pine spray; its vertex tint darkens it.
fn sprig_file(sp: Species) -> &'static str {
    match sp {
        Species::Pine | Species::Spruce => "assets/textures/leaves/pineL.png",
        Species::Broadleaf => "assets/textures/leaves/oakL.png",
        Species::Birch => "assets/textures/leaves/aspenL.png",
    }
}

/// Build the full foliage atlas image. Preferred path: composite the photographic twig
/// cutouts (a REAL sprig — visible stem + individually recognizable leaves + gaps — is
/// what makes a card read as foliage; painted blobs never do). Fallback if the files are
/// missing: the old procedural clusters.
pub fn build_atlas() -> Image {
    let mut cv = Canvas::new();
    let mut rng = Rng::new(0x0F01_1A6E);

    for sp in ALL_SPECIES {
        let (qx, qy) = quad_origin(sp);
        let blitted = image::open(resolve(sprig_file(sp)))
            .map(|img| {
                let img = image::imageops::resize(
                    &img.to_rgba8(),
                    Q,
                    LEAF_H,
                    image::imageops::FilterType::Triangle,
                );
                for (px, py, p) in img.enumerate_pixels() {
                    let i = (((qy + py) * ATLAS + qx + px) * 4) as usize;
                    cv.px[i..i + 4].copy_from_slice(&p.0);
                }
            })
            .is_ok();
        if !blitted {
            bevy::log::warn!("sprig texture missing ({}) — procedural fallback", sprig_file(sp));
            match sp {
                Species::Pine => cluster_needles(
                    &mut cv, qx, qy, &mut rng, 7, (52.0, 80.0), 2.1, srgb(68, 106, 60),
                ),
                Species::Spruce => cluster_needles(
                    &mut cv, qx, qy, &mut rng, 9, (36.0, 56.0), 2.4, srgb(52, 88, 58),
                ),
                Species::Broadleaf => cluster_leaves(
                    &mut cv, qx, qy, &mut rng, 130, (76.0, 124.0), srgb(78, 126, 54),
                ),
                Species::Birch => cluster_leaves(
                    &mut cv, qx, qy, &mut rng, 170, (44.0, 68.0), srgb(116, 152, 64),
                ),
            }
        }
        bark_strip(&mut cv, qx, qy, sp, &mut rng);
    }

    // Alpha-aware mip chain. Without mips the alpha-masked cards UNDERSAMPLE at
    // 80 m+: per-frame sparkle that SMAA/supersampling average into pale translucent
    // rectangles — whole crowns look like you can see the world through them. Colour
    // is alpha-weighted (no transparent-black bleed) and alpha is re-scaled per level
    // so the masked coverage survives the cutoff instead of dissolving.
    let mut data = Vec::new();
    let mut level = cv.px;
    let (mut w, mut h) = (ATLAS, ATLAS);
    data.extend_from_slice(&level);
    let mut mips = 1;
    while w > 1 || h > 1 {
        let nw = (w / 2).max(1);
        let nh = (h / 2).max(1);
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let (mut rs, mut gs, mut bs, mut asum) = (0u32, 0u32, 0u32, 0u32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    let i = ((sy * w + sx) * 4) as usize;
                    let a = level[i + 3] as u32;
                    rs += level[i] as u32 * a;
                    gs += level[i + 1] as u32 * a;
                    bs += level[i + 2] as u32 * a;
                    asum += a;
                }
                let o = ((y * nw + x) * 4) as usize;
                if asum > 0 {
                    next[o] = (rs / asum) as u8;
                    next[o + 1] = (gs / asum) as u8;
                    next[o + 2] = (bs / asum) as u8;
                }
                // Coverage: hold the silhouette for the first levels (box-filtered
                // alpha erodes it), then DECAY it — deep mips otherwise solidify every
                // card into a full grey rectangle, which is the blockier half of the
                // see-through artifact.
                let scaled = if mips <= 3 {
                    (asum / 4) * 13 / 10
                } else {
                    (asum / 4) * 8 / 10
                };
                next[o + 3] = scaled.min(255) as u8;
            }
        }
        data.extend_from_slice(&next);
        level = next;
        w = nw;
        h = nh;
        mips += 1;
    }
    let mut img = Image::new_uninit(
        Extent3d { width: ATLAS, height: ATLAS, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = mips;
    img.data = Some(data);
    img.sampler = repeat_sampler();
    img
}

