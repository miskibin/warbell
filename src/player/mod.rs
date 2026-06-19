//! **Playable hero** — a knight the user drives in third-person, ported from the TS game
//! (`src/world/Character.tsx` + `MouseLookCamera.tsx`). Decomposed into focused systems:
//! [`model`] (the knight mesh), [`movement`] (WASD + jump + terrain collision),
//! [`camera`] (over-the-shoulder orbit + pointer-lock + the free-roam debug toggle) and
//! [`anim`] (limb drivers). Combat / block / health land in later milestones.
//!
//! The scene is world-space (castle at the origin, no centring group), so the hero stores
//! its position as a world `Vec2` like the orks and grounds on `worldmap::ground_at_world`.

mod anim;
mod arts;
mod block;
mod camera;
mod combat;

pub(crate) use combat::{
    spawn_burst, spawn_chips, spawn_dash_trail, spawn_heal_burst, spawn_motes, spawn_shockwave,
    spawn_sweep_burst, CombatFx, Health,
};
mod health;
mod model;
mod movement;

/// First-person view state, toggled by the HUD eye button ([`crate::ui::settings`]) and the V key.
pub use camera::FirstPerson;

use bevy::prelude::*;

use crate::inventory::Inventory;

/// Root scale applied to the TS-unit knight so it stands the same height as the orks
/// (`orks::BASE_SCALE` is 0.7; the knight authors ~1.85u tall → ~0.93u on the ground).
pub const HERO_SCALE: f32 = 0.5;

/// A rig **joint** — a transform-only entity the animator ([`anim`]) poses. Each joint's mesh is a
/// separate child *leaf* entity ([`HeroMesh`]), so first-person can hide the body meshes without
/// hiding the arm joints that hang beneath the torso. (Hands / neck / feet are unanimated, so they
/// carry no `HeroPart`.)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    Hips,
    Torso,
    Head,
    Plume,
    ShoulderL,
    ShoulderR,
    ElbowL,
    ElbowR,
    HipL,
    HipR,
    KneeL,
    KneeR,
    Shield,
}

#[derive(Component)]
pub struct HeroPart {
    pub joint: Joint,
}

/// A body **mesh leaf** (child of a joint). `fp_keep` meshes — the arms, shield and sword — stay
/// visible in first-person; the rest (head/plume/torso/hips/legs) are hidden by [`camera`].
#[derive(Component)]
pub struct HeroMesh {
    pub fp_keep: bool,
}

/// The held weapon mesh leaf (under the right hand). Toggled `Visibility::Hidden` for weapon-free
/// staged gestures (the Director's "hide weapon"), and read by `combat::hero_blade_trail`.
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
    /// `elapsed_secs` until which the hero is invulnerable (Sand-Dash i-frames). `apply_hero_damage`
    /// negates incoming blows while `now < iframe_until`.
    pub iframe_until: f32,
    /// `elapsed_secs` until which no weapon art may fire (the shared post-cast cooldown). Stamina
    /// gates *how many* casts you can afford; this just spaces them out so they can't fire same-frame.
    pub art_cd_until: f32,
}

