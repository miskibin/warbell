//! Port of the pure parts of src/world/villagerStore.ts + traderStore.ts and the
//! wander/state-machine math from Villager.tsx + Trader.tsx.
//!
//!   - the module-global `villagers` / `traders` arrays + create/remove/nearest
//!     helpers -> the `VillagerRoster` / `TraderRoster` structs (one instance per
//!     test, no global — parallel-safe, same convention as the other `*_store`
//!     ports). The Bevy layer instead spawns one ECS entity per roster member, so
//!     these rosters are exercised mainly by the tests; the *math* below is what
//!     the `NpcsPlugin` AI calls.
//!   - the day-phase schedule + wander-point picks (the pure functions inside the
//!     two `useFrame`s) -> `villager_scheduled_mode` / `trader_scheduled_mode`,
//!     `villager_wander_point` / `trader_wander_point`, and the `enter_state`
//!     transition. These are deterministic in `(seed/id, t)` so they need no RNG.
//!
//! Dropped (HUD/browser-only, like the other store ports): subscribe/notify, the
//! guard-combat deal-damage path (orks downing villagers is a later pass), the
//! door-open timer, and recruit/trade interaction. Villager HP/downed fields are
//! kept on the struct (the militia/heir system reads them) but no combat mutates
//! them here yet.

// ─── Villager roster ────────────────────────────────────────────────────────────

/// Villager day-schedule state (Villager.tsx `VillagerStateName`). `Rest` =
/// travelling to the door; `Home` = inside the house (hidden, idle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VillagerStateName {
    Idle,
    Wander,
    Tend,
    Rest,
    Home,
}

/// HP a militia/villager can soak before being downed (VILLAGER_MAX_HP).
pub const VILLAGER_MAX_HP: f64 = 140.0;

/// A townsperson/militia member. Mirrors `VillagerState` in villagerStore.ts;
/// pathfinding scratch (`path`/`path_index`/`path_recompute_at`) lives on the
/// Bevy AI component instead, so it's omitted here.
#[derive(Debug, Clone, Copy)]
pub struct Villager {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub facing: f64,
    pub state: VillagerStateName,
    pub state_since: f64,
    pub state_until: f64,
    pub target_x: f64,
    pub target_z: f64,
    pub home_x: f64,
    pub home_z: f64,
    pub garden_x: f64,
    pub garden_z: f64,
    pub door_x: f64,
    pub door_z: f64,
    pub seed: f64,
    pub palette_index: u32,
    pub hp: f64,
    pub max_hp: f64,
    pub downed: bool,
    /// castle-dwelling villagers double as militia (orks single them out).
    pub is_guard: bool,
    /// recruited from a trader (vs. born a townsperson) — distinct tabard.
    pub recruited: bool,
}

/// Spawn parameters for a villager (the `Omit<...>` init in `createVillager`).
/// `is_guard` is derived (castle membership) like the TS factory; pass the
/// recruited flag explicitly.
#[derive(Debug, Clone, Copy)]
pub struct VillagerInit {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub facing: f64,
    pub home_x: f64,
    pub home_z: f64,
    pub garden_x: f64,
    pub garden_z: f64,
    pub door_x: f64,
    pub door_z: f64,
    pub seed: f64,
    pub palette_index: u32,
    pub recruited: bool,
}

/// The villager roster (the module-global `villagers` array + `nextId`).
#[derive(Debug, Default)]
pub struct VillagerRoster {
    villagers: Vec<Villager>,
    next_id: u32,
}

