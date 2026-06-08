//! Screenshot harness: set `FOREST_SHOT=<path.png>` and the app renders a few
//! frames (so lighting/IBL/prepasses settle), grabs the window, saves, and exits.
//! The Bevy window can't be captured by external tools, so this is how we verify.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

pub struct CapturePlugin;

#[derive(Resource)]
struct ShotPath(String);

#[derive(Resource, Default)]
struct ShotClock(u32);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("FOREST_SHOT") {
            app.insert_resource(ShotPath(path))
                .init_resource::<ShotClock>()
                .add_systems(Update, drive_shot);
        }
    }
}

fn drive_shot(
    mut clock: ResMut<ShotClock>,
    path: Res<ShotPath>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    clock.0 += 1;
    if clock.0 == 90 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.0.clone()));
    }
    if clock.0 > 120 {
        exit.write(AppExit::Success);
    }
}
