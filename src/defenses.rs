//! **Castle defenses** — the keep fights back at night. Four corner **watchtowers**, four
//! **keep archers**, and a **ballista** auto-fire cyan homing bolts at the wave invaders; a
//! **healing shrine** mends the hero inside the walls; the **war bell** (E during prep) rings
//! in the night early. Each structure is gated behind its Bulwark upgrade ([`Defenses`] flags),
//! so the castle only fights as hard as you've built it.
//!
//! Numbers come from the test-gated `tileworld_core::defense` fire profiles, scaled by
//! `DMG_SCALE` so defenses *support* rather than auto-clear. `DMG_SCALE` was re-derived (2026-06)
//! against full-HP orks: it was 0.236 from the old era when ork HP was rescaled ~0.35× down, which
//! left towers/ballista/archers firing but killing nothing once orks went to full core HP (grunt
//! 254, etc.) — the Defense branch's auto-fire upgrades read as "broken." Lifting it ~2.85× (the
//! HP ratio) restores the intended support feel without auto-clearing a wave.
//! Bolt geometry reuses `projectile::advance_bolt`; targets are the night [`WaveInvader`]s.
//! Emitters are invisible logic points co-located with `castle.rs`'s meshes.
//!
//! Deferred (balance long-tail): tower destructibility + ork-targets-tower + prep revive — the
//! firing/shrine/bell core lands here; towers currently fire for the whole wave.

use bevy::prelude::*;
use tileworld_core::defense::{
    heal_step, is_ready, nearest_in_range, FireProfile, BALLISTA, KEEP_ARCHER, SHRINE_HEAL_PER_SEC,
    TOWER_BASE, TOWER_MASTERY, TOWER_MAX_HP,
};

use crate::economy::Defenses;
use crate::palette::lin;
use crate::game_state::Modal;
use crate::orks::WaveInvader;
use crate::player::{spawn_burst, CombatFx, Health, HeroState, PlayerRes};
use crate::projectile::{advance_bolt, BoltStep};
use crate::siege::{GamePhase, Siege};

/// Scale defender damage from core's TS-anchored values so towers/archers *support* the defense
/// rather than auto-clearing the wave. Re-derived 2026-06 from the old 0.236 (tuned for ~0.35×-HP
/// orks) up to ~0.65 against full-HP orks — see module doc. At 0.65 a Tower-Mastery tower does
/// ~7.8 dmg/bolt (≈7.8 DPS), a ballista ~29/bolt, so a fully-built castle chips ~50 DPS into a
/// wave: real help, not an auto-clear.
const DMG_SCALE: f32 = 0.65;
const HALF_X: f32 = 17.0;
const HALF_Z: f32 = 12.0;
const DEFENDER_BOLT_TTL: f32 = 3.0;
/// World Y of the keep roof the archer figures stand on (the keep body top, ≈ `(KEEP_FOUND + KEEP_H)`
/// scaled by the keep's 0.7 y-scale). Tuned against a staged shot.
const ARCHER_FEET_Y: f32 = 1.54;
/// Root scale of the keep-roof archer bipeds — the same size the townsfolk render at
/// (`villagers::SCALE`), so the roof sentries read as the same people as the courtyard militia.
const ARCHER_SCALE: f32 = 0.81;

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

/// A REAL archer body standing watch on the keep roof — the same studio biped + longbow kit the
/// town's bowmen wear (`peasant_model::PeasantKind::Archer`), replacing the old merged-box
/// mannequin. A roof sentry never walks: its only motion is the shared idle clip
/// (`biped::animate_biped` off its `BipedDrive`), a yaw onto the current mark, and the
/// draw-and-loose. Target pick + cadence stay on the co-located [`Defender`]
/// (`defenders_fire`); this component holds the shot **choreography** — the arrow entity must
/// leave the string exactly on the draw clip's release frame, like the town archers' shots
/// (`keep_archer_shots`).
#[derive(Component)]
struct KeepArcher {
    /// Facing it returns to between marks — out over its battlement edge.
    home_yaw: f32,
    /// Current smoothed facing (the root yaw is ours; the animator owns only the joints).
    yaw: f32,
    /// `elapsed_secs` when the current draw began; `< 0` when at ease.
    draw_started: f32,
    /// The invader this draw is aimed at.
    target: Option<Entity>,
    /// This draw's arrow has left the string.
    loosed: bool,
}

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
                    keep_archer_shots.after(defenders_fire),
                    batter_towers,
                    revive_towers,
                    shrine_heal,
                    sync_keep_archers,
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
            // Visual-only in the harness: stage the armory corner + the mason's scaffold
            // (castle_decor) so a defended shot shows the full courtyard.
            reinforced: true,
            villager_arms_tier: 2,
            guard_hp_bonus: 0.0,
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
    mut emitters: Query<(&mut Defender, Option<&mut KeepArcher>)>,
    // `Without<Dying>`: don't lock a shot (and burn the cooldown) onto a fading corpse — a killed
    // invader stays a valid `WaveInvader` for ~1.4s. Matches `step_defender_bolts`'s own query.
    invaders: Query<(Entity, &Transform), (With<WaveInvader>, Without<crate::dying::Dying>)>,
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

    for (mut d, mut roof_archer) in &mut emitters {
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
            if let Some(ka) = roof_archer.as_deref_mut() {
                // A roof archer doesn't shoot instantly: it BEGINS A DRAW here, and
                // `keep_archer_shots` looses the real arrow on the clip's release frame.
                ka.draw_started = now;
                ka.target = Some(targets[i].0);
                ka.loosed = false;
            } else {
                orders.0.push(BoltOrder {
                    origin: d.muzzle,
                    target: targets[i].0,
                    damage: prof.damage as f32 * DMG_SCALE,
                    speed: prof.speed as f32,
                    max_range: prof.max_range as f32,
                });
            }
            d.ready_at = now + prof.cooldown as f32;
        }
    }
}

