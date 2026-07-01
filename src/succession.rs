//! **Succession — the bloodline.** The hero is one of a line of heirs: when he falls, the blade
//! passes to the next and the town's headcount drops by one — **an heir IS a townsperson**. There
//! is exactly one number: `town.population` is the source of truth, and [`Lives::heirs`] is a
//! read-only mirror of it ([`mirror_heirs`]) so the HUD/save never drift from the town. The hero
//! falling with the town empty ends the line — Defeat by *bloodline*, the second way to lose
//! besides the keep falling.
//!
//! **The succession beat** ([`Succession`] + [`drive_succession`]) makes that legible: instead of
//! the hero silently blinking to the gate, his fall plays a short directed beat — the world drops
//! into slow-motion, the camera swings to the nearest living townsperson, the soul wisp flies from
//! the corpse into them, and they **transform into the hero where they stand** (you take control
//! there). The player can finally *see* that the bloodline takes over a peasant's body. When the
//! town is empty the same beat ends in Defeat instead of a rise.
//!
//! The pool grows however the town grows: food surplus (`town::population_system`), camp rescues,
//! mercenary recruits, the housing-gated dawn coming-of-age (`siege::run_director`), and the
//! castle larder (core `town_store`) that organically regrows an emptied town to its bedrock
//! pair — so the bloodline always has somewhere to come back from.

use bevy::prelude::*;
use bevy::time::{Real, Virtual};

use crate::game_state::AppState;

/// The bloodline pool. `heirs` is a **read-only mirror** of `town.population` — never write it;
/// write the town headcount instead ([`mirror_heirs`] stomps any drift every frame).
#[derive(Resource)]
pub struct Lives {
    pub heirs: u32,
    /// Set when the hero falls with no townsperson left → the run is lost.
    pub defeat: bool,
}

impl Default for Lives {
    fn default() -> Self {
        Self { heirs: 0, defeat: false } // heirs filled from town.population by `mirror_heirs`
    }
}

// ── Succession-beat timing (REAL seconds — the phase clock is wall-clock so the beat is a fixed
//    duration regardless of the slow-mo we apply to `Time<Virtual>`). The camera swing rides this
//    REAL clock, NOT the slowed world, so its durations must be long enough to read as a deliberate
//    dolly *against* the slow-mo (a short swing zips while the world crawls — reads as "too fast"). ──
/// Ease the camera in to frame the peasant over this long — a slow, deliberate dolly.
const CAM_IN: f32 = 0.70;
/// The transform instant: the peasant becomes the hero (body steal / Defeat). Holds on the framed
/// peasant for `TRANSFORM_T − CAM_IN` before the swap.
const TRANSFORM_T: f32 = 1.45;
/// Ease the world back to full speed (and the camera back to follow) by here; the beat ends.
const RESUME_END: f32 = 2.25;
/// Ramp the world down to slow-mo over this long at the start.
const SLOW_IN: f32 = 0.22;
/// `Time<Virtual>` relative speed at the depth of the beat.
const SLOW_SPEED: f32 = 0.28;
/// Spawn-protection granted to the risen hero (VIRTUAL secs — matches `HeroHealth::iframe_until`),
/// so a body stolen in the thick of the enemy line isn't instantly re-killed.
const RISE_IFRAMES: f32 = 1.0;

/// The directed succession beat. Transient — not saved; cleared on a fresh run / leaving Playing.
#[derive(Resource, Default)]
pub struct Succession {
    /// A beat is in progress.
    pub active: bool,
    /// The hero fell with no heir left — this beat ends in Defeat, not a rise.
    pub final_death: bool,
    /// The transform/Defeat instant has already fired (fire-once guard).
    transformed: bool,
    /// `Time<Real>` elapsed when the beat began (the phase clock origin).
    real_t0: f32,
    /// Where the hero fell (the wisp launches here).
    corpse_pos: Vec3,
    /// The townsperson whose body we take (`None` → none found, rise at the gate).
    steal_entity: Option<Entity>,
    /// Where the new hero rises — the stolen peasant's spot (or the gate fallback).
    pub steal_pos: Vec3,
    /// 0→1 how far the camera is pulled to the cinematic framing (read by `player_camera`).
    pub cam_blend: f32,
}

pub struct SuccessionPlugin;

