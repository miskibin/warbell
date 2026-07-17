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
use crate::terrain::TerrainMaterial;
use crate::water::WaterMaterial;

/// Distance fog: clear within this many tiles of the camera, ramping to full by [`FOG_FULL`].
/// Pushed out ×1.4 with the enlarged island so view distance keeps pace.
const FOG_CLEAR: f32 = 100.0; // was 68 — 2026-07-08 visibility pass: fog stays clear much farther out
const FOG_FULL: f32 = 215.0; // was 145 — lighter, reaches horizon later

/// Scatter density multipliers — the original TS game was denser. `SCATTER_DENSITY`
/// scales every main-class per-tile chance; `COVER_DENSITY` scales the ground-cover
/// rolls per tile. One lever each, applied uniformly across all biomes + the grass
/// frontier. Back these down if the enlarged + denser map stutters.
const SCATTER_DENSITY: f32 = 1.35;
const COVER_DENSITY: f32 = 1.8;
/// Extra density in the OPEN woods (away from any path), on top of the base multipliers above.
/// Scaled by `roads::openness` so the boost is full between the roads and fades to 0 at a trail
/// edge — the forest reads thicker where there are no paths, without crowding the trails. Trees
/// hit their `tree_min_dist` spacing first, so the extra tree rolls mostly become undergrowth
/// (the fallback bush), thickening the *understorey* rather than fusing the canopy.
const OPEN_SCATTER_BOOST: f32 = 0.35;
const OPEN_COVER_BOOST: f32 = 0.45;

/// Terrain-aware TREE thinning. Real forests don't carpet cliffs — trees thin on steep faces and
/// bare rock shows through. We estimate the local ground gradient and, above [`SLOPE_FREE`] (gentle
/// rolling hills pass untouched), scale tree density down toward [`SLOPE_TREE_FLOOR`]. Only TREE
/// classes are thinned; low cover/rocks stay, so a steep face reads as bare crag with a little scrub
/// rather than a floating forest on a wall. (Gradient is world-Y per world-unit; `GROUND_STEP`=0.5.)
const SLOPE_FREE: f32 = 0.35;
const SLOPE_TREE_THIN: f32 = 1.2;
const SLOPE_TREE_FLOOR: f32 = 0.1;
/// Tree spacing is authored as centre-to-centre, but cross-biome scatter passes don't share the
/// local `tree_pts` list. This blocker clearance makes later passes respect earlier trunks too.
const TREE_BLOCKER_CLEAR_FRAC: f32 = 0.8;

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

// ── Per-region ambience (atmosphere + weather), reactive to the hero's biome ──────

/// One biome's atmosphere, pre-converted to renderer types. Captured ONCE from each
/// biome's [`config()`] at world build (see [`crate::worldmap::build`]) so the per-region
/// transition system can lerp toward it every frame WITHOUT rebuilding configs (which bake
/// meshes). Reading the config here is what keeps those `BiomeConfig` atmosphere fields live.
#[derive(Clone, Copy)]
pub struct AtmoSample {
    pub sky: Color,
    pub sun_color: Color,
    pub sun_illuminance: f32,
    pub ambient_color: Color,
    pub ambient_brightness: f32,
    /// Authored fog thickness (TS `fog_density`). `scene::advance_sky` maps this to a per-region
    /// fog distance so dense biomes (swamp/Blight) pull the fog wall in close, clear ones (forest)
    /// keep the open-island distance. Eased like the colours so crossing a region edge ramps the
    /// fog rather than popping. Previously dead authored data.
    pub fog_density: f32,
    /// Per-region bloom multiplier (1.0 = the island base). `scene::advance_sky` scales the camera
    /// `Bloom` by this (× a dusk/night swell) so the Blight's embers glow hot, the swamp reads
    /// muted, snow/desert sparkle. Eased like the colours so crossing a region edge ramps it.
    pub bloom_scale: f32,
    /// Per-region AMBIENT-fill multiplier (1.0 = the island base). `scene::advance_sky` scales the
    /// global ambient brightness by this in daylight. Prior swamp/Blight darkness fixes all bumped
    /// the SUN (lit side) + fog, but the dense vertical props (mangroves, snags, fortress timber)
    /// read as black silhouettes off-noon because their SHADOW side is fed only by ambient — so the
    /// marsh + mire still looked "too dark / everything shadowed" (player). This lifts the fill in
    /// those regions only, rescuing the shadow side without washing the sun-keyed ground.
    pub ambient_scale: f32,
}

impl AtmoSample {
    /// Pull the atmosphere out of a biome's full config (reads the atmosphere fields). The
    /// sun *position* isn't sampled — `scene`'s day/night clock owns the sun direction.
    pub fn from_config(c: &BiomeConfig) -> Self {
        Self {
            sky: srgb(c.sky),
            sun_color: srgb(c.sun_color),
            sun_illuminance: c.sun_illuminance,
            ambient_color: srgb(c.ambient_color),
            ambient_brightness: c.ambient_brightness,
            fog_density: c.fog_density,
            // Bloom is keyed off the biome here (rather than threaded through every `BiomeConfig`
            // literal): the swamp reads damp + muted, the bright snow/desert sparkle a touch, the
            // rest sit at the island base. The Blight's hot-ember bloom is set in `blight_ambience`.
            bloom_scale: match c.biome {
                Biome::Swamp => 0.85,
                Biome::Snow | Biome::Desert => 1.12,
                _ => 1.0,
            },
            // Lift the swamp's shadow-side fill so its dense mangroves/snags stop crushing to black
            // off-noon (the Blight sets its own, higher, in `blight_ambience`).
            ambient_scale: match c.biome {
                Biome::Swamp => 1.75,
                _ => 1.0,
            },
        }
    }
    /// Build from the island-wide [`crate::worldmap::ATMOSPHERE`] tuple (the grass/ocean base).
    pub fn from_raw(
        sky: u32,
        sun_color: u32,
        sun_illuminance: f32,
        ambient_color: u32,
        ambient_brightness: f32,
        fog_density: f32,
    ) -> Self {
        Self {
            sky: srgb(sky),
            sun_color: srgb(sun_color),
            sun_illuminance,
            ambient_color: srgb(ambient_color),
            ambient_brightness,
            fog_density,
            // Island base: neutral bloom. Callers that want a hotter glow (the Blight) bump it.
            bloom_scale: 1.0,
            // Island base: neutral ambient. The Blight bumps it in `blight_ambience`.
            ambient_scale: 1.0,
        }
    }
}

