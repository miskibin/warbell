//! **Gnashfang Hold + the Blight** — the ork seat of power and the poisoned mire around it,
//! a WALKABLE southern extension of the world grid (the shape + heights are exported to
//! `worldmap::classify` via [`blight_class_base`]). The Blight reads as swamp to gameplay —
//! poison ticks + slow from `player::movement`, swamp ambience — with its own trampled-mud
//! ground, dead-wood scatter, fume vents and sickly pools.
//!
//! Spec: `docs/superpowers/specs/2026-06-11-ork-fortress-design.md`. The hold is a crude
//! timber stronghold — low spiked palisade (~1.5× hero height, so the camera reads the
//! interior over it; full-perimeter blockers keep the hero OUT), seven leaning watchtowers,
//! a shut gate, hide tents, longhouses, a forge, a boar pen, war drums, a hulking great hall
//! and a crooked spire crowned in iron with a green warp brazier — peopled by decorative
//! orks (no [`crate::orks::Ork`] brain: untargetable, never leave) and an oversized pacing
//! warlord. Outside the walls, three REAL ork patrols (full `orks.rs` combat brains, slow
//! respawn) prowl the mire, and the watchtowers fire real (blockable) warp bolts at a hero
//! who presses up close.
//!
//! Audio rides existing rails: the bonfire is tagged `camps::Flicker`, so the ambience
//! module hangs its spatial campfire loop + war-drum sink on it automatically; the war-horn
//! (`war-horn.ogg`) blares spatially from the gate on the hero's first close approach, and
//! every tower shot cracks a `warp-cast.ogg` release. During a night wave every fortress
//! fire flares hotter.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::{BiomeEntity, GroundDetail};
use crate::critters::PartKind;
use crate::firelight::{self, FireLight};
use crate::game_state::Modal;
use crate::orks::{Armory, Faction, OrkPart, OrkVariant};
use crate::palette::lin;
use crate::player::{HeroState, PendingHeroDamage};
use crate::projectile::{advance_bolt, BoltStep};
use crate::quality::GraphicsQuality;
use crate::worldmap::{self, GX, GZ, MAP_SCALE};

// ── Layout (world space; the OLD grid's south edge was z = +81 — the Blight extends it) ──

/// Fortress centre — the hold blob and the "inside the walls" tests key off this.
const CENTRE: Vec2 = Vec2::new(12.0, 103.0);
/// Hold blob radii (wobbled ellipse around [`CENTRE`]) — the fortress's own landmass lobe.
const BLOB_RX: f32 = 34.0;
const BLOB_RZ: f32 = 30.0;
/// The Blight apron — the big landmass lobe that merges the hold into the island's south
/// swamp coast (union with the hold blob = the whole walkable Blight). Centred east enough
/// that the south-stream's mouth (world x ≈ −24) stays open water — the stream delta dies
/// into the mire at the Blight's west rim.
const APRON: Vec2 = Vec2::new(24.0, 108.0);
const APRON_RX: f32 = 58.0;
const APRON_RZ: f32 = 50.0;
/// The gate wall line.
const FRONT_Z: f32 = 80.9;
/// Gate centre (the war-horn sounds from here; the threshold test measures to it).
const GATE: Vec2 = Vec2::new(12.0, FRONT_Z);

/// Hero within this of the gate → horn + the towers are in range to start punishing.
const THRESHOLD_R: f32 = 17.0;
/// Min seconds between horn blasts (re-approach re-horns; loitering doesn't spam).
const HORN_GAP: f32 = 45.0;

/// Watchtower fire: range is deliberately short — the hold only punishes a hero who comes
/// *very* close (the causeway + shore strip), not one wandering the south swamp.
const TOWER_RANGE: f32 = 13.5;
const TOWER_CD: f32 = 1.7;
/// Warp-bolt damage: core shaman parity (26), deliberately UN-nerfed (unlike the camp
/// shamans' −10%) — pressing your face against the ork capital is meant to sting.
const BOLT_DMG: f32 = 26.0;
const BOLT_SPEED: f32 = 10.5;
const BOLT_TTL: f32 = 3.5;
const BOLT_MAX_RANGE: f32 = 26.0;

// ── Public geometry queries (worldmap/camps/boats call these during generation) ─────

/// Signed "inside-ness" of the Blight landmass at world `(wx, wz)`, in rough world units
/// (>0 inside, <0 out): the max of two wobbled-ellipse fields — the big apron lobe that
/// merges into the island's south coast, and the hold's own blob.
pub fn blight_edge_world(wx: f32, wz: f32) -> f32 {
    let field = |c: Vec2, rx: f32, rz: f32, amp: f32| {
        let dx = (wx - c.x) / rx;
        let dz = (wz - c.y) / rz;
        let r = (dx * dx + dz * dz).sqrt();
        let ang = dz.atan2(dx);
        let wob = (ang * 3.0 + 1.2).sin() * amp + (ang * 5.0 - 0.4).sin() * amp * 0.7;
        (1.0 + wob - r) * rx.min(rz)
    };
    field(APRON, APRON_RX, APRON_RZ, 0.06).max(field(CENTRE, BLOB_RX, BLOB_RZ, 0.045))
}

/// Is world `(wx, wz)` on the Blight landmass? (Placement exclusions: camps, swamp scatter.)
pub fn in_blight_world(wx: f32, wz: f32) -> bool {
    blight_edge_world(wx, wz) > 0.0
}

/// BASE-space twin of [`blight_edge_world`] for `worldmap::ground_color`'s blend band
/// (returns base-tile units to match `BLEND`).
pub fn blight_edge_base(bx: f32, bz: f32) -> f32 {
    blight_edge_world(bx * MAP_SCALE - GX, bz * MAP_SCALE - GZ) / MAP_SCALE
}

/// BASE-space hook for `worldmap::classify`: height class of Blight land (`None` = not
/// Blight). 1 = the mire plain, sparse noise-rolled 2-class rises out in the open; inside
/// the hold, 2 = the great-hall terrace and 3 = the spire pad. The gate road and the wall
/// ring stay flat so the palisade never straddles a terrace lip.
pub fn blight_class_base(bx: f32, bz: f32) -> Option<i32> {
    let wx = bx * MAP_SCALE - GX;
    let wz = bz * MAP_SCALE - GZ;
    if blight_edge_world(wx, wz) <= 0.0 {
        return None;
    }
    let d_pad = (wx - 13.0).hypot(wz - 115.0);
    if d_pad < 2.8 {
        return Some(3);
    }
    let d_terrace = (wx - 12.5).hypot(wz - 108.0);
    if d_terrace < 10.5 {
        return Some(2);
    }
    if on_gate_approach(wx, wz) || Vec2::new(wx, wz).distance(CENTRE) < 26.0 {
        return Some(1);
    }
    let n = (wx * 0.21 + 1.7).sin() * (wz * 0.19 - 2.3).cos()
        + (wx * 0.083 + wz * 0.071 + 4.5).sin() * 0.5;
    Some(if n > 0.85 { 2 } else { 1 })
}

/// Scatter keep-out: the gate road (the old causeway line, now just the worn approach)
/// stays clear of props so the walk up to the wall — and the towers' line of fire — reads
/// clean.
pub fn on_gate_approach(wx: f32, wz: f32) -> bool {
    (3.0..=21.0).contains(&wx) && (65.0..=81.5).contains(&wz)
}

/// Water keep-out for the background sailboats: the whole Blight landmass + a wake margin.
/// `boats::boat_drift` bounces a hull that would drift in here.
pub fn boat_keepout(wx: f32, wz: f32) -> bool {
    blight_edge_world(wx, wz) > -10.0
}

/// Footing for fortress denizens — the Blight is real grid land now.
fn ground_y(wx: f32, wz: f32) -> Option<f32> {
    worldmap::ground_at_world(wx, wz)
}

/// Wander bound: keeps the population milling INSIDE the walls (and off the gate line).
fn inside_walls(wx: f32, wz: f32) -> bool {
    Vec2::new(wx, wz).distance(CENTRE) < 17.5 && wz > 84.0
}

// ── Plugin + components ─────────────────────────────────────────────────────────────

pub struct OrkFortressPlugin;

impl Plugin for OrkFortressPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_bolt_assets);
        // Visual breathing stays live through pauses/panels (like camps + firelight).
        app.add_systems(Update, (wobble_flames, drift_smoke, denizen_limbs, quality_lod));
        // Sim carries the freeze gate, per the game_state contract.
        app.add_systems(
            Update,
            (
                denizen_brain,
                tower_fire,
                step_warp_bolts,
                approach_watch,
                siege_flare,
                fortress_barks,
                patrol_respawn,
                blight_rescue,
            )
                .run_if(in_state(Modal::None)),
        );
    }
}

/// A decorative fortress ork (mesh hierarchy from `Armory::spawn_prop`; no combat).
#[derive(Component)]
struct Denizen {
    anchor: Vec2,
    target: Vec2,
    pos: Vec2,
    facing: f32,
    speed: f32,
    gait: f32,
    swing: f32,
    bob: f32,
    phase: f32,
    timer: f32,
    moving: bool,
    rng: u32,
    /// `Some` = a fixed two-point patrol (the warlord's hall ↔ bonfire pace).
    beat: Option<[Vec2; 2]>,
    beat_i: usize,
    /// Hidden on the Low graphics preset (half the population).
    lod_cull: bool,
}

/// One swinging leaf of the fortress gate. `sign` is which side (−1 left / +1 right); `open` is
/// the eased 0→1 swing amount the [`crate::cinematic`] gate system drives. The leaf is authored
/// hinged about its own origin, so a Y-rotation about that origin swings it outward (−Z).
#[derive(Component)]
pub struct FortressGate {
    pub sign: f32,
    pub open: f32,
}

/// A watchtower's fire emitter (the muzzle sits at the crow's-nest rail).
#[derive(Component)]
struct WarTower {
    muzzle: Vec3,
    ready_at: f32,
}

/// A live green warp bolt homing on the hero.
#[derive(Component)]
struct WarpBolt {
    traveled: f32,
    ttl: f32,
}

/// Shared warp-bolt mesh + sickly-green emissive material.
#[derive(Resource)]
struct WarpBoltAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

/// Scale-wobble for fortress flames that must NOT be `camps::Flicker` (the ambience module
/// hangs campfire/war-drum audio on every `Flicker` — only the bonfire should carry that).
#[derive(Component)]
struct Wobble {
    phase: f32,
}

/// Drifting smoke puff (the camps' smoke recipe, local copy — theirs is private).
#[derive(Component)]
struct FortSmoke {
    base: Vec3,
    phase: f32,
    speed: f32,
}

/// Tags a fortress fire's [`FireLight`] with its calm baseline so [`siege_flare`] can swell
/// every fire during a night wave and settle it back at dawn.
#[derive(Component)]
struct FortressFlame {
    base: f32,
}

fn setup_bolt_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.17).mesh().ico(2).unwrap());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 1.0, 0.55),
        emissive: LinearRgba::rgb(1.6, 4.8, 1.2),
        unlit: true,
        ..default()
    });
    commands.insert_resource(WarpBoltAssets { mesh, mat });
}

// ── Build (called from `worldmap::build`; everything tagged `BiomeEntity`) ──────────

/// Palisade ring corners, clockwise. The straight front run (`z = FRONT_Z`) carries the
/// gate gap at x 9..15.
const RING: [Vec2; 14] = [
    Vec2::new(2.0, FRONT_Z),
    Vec2::new(9.0, FRONT_Z), // gate gap 9..15
    Vec2::new(15.0, FRONT_Z),
    Vec2::new(22.0, FRONT_Z),
    Vec2::new(30.0, 88.0),
    Vec2::new(34.0, 100.0),
    Vec2::new(32.0, 112.0),
    Vec2::new(24.0, 121.0),
    Vec2::new(12.0, 124.5),
    Vec2::new(0.0, 123.0),
    Vec2::new(-8.5, 115.0),
    Vec2::new(-11.0, 103.0),
    Vec2::new(-8.0, 91.0),
    Vec2::new(-2.0, 84.0),
];

/// Tower bases: two flanking the gate, five on the ring (NE / E / SE / S / W / NW).
const TOWERS: [Vec2; 7] = [
    Vec2::new(5.3, 82.8),
    Vec2::new(18.7, 82.8),
    Vec2::new(31.5, 99.5),
    Vec2::new(23.0, 118.5),
    Vec2::new(11.5, 121.5),
    Vec2::new(-8.0, 103.0),
    Vec2::new(-5.5, 89.5),
];

/// Hide tents: (centre, scale). Clustered into a west camp-row and an east one.
const TENTS: [(Vec2, f32); 6] = [
    (Vec2::new(0.0, 90.5), 1.0),
    (Vec2::new(24.0, 92.0), 0.9),
    (Vec2::new(-4.5, 96.0), 1.15),
    (Vec2::new(21.5, 99.0), 1.0),
    (Vec2::new(3.0, 104.5), 0.9),
    (Vec2::new(26.5, 104.0), 1.1),
];
/// Timber longhouses (the warband's barracks): (centre, yaw).
const LONGHOUSES: [(Vec2, f32); 2] = [
    (Vec2::new(27.0, 109.5), -0.55),
    (Vec2::new(1.0, 115.5), 0.85),
];
/// The forge (on the hall terrace) and the boar pen (west yard).
const FORGE_AT: Vec2 = Vec2::new(18.5, 112.0);
const PEN_AT: Vec2 = Vec2::new(-2.5, 102.5);
/// War drums flanking the bonfire plaza.
const DRUMS: [Vec2; 2] = [Vec2::new(6.0, 92.0), Vec2::new(12.5, 92.5)];
/// Weapon racks + spoils piles (ground clutter with a story).
const RACKS: [Vec2; 3] = [Vec2::new(15.0, 88.5), Vec2::new(24.5, 96.0), Vec2::new(-1.0, 108.0)];
const PILES: [Vec2; 3] = [Vec2::new(8.0, 86.5), Vec2::new(16.5, 86.5), Vec2::new(22.0, 112.5)];
/// Free-standing war-banner poles on the plaza.
const PLAZA_BANNERS: [Vec2; 2] = [Vec2::new(5.0, 99.5), Vec2::new(19.5, 104.5)];

const HALL_AT: Vec2 = Vec2::new(12.0, 107.0);
const SPIRE_AT: Vec2 = Vec2::new(13.0, 115.0);
const BONFIRE_AT: Vec2 = Vec2::new(9.0, 95.0);
const CAGE_AT: Vec2 = Vec2::new(3.0, 97.0);
/// How much bigger the hall + spire read in the enlarged hold (spawn-transform scale).
const HALL_SCALE: f32 = 1.25;
const SPIRE_SCALE: f32 = 1.15;

/// The fortress war-banner: soot-black-red field, bone hoist band.
const BANNER_FIELD: u32 = 0x5a1410;
const BANNER_ACCENT: u32 = 0xcfc4a0;

pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
) {
    // (The ground itself is worldmap terrain now — `TB::Blight` tiles via
    // `blight_class_base`; this fn only dresses it.)

    // Three shared vertex-colour prop materials — same batching contract as the camps, but
    // each with its own neutral detail texture multiplied over the vertex colours (the
    // primitives' own UVs sample it), so timber, hide and bone-clutter read as slightly
    // DIFFERENT materials. Kept SUBTLE (`strength` ~0.3): the other biomes' props are clean
    // flat vertex-colour, so a heavy grime made the hold read over-textured next to them —
    // this is just a faint tooth, not a wallpaper. Three materials = three batches, cheap.
    let make_prop_mat = |d: &GroundDetail,
                         rough: f32,
                         images: &mut Assets<Image>,
                         std_mats: &mut Assets<StandardMaterial>| {
        let (img, _) = crate::terrain::detail_image(d);
        let tex = images.add(img);
        std_mats.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(tex),
            perceptual_roughness: rough,
            ..default()
        })
    };
    // Neutral grime — bones, spikes, stumps, piles, misc clutter.
    let mat = make_prop_mat(
        &GroundDetail {
            scale: 1.0,
            strength: 0.30,
            variation: 0.5,
            seed: 13.0,
            dark: 0x9c968c,
            base: 0xc2bcb0,
            light: 0xe6e0d4,
            grain: 0.6,
            streak: 0.5,
        },
        0.95,
        images,
        std_mats,
    );
    // Streaky long-grain wood — palisade, gate, towers, hall, spire, longhouses, racks.
    let timber_mat = make_prop_mat(
        &GroundDetail {
            scale: 1.5,
            strength: 0.32,
            variation: 0.45,
            seed: 21.0,
            dark: 0x9a9084,
            base: 0xc6beb0,
            light: 0xe8e0d2,
            grain: 0.4,
            streak: 0.85,
        },
        0.92,
        images,
        std_mats,
    );
    // Blotchy coarse leather — tents, drums, hide roofs.
    let hide_mat = make_prop_mat(
        &GroundDetail {
            scale: 2.1,
            strength: 0.32,
            variation: 0.7,
            seed: 33.0,
            dark: 0xa89c8c,
            base: 0xc8bcaa,
            light: 0xe2d6c4,
            grain: 0.7,
            streak: 0.2,
        },
        0.97,
        images,
        std_mats,
    );
    // The orks themselves stay clean vertex-colour (grime on a face-sized limb is noise).
    let ork_mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    // Orange campfire flame + green warp flame (emissive, bloom-lit) + translucent smoke.
    let flame_mat = std_mats.add(StandardMaterial {
        base_color: crate::palette::srgb(0xff8a30),
        emissive: crate::palette::srgb(0xff8a30).to_linear() * 4.0,
        ..default()
    });
    let warp_mat = std_mats.add(StandardMaterial {
        base_color: crate::palette::srgb(0x86e860),
        emissive: crate::palette::srgb(0x6fe06a).to_linear() * 4.5,
        ..default()
    });
    let glow_mat = std_mats.add(StandardMaterial {
        base_color: crate::palette::srgb(0xffb050),
        emissive: crate::palette::srgb(0xff9838).to_linear() * 3.0,
        unlit: true,
        ..default()
    });
    let smoke_mat = std_mats.add(StandardMaterial {
        base_color: Color::srgba(0.5, 0.5, 0.52, 0.4),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let smoke_puff = meshes.add(Sphere::new(0.5).mesh().ico(1).unwrap());

    let mut rng = 0x6f7c_5eedu32;
    let at = |p: Vec2| Vec3::new(p.x, ground_y(p.x, p.y).unwrap_or(0.0), p.y);

    // ── Palisade ring (one merged mesh per segment run; gate gap left open) ──
    // The Blight is walkable land all the way round now, so EVERY segment registers a real
    // OBB blocker — the timber ring is the only thing keeping the hero (and the patrols)
    // out of the hold. `add_obb` yaw matches `Quat::from_rotation_y`, whose +X maps to
    // (cos, −sin) in XZ → yaw = atan2(−dz, dx).
    for i in 0..RING.len() {
        let a = RING[i];
        let b = RING[(i + 1) % RING.len()];
        if a == Vec2::new(9.0, FRONT_Z) && b == Vec2::new(15.0, FRONT_Z) {
            continue; // the gate fills this gap
        }
        spawn_solid(commands, meshes, &timber_mat, palisade_segment(a, b, &mut rng), Vec3::ZERO, Quat::IDENTITY);
        let mid = (a + b) / 2.0;
        let d = b - a;
        crate::blockers::add_obb(mid.x, mid.y, d.length() / 2.0 + 0.25, 0.45, (-d.y).atan2(d.x));
    }
    crate::blockers::add_obb(12.0, FRONT_Z, 3.2, 0.5, 0.0); // the shut gate itself

    // ── The gate ── static frame (posts/lintel/skulls) + two hinged door leaves. The leaves are
    // their own entities tagged `FortressGate` so the Director (and, later, the live game) can
    // swing them open; the frame is fixed scenery.
    spawn_solid(commands, meshes, &timber_mat, gate_frame_mesh(), at(GATE), Quat::IDENTITY);
    for sign in [-1.0f32, 1.0] {
        // Hinge post sits ~3u out from the gate centre; the leaf reaches inward to the gap middle.
        let hinge = Vec2::new(GATE.x + sign * 3.0, GATE.y);
        commands.spawn((
            Mesh3d(meshes.add(gate_door_mesh(sign))),
            MeshMaterial3d(timber_mat.clone()),
            Transform::from_translation(at(hinge)),
            FortressGate { sign, open: 0.0 },
            BiomeEntity,
        ));
    }

    // ── Watchtowers (leaning, each its own tilt/yaw) + fire emitters + banners ──
    for (i, t) in TOWERS.iter().enumerate() {
        let yaw = rng_range(&mut rng, -0.3, 0.3);
        let lean = rng_range(&mut rng, 0.02, 0.05)
            * if next_u32(&mut rng) % 2 == 0 { 1.0 } else { -1.0 };
        let pos = at(*t);
        let rot = ry(yaw) * Quat::from_rotation_z(lean);
        spawn_solid(commands, meshes, &timber_mat, tower_mesh(&mut rng), pos, rot);
        crate::blockers::add(t.x, t.y, 1.2);
        commands.spawn((
            WarTower {
                muzzle: pos + rot * Vec3::new(0.0, 5.1, 0.0),
                ready_at: i as f32 * 0.37,
            },
            Transform::from_translation(pos),
            BiomeEntity,
        ));
        if i < 2 {
            // Gate towers fly the hold's war-banner (attach follows the tower's tilt so the
            // cloth hangs off the real pole).
            let flag = crate::banner::spawn_flag(
                commands,
                meshes,
                std_mats,
                pos + rot * Vec3::new(0.0, 6.6, 0.0),
                0.9,
                0.55,
                BANNER_FIELD,
                Some(BANNER_ACCENT),
            );
            commands.entity(flag).insert(BiomeEntity);
        }
    }

    // ── Great hall (on the terrace, scaled up for the enlarged hold) ──
    let hall_pos = at(HALL_AT);
    commands.spawn((
        Mesh3d(meshes.add(hall_mesh(&mut rng))),
        MeshMaterial3d(timber_mat.clone()),
        Transform::from_translation(hall_pos).with_scale(Vec3::splat(HALL_SCALE)),
        BiomeEntity,
    ));
    crate::blockers::add_obb(HALL_AT.x, HALL_AT.y, 5.8 * HALL_SCALE, 4.3 * HALL_SCALE, 0.0);
    // Doorway glow + flanking torches (warm light pooling out of the dark hall mouth).
    commands.spawn((
        Mesh3d(meshes.add(bx(2.0 * HALL_SCALE, 2.4 * HALL_SCALE, 0.05, Vec3::ZERO, lin(0xffffff)))),
        MeshMaterial3d(glow_mat.clone()),
        Transform::from_translation(hall_pos + Vec3::new(0.0, 1.45, -3.96) * HALL_SCALE),
        bevy::light::NotShadowCaster,
        BiomeEntity,
    ));
    for sx in [-1.0f32, 1.0] {
        let tp = hall_pos + Vec3::new(sx * 1.55, 2.1, -4.15) * HALL_SCALE;
        let phase = rng_range(&mut rng, 0.0, 6.0);
        let (light, fl) = firelight::torch_light(phase);
        let base = fl.base;
        commands.spawn((
            Mesh3d(meshes.add(flame_mesh(0.55))),
            MeshMaterial3d(flame_mat.clone()),
            Transform::from_translation(tp),
            Wobble { phase },
            light,
            fl,
            FortressFlame { base },
            BiomeEntity,
        ));
        // The torch's bracket pole.
        spawn_solid(
            commands,
            meshes,
            &mat,
            cyl(0.05, 0.9, v(0.0, -0.45, 0.0), Quat::IDENTITY, lin(0x3a2a1a)),
            tp,
            Quat::IDENTITY,
        );
    }
    let hall_flag = crate::banner::spawn_flag(
        commands,
        meshes,
        std_mats,
        hall_pos + Vec3::new(0.0, 7.3, 3.2) * HALL_SCALE,
        1.1,
        0.7,
        BANNER_FIELD,
        Some(BANNER_ACCENT),
    );
    commands.entity(hall_flag).insert(BiomeEntity);

    // ── Crooked spire (on the pad) + iron crown + green warp brazier ──
    let spire_pos = at(SPIRE_AT);
    let spire_rot = ry(0.2);
    commands.spawn((
        Mesh3d(meshes.add(spire_mesh(&mut rng))),
        MeshMaterial3d(timber_mat.clone()),
        Transform { translation: spire_pos, rotation: spire_rot, scale: Vec3::splat(SPIRE_SCALE) },
        BiomeEntity,
    ));
    crate::blockers::add(SPIRE_AT.x, SPIRE_AT.y, 1.8 * SPIRE_SCALE);
    let brazier = spire_pos + spire_rot * (Vec3::new(0.45, 12.05, -0.3) * SPIRE_SCALE);
    commands.spawn((
        Mesh3d(meshes.add(flame_mesh(1.5))),
        MeshMaterial3d(warp_mat.clone()),
        Transform::from_translation(brazier),
        Wobble { phase: 2.4 },
        PointLight {
            color: Color::srgb(0.55, 1.0, 0.5),
            intensity: 42_000.0,
            range: 18.0,
            radius: 0.25,
            shadows_enabled: false,
            ..default()
        },
        FireLight { phase: 2.4, base: 42_000.0 },
        FortressFlame { base: 42_000.0 },
        BiomeEntity,
    ));
    let spire_flag = crate::banner::spawn_flag(
        commands,
        meshes,
        std_mats,
        spire_pos + spire_rot * (Vec3::new(-0.8, 10.4, 0.0) * SPIRE_SCALE),
        1.0,
        0.6,
        BANNER_FIELD,
        Some(BANNER_ACCENT),
    );
    commands.entity(spire_flag).insert(BiomeEntity);

    // ── Bonfire plaza: the hold's great fire — `camps::Flicker` so the ambience module
    //    attaches its spatial campfire loop + war-drum sink here (the drums that carry
    //    across the strait at dusk ARE the fortress's voice). ──
    let fire = at(BONFIRE_AT);
    spawn_solid(commands, meshes, &mat, bonfire_base_mesh(), fire, Quat::IDENTITY);
    crate::blockers::add(BONFIRE_AT.x, BONFIRE_AT.y, 1.1);
    commands.spawn((
        Mesh3d(meshes.add(flame_mesh(2.3))),
        MeshMaterial3d(flame_mat.clone()),
        Transform::from_translation(fire + Vec3::Y * 0.35),
        crate::camps::Flicker { phase: 0.7 },
        PointLight {
            color: firelight::FIRE_COLOR,
            intensity: 95_000.0,
            range: 24.0,
            radius: 0.45,
            shadows_enabled: false,
            ..default()
        },
        FireLight { phase: 0.7, base: 95_000.0 },
        FortressFlame { base: 95_000.0 },
        BiomeEntity,
    ));
    for i in 0..4 {
        commands.spawn((
            Mesh3d(smoke_puff.clone()),
            MeshMaterial3d(smoke_mat.clone()),
            Transform::from_translation(fire).with_scale(Vec3::splat(0.01)),
            FortSmoke { base: fire + Vec3::Y * 0.8, phase: i as f32 / 4.0, speed: 0.28 },
            BiomeEntity,
        ));
    }
    // Hall smoke-hole breath.
    for i in 0..2 {
        let hp = hall_pos + Vec3::new(0.6, 6.1, 1.0) * HALL_SCALE;
        commands.spawn((
            Mesh3d(smoke_puff.clone()),
            MeshMaterial3d(smoke_mat.clone()),
            Transform::from_translation(hp).with_scale(Vec3::splat(0.01)),
            FortSmoke { base: hp, phase: i as f32 / 2.0, speed: 0.22 },
            BiomeEntity,
        ));
    }

    // ── Hide tents (the warband's sprawl — each its own size, yaw and patchwork) ──
    for (p, s) in TENTS {
        let yaw = rng_range(&mut rng, 0.0, TAU);
        spawn_solid(commands, meshes, &hide_mat, tent_mesh(s, &mut rng), at(p), ry(yaw));
        crate::blockers::add_obb(p.x, p.y, 2.0 * s, 1.7 * s, yaw);
    }

    // ── Longhouses (timber barracks with hide roofs) ──
    for (p, yaw) in LONGHOUSES {
        spawn_solid(commands, meshes, &timber_mat, longhouse_mesh(&mut rng), at(p), ry(yaw));
        crate::blockers::add_obb(p.x, p.y, 2.1, 3.1, yaw);
    }

    // ── The forge (hall terrace): hearth + anvil + quench barrel, ember + smoke alive ──
    let forge_pos = at(FORGE_AT);
    spawn_solid(commands, meshes, &timber_mat, forge_mesh(&mut rng), forge_pos, ry(-0.9));
    crate::blockers::add_obb(FORGE_AT.x, FORGE_AT.y, 1.9, 1.5, -0.9);
    let ember = forge_pos + Vec3::new(0.0, 0.75, -0.35);
    commands.spawn((
        Mesh3d(meshes.add(flame_mesh(0.5))),
        MeshMaterial3d(glow_mat.clone()),
        Transform::from_translation(ember),
        Wobble { phase: 4.1 },
        PointLight {
            color: Color::srgb(1.0, 0.62, 0.25),
            intensity: 18_000.0,
            range: 9.0,
            radius: 0.15,
            shadows_enabled: false,
            ..default()
        },
        FireLight { phase: 4.1, base: 18_000.0 },
        FortressFlame { base: 18_000.0 },
        BiomeEntity,
    ));
    commands.spawn((
        Mesh3d(smoke_puff.clone()),
        MeshMaterial3d(smoke_mat.clone()),
        Transform::from_translation(ember).with_scale(Vec3::splat(0.01)),
        FortSmoke { base: ember + Vec3::Y * 0.5, phase: 0.4, speed: 0.3 },
        BiomeEntity,
    ));

    // ── The boar pen (west yard): post-and-rail fence, churned mud, a feed trough ──
    spawn_solid(commands, meshes, &timber_mat, pen_mesh(&mut rng), at(PEN_AT), ry(0.25));
    crate::blockers::add_obb(PEN_AT.x, PEN_AT.y, 2.5, 2.5, 0.25);

    // ── War drums + the spit roast on the bonfire plaza ──
    for (i, p) in DRUMS.iter().enumerate() {
        spawn_solid(commands, meshes, &hide_mat, drum_mesh(&mut rng), at(*p), ry(i as f32 * 1.7));
        crate::blockers::add(p.x, p.y, 0.7);
    }
    spawn_solid(commands, meshes, &mat, spit_mesh(&mut rng), at(Vec2::new(11.5, 97.0)), ry(0.5));

    // ── Weapon racks + spoils piles (plunder stacked where it was dropped) ──
    for p in RACKS {
        spawn_solid(commands, meshes, &timber_mat, rack_mesh(&mut rng), at(p), ry(rng_range(&mut rng, 0.0, TAU)));
        crate::blockers::add(p.x, p.y, 0.6);
    }
    for p in PILES {
        spawn_solid(commands, meshes, &mat, pile_mesh(&mut rng), at(p), ry(rng_range(&mut rng, 0.0, TAU)));
        crate::blockers::add(p.x, p.y, 0.8);
    }

    // ── Free-standing plaza banners (pole mesh + cloth entity) ──
    for p in PLAZA_BANNERS {
        let base = at(p);
        spawn_solid(
            commands,
            meshes,
            &timber_mat,
            group(vec![
                cyl(0.07, 4.4, v(0.0, 2.2, 0.0), Quat::IDENTITY, lin(TIMBER_DARK)),
                bx(0.2, 0.18, 0.18, v(0.0, 4.5, 0.0), lin(BONE)),
            ]),
            base,
            Quat::IDENTITY,
        );
        crate::blockers::add(p.x, p.y, 0.3);
        let flag = crate::banner::spawn_flag(
            commands,
            meshes,
            std_mats,
            base + Vec3::Y * 4.1,
            0.85,
            0.5,
            BANNER_FIELD,
            Some(BANNER_ACCENT),
        );
        commands.entity(flag).insert(BiomeEntity);
    }

    // ── Prisoner cage (bigger than a camp's; the hold hoards captives) ──
    spawn_solid(commands, meshes, &timber_mat, cage_mesh(), at(CAGE_AT), ry(0.4));
    crate::blockers::add_obb(CAGE_AT.x, CAGE_AT.y, 1.25, 1.25, 0.4);

    // ── War totems: one OUTSIDE on the causeway and two inside, all glaring at the
    //    castle (the camps' "gaze points home" rule, scaled up). ──
    for tp in [Vec2::new(8.5, 77.0), Vec2::new(24.0, 90.0), Vec2::new(0.0, 118.0)] {
        let yaw = (-tp.x).atan2(-tp.y);
        spawn_solid(commands, meshes, &timber_mat, totem_mesh(&mut rng), at(tp), ry(yaw));
        crate::blockers::add(tp.x, tp.y, 0.45);
    }

    // ── Skull-spike warnings flanking the causeway (on-grid, decorative) ──
    for sp in [
        Vec2::new(6.5, 78.8),
        Vec2::new(17.5, 78.6),
        Vec2::new(9.5, 75.5),
        Vec2::new(15.0, 75.8),
    ] {
        spawn_solid(commands, meshes, &mat, spikes_mesh(&mut rng), at(sp), ry(rng_range(&mut rng, 0.0, TAU)));
    }

    // ── Trampled-ground dressing: bones, stumps, mud pools inside the walls ──
    let keep_out: Vec<(Vec2, f32)> = TOWERS
        .iter()
        .map(|t| (*t, 2.0))
        .chain(TENTS.iter().map(|(p, s)| (*p, 2.4 * s)))
        .chain(LONGHOUSES.iter().map(|(p, _)| (*p, 4.2)))
        .chain(RACKS.iter().map(|p| (*p, 1.4)))
        .chain(PILES.iter().map(|p| (*p, 1.6)))
        .chain(DRUMS.iter().map(|p| (*p, 1.3)))
        .chain([
            (HALL_AT, 7.0 * HALL_SCALE),
            (SPIRE_AT, 3.6),
            (BONFIRE_AT, 2.4),
            (CAGE_AT, 2.2),
            (FORGE_AT, 2.6),
            (PEN_AT, 3.4),
        ])
        .collect();
    let mut placed = 0;
    let mut tries = 0;
    while placed < 40 && tries < 480 {
        tries += 1;
        let ang = rng_range(&mut rng, 0.0, TAU);
        let r = rng_range(&mut rng, 4.0, 20.0);
        let p = CENTRE + Vec2::new(ang.cos() * r, ang.sin() * r * 0.9);
        if ground_y(p.x, p.y).is_none()
            || p.y < 84.0
            || keep_out.iter().any(|(c, kr)| c.distance(p) < *kr)
        {
            continue;
        }
        let m = match next_u32(&mut rng) % 4 {
            0 => bone_pile_mesh(&mut rng),
            1 => stump_mesh(&mut rng),
            2 => mud_pool_mesh(&mut rng),
            _ => spikes_mesh(&mut rng),
        };
        spawn_solid(commands, meshes, &mat, m, at(p), ry(rng_range(&mut rng, 0.0, TAU)));
        placed += 1;
    }

    // ── The Blight scatter: the whole landmass OUTSIDE the walls — dead trees in three
    //    silhouettes (bare TreeKind::Dead, gnarled claw-trees, snapped snags), bones,
    //    spikes, stumps, mud, sickly warp pools, fuming vents, and the grislier dressing
    //    (gibbets, impaled remains, giant ribcages, scrap heaps, effigies, shroom
    //    clusters, tar pits) — dense at the heart, thinning toward the swamp blend band
    //    (where the swamp's own scatter takes over). Deterministic per tile; every common
    //    shape pre-bakes a few variants so the stand batches. ──
    let tree_meshes: Vec<Handle<Mesh>> = [
        [0.85f32, 0.82, 0.75],
        [0.66, 0.62, 0.55],
        [1.0, 0.95, 0.85],
    ]
    .iter()
    .map(|t| {
        meshes.add(crate::trees::tint_mesh(
            crate::trees::build_tree_mesh(crate::trees::TreeKind::Dead),
            *t,
        ))
    })
    .collect();
    let claw_meshes: Vec<Handle<Mesh>> = (0..3u32)
        .map(|i| {
            let mut s = 0x91ac_0001u32.wrapping_add(i * 977) | 1;
            meshes.add(claw_tree_mesh(&mut s))
        })
        .collect();
    let snag_meshes: Vec<Handle<Mesh>> = (0..2u32)
        .map(|i| {
            let mut s = 0x44d7_136bu32.wrapping_add(i * 1409) | 1;
            meshes.add(snag_mesh(&mut s))
        })
        .collect();
    let shroom_meshes: Vec<Handle<Mesh>> = (0..3u32)
        .map(|i| {
            let mut s = 0x7e55_92c1u32.wrapping_add(i * 661) | 1;
            meshes.add(shroom_mesh(&mut s))
        })
        .collect();
    // Sickly warp pools glow faintly; vents breathe green smoke (both share the warp hue).
    // Kept DARK — a first pass at 0x4d6630 + 0.6 emissive read as bright lime lily-pads.
    let pool_mat = std_mats.add(StandardMaterial {
        base_color: crate::palette::srgb(0x2e3d1a),
        emissive: crate::palette::srgb(0x3d5c20).to_linear() * 0.22,
        perceptual_roughness: 0.25,
        ..default()
    });
    let pool_mesh = meshes.add({
        let mut m = Cylinder::new(1.0, 0.05).mesh().resolution(10).build();
        let n = m.count_vertices();
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 1.0, 1.0, 1.0]; n]);
        m
    });
    let fume_mat = std_mats.add(StandardMaterial {
        base_color: Color::srgba(0.45, 0.58, 0.38, 0.34),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let mut fumes = 0;
    for tz in 56..166 {
        for tx in -36..84 {
            // Per-tile hash-seeded RNG, independent of visit order (the snow-drift idiom).
            let mut s = (tx as i64 * 73_856_093 ^ tz as i64 * 19_349_663) as u32 ^ 0x0b1a_5eed;
            let p = Vec2::new(
                tx as f32 + 0.5 + rng_range(&mut s, -0.35, 0.35),
                tz as f32 + 0.5 + rng_range(&mut s, -0.35, 0.35),
            );
            let depth = blight_edge_world(p.x, p.y);
            if depth <= 0.5
                || ground_y(p.x, p.y).is_none()
                || p.distance(CENTRE) < 25.0
                || on_gate_approach(p.x, p.y)
                || crate::blockers::is_blocked(p.x, p.y)
            {
                continue;
            }
            // Density ramps with depth into the Blight so the swamp edge frays naturally.
            let f = (depth / 8.0).clamp(0.0, 1.0);
            let roll = rng01(&mut s);
            // Cumulative roll table — trees scale with depth, clutter mostly flat. Tree
            // density kept SPARSE (was ~16% of heart tiles → a wall of trunks); the orks
            // ate the forest, the mire should read open and littered, not wooded.
            let trees = 0.075 * f;
            if roll < 0.04 * f {
                spawn_stand(commands, &tree_meshes, &mat, at(p), 0.9, 2.1, &mut s);
                crate::blockers::add(p.x, p.y, 0.3);
            } else if roll < 0.06 * f {
                spawn_stand(commands, &claw_meshes, &mat, at(p), 0.85, 1.7, &mut s);
                crate::blockers::add(p.x, p.y, 0.3);
            } else if roll < trees {
                spawn_stand(commands, &snag_meshes, &mat, at(p), 0.9, 1.5, &mut s);
                crate::blockers::add(p.x, p.y, 0.3);
            } else if roll < trees + 0.06 {
                let m = match next_u32(&mut s) % 4 {
                    0 => bone_pile_mesh(&mut s),
                    1 => spikes_mesh(&mut s),
                    2 => stump_mesh(&mut s),
                    _ => mud_pool_mesh(&mut s),
                };
                spawn_solid(commands, meshes, &mat, m, at(p), ry(rng_range(&mut s, 0.0, TAU)));
            } else if roll < trees + 0.08 {
                spawn_stand(commands, &shroom_meshes, &mat, at(p), 0.8, 1.5, &mut s);
            } else if roll < trees + 0.088 {
                commands.spawn((
                    Mesh3d(pool_mesh.clone()),
                    MeshMaterial3d(pool_mat.clone()),
                    Transform {
                        translation: at(p) + Vec3::Y * 0.02,
                        rotation: Quat::IDENTITY,
                        scale: Vec3::new(rng_range(&mut s, 0.7, 1.6), 1.0, rng_range(&mut s, 0.7, 1.6)),
                    },
                    BiomeEntity,
                ));
            } else if roll < trees + 0.094 {
                spawn_solid(commands, meshes, &mat, tar_pit_mesh(&mut s), at(p), ry(rng01(&mut s) * TAU));
            } else if roll < trees + 0.104 {
                spawn_solid(commands, meshes, &mat, scrap_mesh(&mut s), at(p), ry(rng01(&mut s) * TAU));
            } else if roll < trees + 0.109 {
                spawn_solid(commands, meshes, &mat, impale_mesh(&mut s), at(p), ry(rng01(&mut s) * TAU));
                crate::blockers::add(p.x, p.y, 0.25);
            } else if roll < trees + 0.1125 {
                spawn_solid(commands, meshes, &mat, ribcage_mesh(&mut s), at(p), ry(rng01(&mut s) * TAU));
                crate::blockers::add(p.x, p.y, 0.45);
            } else if roll < trees + 0.1155 {
                spawn_solid(commands, meshes, &timber_mat, gibbet_mesh(&mut s), at(p), ry(rng01(&mut s) * TAU));
                crate::blockers::add(p.x, p.y, 0.25);
            } else if roll < trees + 0.1185 {
                spawn_solid(commands, meshes, &hide_mat, effigy_mesh(&mut s), at(p), ry(rng01(&mut s) * TAU));
                crate::blockers::add(p.x, p.y, 0.25);
            } else if roll < trees + 0.1245 && fumes < 20 {
                fumes += 1;
                let base = at(p);
                spawn_solid(commands, meshes, &mat, vent_mesh(&mut s), base, ry(rng01(&mut s)));
                for k in 0..2 {
                    commands.spawn((
                        Mesh3d(smoke_puff.clone()),
                        MeshMaterial3d(fume_mat.clone()),
                        Transform::from_translation(base).with_scale(Vec3::splat(0.01)),
                        FortSmoke {
                            base: base + Vec3::Y * 0.25,
                            phase: k as f32 / 2.0 + rng01(&mut s),
                            speed: 0.17,
                        },
                        BiomeEntity,
                    ));
                }
            }
        }
    }

    // ── Wayposts flanking the gate road — the orks signpost their own front door ──
    for (i, wp) in [
        Vec2::new(4.5, 66.5),
        Vec2::new(19.5, 68.0),
        Vec2::new(4.0, 72.5),
        Vec2::new(20.0, 74.5),
    ]
    .into_iter()
    .enumerate()
    {
        spawn_solid(commands, meshes, &timber_mat, waypost_mesh(&mut rng), at(wp), ry(i as f32 * 1.4));
        crate::blockers::add(wp.x, wp.y, 0.3);
    }

    // ── Population: a milling warband + the pacing warlord (decorative; untargetable) ──
    let armory = Armory::new(meshes, std_mats, ork_mat.clone());
    let spawn_denizen = |commands: &mut Commands,
                         armory: &Armory,
                         variant: OrkVariant,
                         p: Vec2,
                         scale: f32,
                         beat: Option<[Vec2; 2]>,
                         lod_cull: bool,
                         rng: &mut u32| {
        let facing = rng_range(rng, 0.0, TAU);
        let pos3 = Vec3::new(p.x, ground_y(p.x, p.y).unwrap_or(0.0), p.y);
        let e = armory.spawn_prop(commands, variant, Faction::Red, pos3, facing, scale);
        commands.entity(e).insert((
            Denizen {
                anchor: p,
                target: p,
                pos: p,
                facing,
                speed: if beat.is_some() { 0.9 } else { 1.2 },
                gait: if beat.is_some() { 5.0 } else { 6.5 },
                swing: 0.32,
                bob: 0.05,
                phase: rng_range(rng, 0.0, TAU),
                timer: rng_range(rng, 0.5, 4.0),
                moving: false,
                rng: next_u32(rng) | 1,
                beat,
                beat_i: 0,
                lod_cull,
            },
            BiomeEntity,
        ));
    };
    use OrkVariant::*;
    let roster: [(OrkVariant, Vec2); 14] = [
        (Grunt, Vec2::new(5.0, 92.0)),
        (Grunt, Vec2::new(16.0, 91.0)),
        (Grunt, Vec2::new(21.0, 97.5)),
        (Grunt, Vec2::new(3.5, 100.0)),
        (Grunt, Vec2::new(14.0, 118.0)),
        (Scout, Vec2::new(24.0, 104.0)),
        (Scout, Vec2::new(-1.0, 107.5)),
        (Scout, Vec2::new(7.0, 120.0)),
        (Berserker, Vec2::new(19.0, 88.0)),
        (Berserker, Vec2::new(25.0, 111.0)),
        (Berserker, Vec2::new(2.0, 94.0)),
        (Shaman, Vec2::new(4.0, 112.0)),
        (Shaman, Vec2::new(10.0, 113.5)),
        (Shaman, Vec2::new(20.0, 108.0)),
    ];
    for (i, (variant, p)) in roster.into_iter().enumerate() {
        spawn_denizen(commands, &armory, variant, p, 1.0, None, i % 2 == 1, &mut rng);
    }
    // The warlord: an oversized berserker pacing his beat between hall door and bonfire.
    spawn_denizen(
        commands,
        &armory,
        Berserker,
        Vec2::new(11.0, 99.5),
        1.55,
        Some([Vec2::new(12.0, 100.7), Vec2::new(9.5, 96.6)]),
        false,
        &mut rng,
    );

    // ── The Blight patrols: three REAL warband squads (full `orks.rs` combat brains —
    //    Idle/Patrol/Hunt/Attack, home-leashed) prowling the mire OUTSIDE the walls. The
    //    armory is kept alive in [`BlightPatrols`] so [`patrol_respawn`] can repopulate a
    //    wiped squad after a delay, out of the hero's sight. ──
    let mut sites = PATROL_SITES.map(|(home, squad)| PatrolSite {
        home,
        squad,
        cleared_at: None,
        seed: next_u32(&mut rng) | 1,
    });
    for site in &mut sites {
        spawn_squad(commands, &armory, site);
    }
    commands.insert_resource(BlightPatrols { armory, sites });

    // ── Caged captives: closed cages staked by two patrols (the raid objective). Each blocks
    //    like a solid prop — you walk UP to it; `blight_rescue` opens it when its squad falls. ──
    for (slot, (at_xz, _patrol)) in BLIGHT_CAGES.iter().enumerate() {
        let yaw = rng_range(&mut rng, 0.0, TAU);
        crate::blockers::add_obb(at_xz.x, at_xz.y, 1.1, 1.0, yaw);
        commands.spawn((
            Mesh3d(meshes.add(cage_mesh())),
            MeshMaterial3d(timber_mat.clone()),
            Transform { translation: at(*at_xz), rotation: ry(yaw), scale: Vec3::ONE },
            BiomeEntity,
            BlightCage { slot },
        ));
    }
    commands.insert_resource(BlightCaptives::default());

    // ── The war-hoard: one authored plunder chest deep in the south-east mire. ──
    let (hx, gold, loot) = BLIGHT_HOARD;
    crate::verbs::spawn_trophy_chest(
        commands,
        meshes,
        std_mats,
        at(hx),
        rng_range(&mut rng, 0.0, TAU),
        gold,
        loot,
    );

    info!("ork fortress: Gnashfang Hold built at {:.0},{:.0}", CENTRE.x, CENTRE.y);
}

/// Patrol homes + squad make-up (all `Faction::Red`, like the hold). West + east flanks,
/// an ambush squad beside the gate road, and two deep-mire squads in the southern sprawl.
const PATROL_SITES: [(Vec2, [OrkVariant; 2]); 5] = [
    (Vec2::new(-12.0, 88.0), [OrkVariant::Grunt, OrkVariant::Scout]),
    (Vec2::new(38.0, 100.0), [OrkVariant::Grunt, OrkVariant::Berserker]),
    (Vec2::new(26.0, 76.0), [OrkVariant::Scout, OrkVariant::Grunt]),
    (Vec2::new(-14.0, 122.0), [OrkVariant::Berserker, OrkVariant::Grunt]),
    (Vec2::new(54.0, 118.0), [OrkVariant::Grunt, OrkVariant::Grunt]),
];

/// Seconds after a patrol is wiped before it can return, and how far the hero must be for
/// the return to happen unseen (the camps' respawn manners, slower).
const PATROL_RESPAWN_DELAY: f32 = 120.0;
const PATROL_RESPAWN_FAR: f32 = 45.0;

// ── Caged captives: the orks' plundered townsfolk, the reason to raid the mire ──────────
//
// A couple of barred cages are staked in the open Blight, each guarded by one patrol squad.
// Wipe the guards (one-time — patrols respawn, captives don't) and the cage swings open +
// one townsperson joins the castle (grows the bloodline). Mirrors `villagers::camp_rescue`,
// keyed off the Blight patrols instead of the wilderness camps.

/// A closed captive cage in the mire — `patrol` indexes [`PATROL_SITES`] (the squad guarding
/// it; cleared = freed) and `i` is its slot in [`BlightCaptives`].
#[derive(Component)]
struct BlightCage {
    slot: usize,
}

/// `(cage world XZ, guarding PATROL_SITES index)`. Both cages sit a few units OUTSIDE the
/// walls in walkable mire, hard by their patrol's home so the squad reads as their jailers.
const BLIGHT_CAGES: [(Vec2, usize); 2] = [
    (Vec2::new(42.0, 99.0), 1),    // east flank — guarded by patrol 1 (38,100)
    (Vec2::new(-17.0, 125.0), 3),  // deep south-west mire — guarded by patrol 3 (-14,122)
];

/// The war-hoard: a single one-shot plunder chest deep in the south-east mire, guarded by
/// patrol 4. `(chest world XZ, gold, loot)` — fixed haul (consumables + a purse), no gear, so
/// it rewards the trek without power-creeping the already-strong hero.
const BLIGHT_HOARD: (Vec2, i64, &[&str]) = (Vec2::new(57.0, 119.0), 80, &["feast", "potion", "venom"]);

/// One-time freed/seen flags per [`BLIGHT_CAGES`] slot (re-inserted fresh each world build, so
/// a new game re-stocks the cages). `seen` guards against freeing before the patrol spawns.
#[derive(Resource, Default)]
struct BlightCaptives {
    freed: [bool; BLIGHT_CAGES.len()],
    seen: [bool; BLIGHT_CAGES.len()],
}

struct PatrolSite {
    home: Vec2,
    squad: [OrkVariant; 2],
    /// `Some(t)` once the squad was observed wiped (the respawn clock).
    cleared_at: Option<f32>,
    seed: u32,
}

/// The kept-alive ork armory + patrol roster (see `build`).
#[derive(Resource)]
struct BlightPatrols {
    armory: Armory,
    sites: [PatrolSite; 5],
}

fn spawn_squad(commands: &mut Commands, armory: &Armory, site: &mut PatrolSite) {
    for (k, v) in site.squad.iter().enumerate() {
        let ang = k as f32 * 2.6 + site.home.x;
        let pos = site.home + Vec2::new(ang.cos() * 1.7, ang.sin() * 1.7);
        site.seed = site.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        armory.spawn(commands, *v, Faction::Red, site.home, pos, site.seed);
    }
}

/// Repopulate a wiped Blight patrol after [`PATROL_RESPAWN_DELAY`], only while the hero is
/// far enough not to watch orks pop in (mirrors `camps::respawn_warbands`).
fn patrol_respawn(
    time: Res<Time>,
    hero: Res<HeroState>,
    patrols: Option<ResMut<BlightPatrols>>,
    orks: Query<&crate::orks::Ork, Without<crate::dying::Dying>>,
    mut commands: Commands,
) {
    let Some(mut patrols) = patrols else { return };
    let now = time.elapsed_secs();
    let BlightPatrols { armory, sites } = &mut *patrols;
    for site in sites.iter_mut() {
        if orks.iter().any(|o| o.home().distance(site.home) < 1.0) {
            site.cleared_at = None;
            continue;
        }
        let t0 = *site.cleared_at.get_or_insert(now);
        if now - t0 < PATROL_RESPAWN_DELAY
            || (hero.alive && hero.pos.distance(site.home) < PATROL_RESPAWN_FAR)
        {
            continue;
        }
        site.cleared_at = None;
        spawn_squad(&mut commands, armory, site);
    }
}

/// Free a caged captive once the patrol guarding it is wiped (one-time): swap the closed cage
/// for the opened husk, grow the town by one, float + voice the rescue. Mirrors
/// `villagers::camp_rescue` but keyed off the [`BlightPatrols`] homes. `seen` stops a cage from
/// freeing before its squad has even spawned; wave invaders are excluded so a night siege at the
/// keep can't read as "guards cleared".
#[allow(clippy::too_many_arguments)]
fn blight_rescue(
    mut town: ResMut<crate::town::TownRes>,
    captives: Option<ResMut<BlightCaptives>>,
    orks: Query<&crate::orks::Ork, (Without<crate::orks::WaveInvader>, Without<crate::dying::Dying>)>,
    cages_q: Query<(Entity, &BlightCage, &Transform)>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut captives) = captives else { return };
    for (slot, (at_xz, patrol)) in BLIGHT_CAGES.iter().enumerate() {
        if captives.freed[slot] {
            continue;
        }
        let home = PATROL_SITES[*patrol].0;
        if orks.iter().any(|o| o.home().distance(home) < 1.5) {
            captives.seen[slot] = true; // squad still alive — this cage IS guarded
            continue;
        }
        if !captives.seen[slot] {
            continue; // patrol not spawned yet — don't auto-free
        }
        captives.freed[slot] = true;
        // Open the cage in place (swap the closed prop for the husk at its exact pose).
        let y = ground_y(at_xz.x, at_xz.y).unwrap_or(0.0);
        let mut cage_tf = Transform::from_xyz(at_xz.x, y, at_xz.y);
        for (e, c, tf) in &cages_q {
            if c.slot == slot {
                cage_tf = *tf;
                commands.entity(e).try_despawn();
            }
        }
        crate::camps::open_cage(&mut commands, &mut meshes, &mut materials, cage_tf);
        town.0.population += 1; // the freed townsperson appears via town::sync_population_bodies
        floats.0.push(crate::combat_fx::FloatReq {
            world: Vec3::new(at_xz.x, cage_tf.translation.y + 1.8, at_xz.y),
            text: "Captive freed!  +1 townsperson".into(),
            color: Color::srgb(0.5, 1.0, 0.6),
            scale: 1.2,
        });
        cues.write(crate::audio::AudioCue::CampRescue);
        speak.write(crate::audio::Speak::new(crate::audio::Concept::FirstRescue));
    }
}

