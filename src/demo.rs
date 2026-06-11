//! Scripted demo "director" — autonomous gameplay scenarios for marketing clips.
//!
//! The Bevy window can't take live input headlessly, so instead of filming a human playing we
//! script the actors in-engine and record with the `FOREST_CLIP` harness (`capture.rs`). Pick a
//! scenario with `FOREST_DEMO`:
//!   - `explore` — walk the hero along a scenic path behind a chase-cam; the world stays alive
//!     (villagers, wildlife, wind, day sky). Pair with `FOREST_CLIP`.
//!   - `defend`  — reinforce the courtyard with guards for a lively castle defence. Pair with
//!     `FOREST_CLIP FOREST_WAVE=1 FOREST_DEFEND=1 FOREST_TOWN=1` (siege + auto-defences + the
//!     sustained horde from `siege_clip_refill`); frame with `FOREST_CAM`/`FOREST_CLIP_ORBIT`.
//!   - `build`   — handled in `town.rs` (`demo_build_timelapse`): raise the town one plot at a
//!     time for a construction timelapse; frame with `FOREST_CLIP_ORBIT`.
//!
//! Under `FOREST_CLIP` the hero boots in `PlayMode::FreeRoam`, where `player_move`/`player_camera`
//! early-return — so this owns the hero pose and the camera outright, while the ungated `hero_anim`
//! still swings the limbs from the `Hero` fields we write. Only active when `FOREST_DEMO` is set.

use crate::player::Hero;
use bevy::prelude::*;

pub struct DemoPlugin;

impl Plugin for DemoPlugin {
    fn build(&self, app: &mut App) {
        match std::env::var("FOREST_DEMO").ok().as_deref() {
            Some("explore") => {
                app.add_systems(Update, explore_drive.run_if(in_state(crate::game_state::Modal::None)))
                    .add_systems(PostUpdate, mute_captions);
            }
            Some("defend") => {
                app.add_systems(PostStartup, defend_setup)
                    .add_systems(
                        Update,
                        (defend_hero, defend_keep_guards).run_if(in_state(crate::game_state::Modal::None)),
                    )
                    .add_systems(PostUpdate, (mute_captions, defend_light));
            }
            Some("build") | Some("work") => {
                // Build/work logic lives in town.rs; here we just silence ambient barks.
                app.add_systems(PostUpdate, mute_captions);
            }
            Some("talk") => {
                // PostUpdate so our chosen caption is the last write each frame — the audio
                // director's ambient barks (Update) can't clobber it.
                app.add_systems(PostUpdate, talk_drive.run_if(in_state(crate::game_state::Modal::None)));
            }
            Some("rescue") => {
                app.add_systems(PostUpdate, rescue_drive.run_if(in_state(crate::game_state::Modal::None)));
            }
            _ => {}
        }
    }
}

// ── explore: scripted hero walk + chase-cam ──────────────────────────────────────────

/// Scenic walk route (world XZ). Heads out of the castle lawn south-west toward the forest, over
/// open grass. Keep waypoints on walkable land (this bypasses collision, so avoid walls/water).
const EXPLORE_PATH: [Vec2; 5] = [
    Vec2::new(-2.0, 12.0),
    Vec2::new(-12.0, 19.0),
    Vec2::new(-22.0, 25.0),
    Vec2::new(-30.0, 28.0),
    Vec2::new(-36.0, 30.0),
];
const EXPLORE_SPEED: f32 = 4.0; // world units / sec
const STEP_FREQ: f32 = 7.0; // matches movement.rs leg cadence

/// Position + unit tangent at arc-length `d` along the polyline; `arrived` once past the end.
fn sample_path(path: &[Vec2], d: f32) -> (Vec2, Vec2, bool) {
    let mut rem = d;
    for w in path.windows(2) {
        let seg = w[1] - w[0];
        let len = seg.length();
        if len < 1e-4 {
            continue;
        }
        if rem <= len {
            return (w[0] + seg * (rem / len), seg / len, false);
        }
        rem -= len;
    }
    let n = path.len();
    (path[n - 1], (path[n - 1] - path[n - 2]).normalize_or_zero(), true)
}

