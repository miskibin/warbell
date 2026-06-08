//! Biome framework — the data contract every biome plugs into, plus the runner that
//! turns one [`BiomeConfig`] into a live 32×32 grid scene (ground, scatter, backdrop,
//! particles, set-piece landmarks).
//!
//! Five biomes share ONE pipeline; pressing keys **1–5** swaps the active biome at
//! runtime by despawning everything tagged [`BiomeEntity`] and rebuilding from the new
//! config. The camera, sun and IBL persist across the switch — only atmosphere *values*
//! (fog/ambient/sun colour) are re-applied.
//!
//! ## How a biome is authored
//! Each biome lives in its own `biome_<name>.rs` module exposing exactly two fns:
//!   * `pub fn config() -> BiomeConfig` — declarative: ground colour + detail, atmosphere,
//!     a weighted list of scatter [`PropClass`]es, ground-cover classes, river/ocean
//!     flags, the [`Backdrop`] theme and a [`ParticleKind`].
//!   * `pub fn landmarks(commands, meshes, materials)` — optional bespoke set-pieces
//!     (ruins, oasis, frozen pond…). May be a no-op.
//!
//! The grid is the law: scatter rolls **once per tile** over `[-HALF, HALF]` (tiles ==
//! world units), exactly like the TS game, so densities read as "per-tile chance".

use bevy::light::DirectionalLight;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;

use crate::palette::srgb;
use crate::terrain::{HALF, TerrainMaterial};
use crate::water::WaterMaterial;

/// Distance fog: clear within this many tiles of the camera, ramping to full by [`FOG_FULL`].
/// Pushed out ×1.4 with the enlarged island so view distance keeps pace.
const FOG_CLEAR: f32 = 70.0;
const FOG_FULL: f32 = 160.0;

/// Scatter density multipliers — the original TS game was denser. `SCATTER_DENSITY`
/// scales every main-class per-tile chance; `COVER_DENSITY` scales the ground-cover
/// rolls per tile. One lever each, applied uniformly across all biomes + the grass
/// frontier. Back these down if the enlarged + denser map stutters.
const SCATTER_DENSITY: f32 = 1.35;
const COVER_DENSITY: f32 = 1.5;

/// Fog knobs: `FOREST_FOG="clear,full"` overrides at runtime (no rebuild). `clear` = the
/// fully-clear radius; `full` = distance the fog reaches the horizon colour (smaller = thicker).
fn fog_dist() -> (f32, f32) {
    if let Ok(s) = std::env::var("FOREST_FOG") {
        let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if v.len() == 2 {
            return (v[0], v[1]);
        }
    }
    (FOG_CLEAR, FOG_FULL)
}

// ── The five biomes ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Biome {
    Forest,
    Snow,
    Rocky,
    Desert,
    Swamp,
}

/// Tag on every entity the active biome spawns; the switch despawns these and rebuilds.
#[derive(Component)]
pub struct BiomeEntity;

// ── Declarative biome description ───────────────────────────────────────────────

/// Detail-texture spec → fed to `terrain::detail_image` to bake a seamless ground
/// imprint (the low-opacity `vision.ts` texture). Mirrors the TS `terrainDetail` specs.
#[derive(Clone, Copy)]
pub struct GroundDetail {
    /// World-space tiling scale of the detail texture (shader `detailScale`).
    pub scale: f32,
    /// How strongly the detail imprints onto the flat ground colour (`detailStrength`).
    pub strength: f32,
    /// Large-scale hue/value drift across the terrain (the cure for "flat" ground).
    pub variation: f32,
    /// Texture generator seed + ramp (dark → base → light) + grain/streak amounts.
    pub seed: f32,
    pub dark: u32,
    pub base: u32,
    pub light: u32,
    pub grain: f32,
    pub streak: f32,
}

/// One scatter class: a bag of mesh variants placed with some per-tile probability.
pub struct PropClass {
    /// `(mesh, pick-weight)` — variants chosen among by weight when this class fires.
    pub variants: Vec<(Mesh, f32)>,
    /// Probability slice of the single per-tile (or per-cover-cell) roll.
    pub chance: f32,
    /// Uniform scale range applied per instance.
    pub scale: (f32, f32),
    /// Trees are spacing-checked (no overlapping canopies) and get wind sway; if a tree
    /// fails the spacing test the runner substitutes the first non-tree class instead.
    pub tree: bool,
}