impl VillagerRoster {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a villager (port of `createVillager`). `is_guard` is computed from the
    /// home tile being inside the castle, exactly as the TS factory does.
    pub fn create(&mut self, init: VillagerInit) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let is_guard = crate::city_plan::is_inside_castle(init.home_x, init.home_z);
        self.villagers.push(Villager {
            id,
            x: init.x,
            y: init.y,
            z: init.z,
            facing: init.facing,
            state: VillagerStateName::Idle,
            state_since: 0.0,
            state_until: 0.0,
            target_x: init.x,
            target_z: init.z,
            home_x: init.home_x,
            home_z: init.home_z,
            garden_x: init.garden_x,
            garden_z: init.garden_z,
            door_x: init.door_x,
            door_z: init.door_z,
            seed: init.seed,
            palette_index: init.palette_index,
            hp: VILLAGER_MAX_HP,
            max_hp: VILLAGER_MAX_HP,
            downed: false,
            is_guard,
            recruited: init.recruited,
        });
        id
    }

    pub fn all(&self) -> &[Villager] {
        &self.villagers
    }

    pub fn len(&self) -> usize {
        self.villagers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.villagers.is_empty()
    }

    /// Remove a villager by id (no-op if unknown). Port of `removeVillager`.
    pub fn remove(&mut self, id: u32) {
        if let Some(i) = self.villagers.iter().position(|v| v.id == id) {
            self.villagers.remove(i);
        }
    }

    pub fn reset(&mut self) {
        self.villagers.clear();
        self.next_id = 0;
    }

    /// Apply damage; returns true if this hit downs the villager (port of
    /// `damageVillager`).
    pub fn damage(&mut self, id: u32, amount: f64) -> bool {
        if let Some(v) = self.villagers.iter_mut().find(|v| v.id == id) {
            if v.downed {
                return false;
            }
            v.hp = (v.hp - amount).max(0.0);
            if v.hp <= 0.0 {
                v.downed = true;
                return true;
            }
        }
        false
    }

    /// Stand every downed villager back up at full HP (port of `reviveVillagers`).
    pub fn revive_all(&mut self) {
        for v in &mut self.villagers {
            v.downed = false;
            v.hp = v.max_hp;
        }
    }

    /// Count of villagers able to take up the blade right now (downed can't) —
    /// the run's live pool of lives (port of `getStandingVillagerCount`).
    pub fn standing_count(&self) -> usize {
        self.villagers.iter().filter(|v| !v.downed).count()
    }

    /// Nearest standing villager to a grid point, or `None` (port of
    /// `nearestVillager`). Returns the villager's id.
    pub fn nearest_standing(&self, x: f64, z: f64) -> Option<u32> {
        self.villagers
            .iter()
            .filter(|v| !v.downed)
            .min_by(|a, b| {
                let da = (a.x - x).powi(2) + (a.z - z).powi(2);
                let db = (b.x - x).powi(2) + (b.z - z).powi(2);
                da.total_cmp(&db)
            })
            .map(|v| v.id)
    }
}

// ─── Trader roster ──────────────────────────────────────────────────────────────

/// Trader loiter state (Trader.tsx). Traders never fight or fall — only the
/// counter (`Tend`) and a short stroll (`Wander`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraderStateName {
    Idle,
    Wander,
    Tend,
}

/// An independent merchant NPC. Mirrors `TraderState` in traderStore.ts;
/// pathfinding scratch lives on the Bevy AI component.
#[derive(Debug, Clone)]
pub struct Trader {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub facing: f64,
    pub state: TraderStateName,
    pub state_since: f64,
    pub state_until: f64,
    pub target_x: f64,
    pub target_z: f64,
    pub home_x: f64,
    pub home_z: f64,
    pub garden_x: f64,
    pub garden_z: f64,
    pub door_x: f64,
    pub door_z: f64,
    pub seed: f64,
    pub palette_index: u32,
    /// shop title shown when the player trades with this merchant.
    pub name: String,
}

/// Spawn parameters for a trader (the `Omit<...>` init in `createTrader`).
#[derive(Debug, Clone)]
pub struct TraderInit {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub facing: f64,
    pub home_x: f64,
    pub home_z: f64,
    pub garden_x: f64,
    pub garden_z: f64,
    pub door_x: f64,
    pub door_z: f64,
    pub seed: f64,
    pub palette_index: u32,
    pub name: String,
}

/// The trader roster (the module-global `traders` array + `nextId`).
#[derive(Debug, Default)]
pub struct TraderRoster {
    traders: Vec<Trader>,
    next_id: u32,
}

