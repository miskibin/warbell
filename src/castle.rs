//! The central **castle** — a faithful, textured Bevy port of the TS game's fully-upgraded
//! city (`cityModels.tsx` + `House.tsx` + `WarBell.tsx`, layout from `cityPlan.ts`). Built
//! complete: keep, perimeter walls split around four gates, four corner towers, eight
//! houses, a cobbled courtyard, the war bell, banners, torches and chimney smoke.
//!
//! This module owns the **meshes** — the structures are otherwise static here; the gameplay that
//! lives *on* them is wired elsewhere: the keep's HP/repair and the night siege in `siege.rs`, the
//! war-bell ring + War Table in `interaction.rs`, the tower-mounted archers/ballista in
//! `defenses.rs`. At the island centre (world origin, flat grass safe-zone, y=0). Kept at TS
//! *absolute* size (NOT ×1.4) but the PERIMETER is widened (`HALF_X/HALF_Z`) for more courtyard
//! room. Coordinate map: world = `(base − (72,54))`.
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
const PACKED: u32 = 0x6e5436; // bare packed-earth yard ("klepisko") before the walls
const STRAW: u32 = 0xcaa84e; // hay bales / thatched sacks
const HEN: u32 = 0xe7e2d6; // courtyard fowl (off-white plumage)
const H_WALL: u32 = 0xd3b78b;
const H_ROOF: u32 = 0x6b3322;
const H_ROOF2: u32 = 0x6e6256; // weathered grey-brown shingle (house variety)
const THATCH: u32 = 0xb89b4f; // bound-straw roofing (huts, the farm barn)
const IRON: u32 = 0x6a6e72; // arms & armour (racks, blades, helms)
const PARCHMENT: u32 = 0xe6d9b5; // notice/bounty-board paper
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
// `pub(crate)` so the town's producer buildings (`town_meshes`) render with the SAME
// textured materials as the keep, via the shared [`VillageMats`] resource.
#[derive(Clone, Copy)]
pub(crate) enum M {
    Stone,
    DarkStone,
    LightStone,
    HouseStone,
    Plaster,
    Wood,
    Beam,
    Roof,
    HouseRoof,
    /// Weathered grey-brown shingle — the second roof hue that breaks up the dwelling rows.
    HouseRoof2,
    /// Bound straw — hut + barn roofing.
    Thatch,
    /// Dull arms-and-armour metal (rack blades, helms, strongbox bands).
    Iron,
    /// Paper pinned to the notice / bounty boards.
    Parchment,
    Soil,
    Cobble,
    Packed,
    Straw,
    Hen,
    Banner,
    Bronze,
    BronzeDark,
    Crop,
    Slit,
    Gold,
    Window,
    Flame,
}

#[derive(Clone)]
pub(crate) struct Mats {
    h: std::collections::HashMap<u8, Handle<StandardMaterial>>,
}
impl Mats {
    pub(crate) fn get(&self, m: M) -> Handle<StandardMaterial> {
        self.h[&(m as u8)].clone()
    }
}

/// The shared, procedurally-textured village material set (plaster, shingle, timber, stone, soil,
/// cobble, glowing window, …). Built once with the keep and held as a resource so the town's
/// producer buildings + build plots render in the same textured aesthetic as the castle.
#[derive(Resource, Clone)]
pub(crate) struct VillageMats(pub(crate) Mats);

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

/// Thatch — bound straw courses (horizontal rows, stalk streaks, a shadow line per course).
fn tex_thatch(hex: u32) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, -0.04));
    let mut r = Rng(0x7a7 ^ hex);
    let rows = 8usize;
    let rh = TN as f32 / rows as f32;
    for ry in 0..rows {
        let y = ry as f32 * rh;
        cv.rect(0.0, y, TN as f32, rh - 1.5, shade(c, (r.f() - 0.5) * 0.10));
        cv.rect(0.0, y + rh - 1.5, TN as f32, 1.5, shade(c, -0.22)); // course shadow
        for _ in 0..26 {
            let x = r.f() * TN as f32;
            cv.rect(x, y + r.f() * 2.0, 1.0, rh - 2.0 - r.f() * 3.0, shade(c, (r.f() - 0.5) * 0.16)); // stalks
        }
    }
    speckle(&mut cv, &mut r, 300, c);
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

/// Packed earth — the bare courtyard "klepisko" the young settlement stands on before the walls
/// are raised. Trodden flat (no furrows — that's tilled `tex_soil`): big SOFT damp/dry mottling
/// (overlapping low-alpha discs, no hard rectangles — those read as a repeating grid once tiled),
/// faint surviving grass flecks trodden into the dirt, and a few small pebbles. Deliberately low
/// contrast: the worn-ground feel comes from broad value drift, not busy noise.
fn tex_packed(hex: u32) -> Image {
    let c = rgb(hex);
    let mut cv = Canvas::new(shade(c, 0.0));
    let mut r = Rng(0x9e3 ^ hex);
    // Broad soft mottled patches (damp vs sun-dried earth) — large overlapping discs at low
    // alpha so tones flow into each other instead of tiling as visible blocks.
    for _ in 0..46 {
        let x = r.f() * TN as f32;
        let y = r.f() * TN as f32;
        let rad = 7.0 + r.f() * 18.0;
        cv.disc(x, y, rad, shade(c, (r.f() - 0.5) * 0.13), 0.22 + r.f() * 0.16);
    }
    // Trodden-in grass remnants — small muted green-tan flecks where the lawn survives the
    // foot traffic ("przebłyski trawy"), denser as soft discs + a few single-pixel blades.
    let grass = [c[0] * 0.78 + 18.0, c[1] * 0.9 + 42.0, c[2] * 0.75];
    for _ in 0..26 {
        let x = r.f() * TN as f32;
        let y = r.f() * TN as f32;
        cv.disc(x, y, 1.0 + r.f() * 2.2, shade(grass, (r.f() - 0.4) * 0.12), 0.4 + r.f() * 0.3);
    }
    for _ in 0..70 {
        let x = (r.f() * TN as f32).floor();
        let y = (r.f() * TN as f32).floor();
        cv.rect(x, y, 1.0, 1.0 + r.f() * 2.0, shade(grass, (r.f() - 0.3) * 0.15));
    }
    // Sparse trodden-in pebbles (a light stone over its own little shadow) — fewer + smaller
    // than before so they read as grit, not confetti.
    for _ in 0..40 {
        let x = r.f() * TN as f32;
        let y = r.f() * TN as f32;
        let s = 1.0 + r.f() * 1.6;
        cv.rect(x, y + s, s, s * 0.5, shade(c, -0.12)); // contact shadow
        cv.rect(x, y, s, s, shade(c, 0.10 + r.f() * 0.10)); // pebble
    }
    speckle(&mut cv, &mut r, 450, c);
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
    tex(tex_shingle(H_ROOF2), 0.88, M::HouseRoof2);
    tex(tex_thatch(THATCH), 1.0, M::Thatch);
    tex(tex_soil(SOIL), 1.0, M::Soil);

    // Yard grounds (Packed klepisko / Cobble courtyard): fully OPAQUE — `worn_slab` cuts the
    // ragged rim into the geometry itself. (Alpha-Blend rims rendered as a washed-out film at
    // grazing angles; alpha-Mask fought the depth prepass. See `worn_slab`.)
    let mut ground_tex = |img: Image, rough: f32, m: M| {
        let t = images.add(img);
        h.insert(m as u8, std_mats.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(t),
            perceptual_roughness: rough,
            cull_mode: None,
            double_sided: true,
            ..default()
        }));
    };
    ground_tex(tex_cobble(COBBLE), 0.95, M::Cobble);
    ground_tex(tex_packed(PACKED), 1.0, M::Packed);

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
    solid(STRAW, 0.95, 0.0, M::Straw);
    solid(IRON, 0.42, 0.78, M::Iron);
    solid(PARCHMENT, 0.95, 0.0, M::Parchment);
    solid(HEN, 0.9, 0.0, M::Hen);
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
pub(crate) fn bx(w: f32, h: f32, d: f32, x: f32, y: f32, z: f32) -> Mesh {
    let horiz = ((w + d) * 0.5 / TILE).max(0.6);
    let vert = (h / TILE).max(0.6);
    scale_uv(Mesh::from(Cuboid::new(w, h, d)), horiz, vert).translated_by(Vec3::new(x, y, z))
}

