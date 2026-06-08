//! Port of src/world/animalConfig.ts + the PURE parts of src/world/animalAI.ts —
//! per-species stats and the deterministic, dep-free movement/flee/wander math the
//! Bevy `WildlifePlugin` drives. The TS AI also touched module-global stores
//! (player/bear/animal rosters, A* pathfinding, frontier damage scaling, melee
//! callbacks); those are ECS/world concerns and live in the game crate. Here we
//! keep only the pure geometry so it is `#[cfg(test)]`-checkable like the rest of
//! `tileworld_core`.
//!
//! Simplifications vs. the full TS food-chain (the game-plugin layer applies these):
//!   - prey flee the PLAYER + any predator within fear range (TS also fled bears,
//!     a separate roster; bears aren't ported).
//!   - predators wander, and hunt the player when within aggro (TS predators only
//!     ever targeted the player too — `predatorTarget` ignores other wildlife).
//!   - boars are neutral wanderers that enrage + charge the player when it comes
//!     within `aggro` (TS identical, minus the bog-croc/golem sharing the branch).
//!   - melee damage, A* detours, and corpse/respawn are the plugin's job.
//!
//! f64 throughout for JS-`number` parity (matches the rest of the crate).

use crate::factions::AnimalFaction;

/// Every wild species. 1:1 with the TS `AnimalSpecies` union, PLUS the two
/// creatures that lived in their own TS stores (not `animalStore`): `Bear`
/// (`bearStore.ts` — neutral→hostile wilds predator) and `Dog` (`dogStore.ts` —
/// the village dogs). They're folded in here so the shared spawn / AI / visual /
/// respawn pipeline drives them too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Species {
    Wolf,
    Deer,
    Boar,
    Rabbit,
    PolarBear,
    Scorpion,
    BogCroc,
    Elk,
    Goat,
    Golem,
    /// Heavy wilds bruiser — neutral until it gets close or is struck, then chases
    /// and mauls (`bearStore.ts` + `Bear.tsx`).
    Bear,
    /// Village dog — a benign wanderer that barks (`dogStore.ts` + the `Wildlife.tsx`
    /// dog view). Never flees, never attacks.
    Dog,
}

impl Species {
    /// All species, for spawn tables + exhaustive tests.
    pub const ALL: [Species; 12] = [
        Species::Wolf,
        Species::Deer,
        Species::Boar,
        Species::Rabbit,
        Species::PolarBear,
        Species::Scorpion,
        Species::BogCroc,
        Species::Elk,
        Species::Goat,
        Species::Golem,
        Species::Bear,
        Species::Dog,
    ];
}

/// Behaviour class — selects the AI branch (mirrors the TS `Behavior`, plus the
/// two creatures that had their own stores: `Bear` and `Critter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behavior {
    /// Hunts the player when in aggro; wanders otherwise.
    Predator,
    /// Flees threats (player + predators + bears) within fear range; never attacks.
    Prey,
    /// Neutral wanderer that enrages + charges the player when approached.
    Boar,
    /// Neutral wilds bruiser (`bearStore.ts`): wanders until the player gets within
    /// `aggro` OR it's struck, then it stays hostile — chasing + mauling — until the
    /// player breaks past `leash`. Unlike the boar's timed charge, a bear's aggro is
    /// a latched flag, so being hit from outside aggro range still enrages it.
    Bear,
    /// Benign wanderer (`dogStore.ts`): just roams + barks. Never flees, never
    /// attacks. The `aggro`/`fear`/`melee` stats are all zero.
    Critter,
}

