//! Live debug-tuning panel — an egui window (toggle with **F1**) for tweaking the look at
//! runtime instead of editing constants / env vars and restarting. Mirrors the TS game's
//! `leva` panel. Hidden by default so it never shows in screenshots or normal viewing.
//!
//! Each frame (when open) it reads the current values straight off the live
//! components/resources, renders sliders, and writes any changes back the same frame — no
//! separate state to keep in sync. Nothing is persisted; values reset on restart.
//!
//! Self-contained: the only thing it needs from elsewhere is read/write access to existing
//! components ([`DistanceFog`], [`DepthBlur`](crate::depth_blur::DepthBlur), [`Bloom`]) and
//! resources ([`SkyClock`](crate::scene::SkyClock), [`GlobalAmbientLight`],
//! [`AudioConfig`](crate::audio::AudioConfig), [`GlobalVolume`]).

use bevy::audio::{GlobalVolume, Volume};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use crate::audio::AudioConfig;
use crate::depth_blur::DepthBlur;
use crate::scene::{SkyClock, Sun};

/// Whether the panel window is currently shown (hidden by default; F1 toggles).
#[derive(Resource, Default)]
struct DebugPanel {
    open: bool,
}

/// When `enabled`, the panel's sun/ambient sliders override the day-night cycle's computed
/// values (applied after `advance_sky`). When off, the cycle drives them as normal.
#[derive(Resource)]
struct LightOverride {
    enabled: bool,
    illuminance: f32,
    ambient: f32,
}

impl Default for LightOverride {
    fn default() -> Self {
        Self { enabled: false, illuminance: 10_000.0, ambient: 120.0 }
    }
}

pub struct DebugPanelPlugin;

impl Plugin for DebugPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            // Opens on launch if `FOREST_PANEL` is set (handy for screenshots); else F1.
            .insert_resource(DebugPanel { open: std::env::var("FOREST_PANEL").is_ok() })
            .init_resource::<LightOverride>()
            .add_systems(Update, toggle_panel)
            // The egui pass runs after `Update`, so the lighting override here lands after
            // `advance_sky` has set the cycle values — letting the panel win when enabled.
            .add_systems(EguiPrimaryContextPass, panel_ui);
    }
}

fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut panel: ResMut<DebugPanel>) {
    if keys.just_pressed(KeyCode::F1) {
        panel.open = !panel.open;
    }
}

#[allow(clippy::too_many_arguments)]
fn panel_ui(
    mut contexts: EguiContexts,
    panel: Res<DebugPanel>,
    mut cam: Query<(&mut DistanceFog, &mut DepthBlur, &mut Bloom), With<Camera3d>>,
    mut clock: ResMut<SkyClock>,
    mut audio_cfg: ResMut<AudioConfig>,
    mut global_vol: ResMut<GlobalVolume>,
    mut light_override: ResMut<LightOverride>,
    mut sun: Query<&mut DirectionalLight, With<Sun>>,
    mut ambient: ResMut<GlobalAmbientLight>,
) -> Result {
    if !panel.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;

    egui::Window::new("Debug")
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.label("F1 toggles this panel");

            if let Ok((mut fog, mut blur, mut bloom)) = cam.single_mut() {
                egui::CollapsingHeader::new("Fog").default_open(true).show(ui, |ui| {
                    // Fog uses a Linear falloff (clear within `start`, full by `end`).
                    let (mut start, mut end) = match fog.falloff {
                        FogFalloff::Linear { start, end } => (start, end),
                        _ => (70.0, 160.0),
                    };
                    let mut changed = ui.add(egui::Slider::new(&mut start, 0.0..=300.0).text("clear")).changed();
                    changed |= ui.add(egui::Slider::new(&mut end, 10.0..=600.0).text("full")).changed();
                    if changed {
                        fog.falloff = FogFalloff::Linear { start, end: end.max(start + 1.0) };
                    }
                });

                egui::CollapsingHeader::new("Blur + Bloom").show(ui, |ui| {
                    ui.add(egui::Slider::new(&mut blur.clear, 0.0..=300.0).text("blur clear"));
                    ui.add(egui::Slider::new(&mut blur.full, 0.0..=600.0).text("blur full"));
                    ui.add(egui::Slider::new(&mut blur.radius, 0.0..=12.0).text("blur radius"));
                    ui.add(egui::Slider::new(&mut blur.near, 0.0..=60.0).text("blur near"));
                    ui.add(egui::Slider::new(&mut bloom.intensity, 0.0..=1.0).text("bloom"));
                });
            }

            egui::CollapsingHeader::new("Time / Sun").show(ui, |ui| {
                ui.add(egui::Slider::new(&mut clock.t, 0.0..=1.0).text("time (0=dawn)"));
                ui.checkbox(&mut clock.paused, "pause cycle");
                ui.add(egui::Slider::new(&mut clock.day_secs, 5.0..=600.0).text("cycle secs"));
                ui.separator();
                ui.checkbox(&mut light_override.enabled, "override lighting");
                if light_override.enabled {
                    ui.add(egui::Slider::new(&mut light_override.illuminance, 0.0..=20_000.0).text("sun lux"));
                    ui.add(egui::Slider::new(&mut light_override.ambient, 0.0..=400.0).text("ambient"));
                }
            });

            egui::CollapsingHeader::new("Audio").show(ui, |ui| {
                let mut master = if let Volume::Linear(l) = global_vol.volume { l } else { 1.0 };
                if ui.add(egui::Slider::new(&mut master, 0.0..=2.0).text("master")).changed() {
                    global_vol.volume = Volume::Linear(master);
                }
                ui.add(egui::Slider::new(&mut audio_cfg.ambience_vol, 0.0..=1.0).text("ambience"));
                ui.add(egui::Slider::new(&mut audio_cfg.audible_range, 5.0..=80.0).text("call range"));
                ui.add(egui::Slider::new(&mut audio_cfg.call_min, 2.0..=120.0).text("call min s"));
                ui.add(egui::Slider::new(&mut audio_cfg.call_max, 5.0..=200.0).text("call max s"));
                ui.separator();
                ui.add(egui::Slider::new(&mut audio_cfg.sfx_vol, 0.0..=1.5).text("sfx"));
                ui.add(egui::Slider::new(&mut audio_cfg.voice_vol, 0.0..=1.5).text("voice"));
                ui.add(egui::Slider::new(&mut audio_cfg.music_vol, 0.0..=1.0).text("music"));
                ui.add(egui::Slider::new(&mut audio_cfg.narration_vol, 0.0..=1.5).text("narration"));
                ui.add(egui::Slider::new(&mut audio_cfg.combat_music, 0.0..=2.0).text("combat music"));
            });
        });

    // Apply the lighting override after the day-night cycle (this pass runs post-`Update`).
    if light_override.enabled {
        if let Ok(mut light) = sun.single_mut() {
            light.illuminance = light_override.illuminance;
        }
        ambient.brightness = light_override.ambient;
    }

    Ok(())
}
