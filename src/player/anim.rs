//! Hero limb animation for the articulated knight rig. The walk/idle base pose is blended by the
//! hero's `moving_amt` (legs straighten at rest) with the stride driven by `walk_phase` (so the
//! gait stays locked to actual movement speed); on top sit the combat overrides — the attack slash
//! and the shield-block brace on the two-bone arms — plus the Director's staged gestures, the
//! first-person viewmodel raise, and a slack keel-over on death. Ported/adapted from the user's
//! three.js `updateKnightAnimation` (idle/walk) with game-authored attack/block.

use bevy::prelude::*;

use super::combat::ATTACK_DURATION;
use super::{Hero, HeroHealth, HeroPart, Joint};

/// Shield rest rotation under the left hand (matches its spawn pose); block swings it up to brace.
fn shield_rest() -> Quat {
    Quat::from_euler(EulerRot::XYZ, 0.2, -0.6, 0.15)
}
fn shield_block() -> Quat {
    Quat::from_euler(EulerRot::XYZ, 0.15, -0.15, 0.35)
}

fn e3(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}

pub fn hero_anim(
    time: Res<Time>,
    player: Res<super::PlayerRes>,
    dir: Res<crate::cinematic::DirectorState>,
    fp: Res<super::FirstPerson>,
    hero_q: Query<(&Hero, &HeroHealth)>,
    mut parts: Query<(&HeroPart, &mut Transform)>,
) {
    let Ok((hero, hh)) = hero_q.single() else { return };

    // Slain: let the limbs go slack while the body keels over (root rotation owned by health.rs).
    if !player.0.is_alive() {
        for (part, mut tf) in &mut parts {
            tf.rotation = match part.joint {
                Joint::Hips => {
                    tf.translation = Vec3::new(0.0, 0.95, 0.0);
                    Quat::IDENTITY
                }
                Joint::ShoulderL => e3(0.2, 0.0, -0.2),
                Joint::ShoulderR => e3(0.2, 0.0, 0.2),
                Joint::ElbowL | Joint::ElbowR => Quat::from_rotation_x(-0.3),
                Joint::Shield => shield_rest(),
                _ => Quat::IDENTITY,
            };
        }
        return;
    }

    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let wp = hero.walk_phase;
    let m = hero.moving_amt;
    let attack_p = hero.attacking.then(|| (hero.attack_t / ATTACK_DURATION).clamp(0.0, 1.0));
    let gesture = dir.gesture.map(|g| gesture_pose(g, t - dir.gesture_start));
    let fp_amt = fp.blend.clamp(0.0, 1.0);
    // Frame-rate-independent damp (~0.25s settle) for the block/shield transitions.
    let damp = 1.0 - 0.004_f32.powf(dt);

    for (part, mut tf) in &mut parts {
        let (base_t, base_r) = base_pose(part.joint, t, wp, m);
        if let Some(p) = base_t {
            tf.translation = p;
        }

        let rot = match part.joint {
            // Right arm (sword): gesture › attack › first-person viewmodel › locomotion.
            Joint::ShoulderR | Joint::ElbowR => {
                let elbow = part.joint == Joint::ElbowR;
                match gesture {
                    Some((Some((sh, el)), _)) => {
                        if elbow {
                            el
                        } else {
                            sh
                        }
                    }
                    _ => match attack_p {
                        Some(p) => {
                            let (sh, el) = attack_arm(p, base_r, elbow_rest(Joint::ElbowR, t, wp, m));
                            if elbow {
                                el
                            } else {
                                sh
                            }
                        }
                        None if fp_amt > 0.0 => {
                            let target = if elbow { Quat::from_rotation_x(-1.0) } else { e3(-0.9, 0.2, 0.1) };
                            base_r.slerp(target, fp_amt)
                        }
                        None => base_r,
                    },
                }
            }
            // Left arm (shield hand): gesture › block brace › locomotion.
            Joint::ShoulderL | Joint::ElbowL => {
                let elbow = part.joint == Joint::ElbowL;
                match gesture {
                    Some((_, Some((sh, el)))) => {
                        if elbow {
                            el
                        } else {
                            sh
                        }
                    }
                    _ if hh.blocking => {
                        let target = if elbow { Quat::from_rotation_x(-1.3) } else { e3(-1.2, 0.4, -0.4) };
                        tf.rotation.slerp(target, damp)
                    }
                    _ => base_r,
                }
            }
            // Shield (own pivot under the hand): braces up while blocking, else rests.
            Joint::Shield => {
                let target = if hh.blocking { shield_block() } else { shield_rest() };
                tf.rotation.slerp(target, damp)
            }
            _ => base_r,
        };
        tf.rotation = rot;
    }
}

