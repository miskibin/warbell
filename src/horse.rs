//! The rideable horse: **L** calls one — a short fade to black and the knight is in the saddle — and
//! **L** again climbs down. WASD steers it (camera-relative, like the knight): a canter-paced gallop,
//! **Shift** to run it flat out, **Ctrl** to walk. The moment a fight starts (a hostile closes in, a
//! blow is dealt or taken) the knight jumps down and the horse rears and bolts.
//!
//! Content: `assets/animals/horse.glb` — Henry S's CC-BY "Horse" (pinto with saddle, bridle and shoes,
//! source clips; `tools/animals/export_horse.py`), `assets/audio/horse/` — CC0 Freesound hooves, snort
//! and whinnies (`tools/animals/horse_sfx.py`); credits in `assets/CREDITS.md`.
//!
//! The rider: [`Hero::mounted`] stands the knight's locomotion and combat input down and makes
//! `player::anim` hold the riding seat (`mixamo::ride`, posed in Blender on this very horse with the
//! boots in its stirrups). Each frame [`ride`] puts the hips on the saddle — the saddle's spine bone
//! carries the gait's bounce — through a soft vertical spring, and pitches the trunk with speed and
//! with the lag of the seat through starts and stops (`Hero::ride_lean`).
//!
//! Nothing here is run-state: a load, a new run or the knight going down puts him on foot and the
//! horse away (`reset_on_load`, `reset_on_new_run`). `FOREST_RIDE=<u/s>[,<turn rad/s>]` mounts at boot
//! and drives the horse with no keypress (captures);
//! a third number sets the heading it sets off on (radians, 0 = +Z).

use std::collections::HashMap;
use std::time::Duration;

use bevy::audio::{PlaybackMode, Volume};
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::gltf::Gltf;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};

use crate::npc_skin::WORLD_PER_M;
use crate::player::{Hero, PlayMode, PlayerRes};
use crate::{blockers, steer};

/// Native ground speed of the source clips (m/s, measured from the planted hooves on export).
const WALK_NATIVE: f32 = 1.27 * WORLD_PER_M;
const GALLOP_NATIVE: f32 = 5.92 * WORLD_PER_M;
/// Ridden speeds (world u/s): Ctrl walk, the default canter-paced gallop, Shift flat out (the clip
/// sped up past its native pace — a little hoof slide is the price of covering the island).
const WALK_SPEED: f32 = WALK_NATIVE;
const CRUISE_SPEED: f32 = GALLOP_NATIVE;
const RUN_SPEED: f32 = GALLOP_NATIVE * 1.45;
const FLEE_SPEED: f32 = GALLOP_NATIVE * 1.3;
/// Turn rates (rad/s) at a walk and at a gallop.
const TURN_WALK: f32 = 2.2;
const TURN_GALLOP: f32 = 1.25;
/// Body radius for the blocker / footing tests, and half the hoof-to-hoof length (pitch sampling).
const BODY_R: f32 = 0.45;
const HALF_LEN: f32 = 0.85;
/// Where the rider's hips sit, in the horse root's frame at its first idle frame (the Blender seat:
/// saddle seat 1.515 m up and 4.8 cm behind the girth line, hips 9 cm above it).
const HIPS_LOCAL: Vec3 = Vec3::new(0.0, 1.605 * WORLD_PER_M, -0.048 * WORLD_PER_M);
/// The skeleton bone the saddle rides on (it carries the gait's bounce and pitch).
const SADDLE_BONE: &str = "BN_Spine_02_04";
/// Screen fade: to black, swap, back.
const FADE_S: f32 = 0.35;
/// Past this from the knight a parked or bolted horse is put away.
const PARK_RANGE: f32 = 60.0;
const FLEE_RANGE: f32 = 45.0;
const CLIP_FADE: Duration = Duration::from_millis(250);

pub struct HorsePlugin;

