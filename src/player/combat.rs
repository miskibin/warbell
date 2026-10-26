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
pub(crate) const HIT_PHASE: f32 = 0.3; // fraction into the swing where damage lands

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
/// Global scale on the hero's outgoing melee damage — a flat −25% playtest nerf
/// (applied to the cone/dummy/crit damage `base`, on top of weapons/crit/combo/buffs).
const HERO_OUTGOING_DMG_MUL: f64 = 0.75;

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
pub(crate) const HITSTOP_HIT: f32 = 0.055; // was 0.04: a light cut slid through without a bite
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
pub struct Juice<'w, 's> {
    feedback: ResMut<'w, crate::combat_fx::HitFeedback>,
    hitstop: ResMut<'w, HitStop>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    /// Active run's difficulty — drives the Easy hero-damage handicap.
    siege: Option<Res<'w, crate::siege::Siege>>,
    /// First-person flag — folded in here (not a standalone `Res`) so `player_attack` stays under
    /// Bevy's 16-param ceiling. In FP the view owns facing, so the lock-on facing-snap is skipped.
    fp: Res<'w, FirstPerson>,
    /// The impact flashes currently alive — counted so a cleave can't blow past
    /// [`MAX_IMPACT_LIGHTS`]. Also folded in here to stay under the param ceiling.
    lights: Query<'w, 's, (), With<LightFade>>,
    /// The blade tip's motion (written by [`hero_blade_trail`]): blood leaves with the edge.
    blade: Res<'w, BladeMotion>,
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
    /// A blood drop: the ground height it lands on. It keeps its size in flight and, landing,
    /// leaves a fleck there instead of shrinking away in mid-air.
    drip: Option<f32>,
}

