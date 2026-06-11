//! One-shot stings — combat feedback, the UI blip, footsteps, and the spatial creature
//! voices. Reads the [`AudioCue`] stream and spawns a short-lived `PlaybackMode::Despawn`
//! sink per cue (Bevy frees it when the clip ends — the equivalent of the old game's SFX
//! pool). Non-spatial cues play head-locked; ork voices spawn at a world position so the
//! camera's `SpatialListener` pans + attenuates them.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::synth::{Sting, StingBank};
use super::{jitter, pick, AudioConfig, AudioCue, Surface};

/// All one-shot SFX handles, loaded once at startup.
#[derive(Resource)]
pub(crate) struct SfxBank {
    swing: Handle<AudioSource>,
    /// The meaty blade-on-flesh impact (old `sword-hit-var-2`) — every creature/ork hit uses
    /// THIS one clip, pitch-jittered so repeats don't sound canned (old game's rule).
    flesh: Handle<AudioSource>,
    /// Metallic chips (old `sword-hit-var-{1,3}`) — picked at random for chipping stone (ore).
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
    block: Handle<AudioSource>,
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
    /// Gnashfang Hold's war-horn (`war-horn.ogg` — wood crack, then a deep horn blast).
    war_horn: Handle<AudioSource>,
    /// Warp-bolt release (`warp-cast.ogg`) — shaman staff casts + fortress tower fire.
    warp_cast: Handle<AudioSource>,
    /// Aggressive beast snarls played when a wild predator bites the hero (Wolf/Boar/Scorpion/
    /// BogCroc have no recorded voice, so they share these). Pitch-jittered per bite.
    beast_snarls: Vec<Handle<AudioSource>>,
    beast_roars: Vec<Handle<AudioSource>>,
}

pub(crate) fn setup_sfx(asset: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(SfxBank {
        swing: asset.load("audio/sword-swing.ogg"),
        flesh: asset.load("audio/sword-hit-2.ogg"),
        chips: ["audio/sword-hit-1.ogg", "audio/sword-hit-3.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        chops: ["audio/chop-wood-1.ogg", "audio/chop-wood-2.ogg", "audio/chop-wood-3.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        tree_fall: asset.load("audio/tree-fall.ogg"),
        wood_crack: asset.load("audio/wood-crack.ogg"),
        block: asset.load("audio/block.ogg"),
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
        war_horn: asset.load("audio/war-horn.ogg"),
        warp_cast: asset.load("audio/warp-cast.ogg"),
        beast_snarls: ["audio/monster-snarl.ogg", "audio/monster-growl.ogg", "audio/bear-growl.ogg"]
            .iter()
            .map(|f| asset.load(*f))
            .collect(),
        beast_roars: ["audio/bear-roar.ogg", "audio/monster-growl.ogg"].iter().map(|f| asset.load(*f)).collect(),
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
    cfg: Res<AudioConfig>,
    bank: Res<SfxBank>,
    stings: Res<StingBank>,
    mut seed: Local<u32>,
    mut cues: MessageReader<AudioCue>,
) {
    // Base gains below are the old game's per-`playSfx` values; `sfx`/`voice` (≈ 0.6) is the
    // `audioMix.voice` master every sampled sting passed through. Keep them in sync with
    // `D:/tileworld/src/audio/sfx.ts` if retuning.
    let sfx = cfg.sfx_vol;
    let voice = cfg.voice_vol;
    for cue in cues.read() {
        match *cue {
            AudioCue::Swing => one_shot(&mut commands, bank.swing.clone(), 0.30 * sfx, jitter(&mut seed, 0.12)),
            AudioCue::Impact { kill } => {
                // Always the one flesh clip; a kill plays it louder + a touch lower (heavier).
                let v = if kill { 0.62 } else { 0.50 } * sfx;
                let p = if kill { jitter(&mut seed, 0.06) * 0.85 } else { jitter(&mut seed, 0.08) };
                one_shot(&mut commands, bank.flesh.clone(), v, p);
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
            AudioCue::Block => one_shot(&mut commands, bank.block.clone(), 0.45 * sfx, jitter(&mut seed, 0.1)),
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
                let clip = pick(&bank.ork_grunts, &mut seed);
                spatial_shot(&mut commands, clip, 0.55 * voice, jitter(&mut seed, 0.14), pos);
            }
            AudioCue::OrkRoar(pos) => {
                let clip = pick(&bank.ork_roars, &mut seed);
                spatial_shot(&mut commands, clip, 0.50 * voice, jitter(&mut seed, 0.08), pos);
            }
            // A predator's bite snarl — wide pitch jitter so a flurry of bites never repeats. A
            // heavy beast (bear/croc/golem) gets the deeper roar set, louder + pitched down.
            AudioCue::CreatureBite { at, big } => {
                let (set, vol, pitch) = if big {
                    (&bank.beast_roars, 0.62, jitter(&mut seed, 0.10) * 0.85)
                } else {
                    (&bank.beast_snarls, 0.5, jitter(&mut seed, 0.16))
                };
                spatial_shot(&mut commands, pick(set, &mut seed), vol * voice, pitch, at);
            }
            // A town-guard's blow lands on an invader — a quick spatial swing+flesh thud, kept
            // well under the hero's own hit (≈⅓) so nearby militia skirmishes are heard as
            // background clash, not foreground combat. Earshot is gated by the emitter.
            AudioCue::GuardStrike(at) => {
                spatial_shot(&mut commands, bank.swing.clone(), 0.16 * sfx, jitter(&mut seed, 0.14), at);
                spatial_shot(&mut commands, bank.flesh.clone(), 0.26 * sfx, jitter(&mut seed, 0.10), at);
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
            | AudioCue::CampRescue
            | AudioCue::LowHp => {
                let sting = match *cue {
                    AudioCue::OreShatter => Sting::OreShatter,
                    AudioCue::ChestOpen => Sting::ChestOpen,
                    AudioCue::Gold => Sting::Gold,
                    AudioCue::ShopBuy => Sting::ShopBuy,
                    AudioCue::CampRescue => Sting::CampRescue,
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
            // Hero-mouth cues (grunts / jump / hurt / death / lines) are handled by `voice.rs`.
            _ => {}
        }
    }
}
