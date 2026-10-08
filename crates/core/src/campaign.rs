//! Three short, situation-led introductions to Warbell's day/night loop.
//!
//! Progress records things that actually happened in the world, rather than an active
//! tutorial index. An action done early still counts, rescuing people alone is valid, and
//! calling a night always moves the advice to the battle currently being fought. The Bevy
//! layer observes the world, persists these facts, and supplies the night's zero-based index.
//! The original [`crate::quest::QuestLog`] remains available for older save files.

use crate::quest::{QuestDef, Reward};

/// The current piece of advice. These are suggestions, never locks on the war bell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Objective {
    Rally,
    Rescue,
    ReturnHome,
    FeedPeople,
    CallNight,
    HoldKeep,
    PrepareDefense,
    DefendFarm,
    DisruptRitual,
    SurviveRitual,
    FreePlay,
}

/// Save-relevant campaign facts, shared by the onboarding and daytime raid systems.
///
/// Defaults describe a new run. A save that predates this whole field should use
/// [`Campaign::legacy`] at the save-container boundary, so Continue does not restart the
/// introduction. Missing fields *inside* an existing campaign receive their fresh defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Campaign {
    pub guided: bool,
    pub rallied: bool,
    pub rescued: bool,
    pub returned: bool,
    /// At least one staffed farm has actually produced food.
    pub farm_worked: bool,
    /// At least one permanent upgrade was purchased; visiting the table is insufficient.
    pub defense_bought: bool,
    /// The ritual camp was defeated during the preparation for the third night.
    pub shaman_cleared: bool,
    /// Actual ranged support provides an alternative to raiding the ritual camp.
    pub ranged_prepared: bool,
    /// Last zero-based preparation day whose briefing has been shown.
    pub briefed_day: i32,
    /// Camps raided during [`Self::raid_day`], in the engine's stable camp order.
    pub cleared_camps: Vec<bool>,
    pub raid_day: i32,
}

impl Default for Campaign {
    fn default() -> Self {
        Self {
            guided: true,
            rallied: false,
            rescued: false,
            returned: false,
            farm_worked: false,
            defense_bought: false,
            shaman_cleared: false,
            ranged_prepared: false,
            briefed_day: -1,
            cleared_camps: Vec::new(),
            raid_day: -1,
        }
    }
}

impl Campaign {
    /// Continuing an older run keeps its existing freedom and preparation clock.
    pub fn legacy() -> Self {
        Self {
            guided: false,
            rallied: true,
            rescued: true,
            returned: true,
            farm_worked: true,
            defense_bought: true,
            shaman_cleared: true,
            ranged_prepared: true,
            ..Self::default()
        }
    }

    /// Whether this is one of the three untimed preparation days of a new run.
    ///
    /// The engine holds the preparation clock until the player rings the bell. This must
    /// not be used to prevent the bell from being rung before an objective is completed.
    pub fn learning_day(&self, next_night: usize) -> bool {
        self.guided && next_night < 3
    }

    /// Select advice from the current situation and facts, accepting actions done early.
    ///
    /// `next_night` is the zero-based incoming night in preparation, and the current night
    /// during a wave. Combat takes precedence over unfinished preparation advice.
    pub fn objective(&self, next_night: usize, in_wave: bool) -> Objective {
        if !self.learning_day(next_night) {
            return Objective::FreePlay;
        }
        if in_wave {
            return match next_night {
                0 => Objective::HoldKeep,
                1 => Objective::DefendFarm,
                _ => Objective::SurviveRitual,
            };
        }
        match next_night {
            0 if !self.rescued && !self.rallied => Objective::Rally,
            0 if !self.rescued => Objective::Rescue,
            0 if !self.returned => Objective::ReturnHome,
            0 if !self.farm_worked => Objective::FeedPeople,
            0 => Objective::CallNight,
            1 if !self.defense_bought => Objective::PrepareDefense,
            1 => Objective::DefendFarm,
            2 if !self.shaman_cleared && !self.ranged_prepared => Objective::DisruptRitual,
            2 => Objective::SurviveRitual,
            _ => Objective::FreePlay,
        }
    }

    /// Reuses the existing tracker/card presentation without its linear progress or rewards.
    pub fn lesson(&self, next_night: usize, in_wave: bool) -> &'static QuestDef {
        match self.objective(next_night, in_wave) {
            Objective::Rally => &RALLY,
            Objective::Rescue => &RESCUE,
            Objective::ReturnHome => &RETURN_HOME,
            Objective::FeedPeople => &FEED_PEOPLE,
            Objective::CallNight => &CALL_NIGHT,
            Objective::HoldKeep => &HOLD_KEEP,
            Objective::PrepareDefense => &PREPARE_DEFENSE,
            Objective::DefendFarm if in_wave => &DEFEND_FARM_WAVE,
            Objective::DefendFarm => &DEFEND_FARM,
            Objective::DisruptRitual => &DISRUPT_RITUAL,
            Objective::SurviveRitual if in_wave => &SURVIVE_RITUAL_WAVE,
            Objective::SurviveRitual => &SURVIVE_RITUAL,
            Objective::FreePlay => &FREE_PLAY,
        }
    }
}