impl Plugin for HorsePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mount>()
            .add_systems(Startup, (load, spawn_fade))
            .add_systems(Update, (finish_model, drive_fade).run_if(crate::rts::in_campaign))
            // After everything in Update (locomotion, soft-lock facing, the animator) and before the
            // transforms propagate: the saddle seats the knight this frame, wherever the horse went.
            .add_systems(
                PostUpdate,
                (mount_input, ride, horse_anim, hooves, put_away)
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate)
                    .run_if(in_state(crate::game_state::Modal::None))
                    .run_if(crate::rts::in_campaign),
            )
            .add_systems(Update, reset_on_load.run_if(crate::rts::in_campaign))
            .add_systems(OnExit(crate::game_state::AppState::StartScreen), reset_on_new_run)
            .add_systems(OnExit(crate::game_state::AppState::GameOver), reset_on_new_run);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Phase {
    #[default]
    Afoot,
    /// Fading to black; at the bottom the knight is put in (or out of) the saddle.
    FadeOut { mount: bool },
    /// Fading back in.
    FadeIn,
    Riding,
}

#[derive(Resource, Default)]
struct Mount {
    phase: Phase,
    /// Seconds into the current fade.
    t: f32,
    /// The horse being ridden / just left / about to be mounted.
    horse: Option<Entity>,
    /// The rider's vertical spring (offset of the hips from the saddle, and its rate) and the trunk
    /// lean spring.
    bob: f32,
    bob_v: f32,
    lean: f32,
    lean_v: f32,
    last_speed: f32,
    /// The saddle's height and vertical speed last frame (the seat spring is driven by its acceleration).
    last_target: Option<(f32, f32)>,
    /// `FOREST_RIDE` auto-drive: (speed u/s, turn rad/s, starting heading).
    auto: Option<(f32, f32, Option<f32>)>,
}

struct Model {
    scene: Handle<WorldAsset>,
    graph: Handle<AnimationGraph>,
    nodes: HashMap<String, AnimationNodeIndex>,
}

#[derive(Resource)]
struct HorseAssets {
    gltf: Handle<Gltf>,
    model: Option<Model>,
    hoof: Vec<Handle<AudioSource>>,
    snort: Handle<AudioSource>,
    whinny: [Handle<AudioSource>; 2],
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mood {
    /// Under the knight.
    Ridden,
    /// Left standing where he climbed down.
    Parked,
    /// Rearing, then bolting along `dir` (s since the fight started).
    Bolting { dir: Vec2, t: f32 },
}

/// The horse's gameplay root (feet at the ground, facing +Z rotated by `yaw`).
#[derive(Component)]
struct Horse {
    pos: Vec2,
    y: f32,
    yaw: f32,
    pitch: f32,
    speed: f32,
    turn: f32,
    mood: Mood,
    /// Set once the model instance is ready.
    player: Option<Entity>,
    saddle: Option<Entity>,
    hooves: Vec<(Entity, bool)>,
    /// The saddle bone in the root frame at the first idle frame (what [`HIPS_LOCAL`] is measured at).
    saddle_ref: Option<Affine3A>,
    clip: &'static str,
    /// Frames since the instance became ready (shown after a few, like `animal_skin::Reveal`).
    shown: u8,
    inst: Entity,
    next_snort: f32,
}

#[derive(Component)]
struct HorseInst(Entity);

#[derive(Component)]
struct MountFade;

fn load(mut commands: Commands, server: Res<AssetServer>, mut mount: ResMut<Mount>) {
    commands.insert_resource(HorseAssets {
        gltf: server.load("animals/horse.glb"),
        model: None,
        hoof: (1..=6).map(|i| server.load(format!("audio/horse/hoof-{i}.ogg"))).collect(),
        snort: server.load("audio/horse/snort.ogg"),
        whinny: [server.load("audio/horse/whinny-1.ogg"), server.load("audio/horse/whinny-2.ogg")],
    });
    mount.auto = std::env::var("FOREST_RIDE").ok().map(|v| {
        let mut it = v.split(',').map(|x| x.trim().parse::<f32>().unwrap_or(0.0));
        let (v, w) = (it.next().unwrap_or(CRUISE_SPEED), it.next().unwrap_or(0.0));
        (v, w, it.next())
    });
}

fn finish_model(
    mut assets: ResMut<HorseAssets>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if assets.model.is_some() || !server.is_loaded_with_dependencies(assets.gltf.id()) {
        return;
    }
    let Some(g) = gltfs.get(&assets.gltf) else { return };
    let Some(scene) = g.scenes.first().cloned() else { return };
    let mut names: Vec<_> = g.named_animations.iter().map(|(n, h)| (n.to_string(), h.clone())).collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    let (graph, idx) = AnimationGraph::from_clips(names.iter().map(|(_, h)| h.clone()));
    let nodes = names.into_iter().map(|(n, _)| n).zip(idx).collect();
    assets.model = Some(Model { scene, graph: graphs.add(graph), nodes });
}

fn spawn_fade(mut commands: Commands) {
    commands.spawn((
        MountFade,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::BLACK.with_alpha(0.0)),
        GlobalZIndex(40),
        bevy::ui::FocusPolicy::Pass,
    ));
}

