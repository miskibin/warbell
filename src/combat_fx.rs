//! Combat juice — floating damage numbers, enemy HP bars, ork hurt-flash, and hero
//! hit feedback (screen shake + red flash). Ported from the original game's
//! `fxStore.ts` / `FloatingText.tsx` / `Ork.tsx`; the HP bars were since rebuilt as one
//! shader quad each (`hp_bar.wgsl`) in the shared vitals style (`ui::vitals`).
//!
//! Everything attaches to orks **externally** (the same philosophy as
//! `combat::Health`): hit-sites only push to [`FloatQueue`], set [`HitFeedback`],
//! or insert [`HurtFlash`] — this module does all the rendering and even clones a
//! per-ork material on the fly, so the shared ork model code stays untouched.

use bevy::mesh::{MeshBuilder, MeshTag};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui::UiTransform;

use crate::orks::Ork;
use crate::player::Health;
use crate::ui::UiFonts;

// ── Tuning (ported from the originals) ──────────────────────────────────────
/// Floating-number lifetime (s).
const FLOAT_LIFE: f32 = 1.1;
/// World-units a number rises over its life.
const FLOAT_RISE: f32 = 1.3;
/// Base font px for a scale-1 number.
const FLOAT_FONT: f32 = 24.0;
/// Drop-shadow alpha at full opacity (fades with the number).
const FLOAT_SHADOW_A: f32 = 0.85;
/// Hurt-flash duration on a struck ork (s). Still a front-loaded *pop* (bright hot core at the
/// contact frame, k² falloff — see `hurt_flash`), not a plateau; but 0.11 s lived on only ~3
/// frames at 30 fps, so on capture/stream footage the flash read as barely-there (verified on the
/// baseline combat clip — not one still caught it mid-flash). 0.17 keeps the punch shape while
/// surviving the frame grid; rapid combo hits still don't strobe (each re-insert restarts the pop).
const HURT_FLASH_DUR: f32 = 0.17;
/// Warm tint of the flash (linear RGB weights) — a hot white-amber spark, NOT flat engine-white,
/// so the pop reads as a struck-flesh impact rather than a placeholder blink. Scaled by the
/// per-hit `intensity` (light < crit < heavy), so a heavy blow flashes far harder than a poke.
const HURT_FLASH_TINT: [f32; 3] = [1.0, 0.92, 0.78];
/// Trauma shed per second (old `TRAUMA_DECAY`).
const SHAKE_DECAY: f32 = 2.4;
/// Camera offset (world units) at full trauma.
pub const SHAKE_MAX: f32 = 0.35;
/// HP-bar height as a fraction of the SCREEN height: the bar is screen-constant across the fight
/// range (its world size is re-solved from camera distance + FOV every frame), so it stays a thin,
/// crisp line — the old fixed 0.6u quad was a slab across the frame up close and a speck far off.
/// 0.0088 ≈ 9.5px at 1080p (1px rim + 1px frame each side + a ~5px channel); with the aspect below
/// the bar is ≈105px wide.
const HP_BAR_SCREEN_H: f32 = 0.0088;
const HP_BAR_ASPECT: f32 = 11.0;
/// Past this camera distance the bar stops being screen-constant and shrinks at half the
/// perspective rate, so a far foe's bar still reads as farther away without going microscopic.
const HP_BAR_CONST_DIST: f32 = 14.0;
/// Brightness gain on the UI vitals palette for the world bar: its unlit colour runs through the
/// tonemapper (the HUD's doesn't), which deepens + desaturates it a touch.
const HP_BAR_GAIN: f32 = 1.0;
/// Height above the ork root the bar floats at — at the creature's BASE rig scale. Orks and town
/// NPCs were enlarged by `castle::WORLD_BUMP`, so their bars are spawned at this × that bump (see
/// `ensure_hp_bars`); animals (left at their original scale) use this value as-is. Because the rig
/// base sits at y=0 and scales about it, the head height scales linearly — so the bar tracks it.
const HP_BAR_Y: f32 = 1.55;
/// Lower float for town NPCs — the townsfolk rig is shorter than an ork's. Also ×`WORLD_BUMP` at
/// spawn (the townsfolk were enlarged with everything else).
const HP_BAR_Y_NPC: f32 = 1.2;
/// Beyond this camera distance a bar is hidden — keeps far-off damaged enemies from floating
/// bars across the sky (they read as detached when the body's behind trees / off-screen).
const HP_BAR_MAX_DIST: f32 = 26.0;
/// Max height a bar may float ABOVE THE CAMERA EYE, as a slope over horizontal distance (≈tan 20°).
/// In first-person melee a `WORLD_BUMP`-scaled ork towers over the 1.32 eye, so its overhead bar
/// (root + ~2.1) sits far outside the top of the frame — past this cone the bar slides DOWN onto
/// the foe's chest/face line where it stays readable. Third person is untouched in practice: the
/// orbit eye rides above head height, so the cone almost never binds.
const HP_BAR_CONE_SLOPE: f32 = 0.36;
/// Within this camera distance a *lowered* bar is pulled toward the lens (out of the body it now
/// overlaps — depth-test would bury a chest-height quad inside the torso) and the whole bar is
/// shrunk, so an at-arm's-length bar doesn't stripe across half the frame.
const HP_BAR_NEAR: f32 = 4.0;

// ── Ported float colours ────────────────────────────────────────────────────
// Muted to the UI kit's semantic tones (`ui::theme`) — the old candy red/blue/green read arcade.
/// Brick `-N` when the hero is struck.
pub fn col_hero_hit() -> Color {
    crate::ui::theme::BAD
}
/// Pale-steel `BLOCK`.
pub fn col_block() -> Color {
    crate::ui::theme::INFO
}
/// Parchment-white `N` on a struck enemy.
pub fn col_ork_hit() -> Color {
    crate::ui::theme::TEXT
}
/// Sage `†` on a kill.
pub fn col_kill() -> Color {
    crate::ui::theme::GOOD
}

