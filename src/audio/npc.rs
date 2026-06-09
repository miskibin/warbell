//! Villager / townsfolk voices — the world's flavour chatter. Spatial lines that come from the
//! nearest townsperson: occasional greetings + idle musings when the hero passes by, plus a few
//! event reactions (night falling, dawn breaking, a rescue). Deliberately RARE — a global
//! throttle ([`AMBIENT_GAP`]) plus a 10-minute per-line floor ([`LINE_FLOOR`]) keep them an
//! occasional spice, never a chatter-box. Clips live in `assets/audio/vo/npc/`.

use std::collections::HashMap;

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::player::Hero;
use crate::villagers::Villager;

use super::{frand, AudioConfig, AudioCue};

/// A given villager line plays at most once per this window (the user-requested "no line more
/// than once per 10 minutes" floor).
const LINE_FLOOR: f32 = 600.0;
/// Minimum gap between ANY two ambient (proximity) villager lines, so the town isn't a babble.
const AMBIENT_GAP: f32 = 150.0;
/// Hero must be this close (world units) to a villager to trigger a proximity greeting/musing.
const NEAR_DIST: f32 = 7.0;
/// For an event line, the nearest villager must be within this of the hero to voice it.
const EVENT_NEAR: f32 = 55.0;
/// Spoken-line gain (spatial — the world→audio scale handles distance falloff).
const NPC_GAIN: f32 = 0.9;

/// The five proximity lines, by key (the key drives the per-line [`LINE_FLOOR`]). Aligned with
/// [`NpcVoiceBank::ambient`].
const AMBIENT_KEYS: [&str; 5] = ["greet", "greet_2", "idle_hens", "idle_cousin", "merchant"];

#[derive(Resource)]
pub(crate) struct NpcVoiceBank {
    /// Proximity lines, aligned with [`AMBIENT_KEYS`].
    ambient: Vec<Handle<AudioSource>>,
    siege_fear: Handle<AudioSource>,
    dawn_relief: Handle<AudioSource>,
    rescued: Handle<AudioSource>,
}

#[derive(Resource)]
pub(crate) struct NpcVoiceState {
    /// Per-line last-played time (the 10-minute floor), keyed by the line's name.
    last: HashMap<&'static str, f32>,
    /// Earliest time the next ambient line may play (the global throttle).
    next_ambient: f32,
    rng: u32,
}

impl Default for NpcVoiceState {
    fn default() -> Self {
        // Start a touch into the run so nobody greets you over the menu fade.
        Self { last: HashMap::new(), next_ambient: 25.0, rng: 0x1234_5678 }
    }
}

pub(crate) fn setup_npc_voice(asset: Res<AssetServer>, mut commands: Commands) {
    let ambient = AMBIENT_KEYS.iter().map(|k| asset.load(format!("audio/vo/npc/{k}.ogg"))).collect();
    commands.insert_resource(NpcVoiceBank {
        ambient,
        siege_fear: asset.load("audio/vo/npc/siege_fear.ogg"),
        dawn_relief: asset.load("audio/vo/npc/dawn_relief.ogg"),
        rescued: asset.load("audio/vo/npc/rescued.ogg"),
    });
    commands.init_resource::<NpcVoiceState>();
}

/// Spawn a spoken line as a spatial child of a villager, so it sounds from where they stand and
/// cleans itself up when the clip ends.
fn say_from(commands: &mut Commands, who: Entity, clip: Handle<AudioSource>, vol: f32) {
    commands.entity(who).with_children(|p| {
        p.spawn((
            AudioPlayer(clip),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(vol),
                spatial: true,
                ..default()
            },
            Transform::default(),
        ));
    });
}

/// The villager nearest the hero within `max` (XZ distance), if any.
fn nearest_villager(
    hero: Vec2,
    villagers: &Query<(Entity, &GlobalTransform), With<Villager>>,
    max: f32,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (e, gt) in villagers {
        let t = gt.translation();
        let d = Vec2::new(t.x, t.z).distance(hero);
        if d <= max && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((e, d));
        }
    }
    best.map(|(e, _)| e)
}

/// Occasional proximity chatter: when the hero lingers near a townsperson and the throttle has
/// cleared, the nearest one offers a greeting or an idle musing (each capped to once / 10 min).
pub(crate) fn npc_ambient(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    bank: Res<NpcVoiceBank>,
    mut st: ResMut<NpcVoiceState>,
    hero: Query<&Hero>,
    villagers: Query<(Entity, &GlobalTransform), With<Villager>>,
) {
    let now = time.elapsed_secs();
    if now < st.next_ambient {
        return;
    }
    let Ok(hero) = hero.single() else { return };
    let Some(who) = nearest_villager(hero.pos, &villagers, NEAR_DIST) else { return };
    // Try a few random picks; play the first line that's off its 10-min floor, else stay quiet.
    for _ in 0..6 {
        let i = (frand(&mut st.rng) * AMBIENT_KEYS.len() as f32) as usize % AMBIENT_KEYS.len();
        let key = AMBIENT_KEYS[i];
        if now - *st.last.get(key).unwrap_or(&-1000.0) >= LINE_FLOOR {
            st.last.insert(key, now);
            st.next_ambient = now + AMBIENT_GAP;
            say_from(&mut commands, who, bank.ambient[i].clone(), NPC_GAIN * cfg.voice_vol);
            return;
        }
    }
}

/// Event reactions from a nearby townsperson: a panicked cry as night falls, relief at dawn, and
/// gratitude when a captive is freed. Each obeys the same 10-min per-line floor.
pub(crate) fn npc_events(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    bank: Res<NpcVoiceBank>,
    mut st: ResMut<NpcVoiceState>,
    hero: Query<&Hero>,
    villagers: Query<(Entity, &GlobalTransform), With<Villager>>,
    siege: Option<Res<crate::siege::Siege>>,
    mut cues: MessageReader<AudioCue>,
    mut prev_phase: Local<Option<crate::siege::GamePhase>>,
) {
    use crate::siege::GamePhase;
    let now = time.elapsed_secs();
    let mut chosen: Option<(&'static str, Handle<AudioSource>)> = None;
    if let Some(siege) = &siege {
        let phase = siege.phase;
        if let Some(prev) = *prev_phase {
            if prev == GamePhase::Prep && phase == GamePhase::Wave {
                chosen = Some(("siege_fear", bank.siege_fear.clone()));
            } else if prev == GamePhase::Wave && phase == GamePhase::Prep {
                chosen = Some(("dawn_relief", bank.dawn_relief.clone()));
            }
        }
        *prev_phase = Some(phase);
    }
    // Always drain the cue stream; a rescue (rare) trumps a phase line if both land this frame.
    for c in cues.read() {
        if matches!(c, AudioCue::CampRescue) {
            chosen = Some(("rescued", bank.rescued.clone()));
        }
    }
    let Some((key, clip)) = chosen else { return };
    if now - *st.last.get(key).unwrap_or(&-1000.0) < LINE_FLOOR {
        return;
    }
    let Ok(hero) = hero.single() else { return };
    let Some(who) = nearest_villager(hero.pos, &villagers, EVENT_NEAR) else { return };
    st.last.insert(key, now);
    say_from(&mut commands, who, clip, NPC_GAIN * cfg.voice_vol);
}
