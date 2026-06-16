//! **Tutorial quest chain** — the linear onboarding objectives that teach the core day loop
//! (gather → build → feed → hunt → upgrade → survive). Pure + tested; the Bevy `src/quest.rs`
//! layer feeds it engine [`Signal`]s and renders the tracker / explain card.
//!
//! Content is a static [`QUESTS`] table; the per-run state is the tiny serde-able [`QuestLog`]
//! (one index + one progress float), which rides the save. Objectives are **abstract** — core
//! knows nothing of Bevy `Modal`/`Town`; the src layer maps engine events onto [`Signal`]s and
//! reports them. A signal that doesn't match the active objective is simply ignored, so the
//! Bevy layer is free to fire signals liberally without bookkeeping.

/// One objective a quest can carry. The numeric variants hold their goal; the rest are binary
/// (done in one event).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Objective {
    /// Gather (chop) at least this much wood while the quest is active.
    GatherWood(f64),
    /// Gather (mine) at least this much stone while the quest is active.
    GatherStone(f64),
    /// Build a Farm on any plot.
    BuildFarm,
    /// Raise a House (population cap +).
    BuildHouse,
    /// Kill at least this many wild animals.
    HuntAnimal(u32),
    /// Open the War Table (upgrade tree) once.
    OpenWarTable,
    /// Survive one night siege.
    SurviveNight,
}

impl Objective {
    /// The numeric goal: gather amount, hunt count, or `1.0` for a binary objective.
    pub fn target(self) -> f64 {
        match self {
            Objective::GatherWood(n) | Objective::GatherStone(n) => n,
            Objective::HuntAnimal(n) => n as f64,
            _ => 1.0,
        }
    }

    /// Metered objectives (gather / hunt) show a progress bar + counter in the tracker; binary
    /// ones show only the action hint.
    pub fn is_metered(self) -> bool {
        matches!(
            self,
            Objective::GatherWood(_) | Objective::GatherStone(_) | Objective::HuntAnimal(_)
        )
    }
}

/// An abstract engine fact the Bevy layer reports to the active quest. Matched against the
/// active [`Objective`] in [`QuestLog::record`]; an unmatched signal is a no-op.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Signal {
    WoodGained(f64),
    StoneGained(f64),
    FarmBuilt,
    HouseBuilt,
    AnimalHunted,
    WarTableOpened,
    NightSurvived,
}

/// What finishing a quest grants. Resource amounts are added to the bank/hero; `item` (id, count)
/// drops into the satchel. Everything is zeroable so each quest only sets what it gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reward {
    pub gold: i64,
    pub wood: f64,
    pub stone: f64,
    pub item: Option<(&'static str, i64)>,
}

/// A quest definition (static content).
#[derive(Debug, PartialEq)]
pub struct QuestDef {
    /// Stable id (save/debug only — progress is stored as an index, not an id).
    pub id: &'static str,
    /// Plain, imperative title shown in the tracker ("Gather Wood").
    pub title: &'static str,
    /// The motivational body — *why* it matters, not which key to press.
    pub why: &'static str,
    /// The longer mechanic explanation shown on the click-out card.
    pub explain: &'static str,
    /// A short "what to do" hint ("Chop trees — LMB").
    pub action: &'static str,
    /// `IconAtlas` key for the tracker / card icon.
    pub icon: &'static str,
    pub objective: Objective,
    pub reward: Reward,
}

