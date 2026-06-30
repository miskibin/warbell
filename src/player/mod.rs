//! **Playable hero** — a knight the user drives in third-person, ported from the TS game
//! (`src/world/Character.tsx` + `MouseLookCamera.tsx`). Decomposed into focused systems:
//! [`model`] (the knight mesh), [`movement`] (WASD + jump + terrain collision),
//! [`camera`] (over-the-shoulder orbit + pointer-lock + the free-roam debug toggle) and
//! [`anim`] (limb drivers). Combat / block / health land in later milestones.
//!
//! The scene is world-space (castle at the origin, no centring group), so the hero stores
//! its position as a world `Vec2` like the orks and grounds on `worldmap::ground_at_world`.

pub(crate) mod anim;
mod arts;
mod block;
mod camera;
mod charge;
mod combat;

pub(crate) use combat::{
    spawn_burst, spawn_chips, spawn_dash_trail, spawn_heal_burst, spawn_motes, spawn_shockwave,
    spawn_sweep_burst, CombatFx, Health,
};
/// Swing length (seconds) — exposed so the standalone viewer can loop a preview swing.
pub use combat::ATTACK_DURATION;
mod health;
pub(crate) mod model;
mod movement;

/// First-person view state, toggled by the HUD eye button ([`crate::ui::settings`]) and the V key.
pub use camera::FirstPerson;
/// Sand-Dash slide duration — re-exported so the standalone viewer (`viewer.rs`) can drive the
/// dash-swipe preview at the real cadence. (`anim` reads it directly via `super::movement`.)
pub(crate) use movement::DASH_TIME;

use bevy::prelude::*;

use crate::inventory::Inventory;

/// Root scale applied to the TS-unit knight. Was 0.47 (≈ ork height); now ~1.35× that (0.47 × 1.5,
/// then dialled back 10%) so the hero reads clearly in the third-person frame without towering as a
/// giant. The rest of the human-scale world (orks, townsfolk, castle houses, town buildings) is
/// scaled by the SAME 1.35× so proportions stay consistent; animals are deliberately left small.
/// Camera `EYE_H`/`FP_EYE_H` (player/camera.rs) track this height.
pub const HERO_SCALE: f32 = 0.6345;

/// A rig **joint** — a transform-only entity the animator ([`anim`]) poses. Each joint's mesh is a
/// separate child *leaf* entity ([`HeroMesh`]), so first-person can hide the body meshes without
/// hiding the arm joints that hang beneath the torso. (Hands / neck / feet are unanimated, so they
/// carry no `HeroPart`.)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Joint {
    Hips,
    Torso,
    Head,
    ShoulderL,
    ShoulderR,
    ElbowL,
    ElbowR,
    HipL,
    HipR,
    KneeL,
    KneeR,
    FootL,
    FootR,
    Shield,
    /// The held weapon's own pivot (studio `broadsword` group), so attacks can sweep the blade
    /// independently of the hand — the studio animates `broadsword.rotation` every attack phase.
    Sword,
}

#[derive(Component)]
pub struct HeroPart {
    pub joint: Joint,
}