/// Per-species stats + behaviour. Port of `AnimalConfig` (animalConfig.ts), minus
/// the drop-item / bounty fields that belong to the (un-ported) loot layer; the
/// gameplay-relevant subset the AI + spawner read is kept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimalConfig {
    pub faction: AnimalFaction,
    pub behavior: Behavior,
    pub hp: f64,
    /// chase (predator/boar) or flee (prey) speed, grid units/sec.
    pub speed: f64,
    /// relaxed wander speed.
    pub wander_speed: f64,
    /// predator: detect-target range. boar: charge-trigger proximity.
    pub aggro: f64,
    /// predator/boar: give-up distance.
    pub leash: f64,
    /// prey: start fleeing within this range of a threat.
    pub fear: f64,
    pub melee: f64,
    pub attack_damage: f64,
    pub attack_cooldown: f64,
    pub turn_rate: f64,
    pub collision_radius: f64,
    /// outer group scale for the view mesh.
    pub scale: f64,
    pub bounty_gold: f64,
    pub bounty_xp: f64,
    /// item id dropped on death (from the inventory item defs); `None` = no drop.
    /// Mirrors `animalConfig.ts` `dropItemId`.
    pub drop_item: Option<&'static str>,
    /// 0..1 chance to roll the primary drop (TS `dropChance`, default 1 when set).
    pub drop_chance: f64,
    /// Optional second, rarer drop rolled independently (armor off a boss-tier
    /// creature). Mirrors `dropItemId2`.
    pub drop_item2: Option<&'static str>,
    /// 0..1 chance for the second drop (TS `dropChance2`, default 1 when set).
    pub drop_chance2: f64,
    /// preferred biome to spawn in (drives the startup population placement).
    pub home_biome: crate::tilemap::Biome,
}