/// The black of the mount / dismount fade.
fn drive_fade(mount: Res<Mount>, mut q: Query<(&mut Node, &mut BackgroundColor), With<MountFade>>) {
    let a = match mount.phase {
        Phase::FadeOut { .. } => (mount.t / FADE_S).clamp(0.0, 1.0),
        Phase::FadeIn => 1.0 - (mount.t / FADE_S).clamp(0.0, 1.0),
        _ => 0.0,
    };
    for (mut node, mut bg) in &mut q {
        let d = if a > 0.001 { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
        bg.0 = Color::BLACK.with_alpha(a * a * (3.0 - 2.0 * a));
    }
}

fn spawn_horse(commands: &mut Commands, assets: &HorseAssets, pos: Vec2, y: f32, yaw: f32, now: f32, mood: Mood) -> Option<Entity> {
    let m = assets.model.as_ref()?;
    let root = commands
        .spawn((
            Transform::from_xyz(pos.x, y, pos.y).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            crate::biome::BiomeEntity,
            Name::new("Horse"),
        ))
        .id();
    let inst = commands
        .spawn((WorldAssetRoot(m.scene.clone()), Transform::from_scale(Vec3::splat(WORLD_PER_M)), Visibility::Hidden, HorseInst(root)))
        .observe(horse_ready)
        .id();
    commands.entity(root).add_child(inst);
    commands.entity(root).insert(Horse {
        pos,
        y,
        yaw,
        pitch: 0.0,
        speed: 0.0,
        turn: 0.0,
        mood,
        player: None,
        saddle: None,
        hooves: Vec::new(),
        saddle_ref: None,
        clip: "",
        shown: 0,
        inst,
        next_snort: now + 4.0,
    });
    Some(root)
}

fn horse_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    assets: Res<HorseAssets>,
    of: Query<&HorseInst>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    meshes: Query<(), With<Mesh3d>>,
    names: Query<&Name>,
    mut horses: Query<&mut Horse>,
) {
    let inst = ready.entity;
    let Ok(HorseInst(root)) = of.get(inst) else { return };
    let Ok(mut h) = horses.get_mut(*root) else { return };
    let Some(m) = assets.model.as_ref() else { return };
    for d in children.iter_descendants(inst) {
        if players.contains(d) {
            commands.entity(d).insert((AnimationGraphHandle(m.graph.clone()), AnimationTransitions::new()));
            h.player = Some(d);
        }
        if meshes.contains(d) {
            commands.entity(d).insert(DynamicSkinnedMeshBounds);
        }
        if let Ok(n) = names.get(d) {
            let n = n.as_str();
            if n == SADDLE_BONE {
                h.saddle = Some(d);
            } else if n.contains("Toe") && n.ends_with("_0_end") {
                h.hooves.push((d, false));
            }
        }
    }
    h.clip = "";
}

/// Can a horse stand here (land, no wall, no cliff under the body)?
fn horse_fits(p: Vec2, y: f32) -> bool {
    steer::can_stand(p.x, p.y, BODY_R, y) && !blockers::any_within(p.x, p.y, BODY_R)
}

/// Ground under the horse at `p` facing `yaw`: (height, nose-up pitch).
fn ground_under(p: Vec2, yaw: f32, fallback: f32) -> (f32, f32) {
    let f = Vec2::new(yaw.sin(), yaw.cos()) * HALF_LEN;
    let yf = steer::footing(p.x + f.x, p.y + f.y).unwrap_or(fallback);
    let yb = steer::footing(p.x - f.x, p.y - f.y).unwrap_or(fallback);
    let yc = steer::footing(p.x, p.y).unwrap_or(fallback);
    (yc.max((yf + yb) * 0.5), ((yf - yb) / (2.0 * HALF_LEN)).atan())
}

