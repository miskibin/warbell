//! **Biome verbs** — acting on the world to feed the bag + stone bank. This module owns the
//! [`HeroSwing`] broadcast (combat publishes one cone per swing-hit; mining reads it) and the
//! ore-mining loop. Foraging, chests and hunt drops land alongside it in P2c/P2d.
//!
//! Placement is forest-coord native: [`populate_ore`] (called from `worldmap::build`)
//! reject-samples the rock biome and constructs the test-gated `ore_store::Ore` directly,
//! bypassing core's `OreField::create` (which would snap to core's *own* enlarged tilemap).

use bevy::prelude::*;
use tileworld_core::ore_store::{Ore, ORE_COLLISION_RADIUS, ORE_STONE};
use tileworld_core::{forage_store, frontier};

use crate::audio::AudioCue;
use crate::combat_fx::FloatReq;
use crate::critters::Species;
use crate::economy::Bank;
use crate::game_state::Modal;
use crate::inventory::{try_grant, Inventory, Toasts};
use crate::palette::lin;
use crate::player::{HeroState, PlayerRes};
use crate::worldmap;

/// Forest ore HP — rescaled from core's TS-anchored 500 into forest's 60-HP combat units
/// (~5 swings at the hero's base 25 damage). A real dig, not a slog.
const ORE_HP: f64 = 118.0;
/// Front-cone reach the swing checks ore against (the hero melee cone + the boulder radius).
const SWING_RANGE: f32 = 1.9;
const SWING_CONE_DOT: f32 = 0.5;
/// How many boulders to seed across the rock biome.
const ORE_COUNT: u32 = 18;

/// Published by combat at each swing's hit-phase: the cone the blow sweeps. Mining (and later
/// the training dummies) test their targets against it, sharing the player's one swing.
#[derive(Message)]
pub struct HeroSwing {
    /// Hero world XZ at the moment of the blow.
    pub origin: Vec2,
    /// Facing unit vector `(sin, cos)`.
    pub fwd: Vec2,
    /// Non-crit swing damage (ore/dummies take this; they don't crit).
    pub base_dmg: f32,
}

/// A mineable boulder — wraps the pure `ore_store::Ore` (HP + shatter logic).
#[derive(Component)]
pub struct OreNode {
    ore: Ore,
}

pub struct VerbsPlugin;

impl Plugin for VerbsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<HeroSwing>()
            .add_message::<AnimalKilled>()
            .init_resource::<VerbRng>()
            .add_systems(Startup, setup_drop_assets)
            .add_systems(
                Update,
                (
                    mine_ore,
                    chop_tree,
                    forage_pickup,
                    forage_respawn,
                    apple_harvest,
                    apple_regrow,
                    chest_interact,
                    chest_respawn,
                    animal_drops,
                    ground_pickup,
                )
                    .run_if(in_state(Modal::None)),
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
    mut q: Query<(Entity, &mut OreNode, &Transform)>,
) {
    let now = time.elapsed_secs() as f64;
    for sw in swings.read() {
        for (e, mut node, tf) in &mut q {
            if node.ore.hp <= 0.0 {
                continue;
            }
            let p = tf.translation;
            let to = Vec2::new(p.x - sw.origin.x, p.z - sw.origin.y);
            let dist = to.length();
            if dist > SWING_RANGE + ORE_COLLISION_RADIUS as f32 || dist < 1e-3 {
                continue;
            }
            if (to / dist).dot(sw.fwd) < SWING_CONE_DOT {
                continue;
            }
            let shattered = node.ore.damage(sw.base_dmg as f64, now);
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
                crate::blockers::remove_at(p.x, p.z); // clear the boulder blocker — no ghost collision
                commands.entity(e).try_despawn();
            } else {
                // Metallic chip + a small grey rock-chip spray each pick-swing (was a flesh hit).
                if let Some(fx) = &fx {
                    crate::player::spawn_chips(&mut commands, fx, chip_at, false);
                }
                cues.write(AudioCue::OreChip);
            }
        }
    }
}

// ── Tree chopping (1 tree = 1 wood) — the wood mirror of ore mining ──────────────────

/// A choppable tree: an individual entity (the decorative scattered trees are merged, so
/// can't carry per-tree HP). Looks like any forest tree; fell it for wood.
#[derive(Component)]
pub struct ChopTree {
    hp: f64,
}

