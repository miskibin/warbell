//! Ambush snowmen — the snow biome's "was that a prop?" hazard. A snowman sits as pure
//! static decor (indistinguishable from the scattered `biome_snow` centrepieces) until the
//! hero steps within [`WAKE_R`] or lands a blow on it; then it **lurches to life** (a squash-
//! stretch pop + a crunchy packed-snow grunt), shambles after the hero at a slow, kiteable
//! [`SPEED`], and body-slams him in melee. Outrun it — get past [`LEASH_R`] — and it loses
//! interest, waddles back to its spot and **re-freezes** into decor (re-triggerable).
//!
//! Reuse, not reinvention:
//! * Its mesh is [`crate::biome_snow::build_snowman_mesh`] (the same charming bałwan).
//! * It carries [`crate::player::Health`], so the hero's existing melee cone
//!   (`player::combat::player_attack`, which lists `With<Snowman>`) damages, floats, blood-
//!   sprays and **kills** it through the shared path — a kill routes into [`crate::dying`] like
//!   any ork/animal. We don't re-implement combat here.
//! * Movement rides the shared local-steering ([`crate::steer::advance`]) the wildlife/orks use.
//!
//! Deliberately **transient** (like the battlefield / wildlife): a slain snowman respawns at its
//! home after [`RESPAWN`], and NONE of this round-trips the save — there's no earned run-state to
//! lose, so it's on the CLAUDE.md "not saved" list.

use bevy::prelude::*;

use crate::audio::AudioCue;
use crate::player::{Health, HeroState, PendingHeroDamage};
use crate::steer::{self, footing};
use crate::game_state::SimAppExt;

// ── Tuning ──────────────────────────────────────────────────────────────────────────────────
/// Hero must step this close (world units) to a dormant snowman to wake it.
const WAKE_R: f32 = 4.0;
/// Once hunting, if the hero gets this far away the snowman gives up and leashes home. `> WAKE_R`
/// so there's hysteresis (it doesn't flip wake/leash on the boundary).
const LEASH_R: f32 = 9.0;
/// Slam reach — the hero takes a hit inside this.
const MELEE_R: f32 = 1.9;
/// Slam damage to the hero (orc-grunt-tier miniboss — a real bite, not chaff).
const SLAM_DMG: f32 = 20.0;
/// Seconds between slams.
const SLAM_CD: f32 = 1.5;
/// Shamble speed (units/s) — well under the hero's run so it's always kiteable.
const SPEED: f32 = 2.4;
/// Footprint radius for steering/footing.
const BODY_R: f32 = 0.42;
/// Max facing turn per second (rad) — a lumbering snowman turns slowly.
const MAX_TURN: f32 = 2.6;
/// Wake animation length (s): the anticipation-squash → overshoot-stretch → settle pop.
const WAKE_TIME: f32 = 0.55;
/// Full HP — read once into [`Health`]; the hero has full core power, so this is a tanky target.
const HP: f32 = 300.0;
/// A slain snowman reappears at its home spot after this (predator-tier — the wilds stay cleared).
const RESPAWN: f32 = 150.0;
/// Waddle leg-swing frequency while shambling.
const GAIT: f32 = 5.5;

/// Snow biome region centre — snowmen scatter around here (see CLAUDE.md biome table).
/// Authored at MAP_SCALE 2.2; `world22` rescales it to the current map size.
fn snow_centre() -> Vec2 {
    crate::worldmap::world22(-69.0, -45.0)
}
/// How far from [`snow_centre`] a snowman may be placed.
const SCATTER_R: f32 = 30.0;
/// Target number of ambush snowmen on the island.
const COUNT: usize = 10;
/// Minimum spacing between two snowmen so each reads as its own ambush.
const MIN_SEP: f32 = 6.0;

// ── State ───────────────────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq)]
enum State {
    /// Static decor — brain writes an identity pose so it's frozen like a scatter prop.
    Dormant,
    /// Mid "pop to life" — plays the squash-stretch for [`WAKE_TIME`], then hunts.
    Waking,
    /// Shambling after the hero, slamming in melee.
    Hunting,
    /// Lost the hero — shambling back home to re-freeze.
    Leashing,
}

