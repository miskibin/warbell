//! **Standalone model viewer** — `FOREST_VIEW=<model>` boots a *minimal* app (clean stage, even
//! 3-point lighting, a framed camera) that renders ONE model with no world generation, no
//! gameplay, and no HUD. It exists so character/prop models can be inspected and iterated fast —
//! large in frame, unoccluded, on a neutral backdrop — instead of hunting for the hero inside the
//! full game. See the **`model-viewer`** skill for the capture recipe.
//!
//! It reuses the in-crate mesh builders + the shared `CreatureMaterial`, and the same
//! `FOREST_SHOT` / `FOREST_CLIP` capture harness as the game (so `FOREST_CLIP_ORBIT` gives a
//! turntable). `main()` calls [`run`] and returns early when `FOREST_VIEW` is set.
//!
//! Models: `FOREST_VIEW=hero` (default) renders the knight in rest pose; honours
//! `FOREST_EQUIP="weapon,armor"`. `FOREST_VIEW=landmark:mill|hut|spire|pyramid|stones`
//! previews a biome landmark SET-PIECE — the full `LandmarkModel` (merged base + animated
//! parts) on the white vertex-colour material, with the real `RuinsFxPlugin` animators
//! running, auto-framed from the mesh bounds (big props need a far camera, not the
//! character default). `FOREST_VIEW=kit:<model>[,<model>...]` lines up static models of the
//! realistic forest kit (`structures.glb` / `wayside.glb` names, e.g. `kit:wayside_shrine`) on
//! their PBR materials (`FOREST_VIEW_YAW` turns them); its stage lights run hot, so judge relative
//! tones there and absolute ones in game. `FOREST_CAM` still overrides the framing.

use std::f32::consts::FRAC_PI_2;

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::window::WindowResolution;

/// Build + run the minimal viewer app. Returns only when the window closes / a capture exits.
pub fn run() {
    let model = std::env::var("FOREST_VIEW").unwrap_or_default();
    let mut window = Window { title: format!("Warbell — model viewer: {model}"), ..default() };
    // Match the game's capture resolutions so shots/clips are crisp; otherwise a square window.
    window.resolution = if std::env::var("FOREST_SHOT").is_ok() {
        WindowResolution::new(1920, 1080).with_scale_factor_override(1.0)
    } else if std::env::var("FOREST_CLIP").is_ok() {
        WindowResolution::new(1280, 720).with_scale_factor_override(1.0)
    } else {
        WindowResolution::new(1000, 1000).with_scale_factor_override(1.0)
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(window), ..default() }))
        .add_plugins(crate::creature::CreaturePlugin) // the shared CreatureMaterial + its shader
        .add_plugins(crate::quadruped::QuadrupedPlugin) // poses any previewed quadruped from its QuadDrive
        .add_plugins(crate::biped::BipedPlugin) // poses any previewed ork/peasant biped from its BipedDrive
        .add_plugins(crate::npc_skin::NpcSkinPlugin) // realistic skin over previewed townsfolk
        .add_plugins(crate::capture::CapturePlugin) // FOREST_SHOT / FOREST_CLIP (+ _ORBIT turntable)
        .add_plugins(crate::ruins::RuinsFxPlugin) // landmark part animators (sails/orbits/pulses)
        .insert_resource(GlobalAmbientLight { brightness: 160.0, ..default() })
        .insert_resource(ClearColor(Color::srgb(0.16, 0.17, 0.20)))
        // The realistic kit (Blender orks / animals / props) when present, before `setup` builds.
        .add_plugins(bevy::pbr::MaterialPlugin::<crate::realforest::foliage::FoliageMaterial>::default())
        .add_systems(PreStartup, crate::realforest::build_kit)
        .add_systems(Startup, setup);
    crate::grass_trample::insert_buffer(&mut app);

    // `FOREST_VIEW_ANIM=idle|walk|block|attack` drives the REAL game animator on the previewed
    // model (otherwise it shows the static rest pose). Reuses `hero_anim` so what you see matches
    // the game exactly; the drive system synthesises the `Hero` state each frame.
    if std::env::var("FOREST_VIEW_ANIM").is_ok() {
        app.init_resource::<crate::player::PlayerRes>()
            .init_resource::<crate::cinematic::DirectorState>()
            .init_resource::<crate::player::FirstPerson>()
            .add_systems(Update, (anim_drive, crate::player::anim::hero_anim).chain())
            // `FOREST_VIEW_FP=1`: look out of the knight's head, as first person does in game.
            .add_systems(
                Update,
                (crate::player::fp_head_track, crate::player::fp_body_visibility, crate::player::viewer_fp_cam)
                    .chain()
                    .after(crate::player::anim::hero_anim)
                    .run_if(|| std::env::var("FOREST_VIEW_FP").is_ok()),
            )
            // Animal / biped models read their own driver; inert for the hero.
            .add_systems(Update, (quad_anim_drive, biped_anim_drive, townsfolk_haul_props));
        // Orks own their drive (gait phase included) — posed the same frame, as in game.
        app.add_systems(Update, ork_anim_drive.before(crate::biped::animate_biped));
    }
    // `FOREST_VIEW_SAY=<clip under assets/>` — every previewed townsperson says that voice line on a
    // loop, its face playing the clip's mouth-shape track (`lipsync`), silently: look-dev for the
    // talking faces (frame the head with `FOREST_CAM`).
    if let Ok(clip) = std::env::var("FOREST_VIEW_SAY") {
        let clip: &'static str = Box::leak(clip.into_boxed_str());
        app.add_plugins(crate::lipsync::LipSyncPlugin).add_systems(
            Update,
            move |time: Res<Time>, mut commands: Commands, q: Query<Entity, With<crate::npc_skin::Skinned>>| {
                let t = time.elapsed_secs() % 6.0;
                for e in &q {
                    commands.entity(e).try_insert(crate::lipsync::Speech(Some((clip, t))));
                }
            },
        );
    }
    // `FOREST_VIEW=life` — the village-life clips on a row of townsfolk (`FOREST_VIEW_LIFE=sit|
    // rise|carry|bow|yawn|hide`, default the standing acts), posed straight from `anim::village`.
    if std::env::var("FOREST_VIEW").is_ok_and(|v| v == "life") {
        app.add_systems(Update, view_life_drive.before(crate::biped::animate_biped));
    }
    app.run();
}

