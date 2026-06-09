//! Bevy-free voice-line catalog + pure resolver. Every spoken line in the game is one [`Line`]
//! entry here: its speaker, transcript (the on-screen subtitle AND our in-code record of the
//! quote, per CLAUDE.md), whether it can be cut off, its barge-in priority, and optional reply
//! chains. The Bevy glue that actually plays clips lives in `director.rs`; this module is pure
//! data + decision logic so it can be unit-tested without spinning up an App.
//!
//! Model is the Valve "dynamic dialog" bark scheme (see
//! `docs/superpowers/plans/2026-06-09-voice-line-catalog-refactor.md`): a concept fires, the
//! resolver gathers candidate lines for it, filters by a per-line replay floor, and picks one.

use crate::biome::Biome;

/// Who owns a line — selects voice routing (head-locked vs spatial) via [`SPEAKERS`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Speaker {
    Hero,
    Villager,
    Ork,
}

/// How a speaker's voice is routed. Looked up from [`SPEAKERS`] by the director.
#[derive(Clone, Copy)]
pub struct SpeakerVoice {
    /// Head-locked (hero) vs world-positioned (villager/ork).
    pub spatial: bool,
    /// Base gain multiplier (× `AudioConfig.voice_vol`).
    pub gain: f32,
    /// Display name shown in the subtitle (`None` = no prefix, e.g. the hero's own musings).
    pub name: Option<&'static str>,
}

/// The voice registry: one entry per [`Speaker`]. Linear-scanned (3 entries).
pub const SPEAKERS: &[(Speaker, SpeakerVoice)] = &[
    (Speaker::Hero, SpeakerVoice { spatial: false, gain: 1.0, name: None }),
    (Speaker::Villager, SpeakerVoice { spatial: true, gain: 1.4, name: Some("Townsfolk") }),
    (Speaker::Ork, SpeakerVoice { spatial: true, gain: 0.85, name: None }),
];

pub fn speaker_voice(s: Speaker) -> SpeakerVoice {
    SPEAKERS.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).expect("every Speaker is registered")
}

/// A situation that asks for a line. Triggers (`detect_*` systems) emit one of these; the
/// resolver maps it to candidate [`Line`]s. Biome musings carry the biome so one concept covers
/// all five. The `Reply*` variants are chain targets dispatched by a finished line's `then`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Concept {
    // ── Hero event reactions (was `HeroEvent`) ──
    FirstStone,
    ChestOpen,
    FirstRescue,
    NightWarning,
    LowHp,
    Home,
    Equip,
    LevelUp,
    WaveSurvived,
    FirstKill,
    GoldRich,
    Broke,
    KeepHurt,
    ShrineHeal,
    // ── Hero biome musing (was `HeroLine(Biome)`) ──
    BiomeEntered(Biome),
    // ── Hero observational remarks (was `Trig`) ──
    Intro,
    NearTown,
    NearKids,
    NearPet,
    NearGuard,
    InKeep,
    NightMusing,
    QuietMusing,
    KillMusing,
    // ── Villager ──
    Greeting,
    SiegeFalls,
    Dawn,
    Rescued,
    // ── Ork ──
    OrkSpot,
    OrkDeath,
    // ── Chain reply concepts ──
    ReplyToVillagerJab,
}

/// A follow-up dispatched when a line finishes: ask `target` to look up a line whose
/// `reply_to == Some(concept)`. If none matches the (now-current) facts, nothing plays — the
/// chain self-terminates (the Valve "no explicit interruption" property).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chain {
    pub concept: Concept,
    pub target: Speaker,
}

/// One voice line — the whole record.
#[derive(Clone, Copy)]
pub struct Line {
    /// Stable key; also the clip stem at `audio/vo/<dir>/<id>.ogg` (dir per speaker).
    pub id: &'static str,
    pub speaker: Speaker,
    pub concept: Concept,
    /// Transcript: the on-screen subtitle AND our in-code record of the quote.
    pub text: &'static str,
    /// May a louder/just-as-loud new line cut this off mid-clip?
    pub interruptible: bool,
    /// Barge-in priority: a new line plays over a playing one only if `new.priority >= cur.priority`.
    pub priority: u8,
    /// Plays at most once this often (seconds); 0 = no floor. Per-line replay throttle.
    pub floor: f32,
    /// Plays at most ONCE per run (e.g. "first kill"); reset on a fresh run.
    pub once: bool,
    /// If set, this line is a valid reply to a dispatched chain `Concept`.
    pub reply_to: Option<Concept>,
    /// If set, dispatch this chain when the line finishes.
    pub then: Option<Chain>,
}

