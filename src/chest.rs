//! **Chests** — scattered loot containers (their own subsystem, extracted from `verbs.rs`).
//!
//! Two distance-gated tiers: plain **Wood** chests near the keep, and richer **Relic** chests
//! once you're a short walk out — the Relic chest is visibly fancier (gold banding + a jewel set
//! in the lid) and rolls from the top loot pool, so "looks better" reads as "holds better". Opens
//! are instant and juicy (lid overshoot + a chime + a small screen-shake + a hero bark). No
//! minigame, no traps.
//!
//! Placement is forest-coord native (`populate_chests`, called from `worldmap::build`); the tier is
//! derived deterministically from world position, so no new save fields are needed.

use bevy::prelude::*;
use tileworld_core::frontier;
use tileworld_core::inventory::{item_def, Bag, ItemKind};

use crate::audio::{AudioCue, Concept, Speak};
use crate::combat_fx::{col_kill, FloatQueue, FloatReq, HitFeedback};
use crate::game_state::Modal;
use crate::inventory::{try_grant, Inventory, Toasts};
use crate::palette::lin;
use crate::player::{HeroState, PlayerRes};
use crate::verbs::forest_frontier;
use crate::worldmap;

pub struct ChestPlugin;

impl Plugin for ChestPlugin {
    fn build(&self, app: &mut App) {
        // Sim systems freeze behind panels/pauses; the lid swing is pure animation (a lid keeps
        // falling open behind a panel), so it stays ungated like the other impact-juice drivers.
        app.add_systems(Update, (chest_interact, chest_respawn).run_if(in_state(Modal::None)))
            .add_systems(Update, drive_lid_swing);
    }
}

// ─── Tiers ─────────────────────────────────────────────────────────────────────────

/// A chest's loot rank, set by distance from the keep (`forest_frontier`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChestTier {
    Wood,
    Relic,
}

/// Wood near the keep (frontier `< 0.30`), Relic once you're a short walk out (`>= 0.30`, world
/// distance ~41+). Deliberately reachable — a biome trip crosses it — so the fancy Relic chests
/// are actually encountered, not hidden out at the deep rim.
pub(crate) fn tier_for(frontier: f64) -> ChestTier {
    if frontier >= 0.30 {
        ChestTier::Relic
    } else {
        ChestTier::Wood
    }
}

// ─── Components ─────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub(crate) struct Chest {
    /// true = repeatable supply cache (gold + food), false = one-shot treasure (gear).
    pub(crate) cache: bool,
    pub(crate) opened: bool,
    pub(crate) opened_at: f32,
    /// Frontier factor at placement → loot tier.
    factor: f64,
    /// Hand-authored one-shot loot `(gold, item ids)` — set for the Blight war-hoard so it pays a
    /// fixed plunder haul instead of frontier-rolled gear. `None` for scattered chests.
    trophy: Option<(i64, &'static [&'static str])>,
    /// A deep-rim **war hoard**: a one-shot treasure paying a guaranteed top-tier haul + a heavy
    /// purse, placed only out in the dangerous deep biomes.
    hoard: bool,
    /// Loot rank (Wood/Relic), set from `factor` at spawn.
    pub(crate) tier: ChestTier,
}

/// Number of scattered chests `populate_chests` places, keyed `ChestId(0..CHEST_COUNT)`.
pub(crate) const CHEST_COUNT: usize = 24;

/// ChestId for the one war-hoard at Gnashfang Hold — exactly `CHEST_COUNT`, i.e. one past the
/// scatter `0..CHEST_COUNT` range, so the looted-treasure save flag keys it without colliding with
/// a scattered chest. The five deep-rim hoards take `TROPHY_CHEST_ID + 1 + i` (so `CHEST_COUNT+1 ..`).
/// Deriving this from `CHEST_COUNT` keeps the ranges disjoint if either count changes.
pub(crate) const TROPHY_CHEST_ID: usize = CHEST_COUNT;

/// Stable index of a chest over the spawn order (`0..CHEST_COUNT`). The save keys a looted-treasure
/// flag by this so a re-launched world re-opens the right chests.
#[derive(Component)]
pub(crate) struct ChestId(pub usize);

