//! **Knight hero model** — a faithful Bevy port of the user's procedural three.js
//! "Low-Poly Knight Studio" (`knightBuilder.ts`, the *knight* branch): a finely-articulated knight
//! (hips → torso → neck → head; shoulder → elbow → hand+weapon/shield; hip → knee → foot) in steel
//! plate with light-steel trim, a red helm crest, a gold rampant-lion heater shield, and the
//! equipped weapon on its own pivot.
//!
//! This builds only the **meshes** (one merged, flat-shaded, vertex-coloured `Mesh` per joint,
//! against the shared white creature material); [`super`] spawns the actual joint *hierarchy* of
//! entities from them, and [`super::anim`] poses the joints. Authoring is in the studio's TS units
//! (the knight stands ~1.85u tall before scale); `HERO_SCALE` brings it down to the orks' height.
//!
//! Geometry maps the studio's three.js primitives onto our helpers 1:1:
//! `CylinderGeometry(rt,rb,h,seg)` → [`frustum`], `BoxGeometry` → [`cuboid`], `SphereGeometry` →
//! [`ball`], `ConeGeometry` → [`cone`], `TorusGeometry` → [`torus`]; `position`→`off`, `scale`→
//! `scale`, Euler `rotation`→`xyz`/`rx`/`ry`/`rz` (three.js local matrix is `T*R*S`, same as
//! [`part`]). Nested three.js `Group`s compose via [`node`].

use bevy::asset::RenderAssetUsages;
use bevy::mesh::MeshBuilder;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::f32::consts::PI;

use crate::creature::{surf_code, Surf};
use crate::palette::lin;

// ── Palette (the customizer's defaults, sRGB hex) ─────────────────────────────────────
const ARMOR: u32 = 0x949aa8; // primaryArmor — steel plate
const TRIM: u32 = 0xa3afc2; // trimColor — light-steel trim (hilt/buckle/greave/collar)
const PLUME: u32 = 0xb82424; // plumeColor — red helm crest
const SHIELD_BASE: u32 = 0x4e3826; // brown shield face (reference: matte brown + gold accents)
const EMBLEM: u32 = 0xdbac42; // gold rampant lion + heraldic cross
const SKIRT: u32 = 0x453a2e; // surcoat/tabard/tassets — dark brown (reference tabard, not maroon)
const BLADE: u32 = 0xcfd3dc; // bladeColor — default steel blade
const HILT: u32 = 0xa3afc2; // hiltMat = trimColor
const GRIP: u32 = 0x4e3b31; // gripColor — leather grip
const BELT: u32 = 0x3d2b20; // belt + pouch + strap leather (studio-hardcoded 0x3d2b20)
const DARK: u32 = 0x121213; // eye gaps + nose shadow (studio darkMat)
const CORE: u32 = 0x3d3d3d; // sword fuller groove (studio coreMat)
const MAIL: u32 = 0x383c42; // dark mail hauberk / coif / aventail / spaulder pad (studio mailMat)
const SKIN: u32 = 0xb98562; // exposed face / forearms / hands (studio skinMat)
const DARKCOAT: u32 = 0x22242b; // sabaton sole (studio darkCoatMat)
const GOLD: u32 = 0xe8b84b; // golden weapon gilding
const AXE_STEEL: u32 = 0xaab0bc;
const STONE: u32 = 0x8a8d92;
const FROST: u32 = 0xaad2f0;
const GAMBESON: u32 = 0xb9bcc2; // pale padded under-tunic (skirt + collar)
const GLOVE: u32 = 0x6d7178; // darker steel gauntlets / boots
const CHEST: u32 = 0x5d4836; // brown leather gambeson chest (reference/previs torso)
const TABARD: u32 = 0x8a5a34; // lighter-brown surcoat/skirt cloth (previs tabard)
// ── previs palette (tools/index.html `C`) — the look the user signed off on ──
const PSTEEL: u32 = 0x808d9d;
const PSTEEL_LT: u32 = 0x95a2b2;
const PSTEEL_DK: u32 = 0x4c5562;
const PSTEEL_DIM: u32 = 0x6e7886;
const PLEATHER: u32 = 0x5d4836;
const PLEATHER_DK: u32 = 0x3b2e21;
const PTABARD: u32 = 0x9a6438;
const PTABARD_DK: u32 = 0x6f4626;
const PGLOVE: u32 = 0x282320;
const PGOLD: u32 = 0xb8902f;
const PDARK: u32 = 0x191c22;
const PBLADE: u32 = 0xc3c7cd;
const PGRIP: u32 = 0x6a4a2e;
const PSHIELD: u32 = 0x2b2723;

