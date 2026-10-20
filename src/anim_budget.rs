//! **Animation budget** — Unreal's update-rate optimisation (URO) / animation budget allocator for
//! the skinned crowd. `animate_targets` re-evaluates every bone of every animated character every
//! frame (~3 µs a bone, ~55 bones a person: the village's 41 graphed players cost ~7.5 ms of worker
//! time, and their bone writes feed transform propagation). A person 40 u away is ~70 px tall: a
//! pose refreshed every third frame reads the same. So each frame this picks which players to
//! evaluate by how much of the screen they cover:
//!
//! - on screen: every frame up close, then every 2nd / 3rd / 4th frame with distance;
//! - off screen (they still cast shadows into view): every 2nd frame nearby, every 6th beyond.
//!
//! Skipped players are handed to the vendored `bevy_animation` (`AnimationTickSkip`, WARBELL
//! PATCH): their clocks keep running, so the next evaluated frame lands on the right pose, and
//! their untouched bones also skip transform propagation. Ticks are staggered by entity so the
//! work is spread evenly over frames rather than spiking on every 4th.
//! Complements `anim_lod` (which detaches far / long-unseen skins' graphs entirely).
//! `FOREST_ANIMBUDGET=0` turns it off (A/B).

use bevy::animation::{AnimationPlayer, AnimationTickSkip};
use bevy::camera::primitives::{Frustum, Sphere};
use bevy::prelude::*;

/// (distance, frame period) for players in view: closer than the distance → that period.
const IN_VIEW: [(f32, u32); 3] = [(16.0, 1), (32.0, 2), (55.0, 3)];
const IN_VIEW_FAR: u32 = 4;
/// Off-screen players: nearer than this tick every 2nd frame (shadows / a quick turn), else every 6th.
const OFF_NEAR: f32 = 12.0;
const OFF_NEAR_PERIOD: u32 = 2;
const OFF_FAR_PERIOD: u32 = 6;
/// Bounding sphere radius used for the in-view test (a person / a horse).
const BODY_R: f32 = 1.6;

pub struct AnimBudgetPlugin;

impl Plugin for AnimBudgetPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.add_systems(PostUpdate, pick_ticks.before(bevy::app::AnimationSystems));
    }
}

fn enabled() -> bool {
    // Off under the people-collision test, which reads every villager's live pose.
    std::env::var("FOREST_ANIMBUDGET").map(|v| v != "0").unwrap_or(true) && std::env::var_os("FOREST_LIFECHECK").is_none()
}

fn period(in_view: bool, d: f32) -> u32 {
    if !in_view {
        return if d < OFF_NEAR { OFF_NEAR_PERIOD } else { OFF_FAR_PERIOD };
    }
    IN_VIEW.iter().find(|(r, _)| d < *r).map_or(IN_VIEW_FAR, |(_, p)| *p)
}

fn pick_ticks(
    mut frame: Local<u32>,
    mut skip: ResMut<AnimationTickSkip>,
    cam: Query<(&GlobalTransform, &Frustum), With<Camera3d>>,
    players: Query<(Entity, &GlobalTransform), With<AnimationPlayer>>,
) {
    *frame = frame.wrapping_add(1);
    skip.0.clear();
    let Ok((cam_gt, frustum)) = cam.single() else { return };
    let eye = cam_gt.translation();
    for (e, gt) in &players {
        let p = gt.translation();
        let d = p.distance(eye);
        let in_view = frustum.intersects_sphere(&Sphere { center: (p + Vec3::Y).into(), radius: BODY_R }, false);
        let n = period(in_view, d);
        if n > 1 && (*frame).wrapping_add(e.index_u32()) % n != 0 {
            skip.0.insert(e);
        }
    }
}