/// Is the knight in a fight: a hostile within the soft-lock's threat range, or blows traded lately?
fn in_fight(hero: &Hero, now: f32) -> bool {
    hero.threats > 0 || now < hero.combat_until
}

#[allow(clippy::too_many_arguments)]
fn mount_input(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<PlayMode>,
    player: Res<PlayerRes>,
    build_mode: Res<crate::town::BuildMode>,
    threat: Option<Res<crate::audio::HeroThreat>>,
    siege: Option<Res<crate::siege::Siege>>,
    assets: Res<HorseAssets>,
    audio_cfg: Res<crate::audio::AudioConfig>,
    scripted: Option<Res<crate::player::ScriptedHero>>,
    clip: Option<Res<crate::capture::ClipProgress>>,
    mut mount: ResMut<Mount>,
    mut hero_q: Query<(&mut Hero, &mut Transform), Without<Horse>>,
    mut horses: Query<(Entity, &mut Horse, &mut Transform)>,
) {
    let Ok((mut hero, mut htf)) = hero_q.single_mut() else { return };
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let alive = player.0.is_alive();
    let fight = in_fight(&hero, now);
    // no calling a horse with orks about (or through a siege night)
    let danger = threat.as_ref().is_some_and(|t| t.in_danger) || siege.as_ref().is_some_and(|s| s.phase == crate::siege::GamePhase::Wave);
    let vol = audio_cfg.sfx_vol;

    // A fight (or the knight going down) throws him out of the saddle at once — no fade, he needs
    // his hands — and the horse rears and bolts away from the trouble.
    if hero.mounted && (fight || !alive || *mode != PlayMode::Play) && matches!(mount.phase, Phase::Riding) {
        if let Some((_, mut h, _)) = mount.horse.and_then(|e| horses.get_mut(e).ok()) {
            let away = hero.soft_pos.map(|f| (h.pos - f).normalize_or_zero()).filter(|d| *d != Vec2::ZERO);
            // no foe ringed yet: the trouble is ahead of a ridden horse, so it wheels about
            let dir = away.unwrap_or(-Vec2::new(h.yaw.sin(), h.yaw.cos()));
            h.mood = Mood::Bolting { dir, t: 0.0 };
            spatial(&mut commands, assets.whinny[0].clone(), 0.9 * vol, 1.0, Vec3::new(h.pos.x, h.y + 1.4, h.pos.y));
            step_off(&mut hero, &mut htf, &h);
        } else {
            hero.mounted = false;
        }
        mount.phase = Phase::Afoot;
        return;
    }

    match mount.phase {
        Phase::FadeOut { mount: up } => {
            mount.t += dt;
            if mount.t < FADE_S {
                return;
            }
            if up {
                // Fully black: a horse under the knight (the one he left here, or a fresh one).
                let h_e = match mount.horse.filter(|e| horses.get(*e).is_ok()) {
                    Some(e) => Some(e),
                    None => {
                        let (y, _) = ground_under(hero.pos, hero.facing, hero.y);
                        spawn_horse(&mut commands, &assets, hero.pos, y, hero.facing, now, Mood::Ridden)
                    }
                };
                if let Some(e) = h_e {
                    if let Ok((_, mut h, _)) = horses.get_mut(e) {
                        h.mood = Mood::Ridden;
                        h.speed = 0.0;
                    }
                    mount.horse = Some(e);
                    hero.mounted = true;
                    hero.ride_lean = 0.0;
                    mount.bob = 0.0;
                    mount.bob_v = 0.0;
                    mount.lean = 0.0;
                    mount.lean_v = 0.0;
                    mount.last_target = None;
                    spatial(&mut commands, assets.whinny[1].clone(), 0.55 * vol, 1.0, Vec3::new(hero.pos.x, hero.y + 1.4, hero.pos.y));
                }
            } else if let Some((_, mut h, _)) = mount.horse.and_then(|e| horses.get_mut(e).ok()) {
                h.mood = Mood::Parked;
                h.speed = 0.0;
                step_off(&mut hero, &mut htf, &h);
            } else {
                hero.mounted = false;
            }
            mount.phase = Phase::FadeIn;
            mount.t = 0.0;
        }
        Phase::FadeIn => {
            mount.t += dt;
            if mount.t >= FADE_S {
                mount.phase = if hero.mounted { Phase::Riding } else { Phase::Afoot };
            }
        }
        Phase::Afoot | Phase::Riding => {
            // under a clip the ride starts with the recording (the warm-up runs on real time)
            let rolling = clip.as_ref().map_or(now > 3.0, |c| c.recording);
            let auto_mount = mount.auto.is_some() && !hero.mounted && mount.horse.is_none() && assets.model.is_some() && rolling;
            if !(keys.just_pressed(KeyCode::KeyL) || auto_mount) {
                return;
            }
            if *mode != PlayMode::Play || !alive || build_mode.active || scripted.is_some() {
                return;
            }
            if let (true, Some((_, _, Some(yaw)))) = (auto_mount, mount.auto) {
                hero.facing = yaw;
            }
            if hero.mounted {
                mount.phase = Phase::FadeOut { mount: false };
                mount.t = 0.0;
                return;
            }
            // Calling a horse: not in a fight, not mid-roll / mid-air, and only where one can stand.
            let busy = hero.roll_t >= 0.0 || hero.dash_t >= 0.0 || !hero.on_ground || hero.attacking;
            if fight || danger || busy || assets.model.is_none() || !steer::can_stand(hero.pos.x, hero.pos.y, BODY_R, hero.y) {
                if std::env::var_os("FOREST_HORSELOG").is_some() {
                    info!("HORSE refused: fight {fight} danger {danger} busy {busy} model {} at {:?}", assets.model.is_some(), hero.pos);
                }
                return;
            }
            // A horse left standing close by is the one he climbs back on; any other is sent off.
            if let Some(e) = mount.horse {
                let near = horses.get(e).is_ok_and(|(_, h, _)| h.pos.distance(hero.pos) < 3.5 && h.mood == Mood::Parked);
                if !near {
                    if let Ok((e, _, _)) = horses.get(e) {
                        commands.entity(e).try_despawn();
                    }
                    mount.horse = None;
                }
            }
            mount.phase = Phase::FadeOut { mount: true };
            mount.t = 0.0;
        }
    }
}

