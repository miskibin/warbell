//! The voice director — the ONE Bevy system family that turns a [`Speak`] request into a playing
//! clip + subtitle, enforcing one line at a time per speaker with priority-gated barge-in, and
//! firing reply chains when a line ends. Replaces the bespoke mouth/cooldown bookkeeping that
//! used to live in `voice.rs`/`npc.rs`/`ork.rs`/`hero_remarks.rs`.

use std::collections::{HashMap, HashSet};

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::lines::{
    can_play, passes_gates, pick_line, replies_to, speaker_voice, Active, Chain, Concept, Line,
    Speaker,
};
use super::AudioConfig;

/// A request to speak. Triggers (`detect_*` systems) write these; the director decides if/what
/// actually plays. `at` positions a spatial speaker (villager/ork); ignored for the head-locked
/// hero. Per-line replay throttling is now DATA on each [`Line`] (`floor` / `once` fields).
#[derive(Message, Clone, Copy)]
pub struct Speak {
    pub concept: Concept,
    pub at: Option<Vec3>,
}

impl Speak {
    pub fn new(concept: Concept) -> Self {
        Self { concept, at: None }
    }
    pub fn at(concept: Concept, pos: Vec3) -> Self {
        Self { concept, at: Some(pos) }
    }
}

/// Marks a playing voice sink so a barge-in can stop it. Carries the speaker so we only stop the
/// right mouth.
#[derive(Component)]
pub struct VoiceSink(pub Speaker);

/// How long a `manual` chain reply stays on offer after the prompt line ends (seconds).
pub const REPLY_WINDOW: f32 = 6.0;

/// A `manual` chain whose prompt line just finished: instead of auto-playing, it's offered to the
/// player as an `E — Talk back` interaction near the speaker (`interaction.rs` shows the prompt
/// and calls [`VoiceManager::accept_reply`]). Unanswered, it expires silently — same
/// self-terminating property as a stale chain.
#[derive(Clone, Copy)]
pub struct Offer {
    pub chain: Chain,
    /// Where the prompting speaker stood — the E-prompt is range-gated on this.
    pub pos: Option<Vec3>,
    pub expires_at: f32,
}

#[derive(Resource, Default)]
pub struct OfferedReply(pub Option<Offer>);

/// One-line-at-a-time bookkeeping for every speaker, the per-line replay floor, and the rng.
#[derive(Resource)]
pub struct VoiceManager {
    pub active: HashMap<Speaker, Active>,
    pub last_played: HashMap<&'static str, f32>,
    pub played_once: HashSet<&'static str>,
    pub rng: u32,
    /// Pending chain dispatches: (fire_at, chain, position) queued when a line with `then` starts.
    pub pending_chains: Vec<(f32, Chain, Option<Vec3>)>,
    /// Preloaded clip handles keyed by line id. Populated once at startup; persists across runs.
    pub clips: HashMap<&'static str, Handle<AudioSource>>,
}

impl Default for VoiceManager {
    fn default() -> Self {
        Self {
            active: HashMap::new(),
            last_played: HashMap::new(),
            played_once: HashSet::new(),
            rng: 0x1234_5678,
            pending_chains: Vec::new(),
            clips: HashMap::new(),
        }
    }
}

impl VoiceManager {
    /// Is any NON-hero speaker mid-line right now? (Replaces the old `OthersSpeaking` resource —
    /// the hero's observational lines defer while a villager/ork is talking.)
    pub fn others_speaking(&self, now: f32) -> bool {
        self.active.iter().any(|(s, a)| *s != Speaker::Hero && now < a.ends_at)
    }
    /// Is the hero mid-line? (Replaces `HeroSpeaking` — villagers/orks defer to him.)
    pub fn hero_speaking(&self, now: f32) -> bool {
        self.active.get(&Speaker::Hero).is_some_and(|a| now < a.ends_at)
    }
    /// Clear all state for a fresh run (mirrors the old `reset_hero_line_gates`).
    pub fn reset(&mut self) {
        self.active.clear();
        self.last_played.clear();
        self.played_once.clear();
        self.pending_chains.clear();
    }
    /// The player took an offered reply (pressed E): queue its chain for immediate dispatch.
    /// Re-queued as non-`manual` so `tick_chains` plays it instead of re-offering it.
    pub fn accept_reply(&mut self, offer: Offer, now: f32) {
        let mut chain = offer.chain;
        chain.manual = false;
        self.pending_chains.push((now, chain, offer.pos));
    }
}