/// Drive each keep-roof archer's shot: yaw the body onto the mark (the sentry stands planted —
/// facing is the only whole-body motion), play the draw clip on its `BipedDrive`, and push the
/// real [`crate::projectile::ArrowSpawn`] exactly at the clip's release frame, aimed at the
/// target's live chest. Between draws the sentry eases back to its home battlement facing. The
/// keep-archer damage number stays the core parity profile × [`DMG_SCALE`], same as the old bolt.
fn keep_archer_shots(
    time: Res<Time>,
    defenses: Res<Defenses>,
    mut arrows: ResMut<crate::projectile::ArrowSpawns>,
    invaders: Query<&Transform, (With<WaveInvader>, Without<KeepArcher>, Without<crate::dying::Dying>)>,
    mut q: Query<(Entity, &Defender, &mut KeepArcher, &mut crate::biped::BipedDrive, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.05);
    const TURN: f32 = 4.0; // rad/s — a deliberate sentry pivot, quicker than a strolling villager
    for (self_e, d, mut ka, mut drive, mut tf) in &mut q {
        let p = (now - ka.draw_started) / crate::villagers::BOW_SHOT_SECS;
        let drawing = defenses.keep_archers && ka.draw_started >= 0.0 && (0.0..1.0).contains(&p);
        if !drawing {
            ka.draw_started = -1.0;
            ka.target = None;
            drive.bow = false;
            // At ease: settle back to watching out over the battlement edge.
            ka.yaw += crate::steer::wrap_pi(ka.home_yaw - ka.yaw).clamp(-TURN * dt, TURN * dt);
            tf.rotation = Quat::from_rotation_y(ka.yaw);
            continue;
        }
        drive.bow = true;
        drive.bow_t = p;
        let Some(te) = ka.target else { continue };
        if let Ok(ttf) = invaders.get(te) {
            let to = ttf.translation - tf.translation;
            let want = to.x.atan2(to.z);
            ka.yaw += crate::steer::wrap_pi(want - ka.yaw).clamp(-TURN * dt, TURN * dt);
            tf.rotation = Quat::from_rotation_y(ka.yaw);
            if p >= crate::player::anim::BOW_RELEASE_P && !ka.loosed {
                ka.loosed = true;
                let dir = Vec3::new(to.x, 0.0, to.z).normalize_or_zero();
                arrows.0.push(crate::projectile::ArrowSpawn {
                    from: tf.translation + Vec3::Y * 1.15 + dir * 0.4, // the bow, off the roof deck
                    aim: ttf.translation + Vec3::Y,
                    target: te,
                    shooter: self_e,
                    damage: d.profile.damage as f32 * DMG_SCALE,
                    rival: false,
                });
            }
        } else {
            // The mark died mid-draw: finish the motion gracefully, loose nothing.
            ka.target = None;
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
    invaders: Query<&Transform, (With<WaveInvader>, Without<DefenderBolt>, Without<crate::dying::Dying>)>,
    mut hp_q: Query<&mut Health>,
    mut q: Query<(Entity, &mut DefenderBolt, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut b, mut tf) in &mut q {
        b.ttl -= dt;
        let Ok(ttf) = invaders.get(b.target) else {
            commands.entity(e).try_despawn();
            continue;
        };
        if b.ttl <= 0.0 {
            commands.entity(e).try_despawn();
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
                            // Fade out (idempotent — combat / stuck-reaper may race on the same
                            // invader this frame; begin_dying tolerates it).
                            crate::dying::begin_dying(&mut commands, b.target, time.elapsed_secs());
                        }
                    }
                }
                if let Some(fx) = &fx {
                    spawn_burst(&mut commands, fx, tf.translation, false);
                }
                commands.entity(e).try_despawn();
            }
            BoltStep::Fizzle => commands.entity(e).try_despawn(),
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
    // `Without<Dying>`: a defeated invader keeps its position for the ~1.4s death fade; without this
    // it would keep gnawing on a tower after it's dead, per the corpse-filtering rule in `dying.rs`.
    invaders: Query<&Transform, (With<WaveInvader>, Without<crate::dying::Dying>)>,
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
    mut speak: MessageWriter<crate::audio::Speak>,
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
        // The hero's reverent line — catalog caps it to 5 min floor, so it's an occasional grace.
        speak.write(crate::audio::Speak::new(crate::audio::Concept::ShrineHeal));
    }
}

