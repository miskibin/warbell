//! The hero's *observational* voice — occasional in-character remarks on the world around him:
//! the townsfolk, the kids, a stray hound, the keep, the quiet before a wave, a fresh kill. A
//! companion to [`super::voice`]'s event lines, but **proximity / situation driven** and heavily
//! throttled so it stays flavour, never chatter.
//!
//! **Drop-in scaffold:** clips load from `assets/audio/vo/hero/<key>.ogg`. Until those files
//! exist the whole layer is inert — [`tick`] only plays (and only shows a subtitle) once a clip is
//! actually loaded, so there are no silent sinks or captions-without-voice. Add the oggs and it
//! goes live with no code change. The spoken text lives beside each key below (our record of every
//! quote, per the CLAUDE.md convention) and doubles as the on-screen subtitle.

use std::collections::HashMap;

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::critters::Species;

use super::{frand, AudioConfig, AudioCue};

/// Shortest gap between ANY two remarks; a random slice up to [`REMARK_JITTER`] is added so the
/// cadence is irregular.
const REMARK_GAP: f32 = 34.0;
const REMARK_JITTER: f32 = 16.0;
/// A given remark plays at most once per this window (variety without repetition).
const LINE_FLOOR: f32 = 300.0;
/// Hero must be within this (world units) of a thing for its proximity remark to fire.
const NEAR: f32 = 7.0;
/// "Quiet day" only fires in prep with no ork within this of the hero.
const QUIET_CLEAR: f32 = 28.0;
/// Delay after a run starts before the intro line plays (let the scene settle).
const INTRO_DELAY: f32 = 1.6;

/// What's prompting the hero to speak. Most are proximity; Night/Quiet are phase + clearance;
/// Kill is an event off the combat cue stream.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trig {
    Town,
    Kids,
    Pet,
    Guard,
    Keep,
    Night,
    Quiet,
    Kill,
}

/// `(trigger, clip key, spoken text)` — the key is the `vo/hero/<key>.ogg` filename; the text is
/// the subtitle + our record of the line. Two variants per trigger so a repeat isn't identical.
#[rustfmt::skip]
const REMARKS: &[(Trig, &str, &str)] = &[
    // ── Near townsfolk (folds in the well / market / woodpile / town / laughter / grumble lines) ──
    (Trig::Town, "people_a",   "These people. Loud, stubborn, alive. That's the whole point of all this, isn't it."),
    (Trig::Town, "people_b",   "Look at them — bickering, trading, breathing. That's what the wall is for."),
    (Trig::Town, "name_a",     "Half of them don't know my name. Good. Means they're free to forget the war."),
    (Trig::Town, "name_b",     "They nod and move on. Better that than knowing what's out past the gate."),
    (Trig::Town, "well_a",     "Fresh water, idle talk, small quarrels. The things we're actually fighting for."),
    (Trig::Town, "well_b",     "Gossip at the well. Sounds like nothing. Sounds like peace, is what it is."),
    (Trig::Town, "townday_a",  "A town that still argues over fences and taxes. Means there's still a town."),
    (Trig::Town, "townday_b",  "Still squabbling over hens and rent. The day that stops, we've lost."),
    (Trig::Town, "laugh_a",    "They laugh like there was never a siege. ...Maybe that's the victory."),
    (Trig::Town, "laugh_b",    "Laughter in the square, after all this. ...Maybe that's the whole point."),
    (Trig::Town, "market_a",   "Coin changes hands, bread gets baked, the world turns. I just keep the wolves off it."),
    (Trig::Town, "market_b",   "Buy, sell, haggle — honest work. I'd take it over mine most days."),
    (Trig::Town, "woodpile_a", "Stack it high. The nights are only getting longer."),
    (Trig::Town, "woodpile_b", "More wood. Good. Cold kills slower than orks — but it still kills."),
    (Trig::Town, "grumble_a",  "The taxes, aye. Tell it to the orks — they're wonderful listeners."),
    (Trig::Town, "grumble_b",  "Complain to me about rent. I'll forward it to the horde — they decide who pays."),
    // ── Near the kids ──
    (Trig::Kids, "kids_a",     "Mind those sticks, little ones. ...Gods, let them stay little ones a while longer."),
    (Trig::Kids, "kids_b",     "Run while you can, little ones. Wish I still had the knees for it."),
    // ── Near a dog / cat ──
    (Trig::Pet, "pet_a",       "At least the hound's got the right idea. Rest while the light holds."),
    (Trig::Pet, "pet_b",       "The cat fears nothing. Must be nice, being a cat."),
    // ── Near a guard / militia ──
    (Trig::Guard, "guard_a",   "Stand tall. The wall holds because you do."),
    (Trig::Guard, "guard_b",   "Eyes on the dark, soldier. I'll be right beside you when it comes."),
    // ── Inside the keep ──
    (Trig::Keep, "keep_a",     "Old stones. They've outlived better men than me. They'll outlive me too."),
    (Trig::Keep, "keep_b",     "This keep's swallowed a hundred sieges. One more won't choke it."),
    // ── Night / during a wave ──
    (Trig::Night, "night_a",   "Stars are out. Somewhere up there someone's keeping a tally. Hope I'm ahead."),
    (Trig::Night, "night_b",   "Clear night. Pretty — if you forget what comes with the dark."),
    // ── Prep, no enemies near ──
    (Trig::Quiet, "quiet_a",   "Quiet day. I've learned not to trust quiet days."),
    (Trig::Quiet, "quiet_b",   "Too calm. The quiet always sends a bill, sooner or later."),
    // ── After a kill ──
    (Trig::Kill, "kill_a",     "One more for the pile. I stopped counting around the second winter."),
    (Trig::Kill, "kill_b",     "Down. There's always another behind it. Always is."),
];

