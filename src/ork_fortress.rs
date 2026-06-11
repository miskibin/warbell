//! **Gnashfang Hold** — the ork seat of power, on its own islet beyond the swamp coast,
//! SOUTH of the playable grid. Pure world-dressing with one gameplay tooth: the watchtowers
//! fire real (blockable) warp bolts at a hero who walks the causeway up to the gate.
//!
//! Spec: `docs/superpowers/specs/2026-06-11-ork-fortress-design.md`. The hold is a crude
//! timber stronghold — low spiked palisade (~1.5× hero height, so the camera reads the
//! interior over it), five leaning watchtowers, a shut gate with broken-bridge stubs across
//! the strait, a hulking great hall and a crooked spire crowned in iron with a green warp
//! brazier — peopled by decorative orks (no [`crate::orks::Ork`] brain: untargetable, never
//! leave) and an oversized pacing warlord.
//!
//! Containment is free: `player::movement::footing()` is `None` off-grid, so the sea strait
//! and the islet itself are unwalkable; the only land approach is the causeway carved by
//! [`neck_land_base`] into `worldmap::classify`, and the gate wall (with real blockers)
//! stands exactly at the walkable boundary as the physical excuse.
//!
//! Audio rides existing rails: the bonfire is tagged `camps::Flicker`, so the ambience
//! module hangs its spatial campfire loop + war-drum sink on it automatically; the war-horn
//! is a baked synth sting (`Sting::WarHorn`) blared spatially from the gate on the hero's
//! first close approach. During a night wave every fortress fire flares hotter.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshBuilder, PrimitiveTopology};
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
use crate::worldmap::{self, GROUND_STEP, GX, GZ, MAP_SCALE};

// ── Layout (world space; the grid's south edge is z = +81) ──────────────────────────

/// Fortress centre — the islet blob and the "inside the walls" tests key off this.
const CENTRE: Vec2 = Vec2::new(12.0, 103.0);
/// Islet blob radii (wobbled ellipse around [`CENTRE`]) — generously larger than the wall
/// ring so the hold sits on a real landmass (a wooded dead-tree apron all around), not a
/// dinner-plate islet.
const BLOB_RX: f32 = 34.0;
const BLOB_RZ: f32 = 30.0;
/// The gate wall line — just south of the last walkable row, straddling the grid seam so
/// the hero's footing stops him exactly at the timber.
const FRONT_Z: f32 = 80.9;
/// Gate centre (the war-horn sounds from here; the threshold test measures to it).
const GATE: Vec2 = Vec2::new(12.0, FRONT_Z);
/// North end of the causeway (where it merges into the swamp's own land).
const NECK_Z0: f32 = 68.0;
/// Mirrors `worldmap::SEA_Y` (private there) — islet cliff walls drop to the sea plane.
const SEA_Y: f32 = -0.4;

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

// ── Public geometry queries (worldmap/camps call these during generation) ───────────

/// BASE-space hook for `worldmap::classify`: the causeway — a flat swamp tongue from the
/// south shore to the grid edge, x ≈ 5..19 with a frayed edge so it doesn't read stamped.
pub fn neck_land_base(bx: f32, bz: f32) -> bool {
    on_neck_world(bx * MAP_SCALE - GX, bz * MAP_SCALE - GZ)
}

/// World-space causeway test (also camps' placement exclusion).
pub fn on_neck_world(wx: f32, wz: f32) -> bool {
    if !(NECK_Z0..=81.01).contains(&wz) {
        return false;
    }
    let fray = (wz * 0.55 + 1.0).sin() * 1.0 + (wz * 1.3 + 3.0).sin() * 0.5;
    wx >= 5.0 + fray && wx <= 19.0 + fray * 0.5
}

/// Scatter keep-out: the gate approach (causeway + a fringe) stays clear of swamp props so
/// the walk up to the wall — and the towers' line of fire — reads clean.
pub fn on_gate_approach(wx: f32, wz: f32) -> bool {
    (3.0..=21.0).contains(&wx) && (71.5..=81.5).contains(&wz)
}

