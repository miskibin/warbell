//! **Save / Continue** — six save slots (one autosave + five manual), a dawn + every-10-minutes
//! autosave, and resume a run after a defeat or after quitting.
//!
//! The world is built once at `Startup` and is otherwise *persistent* within a process (in-run
//! state changes never rebuild the island; a *fresh-run* reset rebuilds it **in-process** —
//! `game_state::drive_fresh_run` re-arms `biome::PendingBuild`), so a save is a **logic snapshot**,
//! not an ECS dump:
//! we capture the run-state resources (hero / economy / town / upgrades / keep / heirs / night)
//! plus a few world flags (looted treasure chests, rescued camps, discovered landmarks), write
//! them as JSON, and on load overwrite those same resources + mark the already-spawned entities.
//!
//! - **Slots** — index `0` is the **autosave**, `1..=`[`MANUAL_SLOTS`] are the player's manual
//!   slots (`autosave.json` / `save{n}.json`). [`SaveSlots`] caches one [`SlotMeta`] per slot so the
//!   menus can label + order them without re-reading the disk every frame.
//! - **Autosave** fires on the `Wave → Prep` edge (a cleared night) — see [`autosave_on_dawn`] —
//!   and every [`AUTOSAVE_INTERVAL`] seconds of *play* time ([`Playtime`], pause-aware) once the
//!   run is back in a saveable `Prep` day — see [`autosave_tick`]. Both write slot 0.
//! - **Continue** resumes a slot **in-process** (no relaunch / new window): `game_state`'s
//!   `load_slot` drops it into [`PendingLoad`] + flags the battlefield sweep, then
//!   [`apply_pending_load`] writes it back over the live run-state the moment the run plays and
//!   emits [`GameLoaded`] so `town.rs` reconciles its building meshes. The start / game-over /
//!   pause screens (in `game_state.rs`) show Continue / Load when [`SaveSlots::any`].
//!
//! Serialization rides `tileworld_core`'s optional `serde` feature (Player/Bag/Town/ResourceState);
//! `UpgradeState.purchased` is `&'static str`, so the save stores the id strings and
//! `UpgradeState::restore` re-interns them on load.
//!
//! Deliberately **not** saved (transient/derived, like the swept battlefield): timed `Buffs`,
//! pickup `Toasts`, and the **muster** — a rallied war party (`villagers::Rallied`) is battlefield
//! state, so a loaded run boots unrallied and `K` re-rallies. (See CLAUDE.md for the full list.)

use std::path::PathBuf;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use tileworld_core::inventory::Bag;
use tileworld_core::player::Player;
use tileworld_core::quest::QuestLog;
use tileworld_core::resource_store::ResourceState;
use tileworld_core::town_store::Town;
use tileworld_core::upgrade_store::{node_by_id, UpgradeEffect, UpgradeState};

use crate::economy::{Bank, Defenses, EconomyState, Upgrades};
use crate::game_state::AppState;
use crate::inventory::Inventory;
use crate::landmarks::{Discoveries, Landmark};
use crate::player::PlayerRes;
use crate::quest::QuestLogRes;
use crate::siege::{Difficulty, GamePhase, KeepHp, Siege};
use crate::succession::Lives;
use crate::town::TownRes;
use crate::chest::{Chest, ChestId, ChestLid, CHEST_LID_OPEN};
use crate::ui::notice::Notice;
use crate::villagers::RescuedCamps;
use crate::game_state::SimAppExt;

/// Bump on any breaking change to [`SaveData`] — an older/garbage file is then treated as "no
/// save" (logged, never fatal).
const SAVE_VERSION: u32 = 1;

/// How many **manual** save slots the player gets (slot indices `1..=MANUAL_SLOTS`). Slot `0` is
/// the autosave, so there are `MANUAL_SLOTS + 1` files on disk in total.
pub const MANUAL_SLOTS: usize = 5;

/// Total slot count including the autosave at index 0 — the length of [`SaveSlots`].
pub const SLOT_COUNT: usize = MANUAL_SLOTS + 1;