impl Spark {
    fn new(vel: Vec3, life: f32, scale0: f32) -> Self {
        Spark { vel, life, life0: life, scale0, stretch: false, grav: 9.0, drip: None }
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

/// Max impact flashes alive at once. Each one is a real (if unshadowed) point light the clustered
/// forward renderer has to bin and light every affected fragment against, so a cleave landing on
/// four orks — or a war party's worth of blows in one siege frame — can otherwise stack a dozen
/// lights into the same few metres, where they're indistinguishable from two or three anyway.
pub(crate) const MAX_IMPACT_LIGHTS: usize = 8;

/// Spawn one impact flash at `at`, unless [`MAX_IMPACT_LIGHTS`] are already alive. Intensity in
/// lumens (scene torches run 18–95k; a flash reads at ~15–30k over range ~6).
///
/// `live` is the caller's running count of flashes currently in the world (seeded from a
/// `Query<(), With<LightFade>>` count, then bumped per spawn so a burst inside one system's loop
/// still respects the cap).
pub(crate) fn spawn_impact_light(commands: &mut Commands, live: &mut usize, at: Vec3, color: Color, peak: f32, life: f32, now: f32) {
    if *live >= MAX_IMPACT_LIGHTS {
        return;
    }
    *live += 1;
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
    /// A unit quad (the blood-mist puffs' card).
    quad: Handle<Mesh>,
    /// The shared feathered ground-decal quad + materials (`combat_marks`) the kill / slam
    /// shockwave and the blood splats are drawn with — no per-mark material clones.
    marks: crate::combat_marks::MarkAssets,
    /// Non-emissive pale gold for the blade-trail ribbon (bloom must never catch the trail).
    /// Base materials like this and [`Self::mist_mat`] are cloned per planar fade (`spawn_fade`).
    trail_mat: Handle<StandardMaterial>,
    /// Soft round dark-red puff (a radial-falloff sprite): the fine blood mist of a cut.
    mist_mat: Handle<StandardMaterial>,
}

pub fn setup_combat_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    marks: Res<crate::combat_marks::MarkAssets>,
) {
    let mesh = meshes.add(Sphere::new(0.07).mesh().ico(1).unwrap());
    // Sparks are hot metal: small HDR ember-orange points (an unlit material outputs `base_color`
    // verbatim — `emissive` is ignored — so the HDR gain rides the base colour), deep enough that
    // bloom gives a short glint, never a white/yellow cartoon star.
    let ember = |r: f32, g: f32, b: f32| StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::rgb(r, g, b)),
        unlit: true,
        ..default()
    };
    let hit = materials.add(ember(2.2, 0.85, 0.22));
    let kill = materials.add(ember(2.8, 1.3, 0.45));
    // Healing motes: a faint pale-sage shimmer, not a neon-green burst.
    let heal = materials.add(ember(0.55, 0.75, 0.5));
    // Debris is real matter: PBR-LIT (matte), so blood, stone chips, dust and leaves take the
    // scene's sun / shade / torchlight instead of drawing as flat-colour beads.
    let matte = |c: Color| StandardMaterial { base_color: c, perceptual_roughness: 0.95, reflectance: 0.25, ..default() };
    let blood = materials.add(StandardMaterial { perceptual_roughness: 0.2, ..matte(Color::srgb(0.2, 0.015, 0.012)) });
    let chip = materials.add(matte(Color::srgb(0.46, 0.44, 0.42)));
    let dust = materials.add(matte(Color::srgb(0.62, 0.54, 0.40)));
    let leaf = materials.add(matte(Color::srgb(0.26, 0.36, 0.16)));
    let quad = meshes.add(Rectangle::new(1.0, 1.0).mesh().build());
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
    // The blade-trail ribbon: pale gold with ZERO emissive, so bloom can never whiten it — the
    // trail should read as a translucent smear of motion, not a light source.
    // Opaque base: the ribbon's opacity rides its vertex alpha (age + inner-edge falloff).
    let trail_mat = materials.add(flash(Color::srgb(0.93, 0.9, 0.84), LinearRgba::BLACK));
    // Blood mist: a dark, NON-emissive puff with a soft radial edge (a hard quad read as a card).
    let mist_mat = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(soft_disc())),
        ..flash(Color::srgba(0.30, 0.02, 0.02, 0.5), LinearRgba::BLACK)
    });
    commands.insert_resource(CombatFx {
        mesh, hit, kill, heal, blood, chip, dust, leaf, quad, trail_mat, mist_mat,
        marks: marks.clone(),
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

/// A short-lived planar flash (slash streak / blade-trail ribbon). (The ground-flat shockwave and
/// blood splats moved to `combat_marks`, which fades through the instance tag, not a material.)
/// Owns a cloned material so it can alpha-fade alone; [`update_fx_fades`] frees that material on
/// despawn so per-hit clones never leak (verified: the `mats.remove(&f.mat)` on the ring-out path
/// below is the only exit, and an effect swept early by a biome rebuild drops its last strong
/// handle with the entity, which frees the asset too).
#[derive(Component)]
pub(crate) struct FxFade {
    born: f32,
    life: f32,
    mat: Handle<StandardMaterial>,
    s0: Vec3,
    s1: Vec3,
    a0: f32,
    face: FadeFace,
    /// Alpha last actually written into `mat`. Writing a material re-uploads its uniform + rebuilds
    /// its bind group, so the fade only writes when the value has moved a visible step — see
    /// [`FADE_ALPHA_STEP`].
    last_a: f32,
}

/// Minimum alpha change worth re-writing a fade's material for — at this step a long fade writes
/// ~35 times instead of every frame, each step far under the eye's threshold on a translucent
/// quad. The short flashes (a 0.11s slash) step every frame anyway, so they're unaffected.
const FADE_ALPHA_STEP: f32 = 0.02;

/// How a fading quad orients each frame.
#[derive(Clone, Copy)]
pub(crate) enum FadeFace {
    /// Re-face the camera flat-on each frame (the blood mist puffs).
    Camera,
}

/// Drain a planar flash over its life: scale `s0→s1` (ease-out), fade alpha `a0→0`, orient per
/// its [`FadeFace`], then despawn + free the cloned material.
pub fn update_fx_fades(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    mut q: Query<(Entity, &mut FxFade, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    let cam_pos = cam_q.single().map(|t| t.translation()).ok();
    for (e, mut f, mut tf) in &mut q {
        let k = (now - f.born) / f.life;
        if k >= 1.0 {
            // `try_despawn`: these quads are tagged `BiomeEntity`, so a biome swap (keys 1-5) or a
            // New Game rebuild can wipe a still-fading decal in the same frame this reap fires.
            commands.entity(e).try_despawn();
            mats.remove(&f.mat);
            continue;
        }
        let ease = 1.0 - (1.0 - k) * (1.0 - k); // ease-out
        tf.scale = f.s0.lerp(f.s1, ease);
        match (f.face, cam_pos) {
            (FadeFace::Camera, Some(cp)) => tf.look_at(cp, Vec3::Y),
            _ => {}
        }
        // Only touch the material when the alpha has moved a visible step (see `FADE_ALPHA_STEP`):
        // a `get_mut` here re-prepares the bind group, and a busy fight keeps dozens of these
        // decals alive at once.
        let a = f.a0 * (1.0 - k);
        if (a - f.last_a).abs() >= FADE_ALPHA_STEP {
            if let Some(mut m) = mats.get_mut(&f.mat) {
                m.base_color = m.base_color.with_alpha(a);
            }
            f.last_a = a;
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
/// Debug/capture hook: `FOREST_SWINGTEST=1` re-arms the attack chain on a ~1.15s loop (`=<secs>` sets
/// the gap; `0.3` chains every swing straight off the last, as mashing does — all inside
/// `COMBO_WINDOW`, so the combo steps through chop → slash → thrust; every 4th swing fires the
/// Heavy, out of a held charge as in play) with no keypress and skipping the pointer-lock gate, so a
/// `FOREST_FP`/`FOREST_TPS` shot/clip can frame the full swing set — same pattern as
/// `FOREST_ROLLTEST`. Its own system (after [`player_attack`], which already sits at Bevy's
/// 16-param ceiling and would otherwise disarm the staged charge as an unpressed button).
pub fn swing_test(
    time: Res<Time>,
    mode: Res<PlayMode>,
    player: Res<PlayerRes>,
    fp: Res<super::FirstPerson>,
    mut cues: MessageWriter<AudioCue>,
    mut hero_q: Query<&mut Hero>,
    mut next: Local<f32>,
    mut count: Local<u32>,
    mut charge_from: Local<Option<f32>>,
) {
    let Ok(test) = std::env::var("FOREST_SWINGTEST") else { return };
    // A number sets the gap between swing starts (s): `0.3` mashes the chain back to back.
    let gap = test.parse::<f32>().ok().filter(|g| *g > 0.0 && *g != 1.0).unwrap_or(1.15);
    let Ok(mut hero) = hero_q.single_mut() else { return };
    let now_s = time.elapsed_secs();
    if *mode != PlayMode::Play || !player.0.is_alive() || hero.roll_t >= 0.0 {
        *charge_from = None;
        return;
    }
    // The Heavy is held like a real LMB hold — the charge stance coils up — and released past
    // the threshold. (`player_attack` disarms an unpressed charge each frame; re-stamp it after.)
    if let Some(from) = *charge_from {
        hero.charge_t = now_s - from;
        if hero.charge_t >= CHARGE_THRESHOLD + 0.15 {
            *charge_from = None;
            hero.charge_t = -1.0;
            *next = now_s + gap;
            begin_swing(&mut hero, true, None, now_s, fp.active);
            cues.write(AudioCue::HeroGruntSwing);
            cues.write(AudioCue::Slam);
        }
        return;
    }
    if hero.attacking || now_s < *next {
        return;
    }
    *count += 1;
    if *count % 4 == 0 {
        *charge_from = Some(now_s);
        hero.charge_t = 0.0;
        return;
    }
    *next = now_s + gap;
    begin_swing(&mut hero, false, None, now_s, fp.active);
    cues.write(AudioCue::HeroGruntSwing);
}

/// Cinematic staging of the Heavy: a scene (`scenes::storm`'s coda) holds the hero in the charge
/// stance (`coil`) and lets the blow go (`release`, consumed) without any input, like
/// [`swing_test`] does for captures.
#[derive(Resource, Default)]
pub struct StagedStrike {
    pub coil: bool,
    pub release: bool,
    from: Option<f32>,
}

/// Applies [`StagedStrike`] after `player_attack` (which disarms an unheld charge every frame).
pub fn staged_strike(time: Res<Time>, mut staged: ResMut<StagedStrike>, mut cues: MessageWriter<AudioCue>, mut hero_q: Query<&mut Hero>) {
    let Ok(mut hero) = hero_q.single_mut() else { return };
    let now = time.elapsed_secs();
    if std::mem::take(&mut staged.release) {
        staged.from = None;
        hero.charge_t = -1.0;
        if !hero.attacking {
            begin_swing(&mut hero, true, None, now, false);
            cues.write(AudioCue::HeroGruntSwing);
            cues.write(AudioCue::Slam);
        }
    } else if staged.coil {
        let from = *staged.from.get_or_insert(now);
        hero.charge_t = now - from;
    } else {
        staged.from = None;
    }
}

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
                With<crate::oddities::Oddity>,
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
    let base = (player.0.attack_damage + mods.weapon_bonus())
        * mods.power_mult(now)
        * dmg_mul
        * combo_mult
        * HERO_OUTGOING_DMG_MUL;
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
    // Live impact-flash count, seeded once and bumped per spawn below, so a cleave that lands on
    // four bodies in one frame still respects `MAX_IMPACT_LIGHTS`.
    let mut live_lights = juice.lights.iter().count();
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
        // Can't swing through a wall — LOS-gate the hit (walls/buildings only; foliage is fine).
        if crate::blockers::wall_between(origin.x, origin.y, p.x, p.z) {
            continue;
        }
        struck.push(Vec2::new(p.x, p.z));
        hit_ents.push(e);
        hp.hp -= dmg;
        let dead = hp.hp <= 0.0;
        // A cut reads through BLOOD, not light (The Witcher 3 / KCD2): mist + drops along the
        // edge, a pool under the foe; a crit / Heavy / kill opens up more. (The old hit sparks —
        // an ember starburst on every crit and Heavy — a hard-edged glint card and a bright
        // impact light read as a cartoon flower of light on a body.) A stone golem chips instead.
        let now_s = time.elapsed_secs();
        let mid = Vec3::new(p.x, p.y + 0.9, p.z);
        let stone = animal.as_deref().is_some_and(|a| a.species == crate::critters::Species::Golem);
        let hard = dead || crit || hero.heavy;
        if stone {
            spawn_chips(&mut commands, &fx, mid, hard);
        } else {
            spawn_blood(&mut commands, &fx, &mut juice.materials, mid, dir, juice.blade.tip_vel, p.y, hard, now_s);
            // A pool under the target on every hit (small), big + lingering on a kill.
            spawn_splat(&mut commands, &fx, Vec3::new(p.x, p.y, p.z), dead, now_s);
        }
        // (No ground shockwave on a sword hit any more: sunlit, the dust ring read as a glowing
        // disc under the foe. The Warden slam arts keep theirs.)
        if dead {
            // Only a faint warm kiss of light on the kill — a full flash bloomed into a white blot.
            spawn_impact_light(&mut commands, &mut live_lights, mid, Color::srgb(1.0, 0.9, 0.6), 6_000.0, 0.12, now_s);
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
                spawn_blood(&mut commands, &fx, &mut juice.materials, mid, Vec2::ZERO, Vec3::ZERO, p.y, hp.hp <= 0.0, now_s);
                if hp.hp <= 0.0 {
                    spawn_splat(&mut commands, &fx, Vec3::new(p.x, p.y, p.z), true, now_s);
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
                    spawn_splat(&mut commands, &fx, Vec3::new(p.x, p.y, p.z), false, now_s);
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

    // One sting for the whole swing: impact on a connect (heavier on a kill, the dedicated crit
    // take on a crit — same swing-wide `crit` flag as the juice tiers above), else the empty-
    // swing whoosh. Never stacked — a connecting hit never also plays the whoosh.
    if killed_any {
        cues.write(AudioCue::Impact { kill: true, crit });
    } else if hit_any {
        cues.write(AudioCue::Impact { kill: false, crit });
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
        if kill { (16u32, 4.0, fx.kill.clone(), 0.45) } else { (8, 2.6, fx.hit.clone(), 0.3) };
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
                Transform::from_translation(at).with_scale(Vec3::splat(0.22)),
                Spark { grav: 2.2, ..Spark::new(vel, 1.0, 0.22) },
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
            Transform::from_translation(at).with_scale(Vec3::splat(0.26)),
            Spark::streak(vel, 0.4, 0.26),
            bevy::light::NotShadowCaster,
        ));
    }
    for i in 0..3u32 {
        let vel = d3 * 0.7 + Vec3::new(0.0, 1.0 + i as f32 * 0.3, 0.0);
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.hit.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.2)),
            Spark { grav: 2.2, ..Spark::new(vel, 0.8, 0.2) },
            bevy::light::NotShadowCaster,
        ));
    }
}

/// A 64² white disc with a smooth radial alpha falloff — the sprite of every soft puff.
fn soft_disc() -> Image {
    const N: usize = 64;
    let mut px = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / N as f32 * 2.0 - Vec2::ONE;
            let a = (1.0 - d.length()).clamp(0.0, 1.0);
            px.extend_from_slice(&[255, 255, 255, (a * a * (3.0 - 2.0 * a) * 255.0) as u8]);
        }
    }
    Image::new(
        bevy::render::render_resource::Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        px,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
}

