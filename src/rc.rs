//! `FOREST_RC=<dir>` — **remote-control bridge** for agent/manual testing (the "Unity-MCP style"
//! debug seam). File-based, zero new deps, works from any tool that can read/write JSON files:
//!
//! - The game polls `<dir>/cmd.json` every frame. Drop a JSON command (or `{"seq":N,"ops":[...]}`
//!   batch) there; the game executes it, deletes the file, and echoes per-op results + the seq in
//!   the next state snapshot. A half-written file is retried for ~2s before being rejected.
//! - The game writes `<dir>/state.json` (atomic tmp+rename) every ~0.5s: fps, app/modal state,
//!   sky clock, campaign siege phase + hero vitals, and in Skirmish the full RTS picture — banks,
//!   population, every building/unit/deposit with entity ids usable in follow-up orders.
//! - Every ~2s a compact line is appended to `<dir>/log.jsonl` (fps + economy/army counts) so a
//!   run leaves a greppable timeline for post-mortems.
//!
//! Op reference lives in the skill: `.claude/skills/game-rc/SKILL.md`. Ops are SEMANTIC (place a
//! building, train, order units, set speed, screenshot) — not synthetic input events — so they
//! route through the same entry points the real UI uses (`build::try_place`, `RtsOrder`,
//! `TrainOrder`) and can't drift from the game's own rules.

use std::path::PathBuf;

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use serde_json::{json, Value};

use crate::dying::Dying;
use crate::game_state::{AppState, Modal};
use crate::rts::{
    build, Deposit, GameMode, Order, RtsBanks, RtsBuilding, RtsOrder, RtsOutcome, RtsPop, RtsUnit,
    Selected, Side, TrainOrder, TrainQueue, UnitKind,
};

/// Where the bridge reads `cmd.json` / writes `state.json` + `log.jsonl`.
#[derive(Resource)]
struct RcDir(PathBuf);

/// Per-run bridge scratch (all timings in REAL seconds).
#[derive(Default)]
struct RcScratch {
    fps_ema: f32,
    last_state: f32,
    last_log: f32,
    /// Results of the most recent command batch, echoed into every state write.
    results: Vec<Value>,
    seq_done: u64,
    /// First time a cmd.json failed to parse (half-written file grace window).
    bad_since: Option<f32>,
}

pub struct RcPlugin;

impl Plugin for RcPlugin {
    fn build(&self, app: &mut App) {
        let Ok(dir) = std::env::var("FOREST_RC") else { return };
        let dir = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        // Stale files from a previous run would read as live state — sweep them.
        for f in ["cmd.json", "state.json", "log.jsonl"] {
            let _ = std::fs::remove_file(dir.join(f));
        }
        info!("RC bridge active: {}", dir.display());
        app.insert_resource(RcDir(dir)).add_systems(Update, rc_tick);
    }
}

/// Clocks + coarse mode/state (bundled — the flat param list blows Bevy's 16-param system cap).
#[derive(bevy::ecs::system::SystemParam)]
struct RcClock<'w> {
    time: Res<'w, Time>,
    real: Res<'w, Time<Real>>,
    virt: ResMut<'w, Time<Virtual>>,
    app_state: Res<'w, State<AppState>>,
    /// `Modal` is a SUB-state that only exists inside `Playing` — on the GameOver transition the
    /// resource is removed, so a plain `Res` here panics the bridge exactly at match end
    /// (found by losing a live playtest match).
    modal: Option<Res<'w, State<Modal>>>,
    mode: Res<'w, GameMode>,
}

/// Campaign-side snapshot sources (all optional — absent until their plugins init).
#[derive(bevy::ecs::system::SystemParam)]
struct RcCampaign<'w> {
    sky: Option<Res<'w, crate::scene::SkyClock>>,
    siege: Option<Res<'w, crate::siege::Siege>>,
    hero: Option<Res<'w, crate::player::HeroState>>,
    player: Option<Res<'w, crate::player::PlayerRes>>,
}