/// The hinged lid (a child of the chest) — rotated open on loot, closed on a cache respawn.
#[derive(Component)]
pub(crate) struct ChestLid;

/// Lid hinge angle when open (front swings up + back). Shared by `chest_interact` and the
/// save-restore pass so a loaded looted chest shows its lid open.
pub(crate) const CHEST_LID_OPEN: f32 = -1.7;

const CHEST_INTERACT_DIST: f32 = 2.2;

/// A chest lid swinging on its hinge: opening flings it past the catch with an overshoot
/// (treasure!), re-closing (cache respawn) is a plain ease. The save-restore pass still sets a
/// loaded chest's lid rotation directly — no swing on load.
#[derive(Component)]
struct LidSwing {
    started: f32,
    from: f32,
    to: f32,
}

/// How long a lid swing takes (s).
const LID_SWING_DUR: f32 = 0.4;

// ─── Pure helpers ──────────────────────────────────────────────────────────────────

/// Deterministic [0,1) per-position hash — stable loot per chest.
fn tile_hash(x: f32, z: f32) -> f64 {
    let s = (x as f64 * 127.1 + z as f64 * 311.7).sin() * 43758.5453;
    s - s.floor()
}

/// Ease-out-back: starts at 0, overshoots ~10% past 1, settles at 1. (Chest-local copy — the
/// verbs original drives the tree regrow-pop.)
fn ease_out_back(k: f32) -> f32 {
    let k1 = k - 1.0;
    1.0 + 2.70158 * k1 * k1 * k1 + 1.70158 * k1 * k1
}

/// Chance a rolled chest slot pays a consumable (food/potion) instead of wearable gear. The
/// frontier gear pools (`frontier::roll_gear`) are nearly all weapons/armor — the Relic pool is
/// 100% wearable — which buried the hero in near-identical kit. Biasing hard toward consumables
/// (~85%) makes a dropped weapon/armor the rare exception, not the rule; combined with
/// `dedup_wearables` (owned gear is skipped entirely), the bag stops filling with swords.
const CONSUMABLE_RATE: f64 = 0.85;

/// Roll one scattered-chest item: mostly a consumable (scaled by distance), else frontier gear.
/// `roll` in [0,1) — deterministic per chest slot, so loot stays stable across reloads.
fn roll_chest_item(factor: f64, roll: f64) -> &'static str {
    if roll < CONSUMABLE_RATE {
        return if factor > 0.7 {
            "feast"
        } else if factor > 0.4 {
            "potion"
        } else {
            "bread"
        };
    }
    // Remap the leftover range back to [0,1) so the gear pick still spans its whole pool.
    let g = (roll - CONSUMABLE_RATE) / (1.0 - CONSUMABLE_RATE);
    frontier::roll_gear(factor, g)
}

/// True if `id` is a wearable (weapon/armor) the hero already holds — in the bag or equipped.
fn owns_wearable(bag: &Bag, id: &str) -> bool {
    match item_def(id) {
        Some(d) if matches!(d.kind, ItemKind::Weapon | ItemKind::Armor) => {
            bag.has_item(id)
                || bag.equipped_id.as_deref() == Some(id)
                || bag.equipped_armor_id.as_deref() == Some(id)
        }
        _ => false,
    }
}

