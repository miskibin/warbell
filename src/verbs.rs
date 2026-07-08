//! **Biome verbs** — acting on the world to feed the bag + stone bank. This module owns the
//! [`HeroSwing`] broadcast (combat publishes one cone per swing-hit; mining reads it) and the
//! ore-mining loop. Foraging and hunt drops land alongside it (chests moved to `chest.rs`).
//!
//! Placement is forest-coord native: [`populate_ore`] (called from `worldmap::build`)
//! reject-samples the rock biome and constructs the test-gated `ore_store::Ore` directly,
//! bypassing core's `OreField::create` (which would snap to core's *own* enlarged tilemap).

use bevy::prelude::*;
use tileworld_core::ore_store::{Ore, ORE_COLLISION_RADIUS, ORE_STONE};
use tileworld_core::forage_store;

use crate::audio::AudioCue;
use crate::combat_fx::FloatReq;
use crate::critters::Species;
use crate::economy::Bank;
use crate::inventory::{try_grant, Inventory, Toasts};
use crate::palette::lin;
use crate::player::HeroState;
use crate::worldmap;
use crate::game_state::SimAppExt;

/// Forest ore HP — rescaled from core's TS-anchored 500 into forest's combat units, then bumped so
/// a boulder is a REAL dig: at the flat [`HERO_HARVEST_DMG`] (30) it's a fixed ~20 swings, all run,
/// no matter the hero's level/gear — quarrying stays a commitment, not a one-combo pop. Ore takes
/// `harvest_dmg` (decoupled from combat), so leveling/crit/lifesteal never shortcut a dig. The
/// per-hit erosion (`drive_ore_wear`) makes the long dig read as visible progress, not grind.
const ORE_HP: f64 = 600.0;
/// Front-cone reach the swing checks ore against (the hero melee cone + the boulder radius).
const SWING_RANGE: f32 = 1.9;
const SWING_CONE_DOT: f32 = 0.5;
/// How many boulders to seed across the rock biome. Plentiful so the town's stone miner
/// (`miner.rs`) has boulders to work without starving the hero's own mining. Reject-sampling
/// places as many as the rock region fits (a shortfall just logs — see `populate_ore`).
const ORE_COUNT: u32 = 64;
/// Seconds before a depleted boulder regrows in place. Stone is still slower than trees (cf. 450s
/// `TREE_REGROW`) so it stays a destination worth ranging out for, not a topped-up vending machine —
/// just not the ~18min it was; eased back down toward the old 360s baseline (was tripled to 1080s).
const ORE_REGROW: f32 = 540.0;

/// Published by combat at each swing's hit-phase: the cone the blow sweeps. Mining (and later
/// the training dummies) test their targets against it, sharing the player's one swing.
#[derive(Message)]
pub struct HeroSwing {
    /// Hero world XZ at the moment of the blow.
    pub origin: Vec2,
    /// Facing unit vector `(sin, cos)`.
    pub fwd: Vec2,
    /// Non-crit COMBAT damage (training dummies show this; it carries levels/weapon/crit-base).
    pub base_dmg: f32,
    /// HARVEST damage — what trees/ore actually take. Deliberately decoupled from `base_dmg`: it is
    /// the flat [`HERO_HARVEST_DMG`], so a late-game hero (200+ combat dmg) can't one-shot the forest
    /// and trivialise the RTS economy. Chopping/mining stays a fixed-cost commitment all run.
    pub harvest_dmg: f32,
}

/// Flat per-swing damage the hero deals to trees and ore — NOT his combat `attack_damage`. Sized so
/// a tree (165 HP) falls in ~6 swings and a boulder (600 HP) in ~20, forever, regardless of level or
/// gear. (The woodcutter/miner NPCs keep their own `CHOP_DMG`/`PICK_DMG` — this is the hero only.)
pub const HERO_HARVEST_DMG: f32 = 30.0;

/// A mineable boulder — wraps the pure `ore_store::Ore` (HP + shatter logic).
#[derive(Component)]
pub struct OreNode {
    pub(crate) ore: Ore,
    /// Footprint blocker radius registered at spawn, so a regrown node can re-block (mirror of
    /// `ChopTree::trunk_r`). Also the miner's swing-reach anchor (`miner::pick_work`).
    pub(crate) blocker_r: f32,
    /// Rest scale at full HP — `drive_ore_wear` erodes the rock down from this as the boulder is
    /// mined, and `regrow_ore` springs it back to this. Stored so the wear curve and regrow pop
    /// both anchor on the true rest size, not whatever shrunken scale the node was last left at.
    pub(crate) base_scale: f32,
}

/// Per-boulder wear state: the settled (no-punch) scale eased toward the HP-driven target each
/// frame by [`drive_ore_wear`]. The shrinking rock is the "every hit takes a real bite — keep
/// digging" feedback; the glowing gem core stays the prize as the grey stone wears off around it.
#[derive(Component)]
struct OreWear {
    /// Settled scale (sans the per-hit bite), exponentially chased toward the worn target.
    cur: f32,
}

/// A depleted boulder waiting to regrow: hidden in place (blocker lifted), restored to full HP by
/// [`regrow_ore`] — so mining (hero OR town miner) doesn't permanently strip the map. Mirror of
/// the tree [`Stump`].
#[derive(Component)]
pub struct DepletedOre {
    regrow_at: f32,
}

/// Knock boulder `e` out WITHOUT banking anything: hide it, lift its blocker, and schedule the
/// regrow. The hero's [`mine_ore`] calls this on shatter (then banks the stone itself); the town
/// miner (`miner.rs`) calls it on the depleting blow (then carts the stone home). Shared so the
/// world-state bookkeeping is identical regardless of who lands the last blow.
pub(crate) fn deplete_ore(commands: &mut Commands, e: Entity, pos: Vec3, now: f32) {
    crate::blockers::remove_at(pos.x, pos.z); // clear the boulder blocker — no ghost collision
    commands
        .entity(e)
        .try_insert((Visibility::Hidden, DepletedOre { regrow_at: now + ORE_REGROW }));
}

pub struct VerbsPlugin;

impl Plugin for VerbsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<HeroSwing>()
            .add_message::<AnimalKilled>()
            .init_resource::<VerbRng>()
            .add_systems(Startup, setup_drop_assets)
            .add_sim_systems(
                (
                    mine_ore,
                    regrow_ore,
                    ore_glow_pulse,
                    chop_tree,
                    regrow_trees,
                    forage_pickup,
                    forage_respawn,
                    apple_harvest,
                    apple_tree_shake,
                    apple_regrow,
                    animal_drops,
                    ground_pickup,
                )
                    ,
            )
            // Impact-juice drivers — ungated (like `dying.rs` / `combat_fx`: a mid-fall tree
            // keeps toppling behind a panel; virtual time still freezes them under a hit-stop)
            // and AFTER the wind sway, which rewrites every swaying tree's rotation each frame:
            // these layer the chop shudder / felling topple on top of that write.
            .add_systems(
                Update,
                (drive_trunk_shake, drive_felling, drive_regrow_pop, drive_ore_wear)
                    .after(crate::wind::sway_system),
            );
    }
}

/// Deterministic mulberry32 for drop rolls + scatter jitter ("feels-the-same", no parity need).
#[derive(Resource)]
struct VerbRng(u32);
impl Default for VerbRng {
    fn default() -> Self {
        VerbRng(0x51ed_270b)
    }
}
impl VerbRng {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }
}

