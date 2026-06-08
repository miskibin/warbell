//! Ambient wildlife — the living layer over the world map. Seven species
//! (`critters::Species`) wander, graze and startle from the camera, with their limbs
//! swung procedurally (the `wind.rs` `Sway` trick, applied to legs/head/tail).
//!
//! This is a VIEWER, so there's no combat: no HP, damage, death or respawn. Animals are
//! placed once (biome-matched, walkable, deterministic) inside `worldmap::build`, tagged
//! [`crate::biome::BiomeEntity`] so the biome-switch despawn/rebuild handles them.
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
        app.add_systems(Update, animal_limbs); // limb anim keeps running while frozen
        app.add_systems(Update, animal_brain.run_if(in_state(crate::game_state::Modal::None)));
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
    pos: Vec2,
    facing: f32,
    speed: f32,        // flee / move-with-purpose speed
    wander_speed: f32, // relaxed roam speed
    flee_r: f32,       // camera startle radius (0 = apex predator, ignores the camera)
    wander_r: f32,     // how far from `home` it roams
    gait: f32,         // leg-swing frequency while moving
    swing: f32,        // leg-swing amplitude
    bob: f32,          // vertical body bob while moving
    body_r: f32,       // footprint half-width (collision + cliff-edge footing)
    phase: f32,        // per-instance time offset (desyncs the animation)
    moving: bool,
    timer: f32,
    /// Predator bite cooldown (s); a bite lands at ≤ 0 and resets it.
    atk_cd: f32,
    /// When hunting, the prey entity being chased (None = hunting the hero / not hunting).
    hunt_prey: Option<Entity>,
    pub(crate) rng: u32,
}

/// Marks an articulated child part + how it animates.
#[derive(Component)]
struct AnimPart {
    kind: PartKind,
}

// ── Systems ──────────────────────────────────────────────────────────────────────

fn animal_brain(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    hero: Res<crate::player::HeroState>,
    mut pending: ResMut<crate::player::PendingHeroDamage>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Animal, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let cam_xz = cam.single().ok().map(|g| {
        let t = g.translation();
        Vec2::new(t.x, t.z)
    });

    // Snapshot every animal's (entity, position, predator?, prey?) so the food-chain can target:
    // predators hunt the nearest prey, prey flee the nearest predator. Read-only pre-pass.
    let snap: Vec<(Entity, Vec2, bool, bool)> = q
        .iter()
        .map(|(e, a, _)| (e, a.pos, predator_stats(a.species).is_some(), is_prey(a.species)))
        .collect();
    // Prey caught by a predator this frame (eaten → despawned after the loop).
    let mut eaten: Vec<Entity> = Vec::new();

    for (self_e, mut a, mut tf) in &mut q {
        a.timer -= dt;
        a.atk_cd -= dt;

        let pred = predator_stats(a.species);
        if let Some((aggro_r, _)) = pred {
            // ── Predator: hunt the nearest prey in range; failing that, the hero — but only
            // while near home (~26u) so a pack doesn't trail a target across the whole island. ──
            let near_home = a.pos.distance(a.home) < 26.0;
            let mut tgt: Option<(Vec2, Option<Entity>)> = None;
            let mut best = aggro_r;
            if near_home {
                for (pe, pp, _ispred, isprey) in &snap {
                    if !*isprey || *pe == self_e {
                        continue;
                    }
                    let d = a.pos.distance(*pp);
                    if d < best {
                        best = d;
                        tgt = Some((*pp, Some(*pe)));
                    }
                }
                if tgt.is_none() && hero.alive && a.pos.distance(hero.pos) < aggro_r {
                    tgt = Some((hero.pos, None));
                }
            }
            if let Some((tp, prey)) = tgt {
                a.mode = Mode::Hunt;
                a.target = tp;
                a.hunt_prey = prey;
            } else if a.mode == Mode::Hunt {
                a.mode = Mode::Graze;
                a.timer = rng_range(&mut a.rng, 1.0, 2.5);
                a.hunt_prey = None;
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
                    } else if a.atk_cd <= 0.0 {
                        a.atk_cd = 1.0;
                        if let Some((_, bite)) = pred {
                            pending.0 += bite;
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

        // Ground-follow + heading + a small bob while moving.
        let gy = worldmap::ground_at_world(a.pos.x, a.pos.y).unwrap_or(tf.translation.y);
        let bob = if a.moving { (tw * a.gait + a.phase).sin().abs() * a.bob } else { 0.0 };
        tf.translation = Vec3::new(a.pos.x, gy + bob, a.pos.y);
        tf.rotation = Quat::from_rotation_y(a.facing);
    }

    // Reap prey caught by predators this frame (try_despawn — two predators may share a kill).
    for e in eaten {
        commands.entity(e).try_despawn();
    }
}

fn animal_limbs(
    time: Res<Time>,
    animals: Query<(&Animal, &Children)>,
    mut parts: Query<(&AnimPart, &mut Transform)>,
) {
    let tw = time.elapsed_secs_wrapped();
    for (a, children) in &animals {
        let t = tw + a.phase;
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                PartKind::Leg(sign) => {
                    let s = if a.moving { (t * a.gait).sin() * a.swing } else { (t * 0.8).sin() * 0.04 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Head => {
                    let bob = (t * 0.5).sin() * 0.07;
                    let scan = if a.moving { 0.0 } else { (t * 0.4).sin() * 0.22 };
                    Quat::from_euler(EulerRot::XYZ, bob, scan, 0.0)
                }
                PartKind::Tail => {
                    let wag = (t * if a.moving { 10.0 } else { 3.0 }).sin() * 0.4;
                    Quat::from_rotation_y(wag)
                }
                // Quadruped wildlife has no arms; orks use `Arm` (see `orks.rs`).
                PartKind::Arm(_) => Quat::IDENTITY,
            };
        }
    }
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
    match s {
        Species::Wolf => Some((13.0, 5.0)),
        Species::PolarBear => Some((11.0, 13.0)),
        Species::Boar => Some((8.0, 7.0)),
        _ => None,
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
    }
}

fn valid(place: Place, x: f32, z: f32) -> bool {
    worldmap::ground_at_world(x, z).is_some()
        && !crate::blockers::is_blocked(x, z)
        && !crate::camps::in_clearing(x, z) // keep herds out of the ork camps
        && place_ok(place, x, z)
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

const PLANS: [Plan; 10] = [
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
];

/// Per-species uploaded meshes, ready to clone-spawn.
struct Template {
    torso: Handle<Mesh>,
    parts: Vec<(PartKind, Vec3, Handle<Mesh>)>,
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
            if !valid(plan.place, ax, az) {
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
                    if valid(plan.place, jx, jz) {
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
    }
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
        body_r: (plan.scale * 0.45).max(0.25),
        phase,
        moving: false,
        timer: rng_range(&mut rng, 0.5, 4.0),
        atk_cd: 0.0,
        hunt_prey: None,
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