/// A biome's full ambience: atmosphere + which weather particle drifts over it.
#[derive(Clone, Copy)]
pub struct BiomeAmbience {
    pub atmo: AtmoSample,
    pub particle: ParticleKind,
}

/// Captured at world build: the island base (grass/ocean) ambience + one per biome + the
/// Blight's bespoke red-ember ambience. The atmosphere system and the weather system both read
/// this to know the hero region's target.
#[derive(Resource)]
pub struct BiomeAmbiences {
    pub base: BiomeAmbience,
    pub list: Vec<(Biome, BiomeAmbience)>,
    /// The ork castle (the Blight) reads as [`Biome::Swamp`] to gameplay, but its *mood* is its
    /// own — a sooty blood-red horizon + ash, not swamp green. Surfaced only by [`Self::sample_world`].
    pub blight: BiomeAmbience,
}

impl BiomeAmbiences {
    /// The ambience for the biome the hero is over (`None` = grass/sand/water → base).
    pub fn sample(&self, b: Option<Biome>) -> BiomeAmbience {
        match b {
            Some(b) => self.list.iter().find(|(k, _)| *k == b).map(|(_, a)| *a).unwrap_or(self.base),
            None => self.base,
        }
    }
    /// World-space ambience lookup: the Blight gets its own red-ember mood; everywhere else falls
    /// back to the biome-keyed [`Self::sample`]. This is the single lookup the per-region
    /// atmosphere + weather systems use, so the ork castle stops inheriting swamp's grey-green sky.
    pub fn sample_world(&self, wx: f32, wz: f32) -> BiomeAmbience {
        if crate::ork_fortress::in_blight_world(wx, wz) {
            self.blight
        } else {
            self.sample(crate::worldmap::biome_at_world(wx, wz))
        }
    }
}

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
#[derive(Default)]
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
    /// Collision radius at unit scale for a NON-tree prop — a big boulder blocks, small
    /// clutter doesn't. `0.0` (default) = walk-through. The blocker registered is
    /// `block_radius * instance_scale`, capped at the `blockers` neighbour-scan bound (≤ 1.0).
    /// Ignored for `tree` classes (the trunk gets its own circle).
    pub block_radius: f32,
}

/// Horizon backdrop: a land arc of hills/mountains (+ optional treeline) facing one way,
/// with the opposite arc optionally filled by open ocean — "land on one side, sea on the
/// other". Angles are radians about +X (atan2(z,x) convention).
///
/// Authored per biome but **not yet wired into the world map** (which uses open ocean + fog
/// instead of horizon hills) — kept for a future per-region backdrop renderer.
#[derive(Clone, Copy)]
#[allow(dead_code)] // reserved: no horizon-backdrop renderer on the world map yet
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
#[allow(dead_code)] // Fireflies is an available preset not used by a biome yet
pub enum ParticleKind {
    None,
    Snow,
    Dust,
    Fireflies,
    Pollen,
    Mist,
    /// Dark embery motes drifting + rising over the Blight (the ork castle burned the forest).
    Ash,
}

/// Everything that describes a biome. The world map consumes the **scatter** fields (per-tile
/// props), the **atmosphere** fields (sampled into [`AtmoSample`] for the per-region light
/// tint in `scene::advance_sky`) and **particle** (per-region weather). The rest —
/// `biome`/`name`, `ground_*`/`detail`, `fog_density`, `sun_pos`, `river*`/`backdrop` — are
/// authored but not yet wired into the world map; kept for future per-region work.
#[allow(dead_code)] // some fields are authored-but-not-yet-wired biome data (see doc above)
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
/// Seeded `true` so the world map builds on the first `apply_build` tick (after the
/// scene's camera/sun/fog exist), then flips `false` — the map is the only view.
/// `pub(crate)` + public field so the **in-process reset** (`game_state`) can re-arm a full
/// world rebuild (the same despawn-`BiomeEntity`-and-rebuild path a biome swap uses).
#[derive(Resource)]
pub(crate) struct PendingBuild(pub bool);

/// Set `true` by [`apply_build`] once the world map has been built (boot **and** every rebuild).
/// The loading veil (`loading.rs`) reveals only once this is up — a robust signal that replaces
/// the old "any `BiomeEntity` exists" probe. The reset path clears it before re-arming a rebuild,
/// so the veil holds over the rebuild and lifts when the fresh world lands.
#[derive(Resource, Default)]
pub(crate) struct WorldReady(pub bool);

/// Seconds to let the loading veil present (and its progress bar show at 0) BEFORE the first build
/// phase runs. Each phase blocks the frame it runs on, so without this the first phase fires on the
/// very frame the veil is raised — before the render ever presents the cover. Skipped under the
/// capture harnesses (`FOREST_SHOT`/`FOREST_CLIP`), which want the world up on frame 0.
const BUILD_WARMUP: f32 = 0.4;

/// Drives the chunked world build: one [`crate::worldmap::build_step`] phase per frame so the
/// render loop — and the loading veil — ticks between phases instead of freezing for the whole
/// ~2 s build. Re-armed by [`PendingBuild`] on boot and every in-process rebuild.
#[derive(Resource, Default)]
pub(crate) struct BuildJob {
    /// True while a build is mid-flight (armed → final phase).
    building: bool,
    /// Next phase index to run.
    step: u32,
    /// `elapsed_secs` when the build was armed — drives the [`BUILD_WARMUP`] veil-present delay.
    armed_at: f32,
    /// Set once the pre-build wipe (despawn old `BiomeEntity` + blocker/castle reset) has run.
    wiped: bool,
    /// Data produced by one phase and consumed by a later one (carried across frames).
    state: crate::worldmap::BuildState,
}

