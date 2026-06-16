//! **Guidance voice triggers** — the tutorial-in-character layer. New players don't know *why* they
//! need a farm / woodcutter / mine / houses, or what's currently wrong (starving? broke? walls
//! down?). This watches the live town/economy state and, sparingly, asks for ONE guidance line —
//! pooled per concept across the hero (a musing) and the townsfolk (a gripe), so the same nudge
//! sometimes comes from the knight and sometimes from a peasant (see `lines.rs`).
//!
//! Kept deliberately quiet (the explicit design goal — "don't make the game noisy"):
//!   - **Prep-only** — never mid-siege; advice is for the daytime breather.
//!   - **One global throttle** ([`ADVICE_GAP`]) so at most one nudge every ~75 s, *plus* the
//!     per-line replay floors in the catalog (240–600 s) so the same advice can't repeat soon.
//!   - **One-mouth courtesy** — never speaks over the hero or a villager already talking.
//!   - **Priority order** — emits only the single most-urgent condition that holds; if its pooled
//!     lines are all on cooldown the director just stays silent that cycle.
//!
//! Nothing here is saved — it's all recomputed from live resources (like `hints.rs` / `Toasts`).

use bevy::prelude::*;

use tileworld_core::town_store::MAX_HOUSES;

use crate::economy::{Bank, Upgrades};
use crate::player::PlayerRes;
use crate::siege::{GamePhase, Siege};
use crate::town::TownRes;

use super::director::{Speak, VoiceManager};
use super::lines::Concept;

/// Minimum seconds between any two guidance nudges (on top of each line's own replay floor).
const ADVICE_GAP: f32 = 75.0;
/// How long, when nothing currently warrants a nudge, before we re-evaluate (cheap poll + debounce).
const POLL_GAP: f32 = 8.0;
/// Gold pile that prompts the "spend it at the War Table" nudge.
const RICH_GOLD: i64 = 150;
/// Wood/stone below this reads as "you can't build anything" → nudge to staff the producer.
const LOW_STOCK: f64 = 12.0;

#[derive(Resource)]
pub(crate) struct AdviceTrigger {
    /// Earliest time the next nudge may be emitted.
    next: f32,
    /// Population seen last frame — the baseline for detecting a drop. Tracked EVERY Prep frame
    /// (not just on a nudge) so it never goes stale.
    prev_pop: Option<u32>,
    /// A population drop has been observed but not yet voiced. Latched on the drop and held through
    /// the throttle / one-mouth wait so a loss during a busy window isn't lost; cleared when the
    /// dirge actually fires.
    pop_lost_pending: bool,
}

impl Default for AdviceTrigger {
    fn default() -> Self {
        // Hold off the first ~35 s so a fresh player gets to look around before being advised.
        Self { next: 35.0, prev_pop: None, pop_lost_pending: false }
    }
}

/// Fresh run: re-arm the throttle and forget the population baseline (so a new run's small starting
/// town isn't read as a mass die-off relative to a previous run).
pub(crate) fn reset_advice(mut t: ResMut<AdviceTrigger>) {
    *t = AdviceTrigger::default();
}

/// Pick the single most-urgent guidance concept for the current town state, or `None` if all's
/// well and there's nothing to say. `lost` = the population dropped since the last nudge.
fn advice_for(
    town: &tileworld_core::town_store::Town,
    bank: &tileworld_core::resource_store::ResourceState,
    gold: i64,
    walls: bool,
    wave_index: i32,
    lost: bool,
) -> Option<Concept> {
    let pop = town.population;
    let net = town.net_food();
    // Most urgent first; the player hears whichever tops the list.
    Some(if net < 0.0 && pop > 0 {
        Concept::AdviseFarm // starving — the headline nudge
    } else if lost {
        Concept::PopLost
    } else if pop >= town.pop_cap() && net > 0.0 && town.houses < MAX_HOUSES {
        Concept::AdviseHouses // surplus food but capped → more homes
    } else if bank.wood() < LOW_STOCK {
        Concept::AdviseWood
    } else if bank.stone() < LOW_STOCK {
        Concept::AdviseStone
    } else if !walls && wave_index >= 1 {
        Concept::AdviseWalls // survived a night with no walls up
    } else if gold >= RICH_GOLD {
        Concept::AdviseUpgrade
    } else if net > 0.0 && walls && pop >= 4 {
        Concept::TownThriving // everything's fine — a rare contented remark
    } else if wave_index < 1 {
        Concept::PrepNudge // brand-new run: orient the lost player ("loot, build, arm")
    } else if walls && net >= 0.0 {
        Concept::AdviseBell // defensible + fed → nudge toward ringing in the night
    } else {
        return None;
    })
}

