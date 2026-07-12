//! **RTS building placement + Stronghold-style unmanned construction.**
//!
//! Owns the whole life-cycle of a skirmish structure's *body*:
//!  1. **Meshes** — reuses the campaign producer meshes (`town_meshes` barn / saw-shed / pit-head)
//!     for Farm / Sawmill / Quarry / Gold Mine, and authors two NEW merges here per the mesh
//!     contract (Town Hall + Barracks). Each side gets a palette twist (the Rival is the desert
//!     faction → sand/ochre tones). Everything renders off ONE shared white `StandardMaterial`
//!     with colour baked into vertex `ATTRIBUTE_COLOR` (mesh contract), so it batches.
//!  2. **Pre-built halls** — once the arena world is up (`biome::WorldReady`) a completed Town Hall
//!     is dropped on each base plateau, banks seeded to `starting_bank`, pop caps set to `HALL_POP`.
//!  3. **Ghost placement** — while `Placing` is `Some`, a translucent ghost follows the snapped
//!     cursor (R rotates 90°), tinted green/red by [`placement_valid`]; LMB pays + raises a scaffold,
//!     RMB/Esc cancels. The AI raises buildings through the same [`try_place`] entry point.
//!  4. **Timed construction** — the building scales up out of the ground over `build_secs` inside a
//!     timber scaffold frame; on completion it registers its collision box, gains housing / a train
//!     queue, and drops the frame. Buildings carry `Health` from spawn (attackable throughout); a
//!     dead Town Hall decides the match via `RtsOutcome`.
//!
//! Every system is gated `in_skirmish` + `Playing` + `Modal::None` (all of this is world-sim).

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::castle::M;
use crate::game_state::{AppState, Modal};
use crate::meshkit::{merged_flat, tinted};
use crate::palette::lin;

use super::{
    base_of, building_def, starting_bank, BuildingKind, Deposit, Placing, RtsBanks, RtsBuilding,
    RtsOutcome, RtsPop, Side, TrainQueue, HALL_POP, HOUSE_POP, POP_HARD_CAP,
};

/// The seven placeable kinds — keyed by [`RtsBuildAssets`] and iterated at asset-bake time.
const ALL_KINDS: [BuildingKind; 7] = [
    BuildingKind::TownHall,
    BuildingKind::House,
    BuildingKind::Sawmill,
    BuildingKind::Quarry,
    BuildingKind::GoldMine,
    BuildingKind::Farm,
    BuildingKind::Barracks,
];

/// Scaffold Y-scale a building starts at (never 0 — a degenerate scale can NaN normals / dead AABB).
const START_SCALE_Y: f32 = 0.12;
/// Max terrain height spread (world units) allowed under a footprint before placement is refused.
const FLAT_TOLERANCE: f32 = 0.35;
/// No building may be raised within this radius of the ENEMY base plateau centre (spec §6).
const ENEMY_BASE_KEEPOUT: f32 = 13.0;

// ────────────────────────────────────────────────────────────── plugin

pub struct RtsBuildPlugin;

impl Plugin for RtsBuildPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_build_assets.run_if(super::in_skirmish));
        app.add_systems(
            Update,
            (
                spawn_starting_halls,
                ghost_placement,
                grow_buildings,
                reap_buildings,
                crumble_buildings,
            )
                .run_if(super::in_skirmish)
                .run_if(in_state(AppState::Playing))
                .run_if(in_state(Modal::None)),
        );
    }
}

// ────────────────────────────────────────────────────────────── baked assets

/// Pre-baked meshes + shared materials for every building. Meshes are cached per `(kind, side)`
/// (a `Vec<Handle>` because reused producer meshes are multi-part), so both the real spawn path and
/// the ghost share handles and the ONE `solid` material batches. `pub` so the AI (`ai.rs`) can hand
/// a `&RtsBuildAssets` to [`try_place`].
#[derive(Resource)]
pub struct RtsBuildAssets {
    building: HashMap<(BuildingKind, Side), Vec<Handle<Mesh>>>,
    scaffold: HashMap<u32, Handle<Mesh>>,
    /// Opaque white — reads vertex `ATTRIBUTE_COLOR`; every real building part uses it.
    solid: Handle<StandardMaterial>,
    /// Translucent unlit — the ghost silhouette; its `base_color` is retinted green/red per frame.
    ghost_body: Handle<StandardMaterial>,
}

