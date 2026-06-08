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
    hero_q: Query<(&Hero, &HeroHealth, &Children)>,
    mut parts: Query<(&HeroPart, &mut Transform)>,
) {
    let Ok((hero, hh, children)) = hero_q.single() else { return };
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

    // Frame-rate-independent damp toward the shield's target pose (~0.25s settle).
    let damp = 1.0 - 0.004_f32.powf(dt);

    for &child in children {
        let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
        match part.limb {
            HeroLimb::LegR => tf.rotation = Quat::from_rotation_x(leg_swing),
            HeroLimb::LegL => tf.rotation = Quat::from_rotation_x(-leg_swing),
            HeroLimb::ArmR => {
                // Mid-swing → the slash (begins/ends at the forward rest pose, so no pop);
                // otherwise the arm rests forward (blade presented) + walk swing + idle sway.
                tf.rotation = match attack_p {
                    Some(p) => attack_arm_quat(p),
                    None => Quat::from_rotation_x(arm_swing + idle_sway - ARM_FORWARD),
                };
            }
            HeroLimb::ArmL => {
                tf.rotation = if blocking {
                    // Raise the shield arm across the front to brace behind the plate.
                    Quat::from_euler(EulerRot::XYZ, -1.25, 0.0, 0.4)
                } else {
                    Quat::from_rotation_x(-arm_swing - idle_sway)
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

/// A horizontal sword slash. Wind back + raise (0–0.25), sweep the blade across the front
/// fast (0.25–0.6), recover (0.6–1). Endpoints equal the forward rest pose `(x=-ARM_FORWARD,
/// y=0)` so the swing blends in and out with no snap.
fn attack_arm_quat(p: f32) -> Quat {
    const LIFT: f32 = 0.55; // extra raise during the swing
    const SWEEP: f32 = 1.25; // half the horizontal arc
    let (x, y) = if p < 0.25 {
        let u = p / 0.25;
        (-ARM_FORWARD - LIFT * u, SWEEP * u)
    } else if p < 0.6 {
        let u = (p - 0.25) / 0.35;
        (-(ARM_FORWARD + LIFT), SWEEP - 2.0 * SWEEP * u)
    } else {
        let u = (p - 0.6) / 0.4;
        (-(ARM_FORWARD + LIFT) + LIFT * u, -SWEEP * (1.0 - u))
    };
    Quat::from_euler(EulerRot::XYZ, x, y, 0.0)
}