/// Convenience constructor for the common case (no reply_to / no then, interruptible, prio 10).
const fn line(id: &'static str, speaker: Speaker, concept: Concept, text: &'static str) -> Line {
    Line {
        id, speaker, concept, text,
        interruptible: true, priority: 10, floor: 0.0, once: false,
        reply_to: None, then: None,
    }
}

/// THE catalog. Filled in across the migration tasks (Phase C).
pub const LINES: &[Line] = &[
    // ── Hero event reactions ──
    Line { once: true,  priority: 20, ..line("stone",         Speaker::Hero, Concept::FirstStone,   "[older clip — text not transcribed]") },
    Line { floor: 300.0,              ..line("chest",         Speaker::Hero, Concept::ChestOpen,    "[older clip — text not transcribed]") },
    Line { once: true,  priority: 20, ..line("rescue",        Speaker::Hero, Concept::FirstRescue,  "[older clip — text not transcribed]") },
    Line { priority: 30,              ..line("night",         Speaker::Hero, Concept::NightWarning, "[older clip — text not transcribed]") },
    Line { floor: 300.0, priority: 15, ..line("hurt",         Speaker::Hero, Concept::LowHp,        "[older clip — text not transcribed]") },
    Line { once: true,  priority: 15, ..line("home",          Speaker::Hero, Concept::Home,         "[older clip — text not transcribed]") },
    Line { once: true,  priority: 15, ..line("equip",         Speaker::Hero, Concept::Equip,        "Mm, new armor. I should look it over in my satchel.") },
    Line { floor: 300.0, priority: 15, ..line("levelup",      Speaker::Hero, Concept::LevelUp,      "Stronger. The blade feels lighter than it did.") },
    Line { floor: 300.0, priority: 20, ..line("wave_survived", Speaker::Hero, Concept::WaveSurvived, "Dawn. We held. ...this time.") },
    Line { once: true,  priority: 15, ..line("first_kill",    Speaker::Hero, Concept::FirstKill,    "Down it goes. Plenty more where that came from.") },
    Line { once: true,                ..line("gold_rich",     Speaker::Hero, Concept::GoldRich,     "Coin enough to make the merchant smile. Good.") },
    Line { floor: 300.0,              ..line("broke",         Speaker::Hero, Concept::Broke,        "Pockets empty. Steel will have to do the talking.") },
    Line { floor: 300.0, priority: 25, ..line("keep_hurt",    Speaker::Hero, Concept::KeepHurt,     "The keep's taking a beating. Get to the walls.") },
    Line { floor: 300.0,              ..line("shrine_heal",   Speaker::Hero, Concept::ShrineHeal,   "The old stones still have mercy in them.") },
    // ── Hero biome musings (once per biome per run via `once`) ──
    Line { once: true, ..line("forest", Speaker::Hero, Concept::BiomeEntered(Biome::Forest), "[older clip — text not transcribed]") },
    Line { once: true, ..line("snow",   Speaker::Hero, Concept::BiomeEntered(Biome::Snow),   "[older clip — text not transcribed]") },
    Line { once: true, ..line("rock",   Speaker::Hero, Concept::BiomeEntered(Biome::Rocky),  "[older clip — text not transcribed]") },
    Line { once: true, ..line("desert", Speaker::Hero, Concept::BiomeEntered(Biome::Desert), "[older clip — text not transcribed]") },
    Line { once: true, ..line("swamp",  Speaker::Hero, Concept::BiomeEntered(Biome::Swamp),  "[older clip — text not transcribed]") },
    // ── Hero observational remarks ──
    // Trigger: NearTown — hero near townsfolk (folded from people/name/well/townday/laugh/market/woodpile/grumble lines)
    Line { floor: 300.0, priority: 5, ..line("people_a",   Speaker::Hero, Concept::NearTown, "These people. Loud, stubborn, alive. That's the whole point of all this, isn't it.") },
    Line { floor: 300.0, priority: 5, ..line("people_b",   Speaker::Hero, Concept::NearTown, "Look at them — bickering, trading, breathing. That's what the wall is for.") },
    Line { floor: 300.0, priority: 5, ..line("name_a",     Speaker::Hero, Concept::NearTown, "Half of them don't know my name. Good. Means they're free to forget the war.") },
    Line { floor: 300.0, priority: 5, ..line("name_b",     Speaker::Hero, Concept::NearTown, "They nod and move on. Better that than knowing what's out past the gate.") },
    Line { floor: 300.0, priority: 5, ..line("well_a",     Speaker::Hero, Concept::NearTown, "Fresh water, idle talk, small quarrels. The things we're actually fighting for.") },
    Line { floor: 300.0, priority: 5, ..line("well_b",     Speaker::Hero, Concept::NearTown, "Gossip at the well. Sounds like nothing. Sounds like peace, is what it is.") },
    Line { floor: 300.0, priority: 5, ..line("townday_a",  Speaker::Hero, Concept::NearTown, "A town that still argues over fences and taxes. Means there's still a town.") },
    Line { floor: 300.0, priority: 5, ..line("townday_b",  Speaker::Hero, Concept::NearTown, "Still squabbling over hens and rent. The day that stops, we've lost.") },
    Line { floor: 300.0, priority: 5, ..line("laugh_a",    Speaker::Hero, Concept::NearTown, "They laugh like there was never a siege. ...Maybe that's the victory.") },
    Line { floor: 300.0, priority: 5, ..line("laugh_b",    Speaker::Hero, Concept::NearTown, "Laughter in the square, after all this. ...Maybe that's the whole point.") },
    Line { floor: 300.0, priority: 5, ..line("market_a",   Speaker::Hero, Concept::NearTown, "Coin changes hands, bread gets baked, the world turns. I just keep the wolves off it.") },
    Line { floor: 300.0, priority: 5, ..line("market_b",   Speaker::Hero, Concept::NearTown, "Buy, sell, haggle — honest work. I'd take it over mine most days.") },
    Line { floor: 300.0, priority: 5, ..line("woodpile_a", Speaker::Hero, Concept::NearTown, "Stack it high. The nights are only getting longer.") },
    Line { floor: 300.0, priority: 5, ..line("woodpile_b", Speaker::Hero, Concept::NearTown, "More wood. Good. Cold kills slower than orks — but it still kills.") },
    Line { floor: 300.0, priority: 5, ..line("grumble_a",  Speaker::Hero, Concept::NearTown, "The taxes, aye. Tell it to the orks — they're wonderful listeners.") },
    Line { floor: 300.0, priority: 5, ..line("grumble_b",  Speaker::Hero, Concept::NearTown, "Complain to me about rent. I'll forward it to the horde — they decide who pays.") },
    // Trigger: NearKids — hero near child villagers
    Line { floor: 300.0, priority: 5, ..line("kids_a",     Speaker::Hero, Concept::NearKids, "Mind those sticks, little ones. ...Gods, let them stay little ones a while longer.") },
    Line { floor: 300.0, priority: 5, ..line("kids_b",     Speaker::Hero, Concept::NearKids, "Run while you can, little ones. Wish I still had the knees for it.") },
    // Trigger: NearPet — hero near a dog or cat
    Line { floor: 300.0, priority: 5, ..line("pet_a",      Speaker::Hero, Concept::NearPet, "At least the hound's got the right idea. Rest while the light holds.") },
    Line { floor: 300.0, priority: 5, ..line("pet_b",      Speaker::Hero, Concept::NearPet, "The cat fears nothing. Must be nice, being a cat.") },
    // Trigger: NearGuard — hero near a guard / militia
    Line { floor: 300.0, priority: 5, ..line("guard_a",    Speaker::Hero, Concept::NearGuard, "Stand tall. The wall holds because you do.") },
    Line { floor: 300.0, priority: 5, ..line("guard_b",    Speaker::Hero, Concept::NearGuard, "Eyes on the dark, soldier. I'll be right beside you when it comes.") },
    // Trigger: InKeep — hero inside the keep footprint
    Line { floor: 300.0, priority: 5, ..line("keep_a",     Speaker::Hero, Concept::InKeep, "Old stones. They've outlived better men than me. They'll outlive me too.") },
    Line { floor: 300.0, priority: 5, ..line("keep_b",     Speaker::Hero, Concept::InKeep, "This keep's swallowed a hundred sieges. One more won't choke it.") },
    // Trigger: NightMusing — during a wave
    Line { floor: 300.0, priority: 5, ..line("night_a",    Speaker::Hero, Concept::NightMusing, "Stars are out. Somewhere up there someone's keeping a tally. Hope I'm ahead.") },
    Line { floor: 300.0, priority: 5, ..line("night_b",    Speaker::Hero, Concept::NightMusing, "Clear night. Pretty — if you forget what comes with the dark.") },
    // Trigger: QuietMusing — prep phase, no orks nearby
    Line { floor: 300.0, priority: 5, ..line("quiet_a",    Speaker::Hero, Concept::QuietMusing, "Quiet day. I've learned not to trust quiet days.") },
    Line { floor: 300.0, priority: 5, ..line("quiet_b",    Speaker::Hero, Concept::QuietMusing, "Too calm. The quiet always sends a bill, sooner or later.") },
    // Trigger: KillMusing — after a kill
    Line { floor: 300.0, priority: 5, ..line("kill_a",     Speaker::Hero, Concept::KillMusing, "One more for the pile. I stopped counting around the second winter.") },
    Line { floor: 300.0, priority: 5, ..line("kill_b",     Speaker::Hero, Concept::KillMusing, "Down. There's always another behind it. Always is.") },
    // ── Hero intro lines (once per run — the tutorial in the hero's own voice) ──
    // Clips at assets/audio/vo/hero/intro_a.ogg and intro_b.ogg (not yet shipped — guard skips them silently)
    Line { once: true, priority: 3, ..line("intro_a", Speaker::Hero, Concept::Intro, "Daylight's short — open the chests, gather coin and stone, buy what'll keep you breathing. When dark comes, the orks come for the keep. We hold it.") },
    Line { once: true, priority: 3, ..line("intro_b", Speaker::Hero, Concept::Intro, "By day you scavenge — chests, ore, gold — and arm up at the War Table. By night the horde hits these walls. Keep the keep standing. Don't waste the light.") },
];

