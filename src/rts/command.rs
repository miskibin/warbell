//! Order routing + **the shared unit mover**.
//!
//! This module owns the movement vocabulary the whole RTS depends on — `MoveTo`, `AttackTarget`,
//! `HarvestAt`, `unit_speed`, and `rts_move_units` — so `workers.rs` (haul loop) and wave-3's
//! `units.rs`/`ai.rs` (combat brains) drive units by inserting these EXACT components rather than
//! each rolling their own locomotion. Player input (RMB context command + F attack-move latch) and
//! the AI both emit `RtsOrder` messages; `rts_consume_orders` fans them into per-unit `MoveTo`
//! goals; `rts_move_units` walks every `MoveTo` entity along an A* `NavPath` (staggered replans),
//! exactly like `villagers::worker_steer` / the siege invaders.
//!
//! Contract for spawners (workers.rs / units.rs): **every `RtsUnit` must be spawned with a
//! `NavPath` component** — the mover requires it, and issuing a `MoveTo` without one leaves the
//! unit motionless. To make a unit walk somewhere from any system, insert `MoveTo { goal, fight }`
//! (the mover clears it on arrival). `AttackTarget` / `HarvestAt` are carried for wave-3 / workers
//! to consume; this module only inserts/clears them.

use bevy::math::EulerRot;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::dying::Dying;
use crate::game_state::{AppState, Modal};
use crate::navgrid::{path_to_budget, NavPath};
use crate::rts::pick;
use crate::rts::{Deposit, Order, Placing, RtsBuilding, RtsOrder, RtsUnit, Selected, Side, UnitKind};
use crate::steer;

// ── shared movement vocabulary (sibling workers.rs + wave-3 units.rs/ai.rs use these EXACT names) ──

/// Walk to `goal`. `fight = true` = attack-move: wave-3 brains read it to engage hostiles en route;
/// the mover itself just carries it and drives locomotion.
#[derive(Component)]
pub struct MoveTo {
    pub goal: Vec2,
    pub fight: bool,
}

/// Explicit attack order — the enemy this unit was told to kill. Wave-3 combat brains consume it;
/// this module inserts it (on an `Order::Attack`) and clears it (on a plain move/harvest).
#[derive(Component)]
pub struct AttackTarget(pub Entity);

/// A worker's harvest reassignment — the `Deposit` entity it should haul from. `workers.rs` owns
/// the haul state machine and consumes this; the command layer only forwards the player's pick.
#[derive(Component)]
pub struct HarvestAt(pub Entity);

/// Attack-move latch (armed by `F`, consumed by the next LMB/RMB). A shared resource so `select.rs`
/// suppresses click-select while it's armed (LMB then means attack-move, not selection).
///
/// NB deliberately NOT the RTS-conventional `A`: WASD pans the iso camera (`camera.rs`), so an `A`
/// latch armed on every pan-left and silently ate the next click — the "selection doesn't work"
/// bug. `F` (fight) is free of the camera cluster.
#[derive(Resource, Default)]
pub struct AttackMove(pub bool);

/// Ground move speed (world units/s) per unit kind — spec §7 / task. Faster than campaign villagers
/// (guard ≈2.9) because the iso camera is zoomed out and RTS orders must feel responsive.
pub fn unit_speed(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Worker => 4.2,
        UnitKind::Swordsman | UnitKind::Archer => 4.8,
    }
}

// ── mover tuning ──
/// Collision footprint radius fed to `steer` (matches the campaign villager body).
const BODY_R: f32 = 0.35;
/// Within this distance of the goal a unit has arrived — `MoveTo` is removed.
const ARRIVE: f32 = 0.6;
/// Beyond this range, follow the cached A* `NavPath`; within it, cheap direct steer (no path churn).
const PATH_RANGE: f32 = 4.0;
/// A* node budget for the RTS mover. The default `NAV_MAX_NODES` (8400) is sized for the ~40-tile
/// keep march and **exhausts → empty on cross-arena trips** (its own doc says so) — the two bases sit
/// ~80 tiles apart diagonally, so an "attack the enemy base" order drained the budget, got an empty
/// path, and the army stalled boxed in by its own buildings. This budget covers a full arena crossing.
const RTS_NAV_BUDGET: u32 = 60_000;
/// Max facing slew (rad/s) — snappier than villagers (RTS units pivot briskly).
const MAX_TURN: f32 = 6.0;
/// Phyllotaxis group fan-out: slot `n` sits at radius `SPREAD·√n`, angle `n·GOLDEN` around the goal
/// (spacing ≈1.1u, reusing the rally-blob idea so a 30-unit order doesn't stack on one tile).
const GROUP_SPREAD: f32 = 1.1;
const GROUP_GOLDEN: f32 = 2.399_963_2;

