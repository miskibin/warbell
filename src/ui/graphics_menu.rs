//! **Settings menu** — the full-screen, top-tabbed options screen (CS2-style), reached from a single
//! **Settings** button in both the start screen and the pause menu. Tabs:
//!
//! - **Graphics** — preset chips + render scale + the render-pipeline controls ([`GraphicsSettings`]).
//! - **Display** — window mode / resolution / vsync ([`WindowSettings`]).
//! - **Audio** — Master / Music / SFX volume + Mute ([`AudioSettings`]).
//! - **Controls** — camera mode ([`FirstPerson`]) + a read-only keybind reference.
//!
//! Built on Bevy 0.19's native headless widgets ([`Slider`] / [`Checkbox`]) for the sliders + on/off
//! passes, with the game's segmented-button look for multi-choice rows. Only the **active tab's** rows
//! are spawned, so nothing ever overflows. The widgets are headless (they own drag/toggle/keyboard +
//! emit [`ValueChange`]); we supply the themed tree and read values back into the resources.
//!
//! Opened over an already-frozen screen (Paused / StartScreen), so it needs no `Modal` sub-state; its
//! systems are ungated. `Esc` / the ✕ closes and persists the whole config to disk.

use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::Checked;
use bevy::ui_widgets::{
    slider_self_update, Checkbox, Slider, SliderRange, SliderStep, SliderThumb, SliderValue,
    ValueChange,
};

use crate::player::FirstPerson;
use crate::quality::{
    save_graphics_config, AaLevel, AoLevel, AudioPrefs, GraphicsQuality, GraphicsSettings,
    ShadowLevel, TerrainDetail, WindowSettings,
};

use super::fonts::{label, UiFonts};
use super::settings::AudioSettings;
use super::theme::*;
use super::widgets::{self, border};

/// Open/closed flag for the Settings menu. Flipped true by the pause-menu / start-screen buttons,
/// false by `Esc` or the ✕. The overlay is reconciled from this each frame.
#[derive(Resource, Default)]
pub struct GraphicsMenuOpen(pub bool);

/// Which tab is showing. Clicking a tab sets this; the panel rebuilds its content pane for it.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    #[default]
    Graphics,
    Display,
    Audio,
    Controls,
}

pub struct GraphicsMenuPlugin;

impl Plugin for GraphicsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GraphicsMenuOpen>()
            .init_resource::<SettingsTab>()
            // Headless-widget value plumbing (global observers — only our widgets emit these).
            .add_observer(slider_self_update) // keep SliderValue tracking the drag
            .add_observer(on_slider_change)
            .add_observer(on_toggle_change)
            .add_systems(Startup, stage_open) // FOREST_GFXMENU=1 opens the menu at boot (shot harness)
            .add_systems(
                Update,
                (
                    sync_overlay, // spawn / despawn / rebuild the panel
                    (tab_click, menu_buttons, menu_keys, sync_segments, sync_controls, sync_slider_visual)
                        .run_if(menu_is_open),
                ),
            );
    }
}

fn menu_is_open(open: Res<GraphicsMenuOpen>) -> bool {
    open.0
}

/// `FOREST_GFXMENU=1`: open the Settings menu at boot so the screenshot harness can frame it.
/// `FOREST_GFXTAB=display|audio|controls` picks the starting tab.
fn stage_open(mut open: ResMut<GraphicsMenuOpen>, mut tab: ResMut<SettingsTab>) {
    if std::env::var("FOREST_GFXMENU").is_ok() {
        open.0 = true;
        *tab = match std::env::var("FOREST_GFXTAB").ok().as_deref() {
            Some("display") => SettingsTab::Display,
            Some("audio") => SettingsTab::Audio,
            Some("controls") => SettingsTab::Controls,
            _ => SettingsTab::Graphics,
        };
    }
}

// ── Markers ────────────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
struct GfxMenuRoot;
#[derive(Component)]
struct GfxCloseBtn;
/// A top-bar tab button; click selects its tab.
#[derive(Component, Clone, Copy)]
struct TabBtn(SettingsTab);
/// Tags a tab button's label so the highlight can recolour both.
#[derive(Component)]
struct TabLabel;

