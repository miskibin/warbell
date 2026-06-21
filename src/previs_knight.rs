//! **Isolated 1:1 transcription of the three.js previs knight** (`tools/index.html`). Built to match
//! it point-for-point: the same `plate` (tapered chamfered box) and `rev` (lathe) primitives, the
//! exact part dims / positions / colours, the same scene-graph (root→torso→arms/neck/legs…), rendered
//! flat-solid on the hero material. Spawned standalone via `FOREST_VIEW=knight2` to verify the look
//! before integrating into the rigged game hero.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

use crate::creature::{surf_code, CreatureMaterial, Surf};
use crate::palette::lin;

// ── previs palette (tools/index.html `C`) ──
const STEEL: u32 = 0x808d9d;
const STEEL_LT: u32 = 0x95a2b2;
const STEEL_DK: u32 = 0x4c5562;
const STEEL_DIM: u32 = 0x6e7886;
const LEATHER: u32 = 0x5d4836;
const LEATHER_DK: u32 = 0x3b2e21;
const TABARD: u32 = 0x9a6438;
const TABARD_DK: u32 = 0x6f4626;
const GLOVE: u32 = 0x282320;
const GOLD: u32 = 0xb8902f;
const DARK: u32 = 0x191c22;
const BLADE: u32 = 0xc3c7cd;
const GRIP: u32 = 0x6a4a2e;
const SHIELD_FACE: u32 = 0x2b2723;

fn surf_for(c: u32) -> Surf {
    match c {
        LEATHER | LEATHER_DK | TABARD | TABARD_DK | GLOVE | GRIP | DARK | SHIELD_FACE => Surf::Cloth,
        _ => Surf::Metal,
    }
}

// ── mesh plumbing ──
fn mesh_from(pos: Vec<[f32; 3]>, idx: Vec<u32>) -> Mesh {
    let n = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}
