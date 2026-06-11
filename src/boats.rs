//! Background sailboats drifting on the open ocean. A handful of low-poly hulls patrol
//! back and forth along the ocean arc (the sea sector `sea.rs` lays down), bobbing on the
//! swell with a gentle roll/pitch so the horizon never feels dead. Pure decoration — no
//! collision, no gameplay; they live only over deep water (radius ≥ ocean inner edge) so
//! they never sail onto land.
//!
//! CONTRACT (matches `decor.rs` / `props.rs`):
//! - Each boat mesh is built ONCE, base at the waterline (y=0 local → sits at `SEA_Y`).
//!   Parts are vertex-coloured via `palette::lin`, merged with `Mesh::merge`, then
//!   flat-shaded for the crisp low-poly facets.
//! - All boats share ONE white vertex-colour `StandardMaterial` (double-sided so the
//!   hand-wound hull/sail show from any angle) → the renderer batches them.
//! - Spawned from `biome.rs` right after the sea (only when `backdrop.ocean`), tagged
//!   [`BiomeEntity`] so a biome switch wipes them. Deterministic Mulberry32 placement.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::lin;

const TAU: f32 = std::f32::consts::TAU;
const SEED: u32 = 0xb0a7;
const N_BOATS_MAP: usize = 9; // world-map fleet size

// ── Hull form (local: bow at +X, mast up +Y, beam along ±Z) ──
const STERN_X: f32 = -1.6;
const BOW_X: f32 = 1.9;
const BEAM: f32 = 0.62; // half-width amidships
const RAIL: f32 = 0.17; // deck/gunwale height above the waterline
const DEPTH: f32 = 0.20; // keel depth below the waterline
const STATIONS: usize = 8;

/// Per-variant palette so the little fleet isn't all identical.
struct Palette {
    hull: u32,
    deck: u32,
    cabin: u32,
    mast: u32,
    sail: u32,
}
const PALETTES: [Palette; 3] = [
    Palette { hull: 0x5a3a22, deck: 0x7a5636, cabin: 0x8a4a2a, mast: 0x3a2a1a, sail: 0xede3c8 },
    Palette { hull: 0x4a3326, deck: 0x6b4a2a, cabin: 0x9a3030, mast: 0x3a2a1a, sail: 0xd8c8a8 },
    Palette { hull: 0x6b4a2a, deck: 0x8a6a44, cabin: 0x3a5a7a, mast: 0x2f2114, sail: 0xf2ecd8 },
];

// ── Mulberry32 RNG (same recipe as scatter.rs/decor.rs so layouts stay stable) ──
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next() * (hi - lo)
    }
}

/// A drifting hull tracing an ellipse `(cx,cz) + (rx·cosφ, rz·sinφ)` on the sea. When
/// `wrap` it orbits the island full-circle; otherwise it patrols (bounces) between the
/// `lo`/`hi` bearings of an open-sea wedge. `boat_drift` advances φ and rides the swell.
#[derive(Component)]
struct Boat {
    cx: f32,
    cz: f32,
    rx: f32,
    rz: f32,
    phi: f32,
    omega: f32, // angular speed (signed)
    wrap: bool, // true = full orbit, false = bounce in [lo,hi]
    lo: f32,
    hi: f32,
    sea_y: f32,
    bob_amp: f32,
    bob_freq: f32,
    phase: f32,
}

pub struct BoatsPlugin;

impl Plugin for BoatsPlugin {
    fn build(&self, app: &mut App) {
        // Ambient: keep drifting even in menus (like the firefly bob).
        app.add_systems(Update, boat_drift);
    }
}

/// One shared white vertex-colour material (double-sided so the hand-wound hull + sail
/// show from any angle) plus the three hull-mesh variants. Built once per spawn call.
fn fleet_assets(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> (Handle<StandardMaterial>, Vec<Handle<Mesh>>) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.7,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let variants = (0..PALETTES.len()).map(|v| meshes.add(build_boat(v))).collect();
    (mat, variants)
}