// ── PROPORTIONS (reference v2.0 turnaround): stylised/stocky knight, 6.5 heads tall. ──────────────
// Unit = head height (HH). Feet on the ground (rig-local y=0); head top at 6.5·HH ≈ 1.82.
// Vertical bands from the feet, in HH: boots 0–0.4 · calf 0.4–1.9 · thigh 1.9–3.3 · belt 3.3–3.9 ·
// torso 3.9–5.3 · neck 5.3–5.5 · head 5.5–6.5. The hips spine-root sits at 3.75 HH (=1.05, the
// anim-fixed value). Every pivot below is derived from these numbers — never eyeballed.
// Rig offsets are the PREVIS joint pivots (tools/index.html, in head-units where the figure is ~5.6u
// tall) multiplied by K so the rig lands at ~2.35u — the scale the animator's translation deltas
// (hips bob, Y_HIPS=1.05, shield mount, landing dip) were tuned for. Per-joint meshes are likewise
// built at previs scale and shrunk by K (see `gk`). HERO_SCALE then brings it to in-world size.
// EXACT previs (tools/index.html) joint pivots × K — the proportions the user approved on `knight2`.
// previs world Ys: hip-joint 2.55 · knee 1.40 · shoulder 3.84 · elbow 2.86 · neck 4.10 · waist ~2.5;
// K lands the waist at Y_HIPS=1.05 for the animator. Per-joint meshes are likewise built at previs
// scale and shrunk by K (see `gk`).
pub(crate) const K: f32 = 0.42; // previs-unit → rig scale
pub(crate) const HH: f32 = 0.28; // (legacy; unused)
pub(crate) const Y_HIPS: f32 = 1.05; // spine root = previs waist 2.5 × K
pub(crate) const O_TORSO: f32 = 0.0; // torso pivot = hips (waist)
pub(crate) const O_NECK: f32 = 0.60; // torso → neck (head shrunk ⇒ sits a touch lower)
pub(crate) const O_HEAD: f32 = 0.0;
pub(crate) const O_SHOULDER_Y: f32 = 0.563; // torso → shoulder (previs 1.34 × K)
pub(crate) const SHOULDER_DX: f32 = 0.386; // half shoulder span (previs 0.92 × K)
pub(crate) const O_ELBOW: f32 = -0.358; // shoulder → elbow (arms shortened ~13%)
pub(crate) const O_HAND: f32 = -0.497; // elbow → hand (arms shortened ~13%)
pub(crate) const HIP_DX: f32 = 0.193; // half hip span (previs 0.46 × K)
pub(crate) const O_HIP_Y: f32 = 0.021; // hips → hip joint (previs 0.05 × K)
pub(crate) const O_KNEE: f32 = -0.483; // hip → knee (previs 1.15 × K)
pub(crate) const O_FOOT: f32 = -0.588; // knee → ankle (previs 1.40 × K)

/// Tip of the held weapon in **sword-local** space (top of the arming-sword blade), read by
/// `combat::hero_blade_trail` off the [`super::HeroWeapon`] global transform.
pub const WEAPON_TIP_LOCAL: Vec3 = Vec3::new(0.0, 1.16, 0.0); // previs blade tip (2.76 × K)

// ── Mesh helpers (the orks/critters contract: primitives → tint → merge → flat-shade) ──
fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
#[allow(dead_code)]
fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
/// Map a part colour to its surface family so the shader textures it appropriately — plate/blade as
/// brushed metal, the belt/grip/cloth/crest as fabric/leather, eyes as a soft skin band. The code
/// rides in the vertex-colour alpha *per part*, so it survives the merge (one joint mesh, many surfaces).
fn surf_for(c: u32) -> Surf {
    match c {
        SKIRT | BELT | GRIP | PLUME | DARKCOAT | CHEST | TABARD | GAMBESON => Surf::Cloth,
        PLEATHER | PLEATHER_DK | PTABARD | PTABARD_DK | PGLOVE | PGRIP | PDARK | PSHIELD => Surf::Cloth,
        SKIN => Surf::Skin,
        _ => Surf::Metal,
    }
}
fn tinted(mut m: Mesh, c: u32) -> Mesh {
    let n = m.count_vertices();
    let mut col = lin(c);
    col[3] = surf_code(surf_for(c)); // surface family in alpha (the shader's per-surface texture)
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![col; n]);
    m
}
/// Merge a joint's parts into ONE flat-shaded, vertex-coloured mesh (duplicate FIRST — the flat
/// normals pass panics on an indexed mesh). Per-part surfaces are already baked into the alpha.
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn cuboid(w: f32, h: f32, d: f32) -> Mesh {
    Cuboid::new(w, h, d).mesh().build()
}
/// Tapered cylinder = `ConicalFrustum` (`rt==rb` → plain cylinder, `rt==0` → cone). Args mirror
/// three.js `CylinderGeometry(radiusTop, radiusBottom, height, radialSegments)`.
fn frustum(rt: f32, rb: f32, h: f32, res: u32) -> Mesh {
    ConicalFrustum { radius_top: rt, radius_bottom: rb, height: h }.mesh().resolution(res).build()
}
fn cone(r: f32, h: f32, res: u32) -> Mesh {
    Cone { radius: r, height: h }.mesh().resolution(res).build()
}
fn ball(r: f32) -> Mesh {
    Sphere::new(r).mesh().ico(2).unwrap() // ico(2) — smoother dome facets (pauldron/couter/poleyn/crown)
}
/// A chamfered (beveled-edge) box — the smooth low-poly "rounded box" look of the previs (three.js
/// `RoundedBoxGeometry` + flat shading), replacing the game's sharp [`cuboid`] on the plate parts.
/// 24 verts (each face an inset rect) joined by 12 edge bevels + 8 corner tris; `e` = chamfer inset.
/// Winding is auto-fixed outward (convex, origin-centred) so `group`'s flat-normals come out right.
fn chamfer_box(w: f32, h: f32, d: f32, e: f32) -> Mesh {
    let (a, b, c) = (w * 0.5, h * 0.5, d * 0.5);
    let e = e.min(a * 0.49).min(b * 0.49).min(c * 0.49).max(0.001);
    let (ai, bi, ci) = (a - e, b - e, c - e);
    let pos: Vec<[f32; 3]> = vec![
        [a, -bi, -ci], [a, bi, -ci], [a, bi, ci], [a, -bi, ci], // +X (0..3)
        [-a, -bi, -ci], [-a, bi, -ci], [-a, bi, ci], [-a, -bi, ci], // -X (4..7)
        [-ai, b, -ci], [ai, b, -ci], [ai, b, ci], [-ai, b, ci], // +Y (8..11)
        [-ai, -b, -ci], [ai, -b, -ci], [ai, -b, ci], [-ai, -b, ci], // -Y (12..15)
        [-ai, -bi, c], [ai, -bi, c], [ai, bi, c], [-ai, bi, c], // +Z (16..19)
        [-ai, -bi, -c], [ai, -bi, -c], [ai, bi, -c], [-ai, bi, -c], // -Z (20..23)
    ];
    let mut raw: Vec<[u32; 3]> = Vec::new();
    let mut quad = |a: u32, b: u32, c: u32, d: u32| {
        raw.push([a, b, c]);
        raw.push([a, c, d]);
    };
    for f in 0..6u32 {
        let o = f * 4;
        quad(o, o + 1, o + 2, o + 3); // 6 face quads
    }
    // 12 edge bevels (each links two faces' shared corner pair)
    let edges = [
        [1, 2, 10, 9], [3, 0, 13, 14], [6, 5, 8, 11], [7, 4, 12, 15], // ±X with ±Y
        [3, 2, 18, 17], [0, 1, 22, 21], [7, 6, 19, 16], [4, 5, 23, 20], // ±X with ±Z
        [11, 10, 18, 19], [8, 9, 22, 23], [15, 14, 17, 16], [12, 13, 21, 20], // ±Y with ±Z
    ];
    for q in edges {
        quad(q[0], q[1], q[2], q[3]);
    }
    // 8 corner tris
    for t in [[2, 10, 18], [1, 9, 22], [3, 14, 17], [0, 13, 21], [6, 11, 19], [5, 8, 23], [7, 15, 16], [4, 12, 20]] {
        raw.push(t);
    }
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx: Vec<u32> = Vec::new();
    for t in raw {
        let (va, vb, vc) = (g(t[0]), g(t[1]), g(t[2]));
        let n = (vb - va).cross(vc - va);
        let ctr = (va + vb + vc) / 3.0;
        if n.dot(ctr) >= 0.0 {
            idx.extend(t);
        } else {
            idx.extend([t[0], t[2], t[1]]);
        }
    }
    let n = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}
