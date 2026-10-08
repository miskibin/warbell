//! The third-person hero body: the Royal Footman, baked from the three.js model
//! (`assets/models/footman.bin`) and shaded by the triplanar path in `creature.wgsl`.
//!
//! Meshes stay in the geometry's own space and keep the transform they had under each joint, so
//! the shader's object-space triplanar sample matches the authored textures. The asset is mirrored
//! across X — the sword arm lands on `+X`, which is the side [`super::anim`] poses as `ShoulderR`.
//! Sword and shield entity transforms are pre-multiplied by the inverse of the animator's rest
//! pose, so that rest pose reproduces the footman's carry instead of swinging the blade twice.

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};

use super::anim::sword_rest_r;
use super::{HeroMesh, HeroPart, HeroWeapon, Joint};

const BIN: &[u8] = include_bytes!("../../assets/models/footman.bin");

/// Standing height the previous knight occupied in rig units. The footman is authored in metres
/// (~1.93 tall); this scale brings him back up so `HERO_SCALE` and the cameras still frame him.
const TARGET_RIG_HEIGHT: f32 = 2.35;

/// Shield rest, matching `anim::SHIELD_REST_T` / `shield_rest_r`. The asset's shield meshes are
/// pre-transformed by the inverse, so this pose is the footman's held shield.
const SHIELD_REST_T: Vec3 = Vec3::new(-0.07, -0.08, 0.13);

fn shield_rest_r() -> Quat {
    Quat::from_euler(EulerRot::XYZ, 0.12, -1.5, 0.0)
}

static TIP: OnceLock<Vec3> = OnceLock::new();

/// Blade tip in the blade mesh's local space. [`super::combat::hero_blade_trail`] reads it off the
/// `HeroWeapon` entity (the blade mesh itself).
pub fn weapon_tip() -> Vec3 {
    TIP.get().copied().unwrap_or(Vec3::new(0.0, -0.95, -0.006))
}

struct MatCpu {
    tex: String,
    color: u32,
    metal: f32,
    rough: f32,
    scale: f32,
    bump: f32,
    rough_mul: f32,
    double: bool,
    mode: u8,
}

struct MeshCpu {
    joint: String,
    mat: usize,
    xf: Transform,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

struct Model {
    min_y: f32,
    max_y: f32,
    joints: Vec<(String, String, Vec3)>,
    mats: Vec<MatCpu>,
    meshes: Vec<MeshCpu>,
    blade: usize,
    tip: Vec3,
}

struct R<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        s
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().unwrap())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn str(&mut self) -> String {
        let n = self.u8() as usize;
        String::from_utf8(self.take(n).to_vec()).expect("footman name")
    }
    fn f32s(&mut self, n: usize) -> Vec<f32> {
        (0..n).map(|_| self.f32()).collect()
    }
    fn u32s(&mut self, n: usize) -> Vec<u32> {
        (0..n).map(|_| self.u32()).collect()
    }
}

fn parse() -> Model {
    let mut r = R { b: BIN, i: 0 };
    assert_eq!(&r.take(4), b"FTMN", "footman.bin magic");
    assert_eq!(r.u32(), 1, "footman.bin version");
    let min_y = r.f32();
    let max_y = r.f32();
    let _hip_y = r.f32();
    let tip = Vec3::new(r.f32(), r.f32(), r.f32());
    let blade = r.u32() as usize;
    let nj = r.u32();
    let mut joints = Vec::with_capacity(nj as usize);
    for _ in 0..nj {
        let name = r.str();
        let parent = r.str();
        let pos = Vec3::new(r.f32(), r.f32(), r.f32());
        joints.push((name, parent, pos));
    }
    let nm = r.u32();
    let mut mats = Vec::with_capacity(nm as usize);
    for _ in 0..nm {
        let _name = r.str();
        mats.push(MatCpu {
            tex: r.str(),
            color: r.u32(),
            metal: r.f32(),
            rough: r.f32(),
            scale: r.f32(),
            bump: r.f32(),
            rough_mul: r.f32(),
            double: r.u8() != 0,
            mode: r.u8(),
        });
    }
    let nmesh = r.u32() as usize;
    let mut meshes = Vec::with_capacity(nmesh);
    for _ in 0..nmesh {
        let joint = r.str();
        let mat = r.u16() as usize;
        let xf = Transform {
            translation: Vec3::new(r.f32(), r.f32(), r.f32()),
            rotation: Quat::from_xyzw(r.f32(), r.f32(), r.f32(), r.f32()).normalize(),
            scale: Vec3::new(r.f32(), r.f32(), r.f32()),
        };
        let nv = r.u32() as usize;
        let ni = r.u32() as usize;
        let p = r.f32s(nv * 3);
        let n = r.f32s(nv * 3);
        let indices = r.u32s(ni);
        let positions = p.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        let normals = n.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        meshes.push(MeshCpu { joint, mat, xf, positions, normals, indices });
    }
    // Textures follow; the GPU build reads them from the same cursor via `textures_at`.
    Model { min_y, max_y, joints, mats, meshes, blade, tip }
}

