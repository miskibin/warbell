//! **Bog dressing** (map-character overhaul pass 3) — the swamp's signature set-dressing over
//! and around the new standing bog pools (`worldmap::pool_sd`): dead trees rising OUT of the
//! water, half-sunk cypress stumps, a drowned tower ruin leaning in the biggest pool, a stilt
//! hut at a shore, will-o'-wisp motes drifting over the water and glow-mushroom clusters
//! hugging the banks. This finally WIRES the authored-but-dead `biome_swamp::landmarks()`
//! content (wisps, glow-mushrooms, hollow dead tree) into the world map — plus a slow bob so
//! the wisps live instead of hanging frozen.
//!
//! Everything is deterministic (mulberry32-style hash walk over a fixed seed), tagged
//! `BiomeEntity`, distance-culled, and placed by sampling `pool_sd_world`: trees want DEEP
//! water (sd < −0.8), shore props hug the band just outside the waterline. The wisp/mushroom
//! emissives are `unlit` + `NotShadowCaster`, so the whole layer stays cheap.

use std::f32::consts::TAU;

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::lin;

/// Water level of the carved pools (the sea plane) — bases of drowned props sink below it.
const WATER_Y: f32 = -0.4;

pub struct BogPlugin;
impl Plugin for BogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, bob_wisps);
    }
}

/// Marker + phase for the will-o'-wisp hover animation.
#[derive(Component)]
struct Wisp {
    base: Vec3,
    phase: f32,
}

