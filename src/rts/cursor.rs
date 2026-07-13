//! Custom skirmish cursor — a gold targeting reticle drawn as a top-most UI overlay that follows
//! the mouse, with the OS cursor hidden. RTS uses a free (non-pointer-locked) cursor, so this is a
//! plain follow-the-mouse node; it's `FocusPolicy::Pass` so it never intercepts clicks, and it hides
//! itself when the pointer leaves the window (so a capture doesn't show a stray reticle in a corner).

use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::game_state::AppState;
use crate::rts::in_skirmish;

/// Reticle diameter (px).
const RING: f32 = 20.0;
const GOLD: Color = Color::srgb(0.95, 0.82, 0.35);

#[derive(Component)]
struct CursorRing;

pub struct RtsCursorPlugin;

impl Plugin for RtsCursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn_cursor, follow_cursor)
                .chain()
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

/// Lazily spawn the reticle + hide the OS cursor (once).
fn spawn_cursor(
    mut commands: Commands,
    existing: Query<Entity, With<CursorRing>>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if !existing.is_empty() {
        return;
    }
    if let Ok(mut cur) = cursor_q.single_mut() {
        cur.visible = false; // hide the OS pointer; the reticle replaces it
    }
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
            GlobalZIndex(1000),
            Visibility::Hidden,
            CursorRing,
        ))
        .with_children(|p| {
            // Centre dot.
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

/// Park the reticle on the cursor each frame; hide it when the pointer leaves the window.
fn follow_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut ring: Query<(&mut Node, &mut Visibility), With<CursorRing>>,
) {
    let Ok((mut node, mut vis)) = ring.single_mut() else { return };
    let Ok(win) = windows.single() else { return };
    match win.cursor_position() {
        Some(c) => {
            node.left = Val::Px(c.x - RING * 0.5);
            node.top = Val::Px(c.y - RING * 0.5);
            *vis = Visibility::Visible;
        }
        None => *vis = Visibility::Hidden,
    }
}
