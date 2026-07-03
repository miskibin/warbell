//! **The melee ring** — group-combat choreography around the hero (the "kung-fu circle" /
//! attack-token pattern every brawler uses: Arkham, Assassin's Creed, DOOM's token pools, and the
//! *Game AI Pro* "Beyond the Kung-Fu Circle" stage-manager this is a minimal cut of).
//!
//! Only [`MELEE_CAP`] melee attackers may PRESS the hero at once; everyone else who wants him
//! holds a loose ring at [`RING_R`] and slowly prowls around it (their steering target orbits, so
//! they circle the fight instead of piling into one scrum and swinging as a blob). A held token
//! expires after [`TOKEN_TIME`] and a striker releases on landing a blow, then rests
//! [`REST_TIME`] before re-claiming — so attackers visibly ROTATE through the fight.
//!
//! Deliberate exemptions, per the research on how these systems are tuned:
//! - **Shamans** (ranged) never take melee tokens — they already hold their own cast ring, and a
//!   bolt in flight while a club swings is the point of mixed groups.
//! - **Frenzied berserkers** bypass the ring (the Souls-style relentless elite — trash waits its
//!   turn, the elite does not).
//! - Bosses/wardens/the warlord aren't orks and are untouched.
//!
//! Both ork brains consult this (the camp brain in `orks.rs`, the night-wave invader brain in
//! `siege.rs`); guards/keep/building targets are NOT token-gated — the circle exists to make the
//! *hero's* melee readable, not to slow the siege down.

use bevy::prelude::*;
use std::collections::HashMap;

/// Radius of the waiting ring around the hero (world units) — outside the ~1.5 club range, inside
/// sight, close enough to read as "surrounding you".
pub const RING_R: f32 = 2.9;
/// An ork closer to the hero than this without a token peels back out to the ring.
pub const HOLD_ENGAGE: f32 = RING_R + 1.6;
/// Simultaneous melee attackers allowed on the hero. (Difficulty could scale this later —
/// research: Arkham/AC run 1, DOOM runs more per-type; 2 reads busy without mobbing.)
pub const MELEE_CAP: usize = 2;
/// Max seconds a token is held before it's forcibly rotated to the next waiter.
pub const TOKEN_TIME: f32 = 2.6;
/// Per-ork rest after releasing a token, so the same two don't monopolise the fight.
pub const REST_TIME: f32 = 1.4;
/// Waiting orks prowl the ring at this fraction of their charge speed.
pub const HOLD_SPEED: f32 = 0.55;

/// The stage manager: who currently holds a melee token on the hero, and who is resting.
/// Maps stay ork-count-bounded (expired entries are dropped on the claims that touch them).
#[derive(Resource, Default)]
pub struct MeleeRing {
    /// token holder → `elapsed_secs` the hold expires.
    holders: HashMap<Entity, f32>,
    /// recently-released attacker → `elapsed_secs` it may claim again.
    rested: HashMap<Entity, f32>,
}

impl MeleeRing {
    /// Claim (or keep) an attack token. Call every frame while wanting the hero; returns whether
    /// this ork may press the attack this frame.
    pub fn try_claim(&mut self, e: Entity, now: f32) -> bool {
        if let Some(&exp) = self.holders.get(&e) {
            if now < exp {
                return true;
            }
            // Held too long (e.g. never reached the hero) — rotate it out.
            self.holders.remove(&e);
            self.rested.insert(e, now + REST_TIME);
            return false;
        }
        self.holders.retain(|_, &mut exp| now < exp); // drop stale holders (incl. the slain)
        if self.holders.len() >= MELEE_CAP {
            return false;
        }
        match self.rested.get(&e) {
            Some(&t) if now < t => false,
            _ => {
                self.rested.remove(&e);
                self.holders.insert(e, now + TOKEN_TIME);
                true
            }
        }
    }

    /// A blow landed (or the attacker broke off) — hand the token to the next waiter and rest.
    pub fn release(&mut self, e: Entity, now: f32) {
        if self.holders.remove(&e).is_some() {
            self.rested.insert(e, now + REST_TIME);
        }
    }
}

/// Steering target for a waiting ork: a point ON the ring, led ~20° along its orbit (sign fixed
/// per-entity), so following it prowls the ork slowly AROUND the hero instead of parking it.
pub fn hold_point(e: Entity, hero: Vec2, pos: Vec2) -> Vec2 {
    let to = pos - hero;
    let base = if to.length_squared() > 1e-4 {
        to.y.atan2(to.x)
    } else {
        (e.to_bits() % 8) as f32 * 0.785 // stacked on the hero somehow — fan out by id
    };
    let sign = if e.to_bits() % 2 == 0 { 1.0 } else { -1.0 };
    let ang = base + sign * 0.35;
    hero + Vec2::new(ang.cos(), ang.sin()) * RING_R
}
