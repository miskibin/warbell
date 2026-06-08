//! Wildlife **models** — box-mesh quadrupeds ported from the TS animal views
//! (`src/world/Wolf.tsx`, `Deer.tsx`, …). Each species is a small entity hierarchy:
//! a static **torso** mesh plus a handful of articulated **parts** (4 legs, a head, a
//! tail) that `wildlife::animal_limbs` swings each frame — exactly the `wind.rs` `Sway`
//! trick, but on limbs instead of tree crowns.
//!
//! CONTRACT (mirrors `props.rs`): every part is ONE merged, flat-shaded, vertex-coloured
//! `Mesh` against the shared white creature material; feet rest at ~`y=0` so the root,
//! placed on the ground, plants the animal. Sub-mesh rotations (ear/antler/tusk tilts,
//! neck angle) are baked into geometry — a part's pivot rotation rests at identity and
//! the limb system overwrites it.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::palette::lin;

/// The ambient wildlife species (the TS monsters — scorpion/croc/golem — are out).
/// `Hash` so `audio::Voices` can key per-species sound sets off it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Species {
    Wolf,
    Deer,
    Boar,
    Rabbit,
    PolarBear,
    Elk,
    Goat,
    Camel,
    Dog,
    Cat,
}

/// What kind of articulated part this is + how it animates.
/// `Leg(sign)` swings about X with the gait, sign giving the diagonal-gait phase.
/// `Arm(sign)` is the biped equivalent (orks) — swings about X opposite the legs.
#[derive(Clone, Copy)]
pub enum PartKind {
    Leg(f32),
    Arm(f32),
    Head,
    Tail,
}

/// One articulated part: its animation kind, its pivot (local to the root) and its mesh
/// (built in part-local space, so the limb system rotates it about the pivot).
pub struct PartDef {
    pub kind: PartKind,
    pub pivot: Vec3,
    pub mesh: Mesh,
}

/// A built creature: the static torso mesh + its articulated parts.
pub struct CreatureSpec {
    pub torso: Mesh,
    pub parts: Vec<PartDef>,
}

// ─── Mesh helpers (local copies of the props.rs contract) ────────────────────────

fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn merged(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("creature parts share attributes");
    }
    base
}
/// Merge + hard flat-shade — the crisp low-poly facets the TS models use.
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut m = merged(parts);
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), lin(c))
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), lin(c))
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), lin(c))
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
fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// Four hip-pivot legs, diagonal-gait signs (lf,rb = +1 ; rf,lb = -1). `front_z`/`back_z`
/// are the leg z offsets; `mk` builds the leg mesh in part-local space (pivot at top).
fn legs(hip_y: f32, hx: f32, front_z: f32, back_z: f32, mk: &dyn Fn() -> Mesh) -> Vec<PartDef> {
    vec![
        PartDef { kind: PartKind::Leg(1.0), pivot: v(-hx, hip_y, front_z), mesh: mk() }, // lf
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(hx, hip_y, front_z), mesh: mk() }, // rf
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(-hx, hip_y, back_z), mesh: mk() }, // lb
        PartDef { kind: PartKind::Leg(1.0), pivot: v(hx, hip_y, back_z), mesh: mk() },  // rb
    ]
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────────

pub fn build(s: Species) -> CreatureSpec {
    match s {
        Species::Wolf => wolf(),
        Species::Deer => deer(),
        Species::Boar => boar(),
        Species::Rabbit => rabbit(),
        Species::PolarBear => polar_bear(),
        Species::Elk => elk(),
        Species::Goat => goat(),
        Species::Camel => camel(),
        Species::Dog => dog(),
        Species::Cat => cat(),
    }
}