/// Drop wearable gear the hero already owns (and collapse a repeat within the same haul) so a
/// chest never grants a second copy of a weapon/armor piece — that's just bag clutter. Consumables
/// pass through untouched; order is preserved.
fn dedup_wearables(bag: &Bag, loot: Vec<&'static str>) -> Vec<&'static str> {
    let mut kept: Vec<&'static str> = Vec::new();
    for id in loot {
        let wear = item_def(id)
            .map(|d| matches!(d.kind, ItemKind::Weapon | ItemKind::Armor))
            .unwrap_or(false);
        if wear && (owns_wearable(bag, id) || kept.contains(&id)) {
            continue; // duplicate wearable — ignore
        }
        kept.push(id);
    }
    kept
}

// ─── Open (F) ──────────────────────────────────────────────────────────────────────

/// Press **F** near a closed chest to loot it: gold to the purse + items to the bag (blocked if the
/// bag can't hold the gear), lid swings open with a tier-scaled juice burst. Caches re-close +
/// refill at dawn.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn chest_interact(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<HeroState>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut player: ResMut<PlayerRes>,
    mut cues: MessageWriter<AudioCue>,
    mut speak: MessageWriter<Speak>,
    mut floats: ResMut<FloatQueue>,
    mut feedback: ResMut<HitFeedback>,
    mut commands: Commands,
    mut chests: Query<(&mut Chest, &Transform, &Children), Without<ChestLid>>,
    lids: Query<(), (With<ChestLid>, Without<Chest>)>,
) {
    if !keys.just_pressed(KeyCode::KeyF) || !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for (mut chest, tf, children) in &mut chests {
        if chest.opened {
            continue;
        }
        let p = tf.translation;
        if Vec2::new(p.x, p.z).distance(hero.pos) > CHEST_INTERACT_DIST {
            continue;
        }
        let head = Vec3::new(p.x, p.y + 1.4, p.z);

        // Resolve loot: hand-authored trophy → fixed haul; caches → gold + a loaf; deep-rim hoard →
        // guaranteed top-tier haul + heavy purse; ordinary treasure → frontier gear + gold (Relic
        // rolls the top pool). The curves steepen with distance so the frontier pulls the hero out.
        let (loot, gold): (Vec<&'static str>, i64) = if let Some((g, ids)) = chest.trophy {
            (ids.to_vec(), g)
        } else if chest.cache {
            let mut loot = vec!["bread"];
            if tile_hash(p.x, p.z) > 0.7 {
                loot.push("mercenary_contract");
            }
            (loot, (4.0 + chest.factor * 24.0).round() as i64)
        } else if chest.hoard {
            let h = tile_hash(p.x, p.z);
            let loot = (0..4)
                .map(|i| frontier::roll_gear(1.0, (h + i as f64 * 0.37) % 1.0))
                .collect();
            (loot, (50.0 + chest.factor * 60.0 + h * 25.0).round() as i64)
        } else {
            let h = tile_hash(p.x, p.z);
            let items = 1 + (chest.factor * 2.0).round() as i64;
            // Relic chests roll from the top pool (factor pinned to 1.0) so the deep-biome haul is
            // reliably strong; Wood uses the frontier-graded factor. Gold stays the modest curve —
            // exploration pays in GEAR, not purses.
            let roll_factor = if chest.tier == ChestTier::Relic { 1.0 } else { chest.factor };
            let loot = (0..items)
                .map(|i| roll_chest_item(roll_factor, (h + i as f64 * 0.37) % 1.0))
                .collect();
            (loot, (5.0 + chest.factor * 55.0 + h * 10.0).round() as i64)
        };
        // Ignore duplicate wearables: drop any weapon/armor the hero already owns (or a repeat
        // within this haul) so a chest never stuffs the bag with a second copy. Rolled hauls only —
        // the authored trophy passes through intact.
        let loot = if chest.trophy.is_some() { loot } else { dedup_wearables(&inv.0, loot) };
        // Won't open if the bag can't hold the gear (TS: full bag rejects the chest).
        if !inv.0.has_room_for(&loot) {
            floats.0.push(FloatReq {
                world: head,
                text: "Bag full".into(),
                color: crate::combat_fx::col_block(),
                scale: 1.0,
            });
            cues.write(AudioCue::UiSelect);
            return;
        }
        player.0.add_gold(gold);
        for id in &loot {
            try_grant(&mut inv.0, &mut toasts.0, id, 1, now as f64);
        }
        floats.0.push(FloatReq {
            world: head,
            text: format!("+{gold} gold"),
            color: col_kill(),
            scale: 1.1,
        });
        cues.write(AudioCue::ChestOpen);
        cues.write(AudioCue::Gold); // a coin chime layered over the chest creak

        // Tiered juice: Relic punches the camera a little harder + layers a second coin chime.
        let relic = chest.tier == ChestTier::Relic;
        feedback.trauma = (feedback.trauma + if relic { 0.35 } else { 0.12 }).min(1.0);
        if relic {
            cues.write(AudioCue::Gold); // a second coin chime for the richer payout
        }
        speak.write(Speak::new(Concept::ChestOpen)); // hero muses

        chest.opened = true;
        chest.opened_at = now;
        for &c in children {
            if lids.get(c).is_ok() {
                // Swing the hinge open (overshooting past the catch) instead of snapping.
                commands.entity(c).try_insert(LidSwing { started: now, from: 0.0, to: CHEST_LID_OPEN });
            }
        }
        return; // one chest per press
    }
}

