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
                    forage_pickup,
                    forage_respawn,
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
    mut swings: MessageReader<HeroSwing>,
    mut bank: ResMut<Bank>,
    mut commands: Commands,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
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
            if shattered {
                bank.0.add_stone(node.ore.stone_reward);
                floats.0.push(crate::combat_fx::FloatReq {
                    world: head,
                    text: format!("+{} stone", node.ore.stone_reward as i64),
                    color: Color::srgb(0.82, 0.82, 0.88),
                    scale: 1.1,
                });
                cues.write(AudioCue::Impact { kill: true });
                commands.entity(e).despawn();
            } else {
                cues.write(AudioCue::Impact { kill: false });
            }
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
    let mesh = meshes.add(Sphere::new(0.45).mesh().ico(2).unwrap());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.40, 0.39, 0.42),
        perceptual_roughness: 0.96,
        metallic: 0.12,
        emissive: LinearRgba::rgb(0.05, 0.06, 0.09), // faint ore-vein sheen
        ..default()
    });

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
        // Sink the boulder slightly so it reads as embedded in the ground.
        let scale = crate::wildlife::rng_range(&mut rng, 0.8, 1.25);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, y + 0.25 * scale, z).with_scale(Vec3::splat(scale)),
            OreNode { ore },
            crate::biome::BiomeEntity,
        ));
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
            cues.write(AudioCue::UiSelect);
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
/// `worldmap::build`). Herb = a green sprig, apple = a small red fruit on the ground.
pub fn populate_forage(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let herb_mesh = meshes.add(Sphere::new(0.16).mesh().ico(2).unwrap());
    let herb_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.62, 0.28),
        emissive: LinearRgba::rgb(0.05, 0.14, 0.04),
        perceptual_roughness: 0.85,
        ..default()
    });
    let apple_mesh = meshes.add(Sphere::new(0.15).mesh().ico(2).unwrap());
    let apple_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.17, 0.15),
        emissive: LinearRgba::rgb(0.16, 0.02, 0.02),
        perceptual_roughness: 0.5,
        ..default()
    });

    seed_forage(commands, &herb_mesh, &herb_mat, "marsh_herb", 0.9, crate::biome::Biome::Swamp, 13, 0x4e_b5_1c_0d);
    seed_forage(commands, &apple_mesh, &apple_mat, "apple", 1.0, crate::biome::Biome::Forest, 14, 0xa9_71_3f_55);
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
            Transform::from_xyz(x, y + 0.14, z),
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
struct Chest {
    /// true = repeatable supply cache (gold + food), false = one-shot treasure (gear).
    cache: bool,
    opened: bool,
    opened_at: f32,
    /// Frontier factor at placement → loot tier.
    factor: f64,
}

/// The hinged lid (a child of the chest) — rotated open on loot, closed on a cache respawn.
#[derive(Component)]
struct ChestLid;

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
        cues.write(AudioCue::UiSelect);
        chest.opened = true;
        chest.opened_at = now;
        for &c in children {
            if let Ok(mut lt) = lids.get_mut(c) {
                lt.rotation = Quat::from_rotation_x(-1.2); // hinge open
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
    let base_mesh = meshes.add(Cuboid::new(0.72, 0.44, 0.52).mesh().build());
    let lid_mesh = meshes.add(Cuboid::new(0.74, 0.16, 0.54).mesh().build());
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.30, 0.16),
        perceptual_roughness: 0.8,
        ..default()
    });
    let lid_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.52, 0.36, 0.20),
        emissive: LinearRgba::rgb(0.12, 0.08, 0.01), // faint gold trim
        perceptual_roughness: 0.7,
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
                Transform::from_xyz(x, y + 0.22, z),
                Visibility::Visible,
                Chest { cache, opened: false, opened_at: 0.0, factor },
                crate::biome::BiomeEntity,
            ))
            .id();
        commands.entity(parent).with_children(|p| {
            p.spawn((Mesh3d(base_mesh.clone()), MeshMaterial3d(wood.clone()), Transform::default()));
            p.spawn((
                Mesh3d(lid_mesh.clone()),
                MeshMaterial3d(lid_mat.clone()),
                Transform::from_xyz(0.0, 0.30, 0.0),
                ChestLid,
            ));
        });
        placed += 1;
    }
}

// ─── Hunting: per-species drops + ground pickups ───────────────────────────────────
//
// On a wild-animal kill (`AnimalKilled`, published by combat) we roll its config drop(s) +
// a frontier-graded bonus, spawning floating loot motes the hero walks over to bag. HP and
// bounty come from [`animal_profile`], a forest-native rescale of core's `animal_config`.

/// Rescale factor from core's TS-anchored animal HP into forest's 60-HP combat units.
const ANIMAL_SCALE: f64 = 0.236;

/// A wild animal's forest combat profile: rescaled HP + (HP-independent) bounty + loot drops.
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
fn core_species(s: Species) -> Option<tileworld_core::animal::Species> {
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
        Species::Camel | Species::Cat => return None,
    })
}

/// Forest combat profile for a species — core stats rescaled, drops/bounty kept verbatim
/// (bounty is HP-independent per the parity brief). Camel/Cat are hand-authored (no core entry).
pub fn animal_profile(s: Species) -> AnimalProfile {
    if let Some(cs) = core_species(s) {
        let c = tileworld_core::animal::animal_config(cs);
        AnimalProfile {
            hp: ((c.hp * ANIMAL_SCALE).round() as f32).max(2.0),
            gold: c.bounty_gold as i64,
            xp: c.bounty_xp as i64,
            drop: c.drop_item.map(|id| (id, c.drop_chance)),
            drop2: c.drop_item2.map(|id| (id, c.drop_chance2)),
        }
    } else {
        match s {
            Species::Camel => AnimalProfile { hp: 12.0, gold: 8, xp: 12, drop: None, drop2: None },
            _ /* Cat */ => AnimalProfile { hp: 2.0, gold: 2, xp: 3, drop: None, drop2: None },
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
