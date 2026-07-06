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

mod advice;
mod ambience;
mod animals;
pub(crate) mod director;
mod footsteps;
pub(crate) mod lines;
mod music;
mod npc;
mod ork;
mod rival_voice;
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
    /// The hero's Sand-Dash warden art — a compressed-air whoosh as he blinks forward.
    Dash,
    /// The hero's dodge-roll (Alt) — a gritty Witcher-style tumble grunt (own clip, not the Dash
    /// whoosh), pitch-jittered per roll so back-to-back evades never sound stamped.
    Roll,
    /// The hero's Bramble-Sweep warden art — an expanding circular energy/shockwave burst.
    Sweep,
    /// The hero's Ground-Slam warden art — a heavy stone-fist impact (random of two takes).
    Slam,
    /// Hero's blow lands → impact (heavier on a kill). `crit` = the swing was a critical (a rolled
    /// crit, a charged Heavy, or a riposte — one flag per swing, from `player::combat`): the impact
    /// swaps to the dedicated crit take so a crit is *heard*, not just seen.
    Impact { kill: bool, crit: bool },
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
    /// A warden (biome boss) roar at a world position — the deep "ancient thing wakes" bellow, far
    /// bigger than an ork's. Fired on aggro + each crit wind-up (`boss::boss_brain`).
    BossRoar(Vec3),
    /// A warden charging its telegraphed critical (`ability-cast.ogg`) at a world position — a
    /// rising magical whine layered over the crit-windup roar, so the "killing blow incoming" tell
    /// is unmistakable by ear (`boss::boss_brain`).
    BossWindup(Vec3),
    /// A wild predator's snarl as it bites the hero, at a world position. `big` = a heavy beast
    /// (bear/croc/golem) → a deeper, louder roar. Pitch-jittered so a flurry never repeats.
    CreatureBite { at: Vec3, big: bool },
    /// A town-guard's melee strike on an invader, at a world position — a quiet spatial swing+thud
    /// so the player *hears* the militia fighting nearby. Emitted only for skirmishes close to the
    /// hero (a small earshot) so the whole battlefield doesn't clatter at once.
    GuardStrike(Vec3),
    /// An archer LOOSES an arrow, at the bow's world position — the sampled bowstring snap + shaft
    /// whip (`bow-shot.ogg`). Earshot-gated by the emitters (the town archers in
    /// `villagers::guard_combat`, the rival's desert bowmen in `rival.rs`) like the guard clash,
    /// but from farther (a twang carries).
    BowShot(Vec3),
    /// One metallic chip on a pick-swing against an ore boulder (sampled `var-1`/`var-3`
    /// clips, pitch-jittered). Distinct from the `OreShatter` synth sting on the breaking blow.
    OreChip,
    /// A single axe chop landing on a tree (sampled `chop-wood-{1..3}.ogg`, random take +
    /// pitch jitter). Emitted once per swing that strikes any choppable tree (`verbs::chop_tree`).
    WoodChop,
    /// A felled tree hitting the ground — the combined crack+crash (`tree-fall.ogg`). `cactus`
    /// swaps it for the dry wood-crack alone (`wood-crack.ogg`), since a saguaro has no heavy
    /// timber to crash. Emitted once at the landing frame by `verbs::drive_felling`, earshot-gated.
    TreeFall { cactus: bool },
    /// Herb / loot picked up (sampled `forage.ogg`).
    Forage,
    /// Night wave summoned — the war bell's single hard toll (sampled `war-bell.ogg`).
    WarBell,
    // ── Procedural stings (synth-baked, handled by `sfx` via [`synth::StingBank`]) ──
    /// Ore boulder shattered.
    OreShatter,
    /// Chest lid opened.
    ChestOpen,
    /// Hero gained a level.
    LevelUp,
    /// Gold collected.
    Gold,
    /// Shop purchase confirmed.
    ShopBuy,
    /// Camp/Blight captive freed — carries the cage's world position so the freed peasant's
    /// voiced `Rescued` reaction (`npc.rs`) speaks from the cage, not from some random
    /// townsperson across the map.
    CampRescue(Vec3),
    /// Hero HP crossed the low threshold.
    LowHp,
    /// The ork fortress sounds its war-horn (sampled `war-horn.ogg` — a sharp wood crack,
    /// then a deep blast; spatial at the gate) — the hero crossed Gnashfang Hold's outer
    /// threshold (`ork_fortress.rs`).
    FortressHorn(Vec3),
    /// A green warp bolt leaves a shaman's staff or a fortress watchtower (sampled
    /// `warp-cast.ogg` — a sharp magical release; spatial at the muzzle).
    WarpCast(Vec3),
    /// A distant thunder rumble during a night siege — fired (after a short flash→sound delay) by
    /// `storm.rs` when the storm strobes the battlefield. Synth-baked low rumble; non-spatial.
    Thunder,
    /// A wild predator just locked onto a target (idle/graze → hunt) — a low stalk-growl at a
    /// world position, the ~2 s "you've been seen" tell before the charge. Reuses the beast-snarl
    /// pool pitched DOWN so it reads as a warning, not a bite. Throttled like the other beasts.
    CreatureAggro(Vec3),
    /// A dormant ambush snowman just lurched to life (hero stepped near / struck it) — a crunchy
    /// packed-snow grunt pitched DOWN, at the snowman's world position (`snowman.rs`).
    SnowmanWake(Vec3),
    /// An awakened snowman's slam connecting on the hero — a random snowman attack grunt at its
    /// world position (`snowman.rs`).
    SnowmanSlam(Vec3),
}

