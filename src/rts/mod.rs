//! RTS Skirmish mode ("Potyczka") — a Stronghold-Crusader-style POC that reuses the campaign
//! substrate (terrain, navgrid, combat channels, worker haul loops) on a small symmetric arena.
//! Spec: docs/superpowers/specs/2026-07-12-rts-skirmish-mode-design.md
//!
//! This module owns the shared RTS vocabulary (mode, sides, banks, unit/building kinds,
//! command messages) so the submodules stay decoupled: each submodule is its own plugin and
//! only reaches across through these types. Every RTS system must be gated with
//! `.run_if(in_skirmish)` (plus the usual `Modal::None` sim gate where it simulates).

use bevy::prelude::*;

pub mod ai;
pub mod audio;
pub mod build;
pub mod camera;
pub mod command;
pub mod deposits;
pub mod ecotest;
pub mod minimap;
pub mod hud;
pub mod pick;
pub mod select;
pub mod units;
pub mod workers;

// ---------------------------------------------------------------- mode

/// Coarse process-wide mode, decided once at boot from `FOREST_RTS=1` and never changed
/// mid-process (entering Potyczka relaunches the exe, exactly like New Game).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameMode {
    Campaign,
    Skirmish,
}

pub fn mode_from_env() -> GameMode {
    if std::env::var("FOREST_RTS").is_ok() { GameMode::Skirmish } else { GameMode::Campaign }
}

/// Run condition: this system only runs in the RTS skirmish mode.
pub fn in_skirmish(mode: Res<GameMode>) -> bool {
    *mode == GameMode::Skirmish
}

/// Run condition: this system only runs in the classic campaign (hero) mode.
pub fn in_campaign(mode: Res<GameMode>) -> bool {
    *mode == GameMode::Campaign
}

// ---------------------------------------------------------------- sides

/// Which army an RTS unit/building/claim belongs to. Mirrored rules: both sides pay the
/// same costs and obey the same caps; hostility is simply "the other side".
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Side {
    Player,
    Rival,
}

impl Side {
    pub fn ix(self) -> usize {
        match self {
            Side::Player => 0,
            Side::Rival => 1,
        }
    }
    pub fn foe(self) -> Side {
        match self {
            Side::Player => Side::Rival,
            Side::Rival => Side::Player,
        }
    }
}

/// Arena base centres (world XZ; castle-at-origin frame). The arena generator flattens a
/// plateau at each and the deposits mirror through the origin.
pub const PLAYER_BASE: Vec2 = Vec2::new(-22.0, 22.0);
pub const RIVAL_BASE: Vec2 = Vec2::new(22.0, -22.0);
/// Usable land radius of the arena ellipse (tiles = world units); ocean beyond. Kept ≥ the base
/// centre distance (~31) so the camera can still frame both bases after the map shrank.
pub const ARENA_RADIUS: f32 = 36.0;

pub fn base_of(side: Side) -> Vec2 {
    match side {
        Side::Player => PLAYER_BASE,
        Side::Rival => RIVAL_BASE,
    }
}

// ---------------------------------------------------------------- resources / banks

/// One side's stock of the four skirmish resources. All-or-nothing spend, mirroring core
/// `ResourceState` semantics (`f64` like the core stores; gold included because the core
/// store lacks it).
#[derive(Clone, Copy, Default, Debug)]
pub struct RtsBank {
    pub wood: f64,
    pub stone: f64,
    pub gold: f64,
    pub food: f64,
}

/// A cost in the four resources (also used for deposit yields).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Cost {
    pub wood: f64,
    pub stone: f64,
    pub gold: f64,
    pub food: f64,
}

impl Cost {
    pub const fn wood(w: f64) -> Self {
        Cost { wood: w, stone: 0.0, gold: 0.0, food: 0.0 }
    }
}

impl RtsBank {
    pub fn can_afford(&self, c: &Cost) -> bool {
        self.wood >= c.wood && self.stone >= c.stone && self.gold >= c.gold && self.food >= c.food
    }
    /// All-or-nothing: returns false (and changes nothing) if any component is short.
    pub fn spend(&mut self, c: &Cost) -> bool {
        if !self.can_afford(c) {
            return false;
        }
        self.wood -= c.wood;
        self.stone -= c.stone;
        self.gold -= c.gold;
        self.food -= c.food;
        true
    }
    pub fn add(&mut self, c: &Cost) {
        self.wood += c.wood;
        self.stone += c.stone;
        self.gold += c.gold;
        self.food += c.food;
    }
}

