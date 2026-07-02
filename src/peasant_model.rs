//! **Peasant models on the shared biped skeleton** — a faithful port of the studio
//! `peasantBuilder.ts` (woodcutter / farmer / unemployed / miner, + a guard variant) authored for
//! [`crate::biped`]'s studio knight skeleton. Each [`PeasantKind`] yields a [`crate::biped::BipedMeshes`]
//! (per-joint flat-shaded vertex-coloured meshes) for `spawn_biped`. `skin`/`tunic`/`trouser` are
//! passed in so the town keeps its per-villager colour variety; the rest are the studio's fixed hues.
//!
//! Each peasant carries ONLY the tool of his trade on the **right hand's `Sword` pivot** (built +Y
//! like the hero/ork weapons so the shared rest rotation holds them carried): woodcutter → axe,
//! farmer → hoe, miner → pickaxe, guard → sword; the unemployed go empty-handed. The off-hand
//! (`Shield` pivot) is empty for civilians; the GUARD mounts a round livery shield there.
//!
//! Every part carries a [`Surf`] code (baked into the vertex-colour alpha by [`crate::creature::surf`])
//! so the shared `CreatureMaterial` shader textures cloth/metal/skin correctly — without it every part
//! decodes as flat `Skin`.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

use crate::biped::BipedMeshes;
use crate::creature::Surf;
use crate::palette::lin;

/// Which worker the studio peasant builder makes (maps from `villagers::Kind`/`Trade`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PeasantKind {
    Woodcutter,
    Farmer,
    Unemployed,
    Miner,
    Guard,
}