pub(crate) fn flat(mut m: Mesh) -> Mesh {
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

pub(crate) fn cyl(r: f32, h: f32, x: f32, y: f32, z: f32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(12).build().translated_by(Vec3::new(x, y, z))
}

/// Tapered 12-gon frustum (bell body): top radius `rt`, bottom `rb`.
pub(crate) fn taper(rt: f32, rb: f32, h: f32, y: f32) -> Mesh {
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
pub(crate) fn gable(span_x: f32, span_z: f32, rise: f32, base_y: f32) -> Mesh {
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

/// Smooth low-frequency field used to fray the yard rim and drift its brightness —
/// a few sin octaves (≈1–3-unit features), cheap and seamless in world space.
fn wear_noise(x: f32, z: f32) -> f32 {
    (x * 1.31 + 1.7).sin() * (z * 1.13 - 2.3).cos()
        + (x * 0.53 + z * 0.61 + 4.5).sin() * 0.6
        + (x * 2.70 - z * 2.20 + 0.8).sin() * 0.25
}

/// The yard ground sheet — a vertex grid whose rim is CUT BY THE NOISE FIELD: grid cells whose
/// wear value falls below the threshold are simply not emitted, so trodden fingers of paving
/// reach into the lawn and grass bites back into the yard along a crisp ragged outline (no
/// hard rectangle edge). Fully opaque — earlier versions faded the rim with vertex ALPHA, but
/// blended rims render as a washed-out translucent film at grazing angles, and alpha-Mask
/// fights the depth prepass (whose cutoff ignores vertex alpha → sky-coloured holes). Geometry
/// can't lie. The interior carries a low-frequency brightness mottle in the vertex colour so
/// the tiling texture stops reading as a grid. `noise_amp` scales how ragged the rim is;
/// `mottle` the interior value drift.
fn worn_slab(w: f32, d: f32, y: f32, fray: f32, noise_amp: f32, mottle: f32) -> Mesh {
    let hw = w / 2.0 + fray;
    let hd = d / 2.0 + fray;
    let step = 0.5_f32;
    let nx = (2.0 * hw / step).ceil() as usize;
    let nz = (2.0 * hd / step).ceil() as usize;
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity((nx + 1) * (nz + 1));
    let mut nrm: Vec<[f32; 3]> = Vec::with_capacity(pos.capacity());
    let mut uv: Vec<[f32; 2]> = Vec::with_capacity(pos.capacity());
    let mut col: Vec<[f32; 4]> = Vec::with_capacity(pos.capacity());
    let mut wear: Vec<f32> = Vec::with_capacity(pos.capacity());
    for iz in 0..=nz {
        for ix in 0..=nx {
            // Spread the grid exactly across [-hw, hw] (no clamped duplicate columns).
            let x = -hw + 2.0 * hw * ix as f32 / nx as f32;
            let z = -hd + 2.0 * hd * iz as f32 / nz as f32;
            // Distance inside the OUTER rect; the wear band spans ~2×fray of it.
            let d_in = (hw - x.abs()).min(hd - z.abs());
            let t = d_in / (fray * 2.0) + wear_noise(x, z) * noise_amp - 0.5 * noise_amp;
            // Low-frequency brightness drift breaks the texture's tile repetition.
            let b = 1.0
                + mottle * (x * 0.37 + 1.1).sin() * (z * 0.41 - 0.6).cos()
                + mottle * 0.7 * (x * 0.13 - z * 0.17 + 2.0).sin();
            pos.push([x, y, z]);
            nrm.push([0.0, 1.0, 0.0]);
            uv.push([x / TILE, z / TILE]);
            col.push([b, b, b, 1.0]);
            wear.push(t);
        }
    }
    let mut idx: Vec<u32> = Vec::with_capacity(nx * nz * 6);
    let row = (nx + 1) as u32;
    for iz in 0..nz as u32 {
        for ix in 0..nx as u32 {
            let i0 = iz * row + ix;
            let (i1, i2, i3) = (i0 + 1, i0 + row + 1, i0 + row);
            // Keep the cell only where the worn surface survives (mean corner wear above the
            // cut) — the rim outline follows the noise, half-cell crisp.
            let mean = (wear[i0 as usize] + wear[i1 as usize] + wear[i2 as usize] + wear[i3 as usize]) / 4.0;
            if mean < 0.5 {
                continue;
            }
            // Same up-facing winding as `slab`.
            idx.extend_from_slice(&[i0, i2, i1, i0, i3, i2]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(Indices::U32(idx));
    m
}

/// A flat upward-facing slab quad at height `y` (cobble courtyard), UV tiled.
#[allow(dead_code)] // superseded by `worn_slab` for the yard; kept for flat textured sheets
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

pub(crate) fn bake(m: Mesh, pos: Vec3, rot: f32, scale: Vec3) -> Mesh {
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
    // Stepped base: a broad ground footing skirt under the foundation course — depth at the
    // waterline without reflowing any of the body geometry above it.
    v.push((bx(KEEP_W + 0.9, 0.14, KEEP_D + 0.9, 0.0, 0.07, 0.0), M::DarkStone));
    v.push((bx(KEEP_W + 0.5, KEEP_FOUND, KEEP_D + 0.5, 0.0, KEEP_FOUND / 2.0, 0.0), M::DarkStone));
    v.push((bx(KEEP_W, KEEP_H, KEEP_D, 0.0, KEEP_FOUND + KEEP_H / 2.0, 0.0), M::Stone));
    // Light-stone belt course banding the body (horizontal articulation).
    v.push((bx(KEEP_W + 0.18, 0.16, KEEP_D + 0.18, 0.0, KEEP_FOUND + KEEP_H * 0.52, 0.0), M::LightStone));
    // Corner buttresses.
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.5, KEEP_H + 0.1, 0.5, sx * (KEEP_W / 2.0 - 0.15), KEEP_FOUND + (KEEP_H + 0.1) / 2.0, sz * (KEEP_D / 2.0 - 0.15)), M::DarkStone));
        }
    }
    // Corbelled battlement lip just under the merlons (overhang shadow line, like the walls).
    v.push((bx(KEEP_W + 0.22, 0.16, KEEP_D + 0.22, 0.0, roof_y - 0.04, 0.0), M::DarkStone));
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
    // Central tower + cornice + roof + finial + a fluttering pennant.
    v.push((bx(2.0, 1.3, 2.0, 0.0, roof_y + 0.65, 0.0), M::LightStone));
    v.push((bx(2.16, 0.14, 2.16, 0.0, roof_y + 1.24, 0.0), M::DarkStone)); // cornice under the roof
    v.push((pyramid(1.4, 0.95, roof_y + 1.3), M::Roof));
    v.push((Sphere::new(0.18).mesh().ico(2).unwrap().translated_by(Vec3::new(0.0, roof_y + 2.2, 0.0)), M::Gold));
    // Spire pole — the pennant itself is a fluttering cloth entity (banner.rs), spawned in
    // `build` so the merged keep geometry keeps only the pole.
    v.push((cyl(0.03, 0.6, 0.0, roof_y + 2.6, 0.0), M::Beam));
    // Threshold steps + door + arch beam + flanking banners.
    v.push((bx(1.8, 0.1, 0.34, 0.0, 0.05, KEEP_D / 2.0 + 0.32), M::HouseStone)); // lower step
    v.push((bx(1.5, 0.1, 0.22, 0.0, 0.13, KEEP_D / 2.0 + 0.22), M::HouseStone)); // upper step
    v.push((bx(1.4, 1.6, 0.12, 0.0, KEEP_FOUND + 0.85, KEEP_D / 2.0 + 0.02), M::Wood));
    v.push((bx(1.7, 0.3, 0.2, 0.0, KEEP_FOUND + 1.75, KEEP_D / 2.0 + 0.05), M::Beam));
    for sx in [-1.45_f32, 1.45] {
        v.push((bx(0.6, 1.5, 0.04, sx, KEEP_FOUND + 1.3, KEEP_D / 2.0 + 0.08), M::Banner));
    }
    // Arrow slits — front + back pairs, one centred on each side.
    for sx in [-2.4_f32, 2.4] {
        v.push((bx(0.16, 0.7, 0.06, sx, KEEP_FOUND + 1.1, KEEP_D / 2.0 + 0.02), M::Slit));
        v.push((bx(0.16, 0.7, 0.06, sx, KEEP_FOUND + 1.1, -KEEP_D / 2.0 - 0.02), M::Slit));
    }
    for sx in [-1.0_f32, 1.0] {
        v.push((bx(0.06, 0.7, 0.16, sx * (KEEP_W / 2.0 + 0.02), KEEP_FOUND + 1.1, 0.0), M::Slit));
    }
    // Warm glowing windows on the side faces (the keep reads as lived-in at dusk).
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.4_f32, 1.4] {
            v.push((bx(0.05, 0.5, 0.4, sx * (KEEP_W / 2.0 + 0.01), KEEP_FOUND + 0.6, sz), M::Window));
        }
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
    v.push((bx(2.05, 0.32, 2.05, 0.0, 0.16, 0.0), M::DarkStone)); // base plinth
    v.push((bx(1.8, TOWER_H, 1.8, 0.0, TOWER_H / 2.0, 0.0), M::Stone)); // shaft
    v.push((bx(1.94, 0.12, 1.94, 0.0, TOWER_H * 0.55, 0.0), M::LightStone)); // string course
    v.push((bx(2.1, 0.4, 2.1, 0.0, TOWER_H + 0.1, 0.0), M::DarkStone)); // corbelled battlement
    // Full crenellation: four corner merlons + four edge-midpoint merlons (gapped crenels between).
    let my = TOWER_H + 0.45;
    for (mx, mz) in [
        (-0.85_f32, -0.85_f32), (0.85, -0.85), (0.85, 0.85), (-0.85, 0.85),
        (0.0, -0.92), (0.92, 0.0), (0.0, 0.92), (-0.92, 0.0),
    ] {
        v.push((bx(0.4, 0.42, 0.4, mx, my, mz), M::DarkStone));
    }
    v.push((pyramid(1.5, 1.4, TOWER_H + 0.3), M::Roof));
    // Arrow slits on all four faces.
    for (ax, az, rot) in [(0.0, 0.91, 0.0), (0.0, -0.91, 0.0), (0.91, 0.0, HALF_PI), (-0.91, 0.0, HALF_PI)] {
        v.push((bake(bx(0.16, 0.8, 0.06, 0.0, 0.0, 0.0), Vec3::new(ax, TOWER_H * 0.5, az), rot, Vec3::ONE), M::Slit));
    }
    // Flag pole — the flag itself is a fluttering cloth entity (banner.rs), spawned in `build`.
    v.push((cyl(0.04, 1.0, 0.0, TOWER_H + 2.0, 0.0), M::Beam));
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

// House (the standard cottage's dims — other archetypes carry their own).
const HW: f32 = 2.6;
const HH: f32 = 1.15;
const HD: f32 = 2.0;
const H_FOUND: f32 = 0.18;

/// Four dwelling silhouettes spread across the twelve slots (3 huts, 4 cottages, 3 townhouses,
/// 2 longhouses) so the rows read as a grown village instead of barracks. All archetypes keep
/// the shared conventions: local origin, base at y=0, FRONT facing +Z, spawned at the same
/// (0.9, 0.74, 0.9) scale — the accessors below feed the per-slot blocker / shutter / smoke
/// wiring that used to assume one cottage.
#[derive(Clone, Copy, PartialEq)]
enum HouseStyle {
    Hut,
    Cottage,
    Townhouse,
    Longhouse,
}

/// Style per dwelling slot (indexes `houses()`); reveal order is unchanged, so even the early
/// settlement mixes a hut, cottages and a townhouse rather than clones.
const HOUSE_STYLES: [HouseStyle; 12] = [
    HouseStyle::Hut, HouseStyle::Cottage, HouseStyle::Townhouse, HouseStyle::Cottage, // N row
    HouseStyle::Cottage, HouseStyle::Longhouse, HouseStyle::Cottage, HouseStyle::Hut, // S row
    HouseStyle::Townhouse, HouseStyle::Hut, HouseStyle::Longhouse, HouseStyle::Townhouse, // E/W
];

/// Roof material per slot — huts thatch; the rest alternate warm vs weathered shingle.
fn house_roof(i: usize) -> M {
    match HOUSE_STYLES[i] {
        HouseStyle::Hut => M::Thatch,
        _ => {
            if i % 2 == 0 {
                M::HouseRoof
            } else {
                M::HouseRoof2
            }
        }
    }
}

/// Local (pre-scale) footprint (w, d) — feeds the per-slot collision blocker.
fn house_dims(style: HouseStyle) -> (f32, f32) {
    match style {
        HouseStyle::Hut => (2.0, 1.7),
        HouseStyle::Cottage => (HW, HD),
        HouseStyle::Townhouse => (2.3, 1.9),
        HouseStyle::Longhouse => (3.6, 1.8),
    }
}

/// Local (pre-scale) front-window centre — where the curfew shutters hang.
fn house_window(style: HouseStyle) -> Vec3 {
    match style {
        HouseStyle::Hut => Vec3::new(0.45, 0.76, 0.85),
        HouseStyle::Cottage => Vec3::new(0.5, H_FOUND + 0.9, HD / 2.0),
        HouseStyle::Townhouse => Vec3::new(0.45, 1.8, 1.03), // the upstairs pane
        HouseStyle::Longhouse => Vec3::new(1.05, 0.78, 0.9),
    }
}

/// Local (pre-scale) chimney spot + stack-top height, for the smoke column. Huts vent
/// through the thatch — no chimney, no smoke.
fn house_chimney(style: HouseStyle) -> Option<(Vec2, f32)> {
    match style {
        HouseStyle::Hut => None,
        HouseStyle::Cottage => Some((Vec2::new(HW / 2.0 - 0.4, 0.25), H_FOUND + HH + 0.74)),
        HouseStyle::Townhouse => Some((Vec2::new(0.75, 0.2), 2.98)),
        HouseStyle::Longhouse => Some((Vec2::new(-1.2, 0.25), 1.94)),
    }
}

fn house_parts_for(i: usize) -> Vec<(Mesh, M)> {
    let roof = house_roof(i);
    match HOUSE_STYLES[i] {
        HouseStyle::Hut => hut_parts(),
        HouseStyle::Cottage => house_parts(roof),
        HouseStyle::Townhouse => townhouse_parts(roof),
        HouseStyle::Longhouse => longhouse_parts(roof),
    }
}

/// The textured village cottage (plaster walls, half-timbered posts, shingle gable, glowing
/// window, stone chimney) — the baseline dwelling the other archetypes riff on.
fn house_parts(roof: M) -> Vec<(Mesh, M)> {
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
    v.push((gable(HW + 0.3, HD + 0.3, 0.6, wall_top), roof));
    v
}

/// The smallest dwelling: one plaster room under a deep-eaved thatch gable.
fn hut_parts() -> Vec<(Mesh, M)> {
    let (w, h, d, f) = (2.0, 0.95, 1.7, 0.14);
    let top = f + h;
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(w + 0.16, f, d + 0.16, 0.0, f / 2.0, 0.0), M::HouseStone));
    v.push((bx(w, h, d, 0.0, f + h / 2.0, 0.0), M::Plaster));
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.14, h, 0.14, sx * (w / 2.0), f + h / 2.0, sz * (d / 2.0)), M::Beam));
        }
    }
    v.push((bx(0.42, 0.78, 0.06, -0.45, f + 0.42, d / 2.0 + 0.02), M::Wood)); // door
    v.push((bx(0.52, 0.09, 0.08, -0.45, f + 0.86, d / 2.0 + 0.03), M::Beam));
    v.push((bx(0.42, 0.36, 0.04, 0.45, 0.76, d / 2.0 + 0.02), M::Window));
    v.push((bx(0.5, 0.06, 0.06, 0.45, 0.98, d / 2.0 + 0.03), M::Beam));
    v.push((gable(w + 0.55, d + 0.6, 0.8, top), M::Thatch)); // deep eaves
    v
}