/// Spawn one static prop against the shared vertex-colour material (batches with the rest).
fn spawn_solid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    m: Mesh,
    pos: Vec3,
    rot: Quat,
) {
    commands.spawn((
        Mesh3d(meshes.add(m)),
        MeshMaterial3d(mat.clone()),
        Transform { translation: pos, rotation: rot, scale: Vec3::ONE },
        BiomeEntity,
    ));
}

/// Spawn one instance of a pre-baked shared-mesh variant set (random pick / yaw / scale) —
/// the scatter's batching path: a whole stand of these costs one draw per variant.
fn spawn_stand(
    commands: &mut Commands,
    handles: &[Handle<Mesh>],
    mat: &Handle<StandardMaterial>,
    pos: Vec3,
    lo: f32,
    hi: f32,
    s: &mut u32,
) {
    let v = (next_u32(s) as usize) % handles.len();
    commands.spawn((
        Mesh3d(handles[v].clone()),
        MeshMaterial3d(mat.clone()),
        Transform {
            translation: pos,
            rotation: ry(rng_range(s, 0.0, TAU)),
            scale: Vec3::splat(rng_range(s, lo, hi)),
        },
        BiomeEntity,
    ));
}

// ── Prop meshes (vertex-coloured, flat-shaded — the CONTRACT.md recipe) ─────────────