/// Blood off a cut, after how it reads in The Witcher 3 / KCD2: subtle, but weighty. A short dark
/// MIST puff at the wound, a fan of DROPS flung along the blade's sweep (`sweep`, the tip's
/// velocity at contact — blood leaves with the edge, not straight out of the foe's back) and out
/// of the far side of the blow (`dir`, world XZ), arcing down under gravity; each drop that lands
/// leaves a small fleck where it fell ([`spawn_fleck`]), so a fight paints the ground around it.
/// A kill opens up more. `ground` = the struck foe's foot height.
pub(crate) fn spawn_blood(
    commands: &mut Commands,
    fx: &CombatFx,
    mats: &mut Assets<StandardMaterial>,
    at: Vec3,
    dir: Vec2,
    sweep: Vec3,
    ground: f32,
    kill: bool,
    now: f32,
) {
    let dir3 = Vec3::new(dir.x, 0.0, dir.y);
    // The edge's travel, flattened a little toward the ground plane and capped: drops ride it.
    let edge = (sweep * Vec3::new(1.0, 0.6, 1.0)).clamp_length_max(9.0) * 0.32;
    let (n, spd) = if kill { (36u32, 1.25) } else { (22, 1.0) };
    let seed = mark_seed(at, now);
    for i in 0..n {
        let h = seed.wrapping_add(i.wrapping_mul(0x9e37_79b9)).wrapping_mul(0x85eb_ca6b);
        let r = |k: u32| ((h >> (k * 5)) & 31) as f32 / 31.0;
        let a = i as f32 * 2.399_963_2; // golden angle → even spread
        let spread = Vec3::new(a.cos(), 0.0, a.sin()) * (0.35 + 0.5 * r(0));
        let vel = (edge * (0.55 + 0.6 * r(1)) + dir3 * (0.9 + 0.8 * r(2)) + spread + Vec3::Y * (0.4 + 1.3 * r(3))) * spd;
        // Fine spray and a few fat drops (the sphere mote is 0.14 u across ×scale: a fat drop
        // ~5 cm, the spray 1.5–3 cm — any bigger and they read as red capsules, not blood).
        let sc = if i % 5 == 0 { 0.3 + 0.12 * r(4) } else { 0.1 + 0.1 * r(4) };
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.blood.clone()),
            Transform::from_translation(at + spread * 0.12).with_scale(Vec3::splat(sc)),
            Spark { drip: Some(ground), ..Spark::streak(vel, 1.6, sc) },
            bevy::light::NotShadowCaster,
        ));
    }
    // The mist: a few soft dark puffs at the wound, drifting with the cut and swelling as they thin.
    let puffs = if kill { 4 } else { 3 };
    for i in 0..puffs {
        let o = (edge * 0.025 + dir3 * 0.05) * i as f32 + Vec3::Y * 0.04 * i as f32;
        let s0 = 0.18 + 0.05 * i as f32;
        spawn_fade(
            commands, mats, &fx.mist_mat, &fx.quad,
            Transform::from_translation(at + o),
            Vec3::splat(s0), Vec3::splat(s0 * if kill { 3.6 } else { 3.0 }),
            if kill { 0.55 } else { 0.42 }, 0.24 + 0.05 * i as f32, FadeFace::Camera, now,
        );
    }
}

