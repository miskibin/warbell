//! **Save / Continue** — one-slot autosave at every dawn, and resume a run after a defeat or
//! after quitting.
//!
//! The world is built once at `Startup` and is otherwise *persistent* within a process (in-run
//! state changes never rebuild the island; a *fresh-run* reset relaunches the process — see
//! `game_state::RestartProcess`), so a save is a **logic snapshot**, not an ECS dump:
//! we capture the run-state resources (hero / economy / town / upgrades / keep / heirs / night)
//! plus a few world flags (looted treasure chests, rescued camps, discovered landmarks), write
//! them as JSON, and on load overwrite those same resources + mark the already-spawned entities.
//!
//! - **Autosave** fires on the `Wave → Prep` edge (a cleared night) — see [`autosave_on_dawn`].
//! - **Continue** resumes the save **in-process** (no relaunch / new window): `game_state`'s
//!   `begin_continue` drops it into [`PendingLoad`] + flags the battlefield sweep, then
//!   [`apply_pending_load`] writes it back over the live run-state the moment the run plays and
//!   emits [`GameLoaded`] so `town.rs` reconciles its building meshes. The start / game-over
//!   screens (in `game_state.rs`) show a Continue button when [`SaveExists`].
//!
//! Serialization rides `tileworld_core`'s optional `serde` feature (Player/Bag/Town/ResourceState);
//! `UpgradeState.purchased` is `&'static str`, so the save stores the id strings and
//! `UpgradeState::restore` re-interns them on load.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use tileworld_core::inventory::Bag;
use tileworld_core::player::Player;
use tileworld_core::resource_store::ResourceState;
use tileworld_core::town_store::Town;
use tileworld_core::upgrade_store::{node_by_id, UpgradeEffect, UpgradeState};

use crate::economy::{Bank, Defenses, EconomyState, Upgrades};
use crate::game_state::{AppState, Modal};
use crate::inventory::Inventory;
use crate::landmarks::{Discoveries, Landmark};
use crate::player::PlayerRes;
use crate::siege::{Difficulty, GamePhase, KeepHp, Siege};
use crate::succession::Lives;
use crate::town::TownRes;
use crate::chest::{Chest, ChestId, ChestLid, CHEST_LID_OPEN};
use crate::villagers::RescuedCamps;

/// Bump on any breaking change to [`SaveData`] — an older/garbage file is then treated as "no
/// save" (logged, never fatal).
const SAVE_VERSION: u32 = 1;

/// The full snapshot of a run, taken at dawn. One JSON object = one save slot.
#[derive(Serialize, Deserialize, Clone)]
pub struct SaveData {
    pub version: u32,
    // ── run progress ──
    pub difficulty: Difficulty,
    /// 0-based index of the just-cleared night (`-1` never saved — we only save after a wave).
    pub wave_index: i32,
    pub keep_hp: f32,
    pub keep_max: f32,
    pub heirs: u32,
    // ── core stores (serde via the core `serde` feature) ──
    pub player: Player,
    pub bank: ResourceState,
    pub bag: Bag,
    pub town: Town,
    // ── economy / defense (unlocked weapons re-derived from `upgrades` on load) ──
    pub upgrades: Vec<String>,
    pub defenses: Defenses,
    pub tax_office: bool,
    pub shop_discount: f32,
    // ── world flags ──
    pub rescued_camps: Vec<bool>,
    pub discoveries_found: u32,
    pub discoveries_completed: bool,
    pub discovered_landmarks: Vec<String>,
    /// Indexed by `ChestId`; only one-shot treasure chests (caches respawn on their own).
    pub opened_chests: Vec<bool>,
}

/// Set when the player picks **Continue**; drained by [`apply_pending_load`] on the next play frame.
#[derive(Resource, Default)]
pub struct PendingLoad(pub Option<SaveData>);

/// Whether a valid save file exists — drives the Continue button's visibility. Set at startup and
/// flipped true after each successful autosave.
#[derive(Resource, Default)]
pub struct SaveExists(pub bool);

/// Emitted by [`apply_pending_load`] once the resource state is restored, carrying the snapshot so
/// the modules that own world entities reconcile them: `town.rs` rebuilds building meshes,
/// `verbs.rs` re-opens looted chests, `landmarks.rs` re-marks discovered landmarks.
#[derive(Message, Clone)]
pub struct GameLoaded(pub SaveData);

pub struct SaveGamePlugin;