fn setup_build_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let solid = mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let ghost_body = mats.add(StandardMaterial {
        base_color: Color::srgba(0.3, 1.0, 0.35, 0.5),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    });

    let mut building = HashMap::new();
    for &kind in &ALL_KINDS {
        for &side in &[Side::Player, Side::Rival] {
            let handles: Vec<Handle<Mesh>> =
                building_meshes(kind, side).into_iter().map(|m| meshes.add(m)).collect();
            building.insert((kind, side), handles);
        }
    }

    let mut scaffold = HashMap::new();
    for fp in [2u32, 3, 4] {
        scaffold.insert(fp, meshes.add(scaffold_frame(fp)));
    }

    commands.insert_resource(RtsBuildAssets { building, scaffold, solid, ghost_body });
}

// ────────────────────────────────────────────────────────────── pre-built halls

/// Once the arena terrain is up (`WorldReady`), drop a completed Town Hall on each base plateau and
/// seed that side's bank + pop cap. Runs every frame until it has fired once (`Local` latch).
fn spawn_starting_halls(
    mut commands: Commands,
    assets: Option<Res<RtsBuildAssets>>,
    world_ready: Res<crate::biome::WorldReady>,
    mut banks: ResMut<RtsBanks>,
    mut pop: ResMut<RtsPop>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(assets) = assets else { return };
    if !world_ready.0 {
        return; // ground not generated yet — ground_at_world would return the fallback 0.0
    }
    // Staging aid: `FOREST_RTS_RICH=1` fattens BOTH starting banks ×20 (still mirrored-fair) so a
    // harness shot/clip can film the AI's build-out and attack waves without real-economy waits.
    let rich = if std::env::var("FOREST_RTS_RICH").is_ok() { 20.0 } else { 1.0 };
    for &side in &[Side::Player, Side::Rival] {
        spawn_hall(&mut commands, &assets, side, base_of(side));
        let mut bank = starting_bank();
        bank.wood *= rich;
        bank.stone *= rich;
        bank.gold *= rich;
        bank.food *= rich;
        *banks.side_mut(side) = bank;
        pop.0[side.ix()].count = 0;
        pop.0[side.ix()].cap = HALL_POP;
    }
    *done = true;
}

/// Spawn a **completed** Town Hall (built, full scale, blocker + Health registered).
fn spawn_hall(commands: &mut Commands, assets: &RtsBuildAssets, side: Side, pos: Vec2) {
    let def = building_def(BuildingKind::TownHall);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let handles = assets.building.get(&(BuildingKind::TownHall, side)).cloned().unwrap_or_default();
    let root = commands
        .spawn((
            Transform::from_xyz(pos.x, y, pos.y),
            Visibility::Visible,
            RtsBuilding { kind: BuildingKind::TownHall, built: true },
            side,
            crate::player::Health { hp: def.hp, max: def.hp },
        ))
        .id();
    commands.entity(root).with_children(|p| {
        for h in &handles {
            p.spawn((Mesh3d(h.clone()), MeshMaterial3d(assets.solid.clone()), Transform::default()));
        }
    });
    let hx = footprint_half(def.footprint);
    crate::blockers::add_box(pos.x, pos.y, hx, hx);
}

// ────────────────────────────────────────────────────────────── ghost placement

/// The live ghost's state, carried across frames on a `Local`.
#[derive(Default)]
struct GhostState {
    root: Option<Entity>,
    kind: Option<BuildingKind>,
    rot_steps: u32,
}