impl TraderRoster {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a trader (port of `createTrader`).
    pub fn create(&mut self, init: TraderInit) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.traders.push(Trader {
            id,
            x: init.x,
            y: init.y,
            z: init.z,
            facing: init.facing,
            state: TraderStateName::Idle,
            state_since: 0.0,
            state_until: 0.0,
            target_x: init.x,
            target_z: init.z,
            home_x: init.home_x,
            home_z: init.home_z,
            garden_x: init.garden_x,
            garden_z: init.garden_z,
            door_x: init.door_x,
            door_z: init.door_z,
            seed: init.seed,
            palette_index: init.palette_index,
            name: init.name,
        });
        id
    }

    pub fn all(&self) -> &[Trader] {
        &self.traders
    }

    pub fn len(&self) -> usize {
        self.traders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.traders.is_empty()
    }

    /// Remove a trader by id (no-op if unknown). Port of `removeTrader`.
    pub fn remove(&mut self, id: u32) {
        if let Some(i) = self.traders.iter().position(|t| t.id == id) {
            self.traders.remove(i);
        }
    }

    pub fn reset(&mut self) {
        self.traders.clear();
        self.next_id = 0;
    }

    /// Nearest trader to a grid point within `max_dist`, or `None` (port of
    /// `nearestTrader`). Returns the trader's id.
    pub fn nearest(&self, x: f64, z: f64, max_dist: f64) -> Option<u32> {
        let max_sq = max_dist * max_dist;
        self.traders
            .iter()
            .map(|t| (t.id, (t.x - x).powi(2) + (t.z - z).powi(2)))
            .filter(|(_, d)| *d < max_sq)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}

// ─── Wander / state-machine math (the pure functions from the two views) ─────────

const TWO_PI: f64 = std::f64::consts::PI * 2.0;
const PI: f64 = std::f64::consts::PI;

/// Villager wander radius (Villager.tsx `WANDER_RADIUS`).
pub const VILLAGER_WANDER_RADIUS: f64 = 3.0;
/// Trader wander radius (Trader.tsx `WANDER_RADIUS`).
pub const TRADER_WANDER_RADIUS: f64 = 2.2;

/// Villager stroll speed (Villager.tsx `SPEED`).
pub const VILLAGER_SPEED: f64 = 1.6;
/// Trader stroll speed (Trader.tsx `SPEED`).
pub const TRADER_SPEED: f64 = 1.4;

/// Distance at which a villager/trader is "arrived" at its target.
pub const ARRIVE_DIST: f64 = 0.35;
/// Distance at which a path waypoint counts as reached.
pub const WAYPOINT_DIST: f64 = 0.4;

// ─── Guard combat (Villager.tsx GUARD_* — castle militia fighting orks) ─────────────
// Castle-dwelling villagers (`is_guard`) break their routine to defend the keep:
// they aggro nearby orks within `guard_aggro`, chase at `GUARD_SPEED`, and swing for
// `guard_damage` on the cooldown. Both aggro and damage scale with the Villager-Arms
// upgrade tier (def_armor_1/2). They can be downed (hp 0) and are revived each prep.

/// Base aggro radius a guard notices an ork at (Villager.tsx `GUARD_AGGRO`).
pub const GUARD_AGGRO: f64 = 7.5;
/// Extra aggro per Villager-Arms tier (Villager.tsx `+3.5/tier`).
pub const GUARD_AGGRO_PER_TIER: f64 = 3.5;
/// How far from the keep a guard will chase before it must hold (Villager.tsx
/// `GUARD_DEFEND_RADIUS`); widened during a wave so the militia sallies further.
pub const GUARD_DEFEND_RADIUS: f64 = 12.0;
/// Wave-time multiplier on the defend radius (Villager.tsx `×1.8` during waves).
pub const GUARD_DEFEND_WAVE_MULT: f64 = 1.8;
/// Guard melee reach (Villager.tsx `GUARD_MELEE`).
pub const GUARD_MELEE: f64 = 1.45;
/// Guard chase speed when defending (Villager.tsx `GUARD_SPEED`).
pub const GUARD_SPEED: f64 = 2.4;
/// Guard swing windup (Villager.tsx `GUARD_ATTACK_DURATION`).
pub const GUARD_ATTACK_DURATION: f64 = 0.55;
/// Seconds between guard swings (Villager.tsx `GUARD_ATTACK_COOLDOWN`).
pub const GUARD_ATTACK_COOLDOWN: f64 = 1.0;
/// Base guard melee damage (Villager.tsx `GUARD_DAMAGE`).
pub const GUARD_DAMAGE: f64 = 9.0;
/// Extra guard damage per Villager-Arms tier (Villager.tsx `+7/tier`).
pub const GUARD_DAMAGE_PER_TIER: f64 = 7.0;