impl Plugin for SuccessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Lives>()
            .init_resource::<Succession>()
            .add_systems(OnExit(AppState::StartScreen), reset_lives)
            .add_systems(OnExit(AppState::GameOver), reset_lives)
            // `OnExit(GameOver)` fires on an in-process Continue (fresh runs relaunch instead);
            // `reset_lives` clears the defeat flag before `apply_pending_load` restores the save.
            // Ungated: the heir count shown in the HUD tracks the town even while frozen.
            .add_systems(Update, mirror_heirs)
            // The beat is world-sim (slows time, despawns the stolen townsperson, moves the hero),
            // so it carries the `Modal::None` freeze-gate too — a panel opened mid-beat must freeze
            // it like the rest of the sim, not keep mutating the world behind the panel.
            .add_systems(
                Update,
                (drive_succession, watch_bloodline)
                    .run_if(in_state(AppState::Playing))
                    .run_if(in_state(crate::game_state::Modal::None)),
            )
            // Safety: a pause / GameOver mid-beat must never leave the world stuck in slow-mo.
            .add_systems(OnExit(AppState::Playing), end_beat_safely);

        // `FOREST_SUCCESSION=1`: stage a hero death shortly after boot so the beat can be filmed /
        // screenshotted via the capture harness (same `FOREST_*` staging-hook style as the rest).
        if std::env::var("FOREST_SUCCESSION").is_ok() {
            app.add_systems(Update, force_succession.run_if(in_state(AppState::Playing)));
        }
    }
}

/// New run: clear the defeat flag + any in-flight beat. The heir count needs no seeding — it
/// mirrors the town, and `town::reset_town` owns the starting headcount.
fn reset_lives(mut lives: ResMut<Lives>, mut succ: ResMut<Succession>, mut vtime: ResMut<Time<Virtual>>) {
    *lives = Lives::default();
    *succ = Succession::default();
    vtime.set_relative_speed(1.0);
}

/// Enforce heirs ≡ town.population every frame. Heirs and townsfolk are the SAME people; any
/// system that used to bump `heirs` separately now writes the town headcount and this sync
/// carries it into the HUD/save view.
fn mirror_heirs(town: Res<crate::town::TownRes>, mut lives: ResMut<Lives>) {
    if lives.heirs != town.0.population {
        lives.heirs = town.0.population;
    }
}