pub struct RtsCommandPlugin;

impl Plugin for RtsCommandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AttackMove>().add_systems(
            Update,
            (rts_issue_orders, rts_consume_orders, rts_move_units)
                .chain()
                .run_if(super::in_skirmish)
                .run_if(in_state(AppState::Playing))
                .run_if(in_state(Modal::None)),
        );
    }
}

/// Player input → `RtsOrder`. RMB with a unit selection: enemy unit/building → Attack, deposit →
/// Harvest, else ground → Move. `F` latches attack-move for the next LMB/RMB → AttackMove.
#[allow(clippy::too_many_arguments)]
fn rts_issue_orders(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    placing: Res<Placing>,
    mut attack: ResMut<AttackMove>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    selected: Query<Entity, (With<Selected>, With<RtsUnit>)>,
    enemy_units: Query<(Entity, &GlobalTransform, &Side), (With<RtsUnit>, Without<Dying>)>,
    enemy_buildings: Query<(Entity, &GlobalTransform, &Side), (With<RtsBuilding>, Without<Dying>)>,
    deposits: Query<(Entity, &GlobalTransform), With<Deposit>>,
    mut orders: MessageWriter<RtsOrder>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
) {
    // Build placement owns the pointer; drop the latch and bail.
    if placing.0.is_some() {
        attack.0 = false;
        return;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        attack.0 = true; // arm attack-move for the next click (F — see the AttackMove doc: A pans)
    }

    let Ok(win) = windows.single() else { return };
    let Ok((camera, cam_tf)) = camera.single() else { return };
    let Some(cursor) = win.cursor_position() else { return };
    if pick::over_hud(cursor, win.height()) {
        return; // click belongs to the HUD
    }

    let units: Vec<Entity> = selected.iter().collect();

    // Attack-move: A then LMB or RMB on ground.
    if attack.0 {
        if mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right) {
            attack.0 = false;
            if !units.is_empty() {
                if let Some(g) = pick::cursor_ray_ground(camera, cam_tf, cursor) {
                    orders.write(RtsOrder { units, order: Order::AttackMove(g) });
                    cues.write(crate::audio::AudioCue::UiSelect);
                }
            }
        }
        return; // while armed, LMB is an order, not a selection (select.rs early-returns on the latch)
    }

    // Normal RMB context command.
    if mouse.just_pressed(MouseButton::Right) && !units.is_empty() {
        // The player's foe is the Rival side (only Rival targets are attackable/harvest is neutral).
        let hit_unit = pick::nearest_within(
            camera,
            cam_tf,
            cursor,
            pick::UNIT_PICK_PX,
            enemy_units
                .iter()
                .filter(|(_, _, s)| **s == Side::Rival)
                // Mid-chest pick point, same as select.rs (feet project below the visual body).
                .map(|(e, gt, _)| (e, gt.translation() + Vec3::Y * pick::UNIT_PICK_Y)),
        );
        let hit_bld = pick::nearest_within(
            camera,
            cam_tf,
            cursor,
            pick::BUILDING_PICK_PX,
            enemy_buildings.iter().filter(|(_, _, s)| **s == Side::Rival).map(|(e, gt, _)| (e, gt.translation())),
        );
        let hit_dep = pick::nearest_within(
            camera,
            cam_tf,
            cursor,
            pick::DEPOSIT_PICK_PX,
            deposits.iter().map(|(e, gt)| (e, gt.translation())),
        );

        let order = if let Some(e) = hit_unit.or(hit_bld) {
            Order::Attack(e)
        } else if let Some(e) = hit_dep {
            Order::Harvest(e)
        } else if let Some(g) = pick::cursor_ray_ground(camera, cam_tf, cursor) {
            Order::Move(g)
        } else {
            return;
        };
        orders.write(RtsOrder { units, order });
        cues.write(crate::audio::AudioCue::UiSelect);
    }
}

/// Goal for group slot `n` of `count` — the phyllotaxis blob around `goal` (single unit → exact goal).
fn group_goal(goal: Vec2, slot: usize, count: usize) -> Vec2 {
    if count <= 1 {
        return goal;
    }
    let r = GROUP_SPREAD * (slot as f32).sqrt();
    let a = slot as f32 * GROUP_GOLDEN;
    goal + Vec2::new(r * a.cos(), r * a.sin())
}