/// Read each published swing; any live boulder inside the cone takes the blow. On shatter the
/// node banks its stone (HUD counter) + pops a float, and the boulder despawns.
fn mine_ore(
    time: Res<Time>,
    fx: Option<Res<crate::player::CombatFx>>,
    mut swings: MessageReader<HeroSwing>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut q: Query<(Entity, &mut OreNode, &Transform, Option<&mut TrunkShake>)>,
) {
    let now = time.elapsed_secs() as f64;
    for sw in swings.read() {
        for (e, mut node, tf, mut shake) in &mut q {
            if node.ore.hp <= 0.0 {
                continue;
            }
            let p = tf.translation;
            let to = Vec2::new(p.x - sw.origin.x, p.z - sw.origin.y);
            let dist = to.length();
            if dist > SWING_RANGE + ORE_COLLISION_RADIUS as f32 || dist < 1e-3 {
                continue;
            }
            let dir = to / dist;
            if dir.dot(sw.fwd) < SWING_CONE_DOT {
                continue;
            }
            let shattered = node.ore.damage(sw.harvest_dmg as f64, now);
            let head = Vec3::new(p.x, p.y + 1.0, p.z);
            let chip_at = Vec3::new(p.x, p.y + 0.6, p.z);
            if shattered {
                bank.0.add_stone(node.ore.stone_reward);
                floats.0.push(crate::combat_fx::FloatReq {
                    world: head,
                    text: format!("+{} stone", node.ore.stone_reward as i64),
                    color: Color::srgb(0.82, 0.82, 0.88),
                    scale: 1.1,
                });
                if let Some(fx) = &fx {
                    crate::player::spawn_chips(&mut commands, fx, chip_at, true);
                }
                cues.write(AudioCue::OreChip); // metallic crack on the breaking blow…
                cues.write(AudioCue::OreShatter); // …layered under the synth shatter sting
                speak.write(crate::audio::Speak::new(crate::audio::Concept::FirstStone));
                // Don't despawn — deplete + schedule a slow regrow (shared with the town miner),
                // so the boulder comes back instead of stripping the map.
                deplete_ore(&mut commands, e, p, now as f32);
            } else {
                // Metallic chip + a small grey rock-chip spray each pick-swing (was a flesh hit),
                // and the boulder itself judders under the pick (its rest yaw is restored after).
                if let Some(fx) = &fx {
                    crate::player::spawn_chips(&mut commands, fx, chip_at, false);
                }
                match shake.as_deref_mut() {
                    Some(s) => s.restart(now as f32, dir),
                    None => {
                        commands.entity(e).try_insert(TrunkShake::new(now as f32, dir));
                    }
                }
                cues.write(AudioCue::OreChip);
            }
        }
    }
}

/// Restore each due depleted boulder to a full, mineable node (visible again, blocker re-added),
/// springing it up with a [`RegrowPop`] instead of blinking it in. Mirror of [`regrow_trees`].
fn regrow_ore(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut OreNode, &mut OreWear, &DepletedOre, &Transform, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    for (e, mut node, mut wear, dep, tf, mut vis) in &mut q {
        if now < dep.regrow_at {
            continue;
        }
        node.ore.hp = node.ore.max_hp;
        // Reset the eroded scale to full so the freshly-regrown boulder doesn't lurch from its old
        // worn size up to rest the instant the pop ends; the pop springs it in from the rest scale.
        wear.cur = node.base_scale;
        *vis = Visibility::Visible;
        crate::blockers::add(tf.translation.x, tf.translation.z, node.blocker_r);
        commands
            .entity(e)
            .try_insert(RegrowPop { started: now, base: Vec3::splat(node.base_scale) })
            .try_remove::<DepletedOre>();
    }
}

// ── Tree chopping (1 tree = TREE_WOOD wood) — the wood mirror of ore mining ──────────

/// A choppable tree: an individual entity. Trees stay individual (decorative passive scatter
/// — bushes/rocks/ground cover — is batched into per-chunk merged meshes, but trees carry
/// per-tree chop HP + wind sway so they can't be baked in). Fell it for wood.
#[derive(Component)]
pub struct ChopTree {
    hp: f64,
    /// The trunk-blocker radius registered at scatter time, so a regrown tree can re-block.
    trunk_r: f32,
}

impl ChopTree {
    /// A fresh choppable tree at full HP — added to every scattered tree in `biome::scatter_region`,
    /// which passes the same trunk radius it registered as the blocker.
    pub fn new(trunk_r: f32) -> Self {
        Self { hp: TREE_HP, trunk_r }
    }

    /// True once the tree has been chopped to 0 HP — used by cutters to bail before double-felling a
    /// tree another actor already dropped this frame (the `Stump` insert that removes it is deferred).
    pub fn felled(&self) -> bool {
        self.hp <= 0.0
    }

    /// Trunk-blocker radius — the woodcutter (`lumberjack.rs`) plants its swing just outside this
    /// so it stands at the bark, not an arm's length off it.
    pub fn trunk_r(&self) -> f32 {
        self.trunk_r
    }

    /// Take a work swing (a woodcutter's axe — `lumberjack.rs`). True once the tree is felled.
    pub fn work_chop(&mut self, dmg: f64) -> bool {
        self.hp -= dmg;
        self.hp <= 0.0
    }
}

/// Marks a [`ChopTree`] that's actually a desert saguaro, not a woody tree. Tagged at scatter
/// time for every tree-class instance in a `Biome::Desert` patch (`biome::scatter_region`). The
/// only gameplay effect is the felling sound: a cactus has no heavy timber crash, so it gets the
/// dry wood-crack clip instead of the full tree-fall (`audio::AudioCue::TreeFall { cactus }`).
#[derive(Component)]
pub struct Cactus;

/// A felled tree waiting to regrow: hidden in place (the trunk blocker lifted), restored to a
/// full [`ChopTree`] by [`regrow_trees`] — so the woodcutters can't permanently deforest the
/// safe zone, and the player's own clear-cuts heal over too.
#[derive(Component)]
pub struct Stump {
    regrow_at: f32,
}

// ── Chop/mine impact juice — trunk shudder, felling topple, regrow pop ──────────────
//
// Transform-only animations on the existing prop entities (the `dying.rs` philosophy): no new
// meshes/materials per event, just damped rotation/scale writes layered after the wind sway.

/// A standing tree (or ore boulder) shuddering from an axe/pick blow: a damped rock along the
/// blow direction. For swaying trees the wobble composes on top of the rotation `wind::sway_system`
/// rewrites each frame; static props (ore) capture + restore their rest rotation instead.
#[derive(Component)]
pub struct TrunkShake {
    started: f32,
    /// Unit world-XZ direction of the blow — the trunk rocks away from the chopper first.
    dir: Vec2,
    /// Rest rotation for NON-swaying props, captured on the first drive frame and restored when
    /// the shake rings out. Swaying trees ignore it (the sway rewrite is the rest pose).
    base_rot: Option<Quat>,
}

impl TrunkShake {
    pub fn new(now: f32, dir: Vec2) -> Self {
        Self { started: now, dir, base_rot: None }
    }
    /// Re-kick an in-flight shake (rapid swings) without forgetting the stored rest rotation.
    pub fn restart(&mut self, now: f32, dir: Vec2) {
        self.started = now;
        self.dir = dir;
    }
}

/// How long a chop shudder rings (s) — under the hero's 0.45s swing so each blow reads alone.
const TRUNK_SHAKE_DUR: f32 = 0.42;
/// Peak shudder tilt (radians) — small at the trunk, clearly visible at the canopy.
const TRUNK_SHAKE_AMP: f32 = 0.06;

/// World-space tilt axis that tips a prop's top toward `dir` for a positive angle.
fn tip_axis(dir: Vec2) -> Vec3 {
    Vec3::new(dir.y, 0.0, -dir.x)
}

/// Drive each shudder: a damped sine rock about the blow-perpendicular axis, then restore.
#[allow(clippy::type_complexity)]
fn drive_trunk_shake(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<
        (Entity, &mut TrunkShake, &mut Transform, Has<crate::wind::Sway>),
        Without<Felling>,
    >,
) {
    let now = time.elapsed_secs();
    for (e, mut shake, mut tf, swaying) in &mut q {
        let t = now - shake.started;
        if t >= TRUNK_SHAKE_DUR {
            if !swaying {
                if let Some(base) = shake.base_rot {
                    tf.rotation = base;
                }
            }
            commands.entity(e).try_remove::<TrunkShake>();
            continue;
        }
        let k = 1.0 - t / TRUNK_SHAKE_DUR;
        let rock = Quat::from_axis_angle(tip_axis(shake.dir), (t * 26.0).sin() * TRUNK_SHAKE_AMP * k * k);
        if swaying {
            tf.rotation = rock * tf.rotation; // sway rewrote the rest pose this frame
        } else {
            let base = *shake.base_rot.get_or_insert(tf.rotation);
            tf.rotation = rock * base;
        }
    }
}

/// A felled tree mid-topple: it tips over along the felling blow with the accelerating arc of a
/// real fall, then the landing puffs leaves + a ground ring (+ a thud in earshot) and the trunk
/// hides — the invisible [`Stump`] (already inserted by [`fell_tree`]) regrows it later.
#[derive(Component)]
pub struct Felling {
    started: f32,
    /// Unit world-XZ the tree falls along (away from the felling blow).
    dir: Vec2,
    /// Standing rotation, captured on the first drive frame (after the sway writes) so the
    /// topple composes from the true rest pose.
    base_rot: Option<Quat>,
    /// Latches once the falling-tree SFX has fired — it plays mid-fall (a falling sound, not a
    /// landing thud), so it must trigger before the landing frame and only once.
    sfx_done: bool,
}

/// Seconds from the felling blow to the trunk hitting the ground.
const FELL_DUR: f32 = 1.05;
/// Total topple angle (~86° — flat enough to read as DOWN before the hide).
const FELL_ANGLE: f32 = 1.5;
/// The landing thud / leaf-burst point sits about this far out along the fallen trunk.
const FELL_CROWN_REACH: f32 = 1.6;
/// Lead the falling-tree SFX this far ahead of the landing frame: the clip is a *falling* sound
/// (whoosh into a crash), so it has to start while the trunk is still toppling, not on impact.
const FELL_SFX_LEAD: f32 = 0.5;

