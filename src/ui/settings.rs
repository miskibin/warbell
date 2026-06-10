//! **Settings** — the always-visible top-right toggles ported from the 3js `AudioToggle` /
//! `SettingsPanel`. Two icon buttons with real backing: **mute** drives Bevy's [`GlobalVolume`],
//! **fullscreen** flips the primary window's [`WindowMode`]. Each is also reachable from the keyboard
//! (M / F11) and a [`Notice`] confirms the change.

use bevy::audio::{AudioSink, AudioSinkPlayback, SpatialAudioSink};
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};

use crate::economy::Bank;
use crate::player::PlayerRes;
use crate::quality::GraphicsQuality;

use super::fonts::{label, UiFonts};
use super::icons::IconAtlas;
use super::notice::Notice;
use super::theme::*;
use super::widgets::border;

#[derive(Resource, Default)]
pub struct AudioSettings {
    pub muted: bool,
}

#[derive(Component)]
struct AudioToggle;
#[derive(Component)]
struct AudioIcon;
#[derive(Component)]
struct FullscreenToggle;
#[derive(Component)]
struct FsIcon;
/// The Low/High graphics-preset button, and the text label inside it.
#[derive(Component)]
struct QualityToggle;
/// Debug cheat: grants 1000 of every resource (gold + stone + food + wood) on click.
#[derive(Component)]
struct DebugGrant;
#[derive(Component)]
struct QualityLabel;

pub struct SettingsPlugin;
impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .add_systems(Startup, setup_settings)
            .add_systems(
                Update,
                (settings_click, keys, sync_audio_icon, sync_mute, sync_quality_label),
            );
    }
}

fn setup_settings(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(14.0),
            right: Val::Px(14.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            // Debug cheat: "+1k" text button grants 1000 of every resource for testing.
            row.spawn((
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
                DebugGrant,
            ))
            .with_children(|b| {
                b.spawn(label(&fonts.bold, "+1k", 13.0, TEXT));
            });
            // Graphics-quality toggle: a text button ("High"/"Low") so the choice is explicit and
            // legible without depending on an icon asset. Click or press F10 to flip.
            row.spawn((
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
                QualityToggle,
            ))
            .with_children(|b| {
                b.spawn((label(&fonts.bold, "High", 13.0, TEXT), QualityLabel));
            });
            for marker in [0u8, 1] {
                let mut e = row.spawn((
                    Node {
                        width: Val::Px(34.0),
                        height: Val::Px(34.0),
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
                ));
                if marker == 0 {
                    e.insert(AudioToggle).with_children(|b| {
                        b.spawn((Node { width: Val::Px(18.0), height: Val::Px(18.0), ..default() }, ImageNode::new(Handle::default()), AudioIcon));
                    });
                } else {
                    e.insert(FullscreenToggle).with_children(|b| {
                        b.spawn((Node { width: Val::Px(18.0), height: Val::Px(18.0), ..default() }, ImageNode::new(Handle::default()), FsIcon));
                    });
                }
            }
        });
}

/// Keep the fullscreen button's icon set, and the audio button's icon in sync with the mute state
/// (also covers the startup race where the icon atlas isn't ready when the buttons spawn).
fn sync_audio_icon(
    settings: Res<AudioSettings>,
    atlas: Res<IconAtlas>,
    mut audio_q: Query<&mut ImageNode, (With<AudioIcon>, Without<FsIcon>)>,
    mut fs_q: Query<&mut ImageNode, (With<FsIcon>, Without<AudioIcon>)>,
) {
    let key = if settings.muted { "sym:audio_off" } else { "sym:audio_on" };
    if let (Ok(mut img), Some(h)) = (audio_q.single_mut(), atlas.get(key)) {
        if img.image != h {
            img.image = h;
        }
    }
    if let (Ok(mut img), Some(h)) = (fs_q.single_mut(), atlas.get("sym:fullscreen")) {
        if img.image != h {
            img.image = h;
        }
    }
}

#[allow(clippy::type_complexity)]
fn settings_click(
    q: Query<
        (
            &Interaction,
            Option<&AudioToggle>,
            Option<&FullscreenToggle>,
            Option<&QualityToggle>,
            Option<&DebugGrant>,
        ),
        Changed<Interaction>,
    >,
    mut settings: ResMut<AudioSettings>,
    mut quality: ResMut<GraphicsQuality>,
    mut bank: ResMut<Bank>,
    mut player: ResMut<PlayerRes>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    for (interaction, audio, fs, qual, grant) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if audio.is_some() {
            toggle_mute(&mut settings, &mut notice, now);
        }
        if fs.is_some() {
            toggle_fullscreen(&mut windows, &mut notice, now);
        }
        if qual.is_some() {
            toggle_quality(&mut quality, &mut notice, now);
        }
        if grant.is_some() {
            grant_debug_resources(&mut bank, &mut player, &mut notice, now);
        }
    }
}

/// Debug cheat behind the "+1k" button: 1000 gold + 1000 of each bank resource.
fn grant_debug_resources(bank: &mut Bank, player: &mut PlayerRes, notice: &mut Notice, now: f64) {
    bank.0.add_stone(1000.0);
    bank.0.add_food(1000.0);
    bank.0.add_wood(1000.0);
    player.0.add_gold(1000);
    notice.push("Debug: +1000 gold/stone/food/wood", now);
}

/// M = mute, F11 = fullscreen, F10 = graphics preset.
fn keys(
    input: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<AudioSettings>,
    mut quality: ResMut<GraphicsQuality>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    if input.just_pressed(KeyCode::KeyM) {
        toggle_mute(&mut settings, &mut notice, now);
    }
    if input.just_pressed(KeyCode::F11) {
        toggle_fullscreen(&mut windows, &mut notice, now);
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

/// Keep every live audio sink's mute state matching the setting. Bevy's `GlobalVolume` is only
/// read when a sink is created, so muting must be pushed onto the playing sinks here — this also
/// catches sinks that start while muted (they get muted within a frame). `mute()`/`unmute()`
/// remember each sink's real volume, so unmuting restores it exactly.
fn sync_mute(
    settings: Res<AudioSettings>,
    mut sinks: Query<&mut AudioSink>,
    mut spatial: Query<&mut SpatialAudioSink>,
) {
    let want = settings.muted;
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

/// Reflect the active graphics preset on the toggle button's label.
fn sync_quality_label(
    quality: Res<GraphicsQuality>,
    mut q: Query<&mut Text, With<QualityLabel>>,
) {
    if !quality.is_changed() {
        return;
    }
    if let Ok(mut t) = q.single_mut() {
        **t = quality.label().to_string();
    }
}

pub(crate) fn toggle_fullscreen(
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    notice: &mut Notice,
    now: f64,
) {
    let Ok(mut window) = windows.single_mut() else { return };
    let to_full = matches!(window.mode, WindowMode::Windowed);
    window.mode = if to_full {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    notice.push(if to_full { "Fullscreen" } else { "Windowed" }, now);
}
