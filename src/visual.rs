//! Extra visual polish + the live-tunable state behind the Debug panel's "Render" section.
//!
//! Adds two things on top of the existing pipeline (camera post-fx lives in `scene.rs`):
//!   * drifting **pollen / dust motes** that catch the light — "living air";
//!   * a global **prop specular** tweak (roughness / reflectance) so the matte low-poly
//!     props pick up a little form-giving highlight.
//!
//! Tunables live in [`VisualSettings`]; the Debug panel mutates it and
//! [`apply_visual_settings`] pushes the pollen-glow + prop-specular changes onto the
//! materials. Colour-grade + exposure are mutated directly on their live components by the
//! panel.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const POLLEN_COUNT: usize = 60;
const TAU: f32 = std::f32::consts::TAU;
/// Warm pollen glow colour (sRGB → linear in the emissive so bloom catches it).
const POLLEN_TINT: Color = Color::srgb(1.0, 0.93, 0.7);

/// Live-tunable visual knobs not owned by a single Bevy component (driven by the panel).
#[derive(Resource)]
pub struct VisualSettings {
    /// Pollen emissive strength (0 = invisible motes).
    pub pollen_glow: f32,
    /// Pollen drift speed multiplier.
    pub pollen_speed: f32,
    /// Roughness pushed onto the white prop materials (lower = glossier).
    pub prop_roughness: f32,
    /// Reflectance pushed onto the white prop materials (specular strength).
    pub prop_reflectance: f32,
    /// Master bloom multiplier. `advance_sky` computes a per-biome/per-time bloom each frame and
    /// scales it by this, so the F1 panel's bloom slider STICKS (writing `Bloom::intensity` directly
    /// would be stomped next frame by that drive). 1.0 = the authored amount, 0 = off.
    pub bloom: f32,
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            pollen_glow: 3.5,
            pollen_speed: 1.0,
            prop_roughness: 0.62,
            prop_reflectance: 0.50,
            bloom: 1.0,
        }
    }
}

/// Handle to the shared pollen material so the apply system can retune its glow.
#[derive(Resource)]
struct PollenMat(Handle<StandardMaterial>);

/// A drifting mote: bobs + wanders around its spawn point (deterministic per-mote phase).
#[derive(Component)]
struct Pollen {
    base: Vec3,
    phase: f32,
    speed: f32,
    bob: f32,
    drift: f32,
}

pub struct VisualPlugin;

impl Plugin for VisualPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VisualSettings>()
            .add_systems(Startup, (spawn_pollen, spawn_clouds))
            .add_systems(Update, (drift_pollen, drift_clouds, apply_visual_settings));
        if crate::blenderenv::look_enabled() {
            app.add_systems(Update, drive_world_clouds);
        }
    }
}

// ── Low-poly clouds ────────────────────────────────────────────────────────────────
//
// Fluffy clouds in the game's own flat-shaded style: each cloud is a merged cluster of
// squashed white icosphere puffs, lit by the sun (bright top, sky-blue IBL fill on the
// underside) with a little emissive so it never reads as a grey blob. They drift slowly on
// the wind and wrap around, filling the previously-empty sky.

const CLOUD_DRIFT: f32 = 0.6; // world units / sec
const CLOUD_BOUND: f32 = 130.0; // wrap-around half-extent in X

#[derive(Component)]
struct Cloud {
    /// WORLD sky clouds sit on a distant camera-centered ring, like the existing night sky.
    /// Native clouds keep their original fixed world positions.
    sky_offset: Option<Vec3>,
}

#[derive(Resource)]
struct WorldCloudMats(Vec<Handle<StandardMaterial>>);

fn cloud_noise(x: f32, y: f32, seed: u32) -> f32 {
    fn hash(x: i32, y: i32, seed: u32) -> f32 {
        let mut v = (x as u32).wrapping_mul(0x9e37_79b9)
            ^ (y as u32).wrapping_mul(0x85eb_ca6b)
            ^ seed.wrapping_mul(0xc2b2_ae35);
        v ^= v >> 16;
        v = v.wrapping_mul(0x7feb_352d);
        v ^= v >> 15;
        v = v.wrapping_mul(0x846c_a68b);
        v ^= v >> 16;
        v as f32 / u32::MAX as f32
    }
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let a = hash(ix, iy, seed);
    let b = hash(ix + 1, iy, seed);
    let c = hash(ix, iy + 1, seed);
    let d = hash(ix + 1, iy + 1, seed);
    (a + (b - a) * sx) * (1.0 - sy) + (c + (d - c) * sx) * sy
}

