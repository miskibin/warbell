//! **Vista pass** (map-character overhaul pass 5) — the "miejsca w które się można wpatrywać":
//! a WATERFALL pouring off the rock mesa's passless SW wall into the lake (the island's one
//! moving landmark — motion + white water pull the eye from the whole south), and three
//! authored OVERLOOK spots — a framing prop (crooked lone pine + sitting stone + cairn) on a
//! shelf edge with a composed view behind it: the rock summit gazing back at the castle plain,
//! the snow summit over the western sea, and the canyon mouth's release onto the desert.
//!
//! The waterfall is found procedurally but deterministically: walk from the lake centre toward
//! the rock massif until the shore, then hunt the first ≥1.6u wall face — exactly the cliff the
//! mesa design left passless for this purpose. Sheets are static streaked meshes; the LIFE
//! comes from falling foam motes + drifting base mist (cheap unlit blobs, `FallMote`).

use std::f32::consts::TAU;

use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::lin;

pub struct VistaPlugin;
impl Plugin for VistaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, animate_falls);
    }
}

/// A falling foam mote (cycling top→bottom of the falls) or a base-mist puff (slow rise + fade).
#[derive(Component)]
struct FallMote {
    top: Vec3,
    drop: f32,
    speed: f32,
    phase: f32,
    /// true = falling foam chunk; false = base mist (drifts up a little instead).
    falling: bool,
}

fn animate_falls(time: Res<Time>, mut q: Query<(&FallMote, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (m, mut tf) in &mut q {
        let cycle = (t * m.speed + m.phase).rem_euclid(1.0);
        if m.falling {
            // Accelerating drop, slight outward drift, shrinking near the plunge pool.
            let d = cycle * cycle;
            tf.translation = m.top + Vec3::new((m.phase * 9.0).sin() * 0.2, -d * m.drop, 0.25 * cycle);
            tf.scale = Vec3::splat(0.5 + 0.5 * (1.0 - d));
        } else {
            tf.translation = m.top + Vec3::new((t * 0.5 + m.phase * 6.0).sin() * 0.8, cycle * 1.6, (t * 0.4 + m.phase).cos() * 0.8);
            tf.scale = Vec3::splat((0.6 + cycle) * (1.0 - cycle).max(0.0) * 2.0);
        }
    }
}

// ── meshes ───────────────────────────────────────────────────────────────────────────
fn tint(mut m: Mesh, col: u32) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(col); n]);
    m
}
fn assemble(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for part in it {
        base.merge(&part).expect("parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

const FOAM: u32 = 0xdfeef2;
const FOAM_DK: u32 = 0xa8ccd4;
const ROCK_WET: u32 = 0x3c4448;

/// The cascade: overlapping streaked water sheets down the wall face (local −Z faces the lake,
/// base at y=0 = the plunge pool), wet rocks flanking it, a foam apron at the base.
fn cascade(height: f32) -> Mesh {
    let mut v = Vec::new();
    // Main sheets: alternating bright/dark vertical strips, slightly staggered in Z and X.
    for (i, (w, dx, dz, col)) in [
        (1.1_f32, -0.75_f32, 0.00_f32, FOAM),
        (0.7, 0.15, -0.08, FOAM_DK),
        (0.9, 0.85, 0.04, FOAM),
        (0.5, -0.15, 0.10, FOAM_DK),
        (0.6, 1.45, -0.02, FOAM),
    ]
    .into_iter()
    .enumerate()
    {
        let h = height * (0.94 + (i as f32 * 1.7).sin() * 0.06);
        v.push(tint(
            Cuboid::new(w, h, 0.16).mesh().build().translated_by(Vec3::new(dx, h * 0.5, dz)),
            col,
        ));
    }
    // Wet flanking rocks so the water reads as pouring THROUGH a notch.
    for (sx, r) in [(-1.7_f32, 0.55_f32), (2.3, 0.7)] {
        v.push(tint(
            Sphere::new(r).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.0, 1.5, 1.0)).translated_by(Vec3::new(sx, r * 0.9, 0.1)),
            ROCK_WET,
        ));
    }
    // Plunge-pool foam apron.
    v.push(tint(
        Sphere::new(1.9).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.3, 0.16, 1.0)).translated_by(Vec3::new(0.3, 0.06, -0.9)),
        FOAM,
    ));
    assemble(v)
}

