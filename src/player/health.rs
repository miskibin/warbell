//! Hero vitals — drains the orks' [`PendingHeroDamage`] into HP (mitigated while blocking),
//! and on death respawns the hero at a castle gate after a short beat. Ported from the
//! damage + "respawn" path of `playerStore.ts` (the TS succession system is out of scope).

use bevy::prelude::*;

use crate::audio::AudioCue;

use super::{Hero, HeroHealth, PendingHeroDamage, PlayerRes, HERO_SCALE};

const BLOCK_MITIGATION: f32 = 0.2; // a blocked hit deals 20% (and costs stamina)
const BLOCK_HIT_STAMINA: f32 = 18.0; // stamina spent absorbing one blocked hit
const RESPAWN_DELAY: f64 = 1.6; // s down before the hero rises again (succession lands in P5)

pub fn apply_hero_damage(
    time: Res<Time>,
    mut pending: ResMut<PendingHeroDamage>,
    mut player: ResMut<PlayerRes>,
    buffs: Res<crate::inventory::Buffs>,
    inv: Res<crate::inventory::Inventory>,
    mut lives: ResMut<crate::succession::Lives>,
    mut hero_q: Query<(&mut Hero, &mut Transform, &mut HeroHealth)>,
    villagers: Query<(Entity, &Transform), (With<crate::villagers::Villager>, Without<Hero>)>,
    mut commands: Commands,
    mut cues: MessageWriter<AudioCue>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
) {
    let Ok((mut hero, mut tf, mut hh)) = hero_q.single_mut() else {
        pending.0 = 0.0;
        return;
    };
    let p = &mut player.0;
    let now = time.elapsed_secs() as f64;

    // ── Take queued ork damage (unless already down) ──
    if pending.0 > 0.0 && p.dead_since.is_none() {
        let mut dmg = pending.0;
        let blocking = hh.blocking;
        if blocking {
            dmg *= BLOCK_MITIGATION;
            cues.write(AudioCue::Block); // shield knock — only when a hit is actually absorbed
            hh.stamina = (hh.stamina - BLOCK_HIT_STAMINA).max(0.0);
            if hh.stamina <= 0.0 {
                hh.block_locked = true;
                hh.blocking = false;
            }
        }
        // Layer the resist-buff (taken) + worn-armor (armor) mults onto the (already
        // block-reduced) blow — matches the TS `damage(amount, takenMult, armorMult)`.
        p.damage(dmg as f64, now, buffs.0.damage_taken_mult(now), inv.0.armor_damage_mult());
        let dead = p.hp <= 0.0;

        // Combat juice: a floating number ("BLOCK" / "-N") + red flash + screen shake.
        let head = Vec3::new(hero.pos.x, hero.y + 2.2, hero.pos.y);
        let (text, color) = if blocking {
            ("BLOCK".to_string(), crate::combat_fx::col_block())
        } else {
            (format!("-{}", dmg.round() as i32), crate::combat_fx::col_hero_hit())
        };
        floats.0.push(crate::combat_fx::FloatReq { world: head, text, color, scale: 1.0 });
        feedback.flash = 0.35;
        feedback.trauma = (feedback.trauma + if dead { 0.5 } else { 0.22 }).min(1.0);

        cues.write(if dead { AudioCue::HeroDeath } else { AudioCue::HeroHurt });
    }
    pending.0 = 0.0;

    // ── Death → the blade passes to an heir, who rises at the north gate after a beat ──
    if let Some(t0) = p.dead_since {
        if now - t0 >= RESPAWN_DELAY {
            // No heir left → the bloodline ends; leave the hero down and let the run lose.
            if lives.heirs == 0 {
                lives.defeat = true;
                return;
            }
            lives.heirs -= 1; // the next heir takes up the blade
            // The blade passes: a townsfolk standing nearest the fallen hero is consumed (the
            // heir who takes up the line). Best-effort — fine when the town has run dry.
            let mut nearest: Option<(Entity, f32)> = None;
            for (e, vtf) in &villagers {
                let d = Vec2::new(vtf.translation.x, vtf.translation.z).distance(hero.pos);
                if nearest.is_none_or(|(_, bd)| d < bd) {
                    nearest = Some((e, d));
                }
            }
            if let Some((e, _)) = nearest {
                commands.entity(e).try_despawn();
            }
            let gate = crate::castle::gate_centers()[0];
            let pos = Vec2::new(gate.x, gate.y - 3.0);
            let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
            hero.pos = pos;
            hero.y = y;
            hero.facing = 0.0;
            hero.vel_y = 0.0;
            hero.on_ground = true;
            hero.attacking = false;
            tf.translation = Vec3::new(pos.x, y, pos.y);
            tf.rotation = Quat::from_rotation_y(0.0);
            tf.scale = Vec3::splat(HERO_SCALE);
            p.respawn_at(pos.x as f64, y as f64, pos.y as f64); // full HP, clears dead_since
            hh.stamina = hh.stamina_max;
            hh.block_locked = false;
            hh.blocking = false;
        }
    }
}