/// Re-close + refill supply caches at dawn (the Wave→Prep clear) — once per survived night. Time
/// alone never restocks them, so loitering through a long prep day earns nothing.
#[allow(clippy::type_complexity)]
fn chest_respawn(
    time: Res<Time>,
    siege: Option<Res<crate::siege::Siege>>,
    mut prev_phase: Local<Option<crate::siege::GamePhase>>,
    mut commands: Commands,
    mut chests: Query<(&mut Chest, &Children), Without<ChestLid>>,
    lids: Query<(), (With<ChestLid>, Without<Chest>)>,
) {
    let Some(siege) = siege else { return };
    let dawned = matches!(
        (*prev_phase, siege.phase),
        (Some(crate::siege::GamePhase::Wave), crate::siege::GamePhase::Prep)
    );
    *prev_phase = Some(siege.phase);
    if !dawned {
        return;
    }
    let now = time.elapsed_secs();
    for (mut chest, children) in &mut chests {
        if chest.cache && chest.opened {
            chest.opened = false;
            for &c in children {
                if lids.get(c).is_ok() {
                    commands.entity(c).try_insert(LidSwing { started: now, from: CHEST_LID_OPEN, to: 0.0 });
                }
            }
        }
    }
}

fn drive_lid_swing(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &LidSwing, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (e, swing, mut tf) in &mut q {
        let k = (now - swing.started) / LID_SWING_DUR;
        if k >= 1.0 {
            tf.rotation = Quat::from_rotation_x(swing.to);
            commands.entity(e).try_remove::<LidSwing>();
            continue;
        }
        // Opening (toward the negative hinge angle) gets the springy overshoot; closing eases.
        let eased = if swing.to < swing.from { ease_out_back(k) } else { k * k * (3.0 - 2.0 * k) };
        tf.rotation = Quat::from_rotation_x(swing.from + (swing.to - swing.from) * eased);
    }
}

// ─── Placement ─────────────────────────────────────────────────────────────────────

