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
    RtsOutcome, RtsPop, Side, TrainQueue, HALL_POP, HOUSE_POP, PLAYER_BASE, POP_HARD_CAP,
};

/// The seven placeable kinds — keyed by [`RtsBuildAssets`] and iterated at asset-bake time.
const ALL_KINDS: [BuildingKind; 10] = [
    BuildingKind::TownHall,
    BuildingKind::House,
    BuildingKind::Sawmill,
    BuildingKind::Quarry,
    BuildingKind::GoldMine,
    BuildingKind::Farm,
    BuildingKind::Barracks,
    BuildingKind::Wall,
    BuildingKind::Watchtower,
    BuildingKind::Market,
];

/// Scaffold Y-scale a building starts at (never 0 — a degenerate scale can NaN normals / dead AABB).
const START_SCALE_Y: f32 = 0.12;
/// Uniform visual scale applied to every building body — chunkier, more imposing structures (the
/// meshes were authored small). Collision/footprint logic is unchanged; this is purely the model.
pub(crate) const BUILD_SCALE: f32 = 1.35;
/// Placement snaps to this world-unit grid so a built-up town reads tidy instead of chaotic (every
/// building lands on an aligned cell, not a fractional offset).
const PLACE_GRID: f32 = 2.0;

/// Snap a world point to the placement grid.
fn snap(p: Vec2) -> Vec2 {
    (p / PLACE_GRID).round() * PLACE_GRID
}
/// Max terrain height spread (world units) allowed under a footprint before placement is refused.
const FLAT_TOLERANCE: f32 = 0.35;
/// No building may be raised within this radius of the ENEMY base plateau centre (spec §6).
const ENEMY_BASE_KEEPOUT: f32 = 13.0;
/// Your castle's territory: you may only build within this radius of your OWN base centre. Covers
/// your half of the arena + the contested centre (origin is ~31u from each base), but stops well
/// short of the enemy's base (~62u away) — so you can't wall in / cheese the opponent's plateau.
pub(crate) const BUILD_TERRITORY_R: f32 = 34.0;

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
                territory_ring,
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
    /// Per building kind: the part meshes + which `M` material each uses. Both sides share these —
    /// friend/foe is read off the ground ring + minimap, not the building's hue.
    building: HashMap<BuildingKind, Vec<(Handle<Mesh>, M)>>,
    scaffold: HashMap<u32, Handle<Mesh>>,
    /// The campaign's procedurally-TEXTURED village material set (`M` → shingle/plaster/stone/…) —
    /// the same one the castle + town producers use, so RTS buildings finally look textured.
    mats: crate::castle::Mats,
    /// Opaque white reading vertex `ATTRIBUTE_COLOR` — used only for the timber scaffold frame.
    solid: Handle<StandardMaterial>,
    /// Translucent unlit — the ghost silhouette; its `base_color` is retinted green/red per frame.
    ghost_body: Handle<StandardMaterial>,
}

fn setup_build_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Build the campaign's textured village materials here (skirmish never runs the castle phase
    // that normally creates them), and share the resource so any other RTS system can reuse it.
    let village = crate::castle::build_mats(&mut images, &mut std_mats);
    commands.insert_resource(crate::castle::VillageMats(village.clone()));

    let solid = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let ghost_body = std_mats.add(StandardMaterial {
        base_color: Color::srgba(0.3, 1.0, 0.35, 0.5),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        double_sided: true,
        ..default()
    });

    let mut building = HashMap::new();
    for &kind in &ALL_KINDS {
        let parts: Vec<(Handle<Mesh>, M)> =
            building_meshes(kind).into_iter().map(|(mesh, m)| (meshes.add(mesh), m)).collect();
        building.insert(kind, parts);
    }

    let mut scaffold = HashMap::new();
    for fp in [2u32, 3, 4] {
        scaffold.insert(fp, meshes.add(scaffold_frame(fp)));
    }

    commands.insert_resource(RtsBuildAssets { building, scaffold, mats: village, solid, ghost_body });
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
    let handles = assets.building.get(&BuildingKind::TownHall).cloned().unwrap_or_default();
    let root = commands
        .spawn((
            Transform::from_xyz(pos.x, y, pos.y).with_scale(Vec3::splat(BUILD_SCALE)),
            Visibility::Visible,
            RtsBuilding { kind: BuildingKind::TownHall, built: true },
            side,
            crate::player::Health { hp: def.hp, max: def.hp },
        ))
        .id();
    commands.entity(root).with_children(|p| {
        for (h, m) in &handles {
            p.spawn((Mesh3d(h.clone()), MeshMaterial3d(assets.mats.get(*m)), Transform::default()));
        }
    });
    let hx = footprint_half(def.footprint);
    crate::blockers::add_box(pos.x, pos.y, hx, hx);
}

