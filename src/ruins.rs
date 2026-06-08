//! Landmark props — a weathered standing-stone trilithon + a giant bare dead tree,
//! the background silhouettes the TS forest screenshot shows. Built as single merged
//! vertex-coloured meshes (one per landmark) so they batch against the scene's shared
//! white material; each model's BASE sits at y=0 and the scatter places/scales it.
//!
//! Shapes/colours follow the TS landmarks (`StandingStones.tsx`, `GiantDeadTree.tsx`)
//! distilled to the brief: the trilithon is a clean Stonehenge-style pair of uprights
//! + lintel (mottled greys), the dead tree a tall tapered gnarled trunk with a few
//! thick bare branches forking high up (weathered grey-brown). Rust/Bevy 0.18 API per
//! `CONTRACT.md` + the verified-APIs doc §9.

use bevy::prelude::*;

use crate::palette::{lin, lin_scaled, DEAD_WOOD, DEAD_WOOD_DARK};

// ── Trilithon stone tints (mottled weathered granite greys) ──────────────────
const STONE_A: u32 = 0x9a9aa3; // left upright — cool light grey
const STONE_B: u32 = 0x8a8f98; // lintel — mid grey (the cross-beam reads darker)
const STONE_C: u32 = 0xaab0b2; // right upright — pale warm grey
const STONE_CAP: u32 = 0x6b6862; // shaded hewn caps / shadowed underside
const STONE_MOSS: u32 = 0x74803f; // subtle moss accent creeping up a base

// ── Dead-tree wood tints ─────────────────────────────────────────────────────
const TREE_BARK: u32 = DEAD_WOOD; // 0x6e6258 weathered grey-brown
const TREE_BARK_DARK: u32 = DEAD_WOOD_DARK; // 0x4a4238 lower trunk / collar
const TREE_ROOT: u32 = 0x2f2418; // near-black wet root wood at the base

// ── Mesh helpers (verified 0.18 forms) ───────────────────────────────────────

/// Tag every vertex of a part with one flat linear colour. REQUIRED before merge —
/// all merged parts must carry the same attribute set, and the scene's shared white
/// `StandardMaterial` reads colour straight off `ATTRIBUTE_COLOR`.
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

/// Merge a list of already-tinted parts into one mesh. `Mesh::merge` returns a
/// `Result` in 0.18 (all parts share POSITION/NORMAL/UV_0/COLOR, so it never fails).
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("parts share attributes");
    }
    base
}

/// A box primitive, positioned so its CENTER lands at `center`.
fn box_at(x: f32, y: f32, z: f32, center: Vec3) -> Mesh {
    Cuboid::new(x, y, z).mesh().build().translated_by(center)
}

/// A cylinder primitive whose BASE sits at the local origin, then optionally pitched
/// about Z and yawed about Y, then translated to its attach point. Order: lift the
/// base-centred cylinder so its base is at origin → rotate (so it swings about its
/// base) → translate to where it attaches. `resolution` keeps the low-poly facet look.
fn limb(radius: f32, height: f32, resolution: u32, pitch_z: f32, yaw_y: f32, attach: Vec3) -> Mesh {
    let rot = Quat::from_rotation_y(yaw_y) * Quat::from_rotation_z(pitch_z);
    Cylinder::new(radius, height)
        .mesh()
        .resolution(resolution)
        .build()
        .translated_by(Vec3::new(0.0, height * 0.5, 0.0)) // base → origin
        .rotated_by(rot)
        .translated_by(attach)
}

// ── Trilithon ─────────────────────────────────────────────────────────────────