/// Pose every previewed rig with one village-life clip each (left → right = clip index).
fn view_life_drive(
    time: Res<Time>,
    mut commands: Commands,
    q: Query<(Entity, &GlobalTransform), With<crate::biped::BipedDrive>>,
    mut order: Local<Vec<(f32, Entity)>>,
) {
    order.clear();
    order.extend(q.iter().map(|(e, g)| (g.translation().x, e)));
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let now = time.elapsed_secs();
    for (i, (_, e)) in order.iter().enumerate() {
        let pose = crate::player::anim::village::preview(i, now + i as f32 * 0.7, 0.76);
        commands.entity(*e).insert(crate::biped::PoseOverride(pose));
    }
}

/// Synthesise `Hero`/`HeroHealth` state from `FOREST_VIEW_ANIM` so `hero_anim` plays that clip.
fn anim_drive(time: Res<Time>, mut q: Query<(&mut crate::player::Hero, &mut crate::player::HeroHealth)>) {
    let Ok((mut hero, mut hh)) = q.single_mut() else { return };
    let dt = time.delta_secs();
    hero.moving = false;
    hero.moving_amt = 0.0;
    hero.run_amt = 0.0;
    hero.gait_speed = 0.0;
    hero.on_ground = true;
    hero.attacking = false;
    hero.victory = false;
    hero.heavy = false;
    hero.charge_t = -1.0;
    hero.dash_t = -1.0;
    hh.blocking = false;
    // Loop a one-shot swing of the given studio attack variant for the preview.
    let swing = |hero: &mut crate::player::Hero, variant: u8| {
        hero.attacking = true;
        hero.attack_variant = variant;
        hero.attack_t = (hero.attack_t + dt) % crate::player::ATTACK_DURATION;
    };
    let mode = std::env::var("FOREST_VIEW_ANIM").unwrap_or_default();
    match mode.as_str() {
        "walk" => {
            crate::player::anim::stage_gait(&mut hero, crate::player::SPEED, dt);
        }
        "run" => {
            hero.run_amt = 1.0;
            // `FOREST_VIEW_SPEED=<world u/s>` previews the gait at any pace (default: the sprint).
            let v = std::env::var("FOREST_VIEW_SPEED").ok().and_then(|v| v.parse().ok());
            crate::player::anim::stage_gait(&mut hero, v.unwrap_or(crate::player::SPEED * crate::player::SPRINT_MULT), dt);
        }
        "block" | "defend" => hh.blocking = true,
        "blockwalk" => {
            hh.blocking = true;
            crate::player::anim::stage_gait(&mut hero, crate::player::SPEED, dt);
        }
        // One light swing of a combo step every 2.2 s, its recovery playing out in between.
        "swing1" | "swing2" | "swing3" => {
            let t = time.elapsed_secs() % 2.2;
            if t < crate::player::ATTACK_DURATION {
                hero.attacking = true;
                hero.attack_variant = mode.as_bytes()[5] - b'1';
                hero.attack_t = t;
            }
        }
        // The full light chain (steps 1 → 2 → 3, each pressed as the last lands) every 4 s.
        "combo" => {
            let t = time.elapsed_secs() % 4.0;
            let step = (t / 0.5) as u8;
            if step < 3 && t % 0.5 < crate::player::ATTACK_DURATION {
                hero.attacking = true;
                hero.attack_variant = step;
                hero.attack_t = t % 0.5;
            }
        }
        "attack" | "attack1" => swing(&mut hero, 0),
        "attack2" => swing(&mut hero, 1),
        "attack3" => swing(&mut hero, 2),
        // The charged Heavy Strike: variant 3 = `combat::HEAVY_VARIANT`, with the heavy flag set.
        "heavy" => {
            hero.heavy = true;
            swing(&mut hero, 3);
        }
        // The held charge-stance coil: set absolutely from wall-clock (the per-frame reset above
        // would otherwise defeat an accumulating ramp). Ramps to `combat::CHARGE_THRESHOLD` = 0.8.
        "charge" => hero.charge_t = (time.elapsed_secs() * 0.25).min(0.8),
        // Combined moves: a swing / a leap taken mid-run.
        "runattack" => {
            hero.run_amt = 1.0;
            crate::player::anim::stage_gait(&mut hero, crate::player::SPEED * crate::player::SPRINT_MULT, dt);
            swing(&mut hero, 1);
        }
        "runjump" => {
            hero.run_amt = 1.0;
            crate::player::anim::stage_gait(&mut hero, crate::player::SPEED * crate::player::SPRINT_MULT, dt);
            hero.on_ground = false;
            hero.vel_y = (time.elapsed_secs() * 1.2).cos() * 6.5;
        }
        "victory" => hero.victory = true,
        // Draw / sheathe loop: a short "fight" (combat window open) from 1.0 s of every 8, then calm
        // long enough for the sheathe delay to run out and the kit to go away.
        "draw" | "drawwalk" => {
            let now = time.elapsed_secs();
            if (1.0..1.5).contains(&(now % 8.0)) {
                hero.combat_until = now + 0.1;
            }
            if mode == "drawwalk" {
                crate::player::anim::stage_gait(&mut hero, crate::player::SPEED, dt);
            }
        }
        // Loop the Sand-Dash slide progress (0→1 along the blink) so a clip shows the dash-swipe lunge.
        "dash" => hero.dash_t = (time.elapsed_secs() * 0.5) % crate::player::DASH_TIME,
        "jump" => {
            // Sweep vel_y smoothly +JUMP_SPEED → −JUMP_SPEED so the continuous arc (and the landing
            // squash, when it touches back down at v≈0) is exercised through a turntable/clip.
            hero.on_ground = false;
            hero.vel_y = (time.elapsed_secs() * 1.2).cos() * 6.5; // 6.5 = movement::JUMP_SPEED
        }
        _ => {} // idle
    }
}

