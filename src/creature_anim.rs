//! Shared procedural animation helpers layered onto the existing sin-gait limb systems
//! (`ork_limbs`, `animal_limbs`, hero `hero_anim`). All pure — no ECS — so each rig calls them
//! and composes the result with its current rotations. No skeleton, no keyframes.

use bevy::prelude::*;

/// Local-yaw the head should add to glance toward `target` from a creature at `pos` facing
/// `facing` (world). Clamped to `max` rad. Returns 0 when the target is coincident.
/// Uses the steer convention (x = sin(facing), y = cos(facing)).
pub fn head_look_yaw(pos: Vec2, facing: f32, target: Vec2, max: f32) -> f32 {
    let to = target - pos;
    if to.length_squared() < 1e-4 {
        return 0.0;
    }
    let want = to.x.atan2(to.y);
    let rel = wrap_pi(want - facing);
    rel.clamp(-max, max)
}

/// A gentle idle breathing/weight-shift offset given a phase `t` (seconds + per-instance phase).
/// Returns (pitch, roll) radians to add to the torso/head while idle. Tiny by design.
pub fn idle_micro(t: f32) -> (f32, f32) {
    let breath = (t * 1.1).sin() * 0.02;
    let shift = (t * 0.37).sin() * 0.015;
    (breath, shift)
}

/// Tuning for [`idle_head_glance`] — how a given rig glances around when idle.
#[derive(Clone, Copy)]
pub struct GlanceCfg {
    /// Max distance the creature will track `target` before falling back to a scan.
    pub range: f32,
    /// Max local-yaw the head turns toward `target` (rad).
    pub max_yaw: f32,
    /// Amplitude of the slow idle scan when there's no target (rad).
    pub scan_amp: f32,
    /// Amplitude of the vertical idle bob (rad).
    pub bob_amp: f32,
}

/// Composed idle head rotation shared by `ork_limbs` and `animal_limbs`: a standing creature
/// glances toward `target` (when alert and in range), else does a slow scan; a moving one looks
/// straight ahead. Breathing always rides on the pitch. Pass `target = Some(hero.pos)` only when
/// the creature is alert (e.g. orks gate out their Attack mode at the call site), `None` otherwise.
pub fn idle_head_glance(pos: Vec2, facing: f32, t: f32, moving: bool, target: Option<Vec2>, cfg: GlanceCfg) -> Quat {
    let (breath, _) = idle_micro(t);
    let look = match target {
        Some(tp) if !moving && pos.distance(tp) < cfg.range => head_look_yaw(pos, facing, tp, cfg.max_yaw),
        _ if moving => 0.0,
        _ => (t * 0.4).sin() * cfg.scan_amp,
    };
    let bob = (t * 0.5).sin() * cfg.bob_amp + breath;
    Quat::from_euler(EulerRot::XYZ, bob, look, 0.0)
}

/// Bank (local Z roll) to lean into a turn, from the per-frame facing delta `dyaw` (rad this
/// frame) and `dt`. Returns a clamped roll; scale by speed at the call site if desired.
pub fn turn_lean(dyaw: f32, dt: f32, max: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    (-dyaw / dt * 0.08).clamp(-max, max)
}

/// Run vs walk swing amplitude multiplier: 1.0 at walk, up to `peak` when `fast`.
pub fn gait_amp(fast: bool, peak: f32) -> f32 {
    if fast {
        peak
    } else {
        1.0
    }
}

/// Wrap an angle to (-PI, PI]. (Local copy so the helper has no cross-module dep.)
pub fn wrap_pi(a: f32) -> f32 {
    let mut x = a;
    while x > std::f32::consts::PI {
        x -= std::f32::consts::TAU;
    }
    while x <= -std::f32::consts::PI {
        x += std::f32::consts::TAU;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_look_clamps_and_centers() {
        // target straight ahead (facing +Z means want = atan2(0,1) = 0)
        assert!(head_look_yaw(Vec2::ZERO, 0.0, Vec2::new(0.0, 5.0), 0.6).abs() < 1e-5);
        // target hard left, clamped to max
        let y = head_look_yaw(Vec2::ZERO, 0.0, Vec2::new(-5.0, 0.01), 0.6);
        assert!((y + 0.6).abs() < 1e-3, "got {y}");
    }

    #[test]
    fn turn_lean_opposes_turn_and_clamps() {
        let l = turn_lean(1.0, 0.1, 0.3); // turning +, lean negative
        assert!(l < 0.0 && l >= -0.3);
        assert_eq!(turn_lean(100.0, 0.1, 0.3), -0.3); // clamped
    }

    #[test]
    fn gait_amp_switches() {
        assert_eq!(gait_amp(false, 1.6), 1.0);
        assert_eq!(gait_amp(true, 1.6), 1.6);
    }
}
