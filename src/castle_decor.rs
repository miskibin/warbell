//! **Courtyard set dressing + upgrade trophies.** The castle is the first thing the player
//! sees, so day one it's already a lived-in bailey: notice board, water trough, market goods,
//! benches, lantern posts — and as the town grows, laundry lines, kitchen gardens and
//! woodpiles fill in beside the dwellings. On top of that, War-Table purchases plant REAL set
//! pieces in the yard: Town Guard Arms raises an armory corner, the Tax Office opens a counting
//! booth, the Healing Shrine gets an actual shrine, Reinforced Keep scaffolds the keep wall,
//! Tower Mastery lights standing braziers by the towers, the Arsenal unlocks go up on display
//! stands. The upgrade tree becomes visible history — you can read a run's purchases off the
//! courtyard.
//!
//! All parts follow the castle's conventions: built from [`crate::castle::Mats`]'s shared
//! textured materials at local origin (base y = 0, front +Z), baked to a world spot by
//! [`build`], and revealed by [`sync_decor`] off the live [`Defenses`] / [`EconomyState`] /
//! [`Upgrades`] / town state (flag-driven, so staged `FOREST_DEFEND` shots and loaded saves
//! both show the right dressing). Decorative only — NO collision (blockers are append-only),
//! and every spot is hand-picked clear of the keep, bell, gate lanes, torches, training
//! dummies, house slots and the path network.

use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::castle::{bake, bx, cyl, flat, gable, log_x, taper, Mats, M};
use crate::economy::{Defenses, EconomyState, Upgrades};

const HALF_PI: f32 = std::f32::consts::FRAC_PI_2;