// Lifted a few stops above the camps' palette — the hold sits far from the camera and
// against open sea, so darker hues collapsed into one black silhouette (first-shot lesson).
const TIMBER: u32 = 0x6b4a2c;
const TIMBER_DARK: u32 = 0x4a3520;
const TIMBER_PALE: u32 = 0x8a6840;
const IRON: u32 = 0x4a4e56;
const BONE: u32 = 0xddd3b6;
const HIDE: u32 = 0x8a6a45;
const HIDE_DARK: u32 = 0x6d573a;
const HIDE_PATCH: u32 = 0x9d8055;
const WARPAINT: u32 = 0x6a8a20;

/// One palisade run from `a` to `b` (world space; the mesh is authored in world coords and
/// spawned at the origin): jittered sharpened posts + two lashed rails.
fn palisade_segment(a: Vec2, b: Vec2, rng: &mut u32) -> Mesh {
    let d = b - a;
    let len = d.length();
    let dir = d / len;
    let yaw = dir.x.atan2(dir.y);
    let mut p: Vec<Mesh> = Vec::new();
    let n = (len / 1.05).ceil() as i32 + 1;
    for i in 0..n {
        let t = i as f32 / (n - 1).max(1) as f32;
        let mut pos = a + d * t;
        pos += Vec2::new(-dir.y, dir.x) * rng_range(rng, -0.09, 0.09);
        let h = rng_range(rng, 2.35, 2.85);
        let r = rng_range(rng, 0.12, 0.16);
        let y = ground_y(pos.x, pos.y).unwrap_or(0.0);
        let shade = if next_u32(rng) % 3 == 0 { TIMBER_PALE } else { TIMBER };
        p.push(cyl(r, h, v(pos.x, y + h / 2.0, pos.y), Quat::IDENTITY, lin(shade)));
        if rng01(rng) < 0.7 {
            p.push(tinted(
                Cone { radius: r * 0.95, height: 0.4 }
                    .mesh()
                    .build()
                    .translated_by(v(pos.x, y + h + 0.2, pos.y)),
                lin(TIMBER_DARK),
            ));
        }
        if rng01(rng) < 0.12 {
            p.push(bx(0.16, 0.15, 0.15, v(pos.x, y + h - 0.25, pos.y), lin(BONE))); // trophy skull
        }
    }
    let mid = (a + b) / 2.0;
    let my = ground_y(mid.x, mid.y).unwrap_or(0.0);
    for rail_y in [0.95f32, 1.8] {
        p.push(bxr(
            0.07,
            0.1,
            len,
            v(mid.x, my + rail_y + rng_range(rng, -0.05, 0.05), mid.y),
            ry(yaw),
            lin(TIMBER_DARK),
        ));
    }
    group(p)
}

/// The gate **frame**: heavy posts, skull-topped lintel. Faces −Z (the island); authored about
/// its own origin (the gate centre). The two door leaves are separate hinged entities
/// ([`gate_door_mesh`]) so they can swing — this is the fixed surround only.
fn gate_frame_mesh() -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for sx in [-1.0f32, 1.0] {
        p.push(cyl(0.28, 3.6, v(sx * 3.1, 1.8, 0.0), Quat::IDENTITY, lin(TIMBER)));
        p.push(tinted(
            Cone { radius: 0.26, height: 0.5 }.mesh().build().translated_by(v(sx * 3.1, 3.85, 0.0)),
            lin(TIMBER_DARK),
        ));
    }
    p.push(bx(6.9, 0.5, 0.55, v(0.0, 3.35, 0.0), lin(TIMBER_DARK))); // lintel
    for (i, sx) in [-1.0f32, 0.0, 1.0].iter().enumerate() {
        let dy = if i == 1 { 0.06 } else { 0.0 };
        p.push(bx(0.34, 0.30, 0.30, v(sx * 1.6, 3.75 + dy, 0.0), lin(BONE))); // skull row
    }
    group(p)
}