// Fixed studio peasant hues (skin/tunic/trouser are caller-supplied for town variety).
const LEATHER: u32 = 0x4e3b31; // gripColor
const WOOD: u32 = 0x6b4a2e; // wood brown (tool hafts — a real timber haft, not the old pale-grey trim)
const IRON: u32 = 0xcfd3dc; // bladeColor (tools/helmet)
const HAIR: u32 = 0x3a2418; // natural brown (studio reuses plumeColor, but that's our hero red)
const DARK: u32 = 0x23160f; // eyes/mouth
const STRAW: u32 = 0xd7b45f; // farmer hat
const PATCH: u32 = 0x9f7f4f; // stripes/apron/patches/torn hem
const LAMP: u32 = 0xffd166; // miner lamp
// Desert garb (rival NPCs): a keffiyeh headcloth + agal cord + a draped cloak/sash, so the rival's
// men read instantly as "not ours" no matter their trade.
const DESERT_CLOTH: u32 = 0xe7d9b6; // pale sand headcloth drape
const DESERT_BAND: u32 = 0x4a3a22; // dark agal cord / sash
const DESERT_CLOAK: u32 = 0xb1925a; // warm sand cloak
// Soldier livery — a guard wears a steel breastplate + a faction-coloured tabard so a FIGHTER reads
// at a glance from an unarmed peasant/worker (whom you must NOT cut down). Blue = the player's
// militia, crimson = the rival's garrison (picked by the `desert` flag, which is set only for rival
// NPCs). Matches the rival's crimson banners in `rival.rs`.
const LIVERY_BLUE: u32 = 0x2f4a8a;
const LIVERY_CRIMSON: u32 = 0x9a2420;
const PLATE: u32 = 0xb9bec8; // brushed steel armour (a touch darker than the bright IRON tool blade)
const DESERT_BRONZE: u32 = 0x8a6a34; // warm desert war-metal (conical war-helm, scimitar hilt)
// The rival garrison's body armour is PALE SANDY plate (sun-bleached leather/scale), NOT heavy
// cold steel like ours and NOT dark bronze — so the desert soldier reads "piaskowy" (sandy/light).
const DESERT_ARMOR: u32 = 0xcab089; // sandy plate
// Desert lower-body garb (rival workers) — a flowing thobe + loose linens + sandals, so the rival's
// men read as a foreign desert people from head to toe, not "our peasant in a headscarf".
const DESERT_ROBE: u32 = 0xcdb784; // sand thobe skirt (a shade lighter/warmer than the DESERT_CLOTH headwrap)
const DESERT_PANT: u32 = 0xb8a06b; // loose linen trousers under the robe
const DESERT_SANDAL: u32 = 0x6e5536; // bare-foot leather sandal

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
/// Tint a part `c` and tag its surface family `s` (alpha-encoded for the creature shader).
fn tinted(mut m: Mesh, c: u32, s: Surf) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(c); n]);
    crate::creature::surf(m, s)
}
/// Bevelled box matching the hero's softened low-poly edges (`player::model::chamfer_box`), so the
/// peasant's box details (head/buckles/hat) read as the same family as the knight, not sharp cuboids.
fn chamfer_box(w: f32, h: f32, d: f32, e: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (a, b, c) = (w * 0.5, h * 0.5, d * 0.5);
    let e = e.min(a * 0.49).min(b * 0.49).min(c * 0.49).max(0.001);
    let (ai, bi, ci) = (a - e, b - e, c - e);
    let pos: Vec<[f32; 3]> = vec![
        [a, -bi, -ci], [a, bi, -ci], [a, bi, ci], [a, -bi, ci],
        [-a, -bi, -ci], [-a, bi, -ci], [-a, bi, ci], [-a, -bi, ci],
        [-ai, b, -ci], [ai, b, -ci], [ai, b, ci], [-ai, b, ci],
        [-ai, -b, -ci], [ai, -b, -ci], [ai, -b, ci], [-ai, -b, ci],
        [-ai, -bi, c], [ai, -bi, c], [ai, bi, c], [-ai, bi, c],
        [-ai, -bi, -c], [ai, -bi, -c], [ai, bi, -c], [-ai, bi, -c],
    ];
    let mut raw: Vec<[u32; 3]> = Vec::new();
    let mut quad = |a: u32, b: u32, c: u32, d: u32| {
        raw.push([a, b, c]);
        raw.push([a, c, d]);
    };
    for f in 0..6u32 {
        let o = f * 4;
        quad(o, o + 1, o + 2, o + 3);
    }
    let edges = [
        [1, 2, 10, 9], [3, 0, 13, 14], [6, 5, 8, 11], [7, 4, 12, 15],
        [3, 2, 18, 17], [0, 1, 22, 21], [7, 6, 19, 16], [4, 5, 23, 20],
        [11, 10, 18, 19], [8, 9, 22, 23], [15, 14, 17, 16], [12, 13, 21, 20],
    ];
    for q in edges {
        quad(q[0], q[1], q[2], q[3]);
    }
    for t in [[2, 10, 18], [1, 9, 22], [3, 14, 17], [0, 13, 21], [6, 11, 19], [5, 8, 23], [7, 15, 16], [4, 12, 20]] {
        raw.push(t);
    }
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx: Vec<u32> = Vec::new();
    for t in raw {
        let (va, vb, vc) = (g(t[0]), g(t[1]), g(t[2]));
        let nrm = (vb - va).cross(vc - va);
        let ctr = (va + vb + vc) / 3.0;
        if nrm.dot(ctr) >= 0.0 {
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
fn cham(w: f32, h: f32, d: f32) -> f32 {
    (w.min(h).min(d) * 0.26).clamp(0.008, 0.04)
}
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("peasant parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32, s: Surf) -> Mesh {
    tinted(chamfer_box(w, h, d, cham(w, h, d)).translated_by(off), c, s)
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: u32, s: Surf) -> Mesh {
    tinted(chamfer_box(w, h, d, cham(w, h, d)).rotated_by(rot).translated_by(off), c, s)
}
fn frustum(rt: f32, rb: f32, h: f32, off: Vec3, c: u32, s: Surf) -> Mesh {
    tinted(ConicalFrustum { radius_top: rt, radius_bottom: rb, height: h }.mesh().resolution(6).build().translated_by(off), c, s)
}
fn frustum_s(rt: f32, rb: f32, h: f32, scale: Vec3, off: Vec3, c: u32, s: Surf) -> Mesh {
    tinted(ConicalFrustum { radius_top: rt, radius_bottom: rb, height: h }.mesh().resolution(6).build().scaled_by(scale).translated_by(off), c, s)
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, scale: Vec3, c: u32, s: Surf) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().scaled_by(scale).rotated_by(rot).translated_by(off), c, s)
}
/// Warm + deepen a skin hue toward a sun-tanned desert complexion (keep the red, drop green/blue),
/// preserving the caller's per-NPC variety while making the rival's people read as a foreign people.
fn tan_skin(c: u32) -> u32 {
    let r = (((c >> 16) & 0xff) as f32 * 0.93).min(255.0) as u32;
    let g = (((c >> 8) & 0xff) as f32 * 0.78).min(255.0) as u32;
    let b = ((c & 0xff) as f32 * 0.62).min(255.0) as u32;
    (r << 16) | (g << 8) | b
}

/// Build the per-joint peasant meshes for `kind` with the given body colours.
pub fn peasant_biped_meshes(kind: PeasantKind, skin: u32, tunic: u32, trouser: u32, kid: bool, desert: bool) -> BipedMeshes {
    use PeasantKind::*;
    // Surfaces: leather/wood-trim/straw/hair/patches read as matte Cloth; iron + the brass lamp as
    // Metal; skin/eyes/mouth as Skin. (WOOD here is a pale trim hue on tool hafts — flat Skin avoids
    // a cloth weave on bare wood.)
    let woodcutter = kind == Woodcutter;
    let farmer = kind == Farmer;
    let unemployed = kind == Unemployed;
    let miner = kind == Miner;
    let guard = kind == Guard;

    // Desert (rival) people are a foreign desert folk: sun-tanned skin and loose sand linens, not the
    // player town's pale skin + brown trews. (Workers also get the full thobe + sandals below.)
    let skin = if desert { tan_skin(skin) } else { skin };
    let trouser = if desert { DESERT_PANT } else { trouser };

    let mut hips_parts = vec![
        frustum(0.2, 0.17, 0.13, v(0.0, -0.06, 0.0), trouser, Surf::Cloth), // pelvis
        frustum(0.24, 0.22, 0.06, v(0.0, 0.03, 0.01), LEATHER, Surf::Cloth), // belt
        bx(0.055, 0.04, 0.025, v(0.0, 0.035, 0.225), IRON, Surf::Metal), // buckle
    ];
    if desert && !guard {
        // A flowing thobe skirt flaring from the waist down over the thighs — the dominant exotic
        // silhouette (the armoured guard wears his tabard instead, so robe is worker-only). Knee-length
        // so the loose linen shins + sandals still show below. Embroidered dark hem band at the edge.
        hips_parts.push(frustum(0.215, 0.33, 0.46, v(0.0, -0.22, 0.0), DESERT_ROBE, Surf::Cloth)); // robe skirt
        hips_parts.push(frustum(0.335, 0.34, 0.05, v(0.0, -0.43, 0.0), DESERT_BAND, Surf::Cloth)); // hem band
    }
    let hips = group(hips_parts);

    // A clean tunic: cone body + collar + hem + belt only. The old vest panel / diagonal shoulder
    // strap / per-trade patches were removed — they piled up into visual clutter on the chest and
    // read as "random junk". A worker's trade is now told by his TOOL + headgear (and the farmer's
    // apron / desert cloak / guard plate below), not by chest decals.
    let mut torso_parts = vec![
        frustum_s(0.25, 0.2, 0.44, v(1.05, 1.0, 0.8), v(0.0, 0.14, 0.02), tunic, Surf::Cloth), // tunic
        frustum_s(0.14, 0.17, 0.045, v(1.05, 1.0, 0.8), v(0.0, 0.34, 0.04), LEATHER, Surf::Cloth), // collar
        frustum_s(0.23, 0.22, 0.04, v(1.08, 1.0, 0.82), v(0.0, -0.08, 0.02), LEATHER, Surf::Cloth), // hem
    ];
    if farmer && !desert {
        // The apron is the farmer's clean trade mark (kept; the suspenders hold it up). Desert farmers
        // skip it — their thobe + turban already mark them, and a European apron would clash.
        torso_parts.push(bxr(0.2, 0.32, 0.035, v(0.0, 0.08, 0.18), rx(0.08), PATCH, Surf::Cloth)); // apron
        for side in [-1.0_f32, 1.0] {
            torso_parts.push(bxr(0.035, 0.42, 0.035, v(side * 0.08, 0.17, 0.195), rz(side * 0.12), LEATHER, Surf::Cloth)); // suspender
        }
    }
    if desert {
        // A cloak draped down the back — a clear desert silhouette over whatever clothes are under it.
        torso_parts.push(bxr(0.36, 0.52, 0.04, v(0.0, 0.04, -0.17), rx(-0.05), DESERT_CLOAK, Surf::Cloth)); // back cloak
        if !guard {
            // The diagonal sash is the WORKER's desert mark; a soldier wears the steel + tabard below
            // instead, so fighter vs labourer stays legible even under the shared keffiyeh/cloak.
            torso_parts.push(bxr(0.46, 0.075, 0.05, v(0.0, 0.12, 0.19), rz(0.5), DESERT_BAND, Surf::Cloth)); // chest sash
        }
    }
    if guard {
        // A FIGHTER reads at a glance: a steel breastplate + shoulder pauldrons, over a faction-coloured
        // tabard (blue = player militia, crimson = rival garrison). Unarmed peasants/workers never get
        // this, so the player can tell whom to cut down from whom to spare.
        let livery = if desert { LIVERY_CRIMSON } else { LIVERY_BLUE };
        // Ours is cold STEEL plate; the rival's is PALE SANDY lamellar (sun-bleached leather/scale) so
        // the desert soldier reads light and sandy, not a dark-bronze knight. Desert plate gets scale
        // row lines too. The surface family is Cloth for the sandy leather (matte, not steel-shiny).
        let armor = if desert { DESERT_ARMOR } else { PLATE };
        let armor_surf = if desert { Surf::Cloth } else { Surf::Metal };
        torso_parts.push(frustum_s(0.255, 0.215, 0.40, v(1.08, 1.0, 0.88), v(0.0, 0.16, 0.03), armor, armor_surf)); // breastplate
        torso_parts.push(bxr(0.21, 0.36, 0.03, v(0.0, 0.13, 0.2), rx(0.05), livery, Surf::Cloth)); // tabard front
        for side in [-1.0_f32, 1.0] {
            torso_parts.push(bxr(0.14, 0.08, 0.18, v(side * 0.2, 0.31, 0.02), rz(side * -0.2), armor, armor_surf)); // pauldron
        }
    }
    let torso = group(torso_parts);

    let neck = group(vec![frustum(0.08, 0.09, 0.08, v(0.0, -0.02, 0.0), skin, Surf::Skin)]);

    let mut head_parts = vec![
        bx(0.22, 0.24, 0.2, v(0.0, 0.05, 0.02), skin, Surf::Skin), // face
        bx(0.04, 0.055, 0.035, v(0.0, 0.05, 0.125), skin, Surf::Skin), // nose
        bx(0.07, 0.011, 0.012, v(0.0, -0.005, 0.132), DARK, Surf::Skin), // mouth
    ];
    for side in [-1.0_f32, 1.0] {
        head_parts.push(bx(0.03, 0.018, 0.012, v(side * 0.055, 0.09, 0.126), DARK, Surf::Skin)); // eye
        head_parts.push(bxr(0.042, 0.012, 0.012, v(side * 0.055, 0.12, 0.13), rz(side * if miner { -0.2 } else { 0.12 }), HAIR, Surf::Cloth)); // brow
        head_parts.push(bxr(0.035, 0.055, 0.035, v(side * 0.13, 0.055, 0.0), rz(side * 0.18), skin, Surf::Skin)); // ear
    }
    if (woodcutter || unemployed) && !kid {
        head_parts.push(bxr(0.14, 0.07, 0.028, v(0.0, -0.035, 0.12), rx(0.15), HAIR, Surf::Cloth)); // beard (never on kids — they read as children)
    }
    if desert && !guard {
        // A fat WRAPPED TURBAN — the rival WORKER's headgear (a soldier wears the bronze war-helm below,
        // so the armoured fighter still reads apart from the labourer under the shared desert palette).
        // A rounded dome + a banded wrap fold + a side tuck-knot read clearly as a turban, not a cap.
        head_parts.push(frustum(0.16, 0.135, 0.175, v(0.0, 0.16, 0.0), DESERT_CLOTH, Surf::Cloth)); // turban dome
        head_parts.push(frustum(0.168, 0.166, 0.05, v(0.0, 0.12, 0.0), DESERT_BAND, Surf::Cloth)); // wrap fold band
        head_parts.push(bxr(0.06, 0.07, 0.06, v(0.1, 0.255, -0.01), rz(-0.4), DESERT_CLOTH, Surf::Cloth)); // side tuck-knot
        head_parts.push(bxr(0.26, 0.26, 0.03, v(0.0, -0.02, -0.13), rx(-0.12), DESERT_CLOTH, Surf::Cloth)); // long neck drape
        for side in [-1.0_f32, 1.0] {
            head_parts.push(bxr(0.04, 0.22, 0.18, v(side * 0.128, -0.01, 0.0), Quat::IDENTITY, DESERT_CLOTH, Surf::Cloth)); // cheek flap
        }
    } else if farmer {
        head_parts.push(frustum_s(0.2, 0.22, 0.022, v(1.15, 1.0, 0.82), v(0.0, 0.17, 0.0), STRAW, Surf::Cloth)); // brim
        head_parts.push(frustum(0.1, 0.13, 0.09, v(0.0, 0.225, 0.0), STRAW, Surf::Cloth)); // crown
        head_parts.push(frustum(0.105, 0.132, 0.018, v(0.0, 0.195, 0.0), LEATHER, Surf::Cloth)); // hat band
        head_parts.push(bxr(0.018, 0.16, 0.012, v(0.13, 0.23, 0.02), xyz(0.2, 0.0, -0.7), STRAW, Surf::Cloth)); // sprig
    } else if miner {
        head_parts.push(frustum(0.13, 0.145, 0.07, v(0.0, 0.18, 0.0), IRON, Surf::Metal)); // helmet
        head_parts.push(bx(0.18, 0.02, 0.08, v(0.0, 0.16, 0.09), IRON, Surf::Metal)); // brim
        head_parts.push(bx(0.045, 0.04, 0.022, v(0.0, 0.18, 0.14), LAMP, Surf::Metal)); // lamp
        head_parts.push(bx(0.065, 0.055, 0.014, v(0.0, 0.18, 0.132), DARK, Surf::Skin)); // lamp frame
    } else if guard && desert {
        // Rival garrison: a clean SARACEN conical war-helm so the foreign soldier reads exotic at a
        // glance, distinct from the player militia's cold-steel helm. The earlier version stacked a
        // thin spike + finial knob + fat turban + face-veil into a cluttered tower over a faceless
        // head — pared back to ONE tapered bronze cone on a pale turban base band, face left visible.
        let bronze = DESERT_BRONZE;
        head_parts.push(frustum(0.14, 0.158, 0.095, v(0.0, 0.13, 0.0), DESERT_CLOTH, Surf::Cloth)); // turban base band
        head_parts.push(cone(0.135, 0.27, v(0.0, 0.305, 0.0), Quat::IDENTITY, Vec3::ONE, bronze, Surf::Metal)); // conical war-helm
        head_parts.push(bxr(0.22, 0.22, 0.03, v(0.0, -0.02, -0.12), rx(-0.12), DESERT_CLOTH, Surf::Cloth)); // light neck drape
    } else if guard {
        head_parts.push(frustum(0.125, 0.14, 0.11, v(0.0, 0.17, 0.0), PLATE, Surf::Metal)); // steel helm
        head_parts.push(bxr(0.04, 0.1, 0.16, v(0.0, 0.25, 0.0), Quat::IDENTITY, PLATE, Surf::Metal)); // helm crest ridge
        head_parts.push(bx(0.05, 0.11, 0.03, v(0.0, 0.07, 0.12), PLATE, Surf::Metal)); // nasal bar
    } else {
        head_parts.push(frustum(0.12, 0.14, 0.075, v(0.0, 0.18, 0.0), HAIR, Surf::Cloth)); // cap
        head_parts.push(bxr(0.17, 0.035, 0.14, v(0.02, 0.23, -0.015), xyz(-0.12, 0.0, if unemployed { 0.2 } else { -0.08 }), HAIR, Surf::Cloth)); // cap top
    }
    let head = group(head_parts);

    let shoulder = || {
        group(vec![
            frustum(0.078, 0.066, 0.28, v(0.0, -0.14, 0.0), tunic, Surf::Cloth), // sleeve
            frustum(0.072, 0.07, 0.045, v(0.0, -0.27, 0.0), LEATHER, Surf::Cloth), // cuff
        ])
    };
    let elbow = || {
        group(vec![
            frustum(0.058, 0.064, 0.29, v(0.0, -0.145, 0.0), skin, Surf::Skin), // forearm
            frustum(0.066, 0.07, 0.075, v(0.0, -0.22, 0.0), LEATHER, Surf::Cloth), // wrist wrap
            bx(0.07, 0.07, 0.08, v(0.0, -0.22, 0.0), skin, Surf::Skin), // fist
        ])
    };
    let hip = || {
        let mut p = vec![frustum(0.115, 0.097, 0.38, v(0.0, -0.2, 0.0), trouser, Surf::Cloth)]; // thigh
        if unemployed || farmer {
            p.push(bxr(0.075, 0.08, 0.025, v(0.0, -0.2, 0.08), rx(0.1), PATCH, Surf::Cloth)); // knee patch
        }
        group(p)
    };
    let knee = || {
        let mut p = vec![frustum(0.09, 0.095, 0.40, v(0.0, -0.21, 0.0), trouser, Surf::Cloth)]; // shin (runs down to the ankle)
        if desert {
            p.push(frustum(0.095, 0.1, 0.06, v(0.0, -0.36, 0.0), DESERT_BAND, Surf::Cloth)); // bare-leg ankle wrap (no boot)
        } else {
            p.push(frustum(0.092, 0.1, 0.16, v(0.0, -0.35, 0.0), LEATHER, Surf::Cloth)); // boot top wrapping the ankle
        }
        group(p)
    };
    let boot_s = if miner { v(1.05, 1.12, 1.1) } else { Vec3::ONE };
    // Boots for our folk; flat open sandals for the desert people (a thin sole + a toe strap), so even
    // the feet read foreign. A proper boot overlaps the shin (top ~0.195) so the lower leg reads as one
    // piece, not a foot floating below a gap.
    let foot = || {
        if desert {
            group(vec![
                tinted(Cuboid::new(0.125, 0.05, 0.24).mesh().build().translated_by(v(0.0, -0.05, 0.05)), DESERT_SANDAL, Surf::Cloth), // sole
                bx(0.12, 0.05, 0.07, v(0.0, -0.005, 0.04), DESERT_BAND, Surf::Cloth), // toe strap
            ])
        } else {
            group(vec![tinted(Cuboid::new(0.13, 0.14, 0.2).mesh().build().scaled_by(boot_s).translated_by(v(0.0, -0.025, 0.03)), LEATHER, Surf::Cloth)])
        }
    };

    // Tool on the right hand (built +Y in sword-local; the shared `Sword` rest rotation carries it).
    let weapon = if woodcutter {
        Some(group(vec![
            frustum(0.022, 0.026, 0.52, v(0.0, 0.26, 0.0), WOOD, Surf::Skin), // handle
            frustum(0.03, 0.034, 0.07, v(0.0, 0.48, 0.0), IRON, Surf::Metal), // collar
            bxr(0.17, 0.1, 0.04, v(0.07, 0.52, 0.0), rz(0.12), IRON, Surf::Metal), // head
            cone(0.065, 0.13, v(0.2, 0.52, 0.0), rz(-FRAC_PI_2), v(1.0, 0.85, 0.45), IRON, Surf::Metal), // edge
        ]))
    } else if miner {
        Some(group(vec![
            frustum(0.02, 0.024, 0.5, v(0.0, 0.25, 0.0), WOOD, Surf::Skin), // handle
            bx(0.06, 0.08, 0.04, v(0.0, 0.46, 0.0), IRON, Surf::Metal), // socket
            bx(0.34, 0.05, 0.04, v(0.0, 0.5, 0.0), IRON, Surf::Metal), // pick bar
            cone(0.035, 0.14, v(0.0, 0.56, 0.06), rx(FRAC_PI_2), Vec3::ONE, IRON, Surf::Metal), // point
            cone(0.03, 0.11, v(0.19, 0.5, 0.0), rz(-FRAC_PI_2), v(1.0, 0.8, 0.45), IRON, Surf::Metal), // chisel
        ]))
    } else if farmer {
        Some(group(vec![
            frustum(0.02, 0.024, 0.54, v(0.0, 0.27, 0.0), WOOD, Surf::Skin), // handle
            frustum(0.032, 0.036, 0.08, v(0.0, 0.5, 0.0), IRON, Surf::Metal), // socket
            bxr(0.22, 0.045, 0.035, v(0.12, 0.5, 0.02), rz(0.55), IRON, Surf::Metal), // hoe blade
        ]))
    } else if guard && desert {
        // The rival garrison carries a CURVED SCIMITAR (a saracen sabre), not our straight arming
        // sword — segmented along a shallow arc with the cutting edge swept toward +X, bronze hilt.
        let mut parts = vec![
            bx(0.16, 0.05, 0.05, v(0.0, 0.08, 0.0), DESERT_BRONZE, Surf::Metal), // crossguard
            frustum(0.02, 0.018, 0.14, v(0.0, -0.03, 0.0), LEATHER, Surf::Cloth), // grip
            bx(0.05, 0.05, 0.05, v(0.0, -0.11, 0.0), DESERT_BRONZE, Surf::Metal), // pommel
        ];
        // (width, height, offset, z-rotation) — precomputed arc segments (see commit note).
        for (w, h, off, a) in [
            (0.050_f32, 0.15_f32, v(0.007, 0.165, 0.0), -0.1125_f32),
            (0.055, 0.15, v(0.036, 0.291, 0.0), -0.3375),
            (0.060, 0.15, v(0.092, 0.407, 0.0), -0.5625),
            (0.055, 0.15, v(0.173, 0.508, 0.0), -0.7875),
        ] {
            parts.push(bxr(w, h, 0.022, off, rz(a), IRON, Surf::Metal)); // blade segment
        }
        parts.push(cone(0.03, 0.14, v(0.266, 0.591, 0.0), rz(-0.95), v(1.7, 1.0, 0.73), IRON, Surf::Metal)); // swept point
        Some(group(parts))
    } else if guard {
        // A REAL arming sword: a long flat blade tapering to a point above the crossguard — the old
        // "blade" was a degenerate height-0.04 cone (radius 0), so the guard held a bare hilt stub.
        Some(group(vec![
            bx(0.06, 0.46, 0.024, v(0.0, 0.31, 0.0), IRON, Surf::Metal), // blade
            cone(0.03, 0.12, v(0.0, 0.58, 0.0), Quat::IDENTITY, v(2.0, 1.0, 0.8), IRON, Surf::Metal), // point
            bx(0.18, 0.045, 0.05, v(0.0, 0.06, 0.0), IRON, Surf::Metal), // crossguard
            frustum(0.02, 0.018, 0.14, v(0.0, -0.03, 0.0), LEATHER, Surf::Cloth), // grip
            bx(0.05, 0.05, 0.05, v(0.0, -0.11, 0.0), IRON, Surf::Metal), // pommel
        ]))
    } else {
        None // unemployed — empty handed
    };

    // Left hand: WORKERS stay empty (a peasant carries only his trade tool — the old belt-pouch
    // read as random junk and is gone). GUARDS mount a round livery shield: soldier kit that reads
    // "militia" at siege distance (blue = ours, crimson = the rival's, same split as the tabard).
    // The biped animator carries the hero's EDGE-ON shield pose (tuned for the knight's heater), so
    // the disc bakes a +Y counter-rotation to still show its face — same trick as the ork buckler.
    let shield = if guard {
        let livery = if desert { LIVERY_CRIMSON } else { LIVERY_BLUE };
        Some(
            group(vec![
                frustum(0.24, 0.24, 0.04, v(0.0, 0.0, 0.0), WOOD, Surf::Cloth), // wooden disc
                frustum(0.25, 0.25, 0.016, v(0.0, -0.014, 0.0), LEATHER, Surf::Cloth), // rim backing
                frustum(0.17, 0.17, 0.048, v(0.0, 0.0, 0.0), livery, Surf::Cloth), // livery field
                cone(0.06, 0.08, v(0.0, 0.045, 0.0), Quat::IDENTITY, Vec3::ONE, PLATE, Surf::Metal), // boss
            ])
            .rotated_by(Quat::from_rotation_x(FRAC_PI_2)) // disc face → +Z (shield-local)
            .rotated_by(Quat::from_rotation_y(1.15)), // counter the hero pose's edge-on carry
        )
    } else {
        None
    };

    BipedMeshes {
        hips,
        torso,
        neck,
        head,
        shoulder_l: shoulder(),
        shoulder_r: shoulder(),
        elbow_l: elbow(),
        elbow_r: elbow(),
        hip_l: hip(),
        hip_r: hip(),
        knee_l: knee(),
        knee_r: knee(),
        foot_l: foot(),
        foot_r: foot(),
        weapon,
        shield,
        lion: None,
    }
}
