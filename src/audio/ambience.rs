//! Looping ambience beds + the campfire loop.
//!
//! A quiet looping bed per biome, faded in while you're inside that biome and out when you
//! leave. Non-spatial (global), so it plays at constant volume regardless of camera angle.
//! "Inside a biome" = the active single-biome view, or — on the combined world map — the
//! biome of the tile under the camera. Both loops play continuously at volume 0; we only ride
//! their volumes, so crossing a region edge crossfades smoothly with no start/stop pops.
//!
//! The campfire loop is the one SPATIAL ambience: each camp's flame entity gets a looping sink
//! attached as a child, so it gets louder as you fly toward the fire.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::biome::Biome;
use crate::worldmap;

use super::AudioConfig;

/// Crossfade rate (per second) as you enter / leave a biome. ~1.5 s to fully fade.
const AMBIENCE_FADE: f32 = 0.8;

/// Per-biome bed gain, relative to `ambience_vol` — bumped so each biome's character (forest
/// birds, desert wind, swamp, snow-wind) reads clearly over the mix.
const BIOME_AMBIENCE_MULT: f32 = 2.0;
/// Water bed gain, relative to `ambience_vol` — kept low so rivers/coast murmur under everything
/// rather than wash it out.
const WATER_AMBIENCE_MULT: f32 = 0.5;

/// Campfire loop level at the source (spatial falloff handles distance from there).
const CAMPFIRE_VOL: f32 = 0.5;

/// How close to the castle (origin) the camera must be for the town-square bed to fade in.
const CASTLE_AMBIENCE_R: f32 = 24.0;
/// Meadow (grasshoppers / open-field) bed gain, relative to `ambience_vol`. Sits under the biome
/// beds — it's the quiet country hum of the grass ring around the castle, not a feature in itself.
const MEADOW_AMBIENCE_MULT: f32 = 1.5;

/// What makes an ambience loop fade in.
#[derive(Clone, Copy, PartialEq)]
enum AmbienceKind {
    /// Plays while you're in this biome.
    Biome(Biome),
    /// Plays while the camera is over / near water — a river band in any view, or the open
    /// sea / coast (off the island) on the combined world map.
    Water,
    /// The busy town-square bustle around the castle — only during the **day** (prep); it falls
    /// silent at night when the curfew empties the streets for the siege.
    Castle,
    /// The open meadow ("łąka") — grasshoppers + field hum over the grass ring AROUND the castle
    /// (beyond the town-square radius), but NOT inside any of the five biome blobs and NOT over
    /// water. Day only, like the town bustle: the grasshoppers fall silent under the night siege.
    Meadow,
}

/// A persistent ambience loop + its current (lerped) volume level.
#[derive(Component)]
pub(crate) struct Ambience {
    kind: AmbienceKind,
    level: f32,
}

/// Tag marking a campfire flame that already has its looping sink, so we attach exactly once.
#[derive(Component)]
pub(crate) struct CampfireAudio;

/// War-drum loop level at the source (spatial falloff handles distance from there). Kept at
/// unity: the drums should carry — hearing them faintly from across the river IS the feature —
/// but the real recording is full-scale (the old synth peaked at ~0.4), so >1.0 would clip.
const WAR_DRUM_VOL: f32 = 1.0;
/// Seconds of prep left when the camps start drumming (the warbands muster before dark).
const DRUM_LEAD: f32 = 45.0;
/// Drum fade rate (per second) — a slow swell, not a switch.
const DRUM_FADE: f32 = 0.35;

/// A camp's spatial war-drum sink + its current (lerped) volume level.
#[derive(Component)]
pub(crate) struct WarDrums {
    level: f32,
}

/// Tag marking a flame that already carries its drum sink (mirrors [`CampfireAudio`]).
#[derive(Component)]
pub(crate) struct WarDrumAudio;

