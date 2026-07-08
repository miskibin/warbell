//! Swamp / Bagno biome (key 5) — a murky wetland. Dark olive green-brown ground with a
//! high-variation wet mottle; dim, desaturated greenish daylight under DENSE green-grey
//! fog; a murky green river. The scatter is built from bespoke wetland props authored
//! right in this module (no shared `trees`/`props` reuse — the swamp wants its own
//! gnarled, drowned look). Light/shadow is BAKED into the vertex colours: every prop is
//! mud-dark at its waterline foot and brightens toward its sunlit crown:
//!
//!   * GNARLED MANGROVE swamp tree (tree class) — a crooked three-segment trunk (dark
//!     water-stained foot → pale sunlit top) standing on flared buttress root fins and
//!     stilt roots, five drooping bare limbs under a broad flat layered canopy (wide dark
//!     underside skirt, small bright lobes on top) dripping two-tone hanging-moss strands
//!     off the limbs AND the canopy rim. Three variants vary lean / fullness / phase.
//!   * CYPRESS-KNEE / rotten stump (tree class) — a fluted mossy bark drum on a dark mud
//!     skirt, a damp cut top sunk with a rotten hollow, rim splinters, shelf-fungus
//!     brackets, and a ring of knob-tipped cypress "knees" leaning back at the stump.
//!     Two variants (variant 1 is shorter and carries one tall snapped shard).
//!   * CATTAIL REED clump (first non-tree class → the tree spacing fallback) — stalks
//!     fanning out of a dark wet sheath, half topped with a brown seed head + the pale
//!     flowering spike above it, plus a few broad flattened leaf blades.
//!   * Swamp ACCENTS — toadstool clusters with baked gill shadows / sunlit cap bumps /
//!     pale flecks, a mud-skirted mossy boulder with stepped shelf fungus and shed
//!     pebbles, a taller toadstool trio, and a twisted HALF-SUNKEN LOG (broken-end
//!     shards, snag branches, moss saddle, one bracket).
//!
//! Ground cover: two-tone moss patches with bright sunlit tufts, reed sprigs around a
//! miniature cattail, double swamp mushrooms, lily-pad drifts (one variant carries a pale
//! pink lotus bud), bog cotton and lilac marsh flowers.
//! Particle: drifting low Mist. Backdrop: a dark misty conifer treeline over low murky
//! hills (no ocean — the land arc fills most of the horizon). Landmark: a big hollow dead
//! tree on the land side with a knot of glowing greenish will-o'-wisp motes hovering over
//! the muck beside it.
//!
//! CONTRACT (mirrors `biome_forest.rs` + the mesh modules): every prop is ONE merged,
//! vertex-coloured mesh, base at y=0, built from `tinted` primitive parts merged via
//! `Mesh::merge` then `flat_shaded` for crisp low-poly facets. The scatter draws them all
//! against the shared white vertex-colour material, so colour lives in `ATTRIBUTE_COLOR`.

// The `landmarks()` GLOWMUSH/will-o'-wisp set-piece + its helpers/consts below are authored biome
// content the world map doesn't place yet (it uses `ruins` landmarks instead). Kept per design;
// allow the resulting dead code until it's wired into a per-region pass.
#![allow(dead_code)]

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::{
    Backdrop, Biome, BiomeConfig, BiomeEntity, GroundDetail, ParticleKind, PropClass,
};
use crate::palette::{lin, lin_scaled};
use crate::meshkit::{flat_shaded, merged, tinted};

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;
const FRAC_PI_2: f32 = std::f32::consts::FRAC_PI_2;

/// Mangroves are authored ~1.7u tall; scale up so they loom at eye level like the forest.
const TREE_SCALE: f32 = 1.7;

// ── Swamp palette (murky, desaturated — deep olive greens + rotting browns) ──────────
// Most parts also pass through `lin_scaled(c, v)`: v < 1 for the mud-dark waterline /
// underside tones, v > 1 for the sunlit crowns, so each prop bakes its own shading.
// These trunk/canopy-dark tones were so close to black (≈0.02 linear) that the marsh + the
// blight around the ork Hold read as a field of pure-black silhouettes against the lit ground
// (player feedback: "za ciemno / wszystko zacienione"). Lifted ~50% so the wood reads as dark
// rotten brown rather than a hole — the shaded mood stays, the props stop crushing to black.
const MANGROVE_BARK: u32 = 0x5a4432; // twisted dark swamp-wood trunk (was 0x3b2c20 — near-black)
const MANGROVE_BARK_DK: u32 = 0x423022; // shadowed underside / drooping limbs (was 0x281d14)
const MANGROVE_ROOT: u32 = 0x4e3c28; // buttress fins + stilt roots, a touch warmer (was 0x322619)
const CANOPY_DARK: u32 = 0x3a603c; // deep swamp-green canopy body / underside (was 0x2a4a2c)
const CANOPY_MID: u32 = 0x37623a; // a slightly lighter canopy lobe
const CANOPY_LIGHT: u32 = 0x4d7d46; // bright sunlit top lobes
const HANGING_MOSS: u32 = 0x6f7e54; // grey-green Spanish-moss strands (pale sage)

const STUMP_BARK: u32 = 0x4a3826; // rotten cypress stump bark
const STUMP_TOP: u32 = 0x5f4a30; // damp cut-top wood (scaled way down = the rotten hollow)
const STUMP_MOSS: u32 = 0x4d6e3a; // moss cushion on the stump
const KNEE_WOOD: u32 = 0x5f462e; // cypress-knee root knob (lifted off near-black, was 0x42301f)

const REED_STALK: u32 = 0x6b8a44; // marsh reed green (TS REED_MAT)
const REED_STALK_DK: u32 = 0x4f6c34; // darker reed (TS REED_DARK_MAT)
const CATTAIL_HEAD: u32 = 0x7a4a2a; // brown cattail seed head (TS CATTAIL_MAT)

const TOAD_STEM: u32 = 0xccc2a0; // pale toadstool stem (also the cap flecks)
const TOAD_CAP: u32 = 0x7a5a34; // dull swamp-brown toadstool cap
const SHELF_FUNGUS: u32 = 0x9a7c4a; // ochre shelf fungus bracket
const SWAMP_ROCK: u32 = 0x57614e; // mossy grey-green boulder
const SWAMP_ROCK_MOSS: u32 = 0x47633a; // moss accent on the boulder

const LILY_PAD: u32 = 0x3a6a40; // murky lily-pad green
const LILY_PAD_EDGE: u32 = 0x294d2e; // darker pad rim
const LILY_BLOOM: u32 = 0xdcc0d8; // pale-pink closed lotus bud on a pad
const MOSS_PATCH: u32 = 0x4a6a38; // ground moss carpet patch
const BOG_COTTON: u32 = 0xeef0e6; // fluffy white bog-cotton head
const SWAMP_FLOWER: u32 = 0xcdb6e6; // pale lilac marsh flower
const SWAMP_FLOWER_CORE: u32 = 0xe8d27a; // pale gold flower centre / lotus heart

// Will-o'-wisp glow (sickly green; sRGB → linear in the emissive so it blooms).
const WISP_GLOW: Color = Color::srgb(0.45, 1.0, 0.55);
const WISP_EMISSIVE: f32 = 55.0;

