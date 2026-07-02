//! Hero vitals — drains the orks' [`PendingHeroDamage`] into HP (negated entirely while
//! blocking, at a stamina cost). Death (HP → 0) arms `core::Player::dead_since`; the *succession
//! beat* that follows — slow-mo, camera swing, and the heir possessing the nearest townsperson —
//! lives in [`crate::succession::drive_succession`]. This module just takes the hit + crumples.

use bevy::prelude::*;

use crate::audio::AudioCue;

use super::{Hero, HeroHealth, PendingHeroDamage, PlayerRes};

const BLOCK_HIT_STAMINA: f32 = 18.0; // stamina spent absorbing one blocked hit
const CRIT_BLOCK_STAMINA: f32 = 55.0; // a parried warden CRITICAL nearly drains the guard bar
/// Seconds the hero takes to keel over once slain (the death "crumple", like the orks').
const DEATH_FALL_SECS: f32 = 0.55;

pub fn apply_hero_damage(
    time: Res<Time>,
    mut pending: ResMut<PendingHeroDamage>,
    mut crit: ResMut<crate::player::PendingCrit>,
    mut player: ResMut<PlayerRes>,
    buffs: Res<crate::inventory::Buffs>,
    inv: Res<crate::inventory::Inventory>,
    mut hero_q: Query<(&Hero, &mut HeroHealth)>,
    mut cues: MessageWriter<AudioCue>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
) {
    // A warden critical landed this frame (lethal unless blocked/dodged). Read + clear it here so
    // it's consumed exactly once, alongside the same pending-damage drain it rode in on.
    let is_crit = crit.0;
    crit.0 = false;

    let Ok((hero, mut hh)) = hero_q.single_mut() else {
        *pending = Default::default();
        return;
    };
    let p = &mut player.0;
    let now = time.elapsed_secs() as f64;

    // ── Sand-Dash i-frames: negate any blow that lands mid-blink (a true dodge). ──
    let invuln = time.elapsed_secs() < hh.iframe_until;
    if invuln && pending.0 > 0.0 && p.dead_since.is_none() {
        let head = Vec3::new(hero.pos.x, hero.y + 2.2, hero.pos.y);
        floats.0.push(crate::combat_fx::FloatReq {
            world: head,
            text: if is_crit { "DODGE!".into() } else { "dodge".into() },
            color: crate::combat_fx::col_block(),
            scale: if is_crit { 1.2 } else { 0.9 },
        });
        *pending = Default::default();
    }

    // ── Take queued ork damage (unless already down / dodging) ──
    if pending.0 > 0.0 && p.dead_since.is_none() {
        let mut dmg = pending.0;
        let blocking = hh.blocking;
        if blocking {
            // A raised shield absorbs the hit COMPLETELY — the cost is stamina, not HP
            // (`p.damage` no-ops on 0, so no hurt flash / death path fires). A parried CRITICAL
            // costs far more stamina (it nearly drains the guard), but it's what saves your life.
            dmg = 0.0;
            cues.write(AudioCue::Block); // shield knock — only when a hit is actually absorbed
            let cost = if is_crit { CRIT_BLOCK_STAMINA } else { BLOCK_HIT_STAMINA };
            hh.stamina = (hh.stamina - cost).max(0.0);
            if hh.stamina <= 0.0 {
                hh.block_locked = true;
                hh.blocking = false;
            }
        }
        // Layer the resist-buff (taken) + worn-armor (armor) mults onto the unblocked blow
        // — matches the TS `damage(amount, takenMult, armorMult)`.
        p.damage(dmg as f64, now, buffs.0.damage_taken_mult(now), inv.0.armor_damage_mult());
        let dead = p.hp <= 0.0;

        // Combat juice: a floating number ("BLOCK" / "-N") + red flash + screen shake.
        let head = Vec3::new(hero.pos.x, hero.y + 2.2, hero.pos.y);
        let (text, color) = if blocking {
            (if is_crit { "PARRY!".to_string() } else { "BLOCK".to_string() }, crate::combat_fx::col_block())
        } else if is_crit {
            // An unblocked critical one-shots — name it rather than dumping the overkill number.
            ("EXECUTED!".to_string(), crate::combat_fx::col_hero_hit())
        } else {
            (format!("-{}", dmg.round() as i32), crate::combat_fx::col_hero_hit())
        };
        floats.0.push(crate::combat_fx::FloatReq { world: head, text, color, scale: 1.0 });
        // A fully-blocked hit keeps the impact shake (shield knock) but skips the red
        // damage flash and the hurt grunt — no damage was taken.
        if !blocking {
            feedback.flash = 0.35;
        }
        feedback.trauma = (feedback.trauma + if dead { 0.5 } else { 0.28 }).min(1.0);
        // Steer the shake ALONG the blow (attacker → hero) so a hit visibly knocks the camera
        // away from its source; an unattributed hazard (`.1 == ZERO`) keeps the chaos shake.
        if pending.1.length_squared() > 1e-6 {
            feedback.shake_dir = pending.1.normalize();
        }
        crate::combat_fx::add_fov_kick(&mut feedback, if dead { 2.0 } else { 0.8 });

        if !blocking {
            cues.write(if dead { AudioCue::HeroDeath } else { AudioCue::HeroHurt });
        }
    }
    *pending = Default::default();

    // Death itself (HP → 0, `dead_since` armed by `core::Player::damage`) is now handed off to the
    // succession beat (`succession::drive_succession`): it slows the world, swings the camera to the
    // nearest townsperson, and either possesses them (full-HP rise on their spot) or — town empty —
    // declares Defeat. This system's sole remaining job is taking the hit.
}

/// Hero death "crumple": once slain (`dead_since` set), keel over backward and sink — the same
/// read as the orks' `Dying` fade, but on the persistent hero entity (respawn rights the pose).
/// Runs last in the gated chain so it overrides `player_move`'s idle-corpse transform; on the
/// respawn frame `apply_hero_damage` has already cleared `dead_since` + reset the pose, so this
/// no-ops and the heir stands upright.
pub fn hero_death_anim(
    time: Res<Time>,
    player: Res<PlayerRes>,
    mut hero_q: Query<(&Hero, &mut Transform)>,
) {
    let Some(t0) = player.0.dead_since else { return };
    let Ok((hero, mut tf)) = hero_q.single_mut() else { return };
    let t = (time.elapsed_secs() as f64 - t0) as f32;
    // Ease-out the fall, then hold flat for the rest of the down-time.
    let k = (t / DEATH_FALL_SECS).clamp(0.0, 1.0);
    let eased = 1.0 - (1.0 - k) * (1.0 - k);
    let lie = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2 * eased); // tip onto the back
    tf.rotation = Quat::from_rotation_y(hero.facing) * lie;
    // Settle the root a touch into the ground so the laid-out body rests on the turf.
    tf.translation = Vec3::new(hero.pos.x, hero.y - 0.15 * eased, hero.pos.y);
}
