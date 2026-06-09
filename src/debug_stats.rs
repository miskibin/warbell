//! Developer **stats overlay** — an egui window (toggle with **F2**) showing live performance
//! and game-state telemetry for debugging. Distinct from `debug_panel.rs` (F1), which *tunes*
//! the look; this one is read-only *instrumentation*: FPS + a frame-time graph, entity/actor
//! counts, the state machine (AppState / Modal / siege phase), and the hero's vitals.
//!
//! It pulls FPS / frame-time / entity-count from Bevy's own diagnostics (registered here) and
//! reads everything else straight off the live resources/components, so there's nothing to keep
//! in sync. Hidden by default; never shows in screenshots or normal play.

use bevy::diagnostic::{
    DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::economy::Bank;
use crate::game_state::{AppState, Modal};
use crate::orks::{Ork, WaveInvader};
use crate::player::{Hero, PlayerRes};
use crate::siege::{GamePhase, KeepHp, Siege};
use crate::wildlife::Animal;

/// Whether the overlay is currently shown (hidden by default; F2 toggles).
#[derive(Resource, Default)]
struct StatsPanel {
    open: bool,
}

pub struct DebugStatsPlugin;

impl Plugin for DebugStatsPlugin {
    fn build(&self, app: &mut App) {
        // Register the FPS / frame-time / entity-count diagnostics the overlay reads. (Bevy's
        // `EguiPlugin` is already added by `debug_panel`, so we don't add it again.)
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
        ))
        .init_resource::<StatsPanel>()
        .add_systems(Update, toggle_panel)
        .add_systems(EguiPrimaryContextPass, stats_ui);
    }
}

fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut panel: ResMut<StatsPanel>) {
    if keys.just_pressed(KeyCode::F2) {
        panel.open = !panel.open;
    }
}