/// Stats for a species — mirrors `ANIMAL_CONFIG[species]` in animalConfig.ts.
pub fn animal_config(species: Species) -> AnimalConfig {
    use crate::tilemap::Biome;
    match species {
        // Pack predator — hunts the player.
        Species::Wolf => AnimalConfig {
            faction: AnimalFaction::Predator,
            behavior: Behavior::Predator,
            hp: 80.0,
            speed: 3.8,
            wander_speed: 1.1,
            aggro: 12.0,
            leash: 18.0,
            fear: 0.0,
            melee: 1.4,
            attack_damage: 12.0,
            attack_cooldown: 1.1,
            turn_rate: 8.0,
            collision_radius: 0.32,
            scale: 0.48,
            bounty_gold: 12.0,
            bounty_xp: 22.0,
            drop_item: None,
            drop_chance: 0.0,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Forest,
        },
        // Skittish grazer — bolts from the player + predators.
        Species::Deer => AnimalConfig {
            faction: AnimalFaction::Prey,
            behavior: Behavior::Prey,
            hp: 45.0,
            speed: 3.5,
            wander_speed: 1.3,
            aggro: 0.0,
            leash: 0.0,
            fear: 8.0,
            melee: 0.0,
            attack_damage: 0.0,
            attack_cooldown: 0.0,
            turn_rate: 7.0,
            collision_radius: 0.3,
            scale: 0.5,
            bounty_gold: 10.0,
            bounty_xp: 14.0,
            drop_item: None,
            drop_chance: 0.0,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Grass,
        },
        // Neutral tank — ignores you until provoked, then charges + gores.
        Species::Boar => AnimalConfig {
            faction: AnimalFaction::Boar,
            behavior: Behavior::Boar,
            hp: 140.0,
            speed: 3.2,
            wander_speed: 0.9,
            aggro: 5.0, // proximity that triggers a charge
            leash: 16.0,
            fear: 0.0,
            melee: 1.5,
            attack_damage: 18.0,
            attack_cooldown: 1.4,
            turn_rate: 6.0,
            collision_radius: 0.4,
            scale: 0.48,
            bounty_gold: 16.0,
            bounty_xp: 26.0,
            drop_item: None,
            drop_chance: 0.0,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Grass,
        },
        // Tiny, very skittish ambient prey.
        Species::Rabbit => AnimalConfig {
            faction: AnimalFaction::Prey,
            behavior: Behavior::Prey,
            hp: 8.0,
            speed: 4.0,
            wander_speed: 1.4,
            aggro: 0.0,
            leash: 0.0,
            fear: 6.0,
            melee: 0.0,
            attack_damage: 0.0,
            attack_cooldown: 0.0,
            turn_rate: 9.0,
            collision_radius: 0.0,
            scale: 0.4,
            bounty_gold: 3.0,
            bounty_xp: 5.0,
            drop_item: None,
            drop_chance: 0.0,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Grass,
        },
        // Snow: hulking predator — slow, heavy hits.
        Species::PolarBear => AnimalConfig {
            faction: AnimalFaction::Predator,
            behavior: Behavior::Predator,
            hp: 200.0,
            speed: 3.0,
            wander_speed: 0.9,
            aggro: 13.0,
            leash: 20.0,
            fear: 0.0,
            melee: 1.6,
            attack_damage: 24.0,
            attack_cooldown: 1.4,
            turn_rate: 6.0,
            collision_radius: 0.42,
            scale: 0.62,
            bounty_gold: 28.0,
            bounty_xp: 40.0,
            drop_item: Some("fur"),
            drop_chance: 0.8,
            drop_item2: Some("leather_armor"),
            drop_chance2: 0.5,
            home_biome: Biome::Snow,
        },
        // Desert: fast, fragile, venomous predator.
        Species::Scorpion => AnimalConfig {
            faction: AnimalFaction::Predator,
            behavior: Behavior::Predator,
            hp: 55.0,
            speed: 4.4,
            wander_speed: 1.4,
            aggro: 11.0,
            leash: 16.0,
            fear: 0.0,
            melee: 1.1,
            attack_damage: 14.0,
            attack_cooldown: 0.9,
            turn_rate: 10.0,
            collision_radius: 0.28,
            scale: 0.4,
            bounty_gold: 14.0,
            bounty_xp: 22.0,
            drop_item: Some("venom"),
            drop_chance: 0.7,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Desert,
        },
        // Swamp: neutral tank that ambush-charges when approached (boar branch).
        Species::BogCroc => AnimalConfig {
            faction: AnimalFaction::Boar,
            behavior: Behavior::Boar,
            hp: 170.0,
            speed: 3.6,
            wander_speed: 0.8,
            aggro: 6.0,
            leash: 16.0,
            fear: 0.0,
            melee: 1.5,
            attack_damage: 20.0,
            attack_cooldown: 1.3,
            turn_rate: 6.0,
            collision_radius: 0.4,
            scale: 0.5,
            bounty_gold: 20.0,
            bounty_xp: 30.0,
            drop_item: Some("croc_steak"),
            drop_chance: 0.9,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Swamp,
        },
        // Forest: large grazer, flees (prey branch).
        Species::Elk => AnimalConfig {
            faction: AnimalFaction::Prey,
            behavior: Behavior::Prey,
            hp: 60.0,
            speed: 3.6,
            wander_speed: 1.2,
            aggro: 0.0,
            leash: 0.0,
            fear: 9.0,
            melee: 0.0,
            attack_damage: 0.0,
            attack_cooldown: 0.0,
            turn_rate: 7.0,
            collision_radius: 0.32,
            scale: 0.58,
            bounty_gold: 12.0,
            bounty_xp: 18.0,
            drop_item: Some("elk_jerky"),
            drop_chance: 0.9,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Forest,
        },
        // Rock: nimble grazer, flees (prey branch).
        Species::Goat => AnimalConfig {
            faction: AnimalFaction::Prey,
            behavior: Behavior::Prey,
            hp: 40.0,
            speed: 3.9,
            wander_speed: 1.3,
            aggro: 0.0,
            leash: 0.0,
            fear: 8.0,
            melee: 0.0,
            attack_damage: 0.0,
            attack_cooldown: 0.0,
            turn_rate: 9.0,
            collision_radius: 0.28,
            scale: 0.42,
            bounty_gold: 10.0,
            bounty_xp: 14.0,
            drop_item: Some("goat_charm"),
            drop_chance: 0.6,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Rock,
        },
        // Rock: very slow, very tanky; boar branch.
        Species::Golem => AnimalConfig {
            faction: AnimalFaction::Boar,
            behavior: Behavior::Boar,
            hp: 280.0,
            speed: 2.4,
            wander_speed: 0.6,
            aggro: 5.0,
            leash: 14.0,
            fear: 0.0,
            melee: 1.7,
            attack_damage: 28.0,
            attack_cooldown: 1.6,
            turn_rate: 5.0,
            collision_radius: 0.46,
            scale: 0.6,
            bounty_gold: 36.0,
            bounty_xp: 55.0,
            drop_item: Some("stone_maul"),
            drop_chance: 0.5,
            drop_item2: Some("iron_armor"),
            drop_chance2: 0.4,
            home_biome: Biome::Rock,
        },
        // Wilds bruiser (`bearStore.ts` + `Bear.tsx`): 180 base HP, neutral until
        // the player is within BEAR_AGGRO(6.5) OR it's struck, chases at BEAR_SPEED
        // (2.6 — faster than a walk, slower than a sprint) until BEAR_LEASH(14),
        // mauls for BEAR_ATTACK_DAMAGE(22) every BEAR_ATTACK_COOLDOWN(1.3)s.
        // A predator faction so prey flee it (the food chain).
        Species::Bear => AnimalConfig {
            faction: AnimalFaction::Predator,
            behavior: Behavior::Bear,
            hp: 180.0,
            speed: 2.6,        // BEAR_SPEED
            wander_speed: 0.9, // BEAR_WANDER_SPEED
            aggro: 6.5,        // BEAR_AGGRO
            leash: 14.0,       // BEAR_LEASH
            fear: 0.0,
            melee: 1.7,           // BEAR_MELEE
            attack_damage: 22.0,  // BEAR_ATTACK_DAMAGE
            attack_cooldown: 1.3, // BEAR_ATTACK_COOLDOWN
            turn_rate: 7.0,       // BEAR_TURN
            collision_radius: 0.45,
            scale: 0.6,
            bounty_gold: 30.0,
            bounty_xp: 45.0,
            // No species drop — the TS bear only ever rolled `maybeFrontierDrop`.
            drop_item: None,
            drop_chance: 0.0,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Grass, // scattered across the wilds via the grass ring
        },
        // Village dog (`dogStore.ts` + `Wildlife.tsx`): 60 HP, wanders the hamlet at
        // DOG_SPEED(1.3) and barks; harmless (Prey faction so it isn't a threat, but
        // a `Critter` branch so it never bolts). No aggro / fear / melee.
        Species::Dog => AnimalConfig {
            faction: AnimalFaction::Prey,
            behavior: Behavior::Critter,
            hp: 60.0,
            speed: 1.3,        // unused (never flees/charges) — matches DOG_SPEED
            wander_speed: 1.3, // DOG_SPEED
            aggro: 0.0,
            leash: 0.0,
            fear: 0.0,
            melee: 0.0,
            attack_damage: 0.0,
            attack_cooldown: 0.0,
            turn_rate: 8.0, // the TS dog turns at rate 8 toward its heading
            collision_radius: 0.15, // DOG_RADIUS
            scale: 0.4,
            bounty_gold: 0.0,
            bounty_xp: 0.0,
            drop_item: None,
            drop_chance: 0.0,
            drop_item2: None,
            drop_chance2: 0.0,
            home_biome: Biome::Grass, // village/grass belt ring
        },
    }
}