// Glowing mushroom cluster — pale cool stems (on the shared white mat) under
// bioluminescent caps (their own emissive mat so they bloom, like the wisps).
const GLOWMUSH_STEM: u32 = 0xcfe8e0; // pale cool stem
const GLOWMUSH_GLOW: Color = Color::srgb(0.35, 1.0, 0.82); // teal-green cap glow
const GLOWMUSH_EMISSIVE: f32 = 26.0;
/// Local layout of one cluster: (dx, dz, scale) per mushroom. Shared by the stem + cap
/// builders so caps land exactly on their stems.
const GLOWMUSH_SPOTS: [(f32, f32, f32); 6] = [
    (0.0, 0.0, 1.3),
    (0.16, 0.06, 0.9),
    (-0.13, 0.10, 0.8),
    (0.05, -0.14, 0.7),
    (-0.08, -0.05, 0.55),
    (0.19, -0.11, 0.5),
];

// ── Mesh helpers (verbatim from trees.rs / decor.rs) ─────────────────────────────────

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// A faceted icosphere blob (ico detail 0), optionally squashed (or stretched, > 1),
/// centred at `off`, in an explicit linear colour (callers bake light via `lin_scaled`).
fn ball_at(r: f32, off: Vec3, squash: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        c,
    )
}

/// An upright cylinder whose centre sits at `cy` (a part of height `h` rooted at y=0 uses
/// `cy = h/2`). `res` ≥ 3 (the Cylinder builder asserts resolution > 2).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: [f32; 4]) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), c)
}

/// A slim cone leaned out from upright: built along +Y with its base on the ground,
/// tilted by `tilt` about Z, yawed by `yaw` about Y, then planted at `foot`. The base is
/// auto-lifted by `r·sin(tilt)` so the tipped rim never dips below `foot.y` (the mesh
/// contract: nothing under y=0). The workhorse for stalks, buttress fins, knees, snags
/// and shards. NOTE on aim: with foot at angle `a` from the axis, `yaw = -a` + positive
/// `tilt` leans the tip back INWARD (toward the axis) — used for buttresses/knees.
fn lean_cone(r: f32, h: f32, res: u32, tilt: f32, yaw: f32, foot: Vec3, c: [f32; 4]) -> Mesh {
    let lift = r * tilt.abs().sin() + 0.002;
    tinted(
        Cone { radius: r, height: h }
            .mesh()
            .resolution(res)
            .build()
            .translated_by(y(h * 0.5))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(foot + y(lift)),
        c,
    )
}

/// A shelf-fungus bracket jutting out at `at`, yawed to face its host: a squashed ochre
/// half-disc over a thin DARKER underside disc (baked under-shelf shadow). Returns both
/// parts pre-`tinted`; callers `extend` them into the part list.
fn shelf_bracket(at: Vec3, r: f32, yaw: f32) -> [Mesh; 2] {
    let disc = |rr: f32, th: f32, dy: f32, c: [f32; 4]| {
        tinted(
            Cylinder::new(rr, th)
                .mesh()
                .resolution(8)
                .build()
                .scaled_by(Vec3::new(1.0, 1.0, 0.55))
                .rotated_by(Quat::from_rotation_y(yaw))
                .translated_by(at + y(dy)),
            c,
        )
    };
    [
        disc(r, 0.028, 0.0, lin(SHELF_FUNGUS)),
        disc(r * 0.92, 0.016, -0.024, lin_scaled(SHELF_FUNGUS, 0.6)),
    ]
}

// ── Prop builders ────────────────────────────────────────────────────────────────────

/// **Gnarled mangrove swamp tree** — the tree class. A crooked three-segment trunk (dark
/// water-stained foot brightening to a pale sunlit top) standing on four flared buttress
/// root fins + three stilt roots, five drooping bare limbs, and a broad flat LAYERED
/// canopy: a wide dark underside skirt, a fuller mid layer, then small bright sunlit
/// lobes on top. Two-tone hanging-moss strands drip off the limb tips and the canopy rim,
/// each ending in a wispy blob. Authored ~1.7u tall, base flush at y=0. Three variants
/// vary lean, canopy fullness and the buttress/moss phase.
pub(crate) fn build_mangrove_mesh(variant: u32) -> Mesh {
    let lean = match variant {
        0 => 0.10_f32,
        1 => -0.14,
        _ => 0.06,
    };
    let mut parts: Vec<Mesh> = Vec::new();

    // ── Buttressed base — four flared root fins leaning back INTO the trunk (yaw = -a,
    // see `lean_cone`), all in dark wet-stained root tones (the baked waterline shadow).
    for i in 0..4 {
        let a = (i as f32 / 4.0) * TAU + 0.9 + variant as f32 * 0.5;
        let foot = Vec3::new(a.cos() * 0.17, 0.0, a.sin() * 0.17);
        parts.push(lean_cone(0.075, 0.36, 5, 0.50, -a, foot, lin_scaled(MANGROVE_ROOT, 0.85)));
    }
    // Three stilt-roots splaying out between the fins — the tree stands "on legs".
    for i in 0..3 {
        let a = (i as f32 / 3.0) * TAU + 0.4;
        let root = Cylinder::new(0.045, 0.32)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(0.16))
            // splay outward: lean then yaw around the base
            .rotated_by(Quat::from_rotation_z(0.66))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(Vec3::new(a.cos() * 0.11, 0.035, a.sin() * 0.11));
        parts.push(tinted(root, lin_scaled(MANGROVE_ROOT, 0.75)));
    }
    // A low knotty boss where the roots gather.
    parts.push(ball_at(0.16, y(0.15), 0.8, lin_scaled(MANGROVE_ROOT, 0.9)));

    // ── Trunk — three stacked crooked segments alternating lean for a gnarled silhouette,
    // dark mud-stained at the foot and brightening toward the sunlit top (baked vertical
    // light gradient). Knuckle bosses hide the joints so the kinks read as wood, not gaps.
    let segs = [
        (0.105_f32, 0.48_f32, lean, lin_scaled(MANGROVE_BARK, 0.80)),
        (0.085, 0.42, -lean * 1.5, lin(MANGROVE_BARK)),
        (0.063, 0.38, lean * 0.9, lin_scaled(MANGROVE_BARK, 1.14)),
    ];
    let mut anchor = y(0.10); // lifted so the leaned base clears y=0 (roots hide the gap)
    for (i, &(r, h, tilt, c)) in segs.iter().enumerate() {
        let q = Quat::from_rotation_z(tilt);
        let seg = Cylinder::new(r, h)
            .mesh()
            .resolution(6)
            .build()
            .translated_by(y(h * 0.5))
            .rotated_by(q)
            .translated_by(anchor);
        parts.push(tinted(seg, c));
        anchor += q * Vec3::new(0.0, h, 0.0);
        if i < 2 {
            parts.push(ball_at(r * 1.15, anchor, 0.8, lin(MANGROVE_BARK)));
        }
    }
    // Crown anchor point (top of the upper trunk) — limbs & canopy hang off it.
    let crown = anchor;

    // ── Five drooping bare limbs radiating from the crown (thin cylinders tilted past
    // horizontal so they sag). Record each tip so moss can hang off it.
    let mut limb_tips: Vec<Vec3> = Vec::new();
    let limbs = [
        (0.5_f32, 0.90_f32, 0.36_f32), // (yaw, droop, length)
        (1.7, 0.62, 0.30),
        (2.9, 1.00, 0.30),
        (4.2, 0.72, 0.34),
        (5.4, 0.55, 0.26),
    ];
    for (i, &(yaw, droop, len)) in limbs.iter().enumerate() {
        let tilt = Quat::from_rotation_z(FRAC_PI_2 - 0.2 + droop);
        let spin = Quat::from_rotation_y(yaw + variant as f32 * 0.7);
        let limb = Cylinder::new(0.024, len)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(tilt)
            .rotated_by(spin)
            .translated_by(crown);
        let c = if i % 2 == 0 { lin(MANGROVE_BARK) } else { lin(MANGROVE_BARK_DK) };
        parts.push(tinted(limb, c));
        // The far tip of the limb in world space.
        limb_tips.push(crown + spin * (tilt * Vec3::new(0.0, len, 0.0)));
    }

    // ── Broad flat layered canopy — a wide DARK underside skirt (ico-1 disc: the baked
    // canopy shadow), a fuller mid layer above it, then small bright sunlit lobes on top.
    let skirt = Sphere::new(0.36)
        .mesh()
        .ico(1)
        .expect("ico detail in range")
        .scaled_by(Vec3::new(1.0, 0.42, 1.0))
        .translated_by(crown + y(0.04));
    parts.push(tinted(skirt, lin_scaled(CANOPY_DARK, 0.72)));
    let mid = Sphere::new(0.30)
        .mesh()
        .ico(1)
        .expect("ico detail in range")
        .scaled_by(Vec3::new(1.0, 0.50, 1.0))
        .translated_by(crown + y(0.14));
    parts.push(tinted(mid, lin(CANOPY_DARK)));
    parts.push(ball_at(0.17, crown + Vec3::new(0.15, 0.22, -0.08), 0.6, lin(CANOPY_MID)));
    parts.push(ball_at(0.14, crown + Vec3::new(-0.13, 0.25, 0.09), 0.6, lin(CANOPY_LIGHT)));
    if variant != 1 {
        parts.push(ball_at(0.12, crown + Vec3::new(0.02, 0.28, 0.14), 0.6, lin(CANOPY_LIGHT)));
    }

    // ── Hanging moss — pale sage strands dangling straight down off the limb tips AND
    // the canopy rim, alternating two tones, each ending in a wispy blob.
    let mut hangs: Vec<(Vec3, f32)> = Vec::new();
    for (i, &tip) in limb_tips.iter().enumerate() {
        if i == 4 {
            continue; // leave one limb bare for an asymmetric look
        }
        hangs.push((tip, 0.22 + (i as f32) * 0.035));
    }
    for i in 0..3 {
        let a = (i as f32 / 3.0) * TAU + 1.1 + variant as f32;
        hangs.push((crown + Vec3::new(a.cos() * 0.30, 0.02, a.sin() * 0.30), 0.18 + i as f32 * 0.05));
    }
    for (i, &(at, hang)) in hangs.iter().enumerate() {
        let c = if i % 2 == 0 { lin(HANGING_MOSS) } else { lin_scaled(HANGING_MOSS, 0.78) };
        // A thin tapered cone pointing DOWN (rotate the upright cone PI about X).
        let strand = Cone { radius: 0.018, height: hang }
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(hang * 0.5))
            .rotated_by(Quat::from_rotation_x(PI))
            .translated_by(at);
        parts.push(tinted(strand, c));
        parts.push(ball_at(0.045, at - y(hang), 0.55, c));
    }

    flat_shaded(merged(parts))
}