/// Soft alpha cloud banks for the opt-in WORLD sky. These are transparent 3D billboards
/// drifting above the playable island, not a screenshot overlay. Four deterministic lobe
/// layouts avoid the repeated polyhedron silhouette of the native cloud meshes.
fn world_cloud_texture(seed: u32) -> Image {
    const W: u32 = 512;
    const H: u32 = 256;
    let mut pixels = vec![0u8; (W * H * 4) as usize];
    let phase = seed as f32 * 1.73;
    let lobes = [
        (-0.66, -0.18, 0.34, 0.34),
        (-0.39, 0.03, 0.37, 0.49),
        (-0.09, 0.18, 0.37, 0.55),
        (0.25, 0.12, 0.40, 0.49),
        (0.59, -0.12, 0.35, 0.35),
    ];
    for y in 0..H {
        for x in 0..W {
            let u = (x as f32 + 0.5) / W as f32 * 2.0 - 1.0;
            let v = 1.0 - (y as f32 + 0.5) / H as f32 * 2.0;
            let mut body = 0.0f32;
            for (i, &(cx, cy, rx, ry)) in lobes.iter().enumerate() {
                let wobble = (phase + i as f32 * 3.1).sin() * 0.075;
                let dx = (u - cx - wobble) / rx;
                let dy = (v - cy - wobble * 0.35) / ry;
                body = body.max((1.0 - dx * dx - dy * dy).max(0.0));
            }
            // A weak connecting shelf holds the puffs together without turning their
            // undersides into the uniform grey lozenge seen in the v3 capture.
            let base = (1.0 - (u / 0.92).powi(4) - ((v + 0.24) / 0.27).powi(2)).max(0.0);
            body = body.max(base * 0.42);
            let coarse = cloud_noise(u * 8.0 + 11.0, v * 7.0 + 5.0, seed);
            let fine = cloud_noise(u * 21.0 + 3.0, v * 17.0 + 13.0, seed + 73);
            let billow = cloud_noise(u * 12.0 + 19.0, v * 11.0 + 7.0, seed + 173);
            body += (coarse - 0.5) * 0.31 + (fine - 0.5) * 0.13;
            let edge = ((body - 0.035) / 0.29).clamp(0.0, 1.0);
            let alpha = edge * edge * (3.0 - 2.0 * edge);
            // Form shading survives the AgX tonemap: bright rounded caps, a cooler
            // underside and irregular inner wisps. The old 0.84–1.0 range became a
            // featureless slab after blending over the pale sky.
            let shade = (0.70 + 0.17 * ((v + 0.35) / 0.95).clamp(0.0, 1.0)
                + 0.13 * body.clamp(0.0, 1.0)
                + 0.15 * (coarse - 0.5) + 0.10 * (billow - 0.5)).clamp(0.62, 1.0) * 255.0;
            let o = ((y * W + x) * 4) as usize;
            pixels[o] = shade as u8;
            pixels[o + 1] = (shade + 3.0).min(255.0) as u8;
            pixels[o + 2] = (shade + 12.0).min(255.0) as u8;
            pixels[o + 3] = (alpha * 249.0).round() as u8;
        }
    }
    Image::new(
        Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// One cloud = 6–9 flattened white puffs clustered into a lozenge. `seed` varies the shape.
fn build_cloud_mesh(seed: u32) -> Mesh {
    let mut s = seed;
    let mut next = move || {
        s = s.wrapping_add(0x6d2b_79f5);
        let mut t = s;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    };
    let n = 6 + (next() * 4.0) as usize;
    let mut parts: Vec<Mesh> = Vec::new();
    for _ in 0..n {
        let rad = 0.8 + next() * 0.7;
        let dx = (next() * 2.0 - 1.0) * 2.4;
        let dz = (next() * 2.0 - 1.0) * 1.1;
        let dy = next() * 0.4;
        parts.push(
            Sphere::new(rad)
                .mesh()
                .ico(1)
                .expect("ico detail in range")
                .scaled_by(Vec3::new(1.0, 0.6, 1.0))
                .translated_by(Vec3::new(dx, dy, dz)),
        );
    }
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one puff");
    for p in it {
        base.merge(&p).expect("cloud puffs share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}

/// Scatter a few dozen clouds across the sky over the patch. Deterministic, persists across
/// biome switches. Lit white material with a touch of emissive so undersides stay bright.
fn spawn_clouds(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Capture aid: a top-down overview shot is blanketed by the cloud layer (32 large puffs at
    // y≈42–80), so let a shot suppress them. No effect on normal play.
    if std::env::var("FOREST_NOCLOUDS").is_ok() || crate::forest_slice::enabled() {
        return;
    }
    if crate::blenderenv::look_enabled() {
        let quad = meshes.add(Rectangle::new(1.0, 1.0));
        let cloud_mats: Vec<_> = (0..4).map(|v| {
            let texture = images.add(world_cloud_texture(v));
            mats.add(StandardMaterial {
                base_color: Color::linear_rgba(2.10, 2.10, 2.10, 0.98),
                base_color_texture: Some(texture),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        }).collect();
        let mut seed = 0xC10D_u32;
        let mut next = || {
            seed = seed.wrapping_add(0x6d2b_79f5);
            let mut t = seed;
            t = (t ^ (t >> 15)).wrapping_mul(t | 1);
            t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
            ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
        };
        for _ in 0..36 {
            let angle = next() * TAU;
            let radius = 155.0 + next() * 30.0;
            let offset = Vec3::new(angle.cos() * radius, 24.0 + next() * 26.0, angle.sin() * radius);
            let width = 28.0 + next() * 25.0;
            let height = width * (0.40 + next() * 0.10);
            commands.spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(cloud_mats[(next() * 4.0) as usize % 4].clone()),
                Transform::from_translation(offset).with_scale(Vec3::new(width, height, 1.0)),
                NotShadowCaster,
                Cloud { sky_offset: Some(offset) },
                crate::game_state::CampaignOnly,
            ));
        }
        commands.insert_resource(WorldCloudMats(cloud_mats));
        return;
    }
    // Same blanket problem in the RTS skirmish: the iso ortho camera looks down through the
    // cloud band the whole match. The layer is spawned regardless (the mode can now flip
    // mid-process) and tagged `CampaignOnly`, so `game_state::apply_mode_visibility` hides it
    // for the arena and re-shows it back in the campaign.
    let cloud_mat = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 1.0, 1.0),
        // Small white emissive keeps the shaded side bright (clouds, not grey rocks).
        emissive: LinearRgba::rgb(0.35, 0.37, 0.42),
        perceptual_roughness: 1.0,
        ..default()
    });
    let shapes: Vec<Handle<Mesh>> = (0..4).map(|v| meshes.add(build_cloud_mesh(0x1234 + v * 977))).collect();

    let mut seed = 0xC10D_u32;
    let mut next = || {
        seed = seed.wrapping_add(0x6d2b_79f5);
        let mut t = seed;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    };
    for _ in 0..32 {
        let x = -CLOUD_BOUND + next() * (CLOUD_BOUND * 2.0);
        let z = -120.0 + next() * 240.0;
        let y = 42.0 + next() * 38.0;
        let s = 5.0 + next() * 10.0;
        commands.spawn((
            Mesh3d(shapes[(next() * 4.0) as usize % 4].clone()),
            MeshMaterial3d(cloud_mat.clone()),
            Transform::from_xyz(x, y, z).with_scale(Vec3::splat(s)),
            NotShadowCaster,
            Cloud { sky_offset: None },
            crate::game_state::CampaignOnly,
        ));
    }
}

/// Slow wind drift on the clouds; wrap back around once they sail past the far edge.
fn drift_clouds(time: Res<Time>, camera: Query<&GlobalTransform, With<Camera3d>>, mut q: Query<(&mut Transform, &mut Cloud)>) {
    let dx = CLOUD_DRIFT * time.delta_secs();
    let blender_world = crate::blenderenv::look_enabled();
    let view_position = if blender_world { camera.single().ok().map(|c| c.translation()) } else { None };
    for (mut tf, mut cloud) in &mut q {
        if let Some(offset) = &mut cloud.sky_offset {
            offset.x += dx;
            if offset.x > 210.0 { offset.x -= 420.0; }
            if let Some(view_position) = view_position {
                tf.translation = view_position + *offset;
                tf.look_at(view_position, Vec3::Y);
            }
        } else {
            tf.translation.x += dx;
            if tf.translation.x > CLOUD_BOUND {
                tf.translation.x -= CLOUD_BOUND * 2.0;
            }
        }
    }
}

fn drive_world_clouds(
    clock: Res<crate::scene::SkyClock>,
    cloud_mats: Option<Res<WorldCloudMats>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(cloud_mats) = cloud_mats else { return; };
    static CLOUD_GAIN: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    let cloud_gain = *CLOUD_GAIN.get_or_init(|| std::env::var("FOREST_WORLD_CLOUD_GAIN")
        .ok().and_then(|s| s.parse::<f32>().ok()).filter(|v| v.is_finite())
        .map(|v| v.clamp(0.3, 2.0)).unwrap_or(1.4));
    let sun_height = (clock.t * TAU).sin();
    let day = ((sun_height + 0.05) / 0.25).clamp(0.0, 1.0);
    let day = day * day * (3.0 - 2.0 * day);
    let dusk = (1.0 - (sun_height / 0.45).clamp(0.0, 1.0)) * day;
    let tint = Color::linear_rgba(
        (0.12 + 1.38 * day) * cloud_gain,
        (0.16 + 1.34 * day - 0.08 * dusk) * cloud_gain,
        (0.25 + 1.25 * day - 0.15 * dusk) * cloud_gain,
        0.10 + 0.88 * day,
    );
    for handle in &cloud_mats.0 {
        if let Some(mut mat) = materials.get_mut(handle) { mat.base_color = tint; }
    }
}

/// Scatter ~150 small unlit emissive motes across the patch, drifting slowly. Deterministic
/// placement (Mulberry32, no `random()`), persists across biome switches (not `BiomeEntity`).
fn spawn_pollen(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let glow = VisualSettings::default().pollen_glow;
    let mesh = meshes.add(Sphere::new(0.03).mesh().ico(1).expect("ico detail in range"));
    let mat = mats.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.96, 0.8),
        emissive: LinearRgba::from(POLLEN_TINT) * glow,
        unlit: true,
        ..default()
    });

    let mut seed = 0x9e37_79b9_u32;
    let mut next = || {
        seed = seed.wrapping_add(0x6d2b_79f5);
        let mut t = seed;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    };
    for _ in 0..POLLEN_COUNT {
        let x = -15.0 + next() * 30.0;
        let z = -15.0 + next() * 30.0;
        let y = 0.5 + next() * 3.2;
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, y, z),
            NotShadowCaster,
            Pollen {
                base: Vec3::new(x, y, z),
                phase: next() * TAU,
                speed: 0.2 + next() * 0.4,
                bob: 0.15 + next() * 0.3,
                drift: 0.2 + next() * 0.5,
            },
        ));
    }
    commands.insert_resource(PollenMat(mat));
}

