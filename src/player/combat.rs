//! Hero melee — left-click swing, a front-cone hit scan over orks + wildlife, damage +
//! despawn, with impact/death spark bursts. Ported from the swing logic in `Character.tsx`.
//!
//! Targeting lives HERE, not in `orks.rs` / `wildlife.rs`, so those shared model files stay
//! untouched: [`Health`] is attached to their entities externally by [`ensure_combat_health`]
//! and the cone scan reads each target's `GlobalTransform`.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use tileworld_core::player::{cleave_damage, roll_crit, CLEAVE_R2};

use crate::audio::AudioCue;
use crate::orks::Ork;
use crate::wildlife::Animal;

use super::camera::OrbitCam;
use super::{Hero, HeroHealth, HeroLimb, HeroPart, PlayMode, PlayerRes};

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
/// `remaining`; [`drive_hit_stop`] crawls virtual time (×`HITSTOP_SLOWMO`) while it counts down
/// (AI, orbs and anim all slow to a near-stop for the beat). Runs ungated so it always resumes
/// the clock.
#[derive(Resource, Default)]
pub struct HitStop {
    pub remaining: f32,
}

const HITSTOP_KILL: f32 = 0.09;
const HITSTOP_HIT: f32 = 0.05;
/// During hit-stop the sim runs at this fraction of real time rather than a dead freeze — a brief
/// slow-mo dip that still sells the "punch" without the full-stop stutter that read as micro-lag.
const HITSTOP_SLOWMO: f32 = 0.15;
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
        vtime.set_relative_speed(HITSTOP_SLOWMO);
    } else {
        vtime.set_relative_speed(1.0);
    }
}

/// Hit-feedback output channels bundled into one [`SystemParam`] — keeps [`player_attack`] under
/// Bevy's 16-param ceiling now that it also spawns the planar impact flashes (which need
/// `Assets<StandardMaterial>` to clone a per-instance fade material).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Juice<'w> {
    feedback: ResMut<'w, crate::combat_fx::HitFeedback>,
    hitstop: ResMut<'w, HitStop>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

/// Townsfolk the hero can harmlessly bonk with a swing: the [`crate::villagers::Villager`] bodies
/// (which are NOT in the damage-dealing `targets` query) plus the voice channel to make them yelp.
/// Bundled into one [`SystemParam`] so [`player_attack`] stays under Bevy's 16-param ceiling.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Bystanders<'w, 's> {
    speak: MessageWriter<'w, crate::audio::Speak>,
    folk: Query<
        'w,
        's,
        &'static GlobalTransform,
        (With<crate::villagers::Villager>, Without<crate::dying::Dying>),
    >,
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