/// Water keep-out for the background sailboats: the hold's whole bay (islet + a wake
/// margin). `boats::boat_drift` bounces a hull that would drift in here.
pub fn boat_keepout(wx: f32, wz: f32) -> bool {
    Vec2::new(wx, wz).distance(CENTRE) < BLOB_RX + 12.0
}

// ── Islet heightfield (the fortress's own off-grid terrain) ─────────────────────────

/// Height class at world `(wx, wz)` on the islet: 1 = shore plain (y 0), 2 = the great-hall
/// terrace (y 0.5), 3 = the spire pad (y 1.0). `None` = open sea.
fn islet_class(wx: f32, wz: f32) -> Option<i32> {
    let dx = (wx - CENTRE.x) / BLOB_RX;
    let dz = (wz - CENTRE.y) / BLOB_RZ;
    let ang = dz.atan2(dx);
    let wob = (ang * 3.0 + 1.2).sin() * 0.045 + (ang * 5.0 - 0.4).sin() * 0.035;
    let inside_blob = (dx * dx + dz * dz).sqrt() < 1.0 + wob;
    // The neck corridor guarantees land under the gate wall + its towers even where the
    // wobbled blob pulls shy of the grid seam.
    let in_neck = (1.0..=23.0).contains(&wx) && (80.5..=88.0).contains(&wz);
    if !inside_blob && !in_neck {
        return None;
    }
    let d_terrace = (wx - 12.5).hypot(wz - 108.0);
    let d_pad = (wx - 13.0).hypot(wz - 115.0);
    Some(if d_pad < 2.8 {
        3
    } else if d_terrace < 10.5 {
        2
    } else {
        1
    })
}

fn islet_y(wx: f32, wz: f32) -> Option<f32> {
    islet_class(wx, wz).map(|c| (c - 1) as f32 * GROUND_STEP)
}

