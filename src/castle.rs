//! The central **castle** — a faithful, textured Bevy port of the TS game's fully-upgraded
//! city (`cityModels.tsx` + `House.tsx` + `WarBell.tsx`, layout from `cityPlan.ts`). Built
//! complete: keep, perimeter walls split around four gates, four corner towers, eight
//! houses, a cobbled courtyard, the war bell, banners, torches and chimney smoke.
//!
//! Static — no gameplay. At the island centre (world origin, flat grass safe-zone, y=0).
//! Kept at TS *absolute* size (NOT ×1.4) but the PERIMETER is widened (`HALF_X/HALF_Z`) for
//! more courtyard room. Coordinate map: world = `(base − (72,54))`.
//!
//! Surfaces use procedural canvas-style textures ported from the TS `textures.ts`
//! (ashlar stone, plaster, wood planks, roof shingles, cobbles, tilled soil) on tiling
//! repeat-sampled materials — the "feels textured like the original" the brief asked for.
//! Solid bits (banners, bronze, crops) + emissive bits (windows, torch flames, gold) use
//! plain materials. Walls + building footprints register in [`crate::blockers`] so the
//! ambient animals route around them.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::biome::BiomeEntity;
use crate::economy::Defenses;
use crate::palette::srgb;

// ── Palette (sRGB hex, from the TS materials) ────────────────────────────────────
const STONE: u32 = 0x7d7e86;
const DARK_STONE: u32 = 0x5c5d64;
const LIGHT_STONE: u32 = 0x969aa4;
const BEAM: u32 = 0x5a3a22;
const ROOF: u32 = 0x7a2f28;
const BANNER: u32 = 0x2f5fa6;
const WOOD: u32 = 0x3a2618;
const SOIL: u32 = 0x6b4a2a;
const CROP: u32 = 0x8fae4a;
const GOLD: u32 = 0xe0b04a;
const COBBLE: u32 = 0x8b8a86; // courtyard paving
const H_WALL: u32 = 0xd3b78b;
const H_ROOF: u32 = 0x6b3322;
const H_STONE: u32 = 0x6e6e76;
const WINDOW_GLOW: u32 = 0xffd58c;
const BRONZE: u32 = 0xb9892f;
const BRONZE_DARK: u32 = 0x7c5a1e;
const TORCH_FLAME: u32 = 0xff7a2a;
const SLIT: u32 = 0x23242a; // arrow-slit / shadow inset

const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;
/// Texture tiles every ~this many world units (keeps block scale consistent).
const TILE: f32 = 1.5;

// Perimeter half-extents (widened from the TS 13×9 for more courtyard room).
const HALF_X: f32 = 17.0;
const HALF_Z: f32 = 12.0;
const GATE_GAP: f32 = 4.0;

// ── Material slots ───────────────────────────────────────────────────────────────
#[derive(Clone, Copy)]
enum M {
    Stone,
    DarkStone,
    LightStone,
    HouseStone,
    Plaster,
    Wood,
    Beam,
    Roof,
    HouseRoof,
    Soil,
    Cobble,
    Banner,
    Bronze,
    BronzeDark,
    Crop,
    Slit,
    Gold,
    Window,
    Flame,
}

struct Mats {
    h: std::collections::HashMap<u8, Handle<StandardMaterial>>,
}
impl Mats {
    fn get(&self, m: M) -> Handle<StandardMaterial> {
        self.h[&(m as u8)].clone()
    }
}

// ── Procedural textures (ported from textures.ts) ────────────────────────────────
struct Rng(u32);
impl Rng {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    }
}

fn rgb(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 0xff) as f32, ((hex >> 8) & 0xff) as f32, (hex & 0xff) as f32]
}
/// Shift each channel by `amt`×255, clamp to bytes.
fn shade(c: [f32; 3], amt: f32) -> [u8; 3] {
    let f = |v: f32| (v + amt * 255.0).clamp(0.0, 255.0).round() as u8;
    [f(c[0]), f(c[1]), f(c[2])]
}