/// One door **leaf**, authored hinged about its own origin so a Y-rotation swings it. `sign`
/// picks the side: the panel (planks + iron bands + outward spike studs) reaches *inward* from
/// the hinge toward the gate centre — for `sign = +1` it occupies local x ∈ [−2.95, 0], for
/// `sign = −1` it occupies x ∈ [0, 2.95]. Studs/bands face −Z (the island) on both.
fn gate_door_mesh(sign: f32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let cx = -sign * 1.475; // panel centre, offset inward from the hinge at x = 0
    p.push(bx(2.95, 3.0, 0.16, v(cx, 1.5, 0.0), lin(TIMBER)));
    for i in 0..4 {
        p.push(bx(0.10, 2.9, 0.03, v(cx - 1.2 + i as f32 * 0.8, 1.5, -0.09), lin(TIMBER_DARK)));
    }
    for by in [0.7f32, 2.2] {
        p.push(bx(2.85, 0.14, 0.05, v(cx, by, -0.11), lin(IRON)));
    }
    for (ux, uy) in [(-0.8f32, 0.9f32), (0.8, 0.9), (-0.8, 2.0), (0.8, 2.0)] {
        p.push(tinted(
            Cone { radius: 0.07, height: 0.22 }
                .mesh()
                .build()
                .rotated_by(rx(-FRAC_PI_2))
                .translated_by(v(cx + ux, uy, -0.2)),
            lin(IRON),
        ));
    }
    group(p)
}

/// A leaning watchtower: splayed legs, cross-braces, crow's-nest platform with spiked
/// parapet and a ragged hide canopy. Authored upright (the spawn adds the lean).
fn tower_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(bxr(
            0.17,
            4.8,
            0.17,
            v(sx * 0.62, 2.3, sz * 0.62),
            Quat::from_rotation_z(-sx * 0.095) * rx(sz * 0.095),
            lin(if (sx + sz).abs() > 1.0 { TIMBER } else { TIMBER_PALE }),
        ));
    }
    // Cross-braces on two faces + ladder rungs up the north face.
    for s in [-1.0f32, 1.0] {
        p.push(bxr(0.07, 2.6, 0.07, v(s * 0.0, 1.5, -0.78), Quat::from_rotation_z(s * 0.65), lin(TIMBER_DARK)));
        p.push(bxr(0.07, 2.6, 0.07, v(s * 0.78, 1.5, 0.0), rx(0.65) * ry(FRAC_PI_2), lin(TIMBER_DARK)));
    }
    for i in 0..5 {
        p.push(bx(0.7, 0.06, 0.06, v(0.0, 0.8 + i as f32 * 0.8, -0.72), lin(TIMBER_PALE)));
    }
    p.push(bx(2.1, 0.16, 2.1, v(0.0, 4.55, 0.0), lin(TIMBER_DARK))); // platform
    for (sx, sz) in [(0.0f32, -1.0f32), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
        let (w, dd) = if sz == 0.0 { (0.09, 2.1) } else { (2.1, 0.09) };
        p.push(bx(w, 0.62, dd, v(sx * 1.0, 4.95, sz * 1.0), lin(TIMBER)));
    }
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(bx(0.12, 0.95, 0.12, v(sx * 1.0, 5.1, sz * 1.0), lin(TIMBER)));
        p.push(tinted(
            Cone { radius: 0.07, height: 0.3 }
                .mesh()
                .build()
                .translated_by(v(sx * 1.0, 5.72, sz * 1.0)),
            lin(TIMBER_DARK),
        ));
    }
    // Hide canopy on two poles, pitched and a little skewed.
    p.push(cyl(0.05, 1.3, v(-0.7, 5.9, 0.4), Quat::IDENTITY, lin(TIMBER_DARK)));
    p.push(cyl(0.05, 1.5, v(0.7, 6.0, -0.4), Quat::IDENTITY, lin(TIMBER_DARK)));
    p.push(bxr(
        2.0,
        0.07,
        1.7,
        v(0.0, 6.55, 0.0),
        Quat::from_rotation_z(0.16) * rx(rng_range(rng, -0.12, 0.12)),
        lin(HIDE),
    ));
    p.push(bx(0.2, 0.18, 0.18, v(0.0, 5.35, -1.05), lin(BONE))); // skull on the front rail
    p.push(cyl(0.045, 1.9, v(0.0, 6.0, 0.0), Quat::IDENTITY, lin(TIMBER_DARK))); // banner pole
    group(p)
}

/// The warlord's great hall: hulking timber walls, a sagging layered hide roof with a
/// spiked ridge, skull-crowned north gable and a black doorway (the glow is a separate
/// emissive entity). Footprint ~11×8, door on −Z. Authored with its floor at y 0.
fn hall_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    p.push(bx(11.4, 0.3, 8.4, v(0.0, 0.15, 0.0), lin(TIMBER_DARK))); // plinth
    p.push(bx(11.0, 2.6, 8.0, v(0.0, 1.6, 0.0), lin(TIMBER)));
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(cyl(0.22, 3.1, v(sx * 5.4, 1.55, sz * 3.9), Quat::IDENTITY, lin(TIMBER_PALE)));
    }
    // Proud vertical planks (subtle wall rhythm) on the long east/west faces.
    for sx in [-1.0f32, 1.0] {
        for i in 0..6 {
            let shade = if i % 2 == 0 { TIMBER_DARK } else { TIMBER_PALE };
            p.push(bx(0.05, 2.4, 0.2, v(sx * 5.53, 1.55, -3.0 + i as f32 * 1.2), lin(shade)));
        }
    }
    // Roof: main slopes to a ridge along Z, then drooping eave skirts + ragged tatters.
    for sx in [-1.0f32, 1.0] {
        p.push(bxr(6.5, 0.14, 8.8, v(sx * 2.75, 4.4, 0.0), rz(-sx * 0.49), lin(HIDE)));
        p.push(bxr(5.9, 0.1, 8.3, v(sx * 2.5, 4.62, 0.25), rz(-sx * 0.49), lin(HIDE_DARK)));
        p.push(bxr(1.9, 0.09, 8.9, v(sx * 5.15, 2.62, 0.0), rz(-sx * 0.78), lin(HIDE_DARK)));
        for i in 0..4 {
            p.push(bxr(
                0.5,
                0.06,
                rng_range(rng, 0.5, 0.9),
                v(sx * 5.75, 2.2 + rng_range(rng, -0.15, 0.1), -3.0 + i as f32 * 2.0),
                rz(-sx * rng_range(rng, 0.5, 0.9)),
                lin(HIDE),
            ));
        }
    }
    p.push(cyl(0.13, 9.2, v(0.0, 5.95, 0.0), rx(FRAC_PI_2), lin(TIMBER_DARK))); // ridge log
    for i in 0..5 {
        p.push(tinted(
            Cone { radius: 0.07, height: 0.5 }
                .mesh()
                .build()
                .translated_by(v(0.0, 6.25, -3.4 + i as f32 * 1.7)),
            lin(IRON),
        ));
    }
    // North gable: stacked narrowing timber + the big horned skull.
    p.push(bx(8.0, 1.2, 0.5, v(0.0, 3.5, -3.75), lin(TIMBER_DARK)));
    p.push(bx(5.2, 1.1, 0.5, v(0.0, 4.6, -3.75), lin(TIMBER)));
    p.push(bx(2.4, 1.0, 0.5, v(0.0, 5.6, -3.75), lin(TIMBER_DARK)));
    p.push(bx(0.62, 0.56, 0.45, v(0.0, 5.35, -4.05), lin(BONE)));
    for sx in [-1.0f32, 1.0] {
        p.push(tinted(
            Cone { radius: 0.07, height: 0.55 }
                .mesh()
                .build()
                .rotated_by(rz(sx * 1.15))
                .translated_by(v(sx * 0.55, 5.62, -4.05)),
            lin(BONE),
        ));
    }
    // Doorway: black inset + frame + worn entry planks.
    p.push(bx(2.3, 2.7, 0.2, v(0.0, 1.35, -3.98), lin(0x171008)));
    for sx in [-1.0f32, 1.0] {
        p.push(cyl(0.14, 2.9, v(sx * 1.3, 1.45, -4.05), Quat::IDENTITY, lin(TIMBER_PALE)));
    }
    p.push(bx(2.9, 0.22, 0.3, v(0.0, 2.95, -4.05), lin(TIMBER_PALE)));
    p.push(bx(2.6, 0.14, 0.6, v(0.0, 0.07, -4.45), lin(TIMBER_DARK)));
    // The hall-ridge banner pole (the cloth is a banner.rs entity).
    p.push(cyl(0.06, 2.9, v(0.0, 6.7, 3.2), Quat::IDENTITY, lin(TIMBER_DARK)));
    group(p)
}

/// The crooked spire: five tapering timber tiers, each twisted and shoved off-axis, up to
/// an iron-crowned platform (the green brazier flame is a separate emissive entity).
fn spire_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    p.push(bx(3.4, 0.4, 3.4, v(0.0, 0.2, 0.0), lin(TIMBER_DARK)));
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(bxr(
            0.14,
            1.6,
            0.14,
            v(sx * 1.55, 0.9, sz * 1.55),
            Quat::from_rotation_z(-sx * 0.25) * rx(sz * 0.25),
            lin(TIMBER),
        ));
    }
    let tiers = [(2.6f32, 2.4f32), (2.15, 2.3), (1.75, 2.2), (1.4, 2.1), (1.1, 2.0)];
    let mut y = 0.4;
    let mut twist = 0.0;
    let mut off = Vec2::ZERO;
    for (i, (w, h)) in tiers.iter().enumerate() {
        twist += rng_range(rng, 0.10, 0.22);
        off += Vec2::new(rng_range(rng, -0.14, 0.14), rng_range(rng, -0.14, 0.14));
        let shade = if i % 2 == 0 { TIMBER } else { TIMBER_DARK };
        p.push(bxr(*w, *h, *w, v(off.x, y + h / 2.0, off.y), ry(twist), lin(shade)));
        p.push(bxr(w + 0.12, 0.14, w + 0.12, v(off.x, y + h, off.y), ry(twist), lin(TIMBER_DARK)));
        if i % 2 == 1 {
            // A jutting beam stub with a dangling skull — hand-hewn clutter.
            p.push(bxr(0.1, 0.1, w + 1.1, v(off.x, y + h * 0.6, off.y), ry(twist + 0.5), lin(TIMBER_PALE)));
            p.push(bx(0.15, 0.14, 0.14, v(off.x + (twist + 0.5).sin() * (w / 2.0 + 0.5), y + h * 0.6 - 0.25, off.y + (twist + 0.5).cos() * (w / 2.0 + 0.5)), lin(BONE)));
        }
        y += h;
    }
    // Crow platform + iron crown + the centre pole skull.
    p.push(bxr(1.9, 0.16, 1.9, v(off.x, y + 0.08, off.y), ry(twist), lin(TIMBER_DARK)));
    for i in 0..6 {
        let a = i as f32 / 6.0 * TAU;
        p.push(tinted(
            Cone { radius: 0.09, height: 0.55 }
                .mesh()
                .build()
                .translated_by(v(off.x + a.cos() * 0.75, y + 0.45, off.y + a.sin() * 0.75)),
            lin(IRON),
        ));
    }
    p.push(cyl(0.05, 1.1, v(off.x, y + 0.7, off.y), Quat::IDENTITY, lin(IRON)));
    p.push(bx(0.2, 0.19, 0.19, v(off.x, y + 1.3, off.y), lin(BONE)));
    // The brazier bowl the green flame sits in (flame entity is offset to match).
    p.push(cyl(0.34, 0.28, v(0.45, y + 0.3, -0.3), Quat::IDENTITY, lin(IRON)));
    // The spire banner pole.
    p.push(cyl(0.05, 2.4, v(-0.8, y - 0.9, 0.0), Quat::IDENTITY, lin(TIMBER_DARK)));
    group(p)
}

/// An ork hide tent: crossed-pole A-frame, two stretched-hide slopes with mismatched
/// patches and ragged eaves, a dark door mouth at −Z, a trophy skull on the ridge.
/// `pub(crate)`: the wilderness camps (`camps.rs`) pitch the same tents.
pub(crate) fn tent_mesh(s: f32, rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let h = 2.2 * s; // ridge height
    let w = 1.8 * s; // ground half-width
    let d = 1.5 * s; // half-depth
    let tilt = (w / h).atan();
    let slope = (w * w + h * h).sqrt();
    // Crossed pole pairs (tips poke past the ridge) + the ridge log between them.
    for sz in [-1.0f32, 1.0] {
        for sx in [-1.0f32, 1.0] {
            p.push(cyl(
                0.07 * s,
                slope * 1.14,
                v(sx * w * 0.5, h * 0.52, sz * d * 0.94),
                rz(-sx * tilt),
                lin(TIMBER_DARK),
            ));
        }
    }
    p.push(cyl(0.08 * s, d * 2.3, v(0.0, h + 0.08 * s, 0.0), rx(FRAC_PI_2), lin(TIMBER)));
    // The two hide slopes: main sheet + a mismatched patch + ragged eave tatters.
    for sx in [-1.0f32, 1.0] {
        let main = if rng01(rng) < 0.5 { HIDE } else { HIDE_DARK };
        p.push(bxr(slope, 0.06, d * 2.0, v(sx * w * 0.5, h * 0.5, 0.0), rz(-sx * tilt), lin(main)));
        p.push(bxr(
            slope * 0.45,
            0.05,
            d * rng_range(rng, 0.6, 1.0),
            v(sx * (w * 0.5 + 0.04), h * 0.5 + 0.03, rng_range(rng, -0.4, 0.4) * d),
            rz(-sx * tilt),
            lin(if main == HIDE { HIDE_DARK } else { HIDE_PATCH }),
        ));
        for i in 0..3 {
            p.push(bxr(
                0.34 * s,
                0.05,
                rng_range(rng, 0.3, 0.55) * s,
                v(sx * (w * 0.86), h * 0.13, (-0.8 + i as f32 * 0.8) * d),
                rz(-sx * (tilt + rng_range(rng, 0.2, 0.5))),
                lin(HIDE_DARK),
            ));
        }
    }
    // Back wall + the dark door mouth up front.
    p.push(bx(w * 1.5, h * 0.8, 0.08, v(0.0, h * 0.4, d * 0.95), lin(HIDE_DARK)));
    p.push(bx(0.75 * s, 1.0 * s, 0.1, v(0.0, 0.5 * s, -d * 0.92), lin(0x171008)));
    if rng01(rng) < 0.7 {
        p.push(bx(0.17 * s, 0.16 * s, 0.16 * s, v(0.0, h + 0.2 * s, -d * 0.8), lin(BONE)));
    }
    group(p)
}

