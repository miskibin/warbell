//! **Textured producer buildings + build plots for the town economy.** Built from the SAME
//! procedural materials as the keep ([`crate::castle::VillageMats`] / `M`) but each trade now
//! has its OWN structure — no more one-cottage-fits-all. Each builder returns a list of
//! `(Mesh, M)` parts (a mesh + its material slot); `town::spawn_building` bakes them at the
//! plot and assigns the shared textured materials.
//!
//! Layout contract (per `town::spawn_building`): the main structure sits on the **−X half**
//! of the plot (a collision box covers it), the working yard spreads over the **+X half**
//! (walkable — workers stand there). Bases at y = 0.
//!
//! - **Farm** — a thatched timber barn beside a tilled, fenced field of crop rows, watched
//!   by a scarecrow.
//! - **Woodcutter** — an open-sided saw shed over a sawpit, with the log stack, sawhorse and
//!   chopping stump out in the yard.
//! - **Stone Miner** — a timber pit-head frame with windlass + bucket over a dark shaft
//!   mouth, ore crate and cut-stone yard, a parked handcart and a lantern for the early shift.
//! - **Plot** — a marked-out construction site: a cleared earth pad framed in timber beams,
//!   corner survey stakes, and a little "build here" signpost.

use bevy::prelude::*;

use crate::castle::{bx, flat, gable, log_x, taper, M};

const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;

// ── Farm: thatched barn (−X) + tilled field (+X) ─────────────────────────────────────

/// The farm's barn: plank walls under a deep thatch gable, big double door, hayloft hatch
/// with a hoist beam poking out of the gable.
fn barn(cx: f32) -> Vec<(Mesh, M)> {
    let (w, h, d, f) = (1.8, 0.85, 1.55, 0.12);
    let top = f + h;
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(w + 0.14, f, d + 0.14, cx, f / 2.0, 0.0), M::HouseStone)); // footing
    v.push((bx(w, h, d, cx, f + h / 2.0, 0.0), M::Wood)); // plank walls
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.12, h, 0.12, cx + sx * (w / 2.0), f + h / 2.0, sz * (d / 2.0)), M::Beam));
        }
    }
    // Big double door on the +Z front (frame + two leaves).
    v.push((bx(0.78, 0.66, 0.05, cx, f + 0.35, d / 2.0 + 0.02), M::Beam));
    for sx in [-0.19_f32, 0.19] {
        v.push((bx(0.34, 0.6, 0.05, cx + sx, f + 0.33, d / 2.0 + 0.04), M::Wood));
    }
    // Hayloft hatch + hoist beam up in the gable, a hay tuft spilling out.
    v.push((bx(0.3, 0.26, 0.05, cx, top + 0.22, d / 2.0 - 0.1), M::Slit));
    v.push((bx(0.07, 0.07, 0.55, cx, top + 0.4, d / 2.0 + 0.18), M::Beam));
    v.push((bx(0.26, 0.12, 0.2, cx, top + 0.14, d / 2.0 + 0.02), M::Straw));
    // The deep-eaved thatch roof.
    v.push((gable(w + 0.5, d + 0.55, 0.72, top).translated_by(Vec3::new(cx, 0.0, 0.0)), M::Thatch));
    v
}

/// A scarecrow watching the rows: cross-frame, straw body, sack head under a plank hat.
fn scarecrow(cx: f32, cz: f32) -> Vec<(Mesh, M)> {
    vec![
        (bx(0.07, 0.95, 0.07, cx, 0.48, cz), M::Beam), // post
        (bx(0.7, 0.07, 0.07, cx, 0.72, cz), M::Beam),  // arms
        (taper(0.09, 0.14, 0.4, 0.62).translated_by(Vec3::new(cx, 0.0, cz)), M::Straw), // body
        (bx(0.16, 0.16, 0.16, cx, 1.0, cz), M::Plaster), // sack head
        (bx(0.3, 0.05, 0.3, cx, 1.11, cz), M::Wood),   // hat brim
        (bx(0.16, 0.1, 0.16, cx, 1.17, cz), M::Wood),  // hat crown
    ]
}

