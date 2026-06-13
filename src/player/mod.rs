//! **Playable hero** — a knight the user drives in third-person, ported from the TS game
//! (`src/world/Character.tsx` + `MouseLookCamera.tsx`). Decomposed into focused systems:
//! [`model`] (the knight mesh), [`movement`] (WASD + jump + terrain collision),
//! [`camera`] (over-the-shoulder orbit + pointer-lock + the free-roam debug toggle) and
//! [`anim`] (limb drivers). Combat / block / health land in later milestones.
//!
//! The scene is world-space (castle at the origin, no centring group), so the hero stores
//! its position as a world `Vec2` like the orks and grounds on `worldmap::ground_at_world`.

mod anim;
mod block;
mod camera;
mod combat;

pub(crate) use combat::{
    spawn_burst, spawn_chips, spawn_heal_burst, spawn_motes, spawn_shockwave, CombatFx, Health,
};
mod health;
mod model;
mod movement;

use bevy::prelude::*;

use crate::inventory::Inventory;

/// Root scale applied to the TS-unit knight so it stands the same height as the orks
/// (`orks::BASE_SCALE` is 0.7; the knight authors ~1.25u tall → ~0.87u on the ground).
pub const HERO_SCALE: f32 = 0.612;

/// Which articulated limb a hero child mesh is, so [`anim`] can pose it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HeroLimb {
    LegR,
    LegL,
    ArmR,
    ArmL,
    Head,
    Shield,
}

#[derive(Component)]
pub struct HeroPart {
    pub limb: HeroLimb,
}

/// The held weapon mesh — a child of the `ArmR` entity (so it swings with the arm) that can be
/// toggled `Visibility::Hidden` for weapon-free staged gestures (the Director's "hide weapon").
#[derive(Component)]
pub struct HeroWeapon;

/// The hero's hot per-frame state (mutated directly each frame, never via events).
#[derive(Component)]
pub struct Hero {
    /// World XZ.
    pub pos: Vec2,
    pub y: f32,
    pub facing: f32,
    pub vel_y: f32,
    pub on_ground: bool,
    pub air_takeoff_y: f32,
    pub walk_phase: f32,
    /// 0..1 smooth blend tracking `moving` (drives anim weight).
    pub moving_amt: f32,
    pub moving: bool,
    // ── Attack (M2) ──
    pub attacking: bool,
    /// Seconds into the current swing.
    pub attack_t: f32,
    /// Whether this swing's cone-damage has already been applied.
    pub hit_dealt: bool,
}

/// Hero **shield/stamina** state — only the block mechanic. HP, gold, XP/level and the combat
/// stats live on [`PlayerRes`] (the single progression home), so this carries no `hp`/`dead`.
#[derive(Component)]
pub struct HeroHealth {
    pub stamina: f32,
    pub stamina_max: f32,
    pub block_locked: bool,
    pub regen_pause: f32,
    pub blocking: bool,
}

impl Default for HeroHealth {
    fn default() -> Self {
        HeroHealth {
            stamina: 150.0,
            stamina_max: 150.0,
            block_locked: false,
            regen_pause: 0.0,
            blocking: false,
        }
    }
}

/// The hero's **progression + combat state** — HP, gold, XP/level and the upgrade-tree combat
/// stats (crit / lifesteal / cleave / move-speed / bounty / attack-damage). The single source
/// of truth that combat, the economy and the upgrade tree all read & write; the live pose stays
/// on the [`Hero`] component. Adopted wholesale from the test-gated `tileworld_core::player`
/// (125 HP, 30 starting gold, the TS xp/level curves).
#[derive(Resource, Default)]
pub struct PlayerRes(pub tileworld_core::player::Player);

/// The shared creature material every hero mesh uses (vertex colours carry the hue; the shader
/// adds per-surface texture from the alpha-packed surf code). Stored so [`reskin_hero`] can
/// rebuild the limb meshes against the same material on an equip change.
#[derive(Resource)]
pub struct HeroMaterial(pub Handle<crate::creature::CreatureMaterial>);

/// Control mode. **Play** drives the knight + follow-cam; **FreeRoam** hands the camera back
/// to `controls::FlyCam` for debugging. Toggle with the backtick key.
#[derive(Resource, PartialEq, Eq, Clone, Copy)]
pub enum PlayMode {
    Play,
    FreeRoam,
}

/// Hero world pose mirrored into a resource at the end of movement, so other systems
/// (ork AI in M3, the camera) read a resource instead of cross-querying the hero entity.
#[derive(Resource, Default)]
#[allow(dead_code)]
pub struct HeroState {
    pub pos: Vec2,
    pub y: f32,
    pub facing: f32,
    pub alive: bool,
}

