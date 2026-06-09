//! Game audio — **event-driven**. Gameplay systems emit [`AudioCue`]s (and set [`MusicState`]);
//! this module owns every audio sink. Ported from the old web game's `audio.ts` / `sfx.ts`
//! central registry, split into focused submodules so each file does one job:
//!
//! - [`sfx`] — one-shot combat/UI/footstep stings + spatial creature voices
//! - [`voice`] — the hero's "one-mouth" voice (swing grunt / hurt / death / biome line)
//! - [`music`] — background bed + a combat layer that swells while orks fight the hero
//! - [`ambience`] — biome / water loops + a spatial campfire loop at each camp
//! - [`footsteps`] — per-step stings tied to the hero's gait + ground surface
//! - [`animals`] — the proximity-gated spatial wildlife calls (this game's own clips)
//!
//! Decoupling rule: gameplay code NEVER spawns a sink — it writes an [`AudioCue`] (or flips a
//! flag on [`MusicState`]). Only this module reads them and plays sound.

mod ambience;
mod animals;
pub(crate) mod director;
mod footsteps;
pub(crate) mod lines;
mod music;
mod npc;
mod ork;
mod sfx;
pub(crate) mod synth;
mod voice;

pub(crate) use lines::{Concept, Speaker};
pub(crate) use director::Speak;

use bevy::audio::Volume;
use bevy::prelude::*;

use crate::biome::Biome;

/// Ground surface under the hero — selects which footstep clip plays.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Dirt,
    Snow,
    Stone,
}


/// A one-shot audio request. Gameplay writes these via `MessageWriter<AudioCue>`; [`sfx`] and
/// [`voice`] each read the whole stream and handle their own subset.
#[derive(Message, Clone, Copy)]
pub enum AudioCue {
    // ── Combat / UI feedback (non-spatial one-shots, handled by `sfx`) ──
    /// Empty-swing whoosh — fired ONLY on a whiff (a connecting hit plays `Impact` instead,
    /// never both — matches `Character.tsx`'s deferred-whoosh logic).
    Swing,
    /// Hero's blow lands → impact (heavier on a kill).
    Impact { kill: bool },
    /// A hit was actually absorbed by the raised shield (wood/steel knock).
    Block,
    /// One footstep; `landing` = the louder touchdown step after a jump/fall.
    Footstep { surface: Surface, landing: bool },
    /// UI confirm blip (biome switch).
    UiSelect,
    // ── Hero "mouth" (head-locked, one at a time, handled by `voice`) ──
    /// Exertion grunt layered over a swing — voice rolls a 34% chance + the canGrunt gate.
    HeroGruntSwing,
    /// Effort grunt on a jump — voice rolls a 40% chance + the canGrunt gate (no other jump sfx).
    HeroJump,
    /// Pain cry on a non-fatal hit.
    HeroHurt,
    /// Death scream on the killing blow (interrupts any line).
    HeroDeath,
    // ── Spatial creature voices (handled by `sfx`, positioned in the world) ──
    /// Ork aggro grunt at a world position.
    OrkGrunt(Vec3),
    /// Warband alert roar at a world position (hero enters a camp clearing).
    OrkRoar(Vec3),
    /// A wild predator's snarl as it bites the hero, at a world position. `big` = a heavy beast
    /// (bear/croc/golem) → a deeper, louder roar. Pitch-jittered so a flurry never repeats.
    CreatureBite { at: Vec3, big: bool },
    /// A town-guard's melee strike on an invader, at a world position — a quiet spatial swing+thud
    /// so the player *hears* the militia fighting nearby. Emitted only for skirmishes close to the
    /// hero (a small earshot) so the whole battlefield doesn't clatter at once.
    GuardStrike(Vec3),
    /// One metallic chip on a pick-swing against an ore boulder (sampled `var-1`/`var-3`
    /// clips, pitch-jittered). Distinct from the `OreShatter` synth sting on the breaking blow.
    OreChip,
    /// A single axe chop landing on a tree (sampled `chop-wood.ogg`, pitch-jittered). Emitted
    /// once per swing that strikes any choppable tree (`verbs::chop_tree`).
    WoodChop,
    // ── Procedural stings (synth-baked, handled by `sfx` via [`synth::StingBank`]) ──
    /// Ore boulder shattered.
    OreShatter,
    /// Chest lid opened.
    ChestOpen,
    /// Herb / loot picked up.
    Forage,
    /// Hero gained a level.
    LevelUp,
    /// Gold collected.
    Gold,
    /// Shop purchase confirmed.
    ShopBuy,
    /// Night wave summoned (war bell).
    WarBell,
    /// Camp captive freed.
    CampRescue,
    /// Hero HP crossed the low threshold.
    LowHp,
}

