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

use super::{frand, AudioConfig, AudioCue, HeroSpeaking, OthersSpeaking};

/// A given villager line plays at most once per this window (the user-requested "no line more
/// than once per 10 minutes" floor).
const LINE_FLOOR: f32 = 600.0;
/// Minimum gap between ANY two ambient (proximity) villager lines, so the town isn't a babble.
/// (Lowered 150 → 80 → 55 → 20: a livelier, chattier town. Still well above a line's length, so
/// two ambient lines never overlap; the per-line 10-min floor still prevents repeats.)
const AMBIENT_GAP: f32 = 20.0;
/// Chance a villager actually speaks once the gap clears and one's in range — high, so the town
/// feels chatty and alive. A miss burns a full gap; the 10-min per-line floor stops repeats.
const SPEAK_CHANCE: f32 = 0.9;
/// Two villagers within this (world units) won't talk at the same time — a new line is held while a
/// *nearby* one is still mid-sentence, so a crowd doesn't turn into a garbled babble.
const CLOSE_SPEAKER_DIST: f32 = 14.0;
/// Hero must be this close (world units) to a villager to trigger a proximity greeting/musing.
const NEAR_DIST: f32 = 7.0;
/// For an event line, the nearest villager must be within this of the hero to voice it.
const EVENT_NEAR: f32 = 55.0;
/// Spoken-line gain (spatial — the world→audio scale handles distance falloff). Bumped 0.9 → 1.4
/// so the townsfolk carry over the mix.
const NPC_GAIN: f32 = 1.4;

/// The proximity lines, by key (the key drives the per-line [`LINE_FLOOR`]). Aligned with
/// [`NpcVoiceBank::ambient`]. The first five are the original townsfolk greetings/musings; the
/// rest are the funny / passive-aggressive comments + short town stories, so the city feels like
/// a crowd of people, not one greeter.
///
/// **Spoken text of each line** (keep this in sync when clips change — it's our only record of
/// what the town actually says, so we can retune triggers without re-listening):
/// - `greet`        — "Oh, hello there, m'lord. Mind the mud."                                  (Ed)
/// - `greet_2`      — "Bless your night. We sleep easier when you're about."                    (Ed)
/// - `idle_hens`    — "I told the hens about the orks. They were not impressed."                (Ed)
/// - `idle_cousin`  — "Me cousin says he killed an ork once. Me cousin says a lotta things."     (Ed)
/// - `merchant`     — "Finest wares this side of the swamp. Only wares this side of the swamp,
///                     but still."                                                              (Ed)
/// - `pa_sword`     — "Look at the size of that sword. My uncle's is smaller, and he's twice the
///                     man."                                         (Ed; gated on a weapon equipped)
/// - `pa_hero`      — "Off to be a hero again, are we? Must be nice having the time."            (Ed)
/// - `pa_chosen`    — "Oh, the chosen one graces us. Mind you don't trip on all that destiny."   (Ed)
/// - `pa_slept`     — "Saved us all last night, did you? Funny, I slept fine without you."       (Ed)
/// - `pa_fence`     — "Big strong knight. Can't fix a fence, but big strong knight."             (Ed)
/// - `story_barn`   — "See ol' Marek's barn? Burned clean down. He says lightning. Was the ale." (Ed)
/// - `story_miller` — "The miller's daughter married a soldier. He left, she kept the goat. Smart
///                     girl."                                                                   (Ed)
/// - `story_witch`  — "They say the swamp witch grants wishes. They also say she eats fingers.
///                     I'll keep my fingers."                                                   (Ed)
/// - `story_gran`   — "We don't talk about grandfather."   (Ed; the "soup ladle" setup didn't render)
/// - `story_baker`  — "Heard the baker's getting rich. Heard it from the baker. So."             (Ed)
/// - `day_dream`    — "Another glorious day of standing exactly here. Living the dream."  (Professor)
/// - `chicken`      — "If one more chicken gets into the chapel, I'm converting."         (Professor)
/// - `taxes`        — "Taxes up, walls down, orks at the door. But sure, ring the bell. That'll
///                     help."                                                            (Professor)
/// - `trade`        — "I had a trade once, then the war. Now I... gesture vaguely... do this." (Professor)
/// - `grateful`     — "We're ever so grateful. Truly. Now could you... grateful your boots off my
///                     step."                                                            (Professor)
/// - `screaming`    — "Bless you for the protection. The screaming at night is a lovely touch." (Professor)
const AMBIENT_KEYS: [&str; 21] = [
    "greet",
    "greet_2",
    "idle_hens",
    "idle_cousin",
    "merchant",
    "pa_sword",
    "pa_hero",
    "pa_chosen",
    "pa_slept",
    "pa_fence",
    "story_barn",
    "story_miller",
    "story_witch",
    "story_gran",
    "story_baker",
    "day_dream",
    "chicken",
    "taxes",
    "trade",
    "grateful",
    "screaming",
];

