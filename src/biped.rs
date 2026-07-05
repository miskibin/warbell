//! **Shared biped rig + animator** — lets non-hero bipeds (orcs, peasants/villagers) drive the
//! exact same studio `Pose` clips the hero uses, instead of their own bespoke limb code. The pose
//! math lives in [`crate::player::anim`] (the hero's proven, verified clips); this module just
//! exposes it to other mobs via a small ECS contract:
//!
//! - [`BipedPart`] tags each joint entity of a biped (with a back-reference to its `root`).
//! - [`BipedDrive`] is the per-frame animation input a mob's AI fills (gait phase, attack, …).
//! - [`animate_biped`] reads every drive, builds one [`crate::player::anim::Pose`] per root, and
//!   writes it onto that root's tagged joints.
//!
//! The hero keeps its own richer driver (`player::anim::hero_anim`: gestures, first-person raise,
//! landing squash) and is **not** touched by this module — it only borrows the clip functions.
//! Spawning a biped's skeleton (the joint hierarchy from a per-joint mesh set) is added alongside
//! the first mob that uses it.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::player::anim::{action_over_loco, attack_phase, attack_pose, carry_pose, loco_pose, sit_pose, work_pose, Pose};
use crate::player::{Joint, ATTACK_DURATION};

/// One animated joint of a biped mob. `root` points back at the entity carrying [`BipedDrive`] so a
/// single [`animate_biped`] pass can pose many mobs at once.
#[derive(Component)]
pub struct BipedPart {
    pub joint: Joint,
    pub root: Entity,
}

/// Marks the `rig` wrapper entity [`spawn_biped`] parents the whole joint tree under. A caller that
/// rebuilds a mob's body in place (e.g. a villager changing job in `villagers::reskin_townsfolk`)
/// despawns the child carrying this to drop the *whole* skeleton in one go, then re-spawns it.
#[derive(Component)]
pub struct BipedRig;

/// Per-frame animation inputs a mob's AI writes each tick; [`animate_biped`] turns it into a pose.
/// (Superset grows as more clips are wired — jump/block/sit/woodchop/mine come with later mobs.)
#[derive(Component, Default)]
pub struct BipedDrive {
    /// 0..1 blend toward "moving" (idle ↔ gait).
    pub moving_amt: f32,
    /// 0..1 blend toward "running" (walk ↔ run).
    pub run_amt: f32,
    /// Gait phase, advanced by the AI at its movement speed.
    pub walk_phase: f32,
    /// Mid-swing this frame.
    pub attacking: bool,
    /// Seconds into the current swing (0..`ATTACK_DURATION`).
    pub attack_t: f32,
    /// Which studio attack clip (0 overhead / 1 slash / 2 thrust).
    pub attack_variant: u8,
    /// Seated on a stump (overrides locomotion with the seated pose).
    pub sitting: bool,
    /// Posted-worker tool stroke when standing still: 0 none · 1 hoe (farmer) · 2 chop/pick
    /// (woodcutter/miner). Layered over the idle stance by [`work_pose`].
    pub work: u8,
    /// True while hauling a load home (a woodcutter's log / a miner's cart) — both arms grip forward.
    pub carrying: bool,
    /// Per-instance clock offset (radians/seconds) so neighbouring mobs don't fidget/work in
    /// lockstep. Fed into the work stroke; locomotion already carries its own phase via `walk_phase`.
    pub phase: f32,
}

/// Select + compose the studio clips for one biped from its drive (reuses the hero's clip math).
/// An attack while moving layers over the gait (a running attack), exactly like the hero.
pub(crate) fn biped_pose(d: &BipedDrive, now: f32) -> Pose {
    if d.sitting {
        return sit_pose();
    }
    let moving = d.moving_amt.clamp(0.0, 1.0);
    let loco = loco_pose(now, d.walk_phase, moving, d.run_amt.clamp(0.0, 1.0));
    if d.attacking {
        let (phase, p) = attack_phase((d.attack_t / ATTACK_DURATION).clamp(0.0, 1.0));
        let atk = attack_pose(d.attack_variant, &phase, p);
        if moving > 0.05 {
            action_over_loco(&atk, &loco, moving)
        } else {
            atk
        }
    } else if d.carrying {
        // Hauling a load home: both arms grip forward (carry pose) while the legs keep walking.
        action_over_loco(&carry_pose(), &loco, moving)
    } else if d.work != 0 && moving < 0.05 {
        // A posted worker plies their trade in place; walking back to/from post falls through to loco.
        work_pose(now + d.phase, d.work == 1)
    } else {
        loco
    }
}

