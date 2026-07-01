//! **Orks** — the camp warbands. Box-mesh humanoids ported 1:1 from the TS `Ork.tsx` mesh
//! tree, plus a full combat AI: each ork is home-anchored to its camp and runs an
//! Idle / Patrol / Hunt / Attack state machine — idling within the camp until the hero (or a
//! rival-faction ork) comes into range, then hunting via A* (`navgrid`) and striking. Berserkers
//! frenzy under 40% HP. (The night sieges marching on the keep are a separate brain in
//! `siege.rs`; this module owns the camp warbands.)
//!
//! Like `critters`, an ork is a small entity hierarchy: a static **torso** (legs become
//! articulated) plus articulated **parts** — 2 legs, 2 arms (the right arm carries a baked
//! club, or a staff + orb for the shaman) and a head. The limbs swing procedurally (the
//! `wind.rs` / `animal_limbs` sin trick). Meshes are merged, flat-shaded and vertex-coloured
//! against one shared white material, so a whole warband batches into few draw calls.
//!
//! Variants (grunt / scout / berserker / shaman) differ in skin, scale and weapon; the camp's
//! **faction** (red / blue) tints the loincloth + war-paint so rival camps read apart. All 8
//! variant×faction meshes are built once into an [`Armory`] and clone-spawned per camp.

use std::f32::consts::TAU;

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::critters::PartKind;
use crate::palette::{lin, lin_scaled};
use crate::steer;
use crate::worldmap;

/// Base root scale (the TS group scale before the per-variant `cfg.scale`). Was 0.7, bumped ×1.35
/// with the hero/house rescale, then cut 15% (×0.85) so orks read a touch shorter than the hero.
pub(crate) const BASE_SCALE: f32 = 0.80325;
/// Orks turn slower than wildlife — a lumbering pivot. rad/s.
pub(crate) const ORK_MAX_TURN: f32 = 2.5;

// ── Combat (M3): a camp ork aggros on the hero, charges, clubs him, leashes home. ──
// (`pub(crate)` so the night-wave invader AI in `siege.rs` reuses the same tuning.)
/// How close the hero must come for an ork to notice + give chase.
pub(crate) const ORK_SIGHT: f32 = 9.0;
/// Within this of the hero, the ork stops and strikes instead of chasing.
pub(crate) const ORK_ATTACK_RANGE: f32 = 1.5;
/// Max distance from its home camp an ork will pursue — keeps each warband local.
const ORK_LEASH: f32 = 16.0;
/// Damage per club hit (queued onto `player::PendingHeroDamage`). Old-game grunt `orkConfig.ts`
/// damage is 24; this is an intentional **−10% playtest nerf** (24 → 21.6) because the hero was
/// dying too fast on Normal — a deliberate divergence from parity, not a reintroduced rescale.
/// `variant_melee` anchors every other variant off this via the core damage ratio, so they all
/// drop ~10% in lockstep.
pub(crate) const ORK_DAMAGE: f32 = 21.6;
/// Seconds between an ork's strikes.
pub(crate) const ORK_ATTACK_CD: f32 = 1.1;

// ── Shaman (ranged caster) — ported from `orkConfig.ts` shaman, scaled to this scene. ──
/// A shaman stands off and casts once the hero is within this range (no melee charge).
pub(crate) const SHAMAN_CAST_RANGE: f32 = 8.0;
/// Seconds between bolt casts.
pub(crate) const SHAMAN_CAST_CD: f32 = 2.1;
/// Bolt damage — old-game shaman `orkConfig.ts` value is 26; same intentional **−10% playtest
/// nerf** as `ORK_DAMAGE` (26 → 23.4), still above the club as in the original.
pub(crate) const SHAMAN_BOLT_DAMAGE: f32 = 23.4;
/// A shaman heals the nearest wounded ally within this range.
const SHAMAN_HEAL_RANGE: f32 = 8.0;
/// HP restored per heal (old-game shaman `healAmount`).
const SHAMAN_HEAL_AMOUNT: f32 = 24.0;
/// Seconds between heals.
const SHAMAN_HEAL_CD: f32 = 5.0;

// ── Variants & factions ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrkVariant {
    Grunt,
    Scout,
    Berserker,
    Shaman,
}
pub const VARIANTS: [OrkVariant; 4] =
    [OrkVariant::Grunt, OrkVariant::Scout, OrkVariant::Berserker, OrkVariant::Shaman];

/// Map the forest variant onto the core `ork_config` variant (same names, distinct types).
pub(crate) fn core_variant(v: OrkVariant) -> tileworld_core::ork_config::OrkVariant {
    use tileworld_core::ork_config::OrkVariant as C;
    match v {
        OrkVariant::Grunt => C::Grunt,
        OrkVariant::Scout => C::Scout,
        OrkVariant::Berserker => C::Berserker,
        OrkVariant::Shaman => C::Shaman,
    }
}

/// Gold a slain ork drops, after the player's bounty multiplier (HP-independent — port as-is).
pub(crate) fn bounty_gold(v: OrkVariant, bounty_mult: f64) -> i64 {
    tileworld_core::ork_config::ork_bounty_gold(core_variant(v), bounty_mult)
}
/// XP a slain ork drops (the bounty boon is gold-only).
pub(crate) fn bounty_xp(v: OrkVariant) -> i64 {
    tileworld_core::ork_config::ork_bounty_xp(core_variant(v))
}

/// Per-variant melee damage to the hero in this scene's combat units. The Grunt is the
/// `ORK_DAMAGE` anchor; others scale by the `ork_config` damage ratio (with core's Scout 15 /
/// Berserker 30 vs Grunt 24, that lands Scout ≈14, Berserker ≈27). The Shaman casts
/// `SHAMAN_BOLT_DAMAGE` instead, so this is unused for it.
pub(crate) fn variant_melee(v: OrkVariant) -> f32 {
    use tileworld_core::ork_config::{ork_config, OrkVariant as C};
    let grunt = ork_config(C::Grunt).damage as f32; // core ratio anchor (24); ORK_DAMAGE is the nerfed scene value
    (ORK_DAMAGE * ork_config(core_variant(v)).damage as f32 / grunt).round()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Red,
    Blue,
}

impl Faction {
    /// Warband colour (sRGB hex) — loincloth/war-paint tint + the camp banner flag.
    pub fn hex(self) -> u32 {
        match self {
            Faction::Red => 0x9a2a22,
            Faction::Blue => 0x26468f,
        }
    }
}

/// Per-variant look + ambient behaviour (the viewer subset of `orkConfig.ts`).
#[derive(Clone, Copy)]
struct Stats {
    skin: u32,
    scale: f32,
    speed: f32,
    gait: f32,
    swing: f32,
    bob: f32,
    wander_r: f32,
    body_r: f32,
    shaman: bool,
}

fn stats(v: OrkVariant) -> Stats {
    match v {
        // Brutes are bulked up so the warband reads as a hulking threat; the Scout stays
        // small + nimble (the lithe outrider) as the deliberate odd one out.
        OrkVariant::Grunt => Stats { skin: 0x3a6a2a, scale: 1.35, speed: 1.6, gait: 7.0, swing: 0.35, bob: 0.05, wander_r: 3.0, body_r: 0.36, shaman: false },
        OrkVariant::Scout => Stats { skin: 0x5f9a3c, scale: 0.78, speed: 2.3, gait: 9.0, swing: 0.42, bob: 0.07, wander_r: 3.6, body_r: 0.22, shaman: false },
        OrkVariant::Berserker => Stats { skin: 0x7a3a26, scale: 1.45, speed: 1.9, gait: 8.0, swing: 0.42, bob: 0.06, wander_r: 3.2, body_r: 0.40, shaman: false },
        OrkVariant::Shaman => Stats { skin: 0x6a3f86, scale: 1.3, speed: 1.3, gait: 6.0, swing: 0.30, bob: 0.04, wander_r: 2.6, body_r: 0.35, shaman: true },
    }
}

// ── Components ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum OrkMode {
    Idle,
    Patrol,
    /// Charging the hero.
    Hunt,
    /// In range — standing and clubbing.
    Attack,
}

#[derive(Component)]
pub struct Ork {
    home: Vec2,
    /// This ork's personal patrol spot in the camp (its `WARBAND` ring offset), distinct from the
    /// shared camp `home`/centre. Idle wander + the return-to-camp target orbit THIS, so a warband
    /// fans out around the fire instead of all four piling onto the dead centre. (`home` stays the
    /// camp centre — leash range + the respawn camp-match key off it.)
    anchor: Vec2,
    target: Vec2,
    // `pub(crate)` fields are the ones the night-wave invader brain in `siege.rs` reads/writes.
    pub(crate) pos: Vec2,
    pub(crate) facing: f32,
    pub(crate) speed: f32,
    wander_r: f32,
    gait: f32,
    swing: f32,
    bob: f32,
    pub(crate) body_r: f32,
    phase: f32,
    pub(crate) moving: bool,
    mode: OrkMode,
    /// A camp "rooster": spawned seated on a stump and held in the sit pose while idle; it STANDS
    /// and fights the instant the hero comes within [`ORK_SIGHT`], then behaves like any warband
    /// ork. Cleared to `false` on first engage so it never re-roosts mid-fight. Default `false`.
    seated_rest: bool,
    timer: f32,
    /// Strike cooldown (s) — counts down; a hit fires at ≤ 0 and resets it.
    pub(crate) atk_cd: f32,
    /// Timestamp (`elapsed_secs`) the last strike fired; `ork_limbs` plays a club-chop (or the
    /// shaman's staff-jab) for [`ATTACK_ANIM_DUR`] after it. `0` = none yet. Stamped by both the
    /// camp brain and the night-wave invader brain (`siege.rs`).
    pub(crate) atk_anim: f32,
    /// Timestamp of the last blow the ork TOOK; drives a brief springy recoil-wobble (the same
    /// read as the training dummies'). Stamped by `player::combat`. `0` = none.
    pub(crate) hit_recoil: f32,
    /// This ork is the camp shaman (casts bolts + heals instead of clubbing).
    pub(crate) shaman: bool,
    /// Heal cooldown (s) — shamans only.
    heal_cd: f32,
    rng: u32,
    /// Variant + warband — read by combat (per-variant HP/damage/bounty) and the shaman
    /// heal (same-faction only).
    pub(crate) variant: OrkVariant,
    pub(crate) faction: Faction,
    /// Decaying knockback shove (world XZ, units/s) from a recent hit — applied + slid against
    /// terrain each frame by the brains.
    pub(crate) kb: Vec2,
    /// A rival-faction ork this one is brawling (no hero in sight). The Attack arm skips the
    /// hero blow while this is `Some`; [`ork_brawl`] deals the melee to the rival.
    brawl_target: Option<Entity>,
    /// Brawl strike cooldown (s), separate from the hero-attack `atk_cd`.
    brawl_cd: f32,
    /// Cached A* route to the hero while hunting (camp orks route around walls/props instead of
    /// wedging on them). Invaders ignore these — they use their own `navgrid::NavPath`.
    hunt_path: Vec<Vec2>,
    hunt_cursor: usize,
    /// Game-time to recompute the hunt path (throttled + staggered per ork).
    hunt_replan_at: f32,
    /// Hero position the cached path was planned for (replan once it drifts).
    hunt_goal: Vec2,
}