/// Guard melee damage at Villager-Arms `tier` (0–2): `GUARD_DAMAGE + 7·tier`.
pub fn guard_damage(tier: i64) -> f64 {
    GUARD_DAMAGE + GUARD_DAMAGE_PER_TIER * tier as f64
}

/// Guard aggro radius at Villager-Arms `tier` (0–2): `GUARD_AGGRO + 3.5·tier`.
pub fn guard_aggro(tier: i64) -> f64 {
    GUARD_AGGRO + GUARD_AGGRO_PER_TIER * tier as f64
}

/// How far a guard will range from the keep to engage — wider during a wave so the
/// militia pushes out to meet the assault (`GUARD_DEFEND_RADIUS × 1.8` at night).
pub fn guard_defend_radius(in_wave: bool) -> f64 {
    if in_wave { GUARD_DEFEND_RADIUS * GUARD_DEFEND_WAVE_MULT } else { GUARD_DEFEND_RADIUS }
}

/// Day-phase schedule for a villager (Villager.tsx `scheduledMode`). The day is
/// a 60s cycle: tend the garden, then wander, then head to the door, then home.
pub fn villager_scheduled_mode(t: f64) -> VillagerStateName {
    let day_phase = (t / 60.0).rem_euclid(1.0);
    if day_phase < 0.4 {
        VillagerStateName::Tend
    } else if day_phase < 0.6 {
        VillagerStateName::Wander
    } else if day_phase < 0.65 {
        VillagerStateName::Rest // travelling to the door
    } else {
        VillagerStateName::Home // inside the house
    }
}

/// A villager's next wander destination (Villager.tsx `nextWanderPoint`). Uses
/// the same sin-hash so the path is deterministic in `(id, t)` — no RNG needed.
pub fn villager_wander_point(id: f64, home_x: f64, home_z: f64, t: f64) -> (f64, f64) {
    let ang = (((id * 12.9898 + t * 0.31).sin()) * 43758.5453).rem_euclid(TWO_PI);
    let r = VILLAGER_WANDER_RADIUS * (0.4 + (t * 0.17 + id).sin().abs() * 0.6);
    (home_x + ang.cos() * r, home_z + ang.sin() * r)
}

/// Day-phase schedule for a trader (Trader.tsx `scheduledMode`). Traders hang
/// around the stall on a 22s cycle: mind the counter (tend), then a short stroll.
pub fn trader_scheduled_mode(t: f64) -> TraderStateName {
    let phase = (t / 22.0).rem_euclid(1.0);
    if phase < 0.55 {
        TraderStateName::Tend
    } else {
        TraderStateName::Wander
    }
}

/// A trader's next wander destination (Trader.tsx wander branch).
pub fn trader_wander_point(id: f64, home_x: f64, home_z: f64, t: f64) -> (f64, f64) {
    let ang = (((id * 12.9898 + t * 0.29).sin()) * 43758.5453).rem_euclid(TWO_PI);
    let r = TRADER_WANDER_RADIUS * (0.4 + (t * 0.15 + id).sin().abs() * 0.6);
    (home_x + ang.cos() * r, home_z + ang.sin() * r)
}

/// A villager's tend target — drift around the garden anchor (Villager.tsx
/// `tend` branch). Deterministic in `(seed, t)`.
pub fn villager_tend_point(seed: f64, garden_x: f64, garden_z: f64, t: f64) -> (f64, f64) {
    (
        garden_x + (seed + t * 0.5).sin() * 0.4,
        garden_z + (seed + t * 0.7).cos() * 0.4,
    )
}