/// A timber longhouse — the warband's barracks: log-course walls on corner posts, pitched
/// hide roof with patches, a skull gable, a dark door mouth (−Z) and a log pile outside.
fn longhouse_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    p.push(bx(3.8, 0.24, 5.8, v(0.0, 0.12, 0.0), lin(TIMBER_DARK))); // plinth
    p.push(bx(3.4, 1.9, 5.4, v(0.0, 1.15, 0.0), lin(TIMBER)));
    for sx in [-1.0f32, 1.0] {
        for i in 0..3 {
            p.push(bx(
                0.07,
                0.16,
                5.3,
                v(sx * 1.74, 0.55 + i as f32 * 0.55, 0.0),
                lin(if i % 2 == 0 { TIMBER_DARK } else { TIMBER_PALE }),
            ));
        }
    }
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(cyl(0.14, 2.3, v(sx * 1.62, 1.15, sz * 2.6), Quat::IDENTITY, lin(TIMBER_PALE)));
    }
    // Pitched hide roof (ridge along Z) + a slipped patch each side + ridge log + gable skull.
    let tilt = 0.62f32;
    for sx in [-1.0f32, 1.0] {
        p.push(bxr(
            2.4,
            0.1,
            6.2,
            v(sx * 0.95, 2.65, 0.0),
            rz(-sx * tilt),
            lin(if rng01(rng) < 0.5 { HIDE } else { HIDE_DARK }),
        ));
        p.push(bxr(
            1.1,
            0.08,
            rng_range(rng, 1.2, 2.2),
            v(sx * 1.7, 2.25, rng_range(rng, -1.5, 1.5)),
            rz(-sx * (tilt + 0.25)),
            lin(HIDE_PATCH),
        ));
    }
    p.push(cyl(0.1, 6.4, v(0.0, 3.32, 0.0), rx(FRAC_PI_2), lin(TIMBER_DARK)));
    p.push(bx(0.5, 0.42, 0.36, v(0.0, 2.6, -2.85), lin(BONE)));
    // Door mouth + frame posts.
    p.push(bx(1.0, 1.5, 0.14, v(0.0, 0.75, -2.74), lin(0x171008)));
    for sx in [-1.0f32, 1.0] {
        p.push(cyl(0.09, 1.7, v(sx * 0.62, 0.85, -2.78), Quat::IDENTITY, lin(TIMBER_PALE)));
    }
    // Log pile against the +X wall.
    for i in 0..4 {
        let (lift, off) = if i < 3 { (0.14, (i as f32 - 1.0) * 0.30) } else { (0.40, -0.15) };
        p.push(cyl(
            0.14,
            rng_range(rng, 1.6, 2.2),
            v(1.95 + off, lift, 1.2),
            rx(FRAC_PI_2) * rz(rng_range(rng, -0.08, 0.08)),
            lin(if i % 2 == 0 { TIMBER } else { TIMBER_PALE }),
        ));
    }
    group(p)
}

/// The hold's forge: a stone hearth (the ember glow is a separate emissive entity), a
/// horned anvil on a stump and a quench barrel. Authored about its origin, hearth at −Z.
fn forge_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for i in 0..10 {
        let a = i as f32 / 10.0 * TAU;
        p.push(tinted(
            Sphere::new(rng_range(rng, 0.16, 0.24))
                .mesh()
                .ico(0)
                .unwrap()
                .translated_by(v(a.cos() * 0.85, 0.12, a.sin() * 0.6 - 0.35)),
            lin(0x66666e),
        ));
    }
    p.push(bx(1.5, 0.85, 1.1, v(0.0, 0.42, -0.35), lin(0x5a5a62))); // hearth block
    p.push(bx(0.5, 1.15, 0.5, v(-0.45, 1.4, -0.6), lin(0x4e4e56))); // chimney stub
    // Anvil on a stump + the horn nub.
    p.push(cyl(0.3, 0.5, v(0.95, 0.25, 0.55), Quat::IDENTITY, lin(TIMBER_DARK)));
    p.push(bx(0.62, 0.22, 0.3, v(0.95, 0.61, 0.55), lin(IRON)));
    p.push(bx(0.22, 0.14, 0.18, v(1.32, 0.62, 0.55), lin(IRON)));
    // Quench barrel with a dark water top.
    p.push(cyl(0.3, 0.62, v(-1.15, 0.31, 0.5), Quat::IDENTITY, lin(TIMBER)));
    p.push(cyl(0.26, 0.04, v(-1.15, 0.63, 0.5), Quat::IDENTITY, lin(0x2a3438)));
    group(p)
}

/// A crude boar pen: post-and-rail fence with a gate gap (−Z side), churned mud and a
/// feed trough. Fence half-extent ~2.3.
fn pen_mesh(rng: &mut u32) -> Mesh {
    const HW: f32 = 2.3;
    let mut p: Vec<Mesh> = Vec::new();
    // Posts on all four sides (the −Z side leaves its middle pair out for the gap).
    for side in 0..4 {
        for i in 0..5 {
            if side == 0 && (i == 2 || i == 3) {
                continue;
            }
            let t = i as f32 / 4.0 * 2.0 - 1.0;
            let (x, z) = match side {
                0 => (t * HW, -HW),
                1 => (t * HW, HW),
                2 => (-HW, t * HW),
                _ => (HW, t * HW),
            };
            let h = rng_range(rng, 0.95, 1.2);
            p.push(cyl(0.08, h, v(x, h / 2.0, z), rz(rng_range(rng, -0.07, 0.07)), lin(TIMBER_DARK)));
        }
    }
    // Rails: full runs on three sides, two stubs flanking the gate gap.
    for rail_y in [0.42f32, 0.82] {
        p.push(bx(2.0 * HW, 0.08, 0.09, v(0.0, rail_y, HW), lin(TIMBER)));
        p.push(bx(0.09, 0.08, 2.0 * HW, v(-HW, rail_y + 0.03, 0.0), lin(TIMBER)));
        p.push(bx(0.09, 0.08, 2.0 * HW, v(HW, rail_y - 0.02, 0.0), lin(TIMBER)));
        for sx in [-1.0f32, 1.0] {
            p.push(bx(HW - 0.7, 0.08, 0.09, v(sx * (HW * 0.5 + 0.35), rail_y, -HW), lin(TIMBER)));
        }
    }
    // Churned mud + the feed trough.
    p.push(cyl(1.3, 0.03, v(0.4, 0.03, 0.3), Quat::IDENTITY, lin(0x241d15)));
    p.push(cyl(0.8, 0.03, v(-0.8, 0.045, -0.6), Quat::IDENTITY, lin(0x1c1610)));
    p.push(bx(1.1, 0.26, 0.42, v(0.6, 0.13, 1.6), lin(TIMBER_DARK)));
    p.push(bx(0.95, 0.06, 0.3, v(0.6, 0.24, 1.6), lin(0x4a3d28)));
    group(p)
}

/// A war drum: a fat hide-topped barrel on log cradles, beater sticks left crossed on top.
fn drum_mesh(rng: &mut u32) -> Mesh {
    let r = rng_range(rng, 0.5, 0.62);
    group(vec![
        cyl(r, 0.9, v(0.0, 0.55, 0.0), Quat::IDENTITY, lin(TIMBER)),
        cyl(r + 0.03, 0.07, v(0.0, 1.02, 0.0), Quat::IDENTITY, lin(HIDE)),
        cyl(r + 0.04, 0.06, v(0.0, 0.35, 0.0), Quat::IDENTITY, lin(TIMBER_DARK)),
        cyl(0.1, 0.6, v(-r * 0.8, 0.1, 0.0), rx(FRAC_PI_2), lin(TIMBER_DARK)),
        cyl(0.1, 0.6, v(r * 0.8, 0.1, 0.0), rx(FRAC_PI_2), lin(TIMBER_DARK)),
        cyl(0.03, 0.7, v(0.15, 1.12, 0.05), rz(0.9) * rx(0.4), lin(TIMBER_PALE)),
        cyl(0.03, 0.7, v(-0.12, 1.12, -0.04), rz(-0.8) * rx(-0.3), lin(TIMBER_PALE)),
    ])
}

/// A weapon rack: two posts, a crossbar, and a row of leaning spears/clubs/crude blades.
fn rack_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for sx in [-1.0f32, 1.0] {
        p.push(cyl(0.07, 1.5, v(sx * 0.8, 0.75, 0.0), Quat::IDENTITY, lin(TIMBER_DARK)));
    }
    p.push(bx(1.9, 0.09, 0.09, v(0.0, 1.42, 0.0), lin(TIMBER)));
    for i in 0..4 {
        let x = -0.6 + i as f32 * 0.4;
        let tilt = rng_range(rng, -0.16, 0.16);
        match next_u32(rng) % 3 {
            0 => {
                p.push(cyl(0.035, 1.7, v(x, 0.85, 0.1), rx(0.22) * rz(tilt), lin(TIMBER_PALE)));
                p.push(tinted(
                    Cone { radius: 0.06, height: 0.22 }
                        .mesh()
                        .build()
                        .translated_by(v(x, 1.72, 0.28)),
                    lin(IRON),
                ));
            }
            1 => {
                p.push(cyl(0.05, 1.2, v(x, 0.6, 0.12), rx(0.2) * rz(tilt), lin(TIMBER)));
                p.push(bx(0.18, 0.3, 0.18, v(x, 1.22, 0.24), lin(TIMBER_DARK)));
            }
            _ => {
                p.push(bxr(0.1, 1.1, 0.04, v(x, 0.62, 0.12), rx(0.2) * rz(tilt), lin(IRON)));
                p.push(bx(0.16, 0.1, 0.08, v(x, 0.16, 0.04), lin(TIMBER_DARK)));
            }
        }
    }
    group(p)
}

/// The spit roast beside the bonfire: two forked poles, a skewer, the night's carcass.
fn spit_mesh(rng: &mut u32) -> Mesh {
    group(vec![
        cyl(0.06, 1.1, v(-0.8, 0.55, 0.0), rz(0.1), lin(TIMBER_DARK)),
        cyl(0.06, 1.1, v(0.8, 0.55, 0.0), rz(-0.1), lin(TIMBER_DARK)),
        cyl(0.04, 2.0, v(0.0, 1.05, 0.0), rz(FRAC_PI_2), lin(TIMBER_PALE)),
        bxr(0.7, 0.3, 0.34, v(0.0, 1.0, 0.0), rz(rng_range(rng, -0.2, 0.2)), lin(0x8a5a38)),
        bx(0.2, 0.16, 0.2, v(0.45, 1.0, 0.0), lin(0x7a4a2c)),
    ])
}

/// A spoils pile: stacked plunder crates + grain sacks dumped where they were dropped.
fn pile_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    p.push(bxr(0.7, 0.7, 0.7, v(0.0, 0.35, 0.0), ry(rng01(rng)), lin(TIMBER)));
    p.push(bxr(0.55, 0.55, 0.55, v(0.75, 0.27, 0.3), ry(rng01(rng) * 0.8), lin(TIMBER_PALE)));
    p.push(bxr(0.5, 0.5, 0.5, v(0.2, 0.95, 0.1), ry(rng01(rng)), lin(TIMBER_DARK)));
    for (x, z) in [(-0.7f32, 0.4f32), (-0.5, -0.4), (0.9, -0.5)] {
        p.push(tinted(
            Sphere::new(0.32)
                .mesh()
                .ico(1)
                .unwrap()
                .scaled_by(Vec3::new(1.0, 0.62, 1.0))
                .translated_by(v(x, 0.2, z)),
            lin(0x9a8a64),
        ));
    }
    group(p)
}

/// A gnarled claw-tree: a thick leaning trunk forking into crooked talon branches that
/// rake at the sky — the Blight's second tree silhouette (`TreeKind::Dead` is the first).
fn claw_tree_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let shade = [0x4a3b28u32, 0x3a2e1e, 0x55432c][(next_u32(rng) % 3) as usize];
    let dark = lin(shade);
    let h = rng_range(rng, 1.8, 2.6);
    p.push(cyl(0.17, h, v(0.0, h / 2.0, 0.0), rz(rng_range(rng, -0.08, 0.08)), dark));
    let top = v(0.0, h - 0.08, 0.0);
    let n = 4 + (next_u32(rng) % 2) as i32;
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU + rng01(rng) * 0.8;
        let out = rng_range(rng, 0.5, 0.95);
        let q = ry(a) * rz(out);
        let axis = q * Vec3::Y;
        let bl = rng_range(rng, 0.9, 1.5);
        p.push(tinted(
            Cylinder::new(0.07, bl).mesh().resolution(5).build().rotated_by(q).translated_by(top + axis * (bl * 0.5)),
            dark,
        ));
        // The talon tip, bent further outward.
        let q2 = ry(a) * rz(out + 0.55);
        p.push(tinted(
            Cone { radius: 0.055, height: 0.5 }
                .mesh()
                .build()
                .rotated_by(q2)
                .translated_by(top + axis * bl + (q2 * Vec3::Y) * 0.2),
            lin(0x2e2418),
        ));
    }
    group(p)
}

/// A snapped snag: a leaning broken trunk with a splintered crown, its fallen top rotting
/// in the mud beside it.
fn snag_mesh(rng: &mut u32) -> Mesh {
    let h = rng_range(rng, 1.4, 2.2);
    let lean = rng_range(rng, 0.15, 0.3) * if next_u32(rng) % 2 == 0 { 1.0 } else { -1.0 };
    let mut p = vec![cyl(0.19, h, v(0.0, h / 2.0, 0.0), rz(lean), lin(0x443522))];
    for _ in 0..3 {
        p.push(tinted(
            Cone { radius: 0.06, height: rng_range(rng, 0.25, 0.5) }
                .mesh()
                .build()
                .translated_by(v(-lean * h + rng_range(rng, -0.12, 0.12), h * 0.95, rng_range(rng, -0.12, 0.12))),
            lin(0x57452c),
        ));
    }
    p.push(bxr(
        0.3,
        0.3,
        rng_range(rng, 1.0, 1.8),
        v(rng_range(rng, 0.5, 0.9), 0.15, rng_range(rng, -0.4, 0.4)),
        ry(rng01(rng) * TAU) * rz(0.05),
        lin(0x3a2d1c),
    ));
    group(p)
}

/// A gibbet: a leaning pole and arm with a small iron bone-cage swinging on chain links —
/// the orks' message to travellers.
fn gibbet_mesh(rng: &mut u32) -> Mesh {
    let mut p = vec![
        cyl(0.09, 3.2, v(0.0, 1.6, 0.0), rz(rng_range(rng, -0.05, 0.05)), lin(TIMBER_DARK)),
        bx(1.4, 0.1, 0.1, v(0.55, 3.1, 0.0), lin(TIMBER)),
    ];
    for k in 0..3 {
        p.push(cyl(0.022, 0.18, v(1.1, 2.92 - k as f32 * 0.16, 0.0), Quat::IDENTITY, lin(IRON)));
    }
    let cy = 2.1;
    p.push(bx(0.5, 0.06, 0.5, v(1.1, cy + 0.32, 0.0), lin(IRON)));
    p.push(bx(0.45, 0.06, 0.45, v(1.1, cy - 0.32, 0.0), lin(IRON)));
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        p.push(bx(0.05, 0.62, 0.05, v(1.1 + sx * 0.2, cy, sz * 0.2), lin(IRON)));
    }
    p.push(bx(0.2, 0.3, 0.18, v(1.1, cy - 0.08, 0.0), lin(BONE))); // the remains
    group(p)
}

