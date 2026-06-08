//! Pure defense-structure logic — the deterministic, testable core of the war
//! bell + auto-firing defenses (towers / ballista) + healing shrine.
//!
//! The TS originals (`WarBell.tsx`, `Towers.tsx`, `Ballista.tsx`, `HealingShrine.tsx`)
//! mix model JSX, R3F `useFrame` polling and store calls. This module extracts ONLY
//! the numbers + the per-frame decisions that don't need the ECS world:
//!
//!   - the tower/ballista FIRE PROFILES (range/damage/cooldown/max-range/speed),
//!   - `nearest_in_range` — pick the closest of a set of targets within cast range
//!     (the inner loop shared by `Towers.tsx` and `Ballista.tsx`),
//!   - `is_ready` — the cooldown gate (`now >= ready_at`),
//!   - `heal_step` — the healing-shrine whole-HP accumulator flush.
//!
//! The ECS layer (`crates/game/src/defense.rs`) owns spawning the models, polling
//! the live ork entities, firing via the projectiles system, and applying the heal.

/// A defender structure's firing profile. Mirrors the `BASE`/`MASTERY` objects in
/// `Towers.tsx` and the `PROFILE` in `Ballista.tsx`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FireProfile {
    /// Cast range (tiles): an ork must be within this to be targeted.
    pub range: f64,
    /// Damage dealt per bolt.
    pub damage: f64,
    /// Seconds between shots.
    pub cooldown: f64,
    /// Distance a bolt may travel before fizzling (lets fast orks outrun a shot).
    pub max_range: f64,
    /// Bolt flight speed (tiles/s).
    pub speed: f64,
}

/// Watchtower HP — orks can batter a tower down (`towerStore.ts` `TOWER_MAX_HP`).
/// A tower at 0 HP is destroyed (stops firing, shows rubble); every prep phase
/// rebuilds all towers to full (`reviveTowers`).
pub const TOWER_MAX_HP: f64 = 180.0;

/// Base watchtower fire profile — `Towers.tsx` `BASE` (+ the TS `speed: 11`).
pub const TOWER_BASE: FireProfile =
    FireProfile { range: 18.0, damage: 7.0, cooldown: 1.6, max_range: 22.0, speed: 11.0 };

/// Tower-Mastery upgrade profile — `Towers.tsx` `MASTERY` (faster/farther/harder).
pub const TOWER_MASTERY: FireProfile =
    FireProfile { range: 24.0, damage: 12.0, cooldown: 1.0, max_range: 28.0, speed: 11.0 };

/// Ballista fire profile — `Ballista.tsx` `PROFILE` (long range, big single hits,
/// the TS `speed: 16`).
pub const BALLISTA: FireProfile =
    FireProfile { range: 24.0, damage: 45.0, cooldown: 2.6, max_range: 28.0, speed: 16.0 };

/// Keep-archer fire profile — `KeepArchers.tsx` `ARCHER` (+ the TS bolt `speed: 12`).
/// Four bowmen on the keep-roof corners; shorter range / lighter hits than a tower,
/// but stationed right over the gate the orks funnel toward.
pub const KEEP_ARCHER: FireProfile =
    FireProfile { range: 13.0, damage: 6.0, cooldown: 1.7, max_range: 16.0, speed: 12.0 };

/// Bolt muzzle height above a tower's base (`Towers.tsx` `TOWER_MUZZLE_Y`).
pub const TOWER_MUZZLE_Y: f64 = 6.0;
/// Bolt muzzle height above the ballista platform (`Ballista.tsx` `MUZZLE_Y`).
pub const BALLISTA_MUZZLE_Y: f64 = 1.3;
/// Bolt muzzle height above the keep's base — the roof deck (keep block top at local
/// y=2.2, scaled by the keep group's 0.7 → 1.54) plus the bow height (~0.66). The
/// figures stand on that same deck. Keep in sync with `map_render::build_castle`.
pub const KEEP_ARCHER_MUZZLE_Y: f64 = 2.2;
/// Height of the keep roof deck (where the archer figures stand): keep block top
/// (local 2.2) × the keep group Y-scale (0.7). `map_render::build_castle`.
pub const KEEP_ROOF_DECK_Y: f64 = 1.54;

/// Healing shrine: HP restored per second while the hero is inside the walls
/// (`HealingShrine.tsx` `HEAL_PER_SEC`).
pub const SHRINE_HEAL_PER_SEC: f64 = 4.0;