// ── 1. Floating damage numbers (screen-space UI text) ───────────────────────

/// One number to spawn this frame.
pub struct FloatReq {
    pub world: Vec3,
    pub text: String,
    pub color: Color,
    pub scale: f32,
}

/// Spawn queue — hit-sites push, [`spawn_floats`] drains.
#[derive(Resource, Default)]
pub struct FloatQueue(pub Vec<FloatReq>);

#[derive(Component)]
struct FloatText {
    anchor: Vec3,
    born: f32,
    color: Color,
    scale: f32,
    /// Longest line, in chars (horizontal centring) and the line count (vertical centring).
    len: usize,
    lines: usize,
    /// Seconds on screen: [`FLOAT_LIFE`] for a number / short word, longer for a sentence (a
    /// landmark's lore line must stay up long enough to read).
    life: f32,
}

fn spawn_floats(
    mut commands: Commands,
    time: Res<Time>,
    fonts: Res<UiFonts>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    mut q: ResMut<FloatQueue>,
) {
    let now = time.elapsed_secs();
    let cam_pos = cam_q.single().map(|gt| gt.translation()).ok();
    for r in q.0.drain(..) {
        let len = r.text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
        let lines = r.text.lines().count().max(1);
        let life = (FLOAT_LIFE + r.text.chars().count().saturating_sub(24) as f32 * 0.045).min(4.5);
        // Same close-range clamp as the HP bars: hit sites anchor numbers ~2.2u above the
        // target, which at FP melee range is far above the frame's top edge — every damage
        // number on a point-blank foe spawned invisible. Cap the anchor to a view cone above
        // the camera eye (minus a little rise headroom) so close-quarters numbers pop where
        // the player is actually looking.
        let mut world = r.world;
        if let Some(cp) = cam_pos {
            let flat = Vec2::new(world.x - cp.x, world.z - cp.z).length();
            world.y = world.y.min(cp.y + flat * HP_BAR_CONE_SLOPE - FLOAT_RISE * 0.4);
        }
        commands.spawn((
            Text::new(r.text),
            // Rounded to a whole-pixel bucket (drive_floats animates within the same integer set):
            // a per-size glyph atlas is permanent in Bevy, so only integer sizes are ever minted.
            TextFont { font: fonts.extrabold.clone().into(), font_size: (FLOAT_FONT * r.scale).round().into(), ..default() },
            TextColor(r.color),
            // A crisp dark drop shadow so numbers pop against the bright scene (fades in drive_floats).
            TextShadow { offset: Vec2::new(0.0, 2.5), color: Color::srgba(0.0, 0.0, 0.0, FLOAT_SHADOW_A) },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-9999.0),
                top: Val::Px(-9999.0),
                ..default()
            },
            UiTransform::IDENTITY,
            GlobalZIndex(20),
            FloatText { anchor: world, born: now, color: r.color, scale: r.scale, len, lines, life },
        ));
    }
}

fn drive_floats(
    time: Res<Time>,
    mut commands: Commands,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    mut q: Query<(Entity, &FloatText, &mut Node, &mut UiTransform, &mut TextColor, &mut TextShadow)>,
) {
    let now = time.elapsed_secs();
    let Ok((cam, cam_tf)) = cam_q.single() else { return };
    for (e, f, mut node, mut ui_tf, mut tc, mut shadow) in &mut q {
        let t = now - f.born;
        let k = (t / f.life).clamp(0.0, 1.0);
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        // Pop in fast with a slight overshoot, then settle at 1 (old FloatingText).
        let pop =
            if t < 0.16 { 0.6 + (t / 0.16) * 0.55 } else { (1.15 - (t - 0.16) * 1.6).max(1.0) };
        // The pop is driven by UiTransform.scale, NOT font_size: Bevy mints a fresh 512² glyph
        // atlas for every distinct font_size and NEVER frees them, so continuously animating the
        // size (a new value nearly every frame, per number) leaked atlases without bound — this
        // regressed past the old "round to whole px" mitigation once per-hit `scale` variety (crit/
        // heavy/etc.) combined with the pop sweep to mint far more than the intended ~30 sizes,
        // which is the periodic multi-hundred-ms freeze (a new atlas PAGE allocation) players saw
        // roughly every ~10s in a long fight. `font_size` is now fixed at spawn (one atlas per
        // categorical `scale`, never re-touched) and the whole pop/overshoot animation is a free UI
        // transform scale — no new glyph rasterization, ever.
        ui_tf.scale = Vec2::splat(pop);
        let fade = 1.0 - k * k;
        tc.0 = f.color.with_alpha(fade);
        shadow.color = Color::srgba(0.0, 0.0, 0.0, FLOAT_SHADOW_A * fade);
        let world = f.anchor + Vec3::Y * (FLOAT_RISE * k);
        // Centring uses the FIXED spawn font size (UiTransform scales around the node's own centre,
        // so the pop grows/shrinks in place — no re-centring needed as `pop` changes).
        let base_font = (FLOAT_FONT * f.scale).round();
        match cam.world_to_viewport(cam_tf, world) {
            Ok(px) => {
                node.display = Display::Flex;
                // Roughly centre the glyphs on the anchor point.
                node.left = Val::Px(px.x - base_font * 0.3 * f.len as f32);
                node.top = Val::Px(px.y - base_font * (0.5 + 0.6 * (f.lines as f32 - 1.0)));
            }
            Err(_) => node.display = Display::None,
        }
    }
}

