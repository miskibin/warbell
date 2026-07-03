//! **Wayside furniture** (map-character overhaul pass 2) — the small stuff that makes a road
//! read as a *used route* instead of a dirt smear: signposts at every network junction, and a
//! seeded march of mile cairns / fence runs / wayside shrines spaced along the cart arteries.
//! KCD's "roadside grammar" at low-poly cost: none of it is interactive, all of it is
//! navigation legibility (a junction is a decision point — the post marks it; furniture ticks
//! by as you travel so progress is felt).
//!
//! Build contract: each piece is one merged, vertex-coloured, flat-shaded mesh from primitives
//! (`ruins.rs`/`vignettes.rs` contract — base at y=0, one shared white material, so the whole
//! layer auto-batches per kind). Placement is deterministic (mulberry32 on a fixed seed) and
//! rejects water / roads themselves / build plots / clearings / blocked or steep ground. Pieces
//! are cover-class: NO nav blockers — invader steering and villager pathing must never snag on
//! a fence post planted beside the highway.
//!
//! Signpost boards are unlabeled arrows (world-space text is out of scope for the pass); the
//! post itself marking "a fork with destinations" is the legibility win.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::palette::lin;

// ── Mesh helpers (ruins/vignettes contract) ─────────────────────────────────────────
fn tint(mut m: Mesh, col: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(col); n]);
    m
}
fn bx(w: f32, h: f32, d: f32, c: Vec3, col: u32) -> Mesh {
    tint(Cuboid::new(w, h, d).mesh().build().translated_by(c), col)
}
fn cyl(r: f32, h: f32, c: Vec3, col: u32) -> Mesh {
    tint(Cylinder::new(r, h).mesh().resolution(7).build().translated_by(c), col)
}
fn ball(r: f32, c: Vec3, squash: f32, col: u32) -> Mesh {
    tint(
        Sphere::new(r).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.0, squash, 1.0)).translated_by(c),
        col,
    )
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

const POST: u32 = 0x6a4e2e;
const BOARD: u32 = 0x8a6a40;
const STONE_A: u32 = 0x8d9097;
const STONE_B: u32 = 0x767a82;
const SHRINE_STONE: u32 = 0x9a938a;
const ROOF: u32 = 0x5c4326;
const CANDLE: u32 = 0xd9a13c;

/// Junction signpost: a post with two arrow boards at different heights/headings.
fn signpost() -> Mesh {
    let mut v = Vec::new();
    v.push(cyl(0.065, 2.05, Vec3::new(0.0, 1.02, 0.0), POST));
    v.push(ball(0.09, Vec3::new(0.0, 2.08, 0.0), 0.8, POST)); // cap
    for (y, yaw) in [(1.62_f32, 0.35_f32), (1.38, -2.2)] {
        // Board + a pointed tip wedge so it reads as an arrow, both swung to their heading.
        let rot = Quat::from_rotation_y(yaw);
        v.push(tint(
            Cuboid::new(0.72, 0.15, 0.045).mesh().build().translated_by(Vec3::new(0.30, 0.0, 0.0)).rotated_by(rot).translated_by(Vec3::new(0.0, y, 0.0)),
            BOARD,
        ));
        v.push(tint(
            Cuboid::new(0.16, 0.15, 0.045)
                .mesh()
                .build()
                .rotated_by(Quat::from_rotation_y(0.785))
                .translated_by(Vec3::new(0.70, 0.0, 0.0))
                .rotated_by(rot)
                .translated_by(Vec3::new(0.0, y, 0.0)),
            BOARD,
        ));
    }
    assemble(v)
}

/// Mile cairn: a stack of squashed stones.
fn cairn() -> Mesh {
    let mut v = Vec::new();
    for (i, (r, y)) in [(0.30_f32, 0.10_f32), (0.24, 0.30), (0.18, 0.47), (0.12, 0.60)].into_iter().enumerate() {
        let col = if i % 2 == 0 { STONE_A } else { STONE_B };
        v.push(ball(r, Vec3::new((i as f32 * 0.7).sin() * 0.04, y, (i as f32 * 1.3).cos() * 0.04), 0.62, col));
    }
    assemble(v)
}

/// Short fence run: three posts + two rails, ~2.2u long along local X.
fn fence() -> Mesh {
    let mut v = Vec::new();
    for i in -1..=1 {
        v.push(cyl(0.05, 0.62, Vec3::new(i as f32 * 1.0, 0.31, 0.0), POST));
    }
    for y in [0.28_f32, 0.50] {
        v.push(tint(
            Cylinder::new(0.032, 2.25).mesh().resolution(6).build().rotated_by(Quat::from_rotation_z(FRAC_PI_2)).translated_by(Vec3::new(0.0, y, 0.0)),
            BOARD,
        ));
    }
    assemble(v)
}

/// Wayside shrine: stone plinth + niche + peaked wooden roof + a warm candle dot inside.
fn shrine() -> Mesh {
    let mut v = Vec::new();
    v.push(bx(0.52, 0.36, 0.52, Vec3::new(0.0, 0.18, 0.0), SHRINE_STONE));
    v.push(bx(0.36, 0.44, 0.30, Vec3::new(0.0, 0.56, 0.0), SHRINE_STONE));
    v.push(ball(0.055, Vec3::new(0.0, 0.62, 0.12), 1.0, CANDLE));
    for s in [-1.0_f32, 1.0] {
        v.push(tint(
            Cuboid::new(0.34, 0.05, 0.46).mesh().build().rotated_by(Quat::from_rotation_z(s * 0.62)).translated_by(Vec3::new(s * 0.14, 0.90, 0.0)),
            ROOF,
        ));
    }
    assemble(v)
}