/// Lines that only make sense when the hero is visibly armed — held out of the rotation until a
/// weapon is equipped, so the "look at the size of that sword" jab lands when there's a sword.
const NEEDS_WEAPON: [&str; 1] = ["pa_sword"];

/// Subtitle text for each ambient line, **aligned 1:1 with [`AMBIENT_KEYS`]** (so the on-screen
/// caption matches the clip). Keep in sync with the clips + the doc table above.
const AMBIENT_TEXT: [&str; 21] = [
    "Oh, hello there, m'lord. Mind the mud.",
    "Bless your night. We sleep easier when you're about.",
    "I told the hens about the orks. They were not impressed.",
    "Me cousin says he killed an ork once. Me cousin says a lotta things.",
    "Finest wares this side of the swamp. Only wares this side of the swamp, but still.",
    "Look at the size of that sword. My uncle's is smaller, and he's twice the man.",
    "Off to be a hero again, are we? Must be nice having the time.",
    "Oh, the chosen one graces us. Mind you don't trip on all that destiny.",
    "Saved us all last night, did you? Funny, I slept fine without you.",
    "Big strong knight. Can't fix a fence, but big strong knight.",
    "See ol' Marek's barn? Burned clean down. He says lightning. Was the ale.",
    "The miller's daughter married a soldier. He left, she kept the goat. Smart girl.",
    "They say the swamp witch grants wishes. They also say she eats fingers. I'll keep my fingers.",
    "We don't talk about grandfather.",
    "Heard the baker's getting rich. Heard it from the baker. So.",
    "Another glorious day of standing exactly here. Living the dream.",
    "If one more chicken gets into the chapel, I'm converting.",
    "Taxes up, walls down, orks at the door. But sure, ring the bell. That'll help.",
    "I had a trade once, then the war. Now I... do this.",
    "We're ever so grateful. Truly. Now could you grateful your boots off my step.",
    "Bless you for the protection. The screaming at night is a lovely touch.",
];

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
    /// When the most-recent villager line finishes, and where its speaker stands — shared by both
    /// voice systems so a new line near that spot is held until it ends (no overlapping crowds).
    voice_until: f32,
    voice_pos: Vec2,
    rng: u32,
    /// Shuffle-bag of ambient line indices: drawn down to empty, then reshuffled, so EVERY line
    /// is heard once before any repeats. Replaces the old random-pick-with-floor, which re-rolled
    /// the same handful (and went silent once the 10-min floor had locked everything).
    bag: Vec<usize>,
    /// Last index drawn — used to avoid an immediate repeat across a reshuffle.
    bag_last: Option<usize>,
}

impl Default for NpcVoiceState {
    fn default() -> Self {
        // Start a touch into the run so nobody greets you over the menu fade.
        Self {
            last: HashMap::new(),
            next_ambient: 25.0,
            voice_until: 0.0,
            voice_pos: Vec2::ZERO,
            rng: 0x1234_5678,
            bag: Vec::new(),
            bag_last: None,
        }
    }
}