// ─── Wolf ───────────────────────────────────────────────────────────────────────
fn wolf() -> CreatureSpec {
    const FUR: u32 = 0x6b6f78;
    const DARK: u32 = 0x494d55;
    const SNOUT: u32 = 0x3a3e44;
    const NOSE: u32 = 0x141414;
    const EYE: u32 = 0xd8c84a;
    let torso = group(vec![
        bx(0.42, 0.42, 1.0, v(0.0, 0.62, 0.0), FUR),
        bx(0.44, 0.4, 0.42, v(0.0, 0.7, 0.28), FUR),
    ]);
    let head = group(vec![
        bx(0.32, 0.3, 0.32, v(0.0, 0.0, 0.0), FUR),
        bx(0.16, 0.14, 0.22, v(0.0, -0.06, 0.22), SNOUT),
        bx(0.08, 0.06, 0.05, v(0.0, -0.04, 0.34), NOSE),
        cone(0.07, 0.18, v(-0.11, 0.22, -0.02), rz(-0.1), DARK),
        cone(0.07, 0.18, v(0.11, 0.22, -0.02), rz(0.1), DARK),
        bx(0.04, 0.04, 0.01, v(-0.09, 0.03, 0.15), EYE),
        bx(0.04, 0.04, 0.01, v(0.09, 0.03, 0.15), EYE),
    ]);
    let tail = group(vec![bxr(0.13, 0.13, 0.4, v(0.0, 0.04, -0.18), rx(0.7), DARK)]);
    let mut parts = legs(0.52, 0.16, 0.34, -0.34, &|| bx(0.12, 0.5, 0.13, v(0.0, -0.25, 0.0), DARK));
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 0.8, 0.56), mesh: head });
    parts.push(PartDef { kind: PartKind::Tail, pivot: v(0.0, 0.62, -0.5), mesh: tail });
    CreatureSpec { torso, parts }
}

// ─── Deer ───────────────────────────────────────────────────────────────────────
fn deer() -> CreatureSpec {
    const COAT: u32 = 0xa9794a;
    const DARK: u32 = 0x7a5630;
    const BELLY: u32 = 0xd8c2a0;
    const NOSE: u32 = 0x1a1410;
    const ANTLER: u32 = 0xcdbb90;
    let torso = group(vec![
        bx(0.34, 0.36, 0.85, v(0.0, 0.95, 0.0), COAT),
        bx(0.3, 0.1, 0.78, v(0.0, 0.8, 0.0), BELLY),
    ]);
    let head = group(vec![
        bxr(0.15, 0.45, 0.15, v(0.0, 0.18, 0.04), rx(-0.5), COAT),
        bx(0.18, 0.2, 0.34, v(0.0, 0.42, 0.2), COAT),
        bx(0.12, 0.12, 0.14, v(0.0, 0.38, 0.4), DARK),
        bx(0.07, 0.06, 0.04, v(0.0, 0.38, 0.48), NOSE),
        bx(0.03, 0.03, 0.01, v(-0.07, 0.5, 0.28), NOSE),
        bx(0.03, 0.03, 0.01, v(0.07, 0.5, 0.28), NOSE),
        cone(0.05, 0.16, v(-0.13, 0.52, 0.16), rz(-0.5), DARK),
        cone(0.05, 0.16, v(0.13, 0.52, 0.16), rz(0.5), DARK),
        cone(0.022, 0.22, v(-0.08, 0.62, 0.12), xyz(0.2, 0.0, -0.3), ANTLER),
        cone(0.022, 0.22, v(0.08, 0.62, 0.12), xyz(0.2, 0.0, 0.3), ANTLER),
    ]);
    let tail = group(vec![bx(0.08, 0.16, 0.08, v(0.0, -0.04, -0.04), COAT)]);
    let mut parts = legs(0.78, 0.13, 0.32, -0.32, &|| bx(0.08, 0.74, 0.08, v(0.0, -0.37, 0.0), DARK));
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 1.05, 0.4), mesh: head });
    parts.push(PartDef { kind: PartKind::Tail, pivot: v(0.0, 1.0, -0.42), mesh: tail });
    CreatureSpec { torso, parts }
}

