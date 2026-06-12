//! Hero limb animation — walk/idle leg + arm swing, idle sway, head scan, the attack-swing
//! arm override, and the shield raise while blocking. Ported from the animation drivers in
//! `Character.tsx`. The right arm carries the baked sword, so swinging it swings the blade.

use bevy::prelude::*;

use super::combat::ATTACK_DURATION;
use super::model::{shield_block_rot, shield_rest_rot, SHIELD_BLOCK_POS, SHIELD_REST_POS};
use super::{Hero, HeroHealth, HeroLimb, HeroPart};

/// Forward lean of the resting sword arm (negative X) so the blade is presented in front.
const ARM_FORWARD: f32 = 0.5;

pub fn hero_anim(
    time: Res<Time>,
    player: Res<super::PlayerRes>,
    dir: Res<crate::cinematic::DirectorState>,
    hero_q: Query<(&Hero, &HeroHealth, &Children)>,
    mut parts: Query<(&HeroPart, &mut Transform)>,
) {
    let Ok((hero, hh, children)) = hero_q.single() else { return };
    // Slain: let the limbs go slack (no walk/idle swing) while the body keels over.
    if !player.0.is_alive() {
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.limb {
                HeroLimb::ArmR => Quat::from_rotation_x(-ARM_FORWARD * 0.5),
                _ => Quat::IDENTITY,
            };
        }
        return;
    }
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let m = hero.moving_amt;
    let wp = hero.walk_phase;
    let blocking = hh.blocking;

    let leg_swing = wp.sin() * 0.7 * m;
    let idle_sway = (t * 1.1).sin() * 0.08 * (1.0 - m);
    let arm_swing = (wp + std::f32::consts::PI).sin() * 0.55 * m;
    let head_scan = (t * 0.4).sin() * 0.18 * (1.0 - m);

    // Active swing phase (0..1), if mid-attack.
    let attack_p = hero.attacking.then(|| (hero.attack_t / ATTACK_DURATION).clamp(0.0, 1.0));

    // Staged trailer gesture (F1 Director): when set, it overrides the sword/shield arms with a
    // posed/looping animation the normal game never plays. `(armR, armL)` — `None` per arm = keep
    // the default animation for that arm.
    let gesture_arms = dir.gesture.map(|g| gesture_pose(g, t - dir.gesture_start));

    // Frame-rate-independent damp toward the shield's target pose (~0.25s settle).
    let damp = 1.0 - 0.004_f32.powf(dt);

    for &child in children {
        let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
        match part.limb {
            HeroLimb::LegR => tf.rotation = Quat::from_rotation_x(leg_swing),
            HeroLimb::LegL => tf.rotation = Quat::from_rotation_x(-leg_swing),
            HeroLimb::ArmR => {
                // Gesture override wins; else mid-swing → the slash (begins/ends at the forward
                // rest pose, so no pop); else the arm rests forward + walk swing + idle sway.
                tf.rotation = match gesture_arms {
                    Some((Some(q), _)) => q,
                    _ => match attack_p {
                        Some(p) => attack_arm_quat(p),
                        None => Quat::from_rotation_x(arm_swing + idle_sway - ARM_FORWARD),
                    },
                };
            }
            HeroLimb::ArmL => {
                tf.rotation = match gesture_arms {
                    Some((_, Some(q))) => q,
                    _ if blocking => {
                        // Raise the shield arm across the front to brace behind the plate.
                        Quat::from_euler(EulerRot::XYZ, -1.25, 0.0, 0.4)
                    }
                    _ => Quat::from_rotation_x(-arm_swing - idle_sway),
                };
            }
            HeroLimb::Head => tf.rotation = Quat::from_rotation_y(head_scan),
            HeroLimb::Shield => {
                let (tp, tr) = if blocking {
                    (SHIELD_BLOCK_POS, shield_block_rot())
                } else {
                    (SHIELD_REST_POS, shield_rest_rot())
                };
                tf.translation = tf.translation.lerp(tp, damp);
                tf.rotation = tf.rotation.slerp(tr, damp);
            }
        }
    }
}