/// Screenshot/tuning hook: `FOREST_FLOATTEST=1` continuously stages a spread of sample combat
/// numbers near the hero so a capture can frame the floating-text styling. No effect in normal play.
fn float_test(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    mut q: ResMut<FloatQueue>,
    mut t: Local<f32>,
) {
    if std::env::var("FOREST_FLOATTEST").is_err() {
        return;
    }
    *t -= time.delta_secs();
    if *t > 0.0 {
        return;
    }
    *t = 0.5;
    let base = Vec3::new(hero.pos.x, 2.2, hero.pos.y);
    let amber = crate::ui::theme::GOLD;
    let samples: [(&str, Color, f32, Vec3); 6] = [
        ("25", col_ork_hit(), 1.0, Vec3::new(-1.4, 0.4, 0.0)),
        ("48", col_ork_hit(), 1.5, Vec3::new(-0.4, 1.1, 0.0)),
        ("-7", col_hero_hit(), 1.0, Vec3::new(0.8, 0.2, 0.0)),
        ("BLOCK", col_block(), 0.95, Vec3::new(1.6, 0.9, 0.0)),
        ("†", col_kill(), 1.4, Vec3::new(0.2, 1.6, 0.0)),
        ("+3 gold", amber, 1.0, Vec3::new(-1.0, -0.2, 0.0)),
    ];
    for (text, color, scale, off) in samples {
        q.0.push(FloatReq { world: base + off, text: text.into(), color, scale });
    }
}

/// Tuning/capture hook: `FOREST_HITTEST=1` staged-hits the ork nearest the hero every ~0.6s,
/// cycling light → crit → heavy → kill, so a clip/still shows the FULL landed-blow package —
/// flash pop, absorb squash, recoil lean, knockback shove, hit-stop, camera trauma + FOV kick —
/// across the weight tiers without landing real swings. Mirrors the `player_attack` hit site
/// (same `KNOCKBACK`/`HITSTOP_*`/`SHAKE_*` tiers), so capture footage is honest about the real
/// game feel. Pair with `FOREST_ORKLINE` (parked orks) + `FOREST_TPS`/`FOREST_CLIP`. No effect in
/// normal play.
fn hit_test(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    mut commands: Commands,
    mut orks: Query<
        (Entity, &GlobalTransform, &mut Ork, Option<&mut HitSquash>),
        Without<crate::dying::Dying>,
    >,
    mut floats: ResMut<FloatQueue>,
    mut hitstop: ResMut<crate::player::HitStop>,
    mut feedback: ResMut<HitFeedback>,
    mut t: Local<f32>,
    mut tier: Local<u32>,
) {
    if std::env::var("FOREST_HITTEST").is_err() {
        return;
    }
    *t -= time.delta_secs();
    if *t > 0.0 {
        return;
    }
    *t = 0.6;
    let now = time.elapsed_secs();
    let mut best: Option<(Entity, Vec3, f32)> = None;
    for (e, gt, _, _) in &orks {
        let p = gt.translation();
        let d = Vec2::new(p.x - hero.pos.x, p.z - hero.pos.y).length();
        if best.is_none_or(|b| d < b.2) {
            best = Some((e, p, d));
        }
    }
    let Some((e, p, _)) = best else { return };
    // Blow direction hero→ork (the shove axis); camera recoils back along it like a real swing.
    let dir = Vec2::new(p.x - hero.pos.x, p.z - hero.pos.y).normalize_or_zero();
    let recoil = -dir;
    // Step 3 stages a DIRECTED KILL (topples away from the hero) so a clip shows the death money-shot
    // + proves the corpse stops colliding; steps 0–2 cycle the light/crit/heavy hit reaction.
    if *tier % 4 == 3 {
        *tier += 1;
        crate::dying::begin_dying_struck(&mut commands, e, now, dir, true);
        floats.0.push(FloatReq { world: p + Vec3::Y * 2.2, text: "†".into(), color: col_kill(), scale: 1.4 });
        feedback.shake_dir = recoil;
        feedback.trauma = (feedback.trauma + crate::player::SHAKE_KILL).min(1.0);
        add_fov_kick(&mut feedback, FOV_KICK_KILL);
        hitstop.remaining = hitstop.remaining.max(crate::player::HITSTOP_KILL);
        return;
    }
    let (intensity, amp, label, heavy) = match *tier % 4 {
        0 => (0.34, 0.10, "hit", false),
        1 => (0.65, 0.14, "CRIT", false),
        _ => (0.95, 0.17, "HEAVY", true),
    };
    *tier += 1;
    let crit = label == "CRIT";
    let mut squash = None;
    if let Ok((_, _, mut o, sq)) = orks.get_mut(e) {
        o.hit_recoil = now;
        // The shove — same push the real hit site applies (crit/heavy shove harder).
        o.kb = dir * if crit || heavy { crate::player::KNOCKBACK_CRIT } else { crate::player::KNOCKBACK };
        if heavy || crit {
            o.atk_anim = 0.0; // stagger: cancel any wind-up
        }
        squash = sq;
    }
    commands.entity(e).try_insert(HurtFlash::new(now, intensity));
    // Re-kick in place (see the drive-squash rest-scale note) so repeated staged hits never
    // re-capture a mid-squash scale as rest and strand the ork flat.
    match squash {
        Some(mut s) => s.restart(now, amp),
        None => {
            commands.entity(e).try_insert(HitSquash::new(now, amp, false));
        }
    }
    floats.0.push(FloatReq { world: p + Vec3::Y * 2.2, text: label.into(), color: col_ork_hit(), scale: 1.2 });
    // Camera + clock: the same tiered punch `player_attack` lands.
    feedback.shake_dir = recoil;
    let (shake, kick, stop) = if heavy {
        (crate::player::SHAKE_HEAVY, crate::player::FOV_KICK_HEAVY, crate::player::HITSTOP_HEAVY)
    } else if crit {
        (crate::player::SHAKE_CRIT, FOV_KICK_CRIT, crate::player::HITSTOP_CRIT)
    } else {
        (crate::player::SHAKE_HIT, FOV_KICK_HIT, crate::player::HITSTOP_HIT)
    };
    feedback.trauma = (feedback.trauma + shake).min(1.0);
    add_fov_kick(&mut feedback, kick);
    hitstop.remaining = hitstop.remaining.max(stop);
}