impl ChopTree {
    /// A fresh choppable tree at full HP — added to every scattered tree in `biome::scatter_region`.
    pub fn new() -> Self {
        Self { hp: TREE_HP }
    }
}

/// Swings to fell a tree (~2 at the hero's base 25–30 dmg).
const TREE_HP: f64 = 55.0;
/// Wood banked per felled tree.
const TREE_WOOD: f64 = 1.0;
/// Tree trunk radius added to the swing reach.
const CHOP_TREE_RADIUS: f32 = 1.0;

/// Read each published swing; any live choppable tree in the cone takes the blow. On felling
/// it banks 1 wood, pops a float, and despawns. Mirrors [`mine_ore`].
fn chop_tree(
    mut swings: MessageReader<HeroSwing>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(Entity, &mut ChopTree, &Transform)>,
) {
    for sw in swings.read() {
        let mut struck = false; // one chop thunk per swing, even if several trees are in the cone
        for (e, mut tree, tf) in &mut q {
            if tree.hp <= 0.0 {
                continue;
            }
            let p = tf.translation;
            let to = Vec2::new(p.x - sw.origin.x, p.z - sw.origin.y);
            let dist = to.length();
            if dist > SWING_RANGE + CHOP_TREE_RADIUS || dist < 1e-3 {
                continue;
            }
            if (to / dist).dot(sw.fwd) < SWING_CONE_DOT {
                continue;
            }
            struck = true;
            tree.hp -= sw.base_dmg as f64;
            if tree.hp <= 0.0 {
                bank.0.add_wood(TREE_WOOD);
                floats.0.push(crate::combat_fx::FloatReq {
                    world: Vec3::new(p.x, p.y + 1.6, p.z),
                    text: format!("+{} wood", TREE_WOOD as i64),
                    color: Color::srgb(0.78, 0.62, 0.36),
                    scale: 1.1,
                });
                crate::blockers::remove_at(p.x, p.z); // clear the trunk blocker so no ghost nub
                commands.entity(e).try_despawn();
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
        let scale = crate::wildlife::rng_range(&mut rng, 0.8, 1.25);
        let yaw = crate::wildlife::rng_range(&mut rng, 0.0, std::f32::consts::TAU);
        let vi = (ore.variant as usize) % crystal_mats.len();
        let crystal_mat = crystal_mats[vi].clone();
        let glow = gem[vi].0;
        // Block the boulder's footprint so the hero (and every mover) bumps it instead of walking
        // through it — scaled with the rock, kept ≤1.0 for the neighbour-only blocker scan. Cleared
        // in `mine_ore` on shatter so no invisible nub lingers where the boulder stood.
        crate::blockers::add(x, z, (0.55 * scale).min(0.95));
        commands
            .spawn((
                Transform::from_xyz(x, y + 0.10 * scale, z)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(scale)),
                Visibility::Visible,
                OreNode { ore },
                crate::biome::BiomeEntity,
            ))
            .with_children(|p| {
                p.spawn((Mesh3d(rock_mesh.clone()), MeshMaterial3d(rock_mat.clone()), Transform::default()));
                p.spawn((Mesh3d(crystal_mesh.clone()), MeshMaterial3d(crystal_mat), Transform::default()));
                // Soft coloured glow from the gem core — no shadows (cheap; ~18 of these on the map).
                p.spawn((
                    PointLight {
                        color: glow,
                        intensity: 12_000.0,
                        range: 4.5,
                        radius: 0.15,
                        shadows_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 0.7, 0.0),
                ));
            });
        placed += 1;
    }
    if placed < ORE_COUNT {
        info!("ore: placed {placed}/{ORE_COUNT} boulders");
    }
}

// ─── Forage (herbs / apples) ───────────────────────────────────────────────────────
//
// Walk-up auto-gather: standing within a plant's tight harvest radius banks it into the bag
// (if there's room) and the plant regrows after a delay. ECS-native (the core `ForageStore`
// snaps to its own tilemap; we keep state per-entity over forest meshes + the 90s constant).

/// Respawn delay (s) — core's `DEFAULT_RESPAWN`.
const FORAGE_RESPAWN: f32 = forage_store::DEFAULT_RESPAWN as f32;

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
    // Swamp brambles: a low leafy mound studded with near-black blackberries (vertex-coloured,
    // so it shares the white prop material like the apple tree). Reads as a gatherable berry bush.
    let herb_mesh = meshes.add(bramble_mesh());
    let herb_mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    seed_forage(commands, &herb_mesh, &herb_mat, "marsh_herb", 0.9, crate::biome::Biome::Swamp, 20, 0x4e_b5_1c_0d);

    // Forest apples: standout apple TREES (permanent scenery) carrying a cluster of apples that
    // you strip the WHOLE tree at once by walking up — the apples pop off in a satisfying burst.
    let apple_mesh = meshes.add(Sphere::new(0.11).mesh().ico(2).unwrap());
    let apple_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.82, 0.16, 0.13),
        emissive: LinearRgba::rgb(0.20, 0.02, 0.02),
        perceptual_roughness: 0.5,
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
const APPLE_SPOTS: [(f32, f32, f32); 6] = [
    (0.34, 0.84, 0.28),
    (-0.36, 0.88, 0.14),
    (0.10, 0.74, 0.42),
    (0.40, 1.04, -0.08),
    (-0.22, 1.10, 0.28),
    (0.04, 1.22, 0.04),
];

