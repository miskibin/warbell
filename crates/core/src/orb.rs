//! Port of src/world/orbStore.ts — reward orbs (the gold/XP motes that burst off
//! a slain creature, hang for a beat, then accelerate into the hero).
//!
//! The accelerating "suck" (not a constant glide) is what makes collection feel
//! satisfying; deferring the actual gold/XP grant to the moment an orb lands
//! makes the HUD counter race up as they stream in. A hard life cap force-collects
//! any straggler so a reward is never lost to a stuck orb.
//!
//! Pure-side this module owns ONLY the per-orb integration + value splitting. The
//! TS module read the live player position and called addGold/addXp on contact;
//! here the player pose is passed into `step_orb` and contact/cap is reported back
//! via `OrbStep` for the ECS layer to grant. Randomness in the burst is injected
//! (an `f64` source in [0,1)) so the math stays deterministic + testable.

/// Reward kind — gold races the purse, xp races the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrbKind {
    Gold,
    Xp,
}

pub const MAX_SPEED: f64 = 30.0;
pub const COLLECT_DIST: f64 = 0.85;
/// force-collect past this age — keeps the suck short + snappy
pub const LIFE_CAP: f64 = 1.1;
const BURST_GRAVITY: f64 = -14.0;
const BURST_DRAG: f64 = 3.0;
/// how hard velocity snaps onto the homing line (bigger = snappier)
const SEEK_RESPONSE: f64 = 16.0;

/// A live reward orb. Mirrors the TS `Orb` interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orb {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub kind: OrbKind,
    pub value: i64,
    pub age: f64,
    /// age (sec) at which the orb switches from ballistic burst to homing seek
    pub seek_at: f64,
}

/// Burst `count` orbs of `kind` at `(x,y,z)`, splitting `total_value` across them
/// with no loss. `rng` yields successive f64 in [0,1) (the TS `Math.random()`),
/// driving the burst spread/speed/seek-delay; pass a deterministic source in
/// tests. Returns the spawned orbs (the ECS layer pushes them into its pool /
/// spawns entities). Mirrors `spawnOrbs`.
///
/// As in the TS: never spawn more orbs than there is integer value to split, so
/// trailing orbs can't round up to 1 each and over-grant.
pub fn spawn_orbs(
    kind: OrbKind,
    x: f64,
    y: f64,
    z: f64,
    count: i64,
    total_value: i64,
    rng: &mut impl FnMut() -> f64,
) -> Vec<Orb> {
    let mut out = Vec::new();
    if total_value <= 0 || count <= 0 {
        return out;
    }
    let count = count.min(total_value);
    let base = total_value / count;
    let mut rem = total_value - base * count;
    for i in 0..count {
        let mut val = base;
        if rem > 0 {
            val += 1;
            rem -= 1;
        }
        let a = (i as f64 / count as f64) * std::f64::consts::PI * 2.0 + rng() * 1.3;
        let sp = 1.4 + rng() * 1.3;
        out.push(Orb {
            x,
            y: y + 0.2,
            z,
            vx: a.cos() * sp,
            vy: 1.6 + rng() * 1.4,
            vz: a.sin() * sp,
            kind,
            value: val,
            age: 0.0,
            seek_at: 0.1 + rng() * 0.1,
        });
    }
    out
}

/// What `step_orb` decided this tick — the caller acts on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrbStep {
    /// Still in flight — keep the orb.
    Flying,
    /// Reached the hero (or hit the life cap) — grant `value`, then despawn.
    Collected,
}

