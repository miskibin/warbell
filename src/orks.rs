//! **Orks** — the camp warbands. Box-mesh humanoids ported 1:1 from the TS `Ork.tsx` mesh
//! tree, plus an ambient AI: each ork is home-anchored to its camp and idles / patrols within
//! it (the scene is a viewer — no combat, no player aggro, no marching on the keep, no flee).
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

/// Base root scale (the TS group scale before the per-variant `cfg.scale`).
const BASE_SCALE: f32 = 0.7;
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
/// Damage per club hit (queued onto `player::PendingHeroDamage`).
pub(crate) const ORK_DAMAGE: f32 = 8.0;
/// Seconds between an ork's strikes.
pub(crate) const ORK_ATTACK_CD: f32 = 1.1;

// ── Shaman (ranged caster) — ported from `orkConfig.ts` shaman, scaled to this scene. ──
/// A shaman stands off and casts once the hero is within this range (no melee charge).
pub(crate) const SHAMAN_CAST_RANGE: f32 = 8.0;
/// Seconds between bolt casts.
pub(crate) const SHAMAN_CAST_CD: f32 = 2.1;
/// Bolt damage (kept above the club's `ORK_DAMAGE`, as in the original).
pub(crate) const SHAMAN_BOLT_DAMAGE: f32 = 12.0;
/// A shaman heals the nearest wounded ally within this range.
const SHAMAN_HEAL_RANGE: f32 = 8.0;
/// HP restored per heal.
const SHAMAN_HEAL_AMOUNT: f32 = 20.0;
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
/// `ORK_DAMAGE` anchor; others scale by the `ork_config` damage ratio (Scout ≈5, Berserker
/// ≈10). The Shaman casts `SHAMAN_BOLT_DAMAGE` instead, so this is unused for it.
pub(crate) fn variant_melee(v: OrkVariant) -> f32 {
    use tileworld_core::ork_config::{ork_config, OrkVariant as C};
    let grunt = ork_config(C::Grunt).damage as f32; // 24
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
        OrkVariant::Grunt => Stats { skin: 0x3a6a2a, scale: 1.35, speed: 1.6, gait: 7.0, swing: 0.35, bob: 0.05, wander_r: 3.0, body_r: 0.45, shaman: false },
        OrkVariant::Scout => Stats { skin: 0x5f9a3c, scale: 0.78, speed: 2.3, gait: 9.0, swing: 0.42, bob: 0.07, wander_r: 3.6, body_r: 0.28, shaman: false },
        OrkVariant::Berserker => Stats { skin: 0x7a3a26, scale: 1.45, speed: 1.9, gait: 8.0, swing: 0.42, bob: 0.06, wander_r: 3.2, body_r: 0.5, shaman: false },
        OrkVariant::Shaman => Stats { skin: 0x6a3f86, scale: 1.3, speed: 1.3, gait: 6.0, swing: 0.30, bob: 0.04, wander_r: 2.6, body_r: 0.44, shaman: true },
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
    timer: f32,
    /// Strike cooldown (s) — counts down; a hit fires at ≤ 0 and resets it.
    pub(crate) atk_cd: f32,
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
    let cur_y = worldmap::ground_at_world(o.pos.x, o.pos.y).unwrap_or(0.0);
    let step = o.kb * dt;
    let nx = o.pos.x + step.x;
    let nz = o.pos.y + step.y;
    if steer::can_stand(nx, o.pos.y, o.body_r, cur_y) {
        o.pos.x = nx;
    }
    if steer::can_stand(o.pos.x, nz, o.body_r, cur_y) {
        o.pos.y = nz;
    }
    o.kb *= (1.0 - 9.0 * dt).max(0.0);
}

#[derive(Component)]
struct OrkPart {
    kind: PartKind,
}