/// Seconds of **play** time ([`Playtime`], which freezes with the sim) between periodic autosaves.
pub const AUTOSAVE_INTERVAL: f64 = 600.0;

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
    /// Which world this run is on (`crate::worldmap::MapId` as `u8`; 0 = Home). Additive —
    /// old saves default to 0, so they resume on the home island as before. On load,
    /// `restore_active_map` rebuilds the world if this differs from the booted map.
    #[serde(default)]
    pub map_id: u8,
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
    /// Freed flags per Blight-fortress cage (analogous to `rescued_camps`). Additive — old saves
    /// default to all-unfreed via `serde(default)`, which just re-arms the cages (harmless).
    #[serde(default)]
    pub blight_captives_freed: Vec<bool>,
    pub discoveries_found: u32,
    pub discoveries_completed: bool,
    pub discovered_landmarks: Vec<String>,
    /// Names of landmarks whose sealed gear the hero has CLAIMED (won the Hold-the-Rune trial).
    /// Additive — old saves default to empty (no gear claimed), so the trials simply re-arm.
    #[serde(default)]
    pub claimed_landmark_gear: Vec<String>,
    /// Indexed by `ChestId`; only one-shot treasure chests (caches respawn on their own).
    pub opened_chests: Vec<bool>,
    /// Tutorial-quest progress. Additive + **optional**: a save written before the quest system
    /// existed has no field → `None` (distinguishable from a new save sitting at quest 0, which is
    /// `Some(active: 0)`). `restore_quest_log` treats `None` as "already past onboarding" so an old
    /// run doesn't restart the tutorial on every Continue.
    #[serde(default)]
    pub quest: Option<QuestLog>,
    // ── rival stronghold (Stronghold-Crusader-style AI opponent) ──
    /// The rival lord's banked gold. Additive — old saves default to 0.0 (a fresh treasury).
    #[serde(default)]
    pub rival_gold: f64,
    /// The rival's population (drives its tax income). Additive — defaults to 0; `restore_rival`
    /// floors it back to the founding base, so an old save resumes with a starter rival.
    #[serde(default)]
    pub rival_population: u32,
    /// How many buildings the rival has raised (== next plot index). Additive — defaults to 0
    /// (a bare bailey), so old saves resume with the fort but no grown town, which then grows.
    #[serde(default)]
    pub rival_built: usize,
    /// The player razed the rival fort — stays destroyed across save/load. Additive — old saves
    /// default to `false` (the fort still stands).
    #[serde(default)]
    pub rival_destroyed: bool,
    // ── slot metadata (menu labels / "latest slot" ordering) ──
    /// Unix seconds at the moment the snapshot was taken. Additive — old saves default to `0`,
    /// which still sorts as "oldest" in [`SaveSlots::latest`] without hiding the slot.
    #[serde(default)]
    pub saved_at: u64,
    /// Total **play** time of the run when it was saved (see [`Playtime`]). Additive — old saves
    /// default to `0.0` and simply resume with the clock restarting.
    #[serde(default)]
    pub playtime_secs: f64,
}

/// Set when the player picks **Continue**; drained by [`apply_pending_load`] on the next play frame.
#[derive(Resource, Default)]
pub struct PendingLoad(pub Option<SaveData>);

/// One slot's headline, read off its file — everything the menus need to label + order a slot
/// without deserializing the whole run again.
#[derive(Clone, Copy, Debug)]
pub struct SlotMeta {
    /// Unix seconds the snapshot was written (`0` for a pre-slots save — still a valid slot).
    pub saved_at: u64,
    /// The saved run's night index (`-1` = day one, before the first siege).
    pub wave_index: i32,
    /// Play time of the saved run, in seconds.
    pub playtime_secs: f64,
    /// The hero's level at the save.
    pub level: i64,
}

/// What's on disk, cached: `0` = autosave, `1..=`[`MANUAL_SLOTS`] = manual slots. Refreshed at
/// startup, whenever the start screen opens, and after every write/delete — see [`refresh_slots`].
/// Replaces the old single `SaveExists` flag.
#[derive(Resource, Default)]
pub struct SaveSlots(pub [Option<SlotMeta>; SLOT_COUNT]);

impl SaveSlots {
    /// Any slot at all occupied — drives the Continue / Load buttons' visibility.
    pub fn any(&self) -> bool {
        self.0.iter().any(|s| s.is_some())
    }

    /// The autosave slot's meta (slot 0), if it exists — what New Game would overwrite.
    pub fn autosave(&self) -> Option<&SlotMeta> {
        self.0[0].as_ref()
    }

    /// The most recently written slot — what **Continue** resumes. Ties (including a whole set of
    /// legacy `saved_at == 0` files) resolve to the lowest index, i.e. the autosave.
    pub fn latest(&self) -> Option<usize> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.as_ref().map(|m| (i, m.saved_at)))
            .max_by_key(|&(i, at)| (at, std::cmp::Reverse(i)))
            .map(|(i, _)| i)
    }
}

/// Pause-aware **play** clock for the current run, in seconds — the periodic autosave's ruler.
/// Mirrors `siege::GameTime`: its advancing system is sim-gated *and* skips while the day/night
/// cycle is paused, so idling in a menu never triggers an autosave. Round-trips the save
/// (`SaveData::playtime_secs`) and resets to 0 on a fresh run.
#[derive(Resource, Default)]
pub struct Playtime(pub f64);

