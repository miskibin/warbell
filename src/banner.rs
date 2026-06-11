//! **Cloth banners** — small CPU-animated flags that actually flutter, replacing the
//! static box "flags" on the keep spire, the four wall towers and the ork-camp banner
//! poles. The static castle/camp geometry stays merged + batched; only the cloth itself
//! is a separate entity whose tiny grid mesh (11×5 verts) is re-waved every frame.
//!
//! Every flag streams in the SAME fixed world direction ([`WIND_YAW`]) regardless of the
//! structure it hangs off — one coherent island wind, matching the tree sway in `wind.rs`
//! (which this deliberately does not touch). The hoist edge (x = 0) is pinned at the pole;
//! amplitude grows toward the fly end, with a slight tail sag that lifts on gusts.
//!
//! Per the mesh contract, colour lives in `ATTRIBUTE_COLOR` — but cloth needs its own
//! two-sided material (the shared white prop material backface-culls), and since a
//! per-frame-mutated mesh can't batch anyway, each flag owning a material handle is free.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::Face;

use crate::palette::lin;

/// One island-wide wind heading (radians about Y) every flag streams along. The cloth is
/// authored along local +X, so `from_rotation_y(WIND_YAW)` streams it toward world
/// `(cos WIND_YAW, 0, -sin WIND_YAW)` ≈ (0.90, 0, −0.43) — roughly the ±X axis the tree
/// sway in `wind.rs` leans about, so flags and canopies agree on the wind.
const WIND_YAW: f32 = 0.45;
/// Cloth grid resolution: columns along the fly (x), rows down the hoist (y).
const NX: usize = 11;
const NY: usize = 5;

pub struct BannerPlugin;

impl Plugin for BannerPlugin {
    fn build(&self, app: &mut App) {
        // Ungated visual animation (like `wind.rs` sway): reads virtual time, so it
        // freezes with the world behind panels but keeps drawing.
        app.add_systems(Update, flutter_flags);
        // Screenshot hook: FOREST_FLAGTEST=1 parks one test flag on open ground near the
        // hero spawn so the cloth can be framed in isolation.
        if std::env::var("FOREST_FLAGTEST").is_ok() {
            app.add_systems(Startup, spawn_test_flag);
        }
    }
}

fn spawn_test_flag(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_flag(&mut commands, &mut meshes, &mut materials, Vec3::new(0.0, 6.0, -22.0), 0.85, 0.42, 0x2f5fa6, Some(0xd9b34a));
}

/// A fluttering cloth flag. The mesh handle on the same entity is unique to this flag and
/// gets its positions + normals rewritten every frame from these parameters.
#[derive(Component)]
pub struct ClothFlag {
    w: f32,
    h: f32,
    /// Wave amplitude at the fly end (world units).
    amp: f32,
    /// Per-flag phase so neighbouring flags don't flutter in lockstep.
    phase: f32,
}

/// Spawn a cloth flag whose hoist edge hangs at world `attach` (the pole, at the flag's
/// vertical centre), streaming along the global wind. `field` is the cloth colour and
/// `accent` an optional hoist-band colour (a cheap heraldic detail). Returns the entity so
/// callers can tag it (`BiomeEntity`, `CastlePart`, …).
pub fn spawn_flag(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    attach: Vec3,
    w: f32,
    h: f32,
    field: u32,
    accent: Option<u32>,
) -> Entity {
    let flag = ClothFlag {
        w,
        h,
        amp: h * 0.22,
        phase: attach.x * 0.7 + attach.z * 0.55, // the wind.rs position hash
    };
    let mut mesh = flag_grid_mesh(&flag, field, accent);
    write_wave(&flag, 0.0, &mut mesh);
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        double_sided: true,
        cull_mode: None::<Face>,
        ..default()
    });
    commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat),
            Transform::from_translation(attach).with_rotation(Quat::from_rotation_y(WIND_YAW)),
            flag,
        ))
        .id()
}

/// Build the static parts of the cloth grid (indices, UVs, colours); positions/normals are
/// filled by [`write_wave`]. Colours: the field, a slightly darker bottom row (weathering)
/// and an optional accent band on the two hoist-side columns.
fn flag_grid_mesh(flag: &ClothFlag, field: u32, accent: Option<u32>) -> Mesh {
    let _ = flag;
    let field_c = lin(field);
    let dark_c = [field_c[0] * 0.82, field_c[1] * 0.82, field_c[2] * 0.82, 1.0];
    let accent_c = accent.map(lin);

    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(NX * NY);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(NX * NY);
    for j in 0..NY {
        for i in 0..NX {
            let c = match accent_c {
                Some(a) if i < 2 => a,
                _ if j == NY - 1 => dark_c, // bottom edge weathers darker
                _ => field_c,
            };
            colors.push(c);
            uvs.push([i as f32 / (NX - 1) as f32, j as f32 / (NY - 1) as f32]);
        }
    }
    let mut idx: Vec<u32> = Vec::with_capacity((NX - 1) * (NY - 1) * 6);
    for j in 0..NY - 1 {
        for i in 0..NX - 1 {
            let a = (j * NX + i) as u32;
            let b = a + 1;
            let c = a + NX as u32;
            let d = c + 1;
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    m.insert_indices(Indices::U32(idx));
    m
}

/// Write the waved positions + normals for time `t` into `mesh`. The hoist (x=0) is
/// pinned; two travelling sines ripple toward the fly end, whose tail also sags and lifts
/// with a slow gust cycle.
fn write_wave(flag: &ClothFlag, t: f32, mesh: &mut Mesh) {
    let (w, h) = (flag.w, flag.h);
    let k1 = 6.3 / w; // ~one full wave across the cloth
    let k2 = 11.0 / w;
    let gust = 0.55 + 0.45 * (t * 0.9 + flag.phase).sin();
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(NX * NY);
    for j in 0..NY {
        for i in 0..NX {
            let u = i as f32 / (NX - 1) as f32;
            let x = u * w;
            let y0 = (0.5 - j as f32 / (NY - 1) as f32) * h;
            let wave = (x * k1 - t * 5.2 + flag.phase).sin()
                + 0.45 * (x * k2 - t * 8.1 + flag.phase * 1.7 + y0 * 2.2).sin();
            let z = wave * flag.amp * u * u; // quadratic pin: stiff at the hoist
            let y = y0 - 0.16 * h * u * (1.0 - gust * 0.8);
            pos.push([x, y, z]);
        }
    }
    // Normals from central differences over the displaced grid.
    let at = |i: usize, j: usize| Vec3::from(pos[j * NX + i]);
    let mut nrm: Vec<[f32; 3]> = Vec::with_capacity(NX * NY);
    for j in 0..NY {
        for i in 0..NX {
            let dx = at((i + 1).min(NX - 1), j) - at(i.saturating_sub(1), j);
            let dy = at(i, (j + 1).min(NY - 1)) - at(i, j.saturating_sub(1));
            let n = dx.cross(dy).normalize_or(Vec3::Z);
            nrm.push([n.x, n.y, n.z]);
        }
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
}

/// Re-wave every flag's grid each frame (a handful of 55-vertex meshes — trivial).
fn flutter_flags(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    q: Query<(&ClothFlag, &Mesh3d)>,
) {
    let t = time.elapsed_secs_wrapped();
    for (flag, mesh3d) in &q {
        if let Some(mesh) = meshes.get_mut(&mesh3d.0) {
            write_wave(flag, t, mesh);
        }
    }
}