#[derive(Component)]
pub struct Snowman {
    /// Index into [`SnowmanField::slots`] — so a death frees the right home to respawn.
    slot: usize,
    home: Vec2,
    home_facing: f32,
    pos: Vec2,
    facing: f32,
    scale: f32,
    state: State,
    /// Countdown while [`State::Waking`].
    wake_t: f32,
    /// Slam cooldown (s).
    atk_cd: f32,
    /// `elapsed_secs` of the last slam — drives the lunge animation. `0` = none.
    slam_at: f32,
    /// Animation phase (advances only while shambling) — desyncs the waddle.
    phase: f32,
    /// Last frame's `Health.hp` — a drop means the hero struck it this frame (edge-triggered wake).
    last_hp: f32,
}

/// One home spot: where a snowman lives, and its respawn bookkeeping.
struct Slot {
    pos: Vec2,
    facing: f32,
    scale: f32,
    occupied: bool,
    respawn_at: f32,
}

#[derive(Resource, Default)]
struct SnowmanField {
    slots: Vec<Slot>,
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    /// Set once the world is up and the field is seeded.
    seeded: bool,
}

pub struct SnowmanPlugin;

impl Plugin for SnowmanPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SnowmanField>();
        // Seed once the terrain exists (footing/biome sampling needs the built world). Ungated:
        // the body early-returns until `WorldReady`, then runs its one-shot spawn.
        app.add_systems(Update, seed_snowmen);
        // Sim tier — frozen with the rest of the world on a panel/pause (Modal only lives in Play).
        app.add_sim_systems(
            (snowman_brain, detect_snowman_deaths, respawn_snowmen)
                .chain()
                ,
        );
    }
}

/// Tiny deterministic xorshift so placement is reproducible run-to-run (no crate pull).
fn frand(s: &mut u32) -> f32 {
    if *s == 0 {
        *s = 0x1234_9abc;
    }
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32
}

/// Once the world is built, pick ~[`COUNT`] valid home spots in the snow region and spawn a
/// dormant snowman at each. One-shot (guarded by `field.seeded`).
fn seed_snowmen(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    ready: Res<crate::biome::WorldReady>,
    mut field: ResMut<SnowmanField>,
) {
    if field.seeded || !ready.0 {
        return;
    }
    field.seeded = true;

    // Shared assets: the bałwan mesh + one matte white vertex-colour material (the prop shading is
    // baked into the mesh, so every snowman batches against this single material).
    field.mesh = meshes.add(crate::biome_snow::build_snowman_mesh());
    field.mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        reflectance: 0.15,
        ..default()
    });

    let mut rng: u32 = 0xB0FF_0011;
    let mut accepted: Vec<Vec2> = Vec::new();
    let mut attempts = 0;
    while accepted.len() < COUNT && attempts < COUNT * 60 {
        attempts += 1;
        // Uniform-ish point in the scatter disc around the snow centre.
        let ang = frand(&mut rng) * std::f32::consts::TAU;
        let rad = SCATTER_R * frand(&mut rng).sqrt();
        let p = snow_centre() + Vec2::new(ang.cos(), ang.sin()) * rad;
        // Must be walkable snow, off any build plot, and spaced from its siblings.
        if footing(p.x, p.y).is_none()
            || crate::worldmap::biome_at_world(p.x, p.y) != Some(crate::biome::Biome::Snow)
            || crate::town::near_build_plot(p.x, p.y)
            || accepted.iter().any(|q| q.distance(p) < MIN_SEP)
        {
            continue;
        }
        accepted.push(p);
        let facing = frand(&mut rng) * std::f32::consts::TAU;
        let scale = 1.0 + frand(&mut rng) * 0.28; // a touch bigger than decor — these are minibosses
        let slot = field.slots.len();
        field.slots.push(Slot { pos: p, facing, scale, occupied: true, respawn_at: 0.0 });
        spawn_one(&mut commands, &field, slot, p, facing, scale);
    }
}

