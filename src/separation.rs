//! **Inter-agent separation** — keeps orks and townsfolk from clipping *through each other*.
//!
//! The shared local steering (`steer.rs`) only routes a mover around the *static* obstacle set
//! (`blockers.rs`: trunks, walls, buildings) — it has no idea where the *other* movers are, so a
//! warband charging the same hero, or a knot of guards on one invader, would pile into one
//! overlapping blob. The hero already shoves out of creature bodies one-way
//! (`player::movement::shove_out_of`); this is the symmetric version for the AI crowd: each frame
//! every live ork + villager + animal body that overlaps another gets nudged apart, so two bodies
//! meet at their skins instead of merging.
//!
//! It's a relaxation pass, not a hard constraint: each overlap is resolved a *fraction* per frame
//! (full per-frame resolution makes dense piles jitter), capped to a small step, and slid against
//! the same standable-ground + blocker rules locomotion uses — so a shove can never punt a body
//! through a wall or off a cliff. Runs gated on `Modal::None` like every other sim system.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::orks::Ork;
use crate::villagers::Villager;
use crate::wildlife::Animal;
use crate::{blockers, steer};

pub struct SeparationPlugin;

impl Plugin for SeparationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            separate_agents.run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

/// Fraction of each pair's overlap resolved per frame. Full resolution (1.0) snaps dense piles
/// into a jitter; half settles smoothly as the brains re-step.
const RELAX: f32 = 0.5;
/// Max separation nudge applied to one body in one frame (world units) — a backstop so a deep
/// pile can't teleport a body across a wall in a single step.
const MAX_PUSH: f32 = 0.3;

/// One body in the crowd: its entity, centre (world XZ) and collision radius.
type Body = (Entity, Vec2, f32);

#[allow(clippy::type_complexity)]
fn separate_agents(
    mut set: ParamSet<(
        Query<(Entity, &mut Ork, &mut Transform), Without<crate::dying::Dying>>,
        Query<(Entity, &mut Villager, &mut Transform), Without<crate::dying::Dying>>,
        Query<(Entity, &mut Animal, &mut Transform), Without<crate::dying::Dying>>,
    )>,
) {
    // 1. Snapshot every live ork + villager + animal body. (Read through the mut queries one at a
    // time — all three touch `Transform`, which is why they live in a `ParamSet`.)
    let mut bodies: Vec<Body> = Vec::new();
    for (e, o, _) in set.p0().iter() {
        bodies.push((e, o.pos, o.body_r));
    }
    for (e, v, _) in set.p1().iter() {
        bodies.push((e, v.pos, v.body_r));
    }
    for (e, a, _) in set.p2().iter() {
        bodies.push((e, a.pos, a.body_r));
    }
    if bodies.len() < 2 {
        return;
    }

    // 2. Bucket by 1-unit tile and resolve each overlapping pair once. Bodies overlap only within
    // `r_i + r_j` (≤ ~0.8 < 1 tile), so a 3×3 neighbour scan catches every real pair.
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, (_, p, _)) in bodies.iter().enumerate() {
        grid.entry((p.x.floor() as i32, p.y.floor() as i32)).or_default().push(i);
    }
    let mut push = vec![Vec2::ZERO; bodies.len()];
    for (i, &(_, pi, ri)) in bodies.iter().enumerate() {
        let (tx, tz) = (pi.x.floor() as i32, pi.y.floor() as i32);
        for dx in -1..=1 {
            for dz in -1..=1 {
                let Some(cell) = grid.get(&(tx + dx, tz + dz)) else { continue };
                for &j in cell {
                    if j <= i {
                        continue; // each unordered pair exactly once
                    }
                    let (_, pj, rj) = bodies[j];
                    let delta = pi - pj;
                    let d = delta.length();
                    let min_d = ri + rj;
                    if d >= min_d {
                        continue;
                    }
                    // Separation axis (deterministic split when two centres coincide).
                    let dir = if d > 1e-4 {
                        delta / d
                    } else {
                        let a = i as f32 * 2.399_963; // golden-angle scatter, stable per index
                        Vec2::new(a.cos(), a.sin())
                    };
                    let half = dir * ((min_d - d) * 0.5 * RELAX);
                    push[i] += half;
                    push[j] -= half;
                }
            }
        }
    }

    // 3. Map each entity to its accumulated nudge (skip the bodies that aren't overlapping).
    let pushes: HashMap<Entity, Vec2> = bodies
        .iter()
        .zip(push.iter())
        .filter(|(_, p)| p.length_squared() > 1e-8)
        .map(|((e, _, _), p)| (*e, *p))
        .collect();
    if pushes.is_empty() {
        return;
    }

    // 4. Apply, sliding against the same standable + blocker rules as locomotion.
    for (e, mut o, mut tf) in set.p0().iter_mut() {
        if let Some(&p) = pushes.get(&e) {
            let r = o.body_r;
            apply(&mut o.pos, &mut tf, r, p);
        }
    }
    for (e, mut v, mut tf) in set.p1().iter_mut() {
        if let Some(&p) = pushes.get(&e) {
            let r = v.body_r;
            apply(&mut v.pos, &mut tf, r, p);
        }
    }
    for (e, mut a, mut tf) in set.p2().iter_mut() {
        if let Some(&p) = pushes.get(&e) {
            let r = a.body_r;
            apply(&mut a.pos, &mut tf, r, p);
        }
    }
}

/// Slide `pos` by the (capped) nudge `push`, axis-separated so a blocked axis still lets the other
/// move, and refuse any step onto unstandable ground or into a prop/wall blocker. Keeps the
/// transform's Y (the brain's ground-follow + bob this frame) untouched — only XZ is corrected.
fn apply(pos: &mut Vec2, tf: &mut Transform, body_r: f32, mut push: Vec2) {
    let len = push.length();
    if len < 1e-5 {
        return;
    }
    if len > MAX_PUSH {
        push *= MAX_PUSH / len;
    }
    let cur_y = steer::footing(pos.x, pos.y).unwrap_or(tf.translation.y);
    let nx = pos.x + push.x;
    let nz = pos.y + push.y;
    if steer::can_stand(nx, pos.y, body_r, cur_y) && !blockers::is_blocked(nx, pos.y) {
        pos.x = nx;
    }
    if steer::can_stand(pos.x, nz, body_r, cur_y) && !blockers::is_blocked(pos.x, nz) {
        pos.y = nz;
    }
    tf.translation.x = pos.x;
    tf.translation.z = pos.y;
}
