//! Game-wide custom cursor — a gold reticle that replaces the OS pointer in EVERY state (menu,
//! campaign panels, skirmish). Drawn as a top-most, click-through UI overlay that follows the mouse.
//!
//! It runs in `PostUpdate` so its `visible = false` write lands AFTER the campaign's own cursor
//! systems (`controls.rs`, `player/camera.rs`) set visibility in `Update` — the reticle always wins
//! over the OS pointer when the cursor is FREE. When the pointer is LOCKED (first-person camera
//! grab), there's no free cursor, so we hide the reticle and leave the lock code's `visible=false`
//! alone. Hidden too when the mouse leaves the window (so a capture never shows a stray reticle).

use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

/// Reticle diameter (px).
const RING: f32 = 20.0;
const GOLD: Color = Color::srgb(0.95, 0.82, 0.35);

#[derive(Component)]
struct CursorRing;

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_cursor)
            // PostUpdate so we override any Update-frame cursor-visibility writes.
            .add_systems(PostUpdate, drive_cursor);
    }
}

fn spawn_cursor(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(RING),
                height: Val::Px(RING),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(GOLD),
            FocusPolicy::Pass,
            GlobalZIndex(10_000),
            Visibility::Hidden,
            CursorRing,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(RING * 0.5 - 3.0),
                    top: Val::Px(RING * 0.5 - 3.0),
                    width: Val::Px(4.0),
                    height: Val::Px(4.0),
                    border_radius: BorderRadius::all(Val::Percent(50.0)),
                    ..default()
                },
                BackgroundColor(GOLD),
                FocusPolicy::Pass,
            ));
        });
}

fn drive_cursor(
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut ring: Query<(&mut Node, &mut Visibility), With<CursorRing>>,
) {
    let Ok((mut node, mut vis)) = ring.single_mut() else { return };
    let Ok(mut cur) = cursor_q.single_mut() else { return };
    let Ok(win) = windows.single() else { return };

    // Pointer locked (FP camera grab) → no free cursor: hide the reticle, leave `visible` to the
    // lock code.
    if cur.grab_mode != CursorGrabMode::None {
        *vis = Visibility::Hidden;
        return;
    }
    // Free cursor everywhere else (menu / panels / skirmish): the reticle replaces the OS pointer.
    cur.visible = false;
    match win.cursor_position() {
        Some(c) => {
            node.left = Val::Px(c.x - RING * 0.5);
            node.top = Val::Px(c.y - RING * 0.5);
            *vis = Visibility::Visible;
        }
        None => *vis = Visibility::Hidden, // mouse left the window
    }
}