/// A boolean (Checkbox) setting.
#[derive(Component, Clone, Copy, PartialEq)]
enum ToggleId {
    Bloom,
    Dof,
    Outline,
    GodRays,
    MotionBlur,
    Vsync,
    Mute,
}
/// Inner "fill" node of a checkbox (the tick).
#[derive(Component)]
struct CheckFill;

/// Which scalar a slider drives.
#[derive(Component, Clone, Copy, PartialEq)]
enum SliderId {
    RenderScale,
    Master,
    Music,
    Sfx,
}
/// Live percentage readout paired with a slider (same id).
#[derive(Component, Clone, Copy)]
struct SliderReadout(SliderId);

/// One segmented-button choice. Highlighted by [`sync_segments`]; a click applies the setting.
#[derive(Component, Clone, Copy, PartialEq)]
enum Seg {
    Preset(GraphicsQuality),
    Shadows(ShadowLevel),
    Aa(AaLevel),
    Ao(AoLevel),
    Terrain(TerrainDetail),
    Resolution(Option<[u32; 2]>),
    Fullscreen(bool),
    /// Camera mode — `true` = first person.
    Camera(bool),
}
/// Tags a segment button's text so [`sync_segments`] recolours it with the button.
#[derive(Component)]
struct SegLabel;

/// Resolutions offered in the Display tab (plus a prepended "Native" = no override).
const RES_CHOICES: &[[u32; 2]] =
    &[[1280, 720], [1600, 900], [1920, 1080], [2560, 1440], [3840, 2160]];

// ── Overlay lifecycle ────────────────────────────────────────────────────────────────────────

/// Spawn the panel when the menu opens, rebuild it when the tab changes, despawn (+ persist) on close.
#[allow(clippy::too_many_arguments)]
fn sync_overlay(
    open: Res<GraphicsMenuOpen>,
    tab: Res<SettingsTab>,
    existing: Query<Entity, With<GfxMenuRoot>>,
    fonts: Res<UiFonts>,
    quality: Res<GraphicsQuality>,
    settings: Res<GraphicsSettings>,
    window: Res<WindowSettings>,
    audio: Res<AudioSettings>,
    first_person: Res<FirstPerson>,
    mut commands: Commands,
    mut built_tab: Local<Option<SettingsTab>>,
) {
    let is_up = !existing.is_empty();
    if open.0 {
        // (Re)build on first open OR when the tab changed.
        if !is_up || *built_tab != Some(*tab) {
            for e in &existing {
                commands.entity(e).despawn();
            }
            spawn_panel(&mut commands, &fonts, *tab, &quality, &settings, &window, &audio, &first_person);
            *built_tab = Some(*tab);
        }
    } else if is_up {
        for e in &existing {
            commands.entity(e).despawn();
        }
        *built_tab = None;
        // Persist on close — a natural commit point (not every slider tick).
        let prefs = AudioPrefs { master: audio.master, music: audio.music, sfx: audio.sfx, muted: audio.muted };
        save_graphics_config(&quality, &settings, &window, &prefs);
    }
}

/// Esc (or the ✕, handled in `menu_buttons`) closes the menu.
fn menu_keys(keys: Res<ButtonInput<KeyCode>>, mut open: ResMut<GraphicsMenuOpen>) {
    if keys.just_pressed(KeyCode::Escape) {
        open.0 = false;
    }
}

