//! **Battle aftermath** — persistent traces of the night's fighting, so the morning after
//! a wave the ground still *tells the story*: dark blood stains where orks fell, dropped
//! clubs / stuck spears / lost helmets among them, and scorch marks where shaman bolts
//! burst. Environmental storytelling: the combat-fx splats fade in seconds (`combat_fx`),
//! but these stay through the following day and are swept only when the NEXT assault
//! begins (Prep→Wave), so each day you walk a battlefield that reads as last night.
//!
//! Bounded: a FIFO cap reaps the oldest marks so a marathon run can't litter thousands of
//! entities. Everything is tagged `BiomeEntity` too, so world rebuilds wipe it like any
//! other dressing.

use std::collections::VecDeque;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::dying::Dying;
use crate::orks::Ork;
use crate::palette::lin;
use crate::siege::{GamePhase, Siege};

/// Most aftermath entities allowed at once (oldest reaped first).
const MAX_MARKS: usize = 160;
/// Chance an ork's fall also drops a piece of gear next to the stain.
const DROP_CHANCE: f32 = 0.45;
/// Blood stains fade out far quicker than the day-long gear/scorch traces: a stain reads right
/// after the kill, then is gone within ~30s (the gear + scorches still carry the morning-after
/// story). Hold full opacity for `BLOOD_HOLD`s, then fade over `BLOOD_FADE`s and despawn.
const BLOOD_HOLD: f32 = 12.0;
const BLOOD_FADE: f32 = 18.0;

const TAU: f32 = std::f32::consts::TAU;
const FRAC_PI_2: f32 = std::f32::consts::FRAC_PI_2;

// Palette (aftermath-only tones).
const BLOOD_DRY: u32 = 0x3c120c; // dried near-black crimson
const CHAR: u32 = 0x16130f; // scorched earth
const CHAR_RIM: u32 = 0x2e2218; // singed rim around a scorch
const CLUB_WOOD: u32 = 0x4b3724;
const CLUB_KNOB: u32 = 0x33271a;
const SPEAR_SHAFT: u32 = 0x3a2a1a;
const IRON: u32 = 0x565a62;
const IRON_DARK: u32 = 0x3c4046;

/// A non-ork battle mark to drop (today: shaman-bolt scorches from `projectile.rs`).
#[derive(Message)]
pub struct BattleMark {
    pub at: Vec3,
}

#[derive(Component)]
struct Aftermath;

/// A blood stain that fades out on its own (unlike gear/scorches, which persist till the next
/// assault). Carries its OWN material handle — the stain's alpha lives in the mesh vertex colour,
/// so [`fade_blood`] dims this per-stain material's `base_color` alpha (which multiplies it) to
/// ramp the whole splat out, then despawns it.
#[derive(Component)]
struct BloodStain {
    born: f32,
    mat: Handle<StandardMaterial>,
}

/// FIFO of live marks for the cap.
#[derive(Resource, Default)]
struct MarkLog {
    entities: VecDeque<Entity>,
    rng: u32,
}

/// Prebuilt mark meshes + the two materials (solid gear vs alpha-blended ground decals).
#[derive(Resource)]
struct MarkAssets {
    solid_mat: Handle<StandardMaterial>,
    decal_mat: Handle<StandardMaterial>,
    stain: Handle<Mesh>,
    scorch: Handle<Mesh>,
    club: Handle<Mesh>,
    spear: Handle<Mesh>,
    helmet: Handle<Mesh>,
}

pub struct AftermathPlugin;

impl Plugin for AftermathPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BattleMark>()
            .init_resource::<MarkLog>()
            .add_systems(Startup, setup_assets)
            .add_systems(
                Update,
                (mark_ork_falls, mark_scorches, fade_blood, sweep_at_next_assault)
                    .run_if(in_state(crate::game_state::Modal::None)),
            );
    }
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Gear shares the standard white vertex-colour recipe so it batches with other props;
    // the ground decals need alpha (a hard opaque disc reads stamped-on, not soaked-in).
    let solid_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    let decal_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    commands.insert_resource(MarkAssets {
        solid_mat,
        decal_mat,
        stain: meshes.add(stain_mesh(BLOOD_DRY, 0.5, 0.78)),
        scorch: meshes.add(scorch_mesh()),
        club: meshes.add(club_mesh()),
        spear: meshes.add(spear_mesh()),
        helmet: meshes.add(helmet_mesh()),
    });
}