#[allow(clippy::type_complexity)]
fn explore_drive(
    time: Res<Time>,
    prog: Option<Res<crate::capture::ClipProgress>>,
    mut hero_q: Query<(&mut Hero, &mut Transform), Without<Camera3d>>,
    mut cam_q: Query<&mut Transform, (With<Camera3d>, Without<Hero>)>,
    mut dist: Local<f32>,
) {
    let dt = time.delta_secs();
    let (Ok((mut hero, mut htf)), Ok(mut ctf)) = (hero_q.single_mut(), cam_q.single_mut()) else {
        return;
    };
    // Hold at the start until recording begins (warm-up lets shaders/lighting settle first).
    let rec = prog.as_ref().map_or(true, |p| p.recording);
    if rec {
        // Only step forward onto solid land — never walk out over a river (no terrain there).
        let next = *dist + EXPLORE_SPEED * dt;
        let (np, _, _) = sample_path(&EXPLORE_PATH, next);
        if crate::worldmap::ground_at_world(np.x, np.y).is_some() {
            *dist = next;
        }
    }
    let (pos, dir, arrived) = sample_path(&EXPLORE_PATH, *dist);

    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(hero.y);
    let walking = rec && !arrived;
    hero.pos = pos;
    hero.y = y;
    hero.facing = dir.x.atan2(dir.y);
    hero.moving = walking;
    hero.moving_amt = if walking { 1.0 } else { 0.0 };
    if walking {
        hero.walk_phase += dt * STEP_FREQ;
    }
    let bob = hero.walk_phase.sin().abs() * 0.05 * hero.moving_amt;
    htf.translation = Vec3::new(pos.x, y + bob, pos.y);
    htf.rotation = Quat::from_rotation_y(hero.facing);

    // Third-person chase: behind the walk direction, raised, looking at the hero's head.
    let eye = Vec3::new(pos.x, y + 1.0, pos.y);
    let back = Vec3::new(dir.x, 0.0, dir.y).normalize_or_zero();
    ctf.translation = eye - back * 5.8 + Vec3::Y * 2.5;
    ctf.look_at(eye, Vec3::Y);
}

// ── talk: cycle the funny townsfolk barks as on-screen captions ──────────────────────

/// Real villager bark lines (verbatim from `audio/lines.rs`). We re-assert the caption every
/// frame so the director's random ambient barks can't clobber our chosen quip mid-clip.
// The funniest, most sarcastic townsfolk barks — each has a recorded VO clip
// (screaming / statue / optimist), so the spoken audio matches the caption on screen.
const TALK_LINES: [&str; 3] = [
    "Bless you for the protection. The screaming at night is a lovely touch.",
    "They'll raise you a statue one day, m'lord. The pigeons are very excited.",
    "Mum always said look on the bright side. So: at least the orks are punctual.",
];
const TALK_EVERY: f32 = 6.2;

fn talk_drive(
    time: Res<Time>,
    prog: Option<Res<crate::capture::ClipProgress>>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
) {
    let now = time.elapsed_secs();
    // Cycle on the frame-locked clock (clip elapsed-time is wall-clocked, not 1:1 with playback).
    let f = prog.as_ref().map_or(0, |p| p.frame);
    let per = (TALK_EVERY * 30.0) as u32; // recorded frames per line at the 30fps playback rate
    let i = ((f / per.max(1)) as usize) % TALK_LINES.len();
    subs.force(now, Some("Townsfolk"), TALK_LINES[i], TALK_EVERY + 0.5);
}

/// Lock the caption to empty so the audio director's random ambient barks never show — for the
/// scenery clips (explore/build/defend) that shouldn't carry stray subtitles.
fn mute_captions(time: Res<Time>, mut subs: ResMut<crate::subtitles::Subtitles>) {
    subs.force(time.elapsed_secs(), None, "", 0.0);
}

// ── rescue: free a caged captive — the real camp_rescue line fires ───────────────────

const RESCUE_LINE: &str = "You came for me? Gods bless you. I'll take up a spear, I swear it.";

/// Teleport the hero to the nearest camp's cage, frame it, then fell the warband so the genuine
/// `villagers::camp_rescue` flow frees the captive and speaks. We also hold the rescue caption on
/// screen so the (funny) line is guaranteed to read in the clip.
#[allow(clippy::type_complexity)]
fn rescue_drive(
    time: Res<Time>,
    mut commands: Commands,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    mut hero_q: Query<(&mut Hero, &mut Transform), (Without<Camera3d>, Without<crate::orks::Ork>)>,
    mut cam_q: Query<&mut Transform, (With<Camera3d>, Without<Hero>, Without<crate::orks::Ork>)>,
    orks: Query<(Entity, &crate::orks::Ork), Without<crate::orks::WaveInvader>>,
    prog: Option<Res<crate::capture::ClipProgress>>,
    mut site: Local<Option<(Vec2, Vec2)>>,
    mut cleared: Local<bool>,
) {
    let now = time.elapsed_secs();
    let f = prog.as_ref().map_or(0, |p| p.frame); // frame-locked beat (0 during warm-up)
    if site.is_none() {
        // Nearest camp cage to the castle (origin).
        *site = crate::camps::cage_positions()
            .into_iter()
            .min_by(|a, b| a.0.length_squared().total_cmp(&b.0.length_squared()));
    }
    let Some((cage, centre)) = *site else { return };

    let to_origin = (-cage).normalize_or_zero();
    let hpos = cage + to_origin * 3.2; // stand a stride from the cage, on the approach side
    let hy = crate::worldmap::ground_at_world(hpos.x, hpos.y).unwrap_or(0.0);
    let face = cage - hpos;
    if let Ok((mut hero, mut htf)) = hero_q.single_mut() {
        hero.pos = hpos;
        hero.y = hy;
        hero.facing = face.x.atan2(face.y);
        hero.moving = false;
        hero.moving_amt = 0.0;
        htf.translation = Vec3::new(hpos.x, hy, hpos.y);
        htf.rotation = Quat::from_rotation_y(hero.facing);
    }
    if let Ok(mut ctf) = cam_q.single_mut() {
        let cam = Vec3::new(hpos.x + to_origin.x * 4.5, hy + 2.6, hpos.y + to_origin.y * 4.5);
        ctf.translation = cam;
        ctf.look_at(Vec3::new(cage.x, hy + 1.0, cage.y), Vec3::Y);
    }

    // A beat after recording starts (frame-counted — clip elapsed-time is wall-clocked), fell the
    // warband → the real camp_rescue frees the captive on the next clear.
    if !*cleared && f > 36 {
        *cleared = true;
        for (e, o) in &orks {
            if o.home().distance(centre) < 6.0 {
                crate::dying::begin_dying(&mut commands, e, now);
            }
        }
    }
    // Pin the rescue quip on screen the whole clip (ambient hurt/guard barks can't override it).
    subs.force(now, Some("Townsfolk"), RESCUE_LINE, 2.0);
}

