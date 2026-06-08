//! Winding river with realistic animated water.
//!
//! A smooth ribbon mesh follows a sine-curved centerline across the scene, sitting just
//! below the ground (`WATER_Y`) so the banks read. The surface is an
//! `ExtendedMaterial<StandardMaterial, WaterExt>` whose FRAGMENT shader
//! (`assets/shaders/water.wgsl`) perturbs the lighting normal with scrolling ripple
//! waves: because the base StandardMaterial has tiny roughness + high reflectance, that
//! moving normal makes it reflect the Atmosphere sky (via IBL) and slide the sun glint
//! across the surface — i.e. real-looking water. Thin flat-shaded mud/sand strips line
//! each edge so water meets land cleanly.
//!
//! Exposes [`on_river`] / [`river_bank_t`] (per CONTRACT2.md) so the scatter + decor can
//! keep props out of the water and dress the banks. The mesh geometry and these two
//! helpers share the SAME centerline math, so they always agree.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::palette::{lin, lin_scaled, FOREST_GROUND};

// ── River geometry constants (shared by the mesh + the on_river/bank_t helpers) ──

/// Sideways swing amplitude of the centerline (world units): `x = AMP * sin(z*FREQ)`.
const RIVER_AMP: f32 = 6.0;
/// Spatial frequency of the centerline sine (radians per world unit along Z).
const RIVER_FREQ: f32 = 0.12;
/// Half the water-surface width: a tile is "on the river" within this of the centerline.
const HALF_WIDTH: f32 = 2.4;
/// How far past the water edge the bank ramp (`river_bank_t` 0→1) extends.
const BANK_RAMP: f32 = 3.0;
/// The river runs the full depth of the scene (well past the 32×32 patch) so it reads to
/// the fog horizon at both ends.
const Z_MIN: f32 = -90.0;
const Z_MAX: f32 = 90.0;

/// Water sheet height. Our ground is a single FLAT plane at y=0 (no carved channel),
/// so the water must sit just ABOVE it or the opaque ground occludes it — a shallow
/// forest stream flush with the grass.
const WATER_Y: f32 = 0.045;
/// Wet-waterline lip height: the muddy shore rises just under the water surface, then
/// the strip ramps back DOWN to ~ground level at its outer edge so it meets the grass
/// flush (no hard raised stripe).
const BANK_Y: f32 = 0.025;

/// Mesh tessellation: segments along the river length and columns across its width.
const LEN_SEGMENTS: usize = 280;
const WIDTH_COLUMNS: usize = 6;

// Bank tones — wet/dark at the waterline, grading to the surrounding grass at the outer
// edge so the shore blends in instead of reading as a saturated tan stripe.
const WET_MUD: u32 = 0x3d3324; // dark, saturated-wet right at the waterline
const DAMP_EARTH: u32 = 0x5a4c33; // damp muddy earth in the mid-strip

const WATER_SHADER: &str = "shaders/water.wgsl";

// ── Centerline math (one source of truth for mesh + queries) ────────────────────

/// World-space X of the river centerline at depth `z`.
#[inline]
fn centerline_x(z: f32) -> f32 {
    RIVER_AMP * (z * RIVER_FREQ).sin()
}

/// Unit tangent of the centerline in the XZ plane at depth `z` (advancing +Z).
#[inline]
fn centerline_tangent(z: f32) -> Vec2 {
    // d/dz of (centerline_x(z), z) = (AMP*FREQ*cos(z*FREQ), 1).
    let dx = RIVER_AMP * RIVER_FREQ * (z * RIVER_FREQ).cos();
    Vec2::new(dx, 1.0).normalize()
}

/// Unit normal (in XZ) pointing to the river's left/right edges at depth `z`.
#[inline]
fn centerline_normal(z: f32) -> Vec2 {
    let t = centerline_tangent(z);
    Vec2::new(t.y, -t.x) // perpendicular in XZ
}

/// Perpendicular distance from world point `(x,z)` to the centerline. Uses the local
/// normal at the point's own `z`, which is accurate because the curve is gentle
/// (|slope| ≤ AMP*FREQ = 0.72) — good enough for placement masking + banks.
#[inline]
fn dist_to_centerline(x: f32, z: f32) -> f32 {
    let n = centerline_normal(z);
    // Offset from the centerline point at this z is purely in X (same-z projection); its
    // perpendicular distance is that offset dotted with the unit normal's X component.
    ((x - centerline_x(z)) * n.x).abs()
}