/// When the next periodic autosave is due (in [`Playtime`] seconds) and whether one is *owed* —
/// a trigger that lands mid-siege sets `pending` and waits for the next `Prep` day.
#[derive(Resource)]
pub struct AutosaveTimer {
    pub next_at: f64,
    pub pending: bool,
}

impl Default for AutosaveTimer {
    fn default() -> Self {
        Self { next_at: AUTOSAVE_INTERVAL, pending: false }
    }
}

/// Emitted by [`apply_pending_load`] once the resource state is restored, carrying the snapshot so
/// the modules that own world entities reconcile them: `town.rs` rebuilds building meshes,
/// `verbs.rs` re-opens looted chests, `landmarks.rs` re-marks discovered landmarks.
#[derive(Message, Clone)]
pub struct GameLoaded(pub SaveData);

/// Request a snapshot into a **manual** slot (`1..=`[`MANUAL_SLOTS`]) — fired by the pause menu's
/// slot picker (see `game_state::slot_picker_click`). Handled by [`manual_save`], which writes only
/// while in `Prep` (a mid-siege save would resume in the wrong place). The autosaves don't use this.
#[derive(Message)]
pub struct RequestSave(pub usize);

pub struct SaveGamePlugin;

impl Plugin for SaveGamePlugin {
    fn build(&self, app: &mut App) {
        // Resources + messages stay registered in both modes (game_state reads `PendingLoad` and
        // `SaveSlots` non-optionally). No save/load in Skirmish — every system is Campaign-only.
        app.init_resource::<PendingLoad>()
            .init_resource::<SaveSlots>()
            .init_resource::<Playtime>()
            .init_resource::<AutosaveTimer>()
            .add_message::<GameLoaded>()
            .add_message::<RequestSave>()
            // Ungated: only scans which save files exist (no gameplay side effects), so it can run
            // in EVERY boot — a campaign-booted process can flip modes mid-process. Save/load
            // systems below stay `in_campaign`-gated. The Startup pass also migrates a legacy
            // single-slot `save.json` into the autosave slot.
            .add_systems(Startup, boot_slots)
            // Re-scan whenever the title comes up (a run may have written slots since).
            .add_systems(OnEnter(AppState::StartScreen), rescan_slots)
            // A fresh run restarts the play clock + the periodic-autosave countdown. Same hooks as
            // every other `reset_*` — a Continue then overwrites them in `apply_pending_load`.
            .add_systems(OnExit(AppState::StartScreen), reset_run_clocks)
            .add_systems(OnExit(AppState::GameOver), reset_run_clocks)
            // Pause-aware play clock + the snapshots it drives. Gated like the rest of the sim, and
            // chained so a dawn write always precedes (and re-arms) the periodic one.
            .add_sim_systems(
                (advance_playtime, autosave_on_dawn, autosave_tick)
                    .chain()
                    .run_if(crate::rts::in_campaign),
            )
            // Manual save (pause-menu button). Runs in `Paused` — where the world is frozen but
            // every run-state resource still lives — so it can snapshot the current day on demand.
            .add_systems(Update, manual_save.run_if(in_state(AppState::Paused)).run_if(crate::rts::in_campaign))
            // FOREST_SAVETEST=1 — headless-harness hook: write one real autosave (the full
            // `SaveCtx::snapshot()` path) ~2s into Play, so a capture run can verify the save
            // pipeline end-to-end (write → refresh → next boot's menu sees it) with no keypress.
            .add_systems(
                Update,
                savetest_write
                    .run_if(|| std::env::var("FOREST_SAVETEST").is_ok())
                    .run_if(in_state(AppState::Playing))
                    .run_if(crate::rts::in_campaign),
            )
            // Apply a pending load the moment a run is playing (cheap no-op when nothing pending).
            .add_systems(Update, apply_pending_load.run_if(in_state(AppState::Playing)).run_if(crate::rts::in_campaign))
            // Reconcile world entities from the GameLoaded snapshot (ungated; fires once per load).
            // `restore_active_map` rebuilds the terrain if the loaded run was on a different map.
            .add_systems(
                Update,
                (restore_discovered_landmarks, restore_opened_chests, restore_active_map)
                    .run_if(crate::rts::in_campaign),
            );
    }
}

// ── File location + IO ──────────────────────────────────────────────────────────────

/// The save **directory**: an OS data dir when resolvable, else `None` (→ a CWD-file fallback).
fn save_dir() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        Some(PathBuf::from(appdata).join("tileworld"))
    } else if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        Some(PathBuf::from(xdg).join("tileworld"))
    } else if let Ok(home) = std::env::var("HOME") {
        Some(PathBuf::from(home).join(".local/share/tileworld"))
    } else {
        None
    }
}

