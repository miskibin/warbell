//! Port of the AMBIENT decorations — `src/world/Cat.tsx` + `src/world/Birds.tsx`.
//!
//! These are decorative critters, NOT part of the combat `animal` roster: cats wander
//! the villages + castle and (in the full game) stalk low birds; birds circle overhead
//! in flocks. They have no HP, deal/take no damage, and grant nothing — pure flavour.
//!
//! Only the deterministic, dependency-free math lives here (spawn plans + the per-tick
//! position/state math), so it's `#[cfg(test)]`-checkable like the rest of the crate.
//! The Bevy layer (`game/src/wildlife.rs`) owns the entities/models and feeds the
//! player pose + land-check in.
//!
//! Simplifications vs. the TS:
//!   - the cat↔bird stalk coupling (`Cat` reading `getBirds()` to pounce a low bird)
//!     is dropped: the ported `Birds` fly high and never dive to the ground (the TS
//!     `Birds.tsx` itself removed the dive — see its comment "Birds stay high"), so
//!     no bird is ever stalk-eligible. Cats therefore cycle idle/walk/sit only, which
//!     is exactly what the TS does when no low bird is near.
//!   - the per-part body/tail wag is a render detail, not ported.
//!
//! f64 throughout for JS-`number` parity (matches the rest of the crate).

use crate::tilemap::shift_to_centre;

// ─── Cats (Cat.tsx) ──────────────────────────────────────────────────────────────

/// Relaxed walk speed (TS `CAT_SPEED_WALK`).
pub const CAT_SPEED_WALK: f64 = 1.0;
/// How far a cat wanders from its home anchor (TS `WANDER_RADIUS`).
pub const CAT_WANDER_RADIUS: f64 = 3.2;

/// The four cat home anchors — base-map coords from `World.tsx` `CAT_HOMES`, shifted
/// onto the enlarged map exactly like the TS `.map(shiftToCentre)`. Paired with the
/// TS per-cat `seed` literals `[0.7, 2.1, 3.4, 5.6]`.
pub const CAT_BASE_HOMES: [(f64, f64); 4] = [(72.0, 67.0), (50.0, 40.0), (66.0, 50.0), (80.0, 58.0)];
/// Per-cat seeds (TS `seed={[0.7, 2.1, 3.4, 5.6][i]}`).
pub const CAT_SEEDS: [f64; 4] = [0.7, 2.1, 3.4, 5.6];

/// The grid-space (enlarged-map) home anchor for cat `i`, or `None` out of range.
pub fn cat_home(i: usize) -> Option<(f64, f64)> {
    let (bx, bz) = *CAT_BASE_HOMES.get(i)?;
    Some(shift_to_centre(bx, bz))
}

/// A cat's behaviour mode (TS `CatMode`, minus `stalk` — see the module note: no bird
/// ever dips low enough to trigger a stalk in the ported world).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatMode {
    Idle,
    Walk,
    Sit,
}

/// The TS deterministic hash used to drive a cat's state re-rolls + wander targets
/// (`pseudoRand` in Cat.tsx): `frac(sin(seed*12.9898 + n*78.233) * 43758.5453)`.
pub fn cat_pseudo_rand(seed: f64, n: f64) -> f64 {
    let x = (seed * 12.9898 + n * 78.233).sin() * 43758.5453;
    x - x.floor()
}

/// A re-roll decision when a cat's `state_until` expires: the new mode, how long it
/// lasts (added to `now` by the caller), and — for `Walk` — the new wander target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatReroll {
    pub mode: CatMode,
    /// Seconds the new state lasts.
    pub duration: f64,
    /// Walk destination (grid space); `None` for idle/sit.
    pub target: Option<(f64, f64)>,
}