/// Per-run gates for the "once" event voice lines (the old game's `spoken` key-set). Reset on
/// a fresh run. `been_away` latches the home-return line: it can only fire after the hero has
/// roamed past [`AWAY_RADIUS`] from the castle.
#[derive(Resource, Default)]
pub(crate) struct HeroLineGates {
    pub home: bool,
    pub been_away: bool,
    /// Once-per-run gates for the new spoken reactions (the rest are repeatable / floor-capped).
    pub equip: bool,
    pub first_kill: bool,
    pub gold_rich: bool,
    // `spoken_biomes` removed — biome dedup is now the catalog `once` flag (director tracks it).
}

/// Castle is at the world origin; the hero must roam beyond this, then return inside
/// [`HOME_RADIUS`], to trigger the home-return line.
const AWAY_RADIUS: f32 = 34.0;
const HOME_RADIUS: f32 = 16.0;

/// Combat-music driver. Ork AI sets `fighting = true` while any ork hunts / strikes the hero;
/// [`music`] eases the combat layer in/out from it.
#[derive(Resource, Default)]
pub struct MusicState {
    pub fighting: bool,
}

/// Shared **hero-line spacing** for the hero's spoken voice. EVERY spoken hero LINE — the catalog's
/// event reactions, biome musings, AND observational remarks (now all routed through `director`) —
/// stamps `until = now + HERO_LINE_CD` and the line's `priority` when it starts, and the director
/// refuses to begin a new hero line while `now < until` **unless** the newcomer strictly out-ranks
/// the line that opened the window (so urgent warnings — night falling, the keep under attack —
/// still cut through ~20 s of idle chatter). A line that wanted to fire inside the window is simply
/// dropped (never queued — "consider it played"); it re-fires next frame if its trigger persists.
/// Short combat exertions (swing/jump/hurt grunts) are exempt; the death cry spends the window.
#[derive(Resource, Default)]
pub(crate) struct HeroLineCooldown {
    pub until: f32,
    /// Priority of the line that opened the current window — a newcomer must exceed it to bypass.
    pub priority: u8,
}
/// Length of [`HeroLineCooldown`] (seconds) — per user: ~20 s between hero lines.
pub(crate) const HERO_LINE_CD: f32 = 20.0;

/// Estimated end time of the hero's CURRENTLY-PLAYING line (≈ clip length). Distinct from the 20 s
/// [`HeroLineCooldown`]: this tracks only the few seconds a clip actually sounds, so villagers +
/// orks (and the finish-grace check) know when he's mid-sentence and stay off him.
#[derive(Resource, Default)]
pub(crate) struct HeroSpeaking {
    pub until: f32,
}

/// Mirror of [`HeroSpeaking`]: estimated end time of a NON-hero spoken line currently sounding —
/// a villager's chatter or an ork's battle bark. The hero's spoken lines (`voice.rs` biome/event
/// musings + `hero_remarks.rs` observations) defer while `now < until`, so he never talks over the
/// townsfolk or the horde commenting on guards/town/etc. His combat exertion grunts + death cry are
/// exempt (reflex, not commentary).
#[derive(Resource, Default)]
pub(crate) struct OthersSpeaking {
    pub until: f32,
}