/// Both sides' banks, indexed by `Side::ix()`. The AI spends from its own bank only — no
/// cheating.
#[derive(Resource, Default)]
pub struct RtsBanks(pub [RtsBank; 2]);

impl RtsBanks {
    pub fn side(&self, s: Side) -> &RtsBank {
        &self.0[s.ix()]
    }
    pub fn side_mut(&mut self, s: Side) -> &mut RtsBank {
        &mut self.0[s.ix()]
    }
}

/// Starting stock per side. Bumped well above spec §2's (50/30/20/30) so the player can lay down a
/// handful of buildings immediately instead of waiting on the economy — enough for ~two houses, a
/// sawmill, a farm, a barracks and a mine off the bat.
pub fn starting_bank() -> RtsBank {
    RtsBank { wood: 250.0, stone: 150.0, gold: 120.0, food: 120.0 }
}

// ---------------------------------------------------------------- population

/// Per-side population bookkeeping: `count` = living units (workers + soldiers), `cap` =
/// hall + houses. Enforces the ~30/side ceiling via housing.
#[derive(Resource)]
pub struct RtsPop(pub [PopSide; 2]);

#[derive(Clone, Copy, Debug)]
pub struct PopSide {
    pub count: u32,
    pub cap: u32,
}

pub const HALL_POP: u32 = 6;
pub const HOUSE_POP: u32 = 4;
pub const POP_HARD_CAP: u32 = 30;

impl Default for RtsPop {
    fn default() -> Self {
        RtsPop([PopSide { count: 0, cap: HALL_POP }; 2])
    }
}

// ---------------------------------------------------------------- buildings

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BuildingKind {
    TownHall,
    House,
    Sawmill,
    Quarry,
    GoldMine,
    Farm,
    Barracks,
}

/// Static per-building data (spec §6). `footprint` is in whole tiles, square.
pub struct BuildingDef {
    pub kind: BuildingKind,
    pub name: &'static str,
    pub footprint: u32,
    pub cost: Cost,
    pub build_secs: f32,
    pub hp: f32,
}

pub const BUILDINGS: [BuildingDef; 7] = [
    BuildingDef {
        kind: BuildingKind::TownHall,
        name: "Town Hall",
        footprint: 4,
        cost: Cost { wood: 0.0, stone: 0.0, gold: 0.0, food: 0.0 },
        build_secs: 0.0,
        hp: 1200.0,
    },
    BuildingDef {
        kind: BuildingKind::House,
        name: "House",
        footprint: 2,
        cost: Cost::wood(20.0),
        build_secs: 8.0,
        hp: 260.0,
    },
    BuildingDef {
        kind: BuildingKind::Sawmill,
        name: "Sawmill",
        footprint: 3,
        cost: Cost::wood(25.0),
        build_secs: 10.0,
        hp: 320.0,
    },
    BuildingDef {
        kind: BuildingKind::Quarry,
        name: "Quarry",
        footprint: 3,
        cost: Cost::wood(30.0),
        build_secs: 12.0,
        hp: 320.0,
    },
    BuildingDef {
        kind: BuildingKind::GoldMine,
        name: "Gold Mine",
        footprint: 3,
        cost: Cost { wood: 30.0, stone: 10.0, gold: 0.0, food: 0.0 },
        build_secs: 14.0,
        hp: 320.0,
    },
    BuildingDef {
        kind: BuildingKind::Farm,
        name: "Farm",
        footprint: 3,
        cost: Cost::wood(15.0),
        build_secs: 8.0,
        hp: 280.0,
    },
    BuildingDef {
        kind: BuildingKind::Barracks,
        name: "Barracks",
        footprint: 4,
        cost: Cost { wood: 40.0, stone: 20.0, gold: 0.0, food: 0.0 },
        build_secs: 20.0,
        hp: 520.0,
    },
];

pub fn building_def(kind: BuildingKind) -> &'static BuildingDef {
    BUILDINGS.iter().find(|d| d.kind == kind).expect("every BuildingKind has a def")
}

/// A placed RTS structure (scaffold or complete). Carries `Side` + `Health` alongside.
#[derive(Component, Clone, Copy)]
pub struct RtsBuilding {
    pub kind: BuildingKind,
    /// Building rises unmanned, Stronghold-style; `built` flips when the timer completes.
    pub built: bool,
}

/// Which deposit type a producer building's workers harvest.
pub fn harvest_kind(kind: BuildingKind) -> Option<DepositKind> {
    match kind {
        BuildingKind::Sawmill => Some(DepositKind::Wood),
        BuildingKind::Quarry => Some(DepositKind::Stone),
        BuildingKind::GoldMine => Some(DepositKind::Gold),
        _ => None, // Farm food comes from the field cycle, not a deposit
    }
}

