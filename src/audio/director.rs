//! The voice director — the ONE Bevy system family that turns a [`Speak`] request into a playing
//! clip + subtitle, enforcing one line at a time per speaker with priority-gated barge-in, and
//! firing reply chains when a line ends. Replaces the bespoke mouth/cooldown bookkeeping that
//! used to live in `voice.rs`/`npc.rs`/`ork.rs`/`hero_remarks.rs`.

use std::collections::HashMap;

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::lines::{
    can_play, pick_line, replies_to, speaker_voice, Active, Chain, Concept, Line, Speaker,
};
use super::AudioConfig;

/// A request to speak. Triggers (`detect_*` systems) write these; the director decides if/what
/// actually plays. `at` positions a spatial speaker (villager/ork); ignored for the head-locked
/// hero. `floor` is the per-line replay floor for this request's concept.
#[derive(Message, Clone, Copy)]
pub struct Speak {
    pub concept: Concept,
    pub at: Option<Vec3>,
    pub floor: f32,
}

impl Speak {
    pub fn new(concept: Concept) -> Self {
        Self { concept, at: None, floor: 0.0 }
    }
    pub fn at(concept: Concept, pos: Vec3) -> Self {
        Self { concept, at: Some(pos), floor: 0.0 }
    }
    pub fn floored(mut self, floor: f32) -> Self {
        self.floor = floor;
        self
    }
}

/// Marks a playing voice sink so a barge-in can stop it. Carries the speaker so we only stop the
/// right mouth.
#[derive(Component)]
pub struct VoiceSink(pub Speaker);

/// One-line-at-a-time bookkeeping for every speaker, the per-line replay floor, and the rng.
#[derive(Resource)]
pub struct VoiceManager {
    pub active: HashMap<Speaker, Active>,
    pub last_played: HashMap<&'static str, f32>,
    pub rng: u32,
    /// Pending chain dispatches: (fire_at, chain, position) queued when a line with `then` starts.
    pub pending_chains: Vec<(f32, Chain, Option<Vec3>)>,
}

impl Default for VoiceManager {
    fn default() -> Self {
        Self {
            active: HashMap::new(),
            last_played: HashMap::new(),
            rng: 0x1234_5678,
            pending_chains: Vec::new(),
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
}

/// Estimate a clip's spoken length from its transcript (same model as the subtitle reader, so the
/// mouth-busy window matches the caption).
fn line_secs(text: &str) -> f32 {
    crate::subtitles::read_secs(text)
}

/// Resolve `Speak` requests into clips. For each request: pick a fresh line for the concept, check
/// the speaker's barge-in gate, and if clear, stop any current sink for that speaker and play.
pub fn speak_director(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    asset: Res<AssetServer>,
    mut commands: Commands,
    mut mgr: ResMut<VoiceManager>,
    mut reqs: MessageReader<Speak>,
    sinks: Query<(Entity, &VoiceSink)>,
    hero: Query<&crate::player::Hero>,
) {
    let now = time.elapsed_secs();
    let hero_pos = hero.single().ok().map(|h| Vec3::new(h.pos.x, 1.6, h.pos.y));

    for req in reqs.read() {
        // Pull rng out across the immutable `pick_line` borrow of `mgr.last_played`.
        let mut rng = mgr.rng;
        let chosen = pick_line(req.concept, &mgr.last_played, now, req.floor, &mut rng).copied();
        mgr.rng = rng;
        let Some(line) = chosen else { continue };

        if !can_play(mgr.active.get(&line.speaker), now, line.priority) {
            continue;
        }
        play_line(&mut commands, &asset, &cfg, &mut mgr, &sinks, now, &line, req.at.or(hero_pos));
    }
}

/// The shared "actually play it" path (used by the director AND chain replies).
#[allow(clippy::too_many_arguments)]
fn play_line(
    commands: &mut Commands,
    asset: &AssetServer,
    cfg: &AudioConfig,
    mgr: &mut VoiceManager,
    sinks: &Query<(Entity, &VoiceSink)>,
    now: f32,
    line: &Line,
    pos: Option<Vec3>,
) {
    let voice = speaker_voice(line.speaker);
    // One mouth per speaker: stop whatever this speaker had going.
    for (e, s) in sinks {
        if s.0 == line.speaker {
            commands.entity(e).try_despawn();
        }
    }
    let dir = match line.speaker {
        Speaker::Hero => "hero",
        Speaker::Villager => "npc",
        Speaker::Ork => "ork",
    };
    let clip: Handle<AudioSource> = asset.load(format!("audio/vo/{dir}/{}.ogg", line.id));
    let dur = line_secs(line.text);
    let vol = voice.gain * cfg.voice_vol;

    let mut ent = commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            spatial: voice.spatial,
            ..default()
        },
        VoiceSink(line.speaker),
    ));
    if voice.spatial {
        ent.insert(Transform::from_translation(pos.unwrap_or(Vec3::ZERO)));
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
    if let Some(chain) = line.then {
        mgr.pending_chains.push((now + dur, chain, pos));
    }

    // Subtitle with speaker attribution is wired in Task D1 (needs Subtitles + a loaded-clip guard).
    // subs.say_as(now, voice.name, line.text, dur);
}

/// When a line with a `then` chain finishes, dispatch the follow-up concept to its target speaker.
/// The reply is resolved against CURRENT facts here (not when the prompt started), so a stale
/// chain finds no matching reply and silently dies — the Valve "no explicit interrupt" property.
pub fn tick_chains(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    asset: Res<AssetServer>,
    mut commands: Commands,
    mut mgr: ResMut<VoiceManager>,
    sinks: Query<(Entity, &VoiceSink)>,
) {
    let now = time.elapsed_secs();
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
        // Pick the highest-priority reply that's off its floor (replies use a short 30s floor).
        let pick = replies_to(chain.concept)
            .filter(|l| now - *mgr.last_played.get(l.id).unwrap_or(&f32::NEG_INFINITY) >= 30.0)
            .max_by_key(|l| l.priority)
            .copied();
        let Some(reply) = pick else { continue };
        if can_play(mgr.active.get(&reply.speaker), now, reply.priority) {
            play_line(&mut commands, &asset, &cfg, &mut mgr, &sinks, now, &reply, pos);
        }
    }
}