const TN: usize = 128;
struct Canvas {
    px: Vec<u8>,
}
impl Canvas {
    fn new(base: [u8; 3]) -> Self {
        let mut px = vec![0u8; TN * TN * 4];
        for i in 0..TN * TN {
            px[i * 4] = base[0];
            px[i * 4 + 1] = base[1];
            px[i * 4 + 2] = base[2];
            px[i * 4 + 3] = 255;
        }
        Canvas { px }
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [u8; 3]) {
        let x0 = (x.floor().max(0.0)) as usize;
        let y0 = (y.floor().max(0.0)) as usize;
        let x1 = ((x + w).ceil().min(TN as f32)) as usize;
        let y1 = ((y + h).ceil().min(TN as f32)) as usize;
        for yy in y0..y1 {
            for xx in x0..x1 {
                let i = (yy * TN + xx) * 4;
                self.px[i] = c[0];
                self.px[i + 1] = c[1];
                self.px[i + 2] = c[2];
            }
        }
    }
    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 3], a: f32) {
        let x0 = ((cx - r).floor().max(0.0)) as usize;
        let y0 = ((cy - r).floor().max(0.0)) as usize;
        let x1 = ((cx + r).ceil().min(TN as f32)) as usize;
        let y1 = ((cy + r).ceil().min(TN as f32)) as usize;
        for yy in y0..y1 {
            for xx in x0..x1 {
                let dx = xx as f32 - cx;
                let dy = yy as f32 - cy;
                if dx * dx + dy * dy <= r * r {
                    let i = (yy * TN + xx) * 4;
                    for k in 0..3 {
                        self.px[i + k] = (self.px[i + k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
                    }
                }
            }
        }
    }
    fn into_image(self) -> Image {
        let mut img = Image::new(
            Extent3d { width: TN as u32, height: TN as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            self.px,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            address_mode_w: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
        img
    }
}

fn speckle(cv: &mut Canvas, r: &mut Rng, n: usize, c: [f32; 3]) {
    for _ in 0..n {
        let x = (r.f() * TN as f32).floor();
        let y = (r.f() * TN as f32).floor();
        cv.rect(x, y, 1.0, 1.0, shade(c, (r.f() - 0.5) * 0.1));
    }
}

/// Ashlar courses — castle stone (running-bond bricks + mortar + bevel highlight).
fn tex_stone(hex: u32) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, -0.07));
    let mut r = Rng(0x511 ^ hex);
    let (rows, cols) = (4usize, 4usize);
    let (bh, bw) = (TN as f32 / rows as f32, TN as f32 / cols as f32);
    for ry in 0..rows {
        let off = (ry % 2) as f32 * (bw / 2.0);
        for i in -1..=cols as i32 {
            let x = i as f32 * bw + off + 1.5;
            let y = ry as f32 * bh + 1.5;
            cv.rect(x, y, bw - 3.0, bh - 3.0, shade(c, (r.f() - 0.5) * 0.12));
            cv.rect(x, y, bw - 3.0, 1.5, shade(c, 0.06)); // top bevel
        }
    }
    speckle(&mut cv, &mut r, 600, c);
    cv.into_image()
}

/// Plaster / stucco — house walls (mottled blobs + speckle).
fn tex_plaster(hex: u32) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, 0.0));
    let mut r = Rng(0x9a3 ^ hex);
    for _ in 0..90 {
        let x = r.f() * TN as f32;
        let y = r.f() * TN as f32;
        let rad = 4.0 + r.f() * 14.0;
        cv.disc(x, y, rad, shade(c, (r.f() - 0.5) * 0.5), 0.06 + r.f() * 0.08);
    }
    speckle(&mut cv, &mut r, 400, c);
    cv.into_image()
}

/// Wood planks — beams/doors (vertical planks + gaps + grain streaks).
fn tex_wood(hex: u32, planks: usize) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, 0.0));
    let mut r = Rng(0x4d2 ^ hex);
    let pw = TN as f32 / planks as f32;
    for i in 0..planks {
        let x = i as f32 * pw;
        cv.rect(x, 0.0, pw, TN as f32, shade(c, (r.f() - 0.5) * 0.1));
        cv.rect(x, 0.0, 1.5, TN as f32, shade(c, -0.22)); // plank gap
        for _ in 0..7 {
            let gx = x + 3.0 + r.f() * (pw - 6.0);
            cv.rect(gx, 0.0, 0.8, TN as f32, shade(c, (r.f() - 0.5) * 0.14)); // grain streak
        }
    }
    cv.into_image()
}

/// Roof shingles — overlapping rows (offset tiles + shadow band per row).
fn tex_shingle(hex: u32) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, -0.12));
    let mut r = Rng(0x5f1 ^ hex);
    let (rows, cols) = (6usize, 6usize);
    let (rh, cw) = (TN as f32 / rows as f32, TN as f32 / cols as f32);
    for ry in 0..rows {
        let off = (ry % 2) as f32 * (cw / 2.0);
        for i in -1..=cols as i32 {
            let x = i as f32 * cw + off;
            let y = ry as f32 * rh;
            cv.rect(x + 0.5, y, cw - 1.0, rh * 0.86, shade(c, (r.f() - 0.5) * 0.14));
            cv.rect(x + 0.5, y + rh * 0.86, cw - 1.0, rh * 0.14, shade(c, -0.2)); // shadow line
        }
    }
    cv.into_image()
}

/// Cobbles — courtyard paving (jittered running-bond stones + bevels).
fn tex_cobble(hex: u32) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, -0.18));
    let mut r = Rng(0xc0b ^ hex);
    let cells = 5usize;
    let cs = TN as f32 / cells as f32;
    for ry in 0..cells {
        let off = (ry % 2) as f32 * (cs / 2.0);
        for rx in -1..=cells as i32 {
            let jx = (r.f() - 0.5) * 4.0;
            let jy = (r.f() - 0.5) * 4.0;
            let x = rx as f32 * cs + off + 2.0 + jx;
            let y = ry as f32 * cs + 2.0 + jy;
            let (w, h) = (cs - 4.0, cs - 4.0);
            cv.rect(x, y, w, h, shade(c, (r.f() - 0.5) * 0.18));
            cv.rect(x, y, w, 1.5, shade(c, 0.08)); // top sheen
            cv.rect(x, y, 1.5, h, shade(c, 0.08));
            cv.rect(x, y + h - 1.5, w, 1.5, shade(c, -0.12)); // bottom shadow
        }
    }
    speckle(&mut cv, &mut r, 400, c);
    cv.into_image()
}