/// What reveals a piece of set dressing.
#[derive(Clone, Copy)]
pub enum DecorGate {
    /// Day-one life — always shown.
    Always,
    /// Shown once the town holds more than `n` houses (dressing that fills in beside homes).
    House(u32),
    /// Town Guard Arms tier (1 = the armory corner, 2 = its veteran extension).
    ArmsTier(u32),
    /// Tower Mastery — standing braziers by the corner towers.
    TowerMastery,
    /// Reinforced Keep — mason's scaffold + stone against the keep wall.
    Reinforced,
    /// Healing Shrine — the shrine itself (the heal aura finally has a body).
    Shrine,
    /// Tax Office — the counting booth.
    TaxOffice,
    /// Merchant Guild — banner + extra goods at the merchant stall.
    Guild,
    /// An Arsenal weapon unlock, by shop id ("axe", "sword_gold") — its display stand.
    Weapon(&'static str),
    /// Any other purchased node by id (e.g. "eco_bounty", "hero_dmg_1").
    Purchased(&'static str),
}

#[derive(Component)]
pub struct Decor {
    gate: DecorGate,
}

pub struct CastleDecorPlugin;

impl Plugin for CastleDecorPlugin {
    fn build(&self, app: &mut App) {
        // Render-side reveal (like castle::sync_castle): ungated, so a loaded save or a staged
        // screenshot shows the right dressing even while the sim is frozen.
        app.add_systems(Update, sync_decor);
    }
}

/// Reveal each piece off the live flags. Defense-branch pieces key on the [`Defenses`] flags
/// (not purchase records) so `FOREST_DEFEND` staging and saves light them correctly.
fn sync_decor(
    up: Res<Upgrades>,
    def: Res<Defenses>,
    eco: Res<EconomyState>,
    town: Res<crate::town::TownRes>,
    mut q: Query<(&Decor, &mut Visibility)>,
) {
    for (d, mut vis) in &mut q {
        let show = match d.gate {
            DecorGate::Always => true,
            DecorGate::House(n) => town.0.houses > n,
            DecorGate::ArmsTier(n) => def.villager_arms_tier >= n,
            DecorGate::TowerMastery => def.tower_mastery,
            DecorGate::Reinforced => def.reinforced,
            DecorGate::Shrine => def.shrine,
            DecorGate::TaxOffice => eco.tax_office,
            DecorGate::Guild => eco.shop_discount < 0.999,
            DecorGate::Weapon(id) => eco.unlocked_weapons.contains(&id),
            DecorGate::Purchased(id) => up.0.is_purchased(id),
        };
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
}

// ── Spawn ────────────────────────────────────────────────────────────────────────

/// Bake + spawn every dressing set. Called from `castle::build` (same shared materials).
pub fn build(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Mats) {
    let mut set = |parts: Vec<(Mesh, M)>, pos: Vec3, yaw: f32, gate: DecorGate| {
        let vis = if matches!(gate, DecorGate::Always) { Visibility::Inherited } else { Visibility::Hidden };
        for (m, slot) in parts {
            commands.spawn((
                Mesh3d(meshes.add(bake(m, pos, yaw, Vec3::ONE))),
                MeshMaterial3d(mats.get(slot)),
                Transform::default(),
                vis,
                Decor { gate },
                BiomeEntity,
            ));
        }
    };

    // ── Day-one life (Always) ────────────────────────────────────────────────────
    set(notice_board_parts(), Vec3::new(-3.3, 0.0, 4.4), 0.35, DecorGate::Always);
    set(trough_parts(), Vec3::new(3.6, 0.0, 4.7), 0.25, DecorGate::Always);
    set(market_parts(), Vec3::new(5.3, 0.0, -2.9), 0.7, DecorGate::Always);
    set(bench_parts(), Vec3::new(-5.9, 0.0, -1.4), HALF_PI, DecorGate::Always);
    set(bench_parts(), Vec3::new(2.6, 0.0, 9.6), 2.8, DecorGate::Always);
    for (lx, lz, lyaw) in [
        (2.0, -6.6, 0.0),
        (-2.0, -9.6, std::f32::consts::PI),
        (2.1, 10.4, std::f32::consts::PI),
        (-9.2, 2.0, -HALF_PI),
        (9.2, -2.0, HALF_PI),
    ] {
        set(lantern_parts(), Vec3::new(lx, 0.0, lz), lyaw, DecorGate::Always);
    }

    // ── Filled in as the town grows (House(n): shown once houses > n) ───────────
    set(garden_parts(), Vec3::new(-4.2, 0.0, -9.4), 0.15, DecorGate::House(1));
    set(woodpile_parts(), Vec3::new(5.4, 0.0, -9.5), 0.4, DecorGate::House(2));
    set(laundry_parts(), Vec3::new(-10.0, 0.0, -10.2), 0.05, DecorGate::House(1));
    set(garden_parts(), Vec3::new(4.2, 0.0, 9.4), -0.2, DecorGate::House(6));
    set(laundry_parts(), Vec3::new(10.0, 0.0, 10.2), -0.08, DecorGate::House(7));

    // ── Upgrade set pieces ───────────────────────────────────────────────────────
    // The armory corner west of the plaza: racked spears + shields + a leather stand (tier 1),
    // extended with steel — sword rail + iron stand (tier 2). Arsenal unlocks join on display.
    set(armory_parts(), Vec3::new(-8.6, 0.0, 3.0), 0.5, DecorGate::ArmsTier(1));
    set(armory_veteran_parts(), Vec3::new(-8.2, 0.0, 4.9), 0.8, DecorGate::ArmsTier(2));
    set(axe_display_parts(), Vec3::new(-6.9, 0.0, 6.3), 0.9, DecorGate::Weapon("axe"));
    set(sword_display_parts(), Vec3::new(-7.9, 0.0, 7.6), 1.1, DecorGate::Weapon("sword_gold"));
    set(grindstone_parts(), Vec3::new(-5.6, 0.0, 8.4), 0.8, DecorGate::Purchased("hero_dmg_1"));

    // Civic east side: the tax-collector's counting booth; the shrine the heal aura lives in.
    set(tax_booth_parts(), Vec3::new(8.5, 0.0, 2.9), -0.6, DecorGate::TaxOffice);
    set(shrine_parts(), Vec3::new(8.0, 0.0, -3.4), -0.8, DecorGate::Shrine);

    // Bounty board inside the north gate — wanted papers for the ork warlords.
    set(bounty_board_parts(), Vec3::new(3.4, 0.0, -10.4), 0.1, DecorGate::Purchased("eco_bounty"));

    // Reinforced Keep: a mason's scaffold against the keep's west wall + dressed stone waiting.
    set(scaffold_parts(), Vec3::new(-3.55, 0.0, 0.0), 0.0, DecorGate::Reinforced);
    set(stone_pile_parts(), Vec3::new(-4.8, 0.0, 1.3), 0.3, DecorGate::Reinforced);

    // Tower Mastery: a standing fire basket inside each wall corner (the crews work all night).
    for (sx, sz) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        set(brazier_parts(), Vec3::new(sx * 15.6, 0.0, sz * 10.6), sx * 0.4, DecorGate::TowerMastery);
    }

    // Merchant Guild: banner + stacked goods dressing the wandering merchant's stall (outside
    // the north wall — snap to the terrain there, the bailey's flat y=0 doesn't reach it).
    let stall_y = |x: f32, z: f32| crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
    set(guild_banner_parts(), Vec3::new(1.3, stall_y(1.3, -16.2), -16.2), 0.2, DecorGate::Guild);
    set(guild_goods_parts(), Vec3::new(3.8, stall_y(3.8, -16.6), -16.6), -0.5, DecorGate::Guild);

    // Firelight: flickering pools for the night-burning pieces (the meshes' emissive alone
    // reads flat in the dark). Same pooled flicker as the wall torches.
    for (i, (sx, sz)) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].into_iter().enumerate() {
        commands.spawn((
            Transform::from_xyz(sx * 15.6, 2.2, sz * 10.6),
            Visibility::Hidden,
            Decor { gate: DecorGate::TowerMastery },
            BiomeEntity,
            crate::firelight::torch_light(i as f32 * 1.9 + 0.7),
        ));
    }
    commands.spawn((
        Transform::from_xyz(8.0, 1.1, -3.4),
        Visibility::Hidden,
        Decor { gate: DecorGate::Shrine },
        BiomeEntity,
        crate::firelight::torch_light(4.2),
    ));
}

// ── Day-one parts ────────────────────────────────────────────────────────────────

/// Village notice board: two posts, a plank board under a little shingle cap, papers pinned.
fn notice_board_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for sx in [-0.55_f32, 0.55] {
        v.push((bx(0.1, 1.35, 0.1, sx, 0.68, 0.0), M::Beam));
    }
    v.push((bx(1.3, 0.62, 0.07, 0.0, 1.0, 0.0), M::Wood));
    v.push((gable(1.5, 0.34, 0.16, 1.34), M::HouseRoof2));
    v.push((bx(0.2, 0.26, 0.02, -0.32, 1.02, 0.05), M::Parchment));
    v.push((bx(0.22, 0.2, 0.02, 0.05, 0.98, 0.05), M::Parchment));
    v.push((bx(0.18, 0.24, 0.02, 0.38, 1.04, 0.05), M::Parchment));
    v
}

