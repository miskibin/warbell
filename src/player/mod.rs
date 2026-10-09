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
    spawn_sweep_burst, CombatFx, Health, HitStop, FOV_KICK_HEAVY, HITSTOP_CRIT, HITSTOP_HEAVY,
    HITSTOP_HIT, HITSTOP_KILL, KNOCKBACK, KNOCKBACK_CRIT, SHAKE_CRIT, SHAKE_HEAVY, SHAKE_HIT,
    SHAKE_KILL,
};
/// Swing length (seconds) — exposed so the standalone viewer can loop a preview swing.
pub use combat::ATTACK_DURATION;
mod health;
pub(crate) mod model;
mod footman;
mod movement;
pub(crate) use movement::{SPEED, SPRINT_MULT};
mod softlock;
mod viewmodel;

/// First-person view state, toggled by the HUD eye button ([`crate::ui::settings`]) and the V key.
pub use camera::FirstPerson;
pub(crate) use camera::fp_body_visibility;
pub use viewmodel::ReticleTarget;
/// Sand-Dash slide duration — re-exported so the standalone viewer (`viewer.rs`) can drive the
/// dash-swipe preview at the real cadence. (`anim` reads it directly via `super::movement`.)
pub(crate) use movement::DASH_TIME;

use bevy::prelude::*;

/// Root scale applied to the TS-unit knight. Was 0.47 (≈ ork height); now ~1.35× that (0.47 × 1.5,
/// then dialled back 10%) so the hero reads clearly in the third-person frame without towering as a
/// giant. The rest of the human-scale world (orks, townsfolk, castle houses, town buildings) is
/// scaled by the SAME 1.35× so proportions stay consistent; animals are deliberately left small.
/// Camera `EYE_H`/`FP_EYE_H` (player/camera.rs) track this height.
pub const HERO_SCALE: f32 = 0.6345;

/// A rig **joint** — a transform-only entity the animator ([`anim`]) poses. Each joint's mesh is a
/// separate child *leaf* entity ([`HeroMesh`]). (Hands / neck / feet are unanimated, so they carry
/// no `HeroPart`.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// A body **mesh leaf** (child of a joint). In first person every one of these is hidden by
/// [`camera::fp_body_visibility`] — the hands, sword and shield you see there are the separate
/// camera-parented [`viewmodel`], so the third-person rig keeps animating untouched underneath.
#[derive(Component)]
pub struct HeroMesh;

/// The held weapon mesh leaf (under the right hand). Toggled `Visibility::Hidden` for weapon-free
/// staged gestures (the Director's "hide weapon"), and read by `combat::hero_blade_trail`.
#[derive(Component)]
pub struct HeroWeapon;

