//! Ambient wildlife — the living layer over the world map. Seven species
//! (`critters::Species`) wander, graze and startle from the camera, with their limbs
//! swung procedurally (the `wind.rs` `Sway` trick, applied to legs/head/tail).
//!
//! Animals are full combat actors: per-species HP, a predator/prey food-chain, struck-enrage,
//! death-fade (`crate::dying`), loot drops, and respawn (35s herbivore / 50s predator, near the
//! death spot — except predator homes get pushed back out past [`TOWN_SANCTUARY_R`]).
//! They're placed biome-matched + walkable + deterministic inside `worldmap::build`,
//! tagged [`crate::biome::BiomeEntity`] so the biome-switch despawn/rebuild handles them.
//!
//! Two `Update` systems:
//!   * [`animal_brain`] — wander ↔ graze state machine + camera startle; integrates the
//!     XZ position (rejecting water / cliff steps via [`worldmap::ground_at_world`]),
//!     follows the ground, faces the heading, bobs while moving.
//!   * [`animal_limbs`] — overwrites each articulated child part's rotation from the
//!     parent animal's gait + a per-instance phase.

use bevy::prelude::*;

use crate::biome::{Biome, BiomeEntity};
use crate::critters::{self, PartKind, Species};
use crate::steer;
use crate::worldmap;

/// Max facing turn rate (rad/s). Caps how fast an animal can rotate so it never snaps
/// 180° between frames — the cure for the steering-oscillation flicker.
const MAX_TURN: f32 = 3.5;

pub struct WildlifePlugin;

impl Plugin for WildlifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RespawnQueue>();
        app.add_systems(Update, animal_limbs); // limb anim keeps running while frozen
        app.add_systems(
            Update,
            (animal_brain, enqueue_respawn, drain_respawns)
                .run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

// ── Components ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Wander,
    Graze,
    Flee,
    /// Predator charging the hero (wolf / bear / boar).
    Hunt,
}

#[derive(Component)]
pub struct Animal {
    /// Which species — `audio.rs` keys its per-species voice set off this.
    pub(crate) species: Species,
    /// Countdown to this animal's next ambient call (s). `audio::animal_voices` ticks it;
    /// a camera startle forces it to 0 so the creature vocalises on the spot.
    pub(crate) voice_timer: f32,
    /// Hard minimum-gap timer between any two of this animal's calls (s). Enforced in
    /// `audio::animal_voices` so a startle (which zeroes `voice_timer`) can never make a
    /// stuck, repeatedly-startling animal spawn a sound every frame.
    pub(crate) call_cd: f32,
    mode: Mode,
    home: Vec2,
    target: Vec2,
    pub(crate) pos: Vec2,
    facing: f32,
    speed: f32,        // flee / move-with-purpose speed
    wander_speed: f32, // relaxed roam speed
    flee_r: f32,       // camera startle radius (0 = apex predator, ignores the camera)
    wander_r: f32,     // how far from `home` it roams
    gait: f32,         // leg-swing frequency while moving
    swing: f32,        // leg-swing amplitude
    bob: f32,          // vertical body bob while moving
    pub(crate) body_r: f32, // footprint half-width (collision + cliff-edge footing)
    phase: f32,        // per-instance time offset (desyncs the animation)
    moving: bool,
    timer: f32,
    /// Predator bite cooldown (s); a bite lands at ≤ 0 and resets it.
    atk_cd: f32,
    /// Timestamp (`elapsed_secs`) of the last strike; `animal_limbs` plays a lunge-bite / tail-
    /// sting / arm-slam for the species' strike duration after it. `0` = none yet.
    atk_anim: f32,
    /// Timestamp of the last blow the animal TOOK; drives a springy recoil-wobble (reuses
    /// `orks::recoil_tilt`). Stamped by `player::combat`. `0` = none.
    pub(crate) hit_recoil: f32,
    /// When hunting, the prey entity being chased (caught on contact — eaten).
    hunt_prey: Option<Entity>,
    /// When hunting, the townsperson being charged (bitten on contact, like the hero).
    hunt_npc: Option<Entity>,
    /// Elapsed-seconds until which a struck predator stays latched onto its attacker (0 = calm).
    aggro_until: f32,
    /// Who the latch is aimed at: `None` = the hero, `Some` = the townsperson that struck it.
    aggro_target: Option<Entity>,
    /// Decaying knockback shove (world XZ, units/s) from a recent hero blow — applied + slid
    /// against terrain each frame, the same stagger orks get (`orks::apply_knockback`).
    pub(crate) kb: Vec2,
    pub(crate) rng: u32,
}

/// Inserted by combat on a surviving animal it struck — a predator latches onto its attacker
/// (struck-enrage). `animal_brain` reads it once, then removes it.
#[derive(Component)]
pub struct Struck {
    /// Who landed the blow: `None` = the hero; `Some` = a townsperson (guard or defending worker).
    pub by: Option<Entity>,
}

