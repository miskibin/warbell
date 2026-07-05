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
/// HP-bar quad size (world units), at the ork's baked scale.
const HP_BAR_W: f32 = 0.6;
const HP_BAR_H: f32 = 0.085;
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
            FloatText { anchor: r.world, born: now, color: r.color, scale: r.scale, len },
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
        let k = (t / FLOAT_LIFE).clamp(0.0, 1.0);
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
                node.top = Val::Px(px.y - base_font * 0.5);
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
    mut orks: Query<(Entity, &GlobalTransform, &mut Ork), Without<crate::dying::Dying>>,
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
    for (e, gt, _) in &orks {
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
    if let Ok((_, _, mut o)) = orks.get_mut(e) {
        o.hit_recoil = now;
        // The shove — same push the real hit site applies (crit/heavy shove harder).
        o.kb = dir * if crit || heavy { crate::player::KNOCKBACK_CRIT } else { crate::player::KNOCKBACK };
        if heavy || crit {
            o.atk_anim = 0.0; // stagger: cancel any wind-up
        }
    }
    commands.entity(e).try_insert(HurtFlash::new(now, intensity));
    commands.entity(e).try_insert(HitSquash::new(now, amp, false));
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

/// A follower bar that tracks `ork` (any hittable: ork, animal, or town NPC) and despawns when it
/// dies.
#[derive(Component)]
struct HpBar {
    ork: Entity,
    /// World-Y the bar floats at above the tracked entity — creatures sit higher than townsfolk.
    y: f32,
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

/// Spawn one follower bar tracking `e`, floating at world-Y `y`. Shared by every hittable
/// (creatures via `Health`, town NPCs via `NpcHp`). `try_insert` per the despawn-race convention:
/// the target can be despawned (biome rebuild, a defender bolt) the same frame it's first seen.
fn spawn_hp_bar(commands: &mut Commands, assets: &HpBarAssets, e: Entity, y: f32) {
    commands.entity(e).try_insert(HasHpBar);
    commands
        .spawn((Transform::default(), Visibility::Hidden, HpBar { ork: e, y }, crate::biome::BiomeEntity))
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
}

#[allow(clippy::type_complexity)]
fn drive_hp_bars(
    time: Res<Time>,
    mut commands: Commands,
    assets: Res<HpBarAssets>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    // Vitals come from `Health` (orks/animals) OR `NpcHp` (town NPCs) — whichever the target carries.
    targets: Query<
        (&GlobalTransform, Option<&Health>, Option<&crate::villagers::NpcHp>, Option<&HurtFlash>, Option<&crate::dying::Dying>),
        Or<(With<Ork>, With<crate::wildlife::Animal>, With<crate::villagers::NpcHp>, With<crate::rival::RivalSoldier>)>,
    >,
    mut bars: Query<(Entity, &HpBar, &mut Transform, &mut Visibility, &Children)>,
    mut fgs: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), (With<HpBarFg>, Without<HpBar>)>,
) {
    let now = time.elapsed_secs();
    let Ok(cam_tf) = cam_q.single() else { return };
    let cam_pos = cam_tf.translation();
    for (bar_e, bar, mut tf, mut vis, children) in &mut bars {
        let Ok((ork_gt, health, npc_hp, hurt, dying)) = targets.get(bar.ork) else {
            commands.entity(bar_e).try_despawn();
            continue;
        };
        // (hp, max) from whichever vitals component the target carries.
        let (cur, max) = match (health, npc_hp) {
            (Some(h), _) => (h.hp, h.max),
            (_, Some(n)) => (n.hp, n.max),
            _ => {
                commands.entity(bar_e).try_despawn();
                continue;
            }
        };
        if dying.is_some() {
            *vis = Visibility::Hidden; // no bar over a crumpling corpse
            continue;
        }
        let ratio = (cur / max).clamp(0.0, 1.0);
        if ratio >= 1.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        let head = ork_gt.translation() + Vec3::Y * bar.y;
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

/// A target's own (cloned) skin material handle, so we can flash one ork / animal without
/// touching the rest of the warband / herd (both ship sharing one material for batching).
#[derive(Component)]
struct HurtSkin(Handle<crate::creature::CreatureMaterial>);

/// Give every ork its own material clone (orks ship sharing one for batching).
/// Cheap — there are only dozens of orks — and keeps `orks.rs` untouched.
fn ensure_ork_skin(
    mut commands: Commands,
    mut mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    orks: Query<(Entity, &Children), (With<Ork>, Without<HurtSkin>)>,
    child_mats: Query<&MeshMaterial3d<crate::creature::CreatureMaterial>>,
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
    mut mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    animals: Query<(Entity, &Children), (With<crate::wildlife::Animal>, Without<HurtSkin>)>,
    child_mats: Query<&MeshMaterial3d<crate::creature::CreatureMaterial>>,
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
    mut mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    q: Query<(Entity, &HurtFlash, &HurtSkin)>,
) {
    let now = time.elapsed_secs();
    for (e, hf, skin) in &q {
        let remain = hf.until - now;
        if remain <= 0.0 {
            if let Some(mut m) = mats.get_mut(&skin.0) {
                m.base.emissive = LinearRgba::BLACK;
            }
            commands.entity(e).remove::<HurtFlash>();
            continue;
        }
        // Front-loaded pop: bright at the contact frame, falls off fast (k² not linear) so it
        // punches then clears instead of plateauing. Warm-tinted + scaled by the per-hit intensity.
        let k = (remain / HURT_FLASH_DUR).clamp(0.0, 1.0);
        if let Some(mut m) = mats.get_mut(&skin.0) {
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
        app.init_resource::<FloatQueue>()
            .init_resource::<HitFeedback>()
            .add_systems(Startup, setup_hp_bar_assets)
            .add_systems(
                Update,
                (
                    float_test,
                    hit_test,
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