/// 0..1 world-build progress, read by the loading veil's progress bar: reset to 0 when a build is
/// armed, climbs as phases complete, pinned at 1.0 once the world is up.
#[derive(Resource, Default)]
pub(crate) struct BuildProgress(pub f32);

pub struct BiomePlugin;

impl Plugin for BiomePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingBuild(true))
            .init_resource::<WorldReady>()
            .init_resource::<BuildJob>()
            .init_resource::<BuildProgress>()
            .init_resource::<crate::worldmap::ActiveMap>()
            .add_systems(
                Update,
                (
                    // Only run while a build is pending/in-progress. Once the world is finished,
                    // leaving it scheduled every frame is a no-op that still holds `ResMut` on SIX
                    // asset stores, serialising it against every render-extraction/spawn system that
                    // touches them — ≈1.4 ms/frame of pure scheduling drag (measured via trace_chrome).
                    drive_build.run_if(build_active),
                    apply_world_atmosphere.run_if(resource_changed::<WorldReady>),
                ),
            );
    }
}

/// `drive_build`'s run-condition: true only while a build is queued or running.
fn build_active(pending: Res<PendingBuild>, job: Res<BuildJob>) -> bool {
    pending.0 || job.building
}

/// Atmosphere tuple: (sky, fog_density, sun_color, sun_illuminance, ambient_color,
/// ambient_brightness, sun_pos).
type Atmo = (u32, f32, u32, f32, u32, f32, Vec3);

/// The world-build driver. When [`PendingBuild`] is raised it arms a [`BuildJob`], waits
/// [`BUILD_WARMUP`] so the veil presents, wipes the prior world ONCE, then runs one
/// [`crate::worldmap::build_step`] phase per frame — so the loading screen animates between
/// phases — until the world is up (`WorldReady`). Capture harnesses run every phase in one frame.
#[allow(clippy::too_many_arguments)]
fn drive_build(
    mut pending: ResMut<PendingBuild>,
    mut job: ResMut<BuildJob>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut terrain_mats: ResMut<Assets<TerrainMaterial>>,
    mut water_mats: ResMut<Assets<WaterMaterial>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    existing: Query<Entity, With<BiomeEntity>>,
    mut castle_built: ResMut<crate::castle::CastleBuilt>,
    mut world_ready: ResMut<WorldReady>,
    active_map: Res<crate::worldmap::ActiveMap>,
    mut progress: ResMut<BuildProgress>,
    time: Res<Time>,
) {
    // Arm a fresh build whenever PendingBuild is raised (boot + every in-process rebuild).
    if pending.0 {
        pending.0 = false;
        job.building = true;
        job.step = 0;
        job.wiped = false;
        job.armed_at = time.elapsed_secs();
        job.state = crate::worldmap::BuildState::default();
        progress.0 = 0.0;
        // Point the (resource-blind) generator at the chosen map BEFORE building. `tiles()`
        // memoises per map, so this just selects which grid the build phases read.
        crate::worldmap::set_active_map(active_map.0);
    }
    if !job.building {
        return;
    }

    let capturing = std::env::var("FOREST_SHOT").is_ok() || std::env::var("FOREST_CLIP").is_ok();

    // Hold the first phase off until the veil has presented (player sees the cover + bar at 0, not
    // a frozen frame), then wipe the prior world ONCE, just before the first spawn.
    if !job.wiped {
        if !capturing && time.elapsed_secs() - job.armed_at < BUILD_WARMUP {
            return;
        }
        // `try_despawn`: a BiomeEntity may already be queued for reap by combat/AI the frame a
        // rebuild fires. Reset blockers + the castle's "already built" tracking so `sync_castle`
        // re-registers collision for the rebuilt walls/houses (else you'd walk through them).
        for e in &existing {
            commands.entity(e).try_despawn();
        }
        crate::blockers::reset();
        *castle_built = crate::castle::CastleBuilt::default();
        job.wiped = true;
    }

    // Run phases: one per frame normally (so the veil animates between them), all at once under a
    // capture (the harness wants a finished world to warm up on, not the veil).
    loop {
        let step = job.step;
        crate::worldmap::build_step(
            step,
            &mut commands,
            &mut meshes,
            &mut images,
            &mut std_mats,
            &mut terrain_mats,
            &mut water_mats,
            &mut creature_mats,
            &mut job.state,
        );
        job.step += 1;
        if job.step >= crate::worldmap::BUILD_STEPS || !capturing {
            break;
        }
    }

    progress.0 = (job.step as f32 / crate::worldmap::BUILD_STEPS as f32).min(1.0);
    if job.step >= crate::worldmap::BUILD_STEPS {
        job.building = false;
        progress.0 = 1.0;
        world_ready.0 = true; // veil reveals; apply_world_atmosphere re-tints sky/sun/fog next frame
        info!("view → world map");
    }
}

