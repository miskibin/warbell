//! The hero's voice — head-locked, ONE MOUTH at a time. Ported from the old game's narration
//! node + `canGrunt` gate: a new line / death cry stops whatever the hero was saying, exertion
//! grunts are rate-limited and never fire while a spoken line is playing.
//!
//! Enforcing "one mouth" without juggling possibly-dead entity ids: every voice sink carries
//! [`HeroVoiceTag`], and starting a new one first despawns all tagged sinks. The query only
//! yields LIVE entities, so a clip that already finished (and self-despawned via
//! `PlaybackMode::Despawn`) is simply absent — no stale-entity despawn.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::biome::Biome;

use super::{frand, pick, AudioConfig, AudioCue};

/// Seconds between any two exertion grunts, so combat doesn't spam the hero's voice.
const GRUNT_MIN_GAP: f32 = 1.6;
/// How long a spoken line (or death cry) blocks grunts after it starts (clips are ~2–4 s; we
/// don't know the exact length, so this is a conservative mouth-busy window).
const LINE_GUARD: f32 = 4.0;

#[derive(Component)]
pub(crate) struct HeroVoiceTag;

#[derive(Resource)]
pub(crate) struct VoiceBank {
    swings: Vec<Handle<AudioSource>>,
    jump: Handle<AudioSource>,
    hurts: Vec<Handle<AudioSource>>,
    deaths: Vec<Handle<AudioSource>>,
    /// (biome, line clip) — small fixed list, looked up by linear scan (`Biome` isn't `Hash`).
    lines: Vec<(Biome, Handle<AudioSource>)>,
}

/// Mouth bookkeeping — last grunt time + when the current line stops blocking grunts.
#[derive(Resource)]
pub(crate) struct HeroMouth {
    last_grunt: f32,
    line_until: f32,
}

impl Default for HeroMouth {
    fn default() -> Self {
        Self { last_grunt: -100.0, line_until: 0.0 }
    }
}

pub(crate) fn setup_voice(asset: Res<AssetServer>, mut commands: Commands) {
    let lines = vec![
        (Biome::Forest, asset.load("audio/vo/forest.ogg")),
        (Biome::Snow, asset.load("audio/vo/snow.ogg")),
        (Biome::Rocky, asset.load("audio/vo/rock.ogg")),
        (Biome::Desert, asset.load("audio/vo/desert.ogg")),
        (Biome::Swamp, asset.load("audio/vo/swamp.ogg")),
    ];
    commands.insert_resource(VoiceBank {
        swings: ["audio/player-swing-1.ogg", "audio/player-swing-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        jump: asset.load("audio/player-jump-1.ogg"),
        hurts: ["audio/player-hurt-1.ogg", "audio/player-hurt-2.ogg", "audio/player-hurt-3.ogg"].iter().map(|f| asset.load(*f)).collect(),
        deaths: ["audio/player-death-1.ogg", "audio/player-death-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        lines,
    });
    commands.init_resource::<HeroMouth>();
}

pub(crate) fn play_voice_cues(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    bank: Res<VoiceBank>,
    mut mouth: ResMut<HeroMouth>,
    mut seed: Local<u32>,
    existing: Query<Entity, With<HeroVoiceTag>>,
    mut cues: MessageReader<AudioCue>,
) {
    let now = time.elapsed_secs();
    // Decide the single sound the mouth plays this frame; later cues override earlier ones.
    let mut pending: Option<(Handle<AudioSource>, f32, bool)> = None; // (clip, vol, is_line)

    for cue in cues.read() {
        match *cue {
            AudioCue::HeroGruntSwing => {
                // 34% of swings grunt (Character.tsx `playPlayerAttack`), then the canGrunt gate.
                if frand(&mut seed) < 0.34 && now >= mouth.line_until && now - mouth.last_grunt >= GRUNT_MIN_GAP {
                    mouth.last_grunt = now;
                    pending = Some((pick(&bank.swings, &mut seed), 0.4 * cfg.voice_vol, false));
                }
            }
            AudioCue::HeroJump => {
                // Only ~40% of jumps grunt (Character.tsx), and only when the mouth is free.
                if frand(&mut seed) < 0.40 && now >= mouth.line_until && now - mouth.last_grunt >= GRUNT_MIN_GAP {
                    mouth.last_grunt = now;
                    pending = Some((bank.jump.clone(), 0.28 * cfg.voice_vol, false));
                }
            }
            AudioCue::HeroHurt => {
                // canGrunt = not mid-line (`line_until`) + 1.6 s since the last grunt.
                if now >= mouth.line_until && now - mouth.last_grunt >= GRUNT_MIN_GAP {
                    mouth.last_grunt = now;
                    pending = Some((pick(&bank.hurts, &mut seed), 0.45 * cfg.voice_vol, false));
                }
            }
            AudioCue::HeroDeath => {
                mouth.line_until = now + LINE_GUARD;
                pending = Some((pick(&bank.deaths, &mut seed), cfg.voice_vol, true));
            }
            AudioCue::HeroLine(b) => {
                if let Some(h) = bank.lines.iter().find(|(bb, _)| *bb == b).map(|(_, h)| h) {
                    mouth.line_until = now + LINE_GUARD;
                    pending = Some((h.clone(), cfg.narration_vol, true));
                }
            }
            _ => {}
        }
    }

    if let Some((clip, vol, _is_line)) = pending {
        // Stop whatever the hero was saying (only live sinks are yielded — no stale ids).
        for e in &existing {
            commands.entity(e).despawn();
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
        ));
    }
}
