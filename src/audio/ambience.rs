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

/// Campfire loop level at the source (spatial falloff handles distance from there).
const CAMPFIRE_VOL: f32 = 0.5;

/// What makes an ambience loop fade in.
#[derive(Clone, Copy, PartialEq)]
enum AmbienceKind {
    /// Plays while you're in this biome.
    Biome(Biome),
    /// Plays while the camera is over / near water — a river band in any view, or the open
    /// sea / coast (off the island) on the combined world map.
    Water,
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

/// Spawn the always-on (initially silent) ambience loops once at startup. Not tagged
/// `BiomeEntity`, so they survive biome switches; only their volume changes.
pub(crate) fn setup_ambience(asset: Res<AssetServer>, mut commands: Commands) {
    let beds = [
        (AmbienceKind::Biome(Biome::Snow), "audio/wind.ogg"),
        (AmbienceKind::Biome(Biome::Forest), "audio/forest-ambient.ogg"),
        (AmbienceKind::Biome(Biome::Desert), "audio/desert-wind.ogg"),
        (AmbienceKind::Biome(Biome::Swamp), "audio/swamp-ambient.ogg"),
        (AmbienceKind::Water, "audio/water.ogg"),
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
        commands.entity(e).insert(CampfireAudio).with_children(|p| {
            p.spawn((
                AudioPlayer(asset.load::<AudioSource>("audio/campfire-loop.ogg")),
                PlaybackSettings {
                    mode: PlaybackMode::Loop,
                    volume: Volume::Linear(CAMPFIRE_VOL),
                    spatial: true,
                    ..default()
                },
                Transform::default(),
            ));
        });
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
    let k = (dt * AMBIENCE_FADE).min(1.0);
    for (mut amb, mut sink) in &mut q {
        let on = match amb.kind {
            AmbienceKind::Biome(b) => Some(b) == cur,
            AmbienceKind::Water => water,
        };
        let target = if on { cfg.ambience_vol } else { 0.0 };
        amb.level += (target - amb.level) * k;
        sink.set_volume(Volume::Linear(amb.level));
    }
}