/// **Cypress-knee stump** — the second tree-class prop: a fluted mossy rotten bark drum
/// on a dark mud skirt, a damp cut top sunk with a darker rotten hollow, jagged rim
/// splinters, shelf-fungus brackets on the shaded side, and a ring of cypress "knees"
/// (knob-tipped root cones leaning back at the stump out of the muck). Base at y=0,
/// ~0.4u tall. Variant 1 is a shorter drum carrying one tall snapped shard.
pub(crate) fn build_cypress_stump_mesh(variant: u32) -> Mesh {
    let r = 0.32;
    let h = if variant == 0 { 0.36 } else { 0.30 };
    let mut parts = vec![
        // Dark muddy waterline skirt, then the bark drum above it (skirt centre/squash
        // chosen so the blob bottoms out AT y=0, not under it).
        ball_at(r * 1.18, y(0.115), 0.30, lin_scaled(STUMP_BARK, 0.68)),
        cyl_up(r, h, h * 0.5, 9, lin(STUMP_BARK)),
        // Damp cut top + the darker rotten hollow sunk in its middle.
        cyl_up(r * 0.94, 0.05, h + 0.01, 9, lin_scaled(STUMP_TOP, 1.1)),
        cyl_up(r * 0.52, 0.022, h + 0.045, 8, lin_scaled(STUMP_TOP, 0.42)),
        // Moss cushions slumped over the rim — brighter where the light lands on top.
        ball_at(r * 0.62, Vec3::new(r * 0.5, h - 0.02, 0.0), 0.5, lin(STUMP_MOSS)),
        ball_at(r * 0.38, Vec3::new(-r * 0.42, h + 0.02, r * 0.32), 0.55, lin_scaled(STUMP_MOSS, 1.18)),
        ball_at(r * 0.30, Vec3::new(-r * 0.1, h + 0.03, -r * 0.45), 0.5, lin_scaled(STUMP_MOSS, 1.05)),
    ];
    // Root flutes — slim cones hugging the drum so the bark reads ridged, not tubular.
    for i in 0..4 {
        let a = (i as f32 / 4.0) * TAU + 0.7;
        let foot = Vec3::new(a.cos() * (r + 0.025), 0.0, a.sin() * (r + 0.025));
        parts.push(lean_cone(0.055, h * 0.85, 4, 0.16, -a, foot, lin_scaled(STUMP_BARK, 0.88)));
    }
    // Jagged splinters on the rim (variant 1 swaps one for a tall sunlit snapped shard).
    for i in 0..3 {
        let a = (i as f32 / 3.0) * TAU + 1.2;
        let sh = if variant == 1 && i == 0 { 0.46 } else { 0.10 + (i % 2) as f32 * 0.05 };
        let c = if i == 0 { lin_scaled(STUMP_BARK, 1.12) } else { lin(STUMP_BARK) };
        parts.push(lean_cone(0.05, sh, 4, 0.10, a, Vec3::new(a.cos() * r * 0.8, h, a.sin() * r * 0.8), c));
    }
    // Two shelf-fungus brackets stepped up one flank (ochre tops, shadowed undersides).
    parts.extend(shelf_bracket(Vec3::new(r + 0.02, h * 0.42, 0.05), 0.12, 0.2));
    parts.extend(shelf_bracket(Vec3::new(r - 0.01, h * 0.72, 0.10), 0.09, 0.5));
    // A ring of little cypress knees rising out of the muck, leaning back at the stump,
    // each tipped with a pale worn knob that catches light against the dark mud.
    let knees = 6;
    for i in 0..knees {
        let a = (i as f32 / knees as f32) * TAU + 0.3;
        let dist = r + 0.16 + (i % 2) as f32 * 0.12;
        let kh = 0.15 + (i % 3) as f32 * 0.07;
        let foot = Vec3::new(a.cos() * dist, 0.0, a.sin() * dist);
        parts.push(lean_cone(0.065, kh, 5, 0.18, -a, foot, lin(KNEE_WOOD)));
        let tip = Quat::from_rotation_y(-a) * (Quat::from_rotation_z(0.18) * Vec3::new(0.0, kh, 0.0)) + foot;
        parts.push(ball_at(0.035, tip, 0.8, lin_scaled(KNEE_WOOD, 1.25)));
    }
    flat_shaded(merged(parts))
}

