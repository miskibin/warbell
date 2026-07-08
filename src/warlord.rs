//! **The Warlord of Gnashfang Hold** — the game's final boss and win condition. He stands inert as
//! a decorative pacing [`Denizen`](crate::ork_fortress) until the hero **breaks the gate**
//! ([`ork_fortress`](crate::ork_fortress) handles the breach); then his decorative husk is swapped
//! for this real boss, who fights to the death **hard-leashed to his hall** (he barely chases — kite
//! him out of the courtyard and he stalks back). Slaying him sets `Player::conquered_warlord` and
//! flips the run to [`GamePhase::Victory`] — the ONLY way the game is won (nights themselves loop
//! forever now; see `siege.rs`).
//!
//! He is NOT a biome [`Boss`](crate::boss::Boss) — that type is keyed to the five biome wardens with
//! their dawn-leveling + boon rewards. The Warlord is a standalone entity built from the oversized
//! berserker ork model ([`Armory::spawn_prop`]) plus a [`Warlord`] marker + [`Health`]. He's made
//! hittable by adding `With<Warlord>` to the hero cone-scan in `player/combat.rs` + `player/arts.rs`,
//! so the hero's swings, arts, cleave and the Frostbite/Venom boons all land on him for free; the
//! kill itself (the `Dying` fade) is the shared combat path, and [`warlord_death`] watches for it.

use bevy::prelude::*;

use crate::boss::Slowed;
use crate::critters::PartKind;
use crate::dying::Dying;
use crate::game_state::AppState;
use crate::orks::{Armory, Faction, OrkPart, OrkVariant};
use crate::player::{CombatFx, Health, HeroState, PendingCrit, PendingHeroDamage, PlayerRes};
use crate::siege::{GamePhase, Siege};
use crate::ui::anim::{anim, AnimKind};
use crate::ui::fonts::{label, UiFonts};
use crate::ui::theme::*;
use crate::ui::widgets;
use crate::{steer, worldmap};
use crate::game_state::SimAppExt;

// ── Tuning (all forest-side) ─────────────────────────────────────────────────────────────
/// Base HP at hero level 1, grown ×`HP_PER_LEVEL` per hero level so a fully-geared late hero still
/// gets a real fight. Deliberately the toughest sack of HP in the game — this is the finale.
const BASE_HP: f32 = 3570.0; // 15% off (was 4200)
const HP_PER_LEVEL: f32 = 0.12;
/// Melee damage to the hero (no per-night growth — the warlord is a one-time fight).
const MELEE_DMG: f32 = 47.6; // 15% off (was 56); MELEE_NPC_DMG derives from this
/// Boss cleave vs a town defender — the armour-blunted share (same 0.6 mult ork melee uses on
/// guards) so a mustered war party bleeds against him but isn't deleted in two swings.
const MELEE_NPC_DMG: f32 = MELEE_DMG * 0.6;
const BODY_R: f32 = 0.95;
/// Faster than a warden (2.4) but **hard-leashed** ([`LEASH`]) so he can't be kited across the mire —
/// the user's "the boss doesn't chase too much": step out of his courtyard and he breaks off home.
const SPEED: f32 = 2.7;
const TURN: f32 = 2.4;
const MELEE_RANGE: f32 = 3.4;
const MELEE_CD: f32 = 1.2;
/// He fights only this far from his hall; past it he stalks back and waits.
const LEASH: f32 = 16.0;
/// Telegraphed killing blow — rears overhead for [`CRIT_TELEGRAPH`]s, then drops a lethal blow
/// (block with RMB or dodge out of [`CRIT_RANGE`] to negate it — the same contract as the wardens).
const CRIT_CD: f32 = 8.5;
const CRIT_TELEGRAPH: f32 = 1.2;
const CRIT_RANGE: f32 = 4.2; // just past MELEE_RANGE (3.4) — dodge clear of melee to escape it (was 5.5: a kill radius far past melee, so "far away" still ate the blow)
const CRIT_LETHAL: f32 = 100_000.0;
/// Oversized berserker — reads as a boss from across the courtyard.
const WARLORD_SCALE: f32 = 1.8;

fn max_hp(hero_level: i64) -> f32 {
    BASE_HP * (1.0 + HP_PER_LEVEL * (hero_level.max(1) - 1) as f32)
}

// ── Component ──────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub struct Warlord {
    pos: Vec2,
    facing: f32,
    home: Vec2,
    moving: bool,
    phase: f32,
    atk_cd: f32,
    atk_anim: f32,
    crit_cd: f32,
    /// `elapsed_secs` the winding-up critical lands (`0.0` = not winding up).
    crit_at: f32,
}

/// Marks a fallen warlord whose Victory has already fired, so [`warlord_death`] only wins once.
#[derive(Component)]
struct WonAlready;

