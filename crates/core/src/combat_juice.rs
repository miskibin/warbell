//! Port of the deterministic curves in `src/world/fxStore.ts` — the trauma-based
//! screen-shake decay and the FOV-punch decay. Pure math + state advance, no Bevy:
//! the camera (`crates/game/src/player_ctl.rs`) owns applying the resulting offsets,
//! this module owns "how much shake / FOV kick is left right now".
//!
//! Two independent channels, both ports of fxStore.ts:
//!
//!   1. **Screen shake (trauma)** — `addShake(amount)` charges a 0..1 `trauma` pool;
//!      `getShake(dt)` decays it by `TRAUMA_DECAY` per second and returns the camera
//!      offset magnitude `MAX_SHAKE · trauma²` (squared so the shake ramps in sharply
//!      and tails off gently; overlapping hits stack toward the cap instead of
//!      fighting for the max). fxStore.ts L10-28.
//!
//!   2. **FOV punch** — `add_fov_kick(deg)` adds a degrees kick (clamped to `FOV_MAX`);
//!      `fov_kick(dt)` decays it linearly by `FOV_DECAY` deg/s and returns the current
//!      offset added to the base FOV. fxStore.ts L30-66 (`fovTunables`).
//!
//! The TS read live `performance.now()` and tracked the last call internally; here the
//! caller passes an explicit `dt` per advance (the Bevy frame delta), keeping the
//! struct a plain deterministic state machine that's trivially unit-testable.

/// Max positional shake offset at full trauma (fxStore.ts `MAX_SHAKE = 0.9`).
pub const MAX_SHAKE: f64 = 0.9;
/// Trauma shed per second (fxStore.ts `TRAUMA_DECAY = 2.4`).
pub const TRAUMA_DECAY: f64 = 2.4;

/// Per-event FOV-punch sizes in degrees (fxStore.ts `fovTunables`).
pub const FOV_KICK_KILL: f64 = 1.3; // takedown punch
pub const FOV_KICK_HIT: f64 = 0.1; // connecting-blow punch
pub const FOV_KICK_LAND: f64 = 1.1; // hard-landing punch
/// Cap so stacked punches never blow the view open (fxStore.ts `fovTunables.max`).
pub const FOV_MAX: f64 = 7.0;
/// Degrees of FOV kick shed per second (fxStore.ts `fovTunables.decay = 22`).
pub const FOV_DECAY: f64 = 22.0;

/// Per-event trauma charges (mirrors the `addShake(...)` call sites in Character.tsx
/// / playerStore.ts so the Bevy call sites use the same magnitudes).
pub const SHAKE_KILL: f64 = 0.55; // a takedown (Character.tsx L916)
pub const SHAKE_HIT: f64 = 0.3; // a connecting blow (Character.tsx L921/927/933)
pub const SHAKE_BLOCK: f64 = 0.1; // a shield block (playerStore.ts L169)
pub const SHAKE_PLAYER_HURT: f64 = 0.22; // the hero takes a hit (playerStore.ts L197)
pub const SHAKE_PLAYER_DEATH: f64 = 0.5; // the hero falls (playerStore.ts L197)
pub const SHAKE_KEEP_HIT: f64 = 0.25; // the keep is chipped (castleStore.ts L40)

/// Both juice channels: the trauma pool (screen shake) + the FOV kick. Held by the
/// game crate as a resource; advanced once per frame and read by the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CombatJuice {
    /// Screen-shake charge, 0..1.
    pub trauma: f64,
    /// FOV punch in degrees, 0..FOV_MAX.
    pub fov_kick: f64,
}

impl Default for CombatJuice {
    fn default() -> Self {
        CombatJuice { trauma: 0.0, fov_kick: 0.0 }
    }
}