// ─── Pure AI math (the dep-free core of animalAI.ts) ────────────────────────────

/// Tiny deterministic RNG (mulberry32) — gives the wander step reproducible
/// "random" picks in tests, replacing the TS `Math.random()`. Identical generator
/// to the game crate's `CombatRng`, kept here so core stays self-contained.
#[derive(Debug, Clone, Copy)]
pub struct AnimalRng {
    s: u32,
}

impl AnimalRng {
    pub fn new(seed: u32) -> Self {
        // Avoid an all-zero state degenerating the generator.
        AnimalRng {
            s: seed ^ 0x9E37_79B9,
        }
    }
    /// Next f64 in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        self.s = self.s.wrapping_add(0x6D2B_79F5);
        let mut t = self.s;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        let r = (t ^ (t >> 14)) as f64;
        r / 4_294_967_296.0
    }
}

/// Angle interpolation toward a target heading (TS `lerpAngle`): shortest-arc
/// blend, used to turn an animal's facing toward its movement direction.
pub fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let mut d = b - a;
    while d > std::f64::consts::PI {
        d -= 2.0 * std::f64::consts::PI;
    }
    while d < -std::f64::consts::PI {
        d += 2.0 * std::f64::consts::PI;
    }
    a + d * t
}

/// New facing after turning toward (dx,dz) for `dt` at `rate` (TS `faceToward`).
/// Returns the unchanged facing when the direction is ~zero.
pub fn face_toward(facing: f64, dx: f64, dz: f64, dt: f64, rate: f64) -> f64 {
    if dx * dx + dz * dz < 1e-6 {
        return facing;
    }
    lerp_angle(facing, dx.atan2(dz), (dt * rate).min(1.0))
}

