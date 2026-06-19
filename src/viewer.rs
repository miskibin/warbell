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
//! `FOREST_EQUIP="weapon,armor"`. New models slot into the `match` in [`spawn_model`].

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

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(window), ..default() }))
        .add_plugins(crate::creature::CreaturePlugin) // the shared CreatureMaterial + its shader
        .add_plugins(crate::capture::CapturePlugin) // FOREST_SHOT / FOREST_CLIP (+ _ORBIT turntable)
        .insert_resource(GlobalAmbientLight { brightness: 160.0, ..default() })
        .insert_resource(ClearColor(Color::srgb(0.16, 0.17, 0.20)))
        .add_systems(Startup, setup)
        .run();
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
        DirectionalLight { illuminance: 10_000.0, shadows_enabled: true, ..default() },
        Transform::from_xyz(4.0, 7.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 3_500.0, shadows_enabled: false, ..default() },
        Transform::from_xyz(-5.0, 4.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 2_500.0, shadows_enabled: false, ..default() },
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

    // The model itself, on the shared creature material.
    let mat = crate::creature::make_creature_material(&mut creature_mats);
    let root = commands.spawn((Transform::default(), Visibility::Visible)).id();
    spawn_model(&mut commands, root, &mut meshes, &mat);
}

/// Spawn the model named by `FOREST_VIEW` under `root`. Add new models as `match` arms.
fn spawn_model(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<crate::creature::CreatureMaterial>,
) {
    match std::env::var("FOREST_VIEW").unwrap_or_default().as_str() {
        // Future: "ork", "ork:berserker", "wolf", … dispatch to their builders here.
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
