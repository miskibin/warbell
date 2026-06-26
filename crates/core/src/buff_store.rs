//! Port of src/world/buffStore.ts — three short timed buffs (resist / power /
//! haste) and the gameplay multipliers the combat/movement hot paths read.
//!
//! Expiry is lazy: a buff is active while `until > now`. The TS read the clock
//! internally via `performance.now()*0.001`; per the parity brief the clock is an
//! EXPLICIT `now: f64` (SECONDS) argument on every method here, so the logic is
//! deterministic. `applyBuff` takes `duration_ms` (matching the TS ms duration)
//! and stores `until = now + duration_ms/1000`. The subscribe/notify fan-out is
//! HUD-only → SKIPPED (the HUD polls `active_buffs(now)` each frame in TS too).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuffKind {
    Resist,
    Power,
    Haste,
}

impl BuffKind {
    /// Display label (single source, mirrors BUFF_LABEL in the TS).
    pub fn label(self) -> &'static str {
        match self {
            BuffKind::Resist => "Resist",
            BuffKind::Power => "Power",
            BuffKind::Haste => "Haste",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Buff {
    /// wall-clock (sec) the buff expires; 0 = inactive
    until: f64,
    /// multiplier magnitude (e.g. resist 0.6, power 1.4); read only while active
    mag: f64,
    /// full granted duration (sec); HUD uses it for the countdown bar ratio
    full_sec: f64,
}

impl Buff {
    const fn inactive() -> Self {
        Self { until: 0.0, mag: 1.0, full_sec: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveBuff {
    pub kind: BuffKind,
    /// seconds remaining
    pub remain: f64,
    /// full granted duration (sec) — for the HUD countdown ratio
    pub full_sec: f64,
    /// multiplier magnitude (resist 0.6, power 1.4, haste 1.3, …) — lets the HUD spell out what the
    /// buff actually does (e.g. "+40% damage") instead of a bare timer.
    pub mag: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuffStore {
    resist: Buff,
    power: Buff,
    haste: Buff,
}

impl Default for BuffStore {
    fn default() -> Self {
        Self {
            resist: Buff::inactive(),
            power: Buff::inactive(),
            haste: Buff::inactive(),
        }
    }
}

impl BuffStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn slot(&self, kind: BuffKind) -> &Buff {
        match kind {
            BuffKind::Resist => &self.resist,
            BuffKind::Power => &self.power,
            BuffKind::Haste => &self.haste,
        }
    }

    fn slot_mut(&mut self, kind: BuffKind) -> &mut Buff {
        match kind {
            BuffKind::Resist => &mut self.resist,
            BuffKind::Power => &mut self.power,
            BuffKind::Haste => &mut self.haste,
        }
    }

    fn is_active(&self, kind: BuffKind, now: f64) -> bool {
        self.slot(kind).until > now
    }

    /// Grant (or refresh) a buff for `duration_ms` with multiplier `mag` at time
    /// `now` (seconds). Re-applying overwrites until/mag/full_sec.
    pub fn apply_buff(&mut self, kind: BuffKind, duration_ms: f64, mag: f64, now: f64) {
        let b = self.slot_mut(kind);
        b.until = now + duration_ms / 1000.0;
        b.mag = mag;
        b.full_sec = duration_ms / 1000.0;
    }

    /// Incoming-damage multiplier (resist → <1 while active, else 1).
    pub fn damage_taken_mult(&self, now: f64) -> f64 {
        if self.is_active(BuffKind::Resist, now) {
            self.resist.mag
        } else {
            1.0
        }
    }

    /// Outgoing-damage multiplier (power → >1 while active, else 1).
    pub fn damage_dealt_mult(&self, now: f64) -> f64 {
        if self.is_active(BuffKind::Power, now) {
            self.power.mag
        } else {
            1.0
        }
    }

    /// Move-speed multiplier (haste → >1 while active, else 1).
    pub fn speed_mult(&self, now: f64) -> f64 {
        if self.is_active(BuffKind::Haste, now) {
            self.haste.mag
        } else {
            1.0
        }
    }

    /// Active buffs with remaining seconds, for the HUD. Iteration order mirrors
    /// the TS `Object.keys` order: resist, power, haste.
    pub fn active_buffs(&self, now: f64) -> Vec<ActiveBuff> {
        let mut out = Vec::new();
        for kind in [BuffKind::Resist, BuffKind::Power, BuffKind::Haste] {
            let b = self.slot(kind);
            let remain = b.until - now;
            if remain > 0.0 {
                out.push(ActiveBuff { kind, remain, full_sec: b.full_sec, mag: b.mag });
            }
        }
        out
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    // Port of src/world/buffStore.test.ts. The TS read the clock via
    // performance.now(); here every method takes an explicit `now` (seconds), so
    // tests pin a fixed clock. duration-0 expires when applied and read at the
    // same instant (until == now, and active requires until > now).
    use super::*;

    #[test]
    fn multipliers_are_neutral_with_no_buffs() {
        let s = BuffStore::new();
        let now = 100.0;
        assert_eq!(s.damage_taken_mult(now), 1.0);
        assert_eq!(s.damage_dealt_mult(now), 1.0);
        assert_eq!(s.speed_mult(now), 1.0);
        assert_eq!(s.active_buffs(now), vec![]);
    }

    #[test]
    fn resist_lowers_damage_taken_while_active() {
        let mut s = BuffStore::new();
        let now = 100.0;
        s.apply_buff(BuffKind::Resist, 1000.0, 0.6, now);
        assert_eq!(s.damage_taken_mult(now), 0.6);
        assert_eq!(s.damage_dealt_mult(now), 1.0); // unrelated buffs stay neutral
    }

    #[test]
    fn power_raises_dealt_haste_raises_speed() {
        let mut s = BuffStore::new();
        let now = 100.0;
        s.apply_buff(BuffKind::Power, 1000.0, 1.4, now);
        s.apply_buff(BuffKind::Haste, 1000.0, 1.3, now);
        assert_eq!(s.damage_dealt_mult(now), 1.4);
        assert_eq!(s.speed_mult(now), 1.3);
    }

    #[test]
    fn a_buff_expires_after_its_duration() {
        let mut s = BuffStore::new();
        let now = 100.0;
        s.apply_buff(BuffKind::Resist, 0.0, 0.6, now); // already expired on read
        assert_eq!(s.damage_taken_mult(now), 1.0);
    }

    #[test]
    fn reapplying_refreshes_multiplier_and_keeps_active() {
        let mut s = BuffStore::new();
        let now = 100.0;
        s.apply_buff(BuffKind::Power, 0.0, 1.4, now); // expired
        s.apply_buff(BuffKind::Power, 1000.0, 1.5, now); // fresh, new mag
        assert_eq!(s.damage_dealt_mult(now), 1.5);
    }

    #[test]
    fn active_buffs_lists_active_with_remaining_seconds() {
        let mut s = BuffStore::new();
        let now = 100.0;
        s.apply_buff(BuffKind::Haste, 2000.0, 1.3, now);
        let active = s.active_buffs(now);
        assert_eq!(active.iter().map(|b| b.kind).collect::<Vec<_>>(), vec![BuffKind::Haste]);
        assert!(active[0].remain > 0.0);
        assert!(active[0].remain <= 2.0);
    }

    #[test]
    fn reports_full_granted_duration_for_hud_bar_ratio() {
        let mut s = BuffStore::new();
        let now = 100.0;
        s.apply_buff(BuffKind::Resist, 8000.0, 0.6, now);
        let active = s.active_buffs(now);
        assert_eq!(active[0].full_sec, 8.0); // 8000ms regardless of buff kind
    }
}