/// A body **mesh leaf** (child of a joint). The whole hero renders in first-person now (so the
/// hands don't flicker), EXCEPT meshes flagged `fp_hide` — just the head, which would otherwise
/// fill the lens as a black blob. Hidden in FP by [`camera::fp_body_visibility`].
#[derive(Component)]
pub struct HeroMesh {
    pub fp_keep: bool,
    /// Hidden in first-person (the head only) so it doesn't block the camera.
    pub fp_hide: bool,
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
    /// Horizontal (world XZ) velocity — ramped toward the input target so the hero accelerates in
    /// and slides to a stop instead of snapping on/off. Transient (not saved).
    pub vel: Vec2,
    pub vel_y: f32,
    pub on_ground: bool,
    pub air_takeoff_y: f32,
    pub walk_phase: f32,
    /// 0..1 smooth blend tracking `moving` (drives anim weight).
    pub moving_amt: f32,
    /// 0..1 smooth blend tracking `sprinting` (drives the walk→run pose blend in [`anim`]).
    pub run_amt: f32,
    pub moving: bool,
    // ── Attack (M2) ──
    pub attacking: bool,
    /// Seconds into the current swing.
    pub attack_t: f32,
    /// Whether this swing's cone-damage has already been applied.
    pub hit_dealt: bool,
    /// Which studio attack clip this swing plays: 0 = overhead chop, 1 = horizontal slash,
    /// 2 = forward thrust. Rolled per-swing in `combat::player_attack` so attacks vary.
    pub attack_variant: u8,
    /// Transient: play the studio **victory** clip (sword raised, proud sway). Set by a win / a
    /// preview hook; not persisted (derived, like `attacking`).
    pub victory: bool,
    // ── Charged Heavy Strike ──
    /// Seconds the attack button has been held since the last press, or **`-1.0` when not charging**
    /// (the sentinel — `>= 0.0` means a charge is armed/building). Set by `combat::player_attack`;
    /// drives the charge bar, the move-slow ([`movement`]) and the charge stance ([`anim`]).
    /// Transient (derived, like `attacking`) — not saved; reset to `-1.0` on a fresh run.
    pub charge_t: f32,
    /// Whether the *current* swing is the charged Heavy Strike (guaranteed crit, ×3 damage, max
    /// juice) rather than a normal tap — drives the heavy pose ([`anim`]) + damage ([`combat`]).
    /// Set on release of a full charge; cleared when the swing ends. Transient.
    pub heavy: bool,
    // ── Sand Dash slide ──
    /// Seconds into the active Sand-Dash slide, or **`-1.0` when not dashing** (the sentinel).
    /// Armed by `arts::player_arts`; [`movement`] slides the body `dash_from → dash_to` over
    /// `movement::DASH_TIME` (so the dash *travels* instead of teleporting) and [`anim`] plays the
    /// dash-swipe lunge from it. Transient (derived, like `attacking`) — not saved.
    pub dash_t: f32,
    /// World-XZ endpoints of the active dash slide (only meaningful while `dash_t >= 0.0`).
    pub dash_from: Vec2,
    pub dash_to: Vec2,
    // ── Attack lock-on ──
    /// Facing angle the current swing is soft-snapping toward — the nearest enemy in lock range when
    /// the swing started. `Some` only while a swing steers toward a target (third-person; FP aims by
    /// view), cleared when the swing ends. Makes a blow face what you're hitting instead of your
    /// strafe direction. Transient (derived, like `attacking`) — not saved.
    pub lock_face: Option<f32>,
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
    /// Shield raised this frame — read by the ork/wildlife keep-out so a guarded attacker is held
    /// off the *extended* shield (further out front) rather than the bare torso. Published by
    /// `block::player_block`.
    pub blocking: bool,
}

/// Damage the orks have dealt the hero since the last health tick. Orks accumulate onto it
/// (`+=`); [`health::apply_hero_damage`] drains it once per frame. Mirrors the TS store-
/// mediated combat channel — no collision events.
#[derive(Resource, Default)]
pub struct PendingHeroDamage(pub f32);

