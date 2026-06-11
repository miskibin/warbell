//! Shield block — hold right-mouse to raise the shield: drains stamina, sets the `blocking`
//! flag that [`super::health`] reads to negate hits outright, and locks out when stamina is
//! spent until it recovers. Ported from `blockStore.ts`.

use bevy::prelude::*;

use super::{HeroHealth, PlayMode, PlayerRes};

const BLOCK_DRAIN: f32 = 38.0; // stamina/s while held (~2.6s of guard)
const BLOCK_REGEN: f32 = 26.0; // stamina/s recovered once regen starts
const BLOCK_REGEN_DELAY: f32 = 0.6; // s after a guard before stamina regenerates
const BLOCK_RECOVER: f32 = 30.0; // stamina needed to clear a lockout

pub fn player_block(
    time: Res<Time>,
    mode: Res<PlayMode>,
    player: Res<PlayerRes>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut hero_q: Query<&mut HeroHealth>,
) {
    let Ok(mut hh) = hero_q.single_mut() else { return };
    let dt = time.delta_secs();

    if *mode != PlayMode::Play || !player.0.is_alive() {
        hh.blocking = false;
        return;
    }

    // The block SFX is NOT fired here — raising the shield is silent. The knock plays only when
    // a hit is actually absorbed (see `health::apply_hero_damage`), matching `playerStore.ts`.
    let want = buttons.pressed(MouseButton::Right) && !hh.block_locked && hh.stamina > 0.0;
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
}
