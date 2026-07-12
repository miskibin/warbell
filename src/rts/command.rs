//! Order routing: consumes `RtsOrder` messages (RMB move/attack, attack-move, harvest) and
//! writes goals into the NavPath machinery with group fan-out offsets.

use bevy::prelude::*;

pub struct RtsCommandPlugin;

impl Plugin for RtsCommandPlugin {
    fn build(&self, _app: &mut App) {}
}