/// Put the knight on the ground beside the horse (its left, else its right, else where he sat).
fn step_off(hero: &mut Hero, tf: &mut Transform, h: &Horse) {
    let left = Vec2::new(h.yaw.cos(), -h.yaw.sin());
    let spot = [h.pos + left * 0.85, h.pos - left * 0.85, h.pos]
        .into_iter()
        .find(|p| steer::can_stand(p.x, p.y, 0.25, h.y) && !blockers::any_within(p.x, p.y, 0.25))
        .unwrap_or(h.pos);
    hero.mounted = false;
    hero.ride_lean = 0.0;
    hero.pos = spot;
    hero.y = steer::footing(spot.x, spot.y).unwrap_or(h.y);
    hero.facing = h.yaw;
    hero.vel = Vec2::ZERO;
    hero.vel_y = 0.0;
    hero.on_ground = true;
    tf.translation = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
    tf.rotation = Quat::from_rotation_y(hero.facing);
}

/// Steer the ridden horse (and run the parked / bolting ones), then seat the knight on the saddle.
#[allow(clippy::too_many_arguments)]
fn ride(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mount: ResMut<Mount>,
    cam_q: Query<&Transform, (With<Camera3d>, Without<Hero>, Without<Horse>)>,
    mut hero_q: Query<(&mut Hero, &mut Transform), Without<Horse>>,
    mut horses: Query<(Entity, &mut Horse, &mut Transform, &GlobalTransform), Without<Hero>>,
    bones: Query<&GlobalTransform, (Without<Horse>, Without<Hero>)>,
) {
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    let Ok((mut hero, mut htf)) = hero_q.single_mut() else { return };
    let ridden = mount.horse.filter(|_| hero.mounted);
    for (e, mut h, mut tf, gt) in &mut horses {
        // ── what the horse wants: speed + heading ──
        let (want_speed, want_dir): (f32, Option<Vec2>) = match h.mood {
            Mood::Ridden if Some(e) == ridden => {
                if let Some((v, w, _)) = mount.auto {
                    let yaw = h.yaw + w * dt;
                    (v, Some(Vec2::new(yaw.sin(), yaw.cos())))
                } else {
                    let dir = crate::player::move_input(&keys, cam_q.iter().next());
                    let speed = if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
                        WALK_SPEED
                    } else if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                        RUN_SPEED
                    } else {
                        CRUISE_SPEED
                    };
                    (if dir.is_some() { speed } else { 0.0 }, dir)
                }
            }
            Mood::Bolting { dir, ref mut t } => {
                *t += dt;
                // a rear first, then away
                if *t < 1.1 { (0.0, None) } else { (FLEE_SPEED, Some(dir)) }
            }
            _ => (0.0, None),
        };
        // ── turn toward the wanted heading at the gait's rate; a hard about-turn slows to a walk ──
        let mut want_speed = want_speed;
        let mut turn = 0.0;
        if let Some(d) = want_dir {
            let target = d.x.atan2(d.y);
            let diff = steer::wrap_pi(target - h.yaw);
            let rate = TURN_WALK + (TURN_GALLOP - TURN_WALK) * (h.speed / CRUISE_SPEED).clamp(0.0, 1.0);
            turn = diff.clamp(-rate * dt, rate * dt);
            if diff.abs() > 1.9 {
                want_speed = want_speed.min(WALK_SPEED);
            }
        }
        h.yaw = steer::wrap_pi(h.yaw + turn);
        h.turn += (turn / dt - h.turn) * (dt * 6.0).min(1.0);
        let acc = if want_speed > h.speed { 2.6 } else { 4.5 };
        h.speed += (want_speed - h.speed).clamp(-acc * dt, acc * dt);
        // ── step, sliding along whatever stops it ──
        if h.speed > 0.01 {
            let fwd = Vec2::new(h.yaw.sin(), h.yaw.cos());
            let step = fwd * h.speed * dt;
            let tries = [step, Vec2::new(step.x, 0.0), Vec2::new(0.0, step.y)];
            if std::env::var_os("FOREST_HORSELOG").is_some() {
                let p = h.pos + step + step.normalize_or_zero() * 0.35;
                info!(
                    "HORSE pos {:?} y {:.2} yaw {:.2} speed {:.2} stand {} blocked {} footing {:?}",
                    h.pos, h.y, h.yaw, h.speed, steer::can_stand(p.x, p.y, BODY_R, h.y), blockers::any_within(p.x, p.y, BODY_R), steer::footing(p.x, p.y)
                );
            }
            match tries.into_iter().find(|s| s.length() > 0.2 * step.length() && horse_fits(h.pos + *s + s.normalize_or_zero() * 0.35, h.y)) {
                Some(s) => h.pos += s,
                None => {
                    h.speed = 0.0;
                    // a bolting horse that hits a wall picks another way out
                    if let Mood::Bolting { ref mut dir, .. } = h.mood {
                        *dir = Vec2::new(-dir.y, dir.x);
                    }
                }
            }
        }
        let (gy, pitch) = ground_under(h.pos, h.yaw, h.y);
        h.y += (gy - h.y) * (dt * 12.0).min(1.0);
        h.pitch += (pitch - h.pitch) * (dt * 6.0).min(1.0);
        tf.translation = Vec3::new(h.pos.x, h.y, h.pos.y);
        tf.rotation = Quat::from_rotation_y(h.yaw) * Quat::from_rotation_x(-h.pitch);

        // ── seat the rider ──
        if Some(e) != ridden {
            continue;
        }
        // The saddle bone's motion off its first idle frame, carried onto the measured hips point.
        // Its global is last frame's (propagation runs after this), so it is taken relative to the
        // root's last-frame global and re-applied on the root's transform of THIS frame.
        let root_now = Affine3A::from_scale_rotation_translation(Vec3::ONE, tf.rotation, tf.translation);
        let rel = h.saddle.and_then(|s| bones.get(s).ok()).map(|b| gt.affine().inverse() * b.affine());
        if let (Some(rel), None, true) = (rel, h.saddle_ref, h.shown > 3) {
            h.saddle_ref = Some(rel);
        }
        let bounce = match (rel, h.saddle_ref) {
            (Some(r), Some(r0)) => r * r0.inverse(),
            _ => Affine3A::IDENTITY,
        };
        let hips = root_now.transform_point3(bounce.transform_point3(HIPS_LOCAL));
        // The seat: the hips follow the saddle's bounce through a soft spring (a rider absorbs
        // it), never sinking into the leather.
        let hip_h = crate::player::model::HIP_REST_Y * crate::player::anim::hero_model_scale();
        // The rider's offset off the saddle is a damped spring the saddle's vertical acceleration
        // drives (partly: the seat grips him) — he lags its bounce and settles through a stop.
        let (last_y, last_v) = mount.last_target.unwrap_or((hips.y, 0.0));
        let sv = (hips.y - last_y) / dt;
        let sa = (sv - last_v) / dt;
        mount.last_target = Some((hips.y, sv));
        let (k, zeta) = (260.0_f32, 0.55_f32);
        let a = -k * mount.bob - 2.0 * zeta * k.sqrt() * mount.bob_v - sa.clamp(-40.0, 40.0) * 0.35;
        mount.bob_v += a * dt;
        mount.bob += mount.bob_v * dt;
        mount.bob = mount.bob.clamp(-0.025, 0.08);
        // Trunk: forward with speed, back as the horse surges, forward as it brakes; with the slope.
        let accel = (h.speed - mount.last_speed) / dt;
        mount.last_speed = h.speed;
        let lean_t = -0.05 + 0.2 * (h.speed / CRUISE_SPEED).min(1.4) + 0.35 * h.pitch;
        let kl = 70.0_f32;
        let la = (lean_t - mount.lean) * kl - mount.lean_v * 2.0 * 0.5 * kl.sqrt() - accel.clamp(-8.0, 8.0) * 0.9;
        mount.lean_v += la * dt;
        mount.lean += mount.lean_v * dt;
        mount.lean = mount.lean.clamp(-0.35, 0.6);
        hero.ride_lean = mount.lean;
        hero.pos = Vec2::new(hips.x, hips.z);
        hero.y = hips.y - hip_h + mount.bob;
        hero.facing = h.yaw;
        hero.on_ground = true;
        htf.translation = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
        htf.rotation = Quat::from_rotation_y(hero.facing);
    }
}