/// Drive the placement ghost when `Placing` is armed: follow the snapped cursor, tint by validity,
/// rotate on **R**, place on **LMB**, cancel on **RMB / Esc**. Placement always goes through
/// [`try_place`] so the player and AI paths share one validate+spend+spawn.
#[allow(clippy::too_many_arguments)]
fn ghost_placement(
    mut commands: Commands,
    mut placing: ResMut<Placing>,
    mut ghost: Local<GhostState>,
    assets: Option<Res<RtsBuildAssets>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut banks: ResMut<RtsBanks>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    deposits_q: Query<&Transform, With<Deposit>>,
) {
    let Some(assets) = assets else { return };

    // Not placing → tear the ghost down and bail.
    let Some(kind) = placing.0 else {
        clear_ghost(&mut commands, &mut ghost);
        return;
    };

    // Cancel (RMB / Esc) — cheapest to test first.
    if keys.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Right) {
        placing.0 = None;
        clear_ghost(&mut commands, &mut ghost);
        return;
    }

    if keys.just_pressed(KeyCode::KeyR) {
        ghost.rot_steps = (ghost.rot_steps + 1) % 4;
    }

    // Cursor → ground. `pick::cursor_ray_ground` is the sibling-owned ray/terrain refinement.
    let (Ok(window), Ok((cam, cam_tf))) = (windows.single(), cameras.single()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else { return };
    let Some(gp) = super::pick::cursor_ray_ground(cam, cam_tf, cursor) else {
        return;
    };
    let snapped = Vec2::new(gp.x.round(), gp.y.round());

    // (Re)spawn the ghost body when the kind changes. `just_armed` = this is the very frame the
    // build-strip button armed `Placing`. The button click and the ghost's confirm-click both read
    // the SAME `just_pressed(Left)` this frame, so without this guard the arming click also lands
    // the building instantly (at the ground point under the HUD button) — the player never gets to
    // choose a spot. Skip placing on the arming frame; the next click confirms.
    let just_armed = ghost.kind != Some(kind);
    if just_armed {
        clear_ghost(&mut commands, &mut ghost);
        ghost.root = Some(spawn_ghost(&mut commands, &assets, kind));
        ghost.kind = Some(kind);
    }

    let deposits: Vec<Vec2> =
        deposits_q.iter().map(|t| Vec2::new(t.translation.x, t.translation.z)).collect();
    let valid = placement_valid(kind, Side::Player, snapped, &deposits);

    if let Some(mut m) = mats.get_mut(&assets.ghost_body) {
        m.base_color = if valid {
            Color::srgba(0.32, 1.0, 0.38, 0.5)
        } else {
            Color::srgba(1.0, 0.32, 0.3, 0.5)
        };
    }

    let y = crate::worldmap::ground_at_world(snapped.x, snapped.y).unwrap_or(0.0);
    if let Some(e) = ghost.root {
        commands.entity(e).try_insert(
            Transform::from_xyz(snapped.x, y, snapped.y)
                .with_rotation(Quat::from_rotation_y(ghost.rot_steps as f32 * FRAC_PI_2)),
        );
    }

    if !just_armed
        && mouse.just_pressed(MouseButton::Left)
        && valid
        && try_place(&mut commands, &assets, &mut banks, &deposits, kind, Side::Player, snapped, ghost.rot_steps)
    {
        placing.0 = None;
        clear_ghost(&mut commands, &mut ghost);
        ghost.rot_steps = 0;
    }
}

fn clear_ghost(commands: &mut Commands, ghost: &mut GhostState) {
    if let Some(e) = ghost.root.take() {
        commands.entity(e).try_despawn();
    }
    ghost.kind = None;
}

fn spawn_ghost(commands: &mut Commands, assets: &RtsBuildAssets, kind: BuildingKind) -> Entity {
    let root = commands.spawn((Transform::default(), Visibility::Visible)).id();
    let handles = assets.building.get(&(kind, Side::Player)).cloned().unwrap_or_default();
    commands.entity(root).with_children(|p| {
        for h in &handles {
            p.spawn((Mesh3d(h.clone()), MeshMaterial3d(assets.ghost_body.clone()), Transform::default()));
        }
    });
    root
}

// ────────────────────────────────────────────────────────────── placement API