/// All catalog lines for a concept, in declaration order.
pub fn candidates(concept: Concept) -> impl Iterator<Item = &'static Line> {
    LINES.iter().filter(move |l| l.concept == concept)
}

/// All catalog lines that are a valid reply to a dispatched chain concept.
pub fn replies_to(concept: Concept) -> impl Iterator<Item = &'static Line> {
    LINES.iter().filter(move |l| l.reply_to == Some(concept))
}

/// xorshift — same as the audio module's RNG, duplicated here to keep `lines` Bevy/dep-free.
fn next_rng(s: &mut u32) -> u32 {
    if *s == 0 {
        *s = 0x9e37_79b9;
    }
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}
fn frand(s: &mut u32) -> f32 {
    (next_rng(s) & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

/// Does this line pass its per-line replay gates right now? Blocked if it's a `once` line already
/// played this run, or if it played more recently than `floor` seconds ago.
pub fn passes_gates(
    line: &Line,
    last: &std::collections::HashMap<&'static str, f32>,
    played_once: &std::collections::HashSet<&'static str>,
    now: f32,
) -> bool {
    if line.once && played_once.contains(line.id) {
        return false;
    }
    now - *last.get(line.id).unwrap_or(&f32::NEG_INFINITY) >= line.floor
}

/// Pick a line for `concept`: among candidates, keep only those passing their per-line gates
/// (replay floor + once-per-run), then random-pick. `None` if no candidate is currently eligible.
pub fn pick_line(
    concept: Concept,
    last: &std::collections::HashMap<&'static str, f32>,
    played_once: &std::collections::HashSet<&'static str>,
    now: f32,
    rng: &mut u32,
) -> Option<&'static Line> {
    let fresh: Vec<&'static Line> =
        candidates(concept).filter(|l| passes_gates(l, last, played_once, now)).collect();
    if fresh.is_empty() {
        return None;
    }
    let i = (frand(rng) * fresh.len() as f32) as usize % fresh.len();
    Some(fresh[i])
}

/// What a speaker is currently saying (tracked by the director's `VoiceManager`).
#[derive(Clone, Copy, Debug)]
pub struct Active {
    pub id: &'static str,
    /// `elapsed_secs` when the clip is estimated to finish.
    pub ends_at: f32,
    pub priority: u8,
    pub interruptible: bool,
    /// Chain to dispatch when it finishes (consumed once).
    pub then: Option<Chain>,
}

/// May a new line of `new_priority` start now, given the speaker's current `active` line?
/// Rule (Pixel Crushers): play if the speaker is idle, its line already finished, or the current
/// line is interruptible AND the newcomer is at least as important.
pub fn can_play(active: Option<&Active>, now: f32, new_priority: u8) -> bool {
    match active {
        None => true,
        Some(a) if now >= a.ends_at => true,
        Some(a) => a.interruptible && new_priority >= a.priority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn candidates_filters_by_concept() {
        assert_eq!(candidates(Concept::LevelUp).count(), 1);
        assert_eq!(candidates(Concept::ChestOpen).count(), 1);
    }

    #[test]
    fn pick_line_none_when_no_candidates() {
        let (last, once) = (HashMap::new(), HashSet::new());
        let mut rng = 1;
        // OrkSpot has no catalog entry yet
        assert!(pick_line(Concept::OrkSpot, &last, &once, 100.0, &mut rng).is_none());
    }

    #[test]
    fn pick_line_returns_candidate() {
        let (last, once) = (HashMap::new(), HashSet::new());
        let mut rng = 1;
        assert_eq!(pick_line(Concept::LevelUp, &last, &once, 0.0, &mut rng).unwrap().id, "levelup");
    }

    fn test_line() -> Line {
        line("t", Speaker::Hero, Concept::LevelUp, "x")
    }

    #[test]
    fn passes_gates_floor_blocks_then_clears() {
        let mut l = test_line();
        l.floor = 300.0;
        let mut last = HashMap::new();
        last.insert("t", 50.0);
        let once = HashSet::new();
        assert!(!passes_gates(&l, &last, &once, 60.0));  // 10s later → floored
        assert!(passes_gates(&l, &last, &once, 400.0));  // 350s later → cleared
    }

    #[test]
    fn passes_gates_first_play_ignores_floor() {
        let mut l = test_line();
        l.floor = 300.0;
        let (last, once) = (HashMap::new(), HashSet::new());
        assert!(passes_gates(&l, &last, &once, 0.0)); // never played → passes
    }

    #[test]
    fn passes_gates_once_blocks_after_played() {
        let mut l = test_line();
        l.once = true;
        let last = HashMap::new();
        let mut once = HashSet::new();
        assert!(passes_gates(&l, &last, &once, 0.0));        // not yet played
        once.insert("t");
        assert!(!passes_gates(&l, &last, &once, 1000.0));    // played once → blocked forever
    }

    #[test]
    fn every_speaker_is_registered() {
        for s in [Speaker::Hero, Speaker::Villager, Speaker::Ork] {
            let _ = speaker_voice(s);
        }
    }

    fn active(prio: u8, interruptible: bool, ends_at: f32) -> Active {
        Active { id: "x", ends_at, priority: prio, interruptible, then: None }
    }

    #[test]
    fn can_play_when_idle() {
        assert!(can_play(None, 0.0, 0));
    }

    #[test]
    fn can_play_when_current_finished() {
        let a = active(255, false, 5.0);
        assert!(can_play(Some(&a), 6.0, 0));
    }

    #[test]
    fn cannot_interrupt_protected_line() {
        let a = active(50, false, 100.0);
        assert!(!can_play(Some(&a), 1.0, 255));
    }

    #[test]
    fn interrupt_needs_equal_or_higher_priority() {
        let a = active(50, true, 100.0);
        assert!(!can_play(Some(&a), 1.0, 49));
        assert!(can_play(Some(&a), 1.0, 50));
        assert!(can_play(Some(&a), 1.0, 200));
    }
}