// ─── Boar ───────────────────────────────────────────────────────────────────────
fn boar() -> CreatureSpec {
    const HIDE: u32 = 0x4a3a2e;
    const DARK: u32 = 0x33271f;
    const BRISTLE: u32 = 0x1f1814;
    const TUSK: u32 = 0xe8ddc0;
    const SNOUT: u32 = 0x5a463a;
    const NOSE: u32 = 0x15100c;
    let mut torso_parts = vec![
        bx(0.5, 0.46, 0.9, v(0.0, 0.55, 0.0), HIDE),
        bx(0.46, 0.34, 0.42, v(0.0, 0.74, 0.2), HIDE),
    ];
    for (i, z) in [0.22f32, 0.05, -0.12, -0.28].into_iter().enumerate() {
        torso_parts.push(cone(0.04, 0.16, v(0.0, 0.92 - i as f32 * 0.02, z), rx(-0.3), BRISTLE));
    }
    let torso = group(torso_parts);
    let head = group(vec![
        bx(0.38, 0.34, 0.36, v(0.0, 0.0, 0.0), HIDE),
        bx(0.2, 0.18, 0.22, v(0.0, -0.08, 0.24), SNOUT),
        bx(0.12, 0.1, 0.05, v(0.0, -0.06, 0.36), NOSE),
        cone(0.028, 0.16, v(-0.1, -0.12, 0.3), xyz(-0.5, 0.0, -0.2), TUSK),
        cone(0.028, 0.16, v(0.1, -0.12, 0.3), xyz(-0.5, 0.0, 0.2), TUSK),
        cone(0.06, 0.14, v(-0.16, 0.18, -0.02), rz(-0.4), DARK),
        cone(0.06, 0.14, v(0.16, 0.18, -0.02), rz(0.4), DARK),
        bx(0.04, 0.04, 0.01, v(-0.1, 0.04, 0.17), NOSE),
        bx(0.04, 0.04, 0.01, v(0.1, 0.04, 0.17), NOSE),
    ]);
    let mut parts = legs(0.36, 0.18, 0.3, -0.3, &|| bx(0.13, 0.34, 0.14, v(0.0, -0.17, 0.0), DARK));
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 0.6, 0.55), mesh: head });
    CreatureSpec { torso, parts }
}

// ─── Rabbit ───────────────────────────────────────────────────────────────────────
fn rabbit() -> CreatureSpec {
    const FUR: u32 = 0x9a8a78;
    const DARK: u32 = 0x6f6052;
    const EAR_IN: u32 = 0xcaa090;
    const NOSE: u32 = 0xc97a7a;
    const EYE: u32 = 0x15100c;
    const TAIL: u32 = 0xefe9e0;
    let torso = group(vec![
        bx(0.26, 0.26, 0.36, v(0.0, 0.2, -0.02), FUR),
        bx(0.1, 0.1, 0.1, v(0.0, 0.24, -0.22), TAIL), // cotton tail (static)
    ]);
    let head = group(vec![
        bx(0.22, 0.2, 0.2, v(0.0, 0.0, 0.0), FUR),
        bx(0.05, 0.04, 0.03, v(0.0, -0.02, 0.11), NOSE),
        bx(0.03, 0.03, 0.01, v(-0.07, 0.03, 0.09), EYE),
        bx(0.03, 0.03, 0.01, v(0.07, 0.03, 0.09), EYE),
        bx(0.05, 0.28, 0.03, v(-0.06, 0.23, -0.01), FUR),
        bx(0.025, 0.22, 0.008, v(-0.06, 0.23, 0.006), EAR_IN),
        bx(0.05, 0.28, 0.03, v(0.06, 0.23, -0.01), FUR),
        bx(0.025, 0.22, 0.008, v(0.06, 0.23, 0.006), EAR_IN),
    ]);
    // Front legs small, hind legs big — same hip height so it reads as a crouch.
    let parts = vec![
        PartDef { kind: PartKind::Leg(1.0), pivot: v(-0.07, 0.16, 0.12), mesh: bx(0.06, 0.14, 0.06, v(0.0, -0.07, 0.0), DARK) },
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(0.07, 0.16, 0.12), mesh: bx(0.06, 0.14, 0.06, v(0.0, -0.07, 0.0), DARK) },
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(-0.08, 0.16, -0.1), mesh: bx(0.1, 0.16, 0.2, v(0.0, -0.08, 0.02), DARK) },
        PartDef { kind: PartKind::Leg(1.0), pivot: v(0.08, 0.16, -0.1), mesh: bx(0.1, 0.16, 0.2, v(0.0, -0.08, 0.02), DARK) },
        PartDef { kind: PartKind::Head, pivot: v(0.0, 0.34, 0.18), mesh: head },
    ];
    CreatureSpec { torso, parts }
}

