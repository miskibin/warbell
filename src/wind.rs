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
//! Cost: one `Quat` compose + write per swaying entity per frame, in a single `Update`
//! system. No allocations, no notifies, fully deterministic given the per-instance
//! `phase`.

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
        app.add_systems(Update, sway_system);
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

/// Each frame, recompute every [`Sway`] entity's rotation as `lean * base`, where the
/// lean is a small two-axis wobble driven by elapsed time + the instance phase.
/// `pub(crate)` so the chop-impact systems (`verbs.rs`) can order `.after()` it — they layer a
/// trunk shudder / felling topple on top of the rotation this writes.
pub(crate) fn sway_system(
    time: Res<Time>,
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
    let cam_xz: Option<Vec2> = cam_q.iter().next().map(|gt| {
        let p = gt.translation();
        Vec2::new(p.x, p.z)
    });

    for (sway, mut tf) in &mut q {
        // Distance gate — read .translation via Deref (does NOT dirty change detection).
        // Only skip the write if we have a known camera position.
        if let Some(cxz) = cam_xz {
            let tree_xz = Vec2::new(tf.translation.x, tf.translation.z);
            if tree_xz.distance_squared(cxz) > SWAY_RADIUS_SQ {
                // Out of range: leave rotation untouched, do not DerefMut tf.
                continue;
            }
        }

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
}