/// Two-story jettied townhouse: stone ground floor, timber-framed plaster upper that juts
/// past it, glowing pane upstairs, tall gable. The vertical accent in each row.
fn townhouse_parts(roof: M) -> Vec<(Mesh, M)> {
    let (w, d, f) = (2.3, 1.9, 0.18);
    let h1 = 1.0; // stone ground floor
    let h2 = 0.85; // jettied upper floor
    let up0 = f + h1 + 0.12; // upper-floor base (above the jetty band)
    let top = up0 + h2;
    let (uw, ud) = (w + 0.16, d + 0.16); // the jetty overhang
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(w + 0.2, f, d + 0.2, 0.0, f / 2.0, 0.0), M::HouseStone));
    v.push((bx(w, h1, d, 0.0, f + h1 / 2.0, 0.0), M::HouseStone));
    v.push((bx(uw + 0.08, 0.12, ud + 0.08, 0.0, f + h1 + 0.06, 0.0), M::Beam)); // jetty band
    v.push((bx(uw, h2, ud, 0.0, up0 + h2 / 2.0, 0.0), M::Plaster));
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.14, h2, 0.14, sx * (uw / 2.0), up0 + h2 / 2.0, sz * (ud / 2.0)), M::Beam));
        }
    }
    v.push((bx(0.46, 0.95, 0.06, -0.5, f + 0.5, d / 2.0 + 0.02), M::Wood)); // door
    v.push((bx(0.56, 0.1, 0.08, -0.5, f + 1.0, d / 2.0 + 0.03), M::Beam));
    v.push((bx(0.38, 0.3, 0.04, 0.55, f + 0.62, d / 2.0 + 0.02), M::Slit)); // dark ground pane
    v.push((bx(0.42, 0.42, 0.04, 0.45, 1.8, ud / 2.0 + 0.02), M::Window)); // upstairs glow
    v.push((bx(0.52, 0.06, 0.06, 0.45, 2.05, ud / 2.0 + 0.03), M::Beam));
    v.push((bx(0.26, 0.9, 0.26, 0.75, top + 0.4, 0.2), M::HouseStone)); // chimney
    v.push((bx(0.32, 0.08, 0.32, 0.75, top + 0.83, 0.2), M::Beam));
    v.push((gable(uw + 0.3, ud + 0.34, 0.72, top), roof));
    v
}

