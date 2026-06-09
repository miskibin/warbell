//! The hero's voice — head-locked, ONE MOUTH at a time. Ported from the old game's narration
//! node + `canGrunt` gate: a new line / death cry stops whatever the hero was saying, exertion
//! grunts are rate-limited and never fire while a spoken line is playing.
//!
//! Enforcing "one mouth" without juggling possibly-dead entity ids: every voice sink carries
//! [`HeroVoiceTag`], and starting a new one first despawns all tagged sinks. The query only
//! yields LIVE entities, so a clip that already finished (and self-despawned via
//! `PlaybackMode::Despawn`) is simply absent — no stale-entity despawn.
//!
//! Catalog hero lines (event reactions + biome musings) now play via the director as
//! `VoiceSink(Speaker::Hero)`. This module handles ONLY the reflexes: swing grunts, jump grunts,
//! hurt grunts, and the death cry. The two mouths are coordinated:
//! - Grunts defer while a catalog hero line is sounding (`VoiceManager::hero_speaking`).
//! - The death cry despawns any live catalog hero sink so the scream cuts a musing.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::{
    frand, pick, AudioConfig, AudioCue, HeroLineCooldown, HeroMouthTag, HeroSpeaking, HERO_LINE_CD,
};

/// Seconds between any two exertion grunts, so combat doesn't spam the hero's voice.
const GRUNT_MIN_GAP: f32 = 1.6;
/// How long a death cry blocks grunts after it starts.
const LINE_GUARD: f32 = 4.0;

#[derive(Component)]
pub(crate) struct HeroVoiceTag;

#[derive(Resource)]
pub(crate) struct VoiceBank {
    swings: Vec<Handle<AudioSource>>,
    jump: Handle<AudioSource>,
    hurts: Vec<Handle<AudioSource>>,
    deaths: Vec<Handle<AudioSource>>,
}

/// Mouth bookkeeping — last grunt time and when the death cry stops blocking grunts.
#[derive(Resource)]
pub(crate) struct HeroMouth {
    pub(crate) last_grunt: f32,
    /// Set by the death cry to block grunts for LINE_GUARD seconds.
    pub(crate) line_until: f32,
}

impl Default for HeroMouth {
    fn default() -> Self {
        Self {
            last_grunt: -100.0,
            line_until: 0.0,
        }
    }
}

pub(crate) fn setup_voice(asset: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(VoiceBank {
        swings: ["audio/player-swing-1.ogg", "audio/player-swing-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        jump: asset.load("audio/player-jump-1.ogg"),
        hurts: ["audio/player-hurt-1.ogg", "audio/player-hurt-2.ogg", "audio/player-hurt-3.ogg"].iter().map(|f| asset.load(*f)).collect(),
        deaths: ["audio/player-death-1.ogg", "audio/player-death-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
    });
    commands.init_resource::<HeroMouth>();
}

pub(crate) fn play_voice_cues(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    bank: Res<VoiceBank>,
    mut mouth: ResMut<HeroMouth>,
    mut cd: ResMut<HeroLineCooldown>,
    mut speaking: ResMut<HeroSpeaking>,
    mgr: Res<super::director::VoiceManager>,
    mut seed: Local<u32>,
    existing: Query<Entity, With<HeroMouthTag>>,
    hero_sinks: Query<(Entity, &super::director::VoiceSink)>,
    mut cues: MessageReader<AudioCue>,
) {
    let now = time.elapsed_secs();
    // Decide the single sound the mouth plays this frame; later cues override earlier ones.
    // (clip, vol, is_line) — is_line marks the death cry (vs short grunts).
    let mut pending: Option<(Handle<AudioSource>, f32, bool)> = None;

    for cue in cues.read() {
        match *cue {
            AudioCue::HeroGruntSwing => {
                // 34% of swings grunt (Character.tsx `playPlayerAttack`), then the canGrunt gate.
                // Suppressed while a catalog hero line is sounding (no grunt mid-sentence).
                if frand(&mut seed) < 0.34
                    && now >= mouth.line_until
                    && now - mouth.last_grunt >= GRUNT_MIN_GAP
                    && !mgr.hero_speaking(now)
                {
                    mouth.last_grunt = now;
                    pending = Some((pick(&bank.swings, &mut seed), 0.4 * cfg.voice_vol, false));
                }
            }
            AudioCue::HeroJump => {
                // Only ~40% of jumps grunt (Character.tsx), and only when the mouth is free.
                // Suppressed while a catalog hero line is sounding.
                if frand(&mut seed) < 0.40
                    && now >= mouth.line_until
                    && now - mouth.last_grunt >= GRUNT_MIN_GAP
                    && !mgr.hero_speaking(now)
                {
                    mouth.last_grunt = now;
                    pending = Some((bank.jump.clone(), 0.28 * cfg.voice_vol, false));
                }
            }
            AudioCue::HeroHurt => {
                // canGrunt = not mid-line (`line_until`) + 1.6 s since the last grunt.
                // Suppressed while a catalog hero line is sounding.
                if now >= mouth.line_until
                    && now - mouth.last_grunt >= GRUNT_MIN_GAP
                    && !mgr.hero_speaking(now)
                {
                    mouth.last_grunt = now;
                    pending = Some((pick(&bank.hurts, &mut seed), 0.45 * cfg.voice_vol, false));
                }
            }
            AudioCue::HeroDeath => {
                // A death cry always plays (interrupts) and spends the shared cooldown.
                // Also despawn any live catalog hero sink so the scream cuts a musing.
                mouth.line_until = now + LINE_GUARD;
                cd.until = now + HERO_LINE_CD;
                cd.priority = u8::MAX; // nothing out-ranks a death cry — no chatter trails it
                for (e, s) in &hero_sinks {
                    if s.0 == super::Speaker::Hero {
                        commands.entity(e).try_despawn();
                    }
                }
                pending = Some((pick(&bank.deaths, &mut seed), cfg.voice_vol, true));
            }
            _ => {}
        }
    }

    if let Some((clip, vol, is_line)) = pending {
        // One mouth: stop whatever he was saying — a prior line OR a remark (both `HeroMouthTag`).
        // Only live sinks are yielded, so no stale ids; `try_despawn` is safe if one already ended.
        for e in &existing {
            commands.entity(e).try_despawn();
        }
        // A LINE (death cry) occupies the mouth for ~its length, so villagers/orks defer; grunts don't.
        if is_line {
            speaking.until = now + LINE_GUARD;
        }
        commands.spawn((
            AudioPlayer(clip),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(vol),
                spatial: false,
                ..default()
            },
            HeroVoiceTag,
            HeroMouthTag,
        ));
    }
}