// ── Spawners ─────────────────────────────────────────────────────────────────────

/// Every ork that starts its death fade leaves a dried stain (and sometimes gear). Runs on
/// `Added<Dying>` so it fires exactly once per ork, whoever landed the kill (hero, guard,
/// tower, rival faction).
fn mark_ork_falls(
    time: Res<Time>,
    fallen: Query<&GlobalTransform, (With<Ork>, Added<Dying>)>,
    assets: Option<Res<MarkAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut log: ResMut<MarkLog>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let now = time.elapsed_secs();
    for gt in &fallen {
        let p = gt.translation();
        let yaw = rng01(&mut log.rng) * TAU;
        let s = 0.8 + rng01(&mut log.rng) * 0.6;
        // Squash one axis so no two stains read as the same stamped circle.
        let squash = 0.6 + rng01(&mut log.rng) * 0.4;
        let stain_tf = Transform {
            translation: Vec3::new(p.x, p.y + 0.025, p.z),
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::new(s, 1.0, s * squash),
        };
        // Each stain gets its own (alpha-blended) material so `fade_blood` can dim it solo.
        let stain_mat = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let e = spawn_mark(&mut commands, &mut log, assets.stain.clone(), stain_mat.clone(), stain_tf);
        commands.entity(e).try_insert(BloodStain { born: now, mat: stain_mat });
        if rng01(&mut log.rng) < DROP_CHANCE {
            let mesh = match (rng01(&mut log.rng) * 3.0) as u32 {
                0 => assets.club.clone(),
                1 => assets.spear.clone(),
                _ => assets.helmet.clone(),
            };
            let a = rng01(&mut log.rng) * TAU;
            let d = 0.5 + rng01(&mut log.rng) * 0.6;
            let gear_yaw = rng01(&mut log.rng) * TAU;
            let gear_tf = Transform {
                translation: Vec3::new(p.x + a.cos() * d, p.y, p.z + a.sin() * d),
                rotation: Quat::from_rotation_y(gear_yaw),
                scale: Vec3::ONE,
            };
            spawn_mark(&mut commands, &mut log, mesh, assets.solid_mat.clone(), gear_tf);
        }
    }
}

/// Scorch the ground where a shaman bolt burst (written by `projectile.rs`).
fn mark_scorches(
    mut marks: MessageReader<BattleMark>,
    assets: Option<Res<MarkAssets>>,
    mut log: ResMut<MarkLog>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    for m in marks.read() {
        let ground = crate::worldmap::ground_at_world(m.at.x, m.at.z).unwrap_or(m.at.y - 1.0);
        let s = 0.8 + rng01(&mut log.rng) * 0.5;
        let yaw = rng01(&mut log.rng) * TAU;
        let squash = 0.75 + rng01(&mut log.rng) * 0.25;
        let tf = Transform {
            translation: Vec3::new(m.at.x, ground + 0.03, m.at.z),
            rotation: Quat::from_rotation_y(yaw),
            scale: Vec3::new(s, 1.0, s * squash),
        };
        spawn_mark(&mut commands, &mut log, assets.scorch.clone(), assets.decal_mat.clone(), tf);
    }
}

fn spawn_mark(
    commands: &mut Commands,
    log: &mut MarkLog,
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    tf: Transform,
) -> Entity {
    let e = commands
        .spawn((Mesh3d(mesh), MeshMaterial3d(mat), tf, NotShadowCaster, Aftermath, BiomeEntity))
        .id();
    log.entities.push_back(e);
    while log.entities.len() > MAX_MARKS {
        if let Some(old) = log.entities.pop_front() {
            commands.entity(old).try_despawn();
        }
    }
    e
}

/// Ramp each blood stain's per-stain material alpha down after [`BLOOD_HOLD`]s and despawn it once
/// fully faded — so blood clears within ~30s while gear/scorches keep the day-long battlefield read.
fn fade_blood(
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    stains: Query<(Entity, &BloodStain)>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    for (e, b) in &stains {
        let age = now - b.born;
        if age < BLOOD_HOLD {
            continue;
        }
        let f = ((age - BLOOD_HOLD) / BLOOD_FADE).min(1.0); // 0 → 1 across the fade window
        if f >= 1.0 {
            commands.entity(e).try_despawn();
        } else if let Some(m) = materials.get_mut(&b.mat) {
            m.base_color = m.base_color.with_alpha(1.0 - f); // multiplies the mesh's vertex alpha
        }
    }
}

