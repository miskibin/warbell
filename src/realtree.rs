//! **Realistic textured tree (POC)** — a proof-of-concept tree that deliberately breaks
//! the project's low-poly / single-white-material convention to explore a *photographic*
//! look built from web textures (CC0 from ambientCG: `Bark012` + `LeafSet024`).
//!
//! Unlike the batched primitive props in `trees.rs` (one shared white material, colour
//! baked into vertices), this tree owns **two textured materials**:
//!   - a smooth, normal-mapped **bark** trunk/branch skeleton (one merged mesh of tapered
//!     conical-frustum segments grown recursively), and
//!   - a **canopy** of a few hundred alpha-masked **leaf cards** — quads each UV-mapped to
//!     one cell of the 3×3 leaf atlas, scattered over the crown and clustered at branch
//!     tips. The classic real-time "leaf instancing" approach: many cutout quads read as a
//!     full, soft canopy far more convincingly than blob spheres.
//!
//! It is gated behind `FOREST_TREE` so it only appears when staging a POC screenshot and
//! never touches normal gameplay. `FOREST_TREE=1` drops it at a default open spot;
//! `FOREST_TREE="x,z"` places it at a world XZ. Frame it with `FOREST_CAM` + `FOREST_SHOT`.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

pub struct RealTreePlugin;

impl Plugin for RealTreePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("FOREST_TREE").is_ok() {
            app.add_systems(Startup, spawn_real_tree);
        }
    }
}

// ── deterministic tiny RNG (xorshift) so the tree is reproducible per screenshot ────────
struct Rng(u32);
impl Rng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// uniform 0.0..1.0
    fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    /// uniform lo..hi
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }
}

// ── branch skeleton → one merged, UV-scaled, tangent-bearing bark mesh ──────────────────

/// Append one tapered segment (a conical frustum, base at `from`, growing along `dir`) to
/// `parts`, with bark UVs tiled ~`u_tiles` around and proportional to length along it.
fn segment(parts: &mut Vec<Mesh>, from: Vec3, dir: Vec3, len: f32, r0: f32, r1: f32) {
    let frustum = ConicalFrustum {
        radius_top: r1,
        radius_bottom: r0,
        height: len,
    };
    let mut m = frustum.mesh().resolution(10).build();
    // bark tiling: ~2 wraps around, repeat along height by length so bark doesn't smear
    scale_uv(&mut m, 2.0, len * 1.1);
    // primitive frustum is centred on origin along +Y; lift so its base sits at y=0,
    // rotate +Y → dir, then move the base to `from`.
    let m = m
        .translated_by(Vec3::Y * len * 0.5)
        .rotated_by(Quat::from_rotation_arc(Vec3::Y, dir.normalize()))
        .translated_by(from);
    parts.push(m);
}

/// Scale the UV_0 attribute in place (tile the bark texture).
fn scale_uv(m: &mut Mesh, su: f32, sv: f32) {
    use bevy::mesh::VertexAttributeValues as V;
    if let Some(V::Float32x2(uvs)) = m.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for uv in uvs.iter_mut() {
            uv[0] *= su;
            uv[1] *= sv;
        }
    }
}

/// Grow the branch skeleton recursively. Each call adds its own segment, records its tip
/// (for leaf clustering), and forks 2–3 children that bend away with some upward bias.
#[allow(clippy::too_many_arguments)]
fn grow(
    parts: &mut Vec<Mesh>,
    tips: &mut Vec<Vec3>,
    rng: &mut Rng,
    from: Vec3,
    dir: Vec3,
    len: f32,
    r0: f32,
    depth: u32,
) {
    let r1 = r0 * 0.72; // taper toward the tip
    segment(parts, from, dir, len, r0, r1);
    let tip = from + dir * len;

    if depth == 0 || r1 < 0.03 {
        tips.push(tip); // a thin terminal twig → cluster leaves here
        return;
    }
    // deeper branches also drop a few leaves along their length, not just at the very ends
    if depth <= 2 {
        tips.push(tip);
    }

    let children = if depth >= 4 { 2 } else { rng.next_u32() % 2 + 2 };
    for _ in 0..children {
        // bend away from the parent: random yaw around the parent dir + a spread tilt,
        // with a steady upward bias so the crown lifts rather than droops.
        let yaw = rng.range(0.0, std::f32::consts::TAU);
        let tilt = rng.range(0.45, 0.95);
        let basis = Quat::from_rotation_arc(Vec3::Y, dir.normalize());
        let local = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(tilt);
        let mut child = (basis * local) * Vec3::Y;
        child = (child + Vec3::Y * 0.55).normalize(); // upward bias
        let child_len = len * rng.range(0.68, 0.82);
        grow(parts, tips, rng, tip, child, child_len, r1, depth - 1);
    }
}