/// Horizon backdrop: a land arc of hills/mountains (+ optional treeline) facing one way,
/// with the opposite arc optionally filled by open ocean — "land on one side, sea on the
/// other". Angles are radians about +X (atan2(z,x) convention).
#[derive(Clone, Copy)]
pub struct Backdrop {
    /// Centre direction of the land arc (radians). Hills/treeline cluster around this.
    pub land_dir: f32,
    /// Half-width of the land arc (radians). Beyond `land_dir ± land_arc` → ocean/empty.
    pub land_arc: f32,
    /// Fill the non-land arc with an animated ocean sheet to the fog horizon.
    pub ocean: bool,
    pub ocean_color: u32,
    /// Hill silhouette tones (body / pale cap / darker foot skirt).
    pub hill_body: u32,
    pub hill_cap: u32,
    pub hill_foot: u32,
    /// A dark conifer treeline band just outside the patch (forest-like edges).
    pub treeline: bool,
    pub treeline_dark: u32,
    pub treeline_mid: u32,
    /// Peak-height range of the hills (lets deserts get low dunes, snow get tall peaks).
    pub hill_h: (f32, f32),
}

/// Ambient weather particle drifting over the patch.
#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Fireflies/Pollen are available presets not used by a biome yet
pub enum ParticleKind {
    None,
    Snow,
    Dust,
    Fireflies,
    Pollen,
    Mist,
}

/// Everything the runner needs to build a biome. Authored declaratively per biome.
pub struct BiomeConfig {
    pub biome: Biome,
    pub name: &'static str,

    // Ground.
    pub ground_color: u32,
    pub ground_roughness: f32,
    pub detail: GroundDetail,

    // Atmosphere / lighting (applied on switch; camera + IBL persist).
    pub sky: u32,
    pub fog_density: f32,
    pub sun_color: u32,
    pub sun_illuminance: f32,
    pub ambient_color: u32,
    pub ambient_brightness: f32,
    /// Sun position (it looks at the origin); controls shadow direction + sky sun disk.
    pub sun_pos: Vec3,

    // Scatter (grid-based: one roll per tile).
    pub seed: u32,
    pub tree_min_dist: f32,
    pub classes: Vec<PropClass>,
    pub cover: Vec<PropClass>,
    pub cover_per_tile: u32,

    // Features.
    pub river: bool,
    /// River water tint (sRGB hex) — blue stream, murky-green swamp, etc.
    pub river_color: u32,
    pub backdrop: Backdrop,
    pub particle: ParticleKind,
}

// ── Plugin + runtime state ──────────────────────────────────────────────────────

/// The combined WORLD MAP (all biomes as wedges around a grass centre), or a single
/// biome filling the whole patch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorldMode {
    Combined,
    Single(Biome),
}

#[derive(Resource)]
pub struct BiomeState {
    pub mode: WorldMode,
}

/// Set to `Some(mode)` to request a (re)build on the next `apply_build` tick. Seeded so
/// the scene builds on frame 1.
#[derive(Resource)]
struct PendingBuild(Option<WorldMode>);

pub struct BiomePlugin;

impl Plugin for BiomePlugin {
    fn build(&self, app: &mut App) {
        let initial = initial_mode();
        app.insert_resource(BiomeState { mode: initial })
            .insert_resource(PendingBuild(Some(initial)))
            .add_systems(Update, (read_switch_keys, apply_build).chain());
    }
}

/// Initial view — the combined WORLD MAP by default; `FOREST_BIOME=forest|snow|rocky|
/// desert|swamp` forces a single-biome full-patch view (used by the screenshot harness).
fn initial_mode() -> WorldMode {
    match std::env::var("FOREST_BIOME").ok().as_deref() {
        Some("forest") => WorldMode::Single(Biome::Forest),
        Some("snow") => WorldMode::Single(Biome::Snow),
        Some("rocky") => WorldMode::Single(Biome::Rocky),
        Some("desert") => WorldMode::Single(Biome::Desert),
        Some("swamp") => WorldMode::Single(Biome::Swamp),
        _ => WorldMode::Combined,
    }
}

/// Registry: map each biome to its authored config. New biomes are added here.
fn config_for(b: Biome) -> BiomeConfig {
    match b {
        Biome::Forest => crate::biome_forest::config(),
        Biome::Snow => crate::biome_snow::config(),
        Biome::Rocky => crate::biome_rocky::config(),
        Biome::Desert => crate::biome_desert::config(),
        Biome::Swamp => crate::biome_swamp::config(),
    }
}

