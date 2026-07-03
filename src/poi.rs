//! **Micro-POIs + flags** (map-character overhaul pass 4) — the 40-second-rule layer: bigger
//! roadside set-pieces spaced along the arteries so a traveller always has something ahead
//! (CDPR's rule: something interesting every ~40 s of travel), plus the "weenie" FLAGS that
//! pull the eye from far away — smoke columns over Gnashfang Hold and crows wheeling above it —
//! and working farmland (tilled fields + fences) around the castle's build-plot ring so the
//! meadow reads as a lived-in settlement instead of a bald lawn.
//!
//! Set-pieces (all vertex-coloured merged primitives, base y=0, ruins.rs contract): a gallows
//! frame on the fortress approach, a burned-out cabin, wayside graves, a standing-stone circle,
//! a collapsed wooden watchtower, a shepherd's hut with a pen. Environmental storytelling à la
//! RDR2 — no interaction, no text; each is a told-without-words scene beside the road.
//!
//! Placement: deterministic arc-length walk along the artery polylines (like `wayside`, but
//! ~90–130u spacing and 6–11u OFF the road), rejecting water/roads/plots/clearings/mesa
//! shelves/blockers. The gallows is special-cased onto the Gnashfang approach road.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::lin;

pub struct PoiPlugin;
impl Plugin for PoiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rise_smoke, wheel_crows));
    }
}

// ── Flags: smoke + crows ────────────────────────────────────────────────────────────
/// One rising smoke mote of a tall column (the cheapest far-visible "weenie" there is).
#[derive(Component)]
struct SmokeMote {
    base: Vec3,
    speed: f32,
    phase: f32,
    /// Column height this mote cycles over (taller than a campfire's little puffs).
    rise: f32,
}