/// One slot's file name: slot 0 is the autosave, `1..=MANUAL_SLOTS` the manual slots.
fn slot_file(slot: usize) -> String {
    if slot == 0 { "autosave.json".to_string() } else { format!("save{slot}.json") }
}

/// A slot's full path (same dir resolution for every slot; CWD-prefixed when there's no data dir).
fn slot_path(slot: usize) -> PathBuf {
    match save_dir() {
        Some(d) => d.join(slot_file(slot)),
        None => PathBuf::from(format!("tileworld-{}", slot_file(slot))),
    }
}

/// Where the pre-multi-slot single save lived. Migrated into slot 0 once, at boot.
fn legacy_path() -> PathBuf {
    match save_dir() {
        Some(d) => d.join("save.json"),
        None => PathBuf::from("tileworld-save.json"),
    }
}

/// Move a pre-multi-slot `save.json` into the autosave slot, once, at boot. Best-effort: if the
/// autosave slot already exists, or the rename fails, the legacy file is simply left alone (it is
/// never read again, so the worst case is a stale file on disk — never a lost or corrupt run).
fn migrate_legacy_save() {
    let legacy = legacy_path();
    let auto = slot_path(0);
    if !legacy.exists() || auto.exists() {
        return;
    }
    if let Some(parent) = auto.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(&legacy, &auto) {
        Ok(()) => info!("migrated legacy save into the autosave slot"),
        Err(e) => warn!("could not migrate legacy save at {legacy:?}: {e}"),
    }
}

/// Read + parse + version-check one slot. Returns `None` for missing / unreadable / unparseable /
/// stale-version files (a load just isn't offered — never a crash).
pub fn load_save(slot: usize) -> Option<SaveData> {
    let path = slot_path(slot);
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

/// Delete one slot. Used by the fresh-run entry points on **slot 0 only** — a New Game wipes the
/// autosave (the run it would resume) but never the player's manual slots. A missing file is not an
/// error (already gone is the goal).
pub fn delete_save(slot: usize) {
    let path = slot_path(slot);
    match std::fs::remove_file(&path) {
        Ok(()) => info!("deleted save slot {slot}"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!("failed to delete save slot {slot}: {e}"),
    }
}

/// Serialize + write one slot (creating the parent dir). Errors are returned for the caller to log.
fn write_save(slot: usize, data: &SaveData) -> std::io::Result<()> {
    let path = slot_path(slot);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&path, json)
}

/// Re-scan every slot file and rebuild the cached [`SaveSlots`] metas. Cheap enough for the menus
/// (six small JSON reads), so it runs at boot, on every entry to the title, and after every write
/// or delete — never per frame.
pub fn refresh_slots(slots: &mut SaveSlots) {
    for slot in 0..SLOT_COUNT {
        slots.0[slot] = load_save(slot).map(|d| SlotMeta {
            saved_at: d.saved_at,
            wave_index: d.wave_index,
            playtime_secs: d.playtime_secs,
            level: d.player.level,
        });
    }
}

/// Startup: migrate a legacy single-slot save, then take the first slot scan.
fn boot_slots(mut slots: ResMut<SaveSlots>) {
    migrate_legacy_save();
    refresh_slots(&mut slots);
}

/// Re-scan the slots whenever the title screen opens (`OnEnter(StartScreen)`), so its Continue /
/// Load buttons reflect anything the run just wrote. Bevy runs the initial `OnEnter(StartScreen)`
/// *before* `Startup`, so this is also what makes the buttons right on frame 0 of a cold boot —
/// hence the legacy migration runs here too (it no-ops once done).
fn rescan_slots(mut slots: ResMut<SaveSlots>) {
    migrate_legacy_save();
    refresh_slots(&mut slots);
}

/// The two per-run clocks, bundled so [`apply_pending_load`] can restore both without blowing past
/// Bevy's 16-param system ceiling.
#[derive(SystemParam)]
struct RunClocks<'w> {
    playtime: ResMut<'w, Playtime>,
    timer: ResMut<'w, AutosaveTimer>,
}

/// Fresh-run reset (`OnExit(StartScreen)` / `OnExit(GameOver)`, like every other `reset_*`): the
/// play clock restarts at zero and the periodic autosave is re-armed a full interval out. A
/// **Continue** takes the same path, then [`apply_pending_load`] overwrites both from the snapshot.
fn reset_run_clocks(mut playtime: ResMut<Playtime>, mut timer: ResMut<AutosaveTimer>) {
    playtime.0 = 0.0;
    *timer = AutosaveTimer::default();
}