/// Tilled soil — farm bed (horizontal furrow ridges + speckle).
fn tex_soil(hex: u32) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, 0.0));
    let mut r = Rng(0x501 ^ hex);
    let mut y = 0.0;
    while y < TN as f32 {
        cv.rect(0.0, y, TN as f32, 5.0, shade(c, 0.05));
        cv.rect(0.0, y + 5.0, TN as f32, 5.0, shade(c, -0.08));
        y += 10.0;
    }
    speckle(&mut cv, &mut r, 700, c);
    cv.into_image()
}

// ── Material table ───────────────────────────────────────────────────────────────
fn build_mats(images: &mut Assets<Image>, std_mats: &mut Assets<StandardMaterial>) -> Mats {
    let mut h = std::collections::HashMap::new();
    // Double-sided: our custom gable/slab/taper meshes don't all wind CCW-outward, and
    // back-face culling would drop those faces (the see-through roof bug). Bevy flips the
    // normal for back fragments so lighting stays correct.
    let mut tex = |img: Image, rough: f32, m: M| {
        let t = images.add(img);
        let handle = std_mats.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(t),
            perceptual_roughness: rough,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        h.insert(m as u8, handle);
    };
    tex(tex_stone(STONE), 0.92, M::Stone);
    tex(tex_stone(DARK_STONE), 0.92, M::DarkStone);
    tex(tex_stone(LIGHT_STONE), 0.9, M::LightStone);
    tex(tex_stone(H_STONE), 0.92, M::HouseStone);
    tex(tex_plaster(H_WALL), 0.95, M::Plaster);
    tex(tex_wood(WOOD, 3), 1.0, M::Wood);
    tex(tex_wood(BEAM, 4), 1.0, M::Beam);
    tex(tex_shingle(ROOF), 0.85, M::Roof);
    tex(tex_shingle(H_ROOF), 0.85, M::HouseRoof);
    tex(tex_soil(SOIL), 1.0, M::Soil);
    tex(tex_cobble(COBBLE), 0.95, M::Cobble);

    let mut solid = |hex: u32, rough: f32, metal: f32, m: M| {
        h.insert(m as u8, std_mats.add(StandardMaterial {
            base_color: srgb(hex),
            perceptual_roughness: rough,
            metallic: metal,
            cull_mode: None,
            double_sided: true,
            ..default()
        }));
    };
    solid(BANNER, 0.8, 0.0, M::Banner);
    solid(BRONZE, 0.45, 0.7, M::Bronze);
    solid(BRONZE_DARK, 0.5, 0.6, M::BronzeDark);
    solid(CROP, 0.9, 0.0, M::Crop);
    solid(SLIT, 0.9, 0.0, M::Slit);

    let mut glow = |hex: u32, intensity: f32, metal: f32, m: M| {
        h.insert(m as u8, std_mats.add(StandardMaterial {
            base_color: srgb(hex),
            emissive: srgb(hex).to_linear() * intensity,
            perceptual_roughness: 0.5,
            metallic: metal,
            ..default()
        }));
    };
    glow(GOLD, 0.8, 0.6, M::Gold);
    glow(WINDOW_GLOW, 2.2, 0.0, M::Window);
    glow(TORCH_FLAME, 6.0, 0.0, M::Flame);

    Mats { h }
}

// ── Mesh helpers ─────────────────────────────────────────────────────────────────
fn scale_uv(mut m: Mesh, su: f32, sv: f32) -> Mesh {
    if let Some(VertexAttributeValues::Float32x2(uvs)) = m.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for uv in uvs.iter_mut() {
            uv[0] *= su;
            uv[1] *= sv;
        }
    }
    m
}

/// A textured box centred at (x,y,z), UVs scaled so the texture tiles at ~`TILE` units.
fn bx(w: f32, h: f32, d: f32, x: f32, y: f32, z: f32) -> Mesh {
    let horiz = ((w + d) * 0.5 / TILE).max(0.6);
    let vert = (h / TILE).max(0.6);
    scale_uv(Mesh::from(Cuboid::new(w, h, d)), horiz, vert).translated_by(Vec3::new(x, y, z))
}

fn flat(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// A 4-sided pyramid roof, base at `base_y`, apex up (45° so faces align to a square).
fn pyramid(r: f32, h: f32, base_y: f32) -> Mesh {
    let m = Cone { radius: r, height: h }
        .mesh()
        .resolution(4)
        .build()
        .rotated_by(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4))
        .translated_by(Vec3::new(0.0, base_y + h / 2.0, 0.0));
    flat(scale_uv(m, r / TILE, h / TILE))
}