/// A flat ring (three.js `TorusGeometry(major, minor)`).
fn torus(major: f32, minor: f32) -> Mesh {
    Torus { minor_radius: minor, major_radius: major }.mesh().build()
}
/// Place a primitive: scale → rotate → translate (matching three.js' `T*R*S`), then tint.
fn part(mut m: Mesh, scale: Vec3, rot: Quat, off: Vec3, c: u32) -> Mesh {
    if scale != Vec3::ONE {
        m = m.scaled_by(scale);
    }
    if rot != Quat::IDENTITY {
        m = m.rotated_by(rot);
    }
    tinted(m.translated_by(off), c)
}
fn at(m: Mesh, off: Vec3, c: u32) -> Mesh {
    part(m, Vec3::ONE, Quat::IDENTITY, off, c)
}
/// Compose one level of three.js `Group` nesting: place `m` in the child's local frame
/// (scale → `crot` → `coff`), then apply the parent group's (`prot`, `poff`), then tint. Matches
/// three.js' `parentMatrix * childMatrix` (both `T*R*S`).
fn node(mut m: Mesh, cscale: Vec3, crot: Quat, coff: Vec3, prot: Quat, poff: Vec3, c: u32) -> Mesh {
    if cscale != Vec3::ONE {
        m = m.scaled_by(cscale);
    }
    if crot != Quat::IDENTITY {
        m = m.rotated_by(crot);
    }
    m = m.translated_by(coff);
    if prot != Quat::IDENTITY {
        m = m.rotated_by(prot);
    }
    tinted(m.translated_by(poff), c)
}

/// Sample a quadratic Bézier `p0 → p1` about control `c` into `steps` segments (the trailing
/// endpoint is included; the leading one is skipped so chained curves don't double a vertex).
fn quad(p0: Vec2, c: Vec2, p1: Vec2, steps: u32, out: &mut Vec<Vec2>) {
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        out.push(p0 * (u * u) + c * (2.0 * u * t) + p1 * (t * t));
    }
}

/// A flat extruded polygon (front +Z face + back −Z face + side walls) from a 2D outline in the XY
/// plane, extruded ±`depth/2` in Z. Triangulated as a fan from the centroid (a shield outline is
/// star-convex about its centre). Carries POSITION/NORMAL/UV_0 so it merges with the primitives
/// (the flat-normals pass in [`group`] recomputes the dummy normals). Outline must be CCW seen from
/// +Z so the front face points at the camera.
fn extrude_poly(pts: &[Vec2], depth: f32) -> Mesh {
    let n = pts.len();
    let c = pts.iter().copied().sum::<Vec2>() / n as f32;
    let hz = depth * 0.5;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    // Front face (+Z): fan from centroid, CCW.
    let fc = pos.len() as u32;
    pos.push([c.x, c.y, hz]);
    for p in pts {
        pos.push([p.x, p.y, hz]);
    }
    for i in 0..n {
        idx.extend([fc, fc + 1 + i as u32, fc + 1 + ((i + 1) % n) as u32]);
    }
    // Back face (−Z): reverse winding.
    let bc = pos.len() as u32;
    pos.push([c.x, c.y, -hz]);
    for p in pts {
        pos.push([p.x, p.y, -hz]);
    }
    for i in 0..n {
        idx.extend([bc, bc + 1 + ((i + 1) % n) as u32, bc + 1 + i as u32]);
    }
    // Side walls.
    for i in 0..n {
        let p0 = pts[i];
        let p1 = pts[(i + 1) % n];
        let b = pos.len() as u32;
        pos.push([p0.x, p0.y, hz]);
        pos.push([p1.x, p1.y, hz]);
        pos.push([p1.x, p1.y, -hz]);
        pos.push([p0.x, p0.y, -hz]);
        idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    let nverts = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; nverts]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; nverts]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}

