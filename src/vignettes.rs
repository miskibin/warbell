//! **Biome vignettes** — one mute little story planted in each biome region: an abandoned camp
//! in the forest, a skeleton-and-strongbox lost caravan in the desert, a collapsed watchtower in
//! the rocks, a snowed-under expedition camp, a wreck rotting in the swamp. They reuse the whole
//! landmark pipeline (`landmarks::attach_custom`): each raises a will-o'-wisp beacon, is a one-time
//! discovery cache, doubles as a shrine, and — because pilgrims path to any `Landmark` — becomes a
//! wandering-NPC destination. The point is to give the player reasons to roam the island between
//! night sieges and stumble on a told-without-words scene.
//!
//! Each vignette is a single merged, vertex-coloured, flat-shaded mesh built from primitives —
//! the exact `ruins.rs` build contract — so it batches against the scene's one white material and
//! seats on flat ground with its base at y=0. Placement mirrors `ruins::populate_landmarks`
//! (reject-sample the flattest in-biome, on-land, unblocked spot), called from `worldmap::build`
//! right after the landmarks so it routes around the ruin already planted in the same region.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use tileworld_core::buff_store::BuffKind;

use crate::palette::lin;

// ── Mesh helpers (same verified 0.18 forms + contract as ruins.rs) ────────────────────

/// Tag every vertex with one flat linear colour (the scene's white material reads it off COLOR).
fn tint(mut m: Mesh, col: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(col); n]);
    m
}

/// Axis-aligned box centred at `c`.
fn bx(w: f32, h: f32, d: f32, c: Vec3, col: u32) -> Mesh {
    tint(Cuboid::new(w, h, d).mesh().build().translated_by(c), col)
}

/// Upright cylinder (8-faceted) centred at `c`.
fn cyl(r: f32, h: f32, c: Vec3, col: u32) -> Mesh {
    tint(Cylinder::new(r, h).mesh().resolution(8).build().translated_by(c), col)
}

/// Faceted sphere centred at `c`, squashed on Y by `squash` (1 = round, <1 = a flat mound).
fn ball(r: f32, c: Vec3, squash: f32, col: u32) -> Mesh {
    tint(
        Sphere::new(r).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.0, squash, 1.0)).translated_by(c),
        col,
    )
}

/// Merge tinted parts into one mesh, then un-index + flat-shade for crisp low-poly facets
/// (`duplicate_vertices` MUST precede `compute_flat_normals`).
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

// ── The five set-pieces (base at y=0) ─────────────────────────────────────────────────

/// Forest: a doused campfire (ash, ring-stones, crossed charred logs), a lean-to tent, a tipped crate.
fn abandoned_camp() -> Mesh {
    const ASH: u32 = 0x403e38;
    const CHAR: u32 = 0x231b13;
    const STONE: u32 = 0x8c8c84;
    const POLE: u32 = 0x6b5836;
    const CANVAS: u32 = 0xab9670;
    const CRATE: u32 = 0x7c5c34;
    let mut v = Vec::new();
    v.push(ball(0.32, Vec3::new(0.0, 0.03, 0.0), 0.35, ASH)); // cold ash bed
    for i in 0..6 {
        let a = i as f32 * (TAU / 6.0);
        v.push(ball(0.11, Vec3::new(a.cos() * 0.4, 0.05, a.sin() * 0.4), 0.7, STONE)); // fire-ring stones
    }
    for a in [0.4_f32, 2.0] {
        v.push(tint(
            Cylinder::new(0.045, 0.66).mesh().resolution(6).build()
                .rotated_by(Quat::from_rotation_y(a) * Quat::from_rotation_z(FRAC_PI_2))
                .translated_by(Vec3::new(0.0, 0.09, 0.0)),
            CHAR,
        )); // charred logs crossing the ash
    }
    let tent = Vec3::new(0.9, 0.0, 0.0);
    // A-frame: each panel's TOP leans to centre (meeting at the ridge), bottom splays outward.
    v.push(tint(
        Cuboid::new(0.05, 0.95, 1.1).mesh().build().rotated_by(Quat::from_rotation_z(-0.62)).translated_by(tent + Vec3::new(-0.26, 0.42, 0.0)),
        CANVAS,
    ));
    v.push(tint(
        Cuboid::new(0.05, 0.95, 1.1).mesh().build().rotated_by(Quat::from_rotation_z(0.62)).translated_by(tent + Vec3::new(0.26, 0.42, 0.0)),
        CANVAS,
    ));
    v.push(tint(
        Cylinder::new(0.035, 1.15).mesh().resolution(6).build().rotated_by(Quat::from_rotation_x(FRAC_PI_2)).translated_by(tent + Vec3::new(0.0, 0.78, 0.0)),
        POLE,
    )); // ridge pole
    v.push(tint(
        Cuboid::new(0.34, 0.34, 0.34).mesh().build().rotated_by(Quat::from_rotation_y(0.5) * Quat::from_rotation_x(0.4)).translated_by(Vec3::new(-0.7, 0.16, 0.4)),
        CRATE,
    )); // tipped crate
    assemble(v)
}

