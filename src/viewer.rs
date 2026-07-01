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
//! `FOREST_EQUIP="weapon,armor"`. `FOREST_VIEW=landmark:trilithon|spire|pyramid`
//! previews a biome landmark prop on the white vertex-colour material.

use std::f32::consts::FRAC_PI_2;

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
        .add_plugins(crate::capture::CapturePlugin) // FOREST_SHOT / FOREST_CLIP (+ _ORBIT turntable)
        .insert_resource(GlobalAmbientLight { brightness: 160.0, ..default() })
        .insert_resource(ClearColor(Color::srgb(0.16, 0.17, 0.20)))
        .add_systems(Startup, setup);

    // `FOREST_VIEW_ANIM=idle|walk|block|attack` drives the REAL game animator on the previewed
    // model (otherwise it shows the static rest pose). Reuses `hero_anim` so what you see matches
    // the game exactly; the drive system synthesises the `Hero` state each frame.
    if std::env::var("FOREST_VIEW_ANIM").is_ok() {
        app.init_resource::<crate::player::PlayerRes>()
            .init_resource::<crate::cinematic::DirectorState>()
            .init_resource::<crate::player::FirstPerson>()
            .add_systems(Update, (anim_drive, crate::player::anim::hero_anim).chain())
            // Animal / biped models read their own driver; inert for the hero.
            .add_systems(Update, (quad_anim_drive, biped_anim_drive));
    }
    app.run();
}

/// Synthesise `Hero`/`HeroHealth` state from `FOREST_VIEW_ANIM` so `hero_anim` plays that clip.
fn anim_drive(time: Res<Time>, mut q: Query<(&mut crate::player::Hero, &mut crate::player::HeroHealth)>) {
    let Ok((mut hero, mut hh)) = q.single_mut() else { return };
    let dt = time.delta_secs();
    hero.moving = false;
    hero.moving_amt = 0.0;
    hero.run_amt = 0.0;
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
    match std::env::var("FOREST_VIEW_ANIM").unwrap_or_default().as_str() {
        "walk" => {
            hero.moving = true;
            hero.moving_amt = 1.0;
            hero.walk_phase += dt * 7.0; // = movement::STEP_FREQ
        }
        "run" => {
            hero.moving = true;
            hero.moving_amt = 1.0;
            hero.run_amt = 1.0;
            hero.walk_phase += dt * 7.0 * 1.75; // STEP_FREQ * SPRINT_MULT
        }
        "block" | "defend" => hh.blocking = true,
        "blockwalk" => {
            hh.blocking = true;
            hero.moving = true;
            hero.moving_amt = 1.0;
            hero.walk_phase += dt * 7.0;
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
            hero.moving = true;
            hero.moving_amt = 1.0;
            hero.run_amt = 1.0;
            hero.walk_phase += dt * 7.0 * 1.75;
            swing(&mut hero, 1);
        }
        "runjump" => {
            hero.moving = true;
            hero.moving_amt = 1.0;
            hero.run_amt = 1.0;
            hero.walk_phase += dt * 7.0 * 1.75;
            hero.on_ground = false;
            hero.vel_y = (time.elapsed_secs() * 1.2).cos() * 6.5;
        }
        "victory" => hero.victory = true,
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
fn biped_anim_drive(time: Res<Time>, mut q: Query<&mut crate::biped::BipedDrive>) {
    let Ok(mut d) = q.single_mut() else { return };
    let now = time.elapsed_secs();
    *d = crate::biped::BipedDrive::default();
    match std::env::var("FOREST_VIEW_ANIM").unwrap_or_default().as_str() {
        "walk" => {
            d.moving_amt = 1.0;
            d.walk_phase = now * 8.0;
        }
        "run" => {
            d.moving_amt = 1.0;
            d.run_amt = 1.0;
            d.walk_phase = now * 14.0;
        }
        "carry" => {
            d.carrying = true;
            d.moving_amt = 1.0;
            d.walk_phase = now * 8.0;
        }
        "hoe" | "work" => d.work = 1,
        "chop" | "pick" => d.work = 2,
        "attack" | "attack1" => {
            d.attacking = true;
            d.attack_t = (now % 1.0) * crate::player::ATTACK_DURATION;
        }
        "sit" => d.sitting = true,
        _ => {} // idle
    }
}

/// Parse `FOREST_CAM="ex,ey,ez,tx,ty,tz"` (eye + look-at), same format as the game's.
fn parse_cam() -> Option<(Vec3, Vec3)> {
    let s = std::env::var("FOREST_CAM").ok()?;
    let n: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    (n.len() == 6).then(|| (Vec3::new(n[0], n[1], n[2]), Vec3::new(n[3], n[4], n[5])))
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
) {
    // Camera — framed on a ~1.8u-tall model standing at the origin (chest-height look-at).
    let (eye, target) = parse_cam().unwrap_or((Vec3::new(0.0, 1.0, 3.0), Vec3::new(0.0, 0.85, 0.0)));
    commands.spawn((Camera3d::default(), Transform::from_translation(eye).looking_at(target, Vec3::Y)));

    // Three-point-ish lighting: a shadowed key, a soft fill from the opposite side, a back rim.
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

    // Neutral ground disk so the model casts a shadow and isn't floating in void.
    let ground = std_mats.add(StandardMaterial {
        base_color: Color::srgb(0.22, 0.24, 0.27),
        perceptual_roughness: 0.96,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(6.0))),
        MeshMaterial3d(ground),
        Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
    ));

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
            crate::player::Hero {
                pos: Vec2::ZERO,
                y: 0.0,
                facing: 0.0,
                vel: Vec2::ZERO,
                vel_y: 0.0,
                on_ground: true,
                air_takeoff_y: 0.0,
                walk_phase: 0.0,
                moving_amt: 0.0,
                run_amt: 0.0,
                moving: false,
                attacking: false,
                attack_t: 0.0,
                hit_dealt: false,
                attack_variant: 0,
                victory: false,
                charge_t: -1.0,
                heavy: false,
                dash_t: -1.0,
                dash_from: Vec2::ZERO,
                dash_to: Vec2::ZERO,
                lock_face: None,
            },
            crate::player::HeroHealth::default(),
        ))
        .id();
    let view = std::env::var("FOREST_VIEW").unwrap_or_default();
    if view.starts_with("landmark:") {
        let mesh = match view.rsplit(':').next() {
            Some("trilithon") | Some("rocky") | Some("stones") => crate::ruins::build_trilithon_mesh(),
            Some("spire") | Some("snow") | Some("ice") => crate::ruins::build_frozen_spire_mesh(),
            Some("pyramid") | Some("desert") => crate::ruins::build_sunken_pyramid_mesh(),
            _ => crate::ruins::build_trilithon_mesh(),
        };
        let white = std_mats.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.92,
            ..default()
        });
        commands.entity(root).insert((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(white),
            Transform::from_scale(Vec3::splat(1.3)),
        ));
    } else {
        spawn_model(&mut commands, root, &mut meshes, &mat);
    }
}