/// The guidance trigger. Registered `Modal::None`-gated (no advice while a panel is open).
pub(crate) fn detect_town_advice(
    time: Res<Time>,
    siege: Res<Siege>,
    town: Res<TownRes>,
    bank: Res<Bank>,
    up: Res<Upgrades>,
    player: Res<PlayerRes>,
    mgr: Res<VoiceManager>,
    mut t: ResMut<AdviceTrigger>,
    mut speak: MessageWriter<Speak>,
) {
    // Advice belongs to the daytime breather — never nag mid-siege.
    if siege.phase != GamePhase::Prep {
        return;
    }
    let now = time.elapsed_secs();

    // Track population EVERY Prep frame so the loss baseline never goes stale, and latch a drop
    // until it's voiced — so a death during the throttle / while someone's talking isn't dropped.
    let pop = town.0.population;
    if t.prev_pop.is_some_and(|p| pop < p) {
        t.pop_lost_pending = true;
    }
    t.prev_pop = Some(pop);

    if now < t.next {
        return;
    }
    // One-mouth courtesy: don't talk over anyone already speaking — retry next frame.
    if mgr.hero_speaking(now) || mgr.others_speaking(now) {
        return;
    }

    let walls = up.0.is_purchased("def_walls");
    match advice_for(&town.0, &bank.0, player.0.gold, walls, siege.wave_index, t.pop_lost_pending) {
        Some(concept) => {
            // at=None → the director positions a villager line at the hero (head-locked for the
            // hero's own musings); the catalog + replay floors decide which pooled line, if any.
            speak.write(Speak::new(concept));
            if concept == Concept::PopLost {
                t.pop_lost_pending = false; // dirge requested — clear the latch
            }
            t.next = now + ADVICE_GAP;
        }
        // Nothing to advise right now: short poll so we're not evaluating every frame.
        None => t.next = now + POLL_GAP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tileworld_core::resource_store::ResourceState;
    use tileworld_core::town_store::Town;

    fn town(pop: u32, houses: u32) -> Town {
        let mut t = Town::new(8, pop);
        t.houses = houses;
        t
    }
    fn stocked(wood: f64, stone: f64) -> ResourceState {
        let mut b = ResourceState::default();
        b.wood = wood;
        b.stone = stone;
        b
    }

    #[test]
    fn starving_town_is_told_to_farm() {
        // pop 5, no farms → net_food negative (upkeep with no production).
        let t = town(5, 3);
        assert_eq!(advice_for(&t, &stocked(99.0, 99.0), 0, true, 3, false), Some(Concept::AdviseFarm));
    }

    #[test]
    fn lost_population_voices_the_dirge_over_lesser_nudges() {
        // Healthy stocks + gold, but a peasant died → PopLost outranks the gold/upgrade nudge.
        let t = town(2, 3); // pop 2 = larder pair, net_food >= 0 (no deficit)
        assert_eq!(advice_for(&t, &stocked(99.0, 99.0), 999, true, 3, true), Some(Concept::PopLost));
    }

    #[test]
    fn low_wood_then_low_stone_are_flagged() {
        let t = town(2, 3);
        assert_eq!(advice_for(&t, &stocked(0.0, 99.0), 0, true, 3, false), Some(Concept::AdviseWood));
        assert_eq!(advice_for(&t, &stocked(99.0, 0.0), 0, true, 3, false), Some(Concept::AdviseStone));
    }

    #[test]
    fn no_walls_after_a_night_prompts_walls() {
        let t = town(2, 3);
        assert_eq!(advice_for(&t, &stocked(99.0, 99.0), 0, false, 2, false), Some(Concept::AdviseWalls));
    }

    #[test]
    fn first_day_orients_the_player() {
        // Day 1 (wave_index -1), nothing wrong, no walls yet → the generic "use the hours" nudge.
        let t = town(2, 3);
        assert_eq!(advice_for(&t, &stocked(99.0, 99.0), 0, false, -1, false), Some(Concept::PrepNudge));
    }
}