/// The once-per-run opening line (random variant) — the tutorial in the hero's own voice.
#[rustfmt::skip]
const INTRO: &[(&str, &str)] = &[
    ("intro_a", "Daylight's short — open the chests, gather coin and stone, buy what'll keep you breathing. When dark comes, the orks come for the keep. We hold it."),
    ("intro_b", "By day you scavenge — chests, ore, gold — and arm up at the War Table. By night the horde hits these walls. Keep the keep standing. Don't waste the light."),
];

#[derive(Component)]
pub(crate) struct HeroRemarkTag;

#[derive(Resource)]
pub(crate) struct RemarkBank(HashMap<&'static str, Handle<AudioSource>>);

#[derive(Resource)]
pub(crate) struct RemarkState {
    /// Earliest time the next remark may play (global throttle).
    next: f32,
    /// Per-line last-played time (the [`LINE_FLOOR`]).
    last: HashMap<&'static str, f32>,
    rng: u32,
    /// When the intro should play (set on the first frame of a run); `None` until armed.
    intro_at: Option<f32>,
    intro_done: bool,
}

impl Default for RemarkState {
    fn default() -> Self {
        Self { next: 0.0, last: HashMap::new(), rng: 0x6d2b_79f5, intro_at: None, intro_done: false }
    }
}

pub(crate) fn setup(asset: Res<AssetServer>, mut commands: Commands) {
    let mut m = HashMap::new();
    for &(_, key, _) in REMARKS {
        m.insert(key, asset.load(format!("audio/vo/hero/{key}.ogg")));
    }
    for &(key, _) in INTRO {
        m.insert(key, asset.load(format!("audio/vo/hero/{key}.ogg")));
    }
    commands.insert_resource(RemarkBank(m));
    commands.init_resource::<RemarkState>();
}

/// Fresh run: clear the throttle + re-arm the intro.
pub(crate) fn reset(mut st: ResMut<RemarkState>) {
    *st = RemarkState::default();
}

/// Play a hero-remark clip non-spatially (head-locked), one at a time. Returns false (plays
/// nothing) if the clip isn't loaded yet — which keeps the whole layer inert until the audio
/// files are dropped in.
fn play(
    commands: &mut Commands,
    existing: &Query<Entity, With<HeroRemarkTag>>,
    bank: &RemarkBank,
    sources: &Assets<AudioSource>,
    key: &str,
    vol: f32,
) -> bool {
    let Some(clip) = bank.0.get(key) else { return false };
    if sources.get(clip).is_none() {
        return false; // not loaded (no audio dropped in yet) → stay silent
    }
    for e in existing {
        commands.entity(e).try_despawn(); // one mouth: stop any prior remark
    }
    commands.spawn((
        AudioPlayer(clip.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            spatial: false,
            ..default()
        },
        HeroRemarkTag,
    ));
    true
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick(
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    sources: Res<Assets<AudioSource>>,
    mut commands: Commands,
    bank: Res<RemarkBank>,
    mut st: ResMut<RemarkState>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    mut cues: MessageReader<AudioCue>,
    existing: Query<Entity, With<HeroRemarkTag>>,
    hero: Query<&crate::player::Hero>,
    siege: Option<Res<crate::siege::Siege>>,
    townsfolk: Query<
        (&GlobalTransform, Has<crate::villagers::Kid>, Has<crate::villagers::Guard>),
        With<crate::villagers::Villager>,
    >,
    pets: Query<(&GlobalTransform, &crate::wildlife::Animal)>,
    orks: Query<&GlobalTransform, (With<crate::orks::Ork>, Without<crate::dying::Dying>)>,
) {
    let now = time.elapsed_secs();
    let Ok(hero) = hero.single() else { return };
    let hp = hero.pos;
    let vol = cfg.narration_vol;

    // Drain the cue stream every frame; note a kill if a connecting blow finished something.
    let mut killed = false;
    for c in cues.read() {
        if matches!(c, AudioCue::Impact { kill: true }) {
            killed = true;
        }
    }

    // ── Intro: once per run, a short beat after the scene comes up. ──
    if !st.intro_done {
        match st.intro_at {
            None => {
                st.intro_at = Some(now + INTRO_DELAY);
                return;
            }
            Some(t) if now < t => return,
            _ => {
                let i = (frand(&mut st.rng) * INTRO.len() as f32) as usize % INTRO.len();
                let (key, text) = INTRO[i];
                if play(&mut commands, &existing, &bank, &sources, key, vol) {
                    subs.say(now, text, crate::subtitles::read_secs(text));
                    st.intro_done = true;
                    st.next = now + REMARK_GAP + frand(&mut st.rng) * REMARK_JITTER;
                }
                return; // hold other remarks until the intro has had its turn
            }
        }
    }

    if now < st.next {
        return;
    }

    // Proximity flags (one pass over the townsfolk; kids/guards are villagers too).
    let dist_ok = |t: &GlobalTransform| {
        let p = t.translation();
        Vec2::new(p.x, p.z).distance(hp) <= NEAR
    };
    let (mut near_kids, mut near_guard, mut near_town) = (false, false, false);
    for (t, is_kid, is_guard) in &townsfolk {
        if dist_ok(t) {
            if is_kid {
                near_kids = true;
            } else if is_guard {
                near_guard = true;
            } else {
                near_town = true;
            }
        }
    }
    let near_pet = pets.iter().any(|(t, a)| matches!(a.species, Species::Dog | Species::Cat) && dist_ok(t));
    let in_keep = crate::castle::in_footprint(hp.x, hp.y);
    let phase = siege.as_ref().map(|s| s.phase);
    let orks_near = orks.iter().any(|t| {
        let p = t.translation();
        Vec2::new(p.x, p.z).distance(hp) <= QUIET_CLEAR
    });

    use crate::siege::GamePhase;
    let trig = if killed {
        Some(Trig::Kill)
    } else if near_kids {
        Some(Trig::Kids)
    } else if near_pet {
        Some(Trig::Pet)
    } else if near_guard {
        Some(Trig::Guard)
    } else if in_keep {
        Some(Trig::Keep)
    } else if near_town {
        Some(Trig::Town)
    } else if phase == Some(GamePhase::Wave) {
        Some(Trig::Night)
    } else if phase.map(|p| p == GamePhase::Prep).unwrap_or(true) && !orks_near {
        Some(Trig::Quiet)
    } else {
        None
    };
    let Some(trig) = trig else { return };

    // Pick an off-cooldown, loaded line from this trigger's pool.
    let pool: Vec<(&'static str, &'static str)> =
        REMARKS.iter().filter(|(t, _, _)| *t == trig).map(|(_, k, x)| (*k, *x)).collect();
    if pool.is_empty() {
        return;
    }
    for _ in 0..6 {
        let (key, text) = pool[(frand(&mut st.rng) * pool.len() as f32) as usize % pool.len()];
        if now - *st.last.get(key).unwrap_or(&-1000.0) < LINE_FLOOR {
            continue;
        }
        if play(&mut commands, &existing, &bank, &sources, key, vol) {
            st.last.insert(key, now);
            st.next = now + REMARK_GAP + frand(&mut st.rng) * REMARK_JITTER;
            subs.say(now, text, crate::subtitles::read_secs(text));
            return;
        }
    }
}