/// Apples one tree carries (and yields when stripped) — what hangs == what you bag. (3 of the 6
/// `APPLE_SPOTS`; bumped from 2 for more forest food.)
const APPLES_PER_TREE: usize = 3;

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
    // point: forest canopy (~1.3) + apple canopy (~0.65). Without this an apple tree lands a
    // trunk-width from a pine and the two crowns interpenetrate. ≈ the forest's own tree spacing.
    const APPLE_CLEAR: f32 = 2.0;
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
        {
            continue;
        }
        let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
        let yaw = crate::wildlife::rng_range(&mut rng, 0.0, std::f32::consts::TAU);
        // Register the trunk as a blocker so the NEXT apple tree (and any mover) keeps clear of it.
        crate::blockers::add(x, z, 0.3);
        commands
            .spawn((
                Mesh3d(tree_mesh.clone()),
                MeshMaterial3d(tree_mat.clone()),
                Transform::from_xyz(x, y, z).with_rotation(Quat::from_rotation_y(yaw)),
                crate::biome::BiomeEntity,
                AppleTree { harvest_r: 2.0, apples: APPLES_PER_TREE as u32, ready: true, harvested_at: 0.0 },
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
    mut trees: Query<(&mut AppleTree, &GlobalTransform, &Children)>,
    mut fruit: Query<(&Transform, &mut Visibility), With<AppleFruit>>,
) {
    if !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for (mut tree, gt, children) in &mut trees {
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

// ─── Chests (F-to-open) ────────────────────────────────────────────────────────────
//
// Treasure chests give one-shot frontier gear; supply caches give repeatable gold + food,
// re-closing after a delay. Loot tier climbs with distance from the castle (forest-native
// frontier gradient), reusing core's pure `roll_gear` pool picks.

const CHEST_INTERACT_DIST: f32 = 2.2;
const CACHE_RESPAWN: f32 = 150.0;

#[derive(Component)]
pub(crate) struct Chest {
    /// true = repeatable supply cache (gold + food), false = one-shot treasure (gear).
    pub(crate) cache: bool,
    pub(crate) opened: bool,
    pub(crate) opened_at: f32,
    /// Frontier factor at placement → loot tier.
    factor: f64,
}

/// Stable index of a chest over the spawn order (`0..CHEST_COUNT`). The save keys a
/// looted-treasure flag by this so a re-launched world re-opens the right chests.
#[derive(Component)]
pub(crate) struct ChestId(pub usize);

/// The hinged lid (a child of the chest) — rotated open on loot, closed on a cache respawn.
#[derive(Component)]
pub(crate) struct ChestLid;

/// Lid hinge angle when open (front swings up + back). Shared by `chest_interact` and the
/// save-restore pass so a loaded looted chest shows its lid open.
pub(crate) const CHEST_LID_OPEN: f32 = -1.7;

/// Forest-native frontier gradient: 0 across the safe core around the castle (the world
/// origin), smoothly → 1 toward the rim. (Core's `frontier_factor` is anchored to its own
/// enlarged tilemap; we recompute the gradient and reuse only core's pure loot picks.)
fn forest_frontier(x: f32, z: f32) -> f64 {
    const SAFE: f32 = 22.0;
    const RIM: f32 = 92.0;
    let d = (x * x + z * z).sqrt();
    let t = ((d - SAFE) / (RIM - SAFE)).clamp(0.0, 1.0) as f64;
    t * t * (3.0 - 2.0 * t) // smoothstep
}

/// Deterministic [0,1) per-position hash — stable loot per chest.
fn tile_hash(x: f32, z: f32) -> f64 {
    let s = (x as f64 * 127.1 + z as f64 * 311.7).sin() * 43758.5453;
    s - s.floor()
}

/// Press **F** near a closed chest to loot it: gold to the purse + items to the bag (blocked
/// if the bag can't hold the gear), lid swings open. Caches re-close + refill after a delay.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn chest_interact(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<HeroState>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut player: ResMut<PlayerRes>,
    mut cues: MessageWriter<AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut chests: Query<(&mut Chest, &Transform, &Children), Without<ChestLid>>,
    mut lids: Query<&mut Transform, (With<ChestLid>, Without<Chest>)>,
) {
    if !keys.just_pressed(KeyCode::KeyF) || !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for (mut chest, tf, children) in &mut chests {
        if chest.opened {
            continue;
        }
        let p = tf.translation;
        if Vec2::new(p.x, p.z).distance(hero.pos) > CHEST_INTERACT_DIST {
            continue;
        }
        let head = Vec3::new(p.x, p.y + 1.4, p.z);
        // Resolve loot: caches → gold + a loaf; treasure → frontier gear + gold.
        let (loot, gold): (Vec<&'static str>, i64) = if chest.cache {
            // Some caches also hold a Mercenary Contract (spent via R to hire a guard).
            let mut loot = vec!["bread"];
            if tile_hash(p.x, p.z) > 0.7 {
                loot.push("mercenary_contract");
            }
            (loot, (10.0 + chest.factor * 30.0).round() as i64)
        } else {
            let h = tile_hash(p.x, p.z);
            let items = 1 + chest.factor.round() as i64;
            let loot = (0..items)
                .map(|i| frontier::roll_gear(chest.factor, (h + i as f64 * 0.37) % 1.0))
                .collect();
            (loot, (15.0 + chest.factor * 60.0 + h * 20.0).round() as i64)
        };
        // Won't open if the bag can't hold the gear (TS: full bag rejects the chest).
        if !inv.0.has_room_for(&loot) {
            floats.0.push(FloatReq { world: head, text: "Bag full".into(), color: crate::combat_fx::col_block(), scale: 1.0 });
            cues.write(AudioCue::UiSelect);
            return;
        }
        player.0.add_gold(gold);
        for id in &loot {
            try_grant(&mut inv.0, &mut toasts.0, id, 1, now as f64);
        }
        floats.0.push(FloatReq { world: head, text: format!("+{gold} gold"), color: crate::combat_fx::col_kill(), scale: 1.1 });
        cues.write(AudioCue::ChestOpen);
        cues.write(AudioCue::Gold); // a coin chime layered over the chest creak
        speak.write(crate::audio::Speak::new(crate::audio::Concept::ChestOpen)); // hero muses

        chest.opened = true;
        chest.opened_at = now;
        for &c in children {
            if let Ok(mut lt) = lids.get_mut(c) {
                lt.rotation = Quat::from_rotation_x(CHEST_LID_OPEN); // hinge open (lid stands up at the back)
            }
        }
        return; // one chest per press
    }
}

/// Re-close + refill supply caches once their respawn delay elapses.
#[allow(clippy::type_complexity)]
fn chest_respawn(
    time: Res<Time>,
    mut chests: Query<(&mut Chest, &Children), Without<ChestLid>>,
    mut lids: Query<&mut Transform, (With<ChestLid>, Without<Chest>)>,
) {
    let now = time.elapsed_secs();
    for (mut chest, children) in &mut chests {
        if chest.cache && chest.opened && now - chest.opened_at >= CACHE_RESPAWN {
            chest.opened = false;
            for &c in children {
                if let Ok(mut lt) = lids.get_mut(c) {
                    lt.rotation = Quat::IDENTITY; // closed
                }
            }
        }
    }
}

/// Scatter chests across the island (called from `worldmap::build`). Alternating treasure /
/// cache, varied distance so loot tiers spread; avoids the courtyard, camps and water.
pub fn populate_chests(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    const CHEST_COUNT: u32 = 12;
    // Vertex-coloured (flat-shaded) chest: a rounded iron-banded lid on a strapped wooden body
    // with a brass lock and stub feet. One white material tints by vertex colour, so body + lid
    // batch into two meshes shared across all chests.
    let base_mesh = meshes.add(chest_body_mesh());
    let lid_mesh = meshes.add(chest_lid_mesh());
    let chest_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.78,
        ..default()
    });

    let mut rng: u32 = 0xc4e5_7a1b;
    let (mut placed, mut attempts) = (0u32, 0u32);
    while placed < CHEST_COUNT && attempts < CHEST_COUNT * 500 + 1000 {
        attempts += 1;
        let x = crate::wildlife::rng_range(&mut rng, -worldmap::GX + 6.0, worldmap::GX - 6.0);
        let z = crate::wildlife::rng_range(&mut rng, -worldmap::GZ + 6.0, worldmap::GZ - 6.0);
        // Keep chests out of the courtyard and off water / blockers / camps.
        if (x * x + z * z).sqrt() < 14.0
            || worldmap::ground_at_world(x, z).is_none()
            || crate::blockers::is_blocked(x, z)
            || crate::camps::in_clearing(x, z)
            || crate::castle::in_footprint(x, z)
        {
            continue;
        }
        let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
        let cache = placed % 2 == 0;
        let factor = forest_frontier(x, z);
        let parent = commands
            .spawn((
                Transform::from_xyz(x, y, z),
                Visibility::Visible,
                Chest { cache, opened: false, opened_at: 0.0, factor },
                ChestId(placed as usize),
                crate::biome::BiomeEntity,
            ))
            .id();
        commands.entity(parent).with_children(|p| {
            p.spawn((Mesh3d(base_mesh.clone()), MeshMaterial3d(chest_mat.clone()), Transform::default()));
            // Lid pivots on the back-top edge (its entity origin), so the open-tilt swings the
            // front up and back like a real hinge.
            p.spawn((
                Mesh3d(lid_mesh.clone()),
                MeshMaterial3d(chest_mat.clone()),
                Transform::from_xyz(0.0, 0.40, -0.25),
                ChestLid,
            ));
        });
        placed += 1;
    }
}

