//! Trailer **staged scenes** — hand-composed, looped tableaus the normal game never assembles, for
//! filming promo shots. The user flies their OWN free-cam (` toggles it); each scene only stages the
//! WORLD: it pins the time-of-day, spawns + poses actors (orks, villagers, the hero) and props at
//! fixed marks, and drives a small looped behaviour so the shot has motion. Fired from the F1 debug
//! panel's "🎬 Director → Scenes" row; one scene at a time.
//!
//! Six scenes (see [`SceneId`]):
//! * **WorkSite** — the hero stands arms-crossed, supervising, while peasants labour; one leans on a
//!   shovel. (Daylight.)
//! * **Mason** — the gag ("He laid three stones. Then he supervised."): a peasant at a half-built
//!   wall slowly sets three stones on the course, then steps back, clasps his hands behind his
//!   back, points and nods — supervising — while two labourers keep grinding beside him.
//! * **WallPatrol** — an ork archer outside the east wall looses a green bolt at the rampart on a
//!   loop, flanked by idle grunts — a ride-along-the-palisade shot.
//! * **OrksFlee** — comedic: a warband streams outward from their campfire in a panicked scatter.
//! * **NightSiege** — the hero on the north wall at night, the war bell beside him, torches lit, a
//!   horde flooding past toward the keep below.
//! * **BarrelPeek** — a peasant peeks out from behind a barrel while orks rampage through the yard.
//!
//! Scene actors carry [`SceneActor`] so the camp/wander brains leave them to this module; props carry
//! [`SceneProp`]. Both are torn down when the scene changes or clears.
//!
//! **`FarmMorning` (the Ch.2 cutscene) is a TEST-ONLY experiment.** Never to be integrated into the
//! game; reachable only via `FOREST_SCENE=farm`. Do not build campaign scenes on top of it without the
//! author's say-so.
//!
//! **`FortArrival` (screenplay P.3, "the bell in the tower") is a VIDEO rig, not a game feature:** it
//! exists only to film the cutscene as a pre-rendered clip (`FOREST_SCENE=fort` + `FOREST_CLIP`, then
//! `tools/cutscene/fort_audio.py mix`). No in-game trigger, skip or save hook — on purpose.
//!
//! **`ShipCabin` (screenplay P.0, "Below Deck", the game's first scene)** is the same kind of video
//! rig (`FOREST_SCENE=cabin`), set aboard the ship off Wreck Cove — see [`cabin`].

use bevy::prelude::*;

use crate::orks::{Faction, OrkVariant};
use crate::siege::InvaderArmory;
use crate::meshkit::{merged_flat as merged, tinted_hex as tinted};

mod cabin;
mod storm;

// ── Public tags ──────────────────────────────────────────────────────────────────────

/// An ork/villager driven by a staged scene — excluded from the camp/wander brains (see the
/// `Without<SceneActor>` filters in `orks.rs`, `villagers.rs`) so this module owns its motion.
#[derive(Component)]
pub struct SceneActor;

/// Static scenery spawned for a scene (props, lights) — despawned wholesale on teardown.
#[derive(Component)]
struct SceneProp;

// ── Scene identity + state ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneId {
    WorkSite,
    WallPatrol,
    OrksFlee,
    NightSiege,
    BarrelPeek,
    Mason,
    /// Ch.2 cutscene (campaign screenplay): dawn after night 1, a farmer mending a burnt fence.
    /// TEST-ONLY experiment. Never to be integrated into the game; reachable only via FOREST_SCENE=farm. Do not build campaign scenes on top of it without the author's say-so.
    FarmMorning,
    /// Screenplay P.3 "Fort i dzwon w wieży": Pip, Maud and the knight walk into the ruined fort at
    /// golden hour. Video rig only (`FOREST_SCENE=fort`) — never triggered by the game.
    FortArrival,
    /// Screenplay P.0 "Na pokładzie": Harrow and Wat in the stern cabin, a storm outside, three days
    /// out from Kell. Video rig only (`FOREST_SCENE=cabin`) — never triggered by the game.
    ShipCabin,
    /// Screenplay P.1 "Piorun": four pictures out of black, each lit by one lightning strike —
    /// the wreck, the orks on the beach, Harrow dead, the helmet. Video rig only (`FOREST_SCENE=storm`).
    Storm,
}

/// The scripted, letterboxed cutscenes (vs. the looped trailer tableaus).
fn is_cutscene(id: Option<SceneId>) -> bool {
    matches!(id, Some(SceneId::FarmMorning | SceneId::FortArrival | SceneId::ShipCabin | SceneId::Storm))
}

/// What the F1 panel asks for vs. what's currently staged. `clock` is seconds since the active scene
/// began (the loop phase every driven behaviour reads). A fresh scene starts the clock at
/// `-cinematic::PRE_ROLL`: actors hold their marks (drivers clamp to `t = 0`) until it crosses
/// zero, giving the user time to set the free-cam before anything moves.
#[derive(Resource, Default)]
pub struct SceneState {
    /// Panel request: `Some(id)` to stage, `None` to clear. Compared against `active` each frame.
    pub want: Option<SceneId>,
    pub active: Option<SceneId>,
    clock: f32,
    /// Where to pin the hero while the scene runs (world pos + yaw), if the scene stages him.
    hero: Option<(Vec3, f32)>,
    /// The staged hero is walking (his gait fields are written by the scene, not cleared by [`hold_scene_hero`]).
    hero_moving: bool,
}

/// Per-ork looped role within a scene.
#[derive(Clone, Copy)]
enum OrkRole {
    /// Stream outward from `origin` along `dir`, wrapping at `span` — a panicked scatter.
    Flee,
    /// March from a start point toward the keep (origin), wrapping — a siege stream.
    Horde,
    /// Stand and (for the archer) mime a draw on a cadence.
    Idle,
    Archer,
}

#[derive(Component)]
struct SceneOrk {
    role: OrkRole,
    origin: Vec2,
    dir: Vec2,
    speed: f32,
    span: f32,
    phase: f32,
}

/// A peasant that periodically swings its tool — reads as digging/hoeing.
#[derive(Component)]
struct SceneLabor {
    period: f32,
    phase: f32,
    last: f32,
}

/// The gag mason — [`drive_scene_mason`] owns his WHOLE rig (root pose + every limb), so
/// `villager_limbs` skips him (see its `Without<SceneMason>` filter). `pub` for that filter.
#[derive(Component)]
pub struct SceneMason;

/// One of the mason's three wall stones, pre-spawned at its course slot at scale ~0 and popped
/// in when its lay cycle "places" it (reset to 0 when the loop wraps).
#[derive(Component)]
struct MasonStone {
    idx: u32,
}

/// A glowing bolt that flies `from`→`to` on a loop (the ork archer's green warp-bolt).
#[derive(Component)]
struct SceneBolt {
    from: Vec3,
    to: Vec3,
    period: f32,
    phase: f32,
}

/// Shared materials/meshes for scene props + bolts (props share one white vertex-coloured material,
/// like every other prop in the game; the bolt/flame are unlit emissives that bloom).
#[derive(Resource)]
struct SceneAssets {
    prop_mat: Handle<StandardMaterial>,
    bolt_mat: Handle<StandardMaterial>,
    bolt_mesh: Handle<Mesh>,
    flame_mat: Handle<StandardMaterial>,
    flame_mesh: Handle<Mesh>,
}

pub struct ScenesPlugin;

impl Plugin for ScenesPlugin {
    fn build(&self, app: &mut App) {
        // `FOREST_SCENE=worksite|wallpatrol|orksflee|nightsiege|barrelpeek` stages a scene at boot
        // (for a screenshot/clip), same staging-hook style as the other `FOREST_*` vars.
        let mut state = SceneState::default();
        if let Ok(s) = std::env::var("FOREST_SCENE") {
            state.want = match s.trim().to_ascii_lowercase().as_str() {
                "worksite" | "work" => Some(SceneId::WorkSite),
                "wallpatrol" | "wall" => Some(SceneId::WallPatrol),
                "orksflee" | "flee" => Some(SceneId::OrksFlee),
                "nightsiege" | "siege" | "night" => Some(SceneId::NightSiege),
                "barrelpeek" | "barrel" => Some(SceneId::BarrelPeek),
                "mason" | "threestones" | "stones" => Some(SceneId::Mason),
                "farm" | "farmmorning" | "ch2" | "protection" => Some(SceneId::FarmMorning),
                "fort" | "fortarrival" | "p3" => Some(SceneId::FortArrival),
                "cabin" | "ship" | "shipcabin" | "p0" | "belowdeck" => Some(SceneId::ShipCabin),
                "storm" | "p1" | "piorun" | "lightning" => Some(SceneId::Storm),
                _ => None,
            };
        }
        app.insert_resource(state)
            .add_systems(Startup, setup_scene_assets)
            .add_systems(
                Update,
                (
                    apply_scene_change,
                    drive_scene_orks,
                    drive_scene_labor,
                    drive_scene_bolts,
                    drive_scene_mason,
                    drive_farm_scene,
                    drive_fort_scene,
                    fort_birds,
                    cabin::drive,
                    storm::drive,
                    storm::drive_fx,
                )
                    .chain(),
            )
            .add_systems(Update, (drive_farm_audio, farm_hammers, fort_mouths.after(drive_fort_scene)))
            .add_systems(Update, (cabin::mouths, cabin::seat_fit, cabin::letter_follow, cabin::crew_follow, cabin::log_heads).after(cabin::drive))
            // Pin the staged hero LAST (after locomotion/gravity have run) and just before transforms
            // propagate, so his elevated wall mark is the authoritative pose regardless of PlayMode.
            .add_systems(
                PostUpdate,
                (hold_scene_hero, farm_camera, farm_hide_hud, farm_clear_stage, fort_clear_stage)
                    .before(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            )
            // The lens FOV must land before Bevy builds the projection matrix: unordered, the
            // executor sometimes ran `camera_system` first and that frame rendered with the
            // gameplay follow-cam's FOV — a zoom-pop every few frames in the captured film.
            .configure_sets(PostUpdate, bevy::camera::CameraUpdateSystems.after(farm_camera))
            // Scripted head turns ride on top of the freshly sampled clip pose.
            .add_systems(
                PostUpdate,
                (fort_head_look, fort_arm_point)
                    .chain()
                    .after(bevy::app::AnimationSystems)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

fn setup_scene_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let prop_mat =
        materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    // Sickly-green warp glow (matches the fortress towers' bolts), unlit so it blooms.
    let bolt_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.78, 0.5), // desaturated off the old neon green
        emissive: LinearRgba::rgb(0.6, 3.4, 0.7),
        unlit: true,
        ..default()
    });
    let bolt_mesh = meshes.add(Sphere::new(0.18).mesh().ico(2).unwrap());
    let flame_mat = crate::flame_fx::flame_card_material(&mut materials);
    let flame_mesh = meshes.add(crate::flame_fx::flame_card_mesh(0.34));
    commands.insert_resource(SceneAssets { prop_mat, bolt_mat, bolt_mesh, flame_mat, flame_mesh });
}

