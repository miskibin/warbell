//! **Biome Wardens** — one roaming world boss per biome. A warden lurks in its biome region,
//! **passive until the hero lands the first hit**, then turns Hostile, chases, melees, and fires
//! a telegraphed signature attack on a cooldown. Each warden **levels up every dawn** (HP +
//! damage grow, healed to full each morning), so slaying one sooner is easier. A warden can be
//! killed **once**; its death grants a permanent combat boon (a new active move *or* a passive
//! that juices the hero's swing) and pops a reward dialog.
//!
//! Models live in [`models`]; combat damage rides the shared hero cone-scan (`player::combat`
//! includes `With<Boss>` in its target query), so a warden takes damage, crits, blood/floats and
//! the `Dying` fade exactly like an ork. The boon flags live on the parity `Player` struct
//! (`tileworld_core::player`); the leveling math is forest-side (a canonical divergence).

mod models;

use bevy::prelude::*;

use crate::biome::Biome;
use crate::dying::{begin_dying, Dying};
use crate::game_state::{AppState, Modal};
use crate::player::{CombatFx, Health, HeroState, PendingHeroDamage, PlayerRes};
use crate::projectile::{BoltSpawn, BoltSpawns};
use crate::siege::{GamePhase, Siege};
use crate::ui::anim::{anim, anim_btn, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::notice::Notice;
use crate::ui::theme::*;
use crate::ui::widgets;
use crate::{steer, worldmap};

use crate::audio::director::{Speak, VoiceManager};
use crate::audio::lines::Concept;
use crate::audio::MusicState;

// ── Tuning (all forest-side; not parity-gated) ────────────────────────────────────────
/// Warden HP at level 1, ×`HP_GROWTH` per level. Out-stats a bare hero on purpose (mid-game) —
/// a deliberately long, attrition fight; bumped (again) so a geared hero can't melt a warden in
/// a few swings — it must be a sustained, dangerous duel.
const BASE_HP: f32 = 2312.0; // another 15% off (was 2720) — wardens still too tanky
const HP_GROWTH: f32 = 1.18;
/// Warden melee damage to the hero at level 1, ×`DMG_GROWTH` per level.
const BASE_MELEE: f32 = 41.9; // another 15% off (was 49.3; signature = melee×SIG_MULT scales with it)
const DMG_GROWTH: f32 = 1.13;
/// Signature attack damage = melee × this.
const SIG_MULT: f32 = 1.6;

const BODY_R: f32 = 0.8;
const SPEED: f32 = 2.4; // slower than the hero (3.5) so it can be kited — but closes harder now
const TURN: f32 = 2.2; // rad/s
/// Hero must come within this to wake the one-time "something stirs" notice.
const NOTICE_RANGE: f32 = 28.0;
/// Within this the warden stops and strikes instead of closing.
const MELEE_RANGE: f32 = 3.4;
const MELEE_CD: f32 = 1.25;
const SIG_CD: f32 = 6.5;
/// **Telegraphed critical** — a warden plants, rears its weapon overhead, and after
/// [`CRIT_TELEGRAPH`] seconds brings down a KILLING blow. It's lethal if it connects, but the long
/// windup is the player's cue: raise the shield (or dodge out of [`CRIT_RANGE`]) before it lands.
const CRIT_CD: f32 = 9.5; // min seconds between a warden's critical attempts (more frequent killing blows = more pressure)
const CRIT_TELEGRAPH: f32 = 1.2; // windup time — the reaction window to block/dodge
const CRIT_RANGE: f32 = 4.2; // must be within this at impact to be struck — just past MELEE_RANGE (3.4) so a lunge connects, but stepping/dodging clear of melee escapes it (was 5.5: a 360° kill radius far past melee, so backing out of reach still ate the blow)
const CRIT_LETHAL: f32 = 100_000.0; // overkill so it one-shots through any resist/armor when unblocked
/// Radius of a Shock signature (stomp / root-snare / poison burst).
const SIG_SHOCK_RADIUS: f32 = 6.5;
const ROAM_RADIUS: f32 = 13.0;
/// A warden fights only within its home region: once the hero leaves this range from the
/// warden's home — or the hero falls — it breaks off and lumbers back home.
const BIOME_LEASH: f32 = 42.0;

/// HP a warden has at `level` (level 1 = freshest/weakest).
fn max_hp(level: i32) -> f32 {
    BASE_HP * HP_GROWTH.powi((level - 1).max(0))
}
fn melee_dmg(level: i32) -> f32 {
    BASE_MELEE * DMG_GROWTH.powi((level - 1).max(0))
}

/// Biome region centres (world XZ) — where each warden spawns + roams (see CLAUDE.md).
fn region_center(b: Biome) -> Vec2 {
    match b {
        Biome::Snow => Vec2::new(-69.0, -45.0),
        Biome::Desert => Vec2::new(60.0, -39.0),
        Biome::Rocky => Vec2::new(66.0, 4.0),
        Biome::Forest => Vec2::new(-60.0, 39.0),
        Biome::Swamp => Vec2::new(0.0, 57.0),
    }
}

const WARDENS: [Biome; 5] =
    [Biome::Forest, Biome::Snow, Biome::Rocky, Biome::Desert, Biome::Swamp];

/// Radius (world units) of the open glade kept TREE-FREE around each warden's lair. Wardens steer
/// around solid trunks (they never phase through them), so a boss fought in dense woods shuffles
/// between trees while the hero is blocked reaching it — the fight reads as broken. Clearing a glade
/// at the lair gives an open arena. A touch wider than [`ROAM_RADIUS`] so the warden's whole passive
/// roam stays clear. NB: only TRUNK trees (the blockers) are skipped in the scatter — low cover
/// (bushes, walk-through rocks, grass, flowers) is KEPT, so the glade reads as a natural clearing,
/// not a bald disc.
pub const GLADE_R: f32 = 16.0;

/// Is world `(wx, wz)` inside any warden's glade? The biome scatter pass calls this to drop
/// trunk-trees (routing them to a low bush instead) so no tree-blocker lands in a boss arena.
pub fn in_warden_glade(wx: f32, wz: f32) -> bool {
    let p = Vec2::new(wx, wz);
    WARDENS.iter().any(|&b| region_center(b).distance_squared(p) < GLADE_R * GLADE_R)
}

/// How a warden's signature attack resolves.
#[derive(Clone, Copy, PartialEq)]
enum Signature {
    /// Radial AoE burst around the warden (golem stomp / treant roots / hag poison cloud).
    Shock,
    /// `n` homing bolts at the hero (bałwan ice shards / revenant sand burst).
    Volley(u32),
}

fn signature(b: Biome) -> Signature {
    match b {
        Biome::Snow => Signature::Volley(3),
        Biome::Desert => Signature::Volley(1),
        _ => Signature::Shock,
    }
}

// ── Components ─────────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct Boss {
    pub biome: Biome,
    pub level: i32,
    pos: Vec2,
    facing: f32,
    home: Vec2,
    /// Flips true the first time the warden takes hero damage; then it fights to the death.
    hostile: bool,
    /// One-time proximity notice fired.
    seen: bool,
    /// HP last frame — a drop means the hero (or poison) struck → aggro.
    last_hp: f32,
    moving: bool,
    phase: f32,
    rng: u32,
    roam_target: Vec2,
    roam_timer: f32,
    atk_cd: f32,
    sig_cd: f32,
    /// Cooldown until the next telegraphed critical may begin.
    crit_cd: f32,
    /// `elapsed_secs` at which a winding-up critical LANDS (`0.0` = not winding up). The window
    /// `[crit_at - CRIT_TELEGRAPH, crit_at]` is the reared-back pose + the player's reaction time.
    crit_at: f32,
    /// `elapsed_secs` of the last signature cast (drives the windup limb pose).
    sig_anim: f32,
    atk_anim: f32,
}