/// Tags a night-wave invader (vs a leashed camp ork). Spawned + driven by `siege.rs`; the
/// camp [`ork_brain`] skips these via `Without<WaveInvader>` so the two AIs stay separate.
/// Carries the stuck-cull idle tracking (idle far from the keep → reaped so a wave can't hang).
#[derive(Component)]
pub struct WaveInvader {
    pub last_pos: Vec2,
    pub idle_since: f32,
}

// ── Plugin + systems ───────────────────────────────────────────────────────────────

pub struct OrksPlugin;

impl Plugin for OrksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, ork_limbs); // limb anim keeps running while frozen
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
    mut q: Query<(Entity, &mut Ork, &mut Transform, Option<&crate::player::Health>), Without<WaveInvader>>,
) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();

    // True if ANY ork is engaged this frame → swells the combat music layer.
    let mut fighting = false;

    // Snapshot (entity, faction, pos) so an ork with no hero in sight can seek a rival to brawl.
    let snap: Vec<(Entity, Faction, Vec2)> = q.iter().map(|(e, o, _, _)| (e, o.faction, o.pos)).collect();

    for (self_e, mut o, mut tf, health) in &mut q {
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
                    o.target = o.home;
                }
            }
        }

        // Spatial grunt on first aggro, and again on closing to strike range (Hunt→Attack) —
        // the two beats the old `Ork.tsx` grunts on (acquire target / start a swing). Per-ork
        // transition edges, so no per-frame spam.
        let was_engaged = matches!(prev_mode, OrkMode::Hunt | OrkMode::Attack);
        let now_engaged = matches!(o.mode, OrkMode::Hunt | OrkMode::Attack);
        if (!was_engaged && now_engaged) || (prev_mode == OrkMode::Hunt && o.mode == OrkMode::Attack) {
            let gy = worldmap::ground_at_world(o.pos.x, o.pos.y).unwrap_or(0.0);
            cues.write(crate::audio::AudioCue::OrkGrunt(Vec3::new(o.pos.x, gy + 1.0, o.pos.y)));
        }
        if now_engaged {
            fighting = true;
        }

        match o.mode {
            OrkMode::Idle => {
                o.moving = false;
                if o.timer <= 0.0 {
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
                    let cur_y = worldmap::ground_at_world(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
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
                // Charge the hero faster than a patrol, steering around props/cliffs.
                let cur_y = worldmap::ground_at_world(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
                let speed = o.speed * 1.4 * if frenzied { 1.4 } else { 1.0 };
                match steer::advance(o.pos, o.facing, o.target, speed * dt, o.body_r, cur_y, ORK_MAX_TURN * 1.6 * dt) {
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
                    if o.shaman {
                        o.atk_cd = SHAMAN_CAST_CD;
                        let gy = worldmap::ground_at_world(o.pos.x, o.pos.y).unwrap_or(0.0);
                        bolts.0.push(crate::projectile::BoltSpawn {
                            origin: Vec3::new(o.pos.x, gy + 1.4, o.pos.y),
                            damage: SHAMAN_BOLT_DAMAGE,
                        });
                    } else {
                        o.atk_cd = ORK_ATTACK_CD * if frenzied { 0.6 } else { 1.0 };
                        pending.0 += variant_melee(o.variant);
                    }
                }
            }
        }

        apply_knockback(&mut o, dt);

        let gy = worldmap::ground_at_world(o.pos.x, o.pos.y).unwrap_or(tf.translation.y);
        let bob = if o.moving { (tw * o.gait + o.phase).sin().abs() * o.bob } else { 0.0 };
        tf.translation = Vec3::new(o.pos.x, gy + bob, o.pos.y);
        tf.rotation = Quat::from_rotation_y(o.facing);
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

fn ork_limbs(time: Res<Time>, orks: Query<(&Ork, &Children)>, mut parts: Query<(&OrkPart, &mut Transform)>) {
    let tw = time.elapsed_secs_wrapped();
    for (o, children) in &orks {
        let t = tw + o.phase;
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                // Legs stride; arms swing opposite the legs (and opposite each other via sign).
                PartKind::Leg(sign) => {
                    let s = if o.moving { (t * o.gait).sin() * o.swing } else { (t * 0.8).sin() * 0.03 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Arm(sign) => {
                    // Shaman raises its staff (right arm) while casting.
                    if o.shaman && sign > 0.0 && matches!(o.mode, OrkMode::Attack) {
                        Quat::from_rotation_x(-1.3)
                    } else {
                        let s = if o.moving { -(t * o.gait).sin() * 0.42 } else { (t * 0.8).sin() * 0.05 };
                        Quat::from_rotation_x(sign * s)
                    }
                }
                PartKind::Head => {
                    let bob = (t * 0.5).sin() * 0.06;
                    let scan = if o.moving { 0.0 } else { (t * 0.4).sin() * 0.25 };
                    Quat::from_euler(EulerRot::XYZ, bob, scan, 0.0)
                }
                PartKind::Tail => Quat::IDENTITY, // orks have no tail
            };
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
    mut q: Query<(Entity, &mut Ork, &GlobalTransform, &mut crate::player::Health)>,
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
    mut q: Query<(Entity, &mut Ork, &mut crate::player::Health), Without<WaveInvader>>,
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
                    commands.entity(e).try_despawn();
                }
            }
        }
    }
}

fn pick_patrol(o: &mut Ork) {
    let ang = rng01(&mut o.rng) * TAU;
    let r = rng_range(&mut o.rng, o.wander_r * 0.3, o.wander_r);
    o.target = o.home + Vec2::new(ang.cos() * r, ang.sin() * r);
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
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), c)
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

