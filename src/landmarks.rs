//! **Landmark POIs** — turns the five biome landmarks (`ruins.rs`: trilithon, frozen spire,
//! sunken pyramid, two giant dead trees), already planted on the walkable island, into real
//! destinations:
//!
//! - **Discovery** — the first time the hero walks within [`DISCOVER_R`] of a landmark it's
//!   *found*: a little pocket gold, a floating "Discovered: …" announce + its lore line, a chime,
//!   and the [`Discoveries`] tally ticks (all five → a bonus). Its signature GEAR stays sealed.
//! - **Rune-Trial** — a found landmark's gear is earned, not given: standing near it shows an
//!   `[E]` prompt (`interaction.rs`) → press it to begin a Hold-the-Rune trial (hold the circle
//!   against a guardian horde; win → the named gear). See the trial systems below.
//! - **Shrine** — once a landmark's gear is claimed (or for gear-less vignettes), the same `[E]`
//!   prays at it for a timed buff (per-biome Resist/Power/Haste, on a [`SHRINE_CD`] cooldown).
//! - **Beacon** — a column of emissive will-o'-wisp motes (unlit → punches through the fog) over a
//!   landmark draws the eye from afar. It marks *both* an undiscovered landmark ("something is out
//!   there") AND a discovered one whose **boost is ready to claim** — a sealed-gear trial, or a
//!   shrine that's off cooldown. It hides only while the shrine rests on its [`SHRINE_CD`] timeout,
//!   then re-lights, so the smoke is a live "usable now" signpost, not a one-shot find marker
//!   ([`sync_beacons`]).
//!
//! All reward plumbing reuses the verbs/inventory path (`try_grant`, `PlayerRes::add_gold`,
//! `FloatQueue`, `AudioCue`). Discovery/shrine state lives on the [`Landmark`] component; the
//! tally lives in the [`Discoveries`] resource. `attach` is called per-landmark from
//! `ruins::populate_landmarks`.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use tileworld_core::buff_store::BuffKind;

use crate::audio::AudioCue;
use crate::biome::{Biome, BiomeEntity};
use crate::combat_fx::{col_kill, FloatQueue, FloatReq};
use crate::game_state::AppState;
use crate::inventory::{try_grant, Buffs, Inventory, Toasts};
use crate::player::{HeroState, PlayerRes};
use crate::ui::fonts::{label, FONT_LABEL, UiFonts};
use crate::game_state::SimAppExt;

/// Walk this close to an unfound landmark → it's discovered.
const DISCOVER_R: f32 = 6.0;
/// Shrine cooldown (s) between prayers — just under a prep day, so ~once per cycle per shrine.
const SHRINE_CD: f32 = 120.0;
/// Shrine buff duration (ms) — longer than a consumable's 12s; it's a trek to earn it.
const SHRINE_BUFF_MS: f64 = 45_000.0;
/// Motes per beacon column.
const BEACON_MOTES: u32 = 22;

// ── Rune-Trial — the gear gate ──────────────────────────────────────────────────────
// Each biome landmark hoards one signature gear piece, SEALED until the hero earns it with a
// "Hold the Rune" trial: press F at a discovered landmark to wake its guardians, then hold the
// rune circle against the onslaught until the meter fills. It reuses the camp ork AI (guardians
// are home-anchored at the landmark, so they aggro the hero like a warband) — a mini-siege, the
// game's own fantasy. Win → the gear is yours; F then prays at the shrine as before. The trial is
// the ONLY source of top-tier gear (random chest/animal rolls are pulled) so acquisition is
// legible: a beacon you see from afar marks exactly where each piece is.

/// Radius of the rune circle the hero must hold to fill the meter. Wide enough that the glowing
/// ring reads as a real arena to defend (not a tight dot underfoot) and the hero has room to swing.
const RUNE_R: f32 = 8.0;
/// Seconds of held ground (inside the circle) to fill the meter and claim the gear.
const HOLD_SECS: f32 = 35.0;
/// The meter drains this much faster than it fills while the hero stands OUTSIDE the circle — so
/// you must defend the spot, not kite the horde in a circle forever.
const DRAIN_MULT: f32 = 1.5;
/// Seconds between guardian spawns while a trial runs.
const GUARD_SPAWN_INTERVAL: f32 = 2.8;
/// Cap on live trial guardians at once — kept low so the trial reads as a tense hold, not a swarm.
const GUARD_MAX_ALIVE: usize = 3;
/// Stray this far from the landmark and the trial aborts (so the horde isn't dragged off).
const TRIAL_ABORT_R: f32 = 22.0;
/// Guardians spawn on a ring this far out — just beyond the (now wider) rune circle.
const GUARD_RING_R: f32 = 12.0;
/// After landmarks are planted, fell every tree within this radius so the set-piece reads from afar
/// and the rune-trial arena (the [`RUNE_R`] ring + the [`GUARD_RING_R`] guardian ring) has open
/// ground. Tree scatter runs long before landmark placement (worldmap build phases 5–9 vs. 23), so
/// trees otherwise crowd right up to a landmark.
/// 13 → 16 (map-character overhaul pass 4, sightline hygiene): a skyline flag hidden behind the
/// treeline pulls nobody toward its road spur — clearing around a landmark is design work.
const LANDMARK_CLEAR_R: f32 = 16.0;