// ── Scene change: teardown + setup ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply_scene_change(
    time: Res<Time>,
    mut state: ResMut<SceneState>,
    assets: Res<SceneAssets>,
    armory: Option<Res<InvaderArmory>>,
    mut director: ResMut<crate::cinematic::DirectorState>,
    mut clock: ResMut<crate::scene::SkyClock>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    (mut images, mut std_mats): (ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>),
    old: Query<Entity, Or<(With<SceneActor>, With<SceneProp>)>>,
    prog: Option<Res<crate::capture::ClipProgress>>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    fonts: Option<Res<crate::ui::fonts::UiFonts>>,
    server: Res<AssetServer>,
    mut rehearsed: Local<bool>,
) {
    if state.want == state.active {
        // A clip's warm-up frames don't count: the scene starts when recording does.
        // `FOREST_SCENE_T` freezes the clock at that instant (a still of one moment of the cutscene).
        let still = is_cutscene(state.active) && std::env::var_os("FOREST_SCENE_T").is_some();
        // Look-dev contact sheet: `FOREST_SCENE_SHEET="t1,t2,…"` under `FOREST_CLIP` saves one frame
        // at each listed scene second (the clock jumps per saved frame), every shot in one boot.
        // A clip's warm-up rehearses the film: the clock flicks through every shot so each actor,
        // prop and light set is drawn once and its pipelines are compiled before recording starts
        // (first sight of a skinned look compiles for seconds, and it showed as people missing, or
        // only their faces and hands, in the first frames of a shot). Back to 0 when it starts.
        let warming = prog.as_ref().is_some_and(|p| !p.recording);
        if let (true, Some(r)) = (warming, rehearsal(state.active)) {
            state.clock = r[(time.elapsed_secs() * 1.5) as usize % r.len()];
            *rehearsed = true;
            return;
        }
        if std::mem::take(&mut *rehearsed) && sheet_times().is_none() {
            // `FOREST_SCENE_FROM=<s>`: the recording starts at that second of the film (iterate on
            // one beat in motion without filming everything before it)
            state.clock = std::env::var("FOREST_SCENE_FROM").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
        }
        if let (true, Some(times)) = (is_cutscene(state.active), sheet_times()) {
            // `FOREST_SCENE_SHEET_HOLD=n` holds each time n frames (keep every n-th): skins posed,
            // the camera's cut settled.
            let hold = std::env::var("FOREST_SCENE_SHEET_HOLD").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
            let k = prog.as_ref().map_or(0, |p| p.frame as usize) / hold;
            state.clock = times[k.min(times.len() - 1)];
            return;
        }
        if !still && !prog.is_some_and(|p| !p.recording) {
            state.clock += time.delta_secs();
        }
        return;
    }
    // Tear down the previous scene's actors + props.
    for e in &old {
        commands.entity(e).try_despawn();
    }
    state.clock = 0.0;
    state.hero = None;
    state.hero_moving = false;
    subs.release();
    director.gesture = None;
    director.hide_weapon = false;

    let Some(id) = state.want else {
        // Cleared: hand the sky back to the normal cycle.
        state.active = None;
        clock.paused = false;
        return;
    };
    let Some(armory) = armory.as_ref() else { return };
    let arm = &armory.0;

    match id {
        SceneId::WorkSite => {
            pin_sky(&mut clock, 0.25); // midday
            // Hero supervising, arms folded (keeps his sword — a captain's idle).
            state.hero = Some((world_y(2.0, 5.0), std::f32::consts::PI)); // facing +Z toward the gang
            director.gesture = Some(crate::cinematic::HeroGesture::ArmsCrossed);
            // Three farmhands hauling/hoeing, facing the camera side.
            for (i, (x, z)) in [(0.0, 8.0), (2.2, 8.7), (4.2, 8.2)].into_iter().enumerate() {
                let e = crate::villagers::spawn_scene_peasant(
                    &mut commands, &mut meshes, &mut creature_mats,
                    Vec2::new(x, z), 0.0, Some(crate::villagers::Trade::Farmer), 31 + i as u32 * 17,
                );
                commands.entity(e).insert(SceneLabor { period: 1.6, phase: i as f32 * 0.37, last: -1.0 });
            }
            // A fourth peasant loafing, leaning by a planted shovel.
            crate::villagers::spawn_scene_peasant(
                &mut commands, &mut meshes, &mut creature_mats, Vec2::new(-2.0, 6.6), 0.5, None, 7,
            );
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, shovel_mesh(), Vec3::new(-1.4, 0.0, 6.7), 0.4, 0.55);
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, stone_pile_mesh(), Vec3::new(1.2, 0.0, 9.6), 0.0, 1.0);
        }
        SceneId::WallPatrol => {
            pin_sky(&mut clock, 0.46); // dusk
            // An ork archer just outside the east wall, drawing on a loop, with two idle grunts.
            spawn_scene_ork(&mut commands, arm, OrkVariant::Scout, Vec2::new(24.0, 2.0), -std::f32::consts::FRAC_PI_2,
                SceneOrk { role: OrkRole::Archer, origin: Vec2::new(24.0, 2.0), dir: Vec2::NEG_X, speed: 0.0, span: 0.0, phase: 0.0 });
            spawn_scene_ork(&mut commands, arm, OrkVariant::Grunt, Vec2::new(25.5, 5.0), -std::f32::consts::FRAC_PI_2,
                SceneOrk { role: OrkRole::Idle, origin: Vec2::new(25.5, 5.0), dir: Vec2::NEG_X, speed: 0.0, span: 0.0, phase: 0.0 });
            spawn_scene_ork(&mut commands, arm, OrkVariant::Berserker, Vec2::new(23.5, -2.2), -std::f32::consts::FRAC_PI_2,
                SceneOrk { role: OrkRole::Idle, origin: Vec2::new(23.5, -2.2), dir: Vec2::NEG_X, speed: 0.0, span: 0.0, phase: 0.0 });
            // Green bolt streaking from the archer at the rampart, looping.
            commands.spawn((
                Mesh3d(assets.bolt_mesh.clone()),
                MeshMaterial3d(assets.bolt_mat.clone()),
                Transform::from_translation(Vec3::new(24.0, 2.6, 2.0)),
                SceneBolt { from: Vec3::new(24.0, 2.6, 2.0), to: Vec3::new(17.5, 1.7, -2.5), period: 1.3, phase: 0.0 },
                SceneActor,
            ));
        }
        SceneId::OrksFlee => {
            pin_sky(&mut clock, 0.30);
            let c = nearest_camp().unwrap_or(Vec2::new(-32.0, 2.0));
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, campfire_mesh(), Vec3::new(c.x, world_y_at(c), c.y), 0.0, 1.0);
            // Eight orks bolting outward in a fan, each on its own loop phase — chaos.
            for i in 0..8u32 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU + 0.3;
                let dir = Vec2::new(a.cos(), a.sin());
                spawn_scene_ork(&mut commands, arm, variant_cycle(i), c + dir * 1.2, a,
                    SceneOrk { role: OrkRole::Flee, origin: c, dir, speed: 4.5 + (i % 3) as f32 * 0.8, span: 16.0, phase: i as f32 / 8.0 });
            }
            // A couple of knocked-over barrels for the gag.
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, barrel_mesh(), Vec3::new(c.x + 2.0, world_y_at(c) + 0.3, c.y + 1.0), 1.3, 1.0);
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, barrel_mesh(), Vec3::new(c.x - 1.5, world_y_at(c) + 0.3, c.y - 1.8), 2.6, 1.0);
        }
        SceneId::NightSiege => {
            pin_sky(&mut clock, 0.82); // deep night
            const WALL_TOP: f32 = 1.05; // north rampart walkway top (WALL_H·0.78)
            const WALL_Z: f32 = -12.0; // wall centre line
            // Hero up on the north rampart, war bell beside him, facing out over the field.
            state.hero = Some((Vec3::new(5.0, WALL_TOP, WALL_Z), 0.0)); // facing -Z (outward)
            director.gesture = Some(crate::cinematic::HeroGesture::Salute);
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, bell_mesh(), Vec3::new(3.0, WALL_TOP, WALL_Z), 0.0, 1.0);
            // Torches strung along the wall walk.
            for x in [-9.0, -3.0, 9.0] {
                spawn_torch(&mut commands, &mut meshes, &assets, Vec3::new(x, WALL_TOP, WALL_Z));
            }
            // A horde flooding from the north toward the keep, funnelled near the gate (x≈0), looping.
            for i in 0..16u32 {
                let lane = (i % 5) as f32 - 2.0;
                let start = Vec2::new(lane * 1.6, -26.0 - (i / 5) as f32 * 3.0);
                let dir = (Vec2::ZERO - start).normalize_or_zero();
                spawn_scene_ork(&mut commands, arm, variant_cycle(i), start, dir.x.atan2(dir.y),
                    SceneOrk { role: OrkRole::Horde, origin: start, dir, speed: 3.2, span: start.length(), phase: i as f32 / 16.0 });
            }
        }
        SceneId::Mason => {
            pin_sky(&mut clock, 0.25); // midday
            // The mason himself — a plain peasant at his mark before the half-built wall.
            let e = crate::villagers::spawn_scene_peasant(
                &mut commands, &mut meshes, &mut creature_mats, MASON_MARK, 0.0, None, 11,
            );
            commands.entity(e).insert(SceneMason);
            // The half-built wall he's "working" on (gap in the top course for his three stones),
            // plus the stone pile he draws from.
            let wy = world_y_at(Vec2::new(MASON_MARK.x, MASON_WALL_Z));
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, wall_course_mesh(), Vec3::new(MASON_MARK.x, wy, MASON_WALL_Z), 0.0, 1.0);
            let pile = MASON_MARK + Vec2::new(-1.1, -0.4);
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, stone_pile_mesh(), Vec3::new(pile.x, world_y_at(pile), pile.y), 0.6, 0.8);
            // His three stones: pre-spawned at their course slots, popped in as each is "placed".
            let stone = meshes.add(mason_stone_mesh());
            for (i, dx) in MASON_STONE_X.into_iter().enumerate() {
                commands.spawn((
                    Mesh3d(stone.clone()),
                    MeshMaterial3d(assets.prop_mat.clone()),
                    Transform {
                        translation: Vec3::new(MASON_MARK.x + dx, wy + 0.40, MASON_WALL_Z),
                        rotation: Quat::from_rotation_y(i as f32 * 0.13 - 0.13), // slightly askew — hand-laid
                        scale: Vec3::splat(0.001),
                    },
                    MasonStone { idx: i as u32 },
                    SceneProp,
                ));
            }
            // Two labourers grinding at the wall's flanks — the ones he ends up "supervising".
            for (i, dx) in [-1.05f32, 1.05].into_iter().enumerate() {
                let e = crate::villagers::spawn_scene_peasant(
                    &mut commands, &mut meshes, &mut creature_mats,
                    MASON_MARK + Vec2::new(dx, 0.1), 0.0, Some(crate::villagers::Trade::Miner), 41 + i as u32 * 13,
                );
                commands.entity(e).insert(SceneLabor { period: 1.5, phase: i as f32 * 0.43, last: -1.0 });
            }
        }
        SceneId::FarmMorning => {
            pin_sky(&mut clock, 0.17); // low golden sun
            setup_farm_morning(&mut commands, &mut meshes, &mut creature_mats, &assets);
        }
        SceneId::FortArrival => {
            pin_sky(&mut clock, FORT_SKY);
            setup_fort_arrival(&mut commands, &mut meshes, &mut creature_mats, &assets, fonts.as_deref());
        }
        SceneId::ShipCabin => {
            pin_sky(&mut clock, cabin::SKY);
            cabin::setup(&mut commands, &mut meshes, &mut images, &mut creature_mats, &mut std_mats, &assets, fonts.as_deref(), &server);
        }
        SceneId::Storm => {
            pin_sky(&mut clock, if storm::day_dev() { 0.3 } else { storm::SKY });
            storm::setup(&mut commands, &mut meshes, &mut images, &mut creature_mats, &mut std_mats, &assets, arm);
        }
        SceneId::BarrelPeek => {
            pin_sky(&mut clock, 0.82);
            // A peasant peeking out from behind a barrel in the yard.
            spawn_prop_mesh(&mut commands, &mut meshes, &assets, barrel_mesh(), Vec3::new(6.0, world_y(6.0, 2.0).y, 2.0), 0.0, 1.0);
            crate::villagers::spawn_scene_peasant(
                &mut commands, &mut meshes, &mut creature_mats, Vec2::new(6.0, 2.7), std::f32::consts::PI, None, 5,
            );
            spawn_torch(&mut commands, &mut meshes, &assets, Vec3::new(8.5, world_y(8.5, 1.0).y, 1.0));
            // Three orks rampaging across the yard toward the keep, looping.
            for i in 0..3u32 {
                let start = Vec2::new(-12.0 + i as f32 * 2.0, -7.0 - i as f32 * 1.5);
                let dir = (Vec2::ZERO - start).normalize_or_zero();
                spawn_scene_ork(&mut commands, arm, variant_cycle(i + 1), start, dir.x.atan2(dir.y),
                    SceneOrk { role: OrkRole::Horde, origin: start, dir, speed: 3.6, span: start.length(), phase: i as f32 / 3.0 });
            }
        }
    }
    // Loop scenes get a camera-setting grace; the cutscene starts at once (`FOREST_SCENE_T=<s>` skips ahead for a still).
    state.clock = if is_cutscene(Some(id)) {
        std::env::var("FOREST_SCENE_T").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
    } else {
        -crate::cinematic::PRE_ROLL
    };
    state.active = Some(id);
}

/// Look-dev: the storm's haze against its dome (`FOREST_STORM_FOG`, x the dome's horizon colour).
fn fog_gain() -> f32 {
    static V: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("FOREST_STORM_FOG").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0))
}

/// The moments a cutscene's warm-up flicks through (one per shot), if it has any.
fn rehearsal(id: Option<SceneId>) -> Option<&'static [f32]> {
    match id {
        Some(SceneId::Storm) => Some(storm::REHEARSAL),
        _ => None,
    }
}

fn sheet_times() -> Option<&'static [f32]> {
    static T: std::sync::OnceLock<Option<Vec<f32>>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let v: Vec<f32> = std::env::var("FOREST_SCENE_SHEET").ok()?.split(',').filter_map(|n| n.trim().parse().ok()).collect();
        (!v.is_empty()).then_some(v)
    })
    .as_deref()
}

// ── Per-frame drives ───────────────────────────────────────────────────────────────────

fn drive_scene_orks(
    time: Res<Time>,
    state: Res<SceneState>,
    mut q: Query<(&mut crate::orks::Ork, &mut Transform, &SceneOrk), With<SceneActor>>,
) {
    if state.active.is_none() {
        return;
    }
    let t = state.clock.max(0.0); // clamp: actors hold their marks through the pre-roll
    let rnow = time.elapsed_secs();
    for (mut o, mut tf, s) in &mut q {
        let pos = match s.role {
            OrkRole::Flee => {
                let d = (t * s.speed + s.phase * s.span).rem_euclid(s.span);
                o.facing = s.dir.x.atan2(s.dir.y);
                o.moving = true;
                s.origin + s.dir * d
            }
            OrkRole::Horde => {
                let len = s.span.max(0.01);
                let d = (t * s.speed + s.phase * len).rem_euclid(len);
                let dir = (Vec2::ZERO - s.origin).normalize_or_zero();
                o.facing = dir.x.atan2(dir.y);
                o.moving = true;
                s.origin + dir * d
            }
            OrkRole::Idle => {
                o.moving = false;
                s.origin
            }
            OrkRole::Archer => {
                o.moving = false;
                o.facing = s.dir.x.atan2(s.dir.y);
                // Mime a draw/loose every ~1.3s (drives the club/staff swing animation). `atk_anim`
                // is a timestamp in the `elapsed_secs` domain the limb animator reads. The clamped
                // `t` sits at 0 all through the pre-roll — don't re-stamp every held frame.
                if state.clock >= 0.0 && (t % 1.3) < time.delta_secs() {
                    o.atk_anim = rnow;
                }
                s.origin
            }
        };
        // `OrkMotion::track` paces the legs from `o.pos`: left at the spawn mark it read a
        // standstill, so a charging horde shuffled its feet while gliding across the field.
        o.pos = pos;
        let gy = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(tf.translation.y);
        tf.translation = Vec3::new(pos.x, gy, pos.y);
        tf.rotation = Quat::from_rotation_y(o.facing);
    }
}

fn drive_scene_labor(
    time: Res<Time>,
    state: Res<SceneState>,
    mut q: Query<(&mut crate::villagers::Villager, &mut SceneLabor)>,
) {
    if state.active.is_none() || state.clock < 0.0 {
        return; // pre-roll: no tool swings (and no stray cycle-0 stamp) until the grace ends
    }
    let t = state.clock;
    for (mut v, mut l) in &mut q {
        let cyc = ((t / l.period) + l.phase).floor();
        if cyc != l.last {
            l.last = cyc;
            // Stamp the swing in the `elapsed_secs` domain `villager_limbs` compares against.
            v.atk_anim = time.elapsed_secs();
        }
    }
}

// ── The mason gag loop ───────────────────────────────────────────────────────────────

/// Where the mason stands (world XZ), facing +Z toward his wall — the open courtyard west of the
/// keep, clear of the market/well gather knots so wanderers don't photobomb the shot.
const MASON_MARK: Vec2 = Vec2::new(-5.5, 7.2);
const MASON_WALL_Z: f32 = MASON_MARK.y + 0.75;
/// One lay cycle: bow to the ground, heave the stone up, set it on the course.
const MASON_LAY_DUR: f32 = 2.6;
const MASON_LAY_TOTAL: f32 = 3.0 * MASON_LAY_DUR;
/// The supervising beat: step back, hands clasped behind, a long point, satisfied nods.
const MASON_SUP_DUR: f32 = 6.0;
const MASON_LOOP: f32 = MASON_LAY_TOTAL + MASON_SUP_DUR;
/// When within a lay cycle the stone lands on the wall (fraction of [`MASON_LAY_DUR`]).
const MASON_PLACE_AT: f32 = 0.80;
/// The three course slots (local X along the wall) his stones fill, in lay order.
const MASON_STONE_X: [f32; 3] = [-0.36, 0.0, 0.36];

/// Piecewise keyframe envelope: smoothstep between consecutive `(x, value)` keys (monotonic x),
/// clamped to the end values outside the key range. The whole mason mime is built from these.
fn env(x: f32, keys: &[(f32, f32)]) -> f32 {
    if x <= keys[0].0 {
        return keys[0].1;
    }
    for w in keys.windows(2) {
        let ((x0, v0), (x1, v1)) = (w[0], w[1]);
        if x < x1 {
            let u = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
            return v0 + (v1 - v0) * (u * u * (3.0 - 2.0 * u));
        }
    }
    keys[keys.len() - 1].1
}

/// "He laid three stones. Then he supervised." — the looped pantomime. This system owns the
/// mason's whole rig: root pose (bow pitch + crouch + the step back), every limb (arms reach /
/// clasp behind the back / point; head looks at the work and nods), and the three stones popping
/// onto the course as each is placed. `villager_limbs` skips him entirely.
#[allow(clippy::type_complexity)]
fn drive_scene_mason(
    state: Res<SceneState>,
    mut roots: Query<
        (&mut crate::villagers::Villager, &mut Transform, &Children),
        (With<SceneMason>, Without<crate::villagers::VilPart>, Without<MasonStone>),
    >,
    mut parts: Query<
        (&crate::villagers::VilPart, &mut Transform),
        (Without<SceneMason>, Without<MasonStone>),
    >,
    mut stones: Query<
        (&MasonStone, &mut Transform),
        (Without<SceneMason>, Without<crate::villagers::VilPart>),
    >,
) {
    if state.active != Some(SceneId::Mason) {
        return;
    }
    // Clamp: he stands at his mark (rest pose, stones hidden) through the camera-setting pre-roll.
    let t = state.clock.max(0.0).rem_euclid(MASON_LOOP);

    // Pop each placed stone in (ease-out); the wrap resets all three for the next loop.
    for (s, mut tf) in &mut stones {
        let placed = (s.idx as f32 + MASON_PLACE_AT) * MASON_LAY_DUR;
        let k = if t >= placed {
            let u = ((t - placed) / 0.3).min(1.0);
            1.0 - (1.0 - u) * (1.0 - u)
        } else {
            0.0
        };
        tf.scale = Vec3::splat(k.max(0.001));
    }

    // Pose channels for this loop instant: torso pitch (+ = bow forward), hip drop, per-arm and
    // head rotations, how far he's stepped back off his mark, and the leg-shuffle swing.
    let (pitch, crouch, arm_l, arm_r, head_pitch, head_yaw, back, leg) = if t < MASON_LAY_TOTAL {
        // ── Laying: bow down, hold (he's not hurrying), heave up, set the stone, straighten. ──
        let u = (t % MASON_LAY_DUR) / MASON_LAY_DUR;
        let pitch = env(u, &[(0.0, 0.0), (0.26, 0.85), (0.44, 0.85), (0.60, 0.14), (0.78, 0.42), (0.92, 0.42), (1.0, 0.02)]);
        let crouch = 0.20 * env(u, &[(0.0, 0.0), (0.26, 1.0), (0.46, 1.0), (0.62, 0.0), (1.0, 0.0)]);
        // Both arms together — a two-handed carry, so the stone reads heavy.
        let arm = env(u, &[(0.0, -0.05), (0.26, -0.55), (0.44, -0.6), (0.60, -1.0), (0.78, -1.25), (0.92, -1.25), (1.0, -0.1)]);
        let head_pitch = env(u, &[(0.0, 0.05), (0.26, 0.32), (0.60, 0.18), (0.80, 0.35), (1.0, 0.08)]);
        (pitch, crouch, arm, arm, head_pitch, 0.0, 0.0, 0.02 * (t * 1.3).sin())
    } else {
        // ── Supervising: step back, chest out, hands clasped behind, point at the work with
        // emphatic nods, one satisfied double-nod, then step back up to the wall for the wrap. ──
        let s = t - MASON_LAY_TOTAL;
        let back = 0.85 * env(s, &[(0.0, 0.0), (0.7, 1.0), (MASON_SUP_DUR - 0.7, 1.0), (MASON_SUP_DUR, 0.0)]);
        let lean = env(s, &[(0.0, 0.0), (0.9, -0.07), (MASON_SUP_DUR - 0.8, -0.07), (MASON_SUP_DUR, 0.0)]);
        let clasp = env(s, &[(0.0, 0.0), (0.9, 0.5), (MASON_SUP_DUR - 0.6, 0.5), (MASON_SUP_DUR, 0.0)]);
        let point = env(s, &[(2.0, 0.0), (2.5, 1.0), (3.8, 1.0), (4.3, 0.0)]);
        let arm_r = clasp + (-1.35 - clasp) * point; // the point overrides the clasped right arm
        let nod = -0.12 * (s * 5.0).sin() * point
            + env(s, &[(4.5, 0.0), (4.7, 0.25), (4.9, 0.02), (5.1, 0.2), (5.3, 0.0)]);
        // Scan the work side to side; while pointing, fix on the right-hand labourer.
        let head_yaw = 0.22 * (s * 0.9).sin() * (1.0 - point) + 0.3 * point;
        let stepping = !(0.7..=MASON_SUP_DUR - 0.7).contains(&s);
        let leg = if stepping { 0.35 * (s * 11.0).sin() } else { 0.02 * (s * 1.1).sin() };
        (lean, 0.0, clasp, arm_r, nod, head_yaw, back, leg)
    };

    for (mut v, mut tf, children) in &mut roots {
        let facing = 0.0; // toward the wall (+Z)
        let pos = Vec2::new(MASON_MARK.x, MASON_MARK.y - back);
        let gy = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(tf.translation.y);
        v.pos = pos;
        v.facing = facing;
        v.moving = false;
        tf.translation = Vec3::new(pos.x, gy - crouch, pos.y);
        tf.rotation = Quat::from_rotation_y(facing) * Quat::from_rotation_x(pitch);
        for &child in children {
            let Ok((part, mut ptf)) = parts.get_mut(child) else { continue };
            use crate::critters::PartKind;
            ptf.rotation = match part.kind {
                PartKind::Leg(sign) => Quat::from_rotation_x(sign * leg),
                PartKind::Arm(sign) => Quat::from_rotation_x(if sign > 0.0 { arm_r } else { arm_l }),
                PartKind::Head => Quat::from_rotation_y(head_yaw) * Quat::from_rotation_x(head_pitch),
                PartKind::Tail => Quat::IDENTITY,
            };
        }
    }
}

