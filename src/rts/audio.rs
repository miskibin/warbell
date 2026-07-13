//! Skirmish audio glue. The game's audio **playback** pipeline (`src/audio/`) is NOT campaign-gated
//! — `sfx::play_cues` and `director::speak_director` already run in skirmish — so making the RTS
//! audible is purely a matter of *writing the same messages campaign code writes*: an [`AudioCue`]
//! for a one-shot sound effect, a [`Speak`] for a voiced line. This module owns the two RTS-only
//! voice triggers that need their own state (throttled low-resource advice + the match-end line);
//! the incidental SFX (selection clicks, orders, building placement, combat strikes, arrow looses)
//! are written inline from the systems that already own those events (`select`/`command`/`build`/
//! `units`).
//!
//! Voice caveat (known): `Concept::AdviseWood/Farm/Stone`, `WarlordSlain`, `KeepLost` carry
//! campaign-flavoured transcripts ("the keep", "the orks"). They play correctly but the wording is
//! campaign-ish until skirmish-specific lines are recorded — an acceptable reuse for now.

use bevy::prelude::*;

use crate::audio::{Concept, Speak};
use crate::game_state::{AppState, Modal};
use crate::rts::{in_skirmish, RtsBanks, RtsOutcome, Side};

/// Below this stock (units) a resource is "short" and worth a spoken nudge.
const LOW_WOOD: f64 = 20.0;
const LOW_FOOD: f64 = 15.0;
const LOW_STONE: f64 = 15.0;
/// Don't repeat the same advice within this many seconds (so a lingering shortage doesn't nag).
const ADVICE_COOLDOWN: f32 = 40.0;

/// Per-concept "last spoken at" clock for the throttle (sim seconds; 0 = never).
#[derive(Resource, Default)]
struct AdviceClock {
    wood: f32,
    food: f32,
    stone: f32,
}

pub struct RtsAudioPlugin;

impl Plugin for RtsAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AdviceClock>().add_systems(
            Update,
            (
                low_resource_advice.run_if(in_state(Modal::None)),
                match_end_voice,
            )
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

/// Watch the player's bank; when a resource runs short, have the hero voice the matching advice
/// (throttled per resource). This is the "we're low on wood / food / stone" nudge the player asked
/// for. Only the PLAYER side is voiced.
fn low_resource_advice(
    time: Res<Time>,
    banks: Res<RtsBanks>,
    mut clock: ResMut<AdviceClock>,
    mut speak: MessageWriter<Speak>,
) {
    let now = time.elapsed_secs();
    let b = banks.side(Side::Player);
    // One line per tick at most (pick the most-pressing shortage) so two empties don't talk over
    // each other. Food first (starvation is the hard fail), then wood (everything costs it), stone.
    if b.food < LOW_FOOD && now - clock.food > ADVICE_COOLDOWN {
        clock.food = now;
        speak.write(Speak::new(Concept::AdviseFarm));
    } else if b.wood < LOW_WOOD && now - clock.wood > ADVICE_COOLDOWN {
        clock.wood = now;
        speak.write(Speak::new(Concept::AdviseWood));
    } else if b.stone < LOW_STONE && now - clock.stone > ADVICE_COOLDOWN {
        clock.stone = now;
        speak.write(Speak::new(Concept::AdviseStone));
    }
}

/// Voice the match verdict once when it lands (victory cheer / defeat lament).
fn match_end_voice(
    outcome: Res<RtsOutcome>,
    mut said: Local<bool>,
    mut speak: MessageWriter<Speak>,
) {
    if *said {
        return;
    }
    match *outcome {
        RtsOutcome::PlayerWon => {
            *said = true;
            speak.write(Speak::new(Concept::WarlordSlain)); // "It's over. It's finally over."
        }
        RtsOutcome::RivalWon => {
            *said = true;
            speak.write(Speak::new(Concept::KeepLost)); // "The walls are down…"
        }
        RtsOutcome::Undecided => {}
    }
}
