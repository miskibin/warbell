//! Port of src/world/playerStore.ts — the hero's progression + combat math.
//!
//! In the TS game this is a module-global singleton with subscribe/notify; here
//! it's a plain `Player` struct with methods (the listener fan-out is HUD-only →
//! becomes Bevy change-detection on a `Player` Resource, see crate `tileworld_bevy`).
//! Side effects in the TS (SFX/FX/voice/grade pulses) are dropped — they belong
//! to the render/audio layer. Cross-store inputs (buff resist mult, worn-armor
//! mult, shield block) are passed in as explicit args so this stays pure/testable;
//! the ECS layer wires the real values.

pub const PLAYER_MAX_HP: f64 = 125.0;
pub const PLAYER_SPAWN: (f64, f64, f64) = (101.0, 1.0, 80.0);
pub const PLAYER_RESPAWN_DELAY: f64 = 2.4;
pub const PLAYER_STARTING_GOLD: i64 = 30;
pub const PLAYER_BASE_DAMAGE: f64 = 25.0;
pub const XP_PER_ORK: i64 = 20;
const XP_FIRST_LEVEL: i64 = 50;
const HP_PER_LEVEL: f64 = 14.0;
const DAMAGE_PER_LEVEL: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Player {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub facing: f64,
    pub moving: bool,
    pub hp: f64,
    pub max_hp: f64,
    pub hurt_flash_until: f64,
    pub dead_since: Option<f64>,
    pub gold: i64,
    pub level: i64,
    pub xp: i64,
    pub xp_to_next: i64,
    pub attack_damage: f64,
    // Upgrade-tree combat/economy flags.
    pub crit_chance: f64,
    pub lifesteal: f64,
    pub move_speed_mult: f64,
    pub cleave: f64,
    pub bounty_mult: f64,
    // Biome-Warden boons — permanent combat boons granted by slaying each biome boss (the
    // forest-side `boss` module sets these directly on the kill, NOT via the upgrade store).
    // `serde(default)` so older saves (written before wardens existed) still load.
    /// Active move: Ground Slam (Stone Golem) — leap + radial AoE shockwave.
    #[cfg_attr(feature = "serde", serde(default))]
    pub has_ground_slam: bool,
    /// Active move: Sand Dash (Sand Revenant) — dash through enemies, slashing.
    #[cfg_attr(feature = "serde", serde(default))]
    pub has_sand_dash: bool,
    /// Active move: Bramble Sweep (Treant) — 360° spin-cleave.
    #[cfg_attr(feature = "serde", serde(default))]
    pub has_bramble_sweep: bool,
    /// Passive: Frostbite (Bałwan) — hits slow the struck foe; a crit briefly freezes it.
    #[cfg_attr(feature = "serde", serde(default))]
    pub frostbite: bool,
    /// Passive: Venom (Bog Hag) — hits poison the struck foe (DoT) + small lifesteal.
    #[cfg_attr(feature = "serde", serde(default))]
    pub venom: bool,
}

impl Default for Player {
    fn default() -> Self {
        Player {
            x: PLAYER_SPAWN.0,
            y: PLAYER_SPAWN.1,
            z: PLAYER_SPAWN.2,
            facing: std::f64::consts::PI,
            moving: false,
            hp: PLAYER_MAX_HP,
            max_hp: PLAYER_MAX_HP,
            hurt_flash_until: 0.0,
            dead_since: None,
            gold: PLAYER_STARTING_GOLD,
            level: 1,
            xp: 0,
            xp_to_next: XP_FIRST_LEVEL,
            attack_damage: PLAYER_BASE_DAMAGE,
            crit_chance: 0.0,
            lifesteal: 0.0,
            move_speed_mult: 1.0,
            cleave: 0.0,
            bounty_mult: 1.0,
            has_ground_slam: false,
            has_sand_dash: false,
            has_bramble_sweep: false,
            frostbite: false,
            venom: false,
        }
    }
}

impl Player {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_alive(&self) -> bool {
        self.hp > 0.0
    }

    /// Deal damage, after the buff-resist mult and worn-armor mult (both 1.0 =
    /// no mitigation). Shield-block is layered by the ECS caller (it pre-reduces
    /// `amount`); a dead player takes nothing.
    pub fn damage(&mut self, amount: f64, now: f64, taken_mult: f64, armor_mult: f64) {
        if self.hp <= 0.0 {
            return;
        }
        let dmg = amount * taken_mult * armor_mult;
        if dmg <= 0.0 {
            return;
        }
        self.hp = (self.hp - dmg).max(0.0);
        self.hurt_flash_until = now + 0.35;
        if self.hp <= 0.0 {
            self.dead_since = Some(now);
        }
    }

    pub fn heal(&mut self, n: f64) {
        self.hp = (self.hp + n).min(self.max_hp);
    }