/// An open curved wall — a partial cylinder/cone (three.js `CylinderGeometry` with `thetaStart` /
/// `thetaLength`, open-ended). `theta = 0` faces +Z. Double-sided so it reads from in front
/// regardless of winding. Kept for porting partial-cylinder parts (e.g. a closed-helm visor shell).
#[allow(dead_code)]
fn arc_shell(rt: f32, rb: f32, h: f32, theta0: f32, theta_len: f32, segs: u32) -> Mesh {
    let hy = h * 0.5;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    for i in 0..=segs {
        let th = theta0 + theta_len * (i as f32 / segs as f32);
        let (s, c) = (th.sin(), th.cos());
        pos.push([rt * s, hy, rt * c]); // top ring
        pos.push([rb * s, -hy, rb * c]); // bottom ring
    }
    let mut idx: Vec<u32> = Vec::new();
    for i in 0..segs {
        let b = (i * 2) as u32;
        idx.extend([b, b + 1, b + 3, b, b + 3, b + 2]); // outward
        idx.extend([b, b + 3, b + 1, b, b + 2, b + 3]); // inward (double-sided)
    }
    let n = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}

/// A tapered cloth/plate panel (studio `createSurcoatPanelGeometry`): a 6-point shape wide at the
/// shoulders, pinched at the waist, tapering to the hem — extruded `depth` thick. Faces +Z.
fn surcoat_panel(top_w: f32, waist_w: f32, bottom_w: f32, h: f32, depth: f32) -> Mesh {
    let (ty, wy, by) = (h / 2.0, h * 0.02, -h / 2.0);
    // Studio winding is CW seen from +Z → reverse to CCW for `extrude_poly`'s front face.
    let mut pts = vec![
        Vec2::new(-top_w / 2.0, ty),
        Vec2::new(top_w / 2.0, ty),
        Vec2::new(waist_w / 2.0, wy),
        Vec2::new(bottom_w / 2.0, by),
        Vec2::new(-bottom_w / 2.0, by),
        Vec2::new(-waist_w / 2.0, wy),
    ];
    pts.reverse();
    extrude_poly(&pts, depth)
}

// ── Equipped-armor tint ───────────────────────────────────────────────────────────────
/// Lerp an sRGB-hex colour `t` of the way toward `target` (byte space — the "feels the same" bar).
fn lerp_hex(c: u32, target: u32, t: f32) -> u32 {
    let ch = |x: u32, s: u32| ((x >> s) & 0xff) as f32;
    let mix = |a: f32, b: f32| (a + (b - a) * t).round().clamp(0.0, 255.0) as u32;
    (mix(ch(c, 16), ch(target, 16)) << 16) | (mix(ch(c, 8), ch(target, 8)) << 8) | mix(ch(c, 0), ch(target, 0))
}

/// The plate colour triple `(base, light, dark)` for the worn armor — `None` (bare) is the default
/// steel plate; a tier lerps light→white / dark→black for facet depth.
/// A worn armor's look. The whole STEEL plate (helm/pauldrons/arms/legs/boots/gorget) recolours to
/// `metal*`, accents (rivets/buckles/crest) to `trim`, and `style` adds signature geometry (dragon
/// spikes, a gold crest…). The cloth gambeson/tabard + dark gauntlets stay constant under it. Bare =
/// the default steel knight. (Previously armor only fed an unused colour — the hero never changed.)
#[derive(Clone, Copy, PartialEq)]
enum ArmorStyle {
    Steel,
    Leather,
    Iron,
    Gold,
    Dragon,
}

#[derive(Clone, Copy)]
struct Skin {
    style: ArmorStyle,
    metal: u32,
    metal_lt: u32,
    metal_dk: u32,
    metal_dim: u32,
    trim: u32,
}

fn skin_for(armor: Option<&str>) -> Skin {
    let (style, metal, trim) = match armor {
        Some("leather_armor") => (ArmorStyle::Leather, 0x6e4a2c, 0x8a6a3a), // brown hide + bronze
        Some("iron_armor") => (ArmorStyle::Iron, 0xb7bfca, 0x808b99), // bright steel
        Some("gold_armor") => (ArmorStyle::Gold, 0xc8a23a, 0xffe9a0), // gilded
        Some("dragon_plate") => (ArmorStyle::Dragon, 0x356b48, 0xd2bf92), // dragon green + bone
        _ => (ArmorStyle::Steel, PSTEEL, PGOLD), // bare / iron sword starter
    };
    Skin {
        style,
        metal,
        metal_lt: lerp_hex(metal, 0xffffff, 0.22),
        metal_dk: lerp_hex(metal, 0x000000, 0.32),
        metal_dim: lerp_hex(metal, 0x000000, 0.14),
        trim,
    }
}

// ── Held weapon (broadsword + the parity-port variants: iron/axe/maul/gold/frost) ─────
// All authored in sword-LOCAL with the blade along **+Y** (studio `broadsword` group); the `Sword`
// joint owns the held rotation (`anim` sets the studio `(2.2,0.3,0)` rest + the attack sweeps).
fn sword_parts(blade: u32) -> Vec<Mesh> {
    vec![
        at(rbx(0.08, 0.5, 0.1, 0.03), v(0.0, 0.0, 0.0), PGRIP), // grip (base at hand origin)
        at(rbx(0.5, 0.12, 0.16, 0.04), v(0.0, 0.3, 0.0), PGOLD), // crossguard
        at(tplate(0.18, 2.4, 0.06, 0.18, 0.5, 0.02), v(0.0, 0.36, 0.0), blade), // tapered blade
        at(lathe(&[[0.0, 0.11], [0.11, 0.07], [0.11, -0.05], [0.0, -0.09]], 8), v(0.0, -0.29, 0.0), PGOLD), // pommel
    ]
}

