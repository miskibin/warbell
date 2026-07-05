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
use super::{FirstPerson, Hero, HeroHealth, PlayMode, PlayerRes};

pub const ATTACK_DURATION: f32 = 0.45;
const ATTACK_RANGE: f32 = 1.8;
const ATTACK_CONE_DOT: f32 = 0.5; // cos 60° — front cone half-angle
const HIT_PHASE: f32 = 0.3; // fraction into the swing where damage lands

// ── Attack aim-assist ──
// When a swing starts, the hero *gently* leans his facing toward the nearest enemy so a swing reads
// as aimed at the foe instead of sideways (the "turns his side to whoever he's hitting" bug). It's a
// soft assist, NOT a lock: the turn is slower than the move-steer, which stays live, so the player's
// own input always overpowers it — standing still and swinging tidies your aim; actively strafing
// keeps your heading. Never a hard snap, never freezes control.
const LOCK_RANGE: f32 = 3.0; // a touch past ATTACK_RANGE so a circled foe is grabbed before you're on top
const LOCK_DOT: f32 = -0.2; // ~200° front arc — leans toward a foe at your side, but never to your back
const LOCK_TURN_RATE: f32 = 6.0; // rad/s — a measured lean (was 9: read as a sudden snap), still under the 15 move-steer so input wins

// ── Combo chain (the Witcher 1-2-3 flow) ──
// Successive swings chained inside the window step through the three studio clips in a fixed
// sequence (overhead → slash → thrust) instead of rolling one at random: each later step whips
// out faster and hits harder, so mashing with rhythm *flows* — and a mid-swing press is buffered
// (`Hero::queued`) so the chain never drops an input. A gap, a roll or a Heavy resets to step 1.
/// Seconds after a swing completes within which the next press continues the chain.
const COMBO_WINDOW: f32 = 0.9;
/// Per-step swing-duration multiplier — steps 2/3 are snappier.
const COMBO_DUR: [f32; 3] = [1.0, 0.86, 0.78];
/// Per-step damage multiplier — the chain rewards staying on the offense.
const COMBO_DMG: [f64; 3] = [1.0, 1.12, 1.28];

// ── Attack magnetism (gap-closer) ──
// A swing that starts with the committed target a step or two out of reach GLIDES the hero onto
// it across the wind-up (the Witcher attack-step): budgeted at swing start from the target's
// range, spent at a fixed rate while the blade winds, halted at sword's length / a wall / a
// terrace lip. Kills the "whiffing at air just out of range" feel; third-person only (in FP the
// view owns the body — a hidden glide underfoot reads as motion sickness).
/// Engage when the target stands within this range at swing start (past it you're not "almost
/// in reach", you're closing — walk). Playtest-tuned DOWN from 4.6/2.8/14: the first cut yanked
/// the hero across a third of the screen — "too aggressive". Now it's a step, not a launch.
const LUNGE_ENGAGE: f32 = 3.2;
/// …and stop the glide at this range — sword's length, matching the keep-out shove.
const LUNGE_STOP: f32 = 1.35;
/// Hard cap on glide distance per swing.
const LUNGE_CAP: f32 = 1.6;
/// Glide speed (units/s) — ~3× walk, arrives before the blade lands without reading as a yank.
const LUNGE_SPEED: f32 = 10.0;
/// Swing phase past which the glide cuts (the blade has landed; recovery never slides).
const LUNGE_END_PHASE: f32 = 0.55;

// ── Riposte ──
/// Seconds after a timed parry (see `health`) in which the next swing is the RIPOSTE — a
/// guaranteed-crit counter-thrust at ×[`RIPOSTE_MULT`] damage.
pub(crate) const RIPOSTE_WINDOW: f32 = 1.1;
const RIPOSTE_MULT: f64 = 2.2;

/// Nearest live enemy within [`LOCK_RANGE`] and the front [`LOCK_DOT`] arc, as `(facing angle,
/// distance)`. Picks the closest so a knight in a melee commits to the foe he's actually next to.
fn lock_target<'a>(
    gts: impl Iterator<Item = &'a GlobalTransform>,
    origin: Vec2,
    facing: f32,
) -> Option<(f32, f32)> {
    let fwd = Vec2::new(facing.sin(), facing.cos());
    let mut best: Option<(f32, f32)> = None; // (dist², angle)
    for gt in gts {
        let p = gt.translation();
        let to = Vec2::new(p.x - origin.x, p.z - origin.y);
        let d2 = to.length_squared();
        if d2 > LOCK_RANGE * LOCK_RANGE || d2 < 1e-6 {
            continue;
        }
        let dir = to / d2.sqrt();
        if dir.dot(fwd) < LOCK_DOT {
            continue; // behind you — don't snap-spin onto it mid-swing
        }
        if best.map_or(true, |(bd, _)| d2 < bd) {
            best = Some((d2, dir.x.atan2(dir.y)));
        }
    }
    best.map(|(d2, a)| (a, d2.sqrt()))
}