/// Re-tint sky / sun / fog / ambient to the active map's atmosphere when a world build finishes
/// (`WorldReady` flips true). Runs on any `WorldReady` change; the reset's true→false change is a
/// no-op. Split out of [`drive_build`] so that driver stays under Bevy's system-param cap.
fn apply_world_atmosphere(
    world_ready: Res<WorldReady>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut fog_q: Query<&mut DistanceFog>,
    // `With<Sun>`: the moon (scene.rs) is a second DirectionalLight — don't repaint it.
    mut sun_q: Query<(&mut DirectionalLight, &mut Transform), With<crate::scene::Sun>>,
) {
    if !world_ready.0 {
        return;
    }
    let atmo: Atmo = crate::worldmap::active_atmosphere();
    let (sky, _fog_density, sun_color, sun_illuminance, amb_color, amb_brightness, sun_pos) = atmo;
    *clear = ClearColor(srgb(sky));
    ambient.color = srgb(amb_color);
    ambient.brightness = amb_brightness;
    let (fog_clear, fog_full) = fog_dist();
    for mut fog in &mut fog_q {
        fog.color = srgb(sky);
        // Linear: fully CLEAR within `fog_clear` tiles, then ramps to the horizon by `fog_full`.
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

/// A class with its (tinted) variant meshes prepared for placement.
///
/// Passive (`tree == false`) props are **merged** into per-chunk meshes (see
/// [`scatter_region`]'s chunk-merge), so we keep the raw [`Mesh`] data around to feed
/// `Mesh::transformed_by` + `Mesh::merge`. Trees stay individual entities (they carry
/// [`crate::verbs::ChopTree`] + wind [`Sway`] and so can't be baked into a shared mesh),
/// so their variants are *also* uploaded to handles up front — every tree instance shares
/// one handle per variant, which the renderer auto-batches.
struct ClassHandles {
    /// The tinted variant meshes (post tree/bush/cover build pipeline — already
    /// flat-normalled with vertex `ATTRIBUTE_COLOR`). Kept as data so passive props can be
    /// transformed-and-merged into chunk meshes.
    variants: Vec<Mesh>,
    /// Per-variant uploaded handle — populated ONLY for `tree` classes (individual entities
    /// share these); empty for passive classes, which merge their `variants` data instead.
    handles: Vec<Handle<Mesh>>,
    weights: Vec<f32>,
    /// Per-variant UNIT-scale trunk-collision radius — populated ONLY for `tree` classes (each
    /// kind's footprint derived from its own silhouette so a wide crown blocks wider than a slim
    /// pole); empty otherwise. Aligned 1:1 with `variants`/`handles`.
    block_radii: Vec<f32>,
    chance: f32,
    scale: (f32, f32),
    tree: bool,
    block_radius: f32,
}

/// Universal per-variant tint spread, baked at upload time: EVERY scattered prop in EVERY
/// biome (trees, bushes, rocks, flowers, mushrooms, litter…) gets warm-light / cool-dark
/// siblings, so no world entity is the one identical colour repeated forever. Deliberately
/// subtle — hue families (flower petals, foliage greens) stay recognisable; the stronger
/// class-specific spreads (e.g. `TREE_TINTS`) stack on top of this one.
const PROP_TINTS: [[f32; 3]; 3] = [
    [1.0, 1.0, 1.0],
    [1.09, 1.05, 0.94], // warmer + lighter
    [0.87, 0.92, 0.90], // cooler + darker
];

fn upload_classes(src: &[PropClass], meshes: &mut Assets<Mesh>) -> Vec<ClassHandles> {
    src.iter()
        .map(|c| {
            let n = c.variants.len() * PROP_TINTS.len();
            let mut variants = Vec::with_capacity(n);
            let mut weights = Vec::with_capacity(n);
            let mut block_radii = Vec::with_capacity(if c.tree { n } else { 0 });
            for (m, w) in &c.variants {
                // Trees collide on a per-kind footprint sized from the source mesh silhouette
                // (cheap, once per variant — the ×PROP_TINTS copies share the same geometry).
                let tree_r = if c.tree {
                    crate::trees::silhouette_block_radius(m)
                } else {
                    0.0
                };
                for t in PROP_TINTS {
                    // Bake the per-variant tint into the raw mesh, then NORMALISE its attribute
                    // set: drop UV_0 so every passive variant in the biome carries the exact same
                    // attributes (POSITION/NORMAL/COLOR). `Mesh::merge` extends `self`'s attributes
                    // from `other` and silently corrupts the result if the sets differ — and we
                    // never sample a texture (the shared white material reads vertex colour), so
                    // UVs are dead weight in a merged chunk anyway.
                    let mut mesh = crate::trees::tint_mesh(m.clone(), t);
                    mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
                    variants.push(mesh);
                    weights.push(*w / PROP_TINTS.len() as f32);
                    if c.tree {
                        block_radii.push(tree_r);
                    }
                }
            }
            // Trees spawn as individual entities sharing one handle per variant; passive props
            // merge their `variants` data into chunk meshes, so they need no handles.
            let handles = if c.tree {
                variants.iter().map(|m| meshes.add(m.clone())).collect()
            } else {
                Vec::new()
            };
            ClassHandles {
                variants,
                handles,
                block_radii,
                weights,
                chance: c.chance,
                scale: c.scale,
                tree: c.tree,
                block_radius: c.block_radius,
            }
        })
        .collect()
}

/// Chunk edge length (world units / tiles) for the passive-prop merge. ~16×16 tiles per
/// chunk keeps each merged mesh modestly sized (a few hundred small props) while collapsing
/// the ~60k individual scatter entities down to ≈2 merged entities per occupied chunk.
pub const COVER_CHUNK: f32 = 16.0;
const CHUNK: f32 = COVER_CHUNK;

/// Marker on a merged ground-cover chunk (grass tufts / flowers / clover). Lets
/// `landmarks::clear_around_landmarks` strip cover that was scattered before the set-pieces
/// were planted.
#[derive(Component)]
pub struct GroundCoverChunk;

/// A passive prop queued for its chunk: its (already tinted, UV-stripped) mesh and the world
/// transform to bake into the vertices. Accumulated, then merged per chunk in [`spawn_chunks`].
struct PendingProp {
    mesh: Mesh,
    transform: Transform,
}

/// One spatial chunk's two merge buckets, keyed by integer chunk coords.
#[derive(Default)]
struct ChunkBucket {
    /// Small ground cover (tufts/flowers/clover/mushrooms/ferns/litter): no shadow, fades out.
    cover: Vec<PendingProp>,
    /// Larger passive scatter (bushes/rocks/litter): casts shadows, no distance fade.
    props: Vec<PendingProp>,
}

/// World-XZ → integer chunk coordinate.
fn chunk_key(x: f32, z: f32) -> (i32, i32) {
    ((x / CHUNK).floor() as i32, (z / CHUNK).floor() as i32)
}

/// Centre of the chunk that owns `(key)` — the chunk entity's translation. Prop vertices are
/// baked RELATIVE to this, so the merged mesh's auto-computed `Aabb` stays local (not a
/// world-spanning box) and the entity transform places it correctly.
fn chunk_center(key: (i32, i32)) -> Vec3 {
    Vec3::new(key.0 as f32 * CHUNK + CHUNK * 0.5, 0.0, key.1 as f32 * CHUNK + CHUNK * 0.5)
}

/// Merge each chunk bucket's queued props into ONE mesh and spawn ONE entity per non-empty
/// bucket. Cover buckets skip the shadow pass and fade out before the fog hides them; props
/// buckets keep shadows. Every chunk entity is tagged [`BiomeEntity`] so the biome-swap path
/// (keys 1–5) despawns + rebuilds them with the rest of the scene.
/// Whether to attach the scatter distance-culling (`VisibilityRange`) added for perf. `FOREST_NOCULL=1`
/// turns it OFF so the win can be measured cleanly: pin the same spot with `FOREST_HERO`, run once
/// with and once without, and compare the F2 `main_opaque_pass` timing. (A raw baseline-vs-after
/// comparison at different camera positions confounds the number — opaque cost is view-dependent.)
/// Read once via `OnceLock` so the per-tree spawn loop doesn't hit the environment each instance.
fn scatter_cull_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_NOCULL").is_err())
}

/// `FOREST_NOGRASS=1`: don't spawn the ground-cover chunk meshes — a profiling toggle to isolate how
/// much of the (fragment-bound) opaque pass is grass overdraw vs the terrain shader vs everything else.
fn no_grass() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_NOGRASS").is_ok())
}