/// Capture hook: `FOREST_HURT_ALL=<frac>` (e.g. `0.6`) wounds every hittable foe (ork / animal /
/// rival soldier / snowman / town NPC) to `frac` of its max HP, then knocks it down a further
/// [`HURT_TEST_STEP`] every [`HURT_TEST_PERIOD`]s for a few steps before healing it back — each foe
/// on its own phase — so a still always frames enemy HP bars mid-fight, some with a fresh damage
/// trail still draining. The hero is held on a matching staircase from 72% (the HUD vitals show a
/// partial fill + their own trail, safely above the low-HP dread grade, and he can't die mid-shot).
/// Only writes on a step change, so real blows still land in between. No effect in normal play.
#[allow(clippy::type_complexity)]
fn hurt_all_test(
    time: Res<Time>,
    mut player: ResMut<crate::player::PlayerRes>,
    mut foes: Query<
        (Entity, &mut Health),
        (
            Or<(
                With<Ork>,
                With<crate::wildlife::Animal>,
                With<crate::rival::RivalSoldier>,
                With<crate::snowman::Snowman>,
            )>,
            Without<crate::dying::Dying>,
        ),
    >,
    mut folk: Query<(Entity, &mut crate::villagers::NpcHp)>,
    mut frac: Local<Option<Option<f32>>>,
    mut steps: Local<std::collections::HashMap<Entity, i32>>,
    mut hero_step: Local<i32>,
) {
    let Some(frac) = *frac.get_or_insert_with(|| {
        std::env::var("FOREST_HURT_ALL").ok().and_then(|v| v.trim().parse::<f32>().ok()).map(|f| f.clamp(0.2, 0.98))
    }) else {
        return;
    };
    let t = time.elapsed_secs();
    // Step index 0..HURT_TEST_STEPS on a per-entity phase (so the bars never all move in unison).
    let step_of = |e: Entity| {
        let phase = ((e.to_bits() as u32).wrapping_mul(2_654_435_761) >> 16) as f32 / 65_536.0;
        ((t / HURT_TEST_PERIOD + phase * HURT_TEST_STEPS as f32) as i32).rem_euclid(HURT_TEST_STEPS)
    };
    let level = |k: i32| (frac - HURT_TEST_STEP * k as f32).max(0.08);
    for (e, mut hp) in &mut foes {
        let k = step_of(e);
        if steps.insert(e, k) != Some(k) {
            hp.hp = hp.max * level(k);
        }
    }
    for (e, mut hp) in &mut folk {
        let k = step_of(e);
        if steps.insert(e, k) != Some(k) {
            hp.hp = hp.max * level(k);
        }
    }
    let k = ((t / HURT_TEST_PERIOD) as i32).rem_euclid(HURT_TEST_STEPS);
    let p = &mut player.0;
    if k != *hero_step || p.hp > p.max_hp * 0.75 || p.hp < p.max_hp * 0.4 {
        *hero_step = k;
        p.hp = p.max_hp * (0.72 - 0.08 * k as f64);
    }
}
/// `FOREST_HURT_ALL` staircase: HP lost per step (fraction of max), seconds per step, steps/cycle.
const HURT_TEST_STEP: f32 = 0.09;
const HURT_TEST_PERIOD: f32 = 1.3;
const HURT_TEST_STEPS: i32 = 4;

// ── 2. Enemy HP bars (billboard follower entities, one shader quad each) ────
//
// Each bar is ONE quad drawn by `assets/shaders/hp_bar.wesl` (rim + iron frame + channel + blood
// fill + damage trail, all in the fragment shader). Every bar shares one mesh + one material; the
// per-bar values (hp, trail, hit flash) ride the instance `MeshTag`, so the renderer instances
// the whole horde's bars into a single draw, and a damage tick rewrites one `u32` — never a
// material (the old two-quad bar swapped material handles on every hurt-flash).

/// The bar's look — the shared vitals palette (`ui::theme`), linear, as the shader uniform.
#[derive(Clone, Copy, ShaderType, Debug)]
pub struct HpBarStyle {
    fill_hi: Vec4,
    fill_lo: Vec4,
    trail: Vec4,
    frame: Vec4,
    empty: Vec4,
}

/// The enemy-bar material: opaque + unlit (see the shader header for why opaque).
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct HpBarMaterial {
    #[uniform(0)]
    style: HpBarStyle,
}

impl Material for HpBarMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/hp_bar.wesl".into()
    }
}

#[derive(Resource)]
struct HpBarAssets {
    quad: Handle<Mesh>,
    mat: Handle<HpBarMaterial>,
}

/// Marks an ork as already having a follower bar (so we attach exactly one).
#[derive(Component)]
struct HasHpBar;

/// A follower bar that tracks `ork` (any hittable: ork, animal, or town NPC) and despawns when it
/// dies.
#[derive(Component)]
pub(crate) struct HpBar {
    ork: Entity,
    /// World-Y the bar floats at above the tracked entity — creatures sit higher than townsfolk.
    y: f32,
    /// The delayed-damage ghost (same state machine as the HUD bars).
    trail: crate::ui::vitals::DamageTrail,
}

/// Pack a bar's per-instance state for `hp_bar.wgsl`: hp + trail fractions (12 bits each) and the
/// hit flash (8 bits).
fn pack_bar_tag(hp: f32, trail: f32, flash: f32) -> u32 {
    let q12 = |v: f32| (v.clamp(0.0, 1.0) * 4095.0).round() as u32;
    let q8 = (flash.clamp(0.0, 1.0) * 255.0).round() as u32;
    q12(hp) | q12(trail) << 12 | q8 << 24
}

fn setup_hp_bar_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<HpBarMaterial>>,
) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0).mesh().build());
    // Unlit colours go straight to the tonemapper (TonyMcMapface), which deepens + desaturates the
    // UI-tuned sRGB palette a touch — `HP_BAR_GAIN` lifts it back so the world bar matches the HUD.
    let lin = |c: Color| (LinearRgba::from(c) * HP_BAR_GAIN).to_vec4();
    let style = HpBarStyle {
        fill_hi: lin(crate::ui::theme::HP_TOP),
        fill_lo: lin(crate::ui::theme::HP_BOT),
        trail: lin(crate::ui::theme::BAR_TRAIL),
        frame: lin(crate::ui::theme::BAR_FRAME),
        empty: lin(crate::ui::theme::BAR_EMPTY),
    };
    let mat = materials.add(HpBarMaterial { style });
    commands.insert_resource(HpBarAssets { quad, mat });
}

