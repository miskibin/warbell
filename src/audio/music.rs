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
use crate::game_state::AppState;
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
    /// The warden (biome-boss) fight theme — swells over the daytime mix while any warden is
    /// engaged (`MusicState.warden_active`), ducking the day/combat/biome layers under it.
    Warden,
    /// The title-screen theme — swells on `AppState::StartScreen` and ducks every other layer.
    Menu,
}

pub(crate) fn setup_music(asset: Res<AssetServer>, mut commands: Commands) {
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
    // All day tracks loop silently; `update_music` raises only the one picked for the current day
    // (the pick is rolled on the first frame, so even day 1 varies). They start at 0.0 — NOT the
    // bed at `music_vol` — so the day bed doesn't blare for a frame at launch before the title
    // theme's first `update_music` tick ducks it (that was the "main music plays first" bug).
    for (i, f) in DAY_TRACKS.iter().enumerate() {
        layer(*f, 0.0, MusicLayer::Day(i));
    }
    layer("audio/music-combat.ogg", 0.0, MusicLayer::Combat); // silent until a fight
    layer("audio/blight-music.ogg", 0.0, MusicLayer::Blight); // silent until the hero enters the Blight
    layer("audio/heat-hail.ogg", 0.0, MusicLayer::Arid); // silent until the hero enters desert/rock
    layer(NIGHT_TRACK, 0.0, MusicLayer::Night); // silent until the siege wave — always this track
    layer("audio/orc-march-tallow.ogg", 0.0, MusicLayer::Boss); // silent until the boss wave
    layer("audio/boss-fight-music.ogg", 0.0, MusicLayer::Warden); // silent until a warden is engaged
    layer("audio/menu-theme.ogg", 0.0, MusicLayer::Menu); // fades in on the title screen
}

/// Cross-frame edge-detect flags for [`update_music`], folded into one `Local` (Bevy caps a
/// system at 16 params, and the volume scalars + RNG already fill the rest).
#[derive(Default)]
pub(crate) struct DriverFlags {
    /// Was the siege in its `Wave` phase last frame? (dawn = Wave→Prep edge).
    prev_wave: bool,
    /// Was the title screen up last frame? (run start = StartScreen→Playing edge).
    prev_on_menu: bool,
    /// Has the first frame run? (first-frame day-track roll + instant menu swell).
    booted: bool,
    /// Eased warden-fight swell, toward `MusicState.warden_active` (0 = no warden engaged).
    warden: f32,
}

pub(crate) fn update_music(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    state: Res<MusicState>,
    app: Res<State<AppState>>,
    siege: Option<Res<Siege>>,
    hero: Option<Res<crate::player::HeroState>>,
    mut heat: Local<f32>,
    mut night: Local<f32>,
    mut blight: Local<f32>,
    mut arid: Local<f32>,
    mut menu: Local<f32>,
    mut flags: Local<DriverFlags>,
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
    let on_menu = *app.get() == AppState::StartScreen;

    // Roll a fresh DAY track at every "new day" so the day music varies: at each dawn (Wave→Prep
    // edge), the moment a run BEGINS (title screen → Playing), AND on the very first frame. The
    // run-start / first-frame rolls are what make **day 1** vary — before, `day_pick` defaulted to
    // 0 (the bed) and only the dawn edge re-rolled it, so the opening day was always the same bed
    // while every later day differed.
    let dawn = !is_wave && flags.prev_wave;
    let run_started = flags.prev_on_menu && !on_menu;
    if !flags.booted || dawn || run_started {
        // Mix the clock into the seed so the pick isn't identical every launch (`frand` self-seeds
        // a zero state); on the run-start roll the menu dwell time gives real entropy.
        *seed ^= time.elapsed_secs().to_bits().rotate_left(13).wrapping_add(0x9e37_79b9);
        *day_pick = (frand(&mut seed) * DAY_TRACKS.len() as f32) as usize % DAY_TRACKS.len();
    }
    flags.prev_wave = is_wave;
    flags.prev_on_menu = on_menu;

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
    // Warden fight: swell the boss theme while any biome boss is engaged (eased on `flags`, which
    // is already a `Local`, so we don't add a 16th system param).
    flags.warden += ((if state.warden_active { 1.0 } else { 0.0 }) - flags.warden) * (dt * NIGHT_FADE).min(1.0);
    // The title screen has its own theme; swell it (and duck everything else) while on it. On the
    // FIRST frame snap it straight to full so it plays the instant the window opens — it used to
    // ease up from silence over ~1 s, so the day bed was heard first and the menu theme only crept
    // in "after a while". After boot it cross-fades normally when leaving / returning to the menu.
    let menu_target = if on_menu { 1.0 } else { 0.0 };
    if !flags.booted {
        *menu = menu_target;
    } else {
        *menu += (menu_target - *menu) * (dt * NIGHT_FADE).min(1.0);
    }
    flags.booted = true;
    let (h, n, b, a, m, w) = (*heat, *night, *blight, *arid, *menu, flags.warden);
    let day = cfg.music_vol * (1.0 - n); // day tracks fade out as night rises

    for (layer, mut sink) in &mut q {
        let v = match layer {
            // Only the chosen day track is audible; combat ducks it; biome themes mute it; a
            // warden fight (`w`) ducks it like the biome themes do.
            MusicLayer::Day(i) => {
                day * (1.0 - BED_DUCK * h)
                    * (1.0 - b)
                    * (1.0 - a)
                    * (1.0 - w)
                    * if *i == *day_pick { 1.0 } else { 0.0 }
            }
            MusicLayer::Combat => day * cfg.combat_music * h * (1.0 - w),
            // Biome themes ride the day slot (ducked by combat, gone at night) but gated on
            // position instead of the day-track roll; a warden fight overrides them too.
            MusicLayer::Blight => day * (1.0 - BED_DUCK * h) * b * (1.0 - w),
            MusicLayer::Arid => day * (1.0 - BED_DUCK * h) * a * (1.0 - w),
            // The warden theme owns the daytime mix while a boss is engaged (gone at night).
            MusicLayer::Warden => day * w,
            MusicLayer::Night => cfg.music_vol * n * if !boss { 1.0 } else { 0.0 },
            MusicLayer::Boss => cfg.music_vol * n * if boss { 1.0 } else { 0.0 },
            MusicLayer::Menu => cfg.music_vol * m,
        };
        // The menu theme owns the mix while it's up; every in-game layer ducks under it.
        let v = if matches!(layer, MusicLayer::Menu) { v } else { v * (1.0 - m) };
        sink.set_volume(Volume::Linear(v));
    }
}