/// A long single-ridge hall: the widest dwelling, mid-posts pacing the long wall, two glowing
/// windows flanking a centre door, gable-end chimney, firewood stacked against the end wall.
fn longhouse_parts(roof: M) -> Vec<(Mesh, M)> {
    let (w, h, d, f) = (3.6, 1.0, 1.8, 0.16);
    let top = f + h;
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(w + 0.2, f, d + 0.2, 0.0, f / 2.0, 0.0), M::HouseStone));
    v.push((bx(w, h, d, 0.0, f + h / 2.0, 0.0), M::Plaster));
    // Corner + mid posts give the long wall its half-timber rhythm.
    for sx in [-w / 2.0, -w / 6.0, w / 6.0, w / 2.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.15, h, 0.15, sx, f + h / 2.0, sz * (d / 2.0)), M::Beam));
        }
    }
    v.push((bx(0.5, 0.92, 0.06, 0.0, f + 0.48, d / 2.0 + 0.02), M::Wood)); // centre door
    v.push((bx(0.6, 0.1, 0.08, 0.0, f + 0.98, d / 2.0 + 0.03), M::Beam));
    for sx in [-1.05_f32, 1.05] {
        v.push((bx(0.42, 0.4, 0.04, sx, 0.78, d / 2.0 + 0.02), M::Window));
        v.push((bx(0.5, 0.06, 0.06, sx, 1.02, d / 2.0 + 0.03), M::Beam));
    }
    v.push((bx(0.28, 0.75, 0.28, -1.2, top + 0.34, 0.25), M::HouseStone)); // gable-end chimney
    v.push((bx(0.34, 0.08, 0.34, -1.2, top + 0.74, 0.25), M::Beam));
    v.push((gable(w + 0.4, d + 0.5, 0.62, top), roof));
    // Firewood against the −X gable wall (cylinder axis turned to lie along Z).
    for (y, kz) in [(0.11_f32, -0.13_f32), (0.11, 0.13), (0.3, 0.0)] {
        v.push((
            Cylinder::new(0.1, 0.7)
                .mesh()
                .resolution(8)
                .build()
                .rotated_by(Quat::from_rotation_x(HALF_PI))
                .translated_by(Vec3::new(-w / 2.0 - 0.24, y, kz)),
            M::Wood,
        ));
    }
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