    pub fn add_gold(&mut self, n: i64) {
        self.gold += n;
    }

    /// Returns true if the spend succeeded. `unlimited` (debug cheat) never
    /// deducts and always succeeds for non-negative spends (refunds pass n<0).
    pub fn spend_gold(&mut self, n: i64, unlimited: bool) -> bool {
        if unlimited && n >= 0 {
            return true;
        }
        if self.gold < n {
            return false;
        }
        self.gold -= n;
        true
    }

    /// Grant xp; resolve resulting level-ups (raise stats; heal to full UNLESS
    /// dead — a kill's xp can land just after a fatal blow and must not revive).
    pub fn add_xp(&mut self, n: i64) {
        self.xp += n;
        while self.xp >= self.xp_to_next {
            self.xp -= self.xp_to_next;
            self.level += 1;
            self.max_hp += HP_PER_LEVEL;
            self.attack_damage += DAMAGE_PER_LEVEL;
            if self.hp > 0.0 {
                self.hp = self.max_hp;
            }
            self.xp_to_next = XP_FIRST_LEVEL * self.level;
        }
    }

    /// Respawn keeps progression; only hp + position reset (succession heir).
    pub fn respawn_at(&mut self, x: f64, y: f64, z: f64) {
        self.hp = self.max_hp;
        self.dead_since = None;
        self.hurt_flash_until = 0.0;
        self.x = x;
        self.y = y;
        self.z = z;
    }

    pub fn bump_max_hp(&mut self, n: f64) {
        self.max_hp += n;
        self.hp = (self.hp + n).min(self.max_hp);
    }

    pub fn bump_attack_damage(&mut self, n: f64) {
        self.attack_damage += n;
    }

    /// Full wipe to a fresh run.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Pure crit roll: `r` in [0,1); crit (2x) when `r < crit_chance`.
pub fn roll_crit(base_damage: f64, crit_chance: f64, r: f64) -> (f64, bool) {
    let crit = r < crit_chance;
    (if crit { base_damage * 2.0 } else { base_damage }, crit)
}

/// Cleave radius (tiles) — splash reaches orks within this distance of a directly
/// struck ork. Squared form is what the distance check uses (TS `CLEAVE_R2 = 2*2`
/// in src/world/Character.tsx, so `CLEAVE_RADIUS = 2`).
pub const CLEAVE_RADIUS: f64 = 2.0;
pub const CLEAVE_R2: f64 = CLEAVE_RADIUS * CLEAVE_RADIUS;

/// Pure cleave splash damage: a fraction of the swing's (already crit-rolled,
/// rounded) damage, rounded to an int. Mirrors the TS
/// `cleaveDmg = Math.round(dmg * cleaveFrac)` in src/world/Character.tsx (~L787).
pub fn cleave_damage(swing_dmg: f64, cleave_frac: f64) -> f64 {
    (swing_dmg * cleave_frac).round()
}

#[cfg(test)]
mod tests {
    // Port of src/world/playerStore.test.ts (the pure parts; SFX/FX dropped).
    use super::*;

    #[test]
    fn damage_subtracts_hp_and_arms_hurt_flash() {
        let mut p = Player::new();
        p.damage(30.0, 10.0, 1.0, 1.0);
        assert_eq!(p.hp, PLAYER_MAX_HP - 30.0);
        assert_eq!(p.hurt_flash_until, 10.35);
    }

    #[test]
    fn damage_clamps_at_zero_and_records_death() {
        let mut p = Player::new();
        p.damage(999.0, 5.0, 1.0, 1.0);
        assert_eq!(p.hp, 0.0);
        assert_eq!(p.dead_since, Some(5.0));
        assert!(!p.is_alive());
    }

    #[test]
    fn dead_player_takes_no_further_damage() {
        let mut p = Player::new();
        p.damage(999.0, 5.0, 1.0, 1.0);
        p.damage(10.0, 6.0, 1.0, 1.0);
        assert_eq!(p.hp, 0.0);
        assert_eq!(p.dead_since, Some(5.0));
    }

    #[test]
    fn respawn_restores_hp_but_keeps_progression() {
        let mut p = Player::new();
        p.add_xp(50);
        let leveled = p.level;
        p.damage(999.0, 1.0, 1.0, 1.0);
        p.respawn_at(PLAYER_SPAWN.0, PLAYER_SPAWN.1, PLAYER_SPAWN.2);
        assert_eq!(p.hp, p.max_hp);
        assert_eq!(p.dead_since, None);
        assert_eq!(p.level, leveled);
    }

    #[test]
    fn heal_clamps_to_max() {
        let mut p = Player::new();
        p.damage(40.0, 1.0, 1.0, 1.0);
        p.heal(1000.0);
        assert_eq!(p.hp, p.max_hp);
    }

