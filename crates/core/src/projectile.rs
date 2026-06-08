//! Port of src/world/projectileStore.ts — the homing-bolt math for the ork shaman.
//!
//! The TS store keeps a module-global `bolts[]` and, in `stepProjectiles`, reads
//! each bolt's live target position out of the player/ork stores and applies
//! damage through those stores' mutators. That cross-store wiring is the ECS
//! layer's job; here we keep ONLY the pure per-bolt integration:
//!
//!   - homing: advance toward the target's current position at `speed`,
//!   - hit test: within `HIT_RADIUS` → impact,
//!   - lifetime: `ttl` (seconds) and `max_range` (distance) → fizzle.
//!
//! `step_bolt` takes the live target pose (resolved by the caller) + dt and
//! returns the bolt's new state plus an outcome the caller acts on (apply
//! damage / despawn). Dep-free f64, mirrors the TS numbers 1:1.

/// A bolt may target the player or a specific ork. The caller resolves this into
/// a concrete target pose + alive flag before stepping (mirrors the TS
/// `targetPos()` which read the live store).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltTarget {
    Player,
    Ork,
}

/// Who fired the bolt — drives its colour at the render layer (ork = arcane
/// purple, defender = bright cyan). Pure-side it's carried, not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltTeam {
    Ork,
    Defender,
}

/// Default flight speed (tiles/s) when none is supplied — TS `opts.speed ?? 9`.
pub const BOLT_SPEED: f64 = 9.0;
/// Default lifetime (s) — TS `ttl: 3`.
pub const BOLT_TTL: f64 = 3.0;
/// Default max travel distance before a bolt fizzles — TS `opts.maxRange ?? 40`.
pub const BOLT_MAX_RANGE: f64 = 40.0;
/// Impact radius — within this of the target, the bolt connects. TS `HIT_RADIUS`.
pub const HIT_RADIUS: f64 = 0.6;

/// A live homing bolt. Mirrors the TS `Bolt` interface (minus the render `id`,
/// which the ECS layer doesn't need — entities are identity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bolt {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub target: BoltTarget,
    pub team: BoltTeam,
    pub speed: f64,
    pub damage: f64,
    /// seconds of life remaining
    pub ttl: f64,
    /// distance the bolt may travel before it fizzles short of its target
    pub max_range: f64,
    /// distance travelled so far
    pub traveled: f64,
    /// where it was fired from — the direction a shield blocks against
    pub origin_x: f64,
    pub origin_z: f64,
}

impl Bolt {
    /// Spawn a bolt at `(x,y,z)` homing on `target`, dealing `damage`. Speed,
    /// team and max-range default to the TS constants. Mirrors `spawnBolt`.
    pub fn new(x: f64, y: f64, z: f64, target: BoltTarget, damage: f64) -> Self {
        Bolt {
            x,
            y,
            z,
            target,
            team: BoltTeam::Ork,
            speed: BOLT_SPEED,
            damage,
            ttl: BOLT_TTL,
            max_range: BOLT_MAX_RANGE,
            traveled: 0.0,
            origin_x: x,
            origin_z: z,
        }
    }

    /// Builder: override the firing team (defaults to `Ork`).
    pub fn with_team(mut self, team: BoltTeam) -> Self {
        self.team = team;
        self
    }

    /// Builder: override flight speed.
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// Builder: override max travel range.
    pub fn with_max_range(mut self, max_range: f64) -> Self {
        self.max_range = max_range;
        self
    }
}

/// What `step_bolt` decided this tick — the caller acts on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltOutcome {
    /// Still flying — keep the bolt.
    Flying,
    /// Reached the target — apply `damage` then despawn the bolt.
    Hit,
    /// Expired (ttl/range) or the target is gone — despawn, no damage.
    Fizzle,
}

/// The live target pose for a bolt, resolved by the caller from the ECS world
/// before stepping (the TS `targetPos()` equivalent). The TS aims at the
/// target's *eye* (`y + 1`); the caller bakes that offset in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetPose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub alive: bool,
}