impl Boss {
    /// Collision footprint — world XZ centre + radius — so the hero is shoved out of the warden's
    /// bulk instead of clipping straight through it. Kept a touch under the hero's 1.8u melee
    /// reach (radius + the hero's 0.22 body ≈ 1.5–1.6) so the player can still stand close enough
    /// to land a swing on the boss's centre.
    pub fn footprint(&self) -> (Vec2, f32) {
        // Kept just under the hittable ceiling (`r + PLAYER_R 0.22` < the hero's 1.8 melee reach,
        // so ≤ ~1.36): bumped the slimmer wardens up to a firmer footprint so you stop ON their
        // bulk instead of clipping a step into it. The golem was already at the ceiling.
        let r = match self.biome {
            Biome::Rocky => 1.36, // the golem is the widest — at the hittable limit
            Biome::Snow => 1.34,
            _ => 1.3,
        };
        (self.pos, r)
    }
}

/// An articulated warden limb (legs/arms/head), swung by [`boss_limbs`].
#[derive(Component)]
struct BossPart {
    kind: crate::critters::PartKind,
}

/// A frost-slowed foe — its move speed is scaled by `factor` until `until`. `factor` 0 = frozen.
/// Applied by the hero's Frostbite boon (`player::combat`); honored by `boss_brain` + `ork_brain`.
#[derive(Component)]
pub struct Slowed {
    pub until: f32,
    pub factor: f32,
}
impl Slowed {
    pub fn new(now: f32, factor: f32, dur: f32) -> Self {
        Slowed { until: now + dur, factor }
    }
}

/// A venom-poisoned foe — loses `dps` HP/sec until `until`; the hero heals a fraction (lifesteal).
/// Applied by the hero's Venom boon; ticked by [`tick_poison`].
#[derive(Component)]
pub struct Poisoned {
    pub until: f32,
    pub dps: f32,
}

/// True while any warden is mid-critical wind-up. The follow-cam ([`crate::player::camera`]) reads
/// this to ease a slow tension dolly-OUT across the telegraph, snapping back once the blow resolves —
/// so a "killing blow incoming" reads in the framing, not just the audio/limb tell.
#[derive(Resource, Default)]
pub struct CritTension(pub bool);

// ── Reward plumbing ────────────────────────────────────────────────────────────────────