// ── Rustic yard clutter ──────────────────────────────────────────────────────────
// Small work-yard props that dress the courtyard corners. Tagged `Always` — they used to be
// `PreWalls`, which meant buying the Palisade Walls DELETED every sign of life and left bare
// paving; now the settlement's working corners stay through the whole game. Built at local
// origin, base at y=0; the `build()` spawn closure bakes each cluster to its courtyard spot.
// Decorative only — they register NO collision (blockers are append-only and can't be cleanly
// removed), and they sit at the courtyard corners (±10, ±6), clear of the keep, the bell, the
// gates and every house slot. (More set dressing — and the upgrade-bought set pieces — live in
// `castle_decor.rs`.)

/// A log lying along the X axis (for stacked woodpiles / windlass rollers).
pub(crate) fn log_x(r: f32, len: f32, y: f32, z: f32) -> Mesh {
    Cylinder::new(r, len)
        .mesh()
        .resolution(8)
        .build()
        .rotated_by(Quat::from_rotation_z(HALF_PI))
        .translated_by(Vec3::new(0.0, y, z))
}

/// Chopping block with a buried axe + a small stacked woodpile.
fn wood_yard_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((cyl(0.3, 0.5, 0.0, 0.25, 0.0), M::Wood)); // chopping block
    v.push((bx(0.05, 0.5, 0.05, 0.08, 0.55, 0.0), M::Beam)); // axe handle
    v.push((bx(0.2, 0.12, 0.06, 0.08, 0.78, 0.0), M::DarkStone)); // axe head
    // Pyramid-stacked logs beside the block (3 / 2 / 1).
    for (row, &(y, n)) in [(0.12_f32, 3i32), (0.33, 2), (0.54, 1)].iter().enumerate() {
        for k in 0..n {
            let z = -0.9 + (k as f32 - (n - 1) as f32 / 2.0) * 0.26;
            let m = if (row + k as usize) % 2 == 0 { M::Wood } else { M::Beam };
            v.push((log_x(0.11, 1.5, y, z), m));
        }
    }
    v
}

/// Two stacked hay bales (roped) with a couple of grain sacks at the base.
fn hay_corner_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(0.9, 0.55, 0.55, 0.0, 0.28, 0.0), M::Straw));
    v.push((bx(0.9, 0.55, 0.55, 0.0, 0.83, 0.05), M::Straw));
    for x in [-0.25_f32, 0.25] {
        v.push((bx(0.92, 0.05, 0.58, x, 0.28, 0.0), M::Beam)); // binding rope
        v.push((bx(0.92, 0.05, 0.58, x, 0.83, 0.05), M::Beam));
    }
    for (sx, sz) in [(0.72_f32, 0.24_f32), (0.8, -0.22)] {
        v.push((taper(0.1, 0.2, 0.42, 0.21).translated_by(Vec3::new(sx, 0.0, sz)), M::Plaster)); // burlap sack
    }
    v
}

/// A hand-cart with side rails + two barrels (hooped) parked beside it.
fn cart_corner_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(1.3, 0.12, 0.7, 0.0, 0.42, 0.0), M::Beam)); // bed
    for sz in [-0.34_f32, 0.34] {
        v.push((bx(1.3, 0.18, 0.06, 0.0, 0.55, sz), M::Wood)); // side rail
    }
    // Wheels (discs standing in the X/Y plane → cylinder axis along Z).
    for sz in [-0.4_f32, 0.4] {
        v.push((
            Cylinder::new(0.32, 0.08)
                .mesh()
                .resolution(10)
                .build()
                .rotated_by(Quat::from_rotation_x(HALF_PI))
                .translated_by(Vec3::new(-0.2, 0.32, sz)),
            M::Wood,
        ));
    }
    for sz in [-0.28_f32, 0.28] {
        v.push((bx(0.9, 0.06, 0.06, 0.85, 0.62, sz), M::Beam)); // handle shaft
    }
    for (cx, cz) in [(0.95_f32, -0.6_f32), (1.2, -0.35)] {
        v.push((taper(0.26, 0.3, 0.62, 0.31).translated_by(Vec3::new(cx, 0.0, cz)), M::Wood)); // barrel
        v.push((cyl(0.31, 0.05, cx, 0.16, cz), M::Beam)); // lower hoop
        v.push((cyl(0.31, 0.05, cx, 0.48, cz), M::Beam)); // upper hoop
    }
    v
}

/// A little roofed draw-well — stone curb, posts, windlass roller, a bucket on the rim.
fn well_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((cyl(0.55, 0.5, 0.0, 0.25, 0.0), M::HouseStone)); // curb
    v.push((cyl(0.42, 0.04, 0.0, 0.46, 0.0), M::Slit)); // dark water surface (recessed below the rim)
    for sx in [-0.5_f32, 0.5] {
        v.push((bx(0.1, 1.1, 0.1, sx, 0.55, 0.0), M::Beam)); // post
    }
    v.push((log_x(0.07, 1.0, 1.05, 0.0), M::Wood)); // windlass roller
    v.push((gable(1.4, 0.7, 0.35, 1.15), M::HouseRoof)); // little roof
    v.push((taper(0.13, 0.16, 0.26, 0.13).translated_by(Vec3::new(0.0, 0.0, 0.62)), M::Wood)); // bucket on the rim
    v
}

/// A low-poly courtyard hen (off-white body, gold beak, red comb). Parts share local origin; the
/// whole bird is parented under a moving root and bobbed/pecked by [`peck_hens`].
fn hen_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((flat(Sphere::new(0.12).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.3, 0.95, 1.0)).translated_by(Vec3::new(0.0, 0.12, 0.0))), M::Hen)); // body
    v.push((flat(Sphere::new(0.07).mesh().ico(1).unwrap().translated_by(Vec3::new(0.13, 0.23, 0.0))), M::Hen)); // head
    v.push((Cone { radius: 0.03, height: 0.08 }.mesh().build().rotated_by(Quat::from_rotation_z(-HALF_PI)).translated_by(Vec3::new(0.22, 0.22, 0.0)), M::Gold)); // beak
    v.push((bx(0.03, 0.06, 0.05, 0.12, 0.3, 0.0), M::Roof)); // comb
    v.push((flat(bx(0.16, 0.12, 0.02, 0.0, 0.0, 0.0).rotated_by(Quat::from_rotation_z(0.5)).translated_by(Vec3::new(-0.13, 0.2, 0.0))), M::Hen)); // cocked tail
    v
}

// ── Layout (parametric perimeter) ────────────────────────────────────────────────