#[allow(clippy::too_many_arguments)]
fn stats_ui(
    mut contexts: EguiContexts,
    panel: Res<StatsPanel>,
    diags: Res<DiagnosticsStore>,
    app_state: Res<State<AppState>>,
    modal: Option<Res<State<Modal>>>,
    siege: Option<Res<Siege>>,
    keep: Option<Res<KeepHp>>,
    player: Option<Res<PlayerRes>>,
    bank: Option<Res<Bank>>,
    hero_q: Query<&Hero>,
    orks_q: Query<(), (With<Ork>, Without<WaveInvader>, Without<crate::dying::Dying>)>,
    invaders_q: Query<(), (With<WaveInvader>, Without<crate::dying::Dying>)>,
    animals_q: Query<(), (With<Animal>, Without<crate::dying::Dying>)>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !panel.open {
        return Ok(());
    }

    egui::Window::new("Stats (F2)")
        .default_width(260.0)
        .resizable(false)
        .show(ctx, |ui| {
            // ── Performance ──────────────────────────────────────────────────────────
            let fps = diags
                .get(&FrameTimeDiagnosticsPlugin::FPS)
                .and_then(|d| d.smoothed())
                .unwrap_or(0.0);
            let frame_ms = diags
                .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
                .and_then(|d| d.smoothed())
                .unwrap_or(0.0);

            // Colour the FPS by health (green ≥ 55, amber ≥ 30, red below).
            let fps_col = if fps >= 55.0 {
                egui::Color32::from_rgb(120, 220, 120)
            } else if fps >= 30.0 {
                egui::Color32::from_rgb(230, 200, 100)
            } else {
                egui::Color32::from_rgb(230, 110, 110)
            };
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{fps:.0}"))
                        .size(28.0)
                        .strong()
                        .color(fps_col),
                );
                ui.label(format!("FPS   ({frame_ms:.2} ms/frame)"));
            });

            // Frame-time sparkline straight off the diagnostic's rolling history.
            if let Some(d) = diags.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME) {
                let history: Vec<f32> = d.values().map(|v| *v as f32).collect();
                frame_graph(ui, &history);
            }

            ui.separator();

            // ── Counts ───────────────────────────────────────────────────────────────
            let entities = diags
                .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
                .and_then(|d| d.value())
                .unwrap_or(0.0);
            egui::Grid::new("counts").num_columns(2).striped(true).show(ui, |ui| {
                ui.label("entities");
                ui.label(format!("{entities:.0}"));
                ui.end_row();
                ui.label("camp orks");
                ui.label(format!("{}", orks_q.iter().count()));
                ui.end_row();
                ui.label("invaders");
                ui.label(format!("{}", invaders_q.iter().count()));
                ui.end_row();
                ui.label("wildlife");
                ui.label(format!("{}", animals_q.iter().count()));
                ui.end_row();
            });

            ui.separator();

            // ── State machine ────────────────────────────────────────────────────────
            egui::Grid::new("state").num_columns(2).striped(true).show(ui, |ui| {
                ui.label("app");
                ui.label(format!("{:?}", app_state.get()));
                ui.end_row();
                ui.label("modal");
                ui.label(modal.map_or("—".to_string(), |m| format!("{:?}", m.get())));
                ui.end_row();
                if let Some(s) = siege.as_ref() {
                    ui.label("phase");
                    let phase_txt = match s.phase {
                        GamePhase::Prep => format!("Prep ({:.0}s left)", s.prep_seconds_left),
                        GamePhase::Wave => format!("Wave {} ", s.wave_index + 1),
                        other => format!("{other:?}"),
                    };
                    ui.label(phase_txt);
                    ui.end_row();
                    ui.label("difficulty");
                    ui.label(format!("{:?}", s.difficulty));
                    ui.end_row();
                }
                if let Some(k) = keep.as_ref() {
                    ui.label("keep hp");
                    ui.label(format!("{:.0} / {:.0}", k.hp.max(0.0), k.max));
                    ui.end_row();
                }
            });

            ui.separator();

            // ── Hero ─────────────────────────────────────────────────────────────────
            egui::CollapsingHeader::new("Hero").default_open(true).show(ui, |ui| {
                egui::Grid::new("hero").num_columns(2).striped(true).show(ui, |ui| {
                    if let Some(p) = player.as_ref() {
                        ui.label("hp");
                        ui.label(format!("{:.0} / {:.0}", p.0.hp.max(0.0), p.0.max_hp));
                        ui.end_row();
                        ui.label("level");
                        ui.label(format!("{}  ({} / {} xp)", p.0.level, p.0.xp, p.0.xp_to_next));
                        ui.end_row();
                        ui.label("gold");
                        ui.label(format!("{}", p.0.gold));
                        ui.end_row();
                    }
                    if let Some(b) = bank.as_ref() {
                        ui.label("stone");
                        ui.label(format!("{:.0}", b.0.stone));
                        ui.end_row();
                    }
                    if let Ok(h) = hero_q.single() {
                        ui.label("pos");
                        ui.label(format!("({:.1}, {:.1})", h.pos.x, h.pos.y));
                        ui.end_row();
                    }
                });
            });
        });

    Ok(())
}

/// A compact frame-time sparkline. Draws the rolling history (ms per frame) as a polyline scaled
/// to its own peak, with 60 fps (16.7 ms) and 30 fps (33.3 ms) reference lines so a spike that
/// crosses into stutter territory is obvious at a glance.
fn frame_graph(ui: &mut egui::Ui, history: &[f32]) {
    let (rect, painter) =
        ui.allocate_painter(egui::vec2(ui.available_width(), 48.0), egui::Sense::hover());
    let rect = rect.rect;
    painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(24, 26, 30));
    if history.len() < 2 {
        return;
    }

    // Scale to the larger of the observed peak or 33.3 ms so the 30-fps line is always on-chart.
    let peak = history.iter().copied().fold(0.0_f32, f32::max).max(33.3);
    let y_at = |ms: f32| rect.bottom() - (ms / peak).clamp(0.0, 1.0) * rect.height();

    // Reference lines: green = 60 fps budget, red = 30 fps floor.
    for (ms, col) in [
        (1000.0 / 60.0, egui::Color32::from_rgb(70, 120, 70)),
        (1000.0 / 30.0, egui::Color32::from_rgb(130, 70, 70)),
    ] {
        let y = y_at(ms);
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0, col),
        );
    }

    let n = history.len();
    let pts: Vec<egui::Pos2> = history
        .iter()
        .enumerate()
        .map(|(i, &ms)| {
            let x = rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
            egui::pos2(x, y_at(ms))
        })
        .collect();
    painter.line(pts, egui::Stroke::new(1.5, egui::Color32::from_rgb(120, 200, 240)));
}