/// Staged-gesture arm poses (Director). Returns `(right_arm, left_arm)` rotations; `None` per arm
/// means "leave that arm on its normal animation". `ph` is seconds since the gesture began (loop
/// phase). The sword arm is `ArmR`, the shield arm `ArmL`; on this rig a more-negative X pitches
/// the arm up/forward, +Z splays it outward. Rough by design — eyeball + nudge against a capture.
fn gesture_pose(g: crate::cinematic::HeroGesture, ph: f32) -> (Option<Quat>, Option<Quat>) {
    use crate::cinematic::HeroGesture::*;
    let e = |x: f32, y: f32, z: f32| Quat::from_euler(EulerRot::XYZ, x, y, z);
    // Ease-in raise (0→1 over ~0.45s) so a gesture lifts smoothly into place rather than snapping.
    let raise = (ph / 0.45).clamp(0.0, 1.0);
    let ease = raise * raise * (3.0 - 2.0 * raise); // smoothstep
    match g {
        // Arm raised overhead, hand flicking side to side (weapon auto-hidden for this one).
        Wave => (Some(e(-2.55 * ease, 0.0, 0.20 + (ph * 5.0).sin() * 0.45 * ease)), None),
        // Sword arm snapped up in a blade salute (weapon kept — it suits the pose).
        Salute => (Some(e(-2.6 * ease, 0.0, 0.85 * ease)), None),
        // Arm thrust out horizontally, commanding (weapon kept — points the blade).
        Point => (Some(e(-1.6 * ease, 0.0, 0.05)), None),
        // Both forearms folded across the chest — the "supervise" idle.
        ArmsCrossed => (
            Some(e(-1.15 * ease, 0.0, 0.85 * ease)),
            Some(e(-1.15 * ease, 0.0, -0.85 * ease)),
        ),
        // Both arms thrown overhead, a small triumphant pump (weapon auto-hidden).
        Cheer => {
            let pump = (ph * 4.0).sin() * 0.15;
            (
                Some(e((-2.75 + pump) * ease, 0.0, -0.35 * ease)),
                Some(e((-2.75 + pump) * ease, 0.0, 0.35 * ease)),
            )
        }
        // A repeating chop/hammer swing (reuses the attack arc on a loop) — "at work".
        // `max(0)`: during the director PRE_ROLL the phase is negative (a negative `fract()`
        // would walk the swing backwards) — hold the rest pose instead.
        Work => (Some(attack_arm_quat((ph.max(0.0) * 1.3).fract())), None),
    }
}

/// A horizontal sword slash with snap. Ease-IN windup + raise (0–0.25) for anticipation, an
/// ease-OUT sweep across the front (0.25–0.55) so the blade *cracks* through at the hit phase
/// then decelerates, recover (0.55–1). Endpoints equal the forward rest pose `(x=-ARM_FORWARD,
/// y=0)` so the swing blends in and out with no pop.
fn attack_arm_quat(p: f32) -> Quat {
    const LIFT: f32 = 0.7; // extra raise during the swing (bigger arc than the old 0.55)
    const SWEEP: f32 = 1.45; // half the horizontal arc (was 1.25)
    let (x, y) = if p < 0.25 {
        let u = p / 0.25;
        let e = u * u; // accelerate into the wound-up top
        (-ARM_FORWARD - LIFT * e, SWEEP * e)
    } else if p < 0.55 {
        let u = (p - 0.25) / 0.30;
        let e = 1.0 - (1.0 - u) * (1.0 - u); // ease-out: fast crack, then settle
        (-(ARM_FORWARD + LIFT), SWEEP - 2.0 * SWEEP * e)
    } else {
        let u = (p - 0.55) / 0.45;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        (-(ARM_FORWARD + LIFT) + LIFT * e, -SWEEP * (1.0 - e))
    };
    Quat::from_euler(EulerRot::XYZ, x, y, 0.0)
}