/// Spawn the model named by `FOREST_VIEW` under `root`. Add new models as `match` arms.
fn spawn_model(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<crate::creature::CreatureMaterial>,
) {
    let view = std::env::var("FOREST_VIEW").unwrap_or_default();
    match view.as_str() {
        // The ork on the shared studio biped skeleton (Phase 2 re-rig verification). Rest pose
        // unless `FOREST_VIEW_ANIM` is wired for bipeds; geometry/proportions read true here.
        // `FOREST_VIEW=ork:scout|berserker|shaman` picks the variant (default grunt).
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
            let h = crate::orks::ork_biped_meshes(variant, crate::orks::Faction::Red).upload(meshes);
            commands.entity(root).insert(crate::biped::BipedDrive::default());
            crate::biped::spawn_biped(commands, root, mat, h, 1.22, 1.0, 0.17, 0.38, -0.05, Some(shield_xf));
        }
        // The peasant worker types on the shared skeleton (Phase 3). `FOREST_VIEW=peasant:farmer|
        // miner|unemployed|guard` picks the type (default woodcutter).
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
            } else {
                Woodcutter
            };
            // `FOREST_VIEW=peasant:guard:desert` (any `desert` in the spec) inspects the rival's
            // desert-garbed variant (turban + cloak, sandy tunic).
            let desert = s.contains("desert");
            let tunic = if desert { 0xbf9a55 } else { 0x6a4a2a };
            let m = crate::peasant_model::peasant_biped_meshes(kind, 0xd8a06a, tunic, 0x3a2a18, false, desert);
            let h = m.upload(meshes);
            commands.entity(root).insert(crate::biped::BipedDrive::default());
            // Off-hand empty: peasants carry only their trade tool.
            crate::biped::spawn_biped(commands, root, mat, h, 1.06, 1.0, 0.15, 0.3, -0.06, None);
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
        // Isolated 1:1 transcription of the three.js previs knight (static rest pose).
        "knight2" => crate::previs_knight::spawn(commands, root, meshes, mat.clone()),
        _ => {
            // Default: the player knight in rest pose. `FOREST_EQUIP="weapon,armor"` swaps gear.
            let (weapon, armor) = parse_equip();
            let m = crate::player::model::build_knight(weapon.as_deref(), armor.as_deref());
            crate::player::spawn_hero_meshes(commands, root, m, meshes, mat);
        }
    }
}

/// `FOREST_EQUIP="weapon_id,armor_id"` → `(weapon, armor)`; either side may be empty.
fn parse_equip() -> (Option<String>, Option<String>) {
    let Ok(s) = std::env::var("FOREST_EQUIP") else { return (None, None) };
    let mut it = s.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).map(str::to_string);
    (it.next(), it.next())
}
