//! GPU wind sway for merged ground-cover chunks (grass tufts / flowers / ferns / reed sprigs).
//!
//! The scatter merges each 16×16 chunk's ground cover into ONE mesh sharing a white
//! vertex-colour `StandardMaterial` (`biome.rs`), so a per-entity CPU sway (the one `wind.rs`
//! runs on individual trees) can't reach the blades — there are no per-blade entities. Instead
//! this is a **vertex-shader** bend: an `ExtendedMaterial<StandardMaterial, WindExt>` whose
//! VERTEX stage (`assets/shaders/foliage_wind.wgsl`, + a matching prepass twin) displaces each
//! vertex by a height-weighted sin/cos wander keyed on world XZ. The fragment stays the default
//! StandardMaterial PBR, so the cover lights / fogs / batches exactly as before.
//!
//! Wiring is decoupled from the world build: `biome.rs` still spawns cover chunks on the plain
//! `StandardMaterial` and tags them [`GroundCoverChunk`](crate::biome::GroundCoverChunk);
//! [`swap_cover_material`] re-homes any such chunk onto the shared wind material. That keeps the
//! (param-capped) `worldmap::build` / `scatter_region` signatures untouched, and re-fires for
//! free after any in-process rebuild (biome swap / New Game spawns fresh plain-material chunks).
//!
//! The per-blade bend weight is the blade's LOCAL height (0 at the planted base), baked into
//! vertex COLOR.a by `biome.rs::upload_classes` — the merged mesh bakes terrain elevation into
//! `position.y`, so raw Y can't be the weight. See the shader header for the math + why the
//! prepass must apply the identical offset.

use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

const WIND_SHADER: &str = "shaders/foliage_wind.wgsl";

pub type FoliageWindMaterial = ExtendedMaterial<StandardMaterial, WindExt>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct WindParams {
    /// x = master sway amplitude (world units per unit of blade height), y = gust depth,
    /// z = gust frequency (rad/s), w = unused. Set ONCE at material creation and never mutated —
    /// mutating a material asset every frame re-specializes every mesh using it (~100ms CPU here,
    /// a 10× frame-time regression). Time comes from `globals.time` in the shader instead.
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct WindExt {
    #[uniform(100)]
    pub params: WindParams,
}

impl MaterialExtension for WindExt {
    fn vertex_shader() -> ShaderRef {
        WIND_SHADER.into()
    }
    // NB: no `prepass_vertex_shader` override. The main pass reads `globals.time`, which the prepass
    // view bind group lacks — so a prepass twin can't share the clock without the per-frame material
    // write that tanked perf. Cover is `NotShadowCaster` + tiny; the main-only displacement's depth
    // mismatch against the (undisplaced) prepass is checked visually — see the module perf notes.
}

/// The one shared cover-wind material handle (base params match the scatter's white prop
/// material). [`swap_cover_material`] routes every ground-cover chunk onto it.
#[derive(Resource)]
pub struct FoliageWindMat(pub Handle<FoliageWindMaterial>);

pub struct FoliageWindPlugin;

impl Plugin for FoliageWindPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FoliageWindMaterial>::default())
            .add_systems(Startup, setup_wind_mat)
            .add_systems(Update, swap_cover_material);
    }
}

/// Master sway amplitude, overridable live for harness tuning: `FOREST_WIND=0.24 cargo run`.
/// 0 = still. Default is a gentle meadow breeze.
fn wind_amp() -> f32 {
    std::env::var("FOREST_WIND")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.20)
}

fn setup_wind_mat(mut commands: Commands, mut mats: ResMut<Assets<FoliageWindMaterial>>) {
    let handle = mats.add(ExtendedMaterial {
        // Mirror the scatter's shared prop material (biome.rs::scatter_region) so cover shaded on
        // the wind material is indistinguishable from cover that hasn't been swapped yet.
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.92,
            reflectance: 0.18,
            ..default()
        },
        extension: WindExt {
            // x=amp, y=gust depth, z=gust freq, w=unused. Set once — NEVER mutated per frame.
            params: WindParams {
                params: Vec4::new(wind_amp(), 0.4, 0.35, 0.0),
            },
        },
    });
    commands.insert_resource(FoliageWindMat(handle));
}

/// Route freshly-built ground-cover chunks onto the wind material. Runs every frame, but the
/// query empties once every chunk is swapped (the swap removes the `StandardMaterial` handle it
/// filters on), so it costs nothing after a build settles. Re-fires after any world rebuild
/// because the rebuild spawns new plain-material chunks.
fn swap_cover_material(
    mut commands: Commands,
    wind_mat: Option<Res<FoliageWindMat>>,
    q: Query<
        Entity,
        (
            With<crate::biome::GroundCoverChunk>,
            With<MeshMaterial3d<StandardMaterial>>,
        ),
    >,
) {
    let Some(wind_mat) = wind_mat else { return };
    for e in &q {
        // `try_remove` + `try_insert` are apply-time-safe (silent no-op if the entity is gone):
        // `clear_around_landmarks` reaps some cover chunks the same frame this queues the swap, so
        // a plain `remove` would spam "entity despawned" every build.
        commands
            .entity(e)
            .try_remove::<MeshMaterial3d<StandardMaterial>>()
            .try_insert(MeshMaterial3d(wind_mat.0.clone()));
    }
}