// ─── Chest model (vertex-coloured, flat-shaded) ─────────────────────────────────────
//
// Authored in parent-local space with the body resting on the ground (bottom at y=0). The lid
// is a SEPARATE child whose entity origin sits on the back-top hinge edge — `chest_lid_mesh`
// builds the lid forward of that origin (+Z = front) so the hinge rotation reads naturally.

const CW_BODY: u32 = 0x6b4a2a; // chest wood (body)
const CW_LID: u32 = 0x7a5530; // chest wood (lid — lighter)
const CW_IRON: u32 = 0x2c2c34; // dark iron straps / bands
const CW_BRASS: u32 = 0xc9962f; // brass lock + clasp
const CW_FOOT: u32 = 0x3a2818; // dark stub feet

/// Strapped wooden body: a box with iron side-bands, a brass lock plate, a top rim and four
/// stub feet. Bottom sits at local y=0 so it rests on the ground.
fn chest_body_mesh() -> Mesh {
    let iron = lin(CW_IRON);
    cgroup(vec![
        cbx(0.70, 0.40, 0.50, v(0.0, 0.20, 0.0), lin(CW_BODY)), // body
        cbx(0.74, 0.05, 0.54, v(0.0, 0.40, 0.0), iron),         // top rim band
        cbx(0.07, 0.42, 0.54, v(-0.24, 0.20, 0.0), iron),       // left strap (wraps front↔back)
        cbx(0.07, 0.42, 0.54, v(0.24, 0.20, 0.0), iron),        // right strap
        cbx(0.18, 0.20, 0.04, v(0.0, 0.27, 0.26), lin(CW_BRASS)), // front lock plate
        cbx(0.04, 0.07, 0.06, v(0.0, 0.25, 0.28), iron),        // keyhole
        cbx(0.10, 0.10, 0.10, v(-0.28, 0.05, 0.18), lin(CW_FOOT)), // feet
        cbx(0.10, 0.10, 0.10, v(0.28, 0.05, 0.18), lin(CW_FOOT)),
        cbx(0.10, 0.10, 0.10, v(-0.28, 0.05, -0.18), lin(CW_FOOT)),
        cbx(0.10, 0.10, 0.10, v(0.28, 0.05, -0.18), lin(CW_FOOT)),
    ])
}

