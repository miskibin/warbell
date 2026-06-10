//! Hero locomotion — camera-relative WASD, axis-separated terrain + prop collision, jump +
//! gravity. Ported from `Character.tsx`'s movement block; constants are the TS values.
//!
//! Footing uses `steer::can_stand` (the same rule the orks/wildlife walk by: on land, within
//! one terrace class) + `blockers::is_blocked` for solid props — so the hero slides along
//! walls, can't walk into water, and can't climb 2-class cliff faces.

use bevy::prelude::*;

use crate::audio::AudioCue;
use crate::biome::Biome;
use crate::orks::Ork;
use crate::villagers::Villager;
use crate::wildlife::Animal;
use crate::{blockers, steer, worldmap};

use super::{Hero, HeroState, PendingHeroDamage, PlayMode, PlayerRes};

const SPEED: f32 = 3.5;
const SPRINT_MULT: f32 = 1.75;
const GRAVITY: f32 = 20.0;
const JUMP_SPEED: f32 = 6.5;
const TURN_RATE: f32 = 12.0;
const STEP_FREQ: f32 = 7.0;
const PLAYER_R: f32 = 0.22;

// ── Hazards (ported from Character.tsx) ──
/// A fall shorter than this lands free; beyond it hurts.
const FALL_SAFE: f32 = 1.1;
const FALL_DMG_PER_UNIT: f32 = 16.0;
const FALL_DMG_MAX: f32 = 45.0;
/// The swamp drags: move at 75% speed, and poison gnaws 2 HP every 2.5s while standing in it.
const SWAMP_SLOW: f32 = 0.75;
const SWAMP_POISON: f32 = 2.0;
const SWAMP_POISON_INTERVAL: f32 = 2.5;

fn key_axis(keys: &ButtonInput<KeyCode>, pos: KeyCode, neg: KeyCode) -> f32 {
    (keys.pressed(pos) as i32 - keys.pressed(neg) as i32) as f32
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    a + steer::wrap_pi(b - a) * t
}

fn write_state(state: &mut HeroState, hero: &Hero) {
    state.pos = hero.pos;
    state.y = hero.y;
    state.facing = hero.facing;
    state.alive = true;
}

/// Hero footing height at `(x, z)`: terrain, or the bridge deck where the river shows through.
/// `worldmap::ground_at_world` is terrain-only (reads `None` over water), so — unlike the orks,
/// who cross on nav-grid waypoints — the hero ORs the deck in here to walk the planks.
fn footing(x: f32, z: f32) -> Option<f32> {
    worldmap::ground_at_world(x, z).or_else(|| crate::bridges::deck_y_at(x, z))
}

/// Bridge-aware twin of `steer::can_stand`: the body centre + four footprint edges must all be
/// on footing (terrain or deck) within one terrace class of `cur_y`.
fn hero_can_stand(x: f32, z: f32, r: f32, cur_y: f32) -> bool {
    const OFF: [(f32, f32); 5] = [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];
    OFF.iter().all(|(dx, dz)| {
        matches!(footing(x + dx * r, z + dz * r), Some(y) if (y - cur_y).abs() <= steer::MAX_STEP)
    })
}

/// Asymmetric locomotion rule that lets the hero *drop* off cliffs (the orks/wildlife can't).
/// Every footprint point must still be on footing — no walking into water or off the map — and
/// none may rise more than one terrace class above `ref_y` (can't climb a cliff face). There's
/// no lower bound, so a downward ledge is walkable: the hero steps off, gravity takes over, and
/// the landing code bruises him if the drop cleared `FALL_SAFE`.
///
/// `ref_y` is the hero's *body* height (`hero.y`), not the ground under him: while falling past a
/// ledge that keeps the just-departed high tile in his radius, the high tile stays within
/// `MAX_STEP` of his airborne body, so it doesn't snag him at the lip.
fn hero_can_step(x: f32, z: f32, r: f32, ref_y: f32) -> bool {
    const OFF: [(f32, f32); 5] = [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];
    OFF.iter().all(|(dx, dz)| {
        matches!(footing(x + dx * r, z + dz * r), Some(y) if y <= ref_y + steer::MAX_STEP)
    })
}