fn tinted(mut m: Mesh, c: u32) -> Mesh {
    let n = m.count_vertices();
    let mut col = lin(c);
    col[3] = surf_code(surf_for(c));
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![col; n]);
    m
}
/// flat-shade one part (duplicate FIRST, then flat normals — the pass panics on an indexed mesh).
fn flat(mut m: Mesh) -> Mesh {
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

// chamfer-box topology (24 verts: 6 face quads + 12 edge bevels + 8 corner tris), shared by tplate.
const CB_EDGES: [[u32; 4]; 12] = [
    [1, 2, 10, 9], [3, 0, 13, 14], [6, 5, 8, 11], [7, 4, 12, 15],
    [3, 2, 18, 17], [0, 1, 22, 21], [7, 6, 19, 16], [4, 5, 23, 20],
    [11, 10, 18, 19], [8, 9, 22, 23], [15, 14, 17, 16], [12, 13, 21, 20],
];
const CB_CORNERS: [[u32; 3]; 8] =
    [[2, 10, 18], [1, 9, 22], [3, 14, 17], [0, 13, 21], [6, 11, 19], [5, 8, 23], [7, 15, 16], [4, 12, 20]];

/// `plate(w,h,d,topW,topD,r)` — a chamfered box tapered toward the top, base at y=0 (three.js
/// `RoundedBoxGeometry` + per-vertex taper). `e` = chamfer inset.
fn plate(w: f32, h: f32, d: f32, top_w: f32, top_d: f32, e: f32) -> Mesh {
    let (a, b, c) = (w * 0.5, h * 0.5, d * 0.5);
    let e = e.min(a * 0.49).min(b * 0.49).min(c * 0.49).max(0.001);
    let (ai, bi, ci) = (a - e, b - e, c - e);
    let mut pos: Vec<[f32; 3]> = vec![
        [a, -bi, -ci], [a, bi, -ci], [a, bi, ci], [a, -bi, ci],
        [-a, -bi, -ci], [-a, bi, -ci], [-a, bi, ci], [-a, -bi, ci],
        [-ai, b, -ci], [ai, b, -ci], [ai, b, ci], [-ai, b, ci],
        [-ai, -b, -ci], [ai, -b, -ci], [ai, -b, ci], [-ai, -b, ci],
        [-ai, -bi, c], [ai, -bi, c], [ai, bi, c], [-ai, bi, c],
        [-ai, -bi, -c], [ai, -bi, -c], [ai, bi, -c], [-ai, bi, -c],
    ];
    for v in pos.iter_mut() {
        let f = (v[1] + b) / h; // 0 bottom .. 1 top
        v[0] *= 1.0 + (top_w - 1.0) * f;
        v[2] *= 1.0 + (top_d - 1.0) * f;
        v[1] += b; // base to y=0
    }
    let center = Vec3::new(0.0, b, 0.0);
    let mut raw: Vec<[u32; 3]> = Vec::new();
    for f in 0..6u32 {
        let o = f * 4;
        raw.push([o, o + 1, o + 2]);
        raw.push([o, o + 2, o + 3]);
    }
    for q in CB_EDGES {
        raw.push([q[0], q[1], q[2]]);
        raw.push([q[0], q[2], q[3]]);
    }
    raw.extend(CB_CORNERS);
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx = Vec::new();
    for t in raw {
        let (va, vb, vc) = (g(t[0]), g(t[1]), g(t[2]));
        let nrm = (vb - va).cross(vc - va);
        let out = (va + vb + vc) / 3.0 - center;
        if nrm.dot(out) >= 0.0 {
            idx.extend(t);
        } else {
            idx.extend([t[0], t[2], t[1]]);
        }
    }
    mesh_from(pos, idx)
}
fn rb(w: f32, h: f32, d: f32, e: f32) -> Mesh {
    plate(w, h, d, 1.0, 1.0, e)
}
/// `rev(pts,seg)` — a lathe: revolve the 2D profile `[radius, height]` around Y. Winding auto-fixed
/// radially-outward.
fn lathe(profile: &[[f32; 2]], segs: u32) -> Mesh {
    let n = profile.len() as u32;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    for s in 0..=segs {
        let th = (s as f32 / segs as f32) * TAU;
        let (st, ct) = (th.sin(), th.cos());
        for p in profile {
            pos.push([p[0] * ct, p[1], p[0] * st]);
        }
    }
    let mut raw: Vec<[u32; 3]> = Vec::new();
    for s in 0..segs {
        for i in 0..n - 1 {
            let (a, b, c, d) = (s * n + i, (s + 1) * n + i, (s + 1) * n + i + 1, s * n + i + 1);
            raw.push([a, b, d]);
            raw.push([b, c, d]);
        }
    }
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx = Vec::new();
    for t in raw {
        let (va, vb, vc) = (g(t[0]), g(t[1]), g(t[2]));
        let nrm = (vb - va).cross(vc - va);
        let ctr = (va + vb + vc) / 3.0;
        if nrm.dot(Vec3::new(ctr.x, 0.0, ctr.z)) >= 0.0 {
            idx.extend(t);
        } else {
            idx.extend([t[0], t[2], t[1]]);
        }
    }
    mesh_from(pos, idx)
}
const ICO: u32 = 2;
fn ball(r: f32) -> Mesh {
    Sphere::new(r).mesh().ico(ICO).unwrap()
}

// ── spawn helpers (mirror the three.js scene graph) ──
struct Ctx<'w, 's, 'a> {
    cmd: &'a mut Commands<'w, 's>,
    meshes: &'a mut Assets<Mesh>,
    mat: Handle<CreatureMaterial>,
}
impl Ctx<'_, '_, '_> {
    /// a `THREE.Group` child node (transform only).
    fn node(&mut self, parent: Entity, t: Transform) -> Entity {
        let e = self.cmd.spawn((t, Visibility::Visible)).id();
        self.cmd.entity(parent).add_child(e);
        e
    }
    /// a mesh part placed in `parent` at (off, rot), tinted `c`, flat-shaded.
    fn put(&mut self, parent: Entity, m: Mesh, off: Vec3, rot: Quat, c: u32) {
        let h = self.meshes.add(flat(tinted(m, c)));
        let e = self
            .cmd
            .spawn((Mesh3d(h), MeshMaterial3d(self.mat.clone()), Transform { translation: off, rotation: rot, scale: Vec3::ONE }))
            .id();
        self.cmd.entity(parent).add_child(e);
    }
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
fn tr(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

// rig constants (tools/index.html)
const HIP: f32 = 2.55;
const KNEE: f32 = 1.40;
const SHO: f32 = 3.84;
const ELB: f32 = 2.86;
const NECK: f32 = 4.10;
const LEG_X: f32 = 0.46;
const PX: f32 = 0.92;

/// Spawn the previs knight (rest pose) under `parent`.
pub fn spawn(cmd: &mut Commands, parent: Entity, meshes: &mut Assets<Mesh>, mat: Handle<CreatureMaterial>) {
    let mut x = Ctx { cmd, meshes, mat };
    let root = x.node(parent, Transform::IDENTITY);
    let torso = x.node(root, Transform::IDENTITY);

    // TORSO
    x.put(torso, plate(1.0, 0.4, 1.04, 1.12, 1.1, 0.08), Vec3::new(0.0, 2.32, 0.0), Quat::IDENTITY, STEEL); // fauld
    // tabard panels (rest: no sway)
    let tab_h = 0.92;
    let tab_f = x.node(torso, tr(0.0, 2.46, 0.42));
    x.put(tab_f, plate(0.54, tab_h, 0.06, 1.35, 1.0, 0.03), Vec3::new(0.0, -tab_h, 0.0), Quat::IDENTITY, TABARD);
    let tab_b = x.node(torso, tr(0.0, 2.46, -0.42));
    x.put(tab_b, plate(0.54, tab_h, 0.06, 1.35, 1.0, 0.03), Vec3::new(0.0, -tab_h, 0.0), Quat::IDENTITY, TABARD_DK);
    x.put(torso, rb(1.08, 0.2, 1.02, 0.05), Vec3::new(0.0, 2.42, 0.0), Quat::IDENTITY, LEATHER_DK); // belt
    x.put(torso, lathe(&[[0.0, 0.13], [0.16, 0.1], [0.16, -0.02], [0.0, -0.05]], 10), Vec3::new(0.0, 2.5, 0.55), xyz(PI / 2.0, 0.0, 0.0), GOLD); // buckle
    x.put(torso, plate(1.12, 1.55, 1.06, 1.18, 1.12, 0.16), Vec3::new(0.0, 2.5, 0.0), Quat::IDENTITY, LEATHER); // gambeson chest
    x.put(torso, plate(0.22, 1.24, 0.12, 0.5, 1.0, 0.05), Vec3::new(0.0, 2.66, 0.5), Quat::IDENTITY, LEATHER); // chest keel
    x.put(torso, lathe(&[[0.0, 0.34], [0.5, 0.3], [0.56, 0.08], [0.5, 0.0], [0.0, 0.0]], 16), Vec3::new(0.0, 3.95, 0.0), Quat::IDENTITY, STEEL_LT); // gorget

    // HEAD
    let neck = x.node(torso, tr(0.0, NECK, 0.0));
    x.put(neck, rb(0.42, 0.26, 0.42, 0.08), Vec3::new(0.0, 0.0, 0.0), Quat::IDENTITY, STEEL_DK);
    x.put(neck, lathe(&[[0.0, 1.28], [0.3, 1.2], [0.5, 0.98], [0.56, 0.62], [0.57, 0.06], [0.5, 0.0], [0.0, 0.0]], 18), Vec3::new(0.0, 0.2, 0.0), Quat::IDENTITY, STEEL); // helm
    x.put(neck, plate(0.12, 1.0, 0.16, 0.6, 1.0, 0.04), Vec3::new(0.0, 0.35, 0.5), Quat::IDENTITY, STEEL_DIM); // brow keel
    x.put(neck, rb(0.3, 0.08, 0.06, 0.02), Vec3::new(0.17, 0.94, 0.49), Quat::IDENTITY, DARK); // eye slit R
    x.put(neck, rb(0.3, 0.08, 0.06, 0.02), Vec3::new(-0.17, 0.94, 0.49), Quat::IDENTITY, DARK); // eye slit L
    for hx in [-0.18, -0.06, 0.06, 0.18] {
        x.put(neck, rb(0.05, 0.05, 0.05, 0.02), Vec3::new(hx, 0.52, 0.52), Quat::IDENTITY, DARK); // breath holes
    }

    // ARMS  (s: L=+1, R=-1)
    for (label, s) in [("L", 1.0f32), ("R", -1.0f32)] {
        let sh = x.node(torso, Transform { translation: Vec3::new(s * PX, SHO, 0.0), rotation: xyz(0.0, 0.0, s * 0.14), scale: Vec3::ONE });
        x.put(sh, lathe(&[[0.0, 0.32], [0.22, 0.29], [0.4, 0.18], [0.5, 0.03], [0.5, -0.14], [0.4, -0.22], [0.2, -0.24], [0.0, -0.24]], 16), Vec3::new(0.0, 3.66 - SHO, 0.02), xyz(0.05, 0.0, -s * 0.12), STEEL_LT); // pauldron
        x.put(sh, plate(0.46, 0.78, 0.5, 0.92, 0.94, 0.08), Vec3::new(0.0, 2.95 - SHO, 0.0), Quat::IDENTITY, STEEL); // rerebrace
        let el = x.node(sh, tr(0.0, ELB - SHO, 0.0));
        x.put(el, lathe(&[[0.0, 0.26], [0.2, 0.22], [0.28, 0.08], [0.28, 0.0], [0.0, 0.0]], 14), Vec3::new(0.0, 2.86 - ELB, 0.04), Quat::IDENTITY, STEEL_LT); // couter
        x.put(el, plate(0.44, 0.84, 0.48, 0.78, 0.82, 0.08), Vec3::new(0.0, 2.0 - ELB, 0.0), Quat::IDENTITY, STEEL); // vambrace
        x.put(el, plate(0.46, 0.4, 0.52, 0.8, 0.86, 0.06), Vec3::new(0.0, 1.56 - ELB, 0.0), Quat::IDENTITY, GLOVE); // gauntlet
        x.put(el, rb(0.42, 0.16, 0.46, 0.05), Vec3::new(0.0, 1.62 - ELB, 0.16), Quat::IDENTITY, STEEL_DK); // knuckle
        if label == "R" {
            let g = x.node(el, Transform { translation: Vec3::new(0.0, 1.5 - ELB, 0.04), rotation: xyz(1.0, 0.0, 0.12), scale: Vec3::ONE });
            spawn_sword(&mut x, g);
        } else {
            let g = x.node(el, Transform { translation: Vec3::new(0.18, 1.5 - ELB, -0.04), rotation: xyz(0.05, -1.5, 0.0), scale: Vec3::ONE });
            spawn_shield(&mut x, g);
        }
    }

    // LEGS
    for s in [1.0f32, -1.0f32] {
        let hip = x.node(root, tr(s * LEG_X, HIP, 0.0));
        x.put(hip, plate(0.52, 1.18, 0.62, 1.18, 1.12, 0.1), Vec3::new(0.0, 1.42 - HIP, 0.0), Quat::IDENTITY, STEEL); // cuisse
        let kn = x.node(hip, tr(0.0, KNEE - HIP, 0.0));
        x.put(kn, lathe(&[[0.0, 0.34], [0.18, 0.3], [0.33, 0.16], [0.36, 0.0], [0.0, 0.0]], 14), Vec3::new(0.0, 1.2 - KNEE, 0.16), Quat::IDENTITY, STEEL_LT); // poleyn
        x.put(kn, plate(0.5, 1.12, 0.58, 0.78, 0.84, 0.09), Vec3::new(0.0, 0.26 - KNEE, 0.0), Quat::IDENTITY, STEEL); // greave
        x.put(kn, rb(0.5, 0.26, 0.66, 0.06), Vec3::new(0.0, -KNEE, -0.02), Quat::IDENTITY, STEEL_DK); // sabaton
        x.put(kn, plate(0.46, 0.22, 0.5, 0.6, 0.7, 0.05), Vec3::new(0.0, 0.04 - KNEE, 0.42), Quat::IDENTITY, STEEL); // toe
    }
}

fn spawn_sword(x: &mut Ctx, g: Entity) {
    x.put(g, rb(0.08, 0.5, 0.1, 0.03), Vec3::new(0.0, 0.0, 0.0), Quat::IDENTITY, GRIP);
    x.put(g, rb(0.5, 0.12, 0.16, 0.04), Vec3::new(0.0, 0.3, 0.0), Quat::IDENTITY, GOLD); // crossguard
    x.put(g, plate(0.18, 2.4, 0.06, 0.18, 0.5, 0.02), Vec3::new(0.0, 0.36, 0.0), Quat::IDENTITY, BLADE);
    x.put(g, lathe(&[[0.0, 0.11], [0.11, 0.07], [0.11, -0.05], [0.0, -0.09]], 8), Vec3::new(0.0, -0.29, 0.0), Quat::IDENTITY, GOLD); // pommel
}

fn spawn_shield(x: &mut Ctx, g: Entity) {
    // heater outline (tools shield Shape), sampled into a polygon and extruded.
    let mut base = vec![Vec2::new(-0.5, 0.72), Vec2::new(0.5, 0.72), Vec2::new(0.53, 0.05)];
    quad(Vec2::new(0.53, 0.05), Vec2::new(0.46, -0.42), Vec2::new(0.0, -0.84), 5, &mut base);
    quad(Vec2::new(0.0, -0.84), Vec2::new(-0.46, -0.42), Vec2::new(-0.53, 0.05), 5, &mut base);
    let face = extrude(&base, 0.1);
    x.put(g, face.clone(), Vec3::new(0.0, 0.0, 0.03), Quat::IDENTITY, SHIELD_FACE);
    // gold rim slightly larger, behind
    let rim: Vec<Vec2> = base.iter().map(|p| *p * 1.08).collect();
    x.put(g, extrude(&rim, 0.07), Vec3::new(0.0, 0.0, -0.03), Quat::IDENTITY, GOLD);
    // emblem: cross + boss + studs
    x.put(g, rb(0.1, 0.5, 0.04, 0.02), Vec3::new(0.0, 0.05, 0.14), Quat::IDENTITY, GOLD);
    x.put(g, rb(0.34, 0.1, 0.04, 0.02), Vec3::new(0.0, 0.16, 0.14), Quat::IDENTITY, GOLD);
    x.put(g, lathe(&[[0.0, 0.08], [0.09, 0.05], [0.09, -0.03], [0.0, -0.05]], 8), Vec3::new(0.0, 0.16, 0.14), xyz(PI / 2.0, 0.0, 0.0), GOLD);
    for p in [[0.0, 0.64], [-0.43, 0.18], [0.43, 0.18], [-0.28, -0.5], [0.28, -0.5]] {
        x.put(g, lathe(&[[0.0, 0.035], [0.05, 0.02], [0.0, -0.015]], 6), Vec3::new(p[0], p[1], 0.1), xyz(PI / 2.0, 0.0, 0.0), GOLD);
    }
}

fn quad(p0: Vec2, c: Vec2, p1: Vec2, steps: u32, out: &mut Vec<Vec2>) {
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        out.push(p0 * (u * u) + c * (2.0 * u * t) + p1 * (t * t));
    }
}
/// extruded polygon (front +Z + back -Z + walls), outline in XY (CCW from +Z).
fn extrude(pts: &[Vec2], depth: f32) -> Mesh {
    let n = pts.len();
    let ctr = pts.iter().copied().sum::<Vec2>() / n as f32;
    let hz = depth * 0.5;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let fc = pos.len() as u32;
    pos.push([ctr.x, ctr.y, hz]);
    for p in pts {
        pos.push([p.x, p.y, hz]);
    }
    for i in 0..n {
        idx.extend([fc, fc + 1 + i as u32, fc + 1 + ((i + 1) % n) as u32]);
    }
    let bc = pos.len() as u32;
    pos.push([ctr.x, ctr.y, -hz]);
    for p in pts {
        pos.push([p.x, p.y, -hz]);
    }
    for i in 0..n {
        idx.extend([bc, bc + 1 + ((i + 1) % n) as u32, bc + 1 + i as u32]);
    }
    for i in 0..n {
        let (p0, p1) = (pts[i], pts[(i + 1) % n]);
        let b = pos.len() as u32;
        pos.push([p0.x, p0.y, hz]);
        pos.push([p1.x, p1.y, hz]);
        pos.push([p1.x, p1.y, -hz]);
        pos.push([p0.x, p0.y, -hz]);
        idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    mesh_from(pos, idx)
}