// ── Panel construction ───────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn spawn_panel(
    commands: &mut Commands,
    fonts: &UiFonts,
    tab: SettingsTab,
    quality: &GraphicsQuality,
    settings: &GraphicsSettings,
    window: &WindowSettings,
    audio: &AudioSettings,
    first_person: &FirstPerson,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(SCRIM),
            GlobalZIndex(120), // above the pause menu (50) and start screen
            GfxMenuRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    width: Val::Percent(90.0),
                    max_width: Val::Px(940.0),
                    height: Val::Percent(88.0),
                    padding: UiRect::axes(Val::Px(34.0), Val::Px(26.0)),
                    border: border(2.0),
                    border_radius: radius(R_PANEL),
                    ..default()
                },
                widgets::card_paint(),
            ))
            .with_children(|c| {
                // ── Header ──
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(Val::Px(14.0)),
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(label(&fonts.display, "SETTINGS", 28.0, GOLD));
                    widgets::close_button(h, &fonts.bold, GfxCloseBtn, false);
                });

                // ── Tab bar ──
                c.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(6.0),
                        margin: UiRect::bottom(Val::Px(16.0)),
                        ..default()
                    },
                ))
                .with_children(|bar| {
                    for (t, name) in [
                        (SettingsTab::Graphics, "Graphics"),
                        (SettingsTab::Display, "Display"),
                        (SettingsTab::Audio, "Audio"),
                        (SettingsTab::Controls, "Controls"),
                    ] {
                        let on = t == tab;
                        bar.spawn((
                            Button,
                            Interaction::default(),
                            TabBtn(t),
                            Node {
                                padding: UiRect::axes(Val::Px(18.0), Val::Px(9.0)),
                                border_radius: radius(R_BTN),
                                ..default()
                            },
                            BackgroundColor(if on { GOLD_DEEP } else { Color::NONE }),
                        ))
                        .with_children(|b| {
                            b.spawn((label(&fonts.bold, name, 15.0, if on { INK } else { TEXT_FAINT }), TabLabel));
                        });
                    }
                });

                // ── Content pane (active tab only) ──
                c.spawn((Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    row_gap: Val::Px(2.0),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },))
                .with_children(|pane| match tab {
                    SettingsTab::Graphics => graphics_pane(pane, fonts, *quality, settings),
                    SettingsTab::Display => display_pane(pane, fonts, window),
                    SettingsTab::Audio => audio_pane(pane, fonts, audio),
                    SettingsTab::Controls => controls_pane(pane, fonts, first_person),
                });

                c.spawn((
                    label(&fonts.regular, "Esc to close   ·   choices are saved", 12.0, GREY),
                    Node { margin: UiRect::top(Val::Px(10.0)), ..default() },
                ));
            });
        });
}

type Pane<'a> = bevy::ecs::relationship::RelatedSpawnerCommands<'a, ChildOf>;

fn graphics_pane(p: &mut Pane<'_>, fonts: &UiFonts, quality: GraphicsQuality, s: &GraphicsSettings) {
    seg_row(p, fonts, "Preset", &[
        ("Low", Seg::Preset(GraphicsQuality::Low)),
        ("High", Seg::Preset(GraphicsQuality::High)),
        ("Ultra", Seg::Preset(GraphicsQuality::Ultra)),
        ("Custom", Seg::Preset(GraphicsQuality::Custom)),
    ], Seg::Preset(quality));
    divider(p);
    // Max 2.0: above 1.0 supersamples (SSAA) for true edge-AA, below 1.0 trims fragment cost.
    slider_row(p, fonts, "Render scale", SliderId::RenderScale, s.render_scale, 0.3, 2.0);
    seg_row(p, fonts, "Shadows", &[
        ("Off", Seg::Shadows(ShadowLevel::Off)),
        ("Low", Seg::Shadows(ShadowLevel::Low)),
        ("Med", Seg::Shadows(ShadowLevel::Medium)),
        ("High", Seg::Shadows(ShadowLevel::High)),
    ], Seg::Shadows(s.shadows));
    seg_row(p, fonts, "Anti-aliasing", &[
        ("Off", Seg::Aa(AaLevel::Off)),
        ("Low", Seg::Aa(AaLevel::Low)),
        ("High", Seg::Aa(AaLevel::High)),
        ("Ultra", Seg::Aa(AaLevel::Ultra)),
    ], Seg::Aa(s.antialias));
    seg_row(p, fonts, "Ambient occlusion", &[
        ("Off", Seg::Ao(AoLevel::Off)),
        ("Medium", Seg::Ao(AoLevel::Medium)),
        ("Ultra", Seg::Ao(AoLevel::Ultra)),
    ], Seg::Ao(s.ssao));
    seg_row(p, fonts, "Terrain detail", &[
        ("Low", Seg::Terrain(TerrainDetail::Low)),
        ("High", Seg::Terrain(TerrainDetail::High)),
        ("Ultra", Seg::Terrain(TerrainDetail::Ultra)),
    ], Seg::Terrain(s.terrain));
    check_row(p, fonts, "Bloom", ToggleId::Bloom, s.bloom);
    check_row(p, fonts, "Depth of field", ToggleId::Dof, s.depth_of_field);
    check_row(p, fonts, "Outline", ToggleId::Outline, s.outline);
    check_row(p, fonts, "God rays", ToggleId::GodRays, s.god_rays);
    check_row(p, fonts, "Motion blur", ToggleId::MotionBlur, s.motion_blur);
}