/// Scatter chests across the island (called from `worldmap::build`). Every 3rd is a supply cache,
/// the rest treasure; varied distance so the Wood/Relic split spreads. Avoids the courtyard, camps,
/// water and build plots. Non-cache Relic chests get the fancy gold-and-jewel model.
pub fn populate_chests(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    // local u32 alias of the module-level scatter count (loop counters are u32)
    const CHEST_COUNT: u32 = crate::chest::CHEST_COUNT as u32;
    let wood_body = meshes.add(chest_body_mesh());
    let wood_lid = meshes.add(chest_lid_mesh());
    let relic_body = meshes.add(chest_body_mesh_relic());
    let relic_lid = meshes.add(chest_lid_mesh_relic());
    let chest_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.78,
        ..default()
    });

    let mut rng: u32 = 0xc4e5_7a1b;
    let (mut placed, mut attempts) = (0u32, 0u32);
    let mut n_relic = 0u32; // for the placement log (mirrors camps/verbs)
    while placed < CHEST_COUNT && attempts < CHEST_COUNT * 500 + 1000 {
        attempts += 1;
        let x = crate::wildlife::rng_range(&mut rng, -worldmap::GX + 6.0, worldmap::GX - 6.0);
        let z = crate::wildlife::rng_range(&mut rng, -worldmap::GZ + 6.0, worldmap::GZ - 6.0);
        if (x * x + z * z).sqrt() < 14.0
            || worldmap::ground_at_world(x, z).is_none()
            || crate::blockers::is_blocked(x, z)
            || crate::camps::in_clearing(x, z)
            || crate::castle::in_footprint(x, z)
            || crate::town::near_build_plot(x, z)
            || crate::bridges::near_bridge(x, z, 1.0)
        {
            continue;
        }
        let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
        // Only every 3rd chest is a supply cache (the rest are treasure that can be Relic).
        let cache = placed % 3 == 0;
        let factor = forest_frontier(x, z);
        let tier = tier_for(factor);
        if tier == ChestTier::Relic {
            n_relic += 1;
        }
        // The fancy model marks a *better haul*: Relic treasure only (caches stay plain wood).
        let fancy = tier == ChestTier::Relic && !cache;
        let (body, lid) = if fancy { (&relic_body, &relic_lid) } else { (&wood_body, &wood_lid) };
        spawn_chest(
            commands,
            body,
            lid,
            &chest_mat,
            Vec3::new(x, y, z),
            0.0,
            Chest { cache, opened: false, opened_at: 0.0, factor, trophy: None, hoard: false, tier },
            ChestId(placed as usize),
        );
        placed += 1;
    }

    // ── Deep-rim war hoards: one rare top-tier reward per biome, far past the safe zone. ──
    const HOARD_SPOTS: [(f32, f32); 5] = [
        (-69.0, -45.0), // snow massif
        (60.0, -39.0),  // desert deep
        (66.0, 4.0),    // rocky highlands
        (-60.0, 39.0),  // forest heart
        (0.0, 57.0),    // swamp mire
    ];
    for (i, &(ax, az)) in HOARD_SPOTS.iter().enumerate() {
        let mut spot = None;
        'search: for ring in 0..6 {
            let r = ring as f32 * 3.0;
            for k in 0..8 {
                let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                let (x, z) = (ax + ang.cos() * r, az + ang.sin() * r);
                if worldmap::ground_at_world(x, z).is_some()
                    && !crate::blockers::is_blocked(x, z)
                    && !crate::camps::in_clearing(x, z)
                    && !crate::town::near_build_plot(x, z)
                    && !crate::bridges::near_bridge(x, z, 1.0)
                {
                    spot = Some((x, z));
                    break 'search;
                }
                if r == 0.0 {
                    break; // ring 0 is the single centre point
                }
            }
        }
        let Some((x, z)) = spot else { continue };
        let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
        let factor = forest_frontier(x, z);
        // Hoards are always the fancy Relic chest (they hold the best loot in the game).
        spawn_chest(
            commands,
            &relic_body,
            &relic_lid,
            &chest_mat,
            Vec3::new(x, y, z),
            0.0,
            Chest { cache: false, opened: false, opened_at: 0.0, factor, trophy: None, hoard: true, tier: tier_for(factor) },
            ChestId(TROPHY_CHEST_ID + 1 + i),
        );
    }
    info!("chests: {placed} scattered ({n_relic} Relic) + 5 hoards");
}

/// Spawn the single hand-authored war-hoard chest (Gnashfang Hold's plunder) at a fixed mire spot.
/// One-shot treasure with authored `gold` + `loot`, drawn as the fancy Relic chest. Keyed
/// [`TROPHY_CHEST_ID`]. Called from `ork_fortress::build`.
pub fn spawn_trophy_chest(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    pos: Vec3,
    rot: f32,
    gold: i64,
    loot: &'static [&'static str],
) {
    let body = meshes.add(chest_body_mesh_relic());
    let lid = meshes.add(chest_lid_mesh_relic());
    let chest_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.78,
        ..default()
    });
    let factor = forest_frontier(pos.x, pos.z);
    spawn_chest(
        commands,
        &body,
        &lid,
        &chest_mat,
        pos,
        rot,
        Chest { cache: false, opened: false, opened_at: 0.0, factor: 1.0, trophy: Some((gold, loot)), hoard: false, tier: tier_for(factor) },
        ChestId(TROPHY_CHEST_ID),
    );
}

/// Spawn one chest entity: a body + a hinged lid child. Shared by the scatter, the hoards and the
/// trophy; the caller passes the Wood or Relic mesh pair.
fn spawn_chest(
    commands: &mut Commands,
    body_mesh: &Handle<Mesh>,
    lid_mesh: &Handle<Mesh>,
    chest_mat: &Handle<StandardMaterial>,
    pos: Vec3,
    rot: f32,
    chest: Chest,
    id: ChestId,
) {
    commands
        .spawn((
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(rot)),
            Visibility::Visible,
            chest,
            id,
            crate::biome::BiomeEntity,
        ))
        .with_children(|p| {
            p.spawn((Mesh3d(body_mesh.clone()), MeshMaterial3d(chest_mat.clone()), Transform::default()));
            // Lid pivots on the back-top hinge edge (its entity origin).
            p.spawn((
                Mesh3d(lid_mesh.clone()),
                MeshMaterial3d(chest_mat.clone()),
                Transform::from_xyz(0.0, 0.40, -0.25),
                ChestLid,
            ));
        });
}

