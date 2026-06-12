//! **Landmark POIs** — turns the five biome landmarks (`ruins.rs`: trilithon, frozen spire,
//! sunken pyramid, two giant dead trees), already planted on the walkable island, into real
//! destinations:
//!
//! - **Discovery** — the first time the hero walks within [`DISCOVER_R`] of a landmark it's
//!   *found*: a one-time cache (gold + a frontier-graded relic), a floating "Discovered: …"
//!   announce + its lore line, a chime, and the [`Discoveries`] tally ticks (all five → a bonus).
//! - **Shrine** — once found, **F** within [`SHRINE_R`] prays at it for a timed buff (per-biome
//!   Resist/Power/Haste, on a [`SHRINE_CD`] cooldown). The repeatable reason to come back.
//! - **Beacon** — over each *undiscovered* landmark a column of emissive will-o'-wisp motes
//!   (unlit → punches through the fog) draws the eye from afar. It despawns once the place is
//!   found. Ambient life as a signpost: "something is out there."
//!
//! All reward plumbing reuses the verbs/inventory path (`try_grant`, `PlayerRes::add_gold`,
//! `FloatQueue`, `AudioCue`). Discovery/shrine state lives on the [`Landmark`] component; the
//! tally lives in the [`Discoveries`] resource. `attach` is called per-landmark from
//! `ruins::populate_landmarks`.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use tileworld_core::buff_store::BuffKind;
use tileworld_core::frontier;

use crate::audio::AudioCue;
use crate::biome::{Biome, BiomeEntity};
use crate::combat_fx::{col_kill, FloatQueue, FloatReq};
use crate::game_state::Modal;
use crate::inventory::{try_grant, Buffs, Inventory, Toasts};
use crate::player::{HeroState, PlayerRes};

/// Walk this close to an unfound landmark → it's discovered.
const DISCOVER_R: f32 = 6.0;
/// Press **F** this close to a found landmark → pray at its shrine.
const SHRINE_R: f32 = 3.5;
/// Shrine cooldown (s) between prayers — just under a prep day, so ~once per cycle per shrine.
const SHRINE_CD: f32 = 120.0;
/// Shrine buff duration (ms) — longer than a consumable's 12s; it's a trek to earn it.
const SHRINE_BUFF_MS: f64 = 45_000.0;
/// Motes per beacon column.
const BEACON_MOTES: u32 = 22;

// ── Components / resources ────────────────────────────────────────────────────────

/// A discoverable landmark POI (attached to the `ruins.rs` landmark entity).
#[derive(Component)]
pub struct Landmark {
    pub name: &'static str,
    lore: &'static str,
    buff: BuffKind,
    buff_mag: f64,
    discovered: bool,
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
        },
        Biome::Desert => Meta {
            name: "The Sunken Pyramid",
            lore: "The buried kings lend their wrath.",
            buff: BuffKind::Power,
            buff_mag: 1.4,
            beacon: Color::srgb(0.69, 0.40, 1.0), // violet over tan sand
        },
        Biome::Rocky => Meta {
            name: "The Standing Stones",
            lore: "The old circle quickens the blood.",
            buff: BuffKind::Haste,
            buff_mag: 1.3,
            beacon: Color::srgb(1.0, 0.76, 0.23), // gold over grey rock
        },
        Biome::Forest => Meta {
            name: "The Hollow Oak",
            lore: "Roots drink deep; so shall you strike.",
            buff: BuffKind::Power,
            buff_mag: 1.4,
            beacon: Color::srgb(1.0, 0.68, 0.18), // amber over green forest
        },
        Biome::Swamp => Meta {
            name: "The Mire Sentinel",
            lore: "Bog-iron bark shrugs off the worst.",
            buff: BuffKind::Resist,
            buff_mag: 0.6,
            beacon: Color::srgb(0.31, 0.94, 0.90), // bright cyan over murk
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
    attach_custom(commands, entity, m.name, m.lore, m.buff, m.buff_mag, m.beacon, pos, meshes, materials);
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
            // Beacon drift is a visual — runs even while the world is frozen, like the particles.
            .add_systems(Update, beacon_drift)
            // Reconcile beacons to discovery state (ungated): catches the save-restore path, which
            // flips `discovered` directly without going through `discover`'s beacon snuff.
            .add_systems(Update, snuff_found_beacons)
            .add_systems(
                Update,
                (track_total, discover, shrine).run_if(in_state(Modal::None)),
            );
    }
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