/// Spawn one follower bar tracking `e`, floating at world-Y `y`. Shared by every hittable
/// (creatures via `Health`, town NPCs via `NpcHp`). `try_insert` per the despawn-race convention:
/// the target can be despawned (biome rebuild, a defender bolt) the same frame it's first seen.
fn spawn_hp_bar(commands: &mut Commands, assets: &HpBarAssets, e: Entity, y: f32) {
    commands.entity(e).try_insert(HasHpBar);
    commands.spawn((
        Mesh3d(assets.quad.clone()),
        MeshMaterial3d(assets.mat.clone()),
        Transform::default(),
        Visibility::Hidden,
        MeshTag::new(pack_bar_tag(1.0, 1.0, 0.0)),
        HpBar { ork: e, y, trail: crate::ui::vitals::DamageTrail::new(1.0) },
        bevy::light::NotShadowCaster,
        crate::biome::BiomeEntity,
    ));
}

#[allow(clippy::type_complexity)]
fn ensure_hp_bars(
    mut commands: Commands,
    assets: Res<HpBarAssets>,
    // `With<Health>`: only attach a bar once the target's vitals exist. `ensure_combat_health`
    // (which adds `Health`) is gated on `Modal::None`, while this runs ungated — without this
    // filter a bar could be born a frame before `Health`, then `drive_hp_bars` (whose query
    // *requires* the vitals) would fail its lookup and despawn the bar for good (`HasHpBar`
    // lingers, so it never respawns). That silently killed every enemy's bar spawned at startup.
    // Split so the enlarged orks get the bumped float while same-scale animals keep the base height.
    orks: Query<Entity, (With<Ork>, With<Health>, Without<HasHpBar>)>,
    animals: Query<Entity, (With<crate::wildlife::Animal>, With<Health>, Without<HasHpBar>)>,
    // Town NPCs carry their HP in `NpcHp` (the whole `Townsfolk` pool — guards + workers), so they
    // float the same bar as any creature, just lower (their rig is shorter). It only shows when
    // wounded, exactly like the orks'.
    folk: Query<Entity, (With<crate::villagers::NpcHp>, Without<HasHpBar>)>,
    // Rival soldiers are biped guards carrying `Health` (not `NpcHp`) — give them the same bar as
    // the townsfolk so the player can read a guard's HP as they whittle it down.
    rivals: Query<Entity, (With<crate::rival::RivalSoldier>, With<Health>, Without<HasHpBar>)>,
    // Snowmen are full HP enemies (`Health` + `Snowman`) the hero fights — give them a bar too so
    // they read as foes, not decor, while you whittle them down.
    snowmen: Query<Entity, (With<crate::snowman::Snowman>, With<Health>, Without<HasHpBar>)>,
    // The castle gates (`gates::GateHp`): a bar over the gate while the orks hack it.
    gates: Query<Entity, (With<crate::gates::GateHp>, Without<HasHpBar>)>,
) {
    // Orks + townsfolk were enlarged by WORLD_BUMP (animals were not), so lift their bars to match
    // the taller heads. The base sits at y=0 and scales about it, so head height scales by the bump.
    let bump = crate::castle::WORLD_BUMP;
    for e in &orks {
        spawn_hp_bar(&mut commands, &assets, e, HP_BAR_Y * bump);
    }
    for e in &animals {
        spawn_hp_bar(&mut commands, &assets, e, HP_BAR_Y);
    }
    for e in &folk {
        spawn_hp_bar(&mut commands, &assets, e, HP_BAR_Y_NPC * bump);
    }
    for e in &rivals {
        spawn_hp_bar(&mut commands, &assets, e, HP_BAR_Y_NPC * bump);
    }
    for e in &snowmen {
        spawn_hp_bar(&mut commands, &assets, e, HP_BAR_Y);
    }
    for e in &gates {
        spawn_hp_bar(&mut commands, &assets, e, 2.4);
    }
}