/// Reveal the keep-roof archers once the Keep Archers upgrade is owned (they stand guard day +
/// night, not only during a siege). The bodies are real bipeds now — the shared idle clip keeps
/// them alive, so no manual bob; this only gates visibility.
fn sync_keep_archers(defenses: Res<Defenses>, mut q: Query<&mut Visibility, With<KeepArcher>>) {
    let want = if defenses.keep_archers { Visibility::Inherited } else { Visibility::Hidden };
    for mut vis in &mut q {
        if *vis != want {
            *vis = want;
        }
    }
}

// (The war bell's **E** ring-in is now handled by the unified `interaction.rs` resolver, along
// with keep→upgrades and merchant→shop. The bare **B** debug skip stays in `siege::siege_controls`.)

/// Spawn the firing emitters (called from `worldmap::build`): four corner towers, four
/// keep-roof archers, and a ballista just outside the north gate (the one structure with a
/// visible engine — the towers/keep reuse `castle.rs`'s meshes).
pub fn populate_defenders(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
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
    // Keep archers: four REAL archer bipeds manning the keep-roof edges — the same longbow kit
    // the town's bowmen wear (`peasant_model::Archer`), not the old merged-box mannequin. Planted
    // sentries: `keep_archer_shots` yaws them onto marks and plays the draw; they never walk.
    // Hidden until the Keep Archers upgrade is owned (`sync_keep_archers`).
    let sentry_mat = crate::creature::make_creature_material(creature_mats);
    // Per-man variety pulled from the villager palette family (skin / tunic hexes).
    let sentry_looks: [(u32, u32); 4] =
        [(0xd8a06a, 0x4a6a3a), (0xc89070, 0x3a7a72), (0xe8c4a0, 0x6a4a6a), (0xa36b4a, 0x4a6a3a)];
    // One archer at the middle of each roof edge (front/back/sides) — on the open roof, clear of
    // the corner turrets, facing out over the battlements.
    for (i, (x, z)) in [(0.0_f32, 2.0_f32), (0.0, -2.0), (2.7, 0.0), (-2.7, 0.0)].into_iter().enumerate() {
        let home_yaw = x.atan2(z); // villager facing convention: from_rotation_y(yaw), forward +Z
        let root = commands
            .spawn((
                Transform {
                    translation: Vec3::new(x, ARCHER_FEET_Y, z),
                    rotation: Quat::from_rotation_y(home_yaw),
                    scale: Vec3::splat(ARCHER_SCALE),
                },
                Visibility::Hidden,
                Defender {
                    kind: Kind::Archer,
                    profile: KEEP_ARCHER,
                    muzzle: Vec3::new(x, 2.5, z),
                    ready_at: 0.0,
                    hp: f32::INFINITY,
                    max_hp: f32::INFINITY,
                    batter_cd: 0.0,
                },
                KeepArcher { home_yaw, yaw: home_yaw, draw_started: -1.0, target: None, loosed: false },
                // The shared biped animator idles/draws off this; `phase` desyncs the breathing.
                crate::biped::BipedDrive { phase: i as f32 * 1.7, ..default() },
                crate::biome::BiomeEntity,
            ))
            .id();
        let (skin, tunic) = sentry_looks[i];
        let h = crate::peasant_model::peasant_biped_meshes(
            crate::peasant_model::PeasantKind::Archer,
            skin,
            tunic,
            0x3a2a18,
            false,
            false,
        )
        .upload(meshes);
        // Off-hand bow mount — same transform the townsfolk use (`villagers::build_biped_body`).
        let shield_xf = Transform {
            translation: Vec3::new(0.0, 0.0, 0.14),
            rotation: Quat::from_euler(EulerRot::XYZ, 0.15, -0.45, 0.1),
            ..default()
        };
        crate::biped::spawn_biped(commands, root, &sentry_mat, h, 1.06, 1.0, 0.15, 0.3, -0.06, Some(shield_xf));
    }
    // Ballista flanking the north gate (offset off the gate axis so it doesn't sit on the hero
    // spawn / follow-cam line — the hero spawns at the gate centre, `gate.y - 3.0`).
    let (bx, bz) = (2.6, -HALF_Z - 2.2);
    let y = crate::worldmap::ground_at_world(bx, bz).unwrap_or(0.0);
    // A real low-poly ballista (vertex-coloured): wheeled sled, A-frame, stock, bow limbs +
    // string and a loaded bolt — aimed outward (−Z) away from the gate. One white material so the
    // mesh's vertex colours show. Yaw it slightly so it points out along its placement radius.
    let mesh = meshes.add(ballista_mesh());
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.82,
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(bx, y, bz).with_rotation(Quat::from_rotation_y(0.18)),
        Defender {
            kind: Kind::Ballista,
            profile: BALLISTA,
            muzzle: Vec3::new(bx, y + 0.72, bz),
            ready_at: 0.0,
            hp: f32::INFINITY,
            max_hp: f32::INFINITY,
            batter_cd: 0.0,
        },
        crate::biome::BiomeEntity,
    ));
    // Solid sled — hero/orks route around the war engine. Local z is the long axis (runners +
    // bow), matching the 0.18 yaw on the mesh.
    crate::blockers::add_obb(bx, bz, 0.6, 0.85, 0.18);
}