/// The held-weapon mesh for the equipped item, in sword-local (+Y) space (unknown id → broadsword).
fn weapon_parts(weapon: Option<&str>) -> Vec<Mesh> {
    match weapon {
        Some("axe") => vec![
            at(frustum(0.028, 0.028, 0.8, 6), v(0.0, 0.12, 0.0), GRIP),
            at(frustum(0.034, 0.034, 0.05, 6), v(0.0, 0.27, 0.0), HILT),
            at(frustum(0.034, 0.034, 0.05, 6), v(0.0, -0.12, 0.0), HILT),
            at(ball(0.04), v(0.0, -0.3, 0.0), HILT),
            at(cuboid(0.26, 0.22, 0.05), v(0.13, 0.42, 0.0), AXE_STEEL),
            part(cone(0.11, 0.14, 6), Vec3::ONE, rz(-PI / 2.0), v(0.28, 0.42, 0.0), AXE_STEEL),
            part(cone(0.05, 0.10, 6), Vec3::ONE, rz(PI / 2.0), v(-0.04, 0.42, 0.0), AXE_STEEL),
        ],
        // Ornate gilded sword: winged crossguard + a fullered gold blade + a fat pommel.
        Some("sword_gold") => vec![
            at(rbx(0.08, 0.5, 0.1, 0.03), v(0.0, 0.0, 0.0), PGRIP), // grip
            at(rbx(0.66, 0.12, 0.16, 0.04), v(0.0, 0.3, 0.0), GOLD), // wide crossguard
            part(cone(0.07, 0.2, 6), Vec3::ONE, rz(-PI / 2.0), v(0.3, 0.3, 0.0), GOLD), // wing R
            part(cone(0.07, 0.2, 6), Vec3::ONE, rz(PI / 2.0), v(-0.3, 0.3, 0.0), GOLD), // wing L
            at(tplate(0.22, 2.5, 0.06, 0.16, 0.5, 0.02), v(0.0, 0.36, 0.0), GOLD), // blade
            at(tplate(0.05, 2.0, 0.018, 0.5, 0.6, 0.01), v(0.0, 0.5, 0.045), lerp_hex(GOLD, 0xffffff, 0.4)), // fuller
            at(lathe(&[[0.0, 0.13], [0.13, 0.08], [0.13, -0.05], [0.0, -0.11]], 8), v(0.0, -0.31, 0.0), GOLD), // pommel
        ],
        // Frostfang GREATSWORD (top tier): a long, wide two-hander with an icy fuller.
        Some("blade_frost") => vec![
            at(rbx(0.08, 0.62, 0.1, 0.03), v(0.0, -0.06, 0.0), PGRIP), // long two-hand grip
            at(rbx(0.66, 0.13, 0.18, 0.04), v(0.0, 0.32, 0.0), lerp_hex(FROST, 0xffffff, 0.3)), // wide crossguard
            at(tplate(0.3, 3.1, 0.07, 0.16, 0.5, 0.02), v(0.0, 0.38, 0.0), FROST), // long wide blade
            at(tplate(0.06, 2.6, 0.02, 0.4, 0.6, 0.01), v(0.0, 0.5, 0.05), lerp_hex(FROST, 0xffffff, 0.45)), // icy fuller
            at(lathe(&[[0.0, 0.12], [0.12, 0.08], [0.12, -0.05], [0.0, -0.1]], 8), v(0.0, -0.4, 0.0), lerp_hex(FROST, 0xffffff, 0.3)), // pommel
        ],
        Some("stone_maul") => vec![
            at(frustum(0.035, 0.035, 0.95, 6), v(0.0, 0.1, 0.0), GRIP),
            at(frustum(0.042, 0.042, 0.06, 6), v(0.0, 0.32, 0.0), HILT),
            at(ball(0.045), v(0.0, -0.36, 0.0), HILT),
            at(cuboid(0.34, 0.26, 0.26), v(0.0, 0.6, 0.0), STONE),
            at(cuboid(0.36, 0.04, 0.27), v(0.0, 0.52, 0.0), HILT),
            at(cuboid(0.36, 0.04, 0.27), v(0.0, 0.68, 0.0), HILT),
            at(cuboid(0.06, 0.2, 0.2), v(0.19, 0.6, 0.0), STONE),
            at(cuboid(0.06, 0.2, 0.2), v(-0.19, 0.6, 0.0), STONE),
        ],
        _ => sword_parts(PBLADE),
    }
}

// ── previs primitives: tapered chamfer box (`plate`), lathe (`rev`), and the K-shrink merge ──
fn tplate(w: f32, h: f32, d: f32, top_w: f32, top_d: f32, e: f32) -> Mesh {
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
    for vtx in pos.iter_mut() {
        let f = (vtx[1] + b) / h;
        vtx[0] *= 1.0 + (top_w - 1.0) * f;
        vtx[2] *= 1.0 + (top_d - 1.0) * f;
        vtx[1] += b;
    }
    let center = Vec3::new(0.0, b, 0.0);
    let edges = [[1, 2, 10, 9], [3, 0, 13, 14], [6, 5, 8, 11], [7, 4, 12, 15], [3, 2, 18, 17], [0, 1, 22, 21], [7, 6, 19, 16], [4, 5, 23, 20], [11, 10, 18, 19], [8, 9, 22, 23], [15, 14, 17, 16], [12, 13, 21, 20]];
    let corners = [[2, 10, 18], [1, 9, 22], [3, 14, 17], [0, 13, 21], [6, 11, 19], [5, 8, 23], [7, 15, 16], [4, 12, 20]];
    let mut raw: Vec<[u32; 3]> = Vec::new();
    for f in 0..6u32 {
        let o = f * 4;
        raw.push([o, o + 1, o + 2]);
        raw.push([o, o + 2, o + 3]);
    }
    for q in edges {
        raw.push([q[0], q[1], q[2]]);
        raw.push([q[0], q[2], q[3]]);
    }
    for t in corners {
        raw.push(t);
    }
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
    let n = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}