/// Rival warbands trade blows when they close (`factions::orks_hostile` — Red vs Blue).
const BRAWL_RANGE: f32 = 1.8;
const BRAWL_CD: f32 = 1.2;
const BRAWL_DMG: f32 = 6.0;

impl Ork {
    /// The camp this ork is anchored to (world XZ) — read by the rescue check to tell whether a
    /// camp's warband is still alive.
    pub(crate) fn home(&self) -> Vec2 {
        self.home
    }
}

/// Apply + decay one ork's knockback this frame, sliding it against terrain (so a shove can't
/// punt an ork through a cliff/wall). Shared by the camp + invader brains.
pub(crate) fn apply_knockback(o: &mut Ork, dt: f32) {
    if o.kb.length_squared() <= 0.0025 {
        o.kb = Vec2::ZERO;
        return;
    }
    let cur_y = steer::footing(o.pos.x, o.pos.y).unwrap_or(0.0);
    // Already wedged inside a prop/building footprint? Waive the blocker test so the shove can
    // carry the ork OUT — same escape rule `steer::step_clear` uses (else a corner traps it).
    let inside = crate::blockers::is_blocked(o.pos.x, o.pos.y);
    let step = o.kb * dt;
    let nx = o.pos.x + step.x;
    let nz = o.pos.y + step.y;
    if steer::can_stand(nx, o.pos.y, o.body_r, cur_y)
        && (inside || !crate::blockers::is_blocked(nx, o.pos.y))
    {
        o.pos.x = nx;
    }
    if steer::can_stand(o.pos.x, nz, o.body_r, cur_y)
        && (inside || !crate::blockers::is_blocked(o.pos.x, nz))
    {
        o.pos.y = nz;
    }
    o.kb *= (1.0 - 9.0 * dt).max(0.0);
}

#[derive(Component)]
pub(crate) struct OrkPart {
    pub(crate) kind: PartKind,
}

/// A glowing ork eye (emissive sphere child of the ork root) — menacing pinpoints that read at
/// night when the rest of the ork goes dark. Tagged so `combat_fx`'s per-ork skin-clone leaves
/// its emissive material alone (it would otherwise overwrite the glow with the body skin).
#[derive(Component)]
pub(crate) struct OrkEye;

/// Tree-local eye positions on the ork root (the head's eye boxes lifted to the root frame).
const EYE_OFFS: [Vec3; 2] = [Vec3::new(-0.08, 1.12, 0.24), Vec3::new(0.08, 1.12, 0.24)];

/// Tags a night-wave invader (vs a leashed camp ork). Spawned + driven by `siege.rs`; the
/// camp [`ork_brain`] skips these via `Without<WaveInvader>` so the two AIs stay separate.
/// Carries the stuck-cull progress tracking (no keep-ward progress for too long → reaped so a
/// wave can't hang on a wedged ork — see `siege::stuck_step`).
#[derive(Component)]
pub struct WaveInvader {
    /// Closest approach to the keep so far (min distance); progress = this strictly dropping.
    pub closest: f32,
    /// Game-time the closest approach last improved; stale past the timeout → reaped.
    pub progress_at: f32,
}

// ── Plugin + systems ───────────────────────────────────────────────────────────────

pub struct OrksPlugin;

