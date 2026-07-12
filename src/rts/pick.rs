//! Cursor→world picking for the RTS mode: an iso-camera viewport ray refined against the terrain,
//! plus screen-space hit tests (nearest-candidate, rect containment) that the selection/command
//! layers build on.
//!
//! **Pure helpers only** — no systems, no ECS state (the plugin registers nothing). Every fn takes
//! the single `Camera` + its `GlobalTransform` (fetched by the caller off the one `Camera3d`, per
//! the single-camera hard rule) so `select.rs` / `command.rs` reuse them without a plugin
//! dependency. Callers pre-filter their candidate lists by `Side` / `Without<Dying>`, so these stay
//! about projection math, not gameplay policy.

use bevy::prelude::*;

use crate::worldmap::ground_at_world;

pub struct RtsPickPlugin;
impl Plugin for RtsPickPlugin {
    // Nothing to register: this module is a bag of pure helper fns the sibling input systems call.
    fn build(&self, _app: &mut App) {}
}

/// Screen-space pick radius (px) for a unit body under the cursor (spec §4, widened for feel).
pub const UNIT_PICK_PX: f32 = 24.0;

/// Where on a unit body the pick ray tests: entity translation is the FEET, but the player clicks
/// the torso/head — under the ~50°-pitch iso cam the feet project a good 20px below the visual
/// body, so picking at the feet made clicks on the body miss. Lift the sample to mid-chest.
pub const UNIT_PICK_Y: f32 = 0.9;
/// Larger radius for buildings — bigger footprints read from farther on screen.
pub const BUILDING_PICK_PX: f32 = 40.0;
/// Deposits are chunky world features; a generous radius so RMB-harvest is forgiving.
pub const DEPOSIT_PICK_PX: f32 = 34.0;

/// HUD keep-out bands (px) at the top / bottom of the screen. The RTS HUD (sibling `hud.rs`) parks
/// its resource bar / build strip / selection panel in these bands, so clicks inside them belong to
/// the UI, not to world selection/commands. A pragmatic screen-region guard for the POC (the design
/// accepts this in §4 until a proper `Interaction`-node test is wired).
pub const HUD_TOP_PX: f32 = 92.0;
pub const HUD_BOTTOM_PX: f32 = 96.0;

/// True if `cursor` sits inside one of the HUD keep-out bands and should be ignored by the world
/// input layers.
pub fn over_hud(cursor: Vec2, window_height: f32) -> bool {
    cursor.y < HUD_TOP_PX || cursor.y > window_height - HUD_BOTTOM_PX
}

/// Cursor → terrain world XZ. Under the iso ORTHO projection the viewport ray is **parallel**, so a
/// single `y=0` hit is wrong on sloped ground: we seed on the ground plane then refine the
/// ray/terrain intersection against `ground_at_world` a few times (the arena is gentle, so 3
/// iterations converge). Returns `None` if the ray is degenerate (parallel to the ground).
pub fn cursor_ray_ground(camera: &Camera, cam_tf: &GlobalTransform, cursor: Vec2) -> Option<Vec2> {
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let origin = ray.origin;
    let dir: Vec3 = *ray.direction;
    if dir.y.abs() < 1e-5 {
        return None; // ray parallel to the ground — no intersection
    }
    // Seed on the y=0 plane, then walk the hit onto the real terrain height.
    let t0 = -origin.y / dir.y;
    let mut wx = origin.x + dir.x * t0;
    let mut wz = origin.z + dir.z * t0;
    for _ in 0..3 {
        let gy = ground_at_world(wx, wz).unwrap_or(0.0);
        let t = (gy - origin.y) / dir.y;
        wx = origin.x + dir.x * t;
        wz = origin.z + dir.z * t;
    }
    Some(Vec2::new(wx, wz))
}

/// Project a world point to viewport px (None if it can't be projected this frame).
pub fn project(camera: &Camera, cam_tf: &GlobalTransform, world: Vec3) -> Option<Vec2> {
    camera.world_to_viewport(cam_tf, world).ok()
}

/// From `(entity, world-pos)` candidates, the one whose screen projection is nearest `cursor` and
/// within `radius_px`. Callers pre-filter candidates by `Side` / `Without<Dying>`.
pub fn nearest_within<I>(
    camera: &Camera,
    cam_tf: &GlobalTransform,
    cursor: Vec2,
    radius_px: f32,
    candidates: I,
) -> Option<Entity>
where
    I: IntoIterator<Item = (Entity, Vec3)>,
{
    let mut best: Option<Entity> = None;
    let mut best_d = radius_px;
    for (e, world) in candidates {
        if let Some(sp) = project(camera, cam_tf, world) {
            let d = sp.distance(cursor);
            if d <= best_d {
                best_d = d;
                best = Some(e);
            }
        }
    }
    best
}

/// True if `world` projects inside the (min,max) screen rect — the rubber-band containment test.
pub fn in_screen_rect(camera: &Camera, cam_tf: &GlobalTransform, world: Vec3, min: Vec2, max: Vec2) -> bool {
    project(camera, cam_tf, world).is_some_and(|sp| sp.x >= min.x && sp.x <= max.x && sp.y >= min.y && sp.y <= max.y)
}
