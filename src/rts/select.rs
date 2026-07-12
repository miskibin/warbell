//! Unit/building selection: LMB click, drag box, Shift add/toggle; a flat ground ring under each
//! selected unit and a rubber-band overlay.
//!
//! Click vs. drag is decided on release by how far the cursor moved from the press point. A click
//! selects the nearest own unit (else own building, else clears); a drag box-selects own units
//! inside the screen rect. Shift adds/toggles. Buildings are single-select only and never box-
//! selected. Enemy entities are never `Selected` (info-only, out of POC scope). Input is suspended
//! while build placement is active (`Placing`) or attack-move is armed (`command::AttackMove`) so
//! those layers own the click, and clicks over the HUD bands are ignored.
//!
//! Selection highlight is a pooled flat ground ring per selected unit (the same cheap
//! unlit-emissive annulus recipe as `town::sync_build_rings`). NB the toon `outline.rs` post pass is
//! a **per-camera fullscreen effect**, not a per-entity opt-in, so it can't ring individual units —
//! the ground ring is the highlight.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::dying::Dying;
use crate::game_state::{AppState, Modal};
use crate::rts::command::AttackMove;
use crate::rts::pick;
use crate::rts::{Placing, RtsBuilding, RtsUnit, Selected, Side};
use crate::ui::theme;

/// Cursor travel (px) past which a press-drag becomes a box-select instead of a click.
const DRAG_THRESHOLD_PX: f32 = 6.0;

/// In-flight LMB drag: `start` = press point (None when not dragging), `cur` = latest cursor,
/// `box_active` = moved past the threshold (rubber-band mode). Read by `draw_band` too.
#[derive(Resource, Default)]
struct RtsDrag {
    start: Option<Vec2>,
    cur: Vec2,
    box_active: bool,
}

/// Marker on the pooled ground-ring meshes (placed under selected units by `sync_selection_rings`).
#[derive(Component)]
struct SelectionRing;

/// Marker on the single rubber-band UI node.
#[derive(Component)]
struct SelectionBand;

pub struct RtsSelectPlugin;