fn display_pane(p: &mut Pane<'_>, fonts: &UiFonts, w: &WindowSettings) {
    seg_row(p, fonts, "Window mode", &[
        ("Windowed", Seg::Fullscreen(false)),
        ("Fullscreen", Seg::Fullscreen(true)),
    ], Seg::Fullscreen(w.fullscreen));
    let mut opts: Vec<(String, Seg)> = vec![("Native".into(), Seg::Resolution(None))];
    for r in RES_CHOICES {
        opts.push((format!("{}×{}", r[0], r[1]), Seg::Resolution(Some(*r))));
    }
    let refs: Vec<(&str, Seg)> = opts.iter().map(|(s, v)| (s.as_str(), *v)).collect();
    seg_row(p, fonts, "Resolution", &refs, Seg::Resolution(w.resolution));
    check_row(p, fonts, "VSync", ToggleId::Vsync, w.vsync);
}

fn audio_pane(p: &mut Pane<'_>, fonts: &UiFonts, a: &AudioSettings) {
    slider_row(p, fonts, "Master volume", SliderId::Master, a.master, 0.0, 1.0);
    slider_row(p, fonts, "Music volume", SliderId::Music, a.music, 0.0, 1.0);
    slider_row(p, fonts, "SFX volume", SliderId::Sfx, a.sfx, 0.0, 1.0);
    check_row(p, fonts, "Mute all", ToggleId::Mute, a.muted);
}

fn controls_pane(p: &mut Pane<'_>, fonts: &UiFonts, fp: &FirstPerson) {
    seg_row(p, fonts, "Camera", &[
        ("Third person", Seg::Camera(false)),
        ("First person", Seg::Camera(true)),
    ], Seg::Camera(fp.active));
    divider(p);
    p.spawn((label(&fonts.semibold, "KEYBINDS", FONT_CAPTION_SIZE, KICKER), Node { margin: UiRect::vertical(Val::Px(4.0)), ..default() }));
    for (keys, action) in [
        ("W A S D", "Move"),
        ("LMB", "Attack"),
        ("RMB", "Block / Parry"),
        ("Alt", "Dodge roll"),
        ("E", "Interact"),
        ("F", "Loot"),
        ("I", "Satchel"),
        ("R", "Recruit"),
        ("H", "Help"),
        ("V", "Toggle camera"),
        ("M", "Mute"),
        ("Esc", "Pause"),
    ] {
        p.spawn(row_node()).with_children(|r| {
            r.spawn(label(&fonts.semibold, action, 13.5, TEXT));
            r.spawn(label(&fonts.bold, keys, 13.0, TEXT_FAINT));
        });
    }
}

const FONT_CAPTION_SIZE: f32 = 11.0;

/// A labelled row whose right side is a horizontal segmented control.
fn seg_row(p: &mut Pane<'_>, fonts: &UiFonts, title: &str, options: &[(&str, Seg)], active: Seg) {
    p.spawn(row_node()).with_children(|r| {
        r.spawn(label(&fonts.semibold, title, 14.0, TEXT));
        r.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                padding: UiRect::all(Val::Px(3.0)),
                column_gap: Val::Px(2.0),
                border: border(1.0),
                border_radius: radius(8.0),
                ..default()
            },
            BackgroundColor(rgba(24, 19, 13, 0.72)),
            BorderColor::all(BORDER_SOFT),
        ))
        .with_children(|seg| {
            for (txt, val) in options {
                let on = *val == active;
                seg.spawn((
                    Button,
                    Interaction::default(),
                    Node {
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: radius(6.0),
                        ..default()
                    },
                    BackgroundColor(if on { GOLD_DEEP } else { Color::NONE }),
                    *val,
                ))
                .with_children(|b| {
                    b.spawn((label(&fonts.semibold, *txt, 12.5, if on { INK } else { TEXT_FAINT }), SegLabel));
                });
            }
        });
    });
}