/// True where the river water surface is (within the half-width of the centerline).
/// Scatter calls this to avoid spawning props in the water.
pub fn on_river(x: f32, z: f32) -> bool {
    if z < Z_MIN || z > Z_MAX {
        return false;
    }
    dist_to_centerline(x, z) <= HALF_WIDTH
}

/// 0 at the centerline, ramping to 1 a few units past the bank — decor uses this to
/// place wet-shore dressing (reeds, pebbles) along the edges and fade it out inland.
/// Returns 1.0 well away from the river (fully "dry land").
pub fn river_bank_t(x: f32, z: f32) -> f32 {
    if z < Z_MIN || z > Z_MAX {
        return 1.0;
    }
    let d = dist_to_centerline(x, z);
    if d <= HALF_WIDTH {
        0.0
    } else {
        ((d - HALF_WIDTH) / BANK_RAMP).clamp(0.0, 1.0)
    }
}

// ── Material ────────────────────────────────────────────────────────────────────

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct WaterParams {
    /// x=amplitude, y=wave frequency, z=scroll speed, w=fresnel strength.
    pub params: Vec4,
    /// rgb = sky/fresnel tint added at grazing angles (a unused).
    pub sky_tint: Vec4,
}

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct WaterExt {
    #[uniform(100)]
    pub params: WaterParams,
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }
    // alpha_mode() left as None → inherits the base StandardMaterial's AlphaMode::Blend,
    // so the river is the translucent reflective sheet we configure below.
}

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        // Registers the water material only; the biome runner spawns the river when the
        // active biome has `river: true`.
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
    }
}

/// Spawn the winding river (water sheet + shore banks). Called by the biome runner for
/// biomes with `river: true`; tagged [`BiomeEntity`] so a switch wipes it.
pub fn spawn_river(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    water_mats: &mut Assets<WaterMaterial>,
    std_mats: &mut Assets<StandardMaterial>,
    river_color: u32,
) {
    // ── Water sheet ──────────────────────────────────────────────────────────
    let water_mesh = meshes.add(build_river_ribbon());
    let water_material = water_mats.add(ExtendedMaterial {
        base: StandardMaterial {
            // Per-biome river tint, slightly transparent so the reflection + fresnel rim
            // read as a real water sheet rather than a flat card.
            base_color: Color::srgba(
                ((river_color >> 16) & 0xff) as f32 / 255.0,
                ((river_color >> 8) & 0xff) as f32 / 255.0,
                (river_color & 0xff) as f32 / 255.0,
                0.85,
            ),
            perceptual_roughness: 0.34, // matte-ish → soft sheen, not a sharp mirror
            metallic: 0.0,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None, // visible if the fly-cam dips below the surface
            ..default()
        },
        extension: WaterExt {
            params: WaterParams {
                // Basic: small gentle wave — low amplitude, low frequency, slow scroll.
                params: Vec4::new(0.12, 0.8, 0.5, 0.0),
                sky_tint: Vec4::new(0.70, 0.82, 0.93, 0.0),
            },
        },
    });
    commands.spawn((
        Mesh3d(water_mesh),
        MeshMaterial3d(water_material),
        Transform::default(),
        crate::biome::BiomeEntity,
    ));

    // ── Mud/sand bank strips (one shared white vertex-colour material) ─────────
    let bank_mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE, // vertex colours carry the mud/sand tone
        perceptual_roughness: 1.0,
        // Double-sided so the thin shore strips read from any camera angle regardless of
        // the per-side triangle winding.
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(build_banks())),
        MeshMaterial3d(bank_mat),
        Transform::default(),
        bevy::light::NotShadowCaster,
        crate::biome::BiomeEntity,
    ));
}

// ── Mesh builders ────────────────────────────────────────────────────────────