fn drive_scene_bolts(state: Res<SceneState>, mut q: Query<(&mut Transform, &SceneBolt)>) {
    if state.active.is_none() {
        return;
    }
    let t = state.clock.max(0.0); // parked at its phase point through the pre-roll
    for (mut tf, b) in &mut q {
        let u = ((t / b.period) + b.phase).rem_euclid(1.0);
        tf.translation = b.from.lerp(b.to, u);
    }
}

/// Pin the staged hero at his mark each frame (works in free-cam, the filming mode, where the
/// locomotion system holds him at `Hero.pos`/`.y` without gravity).
fn hold_scene_hero(
    state: Res<SceneState>,
    mut hero_q: Query<(&mut crate::player::Hero, &mut Transform)>,
) {
    let Some((p, yaw)) = state.hero else { return };
    let Ok((mut hero, mut tf)) = hero_q.single_mut() else { return };
    hero.pos = Vec2::new(p.x, p.z);
    hero.y = p.y;
    hero.facing = yaw;
    hero.moving = state.hero_moving;
    tf.translation = p;
    tf.rotation = Quat::from_rotation_y(yaw);
}

// ── Ch.2 cutscene: "Bless you for the protection" ────────────────────────────────────
//
// Campaign screenplay Ch.2 (`docs/superpowers/specs/2026-10-13-kampania-scenariusz-design.md`): the
// dawn after night 1. Farmers mend a burnt fence, one hauls planks over, the knight walks up the road,
// the exchange, and the joke lands. ~30 s, every clip from the UAL pool.
// Pure scene-director: the same `SceneState` clock drives actors, camera cuts, captions, audio cues,
// letterbox + fades, and the scene clears itself (`want = None`) at the end. `FOREST_SCENE=farm`
// stages it (`FOREST_SCENE_T=<s>` freezes the clock there for a still, silent; under `FOREST_CLIP`
// it starts with recording). Audio cues live in `assets/cutscene/farm_morning.cues`
// (written by `tools/cutscene/farm_audio.py`, which also mixes the video's soundtrack).
// TEST-ONLY experiment — never integrated into the game (no in-game trigger; `FOREST_SCENE=farm` only).

/// The fence line's centre (world XZ): open grass on the road out to the south-west farms.
const FARM: Vec2 = Vec2::new(-36.0, 34.0);
/// Where the kneeling farmer works — just on the +Z side of the fence gap, facing the fence (−Z).
const FARM_KNEEL: Vec2 = Vec2::new(FARM.x, FARM.y + 0.55);
const FARM_END: f32 = 29.5;
/// The knight's walk-in: down the road from the east to a conversational stop beside the farmer.
const WAT_FROM: Vec2 = Vec2::new(FARM.x + 11.5, FARM.y + 1.2);
const WAT_TO: Vec2 = Vec2::new(FARM.x + 2.0, FARM.y + 0.95);
/// The plank-hauler: along a lane behind the fence, then in to the fence beside the gap.
const HAUL_Z: f32 = FARM.y + 3.3;
const HAUL_SPEED: f32 = 1.2; // the carry-walk clip is paced to it (npc_skin::Skinned::loco_native), feet planted
const HAUL_X0: f32 = FARM.x + 8.0;
const HAUL_TURN_X: f32 = FARM.x - 2.6;
const HAUL_SPOT: Vec2 = Vec2::new(HAUL_TURN_X, FARM.y + 1.1);

/// Timed captions: (start s, duration s, speaker, line). Texts verbatim from the screenplay; the
/// farmer's first line is the existing villager VO clip `screaming` (`audio/lines.rs`, 6.2 s).
const FARM_LINES: [(f32, f32, &str, &str); 4] = [
    (9.4, 6.2, "Farmer", "Bless you for the protection. The screaming at night is a lovely touch."),
    (16.1, 1.8, "Wat", "You're welcome."),
    (18.8, 2.0, "Farmer", "Wasn't thanking you."),
    (22.4, 1.6, "Wat", "I know."),
];

#[derive(Component, Clone, Copy, PartialEq)]
enum FarmRole {
    Kneeler,
    Hauler,
}

/// A background villager that holds one mark all scene long.
#[derive(Component)]
struct FarmFixed {
    pos: Vec2,
    facing: f32,
}

/// The plank bundle the hauler carries (hidden once he sets it down).
#[derive(Component)]
struct FarmBundle;

/// A mending hammer in a worker's hand (shown only while they hammer).
#[derive(Component)]
struct FarmHammer(Entity);

/// Full-screen black overlay for the open/close fades.
#[derive(Component)]
struct FarmFade;

fn setup_farm_morning(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    assets: &SceneAssets,
) {
    let a = FARM;
    // The fence: three bays each side of a gap; the right-hand ones scorched.
    for dx in [-6.0f32, -4.0, -2.0, 2.0, 4.0, 6.0] {
        let p = Vec2::new(a.x + dx, a.y);
        spawn_prop_mesh(commands, meshes, assets, fence_section_mesh(dx > 3.0), Vec3::new(p.x, world_y_at(p), p.y), 0.0, 1.0);
    }
    // Torn-out rail: one end still nailed to the post, hanging; a spare lying in the grass.
    let hinge = Vec3::new(a.x - 1.0, world_y_at(Vec2::new(a.x - 1.0, a.y)) + 0.72, a.y);
    commands.spawn((
        Mesh3d(meshes.add(cube(1.0, 0.07, 0.05, Vec3::new(0.5, 0.0, 0.0), WOOD))),
        MeshMaterial3d(assets.prop_mat.clone()),
        Transform::from_translation(hinge).with_rotation(Quat::from_rotation_z(-0.6)),
        SceneProp,
    ));
    let spare = Vec2::new(a.x + 0.6, a.y - 0.5);
    spawn_prop_mesh(commands, meshes, assets, cube(1.4, 0.06, 0.12, Vec3::new(0.0, 0.03, 0.0), WOOD), Vec3::new(spare.x, world_y_at(spare), spare.y), 0.5, 1.0);
    // Planks stacked beside the work.
    let pile = Vec2::new(a.x - 1.5, a.y + 0.45);
    spawn_prop_mesh(commands, meshes, assets, plank_pile_mesh(), Vec3::new(pile.x, world_y_at(pile), pile.y), 0.2, 1.0);
    // The burnt field behind them: charred ground, blackened stakes, a few smouldering embers.
    for (dx, dz, r) in [(-3.0f32, 6.5f32, 2.6f32), (1.5, 8.0, 3.2), (5.5, 6.0, 2.2), (-6.5, 9.5, 2.8), (9.0, 9.0, 2.4)] {
        let p = Vec2::new(a.x + dx, a.y + dz);
        spawn_prop_mesh(commands, meshes, assets, scorch_patch_mesh(), Vec3::new(p.x, world_y_at(p) + 0.02, p.y), dx, r);
    }
    for (dx, dz) in [(-2.0f32, 5.6f32), (0.2, 7.4), (2.8, 6.8), (4.4, 8.6), (-4.6, 8.2), (-0.8, 9.6)] {
        let p = Vec2::new(a.x + dx, a.y + dz);
        spawn_prop_mesh(commands, meshes, assets, char_stake_mesh(), Vec3::new(p.x, world_y_at(p), p.y), dx * 1.7, 0.7 + (dz % 1.0) * 0.5);
    }
    for (dx, dz) in [(-1.4f32, 6.2f32), (3.4, 7.6), (-5.2, 9.0)] {
        let p = Vec2::new(a.x + dx, a.y + dz);
        commands.spawn((
            Mesh3d(assets.flame_mesh.clone()),
            MeshMaterial3d(assets.flame_mat.clone()),
            Transform::from_translation(Vec3::new(p.x, world_y_at(p) + 0.18, p.y)).with_scale(Vec3::splat(0.55)),
            bevy::light::NotShadowCaster,
            crate::camps::Flicker { phase: p.x * 1.7 },
            SceneProp,
        ));
    }

    // Warm fill from the camera side: the low sun is behind the work, which left faces as silhouettes.
    for (x, y, z, lum) in [(-0.6f32, 2.0f32, -3.0f32, 60_000.0f32), (2.4, 2.2, -3.0, 45_000.0)] {
        commands.spawn((
            PointLight { color: Color::srgb(1.0, 0.88, 0.74), intensity: lum, range: 9.0, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(Vec3::new(a.x + x, world_y_at(a) + y, a.y + z)),
            SceneProp,
        ));
    }

    // The kneeling farmer (never looks up) and the plank-hauler.
    let farmer = crate::villagers::spawn_scene_peasant(
        commands, meshes, creature_mats, FARM_KNEEL, std::f32::consts::PI, Some(crate::villagers::Trade::Farmer), 23,
    );
    commands.entity(farmer).insert((FarmRole::Kneeler, crate::npc_skin::SkinClip("Fixing_Kneeling")));
    let hauler = crate::villagers::spawn_scene_peasant(
        commands, meshes, creature_mats, Vec2::new(HAUL_X0, HAUL_Z), -std::f32::consts::FRAC_PI_2, None, 13,
    );
    let bundle = meshes.add(plank_bundle_mesh());
    commands.entity(hauler).insert((FarmRole::Hauler, crate::npc_skin::SkinClip("Walk_Carry_Loop"))).with_children(|p| {
        // The load: glued between the carrier's real hands every frame (`npc_skin::carry_loads`).
        p.spawn((
            Mesh3d(bundle),
            MeshMaterial3d(assets.prop_mat.clone()),
            Transform::from_xyz(0.0, 1.0, 0.5),
            crate::npc_skin::CarriedLoad { across: false },
            FarmBundle,
        ));
    });
    // Life in the background: someone raking the ash, someone watching from the road with folded arms.
    for (pos, facing, clip, seed) in [
        (Vec2::new(a.x + 4.0, a.y + 9.5), 0.6f32, "Farm_Harvest", 41u32),
        (Vec2::new(a.x - 8.0, a.y + 7.0), -1.0, "Idle_FoldArms_Loop", 67),
    ] {
        let e = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, pos, facing, None, seed);
        commands.entity(e).insert((FarmFixed { pos, facing }, crate::npc_skin::SkinClip(clip)));
    }

    // Letterbox (2.39:1 on a 16:9 frame) + fade-from-black.
    for top in [true, false] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(12.8),
                top: if top { Val::Px(0.0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { Val::Px(0.0) },
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(35),
            SceneProp,
        ));
    }
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(38),
        FarmFade,
        SceneProp,
    ));
}

/// The knight: down the road to a conversational stop beside the farmer, then he stays and listens.
/// (position, facing, striding?) at scene time `t`.
fn farm_wat(t: f32) -> (Vec2, f32, bool) {
    let speed = crate::player::SPEED;
    let yaw = |v: Vec2| v.x.atan2(v.y);
    let out = WAT_TO - WAT_FROM; // walking in
    let len = out.length();
    let arrive = len / speed;
    let (walk_in, face_farmer) = (yaw(out), yaw(FARM_KNEEL - WAT_TO));
    if t < arrive {
        return (WAT_FROM + out * (t * speed / len), walk_in, t > 0.0);
    }
    // swing round to the farmer as he stops
    let k = env(t, &[(arrive, 0.0), (arrive + 0.6, 1.0)]);
    (WAT_TO, walk_in + (face_farmer - walk_in) * k, false)
}

/// The kneeler's work, varied: hammering, a pause to reach for a plank, a crouch to listen, and a
/// brief stand to answer — never looking up at the hero until the very last line.
fn farmer_clip(t: f32) -> &'static str {
    match t {
        t if (3.4..5.9).contains(&t) => "PickUp_Table",
        t if (16.0..18.5).contains(&t) => "Crouch_Idle_Loop",
        t if (18.5..21.2).contains(&t) => "Idle_Talking_Loop",
        t if (25.0..27.4).contains(&t) => "PickUp_Table",
        _ => "Fixing_Kneeling",
    }
}

/// The hauler: carry the planks along the lane, turn in to the fence, set them down, then help mend.
/// (position, facing, still walking?, clip) at time `t`.
fn farm_hauler(t: f32) -> (Vec2, f32, bool, &'static str) {
    let leg1 = (HAUL_X0 - HAUL_TURN_X) / HAUL_SPEED;
    let leg2 = HAUL_SPOT.distance(Vec2::new(HAUL_TURN_X, HAUL_Z)) / HAUL_SPEED;
    let west = -std::f32::consts::FRAC_PI_2;
    if t < leg1 {
        return (Vec2::new(HAUL_X0 - HAUL_SPEED * t, HAUL_Z), west, true, "Walk_Carry_Loop");
    }
    let turn = env(t, &[(leg1, 0.0), (leg1 + 0.5, 1.0)]);
    let facing = west + (-std::f32::consts::PI - west) * turn; // round to face the fence (−Z)
    if t < leg1 + leg2 {
        let k = (t - leg1) / leg2;
        return (Vec2::new(HAUL_TURN_X, HAUL_Z).lerp(HAUL_SPOT, k), facing, true, "Walk_Carry_Loop");
    }
    let t2 = t - (leg1 + leg2);
    (HAUL_SPOT, -std::f32::consts::PI, false, if t2 < 2.6 { "PickUp_Table" } else { "Fixing_Kneeling" })
}