/// Pick + cross-fade the horse's clip from its speed and turn, paced to its ground speed.
fn horse_anim(
    assets: Res<HorseAssets>,
    mut horses: Query<&mut Horse>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    mut vis: Query<&mut Visibility>,
) {
    let Some(m) = assets.model.as_ref() else { return };
    for mut h in &mut horses {
        let Some(pe) = h.player else { continue };
        let Ok((mut player, mut tr)) = players.get_mut(pe) else { continue };
        if h.shown < 8 {
            h.shown += 1;
            if h.shown == 8 {
                if let Ok(mut v) = vis.get_mut(h.inst) {
                    *v = Visibility::Inherited;
                }
            }
        }
        let turning = h.turn.abs() > 0.45;
        let rearing = matches!(h.mood, Mood::Bolting { t, .. } if t < 1.1);
        let (clip, once, rate): (&'static str, bool, f32) = if rearing {
            ("Rear", true, 1.6)
        } else if h.speed > 2.2 {
            let c = if turning { if h.turn > 0.0 { "GallopL" } else { "GallopR" } } else { "Gallop" };
            (c, false, (h.speed / GALLOP_NATIVE).clamp(0.6, 1.5))
        } else if h.speed > 0.12 {
            let c = if turning { if h.turn > 0.0 { "WalkL" } else { "WalkR" } } else { "Walk" };
            (c, false, (h.speed / WALK_NATIVE).clamp(0.5, 1.9))
        } else {
            ("Idle", false, 1.0)
        };
        if clip != h.clip {
            let Some(&node) = m.nodes.get(clip) else { continue };
            let first = h.clip.is_empty();
            let anim = tr.play(&mut player, node, if first { Duration::ZERO } else { CLIP_FADE });
            if !once {
                anim.repeat();
            }
            h.clip = clip;
        }
        for (_, an) in player.playing_animations_mut() {
            an.set_speed(rate);
        }
    }
}

