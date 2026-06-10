//! Combat juice — floating damage numbers, ork HP bars, ork hurt-flash, and hero
//! hit feedback (screen shake + red flash). Ported from the original game's
//! `fxStore.ts` / `FloatingText.tsx` / `Ork.tsx`.
//!
//! Everything attaches to orks **externally** (the same philosophy as
//! `combat::Health`): hit-sites only push to [`FloatQueue`], set [`HitFeedback`],
//! or insert [`HurtFlash`] — this module does all the rendering and even clones a
//! per-ork material on the fly, so the shared ork model code stays untouched.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

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
/// White-flash duration on a struck ork (s).
const HURT_FLASH_DUR: f32 = 0.12;
/// Peak emissive of the hurt-flash. Deliberately FAINT: a bright white flash (0.8, then 0.28)
/// strobed under rapid hits and masked the squash/recoil body language — at 0.12 it's a glint
/// that confirms the hit while the pose does the talking.
const HURT_FLASH_PEAK: f32 = 0.12;
/// Trauma shed per second (old `TRAUMA_DECAY`).
const SHAKE_DECAY: f32 = 2.4;
/// Camera offset (world units) at full trauma.
pub const SHAKE_MAX: f32 = 0.35;
/// HP-bar quad size (world units), at the ork's baked scale.
const HP_BAR_W: f32 = 0.6;
const HP_BAR_H: f32 = 0.085;
/// Height above the ork root the bar floats at.
const HP_BAR_Y: f32 = 1.9;
/// Beyond this camera distance a bar is hidden — keeps far-off damaged enemies from floating
/// bars across the sky (they read as detached when the body's behind trees / off-screen).
const HP_BAR_MAX_DIST: f32 = 26.0;

// ── Ported float colours ────────────────────────────────────────────────────
/// Red `-N` when the hero is struck (`#ff5a4a`).
pub fn col_hero_hit() -> Color {
    Color::srgb(1.0, 0.353, 0.290)
}
/// Blue `BLOCK` (`#bcd4ff`).
pub fn col_block() -> Color {
    Color::srgb(0.737, 0.831, 1.0)
}
/// White `N` on a struck enemy.
pub fn col_ork_hit() -> Color {
    Color::WHITE
}
/// Green `☠` on a kill (`#9be38a`).
pub fn col_kill() -> Color {
    Color::srgb(0.608, 0.890, 0.541)
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
    len: usize,
}

fn spawn_floats(
    mut commands: Commands,
    time: Res<Time>,
    fonts: Res<UiFonts>,
    mut q: ResMut<FloatQueue>,
) {
    let now = time.elapsed_secs();
    for r in q.0.drain(..) {
        let len = r.text.chars().count();
        commands.spawn((
            Text::new(r.text),
            TextFont { font: fonts.extrabold.clone(), font_size: FLOAT_FONT * r.scale, ..default() },
            TextColor(r.color),
            // A crisp dark drop shadow so numbers pop against the bright scene (fades in drive_floats).
            TextShadow { offset: Vec2::new(0.0, 2.5), color: Color::srgba(0.0, 0.0, 0.0, FLOAT_SHADOW_A) },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-9999.0),
                top: Val::Px(-9999.0),
                ..default()
            },
            GlobalZIndex(20),
            FloatText { anchor: r.world, born: now, color: r.color, scale: r.scale, len },
        ));
    }
}