// ────────────────────────────────────────────────────────────── ghost placement

/// The live ghost's state, carried across frames on a `Local`.
#[derive(Default)]
struct GhostState {
    /// The single-building ghost body (None for a Wall, which uses `wall_ghosts`).
    root: Option<Entity>,
    kind: Option<BuildingKind>,
    rot_steps: u32,
    /// Wall drag: the tile where the LMB drag began (None = not dragging a wall).
    wall_start: Option<Vec2>,
    /// Pooled ghost segments previewing a wall run.
    wall_ghosts: Vec<Entity>,
}

/// The ghost tint colours.
const GHOST_OK: Color = Color::srgba(0.32, 1.0, 0.38, 0.5);
const GHOST_BAD: Color = Color::srgba(1.0, 0.32, 0.3, 0.5);
/// Wall segments step by their footprint so a dragged run abuts cleanly; cap the run length.
const WALL_STEP: f32 = 2.0;
const WALL_RUN_MAX: i32 = 30;

/// Axis-snapped run of wall tiles from `start` to `end` — only straight (H or V) runs, no diagonals:
/// snap to whichever axis the drag favours, then step by the wall footprint. First tile = `start`.
fn wall_line(start: Vec2, end: Vec2) -> Vec<Vec2> {
    let d = end - start;
    let (axis, len) = if d.x.abs() >= d.y.abs() {
        (Vec2::new(d.x.signum(), 0.0), d.x.abs())
    } else {
        (Vec2::new(0.0, d.y.signum()), d.y.abs())
    };
    let n = ((len / WALL_STEP).round() as i32).clamp(0, WALL_RUN_MAX);
    (0..=n)
        .map(|i| {
            let p = start + axis * (i as f32 * WALL_STEP);
            Vec2::new(p.x.round(), p.y.round())
        })
        .collect()
}