/// Desert: a bleached skeleton (skull + ribs) and a cracked-open strongbox half-buried in a drift.
fn lost_caravan() -> Mesh {
    const SAND: u32 = 0xcdb389;
    const BONE: u32 = 0xe9e1d0;
    const BONE_D: u32 = 0xcfc6b0;
    const SOCK: u32 = 0x35312b;
    const WOOD: u32 = 0x6a4a2c;
    const IRON: u32 = 0x33333a;
    const BRASS: u32 = 0xb9892f;
    const SHAFT: u32 = 0x7a5a36;
    let mut v = Vec::new();
    v.push(ball(1.1, Vec3::new(0.0, 0.02, 0.0), 0.16, SAND)); // low sand drift
    let sk = Vec3::new(-0.55, 0.0, 0.2);
    v.push(ball(0.17, sk + Vec3::new(0.0, 0.12, 0.0), 0.9, BONE)); // cranium
    v.push(ball(0.10, sk + Vec3::new(0.0, 0.09, 0.18), 0.8, BONE_D)); // snout
    v.push(ball(0.035, sk + Vec3::new(0.06, 0.14, 0.12), 0.7, SOCK)); // eye sockets
    v.push(ball(0.035, sk + Vec3::new(-0.06, 0.14, 0.12), 0.7, SOCK));
    for i in 0..4 {
        let rz = -0.2 - i as f32 * 0.16;
        v.push(tint(
            Cylinder::new(0.02, 0.34).mesh().resolution(6).build().rotated_by(Quat::from_rotation_z(FRAC_PI_2)).translated_by(sk + Vec3::new(0.0, 0.05, rz)),
            BONE,
        )); // ribs laid flat
    }
    let ch = Vec3::new(0.5, 0.0, -0.1);
    v.push(bx(0.6, 0.36, 0.42, ch + Vec3::new(0.0, 0.14, 0.0), WOOD)); // chest body
    for sx in [-0.22_f32, 0.22] {
        v.push(bx(0.04, 0.4, 0.46, ch + Vec3::new(sx, 0.15, 0.0), IRON)); // iron bands
    }
    v.push(tint(
        Cuboid::new(0.6, 0.14, 0.42).mesh().build().rotated_by(Quat::from_rotation_x(-0.6)).translated_by(ch + Vec3::new(0.0, 0.4, -0.12)),
        WOOD,
    )); // lid flung open
    v.push(bx(0.07, 0.1, 0.04, ch + Vec3::new(0.0, 0.2, 0.23), BRASS)); // lock
    v.push(tint(
        Cylinder::new(0.02, 1.0).mesh().resolution(6).build().rotated_by(Quat::from_rotation_y(0.6) * Quat::from_rotation_z(FRAC_PI_2)).translated_by(Vec3::new(0.1, 0.04, 0.7)),
        SHAFT,
    )); // a broken spear in the sand
    assemble(v)
}