/// Pose every biped: one [`Pose`] per root (from its [`BipedDrive`]), written onto its [`BipedPart`]s.
/// Ungated (like the hero animator) so a frozen/paused world still draws its mobs posed.
pub fn animate_biped(
    time: Res<Time>,
    drives: Query<(Entity, &BipedDrive)>,
    mut parts: Query<(&BipedPart, &mut Transform)>,
    // Reused across frames so a full siege+town's worth of bipeds doesn't heap-alloc a fresh map
    // every frame (clear keeps the capacity).
    mut poses: Local<HashMap<Entity, Pose>>,
) {
    let now = time.elapsed_secs();
    poses.clear();
    poses.extend(drives.iter().map(|(e, d)| (e, biped_pose(d, now))));
    for (part, mut tf) in &mut parts {
        if let Some(pose) = poses.get(&part.root) {
            let jp = pose.get(part.joint);
            if let Some(t) = jp.t {
                tf.translation = t;
            }
            tf.rotation = jp.r;
        }
    }
}

/// A per-joint mesh set for a biped — the 14 body joints (hips/torso/neck/head, both shoulders/
/// elbows, both hips/knees/feet) plus an optional held weapon (on the `Sword` pivot), shield, and
/// shield emblem. Each becomes one flat-shaded vertex-coloured mesh, like the hero's.
#[allow(dead_code)]
pub struct BipedMeshes {
    pub hips: Mesh,
    pub torso: Mesh,
    pub neck: Mesh,
    pub head: Mesh,
    pub shoulder_l: Mesh,
    pub shoulder_r: Mesh,
    pub elbow_l: Mesh,
    pub elbow_r: Mesh,
    pub hip_l: Mesh,
    pub hip_r: Mesh,
    pub knee_l: Mesh,
    pub knee_r: Mesh,
    pub foot_l: Mesh,
    pub foot_r: Mesh,
    pub weapon: Option<Mesh>,
    pub shield: Option<Mesh>,
    pub lion: Option<Mesh>,
}

/// Pre-uploaded handles for a [`BipedMeshes`] set — so a cached spawner (e.g. the ork `Armory`)
/// uploads each mesh once and clones cheap handles per mob, instead of re-uploading 14 meshes per
/// spawn (which would bloat memory badly during a siege).
#[derive(Clone)]
pub struct BipedHandles {
    hips: Handle<Mesh>,
    torso: Handle<Mesh>,
    neck: Handle<Mesh>,
    head: Handle<Mesh>,
    shoulder_l: Handle<Mesh>,
    shoulder_r: Handle<Mesh>,
    elbow_l: Handle<Mesh>,
    elbow_r: Handle<Mesh>,
    hip_l: Handle<Mesh>,
    hip_r: Handle<Mesh>,
    knee_l: Handle<Mesh>,
    knee_r: Handle<Mesh>,
    foot_l: Handle<Mesh>,
    foot_r: Handle<Mesh>,
    weapon: Option<Handle<Mesh>>,
    shield: Option<Handle<Mesh>>,
    lion: Option<Handle<Mesh>>,
}

impl BipedHandles {
    /// Swap the off-hand (shield-slot) mesh — e.g. an ork torch-bearer carries a lit war-torch in
    /// the shield hand instead of a buckler. `None` strips the off-hand entirely. The shield
    /// emblem is dropped either way (it only makes sense riding an actual shield).
    pub fn with_shield(mut self, shield: Option<Handle<Mesh>>) -> Self {
        self.shield = shield;
        self.lion = None;
        self
    }
}

impl BipedMeshes {
    /// Upload every mesh once, returning shareable (cloneable) handles for cached spawning.
    pub fn upload(self, meshes: &mut Assets<Mesh>) -> BipedHandles {
        BipedHandles {
            hips: meshes.add(self.hips),
            torso: meshes.add(self.torso),
            neck: meshes.add(self.neck),
            head: meshes.add(self.head),
            shoulder_l: meshes.add(self.shoulder_l),
            shoulder_r: meshes.add(self.shoulder_r),
            elbow_l: meshes.add(self.elbow_l),
            elbow_r: meshes.add(self.elbow_r),
            hip_l: meshes.add(self.hip_l),
            hip_r: meshes.add(self.hip_r),
            knee_l: meshes.add(self.knee_l),
            knee_r: meshes.add(self.knee_r),
            foot_l: meshes.add(self.foot_l),
            foot_r: meshes.add(self.foot_r),
            weapon: self.weapon.map(|m| meshes.add(m)),
            shield: self.shield.map(|m| meshes.add(m)),
            lion: self.lion.map(|m| meshes.add(m)),
        }
    }
}