pub(crate) fn setup_npc_voice(asset: Res<AssetServer>, mut commands: Commands) {
    let ambient = AMBIENT_KEYS.iter().map(|k| asset.load(format!("audio/vo/npc/{k}.ogg"))).collect();
    commands.insert_resource(NpcVoiceBank {
        ambient,
        // "They're coming. Inside, inside. Lock the door."   (fires on the Prep→Wave edge — dusk)
        siege_fear: asset.load("audio/vo/npc/siege_fear.ogg"),
        // "Made it to morning. Knew you'd see us through."   (fires on the Wave→Prep edge — dawn)
        dawn_relief: asset.load("audio/vo/npc/dawn_relief.ogg"),
        // "You came for me? Gods bless you. I'll take up a spear, I swear it."  (on a camp rescue)
        rescued: asset.load("audio/vo/npc/rescued.ogg"),
    });
    // Seed the chatter RNG from wall-clock entropy so the line order differs every run — a fixed
    // seed made each session replay the same opening handful ("the same 6 again and again").
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0x1234_5678)
        | 1;
    commands.insert_resource(NpcVoiceState { rng: seed, ..default() });
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

/// The villager nearest the hero within `max` (XZ distance), if any — with its world XZ position
/// (used for the no-overlap-when-close guard).
fn nearest_villager(
    hero: Vec2,
    villagers: &Query<(Entity, &GlobalTransform), With<Villager>>,
    max: f32,
) -> Option<(Entity, Vec2)> {
    let mut best: Option<(Entity, Vec2, f32)> = None;
    for (e, gt) in villagers {
        let t = gt.translation();
        let p = Vec2::new(t.x, t.z);
        let d = p.distance(hero);
        if d <= max && best.is_none_or(|(_, _, bd)| d < bd) {
            best = Some((e, p, d));
        }
    }
    best.map(|(e, p, _)| (e, p))
}

/// Occasional proximity chatter: when the hero lingers near a townsperson and the throttle has
/// cleared, the nearest one offers a greeting or an idle musing (each capped to once / 10 min).
pub(crate) fn npc_ambient(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    mut commands: Commands,
    bank: Res<NpcVoiceBank>,
    mut st: ResMut<NpcVoiceState>,
    speaking: Res<HeroSpeaking>,
    mut others: ResMut<OthersSpeaking>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    inv: Res<crate::inventory::Inventory>,
    hero: Query<&Hero>,
    villagers: Query<(Entity, &GlobalTransform), With<Villager>>,
) {
    let now = time.elapsed_secs();
    if now < st.next_ambient {
        return;
    }
    // Never chatter over the hero's own voice (one-mouth courtesy); retry once he's done.
    if now < speaking.until {
        return;
    }
    let Ok(hero) = hero.single() else { return };
    let Some((who, who_pos)) = nearest_villager(hero.pos, &villagers, NEAR_DIST) else { return };
    // Don't talk over a nearby villager who's still mid-line — retry next frame once they finish.
    if now < st.voice_until && who_pos.distance(st.voice_pos) < CLOSE_SPEAKER_DIST {
        return;
    }
    // Mostly stay quiet even when eligible — a miss waits a full gap, not an instant retry.
    if frand(&mut st.rng) >= SPEAK_CHANCE {
        st.next_ambient = now + AMBIENT_GAP;
        return;
    }
    let armed = inv.0.weapon_bonus() > 0.0;
    // Draw the next line from the shuffle-bag: every line plays once before any repeat, in a fresh
    // random order each cycle. The sword jab is held back (skipped this cycle) until a weapon's on.
    let mut chosen = None;
    for _ in 0..=AMBIENT_KEYS.len() {
        if st.bag.is_empty() {
            refill_bag(&mut st);
        }
        let i = st.bag.pop().unwrap();
        if !armed && NEEDS_WEAPON.contains(&AMBIENT_KEYS[i]) {
            continue; // returns to the rotation on the next reshuffle, once there's a sword to mock
        }
        chosen = Some(i);
        break;
    }
    let Some(i) = chosen else { return };
    let dur = crate::subtitles::read_secs(AMBIENT_TEXT[i]);
    st.bag_last = Some(i);
    st.last.insert(AMBIENT_KEYS[i], now);
    st.next_ambient = now + AMBIENT_GAP;
    st.voice_until = now + dur;
    st.voice_pos = who_pos;
    others.until = now + dur; // hero holds his commentary while this villager speaks
    say_from(&mut commands, who, bank.ambient[i].clone(), NPC_GAIN * cfg.voice_vol);
    subs.say(now, AMBIENT_TEXT[i], dur);
}