/// Filled by [`reward_on_death`] when a warden falls; consumed by the [`Modal::BossReward`] dialog.
#[derive(Resource, Default)]
struct PendingReward(Option<RewardInfo>);

#[derive(Clone)]
struct RewardInfo {
    boss_name: &'static str,
    boon_name: &'static str,
    boon_desc: String,
}

/// `(boon display name, description incl. keybind)` granted by slaying the `biome` warden.
fn boon_for(biome: Biome) -> (&'static str, String) {
    match biome {
        Biome::Rocky => ("Ground Slam", "Press  Z  — a heavy slam: big damage in a wide ring and hurls foes back.".into()),
        Biome::Desert => ("Sand Dash", "Press  X  — a long blink that passes THROUGH danger unharmed (brief invulnerability).".into()),
        Biome::Forest => ("Bramble Sweep", "Press  C  — a spin-cleave that heals you for every foe it strikes.".into()),
        Biome::Snow => ("Frostbite", "Your strikes now chill foes — slowing them, and freezing on a crit.".into()),
        Biome::Swamp => ("Venom", "Your strikes now poison foes over time — and bleed life back to you.".into()),
    }
}

// ── Plugin ─────────────────────────────────────────────────────────────────────────────

pub struct BossPlugin;

impl Plugin for BossPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingReward>()
            .init_resource::<CritTension>()
            .add_systems(PostStartup, spawn_wardens)
            // Fresh in-process run (New Game / Restart / Play Again all exit StartScreen or GameOver
            // without a GameLoaded): rebuild the full level-1 pantheon so a warden slain in a prior
            // run isn't permanently gone (wardens aren't BiomeEntity, so the world rebuild skips them).
            .add_systems(OnExit(AppState::StartScreen), respawn_wardens_on_new_run)
            .add_systems(OnExit(AppState::GameOver), respawn_wardens_on_new_run)
            // On a loaded game, remove wardens the saved hero already slew (ungated; once per load).
            .add_systems(Update, despawn_slain_wardens)
            .add_systems(Update, boss_limbs) // limb sway keeps running while frozen
            .add_systems(Update, warden_music_flag) // ungated: boss music holds through a panel-freeze
            .add_systems(Update, sync_boss_bar.run_if(in_state(AppState::Playing)))
            .add_systems(OnExit(AppState::Playing), despawn_boss_bar)
            .add_systems(
                Update,
                (boss_brain, boss_proximity, boss_levelup, tick_status, reward_on_death)
                    .run_if(in_state(Modal::None)),
            )
            // Reward dialog: spawns frozen over the world, dismissed with Continue / Enter / Esc.
            .add_systems(OnEnter(Modal::BossReward), spawn_reward_ui)
            .add_systems(OnExit(Modal::BossReward), despawn_reward_ui)
            .add_systems(Update, reward_dismiss.run_if(in_state(Modal::BossReward)));
    }
}

// ── Spawn ──────────────────────────────────────────────────────────────────────────────

/// Find standable ground at/near `p` (spiral out a few rings so a region centre that lands on
/// water/cliff still places the warden).
fn ground_near(p: Vec2) -> (Vec2, f32) {
    if let Some(y) = worldmap::ground_at_world(p.x, p.y) {
        return (p, y);
    }
    for r in [4.0f32, 8.0, 12.0, 18.0] {
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            let q = p + Vec2::new(a.cos() * r, a.sin() * r);
            if let Some(y) = worldmap::ground_at_world(q.x, q.y) {
                return (q, y);
            }
        }
    }
    (p, 0.0)
}

fn spawn_wardens(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<crate::creature::CreatureMaterial>>,
) {
    let mat = crate::creature::make_creature_material(&mut materials);
    for (i, biome) in WARDENS.into_iter().enumerate() {
        let (pos, y) = ground_near(region_center(biome));
        let scale = models::root_scale(biome);
        let spec = models::build(biome);
        let facing = i as f32 * 1.3;
        let root = commands
            .spawn((
                Transform {
                    translation: Vec3::new(pos.x, y, pos.y),
                    rotation: Quat::from_rotation_y(facing),
                    scale: Vec3::splat(scale),
                },
                Visibility::Visible,
                Boss {
                    biome,
                    level: 1,
                    pos,
                    facing,
                    home: pos,
                    hostile: false,
                    seen: false,
                    last_hp: max_hp(1),
                    moving: false,
                    phase: i as f32 * 1.7,
                    rng: 0x9e37_79b9 ^ (i as u32 + 1).wrapping_mul(0x85eb_ca6b),
                    roam_target: pos,
                    roam_timer: 0.0,
                    atk_cd: 0.0,
                    sig_cd: SIG_CD,
                    crit_cd: CRIT_CD * 0.5, // first critical comes a little sooner than the full cadence
                    crit_at: 0.0,
                    sig_anim: 0.0,
                    atk_anim: 0.0,
                },
                Health { hp: max_hp(1), max: max_hp(1) },
            ))
            .id();
        commands.entity(root).with_children(|p| {
            p.spawn((Mesh3d(meshes.add(spec.torso)), MeshMaterial3d(mat.clone()), Transform::default()));
            for part in spec.parts {
                p.spawn((
                    Mesh3d(meshes.add(part.mesh)),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(part.pivot),
                    BossPart { kind: part.kind },
                ));
            }
        });
    }
}

