//! Spatial wildlife voices — animals call out, but only when the camera is near enough to
//! hear them. Bevy's built-in spatial audio (a `SpatialListener` on the camera) does the
//! stereo panning + distance falloff; we additionally CULL emission to animals within
//! [`AudioConfig::audible_range`] of the camera, so we never spawn a sink for an animal the
//! player can't possibly hear.
//!
//! Only species we have real recordings for get a voice; the rest stay silent. Each call
//! spawns a short-lived child audio entity on the animal ([`PlaybackMode::Despawn`]), so the
//! sound follows the creature and cleans itself up when the clip ends. (Moved verbatim from
//! the original `audio.rs`; these are this game's own curated clips, not the ported bank.)

use std::collections::HashMap;

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::critters::Species;
use crate::wildlife::{rng_range, Animal};

use super::AudioConfig;

/// Hard floor between any two calls from the SAME animal. A startle zeroes `voice_timer`, so
/// without this a stuck animal that re-startles every frame would spawn a sound every frame
/// (the "broke my speakers" runaway). This caps every animal to one call per `MIN_GAP`.
const MIN_GAP: f32 = 8.0;

/// Master scale applied to every wildlife call on top of its per-species gain — keeps the
/// animals as quiet background ambience under the combat/footstep mix rather than barking over it.
const WILDLIFE_GAIN: f32 = 0.5;

/// One species' ambient call set: clips chosen at random + a per-species gain + how rarely it
/// calls. `rarity` multiplies the rescheduled idle timer, so a species with `rarity > 1.0` calls
/// proportionally less often than the base `call_min..call_max` cadence (e.g. a dramatic wolf
/// howl we don't want barking constantly).
struct VoiceSet {
    clips: Vec<Handle<AudioSource>>,
    volume: f32,
    rarity: f32,
}

/// Per-species voices. A species absent from the map makes no sound.
#[derive(Resource, Default)]
pub(crate) struct Voices(HashMap<Species, VoiceSet>);

/// Load the curated `assets/audio/*.ogg` clips once at startup. To give a silent species a
/// voice later, drop its `.ogg` in `assets/audio/` and add a line here.
pub(crate) fn load_voices(asset: Res<AssetServer>, mut commands: Commands) {
    let mut m = HashMap::new();
    let mut add = |s: Species, vol: f32, rarity: f32, files: &[&'static str]| {
        m.insert(s, VoiceSet { clips: files.iter().map(|f| asset.load(*f)).collect(), volume: vol, rarity });
    };
    // (species, gain, rarity, clips). rarity 1.0 = base cadence; >1.0 = calls that much less often.
    add(Species::Camel, 0.5, 1.0, &["audio/camel.ogg"]);
    add(Species::Deer, 0.9, 1.0, &["audio/deer-1.ogg", "audio/deer-2.ogg"]);
    add(Species::Goat, 0.9, 1.0, &["audio/goat-1.ogg", "audio/goat-2.ogg"]);
    add(Species::Rabbit, 0.35, 1.0, &["audio/rabbit.ogg"]);
    add(Species::PolarBear, 1.3, 1.0, &["audio/bear-growl.ogg", "audio/bear-roar.ogg"]);
    add(Species::Dog, 0.4, 1.0, &["audio/dog-1.ogg", "audio/dog-2.ogg", "audio/dog-3.ogg", "audio/dog-4.ogg"]);
    add(Species::Cat, 0.35, 1.0, &["audio/cat-1.ogg", "audio/cat-2.ogg", "audio/cat-3.ogg", "audio/cat-4.ogg"]);
    // Wolf: a long dramatic howl — rarity 4.0 stretches its idle gap to ~2–5 min so it lands as a
    // rare, atmospheric call rather than constant howling.
    add(Species::Wolf, 1.0, 4.0, &["audio/wolf-1.ogg", "audio/wolf-2.ogg"]);
    add(Species::Boar, 1.0, 1.0, &["audio/boar-1.ogg", "audio/boar-2.ogg"]);
    // Elk: no recording yet → silent (no entry).
    commands.insert_resource(Voices(m));
}

/// Tick each voiced animal's call timer; when it fires AND the camera is in earshot, spawn a
/// one-shot spatial sink as a child of the animal.
pub(crate) fn animal_voices(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    voices: Res<Voices>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    mut q: Query<(Entity, &mut Animal, &GlobalTransform)>,
) {
    let dt = time.delta_secs();
    let Ok(cam) = cam.single() else { return };
    let cam_pos = cam.translation();

    for (e, mut a, gt) in &mut q {
        let Some(set) = voices.0.get(&a.species) else { continue };
        a.voice_timer -= dt;
        a.call_cd -= dt;
        if a.voice_timer > 0.0 {
            continue;
        }
        // Timer elapsed: reschedule the next idle call regardless of what happens below, so a
        // far-off (or rate-limited) animal silently rolls its next call rather than retrying.
        a.voice_timer = rng_range(&mut a.rng, cfg.call_min, cfg.call_max) * set.rarity;
        // Hard rate-limit — the runaway fix. Even if a startle just zeroed `voice_timer`, an
        // animal cannot emit again until `MIN_GAP` has passed since its last actual call.
        if a.call_cd > 0.0 {
            continue;
        }
        if gt.translation().distance(cam_pos) > cfg.audible_range {
            continue;
        }
        a.call_cd = MIN_GAP;
        let i = (rng_range(&mut a.rng, 0.0, set.clips.len() as f32) as usize).min(set.clips.len() - 1);
        let clip = set.clips[i].clone();
        let volume = set.volume * WILDLIFE_GAIN;
        commands.entity(e).with_children(|p| {
            p.spawn((
                AudioPlayer(clip),
                PlaybackSettings {
                    mode: PlaybackMode::Despawn,
                    volume: Volume::Linear(volume),
                    spatial: true,
                    ..default()
                },
                Transform::default(),
            ));
        });
    }
}