impl CombatJuice {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear both channels (a fresh run / world remount — the TS `resetFovKick` +
    /// the implicit trauma reset).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Add trauma (`addShake`). Bigger events add more; clamped to 1 so it never runs
    /// away (overlapping hits stack toward the cap).
    pub fn add_shake(&mut self, amount: f64) {
        self.trauma = (self.trauma + amount).clamp(0.0, 1.0);
    }

    /// Punch the FOV out by `deg` (`addFovKick`), clamped to `FOV_MAX`.
    pub fn add_fov_kick(&mut self, deg: f64) {
        self.fov_kick = (self.fov_kick + deg).clamp(0.0, FOV_MAX);
    }

    /// Decay the trauma by `dt` (clamped to a 0.1 s max step like the TS) and return
    /// the current shake offset magnitude `MAX_SHAKE · trauma²` (0 when settled).
    pub fn step_shake(&mut self, dt: f64) -> f64 {
        let dt = dt.clamp(0.0, 0.1);
        if self.trauma > 0.0 {
            self.trauma = (self.trauma - TRAUMA_DECAY * dt).max(0.0);
        }
        if self.trauma <= 0.0 {
            return 0.0;
        }
        MAX_SHAKE * self.trauma * self.trauma
    }

    /// Decay the FOV kick by `dt` (clamped to a 0.1 s max step) and return the current
    /// FOV offset in degrees (0 when settled).
    pub fn step_fov(&mut self, dt: f64) -> f64 {
        let dt = dt.clamp(0.0, 0.1);
        if self.fov_kick > 0.0 {
            self.fov_kick = (self.fov_kick - FOV_DECAY * dt).max(0.0);
        }
        self.fov_kick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_juice_is_settled() {
        let mut j = CombatJuice::new();
        assert_eq!(j.step_shake(1.0 / 60.0), 0.0, "no shake at rest");
        assert_eq!(j.step_fov(1.0 / 60.0), 0.0, "no FOV kick at rest");
    }

    #[test]
    fn shake_offset_is_max_at_full_trauma_and_squared() {
        let mut j = CombatJuice::new();
        j.add_shake(1.0);
        // The very first read (dt ~0) returns MAX_SHAKE · 1² = MAX_SHAKE.
        let s = j.step_shake(0.0);
        assert!((s - MAX_SHAKE).abs() < 1e-9, "full trauma → MAX_SHAKE, got {s}");

        // Trauma squared: at trauma 0.5 the offset is MAX_SHAKE · 0.25.
        let mut k = CombatJuice::new();
        k.trauma = 0.5;
        let s = k.step_shake(0.0);
        assert!((s - MAX_SHAKE * 0.25).abs() < 1e-9, "trauma² curve, got {s}");
    }

    #[test]
    fn add_shake_clamps_at_one() {
        let mut j = CombatJuice::new();
        j.add_shake(0.8);
        j.add_shake(0.8); // would be 1.6 — clamps to 1.
        assert!((j.trauma - 1.0).abs() < 1e-9, "trauma clamps to 1, got {}", j.trauma);
    }

    #[test]
    fn shake_decays_to_zero_over_time() {
        let mut j = CombatJuice::new();
        j.add_shake(1.0);
        // TRAUMA_DECAY 2.4/s → full trauma settles in ~1/2.4 ≈ 0.42 s. Step 60 frames.
        let mut last = j.step_shake(0.0);
        for _ in 0..60 {
            let s = j.step_shake(1.0 / 60.0);
            assert!(s <= last + 1e-9, "shake is monotonically non-increasing");
            last = s;
        }
        assert_eq!(j.step_shake(1.0 / 60.0), 0.0, "shake settles to 0 within a second");
    }

    #[test]
    fn fov_kick_adds_clamps_and_decays_linearly() {
        let mut j = CombatJuice::new();
        j.add_fov_kick(FOV_KICK_KILL);
        assert!((j.fov_kick - FOV_KICK_KILL).abs() < 1e-9);

        // Clamp at FOV_MAX.
        j.add_fov_kick(100.0);
        assert!((j.fov_kick - FOV_MAX).abs() < 1e-9, "FOV kick clamps to FOV_MAX");

        // Linear decay: one second sheds FOV_DECAY degrees (but not below 0).
        let mut k = CombatJuice::new();
        k.add_fov_kick(5.0);
        let after = k.step_fov(0.1); // 0.1 s → −2.2 deg
        assert!((after - (5.0 - FOV_DECAY * 0.1)).abs() < 1e-9, "linear FOV decay, got {after}");
    }

    #[test]
    fn fov_kick_settles_to_zero() {
        let mut j = CombatJuice::new();
        j.add_fov_kick(FOV_KICK_KILL);
        for _ in 0..30 {
            j.step_fov(1.0 / 60.0);
        }
        assert_eq!(j.step_fov(1.0 / 60.0), 0.0, "FOV kick settles to 0");
    }

    #[test]
    fn dt_is_clamped_so_a_long_frame_cannot_overshoot() {
        // A huge dt (a stall) clamps to a 0.1 s step, so trauma/fov don't jump negative
        // or skip the decay curve — matches the TS `Math.min(0.1, ...)` guard.
        let mut j = CombatJuice::new();
        j.add_shake(1.0);
        j.add_fov_kick(FOV_MAX);
        j.step_shake(100.0);
        j.step_fov(100.0);
        assert!(j.trauma >= 0.0 && j.fov_kick >= 0.0);
        // One 0.1 s step sheds TRAUMA_DECAY*0.1 = 0.24 trauma and FOV_DECAY*0.1 = 2.2 deg.
        assert!((j.trauma - (1.0 - TRAUMA_DECAY * 0.1)).abs() < 1e-9);
        assert!((j.fov_kick - (FOV_MAX - FOV_DECAY * 0.1)).abs() < 1e-9);
    }

    #[test]
    fn reset_clears_both_channels() {
        let mut j = CombatJuice::new();
        j.add_shake(1.0);
        j.add_fov_kick(FOV_MAX);
        j.reset();
        assert_eq!(j, CombatJuice::default());
    }
}
