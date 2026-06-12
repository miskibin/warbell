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
use bevy::camera::Exposure;
use bevy::light::{FogVolume, VolumetricFog};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;

use crate::dof::Dof;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use crate::audio::AudioConfig;
use crate::outline::Outline;
use crate::scene::{SkyClock, Sun};
use crate::visual::VisualSettings;

/// Whether the panel window is currently shown (hidden by default; F1 toggles).
#[derive(Resource, Default)]
struct DebugPanel {
    open: bool,
}

/// Set true (during the egui pass) whenever egui wants the pointer — i.e. the cursor is
/// over the panel or dragging a widget. The camera controllers read this and skip their
/// cursor-grab / mouse-look so dragging a slider never rotates the world. Updated one frame
/// behind the camera systems, which is fine: you've hovered the panel before you click it.
#[derive(Resource, Default)]
pub struct EguiWantsPointer(pub bool);

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
            .init_resource::<EguiWantsPointer>()
            // `FOREST_HIDEHUD` boots with the HUD already hidden (clean stills/clips).
            .insert_resource(HudHidden(std::env::var("FOREST_HIDEHUD").is_ok()))
            .add_systems(Update, (toggle_panel, apply_hud_hidden))
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

/// Clean-recording toggle: hide ALL game HUD (objective banner, hotbar, HP/stamina bars,
/// interaction prompts, subtitles, toasts) so a trailer shot shows only the 3D scene. **F3**
/// flips it; the Director panel has a matching checkbox. (The F1 egui panel is hidden separately
/// with F1 — it's immediate-mode, not a UI node.) Re-applies whenever the flag changes, so newly
/// spawned HUD nodes are caught on the next toggle.
#[derive(Resource, Default)]
pub struct HudHidden(pub bool);

fn apply_hud_hidden(
    keys: Res<ButtonInput<KeyCode>>,
    mut hidden: ResMut<HudHidden>,
    // Every ROOT UI node (a `Node` with no parent) — toggling its visibility cascades to the whole
    // HUD subtree. World-space entities aren't `Node`s, so the scene itself is untouched.
    mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>)>,
    mut was_hidden: Local<bool>,
) {
    if keys.just_pressed(KeyCode::F3) {
        hidden.0 = !hidden.0;
    }
    if hidden.0 {
        // Re-assert every frame so HUD that spawns AFTER the toggle (toasts, prompts, late banners)
        // is caught too. Idempotent — only writes a root that isn't already hidden.
        for mut vis in &mut roots {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
        }
    } else if *was_hidden {
        // Turned back on: restore once.
        for mut vis in &mut roots {
            *vis = Visibility::Inherited;
        }
    }
    *was_hidden = hidden.0;
}