// ── Components / resources ────────────────────────────────────────────────────────

/// A discoverable landmark POI (attached to the `ruins.rs` landmark entity).
#[derive(Component)]
pub struct Landmark {
    pub name: &'static str,
    lore: &'static str,
    buff: BuffKind,
    buff_mag: f64,
    discovered: bool,
    /// Item id of the signature gear sealed here, or `""` for a landmark with no gear (vignette
    /// set-pieces). Granted once the [`RuneTrial`] is won; until then the cache stays sealed.
    gear: &'static str,
    /// Whether the sealed gear has been claimed (the Hold-the-Rune trial was won).
    gear_claimed: bool,
    /// Next `elapsed_secs` at which the shrine may be prayed at again.
    shrine_ready_at: f32,
}

/// One emissive mote in a landmark's beacon column. `name` ties it to its landmark so the
/// column despawns on discovery.
#[derive(Component)]
struct Beacon {
    name: &'static str,
    /// Column foot (world XZ + base Y).
    base: Vec3,
    y_lo: f32,
    y_hi: f32,
    phase: f32,
    rise: f32,
}

impl Landmark {
    /// Whether the hero has already found this landmark (read by the pilgrim hint).
    pub fn is_discovered(&self) -> bool {
        self.discovered
    }

    /// Mark found on a loaded game (no loot/announce — that's the live `discover` path).
    /// The matching beacon is snuffed by [`snuff_found_beacons`] next frame.
    pub(crate) fn set_discovered(&mut self, v: bool) {
        self.discovered = v;
    }

    /// Whether this landmark's sealed gear has been claimed (read by the save snapshot).
    pub fn is_gear_claimed(&self) -> bool {
        self.gear_claimed
    }

    /// Whether this landmark hoards a gear piece at all (vignette set-pieces carry none → only a
    /// shrine). Read by `interaction.rs` to choose the prompt: a sealed-gear landmark offers the
    /// trial, an empty or already-claimed one offers the shrine.
    pub fn has_gear(&self) -> bool {
        !self.gear.is_empty()
    }

    /// Mark the gear claimed on a loaded game (the live path is winning the trial).
    pub(crate) fn set_gear_claimed(&mut self, v: bool) {
        self.gear_claimed = v;
    }
}

/// How many landmarks the hero has found this session (for the all-found bonus + future HUD).
#[derive(Resource, Default)]
pub struct Discoveries {
    pub found: u32,
    pub total: u32,
    pub(crate) completed: bool,
}

struct Meta {
    name: &'static str,
    lore: &'static str,
    buff: BuffKind,
    buff_mag: f64,
    beacon: Color,
    /// The signature gear (item id) sealed at this landmark, claimed by winning its trial.
    gear: &'static str,
}

/// Per-biome landmark flavour: name, lore one-liner, the shrine buff it grants, beacon tint.
fn meta(b: Biome) -> Meta {
    match b {
        // Beacon hues are chosen to CONTRAST each biome's ground (not match it) so the column
        // reads from afar — gold over green forest/grey rock, cyan over white snow / murk, etc.
        Biome::Snow => Meta {
            name: "The Frozen Spire",
            lore: "Ancient ice that turns blades aside.",
            buff: BuffKind::Resist,
            buff_mag: 0.6,
            beacon: Color::srgb(0.20, 0.88, 1.0), // cyan over white snow
            gear: "blade_frost", // Frostfang Greatsword (+34) — frost hoarded in the ice
        },
        Biome::Desert => Meta {
            name: "The Sunken Pyramid",
            lore: "The buried kings lend their wrath.",
            buff: BuffKind::Power,
            buff_mag: 1.4,
            beacon: Color::srgb(0.69, 0.40, 1.0), // violet over tan sand
            gear: "gold_armor", // Gilded Plate (28%) — the buried kings' gold
        },
        Biome::Rocky => Meta {
            name: "The Standing Stones",
            lore: "The old circle quickens the blood.",
            buff: BuffKind::Haste,
            buff_mag: 1.3,
            beacon: Color::srgb(1.0, 0.76, 0.23), // gold over grey rock
            gear: "stone_maul", // Stone Maul (+18) — the circle's own heft
        },
        // NB (July 2026 map-character overhaul): the forest/swamp landmarks were re-identified —
        // Hollow Oak → the Old Mill, Mire Sentinel → the Witch's Hut. Saves round-trip discovery
        // by NAME, so a pre-overhaul save simply shows the new places undiscovered (harmless).
        Biome::Forest => Meta {
            name: "The Old Mill",
            lore: "Its sails still turn, though no wind asks them to.",
            buff: BuffKind::Power,
            buff_mag: 1.4,
            beacon: Color::srgb(1.0, 0.68, 0.18), // amber over green forest
            gear: "sword_gold", // Golden Blade (+21) — the miller's hidden pay
        },
        Biome::Swamp => Meta {
            name: "The Witch's Hut",
            lore: "The brew never cools. The door never opens.",
            buff: BuffKind::Resist,
            buff_mag: 0.6,
            beacon: Color::srgb(0.31, 0.94, 0.90), // bright cyan over murk
            gear: "dragon_plate", // Dragonscale Plate (42%) — payment the witch never collected
        },
    }
}