// The payoff is the changed town and battle. The legacy UI's objective field is binary so
// it does not show a misleading numeric gather meter for these world-state lessons.
const NO_REWARD: Reward = Reward { gold: 0, wood: 0.0, stone: 0.0, item: None };

static RALLY: QuestDef = QuestDef {
    id: "campaign_rally",
    title: "Bring your people home",
    why: "Orks are holding villagers nearby. Free them and your town gains workers and defenders.",
    explain: "Press K to gather a rescue party. Your people follow you and fight \
              alongside you. Head for the marked prisoner camp.\n\n\
              You can also go alone. There is time to explore; ring the war bell when you are ready.",
    action: "K — gather the militia · follow the gold marker",
    icon: "def_armor_1",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static RESCUE: QuestDef = QuestDef {
    id: "campaign_rescue",
    title: "Free the captives",
    why: "A larger town can work the fields and hold the keep. The prisoners become your people.",
    explain: "Follow the marker to the prisoner camp and defeat its ork guards. The cage opens \
              when the last guard falls. Your rescue party fights beside you; stay close enough \
              to support them.\n\nYou can retreat and return with stronger equipment.",
    action: "Marked camp — defeat the guards",
    icon: "def_armor_1",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static RETURN_HOME: QuestDef = QuestDef {
    id: "campaign_return_home",
    title: "Put the town back to work",
    why: "The rescued villagers have joined your town. A marching war party cannot tend the fields.",
    explain: "Return to the keep and press K to stand the party down. Your people return to their \
              jobs. Watch them take up work: the rescue has changed more than a number.",
    action: "At the keep — K to stand down",
    icon: "stat:food",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static FEED_PEOPLE: QuestDef = QuestDef {
    id: "campaign_feed_people",
    title: "Feed the people you rescued",
    why: "A working farm keeps the town fed and lets it grow. More people mean more hands and defenders.",
    explain: "Press B near the keep and build a Farm on a free plot. An available villager takes \
              the job automatically. Let the worker start producing food.\n\n\
              If your party is still following you, press K at the keep to stand it down. \
              An already working farm counts too.",
    action: "Near the keep — B, choose Farm",
    icon: "stat:food",
    shot: Some("quests/build_farm.png"),
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static CALL_NIGHT: QuestDef = QuestDef {
    id: "campaign_call_night",
    title: "Call the first night",
    why: "Your people are home and the fields are working. Now see what they can defend together.",
    explain: "Walk to the war bell beside the keep and press E when you are ready. The first \
              assault tests your town and your blade. Keep the orks away from the keep until dawn.\n\n\
              You choose when preparation ends; there is time to gather or explore first.",
    action: "War bell — E when ready",
    icon: "buff:power",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static HOLD_KEEP: QuestDef = QuestDef {
    id: "campaign_hold_keep",
    title: "Hold the keep until dawn",
    why: "The keep protects everyone you brought home. Your townsfolk fight alongside you.",
    explain: "Fight beside your people and intercept orks heading for the keep. Blocking and \
              dodging give you room to recover between attacks. Defeat the assault to reach dawn.",
    action: "Defeat the assault — protect the keep",
    icon: "buff:power",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static PREPARE_DEFENSE: QuestDef = QuestDef {
    id: "campaign_prepare_defense",
    title: "Prepare for the farm raiders",
    why: "Tonight's raiders will attack the fields. Protecting food keeps your town growing after the battle.",
    explain: "At the keep, press E to open the War Table and buy an upgrade. Town Guard Arms \
              strengthens your people; Palisade Walls channels attackers towards the gates.\n\n\
              Choose an upgrade that suits your plan: stronger guards, new defenses, or your \
              own strength. See its effect before ringing the bell. \
              You can also rely on your own fighting and ring the bell when ready.",
    action: "Keep — E, buy Town Guard Arms or Walls",
    icon: "def_walls",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static DEFEND_FARM: QuestDef = QuestDef {
    id: "campaign_defend_farm",
    title: "Keep the fields standing",
    why: "Raiders are coming for your producers. A saved farm keeps feeding your people at dawn.",
    explain: "Watch the farms and intercept raiders that break away from the main assault. \
              Your upgrades and townsfolk support the defense.\n\n\
              Before battle, you can improve the defenses or raid a camp to weaken the incoming \
              force. Ring the bell when ready. If a producer falls, rebuild its ruins after the fight.",
    action: "War bell — E when ready",
    icon: "stat:food",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static DEFEND_FARM_WAVE: QuestDef = QuestDef {
    id: "campaign_defend_farm_wave",
    title: "Keep the fields standing",
    why: "Raiders are attacking your producers. A saved farm keeps feeding your people at dawn.",
    explain: "Intercept raiders heading for the farms. Your townsfolk and permanent upgrades \
              support the defense, while you can move to the producer most in danger.\n\n\
              Keep the keep standing too. If a producer falls, rebuild its ruins after the fight.",
    action: "Intercept farm raiders — protect the keep",
    icon: "stat:food",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static DISRUPT_RITUAL: QuestDef = QuestDef {
    id: "campaign_disrupt_ritual",
    title: "Choose how to face the shamans",
    why: "Shamans are preparing tonight's assault. Your choice by day changes the battle at night.",
    explain: "Raid the marked ritual camp and defeat its guards to remove its shamans from \
              tonight's wave. Rally your people at the keep with K if you want support.\n\n\
              Or stay home and buy Keep Archers or Watchtowers for ranged support. \
              You can fight without either preparation. \
              Ring the bell when ready.",
    action: "Raid the ritual camp, or prepare ranged support",
    icon: "def_keep_archers",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static SURVIVE_RITUAL: QuestDef = QuestDef {
    id: "campaign_survive_ritual",
    title: "Face the third night",
    why: "The raid changes who reaches your walls. Ranged defenders help control shamans that remain.",
    explain: "If you defeated the ritual camp during this preparation, its shamans cannot join \
              tonight's assault. Otherwise, fight their ranged attacks with cover, movement and \
              support from your defenders.\n\n\
              Ring the bell when ready, then hold the keep until dawn. Future days ask you to \
              combine what you have learned.",
    action: "War bell — E when ready; hold the keep",
    icon: "def_keep_archers",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static SURVIVE_RITUAL_WAVE: QuestDef = QuestDef {
    id: "campaign_survive_ritual_wave",
    title: "Hold against the ritual assault",
    why: "Your daytime choices shape this assault. Defend together and hold the keep until dawn.",
    explain: "Shamans remaining in the assault fight from range. Use cover and movement to \
              reach them, with your defenders supporting you.\n\n\
              Shamans from a ritual camp you defeated during this preparation do not join \
              tonight's wave. Finish the remaining attackers to reach dawn.",
    action: "Stop the attackers — hold the keep",
    icon: "def_keep_archers",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

static FREE_PLAY: QuestDef = QuestDef {
    id: "campaign_free_play",
    title: "Read the threat, choose your response",
    why: "Each day is a chance to change the night ahead.",
    explain: "Use the day's threat report to choose where to raid and what to defend. Camps you \
              clear weaken the next assault. Keep the town working while you prepare, then ring \
              the war bell when you are ready.",
    action: "Prepare for the reported threat",
    icon: "buff:power",
    shot: None,
    objective: crate::quest::Objective::SurviveNight,
    reward: NO_REWARD,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_day_connects_the_rescue_to_work_and_the_night() {
        let mut c = Campaign::default();
        assert_eq!(c.objective(0, false), Objective::Rally);
        c.rallied = true;
        assert_eq!(c.objective(0, false), Objective::Rescue);
        c.rescued = true;
        assert_eq!(c.objective(0, false), Objective::ReturnHome);
        c.returned = true;
        assert_eq!(c.objective(0, false), Objective::FeedPeople);
        c.farm_worked = true;
        assert_eq!(c.objective(0, false), Objective::CallNight);
        assert_eq!(c.objective(0, true), Objective::HoldKeep);
    }

    #[test]
    fn a_solo_rescue_does_not_require_rallying_afterwards() {
        let c = Campaign { rescued: true, ..Campaign::default() };
        assert_eq!(c.objective(0, false), Objective::ReturnHome);
        assert!(!c.rallied);
    }

    #[test]
    fn facts_done_early_still_count() {
        let mut c = Campaign {
            farm_worked: true,
            defense_bought: true,
            ..Campaign::default()
        };
        c.rescued = true;
        c.returned = true;
        assert_eq!(c.objective(0, false), Objective::CallNight);
        assert_eq!(c.objective(1, false), Objective::DefendFarm);
        // An arbitrary permanent purchase does not teach ranged defense against shamans.
        assert_eq!(c.objective(2, false), Objective::DisruptRitual);
    }

    #[test]
    fn each_night_can_be_called_without_finishing_preparation() {
        let c = Campaign::default();
        assert_eq!(c.objective(0, true), Objective::HoldKeep);
        assert_eq!(c.objective(1, true), Objective::DefendFarm);
        assert_eq!(c.objective(2, true), Objective::SurviveRitual);
        // Missed actions from day one do not trap advice in day one later in the run.
        assert_eq!(c.objective(1, false), Objective::PrepareDefense);
        assert_eq!(c.objective(2, false), Objective::DisruptRitual);
    }

    #[test]
    fn ritual_accepts_a_raid_or_actual_ranged_support() {
        let raid = Campaign { shaman_cleared: true, ..Campaign::default() };
        let defenders = Campaign { ranged_prepared: true, ..Campaign::default() };
        assert_eq!(raid.objective(2, false), Objective::SurviveRitual);
        assert_eq!(defenders.objective(2, false), Objective::SurviveRitual);
    }

    #[test]
    fn only_three_fresh_preparation_days_are_untimed() {
        let c = Campaign::default();
        assert!(c.learning_day(0));
        assert!(c.learning_day(1));
        assert!(c.learning_day(2));
        assert!(!c.learning_day(3));
        assert!(!c.learning_day(usize::MAX));
        assert_eq!(c.objective(3, false), Objective::FreePlay);
        assert_eq!(c.objective(3, true), Objective::FreePlay);
    }

    #[test]
    fn legacy_runs_never_restart_guidance_or_stop_the_clock() {
        let c = Campaign::legacy();
        assert!(!c.guided);
        assert!(c.rallied && c.rescued && c.returned && c.farm_worked);
        assert!(c.defense_bought && c.shaman_cleared && c.ranged_prepared);
        for night in 0..4 {
            assert!(!c.learning_day(night));
            assert_eq!(c.objective(night, false), Objective::FreePlay);
            assert_eq!(c.objective(night, true), Objective::FreePlay);
        }
    }

    #[test]
    fn lessons_have_no_resource_reward_or_numeric_tutorial_meter() {
        let c = Campaign::default();
        let lessons = [
            c.lesson(0, false),
            &RESCUE,
            &RETURN_HOME,
            &FEED_PEOPLE,
            &CALL_NIGHT,
            c.lesson(0, true),
            c.lesson(1, false),
            c.lesson(1, true),
            c.lesson(2, false),
            c.lesson(2, true),
            c.lesson(3, false),
        ];
        for lesson in lessons {
            assert_eq!(lesson.reward, NO_REWARD, "{}", lesson.id);
            assert!(!lesson.objective.is_metered(), "{}", lesson.id);
            assert!(!lesson.title.is_empty());
            assert!(!lesson.action.is_empty());
        }
    }

    #[test]
    fn wave_lessons_offer_battle_advice_instead_of_the_bell_hint() {
        let c = Campaign { defense_bought: true, ranged_prepared: true, ..Campaign::default() };
        for night in 1..3 {
            assert!(c.lesson(night, false).action.contains("War bell"));
            assert!(!c.lesson(night, true).action.contains("War bell"));
            assert_ne!(c.lesson(night, false).id, c.lesson(night, true).id);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn full_campaign_round_trip_retains_facts_and_same_day_raids() {
        let c = Campaign {
            guided: true,
            rallied: true,
            rescued: true,
            returned: true,
            farm_worked: true,
            defense_bought: true,
            shaman_cleared: true,
            ranged_prepared: true,
            briefed_day: 2,
            cleared_camps: vec![false, true, false, true],
            raid_day: 2,
        };
        let json = serde_json::to_string(&c).unwrap();
        let loaded: Campaign = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, c);
        assert_eq!(loaded.objective(2, false), Objective::SurviveRitual);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn additive_campaign_fields_receive_fresh_defaults() {
        let c: Campaign = serde_json::from_str(r#"{"rescued":true,"returned":true}"#).unwrap();
        assert!(c.guided);
        assert!(c.rescued && c.returned);
        assert!(!c.farm_worked && !c.ranged_prepared);
        assert_eq!(c.briefed_day, -1);
        assert_eq!(c.raid_day, -1);
        assert!(c.cleared_camps.is_empty());
        assert_eq!(c.objective(0, false), Objective::FeedPeople);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn missing_campaign_in_older_save_uses_legacy_at_container_boundary() {
        #[derive(serde::Deserialize)]
        struct OlderCompatibleSave {
            #[serde(default = "Campaign::legacy")]
            campaign: Campaign,
        }
        let saved: OlderCompatibleSave = serde_json::from_str("{}").unwrap();
        assert_eq!(saved.campaign, Campaign::legacy());
        assert!(!saved.campaign.learning_day(0));
    }
}
