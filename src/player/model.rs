//! **Knight hero model** — box-mesh humanoid ported 1:1 from the TS `Character.tsx` mesh
//! tree (plate armour, visored helm, an iron sword baked into the right arm, a cross
//! shield on its own pivot). Built exactly like `orks.rs` / `critters.rs`: each articulated
//! part is ONE merged, flat-shaded, vertex-coloured `Mesh` against the shared white hero
//! material; feet rest at `y = 0` so the root, placed on the ground, plants the knight.
//!
//! Authoring is in TS units (the TS root group is `scale 0.5`, knight ~1.25u tall before
//! scale); the spawn applies `HERO_SCALE` so the knight stands the same height as the orks.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::creature::{surf, Surf};
use crate::palette::lin;

use super::HeroLimb;

// ── Palette (sRGB hex, matches Character.tsx) ────────────────────────────────────────
const ARMOR: u32 = 0xd6d8df;
const ARMOR_LIGHT: u32 = 0xe6e8ed;
const ARMOR_DARK: u32 = 0x9aa0aa;
const VISOR: u32 = 0x1a1a22;
const BELT: u32 = 0x3a2a1a;
const BLADE: u32 = 0xc0c6d0;
const HILT: u32 = 0x3a3a40;
const GRIP: u32 = 0x5a3a22;
const SHIELD_FACE: u32 = 0xa8b8d0;
const SHIELD_RIM: u32 = 0x6a3a22;
const SHIELD_EMBLEM: u32 = 0xd3b14c;
const GOLD: u32 = 0xe8b84b; // Golden Blade gilding
const AXE_STEEL: u32 = 0xaab0bc; // Battle Axe head
const STONE: u32 = 0x8a8d92; // Stone Maul head
const FROST: u32 = 0xaad2f0; // Frostfang greatsword (Bevy rim item, no TS mesh)
const PLUME: u32 = 0xa03028; // crimson helm plume
const CHAIN: u32 = 0x55585f; // chainmail skirt under the belt

/// Shield rest pose (own pivot, decoupled from the left arm): slung on the left flank,
/// decorated face out (−X). Block (M3) swings it across the front.
pub const SHIELD_REST_POS: Vec3 = Vec3::new(-0.3, 0.62, 0.06);
pub fn shield_rest_rot() -> Quat {
    Quat::from_euler(EulerRot::XYZ, 0.04, -1.3, 0.05)
}
/// Block pose: shield swung across the front (face +Z), braced high.
pub const SHIELD_BLOCK_POS: Vec3 = Vec3::new(-0.12, 0.82, 0.5);
pub fn shield_block_rot() -> Quat {
    Quat::from_euler(EulerRot::XYZ, -0.12, 0.05, -0.05)
}

// ── Articulated part + spec ──────────────────────────────────────────────────────────
pub struct HeroPartDef {
    pub limb: HeroLimb,
    pub pivot: Vec3,
    /// Rest orientation of the part (identity for limbs; the shield rests rotated).
    pub rest: Quat,
    pub mesh: Mesh,
}

pub struct KnightSpec {
    pub torso: Mesh,
    pub parts: Vec<HeroPartDef>,
    /// The held weapon, as its OWN mesh + sword-group-local transform. Spawned as a child of the
    /// `ArmR` entity (so it still swings with the arm) but kept separate so it can be *hidden* for
    /// weapon-free staged gestures (the Director's "hide weapon" toggle).
    pub weapon: Mesh,
    pub weapon_xf: Transform,
}

// ── Mesh helpers (local copies of the orks/critters contract) ────────────────────────
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
fn tinted(mut m: Mesh, c: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(c); n]);
    m
}
/// Merge + hard flat-shade — the crisp low-poly facets the TS models use.
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("hero parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), c)
}
fn cyl(r: f32, h: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(8).build().translated_by(off), c)
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), c)
}
fn sphere(r: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Sphere::new(r).mesh().ico(1).unwrap().translated_by(off), c)
}
// ── Equip-driven palette + geometry selectors ────────────────────────────────────────
/// Lerp an sRGB-hex colour `t` of the way toward `target` (in byte space — close enough for
/// the "feels the same" parity bar; the TS derives plate light/dark the same way).
fn lerp_hex(c: u32, target: u32, t: f32) -> u32 {
    let ch = |x: u32, s: u32| ((x >> s) & 0xff) as f32;
    let mix = |a: f32, b: f32| (a + (b - a) * t).round().clamp(0.0, 255.0) as u32;
    let r = mix(ch(c, 16), ch(target, 16));
    let g = mix(ch(c, 8), ch(target, 8));
    let b = mix(ch(c, 0), ch(target, 0));
    (r << 16) | (g << 8) | b
}