fn field(cx: f32) -> Vec<(Mesh, M)> {
    let fw = 1.9;
    let fd = 2.2;
    let mut v: Vec<(Mesh, M)> = vec![
        // Tilled soil bed (the same earth texture as the keep's kitchen garden / ground).
        (bx(fw, 0.12, fd, cx, 0.06, 0.0), M::Soil),
    ];
    // Four ripe crop rows running along X.
    for i in 0..4 {
        let z = -fd * 0.5 + 0.45 + i as f32 * (fd - 0.9) / 3.0;
        v.push((bx(fw - 0.4, 0.28, 0.16, cx, 0.22, z), M::Crop));
    }
    // Post-and-rail fence (timber) around the field.
    let hx = fw * 0.5;
    let hz = fd * 0.5;
    for (px, pz) in [(-hx, -hz), (hx, -hz), (-hx, hz), (hx, hz), (0.0, -hz), (0.0, hz)] {
        v.push((bx(0.08, 0.44, 0.08, cx + px, 0.22, pz), M::Beam));
    }
    for px in [-hx, hx] {
        v.push((bx(0.05, 0.06, fd, cx + px, 0.32, 0.0), M::Beam));
    }
    // A hay bale tucked at the field corner.
    v.push((bx(0.5, 0.42, 0.72, cx - fw * 0.5 - 0.35, 0.21, fd * 0.5 - 0.45), M::Crop));
    v
}

pub fn farm_parts() -> Vec<(Mesh, M)> {
    let mut v = barn(-0.95);
    v.extend(field(0.95));
    v.extend(scarecrow(0.55, -1.45));
    v
}

// ── Woodcutter: open saw shed over a sawpit (−X) + a stacked log yard (+X) ────────────

/// The woodcutter's shed: four posts under a weathered shingle roof, open on all sides, a
/// sawpit beneath (dark slot with a half-sawn log across it) and a long two-man saw leaning
/// on a post.
fn saw_shed(cx: f32) -> Vec<(Mesh, M)> {
    let (w, d, h) = (1.7, 1.5, 1.1);
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for sx in [-1.0_f32, 1.0] {
        for sz in [-1.0_f32, 1.0] {
            v.push((bx(0.13, h, 0.13, cx + sx * (w / 2.0), h / 2.0, sz * (d / 2.0)), M::Beam));
        }
    }
    // Plates along the post tops carrying the roof.
    for sz in [-1.0_f32, 1.0] {
        v.push((bx(w + 0.2, 0.09, 0.11, cx, h + 0.045, sz * (d / 2.0)), M::Beam));
    }
    v.push((gable(w + 0.5, d + 0.5, 0.42, h + 0.09).translated_by(Vec3::new(cx, 0.0, 0.0)), M::HouseRoof2));
    // The sawpit: a dark slot in the floor with a half-sawn log bridged across it.
    v.push((bx(1.1, 0.06, 0.55, cx, 0.03, 0.0), M::Slit));
    for sz in [-0.32_f32, 0.32] {
        v.push((bx(1.3, 0.09, 0.12, cx, 0.1, sz), M::Wood)); // pit edge boards
    }
    v.push((log_x(0.14, 1.5, 0.26, 0.0).translated_by(Vec3::new(cx, 0.0, 0.0)), M::Wood));
    // Long two-man saw leaning against the front-right post: thin iron blade + wood handles.
    let (sx, sz) = (cx + w / 2.0 - 0.02, d / 2.0 - 0.05);
    v.push((
        bx(0.025, 1.05, 0.1, 0.0, 0.52, 0.0)
            .rotated_by(Quat::from_rotation_z(0.35))
            .translated_by(Vec3::new(sx + 0.12, 0.0, sz)),
        M::Iron,
    ));
    v.push((bx(0.05, 0.16, 0.05, sx + 0.3, 0.96, sz), M::Wood));
    v.push((bx(0.05, 0.16, 0.05, sx - 0.06, 0.06, sz), M::Wood));
    v
}