/// Drive a previewed quadruped's [`crate::quadruped::QuadDrive`] from `FOREST_VIEW_ANIM`
/// (walk/run/attack/sit/lie; anything else = idle) so a clip shows its gait. Inert for the hero.
fn quad_anim_drive(time: Res<Time>, mut q: Query<&mut crate::quadruped::QuadDrive>) {
    let Ok(mut d) = q.single_mut() else { return };
    let now = time.elapsed_secs();
    d.moving_amt = 0.0;
    d.run_amt = 0.0;
    d.sit_amt = 0.0;
    d.lie_amt = 0.0;
    d.attacking = false;
    match std::env::var("FOREST_VIEW_ANIM").unwrap_or_default().as_str() {
        "walk" => {
            d.moving_amt = 1.0;
            d.phase = now;
        }
        "run" => {
            d.moving_amt = 1.0;
            d.run_amt = 1.0;
            d.phase = now;
        }
        "attack" | "attack1" => {
            d.attacking = true;
            d.attack_t = now % 1.0; // loop the strike for the preview
        }
        "sit" => d.sit_amt = 1.0,
        "lie" => d.lie_amt = 1.0,
        _ => {} // idle
    }
}

/// Drive a previewed ork/peasant biped's [`crate::biped::BipedDrive`] from `FOREST_VIEW_ANIM`
/// (walk/run/carry/hoe/chop/attack/sit; else idle). Inert for the hero/quad models.
fn biped_anim_drive(
    time: Res<Time>,
    mut q: Query<&mut crate::biped::BipedDrive, Without<crate::player::anim::ork::OrkMotion>>,
) {
    // Every previewed rig (the `peasant:lineup` row has several), each keeping its own phase.
    for mut d in &mut q {
        // The held tool + joint layout are the body's (set at spawn), not part of the clip.
        let (phase, tool, limb_dx) = (d.phase, d.tool, d.limb_dx);
        *d = crate::biped::BipedDrive { phase, tool, limb_dx, ..default() };
        drive_biped_clip(&time, &mut d);
    }
}

fn drive_biped_clip(time: &Time, d: &mut crate::biped::BipedDrive) {
    let now = time.elapsed_secs();
    match std::env::var("FOREST_VIEW_ANIM").unwrap_or_default().as_str() {
        "walk" => {
            d.moving_amt = 1.0;
            d.walk_phase = now * 8.0 + d.phase;
        }
        "run" => {
            d.moving_amt = 1.0;
            d.run_amt = 1.0;
            d.walk_phase = now * 14.0 + d.phase;
        }
        "carry" => {
            d.carrying = true;
            d.moving_amt = 1.0;
            d.walk_phase = now * 8.0 + d.phase;
        }
        // A carter hauling his handcart home (fists on the shafts; see `townsfolk_haul_props`).
        "pull" | "cart" => {
            d.carrying = true;
            d.pulling = true;
            d.moving_amt = 1.0;
            d.walk_phase = now * 8.0 + d.phase;
        }
        "hoe" | "work" => d.work = 1,
        "chop" | "pick" => d.work = 2,
        "attack" | "attack1" => {
            d.attacking = true;
            d.attack_t = (now % 1.0) * crate::player::ATTACK_DURATION;
        }
        // The archer's draw-and-loose on a loop: the 1.15s shot + a 0.85s at-ease beat between
        // shots (matches the in-game `villagers::BOW_SHOT_SECS` pacing near enough for preview).
        "bow" | "shoot" => {
            let p = (now % 2.0) / 1.15;
            d.bow = p <= 1.0;
            d.bow_t = p.min(1.0);
        }
        "sit" => d.sitting = true,
        _ => {} // idle
    }
}

/// Drive a previewed ork's animator ([`crate::player::anim::ork::OrkMotion`]) from
/// `FOREST_VIEW_ANIM`, synthesising what its brain would report: `walk` (patrol pace) / `run` (a
/// charge, on guard) / `charge` (closing in with the club cocked) as ground motion; `guard`;
/// `smash` / `swipe` / `attack` (either) / `cast` as a standing fight with a blow landing every
/// cooldown (the countdown winds it up, the stamp lands it); `sit`; else at ease. Owns the ork's
/// drive outright ([`biped_anim_drive`] skips orks — its clip phase would fight the foot-locked
/// gait phase) and runs before the pose is taken, like `orks::ork_drive` in game.
fn ork_anim_drive(
    time: Res<Time>,
    mut q: Query<(&mut crate::biped::BipedDrive, &mut crate::player::anim::ork::OrkMotion)>,
    mut ground: Local<Vec2>,
) {
    use crate::player::anim::ork::OrkBlow;
    let (now, dt) = (time.elapsed_secs(), time.delta_secs());
    // `FOREST_VIEW_FOE_ANIM` drives the foe facing a first-person preview (the hero takes `_ANIM`).
    let mode = std::env::var("FOREST_VIEW_FOE_ANIM").or_else(|_| std::env::var("FOREST_VIEW_ANIM")).unwrap_or_default();
    // World speed (the root is unscaled here: rig units / s) — a grunt's patrol / charge in rig units.
    let speed = match mode.as_str() {
        "walk" => 1.75,
        "run" | "charge" => 2.6,
        _ => 0.0,
    };
    ground.y += speed * dt;
    let cd = if mode == "cast" { crate::orks::SHAMAN_CAST_CD } else { crate::orks::ORK_ATTACK_CD };
    let (strike_in, stamp) = match mode.as_str() {
        "smash" | "swipe" | "attack" | "cast" => {
            // A blow lands every `cd` from t = 1s: countdown to the next, stamp of the last.
            let t = now - 1.0;
            let last = (t / cd).floor();
            (cd - (t - last * cd), if t >= 0.0 { 1.0 + last * cd } else { 0.0 })
        }
        "guard" | "run" => (crate::player::anim::ork::STRIKE_HOLD, 0.0),
        "charge" => (crate::player::anim::ork::SWING_T, 0.0),
        _ => (f32::INFINITY, 0.0),
    };
    for (mut d, mut m) in &mut q {
        let phase = d.phase;
        *d = crate::biped::BipedDrive { phase, moving_amt: if speed > 0.0 { 1.0 } else { 0.0 }, sitting: mode == "sit", walk_phase: d.walk_phase, ..default() };
        match mode.as_str() {
            "smash" if !m.build.staff => m.set_next(OrkBlow::Smash),
            "swipe" if !m.build.staff => m.set_next(OrkBlow::Swipe),
            _ => {}
        }
        m.track(&mut d, stamp, strike_in, *ground, 1.0, now, dt);
    }
}

