//! Port of src/world/orkConfig.ts — per-variant ork stats + bounty helpers.
//! A variant is fully described by one `OrkConfig` entry (the single source of
//! tuning the AI reads), so this is also the spec for combat/reward numbers.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrkVariant {
    Grunt,
    Scout,
    Berserker,
    Shaman,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrkConfig {
    pub hp: f64,
    /// melee (or bolt, for shaman) damage per landed hit
    pub damage: f64,
    pub speed: f64,
    pub aggro: f64,
    pub melee: f64,
    pub turn_rate: f64,
    pub attack_duration: f64,
    pub attack_cooldown: f64,
    pub path_recompute: f64,
    pub waypoint_radius: f64,
    pub collision_radius: f64,
    pub scale: f64,
    pub skin: &'static str,
    pub bounty_gold: f64,
    pub bounty_xp: f64,
    // Behaviour flags / ranged-caster tuning (None when not applicable).
    pub ranged: bool,
    pub ranged_range: Option<f64>,
    pub heal_amount: Option<f64>,
    pub heal_cooldown: Option<f64>,
    pub heal_range: Option<f64>,
    pub frenzy: bool,
}

/// Stats for a variant — mirrors ORK_CONFIG[variant] in orkConfig.ts.
pub fn ork_config(v: OrkVariant) -> OrkConfig {
    match v {
        OrkVariant::Grunt => OrkConfig {
            hp: 254.0, damage: 24.0, speed: 2.3, aggro: 9.0, melee: 1.5, turn_rate: 6.0,
            attack_duration: 0.7, attack_cooldown: 1.6, path_recompute: 0.55,
            waypoint_radius: 0.45, collision_radius: 0.32, scale: 1.0, skin: "#3a6a2a",
            bounty_gold: 8.0, bounty_xp: 20.0, ranged: false, ranged_range: None,
            heal_amount: None, heal_cooldown: None, heal_range: None, frenzy: false,
        },
        OrkVariant::Scout => OrkConfig {
            hp: 136.0, damage: 15.0, speed: 3.3, aggro: 13.0, melee: 1.4, turn_rate: 9.0,
            attack_duration: 0.5, attack_cooldown: 1.0, path_recompute: 0.4,
            waypoint_radius: 0.45, collision_radius: 0.26, scale: 0.78, skin: "#5f9a3c",
            bounty_gold: 6.0, bounty_xp: 14.0, ranged: false, ranged_range: None,
            heal_amount: None, heal_cooldown: None, heal_range: None, frenzy: false,
        },
        OrkVariant::Berserker => OrkConfig {
            hp: 306.0, damage: 30.0, speed: 2.6, aggro: 10.0, melee: 1.5, turn_rate: 7.0,
            attack_duration: 0.45, attack_cooldown: 0.85, path_recompute: 0.45,
            waypoint_radius: 0.45, collision_radius: 0.34, scale: 1.06, skin: "#7a3a26",
            bounty_gold: 14.0, bounty_xp: 30.0, ranged: false, ranged_range: None,
            heal_amount: None, heal_cooldown: None, heal_range: None, frenzy: true,
        },
        OrkVariant::Shaman => OrkConfig {
            hp: 201.0, damage: 26.0, speed: 1.8, aggro: 15.0, melee: 11.0, turn_rate: 5.0,
            attack_duration: 0.6, attack_cooldown: 2.1, path_recompute: 0.5,
            waypoint_radius: 0.45, collision_radius: 0.3, scale: 0.96, skin: "#6a3f86",
            bounty_gold: 18.0, bounty_xp: 34.0, ranged: true, ranged_range: Some(12.0),
            heal_amount: Some(24.0), heal_cooldown: Some(5.0), heal_range: Some(8.0),
            frenzy: false,
        },
    }
}

/// Gold dropped by a slain ork after the player's bounty multiplier, rounded.
pub fn ork_bounty_gold(v: OrkVariant, bounty_mult: f64) -> i64 {
    (ork_config(v).bounty_gold * bounty_mult).round() as i64
}

/// XP dropped by a slain ork (no bounty multiplier — that boon is gold-only).
pub fn ork_bounty_xp(v: OrkVariant) -> i64 {
    ork_config(v).bounty_xp as i64
}

#[cfg(test)]
mod tests {
    // No dedicated orkConfig.test.ts existed; these lock the numbers the wave
    // director + reward path depend on (grunt/berserker base HP, bounties).
    use super::OrkVariant::*;
    use super::*;

    #[test]
    fn base_hp_matches_source() {
        assert_eq!(ork_config(Grunt).hp, 254.0);
        assert_eq!(ork_config(Scout).hp, 136.0);
        assert_eq!(ork_config(Berserker).hp, 306.0);
        assert_eq!(ork_config(Shaman).hp, 201.0);
    }

    #[test]
    fn bounty_gold_scales_and_rounds() {
        assert_eq!(ork_bounty_gold(Grunt, 1.0), 8);
        assert_eq!(ork_bounty_gold(Shaman, 1.0), 18);
        assert_eq!(ork_bounty_gold(Grunt, 1.5), 12); // round(8*1.5)
        assert_eq!(ork_bounty_gold(Scout, 1.25), 8); // round(6*1.25=7.5)=8
    }

    #[test]
    fn bounty_xp_ignores_multiplier() {
        assert_eq!(ork_bounty_xp(Grunt), 20);
        assert_eq!(ork_bounty_xp(Berserker), 30);
        assert_eq!(ork_bounty_xp(Shaman), 34);
    }

    #[test]
    fn only_shaman_is_ranged_only_berserker_frenzies() {
        assert!(ork_config(Shaman).ranged);
        assert!(!ork_config(Grunt).ranged);
        assert!(ork_config(Berserker).frenzy);
        assert!(!ork_config(Shaman).frenzy);
    }
}