/// Water trough on timber feet, dark water inside (the horses of the bailey drink somewhere).
fn trough_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(1.1, 0.32, 0.46, 0.0, 0.28, 0.0), M::Wood),
        (bx(0.98, 0.04, 0.34, 0.0, 0.42, 0.0), M::Slit), // still water
        (bx(0.12, 0.14, 0.5, -0.42, 0.07, 0.0), M::Beam),
        (bx(0.12, 0.14, 0.5, 0.42, 0.07, 0.0), M::Beam),
    ]
}

/// Market goods by the plaza: stacked crates, a hooped barrel, grain sacks, a crop basket.
fn market_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(0.5, 0.5, 0.5, 0.0, 0.25, 0.0), M::Wood)); // crate
    v.push((bx(0.44, 0.44, 0.44, 0.04, 0.72, -0.04), M::Wood)); // crate on top
    v.push((bake(bx(0.48, 0.48, 0.48, 0.0, 0.24, 0.0), Vec3::new(0.75, 0.0, 0.35), 0.6, Vec3::ONE), M::Wood));
    v.push((taper(0.24, 0.28, 0.6, 0.3).translated_by(Vec3::new(-0.65, 0.0, 0.3)), M::Wood)); // barrel
    v.push((cyl(0.29, 0.05, -0.65, 0.15, 0.3), M::Beam)); // hoops
    v.push((cyl(0.29, 0.05, -0.65, 0.46, 0.3), M::Beam));
    for (sx, sz) in [(-0.2_f32, 0.7_f32), (0.25, 0.85)] {
        v.push((taper(0.11, 0.2, 0.4, 0.2).translated_by(Vec3::new(sx, 0.0, sz)), M::Plaster)); // sacks
    }
    v.push((cyl(0.22, 0.16, 0.85, 0.08, -0.4), M::Beam)); // basket
    v.push((bx(0.3, 0.12, 0.3, 0.85, 0.2, -0.4), M::Crop)); // greens in it
    v
}