/// The swing's committed aim `(facing angle, distance)`: the SOFT-LOCK target when one is ringed
/// (so the blow lands on what the ring shows), else the nearest-in-arc pick (e.g. wildlife, which
/// the soft-lock deliberately ignores). Drives both the facing lean and the gap-closer budget.
fn aim_pick<'a>(
    gts: impl Iterator<Item = &'a GlobalTransform>,
    hero: &Hero,
) -> Option<(f32, f32)> {
    if let Some(tp) = hero.soft_pos {
        let to = tp - hero.pos;
        let d = to.length();
        if d > 1e-3 {
            return Some((to.x.atan2(to.y), d));
        }
    }
    lock_target(gts, hero.pos, hero.facing)
}

/// Start a swing (fresh press, buffered chain step, or the charged Heavy): stamps the combo step
/// + its speed/variant, commits the aim lean, arms the gap-closer glide, and consumes a pending
/// riposte. `fp` = first-person (view owns facing → no lean, no glide).
fn begin_swing(hero: &mut Hero, heavy: bool, aim: Option<(f32, f32)>, now: f32, fp: bool) {
    hero.attacking = true;
    hero.attack_t = 0.0;
    hero.hit_dealt = false;
    hero.queued = false;
    hero.heavy = heavy;
    hero.lock_face = if fp { None } else { aim.map(|(a, _)| a) };
    // A timed parry armed the counter: the very next (light) swing is the riposte.
    hero.riposte = !heavy && now < hero.riposte_until;
    hero.riposte_until = 0.0;
    if heavy {
        hero.attack_variant = HEAVY_VARIANT;
        hero.attack_dur = ATTACK_DURATION;
        // A Heavy is its own statement — the chain restarts after it.
        hero.combo = 0;
        hero.combo_until = 0.0;
    } else {
        hero.combo = if now <= hero.combo_until { (hero.combo + 1) % 3 } else { 0 };
        if hero.riposte {
            hero.combo = 2; // the riposte IS the counter-thrust (step 3's clip + snap)
        }
        hero.attack_variant = hero.combo;
        hero.attack_dur = ATTACK_DURATION * COMBO_DUR[hero.combo as usize];
    }
    hero.lunge_left = match aim {
        Some((_, d)) if !fp && d <= LUNGE_ENGAGE && d > LUNGE_STOP => (d - LUNGE_STOP).min(LUNGE_CAP),
        _ => 0.0,
    };
}

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

// Hit-stop + shake are TIERED by blow weight so a light poke, a crit, and a kill each FEEL
// distinct (they were all one weight before). Kill > crit > light, in both freeze length and
// camera trauma. The light hit is lighter than the old single value; the kill a touch shorter so
// the new crit tier sits cleanly between them.
// The tiers are `pub(crate)` (re-exported via `player`) so `combat_fx::hit_test` can mirror the
// real hit-site juice exactly — FOREST_HITTEST capture footage must not under-sell the game feel.
pub(crate) const HITSTOP_KILL: f32 = 0.10;
pub(crate) const HITSTOP_CRIT: f32 = 0.07;
pub(crate) const HITSTOP_HIT: f32 = 0.04;
/// During hit-stop the sim runs at this fraction of real time rather than a dead freeze — a brief
/// slow-mo dip that still sells the "punch" without the full-stop stutter that read as micro-lag.
const HITSTOP_SLOWMO: f32 = 0.15;
pub(crate) const SHAKE_KILL: f32 = 0.46;
pub(crate) const SHAKE_CRIT: f32 = 0.32;
pub(crate) const SHAKE_HIT: f32 = 0.20;
pub(crate) const KNOCKBACK: f32 = 6.0;
pub(crate) const KNOCKBACK_CRIT: f32 = 9.0;
/// A crit/heavy blow STAGGERS the ork: its attack is interrupted and its next strike pushed out by
/// this long (s). Reuses the existing `atk_cd` gate (honoured by both the camp and siege brains) so
/// a hard hit visibly stops the ork's offense — the "you felt that" beat — with no new AI state.
const STAGGER_CD: f32 = 0.55;
/// Per-blow-weight juice: (hurt-flash peak intensity, squash amplitude). Light poke → crit → heavy.
/// The flash pops far harder and the body gives more as the blow gets heavier, so the three tiers
/// read as distinct hits instead of one flat reaction.
const JUICE_HEAVY: (f32, f32) = (0.95, 0.17);
const JUICE_CRIT: (f32, f32) = (0.65, 0.14);
const JUICE_HIT: (f32, f32) = (0.34, 0.10);