    #[test]
    fn gold_economy() {
        let mut p = Player::new();
        assert_eq!(p.gold, PLAYER_STARTING_GOLD);
        p.add_gold(15);
        assert_eq!(p.gold, PLAYER_STARTING_GOLD + 15);
        let mut p = Player::new();
        assert!(p.spend_gold(10, false));
        assert_eq!(p.gold, PLAYER_STARTING_GOLD - 10);
        let mut p = Player::new();
        assert!(!p.spend_gold(PLAYER_STARTING_GOLD + 1, false));
        assert_eq!(p.gold, PLAYER_STARTING_GOLD);
        let mut p = Player::new();
        assert!(p.spend_gold(99999, true)); // unlimited cheat
        assert_eq!(p.gold, PLAYER_STARTING_GOLD);
    }

    #[test]
    fn xp_below_threshold_grants_no_level() {
        let mut p = Player::new();
        p.add_xp(10);
        assert_eq!(p.level, 1);
        assert_eq!(p.xp, 10);
        assert_eq!(p.xp_to_next, 50);
    }

    #[test]
    fn xp_crossing_threshold_levels_heals_raises_stats() {
        let mut p = Player::new();
        p.add_xp(50);
        assert_eq!(p.level, 2);
        assert_eq!(p.xp, 0);
        assert_eq!(p.max_hp, PLAYER_MAX_HP + 14.0);
        assert_eq!(p.hp, p.max_hp);
        assert_eq!(p.attack_damage, PLAYER_BASE_DAMAGE + 6.0);
        assert_eq!(p.xp_to_next, 100);
    }

    #[test]
    fn xp_single_grant_carries_multiple_levels() {
        let mut p = Player::new();
        p.add_xp(200); // 50->L2 (rem150), 100->L3 (rem50), 150 not met
        assert_eq!(p.level, 3);
        assert_eq!(p.xp, 50);
        assert_eq!(p.xp_to_next, 150);
        assert_eq!(p.max_hp, PLAYER_MAX_HP + 28.0);
        assert_eq!(p.attack_damage, PLAYER_BASE_DAMAGE + 12.0);
    }

    #[test]
    fn dead_player_banks_xp_but_is_not_revived() {
        let mut p = Player::new();
        p.damage(999.0, 1.0, 1.0, 1.0);
        assert!(!p.is_alive());
        let before = p.level;
        p.add_xp(500);
        assert!(p.level > before);
        assert_eq!(p.hp, 0.0);
        assert!(!p.is_alive());
    }

    #[test]
    fn bump_max_hp_raises_ceiling_and_heals() {
        let mut p = Player::new();
        p.damage(50.0, 1.0, 1.0, 1.0); // hp = max-50
        p.bump_max_hp(20.0);
        assert_eq!(p.max_hp, PLAYER_MAX_HP + 20.0);
        assert_eq!(p.hp, PLAYER_MAX_HP - 30.0);
    }

    #[test]
    fn bump_attack_damage() {
        let mut p = Player::new();
        p.bump_attack_damage(5.0);
        assert_eq!(p.attack_damage, PLAYER_BASE_DAMAGE + 5.0);
    }

    #[test]
    fn upgrade_flags_default_neutral_and_reset() {
        let mut p = Player::new();
        assert_eq!(p.crit_chance, 0.0);
        assert_eq!(p.move_speed_mult, 1.0);
        assert_eq!(p.bounty_mult, 1.0);
        p.crit_chance = 0.2;
        p.bounty_mult = 1.5;
        p.reset();
        assert_eq!(p.crit_chance, 0.0);
        assert_eq!(p.bounty_mult, 1.0);
    }

    #[test]
    fn roll_crit_behaviour() {
        assert_eq!(roll_crit(40.0, 0.2, 0.5), (40.0, false));
        assert_eq!(roll_crit(40.0, 0.2, 0.1), (80.0, true));
        assert!(!roll_crit(25.0, 0.0, 0.0).1); // r=0 not < 0
        assert!(roll_crit(25.0, 1.0, 0.999).1);
    }

    #[test]
    fn cleave_damage_is_rounded_fraction() {
        // TS: cleaveFrac 0.3 → Math.round(dmg * 0.3).
        assert_eq!(cleave_damage(25.0, 0.3), 8.0); // 7.5 → 8
        assert_eq!(cleave_damage(40.0, 0.3), 12.0); // 12.0
        assert_eq!(cleave_damage(50.0, 0.5), 25.0);
        assert_eq!(cleave_damage(100.0, 0.0), 0.0); // no cleave upgrade
        assert_eq!(cleave_damage(33.0, 0.3), 10.0); // 9.9 → 10
    }

    #[test]
    fn cleave_radius_matches_ts_squared() {
        // TS CLEAVE_R2 = 2 * 2.
        assert_eq!(CLEAVE_R2, 4.0);
        assert_eq!(CLEAVE_RADIUS, 2.0);
    }
}