/// Gentle independent bob + wander per mote, scaled by the live drift-speed knob.
fn drift_pollen(time: Res<Time>, settings: Res<VisualSettings>, mut q: Query<(&mut Transform, &Pollen)>) {
    let t = time.elapsed_secs() * settings.pollen_speed;
    for (mut tf, p) in &mut q {
        let ph = p.phase + t * p.speed;
        tf.translation.x = p.base.x + (ph * 0.6).sin() * p.drift;
        tf.translation.z = p.base.z + (ph * 0.8 + 1.1).cos() * p.drift;
        tf.translation.y = p.base.y + ph.sin() * p.bob;
    }
}

/// Push the panel's pollen-glow + prop-specular knobs onto the materials, only when they
/// actually change (so the GPU upload doesn't churn every frame).
fn apply_visual_settings(
    settings: Res<VisualSettings>,
    pollen_mat: Option<Res<PollenMat>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if !settings.is_changed() {
        return;
    }

    // Pollen glow.
    if let Some(pm) = pollen_mat {
        if let Some(mut m) = mats.get_mut(&pm.0) {
            m.emissive = LinearRgba::from(POLLEN_TINT) * settings.pollen_glow;
        }
    }

    // Prop specular — applied to the white, opaque, non-emissive prop materials (the
    // scatter / landmark `Color::WHITE` mats). Skips water/terrain (own material types),
    // wisps/fireflies/pollen (unlit), and tinted set-pieces (non-white base). Collect ids
    // first to release the immutable borrow before mutating.
    let ids: Vec<_> = mats
        .iter()
        .filter_map(|(id, m)| {
            let c = m.base_color.to_linear();
            let white = c.red > 0.85 && c.green > 0.85 && c.blue > 0.85;
            // White, opaque, non-emissive prop mats only (skips clouds = emissive, wisps =
            // unlit, tinted set-pieces = non-white).
            (white && !m.unlit && m.emissive == LinearRgba::BLACK).then_some(id)
        })
        .collect();
    for id in ids {
        if let Some(mut m) = mats.get_mut(id) {
            m.perceptual_roughness = settings.prop_roughness;
            m.reflectance = settings.prop_reflectance;
        }
    }
}
