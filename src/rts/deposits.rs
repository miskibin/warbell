//! Finite resource deposits on the arena: tree groves (wood), stone outcrops, gold veins.
//! Spawning, progressive depletion visuals, exhaustion (no regrow in Skirmish).
//!
//! Each deposit is ONE [`Deposit`] anchor entity (the thing `workers.rs` harvests) plus a set of
//! cosmetic **visual parts** ([`DepositVisuals`]): a wood grove is an invisible anchor ringed by
//! real tree entities; a stone/gold outcrop is an invisible anchor over a small cluster of faceted
//! boulders. As `remaining` falls, the parts are felled progressively (via the shared
//! `dying::begin_dying` topple-and-reap), and the boulders' nav blockers are lifted. At `0` the
//! last parts topple and the anchor is despawned — the site is spent for good (no regrow).

use std::f32::consts::TAU;

use bevy::prelude::*;

use crate::meshkit::{merged_flat, tinted};
use crate::palette::lin;
use crate::rts::{in_skirmish, Deposit, DepositKind};
use crate::trees::{build_tree_mesh, TreeKind};

/// How many units of resource a full site holds (contested-centre variant is richer).
const WOOD_REMAIN: f64 = 400.0;
const WOOD_REMAIN_MID: f64 = 800.0;
const STONE_REMAIN: f64 = 300.0;
const STONE_REMAIN_MID: f64 = 600.0;
const GOLD_REMAIN: f64 = 200.0;
const GOLD_REMAIN_MID: f64 = 400.0;

/// Tree count in a grove (contested-centre grove is denser).
const GROVE_TREES: usize = 7;
const GROVE_TREES_MID: usize = 12;
/// Boulders in a stone/gold cluster.
const CLUSTER_ROCKS: usize = 5;
const CLUSTER_ROCKS_MID: usize = 8;
/// Blocker radius registered under each boulder so units path around the cluster.
const ROCK_BLOCK_R: f32 = 0.9;

pub struct RtsDepositsPlugin;

impl Plugin for RtsDepositsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_arena_deposits.run_if(in_skirmish),
                spawn_arena_hills.run_if(in_skirmish),
                deplete_deposit_visuals
                    .run_if(in_skirmish)
                    .run_if(in_state(crate::game_state::Modal::None)),
            ),
        );
    }
}

/// Cosmetic parts belonging to a [`Deposit`] anchor, felled in order as the site depletes.
/// `parts[i].1` is the part's nav-blocker centre (rocks) or `None` (trees register none).
/// `pub(super)` so `workers.rs` can aim a harvester at a real standing tree/boulder
/// ([`nearest_standing_part`]) instead of the invisible anchor.
#[derive(Component)]
pub(super) struct DepositVisuals {
    parts: Vec<(Entity, Option<Vec2>)>,
    /// `remaining` at spawn — the denominator for the standing-fraction.
    start: f64,
    /// How many parts have already been felled (index into `parts`).
    felled: usize,
}