/// New Game / Restart / Play Again (a fresh in-process run — no `GameLoaded`, so `despawn_slain_wardens`
/// never fires): despawn every warden and respawn all five at level 1, matching the boon flags that
/// `Player::reset()` clears. Without this a warden killed in a prior run stays permanently gone for
/// the rest of the process — its entity was `try_despawn`'d by the death-fade and is never rebuilt,
/// since wardens carry no `BiomeEntity` tag for the world-rebuild sweep to catch. On a Continue this
/// spawns fresh wardens too, then `despawn_slain_wardens` prunes the saved-slain ones the same frame.
fn respawn_wardens_on_new_run(
    mut commands: Commands,
    existing: Query<Entity, With<Boss>>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<crate::creature::CreatureMaterial>>,
) {
    for e in &existing {
        commands.entity(e).try_despawn();
    }
    spawn_wardens(commands, meshes, materials);
}

/// On a loaded game, despawn any warden the saved hero has **already slain**. A warden isn't
/// serialized as an entity — its kill is recorded by the permanent boon it granted (a flag on the
/// parity `Player`), so the saved `Player`'s boons ARE the "which wardens are dead" record. We read
/// them straight off the [`GameLoaded`](crate::savegame::GameLoaded) snapshot (not live `PlayerRes`,
/// which `apply_pending_load` may write the same frame in undefined order). Without this, a beaten
/// warden returns alive + re-killable on an in-process Continue. (Warden *levels* aren't persisted;
/// survivors re-level from 1 each load — easier, not harder.)
fn despawn_slain_wardens(
    mut commands: Commands,
    mut ev: MessageReader<crate::savegame::GameLoaded>,
    wardens: Query<(Entity, &Boss)>,
) {
    let Some(crate::savegame::GameLoaded(data)) = ev.read().last() else { return };
    for (e, b) in &wardens {
        let slain = match b.biome {
            Biome::Forest => data.player.has_bramble_sweep,
            Biome::Snow => data.player.frostbite,
            Biome::Rocky => data.player.has_ground_slam,
            Biome::Desert => data.player.has_sand_dash,
            Biome::Swamp => data.player.venom,
        };
        if slain {
            commands.entity(e).try_despawn();
        }
    }
}

// ── Brain: roam → aggro-on-hit → chase + melee + signature ──────────────────────────────