/// The keep's staged-destruction look, gated on its HP ratio (`cityModels.tsx` `Keep`
/// `useFrame`). The render layer swaps the merlon LOD + tint per stage, and shows
/// rubble + smoke/embers once `Burning`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepStage {
    /// `ratio > 0.66`: full merlons, pristine stone tint.
    Intact,
    /// `0.33 < ratio <= 0.66`: ~1/3 of the merlons knocked off, stone dimmed to 0.7.
    Damaged,
    /// `ratio <= 0.33`: most merlons gone, stone dimmed to 0.45, and the keep BURNS
    /// (ground rubble + smoke + embers).
    Burning,
}

impl KeepStage {
    /// Stone darkening factor for this stage (`cityModels.tsx`: `ratio>0.66 ? 1 :
    /// ratio>0.33 ? 0.7 : 0.45`). Multiplies the pristine stone/roof colours.
    pub fn tint_factor(self) -> f32 {
        match self {
            KeepStage::Intact => 1.0,
            KeepStage::Damaged => 0.7,
            KeepStage::Burning => 0.45,
        }
    }
    /// Whether the keep is on fire at this stage (rubble + smoke + embers shown).
    pub fn burning(self) -> bool {
        matches!(self, KeepStage::Burning)
    }
}

/// Classify the keep's destruction stage from its HP ratio (`hp / max_hp`), per the
/// `cityModels.tsx` `Keep` thresholds (`>0.66` full, `>0.33` partial, else sparse +
/// burning). A ratio of exactly 0.66 falls to `Damaged`, exactly 0.33 to `Burning`
/// (the TS uses strict `>`).
pub fn keep_stage(ratio: f64) -> KeepStage {
    if ratio > 0.66 {
        KeepStage::Intact
    } else if ratio > 0.33 {
        KeepStage::Damaged
    } else {
        KeepStage::Burning
    }
}

/// Has a structure's cooldown elapsed? (`now >= ready_at`, the TS `now < readyAt`
/// negated.) Re-arm with `ready_at = now + profile.cooldown` after firing.
pub fn is_ready(now: f64, ready_at: f64) -> bool {
    now >= ready_at
}

/// Pick the index of the nearest target within `range` of `(sx, sz)`, or `None` if
/// every target is out of range. The shared inner loop of `Towers.tsx` /
/// `Ballista.tsx`: it compares squared distances (cheap) and keeps the closest one
/// inside `range²`. `targets` is the live ork positions; the caller maps the
/// returned index back to its entity.
pub fn nearest_in_range(sx: f64, sz: f64, range: f64, targets: &[(f64, f64)]) -> Option<usize> {
    let range_sq = range * range;
    let mut best: Option<usize> = None;
    let mut best_d = range_sq;
    for (i, &(tx, tz)) in targets.iter().enumerate() {
        let dx = tx - sx;
        let dz = tz - sz;
        let d = dx * dx + dz * dz;
        if d < best_d {
            best_d = d;
            best = Some(i);
        }
    }
    best
}