/// Per-biome bespoke set-pieces (called after the generic scatter).
fn landmarks_for(
    b: Biome,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    match b {
        Biome::Forest => crate::biome_forest::landmarks(commands, meshes, materials),
        Biome::Snow => crate::biome_snow::landmarks(commands, meshes, materials),
        Biome::Rocky => crate::biome_rocky::landmarks(commands, meshes, materials),
        Biome::Desert => crate::biome_desert::landmarks(commands, meshes, materials),
        Biome::Swamp => crate::biome_swamp::landmarks(commands, meshes, materials),
    }
}

/// Key **0** / **M** → the combined world map; number keys **1–5** → a single biome
/// full-patch view (Forest/Snow/Rocky/Desert/Swamp).
fn read_switch_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut pending: ResMut<PendingBuild>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
) {
    let pick = if keys.just_pressed(KeyCode::Digit1) {
        Some(WorldMode::Single(Biome::Forest))
    } else if keys.just_pressed(KeyCode::Digit2) {
        Some(WorldMode::Single(Biome::Snow))
    } else if keys.just_pressed(KeyCode::Digit3) {
        Some(WorldMode::Single(Biome::Rocky))
    } else if keys.just_pressed(KeyCode::Digit4) {
        Some(WorldMode::Single(Biome::Desert))
    } else if keys.just_pressed(KeyCode::Digit5) {
        Some(WorldMode::Single(Biome::Swamp))
    } else if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::KeyM) {
        Some(WorldMode::Combined)
    } else {
        None
    };
    if let Some(m) = pick {
        pending.0 = Some(m);
        // UI confirm blip on every switch; the hero speaks a thought when entering a biome.
        cues.write(crate::audio::AudioCue::UiSelect);
        if let WorldMode::Single(b) = m {
            cues.write(crate::audio::AudioCue::HeroLine(b));
        }
    }
}

/// Atmosphere tuple: (sky, fog_density, sun_color, sun_illuminance, ambient_color,
/// ambient_brightness, sun_pos).
type Atmo = (u32, f32, u32, f32, u32, f32, Vec3);

/// The runner. When a build is pending: despawn the old build, build the requested view
/// (world map or single biome), and re-apply atmosphere. Camera/sun/IBL persist.
#[allow(clippy::too_many_arguments)]
fn apply_build(
    mut pending: ResMut<PendingBuild>,
    mut state: ResMut<BiomeState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut terrain_mats: ResMut<Assets<TerrainMaterial>>,
    mut water_mats: ResMut<Assets<WaterMaterial>>,
    existing: Query<Entity, With<BiomeEntity>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut fog_q: Query<&mut DistanceFog>,
    mut sun_q: Query<(&mut DirectionalLight, &mut Transform)>,
) {
    let Some(mode) = pending.0.take() else { return };
    state.mode = mode;

    // Wipe the previous build (incl. the obstacle set wildlife navigates by).
    for e in &existing {
        commands.entity(e).despawn();
    }
    crate::blockers::reset();

    let atmo: Atmo = match mode {
        WorldMode::Single(biome) => {
            let cfg = config_for(biome);
            debug_assert_eq!(cfg.biome, biome, "config_for returned a mismatched biome");

            crate::terrain::spawn_ground(&cfg, &mut commands, &mut meshes, &mut images, &mut terrain_mats);
            scatter_region(&cfg, &mut commands, &mut meshes, &mut std_mats, -HALF, HALF, cfg.river, &|_x, _z| true, &|_x, _z| 0.0);
            crate::distant::spawn_backdrop(&cfg.backdrop, &mut commands, &mut meshes, &mut std_mats);
            if cfg.river {
                crate::water::spawn_river(&mut commands, &mut meshes, &mut water_mats, &mut std_mats, cfg.river_color);
            }
            if cfg.backdrop.ocean {
                crate::sea::spawn_sea(&cfg.backdrop, &mut commands, &mut meshes, &mut water_mats, &mut std_mats);
            }
            crate::particles::spawn(cfg.particle, &mut commands, &mut meshes, &mut std_mats);
            landmarks_for(biome, &mut commands, &mut meshes, &mut std_mats);

            info!("view → {} (single)", cfg.name);
            (cfg.sky, cfg.fog_density, cfg.sun_color, cfg.sun_illuminance, cfg.ambient_color, cfg.ambient_brightness, cfg.sun_pos)
        }
        WorldMode::Combined => {
            crate::worldmap::build(&mut commands, &mut meshes, &mut images, &mut std_mats, &mut terrain_mats, &mut water_mats);
            info!("view → world map");
            crate::worldmap::ATMOSPHERE
        }
    };

    // Atmosphere (camera/sun/IBL persist; just re-tint).
    let (sky, _fog_density, sun_color, sun_illuminance, amb_color, amb_brightness, sun_pos) = atmo;
    *clear = ClearColor(srgb(sky));
    ambient.color = srgb(amb_color);
    ambient.brightness = amb_brightness;
    let (fog_clear, fog_full) = fog_dist();
    for mut fog in &mut fog_q {
        fog.color = srgb(sky);
        // Linear: fully CLEAR within `fog_clear` tiles of the camera, then ramps to the
        // horizon colour by `fog_full`. Gives a hard "see clearly nearby" radius (vs the
        // density-from-0 exponential), matching the DoF clear zone.
        fog.falloff = bevy::pbr::FogFalloff::Linear { start: fog_clear, end: fog_full };
    }
    for (mut light, mut tf) in &mut sun_q {
        light.color = srgb(sun_color);
        light.illuminance = sun_illuminance;
        *tf = Transform::from_translation(sun_pos).looking_at(Vec3::ZERO, Vec3::Y);
    }
}