// ── Public hook (called from ruins::populate_landmarks) ─────────────────────────────

/// Make `entity` (a planted landmark mesh) a discoverable POI and raise its beacon column at
/// `pos` (the landmark's world base).
pub fn attach(
    commands: &mut Commands,
    entity: Entity,
    biome: Biome,
    pos: Vec3,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let m = meta(biome);
    attach_custom(commands, entity, m.name, m.lore, m.buff, m.buff_mag, m.beacon, m.gear, pos, meshes, materials);
}

/// Like [`attach`] but with explicit POI flavour (name / lore / shrine buff / beacon tint) rather
/// than the per-biome defaults — used by the [`crate::vignettes`] set-pieces, which share a biome
/// with a ruin but tell their own story, so they need their own name, buff and beacon hue.
#[allow(clippy::too_many_arguments)]
pub fn attach_custom(
    commands: &mut Commands,
    entity: Entity,
    name: &'static str,
    lore: &'static str,
    buff: BuffKind,
    buff_mag: f64,
    beacon: Color,
    gear: &'static str,
    pos: Vec3,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.entity(entity).try_insert(Landmark {
        name,
        lore,
        buff,
        buff_mag,
        discovered: false,
        gear,
        gear_claimed: false,
        shrine_ready_at: 0.0,
    });
    spawn_beacon(commands, meshes, materials, name, pos, beacon);
}

/// Raise a column of emissive will-o'-wisp motes over a landmark. Tagged [`BiomeEntity`] so the
/// debug biome-swap (keys 1–5) reaps them with the rest of the island.
fn spawn_beacon(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    name: &'static str,
    pos: Vec3,
    color: Color,
) {
    let mesh = meshes.add(Sphere::new(0.24).mesh().ico(1).unwrap());
    // Strong emissive → blooms into a glowing column that punches through the haze.
    let mat = materials.add(StandardMaterial {
        base_color: color.with_alpha(0.95),
        emissive: LinearRgba::from(color) * 16.0,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let y_lo = pos.y + 1.5;
    let y_hi = pos.y + 13.0;
    for i in 0..BEACON_MOTES {
        let h0 = hash(i);
        let h1 = hash(i + 911);
        let h2 = hash(i + 7331);
        // Tight column with a little jitter so it reads as a rising wisp, not a pole.
        let off = Vec3::new((h0 - 0.5) * 0.7, 0.0, (h1 - 0.5) * 0.7);
        let y = y_lo + h2 * (y_hi - y_lo);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(pos.x + off.x, y, pos.z + off.z)
                .with_scale(Vec3::splat(0.7 + h0 * 0.7)),
            // Explicit Visibility so `sync_beacons` can toggle the column on the shrine cooldown.
            Visibility::Inherited,
            Beacon {
                name,
                base: Vec3::new(pos.x + off.x, 0.0, pos.z + off.z),
                y_lo,
                y_hi,
                phase: h1 * std::f32::consts::TAU,
                rise: 1.1 + h2 * 0.9,
            },
            NotShadowCaster,
            BiomeEntity,
        ));
    }
}

/// Deterministic [0,1) integer hash (per-mote variation), same family as the scatter RNG.
fn hash(n: u32) -> f32 {
    let mut t = n.wrapping_mul(0x6d2b_79f5).wrapping_add(0x9e37_79b9);
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}

// ── Plugin ──────────────────────────────────────────────────────────────────────────

pub struct LandmarksPlugin;

impl Plugin for LandmarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Discoveries>()
            .init_resource::<RuneTrial>()
            .add_message::<LandmarkInteract>()
            // Wipe the found/completed tally on a fresh run — the freshly-rebuilt `Landmark` entities
            // reset to undiscovered, but this resource is not `BiomeEntity` and otherwise leaks across
            // runs, letting the all-found +75g bonus fire early (or never) in the next run.
            .add_systems(OnExit(AppState::StartScreen), reset_discoveries)
            .add_systems(OnExit(AppState::GameOver), reset_discoveries)
            // Beacon drift is a visual — runs even while the world is frozen, like the particles.
            .add_systems(Update, beacon_drift)
            // Light/snuff each beacon to mirror its landmark's "boost ready" state (ungated, so it
            // tracks through the save-restore path too — that flips `discovered` directly).
            .add_systems(Update, sync_beacons)
            // The trial HUD + rune ring draw ungated (like the rest of the HUD) so they show + clean
            // up through any frame.
            .add_systems(Update, (sync_rune_hud, sync_rune_ring))
            // Fell trees crowding the landmarks once they've spawned (runs once, then idles).
            .add_systems(Update, clear_around_landmarks)
            .add_sim_systems(
                (track_total, discover, shrine, start_rune_trial, drive_rune_trial)
                    ,
            );
    }
}