/// The sheathed sword slung across the hero's back (a copy of the hand sword's meshes under the
/// torso). `anim::hero_anim` swaps it with the hand sword: drawn only for a fight, slung on the back
/// otherwise (so a sprint pumps two free arms instead of waving a blade).
#[derive(Component)]
pub struct BackSword;

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
    /// Horizontal ground speed (world u/s) the gait is cut for — the animator picks walk / jog /
    /// sprint from it and `walk_phase` advances by [`anim::gait_phase_rate`] of it, so the planted
    /// boot stays locked to the ground. Written by movement (and the preview drivers).
    pub gait_speed: f32,
    /// 0..1 smooth blend tracking `moving` (drives anim weight).
    pub moving_amt: f32,
    /// 0..1 smooth blend tracking `sprinting` (sprint dust in `footstep_fx`; the hero's gait style
    /// itself follows `gait_speed`).
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
    // ── Soft-lock (combat aim-assist) ──
    /// Enemy the soft-lock has picked as your current target — the nearest hostile in the front arc
    /// while in combat. Drives the measured auto-face, the swing's aim, and the ground target ring.
    /// `None` out of combat / no hostile in range. Transient (derived) — not saved.
    pub soft_target: Option<Entity>,
    /// World-XZ of `soft_target`, cached so the swing lock + ring don't re-query it. Transient.
    pub soft_pos: Option<Vec2>,
    // ── Combo chain (the Witcher-style 1-2-3 flow; see `combat`) ──
    /// This swing's duration (s) — combo steps 2/3 swing faster than step 1, so the phase math
    /// divides by this instead of the flat `ATTACK_DURATION`. Reset per swing. Transient.
    pub attack_dur: f32,
    /// Current combo step 0/1/2 (overhead → slash → thrust). Advances while swings chain inside
    /// [`combat::COMBO_WINDOW`]; resets on a gap, a roll, or a Heavy. Transient.
    pub combo: u8,
    /// `elapsed_secs` deadline to chain the next combo step (stamped when a swing completes).
    pub combo_until: f32,
    /// A mid-swing attack press was buffered — the next swing fires the instant this one ends,
    /// so mashing chains fluidly instead of dropping inputs. Transient.
    pub queued: bool,
    // ── Attack magnetism (gap-closer) ──
    /// World units of forward lunge left to spend across this swing's wind-up — set at swing
    /// start from the soft-target's distance so the blow *steps into* the foe (the Witcher
    /// attack-glide) instead of whiffing at air. `0` = no lunge. Transient.
    pub lunge_left: f32,
    // ── Parry / riposte ──
    /// `elapsed_secs` until which the next swing is a RIPOSTE (granted by a timed parry in
    /// [`health`]): a guaranteed-crit counter-thrust. Consumed at swing start. Transient.
    pub riposte_until: f32,
    /// Whether the *current* swing is the riposte counter (drives its damage). Transient.
    pub riposte: bool,
    // ── Dodge roll (Alt) ──
    /// Seconds into the active dodge roll, or **`-1.0` when not rolling** (the sentinel, like
    /// `dash_t`). Armed by [`movement::player_roll`]; [`movement::player_move`] slides the body
    /// `roll_from → roll_to` and tumbles the root through a full somersault; [`anim`] tucks the
    /// limbs. Transient — not saved.
    pub roll_t: f32,
    /// World-XZ endpoints of the active roll (only meaningful while `roll_t >= 0.0`).
    pub roll_from: Vec2,
    pub roll_to: Vec2,
    /// `true` = a backward roll (no move input: the hero dives *away* while still facing the
    /// foe), so the tumble spins the other way.
    pub roll_back: bool,
    // ── Combat stance (Witcher "Alert Near" — see `movement`) ──
    /// 0..1 smooth blend: 1 = the stance is engaged (a soft-target is near, not sprinting, not
    /// FP) — the body squares to the foe while WASD strafes/backpedals. Drives the animator's
    /// gait twist and the camera's combat framing. Transient — not saved.
    pub stance_amt: f32,
    /// Smoothed leg-yaw offset (radians): in the stance the pelvis+legs aim along the MOVEMENT
    /// while the torso/head counter-rotate to stay on the foe. Wraps at ±90° (past that the gait
    /// reverses instead — see `back_amt`). Written by [`movement`], read by [`anim`]. Transient.
    pub strafe_twist: f32,
    /// 0..1 smooth blend toward the BACKPEDAL gait (stance movement pointing behind the body):
    /// the walk cycle plays in reverse so the hero steps backward while facing the foe. Transient.
    pub back_amt: f32,
    /// Live hostiles near the hero (within `softlock::THREAT_RANGE`, any direction) — written by
    /// [`softlock`], read by the camera to scale its combat dolly with the size of the scrap.
    pub threats: u32,
    /// `elapsed_secs` until which the hero counts as IN COMBAT — refreshed whenever he lands a
    /// blow on an enemy ([`combat`]/[`arts`]) or takes/blocks one ([`health`]). The combat stance
    /// engages only while this holds (walking past a camp never squares you up; trading blows
    /// does), lingering [`COMBAT_LINGER`]s past the last exchange. Transient — not saved.
    pub combat_until: f32,
}

/// Seconds the "in combat" state lingers past the last blow dealt/taken (drives the stance).
pub(crate) const COMBAT_LINGER: f32 = 6.0;

impl Hero {
    /// A fresh hero at rest — the single initializer shared by spawn + the new-run reset, so a
    /// new transient field only needs adding here.
    pub(crate) fn fresh(pos: Vec2, y: f32, facing: f32) -> Self {
        Hero {
            pos,
            y,
            facing,
            vel: Vec2::ZERO,
            vel_y: 0.0,
            on_ground: true,
            air_takeoff_y: y,
            walk_phase: 0.0,
            gait_speed: 0.0,
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
            soft_target: None,
            soft_pos: None,
            attack_dur: combat::ATTACK_DURATION,
            combo: 0,
            combo_until: 0.0,
            queued: false,
            lunge_left: 0.0,
            riposte_until: 0.0,
            riposte: false,
            roll_t: -1.0,
            roll_from: Vec2::ZERO,
            roll_to: Vec2::ZERO,
            roll_back: false,
            stance_amt: 0.0,
            strafe_twist: 0.0,
            back_amt: 0.0,
            threats: 0,
            combat_until: 0.0,
        }
    }
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
    /// `elapsed_secs` the shield was last RAISED (the RMB rising edge, stamped by
    /// [`block::player_block`]). A blow that lands within [`health`]'s parry window of this is a
    /// timed **parry** — staggers the attacker, costs no stamina, and arms the riposte.
    pub guard_raised_at: f32,
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
            guard_raised_at: -100.0,
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
    /// Shield raised this frame — read by the ork/wildlife keep-out so a guarded attacker is held
    /// off the *extended* shield (further out front) rather than the bare torso. Published by
    /// `block::player_block`.
    pub blocking: bool,
}