#[allow(clippy::too_many_arguments)]
fn drive_felling(
    time: Res<Time>,
    mut commands: Commands,
    hero: Res<HeroState>,
    fxa: Option<Res<TreeFx>>,
    fx: Option<Res<crate::player::CombatFx>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(Entity, &mut Felling, &mut Transform, &mut Visibility, Has<Cactus>)>,
) {
    let now = time.elapsed_secs();
    for (e, mut fell, mut tf, mut vis, cactus) in &mut q {
        let t = now - fell.started;
        if t >= FELL_DUR {
            // Landing: hide the trunk (regrow restores it) + sell the impact where the crown hit.
            *vis = Visibility::Hidden;
            let at = tf.translation + Vec3::new(fell.dir.x, 0.0, fell.dir.y) * FELL_CROWN_REACH;
            let gy = worldmap::ground_at_world(at.x, at.z).unwrap_or(tf.translation.y);
            if let Some(fxa) = &fxa {
                crate::player::spawn_motes(
                    &mut commands, &fxa.chip_mesh, &fxa.leaf_mat,
                    Vec3::new(at.x, gy + 0.3, at.z), 9, 2.6, 1.1, 0.7,
                );
            }
            if let Some(fx) = &fx {
                crate::player::spawn_shockwave(&mut commands, fx, &mut materials, Vec3::new(at.x, gy, at.z), now);
            }
            commands.entity(e).try_remove::<Felling>();
            continue;
        }
        // Mid-fall: kick the falling-tree SFX `FELL_SFX_LEAD` before the landing frame so the
        // whoosh runs over the topple and the crash lands as the trunk hits. Fires once; a full
        // crack+crash for woody trees, just the dry crack for a cactus. Earshot-gated.
        if !fell.sfx_done && t >= FELL_DUR - FELL_SFX_LEAD {
            fell.sfx_done = true;
            if hero.pos.distance(Vec2::new(tf.translation.x, tf.translation.z)) < 18.0 {
                cues.write(AudioCue::TreeFall { cactus });
            }
        }
        let k = t / FELL_DUR;
        let base = *fell.base_rot.get_or_insert(tf.rotation);
        // k² — a real fall starts slow and accelerates into the ground.
        tf.rotation = Quat::from_axis_angle(tip_axis(fell.dir), k * k * FELL_ANGLE) * base;
    }
}

/// A regrown tree springing back up: scale pops 0 → rest with a slight overshoot, so a returning
/// tree *grows in* instead of blinking into place. Inserted by [`regrow_trees`].
#[derive(Component)]
struct RegrowPop {
    started: f32,
    /// The tree's rest scale (scatter bakes a per-instance scale).
    base: Vec3,
}

/// How long the regrow pop takes (s).
const REGROW_POP_DUR: f32 = 0.55;

/// Ease-out-back: starts at 0, overshoots ~10% past 1, settles at 1.
fn ease_out_back(k: f32) -> f32 {
    let k1 = k - 1.0;
    1.0 + 2.701_58 * k1 * k1 * k1 + 1.701_58 * k1 * k1
}

fn drive_regrow_pop(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &RegrowPop, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (e, pop, mut tf) in &mut q {
        let k = (now - pop.started) / REGROW_POP_DUR;
        if k >= 1.0 {
            tf.scale = pop.base;
            commands.entity(e).try_remove::<RegrowPop>();
            continue;
        }
        tf.scale = pop.base * ease_out_back(k).max(0.02);
    }
}

/// Shared chop-burst visuals (built in [`setup_drop_assets`]): one tiny cuboid flown as wood
/// chips (pale split-wood) or leaf flutter (canopy green) via the spark physics.
#[derive(Resource)]
pub(crate) struct TreeFx {
    chip_mesh: Handle<Mesh>,
    wood_mat: Handle<StandardMaterial>,
    leaf_mat: Handle<StandardMaterial>,
}

/// Wood chips off the trunk + a small leaf flutter from the canopy on a landed axe blow —
/// shared by the hero's [`chop_tree`] and the woodcutter NPC (`lumberjack.rs`). `dir` = blow
/// direction; the chips spray from the chopper-facing side of the trunk.
pub(crate) fn chop_burst(commands: &mut Commands, fxa: &TreeFx, tree_pos: Vec3, dir: Vec2) {
    let impact = tree_pos + Vec3::new(-dir.x * 0.35, 1.0, -dir.y * 0.35);
    crate::player::spawn_motes(commands, &fxa.chip_mesh, &fxa.wood_mat, impact, 5, 2.4, 0.9, 0.45);
    let canopy = tree_pos + Vec3::Y * 2.1;
    crate::player::spawn_motes(commands, &fxa.chip_mesh, &fxa.leaf_mat, canopy, 3, 1.5, 1.1, 0.7);
}

/// Swings to fell a tree — a real commitment: ~6 hits at the hero's flat [`HERO_HARVEST_DMG`] (30),
/// fixed for the whole run so a maxed hero can't one-shot the stand. The woodcutter NPC's per-swing
/// damage is scaled to match, so town wood income keeps its old pace (see `lumberjack::CHOP_DMG`).
const TREE_HP: f64 = 165.0;
/// Wood banked per felled tree. This is the ONLY wood source — the Woodcutter plot has no
/// passive trickle (core `BuildKind::produces` → `None`) — so a tree is worth a real haul.
pub(crate) const TREE_WOOD: f64 = 2.0;
/// Seconds before a felled tree grows back in place. 3× the old 150s — a cleared stand stays
/// cleared long enough to feel earned, and wood is worth ranging out for.
const TREE_REGROW: f32 = 450.0;

/// Fell `e`: bank the wood (+ float), then [`topple_tree`]. The hero's [`chop_tree`] path —
/// he pockets the wood on the spot. The woodcutter NPC instead calls [`topple_tree`] directly
/// and hauls the log home before any wood is banked (`lumberjack.rs`).
pub fn fell_tree(
    commands: &mut Commands,
    e: Entity,
    at: Vec3,
    dir: Vec2,
    now: f32,
    bank: &mut tileworld_core::resource_store::ResourceState,
    floats: &mut crate::combat_fx::FloatQueue,
) {
    bank.add_wood(TREE_WOOD);
    floats.0.push(crate::combat_fx::FloatReq {
        world: Vec3::new(at.x, at.y + 1.6, at.z),
        text: format!("+{} wood", TREE_WOOD as i64),
        color: Color::srgb(0.78, 0.62, 0.36),
        scale: 1.1,
    });
    topple_tree(commands, e, at, dir, now);
}

/// Knock tree `e` over WITHOUT banking anything: lift the trunk blocker and start the
/// [`Felling`] topple along `dir` (the blow direction) — the tree falls over for real, then
/// [`drive_felling`] hides it on landing and the [`Stump`] regrows it later. The `Stump` goes
/// on NOW, so a falling tree is already un-choppable / un-assignable (every chop site filters
/// `Without<Stump>`).
pub fn topple_tree(commands: &mut Commands, e: Entity, at: Vec3, dir: Vec2, now: f32) {
    crate::blockers::remove_at(at.x, at.z); // clear the trunk blocker so no ghost nub
    let dir = if dir.length_squared() > 1e-6 { dir.normalize() } else { Vec2::Y };
    commands.entity(e).try_insert((
        Stump { regrow_at: now + TREE_REGROW },
        Felling { started: now, dir, base_rot: None, sfx_done: false },
    ));
}

/// Restore each due stump to a standing, choppable tree (visible again, trunk re-blocked),
/// springing it up from the ground with a [`RegrowPop`] instead of blinking it in.
fn regrow_trees(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut ChopTree, &Stump, &Transform, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    for (e, mut tree, stump, tf, mut vis) in &mut q {
        if now < stump.regrow_at {
            continue;
        }
        tree.hp = TREE_HP;
        *vis = Visibility::Visible;
        crate::blockers::add(tf.translation.x, tf.translation.z, tree.trunk_r);
        commands
            .entity(e)
            .try_insert(RegrowPop { started: now, base: tf.scale })
            .try_remove::<Stump>();
    }
}