/// A clop for every hoof that comes down; a snort now and then while it stands or walks.
fn hooves(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<HorseAssets>,
    audio_cfg: Res<crate::audio::AudioConfig>,
    mut horses: Query<&mut Horse>,
    bones: Query<&GlobalTransform>,
    mut pick: Local<u32>,
) {
    let now = time.elapsed_secs();
    let vol = audio_cfg.sfx_vol;
    for mut h in &mut horses {
        let ground = h.y;
        let loud = 0.35 + 0.45 * (h.speed / CRUISE_SPEED).min(1.0);
        let mut plants = Vec::new();
        for (b, down) in h.hooves.iter_mut() {
            let Ok(g) = bones.get(*b) else { continue };
            let p = g.translation();
            let rel = p.y - ground;
            if !*down && rel < 0.07 {
                *down = true;
                plants.push(p);
            } else if *down && rel > 0.13 {
                *down = false;
            }
        }
        if h.speed > 0.1 {
            for p in plants {
                *pick = pick.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let i = (*pick >> 16) as usize % assets.hoof.len();
                let pitch = 0.9 + ((*pick >> 8) & 255) as f32 / 255.0 * 0.2;
                spatial(&mut commands, assets.hoof[i].clone(), loud * vol, pitch, p);
            }
        }
        if h.speed < 2.0 && now > h.next_snort && h.shown >= 8 {
            spatial(&mut commands, assets.snort.clone(), 0.45 * vol, 1.0, Vec3::new(h.pos.x, h.y + 1.3, h.pos.y));
            h.next_snort = now + 9.0 + (now * 7.13).fract() * 12.0;
        }
    }
}