/// Tags every hero-mouth sink — a `voice` line OR a `hero_remarks` remark — so the place/biome
/// auto-stop can find whichever one is playing and fade it.
#[derive(Component)]
pub(crate) struct HeroMouthTag;

/// A **place-bound** hero line cut short if he wanders off from what prompted it: a proximity
/// remark anchors to where he stood; a biome musing anchors to the biome. Lines without it play out.
#[derive(Component, Clone, Copy)]
pub(crate) enum HeroLineAnchor {
    /// Cut once the hero is more than `r` (world units, xz) from `pos`.
    Near { pos: Vec2, r: f32 },
    /// Cut once the hero is no longer standing in this biome.
    Biome(Biome),
}

/// Marks a displaced hero line so it **fades** out over a few frames instead of clicking off
/// mid-word — a trailing-off, not a hard cut.
#[derive(Component)]
pub(crate) struct HeroLineFadeOut;

/// When a displaced line is within this much of its estimated end, let him finish the sentence
/// rather than cutting it — a near-over line isn't worth interrupting.
const FINISH_GRACE: f32 = 1.2;

/// Live-tunable mix (F1 debug panel). The wildlife knobs are unchanged from the original
/// `audio.rs`; the rest scale their category at the point of playback.
#[derive(Resource)]
pub struct AudioConfig {
    /// Target volume of a biome/water ambience bed while you're inside it (quiet bed).
    pub ambience_vol: f32,
    /// Camera→animal distance beyond which a wildlife call is not emitted.
    pub audible_range: f32,
    /// Min / max seconds between one animal's ambient calls.
    pub call_min: f32,
    pub call_max: f32,
    /// Master for combat/UI/footstep one-shots — matches the old game's `audioMix.voice`
    /// (every sampled sting ran through it). Per-cue base gains are the old `playSfx` values.
    pub sfx_vol: f32,
    /// Master for hero grunts + creature voices (also the old `audioMix.voice`).
    pub voice_vol: f32,
    /// Music bed + combat layer level.
    pub music_vol: f32,
    /// Hero spoken biome lines level (kept low, under the mix).
    pub narration_vol: f32,
    /// Combat layer gain, relative to `music_vol`, while fighting.
    pub combat_music: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            ambience_vol: 0.1,
            audible_range: 32.0,
            call_min: 30.0,
            call_max: 70.0,
            sfx_vol: 0.6,
            voice_vol: 0.6,
            music_vol: 0.22,
            narration_vol: 0.57,
            combat_music: 1.0,
        }
    }
}

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioConfig>()
            .init_resource::<MusicState>()
            .init_resource::<synth::StingBank>()
            .init_resource::<HeroLineGates>()
            .init_resource::<HeroLineCooldown>()
            .init_resource::<HeroSpeaking>()
            .init_resource::<OthersSpeaking>()
            .init_resource::<director::VoiceManager>()
            .init_resource::<RemarkTrigger>()
            .init_resource::<npc::VillagerTrigger>()
            .init_resource::<ork::OrkTrigger>()
            .add_message::<AudioCue>()
            .add_message::<director::Speak>()
            .add_systems(
                Startup,
                (
                    animals::load_voices,
                    ambience::setup_ambience,
                    music::setup_music,
                    sfx::setup_sfx,
                    synth::bake_stings,
                    voice::setup_voice,
                    npc::setup_villager_trigger,
                    director::setup_voice_manager,
                    director::preload_voice_lines,
                ),
            )
            .add_systems(
                Update,
                (
                    animals::animal_voices,
                    ambience::biome_ambience,
                    ambience::attach_campfire_audio,
                    footsteps::hero_footsteps,
                    music::update_music,
                    sfx::play_cues,
                    voice::play_voice_cues,
                    detect_player_events,
                    detect_home_return,
                    detect_biome_entry,
                    detect_siege_voice,
                    detect_equip,
                    synth::debug_play_stings,
                    stop_displaced_hero_lines,
                    fade_out_hero_lines,
                ),
            )
            // Villager + ork + hero-remark voices only while actually playing (no chatter in
            // menus / panels). Ordered AFTER the hero's own voice so it sets this frame's
            // `HeroLineCooldown` first — event/biome lines win, observational remarks defer.
            .add_systems(
                Update,
                (npc::detect_villager_ambient, npc::detect_villager_events, ork::detect_ork_voices, detect_hero_remarks)
                    .after(voice::play_voice_cues)
                    .run_if(in_state(crate::game_state::AppState::Playing)),
            )
            // The director is the PLAYBACK layer — gated on `Playing` (like every sibling audio
            // system) so an in-flight line finishes through a panel. The SIM layer is the
            // `detect_*` trigger systems that emit `Speak`; those carry `Modal::None` so no new
            // line is *decided* while the world is frozen.
            .add_systems(
                Update,
                (director::speak_director, director::tick_chains)
                    .run_if(in_state(crate::game_state::AppState::Playing)),
            )
            // Fresh run: clear the once-per-run voice gates (mirrors siege's reset).
            .add_systems(
                OnExit(crate::game_state::AppState::StartScreen),
                (reset_hero_line_gates, reset_remark_trigger, director::reset_voices, npc::reset_villager_trigger, ork::reset_ork_trigger),
            )
            .add_systems(
                OnExit(crate::game_state::AppState::GameOver),
                (reset_hero_line_gates, reset_remark_trigger, director::reset_voices, npc::reset_villager_trigger, ork::reset_ork_trigger),
            );
    }
}