/// Spawn the studio knight skeleton (the same joint hierarchy + frames the hero uses) for a mob,
/// from pre-uploaded `h` handles. Tags each joint [`BipedPart`] so [`animate_biped`] poses it from
/// the root's [`BipedDrive`]. **Returns `(Head, Option<Shield>)` joint entities** so callers can
/// attach extras (an ork's glowing eyes on the head; a torch-bearer's flame + light on the
/// off-hand). Per-mob tuning: `head_scale`/`foot_scale` (studio group scales), `hip_dx`/
/// `shoulder_dx` (limb spacing), `rig_offset_y` (drop feet onto the ground), and `shield_xf`
/// (Some → mount the shield + emblem on the left hand). The held `weapon` rides the `Sword`
/// pivot. Built under a `rig` child of `root` so the caller's root scale sizes the whole mob.
#[allow(clippy::too_many_arguments)]
pub fn spawn_biped(
    commands: &mut Commands,
    root: Entity,
    mat: &Handle<crate::creature::CreatureMaterial>,
    h: BipedHandles,
    head_scale: f32,
    foot_scale: f32,
    hip_dx: f32,
    shoulder_dx: f32,
    rig_offset_y: f32,
    shield_xf: Option<Transform>,
) -> (Entity, Option<Entity>) {
    let p = |t: Vec3| Transform::from_translation(t);
    let ps = |t: Vec3, s: f32| Transform { translation: t, scale: Vec3::splat(s), ..default() };
    // Spawn a tagged joint (transform-only) under `parent`, with an optional mesh-leaf child.
    let joint = |commands: &mut Commands, parent: Entity, j: Option<Joint>, xf: Transform, leaf: Option<Handle<Mesh>>| -> Entity {
        let mut ec = commands.spawn((xf, Visibility::Visible));
        if let Some(j) = j {
            ec.insert(BipedPart { joint: j, root });
        }
        let e = ec.id();
        commands.entity(parent).add_child(e);
        if let Some(mesh) = leaf {
            let leaf_e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat.clone()), Transform::default())).id();
            commands.entity(e).add_child(leaf_e);
        }
        e
    };
    use Joint::*;

    let rig = commands.spawn((Transform::from_xyz(0.0, rig_offset_y, 0.0), Visibility::Visible, BipedRig)).id();
    commands.entity(root).add_child(rig);

    // Spine.
    let hips = joint(commands, rig, Some(Hips), p(Vec3::new(0.0, 1.05, 0.0)), Some(h.hips));
    let torso = joint(commands, hips, Some(Torso), p(Vec3::new(0.0, 0.15, 0.0)), Some(h.torso));
    let neck = joint(commands, torso, None, p(Vec3::new(0.0, 0.35, 0.0)), Some(h.neck));
    let head_e = joint(commands, neck, Some(Head), ps(Vec3::new(0.0, 0.08, 0.0), head_scale), Some(h.head));

    // Left arm (+ optional shield/emblem on the hand).
    let sh_l = joint(commands, torso, Some(ShoulderL), p(Vec3::new(-shoulder_dx, 0.27, 0.01)), Some(h.shoulder_l));
    let el_l = joint(commands, sh_l, Some(ElbowL), p(Vec3::new(0.0, -0.30, 0.0)), Some(h.elbow_l));
    let hand_l = joint(commands, el_l, None, p(Vec3::new(0.0, -0.25, 0.0)), None);
    let mut shield_out = None;
    if let (Some(sx), Some(shield)) = (shield_xf, h.shield) {
        let shield_e = joint(commands, hand_l, Some(Shield), sx, Some(shield));
        if let Some(lion) = h.lion {
            joint(commands, shield_e, None, p(Vec3::new(0.0, -0.03, 0.033)), Some(lion));
        }
        shield_out = Some(shield_e);
    }

    // Right arm (+ optional weapon on the Sword pivot).
    let sh_r = joint(commands, torso, Some(ShoulderR), p(Vec3::new(shoulder_dx, 0.27, 0.01)), Some(h.shoulder_r));
    let el_r = joint(commands, sh_r, Some(ElbowR), p(Vec3::new(0.0, -0.30, 0.0)), Some(h.elbow_r));
    let hand_r = joint(commands, el_r, None, p(Vec3::new(0.0, -0.25, 0.0)), None);
    if let Some(weapon) = h.weapon {
        joint(commands, hand_r, Some(Sword), Transform::default(), Some(weapon));
    }

    // Legs.
    let hip_l = joint(commands, hips, Some(HipL), p(Vec3::new(-hip_dx, -0.05, 0.0)), Some(h.hip_l));
    let knee_l = joint(commands, hip_l, Some(KneeL), p(Vec3::new(0.0, -0.40, 0.0)), Some(h.knee_l));
    joint(commands, knee_l, Some(FootL), ps(Vec3::new(0.0, -0.45, 0.0), foot_scale), Some(h.foot_l));
    let hip_r = joint(commands, hips, Some(HipR), p(Vec3::new(hip_dx, -0.05, 0.0)), Some(h.hip_r));
    let knee_r = joint(commands, hip_r, Some(KneeR), p(Vec3::new(0.0, -0.40, 0.0)), Some(h.knee_r));
    joint(commands, knee_r, Some(FootR), ps(Vec3::new(0.0, -0.45, 0.0), foot_scale), Some(h.foot_r));

    (head_e, shield_out)
}

pub struct BipedPlugin;

impl Plugin for BipedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, animate_biped);
    }
}
