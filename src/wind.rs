//! Gentle CPU wind sway for trees (and any other entity tagged with [`Sway`]).
//!
//! This is the Bevy port of the TS foliage wind (`src/world/wind.ts`), but done on
//! the CPU as a per-entity `Transform` rotation rather than a vertex-shader
//! displacement. Reasons it's a rotation, not a vertex bend:
//! - It's shader-free and material-agnostic, so it composes with the shared white
//!   vertex-colour `StandardMaterial` the scatter uses without touching any pipeline
//!   or risking a shader recompile.
//! - The pivot is the entity origin, which the scatter places at the tree's *base*
//!   (`y = 0`, trunk bottom on the ground). Leaning about the base therefore keeps
//!   the trunk planted and swings the canopy — exactly the "height-weighted" feel the
//!   TS shader gets by squaring `transformed.y`.
//!
//! Frequency parity with the TS shader: the TS body drives X displacement with
//! `sin(t*1.5) + 0.4*sin(t*3.1)` and Z displacement with `cos(t*1.2)`. We reuse the
//! same 1.5 / 3.1 / 1.2 frequencies and the 0.4 secondary weight, and scale the TS
//! 0.045 / 0.035 *positional* amplitudes down into small *angular* amplitudes (radians)
//! so a ~1.5u-tall canopy sways a comparable amount without the trunk visibly shearing.
//!
//! Cost: one `Quat` compose + write per swaying entity **inside the camera radius**, per
//! frame. The island carries ~15–20k swayed trees; walking that whole query just to
//! distance-reject them was the remaining CPU cost after the write itself was gated.
//! A static XZ cell index (trees don't translate) visits only the cells the radius
//! overlaps. Fully deterministic given the per-instance `phase`.

use std::collections::HashMap;

use bevy::prelude::*;

/// Master angular amplitude for the primary (Z-axis) lean, in radians. Kept subtle
/// (~1.7°) so trunks stay believably planted; the canopy reads the motion because it
/// sits ~1–1.6u above the y=0 pivot. Scaled from the TS 0.045 positional amplitude.
const AMP_Z: f32 = 0.021;

/// Secondary angular amplitude for the cross (X-axis) lean, in radians. Smaller than
/// `AMP_Z` so the dominant sway direction stays legible, matching the TS 0.035 vs 0.045
/// X/Z split. The result is a gentle elliptical wander of the crown rather than a flat
/// metronome swing.
const AMP_X: f32 = 0.015;

/// Per-instance sway state. Inserted by the scatter on each tree (and optionally each
/// bush): `base` is the tree's authored Y-rotation (its cardinal/random yaw) which the
/// sway is composed on top of every frame; `phase` desynchronises neighbours so the
/// canopy doesn't pulse in lockstep.
///
/// The animating system OVERWRITES `Transform.rotation` each frame, so `base` must hold
/// the entity's intended rest rotation (the scatter must NOT also bake that yaw into the
/// spawned `Transform.rotation`, or it would be double-counted — pass it here instead).
#[derive(Component, Clone, Copy, Debug)]
pub struct Sway {
    /// Per-instance phase offset (radians) so neighbouring trees sway out of step.
    pub phase: f32,
    /// The entity's rest rotation (authored base yaw); the wind lean is layered on top.
    pub base: Quat,
}

/// Build a [`Sway`] for a prop at world `(x, z)` with rest rotation `base`.
///
/// The phase is a deterministic function of position — the same hash the TS shader uses
/// (`pos.x * 0.7 + pos.z * 0.55`) — so two trees at the same spot always pick the same
/// phase and the layout stays stable across runs. The scatter should call this and
/// insert the result on each tree entity, passing the yaw it would otherwise have baked
/// into the `Transform` as `base`.
pub fn sway_for(x: f32, z: f32, base: Quat) -> Sway {
    Sway { phase: x * 0.7 + z * 0.55, base }
}

/// Adds the wind-sway `Update` system. Insert this plugin in `main.rs`; the scatter is
/// responsible for attaching [`Sway`] (via [`sway_for`]) to the entities that should move.
pub struct WindPlugin;

impl Plugin for WindPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwayIndex>().add_systems(Update, (sync_sway_index, sway_system).chain());
    }
}

/// Cell edge for the sway index. 32u puts the 70u radius over a 5×5 block of cells, so a
/// frame touches a few dozen buckets instead of every tree on the island. Trees are indexed
/// by the translation they spawn with — felling and sway only rewrite rotation.
const SWAY_CELL: f32 = 32.0;

#[derive(Resource, Default)]
pub(crate) struct SwayIndex {
    cells: HashMap<(i32, i32), Vec<Entity>>,
    /// Cell a tree was filed under, so a chop/despawn can pull it back out.
    by_entity: HashMap<Entity, (i32, i32)>,
}

fn sway_cell(p: Vec3) -> (i32, i32) {
    ((p.x / SWAY_CELL).floor() as i32, (p.z / SWAY_CELL).floor() as i32)
}

/// File newly spawned trees and drop despawned ones. Runs immediately before [`sway_system`]
/// so a tree sways on the frame it appears and stops being visited the frame it goes.
fn sync_sway_index(
    added: Query<(Entity, &Transform), Added<Sway>>,
    mut removed: RemovedComponents<Sway>,
    mut index: ResMut<SwayIndex>,
) {
    for e in removed.read() {
        if let Some(cell) = index.by_entity.remove(&e) {
            if let Some(bucket) = index.cells.get_mut(&cell) {
                bucket.retain(|x| *x != e);
            }
        }
    }
    for (e, tf) in &added {
        let cell = sway_cell(tf.translation);
        index.cells.entry(cell).or_default().push(e);
        index.by_entity.insert(e, cell);
    }
}

