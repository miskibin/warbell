//! Game-wide custom cursor — a gold reticle set as the **hardware** cursor via [`CursorIcon`], so
//! the OS/compositor draws it at zero latency. (The earlier version was a UI-node software cursor
//! that lagged the real mouse by a frame or two — "zamula".) The image is generated procedurally
//! (no asset file) and set once on the primary window; the OS shows it whenever the cursor is
//! visible and hides it under a first-person pointer-lock, so it works in every state (menu,
//! campaign panels, skirmish) with no per-frame follow system.

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
    let handle = images.add(reticle());
    commands.entity(win).insert(CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
        handle,
        texture_atlas: None,
        flip_x: false,
        flip_y: false,
        rect: None,
        hotspot: ((SIZE / 2) as u16, (SIZE / 2) as u16),
    })));
    *done = true;
}

/// A gold ring + centre dot with a dark contrast halo, on transparent — the reticle bitmap.
fn reticle() -> Image {
    let mut data = vec![0u8; SIZE * SIZE * 4];
    let c = (SIZE as f32 - 1.0) * 0.5;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let d = (dx * dx + dy * dy).sqrt();
            let i = (y * SIZE + x) * 4;
            let gold = (11.0..=13.5).contains(&d) || d < 2.3;
            let halo = !gold && (9.6..=15.0).contains(&d);
            if gold {
                data[i] = 242;
                data[i + 1] = 209;
                data[i + 2] = 89;
                data[i + 3] = 255;
            } else if halo {
                data[i] = 18;
                data[i + 1] = 14;
                data[i + 2] = 6;
                data[i + 3] = 150;
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