/// Fell every tree and despawn merged ground-cover chunks within [`LANDMARK_CLEAR_R`] of a
/// landmark, once, after the landmarks exist. Tree/cover scatter (worldmap build phases 5–9
/// and 12) runs long before landmark placement (phase 23), so props crowd right up to the
/// set-pieces; this opens the ground around each so flowers don't poke through the mesh and
/// the rune-trial arena is clear. Cover chunks use a slightly wider radius (chunk half-extent)
/// so a 16×16 merged mesh can't straddle the landmark with one corner still full of tufts.
fn clear_around_landmarks(
    mut commands: Commands,
    landmarks: Query<&Transform, With<Landmark>>,
    trees: Query<(Entity, &Transform), With<crate::verbs::ChopTree>>,
    cover: Query<(Entity, &Transform), With<crate::biome::GroundCoverChunk>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let centers: Vec<Vec2> = landmarks.iter().map(|tf| Vec2::new(tf.translation.x, tf.translation.z)).collect();
    if centers.is_empty() {
        return; // landmarks not planted yet — try again next frame
    }
    *done = true;
    let r2 = LANDMARK_CLEAR_R * LANDMARK_CLEAR_R;
    let cover_r = LANDMARK_CLEAR_R + crate::biome::COVER_CHUNK * 0.75;
    let cover_r2 = cover_r * cover_r;
    for (e, tf) in &trees {
        let p = Vec2::new(tf.translation.x, tf.translation.z);
        if centers.iter().any(|c| c.distance_squared(p) < r2) {
            crate::blockers::remove_at(p.x, p.y); // p.y is world Z
            commands.entity(e).try_despawn();
        }
    }
    for (e, tf) in &cover {
        let p = Vec2::new(tf.translation.x, tf.translation.z);
        if centers.iter().any(|c| c.distance_squared(p) < cover_r2) {
            commands.entity(e).try_despawn();
        }
    }
}

/// New Game / Restart / Play Again: zero the found/completed tally (keep `total` — the landmark
/// count is constant across runs, re-latched by `track_total` anyway). Mirrors the reset systems on
/// every other earned run-state resource (economy, quests, siege, lives, rescues).
fn reset_discoveries(mut disc: ResMut<Discoveries>) {
    disc.found = 0;
    disc.completed = false;
}

/// Latch the landmark count into [`Discoveries::total`] once they've spawned (drives the
/// all-found bonus). Runs until it sees a non-zero count, then idles.
fn track_total(mut disc: ResMut<Discoveries>, q: Query<(), With<Landmark>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let n = q.iter().count() as u32;
    if n > 0 {
        disc.total = n;
        *done = true;
    }
}

/// Drift each beacon mote up its column, wrap at the top, gentle sway — a slow rising wisp.
fn beacon_drift(time: Res<Time>, mut q: Query<(&Beacon, &mut Transform)>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs_wrapped();
    for (b, mut tf) in &mut q {
        tf.translation.y += b.rise * dt;
        if tf.translation.y > b.y_hi {
            tf.translation.y = b.y_lo;
        }
        tf.translation.x = b.base.x + (t * 0.8 + b.phase).sin() * 0.25;
        tf.translation.z = b.base.z + (t * 0.7 + b.phase * 1.7).cos() * 0.25;
    }
}

/// Whether a landmark's beacon column should currently glow: an undiscovered landmark (find-me),
/// a discovered one with sealed gear still to claim (its trial is a boost waiting), or a shrine
/// whose [`SHRINE_CD`] cooldown has elapsed. A shrine resting on cooldown goes dark — that gap IS
/// the "used, come back later" signal.
fn beacon_shown(lm: &Landmark, now: f32) -> bool {
    if !lm.discovered {
        return true; // undiscovered → the original "something's out there" find marker
    }
    if lm.has_gear() && !lm.gear_claimed {
        return true; // sealed gear → its Hold-the-Rune trial is the boost to claim
    }
    now >= lm.shrine_ready_at // shrine: lit when ready, dark while on its cooldown timeout
}