/// Byte offset where the texture block starts (after meshes). Re-walks the header so the PNG bytes
/// are borrowed from `BIN` instead of copied into the parsed model.
fn texture_offset() -> usize {
    let mut r = R { b: BIN, i: 4 + 4 }; // magic + version
    r.i += 4 * 6; // min, max, hip, tip xyz
    r.i += 4; // blade
    let nj = r.u32();
    for _ in 0..nj {
        let n = r.u8() as usize;
        r.i += n;
        let p = r.u8() as usize;
        r.i += p + 12;
    }
    let nm = r.u32();
    for _ in 0..nm {
        let n = r.u8() as usize;
        r.i += n;
        let t = r.u8() as usize;
        r.i += t + 4 + 4 * 5 + 2;
    }
    let nmesh = r.u32();
    for _ in 0..nmesh {
        let n = r.u8() as usize;
        r.i += n + 2 + 4 * 10; // joint, mat, transform
        let nv = r.u32();
        let ni = r.u32();
        r.i += (nv as usize) * 6 * 4 + (ni as usize) * 4;
    }
    r.i
}

fn srgb_to_lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn lin_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Full mip chain. Albedo is filtered in linear light (then re-encoded) so the mips match a GPU
/// sRGB `generateMipmap`; the height/roughness map stays linear.
fn mip_chain(base: &[u8], w: u32, h: u32, srgb: bool) -> (Vec<u8>, u32) {
    let mut data = base.to_vec();
    let mut levels = 1u32;
    let mut prev_w = w;
    let mut prev_h = h;
    let mut prev_off = 0usize;
    while prev_w > 1 || prev_h > 1 {
        let nw = (prev_w / 2).max(1);
        let nh = (prev_h / 2).max(1);
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 4];
                for dy in 0..2u32 {
                    for dx in 0..2u32 {
                        let sx = (x * 2 + dx).min(prev_w - 1);
                        let sy = (y * 2 + dy).min(prev_h - 1);
                        let i = prev_off + ((sy * prev_w + sx) * 4) as usize;
                        for c in 0..4 {
                            let b = data[i + c] as f32 / 255.0;
                            acc[c] += if srgb && c < 3 { srgb_to_lin(b) } else { b };
                        }
                    }
                }
                let o = ((y * nw + x) * 4) as usize;
                for c in 0..4 {
                    let v = acc[c] * 0.25;
                    let e = if srgb && c < 3 { lin_to_srgb(v) } else { v };
                    next[o + c] = (e.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        prev_off += (prev_w * prev_h * 4) as usize;
        prev_w = nw;
        prev_h = nh;
        levels += 1;
    }
    (data, levels)
}

fn decoded_image(png: &[u8], srgb: bool) -> Image {
    let img = image::load_from_memory(png).expect("footman texture").to_rgba8();
    let (w, h) = img.dimensions();
    let raw = img.into_raw();
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let (data, levels) = mip_chain(&raw, w, h, srgb);
    let mut image = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        raw,
        format,
        RenderAssetUsages::default(),
    );
    // `Image::new` checks the base level only; the chain (and its level count) land after that.
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

fn srgb_hex(hex: u32) -> Color {
    Color::srgb(
        ((hex >> 16) & 255) as f32 / 255.0,
        ((hex >> 8) & 255) as f32 / 255.0,
        (hex & 255) as f32 / 255.0,
    )
}

struct Gpu {
    model: Model,
    meshes: Vec<Handle<Mesh>>,
    mats: Vec<Handle<crate::creature::CreatureMaterial>>,
}

static GPU: OnceLock<Gpu> = OnceLock::new();

fn gpu(
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<crate::creature::CreatureMaterial>,
) -> &'static Gpu {
    GPU.get_or_init(|| {
        let model = parse();
        let mut r = R { b: BIN, i: texture_offset() };
        let nt = r.u32();
        let mut tex: HashMap<String, (Handle<Image>, Handle<Image>)> = HashMap::new();
        for _ in 0..nt {
            let name = r.str();
            let kind = r.u8();
            let len = r.u32() as usize;
            let png = r.take(len);
            let img = images.add(decoded_image(png, kind == 0));
            let e = tex.entry(name).or_insert_with(|| (Handle::default(), Handle::default()));
            if kind == 0 {
                e.0 = img;
            } else {
                e.1 = img;
            }
        }
        assert_eq!(r.i, BIN.len(), "footman.bin trailing bytes");
        let mats = model
            .mats
            .iter()
            .map(|m| {
                let (alb, mr) = if m.mode == 1 {
                    tex.get(&m.tex).cloned().unwrap_or_else(|| {
                        let w = white_image(images);
                        (w.clone(), w)
                    })
                } else {
                    let w = white_image(images);
                    (w.clone(), w)
                };
                let mode = if m.mode == 1 { 1.0 } else { 0.0 };
                materials.add(ExtendedMaterial {
                    base: StandardMaterial {
                        base_color: srgb_hex(m.color),
                        metallic: m.metal,
                        perceptual_roughness: m.rough.clamp(0.05, 1.0),
                        cull_mode: if m.double { None } else { Some(Face::Back) },
                        double_sided: m.double,
                        ..default()
                    },
                    extension: crate::creature::CreatureExt {
                        params: crate::creature::CreatureParams {
                            params: Vec4::ZERO,
                            pbr: Vec4::new(m.scale, m.bump, m.rough_mul, mode),
                        },
                        albedo: Some(alb),
                        mr: Some(mr),
                    },
                })
            })
            .collect();
        let mesh_handles = model
            .meshes
            .iter()
            .map(|m| {
                let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, m.positions.clone());
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, m.normals.clone());
                // White vertex colour so this mesh takes the VERTEX_COLORS pipeline (the shared
                // creature shader reads `in.color`). The tint lives on the material, not here.
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 1.0, 1.0, 1.0]; m.positions.len()]);
                let uvs = vec![[0.0, 0.0]; m.positions.len()];
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
                mesh.insert_indices(Indices::U32(m.indices.clone()));
                meshes.add(mesh)
            })
            .collect();
        let _ = TIP.set(model.tip);
        info!(
            "footman: {} meshes, {:.2}u tall (rig scale {:.3})",
            model.meshes.len(),
            model.max_y - model.min_y,
            TARGET_RIG_HEIGHT / (model.max_y - model.min_y)
        );
        Gpu { model, meshes: mesh_handles, mats }
    });
    GPU.get().unwrap()
}