/// A plank bench on two block legs.
fn bench_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(1.2, 0.07, 0.34, 0.0, 0.42, 0.0), M::Wood),
        (bx(0.1, 0.4, 0.3, -0.45, 0.2, 0.0), M::Beam),
        (bx(0.1, 0.4, 0.3, 0.45, 0.2, 0.0), M::Beam),
    ]
}

/// Lantern post: post + arm + a warm lamp box (shares the window material, so `window_glow`
/// brightens every lantern at dusk for free — the paths light up with the houses).
fn lantern_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(0.09, 1.55, 0.09, 0.0, 0.78, 0.0), M::Beam),
        (bx(0.34, 0.06, 0.06, 0.13, 1.52, 0.0), M::Beam),
        (bx(0.15, 0.2, 0.15, 0.28, 1.36, 0.0), M::Window),
        (bx(0.19, 0.04, 0.19, 0.28, 1.48, 0.0), M::BronzeDark),
    ]
}

/// A kitchen garden: small tilled bed, two crop rows, three picket stakes.
fn garden_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = vec![(bx(1.15, 0.1, 0.9, 0.0, 0.05, 0.0), M::Soil)];
    for dz in [-0.2_f32, 0.2] {
        v.push((bx(0.85, 0.2, 0.14, 0.0, 0.16, dz), M::Crop));
    }
    for sx in [-0.5_f32, 0.0, 0.5] {
        v.push((bx(0.06, 0.34, 0.06, sx, 0.17, 0.52), M::Beam));
    }
    v
}

/// Firewood stacked between two stakes (the loose version of the longhouse's gable stack).
fn woodpile_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for (row, &(y, n)) in [(0.12_f32, 3i32), (0.34, 2)].iter().enumerate() {
        for k in 0..n {
            let z = (k as f32 - (n - 1) as f32 / 2.0) * 0.26;
            let m = if (row + k as usize) % 2 == 0 { M::Wood } else { M::Beam };
            v.push((log_x(0.11, 1.1, y, z), m));
        }
    }
    for sx in [-0.62_f32, 0.62] {
        v.push((bx(0.07, 0.62, 0.07, sx, 0.31, 0.0), M::Beam));
    }
    v
}

/// Laundry line: two posts, a taut line, cloth squares drying in the courtyard wind.
fn laundry_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for sx in [-1.3_f32, 1.3] {
        v.push((bx(0.08, 1.42, 0.08, sx, 0.71, 0.0), M::Beam));
    }
    v.push((bx(2.6, 0.025, 0.025, 0.0, 1.36, 0.0), M::Beam)); // the line
    for (sx, w, h, m) in [
        (-0.8_f32, 0.42_f32, 0.5_f32, M::Plaster),
        (-0.15, 0.36, 0.42, M::Banner),
        (0.45, 0.4, 0.36, M::Hen),
        (0.95, 0.3, 0.46, M::Plaster),
    ] {
        v.push((bx(w, h, 0.02, sx, 1.34 - h / 2.0, 0.0), m));
    }
    v
}