/// The worn route network `(cx, cz, w, d)`: [0] the keep/bell/muster plaza, then the four
/// gate paths. Drawn as packed earth pre-walls and cobble after (see `build`); also used to
/// keep grass sprigs off the paving.
const PATH_RECTS: [(f32, f32, f32, f32); 5] = [
    (0.0, 2.0, 13.0, 13.0),   // plaza: keep + bell + muster yard
    (0.0, -8.25, 3.4, 7.5),   // north gate → plaza
    (0.0, 10.25, 3.4, 3.5),   // south gate → plaza
    (-11.75, 0.0, 10.5, 3.4), // west gate → plaza
    (11.75, 0.0, 10.5, 3.4),  // east gate → plaza
];

/// Is `(wx, wz)` on (or hard against) the plaza/path paving? Used to keep grass off the routes.
fn on_paving(wx: f32, wz: f32) -> bool {
    PATH_RECTS
        .iter()
        .any(|(cx, cz, w, d)| (wx - cx).abs() < w / 2.0 + 0.5 && (wz - cz).abs() < d / 2.0 + 0.5)
}

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
/// Twelve dwelling slots: two interior N/S rows (8) flanking the N/S gates, plus four E/W
/// flanks tucked beside the side gates — all clear of the keep, the bell, and the gate lanes.
/// The first `town.houses` reveal in order, so the early settlement uses the familiar 8 and the
/// last four are room to grow into.
fn houses() -> [(f32, f32); 12] {
    let hz = HALF_Z - 3.0; // 9.0 — the two long N/S rows
    let ex = HALF_X - 3.0; // 14.0 — the E/W flanks, off the side-gate lanes (z = ±4.5)
    [
        (-13.0, -hz), (-7.0, -hz), (7.0, -hz), (13.0, -hz),
        (-13.0, hz), (-7.0, hz), (7.0, hz), (13.0, hz),
        (-ex, -4.5), (-ex, 4.5), (ex, -4.5), (ex, 4.5),
    ]
}

/// World-XZ of the dwelling slot the NEXT house will occupy (`houses()[built]`), or `None`
/// once all twelve stand. The on-site "Raise house" interaction + its pad anchor here, so a
/// new home always rises exactly where the player chose to build it (`sync_castle` reveals
/// the mesh at this same slot when `town.houses` increments).
pub(crate) fn next_house_site(built: u32) -> Option<Vec2> {
    houses().get(built as usize).map(|&(x, z)| Vec2::new(x, z))
}

/// True inside the castle footprint (+ margin) — scatter is cleared here.
pub fn in_footprint(wx: f32, wz: f32) -> bool {
    wx.abs() <= HALF_X + 1.6 && wz.abs() <= HALF_Z + 1.6
}

/// Courtyard half-extents (the wall perimeter) — for placing town villagers inside.
pub fn courtyard_half() -> (f32, f32) {
    (HALF_X, HALF_Z)
}