fn rbx(w: f32, h: f32, d: f32, e: f32) -> Mesh {
    tplate(w, h, d, 1.0, 1.0, e)
}
fn lathe(profile: &[[f32; 2]], segs: u32) -> Mesh {
    let n = profile.len() as u32;
    let mut pos: Vec<[f32; 3]> = Vec::new();
    for s in 0..=segs {
        let th = (s as f32 / segs as f32) * std::f32::consts::TAU;
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
    let nn = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; nn]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; nn]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}
/// merge previs-scale parts → one flat-shaded joint mesh, shrunk by `K` to fit the rig offsets.
fn gk(parts: Vec<Mesh>) -> Mesh {
    group(parts).scaled_by(Vec3::splat(K))
}

// ── Per-joint geometry (each in that joint's LOCAL space) ──────────────────────────────
// FROM-SCRATCH rebuild to the reference v2.0 turnaround: plain, clean low-poly bryła — proportions
// are HH-derived (see PROPORTIONS), not eyeballed. Details (heraldry/etching) come later; this stage
// is silhouette only. Steel = ARMOR (`a`), pale under-tunic = GAMBESON, brown tabard/belt = SKIRT/
// BELT, dark gauntlets/boots = GLOVE, brown shield = SHIELD_BASE + gold trim.

/// Hips: a slim gambeson pelvis, the brown belt, and the pale gambeson skirt hanging to mid-thigh.
fn hips_mesh(s: &Skin) -> Mesh {
    gk(vec![
        at(tplate(1.0, 0.4, 1.04, 1.12, 1.1, 0.08), v(0.0, -0.18, 0.0), s.metal), // fauld
        at(rbx(1.08, 0.2, 1.02, 0.05), v(0.0, -0.08, 0.0), PLEATHER_DK), // belt
        part(lathe(&[[0.0, 0.13], [0.16, 0.1], [0.16, -0.02], [0.0, -0.05]], 10), Vec3::ONE, rx(PI / 2.0), v(0.0, 0.0, 0.55), s.trim), // buckle
        at(tplate(0.5, 0.6, 0.06, 1.4, 1.0, 0.03), v(0.0, -0.64, 0.42), PTABARD), // tabard front (shorter — a flap, not a belly)
        at(tplate(0.5, 0.6, 0.06, 1.4, 1.0, 0.03), v(0.0, -0.64, -0.42), PTABARD_DK), // tabard back
    ])
}

/// Torso: a grey gambeson body (chest tapering to the waist, wide in X / shallow in Z) under a brown
/// sleeveless tabard (front + back panels — the grey sides show), per the reference.
fn torso_mesh(s: &Skin) -> Mesh {
    let mut parts = vec![
        at(tplate(1.12, 1.55, 1.06, 1.18, 1.12, 0.16), v(0.0, 0.0, 0.0), PLEATHER), // gambeson chest
        at(tplate(0.22, 1.24, 0.12, 0.5, 1.0, 0.05), v(0.0, 0.16, 0.5), PLEATHER), // chest keel
        at(lathe(&[[0.0, 0.34], [0.5, 0.3], [0.56, 0.08], [0.5, 0.0], [0.0, 0.0]], 16), v(0.0, 1.45, 0.0), s.metal_lt), // gorget
        // ── back detail (the 3rd-person camera sees this most) ──
        at(tplate(0.9, 0.66, 0.1, 0.92, 1.0, 0.05), v(0.0, 0.9, -0.5), s.metal), // shoulder-blade backplate
        at(tplate(0.14, 1.36, 0.1, 0.5, 1.0, 0.04), v(0.0, 0.1, -0.5), s.metal_dk), // spine ridge
        at(rbx(1.06, 0.13, 0.08, 0.03), v(0.0, 1.02, -0.5), PLEATHER_DK), // upper back strap
        at(rbx(1.06, 0.13, 0.08, 0.03), v(0.0, 0.42, -0.5), PLEATHER_DK), // lower back strap
        at(rbx(0.11, 0.16, 0.07, 0.02), v(0.42, 1.02, -0.52), s.trim), // strap buckle (upper)
        at(rbx(0.11, 0.16, 0.07, 0.02), v(0.42, 0.42, -0.52), s.trim), // strap buckle (lower)
        at(rbx(0.13, 0.13, 0.06, 0.02), v(0.5, 0.95, 0.46), s.trim), // side cuirass buckle R
        at(rbx(0.13, 0.13, 0.06, 0.02), v(-0.5, 0.95, 0.46), s.trim), // side cuirass buckle L
    ];
    // Signature chest device per armor (silhouette/identity beyond the recolour).
    match s.style {
        ArmorStyle::Gold => {
            // Gilded chest boss + a rivet ring.
            parts.push(part(lathe(&[[0.0, 0.2], [0.13, 0.13], [0.17, 0.0], [0.0, -0.03]], 12), Vec3::ONE, rx(PI / 2.0), v(0.0, 0.55, 0.53), s.trim));
        }
        ArmorStyle::Dragon => {
            // A row of bone scale-spikes up the chest centre.
            for i in 0..3 {
                parts.push(part(cone(0.08, 0.2, 5), Vec3::ONE, rx(-PI / 2.0), v(0.0, 0.35 + i as f32 * 0.34, 0.52), s.trim));
            }
        }
        _ => {}
    }
    gk(parts)
}

/// Neck: the short steel collar stub (helm rides the Head joint above).
fn neck_mesh(s: &Skin) -> Mesh {
    gk(vec![at(rbx(0.42, 0.26, 0.42, 0.08), v(0.0, 0.0, 0.0), s.metal_dk)])
}