#[allow(clippy::type_complexity)]
fn drive_hp_bars(
    time: Res<Time>,
    mut commands: Commands,
    cam_q: Query<(&GlobalTransform, &Projection), With<Camera3d>>,
    // Vitals come from `Health` (orks/animals) OR `NpcHp` (town NPCs) — whichever the target carries.
    targets: Query<
        (
            &GlobalTransform,
            Option<&Health>,
            Option<&crate::villagers::NpcHp>,
            Option<&crate::dying::Dying>,
            Option<&crate::gates::GateHp>,
        ),
        Or<(
            With<Ork>,
            With<crate::wildlife::Animal>,
            With<crate::villagers::NpcHp>,
            With<crate::rival::RivalSoldier>,
            With<crate::snowman::Snowman>,
            With<crate::gates::GateHp>,
        )>,
    >,
    mut bars: Query<(Entity, &mut HpBar, &mut Transform, &mut Visibility, &mut MeshTag)>,
    reticle_target: Option<Res<crate::player::ReticleTarget>>,
    fp: Option<Res<crate::player::FirstPerson>>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.1);
    let in_fp = fp.is_some_and(|f| f.active && f.blend > 0.5);
    // In first person the ringed foe's health lives under the crosshair (`viewmodel::sync_reticle`),
    // so its world-space bar — which would stripe across its chest right behind the reticle — hides.
    let reticle_foe = reticle_target.and_then(|t| t.0);
    let Ok((cam_tf, proj)) = cam_q.single() else { return };
    let cam_pos = cam_tf.translation();
    let cam_fwd = cam_tf.forward().as_vec3();
    // World units per screen-height at 1u of depth (vertical FOV), for the screen-constant size.
    let view_h_per_unit = match proj {
        Projection::Perspective(p) => 2.0 * (p.fov * 0.5).tan(),
        _ => 0.93,
    };
    for (bar_e, mut bar, mut tf, mut vis, mut mesh_tag) in &mut bars {
        let Ok((ork_gt, health, npc_hp, dying, gate_hp)) = targets.get(bar.ork) else {
            commands.entity(bar_e).try_despawn();
            continue;
        };
        // (hp, max) from whichever vitals component the target carries.
        let (cur, max) = match (health, npc_hp, gate_hp) {
            (Some(h), _, _) => (h.hp, h.max),
            (_, Some(n), _) => (n.hp, n.max),
            (_, _, Some(g)) => (g.hp, g.max),
            _ => {
                commands.entity(bar_e).try_despawn();
                continue;
            }
        };
        let ratio = (cur / max).clamp(0.0, 1.0);
        // The damage ghost runs even while the bar is hidden (culled / behind the reticle), so it
        // never replays a stale drain when the bar comes back.
        bar.trail.step(ratio, now, dt);
        if dying.is_some() || reticle_foe == Some(bar.ork) {
            vis.set_if_neq(Visibility::Hidden); // no bar over a crumpling corpse / the reticle's foe
            continue;
        }
        if ratio >= 1.0 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let root = ork_gt.translation();
        let mut head = root + Vec3::Y * bar.y;
        // Distance cull FIRST, off the un-adjusted overhead point: a damaged enemy across the map
        // shouldn't float a bar in the sky, and the great majority of bars during a siege are out
        // here — so bail before paying for the cone/billboard math below. (The close-range cone can
        // only ever pull `head` DOWN toward the camera, and it only engages inside `HP_BAR_NEAR`,
        // far under the cull radius, so testing the raw overhead point culls the same set.)
        if head.distance(cam_pos) > HP_BAR_MAX_DIST {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        // Close-range readability: as the camera closes inside HP_BAR_NEAR the bar SLIDES DOWN
        // from overhead to the foe's CHEST. (First person never reaches that branch — arm's-length
        // foes carry no world bar there, see below.) A view cone above the eye catches the
        // FP mid-range in between, and any lowered bar is pulled toward the camera so the depth
        // test doesn't bury it inside the torso.
        let flat = Vec2::new(head.x - cam_pos.x, head.z - cam_pos.z).length();
        // First-person melee: the eye is inside the scrum, so a bar pulled to the lens would fill
        // the screen. Foes at arm's length carry none (the reticle shows the ringed foe's health).
        if in_fp && flat < HP_BAR_NEAR {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let closeness = ((HP_BAR_NEAR - flat) / HP_BAR_NEAR).clamp(0.0, 1.0);
        let chest = root.y + bar.y * 0.55;
        let mut want_y = head.y - (head.y - chest) * closeness;
        let max_y = cam_pos.y + flat * HP_BAR_CONE_SLOPE;
        want_y = want_y.min(max_y).max(chest.min(head.y));
        if want_y < head.y {
            head.y = want_y;
            let pull = closeness * 0.8;
            head += (cam_pos - head).normalize_or_zero() * pull;
        }
        // View-space DEPTH, not Euclidean distance: projected size goes with depth, so scaling by the
        // straight-line distance would balloon bars toward the frame's edges.
        let depth = (head - cam_pos).dot(cam_fwd).max(0.05);
        vis.set_if_neq(Visibility::Visible);
        tf.translation = head;
        // Billboard with the quad's FRONT (+Z, uv-x running left→right on screen) toward the lens.
        tf.look_at(head * 2.0 - cam_pos, Vec3::Y);
        // Screen-constant size across the fight range (a thin crisp line at any distance — never a
        // slab across the frame up close, never a speck far off); past `HP_BAR_CONST_DIST` it eases
        // into half-rate perspective so a far foe's bar still reads as farther away.
        let d_eff = if depth <= HP_BAR_CONST_DIST { depth } else { HP_BAR_CONST_DIST + (depth - HP_BAR_CONST_DIST) * 0.5 };
        let h = HP_BAR_SCREEN_H * view_h_per_unit * d_eff;
        tf.scale = Vec3::new(h * HP_BAR_ASPECT, h, 1.0);
        // Only WRITE the tag when it actually moves: assigning through `Mut` marks the component
        // changed whether or not the value differs, which re-extracts that mesh instance.
        let want = pack_bar_tag(ratio, bar.trail.shown, bar.trail.flash(now));
        if mesh_tag.value != want {
            mesh_tag.value = want;
        }
    }
}

// ── 3. Hurt-flash (per-entity material, cloned LAZILY) ──────────────────────
// Shared by orks AND wildlife: a struck target whitens for a beat. Both ship sharing ONE skin
// material per variant/herd so the renderer batches the whole horde into a few draw calls, and a
// flash has to touch one body alone — so the struck entity borrows a private clone of that skin
// **for the duration of the flash only**, then hands it back.
//
// This used to be eager: every ork and every animal got its own material clone the frame it
// spawned. That silently destroyed the batching it was built to protect — a 100-ork siege meant
// 100 unique materials × ~17 mesh children each, i.e. no instancing anywhere in the horde, all so
// that the handful being hit at any instant could flash. Now the clone is minted on the flash's
// first frame and freed when it rings out, so at rest the horde is back to one material.

/// A struck ork / animal flashes hot until this time (s); `intensity` is the per-hit peak emissive
/// (tiered by blow weight at the hit-site, ~0.3 light → ~0.95 heavy).
#[derive(Component)]
pub struct HurtFlash {
    pub until: f32,
    pub intensity: f32,
}

impl HurtFlash {
    /// Flash starting now at the given peak intensity.
    pub fn new(now: f32, intensity: f32) -> Self {
        HurtFlash { until: now + HURT_FLASH_DUR, intensity }
    }
}

/// The private skin a currently-flashing ork / animal has borrowed: `own` is the clone its mesh
/// children point at while it whitens, `shared` is the batched variant/herd skin they came from and
/// go back to when the flash rings out. Present ONLY for the length of a flash.
#[derive(Component)]
struct HurtSkin {
    own: Handle<crate::creature::CreatureMaterial>,
    shared: Handle<crate::creature::CreatureMaterial>,
}

/// Drive every hurt-flash: mint the private skin on the first frame, ramp its emissive down, then
/// restore the shared skin (re-batching the horde) and free the clone on the last.
///
/// The mesh materials are swapped through the query directly (not via `Commands`), so a flash
/// starts on the very frame the hit lands rather than a frame late.
///
/// **Death fade is untouched by this**: `dying.rs` crumples a corpse with a transform-only
/// animation *precisely because* the horde shares materials, so a `Dying` entity needs no private
/// skin. One that dies mid-flash simply keeps the clone until it rings out (or until the corpse is
/// reaped, which drops the last strong handle and frees the asset anyway).
fn hurt_flash(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    // Only orks + wildlife carry a shared creature skin this trick knows how to borrow (the old
    // eager `ensure_*_skin` pair covered exactly these two).
    flashing: Query<
        (Entity, &HurtFlash, Option<&HurtSkin>, &Children),
        Or<(With<Ork>, With<crate::wildlife::Animal>)>,
    >,
    mut child_mats: Query<&mut MeshMaterial3d<crate::creature::CreatureMaterial>>,
    eyes: Query<(), With<crate::orks::OrkEye>>,
) {
    let now = time.elapsed_secs();
    for (e, hf, skin, children) in &flashing {
        let remain = hf.until - now;
        if remain <= 0.0 {
            // Rung out: hand the shared skin back (the horde batches again) and drop the clone.
            if let Some(s) = skin {
                for &c in children {
                    if let Ok(mut m) = child_mats.get_mut(c) {
                        if m.0.id() == s.own.id() {
                            m.0 = s.shared.clone();
                        }
                    }
                }
                mats.remove(&s.own);
                commands.entity(e).try_remove::<HurtSkin>();
            }
            commands.entity(e).try_remove::<HurtFlash>();
            continue;
        }
        // Lazily borrow a private skin on the flash's FIRST frame.
        let own = match skin {
            Some(s) => s.own.clone(),
            None => {
                // Base skin = a NON-eye child's material (the shared body mat); the glowing eyes
                // keep their own emissive material untouched (else the whole ork would flash amber).
                // Wildlife has no eye children, so this filter is a no-op for a herd.
                let Some(shared) = children
                    .iter()
                    .filter(|c| eyes.get(*c).is_err())
                    .find_map(|c| child_mats.get(c).ok().map(|m| m.0.clone()))
                else {
                    continue;
                };
                let Some(base) = mats.get(&shared).cloned() else { continue };
                let own = mats.add(base);
                for &c in children {
                    if eyes.get(c).is_ok() {
                        continue; // leave the glowing eyes alone
                    }
                    if let Ok(mut m) = child_mats.get_mut(c) {
                        m.0 = own.clone();
                    }
                }
                // `try_insert`: an ork/animal (BiomeEntity) can be despawned the same frame it's
                // struck (biome rebuild / kill / eaten), which despawns its children too.
                commands.entity(e).try_insert(HurtSkin { own: own.clone(), shared });
                own
            }
        };
        // Front-loaded pop: bright at the contact frame, falls off fast (k² not linear) so it
        // punches then clears instead of plateauing. Warm-tinted + scaled by the per-hit intensity.
        let k = (remain / HURT_FLASH_DUR).clamp(0.0, 1.0);
        if let Some(mut m) = mats.get_mut(&own) {
            let v = k * k * hf.intensity;
            m.base.emissive =
                LinearRgba::rgb(v * HURT_FLASH_TINT[0], v * HURT_FLASH_TINT[1], v * HURT_FLASH_TINT[2]);
        }
    }
}

// ── 4. Hit squash-and-stretch ("spring") on a struck creature ───────────────

/// A struck (surviving) ork / animal briefly squashes flat then springs back past rest — the
/// cartoon "boing" that makes a landed blow read on the body itself, layered on the existing
/// knockback shove + `recoil_tilt` wobble. Scale-only, so it composes with the AI brains'
/// per-frame translation/rotation writes (which never touch scale). Inserted by hit-sites
/// (`player::combat`); a `Dying` crumple owns the transform from the killing blow on.
#[derive(Component)]
pub struct HitSquash {
    started: f32,
    /// Rest scale, captured on the first drive frame (roots bake a per-variant scale there,
    /// so the hit-site can't just assume `Vec3::ONE`).
    base: Option<Vec3>,
    /// Peak compression (fraction of rest height), tiered by blow weight at the hit-site.
    amp: f32,
    /// `true` = soft body (wildlife): a springy ring that bounces past rest — the cartoon "boing".
    /// `false` = armoured ork: a single damped *absorb* pulse that compresses on impact and eases
    /// back with NO overshoot — reads as weight/give, not a bounce (the old ring read cheap).
    springy: bool,
}

impl HitSquash {
    /// Squash starting now at the given peak amplitude; `springy` picks bounce vs absorb.
    pub fn new(now: f32, amp: f32, springy: bool) -> Self {
        HitSquash { started: now, base: None, amp, springy }
    }
    /// Re-kick an in-flight squash (rapid hits) without forgetting the true rest scale; keeps the
    /// HARDER of the two amplitudes so a heavy landing on a light one isn't softened.
    pub fn restart(&mut self, now: f32, amp: f32) {
        self.started = now;
        self.amp = self.amp.max(amp);
    }
}

/// How long the squash rings (s) — under the 0.45s swing so spam-clicks read as separate pops.
const SQUASH_DUR: f32 = 0.34;
/// Ring frequency (rad/s) for the SPRINGY (wildlife) bounce. Kept LOW on purpose: hit-stop freezes
/// virtual time for the first 0.05–0.09s of the squash, so the post-freeze compression has to last
/// several more frames to read — at the old 26 rad/s it decayed within ~2 frames and was invisible.
const SQUASH_FREQ: f32 = 14.0;

/// Drive each squash: a damped cosine that starts fully compressed at impact, springs PAST rest
/// on the rebound and settles back, with an anti-phase horizontal bulge so the body keeps
/// roughly constant volume. Restores the rest scale exactly when it rings out.
fn drive_hit_squash(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut HitSquash, &mut Transform), Without<crate::dying::Dying>>,
) {
    let now = time.elapsed_secs();
    for (e, mut sq, mut tf) in &mut q {
        let base = *sq.base.get_or_insert(tf.scale);
        let t = now - sq.started;
        if t >= SQUASH_DUR {
            tf.scale = base;
            commands.entity(e).try_remove::<HitSquash>();
            continue;
        }
        let k = 1.0 - t / SQUASH_DUR; // 1 at impact → 0 at ring-out (envelope)
        let w = if sq.springy {
            // Soft body: damped cosine — springs PAST rest on the rebound (the cartoon boing).
            (t * SQUASH_FREQ).cos() * sq.amp * k * k
        } else {
            // Armoured ork: a single absorb pulse — full compression at impact, eases back to rest
            // with NO overshoot. `k²` front-loads the give so the hit lands hard then settles.
            sq.amp * k * k
        };
        // +w on the horizontals (anti-phase bulge) keeps the body roughly constant volume.
        tf.scale = base * Vec3::new(1.0 + w * 0.6, 1.0 - w, 1.0 + w * 0.6);
    }
}