/// Parse `FOREST_CAM="ex,ey,ez,tx,ty,tz"` (eye + look-at), same format as the game's.
fn parse_cam() -> Option<(Vec3, Vec3)> {
    let s = std::env::var("FOREST_CAM").ok()?;
    let n: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    (n.len() == 6).then(|| (Vec3::new(n[0], n[1], n[2]), Vec3::new(n[3], n[4], n[5])))
}

/// Min/max corners of a mesh's vertices — the auto-framing input for big props.
fn mesh_bounds(m: &Mesh) -> (Vec3, Vec3) {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    if let Some(VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) {
        for v in p {
            lo = lo.min(Vec3::from(*v));
            hi = hi.max(Vec3::from(*v));
        }
    }
    (lo, hi)
}

/// The landmark set-piece named by `landmark:<name>` (biome aliases accepted).
fn landmark_model(name: &str) -> crate::ruins::LandmarkModel {
    use crate::landmark_models as lm;
    match name {
        "hut" | "witch" | "swamp" => lm::witch_hut(),
        "spire" | "snow" | "ice" => lm::frozen_spire(),
        "pyramid" | "desert" => lm::sunken_pyramid(),
        "stones" | "rocky" | "trilithon" | "circle" => lm::standing_stones(),
        // `mill` / `forest` / anything else → the old mill.
        _ => lm::old_mill(),
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
) {
    // Camera — framed on a ~1.8u-tall model standing at the origin (chest-height look-at).
    // The landmark path below re-frames from the model's real bounds instead.
    let view_name = std::env::var("FOREST_VIEW").unwrap_or_default();
    let default_cam = if matches!(view_name.as_str(), "peasant:lineup" | "people" | "life") {
        (Vec3::new(1.2, 1.3, 7.6), Vec3::new(0.0, 0.85, 0.0))
    } else {
        (Vec3::new(0.0, 1.0, 3.0), Vec3::new(0.0, 0.85, 0.0))
    };
    let (mut eye, mut target) = parse_cam().unwrap_or(default_cam);

    // `FOREST_VIEW=landmark:<name>` — a full landmark set-piece: merged base on the white
    // vertex-colour material + its animated parts (RuinsFxPlugin drives them here too).
    let view_env = std::env::var("FOREST_VIEW").unwrap_or_default();
    // `FOREST_VIEW=kit:<model>[,<model>...]` — static models from the realistic forest kit (e.g.
    // `kit:wayside_signpost,wayside_cairn`), side by side along +X on their real PBR materials,
    // auto-framed. `FOREST_VIEW_YAW=<rad>` turns every model in place.
    if let Some(list) = view_env.strip_prefix("kit:") {
        let Some(kit) = crate::realforest::get() else {
            error!("viewer: the realistic forest kit failed to load");
            return;
        };
        let yaw: f32 = std::env::var("FOREST_VIEW_YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let names: Vec<&str> = list.split(',').map(str::trim).filter(|n| kit.has(n)).collect();
        let reach = |n: &str| kit.extents(n).map_or(1.0, |(h, _)| h.length());
        let total: f32 = names.iter().map(|n| reach(n) * 2.0 + 0.5).sum();
        let (mut x, mut top) = (-total * 0.5, 1.0_f32);
        for n in &names {
            let r = reach(n);
            x += r + 0.25;
            kit.spawn_static(&mut commands, n, Transform::from_xyz(x, 0.0, 0.0).with_rotation(Quat::from_rotation_y(yaw)));
            top = top.max(kit.extents(n).map_or(1.0, |(_, h)| h));
            x += r + 0.25;
        }
        if parse_cam().is_none() {
            let radius = (total * 0.5).max(top).max(1.0);
            target = Vec3::new(0.0, top * 0.4, 0.0);
            eye = target + Vec3::new(0.35, 0.42, 1.0).normalize() * radius * 1.9;
        }
        commands.spawn((Camera3d::default(), Transform::from_translation(eye).looking_at(target, Vec3::Y)));
        spawn_stage_lights(&mut commands);
        spawn_ground(&mut commands, &mut meshes, &mut std_mats, total.max(6.0));
        return;
    }
    if let Some(name) = view_env.strip_prefix("landmark:") {
        let model = landmark_model(name);
        if parse_cam().is_none() {
            // Auto-fit: pull back proportionally to the model's bounding radius, from a
            // low front-right three-quarter angle (shows silhouette + ground dressing).
            let (lo, hi) = mesh_bounds(&model.base);
            let centre = (lo + hi) * 0.5;
            let radius = ((hi - lo) * 0.5).length().max(1.0);
            target = Vec3::new(centre.x, centre.y * 0.9, centre.z);
            eye = centre + Vec3::new(0.62, 0.38, 1.0).normalize() * radius * 2.1;
        }
        commands.spawn((Camera3d::default(), Transform::from_translation(eye).looking_at(target, Vec3::Y)));
        spawn_stage_lights(&mut commands);
        spawn_ground(&mut commands, &mut meshes, &mut std_mats, 14.0);
        let white = std_mats.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.92,
            ..default()
        });
        crate::ruins::spawn_landmark_model(
            &mut commands,
            &mut meshes,
            &mut std_mats,
            &white,
            model,
            Transform::default(),
        );
        return;
    }

    if view_name == "people:scale" {
        // `FOREST_VIEW=people:scale`: hero + townsfolk at their real in-world scales, front-on
        // through an ORTHOGRAPHIC camera (2.2 world units tall) so heights / widths can be
        // measured straight off the image (1080 px / 2.2 u ≈ 491 px per world unit).
        let (eye, target) = parse_cam().unwrap_or((Vec3::new(0.0, 0.8, 12.0), Vec3::new(0.0, 0.8, 0.0)));
        commands.spawn((
            Camera3d::default(),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: bevy::camera::ScalingMode::FixedVertical { viewport_height: 2.2 },
                ..OrthographicProjection::default_3d()
            }),
            Transform::from_translation(eye).looking_at(target, Vec3::Y),
        ));
    } else {
        commands.spawn((Camera3d::default(), Transform::from_translation(eye).looking_at(target, Vec3::Y)));
    }

    spawn_stage_lights(&mut commands);
    spawn_ground(&mut commands, &mut meshes, &mut std_mats, 6.0);

    // `FOREST_VIEW=trees` — the harvestable "chop these for wood" resources on the clean stage
    // (quest-card art): a few tree kinds + a desert saguaro, no world scene around them. These use
    // vertex-coloured meshes on a plain white `StandardMaterial` (like the `FOREST_TREELINE` hook),
    // not the creature material, so we spawn them here and skip the hero model entirely.
    if std::env::var("FOREST_VIEW").as_deref() == Ok("trees") {
        let mat = std_mats.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.62,
            reflectance: 0.5,
            ..default()
        });
        let items: [Mesh; 4] = [
            crate::trees::build_tree_mesh(crate::trees::TreeKind::Broadleaf),
            crate::trees::build_tree_mesh(crate::trees::TreeKind::Pine),
            crate::trees::build_tree_mesh(crate::trees::TreeKind::Birch),
            crate::biome_desert::build_saguaro_mesh(1),
        ];
        let n = items.len();
        for (i, mesh) in items.into_iter().enumerate() {
            let x = (i as f32 - (n as f32 - 1.0) * 0.5) * 2.7;
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(x, 0.0, 0.0).with_scale(Vec3::splat(1.15)),
            ));
        }
        return;
    }

    // The model itself, on the shared creature material. The root also carries `Hero`/`HeroHealth`
    // so the optional `hero_anim` preview (FOREST_VIEW_ANIM) has state to read; inert otherwise.
    let mat = crate::creature::make_hero_material(&mut creature_mats);
    let root = commands
        .spawn((
            Transform::default(),
            Visibility::Visible,
            crate::player::Hero::fresh(Vec2::ZERO, 0.0, 0.0),
            crate::player::HeroHealth::default(),
        ))
        .id();
    spawn_model(&mut commands, root, &mut meshes, &mut images, &mat, &mut creature_mats);
    // First-person preview / a foe beside him: the knight at his in-game scale, plus an optional foe
    // facing him `FOREST_VIEW_FOE_DIST` (default 1.6) in front — `FOREST_VIEW_FOE=grunt|scout|
    // berserker|shaman` (frame both from the side with `FOREST_CAM` to compare heights).
    if std::env::var("FOREST_VIEW_FP").is_ok() || std::env::var("FOREST_VIEW_FOE").is_ok() {
        commands.entity(root).insert(Transform::from_scale(crate::player::HERO_SCALE_V));
        if let Ok(foe) = std::env::var("FOREST_VIEW_FOE") {
            use crate::orks::OrkVariant::*;
            let variant = match foe.as_str() {
                "scout" => Scout,
                "berserker" => Berserker,
                "shaman" => Shaman,
                _ => Grunt,
            };
            let dist = std::env::var("FOREST_VIEW_FOE_DIST").ok().and_then(|v| v.parse().ok()).unwrap_or(1.6);
            let ork = commands
                .spawn((
                    Transform::from_xyz(0.0, 0.0, dist)
                        .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                        .with_scale(Vec3::splat(crate::orks::world_scale(variant))),
                    Visibility::Visible,
                    crate::biped::BipedDrive::default(),
                    crate::orks::ork_motion(variant, 7),
                ))
                .id();
            let h = crate::orks::ork_biped_meshes(variant, crate::orks::Faction::Red).upload(&mut meshes);
            let (hip_dx, shoulder_dx, head_scale) = crate::orks::rig_dims(variant);
            let shield_xf = Transform {
                translation: Vec3::new(0.0, 0.0, 0.14),
                rotation: Quat::from_euler(EulerRot::XYZ, 0.15, -0.45, 0.1),
                scale: Vec3::ONE,
            };
            crate::biped::spawn_biped(&mut commands, ork, &mat, h, head_scale, 1.0, hip_dx, shoulder_dx, -0.05, Some(shield_xf));
        }
    }
}