/// Two upright weathered standing stones with a lintel across the top — a clean
/// Stonehenge-style trilithon, ~3u tall, base at y=0. Each upright is a tapered
/// pillar (a tall box bulk + a narrower hewn cap for the chiselled silhouette);
/// the three stones carry slightly different greys so the grey reads mottled rather
/// than stamped. A low dark plinth grounds it and a moss patch breaks the base.
pub fn build_trilithon_mesh() -> Mesh {
    const POST_W: f32 = 0.7; // upright width (X)
    const POST_D: f32 = 0.7; // upright depth (Z)
    const POST_H: f32 = 2.6; // upright bulk height
    const GAP: f32 = 2.0; // centre-to-centre spacing of the two uprights
    const HALF: f32 = GAP * 0.5; // ±1.0 in X
    const LINTEL_H: f32 = 0.45; // cross-beam thickness
    const PLINTH_H: f32 = 0.14; // low platform under both posts

    let mut parts: Vec<Mesh> = Vec::new();

    // Low turf/earth plinth the uprights stand on — base at y=0.
    parts.push(tinted(
        box_at(GAP + POST_W + 0.5, PLINTH_H, POST_D + 0.5, Vec3::new(0.0, PLINTH_H * 0.5, 0.0)),
        lin(STONE_CAP),
    ));

    // The two uprights. Left = STONE_A, right = STONE_C (distinct greys). Each is a
    // tall box bulk sitting on the plinth + a narrower, darker hewn cap on top, so
    // the silhouette reads tapered/chiselled. Posts rise from y=PLINTH_H.
    for (sx, body_hex) in [(-HALF, STONE_A), (HALF, STONE_C)] {
        let base_y = PLINTH_H;
        // Slight per-stone brightness jitter so even the two uprights differ subtly.
        let v = if sx < 0.0 { 0.97 } else { 1.03 };
        // Main bulk.
        parts.push(tinted(
            box_at(POST_W, POST_H, POST_D, Vec3::new(sx, base_y + POST_H * 0.5, 0.0)),
            lin_scaled(body_hex, v),
        ));
        // Hewn cap — narrower + darker, capping the upright just under the lintel.
        parts.push(tinted(
            box_at(
                POST_W * 0.82,
                0.18,
                POST_D * 0.82,
                Vec3::new(sx, base_y + POST_H - 0.02, 0.0),
            ),
            lin(STONE_CAP),
        ));
    }

    // Horizontal lintel spanning both uprights' tops. Width = gap + a post-width of
    // overhang on each side so it visibly bridges them; sits atop the posts.
    let lintel_w = GAP + POST_W + 0.3;
    let lintel_y = PLINTH_H + POST_H + LINTEL_H * 0.5 - 0.02;
    parts.push(tinted(
        box_at(lintel_w, LINTEL_H, POST_D, Vec3::new(0.0, lintel_y, 0.0)),
        lin(STONE_B),
    ));
    // A darker shadowed strip on the lintel's underside for grounding contrast.
    parts.push(tinted(
        box_at(lintel_w * 0.98, 0.06, POST_D * 0.7, Vec3::new(0.0, PLINTH_H + POST_H + 0.02, 0.0)),
        lin(STONE_CAP),
    ));

    // Moss creeping up the base of the left upright (thin slab on the +Z face).
    parts.push(tinted(
        box_at(
            POST_W * 0.6,
            0.7,
            0.04,
            Vec3::new(-HALF, PLINTH_H + 0.45, POST_D * 0.5 + 0.02),
        ),
        lin(STONE_MOSS),
    ));

    merged(parts)
}

// ── Giant dead tree ───────────────────────────────────────────────────────────

