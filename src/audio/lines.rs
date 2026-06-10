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
    /// Random playback-speed (pitch) range rolled per utterance — `(1.0, 1.0)` = no shift.
    pub pitch: (f32, f32),
}

/// The voice registry: one entry per [`Speaker`]. Linear-scanned (3 entries).
pub const SPEAKERS: &[(Speaker, SpeakerVoice)] = &[
    (Speaker::Hero,     SpeakerVoice { spatial: false, gain: 1.0,  name: None,              pitch: (1.0,  1.0)  }),
    (Speaker::Villager, SpeakerVoice { spatial: true,  gain: 1.4,  name: Some("Townsfolk"), pitch: (1.0,  1.0)  }),
    (Speaker::Ork,      SpeakerVoice { spatial: true,  gain: 0.85, name: None,              pitch: (0.82, 1.18) }),
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
    VillagerArmedJab,
    SiegeFalls,
    Dawn,
    Rescued,
    /// The hero swung at (and harmlessly bonked) a townsperson — they answer with a barbed remark.
    HitByHero,
    // ── Ork ──
    OrkSpot,
    OrkDeath,
    // ── Chain reply concepts ──
    ReplyToVillagerJab,
    /// Second-level chain: after the hero's comeback, the villager gets the last word.
    VillagerLastWord,
}

/// A follow-up dispatched when a line finishes: ask `target` to look up a line whose
/// `reply_to == Some(concept)`. If none matches the (now-current) facts, nothing plays — the
/// chain self-terminates (the Valve "no explicit interruption" property).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chain {
    pub concept: Concept,
    pub target: Speaker,
    /// `true` = don't auto-play: offer the reply to the PLAYER as an `E — Talk back` prompt near
    /// the speaker for a few seconds (see `interaction.rs`); unanswered, it expires silently.
    pub manual: bool,
}

/// A villager's at-the-hero jab → the hero MAY fire back: offered to the player (`manual`), not
/// auto-played. Used as the `then` on the `pa_*` jab lines; resolved against the hero's
/// `ReplyToVillagerJab` reply pool when the player takes it.
const REPLY_TO_JAB: Chain =
    Chain { concept: Concept::ReplyToVillagerJab, target: Speaker::Hero, manual: true };