/// Read each published swing; any live choppable tree in the cone takes the blow. On felling
/// it banks [`TREE_WOOD`] wood, pops a float, and despawns. Mirrors [`mine_ore`].
fn chop_tree(
    time: Res<Time>,
    mut swings: MessageReader<HeroSwing>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    fxa: Option<Res<TreeFx>>,
    mut q: Query<(Entity, &mut ChopTree, &Transform, Option<&mut TrunkShake>), Without<Stump>>,
) {
    let now = time.elapsed_secs();
    for sw in swings.read() {
        let mut struck = false; // one chop thunk per swing, even if several trees are in the cone
        for (e, mut tree, tf, mut shake) in &mut q {
            if tree.hp <= 0.0 {
                continue;
            }
            let p = tf.translation;
            let to = Vec2::new(p.x - sw.origin.x, p.z - sw.origin.y);
            let dist = to.length();
            // Measure to the trunk SURFACE, not a fat fixed pad — a thin sapling/cactus shouldn't
            // be hittable from an arm's length of empty air around it. Floored so tiny saplings
            // still have a sane sliver of reach.
            if dist > SWING_RANGE + tree.trunk_r().max(0.25) || dist < 1e-3 {
                continue;
            }
            let dir = to / dist;
            if dir.dot(sw.fwd) < SWING_CONE_DOT {
                continue;
            }
            struck = true;
            tree.hp -= sw.harvest_dmg as f64;
            if tree.hp <= 0.0 {
                fell_tree(&mut commands, e, p, dir, now, &mut bank.0, &mut floats);
            } else {
                // A surviving trunk shudders under the blow + sheds chips and a few leaves.
                match shake.as_deref_mut() {
                    Some(s) => s.restart(now, dir),
                    None => {
                        commands.entity(e).try_insert(TrunkShake::new(now, dir));
                    }
                }
                if let Some(fxa) = &fxa {
                    chop_burst(&mut commands, fxa, p, dir);
                }
            }
        }
        if struck {
            cues.write(AudioCue::WoodChop);
        }
    }
}

/// Seed `ORE_COUNT` boulders across the rock biome (called from `worldmap::build`). Each is a
/// lumpy grey rock; positions reject-sample valid rock-biome ground clear of camps + castle.
pub fn populate_ore(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // A flattened faceted rock crowned by a glowing gem cluster — reads as a deliberate mineable
    // node, not a stray pebble. Rock is plain grey; the crystal core glows (one gem hue per ore
    // variant) so the eye is drawn to it.
    let rock_mesh = meshes.add(ore_rock_mesh());
    let crystal_mesh = meshes.add(ore_crystal_mesh());
    let rock_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE, // grey shades live in the rock mesh's vertex colours
        perceptual_roughness: 0.95,
        metallic: 0.1,
        ..default()
    });
    // One gem hue per ore variant: the crystal material (base + emissive for the bloom glow) and a
    // matching point-light colour, so each boulder casts a soft coloured glow on its rock + ground.
    let gem: [(Color, LinearRgba); 4] = [
        (Color::srgb(0.45, 0.92, 1.0), LinearRgba::rgb(0.20, 1.5, 2.1)),  // teal
        (Color::srgb(0.78, 0.52, 1.0), LinearRgba::rgb(1.0, 0.40, 1.8)),  // amethyst
        (Color::srgb(1.0, 0.80, 0.42), LinearRgba::rgb(1.8, 1.0, 0.22)),  // amber
        (Color::srgb(0.52, 1.0, 0.60), LinearRgba::rgb(0.22, 1.7, 0.50)), // emerald
    ];
    let crystal_mats: Vec<Handle<StandardMaterial>> = gem
        .iter()
        .map(|&(base_color, emissive)| {
            materials.add(StandardMaterial { base_color, emissive, perceptual_roughness: 0.2, ..default() })
        })
        .collect();

    let mut rng: u32 = 0x0e6e_5eed;
    let mut placed = 0u32;
    let mut attempts = 0u32;
    while placed < ORE_COUNT && attempts < ORE_COUNT * 400 + 800 {
        attempts += 1;
        let x = crate::wildlife::rng_range(&mut rng, -worldmap::GX + 5.0, worldmap::GX - 5.0);
        let z = crate::wildlife::rng_range(&mut rng, -worldmap::GZ + 5.0, worldmap::GZ - 5.0);
        if worldmap::biome_at_world(x, z) != Some(crate::biome::Biome::Rocky)
            || worldmap::ground_at_world(x, z).is_none()
            || crate::blockers::is_blocked(x, z)
            || crate::camps::in_clearing(x, z)
            || crate::castle::in_footprint(x, z)
            || crate::bridges::near_bridge(x, z, 1.0)
        {
            continue;
        }
        let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
        let seed = crate::wildlife::rng_range(&mut rng, 0.0, 1.0);
        let ore = Ore {
            id: placed as i64,
            x: x as f64,
            y: y as f64,
            z: z as f64,
            hp: ORE_HP,
            max_hp: ORE_HP,
            hurt_flash_until: 0.0,
            seed: seed as f64,
            collision_radius: ORE_COLLISION_RADIUS,
            variant: ((seed * 4.0).floor() as i32).rem_euclid(4),
            stone_reward: ORE_STONE,
        };
        // Sink the rock slightly so it reads as embedded in the ground; a random yaw varies it.
        // Scale floor kept high — an ore node should never shrink into "just another pebble".
        let scale = crate::wildlife::rng_range(&mut rng, 1.0, 1.4);
        let yaw = crate::wildlife::rng_range(&mut rng, 0.0, std::f32::consts::TAU);
        let vi = (ore.variant as usize) % crystal_mats.len();
        let crystal_mat = crystal_mats[vi].clone();
        let glow = gem[vi].0;
        // Block the boulder's footprint so the hero (and every mover) bumps it instead of walking
        // through it — scaled with the rock, kept ≤1.0 for the neighbour-only blocker scan. Cleared
        // in `mine_ore` on shatter so no invisible nub lingers where the boulder stood.
        let blocker_r = (0.55 * scale).min(0.95);
        crate::blockers::add(x, z, blocker_r);
        commands
            .spawn((
                Transform::from_xyz(x, y + 0.10 * scale, z)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(scale)),
                Visibility::Visible,
                OreNode { ore, blocker_r, base_scale: scale },
                OreWear { cur: scale },
                crate::biome::BiomeEntity,
            ))
            .with_children(|p| {
                p.spawn((Mesh3d(rock_mesh.clone()), MeshMaterial3d(rock_mat.clone()), Transform::default()));
                p.spawn((Mesh3d(crystal_mesh.clone()), MeshMaterial3d(crystal_mat), Transform::default()));
                // Strong coloured glow from the gem core, pulsing slowly (`ore_glow_pulse`) so
                // the node breathes like a live thing — no shadows (cheap; ~18 on the map).
                p.spawn((
                    PointLight {
                        color: glow,
                        intensity: ORE_GLOW_BASE,
                        range: 7.0,
                        radius: 0.2,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.9, 0.0),
                    OreGlow { phase: seed * std::f32::consts::TAU },
                ));
            });
        placed += 1;
    }
    if placed < ORE_COUNT {
        info!("ore: placed {placed}/{ORE_COUNT} boulders");
    }
}

/// Resting intensity of an ore node's gem light (the pulse swings around this).
const ORE_GLOW_BASE: f32 = 28_000.0;

/// The pulsing gem light of an ore node (a child of the [`OreNode`] entity); `phase` staggers
/// the nodes so the whole field doesn't throb in lockstep.
#[derive(Component)]
struct OreGlow {
    phase: f32,
}

/// Slow breathing pulse on every ore node's gem light — the "this is special, come mine me"
/// beacon. Cheap: only mutates `PointLight::intensity` on ~18 lights.
fn ore_glow_pulse(time: Res<Time>, mut q: Query<(&mut PointLight, &OreGlow)>) {
    let t = time.elapsed_secs();
    for (mut light, glow) in &mut q {
        light.intensity = ORE_GLOW_BASE * (1.0 + 0.35 * (t * 2.1 + glow.phase).sin());
    }
}

/// How small a boulder erodes by the depleting blow, as a fraction of its rest scale. The visible
/// "I'm eating this rock" payoff — but it stops well above pebble size so the glowing gem core
/// stays a clear, worth-hitting target right up to the shatter.
const ORE_WEAR_MIN: f32 = 0.5;
/// Per-hit inward "bite" (fraction of scale) layered on the shudder, decaying over [`ORE_PUNCH_DUR`]
/// — a quick squash so EVERY swing reads as biting in, on top of the slow HP-driven shrink.
const ORE_PUNCH_AMP: f32 = 0.13;
const ORE_PUNCH_DUR: f32 = 0.18;
/// Exponential approach rate (per second) of the settled scale toward its HP target — fast enough
/// that a single blow's worth of shrink lands within the shudder, not seconds later.
const ORE_WEAR_LERP: f32 = 16.0;

