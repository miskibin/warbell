//! **Training dummies** — straw-stuffed pells planted in the keep courtyard, ported from the
//! old game's `TrainingDummy.tsx` / `MusterYard.tsx`. Indestructible practice targets: they
//! take the hero's swing (sharing the [`crate::verbs::HeroSwing`] cone every other verb reads),
//! pop a damage number and recoil-wobble, but have no HP and grant no reward — pure feedback so
//! a new player can learn the swing's reach + timing in the safe courtyard.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::meshkit::{merged_flat as merged, tinted_hex as tinted};

/// Swing reach the dummy tests against (hero melee cone + the dummy's girth) — matches the
/// ore-mining reach so the two read the same.
const HIT_RANGE: f32 = 1.9;
const HIT_CONE_DOT: f32 = 0.5;
/// Seconds a struck dummy keeps recoil-wobbling.
const WOBBLE_DUR: f32 = 0.5;

#[derive(Component)]
pub struct Dummy {
    /// Elapsed-seconds the current recoil wobble ends (0 = at rest).
    wobble_until: f32,
    /// Direction (world XZ) the last blow came FROM, so the recoil leans away from it.
    knock: Vec2,
}

// ── tiny local mesh helpers (the props.rs / critters.rs contract) ──────────────────
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn cyl(r: f32, h: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().build().translated_by(off), c)
}
fn cone_at(r: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), c)
}

/// The merged straw-pell mesh (feet at y≈0).
fn dummy_mesh() -> Mesh {
    const WOOD: u32 = 0x6b4a2a;
    const DARK: u32 = 0x4a3322;
    const BURLAP: u32 = 0xb39a72;
    const STRAW: u32 = 0xcdae5e;
    merged(vec![
        bx(0.36, 0.12, 0.36, Vec3::new(0.0, 0.06, 0.0), DARK), // foot anchor
        bx(0.09, 0.92, 0.09, Vec3::new(0.0, 0.58, 0.0), WOOD), // post
        bx(0.6, 0.09, 0.09, Vec3::new(0.0, 0.72, 0.0), WOOD),  // cross-arm
        cyl(0.19, 0.44, Vec3::new(0.0, 0.62, 0.0), BURLAP),    // burlap torso
        cone_at(0.22, 0.1, Vec3::new(0.0, 0.42, 0.0), Quat::IDENTITY, STRAW), // waist straw
        cone_at(0.17, 0.1, Vec3::new(0.0, 0.86, 0.0), Quat::from_rotation_x(std::f32::consts::PI), STRAW), // neck straw
        // head: a small burlap block (cheaper + flatter-shaded than a sphere)
        bx(0.22, 0.22, 0.22, Vec3::new(0.0, 0.99, 0.0), BURLAP),
        // straw tufts at the arm ends
        cone_at(0.08, 0.12, Vec3::new(-0.3, 0.72, 0.0), Quat::from_rotation_z(1.2), STRAW),
        cone_at(0.08, 0.12, Vec3::new(0.3, 0.72, 0.0), Quat::from_rotation_z(-1.2), STRAW),
    ])
}

/// Plant the lone practice pell in the courtyard near the keep. Called from `worldmap::build`
/// (combined map only), so the castle's ground height is already available.
pub fn populate(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mesh = meshes.add(dummy_mesh());
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, ..default() });
    // One pell off to the side of the keep's southern courtyard (the muster yard), plus one by
    // the meadow rest-campfire next to the hero spawn (2026-07) — the first thing a new player
    // can whack, three steps from where they wake up. Both clear of the gate lanes.
    const SPOTS: [(f32, f32); 2] = [(-4.5, 6.0), (-19.0, 12.5)];
    for (i, (x, z)) in SPOTS.into_iter().enumerate() {
        let y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
        // A little yaw variety so the row isn't mechanical.
        let yaw = (i as f32 * 1.7) % std::f32::consts::TAU;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_y(yaw)),
            Dummy { wobble_until: 0.0, knock: Vec2::ZERO },
            BiomeEntity,
        ));
        // Solid pell — a small base so you can't walk through it, but the hero still steps in
        // well within HIT_RANGE to swing at it.
        crate::blockers::add(x, z, 0.3);
    }
}

pub struct TrainingDummiesPlugin;

impl Plugin for TrainingDummiesPlugin {
    fn build(&self, app: &mut App) {
        // Wobble anim keeps running while frozen; hit detection is sim-gated.
        app.add_systems(Update, dummy_wobble).add_systems(
            Update,
            dummy_hits.run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

/// Each published hero swing: any dummy in the cone takes a (no-op) hit — pop a damage number,
/// record the knock direction + arm the recoil wobble. No HP, no death, no reward.
fn dummy_hits(
    time: Res<Time>,
    mut swings: MessageReader<crate::verbs::HeroSwing>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut q: Query<(&mut Dummy, &Transform)>,
) {
    let now = time.elapsed_secs();
    for sw in swings.read() {
        for (mut d, tf) in &mut q {
            let p = tf.translation;
            let to = Vec2::new(p.x - sw.origin.x, p.z - sw.origin.y);
            let dist = to.length();
            if dist > HIT_RANGE || dist < 1e-3 || (to / dist).dot(sw.fwd) < HIT_CONE_DOT {
                continue;
            }
            d.wobble_until = now + WOBBLE_DUR;
            d.knock = (to / dist).max(Vec2::splat(-1.0)).min(Vec2::splat(1.0));
            floats.0.push(crate::combat_fx::FloatReq {
                world: Vec3::new(p.x, p.y + 1.4, p.z),
                text: format!("{}", sw.base_dmg.round() as i64),
                color: crate::combat_fx::col_ork_hit(),
                scale: 1.0,
            });
        }
    }
}

/// Decay the recoil: tilt the pell away from the blow with a damped wobble, easing back upright.
fn dummy_wobble(time: Res<Time>, mut q: Query<(&Dummy, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (d, mut tf) in &mut q {
        // Base upright yaw is baked into the spawned rotation; we only add a transient tilt, so
        // re-derive yaw and rebuild the rotation each frame (no accumulation).
        let (yaw, _, _) = tf.rotation.to_euler(EulerRot::YXZ);
        let remain = d.wobble_until - now;
        let tilt = if remain > 0.0 {
            let k = remain / WOBBLE_DUR; // 1 → 0
            (now * 26.0).sin() * 0.22 * k * k
        } else {
            0.0
        };
        // Lean about the axis perpendicular to the knock direction (so it rocks away from the hit).
        tf.rotation = Quat::from_rotation_y(yaw)
            * Quat::from_rotation_x(tilt * d.knock.y)
            * Quat::from_rotation_z(-tilt * d.knock.x);
    }
}
