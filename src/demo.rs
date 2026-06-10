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
                app.add_systems(Update, explore_drive.run_if(in_state(crate::game_state::Modal::None)));
            }
            Some("defend") => {
                app.add_systems(PostStartup, defend_setup);
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
const EXPLORE_PATH: [Vec2; 6] = [
    Vec2::new(-2.0, 12.0),
    Vec2::new(-12.0, 20.0),
    Vec2::new(-24.0, 28.0),
    Vec2::new(-36.0, 34.0),
    Vec2::new(-46.0, 38.0),
    Vec2::new(-52.0, 39.0),
];
const EXPLORE_SPEED: f32 = 4.5; // world units / sec
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
    mut hero_q: Query<(&mut Hero, &mut Transform), Without<Camera3d>>,
    mut cam_q: Query<&mut Transform, (With<Camera3d>, Without<Hero>)>,
    mut dist: Local<f32>,
) {
    let dt = time.delta_secs();
    let (Ok((mut hero, mut htf)), Ok(mut ctf)) = (hero_q.single_mut(), cam_q.single_mut()) else {
        return;
    };
    *dist += EXPLORE_SPEED * dt;
    let (pos, dir, arrived) = sample_path(&EXPLORE_PATH, *dist);

    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(hero.y);
    hero.pos = pos;
    hero.y = y;
    hero.facing = dir.x.atan2(dir.y);
    hero.moving = !arrived;
    hero.moving_amt = if arrived { 0.0 } else { 1.0 };
    if !arrived {
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
const TALK_LINES: [&str; 5] = [
    "I told the hens about the orks. They were not impressed.",
    "Me cousin says he killed an ork once. Me cousin says a lotta things.",
    "Another glorious day of standing exactly here. Living the dream.",
    "If one more chicken gets into the chapel, I'm converting.",
    "Finest wares this side of the swamp. Only wares this side of the swamp, but still.",
];
const TALK_EVERY: f32 = 3.6;

fn talk_drive(time: Res<Time>, mut subs: ResMut<crate::subtitles::Subtitles>) {
    let now = time.elapsed_secs();
    let i = ((now / TALK_EVERY) as usize) % TALK_LINES.len();
    subs.force(now, Some("Townsfolk"), TALK_LINES[i], TALK_EVERY + 0.5);
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
    mut site: Local<Option<(Vec2, Vec2)>>,
    mut cleared: Local<bool>,
    mut tick: Local<u32>,
) {
    *tick += 1;
    let now = time.elapsed_secs();
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

    // After a brief beat (frame-counted — clip elapsed-time is wall-clocked, not frame-locked),
    // fell the warband → the real camp_rescue frees the captive on the next clear.
    if !*cleared && *tick > 36 {
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
    for i in 0..8u32 {
        crate::villagers::spawn_courtyard_guard(&mut commands, &mut meshes, &mut materials, 1009 + i * 97);
    }
}
