//! One-shot stings — combat feedback, the UI blip, footsteps, and the spatial creature
//! voices. Reads the [`AudioCue`] stream and spawns a short-lived `PlaybackMode::Despawn`
//! sink per cue (Bevy frees it when the clip ends — the equivalent of the old game's SFX
//! pool). Non-spatial cues play head-locked; ork voices spawn at a world position so the
//! camera's `SpatialListener` pans + attenuates them.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::synth::{Sting, StingBank};
use super::{jitter, pick, AudioConfig, AudioCue, Surface};

// ── Stacking guard for background combat stings ──────────────────────────────────────────
// Each ork / guard / beast emits its OWN sting per blow, so a melee of many of them spawns
// dozens of identical sinks in a single frame and the mix clips into a wall of mush. We cap
// the stacking-prone categories centrally here: each may retrigger no faster than its gap,
// and at most N may play in any one frame, so a same-frame pile-up collapses to one or two
// voices. The hero's own Impact/Swing are NOT throttled — those are foreground, one-per-swing
// already, and the player wants to hear every one.
const THROTTLE_N: usize = 4;
const T_GRUNT: usize = 0;
const T_ROAR: usize = 1;
const T_BITE: usize = 2;
const T_GUARD: usize = 3;
/// Min seconds between two plays of each throttled category.
const THROTTLE_GAP: [f32; THROTTLE_N] = [0.18, 0.30, 0.10, 0.12];
/// Max plays of each category within a single frame.
const THROTTLE_FRAME_CAP: [u8; THROTTLE_N] = [1, 1, 2, 1];

/// Per-category rate limiter for the background combat stings (see above).
#[derive(Default)]
pub(crate) struct SfxThrottle {
    /// Last play time per category.
    last: [f32; THROTTLE_N],
    /// Time-stamp of the frame `count` belongs to. `Default` zero-inits this and `count` together,
    /// so the first frame needs no special reset — the `frame != now` check just re-zeros `count`
    /// whenever the timestamp moves on.
    frame: f32,
    /// How many of each category have played in `frame` so far.
    count: [u8; THROTTLE_N],
}

impl SfxThrottle {
    /// Returns true and books a slot if `cat` may play now; false if it's still throttled.
    fn allow(&mut self, cat: usize, now: f32) -> bool {
        if self.frame != now {
            self.frame = now;
            self.count = [0; THROTTLE_N];
        }
        if self.count[cat] >= THROTTLE_FRAME_CAP[cat] || now - self.last[cat] < THROTTLE_GAP[cat] {
            return false;
        }
        self.last[cat] = now;
        self.count[cat] += 1;
        true
    }
}