fn cyl(r: f32, h: f32, x: f32, y: f32, z: f32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(12).build().translated_by(Vec3::new(x, y, z))
}

/// Tapered 12-gon frustum (bell body): top radius `rt`, bottom `rb`.
fn taper(rt: f32, rb: f32, h: f32, y: f32) -> Mesh {
    let seg = 12usize;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let tau = std::f32::consts::TAU;
    for i in 0..seg {
        let a = i as f32 / seg as f32 * tau;
        let (co, si) = (a.cos(), a.sin());
        pos.push([co * rb, y - h / 2.0, si * rb]);
        pos.push([co * rt, y + h / 2.0, si * rt]);
    }
    for i in 0..seg {
        let b0 = (i * 2) as u32;
        let t0 = b0 + 1;
        let b1 = (((i + 1) % seg) * 2) as u32;
        let t1 = b1 + 1;
        idx.extend_from_slice(&[b0, t0, t1, b0, t1, b1]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    flat(m)
}

/// Gable (triangular-prism) roof — ridge along X, slopes facing ±Z, gable triangles ±X.
fn gable(span_x: f32, span_z: f32, rise: f32, base_y: f32) -> Mesh {
    let hx = span_x / 2.0;
    let hz = span_z / 2.0;
    let (y0, y1) = (base_y, base_y + rise);
    // 0,1,2 = +X end (back-left, back-right, apex); 3,4,5 = -X end
    let pos = vec![
        [hx, y0, -hz], [hx, y0, hz], [hx, y1, 0.0],
        [-hx, y0, -hz], [-hx, y0, hz], [-hx, y1, 0.0],
    ];
    // planar UV from (x,z) so shingles tile across the roof
    let uv = vec![
        [hx / TILE, -hz / TILE], [hx / TILE, hz / TILE], [hx / TILE, 0.0],
        [-hx / TILE, -hz / TILE], [-hx / TILE, hz / TILE], [-hx / TILE, 0.0],
    ];
    let idx = vec![
        0, 2, 1, // +X gable (normal +X)
        3, 4, 5, // -X gable (normal -X)
        1, 2, 5, 1, 5, 4, // +Z slope (normal +Y/+Z)
        0, 3, 5, 0, 5, 2, // -Z slope (normal +Y/-Z)
        0, 4, 3, 0, 1, 4, // underside (normal -Y)
    ];
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    flat(m)
}

/// A flat upward-facing slab quad at height `y` (cobble courtyard), UV tiled.
fn slab(w: f32, d: f32, y: f32) -> Mesh {
    let (hw, hd) = (w / 2.0, d / 2.0);
    let pos = vec![[-hw, y, -hd], [hw, y, -hd], [hw, y, hd], [-hw, y, hd]];
    let nrm = vec![[0.0, 1.0, 0.0]; 4];
    let uv = vec![[0.0, 0.0], [w / TILE, 0.0], [w / TILE, d / TILE], [0.0, d / TILE]];
    let idx = vec![0u32, 2, 1, 0, 3, 2];
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    m
}

fn bake(m: Mesh, pos: Vec3, rot: f32, scale: Vec3) -> Mesh {
    m.scaled_by(scale).rotated_by(Quat::from_rotation_y(rot)).translated_by(pos)
}

// ── Per-structure local parts ────────────────────────────────────────────────────
const KEEP_W: f32 = 7.0;
const KEEP_H: f32 = 1.9;
const KEEP_D: f32 = 6.0;
const KEEP_FOUND: f32 = 0.3;

fn keep_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    let roof_y = KEEP_FOUND + KEEP_H;
    v.push((bx(KEEP_W + 0.5, KEEP_FOUND, KEEP_D + 0.5, 0.0, KEEP_FOUND / 2.0, 0.0), M::DarkStone));
    v.push((bx(KEEP_W, KEEP_H, KEEP_D, 0.0, KEEP_FOUND + KEEP_H / 2.0, 0.0), M::Stone));
    // Corner buttresses.
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.5, KEEP_H + 0.1, 0.5, sx * (KEEP_W / 2.0 - 0.15), KEEP_FOUND + (KEEP_H + 0.1) / 2.0, sz * (KEEP_D / 2.0 - 0.15)), M::DarkStone));
        }
    }
    // Merlons.
    let mut x = -KEEP_W / 2.0 + 0.4;
    while x <= KEEP_W / 2.0 - 0.4 + 1e-3 {
        v.push((bx(0.5, 0.5, 0.5, x, roof_y + 0.25, -KEEP_D / 2.0 + 0.2), M::DarkStone));
        v.push((bx(0.5, 0.5, 0.5, x, roof_y + 0.25, KEEP_D / 2.0 - 0.2), M::DarkStone));
        x += 1.0;
    }
    let mut z = -KEEP_D / 2.0 + 1.2;
    while z <= KEEP_D / 2.0 - 1.2 + 1e-3 {
        v.push((bx(0.5, 0.5, 0.5, -KEEP_W / 2.0 + 0.2, roof_y + 0.25, z), M::DarkStone));
        v.push((bx(0.5, 0.5, 0.5, KEEP_W / 2.0 - 0.2, roof_y + 0.25, z), M::DarkStone));
        z += 1.0;
    }
    // Central tower + roof + finial.
    v.push((bx(2.0, 1.3, 2.0, 0.0, roof_y + 0.65, 0.0), M::LightStone));
    v.push((pyramid(1.4, 0.95, roof_y + 1.3), M::Roof));
    v.push((Mesh::from(Sphere::new(0.18).mesh().ico(2).unwrap()).translated_by(Vec3::new(0.0, roof_y + 2.1, 0.0)), M::Gold));
    // Door + arch beam + flanking banners.
    v.push((bx(1.4, 1.6, 0.12, 0.0, KEEP_FOUND + 0.85, KEEP_D / 2.0 + 0.02), M::Wood));
    v.push((bx(1.7, 0.3, 0.2, 0.0, KEEP_FOUND + 1.75, KEEP_D / 2.0 + 0.05), M::Beam));
    for sx in [-1.45_f32, 1.45] {
        v.push((bx(0.6, 1.5, 0.04, sx, KEEP_FOUND + 1.3, KEEP_D / 2.0 + 0.08), M::Banner));
    }
    // Arrow slits on the front.
    for sx in [-2.4_f32, 2.4] {
        v.push((bx(0.16, 0.7, 0.06, sx, KEEP_FOUND + 1.1, KEEP_D / 2.0 + 0.02), M::Slit));
    }
    v
}