/// Per-run gates for the "once" event voice lines (the old game's `spoken` key-set). Reset on
/// a fresh run. `been_away` latches the home-return line: it can only fire after the hero has
/// roamed past [`AWAY_RADIUS`] from the castle.
#[derive(Resource, Default)]
pub(crate) struct HeroLineGates {
    pub home: bool,
    pub been_away: bool,
    /// Once-per-run gates for the new spoken reactions (the rest are repeatable / floor-capped).
    /// (Gear-found "equip" is no longer gated here — it's driven off the director's `once` latch in
    /// `detect_gear_found`, so a request dropped by the hero-speech window isn't lost.)
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
    /// True while any warden (biome boss) is engaged (hostile); [`music`] swells the boss-fight
    /// track over the daytime mix from it. Set by `boss::warden_music_flag`.
    pub warden_active: bool,
}

/// Shared **hero-line spacing** for the hero's spoken voice. EVERY spoken hero LINE — the catalog's
/// event reactions, biome musings, AND observational remarks (now all routed through `director`) —
/// stamps `until = now + HERO_LINE_CD` and the line's `priority` when it starts, and the director
/// refuses to begin a new hero line while `now < until` **unless** the newcomer is URGENT
/// (`lines::HERO_URGENT_PRIORITY`) and strictly out-ranks the line that opened the window — so
/// real warnings (night falling, the keep under attack) cut through ~20 s of idle chatter, but
/// ordinary remarks can never ladder up the priority tiers back-to-back. An urgent line that cuts
/// through re-stamps the window at its own priority, so everything quieter waits the full cooldown
/// behind it. A line that wanted to fire inside the window is simply dropped (never queued —
/// "consider it played"); it re-fires next frame if its trigger persists. Short combat exertions
/// (swing/jump/hurt grunts) are exempt; the death cry spends the window.
#[derive(Resource, Default)]
pub(crate) struct HeroLineCooldown {
    pub until: f32,
    /// Priority of the line that opened the current window — a newcomer must exceed it to bypass.
    pub priority: u8,
}
/// Length of [`HeroLineCooldown`] (seconds) — per user: ~20 s between hero lines.
pub(crate) const HERO_LINE_CD: f32 = 20.0;

/// One central "is the hero in a fight right now" flag, recomputed each frame by
/// [`track_hero_threat`]. The director consults it to MUTE every *peaceful* concept
/// (`lines::is_peaceful`) — ambient remarks, town chatter, economy advice, exploration musings —
/// the instant combat is on, no matter the siege phase. This replaces the old scatter of
/// per-trigger phase checks that let e.g. a daytime warden fight slip a "Quiet day…" musing through
/// (the bug: the old gate only counted *orks*, so a boss/warden/rival fight read as "peaceful").
#[derive(Resource, Default)]
pub(crate) struct HeroThreat {
    pub in_danger: bool,
}
/// A hostile this near the hero (world units) counts as "in a fight" for voice gating.
const THREAT_RADIUS: f32 = 22.0;

/// Recompute [`HeroThreat`]: in danger if ANY live hostile (ork, biome warden / night boss, the
/// Warlord, or a rival soldier/raider) is within [`THREAT_RADIUS`] of the hero, OR a night wave is
/// underway. Predators are intentionally excluded for now (prey animals share the `Animal` tag, so
/// "near a deer" must not read as combat) — a future pass can add predator-only detection.
pub(crate) fn track_hero_threat(
    mut threat: ResMut<HeroThreat>,
    hero: Option<Res<crate::player::HeroState>>,
    siege: Option<Res<crate::siege::Siege>>,
    hostiles: Query<
        &GlobalTransform,
        (
            Or<(
                With<crate::orks::Ork>,
                With<crate::boss::Boss>,
                With<crate::warlord::Warlord>,
                With<crate::rival::RivalSoldier>,
                With<crate::rival::RivalRaider>,
            )>,
            Without<crate::dying::Dying>,
        ),
    >,
) {
    let wave = siege.is_some_and(|s| s.phase == crate::siege::GamePhase::Wave);
    let near = hero.is_some_and(|h| {
        let hp = h.pos;
        let r2 = THREAT_RADIUS * THREAT_RADIUS;
        hostiles.iter().any(|g| {
            let t = g.translation();
            Vec2::new(t.x, t.z).distance_squared(hp) < r2
        })
    });
    threat.in_danger = wave || near;
}

