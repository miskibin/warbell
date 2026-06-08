//! Port of `src/world/dummyStore.ts` + the deterministic pell-quintain swing cycle
//! from `src/world/MusterYard.tsx` — the castle muster-ground practice targets.
//!
//! The TS splits this across three files: `dummyStore.ts` (the hittable-target store,
//! "oreStore minus the HP/reward, plus an `isPell` flag"), `TrainingDummy.tsx` (the
//! primitive model), and `MusterYard.tsx` (placement + the live pell-arm animation +
//! block-drill resolve). The PURE pieces port here:
//!   - the muster-yard placement table (BASE coords → enlarged map via `shift_to_centre`),
//!   - the `DummyState` (hurt-flash + recoil-wobble deadlines, `is_pell`),
//!   - `damage_dummy` (a hit just sets the two deadlines — NO HP, NO reward),
//!   - the pell quintain's deterministic arm-yaw curve over its swing cycle, and
//!   - the recoil-wobble angle curve.
//! The model (TrainingDummy.tsx) + the live drive + the block resolve (which read the
//! Bevy player/block state + spawn FX) live in `crates/game`.
//!
//! Dummies are INDESTRUCTIBLE practice props: they never die, carry no HP, drop no
//! loot — confirmed against `dummyStore.ts` ("There is NO HP and NO reward — that's
//! the whole point"). A hit only flashes + wobbles them.

use crate::tilemap::shift_to_centre;

// ─── Hit-reaction timings (dummyStore.ts `damageDummy` + MusterYard.tsx feedback) ──

/// Seconds the straw hit-flash lasts after a swing lands (`damageDummy`: `now + 0.18`).
pub const HURT_FLASH: f64 = 0.18;
/// Seconds the recoil wobble lasts after a hit (`damageDummy`: `now + 0.5`).
pub const WOBBLE: f64 = 0.5;
/// Player/pathing collision radius of a dummy post (`dummyStore.ts` `collisionRadius`).
pub const COLLISION_RADIUS: f64 = 0.28;

// ─── Pell quintain swing cycle (MusterYard.tsx L57-71) ────────────────────────────

/// Long rest before the arm winds up (seconds).
pub const REST: f64 = 3.2;
/// Wind-up telegraph before the strike (seconds).
pub const WINDUP: f64 = 0.6;
/// Fast strike — the player's block window (seconds).
pub const STRIKE: f64 = 0.22;
/// Recovery back to rest after the strike (seconds).
pub const RECOVER: f64 = 0.6;
/// Full cycle length (seconds).
pub const CYCLE: f64 = REST + WINDUP + STRIKE + RECOVER;
/// Cycle time the strike begins at.
pub const STRIKE_START: f64 = REST + WINDUP;
/// Cycle time the strike ends at.
pub const STRIKE_END: f64 = STRIKE_START + STRIKE;

/// Arm yaw at rest — the club sits out to the side (`YAW_REST`).
pub const YAW_REST: f64 = 0.6;
/// Arm yaw wound back at the top of the wind-up (`YAW_WOUND`).
pub const YAW_WOUND: f64 = 1.4;
/// Arm yaw at the strike — the club sweeps to the dummy's front, toward the player
/// (`YAW_FRONT = -π/2`).
pub const YAW_FRONT: f64 = -std::f64::consts::FRAC_PI_2;

/// Player must be within ~2 tiles of the pell to actually get bonked / block it
/// (`MusterYard.tsx` `BONK_R2 = 2*2`).
pub const BONK_R2: f64 = 2.0 * 2.0;