/// Resolve `Speak` requests into clips. For each request: pick a fresh line for the concept, check
/// the speaker's barge-in gate, and if clear, stop any current sink for that speaker and play.
pub fn speak_director(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    mut mgr: ResMut<VoiceManager>,
    mut cd: ResMut<super::HeroLineCooldown>,
    mut reqs: MessageReader<Speak>,
    sinks: Query<(Entity, &VoiceSink)>,
    hero: Query<&crate::player::Hero>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    sources: Res<Assets<AudioSource>>,
) {
    let now = time.elapsed_secs();
    let hero_pos = hero.single().ok().map(|h| Vec3::new(h.pos.x, 1.6, h.pos.y));

    for req in reqs.read() {
        // Pull rng out across the immutable `pick_line` borrow of `mgr.last_played`.
        let mut rng = mgr.rng;
        let chosen = pick_line(req.concept, &mgr.last_played, &mgr.played_once, now, &mut rng).copied();
        mgr.rng = rng;
        let Some(line) = chosen else { continue };

        // Shared hero-line spacing (~20 s): drop a hero line still inside the window unless it
        // strictly out-ranks the line that opened it (urgent warnings cut through idle chatter).
        // Chain replies skip this — they go straight through `tick_chains`/`play_line`.
        if line.speaker == Speaker::Hero && now < cd.until && line.priority <= cd.priority {
            continue;
        }
        if !can_play(mgr.active.get(&line.speaker), now, line.priority) {
            continue;
        }
        play_line(&mut commands, &cfg, &mut mgr, &mut cd, &sinks, &mut subs, &sources, now, &line, req.at.or(hero_pos));
    }
}

/// The shared "actually play it" path (used by the director AND chain replies).
#[allow(clippy::too_many_arguments)]
fn play_line(
    commands: &mut Commands,
    cfg: &AudioConfig,
    mgr: &mut VoiceManager,
    cd: &mut super::HeroLineCooldown,
    sinks: &Query<(Entity, &VoiceSink)>,
    subs: &mut crate::subtitles::Subtitles,
    sources: &Assets<AudioSource>,
    now: f32,
    line: &Line,
    pos: Option<Vec3>,
) {
    let voice = speaker_voice(line.speaker);
    // Look up the preloaded handle; bail if the line has no clip registered.
    let Some(clip) = mgr.clips.get(line.id).cloned() else { return };
    // Not loaded yet OR the .ogg doesn't exist → no-op: don't block the mouth, don't caption.
    if sources.get(&clip).is_none() {
        return;
    }
    // Roll a per-utterance playback speed (pitch shift) from the speaker's configured range.
    // For Hero and Villager the range is (1.0, 1.0) → speed = 1.0 (no shift). For Ork the range
    // is (0.82, 1.18) → the "different ork every time" effect that ork.rs used to apply directly.
    let speed = if voice.pitch.0 < voice.pitch.1 {
        voice.pitch.0 + super::frand(&mut mgr.rng) * (voice.pitch.1 - voice.pitch.0)
    } else {
        1.0
    };
    // One mouth per speaker: stop whatever this speaker had going.
    for (e, s) in sinks {
        if s.0 == line.speaker {
            commands.entity(e).try_despawn();
        }
    }
    let dur = crate::subtitles::read_secs(line.text);
    let vol = voice.gain * cfg.voice_vol;
    let mut ent = commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            speed,
            spatial: voice.spatial,
            ..default()
        },
        VoiceSink(line.speaker),
    ));
    if voice.spatial {
        ent.insert(Transform::from_translation(pos.unwrap_or(Vec3::ZERO)));
    }
    // Place-bound hero lines (proximity remarks / biome musings) carry a walk-away anchor so they
    // fade the moment he leaves what prompted them (see `stop_displaced_hero_lines`). Event
    // reactions and the night/quiet/kill musings aren't tied to a spot → no anchor, they play out.
    if line.speaker == Speaker::Hero {
        if let Some(anchor) = anchor_for(line.concept, pos) {
            ent.insert(anchor);
        }
    }
    mgr.active.insert(
        line.speaker,
        Active {
            id: line.id,
            ends_at: now + dur,
            priority: line.priority,
            interruptible: line.interruptible,
            then: line.then,
        },
    );
    mgr.last_played.insert(line.id, now);
    if line.once {
        mgr.played_once.insert(line.id);
    }
    if let Some(chain) = line.then {
        mgr.pending_chains.push((now + dur, chain, pos));
    }
    // Open the shared hero-line spacing window (~20 s). Only the hero is gapped this way; villagers
    // and orks are paced by their own trigger cadences. A no-op (missing clip) returned above, so a
    // silently-skipped line never spends the window.
    if line.speaker == Speaker::Hero {
        cd.until = now + super::HERO_LINE_CD;
        cd.priority = line.priority;
    }
    subs.say_as(now, voice.name, line.text, dur);
}