// ── Placement ────────────────────────────────────────────────────────────────────────
fn rng_next(state: &mut u32) -> f32 {
    *state = state.wrapping_add(0x6d2b_79f5);
    let mut t = *state;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}

/// A spot is usable if it's on land, off the road surface itself, and clear of everything that
/// owns its ground. Steepness is rejected with a 4-probe spread (furniture on a slope floats).
fn spot_ok(x: f32, z: f32) -> bool {
    let Some(y0) = crate::worldmap::ground_at_world(x, z) else { return false };
    for (dx, dz) in [(0.45_f32, 0.0_f32), (-0.45, 0.0), (0.0, 0.45), (0.0, -0.45)] {
        match crate::worldmap::ground_at_world(x + dx, z + dz) {
            Some(y) if (y - y0).abs() <= 0.30 => {}
            _ => return false,
        }
    }
    !crate::roads::on_road(x, z)
        && !crate::blockers::is_blocked(x, z)
        && !crate::town::near_build_plot(x, z)
        && !crate::castle::in_footprint(x, z)
        && !crate::camps::in_clearing(x, z)
        && !crate::rival::near_fort(x, z)
        && !crate::worldmap::cliff_shelf_world(x, z)
}

/// Plant signposts at road junctions + cairn/fence/shrine furniture along the arteries.
/// Called from `worldmap::build_step` (phase 30) — after roads, camps, plots and landmarks, so
/// every rejection query above is baked.
pub fn populate(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    use crate::biome::BiomeEntity;
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    let range = bevy::camera::visibility::VisibilityRange {
        start_margin: 0.0..0.0,
        end_margin: 95.0..95.0, // abrupt, inside the fog ramp (see biome.rs cover_range notes)
        use_aabb: true,
    };
    let h_sign = meshes.add(signpost());
    let h_cairn = meshes.add(cairn());
    let h_fence = meshes.add(fence());
    let h_shrine = meshes.add(shrine());
    let mut rng: u32 = 0x3a5f_11d7;
    let spawn = |commands: &mut Commands, handle: &Handle<Mesh>, x: f32, y: f32, z: f32, yaw: f32, s: f32| {
        commands.spawn((
            Mesh3d(handle.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(s)),
            BiomeEntity,
            range.clone(),
        ));
    };

    // 1. Signposts: one just off each junction (probe a ring of offsets for a legal spot).
    let mut posts = 0;
    for j in crate::roads::junctions() {
        let base = rng_next(&mut rng) * TAU;
        let mut placed = false;
        // Two probe rings — a handful of junctions (deep swamp / dune forks) fail the whole
        // inner ring on slope/water and went bare with a single-radius probe.
        for k in 0..16 {
            let (a, r) = (base + (k % 8) as f32 * (TAU / 8.0), if k < 8 { 2.3 } else { 3.6 });
            let (x, z) = (j.x + a.cos() * r, j.y + a.sin() * r);
            if spot_ok(x, z) {
                let y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
                spawn(commands, &h_sign, x, y, z, rng_next(&mut rng) * TAU, 0.95 + rng_next(&mut rng) * 0.15);
                posts += 1;
                placed = true;
                break;
            }
        }
        if !placed {
            info!("wayside: no legal signpost spot at junction {:.0},{:.0}", j.x, j.y);
        }
    }

    // 2. Furniture along the arteries: arc-length walk, one piece every ~26–40u, alternating
    //    sides, offset just past the road shoulder.
    let mut pieces = 0;
    let mut last: Vec<Vec2> = Vec::new();
    for poly in crate::roads::artery_polylines() {
        let mut next_at = 14.0 + rng_next(&mut rng) * 18.0;
        let mut travelled = 0.0_f32;
        let mut side = if rng_next(&mut rng) < 0.5 { 1.0_f32 } else { -1.0 };
        for w in poly.windows(2) {
            let seg = w[0].distance(w[1]);
            if seg < 1e-3 {
                continue;
            }
            while travelled + seg >= next_at {
                let t = (next_at - travelled) / seg;
                let p = w[0].lerp(w[1], t);
                let dir = (w[1] - w[0]) / seg;
                let perp = Vec2::new(-dir.y, dir.x);
                let off = 2.6 + rng_next(&mut rng) * 1.0;
                let q = p + perp * off * side;
                side = -side;
                next_at += 26.0 + rng_next(&mut rng) * 14.0;
                if !spot_ok(q.x, q.y) || last.iter().any(|l| l.distance(q) < 9.0) {
                    continue;
                }
                let y = crate::worldmap::ground_at_world(q.x, q.y).unwrap_or(0.0);
                let roll = rng_next(&mut rng);
                let road_yaw = dir.y.atan2(dir.x); // world yaw aligning local X with the road
                if roll < 0.38 {
                    spawn(commands, &h_cairn, q.x, y, q.y, rng_next(&mut rng) * TAU, 0.85 + rng_next(&mut rng) * 0.4);
                } else if roll < 0.72 {
                    spawn(commands, &h_fence, q.x, y, q.y, -road_yaw, 0.9 + rng_next(&mut rng) * 0.25);
                } else {
                    spawn(commands, &h_shrine, q.x, y, q.y, -road_yaw + FRAC_PI_2, 0.95 + rng_next(&mut rng) * 0.2);
                }
                last.push(q);
                pieces += 1;
            }
            travelled += seg;
        }
    }
    info!("wayside: {posts} signposts, {pieces} roadside pieces");
}