fn lerp(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

/// Smoothstep easing (`MusterYard.tsx` `smooth`).
fn smooth(k: f64) -> f64 {
    k * k * (3.0 - 2.0 * k)
}

/// The pell arm's yaw (radians about Y) at cycle-time `tc` ∈ [0, CYCLE) — the
/// deterministic quintain animation: rest → wound (eased) → strike-sweep to the front
/// (eased) → recover back to rest (eased). 1:1 with `MusterYard.tsx` L114-122.
pub fn pell_arm_yaw(tc: f64) -> f64 {
    if tc < REST {
        YAW_REST
    } else if tc < STRIKE_START {
        lerp(YAW_REST, YAW_WOUND, smooth((tc - REST) / WINDUP))
    } else if tc < STRIKE_END {
        lerp(YAW_WOUND, YAW_FRONT, smooth((tc - STRIKE_START) / STRIKE))
    } else {
        lerp(YAW_FRONT, YAW_REST, smooth((tc - STRIKE_END) / RECOVER))
    }
}

/// Cycle-time for a pell with `seed` at absolute time `now`, wrapped into [0, CYCLE).
/// Phase-offsets each pell by `seed * CYCLE` so neighbouring quintains don't swing in
/// lockstep (`MusterYard.tsx` L112).
pub fn pell_cycle_time(now: f64, seed: f64) -> f64 {
    (((now - seed * CYCLE) % CYCLE) + CYCLE) % CYCLE
}

/// True while the pell arm is mid-strike (the block window) at cycle-time `tc`.
pub fn pell_in_strike(tc: f64) -> bool {
    (STRIKE_START..STRIKE_END).contains(&tc)
}

/// The recoil-wobble roll angle (radians about Z) at `now`, given the wobble decays
/// at `wobble_until`. Damped sinusoid `sin(w·28)·0.14·w` where `w` is the remaining
/// wobble fraction (`MusterYard.tsx` L107-108). 0 once settled.
pub fn wobble_angle(now: f64, wobble_until: f64) -> f64 {
    let w = ((wobble_until - now) / WOBBLE).max(0.0);
    (w * 28.0).sin() * 0.14 * w
}

/// The straw hit-flash intensity (0..0.6 emissive add) at `now`, decaying over
/// `HURT_FLASH` from `hurt_flash_until` (`MusterYard.tsx` L105-106: `flash·0.6`).
pub fn flash_intensity(now: f64, hurt_flash_until: f64) -> f64 {
    let flash = ((hurt_flash_until - now) / HURT_FLASH).max(0.0);
    flash * 0.6
}

// ─── DummyState (dummyStore.ts) ───────────────────────────────────────────────────

/// One practice dummy — `oreStore` minus HP/reward, plus an `is_pell` flag. Grid
/// coords; `y` is the tile top under it (the game crate seats it). Never dies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DummyState {
    pub x: f64,
    pub z: f64,
    /// Phase seed for the pell swing cycle (and any per-dummy variation).
    pub seed: f64,
    /// Sim-time the straw hit-flash decays at (set on a hit).
    pub hurt_flash_until: f64,
    /// Sim-time the recoil wobble decays at (set on a hit).
    pub wobble_until: f64,
    /// The one quintain per yard that swings an arm to drill blocking.
    pub is_pell: bool,
}

impl DummyState {
    pub fn new(x: f64, z: f64, seed: f64, is_pell: bool) -> Self {
        DummyState { x, z, seed, hurt_flash_until: 0.0, wobble_until: 0.0, is_pell }
    }

    /// Register a hit: brief straw-flash + recoil wobble. No HP, no reward
    /// (`dummyStore.ts` `damageDummy`).
    pub fn damage(&mut self, now: f64) {
        self.hurt_flash_until = now + HURT_FLASH;
        self.wobble_until = now + WOBBLE;
    }

    /// True if a circle of radius `r` at (x,z) overlaps this dummy's post — the
    /// player-vs-dummy collision (`dummyStore.ts` `dummyCollidesAt`).
    pub fn collides_at(&self, x: f64, z: f64, r: f64) -> bool {
        let dx = x - self.x;
        let dz = z - self.z;
        let rsum = r + COLLISION_RADIUS;
        dx * dx + dz * dz < rsum * rsum
    }
}

// ─── Muster-yard placement (MusterYard.tsx YARDS) ─────────────────────────────────

/// One dummy placement, authored in BASE coords (`MusterYard.tsx` `YARDS`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DummySpec {
    /// BASE-space (x, z) — converted to the enlarged map via `shift_to_centre`.
    pub base: (f64, f64),
    pub seed: f64,
    pub is_pell: bool,
}

/// The muster-yard dummy table (`MusterYard.tsx` `YARDS[0].dummies`): a pell + a plain
/// target flanking the spawn (south), and a pair by the north gate. BASE coords.
pub const DUMMY_SPECS: [DummySpec; 4] = [
    DummySpec { base: (68.0, 58.0), seed: 0.2, is_pell: true },
    DummySpec { base: (76.0, 58.0), seed: 0.55, is_pell: false },
    DummySpec { base: (68.0, 50.0), seed: 0.36, is_pell: false },
    DummySpec { base: (76.0, 50.0), seed: 0.68, is_pell: false },
];

/// The muster-yard signpost placement (`MusterYard.tsx` `YARDS[0].signpost`), BASE coords.
pub const SIGNPOST_BASE: (f64, f64) = (75.0, 62.0);

/// Build the muster-yard dummies in enlarged-map grid coords (BASE → `shift_to_centre`,
/// the castle-attached idiom). `y`/deadlines are left for the game crate to seat/clear.
pub fn muster_dummies() -> Vec<DummyState> {
    DUMMY_SPECS
        .iter()
        .map(|d| {
            let (x, z) = shift_to_centre(d.base.0, d.base.1);
            DummyState::new(x, z, d.seed, d.is_pell)
        })
        .collect()
}