#[allow(clippy::too_many_arguments)]
fn boss_brain(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut pending: ResMut<PendingHeroDamage>,
    mut crit: ResMut<crate::player::PendingCrit>,
    mut bolts: ResMut<BoltSpawns>,
    fx: Option<Res<CombatFx>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut notice: ResMut<Notice>,
    mut tension: ResMut<CritTension>,
    mut taught_crit: Local<bool>,
    mut commands: Commands,
    mut q: Query<(&mut Boss, &mut Transform, &Health, Option<&Slowed>), Without<Dying>>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let tw = time.elapsed_secs_wrapped();
    // Recomputed each frame: true if ANY warden is rearing back for a killing blow (drives the
    // follow-cam tension dolly-out).
    let mut any_winding = false;
    for (mut b, mut tf, health, slowed) in &mut q {
        b.atk_cd -= dt;
        b.sig_cd -= dt;
        b.crit_cd -= dt;

        // Aggro on any HP loss (hero swing / cleave / poison) — the warden wakes with a roar on
        // the turn to hostile (not every subsequent hit).
        if !b.hostile && health.hp < b.last_hp - 0.01 {
            b.hostile = true;
            // Reaction beat: seed ALL THREE attack cooldowns so the warden ROARS and rears before
            // it can hit you — otherwise it counter-hits the very frame you wake it (the cooldowns
            // tick down the whole time it roams passive, so by the time you reach it they're all at
            // 0). Miss any and that attack lands instantly, un-blockable: a leveled melee/crit
            // chunks ~80% of the hero's HP, and the signature (melee×1.6, the biggest single hit)
            // detonates on the wake frame — all before the player can react.
            b.atk_cd = MELEE_CD;
            b.sig_cd = b.sig_cd.max(SIG_CD * 0.5);
            b.crit_cd = b.crit_cd.max(CRIT_CD * 0.5);
            let gy = steer::footing(b.pos.x, b.pos.y).unwrap_or(tf.translation.y);
            cues.write(crate::audio::AudioCue::BossRoar(Vec3::new(b.pos.x, gy + 1.8, b.pos.y)));
        }
        b.last_hp = health.hp;

        let slow = slowed.map(|s| s.factor).unwrap_or(1.0);
        let hero_d = if hero.alive { b.pos.distance(hero.pos) } else { f32::INFINITY };

        // Leash: break off if the hero leaves the warden's home region (or falls), and head home.
        if b.hostile && (!hero.alive || b.home.distance(hero.pos) > BIOME_LEASH) {
            b.hostile = false;
            b.crit_at = 0.0; // abandon any windup if the hero flees the region / falls
            b.roam_target = b.home;
            b.roam_timer = 8.0;
        }

        if b.hostile && hero.alive && b.crit_at > 0.0 {
            // ── Winding up a critical: the warden plants and rears its weapon overhead (the limb
            //    pose lives in `boss_limbs`), tracking the hero slowly. On impact the blow falls —
            //    LETHAL if it connects, but blocking/dodging negates it (see `apply_hero_damage`). ──
            any_winding = true;
            b.moving = false;
            let to = hero.pos - b.pos;
            if to.length_squared() > 1e-4 {
                let want = to.x.atan2(to.y);
                let turn = TURN * dt; // slow tracking — the player can still circle out of range
                b.facing += steer::wrap_pi(want - b.facing).clamp(-turn, turn);
            }
            if now >= b.crit_at {
                b.crit_at = 0.0;
                let gy = steer::footing(b.pos.x, b.pos.y).unwrap_or(tf.translation.y);
                if hero_d < CRIT_RANGE {
                    crit.0 = true;
                    pending.0 += CRIT_LETHAL; // negated entirely if the hero is blocking / dodging
                    cues.write(crate::audio::AudioCue::Slam);
                }
                if let Some(fx) = &fx {
                    crate::player::spawn_shockwave(
                        &mut commands,
                        fx,
                        &mut materials,
                        Vec3::new(b.pos.x, gy + 0.05, b.pos.y),
                        now,
                    );
                }
            }
        } else if b.hostile && hero.alive {
            // Face + chase the hero; stand and strike in range.
            if hero_d > MELEE_RANGE {
                let cur_y = steer::footing(b.pos.x, b.pos.y).unwrap_or(tf.translation.y);
                if let Some(s) = steer::advance(b.pos, b.facing, hero.pos, SPEED * slow * dt, BODY_R, cur_y, TURN * dt) {
                    b.facing = s.facing;
                    b.pos = s.pos;
                    b.moving = s.moving;
                } else {
                    b.moving = false;
                }
            } else {
                b.moving = false;
                let to = hero.pos - b.pos;
                if to.length_squared() > 1e-4 {
                    let want = to.x.atan2(to.y);
                    let turn = TURN * 2.0 * dt;
                    b.facing += steer::wrap_pi(want - b.facing).clamp(-turn, turn);
                }
                if b.atk_cd <= 0.0 {
                    b.atk_cd = MELEE_CD;
                    b.atk_anim = now;
                    pending.0 += melee_dmg(b.level);
                }
            }
            // Begin a telegraphed critical when off cooldown and the hero is in striking range.
            if b.crit_cd <= 0.0 && hero_d < CRIT_RANGE {
                b.crit_cd = CRIT_CD;
                b.crit_at = now + CRIT_TELEGRAPH;
                let gy = steer::footing(b.pos.x, b.pos.y).unwrap_or(tf.translation.y);
                let head = Vec3::new(b.pos.x, gy + 1.8, b.pos.y);
                // Two-layer telegraph: the deep roar (weight) + a rising charge whine (the distinct
                // "killing blow is charging" cue, escalating across the 1.2s windup).
                cues.write(crate::audio::AudioCue::BossRoar(head));
                cues.write(crate::audio::AudioCue::BossWindup(head));
                if !*taught_crit {
                    *taught_crit = true;
                    notice.push(
                        "The warden rears back for a killing blow — raise your shield (hold RMB) or dodge clear!".to_string(),
                        now as f64,
                    );
                }
            }
            // Signature on its own cooldown once the hero is within a reasonable band.
            if b.sig_cd <= 0.0 && hero_d < SIG_SHOCK_RADIUS * 2.0 {
                b.sig_cd = SIG_CD;
                b.sig_anim = now;
                let gy = steer::footing(b.pos.x, b.pos.y).unwrap_or(tf.translation.y);
                let sig_dmg = melee_dmg(b.level) * SIG_MULT;
                match signature(b.biome) {
                    Signature::Shock => {
                        if hero_d < SIG_SHOCK_RADIUS {
                            pending.0 += sig_dmg;
                        }
                        if let Some(fx) = &fx {
                            crate::player::spawn_shockwave(
                                &mut commands,
                                fx,
                                &mut materials,
                                Vec3::new(b.pos.x, gy + 0.05, b.pos.y),
                                now,
                            );
                        }
                    }
                    Signature::Volley(n) => {
                        let each = if n > 1 { sig_dmg * 0.6 } else { sig_dmg };
                        for _ in 0..n {
                            bolts.0.push(BoltSpawn {
                                origin: Vec3::new(b.pos.x, gy + 1.8, b.pos.y),
                                damage: each,
                            });
                        }
                    }
                }
            }
        } else {
            // Passive roam around its home region — but if it strayed far (e.g. just broke off a
            // chase), march straight home first.
            b.roam_timer -= dt;
            if b.roam_timer <= 0.0 || b.pos.distance(b.roam_target) < 1.0 {
                if b.pos.distance(b.home) > ROAM_RADIUS {
                    b.roam_target = b.home;
                    b.roam_timer = 8.0;
                } else {
                    let a = rng01(&mut b.rng) * std::f32::consts::TAU;
                    let r = rng01(&mut b.rng) * ROAM_RADIUS;
                    b.roam_target = b.home + Vec2::new(a.cos() * r, a.sin() * r);
                    b.roam_timer = 3.0 + rng01(&mut b.rng) * 5.0;
                }
            }
            let cur_y = steer::footing(b.pos.x, b.pos.y).unwrap_or(tf.translation.y);
            match steer::advance(b.pos, b.facing, b.roam_target, SPEED * 0.5 * slow * dt, BODY_R, cur_y, TURN * dt) {
                Some(s) => {
                    b.facing = s.facing;
                    b.pos = s.pos;
                    b.moving = s.moving;
                }
                None => {
                    b.moving = false;
                    b.roam_timer = 0.0;
                }
            }
        }

        // Place the root: ground-follow + a step bob.
        let gy = steer::footing(b.pos.x, b.pos.y).unwrap_or(tf.translation.y);
        let bob = if b.moving { (tw * 5.0 + b.phase).sin().abs() * 0.08 } else { (tw * 1.2).sin() * 0.03 };
        tf.translation = Vec3::new(b.pos.x, gy + bob, b.pos.y);
        tf.rotation = Quat::from_rotation_y(b.facing);
    }
    tension.0 = any_winding;
}