/// The healing-shrine whole-HP accumulator step (`HealingShrine.tsx`): bank
/// `heal_per_sec * dt` into `acc`, then flush whole points so the HUD only updates
/// on integer HP gains. Returns `(whole_hp_to_heal, new_acc)`. Mirrors the TS
/// `healAcc` flush — fractional HP is carried, whole points are applied.
pub fn heal_step(acc: f64, heal_per_sec: f64, dt: f64) -> (i64, f64) {
    let banked = acc + heal_per_sec * dt;
    if banked >= 1.0 {
        let whole = banked.floor();
        (whole as i64, banked - whole)
    } else {
        (0, banked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tower_profiles_match_ts() {
        assert_eq!(TOWER_BASE.range, 18.0);
        assert_eq!(TOWER_BASE.damage, 7.0);
        assert_eq!(TOWER_BASE.cooldown, 1.6);
        assert_eq!(TOWER_BASE.max_range, 22.0);
        assert_eq!(TOWER_MASTERY.range, 24.0);
        assert_eq!(TOWER_MASTERY.damage, 12.0);
        assert_eq!(TOWER_MASTERY.cooldown, 1.0);
        assert_eq!(BALLISTA.range, 24.0);
        assert_eq!(BALLISTA.damage, 45.0);
        assert_eq!(BALLISTA.cooldown, 2.6);
        assert_eq!(BALLISTA.speed, 16.0);
        assert_eq!(KEEP_ARCHER.range, 13.0);
        assert_eq!(KEEP_ARCHER.damage, 6.0);
        assert_eq!(KEEP_ARCHER.cooldown, 1.7);
        assert_eq!(KEEP_ARCHER.max_range, 16.0);
        assert_eq!(KEEP_ARCHER.speed, 12.0);
    }

    #[test]
    fn tower_max_hp_matches_ts() {
        // towerStore.ts `TOWER_MAX_HP = 180`.
        assert_eq!(TOWER_MAX_HP, 180.0);
    }

    #[test]
    fn keep_stage_bands_match_ts_thresholds() {
        // cityModels.tsx: ratio>0.66 full, >0.33 partial, else sparse+burning.
        assert_eq!(keep_stage(1.0), KeepStage::Intact);
        assert_eq!(keep_stage(0.67), KeepStage::Intact);
        assert_eq!(keep_stage(0.66), KeepStage::Damaged); // strict >, so 0.66 falls
        assert_eq!(keep_stage(0.5), KeepStage::Damaged);
        assert_eq!(keep_stage(0.34), KeepStage::Damaged);
        assert_eq!(keep_stage(0.33), KeepStage::Burning);
        assert_eq!(keep_stage(0.1), KeepStage::Burning);
        assert_eq!(keep_stage(0.0), KeepStage::Burning);
    }

    #[test]
    fn keep_stage_tint_and_burning_flags() {
        assert_eq!(KeepStage::Intact.tint_factor(), 1.0);
        assert_eq!(KeepStage::Damaged.tint_factor(), 0.7);
        assert_eq!(KeepStage::Burning.tint_factor(), 0.45);
        assert!(!KeepStage::Intact.burning());
        assert!(!KeepStage::Damaged.burning());
        assert!(KeepStage::Burning.burning());
    }

    #[test]
    fn cooldown_gate() {
        assert!(!is_ready(1.0, 2.0), "still on cooldown");
        assert!(is_ready(2.0, 2.0), "exactly ready");
        assert!(is_ready(3.0, 2.0), "past ready");
    }

    #[test]
    fn nearest_in_range_picks_the_closest_within_cast() {
        // Source at origin; three orks, the middle one closest.
        let orks = [(10.0, 0.0), (3.0, 0.0), (6.0, 0.0)];
        let i = nearest_in_range(0.0, 0.0, 18.0, &orks).expect("one in range");
        assert_eq!(i, 1, "the ork at distance 3 is nearest");
    }

    #[test]
    fn nearest_in_range_excludes_out_of_range() {
        // All orks beyond the cast range → no target.
        let orks = [(30.0, 0.0), (0.0, 25.0)];
        assert_eq!(nearest_in_range(0.0, 0.0, 18.0, &orks), None);
    }

    #[test]
    fn nearest_in_range_empty() {
        assert_eq!(nearest_in_range(0.0, 0.0, 18.0, &[]), None);
    }

    #[test]
    fn nearest_in_range_at_exact_boundary_is_excluded() {
        // Distance exactly == range is NOT inside (strict `<` on the squared dist,
        // matching the TS `d < bestD` seeded with `rangeSq`).
        let orks = [(18.0, 0.0)];
        assert_eq!(nearest_in_range(0.0, 0.0, 18.0, &orks), None);
        let orks = [(17.9, 0.0)];
        assert_eq!(nearest_in_range(0.0, 0.0, 18.0, &orks), Some(0));
    }

    #[test]
    fn heal_step_banks_fractions_and_flushes_whole_points() {
        // 4 HP/s over a 60 Hz tick (1/60 s) = 0.0667 HP — nothing flushed yet.
        let (whole, acc) = heal_step(0.0, SHRINE_HEAL_PER_SEC, 1.0 / 60.0);
        assert_eq!(whole, 0, "fraction banked, no whole HP yet");
        assert!((acc - 4.0 / 60.0).abs() < 1e-9);

        // Once the accumulator crosses 1.0, flush exactly one whole point.
        let (whole, acc) = heal_step(0.95, SHRINE_HEAL_PER_SEC, 1.0 / 60.0);
        assert_eq!(whole, 1, "crossed 1.0 → one whole HP");
        assert!((0.0..1.0).contains(&acc), "remainder carried, {acc}");
    }

    #[test]
    fn heal_step_flushes_multiple_points_on_a_big_dt() {
        // A whole second at 4 HP/s flushes 4 points at once.
        let (whole, acc) = heal_step(0.0, SHRINE_HEAL_PER_SEC, 1.0);
        assert_eq!(whole, 4);
        assert!(acc.abs() < 1e-9, "no remainder, {acc}");
    }
}