/// Validate a footprint (spec §6): land + terrain flat enough + collision-clear + off any deposit +
/// outside the enemy base plateau. Pure over the global blocker/terrain state + the passed deposit
/// centres, so both the ghost and the headless AI validate identically.
pub fn placement_valid(kind: BuildingKind, side: Side, pos: Vec2, deposits: &[Vec2]) -> bool {
    let half = footprint_half(building_def(kind).footprint);

    // Enemy base keep-out.
    if pos.distance(base_of(side.foe())) < ENEMY_BASE_KEEPOUT {
        return false;
    }
    // Deposit overlap (deposits carry no radius; use a fixed clearance).
    for d in deposits {
        if pos.distance(*d) < half + 1.6 {
            return false;
        }
    }
    // Terrain: sweep a grid over the footprint testing land + flatness + blockers.
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    let steps = 4; // 5×5 samples across the footprint
    for i in 0..=steps {
        for j in 0..=steps {
            let sx = pos.x - half + (i as f32 / steps as f32) * 2.0 * half;
            let sz = pos.y - half + (j as f32 / steps as f32) * 2.0 * half;
            match crate::worldmap::ground_at_world(sx, sz) {
                Some(h) => {
                    lo = lo.min(h);
                    hi = hi.max(h);
                }
                None => return false, // off-land / ocean
            }
            if crate::blockers::is_blocked(sx, sz) {
                return false;
            }
        }
    }
    hi - lo <= FLAT_TOLERANCE
}

/// Ring-search a valid placement for `kind` around `centre` (nearest ring first). Shared by the
/// ecotest stager and the RC bridge's auto-spot `build` op.
pub fn find_spot(kind: BuildingKind, side: Side, centre: Vec2, deposits: &[Vec2]) -> Option<Vec2> {
    for r in [4.0f32, 6.0, 8.0, 10.0, 13.0, 16.0] {
        for k in 0..16 {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            let pos = (centre + Vec2::new(a.cos(), a.sin()) * r).round();
            if placement_valid(kind, side, pos, deposits) {
                return Some(pos);
            }
        }
    }
    None
}

/// Validate → spend (all-or-nothing) → raise a scaffold. Returns whether the building went up. The
/// single entry point for BOTH the ghost path and the AI (`ai.rs`), so their rules can't diverge.
#[allow(clippy::too_many_arguments)]
pub fn try_place(
    commands: &mut Commands,
    assets: &RtsBuildAssets,
    banks: &mut RtsBanks,
    deposits: &[Vec2],
    kind: BuildingKind,
    side: Side,
    pos: Vec2,
    rot_steps: u32,
) -> bool {
    if !placement_valid(kind, side, pos, deposits) {
        return false;
    }
    let def = building_def(kind);
    if !banks.side_mut(side).spend(&def.cost) {
        return false; // POC: silent no-op on short funds (HUD toast is the sibling's job)
    }
    spawn_scaffold(commands, assets, kind, side, pos, rot_steps);
    true
}

/// A building under construction: the growth timer + the timber frame entity to drop on completion.
#[derive(Component)]
struct UnderConstruction {
    elapsed: f32,
    total: f32,
    frame: Option<Entity>,
}

fn spawn_scaffold(
    commands: &mut Commands,
    assets: &RtsBuildAssets,
    kind: BuildingKind,
    side: Side,
    pos: Vec2,
    rot_steps: u32,
) {
    let def = building_def(kind);
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let yaw = rot_steps as f32 * FRAC_PI_2;
    let handles = assets.building.get(&(kind, side)).cloned().unwrap_or_default();

    // The growing building: its parent Y-scale animates 0.12 → 1.0 (bases sit at y=0, so it rises
    // out of the ground). Health is present from spawn — attackable mid-build.
    let root = commands
        .spawn((
            Transform::from_xyz(pos.x, y, pos.y)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::new(1.0, START_SCALE_Y, 1.0)),
            Visibility::Visible,
            RtsBuilding { kind, built: false },
            side,
            crate::player::Health { hp: def.hp, max: def.hp },
        ))
        .id();
    commands.entity(root).with_children(|p| {
        for h in &handles {
            p.spawn((Mesh3d(h.clone()), MeshMaterial3d(assets.solid.clone()), Transform::default()));
        }
    });

    // A static full-height timber frame (a SEPARATE entity so it doesn't scale with the growth).
    let frame = assets.scaffold.get(&def.footprint).map(|fm| {
        commands
            .spawn((
                Mesh3d(fm.clone()),
                MeshMaterial3d(assets.solid.clone()),
                Transform::from_xyz(pos.x, y, pos.y).with_rotation(Quat::from_rotation_y(yaw)),
                Visibility::Visible,
            ))
            .id()
    });

    commands
        .entity(root)
        .try_insert(UnderConstruction { elapsed: 0.0, total: def.build_secs.max(0.01), frame });
    commands.spawn(crate::build_fx::DustBurst::building(Vec3::new(pos.x, y, pos.y)));
}