// ---------------------------------------------------------------- deposits

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum DepositKind {
    Wood,
    Stone,
    Gold,
}

/// A finite resource site. Wood groves also own real tree entities; stone/gold are
/// ore-style rocks. `remaining` counts units of resource left; at zero the site is spent
/// (trees felled / rock shattered) and never regrows in Skirmish.
#[derive(Component)]
pub struct Deposit {
    pub kind: DepositKind,
    pub remaining: f64,
}

// ---------------------------------------------------------------- units

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum UnitKind {
    Worker,
    Swordsman,
    Archer,
}

/// Any commandable RTS body (both sides). Combat stats per spec §7.
#[derive(Component, Clone, Copy)]
pub struct RtsUnit {
    pub kind: UnitKind,
}

pub fn unit_hp(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Worker => 40.0,
        UnitKind::Swordsman => 90.0,
        UnitKind::Archer => 60.0,
    }
}

pub fn unit_damage(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Worker => 0.0,
        UnitKind::Swordsman => 12.0,
        UnitKind::Archer => 9.0,
    }
}

/// Training cost (both soldier kinds; converts one idle worker, spec §7).
pub fn train_cost(kind: UnitKind) -> Cost {
    match kind {
        UnitKind::Worker => Cost::default(),
        UnitKind::Swordsman | UnitKind::Archer => {
            Cost { wood: 10.0, stone: 0.0, gold: 15.0, food: 0.0 }
        }
    }
}

pub const TRAIN_SECS: f32 = 8.0;

// ---------------------------------------------------------------- build + training interfaces

/// What the player is currently placing (HUD build strip sets it; `build.rs` drives the ghost
/// and clears it on place/cancel). `None` = normal command input.
#[derive(Resource, Default)]
pub struct Placing(pub Option<BuildingKind>);

/// HUD (or the AI) asks a barracks to enqueue a unit. `units.rs` validates (cost + free
/// worker + pop) and drives the queue.
#[derive(Message)]
pub struct TrainOrder {
    pub building: Entity,
    pub kind: UnitKind,
}

/// Live training state on a barracks: FIFO queue (depth ≤ 3) + progress through the current
/// trainee's `TRAIN_SECS`. HUD reads it for buttons/progress; `units.rs` drives it.
#[derive(Component, Default)]
pub struct TrainQueue {
    pub queue: Vec<UnitKind>,
    pub progress: f32,
}

pub const TRAIN_QUEUE_DEPTH: usize = 3;

// ---------------------------------------------------------------- selection + commands

/// Marker on currently selected player entities (units or one building).
#[derive(Component)]
pub struct Selected;

/// An order issued to a set of units (player input or AI). `command.rs` consumes these and
/// writes goals into the NavPath machinery.
#[derive(Message)]
pub struct RtsOrder {
    pub units: Vec<Entity>,
    pub order: Order,
}

#[derive(Clone, Copy, Debug)]
pub enum Order {
    Move(Vec2),
    Attack(Entity),
    AttackMove(Vec2),
    Harvest(Entity),
}

// ---------------------------------------------------------------- outcome

/// Set when a Town Hall dies; `game_state::watch_end` (parameterized) flips to GameOver.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RtsOutcome {
    #[default]
    Undecided,
    PlayerWon,
    RivalWon,
}

// ---------------------------------------------------------------- plugin assembly

/// Added unconditionally in `main.rs`; every system inside the submodules is gated on
/// `in_skirmish`, so in Campaign this is inert beyond holding the `GameMode` resource.
pub struct RtsPlugin;

impl Plugin for RtsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(mode_from_env())
            .init_resource::<RtsBanks>()
            .init_resource::<RtsPop>()
            .init_resource::<RtsOutcome>()
            .add_message::<RtsOrder>()
            .add_message::<TrainOrder>()
            .init_resource::<Placing>()
            .add_plugins((
                camera::RtsCameraPlugin,
                pick::RtsPickPlugin,
                select::RtsSelectPlugin,
                command::RtsCommandPlugin,
                build::RtsBuildPlugin,
                deposits::RtsDepositsPlugin,
                workers::RtsWorkersPlugin,
                units::RtsUnitsPlugin,
                ai::RtsAiPlugin,
                hud::RtsHudPlugin,
                ecotest::RtsEcoTestPlugin,
                audio::RtsAudioPlugin,
                minimap::RtsMinimapPlugin,
            ));
    }
}