/// One-time "a beast stirs" notice when the hero first nears a living, un-discovered warden.
fn boss_proximity(
    time: Res<Time>,
    hero: Res<HeroState>,
    mgr: Res<VoiceManager>,
    mut notice: ResMut<Notice>,
    mut speak: MessageWriter<Speak>,
    mut q: Query<&mut Boss, Without<Dying>>,
) {
    if !hero.alive {
        return;
    }
    let now = time.elapsed_secs_f64();
    for mut b in &mut q {
        if b.seen {
            continue;
        }
        if b.pos.distance(hero.pos) < NOTICE_RANGE {
            b.seen = true;
            notice.push(format!("Something massive stirs in the {}…", biome_word(b.biome)), now);
            // First warden of the run → the hero's generic "why hunt it" teach (`warden_near`,
            // a `once` line); after it's played, every sighting gets that warden's biome flavor.
            let concept = if mgr.played_once.contains("warden_near") {
                Concept::NearWarden(b.biome)
            } else {
                Concept::WardenSighted
            };
            speak.write(Speak::new(concept));
        }
    }
}

/// Drive the boss-fight music layer: on while any warden is engaged (turned hostile). `music.rs`
/// swells the boss track over the daytime mix from this flag.
fn warden_music_flag(bosses: Query<&Boss, Without<Dying>>, mut music: ResMut<MusicState>) {
    music.warden_active = bosses.iter().any(|b| b.hostile);
}

fn biome_word(b: Biome) -> &'static str {
    match b {
        Biome::Forest => "deep woods",
        Biome::Snow => "frozen wastes",
        Biome::Rocky => "crags",
        Biome::Desert => "dunes",
        Biome::Swamp => "mire",
    }
}

/// Each dawn (a night cleared → back to Prep), every living warden gains a level and heals to full.
fn boss_levelup(
    siege: Res<Siege>,
    mut q: Query<(&mut Boss, &mut Health), Without<Dying>>,
    mut prev: Local<Option<GamePhase>>,
) {
    let dawn = matches!(*prev, Some(GamePhase::Wave)) && siege.phase == GamePhase::Prep;
    *prev = Some(siege.phase);
    // Guard the fresh-run false trigger: this system is frozen (Modal gone) whenever we leave
    // Playing, so a death mid-siege leaves `prev == Some(Wave)`. A New Game / Restart then resets
    // Siege to Prep (wave_index = -1) and this would false-fire a level-up on day 1 before a single
    // night is fought. A *real* dawn always follows a night, so wave_index is >= 0 then — mirrors
    // the identical guard in `savegame::autosave_on_dawn`.
    if !dawn || siege.wave_index < 0 {
        return;
    }
    for (mut b, mut h) in &mut q {
        b.level += 1;
        h.max = max_hp(b.level);
        h.hp = h.max;
        b.last_hp = h.hp; // don't read the heal as a hit
    }
}

/// Tick venom DoT (heals the hero a fraction) + expire spent Slowed/Poisoned.
fn tick_status(
    time: Res<Time>,
    mut commands: Commands,
    mut player: ResMut<PlayerRes>,
    mut poisoned: Query<(Entity, &Poisoned, &mut Health), Without<Dying>>,
    slowed: Query<(Entity, &Slowed)>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    for (e, p, mut h) in &mut poisoned {
        if now >= p.until {
            commands.entity(e).remove::<Poisoned>();
            continue;
        }
        let dmg = p.dps * dt;
        h.hp -= dmg;
        player.0.heal((dmg * 0.25) as f64); // venom lifesteal
        if h.hp <= 0.0 {
            begin_dying(&mut commands, e, now);
        }
    }
    for (e, s) in &slowed {
        if now >= s.until {
            commands.entity(e).remove::<Slowed>();
        }
    }
}