/// Despawn any beacon whose landmark is already discovered. The live `discover` path snuffs its
/// own beacon inline; this covers the **save-restore** path, where `Landmark::set_discovered`
/// flips the flag with no beacon bookkeeping. Cheap: idles once every found landmark's column is
/// gone (the common steady state), only doing work the frame after a load marks new discoveries.
fn snuff_found_beacons(
    mut commands: Commands,
    found: Query<&Landmark>,
    beacons: Query<(Entity, &Beacon)>,
) {
    for (e, b) in &beacons {
        if found.iter().any(|lm| lm.discovered && lm.name == b.name) {
            commands.entity(e).try_despawn();
        }
    }
}

/// First approach into [`DISCOVER_R`] discovers a landmark: bank a one-time graded cache, announce
/// it + its lore, chime, tick the tally (all found → bonus), and snuff its beacon.
#[allow(clippy::too_many_arguments)]
fn discover(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut player: ResMut<PlayerRes>,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut floats: ResMut<FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut disc: ResMut<Discoveries>,
    mut commands: Commands,
    mut q: Query<(&mut Landmark, &Transform)>,
    beacons: Query<(Entity, &Beacon)>,
) {
    if !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for (mut lm, tf) in &mut q {
        if lm.discovered {
            continue;
        }
        let p = tf.translation;
        if Vec2::new(p.x, p.z).distance(hero.pos) > DISCOVER_R {
            continue;
        }
        lm.discovered = true;

        // One-time cache: a frontier-graded relic + pocket gold. The relic, the shrine buff and
        // the lore ARE the discovery reward — the purse is deliberately small (was 40+120f ≈ 160
        // at the rim; five landmarks + the completion bonus out-earned whole nights of defending,
        // so sightseeing trivialized the economy). Gold belongs to the town (tithe) + bounties.
        let factor = frontier_factor(p.x, p.z);
        let gold = (10.0 + factor * 30.0).round() as i64;
        player.0.add_gold(gold);
        let relic = frontier::roll_gear(factor, tile_hash(p.x, p.z));
        try_grant(&mut inv.0, &mut toasts.0, relic, 1, now as f64);

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
        cues.write(AudioCue::ChestOpen);
        cues.write(AudioCue::Gold);
        speak.write(crate::audio::Speak::new(crate::audio::Concept::ChestOpen));

        // Snuff the beacon — it's been found.
        for (e, b) in &beacons {
            if b.name == lm.name {
                commands.entity(e).try_despawn();
            }
        }

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

/// **F** at a found landmark prays at its shrine: a timed per-biome buff on a cooldown.
#[allow(clippy::too_many_arguments)]
fn shrine(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    hero: Res<HeroState>,
    mut buffs: ResMut<Buffs>,
    mut floats: ResMut<FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(&mut Landmark, &Transform)>,
) {
    if !keys.just_pressed(KeyCode::KeyF) || !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for (mut lm, tf) in &mut q {
        if !lm.discovered {
            continue;
        }
        let p = tf.translation;
        if Vec2::new(p.x, p.z).distance(hero.pos) > SHRINE_R {
            continue;
        }
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
            return;
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
        return; // one shrine per press
    }
}

// ── Frontier gradient (mirrors verbs.rs — landmarks reuse the same loot tiering) ──────

fn frontier_factor(x: f32, z: f32) -> f64 {
    const SAFE: f32 = 22.0;
    const RIM: f32 = 92.0;
    let d = (x * x + z * z).sqrt();
    let t = ((d - SAFE) / (RIM - SAFE)).clamp(0.0, 1.0) as f64;
    t * t * (3.0 - 2.0 * t)
}

fn tile_hash(x: f32, z: f32) -> f64 {
    let s = (x as f64 * 127.1 + z as f64 * 311.7).sin() * 43758.5453;
    s - s.floor()
}