// ── defend: courtyard guard reinforcement ────────────────────────────────────────────

/// Stand up a squad of courtyard guards so the castle visibly fights back (they auto-engage
/// invaders via `villagers::guard_combat`). The orks/auto-defences come from the siege hooks.
fn defend_setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for i in 0..26u32 {
        crate::villagers::spawn_courtyard_guard(&mut commands, &mut meshes, &mut materials, 1009 + i * 97);
    }
}

/// Keep the defending townsfolk on their feet for the whole clip — without this the warm-up melee
/// (36 orks vs the squad) wipes them before recording even starts, leaving "just orks" on screen.
fn defend_keep_guards(mut guards: Query<&mut crate::villagers::NpcHp, With<crate::villagers::Guard>>) {
    for mut hp in &mut guards {
        hp.hp = hp.max;
    }
}

/// Cinematic moonlight lift for the night siege: the wave phase forces deep night (advance_sky),
/// so the battle is unreadably dark. Flood a cool ambient fill (PostUpdate, after advance_sky) so
/// the guards, orks and knight all read while the sky stays night. Clip-only.
fn defend_light(mut ambient: ResMut<GlobalAmbientLight>) {
    ambient.brightness = 480.0;
    ambient.color = Color::srgb(0.72, 0.80, 1.0);
}

const HERO_SWING: f32 = 0.45; // matches combat::ATTACK_DURATION

/// Put the knight in the thick of the night battle: he holds the gate, faces the nearest invader,
/// swings on a cadence (the `hero_anim` swing reads these fields, mode-independent) and fells orks
/// in front at the hit beat. Kept topped-up so he doesn't fall mid-clip. FreeRoam-only (capture).
#[allow(clippy::type_complexity)]
fn defend_hero(
    time: Res<Time>,
    mut commands: Commands,
    mut player: ResMut<crate::player::PlayerRes>,
    mut hero_q: Query<(&mut Hero, &mut Transform), (Without<Camera3d>, Without<crate::orks::WaveInvader>)>,
    orks: Query<(Entity, &Transform), (With<crate::orks::WaveInvader>, Without<crate::dying::Dying>, Without<Hero>)>,
    mut gap: Local<f32>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    player.0.hp = player.0.max_hp; // invulnerable for the show
    let Ok((mut hero, mut htf)) = hero_q.single_mut() else { return };
    let stand = Vec2::new(0.0, 7.0); // in the courtyard by the keep, where breached orks + guards meet

    // Face the nearest living invader.
    let mut best: Option<f32> = None;
    for (_, tf) in &orks {
        let p = Vec2::new(tf.translation.x, tf.translation.z);
        let d = p.distance(stand);
        if best.map_or(true, |bd| d < bd) {
            best = Some(d);
            let dir = p - stand;
            hero.facing = dir.x.atan2(dir.y);
        }
    }

    // Swing cadence: a swing, then a short gap; fell front-arc invaders at the hit beat.
    if hero.attacking {
        hero.attack_t += dt;
        let p = hero.attack_t / HERO_SWING;
        if p >= 1.0 {
            hero.attacking = false;
            *gap = 0.0;
        } else if !hero.hit_dealt && p >= 0.3 {
            hero.hit_dealt = true;
            let fwd = Vec2::new(hero.facing.sin(), hero.facing.cos());
            for (e, tf) in &orks {
                let to = Vec2::new(tf.translation.x, tf.translation.z) - stand;
                if to.length() < 3.4 && to.normalize_or_zero().dot(fwd) > 0.25 {
                    crate::dying::begin_dying(&mut commands, e, now);
                }
            }
        }
    } else {
        *gap += dt;
        if *gap >= 0.22 {
            hero.attacking = true;
            hero.attack_t = 0.0;
            hero.hit_dealt = false;
        }
    }

    let hy = crate::worldmap::ground_at_world(stand.x, stand.y).unwrap_or(hero.y);
    hero.pos = stand;
    hero.y = hy;
    hero.moving = false;
    hero.moving_amt = 0.0;
    htf.translation = Vec3::new(stand.x, hy, stand.y);
    htf.rotation = Quat::from_rotation_y(hero.facing);
}
