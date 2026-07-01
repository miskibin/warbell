//! **Soft-lock** — a light, AAA-style combat aim-assist (the Witcher "soft target"): while a
//! hostile is near and in front, the game quietly PICKS it as your target, marks it with a subtle
//! glowing ground ring, and MEASUREDLY eases the hero's facing onto it — never a hard snap, never a
//! camera lock. Your own movement always overrides (the turn only nudges while you stand), and the
//! swing then commits to the same ringed foe (see `combat::player_attack`, which reads `soft_pos`).
//!
//! It engages purely from proximity to a THREAT (ork / boss / warlord / rival soldier within
//! [`SOFT_RANGE`] in the front arc) — i.e. exactly when you're in a fight — and drops the instant no
//! threat is in front, so exploring never auto-turns you at wildlife or workers.

use bevy::prelude::*;

use super::{Hero, PlayMode};
use crate::boss::Boss;
use crate::dying::Dying;
use crate::orks::Ork;
use crate::rival::RivalSoldier;
use crate::warlord::Warlord;

/// Marker on the single reusable target-ring entity (moved under the soft target each frame).
#[derive(Component)]
pub struct TargetRing;

/// Consider hostiles within this world-XZ distance (a bit past melee, so a closing foe is picked
/// before you're on top of it).
const SOFT_RANGE: f32 = 7.0;
/// Front-arc gate: `dir·facing` must exceed this. −0.15 ≈ a 190° arc — a foe circling to your side
/// stays targeted, but one squarely behind you does not.
const FRONT_DOT: f32 = -0.15;
/// How much better a dead-ahead foe scores vs a side one (world units of "virtual" nearness). Makes
/// the pick favour whoever you're facing, so you steer targets by turning — not just by nearness.
const ALIGN_BONUS: f32 = 2.0;
/// The current target keeps this scoring edge, so it doesn't flicker to a marginally-closer foe.
const STICKY_BONUS: f32 = 1.6;
/// Measured auto-face speed (rad/s) — deliberately gentle (well under the 6–15 rad/s swing/move
/// turns) so it reads as a soft assist easing you onto the foe, not a snap.
const SOFT_TURN_RATE: f32 = 2.5;

/// Spawn the one reusable ground ring (hidden until there's a target). A thin flat annulus, unlit +
/// emissive so it glows as a soft "focus" mark regardless of time-of-day; alpha-blended so it reads
/// as a light overlay on the muck/grass, not a solid disc.
pub fn spawn_target_ring(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Annulus::new(0.62, 0.82).mesh().resolution(40).build());
    let mat = mats.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.82, 0.45, 0.55), // warm amber "focus" glow
        emissive: LinearRgba::rgb(0.9, 0.6, 0.2),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        // Annulus faces +Z; tip it flat (normal +Y) so it lies on the ground.
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        Visibility::Hidden,
        bevy::light::NotShadowCaster,
        TargetRing,
    ));
}

/// Pick the soft target, ease facing onto it, and drive the ring. Runs right after `player_move` so
/// live movement input (which already set `facing`) wins — the assist only nudges while you stand.
pub fn soft_lock(
    mode: Res<PlayMode>,
    time: Res<Time>,
    mut hero_q: Query<&mut Hero>,
    hostiles: Query<
        (Entity, &GlobalTransform),
        (
            Or<(With<Ork>, With<Boss>, With<Warlord>, With<RivalSoldier>)>,
            Without<Dying>,
        ),
    >,
    mut ring_q: Query<(&mut Transform, &mut Visibility), With<TargetRing>>,
) {
    let Ok(mut hero) = hero_q.single_mut() else { return };

    // Out of play (free-cam / paused-to-menu): no target, hide the ring.
    if *mode != PlayMode::Play {
        hero.soft_target = None;
        hero.soft_pos = None;
        for (_, mut vis) in &mut ring_q {
            *vis = Visibility::Hidden;
        }
        return;
    }

    // ── Pick the best hostile in the front arc within range ──
    let fwd = Vec2::new(hero.facing.sin(), hero.facing.cos());
    let mut best: Option<(f32, Entity, Vec2)> = None; // (score — lower is better, entity, pos)
    for (e, gt) in &hostiles {
        let t = gt.translation();
        let pos = Vec2::new(t.x, t.z);
        let to = pos - hero.pos;
        let d = to.length();
        if d > SOFT_RANGE || d < 1e-3 {
            continue;
        }
        let align = (to / d).dot(fwd); // 1 = dead ahead
        if align < FRONT_DOT {
            continue; // behind you
        }
        let mut score = d - align * ALIGN_BONUS;
        if hero.soft_target == Some(e) {
            score -= STICKY_BONUS; // keep the committed target unless clearly beaten
        }
        if best.map_or(true, |(bs, _, _)| score < bs) {
            best = Some((score, e, pos));
        }
    }
    match best {
        Some((_, e, pos)) => {
            hero.soft_target = Some(e);
            hero.soft_pos = Some(pos);
        }
        None => {
            hero.soft_target = None;
            hero.soft_pos = None;
        }
    }

    // ── Measured auto-face — only while standing (moving already steers facing) and not mid-swing
    // (the swing owns its own lean toward the same target). Gentle rate = an ease, not a snap.
    if let (Some(tp), false, false) = (hero.soft_pos, hero.moving, hero.attacking) {
        let to = tp - hero.pos;
        let want = to.x.atan2(to.y);
        let f = (time.delta_secs() * SOFT_TURN_RATE).min(1.0);
        hero.facing = super::movement::lerp_angle(hero.facing, want, f);
    }

    // ── Drive the ring ──
    for (mut tf, mut vis) in &mut ring_q {
        match hero.soft_pos {
            Some(tp) => {
                let y = crate::worldmap::ground_at_world(tp.x, tp.y).unwrap_or(hero.y) + 0.05;
                tf.translation = Vec3::new(tp.x, y, tp.y);
                *vis = Visibility::Visible;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}