fn drive_floats(
    time: Res<Time>,
    mut commands: Commands,
    cam_q: Query<(&Camera, &GlobalTransform)>,
    mut q: Query<(Entity, &FloatText, &mut Node, &mut TextColor, &mut TextFont, &mut TextShadow)>,
) {
    let now = time.elapsed_secs();
    let Ok((cam, cam_tf)) = cam_q.single() else { return };
    for (e, f, mut node, mut tc, mut tf, mut shadow) in &mut q {
        let t = now - f.born;
        let k = (t / FLOAT_LIFE).clamp(0.0, 1.0);
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        // Pop in fast with a slight overshoot, then settle at 1 (old FloatingText).
        let pop =
            if t < 0.16 { 0.6 + (t / 0.16) * 0.55 } else { (1.15 - (t - 0.16) * 1.6).max(1.0) };
        let font = FLOAT_FONT * f.scale * pop;
        tf.font_size = font;
        let fade = 1.0 - k * k;
        tc.0 = f.color.with_alpha(fade);
        shadow.color = Color::srgba(0.0, 0.0, 0.0, FLOAT_SHADOW_A * fade);
        let world = f.anchor + Vec3::Y * (FLOAT_RISE * k);
        match cam.world_to_viewport(cam_tf, world) {
            Ok(px) => {
                node.display = Display::Flex;
                // Roughly centre the glyphs on the anchor point.
                node.left = Val::Px(px.x - font * 0.3 * f.len as f32);
                node.top = Val::Px(px.y - font * 0.5);
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
    let amber = Color::srgb(0.96, 0.78, 0.30);
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

// ── 2. Ork HP bars (billboard follower entities) ────────────────────────────

#[derive(Resource)]
struct HpBarAssets {
    quad: Handle<Mesh>,
    bg: Handle<StandardMaterial>,
    fg: Handle<StandardMaterial>,
    fg_hurt: Handle<StandardMaterial>,
}

/// Marks an ork as already having a follower bar (so we attach exactly one).
#[derive(Component)]
struct HasHpBar;

/// A follower bar that tracks `ork` and despawns when it dies.
#[derive(Component)]
struct HpBar {
    ork: Entity,
}

/// The shrinking foreground quad inside a bar.
#[derive(Component)]
struct HpBarFg;

fn setup_hp_bar_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0).mesh().build());
    let unlit = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        cull_mode: None, // double-sided so the billboard is visible from either face
        ..default()
    };
    let bg = materials.add(unlit(Color::srgba(0.0, 0.0, 0.0, 0.7)));
    let fg = materials.add(unlit(Color::srgb(0.839, 0.227, 0.227))); // #d63a3a
    let fg_hurt = materials.add(unlit(Color::srgb(1.0, 0.667, 0.125))); // #ffaa20
    commands.insert_resource(HpBarAssets { quad, bg, fg, fg_hurt });
}

#[allow(clippy::type_complexity)]
fn ensure_hp_bars(
    mut commands: Commands,
    assets: Res<HpBarAssets>,
    // `With<Health>`: only attach a bar once the target's vitals exist. `ensure_combat_health`
    // (which adds `Health`) is gated on `Modal::None`, while this runs ungated — without this
    // filter a bar could be born a frame before `Health`, then `drive_hp_bars` (whose query
    // *requires* `&Health`) would fail its lookup and despawn the bar for good (`HasHpBar`
    // lingers, so it never respawns). That silently killed every enemy's bar spawned at startup.
    orks: Query<
        Entity,
        (Or<(With<Ork>, With<crate::wildlife::Animal>)>, With<Health>, Without<HasHpBar>),
    >,
) {
    for e in &orks {
        // `try_insert`: an ork can be despawned (biome rebuild, or a defender bolt reaping a
        // wave invader) the same frame it's first seen here — tolerate the gone entity.
        commands.entity(e).try_insert(HasHpBar);
        commands
            .spawn((Transform::default(), Visibility::Hidden, HpBar { ork: e }, crate::biome::BiomeEntity))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(assets.bg.clone()),
                    // +Z = AWAY from the camera (the billboard's forward/-Z faces the camera via
                    // `look_at`), so the dark panel sits BEHIND the red fill instead of hiding it.
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.01))
                        .with_scale(Vec3::new(HP_BAR_W + 0.04, HP_BAR_H + 0.02, 1.0)),
                    bevy::light::NotShadowCaster,
                ));
                p.spawn((
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(assets.fg.clone()),
                    Transform::from_scale(Vec3::new(HP_BAR_W, HP_BAR_H, 1.0)),
                    bevy::light::NotShadowCaster,
                    HpBarFg,
                ));
            });
    }
}