/// True strictly INSIDE the wall ring (wall thickness excluded). Siege rule: an invader only
/// batters the keep from in here — outside, the walls shield it, so the wall ring is a real
/// obstacle (thread a gate or do nothing), never a spot to chip the keep from.
pub fn in_courtyard(wx: f32, wz: f32) -> bool {
    wx.abs() < HALF_X - WALL_THICK && wz.abs() < HALF_Z - WALL_THICK
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
) -> Mats {
    let mats = build_mats(images, std_mats);
    // Share the textured material set with the town (producer buildings + plots render textured).
    // Returned to the caller too, so plot seeding in the same build pass (before this deferred
    // `insert_resource` lands) can use it directly.
    commands.insert_resource(VillageMats(mats.clone()));
    // Each part is tagged with the upgrade that reveals it (`CastleKind`); gated parts start
    // hidden so the castle BUILDS UP as you buy (a deliberate change from the old always-full
    // render). `Always` parts (keep core, courtyard, bell, keep-door torches) show from the start.
    let mut spawn = |parts: Vec<(Mesh, M)>, pos: Vec3, rot: f32, scale: Vec3, kind: CastleKind| {
        // `Always` and `PreWalls` are present on a fresh, wall-less keep; the rest start hidden and
        // `sync_castle` reveals them as you buy upgrades (and flips PreWalls off once Walls go up).
        let vis = if matches!(kind, CastleKind::Always | CastleKind::PreWalls) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for (m, slot) in parts {
            let mesh = meshes.add(bake(m, pos, rot, scale));
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mats.get(slot)),
                Transform::default(),
                vis,
                CastlePart { kind },
                // Authored position — `sync_castle` aims the construction dust burst with it
                // (the mesh is baked in world space, the Transform stays identity).
                crate::build_fx::RevealAt(pos),
                BiomeEntity,
            ));
        }
    };

    // Ground: NOT one big sheet — that read as a repeating carpet wall-to-wall. Instead a worn
    // route network: a plaza under the keep + bell + muster yard, and four paths running to the
    // gates, with the courtyard lawn showing everywhere between. Day one the routes are trodden
    // packed earth (the feet of the settlement wrote them); raising the Palisade Walls paves the
    // SAME network in cobble — paving as progression, never a full-courtyard slab again. Both
    // variants are `worn_slab`s (rims alpha-dissolve into the lawn); paths sit 1.5cm above the
    // plaza sheet so their frayed overlaps at the junctions can't z-fight.
    for (i, (px, pz, w, d)) in PATH_RECTS.into_iter().enumerate() {
        let y = if i == 0 { 0.02 } else { 0.035 };
        spawn(
            vec![(worn_slab(w, d, y, 1.1, 0.18, 0.07), M::Packed)],
            Vec3::new(px, 0.0, pz),
            0.0,
            Vec3::ONE,
            CastleKind::PreWalls,
        );
        spawn(
            vec![(worn_slab(w, d, y + 0.005, 0.7, 0.12, 0.04), M::Cobble)],
            Vec3::new(px, 0.0, pz),
            0.0,
            Vec3::ONE,
            CastleKind::Walls,
        );
    }

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
        spawn(house_parts_for(i), Vec3::new(x, 0.0, z), face_center(x, z), Vec3::new(0.9, 0.74, 0.9), CastleKind::House(i as u8));
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

    // The settlement's working corners: woodpile, hay store, cart, draw-well. Permanent (`Always`)
    // — the courtyard never goes blank, walls or no walls. Tucked into the four courtyard corners
    // (±10, ±6), clear of the gates, the bell, the paths and every house slot.
    spawn(wood_yard_parts(), Vec3::new(-10.0, 0.0, 6.0), 0.6, Vec3::ONE, CastleKind::Always);
    spawn(hay_corner_parts(), Vec3::new(10.0, 0.0, 6.0), -0.5, Vec3::ONE, CastleKind::Always);
    spawn(cart_corner_parts(), Vec3::new(-10.0, 0.0, -6.0), 2.3, Vec3::ONE, CastleKind::Always);
    spawn(well_parts(), Vec3::new(10.0, 0.0, -6.0), 0.4, Vec3::ONE, CastleKind::Always);

    // Pooled flicker-lights at each torch flame — the `torch_parts` geometry is emissive-only, so
    // the gates/keep door read flat in the dark without these. Tagged with the SAME `CastlePart`
    // as the torch they sit on, so `sync_castle` lights gate torches only once the gate is built
    // and a Hidden light casts nothing. Spawned here (not in the torch loop above) because the
    // `spawn` closure holds `&mut commands` until its last call just above. Flame local y = 1.22.
    for (x, z, rot) in gates() {
        let half = GATE_GAP / 2.0 + 0.5;
        for (k, sx) in [-half, half].into_iter().enumerate() {
            let local = Quat::from_rotation_y(rot) * Vec3::new(sx, 0.0, 1.0);
            commands.spawn((
                Transform::from_translation(Vec3::new(x + local.x, 1.25, z + local.z)),
                Visibility::Hidden,
                CastlePart { kind: CastleKind::Gate },
                BiomeEntity,
                crate::firelight::torch_light(x * 0.7 + z * 0.31 + k as f32 * 2.1),
            ));
        }
    }
    for sx in [-2.3_f32, 2.3] {
        commands.spawn((
            Transform::from_translation(Vec3::new(sx, 1.25, 3.4)),
            Visibility::Inherited, // keep-door torches are always lit
            CastlePart { kind: CastleKind::Always },
            BiomeEntity,
            crate::firelight::torch_light(sx * 1.3),
        ));
    }

    // Curfew shutters over each house's front window — a wooden pair that swings shut as night
    // falls (`shutters::drive_shutters`), so the lit town visibly buttons up before the siege (and
    // a closed leaf hides the glowing pane, reading as "lamp out"). Built here, after the `spawn`
    // closure releases `commands`; tagged per-house so each pair reveals with its dwelling. The two
    // mirrored leaf meshes are baked at world size and shared across all houses.
    let shutter_right = meshes.add(bx(0.20, 0.36, 0.03, -0.10, 0.0, 0.0));
    let shutter_left = meshes.add(bx(0.20, 0.36, 0.03, 0.10, 0.0, 0.0));
    for (i, (hx, hz)) in houses().into_iter().enumerate() {
        crate::shutters::spawn_house_shutters(
            commands,
            &shutter_right,
            &shutter_left,
            mats.get(M::Wood),
            hx,
            hz,
            face_center(hx, hz),
            CastleKind::House(i as u8),
            house_window(HOUSE_STYLES[i]),
        );
    }

    // Cloth flags (banner.rs) on the poles the merged geometry left bare: the keep spire's
    // pennant (always shown) and each wall tower's flag (revealed with the Towers upgrade —
    // `sync_castle` drives their visibility via the same `CastlePart` tag as the towers).
    // Attach heights are the static flags' old local positions × the structures' Y scales.
    let keep_flag = crate::banner::spawn_flag(
        commands, meshes, std_mats,
        Vec3::new(0.0, 4.88 * 0.7, 0.0), 0.85, 0.42, BANNER, Some(0xd9b34a),
    );
    commands
        .entity(keep_flag)
        .insert((CastlePart { kind: CastleKind::Always }, BiomeEntity));
    for (x, z) in towers() {
        let flag = crate::banner::spawn_flag(
            commands, meshes, std_mats,
            Vec3::new(x, 4.75 * 0.74, z), 0.68, 0.36, BANNER, None,
        );
        commands.entity(flag).insert((
            CastlePart { kind: CastleKind::Towers },
            Visibility::Hidden,
            BiomeEntity,
        ));
    }

    // Grass tufts + clover round the courtyard rim and scattered through the lawn (off the
    // paving) — the courtyard floor is living ground now, not a stamped sheet. Permanent.
    {
        let cover_mat = std_mats.add(StandardMaterial {
            base_color: Color::WHITE, // vertex colour carries the hue (groundcover contract)
            perceptual_roughness: 0.62,
            reflectance: 0.5,
            ..default()
        });
        use crate::groundcover as gc;
        let tufts: Vec<Handle<Mesh>> =
            (0..gc::NUM_GRASS_VARIANTS).map(|v| meshes.add(gc::build_grass_tuft_mesh(v))).collect();
        let clovers: Vec<Handle<Mesh>> =
            (0..gc::NUM_CLOVER_VARIANTS).map(|v| meshes.add(gc::build_clover_mesh(v))).collect();
        let mut r = Rng(0x9ea5);
        // Pick a random variant handle from a set.
        let pick = |set: &[Handle<Mesh>], r: &mut Rng| -> Handle<Mesh> {
            set[((r.f() * set.len() as f32) as usize).min(set.len() - 1)].clone()
        };
        let sprig = |x: f32, z: f32, mesh: Handle<Mesh>, s: f32, r: &mut Rng, commands: &mut Commands| {
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(cover_mat.clone()),
                Transform {
                    translation: Vec3::new(x, 0.02, z),
                    rotation: Quat::from_rotation_y(r.f() * std::f32::consts::TAU),
                    scale: Vec3::splat(s),
                },
                bevy::light::NotShadowCaster,
                CastlePart { kind: CastleKind::Always },
                BiomeEntity,
            ));
        };
        // Rim band: walk the yard perimeter, jittering across the fray band; skip the four
        // gate lanes (the approach roads keep their trodden-bare mouths).
        let (bx_, bz_) = (HALF_X - 1.2, HALF_Z - 1.2);
        for i in 0..64 {
            let t = i as f32 / 64.0;
            let p = t * 2.0 * (bx_ + bz_); // half-perimeter parameter (mirrored to ±)
            let (mut x, mut z) = if p < bx_ * 2.0 { (p - bx_, bz_) } else { (bx_, p - bx_ * 2.0 - bz_) };
            if r.f() < 0.5 { x = -x; }
            if r.f() < 0.5 { z = -z; }
            x += (r.f() - 0.5) * 3.4;
            z += (r.f() - 0.5) * 3.4;
            let near_gate = (x.abs() < 2.4 && z.abs() > bz_ - 1.5) || (z.abs() < 2.4 && x.abs() > bx_ - 1.5);
            if near_gate {
                continue;
            }
            let mesh = if r.f() < 0.55 { pick(&tufts, &mut r) } else { pick(&clovers, &mut r) };
            let s = 0.45 + r.f() * 0.45;
            sprig(x, z, mesh, s, &mut r, commands);
        }
        // Lawn patches inside the courtyard — small, sparse, low ("gdzie niegdzie"), and
        // never poking up through the plaza/path paving.
        for _ in 0..14 {
            let x = (r.f() - 0.5) * 2.0 * (bx_ - 2.0);
            let z = (r.f() - 0.5) * 2.0 * (bz_ - 2.0);
            if on_paving(x, z) {
                continue;
            }
            let mesh = if r.f() < 0.35 { pick(&tufts, &mut r) } else { pick(&clovers, &mut r) };
            let s = 0.35 + r.f() * 0.3;
            sprig(x, z, mesh, s, &mut r, commands);
        }
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
        // Smoke rises from the archetype's own stack; huts have none (they vent through thatch).
        let Some((ch, ch_top)) = house_chimney(HOUSE_STYLES[hi]) else { continue };
        let rot = face_center(hx, hz);
        let local = Quat::from_rotation_y(rot) * Vec3::new(ch.x * 0.9, 0.0, ch.y * 0.9);
        let (cx, cz) = (hx + local.x, hz + local.z);
        let base_y = ch_top * 0.74 + 0.2;
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

    // A few hens pecking in the dirt by the wood yard + well (one shared, batched mesh set; each is
    // a moving root parented to its parts, so `peck_hens` can bob the whole bird). Permanent — the
    // courtyard keeps its fowl whatever gets built.
    let hen_meshes: Vec<(Handle<Mesh>, M)> =
        hen_parts().into_iter().map(|(m, slot)| (meshes.add(m), slot)).collect();
    for (i, (hx, hz, yaw, sp)) in [
        (-10.6_f32, 5.2_f32, 0.7_f32, 1.6_f32),
        (-9.2, 6.7, 2.2, 1.9),
        (10.5, -6.6, -1.1, 1.7),
        (10.9, -5.1, 2.6, 2.2),
    ]
    .into_iter()
    .enumerate()
    {
        let base_yaw = Quat::from_rotation_y(yaw);
        let parent = commands
            .spawn((
                Transform::from_xyz(hx, 0.0, hz).with_rotation(base_yaw),
                Visibility::Inherited,
                CastlePart { kind: CastleKind::Always },
                Hen { base_yaw, phase: i as f32 * 1.3, speed: sp },
                BiomeEntity,
            ))
            .id();
        commands.entity(parent).with_children(|p| {
            for (mh, slot) in &hen_meshes {
                p.spawn((Mesh3d(mh.clone()), MeshMaterial3d(mats.get(*slot)), Transform::default()));
            }
        });
    }

    // Courtyard set dressing + the upgrade-bought set pieces (armory, tax booth, shrine, …) —
    // a separate module so the structures stay readable here.
    crate::castle_decor::build(commands, meshes, &mats);

    // Only the always-present keep is solid from the start; the gated structures register their
    // blockers when their upgrade reveals them (see `sync_castle`), so the courtyard is open
    // until you build the walls — no invisible barriers.
    register_keep_blocker();
    mats
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