/// Re-roll a cat between idle/walk/sit when its current state expires. Port of the
/// `else if (t > st.stateUntil)` block in Cat.tsx: `r = pseudoRand(seed, t|0)`,
/// `<0.35` → sit, `<0.45` → idle, else walk to a point within `WANDER_RADIUS` of home.
/// `t_floor` is `t | 0` (the integer time the TS hashed against). Deterministic.
pub fn cat_reroll(home: (f64, f64), seed: f64, t_floor: f64) -> CatReroll {
    let r = cat_pseudo_rand(seed, t_floor);
    if r < 0.35 {
        CatReroll {
            mode: CatMode::Sit,
            duration: 2.5 + cat_pseudo_rand(seed + 2.0, t_floor) * 3.0,
            target: None,
        }
    } else if r < 0.45 {
        CatReroll {
            mode: CatMode::Idle,
            duration: 1.0 + cat_pseudo_rand(seed + 3.0, t_floor) * 1.5,
            target: None,
        }
    } else {
        let tx = home.0 + (cat_pseudo_rand(seed + 4.0, t_floor) - 0.5) * CAT_WANDER_RADIUS * 2.0;
        let tz = home.1 + (cat_pseudo_rand(seed + 5.0, t_floor) - 0.5) * CAT_WANDER_RADIUS * 2.0;
        CatReroll {
            mode: CatMode::Walk,
            duration: 2.5 + cat_pseudo_rand(seed + 6.0, t_floor) * 2.5,
            target: Some((tx, tz)),
        }
    }
}

// ─── Birds (Birds.tsx) ─────────────────────────────────────────────────────────────

/// One circling flock — centre is in WORLD coords (already offset by −CENTER), like the
/// TS `FLOCKS` array. Mirrors `Flock`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flock {
    pub cx: f64,
    pub cy: f64,
    pub cz: f64,
    pub radius: f64,
    pub count: u32,
    pub speed: f64,
    pub phase: f64,
}

/// The five flocks from `Birds.tsx` `FLOCKS` (world coords, unchanged).
pub const FLOCKS: [Flock; 5] = [
    Flock { cx: -28.0, cy: 11.0, cz: 18.0, radius: 10.0, count: 5, speed: 0.32, phase: 0.0 },
    Flock { cx: 24.0, cy: 12.0, cz: -22.0, radius: 12.0, count: 5, speed: 0.28, phase: 1.5 },
    Flock { cx: 18.0, cy: 11.0, cz: 22.0, radius: 9.0, count: 4, speed: 0.36, phase: 3.2 },
    // over the castle
    Flock { cx: 6.0, cy: 13.0, cz: -2.0, radius: 13.0, count: 5, speed: 0.24, phase: 2.1 },
    Flock { cx: -10.0, cy: 10.0, cz: -20.0, radius: 8.0, count: 4, speed: 0.4, phase: 4.6 },
];

/// Total bird count across all flocks (TS `TOTAL`).
pub fn bird_total() -> usize {
    FLOCKS.iter().map(|f| f.count as usize).sum()
}

/// Per-bird static layout within its flock (TS `meta`): the orbit radius + vertical
/// offset + a heading-phase index. Built once; the position formula reads it each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BirdMeta {
    /// flock index.
    pub fi: usize,
    /// index within the flock.
    pub ii: u32,
    pub ring_r: f64,
    pub ring_y: f64,
}

/// Build the per-bird layout list, in the SAME order as the TS instanced mesh
/// (`FLOCKS.forEach … for i in 0..count`). Mirrors the `meta` `useMemo`.
pub fn bird_layout() -> Vec<BirdMeta> {
    let mut list = Vec::with_capacity(bird_total());
    for (fi, f) in FLOCKS.iter().enumerate() {
        for i in 0..f.count {
            let ring_r = f.radius * (0.9 + (i % 3) as f64 * 0.06);
            let ring_y = ((i as f64 * 1.7 + fi as f64 * 0.3).sin() * 0.5 + 0.5) * 1.4 - 0.7;
            list.push(BirdMeta { fi, ii: i, ring_r, ring_y });
        }
    }
    list
}

/// World-space position of a bird at time `t`. Port of the per-bird body of the
/// `useFrame` loop in Birds.tsx: orbit angle `phase + t*speed + ii*(2π/count)`, x/z on
/// the ring, y a gentle bob (`cy + ring_y + sin(t*1.6 + ii)*0.4`). Returns `(x, y, z)`.
pub fn bird_position(m: &BirdMeta, t: f64) -> (f64, f64, f64) {
    let f = &FLOCKS[m.fi];
    let ang = f.phase + t * f.speed + m.ii as f64 * (std::f64::consts::TAU / f.count as f64);
    let x = f.cx + ang.cos() * m.ring_r;
    let z = f.cz + ang.sin() * m.ring_r;
    let y = f.cy + m.ring_y + (t * 1.6 + m.ii as f64).sin() * 0.4;
    (x, y, z)
}