// ── Charged Heavy Strike ────────────────────────────────────────────────────────────────────────
// Holding LMB past `CHARGE_THRESHOLD` and releasing unleashes a guaranteed-crit Heavy Strike — a
// SECOND, separate swing fired ~0.8s after the light one (so it never clunks into a double). A quick
// tap is unchanged: the light swing still fires on press, and the charge stays invisible below
// `CHARGE_GRACE`. The 0.8s wind-up + slowed feet (`CHARGE_MOVE_MULT`) + a small stamina cost are the
// price. See `player::charge` (the bar UI) and `anim::charge_stance` / `anim::heavy_chop`.
/// Hold time (s) required to qualify the release as a Heavy Strike.
pub(crate) const CHARGE_THRESHOLD: f32 = 0.8;
/// Below this hold time (s) the charge is invisible — a normal tap never flashes the bar or slows.
pub(crate) const CHARGE_GRACE: f32 = 0.2;
/// Move-speed multiplier while charging (read by `movement::player_move`).
pub(crate) const CHARGE_MOVE_MULT: f32 = 0.4;
/// `attack_variant` sentinel for the heavy swing (the light swing rolls 0..=2; `anim` maps this to
/// `heavy_chop`).
pub(crate) const HEAVY_VARIANT: u8 = 3;
/// Heavy = guaranteed crit at this multiple of the swing's base damage.
const HEAVY_MULT: f64 = 3.0;
/// Stamina spent the instant a heavy fires (small — the wind-up is the real cost). Drawn from the
/// same pool block/arts use, so spamming heavies trades against blocking. Exposed for the bar UI's
/// "can I afford it" grey-out.
pub(crate) const HEAVY_STAMINA_COST: f32 = 30.0;
/// Heavy juice tier — a notch ABOVE a kill (`HITSTOP_KILL`/`SHAKE_KILL`) so the payoff lands hard.
pub(crate) const HITSTOP_HEAVY: f32 = 0.14;
pub(crate) const SHAKE_HEAVY: f32 = 0.6;
pub(crate) const FOV_KICK_HEAVY: f32 = 2.4;

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
    /// Active run's difficulty — drives the Easy hero-damage handicap.
    siege: Option<Res<'w, crate::siege::Siege>>,
    /// First-person flag — folded in here (not a standalone `Res`) so `player_attack` stays under
    /// Bevy's 16-param ceiling. In FP the view owns facing, so the lock-on facing-snap is skipped.
    fp: Res<'w, FirstPerson>,
}

/// Townsfolk the hero can harmlessly bonk with a swing: the [`crate::villagers::Villager`] bodies
/// (which are NOT in the damage-dealing `targets` query) plus the voice channel to make them yelp.
/// Bundled into one [`SystemParam`] so [`player_attack`] stays under Bevy's 16-param ceiling.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Bystanders<'w, 's> {
    speak: MessageWriter<'w, crate::audio::Speak>,
    // The rival stronghold's desert garrison + workers + raiders carry `Villager` (for animation)
    // but are NOT our townsfolk — exclude them so bonking a rival peasant doesn't earn OUR
    // peasants' sarcastic earful (`Concept::HitByHero`). They're "not ours".
    folk: Query<
        'w,
        's,
        &'static GlobalTransform,
        (
            With<crate::villagers::Villager>,
            Without<crate::dying::Dying>,
            Without<crate::rival::RivalWorker>,
            Without<crate::rival::RivalSoldier>,
            Without<crate::rival::RivalRaider>,
        ),
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
    /// Elongate the mote along its velocity (a streak, not a ball) — metal glints + blood
    /// droplets read as motion this way; dust/leaves/chips stay round and tumble.
    stretch: bool,
    /// Gravity (units/s²) — 9 for the standard arc; low (~2.5) for hanging embers.
    grav: f32,
}