/// Kids' play-chatter PEAK level, as a fraction of a villager voice line. They should sit clearly
/// *below* real dialogue — background colour, not lines the player is meant to parse. Expressed
/// relative to (and so tracking) the villager voice gain rather than the ambience bus.
const KIDS_VS_VILLAGER: f32 = 2.0 / 3.0;
/// Hero-distance bands (world units) for the chatter swell. Beyond `FAR` it's silent; it swells to
/// full toward `MID`, then DUCKS back inside `HUSH` — the children go quiet when the knight walks
/// right up to them (the "hush as you approach" beat), leaving only a shy murmur (`HUSH_FLOOR`).
const KIDS_FAR: f32 = 24.0;
const KIDS_MID: f32 = 9.0;
const KIDS_HUSH: f32 = 4.5;
const KIDS_HUSH_FLOOR: f32 = 0.12;
/// Chatter fade rate (per second) — a gentle swell/duck, no abrupt cut.
const KIDS_FADE: f32 = 1.2;
/// Intermittent envelope: the kids don't jabber non-stop — a short burst (≈one line/snippet) then a
/// long silent gap, so you only catch a bit of play *every so often*. `(min, max)` seconds.
const KIDS_BURST: (f32, f32) = (3.0, 6.0);
const KIDS_GAP: (f32, f32) = (16.0, 32.0);
/// Quiet beat after the hero arrives (a kid is back in earshot) before the first line pipes up.
const KIDS_ARRIVE_BEAT: f32 = 1.5;

/// The kids' play-chatter loop + its envelope state.
#[derive(Component)]
pub(crate) struct KidsChatter {
    /// Current (lerped) volume level.
    level: f32,
    /// True while in a "talking" burst; false during the silent gap between bursts.
    talking: bool,
    /// `elapsed_secs` at which the current burst/gap ends and the envelope flips.
    until: f32,
    /// xorshift state for the per-burst/gap jitter.
    rng: u32,
}

/// Distance → chatter gain: 0 past `KIDS_FAR`, ramps to 1 by `KIDS_MID`, then ducks to
/// `KIDS_HUSH_FLOOR` by `KIDS_HUSH` (and stays there closer in).
fn kids_swell(d: f32) -> f32 {
    if d >= KIDS_FAR {
        0.0
    } else if d >= KIDS_MID {
        ((KIDS_FAR - d) / (KIDS_FAR - KIDS_MID)).clamp(0.0, 1.0)
    } else if d >= KIDS_HUSH {
        let t = (d - KIDS_HUSH) / (KIDS_MID - KIDS_HUSH);
        KIDS_HUSH_FLOOR + t * (1.0 - KIDS_HUSH_FLOOR)
    } else {
        KIDS_HUSH_FLOOR
    }
}

/// Spawn the always-on (initially silent) ambience loops once at startup. Not tagged
/// `BiomeEntity`, so they survive biome switches; only their volume changes.
pub(crate) fn setup_ambience(asset: Res<AssetServer>, mut commands: Commands) {
    let beds = [
        (AmbienceKind::Biome(Biome::Snow), "audio/wind.ogg"),
        (AmbienceKind::Biome(Biome::Forest), "audio/forest-ambient.ogg"),
        (AmbienceKind::Biome(Biome::Desert), "audio/desert-wind.ogg"),
        (AmbienceKind::Biome(Biome::Swamp), "audio/swamp-ambient.ogg"),
        (AmbienceKind::Water, "audio/water.ogg"),
        (AmbienceKind::Castle, "audio/castle-ambient.ogg"),
        (AmbienceKind::Meadow, "audio/meadow-ambient.ogg"),
    ];
    for (kind, file) in beds {
        commands.spawn((
            AudioPlayer(asset.load::<AudioSource>(file)),
            PlaybackSettings {
                mode: PlaybackMode::Loop,
                volume: Volume::Linear(0.0),
                spatial: false,
                ..default()
            },
            Ambience { kind, level: 0.0 },
        ));
    }
    // The kids' play-chatter loop — non-spatial (volume fully controlled by `kids_chatter`),
    // silent until the hero nears a visibly-playing child.
    commands.spawn((
        AudioPlayer(asset.load::<AudioSource>("audio/kids-play.ogg")),
        PlaybackSettings {
            mode: PlaybackMode::Loop,
            volume: Volume::Linear(0.0),
            spatial: false,
            ..default()
        },
        KidsChatter { level: 0.0, talking: false, until: 0.0, rng: 0x1234_5678 },
    ));
}