/// **Cattail reed clump** — the FIRST non-tree class (so it is the tree-spacing fallback).
/// A fan of tall thin stalks rising out of a dark wet sheath, roughly half topped with a
/// brown cattail seed head AND the pale flowering spike poking on above it, plus a few
/// broad flattened leaf blades arcing out low. Base at y=0, ~0.8–1.0u tall so it reads
/// against the water. Two variants vary the stalk count / height.
fn build_reed_clump_mesh(variant: u32) -> Mesh {
    let count = if variant == 0 { 7 } else { 9 };
    let mut parts: Vec<Mesh> = Vec::new();
    // Dark wet sheath where the clump rises out of the muck (baked waterline shadow).
    parts.push(tinted(
        Cone { radius: 0.10, height: 0.16 }.mesh().resolution(6).build().translated_by(y(0.08)),
        lin_scaled(REED_STALK_DK, 0.65),
    ));
    for i in 0..count {
        let a = (i as f32 / count as f32) * TAU;
        let foot = 0.11;
        let bx = a.cos() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let bz = a.sin() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let h = 0.72 + ((i * 7) % 5) as f32 * 0.08 + variant as f32 * 0.10;
        let tilt = 0.05 + (i % 4) as f32 * 0.05;
        let foot_off = Vec3::new(bx, 0.0, bz);
        let lean = Quat::from_rotation_y(a) * Quat::from_rotation_z(tilt);
        let stalk_c = if i % 2 == 0 { lin(REED_STALK) } else { lin(REED_STALK_DK) };
        // Slender flat-shaded cone leaning out: build upright, lean (Z) then yaw (Y), shift.
        let stalk = Cone { radius: 0.020, height: h }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(h / 2.0))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(foot_off);
        parts.push(tinted(stalk, stalk_c));

        // Half the stalks carry a brown cattail seed head near the leaned-out tip, with
        // the pale flowering spike continuing on above the sausage.
        if i % 2 == 0 {
            let tip = lean * Vec3::new(0.0, h * 0.86, 0.0) + foot_off;
            let head = Cylinder::new(0.034, 0.17)
                .mesh()
                .resolution(6)
                .build()
                .rotated_by(lean)
                .translated_by(tip);
            parts.push(tinted(head, lin(CATTAIL_HEAD)));
            let spike_at = lean * Vec3::new(0.0, h * 0.86 + 0.135, 0.0) + foot_off;
            let spike = Cone { radius: 0.012, height: 0.10 }
                .mesh()
                .resolution(4)
                .build()
                .rotated_by(lean)
                .translated_by(spike_at);
            parts.push(tinted(spike, lin_scaled(REED_STALK, 1.25)));
        }
    }
    // Three broad leaf blades — flattened cones arcing out low, alternating dark / sunlit.
    for i in 0..3 {
        let a = (i as f32 / 3.0) * TAU + 0.8 + variant as f32 * 0.4;
        let h = 0.46 + (i % 2) as f32 * 0.10;
        let blade = Cone { radius: 0.055, height: h }
            .mesh()
            .resolution(4)
            .build()
            .scaled_by(Vec3::new(1.0, 1.0, 0.30)) // flatten the round cone into a blade
            .translated_by(y(h * 0.5))
            .rotated_by(Quat::from_rotation_z(0.38))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(y(0.022)); // clear y=0: the tilted blade base dips ~r·sin(0.38)
        let c = if i % 2 == 0 { lin(REED_STALK_DK) } else { lin_scaled(REED_STALK, 1.12) };
        parts.push(tinted(blade, c));
    }
    flat_shaded(merged(parts))
}

/// **Swamp accents** — the small accent class. Variants: 0 = a toadstool cluster (baked
/// gill shadows under the caps, sunlit crown bumps, pale flecks on the big cap); 1 = a
/// mud-skirted mossy boulder with stepped shelf-fungus brackets and shed pebbles; 2 = a
/// taller toadstool trio; 3 = a twisted half-sunken log (broken-end shards, snag
/// branches, a moss saddle, one bracket). Base at y=0.
fn build_accent_mesh(variant: u32) -> Mesh {
    match variant % 4 {
        // 0 — a loose cluster of dull swamp toadstools of mixed size. Each cap is three
        // tones: dark gill plate below, dull cap body, bright crown bump on top.
        0 => {
            let mut parts: Vec<Mesh> = Vec::new();
            let spots = [
                (0.0_f32, 0.0_f32, 1.25_f32),
                (0.13, 0.05, 0.9),
                (-0.10, 0.08, 0.7),
                (0.04, -0.12, 0.55),
            ];
            for (i, &(dx, dz, s)) in spots.iter().enumerate() {
                let stem_h = 0.12 * s;
                let cap_r = 0.085 * s;
                let at = Vec3::new(dx, 0.0, dz);
                parts.push(cyl_up(0.03 * s, stem_h, stem_h * 0.5, 6, lin(TOAD_STEM)).translated_by(at));
                parts.push(ball_at(cap_r * 0.92, at + y(stem_h), 0.22, lin_scaled(TOAD_CAP, 0.55)));
                parts.push(ball_at(cap_r, at + y(stem_h + 0.012), 0.6, lin(TOAD_CAP)));
                parts.push(ball_at(cap_r * 0.45, at + y(stem_h + cap_r * 0.5), 0.5, lin_scaled(TOAD_CAP, 1.3)));
                if i == 0 {
                    // Pale flecks on the big cap.
                    parts.push(ball_at(0.018, at + Vec3::new(cap_r * 0.5, stem_h + cap_r * 0.42, 0.02), 0.5, lin(TOAD_STEM)));
                    parts.push(ball_at(0.014, at + Vec3::new(-cap_r * 0.4, stem_h + cap_r * 0.45, -cap_r * 0.3), 0.5, lin(TOAD_STEM)));
                }
            }
            flat_shaded(merged(parts))
        }
        // 1 — a mossy boulder: dark mud skirt at the waterline, mid-tone body lumps, a
        // bright moss cap, shelf-fungus brackets stepping up the shaded side, shed pebbles.
        1 => {
            let mut parts = vec![
                ball_at(0.33, y(0.10), 0.30, lin_scaled(SWAMP_ROCK, 0.62)),
                ball_at(0.30, y(0.20), 0.82, lin(SWAMP_ROCK)),
                ball_at(0.17, Vec3::new(0.23, 0.13, 0.06), 0.85, lin_scaled(SWAMP_ROCK, 0.9)),
                ball_at(0.14, Vec3::new(-0.18, 0.12, -0.11), 0.85, lin_scaled(SWAMP_ROCK, 1.08)),
                // Moss cap catching what little light there is.
                ball_at(0.20, y(0.36), 0.5, lin(SWAMP_ROCK_MOSS)),
                ball_at(0.12, Vec3::new(0.08, 0.40, -0.06), 0.5, lin_scaled(SWAMP_ROCK_MOSS, 1.2)),
                // A couple of pebbles shed at the foot.
                ball_at(0.06, Vec3::new(0.34, 0.035, -0.16), 0.7, lin_scaled(SWAMP_ROCK, 0.85)),
                ball_at(0.045, Vec3::new(-0.30, 0.03, 0.18), 0.7, lin(SWAMP_ROCK)),
            ];
            parts.extend(shelf_bracket(Vec3::new(0.27, 0.17, 0.02), 0.13, 0.15));
            parts.extend(shelf_bracket(Vec3::new(0.25, 0.27, 0.05), 0.10, 0.3));
            parts.extend(shelf_bracket(Vec3::new(-0.24, 0.22, -0.08), 0.09, PI * 0.5));
            flat_shaded(merged(parts))
        }
        // 2 — a tight toadstool trio (taller, paler stems, same three-tone caps).
        2 => {
            let mut parts: Vec<Mesh> = Vec::new();
            for i in 0..3 {
                let a = (i as f32 / 3.0) * TAU;
                let at = Vec3::new(a.cos() * 0.08, 0.0, a.sin() * 0.08);
                let s = 1.0 + (i % 2) as f32 * 0.35;
                let stem_h = 0.15 * s;
                parts.push(cyl_up(0.026 * s, stem_h, stem_h * 0.5, 6, lin(TOAD_STEM)).translated_by(at));
                parts.push(ball_at(0.068 * s, at + y(stem_h), 0.2, lin_scaled(TOAD_CAP, 0.5)));
                parts.push(ball_at(0.072 * s, at + y(stem_h + 0.01), 0.55, lin(TOAD_CAP)));
                parts.push(ball_at(0.032 * s, at + y(stem_h + 0.045 * s), 0.5, lin_scaled(TOAD_CAP, 1.28)));
            }
            flat_shaded(merged(parts))
        }
        // 3 — a twisted half-sunken log: a squashed dark drum lying with one end lifted
        // out of the muck, jagged shards on the broken raised end, two snag branches, a
        // bright moss saddle along its back and a shelf bracket on the flank.
        _ => {
            let mut parts: Vec<Mesh> = Vec::new();
            let r = 0.13;
            let len = 0.95;
            let squash = 0.78;
            let tilt = 0.10_f32; // raise the -Z (broken) end slightly out of the muck
            let lift = r * squash + len * 0.5 * tilt.sin() + 0.005;
            let log = Cylinder::new(r, len)
                .mesh()
                .resolution(7)
                .build()
                .rotated_by(Quat::from_rotation_x(FRAC_PI_2)) // lay the axis along Z
                .scaled_by(Vec3::new(1.0, squash, 1.0)) // settle it into the mud
                .rotated_by(Quat::from_rotation_x(tilt))
                .translated_by(y(lift));
            parts.push(tinted(log, lin_scaled(MANGROVE_BARK, 0.85)));
            // Jagged shards fanning off the raised broken end.
            let end = Quat::from_rotation_x(tilt) * Vec3::new(0.0, 0.0, -len * 0.5) + y(lift);
            for i in 0..3 {
                let a = (i as f32 / 3.0) * TAU + 0.4;
                let sh = 0.10 + (i % 2) as f32 * 0.06;
                parts.push(lean_cone(0.035, sh, 4, 0.9, a, end, lin(MANGROVE_BARK_DK)));
            }
            // Two dead snag branches reaching up off the log's back.
            let back = lift + r * squash;
            parts.push(lean_cone(0.028, 0.32, 4, 0.55, 1.8, Vec3::new(0.0, back - 0.02, -0.08), lin(MANGROVE_BARK_DK)));
            parts.push(lean_cone(0.022, 0.22, 4, 0.85, 4.0, Vec3::new(0.0, back - 0.02, 0.16), lin(MANGROVE_BARK)));
            // Moss saddle along the top — brighter than the mud-dark drum.
            parts.push(ball_at(0.11, Vec3::new(0.0, back - 0.01, 0.05), 0.4, lin(STUMP_MOSS)));
            parts.push(ball_at(0.08, Vec3::new(0.02, back, -0.18), 0.4, lin_scaled(STUMP_MOSS, 1.15)));
            parts.extend(shelf_bracket(Vec3::new(0.12, lift, 0.10), 0.08, FRAC_PI_2));
            flat_shaded(merged(parts))
        }
    }
}