/// A tall bare gnarled dead tree, ~5u to the branch tips, base at y=0. A dark flared
/// root collar grounds a tapered trunk (three stacked, narrowing cylinder segments,
/// each leaning a touch for a slow gnarl), with a handful of thick bare branches
/// forking outward/upward high up — each sprouting a thinner twig — plus a few
/// shallow flared roots at the base. Weathered grey-brown (DEAD_WOOD) over a darker
/// lower trunk; near-black roots.
pub fn build_giant_dead_tree_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();

    // Dark flared root collar / stump the trunk grows out of (base at y=0). Tapered
    // approximated as a short fat cylinder; merge keeps it as the grounded base.
    parts.push(tinted(
        limb(0.46, 0.55, 8, 0.0, 0.0, Vec3::ZERO),
        lin(TREE_BARK_DARK),
    ));

    // Shallow flared roots splaying from the base, tipped nearly flat so their ends
    // dip toward the surface — five around the trunk.
    for i in 0..5 {
        let yaw = i as f32 * std::f32::consts::TAU / 5.0 + 0.3;
        let len = 0.55 + (i as f32 * 0.07);
        // pitch ~ +1.5 rad lays the cylinder out nearly horizontal (about Z), the yaw
        // spins it around the trunk; attach just above ground so it noses outward.
        parts.push(tinted(
            limb(0.13, len, 5, std::f32::consts::FRAC_PI_2 * 1.05, yaw, Vec3::new(0.0, 0.16, 0.0)),
            lin(TREE_ROOT),
        ));
    }

    // ── Trunk: three stacked tapered segments. Bevy cylinders take a single radius,
    // so each segment uses its average radius and steps down; each leans a touch more
    // than the last for a gentle gnarl. We accumulate the tip position + lean as we go.
    // Segment specs: (height, avg_radius, lean_z, lean_x_as_yaw_proxy).
    let seg_specs: [(f32, f32, f32, f32); 3] = [
        (1.5, 0.31, 0.05, 0.0),  // lower (darker)
        (1.4, 0.23, -0.08, 0.5), // mid
        (1.2, 0.15, 0.13, 0.3),  // upper (thinnest)
    ];
    let trunk_base_y = 0.4; // trunk noses up out of the collar
    let mut tip = Vec3::new(0.0, trunk_base_y, 0.0);
    let mut accum = Quat::IDENTITY;
    for (idx, &(h, r, lean_z, lean_yaw)) in seg_specs.iter().enumerate() {
        // Compose this segment's lean onto the accumulated trunk twist.
        accum = accum * Quat::from_rotation_y(lean_yaw) * Quat::from_rotation_z(lean_z);
        // Build a base-at-origin cylinder, lean it, place its base at the running tip.
        let seg = Cylinder::new(r, h)
            .mesh()
            .resolution(6)
            .build()
            .translated_by(Vec3::new(0.0, h * 0.5, 0.0))
            .rotated_by(accum)
            .translated_by(tip);
        let hex = if idx == 0 { TREE_BARK_DARK } else { TREE_BARK };
        parts.push(tinted(seg, lin(hex)));
        // Advance the tip up this segment's leaned axis for the next segment's base.
        tip += accum * Vec3::new(0.0, h, 0.0);
    }
    // `tip` is now the crown (~y ≈ 4.4); branch tips push the silhouette to ~5.

    // ── Branches: thick bare limbs forking outward/upward high on the trunk. Each is
    // a tapered (avg-radius) cylinder pitched out from vertical + yawed around the
    // trunk, sprouting a thinner twig at its tip for a forked bare silhouette.
    // (attach_y, yaw, pitch_z, len, radius)
    let branches: [(f32, f32, f32, f32, f32); 4] = [
        (2.4, 0.4, 0.85, 1.05, 0.115),
        (2.9, 2.5, 0.75, 0.95, 0.10),
        (3.3, 4.4, 0.95, 0.90, 0.09),
        (3.6, 1.4, 0.6, 0.75, 0.075),
    ];
    for &(ay, yaw, pitch, len, r) in branches.iter() {
        let attach = Vec3::new(0.0, ay, 0.0);
        // Branch limb (pitched out about Z, yawed about Y).
        parts.push(tinted(limb(r, len, 5, -pitch, yaw, attach), lin(TREE_BARK)));
        // Tip of the branch, in world-local space, to hang the twig off of.
        let branch_rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(-pitch);
        let tip_pt = attach + branch_rot * Vec3::new(0.0, len, 0.0);
        // Thinner forked twig continuing up/out from the branch tip.
        let twig_rot = branch_rot * Quat::from_rotation_z(0.7);
        let twig = Cylinder::new(r * 0.5, len * 0.5)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(Vec3::new(0.0, len * 0.25, 0.0))
            .rotated_by(twig_rot)
            .translated_by(tip_pt);
        parts.push(tinted(twig, lin(TREE_BARK)));
    }

    merged(parts)
}