/// Unix seconds now — the save's timestamp (`0` if the clock is somehow before the epoch).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Snapshot source (shared by the dawn autosave + the manual save) ─────────────────────

/// All the live run-state a snapshot reads, bundled into one `SystemParam` so both save paths
/// (dawn edge + manual button) build an identical [`SaveData`] from one place — and neither
/// blows past Bevy's 16-param ceiling. Read-only.
#[derive(SystemParam)]
struct SaveCtx<'w, 's> {
    siege: Res<'w, Siege>,
    keep: Res<'w, KeepHp>,
    player: Res<'w, PlayerRes>,
    bank: Res<'w, Bank>,
    inv: Res<'w, Inventory>,
    town: Res<'w, TownRes>,
    up: Res<'w, Upgrades>,
    def: Res<'w, Defenses>,
    eco: Res<'w, EconomyState>,
    lives: Res<'w, Lives>,
    camps: Res<'w, RescuedCamps>,
    captives: Option<Res<'w, crate::ork_fortress::BlightCaptives>>,
    disc: Res<'w, Discoveries>,
    quest: Res<'w, QuestLogRes>,
    rival: Res<'w, crate::rival::RivalState>,
    active_map: Res<'w, crate::worldmap::ActiveMap>,
    playtime: Res<'w, Playtime>,
    chests: Query<'w, 's, (&'static Chest, &'static ChestId)>,
    landmarks: Query<'w, 's, &'static Landmark>,
}

impl SaveCtx<'_, '_> {
    /// Build the full run snapshot from the live resources/entities. The single place the
    /// `SaveData` field list is populated — keep it in sync with [`apply_pending_load`].
    fn snapshot(&self) -> SaveData {
        // Looted-treasure flags, indexed by ChestId (caches refill on their own → never persisted).
        let n_chests = self.chests.iter().map(|(_, id)| id.0 + 1).max().unwrap_or(0);
        let mut opened_chests = vec![false; n_chests];
        for (chest, id) in &self.chests {
            if !chest.cache && chest.opened {
                opened_chests[id.0] = true;
            }
        }
        SaveData {
            version: SAVE_VERSION,
            difficulty: self.siege.difficulty,
            wave_index: self.siege.wave_index,
            keep_hp: self.keep.hp,
            keep_max: self.keep.max,
            heirs: self.lives.heirs,
            map_id: self.active_map.0 as u8,
            player: self.player.0,
            bank: self.bank.0,
            bag: self.inv.0.clone(),
            town: self.town.0.clone(),
            upgrades: self.up.0.purchased().iter().map(|s| s.to_string()).collect(),
            defenses: self.def.clone(),
            tax_office: self.eco.tax_office,
            shop_discount: self.eco.shop_discount,
            rescued_camps: self.camps.done.clone(),
            blight_captives_freed: self.captives.as_ref().map(|c| c.freed.to_vec()).unwrap_or_default(),
            discoveries_found: self.disc.found,
            discoveries_completed: self.disc.completed,
            discovered_landmarks: self
                .landmarks
                .iter()
                .filter(|l| l.is_discovered())
                .map(|l| l.name.to_string())
                .collect(),
            claimed_landmark_gear: self
                .landmarks
                .iter()
                .filter(|l| l.is_gear_claimed())
                .map(|l| l.name.to_string())
                .collect(),
            opened_chests,
            quest: Some(self.quest.0.clone()),
            rival_gold: self.rival.gold,
            rival_population: self.rival.population,
            rival_built: self.rival.built,
            rival_destroyed: self.rival.destroyed,
            saved_at: unix_now(),
            playtime_secs: self.playtime.0,
        }
    }
}

/// Write `data` into `slot` and refresh the cached [`SaveSlots`] on success. Returns whether the
/// write landed (callers add their own user feedback / log line).
fn flush_save(slot: usize, data: &SaveData, slots: &mut SaveSlots) -> bool {
    match write_save(slot, data) {
        Ok(()) => {
            refresh_slots(slots);
            true
        }
        Err(e) => {
            warn!("save failed: {e}");
            false
        }
    }
}

// ── Play clock + autosave (dawn edge + every 10 minutes of play) ─────────────────────

/// Advance the pause-aware [`Playtime`] clock. Sim-gated (so panels/pause freeze it) and held while
/// the day/night cycle is paused, exactly like `siege::advance_game_clock` — idling in a menu must
/// never age the run toward its next autosave.
fn advance_playtime(
    time: Res<Time>,
    sky: Res<crate::scene::SkyClock>,
    mut playtime: ResMut<Playtime>,
) {
    if sky.paused {
        return;
    }
    playtime.0 += time.delta_secs() as f64;
}