/// Banded plank lid: a flat wooden slab with a slightly raised centre crown, two iron straps
/// running front↔back (aligned with the body's), and a brass clasp at the front that meets the
/// lock when closed. Built around the hinge origin with its underside on local y=0 and the body
/// forward (+Z), so it rests flush on the chest top and swings up cleanly on the back-edge hinge.
fn chest_lid_mesh() -> Mesh {
    let iron = lin(CW_IRON);
    cgroup(vec![
        cbx(0.72, 0.12, 0.52, v(0.0, 0.06, 0.25), lin(CW_LID)),    // lid slab — underside flush at y=0
        cbx(0.52, 0.09, 0.34, v(0.0, 0.155, 0.25), lin(CW_LID)),   // raised centre crown
        cbx(0.07, 0.18, 0.56, v(-0.24, 0.09, 0.25), iron),         // left strap (over the lid)
        cbx(0.07, 0.18, 0.56, v(0.24, 0.09, 0.25), iron),          // right strap
        cbx(0.14, 0.12, 0.06, v(0.0, -0.04, 0.50), lin(CW_BRASS)), // front clasp (drops to meet the lock)
    ])
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

/// A tight cluster of upward crystal shards (varied size + tilt) — the glowing mineable core.
/// Raised to crown the blocky rock so the gems stay visible above the tumbled stone blocks.
fn ore_crystal_mesh() -> Mesh {
    cgroup(vec![
        ccone(0.11, 0.48, v(0.0, 0.58, 0.0), Quat::IDENTITY),
        ccone(0.08, 0.32, v(0.16, 0.50, 0.07), rz(-0.45)),
        ccone(0.08, 0.34, v(-0.15, 0.51, -0.06), rz(0.5)),
        ccone(0.07, 0.28, v(0.04, 0.48, 0.18), rx(0.5)),
        ccone(0.06, 0.26, v(-0.06, 0.48, -0.17), rx(-0.5)),
    ])
}

/// Small standout apple tree: a gnarled orchard trunk forking into three limbs, a full
/// three-tone canopy (shadowed underside → orchard green → sunlit top, brighter than the
/// forest's pines/broadleaf so it reads as special) dusted with pale blossom, and a root
/// flare gripping the grass. Trunk base at y=0. The canopy covers every `APPLE_SPOTS`
/// hang point (x ±0.4, y 0.74–1.22) — the apples are separate child entities that pop off.
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

/// Gatherable swamp bramble: a squat dark-green leaf mound dotted with near-black blackberries
/// + a couple ripe red ones. Vertex-coloured (shares the white prop material). Base at y=0.
fn bramble_mesh() -> Mesh {
    let leaf = lin(0x3f6e30);
    let leaf_dk = lin(0x2b4d22);
    let berry = lin(0x271338); // blackberry — near-black purple
    let ripe = lin(0x5a1230); // a few unripe-red
    cgroup(vec![
        // Low leafy mound.
        csph(0.20, v(0.0, 0.15, 0.0), leaf),
        csph(0.15, v(0.16, 0.12, 0.06), leaf_dk),
        csph(0.15, v(-0.14, 0.13, -0.05), leaf_dk),
        csph(0.13, v(0.04, 0.25, -0.10), leaf),
        csph(0.12, v(-0.06, 0.21, 0.14), leaf),
        // Berries clustered over the crown.
        csph(0.055, v(0.10, 0.29, 0.05), berry),
        csph(0.05, v(-0.08, 0.27, -0.02), berry),
        csph(0.055, v(0.02, 0.33, -0.06), ripe),
        csph(0.045, v(0.15, 0.19, 0.12), berry),
        csph(0.05, v(-0.12, 0.23, 0.10), ripe),
    ])
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
}

/// Roll an animal's drops (primary + secondary + a frontier-graded bonus gear roll) and spawn
/// a floating mote for each, scattered around the kill point.
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
        // Frontier bonus: a small, distance-graded chance to also drop a gear piece.
        let f = forest_frontier(k.at.x, k.at.z);
        if rng.unit() < 0.1 + 0.35 * f {
            drops.push(frontier::roll_gear(f, rng.unit()));
        }
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