/// Spawn the model named by `FOREST_VIEW` under `root`. Add new models as `match` arms.
fn spawn_model(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mat: &Handle<crate::creature::CreatureMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
    let view = std::env::var("FOREST_VIEW").unwrap_or_default();
    match view.as_str() {
        // The ork on the shared studio biped skeleton (Phase 2 re-rig verification). Rest pose
        // unless `FOREST_VIEW_ANIM` is wired for bipeds; geometry/proportions read true here.
        // `FOREST_VIEW=ork:scout|berserker|shaman` picks the variant (default grunt);
        // `ork:torch` swaps the buckler for the torch-bearer's held war-torch (grip/rake
        // verification — tune `orks::ork_torch_bake` against this view). The flame marker ball
        // sits at `ork_torch_tip` so the in-game flame/light placement reads here too.
        s if s.starts_with("ork") || s.starts_with("orc") => {
            use crate::orks::OrkVariant::*;
            let variant = match s.rsplit(':').next() {
                Some("scout") => Scout,
                Some("berserker") => Berserker,
                Some("shaman") => Shaman,
                _ => Grunt,
            };
            let shield_xf = Transform {
                translation: Vec3::new(0.0, 0.0, 0.14),
                rotation: Quat::from_euler(EulerRot::XYZ, 0.15, -0.45, 0.1),
                scale: Vec3::ONE,
            };
            let mut h = crate::orks::ork_biped_meshes(variant, crate::orks::Faction::Red).upload(meshes);
            let torch = s.contains("torch");
            if torch {
                h = h.with_shield(Some(meshes.add(crate::orks::ork_torch_mesh())));
            }
            // The ork animator, as in game (`ork_anim_drive` plays its clips under FOREST_VIEW_ANIM).
            commands.entity(root).insert((crate::biped::BipedDrive::default(), crate::orks::ork_motion(variant, 7)));
            let (hip_dx, shoulder_dx, head_scale) = crate::orks::rig_dims(variant);
            let (_, off_hand) = crate::biped::spawn_biped(
                commands, root, mat, h, head_scale, 1.0, hip_dx, shoulder_dx, -0.05, Some(shield_xf),
            );
            if let (true, Some(hand)) = (torch, off_hand) {
                // Bright vertex-coloured stand-in for the emissive flame (the viewer only carries
                // the shared CreatureMaterial) — placement, not look.
                let flame = crate::orks::tinted_flame_marker();
                commands.entity(hand).with_children(|p| {
                    p.spawn((
                        Mesh3d(meshes.add(flame)),
                        MeshMaterial3d(mat.clone()),
                        Transform::from_translation(crate::orks::ork_torch_tip()),
                    ));
                });
            }
        }
        // The hero next to townsfolk at their real in-world scales (see the ortho camera above):
        // average / stocky / lean / heavy men, then the youngest / middle / oldest child build.
        "people:scale" => {
            use crate::peasant_model::PeasantKind::*;
            let step = 0.46;
            commands.entity(root).insert(Transform {
                translation: Vec3::new(-3.5 * step, 0.0, 0.0),
                scale: crate::player::HERO_SCALE_V,
                ..default()
            });
            crate::player::spawn_hero_meshes(commands, root, meshes, images, creature_mats);
            let row: [(bool, u32, u32, u32); 7] = [
                (false, 0, 0xd8a06a, 0x6a4a2a),
                (false, 1, 0xc08866, 0x5a8fc8),
                (false, 2, 0xe8c4a0, 0x4a6a3a),
                (false, 4, 0xa36b4a, 0x7a3a26),
                (true, 0, 0xdca78a, 0x3a7a72),
                (true, 1, 0xe8c4a0, 0x8a6a3a),
                (true, 7, 0xc89070, 0x6a4a6a),
            ];
            for (i, (kid, look, skin, tunic)) in row.into_iter().enumerate() {
                let x = (i as f32 - 2.5) * step;
                let e = commands
                    .spawn((Transform::from_xyz(x, 0.0, 0.0).with_scale(Vec3::splat(crate::villagers::SCALE)), Visibility::Visible))
                    .id();
                spawn_viewer_peasant(commands, e, meshes, mat, Unemployed, false, kid, look, skin, tunic);
            }
        }
        // The village-life clip review row (see `view_life_drive`): eight townsfolk, men, women,
        // an elder and a kid, with a bench-height slab under them for the seated clips.
        "life" => {
            use crate::peasant_model::PeasantKind::Unemployed;
            let looks = [0u32, 2, 4, 5, 1, 8, 11, 3];
            let n = looks.len() as f32;
            for (i, look) in looks.into_iter().enumerate() {
                let x = (i as f32 - (n - 1.0) * 0.5) * 0.85;
                let e = commands.spawn((Transform::from_xyz(x, 0.0, 0.0), Visibility::Visible)).id();
                commands.entity(root).add_child(e);
                spawn_viewer_peasant(commands, e, meshes, mat, Unemployed, false, i == 7, look, 0xd8a06a, [0x6a4a2a, 0x5a8fc8, 0x7a3a26, 0x3a7a72][i % 4]);
            }
            if std::env::var("FOREST_VIEW_LIFE").is_ok_and(|m| m == "sit") {
                let slab = commands
                    .spawn((
                        Mesh3d(meshes.add(Cuboid::new(n * 0.85, 0.06, 0.3))),
                        MeshMaterial3d(mat.clone()),
                        Transform::from_xyz(0.0, 0.53, -0.04),
                    ))
                    .id();
                commands.entity(root).add_child(slab);
            }
        }
        // A row of townsfolk (every trade + desert + a kid, varied looks) for one-shot review of
        // the realistic kit bodies. Frame it with the default lineup camera or `FOREST_CAM`.
        "peasant:lineup" | "people" => {
            use crate::peasant_model::PeasantKind::*;
            let row: [(crate::peasant_model::PeasantKind, bool, bool, u32, u32, u32); 10] = [
                (Unemployed, false, false, 0, 0xd8a06a, 0x6a4a2a),
                (Unemployed, false, false, 1, 0xc08866, 0x5a8fc8),
                (Farmer, false, false, 4, 0xe8c4a0, 0x4a6a3a),
                (Woodcutter, false, false, 9, 0xa36b4a, 0x7a3a26),
                (Miner, false, false, 3, 0xdca78a, 0x8a6a3a),
                (Guard, false, false, 6, 0xc89070, 0x7a3a26),
                (Archer, false, false, 5, 0xe8c4a0, 0x4a6a3a),
                (Farmer, true, false, 7, 0xd8a06a, 0xbf9a55),
                (Guard, true, false, 10, 0xc08866, 0xbf9a55),
                (Unemployed, false, true, 2, 0xdca78a, 0x3a7a72),
            ];
            let n = row.len() as f32;
            for (i, (kind, desert, kid, look, skin, tunic)) in row.into_iter().enumerate() {
                let x = (i as f32 - (n - 1.0) * 0.5) * 0.85;
                let e = commands.spawn((Transform::from_xyz(x, 0.0, 0.0), Visibility::Visible)).id();
                commands.entity(root).add_child(e);
                spawn_viewer_peasant(commands, e, meshes, mat, kind, desert, kid, look, skin, tunic);
            }
        }
        // The peasant worker types on the shared skeleton (Phase 3). `FOREST_VIEW=peasant:farmer|
        // miner|unemployed|guard` picks the type (default woodcutter); `:desert` the rival's garb,
        // `:kid` a child; `FOREST_VIEW_LOOK=<n>` the realistic body's face / hair / beard look.
        s if s.starts_with("peasant") => {
            use crate::peasant_model::PeasantKind::*;
            // Order-independent so `peasant:guard:desert` still reads as a guard (a plain `rsplit`
            // grabbed the trailing `desert` and silently fell back to woodcutter).
            let kind = if s.contains("farmer") {
                Farmer
            } else if s.contains("miner") {
                Miner
            } else if s.contains("unemployed") {
                Unemployed
            } else if s.contains("guard") {
                Guard
            } else if s.contains("archer") {
                Archer
            } else {
                Woodcutter
            };
            // `FOREST_VIEW=peasant:guard:desert` (any `desert` in the spec) inspects the rival's
            // desert-garbed variant (turban + cloak, sandy tunic).
            let desert = s.contains("desert");
            let kid = s.contains("kid");
            let tunic = if desert { 0xbf9a55 } else { 0x6a4a2a };
            let look = std::env::var("FOREST_VIEW_LOOK").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            spawn_viewer_peasant(commands, root, meshes, mat, kind, desert, kid, look, 0xd8a06a, tunic);
        }
        // The studio quadruped animals on the shared quad skeleton (Phase 4). `FOREST_VIEW=
        // animal:wolf|dog|horse|deer|camel|bear|polar` picks the species (default wolf). Rest/idle
        // stance (the viewer runs no animator).
        s if s.starts_with("animal") => {
            use crate::quadruped::QuadSpecies::*;
            let species = match s.rsplit(':').next() {
                Some("dog") => Dog,
                Some("horse") => Horse,
                Some("deer") => Deer,
                Some("camel") => Camel,
                Some("bear") => Bear,
                Some("polar") | Some("polarbear") => PolarBear,
                _ => Wolf,
            };
            let h = crate::quadruped::quad_meshes(species).upload(meshes);
            commands.entity(root).insert(crate::quadruped::QuadDrive::new(species));
            crate::quadruped::spawn_quad(commands, root, mat, species, h);
        }
        // A box-rig critter (`critters.rs`) in its rest pose: `FOREST_VIEW=critter:rabbit|cat|scorpion|croc`
        // (the rabbit is the downloaded model, `creatures_dl.py`).
        s if s.starts_with("critter") => {
            use crate::critters::Species;
            let sp = match s.rsplit(':').next() {
                Some("cat") => Species::Cat,
                Some("scorpion") => Species::Scorpion,
                Some("croc") => Species::BogCroc,
                _ => Species::Rabbit,
            };
            let spec = crate::critters::build(sp);
            let torso = meshes.add(spec.torso);
            let parts: Vec<_> = spec.parts.into_iter().map(|p| (p.pivot, meshes.add(p.mesh))).collect();
            commands.entity(root).with_children(|c| {
                c.spawn((Mesh3d(torso), MeshMaterial3d(mat.clone())));
                for (pivot, m) in parts {
                    c.spawn((Mesh3d(m), MeshMaterial3d(mat.clone()), Transform::from_translation(pivot)));
                }
            });
        }
        // A biome-warden boss (static rest pose — torso + limb parts at their pivots, at the real
        // root scale). `FOREST_VIEW=boss:forest|snow|rocky|desert|swamp` picks the warden.
        s if s.starts_with("boss") || s.starts_with("warden") => {
            use crate::biome::Biome;
            let biome = match s.rsplit(':').next() {
                Some("snow") | Some("balwan") => Biome::Snow,
                Some("rocky") | Some("golem") => Biome::Rocky,
                Some("desert") | Some("revenant") => Biome::Desert,
                Some("swamp") | Some("hag") => Biome::Swamp,
                _ => Biome::Forest,
            };
            let spec = crate::boss::models::build(biome);
            let scale = crate::boss::models::root_scale(biome);
            commands.entity(root).insert(Transform::from_scale(Vec3::splat(scale)));
            crate::boss::spawn_body(commands, root, biome, spec, meshes, mat, false);
        }
        // Isolated 1:1 transcription of the three.js previs knight (static rest pose).
        "knight2" => crate::previs_knight::spawn(commands, root, meshes, mat.clone()),
        _ => {
            // The Royal Footman, in the animator's rest pose (sword carried, shield on the arm).
            // `FOREST_EQUIP` still restyles the first-person viewmodel, not this body.
            let _ = parse_equip();
            crate::player::spawn_hero_meshes(commands, root, meshes, images, creature_mats);
        }
    }
}