/// Spawn a fleet over the world-map's open sea. Each boat patrols an elliptical arc on the
/// three ocean sides (the north is the forest/river edge), bobbing on the swell. Tagged
/// [`BiomeEntity`].
pub fn spawn_boats_island(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    center: Vec2,
    radii: Vec2,
    sea_y: f32,
) {
    // Boats only sail the OCEAN (3 sides); the north (-Z) is the forest/river edge. Patrol
    // the ocean arc, bouncing back before reaching the forest side. North = -π/2.
    let north = -std::f32::consts::FRAC_PI_2;
    let forest_half_arc = 1.05; // ~60° each way of the north is land — boats keep out
    let lo = north + forest_half_arc;
    let hi = north - forest_half_arc + TAU;

    let (mat, variants) = fleet_assets(meshes, materials);
    let mut rng = Rng(SEED ^ 0x5151);
    for k in 0..N_BOATS_MAP {
        // Orbit-ellipse factor: ≥1.2 clears the island (incl. the noisy coast); kept modest
        // so the boats read as near water in front of the open horizon.
        let kf = rng.range(1.22, 1.6);
        let dir = if rng.next() < 0.5 { -1.0 } else { 1.0 };
        // Don't spawn inside the ork fortress's bay (south sea) — resample the bearing.
        let mut phi = rng.range(lo, hi);
        for _ in 0..24 {
            let (s, c) = phi.sin_cos();
            if !crate::ork_fortress::boat_keepout(center.x + c * radii.x * kf, center.y + s * radii.y * kf) {
                break;
            }
            phi = rng.range(lo, hi);
        }
        let boat = Boat {
            cx: center.x,
            cz: center.y,
            rx: radii.x * kf,
            rz: radii.y * kf,
            phi,
            omega: dir * rng.range(0.008, 0.022), // slow drift along the ocean arc
            wrap: false,
            lo,
            hi,
            sea_y,
            bob_amp: rng.range(0.03, 0.08),
            bob_freq: rng.range(0.5, 0.95),
            phase: rng.range(0.0, TAU),
        };
        spawn_boat(commands, &mat, &variants, k, boat, rng.range(1.3, 2.4));
    }
}

fn spawn_boat(
    commands: &mut Commands,
    mat: &Handle<StandardMaterial>,
    variants: &[Handle<Mesh>],
    k: usize,
    boat: Boat,
    scale: f32,
) {
    let (s, c) = boat.phi.sin_cos();
    let pos = Vec3::new(boat.cx + c * boat.rx, boat.sea_y, boat.cz + s * boat.rz);
    commands.spawn((
        Mesh3d(variants[k % variants.len()].clone()),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
        boat,
        BiomeEntity,
    ));
}

/// Advance each boat along its ellipse (orbiting or bouncing) and ride the swell.
fn boat_drift(time: Res<Time>, mut q: Query<(&mut Boat, &mut Transform)>) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    for (mut bo, mut tf) in &mut q {
        let prev_phi = bo.phi;
        bo.phi += bo.omega * dt;
        if !bo.wrap {
            if bo.phi <= bo.lo {
                bo.phi = bo.lo;
                bo.omega = bo.omega.abs();
            } else if bo.phi >= bo.hi {
                bo.phi = bo.hi;
                bo.omega = -bo.omega.abs();
            }
        }

        let (s, c) = bo.phi.sin_cos();
        let (x, z) = (bo.cx + c * bo.rx, bo.cz + s * bo.rz);
        // The ork fortress's bay is closed water: a hull about to drift in turns back the
        // way it came instead (no boat ever crosses Gnashfang Hold's moat).
        if crate::ork_fortress::boat_keepout(x, z) {
            bo.phi = prev_phi;
            bo.omega = -bo.omega;
            continue;
        }
        // Tangential heading: point the bow (+X) along travel.
        let (vx, vz) = (-bo.rx * s * bo.omega, bo.rz * c * bo.omega);
        let yaw = (-vz).atan2(vx);

        let bob = bo.bob_amp * (tw * bo.bob_freq + bo.phase).sin();
        let roll = 0.05 * (tw * 0.8 + bo.phase).sin();
        let pitch = 0.04 * (tw * 0.55 + bo.phase * 1.3).sin();

        tf.translation = Vec3::new(x, bo.sea_y + bob, z);
        tf.rotation =
            Quat::from_rotation_y(yaw) * Quat::from_rotation_x(roll) * Quat::from_rotation_z(pitch);
    }
}

// ── Mesh assembly ───────────────────────────────────────────────────────────────────