/// A tall stake with remains run through partway up — ribs, a skull, one dangling bone.
fn impale_mesh(rng: &mut u32) -> Mesh {
    let h = rng_range(rng, 1.6, 2.2);
    let tilt = rng_range(rng, -0.18, 0.18);
    group(vec![
        cyl(0.06, h, v(0.0, h / 2.0, 0.0), rz(tilt), lin(TIMBER_DARK)),
        tinted(
            Cone { radius: 0.05, height: 0.3 }.mesh().build().translated_by(v(-tilt * h, h, 0.0)),
            lin(TIMBER_PALE),
        ),
        bx(0.42, 0.34, 0.3, v(-tilt * h * 0.6, h * 0.62, 0.0), lin(BONE)),
        bx(0.2, 0.18, 0.18, v(-tilt * h * 0.6, h * 0.62 + 0.28, 0.05), lin(BONE)),
        bxr(0.07, 0.4, 0.07, v(-tilt * h * 0.6 + 0.2, h * 0.62 - 0.3, 0.0), rz(0.3), lin(BONE)),
    ])
}

/// The ribcage of some titanic beast, arcing out of the mud toward a horned skull.
fn ribcage_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let n = 4 + (next_u32(rng) % 2) as i32;
    for i in 0..n {
        let z = i as f32 * 0.55;
        let s = 1.0 - i as f32 * 0.12; // ribs shrink toward the tail
        for sx in [-1.0f32, 1.0] {
            p.push(bxr(0.09, 1.1 * s, 0.13, v(sx * 0.75 * s, 0.5 * s, z), rz(-sx * 0.35), lin(BONE)));
            p.push(bxr(0.08, 0.7 * s, 0.11, v(sx * 0.42 * s, 1.15 * s, z), rz(-sx * 0.85), lin(0xcabfa2)));
        }
    }
    p.push(bx(0.16, 0.14, n as f32 * 0.55, v(0.0, 0.08, (n as f32 - 1.0) * 0.275), lin(0xcabfa2))); // spine
    p.push(bx(0.55, 0.45, 0.6, v(0.0, 0.3, -0.75), lin(BONE))); // the skull
    for sx in [-1.0f32, 1.0] {
        p.push(tinted(
            Cone { radius: 0.08, height: 0.5 }
                .mesh()
                .build()
                .rotated_by(rz(sx * 1.2))
                .translated_by(v(sx * 0.45, 0.62, -0.8)),
            lin(BONE),
        ));
    }
    group(p)
}

/// Battle leavings: split shields, snapped blades, a cart wheel half-sunk in the mud.
fn scrap_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for _ in 0..3 {
        let shade = [TIMBER, TIMBER_PALE, 0x6a3a2a][(next_u32(rng) % 3) as usize];
        p.push(bxr(
            rng_range(rng, 0.4, 0.6),
            0.07,
            rng_range(rng, 0.5, 0.7),
            v(rng_range(rng, -0.5, 0.5), 0.08, rng_range(rng, -0.5, 0.5)),
            ry(rng01(rng) * TAU) * rz(rng_range(rng, -0.25, 0.25)),
            lin(shade),
        ));
    }
    for _ in 0..2 {
        p.push(bxr(
            0.08,
            rng_range(rng, 0.6, 0.9),
            0.03,
            v(rng_range(rng, -0.4, 0.4), 0.12, rng_range(rng, -0.4, 0.4)),
            ry(rng01(rng) * TAU) * rz(rng_range(rng, 0.9, 1.4)),
            lin(IRON),
        ));
    }
    p.push(tinted(
        Cylinder::new(0.45, 0.07)
            .mesh()
            .resolution(8)
            .build()
            .rotated_by(rx(FRAC_PI_2) * rz(rng_range(rng, 0.2, 0.5)))
            .translated_by(v(0.5, 0.3, 0.2)),
        lin(TIMBER_DARK),
    ));
    group(p)
}

/// A crude warning effigy: cross-pole scarecrow draped in hides, skull head, paint band.
fn effigy_mesh(rng: &mut u32) -> Mesh {
    group(vec![
        cyl(0.07, 2.2, v(0.0, 1.1, 0.0), rz(rng_range(rng, -0.07, 0.07)), lin(TIMBER_DARK)),
        bx(1.5, 0.09, 0.09, v(0.0, 1.62, 0.0), lin(TIMBER)),
        bxr(0.6, 0.9, 0.1, v(0.0, 1.15, 0.0), ry(rng_range(rng, -0.2, 0.2)), lin(HIDE_DARK)),
        bxr(0.34, 0.6, 0.08, v(0.45, 1.3, 0.02), rz(0.25), lin(HIDE)),
        bxr(0.3, 0.55, 0.08, v(-0.5, 1.32, -0.02), rz(-0.3), lin(HIDE_PATCH)),
        bx(0.22, 0.2, 0.2, v(0.0, 1.95, 0.0), lin(BONE)),
        bx(0.24, 0.05, 0.21, v(0.0, 1.9, 0.0), lin(WARPAINT)),
    ])
}

/// Sickly warp toadstools clustered on the mud.
fn shroom_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for _ in 0..(3 + next_u32(rng) % 3) {
        let x = rng_range(rng, -0.5, 0.5);
        let z = rng_range(rng, -0.5, 0.5);
        let h = rng_range(rng, 0.16, 0.42);
        let r = h * rng_range(rng, 0.55, 0.8);
        p.push(cyl(r * 0.35, h, v(x, h / 2.0, z), rz(rng_range(rng, -0.15, 0.15)), lin(0xb8b09a)));
        let cap = [0x73923eu32, 0x86a83c, 0x5f7e34][(next_u32(rng) % 3) as usize];
        p.push(tinted(
            Sphere::new(r)
                .mesh()
                .ico(1)
                .unwrap()
                .scaled_by(Vec3::new(1.0, 0.55, 1.0))
                .translated_by(v(x, h, z)),
            lin(cap),
        ));
    }
    group(p)
}

/// A black tar pit with old bones breaking its surface.
fn tar_pit_mesh(rng: &mut u32) -> Mesh {
    let r = rng_range(rng, 1.2, 2.0);
    group(vec![
        cyl(r, 0.035, v(0.0, 0.03, 0.0), Quat::IDENTITY, lin(0x14110c)),
        cyl(
            r * 0.55,
            0.035,
            v(rng_range(rng, -0.4, 0.4), 0.05, rng_range(rng, -0.4, 0.4)),
            Quat::IDENTITY,
            lin(0x0c0a07),
        ),
        bxr(0.09, 0.7, 0.09, v(r * 0.3, 0.2, -r * 0.2), rz(0.6), lin(BONE)),
        bx(0.2, 0.16, 0.16, v(-r * 0.35, 0.08, r * 0.25), lin(BONE)),
        tinted(
            Cone { radius: 0.07, height: 0.4 }
                .mesh()
                .build()
                .rotated_by(rz(-0.9))
                .translated_by(v(-r * 0.3, 0.16, -r * 0.3)),
            lin(0xcabfa2),
        ),
    ])
}

/// A warp-fume vent: a low ring of scorched stones around a dark throat (the green smoke
/// is a pair of `FortSmoke` entities).
fn vent_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for i in 0..7 {
        let a = i as f32 / 7.0 * TAU + rng01(rng);
        p.push(tinted(
            Sphere::new(rng_range(rng, 0.12, 0.2))
                .mesh()
                .ico(0)
                .unwrap()
                .translated_by(v(a.cos() * 0.45, 0.08, a.sin() * 0.45)),
            lin(0x4a4a44),
        ));
    }
    p.push(cyl(0.3, 0.06, v(0.0, 0.05, 0.0), Quat::IDENTITY, lin(0x14120c)));
    group(p)
}

/// A road waypost: a leaning pole with a skull, a hide rag and a warning crossbar — the
/// orks signpost the road to their own gate.
fn waypost_mesh(rng: &mut u32) -> Mesh {
    let lean = rng_range(rng, -0.1, 0.1);
    group(vec![
        cyl(0.07, 2.6, v(0.0, 1.3, 0.0), rz(lean), lin(TIMBER_DARK)),
        bx(0.9, 0.1, 0.1, v(0.0, 2.0, 0.0), lin(TIMBER)),
        bx(0.2, 0.19, 0.19, v(0.0, 2.72, 0.0), lin(BONE)),
        bxr(0.34, 0.5, 0.05, v(0.35, 1.7, 0.0), rz(rng_range(rng, -0.25, 0.25)), lin(HIDE_DARK)),
        bxr(0.3, 0.4, 0.05, v(-0.38, 1.78, 0.0), rz(rng_range(rng, -0.3, 0.3)), lin(HIDE)),
    ])
}

/// The hold's war totem — four stacked carved heads, war-paint bands, horned skull crown.
/// Bigger and meaner than the camps'. Authored facing +Z (the build yaws it at the castle).
fn totem_mesh(rng: &mut u32) -> Mesh {
    let paint = lin(WARPAINT);
    let eye = lin(0x16130f);
    let mut p: Vec<Mesh> = Vec::new();
    p.push(cyl(0.22, 0.3, v(0.0, 0.15, 0.0), Quat::IDENTITY, lin(TIMBER_DARK)));
    let head = |p: &mut Vec<Mesh>, w: f32, y0: f32, h: f32, yaw: f32, c: u32| {
        let q = ry(yaw);
        p.push(bxr(w, h, w * 0.9, v(0.0, y0 + h / 2.0, 0.0), q, lin(c)));
        p.push(bxr(w * 0.92, 0.08, w * 0.2, q * v(0.0, 0.0, w * 0.40) + v(0.0, y0 + h * 0.78, 0.0), q, lin(TIMBER_DARK)));
        for sx in [-1.0_f32, 1.0] {
            p.push(bxr(0.11, 0.11, 0.07, q * v(sx * w * 0.22, 0.0, w * 0.45) + v(0.0, y0 + h * 0.6, 0.0), q, eye));
        }
        p.push(bxr(w * 0.5, 0.06, 0.07, q * v(0.0, 0.0, w * 0.45) + v(0.0, y0 + h * 0.24, 0.0), q, eye));
    };
    let mut y = 0.3;
    for (i, w) in [0.66f32, 0.58, 0.5, 0.44].iter().enumerate() {
        let h = 0.55 - i as f32 * 0.03;
        head(&mut p, *w, y, h, rng_range(rng, -0.16, 0.16), if i % 2 == 0 { TIMBER } else { TIMBER_DARK });
        y += h;
        p.push(bx(w - 0.02, 0.09, w * 0.9 - 0.02, v(0.0, y + 0.045, 0.0), paint));
        y += 0.09;
    }
    for sx in [-1.0_f32, 1.0] {
        p.push(tinted(
            Cone { radius: 0.06, height: 0.36 }
                .mesh()
                .build()
                .rotated_by(rz(sx * 1.1))
                .translated_by(v(sx * 0.28, y + 0.1, 0.0)),
            lin(BONE),
        ));
    }
    p.push(bx(0.24, 0.22, 0.22, v(0.0, y + 0.16, 0.0), lin(BONE)));
    group(p)
}

/// A heavy prisoner cage with three huddled captives (decorative — the hold's are beyond
/// rescue; that's the story it tells). `pub(crate)`: the wilderness camps use the same
/// cage for their closed (pre-rescue) state.
pub(crate) fn cage_mesh() -> Mesh {
    const W: f32 = 2.2;
    const H: f32 = 1.8;
    const HW: f32 = W / 2.0;
    let wood = lin(TIMBER);
    let dark = lin(TIMBER_DARK);
    let bar = lin(IRON);
    let mut p: Vec<Mesh> = Vec::new();
    p.push(bx(W + 0.14, 0.14, W + 0.14, v(0.0, 0.07, 0.0), dark));
    for (sx, sz) in [(-HW, -HW), (HW, -HW), (-HW, HW), (HW, HW)] {
        p.push(bx(0.16, H, 0.16, v(sx, H / 2.0, sz), wood));
    }
    p.push(bx(W, 0.12, 0.12, v(0.0, H - 0.06, -HW), wood));
    p.push(bx(W, 0.12, 0.12, v(0.0, H - 0.06, HW), wood));
    p.push(bx(0.12, 0.12, W, v(-HW, H - 0.06, 0.0), wood));
    p.push(bx(0.12, 0.12, W, v(HW, H - 0.06, 0.0), wood));
    for o in [-0.66f32, -0.22, 0.22, 0.66] {
        p.push(bx(0.08, H - 0.07, 0.08, v(o, H / 2.0, -HW), bar));
        p.push(bx(0.08, H - 0.07, 0.08, v(o, H / 2.0, HW), bar));
        p.push(bx(0.08, H - 0.07, 0.08, v(-HW, H / 2.0, o), bar));
        p.push(bx(0.08, H - 0.07, 0.08, v(HW, H / 2.0, o), bar));
    }
    for (cx, cz) in [(-0.45f32, 0.25f32), (0.4, -0.3), (0.05, 0.55)] {
        p.push(bx(0.34, 0.58, 0.24, v(cx, 0.34, cz), lin(0x7c6a54)));
        p.push(bx(0.24, 0.24, 0.24, v(cx, 0.78, cz), lin(0xcaa980)));
    }
    group(p)
}

/// Two or three skull-topped warning spikes.
fn spikes_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let n = 2 + (next_u32(rng) % 2) as i32;
    for _ in 0..n {
        let x = rng_range(rng, -0.9, 0.9);
        let z = rng_range(rng, -0.9, 0.9);
        let h = rng_range(rng, 0.8, 1.3);
        p.push(cyl(0.03, h, v(x, h / 2.0, z), ry(rng01(rng)) * rz(rng_range(rng, -0.12, 0.12)), lin(TIMBER_DARK)));
        p.push(bx(0.14, 0.15, 0.15, v(x, h + 0.06, z), lin(BONE)));
    }
    group(p)
}

/// A scatter of old bones.
fn bone_pile_mesh(rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for _ in 0..(3 + next_u32(rng) % 3) {
        let x = rng_range(rng, -0.5, 0.5);
        let z = rng_range(rng, -0.5, 0.5);
        p.push(bxr(
            rng_range(rng, 0.3, 0.6),
            0.07,
            0.08,
            v(x, 0.05, z),
            ry(rng_range(rng, 0.0, TAU)),
            lin(BONE),
        ));
    }
    p.push(bx(0.2, 0.18, 0.18, v(rng_range(rng, -0.3, 0.3), 0.1, rng_range(rng, -0.3, 0.3)), lin(BONE)));
    group(p)
}

/// A hacked-off stump (the orks ate the Blight's living trees long ago).
fn stump_mesh(rng: &mut u32) -> Mesh {
    let r = rng_range(rng, 0.22, 0.34);
    let h = rng_range(rng, 0.35, 0.6);
    group(vec![
        cyl(r, h, v(0.0, h / 2.0, 0.0), rz(rng_range(rng, -0.08, 0.08)), lin(TIMBER_DARK)),
        cyl(r * 0.96, 0.05, v(0.0, h + 0.01, 0.0), Quat::IDENTITY, lin(0x8a6f4a)),
    ])
}

