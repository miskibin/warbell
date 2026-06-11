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
    let near_castle = cam_xz.is_some_and(|p| p.length() < CASTLE_AMBIENCE_R);
    let k = (dt * AMBIENCE_FADE).min(1.0);
    for (mut amb, mut sink) in &mut q {
        // Each bed rides `ambience_vol`, scaled per kind: biome beds louder, water quieter.
        let (on, mult) = match amb.kind {
            AmbienceKind::Biome(b) => (Some(b) == cur, BIOME_AMBIENCE_MULT),
            AmbienceKind::Water => (water, WATER_AMBIENCE_MULT),
            AmbienceKind::Castle => (near_castle && day, 1.0),
        };
        let target = if on { cfg.ambience_vol * mult } else { 0.0 };
        amb.level += (target - amb.level) * k;
        sink.set_volume(Volume::Linear(amb.level));
    }
}
