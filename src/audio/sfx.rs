//! One-shot stings — combat feedback, the UI blip, footsteps, and the spatial creature
//! voices. Reads the [`AudioCue`] stream and spawns a short-lived `PlaybackMode::Despawn`
//! sink per cue (Bevy frees it when the clip ends — the equivalent of the old game's SFX
//! pool). Non-spatial cues play head-locked; ork voices spawn at a world position so the
//! camera's `SpatialListener` pans + attenuates them.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::{jitter, pick, AudioConfig, AudioCue, Surface};

/// All one-shot SFX handles, loaded once at startup.
#[derive(Resource)]
pub(crate) struct SfxBank {
    swing: Handle<AudioSource>,
    hit: Handle<AudioSource>,
    block: Handle<AudioSource>,
    ui: Handle<AudioSource>,
    foot_dirt: Handle<AudioSource>,
    foot_snow: Handle<AudioSource>,
    foot_stone: Handle<AudioSource>,
    ork_grunts: Vec<Handle<AudioSource>>,
    ork_roars: Vec<Handle<AudioSource>>,
}

pub(crate) fn setup_sfx(asset: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(SfxBank {
        swing: asset.load("audio/sword-swing.ogg"),
        hit: asset.load("audio/sword-hit.ogg"),
        block: asset.load("audio/block.ogg"),
        ui: asset.load("audio/menu-select.ogg"),
        foot_dirt: asset.load("audio/footstep-dirt.ogg"),
        foot_snow: asset.load("audio/footstep-snow.ogg"),
        foot_stone: asset.load("audio/footstep-stone.ogg"),
        ork_grunts: ["audio/ork-grunt-1.ogg", "audio/ork-grunt-2.ogg", "audio/ork-grunt-3.ogg", "audio/monster-snarl.ogg", "audio/monster-growl.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        ork_roars: ["audio/ork-roar.ogg", "audio/wave-start-roar.ogg"].iter().map(|f| asset.load(*f)).collect(),
    });
}

/// Spawn a non-spatial one-shot.
fn one_shot(commands: &mut Commands, clip: Handle<AudioSource>, vol: f32, speed: f32) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            speed,
            spatial: false,
            ..default()
        },
    ));
}

/// Spawn a one-shot positioned in the world (panned + attenuated by the camera listener).
fn spatial_shot(commands: &mut Commands, clip: Handle<AudioSource>, vol: f32, speed: f32, pos: Vec3) {
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

pub(crate) fn play_cues(
    mut commands: Commands,
    cfg: Res<AudioConfig>,
    bank: Res<SfxBank>,
    mut seed: Local<u32>,
    mut cues: MessageReader<AudioCue>,
) {
    // Base gains below are the old game's per-`playSfx` values; `sfx`/`voice` (≈ 0.6) is the
    // `audioMix.voice` master every sampled sting passed through. Keep them in sync with
    // `D:/tileworld/src/audio/sfx.ts` if retuning.
    let sfx = cfg.sfx_vol;
    let voice = cfg.voice_vol;
    for cue in cues.read() {
        match *cue {
            AudioCue::Swing => one_shot(&mut commands, bank.swing.clone(), 0.30 * sfx, jitter(&mut seed, 0.12)),
            AudioCue::Impact { kill } => {
                let v = if kill { 0.62 } else { 0.50 } * sfx;
                let p = if kill { jitter(&mut seed, 0.06) * 0.85 } else { jitter(&mut seed, 0.08) };
                one_shot(&mut commands, bank.hit.clone(), v, p);
            }
            AudioCue::Block => one_shot(&mut commands, bank.block.clone(), 0.45 * sfx, jitter(&mut seed, 0.1)),
            AudioCue::Footstep { surface, landing } => {
                let clip = match surface {
                    Surface::Dirt => bank.foot_dirt.clone(),
                    Surface::Snow => bank.foot_snow.clone(),
                    Surface::Stone => bank.foot_stone.clone(),
                };
                // STEP_VOL 0.144; a touchdown step is +20% (LAND_STEP_VOL) — Character.tsx.
                let v = if landing { 0.144 * 1.2 } else { 0.144 } * sfx;
                one_shot(&mut commands, clip, v, jitter(&mut seed, 0.12));
            }
            AudioCue::UiSelect => one_shot(&mut commands, bank.ui.clone(), 0.22 * sfx, jitter(&mut seed, 0.06)),
            AudioCue::OrkGrunt(pos) => {
                let clip = pick(&bank.ork_grunts, &mut seed);
                spatial_shot(&mut commands, clip, 0.55 * voice, jitter(&mut seed, 0.14), pos);
            }
            AudioCue::OrkRoar(pos) => {
                let clip = pick(&bank.ork_roars, &mut seed);
                spatial_shot(&mut commands, clip, 0.50 * voice, jitter(&mut seed, 0.08), pos);
            }
            // Hero-mouth cues (grunts / jump / hurt / death / lines) are handled by `voice.rs`.
            _ => {}
        }
    }
}