/// Squared distance helper.
pub fn dist_sq(ax: f64, az: f64, bx: f64, bz: f64) -> f64 {
    let dx = ax - bx;
    let dz = az - bz;
    dx * dx + dz * dz
}

/// The unit step an animal wants to take this tick toward (tx,tz) at `speed`
/// (TS `moveToward`, minus the collision/standable checks the caller applies to
/// the candidate). Returns the candidate (nx, nz); when already at the target it
/// returns the current position unchanged.
pub fn step_toward(x: f64, z: f64, tx: f64, tz: f64, speed: f64, dt: f64) -> (f64, f64) {
    let dx = tx - x;
    let dz = tz - z;
    let len = dx.hypot(dz);
    if len < 0.001 {
        return (x, z);
    }
    let step = speed * dt;
    (x + (dx / len) * step, z + (dz / len) * step)
}

/// Is the candidate continuous coord standable land? Movement-side guard shared by
/// the NPC + wildlife AIs (mirrors the TS `standable` check). The fractional coord
/// is floored to its tile and tested against the map.
pub fn land_ok(x: f64, z: f64) -> bool {
    crate::tilemap::standable(x.floor() as i32, z.floor() as i32)
}

/// Flee target point: a spot 6 tiles directly away from the threat at (fx,fz)
/// (TS `flee` aimed `moveToward` at `pos + away*6`). The caller feeds this to
/// `step_toward` with the flee speed. Away-direction is normalized; a threat at the
/// exact position falls back to +x so the animal still bolts.
pub fn flee_point(x: f64, z: f64, fx: f64, fz: f64) -> (f64, f64) {
    let dx = x - fx;
    let dz = z - fz;
    let len = dx.hypot(dz);
    if len < 1e-6 {
        return (x + 6.0, z);
    }
    (x + (dx / len) * 6.0, z + (dz / len) * 6.0)
}

/// Should a prey animal flee a threat at `threat_dist` tiles? (within `fear`).
/// `fear == 0` (non-prey) never triggers.
pub fn prey_should_flee(cfg: &AnimalConfig, threat_dist: f64) -> bool {
    cfg.fear > 0.0 && threat_dist < cfg.fear
}

/// Should a predator hunt a target at `dist` tiles? (within `aggro`).
pub fn predator_should_hunt(cfg: &AnimalConfig, dist: f64) -> bool {
    cfg.aggro > 0.0 && dist < cfg.aggro
}

/// Bear aggro state machine (`Bear.tsx` L107-115): a neutral bear wakes when the
/// player comes within `aggro`; an already-hostile bear gives up once the player
/// breaks past `leash`. Returns the bear's new aggro flag. `was_aggro` lets a bear
/// that was enraged by a HIT (set externally) stay hostile until the player
/// out-runs the leash — so a struck bear chases even from beyond `aggro`.
pub fn bear_next_aggro(cfg: &AnimalConfig, was_aggro: bool, dist: f64, player_alive: bool) -> bool {
    if !player_alive {
        return false; // a dead player never holds a bear's aggro
    }
    if was_aggro {
        dist <= cfg.leash // stay hostile until the player out-runs the leash
    } else {
        dist < cfg.aggro // wake when the player gets close
    }
}

/// Which species drops actually spawn on a kill, given two independent rolls in
/// [0,1). Port of the `Character.tsx` animal-death loot block: the primary drop
/// rolls against `drop_chance`, the rarer second against `drop_chance2`, each
/// independently (the TS used a fresh `Math.random()` per drop). Returns up to two
/// item ids in (primary, secondary) order. A `None` slot or a failed roll yields no
/// id for that slot. Pure so the drop table is unit-tested without the ECS layer.
pub fn roll_drops(cfg: &AnimalConfig, roll1: f64, roll2: f64) -> [Option<&'static str>; 2] {
    let primary = cfg
        .drop_item
        .filter(|_| roll1 < cfg.drop_chance);
    let secondary = cfg
        .drop_item2
        .filter(|_| roll2 < cfg.drop_chance2);
    [primary, secondary]
}