// ── Upgrade set pieces ───────────────────────────────────────────────────────────

/// Town Guard Arms: a weapon rack of spears with round shields at its feet, and a padded
/// leather armor stand — the guards' kit lives somewhere now.
fn armory_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    // Rack frame.
    for sx in [-0.55_f32, 0.55] {
        v.push((bx(0.09, 1.1, 0.09, sx, 0.55, 0.0), M::Beam));
    }
    v.push((bx(1.3, 0.08, 0.08, 0.0, 1.05, 0.0), M::Beam));
    v.push((bx(1.3, 0.08, 0.08, 0.0, 0.3, 0.06), M::Beam));
    // Three spears leaning in the rack: shaft + iron tip.
    for (i, sx) in [-0.34_f32, 0.0, 0.34].into_iter().enumerate() {
        let lean = 0.1 - i as f32 * 0.08;
        v.push((
            bx(0.05, 1.45, 0.05, 0.0, 0.72, 0.0)
                .rotated_by(Quat::from_rotation_z(lean))
                .translated_by(Vec3::new(sx, 0.0, 0.04)),
            M::Wood,
        ));
        v.push((
            bx(0.07, 0.2, 0.03, 0.0, 1.5, 0.0)
                .rotated_by(Quat::from_rotation_z(lean))
                .translated_by(Vec3::new(sx, 0.0, 0.04)),
            M::Iron,
        ));
    }
    // Two round shields leaning at the rack's feet.
    for (sx, tilt) in [(-0.42_f32, 0.18_f32), (0.38, -0.12)] {
        v.push((
            flat(
                Cylinder::new(0.27, 0.05)
                    .mesh()
                    .resolution(12)
                    .build()
                    .rotated_by(Quat::from_rotation_x(HALF_PI + tilt))
                    .translated_by(Vec3::new(sx, 0.28, 0.3)),
            ),
            M::Wood,
        ));
        v.push((
            flat(
                Cylinder::new(0.07, 0.08)
                    .mesh()
                    .resolution(10)
                    .build()
                    .rotated_by(Quat::from_rotation_x(HALF_PI + tilt))
                    .translated_by(Vec3::new(sx, 0.28, 0.33)),
            ),
            M::Bronze,
        ));
    }
    // Leather armor stand: post, crossarm, padded torso, plain helm.
    let ax = 1.15;
    v.push((bx(0.08, 1.05, 0.08, ax, 0.52, 0.0), M::Beam));
    v.push((bx(0.66, 0.07, 0.07, ax, 0.8, 0.0), M::Beam));
    v.push((taper(0.17, 0.23, 0.5, 0.55).translated_by(Vec3::new(ax, 0.0, 0.0)), M::Wood));
    v.push((bx(0.18, 0.16, 0.18, ax, 1.14, 0.0), M::Iron));
    v
}

/// Veteran Guard: the armory's steel extension — a rail of upright iron swords and a
/// full steel stand (iron torso + crested helm), plus a shield row against it.
fn armory_veteran_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    // Sword rail.
    for sx in [-0.5_f32, 0.5] {
        v.push((bx(0.09, 0.85, 0.09, sx, 0.42, 0.0), M::Beam));
    }
    v.push((bx(1.2, 0.09, 0.12, 0.0, 0.82, 0.0), M::Beam));
    for sx in [-0.3_f32, 0.0, 0.3] {
        v.push((bx(0.06, 0.62, 0.025, sx, 0.5, 0.0), M::Iron)); // blade
        v.push((bx(0.16, 0.04, 0.05, sx, 0.84, 0.0), M::BronzeDark)); // guard
        v.push((bx(0.045, 0.14, 0.045, sx, 0.93, 0.0), M::Wood)); // grip
    }
    // Steel armor stand.
    let ax = 1.05;
    v.push((bx(0.08, 1.1, 0.08, ax, 0.55, 0.0), M::Beam));
    v.push((bx(0.66, 0.07, 0.07, ax, 0.84, 0.0), M::Beam));
    v.push((taper(0.17, 0.23, 0.52, 0.58).translated_by(Vec3::new(ax, 0.0, 0.0)), M::Iron));
    v.push((bx(0.18, 0.17, 0.18, ax, 1.2, 0.0), M::Iron));
    v.push((bx(0.04, 0.12, 0.16, ax, 1.32, 0.0), M::Banner)); // crest
    v
}