/// The fleck a landed blood drop leaves: a small dark stain that soaks in over ~20 s.
fn spawn_fleck(commands: &mut Commands, fx: &CombatFx, at: Vec3, size: f32, now: f32) {
    use crate::combat_marks::{spawn_mark, MarkFade, MarkKind};
    let seed = mark_seed(at, now);
    let d = 0.06 + 0.2 * size;
    let squash = 0.55 + (seed % 17) as f32 / 17.0 * 0.4;
    let e = spawn_mark(commands, &fx.marks, MarkKind::Blood, at + Vec3::Y * 0.02, d, seed as f32 * 0.37, squash, 0.8, seed);
    commands.entity(e).insert((
        MarkFade { kind: MarkKind::Blood, seed, born: now, hold: 14.0, life: 6.0, a0: 0.8, s0: d * 0.7, s1: d },
        crate::biome::BiomeEntity,
    ));
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
        FxFade { born: now, life, mat, s0, s1, a0, face, last_a: a0 },
    ));
}

/// A kill / slam / heavy-blow shockwave: a soft ring of thrown dust that rushes out across the
/// ground and frays as it fades (`combat_marks`, PBR-lit so it sits in the scene's light). It used
/// to be a glowing amber `Annulus` — an arcade "energy ring" that read low-poly.
pub(crate) fn spawn_shockwave(commands: &mut Commands, fx: &CombatFx, at: Vec3, now: f32) {
    use crate::combat_marks::{spawn_mark, MarkFade, MarkKind};
    let seed = mark_seed(at, now);
    let e = spawn_mark(commands, &fx.marks, MarkKind::Shock, at + Vec3::Y * 0.02, 0.9, seed as f32, 1.0, 0.5, seed);
    commands.entity(e).insert((
        MarkFade { kind: MarkKind::Shock, seed, born: now, hold: 0.0, life: 0.42, a0: 0.5, s0: 0.9, s1: 3.6 },
        crate::biome::BiomeEntity,
    ));
}