const WALL_H: f32 = 1.35;
const WALL_THICK: f32 = 0.6;

fn wall_parts(len: f32) -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(len, WALL_H, WALL_THICK, 0.0, WALL_H / 2.0, 0.0), M::Stone));
    // Walkway lip (a thin overhang course just under the merlons).
    v.push((bx(len, 0.12, WALL_THICK + 0.12, 0.0, WALL_H - 0.06, 0.0), M::DarkStone));
    let step = 0.8;
    let count = (len / step).floor().max(1.0) as i32;
    let start = -((count - 1) as f32 * step) / 2.0;
    for i in 0..count {
        v.push((bx(0.38, 0.42, WALL_THICK + 0.06, start + i as f32 * step, WALL_H + 0.21, 0.0), M::Stone));
    }
    v
}

const TOWER_H: f32 = 2.5;

fn tower_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(1.8, TOWER_H, 1.8, 0.0, TOWER_H / 2.0, 0.0), M::Stone));
    v.push((bx(2.1, 0.4, 2.1, 0.0, TOWER_H + 0.1, 0.0), M::DarkStone)); // battlement
    // Corner merlons.
    for sx in [-1, 1] {
        for sz in [-1, 1] {
            v.push((bx(0.4, 0.4, 0.4, sx as f32 * 0.85, TOWER_H + 0.45, sz as f32 * 0.85), M::DarkStone));
        }
    }
    v.push((pyramid(1.45, 1.35, TOWER_H + 0.3), M::Roof));
    // Arrow slits.
    for (ax, az, rot) in [(0.0, 0.91, 0.0), (0.91, 0.0, HALF_PI)] {
        v.push((bake(bx(0.16, 0.8, 0.06, 0.0, 0.0, 0.0), Vec3::new(ax, TOWER_H * 0.55, az), rot, Vec3::ONE), M::Slit));
    }
    // Flag.
    v.push((cyl(0.04, 0.9, 0.0, TOWER_H + 1.95, 0.0), M::Beam));
    v.push((bx(0.55, 0.34, 0.03, 0.3, TOWER_H + 2.2, 0.0), M::Banner));
    v
}

const GATE_H: f32 = 2.0;

fn gate_parts(width: f32) -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    let half = width / 2.0;
    for sx in [-half, half] {
        v.push((bx(0.9, GATE_H, 0.9, sx, GATE_H / 2.0, 0.0), M::Stone));
        v.push((bx(1.0, 0.3, 1.0, sx, GATE_H + 0.05, 0.0), M::DarkStone)); // capital
    }
    v.push((bx(width + 1.2, 0.5, 0.8, 0.0, GATE_H + 0.35, 0.0), M::Beam)); // lintel
    v.push((bx(0.5, 0.4, 0.12, 0.0, GATE_H + 0.8, 0.0), M::Gold)); // crest
    // Open door leaves swung against the posts (with iron band).
    for (sx, rot) in [(-half + 0.1, 0.9_f32), (half - 0.1, -0.9)] {
        let leaf = (half - 0.1).max(0.3);
        v.push((bake(bx(leaf, GATE_H - 0.4, 0.12, 0.0, 0.0, 0.0), Vec3::new(sx, GATE_H / 2.0, 0.6), rot, Vec3::ONE), M::Wood));
    }
    v
}

// House.
const HW: f32 = 2.6;
const HH: f32 = 1.15;
const HD: f32 = 2.0;
const H_FOUND: f32 = 0.18;