/// Erode each boulder toward its remaining-HP scale every frame, plus a transient bite read straight
/// off the active [`TrunkShake`]. Driving the bite from the shake means BOTH hit paths — the hero's
/// swing ([`mine_ore`]) and the town miner's pick (`miner::pick_work`) — feed it with no extra
/// wiring, since both insert a `TrunkShake` on each non-shattering blow. Skips depleted/regrowing
/// nodes so it never fights the hide or the [`RegrowPop`] spring.
#[allow(clippy::type_complexity)]
fn drive_ore_wear(
    time: Res<Time>,
    mut q: Query<
        (&OreNode, &mut OreWear, &mut Transform, Option<&TrunkShake>),
        (Without<DepletedOre>, Without<RegrowPop>),
    >,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    for (node, mut wear, mut tf, shake) in &mut q {
        let frac = (node.ore.hp / node.ore.max_hp).clamp(0.0, 1.0) as f32;
        let target = node.base_scale * (ORE_WEAR_MIN + (1.0 - ORE_WEAR_MIN) * frac);
        // Ease the settled scale toward the worn target — each blow drops `frac`, so the rock
        // visibly takes a notch off with every hit instead of holding full size until it shatters.
        wear.cur += (target - wear.cur) * (1.0 - (-ORE_WEAR_LERP * dt).exp());
        let punch = match shake {
            Some(s) => {
                let t = now - s.started;
                if t < ORE_PUNCH_DUR { (1.0 - t / ORE_PUNCH_DUR) * ORE_PUNCH_AMP } else { 0.0 }
            }
            None => 0.0,
        };
        tf.scale = Vec3::splat((wear.cur * (1.0 - punch)).max(0.02));
    }
}

// ─── Forage (herbs / apples) ───────────────────────────────────────────────────────
//
// Walk-up auto-gather: standing within a plant's tight harvest radius banks it into the bag
// (if there's room) and the plant regrows after a delay. ECS-native (the core `ForageStore`
// snaps to its own tilemap; we keep state per-entity over forest meshes + the 90s constant).

/// Respawn delay (s) — 3× core's TS-parity `DEFAULT_RESPAWN` (90 → 270). A deliberate
/// forest-canonical balance divergence: herbs/apples should be a destination, not a treadmill,
/// so a stripped orchard or bramble patch stays bare a good while.
const FORAGE_RESPAWN: f32 = forage_store::DEFAULT_RESPAWN as f32 * 3.0;

#[derive(Component)]
struct Forage {
    /// Item id granted on gather (`marsh_herb` / `apple`).
    item_id: &'static str,
    /// Auto-gather radius (tiles).
    harvest_r: f32,
    collected: bool,
    /// Elapsed-seconds stamp of the last gather (drives respawn).
    collected_at: f32,
}

/// Gather any active plant the hero is standing on (bag-room permitting); hide + stamp it.
fn forage_pickup(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(&mut Forage, &Transform, &mut Visibility)>,
) {
    if !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for (mut f, tf, mut vis) in &mut q {
        if f.collected {
            continue;
        }
        let d = Vec2::new(tf.translation.x, tf.translation.z).distance(hero.pos);
        if d <= f.harvest_r && try_grant(&mut inv.0, &mut toasts.0, f.item_id, 1, now as f64) {
            f.collected = true;
            f.collected_at = now;
            *vis = Visibility::Hidden;
            cues.write(AudioCue::Forage);
        }
    }
}

/// Regrow collected plants once their respawn delay has elapsed.
fn forage_respawn(time: Res<Time>, mut q: Query<(&mut Forage, &mut Visibility)>) {
    let now = time.elapsed_secs();
    for (mut f, mut vis) in &mut q {
        if f.collected && now - f.collected_at >= FORAGE_RESPAWN {
            f.collected = false;
            *vis = Visibility::Visible;
        }
    }
}

/// Seed marsh herbs over the swamp + forage apples over the forest (called from
/// `worldmap::build`). Herb = a green sprig; apples hang on standout apple TREES you strip whole.
pub fn populate_forage(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    // Swamp herbs: glowing green sprigs (vertex-coloured stalks + bright buds) under a soft
    // emissive green material, so the healing herbs GLOW out of the murk and are actually findable
    // — the old near-black bramble was invisible in the swamp. Plentiful (the swamp is big and
    // poisons you, so the cure should be easy to spot). Gathering banks a `marsh_herb` (heals +
    // resist when eaten with Q, exactly like a forest apple).
    let herb_mesh = meshes.add(marsh_herb_mesh());
    let herb_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(0.16, 0.52, 0.20), // soft green glow → "shiny" healing herb
        perceptual_roughness: 0.55,
        ..default()
    });
    seed_forage(commands, &herb_mesh, &herb_mat, "marsh_herb", 1.0, crate::biome::Biome::Swamp, 55, 0x4e_b5_1c_0d);

    // Forest apples: standout apple TREES (permanent scenery) carrying a cluster of apples that
    // you strip the WHOLE tree at once by walking up — the apples pop off in a satisfying burst.
    // The fruit is glossy-red with a warm emissive lift, sized like an actual apple against
    // the canopy (≈0.29u across after the tree's 1.7× scale). Visibility comes from the
    // hang spots riding the canopy's outer shell + the emissive pop, NOT from oversizing —
    // r 0.15 here read as comical red balloons.
    let apple_mesh = meshes.add(Sphere::new(0.085).mesh().ico(2).unwrap());
    let apple_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.88, 0.15, 0.10),
        emissive: LinearRgba::rgb(0.40, 0.04, 0.03),
        perceptual_roughness: 0.35,
        ..default()
    });
    let tree_mesh = meshes.add(apple_tree_mesh());
    let tree_mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    // Stash the apple mesh/mat so the harvest pop can fling matching motes.
    commands.insert_resource(AppleAssets { fruit_mesh: apple_mesh.clone(), fruit_mat: apple_mat.clone() });
    populate_apple_orchard(commands, &tree_mesh, &tree_mat, &apple_mesh, &apple_mat);
}

/// Canopy positions (tree-local) the gatherable apples hang at — also where they pop from. Only
/// the first [`APPLES_PER_TREE`] are actually hung/harvested; the rest are spare spots.
/// IMPORTANT: each spot sits ON the canopy lobes' outer shell (≈¼ apple-radius inside it), NOT
/// at a lobe centre — buried fruit is invisible fruit. If `apple_tree_mesh`'s canopy changes,
/// re-derive these so every apple still pokes out of the leaves.
const APPLE_SPOTS: [(f32, f32, f32); 6] = [
    (0.55, 0.80, 0.30),   // east-lobe shell, low + front
    (-0.55, 0.86, 0.10),  // west-lobe shell
    (0.18, 0.74, 0.55),   // south-lobe shell, hanging low
    (0.50, 1.10, -0.25),  // upper-east shell, back quarter
    (-0.40, 1.12, 0.35),  // upper-west shell, front quarter
    (0.10, 1.42, 0.10),   // crown tip
];

/// Apples one tree carries (and yields when stripped) — what hangs == what you bag. (3 of the 6
/// `APPLE_SPOTS`; a leaner haul than the old 5 so food has to be worked for, not vacuumed up.)
const APPLES_PER_TREE: usize = 3;

/// Uniform scale every apple tree spawns at — noticeably bigger than the surrounding forest
/// scatter so the orchard trees stand proud of the underbrush (the canopy tops out ~2.5u).
/// Children (the hanging apples) inherit it, which is what makes the fruit chunky + visible.
const APPLE_TREE_SCALE: f32 = 1.7;

/// A whole apple tree you strip at once: walk inside `harvest_r` → all apples pop off into the
/// bag, then the tree regrows them after a delay. `ready` gates re-harvest during regrow.
#[derive(Component)]
struct AppleTree {
    harvest_r: f32,
    apples: u32,
    ready: bool,
    harvested_at: f32,
}

/// One apple hanging on a tree (a child of the [`AppleTree`] entity); hidden while regrowing.
#[derive(Component)]
struct AppleFruit;

/// Transient harvest wobble on an apple tree (inserted by [`apple_harvest`], driven + removed
/// by [`apple_tree_shake`]): the whole tree wags and breathes for a moment as the fruit pops
/// off, so stripping it FEELS like you grabbed the trunk and shook it.
#[derive(Component)]
struct TreeShake {
    started: f32,
    /// The tree's resting yaw, restored when the shake ends.
    base_rot: Quat,
}

/// How long the harvest shake rings out (s).
const SHAKE_DUR: f32 = 0.8;