fn reset_hero_line_gates(mut gates: ResMut<HeroLineGates>) {
    *gates = HeroLineGates::default();
}

// ── Hero observational remarks — trigger system ──

/// Hero must be within this many world units of a thing for its proximity remark to fire.
const NEAR: f32 = 7.0;
/// "Quiet day" only fires in prep with no ork within this of the hero.
const QUIET_CLEAR: f32 = 28.0;
/// Delay after a run starts before the intro line plays (let the scene settle).
const INTRO_DELAY: f32 = 1.6;
/// Minimum seconds between two remark emissions — the global cadence throttle.
const REMARK_GAP: f32 = 20.0;

/// Per-run remark trigger state. Holds the intro arm and the next-remark cadence clock.
#[derive(Resource, Default)]
pub(crate) struct RemarkTrigger {
    intro_done: bool,
    intro_at: Option<f32>,
    next_remark: f32,
}

fn reset_remark_trigger(mut r: ResMut<RemarkTrigger>) {
    *r = RemarkTrigger::default();
}

/// Detect proximity / phase situations and emit [`Speak`] for the hero's observational remarks.
/// Replaces the bespoke `hero_remarks::tick` — per-line replay floors now live in the catalog
/// (`floor: 300.0`) and random variety comes from `pick_line`; this system owns only the
/// cadence gate and the proximity computation.
#[allow(clippy::too_many_arguments)]
fn detect_hero_remarks(
    time: Res<Time>,
    mut trigger: ResMut<RemarkTrigger>,
    mgr: Res<director::VoiceManager>,
    mut speak: MessageWriter<Speak>,
    mut cues: MessageReader<AudioCue>,
    hero: Query<&crate::player::Hero>,
    siege: Option<Res<crate::siege::Siege>>,
    (townsfolk, pets, orks): (
        Query<
            (&GlobalTransform, Has<crate::villagers::Kid>, Has<crate::villagers::Guard>),
            With<crate::villagers::Villager>,
        >,
        Query<(&GlobalTransform, &crate::wildlife::Animal)>,
        Query<&GlobalTransform, (With<crate::orks::Ork>, Without<crate::dying::Dying>)>,
    ),
) {
    let now = time.elapsed_secs();

    // ── Intro: once per run, a short beat after the scene comes up. ──
    if !trigger.intro_done {
        match trigger.intro_at {
            None => {
                trigger.intro_at = Some(now + INTRO_DELAY);
                return;
            }
            Some(t) if now < t => return,
            Some(_) => {
                speak.write(Speak::new(Concept::Intro));
                trigger.intro_done = true;
                // Let the intro have its turn — director/guard handles missing clip silently;
                // catalog `once` prevents replays. Return so the intro gets its own frame.
                return;
            }
        }
    }

    // ── Global cadence: don't remark constantly. ──
    if now < trigger.next_remark {
        return;
    }

    // ── Don't talk over an ongoing hero or NPC line. ──
    if mgr.hero_speaking(now) || mgr.others_speaking(now) {
        return;
    }

    let Ok(hero) = hero.single() else { return };
    let hp = hero.pos;

    // Drain the cue stream every frame; note a kill if a connecting blow finished something.
    let mut killed = false;
    for c in cues.read() {
        if matches!(c, AudioCue::Impact { kill: true }) {
            killed = true;
        }
    }

    // ── Proximity flags (one pass over townsfolk; kids/guards are villagers too). ──
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
    use crate::critters::Species;
    let near_pet = pets
        .iter()
        .any(|(t, a)| matches!(a.species, Species::Dog | Species::Cat) && dist_ok(t));
    let in_keep = crate::castle::in_footprint(hp.x, hp.y);
    let phase = siege.as_ref().map(|s| s.phase);
    let orks_near = orks.iter().any(|t| {
        let p = t.translation();
        Vec2::new(p.x, p.z).distance(hp) <= QUIET_CLEAR
    });

    // ── Trigger priority: Kill > Kids > Pet > Guard > Keep > Town > Night > Quiet. ──
    use crate::siege::GamePhase;
    let concept = if killed {
        Some(Concept::KillMusing)
    } else if near_kids {
        Some(Concept::NearKids)
    } else if near_pet {
        Some(Concept::NearPet)
    } else if near_guard {
        Some(Concept::NearGuard)
    } else if in_keep {
        Some(Concept::InKeep)
    } else if near_town {
        Some(Concept::NearTown)
    } else if phase == Some(GamePhase::Wave) {
        Some(Concept::NightMusing)
    } else if phase.map(|p| p == GamePhase::Prep).unwrap_or(true) && !orks_near {
        Some(Concept::QuietMusing)
    } else {
        None
    };

    // Walk-away anchoring for these proximity remarks is handled in the director: `play_line`
    // attaches a `HeroLineAnchor` (via `anchor_for`) and `stop_displaced_hero_lines` fades the line
    // if he leaves. The trigger just decides what to say.

    let Some(concept) = concept else { return };
    speak.write(Speak::new(concept));
    trigger.next_remark = now + REMARK_GAP;
}