/// RTS-side snapshot sources + the world queries ops act on.
#[derive(bevy::ecs::system::SystemParam)]
struct RcRts<'w, 's> {
    banks: ResMut<'w, RtsBanks>,
    pop: Res<'w, RtsPop>,
    outcome: Res<'w, RtsOutcome>,
    build_assets: Option<Res<'w, build::RtsBuildAssets>>,
    units_q: Query<
        'w,
        's,
        (Entity, &'static RtsUnit, &'static Side, &'static Transform, Option<&'static crate::player::Health>, Has<crate::rts::workers::Assigned>, Has<Selected>),
        Without<Dying>,
    >,
    bldg_q: Query<
        'w,
        's,
        (Entity, &'static RtsBuilding, &'static Side, &'static Transform, Option<&'static crate::player::Health>, Has<TrainQueue>),
        Without<Dying>,
    >,
    dep_q: Query<'w, 's, (Entity, &'static Deposit, &'static Transform)>,
}

/// Side-effect channels ops write into.
#[derive(bevy::ecs::system::SystemParam)]
struct RcEmit<'w, 's> {
    orders: MessageWriter<'w, RtsOrder>,
    trains: MessageWriter<'w, TrainOrder>,
    exit: MessageWriter<'w, AppExit>,
    commands: Commands<'w, 's>,
}

