//! Game-wide custom cursor — a gold **arrow pointer** set as the **hardware** cursor via
//! [`CursorIcon`], so the OS/compositor draws it at zero latency. (The earlier version was a UI-node
//! software cursor that lagged the real mouse by a frame or two — "zamula"; before that a ring
//! reticle, swapped to a proper pointer arrow.) The image is generated procedurally (no asset file)
//! and set once on the primary window; the OS shows it whenever the cursor is visible and hides it
//! under a first-person pointer-lock, so it works in every state (menu, campaign panels, skirmish).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow};

/// Cursor image size (px). The hotspot (click point) is the centre.
const SIZE: usize = 32;

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, set_cursor);
    }
}

/// Install the custom hardware cursor once the primary window exists (retried in Update until then;
/// `done` latches it so we don't re-insert every frame).
fn set_cursor(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Ok(win) = windows.single() else { return };
    let handle = images.add(arrow());
    commands.entity(win).insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
        handle,
        texture_atlas: None,
        flip_x: false,
        flip_y: false,
        rect: None,
        // Hotspot = the arrow's tip (top-left), like a normal pointer.
        hotspot: (1, 1),
    })));
    *done = true;
}

/// The classic pointer polygon (tip at top-left), in image pixels. Point-in-polygon fills it gold;
/// a 1px dark dilation gives the contrast outline so it reads over any terrain.
const ARROW: [(f32, f32); 7] = [
    (1.5, 1.5),   // tip
    (1.5, 19.0),  // down the left edge
    (5.7, 15.2),  // inner notch (left of the tail)
    (8.7, 21.8),  // tail bottom-left
    (11.2, 20.6), // tail bottom-right
    (8.2, 13.8),  // inner notch (right of the tail)
    (13.4, 13.4), // right wing
];

fn in_arrow(px: f32, py: f32) -> bool {
    let mut inside = false;
    let n = ARROW.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = ARROW[i];
        let (xj, yj) = ARROW[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// A gold arrow pointer with a dark outline, on transparent — the cursor bitmap.
fn arrow() -> Image {
    let mut data = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let fill = in_arrow(px, py);
            // Outline: a pixel just outside the fill (sample the fill at a small ring around it).
            let outline = !fill
                && (in_arrow(px - 1.3, py)
                    || in_arrow(px + 1.3, py)
                    || in_arrow(px, py - 1.3)
                    || in_arrow(px, py + 1.3)
                    || in_arrow(px - 1.0, py - 1.0)
                    || in_arrow(px + 1.0, py - 1.0)
                    || in_arrow(px - 1.0, py + 1.0)
                    || in_arrow(px + 1.0, py + 1.0));
            let i = (y * SIZE + x) * 4;
            if fill {
                data[i] = 242;
                data[i + 1] = 209;
                data[i + 2] = 89;
                data[i + 3] = 255;
            } else if outline {
                data[i] = 18;
                data[i + 1] = 14;
                data[i + 2] = 6;
                data[i + 3] = 220;
            }
        }
    }
    Image::new(
        Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}