/// Rocky: a broken tower stump with a jagged crown, tumbled blocks, and a fallen roof beam.
fn fallen_tower() -> Mesh {
    const ST_A: u32 = 0x8f939b;
    const ST_B: u32 = 0x787c84;
    const ST_C: u32 = 0xa3a7ad;
    const CAP: u32 = 0x5f5d58;
    const BEAM: u32 = 0x55462f;
    let mut v = Vec::new();
    v.push(cyl(1.0, 0.18, Vec3::new(0.0, 0.09, 0.0), CAP)); // plinth ring
    v.push(cyl(0.62, 1.2, Vec3::new(0.0, 0.7, 0.0), ST_A)); // tower stump
    for i in 0..7 {
        let a = i as f32 * (TAU / 7.0);
        let h = 0.2 + (i % 3) as f32 * 0.22;
        v.push(bx(0.26, h, 0.26, Vec3::new(a.cos() * 0.5, 1.3 + h * 0.5, a.sin() * 0.5), if i % 2 == 0 { ST_B } else { ST_C }));
    } // jagged broken crown
    for (dx, dz, a, col) in [(1.0_f32, 0.3_f32, 0.4_f32, ST_B), (-0.9, 0.6, 1.1, ST_C), (0.4, -1.0, 2.2, ST_A), (-0.6, -0.8, 0.8, ST_B)] {
        v.push(tint(
            Cuboid::new(0.5, 0.34, 0.42).mesh().build().rotated_by(Quat::from_rotation_y(a) * Quat::from_rotation_x(0.2)).translated_by(Vec3::new(dx, 0.16, dz)),
            col,
        )); // tumbled blocks
    }
    v.push(tint(
        Cylinder::new(0.07, 1.6).mesh().resolution(6).build().rotated_by(Quat::from_rotation_y(0.5) * Quat::from_rotation_z(FRAC_PI_2)).translated_by(Vec3::new(0.5, 0.4, -0.4)),
        BEAM,
    )); // fallen beam
    assemble(v)
}

/// Snow: a half-buried, snow-capped A-frame tent, a planted flag-marker, and a small stone cairn.
fn frozen_camp() -> Mesh {
    const SNOW: u32 = 0xeef6fc;
    const CANVAS: u32 = 0x8c98a6;
    const POLE: u32 = 0x5a4a38;
    const FLAG: u32 = 0x9c3a30;
    const STONE: u32 = 0x8a8f98;
    let mut v = Vec::new();
    v.push(ball(1.0, Vec3::new(0.0, 0.03, 0.0), 0.2, SNOW)); // drift the camp sinks into
    v.push(tint(Cuboid::new(0.05, 0.8, 1.0).mesh().build().rotated_by(Quat::from_rotation_z(-0.6)).translated_by(Vec3::new(-0.22, 0.33, 0.0)), CANVAS)); // A-frame: tops meet at ridge
    v.push(tint(Cuboid::new(0.05, 0.8, 1.0).mesh().build().rotated_by(Quat::from_rotation_z(0.6)).translated_by(Vec3::new(0.22, 0.33, 0.0)), CANVAS));
    v.push(bx(0.34, 0.12, 1.05, Vec3::new(0.0, 0.6, 0.0), SNOW)); // snow heaped on the ridge
    let mk = Vec3::new(0.95, 0.0, 0.3);
    v.push(cyl(0.04, 1.3, mk + Vec3::new(0.0, 0.65, 0.0), POLE)); // marker pole
    v.push(bx(0.34, 0.22, 0.03, mk + Vec3::new(0.18, 1.05, 0.0), FLAG)); // faded flag
    let cn = Vec3::new(-0.8, 0.0, -0.3);
    for (i, (r, y)) in [(0.2_f32, 0.16_f32), (0.16, 0.42), (0.11, 0.6)].into_iter().enumerate() {
        v.push(ball(r, cn + Vec3::new(i as f32 * 0.02, y, 0.0), 0.85, STONE)); // stacked cairn
    }
    assemble(v)
}