fn log_yard(cx: f32) -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    // Two stacked rows of sawn logs running along Z.
    for dx in [-0.34, 0.0, 0.34] {
        v.push((bx(0.32, 0.32, 1.5, cx + dx, 0.17, 0.0), M::Wood));
    }
    for dx in [-0.17, 0.17] {
        v.push((bx(0.32, 0.32, 1.5, cx + dx, 0.49, 0.0), M::Wood));
    }
    // Sawhorse (two slanted timber A-frames + a top rail) in front of the pile.
    let sx = cx - 0.95;
    for sz in [-0.45_f32, 0.45] {
        v.push((bx(0.07, 0.5, 0.07, sx - 0.18, 0.25, sz), M::Beam));
        v.push((bx(0.07, 0.5, 0.07, sx + 0.18, 0.25, sz), M::Beam));
    }
    v.push((bx(0.06, 0.08, 1.1, sx, 0.5, 0.0), M::Beam));
    // Chopping stump + a half-split round on top.
    v.push((bx(0.46, 0.45, 0.46, cx - 0.4, 0.225, 1.05), M::Wood));
    v.push((bx(0.4, 0.12, 0.4, cx - 0.4, 0.5, 1.05), M::Beam));
    v
}

pub fn woodcutter_parts() -> Vec<(Mesh, M)> {
    let mut v = saw_shed(-0.95);
    v.extend(log_yard(0.95));
    v
}

// ── Stone Miner: pit-head frame (−X) + a stone yard (+X) ──────────────────────────────

/// The mine's pit-head: a dark shaft mouth framed in timber, an A-frame headgear carrying a
/// windlass wheel, rope and bucket, an ore crate and a lantern for the early shift.
fn pit_head(cx: f32) -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    // Shaft mouth: dark pit + timber curb framing it.
    v.push((bx(0.95, 0.04, 0.95, cx, 0.02, 0.0), M::Slit));
    for sz in [-0.5_f32, 0.5] {
        v.push((bx(1.16, 0.14, 0.14, cx, 0.09, sz), M::Beam));
    }
    for sx in [-0.5_f32, 0.5] {
        v.push((bx(0.14, 0.14, 0.9, cx + sx, 0.09, 0.0), M::Beam));
    }
    // A-frame headgear straddling the pit (legs lean in along X), crossbar at the apex.
    for sx in [-1.0_f32, 1.0] {
        v.push((
            bx(0.11, 1.5, 0.11, 0.0, 0.75, 0.0)
                .rotated_by(Quat::from_rotation_z(-sx * 0.32))
                .translated_by(Vec3::new(cx + sx * 0.62, 0.0, -0.28)),
            M::Beam,
        ));
        v.push((
            bx(0.11, 1.5, 0.11, 0.0, 0.75, 0.0)
                .rotated_by(Quat::from_rotation_z(-sx * 0.32))
                .translated_by(Vec3::new(cx + sx * 0.62, 0.0, 0.28)),
            M::Beam,
        ));
    }
    v.push((log_x(0.07, 0.7, 1.42, 0.0).rotated_by(Quat::from_rotation_y(HALF_PI)).translated_by(Vec3::new(cx, 0.0, 0.0)), M::Wood)); // windlass axle
    // Pulley wheel on the axle + rope dropping to a bucket just over the pit.
    v.push((
        flat(
            Cylinder::new(0.17, 0.06)
                .mesh()
                .resolution(10)
                .build()
                .rotated_by(Quat::from_rotation_x(HALF_PI))
                .translated_by(Vec3::new(cx, 1.42, 0.0)),
        ),
        M::BronzeDark,
    ));
    v.push((bx(0.03, 0.85, 0.03, cx, 0.95, 0.0), M::Beam)); // rope
    v.push((taper(0.13, 0.17, 0.24, 0.42).translated_by(Vec3::new(cx, 0.0, 0.0)), M::Iron)); // bucket
    // Ore crate beside the pit: a plank box with raw stone + a bronze glint inside.
    let (ox, oz) = (cx - 0.05, 0.95);
    v.push((bx(0.55, 0.3, 0.45, ox, 0.15, oz), M::Wood));
    v.push((bx(0.16, 0.14, 0.14, ox - 0.1, 0.34, oz), M::DarkStone));
    v.push((bx(0.13, 0.12, 0.13, ox + 0.12, 0.33, oz - 0.06), M::Stone));
    v.push((bx(0.09, 0.09, 0.09, ox + 0.04, 0.36, oz + 0.1), M::Bronze));
    // Lantern post for the early shift: post, arm, warm glowing lamp box.
    let (lx, lz) = (cx + 0.85, -0.85);
    v.push((bx(0.08, 1.15, 0.08, lx, 0.58, lz), M::Beam));
    v.push((bx(0.3, 0.06, 0.06, lx + 0.11, 1.12, lz), M::Beam));
    v.push((bx(0.14, 0.18, 0.14, lx + 0.24, 0.98, lz), M::Window));
    v.push((bx(0.18, 0.04, 0.18, lx + 0.24, 1.09, lz), M::BronzeDark));
    v
}