/// The locomotion base pose for a joint: idle (breathing, `t`) blended toward walk (stride keyed to
/// `wp = walk_phase`) by `m = moving_amt`. `Some(translation)` only for the hips (the bob/sway);
/// every other joint keeps its spawn translation and only rotates.
fn base_pose(j: Joint, t: f32, wp: f32, m: f32) -> (Option<Vec3>, Quat) {
    let (it, ir) = idle_pose(j, t);
    let (wt, wr) = walk_pose(j, wp);
    let trans = match (it, wt) {
        (Some(a), Some(b)) => Some(a.lerp(b, m)),
        _ => None,
    };
    (trans, ir.slerp(wr, m))
}

/// Right elbow's *locomotion* rotation (used so the attack swing eases back to the live rest).
fn elbow_rest(j: Joint, t: f32, wp: f32, m: f32) -> Quat {
    base_pose(j, t, wp, m).1
}

fn idle_pose(j: Joint, t: f32) -> (Option<Vec3>, Quat) {
    let breath = (t * 2.2).sin();
    let sway = (t * 1.1).sin();
    let cos11 = (t * 1.1).cos();
    match j {
        Joint::Hips => (Some(Vec3::new(0.0, 0.95 + breath * 0.015, 0.0)), Quat::from_rotation_y(sway * 0.03)),
        Joint::Torso => (None, e3(breath * 0.01, -sway * 0.02, 0.0)),
        Joint::Head => (None, e3(-breath * 0.015, 0.0, sway * 0.01)),
        Joint::Plume => (None, e3(0.0, (t * 1.5).cos() * 0.06, breath * 0.05)),
        Joint::ShoulderL => (None, e3(breath * 0.05 + 0.1, 0.0, -0.15 + cos11 * 0.02)),
        Joint::ElbowL => (None, Quat::from_rotation_x(-0.5 - breath * 0.03)),
        Joint::ShoulderR => (None, e3(breath * 0.05 + 0.12, 0.0, 0.15 - cos11 * 0.02)),
        Joint::ElbowR => (None, Quat::from_rotation_x(-0.4 - breath * 0.02)),
        Joint::Shield => (None, shield_rest()),
        Joint::HipL | Joint::HipR | Joint::KneeL | Joint::KneeR => (None, Quat::IDENTITY),
    }
}

fn walk_pose(j: Joint, wp: f32) -> (Option<Vec3>, Quat) {
    let stride = wp.sin();
    let torso_y = -wp.sin() * 0.08;
    match j {
        Joint::Hips => (
            Some(Vec3::new(wp.sin() * 0.02, 0.93 + (wp * 2.0).sin().abs() * 0.04, 0.0)),
            e3(0.0, wp.sin() * 0.12, wp.sin() * 0.03),
        ),
        Joint::Torso => (None, e3(0.05 + (wp * 2.0).sin() * 0.02, torso_y, 0.0)),
        Joint::Head => (None, e3(-0.02 - (wp * 2.0).sin() * 0.01, -torso_y * 1.1, 0.0)),
        Joint::Plume => (None, e3(0.1 + (wp * 2.0).sin() * 0.15, 0.0, wp.cos() * 0.08)),
        Joint::HipL => (None, Quat::from_rotation_x(stride * 0.5)),
        Joint::KneeL => (None, Quat::from_rotation_x(if stride < 0.0 { -stride * 0.8 } else { -stride * 0.15 })),
        Joint::HipR => (None, Quat::from_rotation_x(-stride * 0.5)),
        Joint::KneeR => (None, Quat::from_rotation_x(if stride > 0.0 { stride * 0.8 } else { stride * 0.15 })),
        Joint::ShoulderL => (None, e3(-stride * 0.35 + 0.1, 0.0, -0.1)),
        Joint::ElbowL => (None, Quat::from_rotation_x(-0.6 - stride.abs() * 0.3)),
        Joint::ShoulderR => (None, e3(stride * 0.35 + 0.1, 0.0, 0.1)),
        Joint::ElbowR => (None, Quat::from_rotation_x(-0.5 - stride.abs() * 0.3)),
        Joint::Shield => (None, shield_rest()),
    }
}