impl Plugin for OrksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (ork_limbs, ork_drive)); // limb anim keeps running while frozen
        app.add_systems(
            Update,
            (ork_brain, ork_brawl, shaman_heal).run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

fn ork_brain(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    mut pending: ResMut<crate::player::PendingHeroDamage>,
    mut bolts: ResMut<crate::projectile::BoltSpawns>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut music: ResMut<crate::audio::MusicState>,
    mut was_clearing: Local<bool>,
    mut q: Query<
        (Entity, &mut Ork, &mut Transform, Option<&crate::player::Health>, Option<&crate::boss::Slowed>),
        (
            Without<WaveInvader>,
            Without<crate::dying::Dying>,
            // Staged fortress marchers are driven by `siege::director_march`, not the camp brain.
            Without<crate::cinematic::DirectorMarcher>,
            // Staged-scene orks (trailer tableaus) are driven by `scenes.rs`, not the camp brain.
            Without<crate::scenes::SceneActor>,
        ),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();

    // True if ANY ork is engaged this frame → swells the combat music layer.
    let mut fighting = false;

    // Snapshot (entity, faction, pos) so an ork with no hero in sight can seek a rival to brawl.
    let snap: Vec<(Entity, Faction, Vec2)> = q.iter().map(|(e, o, _, _, _)| (e, o.faction, o.pos)).collect();

    for (self_e, mut o, mut tf, health, slowed) in &mut q {
        // Frostbite boon: a chilled ork crawls (factor < 1; 0 = frozen).
        let slow_mul = slowed.map(|s| s.factor).unwrap_or(1.0);
        o.timer -= dt;
        o.atk_cd -= dt;
        let prev_mode = o.mode;
        // Berserker frenzy: under 40% HP it charges faster + strikes more often.
        let frenzied =
            o.variant == OrkVariant::Berserker && health.is_some_and(|h| h.hp < h.max * 0.4);

        // ── Aggro: notice the hero near the camp → chase, then stand & strike; orks far
        // from their home never engage (each warband stays local). ──
        let see_hero = hero.alive
            && o.pos.distance(hero.pos) < ORK_SIGHT
            && o.home.distance(hero.pos) < ORK_LEASH;
        let atk_range = if o.shaman { SHAMAN_CAST_RANGE } else { ORK_ATTACK_RANGE };
        if see_hero {
            o.target = hero.pos;
            o.brawl_target = None;
            o.mode = if o.pos.distance(hero.pos) < atk_range { OrkMode::Attack } else { OrkMode::Hunt };
        } else {
            // No hero — seek the nearest rival-faction ork near home to brawl.
            let mut rival: Option<(Entity, Vec2)> = None;
            let mut best = ORK_SIGHT;
            for (re, rf, rp) in &snap {
                if *re == self_e || *rf == o.faction {
                    continue;
                }
                let d = o.pos.distance(*rp);
                if d < best && o.home.distance(*rp) < ORK_LEASH {
                    best = d;
                    rival = Some((*re, *rp));
                }
            }
            if let Some((re, rp)) = rival {
                o.brawl_target = Some(re);
                o.target = rp;
                o.mode = if o.pos.distance(rp) < ORK_ATTACK_RANGE { OrkMode::Attack } else { OrkMode::Hunt };
            } else {
                o.brawl_target = None;
                if matches!(o.mode, OrkMode::Hunt | OrkMode::Attack) {
                    o.mode = OrkMode::Idle;
                    o.timer = rng_range(&mut o.rng, 0.5, 1.5);
                    o.target = o.anchor; // amble back to its own spot, not the camp's dead centre
                }
            }
        }

        // Spatial grunt on first aggro, and again on closing to strike range (Hunt→Attack) —
        // the two beats the old `Ork.tsx` grunts on (acquire target / start a swing). Per-ork
        // transition edges, so no per-frame spam.
        let was_engaged = matches!(prev_mode, OrkMode::Hunt | OrkMode::Attack);
        let now_engaged = matches!(o.mode, OrkMode::Hunt | OrkMode::Attack);
        if (!was_engaged && now_engaged) || (prev_mode == OrkMode::Hunt && o.mode == OrkMode::Attack) {
            let gy = steer::footing(o.pos.x, o.pos.y).unwrap_or(0.0);
            cues.write(crate::audio::AudioCue::OrkGrunt(Vec3::new(o.pos.x, gy + 1.0, o.pos.y)));
        }
        if now_engaged {
            fighting = true;
            // A roosting guard gets to its feet for good on its first engage — never sits back down.
            o.seated_rest = false;
        }

        match o.mode {
            OrkMode::Idle => {
                o.moving = false;
                // A roosting guard stays put on its stump (no idle patrol) until something wakes it.
                if o.timer <= 0.0 && !o.seated_rest {
                    pick_patrol(&mut o);
                }
            }
            OrkMode::Patrol => {
                let dist = (o.target - o.pos).length();
                if dist < 0.3 || o.timer <= 0.0 {
                    o.mode = OrkMode::Idle;
                    o.timer = rng_range(&mut o.rng, 1.5, 4.0);
                    o.moving = false;
                } else {
                    let cur_y = steer::footing(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
                    match steer::advance(o.pos, o.facing, o.target, o.speed * dt, o.body_r, cur_y, ORK_MAX_TURN * dt) {
                        Some(s) => {
                            o.facing = s.facing;
                            o.pos = s.pos;
                            o.moving = s.moving;
                        }
                        None => {
                            o.mode = OrkMode::Idle;
                            o.timer = rng_range(&mut o.rng, 0.4, 1.0);
                            o.moving = false;
                        }
                    }
                }
            }
            OrkMode::Hunt => {
                // Charge faster than a patrol, steering around props/cliffs. When chasing the
                // HERO, follow an A* route (around walls); a rival brawl stays direct (close
                // range, same clearing) so it doesn't thrash the pathfinder.
                let cur_y = steer::footing(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
                let speed = o.speed * 1.4 * if frenzied { 1.4 } else { 1.0 } * slow_mul;
                let step_target = if o.brawl_target.is_none() {
                    let now = time.elapsed_secs();
                    if o.hunt_cursor >= o.hunt_path.len()
                        || now >= o.hunt_replan_at
                        || o.hunt_goal.distance(o.target) > 2.0
                    {
                        o.hunt_path = crate::navgrid::path_to(o.pos, o.target);
                        o.hunt_cursor = 0;
                        o.hunt_goal = o.target;
                        // Stagger replans across a warband (entity-bit offset) to avoid frame spikes.
                        o.hunt_replan_at = now + 0.6 + (self_e.to_bits() % 16) as f32 * 0.03;
                    }
                    while o.hunt_cursor < o.hunt_path.len()
                        && o.pos.distance(o.hunt_path[o.hunt_cursor]) < 1.2
                    {
                        o.hunt_cursor += 1;
                    }
                    o.hunt_path.get(o.hunt_cursor).copied().unwrap_or(o.target)
                } else {
                    o.target
                };
                match steer::advance(o.pos, o.facing, step_target, speed * dt, o.body_r, cur_y, ORK_MAX_TURN * 1.6 * dt) {
                    Some(s) => {
                        o.facing = s.facing;
                        o.pos = s.pos;
                        o.moving = s.moving;
                    }
                    None => o.moving = false,
                }
            }
            OrkMode::Attack => {
                // Stand, turn to face the hero, club him on each cooldown.
                o.moving = false;
                let to = o.target - o.pos;
                if to.length_squared() > 1e-4 {
                    let want = to.x.atan2(to.y);
                    let turn = (ORK_MAX_TURN * 2.0 * dt).abs();
                    o.facing += steer::wrap_pi(want - o.facing).clamp(-turn, turn);
                }
                // Strike the HERO only (a rival brawl is resolved by `ork_brawl`).
                if o.brawl_target.is_none() && o.atk_cd <= 0.0 {
                    // Frontier-graded blow: a deep-biome warband hits ~1.6× as hard as one near
                    // the castle (pairs with its distance-scaled HP from `ensure_combat_health`).
                    let (_, dmg_mul) = crate::verbs::frontier_threat(o.pos.x, o.pos.y);
                    if o.shaman {
                        o.atk_cd = SHAMAN_CAST_CD;
                        let gy = steer::footing(o.pos.x, o.pos.y).unwrap_or(0.0);
                        bolts.0.push(crate::projectile::BoltSpawn {
                            origin: Vec3::new(o.pos.x, gy + 1.4, o.pos.y),
                            damage: SHAMAN_BOLT_DAMAGE * dmg_mul,
                        });
                    } else {
                        o.atk_cd = ORK_ATTACK_CD * if frenzied { 0.6 } else { 1.0 };
                        pending.0 += variant_melee(o.variant) * dmg_mul;
                    }
                    // Trigger the strike animation (club chop / staff jab) — `ork_limbs` reads this.
                    o.atk_anim = time.elapsed_secs();
                }
            }
        }

        apply_knockback(&mut o, dt);

        let gy = steer::footing(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
        let bob = if o.moving { (tw * o.gait + o.phase).sin().abs() * o.bob } else { 0.0 };
        // A melee ork steps into its club-chop (visual only — `pos` is untouched). The shaman
        // casts at range, so it doesn't lunge.
        let lunge = if o.shaman {
            0.0
        } else {
            strike_p(o.atk_anim, now).map_or(0.0, |p| (p * std::f32::consts::PI).sin() * 0.35)
        };
        let fwd = Vec2::new(o.facing.sin(), o.facing.cos());
        let mut lp = o.pos + fwd * lunge;
        // Don't let the lunge slide the body into the knight (the shove only pushes him off `pos`).
        // Shield-aware: a guarded, head-on hero shoves the lunge further out (see `hero_guard_radius`).
        if hero.alive {
            let keep = o.body_r + hero_guard_radius(hero.pos, hero.facing, hero.blocking, o.pos);
            lp = lunge_clear_of_hero(lp, hero.pos, keep);
        }
        tf.translation = Vec3::new(lp.x, gy + bob, lp.y);
        // Springy recoil-wobble on a blow taken (composes with the facing yaw).
        tf.rotation = Quat::from_rotation_y(o.facing) * Quat::from_rotation_x(recoil_tilt(o.hit_recoil, now));
    }

    music.fighting = fighting;

    // Camp alert: a single warband roar when the hero first steps into any clearing.
    let in_clearing = hero.alive && crate::camps::in_clearing(hero.pos.x, hero.pos.y);
    if in_clearing && !*was_clearing {
        let gy = worldmap::ground_at_world(hero.pos.x, hero.pos.y).unwrap_or(0.0);
        cues.write(crate::audio::AudioCue::OrkRoar(Vec3::new(hero.pos.x, gy + 1.2, hero.pos.y)));
    }
    *was_clearing = in_clearing;
}

/// How long a strike animation plays after `atk_anim` is stamped.
const ATTACK_ANIM_DUR: f32 = 0.42;

/// How long the springy hit-recoil wobble lasts after the ork is struck.
const RECOIL_DUR: f32 = 0.34;

/// Damped springy tilt (radians) since being struck at `hit_recoil`; `0` when at rest. Used by
/// the camp brain, the invader brain (`siege.rs`) and wildlife so every creature recoils the same.
///
/// The body snaps to a full BACKWARD lean (negative pitch = away from its facing — in melee the
/// ork faces the hero, so away from the blow) the instant it's struck, then springs upright with
/// a small forward overshoot. Starting at the peak matters: hit-stop freezes virtual time for the
/// first beat of the recoil, so whatever pose this returns at `r = 0` is the one held on screen —
/// the old version phased off `now` (effectively random per hit) and often froze at ~no tilt,
/// which is why struck orks didn't visibly bend.
pub(crate) fn recoil_tilt(hit_recoil: f32, now: f32) -> f32 {
    if hit_recoil <= 0.0 {
        return 0.0;
    }
    let r = now - hit_recoil;
    if r >= RECOIL_DUR {
        return 0.0;
    }
    let k = 1.0 - r / RECOIL_DUR;
    -(r * 17.0).cos() * 0.3 * k * k
}

/// Hero's *visible* front half-width — his shield + torso reach out well past the slim navigation
/// radius (0.22, mirroring `player::movement::PLAYER_R`). Melee attackers keep their body this far
/// off his centre so a
/// pressed-in or club-lunging ork doesn't drive its wide torso through him (the bug: the keep-outs
/// used the 0.22 nav radius, so an ork mesh buried itself in the hero). Their club still reaches —
/// only the body is held back. Tunable: bigger = more standoff, smaller = tighter.
pub(crate) const HERO_GUARD_R: f32 = 0.5;

/// Extra forward reach the RAISED shield adds to the guard when an attacker presses head-on: the
/// braced shield juts out front, so a blocked ork/animal is shoved off the *shield's* face, not the
/// torso. Tapers with the cosine of the attacker's bearing → full at dead-ahead, 0 at the flank/rear
/// (the shield only covers the front), so a creature behind a blocking hero isn't pushed away.
pub(crate) const SHIELD_REACH: f32 = 0.4;

/// The hero's directional keep-out radius against an attacker at `attacker` — how far that
/// creature's *centre* is held off the hero's centre is `creature_body_r + this`. It's the bare
/// [`HERO_GUARD_R`] normally, extended by up to [`SHIELD_REACH`] when the shield is raised and the
/// attacker is in front (so a guarded blow lands on a shield held at arm's length, never inside the
/// knight). Shared by the body-shove (`player::movement`) and the strike-lunge clamp so the render
/// and the collision hold the exact same line.
pub(crate) fn hero_guard_radius(hero_pos: Vec2, hero_facing: f32, blocking: bool, attacker: Vec2) -> f32 {
    if !blocking {
        return HERO_GUARD_R;
    }
    let to = attacker - hero_pos;
    let d = to.length();
    if d < 1e-4 {
        return HERO_GUARD_R;
    }
    // cos(bearing): +1 dead ahead of the hero's facing, 0 abeam, <0 behind.
    let fwd = Vec2::new(hero_facing.sin(), hero_facing.cos());
    let aim = (to.x * fwd.x + to.y * fwd.y) / d;
    HERO_GUARD_R + SHIELD_REACH * aim.max(0.0)
}

/// Keep an attacker's forward strike *lunge* — a visual-only mesh slide over the strike beat —
/// from clipping into the knight. The hero shove (`player::movement::shove_out_of`) pushes him
/// off the body's locomotion `pos`, NOT this lunged draw position, so an un-clamped lunge slides
/// the rendered body straight through him (worst right at melee range, exactly when he's fighting
/// it). Project the lunged point `lp` back out of the hero's keep-out cylinder of radius `keep`
/// (= `body_r + hero_guard_radius`, so the body just touches the shield/torso line). Purely
/// cosmetic — `pos`/gameplay are
/// untouched, and the radial projection works whatever the attacker faces (hero or a rival brawl).
/// Shared by the ork + wildlife strike renders.
pub(crate) fn lunge_clear_of_hero(lp: Vec2, hero_pos: Vec2, keep: f32) -> Vec2 {
    let to_h = lp - hero_pos;
    let dh = to_h.length();
    if dh > 1e-4 && dh < keep {
        hero_pos + to_h / dh * keep
    } else {
        lp
    }
}

/// Strike progress `0..1` since `atk_anim`, or `None` when not currently striking.
fn strike_p(atk_anim: f32, now: f32) -> Option<f32> {
    if atk_anim <= 0.0 {
        return None;
    }
    let p = (now - atk_anim) / ATTACK_ANIM_DUR;
    (0.0..1.0).contains(&p).then_some(p)
}

/// Overhead club chop on X: raise back (ease-in), fast chop forward (ease-out), recover to rest.
fn club_chop_x(p: f32) -> f32 {
    if p < 0.3 {
        let u = p / 0.3;
        -1.5 * (u * u)
    } else if p < 0.55 {
        let u = (p - 0.3) / 0.25;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        -1.5 + 2.4 * e
    } else {
        let u = (p - 0.55) / 0.45;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        0.9 * (1.0 - e)
    }
}

/// Shaman staff jab — raise high, jab down, settle back to the attack rest (−1.3).
fn shaman_cast_x(p: f32) -> f32 {
    if p < 0.4 {
        let u = p / 0.4;
        -1.3 - 0.6 * (u * u)
    } else {
        let u = (p - 0.4) / 0.6;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        -1.9 + 0.6 * e
    }
}

/// Squared camera distance past which limb animation is skipped (fog/DoF hide the joints).
const LIMB_CULL2: f32 = 70.0 * 70.0;

fn ork_limbs(
    time: Res<Time>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    hero: Res<crate::player::HeroState>,
    // Only the box-rig orks (fortress decor + the Warlord) still carry `OrkPart` and need posing
    // here; camp/patrol/wave orks are on the studio biped (`BipedDrive` → `ork_drive`/`animate_biped`),
    // so skip them or this scans the whole horde every frame for child lookups that always miss.
    orks: Query<(&Ork, &Children, &GlobalTransform), Without<crate::biped::BipedDrive>>,
    mut parts: Query<(&OrkPart, &mut Transform)>,
) {
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let cam_p = cam.single().ok().map(|g| g.translation());
    for (o, children, gt) in &orks {
        if let Some(cp) = cam_p {
            if gt.translation().distance_squared(cp) > LIMB_CULL2 {
                continue;
            }
        }
        let t = tw + o.phase;
        let strike = strike_p(o.atk_anim, now);
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                // Legs stride; arms swing opposite the legs (and opposite each other via sign).
                PartKind::Leg(sign) => {
                    // Charging orks stride wider (run gait) than a patrol walk.
                    let amp = crate::creature_anim::gait_amp(matches!(o.mode, OrkMode::Hunt), 1.35);
                    let s = if o.moving { (t * o.gait).sin() * o.swing * amp } else { (t * 0.8).sin() * 0.03 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Arm(sign) => {
                    // Rest pose = walk swing / idle sway.
                    let amp = crate::creature_anim::gait_amp(matches!(o.mode, OrkMode::Hunt), 1.3);
                    let s = if o.moving { -(t * o.gait).sin() * 0.42 * amp } else { (t * 0.8).sin() * 0.05 };
                    let arm_gait = Quat::from_rotation_x(sign * s);
                    // The right arm (sign > 0) carries the club / staff → it does the striking.
                    if sign > 0.0 && o.shaman {
                        match strike {
                            Some(p) => Quat::from_rotation_x(shaman_cast_x(p)),
                            None if matches!(o.mode, OrkMode::Attack) => Quat::from_rotation_x(-1.3),
                            None => arm_gait,
                        }
                    } else if sign > 0.0 {
                        match strike {
                            Some(p) => Quat::from_rotation_x(club_chop_x(p)),
                            None => arm_gait,
                        }
                    } else {
                        arm_gait
                    }
                }
                PartKind::Head => {
                    // Idle orks glance toward the hero (menacing awareness); else a slow scan.
                    // Alert only when not mid-Attack; the shared helper handles range + scan + bob.
                    let target = (hero.alive && !matches!(o.mode, OrkMode::Attack)).then_some(hero.pos);
                    crate::creature_anim::idle_head_glance(
                        o.pos,
                        o.facing,
                        t,
                        o.moving,
                        target,
                        crate::creature_anim::GlanceCfg { range: 14.0, max_yaw: 0.5, scan_amp: 0.25, bob_amp: 0.06 },
                    )
                }
                PartKind::Tail => Quat::IDENTITY, // orks have no tail
            };
        }
    }
}

// ── Biped re-rig drive (camps / patrols / waves use the shared studio rig) ────────────
/// The studio biped rig is taller than the old hand-built ork — shrink the root to keep the same
/// in-world height. Tune against a full-game ork shot.
const BIPED_SIZE_FIX: f32 = 0.85;
/// Drop the biped feet onto the root's ground plane.
const ORK_RIG_OFF: f32 = -0.05;
/// Glowing-eye offsets in the **head joint's** local frame (studio orc eye spots).
const ORK_BIPED_EYE_OFFS: [Vec3; 2] = [Vec3::new(-0.07, 0.1, 0.14), Vec3::new(0.07, 0.1, 0.14)];
/// Shield mount on the left hand (the biped animator rewrites its pose each frame; this is the rest).
fn ork_shield_xf() -> Transform {
    Transform { translation: Vec3::new(0.0, 0.0, 0.14), rotation: xyz(0.15, -0.45, 0.1), scale: Vec3::ONE }
}

/// Drive each (biped) ork's [`crate::biped::BipedDrive`] from its `Ork` AI, so `animate_biped` plays
/// the studio clips. Mirrors the old `ork_limbs` gait/strike timing (gait `(t+phase)*gait`, the
/// `strike_p` window → an overhead chop). Camp/patrol/wave orks use this; fortress decorative orks
/// and the Warlord (no `BipedDrive`) still use `ork_limbs`/`OrkPart`.
fn ork_drive(time: Res<Time>, mut q: Query<(&Ork, &mut crate::biped::BipedDrive), Without<crate::dying::Dying>>) {
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (o, mut d) in &mut q {
        // A roosting guard sits while idle; the brain clears `seated_rest` the moment it engages,
        // so this stands it up to fight (and, once woken, it never re-roosts).
        d.sitting = o.seated_rest && matches!(o.mode, OrkMode::Idle);
        let target = if o.moving { 1.0 } else { 0.0 };
        d.moving_amt += (target - d.moving_amt) * (dt * 8.0).min(1.0);
        d.run_amt = if matches!(o.mode, OrkMode::Hunt) { 0.55 } else { 0.0 }; // charging = run-ish
        d.walk_phase = (tw + o.phase) * o.gait;
        match strike_p(o.atk_anim, now) {
            Some(p) => {
                d.attacking = true;
                d.attack_t = p * crate::player::ATTACK_DURATION;
                d.attack_variant = 0; // overhead chop (matches the old club_chop)
            }
            None => {
                d.attacking = false;
                d.attack_t = 0.0;
            }
        }
    }
}

/// Each shaman, on its heal cooldown, restores HP to the nearest wounded **same-faction ork**
/// within range and sparkles it green. Faction-scoped (only orks, only the shaman's warband) so
/// it never heals wildlife or the enemy camp.
fn shaman_heal(
    time: Res<Time>,
    fx: Option<Res<crate::player::CombatFx>>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Ork, &GlobalTransform, &mut crate::player::Health), Without<crate::dying::Dying>>,
) {
    let dt = time.delta_secs().min(0.05);
    // Snapshot every ork's (entity, faction, xz, hp, max) so we can target then mutate.
    let allies: Vec<(Entity, Faction, Vec2, f32, f32)> = q
        .iter()
        .map(|(e, o, gt, h)| {
            (e, o.faction, Vec2::new(gt.translation().x, gt.translation().z), h.hp, h.max)
        })
        .collect();

    // Pass 1: tick each shaman's cooldown + pick a same-faction wounded ally (mutates only the
    // shaman's own Ork). Defer the target heal to pass 2 to avoid overlapping mutable borrows.
    let mut heals: Vec<(Entity, Vec3)> = Vec::new();
    for (self_e, mut o, _gt, _h) in &mut q {
        if !o.shaman {
            continue;
        }
        o.heal_cd -= dt;
        if o.heal_cd > 0.0 {
            continue;
        }
        let mut best: Option<(Entity, Vec2)> = None;
        let mut best_d = SHAMAN_HEAL_RANGE;
        for (e, fac, p, hp, max) in &allies {
            if *e == self_e || *fac != o.faction || *hp >= *max - 0.5 {
                continue;
            }
            let d = o.pos.distance(*p);
            if d < best_d {
                best_d = d;
                best = Some((*e, *p));
            }
        }
        if let Some((e, p)) = best {
            let gy = worldmap::ground_at_world(p.x, p.y).unwrap_or(0.0);
            heals.push((e, Vec3::new(p.x, gy + 1.2, p.y)));
            o.heal_cd = SHAMAN_HEAL_CD;
        } else {
            o.heal_cd = 1.0; // nothing to heal — re-check soon
        }
    }

    // Pass 2: apply each heal to the target's Health + sparkle.
    for (target, at) in heals {
        if let Ok((_, _, _, mut h)) = q.get_mut(target) {
            h.hp = (h.hp + SHAMAN_HEAL_AMOUNT).min(h.max);
        }
        if let Some(fx) = &fx {
            crate::player::spawn_heal_burst(&mut commands, fx, at);
        }
    }
}

/// Rival warbands trade blows: any ork with a `brawl_target` in melee range chips its rival's
/// HP on the brawl cooldown; a felled rival is reaped (`try_despawn`). Camp-only (invaders march
/// the keep, never brawl). The combat HP-bar + hurt-flash already read the shared `Health`.
#[allow(clippy::type_complexity)]
fn ork_brawl(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<
        (Entity, &mut Ork, &mut crate::player::Health),
        (Without<WaveInvader>, Without<crate::dying::Dying>),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let snap: Vec<(Entity, Vec2)> = q.iter().map(|(e, o, _)| (e, o.pos)).collect();
    let mut hits: Vec<(Entity, f32)> = Vec::new();
    for (_e, mut o, _h) in &mut q {
        o.brawl_cd -= dt;
        let Some(rt) = o.brawl_target else { continue };
        if let Some((_, rp)) = snap.iter().find(|(re, _)| *re == rt) {
            if o.pos.distance(*rp) < BRAWL_RANGE && o.brawl_cd <= 0.0 {
                o.brawl_cd = BRAWL_CD;
                hits.push((rt, BRAWL_DMG));
            }
        }
    }
    for (e, dmg) in hits {
        if let Ok((_, _, mut h)) = q.get_mut(e) {
            if h.hp > 0.0 {
                h.hp -= dmg;
                if h.hp <= 0.0 {
                    crate::dying::begin_dying(&mut commands, e, time.elapsed_secs());
                }
            }
        }
    }
}

fn pick_patrol(o: &mut Ork) {
    let ang = rng01(&mut o.rng) * TAU;
    let r = rng_range(&mut o.rng, o.wander_r * 0.3, o.wander_r);
    // Wander around the ork's OWN spot (not the shared camp centre), so the warband mills as
    // four loose orbits instead of one converging knot.
    o.target = o.anchor + Vec2::new(ang.cos() * r, ang.sin() * r);
    o.mode = OrkMode::Patrol;
    o.timer = rng_range(&mut o.rng, 3.0, 7.0);
}

// ── Models (ported from Ork.tsx) ────────────────────────────────────────────────────

struct PartDef {
    kind: PartKind,
    pivot: Vec3,
    mesh: Mesh,
}
struct OrkSpec {
    torso: Mesh,
    parts: Vec<PartDef>,
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("ork parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
/// Bevelled box matching the hero's softened low-poly edges (`player::model::chamfer_box`), so the
/// ork's box details (tusked jaw, plates, studs) read as the same family as the knight, not sharp cuboids.
fn chamfer_box(w: f32, h: f32, d: f32, e: f32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (a, b, c) = (w * 0.5, h * 0.5, d * 0.5);
    let e = e.min(a * 0.49).min(b * 0.49).min(c * 0.49).max(0.001);
    let (ai, bi, ci) = (a - e, b - e, c - e);
    let pos: Vec<[f32; 3]> = vec![
        [a, -bi, -ci], [a, bi, -ci], [a, bi, ci], [a, -bi, ci],
        [-a, -bi, -ci], [-a, bi, -ci], [-a, bi, ci], [-a, -bi, ci],
        [-ai, b, -ci], [ai, b, -ci], [ai, b, ci], [-ai, b, ci],
        [-ai, -b, -ci], [ai, -b, -ci], [ai, -b, ci], [-ai, -b, ci],
        [-ai, -bi, c], [ai, -bi, c], [ai, bi, c], [-ai, bi, c],
        [-ai, -bi, -c], [ai, -bi, -c], [ai, bi, -c], [-ai, bi, -c],
    ];
    let mut raw: Vec<[u32; 3]> = Vec::new();
    let mut quad = |a: u32, b: u32, c: u32, d: u32| {
        raw.push([a, b, c]);
        raw.push([a, c, d]);
    };
    for f in 0..6u32 {
        let o = f * 4;
        quad(o, o + 1, o + 2, o + 3);
    }
    let edges = [
        [1, 2, 10, 9], [3, 0, 13, 14], [6, 5, 8, 11], [7, 4, 12, 15],
        [3, 2, 18, 17], [0, 1, 22, 21], [7, 6, 19, 16], [4, 5, 23, 20],
        [11, 10, 18, 19], [8, 9, 22, 23], [15, 14, 17, 16], [12, 13, 21, 20],
    ];
    for q in edges {
        quad(q[0], q[1], q[2], q[3]);
    }
    for t in [[2, 10, 18], [1, 9, 22], [3, 14, 17], [0, 13, 21], [6, 11, 19], [5, 8, 23], [7, 15, 16], [4, 12, 20]] {
        raw.push(t);
    }
    let g = |i: u32| Vec3::from_array(pos[i as usize]);
    let mut idx: Vec<u32> = Vec::new();
    for t in raw {
        let (va, vb, vc) = (g(t[0]), g(t[1]), g(t[2]));
        let nrm = (vb - va).cross(vc - va);
        let ctr = (va + vb + vc) / 3.0;
        if nrm.dot(ctr) >= 0.0 {
            idx.extend(t);
        } else {
            idx.extend([t[0], t[2], t[1]]);
        }
    }
    let n = pos.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_indices(Indices::U32(idx));
    m
}
fn cham(w: f32, h: f32, d: f32) -> f32 {
    (w.min(h).min(d) * 0.26).clamp(0.01, 0.05)
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(chamfer_box(w, h, d, cham(w, h, d)).translated_by(off), c)
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(chamfer_box(w, h, d, cham(w, h, d)).rotated_by(rot).translated_by(off), c)
}
fn cone(r: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), c)
}
fn cyl(r: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(7).build().rotated_by(rot).translated_by(off), c)
}
fn orb(r: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(Sphere::new(r).mesh().ico(2).unwrap().translated_by(off), c)
}
/// Bake a part built in a sub-group's local space into the parent: rotate about the group
/// origin, then translate to the group's offset (matches three.js `<group rotation pos>`).
fn baked(m: Mesh, rot: Quat, off: Vec3) -> Mesh {
    m.rotated_by(rot).translated_by(off)
}

/// Tapered cylinder (three.js `CylinderGeometry(rt,rb,h)`) — for the studio-rig ork limbs/torso.
fn frustum(rt: f32, rb: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(ConicalFrustum { radius_top: rt, radius_bottom: rb, height: h }.mesh().resolution(6).build().rotated_by(rot).translated_by(off), c)
}
/// scale → rotate → translate → tint (three.js `T*R*S`), for parts that need a non-uniform scale.
fn part(mut m: Mesh, scale: Vec3, rot: Quat, off: Vec3, c: [f32; 4]) -> Mesh {
    if scale != Vec3::ONE {
        m = m.scaled_by(scale);
    }
    tinted(m.rotated_by(rot).translated_by(off), c)
}

/// Build the ork's per-joint meshes for the **shared studio biped skeleton** (`crate::biped`). A
/// faithful port of the studio `orcBuilder` geometry (clean green orc — skull/brow/jaw/tusks/ears,
/// leather pads + skin limbs, wooden shield, spiked club), authored in each joint's local frame.
/// Glowing eyes are NOT included (spawned separately as emissive entities so they bloom at night).
/// Faction tints the loincloth + mohawk. `variant` headgear + the shaman staff are layered in a
/// later pass; this base is what `FOREST_VIEW=ork` renders for verification.
pub(crate) fn ork_biped_meshes(variant: OrkVariant, faction: Faction) -> crate::biped::BipedMeshes {
    use crate::biped::BipedMeshes;
    const HPI: f32 = std::f32::consts::FRAC_PI_2;
    const PI: f32 = std::f32::consts::PI;
    let st = stats(variant);
    let skin = lin(st.skin); // per-variant skin: grunt green / scout lime / berserker red-brown / shaman purple
    let dark = lin_scaled(st.skin, 0.62); // darker skin accent (war-paint base / fur trim)
    let belly = lin_scaled(st.skin, 1.18); // lighter underbelly plate
    let leather = lin(0x5c3d24);
    let bone = lin(0xddd8c8);
    let wood = lin(0x5a3a1c);
    let iron = lin(0x8a8d92);
    let cloth = lin(faction.hex()); // faction loincloth + war-paint (Red vs Blue clans)
    let club_wood = lin(0x4a2a16);

    let hips = group(vec![
        frustum(0.25, 0.21, 0.12, v(0.0, -0.06, 0.0), Quat::IDENTITY, cloth), // pelvis
        frustum(0.29, 0.27, 0.07, v(0.0, 0.03, 0.01), Quat::IDENTITY, leather), // belt
        bxr(0.22, 0.22, 0.05, v(0.0, -0.1, 0.14), rx(0.15), cloth), // loin flap
    ]);
    let torso = group(vec![
        part(ConicalFrustum { radius_top: 0.30, radius_bottom: 0.27, height: 0.44 }.mesh().resolution(6).build(), v(1.15, 1.0, 1.0), rx(0.08), v(0.0, 0.14, 0.02), skin), // chest (broad but not a slab — arms attach at the edge)
        bxr(0.4, 0.06, 0.06, v(0.0, 0.22, 0.15), rx(0.1), leather), // chest strap
        bxr(0.34, 0.32, 0.06, v(0.0, 0.12, -0.15), rx(-0.08), leather), // back panel
        // ── restored variety: lighter underbelly + faction war-paint + a bone-tooth trophy necklace
        // (the per-variant `skin` tints the body, so each variant/faction reads distinct again) ──
        bxr(0.30, 0.15, 0.02, v(0.0, 0.02, 0.205), rx(0.08), belly), // lighter underbelly plate
        bxr(0.085, 0.30, 0.022, v(0.0, 0.15, 0.215), rx(0.08), cloth), // faction war-paint vertical stripe
        bxr(0.34, 0.045, 0.018, v(0.0, 0.275, 0.205), rx(0.08), dark), // dark war-paint slash
        bxr(0.30, 0.02, 0.012, v(0.0, 0.315, 0.205), rx(0.08), bone), // trophy necklace cord
        cone(0.016, 0.07, v(-0.09, 0.255, 0.215), rx(PI), bone), // hanging tooth L
        cone(0.018, 0.08, v(0.0, 0.255, 0.222), rx(PI), bone), // hanging tooth C
        cone(0.016, 0.07, v(0.09, 0.255, 0.215), rx(PI), bone), // hanging tooth R
    ]);
    let neck = group(vec![frustum(0.14, 0.16, 0.08, v(0.0, -0.01, 0.0), Quat::IDENTITY, leather)]);
    let mut head_parts = vec![
        bx(0.24, 0.26, 0.24, v(0.0, 0.05, 0.02), skin), // skull
        bxr(0.26, 0.06, 0.1, v(0.0, 0.16, 0.1), rx(-0.2), skin), // brow
        bxr(0.22, 0.1, 0.14, v(0.0, -0.02, 0.12), rx(0.15), skin), // jaw
        cone(0.028, 0.11, v(-0.07, -0.01, 0.18), xyz(-0.25, 0.0, -0.08), bone), // tusk L
        cone(0.028, 0.11, v(0.07, -0.01, 0.18), xyz(-0.25, 0.0, 0.08), bone), // tusk R
        cone(0.04, 0.12, v(-0.14, 0.06, -0.04), rz(-1.0), skin), // ear L
        cone(0.04, 0.12, v(0.14, 0.06, -0.04), rz(1.0), skin), // ear R
    ];
    // Per-variant headgear (re-fitted to the studio head): grunt topknot · scout faction headband +
    // bone feather · berserker faction mohawk + cheek war-paint · shaman bone headdress + horns.
    let hair = lin(0x1a1008);
    match variant {
        OrkVariant::Grunt => {
            head_parts.push(cyl(0.04, 0.09, v(0.0, 0.2, -0.02), Quat::IDENTITY, hair)); // topknot
            head_parts.push(orb(0.05, v(0.0, 0.26, -0.02), hair));
        }
        OrkVariant::Scout => {
            head_parts.push(bx(0.27, 0.05, 0.27, v(0.0, 0.12, 0.0), cloth)); // headband
            head_parts.push(cone(0.02, 0.14, v(0.13, 0.2, -0.04), rz(-0.3), bone)); // feather
        }
        OrkVariant::Berserker => {
            for i in 0..4 {
                head_parts.push(cone(0.03, 0.13, v(0.0, 0.19, 0.08 - i as f32 * 0.06), rx(-0.15), cloth)); // mohawk
            }
            head_parts.push(bx(0.035, 0.11, 0.012, v(-0.1, -0.02, 0.135), cloth)); // cheek paint L
            head_parts.push(bx(0.035, 0.11, 0.012, v(0.1, -0.02, 0.135), cloth)); // cheek paint R
        }
        OrkVariant::Shaman => {
            head_parts.push(bx(0.24, 0.09, 0.24, v(0.0, 0.17, 0.0), bone)); // half-skull headdress
            head_parts.push(cone(0.03, 0.15, v(-0.1, 0.23, 0.0), rz(0.5), bone)); // horn L
            head_parts.push(cone(0.03, 0.15, v(0.1, 0.23, 0.0), rz(-0.5), bone)); // horn R
        }
    }
    let head = group(head_parts);
    let shoulder = |sign: f32| {
        group(vec![
            part(Sphere::new(0.16).mesh().ico(1).unwrap(), v(1.2, 0.8, 1.1), rz(sign * 0.25), v(sign * 0.02, 0.04, 0.0), leather), // pad
            part(Sphere::new(0.15).mesh().ico(1).unwrap(), v(1.05, 0.45, 1.05), Quat::IDENTITY, v(0.0, 0.07, 0.0), dark), // fur trim collar
            frustum(0.17, 0.15, 0.28, v(0.0, -0.14, 0.0), Quat::IDENTITY, skin), // bicep (thick)
        ])
    };
    let elbow = || {
        group(vec![
            frustum(0.15, 0.16, 0.27, v(0.0, -0.14, 0.0), Quat::IDENTITY, skin), // forearm (thick)
            frustum(0.165, 0.17, 0.16, v(0.0, -0.16, 0.0), Quat::IDENTITY, leather), // bracer
            bx(0.16, 0.15, 0.16, v(0.0, -0.22, 0.0), skin), // fist
        ])
    };
    let hip = || group(vec![frustum(0.20, 0.17, 0.38, v(0.0, -0.2, 0.0), Quat::IDENTITY, cloth)]); // thigh (thick)
    let knee = || {
        group(vec![
            frustum(0.16, 0.17, 0.36, v(0.0, -0.2, 0.0), Quat::IDENTITY, skin), // shin (thick)
            frustum(0.17, 0.175, 0.18, v(0.0, -0.14, 0.0), Quat::IDENTITY, leather), // shin wrap
        ])
    };
    let foot = || group(vec![bx(0.20, 0.12, 0.27, v(0.0, -0.04, 0.05), leather)]); // boot (broad)

    // Weapon (+Y sword-local; the `Sword` pivot's hero rest rotation tilts it to ready): a spiked
    // club for fighters, or a bone-clawed orb staff for the shaman.
    let weapon = if matches!(variant, OrkVariant::Shaman) {
        let orb_c = lin(0xc89cff);
        group(vec![
            cyl(0.033, 1.0, v(0.0, 0.1, 0.0), Quat::IDENTITY, club_wood), // staff
            cyl(0.04, 0.06, v(0.0, -0.15, 0.0), Quat::IDENTITY, leather), // grip wrap
            orb(0.09, v(0.0, 0.62, 0.0), orb_c), // orb
            cone(0.02, 0.12, v(-0.055, 0.55, 0.0), rz(0.35), bone), // claw L
            cone(0.02, 0.12, v(0.055, 0.55, 0.0), rz(-0.35), bone), // claw R
        ])
    } else {
        let mut club_parts = vec![
            frustum(0.032, 0.036, 0.2, v(0.0, -0.02, 0.0), Quat::IDENTITY, leather), // handle
            frustum(0.09, 0.07, 0.4, v(0.0, 0.28, 0.0), Quat::IDENTITY, club_wood), // head
        ];
        for i in 0..4 {
            let a = i as f32 * HPI;
            club_parts.push(cone(0.03, 0.1, v(a.cos() * 0.1, 0.3, a.sin() * 0.1), xyz(0.0, a, -HPI), iron)); // spike
        }
        group(club_parts)
    };

    // Round wooden shield: disc facing +Z + two iron studs (mounted on the left hand by spawn_biped).
    let shield = group(vec![
        cyl(0.2, 0.04, v(0.0, 0.0, 0.0), rx(HPI), wood),
        cyl(0.022, 0.018, v(0.0, 0.06, 0.022), rx(HPI), iron),
        cyl(0.022, 0.018, v(0.0, -0.07, 0.022), rx(HPI), iron),
    ]);

    BipedMeshes {
        hips,
        torso,
        neck,
        head,
        shoulder_l: shoulder(-1.0),
        shoulder_r: shoulder(1.0),
        elbow_l: elbow(),
        elbow_r: elbow(),
        hip_l: hip(),
        hip_r: hip(),
        knee_l: knee(),
        knee_r: knee(),
        foot_l: foot(),
        foot_r: foot(),
        weapon: Some(weapon),
        shield: Some(shield),
        lion: None,
    }
}

fn spec(variant: OrkVariant, faction: Faction) -> OrkSpec {
    let st = stats(variant);
    let skin = lin(st.skin);
    let dark = lin_scaled(st.skin, 0.62); // SKIN_DARK_ACCENT
    let belly = lin_scaled(st.skin, 1.18); // lighter underbelly plate
    let fac = lin(faction.hex());
    const BELT: u32 = 0x3a2616;
    const TUSK: u32 = 0xece1c2;
    const EYE: u32 = 0xe6c828;
    const CLUB_WOOD: u32 = 0x4a2a16;
    const CLUB_BAND: u32 = 0x1a1008;
    const STAFF_WOOD: u32 = 0x6a4a2a;
    const ORB: u32 = 0xc89cff;
    const BONE: u32 = 0xd8cfae;
    const WRAP: u32 = 0x2e1f12; // leather wrist/ankle wraps
    const RING: u32 = 0xc9a24a; // crude gold earring
    let hair = lin(CLUB_BAND);
    let body_rot = Quat::from_rotation_x(0.2);
    let body_off = v(0.0, 0.74, 0.05);
    const PI: f32 = std::f32::consts::PI;
    const HPI: f32 = std::f32::consts::FRAC_PI_2;

    // Static torso: loincloth (faction) + belt + the pitched body group (torso + underbelly +
    // war-paint + a leather shoulder strap + a trophy tooth-necklace) — the TS silhouette with
    // Bevy-side savage dressing on top.
    let mut torso_parts = vec![
        bx(0.55, 0.2, 0.3, v(0.0, 0.4, 0.0), fac), // loincloth
        bx(0.15, 0.08, 0.29, v(-0.14, 0.27, 0.0), fac), // ragged hem tatters
        bx(0.12, 0.06, 0.29, v(0.05, 0.28, 0.0), fac),
        bx(0.10, 0.07, 0.29, v(0.19, 0.275, 0.0), fac),
        bx(0.56, 0.06, 0.31, v(0.0, 0.49, 0.0), lin(BELT)),
        bx(0.07, 0.06, 0.02, v(0.0, 0.49, 0.155), lin(BONE)), // belt skull trophy
        baked(bx(0.55, 0.42, 0.34, Vec3::ZERO, skin), body_rot, body_off), // torso
        baked(bx(0.40, 0.28, 0.014, v(0.0, -0.06, 0.168), belly), body_rot, body_off), // underbelly
        baked(bx(0.12, 0.32, 0.022, v(0.0, 0.0, 0.175), fac), body_rot, body_off), // war-paint vert
        baked(bx(0.4, 0.06, 0.016, v(0.0, 0.0, 0.176), dark), body_rot, body_off), // war-paint horiz
        baked(bxr(0.07, 0.50, 0.018, v(0.0, 0.0, 0.18), rz(0.65), lin(BELT)), body_rot, body_off), // shoulder strap
        baked(bx(0.34, 0.018, 0.012, v(0.0, 0.185, 0.183), lin(BELT)), body_rot, body_off), // necklace cord
    ];
    for i in -2i32..=2 {
        // trophy teeth strung on the cord, drooping toward the centre
        let x = i as f32 * 0.07;
        let y = 0.15 - (2 - i.abs()) as f32 * 0.012;
        torso_parts.push(baked(cone(0.018, 0.07, v(x, y, 0.185), rx(PI), lin(TUSK)), body_rot, body_off));
    }
    let torso = group(torso_parts);

    // Head: skull + jutting underbite jaw (the tusks rise from it) + brow + eyes + nostrils +
    // ears (one with a crude gold ring), topped by per-variant headgear: grunt topknot, scout
    // faction headband + bone feather, berserker faction-dyed mohawk + cheek war-paint, shaman
    // bone half-skull headdress with horns.
    let mut head_parts = vec![
        bx(0.36, 0.34, 0.34, Vec3::ZERO, skin),
        bx(0.30, 0.11, 0.30, v(0.0, -0.16, 0.05), skin), // underbite jaw
        bx(0.26, 0.04, 0.02, v(0.0, -0.125, 0.195), dark), // mouth shadow above the jaw lip
        bx(0.32, 0.06, 0.01, v(0.0, 0.06, 0.175), dark),
        bx(0.05, 0.04, 0.008, v(-0.08, 0.02, 0.175), lin(EYE)),
        bx(0.05, 0.04, 0.008, v(0.08, 0.02, 0.175), lin(EYE)),
        bx(0.02, 0.025, 0.008, v(-0.035, -0.055, 0.176), dark), // nostrils
        bx(0.02, 0.025, 0.008, v(0.035, -0.055, 0.176), dark),
        cone(0.028, 0.15, v(-0.09, -0.10, 0.19), rz(-0.15), lin(TUSK)), // tusks (from the jaw)
        cone(0.028, 0.15, v(0.09, -0.10, 0.19), rz(0.15), lin(TUSK)),
        bx(0.06, 0.12, 0.14, v(-0.2, 0.0, 0.0), skin),
        bx(0.06, 0.12, 0.14, v(0.2, 0.0, 0.0), skin),
        bx(0.03, 0.05, 0.012, v(-0.2, -0.085, 0.0), lin(RING)), // earring, left ear
    ];
    match variant {
        OrkVariant::Grunt => {
            head_parts.push(cyl(0.045, 0.10, v(0.0, 0.21, -0.02), Quat::IDENTITY, hair)); // topknot
            head_parts.push(orb(0.06, v(0.0, 0.28, -0.02), hair));
        }
        OrkVariant::Scout => {
            head_parts.push(bx(0.37, 0.05, 0.35, v(0.0, 0.10, 0.0), fac)); // faction headband
            head_parts.push(cone(0.02, 0.16, v(0.17, 0.21, -0.05), rz(-0.3), lin(BONE))); // bone feather
        }
        OrkVariant::Berserker => {
            for i in 0..4 {
                // faction-dyed mohawk, raked back
                head_parts.push(cone(0.035, 0.15, v(0.0, 0.21, 0.10 - i as f32 * 0.08), rx(-0.15), fac));
            }
            head_parts.push(bx(0.04, 0.13, 0.012, v(-0.13, -0.03, 0.174), fac)); // cheek war-paint
            head_parts.push(bx(0.04, 0.13, 0.012, v(0.13, -0.03, 0.174), fac));
        }
        OrkVariant::Shaman => {
            head_parts.push(bx(0.30, 0.10, 0.30, v(0.0, 0.19, 0.02), lin(BONE))); // half-skull headdress
            head_parts.push(cone(0.035, 0.18, v(-0.13, 0.26, 0.0), rz(0.5), lin(TUSK))); // horns
            head_parts.push(cone(0.035, 0.18, v(0.13, 0.26, 0.0), rz(-0.5), lin(TUSK)));
        }
    }
    let head = group(head_parts);

    // Right arm + baked weapon (club for melee, staff + orb for shaman): fur-trimmed spiked
    // shoulder pad, wrist wrap, then the weapon.
    let mut arm_r = vec![
        bx(0.2, 0.1, 0.3, v(0.0, -0.02, 0.0), dark), // shoulder
        bx(0.22, 0.05, 0.32, v(0.0, 0.045, 0.0), hair), // fur trim
        bxr(0.17, 0.5, 0.24, v(0.02, -0.25, 0.04), xyz(0.2, 0.0, 0.05), skin), // upper
        bxr(0.17, 0.06, 0.24, v(0.035, -0.44, 0.065), xyz(0.2, 0.0, 0.05), lin(WRAP)), // wrist wrap
        bxr(0.16, 0.1, 0.22, v(0.04, -0.52, 0.08), xyz(0.2, 0.0, 0.05), dark), // forearm
    ];
    if !st.shaman {
        arm_r.push(cone(0.04, 0.13, v(0.0, 0.09, 0.0), xyz(0.0, 0.0, 0.25), lin(CLUB_BAND))); // shoulder spike
    }
    if st.shaman {
        let wr = xyz(0.1, 0.0, 0.08);
        let wo = v(0.05, -0.5, 0.1);
        arm_r.push(baked(cyl(0.033, 1.1, v(0.0, -0.1, 0.0), Quat::IDENTITY, lin(STAFF_WOOD)), wr, wo));
        arm_r.push(baked(cyl(0.04, 0.06, v(0.0, -0.45, 0.0), Quat::IDENTITY, lin(WRAP)), wr, wo)); // grip wrap
        arm_r.push(baked(orb(0.1, v(0.0, 0.5, 0.0), lin(ORB)), wr, wo));
        // bone claw cradling the orb + a faction tassel under it
        arm_r.push(baked(cone(0.02, 0.13, v(-0.06, 0.42, 0.0), rz(0.35), lin(TUSK)), wr, wo));
        arm_r.push(baked(cone(0.02, 0.13, v(0.06, 0.42, 0.0), rz(-0.35), lin(TUSK)), wr, wo));
        arm_r.push(baked(cone(0.02, 0.13, v(0.0, 0.42, 0.06), rx(-0.35), lin(TUSK)), wr, wo));
        arm_r.push(baked(bx(0.03, 0.10, 0.03, v(0.07, 0.32, 0.0), fac), wr, wo));
    } else {
        let wr = xyz(0.4, 0.0, 0.1);
        let wo = v(0.05, -0.65, 0.1);
        arm_r.push(baked(cyl(0.04, 0.26, v(0.0, -0.1, 0.0), Quat::IDENTITY, lin(CLUB_WOOD)), wr, wo));
        arm_r.push(baked(cyl(0.05, 0.07, v(0.0, -0.16, 0.0), Quat::IDENTITY, lin(WRAP)), wr, wo)); // grip wrap
        arm_r.push(baked(cyl(0.09, 0.34, v(0.0, -0.36, 0.0), Quat::IDENTITY, lin(CLUB_WOOD)), wr, wo));
        for i in 0..4 {
            let a = i as f32 * HPI;
            let spike = cone(0.03, 0.09, v(a.cos() * 0.1, -0.36, a.sin() * 0.1), xyz(0.0, a, HPI), lin(CLUB_BAND));
            arm_r.push(baked(spike, wr, wo));
            // second offset spike ring higher on the head
            let b = a + std::f32::consts::FRAC_PI_4;
            let spike2 = cone(0.025, 0.08, v(b.cos() * 0.1, -0.26, b.sin() * 0.1), xyz(0.0, b, HPI), lin(CLUB_BAND));
            arm_r.push(baked(spike2, wr, wo));
        }
        arm_r.push(baked(cone(0.04, 0.10, v(0.0, -0.57, 0.0), rx(PI), lin(CLUB_BAND)), wr, wo)); // crown spike
    }
    let arm_r = group(arm_r);

    // Left arm: matching fur shoulder + a spiked leather bracer on the forearm.
    let arm_l = group(vec![
        bx(0.2, 0.1, 0.3, v(0.0, -0.02, 0.0), dark),
        bx(0.22, 0.05, 0.32, v(0.0, 0.045, 0.0), hair), // fur trim
        bxr(0.17, 0.5, 0.24, v(-0.02, -0.25, 0.04), xyz(0.2, 0.0, -0.05), skin),
        bxr(0.18, 0.13, 0.26, v(-0.04, -0.52, 0.08), xyz(0.2, 0.0, -0.05), dark),
        bxr(0.19, 0.06, 0.27, v(-0.045, -0.475, 0.075), xyz(0.2, 0.0, -0.05), lin(WRAP)), // bracer strap
        cone(0.025, 0.09, v(-0.14, -0.52, 0.0), rz(HPI), lin(CLUB_BAND)), // bracer spikes
        cone(0.025, 0.09, v(-0.14, -0.52, 0.14), rz(HPI), lin(CLUB_BAND)),
    ]);

    // Legs (built top-at-origin so the hip pivot sits at the top; feet rest at root y≈0):
    // shin + leather ankle wrap + a toed foot jutting forward.
    let leg = || {
        group(vec![
            bx(0.2, 0.36, 0.22, v(0.0, -0.18, 0.0), skin),
            bx(0.21, 0.07, 0.23, v(0.0, -0.28, 0.0), lin(WRAP)), // ankle wrap
            bx(0.18, 0.08, 0.12, v(0.0, -0.32, 0.15), skin), // foot / toes
        ])
    };

    let parts = vec![
        PartDef { kind: PartKind::Leg(1.0), pivot: v(-0.13, 0.36, 0.0), mesh: leg() },
        PartDef { kind: PartKind::Leg(-1.0), pivot: v(0.13, 0.36, 0.0), mesh: leg() },
        PartDef { kind: PartKind::Arm(1.0), pivot: v(0.36, 0.95, 0.05), mesh: arm_r },
        PartDef { kind: PartKind::Arm(-1.0), pivot: v(-0.36, 0.95, 0.05), mesh: arm_l },
        PartDef { kind: PartKind::Head, pivot: v(0.0, 1.1, 0.06), mesh: head },
    ];
    OrkSpec { torso, parts }
}

// ── Armory: all variant×faction meshes uploaded once, clone-spawned per camp ─────────

struct Template {
    torso: Handle<Mesh>,
    parts: Vec<(PartKind, Vec3, Handle<Mesh>)>,
    /// The shared-biped re-rig handles (used by `spawn` — camps/patrols/waves). The old
    /// `torso`/`parts` above stay for `spawn_prop` (fortress decor + the Warlord boss).
    biped: crate::biped::BipedHandles,
    st: Stats,
}

pub struct Armory {
    mat: Handle<crate::creature::CreatureMaterial>,
    tmpl: Vec<((OrkVariant, Faction), Template)>,
    eye_mesh: Handle<Mesh>,
    eye_mat: Handle<StandardMaterial>, // glowing eyes stay a plain emissive StandardMaterial
}

impl Armory {
    /// Build all 8 variant×faction ork meshes against the shared vertex-colour `mat`, plus the
    /// shared glowing-eye mesh/material (an unlit hot-amber emissive → bloom catches it at night).
    pub fn new(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>, // still used for the glowing-eye material
        mat: Handle<crate::creature::CreatureMaterial>,
    ) -> Armory {
        let mut tmpl = Vec::new();
        for faction in [Faction::Red, Faction::Blue] {
            for variant in VARIANTS {
                let s = spec(variant, faction);
                let st = stats(variant);
                tmpl.push((
                    (variant, faction),
                    Template {
                        torso: meshes.add(s.torso),
                        parts: s.parts.into_iter().map(|p| (p.kind, p.pivot, meshes.add(p.mesh))).collect(),
                        biped: ork_biped_meshes(variant, faction).upload(meshes),
                        st,
                    },
                ));
            }
        }
        let eye_mesh = meshes.add(Sphere::new(0.05).mesh().ico(1).unwrap());
        let eye_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.78, 0.18),
            emissive: LinearRgba::rgb(6.0, 2.2, 0.3), // hot amber → glows + blooms
            unlit: true,
            ..default()
        });
        Armory { mat, tmpl, eye_mesh, eye_mat }
    }

    fn template(&self, variant: OrkVariant, faction: Faction) -> &Template {
        self.tmpl
            .iter()
            .find(|((v, f), _)| *v == variant && *f == faction)
            .map(|(_, t)| t)
            .expect("armory built for every variant×faction")
    }

    /// Spawn one ork home-anchored at `home`, standing at `pos`, with deterministic `seed`.
    pub fn spawn(
        &self,
        commands: &mut Commands,
        variant: OrkVariant,
        faction: Faction,
        home: Vec2,
        pos: Vec2,
        seed: u32,
    ) -> Entity {
        self.spawn_inner(commands, variant, faction, home, pos, seed, None, false)
    }

    /// Spawn a real fighting warband ork that starts **roosting** on a stump — seated (facing
    /// `facing`) and held in the sit pose while idle — and STANDS to fight the instant the hero
    /// strays within [`ORK_SIGHT`]. Unlike [`spawn_seated`](Self::spawn_seated) (a pure prop) it
    /// carries the [`Ork`] brain + (via `ensure_combat_health`) `Health`, so it can be hit, killed
    /// and counts toward clearing the camp. Home-anchored at `home` like the rest of the warband.
    pub fn spawn_rooster(
        &self,
        commands: &mut Commands,
        variant: OrkVariant,
        faction: Faction,
        home: Vec2,
        pos: Vec2,
        facing: f32,
        seed: u32,
    ) -> Entity {
        self.spawn_inner(commands, variant, faction, home, pos, seed, Some(facing), true)
    }

    /// Shared body for [`spawn`](Self::spawn) / [`spawn_rooster`](Self::spawn_rooster). A
    /// `facing_override` of `None` randomises the facing from the seed (preserving the old
    /// `spawn` rng sequence); `seated_rest` starts the ork roosting (sit pose) until it engages.
    #[allow(clippy::too_many_arguments)]
    fn spawn_inner(
        &self,
        commands: &mut Commands,
        variant: OrkVariant,
        faction: Faction,
        home: Vec2,
        pos: Vec2,
        seed: u32,
        facing_override: Option<f32>,
        seated_rest: bool,
    ) -> Entity {
        let t = self.template(variant, faction);
        let st = t.st;
        let mut rng = seed | 1;
        let phase = rng01(&mut rng) * TAU;
        // Draw the random facing unconditionally so `spawn` (override `None`) keeps its exact rng
        // sequence; a rooster discards the draw and faces where it was placed (toward the fire).
        let rand_facing = rng01(&mut rng) * TAU;
        let facing = facing_override.unwrap_or(rand_facing);
        let y = worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);

        let ork = Ork {
            home,
            anchor: pos,
            target: pos,
            pos,
            facing,
            speed: st.speed,
            wander_r: st.wander_r,
            gait: st.gait,
            swing: st.swing,
            bob: st.bob,
            body_r: st.body_r,
            phase,
            moving: false,
            mode: OrkMode::Idle,
            seated_rest,
            timer: rng_range(&mut rng, 0.5, 4.0),
            atk_cd: 0.0,
            atk_anim: 0.0,
            hit_recoil: 0.0,
            shaman: st.shaman,
            heal_cd: rng_range(&mut rng, 0.0, SHAMAN_HEAL_CD),
            rng,
            variant,
            faction,
            kb: Vec2::ZERO,
            brawl_target: None,
            brawl_cd: 0.0,
            hunt_path: Vec::new(),
            hunt_cursor: 0,
            hunt_replan_at: 0.0,
            hunt_goal: Vec2::ZERO,
        };

        // Re-rigged onto the shared studio biped skeleton (`biped.rs`): the AI fills a `BipedDrive`
        // (via `ork_drive`) which `animate_biped` turns into the studio clips (idle/walk/run/attack).
        // `BIPED_SIZE_FIX` keeps the taller studio rig at the old ork's in-world height.
        let scale = BASE_SCALE * st.scale * BIPED_SIZE_FIX;
        let drive = crate::biped::BipedDrive { sitting: seated_rest, ..default() };
        let root = commands
            .spawn((
                Transform { translation: Vec3::new(pos.x, y, pos.y), rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(scale) },
                Visibility::Visible,
                ork,
                drive,
                BiomeEntity,
            ))
            .id();
        let head = crate::biped::spawn_biped(
            commands, root, &self.mat, t.biped.clone(),
            1.22, 1.0, 0.17, 0.38, ORK_RIG_OFF, Some(ork_shield_xf()),
        );
        // Two glowing eyes on the head joint (head-local) — the menacing night-glow.
        commands.entity(head).with_children(|p| {
            for off in ORK_BIPED_EYE_OFFS {
                p.spawn((Mesh3d(self.eye_mesh.clone()), MeshMaterial3d(self.eye_mat.clone()), Transform::from_translation(off), OrkEye));
            }
        });
        root
    }

    /// Spawn a DECORATIVE **seated** ork roosting on a camp stump — a biped with no [`Ork`] brain
    /// and no `Health` (can't aggro/path/be hit), held permanently in the seated pose via
    /// `BipedDrive.sitting`. Place `pos` on the stump; the ork faces `facing`.
    pub fn spawn_seated(&self, commands: &mut Commands, variant: OrkVariant, faction: Faction, pos: Vec3, facing: f32) -> Entity {
        let t = self.template(variant, faction);
        let scale = BASE_SCALE * t.st.scale * BIPED_SIZE_FIX;
        let root = commands
            .spawn((
                Transform { translation: pos, rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(scale) },
                Visibility::Visible,
                crate::biped::BipedDrive { sitting: true, ..default() },
                BiomeEntity,
            ))
            .id();
        let head = crate::biped::spawn_biped(commands, root, &self.mat, t.biped.clone(), 1.22, 1.0, 0.17, 0.38, ORK_RIG_OFF, Some(ork_shield_xf()));
        commands.entity(head).with_children(|p| {
            for off in ORK_BIPED_EYE_OFFS {
                p.spawn((Mesh3d(self.eye_mesh.clone()), MeshMaterial3d(self.eye_mat.clone()), Transform::from_translation(off), OrkEye));
            }
        });
        root
    }

    /// Spawn a DECORATIVE ork — the full variant mesh hierarchy (torso, limbs tagged
    /// [`OrkPart`], glowing eyes) with **no** [`Ork`] brain and no `Health`, so it can't
    /// aggro, path, or be targeted by the hero. The fortress population (`ork_fortress.rs`)
    /// drives these with its own wander + limb systems; the caller tags lifecycle
    /// (`BiomeEntity`) and inserts its own driver component on the returned root.
    /// `extra_scale` multiplies the variant's own scale (the warlord is an oversized 1.55×).
    pub fn spawn_prop(
        &self,
        commands: &mut Commands,
        variant: OrkVariant,
        faction: Faction,
        pos: Vec3,
        facing: f32,
        extra_scale: f32,
    ) -> Entity {
        let t = self.template(variant, faction);
        let scale = BASE_SCALE * t.st.scale * extra_scale;
        let root = commands
            .spawn((
                Transform {
                    translation: pos,
                    rotation: Quat::from_rotation_y(facing),
                    scale: Vec3::splat(scale),
                },
                Visibility::Visible,
            ))
            .id();
        commands.entity(root).with_children(|p| {
            p.spawn((Mesh3d(t.torso.clone()), MeshMaterial3d(self.mat.clone()), Transform::default()));
            for (kind, pivot, mesh) in &t.parts {
                p.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(self.mat.clone()),
                    Transform::from_translation(*pivot),
                    OrkPart { kind: *kind },
                ));
            }
            for off in EYE_OFFS {
                p.spawn((
                    Mesh3d(self.eye_mesh.clone()),
                    MeshMaterial3d(self.eye_mat.clone()),
                    Transform::from_translation(off),
                    OrkEye,
                ));
            }
        });
        root
    }
}

// ── Deterministic mulberry32 RNG ─────────────────────────────────────────────────────

fn next_u32(s: &mut u32) -> u32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    t ^ (t >> 14)
}
fn rng01(s: &mut u32) -> f32 {
    next_u32(s) as f32 / 4_294_967_296.0
}
fn rng_range(s: &mut u32, lo: f32, hi: f32) -> f32 {
    lo + rng01(s) * (hi - lo)
}