fn rise_smoke(time: Res<Time>, mut q: Query<(&SmokeMote, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (s, mut tf) in &mut q {
        let cycle = (t * s.speed + s.phase).rem_euclid(1.0);
        tf.translation = s.base
            + Vec3::new(
                (t * 0.35 + s.phase * 5.0).sin() * (0.4 + cycle * 1.6),
                cycle * s.rise,
                (t * 0.30 + s.phase * 4.0).cos() * (0.4 + cycle * 1.6),
            );
        // Grow then thin out: big soft puffs mid-column, dissolving at the top.
        let sc = (0.5 + cycle * 1.7) * (1.0 - cycle * cycle).max(0.0);
        tf.scale = Vec3::splat(sc.max(0.01));
    }
}

/// A crow wheeling on a slow circle — a handful over the fortress sell "something dead lives
/// there" from across the island.
#[derive(Component)]
struct Crow {
    centre: Vec3,
    radius: f32,
    speed: f32,
    phase: f32,
}

fn wheel_crows(time: Res<Time>, mut q: Query<(&Crow, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (c, mut tf) in &mut q {
        let a = t * c.speed + c.phase;
        let bob = (t * 1.7 + c.phase * 3.0).sin() * 0.8;
        tf.translation = c.centre + Vec3::new(a.cos() * c.radius, bob, a.sin() * c.radius);
        // Face along the flight direction (tangent).
        tf.rotation = Quat::from_rotation_y(-a - FRAC_PI_2);
    }
}

// ── Mesh helpers (ruins/vignettes contract) ─────────────────────────────────────────
fn tint(mut m: Mesh, col: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(col); n]);
    m
}
fn bx(w: f32, h: f32, d: f32, c: Vec3, col: u32) -> Mesh {
    tint(Cuboid::new(w, h, d).mesh().build().translated_by(c), col)
}
fn bxr(w: f32, h: f32, d: f32, rot: Quat, c: Vec3, col: u32) -> Mesh {
    tint(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(c), col)
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

const WOOD: u32 = 0x5c4226;
const WOOD_DK: u32 = 0x453018;
const CHAR: u32 = 0x1f1a14;
const ASH: u32 = 0x4a463e;
const STONE: u32 = 0x83878d;
const STONE_DK: u32 = 0x64686e;
const MOUND: u32 = 0x5d4a32;
const ROPE: u32 = 0x8f7b52;
const THATCH: u32 = 0x8c7440;

/// Gallows frame on the fortress approach: two posts, a crossbeam, a frayed rope stub swaying
/// over a trapdoor platform. Grim and empty — the story tells itself.
fn gallows() -> Mesh {
    let mut v = Vec::new();
    v.push(bx(2.2, 0.28, 1.6, Vec3::new(0.0, 0.5, 0.0), WOOD_DK)); // platform
    for sx in [-0.9_f32, 0.9] {
        v.push(bx(0.3, 0.5, 1.3, Vec3::new(sx, 0.2, 0.0), WOOD)); // platform legs
    }
    v.push(cyl(0.09, 2.6, Vec3::new(-0.85, 1.9, 0.0), WOOD)); // upright
    v.push(bxr(1.9, 0.14, 0.14, Quat::IDENTITY, Vec3::new(0.05, 3.15, 0.0), WOOD)); // crossbeam
    v.push(bxr(0.5, 0.12, 0.12, Quat::from_rotation_z(-0.7), Vec3::new(-0.55, 2.95, 0.0), WOOD_DK)); // brace
    v.push(cyl(0.025, 0.55, Vec3::new(0.72, 2.85, 0.0), ROPE)); // rope stub
    v.push(ball(0.06, Vec3::new(0.72, 2.56, 0.0), 1.0, ROPE)); // knot
    assemble(v)
}

/// Burned-out cabin: charred corner posts, one collapsed wall, fallen roof beams in an ash bed.
fn burned_cabin() -> Mesh {
    let mut v = Vec::new();
    v.push(ball(1.6, Vec3::new(0.0, 0.04, 0.0), 0.16, ASH)); // ash bed
    for (sx, sz, h) in [(-1.1_f32, -0.9_f32, 1.5_f32), (1.1, -0.9, 1.1), (-1.1, 0.9, 0.8), (1.1, 0.9, 1.3)] {
        v.push(bxr(0.22, h, 0.22, Quat::from_rotation_x(0.06) * Quat::from_rotation_z(0.05), Vec3::new(sx, h * 0.5, sz), CHAR));
    }
    v.push(bx(2.4, 0.7, 0.16, Vec3::new(0.0, 0.35, -0.95), CHAR)); // surviving half-wall
    for (a, y) in [(0.5_f32, 0.28_f32), (-0.9, 0.2), (1.9, 0.35)] {
        v.push(bxr(2.0, 0.12, 0.12, Quat::from_rotation_y(a) * Quat::from_rotation_z(0.10), Vec3::new(0.1, y, 0.1), CHAR));
    }
    v.push(bx(0.5, 0.4, 0.4, Vec3::new(0.9, 0.2, 0.6), WOOD_DK)); // one unburnt crate
    assemble(v)
}

/// A pair of wayside graves: mounds, a wooden cross, a headstone.
fn graves() -> Mesh {
    let mut v = Vec::new();
    for (dx, rot) in [(-0.7_f32, 0.12_f32), (0.7, -0.08)] {
        v.push(tint(
            Sphere::new(0.55).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.0, 0.35, 1.7)).rotated_by(Quat::from_rotation_y(rot)).translated_by(Vec3::new(dx, 0.1, 0.0)),
            MOUND,
        ));
    }
    v.push(cyl(0.05, 0.9, Vec3::new(-0.7, 0.45, -0.75), WOOD)); // cross upright
    v.push(bx(0.5, 0.09, 0.09, Vec3::new(-0.7, 0.68, -0.75), WOOD)); // cross arm
    v.push(bxr(0.5, 0.6, 0.14, Quat::from_rotation_z(0.08), Vec3::new(0.7, 0.3, -0.8), STONE)); // headstone
    assemble(v)
}

/// Standing-stone circle: 6 weathered monoliths, one toppled.
fn standing_stones() -> Mesh {
    let mut v = Vec::new();
    for k in 0..6 {
        let a = k as f32 * (TAU / 6.0);
        let (r, h) = (2.3, 1.2 + (k as f32 * 2.1).sin().abs() * 0.9);
        if k == 4 {
            // The fallen one.
            v.push(bxr(0.6, 0.35, 1.6, Quat::from_rotation_y(a + 0.5), Vec3::new(a.cos() * r, 0.18, a.sin() * r), STONE_DK));
        } else {
            v.push(bxr(
                0.55,
                h,
                0.35,
                Quat::from_rotation_y(a) * Quat::from_rotation_z((k as f32 * 1.3).sin() * 0.08),
                Vec3::new(a.cos() * r, h * 0.5, a.sin() * r),
                if k % 2 == 0 { STONE } else { STONE_DK },
            ));
        }
    }
    v.push(ball(0.5, Vec3::ZERO, 0.3, STONE_DK)); // centre altar stump
    assemble(v)
}