// ── Generic grid scatter ────────────────────────────────────────────────────────

/// Mulberry32 — the TS deterministic RNG; stable layout across runs.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next() * (hi - lo)
    }
}

/// Pick an index into `weights` proportional to weight, given roll `r` in [0,1).
fn pick_weighted(weights: &[f32], r: f32) -> usize {
    let total: f32 = weights.iter().sum();
    let mut acc = 0.0;
    let target = r * total;
    for (i, w) in weights.iter().enumerate() {
        acc += w;
        if target < acc {
            return i;
        }
    }
    weights.len().saturating_sub(1)
}

/// A class with its variant meshes uploaded to handles (ready to spawn-clone).
struct ClassHandles {
    variants: Vec<Handle<Mesh>>,
    weights: Vec<f32>,
    chance: f32,
    scale: (f32, f32),
    tree: bool,
}

fn upload_classes(src: &[PropClass], meshes: &mut Assets<Mesh>) -> Vec<ClassHandles> {
    src.iter()
        .map(|c| ClassHandles {
            variants: c.variants.iter().map(|(m, _)| meshes.add(m.clone())).collect(),
            weights: c.variants.iter().map(|(_, w)| *w).collect(),
            chance: c.chance,
            scale: c.scale,
            tree: c.tree,
        })
        .collect()
}