impl Plugin for RtsSelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RtsDrag>()
            .add_systems(
                Update,
                selection_input
                    .run_if(super::in_skirmish)
                    .run_if(in_state(AppState::Playing))
                    .run_if(in_state(Modal::None)),
            )
            // Cosmetics: keep drawing even if a panel is up (the frozen world still shows selection).
            .add_systems(
                Update,
                (draw_band, sync_selection_rings)
                    .run_if(super::in_skirmish)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn selection_input(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    placing: Res<Placing>,
    attack: Res<AttackMove>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut drag: ResMut<RtsDrag>,
    mut commands: Commands,
    units: Query<(Entity, &GlobalTransform, &Side), (With<RtsUnit>, Without<Dying>)>,
    buildings: Query<(Entity, &GlobalTransform, &Side), (With<RtsBuilding>, Without<Dying>)>,
    sel_all: Query<Entity, With<Selected>>,
    sel_buildings: Query<Entity, (With<Selected>, With<RtsBuilding>)>,
) {
    // Build placement / attack-move own the pointer — no selection while either is active.
    if placing.0.is_some() || attack.0 {
        drag.start = None;
        drag.box_active = false;
        return;
    }
    let Ok(win) = windows.single() else { return };
    let Ok((camera, cam_tf)) = camera.single() else { return };
    let cursor = win.cursor_position();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    // Press: begin a potential drag (unless over the HUD bands).
    if mouse.just_pressed(MouseButton::Left) {
        if let Some(c) = cursor {
            if !pick::over_hud(c, win.height()) {
                drag.start = Some(c);
                drag.cur = c;
                drag.box_active = false;
            }
        }
    }
    // Held: track cursor; promote to a box once past the threshold.
    if mouse.pressed(MouseButton::Left) {
        if let (Some(c), Some(start)) = (cursor, drag.start) {
            drag.cur = c;
            if start.distance(c) > DRAG_THRESHOLD_PX {
                drag.box_active = true;
            }
        }
    }
    // Release: resolve as click-select or box-select.
    if mouse.just_released(MouseButton::Left) {
        let Some(start) = drag.start.take() else { return };
        let end = cursor.unwrap_or(drag.cur);
        let box_mode = drag.box_active;
        drag.box_active = false;

        if box_mode {
            // Box select: own units inside the rect. Shift adds (keep units) but always drops any
            // selected building; a plain box clears everything first. Buildings are never box-picked.
            let (min, max) = (start.min(end), start.max(end));
            if shift {
                for b in &sel_buildings {
                    commands.entity(b).try_remove::<Selected>();
                }
            } else {
                for e in &sel_all {
                    commands.entity(e).try_remove::<Selected>();
                }
            }
            for (e, gt, side) in &units {
                if *side == Side::Player && pick::in_screen_rect(camera, cam_tf, gt.translation(), min, max) {
                    commands.entity(e).try_insert(Selected);
                }
            }
            return;
        }

        // Click select: nearest own unit, else own building, else clear.
        let unit_hit = pick::nearest_within(
            camera,
            cam_tf,
            end,
            pick::UNIT_PICK_PX,
            units
                .iter()
                .filter(|(_, _, s)| **s == Side::Player)
                // Pick against mid-chest, not the feet (see UNIT_PICK_Y) — a click on the body hits.
                .map(|(e, gt, _)| (e, gt.translation() + Vec3::Y * pick::UNIT_PICK_Y)),
        );
        if let Some(e) = unit_hit {
            if shift {
                // Toggle membership; drop any selected building (no mixed unit+building selection).
                for b in &sel_buildings {
                    commands.entity(b).try_remove::<Selected>();
                }
                if sel_all.get(e).is_ok() {
                    commands.entity(e).try_remove::<Selected>();
                } else {
                    commands.entity(e).try_insert(Selected);
                }
            } else {
                for s in &sel_all {
                    commands.entity(s).try_remove::<Selected>();
                }
                commands.entity(e).try_insert(Selected);
            }
            return;
        }

        // Own building? Single-select (Shift behaves the same — buildings never multi-select).
        let bld_hit = pick::nearest_within(
            camera,
            cam_tf,
            end,
            pick::BUILDING_PICK_PX,
            buildings.iter().filter(|(_, _, s)| **s == Side::Player).map(|(e, gt, _)| (e, gt.translation())),
        );
        if let Some(e) = bld_hit {
            for s in &sel_all {
                commands.entity(s).try_remove::<Selected>();
            }
            commands.entity(e).try_insert(Selected);
            return;
        }

        // Empty ground: clear unless Shift.
        if !shift {
            for s in &sel_all {
                commands.entity(s).try_remove::<Selected>();
            }
        }
    }
}

/// Draw/update the rubber-band overlay from `RtsDrag` (lazy-spawns the node once).
fn draw_band(
    drag: Res<RtsDrag>,
    placing: Res<Placing>,
    mut commands: Commands,
    mut band: Query<(&mut Node, &mut Visibility), With<SelectionBand>>,
    mut spawned: Local<bool>,
) {
    if !*spawned {
        *spawned = true;
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::GREEN.with_alpha(0.12)),
            BorderColor::all(theme::GREEN.with_alpha(0.85)),
            Visibility::Hidden,
            GlobalZIndex(50),
            SelectionBand,
        ));
        return; // node shows from next frame
    }
    let Ok((mut node, mut vis)) = band.single_mut() else { return };
    if placing.0.is_none() && drag.box_active {
        let a = drag.start.unwrap_or(drag.cur);
        let (min, max) = (a.min(drag.cur), a.max(drag.cur));
        node.left = Val::Px(min.x);
        node.top = Val::Px(min.y);
        node.width = Val::Px((max.x - min.x).max(0.0));
        node.height = Val::Px((max.y - min.y).max(0.0));
        *vis = Visibility::Visible;
    } else {
        *vis = Visibility::Hidden;
    }
}

/// Pool of flat ground rings placed under each selected unit (one ring per unit; extras hidden).
/// Same reposition-a-pool approach as `town::sync_build_rings` — no per-entity child management, so
/// it can't orphan a ring when a unit dies mid-frame.
fn sync_selection_rings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selected: Query<&GlobalTransform, (With<Selected>, With<RtsUnit>)>,
    mut rings: Query<(&mut Transform, &mut Visibility), With<SelectionRing>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    let (mesh, mat) = assets
        .get_or_insert_with(|| {
            let mesh = meshes.add(Annulus::new(0.5, 0.66).mesh().resolution(40).build());
            let mat = materials.add(StandardMaterial {
                base_color: Color::srgba(0.5, 1.0, 0.55, 0.6),
                emissive: LinearRgba::rgb(0.3, 1.1, 0.4),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            });
            (mesh, mat)
        })
        .clone();

    let positions: Vec<Vec3> = selected.iter().map(|gt| gt.translation()).collect();
    // Grow the pool if more units are selected than we have rings (new rings show next frame).
    let have = rings.iter().count();
    for _ in have..positions.len() {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(0.0, -100.0, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Visibility::Hidden,
            SelectionRing,
        ));
    }
    // Place one ring per selected unit; hide the rest. Only translation is touched, so the flat
    // rotation set at spawn is preserved.
    let mut it = positions.into_iter();
    for (mut tf, mut vis) in &mut rings {
        match it.next() {
            Some(p) => {
                tf.translation = Vec3::new(p.x, p.y + 0.08, p.z);
                *vis = Visibility::Visible;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}