fn house_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    let wall_top = H_FOUND + HH;
    v.push((bx(HW + 0.2, H_FOUND, HD + 0.2, 0.0, H_FOUND / 2.0, 0.0), M::HouseStone));
    v.push((bx(0.26, 0.75, 0.26, HW / 2.0 - 0.4, wall_top + 0.38, 0.25), M::HouseStone)); // chimney
    v.push((bx(0.32, 0.08, 0.32, HW / 2.0 - 0.4, wall_top + 0.74, 0.25), M::Beam)); // cap
    v.push((bx(HW, HH, HD, 0.0, H_FOUND + HH / 2.0, 0.0), M::Plaster));
    // Corner timber posts (half-timbered look). Straddle the wall's corner EDGE (centre on
    // the corner, half the 0.14 post inside, half proud) — so no post face is coplanar with
    // a wall face. (At the old `HW/2 - 0.07` inset the outer faces sat exactly on the wall
    // surface, z-fighting into the corner flicker.)
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.16, HH, 0.16, sx * (HW / 2.0), H_FOUND + HH / 2.0, sz * (HD / 2.0)), M::Beam));
        }
    }
    // Door + lintel + glowing window on the +Z front.
    v.push((bx(0.46, 0.92, 0.06, -0.56, H_FOUND + 0.5, HD / 2.0 + 0.02), M::Wood));
    v.push((bx(0.56, 0.1, 0.08, -0.56, H_FOUND + 1.0, HD / 2.0 + 0.03), M::Beam));
    v.push((bx(0.42, 0.42, 0.04, 0.5, H_FOUND + 0.9, HD / 2.0 + 0.02), M::Window));
    v.push((bx(0.52, 0.06, 0.06, 0.5, H_FOUND + 1.13, HD / 2.0 + 0.03), M::Beam)); // window lintel
    // Gable roof — ridge along X (width), slopes facing ±Z.
    v.push((gable(HW + 0.3, HD + 0.3, 0.6, wall_top), M::HouseRoof));
    v
}

const BELL_POST_H: f32 = 1.6;

fn bell_parts() -> Vec<(Mesh, M)> {
    let beam_y = BELL_POST_H - 0.06;
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(1.5, 0.14, 0.5, 0.0, 0.07, 0.0), M::Beam)); // sill
    for sx in [-0.6_f32, 0.6] {
        v.push((bx(0.12, BELL_POST_H, 0.12, sx, BELL_POST_H / 2.0, 0.0), M::Beam));
    }
    v.push((bx(1.5, 0.14, 0.16, 0.0, beam_y, 0.0), M::Beam)); // crossbeam
    v.push((bx(0.1, 0.18, 0.1, 0.0, beam_y - 0.14, 0.0), M::BronzeDark)); // yoke
    v.push((taper(0.17, 0.36, 0.55, beam_y - 0.5), M::Bronze)); // bell body
    v.push((cyl(0.37, 0.07, 0.0, beam_y - 0.78, 0.0), M::BronzeDark)); // lip
    v.push((bx(0.08, 0.2, 0.08, 0.0, beam_y - 0.66, 0.0), M::BronzeDark)); // clapper
    v
}

fn torch_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(0.12, 1.0, 0.12, 0.0, 0.5, 0.0), M::Wood)); // post
    v.push((bx(0.22, 0.12, 0.22, 0.0, 1.04, 0.0), M::BronzeDark)); // bowl
    v.push((flat(Mesh::from(Sphere::new(0.16).mesh().ico(1).unwrap()).scaled_by(Vec3::new(1.0, 1.5, 1.0)).translated_by(Vec3::new(0.0, 1.22, 0.0))), M::Flame));
    v
}

// ── Layout (parametric perimeter) ────────────────────────────────────────────────
fn wall_segments() -> [(f32, f32, f32, f32); 8] {
    let g = GATE_GAP / 2.0;
    let seg_x = HALF_X - g;
    let cx = (HALF_X + g) / 2.0;
    let seg_z = HALF_Z - g;
    let cz = (HALF_Z + g) / 2.0;
    [
        (-cx, -HALF_Z, 0.0, seg_x),
        (cx, -HALF_Z, 0.0, seg_x),
        (-cx, HALF_Z, 0.0, seg_x),
        (cx, HALF_Z, 0.0, seg_x),
        (-HALF_X, -cz, HALF_PI, seg_z),
        (-HALF_X, cz, HALF_PI, seg_z),
        (HALF_X, -cz, HALF_PI, seg_z),
        (HALF_X, cz, HALF_PI, seg_z),
    ]
}
fn gates() -> [(f32, f32, f32); 4] {
    [(0.0, -HALF_Z, 0.0), (0.0, HALF_Z, 0.0), (-HALF_X, 0.0, HALF_PI), (HALF_X, 0.0, HALF_PI)]
}
fn towers() -> [(f32, f32); 4] {
    [(-HALF_X, -HALF_Z), (HALF_X, -HALF_Z), (HALF_X, HALF_Z), (-HALF_X, HALF_Z)]
}
/// Eight houses in two interior rows, flanking the N/S gates, clear of the keep.
fn houses() -> [(f32, f32); 8] {
    let hz = HALF_Z - 3.0;
    [(-13.0, -hz), (-7.0, -hz), (7.0, -hz), (13.0, -hz), (-13.0, hz), (-7.0, hz), (7.0, hz), (13.0, hz)]
}