/// One house's blocker (registered when that district is built) — sized to its archetype.
fn register_house_blocker(i: usize) {
    let p = COLLISION_PAD;
    let (x, z) = houses()[i];
    let (w, d) = house_dims(HOUSE_STYLES[i]);
    let (hx, hz) = (w * 0.9 / 2.0 + p, d * 0.9 / 2.0 + p);
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

/// A pre-wall courtyard hen: a moving root that [`peck_hens`] bobs + tips forward (pecking the
/// dirt). `base_yaw` is its facing; visibility is driven by its `CastlePart { PreWalls }`.
#[derive(Component)]
struct Hen {
    base_yaw: Quat,
    phase: f32,
    speed: f32,
}

/// Which upgrade reveals a given castle part (the castle builds up instead of starting full).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CastleKind {
    Always,
    /// Shown ONLY before the Palisade Walls go up — the trodden packed-earth route network of the
    /// young settlement. Buying Walls hides it (the same routes re-appear paved in cobble).
    PreWalls,
    Walls,
    Gate,
    Towers,
    House(u8),
}

#[derive(Component)]
pub(crate) struct CastlePart {
    pub(crate) kind: CastleKind,
}

/// Which gated groups have had their (append-only) collision blockers registered, so each is
/// added exactly once the first time it's revealed. **Must be reset whenever `blockers::reset()`
/// wipes the obstacle set** (e.g. a world rebuild / biome swap), or the lazily-registered
/// wall/tower/house boxes never come back — they read as "already built" while their collision is
/// gone. `biome::apply_build` does this reset right after it calls `blockers::reset()`.
#[derive(Resource, Default)]
pub(crate) struct CastleBuilt {
    walls: bool,
    towers: bool,
    houses: [bool; 12],
}

/// Reveal castle parts and lazily register each group's collision the first time it appears.
/// Walls/towers/gate gate on the Defense-branch upgrades (the keep fortifies as you invest); the
/// 8 interior **houses** are the town's dwellings — the first `town.houses` of them show, so the
/// settlement visibly grows as you build homes (start 2 → 4 peasants). The farm is gone (food is a
/// flow now; producers live out on the town plots).
fn sync_castle(
    def: Res<Defenses>,
    town: Res<crate::town::TownRes>,
    mut built: ResMut<CastleBuilt>,
    mut commands: Commands,
    mut q: Query<(Entity, &CastlePart, &mut Visibility, &mut Transform, Option<&crate::build_fx::RevealAt>)>,
    mut seeded: Local<bool>,
) {
    let houses = town.0.houses;
    // Construction feedback (rise + dust) fires only on a *live* flag flip — never on the first
    // pass (FOREST_DEFEND staging / a loaded save start already-built) and never on a biome
    // rebuild's respawn (parts respawn Hidden but def/town didn't change).
    let live = *seeded && (def.is_changed() || town.is_changed());
    *seeded = true;
    let mut dust: Vec<Vec3> = Vec::new();
    for (e, part, mut vis, mut tf, at) in &mut q {
        let show = match part.kind {
            CastleKind::Always => true,
            CastleKind::PreWalls => !def.walls, // the bare-yard look gives way to the cobbled courtyard
            CastleKind::Walls => def.walls,
            CastleKind::Gate => def.walls && def.gate, // a gate without walls would float
            CastleKind::Towers => def.towers,
            CastleKind::House(i) => (i as u32) < houses,
        };
        if live && show && *vis == Visibility::Hidden {
            if let Some(crate::build_fx::RevealAt(pos)) = at {
                let pop = crate::build_fx::BuildPop::rise();
                tf.scale = pop.scale0(); // same-frame, so the part never flashes full-size
                commands.entity(e).try_insert(pop);
                // One burst per structure, not per material part (a house is ~4 entities).
                if !dust.iter().any(|p| p.distance_squared(*pos) < 1.0) {
                    dust.push(*pos);
                    commands.spawn(crate::build_fx::DustBurst::part(*pos));
                }
            }
        }
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
    for i in 0..12 {
        if (i as u32) < houses && !built.houses[i] {
            built.houses[i] = true;
            register_house_blocker(i);
        }
    }
}

/// Windows are lamplight, not paint: barely lit by day, warm and bright once the townsfolk
/// shutter themselves indoors at dusk — the visible half of the night curfew (`villagers`
/// pulls everyone off the streets as night falls; the lit windows say where they went).
/// One shared material drives every window on the island, so this is a single write.
fn window_glow(
    clock: Res<crate::scene::SkyClock>,
    mats: Option<Res<VillageMats>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mats) = mats else { return };
    let night = crate::scene::night_of(clock.t);
    if let Some(m) = materials.get_mut(&mats.0.get(M::Window)) {
        m.emissive = srgb(WINDOW_GLOW).to_linear() * (0.35 + 4.4 * night);
    }
}

pub struct CastlePlugin;
impl Plugin for CastlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CastleBuilt>()
            .add_systems(Update, (drift_smoke, peck_hens, sync_castle, window_glow));
    }
}

/// Bob each hen and tip it forward on the down-stroke so it reads as pecking the dirt (ungated VFX
/// — keeps moving even while the world is frozen, like the chimney smoke).
fn peck_hens(time: Res<Time>, mut q: Query<(&Hen, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (h, mut tf) in &mut q {
        let cycle = (t * h.speed + h.phase).sin();
        let peck = cycle.max(0.0); // 0..1 forward-tip on the down beat
        tf.rotation = h.base_yaw * Quat::from_rotation_z(peck * 0.55);
        tf.translation.y = peck * 0.015; // a tiny hop
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