/// Display stand for the unlocked Battle Axe: a dark plinth, the haft crossed on pegs, a
/// heavy iron head — the Arsenal purchase made flesh.
fn axe_display_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(0.55, 0.14, 0.4, 0.0, 0.07, 0.0), M::DarkStone),
        (bx(0.1, 0.9, 0.1, 0.0, 0.59, -0.08), M::Beam),
        (
            bx(0.07, 1.1, 0.07, 0.0, 0.55, 0.0)
                .rotated_by(Quat::from_rotation_z(0.5))
                .translated_by(Vec3::new(0.0, 0.18, 0.06)),
            M::Wood,
        ),
        (
            bx(0.34, 0.26, 0.06, 0.0, 1.05, 0.0)
                .rotated_by(Quat::from_rotation_z(0.5))
                .translated_by(Vec3::new(0.0, 0.18, 0.06)),
            M::Iron,
        ),
    ]
}

/// Display stand for the unlocked Golden Blade: upright on a plinth, catching the light.
fn sword_display_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(0.5, 0.14, 0.4, 0.0, 0.07, 0.0), M::DarkStone),
        (bx(0.3, 0.1, 0.3, 0.0, 0.19, 0.0), M::LightStone),
        (bx(0.09, 0.85, 0.035, 0.0, 0.72, 0.0), M::Gold), // blade, point up
        (bx(0.3, 0.06, 0.07, 0.0, 0.32, 0.0), M::Bronze), // guard
        (bx(0.05, 0.16, 0.05, 0.0, 0.21, 0.0), M::Wood),  // grip (into the plinth socket)
    ]
}

/// Sharpening wheel at the muster yard: treadle frame, big stone wheel, water bucket.
fn grindstone_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for sz in [-0.22_f32, 0.22] {
        v.push((bx(0.5, 0.5, 0.09, 0.0, 0.25, sz), M::Beam));
    }
    v.push((
        flat(
            Cylinder::new(0.34, 0.12)
                .mesh()
                .resolution(14)
                .build()
                .rotated_by(Quat::from_rotation_x(HALF_PI))
                .translated_by(Vec3::new(0.0, 0.55, 0.0)),
        ),
        M::LightStone,
    ));
    v.push((log_x(0.04, 0.6, 0.55, 0.0).rotated_by(Quat::from_rotation_y(HALF_PI)), M::Beam)); // axle
    v.push((bx(0.3, 0.05, 0.09, 0.0, 0.62, 0.34), M::Wood)); // tool rest
    v.push((taper(0.12, 0.15, 0.24, 0.12).translated_by(Vec3::new(0.5, 0.0, 0.2)), M::Wood)); // bucket
    v
}