/// Advance one bolt toward `target` by `dt`. Mirrors the per-bolt body of the TS
/// `stepProjectiles` loop:
///   1. age the bolt (`ttl -= dt`);
///   2. if the target is dead OR ttl elapsed → `Fizzle` (no damage);
///   3. if within `HIT_RADIUS` → `Hit` (caller applies `bolt.damage`);
///   4. else home one `speed*dt` step toward the target; if it has now flown its
///      full `max_range` without connecting → `Fizzle`.
///
/// Terrain-height clamping (the TS `floor` raise so bolts don't clip hills) is a
/// render concern and is applied by the ECS layer, not here.
pub fn step_bolt(bolt: &mut Bolt, target: &TargetPose, dt: f64) -> BoltOutcome {
    bolt.ttl -= dt;
    // Target gone (dead) or bolt expired → fizzle.
    if !target.alive || bolt.ttl <= 0.0 {
        return BoltOutcome::Fizzle;
    }
    let dx = target.x - bolt.x;
    let dy = target.y - bolt.y;
    let dz = target.z - bolt.z;
    let len = (dx * dx + dy * dy + dz * dz).sqrt();
    if len < HIT_RADIUS {
        return BoltOutcome::Hit;
    }
    let step = bolt.speed * dt;
    // Fizzle if it has flown its full range without connecting (lets fast/distant
    // targets outrun a bolt instead of every shot being a guaranteed hit).
    bolt.traveled += step;
    if bolt.traveled >= bolt.max_range {
        return BoltOutcome::Fizzle;
    }
    bolt.x += (dx / len) * step;
    bolt.y += (dy / len) * step;
    bolt.z += (dz / len) * step;
    BoltOutcome::Flying
}

#[cfg(test)]
mod tests {
    // Port of src/world/projectileStore.test.ts. The TS tests drove the live
    // player/ork stores; here the target pose is passed explicitly. Damage
    // application is the ECS layer's job (tested in tests/sim.rs), so these pin
    // the pure homing/impact/fizzle decisions.
    use super::*;

    fn alive_at(x: f64, y: f64, z: f64) -> TargetPose {
        TargetPose { x, y, z, alive: true }
    }

    #[test]
    fn advances_a_bolt_toward_its_target_without_removing_it() {
        // Player at x=48 (TS spawn), bolt fired from x=40 → should move toward it.
        let mut b = Bolt::new(40.0, 1.0, 36.0, BoltTarget::Player, 10.0);
        let out = step_bolt(&mut b, &alive_at(48.0, 1.0, 36.0), 0.1);
        assert_eq!(out, BoltOutcome::Flying);
        assert!(b.x > 40.0, "moved toward player at x=48 (got {})", b.x);
    }

    #[test]
    fn damages_the_player_on_arrival_and_consumes_the_bolt() {
        // Bolt spawned already on top of the target → Hit immediately.
        let mut b = Bolt::new(48.0, 1.0, 36.0, BoltTarget::Player, 15.0);
        let out = step_bolt(&mut b, &alive_at(48.0, 1.0, 36.0), 0.1);
        assert_eq!(out, BoltOutcome::Hit);
    }

    #[test]
    fn damages_an_ork_target_on_arrival() {
        let mut b = Bolt::new(10.0, 1.0, 20.0, BoltTarget::Ork, 25.0);
        let out = step_bolt(&mut b, &alive_at(10.0, 1.0, 20.0), 0.1);
        assert_eq!(out, BoltOutcome::Hit);
    }

    #[test]
    fn expires_a_bolt_past_its_ttl_without_dealing_damage() {
        // ttl starts at 3; one 4 s step ages it out before it can reach the target.
        let mut b = Bolt::new(0.0, 5.0, 0.0, BoltTarget::Player, 99.0);
        let out = step_bolt(&mut b, &alive_at(48.0, 1.0, 36.0), 4.0);
        assert_eq!(out, BoltOutcome::Fizzle);
    }

    #[test]
    fn drops_a_bolt_whose_target_is_already_dead() {
        let mut b = Bolt::new(10.0, 1.0, 20.0, BoltTarget::Ork, 25.0);
        let dead = TargetPose { x: 10.0, y: 1.0, z: 20.0, alive: false };
        let out = step_bolt(&mut b, &dead, 0.1);
        assert_eq!(out, BoltOutcome::Fizzle);
    }

    #[test]
    fn fizzles_once_it_has_flown_its_full_range() {
        // Target far past max_range straight ahead: the bolt should run out of
        // range and fizzle rather than guaranteeing a hit.
        let mut b = Bolt::new(0.0, 1.0, 0.0, BoltTarget::Player, 10.0)
            .with_max_range(5.0)
            .with_speed(100.0);
        // One big step (100*0.1 = 10 traveled) exceeds the 5-tile range.
        let out = step_bolt(&mut b, &alive_at(1000.0, 1.0, 0.0), 0.1);
        assert_eq!(out, BoltOutcome::Fizzle);
    }

    #[test]
    fn defaults_match_the_ts_constants() {
        let b = Bolt::new(0.0, 0.0, 0.0, BoltTarget::Player, 5.0);
        assert_eq!(b.speed, 9.0);
        assert_eq!(b.ttl, 3.0);
        assert_eq!(b.max_range, 40.0);
        assert_eq!(b.team, BoltTeam::Ork);
    }
}