// ── 5. Hero hit feedback (red flash + screen shake) ─────────────────────────

/// Decaying feedback state. `flash` = red-overlay alpha; `trauma` drives the
/// camera shake; `fov_kick` is an additive FOV punch (degrees) — all read by `player::camera`.
#[derive(Resource, Default)]
pub struct HitFeedback {
    pub flash: f32,
    pub trauma: f32,
    /// Additive camera-FOV punch in DEGREES; eased back to 0 by [`drive_hit_flash`]. The old
    /// game's `fxStore` fovKick — a quick widen on a kill / hard landing for impact.
    pub fov_kick: f32,
    /// World-space XZ direction the next shake should bias ALONG (e.g. `-fwd` for a swing recoil
    /// — the camera kicks back the way the hero faced). Set at a hit site alongside `trauma`;
    /// `player::camera` reads it to steer the jitter. Zero (the default) = unbiased chaos shake.
    /// Decayed in lockstep with `trauma` (see [`drive_hit_flash`]) and zeroed once the shake stops,
    /// so an undirected trauma source (chest open, taking a hit) can't inherit a stale swing axis.
    pub shake_dir: Vec2,
}

/// FOV-punch decay (deg/s) + cap, and the per-event punch magnitudes (old `fovTunables`,
/// nudged up for forest's wider 50° base).
const FOV_KICK_DECAY: f32 = 26.0;
const FOV_KICK_MAX: f32 = 7.0;
pub const FOV_KICK_KILL: f32 = 1.6;
/// A crit lands between a normal hit and a kill — a clear extra punch so a critical *reads* as one.
pub const FOV_KICK_CRIT: f32 = 1.0;
pub const FOV_KICK_HIT: f32 = 0.5;
pub const FOV_KICK_LAND: f32 = 1.4;

