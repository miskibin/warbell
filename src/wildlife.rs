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
use crate::quadruped::{quad_meshes, spawn_quad, QuadDrive, QuadHandles, QuadSpecies};
use crate::steer::{self, footing};
use crate::worldmap;

/// The studio quadruped rig replaces the old box mesh for the mammals that have a studio model
/// (`None` keeps the bespoke box rig: the tiny rabbit/cat + the non-mammal monsters golem/scorpion/
/// croc). Elk reuses the deer (big-antlered), boar the bear, goat a smaller deer.
fn quad_species_for(s: Species) -> Option<QuadSpecies> {
    Some(match s {
        Species::Wolf => QuadSpecies::Wolf,
        Species::Deer | Species::Goat => QuadSpecies::Deer,
        Species::Elk => QuadSpecies::Deer,
        Species::Boar => QuadSpecies::Bear,
        Species::PolarBear => QuadSpecies::PolarBear,
        Species::Camel => QuadSpecies::Camel,
        Species::Dog => QuadSpecies::Dog,
        _ => return None,
    })
}

/// Visual size-up applied to the studio quadruped rig (studio dims are ~half our old box meshes per
/// unit). Multiplies the per-species `Plan.scale`; the collision `body_r` still tracks `Plan.scale`.
/// Tuned by `FOREST_WILDLINE` capture.
const QUAD_SIZE_FIX: f32 = 1.5;

/// Max facing turn rate (rad/s). Caps how fast an animal can rotate so it never snaps
/// 180° between frames — the cure for the steering-oscillation flicker.
const MAX_TURN: f32 = 3.5;

pub struct WildlifePlugin;

