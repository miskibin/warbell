//! Hero footsteps — phase-locked to the gait (a step each half walk-cycle, so the cadence
//! tracks sprint automatically), plus a louder touchdown step on landing. The clip is chosen
//! by the surface under the hero's feet (snow / stone / dirt). Emits [`AudioCue::Footstep`];
//! the [`super::sfx`] consumer plays it. Ported from `Character.tsx`'s step logic
//! (`half = floor(walkPhase / PI)`, `STEP_VOL` / `LAND_STEP_VOL`).

use std::f32::consts::PI;

use bevy::prelude::*;

use crate::biome::Biome;
use crate::player::Hero;
use crate::worldmap;

use super::{AudioCue, Surface};

fn surface_for(b: Biome) -> Surface {
    match b {
        Biome::Snow => Surface::Snow,
        Biome::Rocky => Surface::Stone,
        _ => Surface::Dirt,
    }
}

/// Surface under a world position: the tile's biome on the world map (grass / castle
/// ground → dirt).
fn surface_at(p: Vec2) -> Surface {
    worldmap::biome_at_world(p.x, p.y).map(surface_for).unwrap_or(Surface::Dirt)
}

pub(crate) fn hero_footsteps(
    mut last_half: Local<i64>,
    mut was_air: Local<bool>,
    mut cues: MessageWriter<AudioCue>,
    hero_q: Query<&Hero>,
) {
    let Ok(hero) = hero_q.single() else { return };
    let surface = surface_at(hero.pos);

    // `walk_phase` advances with accepted grounded travel, so derive the gait
    // half-cycle index from it; a change = one footfall.
    let half = (hero.walk_phase / PI).floor() as i64;
    let landed = hero.on_ground && *was_air;
    *was_air = !hero.on_ground;

    if landed {
        // Touchdown after a jump / fall — a single louder step, and swallow any walking step
        // that resuming the grounded gait would otherwise also fire this frame.
        cues.write(AudioCue::Footstep { surface, landing: true });
        *last_half = half;
        return;
    }
    if hero.moving && hero.on_ground {
        if half != *last_half {
            *last_half = half;
            cues.write(AudioCue::Footstep { surface, landing: false });
        }
    } else {
        // Idle or airborne: keep the counter current so resuming a walk doesn't fire a stale step.
        *last_half = half;
    }
}
