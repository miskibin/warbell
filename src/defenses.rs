//! **Castle defenses** — the keep fights back at night. Four corner **watchtowers**, four
//! **keep archers**, and a **ballista** auto-fire cyan homing bolts at the wave invaders; a
//! **healing shrine** mends the hero inside the walls; the **war bell** (E during prep) rings
//! in the night early. Each structure is gated behind its Bulwark upgrade ([`Defenses`] flags),
//! so the castle only fights as hard as you've built it.
//!
//! Numbers come from the test-gated `tileworld_core::defense` fire profiles, with damage
//! **rescaled** into forest's 60-HP ork units (`DMG_SCALE`) so defenses *support* rather than
//! auto-clear. Bolt geometry reuses `projectile::advance_bolt`; targets are the night
//! [`WaveInvader`]s. Emitters are invisible logic points co-located with `castle.rs`'s meshes.
//!
//! Deferred (balance long-tail): tower destructibility + ork-targets-tower + prep revive — the
//! firing/shrine/bell core lands here; towers currently fire for the whole wave.

use bevy::prelude::*;
use tileworld_core::defense::{
    heal_step, is_ready, nearest_in_range, FireProfile, BALLISTA, KEEP_ARCHER, SHRINE_HEAL_PER_SEC,
    TOWER_BASE, TOWER_MASTERY, TOWER_MAX_HP,
};

use crate::audio::AudioCue;
use crate::economy::Defenses;
use crate::game_state::Modal;
use crate::orks::WaveInvader;
use crate::player::{spawn_burst, CombatFx, Health, HeroState, PlayerRes};
use crate::projectile::{advance_bolt, BoltStep};
use crate::siege::{GamePhase, Siege};

/// Rescale defender damage from core's TS-anchored values into forest's 60-HP ork units.
const DMG_SCALE: f32 = 0.236;
const HALF_X: f32 = 17.0;
const HALF_Z: f32 = 12.0;
/// World XZ of the courtyard war bell (matches `castle.rs`'s bell at `(0,0,6)`).
const BELL_POS: Vec2 = Vec2::new(0.0, 6.0);
const BELL_INTERACT_DIST: f32 = 4.2;
const DEFENDER_BOLT_TTL: f32 = 3.0;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Tower,
    Archer,
    Ballista,
}

/// A firing structure (logic point co-located with a castle mesh). Towers carry HP and can be
/// battered down by clustered invaders; archers/ballista are indestructible (`hp = INFINITY`).
#[derive(Component)]
struct Defender {
    kind: Kind,
    profile: FireProfile,
    muzzle: Vec3,
    ready_at: f32,
    hp: f32,
    max_hp: f32,
    /// Cooldown between taking batter damage (towers).
    batter_cd: f32,
}

/// Tower battering: invaders within this of a tower gnaw it down on the cadence below.
const TOWER_BATTER_RANGE: f32 = 3.0;
const TOWER_BATTER_CD: f32 = 1.0;
const TOWER_BATTER_DMG: f32 = 12.0;

/// A defender bolt in flight, homing on its target invader.
#[derive(Component)]
struct DefenderBolt {
    target: Entity,
    damage: f32,
    speed: f32,
    traveled: f32,
    max_range: f32,
    ttl: f32,
}

struct BoltOrder {
    origin: Vec3,
    target: Entity,
    damage: f32,
    speed: f32,
    max_range: f32,
}

#[derive(Resource, Default)]
struct DefenderBolts(Vec<BoltOrder>);

#[derive(Resource)]
struct BoltGfx {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

pub struct DefensePlugin;

impl Plugin for DefensePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DefenderBolts>()
            .add_systems(Startup, (setup_bolt_gfx, debug_enable_defenses))
            .add_systems(
                Update,
                (
                    defenders_fire,
                    spawn_defender_bolts,
                    step_defender_bolts,
                    batter_towers,
                    revive_towers,
                    shrine_heal,
                    war_bell,
                )
                    .run_if(in_state(Modal::None)),
            );
    }
}

fn setup_bolt_gfx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.14).mesh().ico(2).unwrap());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.95, 1.0),
        emissive: LinearRgba::rgb(0.6, 2.4, 3.2), // cyan defender bolt
        unlit: true,
        ..default()
    });
    commands.insert_resource(BoltGfx { mesh, mat });
}

/// `FOREST_DEFEND=1` arms every defense at boot so the harness can shoot a defended wave.
fn debug_enable_defenses(mut defenses: ResMut<Defenses>) {
    if std::env::var("FOREST_DEFEND").is_ok() {
        *defenses = Defenses {
            walls: true, // build the perimeter + gates so A* has gates to route invaders through
            gate: true,
            towers: true,
            tower_mastery: true,
            keep_archers: true,
            ballista: true,
            shrine: true,
            ..default()
        };
    }
}

