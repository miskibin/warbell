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

// ── Tuning (ported from the originals) ──────────────────────────────────────
/// Floating-number lifetime (s).
const FLOAT_LIFE: f32 = 1.1;
/// World-units a number rises over its life.
const FLOAT_RISE: f32 = 1.3;
/// Base font px for a scale-1 number.
const FLOAT_FONT: f32 = 22.0;
/// White-flash duration on a struck ork (s).
const HURT_FLASH_DUR: f32 = 0.12;
/// Trauma shed per second (old `TRAUMA_DECAY`).
const SHAKE_DECAY: f32 = 2.4;
/// Camera offset (world units) at full trauma.
pub const SHAKE_MAX: f32 = 0.35;
/// HP-bar quad size (world units), at the ork's baked scale.
const HP_BAR_W: f32 = 0.9;
const HP_BAR_H: f32 = 0.12;
/// Height above the ork root the bar floats at.
const HP_BAR_Y: f32 = 2.4;

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

fn spawn_floats(mut commands: Commands, time: Res<Time>, mut q: ResMut<FloatQueue>) {
    let now = time.elapsed_secs();
    for r in q.0.drain(..) {
        let len = r.text.chars().count();
        commands.spawn((
            Text::new(r.text),
            TextFont { font_size: FLOAT_FONT * r.scale, ..default() },
            TextColor(r.color),
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
    mut q: Query<(Entity, &FloatText, &mut Node, &mut TextColor, &mut TextFont)>,
) {
    let now = time.elapsed_secs();
    let Ok((cam, cam_tf)) = cam_q.single() else { return };
    for (e, f, mut node, mut tc, mut tf) in &mut q {
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
        tc.0 = f.color.with_alpha(1.0 - k * k);
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
    orks: Query<Entity, (Or<(With<Ork>, With<crate::wildlife::Animal>)>, Without<HasHpBar>)>,
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
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.001))
                        .with_scale(Vec3::new(HP_BAR_W + 0.05, HP_BAR_H + 0.03, 1.0)),
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
    orks: Query<(&GlobalTransform, &Health, Option<&HurtFlash>), Or<(With<Ork>, With<crate::wildlife::Animal>)>>,
    mut bars: Query<(Entity, &HpBar, &mut Transform, &mut Visibility, &Children)>,
    mut fgs: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), (With<HpBarFg>, Without<HpBar>)>,
) {
    let now = time.elapsed_secs();
    let Ok(cam_tf) = cam_q.single() else { return };
    let cam_pos = cam_tf.translation();
    for (bar_e, bar, mut tf, mut vis, children) in &mut bars {
        let Ok((ork_gt, hp, hurt)) = orks.get(bar.ork) else {
            commands.entity(bar_e).despawn();
            continue;
        };
        let ratio = (hp.hp / hp.max).clamp(0.0, 1.0);
        if ratio >= 1.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Visible;
        let head = ork_gt.translation() + Vec3::Y * HP_BAR_Y;
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

// ── 3. Ork hurt-flash (per-ork material, cloned externally) ─────────────────

/// A struck ork flashes white until this time (s).
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

/// The ork's own (cloned) skin material handle, so we can flash one ork without
/// touching the rest of the warband.
#[derive(Component)]
struct OrkSkin(Handle<StandardMaterial>);

/// Give every ork its own material clone (orks ship sharing one for batching).
/// Cheap — there are only dozens of orks — and keeps `orks.rs` untouched.
fn ensure_ork_skin(
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    orks: Query<(Entity, &Children), (With<Ork>, Without<OrkSkin>)>,
    child_mats: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    for (e, children) in &orks {
        let Some(shared) = children.iter().find_map(|c| child_mats.get(c).ok()) else { continue };
        let Some(base) = mats.get(&shared.0).cloned() else { continue };
        let own = mats.add(base);
        for &c in children {
            if child_mats.get(c).is_ok() {
                // `try_insert`: an ork (BiomeEntity) can be despawned the same frame it's first
                // seen here (biome rebuild / kill), which despawns its children too.
                commands.entity(c).try_insert(MeshMaterial3d(own.clone()));
            }
        }
        commands.entity(e).try_insert(OrkSkin(own));
    }
}

fn ork_flash(
    time: Res<Time>,
    mut commands: Commands,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(Entity, &HurtFlash, &OrkSkin)>,
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
            m.emissive = LinearRgba::rgb(k * 2.0, k * 2.0, k * 2.0);
        }
    }
}

// ── 4. Hero hit feedback (red flash + screen shake) ─────────────────────────

/// Decaying feedback state. `flash` = red-overlay alpha; `trauma` drives the
/// camera shake (read by `player::camera`).
#[derive(Resource, Default)]
pub struct HitFeedback {
    pub flash: f32,
    pub trauma: f32,
}

#[derive(Component)]
struct RedFlash;

fn setup_red_flash(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.7, 0.05, 0.05, 0.0)),
        GlobalZIndex(10),
        RedFlash,
    ));
}

fn drive_hit_flash(
    time: Res<Time>,
    mut fb: ResMut<HitFeedback>,
    mut q: Query<&mut BackgroundColor, With<RedFlash>>,
) {
    let dt = time.delta_secs();
    fb.flash = (fb.flash - dt * 1.6).max(0.0);
    fb.trauma = (fb.trauma - dt * SHAKE_DECAY).max(0.0);
    if let Ok(mut bg) = q.single_mut() {
        bg.0 = Color::srgba(0.7, 0.05, 0.05, fb.flash);
    }
}

// ── Plugin ──────────────────────────────────────────────────────────────────

pub struct CombatFxPlugin;

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FloatQueue>()
            .init_resource::<HitFeedback>()
            .add_systems(Startup, (setup_hp_bar_assets, setup_red_flash))
            .add_systems(
                Update,
                (
                    spawn_floats,
                    drive_floats,
                    ensure_ork_skin,
                    ork_flash,
                    ensure_hp_bars,
                    drive_hp_bars,
                    drive_hit_flash,
                ),
            );
    }
}