/// The Tax Office: a roofed counting booth — counter, strongbox with iron bands and a gold
/// latch, coin stacks, a ledger, a hanging coin sign.
fn tax_booth_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for (sx, sz) in [(-0.65_f32, -0.35_f32), (0.65, -0.35), (-0.65, 0.35), (0.65, 0.35)] {
        v.push((bx(0.09, 1.5, 0.09, sx, 0.75, sz), M::Beam));
    }
    v.push((gable(1.7, 1.1, 0.4, 1.5), M::HouseRoof));
    v.push((bx(1.4, 0.55, 0.45, 0.0, 0.28, 0.3), M::Wood)); // counter
    v.push((bx(1.46, 0.06, 0.51, 0.0, 0.58, 0.3), M::Beam)); // counter top
    // Strongbox: banded chest with a gold latch.
    v.push((bx(0.36, 0.26, 0.26, -0.42, 0.74, 0.28), M::Wood));
    for dx in [-0.12_f32, 0.12] {
        v.push((bx(0.05, 0.28, 0.28, -0.42 + dx, 0.74, 0.28), M::Iron));
    }
    v.push((bx(0.07, 0.08, 0.04, -0.42, 0.76, 0.42), M::Gold));
    // Coin stacks + the ledger.
    v.push((cyl(0.05, 0.07, 0.05, 0.65, 0.32), M::Gold));
    v.push((cyl(0.05, 0.045, 0.18, 0.63, 0.24), M::Gold));
    v.push((bx(0.3, 0.04, 0.22, 0.42, 0.63, 0.3), M::Parchment));
    // Hanging coin sign off the front plate.
    v.push((bx(0.05, 0.3, 0.05, 0.55, 1.32, 0.52), M::Beam));
    v.push((bx(0.3, 0.26, 0.03, 0.55, 1.06, 0.52), M::Wood));
    v.push((cyl(0.08, 0.02, 0.55, 1.06, 0.55), M::Gold));
    v
}

/// The Healing Shrine made flesh: stepped stone base, a niched column holding a gold icon,
/// candles burning at its feet (a pooled flicker light joins it at night).
fn shrine_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((bx(0.95, 0.18, 0.75, 0.0, 0.09, 0.0), M::LightStone));
    v.push((bx(0.7, 0.16, 0.52, 0.0, 0.26, 0.0), M::Stone));
    v.push((bx(0.44, 0.66, 0.36, 0.0, 0.67, -0.04), M::LightStone)); // column
    v.push((bx(0.3, 0.34, 0.06, 0.0, 0.72, 0.14), M::Slit)); // the niche shadow
    v.push((bx(0.12, 0.22, 0.05, 0.0, 0.72, 0.16), M::Gold)); // icon
    v.push((gable(0.62, 0.5, 0.22, 1.0), M::HouseStone)); // stone cap
    for (cx, cz, ch) in [(-0.32_f32, 0.26_f32, 0.1_f32), (-0.18, 0.32, 0.16), (0.3, 0.28, 0.12)] {
        v.push((cyl(0.035, ch, cx, 0.18 + ch / 2.0, cz), M::Plaster)); // candles
        v.push((flat(Sphere::new(0.035).mesh().ico(1).unwrap().translated_by(Vec3::new(cx, 0.2 + ch, cz))), M::Flame));
    }
    v
}

/// Bounty board by the north gate: a heavier notice board papered with wanted notices, a
/// blood-red ribbon seal and a nailed-up coin pouch.
fn bounty_board_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for sx in [-0.65_f32, 0.65] {
        v.push((bx(0.12, 1.6, 0.12, sx, 0.8, 0.0), M::Beam));
    }
    v.push((bx(1.55, 0.8, 0.08, 0.0, 1.1, 0.0), M::Wood));
    v.push((gable(1.75, 0.4, 0.18, 1.56), M::HouseRoof2));
    for (px, py, w, h) in [
        (-0.5_f32, 1.12_f32, 0.26_f32, 0.34_f32),
        (-0.12, 1.05, 0.3, 0.4),
        (0.28, 1.16, 0.24, 0.3),
        (0.55, 1.0, 0.2, 0.26),
    ] {
        v.push((bx(w, h, 0.02, px, py, 0.06), M::Parchment));
    }
    v.push((bx(0.07, 0.16, 0.03, -0.12, 0.95, 0.07), M::Banner)); // red ribbon seal
    v.push((taper(0.05, 0.09, 0.16, 0.78).translated_by(Vec3::new(0.5, 0.0, 0.07)), M::Gold)); // the purse
    v
}