// ─── Polar bear ───────────────────────────────────────────────────────────────────
fn polar_bear() -> CreatureSpec {
    const BODY: u32 = 0xeef2f6;
    const SHADOW: u32 = 0xc4ccd6;
    const SNOUT: u32 = 0xb0b8c2;
    const NOSE: u32 = 0x141414;
    const EYE: u32 = 0x2a2a2a;
    let torso = group(vec![
        bx(0.62, 0.58, 1.3, v(0.0, 0.68, 0.0), BODY),
        bx(0.58, 0.46, 0.56, v(0.0, 0.82, 0.38), BODY),
    ]);
    let head = group(vec![
        bx(0.42, 0.4, 0.42, v(0.0, 0.0, 0.0), BODY),
        bx(0.22, 0.18, 0.24, v(0.0, -0.07, 0.26), SNOUT),
        bx(0.1, 0.07, 0.05, v(0.0, -0.05, 0.39), NOSE),
        cone(0.09, 0.14, v(-0.16, 0.24, -0.06), rz(-0.08), SHADOW),
        cone(0.09, 0.14, v(0.16, 0.24, -0.06), rz(0.08), SHADOW),
        bx(0.05, 0.05, 0.01, v(-0.12, 0.04, 0.21), EYE),
        bx(0.05, 0.05, 0.01, v(0.12, 0.04, 0.21), EYE),
    ]);
    let tail = group(vec![bx(0.12, 0.12, 0.18, v(0.0, 0.0, -0.08), SHADOW)]);
    let mut parts = legs(0.55, 0.22, 0.38, -0.38, &|| bx(0.18, 0.55, 0.2, v(0.0, -0.275, 0.0), SHADOW));
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 0.92, 0.72), mesh: head });
    parts.push(PartDef { kind: PartKind::Tail, pivot: v(0.0, 0.68, -0.66), mesh: tail });
    CreatureSpec { torso, parts }
}