/// A diagonal overhead slash on the two-bone sword arm. Eased windup (0–0.35) raises and cocks the
/// arm; a fast strike (0.35–0.6) sweeps it down/forward and extends the elbow; recovery (0.6–1)
/// settles back to the live locomotion rest (`sh_rest`/`el_rest`) so the swing blends with no pop.
fn attack_arm(p: f32, sh_rest: Quat, el_rest: Quat) -> (Quat, Quat) {
    let cocked_sh = e3(-1.7, -0.3, 0.4);
    let cocked_el = Quat::from_rotation_x(-1.7);
    let strike_sh = e3(0.4, 0.6, -0.3);
    let strike_el = Quat::from_rotation_x(-0.2);
    if p < 0.35 {
        let u = p / 0.35;
        let e = u * u; // accelerate into the cocked top
        (sh_rest.slerp(cocked_sh, e), el_rest.slerp(cocked_el, e))
    } else if p < 0.6 {
        let u = (p - 0.35) / 0.25;
        let e = 1.0 - (1.0 - u) * (1.0 - u); // ease-out crack
        (cocked_sh.slerp(strike_sh, e), cocked_el.slerp(strike_el, e))
    } else {
        let u = (p - 0.6) / 0.4;
        let e = 1.0 - (1.0 - u) * (1.0 - u);
        (strike_sh.slerp(sh_rest, e), strike_el.slerp(el_rest, e))
    }
}

/// Staged-gesture arm poses (Director). Returns `(right, left)`, each `Some((shoulder, elbow))` or
/// `None` to leave that arm on its normal animation. `ph` = seconds since the gesture began. Right
/// is the sword arm, left the shield arm. Re-authored from the old single-bone gestures onto the
/// two-bone rig. Rough by design — eyeball + nudge against a capture.
fn gesture_pose(g: crate::cinematic::HeroGesture, ph: f32) -> (Option<(Quat, Quat)>, Option<(Quat, Quat)>) {
    use crate::cinematic::HeroGesture::*;
    let raise = (ph / 0.45).clamp(0.0, 1.0);
    let e = raise * raise * (3.0 - 2.0 * raise); // smoothstep
    match g {
        // Arm raised overhead, hand flicking side to side (weapon auto-hidden by the Director).
        Wave => (Some((e3(-2.55 * e, 0.0, 0.20 + (ph * 5.0).sin() * 0.45 * e), Quat::from_rotation_x(-0.5))), None),
        Salute => (Some((e3(-2.6 * e, 0.0, 0.85 * e), Quat::from_rotation_x(-0.2))), None),
        Point => (Some((e3(-1.6 * e, 0.0, 0.05), Quat::from_rotation_x(-0.1))), None),
        ArmsCrossed => (
            Some((e3(-1.15 * e, 0.0, 0.85 * e), Quat::from_rotation_x(-1.4))),
            Some((e3(-1.15 * e, 0.0, -0.85 * e), Quat::from_rotation_x(-1.4))),
        ),
        Cheer => {
            let pump = (ph * 4.0).sin() * 0.15;
            (
                Some((e3((-2.75 + pump) * e, 0.0, -0.35 * e), Quat::from_rotation_x(-0.3))),
                Some((e3((-2.75 + pump) * e, 0.0, 0.35 * e), Quat::from_rotation_x(-0.3))),
            )
        }
        // Repeating chop (reuses the attack arc on a loop) — "at work". `max(0)`: the director
        // PRE_ROLL phase is negative; hold the start of the swing rather than walking it backwards.
        Work => {
            let (sh, el) = attack_arm((ph.max(0.0) * 1.3).fract(), e3(0.12, 0.0, 0.1), Quat::from_rotation_x(-0.5));
            (Some((sh, el)), None)
        }
    }
}
