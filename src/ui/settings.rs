//! **Settings backing** — the *logic* behind the player-facing settings, with **no permanent HUD
//! chrome**. The toggles themselves live in the Escape pause menu (`game_state::spawn_pause_screen`),
//! which calls the `toggle_*` helpers here; the keyboard shortcuts (M mute / F11 fullscreen /
//! F10 graphics; V first-person lives in `player::camera`) drive the same helpers directly, so a
//! setting is always reachable without opening any menu. **mute** drives Bevy's audio sinks,
//! **fullscreen** flips the primary window's [`WindowMode`]; a [`Notice`] confirms each change.
//!
//! The only thing this module spawns is a pair of **debug cheat buttons**, and only when
//! `FOREST_CHEATS` is set — they never ship in a normal player HUD.

use bevy::audio::{AudioSink, AudioSinkPlayback, SpatialAudioSink};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::economy::Bank;
use crate::player::PlayerRes;
use crate::quality::GraphicsQuality;

use super::fonts::{label, UiFonts};
use super::notice::Notice;
use super::theme::*;
use super::widgets::border;

#[derive(Resource)]
pub struct AudioSettings {
    /// Player's manual mute (M key / the Settings **Mute** toggle).
    pub muted: bool,
    /// Background mute: true while the game window isn't focused (CS2-style). Driven by
    /// [`track_window_focus`], kept separate from `muted` so refocusing restores the player's own
    /// mute choice and the pause-menu label never flips on an alt-tab. `sync_mute` ORs the two.
    pub unfocused: bool,
    /// User volume multipliers (`0.0..=1.0`), applied ON TOP of the authored [`AudioConfig`] mix by
    /// [`apply_audio_volumes`]. `1.0` == the authored balance. `master` scales everything; `music`
    /// and `sfx` scale their channels (sfx also covers voice / narration / ambience).
    pub master: f32,
    pub music: f32,
    pub sfx: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { muted: false, unfocused: false, master: 1.0, music: 1.0, sfx: 1.0 }
    }
}

/// The authored `AudioConfig` mix levels, snapshotted once so the user volume sliders scale FROM the
/// tuned balance instead of overwriting it (otherwise a slider at 100% would clobber the careful
/// per-channel mix). Captured on the first run of [`apply_audio_volumes`].
#[derive(Resource, Default)]
struct AudioBaseVols {
    captured: bool,
    sfx: f32,
    music: f32,
    voice: f32,
    narration: f32,
    ambience: f32,
}

/// Debug cheat: grants 1000 of every resource (gold + stone + food + wood) on click.
#[derive(Component)]
struct DebugGrant;
/// Debug cheat: unlocks all five warden boons (the active moves + passives) on click.
#[derive(Component)]
struct DebugBoons;

pub struct SettingsPlugin;
impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        // Seed audio prefs from the saved settings config (master/music/sfx volume + mute).
        let prefs = crate::quality::load_audio_prefs();
        app.insert_resource(AudioSettings {
            muted: prefs.muted,
            unfocused: false,
            master: prefs.master,
            music: prefs.music,
            sfx: prefs.sfx,
        })
            .init_resource::<AudioBaseVols>()
            .add_systems(Startup, setup_cheats)
            .add_systems(
                Update,
                (
                    cheat_click,
                    keys,
                    track_window_focus,
                    sync_mute,
                    // Push the user volume multipliers onto the live AudioConfig mix (every audio
                    // system already reads those fields, so this is the single wiring point).
                    apply_audio_volumes.run_if(resource_changed::<AudioSettings>),
                ),
            );
    }
}

/// Spawn the dev cheat buttons in the top-right — **only** under `FOREST_CHEATS`, so a normal run
/// has no permanent buttons on screen at all (player settings live in the Esc pause menu).
fn setup_cheats(mut commands: Commands, fonts: Res<UiFonts>) {
    if std::env::var("FOREST_CHEATS").is_err() {
        return;
    }
    let cheat_btn = || {
        (
            Node {
                height: Val::Px(34.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: border(1.0),
                border_radius: radius(R_BTN),
                ..default()
            },
            BackgroundColor(PANEL_HUD),
            BorderColor::all(BORDER_SOFT),
            Button,
            Interaction::default(),
        )
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(14.0),
                right: Val::Px(14.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(8.0),
                ..default()
            },
            GlobalZIndex(91),
        ))
        .with_children(|row| {
            row.spawn((cheat_btn(), DebugGrant)).with_children(|b| {
                b.spawn(label(&fonts.bold, "+1k", 13.0, TEXT));
            });
            row.spawn((cheat_btn(), DebugBoons)).with_children(|b| {
                b.spawn(label(&fonts.bold, "Arts", 13.0, TEXT));
            });
        });
}

/// Handle the two debug cheat buttons (only present under `FOREST_CHEATS`).
fn cheat_click(
    q: Query<(&Interaction, Option<&DebugGrant>, Option<&DebugBoons>), Changed<Interaction>>,
    mut bank: ResMut<Bank>,
    mut player: ResMut<PlayerRes>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    for (interaction, grant, boons) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if grant.is_some() {
            grant_debug_resources(&mut bank, &mut player, &mut notice, now);
        }
        if boons.is_some() {
            grant_all_boons(&mut player, &mut notice, now);
        }
    }
}

