//! The five signature biome landmarks, rebuilt as animated set-pieces (map-character overhaul,
//! July 2026): the **Old Mill** (forest — sails that still turn), the **Witch's Hut** (swamp —
//! green-lit windows, a bubbling cauldron, swaying charms), the **Frozen Spire** (snow — a
//! glowing ice heart circled by levitating shards), the **Sunken Pyramid** (desert — a grand
//! half-buried ziggurat crowned by a spinning sun-disc relic) and the **Standing Stones**
//! (rocky — a walkable rune circle around a hovering keystone).
//!
//! Each builder returns a [`crate::ruins::LandmarkModel`]: ONE merged vertex-coloured base mesh
//! (all the static geometry — batches against the scene's shared white material) plus a handful
//! of [`crate::ruins::AnimPart`] children for whatever moves or glows. Authored in model-local
//! units with the base at y=0; `ruins::populate_landmarks` scales ≈1.4–1.5× into the world.
//! Sizes matter twice: `interaction::LANDMARK_DIST` (3.6u from the root) must stay reachable
//! past the blockers, and the whole silhouette should break its biome's treeline — these are
//! the island's skyline flags. Preview any of them with `FOREST_VIEW=landmark:mill|hut|spire|
//! pyramid|stones` (the viewer runs the same FX systems, so the motion reads there too).

use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::palette::{lin, lin_scaled};
use crate::ruins::{
    ball, box_at, box_rot, chunk_at, cone_at, cyl, cyl_rot, facet_at, facet_tinted, flat_shaded,
    flat_stone, frustum, hash3, lichen_at, limb, merged, mottle, slab_at, slab_ground, tinted, yv,
    AnimPart, Fx, Glow, LandmarkModel,
};

/// A tinted axis-aligned box (the yard-dressing workhorse).
fn tbox(x: f32, y: f32, z: f32, center: Vec3, c: u32) -> Mesh {
    tinted(box_at(x, y, z, center), lin(c))
}

/// A faceted icosphere `Mesh` (unbuilt — chain `scaled_by`/`rotated_by`/`translated_by`).
fn ico(r: f32, detail: u32) -> Mesh {
    Sphere::new(r).mesh().ico(detail).expect("ico detail in range")
}

// ═══════════════════════════════════════════════════════════════════════════════
//  THE OLD MILL — forest. A weathered smock windmill: fieldstone footing, tapered
//  eight-sided timber tower, thatched cap, and four lattice sails that STILL TURN
//  though no miller has climbed the hill in years. One sail's canvas hangs torn;
//  ivy takes the shaded side; a millstone leans by the door; a candle burns.
// ═══════════════════════════════════════════════════════════════════════════════

const MILL_STONE: u32 = 0x8a8074; // fieldstone footing
const MILL_STONE_DK: u32 = 0x6b655c;
const MILL_TIMBER: u32 = 0x6e5a42; // weathered oak boards
const MILL_TIMBER_DK: u32 = 0x52422f; // weather bands / lattice
const MILL_TIMBER_XDK: u32 = 0x3a2e20; // door frame / spars / iron-dark wood
const MILL_THATCH: u32 = 0x96713f; // straw cap
const MILL_THATCH_DK: u32 = 0x7a5a30;
const MILL_CANVAS: u32 = 0xcfc2a2; // sailcloth
const MILL_CANVAS_DK: u32 = 0xb0a284; // the torn, mildewed panel
const MILL_IVY: u32 = 0x5a7a3a;
const MILL_IVY_DK: u32 = 0x47632c;
const MILL_SACK: u32 = 0xb59a6a;
const MILL_DOOR: u32 = 0x241b10; // dark doorway recess

/// Sail hub mount (model-local): on the cap's +Z face, high enough that the lowest
/// sail tip clears a rider's head.
const MILL_HUB: Vec3 = Vec3::new(0.0, 4.75, 1.58);
const SAIL_LEN: f32 = 2.3;