/// Helm: a plain closed rounded bascinet — a steel box face + a domed top, with a subtle dark visor
/// line. No plume / gold / etching yet (silhouette stage). Spans 1 HH (5.5→6.5).
fn head_mesh(s: &Skin) -> Mesh {
    let mut parts = vec![
        at(lathe(&[[0.0, 1.28], [0.3, 1.2], [0.5, 0.98], [0.56, 0.62], [0.57, 0.06], [0.5, 0.0], [0.0, 0.0]], 18), v(0.0, 0.2, 0.0), s.metal), // sugarloaf helm
        at(tplate(0.12, 1.0, 0.16, 0.6, 1.0, 0.04), v(0.0, 0.35, 0.5), s.metal_dim), // brow keel
        at(rbx(0.3, 0.08, 0.06, 0.02), v(0.17, 0.94, 0.49), PDARK), // eye slit R
        at(rbx(0.3, 0.08, 0.06, 0.02), v(-0.17, 0.94, 0.49), PDARK), // eye slit L
        at(rbx(0.05, 0.05, 0.05, 0.02), v(-0.18, 0.52, 0.52), PDARK), // breath holes
        at(rbx(0.05, 0.05, 0.05, 0.02), v(-0.06, 0.52, 0.52), PDARK),
        at(rbx(0.05, 0.05, 0.05, 0.02), v(0.06, 0.52, 0.52), PDARK),
        at(rbx(0.05, 0.05, 0.05, 0.02), v(0.18, 0.52, 0.52), PDARK),
        at(tplate(0.1, 0.8, 0.1, 0.5, 1.0, 0.03), v(0.0, 0.5, -0.46), s.metal_dk), // helm back ridge
        at(rbx(0.06, 0.06, 0.05, 0.02), v(0.48, 0.3, 0.24), s.trim), // helm rivets
        at(rbx(0.06, 0.06, 0.05, 0.02), v(-0.48, 0.3, 0.24), s.trim),
        at(rbx(0.06, 0.06, 0.05, 0.02), v(0.42, 0.3, -0.3), s.trim),
        at(rbx(0.06, 0.06, 0.05, 0.02), v(-0.42, 0.3, -0.3), s.trim),
    ];
    // Signature helm crest per armor.
    match s.style {
        ArmorStyle::Gold => {
            // A tall gilded fin crest running front-to-back over the dome.
            parts.push(at(tplate(0.08, 0.5, 1.0, 1.0, 0.2, 0.02), v(0.0, 1.5, 0.0), s.trim));
        }
        ArmorStyle::Dragon => {
            // A row of bone horn-spikes over the crown.
            for i in 0..4 {
                parts.push(part(cone(0.1, 0.34, 5), Vec3::ONE, rx(-0.25), v(0.0, 1.3, 0.34 - i as f32 * 0.22), s.trim));
            }
        }
        _ => {}
    }
    gk(parts).scaled_by(Vec3::splat(0.85)) // shrink the head (was reading too big)
}

/// Shoulder: a rounded steel pauldron cap + the upper arm (steel, tapering to the elbow). 1.3 HH.
fn shoulder_mesh(sign: f32, s: &Skin) -> Mesh {
    let mut parts = vec![
        part(lathe(&[[0.0, 0.32], [0.22, 0.29], [0.4, 0.18], [0.5, 0.03], [0.5, -0.14], [0.4, -0.22], [0.2, -0.24], [0.0, -0.24]], 16), Vec3::ONE, xyz(0.05, 0.0, sign * 0.12), v(0.0, -0.18, 0.02), s.metal_lt), // draping pauldron
        at(tplate(0.46, 0.68, 0.5, 0.92, 0.94, 0.08), v(0.0, -0.77, 0.0), s.metal), // rerebrace (shortened)
    ];
    // Dragon plate: two bone spikes jut out the top of each pauldron.
    if s.style == ArmorStyle::Dragon {
        parts.push(part(cone(0.11, 0.4, 5), Vec3::ONE, xyz(0.0, 0.0, sign * 0.7), v(sign * 0.34, 0.02, 0.04), s.trim));
        parts.push(part(cone(0.08, 0.28, 5), Vec3::ONE, xyz(0.0, 0.0, sign * 0.5), v(sign * 0.2, 0.1, -0.18), s.trim));
    }
    gk(parts)
}

/// Elbow: the forearm vambrace (steel) + a dark gauntlet fist at the wrist. Forearm 1.2 HH.
fn elbow_mesh(_sign: f32, s: &Skin) -> Mesh {
    gk(vec![
        at(lathe(&[[0.0, 0.26], [0.2, 0.22], [0.28, 0.08], [0.28, 0.0], [0.0, 0.0]], 14), v(0.0, 0.0, 0.04), s.metal_lt), // couter
        at(tplate(0.44, 0.73, 0.48, 0.78, 0.82, 0.08), v(0.0, -0.75, 0.0), s.metal), // vambrace (shortened)
        at(tplate(0.46, 0.4, 0.52, 0.8, 0.86, 0.06), v(0.0, -1.13, 0.0), PGLOVE), // gauntlet
        at(rbx(0.42, 0.16, 0.46, 0.05), v(0.0, -1.08, 0.16), s.metal_dk), // knuckle
    ])
}

/// Thigh: a steel cuisse tapering toward the knee. 1.4 HH.
fn hip_mesh(_sign: f32, s: &Skin) -> Mesh {
    gk(vec![at(tplate(0.52, 1.18, 0.62, 1.18, 1.12, 0.1), v(0.0, -1.13, 0.0), s.metal)]) // cuisse
}

/// Knee + shin: a steel poleyn cap + the greave column. 1.5 HH.
fn knee_mesh(s: &Skin) -> Mesh {
    gk(vec![
        at(lathe(&[[0.0, 0.34], [0.18, 0.3], [0.33, 0.16], [0.36, 0.0], [0.0, 0.0]], 14), v(0.0, -0.2, 0.16), s.metal_lt), // poleyn
        at(tplate(0.5, 1.12, 0.58, 0.78, 0.84, 0.09), v(0.0, -1.14, 0.0), s.metal), // greave
    ])
}

