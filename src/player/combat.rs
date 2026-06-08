//! Hero melee — left-click swing, a front-cone hit scan over orks + wildlife, damage +
//! despawn, with impact/death spark bursts. Ported from the swing logic in `Character.tsx`.
//!
//! Targeting lives HERE, not in `orks.rs` / `wildlife.rs`, so those shared model files stay
//! untouched: [`Health`] is attached to their entities externally by [`ensure_combat_health`]
//! and the cone scan reads each target's `GlobalTransform`.

use bevy::prelude::*;

use tileworld_core::player::{cleave_damage, roll_crit, CLEAVE_R2};

use crate::audio::AudioCue;
use crate::orks::Ork;
use crate::wildlife::Animal;

use super::camera::OrbitCam;
use super::{Hero, HeroHealth, PlayMode, PlayerRes};

pub const ATTACK_DURATION: f32 = 0.45;
const ATTACK_RANGE: f32 = 1.8;
const ATTACK_CONE_DOT: f32 = 0.5; // cos 60° — front cone half-angle
const HIT_PHASE: f32 = 0.3; // fraction into the swing where damage lands

/// Deterministic crit-roll source — one roll per swing. mulberry32 ("feels-the-same", no
/// byte-parity need). Init in `PlayerPlugin`.
#[derive(Resource)]
pub struct CombatRng(u32);
impl Default for CombatRng {
    fn default() -> Self {
        CombatRng(0x1234_5678)
    }
}
impl CombatRng {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }
}

/// Brief whole-sim freeze on a hit/kill — the "punch" that sells a blow. Combat tops up
/// `remaining`; [`drive_hit_stop`] zeroes virtual time while it counts down (AI, orbs and anim
/// all hang for the beat). Runs ungated so it always resumes the clock.
#[derive(Resource, Default)]
pub struct HitStop {
    pub remaining: f32,
}

const HITSTOP_KILL: f32 = 0.09;
const HITSTOP_HIT: f32 = 0.05;
const SHAKE_KILL: f32 = 0.55;
const SHAKE_HIT: f32 = 0.30;
const KNOCKBACK: f32 = 6.0;
const KNOCKBACK_CRIT: f32 = 9.0;

pub fn drive_hit_stop(
    real: Res<Time<bevy::time::Real>>,
    mut vtime: ResMut<Time<bevy::time::Virtual>>,
    mut hs: ResMut<HitStop>,
) {
    if hs.remaining > 0.0 {
        hs.remaining -= real.delta_secs();
        vtime.set_relative_speed(0.0);
    } else {
        vtime.set_relative_speed(1.0);
    }
}

/// Vitals attached to a hittable (ork / animal) by [`ensure_combat_health`].
#[derive(Component)]
pub struct Health {
    pub hp: f32,
    /// Read by the M3 HUD; populated now so combat owns the full vitals.
    #[allow(dead_code)]
    pub max: f32,
}

/// One impact/death mote: a tiny lit sphere that flies out, falls and fades.
#[derive(Component)]
pub(crate) struct Spark {
    vel: Vec3,
    life: f32,
    life0: f32,
    scale0: f32,
}

/// Shared spark mesh + materials, built once.
#[derive(Resource)]
pub(crate) struct CombatFx {
    mesh: Handle<Mesh>,
    hit: Handle<StandardMaterial>,
    kill: Handle<StandardMaterial>,
    /// Green motes for the shaman's heal cast.
    heal: Handle<StandardMaterial>,
}

pub fn setup_combat_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.07).mesh().ico(1).unwrap());
    let hit = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.81, 0.42),
        emissive: LinearRgba::rgb(3.0, 1.6, 0.4),
        unlit: true,
        ..default()
    });
    let kill = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.94, 0.7),
        emissive: LinearRgba::rgb(5.0, 4.0, 1.6),
        unlit: true,
        ..default()
    });
    let heal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 1.0, 0.6),
        emissive: LinearRgba::rgb(0.8, 3.0, 1.2),
        unlit: true,
        ..default()
    });
    commands.insert_resource(CombatFx { mesh, hit, kill, heal });
}