/// Toggle each beacon's visibility to mirror its landmark's "boost ready" state (matched by name).
/// Replaces the old despawn-on-discovery: the column persists so a known-but-ready shrine still
/// shows its smoke, and only winks out for the shrine's cooldown — making reusability legible.
/// Ungated + cheap (≤ a handful of landmarks × their motes), so it also covers the save-restore
/// path, where `Landmark::set_discovered` flips the flag with no beacon bookkeeping.
fn sync_beacons(
    time: Res<Time>,
    landmarks: Query<&Landmark>,
    mut beacons: Query<(&Beacon, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    for (b, mut vis) in &mut beacons {
        let show = landmarks
            .iter()
            .find(|lm| lm.name == b.name)
            .is_some_and(|lm| beacon_shown(lm, now));
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

/// First approach into [`DISCOVER_R`] discovers a landmark: announce it + its lore, a little pocket
/// gold, chime, tick the tally (all found → bonus), and snuff its beacon. The landmark's signature
/// GEAR is NOT granted here — it stays sealed until the hero wins its Hold-the-Rune trial (press F);
/// discovery just reveals + hints at the cache.
#[allow(clippy::too_many_arguments)]
fn discover(
    hero: Res<HeroState>,
    mut player: ResMut<PlayerRes>,
    mut floats: ResMut<FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut disc: ResMut<Discoveries>,
    mut q: Query<(&mut Landmark, &Transform)>,
) {
    if !hero.alive {
        return;
    }
    for (mut lm, tf) in &mut q {
        if lm.discovered {
            continue;
        }
        let p = tf.translation;
        if Vec2::new(p.x, p.z).distance(hero.pos) > DISCOVER_R {
            continue;
        }
        lm.discovered = true;

        // A little pocket gold for the find — the real prize is the SEALED gear, earned by trial.
        // The purse stays deliberately small (gold belongs to the town's tithe + bounties).
        let factor = frontier_factor(p.x, p.z);
        let gold = (10.0 + factor * 30.0).round() as i64;
        player.0.add_gold(gold);

        // Announce.
        floats.0.push(FloatReq {
            world: Vec3::new(p.x, p.y + 3.2, p.z),
            text: format!("Discovered: {}", lm.name),
            color: Color::srgb(1.0, 0.9, 0.5),
            scale: 1.4,
        });
        floats.0.push(FloatReq {
            world: Vec3::new(p.x, p.y + 2.5, p.z),
            text: lm.lore.into(),
            color: Color::srgb(0.85, 0.85, 0.95),
            scale: 0.9,
        });
        floats.0.push(FloatReq {
            world: Vec3::new(p.x, p.y + 1.9, p.z),
            text: format!("+{gold} gold"),
            color: col_kill(),
            scale: 1.1,
        });
        // Hint at the sealed gear (the contextual [E] prompt shows when you stand near it).
        if !lm.gear.is_empty() {
            floats.0.push(FloatReq {
                world: Vec3::new(p.x, p.y + 1.3, p.z),
                text: "A sealed cache hums within — stand close to challenge its guardians".into(),
                color: Color::srgb(0.75, 0.92, 1.0),
                scale: 0.85,
            });
        }
        cues.write(AudioCue::ChestOpen);
        cues.write(AudioCue::Gold);
        speak.write(crate::audio::Speak::new(crate::audio::Concept::ChestOpen));

        // The beacon is NOT snuffed on discovery any more — `sync_beacons` keeps it lit as a
        // "boost ready" marker (sealed-gear trial / off-cooldown shrine) and only hides it while
        // the shrine rests, so a usable landmark always shows its smoke.

        disc.found += 1;
        if disc.total > 0 && disc.found >= disc.total && !disc.completed {
            disc.completed = true;
            player.0.add_gold(75);
            floats.0.push(FloatReq {
                world: Vec3::new(p.x, p.y + 4.0, p.z),
                text: "The isle holds no more secrets — +75 gold".into(),
                color: Color::srgb(1.0, 0.95, 0.6),
                scale: 1.3,
            });
            cues.write(AudioCue::LevelUp);
        }
    }
}

/// Interacting (**E**, via `interaction.rs`) with a found landmark whose gear is already claimed (or
/// a vignette that never had gear) prays at its shrine: a timed per-biome buff on a cooldown. Range
/// is gated by the interaction resolver. Sealed-gear landmarks are handled by `start_rune_trial`.
#[allow(clippy::too_many_arguments)]
fn shrine(
    mut events: MessageReader<LandmarkInteract>,
    time: Res<Time>,
    hero: Res<HeroState>,
    mut buffs: ResMut<Buffs>,
    mut floats: ResMut<FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(&mut Landmark, &Transform)>,
) {
    if !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for LandmarkInteract(e) in events.read() {
        let Ok((mut lm, tf)) = q.get_mut(*e) else { continue };
        if !lm.discovered {
            continue;
        }
        // Sealed gear → that's the trial's job (`start_rune_trial`), not a prayer. (Vignettes carry
        // no gear, so `has_gear()` is false and they fall straight through to praying.)
        if lm.has_gear() && !lm.gear_claimed {
            continue;
        }
        let p = tf.translation;
        let head = Vec3::new(p.x, p.y + 2.6, p.z);
        if now < lm.shrine_ready_at {
            let left = (lm.shrine_ready_at - now).ceil() as i64;
            floats.0.push(FloatReq {
                world: head,
                text: format!("The shrine rests ({left}s)"),
                color: Color::srgb(0.7, 0.7, 0.75),
                scale: 0.9,
            });
            cues.write(AudioCue::UiSelect);
            continue;
        }
        lm.shrine_ready_at = now + SHRINE_CD;
        buffs.0.apply_buff(lm.buff, SHRINE_BUFF_MS, lm.buff_mag, now as f64);
        floats.0.push(FloatReq {
            world: head,
            text: format!("{} blessing — {}", lm.name, lm.buff.label()),
            color: Color::srgb(0.7, 1.0, 0.85),
            scale: 1.2,
        });
        cues.write(AudioCue::Forage);
        cues.write(AudioCue::LevelUp);
    }
}

// ── Rune-Trial — systems that run the "Hold the Rune" gear gate ──────────────────────

/// Find a DRY-LAND spawn point for a trial guardian near the [`GUARD_RING_R`] ring, starting from
/// `ang`. Landmarks beside a river/lake/coast have ring arcs out over water, where the old fixed
/// placement dropped orks into the sea; this sweeps the ring and steps inward (never inside the
/// [`RUNE_R`] hold circle, so a guardian can't pop on top of the hero) for a spot whose whole
/// footprint is standable. `None` if every probe is wet — the caller then skips that spawn tick.
fn guardian_landfall(center: Vec2, ang: f32) -> Option<Vec2> {
    use std::f32::consts::TAU;
    const BODY_R: f32 = 0.45; // ork footprint — keep the whole body off the water, not just its centre
    // Outer ring first, then inward in 1-unit steps, stopping above RUNE_R (=8) so guardians stay
    // outside the arena the hero defends.
    for step in 0..=3 {
        let radius = GUARD_RING_R - step as f32; // 12, 11, 10, 9
        for k in 0..16 {
            let a = ang + k as f32 / 16.0 * TAU;
            let p = center + Vec2::new(a.cos(), a.sin()) * radius;
            if let Some(y) = crate::steer::footing(p.x, p.y) {
                if crate::steer::can_stand(p.x, p.y, BODY_R, y) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// A guardian summoned by an active Rune-Trial — a camp-style ork home-anchored at the landmark
/// (so it aggros the hero like a warband, via the shared `orks::ork_brain`). Tagged so the trial
/// can sweep them all on win/abort. Home ≠ any camp centre, so the camp systems ignore them.
#[derive(Component)]
struct TrialGuardian;

/// The single in-flight Rune-Trial, if any (only one runs at a time). Transient — never saved (a
/// save can only be written in Prep and the trial aborts the instant night falls, so none is ever
/// live at snapshot time).
#[derive(Resource, Default)]
pub struct RuneTrial(Option<ActiveTrial>);

impl RuneTrial {
    /// Whether a Hold-the-Rune trial is currently running. Read by `interaction.rs` to hide the
    /// `[E] Challenge the guardians` prompt once the fight has begun (you can't start a second one).
    pub fn is_active(&self) -> bool {
        self.0.is_some()
    }
}

/// Fired by `interaction.rs` when the hero presses **E** at a landmark (the contextual-prompt
/// system that names the action on a visible chip). The landmark systems decide what it means from
/// the landmark's state: a sealed-gear landmark starts the trial, an empty/claimed one prays.
#[derive(Message)]
pub struct LandmarkInteract(pub Entity);

struct ActiveTrial {
    /// The landmark entity whose sealed gear this trial unlocks.
    landmark: Entity,
    name: &'static str,
    gear: &'static str,
    /// World-XZ centre of the rune circle (the landmark base).
    center: Vec2,
    /// 0→1 hold meter; reaching 1.0 wins.
    progress: f32,
    /// Held at 1.0 once filled — guardians cleared, just waiting for the gear to land in the bag.
    won: bool,
    /// `elapsed_secs` of the next guardian spawn.
    next_spawn: f32,
    /// Rotating index (ring placement + variant cycle).
    spawn_i: u32,
}

/// Begin a Hold-the-Rune trial when the hero interacts (**E**, via `interaction.rs`) with a
/// discovered landmark whose gear is still SEALED. Day/Prep only (no stacking two hordes), one trial
/// at a time. Range is already gated by the interaction resolver; we re-check state here.
#[allow(clippy::too_many_arguments)]
fn start_rune_trial(
    mut events: MessageReader<LandmarkInteract>,
    hero: Res<HeroState>,
    siege: Res<crate::siege::Siege>,
    time: Res<Time>,
    mut trial: ResMut<RuneTrial>,
    mut floats: ResMut<FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    q: Query<(&Landmark, &Transform)>,
) {
    let now = time.elapsed_secs();
    for LandmarkInteract(e) in events.read() {
        let Ok((lm, tf)) = q.get(*e) else { continue };
        if !lm.discovered || !lm.has_gear() || lm.gear_claimed || !hero.alive {
            continue; // not a sealed-gear landmark (the shrine system handles claimed/empty)
        }
        let p = tf.translation;
        let head = Vec3::new(p.x, p.y + 2.6, p.z);
        if trial.0.is_some() {
            push_hint(&mut floats, &mut cues, head, "Another trial is already underway");
            continue;
        }
        if siege.phase != crate::siege::GamePhase::Prep {
            push_hint(&mut floats, &mut cues, head, "Face the guardians by day — not mid-siege");
            continue;
        }
        trial.0 = Some(ActiveTrial {
            landmark: *e,
            name: lm.name,
            gear: lm.gear,
            center: Vec2::new(p.x, p.z),
            progress: 0.0,
            won: false,
            next_spawn: now,
            spawn_i: 0,
        });
        floats.0.push(FloatReq {
            world: head,
            text: format!("{} stirs — hold the rune!", lm.name),
            color: Color::srgb(0.8, 0.95, 1.0),
            scale: 1.3,
        });
        cues.write(AudioCue::ChestOpen);
    }
}

/// Small grey "can't right now" float + click.
fn push_hint(floats: &mut FloatQueue, cues: &mut MessageWriter<AudioCue>, head: Vec3, msg: &str) {
    floats.0.push(FloatReq { world: head, text: msg.into(), color: Color::srgb(0.85, 0.7, 0.6), scale: 0.9 });
    cues.write(AudioCue::UiSelect);
}

/// Drive the in-flight Rune-Trial: fill the meter while the hero holds the circle (drain while he
/// strays), spawn guardians on a cadence up to the cap, and resolve win (grant the gear, mark it
/// claimed) / abort (hero down, hero fled past [`TRIAL_ABORT_R`], or night fell). Reuses the camp
/// ork [`crate::orks::Armory`] (kept alive in `camps::CampWarbands`) so guardians fight with the
/// real warband AI. Gated to `Modal::None` like the rest of the world-sim.
#[allow(clippy::too_many_arguments)]
fn drive_rune_trial(
    time: Res<Time>,
    hero: Res<HeroState>,
    siege: Res<crate::siege::Siege>,
    warbands: Option<Res<crate::camps::CampWarbands>>,
    mut trial: ResMut<RuneTrial>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut floats: ResMut<FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut commands: Commands,
    mut landmarks: Query<&mut Landmark>,
    guardians: Query<Entity, With<TrialGuardian>>,
    alive_guardians: Query<&TrialGuardian, Without<crate::dying::Dying>>,
) {
    if trial.0.is_none() {
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();

    // What to do AFTER the `active` borrow ends (we can't reassign `trial.0` while it's borrowed).
    enum Outcome {
        Continue,
        Abort(Vec2),
        Claimed,
    }

    let outcome = {
        let active = trial.0.as_mut().unwrap();

        // ── Abort: hero down, strayed too far, or night fell.
        let strayed = hero.pos.distance(active.center) > TRIAL_ABORT_R;
        if !hero.alive || strayed || siege.phase != crate::siege::GamePhase::Prep {
            Outcome::Abort(active.center)
        } else {
            if !active.won {
                // Fill while inside the circle; drain faster while outside.
                let inside = hero.pos.distance(active.center) <= RUNE_R;
                let step = if inside { dt / HOLD_SECS } else { -dt / HOLD_SECS * DRAIN_MULT };
                active.progress = (active.progress + step).clamp(0.0, 1.0);

                // Spawn guardians on a cadence, up to the live cap.
                if now >= active.next_spawn && alive_guardians.iter().count() < GUARD_MAX_ALIVE {
                    if let Some(wb) = &warbands {
                        let i = active.spawn_i;
                        let ang = i as f32 * 2.399_963; // golden-angle spread around the rune
                        // Land the guardian on dry ground: a landmark beside a river/lake/coast has
                        // ring points out over water, and the old fixed-ring placement dropped orks
                        // INTO the water. Sweep the ring (and step inward, never inside the hold
                        // circle) for a standable spot; if the whole sweep is wet, skip this tick and
                        // retry on the next cadence.
                        if let Some(pos) = guardian_landfall(active.center, ang) {
                            const VARIANTS: [crate::orks::OrkVariant; 4] = [
                                crate::orks::OrkVariant::Grunt,
                                crate::orks::OrkVariant::Scout,
                                crate::orks::OrkVariant::Berserker,
                                crate::orks::OrkVariant::Grunt,
                            ];
                            let variant = VARIANTS[i as usize % VARIANTS.len()];
                            let seed = i.wrapping_mul(0x9e37_79b1) ^ 0x51ed_2c01;
                            // Home-anchored AT the landmark so it aggros the hero holding the rune.
                            let g = wb.armory.spawn(&mut commands, variant, crate::orks::Faction::Red, active.center, pos, seed);
                            commands.entity(g).try_insert(TrialGuardian);
                            active.spawn_i += 1;
                        }
                        active.next_spawn = now + GUARD_SPAWN_INTERVAL;
                    }
                }

                // Meter full → fight's over: clear guardians, announce, flip to grant-pending.
                if active.progress >= 1.0 {
                    active.won = true;
                    for e in &guardians {
                        commands.entity(e).try_despawn();
                    }
                    floats.0.push(FloatReq {
                        world: Vec3::new(active.center.x, 3.0, active.center.y),
                        text: format!("{} yields its prize!", active.name),
                        color: Color::srgb(1.0, 0.92, 0.5),
                        scale: 1.4,
                    });
                    cues.write(AudioCue::LevelUp);
                    speak.write(crate::audio::Speak::new(crate::audio::Concept::ChestOpen));
                }
            }

            // Grant-pending: only mark claimed + end once the gear actually lands, so a full
            // satchel never silently eats an earned piece.
            if active.won {
                if try_grant(&mut inv.0, &mut toasts.0, active.gear, 1, now as f64) {
                    // Auto-WIELD the prize if it beats what's equipped — a hard-won top-tier piece
                    // should land in hand, not sit unnoticed in the satchel. A side-grade/worse
                    // piece stays bagged so the hero is never downgraded.
                    let equipped = if inv.0.is_gear_upgrade(active.gear) {
                        match inv.0.bag.iter().position(|s| s.item_id.as_deref() == Some(active.gear)) {
                            Some(i) => {
                                inv.0.activate_bag_item(i);
                                true
                            }
                            None => false,
                        }
                    } else {
                        false
                    };
                    if let Ok(mut lm) = landmarks.get_mut(active.landmark) {
                        lm.gear_claimed = true;
                    }
                    floats.0.push(FloatReq {
                        world: Vec3::new(active.center.x, 2.4, active.center.y),
                        text: if equipped {
                            "Claimed and equipped!".into()
                        } else {
                            "Claimed — open your satchel (I) to equip".into()
                        },
                        color: Color::srgb(0.7, 1.0, 0.75),
                        scale: 1.0,
                    });
                    Outcome::Claimed
                } else {
                    if now >= active.next_spawn {
                        floats.0.push(FloatReq {
                            world: Vec3::new(active.center.x, 2.6, active.center.y),
                            text: "Satchel full — make room to take the prize".into(),
                            color: Color::srgb(0.9, 0.7, 0.6),
                            scale: 0.9,
                        });
                        active.next_spawn = now + 2.0;
                    }
                    Outcome::Continue
                }
            } else {
                Outcome::Continue
            }
        }
    };

    match outcome {
        Outcome::Abort(c) => {
            for e in &guardians {
                commands.entity(e).try_despawn();
            }
            floats.0.push(FloatReq {
                world: Vec3::new(c.x, 2.6, c.y),
                text: "The rune goes dark — the trial resets".into(),
                color: Color::srgb(0.85, 0.55, 0.5),
                scale: 1.0,
            });
            cues.write(AudioCue::UiSelect);
            trial.0 = None;
        }
        Outcome::Claimed => {
            trial.0 = None;
        }
        Outcome::Continue => {}
    }
}

/// Bottom-centre progress banner shown only while a Rune-Trial runs — the legible "hold X%" meter.
#[derive(Component)]
struct RuneHud;
#[derive(Component)]
struct RuneHudFill;
#[derive(Component)]
struct RuneHudLabel;

/// Spawn the trial HUD when a trial begins, update its fill + label live, and despawn it when the
/// trial ends. Ungated (draws even if the world were frozen), like the rest of the HUD.
fn sync_rune_hud(
    trial: Res<RuneTrial>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
    existing: Query<Entity, With<RuneHud>>,
    mut fill: Query<&mut Node, (With<RuneHudFill>, Without<RuneHud>)>,
    mut labels: Query<&mut Text, With<RuneHudLabel>>,
) {
    let Some(active) = &trial.0 else {
        for e in &existing {
            commands.entity(e).try_despawn();
        }
        return;
    };
    if existing.is_empty() {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(96.0),
                    left: Val::Percent(50.0),
                    margin: UiRect::left(Val::Px(-150.0)),
                    width: Val::Px(300.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(4.0),
                    ..default()
                },
                GlobalZIndex(70),
                RuneHud,
            ))
            .with_children(|root| {
                root.spawn((label(&fonts.display, "Hold the Rune", FONT_LABEL, Color::srgb(0.85, 0.95, 1.0)), RuneHudLabel));
                // Bar track.
                root.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(12.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.08, 0.10, 0.13)),
                    BorderColor::all(Color::srgb(0.45, 0.65, 0.8)),
                ))
                .with_children(|track| {
                    track.spawn((
                        Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() },
                        BackgroundColor(Color::srgb(0.35, 0.85, 1.0)),
                        RuneHudFill,
                    ));
                });
            });
    }
    let pct = (active.progress * 100.0).round();
    if let Ok(mut n) = fill.single_mut() {
        n.width = Val::Percent(active.progress * 100.0);
    }
    if let Ok(mut t) = labels.single_mut() {
        t.0 = format!("{} — hold the rune  {pct:.0}%", active.name);
    }
}

/// A glowing marker on the rune circle — spawned in a ring while a trial runs so the hold zone is
/// unmistakable, swept when it ends.
#[derive(Component)]
struct RuneRing;

/// Spawn a ring of glowing markers around the rune circle while a trial runs (so the hero SEES
/// exactly where to stand to hold), and sweep them the instant it ends.
fn sync_rune_ring(
    trial: Res<RuneTrial>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<RuneRing>>,
) {
    let Some(active) = &trial.0 else {
        for e in &existing {
            commands.entity(e).try_despawn();
        }
        return;
    };
    if !existing.is_empty() {
        return; // ring already raised
    }
    let mesh = meshes.add(Sphere::new(0.17).mesh().ico(2).unwrap());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.9, 1.0),
        emissive: LinearRgba::rgb(0.4, 1.8, 2.6), // bright cyan glow — punches through the scene
        unlit: true,
        ..default()
    });
    let cy = crate::worldmap::ground_at_world(active.center.x, active.center.y).unwrap_or(0.0) + 0.3;
    const N: u32 = 48;
    for i in 0..N {
        let a = i as f32 / N as f32 * std::f32::consts::TAU;
        let x = active.center.x + a.cos() * RUNE_R;
        let z = active.center.y + a.sin() * RUNE_R;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, cy, z),
            RuneRing,
            NotShadowCaster,
        ));
    }
}

// ── Frontier gradient (the small discovery purse scales with distance from the castle) ────

fn frontier_factor(x: f32, z: f32) -> f64 {
    const SAFE: f32 = 22.0;
    const RIM: f32 = 92.0;
    let d = (x * x + z * z).sqrt();
    let t = ((d - SAFE) / (RIM - SAFE)).clamp(0.0, 1.0) as f64;
    t * t * (3.0 - 2.0 * t)
}