/// The walk-away anchor for a hero line, derived from its concept (ported from the old
/// `hero_remarks` anchor table): proximity remarks bind to where he stood — cut if he wanders more
/// than ~10 units off (the old `NEAR + 3`); a biome musing binds to the biome — cut if he steps
/// out of it. Event reactions and the night/quiet/kill musings aren't place-bound → `None`.
fn anchor_for(concept: Concept, pos: Option<Vec3>) -> Option<super::HeroLineAnchor> {
    use super::HeroLineAnchor;
    match concept {
        Concept::NearTown | Concept::NearKids | Concept::NearPet | Concept::NearGuard
        | Concept::InKeep => pos.map(|p| HeroLineAnchor::Near { pos: Vec2::new(p.x, p.z), r: 10.0 }),
        Concept::BiomeEntered(b) => Some(HeroLineAnchor::Biome(b)),
        _ => None,
    }
}

/// When a line with a `then` chain finishes, dispatch the follow-up concept to its target speaker.
/// The reply is resolved against CURRENT facts here (not when the prompt started), so a stale
/// chain finds no matching reply and silently dies — the Valve "no explicit interrupt" property.
pub fn tick_chains(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    mut mgr: ResMut<VoiceManager>,
    mut cd: ResMut<super::HeroLineCooldown>,
    mut offered: ResMut<OfferedReply>,
    sinks: Query<(Entity, &VoiceSink)>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    sources: Res<Assets<AudioSource>>,
) {
    let now = time.elapsed_secs();
    // An unanswered offer expires silently.
    if offered.0.is_some_and(|o| now >= o.expires_at) {
        offered.0 = None;
    }
    // Take the chains whose time has come.
    let mut due: Vec<(Chain, Option<Vec3>)> = Vec::new();
    mgr.pending_chains.retain(|(at, chain, pos)| {
        if now >= *at {
            due.push((*chain, *pos));
            false
        } else {
            true
        }
    });
    for (chain, pos) in due {
        // A manual chain isn't played — it's put on offer for the player's E-prompt (latest wins).
        if chain.manual {
            offered.0 = Some(Offer { chain, pos, expires_at: now + REPLY_WINDOW });
            continue;
        }
        // Pick the highest-priority reply that passes its per-line gates (once + floor).
        let pick = replies_to(chain.concept)
            .filter(|l| l.speaker == chain.target)
            .filter(|l| passes_gates(l, &mgr.last_played, &mgr.played_once, now))
            .max_by_key(|l| l.priority)
            .copied();
        let Some(reply) = pick else { continue };
        if can_play(mgr.active.get(&reply.speaker), now, reply.priority) {
            play_line(&mut commands, &cfg, &mut mgr, &mut cd, &sinks, &mut subs, &sources, now, &reply, pos);
        }
    }
}

/// Preload every catalog line's clip at startup so handles are warm by the time a line fires
/// (one-shot events won't get rejected by the not-yet-loaded guard) and a missing file is
/// detectable (its handle's asset stays absent forever).
pub fn preload_voice_lines(asset: Res<AssetServer>, mut mgr: ResMut<VoiceManager>) {
    for line in super::lines::LINES {
        let dir = match line.speaker {
            Speaker::Hero => "hero",
            Speaker::Villager => "npc",
            Speaker::Ork => "ork",
        };
        let handle = asset.load(format!("audio/vo/{dir}/{}.ogg", line.id));
        mgr.clips.insert(line.id, handle);
    }
}

/// Fresh run: wipe all voice state (active lines, replay floors, once-per-run set, chains).
pub fn reset_voices(mut mgr: ResMut<VoiceManager>, mut offered: ResMut<OfferedReply>) {
    mgr.reset();
    offered.0 = None;
}

/// Reseed the line-pick RNG from wall-clock entropy so catalog line order differs each run
/// (a fixed seed replays the same picks every session — the same complaint `npc.rs` solved).
pub fn setup_voice_manager(mut mgr: ResMut<VoiceManager>) {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0x1234_5678)
        | 1;
    mgr.rng = seed;
}
