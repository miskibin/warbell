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
    /// Build a Woodcutter's hut (automated wood) on any plot.
    BuildLumber,
    /// Build a Quarry / Stone Mine (automated stone) on any plot.
    BuildMine,
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
    LumberBuilt,
    MineBuilt,
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
    /// Optional screenshot asset path (under `assets/`, e.g. `"quests/buy_house.png"`) shown on the
    /// explainer card so the player *sees* the exact action, not just reads it. `None` = no shot.
    pub shot: Option<&'static str>,
    pub objective: Objective,
    pub reward: Reward,
}

/// The onboarding chain (linear) — a guided walk through the economy loop: **house → feed them →
/// automate wood → automate stone → upgrade → survive.** You start with a stipend of wood + stone
/// (see the Bevy layer's `START_WOOD`/`START_STONE`) so the very first step is already affordable,
/// and each build's reward bootstraps the next, so the chain never leaves you hard-stuck. Every
/// `explain` is written as numbered steps + a plain "why".
pub static QUESTS: &[QuestDef] = &[
    QuestDef {
        id: "buy_house",
        title: "Raise a House",
        why: "A hold is only as strong as the folk who man it. More houses mean more beds, and more \
              beds mean more villagers to work, tithe, and take up arms at night.",
        explain: "1. Walk to your castle and press B to open build mode.\n2. Choose the House — a \
                  gold ring marks the courtyard slot where it rises.\n3. Aim at the ring and press \
                  Enter (or click it) to raise it.\nYou start with timber and stone in the bank, so \
                  you can afford this one right away. Houses never burn.",
        action: "By the castle — press B, then build a House",
        icon: "stat:pop",
        shot: Some("quests/buy_house.png"),
        objective: Objective::BuildHouse,
        reward: Reward { gold: 10, wood: 0.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "gather_farm",
        title: "Gather Timber for a Farm",
        why: "Your new villagers have to eat — and right now the town grows no food. Before you can \
              raise a Farm to feed them, you need timber. A Farm costs 16 wood.",
        explain: "1. Head to the treeline near your castle.\n2. Walk up to a tree and swing your \
                  sword (LMB) to fell it — the logs drop straight into your stores.\n3. Keep \
                  chopping until you've banked enough for a Farm.\nWatch the wood count (top-left) \
                  climb as you cut. Later a Woodcutter does this for you.",
        action: "Chop trees — LMB",
        icon: "stat:wood",
        shot: Some("quests/gather_farm.png"),
        objective: Objective::GatherWood(16.0),
        reward: Reward { gold: 5, wood: 0.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "build_farm",
        title: "Build a Farm",
        why: "A staffed Farm grows food so your people stop starving — and a well-fed town pulls in \
              new settlers on its own. This is how the population snowball begins.",
        explain: "1. Return to your castle and press B for build mode.\n2. Choose the Farm — every \
                  free outer plot lights up with a gold ring.\n3. Aim at a glowing plot and press \
                  Enter (or click) to raise it.\nCheck the food readout (top-left): once the Farm is \
                  worked, it should tick toward the green.",
        action: "By the castle — press B, then build a Farm",
        icon: "stat:food",
        shot: Some("quests/build_farm.png"),
        objective: Objective::BuildFarm,
        reward: Reward { gold: 5, wood: 0.0, stone: 6.0, item: None },
    },
    QuestDef {
        id: "build_lumber",
        title: "Build a Woodcutter's Hut",
        why: "Chopping every log by hand is slow. A Woodcutter's hut puts a villager to work felling \
              trees and hauling the timber home — passive wood while you do everything else.",
        explain: "1. Press B by your castle for build mode.\n2. Choose the Woodcutter — free plots \
                  light up gold.\n3. Aim at a glowing plot and press Enter (or click) to raise it.\n\
                  It costs stone (which the last reward covered), and it needs a spare villager to \
                  staff it — that's why you built houses first.",
        action: "By the castle — press B, then build a Woodcutter",
        icon: "stat:wood",
        shot: Some("quests/build_lumber.png"),
        objective: Objective::BuildLumber,
        reward: Reward { gold: 5, wood: 12.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "build_quarry",
        title: "Build a Quarry",
        why: "Stone raises houses, walls, and towers — the things that keep the horde out. A Quarry \
              sets a miner to work the ore boulders so stone flows in without you swinging at rocks.",
        explain: "1. Press B by your castle for build mode.\n2. Choose the Stone Miner — free plots \
                  light up gold.\n3. Aim at a glowing plot and press Enter (or click) to raise it.\n\
                  With a Farm, a Woodcutter, and a Quarry running, your economy now feeds and \
                  supplies itself.",
        action: "By the castle — press B, then build a Stone Miner",
        icon: "stat:stone",
        shot: Some("quests/build_quarry.png"),
        objective: Objective::BuildMine,
        reward: Reward { gold: 15, wood: 0.0, stone: 0.0, item: None },
    },
    QuestDef {
        id: "war_table",
        title: "Open the War Table",
        why: "Lasting strength is bought, not found — walls, gold, and a sharper blade.",
        explain: "1. Walk up to the keep at the centre of your castle.\n2. Press E to open the War \
                  Table.\n3. Browse the four branches of permanent upgrades — spend gold and stone \
                  to claim them.",
        action: "At the keep — E",
        icon: "def_reinforce",
        shot: None,
        objective: Objective::OpenWarTable,
        reward: Reward { gold: 5, wood: 0.0, stone: 6.0, item: None },
    },
    QuestDef {
        id: "survive_night",
        title: "Survive the Night",
        why: "When your stores are stocked and your blade is ready, call the night yourself — then \
              hold the keep till dawn.",
        explain: "1. By day, walk to the war bell by the keep.\n2. Press E to ring it and summon \
                  the horde early.\n3. Hold the keep through the assault — it and your town mend at \
                  first light.",
        action: "War bell — E",
        icon: "buff:power",
        shot: None,
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
            | (Objective::BuildLumber, Signal::LumberBuilt)
            | (Objective::BuildMine, Signal::MineBuilt)
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
        assert_eq!(log.current().map(|q| q.id), Some("buy_house"));
    }

    #[test]
    fn gather_accumulates_then_completes_and_advances() {
        // Walk to the gather-timber quest (index 1) and accumulate toward its 16-wood goal.
        let mut log = QuestLog { active: 1, progress: 0.0 };
        assert_eq!(log.current().map(|q| q.id), Some("gather_farm"));
        assert_eq!(log.record(Signal::WoodGained(6.0)), None);
        assert!((log.fraction() - 6.0 / 16.0).abs() < 1e-9);
        assert_eq!(log.record(Signal::WoodGained(6.0)), None);
        // Crossing 16 returns the completed index (1) and moves to the build-farm quest.
        assert_eq!(log.record(Signal::WoodGained(4.0)), Some(1));
        assert_eq!(log.current().map(|q| q.id), Some("build_farm"));
        assert_eq!(log.progress, 0.0);
    }

    #[test]
    fn non_matching_signals_are_ignored() {
        let mut log = QuestLog::default(); // Raise a House (binary)
        // Gather / other-build signals don't touch a house objective.
        assert_eq!(log.record(Signal::StoneGained(99.0)), None);
        assert_eq!(log.record(Signal::FarmBuilt), None);
        assert_eq!(log.record(Signal::AnimalHunted), None);
        assert_eq!(log.active, 0);
        assert_eq!(log.progress, 0.0);
    }

    #[test]
    fn binary_objectives_complete_in_one_signal() {
        let mut log = QuestLog { active: 2, progress: 0.0 }; // Build a Farm
        assert_eq!(log.current().map(|q| q.id), Some("build_farm"));
        assert_eq!(log.record(Signal::FarmBuilt), Some(2));
        assert_eq!(log.current().map(|q| q.id), Some("build_lumber"));
    }

    #[test]
    fn producer_build_signals_advance_the_right_quests() {
        // Woodcutter quest (index 3) needs a LumberBuilt; a MineBuilt must not complete it.
        let mut log = QuestLog { active: 3, progress: 0.0 };
        assert_eq!(log.current().map(|q| q.id), Some("build_lumber"));
        assert_eq!(log.record(Signal::MineBuilt), None); // wrong producer — ignored
        assert_eq!(log.record(Signal::LumberBuilt), Some(3));
        assert_eq!(log.current().map(|q| q.id), Some("build_quarry"));
        // Quarry quest needs a MineBuilt.
        assert_eq!(log.record(Signal::LumberBuilt), None);
        assert_eq!(log.record(Signal::MineBuilt), Some(4));
        assert_eq!(log.current().map(|q| q.id), Some("war_table"));
    }

    #[test]
    fn full_walkthrough_reaches_completion() {
        let mut log = QuestLog::default();
        log.record(Signal::HouseBuilt); // buy_house
        log.record(Signal::WoodGained(16.0)); // gather_farm
        log.record(Signal::FarmBuilt); // build_farm
        log.record(Signal::LumberBuilt); // build_lumber
        log.record(Signal::MineBuilt); // build_quarry
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