/// During a wave, each armed structure picks the nearest invader in range and queues a bolt
/// (per-structure cooldown). Tower-Mastery swaps the base profile for the stronger one.
fn defenders_fire(
    time: Res<Time>,
    siege: Res<Siege>,
    defenses: Res<Defenses>,
    mut orders: ResMut<DefenderBolts>,
    mut emitters: Query<&mut Defender>,
    invaders: Query<(Entity, &Transform), With<WaveInvader>>,
) {
    if siege.phase != GamePhase::Wave {
        return;
    }
    let now = time.elapsed_secs();
    let targets: Vec<(Entity, f64, f64)> = invaders
        .iter()
        .map(|(e, tf)| (e, tf.translation.x as f64, tf.translation.z as f64))
        .collect();
    if targets.is_empty() {
        return;
    }
    let pts: Vec<(f64, f64)> = targets.iter().map(|t| (t.1, t.2)).collect();

    for mut d in &mut emitters {
        let enabled = match d.kind {
            Kind::Tower => defenses.towers,
            Kind::Archer => defenses.keep_archers,
            Kind::Ballista => defenses.ballista,
        };
        if !enabled || d.hp <= 0.0 || !is_ready(now as f64, d.ready_at as f64) {
            continue; // a battered-down tower stops firing until it's rebuilt at prep
        }
        let prof = if d.kind == Kind::Tower && defenses.tower_mastery { TOWER_MASTERY } else { d.profile };
        if let Some(i) = nearest_in_range(d.muzzle.x as f64, d.muzzle.z as f64, prof.range, &pts) {
            orders.0.push(BoltOrder {
                origin: d.muzzle,
                target: targets[i].0,
                damage: prof.damage as f32 * DMG_SCALE,
                speed: prof.speed as f32,
                max_range: prof.max_range as f32,
            });
            d.ready_at = now + prof.cooldown as f32;
        }
    }
}

fn spawn_defender_bolts(
    mut commands: Commands,
    gfx: Option<Res<BoltGfx>>,
    mut orders: ResMut<DefenderBolts>,
) {
    let Some(gfx) = gfx else {
        orders.0.clear();
        return;
    };
    for o in orders.0.drain(..) {
        commands.spawn((
            Mesh3d(gfx.mesh.clone()),
            MeshMaterial3d(gfx.mat.clone()),
            Transform::from_translation(o.origin),
            DefenderBolt {
                target: o.target,
                damage: o.damage,
                speed: o.speed,
                traveled: 0.0,
                max_range: o.max_range,
                ttl: DEFENDER_BOLT_TTL,
            },
            bevy::light::NotShadowCaster,
            crate::biome::BiomeEntity,
        ));
    }
}

/// Home each bolt on its target invader; on arrival deal (rescaled) damage and reap the invader
/// if it drops. Fizzles past range / ttl, or if the target despawned.
#[allow(clippy::type_complexity)]
fn step_defender_bolts(
    time: Res<Time>,
    fx: Option<Res<CombatFx>>,
    mut commands: Commands,
    invaders: Query<&Transform, (With<WaveInvader>, Without<DefenderBolt>)>,
    mut hp_q: Query<&mut Health>,
    mut q: Query<(Entity, &mut DefenderBolt, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut b, mut tf) in &mut q {
        b.ttl -= dt;
        let Ok(ttf) = invaders.get(b.target) else {
            commands.entity(e).despawn();
            continue;
        };
        if b.ttl <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let target = ttf.translation + Vec3::new(0.0, 0.9, 0.0);
        let (out, traveled) = advance_bolt(tf.translation, target, b.speed * dt, b.traveled, b.max_range);
        b.traveled = traveled;
        match out {
            BoltStep::Fly(p) => tf.translation = p,
            BoltStep::Hit => {
                if let Ok(mut hp) = hp_q.get_mut(b.target) {
                    if hp.hp > 0.0 {
                        hp.hp -= b.damage;
                        if hp.hp <= 0.0 {
                            // `try_despawn`: combat / the stuck-reaper / the victory clear may
                            // race to remove the same invader this frame — tolerate it being gone.
                            commands.entity(b.target).try_despawn();
                        }
                    }
                }
                if let Some(fx) = &fx {
                    spawn_burst(&mut commands, fx, tf.translation, false);
                }
                commands.entity(e).despawn();
            }
            BoltStep::Fizzle => commands.entity(e).despawn(),
        }
    }
}

/// Invaders clustered at a tower's base gnaw it down; a tower at 0 HP stops firing (it's rubble
/// until prep rebuilds it). Damage is proximity-driven so the column besieging the walls wears
/// the towers without the invader brain needing to target them.
fn batter_towers(
    time: Res<Time>,
    siege: Res<Siege>,
    mut towers: Query<&mut Defender>,
    invaders: Query<&Transform, With<WaveInvader>>,
) {
    if siege.phase != GamePhase::Wave {
        return;
    }
    let dt = time.delta_secs();
    let pts: Vec<Vec2> = invaders.iter().map(|tf| Vec2::new(tf.translation.x, tf.translation.z)).collect();
    if pts.is_empty() {
        return;
    }
    for mut d in &mut towers {
        if d.kind != Kind::Tower || d.hp <= 0.0 {
            continue;
        }
        d.batter_cd -= dt;
        let base = Vec2::new(d.muzzle.x, d.muzzle.z);
        if d.batter_cd <= 0.0 && pts.iter().any(|p| p.distance(base) < TOWER_BATTER_RANGE) {
            d.batter_cd = TOWER_BATTER_CD;
            d.hp = (d.hp - TOWER_BATTER_DMG).max(0.0);
        }
    }
}