/// Slow eerie hover: a gentle vertical bob + a small horizontal drift circle. Runs ungated
/// (render-side dressing, like other anim systems) — 20-odd transforms is nothing.
fn bob_wisps(time: Res<Time>, mut q: Query<(&Wisp, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (w, mut tf) in &mut q {
        tf.translation = w.base
            + Vec3::new(
                (t * 0.31 + w.phase).sin() * 0.55,
                (t * 0.75 + w.phase * 1.7).sin() * 0.28,
                (t * 0.27 + w.phase * 0.6).cos() * 0.55,
            );
    }
}

fn rng_next(state: &mut u32) -> f32 {
    *state = state.wrapping_add(0x6d2b_79f5);
    let mut t = *state;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}

// ── Bespoke meshes (ruins.rs contract: primitives, vertex colour, flat normals, base y=0) ──
fn tint(mut m: Mesh, col: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(col); n]);
    m
}
fn assemble(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for part in it {
        base.merge(&part).expect("parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

/// The drowned tower: a leaning ring-walled stone stump, broken open at the top, half its
/// height meant to sit under the waterline. One per swamp — the pool-country landmark flag.
fn drowned_tower() -> Mesh {
    const STONE: u32 = 0x6f7468;
    const STONE_D: u32 = 0x565b52;
    const MOSS: u32 = 0x4c5e33;
    let mut v = Vec::new();
    // Tapering drum in three courses, each slightly narrower + tone-shifted.
    for (i, (r, h, col)) in
        [(1.35_f32, 1.6_f32, STONE_D), (1.18, 1.4, STONE), (1.02, 1.2, STONE)].into_iter().enumerate()
    {
        let y = match i {
            0 => 0.8,
            1 => 2.3,
            _ => 3.6,
        };
        v.push(tint(Cylinder::new(r, h).mesh().resolution(9).build().translated_by(Vec3::new(0.0, y, 0.0)), col));
    }
    // Broken crown: a few jagged merlon shards instead of a clean rim.
    for k in 0..5 {
        let a = k as f32 * (TAU / 5.0) + 0.4;
        let hh = 0.5 + (k as f32 * 1.7).sin().abs() * 0.5;
        v.push(tint(
            Cuboid::new(0.42, hh, 0.30).mesh().build().translated_by(Vec3::new(a.cos() * 0.92, 4.2 + hh * 0.5, a.sin() * 0.92)),
            if k % 2 == 0 { STONE } else { STONE_D },
        ));
    }
    // A gaping window hole implied by a dark inset + moss streaks running from the waterline.
    v.push(tint(Cuboid::new(0.5, 0.7, 0.12).mesh().build().translated_by(Vec3::new(0.0, 2.9, -1.16)), 0x2b2f28));
    for k in 0..4 {
        let a = k as f32 * 1.7 + 0.6;
        v.push(tint(
            Cuboid::new(0.18, 1.3, 0.06)
                .mesh()
                .build()
                .translated_by(Vec3::new(a.cos() * 1.30, 1.2, a.sin() * 1.30)),
            MOSS,
        ));
    }
    assemble(v)
}

/// A small stilt hut: plank cabin on four poles over the shallows, sagging roof, a ladder stub.
fn stilt_hut() -> Mesh {
    const POLE: u32 = 0x4a3826;
    const WALL: u32 = 0x6b543a;
    const WALL_D: u32 = 0x57422c;
    const ROOF: u32 = 0x3d4a33; // mossy thatch
    let mut v = Vec::new();
    for (sx, sz) in [(-0.8_f32, -0.7_f32), (0.8, -0.7), (-0.8, 0.7), (0.8, 0.7)] {
        v.push(tint(Cuboid::new(0.14, 1.8, 0.14).mesh().build().translated_by(Vec3::new(sx, 0.9, sz)), POLE));
    }
    v.push(tint(Cuboid::new(2.1, 0.12, 1.8).mesh().build().translated_by(Vec3::new(0.0, 1.75, 0.0)), WALL_D)); // floor
    v.push(tint(Cuboid::new(1.8, 1.1, 1.5).mesh().build().translated_by(Vec3::new(0.0, 2.35, 0.0)), WALL)); // cabin
    for s in [-1.0_f32, 1.0] {
        v.push(tint(
            Cuboid::new(1.15, 0.08, 1.9)
                .mesh()
                .build()
                .rotated_by(Quat::from_rotation_z(s * 0.5))
                .translated_by(Vec3::new(s * 0.48, 3.2, 0.0)),
            ROOF,
        ));
    }
    // Ladder stub down toward the water.
    v.push(tint(
        Cuboid::new(0.08, 1.3, 0.08).mesh().build().rotated_by(Quat::from_rotation_x(0.3)).translated_by(Vec3::new(0.6, 1.0, 0.95)),
        POLE,
    ));
    v.push(tint(
        Cuboid::new(0.08, 1.3, 0.08).mesh().build().rotated_by(Quat::from_rotation_x(0.3)).translated_by(Vec3::new(0.95, 1.0, 0.95)),
        POLE,
    ));
    for r in 0..3 {
        v.push(tint(
            Cuboid::new(0.5, 0.06, 0.06)
                .mesh()
                .build()
                .translated_by(Vec3::new(0.78, 0.55 + r as f32 * 0.4, 1.13 - r as f32 * 0.12)),
            WALL_D,
        ));
    }
    assemble(v)
}

/// Deterministic pool-aware placement of the whole bog layer. Runs as `worldmap::build_step`
/// phase 31 — after roads/bridges (boardwalk spans known → keep-out) and camps.
pub fn populate(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    use crate::worldmap::{ground_at_world, pool_sd_world};
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, ..default() });
    let wisp_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 1.0, 0.8),
        emissive: LinearRgba::from(Color::srgb(0.45, 1.0, 0.55)) * 55.0,
        unlit: true,
        ..default()
    });
    let glow_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 1.0, 0.9),
        emissive: LinearRgba::from(Color::srgb(0.35, 1.0, 0.82)) * 26.0,
        unlit: true,
        ..default()
    });
    let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: 110.0..110.0, use_aabb: true };
    let wisp_mesh = meshes.add(Sphere::new(0.07).mesh().ico(1).unwrap());
    let dead_tree = meshes.add(crate::biome_swamp::build_hollow_dead_tree_mesh());
    let mangroves: Vec<Handle<Mesh>> =
        (0..3).map(|v| meshes.add(crate::biome_swamp::build_mangrove_mesh(v))).collect();
    let stumps: Vec<Handle<Mesh>> =
        (0..2).map(|v| meshes.add(crate::biome_swamp::build_cypress_stump_mesh(v))).collect();
    let mush_stems = meshes.add(crate::biome_swamp::build_glowmush_stems_mesh());
    let mush_caps = meshes.add(crate::biome_swamp::build_glowmush_caps_mesh());

    // The two swamp interiors (world coords, mirrors worldmap REGIONS): S swamp + E marsh arm.
    let zones: [(f32, f32, f32); 2] = [(0.0, 83.6, 52.0), (66.0, 52.8, 40.0)];
    let mut rng: u32 = 0xb06_d12e5;
    let clear = |x: f32, z: f32| {
        !crate::bridges::near_bridge(x, z, 1.6)
            && !crate::camps::in_clearing(x, z)
            && !crate::roads::near_road(x, z, 1.5)
            && !crate::blockers::is_blocked(x, z)
    };

    // 1. Dead trees + drowned stumps standing IN the water: want genuinely wet spots so the
    //    trunks rise out of the murk, base sunk under the surface.
    let (mut trees, mut wisps, mut mush) = (0, 0, 0);
    for (zx, zz, zr) in zones {
        let mut placed_trees = 0;
        for _ in 0..2600 {
            if placed_trees >= 14 {
                break;
            }
            let a = rng_next(&mut rng) * TAU;
            let r = rng_next(&mut rng).sqrt() * zr;
            let (x, z) = (zx + a.cos() * r, zz + a.sin() * r);
            if pool_sd_world(x, z) > -0.9 || !clear(x, z) {
                continue;
            }
            let deep = rng_next(&mut rng);
            let (handle, scale, sink) = if deep < 0.30 {
                (&dead_tree, 0.9 + rng_next(&mut rng) * 0.5, 0.55)
            } else if deep < 0.72 {
                (&mangroves[(rng_next(&mut rng) * 3.0) as usize % 3], 1.15 + rng_next(&mut rng) * 0.6, 0.5)
            } else {
                (&stumps[(rng_next(&mut rng) * 2.0) as usize % 2], 1.3 + rng_next(&mut rng) * 0.8, 0.35)
            };
            commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(x, WATER_Y - sink, z)
                    .with_rotation(Quat::from_rotation_y(rng_next(&mut rng) * TAU))
                    .with_scale(Vec3::splat(scale)),
                BiomeEntity,
                range.clone(),
            ));
            placed_trees += 1;
            trees += 1;
        }

        // 2. Will-o'-wisps drifting over the water (green, bobbing — `bob_wisps`).
        let mut placed_wisps = 0;
        for _ in 0..900 {
            if placed_wisps >= 9 {
                break;
            }
            let a = rng_next(&mut rng) * TAU;
            let r = rng_next(&mut rng).sqrt() * zr;
            let (x, z) = (zx + a.cos() * r, zz + a.sin() * r);
            if pool_sd_world(x, z) > -0.55 {
                continue;
            }
            let base = Vec3::new(x, WATER_Y + 0.9 + rng_next(&mut rng) * 0.7, z);
            commands.spawn((
                Mesh3d(wisp_mesh.clone()),
                MeshMaterial3d(wisp_mat.clone()),
                Transform::from_translation(base),
                Wisp { base, phase: rng_next(&mut rng) * TAU },
                NotShadowCaster,
                BiomeEntity,
                range.clone(),
            ));
            placed_wisps += 1;
            wisps += 1;
        }

        // 3. Glow-mushroom clusters hugging the pool shores (on land, just outside the water).
        let mut placed_mush = 0;
        for _ in 0..1400 {
            if placed_mush >= 10 {
                break;
            }
            let a = rng_next(&mut rng) * TAU;
            let r = rng_next(&mut rng).sqrt() * zr;
            let (x, z) = (zx + a.cos() * r, zz + a.sin() * r);
            let sd = pool_sd_world(x, z);
            if !(0.3..2.2).contains(&sd) || !clear(x, z) {
                continue;
            }
            let Some(y) = ground_at_world(x, z) else { continue };
            let tf = Transform::from_xyz(x, y, z)
                .with_rotation(Quat::from_rotation_y(rng_next(&mut rng) * TAU))
                .with_scale(Vec3::splat(0.9 + rng_next(&mut rng) * 0.6));
            commands.spawn((Mesh3d(mush_stems.clone()), MeshMaterial3d(mat.clone()), tf, BiomeEntity, range.clone()));
            commands.spawn((
                Mesh3d(mush_caps.clone()),
                MeshMaterial3d(glow_mat.clone()),
                tf,
                NotShadowCaster,
                BiomeEntity,
                range.clone(),
            ));
            placed_mush += 1;
            mush += 1;
        }
    }

    // 4. The drowned tower — one, in the deepest spot of the S swamp's pool country. Reject-
    //    sample for the most negative pool sd (deepest = widest water around it).
    let mut best: Option<(f32, f32, f32)> = None;
    for _ in 0..3000 {
        let a = rng_next(&mut rng) * TAU;
        let r = rng_next(&mut rng).sqrt() * 50.0;
        let (x, z) = (0.0 + a.cos() * r, 83.6 + a.sin() * r);
        let sd = crate::worldmap::pool_sd_world(x, z);
        if sd < best.map_or(-1.1, |b| b.0) && clear(x, z) {
            best = Some((sd, x, z));
        }
    }
    if best.is_none() {
        warn!("bog: NO spot deep enough for the drowned tower (pool_sd never < -1.1) — check POOL_T/sd scaling");
    }
    if let Some((_, x, z)) = best {
        commands.spawn((
            Mesh3d(meshes.add(drowned_tower())),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, WATER_Y - 1.7, z)
                .with_rotation(Quat::from_rotation_y(rng_next(&mut rng) * TAU) * Quat::from_rotation_z(0.16))
                .with_scale(Vec3::splat(1.15)),
            BiomeEntity,
        ));
        crate::blockers::add_obb(x, z, 1.4, 1.4, 0.0);
        info!("bog: drowned tower at {x:.1},{z:.1}");
    }

    // 5. One stilt hut at a pool shore of the S swamp (feet in the shallows, cabin over water).
    let mut hut: Option<(f32, f32, f32)> = None;
    for _ in 0..2500 {
        let a = rng_next(&mut rng) * TAU;
        let r = rng_next(&mut rng).sqrt() * 50.0;
        let (x, z) = (0.0 + a.cos() * r, 83.6 + a.sin() * r);
        let sd = crate::worldmap::pool_sd_world(x, z);
        // Shoreline band, slightly wet side, clear of decks/roads.
        if (-0.7..-0.25).contains(&sd) && clear(x, z) {
            hut = Some((sd, x, z));
            break;
        }
    }
    if hut.is_none() {
        warn!("bog: no shoreline spot for the stilt hut");
    }
    if let Some((_, x, z)) = hut {
        commands.spawn((
            Mesh3d(meshes.add(stilt_hut())),
            MeshMaterial3d(mat),
            Transform::from_xyz(x, WATER_Y - 0.1, z).with_rotation(Quat::from_rotation_y(rng_next(&mut rng) * TAU)),
            BiomeEntity,
        ));
        crate::blockers::add_obb(x, z, 1.2, 1.0, 0.0);
        info!("bog: stilt hut at {x:.1},{z:.1}");
    }
    if trees == 0 || wisps == 0 || mush == 0 {
        warn!("bog: dressing under-placed ({trees} trees / {wisps} wisps / {mush} mush) — pool_sd gates vs POOL_T mismatch?");
    }
    info!("bog: {trees} drowned trees, {wisps} wisps, {mush} glow-mushroom clusters");
}