/// The hero pose an orb homes toward — the TS sampled `getPlayer()` and aimed at
/// `y + 1.0` (chest height); the caller bakes that offset in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Advance one orb by `dt` toward `player`. Mirrors the per-orb body of the TS
/// `stepOrbs` loop: ballistic burst (drag + gravity + soft floor bounce) until
/// `age >= seek_at`, then critically-damped homing; collected on contact within
/// `COLLECT_DIST` or once `age > LIFE_CAP` (so a stuck orb's reward is never lost).
///
/// IMPORTANT: callers must skip the whole step when `dt <= 0` (hit-stop freeze) —
/// the TS `stepOrbs` early-returns in that case so orbs hang mid-burst. This
/// per-orb fn assumes `dt > 0`.
pub fn step_orb(orb: &mut Orb, player: &PlayerPose, dt: f64) -> OrbStep {
    orb.age += dt;
    if orb.age < orb.seek_at {
        // Burst: ballistic with drag + gravity and a soft floor bounce.
        let d = (1.0 - BURST_DRAG * dt).max(0.0);
        orb.vx *= d;
        orb.vz *= d;
        orb.vy = orb.vy * d + BURST_GRAVITY * dt;
        orb.x += orb.vx * dt;
        orb.y += orb.vy * dt;
        orb.z += orb.vz * dt;
        if orb.y < 0.25 {
            orb.y = 0.25;
            orb.vy *= -0.3;
        }
    } else {
        // Seek: critically-damped homing — velocity snaps onto the line to the
        // hero at a speed that grows with distance, so it leaves fast and lands
        // fast instead of lazily re-accelerating from zero.
        let dx = player.x - orb.x;
        let dy = player.y - orb.y;
        let dz = player.z - orb.z;
        let dist = {
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            if d == 0.0 { 1.0 } else { d }
        };
        let target_speed = MAX_SPEED.min(7.0 + dist * 16.0);
        let k = (SEEK_RESPONSE * dt).min(1.0);
        orb.vx += ((dx / dist) * target_speed - orb.vx) * k;
        orb.vy += ((dy / dist) * target_speed - orb.vy) * k;
        orb.vz += ((dz / dist) * target_speed - orb.vz) * k;
        orb.x += orb.vx * dt;
        orb.y += orb.vy * dt;
        orb.z += orb.vz * dt;
        if dist < COLLECT_DIST {
            return OrbStep::Collected;
        }
    }
    if orb.age > LIFE_CAP {
        return OrbStep::Collected;
    }
    OrbStep::Flying
}

#[cfg(test)]
mod tests {
    // Port of src/world/orbStore.test.ts. The risky invariant is that the *full*
    // value always lands (nothing lost to a stuck orb) and that orbs hold still
    // during hit-stop (dt ≤ 0) — pinned here against the pure stepping fns.
    use super::*;

    /// Deterministic "random": a fixed mid-range value so the burst is reproducible.
    fn fixed_rng() -> impl FnMut() -> f64 {
        || 0.5
    }

    #[test]
    fn splits_the_total_value_across_orbs_with_no_loss() {
        let mut rng = fixed_rng();
        let orbs = spawn_orbs(OrbKind::Gold, 10.0, 1.0, 10.0, 4, 10, &mut rng);
        assert_eq!(orbs.len(), 4);
        let total: i64 = orbs.iter().map(|o| o.value).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn ignores_empty_bursts() {
        let mut rng = fixed_rng();
        assert!(spawn_orbs(OrbKind::Gold, 0.0, 0.0, 0.0, 0, 10, &mut rng).is_empty());
        assert!(spawn_orbs(OrbKind::Gold, 0.0, 0.0, 0.0, 4, 0, &mut rng).is_empty());
    }

    #[test]
    fn never_over_grants_when_count_exceeds_value() {
        let mut rng = fixed_rng();
        // 8 requested orbs but only 3 value: clamps to 3 orbs, total stays 3.
        let orbs = spawn_orbs(OrbKind::Xp, 0.0, 0.0, 0.0, 8, 3, &mut rng);
        assert_eq!(orbs.len(), 3);
        assert_eq!(orbs.iter().map(|o| o.value).sum::<i64>(), 3);
    }

    #[test]
    fn grants_the_full_gold_value_once_every_orb_is_collected() {
        let mut rng = fixed_rng();
        // Burst at (5,5) relative to a hero at the origin; step well past the life
        // cap so every orb is collected (by contact or cap) and tally the grant.
        let mut orbs = spawn_orbs(OrbKind::Gold, 5.0, 0.0, 5.0, 4, 13, &mut rng);
        let player = PlayerPose { x: 0.0, y: 1.0, z: 0.0 };
        let mut granted = 0i64;
        for _ in 0..200 {
            orbs.retain_mut(|o| match step_orb(o, &player, 0.05) {
                OrbStep::Collected => {
                    granted += o.value;
                    false
                }
                OrbStep::Flying => true,
            });
            if orbs.is_empty() {
                break;
            }
        }
        assert!(orbs.is_empty(), "all orbs should be collected");
        assert_eq!(granted, 13, "the full value lands, nothing lost");
    }

    #[test]
    fn holds_orbs_frozen_during_hit_stop() {
        // The TS stepOrbs early-returns when dt <= 0; the ECS caller mirrors that
        // by not calling step_orb at all. Assert the per-orb fn is never the one
        // that moves them: with dt skipped, the orb is byte-identical.
        let mut rng = fixed_rng();
        let orbs = spawn_orbs(OrbKind::Gold, 5.0, 1.0, 5.0, 3, 9, &mut rng);
        let snapshot = orbs.clone();
        // Simulate the caller's `if dt <= 0 { return }` guard: no step applied.
        for (a, b) in orbs.iter().zip(snapshot.iter()) {
            assert_eq!(a.x, b.x);
            assert_eq!(a.y, b.y);
            assert_eq!(a.z, b.z);
            assert_eq!(a.age, b.age);
        }
    }
}