/// Damage the orks have dealt the hero since the last health tick. Orks accumulate onto it
/// (`+=`); [`health::apply_hero_damage`] drains it once per frame. Mirrors the TS store-
/// mediated combat channel — no collision events.
///
/// `.1` is the world-XZ direction the last queued blow TRAVELLED (attacker → hero, normalized;
/// `Vec2::ZERO` = source unknown). Attack sites set it alongside the damage; `apply_hero_damage`
/// reads it once to bias the hit screen-shake AWAY from the attacker, then clears it with the
/// damage. Hazards with no attacker (swamp poison, fall damage) leave it zero → unbiased shake.
#[derive(Resource, Default)]
pub struct PendingHeroDamage(pub f32, pub Vec2);

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
            // `setup_combat_fx` stays UNGATED: it inserts the shared `CombatFx` resource that
            // ungated modules (projectile/defenses impact sparks) read, and RTS archer arrows
            // reuse those FX in Skirmish. Everything hero-specific below is Campaign-only.
            .add_systems(Startup, combat::setup_combat_fx)
            // Ungated spawn: the hero rig + its campaign HUD (ability bar, charge bar, target ring)
            // are built in EVERY boot (the mode can flip Campaign⇄Skirmish mid-process). Each root
            // is tagged `CampaignOnly`, so `apply_mode_visibility` hides them on a Skirmish boot the
            // first Update frame (the mode resource's boot change-tick). `debug_grant_boons` is
            // internally env-gated (no-op unless `FOREST_BOONS`), so it rides along un-gated. All
            // per-frame hero systems below stay `in_campaign`-gated.
            .add_systems(
                PostStartup,
                (spawn_hero, arts::spawn_arts_hud, charge::spawn_charge_bar, debug_grant_boons, softlock::spawn_target_ring),
            )
            // Fresh run: wipe progression + revive the hero on a new run (NOT on un-pause).
            .add_systems(
                OnExit(crate::game_state::AppState::StartScreen),
                reset_player.run_if(crate::rts::in_campaign).run_if(crate::game_state::fresh_run_reset),
            )
            .add_systems(
                OnExit(crate::game_state::AppState::GameOver),
                reset_player.run_if(crate::rts::in_campaign),
            )
            // Render/input — keep running even when the world is frozen (so the paused scene
            // still draws + you can leave free-roam). `toggle_mode` is the backtick free-cam.
            .add_systems(
                Update,
                (
                    camera::toggle_mode,
                    camera::toggle_first_person, // V / HUD eye button: third ⇄ first person
                    camera::player_camera,
                    camera::fp_body_visibility
                        .after(camera::player_camera), // FP: hide the world rig
                    animtest, // debug: FOREST_ANIMTEST=walk|block stages an animation for a capture
                    anim::hero_anim,
                    combat::update_sparks,
                    combat::update_fx_fades,
                    combat::update_light_fades, // impact flashes (kill/heavy/parry) decay + despawn
                    combat::hero_blade_trail,
                    combat::drive_hit_stop, // ungated: must resume the clock after the freeze
                    arts::apply_knock, // ungated: fold queued slam knockbacks into ork kb
                    arts::sync_arts_hud, // ability-chip HUD (show/dim per readiness)
                    charge::sync_charge_bar, // heavy-strike charge bar (show/fill per hold)
                    charge::heavy_tip, // one-time "Hold LMB" hint near the first enemy
                )
                    // Campaign-only: no hero in Skirmish, and `player_camera` must not fight the
                    // RTS iso camera (which drives the SAME single Camera3d).
                    .run_if(crate::rts::in_campaign),
            )
            // First-person viewmodel (camera-parented hands/sword/shield) + reticle. Ungated so the
            // frozen world still draws them; `animate_viewmodel` runs after the camera so its
            // look-inertia reads this frame's view angles.
            .init_resource::<viewmodel::ReticleTarget>()
            .add_systems(Startup, viewmodel::spawn_reticle)
            .add_systems(
                Update,
                (
                    viewmodel::spawn_viewmodel,
                    viewmodel::animate_viewmodel.after(camera::player_camera),
                    viewmodel::sync_reticle,
                )
                    .run_if(crate::rts::in_campaign),
            )
            // World-sim — gated on the freeze condition (`Modal::None` ⇒ Playing, no panel).
            // `player_move`/`attack` also early-return outside `PlayMode::Play` (free-roam).
            .add_systems(
                Update,
                (
                    movement::player_roll, // arm the Alt dodge-roll (before move so it owns this frame)
                    movement::player_move,
                    softlock::soft_lock, // pick + gently face the soft target, drive the ring (after move so input wins)
                    block::player_block,
                    combat::swing_test, // debug: FOREST_SWINGTEST=1 loops the attack chain for a capture
                    combat::player_attack,
                    arts::player_arts, // warden weapon arts (after move/attack so a dash sticks)
                    combat::ensure_combat_health,
                    health::apply_hero_damage,
                    health::hero_death_anim, // keel-over pose; last so it owns the dead transform
                )
                    .chain()
                    .run_if(in_state(crate::game_state::Modal::None))
                    .run_if(crate::rts::in_campaign),
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
        hero.attack_dur = ATTACK_DURATION; // combo pacing off — a staged preview loops the base speed
        hero.attack_t = (hero.attack_t + dt) % ATTACK_DURATION;
    };
    match mode.as_str() {
        "walk" => {
            anim::stage_gait(&mut hero, movement::SPEED, dt);
        }
        "strafe" => {
            // Combat-stance sideways step: legs twisted toward the movement, torso on the "foe".
            anim::stage_gait(&mut hero, movement::SPEED, dt);
            hero.stance_amt = 1.0;
            hero.strafe_twist = 0.6;
        }
        "backpedal" => {
            // Combat-stance retreat: the walk cycle in reverse, eyes still on the "foe".
            anim::stage_gait(&mut hero, movement::SPEED, dt);
            hero.stance_amt = 1.0;
            hero.back_amt = 1.0;
        }
        "run" => {
            hero.run_amt = 1.0;
            anim::stage_gait(&mut hero, movement::SPEED * movement::SPRINT_MULT, dt);
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
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<crate::creature::CreatureMaterial>>,
) {
    // Debug/screenshot hook: `FOREST_HERO="x,z"` drops the hero at a world XZ (e.g. deep in a
    // biome region) so a capture shows that biome's reactive atmosphere/weather.
    let staged = std::env::var("FOREST_HERO").ok().and_then(|s| {
        let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        (v.len() == 2).then(|| Vec2::new(v[0], v[1]))
    });
    let (home, home_facing) = spawn_point();
    let pos = staged.unwrap_or(home);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let facing = if staged.is_some() { 0.0 } else { home_facing };

    let root = commands
        .spawn((
            Transform {
                translation: Vec3::new(pos.x, y, pos.y),
                rotation: Quat::from_rotation_y(facing),
                scale: Vec3::splat(HERO_SCALE),
            },
            Visibility::Visible,
            Hero::fresh(pos, y, facing),
            HeroHealth::default(),
            crate::game_state::CampaignOnly,
        ))
        .id();

    // The Royal Footman is the knight, textured through `creature.wgsl`. Gear ids no longer
    // rebuild the body — the mesh is the footman either way.
    spawn_hero_meshes(&mut commands, root, &mut meshes, &mut images, &mut materials);

    // When staged into a biome for a screenshot, mirror the pose into `HeroState` now so the
    // reactive atmosphere/weather pick up that region immediately (in FreeRoam capture mode
    // `player_move` doesn't run, so it never would otherwise).
    if staged.is_some() {
        commands.insert_resource(HeroState { pos, y, facing, alive: true, blocking: false });
    }
}

/// Spawn the Royal Footman under the hero `root`. Shared by [`spawn_hero`] and the standalone
/// viewer. The body does not change with equipped gear — the footman is the knight.
pub(crate) fn spawn_hero_meshes(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<crate::creature::CreatureMaterial>,
) {
    footman::spawn(commands, root, meshes, images, materials);
}

/// Reset the hero to a fresh run: wipe progression (`Player::reset` → full HP, 30 gold, level 1,
/// neutral combat stats) and revive him at the north gate. Runs when a run (re)starts — leaving the
/// start screen, or leaving game-over (a fresh run relaunches, but an in-process **Continue** also
/// exits game-over here: the wipe is harmless then, as `savegame::apply_pending_load` immediately
/// overwrites the progression from the save while this revival of the hero entity stands). Never
/// on un-pause.
/// The hero's home spawn: the meadow's forest edge (west-southwest of the castle), beside the
/// rest campfire — a new run opens walking IN from the treeline with the castle framed across
/// the meadow (2026-07; previously just outside the north gate at `gate - 3`). Returns
/// `(world XZ, facing)`; the facing aims at the keep so the opening frame reads immediately.
pub fn spawn_point() -> (Vec2, f32) {
    let pos = Vec2::new(-26.0, 18.0);
    // Yaw 0 faces +Z; aim at the castle at the origin.
    let facing = (-pos.x).atan2(-pos.y);
    (pos, facing)
}

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
    let (pos, facing) = spawn_point();
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    *hero = Hero::fresh(pos, y, facing);
    tf.translation = Vec3::new(pos.x, y, pos.y);
    tf.rotation = Quat::from_rotation_y(facing);
    tf.scale = Vec3::splat(HERO_SCALE);
    *hh = HeroHealth::default();
}