impl Plugin for WildlifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RespawnQueue>();
        app.add_message::<PreyEaten>();
        // `animal_limbs` poses the old box rig; `quad_drive` feeds the studio quad rig (one or the
        // other applies per species). Both ungated so wildlife stays animated while the world freezes.
        app.add_systems(Update, (animal_limbs, quad_drive));
        app.add_systems(
            Update,
            (animal_brain, resolve_prey_kills, enqueue_respawn, drain_respawns)
                .chain()
                .run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

/// A prey animal taken down by a predator this frame (emitted by `animal_brain`). Resolved by
/// [`resolve_prey_kills`] into a death-fade + a respawn enqueue, so predation doesn't pop prey
/// instantly and doesn't grind a species to extinction (only player kills used to respawn).
#[derive(Message)]
struct PreyEaten(Entity);

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
    mut eaten_w: MessageWriter<PreyEaten>,
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
            commands.entity(self_e).try_remove::<Struck>();
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
                // Entering the hunt (idle/graze → hunt): a low stalk-growl, the audible "you've
                // been seen" tell before the charge. Only on the transition edge so a sustained
                // chase doesn't re-bark every frame (and the SFX side throttles pile-ups too).
                if !matches!(a.mode, Mode::Hunt) {
                    cues.write(crate::audio::AudioCue::CreatureAggro(Vec3::new(a.pos.x, 1.0, a.pos.y)));
                }
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
                    let cur_y = footing(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
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
                    let cur_y = footing(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
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

        // Decaying knockback shove from a recent hero blow — slid against terrain AND props (so a
        // shove can't punt an animal through a cliff/water or a wall), mirroring
        // `orks::apply_knockback`. Waive the prop test when already wedged inside one so the shove
        // can carry the animal out.
        if a.kb.length_squared() > 0.0025 {
            let cur_y = footing(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
            let inside = crate::blockers::is_blocked(a.pos.x, a.pos.y);
            let step = a.kb * dt;
            if steer::can_stand(a.pos.x + step.x, a.pos.y, a.body_r, cur_y)
                && (inside || !crate::blockers::is_blocked(a.pos.x + step.x, a.pos.y))
            {
                a.pos.x += step.x;
            }
            if steer::can_stand(a.pos.x, a.pos.y + step.y, a.body_r, cur_y)
                && (inside || !crate::blockers::is_blocked(a.pos.x, a.pos.y + step.y))
            {
                a.pos.y += step.y;
            }
            a.kb *= (1.0 - 9.0 * dt).max(0.0);
        } else {
            a.kb = Vec2::ZERO;
        }

        // Ground-follow + heading + a small bob while moving. A quick forward lunge over the
        // strike beat sells the bite's weight — visual only (`pos` is untouched, so collision and
        // gameplay are unaffected).
        // A 0.5 terrace step is *legal* to stand on (≤ MAX_STEP), so an animal validly parks at a
        // terrace edge — but a long, low body drawn at the CENTRE footing then sinks into the higher
        // step beside it. Ground off the highest footing under the body footprint instead, capped so
        // the downhill legs only float a little (a cheap stand-in for per-leg ground IK).
        let gy = body_ground_y(a.pos, a.body_r, tf.translation.y);
        let bob = if a.moving { (tw * a.gait + a.phase).sin().abs() * a.bob } else { 0.0 };
        let lunge = strike_p(a.atk_anim, now, arch_dur(strike_arch(a.species)))
            .map_or(0.0, |p| (p * std::f32::consts::PI).sin() * 0.22);
        let fwd = Vec2::new(a.facing.sin(), a.facing.cos());
        let mut lp = a.pos + fwd * lunge;
        // Hold the rendered body back from the knight by the body skin PLUS the predator's head
        // reach, so the head/jaws snapping forward over the bite land on his front instead of the
        // torso/body sliding (and the head poking) into him. The hero is shoved out to this same
        // line (`player::movement`), so a pressed-in hero can't force the overlap either. Grazers
        // add no reach → the old skin-touch clamp.
        if hero.alive {
            let keep = a.body_r + crate::orks::HERO_R + head_reach(a.species, a.body_r);
            lp = crate::orks::lunge_clear_of_hero(lp, hero.pos, keep);
        }
        tf.translation = Vec3::new(lp.x, gy + bob, lp.y);
        // Springy recoil-wobble on a blow taken (reuses the orks' / dummies' shape).
        tf.rotation = Quat::from_rotation_y(a.facing) * Quat::from_rotation_x(crate::orks::recoil_tilt(a.hit_recoil, now));
    }

    // Signal each prey caught this frame; `resolve_prey_kills` fades it (not an instant pop) and
    // queues its species to respawn so predators don't grind the herd to extinction. (Two predators
    // may share a kill → duplicate entities; the resolver dedups.)
    for e in eaten {
        eaten_w.write(PreyEaten(e));
    }
}

/// Turn each predator-eaten prey into a death-fade + a respawn enqueue (mirrors how player kills
/// respawn, minus the loot drop). Dedups same-frame double kills and skips prey already fading.
fn resolve_prey_kills(
    time: Res<Time>,
    mut eaten: MessageReader<PreyEaten>,
    mut commands: Commands,
    mut queue: ResMut<RespawnQueue>,
    prey_q: Query<(&Animal, &Transform), Without<crate::dying::Dying>>,
) {
    let now = time.elapsed_secs();
    let mut seen: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    for ev in eaten.read() {
        if !seen.insert(ev.0) {
            continue; // two predators reported the same kill
        }
        let Ok((a, tf)) = prey_q.get(ev.0) else { continue }; // already fading / gone
        let delay = if predator_stats(a.species).is_some() { PREDATOR_RESPAWN } else { ANIMAL_RESPAWN };
        queue.0.push(RespawnSlot {
            species: a.species,
            home: Vec2::new(tf.translation.x, tf.translation.z),
            at: now + delay,
        });
        crate::dying::begin_dying(&mut commands, ev.0, now);
    }
}

/// Ground height to draw a body at so it doesn't sink into an adjacent higher terrace step.
/// Samples footing at the body-footprint edges (a touch past `body_r` to cover the long axis) and
/// lifts the centre toward the HIGHEST nearby ground, but only up to [`STEP_CLEAR`] — so a body at
/// a terrace edge clears the step instead of burying in it, while the downhill legs float only a
/// little rather than a full step. `fallback` is used where footing is missing (over water/void).
fn body_ground_y(pos: Vec2, body_r: f32, fallback: f32) -> f32 {
    /// Max the body is lifted above its centre footing to clear a neighbouring step (world Y). A
    /// touch over half a terrace (`GROUND_STEP = 0.5`): enough to pull the body out of the step
    /// face, not so much that the low legs hang in the air.
    const STEP_CLEAR: f32 = 0.3;
    let center = footing(pos.x, pos.y).unwrap_or(fallback);
    let r = body_r * 1.2; // reach a little past the collision skin → covers the longer body axis
    let mut hi = center;
    for (dx, dz) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
        if let Some(y) = footing(pos.x + dx, pos.y + dz) {
            hi = hi.max(y);
        }
    }
    center + (hi - center).min(STEP_CLEAR)
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

/// Map each studio-rigged animal's brain state onto its [`QuadDrive`] each frame (the wildlife
/// mirror of `ork_drive`/`villager_drive`); [`crate::quadruped::animate_quad`] then poses the rig.
/// Skips dying animals so their pose freezes mid-stride while `dying.rs` crumples the body.
fn quad_drive(
    time: Res<Time>,
    mut q: Query<(&Animal, &mut QuadDrive), Without<crate::dying::Dying>>,
) {
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (a, mut d) in &mut q {
        let target = if a.moving { 1.0 } else { 0.0 };
        d.moving_amt += (target - d.moving_amt) * (dt * 8.0).min(1.0);
        // Charging the hero or bolting from the camera → run; relaxed roam → walk.
        d.run_amt = if matches!(a.mode, Mode::Hunt | Mode::Flee) { 1.0 } else { 0.0 };
        d.phase = tw + a.phase; // gait clock; `apply_gait` multiplies by the species walk/run speed
        // A predator's bite/charge swing (only predators stamp `atk_anim`).
        if let Some(p) = strike_p(a.atk_anim, now, 0.45) {
            d.attacking = true;
            d.attack_t = p;
        } else {
            d.attacking = false;
        }
    }
}

fn animal_limbs(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    hero: Res<crate::player::HeroState>,
    // Only box-rig animals carry `AnimPart`; the studio-rigged mammals are on the quad skeleton
    // (`QuadDrive` → `quad_drive`/`animate_quad`), so exclude them or this scans most of the island's
    // wildlife every frame for child lookups that always miss.
    animals: Query<(&Animal, &Children, &GlobalTransform), (Without<crate::dying::Dying>, Without<QuadDrive>)>,
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
                        // Idle/grazing animals lift the head toward the hero (alert glance);
                        // the shared helper handles range + scan + breathing bob.
                        let target = hero.alive.then_some(hero.pos);
                        crate::creature_anim::idle_head_glance(
                            a.pos,
                            a.facing,
                            t,
                            a.moving,
                            target,
                            crate::creature_anim::GlanceCfg { range: 12.0, max_yaw: 0.45, scan_amp: 0.22, bob_amp: 0.07 },
                        )
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

/// Extra keep-out (world units) a HUNTING predator reserves in front of its torso for the head /
/// jaws (or tail / arm) it snaps forward on a strike. The hero is shoved this much further back
/// (`player::movement`) so a biting head lands on his *front* instead of burying into his chest,
/// and the strike-lunge render is clamped to the same line so the snout just reaches the skin.
/// Grazers / neutral critters reserve nothing — you can brush right up to them. Scales with the
/// body so a stone golem reserves more reach than a wolf. `0` ⇒ no muzzle keep-out.
pub(crate) fn head_reach(s: Species, body_r: f32) -> f32 {
    if is_hostile_species(s) {
        (body_r * 0.85).max(0.22)
    } else {
        0.0
    }
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
        && !crate::rival::near_fort(x, z) // …and out of the rival stronghold
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

/// Per-species uploaded meshes, ready to clone-spawn. Exactly one of `box_rig`/`quad` is `Some`.
#[derive(Clone)]
struct Template {
    /// Old bespoke box rig (torso + articulated parts) — `Some` for non-studio species.
    box_rig: Option<BoxRig>,
    /// Studio quadruped rig handles + species — `Some` for the swapped mammals.
    quad: Option<(QuadSpecies, QuadHandles)>,
}

/// The old box-mesh rig: a merged torso + articulated parts (`AnimPart`, posed by `animal_limbs`).
#[derive(Clone)]
struct BoxRig {
    torso: Handle<Mesh>,
    parts: Vec<(PartKind, Vec3, Handle<Mesh>)>,
}

/// Retained spawn assets (shared material + per-species templates) so a slain animal's species
/// can be respawned later. Inserted by [`populate`].
#[derive(Resource)]
struct WildlifeAssets {
    mat: Handle<crate::creature::CreatureMaterial>,
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
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
    // One shared creature material — hue bakes into the mesh vertex colours; per-surface texture
    // (fur/scale/stone) comes from the alpha-packed surf code in the shader.
    let mat = crate::creature::make_creature_material(creature_mats);

    let mut rng: u32 = 0x0a17_5eed;
    let mut templates: Vec<(Species, Template)> = Vec::new();

    for plan in PLANS {
        let tmpl = if let Some(q) = quad_species_for(plan.species) {
            // Studio quadruped rig: upload its per-joint mesh set once, clone-spawn per animal.
            Template { box_rig: None, quad: Some((q, quad_meshes(q).upload(meshes))) }
        } else {
            let spec = critters::build(plan.species);
            Template {
                box_rig: Some(BoxRig {
                    torso: meshes.add(spec.torso),
                    parts: spec.parts.into_iter().map(|p| (p.kind, p.pivot, meshes.add(p.mesh))).collect(),
                }),
                quad: None,
            }
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

    // Screenshot hook: `FOREST_WILDLINE="x,z"` parks one of each surface-type exemplar in a line
    // at the given world XZ for model/texture close-ups (mirrors `FOREST_ORKLINE`). All five are
    // apex predators (`flee_r == 0`), so they don't bolt from the capture camera, and with no prey
    // or hero in range they idle in place — covering Fur (wolf/bear), Stone (golem), Scale
    // (scorpion/croc).
    if let Ok(s) = std::env::var("FOREST_WILDLINE") {
        let p: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        if p.len() == 2 {
            // The five studio quadruped archetypes (small→tall), to eyeball relative in-world scale.
            // All apex/placid here so they idle for the capture; swap in Golem/Scorpion/BogCroc to
            // inspect the kept-custom Stone/Scale rigs instead.
            let line = [
                Species::Dog,
                Species::Wolf,
                Species::Deer,
                Species::PolarBear,
                Species::Camel,
            ];
            for (i, sp) in line.iter().enumerate() {
                if let (Some(plan), Some((_, tmpl))) = (
                    PLANS.iter().find(|pl| pl.species == *sp),
                    templates.iter().find(|(t, _)| t == sp),
                ) {
                    let x = p[0] + i as f32 * 2.6 - 5.2;
                    let z = p[1];
                    spawn_one(commands, &mat, tmpl, plan, x, z, Vec2::new(x, z), 7 + i as u32);
                }
            }
        }
    }

    // Retain the templates + material so slain animals can be respawned.
    commands.insert_resource(WildlifeAssets { mat, templates });
}

#[allow(clippy::too_many_arguments)]
fn spawn_one(
    commands: &mut Commands,
    mat: &Handle<crate::creature::CreatureMaterial>,
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

    // The studio quad rig stands taller per unit, so size it up; `body_r` still tracks `plan.scale`.
    let vis_scale = if tmpl.quad.is_some() { plan.scale * QUAD_SIZE_FIX } else { plan.scale };
    let root = commands
        .spawn((
            Transform { translation: Vec3::new(x, y, z), rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(vis_scale) },
            Visibility::Visible,
            animal,
            BiomeEntity,
        ))
        .id();

    if let Some((q, h)) = &tmpl.quad {
        // Studio quadruped: the joint tree drives off `QuadDrive` (filled by `quad_drive`).
        commands.entity(root).insert(QuadDrive::new(*q));
        spawn_quad(commands, root, mat, *q, h.clone());
    } else if let Some(rig) = &tmpl.box_rig {
        commands.entity(root).with_children(|p| {
            p.spawn((Mesh3d(rig.torso.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
            for (kind, pivot, mesh) in &rig.parts {
                p.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(*pivot),
                    AnimPart { kind: *kind },
                ));
            }
        });
    }
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