/// Marks a warden whose death boon has already been granted, so `reward_on_death` pays out exactly
/// once without relying on catching the single `Added<Dying>` frame.
#[derive(Component)]
struct Rewarded;

/// A warden just fell: grant its boon, queue the reward dialog, announce it. Keys off `With<Dying>`
/// (not `Added<Dying>`) + a `Rewarded` marker rather than the one-frame `Added` filter: a corpse
/// stays `Dying` for the whole ~1.4s fade, so even if a panel opens on the death frame (this system
/// is `Modal::None`-gated) the boon still pays out once the panel closes — instead of being lost.
fn reward_on_death(
    time: Res<Time>,
    mut commands: Commands,
    mut player: ResMut<PlayerRes>,
    mut pending: ResMut<PendingReward>,
    mut next_modal: ResMut<NextState<Modal>>,
    mut notice: ResMut<Notice>,
    q: Query<(Entity, &Boss), (With<Dying>, Without<Rewarded>)>,
) {
    for (e, b) in &q {
        commands.entity(e).try_insert(Rewarded);
        // Grant the permanent boon.
        match b.biome {
            Biome::Forest => player.0.has_bramble_sweep = true,
            Biome::Snow => player.0.frostbite = true,
            Biome::Rocky => player.0.has_ground_slam = true,
            Biome::Desert => player.0.has_sand_dash = true,
            Biome::Swamp => player.0.venom = true,
        }
        let (boon_name, boon_desc) = boon_for(b.biome);
        pending.0 = Some(RewardInfo { boss_name: models::name(b.biome), boon_name, boon_desc });
        notice.push(format!("{} is slain!", models::name(b.biome)), time.elapsed_secs_f64());
        next_modal.set(Modal::BossReward);
    }
}

// ── Limb animation (legs stride, arms sway, head bob; arms raise on a signature) ─────────
fn boss_limbs(
    time: Res<Time>,
    bosses: Query<(&Boss, &Children)>,
    mut parts: Query<(&BossPart, &mut Transform)>,
) {
    use crate::critters::PartKind;
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (b, children) in &bosses {
        let t = tw + b.phase;
        // Signature windup: arms raise high for ~0.5s after a cast.
        let sig = b.sig_anim > 0.0 && (now - b.sig_anim) < 0.5;
        let strike = b.atk_anim > 0.0 && (now - b.atk_anim) < 0.45;
        // Critical windup: both arms haul progressively overhead across the telegraph — a long,
        // unmistakable "killing blow incoming" tell (the player's cue to raise the shield).
        let crit_wind = b.crit_at > 0.0 && now < b.crit_at;
        let crit_p = if crit_wind { (1.0 - (b.crit_at - now) / CRIT_TELEGRAPH).clamp(0.0, 1.0) } else { 0.0 };
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                PartKind::Leg(sign) => {
                    let s = if b.moving { (t * 4.5).sin() * 0.5 } else { (t * 0.7).sin() * 0.03 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Arm(sign) => {
                    if crit_wind {
                        Quat::from_rotation_x(-1.0 - 2.0 * crit_p) // rear overhead, rising to the strike
                    } else if sig {
                        Quat::from_rotation_x(-2.0) // both arms reared back for the blast
                    } else if sign > 0.0 && strike {
                        let p = (now - b.atk_anim) / 0.45;
                        Quat::from_rotation_x(-1.4 + 3.0 * (p * std::f32::consts::PI).sin())
                    } else {
                        let s = if b.moving { -(t * 4.5).sin() * 0.4 } else { (t * 0.7).sin() * 0.05 };
                        Quat::from_rotation_x(sign * s)
                    }
                }
                PartKind::Head => {
                    let yaw = (t * 0.5).sin() * 0.15;
                    Quat::from_rotation_y(yaw)
                }
                PartKind::Tail => Quat::IDENTITY,
            };
        }
    }
}

// ── Boss health bar (top-centre, while a warden is engaged) ──────────────────────────────
#[derive(Component)]
struct BossBarRoot;
#[derive(Component)]
struct BossBarName;
#[derive(Component)]
struct BossBarFill;

fn sync_boss_bar(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    hero: Res<HeroState>,
    bosses: Query<(&Boss, &Health), Without<Dying>>,
    root: Query<Entity, With<BossBarRoot>>,
    mut name_q: Query<&mut Text, With<BossBarName>>,
    mut fill_q: Query<&mut Node, With<BossBarFill>>,
) {
    // The engaged warden (only one fight at a time in practice); none once the hero is down.
    let engaged = if hero.alive {
        bosses
            .iter()
            .filter(|(b, _)| b.hostile)
            .map(|(b, h)| (b, (h.hp / h.max.max(1.0)).clamp(0.0, 1.0)))
            .next()
    } else {
        None
    };

    match (engaged, root.single()) {
        (Some((b, ratio)), Ok(_)) => {
            if let Ok(mut t) = name_q.single_mut() {
                **t = format!("{}   —   Lv {}", models::name(b.biome), b.level);
            }
            if let Ok(mut n) = fill_q.single_mut() {
                n.width = Val::Percent(ratio * 100.0);
            }
        }
        (Some((b, ratio)), Err(_)) => spawn_boss_bar(&mut commands, &fonts, models::name(b.biome), b.level, ratio),
        (None, Ok(e)) => commands.entity(e).try_despawn(),
        (None, Err(_)) => {}
    }
}