// ────────────────────────────────────────────────────────────── construction tick

/// Grow scaffolds toward full size; on completion register the collision box, grant housing / a
/// train queue, drop the frame, and flip `built`.
fn grow_buildings(
    time: Res<Time>,
    mut commands: Commands,
    mut pop: ResMut<RtsPop>,
    mut q: Query<(Entity, &mut UnderConstruction, &mut Transform, &mut RtsBuilding, &Side), Without<Crumbling>>,
) {
    let dt = time.delta_secs();
    for (e, mut uc, mut tf, mut b, side) in &mut q {
        uc.elapsed += dt;
        let t = (uc.elapsed / uc.total).clamp(0.0, 1.0);
        tf.scale.y = START_SCALE_Y + (1.0 - START_SCALE_Y) * t;
        if t < 1.0 {
            continue;
        }
        tf.scale.y = 1.0;
        b.built = true;

        let pos = Vec2::new(tf.translation.x, tf.translation.z);
        let hx = footprint_half(building_def(b.kind).footprint);
        crate::blockers::add_box(pos.x, pos.y, hx, hx);

        if let Some(f) = uc.frame {
            commands.entity(f).try_despawn();
        }

        match b.kind {
            BuildingKind::House => {
                let c = &mut pop.0[side.ix()];
                c.cap = (c.cap + HOUSE_POP).min(POP_HARD_CAP);
            }
            // A finished barracks can train; the HUD / wave-3 combat drive the queue.
            BuildingKind::Barracks => {
                commands.entity(e).try_insert(TrainQueue::default());
            }
            // Producers: workers.rs watches for `built` producers and claims a worker — nothing here.
            _ => {}
        }

        commands.spawn(crate::build_fx::DustBurst::building(Vec3::new(pos.x, tf.translation.y, pos.y)));
        commands.entity(e).try_remove::<UnderConstruction>();
    }
}

// ────────────────────────────────────────────────────────────── death / crumble

/// A dying static — scales down over ~0.6 s then despawns.
#[derive(Component)]
struct Crumbling {
    t: f32,
}

/// Reap buildings whose `Health` hit zero: drop the blocker, free housing, decide the match on a
/// Town Hall, and start the crumble. Runs once per building (skips those already `Crumbling`).
fn reap_buildings(
    mut commands: Commands,
    mut outcome: ResMut<RtsOutcome>,
    mut pop: ResMut<RtsPop>,
    q: Query<
        (Entity, &RtsBuilding, &Side, &Transform, &crate::player::Health, Option<&UnderConstruction>),
        Without<Crumbling>,
    >,
) {
    for (e, b, side, tf, hp, uc) in &q {
        if hp.hp > 0.0 {
            continue;
        }
        let pos = Vec2::new(tf.translation.x, tf.translation.z);
        crate::blockers::remove_box_near(pos.x, pos.y, 0.6);

        match b.kind {
            BuildingKind::House if b.built => {
                pop.0[side.ix()].cap = pop.0[side.ix()].cap.saturating_sub(HOUSE_POP).max(HALL_POP);
            }
            BuildingKind::TownHall => {
                if *outcome == RtsOutcome::Undecided {
                    // The hall's side is the LOSER.
                    *outcome = match side {
                        Side::Player => RtsOutcome::RivalWon,
                        Side::Rival => RtsOutcome::PlayerWon,
                    };
                }
            }
            _ => {}
        }

        // A building killed mid-build leaves an orphan scaffold frame — reap it too.
        if let Some(uc) = uc {
            if let Some(f) = uc.frame {
                commands.entity(f).try_despawn();
            }
        }

        commands.entity(e).try_remove::<UnderConstruction>();
        commands.entity(e).try_insert(Crumbling { t: 0.0 });
        commands.spawn(crate::build_fx::DustBurst::building(tf.translation));
    }
}

