//! Open ocean filling the arc the [`Backdrop`] leaves un-landed. A large annulus-sector
//! water sheet starts just past the 32×32 patch and runs to the fog horizon on the
//! ocean side, so standing in the scene you see land/mountains one way and open sea the
//! other. A thin sandy beach eases the grass→sea shoreline.
//!
//! Reuses the river's `WaterMaterial` (same animated-normal reflective shader) but with
//! a longer, slower open-sea swell. The ground plane is opaque green underneath, so the
//! sea sheet sits just ABOVE y=0 to be visible (same rule as the river).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;

use crate::biome::{Backdrop, BiomeEntity};
use crate::palette::lin;
use crate::water::{WaterExt, WaterMaterial, WaterParams};

/// Sea begins this far from the origin (past the populated patch, HALF=16).
const SEA_R_IN: f32 = 22.0;
/// …and runs out to here, well into the fog.
const SEA_R_OUT: f32 = 360.0;
/// Sea surface height — just above the opaque ground (and the river at 0.045).
const SEA_Y: f32 = 0.05;
/// Beach band, a sandy ring just inside the shoreline at ~ground level.
const BEACH_IN: f32 = 19.5;
const BEACH_OUT: f32 = 22.4;
const BEACH_Y: f32 = 0.012;
const BEACH_SAND: u32 = 0x9a8a60;

/// Spawn the ocean (water sector + beach) for backdrop `b`. Tagged [`BiomeEntity`].
pub fn spawn_sea(
    b: &Backdrop,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    water_mats: &mut Assets<WaterMaterial>,
    std_mats: &mut Assets<StandardMaterial>,
) {
    // Ocean arc = complement of the land arc: centred opposite `land_dir`, spanning the
    // long way around. a0 → a1 with a1 > a0.
    let a0 = b.land_dir + b.land_arc;
    let a1 = b.land_dir + std::f32::consts::TAU - b.land_arc;

    // ── Water sheet ──
    let mesh = meshes.add(build_sector(a0, a1, SEA_R_IN, SEA_R_OUT, SEA_Y, 120, 6));
    let [or, og, ob] = hex3(b.ocean_color);
    let water = water_mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(or, og, ob, 0.88),
            perceptual_roughness: 0.30, // matte-ish, matching the basic river
            metallic: 0.0,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        },
        extension: WaterExt {
            params: WaterParams {
                // Gentle long open-sea swell (basic look).
                params: Vec4::new(0.16, 0.45, 0.4, 0.0),
                sky_tint: Vec4::new(0.70, 0.82, 0.93, 0.0),
            },
        },
    });
    commands.spawn((Mesh3d(mesh), MeshMaterial3d(water), Transform::default(), BiomeEntity));

    // ── Beach band ──
    let beach_mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let beach = meshes.add(build_beach(a0, a1));
    commands.spawn((
        Mesh3d(beach),
        MeshMaterial3d(beach_mat),
        Transform::default(),
        bevy::light::NotShadowCaster,
        BiomeEntity,
    ));
}

/// Full-circle open ocean ringing the whole world map beyond `r_in`, plus a beach ring
/// at the shore. Tagged [`BiomeEntity`]. (Kept for the wedge layout; the island map uses
/// a full sea plane instead.)
#[allow(dead_code)]
pub fn spawn_ring(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    water_mats: &mut Assets<WaterMaterial>,
    std_mats: &mut Assets<StandardMaterial>,
    r_in: f32,
    ocean_color: u32,
) {
    let mesh = meshes.add(build_sector(0.0, std::f32::consts::TAU, r_in, SEA_R_OUT, SEA_Y, 200, 6));
    let [or, og, ob] = hex3(ocean_color);
    let water = water_mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(or, og, ob, 0.88),
            perceptual_roughness: 0.30,
            metallic: 0.0,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        },
        extension: WaterExt {
            params: WaterParams {
                params: Vec4::new(0.16, 0.45, 0.4, 0.0),
                sky_tint: Vec4::new(0.70, 0.82, 0.93, 0.0),
            },
        },
    });
    commands.spawn((Mesh3d(mesh), MeshMaterial3d(water), Transform::default(), BiomeEntity));

    // Beach ring at the shore.
    let beach_mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let beach = meshes.add(build_ring_beach(r_in));
    commands.spawn((
        Mesh3d(beach),
        MeshMaterial3d(beach_mat),
        Transform::default(),
        bevy::light::NotShadowCaster,
        BiomeEntity,
    ));
}