/// True inside the castle footprint (+ margin) — scatter is cleared here.
pub fn in_footprint(wx: f32, wz: f32) -> bool {
    wx.abs() <= HALF_X + 1.6 && wz.abs() <= HALF_Z + 1.6
}

/// Courtyard half-extents (the wall perimeter) — for placing town villagers inside.
pub fn courtyard_half() -> (f32, f32) {
    (HALF_X, HALF_Z)
}

/// The four gate-gap centres (world XZ) — town villagers spill in/out through these.
pub fn gate_centers() -> [Vec2; 4] {
    [Vec2::new(0.0, -HALF_Z), Vec2::new(0.0, HALF_Z), Vec2::new(-HALF_X, 0.0), Vec2::new(HALF_X, 0.0)]
}

fn snap_cardinal(a: f32) -> f32 {
    (a / HALF_PI).round() * HALF_PI
}
fn face_center(wx: f32, wz: f32) -> f32 {
    snap_cardinal((-wx).atan2(-wz))
}

// ── Build ────────────────────────────────────────────────────────────────────────
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
) {
    let mats = build_mats(images, std_mats);
    // Each part is tagged with the upgrade that reveals it (`CastleKind`); gated parts start
    // hidden so the castle BUILDS UP as you buy (a deliberate change from the old always-full
    // render). `Always` parts (keep core, courtyard, bell, keep-door torches) show from the start.
    let mut spawn = |parts: Vec<(Mesh, M)>, pos: Vec3, rot: f32, scale: Vec3, kind: CastleKind| {
        let vis = if matches!(kind, CastleKind::Always) { Visibility::Inherited } else { Visibility::Hidden };
        for (m, slot) in parts {
            let mesh = meshes.add(bake(m, pos, rot, scale));
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mats.get(slot)),
                Transform::default(),
                vis,
                CastlePart { kind },
                BiomeEntity,
            ));
        }
    };

    // Cobbled courtyard floor (the "murowane patio") just above the grass.
    spawn(
        vec![(slab((HALF_X - 0.3) * 2.0, (HALF_Z - 0.3) * 2.0, 0.02), M::Cobble)],
        Vec3::ZERO,
        0.0,
        Vec3::ONE,
        CastleKind::Always,
    );

    // Keep (centre) — always present.
    spawn(keep_parts(), Vec3::ZERO, 0.0, Vec3::new(0.88, 0.7, 0.88), CastleKind::Always);
    for (x, z, rot, len) in wall_segments() {
        spawn(wall_parts(len), Vec3::new(x, 0.0, z), rot, Vec3::new(1.0, 0.78, 1.0), CastleKind::Walls);
    }
    for (x, z) in towers() {
        spawn(tower_parts(), Vec3::new(x, 0.0, z), 0.0, Vec3::new(0.92, 0.74, 0.92), CastleKind::Towers);
    }
    for (x, z, rot) in gates() {
        spawn(gate_parts(GATE_GAP), Vec3::new(x, 0.0, z), rot, Vec3::new(1.0, 0.8, 1.0), CastleKind::Gate);
    }
    for (i, (x, z)) in houses().into_iter().enumerate() {
        spawn(house_parts(), Vec3::new(x, 0.0, z), face_center(x, z), Vec3::new(0.9, 0.74, 0.9), CastleKind::House(i as u8));
    }
    spawn(bell_parts(), Vec3::new(0.0, 0.0, 6.0), 0.0, Vec3::ONE, CastleKind::Always);

    // Torches: gate torches reveal with the gate; the keep-door pair is always lit.
    for (x, z, rot) in gates() {
        let half = GATE_GAP / 2.0 + 0.5;
        for sx in [-half, half] {
            let local = Quat::from_rotation_y(rot) * Vec3::new(sx, 0.0, 1.0);
            spawn(torch_parts(), Vec3::new(x + local.x, 0.0, z + local.z), 0.0, Vec3::ONE, CastleKind::Gate);
        }
    }
    for sx in [-2.3_f32, 2.3] {
        spawn(torch_parts(), Vec3::new(sx, 0.0, 3.4), 0.0, Vec3::ONE, CastleKind::Always);
    }

    // Chimney smoke above each house.
    let smoke_mat = std_mats.add(StandardMaterial {
        base_color: Color::srgba(0.62, 0.64, 0.67, 0.5),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let puff = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());
    for (hi, (hx, hz)) in houses().into_iter().enumerate() {
        let rot = face_center(hx, hz);
        let local = Quat::from_rotation_y(rot) * Vec3::new((HW / 2.0 - 0.4) * 0.9, 0.0, 0.25 * 0.9);
        let (cx, cz) = (hx + local.x, hz + local.z);
        let base_y = (H_FOUND + HH + 0.74) * 0.74 + 0.2;
        for i in 0..3 {
            commands.spawn((
                Mesh3d(puff.clone()),
                MeshMaterial3d(smoke_mat.clone()),
                Transform::from_translation(Vec3::new(cx, base_y, cz)).with_scale(Vec3::splat(0.01)),
                Smoke { x: cx, z: cz, base_y, phase: i as f32 / 3.0, speed: 0.3 },
                Visibility::Hidden, // follows its house's reveal
                CastlePart { kind: CastleKind::House(hi as u8) },
                BiomeEntity,
            ));
        }
    }

    // Only the always-present keep is solid from the start; the gated structures register their
    // blockers when their upgrade reveals them (see `sync_castle`), so the courtyard is open
    // until you build the walls — no invisible barriers.
    register_keep_blocker();
}

