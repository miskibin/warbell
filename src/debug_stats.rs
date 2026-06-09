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
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
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
            // Per-render-pass GPU timings (shadow / prepass / main / bloom / SSAO / SMAA / …).
            // Needs the GPU's TIMESTAMP_QUERY feature; falls back to CPU span times without it.
            RenderDiagnosticsPlugin,
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

            // ── GPU passes ───────────────────────────────────────────────────────────
            // The bottleneck-finder: which render pass actually eats the frame. Reads the
            // `render/<pass>/elapsed_gpu` diagnostics (ms). On a GPU without TIMESTAMP_QUERY
            // these are empty and we fall back to the CPU span times.
            egui::CollapsingHeader::new("GPU passes (ms)").default_open(true).show(ui, |ui| {
                gpu_passes(ui, &diags);
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

/// The per-render-pass timing table — the heart of GPU debugging. Pulls every
/// `render/<pass>/elapsed_gpu` diagnostic (falling back to `/elapsed_cpu` if the GPU can't do
/// timestamp queries), sorts worst-first, and draws each as a labelled bar relative to the
/// heaviest pass so the bottleneck pass is obvious at a glance. Total of the listed passes is
/// shown at the bottom.
fn gpu_passes(ui: &mut egui::Ui, diags: &DiagnosticsStore) {
    let collect = |field: &str| -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = diags
            .iter()
            .filter_map(|d| {
                let p = d.path().as_str();
                let name = p.strip_prefix("render/")?.strip_suffix(field)?;
                let ms = d.smoothed().filter(|m| *m > 0.0)?;
                Some((name.trim_end_matches('/').replace('/', " › "), ms))
            })
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v
    };

    // Prefer GPU timings; fall back to CPU spans on backends without TIMESTAMP_QUERY.
    let (mut rows, cpu_only) = match collect("/elapsed_gpu") {
        v if !v.is_empty() => (v, false),
        _ => (collect("/elapsed_cpu"), true),
    };
    if rows.is_empty() {
        ui.weak("no render-pass timings yet…");
        return;
    }
    if cpu_only {
        ui.weak("GPU timestamps unavailable — showing CPU span times");
    }

    let max = rows.iter().map(|r| r.1).fold(0.0_f64, f64::max).max(0.01);
    let total: f64 = rows.iter().map(|r| r.1).sum();
    rows.truncate(14); // keep the panel compact — the long tail is sub-0.1 ms anyway
    for (name, ms) in &rows {
        let frac = (ms / max) as f32;
        let bar = egui::ProgressBar::new(frac)
            .desired_width(ui.available_width())
            .text(format!("{name}  {ms:.2}"));
        ui.add(bar);
    }
    ui.weak(format!("Σ listed passes: {total:.2} ms"));
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