/// Attach `Health` to every ork / animal that lacks it (so combat can target them without
/// editing their spawn code). Cheap: the queries are empty once everyone is tagged.
pub fn ensure_combat_health(
    mut commands: Commands,
    orks: Query<(Entity, &Ork), Without<Health>>,
    animals: Query<(Entity, &Animal), Without<Health>>,
) {
    // Camp orks get their variant's base HP (60/32/72/47); wave invaders already carry their
    // scaled HP from `siege::spawn_invader`, so they never fall through to here.
    for (e, o) in &orks {
        let hp = crate::siege::base_hp(o.variant);
        commands.entity(e).try_insert(Health { hp, max: hp });
    }
    // Per-species animal HP (rescaled from core's `animal_config` into forest's combat units) —
    // a wolf/bear now soaks several blows where a rabbit still pops in one.
    for (e, a) in &animals {
        let hp = crate::verbs::animal_profile(a.species).hp;
        commands.entity(e).try_insert(Health { hp, max: hp });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn player_attack(
    time: Res<Time>,
    mode: Res<PlayMode>,
    buttons: Res<ButtonInput<MouseButton>>,
    orbit: Res<OrbitCam>,
    fx: Option<Res<CombatFx>>,
    mut player: ResMut<PlayerRes>,
    mut rng: ResMut<CombatRng>,
    mut mods: crate::inventory::CombatMods,
    mut rewards: ResMut<crate::orbs::RewardBursts>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
    mut hitstop: ResMut<HitStop>,
    mut commands: Commands,
    mut cues: MessageWriter<AudioCue>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut hero_q: Query<(&mut Hero, &HeroHealth)>,
    mut targets: Query<
        (Entity, &GlobalTransform, &mut Health, Option<&mut Ork>, Option<&Animal>),
        Or<(With<Ork>, With<Animal>)>,
    >,
) {
    let Ok((mut hero, hh)) = hero_q.single_mut() else { return };
    if *mode != PlayMode::Play || !player.0.is_alive() {
        hero.attacking = false;
        return;
    }
    let dt = time.delta_secs();

    // Start a swing on a click — only while the cursor is locked (actually playing) and not
    // guarding (raising the shield takes priority over swinging).
    if !hero.attacking && !hh.blocking && orbit.locked && buttons.just_pressed(MouseButton::Left) {
        hero.attacking = true;
        hero.attack_t = 0.0;
        hero.hit_dealt = false;
        // Only the exertion grunt fires on the wind-up (and only ~34% of the time — voice gates
        // it). The whoosh is DEFERRED to hit resolution so a connecting blow plays its impact
        // alone and a whiff plays the whoosh alone — never both (Character.tsx).
        cues.write(AudioCue::HeroGruntSwing);
    }
    if !hero.attacking {
        return;
    }
    hero.attack_t += dt;
    let phase = hero.attack_t / ATTACK_DURATION;
    if phase >= 1.0 {
        hero.attacking = false;
        return;
    }
    if hero.hit_dealt || phase < HIT_PHASE {
        return;
    }
    hero.hit_dealt = true;

    // ── Hit scan: a front cone over orks + wildlife ──
    let Some(fx) = fx else { return };
    let fwd = Vec2::new(hero.facing.sin(), hero.facing.cos());
    let origin = hero.pos;

    // One crit roll per swing (TS): every cone target shares it. Damage =
    // (attack_damage + equipped-weapon bonus) × active power-buff, doubled on crit, rounded.
    let now = time.elapsed_secs() as f64;
    let base = (player.0.attack_damage + mods.weapon_bonus()) * mods.power_mult(now);
    // Broadcast the cone so ore/dummies share this swing (non-crit damage).
    mods.publish_swing(origin, fwd, base.round() as f32);
    let (dmg_f, crit) = roll_crit(base, player.0.crit_chance, rng.unit());
    let dmg = dmg_f.round() as f32;
    let bounty_mult = player.0.bounty_mult;
    let lifesteal = player.0.lifesteal;

    let mut hit_any = false;
    let mut killed_any = false;
    // Direct-hit bookkeeping for the cleave pass (positions to splash from + who's already hit).
    let mut struck: Vec<Vec2> = Vec::new();
    let mut hit_ents: Vec<Entity> = Vec::new();
    for (e, gt, mut hp, mut ork, animal) in &mut targets {
        let p = gt.translation();
        let to = Vec2::new(p.x - origin.x, p.z - origin.y);
        let dist = to.length();
        if dist > ATTACK_RANGE || dist < 1e-3 {
            continue;
        }
        let dir = to / dist;
        if dir.dot(fwd) < ATTACK_CONE_DOT {
            continue;
        }
        struck.push(Vec2::new(p.x, p.z));
        hit_ents.push(e);
        hp.hp -= dmg;
        let dead = hp.hp <= 0.0;
        spawn_burst(&mut commands, &fx, Vec3::new(p.x, p.y + 0.9, p.z), dead);
        hit_any = true;
        // Floating number above the target + a white hurt-flash on a survivor.
        let head = Vec3::new(p.x, p.y + 2.2, p.z);
        if dead {
            floats.0.push(crate::combat_fx::FloatReq {
                world: head,
                text: "☠".into(),
                color: crate::combat_fx::col_kill(),
                scale: 1.4,
            });
            killed_any = true;
            // Any kill bursts bounty gold + xp (as reward orbs) and feeds lifesteal. Orks pull
            // their bounty from `ork_config`; wild animals from the rescaled `animal_profile`,
            // and additionally roll loot drops (handled in `verbs::animal_drops`).
            let (gold, xp) = if let Some(o) = ork.as_deref() {
                (crate::orks::bounty_gold(o.variant, bounty_mult), crate::orks::bounty_xp(o.variant))
            } else if let Some(an) = animal {
                let prof = crate::verbs::animal_profile(an.species);
                mods.publish_animal_kill(p, an.species);
                ((prof.gold as f64 * bounty_mult).round() as i64, prof.xp)
            } else {
                (0, 0)
            };
            if gold > 0 || xp > 0 {
                rewards.0.push(crate::orbs::RewardBurst { at: p, gold, xp });
            }
            if lifesteal > 0.0 {
                player.0.heal(lifesteal);
            }
            // `try_despawn`: a defender bolt / biome rebuild may have already reaped this target.
            commands.entity(e).try_despawn();
        } else {
            // Shove a surviving ork back along the blow (harder on a crit).
            if let Some(o) = ork.as_deref_mut() {
                o.kb = dir * if crit { KNOCKBACK_CRIT } else { KNOCKBACK };
            }
            // Crit reads as "{dmg}!" in gold; a normal hit is the plain number.
            let (text, color) = if crit {
                (format!("{}!", dmg as i32), crate::combat_fx::col_kill())
            } else {
                (format!("{}", dmg as i32), crate::combat_fx::col_ork_hit())
            };
            floats.0.push(crate::combat_fx::FloatReq {
                world: head,
                text,
                color,
                scale: if crit { 1.2 } else { 1.0 },
            });
            commands.entity(e).insert(crate::combat_fx::HurtFlash::new(time.elapsed_secs()));
        }
    }
    // ── Cleave pass: splash a fraction of the swing to OTHER orks near a struck target ──
    // (upgrade-gated — `cleave` is 0 until the Champion node is bought, so this is a no-op).
    if player.0.cleave > 0.0 && !struck.is_empty() {
        let splash = cleave_damage(dmg as f64, player.0.cleave) as f32;
        if splash > 0.0 {
            for (e, gt, mut hp, ork, _animal) in &mut targets {
                if ork.is_none() || hit_ents.contains(&e) {
                    continue; // cleave only hits orks, and never the directly-struck ones twice
                }
                let p = gt.translation();
                let pos = Vec2::new(p.x, p.z);
                if !struck.iter().any(|s| s.distance_squared(pos) <= CLEAVE_R2 as f32) {
                    continue;
                }
                hp.hp -= splash;
                let head = Vec3::new(p.x, p.y + 2.2, p.z);
                if hp.hp <= 0.0 {
                    spawn_burst(&mut commands, &fx, Vec3::new(p.x, p.y + 0.9, p.z), true);
                    floats.0.push(crate::combat_fx::FloatReq {
                        world: head,
                        text: "☠".into(),
                        color: crate::combat_fx::col_kill(),
                        scale: 1.2,
                    });
                    if let Some(o) = ork.as_deref() {
                        rewards.0.push(crate::orbs::RewardBurst {
                            at: p,
                            gold: crate::orks::bounty_gold(o.variant, bounty_mult),
                            xp: crate::orks::bounty_xp(o.variant),
                        });
                        if lifesteal > 0.0 {
                            player.0.heal(lifesteal);
                        }
                    }
                    commands.entity(e).try_despawn();
                } else {
                    floats.0.push(crate::combat_fx::FloatReq {
                        world: head,
                        text: format!("{}", splash as i32),
                        color: crate::combat_fx::col_ork_hit(),
                        scale: 0.85,
                    });
                    commands.entity(e).insert(crate::combat_fx::HurtFlash::new(time.elapsed_secs()));
                }
            }
        }
    }

    // Juice: a connecting blow shakes the screen + briefly freezes the sim (heavier on a kill).
    if killed_any {
        feedback.trauma = (feedback.trauma + SHAKE_KILL).min(1.0);
        hitstop.remaining = hitstop.remaining.max(HITSTOP_KILL);
    } else if hit_any {
        feedback.trauma = (feedback.trauma + SHAKE_HIT).min(1.0);
        hitstop.remaining = hitstop.remaining.max(HITSTOP_HIT);
    }

    // One sting for the whole swing: impact on a connect (heavier on a kill), else the empty-
    // swing whoosh. Never stacked — a connecting hit never also plays the whoosh.
    if killed_any {
        cues.write(AudioCue::Impact { kill: true });
    } else if hit_any {
        cues.write(AudioCue::Impact { kill: false });
    } else {
        cues.write(AudioCue::Swing);
    }
}

/// A green sparkle burst when the shaman heals an ally — rising motes.
pub(crate) fn spawn_heal_burst(commands: &mut Commands, fx: &CombatFx, at: Vec3) {
    for i in 0..10u32 {
        let a = i as f32 * 2.399_963_2;
        let mag = 0.4 + ((i * 29 % 10) as f32) * 0.05;
        let vel = Vec3::new(a.cos() * 1.4, 1.8 + (i % 3) as f32 * 0.3, a.sin() * 1.4) * mag;
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.heal.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.6)),
            Spark { vel, life: 0.5, life0: 0.5, scale0: 0.6 },
            bevy::light::NotShadowCaster,
        ));
    }
}

pub(crate) fn spawn_burst(commands: &mut Commands, fx: &CombatFx, at: Vec3, kill: bool) {
    let (n, spd, mat, scale0) =
        if kill { (16u32, 4.0, fx.kill.clone(), 1.0) } else { (8, 2.6, fx.hit.clone(), 0.7) };
    for i in 0..n {
        let a = i as f32 * 2.399_963_2; // golden angle → even-ish spread
        let mag = 0.6 + ((i * 37 % 10) as f32) * 0.06;
        let up = 0.4 + (i % 3) as f32 * 0.35;
        let vel = Vec3::new(a.cos() * spd, up * spd * 0.6, a.sin() * spd) * mag;
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(scale0)),
            Spark { vel, life: 0.45, life0: 0.45, scale0 },
            bevy::light::NotShadowCaster,
        ));
    }
}

pub fn update_sparks(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Spark, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut s, mut tf) in &mut q {
        s.life -= dt;
        if s.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        s.vel.y -= 9.0 * dt;
        let v = s.vel;
        tf.translation += v * dt;
        let k = s.life / s.life0;
        tf.scale = Vec3::splat(s.scale0 * k);
    }
}