/// Wag + squash-and-stretch a shaken tree, decaying to rest over [`SHAKE_DUR`], then restore
/// its resting pose and drop the component.
fn apple_tree_shake(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Transform, &TreeShake)>,
) {
    let now = time.elapsed_secs();
    for (e, mut tf, shake) in &mut q {
        let t = now - shake.started;
        if t >= SHAKE_DUR {
            tf.rotation = shake.base_rot;
            tf.scale = Vec3::splat(APPLE_TREE_SCALE);
            commands.entity(e).try_remove::<TreeShake>();
            continue;
        }
        let decay = 1.0 - t / SHAKE_DUR;
        // Two off-frequency sways so the wag tumbles instead of metronoming, plus a quick
        // vertical jiggle — the classic "shake the fruit loose" read.
        let wag_x = (t * 26.0).sin() * 0.055 * decay;
        let wag_z = (t * 21.0 + 1.3).sin() * 0.045 * decay;
        tf.rotation = shake.base_rot * Quat::from_rotation_x(wag_x) * Quat::from_rotation_z(wag_z);
        let squash = 1.0 + (t * 30.0).sin() * 0.04 * decay;
        tf.scale = Vec3::new(
            APPLE_TREE_SCALE * (2.0 - squash).max(0.5),
            APPLE_TREE_SCALE * squash,
            APPLE_TREE_SCALE * (2.0 - squash).max(0.5),
        );
    }
}

/// The apple mesh + material, reused for the harvest "pop" motes.
#[derive(Resource, Clone)]
struct AppleAssets {
    fruit_mesh: Handle<Mesh>,
    fruit_mat: Handle<StandardMaterial>,
}

/// Scatter standout apple trees across the forest. Each tree carries a cluster of [`AppleFruit`]
/// children stripped whole by [`apple_harvest`] and regrown by [`apple_regrow`].
fn populate_apple_orchard(
    commands: &mut Commands,
    tree_mesh: &Handle<Mesh>,
    tree_mat: &Handle<StandardMaterial>,
    apple_mesh: &Handle<Mesh>,
    apple_mat: &Handle<StandardMaterial>,
) {
    const APPLE_TREES: u32 = 24;
    // Keep the apple tree's WHOLE canopy clear of any existing trunk/prop, not just its trunk
    // point: forest canopy (~1.3) + apple canopy (~0.65 × APPLE_TREE_SCALE ≈ 1.1). Without this
    // an apple tree lands a trunk-width from a pine and the two crowns interpenetrate.
    const APPLE_CLEAR: f32 = 2.8;
    let mut rng = 0xa9_71_3f_55u32 | 1;
    let (mut placed, mut attempts) = (0u32, 0u32);
    while placed < APPLE_TREES && attempts < APPLE_TREES * 400 + 800 {
        attempts += 1;
        let x = crate::wildlife::rng_range(&mut rng, -worldmap::GX + 5.0, worldmap::GX - 5.0);
        let z = crate::wildlife::rng_range(&mut rng, -worldmap::GZ + 5.0, worldmap::GZ - 5.0);
        if worldmap::biome_at_world(x, z) != Some(crate::biome::Biome::Forest)
            || worldmap::ground_at_world(x, z).is_none()
            || crate::blockers::any_within(x, z, APPLE_CLEAR)
            || crate::camps::in_clearing(x, z)
            || crate::castle::in_footprint(x, z)
            || crate::bridges::near_bridge(x, z, 1.0)
            // Keep orchard trees off the paths (this special orchard pass used to skip the road
            // check the main scatter does, so apple trees landed dead-centre on a trail — they
            // read as a walkable path but block it) and out of the warden glade.
            || crate::roads::on_road(x, z)
            || crate::boss::in_warden_glade(x, z)
        {
            continue;
        }
        let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
        let yaw = crate::wildlife::rng_range(&mut rng, 0.0, std::f32::consts::TAU);
        if placed == 0 {
            // One stable anchor for staging close-up shots of the orchard (FOREST_CAM framing).
            info!("apple orchard: first tree at ({x:.1}, {z:.1})");
        }
        // Register the trunk as a blocker so the NEXT apple tree (and any mover) keeps clear of it.
        crate::blockers::add(x, z, 0.45);
        commands
            .spawn((
                Mesh3d(tree_mesh.clone()),
                MeshMaterial3d(tree_mat.clone()),
                Transform::from_xyz(x, y, z)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(APPLE_TREE_SCALE)),
                crate::biome::BiomeEntity,
                AppleTree { harvest_r: 2.6, apples: APPLES_PER_TREE as u32, ready: true, harvested_at: 0.0 },
            ))
            .with_children(|p| {
                for (ax, ay, az) in APPLE_SPOTS.into_iter().take(APPLES_PER_TREE) {
                    p.spawn((
                        Mesh3d(apple_mesh.clone()),
                        MeshMaterial3d(apple_mat.clone()),
                        Transform::from_xyz(ax, ay, az),
                        Visibility::Visible,
                        AppleFruit,
                    ));
                }
            });
        placed += 1;
    }
}

/// Strip a whole apple tree on approach: inside `harvest_r` (bag-room permitting) bank every
/// apple at once, pop each fruit off into a flying-mote burst, and start the regrow timer.
#[allow(clippy::too_many_arguments)]
fn apple_harvest(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut cues: MessageWriter<AudioCue>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    assets: Option<Res<AppleAssets>>,
    mut commands: Commands,
    mut trees: Query<(Entity, &mut AppleTree, &Transform, &GlobalTransform, &Children)>,
    mut fruit: Query<(&Transform, &mut Visibility), (With<AppleFruit>, Without<AppleTree>)>,
) {
    if !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for (te, mut tree, ltf, gt, children) in &mut trees {
        if !tree.ready {
            continue;
        }
        let tp = gt.translation();
        if Vec2::new(tp.x, tp.z).distance(hero.pos) > tree.harvest_r {
            continue;
        }
        if !try_grant(&mut inv.0, &mut toasts.0, "apple", tree.apples as i64, now as f64) {
            continue; // bag full — leave the fruit on the tree
        }
        tree.ready = false;
        tree.harvested_at = now;
        // Kick the harvest shake from the tree's resting yaw (it can't already be shaking —
        // `ready` only comes back long after the wobble has rung out).
        commands.entity(te).try_insert(TreeShake { started: now, base_rot: ltf.rotation });
        // Pop each apple off where it hangs.
        for &c in children {
            if let Ok((ltf, mut vis)) = fruit.get_mut(c) {
                *vis = Visibility::Hidden;
                if let Some(a) = &assets {
                    let wp = gt.transform_point(ltf.translation);
                    crate::player::spawn_motes(&mut commands, &a.fruit_mesh, &a.fruit_mat, wp, 3, 2.4, 1.0, 0.55);
                }
            }
        }
        floats.0.push(FloatReq {
            world: Vec3::new(tp.x, tp.y + 1.7, tp.z),
            text: format!("+{} apples", tree.apples),
            color: Color::srgb(0.95, 0.45, 0.30),
            scale: 1.2,
        });
        // The old game's forage (apples/herbs) played `playGold()` — the bright two-blip pickup
        // jingle for "got something". `AudioCue::Gold` is the faithful synth port of it.
        cues.write(AudioCue::Gold);
    }
}