/// Write a snapshot on every `Wave → Prep` edge (a cleared night) — the "just survived a night"
/// checkpoint, into the autosave slot. `prev` tracks last frame's phase to fire exactly once on the
/// transition. Also re-arms the periodic autosave, so dawn and the 10-minute timer can't
/// double-write back to back.
fn autosave_on_dawn(
    mut prev: Local<Option<GamePhase>>,
    mut slots: ResMut<SaveSlots>,
    mut timer: ResMut<AutosaveTimer>,
    ctx: SaveCtx,
) {
    let phase = ctx.siege.phase;
    let was = prev.replace(phase);
    let dawn = was == Some(GamePhase::Wave) && phase == GamePhase::Prep;
    if !dawn || ctx.siege.wave_index < 0 {
        return;
    }
    if flush_save(0, &ctx.snapshot(), &mut slots) {
        info!("autosaved after night {}", ctx.siege.wave_index + 1);
        timer.pending = false;
        timer.next_at = ctx.playtime.0 + AUTOSAVE_INTERVAL;
    }
}

/// The **every-10-minutes-of-play** autosave. Two halves, deliberately decoupled:
/// 1. the countdown fires purely off [`Playtime`] — it marks the save *owed* and re-arms, so a long
///    siege never stacks up several missed autosaves;
/// 2. an owed save is written the first frame the run is actually saveable — the same rules as the
///    manual save (a `Prep` day, no Hold assault in progress; day one is fine). A trigger during a
///    night therefore lands the moment that night is cleared.
fn autosave_tick(
    mut slots: ResMut<SaveSlots>,
    mut timer: ResMut<AutosaveTimer>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
    assault: Res<crate::ork_fortress::AssaultState>,
    ctx: SaveCtx,
) {
    if ctx.playtime.0 >= timer.next_at {
        timer.pending = true;
        timer.next_at = ctx.playtime.0 + AUTOSAVE_INTERVAL;
    }
    if !timer.pending || ctx.siege.phase != GamePhase::Prep || assault.breached {
        return;
    }
    timer.pending = false;
    if flush_save(0, &ctx.snapshot(), &mut slots) {
        notice.push("Autosaved.", time.elapsed_secs_f64());
        info!("periodic autosave at {:.0} min of play", ctx.playtime.0 / 60.0);
    }
}

// ── Manual save (pause-menu Save Game button) ────────────────────────────────────────

/// Honor a [`RequestSave`] (the pause menu's slot picker): snapshot the current run into the
/// requested manual slot. Only while in `Prep` — a snapshot taken mid-siege would resume in the
/// wrong place (the saved `wave_index` rolls back to a clean Prep, skipping the night you were
/// fighting), so saving is a day-only action. Unlike the dawn autosave there is **no
/// `wave_index < 0` guard**, so day-one progress (before the first night) can be saved and resumed.
/// `FOREST_SAVETEST=1` (see the plugin registration): one real autosave write ~120 frames into
/// Play, through the exact snapshot path the dawn/periodic autosaves use. Test-harness only.
fn savetest_write(mut frames: Local<u32>, mut done: Local<bool>, mut slots: ResMut<SaveSlots>, ctx: SaveCtx) {
    if *done {
        return;
    }
    *frames += 1;
    if *frames < 120 {
        return;
    }
    *done = true;
    let ok = flush_save(0, &ctx.snapshot(), &mut slots);
    info!("FOREST_SAVETEST: autosave written = {ok}");
}