/// Consume `RtsOrder`s into per-unit movement components. A fresh `NavPath` is (re)inserted so each
/// order re-paths cleanly; conflicting order-components are cleared.
fn rts_consume_orders(
    mut orders: MessageReader<RtsOrder>,
    mut commands: Commands,
    transforms: Query<&GlobalTransform>,
    kinds: Query<&RtsUnit>,
) {
    for ord in orders.read() {
        match ord.order {
            Order::Move(goal) | Order::AttackMove(goal) => {
                let fight = matches!(ord.order, Order::AttackMove(_));
                let n = ord.units.len();
                for (i, &e) in ord.units.iter().enumerate() {
                    let g = group_goal(goal, i, n);
                    commands.entity(e).try_insert((MoveTo { goal: g, fight }, NavPath::default()));
                    commands.entity(e).try_remove::<AttackTarget>();
                    commands.entity(e).try_remove::<HarvestAt>();
                }
            }
            Order::Attack(target) => {
                // Start walking toward the target's current position now; wave-3's brain refines the
                // chase from `AttackTarget`.
                let Some(tp) = transforms.get(target).ok().map(|gt| gt.translation()) else { continue };
                let goal = Vec2::new(tp.x, tp.z);
                for &e in &ord.units {
                    commands.entity(e).try_insert((AttackTarget(target), MoveTo { goal, fight: true }, NavPath::default()));
                    commands.entity(e).try_remove::<HarvestAt>();
                }
            }
            Order::Harvest(dep) => {
                // Only workers harvest; forward the pick to workers.rs via `HarvestAt` and start them
                // walking to the deposit now.
                let Some(dp) = transforms.get(dep).ok().map(|gt| gt.translation()) else { continue };
                let goal = Vec2::new(dp.x, dp.z);
                for &e in &ord.units {
                    if kinds.get(e).map(|u| u.kind) != Ok(UnitKind::Worker) {
                        continue;
                    }
                    commands.entity(e).try_insert((HarvestAt(dep), MoveTo { goal, fight: false }, NavPath::default()));
                    commands.entity(e).try_remove::<AttackTarget>();
                }
            }
        }
    }
}

/// THE shared mover: walk every `(RtsUnit, MoveTo)` along its A* `NavPath` (staggered replans),
/// riding terrain height + facing travel, removing `MoveTo` on arrival. Same far-path / near-steer
/// split as `worker_steer`, so ~60 units don't hitch on a group order.
#[allow(clippy::type_complexity)]
fn rts_move_units(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &RtsUnit, &MoveTo, &mut Transform, &mut NavPath), Without<Dying>>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    for (e, unit, mv, mut tf, mut path) in &mut q {
        let pos = Vec2::new(tf.translation.x, tf.translation.z);
        let goal = mv.goal;
        let dist = pos.distance(goal);
        if dist < ARRIVE {
            commands.entity(e).try_remove::<MoveTo>();
            path.waypoints.clear();
            path.cursor = 0;
            continue;
        }

        let facing = tf.rotation.to_euler(EulerRot::YXZ).0;
        let cur_y = steer::footing(pos.x, pos.y).unwrap_or(tf.translation.y);

        // Far: follow the cached A* route (threads any blockers); close: cheap direct steer.
        let step_target = if dist > PATH_RANGE {
            if path.cursor >= path.waypoints.len()
                || now >= path.next_replan
                || path.goal_cached.distance(goal) > 2.0
            {
                path.waypoints = path_to_budget(pos, goal, RTS_NAV_BUDGET);
                path.cursor = 0;
                path.goal_cached = goal;
                // Stagger replans so a group order doesn't A* everyone on one frame.
                path.next_replan = now + 0.6 + (e.to_bits() % 16) as f32 * 0.05;
            }
            while path.cursor < path.waypoints.len() && pos.distance(path.waypoints[path.cursor]) < 1.2 {
                path.cursor += 1;
            }
            path.waypoints.get(path.cursor).copied().unwrap_or(goal)
        } else {
            path.waypoints.clear();
            path.cursor = 0;
            goal
        };

        let speed = unit_speed(unit.kind);
        if let Some(s) = steer::advance(pos, facing, step_target, speed * dt, BODY_R, cur_y, MAX_TURN * dt) {
            let gy = steer::footing(s.pos.x, s.pos.y).unwrap_or(cur_y);
            tf.translation = Vec3::new(s.pos.x, gy, s.pos.y);
            tf.rotation = Quat::from_rotation_y(s.facing);
        }
    }
}