impl Plugin for SaveGamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingLoad>()
            .init_resource::<SaveExists>()
            .add_message::<GameLoaded>()
            .add_systems(Startup, detect_existing_save)
            // Snapshot at dawn (a cleared night). Gated like the rest of the sim.
            .add_systems(Update, autosave_on_dawn.run_if(in_state(Modal::None)))
            // Apply a pending load the moment a run is playing (cheap no-op when nothing pending).
            .add_systems(Update, apply_pending_load.run_if(in_state(AppState::Playing)))
            // Reconcile world entities from the GameLoaded snapshot (ungated; fires once per load).
            .add_systems(Update, (restore_discovered_landmarks, restore_opened_chests));
    }
}

// ── File location + IO ──────────────────────────────────────────────────────────────

/// The save file path: an OS data dir when resolvable, else a CWD fallback. One fixed file.
fn save_path() -> PathBuf {
    let dir = if let Ok(appdata) = std::env::var("APPDATA") {
        Some(PathBuf::from(appdata).join("tileworld"))
    } else if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        Some(PathBuf::from(xdg).join("tileworld"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(PathBuf::from(home).join(".local/share/tileworld"))
    } else {
        None
    };
    match dir {
        Some(d) => d.join("save.json"),
        None => PathBuf::from("tileworld-save.json"),
    }
}

/// Read + parse + version-check the save. Returns `None` for missing / unreadable / unparseable /
/// stale-version files (a load just isn't offered — never a crash).
pub fn load_save() -> Option<SaveData> {
    let path = save_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let data: SaveData = match serde_json::from_str(&text) {
        Ok(d) => d,
        Err(e) => {
            warn!("ignoring unparseable save at {path:?}: {e}");
            return None;
        }
    };
    if data.version != SAVE_VERSION {
        warn!("ignoring save with version {} (expected {})", data.version, SAVE_VERSION);
        return None;
    }
    Some(data)
}

/// Delete the one save slot — used by every **fresh-run** entry point (New Game / Restart) so the
/// old run can't be resumed. A missing file is not an error (already gone is the goal).
pub fn delete_save() {
    let path = save_path();
    match std::fs::remove_file(&path) {
        Ok(()) => info!("deleted save"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!("failed to delete save: {e}"),
    }
}

/// Serialize + write the save (creating the parent dir). Errors are returned for the caller to log.
fn write_save(data: &SaveData) -> std::io::Result<()> {
    let path = save_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&path, json)
}

fn detect_existing_save(mut exists: ResMut<SaveExists>) {
    exists.0 = load_save().is_some();
}

// ── Autosave at dawn ────────────────────────────────────────────────────────────────

/// Write a snapshot on every `Wave → Prep` edge (a cleared night). `prev` tracks last frame's
/// phase to fire exactly once on the transition.
#[allow(clippy::too_many_arguments)]
fn autosave_on_dawn(
    mut prev: Local<Option<GamePhase>>,
    mut exists: ResMut<SaveExists>,
    siege: Res<Siege>,
    keep: Res<KeepHp>,
    player: Res<PlayerRes>,
    bank: Res<Bank>,
    inv: Res<Inventory>,
    town: Res<TownRes>,
    up: Res<Upgrades>,
    def: Res<Defenses>,
    eco: Res<EconomyState>,
    lives: Res<Lives>,
    camps: Res<RescuedCamps>,
    disc: Res<Discoveries>,
    chests: Query<(&Chest, &ChestId)>,
    landmarks: Query<&Landmark>,
) {
    let phase = siege.phase;
    let was = prev.replace(phase);
    let dawn = was == Some(GamePhase::Wave) && phase == GamePhase::Prep;
    if !dawn || siege.wave_index < 0 {
        return;
    }

    // Looted-treasure flags, indexed by ChestId (caches refill on their own → never persisted).
    let n_chests = chests.iter().map(|(_, id)| id.0 + 1).max().unwrap_or(0);
    let mut opened_chests = vec![false; n_chests];
    for (chest, id) in &chests {
        if !chest.cache && chest.opened {
            opened_chests[id.0] = true;
        }
    }

    let data = SaveData {
        version: SAVE_VERSION,
        difficulty: siege.difficulty,
        wave_index: siege.wave_index,
        keep_hp: keep.hp,
        keep_max: keep.max,
        heirs: lives.heirs,
        player: player.0,
        bank: bank.0,
        bag: inv.0.clone(),
        town: town.0.clone(),
        upgrades: up.0.purchased().iter().map(|s| s.to_string()).collect(),
        defenses: def.clone(),
        tax_office: eco.tax_office,
        shop_discount: eco.shop_discount,
        rescued_camps: camps.done.clone(),
        discoveries_found: disc.found,
        discoveries_completed: disc.completed,
        discovered_landmarks: landmarks
            .iter()
            .filter(|l| l.is_discovered())
            .map(|l| l.name.to_string())
            .collect(),
        opened_chests,
    };

    match write_save(&data) {
        Ok(()) => {
            exists.0 = true;
            info!("autosaved after night {}", siege.wave_index + 1);
        }
        Err(e) => warn!("autosave failed: {e}"),
    }
}

// ── Apply a loaded game ─────────────────────────────────────────────────────────────

/// Overwrite the (freshly-reset) run-state resources with a pending snapshot, then emit
/// [`GameLoaded`] carrying it so the world-entity owners (town meshes / chests / landmarks)
/// reconcile. Idempotent: takes [`PendingLoad`] so it runs exactly once per load, then no-ops.
/// Kept to the resource writes (≤16 system params); entity restore lives in the owning modules.
#[allow(clippy::too_many_arguments)]
fn apply_pending_load(
    mut pending: ResMut<PendingLoad>,
    mut siege: ResMut<Siege>,
    mut keep: ResMut<KeepHp>,
    mut player: ResMut<PlayerRes>,
    mut bank: ResMut<Bank>,
    mut inv: ResMut<Inventory>,
    mut town: ResMut<TownRes>,
    mut up: ResMut<Upgrades>,
    mut def: ResMut<Defenses>,
    mut eco: ResMut<EconomyState>,
    mut lives: ResMut<Lives>,
    mut camps: ResMut<RescuedCamps>,
    mut disc: ResMut<Discoveries>,
    mut loaded: MessageWriter<GameLoaded>,
) {
    let Some(data) = pending.0.take() else { return };

    // Run state — clean Prep at the saved night.
    siege.difficulty = data.difficulty;
    siege.wave_index = data.wave_index;
    siege.phase = GamePhase::Prep;
    keep.hp = data.keep_hp;
    keep.max = data.keep_max;
    lives.heirs = data.heirs;
    lives.defeat = false;

    // Core stores (HP/gold/xp/stats, bank, gear, town buildings).
    player.0 = data.player;
    bank.0 = data.bank;
    inv.0 = data.bag.clone();
    town.0 = data.town.clone();

    // Economy / defense. Upgrade ids re-interned; unlocked weapons re-derived from them.
    up.0 = UpgradeState::restore(&data.upgrades);
    *def = data.defenses.clone();
    eco.tax_office = data.tax_office;
    eco.shop_discount = data.shop_discount;
    eco.unlocked_weapons.clear();
    for &id in up.0.purchased() {
        if let Some(node) = node_by_id(id)
            && let UpgradeEffect::UnlockWeapon(w) = node.effect
            && !eco.unlocked_weapons.contains(&w)
        {
            eco.unlocked_weapons.push(w);
        }
    }

    // Camp flags: set to the exact saved length so `camp_rescue`'s length-mismatch reset can't
    // wipe them; `seen` mirrors `done` (a rescued camp was certainly seen populated).
    camps.done = data.rescued_camps.clone();
    camps.seen = data.rescued_camps.clone();
    disc.found = data.discoveries_found;
    disc.completed = data.discoveries_completed;

    let night = data.wave_index + 2;
    loaded.write(GameLoaded(data)); // chests / landmarks / town meshes reconcile from this
    info!("loaded save — resuming at night {night}");
}

/// Re-mark discovered landmarks on a load (entity flags; the tally is restored as a resource in
/// [`apply_pending_load`]). Their beacons are snuffed by `landmarks::snuff_found_beacons`.
pub(crate) fn restore_discovered_landmarks(
    mut ev: MessageReader<GameLoaded>,
    mut landmarks: Query<&mut Landmark>,
) {
    let Some(GameLoaded(data)) = ev.read().last() else { return };
    for mut lm in &mut landmarks {
        if data.discovered_landmarks.iter().any(|n| n == lm.name) {
            lm.set_discovered(true);
        }
    }
}

/// Re-open looted treasure chests on a load (entity flag + lid pose), keyed by `ChestId`.
pub(crate) fn restore_opened_chests(
    mut ev: MessageReader<GameLoaded>,
    mut chests: Query<(&mut Chest, &ChestId, &Children)>,
    mut lids: Query<&mut Transform, With<ChestLid>>,
) {
    let Some(GameLoaded(data)) = ev.read().last() else { return };
    for (mut chest, id, children) in &mut chests {
        if !chest.cache && data.opened_chests.get(id.0).copied().unwrap_or(false) {
            chest.opened = true;
            for &c in children {
                if let Ok(mut lt) = lids.get_mut(c) {
                    lt.rotation = Quat::from_rotation_x(CHEST_LID_OPEN);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A populated, representative snapshot — shared by the round-trip + apply tests.
    fn sample() -> SaveData {
        let mut bag = Bag::new();
        bag.add("potion", 2);
        let mut town = Town::new(8, 1);
        let mut tbank = ResourceState::new();
        tbank.add_wood(50.0);
        tbank.add_stone(50.0);
        town.build(0, tileworld_core::town_store::BuildKind::Farm, &mut tbank);

        let mut player = Player::default();
        player.gold = 777;
        player.level = 4;

        SaveData {
            version: SAVE_VERSION,
            difficulty: Difficulty::Hard,
            wave_index: 2,
            keep_hp: 640.0,
            keep_max: 1400.0,
            heirs: 5,
            player,
            bank: tbank,
            bag,
            town,
            upgrades: vec!["def_walls".into(), "hero_hp_1".into()],
            defenses: Defenses { walls: true, ..Defenses::default() },
            tax_office: false,
            shop_discount: 0.8,
            rescued_camps: vec![true, false, true],
            discoveries_found: 3,
            discoveries_completed: false,
            discovered_landmarks: vec!["The Hollow Oak".into()],
            opened_chests: vec![false, true, false],
        }
    }

    /// A populated `SaveData` survives a JSON round-trip (the file format is stable).
    #[test]
    fn savedata_json_round_trips() {
        let data = sample();
        let json = serde_json::to_string(&data).expect("serialize");
        let back: SaveData = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, data.version);
        assert_eq!(back.difficulty, data.difficulty);
        assert_eq!(back.wave_index, 2);
        assert_eq!(back.player.gold, 777);
        assert_eq!(back.heirs, 5);
        assert_eq!(back.upgrades, data.upgrades);
        assert!(back.defenses.walls);
        assert_eq!(back.rescued_camps, vec![true, false, true]);
        assert_eq!(back.discovered_landmarks, vec!["The Hollow Oak".to_string()]);
        assert_eq!(back.opened_chests, vec![false, true, false]);
        assert!(back.bag.has_item("potion"));
        assert!(back.town.plots[0].is_built());
    }

    /// The `apply_pending_load` *system* drains `PendingLoad` and overwrites the live run-state
    /// resources (the load path's wiring, not just the data shape). Headless — no rendering.
    #[test]
    fn apply_pending_load_overwrites_run_state() {
        let mut app = App::new();
        app.add_message::<GameLoaded>()
            .init_resource::<PendingLoad>()
            .init_resource::<Siege>()
            .init_resource::<KeepHp>()
            .init_resource::<PlayerRes>()
            .init_resource::<Bank>()
            .init_resource::<Inventory>()
            .init_resource::<TownRes>()
            .init_resource::<Upgrades>()
            .init_resource::<Defenses>()
            .init_resource::<EconomyState>()
            .init_resource::<Lives>()
            .init_resource::<RescuedCamps>()
            .init_resource::<Discoveries>()
            .add_systems(Update, apply_pending_load);

        app.insert_resource(PendingLoad(Some(sample())));
        app.update();

        let w = app.world();
        assert_eq!(w.resource::<PlayerRes>().0.gold, 777, "hero gold restored");
        assert_eq!(w.resource::<Lives>().heirs, 5, "heirs restored");
        assert_eq!(w.resource::<Siege>().wave_index, 2, "night restored");
        assert_eq!(w.resource::<Siege>().difficulty, Difficulty::Hard);
        assert_eq!(w.resource::<Siege>().phase, GamePhase::Prep, "resumes in prep");
        assert_eq!(w.resource::<KeepHp>().hp, 640.0);
        assert!(w.resource::<Upgrades>().0.is_purchased("def_walls"), "upgrades restored");
        assert!(w.resource::<Defenses>().walls, "defense flags restored");
        assert!(w.resource::<Bank>().0.stone() >= 50.0, "stone bank restored");
        assert!(w.resource::<Inventory>().0.has_item("potion"), "satchel restored");
        assert!(w.resource::<TownRes>().0.plots[0].is_built(), "town buildings restored");
        assert_eq!(w.resource::<RescuedCamps>().done, vec![true, false, true]);
        assert!(
            w.resource::<PendingLoad>().0.is_none(),
            "PendingLoad drained — apply runs exactly once"
        );
    }
}