#[allow(clippy::type_complexity)]
fn drive_farm_scene(
    time: Res<Time>,
    mut state: ResMut<SceneState>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    mut actors: Query<
        (&mut crate::villagers::Villager, &mut Transform, Option<&mut crate::npc_skin::SkinClip>, Option<&FarmRole>, Option<&FarmFixed>),
        Without<crate::player::Hero>,
    >,
    mut hero_q: Query<&mut crate::player::Hero>,
    mut fade: Query<&mut BackgroundColor, With<FarmFade>>,
    mut bundle: Query<&mut Visibility, With<FarmBundle>>,
) {
    if state.active != Some(SceneId::FarmMorning) {
        return;
    }
    let t = state.clock.max(0.0);
    let (dt, now) = (time.delta_secs(), time.elapsed_secs());

    for (mut v, mut tf, clip, role, fixed) in &mut actors {
        let (pos, facing, walking, want) = match (role, fixed) {
            (Some(FarmRole::Kneeler), _) => (FARM_KNEEL, std::f32::consts::PI, false, farmer_clip(t)),
            (Some(FarmRole::Hauler), _) => farm_hauler(t),
            (None, Some(f)) => (f.pos, f.facing, false, ""),
            _ => continue,
        };
        v.pos = pos;
        v.facing = facing;
        v.moving = walking;
        tf.translation = Vec3::new(pos.x, world_y_at(pos), pos.y);
        tf.rotation = Quat::from_rotation_y(facing);
        if let (Some(mut c), true) = (clip, !want.is_empty()) {
            if c.0 != want {
                c.0 = want;
            }
        }
    }
    // The planks leave the hauler's hands when he sets them down.
    let set_down = HAUL_SPOT.distance(Vec2::new(HAUL_TURN_X, HAUL_Z)) / HAUL_SPEED + (HAUL_X0 - HAUL_TURN_X) / HAUL_SPEED + 1.1;
    for mut vis in &mut bundle {
        *vis = if t > set_down { Visibility::Hidden } else { Visibility::Inherited };
    }

    // The knight: pose handed to `hold_scene_hero` (PostUpdate); gait fields written here.
    let (wp, wf, walking) = farm_wat(t);
    state.hero = Some((Vec3::new(wp.x, world_y_at(wp), wp.y), wf));
    state.hero_moving = walking;
    if let Ok(mut hero) = hero_q.single_mut() {
        if walking {
            crate::player::anim::stage_gait(&mut hero, crate::player::SPEED, dt);
            hero.sprint_held = false;
        } else {
            hero.moving_amt *= (-dt * 8.0).exp();
        }
    }

    // Captions (pinned against ambient barks; `release`d on teardown).
    match FARM_LINES.iter().find(|(s, d, ..)| t >= *s && t < s + d) {
        Some((s, d, who, text)) => subs.force(now, Some(who), text, s + d - t),
        None => subs.force(now, None, "", 0.0),
    }

    // Fade in from black, out to black.
    let a = (1.0 - t / 1.4).max((t - (FARM_END - 1.4)) / 1.4).clamp(0.0, 1.0);
    for mut bg in &mut fade {
        bg.0 = Color::srgba(0.0, 0.0, 0.0, a);
    }

    if t >= FARM_END {
        state.want = None; // done — the next frame's teardown hands the world back
    }
}

/// Give each mender a hammer on their right hand bone once their skin is ready, shown only while
/// they actually hammer (`Fixing_Kneeling`) — the trade tool a farmer would otherwise hold (a hoe,
/// carried upside-down by the kneel clip) is hidden by `npc_skin` for the whole scene.
#[allow(clippy::type_complexity)]
fn farm_hammers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Option<Res<SceneAssets>>,
    state: Res<SceneState>,
    makers: Query<(Entity, &crate::npc_skin::Skinned, &crate::npc_skin::SkinClip), (With<FarmRole>, Without<FarmHammer>)>,
    held: Query<(&FarmHammer, &crate::npc_skin::SkinClip)>,
    mut vis: Query<&mut Visibility>,
) {
    if state.active != Some(SceneId::FarmMorning) {
        return;
    }
    let Some(assets) = assets else { return };
    for (e, sk, _) in &makers {
        let Some((hr, _)) = crate::npc_skin::hands(sk) else { continue };
        // authored in the studio elbow frame (fist at y -0.25, forward +Z), like the village-life props
        let head = commands
            .spawn((
                Mesh3d(meshes.add(merged(vec![
                    cylr(0.018, 0.34, Vec3::new(0.0, -0.27, 0.13), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2), WOOD),
                    cube(0.055, 0.13, 0.06, Vec3::new(0.0, -0.27, 0.30), IRON),
                ]))),
                MeshMaterial3d(assets.prop_mat.clone()),
                crate::npc_skin::elbow_to_hand(true),
                Visibility::Hidden,
            ))
            .id();
        commands.entity(hr).add_child(head);
        commands.entity(e).try_insert(FarmHammer(head));
    }
    for (h, clip) in &held {
        if let Ok(mut v) = vis.get_mut(h.0) {
            let want = if clip.0 == "Fixing_Kneeling" { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
    }
}

/// Play the scene's audio cues (`assets/cutscene/farm_morning.cues`: music bed, the farmer's VO clip,
/// hammer taps…) on the scene clock, and hold the normal music bed silent for the duration. Cues
/// whose file isn't there yet (lines not recorded) are skipped quietly. Silent under a still
/// (`FOREST_SCENE_T`) and in the video capture (its soundtrack is mixed offline from the same file).
#[allow(clippy::too_many_arguments)]
fn drive_farm_audio(
    mut commands: Commands,
    state: Res<SceneState>,
    server: Res<AssetServer>,
    cfg: Option<ResMut<crate::audio::AudioConfig>>,
    mut saved_music: Local<Option<f32>>,
    mut next: Local<usize>,
    mut cues: Local<Vec<Cue>>,
    mut cue_scene: Local<Option<SceneId>>,
    prog: Option<Res<crate::capture::ClipProgress>>,
) {
    let active = is_cutscene(state.active)
        && std::env::var_os("FOREST_SCENE_T").is_none()
        && prog.is_none();
    // Hold the day music off while the scene runs; hand it back after.
    if let Some(mut cfg) = cfg {
        if active && saved_music.is_none() {
            *saved_music = Some(cfg.music_vol);
            cfg.music_vol = 0.0;
        } else if !active {
            if let Some(v) = saved_music.take() {
                cfg.music_vol = v;
            }
        }
    }
    if !active {
        *next = 0;
        return;
    }
    if *cue_scene != state.active {
        *cue_scene = state.active;
        *cues = parse_cues(match state.active {
            Some(SceneId::FortArrival) => include_str!("../assets/cutscene/fort_arrival.cues"),
            Some(SceneId::ShipCabin) => include_str!("../assets/cutscene/ship_cabin.cues"),
            Some(SceneId::Storm) => include_str!("../assets/cutscene/storm.cues"),
            _ => include_str!("../assets/cutscene/farm_morning.cues"),
        });
        *next = 0;
    }
    let t = state.clock;
    while *next < cues.len() && cues[*next].0 <= t {
        let (_, path, vol, trim) = &cues[*next];
        *next += 1;
        if !std::path::Path::new("assets").join(path).exists() {
            continue;
        }
        commands.spawn((
            AudioPlayer::new(server.load(path.clone())),
            PlaybackSettings {
                start_position: trim.map(|(a, _)| std::time::Duration::from_secs_f32(a)),
                duration: trim.map(|(a, b)| std::time::Duration::from_secs_f32(b - a)),
                ..PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(*vol))
            },
        ));
    }
}