/// Drive the placement ghost when `Placing` is armed. Two interactions:
/// - **Normal buildings**: the ghost follows the snapped cursor (tinted by validity, **R** rotates);
///   LMB places, and placement STAYS armed so you can drop as many as you can afford (like a real
///   RTS) — RMB / Esc exits.
/// - **Wall**: click-**drag** a straight (axis-snapped) run and release to raise the whole row of
///   segments at once — no diagonals, no clicking each block.
///
/// Placement always routes through [`try_place`] so the player and AI share one validate+spend+spawn.
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
    mut cues: MessageWriter<crate::audio::AudioCue>,
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
    // R rotates non-wall footprints (walls are axis-snapped, no rotation).
    if kind != BuildingKind::Wall && keys.just_pressed(KeyCode::KeyR) {
        ghost.rot_steps = (ghost.rot_steps + 1) % 4;
    }

    // Cursor → ground.
    let (Ok(window), Ok((cam, cam_tf))) = (windows.single(), cameras.single()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else { return };
    let Some(gp) = super::pick::cursor_ray_ground(cam, cam_tf, cursor) else {
        return;
    };
    let snapped = snap(gp);

    // `just_armed` = the very frame the build-strip button armed `Placing`. The button click and a
    // ghost confirm-click share the SAME `just_pressed(Left)`; without this guard the arming click
    // also lands the building / starts a wall drag instantly. Skip the first frame's action.
    let just_armed = ghost.kind != Some(kind);
    if just_armed {
        clear_ghost(&mut commands, &mut ghost);
        ghost.kind = Some(kind);
        ghost.rot_steps = 0;
    }

    let deposits: Vec<Vec2> =
        deposits_q.iter().map(|t| Vec2::new(t.translation.x, t.translation.z)).collect();
    let can_afford = |b: &RtsBanks| b.side(Side::Player).can_afford(&building_def(kind).cost);

    if kind == BuildingKind::Wall {
        // ── Wall: click-drag a straight run ──
        if !just_armed && mouse.just_pressed(MouseButton::Left) {
            ghost.wall_start = Some(snapped);
        }
        let line = match ghost.wall_start {
            Some(s) => wall_line(s, snapped),
            None => vec![snapped],
        };
        let all_ok = line.iter().all(|t| placement_valid(kind, Side::Player, *t, &deposits));
        if let Some(mut m) = mats.get_mut(&assets.ghost_body) {
            m.base_color = if all_ok { GHOST_OK } else { GHOST_BAD };
        }
        // Pool the preview segments to the run length; park each on its tile, hide extras.
        while ghost.wall_ghosts.len() < line.len() {
            let e = spawn_ghost(&mut commands, &assets, kind);
            ghost.wall_ghosts.push(e);
        }
        let seg_ents = ghost.wall_ghosts.clone();
        for (i, e) in seg_ents.iter().enumerate() {
            if let Some(tile) = line.get(i) {
                let y = crate::worldmap::ground_at_world(tile.x, tile.y).unwrap_or(0.0);
                commands.entity(*e).try_insert((
                    Transform::from_xyz(tile.x, y, tile.y).with_scale(Vec3::splat(BUILD_SCALE)),
                    Visibility::Visible,
                ));
            } else {
                commands.entity(*e).try_insert(Visibility::Hidden);
            }
        }
        // Release → raise the whole run (each affordable, valid tile), then STAY armed for another run.
        if !just_armed && mouse.just_released(MouseButton::Left) && ghost.wall_start.is_some() {
            let mut placed = false;
            for tile in &line {
                if !can_afford(&banks) {
                    break;
                }
                if try_place(&mut commands, &assets, &mut banks, &deposits, kind, Side::Player, *tile, 0) {
                    placed = true;
                }
            }
            if placed {
                cues.write(crate::audio::AudioCue::UiSelect);
            }
            ghost.wall_start = None;
            if !can_afford(&banks) {
                placing.0 = None;
                clear_ghost(&mut commands, &mut ghost);
            }
        }
    } else {
        // ── Normal building: single ghost, REPEAT placement ──
        if ghost.root.is_none() {
            ghost.root = Some(spawn_ghost(&mut commands, &assets, kind));
        }
        let valid = placement_valid(kind, Side::Player, snapped, &deposits);
        if let Some(mut m) = mats.get_mut(&assets.ghost_body) {
            m.base_color = if valid { GHOST_OK } else { GHOST_BAD };
        }
        let y = crate::worldmap::ground_at_world(snapped.x, snapped.y).unwrap_or(0.0);
        if let Some(e) = ghost.root {
            commands.entity(e).try_insert(
                Transform::from_xyz(snapped.x, y, snapped.y)
                    .with_rotation(Quat::from_rotation_y(ghost.rot_steps as f32 * FRAC_PI_2))
                    .with_scale(Vec3::splat(BUILD_SCALE)),
            );
        }
        if !just_armed
            && mouse.just_pressed(MouseButton::Left)
            && valid
            && try_place(&mut commands, &assets, &mut banks, &deposits, kind, Side::Player, snapped, ghost.rot_steps)
        {
            cues.write(crate::audio::AudioCue::UiSelect);
            // Stay armed to drop another — only exit once the player can't afford it (or cancels).
            if !can_afford(&banks) {
                placing.0 = None;
                clear_ghost(&mut commands, &mut ghost);
            }
        }
    }
}

fn clear_ghost(commands: &mut Commands, ghost: &mut GhostState) {
    if let Some(e) = ghost.root.take() {
        commands.entity(e).try_despawn();
    }
    for e in ghost.wall_ghosts.drain(..) {
        commands.entity(e).try_despawn();
    }
    ghost.wall_start = None;
    ghost.kind = None;
}