#[allow(clippy::too_many_arguments)]
fn panel_ui(
    mut contexts: EguiContexts,
    panel: Res<DebugPanel>,
    mut cam: Query<
        (
            &mut DistanceFog,
            &mut Dof,
            &mut Bloom,
            &mut ColorGrading,
            &mut Exposure,
            // Optional: the Low graphics preset removes the outline pass AND the volumetric-fog
            // pass entirely, so these components may be absent — don't let that drop the whole
            // camera row from the panel.
            Option<&mut Outline>,
            Option<&mut VolumetricFog>,
        ),
        With<Camera3d>,
    >,
    mut visual: ResMut<VisualSettings>,
    mut clock: ResMut<SkyClock>,
    mut audio_cfg: ResMut<AudioConfig>,
    mut global_vol: ResMut<GlobalVolume>,
    mut light_override: ResMut<LightOverride>,
    mut sun: Query<&mut DirectionalLight, With<Sun>>,
    mut fog_volume: Query<&mut FogVolume>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut egui_wants: ResMut<EguiWantsPointer>,
    mut director: ResMut<crate::cinematic::DirectorState>,
    mut scene: ResMut<crate::scenes::SceneState>,
    mut hud_hidden: ResMut<HudHidden>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Tell the camera controllers whether egui owns the pointer this frame (cursor over the
    // panel or dragging a widget) so they don't grab the cursor / rotate the view.
    egui_wants.0 = ctx.wants_pointer_input() || ctx.is_pointer_over_area();
    if !panel.open {
        return Ok(());
    }

    egui::Window::new("Debug")
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.label("F1 toggles this panel");

            // ── Trailer Director: staged scenes/animations to film with the free-cam (`). ──
            egui::CollapsingHeader::new("🎬 Director").show(ui, |ui| {
                // Clean-recording: hide all game HUD (also bound to F3). Only write on a real change
                // so we don't re-hide every frame.
                let mut h = hud_hidden.0;
                if ui.checkbox(&mut h, "Hide HUD for recording (F3)").changed() {
                    hud_hidden.0 = h;
                }
                use crate::cinematic::HeroGesture as G;
                ui.checkbox(&mut director.sky_run, "Day→night→dawn timelapse");
                ui.add(egui::Slider::new(&mut director.sky_speed, 0.01..=0.25).text("sky speed"));
                ui.separator();
                ui.label("Hero gesture (stand still, then pick):");
                ui.horizontal(|ui| {
                    if ui.button("None").clicked() { director.gesture = None; }
                    if ui.button("Wave").clicked() { director.gesture = Some(G::Wave); }
                    if ui.button("Salute").clicked() { director.gesture = Some(G::Salute); }
                });
                ui.horizontal(|ui| {
                    if ui.button("Point").clicked() { director.gesture = Some(G::Point); }
                    if ui.button("Arms-cross").clicked() { director.gesture = Some(G::ArmsCrossed); }
                });
                ui.horizontal(|ui| {
                    if ui.button("Cheer").clicked() { director.gesture = Some(G::Cheer); }
                    if ui.button("Work").clicked() { director.gesture = Some(G::Work); }
                });
                ui.checkbox(&mut director.hide_weapon, "Hide hero weapon");
                ui.separator();
                if ui.button("Build stronghold (timelapse)").clicked() { director.build_run = true; }
                ui.horizontal(|ui| {
                    if ui.button("March orks from fortress").clicked() { director.march = true; }
                    if ui.button("Clear marchers").clicked() { director.clear_marchers = true; }
                });
                ui.checkbox(&mut director.gate_open, "Fortress gate open");
                ui.separator();
                ui.label("Staged scenes (looped tableaus):");
                use crate::scenes::SceneId;
                // (label, id) — a click toggles that scene on/off.
                let want = scene.want;
                let mut next = want;
                for row in [
                    [("Work site", SceneId::WorkSite), ("Wall patrol", SceneId::WallPatrol)],
                    [("Orks flee", SceneId::OrksFlee), ("Night siege", SceneId::NightSiege)],
                    [("Barrel peek", SceneId::BarrelPeek), ("Mason gag", SceneId::Mason)],
                ] {
                    ui.horizontal(|ui| {
                        for (label, id) in row {
                            if ui.selectable_label(want == Some(id), label).clicked() {
                                next = if want == Some(id) { None } else { Some(id) };
                            }
                        }
                    });
                }
                if ui.button("Clear scene").clicked() { next = None; }
                scene.want = next;
            });

            if let Ok((
                mut fog,
                mut dof,
                mut bloom,
                mut grading,
                mut exposure,
                mut outline,
                mut volfog,
            )) = cam.single_mut()
            {
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
                    // ── God-rays (volumetric light shafts) ──
                    // `step_count` is the GPU cost; the FogVolume knobs below are what make the
                    // shafts actually VISIBLE: density = how much fog there is to scatter, scatter
                    // = how much light bends toward the eye, forward = concentrate it into beams
                    // aimed at the sun, brightness = nonphysical pop.
                    ui.separator();
                    ui.label("God-rays (volumetric)");
                    if let Some(volfog) = volfog.as_mut() {
                        let mut steps = volfog.step_count;
                        if ui
                            .add(egui::Slider::new(&mut steps, 1..=64).text("steps (GPU cost)"))
                            .changed()
                        {
                            volfog.step_count = steps;
                        }
                    } else {
                        ui.weak("pass off (Low graphics preset)");
                    }
                    if let Ok(mut fv) = fog_volume.single_mut() {
                        ui.add(egui::Slider::new(&mut fv.density_factor, 0.0..=0.5).text("density (amount)"));
                        ui.add(egui::Slider::new(&mut fv.scattering, 0.0..=1.0).text("scattering (toward eye)"));
                        ui.add(egui::Slider::new(&mut fv.scattering_asymmetry, 0.0..=0.99).text("forward (toward sun)"));
                        ui.add(egui::Slider::new(&mut fv.light_intensity, 0.0..=4.0).text("brightness"));
                        ui.add(egui::Slider::new(&mut fv.absorption, 0.0..=1.0).text("absorption (darkening)"));
                    }
                });

                egui::CollapsingHeader::new("Bokeh DoF + Bloom").show(ui, |ui| {
                    // Focus distance is auto-driven onto the player (drive_dof_focus); these
                    // tune the blur. Smaller sharp band + bigger radius = more bokeh.
                    ui.label(format!("focal: {:.1} tiles (auto, on player)", dof.focal));
                    ui.add(egui::Slider::new(&mut dof.range, 0.0..=80.0).text("sharp band (tiles)"));
                    ui.add(egui::Slider::new(&mut dof.far_ramp, 10.0..=250.0).text("far falloff (big=gradual)"));
                    ui.add(egui::Slider::new(&mut dof.max_radius, 0.0..=60.0).text("blur radius px"));
                    let mut coc_debug = dof.debug_view > 0.5;
                    ui.checkbox(&mut coc_debug, "show CoC (white=blurred, black=sharp)");
                    dof.debug_view = if coc_debug { 1.0 } else { 0.0 };
                    ui.add(egui::Slider::new(&mut bloom.intensity, 0.0..=1.0).text("bloom"));
                });

                egui::CollapsingHeader::new("Render").default_open(true).show(ui, |ui| {
                    ui.label("Exposure / colour grade");
                    ui.add(egui::Slider::new(&mut exposure.ev100, 7.0..=13.0).text("exposure ev100"));
                    ui.add(egui::Slider::new(&mut grading.global.post_saturation, 0.5..=2.0).text("saturation"));
                    ui.add(egui::Slider::new(&mut grading.shadows.contrast, 0.5..=1.5).text("shadow contrast"));
                    ui.add(egui::Slider::new(&mut grading.midtones.contrast, 0.5..=1.5).text("mid contrast"));
                    ui.add(egui::Slider::new(&mut grading.highlights.contrast, 0.5..=1.5).text("high contrast"));

                    ui.separator();
                    ui.label("Outline (crisp edges)");
                    if let Some(outline) = outline.as_mut() {
                        ui.add(egui::Slider::new(&mut outline.strength, 0.0..=1.0).text("outline strength"));
                        ui.add(egui::Slider::new(&mut outline.thickness, 1.0..=4.0).text("outline thickness"));
                        ui.add(egui::Slider::new(&mut outline.depth_threshold, 0.005..=0.2).text("silhouette sens"));
                        ui.add(egui::Slider::new(&mut outline.normal_threshold, 0.1..=1.2).text("crease sens"));
                    } else {
                        ui.weak("pass off (Low graphics preset)");
                    }

                    ui.separator();
                    ui.label("Pollen + prop specular");
                    // Temp-then-write so the resource is only marked changed on an actual edit
                    // (the apply system iterates materials, so we don't want per-frame churn).
                    let mut glow = visual.pollen_glow;
                    if ui.add(egui::Slider::new(&mut glow, 0.0..=8.0).text("pollen glow")).changed() {
                        visual.pollen_glow = glow;
                    }
                    let mut pspeed = visual.pollen_speed;
                    if ui.add(egui::Slider::new(&mut pspeed, 0.0..=3.0).text("pollen speed")).changed() {
                        visual.pollen_speed = pspeed;
                    }
                    let mut rough = visual.prop_roughness;
                    if ui.add(egui::Slider::new(&mut rough, 0.0..=1.0).text("prop roughness")).changed() {
                        visual.prop_roughness = rough;
                    }
                    let mut refl = visual.prop_reflectance;
                    if ui.add(egui::Slider::new(&mut refl, 0.0..=1.0).text("prop reflectance")).changed() {
                        visual.prop_reflectance = refl;
                    }
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