/// Spawn one dormant snowman entity for `slot` at `pos`.
fn spawn_one(commands: &mut Commands, field: &SnowmanField, slot: usize, pos: Vec2, facing: f32, scale: f32) {
    let y = footing(pos.x, pos.y).unwrap_or(0.0);
    commands.spawn((
        Mesh3d(field.mesh.clone()),
        MeshMaterial3d(field.mat.clone()),
        Transform::from_translation(Vec3::new(pos.x, y, pos.y))
            .with_rotation(Quat::from_rotation_y(facing))
            .with_scale(Vec3::splat(scale)),
        Health { hp: HP, max: HP },
        Snowman {
            slot,
            home: pos,
            home_facing: facing,
            pos,
            facing,
            scale,
            state: State::Dormant,
            wake_t: 0.0,
            atk_cd: 0.0,
            slam_at: 0.0,
            phase: 0.0,
            last_hp: HP,
        },
    ));
}

/// The whole snowman AI + animation: state machine, shamble steering, the slam, and the "come
/// alive" transform. Skips `Dying` snowmen so `dying.rs` owns the death crumple.
fn snowman_brain(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut pending: ResMut<PendingHeroDamage>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(&mut Snowman, &mut Transform, &Health), Without<crate::dying::Dying>>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let hpos = hero.pos;

    for (mut sn, mut tf, health) in &mut q {
        let d = sn.pos.distance(hpos);
        // Edge-triggered "struck": HP fell since last frame → the hero just hit it. Wakes even a
        // snowman the hero never stepped near (a sniping blow rouses it).
        let struck = health.hp < sn.last_hp - 0.01;
        sn.last_hp = health.hp;

        // ── State transitions ──
        match sn.state {
            State::Dormant => {
                if d <= WAKE_R || struck {
                    sn.state = State::Waking;
                    sn.wake_t = WAKE_TIME;
                    cues.write(AudioCue::SnowmanWake(head(sn.pos)));
                }
            }
            State::Waking => {
                sn.wake_t -= dt;
                if sn.wake_t <= 0.0 {
                    sn.state = State::Hunting;
                }
            }
            State::Hunting => {
                if d > LEASH_R {
                    sn.state = State::Leashing;
                }
            }
            State::Leashing => {
                // Re-aggro if the hero comes back into range (or lands another blow).
                if d <= WAKE_R || struck {
                    sn.state = State::Hunting;
                } else if sn.pos.distance(sn.home) < 0.35 {
                    // Home — re-freeze into decor.
                    sn.state = State::Dormant;
                    sn.facing = sn.home_facing;
                    sn.phase = 0.0;
                }
            }
        }

        // ── Movement + slam ──
        sn.atk_cd = (sn.atk_cd - dt).max(0.0);
        let cur_y = footing(sn.pos.x, sn.pos.y).unwrap_or(tf.translation.y);
        let mut moving = false;
        match sn.state {
            State::Hunting => {
                if d <= MELEE_R {
                    // In reach: stop and slam on cooldown.
                    if sn.atk_cd <= 0.0 {
                        sn.atk_cd = SLAM_CD;
                        sn.slam_at = now;
                        pending.0 += SLAM_DMG;
                        pending.1 = (hpos - sn.pos).normalize_or_zero(); // directional hit-shake
                        cues.write(AudioCue::SnowmanSlam(head(sn.pos)));
                    }
                } else if let Some(step) = steer::advance(sn.pos, sn.facing, hpos, SPEED * dt, BODY_R, cur_y, MAX_TURN * dt) {
                    sn.facing = step.facing;
                    sn.pos = step.pos;
                    moving = step.moving;
                }
            }
            State::Leashing => {
                if let Some(step) = steer::advance(sn.pos, sn.facing, sn.home, SPEED * dt, BODY_R, cur_y, MAX_TURN * dt) {
                    sn.facing = step.facing;
                    sn.pos = step.pos;
                    moving = step.moving;
                }
            }
            _ => {}
        }
        if moving {
            sn.phase += dt * GAIT;
        }

        // ── "Come alive" pose ──
        // Base = frozen decor: identity scale, no lean, sitting on the ground. Each active state
        // layers its motion on top. `sx`/`sy` are the horizontal/vertical scale (volume-preserving
        // squash-stretch); `lean_z` is the side-to-side waddle roll; `lean_x` a forward pitch (the
        // slam lunge); `bob` a vertical hop.
        let mut sy = 1.0f32;
        let mut lean_z = 0.0f32;
        let mut lean_x = 0.0f32;
        let mut bob = 0.0f32;
        match sn.state {
            State::Waking => {
                // 0→1 through the wake: anticipation dip, then a stretch overshoot that settles.
                let k = 1.0 - (sn.wake_t / WAKE_TIME).clamp(0.0, 1.0);
                // sin over the wake gives squash (k<.25) → tall stretch (k~.5) → settle; the -0.18
                // start makes it crouch before it springs.
                sy = 1.0 + 0.4 * (k * std::f32::consts::PI).sin() - 0.18 * (1.0 - k).powi(2);
                bob = 0.18 * (k * std::f32::consts::PI).sin();
            }
            State::Hunting | State::Leashing => {
                // Waddle: rock side to side + a little vertical bob, only while actually stepping.
                lean_z = (sn.phase.sin()) * 0.14;
                bob = (sn.phase.sin().abs()) * 0.06;
                sy = 1.0 + 0.03 * (sn.phase * 2.0).sin();
            }
            State::Dormant => {}
        }
        // Slam lunge overlay (any state, decays over ~0.4s): pitch forward hard then back, with a
        // downward squash on the impact.
        let slam_k = if sn.slam_at > 0.0 { ((now - sn.slam_at) / 0.4).clamp(0.0, 1.0) } else { 1.0 };
        if slam_k < 1.0 {
            let s = (slam_k * std::f32::consts::PI).sin();
            lean_x += s * 0.55;
            sy -= 0.12 * s;
        }
        // Horizontal scale counter-moves the vertical so the body reads as squashing, not growing.
        let sx = 1.0 + (1.0 - sy) * 0.5;

        tf.translation = Vec3::new(sn.pos.x, cur_y + bob, sn.pos.y);
        tf.rotation = Quat::from_rotation_y(sn.facing)
            * Quat::from_rotation_z(lean_z)
            * Quat::from_rotation_x(lean_x);
        tf.scale = Vec3::new(sx, sy, sx) * sn.scale;
    }
}