/// `time  path  volume  [flags…]` per line (`#` comments, blank lines ignored); `video`-flagged
/// lines are only for the offline soundtrack mix (in-game footsteps/ambience come from the game).
/// 	rim=a,b plays only seconds a..b of the file (one phrase cut out of a longer take).
fn parse_cues(src: &str) -> Vec<Cue> {
    let mut v: Vec<Cue> = src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (t, path, vol) = (it.next()?.parse().ok()?, it.next()?.to_string(), it.next()?.parse().ok()?);
            let flags: Vec<&str> = it.collect();
            let trim = flags.iter().find_map(|f| {
                let (a, b) = f.strip_prefix("trim=")?.split_once(',')?;
                Some((a.parse().ok()?, b.parse().ok()?))
            });
            (!flags.contains(&"video")).then_some((t, path, vol, trim))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// (time, path under assets/, volume, optional (from, to) seconds of the file).
type Cue = (f32, String, f32, Option<(f32, f32)>);

/// What the cutscene borrows from the camera and restores on teardown.
struct FarmLookSaved {
    fov: f32,
    dof: (f32, f32, f32),
    grade: Option<crate::filmgrade::FilmGrade>,
    /// The game ran TAA (swapped for SMAA through the film) / the SMAA preset it had, if any.
    taa: bool,
    smaa: Option<bevy::anti_alias::smaa::SmaaPreset>,
    /// The film brought its own bokeh (and the depth it reads) to a camera the preset had none on.
    added_dof: bool,
    added_depth: bool,
}

/// One camera setup: eye, look-at, vertical FOV (degrees), DOF sharp half-band (world units).
type Cam = (Vec3, Vec3, f32, f32);

fn lerp_cam(a: Cam, b: Cam, k: f32) -> Cam {
    (a.0.lerp(b.0, k), a.1.lerp(b.1, k), a.2 + (b.2 - a.2) * k, a.3 + (b.3 - a.3) * k)
}

/// The cutscene's coverage, by scene time. Every shot keeps ≥3.5 u from its subject on a 28–46°
/// lens, so people are always framed whole (a kneeling farmer stands up mid-loop; a close lens put
/// the camera at his hips). Wide → track the walk → two-shot → farmer/knight singles → follow the
/// knight out → pull up and away.
fn farm_shot(t: f32, gy: f32) -> Cam {
    let a = FARM;
    let at3 = |x: f32, y: f32, z: f32| Vec3::new(a.x + x, gy + y, a.y + z);
    let ease = |u: f32| u * u * (3.0 - 2.0 * u);
    let seg = |t0: f32, t1: f32| ease(((t - t0) / (t1 - t0)).clamp(0.0, 1.0));
    let lin = |t0: f32, t1: f32| ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    let (w, _, _) = farm_wat(t);
    let wat = |dy: f32| Vec3::new(w.x, world_y_at(w) + dy, w.y);
    match t {
        // A · establishing crane: down from above the grass toward the fence — the farmer at work,
        // the hauler crossing, the knight small on the road behind.
        t if t < 3.8 => lerp_cam(
            (at3(-8.0, 5.0, -9.0), at3(5.0, 1.0, 1.6), 46.0, 12.0),
            (at3(-3.5, 2.0, -6.4), at3(3.5, 0.9, 1.2), 44.0, 12.0),
            seg(0.0, 3.8),
        ),
        // B · he arrives: keeping pace on the near side, fence posts sliding by.
        t if t < 6.8 => (
            Vec3::new(w.x - 1.2, gy + 1.35, a.y - 4.8),
            wat(1.05) + Vec3::new(0.5, 0.0, 0.0),
            38.0,
            7.0,
        ),
        // C · the two-shot as he stops — everyone in frame, the farmer still not looking up.
        t if t < 9.2 => lerp_cam(
            (at3(4.8, 1.25, -5.6), at3(0.9, 0.85, 0.8), 36.0, 6.5),
            (at3(4.1, 1.22, -5.2), at3(0.9, 0.85, 0.8), 34.0, 6.5),
            lin(6.8, 9.2),
        ),
        // D · farmer, through the gap: "Bless you for the protection…"
        t if t < 15.8 => lerp_cam(
            (at3(-1.3, 1.0, -4.2), at3(0.0, 0.78, 0.55), 29.0, 1.4),
            (at3(-1.0, 0.96, -3.7), at3(0.0, 0.78, 0.55), 28.0, 1.4),
            lin(9.2, 15.8),
        ),
        // E · the knight, three-quarter from the near side: "You're welcome."
        t if t < 18.4 => lerp_cam(
            (at3(0.2, 1.3, -3.4), at3(2.0, 1.2, 0.95), 32.0, 1.6),
            (at3(0.5, 1.3, -3.1), at3(2.0, 1.2, 0.95), 30.0, 1.6),
            lin(15.8, 18.4),
        ),
        // D2 · farmer again, a touch tighter: "Wasn't thanking you."
        t if t < 21.8 => lerp_cam(
            (at3(-1.6, 0.95, -4.0), at3(0.0, 0.8, 0.55), 26.0, 1.4),
            (at3(-1.3, 0.95, -3.7), at3(0.0, 0.8, 0.55), 25.0, 1.4),
            lin(18.4, 21.8),
        ),
        // E2 · the knight's beat — "I know." — and a long look; the joke lands, the work goes on.
        t if t < 25.2 => lerp_cam(
            (at3(1.1, 1.25, -3.2), at3(2.0, 1.22, 0.95), 29.0, 1.6),
            (at3(0.8, 1.25, -3.4), at3(2.0, 1.22, 0.95), 31.0, 1.6),
            lin(21.8, 25.2),
        ),
        // G · pull up and away over the work: the farmers back at it, the knight standing by.
        _ => lerp_cam(
            (at3(3.0, 2.0, -6.2), at3(0.8, 0.8, 1.2), 40.0, 9.0),
            (at3(-2.5, 6.8, -12.5), at3(0.8, 0.4, 2.0), 40.0, 12.0),
            seg(25.2, FARM_END),
        ),
    }
}

/// Camera cuts + lens: [`farm_shot`] plus a faint handheld drift, a shallow depth of field racked
/// onto each shot's subject, and film grain, vignette and a touch of chromatic aberration on the final
/// grade pass. All of it is put back afterwards.
#[allow(clippy::type_complexity)]
fn farm_camera(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<SceneState>,
    mut cam: Query<
        (
            Entity,
            Option<&bevy::anti_alias::smaa::Smaa>,
            &mut Transform,
            &mut Projection,
            Option<&mut crate::dof::Dof>,
            Option<&mut crate::filmgrade::FilmGrade>,
            Option<&mut bevy::post_process::motion_blur::MotionBlur>,
            Option<&mut bevy::anti_alias::taa::TemporalAntiAliasing>,
            Has<bevy::core_pipeline::prepass::DepthPrepass>,
        ),
        (With<Camera3d>, Without<crate::player::Hero>),
    >,
    mut ibl: Query<&mut bevy::light::EnvironmentMapLight, With<Camera3d>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    (mut moon, mut fog): (
        Query<(&mut DirectionalLight, &mut Transform), (With<crate::scene::Moon>, Without<Camera3d>)>,
        Query<&mut bevy::pbr::DistanceFog, With<Camera3d>>,
    ),
    mut saved: Local<Option<FarmLookSaved>>,
    mut last_eye: Local<Option<Vec3>>,
) {
    let Ok((cam_e, smaa, mut c, mut proj, mut dof, mut grade, mut blur, taa, has_depth)) = cam.single_mut() else { return };
    let had_taa = taa.is_some();
    if !is_cutscene(state.active) {
        if last_eye.take().is_some() {
            if let Some(b) = blur.as_mut() {
                b.shutter_angle = 0.5; // quality.rs's in-game shutter
            }
        }
        // Hand the lens back (once) after the scene.
        if let Some(sv) = saved.take() {
            if let Projection::Perspective(p) = &mut *proj {
                p.fov = sv.fov;
            }
            if let Some(dof) = dof.as_mut() {
                (dof.range, dof.far_ramp, dof.max_radius) = sv.dof;
                dof.lens = 0.0;
            }
            if let (Some(g), Some(was)) = (grade.as_mut(), sv.grade) {
                **g = was;
            }
            let mut e = commands.entity(cam_e);
            if sv.taa {
                e.try_insert((
                    bevy::anti_alias::taa::TemporalAntiAliasing::default(),
                    bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening {
                        enabled: true,
                        sharpening_strength: 0.2, // quality.rs's TAA pairing
                        denoise: false,
                    },
                ));
            }
            match sv.smaa {
                Some(preset) => e.try_insert(bevy::anti_alias::smaa::Smaa { preset }),
                None => e.try_remove::<bevy::anti_alias::smaa::Smaa>(),
            };
            if sv.added_dof {
                e.try_remove::<crate::dof::Dof>();
            }
            if sv.added_depth {
                e.try_remove::<bevy::core_pipeline::prepass::DepthPrepass>();
            }
        }
        return;
    }
    let t = state.clock.max(0.0);
    let now = time.elapsed_secs();
    let fort = state.active == Some(SceneId::FortArrival);
    let ship = state.active == Some(SceneId::ShipCabin);
    let storm = state.active == Some(SceneId::Storm);
    let mut up = Vec3::Y;
    let ((eye, at, fov_deg, band), focus) = if fort {
        fort_shot(t)
    } else if storm {
        let (shot, roll_up) = storm::camera(t);
        up = roll_up;
        shot
    } else if ship {
        let (shot, ship_up) = cabin::camera(t);
        up = ship_up;
        shot
    } else {
        (farm_shot(t, world_y_at(FARM)), None)
    };
    let thin = fort || ship || storm;
    // Handheld: slow, tiny, incommensurate drifts on position and aim (never a shake).
    let h = |f: f32, ph: f32| (now * f + ph).sin();
    let sway = Vec3::new(h(0.9, 0.0) + 0.5 * h(2.3, 1.0), h(1.1, 2.0) * 0.6, h(0.7, 4.0)) * 0.012;
    let aim = Vec3::new(h(1.3, 3.0), h(0.8, 5.0) * 0.7, h(1.7, 1.5)) * 0.01;
    c.translation = eye + sway;
    c.look_at(at + aim, up);
    // Motion blur stays shut for the whole film: under the capture harness's per-frame encode stall
    // its motion vectors go stale and smear random frames (not just cuts) into radial streaks. On a
    // hard cut TAA's history is dropped too, or it ghosts the old shot in.
    let cut = last_eye.is_some_and(|p| p.distance(eye) > 1.0);
    *last_eye = Some(eye);
    if let Some(b) = blur.as_mut() {
        b.shutter_angle = 0.0;
    }
    if let (true, Some(mut taa)) = (cut, taa) {
        taa.reset = true;
    }

    if saved.is_none() {
        let fov = if let Projection::Perspective(p) = &*proj { p.fov } else { 50f32.to_radians() };
        *saved = Some(FarmLookSaved {
            fov,
            dof: dof.as_ref().map_or((0.0, 0.0, 0.0), |d| (d.range, d.far_ramp, d.max_radius)),
            grade: grade.as_deref().copied(),
            taa: had_taa,
            smaa: smaa.map(|s| s.preset),
            added_dof: dof.is_none(),
            added_depth: !has_depth,
        });
        // The lens is half the look: a preset that skips bokeh (Low) still shoots the film with it.
        // `quality.rs` only re-applies on a settings change, so these hold until handed back.
        let mut e = commands.entity(cam_e);
        if dof.is_none() {
            e.try_insert(crate::dof::default_dof());
        }
        if !has_depth {
            e.try_insert(bevy::core_pipeline::prepass::DepthPrepass);
        }
        // The film is shot through SMAA, not TAA: under the capture harness's per-frame stall TAA
        // reprojects moving people with stale motion vectors (the same fault that shuts motion blur
        // above) and smears every walking or gesturing figure soft. SMAA keeps them crisp.
        commands
            .entity(cam_e)
            .try_remove::<(
                bevy::anti_alias::taa::TemporalAntiAliasing,
                bevy::render::camera::TemporalJitter,
                bevy::render::camera::MipBias,
                bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening,
            )>()
            .try_insert(bevy::anti_alias::smaa::Smaa { preset: bevy::anti_alias::smaa::SmaaPreset::Ultra });
    }
    if let Projection::Perspective(p) = &mut *proj {
        p.fov = fov_deg.to_radians();
    }
    // Rack focus onto the subject: sharp through the subject, melting off behind it. The fort film
    // shoots through a thin lens (`band` is its aperture): a wide shot stays sharp to the horizon and
    // only a close subject throws the background — the banded model blurred the ground in front of
    // and behind every wide into a tilt-shift miniature.
    if let Some(dof) = dof.as_mut() {
        dof.focal = focus.unwrap_or_else(|| eye.distance(at));
        dof.range = band;
        dof.far_ramp = 16.0;
        // Below deck everything is within a few metres, so the cabin film opens the aperture
        // wider: the hull behind a close face melts the way a fast lens does in lamplight.
        dof.max_radius = if ship || storm { 9.0 } else if thin { 4.5 } else { 7.0 };
        dof.lens = if ship || storm { band * 1.5 } else if thin { band } else { 0.0 };
    }
    if let Some(g) = grade.as_mut() {
        g.grain = 0.045;
        g.vignette = 0.42;
        g.aberration = if ship { 0.003 } else if thin { 0.0012 } else { 0.0035 };
        g.time = now.fract() * 17.0;
        if ship {
            // Lamplight in a storm, shot on fast stock: amber highlights, the night blue in the
            // shadows, heavy grain and vignette, colour held back and the blacks crushed rather
            // than lifted — period drama, not a game render.
            g.grain = 0.15;
            g.vignette = 0.66;
            g.highlight_tint = Vec3::new(1.12, 0.98, 0.82);
            g.shadow_tint = Vec3::new(0.9, 0.97, 1.1);
            g.split = 0.7;
            g.vibrance = 0.0;
            g.saturation = 0.8;
            g.contrast = 1.1;
            g.toe = 0.006;
            g.toe_tint = Vec3::new(0.85, 0.92, 1.0);
            g.clarity = 0.6;
            g.halation = 0.7;
            g.streak = 0.5;
            g.weave = 0.45;
        }
        if storm {
            // Night and lightning on fast stock: cold, near-colourless, the blacks crushed, the
            // flashes blooming off the wet; only the torches keep their orange.
            g.grain = 0.16;
            g.vignette = 0.7;
            g.highlight_tint = Vec3::new(0.96, 1.0, 1.08);
            g.shadow_tint = Vec3::new(0.86, 0.95, 1.14);
            g.split = 0.6;
            g.vibrance = 0.0;
            g.saturation = 0.62;
            g.contrast = 1.16;
            g.toe = 0.008;
            g.toe_tint = Vec3::new(0.82, 0.9, 1.0);
            g.clarity = 0.7;
            g.halation = 0.85;
            g.streak = 0.55;
            g.weave = 0.5;
            g.aberration = 0.0025;
        }
        if fort {
            // Golden hour: warmer highlights, a cooler blue in the shade, a touch more split.
            g.highlight_tint = Vec3::new(1.13, 1.0, 0.80);
            g.shadow_tint = Vec3::new(0.86, 0.97, 1.14);
            g.split = 0.68;
            g.vibrance = 0.2;
            g.halation = 0.5;
            g.streak = 0.7;
            g.weave = 0.45;
        }
    }
    if storm && !storm::day_dev() {
        // The storm's own light over what `advance_sky` wrote this frame (it rewrites all of it
        // every frame, so the night comes back by itself after the film).
        let l = storm::light(t);
        let fl = l.flash;
        for (mut m, mut tf) in &mut moon {
            m.illuminance = l.moon;
            m.color = Color::srgb(0.62 + 0.3 * fl, 0.7 + 0.25 * fl, 1.0);
            *tf = Transform::from_translation(-l.dir * 120.0).looking_to(l.dir, Vec3::Y);
        }
        ambient.color = Color::srgb(0.55 + 0.35 * fl, 0.62 + 0.3 * fl, 0.85 + 0.15 * fl);
        ambient.brightness = l.ambient;
        for mut env in &mut ibl {
            env.intensity = l.ibl;
        }
        // The rain's haze closes in, and fades into the cloud dome's horizon (lit with it on a
        // strike), so the land beyond it melts into the storm instead of ending at the dome.
        let haze = storm::SKY_HORIZON * storm::sky_gain() * (1.0 + 22.0 * fl) * fog_gain();
        for mut f in &mut fog {
            f.color = Color::linear_rgb(haze.x, haze.y, haze.z);
            f.directional_light_color = Color::NONE;
            f.falloff = bevy::pbr::FogFalloff::Linear { start: 8.0, end: storm::SKY_R - 6.0 };
        }
    }
    if ship {
        // Below deck the night sky's blue fill doesn't reach: a dim neutral bounce instead
        // (`advance_sky` writes both afresh every frame, so the open air comes back by itself).
        // Dark: the storm is the light. A lightning flash throws the cold white fill up for a moment.
        let fl = cabin::flash(t);
        ambient.color = Color::srgb(0.72 + 0.2 * fl, 0.68 + 0.26 * fl, 0.66 + 0.34 * fl);
        ambient.brightness = 38.0 + 900.0 * fl;
        for mut env in &mut ibl {
            env.intensity = 30.0 + 700.0 * fl;
        }
    }
}

/// Clear the stage: butterflies (a metre from a long lens they fill the frame with a flat orange
/// wing) and the town's grey build-plot pads within [`CLEAR_R`] of the fence. Hidden, not despawned —
/// put back exactly as they were.
#[allow(clippy::type_complexity)]
fn farm_clear_stage(
    state: Res<SceneState>,
    mut q: Query<
        (Entity, &mut Visibility, &GlobalTransform),
        Or<(With<crate::ambient_life::Butterfly>, With<crate::town::BuildPlot>, With<crate::decor::Firefly>, With<crate::particles::Particle>)>,
    >,
    flies: Query<(), Or<(With<crate::decor::Firefly>, With<crate::particles::Particle>)>>,
    mut fly_tf: Query<&mut Transform, Or<(With<crate::decor::Firefly>, With<crate::particles::Particle>)>>,
    mut bars: Query<&mut Visibility, (With<crate::combat_fx::HpBar>, Without<crate::firelight::Ember>, Without<crate::decor::Firefly>, Without<crate::particles::Particle>, Without<crate::ambient_life::Butterfly>, Without<crate::town::BuildPlot>)>,
    mut embers: Query<&mut Visibility, (With<crate::firelight::Ember>, Without<crate::decor::Firefly>, Without<crate::particles::Particle>, Without<crate::ambient_life::Butterfly>, Without<crate::town::BuildPlot>)>,
    mut hidden: Local<Vec<(Entity, Visibility)>>,
    mut logged: Local<bool>,
) {
    if state.active == Some(SceneId::Storm) {
        // no fireflies (or the biome's weather motes) out in a storm: its own rain falls instead
        for (e, mut v, _) in &mut q {
            if *v != Visibility::Hidden && flies.contains(e) {
                hidden.push((e, *v));
                *v = Visibility::Hidden;
            }
        }
        // (and shrunk to nothing: their own systems re-scale them every frame in Update)
        let mut n = 0;
        for mut tf in &mut fly_tf {
            tf.scale = Vec3::ZERO;
            n += 1;
        }
        // nor sparks off the torches: the rain puts them out (each lives a second or two)
        for mut v in &mut embers {
            *v = Visibility::Hidden;
        }
        // no HP bars over a film's fight
        for mut v in &mut bars {
            *v = Visibility::Hidden;
        }
        if !std::mem::replace(&mut *logged, true) {
            info!("storm: {n} fireflies / weather motes put out");
        }
    } else if state.active == Some(SceneId::FarmMorning) {
        for (e, mut v, gt) in &mut q {
            let p = gt.translation();
            if *v != Visibility::Hidden && Vec2::new(p.x, p.z).distance(FARM) < CLEAR_R {
                hidden.push((e, *v));
                *v = Visibility::Hidden;
            }
        }
    } else {
        for (e, was) in hidden.drain(..) {
            if let Ok((_, mut v, _)) = q.get_mut(e) {
                *v = was;
            }
        }
    }
}

/// Radius (world units) around the fence kept clear of trees / pads / butterflies in the cutscene.
const CLEAR_R: f32 = 17.0;

/// Hide the whole HUD for the duration (every root UI node but the cutscene's own letterbox / fade
/// and the caption row), remembering each node's visibility so it comes back exactly as it was.
#[allow(clippy::type_complexity)]
fn farm_hide_hud(
    state: Res<SceneState>,
    mut roots: Query<
        (Entity, &mut Visibility),
        (With<Node>, Without<ChildOf>, Without<SceneProp>, Without<crate::subtitles::SubtitleRow>),
    >,
    mut hidden: Local<Vec<(Entity, Visibility)>>,
) {
    if is_cutscene(state.active) {
        for (e, mut v) in &mut roots {
            if *v != Visibility::Hidden {
                hidden.push((e, *v));
                *v = Visibility::Hidden;
            }
        }
    } else if !hidden.is_empty() {
        for (e, was) in hidden.drain(..) {
            if let Ok((_, mut v)) = roots.get_mut(e) {
                *v = was;
            }
        }
    }
}

// ── P.3 video rig: "The bell in the tower" ───────────────────────────────────────────
//
// Screenplay P.3 (`docs/superpowers/specs/2026-10-13-kampania-scenariusz-design.md`), filmed as a
// pre-rendered clip — NOT a game feature (no trigger, no skip, nothing saved). Golden hour; Pip runs
// ahead into the ruined fort past Maud and the knight, finds the well, the ruined keep ("that was a
// chapel") and — behind him, over the gate they just walked under — the bell. Maud answers without
// turning round. One clock drives actors, birds, dust, head turns, camera, captions, fades and the
// act card; the recorded Act I lines (`assets/audio/vo/campaign/a1_pip_05…`) are timed in
// `tools/cutscene/fort_audio.py` against the constants below (keep the two in step).

/// Low golden MORNING sun from the east-south-east (`scene.rs` sun = (cos a, sin a, 0.55)): the
/// bell tower stands on the SOUTH wall, so an afternoon (south-west) sun put every look at it straight
/// into the light — washed-out haze. From the ESE it rakes the tower's face and lights Maud's.
const FORT_SKY: f32 = 0.075;
const FORT_END: f32 = 41.95;
/// Fade to black, then the act card on black.
const FORT_FADE_OUT: (f32, f32) = (34.95, 36.35);

const WELL: Vec2 = Vec2::new(9.6, -6.4);
const RUIN: Vec2 = crate::castle::KEEP_POS;
const BELL: Vec2 = crate::castle::BELL_POS;
/// The bell's mouth, world height (hinge at `castle::BELL_PIVOT_Y` 7.1, bronze hangs ~0.6–1.2 below).
const BELL_Y: f32 = 6.3;

const PIP_GATE: Vec2 = Vec2::new(-0.4, 24.6);
/// Where Pip pulls up in the yard ("There's a well!") — the well 10 u ahead of him, the tower behind.
const PIP_1: Vec2 = Vec2::new(6.2, 3.0);
/// A few steps on toward the well, where the ruin catches his eye ("There's a— that's a chapel").
const PIP_2: Vec2 = Vec2::new(7.1, 0.4);
/// At the well's rim, at the very end ("Start with the well").
const PIP_WELL: Vec2 = Vec2::new(8.9, -5.0);
/// Maud stops a few paces behind the boy, between him and the gate.
const MAUD_MARK: Vec2 = Vec2::new(4.0, 8.5);
/// `castle_decor`'s notice board inside the south gate: no edicts posted yet on arrival, and it
/// stands 1.3 u in front of shot 3a's lens (it read as a whole barn wall at 27° fov).
const NOTICE_BOARD: Vec2 = Vec2::new(3.0, 19.4);
/// The pigeons pecking on Pip's line in from the gate — he runs straight through them (~7 s).
const FLOCK: Vec2 = Vec2::new(2.7, 14.6);

/// Timed captions: (start s, duration s, speaker, line) — verbatim from the recorded clips
/// (`a1_pip_05`, `a1_maud_14`, `a1_maud_15`, `a1_maud_16`). Pip's one breathless take (all
/// his lines, "Can I—" too) is split at its breaths (`fort_audio.py` VO `trim`s), one phrase per discovery, so every
/// cut lands in a pause instead of mid-word.
const FORT_LINES: [(f32, f32, &str, &str); 8] = [
    (11.45, 1.15, "Pip", "There's a well!"),
    (13.5, 3.5, "Pip", "And that\u{2026} that was a chapel!"),
    (17.15, 2.45, "Pip", "There's a bell! Up there!"),
    (19.72, 1.2, "Pip", "It's whole."),
    (21.25, 2.62, "Maud", "The well's whole too. Start with the well."),
    (24.5, 0.75, "Pip", "Can I\u{2014}"),
    (25.3, 2.54, "Maud", "At dusk. King's orders, one of them."),
    (28.65, 4.91, "Maud", "God help us. A well, a bell, and a wall and a half."),
];

/// The VO as `tools/cutscene/fort_audio.py` lays it into the soundtrack: (start s, clip, trim
/// from..to s, speaker). Read back here only to move the speakers' mouths (`fort_mouths`) — the
/// sound itself is mixed offline. Keep in step with that table.
const FORT_VO: [(f32, &str, f32, f32, FortRole); 8] = [
    (11.45, "audio/vo/campaign/a1_pip_05.ogg", 0.0, 1.1, FortRole::Pip), // "There's a well!"
    (13.5, "audio/vo/campaign/a1_pip_05.ogg", 1.8, 5.3, FortRole::Pip), // "And that… that was a chapel!"
    (17.15, "audio/vo/campaign/a1_pip_05.ogg", 5.65, 8.1, FortRole::Pip), // (gasp) "There's a bell! Up there!"
    (19.72, "audio/vo/campaign/a1_pip_05.ogg", 8.7, 9.88, FortRole::Pip), // "It's whole."
    (21.25, "audio/vo/campaign/a1_maud_14.ogg", 0.0, 2.62, FortRole::Maud), // "The well's whole too. Start with the well."
    (24.5, "audio/vo/campaign/a1_pip_05.ogg", 11.05, 11.75, FortRole::Pip), // "Can I—"
    (25.3, "audio/vo/campaign/a1_maud_15.ogg", 0.0, 2.54, FortRole::Maud), // "At dusk. King's orders, one of them."
    (28.65, "audio/vo/campaign/a1_maud_16.ogg", 0.0, 4.91, FortRole::Maud), // "God help us. A well, a bell, and a wall and a half."
];

/// Pip's and Maud's mouths follow their lines' loudness (see `lipsync`).
fn fort_mouths(
    mut commands: Commands,
    state: Res<SceneState>,
    server: Res<AssetServer>,
    sources: Res<Assets<AudioSource>>,
    mut clips: Local<Vec<(Handle<AudioSource>, Option<Vec<f32>>)>>,
    actors: Query<(Entity, &FortRole)>,
) {
    if state.active != Some(SceneId::FortArrival) {
        return;
    }
    if clips.is_empty() {
        *clips = FORT_VO.iter().map(|vo| (server.load(vo.1), None)).collect();
    }
    for ((h, env), vo) in clips.iter_mut().zip(&FORT_VO) {
        if env.is_none() {
            *env = sources.get(&*h).map(|s| crate::lipsync::envelope(s, vo.2, vo.3));
        }
    }
    let t = state.clock.max(0.0);
    for (e, role) in &actors {
        let open = clips
            .iter()
            .zip(&FORT_VO)
            .filter(|(_, vo)| vo.4 == *role)
            .filter_map(|((_, env), vo)| env.as_deref().map(|env| crate::lipsync::sample(env, t - vo.0)))
            .fold(0.0, f32::max);
        commands.entity(e).try_insert(crate::lipsync::MouthOpen(open));
    }
}

/// One step of a scripted walk: `Go(to, speed)` straight there, `Until(t)` hold the spot until `t`.
#[derive(Clone, Copy)]
enum Leg {
    Go(Vec2, f32),
    Until(f32),
}

/// Where a scripted walker is at scene time `t`: at `from` from `t0`, then each leg in turn.
/// → (position, heading while moving).
fn route(t: f32, t0: f32, from: Vec2, legs: &[Leg]) -> (Vec2, Option<Vec2>) {
    let (mut at, mut p) = (t0, from);
    if t <= t0 {
        return (from, None);
    }
    let is_go = |i: usize| matches!(legs.get(i), Some(Leg::Go(..)));
    for (i, leg) in legs.iter().enumerate() {
        match *leg {
            Leg::Until(u) => {
                if t < u {
                    return (p, None);
                }
                at = at.max(u);
            }
            Leg::Go(to, v) => {
                let d = to - p;
                let len = d.length();
                if len < 1e-4 {
                    continue;
                }
                // Trapezoid speed: ramp up from a standstill / down into one over `r` s (never
                // between two straight legs), so nobody starts or stops dead — a skin's clip can't
                // snap from a full jog to idle without the feet skating.
                let r = (len / v).min(0.45);
                let (r_in, r_out) = (if i > 0 && is_go(i - 1) { 0.0 } else { r }, if is_go(i + 1) { 0.0 } else { r });
                let dur = len / v + (r_in + r_out) * 0.5;
                if t < at + dur {
                    let u = t - at;
                    let s = if u < r_in {
                        v * u * u / (2.0 * r_in)
                    } else if u < dur - r_out {
                        v * (u - r_in * 0.5)
                    } else {
                        len - v * (dur - u) * (dur - u) / (2.0 * r_out)
                    };
                    return (p + d * (s / len).clamp(0.0, 1.0), Some(d / len));
                }
                at += dur;
                p = to;
            }
        }
    }
    (p, None)
}

/// Pip: runs past the grown-ups, through the gate (flushing the pigeons) and pulls up in the yard;
/// trots on toward the well until the ruin stops him dead; turns round to the bell; is told to wait;
/// and in the end trudges off to the well as he was told.
fn fort_pip(t: f32) -> (Vec2, Option<Vec2>) {
    use Leg::*;
    route(t, 0.3, Vec2::new(-0.8, 38.6), &[
        Go(PIP_GATE, 3.6),
        Go(PIP_1, 3.6), // → ~10.8, looks round
        Until(12.1),    // "There's a well!" — points
        Go(PIP_2, 2.6), // → ~13.5, "And that…" stops dead: the chapel
        Until(28.95),
        Go(PIP_WELL, 1.0), // "Start with the well."
    ])
}

fn fort_maud(t: f32) -> (Vec2, Option<Vec2>) {
    use Leg::*;
    // a grown woman's steady walk (1.35 u/s pushed her skin into a trot), so she sets off nearer
    route(t, 0.0, Vec2::new(1.0, 29.8), &[Go(Vec2::new(0.9, 24.6), 1.15), Go(MAUD_MARK, 1.15)]) // → ~19.2
}

/// The knight leads them in and stops just inside the gate by the tower foot.
fn fort_wat(t: f32) -> (Vec2, Option<Vec2>) {
    use Leg::*;
    let v = crate::player::SPEED;
    route(t, 0.0, Vec2::new(-0.1, 29.5), &[Go(Vec2::new(-0.5, 24.4), v), Go(Vec2::new(-2.2, 21.2), v)])
}

/// Settlers trailing in behind (the last two carrying planks for the rebuild).
fn fort_settler(i: u8, t: f32) -> (Vec2, Option<Vec2>) {
    use Leg::*;
    match i {
        0 => route(t, 4.0, Vec2::new(-1.6, 41.5), &[Go(Vec2::new(0.3, 24.9), 1.25), Go(Vec2::new(2.9, 21.0), 1.25)]),
        1 => route(t, 6.0, Vec2::new(1.8, 42.6), &[Go(Vec2::new(0.6, 24.9), 1.2), Go(Vec2::new(4.4, 22.0), 1.2)]),
        2 => route(t, 10.0, Vec2::new(-0.9, 43.0), &[Go(Vec2::new(-0.2, 24.9), 1.2), Go(Vec2::new(-2.0, 14.0), 1.2)]),
        _ => route(t, 13.0, Vec2::new(1.5, 44.0), &[Go(Vec2::new(0.8, 24.9), 1.2), Go(Vec2::new(3.6, 15.5), 1.2)]),
    }
}

fn yaw_of(d: Vec2) -> f32 {
    d.x.atan2(d.y)
}
fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
/// Ease an angle toward a target along the short way round.
fn turn_toward(cur: f32, target: f32, k: f32) -> f32 {
    cur + wrap_pi(target - cur) * k
}

#[derive(Component, Clone, Copy, PartialEq)]
enum FortRole {
    Pip,
    Maud,
    Settler(u8),
}

/// A scripted head turn layered on top of the clip (radians, in the body's frame): yaw left/right,
/// pitch up, roll = a tilt of the head. Eased toward `want` each frame; `bone` is found lazily.
#[derive(Component, Default)]
pub(super) struct HeadLook {
    cur: Vec3,
    want: Vec3,
    pub(super) bone: Option<Entity>,
}

/// A scripted point: the right arm swung straight out at `at` (world), blended in by `w` over the
/// clip's own arm. Bones found lazily (`upperarm_r` / `lowerarm_r`).
#[derive(Component, Default)]
struct ArmPoint {
    at: Option<Vec3>,
    w: f32,
    bones: Option<(Entity, Entity)>,
}

#[derive(Component)]
struct FortBird(u8);
#[derive(Component)]
struct FortWing {
    bird: u8,
    right: bool,
}
#[derive(Component)]
struct FortTitle;

/// Each pigeon's spot on the ground and takeoff time (scene s) — deterministic, so a
/// `FOREST_SCENE_T` still shows the same flock as the clip.
fn bird_home(i: u8) -> (Vec2, f32) {
    let a = i as f32 * 2.399; // golden-angle scatter
    let r = 0.35 + 0.17 * i as f32;
    (FLOCK + Vec2::new(a.cos(), a.sin()) * r, 6.9 + ((i as f32 * 0.37).fract()) * 0.4)
}

/// A pigeon at scene time `t`: (position, facing yaw, wing angle, flying?).
fn bird_pose(i: u8, t: f32) -> (Vec3, f32, f32, bool) {
    let (home, t_off) = bird_home(i);
    let gy = world_y_at(home);
    let tau = t - t_off;
    if tau <= 0.0 {
        // pecking about: a little bob, wings folded
        let peck = ((t * 2.1 + i as f32 * 1.3).sin() * 0.5 + 0.5).powi(6) * 0.03;
        return (Vec3::new(home.x, gy + 0.02 - peck, home.y), i as f32 * 1.7, 0.08, false);
    }
    // burst up and away from the boy, then all bank toward the north-west wall and climb out
    let away = (home - fort_pip(t_off).0).normalize_or(Vec2::X);
    let common = Vec2::new(-0.55, -0.83);
    let pos_at = |u: f32| {
        let w = (u / 2.4).clamp(0.0, 1.0);
        let w = w * w * (3.0 - 2.0 * w);
        let dist = 2.4 * u + 1.1 * u * u;
        let dir = (away * (1.0 - w) + common * w).normalize_or(away);
        let h = 2.6 * u + 0.55 * u * u;
        Vec3::new(home.x + dir.x * dist, gy + 0.05 + h.min(18.0), home.y + dir.y * dist)
    };
    let p = pos_at(tau);
    let v = pos_at(tau + 0.03) - p;
    let hz = if tau < 1.6 { 11.0 } else { 7.0 };
    let wing = 0.25 + 0.85 * (tau * std::f32::consts::TAU * hz + i as f32).sin();
    (p, v.x.atan2(v.z), wing, true)
}

fn setup_fort_arrival(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    assets: &SceneAssets,
    fonts: Option<&crate::ui::fonts::UiFonts>,
) {
    use crate::npc_skin as ns;
    // The cast — realistic skins forced over the seeded pick (Pip = the boy, Maud = the old woman).
    let (p0, _) = fort_pip(0.0);
    let pip = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, p0, std::f32::consts::PI, None, 3);
    commands.entity(pip).insert((ns::SkinRequest(ns::BOY), ns::SkinClip(""), FortRole::Pip, HeadLook::default(), ArmPoint::default()));
    let (m0, _) = fort_maud(0.0);
    let maud = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, m0, std::f32::consts::PI, None, 9);
    commands.entity(maud).insert((ns::SkinRequest(ns::MAUD), ns::SkinClip(""), FortRole::Maud, HeadLook::default()));
    let load = meshes.add(plank_bundle_mesh());
    for (i, seed) in [(0u8, 5u32), (1, 19), (2, 29), (3, 37)] {
        let (s0, _) = fort_settler(i, 0.0);
        let e = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, s0, std::f32::consts::PI, None, seed);
        commands.entity(e).insert((ns::SkinClip(""), FortRole::Settler(i)));
        if i >= 2 {
            commands.entity(e).with_children(|p| {
                p.spawn((
                    Mesh3d(load.clone()),
                    MeshMaterial3d(assets.prop_mat.clone()),
                    Transform::from_xyz(0.0, 1.0, 0.5),
                    ns::CarriedLoad { across: false },
                ));
            });
        }
    }

    // Pigeons: body + two hinged wings, all vertex-coloured on the shared prop material.
    let body = meshes.add(merged(vec![
        cube(0.085, 0.075, 0.19, Vec3::new(0.0, 0.06, 0.0), 0x5f636b),
        cube(0.055, 0.055, 0.06, Vec3::new(0.0, 0.11, 0.1), 0x4a4f58),
        cube(0.02, 0.015, 0.03, Vec3::new(0.0, 0.1, 0.14), 0x3a3530),
        cube(0.07, 0.018, 0.08, Vec3::new(0.0, 0.07, -0.13), 0x50545c),
    ]));
    let wing_r = meshes.add(merged(vec![cube(0.2, 0.012, 0.09, Vec3::new(0.1, 0.0, 0.0), 0x6a6e76)]));
    let wing_l = meshes.add(merged(vec![cube(0.2, 0.012, 0.09, Vec3::new(-0.1, 0.0, 0.0), 0x6a6e76)]));
    for i in 0..9u8 {
        let (p, yaw, _, _) = bird_pose(i, 0.0);
        commands
            .spawn((
                Transform::from_translation(p).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(0.9)),
                Visibility::default(),
                FortBird(i),
                SceneProp,
            ))
            .with_children(|b| {
                b.spawn((Mesh3d(body.clone()), MeshMaterial3d(assets.prop_mat.clone()), Transform::default()));
                for (right, mesh) in [(true, wing_r.clone()), (false, wing_l.clone())] {
                    b.spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(assets.prop_mat.clone()),
                        Transform::from_xyz(if right { 0.04 } else { -0.04 }, 0.085, 0.0),
                        FortWing { bird: i, right },
                    ));
                }
            });
    }

    spawn_letterbox(commands);
    // The act card, on the black after the fade.
    if let Some(f) = fonts {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                GlobalZIndex(39),
                SceneProp,
            ))
            .with_children(|p| {
                p.spawn((crate::ui::fonts::label(&f.display, "ACT I \u{2014} THE TOLL", 86.0, Color::srgba(0.92, 0.87, 0.76, 0.0)), FortTitle));
            });
    }
}