/// Damage the orks have dealt the hero since the last health tick. Orks accumulate onto it
/// (`+=`); [`health::apply_hero_damage`] drains it once per frame. Mirrors the TS store-
/// mediated combat channel — no collision events.
#[derive(Resource, Default)]
pub struct PendingHeroDamage(pub f32);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // Capture screenshots hold the scene's static overview camera → start in FreeRoam
        // so the follow-cam never hijacks the shot (the hero still spawns, at rest).
        let start_mode = if std::env::var("FOREST_SHOT").is_ok()
            || std::env::var("FOREST_CLIP").is_ok()
        {
            PlayMode::FreeRoam
        } else {
            PlayMode::Play
        };
        app.insert_resource(start_mode)
            .init_resource::<HeroState>()
            .init_resource::<PendingHeroDamage>()
            .init_resource::<PlayerRes>()
            .init_resource::<combat::CombatRng>()
            .init_resource::<combat::HitStop>()
            .insert_resource(camera::OrbitCam::default())
            .add_systems(Startup, combat::setup_combat_fx)
            .add_systems(PostStartup, spawn_hero)
            // Fresh run: wipe progression + revive the hero on a new run (NOT on un-pause).
            .add_systems(
                OnExit(crate::game_state::AppState::StartScreen),
                reset_player,
            )
            .add_systems(OnExit(crate::game_state::AppState::GameOver), reset_player)
            // Render/input — keep running even when the world is frozen (so the paused scene
            // still draws + you can leave free-roam). `toggle_mode` is the backtick free-cam.
            .add_systems(
                Update,
                (
                    camera::toggle_mode,
                    camera::player_camera,
                    reskin_hero, // rebuild limb meshes when weapon/armor equip changes
                    anim::hero_anim,
                    combat::update_sparks,
                    combat::update_fx_fades,
                    combat::hero_blade_trail,
                    combat::drive_hit_stop, // ungated: must resume the clock after the freeze
                ),
            )
            // World-sim — gated on the freeze condition (`Modal::None` ⇒ Playing, no panel).
            // `player_move`/`attack` also early-return outside `PlayMode::Play` (free-roam).
            .add_systems(
                Update,
                (
                    movement::player_move,
                    block::player_block,
                    combat::player_attack,
                    combat::ensure_combat_health,
                    health::apply_hero_damage,
                    health::hero_death_anim, // keel-over pose; last so it owns the dead transform
                )
                    .chain()
                    .run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

fn spawn_hero(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::creature::CreatureMaterial>>,
    inv: Res<Inventory>,
) {
    // One shared creature material; colour lives in the mesh vertex colours and surface texture
    // comes from the alpha-packed surf code (plate=Metal recreates the old metallic sheen).
    let mat = crate::creature::make_creature_material(&mut materials);
    commands.insert_resource(HeroMaterial(mat.clone()));

    // Spawn just outside the north gate, facing into the courtyard (+Z toward origin).
    let gate = crate::castle::gate_centers()[0];
    // Debug/screenshot hook: `FOREST_HERO="x,z"` drops the hero at a world XZ (e.g. deep in a
    // biome region) so a capture shows that biome's reactive atmosphere/weather.
    let staged = std::env::var("FOREST_HERO").ok().and_then(|s| {
        let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        (v.len() == 2).then(|| Vec2::new(v[0], v[1]))
    });
    let pos = staged.unwrap_or(Vec2::new(gate.x, gate.y - 3.0));
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let facing = 0.0_f32;

    let root = commands
        .spawn((
            Transform {
                translation: Vec3::new(pos.x, y, pos.y),
                rotation: Quat::from_rotation_y(facing),
                scale: Vec3::splat(HERO_SCALE),
            },
            Visibility::Visible,
            Hero {
                pos,
                y,
                facing,
                vel_y: 0.0,
                on_ground: true,
                air_takeoff_y: y,
                walk_phase: 0.0,
                moving_amt: 0.0,
                moving: false,
                attacking: false,
                attack_t: 0.0,
                hit_dealt: false,
            },
            HeroHealth::default(),
        ))
        .id();

    // Build the limb meshes reflecting whatever's equipped (bare on a fresh run).
    let spec = model::build_knight(
        inv.0.equipped_id.as_deref(),
        inv.0.equipped_armor_id.as_deref(),
    );
    spawn_hero_meshes(&mut commands, root, spec, &mut meshes, &mat);

    // When staged into a biome for a screenshot, mirror the pose into `HeroState` now so the
    // reactive atmosphere/weather pick up that region immediately (in FreeRoam capture mode
    // `player_move` doesn't run, so it never would otherwise).
    if staged.is_some() {
        commands.insert_resource(HeroState { pos, y, facing, alive: true });
    }
}

/// Spawn the torso + articulated limb meshes as children of the hero `root`, all sharing the
/// hero material. Shared by [`spawn_hero`] and [`reskin_hero`] so an equip swap rebuilds the
/// exact same child layout.
fn spawn_hero_meshes(
    commands: &mut Commands,
    root: Entity,
    spec: model::KnightSpec,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<crate::creature::CreatureMaterial>,
) {
    let model::KnightSpec { torso, parts, weapon, weapon_xf } = spec;
    let torso = meshes.add(torso);
    let weapon = meshes.add(weapon);
    commands.entity(root).with_children(|p| {
        p.spawn((Mesh3d(torso), MeshMaterial3d(mat.clone()), Transform::default()));
        for part in parts {
            let is_arm_r = part.limb == HeroLimb::ArmR;
            let mut ec = p.spawn((
                Mesh3d(meshes.add(part.mesh)),
                MeshMaterial3d(mat.clone()),
                Transform { translation: part.pivot, rotation: part.rest, ..default() },
                HeroPart { limb: part.limb },
            ));
            // Nest the weapon under the sword arm so it inherits the swing but can be hidden.
            if is_arm_r {
                ec.with_children(|a| {
                    a.spawn((
                        Mesh3d(weapon.clone()),
                        MeshMaterial3d(mat.clone()),
                        weapon_xf,
                        HeroWeapon,
                    ));
                });
            }
        }
    });
}

/// Rebuild the hero's limb meshes when the equipped weapon/armor changes (the satchel equips
/// freeze the world, so this ungated render system rebuilds behind the panel and on close).
/// Despawns the old children and re-spawns from a fresh [`model::build_knight`]; the `Hero`
/// root + its components are untouched, and [`anim::hero_anim`] re-binds the new `HeroPart`s
/// next frame. Change-detected + snapshot-gated so it only fires on an actual equip swap.
fn reskin_hero(
    mut commands: Commands,
    inv: Res<Inventory>,
    mat: Option<Res<HeroMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    hero_q: Query<(Entity, Option<&Children>), With<Hero>>,
    mut last: Local<(Option<String>, Option<String>)>,
) {
    if !inv.is_changed() {
        return;
    }
    let cur = (inv.0.equipped_id.clone(), inv.0.equipped_armor_id.clone());
    if cur == *last {
        return; // no equip change (some other bag mutation)
    }
    let Some(mat) = mat else { return };
    let Ok((root, children)) = hero_q.single() else { return };
    *last = cur.clone();

    if let Some(children) = children {
        for &c in children {
            commands.entity(c).try_despawn();
        }
    }
    let spec = model::build_knight(cur.0.as_deref(), cur.1.as_deref());
    spawn_hero_meshes(&mut commands, root, spec, &mut meshes, &mat.0);
}

/// Reset the hero to a fresh run: wipe progression (`Player::reset` → full HP, 30 gold, level 1,
/// neutral combat stats) and revive him at the north gate. Runs when a run (re)starts — leaving the
/// start screen, or leaving game-over (a fresh run relaunches, but an in-process **Continue** also
/// exits game-over here: the wipe is harmless then, as `savegame::apply_pending_load` immediately
/// overwrites the progression from the save while this revival of the hero entity stands). Never
/// on un-pause.
fn reset_player(
    mut player: ResMut<PlayerRes>,
    siege: Option<Res<crate::siege::Siege>>,
    mut hero_q: Query<(&mut Hero, &mut Transform, &mut HeroHealth)>,
) {
    player.0.reset();
    // Difficulty handicap: Easy gives the hero a bigger HP pool so a beginner survives early mistakes.
    let diff = siege.map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);
    let m = crate::siege::mods_for(diff).player_hp_mul as f64;
    if m != 1.0 {
        player.0.max_hp *= m;
        player.0.hp = player.0.max_hp;
    }
    let Ok((mut hero, mut tf, mut hh)) = hero_q.single_mut() else { return };
    let gate = crate::castle::gate_centers()[0];
    let pos = Vec2::new(gate.x, gate.y - 3.0);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    *hero = Hero {
        pos,
        y,
        facing: 0.0,
        vel_y: 0.0,
        on_ground: true,
        air_takeoff_y: y,
        walk_phase: 0.0,
        moving_amt: 0.0,
        moving: false,
        attacking: false,
        attack_t: 0.0,
        hit_dealt: false,
    };
    tf.translation = Vec3::new(pos.x, y, pos.y);
    tf.rotation = Quat::from_rotation_y(0.0);
    tf.scale = Vec3::splat(HERO_SCALE);
    *hh = HeroHealth::default();
}