/// Player-body margin so movers stop just shy of a face instead of clipping into it.
const COLLISION_PAD: f32 = 0.12;

/// The keep is solid from the start.
fn register_keep_blocker() {
    let p = COLLISION_PAD;
    crate::blockers::add_box(0.0, 0.0, KEEP_W * 0.88 / 2.0 + p, KEEP_D * 0.88 / 2.0 + p);
}

/// Perimeter wall blockers — one box per segment (registered when Walls is built).
fn register_walls_blockers() {
    let p = COLLISION_PAD;
    for (x, z, rot, len) in wall_segments() {
        let along = len / 2.0;
        let across = WALL_THICK / 2.0 + p;
        let (hw, hd) = if rot.abs() < 0.1 { (along, across) } else { (across, along) };
        crate::blockers::add_box(x, z, hw, hd);
    }
}

/// Corner-tower blockers (registered when Towers is built).
fn register_towers_blockers() {
    let p = COLLISION_PAD;
    for (x, z) in towers() {
        crate::blockers::add_box(x, z, 0.95 + p, 0.95 + p);
    }
}

/// One house's blocker (registered when that district is built).
fn register_house_blocker(i: usize) {
    let p = COLLISION_PAD;
    let (x, z) = houses()[i];
    let (hx, hz) = (HW * 0.9 / 2.0 + p, HD * 0.9 / 2.0 + p);
    let (hw, hd) = if (face_center(x, z).abs() - HALF_PI).abs() < 0.1 { (hz, hx) } else { (hx, hz) };
    crate::blockers::add_box(x, z, hw, hd);
}

// ── Drifting chimney smoke ───────────────────────────────────────────────────────
#[derive(Component)]
struct Smoke {
    x: f32,
    z: f32,
    base_y: f32,
    phase: f32,
    speed: f32,
}

/// Which upgrade reveals a given castle part (the castle builds up instead of starting full).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CastleKind {
    Always,
    Walls,
    Gate,
    Towers,
    House(u8),
}

#[derive(Component)]
struct CastlePart {
    kind: CastleKind,
}

/// Which gated groups have had their (append-only) collision blockers registered, so each is
/// added exactly once the first time it's revealed.
#[derive(Resource, Default)]
struct CastleBuilt {
    walls: bool,
    towers: bool,
    houses: bool,
}

/// Reveal castle parts and lazily register each group's collision the first time it appears.
/// Walls/towers/gate gate on the Defense-branch upgrades (the keep fortifies as you invest);
/// the houses are always-on set-dressing — the keep's own dwellings, registered once at startup.
/// (Food/population now belong to the town city-building layer, so there's no upgrade-gated
/// castle farm or houses any more.)
fn sync_castle(
    def: Res<Defenses>,
    mut built: ResMut<CastleBuilt>,
    mut q: Query<(&CastlePart, &mut Visibility)>,
) {
    for (part, mut vis) in &mut q {
        let show = match part.kind {
            CastleKind::Always | CastleKind::House(_) => true,
            CastleKind::Walls => def.walls,
            CastleKind::Gate => def.walls && def.gate, // a gate without walls would float
            CastleKind::Towers => def.towers,
        };
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
    // Lazy, once-only collision registration on first reveal.
    if def.walls && !built.walls {
        built.walls = true;
        register_walls_blockers();
    }
    if def.towers && !built.towers {
        built.towers = true;
        register_towers_blockers();
    }
    if !built.houses {
        built.houses = true;
        for i in 0..8 {
            register_house_blocker(i);
        }
    }
}

pub struct CastlePlugin;
impl Plugin for CastlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CastleBuilt>().add_systems(Update, (drift_smoke, sync_castle));
    }
}

fn drift_smoke(time: Res<Time>, mut q: Query<(&Smoke, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (s, mut tf) in &mut q {
        let cycle = (t * s.speed + s.phase).rem_euclid(1.0);
        tf.translation.x = s.x + (t * 0.7 + s.phase * 6.0).sin() * 0.18 * cycle;
        tf.translation.z = s.z + (t * 0.6 + s.phase * 6.0).cos() * 0.18 * cycle;
        tf.translation.y = s.base_y + cycle * 1.7;
        let sc = (0.12 + cycle * 0.42) * (1.0 - cycle).max(0.0);
        tf.scale = Vec3::splat(sc.max(0.001));
    }
}