/// Collapsed WOODEN watchtower (the stone one is a Rocky vignette): a leaning frame, snapped
/// legs, the fallen platform against it.
fn fallen_watchtower() -> Mesh {
    let lean = Quat::from_rotation_z(0.22);
    let mut v = Vec::new();
    for (sx, sz) in [(-0.7_f32, -0.7_f32), (0.7, -0.7), (-0.7, 0.7)] {
        v.push(bxr(0.16, 2.6, 0.16, lean, Vec3::new(sx + 0.25, 1.25, sz), WOOD));
    }
    v.push(bxr(0.16, 1.1, 0.16, Quat::from_rotation_z(1.1), Vec3::new(1.3, 0.4, 0.7), WOOD_DK)); // snapped leg
    v.push(bxr(1.7, 0.12, 1.7, lean * Quat::from_rotation_x(0.15), Vec3::new(0.55, 2.55, 0.0), WOOD_DK)); // tilted platform
    v.push(bxr(1.5, 0.7, 0.1, Quat::from_rotation_z(0.9), Vec3::new(1.6, 0.5, -0.4), WOOD)); // fallen parapet
    for (a, l) in [(0.3_f32, 1.4_f32), (-0.8, 1.1)] {
        v.push(bxr(l, 0.1, 0.1, Quat::from_rotation_y(a), Vec3::new(0.6, 0.1, 0.5), WOOD_DK)); // debris
    }
    assemble(v)
}

/// Shepherd's hut + a small fenced pen.
fn shepherd_hut() -> Mesh {
    let mut v = Vec::new();
    v.push(bx(1.6, 1.0, 1.3, Vec3::new(-1.0, 0.5, 0.0), WOOD)); // hut body
    for s in [-1.0_f32, 1.0] {
        v.push(bxr(1.05, 0.07, 1.6, Quat::from_rotation_z(s * 0.55), Vec3::new(-1.0 + s * 0.42, 1.25, 0.0), THATCH));
    }
    v.push(bx(0.4, 0.55, 0.06, Vec3::new(-1.0, 0.28, 0.66), WOOD_DK)); // door
    // Pen: posts + rails around a small square to the east.
    for (px, pz) in [(0.2_f32, -1.0_f32), (1.8, -1.0), (0.2, 1.0), (1.8, 1.0)] {
        v.push(cyl(0.05, 0.6, Vec3::new(px, 0.3, pz), WOOD_DK));
    }
    for (c, rot, l) in [
        (Vec3::new(1.0, 0.42, -1.0), 0.0_f32, 1.6_f32),
        (Vec3::new(1.0, 0.42, 1.0), 0.0, 1.6),
        (Vec3::new(1.8, 0.42, 0.0), FRAC_PI_2, 2.0),
    ] {
        v.push(tint(
            Cylinder::new(0.03, l).mesh().resolution(6).build().rotated_by(Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_y(rot)).translated_by(c),
            WOOD,
        ));
    }
    assemble(v)
}