/// Add an FOV punch (degrees), capped — call from a hit/kill/landing site.
pub fn add_fov_kick(fb: &mut HitFeedback, deg: f32) {
    fb.fov_kick = (fb.fov_kick + deg).min(FOV_KICK_MAX);
}

/// Decay the hit-feedback channels. The hit *wince* is now rendered as a mature edge-vignette +
/// desaturation by `grade.rs` (off the hero's `hurt_flash_until`), so there is no more flat
/// full-screen red overlay — this just bleeds `flash`/`trauma` down each frame; `trauma` still
/// drives the camera shake (read by `player::camera`).
fn drive_hit_flash(time: Res<Time>, mut fb: ResMut<HitFeedback>) {
    let dt = time.delta_secs();
    fb.flash = (fb.flash - dt * 1.6).max(0.0);
    fb.trauma = (fb.trauma - dt * SHAKE_DECAY).max(0.0);
    fb.fov_kick = (fb.fov_kick - dt * FOV_KICK_DECAY).max(0.0);
    // Bleed the shake bias down with `trauma` and clear it once the shake stops, so a later
    // undirected trauma (chest open, taking a hit) can't reuse the previous swing's axis.
    fb.shake_dir *= (1.0 - dt * SHAKE_DECAY).max(0.0);
    if fb.trauma <= 0.0 {
        fb.shake_dir = Vec2::ZERO;
    }
}

// ── Plugin ──────────────────────────────────────────────────────────────────

pub struct CombatFxPlugin;

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<HpBarMaterial>::default())
            .init_resource::<FloatQueue>()
            .init_resource::<HitFeedback>()
            .add_systems(Startup, setup_hp_bar_assets)
            .add_systems(
                Update,
                (
                    float_test,
                    hit_test,
                    hurt_all_test,
                    spawn_floats,
                    drive_floats,
                    hurt_flash,
                    drive_hit_squash,
                    ensure_hp_bars,
                    drive_hp_bars,
                    drive_hit_flash,
                ),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The packing must match `hp_bar.wgsl`'s decode: hp in bits 0..11, trail 12..23, flash 24..31.
    #[test]
    fn bar_tag_layout_matches_the_shader() {
        let t = pack_bar_tag(0.5, 0.75, 1.0);
        assert_eq!(t & 0xfff, 2048);
        assert_eq!((t >> 12) & 0xfff, 3071);
        assert_eq!(t >> 24, 255);
        // Out-of-range input clamps instead of bleeding into the neighbouring field.
        let c = pack_bar_tag(1.7, -0.3, 2.0);
        assert_eq!(c & 0xfff, 4095);
        assert_eq!((c >> 12) & 0xfff, 0);
        assert_eq!(c >> 24, 255);
    }
}