/// Seconds a struck predator stays enraged + locked onto the hero.
const STRUCK_LATCH: f32 = 6.0;
/// Respawn delays — a slain animal's species reappears nearby after this. 3× slower than the
/// old pace (was 35/50) so the wilds don't refill the moment you turn your back — culled game
/// stays culled long enough to matter, and pushes the hero to range farther for fresh hunts.
const ANIMAL_RESPAWN: f32 = 105.0;
const PREDATOR_RESPAWN: f32 = 150.0;

/// Town sanctuary radius (world units from the castle origin). Covers the grass safe-zone
/// (`worldmap::SAFE_R` = 18) PLUS the build-plot ring (farthest plot centre ~21.5 +
/// `town::PLOT_CLEAR_R`), so peasants working the farms stand inside it. Predators never
/// acquire a target standing in the sanctuary (struck-enrage still overrides — a wounded
/// predator fights back anywhere), and predator herd anchors / respawn homes keep their whole
/// wander circle outside it so packs don't mill around the gates.
const TOWN_SANCTUARY_R: f32 = 27.0;

fn in_sanctuary(p: Vec2) -> bool {
    p.length() < TOWN_SANCTUARY_R
}

/// Marks an articulated child part + how it animates.
#[derive(Component)]
struct AnimPart {
    kind: PartKind,
}

// ── Systems ──────────────────────────────────────────────────────────────────────

