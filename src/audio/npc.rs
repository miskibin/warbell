//! Villager / townsfolk voice TRIGGERS — detects situations and emits [`super::director::Speak`]
//! events. The catalog (lines, text, clip IDs, replay floors) now lives in `audio/lines.rs`; the
//! director (`audio/director.rs`) owns playback, subtitle, and the one-mouth-per-speaker gate.
//! This module only decides WHEN a villager should speak, not WHAT or HOW.
//!
//! Behavioral note: the director enforces strictly one villager voice globally at a time (barge-in
//! by priority). The old model allowed one voice per cluster (no two villagers within
//! `CLOSE_SPEAKER_DIST` simultaneously). This is an accepted simplification — the director is
//! the single source of truth now.

use bevy::prelude::*;

use crate::player::Hero;
use crate::villagers::{Townsfolk, Villager};

use super::director::{Speak, VoiceManager};
use super::frand;
use super::lines::Concept;

/// Minimum gap between ANY two ambient (proximity) villager lines, so the town isn't a babble.
/// Lowered from 20→14 so townsfolk speak a little more often — half the catalog was going
/// unheard, partly because guards never chattered (see [`detect_villager_ambient`]).
const AMBIENT_GAP: f32 = 14.0;
/// Chance a villager actually speaks once the gap clears and one's in range — high, so the town
/// feels chatty and alive. A miss burns a full gap; the per-line floor stops repeats.
const SPEAK_CHANCE: f32 = 0.9;
/// Hero must be this close (world units) to a villager to trigger a proximity greeting/musing.
const NEAR_DIST: f32 = 7.0;
/// For an event line, the nearest villager must be within this of the hero to voice it.
const EVENT_NEAR: f32 = 55.0;
/// Fraction of ambient turns where the armed-sword jab fires (when the hero has a weapon equipped).
/// ~1/20 of the rotation — the jab is rare, landing as a pleasant surprise when there's a sword to mock.
const ARMED_JAB_CHANCE: f32 = 0.05;

/// Trigger-state resource: replaces `NpcVoiceState`'s cadence clock and rng.
/// The per-line replay floors, shuffle bag, and one-mouth bookkeeping are all gone — they live in
/// the catalog (`Line::floor`) and in `VoiceManager` respectively.
#[derive(Resource)]
pub(crate) struct VillagerTrigger {
    /// Earliest time the next ambient line may be emitted (global throttle).
    pub(crate) next_ambient: f32,
    /// Phase-edge tracker for event lines (was a `Local` in the old `npc_events`).
    pub(crate) prev_phase: Option<crate::siege::GamePhase>,
    pub(crate) rng: u32,
}

impl Default for VillagerTrigger {
    fn default() -> Self {
        Self { next_ambient: 25.0, prev_phase: None, rng: 0x1234_5678 }
    }
}

/// Insert a `VillagerTrigger` with the RNG seeded from wall-clock entropy, so line order
/// differs every run (a fixed seed replays the same opening handful — same problem `npc.rs` solved
/// with the shuffle bag before).
pub(crate) fn setup_villager_trigger(mut commands: Commands) {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0x1234_5678)
        | 1;
    commands.insert_resource(VillagerTrigger { rng: seed, ..default() });
}

/// Fresh run: re-arm the trigger cadence (preserve the seeded rng so the line order doesn't
/// reset to the same position it was at run-start — avoids replaying the same opener).
pub(crate) fn reset_villager_trigger(mut t: ResMut<VillagerTrigger>) {
    let rng = t.rng;
    *t = VillagerTrigger { rng, ..default() };
}

/// The villager nearest the hero within `max` (XZ distance), if any. Returns `(Entity, Vec3)` —
/// the Vec3 is the world-space translation so callers can pass it to `Speak::at`. Generic over the
/// query filter so callers can narrow to workers or all villagers.
fn nearest_villager<F: bevy::ecs::query::QueryFilter>(
    hero: Vec2,
    villagers: &Query<(Entity, &GlobalTransform), F>,
    max: f32,
) -> Option<(Entity, Vec3)> {
    let mut best: Option<(Entity, Vec3, f32)> = None;
    for (e, gt) in villagers {
        let t = gt.translation();
        let p = Vec2::new(t.x, t.z);
        let d = p.distance(hero);
        if d <= max && best.is_none_or(|(_, _, bd)| d < bd) {
            best = Some((e, t, d));
        }
    }
    best.map(|(e, t, _)| (e, t))
}