/// A blood splat on the ground under a struck creature — an irregular, soaked-in pool (seeded, so
/// no two match) that fades slowly, so a fight leaves a brief trail of where blows landed. A kill
/// leaves a big, long-lived pool; a glancing hit just flecks the ground.
pub(crate) fn spawn_splat(commands: &mut Commands, fx: &CombatFx, at: Vec3, kill: bool, now: f32) {
    use crate::combat_marks::{spawn_mark, MarkFade, MarkKind};
    let seed = mark_seed(at, now);
    // A fight should leave its mark for a while: pools soak in over tens of seconds.
    let (size, hold, life, a0) = if kill { (1.5, 22.0, 8.0, 0.85) } else { (0.55, 8.0, 5.0, 0.6) };
    // Squash one axis a little (seeded) so pools never read as a stamped circle.
    let squash = 0.72 + (seed % 23) as f32 / 23.0 * 0.28;
    let e = spawn_mark(commands, &fx.marks, MarkKind::Blood, Vec3::new(at.x, at.y + 0.02, at.z), size, seed as f32 * 0.37, squash, a0, seed);
    commands.entity(e).insert((
        MarkFade { kind: MarkKind::Blood, seed, born: now, hold, life, a0, s0: size * 0.8, s1: size },
        crate::biome::BiomeEntity,
    ));
}