/// All one-shot SFX handles, loaded once at startup.
#[derive(Resource)]
pub(crate) struct SfxBank {
    swing: Handle<AudioSource>,
    /// The meaty blade-on-flesh impacts (`sword-hit-{1,2,3}`) — every creature/ork hit picks one
    /// at random and pitch-jitters it, so a flurry of blows never sounds canned. Three steel-on-
    /// target takes; replaced the old single flesh clip (which the comment used to call out as the
    /// old game's one-clip rule — we now have proper variation).
    flesh: Vec<Handle<AudioSource>>,
    /// Metallic chips (`ore-chip-{1,2}`) — picked at random for chipping stone (ore). These are the
    /// old `sword-hit-var-{1,3}` clinks, relocated when the `sword-hit-*` namespace became the
    /// flesh pool — the ore pick must stay a dry metallic clink, not a fleshy thud.
    chips: Vec<Handle<AudioSource>>,
    /// Wood-axe chops landing on a tree (three takes, picked at random + pitch-jittered per
    /// swing so a long chop session never loops one clip).
    chops: Vec<Handle<AudioSource>>,
    /// A whole tree coming down: a wood crack/snap layered with a heavy ground crash
    /// (`tree-fall.ogg`). Played once at the felling landing for woody trees.
    tree_fall: Handle<AudioSource>,
    /// Just the dry wood crack/snap (`wood-crack.ogg`) — the cactus felling sound (a saguaro
    /// has no heavy trunk to crash, so it gets the crack alone).
    wood_crack: Handle<AudioSource>,
    /// The hero's CRITICAL strike landing (`crit-hit.ogg` — a heavy sword slash smashing into
    /// armor). Played INSTEAD of the flesh pool when the swing crit (rolled / Heavy / riposte),
    /// so a crit is heard, not just seen.
    crit: Handle<AudioSource>,
    /// An archer's loose (`bow-shot.ogg` — a powerful bowstring snap + the shaft cutting off it).
    /// Spatial at the bow for every friendly AND rival arrow release.
    bow: Handle<AudioSource>,
    /// Shield-block impacts (`block-{1,2}`) — sharp steel-on-steel parries; picked at random +
    /// pitch-jittered so repeated blocks don't sound stamped.
    blocks: Vec<Handle<AudioSource>>,
    /// Sand-Dash whoosh — a compressed-air burst as the hero blinks forward (warden art).
    dash: Handle<AudioSource>,
    /// Dodge-roll tumble grunt (`dodge-roll.ogg`) — a gritty medieval combat-roll take (silence
    /// trimmed, loudness-matched). The hero's Alt evade, distinct from the Sand-Dash whoosh.
    roll: Handle<AudioSource>,
    /// Bramble-Sweep burst — an expanding circular energy wave (warden art).
    sweep: Handle<AudioSource>,
    /// Ground-Slam impacts — two heavy stone-fist takes, picked at random per slam.
    slams: Vec<Handle<AudioSource>>,
    ui: Handle<AudioSource>,
    /// Sampled herb-pick rustle (replaces the old `Sting::Forage` synth blip).
    forage: Handle<AudioSource>,
    /// Sampled bronze bell toll for summoning the night (replaces the old `Sting::WarBell`
    /// synth tone) — one hard strike with a long ominous decay.
    war_bell: Handle<AudioSource>,
    /// Sampled orchestral level-up fanfare (the old game's `playLevelUpFanfare`) — replaces the
    /// synth arpeggio on `AudioCue::LevelUp` (hero level-up + the big landmark/shrine rewards).
    level_up: Handle<AudioSource>,
    /// Dirt footstep variants (the old `footstep-dirt-var-{1,2,3}`); snow/stone are single clips.
    foot_dirt: Vec<Handle<AudioSource>>,
    foot_snow: Handle<AudioSource>,
    foot_stone: Handle<AudioSource>,
    ork_grunts: Vec<Handle<AudioSource>>,
    ork_roars: Vec<Handle<AudioSource>>,
    /// Warden (biome boss) roars — the deep "ancient thing wakes" bellow, picked at random per
    /// roar. Far bigger + louder than an ork's; played on a warden's aggro + crit wind-up.
    boss_roars: Vec<Handle<AudioSource>>,
    /// Warden crit-windup charge whine (`ability-cast.ogg`) — a rising magical charge layered over
    /// the roar when a warden rears back for its killing blow (the audible "block/dodge NOW" cue).
    boss_windup: Handle<AudioSource>,
    /// Gnashfang Hold's war-horn (`war-horn.ogg` — wood crack, then a deep horn blast).
    war_horn: Handle<AudioSource>,
    /// Warp-bolt release (`warp-cast.ogg`) — shaman staff casts + fortress tower fire.
    warp_cast: Handle<AudioSource>,
    /// Aggressive bite SFX played when a *light* wild predator bites the hero (Wolf/Boar/Scorpion
    /// share this — only the wolf has a real recording). `wolf-bite.ogg` = a sharp canine snarl,
    /// trimmed to the punchy snap. Pitch-jittered per bite.
    beast_snarls: Vec<Handle<AudioSource>>,
    /// Heavy-beast bite SFX (bear/croc/golem). Two trimmed bear attack-impact roars
    /// (`bear-bite-1/2.ogg`) — deeper + louder than the snarls, picked at random per bite.
    beast_roars: Vec<Handle<AudioSource>>,
    /// Ambush-snowman SLAM SFX (`snowman-{1,2}.ogg`) — two crunchy packed-snow attack grunts,
    /// picked at random + pitch-jittered per slam.
    snowman: Vec<Handle<AudioSource>>,
    /// Ambush-snowman WAKE SFX (`snowman-wake-{1,2}.ogg`) — an evil snowman groaning up from a long
    /// frozen sleep; picked at random when a dormant snowman lurches to life.
    snowman_wake: Vec<Handle<AudioSource>>,
}