/// Refill the ambient shuffle-bag with every line index in a fresh random order (Fisher–Yates via
/// the voice RNG), avoiding an immediate repeat of the last line played across the reshuffle.
fn refill_bag(st: &mut NpcVoiceState) {
    let last = st.bag_last;
    st.bag = (0..AMBIENT_KEYS.len()).collect();
    for i in (1..st.bag.len()).rev() {
        let j = (frand(&mut st.rng) * (i as f32 + 1.0)) as usize % (i + 1);
        st.bag.swap(i, j);
    }
    // The bag is drawn from the END; if that next pick equals the last line played, swap it deeper.
    if st.bag.len() > 1 && st.bag.last().copied() == last {
        let n = st.bag.len();
        st.bag.swap(n - 1, 0);
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
    speaking: Res<HeroSpeaking>,
    mut others: ResMut<OthersSpeaking>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    hero: Query<&Hero>,
    villagers: Query<(Entity, &GlobalTransform), With<Villager>>,
    siege: Option<Res<crate::siege::Siege>>,
    mut cues: MessageReader<AudioCue>,
    mut prev_phase: Local<Option<crate::siege::GamePhase>>,
) {
    use crate::siege::GamePhase;
    let now = time.elapsed_secs();
    // (key, subtitle text, clip)
    let mut chosen: Option<(&'static str, &'static str, Handle<AudioSource>)> = None;
    if let Some(siege) = &siege {
        let phase = siege.phase;
        if let Some(prev) = *prev_phase {
            if prev == GamePhase::Prep && phase == GamePhase::Wave {
                chosen =
                    Some(("siege_fear", "They're coming. Inside, inside. Lock the door.", bank.siege_fear.clone()));
            } else if prev == GamePhase::Wave && phase == GamePhase::Prep {
                chosen =
                    Some(("dawn_relief", "Made it to morning. Knew you'd see us through.", bank.dawn_relief.clone()));
            }
        }
        *prev_phase = Some(phase);
    }
    // Always drain the cue stream; a rescue (rare) trumps a phase line if both land this frame.
    for c in cues.read() {
        if matches!(c, AudioCue::CampRescue) {
            chosen = Some((
                "rescued",
                "You came for me? Gods bless you. I'll take up a spear, I swear it.",
                bank.rescued.clone(),
            ));
        }
    }
    let Some((key, text, clip)) = chosen else { return };
    // One-mouth courtesy: don't speak over the hero. On a rescue he fires his own `FirstRescue`
    // reaction the same frame (and `npc_events` runs after his voice), so he claims the first
    // rescue and the freed villager pipes up on later ones — "sometimes me, sometimes them".
    if now < speaking.until {
        return;
    }
    if now - *st.last.get(key).unwrap_or(&-1000.0) < LINE_FLOOR {
        return;
    }
    let Ok(hero) = hero.single() else { return };
    let Some((who, who_pos)) = nearest_villager(hero.pos, &villagers, EVENT_NEAR) else { return };
    // Don't overlap a nearby villager who's still mid-line.
    if now < st.voice_until && who_pos.distance(st.voice_pos) < CLOSE_SPEAKER_DIST {
        return;
    }
    let dur = crate::subtitles::read_secs(text);
    st.last.insert(key, now);
    st.voice_until = now + dur;
    st.voice_pos = who_pos;
    others.until = now + dur; // hero holds his commentary while this villager speaks
    say_from(&mut commands, who, clip, NPC_GAIN * cfg.voice_vol);
    subs.say(now, text, dur);
}