/// One previewed townsperson under `root`, built exactly like `villagers::build_biped_body`
/// (same per-person skeleton via `peasant_model::person_rig`, same off-hand mount, build root scale).
#[allow(clippy::too_many_arguments)]
fn spawn_viewer_peasant(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<crate::creature::CreatureMaterial>,
    kind: crate::peasant_model::PeasantKind,
    desert: bool,
    kid: bool,
    look: u32,
    skin: u32,
    tunic: u32,
) {
    let m = crate::peasant_model::peasant_biped_meshes(kind, skin, tunic, 0x3a2a18, kid, desert, look);
    // Budget check: the whole person as drawn (both limbs of each pair, tool + off-hand prop).
    let tris = |o: &Mesh| o.indices().map_or(o.count_vertices(), |i| i.len()) / 3;
    let body = [&m.hips, &m.torso, &m.neck, &m.head, &m.shoulder_l, &m.shoulder_r, &m.elbow_l, &m.elbow_r, &m.hip_l, &m.hip_r, &m.knee_l, &m.knee_r, &m.foot_l, &m.foot_r];
    let props = [&m.weapon, &m.shield].into_iter().flatten().map(tris).sum::<usize>();
    let skinned = m.skin.as_ref().map_or(0, tris);
    info!(
        "viewer person {kind:?} look {look} desert {desert} kid {kid}: {} body tris{} + {props} prop tris",
        body.into_iter().map(tris).sum::<usize>() + skinned,
        if m.skin.is_some() { " (skinned)" } else { "" }
    );
    // The person's skeleton + height (as `villagers::spawn` builds it): the build's root scale on
    // its own biped root, under the caller's (adult) scale.
    let rig = crate::peasant_model::person_rig(kind, kid, look);
    if let Some((_, hi)) = m.skin.as_ref().map(mesh_bounds) {
        // Head top above the soles, in the caller's units (× the root scale it is spawned under).
        info!(
            "viewer person {kind:?} look {look} kid {kid}: {:.3} tall x root scale (build root {:.3}, head {:.3}, limbs {:?})",
            (hi.y - 0.06 - rig.drop) * rig.root,
            rig.root,
            rig.head_scale,
            rig.limbs
        );
    }
    let h = m.upload(meshes);
    let body = commands.spawn((Transform::from_scale(Vec3::splat(rig.root)), Visibility::Visible)).id();
    commands.entity(root).add_child(body);
    let (hip_dx, shoulder_dx) = (rig.hip_dx, rig.shoulder_dx);
    // The trade's tool in hand, gripped by the tool clips exactly as in town.
    commands.entity(body).insert(crate::biped::BipedDrive {
        phase: look as f32 * 0.9,
        tool: crate::peasant_model::held_tool(kind),
        limb_dx: Vec2::new(hip_dx, shoulder_dx),
        ..default()
    });
    // Off-hand mount matches `villagers::build_biped_body` — only the guard / archer mesh sets
    // carry an off-hand prop, so this stays empty-handed for the worker kinds.
    let shield_xf = Transform {
        translation: Vec3::new(0.0, 0.0, 0.14),
        rotation: Quat::from_euler(EulerRot::XYZ, 0.15, -0.45, 0.1),
        ..default()
    };
    crate::biped::spawn_biped_scaled(commands, body, mat, h, rig.head_scale, 1.0, hip_dx, shoulder_dx, -0.06 - rig.drop, Some(shield_xf), rig.limbs);
    // The realistic cast member the same townsperson wears in town (`npc_skin`).
    use crate::npc_skin as ns;
    use crate::peasant_model::PeasantKind as K;
    let skin_look = match kind {
        _ if kid => if look % 2 == 0 { ns::BOY } else { ns::GIRL },
        K::Woodcutter => ns::WOODCUTTER,
        K::Farmer => ns::FARMER,
        K::Miner => ns::MINER,
        K::Guard => ns::GUARD,
        K::Archer => ns::ARCHER,
        K::Unemployed => [ns::VILLAGER_M, ns::VILLAGER_F1, ns::VILLAGER_F2, ns::ELDER_M, ns::ELDER_F, ns::PRIEST, ns::MERCHANT][look as usize % 7],
    };
    commands.entity(body).insert(ns::SkinRequest(skin_look));
}