fn stone_yard(cx: f32) -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    // Bottom row of cut stone blocks running along Z (mixed greys for a quarried look).
    for (dz, m) in [(-0.5, M::Stone), (0.0, M::LightStone), (0.5, M::DarkStone)] {
        v.push((bx(0.5, 0.42, 0.46, cx, 0.21, dz), m));
    }
    // Two more blocks stacked on top.
    for (dz, m) in [(-0.25, M::LightStone), (0.25, M::Stone)] {
        v.push((bx(0.46, 0.4, 0.44, cx, 0.6, dz), m));
    }
    // A parked empty handcart in front of the pile: a plank bed with side rails on two wheels,
    // and two push-handles out the back (the loaded version rides home on the miner — `miner.rs`).
    let kx = cx - 1.0;
    v.push((bx(0.7, 0.16, 1.0, kx, 0.36, 0.0), M::Wood)); // bed
    for rz in [-0.46_f32, 0.46] {
        v.push((bx(0.66, 0.18, 0.07, kx, 0.5, rz), M::Beam)); // end rails
    }
    for wx in [-0.42_f32, 0.42] {
        v.push((bx(0.1, 0.4, 0.4, kx + wx, 0.2, 0.0), M::DarkStone)); // wheel (disk facing X)
    }
    v.push((bx(0.06, 0.06, 0.5, kx, 0.5, -0.7), M::Beam)); // push-handle bar
    // A pick leaning against the stone pile: a haft topped by a crossways head.
    v.push((bx(0.06, 0.86, 0.06, cx + 0.34, 0.5, 0.7), M::Beam));
    v.push((bx(0.44, 0.07, 0.07, cx + 0.34, 0.9, 0.7), M::Iron));
    v
}

pub fn mine_parts() -> Vec<(Mesh, M)> {
    let mut v = pit_head(-0.95);
    v.extend(stone_yard(0.95));
    v
}

// ── Build plot: a marked-out construction site ───────────────────────────────────────

/// An empty plot ready to build on: a cleared earth pad framed in timber foundation beams, four
/// corner survey stakes, and a little signpost — so it reads as "a plot under construction", not a
/// bare slab.
pub fn plot_parts() -> Vec<(Mesh, M)> {
    let r = 1.55; // half-size of the cleared pad
    let mut v: Vec<(Mesh, M)> = vec![
        // Cleared, levelled earth pad (same soil texture as the fields).
        (bx(r * 2.0, 0.1, r * 2.0, 0.0, 0.05, 0.0), M::Soil),
    ];
    // Timber foundation frame — four beams ringing the pad edge.
    for sz in [-r, r] {
        v.push((bx(r * 2.0 + 0.16, 0.12, 0.16, 0.0, 0.12, sz), M::Beam));
    }
    for sx in [-r, r] {
        v.push((bx(0.16, 0.12, r * 2.0 + 0.16, sx, 0.12, 0.0), M::Beam));
    }
    // Corner survey stakes (lean a touch out) topped with a bright cloth flag.
    for (sx, sz) in [(-r, -r), (r, -r), (r, r), (-r, r)] {
        v.push((bx(0.08, 0.6, 0.08, sx, 0.32, sz), M::Wood));
    }
    // A little "build here" signpost: a post + a slanted board.
    let px = r - 0.2;
    v.push((bx(0.1, 0.8, 0.1, px, 0.4, -r + 0.2), M::Wood));
    v.push((bx(0.6, 0.34, 0.06, px, 0.74, -r + 0.2), M::Beam));
    v
}