fn spawn_chunks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    chunks: std::collections::HashMap<(i32, i32), ChunkBucket>,
) {
    // Cover cuts out abruptly at 62 units — small ground cover (grass/flowers/mushrooms) is a tiny
    // speck at range, so culling it trims overdraw + distant aliased clutter with little visible loss.
    // The cutoff is pushed PAST the fog start (FOG_BASE_START ≈ 56 after the 2026-07 visibility
    // pass; was 55u vs start 46) so a chunk vanishes inside the haze, not in crystal-clear air —
    // earlier (38u) the cutoff sat in front of the fog ramp, so you could watch each flower chunk
    // snap in/out as you walked. An ABRUPT range (empty crossfade band, `is_abrupt()` true) is
    // deliberate: a non-empty band routes the chunk through the dithered-crossfade pipeline
    // (per-fragment `discard`), which defeats early-z on these large merged meshes. Collapsing both
    // margins keeps it abrupt while preserving `use_aabb: true`.
    let cover_range = bevy::camera::visibility::VisibilityRange {
        start_margin: 0.0..0.0,   // always visible up close
        end_margin: 62.0..62.0,   // abrupt cutoff at 62 world units — inside the fog ramp so it hides
        use_aabb: true,
    };
    for (key, bucket) in chunks {
        let center = chunk_center(key);
        if let Some(mesh) = merge_props(bucket.cover).filter(|_| !no_grass()) {
            let mut e = commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(center),
                bevy::light::NotShadowCaster,
                GroundCoverChunk,
                BiomeEntity,
            ));
            if scatter_cull_enabled() {
                e.insert(cover_range.clone());
            }
        }
        if let Some(mesh) = merge_props(bucket.props) {
            let mut e = commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(center),
                BiomeEntity,
            ));
            // Props (bushes/rocks/litter) cut out farther than cover (75 vs 55) — they read at a
            // greater distance — and stop casting shadows: a fogged-out bush past the cascade max
            // (≈150) only adds shadow-pass + opaque-pass cost. Same ABRUPT range as cover (empty
            // band) so it keeps early-z. Gated by `FOREST_NOCULL` for A/B measurement.
            if scatter_cull_enabled() {
                e.insert((
                    bevy::light::NotShadowCaster,
                    bevy::camera::visibility::VisibilityRange {
                        start_margin: 0.0..0.0,
                        // 75u: props are low bushes/rocks — small enough at range that the abrupt
                        // cutoff hides in the fog ramp (FOG_BASE_START 56 → END 142). Tighter than
                        // trees (kept at 180), which are tall enough that an earlier cull would read
                        // as a "forest edge" on a clear day (fog can push past 300 in clear biomes).
                        end_margin: 75.0..75.0,
                        use_aabb: true,
                    },
                ));
            }
        }
    }
}

/// Bake each prop's transform into its vertices and merge the bucket into one mesh
/// (`None` if empty). All parts share POSITION/NORMAL/COLOR (UV stripped at upload), so the
/// merge always succeeds; the transform is already relative to the chunk centre.
///
/// Cover props MIX indexed-ness: the grass tuft is NON-indexed (its `blade()`s call
/// `duplicate_vertices` for the crisp flat facets), while clover/fern/flower/mushroom stay
/// INDEXED. `Mesh::merge` only concatenates index buffers when BOTH meshes are indexed —
/// otherwise it appends the vertices but DROPS the incoming indices (see bevy_mesh `merge`).
/// So an indexed prop merged onto a non-indexed base (any chunk whose first cover prop is a
/// grass tuft) loses its triangle ordering and renders as huge garbage triangles fanning
/// across the chunk. Normalise every part to non-indexed FIRST so the merge is always a clean
/// vertex-soup concat regardless of bucket order. (Cover/scatter meshes are tiny, so the extra
/// duplicated verts are negligible; trees are spawned as individual entities, not bucketed.)
fn merge_props(props: Vec<PendingProp>) -> Option<Mesh> {
    fn unindex(mut m: Mesh) -> Mesh {
        if m.indices().is_some() {
            m.duplicate_vertices(); // → non-indexed (panics on None indices, hence the guard)
        }
        m
    }
    let mut it = props.into_iter();
    let first = it.next()?;
    let mut base = unindex(first.mesh.transformed_by(first.transform));
    for p in it {
        let part = unindex(p.mesh.transformed_by(p.transform));
        base.merge(&part).expect("scatter props share attributes");
    }
    Some(base)
}