/// Cut a place-bound hero line the moment he leaves what prompted it — walks off from the guards
/// he was addressing, or steps out of the biome he was musing on (see [`HeroLineAnchor`]). If it's
/// almost over (within [`FINISH_GRACE`] of its estimated end) let him finish; otherwise start a
/// quick fade. Sinks self-despawn when the clip ends, so this only ever acts on one still playing.
fn stop_displaced_hero_lines(
    mut commands: Commands,
    time: Res<Time>,
    mgr: Res<director::VoiceManager>,
    hero: Query<&crate::player::Hero>,
    // Anchors are attached only to catalog hero lines (`VoiceSink(Hero)`), so `&HeroLineAnchor`
    // already restricts the query to them; no speaker filter needed.
    lines: Query<(Entity, &HeroLineAnchor), Without<HeroLineFadeOut>>,
) {
    let Ok(hero) = hero.single() else { return };
    let now = time.elapsed_secs();
    // The hero line's estimated end, from the director (replaces the old `HeroSpeaking.until`).
    let ends_at = mgr.active.get(&Speaker::Hero).map(|a| a.ends_at);
    for (e, anchor) in &lines {
        let left = match anchor {
            HeroLineAnchor::Near { pos, r } => hero.pos.distance(*pos) > *r,
            HeroLineAnchor::Biome(b) => {
                crate::worldmap::biome_at_world(hero.pos.x, hero.pos.y) != Some(*b)
            }
        };
        if left {
            if ends_at.is_some_and(|end| end - now <= FINISH_GRACE) {
                continue; // almost done → let him finish the thought
            }
            commands.entity(e).insert(HeroLineFadeOut);
        }
    }
}