/// Tear the bar down when the run ends (game-over / start screen) so it never lingers behind
/// the game-over card — `sync_boss_bar` is gated to `Playing`, so it can't reap it itself there.
fn despawn_boss_bar(mut commands: Commands, q: Query<Entity, With<BossBarRoot>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

fn spawn_boss_bar(commands: &mut Commands, fonts: &UiFonts, name: &str, level: i32, ratio: f32) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                // Bottom-centre (Souls-style) so it never collides with the top siege objective banner.
                bottom: Val::Px(86.0),
                left: Val::Percent(50.0),
                width: Val::Px(520.0),
                margin: UiRect::left(Val::Px(-260.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
            GlobalZIndex(40),
            BossBarRoot,
            anim(AnimKind::Rise, 0.0, 0.4),
        ))
        .with_children(|r| {
            r.spawn((
                label(&fonts.display, &format!("{name}   —   Lv {level}"), 18.0, rgb(255, 224, 170)),
                TextShadow { offset: Vec2::new(0.0, 2.0), color: rgba(0, 0, 0, 0.8) },
                BossBarName,
            ));
            // Track + fill.
            r.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(14.0),
                    border: widgets::border(1.0),
                    border_radius: radius(6.0),
                    ..default()
                },
                BackgroundColor(rgba(20, 12, 10, 0.82)),
                BorderColor::all(rgba(224, 168, 74, 0.7)),
            ))
            .with_children(|track| {
                track.spawn((
                    Node {
                        width: Val::Percent(ratio * 100.0),
                        height: Val::Percent(100.0),
                        border_radius: radius(6.0),
                        ..default()
                    },
                    BackgroundColor(rgb(196, 54, 44)),
                    BossBarFill,
                ));
            });
        });
}

// ── Reward dialog (Modal::BossReward) ────────────────────────────────────────────────────
#[derive(Component)]
struct RewardUi;
#[derive(Component)]
struct RewardContinueBtn;

fn spawn_reward_ui(mut commands: Commands, fonts: Res<UiFonts>, pending: Res<PendingReward>) {
    let Some(info) = pending.0.clone() else { return };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(SCRIM),
            GlobalZIndex(90),
            RewardUi,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(12.0),
                    width: Val::Px(460.0),
                    padding: UiRect::axes(Val::Px(40.0), Val::Px(30.0)),
                    border: widgets::border(1.0),
                    border_radius: radius(R_PANEL),
                    ..default()
                },
                widgets::card_paint(),
                anim(AnimKind::PopIn, 0.0, 0.28),
            ))
            .with_children(|c| {
                c.spawn(label(&fonts.display, "WARDEN SLAIN", 30.0, GOLD));
                c.spawn(label(&fonts.semibold, info.boss_name, 14.0, TEXT_DIM));
                // Boon card.
                c.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(6.0),
                        width: Val::Percent(100.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        margin: UiRect::vertical(Val::Px(6.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_CARD),
                        ..default()
                    },
                    BackgroundColor(rgba(36, 28, 16, 0.7)),
                    BorderColor::all(rgba(224, 168, 74, 0.5)),
                ))
                .with_children(|box_| {
                    box_.spawn(label(&fonts.semibold, "NEW ABILITY", 11.0, KICKER));
                    box_.spawn(label(&fonts.display, info.boon_name, 22.0, rgb(255, 224, 170)));
                    box_.spawn(label(&fonts.regular, &info.boon_desc, 14.0, TEXT_DIM));
                });
                c.spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(40.0), Val::Px(12.0)),
                        border: widgets::border(1.0),
                        border_radius: radius(R_BTN),
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    },
                    widgets::btn_primary_paint(), // already bundles Button + Interaction
                    RewardContinueBtn,
                    anim_btn(AnimKind::PopIn, 0.1, 0.28),
                ))
                .with_children(|b| {
                    b.spawn(label(&fonts.extrabold, "CONTINUE", 16.0, INK));
                });
                c.spawn(label(&fonts.regular, "Enter / Esc to continue", 12.0, GREY));
            });
        });
}

fn reward_dismiss(
    keys: Res<ButtonInput<KeyCode>>,
    q: Query<&Interaction, (Changed<Interaction>, With<RewardContinueBtn>)>,
    mut pending: ResMut<PendingReward>,
    mut next_modal: ResMut<NextState<Modal>>,
) {
    let click = q.iter().any(|i| *i == Interaction::Pressed);
    if click
        || keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::Space)
        || keys.just_pressed(KeyCode::Escape)
    {
        pending.0 = None;
        next_modal.set(Modal::None);
    }
}

fn despawn_reward_ui(mut commands: Commands, q: Query<Entity, With<RewardUi>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ── Deterministic mulberry32 ─────────────────────────────────────────────────────────────
fn rng01(s: &mut u32) -> f32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}
