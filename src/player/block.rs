//! Shield block — hold right-mouse to raise the shield: drains stamina, sets the `blocking`
//! flag that [`super::health`] reads to negate hits outright, and locks out when stamina is
//! spent until it recovers. Ported from `blockStore.ts`.

use bevy::prelude::*;

use super::{HeroHealth, PlayMode, PlayerRes};

const BLOCK_DRAIN: f32 = 38.0; // stamina/s while held (~2.6s of guard)
const BLOCK_REGEN: f32 = 26.0; // stamina/s recovered once regen starts
const BLOCK_REGEN_DELAY: f32 = 0.6; // s after a guard before stamina regenerates
const BLOCK_RECOVER: f32 = 30.0; // stamina needed to clear a lockout
const BASE_STAMINA_MAX: f32 = 150.0; // level-1 pool (matches HeroHealth::default)
const STAMINA_PER_LEVEL: f32 = 15.0; // extra max stamina granted per hero level → longer guards

pub fn player_block(
    time: Res<Time>,
    mode: Res<PlayMode>,
    player: Res<PlayerRes>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut hero_q: Query<&mut HeroHealth>,
    mut hero_state: ResMut<super::HeroState>,
) {
    let Ok(mut hh) = hero_q.single_mut() else { return };
    let dt = time.delta_secs();

    // Stamina pool grows with hero level — a veteran guards longer. Level is persisted on
    // PlayerRes (and rides the save), so deriving the cap each frame needs no extra save state.
    let want_max = BASE_STAMINA_MAX + (player.0.level.max(1) - 1) as f32 * STAMINA_PER_LEVEL;
    if want_max > hh.stamina_max {
        hh.stamina += want_max - hh.stamina_max; // a level-up tops up into the new headroom
    }
    hh.stamina_max = want_max;
    hh.stamina = hh.stamina.min(hh.stamina_max);

    if *mode != PlayMode::Play || !player.0.is_alive() {
        hh.blocking = false;
        hero_state.blocking = false;
        return;
    }

    // Capture hook: `FOREST_ANIMTEST=block` stages the guard from `player::animtest`; yield here
    // so this system (whose scheduling order vs. animtest is arbitrary) can't clear the staged
    // flag on the frames where it runs second — the race made block captures a coin-flip.
    if std::env::var("FOREST_ANIMTEST").is_ok_and(|v| v == "block" || v == "defend") {
        return;
    }

    // The block SFX is NOT fired here — raising the shield is silent. The knock plays only when
    // a hit is actually absorbed (see `health::apply_hero_damage`), matching `playerStore.ts`.
    let want = buttons.pressed(MouseButton::Right) && !hh.block_locked && hh.stamina > 0.0;
    if want && !hh.blocking {
        // Rising edge — stamp the raise so a blow landing within the parry window of it PARRIES
        // (see `health::apply_hero_damage`): the timed guard, not the held one, earns the riposte.
        hh.guard_raised_at = time.elapsed_secs();
    }
    if want {
        hh.blocking = true;
        hh.stamina = (hh.stamina - BLOCK_DRAIN * dt).max(0.0);
        hh.regen_pause = BLOCK_REGEN_DELAY;
        if hh.stamina <= 0.0 {
            hh.block_locked = true;
            hh.blocking = false;
        }
    } else {
        hh.blocking = false;
        if hh.regen_pause > 0.0 {
            hh.regen_pause = (hh.regen_pause - dt).max(0.0);
        } else if hh.stamina < hh.stamina_max {
            hh.stamina = (hh.stamina + BLOCK_REGEN * dt).min(hh.stamina_max);
            if hh.block_locked && hh.stamina >= BLOCK_RECOVER {
                hh.block_locked = false;
            }
        }
    }

    hero_state.blocking = hh.blocking;
}