/// Occasional proximity chatter: when the hero lingers near a townsperson and the throttle has
/// cleared, emit a `Speak` for a `Greeting` (or `VillagerArmedJab` when armed). The pool is the
/// whole `Townsfolk` militia — **guards as well as workers** — so the chatter doesn't dry up at
/// night (when every worker is mustered to a guard post) or near a posted guard by day. Before,
/// only `Worker`s spoke, so half the catalog went unheard. The director enforces one mouth at a
/// time and selects the actual clip from the catalog.
pub(crate) fn detect_villager_ambient(
    time: Res<Time>,
    mut t: ResMut<VillagerTrigger>,
    mgr: Res<VoiceManager>,
    inv: Res<crate::inventory::Inventory>,
    hero: Query<&Hero>,
    townsfolk: Query<(Entity, &GlobalTransform), (With<Villager>, With<Townsfolk>)>,
    mut speak: MessageWriter<Speak>,
) {
    let now = time.elapsed_secs();
    if now < t.next_ambient {
        return;
    }
    // Never chatter over the hero's own voice — one-mouth courtesy; retry next gap.
    if mgr.hero_speaking(now) {
        return;
    }
    let Ok(hero) = hero.single() else { return };
    let Some((_who, pos)) = nearest_villager(hero.pos, &townsfolk, NEAR_DIST) else { return };
    // Mostly stay quiet even when eligible — a miss burns a full gap, not an instant retry.
    if frand(&mut t.rng) >= SPEAK_CHANCE {
        t.next_ambient = now + AMBIENT_GAP;
        return;
    }
    // Weapon-gated jab: ~1-in-20 ambient turns when armed emit the sword comment.
    let armed = inv.0.weapon_bonus() > 0.0;
    let concept = if armed && frand(&mut t.rng) < ARMED_JAB_CHANCE {
        Concept::VillagerArmedJab
    } else {
        Concept::Greeting
    };
    speak.write(Speak::at(concept, pos));
    t.next_ambient = now + AMBIENT_GAP;
}

/// Event reactions from a nearby townsperson: a panicked cry as night falls, relief at dawn, and
/// gratitude when a captive is freed. The per-line 10-min floor and "must finish" are now catalog
/// data (`Line::floor = 600`, `Line::interruptible = false`) — no bookkeeping needed here.
pub(crate) fn detect_villager_events(
    time: Res<Time>,
    mut t: ResMut<VillagerTrigger>,
    mgr: Res<VoiceManager>,
    hero: Query<&Hero>,
    // The rival's desert garrison + workers carry `Villager` (for animation) but are NOT our
    // townsfolk — they must never voice the player's villager lines (dawn relief, etc.).
    villagers: Query<
        (Entity, &GlobalTransform),
        (With<Villager>, Without<crate::rival::RivalSoldier>, Without<crate::rival::RivalWorker>),
    >,
    siege: Option<Res<crate::siege::Siege>>,
    mut cues: MessageReader<super::AudioCue>,
    mut speak: MessageWriter<Speak>,
) {
    use crate::siege::GamePhase;
    let now = time.elapsed_secs();
    let mut concept: Option<Concept> = None;
    // A rescue line speaks from the CAGE (the freed captive's own mouth), not from whichever
    // townsperson happens to stand near the hero — camps are deep in the wilderness, where the
    // old nearest-villager rule usually found nobody and the line was silently dropped.
    let mut rescue_at: Option<Vec3> = None;

    if let Some(siege) = &siege {
        let phase = siege.phase;
        if let Some(prev) = t.prev_phase {
            if prev == GamePhase::Prep && phase == GamePhase::Wave {
                concept = Some(Concept::SiegeFalls);
            } else if prev == GamePhase::Wave && phase == GamePhase::Prep {
                concept = Some(Concept::Dawn);
            }
        }
        t.prev_phase = Some(phase);
    }

    // Always drain the cue stream; a rescue (rare) trumps a phase line if both land this frame.
    for c in cues.read() {
        if let super::AudioCue::CampRescue(at) = c {
            concept = Some(Concept::Rescued);
            rescue_at = Some(*at);
        }
    }

    let Some(c) = concept else { return };
    // One-mouth courtesy: don't speak over the hero. On a rescue, the hero fires his own
    // `FirstRescue` reaction the same frame — he claims it, the villager reacts on later ones.
    if mgr.hero_speaking(now) {
        return;
    }
    if let Some(at) = rescue_at {
        speak.write(Speak::at(c, at)); // the freed captive thanks you from the cage
        return;
    }
    let Ok(hero) = hero.single() else { return };
    // Any villager within range (not just workers — event lines from any townsfolk).
    let Some((_who, pos)) = nearest_villager(hero.pos, &villagers, EVENT_NEAR) else { return };
    speak.write(Speak::at(c, pos));
}