/// Second link: some hero comebacks hand the exchange BACK to the villager for a parting shot
/// (jab → comeback → last word, then the chain ends — no `then` on the last-word pool). The NPC
/// answers on his own, so this one stays automatic.
const LAST_WORD: Chain =
    Chain { concept: Concept::VillagerLastWord, target: Speaker::Villager, manual: false };

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
    // Heads-up: these run ~14–17s but read_secs clamps the mouth-busy estimate at 8s, so a
    // higher-priority hero line arriving after ~8s can cut an intro short (rare: priority 3, once,
    // at run start).
    Line { once: true, priority: 3, ..line("intro_a", Speaker::Hero, Concept::Intro, "Daylight's short — open the chests, gather coin and stone, buy what'll keep you breathing. When dark comes, the orks come for the keep. We hold it.") },
    Line { once: true, priority: 3, ..line("intro_b", Speaker::Hero, Concept::Intro, "By day you scavenge — chests, ore, gold — and arm up at the War Table. By night the horde hits these walls. Keep the keep standing. Don't waste the light.") },
    // ── Villager ambient chatter (nearest working townsperson, when the hero lingers) ──
    // One villager voice globally at a time (director enforces this); accepted simplification vs. the
    // old one-per-cluster model. floor:360 keeps the ~7-min rotation cycling without going silent.
    // Speakers: Ed (greet … screaming) and Professor (day_dream … screaming). Text is the clip transcript.
    Line { floor: 360.0, ..line("greet",        Speaker::Villager, Concept::Greeting,         "Oh, hello there, m'lord. Mind the mud.") },
    Line { floor: 360.0, ..line("greet_2",      Speaker::Villager, Concept::Greeting,         "Bless your night. We sleep easier when you're about.") },
    Line { floor: 360.0, ..line("idle_hens",    Speaker::Villager, Concept::Greeting,         "I told the hens about the orks. They were not impressed.") },
    Line { floor: 360.0, ..line("idle_cousin",  Speaker::Villager, Concept::Greeting,         "Me cousin says he killed an ork once. Me cousin says a lotta things.") },
    Line { floor: 360.0, ..line("merchant",     Speaker::Villager, Concept::Greeting,         "Finest wares this side of the swamp. Only wares this side of the swamp, but still.") },
    // These jabs are aimed AT the hero, so each opens a call-and-response chain: when the
    // line finishes it dispatches `ReplyToVillagerJab` to the hero, who picks a comeback from his
    // reply pool below (the Valve "then" dispatch — `tick_chains` resolves it against current facts,
    // so if the hero has wandered out of earshot / is mid-line, the comeback simply doesn't land).
    Line { floor: 360.0, then: Some(REPLY_TO_JAB), ..line("pa_hero",   Speaker::Villager, Concept::Greeting, "Off to be a hero again, are we? Must be nice having the time.") },
    Line { floor: 360.0, then: Some(REPLY_TO_JAB), ..line("pa_chosen", Speaker::Villager, Concept::Greeting, "Oh, the chosen one graces us. Mind you don't trip on all that destiny.") },
    Line { floor: 360.0, then: Some(REPLY_TO_JAB), ..line("pa_slept",  Speaker::Villager, Concept::Greeting, "Saved us all last night, did you? Funny, I slept fine without you.") },
    // pa_armor and everything after it in this file were batch-generated with ElevenLabs
    // ("Victor — deep" voice, single take split on silences), so they share one voice.
    Line { floor: 360.0, then: Some(REPLY_TO_JAB), ..line("pa_armor",  Speaker::Villager, Concept::Greeting, "Lovely armor, that. Shame about the taxes what paid for it.") },
    Line { floor: 360.0, then: Some(REPLY_TO_JAB), ..line("pa_late",   Speaker::Villager, Concept::Greeting, "Oh, look who turns up once the screaming's done. Impeccable timing, as ever.") },
    Line { floor: 360.0, ..line("pa_fence",     Speaker::Villager, Concept::Greeting,         "Big strong knight. Can't fix a fence, but big strong knight.") },
    Line { floor: 360.0, ..line("story_barn",   Speaker::Villager, Concept::Greeting,         "See ol' Marek's barn? Burned clean down. He says lightning. Was the ale.") },
    Line { floor: 360.0, ..line("story_miller", Speaker::Villager, Concept::Greeting,         "The miller's daughter married a soldier. He left, she kept the goat. Smart girl.") },
    Line { floor: 360.0, ..line("story_witch",  Speaker::Villager, Concept::Greeting,         "They say the swamp witch grants wishes. They also say she eats fingers. I'll keep my fingers.") },
    Line { floor: 360.0, ..line("story_gran",   Speaker::Villager, Concept::Greeting,         "We don't talk about grandfather.") },
    Line { floor: 360.0, ..line("story_baker",  Speaker::Villager, Concept::Greeting,         "Heard the baker's getting rich. Heard it from the baker. So.") },
    Line { floor: 360.0, ..line("day_dream",    Speaker::Villager, Concept::Greeting,         "Another glorious day of standing exactly here. Living the dream.") },
    Line { floor: 360.0, ..line("chicken",      Speaker::Villager, Concept::Greeting,         "If one more chicken gets into the chapel, I'm converting.") },
    Line { floor: 360.0, ..line("taxes",        Speaker::Villager, Concept::Greeting,         "Taxes up, walls down, orks at the door. But sure, ring the bell. That'll help.") },
    Line { floor: 360.0, ..line("trade",        Speaker::Villager, Concept::Greeting,         "I had a trade once, then the war. Now I... do this.") },
    Line { floor: 360.0, ..line("grateful",     Speaker::Villager, Concept::Greeting,         "We're ever so grateful. Truly. Now could you grateful your boots off my step.") },
    Line { floor: 360.0, ..line("screaming",    Speaker::Villager, Concept::Greeting,         "Bless you for the protection. The screaming at night is a lovely touch.") },
    Line { floor: 360.0, ..line("statue",       Speaker::Villager, Concept::Greeting,         "They'll raise you a statue one day, m'lord. The pigeons are very excited.") },
    Line { floor: 360.0, ..line("favorite",     Speaker::Villager, Concept::Greeting,         "You're my favorite knight. You're the only knight. Still counts.") },
    Line { floor: 360.0, ..line("moat",         Speaker::Villager, Concept::Greeting,         "I said dig a moat. 'Too dear,' they said. But swords for everyone, sure.") },
    Line { floor: 360.0, ..line("optimist",     Speaker::Villager, Concept::Greeting,         "Mum always said look on the bright side. So: at least the orks are punctual.") },
    Line { floor: 360.0, ..line("roof",         Speaker::Villager, Concept::Greeting,         "There's a hole in my roof shaped just like a catapult stone. Decorative, I'm told.") },
    // pa_sword is weapon-gated → its own concept so the trigger can conditionally emit it only when armed
    Line { floor: 360.0, ..line("pa_sword",     Speaker::Villager, Concept::VillagerArmedJab, "Look at the size of that sword. My uncle's is smaller, and he's twice the man.") },
    Line { floor: 360.0, ..line("pa_shiny",     Speaker::Villager, Concept::VillagerArmedJab, "Ooh, shiny. Did the merchant see you coming, or did you queue up special?") },
    // ── Villager event reactions (must finish; outrank ambient chatter) ──
    // interruptible:false + priority:15 so these aren't cut off by ambient. floor:600 = 10-min floor.
    Line { interruptible: false, priority: 15, floor: 600.0, ..line("siege_fear",  Speaker::Villager, Concept::SiegeFalls, "They're coming. Inside, inside. Lock the door.") },
    Line { interruptible: false, priority: 15, floor: 600.0, ..line("dawn_relief", Speaker::Villager, Concept::Dawn,       "Made it to morning. Knew you'd see us through.") },
    Line { interruptible: false, priority: 15, floor: 600.0, ..line("rescued",     Speaker::Villager, Concept::Rescued,    "You came for me? Gods bless you. I'll take up a spear, I swear it.") },
    // Dry-wit variants of the same events — same must-finish gating, so the pool just gets wider.
    Line { interruptible: false, priority: 15, floor: 600.0, ..line("siege_dry",   Speaker::Villager, Concept::SiegeFalls, "Orks again. Right on schedule. Everyone act surprised.") },
    Line { interruptible: false, priority: 15, floor: 600.0, ..line("dawn_dry",    Speaker::Villager, Concept::Dawn,       "Still alive, then. The betting pool will be devastated.") },
    Line { interruptible: false, priority: 15, floor: 600.0, ..line("rescued_dry", Speaker::Villager, Concept::Rescued,    "My hero. Only took you, what, a fortnight? Bless.") },
    // ── "You just HIT me?!" — harmless bonk reactions (the hero can clip a townsperson with a
    // swing; it does no damage but earns a sarcastic earful). These REUSE existing villager clips
    // whose lines best fit getting smacked by your own knight — no new audio, same affronted tone.
    // priority 13 so a bonk barges over idle chatter (prio 10) but yields to event lines (15);
    // floor 5 throttles machine-gun swinging without making a clip feel unresponsive.
    Line { priority: 13, floor: 5.0, ..line("last_word_c", Speaker::Villager, Concept::HitByHero, "Touchy, touchy. And after everything we do for you.") },
    Line { priority: 13, floor: 5.0, ..line("last_word_a", Speaker::Villager, Concept::HitByHero, "Ooh, sharp. Practice that one on the cows, did we?") },
    Line { priority: 13, floor: 5.0, ..line("last_word_b", Speaker::Villager, Concept::HitByHero, "Noted, m'lord. I'll scream quieter tonight, just for you.") },
    Line { priority: 13, floor: 5.0, ..line("grateful",    Speaker::Villager, Concept::HitByHero, "We're ever so grateful. Truly. Now could you grateful your boots off my step.") },
    Line { priority: 13, floor: 5.0, ..line("screaming",   Speaker::Villager, Concept::HitByHero, "Bless you for the protection. The screaming at night is a lovely touch.") },
    // ── Ork battle barks (nearest ork in earshot; pitch-shifted per utterance) ──
    Line { ..line("spot",   Speaker::Ork, Concept::OrkSpot,  "Little knight. Little bones.") },
    Line { ..line("charge", Speaker::Ork, Concept::OrkSpot,  "Smash the stone. Burn the nest.") },
    Line { ..line("blood",  Speaker::Ork, Concept::OrkSpot,  "Blood. Blood.") },
    Line { ..line("taunt",  Speaker::Ork, Concept::OrkSpot,  "Run, runt. We eat slow ones first.") },
    Line { ..line("where",  Speaker::Ork, Concept::OrkSpot,  "Where? Where you hide, worm?") },
    Line { ..line("feast",  Speaker::Ork, Concept::OrkSpot,  "Tonight we feast.") },
    Line { ..line("shaman", Speaker::Ork, Concept::OrkSpot,  "Spirits take him. Saka.") },
    Line { ..line("gate",   Speaker::Ork, Concept::OrkSpot,  "Break the gate. Break the man.") },
    Line { ..line("meat",   Speaker::Ork, Concept::OrkSpot,  "Fresh meat for the war pot.") },
    // ── Ork death snarl (on a kill) ──
    Line { ..line("death",  Speaker::Ork, Concept::OrkDeath, "Not done.") },
    Line { ..line("death_2", Speaker::Ork, Concept::OrkDeath, "Cold... why cold...") },
    Line { ..line("death_3", Speaker::Ork, Concept::OrkDeath, "Good fight. Good... fight.") },
    // ── Hero comebacks to a villager's jab (chain replies — `reply_to`, never emitted directly) ──
    // Dispatched by `tick_chains` when a `pa_*` jab finishes; `pick`-of-pool gives variety. Priority
    // 12 so the retort lands over idle chatter; floored so the same comeback doesn't repeat soon.
    Line { priority: 12, floor: 90.0, reply_to: Some(Concept::ReplyToVillagerJab), ..line("reply_jab_a", Speaker::Hero, Concept::ReplyToVillagerJab, "Mm. And yet here you still stand, breathing. Funny how that works.") },
    Line { priority: 12, floor: 90.0, reply_to: Some(Concept::ReplyToVillagerJab), ..line("reply_jab_b", Speaker::Hero, Concept::ReplyToVillagerJab, "Destiny's heavy. Someone has to carry it. Might as well be the fool with the sword.") },
    // These two comebacks chain a SECOND link: the villager gets the last word (jab → comeback →
    // parting shot, three speaker turns through the same `then` machinery — no special casing).
    Line { priority: 12, floor: 90.0, reply_to: Some(Concept::ReplyToVillagerJab), then: Some(LAST_WORD), ..line("reply_jab_c", Speaker::Hero, Concept::ReplyToVillagerJab, "Keep talking. The orks find the loud ones first.") },
    Line { priority: 12, floor: 90.0, reply_to: Some(Concept::ReplyToVillagerJab), then: Some(LAST_WORD), ..line("reply_jab_d", Speaker::Hero, Concept::ReplyToVillagerJab, "One day I'll sleep in. Just the once. See how the jokes hold up.") },
    Line { priority: 12, floor: 90.0, reply_to: Some(Concept::ReplyToVillagerJab), ..line("reply_jab_e", Speaker::Hero, Concept::ReplyToVillagerJab, "Wit like that, the orks would die laughing. Saves me the swinging.") },
    // ── Villager last words (chain replies to a hero comeback — never emitted directly) ──
    // End of the exchange: no `then` here, so the chain terminates.
    Line { priority: 12, floor: 120.0, reply_to: Some(Concept::VillagerLastWord), ..line("last_word_a", Speaker::Villager, Concept::VillagerLastWord, "Ooh, sharp. Practice that one on the cows, did we?") },
    Line { priority: 12, floor: 120.0, reply_to: Some(Concept::VillagerLastWord), ..line("last_word_b", Speaker::Villager, Concept::VillagerLastWord, "Noted, m'lord. I'll scream quieter tonight, just for you.") },
    Line { priority: 12, floor: 120.0, reply_to: Some(Concept::VillagerLastWord), ..line("last_word_c", Speaker::Villager, Concept::VillagerLastWord, "Touchy, touchy. And after everything we do for you.") },
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
    fn pick_line_none_when_all_candidates_floored() {
        // LevelUp has a single catalog line (floor 300). Mark it as just played → the only
        // candidate is ineligible → `pick_line` returns None (every concept now has ≥1 line, so we
        // prove the empty-pool path via the floor instead of an unpopulated concept).
        let mut last = HashMap::new();
        last.insert("levelup", 50.0);
        let once = HashSet::new();
        let mut rng = 1;
        assert!(pick_line(Concept::LevelUp, &last, &once, 60.0, &mut rng).is_none()); // 10s < 300 floor
        assert!(pick_line(Concept::LevelUp, &last, &once, 400.0, &mut rng).is_some()); // floor cleared
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

    // ── Call-and-response chain (D3) ───────────────────────────────────────────────────────────

    #[test]
    fn jab_lines_dispatch_the_reply_chain() {
        // The three at-the-hero villager jabs each finish with a `then` that targets the hero.
        for id in ["pa_hero", "pa_chosen", "pa_slept"] {
            let l = LINES.iter().find(|l| l.id == id).unwrap();
            let chain = l.then.unwrap_or_else(|| panic!("{id} should chain a reply"));
            assert_eq!(chain.concept, Concept::ReplyToVillagerJab);
            assert_eq!(chain.target, Speaker::Hero);
            assert!(chain.manual, "the comeback is offered to the player, not auto-played");
        }
    }

    #[test]
    fn reply_pool_answers_the_jab() {
        // `replies_to` is what the director's `tick_chains` queries: every match is a hero line
        // tagged as a valid reply, and there's a pool of them (the user's "a set of replies that
        // fit a prompt").
        let pool: Vec<&Line> = replies_to(Concept::ReplyToVillagerJab).collect();
        assert!(pool.len() >= 2, "want a pool of comebacks, got {}", pool.len());
        assert!(pool.iter().all(|l| l.speaker == Speaker::Hero));
        assert!(pool.iter().all(|l| l.reply_to == Some(Concept::ReplyToVillagerJab)));
    }

    #[test]
    fn chain_resolves_end_to_end() {
        // Replay exactly what `tick_chains` does once a `pa_*` jab finishes: take the chain, gather
        // replies for it, filter to the target speaker + per-line gates, pick the highest priority.
        let chain = LINES.iter().find(|l| l.id == "pa_slept").unwrap().then.unwrap();
        let (last, once) = (HashMap::new(), HashSet::new());
        let pick = replies_to(chain.concept)
            .filter(|l| l.speaker == chain.target)
            .filter(|l| passes_gates(l, &last, &once, 0.0))
            .max_by_key(|l| l.priority);
        let reply = pick.expect("a fresh jab should resolve to a comeback");
        assert_eq!(reply.speaker, Speaker::Hero);
        assert!(reply.id.starts_with("reply_jab_"));
    }

    // ── Second-level chain: jab → comeback → villager last word ────────────────────────────────

    #[test]
    fn some_comebacks_hand_the_villager_the_last_word() {
        let chained: Vec<&Line> = replies_to(Concept::ReplyToVillagerJab)
            .filter(|l| l.then.is_some())
            .collect();
        assert!(!chained.is_empty(), "at least one comeback should chain a last word");
        for l in &chained {
            let chain = l.then.unwrap();
            assert_eq!(chain.concept, Concept::VillagerLastWord);
            assert_eq!(chain.target, Speaker::Villager);
            assert!(!chain.manual, "the NPC's parting shot plays on its own");
        }
        // ...but not ALL of them — sometimes the hero ends the exchange.
        assert!(
            replies_to(Concept::ReplyToVillagerJab).any(|l| l.then.is_none()),
            "some comebacks should end the exchange"
        );
    }

    #[test]
    fn last_word_pool_answers_and_terminates() {
        let pool: Vec<&Line> = replies_to(Concept::VillagerLastWord).collect();
        assert!(pool.len() >= 2, "want a pool of last words, got {}", pool.len());
        assert!(pool.iter().all(|l| l.speaker == Speaker::Villager));
        // The exchange must END here: a last word that chained again could ping-pong forever.
        assert!(pool.iter().all(|l| l.then.is_none()));
    }

    #[test]
    fn three_step_chain_resolves_end_to_end() {
        // jab (villager) → comeback (hero) → last word (villager), replaying `tick_chains` twice.
        let (last, once) = (HashMap::new(), HashSet::new());
        let jab = LINES.iter().find(|l| l.id == "pa_armor").unwrap();
        let step1 = jab.then.expect("jab chains a comeback");
        let comeback = replies_to(step1.concept)
            .filter(|l| l.speaker == step1.target && l.then.is_some())
            .max_by_key(|l| l.priority)
            .expect("a chaining comeback exists");
        let step2 = comeback.then.unwrap();
        let last_word = replies_to(step2.concept)
            .filter(|l| l.speaker == step2.target)
            .filter(|l| passes_gates(l, &last, &once, 0.0))
            .max_by_key(|l| l.priority)
            .expect("the villager gets the last word");
        assert!(last_word.id.starts_with("last_word_"));
        assert!(last_word.then.is_none(), "and the exchange ends there");
    }

    #[test]
    fn stale_chain_self_terminates_when_pool_floored() {
        // If every comeback is on its replay floor (the hero just fired one), the chain finds no
        // reply and silently dies — the Valve "no explicit interruption" property.
        let chain = REPLY_TO_JAB;
        let mut last = HashMap::new();
        for l in replies_to(chain.concept) {
            last.insert(l.id, 0.0); // all played at t=0
        }
        let once = HashSet::new();
        let any = replies_to(chain.concept)
            .filter(|l| l.speaker == chain.target)
            .any(|l| passes_gates(l, &last, &once, 10.0)); // 10s later, floor 90 still active
        assert!(!any, "a floored pool should yield no reply → chain self-terminates");
    }
}