/// Regrow a stripped tree's apples once the respawn delay has elapsed.
fn apple_regrow(
    time: Res<Time>,
    mut trees: Query<(&mut AppleTree, &Children)>,
    mut fruit: Query<&mut Visibility, With<AppleFruit>>,
) {
    let now = time.elapsed_secs();
    for (mut tree, children) in &mut trees {
        if tree.ready || now - tree.harvested_at < FORAGE_RESPAWN {
            continue;
        }
        tree.ready = true;
        for &c in children {
            if let Ok(mut vis) = fruit.get_mut(c) {
                *vis = Visibility::Visible;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn seed_forage(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    item_id: &'static str,
    harvest_r: f32,
    biome: crate::biome::Biome,
    count: u32,
    seed: u32,
) {
    let mut rng = seed | 1;
    let (mut placed, mut attempts) = (0u32, 0u32);
    while placed < count && attempts < count * 400 + 800 {
        attempts += 1;
        let x = crate::wildlife::rng_range(&mut rng, -worldmap::GX + 5.0, worldmap::GX - 5.0);
        let z = crate::wildlife::rng_range(&mut rng, -worldmap::GZ + 5.0, worldmap::GZ - 5.0);
        if worldmap::biome_at_world(x, z) != Some(biome)
            || worldmap::ground_at_world(x, z).is_none()
            || crate::blockers::is_blocked(x, z)
            || crate::camps::in_clearing(x, z)
            || crate::bridges::near_bridge(x, z, 1.0)
        {
            continue;
        }
        let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, y, z),
            Visibility::Visible,
            Forage { item_id, harvest_r, collected: false, collected_at: 0.0 },
            crate::biome::BiomeEntity,
        ));
        placed += 1;
    }
}


/// Forest-native frontier gradient: 0 across the safe core around the castle (the world
/// origin), smoothly → 1 toward the rim. (Core's `frontier_factor` is anchored to its own
/// enlarged tilemap; we recompute the gradient and reuse only core's pure loot picks.)
pub(crate) fn forest_frontier(x: f32, z: f32) -> f64 {
    const SAFE: f32 = 22.0;
    const RIM: f32 = 92.0;
    let d = (x * x + z * z).sqrt();
    let t = ((d - SAFE) / (RIM - SAFE)).clamp(0.0, 1.0) as f64;
    t * t * (3.0 - 2.0 * t) // smoothstep
}

/// Distance-graded danger multipliers `(hp_mul, dmg_mul)` for a ROAMING enemy at world (x,z):
/// ×1 across the safe core, climbing to ×2 HP / ×1.6 damage out at the rim. The reward for
/// ranging far (richer chests, top-tier loot) is paid for in tougher, harder-hitting wildlife
/// and camp orks. Applied at spawn-time HP ([`crate::player::combat::ensure_combat_health`]) and
/// at each bite/club ([`crate::wildlife`], [`crate::orks`]).
///
/// NB this deliberately does NOT touch the night-siege invaders: they spawn in a far ring and
/// march to the keep at the origin, so grading them by spawn distance would silently buff every
/// siege. Invaders carry their own wave-scaled HP from `siege::spawn_invader` and never read this.
pub(crate) fn frontier_threat(x: f32, z: f32) -> (f32, f32) {
    let f = forest_frontier(x, z) as f32;
    (1.0 + f, 1.0 + 0.6 * f)
}


// Local flat-shaded mesh helpers (vertex-coloured; mirror the camps/orks prop builders).
fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn ctint(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn cbx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    ctint(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn cbxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    ctint(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), c)
}
fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}
fn ccyl(r: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    ctint(Cylinder::new(r, h).mesh().resolution(12).build().rotated_by(rot).translated_by(off), c)
}
fn cgroup(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
/// Untinted cone (used for the glowing crystal shards — they take an emissive material, so the
/// vertex colour the others carry is unnecessary; they only merge with each other).
fn ccone(r: f32, h: f32, off: Vec3, rot: Quat) -> Mesh {
    Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off)
}
fn csph(r: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    ctint(Sphere::new(r).mesh().ico(1).unwrap().translated_by(off), c)
}

// ─── Ore + apple-tree models (vertex-coloured / emissive props) ─────────────────────

/// Blocky craggy boulder — a cluster of tumbled, rotated stone blocks (low, wide footprint) so
/// it reads as a chunky mineable rock rather than a smooth pebble; the gem cluster crowns it.
fn ore_rock_mesh() -> Mesh {
    let g1 = lin(0x4c4c55);
    let g2 = lin(0x3a3a43);
    let g3 = lin(0x5a5a64);
    cgroup(vec![
        cbxr(0.95, 0.46, 0.82, v(0.0, 0.20, 0.0), ry(0.35), g1),                              // main slab
        cbxr(0.52, 0.40, 0.50, v(0.34, 0.26, 0.16), Quat::from_euler(EulerRot::XYZ, 0.22, 0.8, 0.15), g2),
        cbxr(0.48, 0.34, 0.58, v(-0.30, 0.22, -0.16), Quat::from_euler(EulerRot::XYZ, -0.16, -0.5, 0.20), g3),
        cbxr(0.40, 0.44, 0.40, v(0.04, 0.40, -0.04), Quat::from_euler(EulerRot::XYZ, 0.25, 0.25, -0.18), g1),
    ])
}

/// A big eruption of upward crystal shards (varied size + tilt) — the glowing mineable core.
/// Deliberately OVERSIZED relative to the rock (the tall centre spike clears a full unit) and
/// the only gem crystals anywhere in the rocky biome, so an ore node is unmistakable from far
/// off: see a glowing crystal → it's mineable. Crowns the blocky rock; small chips spill down
/// its shoulders so the cluster reads rooted, not perched.
fn ore_crystal_mesh() -> Mesh {
    cgroup(vec![
        // Tall centre spike + two strong leaners — the far-distance silhouette.
        ccone(0.155, 0.92, v(0.0, 0.74, 0.0), Quat::IDENTITY),
        ccone(0.115, 0.60, v(0.22, 0.56, 0.10), rz(-0.50)),
        ccone(0.110, 0.62, v(-0.21, 0.57, -0.08), rz(0.55)),
        // Mid fan filling the crown.
        ccone(0.090, 0.46, v(0.06, 0.54, 0.24), rx(0.55)),
        ccone(0.085, 0.44, v(-0.08, 0.54, -0.23), rx(-0.55)),
        ccone(0.075, 0.36, v(0.18, 0.50, -0.16), rz(-0.35) * rx(-0.4)),
        // Shoulder chips spilling down the rock.
        ccone(0.060, 0.26, v(0.30, 0.36, 0.02), rz(-0.85)),
        ccone(0.055, 0.24, v(-0.28, 0.36, 0.12), rz(0.90)),
    ])
}

/// Standout apple tree (authored at unit scale; spawned at [`APPLE_TREE_SCALE`], so in-world
/// it tops the underbrush): a gnarled orchard trunk forking into three limbs, a full
/// three-tone canopy (shadowed underside → orchard green → sunlit top, brighter than the
/// forest's pines/broadleaf so it reads as special) dusted with pale blossom, and a root
/// flare gripping the grass. Trunk base at y=0. The `APPLE_SPOTS` hang points ride this
/// canopy's outer SHELL (the apples — separate child entities that pop off — must protrude
/// from the leaves, never sink inside a lobe; keep the two in sync when reshaping).
fn apple_tree_mesh() -> Mesh {
    let trunk = lin(0x6b4a2a);
    let leaf_dk = lin(0x3d7e2e);
    let leaf = lin(0x4f9c3a);
    let leaf_hi = lin(0x74c64c);
    let blossom = lin(0xf3e9da);
    let mut parts = vec![
        // Stout tapering bole, kinked a touch off plumb like a pruned orchard tree.
        ccyl(0.115, 0.42, v(0.0, 0.21, 0.0), Quat::IDENTITY, trunk),
        ccyl(0.085, 0.34, v(0.025, 0.52, 0.01), rz(-0.10), trunk),
        // Three limbs forking from the bole crook up into the canopy.
        ccyl(0.045, 0.34, v(0.20, 0.78, 0.10), rz(-0.55), trunk),
        ccyl(0.042, 0.32, v(-0.18, 0.80, -0.05), rz(0.60), trunk),
        ccyl(0.038, 0.28, v(0.02, 0.84, -0.14), rx(0.5), trunk),
    ];
    // Root flare: four stubby toes leaning out from the foot.
    for i in 0..4 {
        let a = 0.5 + i as f32 * std::f32::consts::FRAC_PI_2;
        parts.push(ccyl(
            0.045,
            0.14,
            v(a.cos() * 0.10, 0.045, a.sin() * 0.10),
            ry(-a) * rz(1.0),
            trunk,
        ));
    }
    // Canopy: dark grounded underside → mid body → sunlit cap, wrapping the hang spots.
    for (r, x, yy, z, c) in [
        (0.40, 0.0, 0.86, 0.04, leaf_dk),   // core mass
        (0.30, 0.32, 0.84, 0.16, leaf),     // east lobe (covers spot 1)
        (0.30, -0.32, 0.88, 0.04, leaf),    // west lobe (covers spot 2)
        (0.27, 0.10, 0.80, 0.34, leaf_dk),  // south lobe (covers spot 3)
        (0.28, 0.26, 1.06, -0.08, leaf),    // upper-east (spot 4)
        (0.26, -0.18, 1.10, 0.20, leaf_hi), // upper-west (spot 5)
        (0.26, 0.02, 1.22, 0.02, leaf_hi),  // crown (spot 6)
        (0.18, 0.12, 1.34, -0.06, leaf_hi), // sunlit tip
    ] {
        parts.push(csph(r, v(x, yy, z), c));
    }
    // A dusting of pale blossom over the sunny side — the orchard tree's calling card.
    for (x, yy, z) in [
        (0.30_f32, 1.22_f32, 0.14_f32),
        (-0.26, 1.26, -0.06),
        (0.06, 1.40, 0.10),
        (0.42, 1.00, 0.20),
        (-0.38, 1.06, 0.16),
    ] {
        parts.push(csph(0.045, v(x, yy, z), blossom));
    }
    cgroup(parts)
}