/// Three-point-ish lighting: a shadowed key, a soft fill from the opposite side, a back rim.
fn spawn_stage_lights(commands: &mut Commands) {
    commands.spawn((
        DirectionalLight { illuminance: 10_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(4.0, 7.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 3_500.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-5.0, 4.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 2_500.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(0.0, 3.0, -6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Neutral ground disk so the model casts a shadow and isn't floating in void.
/// `r` scales with the subject (6u suits a character; landmarks want more).
fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    std_mats: &mut Assets<StandardMaterial>,
    r: f32,
) {
    let ground = std_mats.add(StandardMaterial {
        base_color: Color::srgb(0.22, 0.24, 0.27),
        perceptual_roughness: 0.96,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(r))),
        MeshMaterial3d(ground),
        Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
    ));
}

/// `FOREST_EQUIP="weapon_id,armor_id"` → `(weapon, armor)`; either side may be empty.
fn parse_equip() -> (Option<String>, Option<String>) {
    let Ok(s) = std::env::var("FOREST_EQUIP") else { return (None, None) };
    let mut it = s.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).map(str::to_string);
    (it.next(), it.next())
}

/// `FOREST_VIEW_ANIM=carry|pull` on a townsperson: hang the load the hauling grip closes on — the
/// woodcutter's log cradled at the chest (`lumberjack::attach_log`'s place) or the miner's
/// handcart on its shafts (`miner::spawn_cart`) — on each previewed tool-holder, once.
fn townsfolk_haul_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    fresh: Query<(Entity, &crate::biped::BipedDrive), Added<crate::biped::BipedDrive>>,
) {
    let mode = std::env::var("FOREST_VIEW_ANIM").unwrap_or_default();
    let cart = matches!(mode.as_str(), "pull" | "cart");
    if mode != "carry" && !cart {
        return;
    }
    for (e, d) in &fresh {
        if d.tool == crate::biped::HeldTool::None {
            continue;
        }
        if cart {
            let a = crate::miner::CartAssets::build(&mut meshes, &mut mats);
            crate::miner::spawn_cart(&mut commands, e, &a);
        } else {
            let log = commands
                .spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.26, 0.26, 1.05))),
                    MeshMaterial3d(mats.add(StandardMaterial { base_color: Color::srgb(0.42, 0.28, 0.15), perceptual_roughness: 0.95, ..default() })),
                    Transform::from_translation(crate::player::anim::LOG_CRADLE + Vec3::Y * crate::villagers::VIL_RIG_OFF)
                        .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
                ))
                .id();
            commands.entity(e).add_child(log);
        }
    }
}