/// Attach a spatial looping campfire sink to each camp flame that lacks one. Camps are part of
/// the `BiomeEntity` set, so a biome switch despawns the flame (+ its child sink) and this
/// re-attaches to the freshly-built camp on the next frame. Cheap once everyone is tagged.
pub(crate) fn attach_campfire_audio(
    asset: Res<AssetServer>,
    mut commands: Commands,
    flames: Query<Entity, (With<crate::camps::Flicker>, Without<CampfireAudio>)>,
) {
    for e in &flames {
        // queue_silenced: a biome swap can despawn the flame between this query and command
        // application — a bare insert would panic, and even a `try_insert` + `with_children`
        // would orphan a forever-looping sink. The closure runs only if the flame still exists.
        let clip = asset.load::<AudioSource>("audio/campfire-loop.ogg");
        commands.entity(e).queue_silenced(move |mut flame: EntityWorldMut| {
            flame.insert(CampfireAudio).with_children(|p| {
                p.spawn((
                    AudioPlayer(clip),
                    PlaybackSettings {
                        mode: PlaybackMode::Loop,
                        volume: Volume::Linear(CAMPFIRE_VOL),
                        spatial: true,
                        ..default()
                    },
                    Transform::default(),
                ));
            });
        });
    }
}

/// Attach a silent spatial war-drum loop to each camp flame that lacks one (same re-attach
/// lifecycle as [`attach_campfire_audio`] — camps are `BiomeEntity`s and rebuild on a biome
/// switch). The synth-baked loop plays at volume 0 all day; [`war_drums`] rides the level.
pub(crate) fn attach_war_drum_audio(
    drums: Option<Res<super::synth::WarDrumLoop>>,
    mut commands: Commands,
    flames: Query<Entity, (With<crate::camps::Flicker>, Without<WarDrumAudio>)>,
) {
    let Some(drums) = drums else { return };
    for e in &flames {
        let clip = drums.0.clone();
        commands.entity(e).queue_silenced(move |mut flame: EntityWorldMut| {
            flame.insert(WarDrumAudio).with_children(|p| {
                p.spawn((
                    AudioPlayer(clip),
                    PlaybackSettings {
                        mode: PlaybackMode::Loop,
                        volume: Volume::Linear(0.0),
                        spatial: true,
                        ..default()
                    },
                    Transform::default(),
                    WarDrums { level: 0.0 },
                ));
            });
        });
    }
}

/// Swell the camp drums as the assault musters: silent through the day, fading in over the
/// last [`DRUM_LEAD`] seconds of prep, full through the wave, dying off at dawn/victory.
pub(crate) fn war_drums(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    siege: Option<Res<crate::siege::Siege>>,
    mut q: Query<(&mut WarDrums, &mut bevy::audio::SpatialAudioSink)>,
) {
    let on = siege.as_deref().is_some_and(|s| match s.phase {
        crate::siege::GamePhase::Wave => true,
        crate::siege::GamePhase::Prep => s.prep_seconds_left < DRUM_LEAD,
        _ => false,
    });
    let target = if on { cfg.ambience_vol * WAR_DRUM_VOL } else { 0.0 };
    let k = (time.delta_secs() * DRUM_FADE).min(1.0);
    for (mut d, mut sink) in &mut q {
        d.level += (target - d.level) * k;
        sink.set_volume(Volume::Linear(d.level));
    }
}

/// Which biome's ambience should be playing: the tile under the camera on the world map
/// (`None` over grass / water / a biome with no ambience).
fn current_ambience_biome(cam_xz: Option<Vec2>) -> Option<Biome> {
    cam_xz.and_then(|p| worldmap::biome_at_world(p.x, p.y))
}

/// True if the camera is over or near water. Samples a small ring around the camera so the bed
/// fades in as you approach a shoreline / river rather than only when dead over it.
fn near_water(cam_xz: Option<Vec2>) -> bool {
    let Some(p) = cam_xz else { return false };
    const R: f32 = 6.0;
    let probes = [
        Vec2::ZERO,
        Vec2::new(R, 0.0),
        Vec2::new(-R, 0.0),
        Vec2::new(0.0, R),
        Vec2::new(0.0, -R),
        Vec2::new(R, R),
        Vec2::new(-R, -R),
    ];
    probes.iter().any(|o| {
        let q = p + *o;
        // River band, or open water (no ground tile) = sea on the map.
        crate::water::on_river(q.x, q.y) || worldmap::ground_at_world(q.x, q.y).is_none()
    })
}

