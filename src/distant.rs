//! Distant scenery — the horizon depth cue, now driven by the active biome's
//! [`Backdrop`]. A ring of low-poly hill/mountain silhouettes plus an optional dark
//! treeline band, but **only within the land arc** (`land_dir ± land_arc`); the opposite
//! arc is left open for the [`sea`](crate::sea) to fill — "mountains/forest on one side,
//! ocean on the other".
//!
//! Everything is placed FAR out (hills r ~150-260, treeline r ~95-128) so it recedes
//! into the `DistanceFog`, baked into a few merged flat-shaded meshes (a handful of draw
//! calls), coloured via `ATTRIBUTE_COLOR` against a shared white material, and
//! `NotShadowCaster` + static. Colours come pre-muted toward the fog so the haze blends
//! them the rest of the way. Deterministic (constant-seeded Mulberry32).

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::{Backdrop, BiomeEntity};
use crate::palette::lin;

// ── Ring geometry ────────────────────────────────────────────────────────────
const HILL_COUNT: u32 = 64;
const HILL_R_MIN: f32 = 150.0;
const HILL_R_MAX: f32 = 250.0;
/// Sink hill bases below y=0 so cones rise cleanly out of the fog floor.
const HILL_SINK: f32 = 6.0;

const TREE_COUNT: u32 = 720;
const TREE_R_MIN: f32 = 95.0;
const TREE_R_MAX: f32 = 128.0;

pub struct DistantPlugin;

impl Plugin for DistantPlugin {
    fn build(&self, _app: &mut App) {
        // No systems — the biome runner calls `spawn_backdrop` on each switch.
    }
}

/// Build the horizon backdrop for `b`. Hills + (optional) treeline fill only the land
/// arc; the rest of the horizon stays open for the ocean. Tagged [`BiomeEntity`].
pub fn spawn_backdrop(
    b: &Backdrop,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        ..default()
    });

    let hills = meshes.add(build_hill_ring_mesh(b));
    commands.spawn((Mesh3d(hills), MeshMaterial3d(mat.clone()), Transform::default(), NotShadowCaster, BiomeEntity));

    if b.treeline {
        let treeline = meshes.add(build_treeline_mesh(b));
        commands.spawn((Mesh3d(treeline), MeshMaterial3d(mat), Transform::default(), NotShadowCaster, BiomeEntity));
    }
}

// ── Deterministic RNG (Mulberry32, same as scatter) ───────────────────────────
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

/// Shortest angular distance between two angles (radians), in `[0, π]`.
fn ang_dist(a: f32, b: f32) -> f32 {
    let mut d = (a - b).abs() % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d = std::f32::consts::TAU - d;
    }
    d
}

/// True if angle `a` falls inside the land arc `dir ± arc`.
fn in_land(a: f32, b: &Backdrop) -> bool {
    ang_dist(a, b.land_dir) <= b.land_arc
}

// ── Mesh helpers ───────────────────────────────────────────────────────────────
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("distant parts share attributes");
    }
    base
}

fn flat_shaded(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// A cone whose BASE sits on local y=0, then translated to `center`.
fn cone_base_y0(radius: f32, height: f32, sides: u32, center: Vec3) -> Mesh {
    Cone { radius, height }
        .mesh()
        .resolution(sides)
        .build()
        .translated_by(Vec3::new(0.0, height * 0.5, 0.0) + center)
}

// ── Hill / mountain silhouette ring (land arc only) ───────────────────────────
fn build_hill_ring_mesh(b: &Backdrop) -> Mesh {
    let mut r = Rng(0x4157_2026);
    let mut parts: Vec<Mesh> = Vec::new();

    for i in 0..HILL_COUNT {
        let a = (i as f32 / HILL_COUNT as f32) * std::f32::consts::TAU + r.range(-0.05, 0.05);
        // Advance the RNG identically whether or not we keep the hill, so the layout is
        // stable; skip hills outside the land arc.
        let rad = r.range(HILL_R_MIN, HILL_R_MAX);
        let h = r.range(b.hill_h.0, b.hill_h.1);
        let br = h * r.range(0.55, 0.85);
        let sides = 5 + (i % 3);
        if !in_land(a, b) {
            continue;
        }
        let cx = a.cos() * rad;
        let cz = a.sin() * rad;
        let foot = Vec3::new(cx, -HILL_SINK, cz);

        parts.push(tinted(cone_base_y0(br, h, sides, foot), lin(b.hill_body)));
        parts.push(tinted(cone_base_y0(br * 1.35, h * 0.22, sides, foot), lin(b.hill_foot)));

        if h > (b.hill_h.0 + b.hill_h.1) * 0.5 {
            let cap_h = h * 0.34;
            let cap_r = br * (cap_h / h) * 1.05;
            let cap_foot = foot + Vec3::new(0.0, h - cap_h, 0.0);
            parts.push(tinted(cone_base_y0(cap_r, cap_h, sides, cap_foot), lin(b.hill_cap)));
        }
    }

    // Guard against an empty land arc (shouldn't happen, but merge needs ≥1 part).
    if parts.is_empty() {
        parts.push(tinted(cone_base_y0(1.0, 1.0, 5, Vec3::new(0.0, -50.0, 0.0)), lin(b.hill_body)));
    }
    flat_shaded(merged(parts))
}

// ── Dense conifer treeline band (land arc only) ───────────────────────────────
fn build_treeline_mesh(b: &Backdrop) -> Mesh {
    let mut r = Rng(0x7eed_2026);
    let mut parts: Vec<Mesh> = Vec::new();

    for i in 0..TREE_COUNT {
        let a = (i as f32 / TREE_COUNT as f32) * std::f32::consts::TAU + r.range(-0.06, 0.06);
        let rad = r.range(TREE_R_MIN, TREE_R_MAX);
        let trunk_h = r.range(0.8, 1.6);
        let trunk_r = r.range(0.18, 0.3);
        let needle_h = r.range(5.0, 9.0);
        let needle_r = r.range(1.6, 2.8);
        let crown_col = if r.next() < 0.5 { b.treeline_dark } else { b.treeline_mid };
        if !in_land(a, b) {
            continue;
        }
        let cx = a.cos() * rad;
        let cz = a.sin() * rad;
        let base = Vec3::new(cx, 0.0, cz);

        parts.push(tinted(
            Cylinder::new(trunk_r, trunk_h)
                .mesh()
                .resolution(4)
                .build()
                .translated_by(base + Vec3::new(0.0, trunk_h * 0.5, 0.0)),
            lin(b.treeline_dark),
        ));
        parts.push(tinted(
            cone_base_y0(needle_r, needle_h, 5, base + Vec3::new(0.0, trunk_h * 0.7, 0.0)),
            lin(crown_col),
        ));
        parts.push(tinted(
            cone_base_y0(
                needle_r * 0.62,
                needle_h * 0.6,
                5,
                base + Vec3::new(0.0, trunk_h * 0.7 + needle_h * 0.45, 0.0),
            ),
            lin(crown_col),
        ));
    }

    if parts.is_empty() {
        parts.push(tinted(cone_base_y0(1.0, 1.0, 5, Vec3::new(0.0, -50.0, 0.0)), lin(b.treeline_dark)));
    }
    flat_shaded(merged(parts))
}
