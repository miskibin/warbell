//! Background music — a calm bed loop with a combat layer that swells over it while the hero
//! is in an ork fight, then fades back out. Ported from the old game's `SoundScape` crossfade
//! (minus the day/night + wave/boss themes, which this viewer has no phases for). Both loops
//! play continuously; we only ride their volumes so there are no start/stop pops.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::{AudioConfig, MusicState};

/// How fast the combat layer eases in/out (per second). Snappy so a fight is felt promptly.
const COMBAT_FADE: f32 = 1.5;
/// How far the calm bed ducks under a full combat swell. 1.0 = the combat track fully REPLACES
/// the bed (plays solo, no overlap) — matches the old game's `COMBAT_DUCK = 1`.
const BED_DUCK: f32 = 1.0;

/// Which music loop a sink is.
#[derive(Component, Clone, Copy)]
pub(crate) enum MusicLayer {
    Bed,
    Combat,
}

pub(crate) fn setup_music(asset: Res<AssetServer>, cfg: Res<AudioConfig>, mut commands: Commands) {
    // Calm bed — audible from the start.
    commands.spawn((
        AudioPlayer(asset.load::<AudioSource>("audio/music-bed.ogg")),
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(cfg.music_vol),
            spatial: false,
            ..default()
        },
        MusicLayer::Bed,
    ));
    // Combat layer — silent until a fight swells it.
    commands.spawn((
        AudioPlayer(asset.load::<AudioSource>("audio/music-combat.ogg")),
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(0.0),
            spatial: false,
            ..default()
        },
        MusicLayer::Combat,
    ));
}

pub(crate) fn update_music(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    state: Res<MusicState>,
    mut heat: Local<f32>,
    mut q: Query<(&MusicLayer, &mut AudioSink)>,
) {
    let dt = time.delta_secs();
    let target = if state.fighting { 1.0 } else { 0.0 };
    *heat += (target - *heat) * (dt * COMBAT_FADE).min(1.0);
    let h = *heat;
    for (layer, mut sink) in &mut q {
        let v = match layer {
            MusicLayer::Bed => cfg.music_vol * (1.0 - BED_DUCK * h),
            MusicLayer::Combat => cfg.music_vol * cfg.combat_music * h,
        };
        sink.set_volume(Volume::Linear(v));
    }
}