// The old `HeroSpeaking` / `OthersSpeaking` "who's mid-sentence" resources are gone: the director's
// `VoiceManager::hero_speaking(now)` / `others_speaking(now)` derive the same facts from the active
// per-speaker lines, so villagers/orks/hero all defer to each other through one source of truth.

/// Tags the hero's reflex grunt/death sinks (`voice.rs`) so a new grunt can stop the prior one.
/// (Catalog hero lines use `director::VoiceSink(Hero)` instead — the two mouths coordinate via
/// `VoiceManager::hero_speaking`.)
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
            music_vol: 0.154, // 0.22 authored bed, dropped 30% for a quieter default mix
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
            .init_resource::<director::VoiceManager>()
            .init_resource::<director::OfferedReply>()
            .init_resource::<RemarkTrigger>()
            .init_resource::<HeroThreat>()
            .init_resource::<npc::VillagerTrigger>()
            .init_resource::<ork::OrkTrigger>()
            .init_resource::<rival_voice::RivalVoiceTrigger>()
            .init_resource::<advice::AdviceTrigger>()
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
                    synth::load_war_drums,
                    voice::setup_voice,
                    npc::setup_villager_trigger,
                    director::setup_voice_manager,
                    director::preload_voice_lines,
                ),
            )
            // PLAYBACK / render-tier audio — ungated so an in-flight line, ambience and music keep
            // playing through a panel-freeze or pause (the frozen world still sounds alive).
            .add_systems(
                Update,
                (
                    animals::animal_voices,
                    ambience::biome_ambience,
                    ambience::attach_campfire_audio,
                    ambience::attach_war_drum_audio,
                    ambience::war_drums,
                    // Faint kid-play chatter near the play patch; silent on the menu / at night.
                    ambience::kids_chatter.run_if(in_state(crate::game_state::AppState::Playing)),
                    footsteps::hero_footsteps,
                    music::update_music,
                    sfx::play_cues,
                    voice::play_voice_cues,
                    synth::debug_play_stings,
                    stop_displaced_hero_lines,
                    fade_out_hero_lines,
                ),
            )
            // SIM-tier hero-voice TRIGGERS — gated on `Modal::None` so no new line is *decided*
            // while the world is frozen (e.g. buying gear in the shop must not fire the satchel
            // line; a level-up sting must not play over a paused panel). Matches the contract the
            // director comment below describes.
            .add_systems(
                Update,
                (
                    detect_player_events,
                    detect_home_return,
                    detect_biome_entry,
                    detect_siege_voice,
                    detect_gear_found,
                    advice::detect_town_advice,
                )
                    .run_if(in_state(crate::game_state::Modal::None)),
            )
            // Villager + ork + hero-remark voices only while actually playing (no chatter in
            // menus / panels). Ordered AFTER the hero's own voice so it sets this frame's
            // `HeroLineCooldown` first — event/biome lines win, observational remarks defer.
            .add_systems(
                Update,
                (npc::detect_villager_ambient, npc::detect_villager_events, ork::detect_ork_voices, rival_voice::detect_rival_voices, detect_hero_remarks)
                    .after(voice::play_voice_cues)
                    .run_if(in_state(crate::game_state::AppState::Playing)),
            )
            // The director is the PLAYBACK layer — gated on `Playing` (like every sibling audio
            // system) so an in-flight line finishes through a panel. The SIM layer is the
            // `detect_*` trigger systems that emit `Speak`; those carry `Modal::None` so no new
            // line is *decided* while the world is frozen.
            // Recompute the central combat flag just before the director reads it, so a peaceful
            // line decided this frame is muted the instant a fight is on.
            .add_systems(
                Update,
                track_hero_threat
                    .before(director::speak_director)
                    .run_if(in_state(crate::game_state::AppState::Playing)),
            )
            .add_systems(
                Update,
                (director::speak_director, director::tick_chains)
                    .run_if(in_state(crate::game_state::AppState::Playing)),
            )
            // Fresh run: clear the once-per-run voice gates (mirrors siege's reset).
            .add_systems(
                OnExit(crate::game_state::AppState::StartScreen),
                (reset_hero_line_gates, reset_remark_trigger, director::reset_voices, npc::reset_villager_trigger, ork::reset_ork_trigger, rival_voice::reset_rival_trigger, advice::reset_advice),
            )
            .add_systems(
                OnExit(crate::game_state::AppState::GameOver),
                (reset_hero_line_gates, reset_remark_trigger, director::reset_voices, npc::reset_villager_trigger, ork::reset_ork_trigger, rival_voice::reset_rival_trigger, advice::reset_advice),
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
/// When the cadence comes due, the hero only ACTUALLY remarks with this probability — idle
/// musings ("old stones…", "quiet day…") are an occasional colour beat, not a metronome. A
/// failed roll re-arms [`REMARK_RETRY`], so the average idle gap lands around
/// `RETRY / CHANCE ≈ 35 s` (plus however long the remark itself ran). Event reactions
/// (night warning, keep hurt, low HP…) don't pass through this system and are unaffected.
const REMARK_CHANCE: f32 = 0.3;
/// Kill barks ("one more for the pile") roll much lower than ordinary remarks — a kill preempts
/// the ambient pool (it's first in the priority list), so without a small chance the hero would
/// comment on every felled ork/animal during a wave. Most kills pass in silence; combined with
/// the per-line 5-min `floor` a bark lands only occasionally.
const KILL_REMARK_CHANCE: f32 = 0.1;
/// Re-check delay after a failed remark roll (seconds).
const REMARK_RETRY: f32 = 10.0;

/// Per-run remark trigger state. Holds the intro arm and the next-remark cadence clock.
#[derive(Resource, Default)]
pub(crate) struct RemarkTrigger {
    intro_done: bool,
    intro_at: Option<f32>,
    next_remark: f32,
    /// RNG for the [`REMARK_CHANCE`] roll (xorshift; `frand` self-seeds a zero state).
    rng: u32,
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
        if matches!(c, AudioCue::Impact { kill: true, .. }) {
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
    // Gnashfang Hold centre (world XZ) — fires when hero approaches the south blight.
    const FORTRESS_CENTRE: Vec2 = Vec2::new(12.0, 103.0);
    const FORTRESS_NEAR_R: f32 = 55.0;
    let near_fortress = hp.distance(FORTRESS_CENTRE) <= FORTRESS_NEAR_R;
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

    // ── Trigger priority: Kill > Kids > Pet > Guard > Keep > Town > Fortress > Night > Quiet. ──
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
    } else if near_fortress {
        Some(Concept::NearFortress)
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
    // The dice roll: most due cadences pass in silence. Kill barks use a much smaller chance so
    // the hero doesn't narrate every kill. Frame-time bits mixed into the rng keep the pattern
    // from repeating run to run.
    let chance = if concept == Concept::KillMusing { KILL_REMARK_CHANCE } else { REMARK_CHANCE };
    trigger.rng ^= now.to_bits();
    if frand(&mut trigger.rng) > chance {
        trigger.next_remark = now + REMARK_RETRY;
        return;
    }
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
            // try_insert: the sink is `PlaybackMode::Despawn` and self-despawns the moment the
            // clip ends (and `play_line` reaps it when a new line barges in), so it can be gone
            // by the time this command applies.
            commands.entity(e).try_insert(HeroLineFadeOut);
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

/// Emit the "new gear / check my satchel" line the first time a piece of GEAR — armour OR a
/// weapon — enters the bag, i.e. when the hero finds, loots or buys gear, NOT when he equips/wears
/// it (the line tells him to go look it over in his satchel, so it belongs at the moment of
/// acquiring it). One shared clip covers either find. Detected centrally off the bag so no pickup
/// call-site needs to know about audio.
///
/// We re-request the line every frame while gear sits in the bag, until it has ACTUALLY played
/// (the director's per-run `once` latch for `"equip"`), rather than latching a gate the instant we
/// *request* it: a request dropped by the ~20 s hero-speech window would otherwise burn the
/// one-shot and the hero would never comment. The catalog line's `once: true` stops it repeating
/// once it lands, so the spam is just a handful of cheap, deduped requests until the window clears.
fn detect_gear_found(
    inv: Res<crate::inventory::Inventory>,
    mgr: Res<director::VoiceManager>,
    mut speak: MessageWriter<Speak>,
) {
    use tileworld_core::inventory::{item_def, ItemKind};
    if mgr.played_once.contains("equip") {
        return; // already commented this run
    }
    let has_gear = inv.0.bag.iter().any(|s| {
        s.item_id
            .as_deref()
            .and_then(item_def)
            .is_some_and(|d| matches!(d.kind, ItemKind::Armor | ItemKind::Weapon))
    });
    if has_gear {
        speak.write(Speak::new(Concept::Equip));
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