/// Ramp a cut-short hero line's volume down to silence over a few frames (~0.3 s), then despawn —
/// the graceful trail-off for [`stop_displaced_hero_lines`].
fn fade_out_hero_lines(
    mut commands: Commands,
    mut q: Query<(Entity, &mut AudioSink), With<HeroLineFadeOut>>,
) {
    for (e, mut sink) in &mut q {
        let v = sink.volume().to_linear() * 0.82; // ~0.3 s to inaudible at 60 fps
        if v <= 0.02 {
            commands.entity(e).try_despawn();
        } else {
            sink.set_volume(Volume::Linear(v));
        }
    }
}

/// Emit the home-return line: once the hero has roamed past [`AWAY_RADIUS`] from the castle
/// (origin) and comes back inside [`HOME_RADIUS`] during prep. Fires at most once per run:
/// `detect_home_return` sets `gates.home` on emit, and the catalog `once` flag on the `home`
/// line is the backstop in the director.
fn detect_home_return(
    hero: Query<&crate::player::Hero>,
    siege: Option<Res<crate::siege::Siege>>,
    mut gates: ResMut<HeroLineGates>,
    mut speak: MessageWriter<Speak>,
) {
    let Ok(hero) = hero.single() else { return };
    let d = hero.pos.length();
    if d > AWAY_RADIUS {
        gates.been_away = true;
    }
    if gates.been_away && !gates.home && d < HOME_RADIUS {
        let in_prep =
            siege.map(|s| matches!(s.phase, crate::siege::GamePhase::Prep)).unwrap_or(true);
        if in_prep {
            speak.write(Speak::new(Concept::Home));
            gates.home = true;
        }
    }
}

/// Emit the hero's biome musing while he stands in a wilderness biome on the world map. The
/// old game spoke this the first time he walked into each biome; we mirror that by sampling
/// the biome under the hero every frame and writing `Speak(BiomeEntered(b))` — the catalog
/// `once` flag de-dupes to once-per-biome-per-run. Re-firing each frame lets a line that
/// couldn't fire (director busy) speak once the mouth frees up.
///
/// `biome_at_world` returns `None` over grass / sand / water, so the castle and beaches stay
/// silent — the "home, finally" line is left to [`detect_home_return`].
fn detect_biome_entry(hero: Query<&crate::player::Hero>, mut speak: MessageWriter<Speak>) {
    let Ok(hero) = hero.single() else { return };
    if let Some(b) = crate::worldmap::biome_at_world(hero.pos.x, hero.pos.y) {
        speak.write(Speak::new(Concept::BiomeEntered(b)));
    }
}

/// Gold purse size at which the hero first remarks on being flush (once per run).
const GOLD_RICH_AT: i64 = 150;

/// Emit progression-driven stings + spoken reactions off the hero's stats (no single call site):
/// level-up, low-HP, first kill, getting rich, and going broke.
fn detect_player_events(
    player: Res<crate::player::PlayerRes>,
    mut gates: ResMut<HeroLineGates>,
    mut cues: MessageWriter<AudioCue>,
    mut speak: MessageWriter<Speak>,
    mut init: Local<bool>,
    mut last_level: Local<i64>,
    mut last_gold: Local<i64>,
    mut was_low: Local<bool>,
) {
    let p = &player.0;
    if !*init {
        *init = true;
        *last_level = p.level;
        *last_gold = p.gold;
        *was_low = false;
    }
    if p.level > *last_level {
        *last_level = p.level;
        cues.write(AudioCue::LevelUp); // synth sting
        speak.write(Speak::new(Concept::LevelUp)); // spoken line (5-min floored)
    }
    // First kill this run — xp first rises above 0.
    if !gates.first_kill && p.xp > 0 {
        gates.first_kill = true;
        speak.write(Speak::new(Concept::FirstKill));
    }
    // Purse crossed a comfortable threshold (first time this run).
    if !gates.gold_rich && p.gold >= GOLD_RICH_AT {
        gates.gold_rich = true;
        speak.write(Speak::new(Concept::GoldRich));
    }
    // Spent down to nothing (had some, now none).
    if *last_gold > 0 && p.gold == 0 {
        speak.write(Speak::new(Concept::Broke)); // 5-min floored
    }
    *last_gold = p.gold;
    let low = p.max_hp > 0.0 && p.hp > 0.0 && p.hp <= p.max_hp * 0.35;
    if low && !*was_low {
        cues.write(AudioCue::LowHp); // synth danger bleep
        speak.write(Speak::new(Concept::LowHp)); // hero's pained line over it
    }
    *was_low = low;
}

