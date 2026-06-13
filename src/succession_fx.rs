//! **Succession visuals** — graves + the soul wisp, ported from the old game's
//! `successionStore.ts` / `Grave.tsx` / `SoulWisp.tsx`. When the blade passes (an heir falls
//! and the next rises at the gate, in `player::health`), this drops a headstone where the hero
//! died and flies a glowing spirit from the body to the rising heir over [`SOUL_DUR`]. Graves
//! accumulate across a run (a quiet tally of the fallen) and are cleared on a fresh run.

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::game_state::AppState;
use crate::palette::lin;

/// Spirit travel time, body → heir (TS `SUCCESSION_DURATION`).
const SOUL_DUR: f32 = 1.7;
/// Apex height of the wisp's arc (world units).
const SOUL_ARC: f32 = 1.6;

/// Emitted by `player::health` when an heir falls and the next takes up the line.
#[derive(Message, Clone, Copy)]
pub struct HeirFell {
    /// Where the fallen hero lies (grave goes here).
    pub grave_at: Vec3,
    /// Where the next heir rises (the wisp flies here).
    pub rise_at: Vec3,
}

#[derive(Component)]
struct Grave;

#[derive(Component)]
struct SoulWisp {
    from: Vec3,
    to: Vec3,
    /// Elapsed-seconds the flight started.
    born: f32,
}

/// Shared baked handles so the spawn path needs only `Commands` + this resource.
#[derive(Resource)]
struct FxAssets {
    grave_mesh: Handle<Mesh>,
    grave_mat: Handle<StandardMaterial>,
    wisp_mesh: Handle<Mesh>,
    wisp_mat: Handle<StandardMaterial>,
}

pub struct SuccessionFxPlugin;

impl Plugin for SuccessionFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<HeirFell>()
            .add_systems(Startup, setup_fx_assets)
            .add_systems(Update, (spawn_succession_fx, drive_souls))
            .add_systems(OnExit(AppState::StartScreen), clear_graves)
            .add_systems(OnExit(AppState::GameOver), clear_graves);
        // (Pause-menu Restart / Load relaunch the process now — see game_state::RestartProcess.)
    }
}

// ── tiny local mesh helpers ────────────────────────────────────────────────────────
fn tinted(mut m: Mesh, c: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(c); n]);
    m
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}

fn grave_mesh() -> Mesh {
    const DIRT: u32 = 0x6b4f37;
    const MOSS: u32 = 0x6f7d3c;
    const STONE: u32 = 0x8d8c86;
    const DARK: u32 = 0x54534f;
    let mut m = bx(0.9, 0.16, 0.6, Vec3::new(0.0, 0.08, 0.05), DIRT); // mound
    for part in [
        bx(0.72, 0.05, 0.3, Vec3::new(0.0, 0.18, 0.12), MOSS), // moss on the mound
        bx(0.5, 0.55, 0.12, Vec3::new(0.0, 0.4, -0.2), STONE), // headstone slab
        bx(0.07, 0.28, 0.02, Vec3::new(0.0, 0.42, -0.27), DARK), // engraved cross (vertical)
        bx(0.22, 0.07, 0.02, Vec3::new(0.0, 0.48, -0.27), DARK), // cross (horizontal)
    ] {
        m.merge(&part).expect("grave parts share attributes");
    }
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

fn setup_fx_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let grave_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        ..default()
    });
    // Warm, unlit, brightly-emissive spirit — reads as a glowing wisp (bloom catches it).
    let wisp_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.55),
        emissive: LinearRgba::rgb(3.0, 2.2, 1.0),
        unlit: true,
        ..default()
    });
    commands.insert_resource(FxAssets {
        grave_mesh: meshes.add(grave_mesh()),
        grave_mat,
        wisp_mesh: meshes.add(Sphere::new(0.2).mesh().ico(3).unwrap()),
        wisp_mat,
    });
}

/// On each fallen heir: plant a grave at the body + launch a soul wisp toward the rising heir.
fn spawn_succession_fx(
    time: Res<Time>,
    mut fell: MessageReader<HeirFell>,
    assets: Option<Res<FxAssets>>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let now = time.elapsed_secs();
    for ev in fell.read() {
        // Grave faces the keep (origin): yaw toward the world centre so the headstone reads.
        let yaw = (-ev.grave_at.x).atan2(-ev.grave_at.z);
        commands.spawn((
            Mesh3d(assets.grave_mesh.clone()),
            MeshMaterial3d(assets.grave_mat.clone()),
            Transform::from_translation(ev.grave_at).with_rotation(Quat::from_rotation_y(yaw)),
            Grave,
            BiomeEntity,
        ));
        commands.spawn((
            Mesh3d(assets.wisp_mesh.clone()),
            MeshMaterial3d(assets.wisp_mat.clone()),
            Transform::from_translation(ev.grave_at + Vec3::Y),
            SoulWisp { from: ev.grave_at + Vec3::Y, to: ev.rise_at + Vec3::Y, born: now },
        ));
    }
}

/// Ease the wisp along a parabolic arc from body to heir; flicker its scale; despawn on arrival.
fn drive_souls(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &SoulWisp, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (e, soul, mut tf) in &mut q {
        let t = ((now - soul.born) / SOUL_DUR).clamp(0.0, 1.0);
        if t >= 1.0 {
            commands.entity(e).try_despawn();
            continue;
        }
        let e_t = t * t * (3.0 - 2.0 * t); // smoothstep
        let mut p = soul.from.lerp(soul.to, e_t);
        p.y += (t * std::f32::consts::PI).sin() * SOUL_ARC; // arc apex mid-flight
        tf.translation = p;
        let flick = 0.85 + (now * 22.0).sin() * 0.12;
        tf.scale = Vec3::splat((0.7 + (t * std::f32::consts::PI).sin() * 0.5) * flick);
    }
}

fn clear_graves(mut commands: Commands, graves: Query<Entity, With<Grave>>, wisps: Query<Entity, With<SoulWisp>>) {
    for e in &graves {
        commands.entity(e).try_despawn();
    }
    for e in &wisps {
        commands.entity(e).try_despawn();
    }
}