/// Present when a scripted demo (`FOREST_DEMO=explore`) owns the hero's locomotion — [`movement`]
/// yields so it doesn't fight the script (which writes pos/facing/anim directly). Lets a `FOREST_TPS`
/// capture film the scripted walk through the real follow-cam.
#[derive(Resource)]
pub struct ScriptedHero;

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
        // `FOREST_TPS=1` forces Play + THIRD-person — the real over-the-shoulder gameplay camera —
        // so a shot/clip frames the world the way a player actually sees it (no god-cam `FOREST_CAM`
        // guessing). Tune the orbit with `FOREST_TPS_AZ`/`_PITCH` (radians) + `_DIST` (units); pair
        // with `FOREST_HERO` to place the hero and `FOREST_DEMO=explore` to film a real walk.
        // `FOREST_FREEROAM=1` boots into the fly-cam *without* capturing/exiting — so a fixed
        // `FOREST_CAM` view holds (the fly-cam stays put with no input), giving a pinned, identical
        // frame to A/B perf changes (e.g. `FOREST_NOCULL` on/off) off the F2 overlay.
        let fp_boot = std::env::var("FOREST_FP").is_ok();
        let tps_boot = std::env::var("FOREST_TPS").is_ok();
        let start_mode = if fp_boot || tps_boot {
            PlayMode::Play
        } else if std::env::var("FOREST_SHOT").is_ok()
            || std::env::var("FOREST_CLIP").is_ok()
            || std::env::var("FOREST_FREEROAM").is_ok()
        {
            PlayMode::FreeRoam
        } else {
            PlayMode::Play
        };
        // Third-person-shot orbit overrides (radians / units), so a capture can pick the viewing
        // angle without touching code. Defaults are the normal in-game over-the-shoulder pose.
        let mut orbit = camera::OrbitCam::default();
        let envf = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok());
        if let Some(a) = envf("FOREST_TPS_AZ") { orbit.azimuth = a; }
        if let Some(p) = envf("FOREST_TPS_PITCH") { orbit.pitch = p; }
        if let Some(d) = envf("FOREST_TPS_DIST") { orbit.dist = d; }
        app.insert_resource(start_mode)
            .init_resource::<HeroState>()
            .init_resource::<PendingHeroDamage>()
            .init_resource::<PendingCrit>()
            .init_resource::<PlayerRes>()
            .init_resource::<combat::CombatRng>()
            .init_resource::<combat::HitStop>()
            .insert_resource(orbit)
            .insert_resource(camera::FirstPerson { active: fp_boot, ..default() })
            .add_systems(Startup, combat::setup_combat_fx)
            .add_systems(PostStartup, (spawn_hero, arts::spawn_arts_hud, charge::spawn_charge_bar, debug_grant_boons))
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
                    animtest, // debug: FOREST_ANIMTEST=walk|block stages an animation for a capture
                    anim::hero_anim,
                    combat::update_sparks,
                    combat::update_fx_fades,
                    combat::hero_blade_trail,
                    combat::drive_hit_stop, // ungated: must resume the clock after the freeze
                    arts::apply_knock, // ungated: fold queued slam knockbacks into ork kb
                    arts::sync_arts_hud, // ability-chip HUD (show/dim per readiness)
                    charge::sync_charge_bar, // heavy-strike charge bar (show/fill per hold)
                    charge::heavy_tip, // one-time "Hold LMB" hint near the first enemy
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

/// Debug/screenshot hook: `FOREST_ANIMTEST=walk|run|block|jump` forces the hero into that animation each
/// frame so a capture can frame it — FreeRoam captures never run `player_move`/`player_block`, so
/// the rig would otherwise sit idle. No-op unless the env var is set.
fn animtest(time: Res<Time>, mut hero_q: Query<(&mut Hero, &mut HeroHealth)>) {
    let Ok(mode) = std::env::var("FOREST_ANIMTEST") else { return };
    let Ok((mut hero, mut hh)) = hero_q.single_mut() else { return };
    let dt = time.delta_secs();
    let swing = |hero: &mut Hero, variant: u8| {
        hero.attacking = true;
        hero.attack_variant = variant;
        hero.attack_t = (hero.attack_t + dt) % ATTACK_DURATION;
    };
    match mode.as_str() {
        "walk" => {
            hero.moving = true;
            hero.moving_amt = 1.0;
            hero.run_amt = 0.0;
            hero.walk_phase += dt * 7.0; // = movement::STEP_FREQ
        }
        "run" => {
            hero.moving = true;
            hero.moving_amt = 1.0;
            hero.run_amt = 1.0;
            hero.walk_phase += dt * 7.0 * 1.75; // STEP_FREQ * SPRINT_MULT
        }
        "block" | "defend" => hh.blocking = true,
        "attack" | "attack1" => swing(&mut hero, 0),
        "attack2" => swing(&mut hero, 1),
        "attack3" => swing(&mut hero, 2),
        "heavy" => {
            hero.heavy = true;
            swing(&mut hero, combat::HEAVY_VARIANT); // the charged Heavy Strike chop
        }
        "charge" => {
            // Force the hold from wall-clock (absolute, so nothing resets it between frames): the
            // charge-stance coil deepens then holds at full.
            hero.charge_t = (time.elapsed_secs() * 0.25).min(combat::CHARGE_THRESHOLD);
        }
        "victory" => hero.victory = true,
        "dash" => {
            // Loop the Sand-Dash slide progress so a capture frames the dash-swipe lunge.
            hero.dash_t = (time.elapsed_secs() * 0.5) % movement::DASH_TIME;
        }
        "jump" => {
            hero.on_ground = false;
            hero.vel_y = 2.0;
        }
        _ => {}
    }
}

