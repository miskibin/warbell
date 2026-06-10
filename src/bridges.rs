//! **Bridges** — plank decks laid across the combined map's real river. The river is a carved
//! terrain channel (the sea plane shows through where `worldmap::is_river_world` is true), so we
//! SCAN that channel at a few depths, find the water run's centre + width, and span it bank to
//! bank. Each deck also registers a walkable span the nav-grid honours, so the night invaders'
//! A* can cross at a bridge. Ports Bridge.tsx/bridges.ts, placed on the actual water.

use std::sync::OnceLock;

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::lin;
use crate::worldmap::{is_river_world, GX, GZ};

/// Half-width along the bank (the deck's SHORT axis) of a deck.
const DECK_HALF_Z: f32 = 1.2;
/// Bank overhang past the water edge on each side (world units).
const OVERHANG: f32 = 1.4;
/// Min world-XZ gap between two bridges (so they don't cluster on one crossing).
const MIN_SPACING: f32 = 13.0;
/// At most this many bridges (four rivers now cross the island — each needs several crossings
/// or the player/invaders detour absurdly far).
const MAX_BRIDGES: usize = 12;
/// Acceptable half-width of the channel being bridged (skip slivers + wide lake-like spans —
/// a clean river crossing is a couple units across).
const MIN_HALF: f32 = 0.6;
const MAX_HALF: f32 = 5.0;

/// A bridge deck: world-XZ centre, the long half-length across the water (incl. overhang), and
/// whether the deck's LONG axis runs along X (`across_x` → crosses a river flowing along Z) or
/// along Z. The short axis is always `DECK_HALF_Z`.
#[derive(Clone, Copy)]
struct Span {
    cx: f32,
    cz: f32,
    half: f32,
    across_x: bool,
}

/// Find a few clean river crossings by scanning the whole island for NARROW water channels.
/// Cached — reads only the pure `is_river_world` channel (no built terrain). The combined-map
/// river is L-shaped (a near-vertical `river_x` branch + a horizontal `river_z` branch), so a
/// deck must span whichever axis the local channel is narrow on — not always X.
fn spans() -> &'static [Span] {
    static SPANS: OnceLock<Vec<Span>> = OnceLock::new();
    SPANS.get_or_init(|| {
        // 1. Gather every candidate crossing on a coarse grid over the island.
        let mut cands: Vec<Span> = Vec::new();
        let mut z = -GZ + 4.0;
        while z < GZ - 4.0 {
            let mut x = -GX + 4.0;
            while x < GX - 4.0 {
                if let Some(s) = crossing_at(x, z) {
                    cands.push(s);
                }
                x += 2.0;
            }
            z += 2.0;
        }
        // 2. Pick well-SPREAD crossings: seed with the narrowest, then repeatedly take the
        //    candidate farthest from every chosen deck (max-min distance, capped at
        //    MIN_SPACING). Narrowest-first alone clustered every deck on the thinnest
        //    channels and left the widest river with no bridge at all; spreading by distance
        //    covers each river — and each stretch of it.
        cands.sort_by(|a, b| a.half.partial_cmp(&b.half).unwrap_or(std::cmp::Ordering::Equal));
        let mut out: Vec<Span> = Vec::new();
        if let Some(&first) = cands.first() {
            out.push(first);
        }
        while out.len() < MAX_BRIDGES {
            let next = cands
                .iter()
                .map(|c| {
                    let d = out
                        .iter()
                        .map(|s| (s.cx - c.cx).hypot(s.cz - c.cz))
                        .fold(f32::INFINITY, f32::min);
                    (c, d)
                })
                .filter(|(_, d)| *d >= MIN_SPACING)
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            match next {
                Some((c, _)) => out.push(*c),
                None => break,
            }
        }
        out
    })
}

/// If `(x, z)` is river water on a clean narrow crossing, return the centred deck span. Measures
/// the channel width along X and along Z; the deck spans the NARROWER axis (bank to bank), and
/// both ends must sit on solid land (not the open sea at a river mouth).
fn crossing_at(x: f32, z: f32) -> Option<Span> {
    if !is_river_world(x, z) {
        return None;
    }
    let (cx_x, half_x) = water_run(x, z, true)?; // channel along X
    let (cz_z, half_z) = water_run(x, z, false)?; // channel along Z
    // Narrower axis = the crossing direction (the deck spans it, bank to bank).
    let (across_x, cx, cz, half) = if half_x <= half_z {
        (true, cx_x, z, half_x)
    } else {
        (false, x, cz_z, half_z)
    };
    if !(MIN_HALF..=MAX_HALF).contains(&half) {
        return None;
    }
    let end = half + OVERHANG;
    let (ex, ez) = if across_x { (end, 0.0) } else { (0.0, end) };
    let ya = crate::worldmap::ground_at_world(cx + ex, cz + ez)?; // a coast / river-mouth
    let yb = crate::worldmap::ground_at_world(cx - ex, cz - ez)?; // is not a crossing
    if (ya - yb).abs() > 0.01 {
        return None; // skewed banks — the deck is flat, so the high end would be a cliff step
    }
    Some(Span { cx, cz, half: end, across_x })
}

/// Walk both ways from `(x, z)` along one axis (`x_axis` ? X : Z) to the channel banks; return
/// the channel's `(centre, half_width)` on that axis. `None` if the run overruns `MAX_HALF` (a
/// wide span — not a tidy crossing) so the caller bails.
fn water_run(x: f32, z: f32, x_axis: bool) -> Option<(f32, f32)> {
    let limit = MAX_HALF * 2.0 + 2.0;
    let wet = |d: f32| if x_axis { is_river_world(x + d, z) } else { is_river_world(x, z + d) };
    let mut pos = 0.5;
    while wet(pos) {
        pos += 0.5;
        if pos > limit {
            return None;
        }
    }
    let mut neg = 0.5;
    while wet(-neg) {
        neg += 0.5;
        if neg > limit {
            return None;
        }
    }
    let centre = (if x_axis { x } else { z }) + (pos - neg) * 0.5;
    Some((centre, (pos + neg) * 0.5))
}