fn spatial(commands: &mut Commands, clip: Handle<AudioSource>, vol: f32, speed: f32, pos: Vec3) {
    commands.spawn((
        AudioPlayer(clip),
        PlaybackSettings { mode: PlaybackMode::Despawn, volume: Volume::Linear(vol), speed, spatial: true, ..default() },
        Transform::from_translation(pos),
    ));
}

/// A bolted horse gone far enough, or one left standing far behind, is put away.
fn put_away(mut commands: Commands, mut mount: ResMut<Mount>, hero_q: Query<&Hero>, horses: Query<(Entity, &Horse)>) {
    let Ok(hero) = hero_q.single() else { return };
    for (e, h) in &horses {
        let d = h.pos.distance(hero.pos);
        let gone = match h.mood {
            Mood::Bolting { t, .. } => d > FLEE_RANGE || t > 16.0,
            Mood::Parked => d > PARK_RANGE,
            Mood::Ridden => false,
        };
        if gone {
            commands.entity(e).try_despawn();
            if mount.horse == Some(e) {
                mount.horse = None;
            }
        }
    }
}

fn clear(commands: &mut Commands, mount: &mut Mount, hero_q: &mut Query<&mut Hero>, horses: &Query<Entity, With<Horse>>) {
    for e in horses {
        commands.entity(e).try_despawn();
    }
    let auto = mount.auto;
    *mount = Mount { auto, ..default() };
    for mut h in hero_q.iter_mut() {
        h.mounted = false;
        h.ride_lean = 0.0;
    }
}

/// A loaded save starts on foot (the battlefield, and the horse with it, is not saved).
fn reset_on_load(
    mut commands: Commands,
    mut loaded: MessageReader<crate::savegame::GameLoaded>,
    mut mount: ResMut<Mount>,
    mut hero_q: Query<&mut Hero>,
    horses: Query<Entity, With<Horse>>,
) {
    if loaded.read().count() > 0 {
        clear(&mut commands, &mut mount, &mut hero_q, &horses);
    }
}

fn reset_on_new_run(mut commands: Commands, mut mount: ResMut<Mount>, mut hero_q: Query<&mut Hero>, horses: Query<Entity, With<Horse>>) {
    clear(&mut commands, &mut mount, &mut hero_q, &horses);
}