/// Rebuild every tower to full on the dawn breather (the Wave→Prep edge).
fn revive_towers(siege: Res<Siege>, mut prev: Local<Option<GamePhase>>, mut towers: Query<&mut Defender>) {
    let was_wave = *prev == Some(GamePhase::Wave);
    if was_wave && siege.phase == GamePhase::Prep {
        for mut d in &mut towers {
            if d.kind == Kind::Tower {
                d.hp = d.max_hp;
                d.batter_cd = 0.0;
            }
        }
    }
    *prev = Some(siege.phase);
}

/// The healing shrine: mend the hero a few HP/s while he stands inside the walls (whole-HP
/// accumulator, so the HUD ticks on integers). Gated on the Bulwark shrine upgrade.
fn shrine_heal(
    time: Res<Time>,
    defenses: Res<Defenses>,
    hero: Res<HeroState>,
    mut player: ResMut<PlayerRes>,
    mut acc: Local<f64>,
) {
    if !defenses.shrine || !hero.alive || !crate::castle::in_footprint(hero.pos.x, hero.pos.y) {
        return;
    }
    let p = &mut player.0;
    if p.hp >= p.max_hp {
        return;
    }
    let (whole, new_acc) = heal_step(*acc, SHRINE_HEAL_PER_SEC, time.delta_secs() as f64);
    *acc = new_acc;
    if whole > 0 {
        p.heal(whole as f64);
    }
}

/// War bell: stand by the courtyard bell during prep and press **E** to ring in the night early
/// (the reducer floors the skip to `MIN_PREP_SECONDS`). The bare **B** keybind is kept as a
/// debug fallback in `siege::siege_controls`.
fn war_bell(
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<HeroState>,
    mut siege: ResMut<Siege>,
    mut cues: MessageWriter<AudioCue>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
) {
    if siege.phase != GamePhase::Prep {
        return;
    }
    if keys.just_pressed(KeyCode::KeyE) && hero.pos.distance(BELL_POS) < BELL_INTERACT_DIST {
        siege.request_prep_skip();
        cues.write(AudioCue::UiSelect);
        feedback.trauma = (feedback.trauma + 0.3).min(1.0);
    }
}

/// Spawn the firing emitters (called from `worldmap::build`): four corner towers, four
/// keep-roof archers, and a ballista just outside the north gate (the one structure with a
/// visible engine — the towers/keep reuse `castle.rs`'s meshes).
pub fn populate_defenders(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let tower_hp = TOWER_MAX_HP as f32;
    for (x, z) in [(-HALF_X, -HALF_Z), (HALF_X, -HALF_Z), (HALF_X, HALF_Z), (-HALF_X, HALF_Z)] {
        commands.spawn((
            Defender {
                kind: Kind::Tower,
                profile: TOWER_BASE,
                muzzle: Vec3::new(x, 4.0, z),
                ready_at: 0.0,
                hp: tower_hp,
                max_hp: tower_hp,
                batter_cd: 0.0,
            },
            crate::biome::BiomeEntity,
        ));
    }
    for (x, z) in [(-2.8, -2.4), (2.8, -2.4), (2.8, 2.4), (-2.8, 2.4)] {
        commands.spawn((
            Defender {
                kind: Kind::Archer,
                profile: KEEP_ARCHER,
                muzzle: Vec3::new(x, 2.5, z),
                ready_at: 0.0,
                hp: f32::INFINITY,
                max_hp: f32::INFINITY,
                batter_cd: 0.0,
            },
            crate::biome::BiomeEntity,
        ));
    }
    // Ballista just outside the north gate, with a small visible engine.
    let (bx, bz) = (0.0, -HALF_Z - 3.0);
    let y = crate::worldmap::ground_at_world(bx, bz).unwrap_or(0.0);
    let mesh = meshes.add(Cuboid::new(1.3, 0.5, 1.7).mesh().build());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.23, 0.14),
        perceptual_roughness: 0.85,
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(bx, y + 0.3, bz),
        Defender {
            kind: Kind::Ballista,
            profile: BALLISTA,
            muzzle: Vec3::new(bx, y + 1.0, bz),
            ready_at: 0.0,
            hp: f32::INFINITY,
            max_hp: f32::INFINITY,
            batter_cd: 0.0,
        },
        crate::biome::BiomeEntity,
    ));
}