/// A full-circle sandy beach ring straddling radius `r_in`.
#[allow(dead_code)]
fn build_ring_beach(r_in: f32) -> Mesh {
    let inner = r_in - 2.6;
    let outer = r_in + 0.5;
    let ang_segs = 200usize;
    let cols = ang_segs + 1;
    let sand = lin(BEACH_SAND);
    let sand_wet = [sand[0] * 0.7, sand[1] * 0.7, sand[2] * 0.66, 1.0];

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let tau = std::f32::consts::TAU;
    for ci in 0..cols {
        let a = (ci as f32 / ang_segs as f32) * tau;
        let (ca, sa) = (a.cos(), a.sin());
        positions.push([ca * inner, BEACH_Y, sa * inner]);
        colors.push(sand);
        positions.push([ca * outer, BEACH_Y * 0.4, sa * outer]);
        colors.push(sand_wet);
    }
    for ci in 0..ang_segs {
        let k = (ci * 2) as u32;
        indices.extend_from_slice(&[k, k + 2, k + 1, k + 1, k + 2, k + 3]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh.duplicate_vertices();
    mesh.compute_flat_normals();
    mesh
}

fn hex3(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0]
}

/// A flat annulus sector at height `y`, `a0→a1` angular span, `r_in→r_out` radial. Up
/// normals; the water shader supplies the ripple normal per-fragment.
fn build_sector(a0: f32, a1: f32, r_in: f32, r_out: f32, y: f32, ang_segs: usize, rad_segs: usize) -> Mesh {
    let cols = ang_segs + 1;
    let rows = rad_segs + 1;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(rows * cols);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(rows * cols);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(rows * cols);

    for ri in 0..rows {
        let tr = ri as f32 / rad_segs as f32;
        // Pack rings denser near shore (where detail matters), looser toward the horizon.
        let rad = r_in + (r_out - r_in) * tr * tr;
        for ci in 0..cols {
            let ta = ci as f32 / ang_segs as f32;
            let a = a0 + (a1 - a0) * ta;
            let x = a.cos() * rad;
            let z = a.sin() * rad;
            positions.push([x, y, z]);
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([ta, tr]);
        }
    }

    let mut indices: Vec<u32> = Vec::with_capacity(rad_segs * ang_segs * 6);
    for ri in 0..rad_segs {
        for ci in 0..ang_segs {
            let a = (ri * cols + ci) as u32;
            let b = (ri * cols + ci + 1) as u32;
            let c = ((ri + 1) * cols + ci) as u32;
            let d = ((ri + 1) * cols + ci + 1) as u32;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Sandy beach ring (vertex-coloured, flat-shaded) between the grass and the sea.
fn build_beach(a0: f32, a1: f32) -> Mesh {
    let ang_segs = 96usize;
    let cols = ang_segs + 1;
    let sand = lin(BEACH_SAND);
    let sand_wet = [sand[0] * 0.7, sand[1] * 0.7, sand[2] * 0.66, 1.0];

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for ci in 0..cols {
        let ta = ci as f32 / ang_segs as f32;
        let a = a0 + (a1 - a0) * ta;
        let (ca, sa) = (a.cos(), a.sin());
        positions.push([ca * BEACH_IN, BEACH_Y, sa * BEACH_IN]);
        colors.push(sand);
        positions.push([ca * BEACH_OUT, BEACH_Y * 0.4, sa * BEACH_OUT]);
        colors.push(sand_wet);
    }
    for ci in 0..ang_segs {
        let k = (ci * 2) as u32;
        indices.extend_from_slice(&[k, k + 2, k + 1, k + 1, k + 2, k + 3]);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh.duplicate_vertices();
    mesh.compute_flat_normals();
    mesh
}
