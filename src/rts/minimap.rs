//! RTS minimap — a north-up top-down panel (bottom-left) with live blips for every building, unit
//! and deposit, plus a box marking the camera's current view. Display-only for now (no click-to-move
//! yet — that needs a selection-input keep-out for the panel rect).
//!
//! Rendering is a **pooled set of blip `Node`s** (same reposition-a-pool trick as the selection
//! rings / rubber-band): each frame we map every entity's world XZ into panel pixels and park one
//! pooled blip there, tinted + sized by kind; extras hide. No per-frame spawn churn beyond growth,
//! and blips carry no text (so no font-atlas thrash — see the siege-perf note).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::dying::Dying;
use crate::game_state::AppState;
use crate::rts::camera::RtsCamFocus;
use crate::rts::{in_skirmish, Deposit, DepositKind, RtsBuilding, RtsUnit, Side, UnitKind};

/// Panel size (px) and the world half-extent it shows (a touch past the land radius so the whole
/// island frames). World XZ in `[-HALF, HALF]` maps to panel `[0, SIZE]`.
const SIZE: f32 = 172.0;
const HALF: f32 = 52.0;
/// Inset of the blip field from the panel border.
const PAD: f32 = 4.0;
/// Panel anchor (px from the window's left / bottom) — must match the spawned node.
const PANEL_LEFT: f32 = 12.0;
const PANEL_BOTTOM: f32 = 12.0;

/// True if `cursor` (screen px) is over the minimap panel — so `select`/`command` ignore the click
/// and let the minimap own it (click-to-move the camera).
pub fn over_minimap(cursor: Vec2, window_h: f32) -> bool {
    let top = window_h - PANEL_BOTTOM - SIZE;
    cursor.x >= PANEL_LEFT && cursor.x <= PANEL_LEFT + SIZE && cursor.y >= top && cursor.y <= top + SIZE
}

/// Marker on the minimap panel node.
#[derive(Component)]
struct MinimapPanel;
/// A pooled blip (building / unit / deposit dot).
#[derive(Component)]
struct Blip;
/// The camera-view outline box.
#[derive(Component)]
struct ViewBox;

pub struct RtsMinimapPlugin;

impl Plugin for RtsMinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn_panel, sync_minimap, minimap_click)
                .chain()
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

/// World XZ (castle-at-origin) → panel-local pixel (north-up: world −Z at the top). Clamped inside
/// the padded field so an off-map blip still shows at the edge.
fn to_px(world: Vec2) -> Vec2 {
    let f = (SIZE - 2.0 * PAD) / (2.0 * HALF);
    Vec2::new(
        (PAD + (world.x + HALF) * f).clamp(PAD, SIZE - PAD),
        (PAD + (world.y + HALF) * f).clamp(PAD, SIZE - PAD),
    )
}

/// Lazily spawn the panel + the view-box once (mirrors `draw_band`'s lazy-spawn).
fn spawn_panel(mut commands: Commands, panel: Query<Entity, With<MinimapPanel>>) {
    if !panel.is_empty() {
        return;
    }
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(12.0),
                width: Val::Px(SIZE),
                height: Val::Px(SIZE),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.09, 0.06, 0.72)),
            BorderColor::all(Color::srgba(0.55, 0.45, 0.25, 0.9)),
            GlobalZIndex(40),
            MinimapPanel,
        ))
        .with_children(|p| {
            // The camera-view outline (spawned first so blips draw over it).
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(20.0),
                    height: Val::Px(20.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                ViewBox,
            ));
        });
}

