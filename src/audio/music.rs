//! Background music — a **phase-driven multi-track** mix ported from the old game's `SoundScape`
//! crossfade. All loops play continuously (no start/stop pops); we only ride their volumes:
//!   - **Day** — a random one of the [`DAY_TRACKS`] plays, re-rolled each dawn; ducked under combat.
//!   - **Combat** — swells over the day track while the hero is in a daytime ork fight.
//!   - **Blight** — biome theme: swells over (and mutes) the day track while the hero stands
//!     in the Blight (Gnashfang Hold's mire). Daytime only — combat + the night siege still
//!     take over.
//!   - **Arid** — the desert + rocky mountains share one theme ("Heat Hail"), riding the same
//!     day slot the same way the Blight theme does.
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
    /// The Blight's own daytime theme.
    Blight,
    /// The shared desert + rocky-mountains daytime theme ("Heat Hail").
    Arid,
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
    layer("audio/blight-music.ogg", 0.0, MusicLayer::Blight); // silent until the hero enters the Blight
    layer("audio/heat-hail.ogg", 0.0, MusicLayer::Arid); // silent until the hero enters desert/rock
    layer(NIGHT_TRACK, 0.0, MusicLayer::Night); // silent until the siege wave — always this track
    layer("audio/orc-march-tallow.ogg", 0.0, MusicLayer::Boss); // silent until the boss wave
}

pub(crate) fn update_music(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    state: Res<MusicState>,
    siege: Option<Res<Siege>>,
    hero: Option<Res<crate::player::HeroState>>,
    mut heat: Local<f32>,
    mut night: Local<f32>,
    mut blight: Local<f32>,
    mut arid: Local<f32>,
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

    // Ease the mix scalars: combat (daytime ork fight), night (the siege wave), and blight
    // (the hero standing in Gnashfang Hold's mire — the one biome with its own theme).
    let combat_target = if state.fighting { 1.0 } else { 0.0 };
    *heat += (combat_target - *heat) * (dt * COMBAT_FADE).min(1.0);
    *night += ((if is_wave { 1.0 } else { 0.0 }) - *night) * (dt * NIGHT_FADE).min(1.0);
    let in_blight = hero
        .as_deref()
        .is_some_and(|h| h.alive && crate::ork_fortress::in_blight_world(h.pos.x, h.pos.y));
    *blight += ((if in_blight { 1.0 } else { 0.0 }) - *blight) * (dt * NIGHT_FADE).min(1.0);
    // The desert and the rocky mountains share one theme; the Blight reads as Swamp to
    // `biome_at_world`, so the two gates can never be on together (eases just crossfade).
    let in_arid = hero.as_deref().is_some_and(|h| {
        h.alive
            && matches!(
                crate::worldmap::biome_at_world(h.pos.x, h.pos.y),
                Some(crate::biome::Biome::Desert | crate::biome::Biome::Rocky)
            )
    });
    *arid += ((if in_arid { 1.0 } else { 0.0 }) - *arid) * (dt * NIGHT_FADE).min(1.0);
    let (h, n, b, a) = (*heat, *night, *blight, *arid);
    let day = cfg.music_vol * (1.0 - n); // day tracks fade out as night rises

    for (layer, mut sink) in &mut q {
        let v = match layer {
            // Only the chosen day track is audible; combat ducks it; biome themes mute it.
            MusicLayer::Day(i) => {
                day * (1.0 - BED_DUCK * h)
                    * (1.0 - b)
                    * (1.0 - a)
                    * if *i == *day_pick { 1.0 } else { 0.0 }
            }
            MusicLayer::Combat => day * cfg.combat_music * h,
            // Biome themes ride the day slot (ducked by combat, gone at night) but gated on
            // position instead of the day-track roll.
            MusicLayer::Blight => day * (1.0 - BED_DUCK * h) * b,
            MusicLayer::Arid => day * (1.0 - BED_DUCK * h) * a,
            MusicLayer::Night => cfg.music_vol * n * if !boss { 1.0 } else { 0.0 },
            MusicLayer::Boss => cfg.music_vol * n * if boss { 1.0 } else { 0.0 },
        };
        sink.set_volume(Volume::Linear(v));
    }
}