// ── Ballista model (vertex-coloured, flat-shaded) ──────────────────────────────────
// Built in local space, base resting at y=0, firing toward −Z (forward). Helpers are prefixed
// (`bbox`…) so they don't clash with the `bx`/`bz` placement locals above.

fn ballista_mesh() -> Mesh {
    let wood = lin(0x6a4527);
    let wood_dk = lin(0x402a17);
    let iron = lin(0x33343c);
    let string = lin(0xd8c7a2);
    let bolt = lin(0x7a5a30);
    let along_x = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    bgroup(vec![
        // wheeled sled: two runners along Z + two cross beams
        bbox(0.13, 0.16, 1.5, vc(-0.42, 0.20, 0.05), wood_dk),
        bbox(0.13, 0.16, 1.5, vc(0.42, 0.20, 0.05), wood_dk),
        bbox(1.02, 0.12, 0.16, vc(0.0, 0.22, 0.55), wood),
        bbox(1.02, 0.12, 0.16, vc(0.0, 0.22, -0.35), wood),
        // wheels (axle along X) at the back corners
        bcyl(0.20, 0.10, vc(-0.5, 0.20, 0.5), along_x, wood_dk),
        bcyl(0.20, 0.10, vc(0.5, 0.20, 0.5), along_x, wood_dk),
        bcyl(0.06, 0.12, vc(-0.5, 0.20, 0.5), along_x, iron), // hub
        bcyl(0.06, 0.12, vc(0.5, 0.20, 0.5), along_x, iron),
        // A-frame supports lifting the stock
        bboxr(0.12, 0.66, 0.14, vc(-0.2, 0.52, 0.18), Quat::from_rotation_x(0.22), wood),
        bboxr(0.12, 0.66, 0.14, vc(0.2, 0.52, 0.18), Quat::from_rotation_x(0.22), wood),
        // the stock / rail (slightly nose-down toward the front)
        bboxr(0.18, 0.13, 1.4, vc(0.0, 0.74, -0.05), Quat::from_rotation_x(-0.05), wood),
        // bow riser + the two angled limbs + iron tip caps
        bbox(0.18, 0.16, 0.18, vc(0.0, 0.72, -0.64), wood_dk),
        bboxr(0.62, 0.09, 0.11, vc(-0.3, 0.72, -0.68), Quat::from_rotation_y(0.55), wood_dk),
        bboxr(0.62, 0.09, 0.11, vc(0.3, 0.72, -0.68), Quat::from_rotation_y(-0.55), wood_dk),
        bbox(0.1, 0.13, 0.1, vc(-0.62, 0.72, -0.52), iron),
        bbox(0.1, 0.13, 0.1, vc(0.62, 0.72, -0.52), iron),
        // bowstring across the limb tips
        bbox(1.3, 0.03, 0.03, vc(0.0, 0.72, -0.5), string),
        // loaded bolt on the stock: shaft + iron head + fletching
        bbox(0.05, 0.05, 0.9, vc(0.0, 0.84, -0.3), bolt),
        bbox(0.09, 0.09, 0.16, vc(0.0, 0.84, -0.8), iron),
        bbox(0.13, 0.11, 0.04, vc(0.0, 0.84, 0.12), wood_dk),
    ])
}

// (The old merged-box `archer_mesh()` keep-roof mannequin is gone — the roof sentries are real
// `peasant_model::Archer` bipeds now, spawned in `populate_defenders`.)

fn vc(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn btint(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn bbox(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    btint(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn bboxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    btint(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), c)
}
fn bcyl(r: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    btint(Cylinder::new(r, h).mesh().resolution(10).build().rotated_by(rot).translated_by(off), c)
}
fn bgroup(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("ballista parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