fn crumble_buildings(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Crumbling, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut c, mut tf) in &mut q {
        c.t += dt;
        let s = (1.0 - c.t / 0.6).max(0.0);
        tf.scale = Vec3::splat(s);
        if c.t >= 0.6 {
            commands.entity(e).try_despawn();
        }
    }
}

// ────────────────────────────────────────────────────────────── mesh authoring

/// Footprint half-extent (world units) for the collision box — slightly inset so units can stand
/// flush against a wall without being trapped inside it.
fn footprint_half(footprint: u32) -> f32 {
    (footprint as f32 * 0.5 - 0.1).max(0.3)
}

/// A translated axis-aligned box primitive (indexed, POSITION/NORMAL/UV_0) — mergeable with the
/// cone/other cuboids in an authored merge.
fn cuboid(w: f32, h: f32, d: f32, x: f32, y: f32, z: f32) -> Mesh {
    Mesh::from(Cuboid::new(w, h, d)).translated_by(Vec3::new(x, y, z))
}

/// A square (45°-rotated) 4-sided pyramid roof, base at `base_y`.
fn pyramid(r: f32, h: f32, base_y: f32) -> Mesh {
    Cone { radius: r, height: h }
        .mesh()
        .resolution(4)
        .build()
        .rotated_by(Quat::from_rotation_y(FRAC_PI_4))
        .translated_by(Vec3::new(0.0, base_y + h / 2.0, 0.0))
}

/// Pick the player or (desert-shifted) rival hue.
fn pal(player: u32, rival: u32, side: Side) -> [f32; 4] {
    lin(if side == Side::Rival { rival } else { player })
}

/// The final render meshes for a `(kind, side)`. Producer kinds reuse the campaign `town_meshes`
/// parts (each part tinted from its `M` slot → its own child); the three authored kinds merge to a
/// single faceted mesh.
fn building_meshes(kind: BuildingKind, side: Side) -> Vec<Mesh> {
    match kind {
        BuildingKind::TownHall => vec![merged_flat(townhall_parts(side))],
        BuildingKind::Barracks => vec![merged_flat(barracks_parts(side))],
        BuildingKind::House => vec![merged_flat(house_parts(side))],
        _ => producer_parts(kind)
            .into_iter()
            .map(|(mesh, m)| {
                let mut c = m_lin(m, side);
                // Gold Mine: warm-gold the quarried stone so it reads as a gold vein, not a quarry.
                if kind == BuildingKind::GoldMine && is_stoneish(m) {
                    c = lin(0xd9a441);
                }
                tinted(mesh, c)
            })
            .collect(),
    }
}

fn producer_parts(kind: BuildingKind) -> Vec<(Mesh, M)> {
    match kind {
        BuildingKind::Farm => crate::town_meshes::farm_parts(),
        BuildingKind::Sawmill => crate::town_meshes::woodcutter_parts(),
        BuildingKind::Quarry | BuildingKind::GoldMine => crate::town_meshes::mine_parts(),
        _ => vec![],
    }
}

fn is_stoneish(m: M) -> bool {
    matches!(m, M::Stone | M::DarkStone | M::LightStone | M::HouseStone)
}

/// Linear colour for a campaign `M` material slot, with the Rival's desert twist (stone→sandstone,
/// timber→pale desert wood, roof→ochre, soil→dune). Exhaustive so a new `M` variant won't silently
/// fall through to a wrong hue.
fn m_lin(m: M, side: Side) -> [f32; 4] {
    let (p, r) = match m {
        M::Stone => (0x8a8b95, 0xc2a878),
        M::DarkStone => (0x6a6b73, 0xa8895c),
        M::LightStone => (0x9da1ac, 0xd8c39a),
        M::HouseStone => (0x86868e, 0xc2a878),
        M::Plaster => (0xd3b78b, 0xdcc596),
        M::Wood => (0x3a2618, 0x6b4a2a),
        M::Beam => (0x5a3a22, 0x7a5a34),
        M::Roof => (0x7a2f28, 0x9c5a2e),
        M::HouseRoof => (0x6b3322, 0x9c5a2e),
        M::HouseRoof2 => (0x6e6256, 0x8a6a44),
        M::Thatch => (0xb89b4f, 0xcbb26a),
        M::Iron => (0x6a6e72, 0x6a6e72),
        M::Parchment => (0xe6d9b5, 0xe6d9b5),
        M::Soil => (0x6b4a2a, 0x9c7a4a),
        M::Cobble => (0x8b8a86, 0xbfae8a),
        M::Packed => (0x6e5436, 0x9c7a4a),
        M::Straw => (0xcaa84e, 0xd8bf6a),
        M::Hen => (0xe7e2d6, 0xe7e2d6),
        M::Banner => (0x2f5fa6, 0xb0402a),
        M::Bronze => (0xb9892f, 0xc79a3a),
        M::BronzeDark => (0x7c5a1e, 0x8a6420),
        M::Crop => (0x8fae4a, 0xb7a24a),
        M::Slit => (0x23242a, 0x2a2620),
        M::Gold => (0xe0b04a, 0xe0b04a),
        M::Window => (0xffd58c, 0xffd58c),
        M::Flame => (0xff7a2a, 0xff7a2a),
        M::Ember => (0xff5a1e, 0xff5a1e),
    };
    pal(p, r, side)
}