fn spawn_hero(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::creature::CreatureMaterial>>,
    inv: Res<Inventory>,
) {
    // The hero's own matte creature material; colour lives in the mesh vertex colours and surface
    // texture comes from the alpha-packed surf code (matte plate/cloth/skin, not shiny plastic).
    let mat = crate::creature::make_hero_material(&mut materials);
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
                vel: Vec2::ZERO,
                vel_y: 0.0,
                on_ground: true,
                air_takeoff_y: y,
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
        commands.insert_resource(HeroState { pos, y, facing, alive: true, blocking: false });
    }
}

/// Spawn a joint entity (transform-only, optionally `HeroPart`-tagged for the animator), parented
/// under `parent`, returning it so children can nest beneath. An optional mesh `leaf` is spawned as
/// a separate child entity (so first-person can toggle body meshes without hiding child joints).
struct Leaf {
    mesh: Handle<Mesh>,
    fp_keep: bool,
    fp_hide: bool,
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
            HeroMesh { fp_keep: l.fp_keep, fp_hide: l.fp_hide },
        ));
        if l.weapon {
            le.insert(HeroWeapon);
        }
        let leaf_e = le.id();
        commands.entity(joint).add_child(leaf_e);
    }
    joint
}

/// Spawn the full articulated knight (hips → torso → neck → head; shoulder → elbow → hand
/// + weapon/shield; hip → knee → foot) as children of the hero `root`, all sharing the hero
/// material. Shared by [`spawn_hero`] and [`reskin_hero`] so an equip swap rebuilds the same tree.
pub(crate) fn spawn_hero_meshes(
    commands: &mut Commands,
    root: Entity,
    m: model::KnightMeshes,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<crate::creature::CreatureMaterial>,
) {
    use Joint::*;
    let p = |t: Vec3| Transform::from_translation(t);
    let body = |mesh: Handle<Mesh>| Some(Leaf { mesh, fp_keep: false, fp_hide: false, weapon: false });
    let arm = |mesh: Handle<Mesh>| Some(Leaf { mesh, fp_keep: true, fp_hide: false, weapon: false });
    // FP eye sits ~at chest/neck height (`FP_EYE_H`) right inside the upper body, so the head, neck,
    // torso and shoulders crowd the lens as a dark blob. `fp_off` hides those in first-person; the
    // forearms/hands/weapon/shield/legs stay visible. Visible in third person regardless.
    let fp_off = |mesh: Handle<Mesh>| Some(Leaf { mesh, fp_keep: false, fp_hide: true, weapon: false });
    // The UPPER arms (shoulder meshes) sit right at the FP eye and balloon into two blobs that fill
    // the lens. Hide just those in first person (`fp_keep:false`) like the body. The FOREARMS stay
    // (`arm`, kept) so the sword/shield read as HELD in a hand — not levitating — posed low into the
    // corners by the FP viewmodel raise (`anim::hero_anim`). Visible in third person regardless.
    let upper = fp_off;

    use model::{HIP_DX, O_ELBOW, O_FOOT, O_HAND, O_HEAD, O_HIP_Y, O_KNEE, O_NECK, O_SHOULDER_Y, O_TORSO, SHOULDER_DX, Y_HIPS};

    // Rig at the feet (y=0); proportions are HH-derived (see model::PROPORTIONS). Feet rest on the
    // ground because the boot mesh bottoms at the ankle joint's height below it.
    let rig = commands
        .spawn((Transform::from_xyz(0.0, 0.0, 0.0), Visibility::Visible))
        .id();
    commands.entity(root).add_child(rig);

    // Spine: hips (anim-fixed Y_HIPS) → torso → neck → head.
    let hips = spawn_joint(commands, rig, Some(Hips), p(Vec3::new(0.0, Y_HIPS, 0.0)), mat, body(meshes.add(m.hips)));
    let torso = spawn_joint(commands, hips, Some(Torso), p(Vec3::new(0.0, O_TORSO, 0.0)), mat, fp_off(meshes.add(m.torso)));
    let neck = spawn_joint(commands, torso, None, p(Vec3::new(0.0, O_NECK, 0.0)), mat, fp_off(meshes.add(m.neck)));
    spawn_joint(commands, neck, Some(Head), p(Vec3::new(0.0, O_HEAD, 0.0)), mat, fp_off(meshes.add(m.head)));

    // Left arm + heater shield on the hand pivot (`anim` rewrites the shield pose every frame).
    let sh_l = spawn_joint(commands, torso, Some(ShoulderL), p(Vec3::new(-SHOULDER_DX, O_SHOULDER_Y, 0.01)), mat, upper(meshes.add(m.shoulder_l)));
    let el_l = spawn_joint(commands, sh_l, Some(ElbowL), p(Vec3::new(0.0, O_ELBOW, 0.0)), mat, arm(meshes.add(m.elbow_l)));
    let hand_l = spawn_joint(commands, el_l, None, p(Vec3::new(0.0, O_HAND, 0.0)), mat, None);
    let shield = spawn_joint(
        commands,
        hand_l,
        Some(Shield),
        Transform { translation: Vec3::new(-0.07, -0.08, 0.13), rotation: Quat::from_euler(EulerRot::XYZ, 0.12, -1.5, 0.0), scale: Vec3::ONE },
        mat,
        arm(meshes.add(m.shield)),
    );
    spawn_joint(commands, shield, None, p(Vec3::new(0.0, -0.03, 0.033)), mat, arm(meshes.add(m.lion)));

    // Right arm + held weapon on its own `Sword` pivot (attacks sweep it).
    let sh_r = spawn_joint(commands, torso, Some(ShoulderR), p(Vec3::new(SHOULDER_DX, O_SHOULDER_Y, 0.01)), mat, upper(meshes.add(m.shoulder_r)));
    let el_r = spawn_joint(commands, sh_r, Some(ElbowR), p(Vec3::new(0.0, O_ELBOW, 0.0)), mat, arm(meshes.add(m.elbow_r)));
    let hand_r = spawn_joint(commands, el_r, None, p(Vec3::new(0.0, O_HAND, 0.0)), mat, None);
    spawn_joint(commands, hand_r, Some(Sword), Transform::default(), mat, Some(Leaf { mesh: meshes.add(m.weapon), fp_keep: true, fp_hide: false, weapon: true }));

    // Legs: hip joint → knee → ankle (HH-derived; feet land on the ground).
    let hip_l = spawn_joint(commands, hips, Some(HipL), p(Vec3::new(-HIP_DX, O_HIP_Y, 0.0)), mat, body(meshes.add(m.hip_l)));
    let knee_l = spawn_joint(commands, hip_l, Some(KneeL), p(Vec3::new(0.0, O_KNEE, 0.0)), mat, body(meshes.add(m.knee_l)));
    spawn_joint(commands, knee_l, Some(FootL), p(Vec3::new(0.0, O_FOOT, 0.0)), mat, body(meshes.add(m.foot_l)));
    let hip_r = spawn_joint(commands, hips, Some(HipR), p(Vec3::new(HIP_DX, O_HIP_Y, 0.0)), mat, body(meshes.add(m.hip_r)));
    let knee_r = spawn_joint(commands, hip_r, Some(KneeR), p(Vec3::new(0.0, O_KNEE, 0.0)), mat, body(meshes.add(m.knee_r)));
    spawn_joint(commands, knee_r, Some(FootR), p(Vec3::new(0.0, O_FOOT, 0.0)), mat, body(meshes.add(m.foot_r)));
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
        vel: Vec2::ZERO,
        vel_y: 0.0,
        on_ground: true,
        air_takeoff_y: y,
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
    };
    tf.translation = Vec3::new(pos.x, y, pos.y);
    tf.rotation = Quat::from_rotation_y(0.0);
    tf.scale = Vec3::splat(HERO_SCALE);
    *hh = HeroHealth::default();
}