// ── Ground-cover builders (small, flat dressing) ─────────────────────────────────────

/// A low moss patch — squashed green lobes hugging the ground (~0.06u), edges scaled
/// darker than the centre, with two bright sunlit tufts riding the crown.
fn build_moss_patch_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    let lobes = [
        (0.0_f32, 0.0_f32, 0.13_f32, 1.0_f32), // (dx, dz, r, brightness)
        (0.12, 0.04, 0.10, 0.85),
        (-0.10, 0.08, 0.09, 0.9),
        (0.05, -0.11, 0.08, 0.8),
        (-0.06, -0.07, 0.07, 0.95),
    ];
    for &(dx, dz, r, v) in &lobes {
        parts.push(ball_at(r, Vec3::new(dx, 0.03, dz), 0.28, lin_scaled(MOSS_PATCH, v)));
    }
    parts.push(ball_at(0.05, Vec3::new(0.02, 0.055, 0.01), 0.4, lin_scaled(MOSS_PATCH, 1.35)));
    parts.push(ball_at(0.04, Vec3::new(0.11, 0.045, 0.05), 0.4, lin_scaled(MOSS_PATCH, 1.22)));
    flat_shaded(merged(parts))
}

/// A small reed sprig — six short blades fanned out of a dark muck foot, with one
/// near-upright centre stalk topped by a miniature cattail head.
fn build_reed_sprig_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    parts.push(ball_at(0.045, y(0.012), 0.4, lin_scaled(REED_STALK_DK, 0.6)));
    for i in 0..6 {
        let a = (i as f32 / 6.0) * TAU;
        let h = 0.26 + ((i * 5) % 3) as f32 * 0.07;
        let blade = Cone { radius: 0.015, height: h }
            .mesh()
            .resolution(4)
            .build()
            .translated_by(y(h * 0.5 + 0.004))
            .rotated_by(Quat::from_rotation_z(0.12 + (i % 3) as f32 * 0.06))
            .rotated_by(Quat::from_rotation_y(a));
        let c = if i % 2 == 0 { lin(REED_STALK) } else { lin(REED_STALK_DK) };
        parts.push(tinted(blade, c));
    }
    // The miniature cattail on the centre stalk.
    let h = 0.40;
    parts.push(tinted(
        Cone { radius: 0.013, height: h }.mesh().resolution(4).build().translated_by(y(h * 0.5)),
        lin(REED_STALK),
    ));
    parts.push(cyl_up(0.020, 0.085, h * 0.82, 5, lin(CATTAIL_HEAD)));
    flat_shaded(merged(parts))
}

/// A small swamp mushroom pair — a pale stem under a three-tone cap (dark gill plate /
/// dull body / bright crown bump) with a tiny button mushroom at its foot, ~0.14u tall.
fn build_swamp_mushroom_mesh() -> Mesh {
    let stem_h = 0.09;
    flat_shaded(merged(vec![
        cyl_up(0.026, stem_h, stem_h * 0.5, 6, lin(TOAD_STEM)),
        ball_at(0.064, y(stem_h), 0.22, lin_scaled(TOAD_CAP, 0.55)),
        ball_at(0.07, y(stem_h + 0.008), 0.55, lin(TOAD_CAP)),
        ball_at(0.03, y(stem_h + 0.035), 0.5, lin_scaled(TOAD_CAP, 1.3)),
        // The tiny button at its foot.
        cyl_up(0.014, 0.045, 0.0225, 5, lin(TOAD_STEM)).translated_by(Vec3::new(0.07, 0.0, 0.03)),
        ball_at(0.032, Vec3::new(0.07, 0.05, 0.03), 0.6, lin_scaled(TOAD_CAP, 0.9)),
    ]))
}

