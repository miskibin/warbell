//! Worker economy: auto-claim by producer buildings, walk→gather→haul-to-own-town-hall
//! loops (generic fork of the lumberjack/miner shape), farm food cycle, flee from enemies,
//! population growth from food surplus.

use bevy::prelude::*;

pub struct RtsWorkersPlugin;

impl Plugin for RtsWorkersPlugin {
    fn build(&self, _app: &mut App) {}
}