/// A churned mud pool (a flat dark disc pressed into the ground).
fn mud_pool_mesh(rng: &mut u32) -> Mesh {
    let r = rng_range(rng, 0.6, 1.3);
    group(vec![
        cyl(r, 0.03, v(0.0, 0.03, 0.0), Quat::IDENTITY, lin(0x241d15)),
        cyl(r * 0.6, 0.03, v(rng_range(rng, -0.3, 0.3), 0.045, rng_range(rng, -0.3, 0.3)), Quat::IDENTITY, lin(0x1c1610)),
    ])
}

/// The hold's bonfire base: a wide stone ring + a log teepee.
fn bonfire_base_mesh() -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for i in 0..9 {
        let a = (i as f32 / 9.0) * TAU;
        p.push(tinted(
            Sphere::new(0.2).mesh().ico(0).unwrap().translated_by(v(a.cos() * 0.95, 0.1, a.sin() * 0.95)),
            lin(0x6e6e76),
        ));
    }
    for i in 0..4 {
        let a = i as f32 / 4.0 * TAU + 0.4;
        p.push(cyl(
            0.08,
            1.3,
            v(a.cos() * 0.3, 0.5, a.sin() * 0.3),
            ry(-a) * rz(0.5),
            lin(if i % 2 == 0 { 0x7a4a26 } else { 0x3a2a1a }),
        ));
    }
    group(p)
}

/// Flame cones (untinted — the emissive material colours them), `scale`× the camp size.
fn flame_mesh(scale: f32) -> Mesh {
    let outer = Cone { radius: 0.17 * scale, height: 0.55 * scale }
        .mesh()
        .build()
        .translated_by(v(0.0, 0.27 * scale, 0.0));
    let inner = Cone { radius: 0.09 * scale, height: 0.35 * scale }
        .mesh()
        .build()
        .translated_by(v(0.0, 0.2 * scale, 0.0));
    let mut m = outer;
    m.merge(&inner).expect("cones share attributes");
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

// ── Systems ─────────────────────────────────────────────────────────────────────────

/// Bounded random walk inside the walls (or the warlord's fixed beat). No combat, no
/// pathfinding — these orks are furniture that breathes.
fn denizen_brain(time: Res<Time>, mut q: Query<(&mut Denizen, &mut Transform)>) {
    let dt = time.delta_secs().min(0.05);
    let tw = time.elapsed_secs_wrapped();
    const MAX_TURN: f32 = 2.2;
    for (mut d, mut tf) in &mut q {
        d.timer -= dt;
        if !d.moving && d.timer <= 0.0 {
            let target = match d.beat {
                Some(beat) => {
                    d.beat_i = (d.beat_i + 1) % 2;
                    Some(beat[d.beat_i])
                }
                None => {
                    // Reject-sample a nearby standable spot.
                    let mut found = None;
                    for _ in 0..6 {
                        let a = rng_range(&mut d.rng, 0.0, TAU);
                        let r = rng_range(&mut d.rng, 0.8, 2.6);
                        let c = d.anchor + Vec2::new(a.cos() * r, a.sin() * r);
                        if denizen_ok(c, d.pos) {
                            found = Some(c);
                            break;
                        }
                    }
                    found
                }
            };
            match target {
                Some(t) => {
                    d.target = t;
                    d.moving = true;
                }
                None => d.timer = rng_range(&mut d.rng, 0.8, 2.0),
            }
        }
        if d.moving {
            let to = d.target - d.pos;
            let dist = to.length();
            if dist < 0.25 {
                d.moving = false;
                d.timer = if d.beat.is_some() {
                    rng_range(&mut d.rng, 2.5, 6.0)
                } else {
                    rng_range(&mut d.rng, 1.5, 5.0)
                };
            } else {
                let want = to.x.atan2(to.y);
                d.facing += crate::steer::wrap_pi(want - d.facing).clamp(-MAX_TURN * dt, MAX_TURN * dt);
                let fwd = Vec2::new(d.facing.sin(), d.facing.cos());
                let next = d.pos + fwd * d.speed * dt;
                if denizen_ok(next, d.pos) {
                    d.pos = next;
                } else {
                    d.moving = false;
                    d.timer = rng_range(&mut d.rng, 0.8, 2.0);
                }
            }
        }
        let gy = ground_y(d.pos.x, d.pos.y).unwrap_or(tf.translation.y);
        let bob = if d.moving { (tw * d.gait + d.phase).sin().abs() * d.bob } else { 0.0 };
        tf.translation = Vec3::new(d.pos.x, gy + bob, d.pos.y);
        tf.rotation = Quat::from_rotation_y(d.facing);
    }
}

/// A denizen step/target is fine if it stays inside the walls, on footing within one
/// terrace step of where it stands, and out of the registered prop blockers.
fn denizen_ok(next: Vec2, cur: Vec2) -> bool {
    if !inside_walls(next.x, next.y) || crate::blockers::is_blocked(next.x, next.y) {
        return false;
    }
    let (Some(ny), Some(cy)) = (ground_y(next.x, next.y), ground_y(cur.x, cur.y)) else {
        return false;
    };
    (ny - cy).abs() <= 0.55
}

/// Procedural limb swing for the decorative population — the `ork_limbs` look (stride,
/// counter-swinging arms, idle head scan) without the combat arms.
fn denizen_limbs(
    time: Res<Time>,
    denizens: Query<(&Denizen, &Children)>,
    mut parts: Query<(&OrkPart, &mut Transform)>,
) {
    let tw = time.elapsed_secs_wrapped();
    for (d, children) in &denizens {
        let t = tw + d.phase;
        for &child in children {
            let Ok((part, mut tf)) = parts.get_mut(child) else { continue };
            tf.rotation = match part.kind {
                PartKind::Leg(sign) => {
                    let s = if d.moving { (t * d.gait).sin() * d.swing } else { (t * 0.8).sin() * 0.03 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Arm(sign) => {
                    let s = if d.moving { -(t * d.gait).sin() * 0.42 } else { (t * 0.8).sin() * 0.05 };
                    Quat::from_rotation_x(sign * s)
                }
                PartKind::Head => {
                    let bob = (t * 0.5).sin() * 0.06;
                    let scan = if d.moving { 0.0 } else { (t * 0.4).sin() * 0.25 };
                    Quat::from_euler(EulerRot::XYZ, bob, scan, 0.0)
                }
                PartKind::Tail => Quat::IDENTITY,
            };
        }
    }
}

/// Each watchtower plinks a green warp bolt at a hero inside its (short) range.
fn tower_fire(
    time: Res<Time>,
    hero: Res<HeroState>,
    assets: Option<Res<WarpBoltAssets>>,
    mut commands: Commands,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut q: Query<&mut WarTower>,
) {
    let Some(assets) = assets else { return };
    if !hero.alive {
        return;
    }
    let now = time.elapsed_secs();
    for mut t in &mut q {
        if now < t.ready_at
            || Vec2::new(t.muzzle.x, t.muzzle.z).distance(hero.pos) > TOWER_RANGE
        {
            continue;
        }
        t.ready_at = now + TOWER_CD;
        // The release crack (`warp-cast.ogg`), spatial at the crow's-nest muzzle.
        cues.write(crate::audio::AudioCue::WarpCast(t.muzzle));
        commands.spawn((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.mat.clone()),
            Transform::from_translation(t.muzzle),
            WarpBolt { traveled: 0.0, ttl: BOLT_TTL },
            PointLight {
                color: Color::srgb(0.55, 1.0, 0.5),
                intensity: 9_000.0,
                range: 7.0,
                radius: 0.1,
                shadows_enabled: false,
                ..default()
            },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
}

/// Advance the warp bolts — the shaman-bolt recipe (homing, blockable via
/// `PendingHeroDamage`, scorch on burst), tuned to the towers' numbers.
fn step_warp_bolts(
    time: Res<Time>,
    hero: Res<HeroState>,
    fx: Option<Res<crate::player::CombatFx>>,
    mut pending: ResMut<PendingHeroDamage>,
    mut marks: MessageWriter<crate::aftermath::BattleMark>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut WarpBolt, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    let target = Vec3::new(hero.pos.x, hero.y + 1.0, hero.pos.y);
    for (e, mut b, mut tf) in &mut q {
        b.ttl -= dt;
        if !hero.alive || b.ttl <= 0.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        let (out, traveled) =
            advance_bolt(tf.translation, target, BOLT_SPEED * dt, b.traveled, BOLT_MAX_RANGE);
        b.traveled = traveled;
        match out {
            BoltStep::Fly(p) => tf.translation = p,
            BoltStep::Hit => {
                pending.0 += BOLT_DMG;
                if let Some(fx) = &fx {
                    crate::player::spawn_burst(&mut commands, fx, tf.translation, false);
                }
                marks.write(crate::aftermath::BattleMark { at: tf.translation });
                commands.entity(e).try_despawn();
            }
            BoltStep::Fizzle => {
                commands.entity(e).try_despawn();
            }
        }
    }
}

/// Shortest gap between fortress barks (a random slice on top keeps the cadence ragged).
/// Tighter than the camp orks' gap — a whole hold full of orks SHOULD be rowdier.
const FORT_BARK_GAP: f32 = 11.0;
const FORT_BARK_JITTER: f32 = 9.0;
/// A denizen must be within this of the hero for its taunt to be worth playing.
const FORT_EARSHOT: f32 = 30.0;

/// Fortress taunts: the camp orks' battle-bark catalog (`Concept::OrkSpot` — "Where you
/// hide, worm?" etc.), barked off the walls by the nearest denizen whenever the hero
/// lingers in earshot. Same director machinery as `audio::ork`, separate (faster) throttle.
fn fortress_barks(
    time: Res<Time>,
    hero: Res<HeroState>,
    mgr: Res<crate::audio::director::VoiceManager>,
    denizens: Query<(&Denizen, &GlobalTransform)>,
    mut speak: MessageWriter<crate::audio::Speak>,
    mut next_bark: Local<f32>,
    mut rng: Local<u32>,
) {
    let now = time.elapsed_secs();
    if now < *next_bark || !hero.alive || mgr.hero_speaking(now) {
        return;
    }
    let mut best: Option<(Vec3, f32)> = None;
    for (_, gt) in &denizens {
        let p = gt.translation();
        let d = Vec2::new(p.x, p.z).distance(hero.pos);
        if d <= FORT_EARSHOT && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((p, d));
        }
    }
    let Some((pos, _)) = best else { return };
    if *rng == 0 {
        *rng = 0x0f0c_c4a7u32 | 1;
    }
    speak.write(crate::audio::Speak::at(crate::audio::Concept::OrkSpot, pos));
    *next_bark = now + FORT_BARK_GAP + rng01(&mut *rng) * FORT_BARK_JITTER;
}

/// The threshold watch: the hero's first close approach blares the war-horn off the gate,
/// raises agitated shouts inside, and names the place once. Re-approaches re-horn (with a
/// floor) — loitering in range doesn't spam.
fn approach_watch(
    time: Res<Time>,
    hero: Res<HeroState>,
    towers: Query<&WarTower>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    mut was_close: Local<bool>,
    mut next_horn: Local<f32>,
    mut named: Local<bool>,
) {
    if towers.is_empty() {
        // Single-biome views (keys 1–5) have no fortress; don't latch state against nothing.
        *was_close = false;
        return;
    }
    let close = hero.alive && hero.pos.distance(GATE) < THRESHOLD_R;
    let now = time.elapsed_secs();
    if close && !*was_close && now >= *next_horn {
        let gate3 = Vec3::new(GATE.x, 3.0, GATE.y);
        cues.write(crate::audio::AudioCue::FortressHorn(gate3));
        // The warband stirring behind the wall (the roar set, positioned just inside).
        cues.write(crate::audio::AudioCue::OrkRoar(Vec3::new(GATE.x, 1.5, GATE.y + 4.0)));
        *next_horn = now + HORN_GAP;
        if !*named {
            notice.push("Gnashfang Hold", time.elapsed_secs_f64());
            *named = true;
        }
    }
    *was_close = close;
}

/// Night-wave tie-in: every fortress fire (bonfire, torches, the spire's warp brazier)
/// swells while a wave is marching and settles back at dawn. Pure ambience.
fn siege_flare(
    time: Res<Time>,
    siege: Option<Res<crate::siege::Siege>>,
    mut q: Query<(&FortressFlame, &mut FireLight)>,
) {
    let hot = siege.is_some_and(|s| matches!(s.phase, crate::siege::GamePhase::Wave));
    let mult = if hot { 1.9 } else { 1.0 };
    let k = (time.delta_secs() * 1.5).min(1.0);
    for (ff, mut fl) in &mut q {
        fl.base += (ff.base * mult - fl.base) * k;
    }
}

/// Scale-wobble for the non-`Flicker` flames (hall torches + the warp brazier).
fn wobble_flames(time: Res<Time>, mut q: Query<(&Wobble, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (w, mut tf) in &mut q {
        let sx = 1.0 + (t * 7.0 + w.phase).sin() * 0.12 + (t * 14.3 + w.phase).sin() * 0.06;
        let sy = 1.0 + (t * 9.5 + w.phase).sin() * 0.22;
        tf.scale = Vec3::new(sx, sy, sx);
    }
}

/// Rising smoke (the camps' drift recipe).
fn drift_smoke(time: Res<Time>, mut q: Query<(&FortSmoke, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (s, mut tf) in &mut q {
        let cycle = (t * s.speed + s.phase).rem_euclid(1.0);
        tf.translation.x = s.base.x + (t * 0.7 + s.phase * 6.0).sin() * 0.22 * cycle;
        tf.translation.z = s.base.z + (t * 0.6 + s.phase * 6.0).cos() * 0.22 * cycle;
        tf.translation.y = s.base.y + cycle * 2.2;
        let sc = (0.14 + cycle * 0.55) * (1.0 - cycle).max(0.0);
        tf.scale = Vec3::splat(sc.max(0.001));
    }
}

/// Low preset: hide half the population + the smoke (the structures and fires stay — the
/// hold must still read from the shore).
fn quality_lod(
    quality: Option<Res<GraphicsQuality>>,
    mut denizens: Query<(&Denizen, &mut Visibility)>,
    mut smoke: Query<&mut Visibility, (With<FortSmoke>, Without<Denizen>)>,
) {
    let Some(quality) = quality else { return };
    if !quality.is_changed() {
        return;
    }
    let low = *quality == GraphicsQuality::Low;
    for (d, mut vis) in &mut denizens {
        if d.lod_cull {
            *vis = if low { Visibility::Hidden } else { Visibility::Visible };
        }
    }
    for mut vis in &mut smoke {
        *vis = if low { Visibility::Hidden } else { Visibility::Visible };
    }
}

// ── Mesh + RNG helpers (the camps.rs idiom, local copy) ─────────────────────────────

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("fortress parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), c)
}
fn cyl(r: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(6).build().rotated_by(rot).translated_by(off), c)
}

fn next_u32(s: &mut u32) -> u32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    t ^ (t >> 14)
}
fn rng01(s: &mut u32) -> f32 {
    next_u32(s) as f32 / 4_294_967_296.0
}
fn rng_range(s: &mut u32, lo: f32, hi: f32) -> f32 {
    lo + rng01(s) * (hi - lo)
}
