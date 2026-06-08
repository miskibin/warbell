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

pub(crate) use combat::{spawn_burst, spawn_heal_burst, CombatFx, Health};
mod health;
mod model;
mod movement;

use bevy::prelude::*;

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
            stamina: 100.0,
            stamina_max: 100.0,
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
        let start_mode =
            if std::env::var("FOREST_SHOT").is_ok() { PlayMode::FreeRoam } else { PlayMode::Play };
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
                    anim::hero_anim,
                    combat::update_sparks,
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
                )
                    .chain()
                    .run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

fn spawn_hero(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let spec = model::build_knight();
    // One shared material; colour lives in the mesh vertex colours (orks/critters pattern).
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.45,
        metallic: 0.3,
        ..default()
    });

    // Spawn just outside the north gate, facing into the courtyard (+Z toward origin).
    let gate = crate::castle::gate_centers()[0];
    let pos = Vec2::new(gate.x, gate.y - 3.0);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let facing = 0.0_f32;

    let torso = meshes.add(spec.torso);
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

    commands.entity(root).with_children(|p| {
        p.spawn((Mesh3d(torso), MeshMaterial3d(mat.clone()), Transform::default()));
        for part in spec.parts {
            p.spawn((
                Mesh3d(meshes.add(part.mesh)),
                MeshMaterial3d(mat.clone()),
                Transform { translation: part.pivot, rotation: part.rest, ..default() },
                HeroPart { limb: part.limb },
            ));
        }
    });
}

/// Reset the hero to a fresh run: wipe progression (`Player::reset` → full HP, 30 gold, level 1,
/// neutral combat stats) and revive him at the north gate. Runs only when a new run begins
/// (leaving the start screen / game-over), never on un-pause.
fn reset_player(
    mut player: ResMut<PlayerRes>,
    mut hero_q: Query<(&mut Hero, &mut Transform, &mut HeroHealth)>,
) {
    player.0.reset();
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