/// Swamp: a listing, half-sunk boat hull with exposed ribs, a broken mast + tattered sail, moss.
fn sunken_wreck() -> Mesh {
    const BOG: u32 = 0x2f3a28;
    const HULL: u32 = 0x47402f;
    const RIB: u32 = 0x352f22;
    const MAST: u32 = 0x564a36;
    const SAIL: u32 = 0x7c7c68;
    const MOSS: u32 = 0x6b7c3d;
    let mut v = Vec::new();
    v.push(ball(1.2, Vec3::new(0.0, 0.02, 0.0), 0.12, BOG)); // bog stain
    let list = Quat::from_rotation_x(0.18) * Quat::from_rotation_z(0.22);
    v.push(tint(Cuboid::new(0.7, 0.5, 1.9).mesh().build().rotated_by(list).translated_by(Vec3::new(0.0, 0.22, 0.0)), HULL)); // hull
    v.push(tint(Cuboid::new(0.78, 0.16, 2.0).mesh().build().rotated_by(list).translated_by(Vec3::new(0.0, 0.46, 0.0)), HULL)); // gunwale lip
    for dz in [-0.4_f32, 0.3, 0.9] {
        v.push(tint(Cuboid::new(0.84, 0.05, 0.06).mesh().build().rotated_by(list).translated_by(Vec3::new(0.0, 0.42, dz)), RIB)); // exposed ribs
    }
    v.push(tint(Cylinder::new(0.06, 1.5).mesh().resolution(6).build().rotated_by(Quat::from_rotation_z(0.5)).translated_by(Vec3::new(0.3, 0.8, 0.1)), MAST)); // broken mast
    v.push(tint(Cuboid::new(0.04, 0.6, 0.5).mesh().build().rotated_by(Quat::from_rotation_z(0.5)).translated_by(Vec3::new(0.55, 0.9, 0.1)), SAIL)); // tattered sail
    v.push(ball(0.22, Vec3::new(-0.2, 0.4, -0.5), 0.5, MOSS));
    v.push(ball(0.16, Vec3::new(0.25, 0.35, 0.7), 0.5, MOSS));
    assemble(v)
}

// ── Placement ─────────────────────────────────────────────────────────────────────────

/// Terrain height spread (max−min world-Y) over a vignette's footprint: centre + an eight-point
/// ring out to `radius`. `None` if any sample runs off-land/over water, so a scene never plants
/// with part of its base over a cliff or the sea. 0 = dead flat. (Local copy of the ruins probe.)
fn footprint_spread(x: f32, z: f32, radius: f32) -> Option<f32> {
    let c = crate::worldmap::ground_at_world(x, z)?;
    let (mut lo, mut hi) = (c, c);
    for i in 0..8 {
        let a = i as f32 * (TAU / 8.0);
        let y = crate::worldmap::ground_at_world(x + a.cos() * radius, z + a.sin() * radius)?;
        lo = lo.min(y);
        hi = hi.max(y);
    }
    Some(hi - lo)
}

struct Spec {
    biome: crate::biome::Biome,
    mesh: fn() -> Mesh,
    scale: f32,
    block_r: f32,
    foot_r: f32,
    name: &'static str,
    lore: &'static str,
    buff: BuffKind,
    mag: f64,
    beacon: Color,
}