/// Letterbox (2.39:1 on a 16:9 frame) + the fade-from-black overlay the scene drives.
fn spawn_letterbox(commands: &mut Commands) {
    for top in [true, false] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(12.8),
                top: if top { Val::Px(0.0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { Val::Px(0.0) },
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(35),
            SceneProp,
        ));
    }
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(38),
        FarmFade,
        SceneProp,
    ));
}

/// The well's mouth / the ruin's broken gable / the bell — what Pip points at as he names it.
fn well_pt() -> Vec3 {
    Vec3::new(WELL.x, world_y_at(WELL) + 0.7, WELL.y)
}
fn ruin_pt() -> Vec3 {
    Vec3::new(RUIN.x, world_y_at(RUIN) + 3.2, RUIN.y)
}
fn bell_pt() -> Vec3 {
    Vec3::new(BELL.x, BELL_Y, BELL.y)
}

/// Pip's beat when he isn't running: (facing target, clip, head yaw/pitch/roll, arm points at). Every
/// thing he names is on screen as he names it, his arm out at it (`a1_pip_05` word times from the
/// clip at its scene times: "well" 11.9, "And that…" 13.6, "chapel" 16.6, "bell" 18.2, "Up there"
/// 18.9, "It's whole" 19.85).
fn pip_beat(t: f32, pos: Vec2) -> (f32, &'static str, Vec3, Option<Vec3>) {
    let to = |q: Vec2| yaw_of(q - pos);
    let m = fort_maud(t).0;
    match t {
        // pulled up in the yard, gawping round: left, right…
        t if t < 11.3 => (to(WELL) - 0.3, "Idle_Loop", Vec3::new(0.75 * ((t - 10.8) * 5.0).sin(), 0.15, 0.0), None),
        // "There's a well!" (11.45)
        t if t < 12.15 => (to(WELL), "Idle_Loop", Vec3::new(0.0, 0.05, 0.0), Some(well_pt())),
        // trotting on … stopped dead at the ruin: "And that… that was a chapel!" (13.6 / 15.7)
        t if t < 16.9 => (to(RUIN), "Idle_Loop", Vec3::new(0.0, 0.12, 0.0), (t > 13.7).then(ruin_pt)),
        // spins round with a gasp: "There's a bell!" (17.8) "Up there!" (18.9) — arm up at the belfry
        t if t < 19.6 => (to(BELL), "Idle_Loop", Vec3::new(0.0, env(t, &[(17.1, 0.15), (18.9, 0.45)]), 0.0), (t > 17.6).then(bell_pt)),
        // "It's whole." (19.85) — just staring up at it
        t if t < 21.1 => (to(BELL), "Idle_Loop", Vec3::new(0.0, 0.45, 0.0), None),
        // Maud: "The well's whole too…" — he turns to her
        t if t < 24.35 => (to(m), "Idle_Loop", Vec3::new(0.0, 0.08, 0.0), None),
        // "Can I—" — to her, hands going (no point: the bell is behind the lens here, an arm thrown
        // at it read as waving at the camera)
        t if t < 25.3 => (to(m), "Idle_Talking_Loop", Vec3::new(0.0, 0.1, 0.0), None),
        // "At dusk. King's orders." — and his head drops
        _ => (to(m), "Idle_Loop", Vec3::new(0.0, env(t, &[(25.95, 0.05), (26.75, -0.42)]), 0.0), None),
    }
}