pub fn old_mill() -> LandmarkModel {
    let mut parts: Vec<Mesh> = Vec::new();

    // ── Fieldstone footing: a faceted drum ringed by a rough boulder course.
    parts.push(frustum(1.75, 1.5, 0.6, yv(0.3), 10, MILL_STONE));
    for i in 0..9 {
        let a = i as f32 * TAU / 9.0 + 0.23;
        let r = 0.24 + hash3(a, 1.0, 0.0) * 0.14;
        parts.push(chunk_at(
            r,
            Vec3::new(a.cos() * 1.66, r * 0.6, a.sin() * 1.66),
            Vec3::new(1.15, 0.8, 1.0),
            a,
            0.1,
            0,
            if i % 2 == 0 { MILL_STONE } else { MILL_STONE_DK },
        ));
    }

    // ── Smock tower: tapered eight-sided timber body + darker weather bands.
    parts.push(frustum(1.42, 0.95, 3.6, yv(2.4), 8, MILL_TIMBER));
    for &(y, r) in &[(1.55_f32, 1.32_f32), (2.55, 1.2), (3.55, 1.07)] {
        parts.push(frustum(r + 0.045, r - 0.02, 0.12, yv(y), 8, MILL_TIMBER_DK));
    }
    // (No corner staves — a first pass had them, but leaning limbs off a tapered shell read
    // as stray scaffold poles; the weather-band frustums carry the "boarded" look alone.)

    // ── Door (+Z face): dark recess, heavy jambs + lintel, a stone step.
    parts.push(tbox(0.62, 1.15, 0.16, Vec3::new(0.0, 0.85, 1.52), MILL_DOOR));
    parts.push(box_rot(0.14, 1.25, 0.2, Vec3::new(-0.42, 0.88, 1.5), Quat::from_rotation_z(0.04), MILL_TIMBER_XDK));
    parts.push(box_rot(0.14, 1.25, 0.2, Vec3::new(0.42, 0.88, 1.5), Quat::from_rotation_z(-0.04), MILL_TIMBER_XDK));
    parts.push(box_rot(0.95, 0.16, 0.24, Vec3::new(0.0, 1.5, 1.48), Quat::from_rotation_z(0.03), MILL_TIMBER_XDK));
    parts.push(flat_stone(0.85, 0.14, 0.55, Vec3::new(0.0, 0.07, 1.85), 0.06, MILL_STONE_DK));
    // A shuttered dark window round the -X side.
    parts.push(box_rot(0.16, 0.4, 0.34, Vec3::new(-1.22, 2.5, 0.3), Quat::from_rotation_y(0.25), MILL_DOOR));

    // ── Thatch cap: flared skirt + dome + finial knob (overhangs the tower top).
    parts.push(frustum(1.22, 0.78, 0.55, yv(4.48), 8, MILL_THATCH));
    parts.push(frustum(0.78, 0.3, 0.5, yv(5.0), 8, MILL_THATCH_DK));
    parts.push(cone_at(0.32, 0.42, yv(5.46), 8, MILL_THATCH));
    parts.push(ball(0.09, yv(5.62), 1.0, MILL_TIMBER_XDK));
    parts.push(frustum(1.24, 1.1, 0.07, yv(4.24), 8, MILL_THATCH_DK));

    // ── Ivy: splotches climbing the shaded (-Z/-X) quarter, thickest at the foot.
    for &(x, y, z, r, c) in &[
        (-0.95_f32, 0.55_f32, -1.15_f32, 0.42_f32, MILL_IVY),
        (-1.25, 1.1, -0.65, 0.34, MILL_IVY_DK),
        (-1.05, 1.75, -0.85, 0.3, MILL_IVY),
        (-1.2, 2.4, -0.35, 0.26, MILL_IVY_DK),
        (-0.85, 3.0, -0.75, 0.22, MILL_IVY),
        (-0.55, 0.4, -1.45, 0.3, MILL_IVY_DK),
    ] {
        // Pressed onto the tapering wall: pull each splotch to the local wall radius.
        let rad = (x * x + z * z).sqrt();
        let wall = 1.44 - y * 0.13;
        let (nx, nz) = (x / rad * wall, z / rad * wall);
        parts.push(lichen_at(r, Vec3::new(nx, y, nz), c));
        parts.push(facet_at(r * 0.6, Vec3::new(nx, y + r * 0.3, nz), 0.5, c));
    }

    // ── Yard: leaning millstone, grain sacks, a broken cartwheel, path flagstones.
    let stone_rot = Quat::from_rotation_y(0.5) * Quat::from_rotation_x(1.32);
    parts.push(cyl_rot(0.58, 0.16, 12, Vec3::new(1.62, 0.56, 0.95), stone_rot, MILL_STONE));
    parts.push(cyl_rot(0.13, 0.18, 6, Vec3::new(1.62, 0.56, 0.95), stone_rot, MILL_STONE_DK));
    for &(x, z, r) in &[(0.95_f32, 1.7_f32, 0.3_f32), (1.3, 1.45, 0.24), (1.05, 1.35, 0.2)] {
        parts.push(ball(r, Vec3::new(x, r * 0.72, z), 0.78, MILL_SACK));
        parts.push(facet_tinted(r * 0.4, Vec3::new(x, r * 1.32, z), 0.5, lin_scaled(MILL_SACK, 0.82)));
    }
    let wheel_rot = Quat::from_rotation_y(-0.4) * Quat::from_rotation_x(1.25);
    parts.push(tinted(
        Torus { minor_radius: 0.05, major_radius: 0.42 }
            .mesh()
            .minor_resolution(4)
            .major_resolution(10)
            .build()
            .rotated_by(wheel_rot)
            .translated_by(Vec3::new(-1.55, 0.42, 1.05)),
        lin(MILL_TIMBER_DK),
    ));
    parts.push(box_rot(0.78, 0.055, 0.055, Vec3::new(-1.55, 0.42, 1.05), wheel_rot, MILL_TIMBER_XDK));
    parts.push(box_rot(0.055, 0.055, 0.78, Vec3::new(-1.55, 0.42, 1.05), wheel_rot, MILL_TIMBER_XDK));
    for (i, &(x, z)) in [(0.0_f32, 2.5_f32), (0.35, 3.1), (-0.25, 3.65)].iter().enumerate() {
        parts.push(facet_at(0.34 - i as f32 * 0.03, Vec3::new(x, 0.045, z), 0.16, MILL_STONE));
    }

    let base = mottle(flat_shaded(merged(parts)), 0.42);

    // ── Animated parts ──────────────────────────────────────────────────────────

    // The sail cross: hub boss + four lattice arms in the part-local XY plane, spinning
    // about local +Z. Mounted on the cap's +Z face, canted back ~8° like a real smock mill.
    let mut sail: Vec<Mesh> = Vec::new();
    sail.push(tinted(
        Cylinder::new(0.17, 0.5).mesh().resolution(8).build().rotated_by(Quat::from_rotation_x(FRAC_PI_2)),
        lin(MILL_TIMBER_XDK),
    ));
    sail.push(ball(0.12, Vec3::new(0.0, 0.0, 0.26), 1.0, MILL_TIMBER_DK));
    for k in 0..4 {
        let rot = Quat::from_rotation_z(k as f32 * FRAC_PI_2);
        let torn = k == 2; // one sail's canvas hangs short — the mill is OLD.
        sail.push(tinted(box_at(0.09, SAIL_LEN, 0.07, yv(SAIL_LEN * 0.5)).rotated_by(rot), lin(MILL_TIMBER_XDK)));
        // Lattice crossbars along the outer half.
        let bars = if torn { 2 } else { 4 };
        for b in 0..bars {
            let y = 0.95 + b as f32 * 0.42;
            sail.push(tinted(
                box_at(0.62, 0.055, 0.035, Vec3::new(0.115, y, 0.0)).rotated_by(rot),
                lin(MILL_TIMBER_DK),
            ));
        }
        // Canvas panel to one side of the spar (short + mildewed on the torn arm).
        let (len, cc) = if torn { (0.72, MILL_CANVAS_DK) } else { (1.62, MILL_CANVAS) };
        sail.push(tinted(
            box_at(0.5, len, 0.028, Vec3::new(0.34, SAIL_LEN - 0.12 - len * 0.5, 0.0)).rotated_by(rot),
            lin(cc),
        ));
        // (No loose "torn flap" — folding a panel off-plane read as a detached floating card;
        // the short mildewed panel + missing crossbars carry the neglect on their own.)
    }
    let sail_mesh = mottle(flat_shaded(merged(sail)), 0.3);
    // Gentle 6° back-cant; the hub stands proud of the cap so the cross never clips the thatch.
    let sail_xf = Transform::from_translation(MILL_HUB).with_rotation(Quat::from_rotation_x(0.1));

    // Weathervane: a little iron arrow that hunts back and forth in the wind.
    let vane = flat_shaded(merged(vec![
        tbox(0.03, 0.42, 0.03, yv(0.21), MILL_TIMBER_XDK),
        tbox(0.5, 0.05, 0.03, yv(0.42), MILL_TIMBER_XDK),
        tinted(
            Cone { radius: 0.055, height: 0.16 }
                .mesh()
                .resolution(4)
                .build()
                .rotated_by(Quat::from_rotation_z(-FRAC_PI_2))
                .translated_by(Vec3::new(0.3, 0.42, 0.0)),
            lin(MILL_TIMBER_XDK),
        ),
        tbox(0.12, 0.16, 0.02, Vec3::new(-0.24, 0.42, 0.0), MILL_TIMBER_DK),
    ]));

    // The lit window: the miller's ghost keeps a candle burning. Gentle pulse.
    let window = flat_shaded(tbox(0.26, 0.34, 0.1, Vec3::ZERO, 0xffffff));

    LandmarkModel {
        base,
        parts: vec![
            AnimPart::solid(sail_mesh, sail_xf, Fx::Spin { axis: Vec3::Z, rate: 0.42 }),
            AnimPart::solid(
                vane,
                Transform::from_xyz(0.0, 5.66, 0.0),
                Fx::Sway { axis: Vec3::Y, amp: 0.55, freq: 0.31, phase: 0.0 },
            ),
            AnimPart::glowing(
                // Pressed into the wall shell (radius ≈1.04 at this height) so it reads as a
                // casement, not a floating pane.
                window,
                Transform::from_xyz(0.57, 3.1, 0.88).with_rotation(Quat::from_rotation_y(0.57)),
                Fx::None,
                Glow { color: Color::srgb_u8(0xff, 0xb4, 0x5e), strength: 4.0, pulse: Some((1.1, 0.28, 0.0)) },
            ),
        ],
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  THE WITCH'S HUT — swamp. A crooked stilt-cabin with a sagging moss roof, green
//  witch-light in the windows, smoke curling off the chimney, charms swaying from
//  the eaves and a cauldron bubbling in the yard, circled by wisps.
// ═══════════════════════════════════════════════════════════════════════════════

const HUT_STILT: u32 = 0x413830; // sodden stilt wood
const HUT_PLANK: u32 = 0x5d4f3e; // wall boards
const HUT_PLANK_DK: u32 = 0x453a2c;
const HUT_ROOF: u32 = 0x4f5a36; // moss-eaten shingles
const HUT_ROOF_DK: u32 = 0x3c4629;
const HUT_MOSS: u32 = 0x6a7b3c;
const HUT_STONE: u32 = 0x5c5a52; // chimney / fire-ring stone
const HUT_BONE: u32 = 0xd8cfb8; // charms + the yard skull
const HUT_POT: u32 = 0x232326; // cauldron iron
const HUT_SHROOM: u32 = 0x9a4a68; // bog-violet toadstools
const HUT_SMOKE: u32 = 0x9a978e;
const WITCH_GREEN: Color = Color::srgb(0.49, 1.0, 0.6);

/// Cauldron yard spot (model-local) — the wisps orbit above it.
const CAULDRON: Vec3 = Vec3::new(1.75, 0.0, 1.3);

pub fn witch_hut() -> LandmarkModel {
    let mut parts: Vec<Mesh> = Vec::new();

    // ── Stilts: six crooked posts + two diagonal braces, feet splayed in the mire.
    let feet: [(f32, f32, f32, f32); 6] = [
        (-1.0, -0.85, 0.1, 0.3),
        (0.95, -0.9, -0.08, 1.2),
        (-1.05, 0.8, 0.12, 2.4),
        (1.0, 0.85, -0.1, 3.6),
        (0.0, -0.95, 0.06, 4.2),
        (0.05, 0.9, -0.07, 5.1),
    ];
    for &(x, z, tilt, yaw) in &feet {
        parts.push(tinted(limb(0.105, 1.35, 5, tilt, yaw, Vec3::new(x, -0.05, z)), lin(HUT_STILT)));
        parts.push(facet_at(0.16, Vec3::new(x, 0.05, z), 0.4, HUT_PLANK_DK));
    }
    // Cross-braces actually spanning stilt to stilt (a first pass had free-floating poles).
    parts.push(box_rot(2.2, 0.07, 0.07, Vec3::new(0.0, 0.62, -0.88), Quat::from_rotation_z(0.46), HUT_STILT));
    parts.push(box_rot(0.07, 0.07, 1.9, Vec3::new(0.98, 0.62, 0.0), Quat::from_rotation_x(-0.44), HUT_STILT));

    // ── Platform: plank deck with a slight drunken cant; a rickety ladder up the front.
    let cant = Quat::from_rotation_z(0.035) * Quat::from_rotation_x(-0.02);
    parts.push(box_rot(2.6, 0.13, 2.2, Vec3::new(0.0, 1.32, 0.0), cant, HUT_PLANK_DK));
    for i in 0..5 {
        parts.push(box_rot(
            2.5,
            0.035,
            0.34,
            Vec3::new(0.0, 1.41, -0.85 + i as f32 * 0.42),
            cant,
            if i % 2 == 0 { HUT_PLANK } else { HUT_PLANK_DK },
        ));
    }
    for &x in &[-0.28_f32, 0.28] {
        parts.push(tinted(limb(0.045, 1.55, 4, 0.32, 0.0, Vec3::new(x, 0.0, 1.55)), lin(HUT_STILT)));
    }
    for r in 0..4 {
        let t = 0.25 + r as f32 * 0.35;
        parts.push(tbox(0.62, 0.05, 0.07, Vec3::new(0.0, t, 1.55 - t * 0.33), HUT_PLANK));
    }

    // ── Cabin: a crooked box, corner posts, and a door that never opens.
    let lean = Quat::from_rotation_z(0.05) * Quat::from_rotation_x(-0.025);
    parts.push(box_rot(1.9, 1.5, 1.6, Vec3::new(-0.05, 2.15, -0.1), lean, HUT_PLANK));
    for i in 0..4 {
        parts.push(box_rot(
            1.94,
            0.045,
            1.64,
            Vec3::new(-0.05, 1.62 + i as f32 * 0.38, -0.1),
            lean,
            HUT_PLANK_DK,
        ));
    }
    for &(x, z) in &[(-1.0_f32, 0.68_f32), (0.9, 0.68), (-1.0, -0.88), (0.9, -0.88)] {
        parts.push(box_rot(0.14, 1.56, 0.14, Vec3::new(x, 2.15, z), lean, HUT_PLANK_DK));
    }
    // Door (+Z) with a crooked lintel and a bone fetish nailed above it.
    parts.push(box_rot(0.5, 0.95, 0.1, Vec3::new(0.32, 1.95, 0.72), lean, 0x1c1712));
    parts.push(box_rot(0.6, 0.1, 0.12, Vec3::new(0.32, 2.46, 0.72), lean * Quat::from_rotation_z(0.08), HUT_STILT));
    parts.push(tbox(0.05, 0.22, 0.05, Vec3::new(0.32, 2.62, 0.78), HUT_BONE));

    // ── Roof: two sagging oversized slabs, moss saddles, a crooked stone chimney.
    parts.push(box_rot(1.55, 0.09, 2.35, Vec3::new(-0.72, 3.28, -0.1), Quat::from_rotation_z(0.62), HUT_ROOF));
    parts.push(box_rot(1.5, 0.09, 2.3, Vec3::new(0.58, 3.32, -0.1), Quat::from_rotation_z(-0.55), HUT_ROOF_DK));
    parts.push(box_rot(0.22, 0.14, 2.4, Vec3::new(-0.06, 3.66, -0.1), Quat::from_rotation_z(0.03), HUT_ROOF_DK));
    for &(x, y, z, r) in &[
        (-0.9_f32, 3.15_f32, 0.5_f32, 0.3_f32),
        (0.75, 3.2, -0.6, 0.26),
        (-0.5, 3.45, -0.7, 0.22),
        (0.2, 3.6, 0.3, 0.2),
    ] {
        parts.push(ball(r, Vec3::new(x, y, z), 0.45, HUT_MOSS));
    }
    for c in 0..4 {
        let t = c as f32;
        parts.push(box_rot(
            0.46 - t * 0.05,
            0.34,
            0.46 - t * 0.05,
            Vec3::new(-0.78 + t * 0.05, 3.0 + t * 0.34, -0.72),
            Quat::from_rotation_y(t * 0.22) * Quat::from_rotation_z(0.04),
            if c % 2 == 0 { HUT_STONE } else { HUT_PLANK_DK },
        ));
    }

    // ── Yard: cauldron on a stone fire-ring, skull stake, toadstools, firewood.
    parts.push(ball(0.42, CAULDRON + yv(0.52), 0.85, HUT_POT));
    parts.push(frustum(0.34, 0.42, 0.14, CAULDRON + yv(0.86), 8, HUT_POT));
    for i in 0..3 {
        let a = i as f32 * TAU / 3.0 + 0.5;
        parts.push(tinted(
            limb(0.035, 0.42, 4, 0.5, a, CAULDRON + Vec3::new(a.cos() * 0.3, 0.0, a.sin() * 0.3)),
            lin(HUT_STILT),
        ));
    }
    for i in 0..6 {
        let a = i as f32 * TAU / 6.0 + 0.2;
        parts.push(facet_at(0.13, CAULDRON + Vec3::new(a.cos() * 0.5, 0.05, a.sin() * 0.5), 0.5, HUT_STONE));
    }
    parts.push(tinted(limb(0.04, 0.95, 4, 0.08, 0.0, Vec3::new(-1.7, 0.0, 1.5)), lin(HUT_STILT)));
    parts.push(ball(0.14, Vec3::new(-1.7, 1.02, 1.52), 0.9, HUT_BONE));
    parts.push(tbox(0.1, 0.08, 0.1, Vec3::new(-1.7, 0.93, 1.6), HUT_BONE));
    for &(x, z, s) in &[(-1.35_f32, -0.5_f32, 1.0_f32), (1.3, 0.4, 0.75), (-0.6, 1.25, 0.6)] {
        for k in 0..3 {
            let a = k as f32 * 2.1 + x;
            let (mx, mz) = (x + a.cos() * 0.14 * s, z + a.sin() * 0.14 * s);
            let h = (0.14 + k as f32 * 0.06) * s;
            parts.push(cyl(0.042 * s, h, Vec3::new(mx, h * 0.5, mz), 5, HUT_BONE));
            parts.push(ball(0.095 * s, Vec3::new(mx, h + 0.02, mz), 0.55, HUT_SHROOM));
        }
    }
    for k in 0..4 {
        parts.push(cyl_rot(
            0.07,
            0.75,
            5,
            Vec3::new(0.55 + (k % 2) as f32 * 0.16, 0.08 + (k / 2) as f32 * 0.13, -0.6),
            Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_y(0.15 * k as f32),
            HUT_STILT,
        ));
    }
    // The raven that is definitely just a raven.
    parts.push(ball(0.085, Vec3::new(-0.06, 3.78, 0.75), 0.9, 0x14161a));
    parts.push(ball(0.06, Vec3::new(-0.06, 3.87, 0.82), 0.9, 0x14161a));
    parts.push(cone_at(0.02, 0.07, Vec3::new(-0.06, 3.87, 0.9), 4, 0x8a8a3a));

    let base = mottle(flat_shaded(merged(parts)), 0.46);

    // ── Animated parts ──────────────────────────────────────────────────────────
    let mut anim: Vec<AnimPart> = Vec::new();

    // Two green-lit windows (front + -X side), breathing out of phase.
    let win = |w: f32, h: f32| flat_shaded(tbox(w, h, 0.1, Vec3::ZERO, 0xffffff));
    anim.push(AnimPart::glowing(
        win(0.3, 0.3),
        Transform::from_xyz(-0.55, 2.3, 0.71).with_rotation(Quat::from_rotation_z(0.05)),
        Fx::None,
        Glow { color: WITCH_GREEN, strength: 5.0, pulse: Some((0.7, 0.35, 0.0)) },
    ));
    anim.push(AnimPart::glowing(
        win(0.26, 0.34),
        Transform::from_xyz(-1.06, 2.2, -0.25).with_rotation(Quat::from_rotation_y(FRAC_PI_2)),
        Fx::None,
        Glow { color: WITCH_GREEN, strength: 5.0, pulse: Some((0.55, 0.35, 1.9)) },
    ));

    // The brew surface — a glowing disc riding just under the cauldron rim.
    anim.push(AnimPart::glowing(
        flat_shaded(tinted(Cylinder::new(0.31, 0.05).mesh().resolution(10).build(), lin(0xffffff))),
        Transform::from_translation(CAULDRON + yv(0.94)),
        Fx::None,
        Glow { color: WITCH_GREEN, strength: 7.0, pulse: Some((1.6, 0.3, 0.7)) },
    ));
    // Three wisps circling above the brew.
    for (i, &(r, h, rate)) in [(0.3_f32, 1.15_f32, 1.1_f32), (0.42, 1.4, -0.8), (0.24, 1.65, 1.5)].iter().enumerate() {
        anim.push(AnimPart::glowing(
            flat_shaded(tinted(ico(0.05 + i as f32 * 0.012, 0), lin(0xffffff))),
            Transform::from_translation(CAULDRON + Vec3::new(r, h, 0.0)),
            Fx::Orbit {
                centre: CAULDRON + yv(h),
                rate,
                bob_amp: 0.1,
                bob_freq: 1.3 + i as f32 * 0.5,
                phase: i as f32 * 2.1,
            },
            Glow { color: Color::srgb(0.62, 1.0, 0.69), strength: 8.0, pulse: None },
        ));
    }

    // Charms swinging from the eaves: bone-and-twig fetishes on cords.
    let charm = |long: f32| {
        flat_shaded(merged(vec![
            tbox(0.022, long, 0.022, yv(-long * 0.5), HUT_STILT),
            tbox(0.05, 0.16, 0.05, yv(-long - 0.06), HUT_BONE),
            tbox(0.14, 0.04, 0.04, yv(-long + 0.08), HUT_BONE),
        ]))
    };
    for (i, &(x, z, long)) in
        [(-1.35_f32, 0.95_f32, 0.5_f32), (1.25, 0.9, 0.38), (1.3, -1.0, 0.55), (-0.2, 1.05, 0.42)].iter().enumerate()
    {
        anim.push(AnimPart::solid(
            charm(long),
            Transform::from_xyz(x, 3.05, z),
            Fx::Sway {
                axis: if i % 2 == 0 { Vec3::X } else { Vec3::Z },
                amp: 0.16,
                freq: 1.1 + i as f32 * 0.23,
                phase: i as f32 * 1.4,
            },
        ));
    }
    // The door lantern: an amber counterpoint to all that green.
    anim.push(AnimPart::glowing(
        flat_shaded(merged(vec![
            tbox(0.02, 0.18, 0.02, yv(-0.09), HUT_STILT),
            tbox(0.11, 0.15, 0.11, yv(-0.26), 0xffffff),
        ])),
        Transform::from_xyz(0.78, 2.6, 0.78),
        Fx::Sway { axis: Vec3::Z, amp: 0.12, freq: 1.5, phase: 0.6 },
        Glow { color: Color::srgb_u8(0xff, 0xb4, 0x5e), strength: 4.0, pulse: Some((1.3, 0.25, 0.3)) },
    ));

    // Chimney smoke: translucent puffs looping up out of the flue mouth — phases pack them
    // close so the column reads connected, not as beads on a string.
    for k in 0..4 {
        let mut m = AnimPart::solid(
            flat_shaded(tinted(ico(0.21, 0).scaled_by(Vec3::new(1.0, 0.78, 1.0)), lin(HUT_SMOKE))),
            Transform::from_xyz(-0.66, 4.02, -0.72),
            Fx::Rise { rate: 0.36, range: 1.55, drift: Vec2::new(0.34, 0.16), grow: 1.2, phase: k as f32 * 0.39 },
        );
        m.alpha = 0.32;
        anim.push(m);
    }

    LandmarkModel { base, parts: anim }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  THE FROZEN SPIRE — snow. A great shard of ancient ice erupting from a frost
//  tor inside a ring of burst floe-slabs; a cyan heart pulses in its throat and
//  six splinters of ice levitate in slow interleaved orbits around it.
// ═══════════════════════════════════════════════════════════════════════════════

const ICE_BODY: u32 = 0xa9d2f0;
const ICE_RIME: u32 = 0xc4e3f6;
const ICE_PALE: u32 = 0xe6f4fc;
const ICE_DEEP: u32 = 0x5f93cc;
const ICE_ABYSS: u32 = 0x3c6ca8; // shadowed core faces
const FROST_ROCK: u32 = 0x66727f;
const SNOW_CAP: u32 = 0xe8f2fa;
const SNOW_SHADE: u32 = 0xc8dff0;

pub fn frozen_spire() -> LandmarkModel {
    let mut parts: Vec<Mesh> = Vec::new();

    // ── Frost tor footing + snow banked against it.
    parts.push(chunk_at(0.55, yv(0.28), Vec3::new(1.5, 0.75, 1.3), 0.2, 0.08, 1, FROST_ROCK));
    parts.push(chunk_at(0.4, Vec3::new(0.55, 0.5, -0.3), Vec3::new(1.2, 0.9, 1.0), 0.9, -0.15, 0, FROST_ROCK));
    parts.push(chunk_at(0.34, Vec3::new(-0.6, 0.42, 0.35), Vec3::new(1.1, 0.85, 1.05), -0.6, 0.12, 0, FROST_ROCK));
    for &(x, z, r) in &[(0.85_f32, 0.55_f32, 0.5_f32), (-0.8, -0.5, 0.42), (0.1, -0.85, 0.38), (-0.45, 0.9, 0.4)] {
        parts.push(ball(r, Vec3::new(x, r * 0.35, z), 0.4, SNOW_CAP));
    }
    parts.push(ball(0.62, yv(0.5), 0.3, SNOW_SHADE));

    // ── The spire: a BLADE-like central shard (asymmetric cross-section, yawed) + flanking
    // splinters, all one crystal. A tilted tip splinter + mid-height protrusions break the
    // smooth "rocket" outline into something wind-carved.
    parts.push(chunk_at(0.62, yv(2.9), Vec3::new(1.15, 4.6, 0.72), 0.45, 0.03, 1, ICE_BODY));
    parts.push(chunk_at(0.26, Vec3::new(-0.1, 5.5, -0.08), Vec3::new(0.7, 2.2, 0.6), 1.1, -0.16, 0, ICE_PALE));
    parts.push(chunk_at(0.34, Vec3::new(0.42, 3.9, -0.28), Vec3::new(1.0, 1.6, 0.8), 0.9, 0.22, 0, ICE_BODY));
    parts.push(chunk_at(0.3, Vec3::new(-0.5, 3.1, 0.3), Vec3::new(1.0, 1.4, 0.85), -0.5, -0.18, 0, ICE_DEEP));
    // A darker core shard inside/behind reads as depth through the flat shading.
    parts.push(chunk_at(0.5, Vec3::new(-0.14, 2.4, -0.12), Vec3::new(0.9, 3.9, 0.8), 0.8, -0.04, 0, ICE_ABYSS));
    // Rime sliver hugging the sunward edge.
    parts.push(chunk_at(0.3, Vec3::new(0.4, 3.3, 0.22), Vec3::new(0.55, 3.4, 0.5), 0.35, 0.05, 0, ICE_PALE));
    parts.push(chunk_at(0.42, Vec3::new(0.95, 1.7, -0.4), Vec3::new(0.85, 2.6, 0.75), 0.5, -0.14, 1, ICE_DEEP));
    parts.push(chunk_at(0.36, Vec3::new(-0.9, 1.35, 0.5), Vec3::new(0.8, 2.1, 0.7), -0.7, 0.12, 1, ICE_BODY));
    parts.push(chunk_at(0.22, Vec3::new(-0.55, 0.9, -0.75), Vec3::new(0.75, 1.5, 0.7), 1.6, 0.08, 0, ICE_RIME));

    // ── Burst ring: floe slabs heaved up and outward, as if the spire punched through.
    for i in 0..8 {
        let a = i as f32 * TAU / 8.0 + 0.27;
        let r = 1.75 + hash3(a, 0.0, 1.0) * 0.5;
        let (sx, sz) = (a.cos() * r, a.sin() * r);
        let h = 0.5 + hash3(a, 2.0, 0.0) * 0.55;
        let c = [ICE_BODY, ICE_DEEP, ICE_RIME][i % 3];
        parts.push(slab_at(
            0.42,
            h,
            0.16,
            Vec3::new(sx, slab_ground(0.42, h, 0.55) - 0.15, sz),
            a + FRAC_PI_2,
            0.55,
            c,
        ));
    }
    // Shed chips + a couple of leaning icicle spears in the snow.
    for &(x, z, r) in &[(1.3_f32, 1.15_f32, 0.12_f32), (-1.2, -1.05, 0.1), (1.5, -0.8, 0.09), (-1.5, 0.7, 0.11)] {
        parts.push(facet_at(r, Vec3::new(x, r * 0.5, z), 0.6, ICE_PALE));
    }
    for &(x, z, h, t) in &[(0.9_f32, -1.3_f32, 0.75_f32, 0.35_f32), (-1.05, 1.25, 0.6, -0.3)] {
        parts.push(tinted(
            Cone { radius: 0.09, height: h }
                .mesh()
                .resolution(5)
                .build()
                .rotated_by(Quat::from_rotation_z(t))
                .translated_by(Vec3::new(x, h * 0.4, z)),
            lin(ICE_RIME),
        ));
    }

    let base = mottle(flat_shaded(merged(parts)), 0.3);

    // ── Animated parts ──────────────────────────────────────────────────────────
    let mut anim: Vec<AnimPart> = Vec::new();

    // The heart: a cold light lodged in the spire's throat, slow as a sleeping pulse.
    // Proud of the blade face (cross-section half-depth ≈0.45 at this height) so it reads
    // as an embedded orb, not a decal.
    anim.push(AnimPart::glowing(
        flat_shaded(tinted(ico(0.3, 0), lin(0xffffff))),
        Transform::from_xyz(0.38, 2.35, 0.52),
        Fx::None,
        Glow { color: Color::srgb(0.35, 0.9, 1.0), strength: 9.0, pulse: Some((0.8, 0.4, 0.0)) },
    ));

    // Six levitating splinters in interleaved orbits — the spire's quiet gravity.
    for i in 0..6 {
        let fi = i as f32;
        let r = 1.45 + (i % 3) as f32 * 0.38;
        let h = 2.0 + fi * 0.46;
        let rate = if i % 2 == 0 { 0.28 + fi * 0.03 } else { -(0.34 + fi * 0.025) };
        let stretch = 1.9 + hash3(fi, 5.0, 2.0) * 1.1;
        anim.push(AnimPart::glowing(
            flat_shaded(tinted(
                ico(0.14, 0).scaled_by(Vec3::new(0.85, stretch, 0.85)).rotated_by(Quat::from_rotation_z(0.18)),
                lin(0xffffff),
            )),
            Transform::from_xyz(r, h, 0.0),
            Fx::Orbit { centre: yv(h), rate, bob_amp: 0.14, bob_freq: 0.5 + fi * 0.11, phase: fi * 1.05 },
            Glow { color: Color::srgb(0.62, 0.86, 1.0), strength: 2.4, pulse: None },
        ));
    }

    LandmarkModel { base, parts: anim }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  THE SUNKEN PYRAMID — desert. A grand six-tier ziggurat half-swallowed by a
//  dune, a processional stair to a four-pillar summit shrine, a toppled obelisk,
//  a colossus head drowning in the sand — and above it all, a gilded sun-disc
//  relic spinning slowly on nothing at all.
// ═══════════════════════════════════════════════════════════════════════════════

const PYR_BODY: u32 = 0xc9a96b; // sandstone courses
const PYR_LT: u32 = 0xe2c98f; // sunlit rims
const PYR_DK: u32 = 0xa8854c;
const PYR_RUST: u32 = 0x9a6a40; // iron-stained courses / obelisks
const PYR_PALE: u32 = 0xd7bd8d; // the colossus head (finer stone)
const PYR_GOLD: u32 = 0xd4a63c; // the altar block
const PYR_DOOR: u32 = 0x2c2418;
const SAND: u32 = 0xd9c08a;
const SAND_DK: u32 = 0xbb9c5e;
const SUN_GOLD: Color = Color::srgb(1.0, 0.78, 0.3);

/// Half-width of tier `i` of 6 (base 2.35 → top 0.62) and the tier height.
fn pyr_w(i: usize) -> f32 {
    2.35 + (0.62 - 2.35) * (i as f32 / 5.0)
}
const PYR_TH: f32 = 0.6;

pub fn sunken_pyramid() -> LandmarkModel {
    let mut parts: Vec<Mesh> = Vec::new();

    // ── Six clean-built tiers with sunlit rims, corner damage and banded colour.
    let bands = [PYR_RUST, PYR_BODY, PYR_DK, PYR_BODY, PYR_RUST, PYR_DK];
    for i in 0..6 {
        let w = pyr_w(i);
        let y = i as f32 * PYR_TH;
        let jig = Quat::from_rotation_y((hash3(i as f32, 3.0, 7.0) - 0.5) * 0.05);
        // Sink each tier 0.12 into the one below (its top stays at y+PYR_TH) so stacked boxes
        // never share a coplanar horizontal face — that coincidence (tier-top == next-tier-bottom)
        // was the landmark's z-fighting flicker.
        let th = PYR_TH + 0.12;
        parts.push(box_rot(w * 2.0, th, w * 2.0, yv(y + PYR_TH - th * 0.5), jig, bands[i]));
        if i % 2 == 0 {
            // Bright rim lifted a hair PROUD of the tier top instead of flush — flush = coplanar = flicker.
            parts.push(box_rot(w * 2.0 + 0.07, 0.06, w * 2.0 + 0.07, yv(y + PYR_TH + 0.01), jig, PYR_LT));
        }
        // One chipped corner per tier (hash-picked) — weather bites the arrises first.
        let hh = hash3(i as f32, 1.0, 11.0);
        let cx = if hh > 0.5 { w } else { -w };
        let cz = if hash3(i as f32, 1.0, 23.0) > 0.5 { w } else { -w };
        parts.push(chunk_at(
            0.12 + hh * 0.08,
            Vec3::new(cx, y + PYR_TH * (0.3 + hh * 0.5), cz),
            Vec3::new(1.0, 0.8, 1.0),
            hh * TAU,
            0.2,
            0,
            PYR_DK,
        ));
    }

    // ── Processional stair up the +Z face (no stringers — a first pass floated two beams
    // over the treads; the clean stepped run reads better).
    for s in 0..12 {
        let y = s as f32 * 0.3;
        // The face slopes inward as the tiers shrink; keep each tread proud of it.
        let w_here = 2.35 + (0.62 - 2.35) * (y / 3.6);
        // Overlap treads vertically (top stays y+0.3) so neighbouring steps don't share a plane.
        parts.push(tbox(0.95 - y * 0.04, 0.4, 0.62, Vec3::new(0.0, y + 0.10, w_here + 0.14), PYR_LT));
    }

    // ── Summit shrine: floor slab, four pillars, two lintels, the gold altar.
    let top_y = 6.0 * PYR_TH;
    // Shrine floor sunk into the top tier (its top stays top_y+0.12) — base no longer coplanar with tier 5.
    parts.push(tbox(1.5, 0.2, 1.5, yv(top_y + 0.02), PYR_LT));
    for &(x, z) in &[(-0.52_f32, -0.52_f32), (0.52, -0.52), (-0.52, 0.52), (0.52, 0.52)] {
        parts.push(cyl(0.11, 0.78, Vec3::new(x, top_y + 0.51, z), 6, PYR_BODY));
        parts.push(tbox(0.3, 0.1, 0.3, Vec3::new(x, top_y + 0.94, z), PYR_DK));
    }
    for &z in &[-0.52_f32, 0.52] {
        parts.push(tbox(1.4, 0.15, 0.34, Vec3::new(0.0, top_y + 1.06, z), PYR_RUST));
    }
    parts.push(tbox(0.44, 0.34, 0.44, yv(top_y + 0.29), PYR_GOLD));
    // Dark doorway recess into the top tier's +Z face.
    parts.push(tbox(0.5, 0.42, 0.12, Vec3::new(0.0, top_y - 0.28, pyr_w(5) + 0.01), PYR_DOOR));

    // ── The dune that is eating the monument (the -X/-Z quarter drowns).
    for &(x, z, r, sq) in &[
        (-2.4_f32, -1.4_f32, 1.5_f32, 0.42_f32),
        (-1.5, -2.4, 1.3, 0.4),
        (-2.8, 0.2, 1.1, 0.34),
        (-0.2, -2.75, 1.05, 0.32),
        (-3.3, -1.2, 0.85, 0.3),
    ] {
        parts.push(ball(r, Vec3::new(x, -r * sq * 0.25, z), sq, SAND));
    }
    for &(x, z, yaw) in &[(-2.2_f32, -2.1_f32, 0.5_f32), (-3.0, -0.4, -0.6), (-1.0, -2.7, 0.9)] {
        parts.push(slab_at(1.0, 0.12, 0.6, Vec3::new(x, 0.07, z), yaw, 0.05, SAND_DK));
    }

    // ── The colossus head, buried to the chin in the +X sand, face turned toward the stair.
    let head = Vec3::new(3.0, 0.38, 1.9);
    let head_rot = Quat::from_rotation_y(-2.25) * Quat::from_rotation_z(0.12);
    parts.push(tinted(
        ico(0.55, 1).scaled_by(Vec3::new(0.92, 1.0, 0.98)).rotated_by(head_rot).translated_by(head),
        lin(PYR_PALE),
    ));
    // Nemes headdress: crown slab + two side wings hugging the cranium.
    parts.push(box_rot(0.78, 0.16, 0.58, head + head_rot * Vec3::new(0.0, 0.42, -0.05), head_rot, PYR_RUST));
    parts.push(box_rot(0.13, 0.52, 0.44, head + head_rot * Vec3::new(-0.5, 0.06, -0.06), head_rot, PYR_RUST));
    parts.push(box_rot(0.13, 0.52, 0.44, head + head_rot * Vec3::new(0.5, 0.06, -0.06), head_rot, PYR_RUST));
    // The face (+Z of the head frame): brow ledge, broken nose, lip slab.
    parts.push(box_rot(0.5, 0.09, 0.13, head + head_rot * Vec3::new(0.0, 0.17, 0.46), head_rot, PYR_PALE));
    parts.push(box_rot(0.13, 0.24, 0.15, head + head_rot * Vec3::new(0.0, 0.0, 0.52), head_rot * Quat::from_rotation_x(0.14), PYR_DK));
    parts.push(box_rot(0.3, 0.07, 0.1, head + head_rot * Vec3::new(0.0, -0.2, 0.48), head_rot, PYR_PALE));
    // Sand lapping the chin.
    parts.push(ball(0.72, Vec3::new(head.x, 0.02, head.z), 0.26, SAND));

    // ── Obelisks at the stair foot: one standing, one toppled mid-fall centuries ago.
    let ob = Vec3::new(1.15, 0.0, 3.15);
    parts.push(frustum(0.17, 0.11, 1.45, ob + yv(0.75), 4, PYR_RUST));
    parts.push(cone_at(0.12, 0.3, ob + yv(1.62), 4, PYR_GOLD));
    parts.push(tbox(0.5, 0.18, 0.5, ob + yv(0.06), PYR_DK));
    let ob2 = Vec3::new(-1.25, 0.0, 3.0);
    parts.push(tbox(0.48, 0.16, 0.48, ob2 + yv(0.05), PYR_DK));
    parts.push(frustum(0.17, 0.14, 0.5, ob2 + yv(0.35), 4, PYR_RUST));
    parts.push(box_rot(
        0.26,
        1.15,
        0.26,
        ob2 + Vec3::new(-0.55, 0.2, -0.4),
        Quat::from_rotation_y(0.7) * Quat::from_rotation_z(1.35),
        PYR_RUST,
    ));
    // Fallen column drums + scattered blocks round the base.
    parts.push(cyl_rot(0.2, 0.55, 7, Vec3::new(2.6, 0.2, -1.4), Quat::from_rotation_z(FRAC_PI_2), PYR_BODY));
    parts.push(cyl_rot(0.2, 0.5, 7, Vec3::new(3.15, 0.2, -1.15), Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_y(0.5), PYR_DK));
    parts.push(slab_at(0.4, 0.28, 0.34, Vec3::new(-2.3, 0.2, 2.5), 0.8, 0.15, PYR_BODY));

    let base = mottle(flat_shaded(merged(parts)), 0.5);

    // ── Animated parts ──────────────────────────────────────────────────────────
    let mut anim: Vec<AnimPart> = Vec::new();

    // The sun-disc relic: a gilded ring + core coin, hovering over the shrine,
    // turning like a coin that never settles.
    let mut disc: Vec<Mesh> = Vec::new();
    disc.push(tinted(
        Torus { minor_radius: 0.055, major_radius: 0.46 }
            .mesh()
            .minor_resolution(4)
            .major_resolution(12)
            .build()
            .rotated_by(Quat::from_rotation_x(FRAC_PI_2)),
        lin(0xffffff),
    ));
    disc.push(tinted(
        Cylinder::new(0.28, 0.045).mesh().resolution(12).build().rotated_by(Quat::from_rotation_x(FRAC_PI_2)),
        lin(0xffffff),
    ));
    for i in 0..8 {
        let a = i as f32 * TAU / 8.0;
        disc.push(tinted(
            box_at(0.05, 0.16, 0.03, Vec3::new(a.cos() * 0.62, a.sin() * 0.62, 0.0))
                .rotated_by(Quat::from_rotation_z(a + FRAC_PI_2)),
            lin(0xffffff),
        ));
    }
    anim.push(AnimPart::glowing(
        flat_shaded(merged(disc)),
        Transform::from_xyz(0.0, 5.15, 0.0),
        Fx::Hover { bob_amp: 0.12, bob_freq: 0.55, spin_rate: 0.65, phase: 0.0 },
        Glow { color: SUN_GOLD, strength: 5.0, pulse: Some((0.9, 0.22, 0.0)) },
    ));

    // Gold glyph strips flanking the stair on the bottom tier, smouldering out of phase.
    for (i, &x) in [-1.05_f32, 1.05].iter().enumerate() {
        let glyphs = merged(
            (0..4)
                .map(|g| tbox(0.1, 0.09 + (g % 2) as f32 * 0.05, 0.05, Vec3::new(0.0, g as f32 * 0.17, 0.0), 0xffffff))
                .collect(),
        );
        anim.push(AnimPart::glowing(
            flat_shaded(glyphs),
            Transform::from_xyz(x, 0.28, 2.36),
            Fx::None,
            Glow { color: SUN_GOLD, strength: 2.6, pulse: Some((0.5, 0.4, i as f32 * 2.4)) },
        ));
    }

    LandmarkModel { base, parts: anim }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  THE STANDING STONES — rocky. A full walkable circle: six rune-cut monoliths
//  (two forming a gate arch), a seventh fallen and broken, a low altar at the
//  centre — and above the altar, a cracked keystone that never quite lands.
// ═══════════════════════════════════════════════════════════════════════════════

const MEG_A: u32 = 0x9a9aa3; // cool light grey
const MEG_B: u32 = 0x8a8f98; // mid grey
const MEG_C: u32 = 0xaab0b2; // pale warm grey
const MEG_DK: u32 = 0x6b6862; // shadowed feet / underside
const MEG_PALE: u32 = 0xb3a692; // sun-bleached caps
const LICHEN_ORANGE: u32 = 0xc88a3a;
const LICHEN_SAGE: u32 = 0x9aa56b;
const RUNE_AMBER: Color = Color::srgb(1.0, 0.72, 0.28);

/// Ring radius and the six standing stones as `(angle, height)`. The first two are the
/// gate pair (+Z side, where the road spur arrives); the gap near angle ~0.2 is where
/// the fallen seventh lies. KEEP IN SYNC with [`stone_blockers`].
const RING_R: f32 = 2.9;
const STONES: [(f32, f32); 6] =
    [(1.19, 3.25), (1.95, 3.25), (2.85, 2.6), (3.75, 2.4), (4.65, 2.8), (5.5, 2.55)];
/// Where the broken seventh monolith lies (angle, radius).
const FALLEN: (f32, f32) = (0.22, 3.35);

/// Blocker boxes (model-local `(dx, dz, hw, hd)`) — one per stone + the altar + the fallen
/// monolith, so the circle interior stays WALKABLE (the rune trial plays out inside it).
pub fn stone_blockers() -> Vec<(f32, f32, f32, f32)> {
    let mut v: Vec<(f32, f32, f32, f32)> =
        STONES.iter().map(|&(a, _)| (a.cos() * RING_R, a.sin() * RING_R, 0.52, 0.4)).collect();
    v.push((0.0, 0.0, 0.62, 0.62)); // altar
    v.push((FALLEN.0.cos() * FALLEN.1, FALLEN.0.sin() * FALLEN.1, 1.0, 0.5));
    v
}

pub fn standing_stones() -> LandmarkModel {
    let mut parts: Vec<Mesh> = Vec::new();

    // ── The six monoliths: worked, tapered, each leaning its own way.
    for (i, &(a, h)) in STONES.iter().enumerate() {
        let (x, z) = (a.cos() * RING_R, a.sin() * RING_R);
        let yaw = -(a + FRAC_PI_2); // broad face toward the circle centre
        let lean = (hash3(a, 9.0, 1.0) - 0.5) * 0.09;
        let body = [MEG_A, MEG_B, MEG_C][i % 3];
        let spin = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(lean);
        // Foot rubble + the shadowed base wedge.
        parts.push(facet_at(0.34, Vec3::new(x, 0.14, z), 0.5, MEG_DK));
        // Two worked courses — strongly tapered, the upper twisted a touch off true so the
        // silhouette reads hand-raised, not machined.
        parts.push(box_rot(0.74, h * 0.6, 0.48, Vec3::new(x, h * 0.3, z), spin, body));
        parts.push(box_rot(
            0.52,
            h * 0.48,
            0.36,
            Vec3::new(x + lean * h * 0.4, h * 0.72, z),
            spin * Quat::from_rotation_y(0.09) * Quat::from_rotation_z(lean * 0.7),
            body,
        ));
        // Bleached cap chunk + two weather bites out of the arrises.
        parts.push(chunk_at(0.3, Vec3::new(x, h * 0.97, z), Vec3::new(1.15, 0.55, 0.9), a, 0.1, 0, MEG_PALE));
        for k in 0..2 {
            let hb = hash3(a, k as f32, 31.0);
            let side = if hb > 0.5 { 0.36 } else { -0.36 };
            parts.push(chunk_at(
                0.13 + hb * 0.07,
                Vec3::new(x + a.cos() * side, h * (0.25 + hb * 0.45), z + a.sin() * side),
                Vec3::new(1.0, 0.75, 0.9),
                hb * TAU,
                0.15,
                0,
                MEG_DK,
            ));
        }
        // Lichen on the weather side.
        parts.push(lichen_at(0.15, Vec3::new(x + a.cos() * 0.36, h * 0.35, z + a.sin() * 0.36), if i % 2 == 0 { LICHEN_ORANGE } else { LICHEN_SAGE }));
    }

    // ── The gate lintel bridging the first pair (+Z, facing the road spur).
    let (a1, h1) = STONES[0];
    let (a2, _) = STONES[1];
    let mid = (a1 + a2) * 0.5;
    let lx = (a1.cos() + a2.cos()) * 0.5 * RING_R;
    let lz = (a1.sin() + a2.sin()) * 0.5 * RING_R;
    let lintel_yaw = -(mid + FRAC_PI_2);
    parts.push(box_rot(
        2.5,
        0.44,
        0.55,
        Vec3::new(lx, h1 + 0.2, lz),
        Quat::from_rotation_y(lintel_yaw) * Quat::from_rotation_z(0.015),
        MEG_B,
    ));
    parts.push(box_rot(0.9, 0.3, 0.42, Vec3::new(lx, h1 + 0.5, lz), Quat::from_rotation_y(lintel_yaw), MEG_PALE));

    // ── The fallen seventh, snapped in two where it landed.
    let (fx, fz) = (FALLEN.0.cos() * FALLEN.1, FALLEN.0.sin() * FALLEN.1);
    let f_yaw = -(FALLEN.0 + FRAC_PI_2);
    parts.push(box_rot(
        0.62,
        1.5,
        0.4,
        Vec3::new(fx - 0.35, 0.3, fz - 0.2),
        Quat::from_rotation_y(f_yaw + 0.15) * Quat::from_rotation_z(FRAC_PI_2 * 0.94),
        MEG_B,
    ));
    parts.push(box_rot(
        0.58,
        0.9,
        0.38,
        Vec3::new(fx + 0.85, 0.26, fz + 0.35),
        Quat::from_rotation_y(f_yaw - 0.35) * Quat::from_rotation_z(FRAC_PI_2 * 1.06),
        MEG_A,
    ));
    parts.push(chunk_at(0.2, Vec3::new(fx + 0.25, 0.12, fz + 0.1), Vec3::new(1.0, 0.7, 1.0), 0.5, 0.0, 0, MEG_DK));

    // ── The altar: three squat legs, two stacked slabs, a dark offering bowl.
    for i in 0..3 {
        let a = i as f32 * TAU / 3.0 + 0.4;
        parts.push(tbox(0.3, 0.42, 0.3, Vec3::new(a.cos() * 0.42, 0.21, a.sin() * 0.42), MEG_DK));
    }
    parts.push(box_rot(1.3, 0.18, 1.0, yv(0.52), Quat::from_rotation_y(0.2), MEG_B));
    parts.push(box_rot(1.05, 0.14, 0.8, yv(0.68), Quat::from_rotation_y(0.32), MEG_PALE));
    parts.push(cyl(0.16, 0.1, yv(0.78), 8, MEG_DK));

    // ── Ring dressing: flat waystones between the monoliths, two cairns, talus.
    for i in 0..6 {
        let a = STONES[i].0 + 0.45;
        parts.push(facet_at(0.24, Vec3::new(a.cos() * RING_R * 0.97, 0.05, a.sin() * RING_R * 0.97), 0.2, MEG_DK));
    }
    for &(a, r) in &[(2.4_f32, 4.3_f32), (5.1, 4.1)] {
        let (cx, cz) = (a.cos() * r, a.sin() * r);
        for s in 0..3 {
            parts.push(flat_stone(
                0.5 - s as f32 * 0.12,
                0.13,
                0.4 - s as f32 * 0.08,
                Vec3::new(cx, 0.07 + s as f32 * 0.13, cz),
                a + s as f32 * 0.5,
                if s % 2 == 0 { MEG_B } else { MEG_C },
            ));
        }
    }
    for &(x, z, r) in &[(1.6_f32, -0.9_f32, 0.14_f32), (-1.8, 0.6, 0.12), (0.4, 1.9, 0.11), (-0.9, -1.7, 0.13)] {
        parts.push(facet_at(r, Vec3::new(x, r * 0.55, z), 0.6, MEG_DK));
    }

    let base = mottle(flat_shaded(merged(parts)), 0.58);

    // ── Animated parts ──────────────────────────────────────────────────────────
    let mut anim: Vec<AnimPart> = Vec::new();

    // Rune strips down each monolith's inner face, smouldering in a slow stagger. Pressed
    // half-proud INTO the face (course half-depth 0.24 → face at RING_R−0.24; glyph depth
    // 0.12 straddles it) so they read carved, not floating.
    for (i, &(a, h)) in STONES.iter().enumerate() {
        let inner = RING_R - 0.27;
        let (x, z) = (a.cos() * inner, a.sin() * inner);
        let yaw = -(a + FRAC_PI_2);
        // Same lean as the monolith's lower course (same hash) — the strip rides the face
        // instead of peeling off a leaning stone; kept low where the lean displaces least.
        let lean = (hash3(a, 9.0, 1.0) - 0.5) * 0.09;
        let glyphs = merged(
            (0..4)
                .map(|g| {
                    tbox(
                        0.1 + (g % 2) as f32 * 0.04,
                        0.1 + ((g + i) % 3) as f32 * 0.03,
                        0.12,
                        Vec3::new(0.0, g as f32 * 0.22, 0.0),
                        0xffffff,
                    )
                })
                .collect(),
        );
        anim.push(AnimPart::glowing(
            flat_shaded(glyphs),
            Transform::from_xyz(x, h * 0.22, z)
                .with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_z(lean)),
            Fx::None,
            Glow { color: RUNE_AMBER, strength: 3.4, pulse: Some((0.55, 0.4, i as f32 * 1.05)) },
        ));
    }

    // The keystone: a cracked block hovering over the altar…
    let keystone = flat_shaded(merged(vec![
        tinted(ico(0.4, 0).scaled_by(Vec3::new(1.0, 0.85, 0.9)), lin(MEG_B)),
        facet_at(0.15, Vec3::new(0.34, 0.16, 0.1), 0.8, MEG_PALE),
        facet_at(0.12, Vec3::new(-0.3, -0.14, -0.12), 0.8, MEG_A),
    ]));
    let hover = Fx::Hover { bob_amp: 0.15, bob_freq: 0.45, spin_rate: 0.3, phase: 0.0 };
    anim.push(AnimPart::solid(keystone, Transform::from_xyz(0.0, 2.3, 0.0), hover));
    // …and the amber light leaking from its cracks (same hover parameters → locked in step).
    let seams = flat_shaded(merged(vec![
        tinted(ico(0.07, 0).translated_by(Vec3::new(0.3, 0.08, 0.22)), lin(0xffffff)),
        tinted(ico(0.06, 0).translated_by(Vec3::new(-0.26, 0.2, -0.14)), lin(0xffffff)),
        tinted(ico(0.065, 0).translated_by(Vec3::new(0.05, -0.2, 0.3)), lin(0xffffff)),
    ]));
    anim.push(AnimPart::glowing(
        seams,
        Transform::from_xyz(0.0, 2.3, 0.0),
        Fx::Hover { bob_amp: 0.15, bob_freq: 0.45, spin_rate: 0.3, phase: 0.0 },
        Glow { color: RUNE_AMBER, strength: 6.0, pulse: Some((0.7, 0.35, 0.5)) },
    ));

    LandmarkModel { base, parts: anim }
}