pub(crate) fn setup_sfx(asset: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(SfxBank {
        swing: asset.load("audio/sword-swing.ogg"),
        flesh: ["audio/sword-hit-1.ogg", "audio/sword-hit-2.ogg", "audio/sword-hit-3.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        chips: ["audio/ore-chip-1.ogg", "audio/ore-chip-2.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        chops: ["audio/chop-wood-1.ogg", "audio/chop-wood-2.ogg", "audio/chop-wood-3.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        tree_fall: asset.load("audio/tree-fall.ogg"),
        wood_crack: asset.load("audio/wood-crack.ogg"),
        crit: asset.load("audio/crit-hit.ogg"),
        bow: asset.load("audio/bow-shot.ogg"),
        blocks: ["audio/block-1.ogg", "audio/block-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        dash: asset.load("audio/sand-dash.ogg"),
        roll: asset.load("audio/dodge-roll.ogg"),
        sweep: asset.load("audio/bramble-sweep.ogg"),
        slams: ["audio/ground-slam-1.ogg", "audio/ground-slam-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        ui: asset.load("audio/menu-select.ogg"),
        forage: asset.load("audio/forage.ogg"),
        war_bell: asset.load("audio/war-bell.ogg"),
        level_up: asset.load("audio/level-up-orchestra.ogg"),
        foot_dirt: ["audio/footstep-dirt-1.ogg", "audio/footstep-dirt-2.ogg", "audio/footstep-dirt-3.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        foot_snow: asset.load("audio/footstep-snow.ogg"),
        foot_stone: asset.load("audio/footstep-stone.ogg"),
        ork_grunts: ["audio/ork-grunt-1.ogg", "audio/ork-grunt-2.ogg", "audio/ork-grunt-3.ogg", "audio/monster-snarl.ogg", "audio/monster-growl.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        ork_roars: ["audio/ork-roar.ogg", "audio/wave-start-roar.ogg"].iter().map(|f| asset.load(*f)).collect(),
        boss_roars: ["audio/boss-roar-1.ogg", "audio/boss-roar-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        boss_windup: asset.load("audio/ability-cast.ogg"),
        war_horn: asset.load("audio/war-horn.ogg"),
        warp_cast: asset.load("audio/warp-cast.ogg"),
        beast_snarls: ["audio/wolf-bite.ogg"].iter().map(|f| asset.load(*f)).collect(),
        beast_roars: ["audio/bear-bite-1.ogg", "audio/bear-bite-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        snowman: ["audio/snowman-1.ogg", "audio/snowman-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
        snowman_wake: ["audio/snowman-wake-1.ogg", "audio/snowman-wake-2.ogg"].iter().map(|f| asset.load(*f)).collect(),
    });
}

/// Spawn a non-spatial one-shot.
fn one_shot(commands: &mut Commands, clip: Handle<AudioSource>, vol: f32, speed: f32) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            speed,
            spatial: false,
            ..default()
        },
    ));
}

/// Spawn a one-shot positioned in the world (panned + attenuated by the camera listener).
fn spatial_shot(commands: &mut Commands, clip: Handle<AudioSource>, vol: f32, speed: f32, pos: Vec3) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(vol),
            speed,
            spatial: true,
            ..default()
        },
        Transform::from_translation(pos),
    ));
}