#[allow(clippy::type_complexity)]
fn sync_minimap(
    mut commands: Commands,
    panel: Query<Entity, With<MinimapPanel>>,
    focus: Res<RtsCamFocus>,
    buildings: Query<(&GlobalTransform, &Side, &RtsBuilding), Without<Dying>>,
    units: Query<(&GlobalTransform, &Side, &RtsUnit), Without<Dying>>,
    deposits: Query<(&GlobalTransform, &Deposit)>,
    mut blips: Query<(&mut Node, &mut BackgroundColor, &mut Visibility), (With<Blip>, Without<ViewBox>)>,
    mut view: Query<&mut Node, (With<ViewBox>, Without<Blip>)>,
) {
    let Ok(panel) = panel.single() else { return };

    // Build the blip list: (panel px, colour, size). Deposits first (drawn under), then buildings,
    // then units on top.
    let mut want: Vec<(Vec2, Color, f32)> = Vec::new();
    for (gt, d) in &deposits {
        let c = match d.kind {
            DepositKind::Wood => Color::srgb(0.35, 0.55, 0.25),
            DepositKind::Stone => Color::srgb(0.6, 0.6, 0.62),
            DepositKind::Gold => Color::srgb(0.85, 0.72, 0.28),
        };
        want.push((to_px(Vec2::new(gt.translation().x, gt.translation().z)), c, 3.0));
    }
    for (gt, side, _b) in &buildings {
        let c = side_color(*side, true);
        want.push((to_px(Vec2::new(gt.translation().x, gt.translation().z)), c, 6.0));
    }
    for (gt, side, u) in &units {
        let c = side_color(*side, false);
        let sz = if u.kind == UnitKind::Worker { 2.5 } else { 3.5 };
        want.push((to_px(Vec2::new(gt.translation().x, gt.translation().z)), c, sz));
    }

    // Grow the pool if needed (new blips show next frame).
    let have = blips.iter().count();
    for _ in have..want.len() {
        commands.entity(panel).with_children(|p| {
            p.spawn((
                Node { position_type: PositionType::Absolute, ..default() },
                BackgroundColor(Color::NONE),
                Visibility::Hidden,
                Blip,
            ));
        });
    }

    // Park one blip per wanted dot; hide the rest.
    let mut it = want.into_iter();
    for (mut node, mut bg, mut vis) in &mut blips {
        match it.next() {
            Some((px, col, sz)) => {
                node.left = Val::Px(px.x - sz * 0.5);
                node.top = Val::Px(px.y - sz * 0.5);
                node.width = Val::Px(sz);
                node.height = Val::Px(sz);
                bg.0 = col;
                *vis = Visibility::Visible;
            }
            None => *vis = Visibility::Hidden,
        }
    }

    // Camera-view box: centred on the focus, sized by the zoom (approximate — the iso view is a
    // rotated diamond, but an axis-aligned box reads clearly enough as "you are looking here").
    if let Ok(mut vn) = view.single_mut() {
        let c = to_px(focus.pos);
        let half_px = (focus.zoom * 0.5) * (SIZE - 2.0 * PAD) / (2.0 * HALF);
        let w = (half_px * 1.6).clamp(8.0, SIZE);
        let h = (half_px * 1.2).clamp(8.0, SIZE);
        vn.left = Val::Px((c.x - w * 0.5).clamp(0.0, SIZE - w));
        vn.top = Val::Px((c.y - h * 0.5).clamp(0.0, SIZE - h));
        vn.width = Val::Px(w);
        vn.height = Val::Px(h);
    }
}

/// Click (or drag) on the minimap → jump the camera focus to that world point. `pressed` (not just-
/// pressed) so holding + dragging scrubs the view around. `select`/`command` skip clicks over the
/// panel (via [`over_minimap`]) so this owns them.
fn minimap_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut focus: ResMut<RtsCamFocus>,
) {
    if !mouse.pressed(MouseButton::Left) {
        return;
    }
    let Ok(win) = windows.single() else { return };
    let Some(c) = win.cursor_position() else { return };
    if !over_minimap(c, win.height()) {
        return;
    }
    let top = win.height() - PANEL_BOTTOM - SIZE;
    let local = Vec2::new(c.x - PANEL_LEFT, c.y - top);
    let f = (SIZE - 2.0 * PAD) / (2.0 * HALF);
    focus.pos = Vec2::new((local.x - PAD) / f - HALF, (local.y - PAD) / f - HALF);
}

fn side_color(side: Side, building: bool) -> Color {
    match (side, building) {
        (Side::Player, true) => Color::srgb(0.35, 0.85, 0.45),
        (Side::Player, false) => Color::srgb(0.45, 0.95, 0.6),
        (Side::Rival, true) => Color::srgb(0.9, 0.32, 0.28),
        (Side::Rival, false) => Color::srgb(1.0, 0.45, 0.4),
    }
}