/// **Swamp ground accent** (cover). `variant`: 0 = bog cotton (green stems each topped
/// with a two-blob fluffy head — shaded core under a bright crown — over a dark muck
/// tuft), 1 = a pale lilac marsh flower (petal ring + gold core + two leaf blades). The
/// soft pale touches that lift the murky floor. Base at y=0, ~0.14–0.24u tall.
fn build_swamp_cover_extra_mesh(variant: u32) -> Mesh {
    match variant % 2 {
        // Bog cotton — three stems out of a dark tuft, fluffy two-tone heads on top.
        0 => {
            let mut parts: Vec<Mesh> = vec![ball_at(0.035, y(0.01), 0.4, lin_scaled(REED_STALK_DK, 0.7))];
            for i in 0..3 {
                let a = (i as f32 / 3.0) * TAU;
                let (bx, bz) = (a.cos() * 0.045, a.sin() * 0.045);
                let h = 0.17 + (i % 2) as f32 * 0.06;
                parts.push(cyl_up(0.009, h, h * 0.5, 4, lin(REED_STALK)).translated_by(Vec3::new(bx, 0.0, bz)));
                parts.push(ball_at(0.038, Vec3::new(bx, h, bz), 0.85, lin_scaled(BOG_COTTON, 0.92)));
                parts.push(ball_at(0.026, Vec3::new(bx, h + 0.028, bz), 0.8, lin(BOG_COTTON)));
            }
            flat_shaded(merged(parts))
        }
        // Pale lilac marsh flower — slim stem, two leaf blades, gold core, petal ring.
        _ => {
            let head_y = 0.15;
            let mut parts = vec![
                tinted(
                    Cone { radius: 0.009, height: head_y }.mesh().resolution(4).build().translated_by(y(head_y * 0.5)),
                    lin(REED_STALK_DK),
                ),
                ball_at(0.017, y(head_y + 0.004), 0.7, lin(SWAMP_FLOWER_CORE)),
            ];
            for &(a, lh) in &[(0.9_f32, 0.09_f32), (3.8, 0.07)] {
                parts.push(lean_cone(0.012, lh, 4, 0.7, a, y(0.012), lin(REED_STALK_DK)));
            }
            for i in 0..5 {
                let a = (i as f32 / 5.0) * TAU;
                parts.push(ball_at(0.027, Vec3::new(a.cos() * 0.04, head_y, a.sin() * 0.04), 0.45, lin(SWAMP_FLOWER)));
            }
            flat_shaded(merged(parts))
        }
    }
}

/// A lily-pad drift — each pad is a darker rim disc under a green top, lying flat on the
/// muck (the `Circle` mesh lies in the XY plane, normal +Z; rotate −90° about X to lie
/// flat). `variant`: 0 = a big pad with two satellites and a pale-pink closed lotus bud
/// (gold heart wrapped in petal blobs); 1 = a plain drift of three mixed-size pads.
#[allow(dead_code)] // kept for future river-surface placement; no longer scattered on the dry floor
fn build_lily_disc_mesh(variant: u32) -> Mesh {
    let flat = |m: Mesh| -> Mesh { m.rotated_by(Quat::from_rotation_x(-FRAC_PI_2)) };
    let pad = |at: Vec3, r: f32| -> [Mesh; 2] {
        [
            tinted(
                flat(Circle::new(r).mesh().resolution(10).build()).translated_by(at + y(0.004)),
                lin(LILY_PAD_EDGE),
            ),
            tinted(
                flat(Circle::new(r * 0.86).mesh().resolution(10).build()).translated_by(at + y(0.012)),
                lin(LILY_PAD),
            ),
        ]
    };
    let mut parts: Vec<Mesh> = Vec::new();
    match variant % 2 {
        0 => {
            parts.extend(pad(Vec3::ZERO, 0.24));
            parts.extend(pad(Vec3::new(0.30, 0.0, 0.12), 0.13));
            parts.extend(pad(Vec3::new(-0.20, 0.0, -0.22), 0.10));
            // The closed lotus bud riding the big pad.
            let bud = Vec3::new(0.06, 0.02, -0.04);
            parts.push(ball_at(0.030, bud + y(0.035), 1.3, lin(SWAMP_FLOWER_CORE)));
            for i in 0..4 {
                let a = (i as f32 / 4.0) * TAU + 0.4;
                parts.push(ball_at(0.030, bud + Vec3::new(a.cos() * 0.030, 0.030, a.sin() * 0.030), 1.15, lin(LILY_BLOOM)));
            }
        }
        _ => {
            parts.extend(pad(Vec3::ZERO, 0.20));
            parts.extend(pad(Vec3::new(0.26, 0.0, -0.10), 0.14));
            parts.extend(pad(Vec3::new(-0.14, 0.0, 0.24), 0.11));
        }
    }
    flat_shaded(merged(parts))
}

/// **Big hollow dead swamp tree** (landmark) — five bark slabs bowed around the trunk
/// axis leaving a dark gap (the hollow), each split at the waterline into a mud-stained
/// dark lower block under a paler upper; a dark rotten heartwood column shows through the
/// gap. Buttress fins + a flared mossy base root it in the muck, a jagged two-tone broken
/// top crowns it, and two stubbed limbs drip hanging-moss strands. Shelf-fungus brackets
/// step up the shaded flank under clinging moss. Base at y=0, ~3u tall, authored at full
/// scale (the landmark spawns it un-scaled).
pub(crate) fn build_hollow_dead_tree_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    let trunk_h = 2.6;
    let radius = 0.55;
    // Five bark slabs around the trunk ring, leaving a gap (the hollow). Lower blocks are
    // mud-dark (waterline stain), uppers alternate two paler bark tones.
    let slabs = [
        (0.9_f32, 1.0_f32), // (yaw centre, width factor)
        (2.1, 0.85),
        (3.2, 1.0),
        (4.3, 0.8),
        (5.2, 0.9),
    ];
    for (i, &(yaw, wf)) in slabs.iter().enumerate() {
        let low_h = 0.8;
        let lower = Cuboid::new(0.34 * wf, low_h, 0.30)
            .mesh()
            .build()
            .translated_by(y(low_h * 0.5))
            .translated_by(Vec3::new(0.0, 0.0, radius)) // push out onto the trunk ring
            .rotated_by(Quat::from_rotation_y(yaw));
        parts.push(tinted(lower, lin_scaled(MANGROVE_BARK, 0.72)));
        let up_h = trunk_h - low_h;
        let bright = if i % 2 == 0 { 1.0 } else { 1.12 };
        let upper = Cuboid::new(0.30 * wf, up_h, 0.26)
            .mesh()
            .build()
            .translated_by(y(low_h + up_h * 0.5))
            .translated_by(Vec3::new(0.0, 0.0, radius + 0.02))
            .rotated_by(Quat::from_rotation_y(yaw));
        parts.push(tinted(upper, lin_scaled(MANGROVE_BARK, bright)));
    }
    // The dark rotten heartwood column showing through the hollow gap (inset, sub-rim).
    parts.push(cyl_up(0.34, trunk_h * 0.92, trunk_h * 0.46, 7, lin_scaled(STUMP_TOP, 0.40)));
    // Buttress fins leaning back into the trunk + a flared mossy base, all mud-dark.
    for i in 0..4 {
        let a = (i as f32 / 4.0) * TAU + 0.6;
        let foot = Vec3::new(a.cos() * (radius + 0.25), 0.0, a.sin() * (radius + 0.25));
        parts.push(lean_cone(0.16, 0.85, 5, 0.42, -a, foot, lin_scaled(MANGROVE_ROOT, 0.85)));
    }
    // (flare squash kept shallow so the blob bottoms rest AT y=0 — r·squash ≤ centre y)
    parts.push(ball_at(radius * 1.45, y(0.16), 0.20, lin_scaled(MANGROVE_ROOT, 0.8)));
    parts.push(ball_at(radius * 1.05, Vec3::new(radius, 0.15, radius * 0.4), 0.26, lin_scaled(MANGROVE_ROOT, 0.9)));

    // Jagged broken top — shards of differing height around the rim, alternating shadowed
    // and sun-bleached tones.
    for i in 0..6 {
        let a = (i as f32 / 6.0) * TAU + 0.5;
        let sh = 0.45 + ((i * 7) % 4) as f32 * 0.28;
        let c = if i % 2 == 0 { lin(MANGROVE_BARK_DK) } else { lin_scaled(MANGROVE_BARK, 1.18) };
        let shard = Cone { radius: 0.16, height: sh }
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(sh * 0.5))
            .translated_by(Vec3::new(a.cos() * radius * 0.72, trunk_h, a.sin() * radius * 0.72));
        parts.push(tinted(shard, c));
    }
    // Two stubbed broken limbs jutting out partway up, each dripping two moss strands
    // (down-cones + wispy tip blobs) — the landmark-scale echo of the mangrove moss.
    for &(yaw, hgt, len) in &[(0.6_f32, 1.7_f32, 0.9_f32), (3.6, 2.0, 0.7)] {
        let tiltq = Quat::from_rotation_z(FRAC_PI_2 - 0.3);
        let spin = Quat::from_rotation_y(yaw);
        let limb = Cylinder::new(0.09, len)
            .mesh()
            .resolution(5)
            .build()
            .translated_by(y(len * 0.5))
            .rotated_by(tiltq)
            .rotated_by(spin)
            .translated_by(y(hgt));
        parts.push(tinted(limb, lin(MANGROVE_BARK)));
        for &(t, hang) in &[(1.0_f32, 0.6_f32), (0.55, 0.42)] {
            let at = spin * (tiltq * Vec3::new(0.0, len * t, 0.0)) + y(hgt);
            let c = if t > 0.9 { lin(HANGING_MOSS) } else { lin_scaled(HANGING_MOSS, 0.8) };
            let strand = Cone { radius: 0.045, height: hang }
                .mesh()
                .resolution(4)
                .build()
                .translated_by(y(hang * 0.5))
                .rotated_by(Quat::from_rotation_x(PI))
                .translated_by(at);
            parts.push(tinted(strand, c));
            parts.push(ball_at(0.09, at - y(hang), 0.55, c));
        }
    }
    // Shelf-fungus brackets stepping up the shaded flank, moss clinging above them.
    parts.extend(shelf_bracket(Vec3::new(-radius - 0.10, 0.6, radius * 0.25), 0.20, 2.6));
    parts.extend(shelf_bracket(Vec3::new(-radius - 0.06, 1.0, radius * 0.30), 0.16, 2.8));
    parts.extend(shelf_bracket(Vec3::new(-radius - 0.02, 1.35, radius * 0.22), 0.12, 2.4));
    parts.push(ball_at(0.30, Vec3::new(-radius * 0.62, 1.7, radius * 0.3), 0.65, lin(STUMP_MOSS)));
    parts.push(ball_at(0.22, Vec3::new(-radius * 0.5, 2.15, radius * 0.2), 0.65, lin_scaled(STUMP_MOSS, 1.15)));

    flat_shaded(merged(parts))
}