/// A faint blue ring on the ground marking your castle's build territory ([`BUILD_TERRITORY_R`]) —
/// shown only while you're placing a building, so the "why is the ghost red out here" boundary is
/// visible ("granice zamku"). Spawned lazily once, then just toggled visible/hidden.
fn territory_ring(
    mut commands: Commands,
    mut ring: Local<Option<Entity>>,
    placing: Res<Placing>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut vis_q: Query<&mut Visibility>,
) {
    let want = placing.0.is_some();
    if ring.is_none() {
        if !want {
            return; // don't spawn until first needed
        }
        let mesh = meshes.add(
            Annulus::new(BUILD_TERRITORY_R - 0.6, BUILD_TERRITORY_R).mesh().resolution(96).build(),
        );
        let mat = mats.add(StandardMaterial {
            base_color: Color::srgba(0.35, 0.72, 1.0, 0.5),
            emissive: LinearRgba::rgb(0.12, 0.4, 0.85),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        let y = crate::worldmap::ground_at_world(PLAYER_BASE.x, PLAYER_BASE.y).unwrap_or(0.0) + 0.15;
        *ring = Some(
            commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(mat),
                    Transform::from_xyz(PLAYER_BASE.x, y, PLAYER_BASE.y)
                        .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                ))
                .id(),
        );
    }
    if let Some(e) = *ring {
        if let Ok(mut v) = vis_q.get_mut(e) {
            *v = if want { Visibility::Visible } else { Visibility::Hidden };
        }
    }
}

