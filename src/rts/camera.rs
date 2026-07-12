//! RTS isometric camera: reposes the ONE existing `Camera3d` (never a second camera —
//! CLAUDE.md hard rule) into an orthographic iso view with WASD/edge-pan + wheel zoom.

use bevy::prelude::*;

pub struct RtsCameraPlugin;

impl Plugin for RtsCameraPlugin {
    fn build(&self, _app: &mut App) {}
}