/// The muster-yard signpost position in enlarged-map grid coords (BASE → `shift_to_centre`).
pub fn muster_signpost() -> (f64, f64) {
    shift_to_centre(SIGNPOST_BASE.0, SIGNPOST_BASE.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city_plan::is_inside_castle;

    #[test]
    fn a_hit_flashes_and_wobbles_but_never_dies() {
        let mut d = DummyState::new(0.0, 0.0, 0.2, false);
        assert_eq!(d.hurt_flash_until, 0.0);
        d.damage(10.0);
        assert!((d.hurt_flash_until - 10.18).abs() < 1e-9, "flash decays at now+0.18");
        assert!((d.wobble_until - 10.5).abs() < 1e-9, "wobble decays at now+0.5");
        // No HP field exists — the dummy is indestructible by construction.
    }

    #[test]
    fn flash_and_wobble_decay_to_zero() {
        let mut d = DummyState::new(0.0, 0.0, 0.2, false);
        d.damage(0.0);
        // Right after the hit: both active.
        assert!(flash_intensity(0.0, d.hurt_flash_until) > 0.0);
        assert!(wobble_angle(0.01, d.wobble_until).abs() >= 0.0); // defined
        // Past both windows: settled.
        assert_eq!(flash_intensity(1.0, d.hurt_flash_until), 0.0, "flash settled");
        assert_eq!(wobble_angle(1.0, d.wobble_until), 0.0, "wobble settled");
        // Peak flash is 0.6 at the instant of the hit.
        assert!((flash_intensity(0.0, d.hurt_flash_until) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn collision_is_a_circle_of_the_post_radius() {
        let d = DummyState::new(5.0, 5.0, 0.0, false);
        // Player radius 0.3 just touching at distance 0.3 + 0.28 = 0.58.
        assert!(d.collides_at(5.5, 5.0, 0.3), "0.5 < 0.58 → overlap");
        assert!(!d.collides_at(6.0, 5.0, 0.3), "1.0 > 0.58 → clear");
    }

    #[test]
    fn pell_yaw_walks_the_swing_cycle() {
        // Rest: flat at YAW_REST.
        assert!((pell_arm_yaw(0.0) - YAW_REST).abs() < 1e-9);
        assert!((pell_arm_yaw(REST - 0.01) - YAW_REST).abs() < 1e-9);
        // Top of the wind-up: at YAW_WOUND.
        assert!((pell_arm_yaw(STRIKE_START) - YAW_WOUND).abs() < 1e-9, "wound by strike start");
        // End of the strike: swept to the front.
        assert!((pell_arm_yaw(STRIKE_END) - YAW_FRONT).abs() < 1e-9, "at front by strike end");
        // End of recovery: back to rest.
        assert!((pell_arm_yaw(CYCLE) - YAW_REST).abs() < 1e-6, "recovered to rest");
        // The strike window flag matches the timings.
        assert!(!pell_in_strike(REST), "resting, not striking");
        assert!(pell_in_strike((STRIKE_START + STRIKE_END) / 2.0), "mid-strike");
        assert!(!pell_in_strike(STRIKE_END + 0.01), "recovering, not striking");
    }

    #[test]
    fn pell_cycle_time_wraps_and_phase_offsets_by_seed() {
        // Wrap into [0, CYCLE).
        let tc = pell_cycle_time(CYCLE * 3.5, 0.0);
        assert!((0.0..CYCLE).contains(&tc));
        assert!((tc - CYCLE * 0.5).abs() < 1e-9, "3.5 cycles → half a cycle in");
        // Two different seeds land at different phases for the same `now`.
        let a = pell_cycle_time(1.0, 0.2);
        let b = pell_cycle_time(1.0, 0.55);
        assert!((a - b).abs() > 1e-6, "seeds phase-offset the pells");
    }

    #[test]
    fn muster_props_land_inside_the_castle_courtyard() {
        // Every dummy + the signpost sits inside the wall bounds (the muster ground is
        // a courtyard staging area, so placements are inside `is_inside_castle`).
        for d in muster_dummies() {
            assert!(is_inside_castle(d.x, d.z), "dummy ({},{}) inside the walls", d.x, d.z);
        }
        let (sx, sz) = muster_signpost();
        assert!(is_inside_castle(sx, sz), "signpost ({sx},{sz}) inside the walls");
        // Exactly one pell among the dummies (the quintain).
        assert_eq!(muster_dummies().iter().filter(|d| d.is_pell).count(), 1, "one quintain per yard");
    }
}