/// Shared spark + impact-flash assets, built once. The faded planar effects (slash / shockwave /
/// splat) clone `*_mat` per instance so their alpha can fade independently; the spark materials
/// (`hit`/`kill`/`heal`/`blood`/`chip`) are shared and fade by shrinking the mote instead.
#[derive(Resource)]
pub(crate) struct CombatFx {
    mesh: Handle<Mesh>,
    hit: Handle<StandardMaterial>,
    kill: Handle<StandardMaterial>,
    /// Green motes for the shaman's heal cast.
    heal: Handle<StandardMaterial>,
    /// Dark crimson, unlit, NO bloom — blood spray on a creature hit.
    blood: Handle<StandardMaterial>,
    /// Grey stone — rock chips off a mined ore boulder.
    chip: Handle<StandardMaterial>,
    /// Warm tan — Sand-Dash afterimage dust puffs.
    dust: Handle<StandardMaterial>,
    /// Green — Bramble-Sweep leaf/thorn motes flung outward.
    leaf: Handle<StandardMaterial>,
    /// A unit quad (slash streak) + flat ring (kill shockwave) + disc (blood splat).
    quad: Handle<Mesh>,
    ring: Handle<Mesh>,
    disc: Handle<Mesh>,
    /// Base materials cloned per planar-flash instance (see `spawn_fade`).
    slash_mat: Handle<StandardMaterial>,
    ring_mat: Handle<StandardMaterial>,
    splat_mat: Handle<StandardMaterial>,
    /// Non-emissive pale gold for the blade-trail ribbon (bloom must never catch the trail).
    trail_mat: Handle<StandardMaterial>,
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
    // Blood: dark, lightless — it must NOT glow under the scene's bloom (sparks do).
    let blood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.54, 0.10, 0.10),
        unlit: true,
        ..default()
    });
    let chip = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.62, 0.66),
        unlit: true,
        ..default()
    });
    // Sand-dash dust: warm tan, no glow (it's dust, not energy).
    let dust = materials.add(StandardMaterial {
        base_color: Color::srgb(0.80, 0.67, 0.42),
        unlit: true,
        ..default()
    });
    // Bramble leaves: green with a faint glow so they pop against foliage.
    let leaf = materials.add(StandardMaterial {
        base_color: Color::srgb(0.34, 0.60, 0.24),
        emissive: LinearRgba::rgb(0.22, 0.55, 0.12),
        unlit: true,
        ..default()
    });
    let quad = meshes.add(Rectangle::new(1.0, 1.0).mesh().build());
    let ring = meshes.add(Annulus::new(0.72, 1.0).mesh().build());
    let disc = meshes.add(Circle::new(0.5).mesh().build());
    // Planar flashes blend over the scene (alpha-faded each frame) and carry an emissive so
    // bloom picks them up while they're bright. Double-sided so they read from either face.
    let flash = |base: Color, emissive: LinearRgba| StandardMaterial {
        base_color: base,
        emissive,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    };
    // Warm, dim glint — NOT hot white: the old (4.0, 3.6, 2.8) emissive at 0.9 alpha bloomed
    // into a big white flare on every contact. Kept just bright enough for bloom to kiss it.
    let slash_mat = materials.add(flash(Color::srgba(1.0, 0.92, 0.74, 0.5), LinearRgba::rgb(1.2, 0.95, 0.55)));
    // The blade-trail ribbon: pale gold with ZERO emissive, so bloom can never whiten it — the
    // trail should read as a translucent smear of motion, not a light source.
    let trail_mat = materials.add(flash(Color::srgba(0.95, 0.87, 0.62, 0.28), LinearRgba::BLACK));
    let ring_mat = materials.add(flash(Color::srgba(1.0, 0.86, 0.6, 0.45), LinearRgba::rgb(1.4, 0.95, 0.4)));
    let splat_mat = materials.add(flash(Color::srgba(0.42, 0.06, 0.06, 0.72), LinearRgba::BLACK));
    commands.insert_resource(CombatFx {
        mesh, hit, kill, heal, blood, chip, dust, leaf, quad, ring, disc, slash_mat, ring_mat,
        splat_mat, trail_mat,
    });
}

/// Sand-Dash afterimage: a line of low tan dust puffs strung along the blink path — subtle,
/// short-lived motes that read as a sand-trail behind the teleport.
pub(crate) fn spawn_dash_trail(commands: &mut Commands, fx: &CombatFx, from: Vec3, to: Vec3) {
    let steps = 7;
    for i in 0..=steps {
        let p = from.lerp(to, i as f32 / steps as f32) + Vec3::Y * 0.55;
        spawn_motes(commands, &fx.mesh, &fx.dust, p, 3, 1.1, 0.42, 0.40);
    }
}

/// Bramble-Sweep: a 360° fling of green leaf/thorn motes radiating out from the hero, riding the
/// expanding ring.
pub(crate) fn spawn_sweep_burst(commands: &mut Commands, fx: &CombatFx, at: Vec3) {
    for i in 0..18u32 {
        let a = i as f32 * 2.399_963_2; // golden angle → even ring
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let vel = dir * (3.4 + (i % 5) as f32 * 0.35) + Vec3::Y * (1.1 + (i % 3) as f32 * 0.4);
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.leaf.clone()),
            Transform::from_translation(at + Vec3::Y * 0.7).with_scale(Vec3::splat(0.55)),
            Spark { vel, life: 0.55, life0: 0.55, scale0: 0.55 },
            bevy::light::NotShadowCaster,
        ));
    }
}