// ── leaf cards ──────────────────────────────────────────────────────────────────────

/// One leaf quad (two tris, double-sided handled by the material) at `center`, sized `size`,
/// oriented by `rot`, UV-mapped to atlas cell (`cx`,`cy`) of a 3×3 grid. Appends into the
/// shared position/normal/uv/index buffers.
#[allow(clippy::too_many_arguments)]
fn leaf_quad(
    pos: &mut Vec<[f32; 3]>,
    nor: &mut Vec<[f32; 3]>,
    uv: &mut Vec<[f32; 2]>,
    idx: &mut Vec<u32>,
    center: Vec3,
    size: f32,
    rot: Quat,
    cx: u32,
    cy: u32,
) {
    let base = pos.len() as u32;
    let hw = size * 0.5;
    let h = size; // leaves slightly taller than wide
    // local quad in XY plane, pivot at the stem (bottom-centre) so leaves hang off twigs
    let corners = [
        Vec3::new(-hw, 0.0, 0.0),
        Vec3::new(hw, 0.0, 0.0),
        Vec3::new(hw, h, 0.0),
        Vec3::new(-hw, h, 0.0),
    ];
    let n = rot * Vec3::Z;
    let c = 1.0 / 3.0;
    let (u0, v0) = (cx as f32 * c, cy as f32 * c);
    // atlas leaf points DOWN in the texture (stem at bottom), so v grows downward
    let uvs = [
        [u0, v0 + c],
        [u0 + c, v0 + c],
        [u0 + c, v0],
        [u0, v0],
    ];
    for (k, corner) in corners.iter().enumerate() {
        pos.push((center + rot * *corner).to_array());
        nor.push(n.to_array());
        uv.push(uvs[k]);
    }
    idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Build the canopy mesh: `density` leaves clustered around every recorded branch tip, each
/// a random atlas cell at a random orientation, sized within `leaf` (min,max).
fn build_canopy(tips: &[Vec3], rng: &mut Rng, density: u32, leaf: (f32, f32)) -> Mesh {
    let mut pos = Vec::new();
    let mut nor = Vec::new();
    let mut uv = Vec::new();
    let mut idx = Vec::new();

    for &tip in tips {
        let n = density + rng.next_u32() % (density / 2).max(1);
        for _ in 0..n {
            let off = Vec3::new(
                rng.range(-0.6, 0.6),
                rng.range(-0.35, 0.7),
                rng.range(-0.6, 0.6),
            );
            let rot = Quat::from_euler(
                EulerRot::YXZ,
                rng.range(0.0, std::f32::consts::TAU),
                rng.range(-1.2, 1.2),
                rng.range(-0.6, 0.6),
            );
            let cell = rng.next_u32() % 9;
            leaf_quad(
                &mut pos,
                &mut nor,
                &mut uv,
                &mut idx,
                tip + off,
                rng.range(leaf.0, leaf.1),
                rot,
                cell % 3,
                cell / 3,
            );
        }
    }

    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    m
}

// ── variants ──────────────────────────────────────────────────────────────────────

/// A tree "species": shape knobs + a canopy tint, all normalised to one final height so
/// every variant matches the scatter trees regardless of its natural proportions.
struct Variant {
    name: &'static str,
    seed: u32,
    trunk_h: f32,    // base trunk before the crown forks
    trunk_r: f32,    // base trunk radius
    branch_len: f32, // first crown branch length
    depth: u32,      // recursion depth → crown bushiness
    lean: Vec3,      // initial crown direction (un-normalised)
    leaf_tint: Color,
    leaf_size: (f32, f32),
    density: u32, // leaves per branch tip
}

/// Five species spanning broad/slender shapes and a seasonal colour spread.
fn variants() -> [Variant; 5] {
    [
    Variant {
        name: "oak (broad green)",
        seed: 0x51ED_7A2C,
        trunk_h: 1.6,
        trunk_r: 0.34,
        branch_len: 1.35,
        depth: 6,
        lean: Vec3::new(0.04, 1.0, 0.02),
        leaf_tint: Color::srgb(0.80, 0.92, 0.70),
        leaf_size: (0.5, 0.82),
        density: 18,
    },
    Variant {
        name: "birch (slender pale-green)",
        seed: 0x1A2B_3C4D,
        trunk_h: 2.3,
        trunk_r: 0.24,
        branch_len: 1.05,
        depth: 5,
        lean: Vec3::new(-0.05, 1.0, 0.03),
        leaf_tint: Color::srgb(0.92, 0.98, 0.72),
        leaf_size: (0.42, 0.66),
        density: 14,
    },
    Variant {
        name: "maple (autumn gold)",
        seed: 0x77AA_BB11,
        trunk_h: 1.5,
        trunk_r: 0.36,
        branch_len: 1.4,
        depth: 6,
        lean: Vec3::new(0.06, 1.0, -0.04),
        leaf_tint: Color::srgb(1.10, 0.74, 0.34),
        leaf_size: (0.55, 0.9),
        density: 20,
    },
    Variant {
        name: "spruce-ish (deep cool green)",
        seed: 0x0BAD_F00D,
        trunk_h: 1.9,
        trunk_r: 0.3,
        branch_len: 1.15,
        depth: 6,
        lean: Vec3::new(0.0, 1.0, 0.0),
        leaf_tint: Color::srgb(0.58, 0.80, 0.58),
        leaf_size: (0.46, 0.72),
        density: 16,
    },
    Variant {
        name: "sapling (small fresh green)",
        seed: 0xC0FF_EE42,
        trunk_h: 1.3,
        trunk_r: 0.22,
        branch_len: 1.0,
        depth: 5,
        lean: Vec3::new(-0.08, 1.0, -0.05),
        leaf_tint: Color::srgb(0.74, 0.96, 0.56),
        leaf_size: (0.4, 0.62),
        density: 14,
    },
    ]
}

// ── spawn ───────────────────────────────────────────────────────────────────────────

/// Highest y across a mesh's vertices (its AABB top), for height normalisation.
fn mesh_top(m: &Mesh) -> f32 {
    use bevy::mesh::VertexAttributeValues as V;
    if let Some(V::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) {
        pos.iter().map(|p| p[1]).fold(0.0, f32::max)
    } else {
        1.0
    }
}

fn spawn_real_tree(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
) {
    // placement: FOREST_TREE="x,z" centres the variant row; "1"/anything else → default spot
    let (cx, cz) = std::env::var("FOREST_TREE")
        .ok()
        .and_then(|s| {
            let p: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
            (p.len() == 2).then(|| (p[0], p[1]))
        })
        .unwrap_or((0.0, 16.0));

    // Match the scatter trees: biome_forest builds 1.5u trees scaled ~0.98–1.93 → ~2u tall.
    const TARGET_H: f32 = 2.1;
    const SPACING: f32 = 2.6;

    // one shared bark material; the canopy gets a per-variant tinted material
    let bark_mat = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(assets.load("textures/tree/bark_color.png")),
        normal_map_texture: Some(assets.load("textures/tree/bark_normal.png")),
        perceptual_roughness: 0.9,
        ..default()
    });
    let leaf_tex = assets.load("textures/tree/leaf.png");

    let vs = variants();
    let n = vs.len();
    for (i, v) in vs.iter().enumerate() {
        let mut rng = Rng(v.seed);
        let mut parts = Vec::new();
        let mut tips = Vec::new();

        segment(&mut parts, Vec3::ZERO, Vec3::Y, v.trunk_h, v.trunk_r, v.trunk_r * 0.82);
        grow(
            &mut parts,
            &mut tips,
            &mut rng,
            Vec3::Y * v.trunk_h,
            v.lean.normalize(),
            v.branch_len,
            v.trunk_r * 0.82,
            v.depth,
        );

        let mut bark = parts
            .into_iter()
            .reduce(|mut a, b| {
                a.merge(&b).expect("bark parts share attributes");
                a
            })
            .expect("at least the trunk");
        if let Err(e) = bark.generate_tangents() {
            warn!("realtree[{}]: tangent generation failed ({e:?})", v.name);
        }

        let canopy = build_canopy(&tips, &mut rng, v.density, v.leaf_size);

        // normalise: scale the whole tree so its tallest leaf sits at TARGET_H
        let natural_h = mesh_top(&bark).max(mesh_top(&canopy)).max(0.01);
        let scale = TARGET_H / natural_h;

        let leaf_mat = mats.add(StandardMaterial {
            base_color: v.leaf_tint,
            base_color_texture: Some(leaf_tex.clone()),
            perceptual_roughness: 0.75,
            alpha_mode: AlphaMode::Mask(0.5),
            cull_mode: None,
            double_sided: true,
            ..default()
        });

        // lay the variants out in a row, centred on (cx,cz)
        let x = cx + (i as f32 - (n as f32 - 1.0) * 0.5) * SPACING;
        commands
            .spawn((
                Name::new(format!("RealTree POC: {}", v.name)),
                Transform::from_translation(Vec3::new(x, 0.0, cz)).with_scale(Vec3::splat(scale)),
                Visibility::Visible,
            ))
            .with_children(|p| {
                p.spawn((Mesh3d(meshes.add(bark)), MeshMaterial3d(bark_mat.clone())));
                p.spawn((
                    Mesh3d(meshes.add(canopy)),
                    MeshMaterial3d(leaf_mat),
                    NotShadowCaster, // many cutout tris aren't worth the shadow pass
                ));
            });
    }
}