/// Spoken siege reactions: "we held" on the dawn after a wave, and a shout when the keep is
/// battered below half during the night. The keep line edges (fires once per dip-below-half),
/// then the voice module's 10-min floor caps it further.
fn detect_siege_voice(
    siege: Option<Res<crate::siege::Siege>>,
    keep: Option<Res<crate::siege::KeepHp>>,
    mut speak: MessageWriter<Speak>,
    mut prev_phase: Local<Option<crate::siege::GamePhase>>,
    mut keep_low: Local<bool>,
) {
    use crate::siege::GamePhase;
    let Some(siege) = siege else { return };
    let phase = siege.phase;
    if let Some(prev) = *prev_phase {
        if prev == GamePhase::Wave && phase == GamePhase::Prep {
            speak.write(Speak::new(Concept::WaveSurvived));
        }
    }
    *prev_phase = Some(phase);
    if phase == GamePhase::Wave {
        if let Some(keep) = keep {
            let low = keep.max > 0.0 && keep.hp > 0.0 && keep.hp <= keep.max * 0.5;
            if low && !*keep_low {
                speak.write(Speak::new(Concept::KeepHurt));
            }
            *keep_low = low;
        }
    } else {
        *keep_low = false;
    }
}

/// Emit the "new armor / check my satchel" line the first time the hero's equipped gear changes
/// to something real (a weapon bonus or armour mitigation) — detected centrally off the bag so no
/// equip call-site needs to know about audio. Once per run.
fn detect_equip(
    inv: Res<crate::inventory::Inventory>,
    mut gates: ResMut<HeroLineGates>,
    mut speak: MessageWriter<Speak>,
    mut init: Local<bool>,
    mut last: Local<(i64, i64)>,
) {
    let wb = inv.0.weapon_bonus() as i64;
    let am = (inv.0.armor_damage_mult() * 1000.0) as i64; // <1000 ⇒ armour equipped
    let snap = (wb, am);
    if !*init {
        *init = true;
        *last = snap;
        return;
    }
    if snap != *last {
        *last = snap;
        // Only speak when newly geared (ignore a reset that strips gear back to fists).
        if !gates.equip && (wb > 0 || am < 1000) {
            gates.equip = true;
            speak.write(Speak::new(Concept::Equip));
        }
    }
}

// ── Tiny shared RNG (xorshift) — clip picks + pitch jitter without pulling a crate. ──
pub(crate) fn next_rng(s: &mut u32) -> u32 {
    if *s == 0 {
        *s = 0x9e37_79b9;
    }
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}

/// Uniform 0..1.
pub(crate) fn frand(s: &mut u32) -> f32 {
    (next_rng(s) & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

/// Pick a random element (panics only on an empty slice — banks are never empty).
pub(crate) fn pick<T: Clone>(items: &[T], s: &mut u32) -> T {
    let i = (frand(s) * items.len() as f32) as usize;
    items[i.min(items.len() - 1)].clone()
}

/// ±`frac` random pitch multiplier — keeps repeated stings from sounding identical.
pub(crate) fn jitter(s: &mut u32, frac: f32) -> f32 {
    1.0 + (frand(s) * 2.0 - 1.0) * frac
}