/// A tilled field: dark earth bed with raised crop rows, framed by a low post fence.
fn field() -> Mesh {
    let mut v = Vec::new();
    v.push(bx(6.4, 0.12, 4.4, Vec3::new(0.0, 0.05, 0.0), 0x3f2f1c)); // tilled bed
    for k in 0..6 {
        let z = -1.75 + k as f32 * 0.7;
        v.push(bx(5.9, 0.14, 0.3, Vec3::new(0.0, 0.14, z), if k % 2 == 0 { 0x4d3a22 } else { 0x57432a }));
        // Sparse green sprouts on the rows.
        for s in 0..5 {
            let x = -2.5 + s as f32 * 1.25 + (k as f32 * 0.7).sin() * 0.3;
            v.push(ball(0.14, Vec3::new(x, 0.26, z), 0.9, 0x5f8f3a));
        }
    }
    for (px, pz) in [(-3.2_f32, -2.2_f32), (3.2, -2.2), (-3.2, 2.2), (3.2, 2.2)] {
        v.push(cyl(0.05, 0.5, Vec3::new(px, 0.25, pz), WOOD_DK));
    }
    for (c, yaw, l) in [
        (Vec3::new(0.0, 0.38, -2.2), 0.0_f32, 6.4_f32),
        (Vec3::new(0.0, 0.38, 2.2), 0.0, 6.4),
        (Vec3::new(-3.2, 0.38, 0.0), FRAC_PI_2, 4.4),
        (Vec3::new(3.2, 0.38, 0.0), FRAC_PI_2, 4.4),
    ] {
        v.push(tint(
            Cylinder::new(0.03, l).mesh().resolution(6).build().rotated_by(Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_y(yaw)).translated_by(c),
            WOOD,
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

/// Flat, unclaimed, off-road land — same discipline as `wayside::spot_ok` but with a wider
/// flatness probe (these set-pieces have real footprints).
fn spot_ok(x: f32, z: f32, probe: f32) -> bool {
    let Some(y0) = crate::worldmap::ground_at_world(x, z) else { return false };
    for k in 0..6 {
        let a = k as f32 * (TAU / 6.0);
        match crate::worldmap::ground_at_world(x + a.cos() * probe, z + a.sin() * probe) {
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
        && !crate::worldmap::is_pool_world(x, z)
        && !crate::bridges::near_bridge(x, z, 2.0)
}

/// Build-phase 32 entry: set-pieces along the arteries + the gallows + fortress flags + fields.
pub fn populate(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: 120.0..120.0, use_aabb: true };
    let mut rng: u32 = 0x9017_44aa;

    // 1. Story set-pieces along the arteries, one every ~90–130u of arc, 6–11u off the road.
    //    (The 40s-rule gap-filler between the big POIs — camps/landmarks/vignettes — and the
    //    small wayside furniture.)
    let kinds: [(Mesh, f32); 5] =
        [(burned_cabin(), 1.9), (graves(), 1.2), (standing_stones(), 2.9), (fallen_watchtower(), 1.9), (shepherd_hut(), 2.3)];
    let handles: Vec<(Handle<Mesh>, f32)> = kinds.into_iter().map(|(m, r)| (meshes.add(m), r)).collect();
    let mut placed: Vec<Vec2> = Vec::new();
    let mut ki = 0;
    for poly in crate::roads::artery_polylines() {
        let mut next_at = 45.0 + rng_next(&mut rng) * 60.0;
        let mut travelled = 0.0_f32;
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
                next_at += 90.0 + rng_next(&mut rng) * 40.0;
                let side = if rng_next(&mut rng) < 0.5 { 1.0 } else { -1.0 };
                let (handle, foot) = &handles[ki % handles.len()];
                // Try a few offsets before giving up on this interval — a single rigid probe
                // rejected most spots (tree blockers hug the verges) and only ~3 pieces landed
                // across the whole island.
                let Some(q) = [(side, 6.5_f32), (-side, 6.5), (side, 9.5), (-side, 9.5)]
                    .into_iter()
                    .map(|(s, d)| p + perp * d * s)
                    .find(|q| spot_ok(q.x, q.y, *foot) && placed.iter().all(|l| l.distance(*q) >= 55.0))
                else {
                    continue;
                };
                ki += 1;
                let y = crate::worldmap::ground_at_world(q.x, q.y).unwrap_or(0.0);
                commands.spawn((
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(q.x, y, q.y).with_rotation(Quat::from_rotation_y(rng_next(&mut rng) * TAU)),
                    BiomeEntity,
                    range.clone(),
                ));
                crate::blockers::add_obb(q.x, q.y, *foot * 0.7, *foot * 0.7, 0.0);
                placed.push(q);
            }
            travelled += seg;
        }
    }

    // 2. The GALLOWS on the Gnashfang approach: walk back up the fortress spur from the gate
    //    and stand it just off the road — the message you read on the way in.
    let gate = crate::ork_fortress::GATE;
    let mut best: Option<Vec2> = None;
    for poly in crate::roads::artery_polylines() {
        let Some(end) = poly.last() else { continue };
        if end.distance(gate) > 12.0 && poly.first().map_or(true, |f| f.distance(gate) > 12.0) {
            continue; // not the fortress spur
        }
        // Walk ~30u back from the gate end.
        let pts: Vec<Vec2> = if poly.last().unwrap().distance(gate) <= 12.0 {
            poly.iter().rev().copied().collect()
        } else {
            poly.clone()
        };
        // Scan a WINDOW of the approach (18–50u back from the gate), several offsets per stop —
        // the Blight approach is uneven and partly waterlogged, and a single rigid 30u probe
        // never found footing (verification: "no spot for the gallows" every run).
        let mut acc = 0.0;
        'walk: for w in pts.windows(2) {
            acc += w[0].distance(w[1]);
            if !(18.0..=50.0).contains(&acc) {
                continue;
            }
            let dir = (w[1] - w[0]).normalize_or_zero();
            let perp = Vec2::new(-dir.y, dir.x);
            for (side, d) in [(1.0_f32, 4.5_f32), (-1.0, 4.5), (1.0, 6.5), (-1.0, 6.5)] {
                let q = w[1] + perp * d * side;
                if spot_ok(q.x, q.y, 1.2) {
                    best = Some(q);
                    break 'walk;
                }
            }
        }
        if best.is_some() {
            break;
        }
    }
    if let Some(q) = best {
        let y = crate::worldmap::ground_at_world(q.x, q.y).unwrap_or(0.0);
        // Face the frame across the road (rotation roughly toward the gate).
        let yaw = (gate.x - q.x).atan2(gate.y - q.y);
        commands.spawn((
            Mesh3d(meshes.add(gallows())),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(q.x, y, q.y).with_rotation(Quat::from_rotation_y(yaw)),
            BiomeEntity,
            range.clone(),
        ));
        crate::blockers::add_obb(q.x, q.y, 1.2, 0.9, yaw);
        info!("poi: gallows at {:.1},{:.1}", q.x, q.y);
    } else {
        warn!("poi: no spot for the gallows on the fortress approach");
    }

    // 3. FLAGS over Gnashfang Hold: a tall smoke column + wheeling crows, visible far across
    //    the island (sightline weenies — cheap particles/meshes, no lighting cost).
    let hold = crate::ork_fortress::CENTRE;
    // Near-black, denser, taller than the first cut — grey-on-grey it vanished against the
    // hazy sky + the snow massif behind the Hold (verification flag).
    let smoke_mesh = meshes.add(Sphere::new(1.15).mesh().ico(1).unwrap());
    let smoke_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.10, 0.095, 0.09, 0.8),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let hold_y = crate::worldmap::ground_at_world(hold.x, hold.y).unwrap_or(1.0);
    for k in 0..10 {
        let phase = k as f32 / 10.0;
        commands.spawn((
            Mesh3d(smoke_mesh.clone()),
            MeshMaterial3d(smoke_mat.clone()),
            Transform::from_xyz(hold.x, hold_y + 3.0, hold.y),
            SmokeMote {
                base: Vec3::new(hold.x + 1.0, hold_y + 3.0, hold.y - 1.5),
                speed: 0.055 + phase * 0.01,
                phase: phase * 10.0,
                rise: 22.0,
            },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
    let crow_mesh = meshes.add(crow_mesh());
    let crow_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.06), unlit: true, ..default() });
    for k in 0..5 {
        let phase = k as f32 * (TAU / 5.0);
        commands.spawn((
            Mesh3d(crow_mesh.clone()),
            MeshMaterial3d(crow_mat.clone()),
            Transform::from_xyz(hold.x, hold_y + 14.0, hold.y),
            Crow {
                centre: Vec3::new(hold.x, hold_y + 13.0 + (k as f32 * 1.3).sin() * 2.0, hold.y),
                radius: 9.0 + (k as f32 * 2.7).sin() * 3.0,
                speed: 0.28 + k as f32 * 0.03,
                phase,
            },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }

    // 4. Farmland around the castle: tilled fields in the diagonal gaps outside the build-plot
    //    corner ring (still on the forced-flat grass apron), so the meadow reads worked.
    let field_mesh = meshes.add(field());
    let mut fields = 0;
    for (fx, fz) in [(27.0_f32, 15.0_f32), (-27.0, 15.0), (27.0, -15.0), (-16.0, 27.0), (16.0, -27.0)] {
        if !spot_ok(fx, fz, 3.4) {
            continue;
        }
        let y = crate::worldmap::ground_at_world(fx, fz).unwrap_or(0.0);
        let yaw = if fx.abs() > fz.abs() { 0.0 } else { FRAC_PI_2 };
        commands.spawn((
            Mesh3d(field_mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(fx, y + 0.01, fz).with_rotation(Quat::from_rotation_y(yaw)),
            BiomeEntity,
            range.clone(),
        ));
        fields += 1;
    }
    info!("poi: {} set-pieces, {fields} fields, smoke + crows over the Hold", placed.len());
}

/// A tiny low-poly crow: body + swept wings (rotated so local -Z is the flight direction).
fn crow_mesh() -> Mesh {
    let mut v = Vec::new();
    v.push(tint(
        Sphere::new(0.16).mesh().ico(1).unwrap().scaled_by(Vec3::new(0.8, 0.7, 1.5)),
        0x0a0a0c,
    ));
    for s in [-1.0_f32, 1.0] {
        v.push(bxr(
            0.85,
            0.03,
            0.26,
            Quat::from_rotation_z(s * 0.28),
            Vec3::new(s * 0.45, 0.05, 0.0),
            0x0a0a0c,
        ));
    }
    assemble(v)
}