fn spec(variant: OrkVariant, faction: Faction) -> OrkSpec {
    let st = stats(variant);
    let skin = lin(st.skin);
    let dark = lin_scaled(st.skin, 0.62); // SKIN_DARK_ACCENT
    let fac = lin(faction.hex());
    const BELT: u32 = 0x3a2616;
    const TUSK: u32 = 0xece1c2;
    const EYE: u32 = 0xe6c828;
    const CLUB_WOOD: u32 = 0x4a2a16;
    const CLUB_BAND: u32 = 0x1a1008;
    const STAFF_WOOD: u32 = 0x6a4a2a;
    const ORB: u32 = 0xc89cff;
    let body_rot = Quat::from_rotation_x(0.2);
    let body_off = v(0.0, 0.74, 0.05);

    // Static torso: loincloth (faction) + belt + the pitched body group (torso + war-paint).
    let torso = group(vec![
        bx(0.55, 0.2, 0.3, v(0.0, 0.4, 0.0), fac), // loincloth
        bx(0.56, 0.06, 0.31, v(0.0, 0.49, 0.0), lin(BELT)),
        baked(bx(0.55, 0.42, 0.34, Vec3::ZERO, skin), body_rot, body_off), // torso
        baked(bx(0.12, 0.32, 0.006, v(0.0, 0.0, 0.175), fac), body_rot, body_off), // war-paint vert
        baked(bx(0.4, 0.06, 0.004, v(0.0, 0.0, 0.176), dark), body_rot, body_off), // war-paint horiz
    ]);

    // Head: skull + brow + eyes + tusks + ears.
    let head = group(vec![
        bx(0.36, 0.34, 0.34, Vec3::ZERO, skin),
        bx(0.32, 0.06, 0.01, v(0.0, 0.06, 0.175), dark),
        bx(0.05, 0.04, 0.008, v(-0.08, 0.02, 0.175), lin(EYE)),
        bx(0.05, 0.04, 0.008, v(0.08, 0.02, 0.175), lin(EYE)),
        cone(0.026, 0.13, v(-0.08, -0.1, 0.17), rz(-0.15), lin(TUSK)),
        cone(0.026, 0.13, v(0.08, -0.1, 0.17), rz(0.15), lin(TUSK)),
        bx(0.06, 0.12, 0.14, v(-0.2, 0.0, 0.0), skin),
        bx(0.06, 0.12, 0.14, v(0.2, 0.0, 0.0), skin),
    ]);

    // Right arm + baked weapon (club for melee, staff + orb for shaman).
    let mut arm_r = vec![
        bx(0.2, 0.1, 0.3, v(0.0, -0.02, 0.0), dark), // shoulder
        bxr(0.17, 0.5, 0.24, v(0.02, -0.25, 0.04), xyz(0.2, 0.0, 0.05), skin), // upper
        bxr(0.16, 0.1, 0.22, v(0.04, -0.52, 0.08), xyz(0.2, 0.0, 0.05), dark), // forearm
    ];
    if st.shaman {
        let wr = xyz(0.1, 0.0, 0.08);
        let wo = v(0.05, -0.5, 0.1);
        arm_r.push(baked(cyl(0.033, 1.1, v(0.0, -0.1, 0.0), Quat::IDENTITY, lin(STAFF_WOOD)), wr, wo));
        arm_r.push(baked(orb(0.1, v(0.0, 0.5, 0.0), lin(ORB)), wr, wo));
    } else {
        let wr = xyz(0.4, 0.0, 0.1);
        let wo = v(0.05, -0.65, 0.1);
        arm_r.push(baked(cyl(0.04, 0.26, v(0.0, -0.1, 0.0), Quat::IDENTITY, lin(CLUB_WOOD)), wr, wo));
        arm_r.push(baked(cyl(0.09, 0.34, v(0.0, -0.36, 0.0), Quat::IDENTITY, lin(CLUB_WOOD)), wr, wo));
        for i in 0..4 {
            let a = i as f32 * std::f32::consts::FRAC_PI_2;
            let spike = cone(0.03, 0.09, v(a.cos() * 0.1, -0.36, a.sin() * 0.1), xyz(0.0, a, std::f32::consts::FRAC_PI_2), lin(CLUB_BAND));
            arm_r.push(baked(spike, wr, wo));
        }
    }
    let arm_r = group(arm_r);

    // Left arm.
    let arm_l = group(vec![
        bx(0.2, 0.1, 0.3, v(0.0, -0.02, 0.0), dark),
        bxr(0.17, 0.5, 0.24, v(-0.02, -0.25, 0.04), xyz(0.2, 0.0, -0.05), skin),
        bxr(0.18, 0.13, 0.26, v(-0.04, -0.52, 0.08), xyz(0.2, 0.0, -0.05), dark),
    ]);

    // Legs (built top-at-origin so the hip pivot sits at the top; feet rest at root y≈0).
    let leg = || group(vec![bx(0.2, 0.36, 0.22, v(0.0, -0.18, 0.0), skin)]);

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
    st: Stats,
}