// ── Plugin ───────────────────────────────────────────────────────────────────────────────

pub struct WarlordPlugin;

impl Plugin for WarlordPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, warlord_limbs) // limb sway runs even while frozen
            .add_systems(Update, sync_warlord_bar.run_if(in_state(AppState::Playing)))
            .add_systems(OnExit(AppState::Playing), despawn_warlord_bar)
            .add_sim_systems((warlord_brain, warlord_death));
    }
}

// ── Spawn (called by `ork_fortress` on the breach) ─────────────────────────────────────────

/// Build the real Warlord boss at `at` (the decorative warlord's pacing spot), facing the hero.
/// Reuses the oversized-berserker ork model from the kept fortress [`Armory`]; `hero_level` scales
/// his HP. Returns the root so the caller can tag it `BiomeEntity` for the world-rebuild lifecycle.
pub fn spawn(commands: &mut Commands, armory: &Armory, at: Vec2, facing: f32, hero_level: i64) -> Entity {
    let y = worldmap::ground_at_world(at.x, at.y).unwrap_or(0.0);
    let root = armory.spawn_prop(commands, OrkVariant::Berserker, Faction::Red, Vec3::new(at.x, y, at.y), facing, WARLORD_SCALE);
    let hp = max_hp(hero_level);
    commands.entity(root).insert((
        Warlord {
            pos: at,
            facing,
            home: at,
            moving: false,
            phase: 1.3,
            atk_cd: MELEE_CD,
            atk_anim: 0.0,
            crit_cd: CRIT_CD * 0.6,
            crit_at: 0.0,
        },
        Health { hp, max: hp },
    ));
    root
}

// ── Brain: chase + melee + telegraphed crit, hard-leashed to the hall ──────────────────────

