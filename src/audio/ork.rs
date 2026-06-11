//! Ork voices — **trigger only**. This module DETECTS ork-speech opportunities and emits
//! [`super::Speak`] requests; the catalog (`lines.rs`) owns the line data + pitch config, and the
//! director (`director.rs`) owns playback + subtitle. The old bespoke `OrkVoiceBank` /
//! `setup_ork_voice` / `ork_voices` driver has been replaced by the catalog path (Task C4).
//!
//! Architecture:
//! - [`OrkTrigger`] is the one global throttle (replaces `OrkVoiceState`).
//! - [`detect_ork_voices`] fires `Speak::at(OrkSpot/OrkDeath, pos)`.
//! - Pitch-shift per utterance is now DATA on `SpeakerVoice::pitch` for `Speaker::Ork` (0.82–1.18)
//!   applied by the director's `play_line` path — no explicit `speed` param needed here.
//! - The old `ORK_LINE_GUARD` / `OthersSpeaking` stamp is gone: the director tracks the ork's
//!   active line via `VoiceManager.active` and the hero defers via `mgr.others_speaking` already.

use bevy::prelude::*;

use crate::dying::Dying;
use crate::orks::Ork;
use crate::player::Hero;

use super::frand;

/// Shortest gap between ANY two ork utterances; a random slice up to [`BARK_GAP_JITTER`] is added
/// on top so the cadence is irregular. (Tightened 18/14 → 12/10 on playtest feedback — the
/// orks read too quiet; the fortress denizens run their own, slightly faster throttle in
/// `ork_fortress::fortress_barks`.)
const BARK_GAP: f32 = 12.0;
const BARK_GAP_JITTER: f32 = 10.0;
/// An ork must be within this of the hero (world units) for its bark to be worth playing.
const EARSHOT: f32 = 32.0;
/// When an ork falls while the cooldown is clear, the chance we play its death snarl (vs. letting
/// a living ork bark a battle line instead) — keeps frequent deaths from drowning out taunts.
const DEATH_CHANCE: f32 = 0.4;

/// Per-run ork bark trigger state. Replaces `OrkVoiceState` — this is now purely the throttle
/// and jitter RNG; the director handles everything else.
#[derive(Resource)]
pub(crate) struct OrkTrigger {
    /// Earliest time the next ork utterance may be emitted — the one global throttle.
    next_bark: f32,
    rng: u32,
}

impl Default for OrkTrigger {
    fn default() -> Self {
        Self { next_bark: 20.0, rng: 0x51ed_270b }
    }
}

pub(crate) fn reset_ork_trigger(mut t: ResMut<OrkTrigger>) {
    *t = OrkTrigger::default();
}

/// The ork-voice trigger: occasionally (global cooldown) emits a [`super::Speak`] for either a
/// newly-fallen ork's death snarl or a living ork's battle bark. The director picks the specific
/// line, applies the speaker's pitch range, plays it, and shows a subtitle.
pub(crate) fn detect_ork_voices(
    time: Res<Time>,
    mut t: ResMut<OrkTrigger>,
    mgr: Res<super::director::VoiceManager>,
    mut speak: MessageWriter<crate::audio::Speak>,
    hero: Query<&Hero>,
    dying: Query<&GlobalTransform, (Added<Dying>, With<Ork>)>,
    alive: Query<&GlobalTransform, (With<Ork>, Without<Dying>)>,
) {
    let now = time.elapsed_secs();
    if now < t.next_bark {
        return;
    }
    // Defer to the hero — no talking over him.
    if mgr.hero_speaking(now) {
        return;
    }
    let Ok(hero) = hero.single() else { return };

    // A newly-fallen ork's dying snarl (only sometimes, so battle cries get a turn too).
    if let Some(gt) = dying.iter().next() {
        if frand(&mut t.rng) < DEATH_CHANCE {
            speak.write(crate::audio::Speak::at(super::Concept::OrkDeath, gt.translation()));
            t.next_bark = now + BARK_GAP + frand(&mut t.rng) * BARK_GAP_JITTER;
            return;
        }
    }

    // Otherwise the nearest living ork in earshot barks a battle line.
    let mut best: Option<(Vec3, f32)> = None;
    for gt in &alive {
        let p = gt.translation();
        let d = Vec2::new(p.x, p.z).distance(hero.pos);
        if d <= EARSHOT && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((p, d));
        }
    }
    let Some((pos, _)) = best else { return };
    speak.write(crate::audio::Speak::at(super::Concept::OrkSpot, pos));
    t.next_bark = now + BARK_GAP + frand(&mut t.rng) * BARK_GAP_JITTER;
}