/// A labelled row with a native [`Checkbox`] on the right.
fn check_row(p: &mut Pane<'_>, fonts: &UiFonts, title: &str, id: ToggleId, on: bool) {
    p.spawn(row_node()).with_children(|r| {
        r.spawn(label(&fonts.semibold, title, 14.0, TEXT));
        // Headless checkbox: clicks arrive via the UI picking system (Pointer<Click> bubbling).
        let mut cb = r.spawn((
            Checkbox,
            id,
            Node {
                width: Val::Px(22.0),
                height: Val::Px(22.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: border(2.0),
                border_radius: radius(5.0),
                ..default()
            },
            BackgroundColor(rgba(24, 19, 13, 0.72)),
            BorderColor::all(if on { GOLD_NOTCH } else { BORDER_SOFT }),
        ));
        if on {
            cb.insert(Checked);
        }
        cb.with_children(|b| {
            b.spawn((
                Node { width: Val::Px(12.0), height: Val::Px(12.0), border_radius: radius(3.0), ..default() },
                BackgroundColor(if on { GREEN } else { Color::NONE }),
                CheckFill,
                Pickable::IGNORE, // let the click reach the Checkbox node
            ));
        });
    });
}

/// A labelled row with a native [`Slider`] + a live percentage readout. The rail/thumb are
/// `Pickable::IGNORE` so the drag always targets the Slider node (else the value never commits).
fn slider_row(p: &mut Pane<'_>, fonts: &UiFonts, title: &str, id: SliderId, value: f32, lo: f32, hi: f32) {
    p.spawn(row_node()).with_children(|r| {
        r.spawn(label(&fonts.semibold, title, 14.0, TEXT));
        r.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(12.0),
            ..default()
        })
        .with_children(|right| {
            right.spawn((
                label(&fonts.bold, format!("{}%", (value * 100.0).round() as i32), 13.0, GOLD),
                SliderReadout(id),
                Node { width: Val::Px(42.0), ..default() },
            ));
            right
                .spawn((
                    Slider::default(),
                    SliderValue(value),
                    SliderRange::new(lo, hi),
                    SliderStep(0.05),
                    id,
                    Node {
                        width: Val::Px(200.0),
                        height: Val::Px(18.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                ))
                .with_children(|s| {
                    s.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            right: Val::Px(0.0),
                            top: Val::Px(7.0),
                            height: Val::Px(4.0),
                            border_radius: radius(2.0),
                            ..default()
                        },
                        BackgroundColor(rgba(24, 19, 13, 0.85)),
                        Pickable::IGNORE,
                    ));
                    s.spawn((
                        SliderThumb,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(14.0),
                            height: Val::Px(14.0),
                            top: Val::Px(2.0),
                            left: Val::Percent(((value - lo) / (hi - lo) * 100.0).clamp(0.0, 100.0)),
                            margin: UiRect::left(Val::Px(-7.0)),
                            border_radius: radius(7.0),
                            ..default()
                        },
                        BackgroundColor(GOLD),
                        Pickable::IGNORE,
                    ));
                });
        });
    });
}

fn row_node() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        min_height: Val::Px(36.0),
        column_gap: Val::Px(16.0),
        ..default()
    }
}

fn divider(p: &mut Pane<'_>) {
    p.spawn((
        Node { height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(6.0)), ..default() },
        BackgroundColor(BORDER_SOFT),
    ));
}

// ── Interaction ────────────────────────────────────────────────────────────────────────────────

/// Top-bar tab clicks → select the tab (the panel rebuilds in `sync_overlay`).
fn tab_click(q: Query<(&Interaction, &TabBtn), Changed<Interaction>>, mut tab: ResMut<SettingsTab>) {
    for (i, t) in &q {
        if *i == Interaction::Pressed && *tab != t.0 {
            *tab = t.0;
        }
    }
}

