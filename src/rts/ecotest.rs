//! `FOREST_RTS_ECOTEST=<secs>` — headless-ish skirmish **economy smoke test** (the "worker never
//! gets stuck" gate). Boots the real game in skirmish, auto-places a player Sawmill beside the
//! nearest wood grove and a Farm beside the base, then watches the live sim:
//!
//! - **FAIL fast** if any bonded worker stops moving for [`STUCK_SECS`] while not gathering/tending
//!   (a stuck mover / unreachable goal — exactly the regression the test exists to catch);
//! - at the deadline, **PASS** iff the player bank gathered BOTH wood and food (i.e. the
//!   lumberjack completed chop→carry→bank trips to the sawmill, and the farmer worked the farm).
//!
//! Prints one `RTS_ECOTEST OK|FAIL ...` line and exits with the matching process code, so a script
//! (or an agent) can run `FOREST_RTS=1 FOREST_RTS_ECOTEST=90 cargo run` and check `$LASTEXITCODE`.
//! Pure staging/verification — registers nothing unless the env var is set.

use std::collections::HashMap;

use bevy::app::AppExit;
use bevy::prelude::*;

use crate::game_state::{AppState, Modal};
use crate::rts::command::MoveTo;
use crate::rts::workers::Assigned;
use crate::rts::{
    base_of, build, in_skirmish, BuildingKind, Deposit, DepositKind, RtsBanks, RtsBuilding,
    RtsUnit, Side, UnitKind,
};

/// A worker carrying `MoveTo` that hasn't displaced for this long is stuck (gather/tend phases
/// stand still but hold no `MoveTo`, so they don't trip this).
const STUCK_SECS: f32 = 30.0;
/// Movement below this (world units) between samples counts as "not moving".
const MOVE_EPS: f32 = 0.25;

#[derive(Resource)]
struct EcoTest {
    /// Sim-seconds to run after staging before the verdict.
    duration: f32,
    staged: bool,
    /// Elapsed at staging + the bank snapshot taken right AFTER paying for the staged buildings.
    started: f32,
    base_wood: f64,
    base_food: f64,
    /// Per-worker (last position, elapsed at last real displacement) for the stuck watchdog.
    track: HashMap<Entity, (Vec2, f32)>,
}

pub struct RtsEcoTestPlugin;

impl Plugin for RtsEcoTestPlugin {
    fn build(&self, app: &mut App) {
        let Ok(v) = std::env::var("FOREST_RTS_ECOTEST") else { return };
        let duration = v.parse::<f32>().unwrap_or(90.0);
        app.insert_resource(EcoTest {
            duration,
            staged: false,
            started: 0.0,
            base_wood: 0.0,
            base_food: 0.0,
            track: HashMap::new(),
        })
        .add_systems(
            Update,
            ecotest_drive
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing))
                .run_if(in_state(Modal::None)),
        );
    }
}

// Spot search lives in `build::find_spot` (shared with the RC bridge's auto-spot build op).

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn ecotest_drive(
    time: Res<Time>,
    mut commands: Commands,
    mut banks: ResMut<RtsBanks>,
    mut test: ResMut<EcoTest>,
    assets: Option<Res<build::RtsBuildAssets>>,
    halls: Query<(&RtsBuilding, &Side)>,
    deposits: Query<(&Deposit, &Transform)>,
    workers: Query<(Entity, &RtsUnit, &Side, &Transform, Has<Assigned>, Has<MoveTo>)>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs();

    // ── stage once the player's hall (and the build assets + deposits) exist ──
    if !test.staged {
        let Some(assets) = assets.as_ref() else { return };
        let hall_up = halls
            .iter()
            .any(|(b, s)| b.kind == BuildingKind::TownHall && b.built && *s == Side::Player);
        let dep_pos: Vec<Vec2> =
            deposits.iter().map(|(_, t)| Vec2::new(t.translation.x, t.translation.z)).collect();
        if !hall_up || dep_pos.is_empty() {
            return;
        }
        let base = base_of(Side::Player);
        // Sawmill next to the wood grove nearest the base (the intended play pattern).
        let grove = deposits
            .iter()
            .filter(|(d, _)| d.kind == DepositKind::Wood && d.remaining > 0.0)
            .map(|(_, t)| Vec2::new(t.translation.x, t.translation.z))
            .min_by(|a, b| {
                a.distance_squared(base).partial_cmp(&b.distance_squared(base)).unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some(grove) = grove else { return };
        let mill = build::find_spot(BuildingKind::Sawmill, Side::Player, grove, &dep_pos);
        let farm = build::find_spot(BuildingKind::Farm, Side::Player, base, &dep_pos);
        let (Some(mill), Some(farm)) = (mill, farm) else {
            println!("RTS_ECOTEST FAIL: no valid staging spot (mill/farm)");
            exit.write(AppExit::error());
            return;
        };
        let ok_m = build::try_place(&mut commands, assets, &mut banks, &dep_pos, BuildingKind::Sawmill, Side::Player, mill, 0);
        let ok_f = build::try_place(&mut commands, assets, &mut banks, &dep_pos, BuildingKind::Farm, Side::Player, farm, 0);
        if !ok_m || !ok_f {
            println!("RTS_ECOTEST FAIL: try_place refused (mill={ok_m} farm={ok_f})");
            exit.write(AppExit::error());
            return;
        }
        // Baseline AFTER paying the costs — any later increase is hauled income.
        test.base_wood = banks.side(Side::Player).wood;
        test.base_food = banks.side(Side::Player).food;
        test.started = now;
        test.staged = true;
        println!("RTS_ECOTEST staged: sawmill@{mill:?} farm@{farm:?}, running {}s", test.duration);
        return;
    }

    // ── stuck watchdog: a MoveTo-carrying player worker must displace within STUCK_SECS ──
    for (e, u, side, tf, assigned, moving) in &workers {
        if *side != Side::Player || u.kind != UnitKind::Worker || !assigned {
            continue;
        }
        let pos = Vec2::new(tf.translation.x, tf.translation.z);
        let entry = test.track.entry(e).or_insert((pos, now));
        if pos.distance(entry.0) > MOVE_EPS {
            *entry = (pos, now);
        } else if moving && now - entry.1 > STUCK_SECS {
            println!("RTS_ECOTEST FAIL: worker {e} stuck at {pos:?} for {STUCK_SECS}s (MoveTo held)");
            exit.write(AppExit::error());
            return;
        }
    }

    // ── verdict at the deadline ──
    if now - test.started >= test.duration {
        let wood = banks.side(Side::Player).wood - test.base_wood;
        let food = banks.side(Side::Player).food - test.base_food;
        // NB food also drains at FOOD_DRAIN per living unit — a positive delta means the farmer
        // out-gathered the upkeep, which is the intended healthy-economy bar.
        if wood > 0.0 && food > 0.0 {
            println!("RTS_ECOTEST OK wood=+{wood:.0} food=+{food:.0}");
            exit.write(AppExit::Success);
        } else {
            println!("RTS_ECOTEST FAIL wood=+{wood:.0} food=+{food:.0} (both must be > 0)");
            exit.write(AppExit::error());
        }
    }
}
