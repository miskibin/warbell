//! RTS soldiers: barracks training pipeline (idle worker + cost → swordsman/archer), Side-
//! based combat brains (melee chase/strike, archer volleys) reusing the shared Health/
//! Dying/damage channels and `blockers::wall_between` LOS.

use bevy::prelude::*;

pub struct RtsUnitsPlugin;

impl Plugin for RtsUnitsPlugin {
    fn build(&self, _app: &mut App) {}
}
