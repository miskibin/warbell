//! Free-grid building placement: ghost preview + R rotation + footprint validation, pay on
//! place, Stronghold-style unmanned timed construction (scaffold grows), blocker
//! registration on completion.

use bevy::prelude::*;

pub struct RtsBuildPlugin;

impl Plugin for RtsBuildPlugin {
    fn build(&self, _app: &mut App) {}
}