fn white_image(images: &mut Assets<Image>) -> Handle<Image> {
    let mut image = Image::new(
        Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    images.add(image)
}

fn joint_of(name: &str) -> Option<Joint> {
    Some(match name {
        "Hips" => Joint::Hips,
        "Torso" => Joint::Torso,
        "Head" => Joint::Head,
        "ShoulderL" => Joint::ShoulderL,
        "ShoulderR" => Joint::ShoulderR,
        "ElbowL" => Joint::ElbowL,
        "ElbowR" => Joint::ElbowR,
        "HipL" => Joint::HipL,
        "HipR" => Joint::HipR,
        "KneeL" => Joint::KneeL,
        "KneeR" => Joint::KneeR,
        "FootL" => Joint::FootL,
        "FootR" => Joint::FootR,
        "Shield" => Joint::Shield,
        "Sword" => Joint::Sword,
        _ => return None,
    })
}

fn pos_of(model: &Model, name: &str) -> Vec3 {
    model.joints.iter().find(|(n, _, _)| n == name).map(|(_, _, p)| *p).unwrap_or(Vec3::ZERO)
}

/// Spawn the footman under `root` (the hero entity, already carrying `HERO_SCALE`).
pub fn spawn(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<crate::creature::CreatureMaterial>,
) {
    let g = gpu(meshes, images, materials);
    let height = (g.model.max_y - g.model.min_y).max(0.01);
    let rig_scale = TARGET_RIG_HEIGHT / height;
    let rig = commands
        .spawn((
            Transform {
                translation: Vec3::new(0.0, -g.model.min_y * rig_scale, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::splat(rig_scale),
            },
            Visibility::Visible,
        ))
        .id();
    commands.entity(root).add_child(rig);

    let mut by_joint: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, m) in g.model.meshes.iter().enumerate() {
        by_joint.entry(m.joint.as_str()).or_default().push(i);
    }

    let spawn_joint = |commands: &mut Commands, parent: Entity, name: &str, xf: Transform| -> Entity {
        let mut ec = commands.spawn((xf, Visibility::Visible));
        if let Some(j) = joint_of(name) {
            ec.insert(HeroPart { joint: j });
        }
        let id = ec.id();
        commands.entity(parent).add_child(id);
        if let Some(list) = by_joint.get(name) {
            for &i in list {
                let m = &g.model.meshes[i];
                let mut le = commands.spawn((
                    Mesh3d(g.meshes[i].clone()),
                    MeshMaterial3d(g.mats[m.mat].clone()),
                    m.xf,
                    HeroMesh,
                ));
                if i == g.model.blade {
                    le.insert(HeroWeapon);
                }
                let leaf = le.id();
                commands.entity(id).add_child(leaf);
            }
        }
        id
    };

    let hips = spawn_joint(commands, rig, "Hips", Transform::from_translation(pos_of(&g.model, "Hips")));
    let torso = spawn_joint(commands, hips, "Torso", Transform::from_translation(pos_of(&g.model, "Torso")));
    spawn_joint(commands, torso, "Head", Transform::from_translation(pos_of(&g.model, "Head")));

    let sh_l = spawn_joint(commands, torso, "ShoulderL", Transform::from_translation(pos_of(&g.model, "ShoulderL")));
    let el_l = spawn_joint(commands, sh_l, "ElbowL", Transform::from_translation(pos_of(&g.model, "ElbowL")));
    // Untagged hand at the fist. The animator rewrites the shield joint's translation every frame,
    // so the fist offset has to live on a parent the clip doesn't touch.
    let hand_l = spawn_joint(commands, el_l, "handL", Transform::from_translation(pos_of(&g.model, "Shield")));
    spawn_joint(
        commands,
        hand_l,
        "Shield",
        Transform { translation: SHIELD_REST_T, rotation: shield_rest_r(), scale: Vec3::ONE },
    );

    let sh_r = spawn_joint(commands, torso, "ShoulderR", Transform::from_translation(pos_of(&g.model, "ShoulderR")));
    let el_r = spawn_joint(commands, sh_r, "ElbowR", Transform::from_translation(pos_of(&g.model, "ElbowR")));
    let hand_r = spawn_joint(commands, el_r, "handR", Transform::from_translation(pos_of(&g.model, "Sword")));
    spawn_joint(commands, hand_r, "Sword", Transform::from_rotation(sword_rest_r()));

    let hip_l = spawn_joint(commands, hips, "HipL", Transform::from_translation(pos_of(&g.model, "HipL")));
    let knee_l = spawn_joint(commands, hip_l, "KneeL", Transform::from_translation(pos_of(&g.model, "KneeL")));
    spawn_joint(commands, knee_l, "FootL", Transform::from_translation(pos_of(&g.model, "FootL")));
    let hip_r = spawn_joint(commands, hips, "HipR", Transform::from_translation(pos_of(&g.model, "HipR")));
    let knee_r = spawn_joint(commands, hip_r, "KneeR", Transform::from_translation(pos_of(&g.model, "KneeR")));
    spawn_joint(commands, knee_r, "FootR", Transform::from_translation(pos_of(&g.model, "FootR")));
}