/// Maud's beat once she's stopped: (facing target, clip, head). She stops behind the boy and answers
/// him straight — the review's "shade of amusement" goes in the body: a slow weary nod and a head
/// tilt, a glance at the well, arms folded again for "At dusk.", then a long look round the ruin.
fn maud_beat(t: f32, pip: Vec2) -> (f32, &'static str, Vec3) {
    let m = MAUD_MARK;
    let at_pip = yaw_of(pip - m);
    let to_well = (wrap_pi(yaw_of(WELL - m) - at_pip)).clamp(-0.7, 0.7);
    match t {
        t if t < 20.85 => (at_pip, "Idle_FoldArms_Loop", Vec3::ZERO),
        t if t < 21.65 => (at_pip, "Yes", Vec3::new(0.0, 0.0, 0.16)),
        // "…Start with the well." — a glance at it
        t if t < 23.85 => (at_pip, "Idle_Talking_Loop", Vec3::new(env(t, &[(22.75, 0.0), (23.15, to_well), (23.75, to_well * 0.3)]), 0.0, 0.12)),
        t if t < 27.95 => (at_pip, "Idle_FoldArms_Loop", Vec3::new(0.0, 0.0, 0.08)),
        // "God help us." — a slow look round: the well, the ruin, the broken north-west wall
        _ => (at_pip + env(t, &[(27.95, 0.0), (32.95, -1.3)]), "Idle_Loop", Vec3::new(0.0, 0.08, 0.0)),
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn drive_fort_scene(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<SceneState>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    mut actors: Query<
        (
            &mut crate::villagers::Villager,
            &mut Transform,
            &mut crate::npc_skin::SkinClip,
            &FortRole,
            Option<&mut HeadLook>,
            Option<&mut ArmPoint>,
        ),
        Without<crate::player::Hero>,
    >,
    mut hero_q: Query<&mut crate::player::Hero>,
    mut fade: Query<&mut BackgroundColor, With<FarmFade>>,
    mut title: Query<&mut TextColor, With<FortTitle>>,
    mut wat_yaw: Local<Option<f32>>,
    mut last_dust: Local<f32>,
    mut pip_ran: Local<bool>,
    mut stir: ResMut<crate::castle::BellStir>,
) {
    if state.active != Some(SceneId::FortArrival) {
        *wat_yaw = None;
        return;
    }
    let t = state.clock.max(0.0);
    let (dt, now) = (time.delta_secs(), time.elapsed_secs());
    // "It's whole." — the bell stirs in the tower and its clapper touches the bronze on the hum
    // (`cs_bell_hum` at 19.55 in `fort_audio.py`). Pinned to the scene clock.
    let s0 = 19.55 - crate::castle::STIR_KISS;
    if (s0..s0 + 11.0).contains(&t) {
        stir.0 = Some(now - (t - s0));
    }
    let k_turn = 1.0 - (-dt * 7.0).exp();
    let k_head = 1.0 - (-dt * 4.5).exp();
    let (pip_pos, pip_dir) = fort_pip(t);

    for (mut v, mut tf, mut clip, role, head, arm) in &mut actors {
        let with_arm = |(f, c, h): (f32, &'static str, Vec3)| (f, c, h, None);
        let ((pos, dir), beat) = match *role {
            FortRole::Pip => (fort_pip(t), pip_beat(t, pip_pos)),
            FortRole::Maud => (fort_maud(t), with_arm(maud_beat(t, pip_pos))),
            FortRole::Settler(i) => {
                let (p, d) = fort_settler(i, t);
                let look = if i == 0 { "Idle_FoldArms_Loop" } else { "Idle_Loop" };
                ((p, d), (yaw_of(pip_pos - p), look, Vec3::ZERO, None))
            }
        };
        let (face, want_clip, want_head, point) = beat;
        if let Some(mut a) = arm {
            let point = point.filter(|_| dir.is_none());
            if point.is_some() {
                a.at = point;
            }
            let goal = if point.is_some() { 1.0 } else { 0.0 };
            a.w += (goal - a.w) * (1.0 - (-dt * 9.0).exp());
        }
        // first frame: snap to the mark instead of swinging round from the spawn facing
        let first = v.pos.distance_squared(pos) > 4.0;
        let target = dir.map_or(face, yaw_of);
        v.facing = if first { target } else { turn_toward(v.facing, target, if dir.is_some() { k_turn * 1.6 } else { k_turn }) };
        v.pos = pos;
        v.moving = dir.is_some();
        tf.translation = Vec3::new(pos.x, world_y_at(pos), pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
        // moving: let the skin pick walk/jog from real speed ("" hint), carriers keep the carry clip
        let c = match (*role, dir.is_some()) {
            (FortRole::Settler(2 | 3), true) => "Walk_Carry_Loop",
            (_, true) => "",
            _ => want_clip,
        };
        if clip.0 != c {
            clip.0 = c;
        }
        if let Some(mut h) = head {
            h.want = if dir.is_some() { Vec3::ZERO } else { want_head };
            let cur = h.cur;
            h.cur = cur + (h.want - cur) * k_head;
        }
    }

    // Kicked-up dust under the boy's running feet, and a skid puff where he pulls up.
    let running = pip_dir.is_some() && t > 0.6;
    if running && t - *last_dust > 0.24 {
        *last_dust = t;
        commands.spawn(crate::build_fx::DustBurst {
            pos: Vec3::new(pip_pos.x, world_y_at(pip_pos) + 0.04, pip_pos.y),
            n: 3,
            spd: 0.5,
            scale0: 0.3,
            radius: 0.08,
        });
    }
    if *pip_ran && !running && t > 1.0 {
        commands.spawn(crate::build_fx::DustBurst {
            pos: Vec3::new(pip_pos.x, world_y_at(pip_pos) + 0.04, pip_pos.y),
            n: 6,
            spd: 0.8,
            scale0: 0.38,
            radius: 0.15,
        });
    }
    *pip_ran = running;

    // The knight: pose handed to `hold_scene_hero` (PostUpdate); gait fields written here.
    let (wp, wd) = fort_wat(t);
    let wt = wd.map_or(yaw_of(pip_pos - wp), yaw_of);
    let wy = match *wat_yaw {
        Some(y) => turn_toward(y, wt, k_turn),
        None => wt,
    };
    *wat_yaw = Some(wy);
    state.hero = Some((Vec3::new(wp.x, world_y_at(wp), wp.y), wy));
    state.hero_moving = wd.is_some();
    if let Ok(mut hero) = hero_q.single_mut() {
        if wd.is_some() {
            crate::player::anim::stage_gait(&mut hero, crate::player::SPEED, dt);
            hero.sprint_held = false;
        } else {
            hero.moving_amt *= (-dt * 8.0).exp();
        }
    }

    match FORT_LINES.iter().find(|(s, d, ..)| t >= *s && t < s + d) {
        Some((s, d, who, text)) => subs.force(now, Some(who), text, s + d - t),
        None => subs.force(now, None, "", 0.0),
    }

    let a = (1.0 - t / 1.2).max((t - FORT_FADE_OUT.0) / (FORT_FADE_OUT.1 - FORT_FADE_OUT.0)).clamp(0.0, 1.0);
    for mut bg in &mut fade {
        bg.0 = Color::srgba(0.0, 0.0, 0.0, a);
    }
    let ta = env(t, &[(36.75, 0.0), (37.95, 1.0), (40.35, 1.0), (41.55, 0.0)]);
    for mut c in &mut title {
        c.0 = c.0.with_alpha(ta);
    }

    if t >= FORT_END {
        state.want = None;
    }
}

/// Fly the pigeons (pose is a pure function of the scene clock — see [`bird_pose`]).
fn fort_birds(
    state: Res<SceneState>,
    mut birds: Query<(&FortBird, &mut Transform, &mut Visibility), Without<FortWing>>,
    mut wings: Query<(&FortWing, &mut Transform), Without<FortBird>>,
) {
    if state.active != Some(SceneId::FortArrival) {
        return;
    }
    let t = state.clock.max(0.0);
    for (b, mut tf, mut vis) in &mut birds {
        let (p, yaw, _, flying) = bird_pose(b.0, t);
        tf.translation = p;
        tf.rotation = Quat::from_rotation_y(yaw);
        let gone = flying && t - bird_home(b.0).1 > 8.0;
        let want = if gone { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
    }
    for (w, mut tf) in &mut wings {
        let (_, _, a, _) = bird_pose(w.bird, t);
        tf.rotation = Quat::from_rotation_z(if w.right { a } else { -a });
    }
}

/// Layer each actor's scripted head turn onto the bone the clip just posed. The offset is built in
/// the body's frame (forward +Z: yaw about Y, pitch up about X, roll about Z) and carried into the
/// head's parent space, so it doesn't depend on the skeleton's local bone axes.
fn fort_head_look(
    state: Res<SceneState>,
    mut roots: Query<(&mut HeadLook, &crate::npc_skin::Skinned, &GlobalTransform)>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    globals: Query<&GlobalTransform>,
    mut tfs: Query<&mut Transform>,
) {
    if !matches!(state.active, Some(SceneId::FortArrival | SceneId::ShipCabin)) {
        return;
    }
    for (mut hl, sk, root) in &mut roots {
        if hl.bone.is_none() {
            hl.bone = children.iter_descendants(sk.inst()).find(|&d| names.get(d).is_ok_and(|n| n.as_str() == "head"));
        }
        let Some(bone) = hl.bone else { continue };
        if hl.cur.length_squared() < 1e-6 {
            continue;
        }
        let Ok(parent) = parents.get(bone) else { continue };
        let Ok(pg) = globals.get(parent.parent()) else { continue };
        let pr = pg.to_scale_rotation_translation().1;
        let rr = root.to_scale_rotation_translation().1;
        let off = Quat::from_rotation_y(hl.cur.x) * Quat::from_rotation_x(-hl.cur.y) * Quat::from_rotation_z(hl.cur.z);
        let w = rr * off * rr.inverse();
        if let Ok(mut tf) = tfs.get_mut(bone) {
            tf.rotation = (pr.inverse() * w * pr * tf.rotation).normalize();
        }
    }
}

/// World affine of `e` from THIS frame's local transforms (the clip just posed the bones, so the
/// propagated `GlobalTransform`s are still last frame's).
fn live_global(e: Entity, parents: &Query<&ChildOf>, tfs: &Query<&mut Transform>) -> bevy::math::Affine3A {
    let mut m = tfs.get(e).map_or(bevy::math::Affine3A::IDENTITY, |t| t.compute_affine());
    let mut cur = e;
    while let Ok(p) = parents.get(cur) {
        cur = p.parent();
        if let Ok(t) = tfs.get(cur) {
            m = t.compute_affine() * m;
        }
    }
    m
}

fn rot_of(m: bevy::math::Affine3A) -> Quat {
    m.to_scale_rotation_translation().1
}

/// Swing Pip's right arm out straight at what he's naming, on top of the clip. Works from the real
/// joint positions (shoulder → elbow → wrist), not from assumed bone axes: the upper arm turns onto
/// the aim line, then the forearm straightens along it.
fn fort_arm_point(
    state: Res<SceneState>,
    mut roots: Query<(&mut ArmPoint, &crate::npc_skin::Skinned)>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    mut tfs: Query<&mut Transform>,
) {
    if state.active != Some(SceneId::FortArrival) {
        return;
    }
    for (mut ap, sk) in &mut roots {
        if ap.bones.is_none() {
            let find = |n: &str| children.iter_descendants(sk.inst()).find(|&d| names.get(d).is_ok_and(|x| x.as_str() == n));
            ap.bones = find("upperarm_r").zip(find("lowerarm_r"));
        }
        let (Some((upper, lower)), Some(at)) = (ap.bones, ap.at) else { continue };
        if ap.w < 0.01 {
            continue;
        }
        let Ok(hand) = children.get(lower).map(|c| c.iter().find(|&d| names.get(d).is_ok_and(|x| x.as_str() == "hand_r"))) else { continue };
        let Some(hand) = hand else { continue };
        let Ok(up_parent) = parents.get(upper).map(|p| p.parent()) else { continue };
        let gp = live_global(up_parent, &parents, &tfs);
        let gu = live_global(upper, &parents, &tfs);
        let gl = live_global(lower, &parents, &tfs);
        let gh = live_global(hand, &parents, &tfs);
        let (shoulder, elbow, wrist) = (Vec3::from(gu.translation), Vec3::from(gl.translation), Vec3::from(gh.translation));
        let aim = (at - shoulder).normalize_or_zero();
        if aim == Vec3::ZERO {
            continue;
        }
        let w = ap.w.clamp(0.0, 1.0);
        let q1 = Quat::IDENTITY.slerp(Quat::from_rotation_arc((elbow - shoulder).normalize(), aim), w);
        let gu_rot = q1 * rot_of(gu);
        let fore = q1 * (wrist - elbow).normalize();
        let q2 = Quat::IDENTITY.slerp(Quat::from_rotation_arc(fore, aim), w * 0.9);
        let gl_rot = q2 * q1 * rot_of(gl);
        if let Ok(mut t) = tfs.get_mut(upper) {
            t.rotation = (rot_of(gp).inverse() * gu_rot).normalize();
        }
        if let Ok(mut t) = tfs.get_mut(lower) {
            t.rotation = (gu_rot.inverse() * gl_rot).normalize();
        }
    }
}

/// The cutscene's coverage → (camera, focus distance override). Outside: low in the grass, the
/// three walk past into the gate, the lens lifts to the tower. Inside, each thing Pip names is shown
/// as he names it, over his shoulder with his arm out at it (well → chapel ruin → bell, then the bell
/// itself); Maud and the boy in over-the-shoulders and a side-on two-shot; a crane up and back over
/// the whole ruin as he walks off to the well.
fn fort_shot(t: f32) -> (Cam, Option<f32>) {
    let ease = |u: f32| u * u * (3.0 - 2.0 * u);
    let seg = |t0: f32, t1: f32| ease(((t - t0) / (t1 - t0)).clamp(0.0, 1.0));
    let lin = |t0: f32, t1: f32| ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    let at = |x: f32, dy: f32, z: f32| Vec3::new(x, world_y_at(Vec2::new(x, z)) + dy, z);
    let pip = |dy: f32| {
        let p = fort_pip(t).0;
        Vec3::new(p.x, world_y_at(p) + dy, p.y)
    };
    let bell = Vec3::new(BELL.x, BELL_Y, BELL.y);
    match t {
        // 1 · the breach: low in the grass outside the south gate, drifting in; the lens lifts and
        //     swings left to find the tower beside the gate.
        t if t < 7.0 => {
            let k = seg(0.0, 7.0);
            let eye = at(2.3, 0.45, 37.4).lerp(at(1.0, 1.5, 33.8), k);
            let look = at(0.1, 1.3, 24.0).lerp(bell, seg(2.8, 7.0));
            ((eye, look, 40.0 - 4.0 * k, 0.35), None)
        }
        // 2 · inside, looking back at the gate (the tower over it): Pip tears in through the
        //     pigeons straight at the lens and skids up in the yard, looking round.
        t if t < 11.1 => {
            let eye = at(8.4, 1.15, -0.9).lerp(at(8.3, 1.18, -0.6), lin(7.0, 11.1));
            let look = at(0.5, 1.4, 20.0).lerp(pip(0.95), seg(7.0, 10.6) * 0.8);
            ((eye, look, 34.0, 0.5), Some(eye.distance(pip(0.9))))
        }
        // 3 · "There's a well!" — behind his shoulder, his arm out at it; he trots toward it.
        t if t < 13.45 => {
            let d = (WELL - PIP_1).normalize();
            let side = Vec2::new(-d.y, d.x);
            // at the boy's own eye-height (he's a head shorter than the grown-ups: any higher and
            // the lens looks over him, cropping him to a crown in the corner)
            let e = PIP_1 - d * 1.8 + side * 0.6;
            let eye = at(e.x, 1.0, e.y).lerp(at(e.x + d.x * 0.6, 1.0, e.y + d.y * 0.6), lin(11.1, 13.45));
            let look = well_pt() + Vec3::Y * 0.1;
            ((eye, look, 32.0, 0.55), Some(eye.distance(well_pt()) * 0.8))
        }
        // 4 · "And that… that was a chapel!" — he stops dead, turns to the ruin and
        //     points; we look past him at it (racking from the boy to the ruin).
        t if t < 17.05 => {
            let d = (RUIN - PIP_2).normalize();
            let side = Vec2::new(-d.y, d.x); // his right: the pointing arm on our side
            let e = PIP_2 - d * 2.2 + side * 0.7;
            let eye = at(e.x, 1.05, e.y).lerp(at(e.x - d.x * 0.3, 1.07, e.y - d.y * 0.3), lin(13.45, 17.05));
            let look = (ruin_pt() - Vec3::Y * 0.4).lerp(pip(0.8), 0.15);
            let (fp, fr) = (eye.distance(pip(1.0)), eye.distance(ruin_pt()));
            ((eye, look, 36.0, 0.55), Some(fp + (fr - fp) * env(t, &[(14.0, 0.0), (14.6, 1.0)])))
        }
        // 5 · "There's a bell! Up there!" — he spins round; low behind him, across the yard to the
        //     tower over the gate, his arm up at the belfry, Maud walking in.
        t if t < 19.6 => {
            let d = (BELL - PIP_2).normalize();
            let side = Vec2::new(d.y, -d.x);
            let e = PIP_2 - d * 1.7 + side * 0.5;
            let eye = at(e.x, 0.75, e.y);
            let look = (bell_pt() - Vec3::Y * 2.2).lerp(bell_pt() - Vec3::Y * 0.8, seg(17.3, 19.4));
            ((eye, look, 24.0, 0.45), Some(eye.distance(bell_pt())))
        }
        // 6 · "It's whole." — the bell itself in its belfry, morning sun on the bronze.
        //    Along shot 5's own sightline (through the belfry opening — any other angle puts a pier
        //    in front of it), a slow telephoto push in.
        t if t < 21.1 => {
            let d = (BELL - PIP_2).normalize();
            let e5 = PIP_2 - d * 1.7 + Vec2::new(d.y, -d.x) * 0.5;
            let dir = (at(e5.x, 0.75, e5.y) - bell_pt()).normalize();
            // slid right of the boy's sightline so the near pier clears the bell
            let right = Vec3::new(dir.z, 0.0, -dir.x).normalize();
            let eye = bell_pt() + dir * (9.5 - 1.3 * seg(19.6, 21.1)) + right * 1.3;
            ((eye, bell_pt() + Vec3::Y * 0.35, 22.0, 0.5), Some(eye.distance(bell_pt())))
        }
        // 7 · Maud: "The well's whole too. Start with the well." — over the boy's shoulder, low (his
        //     eye-line), up at her.
        t if t < 24.35 => {
            let d = (MAUD_MARK - PIP_2).normalize();
            let side = Vec2::new(d.y, -d.x);
            // tighter, the lens just off his ear: she fills the frame, his head a soft dark shape
            let e = PIP_2 - d * 0.85 + side * 0.36;
            let eye = at(e.x, 1.1, e.y).lerp(at(e.x + d.x * 0.2, 1.1, e.y + d.y * 0.2), lin(21.1, 24.35));
            let look = at(MAUD_MARK.x, 1.42, MAUD_MARK.y);
            ((eye, look, 17.0, 0.9), Some(eye.distance(look)))
        }
        // 8 · "Can I—" — the reverse, over her shoulder: the boy, arm up at the bell, asking.
        t if t < 25.3 => {
            let d = (PIP_2 - MAUD_MARK).normalize();
            let side = Vec2::new(-d.y, d.x);
            let e = MAUD_MARK - d * 1.05 + side * 0.36;
            let e = e - d * 0.5;
            let eye = at(e.x, 1.5, e.y);
            let look = pip(0.8);
            ((eye, look, 19.0, 0.9), Some(eye.distance(look)))
        }
        // 9 · "At dusk. King's orders, one of them." — her voice over the boy's face, close, from
        //     her side: the head going down, the well he's been told to start with behind him. (A
        //     side-on two-shot put them at opposite frame edges, ~8.7 u apart, the village between.)
        t if t < 28.15 => {
            let d = (MAUD_MARK - PIP_2).normalize();
            let side = Vec2::new(-d.y, d.x);
            let e = PIP_2 + d * 2.3 + side * 0.45;
            let eye = at(e.x, 0.98, e.y).lerp(at(e.x - d.x * 0.35, 0.96, e.y - d.y * 0.35), lin(25.3, 28.15));
            let look = pip(0.82);
            ((eye, look, 24.0, 1.0), Some(eye.distance(look)))
        }
        // 10 · "God help us…" — from behind Maud, the boy walking off to the well; the crane rises up
        //      and back over the gate wall: the whole ruin, two small figures, the light going. Ends
        //      low enough to keep the horizon (a high top-down just shows patchy mud).
        _ => {
            let k = seg(28.25, 35.95);
            let eye = at(3.5, 1.55, 11.2).lerp(Vec3::new(9.0, world_y_at(Vec2::new(9.0, 30.5)) + 7.0, 30.5), k);
            let look = pip(0.9).lerp(at(1.0, 2.0, -8.0), seg(28.25, 32.35));
            ((eye, look, 34.0 + 10.0 * k, 0.5 - 0.44 * k), None)
        }
    }
}

/// Clear the stage for the arrival — nobody lives here yet: the town's founding house / plots,
/// wandering townsfolk, wildlife and butterflies near the fort are hidden (not despawned) for the
/// clip and put back exactly as they were.
#[allow(clippy::type_complexity)]
fn fort_clear_stage(
    state: Res<SceneState>,
    mut q: Query<
        (Entity, &mut Visibility, &GlobalTransform),
        (
            Or<(
                With<crate::ambient_life::Butterfly>,
                With<crate::town::BuildPlot>,
                With<crate::town::BuildingMesh>,
                With<crate::villagers::Villager>,
                With<crate::wildlife::Animal>,
                With<crate::pasture::Sheep>,
                With<crate::pasture::SheepDog>,
                With<crate::pasture::Shepherd>,
            )>,
            Without<SceneActor>,
            Without<crate::castle::CastlePart>,
        ),
    >,
    // The settlers' houses + their yard dressing (`CastleKind::House`) and the courtyard's town-kit
    // buildings (`castle::town_plots`: barracks, granary, smithy… all but the well) — the colony
    // arrives to a ruin, nobody has built them yet. `castle::sync_castle` rewrites their Visibility every frame, so they're
    // sunk out of sight instead (translation only — collision is registered separately) and raised
    // back on teardown.
    mut parts: Query<(Entity, Option<&crate::castle::CastlePart>, &crate::build_fx::RevealAt, &mut Transform)>,
    mut hidden: Local<Vec<(Entity, Visibility)>>,
    mut sunk: Local<Option<Vec<(Entity, Vec3)>>>,
) {
    if state.active == Some(SceneId::FortArrival) {
        for (e, mut v, gt) in &mut q {
            let p = gt.translation();
            // (the pasture's dog + shepherd re-show themselves every frame: re-hidden each frame,
            // recorded once)
            if *v != Visibility::Hidden && Vec2::new(p.x, p.z).distance(Vec2::new(0.0, 8.0)) < 70.0 {
                if !hidden.iter().any(|(h, _)| *h == e) {
                    hidden.push((e, *v));
                }
                *v = Visibility::Hidden;
            }
        }
        // Every frame (the kit may spawn after the scene starts), but each part only once.
        let v = sunk.get_or_insert_with(Vec::new);
        for (e, part, at, mut tf) in &mut parts {
            let p = Vec2::new(at.0.x, at.0.z);
            let plot = crate::castle::town_plots().iter().any(|tp| tp.name != "well" && tp.pos.distance(p) < 0.05);
            let house = part.is_some_and(|c| matches!(c.kind, crate::castle::CastleKind::House(_)));
            if (plot || house || p.distance(NOTICE_BOARD) < 0.3) && !v.iter().any(|(s, _)| *s == e) {
                v.push((e, tf.translation));
                tf.translation.y -= 500.0;
            }
        }
    } else {
        for (e, was) in hidden.drain(..) {
            if let Ok((_, mut v, _)) = q.get_mut(e) {
                *v = was;
            }
        }
        for (e, p) in sunk.take().unwrap_or_default() {
            if let Ok((_, _, _, mut tf)) = parts.get_mut(e) {
                tf.translation = p;
            }
        }
    }
}

/// One fence bay (2 u wide, centred on the origin): two posts + two rails. `burnt` = scorched black.
fn fence_section_mesh(burnt: bool) -> Mesh {
    let (post, rail) = if burnt { (0x2a2520, 0x352d26) } else { (WOOD_DARK, WOOD) };
    merged(vec![
        cylr(0.05, 0.95, Vec3::new(-1.0, 0.475, 0.0), Quat::IDENTITY, post),
        cylr(0.05, 0.95, Vec3::new(1.0, 0.475, 0.0), Quat::IDENTITY, post),
        cube(2.0, 0.07, 0.05, Vec3::new(0.0, 0.40, 0.0), rail),
        cube(2.0, 0.07, 0.05, Vec3::new(0.0, 0.72, 0.0), rail),
    ])
}

/// A short stack of fresh-cut planks.
fn plank_pile_mesh() -> Mesh {
    merged(vec![
        cube(1.2, 0.05, 0.2, Vec3::new(0.0, 0.03, 0.0), 0xb08a58),
        cube(1.2, 0.05, 0.2, Vec3::new(0.02, 0.08, 0.22), 0xa87f4e),
        cube(1.2, 0.05, 0.2, Vec3::new(-0.03, 0.13, 0.1), 0xb89463),
        cube(1.1, 0.05, 0.2, Vec3::new(0.05, 0.18, 0.16), 0xa98250),
    ])
}

/// The planks the hauler carries (length along local +Z, the way he faces).
fn plank_bundle_mesh() -> Mesh {
    merged(vec![
        cube(0.14, 0.05, 1.3, Vec3::new(-0.075, 0.0, 0.0), 0xb08a58),
        cube(0.14, 0.05, 1.3, Vec3::new(0.075, 0.0, 0.0), 0xa87f4e),
        cube(0.14, 0.05, 1.2, Vec3::new(0.0, 0.055, 0.0), 0xb89463),
    ])
}

/// A scorched patch of ground: a flat soot disc.
fn scorch_patch_mesh() -> Mesh {
    tinted(Cylinder::new(1.0, 0.03).mesh().resolution(28).build().translated_by(Vec3::new(0.0, 0.015, 0.0)), 0x4a3c30)
}

/// A blackened stalk/stake left standing in the burnt field.
fn char_stake_mesh() -> Mesh {
    merged(vec![
        cylr(0.04, 0.8, Vec3::new(0.0, 0.4, 0.0), Quat::IDENTITY, 0x1b1713),
        cone_m(0.05, 0.12, Vec3::new(0.0, 0.86, 0.0), Quat::IDENTITY, 0x2a231d),
    ])
}

// ── Spawn helpers ──────────────────────────────────────────────────────────────────────

fn spawn_scene_ork(commands: &mut Commands, arm: &crate::orks::Armory, variant: OrkVariant, pos: Vec2, _facing: f32, role: SceneOrk) {
    // `arm.spawn` sets a correct ground transform + per-variant scale; `drive_scene_orks` re-poses
    // it (position + facing) every frame, so we only tag it here.
    let e = arm.spawn(commands, variant, Faction::Red, pos, pos, pos.x.to_bits() ^ pos.y.to_bits());
    commands.entity(e).insert((SceneActor, role));
}

fn spawn_prop_mesh(commands: &mut Commands, meshes: &mut Assets<Mesh>, assets: &SceneAssets, mesh: Mesh, pos: Vec3, yaw: f32, scale: f32) {
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(assets.prop_mat.clone()),
        Transform { translation: pos, rotation: Quat::from_rotation_y(yaw), scale: Vec3::splat(scale) },
        SceneProp,
    ));
}

fn spawn_torch(commands: &mut Commands, meshes: &mut Assets<Mesh>, assets: &SceneAssets, pos: Vec3) {
    // Post + a warm point light + flame cards on its head.
    spawn_prop_mesh(commands, meshes, assets, torch_post_mesh(), pos, 0.0, 1.0);
    commands.spawn((
        Mesh3d(assets.flame_mesh.clone()),
        MeshMaterial3d(assets.flame_mat.clone()),
        Transform::from_translation(pos + Vec3::new(0.0, 1.2, 0.0)),
        bevy::light::NotShadowCaster,
        crate::camps::Flicker { phase: pos.x * 1.3 },
        SceneProp,
    ));
    commands.spawn((
        PointLight { color: Color::srgb(1.0, 0.6, 0.25), intensity: 18_000.0, range: 9.0, shadow_maps_enabled: false, ..default() },
        Transform::from_translation(pos + Vec3::new(0.0, 1.3, 0.0)),
        SceneProp,
    ));
}

// ── Prop meshes (vertex-coloured, flat-shaded — the shared-material contract) ───────────

fn cube(w: f32, h: f32, d: f32, off: Vec3, hex: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), hex)
}
fn cylr(r: f32, h: f32, off: Vec3, rot: Quat, hex: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(10).build().rotated_by(rot).translated_by(off), hex)
}
fn cone_m(r: f32, h: f32, off: Vec3, rot: Quat, hex: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), hex)
}