/// A cheap per-mark shape seed from where + when it landed (deterministic, no RNG resource).
fn mark_seed(at: Vec3, now: f32) -> u32 {
    let h = (at.x * 73.1).to_bits() ^ (at.z * 19.7).to_bits().rotate_left(13) ^ (now * 997.0).to_bits().rotate_left(7);
    h.wrapping_mul(0x9e37_79b9) >> 8
}

/// The hero's blade in motion: the tip's world velocity (u/s), for effects that follow the edge.
#[derive(Resource, Default)]
pub(crate) struct BladeMotion {
    pub tip_vel: Vec3,
}

/// Seconds a blade-trail sample lives (the ribbon's length in time).
const TRAIL_LIFE: f32 = 0.12;
/// Tip speed (u/s) above which the blade leaves a trail — the real cut, not the wind-up or carry.
const TRAIL_SPEED: f32 = 7.0;
/// Share of the blade (from the tip) the ribbon spans.
const TRAIL_SPAN: f32 = 0.5;
/// Sub-steps of the Catmull-Rom between two sampled frames.
const TRAIL_SUB: usize = 5;
/// Peak opacity of the ribbon (along its brightest line, a little in from the tip).
const TRAIL_ALPHA: f32 = 0.24;

/// The blade trail's samples and its one ribbon mesh.
#[derive(Default)]
pub struct BladeTrail {
    /// (time, inner edge, tip, starts a new stroke).
    samples: std::collections::VecDeque<(f32, Vec3, Vec3, bool)>,
    mesh: Option<(Entity, Handle<Mesh>)>,
    last: Option<(f32, Vec3)>,
    cutting: bool,
}