/// XZ radius (world units) beyond which tree sway is skipped entirely.
///
/// Past this distance the per-vertex angular displacement is sub-pixel, and the
/// distance fog has already absorbed the canopy into the background. More importantly,
/// *not* writing `Transform.rotation` avoids dirtying Bevy's change-detection for the
/// ~15–20k out-of-range trees each frame, which in turn skips their transform
/// propagation and render-world extraction — the main CPU cost of the system.
/// Trees outside the radius simply freeze at their last sway angle; the amplitude is
/// small enough (~1.7°) that the freeze is visually undetectable.
const SWAY_RADIUS: f32 = 70.0;
const SWAY_RADIUS_SQ: f32 = SWAY_RADIUS * SWAY_RADIUS;

fn apply_sway(sway: &Sway, tf: &mut Transform, t: f32) {
    let p = sway.phase;
    // Primary lean about Z (the TS X-displacement term): a base gust plus a faster
    // 0.4-weighted ripple, the exact 1.5 / 3.1 frequencies from wind.ts.
    let lean_z = ((t * 1.5 + p).sin() + 0.4 * (t * 3.1 + p * 1.7).sin()) * AMP_Z;
    // Cross lean about X (the TS Z-displacement term): the slower 1.2 cosine.
    let lean_x = (t * 1.2 + p * 1.1).cos() * AMP_X;

    // Compose: lean first, then the rest yaw — so the lean tilts the whole tree
    // about its planted base while preserving the authored facing. Writing the full
    // rotation each frame (rather than accumulating) keeps it drift-free.
    tf.rotation = Quat::from_rotation_z(lean_z) * Quat::from_rotation_x(lean_x) * sway.base;
}

/// Each frame, recompute every in-range [`Sway`] entity's rotation as `lean * base`.
/// `pub(crate)` so the chop-impact systems (`verbs.rs`) can order `.after()` it — they layer a
/// trunk shudder / felling topple on top of the rotation this writes.
pub(crate) fn sway_system(
    time: Res<Time>,
    index: Res<SwayIndex>,
    cam_q: Query<&GlobalTransform, With<Camera3d>>,
    mut q: Query<(&Sway, &mut Transform)>,
) {
    // `elapsed_secs_wrapped` (wraps at 3600s by default) keeps f32 precision sharp over
    // long sessions; the wrap period is far longer than any sway period so there's no
    // visible jump when it wraps.
    let t = time.elapsed_secs_wrapped();

    // Resolve the camera's XZ position for distance gating. If no camera exists yet
    // (e.g. during early startup frames) we fall back to animating everything so the
    // first visible frame looks correct.
    let Some(cam) = cam_q.iter().next() else {
        for (sway, mut tf) in &mut q {
            apply_sway(sway, &mut tf, t);
        }
        return;
    };
    let cam_xz = {
        let p = cam.translation();
        Vec2::new(p.x, p.z)
    };

    // Every cell the radius' bounding square can touch. A tree inside the radius is in one
    // of these cells; the distance test below still rejects the square's corners.
    let min_tx = ((cam_xz.x - SWAY_RADIUS) / SWAY_CELL).floor() as i32;
    let max_tx = ((cam_xz.x + SWAY_RADIUS) / SWAY_CELL).floor() as i32;
    let min_tz = ((cam_xz.y - SWAY_RADIUS) / SWAY_CELL).floor() as i32;
    let max_tz = ((cam_xz.y + SWAY_RADIUS) / SWAY_CELL).floor() as i32;
    for tz in min_tz..=max_tz {
        for tx in min_tx..=max_tx {
            let Some(ids) = index.cells.get(&(tx, tz)) else { continue };
            for &e in ids {
                let Ok((sway, mut tf)) = q.get_mut(e) else { continue };
                // Distance gate — read .translation via Deref (does NOT dirty change detection).
                let tree_xz = Vec2::new(tf.translation.x, tf.translation.z);
                if tree_xz.distance_squared(cam_xz) > SWAY_RADIUS_SQ {
                    continue;
                }
                apply_sway(sway, &mut tf, t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_tree(world: &mut World, x: f32, z: f32) -> Entity {
        world
            .spawn((
                Transform::from_xyz(x, 0.0, z),
                sway_for(x, z, Quat::IDENTITY),
            ))
            .id()
    }

    /// Trees inside the radius lean; trees outside keep the rotation they spawned with.
    /// Pins the cell index to the same cut the old full-query distance gate used.
    #[test]
    fn sway_reaches_nearby_trees_and_leaves_distant_ones_still() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, WindPlugin));
        let cam = Transform::from_xyz(0.0, 4.0, 0.0);
        app.world_mut().spawn((Camera3d::default(), cam, GlobalTransform::from(cam)));
        let near = spawn_tree(app.world_mut(), 12.0, 0.0);
        let edge = spawn_tree(app.world_mut(), 69.0, 0.0);
        let far = spawn_tree(app.world_mut(), 140.0, 0.0);
        app.update();

        let near_r = *app.world().get::<Transform>(near).unwrap();
        let edge_r = *app.world().get::<Transform>(edge).unwrap();
        let far_r = *app.world().get::<Transform>(far).unwrap();
        assert_ne!(near_r.rotation, Quat::IDENTITY, "a tree 12u from the camera must sway");
        assert_ne!(edge_r.rotation, Quat::IDENTITY, "a tree just inside 70u must still sway");
        assert_eq!(far_r.rotation, Quat::IDENTITY, "a tree at 140u must keep its rest rotation");
    }
}