/// The spatial-audio anchor for a snowman at XZ `p` (roughly its head height).
fn head(p: Vec2) -> Vec3 {
    Vec3::new(p.x, footing(p.x, p.y).unwrap_or(0.0) + 1.0, p.y)
}

/// A snowman the hero just killed (combat inserted `Dying`): free its home slot and arm the
/// respawn clock so a fresh one rises there later.
fn detect_snowman_deaths(
    time: Res<Time>,
    mut field: ResMut<SnowmanField>,
    dead: Query<&Snowman, Added<crate::dying::Dying>>,
) {
    let now = time.elapsed_secs();
    for sn in &dead {
        if let Some(slot) = field.slots.get_mut(sn.slot) {
            slot.occupied = false;
            slot.respawn_at = now + RESPAWN;
        }
    }
}

/// Refill any empty home whose respawn timer has elapsed with a fresh dormant snowman.
fn respawn_snowmen(time: Res<Time>, mut commands: Commands, mut field: ResMut<SnowmanField>) {
    let now = time.elapsed_secs();
    // Collect the slots to refill first (can't spawn while holding a &mut into `field.slots`).
    let due: Vec<usize> = field
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.occupied && now >= s.respawn_at)
        .map(|(i, _)| i)
        .collect();
    for i in due {
        let (pos, facing, scale) = {
            let s = &field.slots[i];
            (s.pos, s.facing, s.scale)
        };
        spawn_one(&mut commands, &field, i, pos, facing, scale);
        field.slots[i].occupied = true;
    }
}
