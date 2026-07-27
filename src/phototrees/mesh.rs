//! Skeleton → renderable tree meshes with a 3-level LOD chain.
//!
//! Per (species, variant-seed): LOD0 full tubes + leaf cards, LOD1 pruned twigs / fewer
//! bigger cards, LOD2 a one-material impostor (3-sided trunk sampling the atlas bark
//! strip + a handful of huge cards) whose CPU mesh data is kept so `forest.rs` can merge
//! whole chunks of far trees into single meshes.
//!
//! Leaf-card normals use the skeleton's outward "spherical" directions so a canopy
//! lights as one soft volume, not a heap of flat quads (Warbell facet-bake lesson,
//! realistic edition).

use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use super::treegen::tree::{LeafAnchor, Segment, Species, TreeSkeleton};

use super::atlas;


/// Plain CPU mesh accumulator — also the merge unit for far-field chunks.
/// `colors` are per-vertex tints (StandardMaterial multiplies them into the texture) —
/// this is how 16 shared meshes render as a colour-varied forest for free.
#[derive(Default, Clone)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn to_mesh(&self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone());
        if !self.colors.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.clone());
        }
        mesh.insert_indices(Indices::U32(self.indices.clone()));
        mesh
    }

    /// Append `other`, transformed by yaw+scale+translation (the far-merge path).
    pub fn append_transformed(&mut self, other: &MeshData, pos: Vec3, yaw: f32, scale: f32) {
        let rot = Quat::from_rotation_y(yaw);
        let base = self.positions.len() as u32;
        for p in &other.positions {
            let v = rot * (Vec3::from_array(*p) * scale) + pos;
            self.positions.push(v.to_array());
        }
        for n in &other.normals {
            self.normals.push((rot * Vec3::from_array(*n)).to_array());
        }
        self.uvs.extend_from_slice(&other.uvs);
        if other.colors.is_empty() {
            self.colors.extend(std::iter::repeat_n([1.0, 1.0, 1.0, 1.0], other.positions.len()));
        } else {
            self.colors.extend_from_slice(&other.colors);
        }
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
}

/// Per-(species, variant) foliage tint — species character + variant spread.
///
/// **Re-graded for this game, 2026-07-27.** These multiply the photographic sprig texture, whose
/// own pixels carry a lot of brown/tan dead-leaf noise. Upstream's near-neutral tints let that
/// through, and upstream also spent a full quarter of its variants on deliberately autumnal crowns
/// (broadleaf `[1.48, 0.92, 0.40]`, golden birch `[1.52, 1.18, 0.42]`). Dropped into this game's
/// bright saturated summer meadow — faceted grass-green bushes directly beneath — the first
/// screenshot pass read unmistakably as **dying trees in a summer field**.
///
/// So every variant is now green-DOMINANT: G held at or above 1.0 while R and especially B are
/// pulled down, which both greens the leaf pixels and lifts saturation (saturation being the
/// max-min channel spread). At most one variant per species stays warm, and only mildly — seasonal
/// variety is worth keeping, a quarter of the forest looking dead is not.
///
/// NB values above 1.0 are intentional and load-bearing: `StandardMaterial` multiplies vertex
/// colour into the texture, so >1 is the only way to brighten a dark photographic sample. `card()`
/// clamps its own jitter at 2.0, so there is headroom.
fn foliage_tint(sp: Species, var: usize) -> [f32; 3] {
    match sp {
        Species::Pine => [
            [0.80, 1.06, 0.72],
            [0.88, 1.10, 0.64],
            [0.72, 0.98, 0.68],
            [0.92, 1.04, 0.58], // the warm one: sun-bleached, not dead
        ][var % 4],
        Species::Spruce => [
            [0.70, 0.96, 0.86], // cool blue-green conifer character, kept
            [0.78, 1.02, 0.76],
            [0.64, 0.88, 0.80],
            [0.82, 1.04, 0.70],
        ][var % 4],
        Species::Broadleaf => [
            [0.86, 1.14, 0.72],
            [0.94, 1.20, 0.66],
            [0.76, 1.08, 0.70],
            [1.10, 1.06, 0.54], // warm-touched, well short of upstream's orange
        ][var % 4],
        Species::Birch => [
            [0.92, 1.16, 0.66],
            [1.00, 1.22, 0.60],
            [0.84, 1.10, 0.70],
            [1.12, 1.14, 0.56], // pale gold-green, not the old full autumn gold
        ][var % 4],
    }
}