/// The plate colour triple `(base, light, dark)` for the worn armor — derived from the tier
/// tint (light = lerp→white 0.28, dark = lerp→black 0.3), matching `Character.tsx`. `None`
/// (bare) restores the exact default steel palette. Tints are the TS `armorTint` values.
fn armor_palette(armor: Option<&str>) -> (u32, u32, u32) {
    let tint = match armor {
        Some("leather_armor") => 0x7a5230,
        Some("iron_armor") => 0xaeb4c0,
        Some("gold_armor") => 0xe8b84b,
        Some("dragon_plate") => 0x3a6a4a,
        _ => return (ARMOR, ARMOR_LIGHT, ARMOR_DARK),
    };
    (tint, lerp_hex(tint, 0xffffff, 0.28), lerp_hex(tint, 0x000000, 0.3))
}

/// Sword-shaped weapon parts (pommel/grip/guard/blade/fuller/tip) with the blade tinted `c`;
/// shared by the iron sword (default), the golden blade, and the frost greatsword. The fuller
/// (the darker groove down the blade's spine) is derived from the blade colour.
fn sword_parts(blade: u32, pommel: u32) -> Vec<Mesh> {
    let fuller = lerp_hex(blade, 0x000000, 0.35);
    vec![
        sphere(0.05, v(0.0, 0.14, 0.0), pommel),
        cyl(0.03, 0.14, v(0.0, 0.06, 0.0), GRIP),
        bx(0.30, 0.06, 0.08, v(0.0, -0.04, 0.0), pommel),
        sphere(0.035, v(-0.15, -0.04, 0.0), pommel), // guard finials
        sphere(0.035, v(0.15, -0.04, 0.0), pommel),
        bx(0.09, 0.80, 0.03, v(0.0, -0.47, 0.0), blade),
        bx(0.03, 0.64, 0.034, v(0.0, -0.42, 0.0), fuller), // fuller groove
        cone(0.05, 0.12, v(0.0, -0.90, 0.0), rx(std::f32::consts::PI), blade),
    ]
}

/// The held-weapon part meshes for the equipped item, in the sword-group local space (the
/// caller bakes them at the hand). Ported 1:1 from `Character.tsx`; an unknown id falls back to
/// the iron sword (also the bare-handed default).
fn weapon_parts(weapon: Option<&str>) -> Vec<Mesh> {
    match weapon {
        Some("axe") => vec![
            cyl(0.028, 0.8, v(0.0, -0.12, 0.0), GRIP), // haft
            cyl(0.034, 0.05, v(0.0, -0.27, 0.0), HILT), // haft binding rings
            cyl(0.034, 0.05, v(0.0, 0.12, 0.0), HILT),
            sphere(0.04, v(0.0, 0.3, 0.0), HILT),      // pommel cap
            bx(0.26, 0.22, 0.05, v(0.13, -0.42, 0.0), AXE_STEEL), // head
            cone(0.11, 0.14, v(0.28, -0.42, 0.0), rz(-std::f32::consts::FRAC_PI_2), AXE_STEEL), // edge
            cone(0.05, 0.10, v(-0.04, -0.42, 0.0), rz(std::f32::consts::FRAC_PI_2), AXE_STEEL), // back spike
        ],
        Some("sword_gold") => sword_parts(GOLD, GOLD),
        Some("blade_frost") => sword_parts(FROST, FROST),
        Some("stone_maul") => vec![
            cyl(0.035, 0.95, v(0.0, -0.1, 0.0), GRIP), // haft
            cyl(0.042, 0.06, v(0.0, -0.32, 0.0), HILT), // haft binding ring
            sphere(0.045, v(0.0, 0.36, 0.0), HILT),    // pommel cap
            bx(0.34, 0.26, 0.26, v(0.0, -0.6, 0.0), STONE), // head
            bx(0.36, 0.04, 0.27, v(0.0, -0.52, 0.0), HILT), // iron bands
            bx(0.36, 0.04, 0.27, v(0.0, -0.68, 0.0), HILT),
            bx(0.06, 0.2, 0.2, v(0.19, -0.6, 0.0), STONE), // striking cap +x
            bx(0.06, 0.2, 0.2, v(-0.19, -0.6, 0.0), STONE), // striking cap -x
        ],
        _ => sword_parts(BLADE, HILT), // iron sword (default + bare-handed)
    }
}