// ── Glowing mushroom cluster (a swamp landmark accent, split by material) ─────────────
//
// A cluster of 6 mushrooms of mixed size. The STEMS are a separate pale vertex-coloured
// mesh (rides the shared white mat); the CAPS are a separate mesh carrying NO colour
// attribute, so an emissive glow material lights them up and feeds bloom. The two meshes
// share `GLOWMUSH_SPOTS`, so a cap sits exactly atop each stem when spawned at one
// transform. Base flush at y=0.

/// Pale stems for the glowmush cluster (shared white vertex-colour mat): each stem wears
/// a slightly darker collar ring just under its cap (baked under-cap shadow), and the
/// bigger stems get a damp moss tuft at the foot.
pub(crate) fn build_glowmush_stems_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    for &(dx, dz, s) in &GLOWMUSH_SPOTS {
        let sh = 0.14 * s;
        let at = Vec3::new(dx, 0.0, dz);
        parts.push(cyl_up(0.03 * s, sh, sh * 0.5, 6, lin(GLOWMUSH_STEM)).translated_by(at));
        parts.push(cyl_up(0.042 * s, 0.02, sh - 0.016, 6, lin_scaled(GLOWMUSH_STEM, 0.72)).translated_by(at));
        if s >= 0.85 {
            parts.push(ball_at(0.05 * s, at + Vec3::new(0.035, 0.024, -0.02), 0.35, lin_scaled(MOSS_PATCH, 0.9)));
        }
    }
    flat_shaded(merged(parts))
}

/// Glowing caps for the glowmush cluster — domed squashed blobs with NO colour attribute
/// (the emissive material owns the colour). Big caps use ico-1 for a rounder dome, the
/// small ones stay chunky ico-0. Built to match the stem layout/heights.
pub(crate) fn build_glowmush_caps_mesh() -> Mesh {
    let mut parts: Vec<Mesh> = Vec::new();
    for &(dx, dz, s) in &GLOWMUSH_SPOTS {
        let sh = 0.14 * s;
        let detail = if s >= 1.0 { 1 } else { 0 };
        parts.push(
            Sphere::new(0.085 * s)
                .mesh()
                .ico(detail)
                .expect("ico detail in range")
                .scaled_by(Vec3::new(1.0, 0.62, 1.0))
                .translated_by(Vec3::new(dx, sh, dz)),
        );
    }
    // Merge raw (no ATTRIBUTE_COLOR on any part) then flat-shade for crisp facets.
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one cap");
    for p in it {
        base.merge(&p).expect("glowmush caps share attributes");
    }
    flat_shaded(base)
}

// ── BiomeConfig ──────────────────────────────────────────────────────────────────────