/// Mason's scaffold against the keep's west wall (Reinforced Keep) — poles, ledgers, a plank
/// deck half way up. Built local with the wall along +Z.
fn scaffold_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    for pz in [-1.6_f32, 0.0, 1.6] {
        v.push((bx(0.09, 2.2, 0.09, 0.0, 1.1, pz), M::Beam));
        v.push((bx(0.09, 2.2, 0.09, -0.7, 1.1, pz), M::Beam));
    }
    for ly in [0.9_f32, 1.78] {
        v.push((bx(0.07, 0.07, 3.5, 0.0, ly, 0.0), M::Beam));
        v.push((bx(0.07, 0.07, 3.5, -0.7, ly, 0.0), M::Beam));
        v.push((bx(0.8, 0.07, 0.07, -0.35, ly, -1.6), M::Beam));
        v.push((bx(0.8, 0.07, 0.07, -0.35, ly, 1.6), M::Beam));
    }
    v.push((bx(0.66, 0.06, 3.3, -0.35, 1.84, 0.0), M::Wood)); // deck
    v.push((bx(0.3, 0.22, 0.34, -0.35, 1.98, -0.6), M::Stone)); // block waiting on the deck
    v
}

/// Dressed stone stacked for the masons.
fn stone_pile_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(0.5, 0.34, 0.42, 0.0, 0.17, 0.0), M::Stone),
        (bx(0.46, 0.32, 0.4, 0.1, 0.17, 0.45), M::LightStone),
        (bx(0.42, 0.3, 0.38, 0.05, 0.49, 0.2), M::DarkStone),
    ]
}

/// A standing fire basket (Tower Mastery): tall post, iron basket, flame — one per wall corner.
fn brazier_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(0.3, 0.1, 0.3, 0.0, 0.05, 0.0), M::DarkStone),
        (bx(0.1, 1.85, 0.1, 0.0, 0.97, 0.0), M::Beam),
        (taper(0.24, 0.13, 0.26, 2.0), M::Iron),
        (cyl(0.25, 0.04, 0.0, 1.86, 0.0), M::BronzeDark),
        (
            flat(
                Sphere::new(0.16)
                    .mesh()
                    .ico(1)
                    .unwrap()
                    .scaled_by(Vec3::new(1.0, 1.5, 1.0))
                    .translated_by(Vec3::new(0.0, 2.2, 0.0)),
            ),
            M::Flame,
        ),
    ]
}

/// Merchant Guild banner: a tall pole flying the guild cloth with gold trim, by the stall.
fn guild_banner_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(0.1, 2.3, 0.1, 0.0, 1.15, 0.0), M::Beam),
        (bx(0.55, 0.07, 0.07, 0.24, 2.22, 0.0), M::Beam),
        (bx(0.42, 0.95, 0.03, 0.28, 1.72, 0.0), M::Banner),
        (bx(0.42, 0.08, 0.04, 0.28, 1.22, 0.0), M::Gold), // gold fringe
        (Sphere::new(0.07).mesh().ico(1).unwrap().translated_by(Vec3::new(0.0, 2.34, 0.0)), M::Gold),
    ]
}

/// Guild stock overflowing beside the stall: crates, a barrel, a bolt of dyed cloth.
fn guild_goods_parts() -> Vec<(Mesh, M)> {
    vec![
        (bx(0.5, 0.5, 0.5, 0.0, 0.25, 0.0), M::Wood),
        (bx(0.44, 0.42, 0.44, -0.05, 0.71, 0.05), M::Wood),
        (taper(0.22, 0.26, 0.55, 0.28).translated_by(Vec3::new(0.65, 0.0, -0.2)), M::Wood),
        (cyl(0.27, 0.05, 0.65, 0.14, -0.2), M::Beam),
        (cyl(0.27, 0.05, 0.65, 0.42, -0.2), M::Beam),
        (log_x(0.09, 0.7, 0.97, 0.05), M::Banner), // bolt of cloth across the crates
    ]
}