/// Debug cheat behind the "Arts" button: unlock every warden boon (Ground Slam, Sand Dash,
/// Bramble Sweep, Frostbite, Venom) so the active moves + passives can be tested instantly.
fn grant_all_boons(player: &mut PlayerRes, notice: &mut Notice, now: f64) {
    let p = &mut player.0;
    p.has_ground_slam = true;
    p.has_sand_dash = true;
    p.has_bramble_sweep = true;
    p.frostbite = true;
    p.venom = true;
    notice.push("Debug: all warden abilities unlocked", now);
}

/// Debug cheat behind the "+1k" button: 1000 gold + 1000 of each bank resource.
fn grant_debug_resources(bank: &mut Bank, player: &mut PlayerRes, notice: &mut Notice, now: f64) {
    bank.0.add_stone(1000.0);
    bank.0.add_food(1000.0);
    bank.0.add_wood(1000.0);
    player.0.add_gold(1000);
    notice.push("Debug: +1000 gold/stone/food/wood", now);
}

/// M = mute, F11 = fullscreen, F10 = graphics preset. (V / first-person lives in `player::camera`.)
/// F11 flips [`WindowSettings::fullscreen`] (the single source of truth that the Display settings tab
/// also drives) rather than the live `Window` directly, so the key and the menu never desync.
fn keys(
    input: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<AudioSettings>,
    mut quality: ResMut<GraphicsQuality>,
    mut window: ResMut<crate::quality::WindowSettings>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    if input.just_pressed(KeyCode::KeyM) {
        toggle_mute(&mut settings, &mut notice, now);
    }
    if input.just_pressed(KeyCode::F11) {
        window.fullscreen = !window.fullscreen;
        notice.push(if window.fullscreen { "Fullscreen" } else { "Windowed" }, now);
    }
    if input.just_pressed(KeyCode::F10) {
        toggle_quality(&mut quality, &mut notice, now);
    }
}

pub(crate) fn toggle_mute(settings: &mut AudioSettings, notice: &mut Notice, now: f64) {
    settings.muted = !settings.muted;
    // The actual silencing happens in `sync_mute` (live `AudioSink`s) — GlobalVolume alone is
    // only sampled when a sink *starts*, so it never touches already-playing music/ambience.
    notice.push(if settings.muted { "Audio muted" } else { "Audio on" }, now);
}

pub(crate) fn toggle_quality(quality: &mut GraphicsQuality, notice: &mut Notice, now: f64) {
    *quality = quality.next();
    notice.push(format!("Graphics: {}", quality.label()), now);
}

/// CS2-style background mute: silence the game whenever its window loses focus (alt-tabbed, or
/// another app on top), and unmute the moment it's focused again. Writes [`AudioSettings::unfocused`]
/// — `sync_mute` ORs it with the manual `muted` flag — so the player's own mute choice survives a
/// tab-away and the pause-menu label (which tracks only `muted`) doesn't twitch on focus changes.
/// Only writes on an actual change to avoid needless change-detection churn.
fn track_window_focus(
    window: Query<&Window, With<PrimaryWindow>>,
    mut settings: ResMut<AudioSettings>,
) {
    let Ok(window) = window.single() else { return };
    let unfocused = !window.focused;
    if settings.unfocused != unfocused {
        settings.unfocused = unfocused;
    }
}

/// Keep every live audio sink's mute state matching the setting. Bevy's `GlobalVolume` is only
/// read when a sink is created, so muting must be pushed onto the playing sinks here — this also
/// catches sinks that start while muted (they get muted within a frame). `mute()`/`unmute()`
/// remember each sink's real volume, so unmuting restores it exactly.
fn sync_mute(
    settings: Res<AudioSettings>,
    mut sinks: Query<&mut AudioSink>,
    mut spatial: Query<&mut SpatialAudioSink>,
) {
    let want = settings.muted || settings.unfocused;
    for mut s in &mut sinks {
        if s.is_muted() != want {
            if want {
                s.mute();
            } else {
                s.unmute();
            }
        }
    }
    for mut s in &mut spatial {
        if s.is_muted() != want {
            if want {
                s.mute();
            } else {
                s.unmute();
            }
        }
    }
}

/// Scale the live [`AudioConfig`] mix levels by the user's `master × channel` multipliers. Every
/// audio system (`music`/`sfx`/`director`/`ambience`) reads `AudioConfig.*_vol` live, so writing the
/// scaled values here is the single point that makes the Audio settings sliders audible. The
/// authored mix is snapshotted once into [`AudioBaseVols`] so the sliders scale FROM the balance.
/// `sfx` covers voice + narration + ambience too (everything that isn't music).
fn apply_audio_volumes(
    settings: Res<AudioSettings>,
    mut cfg: ResMut<crate::audio::AudioConfig>,
    mut base: ResMut<AudioBaseVols>,
) {
    if !base.captured {
        base.sfx = cfg.sfx_vol;
        base.music = cfg.music_vol;
        base.voice = cfg.voice_vol;
        base.narration = cfg.narration_vol;
        base.ambience = cfg.ambience_vol;
        base.captured = true;
    }
    let master = settings.master.clamp(0.0, 1.0);
    let sfx = master * settings.sfx.clamp(0.0, 1.0);
    cfg.music_vol = base.music * master * settings.music.clamp(0.0, 1.0);
    cfg.sfx_vol = base.sfx * sfx;
    cfg.voice_vol = base.voice * sfx;
    cfg.narration_vol = base.narration * sfx;
    cfg.ambience_vol = base.ambience * sfx;
}