/// A short-lived planar flash (slash streak / kill shockwave / blood splat / blade-trail ribbon).
/// Owns a cloned material so it can alpha-fade alone; [`update_fx_fades`] frees that material on
/// despawn so per-hit clones never leak.
#[derive(Component)]
pub(crate) struct FxFade {
    born: f32,
    life: f32,
    mat: Handle<StandardMaterial>,
    s0: Vec3,
    s1: Vec3,
    a0: f32,
    face: FadeFace,
}

/// How a fading quad orients each frame.
#[derive(Clone, Copy)]
pub(crate) enum FadeFace {
    /// Keep the spawn pose (the ground-flat shockwave + splat).
    Keep,
    /// Re-face the camera flat-on each frame (the contact slash flash).
    Camera,
    /// Cylindrical billboard: the quad's local X stays pinned along this world axis while it
    /// rolls about it to face the camera — a ribbon segment (the blade trail), so the streak
    /// follows the blade's real sweep instead of flashing a fixed-size card at the camera.
    Axis(Vec3),
}

/// Drain a planar flash over its life: scale `s0→s1` (ease-out), fade alpha `a0→0`, orient per
/// its [`FadeFace`], then despawn + free the cloned material.
pub fn update_fx_fades(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    mut q: Query<(Entity, &FxFade, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    let cam_pos = cam_q.single().map(|t| t.translation()).ok();
    for (e, f, mut tf) in &mut q {
        let k = (now - f.born) / f.life;
        if k >= 1.0 {
            commands.entity(e).despawn();
            mats.remove(&f.mat);
            continue;
        }
        let ease = 1.0 - (1.0 - k) * (1.0 - k); // ease-out
        tf.scale = f.s0.lerp(f.s1, ease);
        match (f.face, cam_pos) {
            (FadeFace::Camera, Some(cp)) => tf.look_at(cp, Vec3::Y),
            (FadeFace::Axis(a), Some(cp)) => {
                // Quad normal = the camera direction with its along-axis component removed, so
                // the ribbon stays edge-true to the sweep while showing the camera its face.
                let to_cam = cp - tf.translation;
                let n = (to_cam - a * a.dot(to_cam)).normalize_or_zero();
                if n != Vec3::ZERO {
                    tf.rotation = Quat::from_mat3(&Mat3::from_cols(a, n.cross(a), n));
                }
            }
            _ => {}
        }
        if let Some(m) = mats.get_mut(&f.mat) {
            m.base_color = m.base_color.with_alpha(f.a0 * (1.0 - k));
        }
    }
}

/// Attach `Health` to every ork / animal that lacks it (so combat can target them without
/// editing their spawn code). Cheap: the queries are empty once everyone is tagged.
pub fn ensure_combat_health(
    mut commands: Commands,
    orks: Query<(Entity, &Ork, &Transform), Without<Health>>,
    animals: Query<(Entity, &Animal), Without<Health>>,
) {
    // Camp orks get their variant's full old-game base HP (254/136/306/201, via `siege::base_hp`),
    // scaled UP by frontier distance (×1 near the castle → ×2 at the rim) so deep-biome warbands
    // are genuinely tankier. Wave invaders already carry their wave-scaled HP from
    // `siege::spawn_invader`, so they never fall through to here (and stay un-distance-scaled).
    for (e, o, tf) in &orks {
        let (hp_mul, _) = crate::verbs::frontier_threat(tf.translation.x, tf.translation.z);
        let hp = (crate::siege::base_hp(o.variant) * hp_mul).round();
        commands.entity(e).try_insert(Health { hp, max: hp });
    }
    // Per-species animal HP (from core's `animal_config`), same frontier scaling — a rim wolf
    // soaks twice the blows of a home-wood one (a rabbit still pops near-instantly either way).
    for (e, a) in &animals {
        let (hp_mul, _) = crate::verbs::frontier_threat(a.pos.x, a.pos.y);
        let hp = (crate::verbs::animal_profile(a.species).hp * hp_mul).max(2.0);
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
    mut juice: Juice,
    mut bystanders: Bystanders,
    mut commands: Commands,
    mut cues: MessageWriter<AudioCue>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut hero_q: Query<(&mut Hero, &HeroHealth)>,
    mut targets: Query<
        (
            Entity,
            &GlobalTransform,
            &mut Health,
            Option<&mut Ork>,
            Option<&mut Animal>,
            Option<&mut crate::combat_fx::HitSquash>,
        ),
        (
            Or<(With<Ork>, With<Animal>, With<crate::boss::Boss>)>,
            Without<crate::dying::Dying>,
        ),
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
    for (e, gt, mut hp, mut ork, animal, mut squash) in &mut targets {
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
        // Blood spray driven along the blow + a slash-flash at contact; a kill adds a ground
        // shockwave ring + a lingering splat under the corpse.
        let now_s = time.elapsed_secs();
        let mid = Vec3::new(p.x, p.y + 0.9, p.z);
        spawn_blood(&mut commands, &fx, mid, dir, dead);
        spawn_slash(&mut commands, &fx, &mut juice.materials, mid, now_s);
        // A blood splat under the target on EVERY hit (small + brief), big + lingering on a kill.
        spawn_splat(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y, p.z), dead, now_s);
        if dead {
            spawn_shockwave(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y + 0.05, p.z), now_s);
        }
        hit_any = true;
        // Floating number above the target + a white hurt-flash on a survivor.
        let head = Vec3::new(p.x, p.y + 2.2, p.z);
        if dead {
            floats.0.push(crate::combat_fx::FloatReq {
                world: head,
                text: "†".into(),
                color: crate::combat_fx::col_kill(),
                scale: 1.4,
            });
            killed_any = true;
            // Any kill bursts bounty gold + xp (as reward orbs) and feeds lifesteal. Orks pull
            // their bounty from `ork_config`; wild animals from `animal_profile`,
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
            // Fade out (crumple) instead of popping; the target query excludes Dying so it's
            // never re-hit / re-rewarded.
            crate::dying::begin_dying(&mut commands, e, time.elapsed_secs());
        } else {
            // Shove a surviving ork back along the blow (harder on a crit) + a springy recoil.
            if let Some(o) = ork.as_deref_mut() {
                o.kb = dir * if crit { KNOCKBACK_CRIT } else { KNOCKBACK };
                o.hit_recoil = now_s;
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
            commands.entity(e).try_insert(crate::combat_fx::HurtFlash::new(time.elapsed_secs()));
            // Springy body squash-and-stretch — re-kicked in place on rapid hits so the rest
            // scale captured by the first squash is never forgotten.
            match squash.as_deref_mut() {
                Some(s) => s.restart(now_s),
                None => {
                    commands.entity(e).try_insert(crate::combat_fx::HitSquash::new(now_s));
                }
            }
            // A struck (surviving) animal staggers back along the blow (harder on a crit) +
            // enrages — predators latch onto the hero (`Struck`).
            if let Some(mut an) = animal {
                an.kb = dir * if crit { KNOCKBACK_CRIT } else { KNOCKBACK };
                an.hit_recoil = now_s;
                commands.entity(e).try_insert(crate::wildlife::Struck { by: None });
            }
            // Warden boons: Frostbite chills (a crit freezes) and Venom poisons the struck foe.
            if player.0.frostbite {
                let (factor, dur) = if crit { (0.0, 1.0) } else { (0.45, 2.0) };
                commands.entity(e).try_insert(crate::boss::Slowed::new(now_s, factor, dur));
            }
            if player.0.venom {
                commands
                    .entity(e)
                    .try_insert(crate::boss::Poisoned { until: now_s + 4.0, dps: (base as f32 * 0.4).max(4.0) });
            }
        }
    }
    // ── Cleave pass: splash a fraction of the swing to OTHER orks near a struck target ──
    // (upgrade-gated — `cleave` is 0 until the Champion node is bought, so this is a no-op).
    if player.0.cleave > 0.0 && !struck.is_empty() {
        let splash = cleave_damage(dmg as f64, player.0.cleave) as f32;
        if splash > 0.0 {
            for (e, gt, mut hp, ork, _animal, mut squash) in &mut targets {
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
                let now_s = time.elapsed_secs();
                let mid = Vec3::new(p.x, p.y + 0.9, p.z);
                // Cleave splash has no single blow direction → radial blood puff.
                spawn_blood(&mut commands, &fx, mid, Vec2::ZERO, hp.hp <= 0.0);
                if hp.hp <= 0.0 {
                    spawn_shockwave(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y + 0.05, p.z), now_s);
                    spawn_splat(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y, p.z), true, now_s);
                    floats.0.push(crate::combat_fx::FloatReq {
                        world: head,
                        text: "†".into(),
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
                    crate::dying::begin_dying(&mut commands, e, time.elapsed_secs());
                } else {
                    floats.0.push(crate::combat_fx::FloatReq {
                        world: head,
                        text: format!("{}", splash as i32),
                        color: crate::combat_fx::col_ork_hit(),
                        scale: 0.85,
                    });
                    // No HurtFlash on cleave splash: one swing used to blink every nearby ork
                    // white at once, which read as scene-wide flicker. The float number, recoil
                    // and squash still mark splash victims; only the direct target flashes.
                    spawn_splat(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y, p.z), false, now_s);
                    if let Some(mut o) = ork {
                        o.hit_recoil = now_s;
                    }
                    match squash.as_deref_mut() {
                        Some(s) => s.restart(now_s),
                        None => {
                            commands.entity(e).try_insert(crate::combat_fx::HitSquash::new(now_s));
                        }
                    }
                }
            }
        }
    }

    // ── Bonking a townsperson: the same front-cone, but villagers aren't in the damage query, so
    // a swing that clips one does NO harm — it just lands a soft thud and earns a sarcastic earful.
    // Nearest bonked villager wins, so the line comes from whoever you actually clipped. ──
    let mut bonk: Option<(f32, Vec3)> = None;
    for gt in &bystanders.folk {
        let p = gt.translation();
        let to = Vec2::new(p.x - origin.x, p.z - origin.y);
        let dist = to.length();
        if dist > ATTACK_RANGE || dist < 1e-3 {
            continue;
        }
        if (to / dist).dot(fwd) < ATTACK_CONE_DOT {
            continue;
        }
        if bonk.is_none_or(|(bd, _)| dist < bd) {
            bonk = Some((dist, Vec3::new(p.x, p.y + 1.6, p.z)));
        }
    }
    if let Some((_, head)) = bonk {
        bystanders.speak.write(crate::audio::Speak::at(crate::audio::Concept::HitByHero, head));
        // A connecting bonk plays the impact thud + a light shake below, not the empty-swing whoosh.
        hit_any = true;
    }

    // Juice: a connecting blow shakes the screen, punches the FOV + briefly freezes the sim
    // (heavier on a kill).
    if killed_any {
        juice.feedback.trauma = (juice.feedback.trauma + SHAKE_KILL).min(1.0);
        crate::combat_fx::add_fov_kick(&mut juice.feedback, crate::combat_fx::FOV_KICK_KILL);
        juice.hitstop.remaining = juice.hitstop.remaining.max(HITSTOP_KILL);
    } else if hit_any {
        juice.feedback.trauma = (juice.feedback.trauma + SHAKE_HIT).min(1.0);
        crate::combat_fx::add_fov_kick(&mut juice.feedback, crate::combat_fx::FOV_KICK_HIT);
        juice.hitstop.remaining = juice.hitstop.remaining.max(HITSTOP_HIT);
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

/// Generic "pop" burst: fling `n` motes of an arbitrary `mesh`/`mat` out + up from `at` (they
/// arc, fall and shrink via [`update_sparks`]). Used by the apple-tree harvest pop in `verbs.rs`
/// so it reuses the spark physics without depending on the combat materials.
pub(crate) fn spawn_motes(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    at: Vec3,
    n: u32,
    spd: f32,
    scale0: f32,
    life: f32,
) {
    for i in 0..n {
        let a = i as f32 * 2.399_963_2; // golden angle → even spread
        let mag = 0.6 + ((i * 41 % 10) as f32) * 0.07;
        let up = 0.7 + (i % 3) as f32 * 0.4;
        let vel = Vec3::new(a.cos() * spd, up * spd * 0.7, a.sin() * spd) * mag;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(scale0)),
            Spark { vel, life, life0: life, scale0 },
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

/// Dark blood spray on a creature hit — motes flung mostly ALONG the blow `dir` (world XZ) +
/// spread + up, so the spray reads as driven by the strike. A kill gushes more, harder.
pub(crate) fn spawn_blood(commands: &mut Commands, fx: &CombatFx, at: Vec3, dir: Vec2, kill: bool) {
    let dir3 = Vec3::new(dir.x, 0.0, dir.y);
    let (n, spd, scale0) = if kill { (16u32, 3.4, 0.55) } else { (9, 2.4, 0.45) };
    for i in 0..n {
        let a = i as f32 * 2.399_963_2; // golden angle → even spread
        let spread = Vec3::new(a.cos(), 0.0, a.sin()) * 0.55;
        let up = 0.6 + (i % 3) as f32 * 0.45;
        let mag = 0.6 + (i * 37 % 10) as f32 * 0.06;
        let vel = (dir3 * 1.2 + spread + Vec3::Y * up) * spd * mag;
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.blood.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(scale0)),
            Spark { vel, life: 0.55, life0: 0.55, scale0 },
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Grey rock-chip burst off a mined ore boulder — small radial spray per swing, a bigger debris
/// puff on the shattering blow.
pub(crate) fn spawn_chips(commands: &mut Commands, fx: &CombatFx, at: Vec3, shatter: bool) {
    let (n, spd, scale0, life) = if shatter { (14u32, 3.2, 0.6, 0.5) } else { (6, 2.2, 0.4, 0.4) };
    for i in 0..n {
        let a = i as f32 * 2.399_963_2;
        let mag = 0.6 + (i * 41 % 10) as f32 * 0.06;
        let up = 0.5 + (i % 3) as f32 * 0.4;
        let vel = Vec3::new(a.cos() * spd, up * spd * 0.7, a.sin() * spd) * mag;
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.chip.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(scale0)),
            Spark { vel, life, life0: life, scale0 },
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Spawn one planar flash: clone `base` so it fades alone, lay `mesh` at `pose`, scale `s0→s1`
/// and fade from `a0` over `life`, orienting per `face` each frame.
#[allow(clippy::too_many_arguments)]
fn spawn_fade(
    commands: &mut Commands,
    mats: &mut Assets<StandardMaterial>,
    base: &Handle<StandardMaterial>,
    mesh: &Handle<Mesh>,
    pose: Transform,
    s0: Vec3,
    s1: Vec3,
    a0: f32,
    life: f32,
    face: FadeFace,
    now: f32,
) {
    let Some(m) = mats.get(base).cloned() else { return };
    let mat = mats.add(m);
    commands.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(mat.clone()),
        pose.with_scale(s0),
        bevy::light::NotShadowCaster,
        crate::biome::BiomeEntity,
        FxFade { born: now, life, mat, s0, s1, a0, face },
    ));
}

/// A subtle slash glint at the contact point — a thin, camera-facing streak that widens slightly
/// and fades fast, marking where the blade connected without flaring the screen.
pub(crate) fn spawn_slash(commands: &mut Commands, fx: &CombatFx, mats: &mut Assets<StandardMaterial>, at: Vec3, now: f32) {
    spawn_fade(
        commands, mats, &fx.slash_mat, &fx.quad,
        Transform::from_translation(at),
        Vec3::new(0.35, 0.09, 1.0), Vec3::new(1.05, 0.13, 1.0), 0.5, 0.11, FadeFace::Camera, now,
    );
}

/// An expanding ground ring on a kill — flat shockwave that rushes out + fades.
pub(crate) fn spawn_shockwave(commands: &mut Commands, fx: &CombatFx, mats: &mut Assets<StandardMaterial>, at: Vec3, now: f32) {
    let pose = Transform::from_translation(at + Vec3::Y * 0.05)
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    spawn_fade(commands, mats, &fx.ring_mat, &fx.ring, pose, Vec3::splat(0.4), Vec3::splat(1.6), 0.45, 0.22, FadeFace::Keep, now);
}

/// A dark blood splat on the ground under a struck creature — fades slowly so a fight leaves a
/// brief trail of where blows landed. Bigger on a kill.
pub(crate) fn spawn_splat(commands: &mut Commands, fx: &CombatFx, mats: &mut Assets<StandardMaterial>, at: Vec3, kill: bool, now: f32) {
    let pose = Transform::from_translation(Vec3::new(at.x, at.y + 0.03, at.z))
        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    // A kill leaves a big, long-lived pool; a glancing hit just flecks the ground briefly.
    let (size, life, a0) = if kill { (1.4, 3.5, 0.72) } else { (0.55, 1.6, 0.6) };
    let s = Vec3::splat(size);
    spawn_fade(commands, mats, &fx.splat_mat, &fx.disc, pose, s, s, a0, life, FadeFace::Keep, now);
}

/// Emit a translucent ribbon along the sword tip's path across the fast part of the swing — a
/// cheap weapon trail. Each frame stretches one quad between the previous and current tip
/// positions ([`FadeFace::Axis`] keeps it rolled toward the camera), so the segments chain into
/// one continuous smear that follows the real sweep — instead of the old fixed-size camera-facing
/// cards popping at the tip, which read as white flashes. The blade tip sits at `ArmR`-local
/// `(0,-0.5,0.96)` (the baked sword's cone), so the arm's `GlobalTransform` places it in the world.
pub fn hero_blade_trail(
    time: Res<Time>,
    fx: Option<Res<CombatFx>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut last_tip: Local<Option<Vec3>>,
    hero_q: Query<&Hero>,
    parts: Query<(&HeroPart, &GlobalTransform)>,
) {
    let Some(fx) = fx else { return };
    let Ok(hero) = hero_q.single() else { return };
    let phase = if hero.attacking { hero.attack_t / ATTACK_DURATION } else { -1.0 };
    if !(0.25..0.55).contains(&phase) {
        *last_tip = None; // only across the fast sweep, where the blade actually whips through
        return;
    }
    let Some((_, gt)) = parts.iter().find(|(p, _)| p.limb == HeroLimb::ArmR) else { return };
    let tip = gt.transform_point(Vec3::new(0.0, -0.5, 0.96));
    let prev = last_tip.replace(tip);
    let Some(prev) = prev else { return }; // first sweep frame: just record the anchor
    let seg = tip - prev;
    let len = seg.length();
    if len < 1e-3 {
        return;
    }
    let axis = seg / len;
    spawn_fade(
        &mut commands, &mut materials, &fx.trail_mat, &fx.quad,
        // Rough spawn-frame alignment; FadeFace::Axis re-rolls it toward the camera every frame.
        Transform::from_translation(prev.midpoint(tip))
            .with_rotation(Quat::from_rotation_arc(Vec3::X, axis)),
        // Slight overlap (×1.15) hides the seams between consecutive segments; the ribbon fades
        // by THINNING (height → ~0) rather than ballooning, so it dissolves instead of flashing.
        Vec3::new(len * 1.15, 0.055, 1.0), Vec3::new(len * 1.15, 0.012, 1.0), 0.28, 0.18,
        FadeFace::Axis(axis), time.elapsed_secs(),
    );
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