/// Footing for fortress denizens (islet first, then the grid — the causeway is grid land).
fn ground_y(wx: f32, wz: f32) -> Option<f32> {
    islet_y(wx, wz).or_else(|| worldmap::ground_at_world(wx, wz))
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
            (denizen_brain, tower_fire, step_warp_bolts, approach_watch, siege_flare, fortress_barks)
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
/// gate gap at x 9..15; everything else stands off-grid over its own islet.
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

/// Wattle huts: (centre, radius).
const HUTS: [(Vec2, f32); 8] = [
    (Vec2::new(0.0, 90.5), 2.2),
    (Vec2::new(24.0, 92.0), 2.0),
    (Vec2::new(28.5, 107.0), 2.4),
    (Vec2::new(21.5, 115.0), 2.2),
    (Vec2::new(1.0, 115.5), 2.0),
    (Vec2::new(-5.5, 96.5), 2.2),
    (Vec2::new(21.5, 99.5), 1.9),
    (Vec2::new(2.5, 104.0), 2.0),
];

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

#[allow(clippy::too_many_arguments)]
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
    terrain_mats: &mut Assets<crate::terrain::TerrainMaterial>,
) {
    // ── Islet ground: trampled black mud fading to swampy rim, terraced like the grid ──
    let detail = GroundDetail {
        scale: 0.24,
        strength: 0.55,
        variation: 0.75,
        seed: 7.0,
        dark: 0x2e2419,
        base: 0x4a3b2a,
        light: 0x6e5c40,
        grain: 0.8,
        streak: 0.45,
    };
    let ground_mat = crate::terrain::make_material(&detail, 0.97, images, terrain_mats);
    commands.spawn((
        Mesh3d(meshes.add(build_islet_mesh())),
        MeshMaterial3d(ground_mat),
        Transform::default(),
        BiomeEntity,
    ));

    // Shared vertex-colour material for every timber/bone prop — same batching contract as
    // the camps, but with a neutral grime-grain detail texture multiplied over the vertex
    // colours (the primitives' own UVs sample it), so the hold reads rough and dirty
    // instead of flat-shaded clean.
    let grain = GroundDetail {
        scale: 1.0,
        strength: 0.9,
        variation: 0.5,
        seed: 13.0,
        dark: 0x8e887e,
        base: 0xc2bcb0,
        light: 0xf2ece0,
        grain: 0.9,
        streak: 0.7,
    };
    let (grain_img, _) = crate::terrain::detail_image(&grain);
    let grain_tex = images.add(grain_img);
    let mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(grain_tex),
        perceptual_roughness: 0.95,
        ..default()
    });
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
    for i in 0..RING.len() {
        let a = RING[i];
        let b = RING[(i + 1) % RING.len()];
        if a == Vec2::new(9.0, FRONT_Z) && b == Vec2::new(15.0, FRONT_Z) {
            continue; // the gate fills this gap
        }
        spawn_solid(commands, meshes, &mat, palisade_segment(a, b, &mut rng), Vec3::ZERO, Quat::IDENTITY);
    }
    // Hero-side blockers: only the front run is reachable; the timber is the wall he feels.
    crate::blockers::add_obb(5.5, FRONT_Z, 3.6, 0.45, 0.0);
    crate::blockers::add_obb(18.5, FRONT_Z, 3.6, 0.45, 0.0);
    crate::blockers::add_obb(12.0, FRONT_Z, 3.2, 0.5, 0.0); // the shut gate itself

    // ── The shut gate ──
    spawn_solid(commands, meshes, &mat, gate_mesh(), at(GATE), Quat::IDENTITY);

    // ── Watchtowers (leaning, each its own tilt/yaw) + fire emitters + banners ──
    for (i, t) in TOWERS.iter().enumerate() {
        let yaw = rng_range(&mut rng, -0.3, 0.3);
        let lean = rng_range(&mut rng, 0.02, 0.05)
            * if next_u32(&mut rng) % 2 == 0 { 1.0 } else { -1.0 };
        let pos = at(*t);
        let rot = ry(yaw) * Quat::from_rotation_z(lean);
        spawn_solid(commands, meshes, &mat, tower_mesh(&mut rng), pos, rot);
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
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(hall_pos).with_scale(Vec3::splat(HALL_SCALE)),
        BiomeEntity,
    ));
    crate::blockers::add_obb(HALL_AT.x, HALL_AT.y, 5.8 * HALL_SCALE, 4.3 * HALL_SCALE, 0.0);
    // Doorway glow + flanking torches (warm light pooling out of the dark hall mouth).
    commands.spawn((
        Mesh3d(meshes.add(bx(2.0 * HALL_SCALE, 2.4 * HALL_SCALE, 0.05, Vec3::ZERO, lin(0xffffff)))),
        MeshMaterial3d(glow_mat),
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
        MeshMaterial3d(mat.clone()),
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

    // ── Wattle huts ──
    for (p, r) in HUTS {
        spawn_solid(commands, meshes, &mat, hut_mesh(r, &mut rng), at(p), ry(rng_range(&mut rng, 0.0, TAU)));
        crate::blockers::add(p.x, p.y, r + 0.3);
    }

    // ── Prisoner cage (bigger than a camp's; the hold hoards captives) ──
    spawn_solid(commands, meshes, &mat, cage_mesh(), at(CAGE_AT), ry(0.4));
    crate::blockers::add_obb(CAGE_AT.x, CAGE_AT.y, 1.25, 1.25, 0.4);

    // ── War totems: one OUTSIDE on the causeway and two inside, all glaring at the
    //    castle (the camps' "gaze points home" rule, scaled up). ──
    for tp in [Vec2::new(8.5, 77.0), Vec2::new(24.0, 90.0), Vec2::new(0.0, 118.0)] {
        let yaw = (-tp.x).atan2(-tp.y);
        spawn_solid(commands, meshes, &mat, totem_mesh(&mut rng), at(tp), ry(yaw));
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

    // ── The rotted, broken bridge across the strait (nobody crosses — either way) ──
    spawn_solid(commands, meshes, &mat, bridge_stub_mesh(3.4, &mut rng), Vec3::new(0.2, 0.0, 76.8), ry(0.08));
    spawn_solid(commands, meshes, &mat, bridge_stub_mesh(2.2, &mut rng), Vec3::new(1.5, 0.0, 83.8), ry(PI - 0.06));
    spawn_solid(commands, meshes, &mat, bridge_debris_mesh(), Vec3::new(0.9, 0.0, 81.2), ry(0.5));

    // ── Trampled-ground dressing: bones, stumps, mud pools inside the walls ──
    let keep_out: Vec<(Vec2, f32)> = TOWERS
        .iter()
        .map(|t| (*t, 2.0))
        .chain(HUTS.iter().map(|(p, r)| (*p, r + 1.0)))
        .chain([
            (HALL_AT, 7.0 * HALL_SCALE),
            (SPIRE_AT, 3.6),
            (BONFIRE_AT, 2.4),
            (CAGE_AT, 2.2),
        ])
        .collect();
    let mut placed = 0;
    let mut tries = 0;
    while placed < 34 && tries < 400 {
        tries += 1;
        let ang = rng_range(&mut rng, 0.0, TAU);
        let r = rng_range(&mut rng, 4.0, 20.0);
        let p = CENTRE + Vec2::new(ang.cos() * r, ang.sin() * r * 0.9);
        if islet_class(p.x, p.y).is_none()
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

    // ── The dead-wood apron: gnarled bare trees crowd the land OUTSIDE the walls, so the
    //    hold reads as a fortress hacked out of a blighted swamp forest, not a bare disc.
    //    Three tint variants share mesh handles so the whole stand batches. ──
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
    let mut trees_placed = 0;
    let mut tree_tries = 0;
    while trees_placed < 140 && tree_tries < 900 {
        tree_tries += 1;
        let ang = rng_range(&mut rng, 0.0, TAU);
        let rr = rng_range(&mut rng, 21.5, 34.5);
        let p = CENTRE + Vec2::new(ang.cos() * rr * 1.12, ang.sin() * rr * 0.9);
        // Outside the wall ring (+ margin), on islet land, clear of the causeway approach.
        if p.y < 82.5
            || p.distance(CENTRE) < 22.4
            || islet_class(p.x, p.y).is_none()
            || on_gate_approach(p.x, p.y)
        {
            continue;
        }
        let v = (next_u32(&mut rng) % 3) as usize;
        commands.spawn((
            Mesh3d(tree_meshes[v].clone()),
            MeshMaterial3d(ork_mat.clone()),
            Transform {
                translation: at(p),
                rotation: ry(rng_range(&mut rng, 0.0, TAU)),
                scale: Vec3::splat(rng_range(&mut rng, 1.0, 2.0)),
            },
            BiomeEntity,
        ));
        trees_placed += 1;
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

    info!("ork fortress: Gnashfang Hold built at {:.0},{:.0}", CENTRE.x, CENTRE.y);
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

// ── Islet terrain mesh (the worldmap's terraced-quads recipe over the local field) ───

const MUD_HEART: u32 = 0x403425;
const MUD_MID: u32 = 0x53432e;
const MUD_RIM: u32 = 0x565a38;
const MUD_SHORE: u32 = 0x70614a;

fn lin3(c: u32) -> [f32; 3] {
    let l = lin(c);
    [l[0], l[1], l[2]]
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Ground colour: trampled black mud at the heart, swampy mud at the rim, sandy mud at the
/// waterline, with a sin-mix jitter so it never reads flat.
fn mud_color(wx: f32, wz: f32) -> [f32; 4] {
    let d = (((wx - CENTRE.x) / BLOB_RX).powi(2) + ((wz - CENTRE.y) / BLOB_RZ).powi(2)).sqrt();
    let n = ((wx * 0.7 + 1.3).sin() * (wz * 0.62 - 0.8).cos()
        + (wx * 0.23 + wz * 0.31).sin() * 0.5)
        * 0.5;
    let c = if d < 0.4 {
        mix3(lin3(MUD_HEART), lin3(MUD_MID), d / 0.4)
    } else if d < 0.75 {
        mix3(lin3(MUD_MID), lin3(MUD_RIM), (d - 0.4) / 0.35)
    } else {
        mix3(lin3(MUD_RIM), lin3(MUD_SHORE), (d - 0.75) / 0.25)
    };
    let j = 1.0 + n * 0.10;
    [c[0] * j, c[1] * j, c[2] * j, 1.0]
}

fn build_islet_mesh() -> Mesh {
    const X0: f32 = -23.0;
    const Z0: f32 = 81.0; // the grid's south edge — the causeway tiles end here
    const NX: i32 = 70;
    const NZ: i32 = 54;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut quad = |p: [[f32; 3]; 4], n: [f32; 3], c: [[f32; 4]; 4]| {
        let b = positions.len() as u32;
        for k in 0..4 {
            positions.push(p[k]);
            normals.push(n);
            colors.push(c[k]);
        }
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    };

    let class_at = |ix: i32, iz: i32| -> Option<i32> {
        if !(0..NX).contains(&ix) || !(0..NZ).contains(&iz) {
            return None;
        }
        islet_class(X0 + ix as f32 + 0.5, Z0 + iz as f32 + 0.5)
    };

    for iz in 0..NZ {
        for ix in 0..NX {
            let Some(h) = class_at(ix, iz) else { continue };
            let top = (h - 1) as f32 * GROUND_STEP;
            let wx = X0 + ix as f32;
            let wz = Z0 + iz as f32;

            quad(
                [[wx, top, wz], [wx + 1.0, top, wz], [wx + 1.0, top, wz + 1.0], [wx, top, wz + 1.0]],
                [0.0, 1.0, 0.0],
                [
                    mud_color(wx, wz),
                    mud_color(wx + 1.0, wz),
                    mud_color(wx + 1.0, wz + 1.0),
                    mud_color(wx, wz + 1.0),
                ],
            );

            // Cliff walls down to lower neighbours / the sea — graded lip→base mud.
            let tc = mud_color(wx + 0.5, wz + 0.5);
            let wall_top = [tc[0] * 0.80, tc[1] * 0.78, tc[2] * 0.76, 1.0];
            let wall_bot = [tc[0] * 0.56, tc[1] * 0.54, tc[2] * 0.52, 1.0];
            let wc = [wall_bot, wall_bot, wall_top, wall_top];
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nh_top = if iz == 0 && dz == -1 {
                    // North seam: where the causeway's grid tiles continue, the ground is
                    // continuous — no wall; elsewhere the islet edge drops to the sea.
                    if worldmap::ground_at_world(wx + 0.5, Z0 - 0.5).is_some() {
                        continue;
                    } else {
                        SEA_Y
                    }
                } else {
                    match class_at(ix + dx, iz + dz) {
                        Some(nh) => (nh - 1) as f32 * GROUND_STEP,
                        None => SEA_Y,
                    }
                };
                if top <= nh_top + 1e-4 {
                    continue;
                }
                let (e0, e1, n): ([f32; 2], [f32; 2], [f32; 3]) = match (dx, dz) {
                    (1, 0) => ([wx + 1.0, wz], [wx + 1.0, wz + 1.0], [1.0, 0.0, 0.0]),
                    (-1, 0) => ([wx, wz + 1.0], [wx, wz], [-1.0, 0.0, 0.0]),
                    (0, 1) => ([wx + 1.0, wz + 1.0], [wx, wz + 1.0], [0.0, 0.0, 1.0]),
                    _ => ([wx, wz], [wx + 1.0, wz], [0.0, 0.0, -1.0]),
                };
                quad(
                    [
                        [e0[0], nh_top, e0[1]],
                        [e1[0], nh_top, e1[1]],
                        [e1[0], top, e1[1]],
                        [e0[0], top, e0[1]],
                    ],
                    n,
                    wc,
                );
            }
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
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
const WATTLE: u32 = 0x6e5a3e;
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

/// The shut gate: heavy posts, skull-topped lintel, two studded plank doors. Faces −Z
/// (the island); authored about its own origin.
fn gate_mesh() -> Mesh {
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
    for sx in [-1.0f32, 1.0] {
        // A door panel: planks + iron bands + outward spike studs.
        p.push(bx(2.95, 3.0, 0.16, v(sx * 1.5, 1.5, 0.0), lin(TIMBER)));
        for i in 0..4 {
            p.push(bx(0.10, 2.9, 0.03, v(sx * 1.5 - 1.2 + i as f32 * 0.8, 1.5, -0.09), lin(TIMBER_DARK)));
        }
        for by in [0.7f32, 2.2] {
            p.push(bx(2.85, 0.14, 0.05, v(sx * 1.5, by, -0.11), lin(IRON)));
        }
        for (ux, uy) in [(-0.8f32, 0.9f32), (0.8, 0.9), (-0.8, 2.0), (0.8, 2.0)] {
            p.push(tinted(
                Cone { radius: 0.07, height: 0.22 }
                    .mesh()
                    .build()
                    .rotated_by(rx(-FRAC_PI_2))
                    .translated_by(v(sx * 1.5 + ux, uy, -0.2)),
                lin(IRON),
            ));
        }
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

/// A round wattle hut: mud-daub wall, ragged hide cone roof, black door hole.
fn hut_mesh(r: f32, rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let shade = [WATTLE, 0x655241, 0x78654c][(next_u32(rng) % 3) as usize];
    p.push(cyl(r, 1.5, v(0.0, 0.75, 0.0), Quat::IDENTITY, lin(shade)));
    p.push(tinted(
        Cone { radius: r + 0.55, height: 1.25 }
            .mesh()
            .build()
            .translated_by(v(0.0, 2.12, 0.0)),
        lin(if rng01(rng) < 0.5 { HIDE } else { HIDE_DARK }),
    ));
    for i in 0..7 {
        let a = i as f32 / 7.0 * TAU + rng01(rng);
        p.push(bxr(
            0.5,
            0.05,
            0.35,
            v(a.cos() * (r + 0.35), 1.42 + rng_range(rng, -0.08, 0.05), a.sin() * (r + 0.35)),
            ry(-a) * rz(rng_range(rng, -0.3, 0.3)),
            lin(HIDE_DARK),
        ));
    }
    p.push(bx(0.75, 1.05, 0.2, v(0.0, 0.52, -r), lin(0x171008))); // door hole
    p.push(cyl(0.05, 0.7, v(0.0, 2.9, 0.0), Quat::IDENTITY, lin(TIMBER_DARK)));
    if rng01(rng) < 0.6 {
        p.push(bx(0.16, 0.15, 0.15, v(0.0, 1.25, -r - 0.05), lin(BONE))); // skull over the door
    }
    group(p)
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
/// rescue; that's the story it tells).
fn cage_mesh() -> Mesh {
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

/// A hacked-off stump (the orks ate the islet's trees long ago).
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

/// One broken bridge stub: a plank deck on tilted poles running along +Z from the origin,
/// ending in snapped, sagging planks. The strait's depth is faked — poles just sink to the
/// sea plane.
fn bridge_stub_mesh(len: f32, rng: &mut u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let n = (len / 0.55).ceil() as i32;
    for i in 0..n {
        let z = i as f32 * 0.55;
        let last = i >= n - 2;
        let sag = if last { (i - (n - 3)) as f32 * -0.16 } else { 0.0 };
        let tilt = if last { rng_range(rng, 0.15, 0.5) } else { rng_range(rng, -0.04, 0.04) };
        p.push(bxr(
            1.3,
            0.07,
            0.5,
            v(rng_range(rng, -0.05, 0.05), 0.35 + sag, z),
            rz(tilt),
            lin(if i % 3 == 0 { TIMBER_PALE } else { TIMBER }),
        ));
    }
    for sz in [0.3f32, len * 0.6] {
        for sx in [-0.55f32, 0.55] {
            p.push(cyl(
                0.08,
                1.1,
                v(sx, -0.15, sz),
                rz(rng_range(rng, -0.1, 0.1)),
                lin(TIMBER_DARK),
            ));
        }
    }
    group(p)
}

/// Loose planks + a snapped pile adrift where the bridge gave way.
fn bridge_debris_mesh() -> Mesh {
    group(vec![
        bxr(1.1, 0.06, 0.4, v(0.0, SEA_Y + 0.06, 0.0), ry(0.7) * rz(0.12), lin(TIMBER)),
        bxr(0.9, 0.06, 0.35, v(0.8, SEA_Y + 0.05, 0.9), ry(-0.4), lin(TIMBER_PALE)),
        cyl(0.07, 1.0, v(-0.5, SEA_Y + 0.25, 0.4), rz(0.35), lin(TIMBER_DARK)),
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