pub struct Armory {
    mat: Handle<StandardMaterial>,
    tmpl: Vec<((OrkVariant, Faction), Template)>,
}

impl Armory {
    /// Build all 8 variant×faction ork meshes against the shared vertex-colour `mat`.
    pub fn new(meshes: &mut Assets<Mesh>, mat: Handle<StandardMaterial>) -> Armory {
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
                        st,
                    },
                ));
            }
        }
        Armory { mat, tmpl }
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
        let t = self.template(variant, faction);
        let st = t.st;
        let mut rng = seed | 1;
        let phase = rng01(&mut rng) * TAU;
        let facing = rng01(&mut rng) * TAU;
        let y = worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);

        let ork = Ork {
            home,
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
            timer: rng_range(&mut rng, 0.5, 4.0),
            atk_cd: 0.0,
            shaman: st.shaman,
            heal_cd: rng_range(&mut rng, 0.0, SHAMAN_HEAL_CD),
            rng,
            variant,
            faction,
            kb: Vec2::ZERO,
            brawl_target: None,
            brawl_cd: 0.0,
        };

        let scale = BASE_SCALE * st.scale;
        let root = commands
            .spawn((
                Transform { translation: Vec3::new(pos.x, y, pos.y), rotation: Quat::from_rotation_y(facing), scale: Vec3::splat(scale) },
                Visibility::Visible,
                ork,
                BiomeEntity,
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