pub(crate) fn play_cues(
    mut commands: Commands,
    time: Res<Time>,
    cfg: Res<AudioConfig>,
    bank: Res<SfxBank>,
    stings: Res<StingBank>,
    mut seed: Local<u32>,
    mut throttle: Local<SfxThrottle>,
    mut cues: MessageReader<AudioCue>,
) {
    let now = time.elapsed_secs();
    // Base gains below are the old game's per-`playSfx` values; `sfx`/`voice` (≈ 0.6) is the
    // `audioMix.voice` master every sampled sting passed through. Keep them in sync with
    // `D:/tileworld/src/audio/sfx.ts` if retuning.
    let sfx = cfg.sfx_vol;
    let voice = cfg.voice_vol;
    for cue in cues.read() {
        match *cue {
            AudioCue::Swing => one_shot(&mut commands, bank.swing.clone(), 0.30 * sfx, jitter(&mut seed, 0.12)),
            AudioCue::Impact { kill, crit } => {
                if crit {
                    // The dedicated crit take replaces the flesh pool — louder than any normal
                    // hit, tiny jitter (the clip is the signature; only keep back-to-back crits
                    // from sounding stamped), a crit KILL still drops the pitch heavier.
                    let p = if kill { jitter(&mut seed, 0.04) * 0.9 } else { jitter(&mut seed, 0.05) };
                    one_shot(&mut commands, bank.crit.clone(), 0.72 * sfx, p);
                } else {
                    // Random flesh take + pitch jitter; a kill plays it louder + a touch lower (heavier).
                    let v = if kill { 0.62 } else { 0.50 } * sfx;
                    let p = if kill { jitter(&mut seed, 0.06) * 0.85 } else { jitter(&mut seed, 0.08) };
                    one_shot(&mut commands, pick(&bank.flesh, &mut seed), v, p);
                }
            }
            // Metallic chip per ore pick-swing — random clang + wide pitch jitter so a long mine
            // never repeats the same note (old game's `playPick`).
            AudioCue::OreChip => {
                one_shot(&mut commands, pick(&bank.chips, &mut seed), 0.5 * sfx, jitter(&mut seed, 0.10));
            }
            // A wood-axe chop per swing that bites a tree — random take + pitch jitter so a
            // long chop varies.
            AudioCue::WoodChop => {
                one_shot(&mut commands, pick(&bank.chops, &mut seed), 0.6 * sfx, jitter(&mut seed, 0.12));
            }
            // A tree coming down on the felling blow: woody trees get the full crack+crash, a
            // cactus just the dry crack. Louder than a chop swing (rarer, the kill-stroke) and
            // pitch-jittered lightly so back-to-back fells don't sound identical.
            AudioCue::TreeFall { cactus } => {
                let clip = if cactus { bank.wood_crack.clone() } else { bank.tree_fall.clone() };
                let vol = if cactus { 0.6 } else { 0.85 } * sfx;
                one_shot(&mut commands, clip, vol, jitter(&mut seed, 0.08));
            }
            AudioCue::Block => one_shot(&mut commands, pick(&bank.blocks, &mut seed), 0.45 * sfx, jitter(&mut seed, 0.1)),
            // Sand-Dash whoosh — punchy, tiny pitch jitter so repeat dashes don't sound stamped.
            AudioCue::Dash => one_shot(&mut commands, bank.dash.clone(), 0.6 * sfx, jitter(&mut seed, 0.06)),
            // Dodge-roll tumble grunt — wide-ish pitch jitter so a flurry of rolls varies.
            AudioCue::Roll => one_shot(&mut commands, bank.roll.clone(), 0.7 * sfx, jitter(&mut seed, 0.10)),
            // Bramble-Sweep — the expanding energy-wave burst.
            AudioCue::Sweep => one_shot(&mut commands, bank.sweep.clone(), 0.6 * sfx, jitter(&mut seed, 0.05)),
            // Ground-Slam — random of the two heavy impacts, wide pitch jitter so repeats vary.
            AudioCue::Slam => one_shot(&mut commands, pick(&bank.slams, &mut seed), 0.7 * sfx, jitter(&mut seed, 0.08)),
            AudioCue::Footstep { surface, landing } => {
                let clip = match surface {
                    Surface::Dirt => pick(&bank.foot_dirt, &mut seed),
                    Surface::Snow => bank.foot_snow.clone(),
                    Surface::Stone => bank.foot_stone.clone(),
                };
                // STEP_VOL 0.144; a touchdown step is +20% (LAND_STEP_VOL) — Character.tsx.
                let v = if landing { 0.144 * 1.2 } else { 0.144 } * sfx;
                one_shot(&mut commands, clip, v, jitter(&mut seed, 0.12));
            }
            AudioCue::UiSelect => one_shot(&mut commands, bank.ui.clone(), 0.22 * sfx, jitter(&mut seed, 0.06)),
            // Triumph fanfare — old game's sampled orchestral level-up sting (`playLevelUpFanfare`,
            // vol 0.38). Replaces the synth arpeggio for the hero level-up + landmark/shrine rewards.
            AudioCue::LevelUp => one_shot(&mut commands, bank.level_up.clone(), 0.38 * sfx, jitter(&mut seed, 0.04)),
            AudioCue::OrkGrunt(pos) => {
                if !throttle.allow(T_GRUNT, now) {
                    continue;
                }
                let clip = pick(&bank.ork_grunts, &mut seed);
                spatial_shot(&mut commands, clip, 0.55 * voice, jitter(&mut seed, 0.14), pos);
            }
            AudioCue::OrkRoar(pos) => {
                if !throttle.allow(T_ROAR, now) {
                    continue;
                }
                let clip = pick(&bank.ork_roars, &mut seed);
                spatial_shot(&mut commands, clip, 0.50 * voice, jitter(&mut seed, 0.08), pos);
            }
            // A warden waking / winding up — louder than an ork roar and pitched a touch lower for
            // weight. Not throttled: it's already rare (aggro + crit telegraph only).
            AudioCue::BossRoar(pos) => {
                let clip = pick(&bank.boss_roars, &mut seed);
                spatial_shot(&mut commands, clip, 0.85 * voice, jitter(&mut seed, 0.08) * 0.92, pos);
            }
            // The crit-windup charge whine — pitched a touch DOWN (heavier, ominous) and loud so it
            // cuts through the roar; the player's cue to raise the shield / dodge clear.
            AudioCue::BossWindup(pos) => {
                spatial_shot(&mut commands, bank.boss_windup.clone(), 0.8 * sfx, jitter(&mut seed, 0.05) * 0.85, pos);
                // Layer the rising dread ramp under the charge whine — a synth crescendo that
                // crests as the crit lands (~1.2 s). Spatial so distance attenuates it.
                if let Some(h) = stings.handle(Sting::BossTension) {
                    spatial_shot(&mut commands, h, Sting::BossTension.volume() * sfx, 1.0, pos);
                }
            }
            // A predator's bite snarl — wide pitch jitter so a flurry of bites never repeats. A
            // heavy beast (bear/croc/golem) gets the deeper roar set, louder + pitched down.
            AudioCue::CreatureBite { at, big } => {
                if !throttle.allow(T_BITE, now) {
                    continue;
                }
                let (set, vol, pitch) = if big {
                    (&bank.beast_roars, 0.62, jitter(&mut seed, 0.10) * 0.85)
                } else {
                    (&bank.beast_snarls, 0.5, jitter(&mut seed, 0.16))
                };
                spatial_shot(&mut commands, pick(set, &mut seed), vol * voice, pitch, at);
            }
            // A predator just locked on (idle/graze → hunt): a low stalk-growl, the "you've been
            // seen" tell ~2 s before the charge. Reuses the snarl pool pitched well DOWN so it
            // reads as a warning, not a bite. Throttled with the roars so a pack flip doesn't pile.
            AudioCue::CreatureAggro(at) => {
                if !throttle.allow(T_ROAR, now) {
                    continue;
                }
                spatial_shot(&mut commands, pick(&bank.beast_snarls, &mut seed), 0.55 * voice, jitter(&mut seed, 0.08) * 0.72, at);
            }
            // A town-guard's blow lands on an invader — a quick spatial swing+flesh thud, kept
            // well under the hero's own hit (≈⅓) so nearby militia skirmishes are heard as
            // background clash, not foreground combat. Earshot is gated by the emitter.
            AudioCue::GuardStrike(at) => {
                if !throttle.allow(T_GUARD, now) {
                    continue;
                }
                spatial_shot(&mut commands, bank.swing.clone(), 0.16 * sfx, jitter(&mut seed, 0.14), at);
                spatial_shot(&mut commands, pick(&bank.flesh, &mut seed), 0.26 * sfx, jitter(&mut seed, 0.10), at);
            }
            // An archer's loose — the real sampled bowstring snap + shaft whip (`bow-shot.ogg`;
            // replaced the old pitched-up sword-swing stand-in). Shares the guard-skirmish
            // throttle so a volleying wall doesn't machine-gun the mix.
            AudioCue::BowShot(at) => {
                if !throttle.allow(T_GUARD, now) {
                    continue;
                }
                spatial_shot(&mut commands, bank.bow.clone(), 0.45 * sfx, jitter(&mut seed, 0.08), at);
            }
            // Sampled herb-pick rustle — same 0.35 gain the synth blip used.
            AudioCue::Forage => {
                one_shot(&mut commands, bank.forage.clone(), 0.35 * sfx, jitter(&mut seed, 0.08));
            }
            // The war bell's single hard toll — pitch jitter kept tiny: a bell is one fixed
            // pitch, the jitter only keeps back-to-back rings from sounding stamped.
            AudioCue::WarBell => {
                one_shot(&mut commands, bank.war_bell.clone(), 0.55 * sfx, jitter(&mut seed, 0.02));
            }
            // Procedural synth stings (no clip on disk — baked by `synth.rs`).
            AudioCue::OreShatter
            | AudioCue::ChestOpen
            | AudioCue::Gold
            | AudioCue::ShopBuy
            | AudioCue::CampRescue(_)
            | AudioCue::LowHp => {
                let sting = match *cue {
                    AudioCue::OreShatter => Sting::OreShatter,
                    AudioCue::ChestOpen => Sting::ChestOpen,
                    AudioCue::Gold => Sting::Gold,
                    AudioCue::ShopBuy => Sting::ShopBuy,
                    AudioCue::CampRescue(_) => Sting::CampRescue,
                    _ => Sting::LowHp,
                };
                if let Some(h) = stings.handle(sting) {
                    one_shot(&mut commands, h, sting.volume() * sfx, jitter(&mut seed, 0.05));
                }
            }
            // The fortress war-horn — spatial (it blares from the hold's gate, not the
            // hero's ear), pitch jitter tiny so a horn stays a horn.
            AudioCue::FortressHorn(pos) => {
                spatial_shot(&mut commands, bank.war_horn.clone(), 0.70 * sfx, jitter(&mut seed, 0.03), pos);
            }
            // A warp bolt leaving a shaman staff / fortress tower — short magical release.
            AudioCue::WarpCast(pos) => {
                spatial_shot(&mut commands, bank.warp_cast.clone(), 0.55 * sfx, jitter(&mut seed, 0.12), pos);
            }
            // Distant siege thunder — the synth rumble, head-locked (it's the whole sky, not a
            // point), wide-ish pitch jitter so repeated rolls vary.
            AudioCue::Thunder => {
                if let Some(h) = stings.handle(Sting::Thunder) {
                    one_shot(&mut commands, h, Sting::Thunder.volume() * sfx, jitter(&mut seed, 0.12));
                }
            }
            // A dormant snowman lurching to life — a dedicated "evil snowman waking from a frozen
            // sleep" groan, a touch louder so the "it was a prop a second ago" scare lands. Spatial
            // at the snowman. Not throttled: waking is a rare per-snowman event.
            AudioCue::SnowmanWake(pos) => {
                spatial_shot(&mut commands, pick(&bank.snowman_wake, &mut seed), 0.75 * voice, jitter(&mut seed, 0.06), pos);
            }
            // A snowman's slam landing on the hero — random of the two attack grunts, wide pitch
            // jitter so a flurry never repeats. Shares the bite throttle so a clump of snowmen
            // can't wall the mix. Spatial at the snowman.
            AudioCue::SnowmanSlam(pos) => {
                if !throttle.allow(T_BITE, now) {
                    continue;
                }
                spatial_shot(&mut commands, pick(&bank.snowman, &mut seed), 0.6 * voice, jitter(&mut seed, 0.14), pos);
            }
            // Hero-mouth cues (grunts / jump / hurt / death / lines) are handled by `voice.rs`.
            _ => {}
        }
    }
}