/// Boot: a dark steel sabaton (ankle + a short forward foot), bottoming on the ground (0.4 HH tall).
fn foot_mesh(s: &Skin) -> Mesh {
    gk(vec![
        at(rbx(0.5, 0.26, 0.66, 0.06), v(0.0, 0.0, -0.02), s.metal_dk), // sabaton
        at(tplate(0.46, 0.22, 0.5, 0.6, 0.7, 0.05), v(0.0, 0.04, 0.42), s.metal), // toe
    ])
}

/// Triangular heater shield: dark face + a bronze rim peeking behind it (studio extrudes a curved
/// `Shape`; we sample its quadratic outline into a polygon and [`extrude_poly`] it). Built in
/// shield-local facing +Z.
fn shield_mesh() -> Mesh {
    // previs heater outline (CW from +Z → reverse to CCW for extrude_poly's front face).
    let mut base = vec![Vec2::new(-0.5, 0.72), Vec2::new(0.5, 0.72), Vec2::new(0.53, 0.05)];
    quad(Vec2::new(0.53, 0.05), Vec2::new(0.46, -0.42), Vec2::new(0.0, -0.84), 5, &mut base);
    quad(Vec2::new(0.0, -0.84), Vec2::new(-0.46, -0.42), Vec2::new(-0.53, 0.05), 5, &mut base);
    base.reverse();
    let rim: Vec<Vec2> = base.iter().map(|p| *p * 1.08).collect();
    let mut parts = vec![
        tinted(extrude_poly(&rim, 0.07).translated_by(v(0.0, 0.0, -0.03)), PGOLD), // gold border
        tinted(extrude_poly(&base, 0.1).translated_by(v(0.0, 0.0, 0.03)), SHIELD_BASE), // brown face
        // Central domed gold boss + encircling ring — a classic shield boss, NOT any cross.
        part(lathe(&[[0.0, 0.15], [0.1, 0.1], [0.18, 0.02], [0.2, -0.02], [0.0, -0.05]], 16), Vec3::ONE, rx(PI / 2.0), v(0.0, 0.1, 0.13), PGOLD), // boss dome
        part(torus(0.29, 0.035), Vec3::ONE, rx(PI / 2.0), v(0.0, 0.1, 0.12), PGOLD), // ring around the boss
    ];
    for p in [[-0.4, -0.06], [0.4, -0.06], [-0.27, -0.56], [0.27, -0.56]] {
        parts.push(part(lathe(&[[0.0, 0.04], [0.055, 0.022], [0.0, -0.018]], 6), Vec3::ONE, rx(PI / 2.0), v(p[0], p[1], 0.11), PGOLD)); // corner rivets
    }
    // Larger heater (was ×K ⇒ too small to cover the body); ×1.4 reads as a proper kite shield.
    group(parts).scaled_by(Vec3::splat(K * 1.4))
}

/// The golden rampant-lion emblem (stylised low-poly boxes) mounted on the shield face. Faithful to
/// the studio `lionGroup` — 17 tinted boxes; `(w,h,d)`, position, z-rotation.
/// (Emblem is now baked into [`shield_mesh`] as the previs cross; this stays a tiny no-op so the
/// rig's lion-overlay slot spawns nothing visible.)
fn lion_mesh() -> Mesh {
    group(vec![at(cuboid(0.001, 0.001, 0.001), v(0.0, 0.0, 0.0), PDARK)])
}

// ── The full build (one mesh per joint + the held weapon) ─────────────────────────────
/// Every joint mesh + the held-weapon mesh. [`super::spawn_hero_meshes`] spawns the joint hierarchy
/// from this; an equip change rebuilds it (`super::reskin_hero`). The weapon is mounted on its own
/// `Sword` joint (no static transform here — `anim` owns the held pose).
pub struct KnightMeshes {
    pub hips: Mesh,
    pub torso: Mesh,
    pub neck: Mesh,
    pub head: Mesh,
    pub shoulder_l: Mesh,
    pub shoulder_r: Mesh,
    pub elbow_l: Mesh,
    pub elbow_r: Mesh,
    pub hip_l: Mesh,
    pub hip_r: Mesh,
    pub knee_l: Mesh,
    pub knee_r: Mesh,
    pub foot_l: Mesh,
    pub foot_r: Mesh,
    pub shield: Mesh,
    pub lion: Mesh,
    pub weapon: Mesh,
}

/// Build all knight meshes reflecting the equipped gear: the held weapon swaps geometry and the
/// worn armor recolours the plate (bare = default steel). Re-called by `super::reskin_hero`.
pub fn build_knight(weapon: Option<&str>, armor: Option<&str>) -> KnightMeshes {
    let s = skin_for(armor);
    KnightMeshes {
        hips: hips_mesh(&s),
        torso: torso_mesh(&s),
        neck: neck_mesh(&s),
        head: head_mesh(&s),
        shoulder_l: shoulder_mesh(-1.0, &s),
        shoulder_r: shoulder_mesh(1.0, &s),
        elbow_l: elbow_mesh(-1.0, &s),
        elbow_r: elbow_mesh(1.0, &s),
        hip_l: hip_mesh(-1.0, &s),
        hip_r: hip_mesh(1.0, &s),
        knee_l: knee_mesh(&s),
        knee_r: knee_mesh(&s),
        foot_l: foot_mesh(&s),
        foot_r: foot_mesh(&s),
        shield: shield_mesh(),
        lion: lion_mesh(),
        weapon: gk(weapon_parts(weapon)),
    }
}