/// A glowing marsh herb — a low tuft sprouting several slender stalks, each tipped with a bright
/// luminous bud. Paired with a soft EMISSIVE green material (see [`populate_forage`]) the whole
/// sprig glows out of the swamp murk so the healing herbs are actually findable — the old
/// near-black bramble vanished into the mire. Vertex-coloured to share the prop material contract.
fn marsh_herb_mesh() -> Mesh {
    let stem = lin(0x5aa84a); // fresh green stalk
    let leaf = lin(0x3f8a30); // deeper green blade
    let bud = lin(0xcaffa0); // bright tip — reads luminous under the emissive lift
    let mut parts = vec![csph(0.12, v(0.0, 0.06, 0.0), leaf)]; // root tuft
    const N: usize = 6;
    for i in 0..N {
        let a = i as f32 / N as f32 * std::f32::consts::TAU + 0.5;
        let r = 0.07;
        let base = v(a.cos() * r, 0.06, a.sin() * r);
        let h = 0.40 + (i % 3) as f32 * 0.10; // varied stalk heights
        let spread = 0.14;
        let tip = v(a.cos() * (r + spread), h, a.sin() * (r + spread));
        let dir = (tip - base).normalize_or_zero();
        let rot = Quat::from_rotation_arc(Vec3::Y, dir);
        let len = (tip - base).length();
        let mid = (base + tip) * 0.5;
        parts.push(ccyl(0.016, len, mid, rot, stem)); // slender stalk
        parts.push(csph(0.055, tip, bud)); // glowing bud at the tip
        parts.push(cbxr(0.12, 0.015, 0.05, mid, rot, leaf)); // a leaf blade midway
    }
    cgroup(parts)
}

// ─── Hunting: per-species drops + ground pickups ───────────────────────────────────
//
// On a wild-animal kill (`AnimalKilled`, published by combat) we roll its config drop(s) +
// a frontier-graded bonus, spawning floating loot motes the hero walks over to bag. HP and
// bounty come from [`animal_profile`], straight off core's `animal_config` (the TS values).

/// A wild animal's forest combat profile: full TS HP + (HP-independent) bounty + loot drops.
pub struct AnimalProfile {
    pub hp: f32,
    pub gold: i64,
    pub xp: i64,
    /// Primary drop `(item id, 0..1 chance)`.
    pub drop: Option<(&'static str, f64)>,
    /// Rarer second drop `(item id, 0..1 chance)`.
    pub drop2: Option<(&'static str, f64)>,
}

/// Map a forest species to its core `animal_config` counterpart (Camel/Cat have no core
/// entry — handled inline by [`animal_profile`]).
pub(crate) fn core_species(s: Species) -> Option<tileworld_core::animal::Species> {
    use tileworld_core::animal::Species as C;
    Some(match s {
        Species::Wolf => C::Wolf,
        Species::Deer => C::Deer,
        Species::Boar => C::Boar,
        Species::Rabbit => C::Rabbit,
        Species::PolarBear => C::PolarBear,
        Species::Elk => C::Elk,
        Species::Goat => C::Goat,
        Species::Dog => C::Dog,
        Species::Golem => C::Golem,
        Species::Scorpion => C::Scorpion,
        Species::BogCroc => C::BogCroc,
        Species::Camel | Species::Cat => return None,
    })
}

/// Forest combat profile for a species — core (TS) stats used 1:1, drops/bounty kept verbatim.
/// HP is the old game's value directly (hero base damage is 25, so a wolf soaks ~4 blows, a boar
/// ~6, a golem ~12, while a rabbit still pops in one). Camel/Cat are hand-authored (no core entry).
pub fn animal_profile(s: Species) -> AnimalProfile {
    if let Some(cs) = core_species(s) {
        let c = tileworld_core::animal::animal_config(cs);
        AnimalProfile {
            hp: (c.hp.round() as f32).max(2.0),
            gold: c.bounty_gold as i64,
            xp: c.bounty_xp as i64,
            drop: c.drop_item.map(|id| (id, c.drop_chance)),
            drop2: c.drop_item2.map(|id| (id, c.drop_chance2)),
        }
    } else {
        match s {
            Species::Camel => AnimalProfile { hp: 50.0, gold: 8, xp: 12, drop: None, drop2: None },
            _ /* Cat */ => AnimalProfile { hp: 10.0, gold: 2, xp: 3, drop: None, drop2: None },
        }
    }
}

/// Published by combat on a wild-animal kill so this module rolls + spawns its loot.
#[derive(Message)]
pub struct AnimalKilled {
    pub at: Vec3,
    pub species: Species,
}

/// A floating loot mote on the ground — walk over it to bag the item.
#[derive(Component)]
struct GroundDrop {
    item_id: &'static str,
    home_y: f32,
    spin: f32,
}

/// Shared loot-mote visuals, built once.
#[derive(Resource)]
struct DropAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

fn setup_drop_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Cuboid::new(0.22, 0.22, 0.22).mesh().build());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.75, 0.45),
        emissive: LinearRgba::rgb(0.7, 0.55, 0.18),
        unlit: true,
        ..default()
    });
    commands.insert_resource(DropAssets { mesh, mat });

    // Chop-burst chips: one tiny cuboid, unlit (NO emissive — debris must not bloom like sparks),
    // tinted pale split-wood / canopy green by two shared materials.
    let chip_mesh = meshes.add(Cuboid::new(0.09, 0.09, 0.09).mesh().build());
    let wood_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.60, 0.36),
        unlit: true,
        ..default()
    });
    let leaf_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.31, 0.61, 0.23),
        unlit: true,
        ..default()
    });
    commands.insert_resource(TreeFx { chip_mesh, wood_mat, leaf_mat });
}

/// Roll an animal's drops (meat + its species' primary/secondary items) and spawn a floating mote
/// for each, scattered around the kill point. Wild kills NO LONGER cough up random weapons/armor —
/// the frontier gear-rain was pulled so that top-tier gear is earned ONLY at the biome landmark
/// trials (`landmarks.rs`), making gear acquisition legible instead of a random sprinkle.
fn animal_drops(
    mut kills: MessageReader<AnimalKilled>,
    mut rng: ResMut<VerbRng>,
    assets: Option<Res<DropAssets>>,
    mut commands: Commands,
) {
    let Some(assets) = assets else {
        return;
    };
    for k in kills.read() {
        let prof = animal_profile(k.species);
        let mut drops: Vec<&'static str> = Vec::new();
        // Every wild-animal kill always yields meat (player request) — on top of the species'
        // rolled drops (fur/venom/steak/…), which stay as flavour bonuses.
        drops.push("meat");
        if let Some((id, chance)) = prof.drop {
            if rng.unit() < chance {
                drops.push(id);
            }
        }
        if let Some((id, chance)) = prof.drop2 {
            if rng.unit() < chance {
                drops.push(id);
            }
        }
        // Gear gate: a hunt-kill NEVER yields weapons or armor — not the golem's stone_maul/iron_armor,
        // and not the polar bear's leather_armor either. Wearable gear comes ONLY from the shop and the
        // biome landmark trials (player rule). Meat, hides and buff consumables still pass through.
        // (This was the long-standing "random armor at random moments" leak: hunting polar bears rained
        // leather_armor because the old gate kept the "starter kit" — there is no starter exception now.)
        drops.retain(|id| {
            let wearable = tileworld_core::inventory::item_def(id)
                .map(|d| matches!(d.kind, tileworld_core::inventory::ItemKind::Weapon | tileworld_core::inventory::ItemKind::Armor))
                .unwrap_or(false);
            !wearable
        });
        for id in drops {
            let ang = (rng.unit() * std::f64::consts::TAU) as f32;
            let r = 0.2 + rng.unit() as f32 * 0.5;
            let x = k.at.x + ang.cos() * r;
            let z = k.at.z + ang.sin() * r;
            let home_y = worldmap::ground_at_world(x, z).unwrap_or(k.at.y) + 0.35;
            commands.spawn((
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.mat.clone()),
                Transform::from_xyz(x, home_y, z),
                GroundDrop { item_id: id, home_y, spin: rng.unit() as f32 * 6.28 },
                bevy::light::NotShadowCaster,
                crate::biome::BiomeEntity,
            ));
        }
    }
}

/// Bob + spin each loot mote; bank it to the bag (toast) when the hero walks over it.
fn ground_pickup(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut cues: MessageWriter<AudioCue>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut GroundDrop, &mut Transform)>,
) {
    let t = time.elapsed_secs();
    let now = t as f64;
    for (e, d, mut tf) in &mut q {
        tf.translation.y = d.home_y + (t * 3.0 + d.spin).sin() * 0.08;
        tf.rotation = Quat::from_rotation_y(t * 2.4 + d.spin);
        if hero.alive
            && Vec2::new(tf.translation.x, tf.translation.z).distance(hero.pos) < 1.0
            && try_grant(&mut inv.0, &mut toasts.0, d.item_id, 1, now)
        {
            cues.write(AudioCue::UiSelect);
            commands.entity(e).despawn();
        }
    }
}