// ─── Elk ───────────────────────────────────────────────────────────────────────────
fn elk() -> CreatureSpec {
    const COAT: u32 = 0x7a5230;
    const DARK: u32 = 0x5a3a20;
    const UNDER: u32 = 0xb89a6a;
    const ANTLER: u32 = 0xcbb088;
    const HOOF: u32 = 0x2a2018;
    const EYE: u32 = 0x2a2a2a;
    let torso = group(vec![
        bx(0.36, 0.4, 1.0, v(0.0, 0.95, 0.0), COAT),
        bx(0.38, 0.44, 0.38, v(0.0, 1.0, 0.3), COAT),
        bx(0.28, 0.06, 0.85, v(0.0, 0.76, 0.0), UNDER),
        bxr(0.22, 0.4, 0.2, v(0.0, 1.2, 0.5), rx(-0.35), COAT), // straight neck (simplified)
    ]);
    let head = group(vec![
        bx(0.28, 0.26, 0.3, v(0.0, 0.0, 0.0), COAT),
        bx(0.18, 0.16, 0.22, v(0.0, -0.06, 0.2), UNDER),
        bx(0.09, 0.07, 0.05, v(0.0, -0.05, 0.32), HOOF),
        bx(0.04, 0.04, 0.01, v(-0.1, 0.04, 0.14), EYE),
        bx(0.04, 0.04, 0.01, v(0.1, 0.04, 0.14), EYE),
        bxr(0.06, 0.14, 0.05, v(-0.16, 0.14, -0.04), rz(-0.3), DARK),
        bxr(0.06, 0.14, 0.05, v(0.16, 0.14, -0.04), rz(0.3), DARK),
        // Left antler beam + tines.
        bxr(0.05, 0.3, 0.05, v(-0.1, 0.27, -0.06), rz(0.25), ANTLER),
        bxr(0.04, 0.22, 0.04, v(-0.14, 0.38, 0.01), xyz(-0.5, 0.0, 0.2), ANTLER),
        bxr(0.04, 0.2, 0.04, v(-0.18, 0.49, -0.1), xyz(0.2, 0.0, 0.35), ANTLER),
        bxr(0.04, 0.18, 0.04, v(-0.22, 0.59, -0.06), rz(0.4), ANTLER),
        // Right antler beam + tines (mirrored).
        bxr(0.05, 0.3, 0.05, v(0.1, 0.27, -0.06), rz(-0.25), ANTLER),
        bxr(0.04, 0.22, 0.04, v(0.14, 0.38, 0.01), xyz(-0.5, 0.0, -0.2), ANTLER),
        bxr(0.04, 0.2, 0.04, v(0.18, 0.49, -0.1), xyz(0.2, 0.0, -0.35), ANTLER),
        bxr(0.04, 0.18, 0.04, v(0.22, 0.59, -0.06), rz(-0.4), ANTLER),
    ]);
    let tail = group(vec![bxr(0.1, 0.18, 0.06, v(0.0, 0.1, 0.0), rx(-0.3), UNDER)]);
    let leg = |back: bool| {
        let (lw, ld) = if back { (0.13, 0.14) } else { (0.12, 0.13) };
        group(vec![
            bx(lw, 0.7, ld, v(0.0, -0.35, 0.0), DARK),
            bx(lw - 0.02, 0.06, ld - 0.02, v(0.0, -0.73, 0.0), HOOF),
        ])
    };
    let parts = vec![
        PartDef { kind: PartKind::Leg(1.0), pivot: v(-0.14, 0.72, 0.32), mesh: leg(false) },
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(0.14, 0.72, 0.32), mesh: leg(false) },
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(-0.14, 0.72, -0.32), mesh: leg(true) },
        PartDef { kind: PartKind::Leg(1.0), pivot: v(0.14, 0.72, -0.32), mesh: leg(true) },
        PartDef { kind: PartKind::Head, pivot: v(0.0, 1.5, 0.66), mesh: head },
        PartDef { kind: PartKind::Tail, pivot: v(0.0, 0.95, -0.5), mesh: tail },
    ];
    CreatureSpec { torso, parts }
}

