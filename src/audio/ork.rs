//! Ork voices — the horde's battle barks + dying snarls. One deep "cave monster" take, but each
//! utterance is **pitch-shifted by a random amount** (via [`PlaybackSettings::speed`]) so the
//! warband doesn't all sound like the same throat. Deliberately RARE: a single global cooldown
//! ([`OrkVoiceState::next_bark`]) gates ALL ork speech, so you hear the odd taunt over the din
//! rather than a constant chorus. Clips live in `assets/audio/vo/ork/`.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::dying::Dying;
use crate::orks::Ork;
use crate::player::Hero;

use super::{frand, AudioConfig};

/// Shortest gap between ANY two ork utterances; a random slice up to [`BARK_GAP_JITTER`] is added
/// on top so the cadence is irregular.
const BARK_GAP: f32 = 28.0;
const BARK_GAP_JITTER: f32 = 20.0;
/// An ork must be within this of the hero (world units) for its bark to be worth playing.
const EARSHOT: f32 = 32.0;
/// When an ork falls while the cooldown is clear, the chance we play its death snarl (vs. letting
/// a living ork bark a battle line instead) — keeps frequent deaths from drowning out taunts.
const DEATH_CHANCE: f32 = 0.4;
/// Pitch (and tempo) spread per utterance — the "different ork every time" knob.
const PITCH_LO: f32 = 0.82;
const PITCH_HI: f32 = 1.18;
/// Ork voice gain (spatial).
const ORK_GAIN: f32 = 0.85;

/// The general battle-cry pool (aligned with [`OrkVoiceBank::battle`]).
const BATTLE_KEYS: [&str; 7] = ["spot", "charge", "blood", "taunt", "where", "feast", "shaman"];

#[derive(Resource)]
pub(crate) struct OrkVoiceBank {
    battle: Vec<Handle<AudioSource>>,
    death: Handle<AudioSource>,
}

#[derive(Resource)]
pub(crate) struct OrkVoiceState {
    /// Earliest time the next ork utterance may play (the one global throttle).
    next_bark: f32,
    rng: u32,
}

impl Default for OrkVoiceState {
    fn default() -> Self {
        Self { next_bark: 20.0, rng: 0x51ed_270b }
    }
}

pub(crate) fn setup_ork_voice(asset: Res<AssetServer>, mut commands: Commands) {
    let battle = BATTLE_KEYS.iter().map(|k| asset.load(format!("audio/vo/ork/{k}.ogg"))).collect();
    commands.insert_resource(OrkVoiceBank { battle, death: asset.load("audio/vo/ork/death.ogg") });
    commands.init_resource::<OrkVoiceState>();
}

/// Spawn a clip as a standalone spatial sink at a world point (not a child — so a dying ork's
/// snarl outlives its 1.4 s fade), pitch-shifted by `speed`.
fn say_at(commands: &mut Commands, pos: Vec3, clip: Handle<AudioSource>, vol: f32, speed: f32) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            speed,
            spatial: true,
            ..default()
        },
        Transform::from_translation(pos),
    ));
}

/// The single ork-voice driver: occasionally (global cooldown) either a freshly-fallen ork's
/// death snarl or a living ork's battle bark, from the nearest ork in earshot, randomly pitched.
pub(crate) fn ork_voices(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    bank: Res<OrkVoiceBank>,
    mut st: ResMut<OrkVoiceState>,
    hero: Query<&Hero>,
    dying: Query<&GlobalTransform, (Added<Dying>, With<Ork>)>,
    alive: Query<&GlobalTransform, (With<Ork>, Without<Dying>)>,
) {
    let now = time.elapsed_secs();
    if now < st.next_bark {
        return;
    }
    let Ok(hero) = hero.single() else { return };
    let vol = ORK_GAIN * cfg.voice_vol;
    let pitch = PITCH_LO + frand(&mut st.rng) * (PITCH_HI - PITCH_LO);

    // A newly-fallen ork's dying snarl (only some of the time, so battle cries get a turn too).
    if let Some(gt) = dying.iter().next() {
        if frand(&mut st.rng) < DEATH_CHANCE {
            say_at(&mut commands, gt.translation(), bank.death.clone(), vol, pitch);
            st.next_bark = now + BARK_GAP + frand(&mut st.rng) * BARK_GAP_JITTER;
            return;
        }
    }

    // Otherwise the nearest living ork in earshot barks a random battle line.
    let mut best: Option<(Vec3, f32)> = None;
    for gt in &alive {
        let t = gt.translation();
        let d = Vec2::new(t.x, t.z).distance(hero.pos);
        if d <= EARSHOT && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((t, d));
        }
    }
    let Some((pos, _)) = best else { return }; // no ork near → stay quiet, retry next frame
    let i = (frand(&mut st.rng) * BATTLE_KEYS.len() as f32) as usize % BATTLE_KEYS.len();
    say_at(&mut commands, pos, bank.battle[i].clone(), vol, pitch);
    st.next_bark = now + BARK_GAP + frand(&mut st.rng) * BARK_GAP_JITTER;
}