/// Segmented-button clicks + the ✕ close button.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn menu_buttons(
    segs: Query<(&Interaction, &Seg), Changed<Interaction>>,
    close: Query<&Interaction, (Changed<Interaction>, With<GfxCloseBtn>)>,
    mut open: ResMut<GraphicsMenuOpen>,
    mut quality: ResMut<GraphicsQuality>,
    mut settings: ResMut<GraphicsSettings>,
    mut window: ResMut<WindowSettings>,
    mut first_person: ResMut<FirstPerson>,
) {
    for i in &close {
        if *i == Interaction::Pressed {
            open.0 = false;
        }
    }
    for (i, seg) in &segs {
        if *i != Interaction::Pressed {
            continue;
        }
        match *seg {
            Seg::Preset(GraphicsQuality::Custom) => {} // read-only chip
            Seg::Preset(q) => *quality = q,
            Seg::Shadows(l) => set_custom(&mut quality, || settings.shadows = l),
            Seg::Aa(l) => set_custom(&mut quality, || settings.antialias = l),
            Seg::Ao(l) => set_custom(&mut quality, || settings.ssao = l),
            Seg::Terrain(l) => set_custom(&mut quality, || settings.terrain = l),
            Seg::Resolution(r) => window.resolution = r,
            Seg::Fullscreen(fs) => window.fullscreen = fs,
            Seg::Camera(active) => first_person.active = active,
        }
    }
}

/// Apply a render-setting mutation and flip the active preset to `Custom`.
fn set_custom(quality: &mut GraphicsQuality, mutate: impl FnOnce()) {
    mutate();
    *quality = GraphicsQuality::Custom;
}

/// Native checkbox value changed → write the field + flip the tick. Global observer.
#[allow(clippy::too_many_arguments)]
fn on_toggle_change(
    ev: On<ValueChange<bool>>,
    q: Query<&ToggleId>,
    children: Query<&Children>,
    mut fills: Query<&mut BackgroundColor, With<CheckFill>>,
    mut borders: Query<&mut BorderColor>,
    mut commands: Commands,
    mut settings: ResMut<GraphicsSettings>,
    mut window: ResMut<WindowSettings>,
    mut audio: ResMut<AudioSettings>,
    mut quality: ResMut<GraphicsQuality>,
) {
    let src = ev.source;
    let Ok(id) = q.get(src) else { return };
    let on = ev.value;

    if on {
        commands.entity(src).insert(Checked);
    } else {
        commands.entity(src).remove::<Checked>();
    }
    if let Ok(mut bd) = borders.get_mut(src) {
        *bd = BorderColor::all(if on { GOLD_NOTCH } else { BORDER_SOFT });
    }
    for d in children.iter_descendants(src) {
        if let Ok(mut bg) = fills.get_mut(d) {
            bg.0 = if on { GREEN } else { Color::NONE };
        }
    }

    match *id {
        ToggleId::Bloom => set_custom(&mut quality, || settings.bloom = on),
        ToggleId::Dof => set_custom(&mut quality, || settings.depth_of_field = on),
        ToggleId::Outline => set_custom(&mut quality, || settings.outline = on),
        ToggleId::GodRays => set_custom(&mut quality, || settings.god_rays = on),
        ToggleId::MotionBlur => set_custom(&mut quality, || settings.motion_blur = on),
        ToggleId::Vsync => window.vsync = on, // window setting — independent of the render preset
        ToggleId::Mute => audio.muted = on,   // audio setting
    }
}

/// Native slider committed → write the scalar. Render scale only on `is_final` (so a drag doesn't
/// reallocate the render target every frame); audio commits live for instant feedback. Global observer.
fn on_slider_change(
    ev: On<ValueChange<f32>>,
    q: Query<&SliderId>,
    mut settings: ResMut<GraphicsSettings>,
    mut audio: ResMut<AudioSettings>,
    mut quality: ResMut<GraphicsQuality>,
) {
    let Ok(id) = q.get(ev.source) else { return };
    let v = ((ev.value * 100.0).round() / 100.0).clamp(0.0, 1.0);
    match *id {
        SliderId::RenderScale => {
            if ev.is_final && (settings.render_scale - v).abs() > f32::EPSILON {
                set_custom(&mut quality, || settings.render_scale = v);
            }
        }
        SliderId::Master => audio.master = v,
        SliderId::Music => audio.music = v,
        SliderId::Sfx => audio.sfx = v,
    }
}

// ── Live visual sync ─────────────────────────────────────────────────────────────────────────