/// A long, gently-curving quad ribbon following the centerline. Densely subdivided
/// along its length (so the curve is smooth) and a few columns across (so the strip
/// bends without long sliver triangles). Flat sheet at `WATER_Y`; the shader does all
/// the surface animation, so we keep the geometry planar and let normals come from the
/// fragment shader's ripple field.
fn build_river_ribbon() -> Mesh {
    let rows = LEN_SEGMENTS + 1;
    let cols = WIDTH_COLUMNS + 1;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(rows * cols);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(rows * cols);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(rows * cols);

    for i in 0..rows {
        let tz = i as f32 / LEN_SEGMENTS as f32;
        let z = Z_MIN + (Z_MAX - Z_MIN) * tz;
        let cx = centerline_x(z);
        let n = centerline_normal(z);
        for j in 0..cols {
            // -1..1 across the width.
            let s = (j as f32 / WIDTH_COLUMNS as f32) * 2.0 - 1.0;
            let x = cx + n.x * s * HALF_WIDTH;
            let zz = z + n.y * s * HALF_WIDTH;
            positions.push([x, WATER_Y, zz]);
            normals.push([0.0, 1.0, 0.0]);
            // UV across width 0..1, along length scaled so the (unused) UV is sane.
            uvs.push([(s + 1.0) * 0.5, tz * (Z_MAX - Z_MIN) * 0.1]);
        }
    }

    let mut indices: Vec<u32> = Vec::with_capacity(LEN_SEGMENTS * WIDTH_COLUMNS * 6);
    for i in 0..LEN_SEGMENTS {
        for j in 0..WIDTH_COLUMNS {
            let a = (i * cols + j) as u32;
            let b = (i * cols + j + 1) as u32;
            let c = ((i + 1) * cols + j) as u32;
            let d = ((i + 1) * cols + j + 1) as u32;
            // Two triangles per cell (CCW seen from above).
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Cheap 1-D hash in [0,1) for per-row jitter (deterministic, no RNG state needed).
#[inline]
fn hash1(n: f32) -> f32 {
    let s = (n * 12.9898).sin() * 43758.5453;
    s - s.floor()
}

/// A flat-shaded shore strip on each side of the water. Three columns per side:
/// wet dark mud at the waterline → damp earth → a grass-toned outer edge that ramps
/// back DOWN to ~ground level, so the shore grades into the surrounding grass instead
/// of standing as a hard saturated stripe. Per-row width + colour jitter break the
/// ribbon so the bank reads natural, not painted-on. One merged vertex-coloured mesh.
fn build_banks() -> Mesh {
    let rows = LEN_SEGMENTS + 1;
    const COLS: u32 = 3;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let wet = lin(WET_MUD);
    let damp = lin(DAMP_EARTH);
    // Outer edge ≈ the surrounding grass (a touch darker = wet-shore grass) so the seam
    // disappears into the field.
    let grass = lin_scaled(FOREST_GROUND, 0.80);

    for &side in &[1.0f32, -1.0] {
        let base = positions.len() as u32;
        for i in 0..rows {
            let tz = i as f32 / LEN_SEGMENTS as f32;
            let z = Z_MIN + (Z_MAX - Z_MIN) * tz;
            let cx = centerline_x(z);
            let n = centerline_normal(z);

            // Per-row jitter so neither the width nor the tone is uniform.
            let j = hash1(z * 1.7 + side * 13.0);
            let j2 = hash1(z * 0.6 + side * 31.0);

            // Three offsets out from the centerline; outer edge is irregular.
            let inner = HALF_WIDTH - 0.18; // overlaps the water edge (no gap)
            let mid = HALF_WIDTH + 0.30 + j * 0.30;
            let outer = HALF_WIDTH + 0.95 + j2 * 1.0;

            // Ramp the height down from the wet lip to ~ground at the grass edge.
            let cv = 0.90 + j * 0.18; // per-row brightness wobble
            let scalec = |c: [f32; 4]| [c[0] * cv, c[1] * cv, c[2] * cv, c[3]];
            let at = |off: f32, y: f32| [cx + n.x * side * off, y, z + n.y * side * off];

            positions.push(at(inner, BANK_Y));
            colors.push(scalec(wet));
            positions.push(at(mid, BANK_Y * 0.55));
            colors.push(scalec(damp));
            positions.push(at(outer, 0.004));
            colors.push(scalec(grass));
        }
        // 3 verts per row → 2 quad strips per cell.
        for i in 0..LEN_SEGMENTS as u32 {
            for c in 0..(COLS - 1) {
                let k = base + i * COLS + c;
                let a = k;
                let b = k + 1;
                let cc = k + COLS;
                let d = k + COLS + 1;
                if side > 0.0 {
                    indices.extend_from_slice(&[a, cc, b, b, cc, d]);
                } else {
                    indices.extend_from_slice(&[a, b, cc, b, d, cc]);
                }
            }
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    // Flat low-poly facets like the other props (must un-index before flat normals).
    mesh.duplicate_vertices();
    mesh.compute_flat_normals();
    mesh
}