/// Pick a wander destination near (x,z): a point 2..7 tiles away at a random
/// angle (TS `wander` sampled `2 + rand*5` at a random heading). The caller checks
/// the tile is land before accepting; this just produces a candidate, advancing
/// the RNG. Returns (tx, tz).
pub fn wander_point(x: f64, z: f64, rng: &mut AnimalRng) -> (f64, f64) {
    let ang = rng.next_f64() * std::f64::consts::PI * 2.0;
    let r = 2.0 + rng.next_f64() * 5.0;
    (x + ang.cos() * r, z + ang.sin() * r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::Biome;

    // ─── Config invariants (the values the AI + spawner depend on) ────────────

    #[test]
    fn config_values_match_source() {
        let wolf = animal_config(Species::Wolf);
        assert_eq!(wolf.hp, 80.0);
        assert_eq!(wolf.speed, 3.8);
        assert_eq!(wolf.aggro, 12.0);
        assert_eq!(wolf.behavior, Behavior::Predator);

        let rabbit = animal_config(Species::Rabbit);
        assert_eq!(rabbit.hp, 8.0);
        assert_eq!(rabbit.fear, 6.0);
        assert_eq!(rabbit.behavior, Behavior::Prey);

        let golem = animal_config(Species::Golem);
        assert_eq!(golem.hp, 280.0);
        assert_eq!(golem.speed, 2.4);
        assert_eq!(golem.behavior, Behavior::Boar);

        // Bear (bearStore.ts / Bear.tsx constants).
        let bear = animal_config(Species::Bear);
        assert_eq!(bear.hp, 180.0);
        assert_eq!(bear.speed, 2.6);
        assert_eq!(bear.aggro, 6.5);
        assert_eq!(bear.leash, 14.0);
        assert_eq!(bear.melee, 1.7);
        assert_eq!(bear.attack_damage, 22.0);
        assert_eq!(bear.attack_cooldown, 1.3);
        assert_eq!(bear.behavior, Behavior::Bear);
        assert_eq!(bear.faction, AnimalFaction::Predator);

        // Dog (dogStore.ts / Wildlife.tsx constants).
        let dog = animal_config(Species::Dog);
        assert_eq!(dog.hp, 60.0);
        assert_eq!(dog.wander_speed, 1.3); // DOG_SPEED
        assert_eq!(dog.collision_radius, 0.15); // DOG_RADIUS
        assert_eq!(dog.behavior, Behavior::Critter);
        assert_eq!(dog.attack_damage, 0.0);
    }

    #[test]
    fn bear_aggro_latches_on_proximity_and_releases_past_leash() {
        let bear = animal_config(Species::Bear); // aggro 6.5, leash 14
        // Neutral + player far → stays neutral.
        assert!(!bear_next_aggro(&bear, false, 10.0, true));
        // Neutral + player within aggro → wakes.
        assert!(bear_next_aggro(&bear, false, 5.0, true));
        // Already hostile + player still inside leash → stays hostile (even past aggro,
        // which is how a HIT-enraged bear keeps chasing from beyond 6.5 tiles).
        assert!(bear_next_aggro(&bear, true, 10.0, true));
        // Already hostile + player out-runs the leash → gives up.
        assert!(!bear_next_aggro(&bear, true, 15.0, true));
        // A dead player never holds aggro.
        assert!(!bear_next_aggro(&bear, true, 1.0, false));
    }

    #[test]
    fn faction_matches_behavior_for_every_species() {
        for s in Species::ALL {
            let c = animal_config(s);
            let expected = match c.behavior {
                // Bears share the predator faction so prey flee them (the food chain).
                Behavior::Predator | Behavior::Bear => AnimalFaction::Predator,
                // The dog (Critter) is harmless, so it carries the (non-threatening)
                // prey faction.
                Behavior::Prey | Behavior::Critter => AnimalFaction::Prey,
                Behavior::Boar => AnimalFaction::Boar,
            };
            assert_eq!(c.faction, expected, "{s:?} faction/behavior mismatch");
        }
    }

    #[test]
    fn prey_have_fear_and_no_attack_predators_have_aggro() {
        for s in Species::ALL {
            let c = animal_config(s);
            match c.behavior {
                Behavior::Prey => {
                    assert!(c.fear > 0.0, "{s:?} prey needs a fear range");
                    assert_eq!(c.attack_damage, 0.0, "{s:?} prey deals no damage");
                    assert_eq!(c.aggro, 0.0, "{s:?} prey doesn't aggro");
                }
                Behavior::Predator | Behavior::Boar | Behavior::Bear => {
                    assert!(c.aggro > 0.0, "{s:?} hunter/boar/bear needs an aggro range");
                    assert!(c.attack_damage > 0.0, "{s:?} hunter/boar/bear deals damage");
                    assert!(c.melee > 0.0, "{s:?} hunter/boar/bear needs melee reach");
                }
                Behavior::Critter => {
                    // A benign wanderer: no aggro, no fear, no melee.
                    assert_eq!(c.aggro, 0.0, "{s:?} critter doesn't aggro");
                    assert_eq!(c.fear, 0.0, "{s:?} critter has no fear");
                    assert_eq!(c.attack_damage, 0.0, "{s:?} critter deals no damage");
                }
            }
        }
    }

    // ─── Drop table (the animal-death loot block in Character.tsx) ─────────────

    #[test]
    fn drop_config_matches_source() {
        // The signature-creature drops + chances from animalConfig.ts.
        let pb = animal_config(Species::PolarBear);
        assert_eq!(pb.drop_item, Some("fur"));
        assert_eq!(pb.drop_chance, 0.8);
        assert_eq!(pb.drop_item2, Some("leather_armor"));
        assert_eq!(pb.drop_chance2, 0.5);

        let scorp = animal_config(Species::Scorpion);
        assert_eq!(scorp.drop_item, Some("venom"));
        assert_eq!(scorp.drop_chance, 0.7);
        assert_eq!(scorp.drop_item2, None);

        let golem = animal_config(Species::Golem);
        assert_eq!(golem.drop_item, Some("stone_maul"));
        assert_eq!(golem.drop_chance, 0.5);
        assert_eq!(golem.drop_item2, Some("iron_armor"));
        assert_eq!(golem.drop_chance2, 0.4);

        assert_eq!(animal_config(Species::BogCroc).drop_item, Some("croc_steak"));
        assert_eq!(animal_config(Species::Elk).drop_item, Some("elk_jerky"));
        assert_eq!(animal_config(Species::Goat).drop_item, Some("goat_charm"));

        // Grass-belt + harmless creatures never drop an item.
        for s in [Species::Wolf, Species::Deer, Species::Boar, Species::Rabbit, Species::Bear, Species::Dog] {
            assert_eq!(animal_config(s).drop_item, None, "{s:?} should have no species drop");
            assert_eq!(animal_config(s).drop_item2, None);
        }
    }

    #[test]
    fn roll_drops_respects_each_chance_independently() {
        let pb = animal_config(Species::PolarBear); // fur 0.8, leather_armor 0.5
        // Both rolls under their chance → both drop.
        assert_eq!(roll_drops(&pb, 0.1, 0.1), [Some("fur"), Some("leather_armor")]);
        // Primary under, secondary over → only the primary.
        assert_eq!(roll_drops(&pb, 0.1, 0.9), [Some("fur"), None]);
        // Primary over, secondary under → only the secondary (independent rolls).
        assert_eq!(roll_drops(&pb, 0.9, 0.1), [None, Some("leather_armor")]);
        // Both over → nothing.
        assert_eq!(roll_drops(&pb, 0.95, 0.95), [None, None]);
        // Exactly at the threshold is NOT a drop (strict `<`, matching the TS).
        assert_eq!(roll_drops(&pb, 0.8, 0.5), [None, None]);
    }

    #[test]
    fn roll_drops_yields_nothing_for_a_dropless_species() {
        let deer = animal_config(Species::Deer);
        // Any roll, no item — there's no drop configured.
        assert_eq!(roll_drops(&deer, 0.0, 0.0), [None, None]);
    }

    #[test]
    fn home_biomes_are_signature_placements() {
        assert_eq!(animal_config(Species::PolarBear).home_biome, Biome::Snow);
        assert_eq!(animal_config(Species::Scorpion).home_biome, Biome::Desert);
        assert_eq!(animal_config(Species::BogCroc).home_biome, Biome::Swamp);
        assert_eq!(animal_config(Species::Elk).home_biome, Biome::Forest);
        assert_eq!(animal_config(Species::Goat).home_biome, Biome::Rock);
        assert_eq!(animal_config(Species::Golem).home_biome, Biome::Rock);
    }

    // ─── Movement / flee / wander math ────────────────────────────────────────

    #[test]
    fn step_toward_moves_exactly_speed_times_dt() {
        // From origin toward (10,0) at speed 4 for dt 0.5 → advances 2 tiles.
        let (nx, nz) = step_toward(0.0, 0.0, 10.0, 0.0, 4.0, 0.5);
        assert!((nx - 2.0).abs() < 1e-9);
        assert!(nz.abs() < 1e-9);
    }

    #[test]
    fn step_toward_at_target_does_not_move() {
        let (nx, nz) = step_toward(3.0, 3.0, 3.0, 3.0, 5.0, 0.1);
        assert_eq!((nx, nz), (3.0, 3.0));
    }

    #[test]
    fn flee_point_is_directly_away_from_threat() {
        // Threat at the west (−x); flee point should be to the east (+x).
        let (fx, fz) = flee_point(0.0, 0.0, -5.0, 0.0);
        assert!(fx > 0.0, "flee point should be east of the animal");
        assert!(fz.abs() < 1e-9);
        // And exactly 6 tiles away along the away-axis.
        assert!((fx - 6.0).abs() < 1e-9);
    }

    #[test]
    fn fleeing_a_step_increases_distance_from_threat() {
        // A deer at origin, player threat at (−3,0). One flee step must move it
        // strictly farther from the player (the headless game test mirrors this).
        let cfg = animal_config(Species::Deer);
        let (px, pz) = (-3.0, 0.0);
        let before = dist_sq(0.0, 0.0, px, pz);
        let (tx, tz) = flee_point(0.0, 0.0, px, pz);
        let (nx, nz) = step_toward(0.0, 0.0, tx, tz, cfg.speed, 1.0 / 60.0);
        let after = dist_sq(nx, nz, px, pz);
        assert!(after > before, "flee step must increase distance ({before} -> {after})");
    }

    #[test]
    fn prey_flees_only_inside_fear_range() {
        let deer = animal_config(Species::Deer); // fear 8
        assert!(prey_should_flee(&deer, 5.0));
        assert!(!prey_should_flee(&deer, 9.0));
        // A predator (fear 0) never flees.
        assert!(!prey_should_flee(&animal_config(Species::Wolf), 1.0));
    }

    #[test]
    fn predator_hunts_only_inside_aggro_range() {
        let wolf = animal_config(Species::Wolf); // aggro 12
        assert!(predator_should_hunt(&wolf, 6.0));
        assert!(!predator_should_hunt(&wolf, 15.0));
        // Prey (aggro 0) never hunts.
        assert!(!predator_should_hunt(&animal_config(Species::Deer), 1.0));
    }

    #[test]
    fn wander_point_is_within_the_expected_annulus() {
        let mut rng = AnimalRng::new(42);
        for _ in 0..200 {
            let (tx, tz) = wander_point(10.0, 10.0, &mut rng);
            let d = dist_sq(tx, tz, 10.0, 10.0).sqrt();
            assert!((2.0..=7.0).contains(&d), "wander radius {d} out of 2..7");
        }
    }

    #[test]
    fn rng_is_deterministic_for_a_seed() {
        let mut a = AnimalRng::new(7);
        let mut b = AnimalRng::new(7);
        for _ in 0..50 {
            assert_eq!(a.next_f64(), b.next_f64());
        }
    }

    #[test]
    fn face_toward_turns_toward_the_target_heading() {
        // Facing +z (0); target lies along +x → heading atan2(1,0)=PI/2. A partial
        // turn moves facing toward PI/2 without overshooting.
        let f = face_toward(0.0, 1.0, 0.0, 1.0, 0.5);
        assert!(f > 0.0 && f < std::f64::consts::FRAC_PI_2);
    }

    #[test]
    fn face_toward_ignores_zero_direction() {
        assert_eq!(face_toward(1.23, 0.0, 0.0, 1.0, 8.0), 1.23);
    }
}
