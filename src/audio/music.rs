//! Background music — a **phase-driven multi-track** mix ported from the old game's `SoundScape`
//! crossfade. Four loops play continuously (no start/stop pops); we only ride their volumes:
//!   - **Bed** (day) — audible in prep/free-roam, ducked under a combat swell.
//!   - **Combat** — swells over the bed while the hero is in a daytime ork fight.
//!   - **Night dread** — swells in while the siege is in its `Wave` phase (and it's not the boss).
//!   - **Boss march** — replaces the dread on the final boss wave.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::{frand, AudioConfig, MusicState};
use crate::siege::{GamePhase, Siege, WAVES};

/// The night-dread tracks — one is picked at random each night (Prep→Wave edge): sometimes the
/// original dread, sometimes one of the hurdy-gurdy hymns. All three loop silently from startup;
/// only the chosen one's volume rises during the wave.
const NIGHT_TRACKS: [&str; 3] = [
    "audio/soot-banner-dread.ogg", // the original night dread
    "audio/night-hymn-1.ogg",      // Hurdy-Gurdy Hymn (1)
    "audio/night-hymn-2.ogg",      // Hurdy-Gurdy Hymn (2)
];

/// How fast the combat layer eases in/out (per second).
const COMBAT_FADE: f32 = 1.5;
/// How fast the day↔night crossfade eases.
const NIGHT_FADE: f32 = 0.9;
/// How far the day bed ducks under a full combat swell (1.0 = combat plays solo).
const BED_DUCK: f32 = 1.0;

/// Which music loop a sink is.
#[derive(Component, Clone, Copy)]
pub(crate) enum MusicLayer {
    Bed,
    Combat,
    /// One of the [`NIGHT_TRACKS`] (by index) — only the per-night chosen index plays.
    Night(usize),
    Boss,
}

pub(crate) fn setup_music(asset: Res<AssetServer>, cfg: Res<AudioConfig>, mut commands: Commands) {
    let mut layer = |file: &'static str, vol: f32, which: MusicLayer| {
        commands.spawn((
            AudioPlayer(asset.load::<AudioSource>(file)),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(vol),
                spatial: false,
                ..default()
            },
            which,
        ));
    };
    layer("audio/music-bed.ogg", cfg.music_vol, MusicLayer::Bed); // day bed — audible from start
    layer("audio/music-combat.ogg", 0.0, MusicLayer::Combat); // silent until a fight
    // All night tracks loop silently; the night driver raises only the one picked for that night.
    for (i, f) in NIGHT_TRACKS.iter().enumerate() {
        layer(*f, 0.0, MusicLayer::Night(i));
    }
    layer("audio/orc-march-tallow.ogg", 0.0, MusicLayer::Boss); // silent until the boss wave
}

pub(crate) fn update_music(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    state: Res<MusicState>,
    siege: Option<Res<Siege>>,
    mut heat: Local<f32>,
    mut night: Local<f32>,
    mut prev_wave: Local<bool>,
    mut night_pick: Local<usize>,
    mut seed: Local<u32>,
    mut q: Query<(&MusicLayer, &mut AudioSink)>,
) {
    let dt = time.delta_secs();
    let (is_wave, boss) = match siege.as_deref() {
        Some(s) => {
            let wave = s.phase == GamePhase::Wave;
            (wave, wave && s.wave_index >= 0 && s.wave_index as usize == WAVES.len() - 1)
        }
        None => (false, false),
    };

    // On the dusk edge (Prep→Wave), roll a fresh night track: sometimes the old dread, sometimes
    // a hymn. Picked once per night so it doesn't flicker between tracks mid-wave.
    if is_wave && !*prev_wave {
        *night_pick = (frand(&mut seed) * NIGHT_TRACKS.len() as f32) as usize % NIGHT_TRACKS.len();
    }
    *prev_wave = is_wave;

    // Ease the two mix scalars: combat (daytime ork fight) + night (the siege wave).
    let combat_target = if state.fighting { 1.0 } else { 0.0 };
    *heat += (combat_target - *heat) * (dt * COMBAT_FADE).min(1.0);
    *night += ((if is_wave { 1.0 } else { 0.0 }) - *night) * (dt * NIGHT_FADE).min(1.0);
    let (h, n) = (*heat, *night);
    let day = cfg.music_vol * (1.0 - n); // day tracks fade out as night rises

    for (layer, mut sink) in &mut q {
        let v = match layer {
            MusicLayer::Bed => day * (1.0 - BED_DUCK * h),
            MusicLayer::Combat => day * cfg.combat_music * h,
            MusicLayer::Night(i) => cfg.music_vol * n * if !boss && *i == *night_pick { 1.0 } else { 0.0 },
            MusicLayer::Boss => cfg.music_vol * n * if boss { 1.0 } else { 0.0 },
        };
        sink.set_volume(Volume::Linear(v));
    }
}