/// Overlook framing prop: a crooked windswept lone pine, a flat sitting stone, a marker cairn —
/// stand here, look past the tree, see the island.
fn overlook_prop() -> Mesh {
    const BARK: u32 = 0x4e3a24;
    const NEEDLE: u32 = 0x2f5233;
    const NEEDLE_D: u32 = 0x24402a;
    const STONE: u32 = 0x83878d;
    let mut v = Vec::new();
    // Windswept trunk: two leaning segments.
    v.push(tint(
        Cylinder::new(0.14, 1.6).mesh().resolution(7).build().rotated_by(Quat::from_rotation_z(0.35)).translated_by(Vec3::new(0.0, 0.75, 0.0)),
        BARK,
    ));
    v.push(tint(
        Cylinder::new(0.10, 1.2).mesh().resolution(7).build().rotated_by(Quat::from_rotation_z(0.75)).translated_by(Vec3::new(-0.65, 1.75, 0.0)),
        BARK,
    ));
    // Wind-flagged canopy: layered pads all pushed to the lee side.
    for (dx, dy, r, col) in [
        (-1.15_f32, 2.25_f32, 0.55_f32, NEEDLE),
        (-1.6, 2.05, 0.42, NEEDLE_D),
        (-0.75, 2.5, 0.4, NEEDLE),
        (-1.95, 2.3, 0.3, NEEDLE),
    ] {
        v.push(tint(
            Sphere::new(r).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.5, 0.5, 1.1)).translated_by(Vec3::new(dx, dy, 0.0)),
            col,
        ));
    }
    // Flat sitting stone + small cairn.
    v.push(tint(Cuboid::new(0.9, 0.3, 0.7).mesh().build().translated_by(Vec3::new(0.9, 0.15, 0.5)), STONE));
    for (i, (r, y)) in [(0.22_f32, 0.08_f32), (0.16, 0.28), (0.10, 0.42)].into_iter().enumerate() {
        v.push(tint(
            Sphere::new(r).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.0, 0.6, 1.0)).translated_by(Vec3::new(1.0 + i as f32 * 0.02, y, -0.5)),
            STONE,
        ));
    }
    assemble(v)
}