#[allow(clippy::type_complexity)]
fn animal_brain(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    hero: Res<crate::player::HeroState>,
    mut pending: ResMut<crate::player::PendingHeroDamage>,
    mut npc_dmg: ResMut<crate::villagers::NpcDamage>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut commands: Commands,
    townsfolk: Query<
        (Entity, &crate::villagers::Villager, &Visibility),
        (With<crate::villagers::Townsfolk>, Without<crate::dying::Dying>),
    >,
    mut q: Query<(Entity, &mut Animal, &mut Transform, Option<&Struck>), Without<crate::dying::Dying>>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let cam_xz = cam.single().ok().map(|g| {
        let t = g.translation();
        Vec2::new(t.x, t.z)
    });
    // The town pool is fair game for predators, exactly like the hero (hidden bodies excluded).
    let npcs: Vec<(Entity, Vec2)> = townsfolk
        .iter()
        .filter(|(_, _, vis)| **vis != Visibility::Hidden)
        .map(|(e, v, _)| (e, v.pos))
        .collect();

    // Snapshot every animal's (entity, position, predator?, prey?) so the food-chain can target:
    // predators hunt the nearest prey, prey flee the nearest predator. Read-only pre-pass.
    let snap: Vec<(Entity, Vec2, bool, bool)> = q
        .iter()
        .map(|(e, a, _, _)| (e, a.pos, predator_stats(a.species).is_some(), is_prey(a.species)))
        .collect();
    // Prey caught by a predator this frame (eaten → despawned after the loop).
    let mut eaten: Vec<Entity> = Vec::new();

    for (self_e, mut a, mut tf, struck) in &mut q {
        a.timer -= dt;
        a.atk_cd -= dt;

        let pred = predator_stats(a.species);
        // Struck-enrage: a wounded predator (incl. the boar) latches onto its attacker — the
        // hero, or the townsperson whose blade/axe just landed — for a beat.
        if let Some(s) = struck {
            commands.entity(self_e).remove::<Struck>();
            if pred.is_some() {
                a.aggro_until = now + STRUCK_LATCH;
                a.aggro_target = s.by;
            }
        }
        if let Some((aggro_r, _)) = pred {
            // ── Predator: hunt the nearest prey in range; failing that, the hero or a
            // townsperson (the town pool is treated exactly like the hero) — but only while
            // near home (~26u) so a pack doesn't trail a target across the whole island.
            // A recently-struck predator stays locked on its attacker (ignores leash). ──
            let near_home = a.pos.distance(a.home) < 26.0;
            // (position, prey-to-eat, townsperson-to-bite); both None = the hero.
            let mut tgt: Option<(Vec2, Option<Entity>, Option<Entity>)> = None;
            if now < a.aggro_until {
                match a.aggro_target {
                    None if hero.alive => tgt = Some((hero.pos, None, None)),
                    Some(ne) => match npcs.iter().find(|(e, _)| *e == ne) {
                        Some((_, np)) => tgt = Some((*np, None, Some(ne))),
                        None => a.aggro_until = 0.0, // attacker died/left — shake it off
                    },
                    _ => {}
                }
            }
            if tgt.is_none() && near_home {
                // Anyone standing in the town sanctuary is off-limits (guarded ground) — the
                // re-acquire below runs every frame, so a chase auto-drops the moment its
                // target crosses the ring. Only struck-enrage (above) pursues inside.
                let mut best = aggro_r;
                for (pe, pp, _ispred, isprey) in &snap {
                    if !*isprey || *pe == self_e || in_sanctuary(*pp) {
                        continue;
                    }
                    let d = a.pos.distance(*pp);
                    if d < best {
                        best = d;
                        tgt = Some((*pp, Some(*pe), None));
                    }
                }
                if tgt.is_none() {
                    // No prey about: the hero and the townsfolk are fair game alike — charge
                    // whichever is nearest in aggro range.
                    let mut best = aggro_r;
                    if hero.alive && !in_sanctuary(hero.pos) {
                        let d = a.pos.distance(hero.pos);
                        if d < best {
                            best = d;
                            tgt = Some((hero.pos, None, None));
                        }
                    }
                    for (ne, np) in &npcs {
                        if in_sanctuary(*np) {
                            continue;
                        }
                        let d = a.pos.distance(*np);
                        if d < best {
                            best = d;
                            tgt = Some((*np, None, Some(*ne)));
                        }
                    }
                }
            }
            if let Some((tp, prey, npc)) = tgt {
                a.mode = Mode::Hunt;
                a.target = tp;
                a.hunt_prey = prey;
                a.hunt_npc = npc;
            } else if a.mode == Mode::Hunt {
                a.mode = Mode::Graze;
                a.timer = rng_range(&mut a.rng, 1.0, 2.5);
                a.hunt_prey = None;
                a.hunt_npc = None;
            }
        } else if a.flee_r > 0.0 {
            // ── Prey: bolt from the nearest predator, else the hero, else the camera. ──
            let mut threat: Option<Vec2> = None;
            let mut best = a.flee_r;
            for (pe, pp, ispred, _isprey) in &snap {
                if !*ispred || *pe == self_e {
                    continue;
                }
                let d = a.pos.distance(*pp);
                if d < best {
                    best = d;
                    threat = Some(*pp);
                }
            }
            if hero.alive && a.pos.distance(hero.pos) < best {
                threat = Some(hero.pos);
            }
            if threat.is_none() {
                if let Some(cp) = cam_xz {
                    if a.pos.distance(cp) < a.flee_r {
                        threat = Some(cp);
                    }
                }
            }
            if let Some(tp) = threat {
                if a.mode != Mode::Flee {
                    a.voice_timer = 0.0;
                }
                let away = (a.pos - tp).normalize_or_zero();
                a.mode = Mode::Flee;
                a.timer = 2.2;
                a.target = a.pos + away * a.wander_r.max(6.0);
            }
        }

        match a.mode {
            Mode::Graze => {
                a.moving = false;
                if a.timer <= 0.0 {
                    pick_wander(&mut a);
                }
            }
            Mode::Wander | Mode::Flee => {
                let dist = (a.target - a.pos).length();
                if dist < 0.3 || a.timer <= 0.0 {
                    a.mode = Mode::Graze;
                    a.timer = rng_range(&mut a.rng, 2.0, 5.0);
                    a.moving = false;
                } else {
                    let spd = if a.mode == Mode::Flee { a.speed } else { a.wander_speed };
                    let cur_y = worldmap::ground_at_world(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
                    // Shared local steering (escape-fan + continuity bias + turn-rate cap) —
                    // the anti-flicker logic, identical for orks (see `steer.rs`).
                    match steer::advance(a.pos, a.facing, a.target, spd * dt, a.body_r, cur_y, MAX_TURN * dt) {
                        Some(s) => {
                            a.facing = s.facing;
                            a.pos = s.pos;
                            a.moving = s.moving;
                        }
                        None => {
                            // Boxed in — pause briefly, then pick a fresh heading.
                            a.mode = Mode::Graze;
                            a.timer = rng_range(&mut a.rng, 0.4, 1.0);
                            a.moving = false;
                        }
                    }
                }
            }
            Mode::Hunt => {
                // Charge the hero (target tracks him); bite on contact each cooldown.
                let to = a.target - a.pos;
                let d = to.length();
                if d < 1.2 {
                    a.moving = false;
                    if d > 1e-4 {
                        let want = to.x.atan2(to.y);
                        let turn = MAX_TURN * 1.8 * dt;
                        a.facing += steer::wrap_pi(want - a.facing).clamp(-turn, turn);
                    }
                    if let Some(prey_e) = a.hunt_prey {
                        eaten.push(prey_e); // caught the prey — it's taken down
                        a.mode = Mode::Graze;
                        a.timer = rng_range(&mut a.rng, 2.0, 4.0);
                        a.hunt_prey = None;
                        a.hunt_npc = None;
                    } else if a.atk_cd <= 0.0 {
                        a.atk_cd = 1.0;
                        a.atk_anim = now; // play the lunge-bite / tail-sting / arm-slam
                        if let Some((_, bite)) = pred {
                            // Frontier-graded bite: a rim predator hits ~1.6× as hard as one near
                            // the castle (pairs with its distance-scaled HP — far wilds bite back).
                            let (_, dmg_mul) = crate::verbs::frontier_threat(a.pos.x, a.pos.y);
                            let bite = bite * dmg_mul;
                            // The bite lands on whoever is being charged: a townsperson takes it
                            // through the NPC damage channel, the hero through his own.
                            if let Some(ne) = a.hunt_npc {
                                npc_dmg.0.push(crate::villagers::NpcHit {
                                    victim: ne,
                                    amount: bite,
                                    attacker: Some(self_e),
                                });
                            } else {
                                pending.0 += bite;
                            }
                            // Snarl as it bites — the creature-side SFX so an attack isn't silent.
                            let big = matches!(
                                a.species,
                                Species::PolarBear | Species::BogCroc | Species::Golem
                            );
                            cues.write(crate::audio::AudioCue::CreatureBite {
                                at: Vec3::new(a.pos.x, tf.translation.y + 0.8, a.pos.y),
                                big,
                            });
                        }
                    }
                } else {
                    let cur_y = worldmap::ground_at_world(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
                    match steer::advance(a.pos, a.facing, a.target, a.speed * dt, a.body_r, cur_y, MAX_TURN * 1.8 * dt) {
                        Some(s) => {
                            a.facing = s.facing;
                            a.pos = s.pos;
                            a.moving = s.moving;
                        }
                        None => a.moving = false,
                    }
                }
            }
        }

        // Decaying knockback shove from a recent hero blow — slid against terrain (so a shove
        // can't punt an animal through a cliff/water), mirroring `orks::apply_knockback`.
        if a.kb.length_squared() > 0.0025 {
            let cur_y = worldmap::ground_at_world(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
            let step = a.kb * dt;
            if steer::can_stand(a.pos.x + step.x, a.pos.y, a.body_r, cur_y) {
                a.pos.x += step.x;
            }
            if steer::can_stand(a.pos.x, a.pos.y + step.y, a.body_r, cur_y) {
                a.pos.y += step.y;
            }
            a.kb *= (1.0 - 9.0 * dt).max(0.0);
        } else {
            a.kb = Vec2::ZERO;
        }

        // Ground-follow + heading + a small bob while moving. A quick forward lunge over the
        // strike beat sells the bite's weight — visual only (`pos` is untouched, so collision and
        // gameplay are unaffected).
        let gy = worldmap::ground_at_world(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
        let bob = if a.moving { (tw * a.gait + a.phase).sin().abs() * a.bob } else { 0.0 };
        let lunge = strike_p(a.atk_anim, now, arch_dur(strike_arch(a.species)))
            .map_or(0.0, |p| (p * std::f32::consts::PI).sin() * 0.4);
        let fwd = Vec2::new(a.facing.sin(), a.facing.cos());
        let lp = a.pos + fwd * lunge;
        tf.translation = Vec3::new(lp.x, gy + bob, lp.y);
        // Springy recoil-wobble on a blow taken (reuses the orks' / dummies' shape).
        tf.rotation = Quat::from_rotation_y(a.facing) * Quat::from_rotation_x(crate::orks::recoil_tilt(a.hit_recoil, now));
    }

    // Reap prey caught by predators this frame (try_despawn — two predators may share a kill).
    for e in eaten {
        commands.entity(e).try_despawn();
    }
}

/// Which limb a species strikes with — keys the attack pose in [`animal_limbs`].
#[derive(Clone, Copy, PartialEq)]
enum StrikeArch {
    /// Head-snap lunge (Wolf / PolarBear / Boar / BogCroc).
    Bite,
    /// Tail stinger arcing over (Scorpion).
    Sting,
    /// Overhead arm crash (Golem).
    Slam,
    /// Non-attacker (grazers / neutral critters).
    None,
}

fn strike_arch(s: Species) -> StrikeArch {
    match s {
        Species::Wolf | Species::PolarBear | Species::Boar | Species::BogCroc => StrikeArch::Bite,
        Species::Scorpion => StrikeArch::Sting,
        Species::Golem => StrikeArch::Slam,
        _ => StrikeArch::None,
    }
}

/// Strike duration per archetype — the golem's crush is slow + heavy.
fn arch_dur(a: StrikeArch) -> f32 {
    match a {
        StrikeArch::Slam => 0.7,
        _ => 0.45,
    }
}

/// Strike progress `0..1` since `atk_anim`, or `None` when not currently striking.
fn strike_p(atk_anim: f32, now: f32, dur: f32) -> Option<f32> {
    if atk_anim <= 0.0 {
        return None;
    }
    let p = (now - atk_anim) / dur;
    (0.0..1.0).contains(&p).then_some(p)
}

/// Head-snap bite on X: lift the muzzle (wind), snap down-forward hard, recover.
fn head_bite_x(p: f32) -> f32 {
    if p < 0.3 {
        let u = p / 0.3;
        -0.55 * (u * u)
    } else if p < 0.55 {
        let u = (p - 0.3) / 0.25;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        -0.55 + 1.75 * e
    } else {
        let u = (p - 0.55) / 0.45;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        1.2 * (1.0 - e)
    }
}

/// Scorpion tail sting on X: arch the stinger up + over, snap it forward, recover.
fn tail_sting_x(p: f32) -> f32 {
    if p < 0.4 {
        let u = p / 0.4;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        -1.7 * e
    } else {
        let u = (p - 0.4) / 0.6;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        -1.7 + 1.7 * e
    }
}

/// Golem arm slam on X: raise overhead, crash down past rest, recover.
fn arm_slam_x(p: f32) -> f32 {
    if p < 0.4 {
        let u = p / 0.4;
        -1.8 * (u * u)
    } else if p < 0.62 {
        let u = (p - 0.4) / 0.22;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        -1.8 + 2.7 * e
    } else {
        let u = (p - 0.62) / 0.38;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        0.9 * (1.0 - e)
    }
}

/// Squared camera distance past which limb animation is skipped (actor is a fogged/blurred
/// smudge by then). Shared by [`animal_limbs`]; orks/villagers carry their own copy.
const LIMB_CULL2: f32 = 70.0 * 70.0;

fn animal_limbs(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    animals: Query<(&Animal, &Children, &GlobalTransform)>,
    mut parts: Query<(&AnimPart, &mut Transform)>,
) {
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    // Past ~70 tiles an actor is fog- and DoF-blurred to a smudge, so skinning its joints is
    // wasted CPU. Freeze the rig (it resumes from global-time sines on re-entry, no snap).
    let cam_p = cam.single().ok().map(|g| g.translation());
    for (a, children, gt) in &animals {
        if let Some(cp) = cam_p {
            if gt.translation().distance_squared(cp) > LIMB_CULL2 {
                continue;
            }
        }
        let t = tw + a.phase;
        let arch = strike_arch(a.species);
        let strike = strike_p(a.atk_anim, now, arch_dur(arch));
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                PartKind::Leg(sign) => {
                    let s = if a.moving { (t * a.gait).sin() * a.swing } else { (t * 0.8).sin() * 0.04 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Head => match (arch, strike) {
                    (StrikeArch::Bite, Some(p)) => Quat::from_rotation_x(head_bite_x(p)),
                    _ => {
                        let bob = (t * 0.5).sin() * 0.07;
                        let scan = if a.moving { 0.0 } else { (t * 0.4).sin() * 0.22 };
                        Quat::from_euler(EulerRot::XYZ, bob, scan, 0.0)
                    }
                },
                PartKind::Tail => match (arch, strike) {
                    (StrikeArch::Sting, Some(p)) => Quat::from_rotation_x(tail_sting_x(p)),
                    _ => {
                        let wag = (t * if a.moving { 10.0 } else { 3.0 }).sin() * 0.4;
                        Quat::from_rotation_y(wag)
                    }
                },
                // Golem pincers slam; other rigs leave their arms at rest.
                PartKind::Arm(_) => match (arch, strike) {
                    (StrikeArch::Slam, Some(p)) => Quat::from_rotation_x(arm_slam_x(p)),
                    _ => Quat::IDENTITY,
                },
            };
        }
    }
}

/// Queue a respawn for each player-slain animal (reads combat's `AnimalKilled` message).
fn enqueue_respawn(
    time: Res<Time>,
    mut kills: MessageReader<crate::verbs::AnimalKilled>,
    mut queue: ResMut<RespawnQueue>,
) {
    let now = time.elapsed_secs();
    for k in kills.read() {
        let delay = if predator_stats(k.species).is_some() { PREDATOR_RESPAWN } else { ANIMAL_RESPAWN };
        queue.0.push(RespawnSlot { species: k.species, home: Vec2::new(k.at.x, k.at.z), at: now + delay });
    }
}

/// Respawn each due animal of its species near its death spot, reusing the kept spawn templates.
fn drain_respawns(
    time: Res<Time>,
    assets: Option<Res<WildlifeAssets>>,
    mut queue: ResMut<RespawnQueue>,
    mut commands: Commands,
    mut rng: Local<u32>,
) {
    let Some(assets) = assets else { return };
    if *rng == 0 {
        *rng = 0x51ed_270b;
    }
    let now = time.elapsed_secs();
    let r = &mut *rng;
    queue.0.retain(|slot| {
        if now < slot.at {
            return true;
        }
        if let (Some(plan), Some(tmpl)) =
            (PLANS.iter().find(|p| p.species == slot.species), assets.template(slot.species))
        {
            // A predator slain near town does NOT re-home at its death spot — that would walk
            // the pack to the gates one respawn cycle at a time. Push the home back out past
            // the sanctuary ring along the same bearing.
            let min_r = anchor_min_r(plan);
            let mut home = slot.home;
            if home.length() < min_r {
                home = if home.length() > 1e-3 {
                    home.normalize() * (min_r + 2.0)
                } else {
                    Vec2::new(min_r + 2.0, 0.0)
                };
            }
            for _ in 0..14 {
                let jx = home.x + rng_range(r, -3.0, 3.0);
                let jz = home.y + rng_range(r, -3.0, 3.0);
                if valid(plan.place, jx, jz, min_r) {
                    spawn_one(&mut commands, &assets.mat, tmpl, plan, jx, jz, home, next_u32(r));
                    break;
                }
            }
        }
        false // drop the slot whether or not placement succeeded
    });
}

fn pick_wander(a: &mut Animal) {
    let ang = rng01(&mut a.rng) * std::f32::consts::TAU;
    let r = rng_range(&mut a.rng, a.wander_r * 0.3, a.wander_r);
    a.target = a.home + Vec2::new(ang.cos() * r, ang.sin() * r);
    a.mode = Mode::Wander;
    a.timer = rng_range(&mut a.rng, 4.0, 9.0);
}

/// The species that HUNT the hero (chase + bite) rather than fleeing → `(aggro radius, bite
/// damage)`. Everything else returns `None` and keeps its flee/graze behaviour.
fn predator_stats(s: Species) -> Option<(f32, f32)> {
    // Bite/charge damage is now the full old-game value from core (wolf 12, bear 24, boar 18,
    // scorpion 14, croc 20, golem 28) — the prior forest numbers were ~half that, which is why
    // wildlife felt toothless. Aggro radii stay forest-tuned for this scene.
    let dmg = |sp| {
        crate::verbs::core_species(sp)
            .map(|c| tileworld_core::animal::animal_config(c).attack_damage as f32)
            .unwrap_or(0.0)
    };
    match s {
        Species::Wolf => Some((13.0, dmg(s))),
        Species::PolarBear => Some((11.0, dmg(s))),
        Species::Boar => Some((8.0, dmg(s))),
        // The three biome menaces — they hunt/charge the hero on sight.
        Species::Scorpion => Some((12.0, dmg(s))), // fast, venomous, frequent stings
        Species::BogCroc => Some((9.0, dmg(s))),   // swamp ambusher, heavy bite
        Species::Golem => Some((9.0, dmg(s))),     // slow stone brute, crushing blows
        _ => None,
    }
}

/// The species the town militia treats as hostile (read by `villagers::guard_combat` and the
/// woodcutter's threat sense in `lumberjack.rs`) — exactly the set that hunts people.
pub(crate) fn is_hostile_species(s: Species) -> bool {
    predator_stats(s).is_some()
}

/// Grazers the predators hunt (the food-chain prey set). Dog/Cat are neutral critters — neither
/// predator nor prey — so they only startle from the camera.
fn is_prey(s: Species) -> bool {
    matches!(s, Species::Deer | Species::Elk | Species::Rabbit | Species::Goat | Species::Camel)
}

// ── Placement ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Place {
    Grass,
    Forest,
    GrassOrForest,
    Snow,
    Rock,
    Desert,
    Swamp,
}

fn place_ok(place: Place, x: f32, z: f32) -> bool {
    let b = worldmap::biome_at_world(x, z);
    match place {
        Place::Grass => worldmap::is_grass_world(x, z),
        Place::Forest => b == Some(Biome::Forest),
        Place::GrassOrForest => worldmap::is_grass_world(x, z) || b == Some(Biome::Forest),
        Place::Snow => b == Some(Biome::Snow),
        Place::Rock => b == Some(Biome::Rocky),
        Place::Desert => b == Some(Biome::Desert),
        Place::Swamp => b == Some(Biome::Swamp),
    }
}

fn valid(place: Place, x: f32, z: f32, min_r: f32) -> bool {
    worldmap::ground_at_world(x, z).is_some()
        && !crate::blockers::is_blocked(x, z)
        && !crate::camps::in_clearing(x, z) // keep herds out of the ork camps
        && !crate::town::near_build_plot(x, z) // keep animals off the farm/building plots
        && (x * x + z * z).sqrt() >= min_r // castle/town exclusion (wider for predators)
        && place_ok(place, x, z)
}

/// Minimum anchor distance from the castle for a species: a predator keeps its whole wander
/// circle outside the town sanctuary; grazers only stay off the forced-grass safe-zone (deer at
/// the walls are charming, wolves at the walls eat the peasants).
fn anchor_min_r(plan: &Plan) -> f32 {
    if predator_stats(plan.species).is_some() {
        TOWN_SANCTUARY_R + plan.wander_r
    } else {
        worldmap::SAFE_R
    }
}

/// One species' population + behaviour, all `Copy` so the table is a plain `const`.
#[derive(Clone, Copy)]
struct Plan {
    species: Species,
    count: u32,
    cluster: u32, // herd / pack members per anchor
    scale: f32,
    speed: f32,
    wander_speed: f32,
    flee_r: f32,
    wander_r: f32,
    gait: f32,
    swing: f32,
    bob: f32,
    place: Place,
}

const PLANS: [Plan; 13] = [
    // Deer — grass + forest herds; skittish.
    Plan { species: Species::Deer, count: 10, cluster: 4, scale: 0.8, speed: 6.0, wander_speed: 1.6, flee_r: 14.0, wander_r: 12.0, gait: 13.0, swing: 0.7, bob: 0.06, place: Place::GrassOrForest },
    // Elk — forest herds; larger, calmer gait.
    Plan { species: Species::Elk, count: 8, cluster: 4, scale: 0.95, speed: 5.5, wander_speed: 1.4, flee_r: 13.0, wander_r: 12.0, gait: 10.0, swing: 0.55, bob: 0.05, place: Place::Forest },
    // Rabbit — grass, small clusters; very skittish + bouncy.
    Plan { species: Species::Rabbit, count: 10, cluster: 2, scale: 0.65, speed: 6.5, wander_speed: 1.6, flee_r: 16.0, wander_r: 8.0, gait: 14.0, swing: 0.5, bob: 0.12, place: Place::Grass },
    // Boar — forest/frontier loners; mild flee.
    Plan { species: Species::Boar, count: 5, cluster: 1, scale: 0.8, speed: 4.0, wander_speed: 1.0, flee_r: 8.0, wander_r: 10.0, gait: 11.0, swing: 0.5, bob: 0.04, place: Place::GrassOrForest },
    // Wolf — forest packs; apex, ignores the camera.
    Plan { species: Species::Wolf, count: 6, cluster: 3, scale: 0.8, speed: 5.0, wander_speed: 1.4, flee_r: 0.0, wander_r: 16.0, gait: 12.0, swing: 0.6, bob: 0.04, place: Place::Forest },
    // Goat — rock highlands; nimble, roams the terraces.
    Plan { species: Species::Goat, count: 7, cluster: 2, scale: 0.7, speed: 5.5, wander_speed: 1.5, flee_r: 12.0, wander_r: 10.0, gait: 12.0, swing: 0.6, bob: 0.04, place: Place::Rock },
    // Polar bear — snow massif loners; apex, slow & heavy.
    Plan { species: Species::PolarBear, count: 4, cluster: 1, scale: 1.0, speed: 4.0, wander_speed: 1.0, flee_r: 0.0, wander_r: 14.0, gait: 11.0, swing: 0.6, bob: 0.04, place: Place::Snow },
    // Camel — desert herds; tall, slow & placid (mild flee).
    Plan { species: Species::Camel, count: 6, cluster: 3, scale: 1.0, speed: 4.0, wander_speed: 1.0, flee_r: 6.0, wander_r: 12.0, gait: 9.0, swing: 0.5, bob: 0.05, place: Place::Desert },
    // Dog — grass/forest frontier; small packs, curious (mild flee), bouncy trot.
    Plan { species: Species::Dog, count: 6, cluster: 2, scale: 0.55, speed: 5.5, wander_speed: 1.5, flee_r: 7.0, wander_r: 12.0, gait: 14.0, swing: 0.6, bob: 0.07, place: Place::GrassOrForest },
    // Cat — grassland loners; tiny, skittish, quick.
    Plan { species: Species::Cat, count: 6, cluster: 1, scale: 0.4, speed: 6.0, wander_speed: 1.4, flee_r: 12.0, wander_r: 9.0, gait: 13.0, swing: 0.5, bob: 0.06, place: Place::Grass },
    // Scorpion — desert packs; fast venomous predator (hunts the hero).
    Plan { species: Species::Scorpion, count: 6, cluster: 2, scale: 0.6, speed: 5.5, wander_speed: 1.5, flee_r: 0.0, wander_r: 14.0, gait: 16.0, swing: 0.65, bob: 0.04, place: Place::Desert },
    // Bog croc — swamp ambushers; sparse, slow, heavy charge.
    Plan { species: Species::BogCroc, count: 4, cluster: 1, scale: 0.7, speed: 4.2, wander_speed: 0.9, flee_r: 0.0, wander_r: 10.0, gait: 11.0, swing: 0.6, bob: 0.03, place: Place::Swamp },
    // Golem — rocky-biome bruiser; very rare, slow, devastating.
    Plan { species: Species::Golem, count: 2, cluster: 1, scale: 0.85, speed: 3.0, wander_speed: 0.7, flee_r: 0.0, wander_r: 12.0, gait: 9.0, swing: 0.45, bob: 0.03, place: Place::Rock },
];

/// Per-species uploaded meshes, ready to clone-spawn.
#[derive(Clone)]
struct Template {
    torso: Handle<Mesh>,
    parts: Vec<(PartKind, Vec3, Handle<Mesh>)>,
}

/// Retained spawn assets (shared material + per-species templates) so a slain animal's species
/// can be respawned later. Inserted by [`populate`].
#[derive(Resource)]
struct WildlifeAssets {
    mat: Handle<StandardMaterial>,
    templates: Vec<(Species, Template)>,
}
impl WildlifeAssets {
    fn template(&self, s: Species) -> Option<&Template> {
        self.templates.iter().find(|(sp, _)| *sp == s).map(|(_, t)| t)
    }
}

/// Pending wildlife respawns — a slain animal's species reappears near its death spot.
#[derive(Resource, Default)]
struct RespawnQueue(Vec<RespawnSlot>);
struct RespawnSlot {
    species: Species,
    home: Vec2,
    /// Elapsed-seconds when the replacement is due.
    at: f32,
}

/// Spawn the whole wildlife population. Called from `worldmap::build` (combined map only).
pub fn populate(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // One shared white vertex-colour material — every part bakes its hue into the mesh,
    // so the renderer batches all wildlife into few draw calls (same as the scatter).
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        ..default()
    });

    let mut rng: u32 = 0x0a17_5eed;
    let mut templates: Vec<(Species, Template)> = Vec::new();

    for plan in PLANS {
        let spec = critters::build(plan.species);
        let tmpl = Template {
            torso: meshes.add(spec.torso),
            parts: spec.parts.into_iter().map(|p| (p.kind, p.pivot, meshes.add(p.mesh))).collect(),
        };

        let mut placed = 0u32;
        let mut attempts = 0u32;
        let attempt_cap = plan.count * 300 + 600;
        while placed < plan.count && attempts < attempt_cap {
            attempts += 1;
            // Reject-sample a valid herd anchor inside the island.
            let ax = rng_range(&mut rng, -worldmap::GX + 5.0, worldmap::GX - 5.0);
            let az = rng_range(&mut rng, -worldmap::GZ + 5.0, worldmap::GZ - 5.0);
            if !valid(plan.place, ax, az, anchor_min_r(&plan)) {
                continue;
            }
            let home = Vec2::new(ax, az);
            let want = plan.cluster.min(plan.count - placed);
            for _ in 0..want {
                // Jitter each member around the anchor onto valid ground.
                let mut pos = None;
                for _ in 0..14 {
                    let jx = ax + rng_range(&mut rng, -3.0, 3.0);
                    let jz = az + rng_range(&mut rng, -3.0, 3.0);
                    if valid(plan.place, jx, jz, anchor_min_r(&plan)) {
                        pos = Some((jx, jz));
                        break;
                    }
                }
                if let Some((px, pz)) = pos {
                    let seed = next_u32(&mut rng);
                    spawn_one(commands, &mat, &tmpl, &plan, px, pz, home, seed);
                    placed += 1;
                    if placed >= plan.count {
                        break;
                    }
                }
            }
        }
        if placed < plan.count {
            info!("wildlife: placed {}/{} {:?}", placed, plan.count, plan.species);
        }
        templates.push((plan.species, tmpl));
    }
    // A few dogs & cats living in the castle suburbs, so the town has life underfoot. Homed near
    // the origin (the castle safe-zone is grass) so they mill around the houses, not the wilds.
    for (species, n) in [(Species::Dog, 3u32), (Species::Cat, 3u32)] {
        let (Some(plan), Some((_, tmpl))) = (
            PLANS.iter().find(|p| p.species == species),
            templates.iter().find(|(s, _)| *s == species),
        ) else {
            continue;
        };
        let mut placed = 0;
        let mut tries = 0;
        while placed < n && tries < 400 {
            tries += 1;
            let ax = rng_range(&mut rng, -16.0, 16.0);
            let az = rng_range(&mut rng, -16.0, 16.0);
            let p = Vec2::new(ax, az);
            // Open grass, outside the keep core, off any wall/house/prop blocker or build plot.
            if p.length() < 6.0
                || !worldmap::is_grass_world(ax, az)
                || crate::blockers::is_blocked(ax, az)
                || crate::town::near_build_plot(ax, az)
            {
                continue;
            }
            spawn_one(commands, &mat, tmpl, plan, ax, az, p, next_u32(&mut rng));
            placed += 1;
        }
    }

    // Retain the templates + material so slain animals can be respawned.
    commands.insert_resource(WildlifeAssets { mat, templates });
}

#[allow(clippy::too_many_arguments)]
fn spawn_one(
    commands: &mut Commands,
    mat: &Handle<StandardMaterial>,
    tmpl: &Template,
    plan: &Plan,
    x: f32,
    z: f32,
    home: Vec2,
    seed: u32,
) {
    let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
    let mut rng = seed | 1;
    let phase = rng01(&mut rng) * std::f32::consts::TAU;
    let facing = rng01(&mut rng) * std::f32::consts::TAU;

    let animal = Animal {
        species: plan.species,
        // Stagger first calls across the population so they don't all fire at t≈0.
        voice_timer: rng_range(&mut rng, 5.0, 60.0),
        call_cd: 0.0,
        mode: Mode::Graze,
        home,
        target: Vec2::new(x, z),
        pos: Vec2::new(x, z),
        facing,
        speed: plan.speed,
        wander_speed: plan.wander_speed,
        flee_r: plan.flee_r,
        wander_r: plan.wander_r,
        gait: plan.gait,
        swing: plan.swing,
        bob: plan.bob,
        body_r: (plan.scale * 0.36).max(0.20),
        phase,
        moving: false,
        timer: rng_range(&mut rng, 0.5, 4.0),
        atk_cd: 0.0,
        atk_anim: 0.0,
        hit_recoil: 0.0,
        hunt_prey: None,
        hunt_npc: None,
        aggro_until: 0.0,
        aggro_target: None,
        kb: Vec2::ZERO,
        rng,
    };

    let root = commands
        .spawn((
            Transform { translation: Vec3::new(x, y, z), rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(plan.scale) },
            Visibility::Visible,
            animal,
            BiomeEntity,
        ))
        .id();

    commands.entity(root).with_children(|p| {
        p.spawn((Mesh3d(tmpl.torso.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
        for (kind, pivot, mesh) in &tmpl.parts {
            p.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(*pivot),
                AnimPart { kind: *kind },
            ));
        }
    });
}

// ── Deterministic mulberry32 RNG (matches the scatter's layout philosophy) ─────────

fn next_u32(s: &mut u32) -> u32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    t ^ (t >> 14)
}
fn rng01(s: &mut u32) -> f32 {
    next_u32(s) as f32 / 4_294_967_296.0
}
pub(crate) fn rng_range(s: &mut u32, lo: f32, hi: f32) -> f32 {
    lo + rng01(s) * (hi - lo)
}