// ─── Goat ───────────────────────────────────────────────────────────────────────────
fn goat() -> CreatureSpec {
    const WOOL: u32 = 0xd8d2c4;
    const DARK: u32 = 0xb0a894;
    const HORN: u32 = 0x8a7a5a;
    const HOOF: u32 = 0x2a2018;
    const EYE: u32 = 0x2a2a2a;
    let torso = group(vec![bx(0.34, 0.36, 0.7, v(0.0, 0.53, 0.0), WOOL)]);
    let head = group(vec![
        bx(0.22, 0.22, 0.26, v(0.0, 0.0, 0.0), WOOL),
        bx(0.14, 0.12, 0.18, v(0.0, -0.04, 0.18), DARK),
        bx(0.08, 0.1, 0.08, v(0.0, -0.16, 0.1), DARK), // beard
        cone(0.04, 0.18, v(-0.07, 0.14, -0.04), xyz(-0.5, 0.0, -0.15), HORN),
        cone(0.04, 0.18, v(0.07, 0.14, -0.04), xyz(-0.5, 0.0, 0.15), HORN),
        bx(0.04, 0.04, 0.02, v(-0.09, 0.02, 0.12), EYE),
        bx(0.04, 0.04, 0.02, v(0.09, 0.02, 0.12), EYE),
    ]);
    let tail = group(vec![bxr(0.08, 0.12, 0.06, v(0.0, 0.06, 0.0), rx(0.3), WOOL)]);
    let leg = || {
        group(vec![
            bx(0.1, 0.34, 0.1, v(0.0, -0.17, 0.0), DARK),
            bx(0.1, 0.04, 0.12, v(0.0, -0.32, 0.0), HOOF),
        ])
    };
    let mut parts = legs(0.35, 0.13, 0.196, -0.196, &leg);
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 0.63, 0.4), mesh: head });
    parts.push(PartDef { kind: PartKind::Tail, pivot: v(0.0, 0.59, -0.35), mesh: tail });
    CreatureSpec { torso, parts }
}

// ─── Camel ────────────────────────────────────────────────────────────────────────
// Tall dromedary: deep barrel torso with the signature single hump, a long near-vertical
// neck (baked into the torso so the head pivots at its tip), slim legs. Desert species.
fn camel() -> CreatureSpec {
    const COAT: u32 = 0xc8a064;
    const DARK: u32 = 0x9c7440;
    const UNDER: u32 = 0xd8c090;
    const HOOF: u32 = 0x3a2c1c;
    const NOSE: u32 = 0x2a2018;
    const EYE: u32 = 0x2a2a2a;
    let torso = group(vec![
        bx(0.46, 0.5, 1.15, v(0.0, 1.25, 0.0), COAT),
        bx(0.42, 0.12, 1.0, v(0.0, 1.04, 0.0), UNDER), // belly
        bx(0.44, 0.4, 0.55, v(0.0, 1.62, 0.0), DARK),  // hump
        bxr(0.22, 1.0, 0.22, v(0.0, 1.8, 0.5), rx(0.25), COAT), // neck (static; head pivots at its top)
    ]);
    let head = group(vec![
        bx(0.24, 0.24, 0.3, v(0.0, 0.0, 0.0), COAT),
        bx(0.16, 0.16, 0.22, v(0.0, -0.05, 0.2), UNDER), // muzzle
        bx(0.08, 0.07, 0.05, v(0.0, -0.04, 0.32), NOSE),
        cone(0.05, 0.12, v(-0.1, 0.16, -0.04), rz(-0.3), DARK), // ears
        cone(0.05, 0.12, v(0.1, 0.16, -0.04), rz(0.3), DARK),
        bx(0.04, 0.04, 0.01, v(-0.1, 0.04, 0.14), EYE),
        bx(0.04, 0.04, 0.01, v(0.1, 0.04, 0.14), EYE),
    ]);
    let tail = group(vec![bxr(0.07, 0.4, 0.07, v(0.0, -0.14, -0.02), rx(0.4), DARK)]);
    let leg = || {
        group(vec![
            bx(0.12, 1.0, 0.13, v(0.0, -0.5, 0.0), DARK),
            bx(0.14, 0.06, 0.16, v(0.0, -0.98, 0.02), HOOF),
        ])
    };
    let mut parts = legs(1.0, 0.18, 0.42, -0.42, &leg);
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 2.25, 0.66), mesh: head });
    parts.push(PartDef { kind: PartKind::Tail, pivot: v(0.0, 1.2, -0.6), mesh: tail });
    CreatureSpec { torso, parts }
}