/// The weapon trail: a translucent SURFACE swept by the blade (from part-way down the blade to
/// its tip) over the last [`TRAIL_LIFE`] s, rebuilt each frame as one ribbon mesh. Between
/// sampled frames both edges follow a Catmull-Rom through the samples, so the smear curves with
/// the cut instead of chaining straight quads (each frame's chord at a ~1 u/frame tip speed read
/// as a hard-cornered polyline). Opacity falls off with age and toward the inner edge, so it's a
/// soft streak of motion, not a band. Emitted wherever the blade really moves fast (any swing,
/// the Heavy, a Warden art), keyed off the tip's speed rather than a fixed swing window.
pub fn hero_blade_trail(
    time: Res<Time>,
    fx: Option<Res<CombatFx>>,
    fp: Res<super::FirstPerson>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut trail: Local<BladeTrail>,
    mut motion: ResMut<BladeMotion>,
    hero_q: Query<&Hero>,
    weapon_q: Query<&GlobalTransform, With<super::HeroWeapon>>,
    mut vis_q: Query<&mut Visibility>,
) {
    let Some(fx) = fx else { return };
    let now = time.elapsed_secs();
    let (ribbon, handle) = match &trail.mesh {
        Some(m) => m.clone(),
        None => {
            let h = meshes.add(Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::default(),
            ));
            let e = commands
                .spawn((
                    Mesh3d(h.clone()),
                    MeshMaterial3d(fx.trail_mat.clone()),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    bevy::light::NotShadowCaster,
                    bevy::light::NotShadowReceiver,
                    // Rebuilt in world space every frame: its spawn-time bounds mean nothing.
                    bevy::camera::visibility::NoFrustumCulling,
                ))
                .id();
            trail.mesh = Some((e, h.clone()));
            return;
        }
    };
    // Sample the blade (its GlobalTransform is last frame's propagate — a steady one-frame lag).
    let hero = hero_q.single().ok();
    let blade = weapon_q.single().ok().filter(|_| fp.blend <= 0.3 && hero.is_some());
    // First person: NO ribbon — the tip sweeps right across the lens, and a swept surface that
    // close lands ON the camera as a frame-filling wash. The FP swing reads through the arm.
    if let Some(gt) = blade {
        let tip = gt.transform_point(super::footman::weapon_tip());
        let inner = gt.transform_point(super::footman::weapon_tip() * (1.0 - TRAIL_SPAN));
        let vel = match trail.last {
            Some((t, p)) if now > t => (tip - p) / (now - t),
            _ => Vec3::ZERO,
        };
        if trail.last.is_none_or(|(t, _)| now > t) {
            trail.last = Some((now, tip));
            motion.tip_vel = vel;
            let fast = vel.length() > TRAIL_SPEED && hero.is_some_and(|h| h.attacking || h.dash_t >= 0.0);
            if fast {
                let new_stroke = !trail.cutting;
                trail.samples.push_back((now, inner, tip, new_stroke));
            }
            trail.cutting = fast;
        }
    } else {
        trail.last = None;
        trail.cutting = false;
        motion.tip_vel = Vec3::ZERO;
    }
    while trail.samples.front().is_some_and(|s| now - s.0 > TRAIL_LIFE) {
        trail.samples.pop_front();
    }
    if let Some(first) = trail.samples.front_mut() {
        first.3 = true;
    }

    // ── Rebuild the ribbon: one strip per stroke, sub-stepped on a Catmull-Rom ──
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let samples: Vec<_> = trail.samples.iter().copied().collect();
    let mut start = 0;
    while start < samples.len() {
        let mut end = start + 1;
        while end < samples.len() && !samples[end].3 {
            end += 1;
        }
        let stroke = &samples[start..end];
        if stroke.len() >= 2 {
            let at = |i: isize| stroke[i.clamp(0, stroke.len() as isize - 1) as usize];
            // Each stroke is its own strip: quads only join edges of the same stroke.
            let first = pos.len() as u32;
            let mut push = |t: f32, inner: Vec3, tip: Vec3| {
                let age = ((now - t) / TRAIL_LIFE).clamp(0.0, 1.0);
                let a = TRAIL_ALPHA * (1.0 - age) * (1.0 - age);
                // Three rows (inner, brightest, tip): soft on both edges, so it's a smear of
                // motion rather than a sheet with a cut outline.
                let base = pos.len() as u32;
                pos.push(inner.to_array());
                pos.push(inner.lerp(tip, 0.78).to_array());
                pos.push(tip.to_array());
                col.push([1.0, 1.0, 1.0, 0.0]);
                col.push([1.0, 1.0, 1.0, a]);
                col.push([1.0, 1.0, 1.0, a * 0.3]);
                if base >= first + 3 {
                    let q = |i: u32, j: u32| [i, j, i + 3, i + 3, j, j + 3];
                    idx.extend_from_slice(&q(base - 3, base - 2));
                    idx.extend_from_slice(&q(base - 2, base - 1));
                }
            };
            let cat = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, u: f32| {
                let (u2, u3) = (u * u, u * u * u);
                0.5 * (2.0 * b + (c - a) * u + (2.0 * a - 5.0 * b + 4.0 * c - d) * u2 + (3.0 * b - a - 3.0 * c + d) * u3)
            };
            for i in 0..stroke.len() as isize - 1 {
                let (s0, s1, s2, s3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
                let subs = if i == stroke.len() as isize - 2 { TRAIL_SUB + 1 } else { TRAIL_SUB };
                for k in 0..subs {
                    let u = k as f32 / TRAIL_SUB as f32;
                    push(
                        s1.0 + (s2.0 - s1.0) * u,
                        cat(s0.1, s1.1, s2.1, s3.1, u),
                        cat(s0.2, s1.2, s2.2, s3.2, u),
                    );
                }
            }
        }
        start = end;
    }
    let shown = !idx.is_empty();
    if let Ok(mut v) = vis_q.get_mut(ribbon) {
        v.set_if_neq(if shown { Visibility::Visible } else { Visibility::Hidden });
    }
    if !shown {
        return;
    }
    if let Some(mut mesh) = meshes.get_mut(&handle) {
        let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
        mesh.insert_indices(bevy::mesh::Indices::U32(idx));
    }
}

pub fn update_sparks(
    time: Res<Time>,
    fx: Option<Res<CombatFx>>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Spark, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
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
        if let Some(ground) = s.drip {
            if tf.translation.y <= ground && v.y < 0.0 {
                // Only the heavier drops stain (about half): a long siege mustn't pile up decals.
                if let Some(fx) = fx.as_deref().filter(|_| s.scale0 > 0.28) {
                    spawn_fleck(&mut commands, fx, Vec3::new(tf.translation.x, ground, tf.translation.z), s.scale0, now);
                }
                commands.entity(e).despawn();
                continue;
            }
        }
        let k = if s.drip.is_some() { 1.0 } else { s.life / s.life0 };
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