/// The span whose deck covers `(wx, wz)`, if any. The long axis is X when `across_x`, else Z.
fn span_at(wx: f32, wz: f32) -> Option<&'static Span> {
    spans().iter().find(|s| {
        let (along, across) =
            if s.across_x { (wx - s.cx, wz - s.cz) } else { (wz - s.cz, wx - s.cx) };
        along.abs() <= s.half && across.abs() <= DECK_HALF_Z
    })
}

/// Is `(wx, wz)` on a bridge deck? Consulted by `navgrid::standable` so A* can cross the river.
pub fn is_on_bridge(wx: f32, wz: f32) -> bool {
    span_at(wx, wz).is_some()
}

/// Walkable deck-top Y at `(wx, wz)` if it's on a bridge, else `None`. The hero ORs this onto
/// `worldmap::ground_at_world` (which is terrain-only and reads `None` over the river) so he can
/// stand + ground on the planks. Deck transform sits at `bank_y + 0.2`; planks are 0.1 thick →
/// their top is `bank_y + 0.25`, where the feet rest. Bank ground is sampled at the span's land
/// overhang, so this never recurses into a bridge lookup.
pub fn deck_y_at(wx: f32, wz: f32) -> Option<f32> {
    span_at(wx, wz).map(|s| bank_y(s) + 0.25)
}

/// Ground height at the span's banks (sampled at its long-axis ends, which overhang land).
fn bank_y(s: &Span) -> f32 {
    let (ex, ez) = if s.across_x { (s.half, 0.0) } else { (0.0, s.half) };
    crate::worldmap::ground_at_world(s.cx + ex, s.cz + ez)
        .or_else(|| crate::worldmap::ground_at_world(s.cx - ex, s.cz - ez))
        .unwrap_or(0.0)
}

// ── mesh ───────────────────────────────────────────────────────────────────────────
fn tinted(mut m: Mesh, c: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(c); n]);
    m
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}

/// One deck mesh spanning `2·half_x` across X (local space; deck top at y≈0).
fn deck_mesh(half_x: f32) -> Mesh {
    const LIGHT: u32 = 0x8a5a32;
    const DARK: u32 = 0x6b4222;
    const RAIL: u32 = 0x5a3a22;
    let len = half_x * 2.0;
    let mut parts: Vec<Mesh> = Vec::new();
    let planks = (len * 2.0).max(4.0) as i32;
    for i in 0..planks {
        let x = -half_x + (i as f32 + 0.5) / planks as f32 * len;
        let c = if i % 2 == 0 { LIGHT } else { DARK };
        parts.push(bx(len / planks as f32 * 0.92, 0.1, DECK_HALF_Z * 2.0, Vec3::new(x, 0.0, 0.0), c));
    }
    for sz in [-DECK_HALF_Z, DECK_HALF_Z] {
        parts.push(bx(len, 0.08, 0.1, Vec3::new(0.0, 0.45, sz), RAIL)); // side rail
        for sx in [-half_x + 0.2, half_x - 0.2] {
            parts.push(bx(0.12, 0.55, 0.12, Vec3::new(sx, 0.22, sz), RAIL)); // end post
        }
    }
    for sz in [-DECK_HALF_Z + 0.3, DECK_HALF_Z - 0.3] {
        parts.push(bx(len, 0.12, 0.14, Vec3::new(0.0, -0.12, sz), DARK)); // underbeam
    }
    let mut it = parts.into_iter();
    let mut base = it.next().unwrap();
    for p in it {
        base.merge(&p).expect("bridge parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every river must end up with at least one deck — the greedy narrowest-first pick is
    /// allowed to favour clean crossings, but a whole river with no bridge strands invader
    /// camps (and players) on long detours. Buckets are loose world-space regions of the four
    /// channels: west vertical, southern stream, north horizontal, south horizontal.
    #[test]
    fn every_river_gets_a_bridge() {
        let s = spans();
        assert!(s.len() >= 8, "expected a healthy bridge count, got {}", s.len());
        let west_vert = s.iter().any(|b| b.across_x && b.cx < -30.0);
        let south_stream = s.iter().any(|b| b.across_x && b.cx >= -30.0 && b.cz > 5.0);
        let north_horiz = s.iter().any(|b| !b.across_x && b.cz < -35.0);
        let south_horiz = s.iter().any(|b| !b.across_x && b.cz > 30.0);
        let dump: Vec<(f32, f32, bool)> = s.iter().map(|b| (b.cx, b.cz, b.across_x)).collect();
        assert!(
            west_vert && south_stream && north_horiz && south_horiz,
            "uncovered river (west_vert={west_vert} south_stream={south_stream} \
             north_horiz={north_horiz} south_horiz={south_horiz}); spans: {dump:?}"
        );
    }
}

/// Spawn a deck at each river crossing. Called from `worldmap::build` (after terrain).
pub fn populate(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    for s in spans() {
        // `deck_mesh` always spans X; rotate 90° about Y for a Z-spanning (across_x = false) deck.
        let rot = if s.across_x {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
        };
        commands.spawn((
            Mesh3d(meshes.add(deck_mesh(s.half))),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: Vec3::new(s.cx, bank_y(s) + 0.2, s.cz),
                rotation: rot,
                ..default()
            },
            BiomeEntity,
        ));
    }
}