/// Shove the hero out of a creature's body cylinder (centre `c`, radius `body_r`) so he can't
/// clip through it — a one-way push (the creature holds its ground), sliding along the same
/// standable/blocker rule as locomotion. Shared by the ork + animal collision passes.
fn shove_out_of(hero: &mut Hero, c: Vec2, body_r: f32, cur_y: f32) {
    let min_d = PLAYER_R + body_r;
    let to = hero.pos - c;
    let d = to.length();
    if d <= 1e-4 || d >= min_d {
        return;
    }
    let push = to / d * (min_d - d);
    let nx = hero.pos.x + push.x;
    let nz = hero.pos.y + push.y;
    if hero_can_stand(nx, hero.pos.y, PLAYER_R, cur_y) && !blockers::is_blocked(nx, hero.pos.y) {
        hero.pos.x = nx;
    }
    if hero_can_stand(hero.pos.x, nz, PLAYER_R, cur_y) && !blockers::is_blocked(hero.pos.x, nz) {
        hero.pos.y = nz;
    }
}

pub fn player_move(
    time: Res<Time>,
    mode: Res<PlayMode>,
    player: Res<PlayerRes>,
    buffs: Res<crate::inventory::Buffs>,
    keys: Res<ButtonInput<KeyCode>>,
    mut hero_q: Query<(&mut Hero, &mut Transform), Without<Camera3d>>,
    cam_q: Query<&Transform, (With<Camera3d>, Without<Hero>)>,
    orks: Query<&Ork>,
    animals: Query<&Animal>,
    villagers: Query<&Villager>,
    mut state: ResMut<HeroState>,
    mut pending: ResMut<PendingHeroDamage>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
    mut cues: MessageWriter<AudioCue>,
    mut poison_acc: Local<f32>,
    mut was_swamp: Local<bool>,
) {
    let Ok((mut hero, mut tf)) = hero_q.single_mut() else { return };
    let t = time.elapsed_secs();

    // Hold still in FreeRoam (fly-cam drives the view) or while down (awaiting respawn), but
    // keep the mirror current; `alive=false` when down so orks stop chasing the corpse.
    if *mode != PlayMode::Play || !player.0.is_alive() {
        hero.moving = false;
        let idle_bob = (t * 1.4).sin() * 0.025;
        tf.translation = Vec3::new(hero.pos.x, hero.y + idle_bob, hero.pos.y);
        write_state(&mut state, &hero);
        state.alive = player.0.is_alive();
        return;
    }

    let dt = time.delta_secs().min(0.05);

    // ── Swamp poison: gnaws 2 HP every 2.5s while standing in the marsh, even idle (the first
    // tick fires the instant you step in — `swampPoisonAt` starts at 0 in the TS). ──
    let in_swamp = worldmap::biome_at_world(hero.pos.x, hero.pos.y) == Some(Biome::Swamp);
    if in_swamp {
        if !*was_swamp {
            *poison_acc = SWAMP_POISON_INTERVAL; // arm an immediate first tick on entry
        }
        *poison_acc += dt;
        if *poison_acc >= SWAMP_POISON_INTERVAL {
            *poison_acc -= SWAMP_POISON_INTERVAL;
            pending.0 += SWAMP_POISON;
        }
    }
    *was_swamp = in_swamp;

    // ── Camera-relative move vector, flattened to the ground plane ──
    let mut fwd = cam_q.single().map(|c| *c.forward()).unwrap_or(Vec3::NEG_Z);
    fwd.y = 0.0;
    if fwd.length_squared() < 1e-6 {
        fwd = Vec3::NEG_Z;
    }
    fwd = fwd.normalize();
    let right = Vec3::new(-fwd.z, 0.0, fwd.x);

    let fwd_amt = (key_axis(&keys, KeyCode::KeyW, KeyCode::KeyS)
        + key_axis(&keys, KeyCode::ArrowUp, KeyCode::ArrowDown))
    .clamp(-1.0, 1.0);
    let rgt_amt = (key_axis(&keys, KeyCode::KeyD, KeyCode::KeyA)
        + key_axis(&keys, KeyCode::ArrowRight, KeyCode::ArrowLeft))
    .clamp(-1.0, 1.0);

    let mut move_dir = fwd * fwd_amt + right * rgt_amt;
    let moving = move_dir.length_squared() > 1e-6;
    if moving {
        move_dir = move_dir.normalize();
    }
    hero.moving = moving;

    let target = if moving { 1.0 } else { 0.0 };
    hero.moving_amt += (target - hero.moving_amt) * (dt * 10.0).min(1.0);

    // ── Horizontal motion with axis-separated terrain + prop collision ──
    let sprinting =
        moving && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    let cur_y = footing(hero.pos.x, hero.pos.y).unwrap_or(hero.y);
    if moving {
        let haste = buffs.0.speed_mult(t as f64) as f32; // active Haste buff (1.0 = none)
        let step = SPEED
            * if sprinting { SPRINT_MULT } else { 1.0 }
            * player.0.move_speed_mult as f32
            * haste
            * if in_swamp { SWAMP_SLOW } else { 1.0 }
            * dt;
        let nx = hero.pos.x + move_dir.x * step;
        let nz = hero.pos.y + move_dir.z * step;
        if hero_can_step(nx, hero.pos.y, PLAYER_R, hero.y) && !blockers::is_blocked(nx, hero.pos.y)
        {
            hero.pos.x = nx;
        }
        if hero_can_step(hero.pos.x, nz, PLAYER_R, hero.y) && !blockers::is_blocked(hero.pos.x, nz)
        {
            hero.pos.y = nz;
        }
        let want = move_dir.x.atan2(move_dir.z);
        hero.facing = lerp_angle(hero.facing, want, (dt * TURN_RATE).min(1.0));
    }

    // ── Body-collision vs creatures: shove the hero out of any overlap so he can't clip through
    // an ork or animal (one-way push — the creature holds its ground). ──
    for o in &orks {
        shove_out_of(&mut hero, o.pos, o.body_r, cur_y);
    }
    for a in &animals {
        shove_out_of(&mut hero, a.pos, a.body_r, cur_y);
    }
    // Townsfolk are solid too — you bump them, you don't walk through them.
    for v in &villagers {
        let (p, r) = v.body();
        shove_out_of(&mut hero, p, r, cur_y);
    }

    // ── Vertical: jump + gravity + ground snap ──
    let ground_y = footing(hero.pos.x, hero.pos.y).unwrap_or(0.0);
    let was_on_ground = hero.on_ground;
    if keys.just_pressed(KeyCode::Space) && hero.on_ground {
        hero.vel_y = JUMP_SPEED;
        hero.on_ground = false;
        cues.write(AudioCue::HeroJump); // voice decides (~40% + canGrunt); no other jump sfx
    }
    hero.vel_y -= GRAVITY * dt;
    hero.y += hero.vel_y * dt;
    if hero.y <= ground_y {
        // Just touched down: a long drop (cliff/jump) bruises on landing.
        if !was_on_ground {
            let fall = hero.air_takeoff_y - ground_y;
            if fall > FALL_SAFE {
                pending.0 += (((fall - FALL_SAFE) * FALL_DMG_PER_UNIT).round()).min(FALL_DMG_MAX);
                crate::combat_fx::add_fov_kick(&mut feedback, crate::combat_fx::FOV_KICK_LAND);
            }
        }
        hero.y = ground_y;
        hero.vel_y = 0.0;
        hero.on_ground = true;
    } else {
        if was_on_ground {
            hero.air_takeoff_y = hero.y;
        }
        hero.on_ground = false;
    }

    // ── Walk phase + body bob ──
    if moving {
        hero.walk_phase += dt * STEP_FREQ * if sprinting { SPRINT_MULT } else { 1.0 };
    }
    let m = hero.moving_amt;
    let idle_bob = (t * 1.4).sin() * 0.025;
    let walk_bob = hero.walk_phase.sin().abs() * 0.05;
    let bob = idle_bob * (1.0 - m) + walk_bob * m;

    tf.translation = Vec3::new(hero.pos.x, hero.y + bob, hero.pos.y);
    tf.rotation = Quat::from_rotation_y(hero.facing);

    write_state(&mut state, &hero);
}