/// The yaw heading a bird faces while orbiting (TS `dummy.rotation.y = ang + π/2`).
pub fn bird_facing(m: &BirdMeta, t: f64) -> f64 {
    let f = &FLOCKS[m.fi];
    let ang = f.phase + t * f.speed + m.ii as f64 * (std::f64::consts::TAU / f.count as f64);
    ang + std::f64::consts::FRAC_PI_2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::{CENTER_X, CENTER_Z};

    // ─── Cats ────────────────────────────────────────────────────────────────────

    #[test]
    fn cat_homes_shift_onto_the_enlarged_map() {
        // Four homes, each a distinct shifted point — and the canonical first home
        // (base 72,67 = the village by the keep) lands near the map centre+south.
        for i in 0..4 {
            assert!(cat_home(i).is_some(), "cat {i} should have a home");
        }
        assert!(cat_home(4).is_none());
        let (x0, _z0) = cat_home(0).unwrap();
        // Base 72 is the BASE_CENTER_X, so its shift lands at CENTER_X exactly.
        assert!((x0 - CENTER_X).abs() < 1e-9, "base-centre x maps to CENTER_X");
    }

    #[test]
    fn cat_pseudo_rand_is_deterministic_and_unit_range() {
        for n in 0..100 {
            let r = cat_pseudo_rand(2.1, n as f64);
            assert!((0.0..1.0).contains(&r), "pseudoRand out of [0,1): {r}");
            // Same inputs → same output.
            assert_eq!(r, cat_pseudo_rand(2.1, n as f64));
        }
    }

    #[test]
    fn cat_reroll_picks_modes_by_the_ts_thresholds() {
        // Find an integer time bucket that drives each of the three branches, proving
        // the <0.35 sit / <0.45 idle / else walk split is reproduced.
        let home = (100.0, 100.0);
        let seed = 0.7;
        let mut saw_sit = false;
        let mut saw_idle = false;
        let mut saw_walk = false;
        for tf in 0..400 {
            let r = cat_reroll(home, seed, tf as f64);
            match r.mode {
                CatMode::Sit => {
                    saw_sit = true;
                    assert!(r.target.is_none());
                    assert!(r.duration >= 2.5);
                }
                CatMode::Idle => {
                    saw_idle = true;
                    assert!(r.target.is_none());
                    assert!(r.duration >= 1.0);
                }
                CatMode::Walk => {
                    saw_walk = true;
                    let (tx, tz) = r.target.expect("walk has a target");
                    // The target stays within the wander box around home.
                    assert!((tx - home.0).abs() <= CAT_WANDER_RADIUS + 1e-9);
                    assert!((tz - home.1).abs() <= CAT_WANDER_RADIUS + 1e-9);
                }
            }
        }
        assert!(saw_sit && saw_idle && saw_walk, "all three modes should occur across time");
    }

    // ─── Birds ───────────────────────────────────────────────────────────────────

    #[test]
    fn bird_layout_has_total_count_in_flock_order() {
        let layout = bird_layout();
        assert_eq!(layout.len(), bird_total());
        assert_eq!(bird_total(), 23); // 5+5+4+5+4
        // First five belong to flock 0, indices 0..4 in order.
        for (i, m) in layout.iter().take(5).enumerate() {
            assert_eq!(m.fi, 0);
            assert_eq!(m.ii, i as u32);
        }
        assert_eq!(layout[5].fi, 1);
    }

    #[test]
    fn bird_orbits_on_its_ring_at_flock_height() {
        let layout = bird_layout();
        let m = &layout[0];
        let f = &FLOCKS[m.fi];
        // Sample the orbit at several times: x/z stay on the ring radius about the
        // flock centre, y stays within the bob band of the flock altitude.
        for k in 0..50 {
            let t = k as f64 * 0.37;
            let (x, y, z) = bird_position(m, t);
            let r = ((x - f.cx).powi(2) + (z - f.cz).powi(2)).sqrt();
            assert!((r - m.ring_r).abs() < 1e-9, "bird should ride its ring radius");
            // y within cy + ring_y ± 0.4 bob.
            assert!((y - (f.cy + m.ring_y)).abs() <= 0.4 + 1e-9);
            // High overhead — never near the ground.
            assert!(y > 5.0, "birds stay high (y={y})");
        }
        // The over-castle flock (index 3) hovers above the map centre region.
        let castle_flock = &FLOCKS[3];
        assert!((castle_flock.cx).abs() < 14.0 && (castle_flock.cz).abs() < 14.0);
        let _ = (CENTER_X, CENTER_Z);
    }
}