fn rc_tick(
    dir: Res<RcDir>,
    mut clock: RcClock,
    camp: RcCampaign,
    mut rts: RcRts,
    mut emit: RcEmit,
    mut scratch: Local<RcScratch>,
) {
    let now = clock.real.elapsed_secs();
    let rdt = clock.real.delta_secs().max(1e-5);
    // Smoothed fps off the REAL clock (virtual speed-ups must not read as fps changes).
    let inst = 1.0 / rdt;
    scratch.fps_ema = if scratch.fps_ema <= 0.0 { inst } else { scratch.fps_ema * 0.95 + inst * 0.05 };

    // ── commands ──
    let cmd_path = dir.0.join("cmd.json");
    if let Ok(txt) = std::fs::read_to_string(&cmd_path) {
        match serde_json::from_str::<Value>(&txt) {
            Ok(v) => {
                scratch.bad_since = None;
                let _ = std::fs::remove_file(&cmd_path);
                let (seq, ops): (u64, Vec<Value>) = match &v {
                    Value::Array(a) => (scratch.seq_done + 1, a.clone()),
                    Value::Object(o) if o.contains_key("ops") => (
                        o.get("seq").and_then(|s| s.as_u64()).unwrap_or(scratch.seq_done + 1),
                        o["ops"].as_array().cloned().unwrap_or_default(),
                    ),
                    _ => (scratch.seq_done + 1, vec![v.clone()]),
                };
                scratch.results = ops
                    .iter()
                    .map(|op| run_op(op, &mut clock.virt, &clock.mode, &mut rts, &mut emit))
                    .collect();
                scratch.seq_done = seq;
            }
            Err(e) => {
                // Possibly a half-written file — give the writer ~2s, then reject it.
                let first = *scratch.bad_since.get_or_insert(now);
                if now - first > 2.0 {
                    let _ = std::fs::remove_file(&cmd_path);
                    scratch.results = vec![json!({"ok": false, "err": format!("cmd.json parse: {e}")})];
                    scratch.seq_done += 1;
                    scratch.bad_since = None;
                }
            }
        }
    }

    // ── state snapshot ──
    if now - scratch.last_state >= 0.5 {
        scratch.last_state = now;
        let skirmish = *clock.mode == GameMode::Skirmish;
        let mut state = json!({
            "seq_done": scratch.seq_done,
            "results": scratch.results,
            "fps": (scratch.fps_ema * 10.0).round() / 10.0,
            "real_time": (now * 10.0).round() / 10.0,
            "sim_time": (clock.time.elapsed_secs() * 10.0).round() / 10.0,
            "speed": clock.virt.relative_speed(),
            "app_state": format!("{:?}", clock.app_state.get()),
            "modal": clock.modal.as_ref().map(|m| format!("{:?}", m.get())),
            "mode": if skirmish { "Skirmish" } else { "Campaign" },
        });
        if let Some(sky) = &camp.sky {
            state["sky"] = json!({"t": (sky.t * 1000.0).round() / 1000.0, "day_secs": sky.day_secs, "paused": sky.paused});
        }
        if !skirmish {
            let mut c = json!({});
            if let Some(s) = &camp.siege {
                c["phase"] = json!(format!("{:?}", s.phase));
                c["wave_index"] = json!(s.wave_index);
            }
            if let Some(h) = &camp.hero {
                c["hero"] = json!({"x": r1(h.pos.x), "z": r1(h.pos.y), "alive": h.alive});
            }
            if let Some(p) = &camp.player {
                c["player"] = json!({"hp": p.0.hp.round(), "max_hp": p.0.max_hp, "gold": p.0.gold, "level": p.0.level});
            }
            state["campaign"] = c;
        } else {
            let bank_json = |b: &crate::rts::RtsBank| {
                json!({"wood": b.wood.round(), "stone": b.stone.round(), "gold": b.gold.round(), "food": b.food.round()})
            };
            let units: Vec<Value> = rts
                .units_q
                .iter()
                .map(|(e, u, s, t, hp, assigned, sel)| {
                    json!({
                        "id": e.to_bits().to_string(),
                        "side": side_ch(*s),
                        "kind": format!("{:?}", u.kind),
                        "x": r1(t.translation.x), "z": r1(t.translation.z),
                        "hp": hp.map(|h| h.hp.round()),
                        "working": assigned, "selected": sel,
                    })
                })
                .collect();
            let buildings: Vec<Value> = rts
                .bldg_q
                .iter()
                .map(|(e, b, s, t, hp, _)| {
                    json!({
                        "id": e.to_bits().to_string(),
                        "side": side_ch(*s),
                        "kind": format!("{:?}", b.kind),
                        "x": r1(t.translation.x), "z": r1(t.translation.z),
                        "hp": hp.map(|h| h.hp.round()),
                        "built": b.built,
                    })
                })
                .collect();
            let deposits: Vec<Value> = rts
                .dep_q
                .iter()
                .map(|(e, d, t)| {
                    json!({
                        "id": e.to_bits().to_string(),
                        "kind": format!("{:?}", d.kind),
                        "x": r1(t.translation.x), "z": r1(t.translation.z),
                        "remaining": d.remaining.round(),
                    })
                })
                .collect();
            state["rts"] = json!({
                "outcome": format!("{:?}", *rts.outcome),
                "banks": {"player": bank_json(rts.banks.side(Side::Player)), "rival": bank_json(rts.banks.side(Side::Rival))},
                "pop": {
                    "player": {"count": rts.pop.0[Side::Player.ix()].count, "cap": rts.pop.0[Side::Player.ix()].cap},
                    "rival": {"count": rts.pop.0[Side::Rival.ix()].count, "cap": rts.pop.0[Side::Rival.ix()].cap},
                },
                "units": units,
                "buildings": buildings,
                "deposits": deposits,
            });
        }
        write_atomic(&dir.0.join("state.json"), &state.to_string());
    }

    // ── compact timeline log ──
    if now - scratch.last_log >= 2.0 {
        scratch.last_log = now;
        let line = if *clock.mode == GameMode::Skirmish {
            let army = |side: Side| {
                rts.units_q
                    .iter()
                    .filter(|(_, u, s, ..)| **s == side && matches!(u.kind, UnitKind::Swordsman | UnitKind::Archer))
                    .count()
            };
            let pb = rts.banks.side(Side::Player);
            let rb = rts.banks.side(Side::Rival);
            json!({
                "t": r1(clock.time.elapsed_secs()), "fps": scratch.fps_ema.round(),
                "p": {"wood": pb.wood.round(), "food": pb.food.round(), "gold": pb.gold.round(),
                       "pop": rts.pop.0[Side::Player.ix()].count, "army": army(Side::Player)},
                "r": {"wood": rb.wood.round(), "pop": rts.pop.0[Side::Rival.ix()].count, "army": army(Side::Rival)},
                "outcome": format!("{:?}", *rts.outcome),
            })
        } else {
            json!({
                "t": r1(clock.time.elapsed_secs()), "fps": scratch.fps_ema.round(),
                "phase": camp.siege.as_ref().map(|s| format!("{:?}", s.phase)),
                "wave": camp.siege.as_ref().map(|s| s.wave_index),
                "hero": camp.hero.as_ref().map(|h| vec![r1(h.pos.x), r1(h.pos.y)]),
            })
        };
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.0.join("log.jsonl")) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Execute one op object; returns the result value echoed into `state.results`.
fn run_op(op: &Value, virt: &mut Time<Virtual>, mode: &GameMode, rts: &mut RcRts, emit: &mut RcEmit) -> Value {
    // Field-level destructure so ops can borrow assets (&) and banks (&mut) side by side.
    let RcRts { banks, build_assets, units_q, bldg_q, dep_q, .. } = rts;
    let RcEmit { orders, trains, exit, commands } = emit;
    let name = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let fx = |v: &Value, k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32);
    let ok = |extra: Value| json!({"ok": true, "op": name, "info": extra});
    let err = |m: String| json!({"ok": false, "op": name, "err": m});

    match name {
        // {"op":"speed","mult":3.0} — virtual-clock multiplier (fast-forward a match).
        "speed" => {
            let m = fx(op, "mult").unwrap_or(1.0).clamp(0.05, 10.0);
            virt.set_relative_speed(m);
            ok(json!(m))
        }
        // {"op":"shot","path":"target/rc_shot.png"} — async screenshot (lands a few frames later).
        "shot" => {
            let Some(p) = op.get("path").and_then(|v| v.as_str()) else { return err("no path".into()) };
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(PathBuf::from(p.to_string())));
            ok(json!(p))
        }
        // {"op":"quit"}
        "quit" => {
            exit.write(AppExit::Success);
            ok(json!(null))
        }
        // {"op":"give","wood":100,"stone":0,"gold":0,"food":0,"side":"player"} — bank cheat.
        "give" => {
            let side = if op.get("side").and_then(|v| v.as_str()) == Some("rival") { Side::Rival } else { Side::Player };
            let b = banks.side_mut(side);
            b.wood += fx(op, "wood").unwrap_or(0.0) as f64;
            b.stone += fx(op, "stone").unwrap_or(0.0) as f64;
            b.gold += fx(op, "gold").unwrap_or(0.0) as f64;
            b.food += fx(op, "food").unwrap_or(0.0) as f64;
            ok(json!({"wood": b.wood, "stone": b.stone, "gold": b.gold, "food": b.food}))
        }
        // {"op":"build","kind":"Barracks","x":-20,"z":30} — validate+spend+scaffold at (or, with
        // "auto":true (default), NEAR) the given spot, exactly the ghost/AI path.
        "build" => {
            if *mode != GameMode::Skirmish {
                return err("build: not in skirmish".into());
            }
            let Some(assets) = build_assets.as_ref() else { return err("build assets not ready".into()) };
            let Some(kind) = op.get("kind").and_then(|v| v.as_str()).and_then(parse_building) else {
                return err("bad kind (TownHall|House|Sawmill|Quarry|GoldMine|Farm|Barracks)".into());
            };
            let (Some(x), Some(z)) = (fx(op, "x"), fx(op, "z")) else { return err("need x,z".into()) };
            let want = Vec2::new(x.round(), z.round());
            let deps: Vec<Vec2> = dep_q.iter().map(|(_, _, t)| Vec2::new(t.translation.x, t.translation.z)).collect();
            let auto = op.get("auto").and_then(|v| v.as_bool()).unwrap_or(true);
            let spot = if build::placement_valid(kind, Side::Player, want, &deps) {
                Some(want)
            } else if auto {
                build::find_spot(kind, Side::Player, want, &deps)
            } else {
                None
            };
            let Some(spot) = spot else { return err(format!("no valid spot near ({x},{z})")) };
            if build::try_place(commands, assets, banks, &deps, kind, Side::Player, spot, 0) {
                ok(json!({"kind": format!("{kind:?}"), "x": spot.x, "z": spot.y}))
            } else {
                err("try_place refused (funds?)".into())
            }
        }
        // {"op":"train","unit":"Swordsman","count":4} — enqueue across ALL built player barracks,
        // round-robin, so multiple barracks train in parallel.
        "train" => {
            let Some(kind) = op.get("unit").and_then(|v| v.as_str()).and_then(parse_unit) else {
                return err("bad unit (Swordsman|Archer)".into());
            };
            let n = op.get("count").and_then(|v| v.as_u64()).unwrap_or(1).min(12);
            let halls: Vec<Entity> = bldg_q
                .iter()
                .filter(|(_, b, s, _, _, has_q)| {
                    b.kind == crate::rts::BuildingKind::Barracks && b.built && **s == Side::Player && *has_q
                })
                .map(|(e, ..)| e)
                .collect();
            if halls.is_empty() {
                return err("no built player barracks".into());
            }
            for i in 0..n as usize {
                trains.write(TrainOrder { building: halls[i % halls.len()], kind });
            }
            ok(json!({"unit": format!("{kind:?}"), "count": n, "barracks": halls.len()}))
        }
        // {"op":"order","select":"soldiers"|"workers"|"all"|["<id>",...],
        //  "type":"move"|"attack_move" (+x,z) | "attack"|"harvest" (+"target":"<id>")}
        "order" => {
            let sel = op.get("select").cloned().unwrap_or(json!("soldiers"));
            let units: Vec<Entity> = match &sel {
                Value::Array(ids) => ids
                    .iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    .filter_map(Entity::try_from_bits)
                    .collect(),
                Value::String(s) => units_q
                    .iter()
                    .filter(|(_, u, side, ..)| {
                        **side == Side::Player
                            && match s.as_str() {
                                "workers" => u.kind == UnitKind::Worker,
                                "soldiers" => matches!(u.kind, UnitKind::Swordsman | UnitKind::Archer),
                                _ => true, // "all"
                            }
                    })
                    .map(|(e, ..)| e)
                    .collect(),
                _ => vec![],
            };
            if units.is_empty() {
                return err("selection empty".into());
            }
            let count = units.len();
            let ty = op.get("type").and_then(|v| v.as_str()).unwrap_or("move");
            let order = match ty {
                "move" | "attack_move" => {
                    let (Some(x), Some(z)) = (fx(op, "x"), fx(op, "z")) else { return err("need x,z".into()) };
                    if ty == "move" { Order::Move(Vec2::new(x, z)) } else { Order::AttackMove(Vec2::new(x, z)) }
                }
                "attack" | "harvest" => {
                    let target = op
                        .get("target")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse::<u64>().ok())
                        .and_then(Entity::try_from_bits);
                    let Some(t) = target else { return err("bad/missing target id".into()) };
                    if ty == "attack" { Order::Attack(t) } else { Order::Harvest(t) }
                }
                other => return err(format!("bad order type {other}")),
            };
            orders.write(RtsOrder { units, order });
            ok(json!({"units": count, "type": ty}))
        }
        other => err(format!("unknown op '{other}'")),
    }
}

fn parse_building(s: &str) -> Option<crate::rts::BuildingKind> {
    use crate::rts::BuildingKind::*;
    Some(match s {
        "TownHall" => TownHall,
        "House" => House,
        "Sawmill" => Sawmill,
        "Quarry" => Quarry,
        "GoldMine" => GoldMine,
        "Farm" => Farm,
        "Barracks" => Barracks,
        "Wall" => Wall,
        "Watchtower" => Watchtower,
        "Market" => Market,
        _ => return None,
    })
}

fn parse_unit(s: &str) -> Option<UnitKind> {
    Some(match s {
        "Swordsman" => UnitKind::Swordsman,
        "Archer" => UnitKind::Archer,
        _ => return None,
    })
}

fn side_ch(s: Side) -> &'static str {
    match s {
        Side::Player => "P",
        Side::Rival => "R",
    }
}

fn r1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

/// tmp + rename so a reader never sees a half-written snapshot.
fn write_atomic(path: &std::path::Path, contents: &str) {
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, contents).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}