const WOOD: u32 = 0x6b4a2c;
const WOOD_DARK: u32 = 0x4a3320;
const IRON: u32 = 0x4b4f57;
const STONE_C: u32 = 0x8a8d92;

/// A spade planted in the earth, blade down, leaning (the entity tilts it).
fn shovel_mesh() -> Mesh {
    merged(vec![
        cylr(0.035, 1.5, Vec3::new(0.0, 0.85, 0.0), Quat::IDENTITY, WOOD), // shaft
        cube(0.22, 0.04, 0.05, Vec3::new(0.0, 1.6, 0.0), WOOD_DARK),       // T-grip
        cube(0.24, 0.3, 0.04, Vec3::new(0.0, 0.18, 0.0), IRON),           // blade
    ])
}

/// A short pile of cut stone blocks.
fn stone_pile_mesh() -> Mesh {
    merged(vec![
        cube(1.0, 0.3, 0.7, Vec3::new(0.0, 0.15, 0.0), STONE_C),
        cube(0.8, 0.28, 0.55, Vec3::new(0.05, 0.43, 0.0), 0x9a9da2),
        cube(0.5, 0.26, 0.4, Vec3::new(-0.1, 0.69, 0.05), STONE_C),
    ])
}

/// A half-built wall: a full base course plus raised end stacks, leaving the gap in the top
/// course the mason's three stones fill (slots at [`MASON_STONE_X`]).
fn wall_course_mesh() -> Mesh {
    merged(vec![
        cube(2.6, 0.40, 0.50, Vec3::new(0.0, 0.20, 0.0), STONE_C),
        cube(0.62, 0.26, 0.46, Vec3::new(-0.95, 0.53, 0.0), 0x9a9da2),
        cube(0.62, 0.26, 0.46, Vec3::new(0.95, 0.53, 0.0), 0x84878c),
    ])
}

/// One of the mason's wall stones (base at y=0, per the mesh contract — the slot transform seats
/// it on the course top).
fn mason_stone_mesh() -> Mesh {
    merged(vec![cube(0.32, 0.26, 0.42, Vec3::new(0.0, 0.13, 0.0), 0x9a9da2)])
}

/// A wooden barrel — staves + three iron hoops + a lid.
fn barrel_mesh() -> Mesh {
    let mut p = vec![
        cylr(0.34, 0.9, Vec3::new(0.0, 0.45, 0.0), Quat::IDENTITY, WOOD),
        cylr(0.30, 0.05, Vec3::new(0.0, 0.92, 0.0), Quat::IDENTITY, WOOD_DARK), // lid
    ];
    for y in [0.12, 0.45, 0.78] {
        p.push(cylr(0.36, 0.06, Vec3::new(0.0, y, 0.0), Quat::IDENTITY, IRON));
    }
    merged(p)
}

/// A bell hung in a small A-frame (two posts + crossbeam + a flared bell + clapper).
fn bell_mesh() -> Mesh {
    let post = |x: f32| cylr(0.06, 1.5, Vec3::new(x, 0.75, 0.0), Quat::IDENTITY, WOOD);
    merged(vec![
        post(-0.45),
        post(0.45),
        cube(1.1, 0.1, 0.12, Vec3::new(0.0, 1.45, 0.0), WOOD_DARK), // crossbeam
        cone_m(0.26, 0.42, Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY, 0xb8892f), // bell body (apex up)
        cylr(0.27, 0.06, Vec3::new(0.0, 0.8, 0.0), Quat::IDENTITY, 0x9a6f22),    // bell lip
        cylr(0.04, 0.18, Vec3::new(0.0, 0.86, 0.0), Quat::IDENTITY, IRON),       // clapper
    ])
}

/// A campfire — a ring of logs around a charred core (the firelight is a separate scene light if
/// wanted; the emissive embers here keep it lit even by day).
fn campfire_mesh() -> Mesh {
    let mut p = vec![cube(0.7, 0.08, 0.7, Vec3::new(0.0, 0.04, 0.0), 0x2a2018)]; // ash bed
    for i in 0..5 {
        let a = i as f32 / 5.0 * std::f32::consts::TAU;
        p.push(cylr(0.07, 0.7, Vec3::new(a.cos() * 0.28, 0.18, a.sin() * 0.28), Quat::from_rotation_x(1.1) * Quat::from_rotation_y(a), WOOD));
    }
    merged(p)
}

/// A wall torch — a short post with a pitch-wrapped head; the flame cards + point light are
/// spawned alongside ([`spawn_torch`]).
fn torch_post_mesh() -> Mesh {
    merged(vec![
        cylr(0.05, 1.2, Vec3::new(0.0, 0.6, 0.0), Quat::IDENTITY, WOOD_DARK),
        cylr(0.075, 0.14, Vec3::new(0.0, 1.17, 0.0), Quat::IDENTITY, 0x2c2019),
    ])
}

// ── Small utilities ────────────────────────────────────────────────────────────────────

fn pin_sky(clock: &mut crate::scene::SkyClock, t: f32) {
    clock.t = t;
    clock.paused = true;
}
fn world_y(x: f32, z: f32) -> Vec3 {
    Vec3::new(x, crate::worldmap::ground_at_world(x, z).unwrap_or(0.0), z)
}
fn world_y_at(p: Vec2) -> f32 {
    crate::worldmap::ground_at_world(p.x, p.y).unwrap_or(0.0)
}
fn variant_cycle(i: u32) -> OrkVariant {
    match i % 4 {
        0 => OrkVariant::Grunt,
        1 => OrkVariant::Scout,
        2 => OrkVariant::Berserker,
        _ => OrkVariant::Shaman,
    }
}
/// The ork camp nearest the castle (for the flee scene), if any are placed.
fn nearest_camp() -> Option<Vec2> {
    crate::camps::cage_positions()
        .into_iter()
        .map(|(_, centre)| centre)
        .min_by(|a, b| a.length().partial_cmp(&b.length()).unwrap())
}