/// World XZ of the standing (not-yet-felled) visual part nearest `from` — the actual tree trunk /
/// boulder a worker should walk to and chop at. `None` if the site has no standing part (spent).
pub(super) fn nearest_standing_part(
    vis: &DepositVisuals,
    transforms: &Query<&Transform>,
    from: Vec2,
) -> Option<Vec2> {
    vis.parts[vis.felled.min(vis.parts.len())..]
        .iter()
        .filter_map(|(e, _)| transforms.get(*e).ok())
        .map(|t| Vec2::new(t.translation.x, t.translation.z))
        .min_by(|a, b| {
            a.distance_squared(from)
                .partial_cmp(&b.distance_squared(from))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Take up to `amount` from a deposit, returning what was actually extracted (clamped at the
/// remaining stock). The one depletion entry-point `workers.rs` calls each banked trip.
pub fn take(deposit: &mut Deposit, amount: f64) -> f64 {
    let got = amount.min(deposit.remaining.max(0.0));
    deposit.remaining -= got;
    got
}

/// After the arena world is up ([`crate::biome::WorldReady`], the same signal `build.rs` waits on),
/// seed the nine deposits at `worldmap::arena_sites()` — one-shot.
fn spawn_arena_deposits(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    ready: Res<crate::biome::WorldReady>,
    mut done: Local<bool>,
) {
    if *done || !ready.0 {
        return;
    }
    *done = true;

    let sites = crate::worldmap::arena_sites();
    // Colour lives in the mesh vertex COLOR (mesh contract), so one shared white material batches
    // every prop; trees keep their own scatter-matched white material.
    let rock_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        metallic: 0.1,
        ..default()
    });
    let tree_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.62,
        reflectance: 0.5,
        ..default()
    });
    let stone_mesh = meshes.add(boulder_mesh(false));
    let gold_mesh = meshes.add(boulder_mesh(true));

    let mut seed: u32 = 0x5115_bee5;
    for (i, site) in sites.wood.iter().enumerate() {
        let mid = i == 2;
        let remaining = if mid { WOOD_REMAIN_MID } else { WOOD_REMAIN };
        let trees = if mid { GROVE_TREES_MID } else { GROVE_TREES };
        spawn_grove(&mut commands, &mut meshes, &tree_mat, *site, remaining, trees, &mut seed);
    }
    for (i, site) in sites.stone.iter().enumerate() {
        let mid = i == 2;
        let remaining = if mid { STONE_REMAIN_MID } else { STONE_REMAIN };
        let rocks = if mid { CLUSTER_ROCKS_MID } else { CLUSTER_ROCKS };
        spawn_cluster(&mut commands, DepositKind::Stone, &stone_mesh, &rock_mat, *site, remaining, rocks, &mut seed);
    }
    for (i, site) in sites.gold.iter().enumerate() {
        let mid = i == 2;
        let remaining = if mid { GOLD_REMAIN_MID } else { GOLD_REMAIN };
        let rocks = if mid { CLUSTER_ROCKS_MID } else { CLUSTER_ROCKS };
        spawn_cluster(&mut commands, DepositKind::Gold, &gold_mesh, &rock_mat, *site, remaining, rocks, &mut seed);
    }
}

/// Crown each arena terrain hill (from [`crate::worldmap::arena_hills`]) with a cosmetic boulder
/// mound — big grey rocks stacked toward the centre so the rise reads as a small rocky mountain,
/// not just grey ground. Purely scenery (no `Deposit`); the boulders register modest nav blockers
/// like the deposit clusters, and they sit on the flanks well off the base-to-base lane. One-shot on
/// [`crate::biome::WorldReady`].
fn spawn_arena_hills(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    ready: Res<crate::biome::WorldReady>,
    mut done: Local<bool>,
) {
    if *done || !ready.0 {
        return;
    }
    *done = true;
    let rock_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        metallic: 0.05,
        ..default()
    });
    let mesh = meshes.add(boulder_mesh(false));
    let (hills, hill_r) = crate::worldmap::arena_hills();
    let mut seed: u32 = 0x40C_5EED;
    for c in hills {
        // A dozen boulders: one big centre crag + a scree skirt fading out to `hill_r`.
        let n = 12;
        for k in 0..n {
            let (rx, rz, sc) = if k == 0 {
                (c.x, c.y, 2.4) // the summit crag
            } else {
                let ang = crate::wildlife::rng_range(&mut seed, 0.0, TAU);
                let rr = crate::wildlife::rng_range(&mut seed, 0.6, hill_r * 0.85);
                let sc = crate::wildlife::rng_range(&mut seed, 0.8, 1.8) * (1.0 - rr / (hill_r * 1.3));
                (c.x + ang.cos() * rr, c.y + ang.sin() * rr, sc.max(0.5))
            };
            let ry = crate::worldmap::ground_at_world(rx, rz).unwrap_or(0.0);
            let yaw = crate::wildlife::rng_range(&mut seed, 0.0, TAU);
            crate::blockers::add(rx, rz, ROCK_BLOCK_R * sc * 0.8);
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(rock_mat.clone()),
                Transform::from_xyz(rx, ry, rz)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(sc)),
            ));
        }
    }
}