// ─── Chest models (vertex-coloured, flat-shaded) ────────────────────────────────────
//
// Authored in parent-local space with the body resting on the ground (bottom at y=0). The lid is a
// SEPARATE child whose entity origin sits on the back-top hinge edge — the lid meshes build the lid
// forward of that origin (+Z = front) so the hinge rotation reads naturally. Wood = the common
// chest; Relic = the same shape with gold banding + a jewel in the lid so it reads as "richer".

const CW_BODY: u32 = 0x6b4a2a; // chest wood (body)
const CW_LID: u32 = 0x7a5530; // chest wood (lid — lighter)
const CW_IRON: u32 = 0x2c2c34; // dark iron straps / bands (Wood)
const CW_BRASS: u32 = 0xc9962f; // brass lock + clasp (Wood)
const CW_FOOT: u32 = 0x3a2818; // dark stub feet
const CW_WOOD_RICH: u32 = 0x5a3a22; // darker mahogany body (Relic)
const CW_GOLD: u32 = 0xe0b53a; // bright gold banding / trim (Relic)
const CW_GEM: u32 = 0x35c9d6; // cyan jewel set in the Relic lid + lock

/// Strapped wooden body: a box with iron side-bands, a brass lock plate, a top rim and four stub
/// feet. Bottom sits at local y=0 so it rests on the ground.
fn chest_body_mesh() -> Mesh {
    let iron = lin(CW_IRON);
    cgroup(vec![
        cbx(0.70, 0.40, 0.50, v(0.0, 0.20, 0.0), lin(CW_BODY)), // body
        cbx(0.74, 0.05, 0.54, v(0.0, 0.40, 0.0), iron),         // top rim band
        cbx(0.07, 0.42, 0.54, v(-0.24, 0.20, 0.0), iron),       // left strap
        cbx(0.07, 0.42, 0.54, v(0.24, 0.20, 0.0), iron),        // right strap
        cbx(0.18, 0.20, 0.04, v(0.0, 0.27, 0.26), lin(CW_BRASS)), // front lock plate
        cbx(0.04, 0.07, 0.06, v(0.0, 0.25, 0.28), iron),        // keyhole
        cbx(0.10, 0.10, 0.10, v(-0.28, 0.05, 0.18), lin(CW_FOOT)), // feet
        cbx(0.10, 0.10, 0.10, v(0.28, 0.05, 0.18), lin(CW_FOOT)),
        cbx(0.10, 0.10, 0.10, v(-0.28, 0.05, -0.18), lin(CW_FOOT)),
        cbx(0.10, 0.10, 0.10, v(0.28, 0.05, -0.18), lin(CW_FOOT)),
    ])
}

/// Banded plank lid: a flat slab with a raised centre crown, two iron straps and a front brass
/// clasp. Built around the hinge origin with its underside on local y=0 and the body forward (+Z).
fn chest_lid_mesh() -> Mesh {
    let iron = lin(CW_IRON);
    cgroup(vec![
        cbx(0.72, 0.12, 0.52, v(0.0, 0.06, 0.25), lin(CW_LID)),    // lid slab — underside flush at y=0
        cbx(0.52, 0.09, 0.34, v(0.0, 0.155, 0.25), lin(CW_LID)),   // raised centre crown
        cbx(0.07, 0.18, 0.56, v(-0.24, 0.09, 0.25), iron),         // left strap
        cbx(0.07, 0.18, 0.56, v(0.24, 0.09, 0.25), iron),          // right strap
        cbx(0.14, 0.12, 0.06, v(0.0, -0.04, 0.50), lin(CW_BRASS)), // front clasp
    ])
}

