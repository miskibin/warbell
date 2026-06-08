//! Port of src/world/blockStore.ts — shield-block stamina + frontal mitigation.
//!
//! Hold right-mouse to raise the shield: frontal hits are largely negated, but a
//! stamina bar drains while held and on each blocked hit; empty → the shield is
//! forced down and locked until stamina recovers to a threshold. This stops the
//! player from turtling permanently.
//!
//! The TS store keeps `wantBlock`/`blocking`/`stamina`/`locked`/`regenPause` and
//! advances them in Character.tsx's `useFrame`; `damagePlayer()` calls
//! `absorbBlockedHit()` on a blocked frontal hit. Here the same state lives in
//! `BlockState` with a pure `tick` (advance per dt) + `try_block` (resolve one
//! incoming hit). The ECS layer (`crates/game`) owns the RMB input + wiring the
//! resolved reduction into each `damagePlayer` site.

/// Full stamina (one bar). `BLOCK_STAMINA_MAX` in blockStore.ts.
pub const BLOCK_STAMINA_MAX: f64 = 1.0;
/// Stamina drained per second while the shield is held up (~3.3 s of holding).
pub const BLOCK_DRAIN_HOLD: f64 = 0.3;
/// Extra stamina drained each time a hit is blocked.
pub const BLOCK_DRAIN_PER_HIT: f64 = 0.18;
/// Stamina regained per second once recovering.
pub const BLOCK_REGEN: f64 = 0.34;
/// Seconds of no block activity before stamina may regen again.
pub const BLOCK_REGEN_DELAY: f64 = 0.6;
/// Stamina must refill to here before a locked shield unlocks.
pub const BLOCK_RECOVER_THRESHOLD: f64 = 0.25;
/// Fraction of a frontal hit's damage negated by a successful block (15% leaks).
pub const BLOCK_REDUCTION: f64 = 0.85;
/// cos(~72°) — the front arc the raised shield covers.
pub const BLOCK_CONE_DOT: f64 = 0.3;

/// Shield-block state — the Rust counterpart of blockStore.ts's `BlockState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockState {
    /// Right-mouse held this frame (set from input).
    pub want_block: bool,
    /// Actually blocking this frame (resolved by `tick`).
    pub blocking: bool,
    /// Stamina, 0..1.
    pub stamina: f64,
    /// True once stamina hit 0; blocks disabled until it recovers to the threshold.
    pub locked: bool,
    /// Seconds remaining before stamina may regen again.
    pub regen_pause: f64,
}

impl Default for BlockState {
    fn default() -> Self {
        BlockState {
            want_block: false,
            blocking: false,
            stamina: BLOCK_STAMINA_MAX,
            locked: false,
            regen_pause: 0.0,
        }
    }
}

impl BlockState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Full reset to a fresh run (the TS `resetBlock`).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Advance one tick. `want` = RMB held; `alive` = the player is alive. Resolves
    /// `blocking`, drains while up, and regenerates (after the pause) when not — the
    /// per-frame block update from Character.tsx, distilled to the documented rules.
    pub fn tick(&mut self, dt: f64, want: bool, alive: bool) {
        self.want_block = want;
        // The shield is up only if wanted, not locked, with stamina, and alive.
        let active = want && !self.locked && self.stamina > 0.0 && alive;
        self.blocking = active;
        if active {
            self.stamina = (self.stamina - BLOCK_DRAIN_HOLD * dt).max(0.0);
            self.regen_pause = BLOCK_REGEN_DELAY;
            if self.stamina <= 0.0 {
                self.locked = true;
                self.blocking = false;
            }
            return;
        }
        // Recovering: count down the pause, then regen; unlock at the threshold.
        if self.regen_pause > 0.0 {
            self.regen_pause = (self.regen_pause - dt).max(0.0);
        } else if self.stamina < BLOCK_STAMINA_MAX {
            self.stamina = (self.stamina + BLOCK_REGEN * dt).min(BLOCK_STAMINA_MAX);
        }
        if self.locked && self.stamina >= BLOCK_RECOVER_THRESHOLD {
            self.locked = false;
        }
    }

    /// A blocked frontal hit landed: drain a chunk, pause regen, lock if it empties
    /// (the TS `absorbBlockedHit`).
    pub fn absorb_blocked_hit(&mut self) {
        self.stamina = (self.stamina - BLOCK_DRAIN_PER_HIT).max(0.0);
        self.regen_pause = BLOCK_REGEN_DELAY;
        if self.stamina <= 0.0 {
            self.locked = true;
            self.blocking = false;
        }
    }

    /// Resolve an incoming hit from world-grid `(ax, az)` against a player at
    /// `(px, pz)` facing yaw `facing`. If the shield is up AND the hit is inside the
    /// frontal cone, absorb a stamina chunk and return `true` (caller cuts the
    /// damage by `BLOCK_REDUCTION`); otherwise return `false` (full damage).
    pub fn try_block(&mut self, px: f64, pz: f64, facing: f64, ax: f64, az: f64) -> bool {
        if self.blocking && hit_is_frontal(px, pz, facing, ax, az) {
            self.absorb_blocked_hit();
            true
        } else {
            false
        }
    }
}