fn manual_save(
    mut reqs: MessageReader<RequestSave>,
    mut slots: ResMut<SaveSlots>,
    mut notice: ResMut<Notice>,
    time: Res<Time>,
    assault: Res<crate::ork_fortress::AssaultState>,
    ctx: SaveCtx,
) {
    // Last request wins (only one can be clicked per frame anyway); clamped into the manual range
    // so a bad caller can never stomp the autosave slot.
    let Some(slot) = reqs.read().map(|r| r.0.clamp(1, MANUAL_SLOTS)).last() else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if ctx.siege.phase != GamePhase::Prep {
        notice.push("Can't save during a siege — hold the keep, then save by day.", now);
        return;
    }
    // The assault is transient (a Continue resets the Hold to pristine), so a mid-raid save would
    // reload to a full garrison anyway — refuse it rather than write a misleading snapshot.
    if assault.breached {
        notice.push("Can't save mid-assault — break the Hold or pull back first.", now);
        return;
    }
    if flush_save(slot, &ctx.snapshot(), &mut slots) {
        notice.push(format!("Game saved to slot {slot}."), now);
        info!("manual save to slot {slot} (resume at night {})", ctx.siege.wave_index + 2);
    } else {
        notice.push("Save failed — see the log.", now);
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
    captives: Option<ResMut<crate::ork_fortress::BlightCaptives>>,
    mut disc: ResMut<Discoveries>,
    mut clocks: RunClocks,
    mut loaded: MessageWriter<GameLoaded>,
) {
    let Some(data) = pending.0.take() else { return };

    // Play clock: resume the saved run's total, and put the next periodic autosave a full interval
    // out from there (so loading never fires one instantly, nor defers it past the interval).
    clocks.playtime.0 = data.playtime_secs;
    clocks.timer.pending = false;
    clocks.timer.next_at = data.playtime_secs + AUTOSAVE_INTERVAL;

    // Run state — clean Prep at the saved night.
    siege.difficulty = data.difficulty;
    siege.wave_index = data.wave_index;
    siege.phase = GamePhase::Prep;
    // Rearm the director's scratch timers/counters so the loaded Prep day starts a *fresh* countdown
    // from the current clock, not the abandoned run's stale (possibly already-expired) prep_ends_at —
    // which would otherwise fire BeginWave immediately and drop the resumed night with no prep time.
    siege.rearm_scratch();
    keep.hp = data.keep_hp;
    keep.max = data.keep_max;
    lives.heirs = data.heirs;
    lives.defeat = false;

    // Core stores (HP/gold/xp/stats, bank, gear, town buildings).
    player.0 = data.player;
    bank.0 = data.bank;
    inv.0 = data.bag.clone();
    town.0 = data.town.clone();
    // Older saves were authored with fewer plots; pad up so the current outer
    // ring stays buildable instead of indexing past the saved vec.
    let plot_target = crate::town::PLOT_COUNT.max(town.0.plots.len());
    town.0.plots.resize(plot_target, tileworld_core::town_store::Plot::empty());

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
    // Blight cages: restore the freed flags so respawned patrols can't re-free an already-freed
    // captive (which would dup `population`). The cage VISUALS (door open/shut, captives seated
    // or gone) reconcile from the GameLoaded message below — `camps::reconcile_cages_on_load`.
    // Element-wise copy guards a length mismatch from an older save.
    if let Some(mut captives) = captives {
        for (i, &f) in data.blight_captives_freed.iter().enumerate() {
            if i < captives.freed.len() {
                captives.freed[i] = f;
            }
        }
    }
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
        if data.claimed_landmark_gear.iter().any(|n| n == lm.name) {
            lm.set_gear_claimed(true);
        }
    }
}

