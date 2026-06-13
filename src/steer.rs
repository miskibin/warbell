//! Shared **local obstacle steering** — the cheap "no A*" navigation used by both the
//! ambient wildlife and the camp orks. Pure functions (no ECS): given a position, current
//! facing and a goal point, pick the next step around props/cliffs.
//!
//! This is the cure for the steering-oscillation flicker (entities snapping 180° per frame):
//! an escape-direction fan scored by goal-alignment PLUS a continuity bias toward the current
//! heading (so the entity COMMITS to one way around an obstacle), a turn-rate cap (so facing
//! can never snap), and walk-along-the-slewed-facing (so the body follows where it looks).
//!
//! Footing + props are sampled via [`crate::worldmap::ground_at_world`] and [`crate::blockers`].

use bevy::prelude::*;

/// Max ground-height delta (world Y) an entity may step across in one frame — one terrace
/// class (`GROUND_STEP = 0.5`) plus slack. Two-class cliff faces (≥1.0) block, so movers route
/// around them and never walk into water (`ground_at_world` → `None`).
pub const MAX_STEP: f32 = 0.6;

/// Footing height at `(x, z)`: terrain, or the bridge deck where the river shows through.
/// `worldmap::ground_at_world` is terrain-only (`None` over the river), but `can_stand` lets a
/// mover step ONTO a deck (A* threads NPCs across the river over the planks), so EVERY mover that
/// follows a steered/A* route must ground off this — not raw `ground_at_world`. If it doesn't, its
/// render/step `cur_y` freezes at the bank height the moment it walks onto the deck: it then floats
/// above the planks and wedges (every deck cell reads `> MAX_STEP` from the stale height, so
/// `can_stand` rejects every step). This is THE shared footing — the hero's `player::movement`
/// copy mirrors it; wildlife / villagers / lumberjack / miner / camp orks / siege invaders all use
/// it directly.
pub fn footing(x: f32, z: f32) -> Option<f32> {
    crate::worldmap::ground_at_world(x, z).or_else(|| crate::bridges::deck_y_at(x, z))
}

/// True if a mover with footprint radius `r` can stand at `(x, z)`: its centre and the four
/// cardinal footprint edges must all be on land (**or a bridge deck**, via [`footing`]) within
/// `MAX_STEP` of the current height. Keeps the whole body off water and from overhanging cliff
/// faces.
///
/// The bridge fallback mirrors the hero's footing (`player::movement`) and the nav-grid's own
/// `standable`/`can_step`: A* threads NPCs across the river over a deck, so the local steering that
/// FOLLOWS that route must agree the deck is solid — otherwise the deck reads as open water and the
/// mover wedges at the bank, abandoning every cross-river goal (the stone miner's ore trip).
pub fn can_stand(x: f32, z: f32, r: f32, cur_y: f32) -> bool {
    const OFF: [(f32, f32); 5] = [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];
    for (dx, dz) in OFF {
        let (sx, sz) = (x + dx * r, z + dz * r);
        match footing(sx, sz) {
            Some(y) if (y - cur_y).abs() <= MAX_STEP => {}
            _ => return false,
        }
    }
    true
}

/// True if a step of `dist` along `dir` from `pos` keeps the footprint on safe ground and clear
/// of props (centre + lead point).
pub fn step_clear(pos: Vec2, dir: Vec2, dist: f32, body_r: f32, cur_y: f32) -> bool {
    let np = pos + dir * dist;
    let lead = np + dir * body_r;
    if !can_stand(np.x, np.y, body_r, cur_y) {
        return false;
    }
    // Already inside a blocker (e.g. a building raised over the spot the mover was standing
    // on): waive the prop test so it can walk out — normal collision resumes once clear.
    if crate::blockers::is_blocked(pos.x, pos.y) {
        return true;
    }
    !crate::blockers::is_blocked(np.x, np.y) && !crate::blockers::is_blocked(lead.x, lead.y)
}

/// Wrap an angle to (-π, π].
pub fn wrap_pi(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut x = (a + PI).rem_euclid(TAU) - PI;
    if x <= -PI {
        x += TAU;
    }
    x
}

/// The result of one steering step: the new facing, the new position, and whether the mover
/// actually advanced (vs. pivoted in place toward an opening).
pub struct Step {
    pub facing: f32,
    pub pos: Vec2,
    pub moving: bool,
}

/// Choose the next step from `pos` toward `goal`. `step_dist` is how far the mover travels this
/// frame (`speed * dt`); `max_turn_dt` is the max facing change this frame (`MAX_TURN * dt`).
///
/// Returns `None` when boxed in (no clear escape heading) — the caller should pause/re-plan.
pub fn advance(
    pos: Vec2,
    facing: f32,
    goal: Vec2,
    step_dist: f32,
    body_r: f32,
    cur_y: f32,
    max_turn_dt: f32,
) -> Option<Step> {
    let to = goal - pos;
    let dist = to.length();
    if dist < 1e-4 {
        return None;
    }
    let base = to / dist;
    let cur_dir = Vec2::new(facing.sin(), facing.cos());

    // Among the clear escape headings pick the one scored by goal-alignment PLUS a continuity
    // bias toward the current heading, so the mover COMMITS to one way around an obstacle
    // instead of flip-flopping between the two ±97° escapes every frame (the jitter bug).
    let mut best: Option<Vec2> = None;
    let mut best_score = f32::NEG_INFINITY;
    for off in [0.0f32, 0.4, -0.4, 0.8, -0.8, 1.2, -1.2, 1.7, -1.7] {
        let dir = Vec2::from_angle(off).rotate(base);
        if step_clear(pos, dir, step_dist, body_r, cur_y) {
            let score = dir.dot(base) + 0.6 * dir.dot(cur_dir);
            if score > best_score {
                best_score = score;
                best = Some(dir);
            }
        }
    }
    let dir = best?;

    // Turn toward it at a CAPPED rate (no instant snaps), then walk the way we're now facing if
    // that step is clear (else just pivot toward the opening).
    let want = dir.x.atan2(dir.y);
    let new_facing = facing + wrap_pi(want - facing).clamp(-max_turn_dt, max_turn_dt);
    let fdir = Vec2::new(new_facing.sin(), new_facing.cos());
    if step_clear(pos, fdir, step_dist, body_r, cur_y) {
        Some(Step { facing: new_facing, pos: pos + fdir * step_dist, moving: true })
    } else {
        Some(Step { facing: new_facing, pos, moving: false })
    }
}