fn wood_tint(sp: Species, var: usize) -> [f32; 3] {
    let v = [1.0, 0.88, 1.08, 0.95][var % 4];
    match sp {
        Species::Birch => [v, v, v], // keep the white bark white-ish
        _ => [v, v * 0.97, v * 0.94],
    }
}

fn ortho_frame(d: Vec3) -> (Vec3, Vec3) {
    let helper = if d.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let u = d.cross(helper).normalize();
    (u, u.cross(d).normalize())
}

/// Sweep tapered tubes along segments. `max_level` prunes twigs for lower LODs.
/// `bark_uv`: None = plain cylindrical UVs (near bark material), Some(rect) = squeeze
/// into the atlas bark strip (LOD2 impostor).
fn build_wood(
    sk: &TreeSkeleton,
    sides: u32,
    max_level: u8,
    bark_uv: Option<(f32, f32, f32, f32)>,
    tint: [f32; 3],
) -> MeshData {
    let mut md = MeshData::default();
    for seg in &sk.segments {
        if seg.level > max_level {
            continue;
        }
        tube(&mut md, seg, sides, bark_uv);
    }
    md.colors = vec![[tint[0], tint[1], tint[2], 1.0]; md.positions.len()];
    md
}

fn tube(md: &mut MeshData, seg: &Segment, sides: u32, bark_uv: Option<(f32, f32, f32, f32)>) {
    let a = Vec3::from_array(seg.a);
    let b = Vec3::from_array(seg.b);
    let d = (b - a).normalize_or_zero();
    if d == Vec3::ZERO {
        return;
    }
    let (u, v) = ortho_frame(d);
    let base = md.positions.len() as u32;
    let len = (b - a).length();
    for (end, centre, r) in [(0.0f32, a, seg.ra), (1.0, b, seg.rb)] {
        for s in 0..=sides {
            let ang = s as f32 / sides as f32 * std::f32::consts::TAU;
            let n = u * ang.cos() + v * ang.sin();
            md.positions.push((centre + n * r).to_array());
            md.normals.push(n.to_array());
            let (uu, vv) = (s as f32 / sides as f32, end * len * 0.35);
            match bark_uv {
                None => md.uvs.push([uu, vv]),
                Some((u0, v0, u1, v1)) => {
                    md.uvs.push([u0 + (u1 - u0) * uu, v0 + (v1 - v0) * (vv % 1.0)])
                }
            }
        }
    }
    let ring = sides + 1;
    for s in 0..sides {
        let a0 = base + s;
        let a1 = base + s + 1;
        let b0 = a0 + ring;
        let b1 = a1 + ring;
        md.indices.extend_from_slice(&[a0, b0, a1, a1, b0, b1]);
    }
}

/// Leaf cards: two crossed quads per anchor (at LOD0) or one (lower LODs), UV = the
/// species' atlas leaf region, normals = the anchor's outward direction.
/// Sprig cards, EZ-Tree style: a base-pivot quad growing OUT of the branch tip along
/// its tangent (the photographic twig texture has its stem base at bottom-centre), width
/// = length, optionally a second perpendicular quad ("Double"/cross — holds up close,
/// unlike camera billboards). Normals are ROUNDED per vertex — bent away from the canopy
/// centre — so hundreds of flat cards light as one soft volume.
/// `quads` = cards per anchor, fanned evenly about the anchor direction. Upstream only ever passed
/// the equivalent of 2 ("crossed") or 1; this port generalises it because **card count per anchor is
/// the only lever that fills a crown here.** The generator places leaf anchors exclusively at branch
/// TIPS, so a smaller `every` stride cannot add interior foliage — it just re-uses the same tip
/// positions. With 2 quads the first screenshot pass read as bare poles / storm-stripped whips at
/// ~15-25% of the low-poly canopy's leaf area, and the forest stopped occluding anything.
fn build_sprigs(
    sk: &TreeSkeleton,
    sp: Species,
    every: usize,
    size_mul: f32,
    quads: usize,
    tint: [f32; 3],
) -> MeshData {
    let mut md = MeshData::default();
    let region = atlas::leaf_uv(sp);
    let canopy = Vec3::from_array(sk.canopy_center);
    let quads = quads.max(1);
    // Fan across a half-turn, not a full one: a card is a flat quad visible from both faces
    // (`cull_mode: None`), so a card at θ and one at θ+π are the same sheet.
    let step = std::f32::consts::PI / quads as f32;
    for (i, l) in sk.leaves.iter().enumerate() {
        if i % every != 0 {
            continue;
        }
        for q in 0..quads {
            card(&mut md, l, size_mul, q as f32 * step, region, tint, canopy);
        }
    }
    md
}