/// A wood grove: an invisible anchor at `centre` + `n` real trees in a `r≈3..5` ring. Trees are
/// cosmetic (no blocker) and are felled one at a time as the wood is hauled off.
fn spawn_grove(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    tree_mat: &Handle<StandardMaterial>,
    centre: Vec2,
    remaining: f64,
    n: usize,
    seed: &mut u32,
) {
    let cy = crate::worldmap::ground_at_world(centre.x, centre.y).unwrap_or(0.0);
    let anchor = commands
        .spawn((
            Transform::from_xyz(centre.x, cy, centre.y),
            Visibility::Hidden,
            Deposit { kind: DepositKind::Wood, remaining },
        ))
        .id();

    let mut parts = Vec::with_capacity(n);
    for k in 0..n {
        let ang = k as f32 / n as f32 * TAU + crate::wildlife::rng_range(seed, -0.3, 0.3);
        let r = crate::wildlife::rng_range(seed, 3.0, 5.0);
        let tx = centre.x + ang.cos() * r;
        let tz = centre.y + ang.sin() * r;
        let ty = crate::worldmap::ground_at_world(tx, tz).unwrap_or(cy);
        let kind = if k % 3 == 0 { TreeKind::Broadleaf } else { TreeKind::Pine };
        let yaw = crate::wildlife::rng_range(seed, 0.0, TAU);
        let sc = crate::wildlife::rng_range(seed, 0.9, 1.25);
        let e = commands
            .spawn((
                Mesh3d(meshes.add(build_tree_mesh(kind))),
                MeshMaterial3d(tree_mat.clone()),
                Transform::from_xyz(tx, ty, tz)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(sc)),
            ))
            .id();
        parts.push((e, None));
    }
    commands.entity(anchor).insert(DepositVisuals { parts, start: remaining, felled: 0 });
}

/// A stone/gold outcrop: an invisible anchor at `centre` + a tight cluster of faceted boulders,
/// each registering a nav blocker so units path around the rock. Boulders shatter (topple) and
/// their blockers lift as the vein is worked out.
#[allow(clippy::too_many_arguments)]
fn spawn_cluster(
    commands: &mut Commands,
    kind: DepositKind,
    rock_mesh: &Handle<Mesh>,
    rock_mat: &Handle<StandardMaterial>,
    centre: Vec2,
    remaining: f64,
    n: usize,
    seed: &mut u32,
) {
    let cy = crate::worldmap::ground_at_world(centre.x, centre.y).unwrap_or(0.0);
    let anchor = commands
        .spawn((
            Transform::from_xyz(centre.x, cy, centre.y),
            Visibility::Hidden,
            Deposit { kind, remaining },
        ))
        .id();

    let mut parts = Vec::with_capacity(n);
    for k in 0..n {
        // First rock at the centre, the rest clustered tightly around it.
        let (rx, rz) = if k == 0 {
            (centre.x, centre.y)
        } else {
            let ang = crate::wildlife::rng_range(seed, 0.0, TAU);
            let r = crate::wildlife::rng_range(seed, 0.8, 1.8);
            (centre.x + ang.cos() * r, centre.y + ang.sin() * r)
        };
        let ry = crate::worldmap::ground_at_world(rx, rz).unwrap_or(cy);
        let yaw = crate::wildlife::rng_range(seed, 0.0, TAU);
        let sc = crate::wildlife::rng_range(seed, 0.75, 1.15);
        crate::blockers::add(rx, rz, ROCK_BLOCK_R * sc);
        let e = commands
            .spawn((
                Mesh3d(rock_mesh.clone()),
                MeshMaterial3d(rock_mat.clone()),
                Transform::from_xyz(rx, ry, rz)
                    .with_rotation(Quat::from_rotation_y(yaw))
                    .with_scale(Vec3::splat(sc)),
            ))
            .id();
        parts.push((e, Some(Vec2::new(rx, rz))));
    }
    commands.entity(anchor).insert(DepositVisuals { parts, start: remaining, felled: 0 });
}

