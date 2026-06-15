//! **Weapon arts** — the three active fighting moves unlocked by slaying biome wardens:
//! **Ground Slam** (G, from the Stone Golem), **Sand Dash** (T, from the Sand Revenant) and
//! **Bramble Sweep** (V, from the Treant). Each is gated on its `Player` boon flag + a cooldown,
//! and applies area damage to the same ork/animal/boss targets the normal swing hits (reusing
//! `Health`, the `Dying` fade, reward orbs, lifesteal, and the Frostbite/Venom on-hit boons).

use bevy::prelude::*;

use crate::audio::AudioCue;
use crate::game_state::AppState;
use crate::orks::Ork;
use crate::player::{spawn_burst, spawn_dash_trail, spawn_shockwave, spawn_sweep_burst, CombatFx, Health};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets;
use crate::wildlife::Animal;

use super::camera::OrbitCam;
use super::{Hero, HeroHealth, PlayMode, PlayerRes};

// Keybinds — the left-hand combat cluster (the consumable quick-slots moved to Q/Y/T).
const KEY_SLAM: KeyCode = KeyCode::KeyZ;
const KEY_DASH: KeyCode = KeyCode::KeyX;
const KEY_SWEEP: KeyCode = KeyCode::KeyC;

/// Every art spends a **fixed** amount of stamina (NOT a fraction of max) — so a higher-level hero
/// with a bigger stamina pool simply gets *more* casts before running dry, rather than the cost
/// scaling up with the pool. The arts draw from the same pool the block shield uses, so spamming
/// them leaves you unable to block. A short shared cooldown ([`ART_COOLDOWN`]) spaces casts out.
/// (Costs chosen to match the old fractions at the level-1 pool of 150: 0.6/0.4/0.5 × 150.)
const SLAM_STAMINA_COST: f32 = 90.0; // heavy bruiser — you commit
const DASH_STAMINA_COST: f32 = 60.0; // cheap dodge
const SWEEP_STAMINA_COST: f32 = 75.0;
/// Seconds of shared cooldown after any art fires, so a fat stamina pool can't dump several in one
/// instant — you still get a cast every half-second.
const ART_COOLDOWN: f32 = 0.5;

// Ground Slam — the HEAVY HITTER: biggest radius, biggest damage, hard knockback. Stay-put burst.
const SLAM_RADIUS: f32 = 4.2;
const SLAM_MULT: f32 = 3.0;
const SLAM_KNOCK: f32 = 18.0;

// Sand Dash — the DODGE: long blink, light damage, and brief invulnerability (i-frames) so you can
// blink straight through a boss swing.
const DASH_DIST: f32 = 7.0;
const DASH_HALF_WIDTH: f32 = 1.5;
const DASH_MULT: f32 = 1.0;
/// Seconds of invulnerability granted from the blink's start.
const DASH_IFRAME: f32 = 0.45;

// Bramble Sweep — the SUSTAIN: 360° cleave that heals the hero on cast AND for each foe struck.
const SWEEP_RADIUS: f32 = 2.8;
const SWEEP_MULT: f32 = 1.4;
/// Flat HP the hero heals every time the sweep is cast (even hitting nothing).
const SWEEP_BASE_HEAL: f32 = 28.0;
/// Extra HP healed per foe the sweep strikes.
const SWEEP_LIFESTEAL: f32 = 18.0;