/// End the run once the bloodline is spent (hand off to the GameOver screen).
fn watch_bloodline(lives: Res<Lives>, mut next: ResMut<NextState<AppState>>) {
    if lives.defeat {
        next.set(AppState::GameOver);
    }
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Drive the succession beat: detect the hero's fall, slow the world, swing the camera to the
/// nearest townsperson, and at the transform instant either possess them (full-HP rise on their
/// spot) or — with the town empty — declare Defeat. See the module doc.
#[allow(clippy::too_many_arguments)]
fn drive_succession(
    rtime: Res<Time<Real>>,
    mut vtime: ResMut<Time<Virtual>>,
    mut succ: ResMut<Succession>,
    mut player: ResMut<crate::player::PlayerRes>,
    mut town: ResMut<crate::town::TownRes>,
    mut lives: ResMut<Lives>,
    mut hero_q: Query<(&mut crate::player::Hero, &mut Transform, &mut crate::player::HeroHealth)>,
    villagers: Query<
        (Entity, &Transform),
        (
            With<crate::villagers::Townsfolk>,
            Without<crate::dying::Dying>,
            Without<crate::player::Hero>,
        ),
    >,
    mut commands: Commands,
    mut fell: MessageWriter<crate::succession_fx::HeirFell>,
    mut rose: MessageWriter<crate::succession_fx::HeirRose>,
) {
    // ── Start a beat the frame the hero falls (skip if we're already defeated → GameOver pending) ──
    if !succ.active {
        if player.0.dead_since.is_none() || lives.defeat {
            return;
        }
        let Ok((hero, _, _)) = hero_q.single() else { return };
        let corpse = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
        let corpse_xz = hero.pos;

        *succ = Succession { active: true, real_t0: rtime.elapsed_secs(), corpse_pos: corpse, ..default() };

        if town.0.population == 0 {
            // No heir left — the beat still plays (a slow, weighty fall) but ends in Defeat.
            succ.final_death = true;
            succ.steal_pos = corpse;
        } else {
            // Possess the nearest living townsperson (militia keep the `Townsfolk` tag at night, so
            // there's always a body during a siege). No body found → rise at the north gate.
            let mut nearest: Option<(Entity, Vec3, f32)> = None;
            for (e, vtf) in &villagers {
                let d = Vec2::new(vtf.translation.x, vtf.translation.z).distance(corpse_xz);
                if nearest.is_none_or(|(_, _, bd)| d < bd) {
                    nearest = Some((e, vtf.translation, d));
                }
            }
            match nearest {
                Some((e, pos, _)) => {
                    succ.steal_entity = Some(e);
                    succ.steal_pos = pos;
                }
                None => {
                    let gate = crate::castle::gate_centers()[0];
                    let gx = gate.x;
                    let gz = gate.y - 3.0;
                    let gy = crate::worldmap::ground_at_world(gx, gz).unwrap_or(0.0);
                    succ.steal_pos = Vec3::new(gx, gy, gz);
                }
            }
            // Launch the soul wisp now, corpse → the body we're about to take.
            fell.write(crate::succession_fx::HeirFell { grave_at: corpse, rise_at: succ.steal_pos });
        }
        return;
    }

    let t = rtime.elapsed_secs() - succ.real_t0;

    // Slow-mo curve: ramp down, hold, ramp back up over the resume tail.
    let speed = if t < SLOW_IN {
        1.0 + (SLOW_SPEED - 1.0) * smoothstep(t / SLOW_IN)
    } else if t < TRANSFORM_T {
        SLOW_SPEED
    } else if t < RESUME_END {
        SLOW_SPEED + (1.0 - SLOW_SPEED) * smoothstep((t - TRANSFORM_T) / (RESUME_END - TRANSFORM_T))
    } else {
        1.0
    };
    vtime.set_relative_speed(speed.max(0.05));

    // Camera pull: ease in to frame the peasant, hold, ease back out as the (now-risen) hero is
    // followed normally again.
    succ.cam_blend = if t < CAM_IN {
        smoothstep(t / CAM_IN)
    } else if t < TRANSFORM_T {
        1.0
    } else if t < RESUME_END {
        1.0 - smoothstep((t - TRANSFORM_T) / (RESUME_END - TRANSFORM_T))
    } else {
        0.0
    };

    // ── The transform instant ──
    if t >= TRANSFORM_T && !succ.transformed {
        succ.transformed = true;
        if succ.final_death {
            // The line ends here — declare Defeat and snap the world back to speed for GameOver.
            lives.defeat = true;
            succ.active = false;
            succ.cam_blend = 0.0;
            vtime.set_relative_speed(1.0);
            return;
        }
        // Possess the body: drop one townsperson from the pool, despawn it, and stand the hero up
        // on its spot at full strength with a beat of spawn-protection.
        // Guard the double-decrement: combat (`npc_damage_apply`) can kill the stolen body during the
        // slow-mo beat, and that path already dropped population by one. Only decrement here if the
        // body is still a live townsperson (or we're rising at the gate with no body picked) — a body
        // that died mid-beat is `Dying`/gone, so `villagers.get` fails and we skip the second drop.
        let already_counted = succ.steal_entity.is_some_and(|e| villagers.get(e).is_err());
        if !already_counted {
            town.0.population = town.0.population.saturating_sub(1);
        }
        if let Some(e) = succ.steal_entity.take() {
            commands.entity(e).try_despawn();
        }
        let pos = Vec2::new(succ.steal_pos.x, succ.steal_pos.z);
        let y = succ.steal_pos.y;
        if let Ok((mut hero, mut tf, mut hh)) = hero_q.single_mut() {
            hero.pos = pos;
            hero.y = y;
            hero.facing = 0.0;
            hero.vel_y = 0.0;
            hero.on_ground = true;
            hero.attacking = false;
            tf.translation = succ.steal_pos;
            tf.rotation = Quat::from_rotation_y(0.0);
            tf.scale = Vec3::splat(crate::player::HERO_SCALE);
            player.0.respawn_at(pos.x as f64, y as f64, pos.y as f64); // full HP, clears dead_since
            hh.stamina = hh.stamina_max;
            hh.block_locked = false;
            hh.blocking = false;
            hh.iframe_until = vtime.elapsed_secs() + RISE_IFRAMES;
        }
        // Flash of light at the rise: the peasant becomes the knight.
        rose.write(crate::succession_fx::HeirRose { at: succ.steal_pos });
    }

    if t >= RESUME_END {
        succ.active = false;
        succ.cam_blend = 0.0;
        vtime.set_relative_speed(1.0);
    }
}

/// Belt-and-suspenders: leaving Playing (pause / GameOver / menu) mid-beat restores full speed and
/// abandons the beat, so the world is never frozen in slow-mo on the other side.
fn end_beat_safely(mut succ: ResMut<Succession>, mut vtime: ResMut<Time<Virtual>>) {
    if succ.active {
        *succ = Succession::default();
    }
    vtime.set_relative_speed(1.0);
}

/// `FOREST_SUCCESSION=1` staging hook: once, after a delay, fell the hero so the beat plays for a
/// capture. `FOREST_SUCCESSION=<secs>` sets the delay (default 1.2 s) so a clip's warmup can be
/// cleared before the death lands in-frame. Direct death-poke; `drive_succession` does the rest.
fn force_succession(
    time: Res<Time>,
    mut player: ResMut<crate::player::PlayerRes>,
    mut done: Local<bool>,
    mut elapsed: Local<f32>,
) {
    if *done {
        return;
    }
    let delay = std::env::var("FOREST_SUCCESSION").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.2);
    *elapsed += time.delta_secs();
    if *elapsed < delay {
        return;
    }
    *done = true;
    player.0.hp = 0.0;
    player.0.dead_since = Some(time.elapsed_secs_f64());
}