#[allow(clippy::type_complexity)]
fn drive_hp_bars(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<HpBarAssets>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    orks: Query<(&GlobalTransform, &Health, Option<&HurtFlash>, Option<&crate::dying::Dying>), Or<(With<Ork>, With<crate::wildlife::Animal>)>>,
    mut bars: Query<(Entity, &HpBar, &mut Transform, &mut Visibility, &Children)>,
    mut fgs: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), (With<HpBarFg>, Without<HpBar>)>,
) {
    let now = time.elapsed_secs();
    let Ok(cam_tf) = cam_q.single() else { return };
    let cam_pos = cam_tf.translation();
    for (bar_e, bar, mut tf, mut vis, children) in &mut bars {
        let Ok((ork_gt, hp, hurt, dying)) = orks.get(bar.ork) else {
            commands.entity(bar_e).despawn();
            continue;
        };
        if dying.is_some() {
            *vis = Visibility::Hidden; // no bar over a crumpling corpse
            continue;
        }
        let ratio = (hp.hp / hp.max).clamp(0.0, 1.0);
        if ratio >= 1.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        let head = ork_gt.translation() + Vec3::Y * HP_BAR_Y;
        // Cull distant bars so a damaged enemy across the map doesn't float a bar in the sky.
        if head.distance(cam_pos) > HP_BAR_MAX_DIST {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Visible;
        tf.translation = head;
        tf.look_at(cam_pos, Vec3::Y); // billboard (materials are double-sided)
        let hurting = hurt.is_some_and(|h| now < h.until);
        for &c in children {
            if let Ok((mut fg_tf, mut fg_mat)) = fgs.get_mut(c) {
                fg_tf.scale.x = HP_BAR_W * ratio;
                fg_tf.translation.x = -(1.0 - ratio) * HP_BAR_W / 2.0;
                fg_mat.0 = if hurting { assets.fg_hurt.clone() } else { assets.fg.clone() };
            }
        }
    }
}

// ── 3. Hurt-flash (per-entity material, cloned externally) ──────────────────
// Shared by orks AND wildlife: a struck target whitens for a beat. Both ship sharing one skin
// material (batching), so we clone a per-entity copy on the fly and flash only that copy.

/// A struck ork / animal flashes white until this time (s).
#[derive(Component)]
pub struct HurtFlash {
    pub until: f32,
}

impl HurtFlash {
    /// Flash starting now.
    pub fn new(now: f32) -> Self {
        HurtFlash { until: now + HURT_FLASH_DUR }
    }
}

/// A target's own (cloned) skin material handle, so we can flash one ork / animal without
/// touching the rest of the warband / herd (both ship sharing one material for batching).
#[derive(Component)]
struct HurtSkin(Handle<StandardMaterial>);

/// Give every ork its own material clone (orks ship sharing one for batching).
/// Cheap — there are only dozens of orks — and keeps `orks.rs` untouched.
fn ensure_ork_skin(
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    orks: Query<(Entity, &Children), (With<Ork>, Without<HurtSkin>)>,
    child_mats: Query<&MeshMaterial3d<StandardMaterial>>,
    eyes: Query<(), With<crate::orks::OrkEye>>,
) {
    for (e, children) in &orks {
        // Base skin = a NON-eye child's material (the shared white body mat); the glowing eyes
        // keep their own emissive material untouched (else the whole ork would flash amber).
        let Some(shared) =
            children.iter().filter(|c| eyes.get(*c).is_err()).find_map(|c| child_mats.get(c).ok())
        else {
            continue;
        };
        let Some(base) = mats.get(&shared.0).cloned() else { continue };
        let own = mats.add(base);
        for &c in children {
            if eyes.get(c).is_ok() {
                continue; // leave the glowing eyes alone
            }
            if child_mats.get(c).is_ok() {
                // `try_insert`: an ork (BiomeEntity) can be despawned the same frame it's first
                // seen here (biome rebuild / kill), which despawns its children too.
                commands.entity(c).try_insert(MeshMaterial3d(own.clone()));
            }
        }
        commands.entity(e).try_insert(HurtSkin(own));
    }
}

/// Same trick for wildlife: each animal gets its own clone of the shared white herd material so a
/// struck one flashes alone. No eyes to skip, so every mesh child takes the clone. Keeps
/// `wildlife.rs` untouched.
fn ensure_animal_skin(
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    animals: Query<(Entity, &Children), (With<crate::wildlife::Animal>, Without<HurtSkin>)>,
    child_mats: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    for (e, children) in &animals {
        let Some(shared) = children.iter().find_map(|c| child_mats.get(c).ok()) else { continue };
        let Some(base) = mats.get(&shared.0).cloned() else { continue };
        let own = mats.add(base);
        for &c in children {
            if child_mats.get(c).is_ok() {
                // `try_insert`: an animal (BiomeEntity) can be despawned the same frame it's first
                // seen here (biome rebuild / kill / eaten), which despawns its children too.
                commands.entity(c).try_insert(MeshMaterial3d(own.clone()));
            }
        }
        commands.entity(e).try_insert(HurtSkin(own));
    }
}

fn hurt_flash(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(Entity, &HurtFlash, &HurtSkin)>,
) {
    let now = time.elapsed_secs();
    for (e, hf, skin) in &q {
        let remain = hf.until - now;
        if remain <= 0.0 {
            if let Some(m) = mats.get_mut(&skin.0) {
                m.emissive = LinearRgba::BLACK;
            }
            commands.entity(e).remove::<HurtFlash>();
            continue;
        }
        let k = (remain / HURT_FLASH_DUR).clamp(0.0, 1.0);
        if let Some(m) = mats.get_mut(&skin.0) {
            // A subtle whiten, not a strobe — kept low so rapid hits don't blow out the model
            // (and so the squash/recoil pose stays readable through the flash).
            let v = k * HURT_FLASH_PEAK;
            m.emissive = LinearRgba::rgb(v, v, v);
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
}

impl HitSquash {
    /// Squash starting now.
    pub fn new(now: f32) -> Self {
        HitSquash { started: now, base: None }
    }
    /// Re-kick an in-flight squash (rapid hits) without forgetting the true rest scale.
    pub fn restart(&mut self, now: f32) {
        self.started = now;
    }
}

/// How long the squash rings (s) — under the 0.45s swing so spam-clicks read as separate pops.
const SQUASH_DUR: f32 = 0.34;
/// Peak vertical compression (fraction of rest height) at the moment of impact.
const SQUASH_AMP: f32 = 0.24;
/// Ring frequency (rad/s). Kept LOW on purpose: hit-stop freezes virtual time for the first
/// 0.05–0.09s of the squash, so the post-freeze compression has to last several more frames to
/// read — at the old 26 rad/s it decayed within ~2 frames and the effect was invisible.
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
        let k = 1.0 - t / SQUASH_DUR;
        let w = (t * SQUASH_FREQ).cos() * SQUASH_AMP * k * k;
        tf.scale = base * Vec3::new(1.0 + w * 0.7, 1.0 - w, 1.0 + w * 0.7);
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
}

/// FOV-punch decay (deg/s) + cap, and the per-event punch magnitudes (old `fovTunables`,
/// nudged up for forest's wider 50° base).
const FOV_KICK_DECAY: f32 = 26.0;
const FOV_KICK_MAX: f32 = 7.0;
pub const FOV_KICK_KILL: f32 = 1.6;
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
}

// ── Plugin ──────────────────────────────────────────────────────────────────

pub struct CombatFxPlugin;

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloatQueue>()
            .init_resource::<HitFeedback>()
            .add_systems(Startup, setup_hp_bar_assets)
            .add_systems(
                Update,
                (
                    float_test,
                    spawn_floats,
                    drive_floats,
                    ensure_ork_skin,
                    ensure_animal_skin,
                    hurt_flash,
                    drive_hit_squash,
                    ensure_hp_bars,
                    drive_hp_bars,
                    drive_hit_flash,
                ),
            );
    }
}