// ── Build (params: equipped weapon + armor ids; None = bare iron sword + steel plate) ─
/// Build the knight mesh reflecting the equipped gear: the held weapon swaps geometry
/// (`weapon_parts`) and the worn armor recolours the plate (`armor_palette`). Re-called by
/// the render layer whenever the equip slots change (`reskin_hero`).
pub fn build_knight(weapon: Option<&str>, armor: Option<&str>) -> KnightSpec {
    // Plate colour triple for the worn armor (bare = default steel).
    let (a, al, ad) = armor_palette(armor);

    // Static torso: legs are articulated; belt + body + plate layers are baked in here. The
    // silhouette (belt/body/breastplate) is the TS one; the gorget, backplate, tassets, mail
    // skirt and gold trim are Bevy-side dressing that recolours with the worn tier.
    let torso = group(vec![
        bx(0.40, 0.10, 0.24, v(0.0, 0.33, 0.0), CHAIN), // mail skirt under the belt
        bx(0.42, 0.08, 0.22, v(0.0, 0.4, 0.0), BELT), // belt
        bx(0.10, 0.06, 0.012, v(0.0, 0.4, 0.115), GOLD), // buckle
        bxr(0.15, 0.14, 0.02, v(-0.17, 0.31, 0.11), xyz(0.15, 0.0, 0.12), ad), // tassets (hip plates)
        bxr(0.15, 0.14, 0.02, v(0.17, 0.31, 0.11), xyz(0.15, 0.0, -0.12), ad),
        bx(0.42, 0.46, 0.26, v(0.0, 0.66, 0.0), a), // body
        bx(0.32, 0.32, 0.02, v(0.0, 0.70, 0.135), al), // breastplate
        bx(0.05, 0.30, 0.016, v(0.0, 0.70, 0.142), a), // breastplate centre ridge
        bx(0.32, 0.035, 0.018, v(0.0, 0.875, 0.14), GOLD), // gilded collar trim
        bx(0.30, 0.34, 0.02, v(0.0, 0.68, -0.135), ad), // backplate
        bx(0.26, 0.10, 0.30, v(0.0, 0.92, 0.0), ad), // gorget (neck ring)
    ]);
    let torso = surf(torso, Surf::Metal); // plate sheen (mail/belt read metal-subtle; hue unchanged)

    // Head: helm + stepped crown + visor slit, breath holes, gilded brow band, cheek guards,
    // the dark crest brim and a crimson plume sweeping back over the crown.
    let head = group(vec![
        bx(0.32, 0.3, 0.32, v(0.0, 0.0, 0.0), al),
        bx(0.28, 0.10, 0.28, v(0.0, 0.18, 0.0), al), // stepped crown
        bx(0.24, 0.06, 0.01, v(0.0, -0.01, 0.165), VISOR),
        bx(0.02, 0.025, 0.01, v(-0.045, -0.085, 0.165), VISOR), // breath holes
        bx(0.02, 0.025, 0.01, v(0.045, -0.085, 0.165), VISOR),
        bx(0.30, 0.025, 0.014, v(0.0, 0.045, 0.166), GOLD), // gilded brow band
        bx(0.02, 0.13, 0.18, v(-0.17, -0.075, 0.05), ad), // cheek guards
        bx(0.02, 0.13, 0.18, v(0.17, -0.075, 0.05), ad),
        bx(0.34, 0.06, 0.34, v(0.0, 0.135, 0.0), ad), // crest brim
        bxr(0.05, 0.10, 0.26, v(0.0, 0.27, -0.02), rx(0.22), PLUME), // plume crest
        bxr(0.04, 0.08, 0.20, v(0.0, 0.24, -0.20), rx(0.75), PLUME), // plume tail
    ]);
    let head = surf(head, Surf::Metal); // helm plate (plume keeps its crimson hue)

    // Right arm (sword hand): the bare plate arm. The equipped weapon is built as its OWN mesh
    // ([`KnightSpec::weapon`]) and spawned as a child of the ArmR *entity* at the hand offset, so
    // it still swings with the arm but can be hidden for weapon-free gestures. Sword sits at
    // arm-local (0,-0.5,0.06) rotated x=-π/2 so it extends FORWARD (+Z).
    // Plate-arm dressing shared by both arms (`s` mirrors the pauldron tilt): a two-lame
    // pauldron, the upper arm, an elbow couter, the wrist cuff and a gauntleted fist.
    let plate_arm = |s: f32| {
        vec![
            bxr(0.20, 0.10, 0.30, v(0.0, -0.02, 0.0), rz(s * 0.10), al), // pauldron cap
            bxr(0.17, 0.07, 0.26, v(s * 0.02, -0.105, 0.0), rz(s * 0.20), a), // pauldron lame
            bx(0.12, 0.42, 0.22, v(0.0, -0.21, 0.0), a), // upper
            sphere(0.075, v(0.0, -0.30, 0.03), ad), // elbow couter
            bx(0.13, 0.08, 0.23, v(0.0, -0.45, 0.0), ad), // cuff
            bx(0.11, 0.09, 0.13, v(0.0, -0.51, 0.03), ad), // gauntlet fist
            bx(0.115, 0.025, 0.10, v(0.0, -0.485, 0.06), al), // knuckle plate
        ]
    };

    let sw_rot = rx(-std::f32::consts::FRAC_PI_2);
    let sw_off = v(0.0, -0.5, 0.06);
    let arm_r = surf(group(plate_arm(1.0)), Surf::Metal);
    // The weapon is its own merged mesh (grouped once → no re-merge corruption) placed at the hand.
    let weapon = surf(group(weapon_parts(weapon)), Surf::Metal);
    let weapon_xf = Transform { translation: sw_off, rotation: sw_rot, ..default() };

    // Left arm (shield hand): same plate dressing, mirrored (shield is a separate part).
    let arm_l = surf(group(plate_arm(-1.0)), Surf::Metal);

    // Shield (own pivot): heater plate + raised rim + recessed field + gold cross emblem,
    // with a rotated-square lower point (reads as the heater taper) and corner rivets.
    let shield = group(vec![
        bx(0.42, 0.58, 0.05, v(0.0, 0.0, 0.0), SHIELD_FACE), // plate
        bxr(0.30, 0.30, 0.05, v(0.0, -0.29, 0.0), rz(std::f32::consts::FRAC_PI_4), SHIELD_FACE), // lower point
        bxr(0.33, 0.33, 0.014, v(0.0, -0.29, 0.028), rz(std::f32::consts::FRAC_PI_4), SHIELD_RIM), // point rim
        bx(0.46, 0.62, 0.014, v(0.0, 0.0, 0.028), SHIELD_RIM), // rim
        bx(0.34, 0.5, 0.014, v(0.0, 0.0, 0.034), SHIELD_FACE), // inset field
        bx(0.07, 0.4, 0.014, v(0.0, 0.03, 0.04), SHIELD_EMBLEM), // cross vertical
        bx(0.3, 0.07, 0.014, v(0.0, 0.1, 0.04), SHIELD_EMBLEM), // cross horizontal
        bx(0.035, 0.035, 0.02, v(-0.17, 0.24, 0.03), SHIELD_EMBLEM), // corner rivets
        bx(0.035, 0.035, 0.02, v(0.17, 0.24, 0.03), SHIELD_EMBLEM),
        bx(0.035, 0.035, 0.02, v(-0.17, -0.18, 0.03), SHIELD_EMBLEM),
        bx(0.035, 0.035, 0.02, v(0.17, -0.18, 0.03), SHIELD_EMBLEM),
    ]);
    let shield = surf(shield, Surf::Metal); // heater plate sheen

    // Legs (built top-at-hip so the pivot sits at the hip; foot rests at root y≈0):
    // cuisse → poleyn (knee) → greave → sabaton, replacing the old single box.
    let leg = || {
        surf(
            group(vec![
                bx(0.17, 0.17, 0.19, v(0.0, -0.085, 0.0), a), // cuisse (thigh plate)
                bx(0.10, 0.06, 0.06, v(0.0, -0.175, 0.08), al), // poleyn (knee cap)
                bx(0.15, 0.17, 0.17, v(0.0, -0.255, 0.0), ad), // greave
                bx(0.15, 0.05, 0.24, v(0.0, -0.335, 0.04), al), // sabaton (foot)
            ]),
            Surf::Metal,
        )
    };

    let parts = vec![
        HeroPartDef { limb: HeroLimb::LegR, pivot: v(0.1, 0.36, 0.0), rest: Quat::IDENTITY, mesh: leg() },
        HeroPartDef { limb: HeroLimb::LegL, pivot: v(-0.1, 0.36, 0.0), rest: Quat::IDENTITY, mesh: leg() },
        HeroPartDef { limb: HeroLimb::ArmR, pivot: v(0.27, 0.87, 0.0), rest: Quat::IDENTITY, mesh: arm_r },
        HeroPartDef { limb: HeroLimb::ArmL, pivot: v(-0.27, 0.87, 0.0), rest: Quat::IDENTITY, mesh: arm_l },
        HeroPartDef { limb: HeroLimb::Head, pivot: v(0.0, 1.04, 0.0), rest: Quat::IDENTITY, mesh: head },
        HeroPartDef { limb: HeroLimb::Shield, pivot: SHIELD_REST_POS, rest: shield_rest_rot(), mesh: shield },
    ];

    KnightSpec { torso, parts, weapon, weapon_xf }
}