/// Recolour segmented buttons so the active value (per the live resources) is highlighted.
#[allow(clippy::type_complexity)]
fn sync_segments(
    quality: Res<GraphicsQuality>,
    settings: Res<GraphicsSettings>,
    window: Res<WindowSettings>,
    first_person: Res<FirstPerson>,
    mut segs: Query<(&Seg, &mut BackgroundColor, &Children)>,
    mut labels: Query<&mut TextColor, With<SegLabel>>,
) {
    for (seg, mut bg, kids) in &mut segs {
        let on = match *seg {
            Seg::Preset(q) => *quality == q,
            Seg::Shadows(l) => settings.shadows == l,
            Seg::Aa(l) => settings.antialias == l,
            Seg::Ao(l) => settings.ssao == l,
            Seg::Terrain(l) => settings.terrain == l,
            Seg::Resolution(r) => window.resolution == r,
            Seg::Fullscreen(fs) => window.fullscreen == fs,
            Seg::Camera(active) => first_person.active == active,
        };
        let want = if on { GOLD_DEEP } else { Color::NONE };
        if bg.0 != want {
            bg.0 = want;
        }
        for k in kids {
            if let Ok(mut tc) = labels.get_mut(*k) {
                tc.0 = if on { INK } else { TEXT_FAINT };
            }
        }
    }
}

/// Push resource values into the widgets after a preset fill / cross-control edit. Gated on a change
/// so it never fights an in-progress drag (which commits on release).
#[allow(clippy::type_complexity)]
fn sync_controls(
    settings: Res<GraphicsSettings>,
    window: Res<WindowSettings>,
    audio: Res<AudioSettings>,
    mut commands: Commands,
    sliders: Query<(Entity, &SliderId, &SliderValue)>,
    toggles: Query<(Entity, &ToggleId, Has<Checked>)>,
    mut borders: Query<&mut BorderColor>,
    mut fills: Query<&mut BackgroundColor, With<CheckFill>>,
    children: Query<&Children>,
) {
    if !settings.is_changed() && !window.is_changed() && !audio.is_changed() {
        return;
    }
    for (e, id, cur) in &sliders {
        let want = match *id {
            SliderId::RenderScale => settings.render_scale,
            SliderId::Master => audio.master,
            SliderId::Music => audio.music,
            SliderId::Sfx => audio.sfx,
        };
        if (cur.0 - want).abs() > f32::EPSILON {
            commands.entity(e).insert(SliderValue(want));
        }
    }
    for (e, id, checked) in &toggles {
        let want = match *id {
            ToggleId::Bloom => settings.bloom,
            ToggleId::Dof => settings.depth_of_field,
            ToggleId::Outline => settings.outline,
            ToggleId::GodRays => settings.god_rays,
            ToggleId::MotionBlur => settings.motion_blur,
            ToggleId::Vsync => window.vsync,
            ToggleId::Mute => audio.muted,
        };
        if want != checked {
            if want {
                commands.entity(e).insert(Checked);
            } else {
                commands.entity(e).remove::<Checked>();
            }
        }
        if let Ok(mut bd) = borders.get_mut(e) {
            *bd = BorderColor::all(if want { GOLD_NOTCH } else { BORDER_SOFT });
        }
        for d in children.iter_descendants(e) {
            if let Ok(mut bg) = fills.get_mut(d) {
                bg.0 = if want { GREEN } else { Color::NONE };
            }
        }
    }
}

/// Move each slider's thumb + update its readout when its value changes (drag / keyboard / re-seat).
fn sync_slider_visual(
    sliders: Query<(&SliderId, &SliderValue, &SliderRange, &Children), Changed<SliderValue>>,
    mut thumbs: Query<&mut Node, With<SliderThumb>>,
    mut readouts: Query<(&SliderReadout, &mut Text)>,
) {
    for (id, val, range, kids) in &sliders {
        let pos = range.thumb_position(val.0).clamp(0.0, 1.0);
        for k in kids {
            if let Ok(mut node) = thumbs.get_mut(*k) {
                node.left = Val::Percent(pos * 100.0);
            }
        }
        for (r, mut t) in &mut readouts {
            if r.0 == *id {
                **t = format!("{}%", (val.0 * 100.0).round() as i32);
            }
        }
    }
}