impl Spark {
    fn new(vel: Vec3, life: f32, scale0: f32) -> Self {
        Spark { vel, life, life0: life, scale0, stretch: false, grav: 9.0 }
    }
    fn streak(vel: Vec3, life: f32, scale0: f32) -> Self {
        Spark { stretch: true, ..Spark::new(vel, life, scale0) }
    }
}

/// A brief impact light — a point flash that decays over `life` then despawns. Sells the metal
/// "spark" of a heavy blow / kill / parry against the scene's real lighting (cheap: no shadows,
/// short range, one at a time in practice).
#[derive(Component)]
pub(crate) struct LightFade {
    born: f32,
    life: f32,
    peak: f32,
}

/// Spawn one impact flash at `at`. Intensity in lumens (scene torches run 18–95k; a flash reads
/// at ~15–30k over range ~6).
pub(crate) fn spawn_impact_light(commands: &mut Commands, at: Vec3, color: Color, peak: f32, life: f32, now: f32) {
    commands.spawn((
        PointLight {
            color,
            intensity: peak,
            range: 6.0,
            radius: 0.1,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(at),
        LightFade { born: now, life, peak },
    ));
}

/// Decay + despawn the impact flashes (quadratic fall-off so the pop is front-loaded).
pub fn update_light_fades(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &LightFade, &mut PointLight)>,
) {
    let now = time.elapsed_secs();
    for (e, f, mut pl) in &mut q {
        let k = (now - f.born) / f.life;
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        pl.intensity = f.peak * (1.0 - k) * (1.0 - k);
    }
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

/// A small tan dust scuff (dodge-roll kick-off) — ground motes only, no flash/glow.
pub(crate) fn spawn_dust_puff(commands: &mut Commands, fx: &CombatFx, at: Vec3, n: u32) {
    spawn_motes(commands, &fx.mesh, &fx.dust, at, n, 1.2, 0.42, 0.35);
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
            Spark::new(vel, 0.55, 0.55),
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
        if let Some(mut m) = mats.get_mut(&f.mat) {
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
    mut hero_q: Query<(&mut Hero, &mut HeroHealth, &mut Transform), Without<Camera3d>>,
    mut targets: Query<
        (
            Entity,
            &GlobalTransform,
            &mut Health,
            Option<&mut Ork>,
            Option<&mut Animal>,
            Option<&mut crate::combat_fx::HitSquash>,
            Option<&crate::rival::RivalSoldier>,
            Option<&crate::rival::RivalWorker>,
        ),
        (
            Or<(
                With<Ork>,
                With<Animal>,
                With<crate::boss::Boss>,
                With<crate::warlord::Warlord>,
                With<crate::rival::RivalSoldier>,
                With<crate::rival::RivalWorker>,
                With<crate::snowman::Snowman>,
            )>,
            Without<crate::dying::Dying>,
        ),
    >,
) {
    let Ok((mut hero, mut hh, mut hero_tf)) = hero_q.single_mut() else { return };
    if *mode != PlayMode::Play || !player.0.is_alive() || hero.roll_t >= 0.0 {
        // Out of play, down, or mid dodge-roll (the roll cancelled the swing on arm).
        hero.attacking = false;
        hero.charge_t = -1.0;
        hero.lock_face = None;
        hero.queued = false;
        hero.lunge_left = 0.0;
        return;
    }
    let dt = time.delta_secs();
    let now_s = time.elapsed_secs();

    // Charging is gated on the same conditions as swinging: cursor locked (actually playing) and not
    // guarding (the shield takes priority).
    let can_act = orbit.locked && !hh.blocking;

    // ── Presses: start a swing, or buffer the chain ─────────────────────────────────────────────
    // A press with no swing in flight starts one (aimed at the soft-lock/nearest foe — the O(n)
    // scan runs only at swing starts, never per-frame). A press DURING a swing is buffered
    // (`queued`) and fires the instant this swing ends, so mashing chains the combo fluidly
    // instead of dropping inputs. Every press also arms the Heavy charge.
    if can_act && buttons.just_pressed(MouseButton::Left) {
        if !hero.attacking {
            let aim = if juice.fp.active { None } else { aim_pick(targets.iter().map(|t| t.1), &hero) };
            begin_swing(&mut hero, false, aim, now_s, juice.fp.active);
            // Only the exertion grunt fires on the wind-up (and only ~34% of the time — voice gates
            // it). The whoosh is DEFERRED to hit resolution so a connecting blow plays its impact
            // alone and a whiff plays the whoosh alone — never both (Character.tsx).
            cues.write(AudioCue::HeroGruntSwing);
        } else {
            hero.queued = true; // buffered — the chain continues the frame this swing ends
        }
        hero.charge_t = 0.0; // arm the charge from this press (>= 0.0 = charging)
    }
    // ── Heavy-Strike charge on the LMB hold ──────────────────────────────────────────────────────
    // Holding past `CHARGE_THRESHOLD` and releasing fires a separate Heavy swing. Since 0.8s > the
    // 0.45s swing, the heavy never overlaps the light — they read as a light→heavy combo.
    // While armed (`charge_t >= 0.0`): build the hold, or resolve it on release / loss of control.
    if hero.charge_t >= 0.0 {
        if buttons.pressed(MouseButton::Left) && can_act {
            hero.charge_t += dt;
        } else {
            // Released (or can no longer act) — fire the Heavy if held long enough and affordable.
            if buttons.just_released(MouseButton::Left)
                && can_act
                && hero.charge_t >= CHARGE_THRESHOLD
                && hh.stamina >= HEAVY_STAMINA_COST
            {
                hh.stamina -= HEAVY_STAMINA_COST;
                let aim = if juice.fp.active { None } else { aim_pick(targets.iter().map(|t| t.1), &hero) };
                begin_swing(&mut hero, true, aim, now_s, juice.fp.active);
                cues.write(AudioCue::HeroGruntSwing);
                cues.write(AudioCue::Slam); // a beefier release whoosh for the heavy wind-up
            }
            hero.charge_t = -1.0; // disarm (under threshold, unaffordable, or lost control)
        }
    }
    if !hero.attacking {
        return;
    }
    // Gently lean toward the locked foe across the swing (third-person). This is a soft assist, not a
    // lock — the move-steer in `player_move` stays live and turns faster, so active input overrides
    // this; with no input (standing to swing) it quietly tidies the aim onto the target.
    if let Some(target) = hero.lock_face {
        hero.facing = super::movement::lerp_angle(hero.facing, target, (dt * LOCK_TURN_RATE).min(1.0));
    }
    hero.attack_t += dt;
    let phase = hero.attack_t / hero.attack_dur;

    // ── Attack magnetism: STEP onto the committed foe across the wind-up ──
    // The budget armed at swing start is spent by INJECTING VELOCITY, not by teleporting the
    // root: `player_move` integrates it next frame through the normal locomotion (collision,
    // step rules, gait/`moving_amt`, footsteps), so the close reads as the hero lunging a real
    // stride into the blow — legs driving — instead of a magic slide on frozen feet.
    if hero.lunge_left > 0.0 {
        let arrived =
            phase >= LUNGE_END_PHASE || hero.soft_pos.is_some_and(|tp| tp.distance(hero.pos) <= LUNGE_STOP);
        if arrived {
            hero.lunge_left = 0.0;
        } else {
            let dir = Vec2::new(hero.facing.sin(), hero.facing.cos());
            let speed = LUNGE_SPEED.min(hero.lunge_left / dt.max(1e-3));
            hero.vel = dir * speed;
            hero.lunge_left -= speed * dt;
        }
    }
    // Keep the aim-lean rendered this frame (movement wrote the pre-lean yaw earlier in the
    // chain; without this the body lags the lock-lean by a frame mid-swing).
    hero_tf.rotation = Quat::from_rotation_y(hero.facing);

    if phase >= 1.0 {
        hero.attacking = false;
        hero.heavy = false; // swing done — the next light swing must not inherit the heavy tag
        hero.riposte = false;
        hero.lock_face = None; // release the facing lock so move-steer takes over again
        hero.lunge_left = 0.0;
        hero.combo_until = now_s + COMBO_WINDOW; // the chain clock runs from the swing's settle
        // A buffered press chains the next combo step immediately — no dropped inputs.
        if hero.queued && can_act {
            let aim = if juice.fp.active { None } else { aim_pick(targets.iter().map(|t| t.1), &hero) };
            begin_swing(&mut hero, false, aim, now_s, juice.fp.active);
            cues.write(AudioCue::HeroGruntSwing);
        } else {
            hero.queued = false;
        }
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
    // Difficulty handicap: Easy gives the hero extra melee punch on top of everything else.
    let diff = juice.siege.as_ref().map(|s| s.difficulty).unwrap_or(crate::siege::Difficulty::Normal);
    let dmg_mul = crate::siege::mods_for(diff).hero_dmg_mul as f64;
    // Combo steps 2/3 hit harder (variant == combo step for light swings; a Heavy skips it).
    let combo_mult = if hero.heavy { 1.0 } else { COMBO_DMG[hero.attack_variant.min(2) as usize] };
    let base =
        (player.0.attack_damage + mods.weapon_bonus()) * mods.power_mult(now) * dmg_mul * combo_mult;
    // Broadcast the cone so ore/dummies share this swing (non-crit damage).
    mods.publish_swing(origin, fwd, base.round() as f32);
    // A charged Heavy is a GUARANTEED crit at ×HEAVY_MULT; a RIPOSTE (the counter-thrust a timed
    // parry buys) a guaranteed crit at ×RIPOSTE_MULT — the `crit` flag below then drives the crit
    // knockback/float/freeze/juice. A normal swing rolls its crit as before.
    let (dmg_f, crit) = if hero.heavy {
        (base * HEAVY_MULT, true)
    } else if hero.riposte {
        (base * RIPOSTE_MULT, true)
    } else {
        roll_crit(base, player.0.crit_chance, rng.unit())
    };
    let dmg = dmg_f.round() as f32;
    let bounty_mult = player.0.bounty_mult;
    let lifesteal = player.0.lifesteal;

    let mut hit_any = false;
    let mut killed_any = false;
    // Direct-hit bookkeeping for the cleave pass (positions to splash from + who's already hit).
    let mut struck: Vec<Vec2> = Vec::new();
    let mut hit_ents: Vec<Entity> = Vec::new();
    for (e, gt, mut hp, mut ork, animal, mut squash, rival, worker) in &mut targets {
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
        // Heavy flourish: an extra ground shockwave ring + a bright spark gout on every heavy
        // contact (cosmetic only — the heavy is a single-target hit, no splash damage).
        if hero.heavy {
            spawn_shockwave(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y + 0.05, p.z), now_s);
            spawn_burst(&mut commands, &fx, mid, true);
            spawn_impact_light(&mut commands, mid, Color::srgb(1.0, 0.85, 0.5), 26_000.0, 0.18, now_s);
        }
        // A blood splat under the target on EVERY hit (small + brief), big + lingering on a kill.
        spawn_splat(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y, p.z), dead, now_s);
        if dead {
            spawn_shockwave(&mut commands, &fx, &mut juice.materials, Vec3::new(p.x, p.y + 0.05, p.z), now_s);
            spawn_impact_light(&mut commands, mid, Color::srgb(1.0, 0.9, 0.6), 18_000.0, 0.15, now_s);
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
            } else if rival.is_some() {
                // A felled rival soldier pays a tough-ork-tier bounty (gold scales with the boon).
                ((crate::rival::SOLDIER_BOUNTY_GOLD as f64 * bounty_mult).round() as i64, crate::rival::SOLDIER_BOUNTY_XP)
            } else if worker.is_some() {
                // A cut-down rival labourer pays only a token peasant bounty.
                ((crate::rival::WORKER_BOUNTY_GOLD as f64 * bounty_mult).round() as i64, crate::rival::WORKER_BOUNTY_XP)
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
            // never re-hit / re-rewarded. Directed by the blow so the corpse topples + lurches the
            // way you struck it (a heavy throws it harder) — the kill money-shot.
            crate::dying::begin_dying_struck(&mut commands, e, now_s, dir, hero.heavy);
        } else {
            // Blow-weight tier drives the whole reaction (flash pop, squash give, stagger, spark).
            let (flash_i, sq_amp) = if hero.heavy { JUICE_HEAVY } else if crit { JUICE_CRIT } else { JUICE_HIT };
            // Soft wildlife bounces (springy); armoured orks ABSORB (no cartoon ring).
            let springy = animal.is_some();
            // Shove a surviving ork back along the blow (harder on a crit) + a springy recoil.
            if let Some(o) = ork.as_deref_mut() {
                o.kb = dir * if crit { KNOCKBACK_CRIT } else { KNOCKBACK };
                o.hit_recoil = now_s;
                // Crit/heavy STAGGERS: cancel any wind-up (zero the strike anim) and push the next
                // strike out, so a hard blow visibly interrupts the ork instead of trading evenly.
                if crit || hero.heavy {
                    o.atk_anim = 0.0;
                    o.atk_cd = o.atk_cd.max(STAGGER_CD);
                }
            }
            // A contact spark gout on a crit — metal-on-ork crunch at the hit point, on TOP of the
            // per-hit blood + slash-flash. (A heavy already fired its bigger gout above; light
            // pokes stay clean.)
            if crit && !hero.heavy {
                spawn_burst(&mut commands, &fx, mid, false);
            }
            // A heavy reads as "{dmg}!!" big in gold; a lucky crit "{dmg}!"; a normal hit the plain
            // number.
            let (text, color, fscale) = if hero.heavy {
                (format!("{}!!", dmg as i32), crate::combat_fx::col_kill(), 1.5)
            } else if crit {
                (format!("{}!", dmg as i32), crate::combat_fx::col_kill(), 1.2)
            } else {
                (format!("{}", dmg as i32), crate::combat_fx::col_ork_hit(), 1.0)
            };
            floats.0.push(crate::combat_fx::FloatReq { world: head, text, color, scale: fscale });
            commands.entity(e).try_insert(crate::combat_fx::HurtFlash::new(now_s, flash_i));
            // Body give — absorb (ork) or bounce (wildlife). Re-kicked in place on rapid hits so the
            // rest scale captured by the first squash is never forgotten.
            match squash.as_deref_mut() {
                Some(s) => s.restart(now_s, sq_amp),
                None => {
                    commands.entity(e).try_insert(crate::combat_fx::HitSquash::new(now_s, sq_amp, springy));
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
    // Landing a blow on an enemy opens/refreshes the IN-COMBAT window (drives the stance).
    if !struck.is_empty() {
        hero.combat_until = now_s + super::COMBAT_LINGER;
    }

    // ── Cleave pass: splash a fraction of the swing to OTHER orks near a struck target ──
    // (upgrade-gated — `cleave` is 0 until the Champion node is bought, so this is a no-op).
    if player.0.cleave > 0.0 && !struck.is_empty() {
        let splash = cleave_damage(dmg as f64, player.0.cleave) as f32;
        if splash > 0.0 {
            for (e, gt, mut hp, ork, _animal, mut squash, _rival, _worker) in &mut targets {
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
                    // Cleave splash only hits orks → absorb (not springy), light amplitude.
                    match squash.as_deref_mut() {
                        Some(s) => s.restart(now_s, JUICE_HIT.1),
                        None => {
                            commands.entity(e).try_insert(crate::combat_fx::HitSquash::new(now_s, JUICE_HIT.1, false));
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
        // A clipped townsperson only earns the sarcastic earful when it's a deliberate poke — NOT
        // collateral mid-battle. If any ork is near the hero we're in a fight, so stay quiet; an
        // accidental bonk between sword-strokes shouldn't make the peasant quip over the screaming.
        const BATTLE_QUIET_R2: f32 = 14.0 * 14.0;
        let in_battle = targets.iter().any(|(_, gt, _, ork, _, _, _, _)| {
            ork.is_some() && {
                let p = gt.translation();
                Vec2::new(p.x - origin.x, p.z - origin.y).length_squared() < BATTLE_QUIET_R2
            }
        });
        if !in_battle {
            bystanders.speak.write(crate::audio::Speak::at(crate::audio::Concept::HitByHero, head));
        }
        // A connecting bonk plays the impact thud + a light shake below, not the empty-swing whoosh.
        hit_any = true;
    }

    // Juice: a connecting blow shakes the screen, punches the FOV + briefly freezes the sim,
    // TIERED by weight — kill > crit > light. The shake is also biased to RECOIL: `shake_dir`
    // = `-fwd` kicks the camera back along the swing (a directed jolt, not pure chaos). `crit`
    // is the swing-wide roll (one per swing, line ~419), so any connecting hit on a crit swing
    // gets the crit tier; a bonked townsperson (hit_any, never crit/kill) takes the light tier.
    if hero.heavy && hit_any {
        // Top tier — a landed Heavy hits harder than any kill: longest freeze, biggest shake/punch.
        juice.feedback.shake_dir = -fwd;
        juice.feedback.trauma = (juice.feedback.trauma + SHAKE_HEAVY).min(1.0);
        crate::combat_fx::add_fov_kick(&mut juice.feedback, FOV_KICK_HEAVY);
        juice.hitstop.remaining = juice.hitstop.remaining.max(HITSTOP_HEAVY);
    } else if killed_any {
        juice.feedback.shake_dir = -fwd;
        juice.feedback.trauma = (juice.feedback.trauma + SHAKE_KILL).min(1.0);
        crate::combat_fx::add_fov_kick(&mut juice.feedback, crate::combat_fx::FOV_KICK_KILL);
        juice.hitstop.remaining = juice.hitstop.remaining.max(HITSTOP_KILL);
    } else if hit_any && crit {
        juice.feedback.shake_dir = -fwd;
        juice.feedback.trauma = (juice.feedback.trauma + SHAKE_CRIT).min(1.0);
        crate::combat_fx::add_fov_kick(&mut juice.feedback, crate::combat_fx::FOV_KICK_CRIT);
        juice.hitstop.remaining = juice.hitstop.remaining.max(HITSTOP_CRIT);
    } else if hit_any {
        juice.feedback.shake_dir = -fwd;
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
            Spark::new(vel, 0.5, 0.6),
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
            Spark::new(vel, life, scale0),
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
            Spark::streak(vel, 0.45, scale0),
            bevy::light::NotShadowCaster,
        ));
    }
    // A kill also sheds a few slow EMBERS — small hot motes on low gravity that hang and drift a
    // beat after the burst is gone, so the payoff lingers instead of vanishing in half a second.
    if kill {
        for i in 0..5u32 {
            let a = i as f32 * 2.399_963_2 + 0.9;
            let vel = Vec3::new(a.cos() * 0.9, 1.3 + (i % 3) as f32 * 0.35, a.sin() * 0.9);
            commands.spawn((
                Mesh3d(fx.mesh.clone()),
                MeshMaterial3d(fx.hit.clone()),
                Transform::from_translation(at).with_scale(Vec3::splat(0.45)),
                Spark { grav: 2.2, ..Spark::new(vel, 1.0, 0.45) },
                bevy::light::NotShadowCaster,
            ));
        }
    }
}

/// Parry clash — a directional SHEET of metal sparks off the shield face: streaks fanned back
/// toward the attacker (`dir` = hero → attacker, world XZ) and up, plus a couple of hanging
/// embers. Reads as steel-on-steel, distinct from the radial hit burst.
pub(crate) fn spawn_clash(commands: &mut Commands, fx: &CombatFx, at: Vec3, dir: Vec2) {
    let d3 = Vec3::new(dir.x, 0.0, dir.y);
    let side = Vec3::new(-dir.y, 0.0, dir.x);
    for i in 0..12u32 {
        let f = (i as f32 / 11.0) * 2.0 - 1.0; // -1..1 across the fan
        let vel = (d3 * (2.2 + (i * 29 % 7) as f32 * 0.28) + side * f * 2.1 + Vec3::Y * (1.4 + f.abs()))
            * (0.85 + (i * 13 % 5) as f32 * 0.08);
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.kill.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.5)),
            Spark::streak(vel, 0.4, 0.5),
            bevy::light::NotShadowCaster,
        ));
    }
    for i in 0..3u32 {
        let vel = d3 * 0.7 + Vec3::new(0.0, 1.0 + i as f32 * 0.3, 0.0);
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.hit.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.4)),
            Spark { grav: 2.2, ..Spark::new(vel, 0.8, 0.4) },
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
        // Mixed droplet sizes (fine spray + a few fat drops), streaked along their flight.
        let sc = scale0 * (0.7 + (i * 13 % 7) as f32 * 0.09);
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.blood.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(sc)),
            Spark::streak(vel, 0.55, sc),
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
            Spark::new(vel, life, scale0),
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
    weapon_q: Query<&GlobalTransform, With<super::HeroWeapon>>,
) {
    let Some(fx) = fx else { return };
    let Ok(hero) = hero_q.single() else { return };
    let phase = if hero.attacking { hero.attack_t / hero.attack_dur } else { -1.0 };
    if !(0.25..0.55).contains(&phase) {
        *last_tip = None; // only across the fast sweep, where the blade actually whips through
        return;
    }
    let Ok(gt) = weapon_q.single() else { return };
    let tip = gt.transform_point(super::model::WEAPON_TIP_LOCAL);
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
        let g = s.grav;
        s.vel.y -= g * dt;
        let v = s.vel;
        tf.translation += v * dt;
        let k = s.life / s.life0;
        if s.stretch {
            // A streak: orient the mote along its flight and elongate it by speed, so glints and
            // droplets read as motion trails instead of floating balls. Thins as it fades.
            let speed = v.length();
            if speed > 1e-3 {
                tf.rotation = Quat::from_rotation_arc(Vec3::Y, v / speed);
                let len = (0.9 + speed * 0.35).min(3.2);
                tf.scale = Vec3::new(s.scale0 * k * 0.55, s.scale0 * k * len, s.scale0 * k * 0.55);
                continue;
            }
        }
        tf.scale = Vec3::splat(s.scale0 * k);
    }
}