/// Plant one vignette per biome region — a flat, in-biome, unblocked spot (reject-sampled, best
/// flatness wins), registered as a blocker and attached as a discoverable landmark POI. Called
/// from `worldmap::build` after `ruins::populate_landmarks`, so it avoids the ruin in its region.
pub fn populate_vignettes(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    use crate::biome::{Biome, BiomeEntity};
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    let specs = [
        Spec { biome: Biome::Forest, mesh: abandoned_camp, scale: 1.0, block_r: 0.9, foot_r: 1.6, name: "The Abandoned Camp", lore: "Cold ashes, and a meal left half-eaten.", buff: BuffKind::Haste, mag: 1.3, beacon: Color::srgb(1.0, 0.62, 0.2) },
        Spec { biome: Biome::Desert, mesh: lost_caravan, scale: 1.0, block_r: 1.0, foot_r: 1.5, name: "The Lost Caravan", lore: "The sand keeps its dead — and their gold.", buff: BuffKind::Power, mag: 1.4, beacon: Color::srgb(0.95, 0.55, 1.0) },
        Spec { biome: Biome::Rocky, mesh: fallen_tower, scale: 1.0, block_r: 1.0, foot_r: 1.8, name: "The Fallen Watchtower", lore: "It guarded the pass once. The pass forgot.", buff: BuffKind::Resist, mag: 0.6, beacon: Color::srgb(1.0, 0.8, 0.35) },
        Spec { biome: Biome::Snow, mesh: frozen_camp, scale: 1.0, block_r: 0.9, foot_r: 1.5, name: "The Frozen Camp", lore: "The expedition that never came home.", buff: BuffKind::Resist, mag: 0.6, beacon: Color::srgb(0.3, 0.92, 1.0) },
        Spec { biome: Biome::Swamp, mesh: sunken_wreck, scale: 1.0, block_r: 1.0, foot_r: 1.7, name: "The Sunken Wreck", lore: "The bog swallowed her whole, crew and all.", buff: BuffKind::Haste, mag: 1.3, beacon: Color::srgb(0.4, 0.95, 0.82) },
    ];
    let mut rng: u32 = 0x51a9_e3b7;
    for s in specs {
        let handle = meshes.add((s.mesh)());
        let probe = s.foot_r * s.scale;
        let mut best: Option<(f32, f32, f32, f32, f32)> = None; // (spread, x, z, y, yaw)
        for _ in 0..4000 {
            let x = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GX + 6.0, crate::worldmap::GX - 6.0);
            let z = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GZ + 6.0, crate::worldmap::GZ - 6.0);
            if crate::worldmap::biome_at_world(x, z) != Some(s.biome)
                || crate::worldmap::ground_at_world(x, z).is_none()
                || crate::blockers::is_blocked(x, z)
                || crate::camps::in_clearing(x, z)
                || crate::castle::in_footprint(x, z)
                || crate::rival::near_fort(x, z)
            {
                continue;
            }
            let Some(spread) = footprint_spread(x, z, probe) else { continue };
            let y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
            let yaw = crate::wildlife::rng_range(&mut rng, 0.0, TAU);
            if best.map_or(true, |b| spread < b.0) {
                best = Some((spread, x, z, y, yaw));
            }
            if spread <= 0.01 {
                break;
            }
        }
        if let Some((_, x, z, y, yaw)) = best {
            let id = commands
                .spawn((
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(s.scale)),
                    BiomeEntity,
                ))
                .id();
            // Solid oriented box covering the wreck/camp footprint (a single circle let you clip
            // the wider edges); half-extent from the flatness-probe reach, floored at block_r.
            let hb = (s.foot_r * s.scale * 0.55).max(s.block_r);
            crate::blockers::add_obb(x, z, hb, hb, yaw);
            // Vignette set-pieces carry no sealed gear ("" → no Rune-Trial), only their shrine buff.
            crate::landmarks::attach_custom(commands, id, s.name, s.lore, s.buff, s.mag, s.beacon, "", Vec3::new(x, y, z), meshes, materials);
            info!("vignette {:?} \"{}\" at {:.1},{:.1},{:.1}", s.biome, s.name, x, y, z);
        } else {
            info!("vignette: no spot found for {:?}", s.biome);
        }
    }
}