/// Which art fired this frame (at most one).
enum Art {
    Slam,
    Dash { from: Vec2, to: Vec2 },
    Sweep,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn player_arts(
    time: Res<Time>,
    mode: Res<PlayMode>,
    keys: Res<ButtonInput<KeyCode>>,
    orbit: Res<OrbitCam>,
    fx: Option<Res<CombatFx>>,
    mut player: ResMut<PlayerRes>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    mut cues: MessageWriter<AudioCue>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut rewards: ResMut<crate::orbs::RewardBursts>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
    mut hero_q: Query<(&mut Hero, &mut Transform, &mut super::HeroHealth)>,
    mut targets: Query<
        (Entity, &GlobalTransform, &mut Health, Option<&Ork>, Option<&Animal>),
        (
            Or<(With<Ork>, With<Animal>, With<crate::boss::Boss>)>,
            Without<crate::dying::Dying>,
        ),
    >,
) {
    let Ok((mut hero, mut tf, mut hh)) = hero_q.single_mut() else { return };

    if *mode != PlayMode::Play || !player.0.is_alive() || !orbit.locked {
        return;
    }

    let now = time.elapsed_secs();
    // Pick the art that fired (one per frame; slam > dash > sweep priority on the rare same-frame).
    // Each spends a fixed stamina cost up-front; a shared cooldown gates the cadence.
    let off_cd = now >= hh.art_cd_until;
    let fwd = Vec2::new(hero.facing.sin(), hero.facing.cos());
    let art = if off_cd && player.0.has_ground_slam && hh.stamina >= SLAM_STAMINA_COST && keys.just_pressed(KEY_SLAM) {
        hh.stamina -= SLAM_STAMINA_COST;
        Some(Art::Slam)
    } else if off_cd && player.0.has_sand_dash && hh.stamina >= DASH_STAMINA_COST && keys.just_pressed(KEY_DASH) {
        hh.stamina -= DASH_STAMINA_COST;
        hh.iframe_until = now + DASH_IFRAME; // invulnerable through the blink
        // Dash forward to the furthest standable point along the heading.
        let from = hero.pos;
        let mut to = from;
        for k in 1..=6 {
            let cand = from + fwd * (DASH_DIST * k as f32 / 6.0);
            if crate::worldmap::ground_at_world(cand.x, cand.y).is_some() {
                to = cand;
            } else {
                break;
            }
        }
        hero.pos = to;
        if let Some(y) = crate::worldmap::ground_at_world(to.x, to.y) {
            tf.translation = Vec3::new(to.x, y, to.y);
        }
        Some(Art::Dash { from, to })
    } else if off_cd && player.0.has_bramble_sweep && hh.stamina >= SWEEP_STAMINA_COST && keys.just_pressed(KEY_SWEEP) {
        hh.stamina -= SWEEP_STAMINA_COST;
        Some(Art::Sweep)
    } else {
        None
    };
    let Some(art) = art else { return };
    hh.art_cd_until = now + ART_COOLDOWN; // start the shared cooldown on any successful cast

    let Some(fx) = fx else { return };
    cues.write(match art {
        Art::Dash { .. } => AudioCue::Dash,
        Art::Sweep => AudioCue::Sweep,
        Art::Slam => AudioCue::Slam,
    });
    let base = player.0.attack_damage as f32;
    let bounty_mult = player.0.bounty_mult;
    let lifesteal = player.0.lifesteal;
    let frostbite = player.0.frostbite;
    let venom = player.0.venom;

    // Per-art area test + damage + FX origin.
    let (dmg, origin) = match &art {
        Art::Slam => {
            let at = Vec3::new(hero.pos.x, hero.y + 0.05, hero.pos.y);
            spawn_shockwave(&mut commands, &fx, &mut materials, at, now);
            spawn_burst(&mut commands, &fx, Vec3::new(hero.pos.x, hero.y + 0.4, hero.pos.y), true);
            feedback.trauma = (feedback.trauma + 0.5).min(1.0);
            crate::combat_fx::add_fov_kick(&mut feedback, 1.8); // heavy landing punch
            ((base * SLAM_MULT).round() as f32, hero.pos)
        }
        Art::Sweep => {
            let at = Vec3::new(hero.pos.x, hero.y + 0.05, hero.pos.y);
            spawn_shockwave(&mut commands, &fx, &mut materials, at, now);
            // A 360° fling of green leaf/thorn motes riding the energy ring.
            spawn_sweep_burst(&mut commands, &fx, Vec3::new(hero.pos.x, hero.y, hero.pos.y));
            feedback.trauma = (feedback.trauma + 0.34).min(1.0);
            crate::combat_fx::add_fov_kick(&mut feedback, 2.0);
            ((base * SWEEP_MULT).round() as f32, hero.pos)
        }
        Art::Dash { from, to } => {
            // Teleport read: a dust afterimage strung along the blink, kick-off + landing puffs,
            // and a quick FOV zoom-punch.
            let y = hero.y;
            let from3 = Vec3::new(from.x, y + 0.5, from.y);
            let to3 = Vec3::new(to.x, y + 0.5, to.y);
            spawn_dash_trail(&mut commands, &fx, from3, to3);
            spawn_burst(&mut commands, &fx, from3, false);
            spawn_burst(&mut commands, &fx, to3, false);
            feedback.trauma = (feedback.trauma + 0.28).min(1.0);
            crate::combat_fx::add_fov_kick(&mut feedback, 3.0); // snap-zoom on the blink
            ((base * DASH_MULT).round() as f32, *from)
        }
    };

    let mut killed_any = false;
    // Bramble Sweep sustain: a flat self-heal on cast, plus more per foe struck (added in-loop).
    let mut sweep_heal = 0.0f32;
    if matches!(art, Art::Sweep) {
        player.0.heal(SWEEP_BASE_HEAL as f64);
        sweep_heal += SWEEP_BASE_HEAL;
    }
    for (e, gt, mut hp, ork, animal) in &mut targets {
        let p = gt.translation();
        let pt = Vec2::new(p.x, p.z);
        // In-area test per art.
        let (hit, knock_dir) = match &art {
            Art::Slam => (origin.distance(pt) <= SLAM_RADIUS, (pt - origin).normalize_or_zero()),
            Art::Sweep => (origin.distance(pt) <= SWEEP_RADIUS, (pt - origin).normalize_or_zero()),
            Art::Dash { from, to } => {
                let d = dist_point_segment(pt, *from, *to);
                (d <= DASH_HALF_WIDTH, (*to - *from).normalize_or_zero())
            }
        };
        if !hit {
            continue;
        }
        hp.hp -= dmg;
        // Sweep lifesteal — heal for each foe the cleave bites (the bramble's sustain identity).
        if matches!(art, Art::Sweep) {
            player.0.heal(SWEEP_LIFESTEAL as f64);
            sweep_heal += SWEEP_LIFESTEAL;
        }
        let dead = hp.hp <= 0.0;
        let head = Vec3::new(p.x, p.y + 2.2, p.z);
        let mid = Vec3::new(p.x, p.y + 0.9, p.z);
        spawn_burst(&mut commands, &fx, mid, dead);
        if dead {
            floats.0.push(crate::combat_fx::FloatReq { world: head, text: "†".into(), color: crate::combat_fx::col_kill(), scale: 1.3 });
            killed_any = true;
            let (gold, xp) = if let Some(o) = ork {
                (crate::orks::bounty_gold(o.variant, bounty_mult), crate::orks::bounty_xp(o.variant))
            } else if let Some(an) = animal {
                let prof = crate::verbs::animal_profile(an.species);
                ((prof.gold as f64 * bounty_mult).round() as i64, prof.xp)
            } else {
                (0, 0) // boss: its reward is granted by the boss death-watcher
            };
            if gold > 0 || xp > 0 {
                rewards.0.push(crate::orbs::RewardBurst { at: p, gold, xp });
            }
            if lifesteal > 0.0 {
                player.0.heal(lifesteal);
            }
            crate::dying::begin_dying(&mut commands, e, now);
        } else {
            floats.0.push(crate::combat_fx::FloatReq { world: head, text: format!("{}", dmg as i32), color: crate::combat_fx::col_ork_hit(), scale: 1.0 });
            commands.entity(e).try_insert(crate::combat_fx::HurtFlash::new(now));
            // Slam shoves survivors outward; the boons chill/poison them.
            if matches!(art, Art::Slam) {
                if let Some(_o) = ork {
                    commands.entity(e).try_insert(KnockImpulse { dir: knock_dir, mag: SLAM_KNOCK });
                }
            }
            if frostbite {
                commands.entity(e).try_insert(crate::boss::Slowed::new(now, 0.45, 2.0));
            }
            if venom {
                commands.entity(e).try_insert(crate::boss::Poisoned { until: now + 4.0, dps: (base * 0.4).max(4.0) });
            }
            let _ = knock_dir;
        }
    }
    if killed_any {
        cues.write(AudioCue::Impact { kill: true });
    }
    // Show the sweep's drained HP as a single green tick over the hero.
    if sweep_heal > 0.0 {
        floats.0.push(crate::combat_fx::FloatReq {
            world: Vec3::new(hero.pos.x, hero.y + 2.4, hero.pos.y),
            text: format!("+{}", sweep_heal as i32),
            color: Color::srgb(0.5, 1.0, 0.6),
            scale: 1.1,
        });
    }
}

/// A queued outward shove from Ground Slam (consumed into the ork's own `kb` by [`apply_knock`]).
#[derive(Component)]
pub struct KnockImpulse {
    dir: Vec2,
    mag: f32,
}

/// Fold queued slam knockbacks into each ork's decaying `kb` channel (orks slide it against
/// terrain in their brain). Runs ungated — a knock applied just before a panel still lands.
pub fn apply_knock(mut commands: Commands, mut q: Query<(Entity, &mut Ork, &KnockImpulse)>) {
    for (e, mut o, k) in &mut q {
        o.kb = k.dir * k.mag;
        commands.entity(e).try_remove::<KnockImpulse>();
    }
}

/// Distance from point `p` to segment `a→b`.
fn dist_point_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-6 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

// ── Ability HUD (bottom-right) — one icon cell per unlocked warden art (big game-icon + a small
//    corner key badge, like an inventory slot), lit when there's stamina to fire it, dimmed
//    ("blanked") when it can't be afforded. ──────────────────────────────────────────────────────

/// Which warden art a HUD chip represents.
#[derive(Clone, Copy, PartialEq)]
enum ArtKind {
    Slam,
    Dash,
    Sweep,
}

#[derive(Component)]
struct AbilityBar;
#[derive(Component)]
pub(crate) struct AbilityChip {
    kind: ArtKind,
}
/// The tintable icon inside a chip (recoloured per readiness).
#[derive(Component)]
pub(crate) struct AbilityIcon {
    kind: ArtKind,
}

/// The fixed stamina each art costs (drives both the spend and the "can I afford it" HUD test).
fn art_cost(kind: ArtKind) -> f32 {
    match kind {
        ArtKind::Slam => SLAM_STAMINA_COST,
        ArtKind::Dash => DASH_STAMINA_COST,
        ArtKind::Sweep => SWEEP_STAMINA_COST,
    }
}

/// `(unlocked, can afford)` for an art, from the live player + stamina state.
fn art_state(kind: ArtKind, player: &PlayerRes, hh: &HeroHealth) -> (bool, bool) {
    let unlocked = match kind {
        ArtKind::Slam => player.0.has_ground_slam,
        ArtKind::Dash => player.0.has_sand_dash,
        ArtKind::Sweep => player.0.has_bramble_sweep,
    };
    (unlocked, hh.stamina >= art_cost(kind))
}

/// Spawn the three ability cells (hidden until their boon is earned).
pub(crate) fn spawn_arts_hud(mut commands: Commands, fonts: Res<UiFonts>, atlas: Res<crate::ui::icons::IconAtlas>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                bottom: Val::Px(16.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(8.0),
                ..default()
            },
            GlobalZIndex(30),
            AbilityBar,
        ))
        .with_children(|bar| {
            for (kind, key, sym) in
                [(ArtKind::Slam, "Z", "art:slam"), (ArtKind::Dash, "X", "art:dash"), (ArtKind::Sweep, "C", "art:sweep")]
            {
                bar.spawn((
                    Node {
                        width: Val::Px(52.0),
                        height: Val::Px(52.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: widgets::border(1.0),
                        border_radius: radius(R_CELL),
                        display: Display::None, // shown once unlocked
                        ..default()
                    },
                    BackgroundColor(rgba(18, 14, 10, 0.55)),
                    BorderColor::all(rgba(120, 120, 120, 0.4)),
                    AbilityChip { kind },
                ))
                .with_children(|c| {
                    // Big tintable icon (recoloured per readiness by `sync_arts_hud`).
                    if let Some(h) = atlas.get(sym) {
                        let mut img = ImageNode::new(h);
                        img.color = rgb(255, 235, 200);
                        c.spawn((Node { width: Val::Px(32.0), height: Val::Px(32.0), ..default() }, img, AbilityIcon { kind }));
                    }
                    // Small key badge in the top-left corner (matches the satchel quick-bind badge).
                    c.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(-4.0),
                            left: Val::Px(-4.0),
                            min_width: Val::Px(15.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::axes(Val::Px(3.0), Val::Px(1.0)),
                            border_radius: radius(3.0),
                            ..default()
                        },
                        BackgroundColor(GOLD_DEEP),
                        children![label(&fonts.extrabold, key, 10.0, INK)],
                    ));
                });
            }
        });
}