/// A trader's tend target — drift around the stall counter (Trader.tsx `tend`).
pub fn trader_tend_point(seed: f64, garden_x: f64, garden_z: f64, t: f64) -> (f64, f64) {
    (
        garden_x + (seed + t * 0.4).sin() * 0.25,
        garden_z + (seed + t * 0.6).cos() * 0.25,
    )
}

/// Angle interpolation toward a target heading (shortest-arc blend), used to turn
/// an NPC's facing toward its movement direction. Same as `animal::lerp_angle`,
/// duplicated to keep the modules independent.
pub fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let mut d = b - a;
    while d > PI {
        d -= TWO_PI;
    }
    while d < -PI {
        d += TWO_PI;
    }
    a + d * t
}

/// New facing after turning toward (dx,dz) for `dt` at `rate`. Returns the
/// unchanged facing when the direction is ~zero.
pub fn face_toward(facing: f64, dx: f64, dz: f64, dt: f64, rate: f64) -> f64 {
    if dx * dx + dz * dz < 1e-6 {
        return facing;
    }
    lerp_angle(facing, dx.atan2(dz), (dt * rate).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::{CENTER_X, CENTER_Z};

    #[test]
    fn guard_stats_scale_with_arms_tier() {
        assert_eq!(guard_damage(0), 9.0);
        assert_eq!(guard_damage(1), 16.0);
        assert_eq!(guard_damage(2), 23.0);
        assert_eq!(guard_aggro(0), 7.5);
        assert_eq!(guard_aggro(2), 7.5 + 7.0);
    }

    #[test]
    fn guard_defend_radius_widens_at_night() {
        assert_eq!(guard_defend_radius(false), 12.0);
        assert!(guard_defend_radius(true) > guard_defend_radius(false));
        assert_eq!(guard_defend_radius(true), 12.0 * 1.8);
    }

    fn trader_init(x: f64, z: f64) -> TraderInit {
        TraderInit {
            x,
            y: 1.0,
            z,
            facing: 0.0,
            home_x: x,
            home_z: z,
            garden_x: x + 1.0,
            garden_z: z,
            door_x: x,
            door_z: z,
            seed: 0.5,
            palette_index: 0,
            name: "Merchant".into(),
        }
    }

    // ─── Trader roster (port of traderStore.test.ts) ──────────────────────────

    #[test]
    fn create_trader_adds_with_unique_id_and_seeded_defaults() {
        let mut r = TraderRoster::new();
        let a = r.create(trader_init(2.0, 2.0));
        let b = r.create(trader_init(3.0, 3.0));
        assert_eq!(r.len(), 2);
        assert_ne!(a, b);
        let ta = &r.all()[0];
        assert_eq!(ta.state, TraderStateName::Idle);
        assert_eq!(ta.target_x, 2.0);
    }

    #[test]
    fn reset_traders_clears_roster_and_id_counter() {
        let mut r = TraderRoster::new();
        r.create(trader_init(0.0, 0.0));
        r.reset();
        assert_eq!(r.len(), 0);
        assert_eq!(r.create(trader_init(0.0, 0.0)), 0);
    }

    #[test]
    fn remove_trader_by_id_unknown_is_noop() {
        let mut r = TraderRoster::new();
        let a = r.create(trader_init(0.0, 0.0));
        r.remove(9999);
        assert_eq!(r.len(), 1);
        r.remove(a);
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn nearest_trader_returns_closest_within_max_dist() {
        let mut r = TraderRoster::new();
        r.create(trader_init(0.0, 0.0));
        let near = r.create(trader_init(5.0, 0.0));
        r.create(trader_init(20.0, 0.0));
        assert_eq!(r.nearest(6.0, 0.0, 3.0), Some(near));
    }

    #[test]
    fn nearest_trader_null_when_none_within_max_dist() {
        let mut r = TraderRoster::new();
        r.create(trader_init(0.0, 0.0));
        assert_eq!(r.nearest(50.0, 50.0, 2.0), None);
    }

    // ─── Villager roster ──────────────────────────────────────────────────────

    fn villager_init(x: f64, z: f64, home_x: f64, home_z: f64) -> VillagerInit {
        VillagerInit {
            x,
            y: 1.0,
            z,
            facing: 0.0,
            home_x,
            home_z,
            garden_x: home_x + 2.0,
            garden_z: home_z,
            door_x: x,
            door_z: z,
            seed: 0.4,
            palette_index: 0,
            recruited: false,
        }
    }

    #[test]
    fn create_villager_inside_castle_is_guard() {
        let mut r = VillagerRoster::new();
        // Home at the castle centre → guard; a frontier home → not.
        r.create(villager_init(CENTER_X, CENTER_Z, CENTER_X, CENTER_Z));
        r.create(villager_init(5.0, 5.0, 5.0, 5.0));
        assert!(r.all()[0].is_guard);
        assert!(!r.all()[1].is_guard);
        assert_eq!(r.all()[0].hp, VILLAGER_MAX_HP);
    }

    #[test]
    fn damage_and_revive_villager() {
        let mut r = VillagerRoster::new();
        let id = r.create(villager_init(0.0, 0.0, 0.0, 0.0));
        assert!(!r.damage(id, 100.0)); // survives
        assert_eq!(r.standing_count(), 1);
        assert!(r.damage(id, 100.0)); // downed
        assert!(r.all()[0].downed);
        assert_eq!(r.standing_count(), 0);
        r.revive_all();
        assert!(!r.all()[0].downed);
        assert_eq!(r.all()[0].hp, VILLAGER_MAX_HP);
        assert_eq!(r.standing_count(), 1);
    }

    #[test]
    fn nearest_standing_skips_downed() {
        let mut r = VillagerRoster::new();
        let near = r.create(villager_init(1.0, 0.0, 1.0, 0.0));
        r.create(villager_init(10.0, 0.0, 10.0, 0.0));
        assert_eq!(r.nearest_standing(0.0, 0.0), Some(near));
        r.damage(near, 1000.0); // down the close one
        // Now the far one is the only standing heir.
        assert_ne!(r.nearest_standing(0.0, 0.0), Some(near));
        assert!(r.nearest_standing(0.0, 0.0).is_some());
    }

    // ─── Schedule / wander math ───────────────────────────────────────────────

    #[test]
    fn villager_schedule_cycles_through_the_day() {
        assert_eq!(villager_scheduled_mode(0.0), VillagerStateName::Tend);
        assert_eq!(villager_scheduled_mode(30.0), VillagerStateName::Wander);
        assert_eq!(villager_scheduled_mode(36.0), VillagerStateName::Rest);
        assert_eq!(villager_scheduled_mode(50.0), VillagerStateName::Home);
        // Wraps every 60s.
        assert_eq!(villager_scheduled_mode(60.0), VillagerStateName::Tend);
    }

    #[test]
    fn trader_schedule_alternates_tend_and_wander() {
        assert_eq!(trader_scheduled_mode(0.0), TraderStateName::Tend);
        assert_eq!(trader_scheduled_mode(15.0), TraderStateName::Wander);
        assert_eq!(trader_scheduled_mode(22.0), TraderStateName::Tend);
    }

    #[test]
    fn wander_points_stay_within_their_radius() {
        // The picked destination is always within WANDER_RADIUS of home.
        for step in 0..50 {
            let t = step as f64 * 0.37;
            let (vx, vz) = villager_wander_point(3.0, 50.0, 40.0, t);
            assert!(((vx - 50.0).hypot(vz - 40.0)) <= VILLAGER_WANDER_RADIUS + 1e-6);
            let (tx, tz) = trader_wander_point(2.0, 90.0, 30.0, t);
            assert!(((tx - 90.0).hypot(tz - 30.0)) <= TRADER_WANDER_RADIUS + 1e-6);
        }
    }

    #[test]
    fn face_toward_turns_toward_direction() {
        // From facing 0, turning toward +x (dx=1, dz=0) should head toward atan2(1,0)=π/2.
        let f = face_toward(0.0, 1.0, 0.0, 1.0, 100.0);
        assert!((f - PI / 2.0).abs() < 1e-6);
        // Zero direction leaves facing unchanged.
        assert_eq!(face_toward(0.7, 0.0, 0.0, 1.0, 8.0), 0.7);
    }
}
