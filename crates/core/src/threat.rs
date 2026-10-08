//! Predictable night problems and the exact benefit of clearing their staging camp.
//! A raid removes its specialists from the announced night; ordinary attackers remain.

use crate::ork_config::OrkVariant;
use OrkVariant::{Berserker, Grunt, Scout, Shaman};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreatKind {
    Recon,
    Arson,
    Ritual,
    Flank,
    Breakers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tactic {
    March,
    Arson,
    Flank,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveSlot {
    pub variant: OrkVariant,
    pub tactic: Tactic,
}

#[derive(Clone, Copy, Debug)]
pub struct Threat {
    pub kind: ThreatKind,
    pub name: &'static str,
    pub brief: &'static str,
    pub raid_result: &'static str,
    pub count: u32,
    pub hp_scale: f32,
    pub dmg_scale: f32,
    pub spawn_interval: f32,
    pub variants: &'static [OrkVariant],
}

impl Threat {
    pub fn slot(&self, spawn_index: u32) -> WaveSlot {
        let variant = self.variants[spawn_index as usize % self.variants.len()];
        let tactic = match (self.kind, variant) {
            (ThreatKind::Arson, Grunt) if spawn_index % 4 == 0 => Tactic::Arson,
            (ThreatKind::Flank, Scout) => Tactic::Flank,
            _ => Tactic::March,
        };
        WaveSlot { variant, tactic }
    }

    /// None means this planned slot was prevented, rather than replaced by another enemy.
    /// Callers still consume the slot so a reduced wave can finish normally.
    pub fn slot_after_raid(&self, spawn_index: u32, camp_cleared: bool) -> Option<WaveSlot> {
        let slot = self.slot(spawn_index);
        let specialist = match self.kind {
            ThreatKind::Recon | ThreatKind::Flank => slot.variant == Scout,
            ThreatKind::Arson => slot.tactic == Tactic::Arson,
            ThreatKind::Ritual => slot.variant == Shaman,
            ThreatKind::Breakers => slot.variant == Berserker,
        };
        if camp_cleared && specialist { None } else { Some(slot) }
    }
}

/// The first three nights introduce one problem at a time. Later nights recombine familiar
/// enemies, with a modest stat rise and a bounded number of simultaneous attackers.
pub const THREATS: [Threat; 8] = [
    Threat {
        kind: ThreatKind::Recon, name: "Scouts at the gates",
        brief: "A small scouting party is approaching. Free the captives or defend the gates.",
        raid_result: "Scouts stopped — fewer attackers tonight",
        count: 5, hp_scale: 1.0, dmg_scale: 1.0, spawn_interval: 1.4,
        variants: &[Grunt, Grunt, Scout, Grunt],
    },
    Threat {
        kind: ThreatKind::Arson, name: "Raid on the farms",
        brief: "Torch bearers will attack the farms. Stop their camp or position defenders outside.",
        raid_result: "Torch bearers stopped — farms spared from the raid",
        count: 7, hp_scale: 1.05, dmg_scale: 1.0, spawn_interval: 1.3,
        variants: &[Grunt, Scout, Grunt, Grunt],
    },
    Threat {
        kind: ThreatKind::Ritual, name: "Shamans gathering",
        brief: "Shamans will support the attack. Break their camp or prepare archers.",
        raid_result: "Ritual broken — no shamans tonight",
        count: 8, hp_scale: 1.1, dmg_scale: 1.05, spawn_interval: 1.25,
        variants: &[Grunt, Scout, Shaman, Grunt, Shaman],
    },
    Threat {
        kind: ThreatKind::Flank, name: "Attack on two fronts",
        brief: "Scouts will circle the walls while the main force advances. Stop their camp or cover the side gates.",
        raid_result: "Scouts stopped — no flanking party tonight",
        count: 10, hp_scale: 1.2, dmg_scale: 1.1, spawn_interval: 1.2,
        variants: &[Scout, Grunt, Berserker, Scout, Shaman],
    },
    Threat {
        kind: ThreatKind::Breakers, name: "Berserkers on the march",
        brief: "Berserkers lead a mixed warband. Thin their camp or concentrate defenders at the main gate.",
        raid_result: "Berserkers stopped — the frontline is weakened",
        count: 11, hp_scale: 1.3, dmg_scale: 1.15, spawn_interval: 1.15,
        variants: &[Berserker, Grunt, Scout, Grunt, Shaman],
    },
    Threat {
        kind: ThreatKind::Arson, name: "Fire and steel",
        brief: "Torch bearers target the farms under a mixed escort. Stop their camp or split your defense.",
        raid_result: "Torch bearers stopped — defend the main assault",
        count: 12, hp_scale: 1.35, dmg_scale: 1.2, spawn_interval: 1.15,
        variants: &[Grunt, Scout, Berserker, Grunt, Shaman],
    },
    Threat {
        kind: ThreatKind::Ritual, name: "Ritual and raiders",
        brief: "Shamans support scouts and berserkers. Stop their camp or pick off the casters first.",
        raid_result: "Ritual broken — the escort attacks without shamans",
        count: 13, hp_scale: 1.45, dmg_scale: 1.25, spawn_interval: 1.1,
        variants: &[Shaman, Grunt, Scout, Berserker, Shaman],
    },
    Threat {
        kind: ThreatKind::Flank, name: "Encircling warband",
        brief: "A mixed warband presses the main gate as scouts flank. Stop their camp or defend both approaches.",
        raid_result: "Scouts stopped — the side gates are safe from the flank",
        count: 14, hp_scale: 1.5, dmg_scale: 1.3, spawn_interval: 1.1,
        variants: &[Scout, Berserker, Grunt, Shaman, Scout],
    },
];

/// Late nights rotate the four mature problems instead of repeating a lone giant indefinitely.
pub fn threat_for(next_night: usize) -> &'static Threat {
    let index = if next_night < THREATS.len() { next_night } else { 4 + (next_night - 8) % 4 };
    &THREATS[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_three_nights_introduce_distinct_problems() {
        assert_eq!(threat_for(0).kind, ThreatKind::Recon);
        assert_eq!(threat_for(1).kind, ThreatKind::Arson);
        assert_eq!(threat_for(2).kind, ThreatKind::Ritual);
        assert!(!threat_for(1).variants.contains(&Shaman));
        assert!(!threat_for(1).variants.contains(&Berserker));
    }

    #[test]
    fn ritual_raid_removes_every_shaman_without_replacements() {
        let threat = threat_for(2);
        let before: Vec<_> = (0..threat.count).map(|i| threat.slot(i)).collect();
        let after: Vec<_> = (0..threat.count).filter_map(|i| threat.slot_after_raid(i, true)).collect();
        assert!(before.iter().any(|s| s.variant == Shaman));
        assert!(after.iter().all(|s| s.variant != Shaman));
        assert_eq!(after.len(), before.iter().filter(|s| s.variant != Shaman).count());
    }

    #[test]
    fn arson_raid_removes_torch_bearers_but_keeps_the_escort() {
        let threat = threat_for(1);
        let before: Vec<_> = (0..threat.count).map(|i| threat.slot(i)).collect();
        let after: Vec<_> = (0..threat.count).filter_map(|i| threat.slot_after_raid(i, true)).collect();
        assert!(before.iter().any(|s| s.tactic == Tactic::Arson));
        assert!(after.iter().all(|s| s.tactic != Tactic::Arson));
        assert!(after.iter().any(|s| s.variant == Grunt));
        assert!(after.iter().any(|s| s.variant == Scout));
        assert_eq!(after.len(), before.iter().filter(|s| s.tactic != Tactic::Arson).count());
    }

    #[test]
    fn every_cleared_camp_changes_its_night_without_emptying_it() {
        for threat in THREATS {
            let remaining = (0..threat.count).filter_map(|i| threat.slot_after_raid(i, true)).count();
            assert!(remaining > 0 && remaining < threat.count as usize, "{}", threat.name);
        }
    }

    #[test]
    fn late_nights_mix_known_threats_and_bound_the_stat_ramp() {
        let late: Vec<_> = (8..12).map(threat_for).collect();
        assert_eq!(late.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![ThreatKind::Breakers, ThreatKind::Arson, ThreatKind::Ritual, ThreatKind::Flank]);
        for day in 8..100 {
            let t = threat_for(day);
            assert!(t.variants.len() >= 4);
            assert!(t.count >= 10 && t.count <= 14);
            assert!(t.hp_scale <= 1.5 && t.dmg_scale <= 1.3);
        }
    }
}