// ─── Dog ──────────────────────────────────────────────────────────────────────────
// Friendly tan mutt: stocky little body, floppy ears, an up-curled wagging tail.
fn dog() -> CreatureSpec {
    const COAT: u32 = 0xb5793f;
    const DARK: u32 = 0x6e4a26;
    const SNOUT: u32 = 0x8a5a30;
    const NOSE: u32 = 0x141414;
    const EAR: u32 = 0x5a3a1f;
    const EYE: u32 = 0x1a1a1a;
    let torso = group(vec![
        bx(0.34, 0.34, 0.8, v(0.0, 0.5, 0.0), COAT),
        bx(0.36, 0.32, 0.34, v(0.0, 0.56, 0.22), COAT),
    ]);
    let head = group(vec![
        bx(0.28, 0.26, 0.26, v(0.0, 0.0, 0.0), COAT),
        bx(0.15, 0.13, 0.2, v(0.0, -0.05, 0.2), SNOUT),
        bx(0.07, 0.06, 0.05, v(0.0, -0.04, 0.31), NOSE),
        bxr(0.07, 0.18, 0.1, v(-0.15, 0.02, -0.02), rz(-0.6), EAR), // floppy ears
        bxr(0.07, 0.18, 0.1, v(0.15, 0.02, -0.02), rz(0.6), EAR),
        bx(0.04, 0.04, 0.01, v(-0.08, 0.04, 0.13), EYE),
        bx(0.04, 0.04, 0.01, v(0.08, 0.04, 0.13), EYE),
    ]);
    let tail = group(vec![bxr(0.1, 0.1, 0.32, v(0.0, 0.06, -0.14), rx(0.9), COAT)]);
    let mut parts = legs(0.42, 0.13, 0.26, -0.26, &|| bx(0.1, 0.42, 0.11, v(0.0, -0.21, 0.0), DARK));
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 0.62, 0.44), mesh: head });
    parts.push(PartDef { kind: PartKind::Tail, pivot: v(0.0, 0.52, -0.4), mesh: tail });
    CreatureSpec { torso, parts }
}

// ─── Cat ──────────────────────────────────────────────────────────────────────────
// Small sleek grey cat: slim body, triangular ears, a long upright tail that swishes.
fn cat() -> CreatureSpec {
    const FUR: u32 = 0x6f6f6f;
    const DARK: u32 = 0x4a4a4a;
    const SNOUT: u32 = 0x808080;
    const NOSE: u32 = 0xc97a7a;
    const EAR: u32 = 0x3a3a3a;
    const EYE: u32 = 0x9acd32;
    let torso = group(vec![
        bx(0.2, 0.2, 0.46, v(0.0, 0.32, 0.0), FUR),
        bx(0.2, 0.18, 0.2, v(0.0, 0.36, 0.16), FUR),
    ]);
    let head = group(vec![
        bx(0.2, 0.18, 0.18, v(0.0, 0.0, 0.0), FUR),
        bx(0.1, 0.08, 0.1, v(0.0, -0.04, 0.12), SNOUT),
        bx(0.04, 0.03, 0.03, v(0.0, -0.03, 0.18), NOSE),
        cone(0.06, 0.13, v(-0.08, 0.13, -0.02), rz(-0.12), EAR), // pointy ears
        cone(0.06, 0.13, v(0.08, 0.13, -0.02), rz(0.12), EAR),
        bx(0.035, 0.04, 0.01, v(-0.06, 0.03, 0.09), EYE),
        bx(0.035, 0.04, 0.01, v(0.06, 0.03, 0.09), EYE),
    ]);
    let tail = group(vec![bxr(0.06, 0.46, 0.06, v(0.0, 0.16, -0.04), rx(-0.4), FUR)]); // upright, swishes
    let mut parts = legs(0.3, 0.08, 0.16, -0.16, &|| bx(0.06, 0.3, 0.06, v(0.0, -0.15, 0.0), DARK));
    parts.push(PartDef { kind: PartKind::Head, pivot: v(0.0, 0.44, 0.26), mesh: head });
    parts.push(PartDef { kind: PartKind::Tail, pivot: v(0.0, 0.34, -0.22), mesh: tail });
    CreatureSpec { torso, parts }
}