// ── placement ────────────────────────────────────────────────────────────────────────
/// Build-phase 33: the waterfall + the three overlooks.
pub fn populate(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    use crate::worldmap::ground_at_world;
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.85, ..default() });
    let foam_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.92, 0.97, 1.0, 0.85),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let mist_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.96, 1.0, 0.35),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    // 1. THE WATERFALL — stands at the AUTHORED plunge-stream head (`worldmap::BLUE_STREAMS[0]`
    //    carves real water from the mesa wall's foot down to the lake, so the falls plunge into
    //    a connected stream, not onto the dry shelf). Two search generations failed here: the
    //    single-ray version landed 14u inland on grass, and a "wall within 6u of shore" fan
    //    never fired at all — the wall genuinely stands ~12u behind the shore all around this
    //    lake. Authoring the stream + anchor ended the guessing.
    let (head, flow) = crate::worldmap::waterfall_site_world();
    // The stream head is carved water (ground None); the wall rises against the flow.
    let base_y = crate::worldmap::SEA_Y + 0.05;
    let mut fall: Option<(Vec2, f32, f32, Vec2)> = None; // (wall pos, top y, base y, flow dir)
    for d in [1.0_f32, 1.8, 2.6, 3.4] {
        let back = head - flow * d;
        if let Some(top_y) = ground_at_world(back.x, back.y) {
            if top_y - base_y >= 1.6 {
                fall = Some((head - flow * (d - 1.0), top_y, base_y, flow));
                break;
            }
        }
    }
    if let Some((p, top_y, base_y, dir)) = fall {
        let h = (top_y - base_y) + 0.6;
        let yaw = dir.x.atan2(dir.y); // local −Z faces back toward the lake
        commands.spawn((
            Mesh3d(meshes.add(cascade(h))),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(p.x, base_y - 0.15, p.y).with_rotation(Quat::from_rotation_y(yaw)),
            BiomeEntity,
            NotShadowCaster,
        ));
        // Falling foam chunks down the face + mist at the plunge pool.
        let mote = meshes.add(Sphere::new(0.22).mesh().ico(1).unwrap());
        for k in 0..8 {
            let phase = k as f32 / 8.0;
            commands.spawn((
                Mesh3d(mote.clone()),
                MeshMaterial3d(foam_mat.clone()),
                Transform::from_xyz(p.x, top_y, p.y),
                FallMote {
                    top: Vec3::new(p.x + (phase * 19.0).sin() * 1.2, base_y + h - 0.3, p.y + (phase * 31.0).cos() * 0.3),
                    drop: h - 0.2,
                    speed: 0.55 + phase * 0.12,
                    phase: phase * 7.0,
                    falling: true,
                },
                NotShadowCaster,
                BiomeEntity,
            ));
        }
        let mist = meshes.add(Sphere::new(0.55).mesh().ico(1).unwrap());
        for k in 0..5 {
            let phase = k as f32 / 5.0;
            commands.spawn((
                Mesh3d(mist.clone()),
                MeshMaterial3d(mist_mat.clone()),
                Transform::from_xyz(p.x, base_y, p.y),
                FallMote {
                    top: Vec3::new(p.x - dir.x * 1.2, base_y + 0.2, p.y - dir.y * 1.2),
                    drop: 0.0,
                    speed: 0.16 + phase * 0.05,
                    phase: phase * 5.0,
                    falling: false,
                },
                NotShadowCaster,
                BiomeEntity,
            ));
        }
        info!("vista: waterfall at {:.1},{:.1} (wall {:.1}u)", p.x, p.y, top_y - base_y);
    } else {
        warn!("vista: no wall face found for the waterfall between the lake and the rock mesa");
    }

    // 2. OVERLOOKS — ring-search a flat legal spot near each authored anchor, face the view.
    let prop = meshes.add(overlook_prop());
    let range = VisibilityRange { start_margin: 0.0..0.0, end_margin: 130.0..130.0, use_aabb: true };
    let anchors: [(Vec2, Vec2, &str); 3] = [
        // (anchor, what the view looks AT, label)
        (Vec2::new(96.8, 6.6), Vec2::ZERO, "rock summit → castle plain"),
        (Vec2::new(-101.2, -66.0), Vec2::new(-150.0, -66.0), "snow summit → western sea"),
        (Vec2::new(95.0, -66.0), Vec2::new(66.0, -88.0), "canyon mouth → rival dunes"),
    ];
    let spot_ok = |x: f32, z: f32| {
        let Some(y0) = crate::worldmap::ground_at_world(x, z) else { return false };
        (0..6).all(|k| {
            let a = k as f32 * (TAU / 6.0);
            matches!(crate::worldmap::ground_at_world(x + a.cos() * 1.4, z + a.sin() * 1.4), Some(y) if (y - y0).abs() <= 0.3)
        }) && !crate::roads::on_road(x, z)
            && !crate::blockers::is_blocked(x, z)
    };
    let mut placed = 0;
    for (anchor, view_at, label) in anchors {
        let mut found = None;
        'ring: for r in [0.0_f32, 2.5, 5.0, 7.5, 10.0] {
            for k in 0..10 {
                let a = k as f32 * (TAU / 10.0);
                let (x, z) = (anchor.x + a.cos() * r, anchor.y + a.sin() * r);
                if spot_ok(x, z) {
                    found = Some(Vec2::new(x, z));
                    break 'ring;
                }
            }
        }
        let Some(q) = found else {
            warn!("vista: no overlook footing near {label}");
            continue;
        };
        let y = crate::worldmap::ground_at_world(q.x, q.y).unwrap_or(0.0);
        // The pine's wind-flagged canopy sweeps toward local −X; yaw so that sweep frames the
        // view direction (stand at the prop, look past the lean at `view_at`).
        let v = (view_at - q).normalize_or_zero();
        let yaw = v.y.atan2(v.x) + std::f32::consts::PI;
        commands.spawn((
            Mesh3d(prop.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(q.x, y, q.y).with_rotation(Quat::from_rotation_y(-yaw)),
            BiomeEntity,
            range.clone(),
        ));
        crate::blockers::add_obb(q.x, q.y, 0.5, 0.5, 0.0);
        placed += 1;
        info!("vista: overlook ({label}) at {:.1},{:.1}", q.x, q.y);
    }
    info!("vista: {placed}/3 overlooks");
}