/// Sweep all marks the moment the NEXT assault begins (Prep→Wave), so last night's
/// battlefield stays readable through the whole following day, then yields to fresh traces.
fn sweep_at_next_assault(
    siege: Option<Res<Siege>>,
    mut prev: Local<Option<GamePhase>>,
    mut log: ResMut<MarkLog>,
    mut commands: Commands,
) {
    let Some(siege) = siege else { return };
    let was = prev.replace(siege.phase);
    if siege.phase == GamePhase::Wave && was != Some(GamePhase::Wave) {
        for e in log.entities.drain(..) {
            commands.entity(e).try_despawn();
        }
    }
}

// ── Mark meshes (contract: base y=0, ATTRIBUTE_COLOR, flat-shaded merge) ───────────

fn tint(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

fn tint_a(mut m: Mesh, hex: u32, alpha: f32) -> Mesh {
    let mut c = lin(hex);
    c[3] = alpha;
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

fn merged_flat(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("aftermath parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

/// Flat ground disc (lying in XZ, normal up) — the stain / scorch base shape.
fn disc(r: f32, res: u32) -> Mesh {
    Circle::new(r).mesh().resolution(res).build().rotated_by(Quat::from_rotation_x(-FRAC_PI_2))
}

fn stain_mesh(hex: u32, r: f32, alpha: f32) -> Mesh {
    // A main pool + two satellite spatters so it reads soaked, not stamped.
    merged_flat(vec![
        tint_a(disc(r, 12), hex, alpha),
        tint_a(disc(r * 0.35, 8).translated_by(Vec3::new(r * 0.95, 0.005, r * 0.25)), hex, alpha * 0.9),
        tint_a(disc(r * 0.22, 8).translated_by(Vec3::new(-r * 0.7, 0.005, -r * 0.75)), hex, alpha * 0.8),
    ])
}

fn scorch_mesh() -> Mesh {
    // Charred core with a singed rim peeking out under it.
    merged_flat(vec![
        tint_a(disc(0.62, 14), CHAR_RIM, 0.6),
        tint_a(disc(0.45, 12).translated_by(Vec3::Y * 0.006), CHAR, 0.85),
    ])
}

/// A dropped ork club lying on its side.
fn club_mesh() -> Mesh {
    let lay = Quat::from_rotation_z(-FRAC_PI_2); // shaft along +X, resting on the ground
    merged_flat(vec![
        tint(
            Cylinder::new(0.05, 0.6).mesh().resolution(7).build().rotated_by(lay).translated_by(Vec3::new(0.0, 0.05, 0.0)),
            lin(CLUB_WOOD),
        ),
        tint(
            Sphere::new(0.11).mesh().ico(0).expect("ico detail in range").translated_by(Vec3::new(0.34, 0.09, 0.0)),
            lin(CLUB_KNOB),
        ),
    ])
}

/// A spear stuck in the earth at a battle lean.
fn spear_mesh() -> Mesh {
    let lean = Quat::from_rotation_z(0.5);
    merged_flat(vec![
        tint(
            Cylinder::new(0.022, 1.1).mesh().resolution(6).build().translated_by(Vec3::Y * 0.42).rotated_by(lean),
            lin(SPEAR_SHAFT),
        ),
        tint(
            Cone { radius: 0.05, height: 0.16 }.mesh().build().translated_by(Vec3::Y * 1.02).rotated_by(lean),
            lin(IRON),
        ),
    ])
}

/// A lost ork helmet — a crude squashed iron cap with stub horns, upside-up on the turf.
fn helmet_mesh() -> Mesh {
    let horn = |sx: f32| {
        tint(
            Cone { radius: 0.045, height: 0.16 }
                .mesh()
                .build()
                .translated_by(Vec3::Y * 0.08)
                .rotated_by(Quat::from_rotation_z(sx * 1.25))
                .translated_by(Vec3::new(sx * 0.15, 0.12, 0.0)),
            lin(IRON_DARK),
        )
    };
    merged_flat(vec![
        tint(
            Sphere::new(0.16)
                .mesh()
                .ico(1)
                .expect("ico detail in range")
                .scaled_by(Vec3::new(1.0, 0.72, 1.0))
                .translated_by(Vec3::Y * 0.06),
            lin(IRON),
        ),
        horn(1.0),
        horn(-1.0),
    ])
}

// ── Deterministic mulberry32 (same recipe as camps.rs) ────────────────────────────

fn rng01(s: &mut u32) -> f32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
}