/// Relic body: richer mahogany with GOLD banding/straps, a gold lock plate with a jewelled
/// keyhole, and gold-capped feet — the same silhouette as the Wood chest but obviously finer.
fn chest_body_mesh_relic() -> Mesh {
    let gold = lin(CW_GOLD);
    let wood = lin(CW_WOOD_RICH);
    cgroup(vec![
        cbx(0.70, 0.40, 0.50, v(0.0, 0.20, 0.0), wood),          // body
        cbx(0.76, 0.06, 0.56, v(0.0, 0.40, 0.0), gold),          // gold top rim
        cbx(0.08, 0.42, 0.56, v(-0.24, 0.20, 0.0), gold),        // gold left strap
        cbx(0.08, 0.42, 0.56, v(0.24, 0.20, 0.0), gold),         // gold right strap
        cbx(0.20, 0.22, 0.04, v(0.0, 0.27, 0.26), gold),         // gold lock plate
        cbx(0.05, 0.08, 0.06, v(0.0, 0.25, 0.285), lin(CW_GEM)), // jewelled keyhole
        cbx(0.11, 0.11, 0.11, v(-0.28, 0.05, 0.18), gold),       // gold-capped feet
        cbx(0.11, 0.11, 0.11, v(0.28, 0.05, 0.18), gold),
        cbx(0.11, 0.11, 0.11, v(-0.28, 0.05, -0.18), gold),
        cbx(0.11, 0.11, 0.11, v(0.28, 0.05, -0.18), gold),
    ])
}

/// Relic lid: mahogany slab with GOLD straps + clasp and a faceted jewel set into the crown — the
/// "look better" tell that reads at a glance and at distance.
fn chest_lid_mesh_relic() -> Mesh {
    let gold = lin(CW_GOLD);
    let wood = lin(CW_WOOD_RICH);
    cgroup(vec![
        cbx(0.72, 0.12, 0.52, v(0.0, 0.06, 0.25), wood),          // lid slab
        cbx(0.52, 0.10, 0.34, v(0.0, 0.155, 0.25), wood),         // raised centre crown
        cbx(0.08, 0.19, 0.56, v(-0.24, 0.09, 0.25), gold),        // gold left strap
        cbx(0.08, 0.19, 0.56, v(0.24, 0.09, 0.25), gold),         // gold right strap
        cbx(0.16, 0.13, 0.06, v(0.0, -0.04, 0.50), gold),         // gold front clasp
        cbx(0.18, 0.15, 0.18, v(0.0, 0.25, 0.25), lin(CW_GEM)),   // jewel set in the crown
    ])
}

// ─── Local flat-shaded mesh helpers (mirror the verbs/camps/orks prop builders) ──────

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn ctint(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn cbx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    ctint(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn cgroup(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

// ─── Tests ───────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_splits_at_threshold() {
        assert_eq!(tier_for(0.0), ChestTier::Wood);
        assert_eq!(tier_for(0.29), ChestTier::Wood);
        assert_eq!(tier_for(0.30), ChestTier::Relic);
        assert_eq!(tier_for(1.0), ChestTier::Relic);
    }

    #[test]
    fn dedup_drops_owned_and_repeat_wearables() {
        let mut bag = Bag::new();
        bag.add("iron_armor", 1); // already owned
        let kept = dedup_wearables(&bag, vec!["iron_armor", "sword_gold", "sword_gold", "bread", "bread"]);
        // owned iron_armor dropped, second sword_gold dropped, both breads kept (consumables stack).
        assert_eq!(kept, vec!["sword_gold", "bread", "bread"]);
    }

    #[test]
    fn dedup_counts_equipped_gear_as_owned() {
        let mut bag = Bag::new();
        bag.add("sword_iron", 1);
        let i = bag.bag.iter().position(|s| s.item_id.as_deref() == Some("sword_iron")).unwrap();
        bag.activate_bag_item(i); // equip it (leaves the bag)
        assert!(dedup_wearables(&bag, vec!["sword_iron"]).is_empty(), "equipped weapon counts as owned");
    }

    #[test]
    fn chest_item_biases_consumables_then_falls_back_to_gear() {
        // Below the rate → a consumable, better food deeper out.
        assert_eq!(roll_chest_item(1.0, 0.0), "feast");
        assert_eq!(roll_chest_item(0.5, 0.0), "potion");
        assert_eq!(roll_chest_item(0.0, 0.0), "bread");
        // Above the rate → frontier gear from the matching pool.
        let g = roll_chest_item(1.0, 0.99);
        assert!(["blade_frost", "dragon_plate", "sword_gold", "gold_armor"].contains(&g));
    }
}
