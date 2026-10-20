//! **Shadow LOD** — static `StandardMaterial` meshes stop casting sun shadows past a distance that
//! scales with their size, and pick it back up when the camera returns.
//!
//! The far cascade reaches ~330u at Extreme, and Bevy's cascade culling extrudes it toward the sun,
//! so it collected ~8k casters in a forest view — desert cacti ~330u away, swamp dressing ~220u away
//! — whose shadows land as a few hazy texels nobody can see, yet each costs a shadow draw plus a bin
//! entry in every frame's render prep. A caster's shadow reads about as big on screen as the caster
//! does, so the cutoff is `radius × SHADOW_PER_RADIUS` (× 1 → 2 with render distance, like the
//! cascade horizon): a saguaro (r ≈ 3.3) stops at ~230u on Extreme, a house keeps casting to the edge.
//!
//! Only entities that cast to begin with are touched: a capped one carries [`ShadowCapped`], and only
//! those get their `NotShadowCaster` lifted again. Skinned meshes have their own flip
//! (`anim_lod::shadow_flip`); foliage materials are left alone.
//!
//! Procedural creatures (orks, the part-built animals: `CreatureMaterial` rigs of 10-40 parts) get a
//! flat cutoff instead ([`cap_creature_shadows`]): every part is tested every frame against the same
//! distance, so a whole rig drops / regains its shadow together rather than piece by piece. They
//! filled the far cascade with ~300 casters at 60-260u in the village (each part a shadow draw).
//! `FOREST_SHADOWLOD=0` turns it off (A/B).

use bevy::camera::primitives::Aabb;
use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;

/// Cutoff distance per world unit of bounding radius, at `Normal` render distance.
const SHADOW_PER_RADIUS: f32 = 35.0;
/// Never cut a shadow closer than this (base units), however small the caster.
const SHADOW_MIN: f32 = 60.0;
/// Hysteresis: drop the shadow past `cut × (1 + H)`, take it back inside `cut × (1 − H)`.
const HYST: f32 = 0.06;
/// Entities are re-checked in this many interleaved slices (one slice per frame).
const SLICES: u32 = 8;

/// Marks a mesh whose shadow this module switched off (so only those are switched back on).
#[derive(Component)]
pub struct ShadowCapped;

pub struct ShadowLodPlugin;

impl Plugin for ShadowLodPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("FOREST_SHADOWLOD").as_deref() == Ok("0") {
            return;
        }
        app.add_systems(
            PostUpdate,
            (cap_shadows, cap_creature_shadows).after(bevy::transform::TransformSystems::Propagate),
        );
    }
}

#[allow(clippy::type_complexity)]
fn cap_shadows(
    mut commands: Commands,
    mut frame: Local<u32>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    meshes: Query<
        (Entity, &GlobalTransform, &Aabb, Has<NotShadowCaster>, Has<ShadowCapped>),
        (With<Mesh3d>, With<MeshMaterial3d<StandardMaterial>>, Without<SkinnedMesh>),
    >,
) {
    let Some(eye) = cam.iter().next().map(|t| t.translation()) else { return };
    *frame = frame.wrapping_add(1);
    let slice = *frame % SLICES;
    // Track the cascade horizon's stretch (`ViewDistance::shadow_scale`: 1 / 1.4 / 2), not the ×3 view range.
    let k = SHADOW_PER_RADIUS * (1.0 + (crate::quality::view_scale() - 1.0).max(0.0) * 0.5);
    for (e, tf, aabb, off, capped) in &meshes {
        if e.index_u32() % SLICES != slice || (off && !capped) {
            continue;
        }
        let (scale, _, _) = tf.to_scale_rotation_translation();
        let r = Vec3::from(aabb.half_extents).length() * scale.abs().max_element();
        let cut = (r * k).max(SHADOW_MIN);
        let d2 = tf.transform_point(aabb.center.into()).distance_squared(eye);
        if !off && d2 > (cut * (1.0 + HYST)).powi(2) {
            commands.entity(e).try_insert((NotShadowCaster, ShadowCapped));
        } else if capped && d2 < (cut * (1.0 - HYST)).powi(2) {
            commands.entity(e).try_remove::<(NotShadowCaster, ShadowCapped)>();
        }
    }
}

/// Creature rigs stop casting past this (base units; skinned people / animals stop at 38).
const CREATURE_SHADOW_FAR: f32 = 50.0;
const CREATURE_SHADOW_NEAR: f32 = 46.0;

#[allow(clippy::type_complexity)]
fn cap_creature_shadows(
    mut commands: Commands,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    parts: Query<
        (Entity, &GlobalTransform, Has<NotShadowCaster>, Has<ShadowCapped>),
        (With<Mesh3d>, With<MeshMaterial3d<crate::creature::CreatureMaterial>>, Without<crate::player::HeroMesh>),
    >,
) {
    let Some(eye) = cam.iter().next().map(|t| t.translation()) else { return };
    let k = 1.0 + (crate::quality::view_scale() - 1.0).max(0.0) * 0.5;
    let (far2, near2) = ((CREATURE_SHADOW_FAR * k).powi(2), (CREATURE_SHADOW_NEAR * k).powi(2));
    for (e, tf, off, capped) in &parts {
        if off && !capped {
            continue; // never cast to begin with (eyes, smoke, the viewmodel…)
        }
        // Horizontal distance: a rig's parts share it to within a body length, so they flip together.
        let d2 = tf.translation().xz().distance_squared(eye.xz());
        if !off && d2 > far2 {
            commands.entity(e).try_insert((NotShadowCaster, ShadowCapped));
        } else if capped && d2 < near2 {
            commands.entity(e).try_remove::<(NotShadowCaster, ShadowCapped)>();
        }
    }
}