/// Progressive fell-out: keep the number of standing parts proportional to `remaining`, felling
/// (topple + reap) the next part whenever the stock crosses its threshold. At `0` the last parts
/// topple and the spent anchor is despawned.
fn deplete_deposit_visuals(
    time: Res<Time>,
    mut commands: Commands,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    focus: Res<crate::rts::camera::RtsCamFocus>,
    mut q: Query<(Entity, &Deposit, &Transform, &mut DepositVisuals)>,
) {
    let now = time.elapsed_secs();
    for (anchor, dep, atf, mut vis) in &mut q {
        let apos = Vec2::new(atf.translation.x, atf.translation.z);
        let max = vis.parts.len();
        let frac = if vis.start > 0.0 { (dep.remaining / vis.start).clamp(0.0, 1.0) } else { 0.0 };
        // ceil so a site with any stock left keeps at least one standing part.
        let standing = (frac * max as f64).ceil() as usize;
        let desired_felled = max.saturating_sub(standing.min(max));
        while vis.felled < desired_felled && vis.felled < max {
            let (e, blocker) = vis.parts[vis.felled];
            // Fell sound (only if on-screen): a tree crashing / a boulder shattering.
            if focus.in_earshot(apos) {
                match dep.kind {
                    DepositKind::Wood => cues.write(crate::audio::AudioCue::TreeFall { cactus: false }),
                    DepositKind::Stone | DepositKind::Gold => {
                        cues.write(crate::audio::AudioCue::OreShatter)
                    }
                };
            }
            crate::dying::begin_dying(&mut commands, e, now); // topple + reap (~1.4s)
            if let Some(p) = blocker {
                crate::blockers::remove_at(p.x, p.y);
            }
            vis.felled += 1;
        }
        if dep.remaining <= 0.0 && vis.felled >= max {
            // Spent: the visuals are toppling on their own fade clock; drop the anchor so workers
            // stop seeing this deposit (they already filter `remaining > 0`).
            commands.entity(anchor).try_despawn();
        }
    }
}

/// A low-poly faceted boulder built to the mesh contract (vertex COLOR, flat-shaded, base ≈ y=0
/// so it reads embedded in the ground). `gold` swaps the grey stone palette for a warm gold-vein
/// palette (the same shape recoloured).
fn boulder_mesh(gold: bool) -> Mesh {
    // (base, mid, highlight) linear-RGB source hexes.
    let (base, mid, hi) =
        if gold { (0x7a6326u32, 0xb5912fu32, 0xd9b13cu32) } else { (0x565961u32, 0x6b6d73u32, 0x9aa0aau32) };
    merged_flat(vec![
        facet(0.80, Vec3::new(0.0, 0.55, 0.0), 0.85, base),
        facet(0.50, Vec3::new(0.45, 0.40, -0.20), 0.90, hi),
        facet(0.50, Vec3::new(-0.40, 0.35, 0.30), 0.85, mid),
        facet(0.40, Vec3::new(0.10, 0.25, 0.45), 0.90, mid),
    ])
}

/// One faceted lump — a 20-face icosahedron (ico detail 0, hard-normalled), squashed in Y and
/// translated to `off`, tinted a flat linear colour. Mirrors `biome_rocky::facet_at`.
fn facet(r: f32, off: Vec3, squash: f32, hex: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .unwrap()
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        lin(hex),
    )
}