/// Show/hide each cell + light or dim it (cell border + icon tint) per stamina affordability.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_arts_hud(
    app: Res<State<AppState>>,
    player: Res<PlayerRes>,
    hh_q: Query<&HeroHealth>,
    mut chips: Query<(&AbilityChip, &mut Node, &mut BackgroundColor, &mut BorderColor)>,
    mut icons: Query<(&AbilityIcon, &mut ImageNode)>,
) {
    let playing = *app.get() == AppState::Playing;
    let Ok(hh) = hh_q.single() else { return };
    for (chip, mut node, mut bg, mut border) in &mut chips {
        let (unlocked, ready) = art_state(chip.kind, &player, hh);
        node.display = if unlocked && playing { Display::Flex } else { Display::None };
        if ready {
            *bg = BackgroundColor(rgba(40, 30, 14, 0.85));
            *border = BorderColor::all(GOLD);
        } else {
            *bg = BackgroundColor(rgba(16, 12, 9, 0.5));
            *border = BorderColor::all(rgba(120, 120, 120, 0.35));
        }
    }
    for (icon, mut img) in &mut icons {
        let (_, ready) = art_state(icon.kind, &player, hh);
        img.color = if ready { rgb(255, 235, 200) } else { rgb(96, 96, 96) };
    }
}
