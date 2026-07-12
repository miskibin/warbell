//! RTS HUD: top resource/population bar, bottom-right build strip, bottom-left selection
//! panel (train buttons + progress on a selected barracks). Reuses the ui/ kit.

use bevy::prelude::*;

pub struct RtsHudPlugin;

impl Plugin for RtsHudPlugin {
    fn build(&self, _app: &mut App) {}
}