fn spawn_ghost(commands: &mut Commands, assets: &RtsBuildAssets, kind: BuildingKind) -> Entity {
    let root = commands.spawn((Transform::default(), Visibility::Visible)).id();
    let handles = assets.building.get(&kind).cloned().unwrap_or_default();
    commands.entity(root).with_children(|p| {
        for (h, _m) in &handles {
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
    // Own-territory limit — can't build out in the enemy's half of the map.
    if pos.distance(base_of(side)) > BUILD_TERRITORY_R {
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
    let handles = assets.building.get(&kind).cloned().unwrap_or_default();

    // Instant build (player request): the building stands at full size the moment it's paid for — no
    // scaffold, no grow timer. It's spawned complete; a 1-frame `UnderConstruction` (total ~0) still
    // runs so `grow_buildings` does the one-time completion bookkeeping (collision box, housing /
    // train queue, `built`). Health is present from spawn.
    let root = commands
        .spawn((
            Transform::from_xyz(pos.x, y, pos.y)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(BUILD_SCALE)),
            Visibility::Visible,
            RtsBuilding { kind, built: false },
            side,
            crate::player::Health { hp: def.hp, max: def.hp },
        ))
        .id();
    commands.entity(root).with_children(|p| {
        for (h, m) in &handles {
            p.spawn((Mesh3d(h.clone()), MeshMaterial3d(assets.mats.get(*m)), Transform::default()));
        }
    });

    commands
        .entity(root)
        .try_insert(UnderConstruction { elapsed: 0.0, total: 0.001, frame: None });
    commands.spawn(crate::build_fx::DustBurst::building(Vec3::new(pos.x, y, pos.y)));
}

// ────────────────────────────────────────────────────────────── construction tick

/// Grow scaffolds toward full size; on completion register the collision box, grant housing / a
/// train queue, drop the frame, and flip `built`.
fn grow_buildings(
    time: Res<Time>,
    mut commands: Commands,
    mut pop: ResMut<RtsPop>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    focus: Res<super::camera::RtsCamFocus>,
    mut q: Query<(Entity, &mut UnderConstruction, &mut Transform, &mut RtsBuilding, &Side), Without<Crumbling>>,
) {
    let dt = time.delta_secs();
    for (e, mut uc, mut tf, mut b, side) in &mut q {
        uc.elapsed += dt;
        let t = (uc.elapsed / uc.total).clamp(0.0, 1.0);
        // Scale relative to BUILD_SCALE so x/y/z stay uniform (x,z are set to BUILD_SCALE at spawn).
        tf.scale.y = BUILD_SCALE * (START_SCALE_Y + (1.0 - START_SCALE_Y) * t);
        if t < 1.0 {
            continue;
        }
        tf.scale.y = BUILD_SCALE;
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

        // Completion feedback (only for on-screen builds — otherwise the rival's off-screen town
        // thunks in your ear): a wooden "raised!" note + a villager cheer for the player's builds.
        if focus.in_earshot(pos) {
            let at = Vec3::new(pos.x, tf.translation.y, pos.y);
            cues.write(crate::audio::AudioCue::ChestOpen);
            if *side == Side::Player {
                speak.write(crate::audio::Speak::at(crate::audio::Concept::BuildRaised, at));
            }
        }
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

/// The textured part meshes for a building kind — each part carries the campaign `M` material slot
/// it renders with (the spawn systems attach the real textured material via `VillageMats`). Both
/// sides share these; friend/foe is the ground ring's job.
fn building_meshes(kind: BuildingKind) -> Vec<(Mesh, M)> {
    match kind {
        BuildingKind::TownHall => townhall_parts(),
        BuildingKind::Barracks => barracks_parts(),
        BuildingKind::House => house_parts(),
        BuildingKind::Wall => wall_parts(),
        BuildingKind::Watchtower => watchtower_parts(),
        BuildingKind::Market => market_parts(),
        // Gold Mine reuses the quarry model but swaps its stone slots for the gold-vein metal so it
        // reads as a gold seam, not a grey quarry.
        BuildingKind::GoldMine => {
            producer_parts(kind).into_iter().map(|(mesh, m)| (mesh, goldify(m))).collect()
        }
        _ => producer_parts(kind),
    }
}

/// Remap a stone `M` slot to the gold-vein metal (for the Gold Mine's ore).
fn goldify(m: M) -> M {
    match m {
        M::Stone | M::DarkStone | M::LightStone | M::HouseStone => M::Gold,
        other => other,
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

// ── decorative clutter ("durnostojki") — the low-poly props the buildings sit among, matched to
//    the prop style on the campaign producer meshes (barn/sawpit/log-yard). All base at y = 0. ──

/// A banded wooden barrel (cuboid — all building parts must share the Cuboid/Cone attribute set to
/// merge, so no Cylinder here).
fn barrel(x: f32, z: f32) -> Vec<(Mesh, M)> {
    vec![
        (cuboid(0.36, 0.52, 0.36, x, 0.26, z), M::Wood),
        (cuboid(0.4, 0.06, 0.4, x, 0.15, z), M::Beam),
        (cuboid(0.4, 0.06, 0.4, x, 0.4, z), M::Beam),
    ]
}

/// A small stack of two crates.
fn crate_stack(x: f32, z: f32) -> Vec<(Mesh, M)> {
    vec![
        (cuboid(0.5, 0.5, 0.5, x, 0.25, z), M::Wood),
        (cuboid(0.54, 0.06, 0.54, x, 0.25, z), M::Beam), // mid band
        (cuboid(0.4, 0.4, 0.4, x + 0.13, 0.7, z - 0.1), M::Wood),
    ]
}

/// A tied straw bale.
fn hay_bale(x: f32, z: f32) -> Vec<(Mesh, M)> {
    vec![
        (cuboid(0.72, 0.44, 0.46, x, 0.22, z), M::Straw),
        (cuboid(0.74, 0.07, 0.1, x, 0.22, z - 0.13), M::Beam),
        (cuboid(0.74, 0.07, 0.1, x, 0.22, z + 0.13), M::Beam),
    ]
}

/// A stack of split logs (running along X) with pale cut ends.
fn woodpile(x: f32, z: f32) -> Vec<(Mesh, M)> {
    let mut v = Vec::new();
    for (dz, dy) in [(-0.15_f32, 0.14_f32), (0.15, 0.14), (0.0, 0.35)] {
        v.push((cuboid(0.9, 0.22, 0.22, x, dy, z + dz), M::Wood));
        v.push((cuboid(0.05, 0.2, 0.2, x - 0.45, dy, z + dz), M::LightStone)); // pale cut end
    }
    v
}

/// **Town Hall** (footprint 4×4): a compact keep — a broad stone block under a timber upper storey
/// and a pyramidal roof, with corner merlons, a warm-glowing door + windows, a side banner, a corner
/// watchtower, crenellations, and a settlement's worth of clutter (barrels / crates / hay / a well).
fn townhall_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    // Footing skirt + stone block (top at 1.75).
    v.push((cuboid(2.9, 0.25, 2.9, 0.0, 0.125, 0.0), M::DarkStone));
    v.push((cuboid(2.6, 1.5, 2.6, 0.0, 1.0, 0.0), M::Stone));
    // Corner merlons ringing the stone top.
    for (sx, sz) in [(-1.05, -1.05), (1.05, -1.05), (-1.05, 1.05), (1.05, 1.05)] {
        v.push((cuboid(0.36, 0.4, 0.36, sx, 1.95, sz), M::Stone));
    }
    // Timber upper storey (top at 2.6) + pyramid roof (apex 3.7).
    v.push((cuboid(2.1, 0.85, 2.1, 0.0, 2.175, 0.0), M::Beam));
    v.push((pyramid(1.7, 1.1, 2.6), M::Roof));
    // Gold finial cube + banner pole & cloth.
    v.push((cuboid(0.24, 0.24, 0.24, 0.0, 3.8, 0.0), M::Gold));
    v.push((cuboid(0.08, 1.2, 0.08, 0.0, 4.3, 0.0), M::Beam));
    v.push((cuboid(0.55, 0.36, 0.05, 0.31, 4.6, 0.0), M::Banner));
    // Door (+Z) + flanking windows.
    v.push((cuboid(0.7, 0.9, 0.08, 0.0, 0.7, 1.31), M::Slit));
    for sx in [-0.8, 0.8] {
        v.push((cuboid(0.3, 0.4, 0.06, sx, 1.15, 1.32), M::Window));
    }
    // Crenellations ringing the stone block top (between the corner merlons).
    for sx in [-0.5_f32, 0.0, 0.5] {
        for sz in [-1.3_f32, 1.3] {
            v.push((cuboid(0.3, 0.28, 0.18, sx, 1.89, sz), M::Stone));
        }
    }
    for sz in [-0.5_f32, 0.0, 0.5] {
        for sx in [-1.3_f32, 1.3] {
            v.push((cuboid(0.18, 0.28, 0.3, sx, 1.89, sz), M::Stone));
        }
    }
    // Corner watchtower (−X −Z corner): a taller stone shaft with its own conical roof + a slit.
    let (tx, tz) = (-1.35, -1.35);
    v.push((cuboid(0.95, 3.2, 0.95, tx, 1.6, tz), M::Stone));
    for (dx, dz) in [(-0.42_f32, -0.42_f32), (0.42, -0.42), (-0.42, 0.42), (0.42, 0.42)] {
        v.push((cuboid(0.22, 0.3, 0.22, tx + dx, 3.3, tz + dz), M::Stone)); // tower merlons
    }
    v.push((pyramid(0.72, 0.85, 3.35), M::Roof));
    v.push((cuboid(0.12, 0.5, 0.06, tx, 2.4, tz + 0.5), M::Slit)); // slit
    // Clutter around the base (kept inside the 4×4 footprint, off the +Z door lane).
    v.extend(barrel(1.55, 1.4));
    v.extend(barrel(1.85, 1.05));
    v.extend(crate_stack(1.6, -1.4));
    v.extend(hay_bale(0.2, -1.7));
    v.extend(well_prop(-1.4, 1.5));
    v
}

/// A small stone draw-well prop (square stone ring + two posts + a beam + a little roof).
fn well_prop(x: f32, z: f32) -> Vec<(Mesh, M)> {
    vec![
        (cuboid(0.8, 0.5, 0.8, x, 0.25, z), M::Stone),
        (cuboid(0.6, 0.08, 0.6, x, 0.5, z), M::Slit), // dark water
        (cuboid(0.08, 1.0, 0.08, x - 0.42, 0.5, z), M::Wood),
        (cuboid(0.08, 1.0, 0.08, x + 0.42, 0.5, z), M::Wood),
        (cuboid(0.06, 0.06, 0.95, x, 1.0, z), M::Beam), // ridge beam
        (cuboid(0.95, 0.1, 0.7, x, 1.1, z), M::Wood),   // flat roof plate
    ]
}

/// **Barracks** (footprint 4×4): a long timber hall under a peaked roof, with a fronting door and a
/// weapon rack of iron shafts in the yard.
fn barracks_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    // Long plank hall on the −X half of the plot (yard on +X, like the campaign producers).
    v.push((cuboid(3.4, 1.2, 1.8, -0.2, 0.6, 0.0), M::Wood));
    for (sx, sz) in [(-1.9, -0.9), (1.5, -0.9), (-1.9, 0.9), (1.5, 0.9)] {
        v.push((cuboid(0.14, 1.3, 0.14, sx, 0.65, sz), M::Beam));
    }
    // Peaked roof: two slabs tilted about X meeting at the ridge.
    let a = 0.5;
    v.push((
        Mesh::from(Cuboid::new(3.7, 0.12, 1.25))
            .rotated_by(Quat::from_rotation_x(a))
            .translated_by(Vec3::new(-0.2, 1.5, 0.45)),
        M::HouseRoof,
    ));
    v.push((
        Mesh::from(Cuboid::new(3.7, 0.12, 1.25))
            .rotated_by(Quat::from_rotation_x(-a))
            .translated_by(Vec3::new(-0.2, 1.5, -0.45)),
        M::HouseRoof,
    ));
    // Door (+Z front) + a side banner.
    v.push((cuboid(0.7, 0.9, 0.08, -0.2, 0.45, 0.91), M::Slit));
    v.push((cuboid(0.42, 0.32, 0.05, -1.9, 1.5, 0.0), M::Banner));
    // Weapon rack in the +X yard: two posts, a top rail, three leaning iron shafts.
    let rx = 1.4;
    v.push((cuboid(0.08, 0.9, 0.08, rx - 0.5, 0.45, 0.6), M::Beam));
    v.push((cuboid(0.08, 0.9, 0.08, rx + 0.5, 0.45, 0.6), M::Beam));
    v.push((cuboid(1.1, 0.08, 0.08, rx, 0.88, 0.6), M::Beam));
    for dx in [-0.3, 0.0, 0.3] {
        v.push((cuboid(0.05, 1.05, 0.05, rx + dx, 0.55, 0.6), M::Iron));
    }
    // Training pell: a stout post with a crossbar + a straw-bound head, in the yard.
    let (px, pz) = (1.5, -0.75);
    v.push((cuboid(0.16, 1.5, 0.16, px, 0.75, pz), M::Beam));
    v.push((cuboid(0.9, 0.12, 0.12, px, 1.15, pz), M::Beam)); // arms
    v.push((cuboid(0.26, 0.3, 0.26, px, 1.55, pz), M::Straw)); // straw head
    // Two shields leaning on the hall's front wall.
    for (sx, m) in [(-1.4_f32, M::Iron), (-1.0, M::Banner)] {
        v.push((
            cuboid(0.44, 0.5, 0.08, 0.0, 0.0, 0.0)
                .rotated_by(Quat::from_rotation_z(0.14))
                .translated_by(Vec3::new(sx, 0.52, 0.95)),
            m,
        ));
        v.push((cuboid(0.12, 0.12, 0.05, sx, 0.52, 1.0), M::Bronze)); // boss
    }
    // Yard clutter.
    v.extend(barrel(0.6, 0.75));
    v.extend(hay_bale(1.75, 0.7));
    v
}

/// **House** (footprint 2×2): a small plastered cottage with corner beams, a pyramid roof, a door
/// and a lit window.
fn house_parts() -> Vec<(Mesh, M)> {
    let mut v: Vec<(Mesh, M)> = Vec::new();
    v.push((cuboid(1.5, 0.9, 1.3, 0.0, 0.45, 0.0), M::Plaster));
    for (sx, sz) in [(-0.75, -0.65), (0.75, -0.65), (-0.75, 0.65), (0.75, 0.65)] {
        v.push((cuboid(0.14, 0.9, 0.14, sx, 0.45, sz), M::Beam));
    }
    // Cross-timbering on the front wall (the half-timbered look) + a sill under the window.
    v.push((cuboid(1.5, 0.1, 0.05, 0.0, 0.62, 0.66), M::Beam));
    v.push((cuboid(0.34, 0.06, 0.12, 0.5, 0.46, 0.68), M::Beam)); // window box
    v.push((pyramid(1.15, 0.7, 0.9), M::HouseRoof));
    v.push((cuboid(0.4, 0.6, 0.06, 0.0, 0.3, 0.66), M::Slit));
    v.push((cuboid(0.3, 0.3, 0.06, 0.5, 0.62, 0.67), M::Window));
    // Chimney (−X back corner) with a stone cap.
    v.push((cuboid(0.24, 1.4, 0.24, -0.5, 0.7, -0.4), M::HouseStone));
    v.push((cuboid(0.32, 0.1, 0.32, -0.5, 1.42, -0.4), M::DarkStone));
    // Cottage clutter (kept tight to the 2×2 footprint).
    v.extend(barrel(0.9, 0.9));
    v.extend(woodpile(-0.85, -0.1));
    v
}

/// **Wall** (footprint 2×2): a battlemented stone segment. No function beyond its collision box —
/// place a row of them to fence off a lane.
fn wall_parts() -> Vec<(Mesh, M)> {
    let mut v = vec![
        (cuboid(1.9, 0.2, 0.95, 0.0, 0.1, 0.0), M::DarkStone), // footing
        (cuboid(1.7, 1.6, 0.7, 0.0, 0.9, 0.0), M::Stone),      // wall body
    ];
    // Crenellations along the top.
    for sx in [-0.6_f32, 0.0, 0.6] {
        v.push((cuboid(0.34, 0.32, 0.7, sx, 1.86, 0.0), M::Stone));
    }
    v
}

/// **Watchtower** (footprint 2×2): a tall stone shaft with a battlemented timber crown + a conical
/// roof and arrow slits. Looses arrows at nearby enemies (`units::watchtower_fire`).
fn watchtower_parts() -> Vec<(Mesh, M)> {
    let mut v = vec![
        (cuboid(1.15, 0.25, 1.15, 0.0, 0.12, 0.0), M::DarkStone), // footing
        (cuboid(0.95, 3.5, 0.95, 0.0, 1.75, 0.0), M::Stone),      // shaft
        (cuboid(1.35, 0.4, 1.35, 0.0, 3.6, 0.0), M::Beam),        // overhanging timber platform
    ];
    // Timber crenellations around the platform.
    for (sx, sz) in [(-0.58_f32, -0.58_f32), (0.58, -0.58), (-0.58, 0.58), (0.58, 0.58), (0.0, -0.62), (0.0, 0.62), (-0.62, 0.0), (0.62, 0.0)] {
        v.push((cuboid(0.24, 0.36, 0.24, sx, 3.95, sz), M::Beam));
    }
    // Arrow slits on the four faces.
    for (sx, sz, w, d) in [(0.0, 0.49, 0.14, 0.04), (0.0, -0.49, 0.14, 0.04), (0.49, 0.0, 0.04, 0.14), (-0.49, 0.0, 0.04, 0.14)] {
        v.push((cuboid(w, 0.8, d, sx, 2.3, sz), M::Slit));
    }
    v.push((pyramid(0.75, 1.0, 4.1), M::Roof)); // conical cap
    v.push((cuboid(0.06, 0.9, 0.06, 0.0, 5.1, 0.0), M::Beam)); // flag pole
    v.push((cuboid(0.5, 0.32, 0.04, 0.28, 5.4, 0.0), M::Banner)); // flag
    v
}

/// **Market** (footprint 3×3): a striped-awning stall over a goods counter, barrels + crates of
/// wares around it. Trickles passive gold (`workers::market_income`).
fn market_parts() -> Vec<(Mesh, M)> {
    let mut v = Vec::new();
    // Counter (a plank table) along the −Z front.
    v.push((cuboid(2.6, 0.7, 0.8, 0.0, 0.35, -0.7), M::Wood));
    v.push((cuboid(2.7, 0.08, 0.9, 0.0, 0.72, -0.7), M::Beam)); // counter top edge
    // Four posts + a striped cloth awning.
    for (sx, sz) in [(-1.25_f32, -1.15_f32), (1.25, -1.15), (-1.25, 1.05), (1.25, 1.05)] {
        v.push((cuboid(0.12, 1.9, 0.12, sx, 0.95, sz), M::Beam));
    }
    v.push((cuboid(2.9, 0.1, 2.5, 0.0, 1.95, -0.05), M::Banner)); // awning cloth
    // Wares on the counter: sacks + produce.
    v.push((cuboid(0.4, 0.36, 0.36, -0.7, 0.9, -0.7), M::Straw));
    v.push((cuboid(0.42, 0.3, 0.4, 0.1, 0.88, -0.7), M::Crop));
    v.push((cuboid(0.3, 0.3, 0.3, 0.7, 0.86, -0.7), M::Hen));
    // Goods stacked in the yard behind.
    v.extend(crate_stack(-1.0, 0.7));
    v.extend(barrel(1.0, 0.7));
    v
}
