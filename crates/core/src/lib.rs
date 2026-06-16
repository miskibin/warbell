//! Pure deterministic game logic, ported 1:1 from the TS `src/world/*` modules.
//!
//! Port provenance (source of truth = the original game at `D:\tileworld`):
//!   factions.rs       <- src/world/factions.ts
//!   frontier.rs       <- src/world/frontier.ts
//!   ork_config.rs     <- src/world/orkConfig.ts
//!   animal.rs         <- src/world/animalConfig.ts + the pure parts of animalAI.ts
//!   ambient.rs        <- src/world/Cat.tsx + src/world/Birds.tsx (decorative critters)
//!   block.rs          <- src/world/blockStore.ts (shield stamina; notify dropped)
//!   combat_juice.rs   <- src/world/fxStore.ts (screen-shake trauma + FOV-punch decay)
//!   dummy.rs          <- src/world/dummyStore.ts + MusterYard.tsx (practice dummies + pell)
//!   dust.rs           <- src/world/dustStore.ts + Dust.tsx (pooled ground-dust motes)
//!   wave.rs           <- src/world/waveLogic.ts + WAVES/PREP_DURATION from waveStore.ts
//!   bridges.rs        <- src/world/bridges.ts
//!   house_blockers.rs <- src/world/houseBlockers.ts
//!   inventory.rs      <- src/world/inventoryStore.ts (effects returned, not run)
//!   tilemap.rs        <- src/world/tileMap.ts (full procedural generator)
//!   city_plan.rs      <- src/world/cityPlan.ts (logic subset; see module note)
//!   roads.rs          <- src/world/roads.ts
//!   landmarks.rs      <- src/world/landmarks.ts (data only; see module note)
//!   obstacles.rs      <- src/world/obstacles.ts
//!   pathfinding.rs    <- src/world/pathfinding.ts (grid abstracted behind a trait)
//!   map_grid.rs       <- pathfinding::Grid wired over the real ported map (P1.4)
//!   resource_store.rs <- src/world/resourceStore.ts (state+mutators; notify dropped)
//!   ore_store.rs      <- src/world/oreStore.ts
//!   item_toast_store.rs <- src/world/itemToastStore.ts (notify dropped)
//!   buff_store.rs     <- src/world/buffStore.ts (explicit `now` arg; notify dropped)
//!   forage_store.rs   <- src/world/forageStore.ts (factory; herb/apple instances)
//!   player.rs         <- src/world/playerStore.ts (Player struct; notify/SFX dropped)
//!   projectile.rs     <- src/world/projectileStore.ts (homing bolt; damage hook -> ECS)
//!   orb.rs            <- src/world/orbStore.ts (reward orbs; grant hook -> ECS)
//!   villager.rs       <- src/world/villagerStore.ts + traderStore.ts + the
//!                        wander/state-machine math from Villager.tsx/Trader.tsx
//!   upgrade_store.rs  <- src/world/upgradeStore.ts (catalog+gating; effects typed,
//!                        not run — the ECS layer enacts them)
//!   shop_catalog.rs   <- src/world/shopCatalog.ts + shopStore.ts price/discount
//!
//! Each module's tests are the Rust translation of the matching `*.test.ts`, so
//! `cargo test -p tileworld_core` is the cross-language validation gate. The
//! reachability integration test lives in `tests/map_reachability.rs`.
//!
//! The `*_store` modules' subscribe/notify are HUD/browser-only concerns dropped
//! here; they become Bevy change-detection / persistence later. Each store is a
//! plain struct (no global) so its tests run
//! a fresh instance and stay parallel-safe.

pub mod ambient;
pub mod animal;
pub mod block;
pub mod bridges;
pub mod buff_store;
pub mod chests;
pub mod city_plan;
pub mod combat_juice;
pub mod defense;
pub mod dummy;
pub mod dust;
pub mod factions;
pub mod forage_store;
pub mod frontier;
pub mod house_blockers;
pub mod inventory;
pub mod item_toast_store;
pub mod landmarks;
pub mod map_grid;
pub mod obstacles;
pub mod orb;
pub mod ore_store;
pub mod ork_config;
pub mod pathfinding;
pub mod player;
pub mod projectile;
pub mod quest;
pub mod resource_store;
pub mod rng;
pub mod roads;
pub mod shop_catalog;
pub mod tilemap;
pub mod town_store;
pub mod upgrade_store;
pub mod villager;
pub mod wave;
