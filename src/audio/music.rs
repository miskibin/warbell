//! Background music — a **phase-driven multi-track** mix ported from the old game's `SoundScape`
//! crossfade. All loops play continuously (no start/stop pops); we only ride their volumes:
//!   - **Day** — a random one of the [`DAY_TRACKS`] plays, re-rolled each dawn; ducked under combat.
//!   - **Combat** — swells over the day track while the hero is in a daytime ork fight.
//!   - **Night dread** — the SAME track swells in every night the siege is in its `Wave` phase.
//!   - **Boss march** — replaces the dread on the final boss wave.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::{frand, AudioConfig, MusicState};
use crate::siege::{GamePhase, Siege, WAVES};

/// The day tracks — one is picked at random each dawn (Wave→Prep edge). All loop silently from
/// startup; only the chosen one's volume rides up during the day. (The two hymns are the old
/// hurdy-gurdy night clips, re-tagged as day music.)
const DAY_TRACKS: [&str; 3] = [
    "audio/music-bed.ogg",   // the original day bed
    "audio/day-hymn-1.ogg",  // Hurdy-Gurdy Hymn (1)
    "audio/day-hymn-2.ogg",  // Hurdy-Gurdy Hymn (2)
];

/// The night track — ALWAYS the same dread every night (no per-night roll).
const NIGHT_TRACK: &str = "audio/soot-banner-dread.ogg";

/// How fast the combat layer eases in/out (per second).
const COMBAT_FADE: f32 = 1.5;
/// How fast the day↔night crossfade eases.
const NIGHT_FADE: f32 = 0.9;
/// How far the day bed ducks under a full combat swell (1.0 = combat plays solo).
const BED_DUCK: f32 = 1.0;

/// Which music loop a sink is.
#[derive(Component, Clone, Copy)]
pub(crate) enum MusicLayer {
    /// One of the [`DAY_TRACKS`] (by index) — only the per-day chosen index plays.
    Day(usize),
    Combat,
    Night,
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
    // All day tracks loop silently; the driver raises only the one picked for the current day
    // (index 0, the bed, plays first — re-rolled each subsequent dawn).
    for (i, f) in DAY_TRACKS.iter().enumerate() {
        layer(*f, if i == 0 { cfg.music_vol } else { 0.0 }, MusicLayer::Day(i));
    }
    layer("audio/music-combat.ogg", 0.0, MusicLayer::Combat); // silent until a fight
    layer(NIGHT_TRACK, 0.0, MusicLayer::Night); // silent until the siege wave — always this track
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
    mut day_pick: Local<usize>,
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

    // On the dawn edge (Wave→Prep), roll a fresh DAY track so each day sounds different. Picked
    // once per day so it doesn't flicker mid-day. Night always uses the same single dread track.
    if !is_wave && *prev_wave {
        *day_pick = (frand(&mut seed) * DAY_TRACKS.len() as f32) as usize % DAY_TRACKS.len();
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
            // Only the chosen day track is audible; combat ducks it.
            MusicLayer::Day(i) => day * (1.0 - BED_DUCK * h) * if *i == *day_pick { 1.0 } else { 0.0 },
            MusicLayer::Combat => day * cfg.combat_music * h,
            MusicLayer::Night => cfg.music_vol * n * if !boss { 1.0 } else { 0.0 },
            MusicLayer::Boss => cfg.music_vol * n * if boss { 1.0 } else { 0.0 },
        };
        sink.set_volume(Volume::Linear(v));
    }
}