fn card(
    md: &mut MeshData,
    l: &LeafAnchor,
    size_mul: f32,
    roll: f32,
    uv: (f32, f32, f32, f32),
    tint: [f32; 3],
    canopy: Vec3,
) {
    let dir = Vec3::from_array(l.dir).normalize_or_zero();
    let dir = if dir == Vec3::ZERO { Vec3::Y } else { dir };
    let (u, _) = ortho_frame(dir);
    // Roll the card plane around the growth axis (crossed pair + per-sprig variety).
    let rot = Quat::from_axis_angle(dir, roll + l.pos[0] * 1.7 + l.pos[2] * 2.3);
    let u = rot * u;
    let len = l.size * size_mul;
    let half_w = len * 0.5;
    let c = Vec3::from_array(l.pos); // BASE pivot — the sprig grows from the branch tip
    let base = md.positions.len() as u32;
    // Per-card brightness/hue wobble on top of the variant tint — breaks up the crown.
    let j = 0.88 + ((l.pos[0] * 47.1 + l.pos[1] * 9.7 + l.pos[2] * 23.3).sin().abs()) * 0.24;
    let col = [
        (tint[0] * j).min(2.0),
        (tint[1] * (0.9 + j * 0.1)).min(2.0),
        (tint[2] * j * 0.95).min(2.0),
        1.0,
    ];
    let face = u.cross(dir).normalize_or_zero();
    // (side, along, u, v): texture base (v max) sits at the pivot.
    for (su, sa, uu, vv) in [
        (-1.0f32, 0.0f32, uv.0, uv.3),
        (1.0, 0.0, uv.2, uv.3),
        (1.0, 1.0, uv.2, uv.1),
        (-1.0, 1.0, uv.0, uv.1),
    ] {
        let p = c + u * su * half_w + dir * sa * len;
        // Rounded normal: outward from the canopy centre blended with the card facing.
        let n = ((p - canopy).normalize_or_zero() * 0.75 + face * 0.35).normalize_or_zero();
        md.positions.push(p.to_array());
        md.normals.push((if n == Vec3::ZERO { dir } else { n }).to_array());
        md.uvs.push([uu, vv]);
        md.colors.push(col);
    }
    md.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

pub fn species_index(sp: Species) -> usize {
    match sp {
        Species::Pine => 0,
        Species::Spruce => 1,
        Species::Broadleaf => 2,
        Species::Birch => 3,
    }
}

/// Generator-metres → world units. The generator draws heights in real metres (13–25 m); this
/// game's unit is ≈1.5 m and its own tree meshes are authored ~1.5–2.5u tall before `TREE_SCALE`
/// (1.36) and the per-instance 0.72–1.42 spread. `1.8 / 19.0` maps a mid-range 19 m generator tree
/// to ~1.8u authored, landing final in-world heights at ~1.5–3.8u — the same envelope as the
/// low-poly trees it replaces, so nothing downstream needs retuning.
///
/// **Baked into the mesh rather than applied per instance, on purpose.** The generator is NOT
/// scale-invariant: `grow_axis` computes its gnarl amplitude as
/// `gnarl * (1.0 / r_cur.sqrt()).clamp(1.0, 3.0)` from an ABSOLUTE radius, so growing a 5 m tree
/// directly saturates that clamp on every axis and the trunk comes out visibly wigglier than a
/// scaled-down 20 m tree. Generate big, then shrink. (Same reason we don't touch the per-species
/// `h` ranges: `build_lod2`/`build_billboard` also carry absolute-metre floors like `.max(1.8)`.)
const GEN_TO_WORLD: f32 = 1.8 / 19.0;

/// Per-species mesh detail: `sides` = tube cross-section, `max_level` = branch depth kept
/// (2 = twigs, which only Broadleaf has), `every` = keep every Nth leaf anchor, `quads` = cards
/// fanned per kept anchor.
///
/// Spruce is throttled hard and deliberately: its whorl structure emits **177–285 segments**
/// against Pine's 46–62, so at Pine's settings one spruce is ~3.4–5.5k verts — roughly 4× every
/// other species. Since this port renders ONE tier for every tree in the world (no distance LOD
/// swap), a single runaway species would set the whole frame's cost.
///
/// 2026-07-27 density pass, from the first screenshot A/B (crowns read as bare poles): `every`
/// tightened and `quads` raised from a flat 2. Budgeted per species by their very different anchor
/// counts — Broadleaf carries 42–99 anchors so it stays on a stride of 2, while Birch has only
/// 20–28 and needs every one of them plus 4 cards each to read as a crown at all.
fn detail(sp: Species) -> Detail {
    match sp {
        Species::Pine => Detail { sides: 5, max_level: 1, every: 1, quads: 3 },
        Species::Spruce => Detail { sides: 3, max_level: 1, every: 3, quads: 2 },
        Species::Broadleaf => Detail { sides: 5, max_level: 2, every: 2, quads: 3 },
        Species::Birch => Detail { sides: 5, max_level: 1, every: 1, quads: 4 },
    }
}

struct Detail {
    sides: u32,
    max_level: u8,
    every: usize,
    quads: usize,
}

/// Build ONE complete tree mesh for a (species, variant): trunk + branches + leaf cards merged
/// into a single vertex-coloured, UV'd mesh with its base at `y = 0`, at world scale.
///
/// **The single-mesh trick.** Upstream renders near trees as FOUR entities (wood and leaves are
/// separate meshes, each duplicated across two LOD tiers) because its bark is a photographic
/// texture and its leaves are an alpha-masked atlas — two different materials. It only collapses to
/// one mesh in its far LOD2 impostor, by UV-mapping the trunk into the atlas's own bark strip.
/// We do that at full detail: `build_wood(.., Some(atlas::bark_uv(sp)), ..)` sends the tubes to the
/// bark strip, so trunk and canopy share one texture and therefore one material.
///
/// That matters far more here than it did upstream. This game spawns every tree up front as a
/// permanent entity (~15–20k of them), so 4 entities each would mean 60–80k — and upstream measured
/// exactly that shape at **102 ms/frame** before it introduced a streaming radius. One mesh + one
/// material keeps the existing contract: one entity per tree, one shared handle per variant, GPU
/// instancing intact, and `wind::Sway`/`ChopTree`/`blockers` all still addressing a single entity.
///
/// The cost of the trick is that trunks lose photographic bark detail (the strip is procedural
/// `vnoise`) — see the module docs for why that is a smaller loss than it sounds.
pub fn build_tree(sp: Species, var: usize) -> Mesh {
    let sk = super::treegen::tree::grow(sp, var as u32 * 131 + 7);
    let d = detail(sp);

    // Trunk + branches, UV'd into the atlas bark strip so this shares the leaf material.
    let mut raw = build_wood(&sk, d.sides, d.max_level, Some(atlas::bark_uv(sp)), wood_tint(sp, var));
    // Multiple cards fanned per anchor — the only way to add crown volume, since the generator
    // only anchors leaves at branch tips. 1.5 size (was 1.35) widens each sprig to close the gaps
    // between tips without needing anchors that don't exist.
    let sprigs = build_sprigs(&sk, sp, d.every, 1.5, d.quads, foliage_tint(sp, var));
    raw.append_transformed(&sprigs, Vec3::ZERO, 0.0, 1.0);

    // Shrink to world scale. Uniform, so `append_transformed`'s yaw-only normal handling stays
    // correct (it does no inverse-transpose — fine for a uniform scale, wrong for anything else).
    let mut out = MeshData::default();
    out.append_transformed(&raw, Vec3::ZERO, 0.0, GEN_TO_WORLD);
    out.to_mesh()
}