/// The onboarding chain (linear). Targets/rewards are tuned so each leaves enough to do the next
/// build; see the design doc.
pub static QUESTS: &[QuestDef] = &[
    QuestDef {
        id: "gather_wood",
        title: "Gather Wood",
        why: "Timber is the bones of every building. Lay in a store before you raise anything.",
        explain: "Wood comes from the forest. Walk up to a tree and swing (LMB) to fell it — the \
                  logs drop straight into your stores. Later a Woodcutter fells them for you.",
        action: "Chop trees — LMB",
        icon: "stat:wood",
        objective: Objective::GatherWood(12.0),
        reward: Reward { gold: 5, wood: 0.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "build_farm",
        title: "Build a Farm",
        why: "Your people need food, and a well-fed town draws in new settlers.",
        explain: "Stand on any open plot (a gold ring marks it) and press E to open the build \
                  menu. A staffed Farm feeds the town; surplus food pulls in new villagers.",
        action: "Stand on a plot — E",
        icon: "stat:food",
        objective: Objective::BuildFarm,
        reward: Reward { gold: 0, wood: 6.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "gather_stone",
        title: "Gather Stone",
        why: "Stone raises houses and the walls that keep the horde out.",
        explain: "Ore boulders are scattered around the island. Smash one with your attack (LMB) \
                  to break loose stone. Later a Stone Miner works them for you.",
        action: "Smash ore — LMB",
        icon: "stat:stone",
        objective: Objective::GatherStone(8.0),
        reward: Reward { gold: 5, wood: 0.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "build_house",
        title: "Build a House",
        why: "No beds, no newcomers. A house lifts the cap so your town can grow past its founders.",
        explain: "Press E at the timber site inside the walls to raise a house. Each one shelters \
                  two more villagers — and houses never burn.",
        action: "Timber site — E",
        icon: "stat:pop",
        objective: Objective::BuildHouse,
        reward: Reward { gold: 12, wood: 0.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "hunt_food",
        title: "Hunt for Food",
        why: "The wild has its own bounty. Meat keeps a knight on his feet through a long night.",
        explain: "Wild beasts roam every biome. Cut one down (LMB) and it drops meat and hides — \
                  food you can eat (Q) to heal.",
        action: "Hunt a beast — LMB",
        icon: "stat:food",
        objective: Objective::HuntAnimal(1),
        reward: Reward { gold: 0, wood: 0.0, stone: 0.0, item: Some(("bread", 2)) },
    },
    QuestDef {
        id: "war_table",
        title: "Open the War Table",
        why: "Lasting strength is bought, not found — walls, gold, and a sharper blade.",
        explain: "Press E at the keep to open the War Table. Four branches of permanent upgrades; \
                  spend gold and stone to claim them.",
        action: "At the keep — E",
        icon: "def_reinforce",
        objective: Objective::OpenWarTable,
        reward: Reward { gold: 5, wood: 0.0, stone: 6.0, item: None },
    },
    QuestDef {
        id: "survive_night",
        title: "Survive the Night",
        why: "When your stores are stocked and your blade is ready, call the night yourself — then \
              hold the keep till dawn.",
        explain: "Ring the war bell (E, by day) to summon the horde early. Hold the keep through \
                  the assault; it and your town mend at first light.",
        action: "War bell — E",
        icon: "buff:power",
        objective: Objective::SurviveNight,
        reward: Reward { gold: 25, wood: 0.0, stone: 0.0, item: None },
    },
];

/// Per-run quest progress: the active index into [`QUESTS`] (== `QUESTS.len()` once the chain is
/// done) and accumulated progress toward the active objective. The whole save-relevant state.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct QuestLog {
    pub active: usize,
    pub progress: f64,
}

impl QuestLog {
    /// The active quest, or `None` when the chain is complete.
    pub fn current(&self) -> Option<&'static QuestDef> {
        QUESTS.get(self.active)
    }

    /// Whether the whole chain has been finished.
    pub fn is_complete(&self) -> bool {
        self.active >= QUESTS.len()
    }

    /// Progress toward the active objective as a `0..=1` fraction (for the tracker bar).
    pub fn fraction(&self) -> f64 {
        match self.current() {
            Some(q) => (self.progress / q.objective.target()).clamp(0.0, 1.0),
            None => 1.0,
        }
    }

    /// Feed an engine [`Signal`]. If it matches the active objective, advance progress; returns
    /// the just-completed quest index (so the caller grants its reward + celebrates) when the
    /// objective tips over. A non-matching signal is a no-op (`None`).
    pub fn record(&mut self, sig: Signal) -> Option<usize> {
        let q = self.current()?;
        let obj = q.objective;
        let matched = match (obj, sig) {
            (Objective::GatherWood(_), Signal::WoodGained(a)) => {
                self.progress += a;
                true
            }
            (Objective::GatherStone(_), Signal::StoneGained(a)) => {
                self.progress += a;
                true
            }
            (Objective::HuntAnimal(_), Signal::AnimalHunted) => {
                self.progress += 1.0;
                true
            }
            (Objective::BuildFarm, Signal::FarmBuilt)
            | (Objective::BuildHouse, Signal::HouseBuilt)
            | (Objective::OpenWarTable, Signal::WarTableOpened)
            | (Objective::SurviveNight, Signal::NightSurvived) => {
                self.progress = obj.target();
                true
            }
            _ => false,
        };
        if !matched {
            return None;
        }
        // `1e-9` slack absorbs float accumulation so an exact-target gather still tips over.
        if self.progress + 1e-9 >= obj.target() {
            let done = self.active;
            self.active += 1;
            self.progress = 0.0;
            Some(done)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_log_starts_at_the_first_quest() {
        let log = QuestLog::default();
        assert_eq!(log.active, 0);
        assert!(!log.is_complete());
        assert_eq!(log.current().map(|q| q.id), Some("gather_wood"));
    }

    #[test]
    fn gather_accumulates_then_completes_and_advances() {
        let mut log = QuestLog::default(); // Gather Wood (12)
        assert_eq!(log.record(Signal::WoodGained(5.0)), None);
        assert!((log.fraction() - 5.0 / 12.0).abs() < 1e-9);
        assert_eq!(log.record(Signal::WoodGained(5.0)), None);
        // Crossing 12 returns the completed index (0) and moves to the farm quest.
        assert_eq!(log.record(Signal::WoodGained(3.0)), Some(0));
        assert_eq!(log.current().map(|q| q.id), Some("build_farm"));
        assert_eq!(log.progress, 0.0);
    }

    #[test]
    fn non_matching_signals_are_ignored() {
        let mut log = QuestLog::default(); // Gather Wood
        // Stone / build / hunt signals don't touch a wood objective.
        assert_eq!(log.record(Signal::StoneGained(99.0)), None);
        assert_eq!(log.record(Signal::FarmBuilt), None);
        assert_eq!(log.record(Signal::AnimalHunted), None);
        assert_eq!(log.active, 0);
        assert_eq!(log.progress, 0.0);
    }

    #[test]
    fn binary_objectives_complete_in_one_signal() {
        let mut log = QuestLog { active: 1, progress: 0.0 }; // Build a Farm
        assert_eq!(log.record(Signal::FarmBuilt), Some(1));
        assert_eq!(log.current().map(|q| q.id), Some("gather_stone"));
    }

    #[test]
    fn hunt_counts_kills() {
        // Walk to the hunt quest (index 4) and confirm a single kill finishes it.
        let mut log = QuestLog { active: 4, progress: 0.0 };
        assert_eq!(log.current().map(|q| q.id), Some("hunt_food"));
        assert_eq!(log.record(Signal::AnimalHunted), Some(4));
        assert_eq!(log.current().map(|q| q.id), Some("war_table"));
    }

    #[test]
    fn full_walkthrough_reaches_completion() {
        let mut log = QuestLog::default();
        log.record(Signal::WoodGained(12.0)); // gather_wood
        log.record(Signal::FarmBuilt); // build_farm
        log.record(Signal::StoneGained(8.0)); // gather_stone
        log.record(Signal::HouseBuilt); // build_house
        log.record(Signal::AnimalHunted); // hunt_food
        log.record(Signal::WarTableOpened); // war_table
        assert!(!log.is_complete());
        assert_eq!(log.record(Signal::NightSurvived), Some(6)); // survive_night
        assert!(log.is_complete());
        assert_eq!(log.current(), None);
        assert_eq!(log.fraction(), 1.0);
    }

    #[test]
    fn signals_after_completion_are_noops() {
        let mut log = QuestLog { active: QUESTS.len(), progress: 0.0 };
        assert!(log.is_complete());
        assert_eq!(log.record(Signal::WoodGained(50.0)), None);
        assert_eq!(log.record(Signal::NightSurvived), None);
    }

    #[test]
    fn every_quest_reward_is_nonempty() {
        // Each onboarding quest should pay out *something* (keeps the chain reinforcing).
        for q in QUESTS {
            let r = q.reward;
            let pays = r.gold != 0 || r.wood > 0.0 || r.stone > 0.0 || r.item.is_some();
            assert!(pays, "quest {} has an empty reward", q.id);
        }
    }
}
