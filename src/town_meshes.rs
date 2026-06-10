//! **Textured producer buildings + build plots for the town economy.** Built from the SAME
//! procedural materials as the keep ([`crate::castle::VillageMats`] / `M`) and around the same
//! [`crate::castle::house_parts`] cottage, so a Farm or Woodcutter reads as part of the walled
//! village — not a flat-shaded prop. Each builder returns a list of `(Mesh, M)` parts (a mesh +
//! its material slot); `town::spawn_building` bakes them at the plot and assigns the shared
//! textured materials.
//!
//! - **Farm** — a plaster cottage beside a tilled, fenced field of crop rows + a hay bale.
//! - **Woodcutter** — the same cottage beside a stacked log yard, a sawhorse and a chopping stump.
//! - **Stone Miner** — the same cottage beside a stone yard: cut blocks, a parked handcart, a pick.
//! - **Plot** — a marked-out construction site: a cleared earth pad framed in timber beams, corner
//!   survey stakes, and a little "build here" signpost.

use bevy::prelude::*;

use crate::castle::{bake, bx, house_parts, M};

/// Scale + offset a set of textured parts (e.g. drop the shared house onto one side of a plot).
fn place(parts: Vec<(Mesh, M)>, scale: f32, offset: Vec3) -> Vec<(Mesh, M)> {
    parts.into_iter().map(|(m, mat)| (bake(m, offset, 0.0, Vec3::splat(scale)), mat)).collect()
}

/// Scale the shared cottage down to share a plot with a field/log-yard, and sit it on the −X side.
fn cottage() -> Vec<(Mesh, M)> {
    place(house_parts(), 0.62, Vec3::new(-0.95, 0.0, 0.0))
}

// ── Farm: cottage (−X) + tilled field (+X) ───────────────────────────────────────────

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
    let mut v = cottage();
    v.extend(field(0.95));
    v
}

// ── Woodcutter: cottage (−X) + a stacked log yard (+X) ────────────────────────────────

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
    let mut v = cottage();
    v.extend(log_yard(0.95));
    v
}

// ── Stone Miner: cottage (−X) + a stone yard (+X) ──────────────────────────────────────

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
    v.push((bx(0.44, 0.07, 0.07, cx + 0.34, 0.9, 0.7), M::Bronze));
    v
}

pub fn mine_parts() -> Vec<(Mesh, M)> {
    let mut v = cottage();
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