// ── Ground "fertility" field — the cure for "vegetation doesn't match the ground". ───────────
// The terrain shader (`terrain.wgsl`) paints the meadow as drifting patches: worn/bald trodden
// dirt, damp moss hollows, sun-dried golden sweeps, lush well-watered green, and grey-tan bare-
// earth blotches (the "szare placki"). But the cover scatterer rolled a FIXED per-tile chance
// everywhere — a uniform salt-and-pepper carpet that ignored all of that. So the ground and the
// plants read as two unrelated layers: lush flowers sat on bald dirt, bare turf on rich green.
//
// These helpers port the shader's low-freq value noise to the CPU EXACTLY (same `ter_hash` /
// `ter_noise`, same world-XZ frequencies + offsets, same `fract = x - floor(x)`), so the cover
// loop can thin the scatter on the very patches the albedo already paints bald and thicken it in
// the lush hollows. Result: plants grow in drifts that sit ON the green, clearings open on the
// trodden/tan patches — vegetation and ground finally agree, and the field reads varied (drifts
// + clearings) instead of an even sprinkle. Match the WGSL or the drift won't land on its patch.
fn fg_fract(v: f32) -> f32 {
    v - v.floor()
}

fn ter_hash(px: f32, py: f32) -> f32 {
    // WGSL: p3 = fract(vec3(p.x,p.y,p.x)*0.1031); p3 += dot(p3, p3.yzx+33.33); return fract((p3.x+p3.y)*p3.z)
    let mut p = Vec3::new(fg_fract(px * 0.1031), fg_fract(py * 0.1031), fg_fract(px * 0.1031));
    let d = p.dot(Vec3::new(p.y, p.z, p.x) + 33.33);
    p += Vec3::splat(d);
    fg_fract((p.x + p.y) * p.z)
}

fn ter_noise(px: f32, py: f32) -> f32 {
    let (ix, iy) = (px.floor(), py.floor());
    let (fx, fy) = (px - ix, py - iy);
    let a = ter_hash(ix, iy);
    let b = ter_hash(ix + 1.0, iy);
    let c = ter_hash(ix, iy + 1.0);
    let d = ter_hash(ix + 1.0, iy + 1.0);
    // Quintic (C2) fade, matching the shader so patch edges line up.
    let ux = fx * fx * fx * (fx * (fx * 6.0 - 15.0) + 10.0);
    let uy = fy * fy * fy * (fy * (fy * 6.0 - 15.0) + 10.0);
    a + (b - a) * ux + (c - a) * uy * (1.0 - ux) + (d - b) * ux * uy
}

fn fg_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What the ground is doing at a point, read off the SAME patch fields the terrain shader tints
/// with — so the scatter echoes the painted ground instead of fighting it.
pub struct GroundPatch {
    /// 0 ≈ bald/worn/bare-soil (cover thins to bare), 1 ≈ lush hollow (dense, flowery). Drives
    /// scatter DENSITY.
    pub fertility: f32,
    /// −1 ≈ sun-dried / trodden (grass + sun-flowers), +1 ≈ damp green hollow (ferns / clover /
    /// mushrooms). Drives which SPECIES wins, so the plant matches the patch it grows on.
    pub damp: f32,
}

/// Sample the meadow patches once and derive both density (fertility) and species lean (damp).
/// Centred so the open turf stays well-covered and only the clearly worn/dry/bare patches open up.
pub fn ground_patch(x: f32, z: f32) -> GroundPatch {
    let worn = fg_smoothstep(0.48, 0.82, ter_noise(x * 0.020 + 5.0, z * 0.020 + 9.0));
    let moss = fg_smoothstep(0.52, 0.86, ter_noise(x * 0.040 + 19.0, z * 0.040 + 2.0));
    let dry = fg_smoothstep(0.54, 0.87, ter_noise(x * 0.015 + 33.0, z * 0.015 + 7.0));
    let lush = fg_smoothstep(0.56, 0.89, ter_noise(x * 0.030 + 2.0, z * 0.030 + 27.0));
    let soil = fg_smoothstep(0.60, 0.78, ter_noise(x * 0.06 + 61.0, z * 0.06 + 13.0));
    GroundPatch {
        // Lush/moss thicken the carpet; worn/dry thin it; the bare-soil blotch nearly clears it.
        fertility: (0.74 + lush * 0.45 + moss * 0.26 - worn * 0.42 - dry * 0.24 - soil * 0.95).clamp(0.04, 1.0),
        // Damp green hollows ↔ sun-dried/trodden sweeps.
        damp: ((moss * 0.7 + lush * 0.5) - (dry * 0.8 + worn * 0.5)).clamp(-1.0, 1.0),
    }
}