pub fn config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Swamp,
        name: "Swamp",

        // Dark olive green-brown wet ground. Low roughness so the dim sun throws a damp
        // specular sheen across the muck — reads as standing bog-water rather than dry dirt.
        ground_color: 0x49543a,
        ground_roughness: 0.25,
        detail: GroundDetail {
            scale: 0.16,
            strength: 0.62, // a touch stronger so the finer grain reads (was 0.55)
            variation: 0.95, // high → blotchy wet/dry mottle
            seed: 11.0,
            dark: 0x2c3522, // muddy shadow pools
            base: 0x49543a, // olive base
            light: 0x687a4a, // mossy highlights
            grain: 0.85, // finer wet tooth (was 0.7)
            streak: 0.6,
        },

        // Dim marsh daylight under a NEUTRAL cool-grey damp mist — NOT a green fog. A green-tinted
        // haze painted the whole distance a flat colour wall; a plain desaturated grey reads as
        // proper marsh mist you see into. The swamp's green identity comes entirely from its
        // vegetation + the glowing herbs + the dim, low light — never from a coloured fog.
        sky: 0xbcc2c2,
        fog_density: 0.004, // BELOW the island baseline → fog pushes OUT; the swamp reads open, not hazy
        sun_color: 0xe6e7da, // near-neutral, faintly cool (brightened off 0xdadbce — was darkening the key)
        // 7000 (≈0.70× the island key) then 9000 (≈0.85×) both still read too dim/murky in the
        // marsh (player feedback). 12000 ≈ 1.07× — the swamp now reads a normally-lit, open wetland.
        sun_illuminance: 12_000.0,
        ambient_color: 0xb6bdbc, // neutral cool-grey fill
        ambient_brightness: 68.0,
        sun_pos: Vec3::new(12.0, 30.0, 14.0),

        seed: 5005,
        tree_min_dist: 2.6,
        classes: vec![
            // Trees: 70% gnarled mangrove (3 variants) / 30% cypress stump (2 variants).
            PropClass {
                variants: vec![
                    (build_mangrove_mesh(0), 0.28),
                    (build_mangrove_mesh(1), 0.22),
                    (build_mangrove_mesh(2), 0.20),
                    (build_cypress_stump_mesh(0), 0.18),
                    (build_cypress_stump_mesh(1), 0.12),
                ],
                // Thinned ~35% (player: swamp too dense) — was 0.085.
                chance: 0.055,
                scale: (0.85 * TREE_SCALE, 1.25 * TREE_SCALE),
                tree: true,
                block_radius: 0.0,
            },
            // Cattail reed clumps — FIRST non-tree class → the tree-spacing fallback.
            PropClass {
                variants: vec![
                    (build_reed_clump_mesh(0), 1.0),
                    (build_reed_clump_mesh(1), 1.0),
                ],
                chance: 0.04, // thinned ~35% — was 0.06
                scale: (0.85, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            // Toadstools / mossy rock / toadstool trio / half-sunken log accents.
            PropClass {
                variants: (0..4).map(|v| (build_accent_mesh(v), 1.0)).collect(),
                chance: 0.026, // thinned ~35% — was 0.04
                scale: (0.7, 1.5),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover: vec![
            PropClass {
                variants: vec![(build_moss_patch_mesh(), 1.0)],
                // Cut hard (was 0.22): the flat rounded moss mounds carpeting the floor read as
                // unnatural "plates everywhere" (player). A scattered few, not a tiled sheet.
                chance: 0.09,
                scale: (0.7, 1.4),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_reed_sprig_mesh(), 1.0)],
                chance: 0.12, // thinned ~35% — was 0.18
                scale: (0.7, 1.2),
                tree: false,
                block_radius: 0.0,
            },
            PropClass {
                variants: vec![(build_swamp_mushroom_mesh(), 1.0)],
                chance: 0.08, // thinned ~35% — was 0.12
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
            // Lily pads CUT from the dry floor: perfect flat discs lying on muck read as the
            // unnatural "rounded plates" (player). Pads belong on open water; scattering them as
            // ground cover was the worst offender. Keep `build_lily_disc_mesh` for future
            // river-surface placement.
            // Soft pale floor accents — bog cotton + lilac marsh flowers.
            PropClass {
                variants: (0..2).map(|v| (build_swamp_cover_extra_mesh(v), 1.0)).collect(),
                chance: 0.065, // thinned ~35% — was 0.10
                scale: (0.7, 1.3),
                tree: false,
                block_radius: 0.0,
            },
        ],
        cover_per_tile: 2,

        river: true,
        river_color: 0x3f5a44, // murky green swamp water
        backdrop: Backdrop {
            land_dir: -FRAC_PI_2,
            land_arc: PI * 0.62, // land wraps most of the horizon
            ocean: false,
            ocean_color: 0x2f5a4a,
            // Low murky hills (desaturated grey-greens).
            hill_body: 0x5a6452,
            hill_cap: 0x76806a,
            hill_foot: 0x474f40,
            // Dark misty conifer treeline ringing the marsh.
            treeline: true,
            treeline_dark: 0x223526,
            treeline_mid: 0x2e4530,
            hill_h: (26.0, 58.0),
        },
        // No weather (fog stays pushed out — the player didn't want haze here), but FIREFLIES
        // (map-character overhaul pass 3): 32 warm emissive motes fade in whenever the hero
        // walks the marsh — with the bog pools + wisps + glow-mushrooms they make the swamp at
        // night an attraction instead of a void. Was `None` (the Fireflies preset sat
        // implemented-but-unused in particles.rs since the port).
        particle: ParticleKind::Fireflies,
    }
}

// ── Landmarks ────────────────────────────────────────────────────────────────────────

/// Big hollow dead tree on the land side + a knot of glowing greenish will-o'-wisp motes
/// hovering over the muck beside it. Every spawn is tagged [`BiomeEntity`] so the biome
/// switch wipes it. The motes are unlit emissive spheres so they glow against the dim
/// swamp light and feed bloom; they hover at a fixed ~1u height (no animation system — the
/// module is self-contained and registers no plugin).
pub fn landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Shared white vertex-colour material for the dead-tree set-piece.
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });

    // The hollow dead tree, planted on the land side (z < 0).
    commands.spawn((
        Mesh3d(meshes.add(build_hollow_dead_tree_mesh())),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(-9.0, 0.0, -11.0).with_rotation(Quat::from_rotation_y(0.5)),
        BiomeEntity,
    ));

    // ── Will-o'-wisp motes — small unlit emissive greenish spheres hovering ~1u up in a
    // loose knot near the dead tree, over the muck on the land side.
    let wisp_mesh = meshes.add(Sphere::new(0.07).mesh().ico(1).expect("ico detail in range"));
    let wisp_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 1.0, 0.6),
        emissive: LinearRgba::from(WISP_GLOW) * WISP_EMISSIVE,
        unlit: true,
        ..default()
    });

    // A small deterministic spread of motes around two clusters near the dead tree / water.
    let centres = [Vec3::new(-6.5, 0.0, -7.5), Vec3::new(-11.5, 0.0, -9.0)];
    // (cluster index, dx, dz, height)
    let motes = [
        (0usize, 0.0_f32, 0.0_f32, 1.00_f32),
        (0, 1.4, -0.8, 1.30),
        (0, -1.1, 1.0, 0.85),
        (0, 0.8, 1.5, 1.15),
        (1, 0.0, 0.0, 1.10),
        (1, -1.3, -0.6, 0.90),
        (1, 1.2, 0.9, 1.25),
    ];
    for &(ci, dx, dz, hy) in &motes {
        let pos = centres[ci] + Vec3::new(dx, hy, dz);
        commands.spawn((
            Mesh3d(wisp_mesh.clone()),
            MeshMaterial3d(wisp_mat.clone()),
            Transform::from_translation(pos),
            NotShadowCaster,
            BiomeEntity,
        ));
    }

    // ── Glowing mushroom clusters — pale stems (shared white mat) under bioluminescent
    // caps (emissive mat → bloom). Spread across the patch with a local Mulberry32 RNG,
    // skipping the river column and the open centre framing. ~14 clusters.
    let glow_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.40, 0.95, 0.82),
        emissive: LinearRgba::from(GLOWMUSH_GLOW) * GLOWMUSH_EMISSIVE,
        unlit: true,
        ..default()
    });
    let stems = meshes.add(build_glowmush_stems_mesh());
    let caps = meshes.add(build_glowmush_caps_mesh());

    let mut seed = 0x51ed_2a17_u32;
    let mut next = || {
        seed = seed.wrapping_add(0x6d2b_79f5);
        let mut t = seed;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    };
    let mut placed = 0;
    for _ in 0..160 {
        if placed >= 14 {
            break;
        }
        let x = -14.0 + next() * 28.0;
        let z = -14.0 + next() * 28.0;
        // Skip the river band and the open framing in front of the camera.
        if crate::water::on_river(x, z) || (x * x + z * z) < 9.0 {
            continue;
        }
        let tf = Transform {
            translation: Vec3::new(x, 0.0, z),
            rotation: Quat::from_rotation_y(next() * TAU),
            scale: Vec3::splat(0.85 + next() * 0.7),
        };
        commands.spawn((Mesh3d(stems.clone()), MeshMaterial3d(mat.clone()), tf, BiomeEntity));
        commands.spawn((
            Mesh3d(caps.clone()),
            MeshMaterial3d(glow_mat.clone()),
            tf,
            NotShadowCaster,
            BiomeEntity,
        ));
        placed += 1;
    }
}