/// On load, if the saved run was on a different map than the one currently built, swap the
/// [`crate::worldmap::ActiveMap`] resource and re-arm the world rebuild. The booted world is Home
/// by default, so resuming a run saved on another map would otherwise land on Home terrain. Same-map
/// loads (the common case) skip the rebuild and resume in place. Reads the carried `SaveData`.
pub(crate) fn restore_active_map(
    mut ev: MessageReader<GameLoaded>,
    mut active: ResMut<crate::worldmap::ActiveMap>,
    mut pending_build: ResMut<crate::biome::PendingBuild>,
    mut world_ready: ResMut<crate::biome::WorldReady>,
    mut veil: ResMut<crate::loading::Veil>,
    time: Res<Time>,
) {
    let Some(GameLoaded(data)) = ev.read().last() else { return };
    if data.map_id == crate::worldmap::current_map_u8() {
        return; // same world already built — resume in place
    }
    active.0 = crate::worldmap::MapId::from_u8(data.map_id);
    // Re-arm the in-process rebuild; the veil holds over the despawn-and-rebuild and lifts when the
    // loaded map's world lands. `biome::apply_build` reads `ActiveMap` + regenerates the terrain.
    pending_build.0 = true;
    world_ready.0 = false;
    veil.raise(time.elapsed_secs());
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
            map_id: 2, // Arena — a non-default map id exercises the round-trip
            player,
            bank: tbank,
            bag,
            town,
            upgrades: vec!["def_walls".into(), "hero_hp_1".into()],
            defenses: Defenses { walls: true, ..Defenses::default() },
            tax_office: false,
            shop_discount: 0.8,
            rescued_camps: vec![true, false, true],
            blight_captives_freed: vec![false, true],
            discoveries_found: 3,
            discoveries_completed: false,
            discovered_landmarks: vec!["The Old Mill".into()],
            claimed_landmark_gear: vec!["The Old Mill".into()],
            opened_chests: vec![false, true, false],
            quest: Some(QuestLog { active: 3, progress: 2.0 }),
            rival_gold: 142.5,
            rival_population: 8,
            rival_built: 5,
            rival_destroyed: false,
            saved_at: 1_750_000_000,
            playtime_secs: 1234.0,
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
        assert_eq!(back.map_id, 2, "map id round-trips");
        assert_eq!(back.upgrades, data.upgrades);
        assert!(back.defenses.walls);
        assert_eq!(back.rescued_camps, vec![true, false, true]);
        assert_eq!(back.discovered_landmarks, vec!["The Old Mill".to_string()]);
        assert_eq!(back.opened_chests, vec![false, true, false]);
        assert_eq!(back.quest, Some(QuestLog { active: 3, progress: 2.0 }));
        assert!(back.bag.has_item("potion"));
        assert!(back.town.plots[0].is_built());
    }

    /// A save written before the quest system has no `quest` field → it must parse as `None` (not a
    /// silent `active: 0`), so the load path can tell a pre-quest run from a new run sitting at the
    /// first quest. This is what stops old runs from restarting the tutorial on every Continue.
    #[test]
    fn old_save_without_quest_field_parses_as_none() {
        let mut v = serde_json::to_value(sample()).expect("to value");
        v.as_object_mut().unwrap().remove("quest"); // simulate a pre-quest-system save
        let back: SaveData = serde_json::from_value(v).expect("deserialize old save");
        assert_eq!(back.quest, None, "absent quest field is distinguishable from active:0");
    }

    /// A save written before the second map has no `map_id` → it must default to 0 (Home), so old
    /// runs resume on the home island exactly as before.
    #[test]
    fn old_save_without_map_id_defaults_to_home() {
        let mut v = serde_json::to_value(sample()).expect("to value");
        v.as_object_mut().unwrap().remove("map_id"); // simulate a pre-second-map save
        let back: SaveData = serde_json::from_value(v).expect("deserialize old save");
        assert_eq!(back.map_id, 0, "absent map_id resumes on Home");
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
            .init_resource::<Playtime>()
            .init_resource::<AutosaveTimer>()
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
        assert_eq!(w.resource::<Playtime>().0, 1234.0, "play clock resumed");
        assert_eq!(
            w.resource::<AutosaveTimer>().next_at,
            1234.0 + AUTOSAVE_INTERVAL,
            "next periodic autosave re-armed a full interval past the load"
        );
        assert!(
            w.resource::<PendingLoad>().0.is_none(),
            "PendingLoad drained — apply runs exactly once"
        );
    }

    /// Slot 0 is the autosave and `1..=MANUAL_SLOTS` the manual slots — each with its own file.
    #[test]
    fn slot_files_are_distinct_and_autosave_is_zero() {
        assert_eq!(slot_file(0), "autosave.json");
        assert_eq!(slot_file(1), "save1.json");
        assert_eq!(slot_file(MANUAL_SLOTS), format!("save{MANUAL_SLOTS}.json"));
        let mut names: Vec<String> = (0..SLOT_COUNT).map(slot_file).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), SLOT_COUNT, "every slot maps to its own file");
    }

    fn meta(saved_at: u64) -> SlotMeta {
        SlotMeta { saved_at, wave_index: 0, playtime_secs: 0.0, level: 1 }
    }

    /// `latest()` picks the newest slot by timestamp; ties (incl. a set of legacy `saved_at == 0`
    /// files) fall back to the lowest index, i.e. the autosave. Empty ⇒ `None` and `any()` false.
    #[test]
    fn latest_slot_picks_newest_then_lowest_index() {
        let mut slots = SaveSlots::default();
        assert!(!slots.any());
        assert_eq!(slots.latest(), None);

        slots.0[0] = Some(meta(100));
        slots.0[3] = Some(meta(500));
        assert!(slots.any());
        assert_eq!(slots.latest(), Some(3), "newest wins");

        slots.0[0] = Some(meta(500));
        assert_eq!(slots.latest(), Some(0), "a tie prefers the autosave slot");

        let mut legacy = SaveSlots::default();
        legacy.0[2] = Some(meta(0));
        assert_eq!(legacy.latest(), Some(2), "a legacy save with no timestamp still resumes");
    }

    /// A save written before multi-slot has no `saved_at` / `playtime_secs` → both default, and the
    /// slot stays loadable (it just sorts oldest).
    #[test]
    fn old_save_without_slot_metadata_defaults() {
        let mut v = serde_json::to_value(sample()).expect("to value");
        let obj = v.as_object_mut().unwrap();
        obj.remove("saved_at");
        obj.remove("playtime_secs");
        let back: SaveData = serde_json::from_value(v).expect("deserialize old save");
        assert_eq!(back.saved_at, 0);
        assert_eq!(back.playtime_secs, 0.0);
    }
}