impl Default for HeroHealth {
    fn default() -> Self {
        HeroHealth {
            stamina: 150.0,
            stamina_max: 150.0,
            block_locked: false,
            regen_pause: 0.0,
            blocking: false,
            iframe_until: 0.0,
            art_cd_until: 0.0,
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

/// Set true the frame a warden's telegraphed **critical** lands on the hero. Read by
/// [`health::apply_hero_damage`]: a critical that connects is LETHAL (one-shot) unless the hero is
/// blocking or mid-dodge, which negates it — so the windup is the player's cue to raise the shield.
#[derive(Resource, Default)]
pub struct PendingCrit(pub bool);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // Capture screenshots hold the scene's static overview camera → start in FreeRoam
        // so the follow-cam never hijacks the shot (the hero still spawns, at rest). `FOREST_FP`
        // forces Play + first-person so the eye-view can be captured (it needs the follow-cam).
        // `FOREST_FREEROAM=1` boots into the fly-cam *without* capturing/exiting — so a fixed
        // `FOREST_CAM` view holds (the fly-cam stays put with no input), giving a pinned, identical
        // frame to A/B perf changes (e.g. `FOREST_NOCULL` on/off) off the F2 overlay.
        let fp_boot = std::env::var("FOREST_FP").is_ok();
        let start_mode = if fp_boot {
            PlayMode::Play
        } else if std::env::var("FOREST_SHOT").is_ok()
            || std::env::var("FOREST_CLIP").is_ok()
            || std::env::var("FOREST_FREEROAM").is_ok()
        {
            PlayMode::FreeRoam
        } else {
            PlayMode::Play
        };
        app.insert_resource(start_mode)
            .init_resource::<HeroState>()
            .init_resource::<PendingHeroDamage>()
            .init_resource::<PendingCrit>()
            .init_resource::<PlayerRes>()
            .init_resource::<combat::CombatRng>()
            .init_resource::<combat::HitStop>()
            .insert_resource(camera::OrbitCam::default())
            .insert_resource(camera::FirstPerson { active: fp_boot, ..default() })
            .add_systems(Startup, combat::setup_combat_fx)
            .add_systems(PostStartup, (spawn_hero, arts::spawn_arts_hud, debug_grant_boons))
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
                    camera::toggle_first_person, // V / HUD eye button: third ⇄ first person
                    camera::player_camera,
                    camera::fp_body_visibility, // FP viewmodel: keep arms/sword/shield, hide the rest
                    reskin_hero, // rebuild limb meshes when weapon/armor equip changes
                    anim::hero_anim,
                    combat::update_sparks,
                    combat::update_fx_fades,
                    combat::hero_blade_trail,
                    combat::drive_hit_stop, // ungated: must resume the clock after the freeze
                    arts::apply_knock, // ungated: fold queued slam knockbacks into ork kb
                    arts::sync_arts_hud, // ability-chip HUD (show/dim per readiness)
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
                    arts::player_arts, // warden weapon arts (after move/attack so a dash sticks)
                    combat::ensure_combat_health,
                    health::apply_hero_damage,
                    health::hero_death_anim, // keel-over pose; last so it owns the dead transform
                )
                    .chain()
                    .run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

/// Debug/screenshot hook: `FOREST_BOONS=1` grants all five warden boons at startup so the
/// ability HUD + the active moves can be exercised without first slaying every boss.
fn debug_grant_boons(mut player: ResMut<PlayerRes>) {
    if std::env::var("FOREST_BOONS").is_err() {
        return;
    }
    let p = &mut player.0;
    p.has_ground_slam = true;
    p.has_sand_dash = true;
    p.has_bramble_sweep = true;
    p.frostbite = true;
    p.venom = true;
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

/// Spawn a joint entity (transform-only, optionally `HeroPart`-tagged for the animator), parented
/// under `parent`, returning it so children can nest beneath. An optional mesh `leaf` is spawned as
/// a separate child entity (so first-person can toggle body meshes without hiding child joints).
struct Leaf {
    mesh: Handle<Mesh>,
    fp_keep: bool,
    weapon: bool,
}
fn spawn_joint(
    commands: &mut Commands,
    parent: Entity,
    tag: Option<Joint>,
    xf: Transform,
    mat: &Handle<crate::creature::CreatureMaterial>,
    leaf: Option<Leaf>,
) -> Entity {
    let mut ec = commands.spawn((xf, Visibility::Visible));
    if let Some(j) = tag {
        ec.insert(HeroPart { joint: j });
    }
    let joint = ec.id();
    commands.entity(parent).add_child(joint);
    if let Some(l) = leaf {
        let mut le = commands.spawn((
            Mesh3d(l.mesh),
            MeshMaterial3d(mat.clone()),
            Transform::default(),
            HeroMesh { fp_keep: l.fp_keep },
        ));
        if l.weapon {
            le.insert(HeroWeapon);
        }
        let leaf_e = le.id();
        commands.entity(joint).add_child(leaf_e);
    }
    joint
}

/// Spawn the full articulated knight (hips → torso → neck → head + plume; shoulder → elbow → hand
/// + weapon/shield; hip → knee → foot) as children of the hero `root`, all sharing the hero
/// material. Shared by [`spawn_hero`] and [`reskin_hero`] so an equip swap rebuilds the same tree.
fn spawn_hero_meshes(
    commands: &mut Commands,
    root: Entity,
    m: model::KnightMeshes,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<crate::creature::CreatureMaterial>,
) {
    use Joint::*;
    let p = |t: Vec3| Transform::from_translation(t);
    let body = |mesh: Handle<Mesh>| Some(Leaf { mesh, fp_keep: false, weapon: false });
    let arm = |mesh: Handle<Mesh>| Some(Leaf { mesh, fp_keep: true, weapon: false });

    // A −0.06 rig offset drops the authored feet (bottom ~+0.06) onto the root's ground plane.
    let rig = commands.spawn((Transform::from_xyz(0.0, -0.06, 0.0), Visibility::Visible)).id();
    commands.entity(root).add_child(rig);

    // Spine.
    let hips = spawn_joint(commands, rig, Some(Hips), p(Vec3::new(0.0, 0.95, 0.0)), mat, body(meshes.add(m.hips)));
    let torso = spawn_joint(commands, hips, Some(Torso), p(Vec3::new(0.0, 0.15, 0.0)), mat, body(meshes.add(m.torso)));
    let neck = spawn_joint(commands, torso, None, p(Vec3::new(0.0, 0.35, 0.0)), mat, body(meshes.add(m.neck)));
    let head = spawn_joint(commands, neck, Some(Head), p(Vec3::new(0.0, 0.08, 0.0)), mat, body(meshes.add(m.head)));
    spawn_joint(commands, head, Some(Plume), p(Vec3::new(0.0, 0.14, -0.08)), mat, body(meshes.add(m.plume)));

    // Left arm + lion-emblem heater shield (its own pivot on the forearm).
    let sh_l = spawn_joint(commands, torso, Some(ShoulderL), p(Vec3::new(-0.44, 0.27, 0.0)), mat, arm(meshes.add(m.shoulder_l)));
    let el_l = spawn_joint(commands, sh_l, Some(ElbowL), p(Vec3::new(0.0, -0.28, 0.0)), mat, arm(meshes.add(m.elbow_l)));
    let hand_l = spawn_joint(commands, el_l, None, p(Vec3::new(0.0, -0.25, 0.0)), mat, None);
    let shield = spawn_joint(
        commands,
        hand_l,
        Some(Shield),
        Transform { translation: Vec3::new(-0.13, 0.03, 0.08), rotation: Quat::from_euler(EulerRot::XYZ, 0.2, -0.6, 0.15), scale: Vec3::splat(0.92) },
        mat,
        arm(meshes.add(m.shield)),
    );
    spawn_joint(commands, shield, None, p(Vec3::new(0.0, -0.03, 0.033)), mat, arm(meshes.add(m.lion)));

    // Right arm + held weapon.
    let sh_r = spawn_joint(commands, torso, Some(ShoulderR), p(Vec3::new(0.44, 0.27, 0.0)), mat, arm(meshes.add(m.shoulder_r)));
    let el_r = spawn_joint(commands, sh_r, Some(ElbowR), p(Vec3::new(0.0, -0.28, 0.0)), mat, arm(meshes.add(m.elbow_r)));
    let hand_r = spawn_joint(commands, el_r, None, p(Vec3::new(0.0, -0.25, 0.0)), mat, None);
    spawn_joint(commands, hand_r, None, m.weapon_xf, mat, Some(Leaf { mesh: meshes.add(m.weapon), fp_keep: true, weapon: true }));

    // Legs.
    let hip_l = spawn_joint(commands, hips, Some(HipL), p(Vec3::new(-0.19, -0.05, 0.0)), mat, body(meshes.add(m.hip_l)));
    let knee_l = spawn_joint(commands, hip_l, Some(KneeL), p(Vec3::new(0.0, -0.38, 0.0)), mat, body(meshes.add(m.knee_l)));
    spawn_joint(commands, knee_l, None, p(Vec3::new(0.0, -0.38, 0.0)), mat, body(meshes.add(m.foot_l)));
    let hip_r = spawn_joint(commands, hips, Some(HipR), p(Vec3::new(0.19, -0.05, 0.0)), mat, body(meshes.add(m.hip_r)));
    let knee_r = spawn_joint(commands, hip_r, Some(KneeR), p(Vec3::new(0.0, -0.38, 0.0)), mat, body(meshes.add(m.knee_r)));
    spawn_joint(commands, knee_r, None, p(Vec3::new(0.0, -0.38, 0.0)), mat, body(meshes.add(m.foot_r)));
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