/// The grid scatter over `[lo, hi]²`. One roll per tile; classes consume cumulative
/// probability slices (the rest stays empty). Trees are spacing-checked + wind-swayed.
/// `mask(x,z)` gates placement (the world map uses it to keep each biome inside its
/// wedge + off the paths); `river_guard` skips the sine river band when true.
#[allow(clippy::too_many_arguments)]
pub fn scatter_region(
    cfg: &BiomeConfig,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    lo: f32,
    hi: f32,
    river_guard: bool,
    mask: &dyn Fn(f32, f32) -> bool,
    height_fn: &dyn Fn(f32, f32) -> f32,
) {
    // One shared white vertex-colour material — every prop bakes its hue into the mesh,
    // so the renderer auto-batches them into few draw calls.
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });

    let classes = upload_classes(&cfg.classes, meshes);
    let cover = upload_classes(&cfg.cover, meshes);

    // First non-tree class → the "too close" fallback for trees (forest drops a bush).
    let fallback: Option<&ClassHandles> = classes.iter().find(|c| !c.tree);

    let mut r = Rng(cfg.seed);
    let mut tree_pts: Vec<Vec2> = Vec::new();
    let min_d2 = cfg.tree_min_dist * cfg.tree_min_dist;

    // ── Main per-tile scatter ──
    let mut gx = lo;
    while gx < hi {
        let mut gz = lo;
        while gz < hi {
            let cx = gx + 0.5 + r.range(-0.35, 0.35);
            let cz = gz + 0.5 + r.range(-0.35, 0.35);
            if (river_guard && crate::water::on_river(cx, cz)) || !mask(cx, cz) {
                gz += 1.0;
                continue;
            }
            let py = height_fn(cx, cz);
            let roll = r.next();
            let mut acc = 0.0;
            let mut chosen: Option<&ClassHandles> = None;
            for c in &classes {
                acc += c.chance * SCATTER_DENSITY;
                if roll < acc {
                    chosen = Some(c);
                    break;
                }
            }
            if let Some(c) = chosen {
                let vi = pick_weighted(&c.weights, r.next());
                let mesh = c.variants[vi].clone();
                let s = r.range(c.scale.0, c.scale.1);
                if c.tree {
                    let p = Vec2::new(cx, cz);
                    if tree_pts.iter().any(|q| q.distance_squared(p) < min_d2) {
                        // Too close — drop the fallback prop (e.g. a bush) here instead.
                        if let Some(fb) = fallback {
                            let fi = pick_weighted(&fb.weights, r.next());
                            let fs = r.range(fb.scale.0, fb.scale.1);
                            commands.spawn((
                                Mesh3d(fb.variants[fi].clone()),
                                MeshMaterial3d(mat.clone()),
                                Transform {
                                    translation: Vec3::new(cx, py, cz),
                                    rotation: yaw(&mut r),
                                    scale: Vec3::splat(fs),
                                },
                                BiomeEntity,
                            ));
                        }
                    } else {
                        tree_pts.push(p);
                        // Only the TRUNK blocks — a small circle scaled with the instance
                        // (capped ≤ the blockers neighbour-scan bound) so you can walk under
                        // the canopy and brush past, but not through the bole. Small props
                        // (bushes/rocks/barrel cacti/ground cover) register nothing.
                        crate::blockers::add(cx, cz, (0.2 * s).min(0.8));
                        let base = cardinal(&mut r);
                        commands.spawn((
                            Mesh3d(mesh),
                            MeshMaterial3d(mat.clone()),
                            // Identity rotation — wind `Sway` overwrites it each frame.
                            Transform {
                                translation: Vec3::new(cx, py, cz),
                                rotation: Quat::IDENTITY,
                                scale: Vec3::splat(s),
                            },
                            crate::wind::sway_for(cx, cz, base),
                            BiomeEntity,
                        ));
                    }
                } else {
                    commands.spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(mat.clone()),
                        Transform {
                            translation: Vec3::new(cx, py, cz),
                            rotation: yaw(&mut r),
                            scale: Vec3::splat(s),
                        },
                        BiomeEntity,
                    ));
                }
            }
            gz += 1.0;
        }
        gx += 1.0;
    }

    // ── Ground cover: sub-cell rolls per tile ──
    if !cover.is_empty() {
        let cover_count = ((cfg.cover_per_tile as f32) * COVER_DENSITY).round() as u32;
        let mut gx = lo;
        while gx < hi {
            let mut gz = lo;
            while gz < hi {
                for _ in 0..cover_count {
                    let x = gx + r.next();
                    let z = gz + r.next();
                    if (river_guard && crate::water::on_river(x, z)) || !mask(x, z) {
                        continue;
                    }
                    let py = height_fn(x, z);
                    let roll = r.next();
                    let mut acc = 0.0;
                    for c in &cover {
                        acc += c.chance;
                        if roll < acc {
                            let vi = pick_weighted(&c.weights, r.next());
                            let s = r.range(c.scale.0, c.scale.1);
                            commands.spawn((
                                Mesh3d(c.variants[vi].clone()),
                                MeshMaterial3d(mat.clone()),
                                Transform {
                                    translation: Vec3::new(x, py, z),
                                    rotation: yaw(&mut r),
                                    scale: Vec3::splat(s),
                                },
                                bevy::light::NotShadowCaster,
                                BiomeEntity,
                            ));
                            break;
                        }
                    }
                }
                gz += 1.0;
            }
            gx += 1.0;
        }
    }
}

fn cardinal(r: &mut Rng) -> Quat {
    Quat::from_rotation_y((r.next() * 4.0).floor() * std::f32::consts::FRAC_PI_2)
}

fn yaw(r: &mut Rng) -> Quat {
    Quat::from_rotation_y(r.next() * std::f32::consts::TAU)
}