#[allow(clippy::too_many_arguments)]
fn warlord_brain(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut pending: ResMut<PendingHeroDamage>,
    mut crit: ResMut<PendingCrit>,
    fx: Option<Res<CombatFx>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut commands: Commands,
    mut npc_dmg: ResMut<crate::villagers::NpcDamage>,
    guards: Query<(Entity, &Transform), (With<crate::villagers::NpcHp>, Without<Dying>, Without<Warlord>)>,
    mut q: Query<(Entity, &mut Warlord, &mut Transform, Option<&Slowed>), Without<Dying>>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let tw = time.elapsed_secs_wrapped();
    for (self_e, mut w, mut tf, slowed) in &mut q {
        w.atk_cd -= dt;
        w.crit_cd -= dt;
        let slow = slowed.map(|s| s.factor).unwrap_or(1.0);
        let hero_d = if hero.alive { w.pos.distance(hero.pos) } else { f32::INFINITY };
        // The hero is a target only inside the leash of his hall.
        let hero_active = hero.alive && w.home.distance(hero.pos) <= LEASH;
        // The telegraphed killing blow is hero-only — drop any windup the moment he leaves the fray.
        if !hero_active {
            w.crit_at = 0.0;
        }
        // The war party that marched on him: nearest town defender inside the leash. He turns on
        // the militia too (and his swing cleaves every one in reach, below), so a muster can't just
        // facetank him while the hero plinks — he fights back.
        let mut npc: Option<(Entity, Vec2, f32)> = None;
        for (ge, gtf) in &guards {
            let gp = Vec2::new(gtf.translation.x, gtf.translation.z);
            if w.home.distance(gp) > LEASH {
                continue;
            }
            let d = w.pos.distance(gp);
            if npc.is_none_or(|(_, _, bd)| d < bd) {
                npc = Some((ge, gp, d));
            }
        }
        let engaged = hero_active || npc.is_some();
        // Movement/melee target: the hero when he's in the fray, else the nearest guard.
        let (tgt_pos, tgt_d) = if hero_active {
            (hero.pos, hero_d)
        } else if let Some((_, gp, gd)) = npc {
            (gp, gd)
        } else {
            (w.home, f32::INFINITY)
        };

        if hero_active && w.crit_at > 0.0 {
            // Winding up the killing blow: plant, track the hero slowly, drop it on impact.
            w.moving = false;
            let to = hero.pos - w.pos;
            if to.length_squared() > 1e-4 {
                let want = to.x.atan2(to.y);
                w.facing += steer::wrap_pi(want - w.facing).clamp(-TURN * dt, TURN * dt);
            }
            if now >= w.crit_at {
                w.crit_at = 0.0;
                let gy = steer::footing(w.pos.x, w.pos.y).unwrap_or(tf.translation.y);
                if hero_d < CRIT_RANGE {
                    crit.0 = true;
                    pending.0 += CRIT_LETHAL; // negated if the hero blocks / dodges
                    pending.1 = (hero.pos - w.pos).normalize_or_zero(); // directional hit-shake
                    cues.write(crate::audio::AudioCue::Slam);
                }
                if let Some(fx) = &fx {
                    crate::player::spawn_shockwave(&mut commands, fx, &mut materials, Vec3::new(w.pos.x, gy + 0.05, w.pos.y), now);
                }
            }
        } else if engaged {
            if tgt_d > MELEE_RANGE {
                let cur_y = steer::footing(w.pos.x, w.pos.y).unwrap_or(tf.translation.y);
                if let Some(s) = steer::advance(w.pos, w.facing, tgt_pos, SPEED * slow * dt, BODY_R, cur_y, TURN * dt) {
                    w.facing = s.facing;
                    w.pos = s.pos;
                    w.moving = s.moving;
                } else {
                    w.moving = false;
                }
            } else {
                w.moving = false;
                let to = tgt_pos - w.pos;
                if to.length_squared() > 1e-4 {
                    let want = to.x.atan2(to.y);
                    w.facing += steer::wrap_pi(want - w.facing).clamp(-TURN * 2.0 * dt, TURN * 2.0 * dt);
                }
            }
            // A swing lands when off cooldown and SOMETHING is within reach — the hero AND every
            // town defender in melee both eat it (a boss cleave). His own cooldown gates the rate,
            // so chasing the hero doesn't let him swat the war party for free.
            if w.atk_cd <= 0.0 {
                let hero_hit = hero_active && hero_d <= MELEE_RANGE;
                let mut npc_hits = 0;
                for (ge, gtf) in &guards {
                    let gp = Vec2::new(gtf.translation.x, gtf.translation.z);
                    if w.pos.distance(gp) <= MELEE_RANGE {
                        npc_dmg.0.push(crate::villagers::NpcHit {
                            victim: ge,
                            amount: MELEE_NPC_DMG,
                            attacker: Some(self_e),
                        });
                        npc_hits += 1;
                    }
                }
                if hero_hit || npc_hits > 0 {
                    w.atk_cd = MELEE_CD;
                    w.atk_anim = now;
                    if hero_hit {
                        pending.0 += MELEE_DMG;
                        pending.1 = (hero.pos - w.pos).normalize_or_zero(); // directional hit-shake
                    }
                }
            }
            // Begin a telegraphed critical (hero-only) when off cooldown and the hero is in range.
            if w.crit_cd <= 0.0 && hero_active && hero_d < CRIT_RANGE {
                w.crit_cd = CRIT_CD;
                w.crit_at = now + CRIT_TELEGRAPH;
                let gy = steer::footing(w.pos.x, w.pos.y).unwrap_or(tf.translation.y);
                let head = Vec3::new(w.pos.x, gy + 2.4, w.pos.y);
                cues.write(crate::audio::AudioCue::BossRoar(head));
                cues.write(crate::audio::AudioCue::BossWindup(head));
            }
        } else {
            // Out of leash (or the hero fell): abandon any windup, stalk back to the hall.
            w.crit_at = 0.0;
            if w.pos.distance(w.home) > 1.2 {
                let cur_y = steer::footing(w.pos.x, w.pos.y).unwrap_or(tf.translation.y);
                match steer::advance(w.pos, w.facing, w.home, SPEED * 0.7 * dt, BODY_R, cur_y, TURN * dt) {
                    Some(s) => {
                        w.facing = s.facing;
                        w.pos = s.pos;
                        w.moving = s.moving;
                    }
                    None => w.moving = false,
                }
            } else {
                w.moving = false;
            }
        }

        let gy = steer::footing(w.pos.x, w.pos.y).unwrap_or(tf.translation.y);
        let bob = if w.moving { (tw * 5.0 + w.phase).sin().abs() * 0.08 } else { (tw * 1.2).sin() * 0.03 };
        tf.translation = Vec3::new(w.pos.x, gy + bob, w.pos.y);
        tf.rotation = Quat::from_rotation_y(w.facing);
    }
}

