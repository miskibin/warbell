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

use super::{frand, pick, AudioConfig, AudioCue, HeroEvent, HeroLineGates};

/// Seconds between any two exertion grunts, so combat doesn't spam the hero's voice.
const GRUNT_MIN_GAP: f32 = 1.6;
/// How long a spoken line (or death cry) blocks grunts after it starts (clips are ~2–4 s; we
/// don't know the exact length, so this is a conservative mouth-busy window).
const LINE_GUARD: f32 = 4.0;
/// Minimum gap between ANY two spoken hero lines (biome musings + event hints). The old game's
/// `voiceStore.GLOBAL_GAP` — so the man can't rattle two thoughts back to back. Because the
/// emitter re-sends the cue every frame the hero is inside a biome, a musing the gap suppresses
/// isn't lost: it fires the moment the gap clears while he's still there (mark-spoken-on-fire).
const GLOBAL_LINE_GAP: f32 = 14.0;
/// Per-line floor for the "throttled" flavour reactions (see [`HeroEvent::throttled`]) — a given
/// line plays at most once per this window, so they stay an occasional spice rather than chatter.
const EVENT_REPLAY_GAP: f32 = 600.0;

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
    /// (event, line clip) — the hero's one-off spoken reactions (stone/chest/rescue/night/hurt/home).
    events: Vec<(HeroEvent, Handle<AudioSource>)>,
}

/// Mouth bookkeeping — last grunt time, when the current line stops blocking grunts, and the
/// start time of the last spoken LINE (drives the [`GLOBAL_LINE_GAP`] between musings/hints).
#[derive(Resource)]
pub(crate) struct HeroMouth {
    last_grunt: f32,
    line_until: f32,
    last_line: f32,
    /// Last time each [`HeroEvent`] played (indexed by `HeroEvent::key`) — drives the per-line
    /// [`EVENT_REPLAY_GAP`] floor on throttled reactions.
    event_last: [f32; HeroEvent::COUNT],
}

impl Default for HeroMouth {
    fn default() -> Self {
        Self {
            last_grunt: -100.0,
            line_until: 0.0,
            last_line: -100.0,
            event_last: [-1000.0; HeroEvent::COUNT],
        }
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
    let events = vec![
        (HeroEvent::FirstStone, asset.load("audio/vo/stone.ogg")),
        (HeroEvent::ChestOpen, asset.load("audio/vo/chest.ogg")),
        (HeroEvent::FirstRescue, asset.load("audio/vo/rescue.ogg")),
        (HeroEvent::NightWarning, asset.load("audio/vo/night.ogg")),
        (HeroEvent::LowHp, asset.load("audio/vo/hurt.ogg")),
        (HeroEvent::Home, asset.load("audio/vo/home.ogg")),
        (HeroEvent::Equip, asset.load("audio/vo/equip.ogg")),
        (HeroEvent::LevelUp, asset.load("audio/vo/levelup.ogg")),
        (HeroEvent::WaveSurvived, asset.load("audio/vo/wave_survived.ogg")),
        (HeroEvent::FirstKill, asset.load("audio/vo/first_kill.ogg")),
        (HeroEvent::GoldRich, asset.load("audio/vo/gold_rich.ogg")),
        (HeroEvent::Broke, asset.load("audio/vo/broke.ogg")),
        (HeroEvent::KeepHurt, asset.load("audio/vo/keep_hurt.ogg")),
        (HeroEvent::ShrineHeal, asset.load("audio/vo/shrine_heal.ogg")),
    ];
    commands.insert_resource(VoiceBank {
        swings: ["audio/player-swing-1.ogg", "audio/player-swing-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        jump: asset.load("audio/player-jump-1.ogg"),
        hurts: ["audio/player-hurt-1.ogg", "audio/player-hurt-2.ogg", "audio/player-hurt-3.ogg"].iter().map(|f| asset.load(*f)).collect(),
        deaths: ["audio/player-death-1.ogg", "audio/player-death-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        lines,
        events,
    });
    commands.init_resource::<HeroMouth>();
}

pub(crate) fn play_voice_cues(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    bank: Res<VoiceBank>,
    mut mouth: ResMut<HeroMouth>,
    mut gates: ResMut<HeroLineGates>,
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
                // A death cry always plays (interrupts) but still counts as the last line.
                mouth.line_until = now + LINE_GUARD;
                mouth.last_line = now;
                pending = Some((pick(&bank.deaths, &mut seed), cfg.voice_vol, true));
            }
            AudioCue::HeroLine(b) => {
                // Once per biome per run (old game's `biome:` `spoken` gate), only when the mouth
                // is free, AND at least GLOBAL_LINE_GAP since the last line — so a musing that
                // can't fire (mid-sentence or inside the gap) isn't marked spoken and re-fires
                // once he's quiet (the emitter re-sends every frame he's inside the biome).
                if !gates.spoken_biomes.contains(&b)
                    && now >= mouth.line_until
                    && now - mouth.last_line >= GLOBAL_LINE_GAP
                {
                    if let Some(h) = bank.lines.iter().find(|(bb, _)| *bb == b).map(|(_, h)| h) {
                        gates.spoken_biomes.push(b);
                        mouth.line_until = now + LINE_GUARD;
                        mouth.last_line = now;
                        pending = Some((h.clone(), cfg.narration_vol, true));
                    }
                }
            }
            AudioCue::HeroEvent(ev) => {
                // "Once per run" gate for the first-time / home lines; the rest are repeatable
                // (NightWarning is gated once-per-prep upstream in `siege`).
                let blocked = match ev {
                    HeroEvent::FirstStone if gates.first_stone => true,
                    HeroEvent::FirstRescue if gates.first_rescue => true,
                    HeroEvent::Home if gates.home => true,
                    // Flavour reactions obey a 10-minute per-line floor so they stay occasional.
                    e if e.throttled() && now - mouth.event_last[e.key()] < EVENT_REPLAY_GAP => true,
                    _ => false,
                };
                if !blocked {
                    if let Some(h) = bank.events.iter().find(|(e, _)| *e == ev).map(|(_, h)| h) {
                        match ev {
                            HeroEvent::FirstStone => gates.first_stone = true,
                            HeroEvent::FirstRescue => gates.first_rescue = true,
                            HeroEvent::Home => gates.home = true,
                            _ => {}
                        }
                        mouth.event_last[ev.key()] = now;
                        mouth.line_until = now + LINE_GUARD;
                        // Count event hints toward the musing gap so a biome line doesn't
                        // immediately follow a hint — but don't gate the hint itself (one-shot,
                        // no retry, so a suppressed night warning would be lost).
                        mouth.last_line = now;
                        pending = Some((h.clone(), cfg.narration_vol, true));
                    }
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