/// One hull station: left rail, right rail, keel point at parameter `t` (0=stern, 1=bow).
fn station(t: f32) -> (Vec3, Vec3, Vec3) {
    let x = STERN_X + (BOW_X - STERN_X) * t;
    let beam = BEAM * (1.0 - t * t * t).max(0.0).sqrt(); // sharp bow, full transom
    let depth = DEPTH * (1.0 - t * t).max(0.05).sqrt(); // keel rises toward the bow
    (Vec3::new(x, RAIL, beam), Vec3::new(x, RAIL, -beam), Vec3::new(x, -depth, 0.0))
}

fn push_tri(v: &mut Vec<[f32; 3]>, a: Vec3, b: Vec3, c: Vec3) {
    v.push([a.x, a.y, a.z]);
    v.push([b.x, b.y, b.z]);
    v.push([c.x, c.y, c.z]);
}

fn build_boat(variant: usize) -> Mesh {
    let p = &PALETTES[variant];

    // Hull skin (V-loft sides + transom) — one wood tone.
    let mut hull: Vec<[f32; 3]> = Vec::new();
    for i in 0..STATIONS {
        let t0 = i as f32 / STATIONS as f32;
        let t1 = (i + 1) as f32 / STATIONS as f32;
        let (l0, r0, k0) = station(t0);
        let (l1, r1, k1) = station(t1);
        // Left flank (rail → keel), right flank (keel → rail).
        push_tri(&mut hull, l0, k0, k1);
        push_tri(&mut hull, l0, k1, l1);
        push_tri(&mut hull, k0, r0, r1);
        push_tri(&mut hull, k0, r1, k1);
    }
    let (l0, r0, k0) = station(0.0);
    push_tri(&mut hull, l0, r0, k0); // transom cap

    // Deck cap (flat top at the rail) — slightly lighter plank tone.
    let mut deck: Vec<[f32; 3]> = Vec::new();
    for i in 0..STATIONS {
        let t0 = i as f32 / STATIONS as f32;
        let t1 = (i + 1) as f32 / STATIONS as f32;
        let (l0, r0, _) = station(t0);
        let (l1, r1, _) = station(t1);
        push_tri(&mut deck, l0, r0, r1);
        push_tri(&mut deck, l0, r1, l1);
    }

    // Curved sail (billowed toward +Z), set on a mast just ahead of midships.
    let mut sail: Vec<[f32; 3]> = Vec::new();
    let (sx0, sx1) = (0.28, 1.15);
    let (sy0, sy1) = (RAIL + 0.18, RAIL + 1.3);
    let billow = 0.18;
    let segs = 4;
    for i in 0..segs {
        let u0 = i as f32 / segs as f32;
        let u1 = (i + 1) as f32 / segs as f32;
        let ax0 = sx0 + (sx1 - sx0) * u0;
        let ax1 = sx0 + (sx1 - sx0) * u1;
        let z0 = billow * (std::f32::consts::PI * u0).sin();
        let z1 = billow * (std::f32::consts::PI * u1).sin();
        let a = Vec3::new(ax0, sy0, z0);
        let b = Vec3::new(ax1, sy0, z1);
        let c = Vec3::new(ax1, sy1, z1);
        let d = Vec3::new(ax0, sy1, z0);
        push_tri(&mut sail, a, b, c);
        push_tri(&mut sail, a, c, d);
    }

    let cabin = Cuboid::new(0.85, 0.5, 0.95)
        .mesh()
        .build()
        .translated_by(Vec3::new(-0.55, RAIL + 0.25, 0.0));
    let mast = Cylinder::new(0.045, 1.5)
        .mesh()
        .resolution(8)
        .build()
        .translated_by(Vec3::new(0.2, RAIL + 0.75, 0.0));

    flat_shaded(merged(vec![
        tinted(raw_tris(hull), lin(p.hull)),
        tinted(raw_tris(deck), lin(p.deck)),
        tinted(cabin, lin(p.cabin)),
        tinted(mast, lin(p.mast)),
        tinted(raw_tris(sail), lin(p.sail)),
    ]))
}

/// Wrap a flat triangle-soup into an indexed mesh carrying the SAME attribute set the Bevy
/// primitives use (POSITION/NORMAL/UV) so `Mesh::merge` accepts both. Normals are
/// placeholders — `flat_shaded` recomputes them after the merge.
fn raw_tris(positions: Vec<[f32; 3]>) -> Mesh {
    let n = positions.len();
    let indices: Vec<u32> = (0..n as u32).collect();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    m.insert_indices(Indices::U32(indices));
    m
}

/// Tag every vertex with one flat linear colour (REQUIRED before merge).
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("boat parts share attributes");
    }
    base
}

fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}
