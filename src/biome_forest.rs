//! Forest biome — the reference implementation every other biome mirrors. Ports the
//! original hand-tuned forest (grass ground, broadleaf/birch/dead trees, bushes, rocks,
//! ground cover) into the declarative [`BiomeConfig`], plus a `landmarks` hook that
//! drops the ruins + the full `decor` charm (logs, reeds, fireflies…).
//!
//! Land/ocean split: hills + treeline fill the `z < 0` half (`land_dir = -π/2`,
//! `land_arc = π/2`); the `z > 0` half is open sea.

// The `landmarks()` set-piece + the `decor` charm it spawns are authored biome content that
// the world map doesn't place yet (it uses `ruins` landmarks instead). Kept per design; allow
// the resulting dead code until it's wired into a per-region pass.
#![allow(dead_code)]

use bevy::prelude::*;

use crate::biome::{Backdrop, Biome, BiomeConfig, BiomeEntity, GroundDetail, ParticleKind, PropClass};
use crate::groundcover as gc;
use crate::palette::FOREST_GROUND;
use crate::props;
use crate::trees::{build_tree_mesh, TreeKind};

/// Trees are authored ~1.5u tall; scale up so they tower at eye level.
const TREE_SCALE: f32 = 1.7;

pub fn config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Forest,
        name: "Forest",

        ground_color: FOREST_GROUND,
        ground_roughness: 0.95,
        detail: GroundDetail {
            scale: 0.18,
            strength: 0.45,
            variation: 0.70,
            seed: 1.0,
            dark: 0x356b28,
            base: 0x5d9e44,
            light: 0x95d162,
            grain: 0.55,
            streak: 0.5,
        },

        sky: 0xb2d1ed,
        fog_density: 0.009,
        sun_color: 0xffedc7,
        sun_illuminance: 10_500.0,
        ambient_color: 0xe0edff,
        ambient_brightness: 85.0,
        sun_pos: Vec3::new(16.0, 40.0, 10.0),

        seed: 2027,
        tree_min_dist: 2.7,
        classes: vec![
            // Trees: 55% broadleaf / 16% birch / 22% pine / 7% dead — each living kind
            // expanded into the TREE_TINTS hue spread (neutral / warm-dry / deep-cool) so
            // neighbouring trees stop being the one identical green repeated forever.
            PropClass {
                variants: {
                    let mut v: Vec<(Mesh, f32)> = Vec::new();
                    let n = crate::trees::TREE_TINTS.len() as f32;
                    for (kind, w) in [
                        (TreeKind::Broadleaf, 0.55),
                        (TreeKind::Birch, 0.16),
                        (TreeKind::Pine, 0.22),
                    ] {
                        for t in crate::trees::TREE_TINTS {
                            v.push((crate::trees::tint_mesh(build_tree_mesh(kind), t), w / n));
                        }
                    }
                    v.push((build_tree_mesh(TreeKind::Dead), 0.07));
                    v
                },
                chance: 0.075,
                // Wider spread than the old (0.85, 1.3): the canopy was reading as one
                // even-height broccoli wall — silhouette variety beats more trees.
                scale: (0.72 * TREE_SCALE, 1.42 * TREE_SCALE),
                tree: true,
                block_radius: 0.0,
            },
            // Ancient giants — rare towering broadleafs (~2× canopy height) that break
            // the tree-line silhouette; one every few dozen tiles reads as an
            // old-growth relic the forest grew around.
            PropClass {
                variants: crate::trees::TREE_TINTS
                    .iter()
                    .map(|t| (crate::trees::tint_mesh(build_tree_mesh(TreeKind::Broadleaf), *t), 1.0))
                    .collect(),
                chance: 0.005,
                scale: (1.75 * TREE_SCALE, 2.15 * TREE_SCALE),
                tree: true,
                block_radius: 0.0,
            },
            // Bushes (also the tree-too-close fallback) — same hue-spread trick as trees.
            PropClass {
                variants: {
                    const BUSH_TINTS: [[f32; 3]; 3] =
                        [[1.0, 1.0, 1.0], [1.10, 1.05, 0.78], [0.80, 0.94, 0.84]];
                    let mut v: Vec<(Mesh, f32)> = Vec::new();
                    for bv in 0..props::NUM_BUSH_VARIANTS {
                        for t in BUSH_TINTS {
                            v.push((crate::trees::tint_mesh(props::build_bush_mesh(bv), t), 1.0));
                        }
                    }
                    v
                },
                chance: 0.06,
                scale: (0.8, 1.35),
                tree: false,
                block_radius: 0.0,
            },
            // Rocks — subtle warm-sandstone / cool-slate tints break the uniform grey.
            PropClass {
                variants: {
                    const ROCK_TINTS: [[f32; 3]; 3] =
                        [[1.0, 1.0, 1.0], [1.08, 1.01, 0.90], [0.90, 0.95, 1.06]];
                    let mut v: Vec<(Mesh, f32)> = Vec::new();
                    for rv in 0..props::NUM_ROCK_VARIANTS {
                        for t in ROCK_TINTS {
                            v.push((crate::trees::tint_mesh(props::build_rock_mesh(rv), t), 1.0));
                        }
                    }
                    v
                },
                chance: 0.03,
                scale: (0.6, 1.6),
                tree: false,
                block_radius: 0.24, // big rocks block; small (scale ≲1.0) stay walk-through
            },
        ],
        cover: vec![
            PropClass { variants: vec![(gc::build_grass_tuft_mesh(), 1.0)], chance: 0.15, scale: (0.45, 0.7), tree: false, block_radius: 0.0 },
            PropClass { variants: vec![(gc::build_clover_mesh(), 1.0)], chance: 0.30, scale: (0.7, 1.2), tree: false, block_radius: 0.0 },
            PropClass {
                variants: (0..4).map(|v| (gc::build_mushroom_mesh(v), 1.0)).collect(),
                chance: 0.13,
                scale: (0.7, 1.2),
                tree: false,
                block_radius: 0.0,
            },
            // Wildflowers — the meadow's life. 7 colour/shape variants (pink/yellow/white
            // daisies, red poppies, blue cornflowers, violets), denser than before.
            PropClass {
                variants: (0..gc::NUM_FLOWER_VARIANTS).map(|v| (gc::build_flower_mesh(v), 1.0)).collect(),
                chance: 0.20,
                scale: (0.8, 1.3),
                tree: false,
                block_radius: 0.0,
            },
            PropClass { variants: vec![(gc::build_fern_mesh(), 1.0)], chance: 0.08, scale: (0.7, 1.1), tree: false, block_radius: 0.0 },
            // Forest-floor litter — pinecones, acorns, pebbles, fallen leaves.
            PropClass {
                variants: (0..gc::NUM_LITTER_VARIANTS).map(|v| (gc::build_floor_litter_mesh(v), 1.0)).collect(),
                chance: 0.08,
                scale: (0.7, 1.25),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover_per_tile: 2,

        river: true,
        river_color: 0x2f8fd6,
        backdrop: Backdrop {
            land_dir: -std::f32::consts::FRAC_PI_2,
            land_arc: std::f32::consts::FRAC_PI_2,
            ocean: true,
            ocean_color: 0x2f6fae,
            hill_body: 0x8f9aa0,
            hill_cap: 0xb8c2c6,
            hill_foot: 0x7a8890,
            treeline: true,
            treeline_dark: 0x2c4a34,
            treeline_mid: 0x365c3e,
            hill_h: (34.0, 78.0),
        },
        particle: ParticleKind::None,
    }
}

/// Ruins + the decor charm.
pub fn landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(crate::ruins::build_trilithon_mesh())),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(-11.0, 0.0, -12.0).with_rotation(Quat::from_rotation_y(0.4)),
        BiomeEntity,
    ));
    commands.spawn((
        Mesh3d(meshes.add(crate::ruins::build_giant_dead_tree_mesh())),
        MeshMaterial3d(mat),
        Transform::from_xyz(10.0, 0.0, -13.0),
        BiomeEntity,
    ));

    crate::decor::build(commands, meshes, materials);
}