/// Each frame, fade every ambience loop toward `ambience_vol` while its trigger holds, else 0.
pub(crate) fn biome_ambience(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    siege: Option<Res<crate::siege::Siege>>,
    mode: Res<crate::rts::GameMode>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    mut q: Query<(&mut Ambience, &mut AudioSink)>,
) {
    let dt = time.delta_secs();
    let cam_xz = cam.single().ok().map(|g| {
        let t = g.translation();
        Vec2::new(t.x, t.z)
    });
    let cur = current_ambience_biome(cam_xz);
    let water = near_water(cam_xz);
    // Town-square bustle plays near the castle by day; the night curfew empties the streets, so
    // it falls silent during a wave.
    let day = siege.map(|s| s.phase != crate::siege::GamePhase::Wave).unwrap_or(true);
    // Town-bustle radius: around the origin castle in campaign, and around the player's base in
    // skirmish (the RTS town sits at PLAYER_BASE, not the origin — otherwise the city ambience never
    // plays). Generous radius so the bustle carries across the whole home town.
    let near_castle = cam_xz.is_some_and(|p| {
        p.length() < CASTLE_AMBIENCE_R
            || (*mode == crate::rts::GameMode::Skirmish
                && p.distance(crate::rts::PLAYER_BASE) < CASTLE_AMBIENCE_R + 10.0)
    });
    // The grass ring ("łąka"): standing on dry land that belongs to no biome blob, beyond the
    // town-square radius, and not over water. `cur` is None over grass AND over open sea, so the
    // explicit ground check rules out the sea.
    let on_grass = cam_xz.is_some_and(|p| worldmap::ground_at_world(p.x, p.y).is_some());
    let meadow = cur.is_none() && on_grass && !water && !near_castle && day;
    let k = (dt * AMBIENCE_FADE).min(1.0);
    for (mut amb, mut sink) in &mut q {
        // Each bed rides `ambience_vol`, scaled per kind: biome beds louder, water quieter.
        let (on, mult) = match amb.kind {
            AmbienceKind::Biome(b) => (Some(b) == cur, BIOME_AMBIENCE_MULT),
            AmbienceKind::Water => (water, WATER_AMBIENCE_MULT),
            AmbienceKind::Castle => (near_castle && day, 2.2), // town bustle — carry it over the din
            AmbienceKind::Meadow => (meadow, MEADOW_AMBIENCE_MULT),
        };
        let target = if on { cfg.ambience_vol * mult } else { 0.0 };
        amb.level += (target - amb.level) * k;
        sink.set_volume(Volume::Linear(amb.level));
    }
}

/// Ride the kids' play-chatter loop: swell it as the hero nears the nearest *visibly-playing*
/// child, duck it when he's right on top of them, and fall silent when no kid is visible — which
/// the night curfew gives for free (kids run home and hide, so the loop simply goes quiet at dusk).
pub(crate) fn kids_chatter(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    hero: Query<&crate::player::Hero>,
    kids: Query<(&GlobalTransform, &Visibility), With<crate::villagers::Kid>>,
    mut q: Query<(&mut KidsChatter, &mut AudioSink)>,
) {
    // Distance to the nearest visible kid (None → no kid out → silent).
    let nearest = hero.single().ok().map(|h| h.pos).and_then(|hp| {
        kids.iter()
            .filter(|(_, v)| **v != Visibility::Hidden)
            .map(|(t, _)| {
                let p = t.translation();
                Vec2::new(p.x, p.z).distance(hp)
            })
            .min_by(f32::total_cmp)
    });
    // Peak = 2/3 of a villager voice line (voice_vol × villager gain), ridden by the swell curve.
    let villager_gain = crate::audio::lines::speaker_voice(crate::audio::lines::Speaker::Villager).gain;
    let peak = cfg.voice_vol * villager_gain * KIDS_VS_VILLAGER;
    let now = time.elapsed_secs();
    let near = nearest.is_some_and(|d| d < KIDS_FAR);
    let k = (time.delta_secs() * KIDS_FADE).min(1.0);
    for (mut c, mut sink) in &mut q {
        // Intermittent envelope: only run the burst/gap clock while a kid is in earshot; when none
        // is, hold silent and re-arm a short beat so a line pipes up shortly after the hero walks up.
        if !near {
            c.talking = false;
            c.until = now + KIDS_ARRIVE_BEAT;
        } else if now >= c.until {
            c.talking = !c.talking;
            let (lo, hi) = if c.talking { KIDS_BURST } else { KIDS_GAP };
            c.until = now + lo + crate::audio::frand(&mut c.rng) * (hi - lo);
        }
        let env = if c.talking { 1.0 } else { 0.0 };
        let target = env * nearest.map_or(0.0, |d| peak * kids_swell(d));
        c.level += (target - c.level) * k;
        sink.set_volume(Volume::Linear(c.level));
    }
}