/// A tiny timber scaffold frame ringing the footprint (corner posts + two rail courses). Cached per
/// footprint size; sits static (full height) around a growing building.
fn scaffold_frame(footprint: u32) -> Mesh {
    let h = footprint as f32 * 0.5 - 0.15;
    let top = 2.6;
    let beam = lin(0x6b5236); // pale, weathered scaffold timber
    let mut parts: Vec<Mesh> = Vec::new();
    for (sx, sz) in [(-h, -h), (h, -h), (-h, h), (h, h)] {
        parts.push(tinted(cuboid(0.12, top, 0.12, sx, top / 2.0, sz), beam));
    }
    for &y in &[top, top * 0.5] {
        for &sz in &[-h, h] {
            parts.push(tinted(cuboid(2.0 * h, 0.1, 0.1, 0.0, y, sz), beam));
        }
        for &sx in &[-h, h] {
            parts.push(tinted(cuboid(0.1, 0.1, 2.0 * h, sx, y, 0.0), beam));
        }
    }
    merged_flat(parts)
}

/// **Town Hall** (footprint 4×4): a compact keep — a broad stone block under a timber upper storey
/// and a pyramidal roof, with corner merlons, a warm-glowing door + windows, and a side banner.
fn townhall_parts(side: Side) -> Vec<Mesh> {
    let stone = pal(0x8a8b95, 0xc2a878, side);
    let dark = pal(0x6a6b73, 0xa8895c, side);
    let timber = pal(0x5a3a22, 0x7a5a34, side);
    let roof = pal(0x7a2f28, 0x9c5a2e, side);
    let gold = lin(0xe0b04a);
    let door = lin(0x23242a);
    let glow = lin(0xffd58c);
    let banner = pal(0x2f5fa6, 0xb0402a, side);

    let mut v: Vec<Mesh> = Vec::new();
    // Footing skirt + stone block (top at 1.75).
    v.push(tinted(cuboid(2.9, 0.25, 2.9, 0.0, 0.125, 0.0), dark));
    v.push(tinted(cuboid(2.6, 1.5, 2.6, 0.0, 1.0, 0.0), stone));
    // Corner merlons ringing the stone top.
    for (sx, sz) in [(-1.05, -1.05), (1.05, -1.05), (-1.05, 1.05), (1.05, 1.05)] {
        v.push(tinted(cuboid(0.36, 0.4, 0.36, sx, 1.95, sz), stone));
    }
    // Timber upper storey (top at 2.6) + pyramid roof (apex 3.7).
    v.push(tinted(cuboid(2.1, 0.85, 2.1, 0.0, 2.175, 0.0), timber));
    v.push(tinted(pyramid(1.7, 1.1, 2.6), roof));
    // Gold finial cube + banner pole & cloth.
    v.push(tinted(cuboid(0.24, 0.24, 0.24, 0.0, 3.8, 0.0), gold));
    v.push(tinted(cuboid(0.08, 1.2, 0.08, 0.0, 4.3, 0.0), timber));
    v.push(tinted(cuboid(0.55, 0.36, 0.05, 0.31, 4.6, 0.0), banner));
    // Door (+Z) + flanking windows.
    v.push(tinted(cuboid(0.7, 0.9, 0.08, 0.0, 0.7, 1.31), door));
    for sx in [-0.8, 0.8] {
        v.push(tinted(cuboid(0.3, 0.4, 0.06, sx, 1.15, 1.32), glow));
    }
    v
}