/// Is an incoming hit from `(ax, az)` inside the shield's frontal cone, given a
/// player at `(px, pz)` facing yaw `facing`? Forward = (sin facing, cos facing)
/// (the hero's facing convention — see player_swing/sync_player_visual). A hit
/// exactly on the player counts as frontal.
pub fn hit_is_frontal(px: f64, pz: f64, facing: f64, ax: f64, az: f64) -> bool {
    let dx = ax - px;
    let dz = az - pz;
    let len = (dx * dx + dz * dz).sqrt();
    if len <= 1e-9 {
        return true;
    }
    let (fx, fz) = (facing.sin(), facing.cos());
    (fx * dx + fz * dz) / len >= BLOCK_CONE_DOT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_is_full_and_unlocked() {
        let b = BlockState::new();
        assert_eq!(b.stamina, BLOCK_STAMINA_MAX);
        assert!(!b.locked);
        assert!(!b.blocking);
    }

    #[test]
    fn holding_block_drains_stamina() {
        let mut b = BlockState::new();
        b.tick(1.0, true, true);
        assert!(b.blocking);
        assert!((b.stamina - (BLOCK_STAMINA_MAX - BLOCK_DRAIN_HOLD)).abs() < 1e-9);
    }

    /// Hold the shield up until it drains to empty and locks (returns whether it
    /// locked within a generous window). NOTE: once locked, *holding* lets it regen
    /// (the anti-turtle sawtooth — `want` stays true but `locked` forces it down), so
    /// the lock must be caught at the depletion tick, not after a fixed hold.
    fn hold_until_locked(b: &mut BlockState) -> bool {
        for _ in 0..600 {
            b.tick(1.0 / 60.0, true, true);
            if b.locked {
                return true;
            }
        }
        false
    }

    #[test]
    fn draining_to_empty_locks_the_shield() {
        let mut b = BlockState::new();
        assert!(hold_until_locked(&mut b), "holding the shield drains it to empty");
        assert_eq!(b.stamina, 0.0, "locks at empty");
        assert!(b.locked);
        assert!(!b.blocking, "a locked shield is not blocking");
    }

    #[test]
    fn locked_shield_regens_and_unlocks_at_threshold() {
        let mut b = BlockState::new();
        assert!(hold_until_locked(&mut b));
        // Release and let it recover; after the pause it regens, and unlocks once it
        // passes BLOCK_RECOVER_THRESHOLD.
        for _ in 0..200 {
            b.tick(1.0 / 60.0, false, true);
            if !b.locked {
                break;
            }
        }
        assert!(!b.locked, "should unlock after recovering to the threshold");
        assert!(b.stamina >= BLOCK_RECOVER_THRESHOLD);
    }

    #[test]
    fn frontal_hit_is_blocked_and_drains_extra() {
        let mut b = BlockState::new();
        b.tick(1.0 / 60.0, true, true); // raise the shield
        let s0 = b.stamina;
        // Player at origin facing +Z (facing 0 → forward (0,1)); attacker straight ahead.
        let blocked = b.try_block(0.0, 0.0, 0.0, 0.0, 5.0);
        assert!(blocked);
        assert!(b.stamina <= s0 - BLOCK_DRAIN_PER_HIT + 1e-9);
    }

    #[test]
    fn hit_from_behind_is_not_blocked() {
        let mut b = BlockState::new();
        b.tick(1.0 / 60.0, true, true);
        // Facing +Z; attacker directly behind (−Z) is outside the front cone.
        let blocked = b.try_block(0.0, 0.0, 0.0, 0.0, -5.0);
        assert!(!blocked, "a back hit slips past the shield");
    }

    #[test]
    fn not_holding_means_no_block() {
        let mut b = BlockState::new();
        b.tick(1.0 / 60.0, false, true);
        assert!(!b.try_block(0.0, 0.0, 0.0, 0.0, 5.0));
    }

    #[test]
    fn frontal_cone_boundary() {
        // cos(72°) ≈ 0.309 > 0.3, so a hit ~72° off-forward is just inside.
        let f = 0.0; // forward (0,1)
        // A point at 70° from forward (well inside).
        let ang = 70f64.to_radians();
        assert!(hit_is_frontal(0.0, 0.0, f, ang.sin(), ang.cos()));
        // A point at 80° from forward (outside the ~72° cone).
        let ang = 80f64.to_radians();
        assert!(!hit_is_frontal(0.0, 0.0, f, ang.sin(), ang.cos()));
    }
}