/// The grid scatter over `[lo, hi]²`. One roll per tile; classes consume cumulative
/// probability slices (the rest stays empty). Trees are spacing-checked + wind-swayed.
/// `mask(x,z)` gates placement (the world map uses it to keep each biome inside its
/// wedge + off the paths); `river_guard` skips the sine river band when true.
///
/// `cover_affinity` is a per-ground-cover-class damp lean (same order/length as `cfg.cover`):
/// `+` = the class wins in damp green hollows (ferns/clover/mushrooms), `-` = it wins on the
/// dry/trodden sweeps (grass + sun-flowers). Empty → neutral (every class placed by its raw
/// chance, biome-agnostic). Only the home meadow passes one; other biomes stay neutral.
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
    cover_affinity: &[f32],
) {
    // One shared white vertex-colour material — every prop bakes its hue into the mesh,
    // so the renderer auto-batches them into few draw calls.
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        // Matte: the props (esp. thin grass blades) used to catch a bright specular glint that lit
        // their edges into hard sharp highlights. High roughness + low reflectance kills that glint
        // so foliage reads soft / sun-faded ("wypłowiałe") instead of crisp. Tunable live in F1 → Render.
        perceptual_roughness: 0.92,
        reflectance: 0.18,
        ..default()
    });

    let classes = upload_classes(&cfg.classes, meshes);
    let cover = upload_classes(&cfg.cover, meshes);

    // First non-tree class → the "too close" fallback for trees (forest drops a bush).
    let fallback: Option<&ClassHandles> = classes.iter().find(|c| !c.tree);

    let mut r = Rng(cfg.seed);
    let mut tree_pts: Vec<Vec2> = Vec::new();
    let min_d2 = cfg.tree_min_dist * cfg.tree_min_dist;

    // Passive props accumulate here by chunk (one bucket per spatial cell), then get baked into
    // a single merged mesh per chunk in `spawn_chunks` below — collapsing tens of thousands of
    // individual entities into ≈2 per occupied chunk. Trees stay individual (they need per-tree
    // chop HP + wind sway), so only non-tree scatter + ground cover lands here. A prop's world
    // transform is rebased onto its chunk centre so the merged vertices stay local.
    let mut chunks: std::collections::HashMap<(i32, i32), ChunkBucket> = std::collections::HashMap::new();
    // Queue a passive prop into a chunk bucket: stash mesh + chunk-relative transform.
    let mut queue = |cover_bucket: bool, mesh: Mesh, world: Vec3, rot: Quat, scale: f32| {
        let key = chunk_key(world.x, world.z);
        let transform = Transform {
            translation: world - chunk_center(key),
            rotation: rot,
            scale: Vec3::splat(scale),
        };
        let bucket = chunks.entry(key).or_default();
        let dst = if cover_bucket { &mut bucket.cover } else { &mut bucket.props };
        dst.push(PendingProp { mesh, transform });
    };

    // ── Main per-tile scatter ──
    let mut gx = lo;
    while gx < hi {
        let mut gz = lo;
        while gz < hi {
            let cx = gx + 0.5 + r.range(-0.35, 0.35);
            let cz = gz + 0.5 + r.range(-0.35, 0.35);
            if (river_guard && crate::water::on_river(cx, cz))
                || !mask(cx, cz)
                || crate::roads::on_road(cx, cz)
            {
                gz += 1.0;
                continue;
            }
            let py = height_fn(cx, cz);
            // Terrain-aware tree thinning: estimate the local ground gradient and drop TREE density
            // on steep ground (cliffs, mountain faces, coastal ridges) so woods thin toward bare
            // crag instead of carpeting a wall. Gentle rolling hills (< SLOPE_FREE) are untouched.
            let tree_mult = {
                let d = 1.6;
                let gx = height_fn(cx + d, cz) - py; // forward diff off the already-sampled py (cheap)
                let gz = height_fn(cx, cz + d) - py;
                let slope = (gx * gx + gz * gz).sqrt() / d;
                (1.0 - (slope - SLOPE_FREE).max(0.0) * SLOPE_TREE_THIN).clamp(SLOPE_TREE_FLOOR, 1.0)
            };
            // Thicken the open woods: full boost between the roads, fading to none at a trail edge.
            let density = SCATTER_DENSITY * (1.0 + OPEN_SCATTER_BOOST * crate::roads::openness(cx, cz));
            let roll = r.next();
            let mut acc = 0.0;
            let mut chosen: Option<&ClassHandles> = None;
            for c in &classes {
                let chance = if c.tree { c.chance * tree_mult } else { c.chance };
                acc += chance * density;
                if roll < acc {
                    chosen = Some(c);
                    break;
                }
            }
            if let Some(c) = chosen {
                let vi = pick_weighted(&c.weights, r.next());
                let s = r.range(c.scale.0, c.scale.1);
                let landmark_buffered = crate::ruins::near_landmark_collision_buffer(cx, cz);
                let landmark_core = crate::ruins::near_landmark_visual_footprint(cx, cz);
                if c.tree {
                    let p = Vec2::new(cx, cz);
                    let tree_clear = cfg.tree_min_dist * TREE_BLOCKER_CLEAR_FRAC;
                    // Skip trunk-trees that fall inside a warden glade (the boss arena must stay
                    // open), in a landmark's collision apron, OR too close to a neighbour — drop a
                    // walk-through fallback prop there so the ground keeps understorey rather than
                    // turning into a stripped circle.
                    if crate::boss::in_warden_glade(cx, cz)
                        || landmark_buffered
                        || landmark_core
                        || tree_pts.iter().any(|q| q.distance_squared(p) < min_d2)
                        || crate::blockers::any_within(cx, cz, tree_clear)
                    {
                        // Passive non-tree prop, so it merges into the chunk's props bucket.
                        if let Some(fb) = fallback
                            .filter(|fb| !landmark_core && (fb.block_radius <= 0.0 || !landmark_buffered))
                        {
                            let fi = pick_weighted(&fb.weights, r.next());
                            let fs = r.range(fb.scale.0, fb.scale.1);
                            queue(false, fb.variants[fi].clone(), Vec3::new(cx, py, cz), yaw(&mut r), fs);
                        }
                    } else {
                        tree_pts.push(p);
                        // Per-KIND footprint (sized from each tree's own silhouette at upload):
                        // a wide low crown / conifer skirt blocks wider so you sink only a little
                        // into a big tree, while a slim poplar / airy birch lets you reach its
                        // trunk. Scaled with the instance, capped ≤ the blockers neighbour-scan
                        // bound. (Old behaviour was one flat 0.20 for every kind.)
                        let unit_r = c.block_radii.get(vi).copied().unwrap_or(0.20);
                        let trunk_r = (unit_r * s).min(0.8);
                        crate::blockers::add(cx, cz, trunk_r);
                        let base = cardinal(&mut r);
                        // Trees stay individual entities (chop HP + wind sway) sharing one
                        // uploaded handle per variant — the renderer auto-batches the instances.
                        let mut tree = commands.spawn((
                            Mesh3d(c.handles[vi].clone()),
                            MeshMaterial3d(mat.clone()),
                            // Identity rotation — wind `Sway` overwrites it each frame.
                            Transform {
                                translation: Vec3::new(cx, py, cz),
                                rotation: Quat::IDENTITY,
                                scale: Vec3::splat(s),
                            },
                            crate::wind::sway_for(cx, cz, base),
                            // Every scattered tree is choppable for wood (1 tree = 1 wood). The
                            // trunk blocker is cleared on fell via `blockers::remove_at`.
                            crate::verbs::ChopTree::new(trunk_r),
                            BiomeEntity,
                        ));
                        // Distance-cull far trees: past ~180u the fog (FOG_FULL ≈190) has them
                        // nearly opaque and shadows already stop at the cascade max (≈150), so a far
                        // tree is pure rasterizer cost. ABRUPT (empty band) so it stays a hard GPU
                        // cull. Gated by `FOREST_NOCULL` so the win can be A/B-measured (see helper).
                        if scatter_cull_enabled() {
                            tree.insert(bevy::camera::visibility::VisibilityRange {
                                start_margin: 0.0..0.0,
                                end_margin: 180.0..180.0,
                                use_aabb: true,
                            });
                        }
                        // Desert "trees" are saguaro cacti — tag them so felling plays the dry
                        // wood-crack instead of the full timber crash (no woody trunk to crash).
                        if cfg.biome == Biome::Desert {
                            tree.insert(crate::verbs::Cactus);
                        }
                    }
                } else {
                    if landmark_core || (landmark_buffered && c.block_radius > 0.0) {
                        gz += 1.0;
                        continue;
                    }
                    // Big non-tree props (boulders) block, scaled with the instance and capped at
                    // the blockers neighbour-scan bound. Small clutter has block_radius 0 → nothing.
                    // A floor drops the small end of a mixed class — only clearly-big boulders block;
                    // the smaller/medium rocks of a class stay walk-through (you step over them, not
                    // bump an invisible wall around a knee-high stone).
                    if c.block_radius > 0.0 {
                        let rad = c.block_radius * s;
                        if rad > 1.0 {
                            // Too big for a ≤1.0 circle (the neighbour-scan bound), so a big boulder
                            // used to block only a 1.0 core you could clip straight past. Register a
                            // box hugging its footprint instead (0.85× so the square corners don't
                            // over-block a round-ish boulder) — solid near the edges, not a thin core.
                            let hb = rad * 0.85;
                            crate::blockers::add_box(cx, cz, hb, hb);
                        } else if rad >= 0.30 {
                            crate::blockers::add(cx, cz, rad);
                        }
                    }
                    // Passive scatter (bushes/rocks/litter) — merge into the chunk's props
                    // bucket. The blocker above is registered independently of the entity, so a
                    // boulder still stops you even though its mesh now lives in a shared chunk.
                    let rot = yaw(&mut r);
                    queue(false, c.variants[vi].clone(), Vec3::new(cx, py, cz), rot, s);
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
                    if (river_guard && crate::water::on_river(x, z))
                        || !mask(x, z)
                        // near_road (not on_road): flat cover discs are wide, so reject any whose
                        // body would overhang a trail even if its centre is just off it.
                        || crate::roads::near_road(x, z, 0.9)
                        || crate::ruins::near_landmark_visual_footprint(x, z)
                    {
                        continue;
                    }
                    // Keep cover off trunks, walls, and landmark footprints (blockers register
                    // during the same build; landmarks add theirs before this pass runs on Continue).
                    if crate::blockers::any_within(x, z, 0.4) {
                        continue;
                    }
                    let patch = ground_patch(x, z);
                    // Density DRIFT: gate the spawn on the ground's own fertility so the cover
                    // thins to bare on the worn/tan patches the albedo paints and clumps into
                    // dense flowery drifts in the lush hollows. This is what makes vegetation
                    // track the ground (and read as varied drifts, not an even sprinkle).
                    // Open woods (away from paths) lift the fertility gate so cover clumps thicker
                    // there, tapering back to base at a trail edge.
                    let fert = (patch.fertility * (1.0 + OPEN_COVER_BOOST * crate::roads::openness(x, z))).min(1.0);
                    if r.next() > fert {
                        continue;
                    }
                    let py = height_fn(x, z);
                    // Species LEAN by patch dampness: scale each class's chance by its affinity so
                    // ferns/clover/mushrooms cluster in the damp green hollows and grass + sun-
                    // flowers take the dry sweeps. We've already cleared the fertility gate, so
                    // normalise (pick proportionally) → past the gate something always grows.
                    let affinity = |i: usize| -> f32 {
                        cover_affinity
                            .get(i)
                            .map(|a| (1.0 + a * patch.damp).max(0.05))
                            .unwrap_or(1.0)
                    };
                    let total: f32 = cover.iter().enumerate().map(|(i, c)| c.chance * affinity(i)).sum();
                    if total > 0.0 {
                        let target = r.next() * total;
                        let mut acc = 0.0;
                        for (i, c) in cover.iter().enumerate() {
                            acc += c.chance * affinity(i);
                            if target < acc {
                                let vi = pick_weighted(&c.weights, r.next());
                                let s = r.range(c.scale.0, c.scale.1);
                                // Ground cover → the chunk's cover bucket (NotShadowCaster +
                                // distance-fade, applied once per merged chunk in `spawn_chunks`).
                                let rot = yaw(&mut r);
                                queue(true, c.variants[vi].clone(), Vec3::new(x, py, z), rot, s);
                                break;
                            }
                        }
                    }
                }
                gz += 1.0;
            }
            gx += 1.0;
        }
    }

    // Bake every accumulated chunk bucket into its merged mesh + spawn one entity each. The
    // `queue` closure's mutable borrow of `chunks` has ended (no more queueing), so we can move
    // the map in. `drop(queue)` makes that explicit (and silences any "borrowed after move").
    drop(queue);
    spawn_chunks(commands, meshes, &mat, chunks);
}

fn cardinal(r: &mut Rng) -> Quat {
    Quat::from_rotation_y((r.next() * 4.0).floor() * std::f32::consts::FRAC_PI_2)
}

fn yaw(r: &mut Rng) -> Quat {
    Quat::from_rotation_y(r.next() * std::f32::consts::TAU)
}