/// Animate the warlord's ork limbs (stride / counter-swinging arms / crit-rear), reusing the
/// shared [`OrkPart`] limb markers exactly like `ork_fortress::denizen_limbs` + `boss::boss_limbs`.
fn warlord_limbs(
    time: Res<Time>,
    warlords: Query<(&Warlord, &Children)>,
    mut parts: Query<(&OrkPart, &mut Transform)>,
) {
    let tw = time.elapsed_secs_wrapped();
    let now = time.elapsed_secs();
    for (w, children) in &warlords {
        let t = tw + w.phase;
        let strike = w.atk_anim > 0.0 && (now - w.atk_anim) < 0.45;
        let crit_wind = w.crit_at > 0.0 && now < w.crit_at;
        let crit_p = if crit_wind { (1.0 - (w.crit_at - now) / CRIT_TELEGRAPH).clamp(0.0, 1.0) } else { 0.0 };
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                PartKind::Leg(sign) => {
                    let s = if w.moving { (t * 4.8).sin() * 0.5 } else { (t * 0.7).sin() * 0.03 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Arm(sign) => {
                    if crit_wind {
                        Quat::from_rotation_x(-1.0 - 2.0 * crit_p) // haul overhead across the windup
                    } else if sign > 0.0 && strike {
                        let p = (now - w.atk_anim) / 0.45;
                        Quat::from_rotation_x(-1.4 + 3.0 * (p * std::f32::consts::PI).sin())
                    } else {
                        let s = if w.moving { -(t * 4.8).sin() * 0.4 } else { (t * 0.7).sin() * 0.05 };
                        Quat::from_rotation_x(sign * s)
                    }
                }
                PartKind::Head => Quat::from_rotation_y((t * 0.5).sin() * 0.15),
                PartKind::Tail => Quat::IDENTITY,
            };
        }
    }
}

/// The Warlord fell (the shared combat kill inserted `Dying`): record the win on the `Player`, flip
/// the run to `Victory` (which `game_state` turns into the gold VICTORY screen), and bellow a death
/// roar. `WonAlready` makes this fire once even though the corpse lingers through the `Dying` fade.
fn warlord_death(
    time: Res<Time>,
    mut commands: Commands,
    mut player: ResMut<PlayerRes>,
    mut siege: ResMut<Siege>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    q: Query<(Entity, &Transform), (With<Warlord>, With<Dying>, Without<WonAlready>)>,
) {
    for (e, tf) in &q {
        commands.entity(e).try_insert(WonAlready);
        player.0.conquered_warlord = true;
        siege.phase = GamePhase::Victory; // game_state watches this → AppState::GameOver (VICTORY)
        notice.push("The Warlord is slain — Gnashfang Hold is broken!".to_string(), time.elapsed_secs_f64());
        cues.write(crate::audio::AudioCue::BossRoar(tf.translation + Vec3::Y * 2.4));
    }
}

// ── Health bar (bottom-centre, while the Warlord lives) ────────────────────────────────────
#[derive(Component)]
struct WarlordBarRoot;
#[derive(Component)]
struct WarlordBarFill;

fn sync_warlord_bar(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    hero: Res<HeroState>,
    warlord: Query<&Health, (With<Warlord>, Without<Dying>)>,
    root: Query<Entity, With<WarlordBarRoot>>,
    mut fill_q: Query<&mut Node, With<WarlordBarFill>>,
) {
    let ratio = if hero.alive {
        warlord.iter().next().map(|h| (h.hp / h.max.max(1.0)).clamp(0.0, 1.0))
    } else {
        None
    };
    match (ratio, root.single()) {
        (Some(r), Ok(_)) => {
            if let Ok(mut n) = fill_q.single_mut() {
                n.width = Val::Percent(r * 100.0);
            }
        }
        (Some(r), Err(_)) => spawn_warlord_bar(&mut commands, &fonts, r),
        (None, Ok(e)) => commands.entity(e).try_despawn(),
        (None, Err(_)) => {}
    }
}

fn despawn_warlord_bar(mut commands: Commands, q: Query<Entity, With<WarlordBarRoot>>) {
    for e in &q {
        commands.entity(e).try_despawn();
    }
}

fn spawn_warlord_bar(commands: &mut Commands, fonts: &UiFonts, ratio: f32) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(86.0),
                left: Val::Percent(50.0),
                width: Val::Px(560.0),
                margin: UiRect::left(Val::Px(-280.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
            GlobalZIndex(40),
            WarlordBarRoot,
            anim(AnimKind::Rise, 0.0, 0.4),
        ))
        .with_children(|r| {
            r.spawn((
                label(&fonts.display, "THE WARLORD OF GNASHFANG HOLD", 18.0, rgb(255, 224, 170)),
                TextShadow { offset: Vec2::new(0.0, 2.0), color: rgba(0, 0, 0, 0.8) },
            ));
            r.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(14.0),
                    border: widgets::border(1.0),
                    border_radius: radius(6.0),
                    ..default()
                },
                BackgroundColor(rgba(20, 12, 10, 0.82)),
                BorderColor::all(rgba(224, 168, 74, 0.7)),
            ))
            .with_children(|track| {
                track.spawn((
                    Node {
                        width: Val::Percent(ratio * 100.0),
                        height: Val::Percent(100.0),
                        border_radius: radius(6.0),
                        ..default()
                    },
                    BackgroundColor(rgb(150, 196, 54)), // warp-green, the Hold's hue
                    WarlordBarFill,
                ));
            });
        });
}