/// **Barracks** (footprint 4×4): a long timber hall under a peaked roof, with a fronting door and a
/// weapon rack of iron shafts in the yard.
fn barracks_parts(side: Side) -> Vec<Mesh> {
    let plank = pal(0x3a2618, 0x6b4a2a, side);
    let timber = pal(0x5a3a22, 0x7a5a34, side);
    let roof = pal(0x6b3322, 0x9c5a2e, side);
    let iron = lin(0x6a6e72);
    let door = lin(0x23242a);
    let banner = pal(0x2f5fa6, 0xb0402a, side);

    let mut v: Vec<Mesh> = Vec::new();
    // Long plank hall on the −X half of the plot (yard on +X, like the campaign producers).
    v.push(tinted(cuboid(3.4, 1.2, 1.8, -0.2, 0.6, 0.0), plank));
    for (sx, sz) in [(-1.9, -0.9), (1.5, -0.9), (-1.9, 0.9), (1.5, 0.9)] {
        v.push(tinted(cuboid(0.14, 1.3, 0.14, sx, 0.65, sz), timber));
    }
    // Peaked roof: two slabs tilted about X meeting at the ridge.
    let a = 0.5;
    v.push(tinted(
        Mesh::from(Cuboid::new(3.7, 0.12, 1.25))
            .rotated_by(Quat::from_rotation_x(a))
            .translated_by(Vec3::new(-0.2, 1.5, 0.45)),
        roof,
    ));
    v.push(tinted(
        Mesh::from(Cuboid::new(3.7, 0.12, 1.25))
            .rotated_by(Quat::from_rotation_x(-a))
            .translated_by(Vec3::new(-0.2, 1.5, -0.45)),
        roof,
    ));
    // Door (+Z front) + a side banner.
    v.push(tinted(cuboid(0.7, 0.9, 0.08, -0.2, 0.45, 0.91), door));
    v.push(tinted(cuboid(0.42, 0.32, 0.05, -1.9, 1.5, 0.0), banner));
    // Weapon rack in the +X yard: two posts, a top rail, three leaning iron shafts.
    let rx = 1.4;
    v.push(tinted(cuboid(0.08, 0.9, 0.08, rx - 0.5, 0.45, 0.6), timber));
    v.push(tinted(cuboid(0.08, 0.9, 0.08, rx + 0.5, 0.45, 0.6), timber));
    v.push(tinted(cuboid(1.1, 0.08, 0.08, rx, 0.88, 0.6), timber));
    for dx in [-0.3, 0.0, 0.3] {
        v.push(tinted(cuboid(0.05, 1.05, 0.05, rx + dx, 0.55, 0.6), iron));
    }
    v
}

/// **House** (footprint 2×2): a small plastered cottage with corner beams, a pyramid roof, a door
/// and a lit window.
fn house_parts(side: Side) -> Vec<Mesh> {
    let wall = pal(0xd3b78b, 0xdcc596, side);
    let beam = pal(0x5a3a22, 0x7a5a34, side);
    let roof = pal(0x6b3322, 0x9c5a2e, side);
    let door = lin(0x23242a);
    let glow = lin(0xffd58c);

    let mut v: Vec<Mesh> = Vec::new();
    v.push(tinted(cuboid(1.5, 0.9, 1.3, 0.0, 0.45, 0.0), wall));
    for (sx, sz) in [(-0.75, -0.65), (0.75, -0.65), (-0.75, 0.65), (0.75, 0.65)] {
        v.push(tinted(cuboid(0.14, 0.9, 0.14, sx, 0.45, sz), beam));
    }
    v.push(tinted(pyramid(1.15, 0.7, 0.9), roof));
    v.push(tinted(cuboid(0.4, 0.6, 0.06, 0.0, 0.3, 0.66), door));
    v.push(tinted(cuboid(0.3, 0.3, 0.06, 0.5, 0.62, 0.67), glow));
    v
}
