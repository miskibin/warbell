//! The **world map** — a Bevy port of the TS game's island (`src/world/tileMap.ts`),
//! generated in the original BASE space (144×108): an elliptical island with a noisy
//! coast, five biome blobs (snow NW, desert NE, rock E, forest SW, swamp S), a grass
//! centre safe-zone (the castle spot), a grass frontier with scattered forest clumps and
//! rolling terraced knolls, a beach ring backed by patchy coastal mountain ridges, three
//! meandering rivers (each springing from a highland and draining to the sea, their banks
//! projected onto the smooth channel edge — no tile-grid stairstep) + one lake, and **terraced**
//! stepped heights (flat tile-tops + cliff
//! faces; snow peak 10, rock peak 9). South of the old coast the grid extends into the
//! **Blight** — the walkable ork-fortress mire (`TB::Blight`, shape owned by
//! `ork_fortress.rs`) that gameplay treats as swamp (poison + slow).
//!
//! `build` seeds the full playable island: the ground mesh plus ork-camp / castle / ore / chest
//! placement and wildlife (single-biome views, keys 1–5, have no island layout, so none of these).
//! Biome boundaries get **smooth colour blending** (the ground mesh's per-vertex colour is
//! a soft distance-weighted mix of the biome palettes), while the discrete classification
//! still drives heights + which biome's props scatter on each tile.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::ExtendedMaterial;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::biome::{
    scatter_region, AtmoSample, Backdrop, Biome, BiomeAmbience, BiomeAmbiences, BiomeConfig,
    GroundDetail, ParticleKind, PropClass,
};
use crate::groundcover as gc;
use crate::palette::lin;
use crate::terrain::TerrainMaterial;
use crate::water::{WaterExt, WaterMaterial, WaterParams};

// ── Map dimensions ───────────────────────────────────────────────────────────────
/// Map enlargement vs the original base island (more tiles → more land + props).
/// Bumped 1.5 → 1.8 (a clean +20% on every axis), then 1.8 → 2.0 to give the strongholds (esp.
/// the desert rival, jammed against the north coast) more room, then 2.0 → 2.2 (map-character
/// overhaul pass 0) so the biomes keep real interior once the fixed-world-size claims (warden
/// glade r16, rival plateau r30, fortress apron) are subtracted, then 2.2 → 2.6 (landmark
/// overhaul, July 2026 — paid for by the terrain far-LOD, see `build_terrain_chunk_coarse`).
/// The world-coord-authored landmarks scale with it automatically: `ork_fortress::BLIGHT_DZ` and
/// `rival::RIVAL_CENTRE` are derived from `MAP_SCALE`, and the remaining hand-authored world
/// coords (warden `boss::region_center`, chest hoards, snowman field, the swamp pool keep-out)
/// route through [`world22`] so they track scale bumps too. Per-bump manual checklist: trim
/// `SAFE_R` (base-space, so it silently over-grows), re-size `navgrid::NAV_MAX_NODES` (A* budget
/// ∝ tiles), and re-check CLAUDE.md's biome-centre table for capture framing.
pub const MAP_SCALE: f32 = 2.6;

/// Rescale a world-XZ coordinate that was hand-authored (and playtested) at `MAP_SCALE` 2.2 to
/// the CURRENT scale. Generation runs in base space, so a point that sat inside a biome at 2.2
/// maps to the same base-space spot — same biome, same relative position — at any other scale.
/// Every hand-authored world coordinate should pass through here rather than bake the scale in.
pub fn world22(x: f32, z: f32) -> Vec2 {
    Vec2::new(x, z) * (MAP_SCALE / 2.2)
}
// The GRID is the enlarged resolution; GENERATION still runs in *base* space — the grid
// loop samples `classify(ix / MAP_SCALE, …)`, so the island shape is identical, just
// drawn over more tiles. `CX/CZ` stay the BASE centre used by all the generation math;
// `GX/GZ` are the GRID centre used for world placement + tile-cache indexing. All four
// derive from MAP_SCALE so bumping the scale stays self-consistent.
const BASE_COLS: f32 = 144.0; // original (pre-enlargement) grid width
const BASE_ROWS: f32 = 164.0; // 108 original island rows + 56 southern Blight extension
pub const COLS: i32 = (BASE_COLS * MAP_SCALE) as i32; // 316 at 2.2
/// Rows: the original island (108 base rows) PLUS the southern **Blight** extension —
/// the walkable ork-fortress landmass (`ork_fortress::blight_class_base`). Everything
/// north of the old south edge is the original map, just denser; the Blight grows south.
pub const ROWS: i32 = (BASE_ROWS * MAP_SCALE) as i32; // 360 at 2.2
const CX: f32 = BASE_COLS / 2.0; // 72 — base generation centre
const CZ: f32 = 54.0; // original island half-height (108/2)
/// Grid centre (enlarged) — world placement recentres the map onto the origin here.
pub const GX: f32 = COLS as f32 / 2.0;
/// NOT `ROWS/2`: the castle stays at the origin (= island centre), so GZ is pinned to the
/// ORIGINAL island's half-height scaled, and the Blight extension grows `ROWS` southward only.
pub const GZ: f32 = CZ * MAP_SCALE; // 118.8 at 2.2
const ISLAND_RX: f32 = 71.0;
const ISLAND_RZ: f32 = 53.0;
const ISLAND_EXP: f32 = 2.6;
// Castle safe-zone radius in BASE space (forced flat grass, no biome scatter). `classify` runs in
// base space, so the WORLD radius is `SAFE_R * MAP_SCALE` (≈32.4 at MAP_SCALE 2.6). Trimmed from
// 18.0 → 16.2 → 14.7 → 12.45 across the 1.8 → 2.0 → 2.2 → 2.6 scale bumps: this is base-space, so
// each bump silently grows the *world* safe-zone; the trims keep the biome-free ring round the
// castle at its tuned ~32.4-unit world size. Town build plots flatten themselves
// (`near_build_plot`), so this doesn't gate their footing.
pub const SAFE_R: f32 = 12.45;
pub const GROUND_STEP: f32 = 0.5; // world-Y per height class
pub const SEA_Y: f32 = -0.4;
/// Cliff-face mesh knobs (`build_terrain_chunk`): a tile-edge wall whose mean drop is at
/// least `CLIFF_MIN_DROP` stops being one flat vertical quad and becomes a subdivided,
/// noise-displaced faceted crag face (the flat quads read as Minecraft blocks on every
/// mesa tier / coastal ridge — player report). Interior lattice rows sit on FIXED world-Y
/// multiples of `CLIFF_ROW` so adjacent wall pieces sample identical heights along a shared
/// corner column and stay welded; `CLIFF_SKIRT` sinks the face base below the lower shelf
/// so the displaced bottom edge never daylights a gap.
const CLIFF_MIN_DROP: f32 = 0.55;
const CLIFF_ROW: f32 = 0.4;
const CLIFF_SKIRT: f32 = 0.25;
/// Colour-blend half-width (tiles) at biome edges.
const BLEND: f32 = 4.5;

/// Shared daytime atmosphere: (sky, fog_density, sun_color, sun_illuminance,
/// ambient_color, ambient_brightness, sun_pos). Light fog so the big island reads across.
pub const ATMOSPHERE: (u32, f32, u32, f32, u32, f32, Vec3) =
    (0xb4d2ec, 0.0060, 0xffedc7, 11_000.0, 0xe6edf5, 165.0, Vec3::new(80.0, 110.0, 40.0));

// ── Biome palette (sRGB hex) for the blended ground colour ──────────────────────
// Every per-map ground tone lives in a `Palette` so a second map is a pure data swap (see
// `MapDef` / `active_map`). `PAL_HOME` holds the original island's values **verbatim** — it's
// the regression anchor (map 0 must render byte-identically). Field docs: `*_dark/_dry/_gold`
// are the grass meadow macro-patch tones; `swamp_*` the wet marsh mottle; `blight_*` the
// trampled-ork-mud mottle; `snow_shade/_bright` the wind-drift sastrugi; `forest_dark/_dry`
// the canopy loam/litter; `dirt` grass-cliff soil; `snow_cliff_*` the snow lip over rock;
// `road_dirt` the baked approach-road tint.
#[derive(Clone, Copy)]
struct Palette {
    grass: u32,
    grass_dark: u32,
    grass_dry: u32,
    grass_gold: u32,
    sand: u32,
    forest: u32,
    rock: u32,
    snow: u32,
    desert: u32,
    swamp: u32,
    swamp_dark: u32,
    swamp_moss: u32,
    swamp_algae: u32,
    blight: u32,
    blight_dark: u32,
    blight_ash: u32,
    blight_green: u32,
    blight_rust: u32,
    snow_shade: u32,
    snow_bright: u32,
    forest_dark: u32,
    forest_dry: u32,
    dirt: u32,
    snow_cliff_lip: u32,
    snow_cliff_rock: u32,
    road_dirt: u32,
}

/// The home island's original palette — values unchanged from the pre-second-map era.
const PAL_HOME: Palette = Palette {
    grass: 0x6fb24c,
    grass_dark: 0x4f8c38,
    grass_dry: 0x6f9a48, // green-shifted off the old olive 0x8fa953 (player: ground too yellow)
    grass_gold: 0x7ba049, // was a warm gold 0xa8b048 — now a muted lighter green so variety reads green, not yellow
    sand: 0xcdb079,
    forest: 0x5d9e44,
    rock: 0x8d847a,
    snow: 0xe4ecf5,
    desert: 0xddc189,
    swamp: 0x55613a,
    swamp_dark: 0x363f27,
    swamp_moss: 0x6e7c48,
    swamp_algae: 0x4c6232,
    blight: 0x4d3e2a,
    blight_dark: 0x2c2113,
    blight_ash: 0x6f695c,
    blight_green: 0x59653a,
    blight_rust: 0x5a3320,
    snow_shade: 0xc9d6e8,
    snow_bright: 0xf4f9ff,
    forest_dark: 0x4a8136,
    forest_dry: 0x79a24c,
    dirt: 0x6b4f30,
    snow_cliff_lip: 0xe9f0f9,
    snow_cliff_rock: 0x7b8597,
    road_dirt: 0x8a6d44,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TB {
    Grass,
    Sand,
    Forest,
    Rock,
    Snow,
    Desert,
    Swamp,
    /// The ork-blighted mire around Gnashfang Hold (south extension). Maps to
    /// [`Biome::Swamp`] for gameplay (poison, slow, ambience) but draws its own ground.
    Blight,
}

struct Region {
    x: f32,
    z: f32,
    r: f32,
    biome: TB,
    /// centre height class for mountain biomes (0 = flat biome).
    peak: i32,
    /// Tiered-mesa mode (map-character overhaul pass 1): the region's bulk becomes flat shelves
    /// separated by multi-class SHEER walls (`mesa_height`), exempt from `terrace_inland`, with
    /// the authored `passes` corridors as the only climbable ways up. `false` keeps the legacy
    /// smooth-terraced `mountain_height` (everything ≤1-class walkable).
    cliffy: bool,
    /// Authored pass corridors for a cliffy region (empty otherwise). The FIRST pass is the
    /// castle-facing main ascent — roads target its mouth (`biome_road_targets`).
    passes: &'static [Pass],
}

/// One authored pass corridor through a cliffy region's mesa walls: a smooth ≤1-class ramp
/// staircase from the region rim to the summit, cut radially along `ang`.
struct Pass {
    /// Direction (base-space radians) from the region CENTRE toward the pass mouth on the rim.
    ang: f32,
    /// Corridor half-width in base tiles at the rim (angular width shrinks toward the centre
    /// like `ramp_class`'s, so the walls converge into a climbing canyon).
    half: f32,
}

/// Snow massif passes: SE main ascent facing the castle + an E ramp toward the desert (the
/// snow↔desert ring road threads it). Angles are `atan2(target_z − reg.z, target_x − reg.x)`.
// Half-widths widened (3.0→4.5 / 2.4→3.2) when the massif grew (r 36→42): a longer ramp is
// angularly narrower at its mouth, so at the old widths a tile centre could fall a hair outside
// the corridor and read as a 1-class apron gap (`mesa_passes_climb_to_the_summit` regression).
const SNOW_PASSES: [Pass; 2] = [
    Pass { ang: 0.578, half: 4.5 },  // SE, toward the castle (main ascent)
    Pass { ang: -0.184, half: 3.2 }, // E, toward the desert dunes
];
/// Rock range passes: WNW main ascent (mouth at base ≈(86,39), a clear 20+ base units from the
/// castle — due-west would run the ramp straight into the castle safe-zone's grass fray, which
/// force-flattens class-1 bays into the corridor) + the N slot canyon toward the desert
/// (compression→release: the desert↔rock ring road runs its floor). The SW rim by the lake is
/// deliberately passless — that wall is the waterfall cliff (vista pass).
const ROCK_PASSES: [Pass; 2] = [
    Pass { ang: -2.60, half: 3.0 }, // WNW, toward the castle side (main ascent)
    // N canyon toward the desert. NOT -1.88 (straight at the desert centre): the rival
    // stronghold's forced-flat plateau (world (66,-88), r 30) sits on that line and cuts
    // class-1 bays across the ramp; -1.60 exits the canyon ~34 world units east of the fort —
    // the ring road passes the rival's walls, then climbs the slot.
    Pass { ang: -1.60, half: 2.4 },
];

const REGIONS: [Region; 6] = [
    // NW snow massif (r 31→33: shrink the empty snow↔desert grass seam). peak 10→18 + cliffy
    // (pass 1): tiered mesa shelves + sheer walls, climbable only via SNOW_PASSES. 18 (not the
    // first attempt's 13): at 13 the tier walls came out 1–1.5u and the massif still read as a
    // smooth white dome (verification FAIL) — 18 gives 2u walls (ladder 2/6/10/14/18) that
    // register as cliff bands even white-on-white in haze. Rock stays taller (22).
    // r 33→36: grow the massif into the grass freed by the shrunk swamps ("bigger mountains").
    // r 36→42 + peak 18→20 (player: the snow-side coast mountain should be bigger + more
    // irregular): the massif now reaches the NW `nw_headland` coast, so its flanks run into the
    // sea as a jagged cliff front instead of tapering to a beach. Centre unchanged, so the SE/E
    // pass corridors keep their authored angles (their half-widths grew to match the longer ramp).
    Region { x: 26.0, z: 24.0, r: 42.0, biome: TB::Snow, peak: 20, cliffy: true, passes: &SNOW_PASSES },
    // NE dunes — shifted NW (112,28 → 101,10), so the dune field grows toward the snow massif and
    // fills the grass corridor along the top coast. Radius 34→38 to close the empty grass seam
    // between snow and desert (players noted the wide bare strip there): with snow at r33 the seam
    // shrinks from ~10 to ~4 base, and region_at's wobble/fray makes the two biomes meet on an
    // organic edge instead of a clean grass gutter. Still biased NORTH so the dunes don't bulge onto
    // the keep — at z10 the south reach is ~z48, clear of the castle safe-ring (south edge ~z36).
    Region { x: 101.0, z: 10.0, r: 38.0, biome: TB::Desert, peak: 0, cliffy: false, passes: &[] },
    // E rock range — pulled in toward the castle (122→116: the mine country starts just past
    // the safe-zone fray instead of a 33-unit trek). peak 18→22 + cliffy (pass 1): the tallest
    // biome is now a real tiered MESA — flat shelves, 2.5u sheer walls between them, climbable
    // only via ROCK_PASSES; `terrace_inland` exempts it (`cliff_exempt_base`).
    // r 34→38: the tallest biome grows down toward the shrunk SE marsh — more mountain, less bog.
    Region { x: 116.0, z: 57.0, r: 38.0, biome: TB::Rock, peak: 22, cliffy: true, passes: &ROCK_PASSES },
    // SW forest + S swamp: r +2/+3 (pass 0, map-character overhaul) — the two most
    // "nothing left once you subtract the claims" biomes get extra interior on top of the
    // MAP_SCALE 2.2 bump. Snow/desert radii stay (their shared seam is already tuned tight).
    Region { x: 32.0, z: 80.0, r: 36.0, biome: TB::Forest, peak: 0, cliffy: false, passes: &[] }, // SW forest
    Region { x: 72.0, z: 92.0, r: 23.0, biome: TB::Swamp, peak: 0, cliffy: false, passes: &[] }, // S swamp (r 29→23: less bog)
    // Eastern marsh arm: fills the open grass strip that ran between the S swamp and the
    // rocky mine range (world x +30…+66, z +15…+60), so the marsh laps right up to the
    // foot of the mines instead of leaving a grass corridor. Rock's mountain priority in
    // `region_at` auto-clips the east edge (the mines stay rock), and the castle safe-ring
    // forces grass at the centre, so this only eats the in-between grass.
    Region { x: 102.0, z: 78.0, r: 17.0, biome: TB::Swamp, peak: 0, cliffy: false, passes: &[] }, // E marsh (r 24→17: yields to the rock range)
];

struct Plateau {
    x: f32,
    z: f32,
    r: f32,
    peak: i32,
}
const PLATEAUS: [Plateau; 4] = [
    Plateau { x: 98.0, z: 72.0, r: 9.0, peak: 5 },
    Plateau { x: 52.0, z: 50.0, r: 7.0, peak: 4 },
    Plateau { x: 90.0, z: 36.0, r: 7.0, peak: 5 }, // NE mesa on the desert fringe
    Plateau { x: 20.0, z: 62.0, r: 7.0, peak: 4 }, // W forest highland
];

const DELIBERATE_LAKE: (f32, f32, f32, f32) = (92.0, 80.0, 5.0, 3.0); // x,z,rx,rz

// ── Map selection (which world to generate) ──────────────────────────────────────
/// Which world a run generates. `u8` on the wire (0 = Home, default) so it rides the save +
/// the boot global trivially. Add a variant + a [`MapDef`] to ship a third map.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MapId {
    #[default]
    Home = 0,
    /// The RTS-skirmish **arena** (`FOREST_RTS=1`): a small symmetric grass island, no biome blobs
    /// / rivers / mountains — just two base plateaus, a dirt road between them and mirrored deposit
    /// sites. Generated by [`classify_arena`] / the arena arm of [`build_step`].
    Arena = 2,
}

/// The chosen map, as a Bevy resource (start-screen owns it; default Home — **except** in RTS
/// skirmish, where it defaults to [`MapId::Arena`], see the manual `Default` below).
/// `biome::apply_build` reads it and calls [`set_active_map`] before [`build`], so the pure
/// generation functions — which can't read a resource — see the right map.
#[derive(Resource, Clone, Copy)]
pub struct ActiveMap(pub MapId);

impl Default for ActiveMap {
    fn default() -> Self {
        // Skirmish (`FOREST_RTS=1`) boots straight into the arena. `mode_from_env` is a pure env
        // read, safe here before any resource exists — this is exactly the "branch where ActiveMap
        // is initialized" hook: `biome`'s `init_resource::<ActiveMap>()` picks this up at boot.
        if crate::rts::mode_from_env() == crate::rts::GameMode::Skirmish {
            ActiveMap(MapId::Arena)
        } else {
            ActiveMap(MapId::default())
        }
    }
}

/// Everything that differs between maps. All other generation (island ellipse, rivers, lake,
/// plateaus, coast ridges) is shared and merely perturbed by `noise_phase`.
struct MapDef {
    regions: &'static [Region],
    palette: Palette,
    atmosphere: (u32, f32, u32, f32, u32, f32, Vec3),
    /// Added inside `noise_a`/`noise_b` → reseeds the coastline fray, river winding and inland
    /// hills. Home is `0.0` (identical output — the regression anchor).
    noise_phase: f32,
}

const MAP_HOME: MapDef = MapDef {
    regions: &REGIONS,
    palette: PAL_HOME,
    atmosphere: ATMOSPHERE,
    noise_phase: 0.0,
};

/// **Arena** atmosphere (RTS skirmish): bright, clear high-noon daylight so the top-down battle
/// reads crisply corner-to-corner — light-blue sky, warm white sun, fog pushed thin (the bases sit
/// ~80 units apart and must both stay legible), strong ambient fill, sun high overhead. Same tuple
/// shape as [`ATMOSPHERE`].
const ARENA_ATMOSPHERE: (u32, f32, u32, f32, u32, f32, Vec3) =
    (0xbfe0f5, 0.0038, 0xfff2d8, 12_000.0, 0xeaf1f8, 175.0, Vec3::new(60.0, 150.0, 50.0));

/// The arena reuses the home island's lush green grass/forest palette — a pleasant, familiar
/// meadow look. No biome-blob tones are ever sampled (arena has no regions), so only the grass /
/// forest / sand / dirt entries actually render.
const PAL_ARENA: Palette = PAL_HOME;

/// The arena map def: **no regions** (uniform grass/light-forest, no biome blobs / mountains), the
/// home green palette, bright clear daylight, and its own noise phase so the coast fray + rolling
/// knolls don't echo the home island. All the arena's structure (island ellipse, base
/// plateaus, road, deposit spots) lives in [`classify_arena`], not in shared region/river data.
const MAP_ARENA: MapDef = MapDef {
    regions: &[],
    palette: PAL_ARENA,
    atmosphere: ARENA_ATMOSPHERE,
    noise_phase: 5.0,
};

/// Process-global active map id, set once per build by [`set_active_map`] (from
/// `biome::apply_build`, which reads the [`ActiveMap`] resource). The pure generation fns read
/// it; runtime resources don't reach down here.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

/// Point the generator at `id`. Called immediately before [`build`] (and on load). Switching
/// maps just changes which memoised grid [`tiles`] returns — the loading veil covers the regen.
pub fn set_active_map(id: MapId) {
    ACTIVE.store(id as u8, Ordering::Relaxed);
}
fn active_id() -> u8 {
    ACTIVE.load(Ordering::Relaxed)
}
/// The currently-generated map id as `u8` — the save path compares it against a loaded snapshot
/// to decide whether resuming needs a world rebuild.
pub fn current_map_u8() -> u8 {
    active_id()
}

impl MapId {
    /// Decode the `u8` stored in a save (unknown ids fall back to Home).
    pub fn from_u8(v: u8) -> Self {
        match v {
            2 => MapId::Arena,
            _ => MapId::Home,
        }
    }
}
fn active_map() -> &'static MapDef {
    match active_id() {
        2 => &MAP_ARENA,
        _ => &MAP_HOME,
    }
}
/// True while the RTS-skirmish arena is the active world — the arena takes its own early branches
/// in [`classify`], [`ground_color`], the still-water fields, [`build_step`] and `roads`.
pub fn is_arena() -> bool {
    active_id() == MapId::Arena as u8
}
/// The active map's atmosphere tuple (read by `biome::apply_build` + [`build`]).
pub fn active_atmosphere() -> (u32, f32, u32, f32, u32, f32, Vec3) {
    active_map().atmosphere
}

// ── Procedural generation (ported from tileMap.ts, base space) ──────────────────
fn noise_a(x: f32, z: f32) -> f32 {
    let p = active_map().noise_phase;
    (x * 0.13 + 1.7 + p).sin() * (z * 0.11 - 2.3 + p).cos() + (x * 0.31 + z * 0.29 + 4.5 + p).sin() * 0.5
}
fn noise_b(x: f32, z: f32) -> f32 {
    let p = active_map().noise_phase;
    (x * 0.21 - 3.1 + p).sin() * (z * 0.19 + 0.7 + p).cos() + ((x + z) * 0.07 + 5.2 + p).sin() * 0.4
}

// ── Organic mottle for the GROUND COLOUR (visual only; gameplay/gen still use noise_a/b) ──
// `noise_a`/`noise_b` are axis-separable sine products — `sin(x·a)·cos(z·b)` plus an
// `sin((x+z)·c)` term — which form a regular standing-wave LATTICE: rectangular cells from
// the sin·cos terms, diagonal bands from the (x+z) terms. Baked into the per-vertex meadow
// colour, that lattice read as a grid of "tiles" / diagonal stripes across open ground. This
// hash value-noise (quintic interpolation, domain-rotated octaves) gives the same broad green
// patchiness with NO repeating structure.
fn vhash(ix: i32, iz: i32) -> f32 {
    // lowbias32-style integer hash → well-distributed, no axis correlation.
    let mut h = (ix as u32).wrapping_mul(0x9E37_79B1).wrapping_add((iz as u32).wrapping_mul(0x85EB_CA77));
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}
fn vnoise(x: f32, z: f32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - ix as f32;
    let fz = z - iz as f32;
    let ux = fx * fx * fx * (fx * (fx * 6.0 - 15.0) + 10.0); // quintic (C2) → no cell-edge grid
    let uz = fz * fz * fz * (fz * (fz * 6.0 - 15.0) + 10.0);
    let a = vhash(ix, iz);
    let b = vhash(ix + 1, iz);
    let c = vhash(ix, iz + 1);
    let d = vhash(ix + 1, iz + 1);
    let ab = a + (b - a) * ux;
    let cd = c + (d - c) * ux;
    ab + (cd - ab) * uz
}
/// Signed organic mottle (~`[-1.5, 1.5]`, matching `noise_a`'s range so the existing
/// `ground_color` `smoothstep` thresholds still apply) at the spatial frequency the old
/// `noise_a(x*scale)` produced. Two domain-rotated octaves so nothing lines up on the world
/// axes. `seed` decorrelates calls; `noise_phase` keeps reskinned maps distinct like `noise_a`.
fn omottle(x: f32, z: f32, scale: f32, seed: f32) -> f32 {
    let f = 0.021 * scale; // ≈ the effective per-unit frequency of `noise_a(x*scale)`
    let s = seed + active_map().noise_phase;
    let o1 = vnoise((0.857 * x - 0.515 * z) * f + s, (0.515 * x + 0.857 * z) * f + s * 1.3); // ~31°
    let o2 = vnoise((0.602 * x - 0.799 * z) * f * 2.1 + s * 0.7, (0.799 * x + 0.602 * z) * f * 2.1 + s); // ~53°
    ((o1 * 0.65 + o2 * 0.35) - 0.5) * 3.0
}

/// Outward push (in normalised-ellipse units) of the NW snow-massif coastline: a ragged HEADLAND
/// that runs the snow mountain right out toward the map's NW corner so its flanks meet the sea in
/// a cliff — where the ragged noise peaks the land reaches the grid boundary and simply ends in a
/// wall (a map-edge cliff, which the player OK'd), where it dips the shore pulls back into a cove.
/// Concentrated at the true NW corner (`wx*wz`, both west AND north of centre) and fading to zero
/// long before the other coasts, so only the snow side is reshaped. Added identically to
/// `is_land_shape` and `sea_field` so the shoreline the two derive never disagrees.
fn nw_headland(x: f32, z: f32) -> f32 {
    let wx = ((CX - x) / CX).clamp(0.0, 1.0); // 0 at centre → 1 at the west grid edge
    let wz = ((CZ - z) / CZ).clamp(0.0, 1.0); // 0 at centre → 1 at the north grid edge
    let corner = wx * wz;
    if corner <= 0.0 {
        return 0.0;
    }
    // Capes-and-coves: two octaves with a LOW floor so the push swings widely along the coast —
    // the shoreline wiggles in and out into a ragged, irregular front (NOT the straight edge an
    // earlier high-floor version produced by slamming the whole coast flat against the map grid).
    let wave = 0.55 * vnoise(x * 0.09 + 3.1, z * 0.09 - 2.3) + 0.45 * vnoise(x * 0.19 - 1.7, z * 0.19 + 4.2);
    // Edge guard: fade the push to 0 over the last ~10 base tiles before the grid boundary, so the
    // coast is ALWAYS shaped by this noise curve and never reaches the rectangular grid edge (which
    // renders as a dead-straight "ruler" cliff — the player's complaint). The massif then meets the
    // sea on a ragged curve a few tiles inside the map. `prune_stray_islets` sinks any bit the
    // ragged swing pinches off, so the low floor can't reintroduce shore-hugging islets.
    let edge = smoothstep(2.0, 12.0, x.min(z));
    corner.powf(1.2) * (0.30 + 1.0 * wave) * edge
}

fn is_land_shape(x: f32, z: f32) -> bool {
    let dx = (x - CX).abs() / ISLAND_RX;
    let dz = (z - CZ).abs() / ISLAND_RZ;
    let r = dx.powf(ISLAND_EXP) + dz.powf(ISLAND_EXP);
    let coast = noise_a(x, z) * 0.08;
    r + coast - nw_headland(x, z) < 1.0
}

/// Smooth signed distance-ish to the SEA (the real land/water shoreline) at a base-space corner,
/// in rough base-tile units: `>0` inland, `<0` offshore, `~0` on the waterline. It's the union of
/// the island ellipse margin and the ork-fortress Blight apron (`max` = "most inside" wins), so it
/// matches the SAME coastline both landmasses are drawn from — the marching-squares shore cut welds
/// straight onto it. Uses the exact `is_land_shape` ellipse+fray so its sign never disagrees with
/// the per-tile `classify`. The ellipse margin is scaled by `ISLAND_RZ` into ~tile units so it's
/// comparable to `blight_edge_base`; the crossing interpolation is scale-invariant, so the shore
/// position is unaffected by the exact factor.
fn sea_field(x: f32, z: f32) -> f32 {
    if is_arena() {
        return arena_sea_field(x, z); // the arena is its own island (this uses home-island coords)
    }
    let dx = (x - CX).abs() / ISLAND_RX;
    let dz = (z - CZ).abs() / ISLAND_RZ;
    let r = dx.powf(ISLAND_EXP) + dz.powf(ISLAND_EXP);
    let coast = noise_a(x, z) * 0.08;
    let ellipse = (1.0 - (r + coast - nw_headland(x, z))) * ISLAND_RZ;
    ellipse.max(crate::ork_fortress::blight_edge_base(x, z))
}

fn dist_from_coast(x: f32, z: f32) -> i32 {
    let mut min = 10;
    const DIRS: [(f32, f32); 8] =
        [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)];
    for (dx, dz) in DIRS {
        for d in 1..=10 {
            if !is_land_shape(x + dx * d as f32, z + dz * d as f32) {
                if d < min {
                    min = d;
                }
                break;
            }
        }
    }
    min
}

fn dist_from_castle(x: f32, z: f32) -> f32 {
    (x - CX).hypot(z - CZ)
}

// ── Rivers: meandering courses from the highland sources down to the sea ─────────────
/// A river course in BASE space: control points from a highland SOURCE to a sea MOUTH, plus the
/// channel half-width at the source and at the mouth (rivers widen downstream). Sampled once into
/// a dense, *meandering* polyline (a perpendicular noise offset winds the course) — so the bank is
/// an organic curve, not the four axis-aligned sine branches that used to radiate from the centre.
struct RiverDef {
    pts: &'static [(f32, f32)],
    w_src: f32,
    w_mouth: f32,
}

// Home island — three rivers, each BORN at a highland and draining to a DIFFERENT coast so the
// water is spread across the map instead of clustered at the centre:
//  • Snowmelt — snow massif's SE foot, winding SW through the forest to the west coast.
//  • Minewater — the rock range's SW foot by the lake, draining south to the south coast.
//  • Dunebrook — the NE highland, running north through the desert to the north coast.
// Sources sit just OUTSIDE the peak-region radius (so `in_mountain` doesn't suppress the spring)
// and well clear of the castle safe-zone (so it isn't clipped to dry grass).
const HOME_RIVERS: &[RiverDef] = &[
    // Snowmelt — gathers in the forest SOUTH of the snow massif (clear of the peak) and runs west.
    RiverDef { pts: &[(42.0, 60.0), (30.0, 68.0), (18.0, 76.0), (6.0, 84.0)], w_src: 0.42, w_mouth: 0.78 },
    // Marsh river — west of the rock range (clear of it), draining south through the swamp.
    RiverDef { pts: &[(72.0, 84.0), (74.0, 93.0), (76.0, 102.0), (78.0, 108.0)], w_src: 0.42, w_mouth: 0.72 },
    // North brook — east frontier, clear of the rock range, to the north coast.
    RiverDef { pts: &[(78.0, 33.0), (76.0, 24.0), (74.0, 14.0), (72.0, 5.0), (70.0, -3.0)], w_src: 0.4, w_mouth: 0.64 },
];

fn active_rivers() -> &'static [RiverDef] {
    match active_id() {
        2 => &[], // the arena carries no rivers
        _ => HOME_RIVERS,
    }
}

/// Densely-sampled river centrelines for the active map plus a base-space bounding box (min x, min
/// z, max x, max z, padded) for a cheap "nowhere near any river" early-out in [`river_sd`] — which
/// the per-frame ground sampler now consults, so the box keeps it ~free away from water.
struct RiverField {
    pts: Vec<(f32, f32, f32)>,
    bb: [f32; 4],
}
fn river_points() -> Arc<RiverField> {
    static PTS: OnceLock<Mutex<HashMap<u8, Arc<RiverField>>>> = OnceLock::new();
    let cache = PTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("river point cache poisoned");
    guard
        .entry(active_id())
        .or_insert_with(|| {
            let mut pts: Vec<(f32, f32, f32)> = Vec::new();
            for (ri, def) in active_rivers().iter().enumerate() {
                let seg: Vec<f32> = def.pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).collect();
                let total: f32 = seg.iter().sum::<f32>().max(1e-3);
                let seed = ri as f32 * 17.0 + 3.0; // decorrelate each river's wander/width noise
                let mut acc = 0.0;
                for (si, w) in def.pts.windows(2).enumerate() {
                    let (ax, az) = w[0];
                    let (bx, bz) = w[1];
                    let l = seg[si].max(1e-3);
                    let (dx, dz) = ((bx - ax) / l, (bz - az) / l); // unit tangent
                    let (perpx, perpz) = (-dz, dx); // unit perpendicular
                    let steps = (l / 0.6).ceil() as i32;
                    for k in 0..=steps {
                        let s = k as f32 / steps as f32;
                        let along = acc + s * l;
                        let t = (along / total).clamp(0.0, 1.0);
                        let cx = ax + (bx - ax) * s;
                        let cz = az + (bz - az) * s;
                        // Organic MEANDER: value-noise (not a periodic sine — that read as a ruled,
                        // regular wave) sways the centreline perpendicular to the flow, two octaves,
                        // growing downstream. Marching-squares contours whatever shape this makes,
                        // so the resulting bank is smooth no matter how the course wanders.
                        let m1 = vnoise(along * 0.11 + seed, seed * 1.3) - 0.5;
                        let m2 = vnoise(along * 0.26 + seed * 2.1, seed + 8.0) - 0.5;
                        let m = (m1 * 7.0 + m2 * 3.0) * (0.5 + t * 0.6);
                        // Organic WIDTH: the channel pinches and swells along its length instead of
                        // being a constant-width canal — what made the banks read as parallel rulers.
                        let wn = vnoise(along * 0.19 + seed * 1.7, seed + 4.0); // 0..1
                        let base_half = def.w_src + (def.w_mouth - def.w_src) * t;
                        let half = base_half * (0.74 + wn * 0.6);
                        pts.push((cx + perpx * m, cz + perpz * m, half));
                    }
                    acc += l;
                }
            }
            // Padded bbox: max half (~1.4) + edge noise (~0.4) + a margin.
            let mut bb = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            for &(x, z, _) in &pts {
                bb[0] = bb[0].min(x);
                bb[1] = bb[1].min(z);
                bb[2] = bb[2].max(x);
                bb[3] = bb[3].max(z);
            }
            const PAD: f32 = 2.5;
            Arc::new(RiverField { pts, bb: [bb[0] - PAD, bb[1] - PAD, bb[2] + PAD, bb[3] + PAD] })
        })
        .clone()
}

/// Signed distance (base units) from `(x, z)` to the nearest river surface: negative inside the
/// channel, positive on land. A smooth noise term frays the edge so the bank is irregular rather
/// than a clean offset of the centreline. Does NOT apply the safe-zone / mountain guards — callers
/// pair it with [`river_blocked`].
fn river_sd(x: f32, z: f32) -> f32 {
    let field = river_points();
    // Cheap reject: far outside the rivers' bounding box → solidly land (skips the point scan, so
    // the per-frame ground sampler pays almost nothing away from water).
    if x < field.bb[0] || z < field.bb[1] || x > field.bb[2] || z > field.bb[3] {
        return 5.0;
    }
    let mut best = f32::INFINITY;
    for &(rx, rz, half) in &field.pts {
        let d = (x - rx).hypot(z - rz) - half;
        if d < best {
            best = d;
        }
    }
    // Two octaves of smooth edge fray so the bank wiggles organically at two scales (the cut
    // follows this exactly via marching-squares — it can't stairstep or spike, so the noise is
    // free to be bold). Kept under the channel half-width so the river doesn't pinch shut.
    best - omottle(x, z, 1.5, 7.0) * 0.28 - omottle(x, z, 3.4, 2.0) * 0.13
}

/// The castle safe-zone and the peak massifs carry no river (springs emerge below the peaks; the
/// keep approach stays dry). Both `is_river` and the bank margin gate on this.
fn river_blocked(x: f32, z: f32) -> bool {
    // The Blight (ork-fortress landmass) overrides the south coast in `classify` BEFORE `is_river`,
    // so a river carved there is invisible (buried under trampled mud) and a bridge over it spans
    // dry ground. Clip rivers out of it — a south-draining river just fades into the marsh at the
    // Blight's edge, which is fine.
    // Keep rivers a healthy margin OUT of the peak massifs (a wider keep-out than `in_mountain`'s
    // terrain margin): a river clipping a steep mountain base left broken slivers + carved channels
    // fighting the mountain heightfield (the "rivers in the mountains cause bugs" report).
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    let near_peak = active_map().regions.iter().any(|r| r.peak > 0 && (x - r.x).hypot(z - r.z) + wob < r.r + 5.0);
    dist_from_castle(x, z) < SAFE_R
        || near_peak
        || crate::ork_fortress::blight_class_base(x, z).is_some()
}
fn in_mountain(x: f32, z: f32) -> bool {
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    active_map().regions.iter().any(|r| r.peak > 0 && (x - r.x).hypot(z - r.z) + wob < r.r + 2.0)
}
fn is_river(x: f32, z: f32) -> bool {
    !river_blocked(x, z) && river_sd(x, z) < 0.0
}
/// Signed distance (base units, negative = water) to the deliberate lake's edge. The SAME clean
/// ellipse the lake always was — NO fray, NO resize — so `corner_water` marching-squares it into a
/// smooth shore of the identical shape/size, just without the tile-grid staircase ("square edges").
/// (An earlier pass added fray + rounded the axis to "organic-ify" it — that was overreach: the task
/// was only to de-stairstep the existing shape.)
fn lake_sd(x: f32, z: f32) -> f32 {
    if is_arena() {
        return 100.0; // the arena has no lake (this uses fixed home-island coords)
    }
    let (lx, lz, rx, rz) = DELIBERATE_LAKE;
    let dx = (x - lx) / rx;
    let dz = (z - lz) / rz;
    // Normalised ellipse value → approximate base-unit signed distance. The `= 0` contour is exactly
    // the old `dx²+dz² = 1` ellipse; marching-squares just draws it smooth instead of per-tile.
    ((dx * dx + dz * dz).sqrt() - 1.0) * rx.min(rz)
}
fn is_lake(x: f32, z: f32) -> bool {
    lake_sd(x, z) < 0.0
}

// ── Swamp bog pools (map-character overhaul pass 3) ─────────────────────────────────
/// Authored bog-pool blobs per flat swamp region, in REGION-LOCAL fractions of its radius:
/// `(dx/r, dz/r, rx/r, rz/r)`. AUTHORED (like [`DELIBERATE_LAKE`]), not noise-thresholded —
/// two attempts at deriving pools from a `noise_a` wetness field failed silently because the
/// field's local range inside the swamp regions (max ≈ 0.06–0.37, measured) sits nowhere near
/// its global amplitude, so any fixed threshold is phase-luck. Blobs guarantee pools exist,
/// are big enough to render murky (≥ ~4 base tiles across — smaller reads as an all-white
/// foam plate), and have genuinely deep centres for the bog dressing's gates; `noise_a` only
/// waves their SHORES. Offsets avoid the S-swamp warden's arena direction (region-local
/// ≈(0,−0.42) — see the keep-out below, which still hard-guards it).
// A FEW small, well-separated puddles (not the old 6-blob amoeba clusters): each swamp region
// gets ~2–3 small pools reading as distinct kałuże. Biased AWAY from the E-marsh's western edge
// (positive-x / southern offsets) so, in that region, they don't crowd the waterfall's blue lake
// just west of it (the lake keep-out below also hard-guards it). Min radius fraction stays ≥ 0.11
// (≈2.5 base at r23 / 2 base at r17) so a pool is never so small it renders as an all-white foam
// plate. The deepest still clears the drowned-tower gate (`bog.rs`, sd < −1.1).
const POOL_BLOBS: [(f32, f32, f32, f32); 3] = [
    (0.28, -0.24, 0.15, 0.12), // NE puddle
    (0.14, 0.36, 0.14, 0.13),  // SE puddle
    (-0.22, 0.12, 0.12, 0.11), // W-central (smallest; lake-side in the E-marsh — keep-out trims it)
];

/// World-space keep-out around the swamp warden's arena — a pool inside the boss glade would
/// carve the fight arena into islands (warden `region_center` (0,57), GLADE_R 16 + roam 13).
const POOL_WARDEN_KEEPOUT: f32 = 22.0;

/// The lake's BLUE feeder + drain capsules (BASE space, `((ax,az),(bx,bz),half)`): the water that
/// keeps the deliberate lake a *living* body — a waterfall pours IN, a brook drains OUT — instead
/// of a stagnant blue dot. Rendered as CLEAR water, NOT olive bog: the bog murk mask keys on
/// `pool_sd` alone, so everything here stays blue even where the brook threads the marsh. They
/// share the lake's plumbing (carve + footing + scatter/prop rejection) via [`is_pool`].
///  [0] — the WATERFALL PLUNGE (map-character overhaul pass 5): from the rock mesa's SW tier-wall
///        foot (world ≈(60.3, 32.6)) down to the lake edge. The wall stands ~12 base units back
///        across a dry shelf, so without this the falls poured onto grass; `vista::populate`
///        stands the cascade at capsule [0]'s head.
///  [1..] — the OUTFLOW BROOK: the lake's south edge winding SW down to the marsh river
///        (`HOME_RIVERS[1]` at ≈(76,102)), so lake water rejoins the course that drains to the
///        south coast. Kept clear of the olive bog pools (see the lake keep-out in `pool_sd`).
const BLUE_STREAMS: &[((f32, f32), (f32, f32), f32)] = &[
    ((99.4, 68.8), (94.2, 77.6), 1.1), // [0] waterfall plunge (mesa wall → lake)
    // [1..] the OUTFLOW BROOK, a gently WESTWARD-bending curve (not a ruler-straight canal) from
    // the lake's south edge down to the marsh-river confluence at ≈(76.5,102):
    ((91.0, 82.5), (90.0, 86.5), 0.70),
    ((90.0, 86.5), (87.8, 90.0), 0.72),
    ((87.8, 90.0), (85.5, 93.0), 0.73),
    ((85.5, 93.0), (82.0, 95.5), 0.74),
    ((82.0, 95.5), (79.0, 98.5), 0.75),
    ((79.0, 98.5), (76.5, 102.0), 0.76),
];

/// Signed distance (base units, negative = water) to the nearest blue feeder/drain capsule. A
/// per-position noise term frays the bank so the brook reads as an organic stream, not a smooth
/// pipe (a touch stronger than the lake's shore wave — a brook's edge is rougher than a lakeshore).
fn stream_sd(x: f32, z: f32) -> f32 {
    if is_arena() {
        return 100.0; // no waterfall/brook on the arena (this uses fixed home-island coords)
    }
    let mut best = f32::INFINITY;
    for &((ax, az), (bx, bz), half) in BLUE_STREAMS {
        let (abx, abz) = (bx - ax, bz - az);
        let t = (((x - ax) * abx + (z - az) * abz) / (abx * abx + abz * abz)).clamp(0.0, 1.0);
        let (px, pz) = (ax + abx * t, az + abz * t);
        best = best.min((x - px).hypot(z - pz) - half);
    }
    best + noise_a(x * 0.6, z * 0.6) * 0.4
}

/// The cascade anchor for `vista` (world space): the plunge capsule's head at the mesa wall foot,
/// and the flow direction (head → lake).
pub fn waterfall_site_world() -> (Vec2, Vec2) {
    let ((ax, az), (bx, bz), _) = BLUE_STREAMS[0];
    let head = Vec2::new(ax * MAP_SCALE - GX, az * MAP_SCALE - GZ);
    let mouth = Vec2::new(bx * MAP_SCALE - GX, bz * MAP_SCALE - GZ);
    (head, (mouth - head).normalize())
}

/// Signed distance (approx base units; negative = water) to the swamp bog-pool blobs, or +INF
/// away from them. Self-contained (region + warden + Blight gating INSIDE) so `classify`,
/// `corner_water` and `smooth_surface_y` all see the same shoreline.
fn pool_sd(x: f32, z: f32) -> f32 {
    let mut best = f32::INFINITY;
    for reg in active_map().regions {
        if reg.biome != TB::Swamp || reg.peak != 0 {
            continue;
        }
        if (x - reg.x).hypot(z - reg.z) > reg.r {
            continue; // fast reject — blobs live well inside the region
        }
        for (fx, fz, frx, frz) in POOL_BLOBS {
            let (cx, cz) = (reg.x + fx * reg.r, reg.z + fz * reg.r);
            let (rx, rz) = (frx * reg.r, frz * reg.r);
            let dx = (x - cx) / rx;
            let dz = (z - cz) / rz;
            // Normalised ellipse → approx base-unit signed distance (same trick as `lake_sd`).
            let sd = ((dx * dx + dz * dz).sqrt() - 1.0) * rx.min(rz);
            best = best.min(sd);
        }
    }
    if best == f32::INFINITY {
        return best;
    }
    // Swamp warden arena keep-out (world → base). The glade coord is authored at scale 2.2.
    let wx = x * MAP_SCALE - GX;
    let wz = z * MAP_SCALE - GZ;
    let swamp_glade = world22(0.0, 57.0);
    if (wx - swamp_glade.x).hypot(wz - swamp_glade.y) < POOL_WARDEN_KEEPOUT {
        return f32::INFINITY;
    }
    // The Blight owns its own ground (checked before regions in `classify`) — keep pools out.
    if crate::ork_fortress::blight_class_base(x, z).is_some() {
        return f32::INFINITY;
    }
    // Keep the olive bog pools OFF the waterfall's BLUE plunge pool — the E-marsh laps right up to
    // it, and an olive puddle bleeding onto the blue lake read as one muddy blob. Reject anything
    // inside the lake ellipse grown by a margin (so a pool can't even touch its shore).
    let (lx, lz, lrx, lrz) = DELIBERATE_LAKE;
    if ((x - lx) / (lrx + 4.0)).powi(2) + ((z - lz) / (lrz + 4.0)).powi(2) < 1.0 {
        return f32::INFINITY;
    }
    // Organic shoreline: wave the ellipse edge with noise. Only nibbles ±0.9 base units, so a
    // blob's guaranteed deep core (−rx·min ≈ −3..−5) survives.
    best + noise_a(x * 0.45, z * 0.45) * 0.9
}

/// Full "still water" field: the swamp bog pools PLUS the waterfall plunge stream. Every
/// consumer (carve, corner shore, footing, murk mask, boardwalks) reads this combined field.
fn pool_or_stream_sd(x: f32, z: f32) -> f32 {
    pool_sd(x, z).min(stream_sd(x, z))
}

fn is_pool(x: f32, z: f32) -> bool {
    pool_or_stream_sd(x, z) < 0.0
}

/// World-space still-water test — bog pools + the waterfall plunge stream (scatter / roads /
/// bridges / the murk mask all query in world coords).
pub fn is_pool_world(wx: f32, wz: f32) -> bool {
    is_pool((wx + GX) / MAP_SCALE, (wz + GZ) / MAP_SCALE)
}

/// Is world `(wx, wz)` inside a flat swamp region's footprint? The water-murk mask uses it so
/// the RIVER stretches that thread the marsh render as murky bog water too — a vivid teal
/// channel slicing between dark pools read as a glaring seam (verification flag).
fn in_swamp_region_world(wx: f32, wz: f32) -> bool {
    let bx = (wx + GX) / MAP_SCALE;
    let bz = (wz + GZ) / MAP_SCALE;
    active_map()
        .regions
        .iter()
        .any(|r| r.biome == TB::Swamp && r.peak == 0 && (bx - r.x).hypot(bz - r.z) < r.r - 2.0)
}

/// Signed distance in approx BASE units at world coords (bog dressing uses it to pick "deep
/// enough" spots for drowned trees and to hug pool shores with mushrooms/wisps).
pub fn pool_sd_world(wx: f32, wz: f32) -> f32 {
    pool_sd((wx + GX) / MAP_SCALE, (wz + GZ) / MAP_SCALE)
}

fn edge_fray(x: f32, z: f32) -> f32 {
    (x * 0.5 + z * 0.35 + 1.3).sin() * 1.1
        + (x * 0.9 - z * 0.82 + 4.0).sin() * 1.6
        + (x * 1.5 + z * 1.3 + 2.2).sin() * 1.0
}

fn region_at(x: f32, z: f32) -> Option<usize> {
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    let mut best: Option<usize> = None;
    let mut best_edge = f32::INFINITY;
    for (i, reg) in active_map().regions.iter().enumerate() {
        let fray = if reg.peak > 0 { 0.0 } else { edge_fray(x, z) };
        let d = (x - reg.x).hypot(z - reg.z) + wob + fray;
        let edge = d - reg.r;
        if edge < 0.0 && edge < best_edge {
            best_edge = edge;
            best = Some(i);
        }
    }
    best
}

/// Terraced coastal ridge height class (≥2) at base `(x, z)`, or 0 for no hill. A low-frequency
/// mask picks which stretches of coast get a mountain ring (the rest stays open beach/plain).
/// Height rises MONOTONICALLY toward the sea — tallest just behind the beach, tapering to flat
/// inland — so the inland face is a climbable terraced ramp (≤1 class per step for the
/// nav-grid) while the seaward face drops to the beach as a sheer cliff: the ridge tops are
/// reachable from one side only.
fn coast_hill_class(x: f32, z: f32) -> i32 {
    let d = dist_from_coast(x, z) as f32;
    if !(2.0..=7.0).contains(&d) {
        return 0;
    }
    let mask = (x * 0.045 + 1.7).sin() + (z * 0.05 - 0.6).cos() + noise_a(x * 0.5, z * 0.5) * 0.25;
    if mask < 0.3 {
        return 0;
    }
    let band = ((7.0 - d) / 4.5).clamp(0.0, 1.0); // 0 inland (d=7) → 1 at the coast side
    let h = 1.0 + mask.min(1.6) * 5.0 * band + noise_b(x * 1.1 + 5.0, z * 1.1) * 0.7;
    let cls = h.round().clamp(1.0, 9.0) as i32;
    if cls >= 2 { cls } else { 0 }
}

/// Gentle terraced inland hills (height class 1..=`max`) layered over every flat part of the
/// island — grassy knolls on the frontier, dunes in the desert, wooded rises in the forest —
/// so the middle of the map isn't one flat sheet. Nested thresholds mean a hill always climbs
/// through each class ring in turn, keeping every face a 1-class step the nav-grid can walk;
/// inside `SAFE_R + 8` it stays flat so the castle approaches and siege lanes stay open.
fn inland_hills(x: f32, z: f32, dc: f32, max: i32) -> i32 {
    if dc <= SAFE_R + 8.0 {
        return 1;
    }
    // Thresholds lowered + a 5th class added (map-character overhaul feedback: "musi być
    // więcej drobnych pagórków") — the frontier rolls noticeably now instead of reading as a
    // flat sheet with rare knolls. Still nested → every face stays a 1-class walkable step.
    let roll = noise_a(x * 0.3 + 9.0, z * 0.3 - 5.0) + noise_b(x * 0.16 - 4.0, z * 0.16 + 8.0) * 0.6;
    let h = if roll > 2.05 {
        5
    } else if roll > 1.6 {
        4
    } else if roll > 1.15 {
        3
    } else if roll > 0.7 {
        2
    } else {
        1
    };
    h.min(max.max(1))
}

fn plateau_height(x: f32, z: f32) -> i32 {
    for p in &PLATEAUS {
        let d = (x - p.x).hypot(z - p.z);
        if d >= p.r {
            continue;
        }
        let tiers = (p.peak - 1) as f32;
        let cls = 2 + ((1.0 - d / p.r) * tiers).floor() as i32;
        return cls.clamp(2, p.peak);
    }
    0
}

// Half-width of the castle-facing ramp corridor up each peak region (base tiles). Widened
// 1.7→3.0 so the rock range / snow massif climb is a broad avenue, not a one-tile stair.
const RAMP_HALF_TILES: f32 = 3.0;
fn ramp_class(x: f32, z: f32, reg: &Region) -> Option<i32> {
    if reg.peak <= 0 {
        return None;
    }
    let dx = x - reg.x;
    let dz = z - reg.z;
    let dc = dx.hypot(dz);
    if dc >= reg.r {
        return None;
    }
    let ramp_ang = (CZ - reg.z).atan2(CX - reg.x);
    let mut da = (dz.atan2(dx) - ramp_ang) % std::f32::consts::TAU;
    if da < -std::f32::consts::PI {
        da += std::f32::consts::TAU;
    }
    if da > std::f32::consts::PI {
        da -= std::f32::consts::TAU;
    }
    let half_ang = (RAMP_HALF_TILES / dc.max(1.5)).min(std::f32::consts::PI);
    if da.abs() >= half_ang {
        return None;
    }
    let span = (reg.peak - 2).max(1) as f32;
    let step_len = reg.r / span;
    let cls = 2 + ((reg.r - dc) / step_len).floor() as i32;
    Some(cls.clamp(2, reg.peak))
}

fn mountain_height(x: f32, z: f32, reg: &Region) -> i32 {
    if let Some(rc) = ramp_class(x, z, reg) {
        return rc;
    }
    let dc = (x - reg.x).hypot(z - reg.z);
    let peak = reg.peak;
    let t = (1.0 - dc / reg.r).max(0.0);
    // Noise weight kept LOW (was 0.35 + t·0.95): noise_b swings ±1.4, so the old weight stamped
    // frequent ≥2-class jumps between neighbour tiles — sheer cliff pockets `can_step` refuses,
    // which walled off most of the range. Now slopes are mostly 1-class walkable terraces.
    let h = (peak as f32 * t * t + noise_b(x, z) * (0.2 + t * 0.4)).round() as i32;
    h.clamp(1, peak)
}

// ── Tiered mesas (cliffy peak regions — map-character overhaul pass 1) ─────────────
/// Normalised radial "altitude" 0..1 into a cliffy region, with per-massif anisotropy and
/// two octaves of rim distortion — the player report on v1 was "dwie takie same góry,
/// zupełnie okrągłe": the snow massif now runs elongated (squashed X), the rock range squat
/// and lobed, and the broad low-frequency octave deforms each WHOLE silhouette (not just the
/// rims) so neither reads as a compass-drawn circle.
fn mesa_t(x: f32, z: f32, reg: &Region) -> f32 {
    let (sx, sz) = if reg.biome == TB::Snow { (0.82, 1.20) } else { (1.12, 0.88) };
    let dx = (x - reg.x) * sx;
    let dz = (z - reg.z) * sz;
    // The snow massif runs MORE irregular than the rock range (player: "bardziej nieregularna"):
    // a heavier broad-lobe deformation + an extra mid-frequency spur octave warp its whole
    // silhouette into ragged spurs, so its enlarged flanks meet the NW sea-cliff on a jagged
    // front rather than a smooth arc. The rock range keeps its tuned squat-lobed shape.
    let (lobe, spur, rim) = if reg.biome == TB::Snow { (8.5, 4.0, 3.6) } else { (5.5, 0.0, 3.0) };
    let dc = dx.hypot(dz)
        + noise_b(x * 0.085 + 7.0, z * 0.085 - 3.0) * lobe // broad lobes: silhouette deformation
        + noise_a(x * 0.17 - 2.0, z * 0.17 + 5.0) * spur // mid spurs (snow only)
        + noise_a(x * 0.33, z * 0.33) * rim; // fine rim waviness
    (1.0 - dc / reg.r).clamp(0.0, 1.0)
}

/// Fraction of the way up each tier begins (`mesa_height` ladder). Rock: 5 tiers — broad low
/// shelves, a small summit cap. Snow: 4 (taller walls, a different stepping rhythm — the two
/// massifs must not read as twins).
const MESA_TIERS: [f32; 5] = [0.15, 0.34, 0.54, 0.74, 0.91];
const SNOW_TIERS: [f32; 4] = [0.17, 0.44, 0.70, 0.90];

/// Tiered mesa height for a `cliffy` region: flat shelves separated by multi-class SHEER walls
/// (rock peak 22 → shelf classes 2/7/12/17/22 = 2.5u cliffs between shelves at `GROUND_STEP`
/// 0.5). Both the nav-grid (`can_step`) and the hero (`hero_can_step`) refuse >1-class climbs,
/// so the walls block everyone with zero new logic — downward ledges stay droppable (one-way
/// descents off a shelf work). The authored pass corridors are the only ways UP; outside the
/// lowest tier a 1–2-class apron ring meets the surrounding terrain smoothly.
fn mesa_height(x: f32, z: f32, reg: &Region) -> i32 {
    if let Some(rc) = pass_class(x, z, reg) {
        return rc;
    }
    let t = mesa_t(x, z, reg);
    let tiers: &[f32] = if reg.biome == TB::Snow { &SNOW_TIERS } else { &MESA_TIERS };
    if t < tiers[0] {
        // Foot apron: low fringe so the mesa rises out of walkable ground, not a moat of cliff.
        return if t > tiers[0] * 0.5 { 2 } else { 1 };
    }
    let span = (reg.peak - 2).max(1) as f32;
    let n = tiers.len();
    let mut tier = 0;
    for (i, th) in tiers.iter().enumerate() {
        if t >= *th {
            tier = i;
        }
    }
    2 + (span * tier as f32 / (n - 1) as f32).round() as i32
}

/// Lateral serpentine of a pass corridor's centreline: an angular offset at ring distance `dc`
/// holding a roughly CONSTANT ~2.4-base-unit lateral amplitude, so the ascent snakes up the
/// mountain in S-curves instead of running a ruler-straight radial line ("idealnie prosta
/// droga" report). Phase-salted per region + pass so no two ascents share a curve.
fn pass_sway(dc: f32, reg: &Region, p: &Pass) -> f32 {
    ((dc * 0.21 + reg.x * 0.7 + p.ang * 3.0).sin() * 2.4) / dc.max(2.4)
}

/// Smooth ≤1-class ramp staircase inside any of a cliffy region's authored [`Pass`] corridors
/// (None outside them). Same angular-corridor construction as [`ramp_class`], but per authored
/// pass instead of the single castle-facing default — and serpentined by [`pass_sway`]. The
/// corridor walls — the jump between this ramp and the neighbouring mesa shelf — read as a
/// climbing canyon for free.
fn pass_class(x: f32, z: f32, reg: &Region) -> Option<i32> {
    let dx = x - reg.x;
    let dz = z - reg.z;
    let dc = dx.hypot(dz);
    if dc >= reg.r {
        return None;
    }
    for p in reg.passes {
        let mut da = (dz.atan2(dx) - p.ang - pass_sway(dc, reg, p)) % std::f32::consts::TAU;
        if da < -std::f32::consts::PI {
            da += std::f32::consts::TAU;
        }
        if da > std::f32::consts::PI {
            da -= std::f32::consts::TAU;
        }
        let half_ang = (p.half / dc.max(1.5)).min(std::f32::consts::PI);
        if da.abs() < half_ang {
            let span = (reg.peak - 2).max(1) as f32;
            let step_len = reg.r / span;
            let cls = 2 + ((reg.r - dc) / step_len).floor() as i32;
            return Some(cls.clamp(2, reg.peak));
        }
    }
    None
}

/// True if base `(x, z)` classifies into a cliffy mesa region OR one of its pass corridors —
/// these tiles are EXEMPT from `terrace_inland`'s ≤1-class relaxation (the shelf walls are the
/// point, and relaxing a corridor tile toward a neighbouring low bay would break the staircase)
/// and from the climbability test (the corridors' own smoothness is covered by
/// `mesa_passes_climb_to_the_summit`).
fn cliff_exempt_base(x: f32, z: f32) -> bool {
    if matches!(region_at(x, z), Some(ri) if {
        let r = &active_map().regions[ri];
        r.cliffy && r.peak > 0
    }) {
        return true;
    }
    active_map().regions.iter().any(|r| r.cliffy && r.peak > 0 && pass_class(x, z, r).is_some())
}

/// True near (within `pad` base tiles outside) a cliffy region's rim — the flat "contrast apron"
/// band: tall only reads tall next to flat, so the rolling inland hills are suppressed there.
fn near_cliffy_rim(x: f32, z: f32, pad: f32) -> bool {
    active_map().regions.iter().any(|r| {
        r.cliffy && r.peak > 0 && {
            let d = (x - r.x).hypot(z - r.z);
            (r.r - 2.0..r.r + pad).contains(&d)
        }
    })
}

/// True inside (with margin) any cliffy region — plateaus skip these (a smooth plateau cone
/// punched into the mesa tiers would carve a walkable breach through the walls).
fn in_cliffy(x: f32, z: f32) -> bool {
    active_map()
        .regions
        .iter()
        .any(|r| r.cliffy && r.peak > 0 && (x - r.x).hypot(z - r.z) < r.r + 2.0)
}

// ── Cliffy-mesa exports (roads / placement) ──────────────────────────────────────
/// Base-space mouth of a pass corridor: just outside the region rim along the (serpentined)
/// pass angle, so the road trunk meets the trail where the corridor actually opens.
fn pass_mouth_base(reg: &Region, p: &Pass) -> (f32, f32) {
    let dc = reg.r + 1.0;
    let a = p.ang + pass_sway(dc, reg, p);
    (reg.x + a.cos() * dc, reg.z + a.sin() * dc)
}

/// World-space centres of the active map's five primary biome regions (the road network's
/// trunk/ring skeleton — index 5, the home marsh arm, is deliberately not a road anchor).
pub fn biome_centres_world() -> [Vec2; 5] {
    if is_arena() {
        // The arena has no biome blobs; the campaign `roads` network (the only consumer) is not
        // built in skirmish. Return the two bases padded so a stray call can't index the empty
        // region slice and panic.
        return [crate::rts::PLAYER_BASE, crate::rts::RIVAL_BASE, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO];
    }
    let rs = active_map().regions;
    std::array::from_fn(|i| Vec2::new(rs[i].x * MAP_SCALE - GX, rs[i].z * MAP_SCALE - GZ))
}

/// Where the trunk road for biome region `i` should END: the region centre — or, for a cliffy
/// mesa, the castle-facing pass MOUTH (`passes[0]`): a painted road must never run up the shelf
/// walls; the pass trail (`pass_trails_world`) continues the route to the summit.
pub fn biome_road_target(i: usize) -> Vec2 {
    let reg = &active_map().regions[i];
    if reg.cliffy && !reg.passes.is_empty() {
        let (mx, mz) = pass_mouth_base(reg, &reg.passes[0]);
        Vec2::new(mx * MAP_SCALE - GX, mz * MAP_SCALE - GZ)
    } else {
        Vec2::new(reg.x * MAP_SCALE - GX, reg.z * MAP_SCALE - GZ)
    }
}

/// Is biome region `i` a cliffy mesa (snow / rock)? `roads` skips coast reaches for these — a
/// painted road to their shore would climb the shelf walls; their routes are the pass trails.
pub fn region_is_cliffy(i: usize) -> bool {
    active_map().regions[i].cliffy
}

/// A coastal road endpoint reaching OUT from `from` (a biome road target) toward the nearest
/// shore, so a biome's outer/coastal ground isn't left off the network (players noted parts of
/// the island were cut off). Marches radially outward — the island centre is the castle at the
/// world origin — stepping across the odd narrow river gap but stopping at the open sea, then
/// backs a couple units inland so the road ends on the beach, not in the surf. `None` when `from`
/// is already near the coast (nothing meaningful to extend) or points nowhere.
pub fn coast_reach_world(from: Vec2) -> Option<Vec2> {
    let dir = from.normalize_or_zero();
    if dir == Vec2::ZERO {
        return None;
    }
    let mut last_land = from;
    let mut d = 4.0;
    while d < 150.0 {
        let p = from + dir * d;
        if ground_at_world(p.x, p.y).is_some() {
            last_land = p;
        } else {
            // Water: a short gap with land beyond is a river/pool to step over; sustained water
            // (or the open sea here) is the coast — stop.
            let ahead = from + dir * (d + 7.0);
            if ground_at_world(ahead.x, ahead.y).is_none() || is_open_water_world(p.x, p.y) {
                break;
            }
        }
        d += 1.5;
    }
    let end = last_land - dir * 2.5;
    (end.distance(from) >= 12.0).then_some(end)
}

/// Ring-road node for biome region `i` when the segment approaches from `other` (world): flat
/// regions anchor at their centre; a cliffy mesa anchors at whichever pass mouth lies nearest
/// the approach, so the ring threads a pass gate instead of climbing the walls (the desert↔rock
/// segment runs the N slot canyon this way).
pub fn biome_ring_node(i: usize, other: Vec2) -> Vec2 {
    let reg = &active_map().regions[i];
    if !reg.cliffy || reg.passes.is_empty() {
        return Vec2::new(reg.x * MAP_SCALE - GX, reg.z * MAP_SCALE - GZ);
    }
    reg.passes
        .iter()
        .map(|p| {
            let (mx, mz) = pass_mouth_base(reg, p);
            Vec2::new(mx * MAP_SCALE - GX, mz * MAP_SCALE - GZ)
        })
        .min_by(|a, b| a.distance(other).partial_cmp(&b.distance(other)).unwrap())
        .unwrap()
}

/// The pass-corridor trails (world-space polylines, mouth → summit) of every cliffy region on
/// the active map — painted by `roads` as mountain tracks, so the only ways up the mesas read
/// as marked routes.
pub fn pass_trails_world() -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    for reg in active_map().regions {
        if !reg.cliffy || reg.peak <= 0 {
            continue;
        }
        for p in reg.passes {
            let mut pts = Vec::new();
            let mut dc = reg.r + 1.0;
            while dc > 2.0 {
                // Follow the serpentined corridor centreline (`pass_sway`), so the painted
                // trail snakes exactly where the walkable ramp runs.
                let a = p.ang + pass_sway(dc, reg, p);
                pts.push(Vec2::new(
                    (reg.x + a.cos() * dc) * MAP_SCALE - GX,
                    (reg.z + a.sin() * dc) * MAP_SCALE - GZ,
                ));
                dc -= 2.0;
            }
            out.push(pts);
        }
    }
    out
}

/// True on a HIGH shelf (or high ramp stretch) inside a cliffy mesa (world coords) — placement
/// that must stay trivially reachable from the road level (the ruins landmarks) rejects these.
pub fn cliff_shelf_world(wx: f32, wz: f32) -> bool {
    let bx = (wx + GX) / MAP_SCALE;
    let bz = (wz + GZ) / MAP_SCALE;
    cliff_exempt_base(bx, bz) && ground_at_world(wx, wz).is_some_and(|y| y > 0.9)
}

// ── Arena (RTS skirmish) ──────────────────────────────────────────────────────────
// A small symmetric grass island centred on the world origin. Everything here is authored in
// WORLD space (rts::PLAYER_BASE etc., castle-at-origin frame); `classify_arena` converts the
// base-space grid coord up to world and reasons there, so the arena constants read directly. No
// biome blobs, rivers, lake or mountains — just two force-flattened base plateaus, a dirt road
// between them and mirrored, finite deposit spots. The layout mirrors through the origin (a 180°
// rotation swaps the two sides), so it plays fair.

/// Land radius (world units) of the arena island — comfortably over the base plateaus' far reach
/// (base centre ≈62u out + plateau radius 20 = ~82) plus a wide beach ring, so both bases sit on
/// solid ground with ocean beyond. `rts::ARENA_RADIUS` (72) is the open *play* core; the land
/// extends past it to hold the diagonally-placed bases and the outer biome regions.
/// `pub` so `rts::minimap` frames the real island instead of duplicating the number.
pub const ARENA_LAND_R: f32 = 104.0;
/// Sandy beach ring width (world units) just inside the waterline — the foreshore, a full height
/// class BELOW the inland field (see [`ARENA_FLAT_CLASS`]) so the island ramps down into the sea.
const ARENA_BEACH_W: f32 = 12.0;
/// How far (world units) the organic coast noise swings the waterline in/out of the base circle.
/// Big enough to read as real capes and coves rather than a wobble on a compass circle.
const ARENA_COAST_FRAY: f32 = 7.0;
/// Force-flat base-plateau radius (world units) around each `rts` base centre — level grass
/// building ground, the same "flatten to a comfortable class" trick the castle safe-zone / town
/// build-plots use. Sized to seat a full `POP_HARD_CAP` town.
const ARENA_BASE_R: f32 = 20.0;
/// The height class every force-flat arena spot (plateaus, road, deposits) is pinned to. It sits
/// **one class above the beach** (which is class 1, ≈ sea level) — that step is what gives the
/// island a sloped foreshore instead of the vertical wall an all-class-1 island ended in. The
/// terrain mesh smooths corner heights across the step, so it renders as a continuous ramp up
/// from the waterline, not a stair. Rolling knolls rise one class above this.
const ARENA_FLAT_CLASS: i32 = 2;
/// Visible dirt-road half-width and full-strength packed core (world units) — mirrors an artery's
/// `roads::HALF_W`/core so the arena road reads like a campaign road. Widened with the map so the
/// lane still reads as a highway between two distant bases.
const ARENA_ROAD_HALF: f32 = 4.0;
const ARENA_ROAD_CORE: f32 = 1.8;
/// Force-flat corridor half-width along the road (a touch wider than the visible road, so the
/// packed dirt always sits on level ground and units cross it cleanly).
const ARENA_ROAD_FLAT: f32 = 5.2;
/// Inner radius (world units) of the decorative wooded fringe: trees ring the outer field so the
/// centre stays an open battlefield.
const ARENA_FOREST_R0: f32 = 48.0;
/// Keep trees this far (world units) from a base centre — the plateau (20) plus a clear apron.
const ARENA_BASE_CLEAR: f32 = 25.0;
/// Force-flat disc radius around each deposit spot (kept level + tree-free for the props).
const ARENA_DEPOSIT_FLAT: f32 = 6.0;
/// Keep decorative trees this far from a deposit spot so the grove/outcrop reads cleanly.
const ARENA_DEPOSIT_CLEAR: f32 = 9.0;
/// Keep decorative trees this far from the road centreline so the lane between the bases stays
/// clear end-to-end (comfortably wider than the visible road so nothing crowds it).
const ARENA_ROAD_CLEAR: f32 = 8.0;

/// Mirrored deposit spots (world XZ). Per kind: `[0]` = player-side (just outside the SW base),
/// `[1]` = rival-side (the negation through the origin), `[2]` = the contested centre spot, and
/// (wood/stone) `[3]`/`[4]` = a mirrored SECOND pair further in — the enlarged island has the room
/// and the per-site stock is small enough that one grove no longer feeds a whole town. The
/// contested spots sit on the fair bisector (`z = x`, perpendicular to the road), so each is
/// equidistant from both bases. `rts::deposits` spawns the actual groves / outcrops / veins and
/// treats index `2` as the richer contested site.
const ARENA_WOOD: [Vec2; 5] = [
    Vec2::new(-22.0, 64.0),
    Vec2::new(22.0, -64.0),
    Vec2::new(-26.0, -26.0), // contested
    Vec2::new(-6.0, 44.0),
    Vec2::new(6.0, -44.0),
];
const ARENA_STONE: [Vec2; 5] = [
    Vec2::new(-64.0, 22.0),
    Vec2::new(64.0, -22.0),
    Vec2::new(36.0, 36.0), // contested
    Vec2::new(-40.0, 10.0),
    Vec2::new(40.0, -10.0),
];
/// Gold stays deliberately SCARCE — only one vein per side plus the contested one. Gold is the
/// soldier resource, so keeping it rare is what makes the contested vein (and the Market) worth
/// fighting for.
const ARENA_GOLD: [Vec2; 3] = [Vec2::new(-16.0, 51.0), Vec2::new(16.0, -51.0), Vec2::new(12.0, 12.0)];

/// Ornamental LAKES (world XZ centres + radius) — a mirrored pair, one per side, off the
/// base-to-base lane and clear of every deposit. (It was a single west lake balanced by an east
/// hill; mirroring it makes the water fair to both sides.) `classify_arena` carves them (returns
/// `None`, so the sea plane shows through as shallow water); the shore is cut smooth by the same
/// marching-squares contour the coast uses, and shore foam bakes automatically from the water mask.
const ARENA_LAKES: [Vec2; 2] = [Vec2::new(-44.0, -12.0), Vec2::new(44.0, 12.0)];
const ARENA_LAKE_R: f32 = 14.0;
/// Plain rocky hills — a mirrored pair of coastal bluffs. Walkable terraced cones (no nav
/// blockers, no deposit); `ground_color_arena` tints their tiles rock-grey and `rts::deposits`
/// crowns each with a boulder mound.
const ARENA_HILLS: [Vec2; 2] = [Vec2::new(-22.0, 76.0), Vec2::new(22.0, -76.0)];
const ARENA_HILL_R: f32 = 12.0;

/// Iron-ore MOUNTAINS: a mirrored pair (player N / rival S — negated through the origin, so it
/// plays fair). Each is a broad rocky massif (wider + darker than the plain hills) with a rich
/// **iron-ore** vein — mechanically an ordinary Stone [`crate::rts::Deposit`], but rendered with
/// rust-veined ore boulders in a dark-grey rockscape — flattened into a level bowl at its foot.
const ARENA_MOUNTS: [Vec2; 2] = [Vec2::new(8.0, 70.0), Vec2::new(-8.0, -70.0)];
const ARENA_MOUNT_R: f32 = 20.0;

/// FOREST patches: a mirrored pair on the outer field, off the open battle centre. Each is a dense
/// wood grove in a greener, shadier clearing — extra timber, one stand per side, so the woodland
/// is fair.
const ARENA_FOREST: [Vec2; 2] = [Vec2::new(-72.0, 8.0), Vec2::new(72.0, -8.0)];
const ARENA_FOREST_R: f32 = 18.0;

/// SNOW highlands: a mirrored pair straddling the fair bisector (`z = x`) — each sits exactly
/// equidistant from both bases, so neither side owns one. A cold, terraced massif: raised
/// [`TB::Snow`] on a walkable slope, tinted to a bright frost-white cap in `ground_color_arena`.
/// Gives the two far lobes of the enlarged island a place of their own instead of more lawn.
const ARENA_SNOW: [Vec2; 2] = [Vec2::new(51.0, 51.0), Vec2::new(-51.0, -51.0)];
const ARENA_SNOW_R: f32 = 17.0;

/// DUNE scrub: a mirrored pair of dry, sandy flats on the outer field — [`TB::Desert`] at the
/// normal field height (no rise), tinted sand-gold, carrying the desert scatter (cacti, dry rocks).
/// A warm counterweight to the snow lobes.
const ARENA_DUNES: [Vec2; 2] = [Vec2::new(72.0, 26.0), Vec2::new(-72.0, -26.0)];
const ARENA_DUNE_R: f32 = 13.0;

/// The arena rocky-hill centres (world XZ) + their radius — exposed so the `rts` deposits module can
/// crown each terrain hill with a cosmetic boulder mound (the terrain tint alone read too flat).
pub fn arena_hills() -> (Vec<Vec2>, f32) {
    (ARENA_HILLS.to_vec(), ARENA_HILL_R)
}

/// The arena iron-ore mountain centres (world XZ) + their radius — exposed so the `rts` deposits
/// module can crown each massif with grey crags on its slopes (leaving the central ore bowl clear).
pub fn arena_mountains() -> (Vec<Vec2>, f32) {
    (ARENA_MOUNTS.to_vec(), ARENA_MOUNT_R)
}

/// Raised height class of the arena rocky hills at world `(wx,wz)` — a terraced cone peaking ~2
/// classes above the flat field — or `None` if outside every hill. Shared by [`classify_arena`]
/// (terrain) and [`ground_color_arena`] (grey tint) so the colour matches the raised tiles.
fn arena_hill_class(wx: f32, wz: f32) -> Option<i32> {
    arena_cone_class(wx, wz, &ARENA_HILLS, ARENA_HILL_R, 2.0)
}

/// Raised height class of the arena iron-ore mountains at world `(wx,wz)` — a broad terraced
/// massif (wider than the hills, same ≤1-step-per-tile walkable slope) — or `None` if outside every
/// mountain. Shared by [`classify_arena`] (terrain) and [`ground_color_arena`] (dark-grey tint).
fn arena_mount_class(wx: f32, wz: f32) -> Option<i32> {
    arena_cone_class(wx, wz, &ARENA_MOUNTS, ARENA_MOUNT_R, 2.0)
}

/// Raised height class of the arena SNOW highlands at world `(wx,wz)` — a terraced cold massif,
/// same walkable ≤1-step slope — or `None` if outside every highland. Shared by [`classify_arena`]
/// (terrain) and [`ground_color_arena`] (frost tint).
fn arena_snow_class(wx: f32, wz: f32) -> Option<i32> {
    arena_cone_class(wx, wz, &ARENA_SNOW, ARENA_SNOW_R, 3.0)
}

/// Shared terraced-cone height for the arena's raised regions: a linear cone from `ARENA_FLAT_CLASS`
/// at the rim to `ARENA_FLAT_CLASS + peak` at the centre, rounded to whole classes. The rounding
/// step is always ≤1 class per tile at these radii, so every cone stays walkable (no nav blockers,
/// no cliff) — the terrain mesh's corner smoothing then renders it as a continuous slope.
fn arena_cone_class(wx: f32, wz: f32, centres: &[Vec2], radius: f32, peak: f32) -> Option<i32> {
    let p = Vec2::new(wx, wz);
    let mut best: Option<f32> = None;
    for c in centres {
        let d = (p - *c).length();
        if d < radius {
            let t = 1.0 - d / radius;
            best = Some(best.map_or(t, |b: f32| b.max(t)));
        }
    }
    best.map(|t| ARENA_FLAT_CLASS + (t * peak * arena_shore_damp(wx, wz)).round() as i32)
}

/// Fade any RAISED arena region flat as it nears the foreshore (`1` inland → `0` at the sand).
/// Without it a massif whose rim reaches the beach ring — which the coast fray can pull up to
/// [`ARENA_COAST_FRAY`] units inland at a cove — would present a 2-3 class wall to the class-1
/// sand: a cliff at the waterline, exactly the thing this pass is removing. The fade is gentle
/// enough (≤0.4 class/unit) to keep every cone's ≤1-step-per-tile walkable slope intact.
fn arena_shore_damp(wx: f32, wz: f32) -> f32 {
    // The earliest radius the sand can start once the coast noise swings fully inward.
    let inner = ARENA_LAND_R - ARENA_BEACH_W - ARENA_COAST_FRAY;
    1.0 - smoothstep(inner - 8.0, inner, wx.hypot(wz))
}

/// `t ∈ [0,1]` falloff of the strongest arena region containing world `(wx,wz)` — 0 at the rim,
/// 1 at a centre; `0.0` outside every one. The tint driver for the flat (unraised) regions and the
/// colour companion to [`arena_cone_class`].
fn arena_region_t(wx: f32, wz: f32, centres: &[Vec2], radius: f32) -> f32 {
    let p = Vec2::new(wx, wz);
    let mut t = 0.0f32;
    for c in centres {
        let d = (p - *c).length();
        if d < radius {
            t = t.max(1.0 - d / radius);
        }
    }
    t
}

/// Organic in/out swing (world units) of the arena coastline at base `(x,z)` — added to
/// [`ARENA_LAND_R`] to give the island capes and coves. Hash value-noise octaves, deliberately NOT
/// `noise_a`: that's an axis-separable sine product, so using it here frayed the coast into a
/// REGULAR standing-wave scallop (the same lattice artefact the ground colour hit). Both
/// [`classify_arena`]'s land test and [`arena_sea_field`] route through this one function, so the
/// per-tile shoreline and the smooth marching-squares shore contour can never disagree.
fn arena_coast_offset(x: f32, z: f32) -> f32 {
    let n = 0.60 * vnoise(x * 0.085 + 5.3, z * 0.085 - 1.9)
        + 0.28 * vnoise(x * 0.19 - 2.1, z * 0.19 + 3.7)
        + 0.12 * vnoise(x * 0.44 + 1.1, z * 0.44 - 4.4);
    (n * 2.0 - 1.0) * ARENA_COAST_FRAY
}

/// Smooth signed field for the arena's SEA, in rough base-tile units (`>0` inland, `<0` offshore,
/// `~0` on the waterline) — the arena's answer to [`sea_field`], which only knows the campaign
/// island's ellipse. The mesh marching-squares this to cut a smooth sub-tile shoreline instead of
/// snapping the coast to the tile grid (which is what stepped the arena's edge into a sawtooth).
fn arena_sea_field(x: f32, z: f32) -> f32 {
    let wx = x * MAP_SCALE - GX;
    let wz = z * MAP_SCALE - GZ;
    (ARENA_LAND_R + arena_coast_offset(x, z) - wx.hypot(wz)) / MAP_SCALE
}

/// Signed distance (base-tile units, `<0` = water) to the nearest arena LAKE. Mirrors the campaign
/// [`lake_sd`]: a clean circle with no fray, because marching-squares draws the exact `= 0` contour
/// smooth — the same shape [`classify_arena`] carves per-tile, just without the staircase.
fn arena_lake_sd(x: f32, z: f32) -> f32 {
    let wx = x * MAP_SCALE - GX;
    let wz = z * MAP_SCALE - GZ;
    let p = Vec2::new(wx, wz);
    ARENA_LAKES
        .iter()
        .map(|c| ((p - *c).length() - ARENA_LAKE_R) / MAP_SCALE)
        .fold(f32::INFINITY, f32::min)
}

/// The arena's mirrored deposit sites, in world XZ. Consumed by the wave-2 `rts` deposit module,
/// which spawns the real tree groves (wood), stone outcrops and gold veins on these spots — the
/// generator keeps each one flat + free of decorative trees (see [`classify_arena`]).
pub struct ArenaSites {
    /// Wood-grove sites `[player, rival, contested, player2, rival2]`.
    pub wood: [Vec2; 5],
    /// Stone-outcrop sites `[player, rival, contested, player2, rival2]`.
    pub stone: [Vec2; 5],
    /// Gold-vein sites `[player, rival, contested]` — deliberately the scarcest kind.
    pub gold: [Vec2; 3],
    /// Rich iron-ore sites (mechanically Stone) — one at each mountain foot `[player, rival]`.
    pub iron: [Vec2; 2],
    /// Dense forest-grove wood sites `[player, rival]`.
    pub forest: [Vec2; 2],
}

/// The arena deposit layout (world XZ). Per side one wood/stone/gold just outside the base plateau,
/// mirrored through the origin, plus a richer contested trio near the centre off the road, a rich
/// iron-ore vein at each mountain foot, and a dense forest grove on each outer flank.
pub fn arena_sites() -> ArenaSites {
    ArenaSites {
        wood: ARENA_WOOD,
        stone: ARENA_STONE,
        gold: ARENA_GOLD,
        iron: ARENA_MOUNTS,
        forest: ARENA_FOREST,
    }
}

/// Distance (world units) from `(wx, wz)` to the base-to-base dirt-road segment (player → origin →
/// rival; all three are collinear on `x + z = 0`, so it's one straight segment).
fn arena_road_dist(wx: f32, wz: f32) -> f32 {
    let (a, b) = (crate::rts::PLAYER_BASE, crate::rts::RIVAL_BASE);
    let ab = b - a;
    let p = Vec2::new(wx, wz);
    let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Road strength `[0,1]` at world `(wx, wz)` — a packed core fading to 0 at the half-width, the
/// same profile `roads::RoadField::stamp` bakes for a campaign artery. Drives the ground tint.
fn arena_road_strength(wx: f32, wz: f32) -> f32 {
    let d = arena_road_dist(wx, wz);
    if d > ARENA_ROAD_HALF {
        0.0
    } else if d <= ARENA_ROAD_CORE {
        1.0
    } else {
        1.0 - (d - ARENA_ROAD_CORE) / (ARENA_ROAD_HALF - ARENA_ROAD_CORE)
    }
}

/// Distance (world units) from `(wx, wz)` to the nearest of the nine deposit spots.
fn arena_deposit_dist(wx: f32, wz: f32) -> f32 {
    let p = Vec2::new(wx, wz);
    ARENA_WOOD
        .iter()
        .chain(ARENA_STONE.iter())
        .chain(ARENA_GOLD.iter())
        .chain(ARENA_MOUNTS.iter())
        .chain(ARENA_FOREST.iter())
        .map(|s| p.distance(*s))
        .fold(f32::INFINITY, f32::min)
}

/// Arena terrain classification (base-space in, like every `classify`; converts to world and
/// reasons there). Small grass island, two flat base plateaus, a flat road corridor, flat deposit
/// discs, a sandy beach ring, decorative light-forest on the outer fringe, gentle rolling grass
/// everywhere else. `None` beyond the coast (ocean). Trees are excluded from the bases / road /
/// deposits **at the source** — those tiles are never classified `Forest`, so the biome scatter
/// (which only plants on `Forest` tiles) can't touch them, no scatter-closure edits needed.
fn classify_arena(x: f32, z: f32) -> Option<(TB, i32)> {
    let wx = x * MAP_SCALE - GX;
    let wz = z * MAP_SCALE - GZ;
    let p = Vec2::new(wx, wz);
    let r = p.length();
    // Island disc with an organically-frayed coast; ocean beyond. The SAME expression backs
    // `arena_sea_field`, which the mesh marching-squares into a smooth sub-tile shoreline.
    let coast = arena_coast_offset(x, z);
    if r > ARENA_LAND_R + coast {
        return None;
    }
    // Force-flat base plateaus (player SW, rival NE) — level building ground.
    if (p - crate::rts::PLAYER_BASE).length() < ARENA_BASE_R
        || (p - crate::rts::RIVAL_BASE).length() < ARENA_BASE_R
    {
        return Some((TB::Grass, ARENA_FLAT_CLASS));
    }
    // Force-flat road corridor between the bases.
    if arena_road_dist(wx, wz) < ARENA_ROAD_FLAT {
        return Some((TB::Grass, ARENA_FLAT_CLASS));
    }
    // Force-flat deposit discs (level + clear for the wave-2 props).
    let dep_d = arena_deposit_dist(wx, wz);
    if dep_d < ARENA_DEPOSIT_FLAT {
        return Some((TB::Grass, ARENA_FLAT_CLASS));
    }
    // Iron-ore mountains: broad rocky massifs ringing each flattened ore bowl (the bowl centre was
    // returned Grass just above — the mountain centres are deposit spots). Raised `TB::Rock` on a
    // gentle terraced slope: walkable like the hills, just wider and (via the tint) darker.
    if let Some(mc) = arena_mount_class(wx, wz) {
        return Some((TB::Rock, mc));
    }
    // Snow highlands on the two fair-bisector lobes — a raised, terraced cold massif.
    if let Some(sc) = arena_snow_class(wx, wz) {
        return Some((TB::Snow, sc));
    }
    // Ornamental lakes (mirrored pair): carve a hole so the sea plane reads through as shallow
    // water. After the base/road/deposit returns above, so a lake can never eat a build plot or
    // the lane. `arena_lake_sd` marching-squares this same circle into a smooth shore.
    if arena_lake_sd(x, z) < 0.0 {
        return None;
    }
    // Sandy FORESHORE ring at the waterline, a full class BELOW the inland field
    // (`ARENA_FLAT_CLASS` = 2) — the island ramps down to the sea instead of ending in a wall.
    if r > ARENA_LAND_R - ARENA_BEACH_W + coast {
        return Some((TB::Sand, 1));
    }
    // Dune scrub: dry sandy flats at field height (no rise) — a warm biome on the outer field.
    if arena_region_t(wx, wz, &ARENA_DUNES, ARENA_DUNE_R) > 0.0 {
        return Some((TB::Desert, ARENA_FLAT_CLASS));
    }
    // Gentle rolling grass: mostly flat, an occasional one-class knoll (kept ≤1 step, walkable;
    // `terrace_inland` enforces it globally anyway).
    let h = if noise_b(x + 11.0, z - 7.0) > 0.95 { ARENA_FLAT_CLASS + 1 } else { ARENA_FLAT_CLASS };
    // Decorative light-forest clumps on the OUTER field only (open centre for the battle), kept
    // clear of the bases and deposit spots. The road / base / deposit force-flat checks above have
    // already returned, so a Forest tile can never land on them.
    let base_clear = (p - crate::rts::PLAYER_BASE).length().min((p - crate::rts::RIVAL_BASE).length());
    // Off the bases, the deposit spots and the base-to-base lane — the gate every decorative
    // region shares, so nothing can crowd a build plot or wall off the road.
    let off_lane = base_clear > ARENA_BASE_CLEAR
        && dep_d > ARENA_DEPOSIT_CLEAR
        && arena_road_dist(wx, wz) > ARENA_ROAD_CLEAR;
    // Rocky hills on the free flanks — terraced (walkable, no blockers). Take priority over the
    // tree fringe so a hill reads as rock, not woods.
    if off_lane {
        if let Some(hc) = arena_hill_class(wx, wz) {
            return Some((TB::Rock, hc));
        }
    }
    // Forest PATCHES — a dense stand, near-solid `Forest` tiles (vs the fringe's sparse clumps
    // below), so each side's wood really reads as a wood. Density falls off toward the rim, so the
    // stand fades into the meadow instead of ending on a circle.
    if off_lane {
        let ft = arena_region_t(wx, wz, &ARENA_FOREST, ARENA_FOREST_R);
        if ft > 0.0 && ft * 2.6 + noise_a(x * 0.7 + 13.0, z * 0.7 - 5.0) * 0.5 > 1.0 {
            return Some((TB::Forest, h));
        }
    }
    let forested = r > ARENA_FOREST_R0
        && r < ARENA_LAND_R - ARENA_BEACH_W - 2.0
        && off_lane
        && noise_a(x * 0.6 + 40.0, z * 0.6 - 20.0) * noise_b(x * 0.6 + 7.0, z * 0.6 - 3.0) > 0.55;
    if forested {
        return Some((TB::Forest, h));
    }
    Some((TB::Grass, h))
}

/// Arena ground colour (base-space in, like [`ground_color`]): the home meadow macro-patches, a
/// sandy coast fade, the universal mottle and the dirt road — painted with the SAME `road_dirt`
/// palette + core/verge styling the campaign road uses, but from the arena's own straight
/// base-to-base road (the campaign `roads` network is not built in skirmish). No biome blends, no
/// bog wetness.
fn ground_color_arena(x: f32, z: f32) -> [f32; 4] {
    let p = &active_map().palette;
    let wx = x * MAP_SCALE - GX;
    let wz = z * MAP_SCALE - GZ;
    let mut col = lin3(p.grass);
    // Meadow macro-patches — MUTED vs the campaign's open grass (arena legibility: a calmer, more
    // uniform field lets the units + team rings read against it instead of blending into busy mottle).
    let p1 = omottle(x, z, 4.0, 31.0);
    let p2 = omottle(x, z, 9.0, 53.0);
    let p3 = omottle(x, z, 1.6, 71.0);
    col = mix3(col, lin3(p.grass_dark), smoothstep(0.1, 1.3, p1) * 0.28);
    col = mix3(col, lin3(p.grass_dry), smoothstep(0.4, 1.5, p2) * 0.09);
    col = mix3(col, lin3(p.grass_gold), smoothstep(0.85, 1.6, p3) * 0.05);
    // Sandy fade toward the arena coast (its own island shape — the shared `dist_from_coast` only
    // knows the home ellipse, so it can't drive this).
    let r = wx.hypot(wz);
    let sand_w = smoothstep(ARENA_LAND_R - ARENA_BEACH_W - 2.0, ARENA_LAND_R, r);
    col = mix3(col, lin3(p.sand), sand_w * 0.9);
    // Universal mottle — softened (see the muted patches above; keep the ground quiet).
    let m1 = omottle(x, z, 2.2, 17.0);
    let m2 = omottle(x, z, 14.0, 91.0);
    let v = 1.0 + 0.05 * m1 + 0.03 * m2;
    let warm = 0.03 * omottle(x, z, 1.4, 23.0);
    col = [
        (col[0] * v * (1.0 + warm)).clamp(0.0, 1.0),
        (col[1] * v).clamp(0.0, 1.0),
        (col[2] * v * (1.0 - warm)).clamp(0.0, 1.0),
    ];
    // Rock-grey rise on the hill tiles (blend grows toward each hill's peak so the slope reads
    // rocky). Matches the raised `TB::Rock` tiles from `classify_arena`.
    if let Some(hc) = arena_hill_class(wx, wz) {
        let t = ((hc - ARENA_FLAT_CLASS) as f32 / 2.0).clamp(0.25, 1.0);
        col = mix3(col, [0.30, 0.29, 0.27], t * 0.72);
    }
    // Iron-ore mountains: a DARK grey rockscape with a rust-red ore stain, radiating from each
    // centre (so even the flattened ore bowl reads rocky, not meadow). Darker + redder than the
    // plain hills so the mountain biome reads as its own place.
    {
        let mt = arena_region_t(wx, wz, &ARENA_MOUNTS, ARENA_MOUNT_R);
        if mt > 0.0 {
            // Ramp up fast so the whole massif footprint (not just its core) reads as a grey
            // rockscape against the meadow.
            col = mix3(col, [0.24, 0.23, 0.21], smoothstep(0.05, 0.55, mt) * 0.82);
            col = mix3(col, [0.35, 0.16, 0.10], smoothstep(0.45, 1.0, mt) * 0.42); // rust ore stain toward the core
        }
    }
    // Snow highlands: a cold massif — pale blue-grey scree at the rim climbing to a bright frost
    // cap at the peak, so the lobe reads as its own (cold) place from across the map.
    {
        let st = arena_region_t(wx, wz, &ARENA_SNOW, ARENA_SNOW_R);
        if st > 0.0 {
            col = mix3(col, [0.42, 0.46, 0.52], smoothstep(0.04, 0.42, st) * 0.80); // frosted scree
            col = mix3(col, lin3(p.snow), smoothstep(0.30, 0.90, st) * 0.92); // the snow cap
        }
    }
    // Dune scrub: dry sand-gold flats fading into the meadow at the rim.
    {
        let dt = arena_region_t(wx, wz, &ARENA_DUNES, ARENA_DUNE_R);
        if dt > 0.0 {
            col = mix3(col, lin3(p.grass_dry), smoothstep(0.02, 0.35, dt) * 0.70);
            col = mix3(col, lin3(p.sand), smoothstep(0.20, 0.85, dt) * 0.78);
        }
    }
    // Forest patches: a greener, shadier floor so the stand reads as woodland, not open meadow.
    {
        let ft = arena_region_t(wx, wz, &ARENA_FOREST, ARENA_FOREST_R);
        if ft > 0.0 {
            col = mix3(col, lin3(p.grass_dark), smoothstep(0.05, 0.6, ft) * 0.55);
            col = mix3(col, [0.09, 0.19, 0.08], smoothstep(0.1, 0.8, ft) * 0.34); // deep, shady forest floor
        }
    }
    // Worn dirt road, baked into the ground (same core/verge styling as `ground_color`).
    let road_s = arena_road_strength(wx, wz);
    if road_s > 0.0 {
        let core_w = smoothstep(0.30, 0.55, road_s);
        let verge_w = smoothstep(0.02, 0.14, road_s) * (1.0 - smoothstep(0.18, 0.34, road_s));
        col = mix3(col, lin3(p.road_dirt), (verge_w * 0.28 + core_w * 0.85).min(1.0));
    }
    [col[0], col[1], col[2], 0.0]
}

fn classify(x: f32, z: f32) -> Option<(TB, i32)> {
    if is_arena() {
        return classify_arena(x, z);
    }
    // The Blight: the walkable ork-fortress landmass south of the old coast (shape + heights
    // live in `ork_fortress.rs`). Checked BEFORE the island shape — it deliberately extends
    // past the coast — and it overrides the south-swamp fringe so the trampled mud runs
    // continuous from the marsh into Gnashfang Hold.
    if let Some(h) = crate::ork_fortress::blight_class_base(x, z) {
        return Some((TB::Blight, h));
    }
    if !is_land_shape(x, z) {
        return None;
    }
    // Town build plots must stay flat, empty grass — a terrain step or biome props under a
    // plot would collide with the building constructed on it. Plots are authored in world
    // space; this runs in base space, so convert (world = base·MAP_SCALE − G).
    if crate::town::near_build_plot(x * MAP_SCALE - GX, z * MAP_SCALE - GZ) {
        return Some((TB::Grass, 1));
    }
    // The rival stronghold gets its own forced-flat DESERT plateau (its safe-zone), so its keep,
    // walls and the skirmish ground around it sit level instead of straddling the dune terraces.
    if crate::rival::fort_flat_zone(x * MAP_SCALE - GX, z * MAP_SCALE - GZ) {
        return Some((TB::Desert, 1));
    }
    let dc = dist_from_castle(x, z);
    if dc < SAFE_R + edge_fray(x, z).max(-4.0) {
        return Some((TB::Grass, 1));
    }
    if is_river(x, z) {
        return None;
    }
    if is_lake(x, z) {
        return None;
    }
    // Still-water carve: the swamp bog pools + the waterfall plunge stream. (Early, like the
    // lake — the stream crosses the ROCK region's apron, which the swamp branch never sees.)
    if is_pool(x, z) {
        return None;
    }
    let d = dist_from_coast(x, z) as f32;
    let beach_w =
        1.0 + (1.0 + (x * 0.6 + z * 0.42 + 2.1).sin() * 0.8 + (x * 1.25 - z * 0.95 + 0.4).sin() * 0.6).max(0.0);
    if d <= beach_w {
        return Some((TB::Sand, 1));
    }
    // Coastal mountain ridges around the island rim (skip inside the peak regions — the snow
    // massif / rock range own their heights there). Tall ridges read as bare rock; lower
    // rises keep the local flat-region biome (sandy bluffs in the desert, wooded coastal
    // hills in the forest, …) or grass.
    if !in_mountain(x, z) {
        let ch = coast_hill_class(x, z);
        if ch > 0 {
            let b = match region_at(x, z) {
                Some(ri) if active_map().regions[ri].peak == 0 => active_map().regions[ri].biome,
                _ if ch >= 4 => TB::Rock,
                _ => TB::Grass,
            };
            return Some((b, ch));
        }
    }
    // Plateaus skip cliffy mesa regions (a smooth plateau cone would breach the tier walls).
    let ph = if in_cliffy(x, z) { 0 } else { plateau_height(x, z) };
    if ph > 0 {
        // A plateau inside a flat biome blob keeps that biome (desert mesa, forest highland);
        // out on the frontier it's a grass plateau.
        let b = match region_at(x, z) {
            Some(ri) if active_map().regions[ri].peak == 0 => active_map().regions[ri].biome,
            _ => TB::Grass,
        };
        return Some((b, ph));
    }
    // Pass corridors claim their tiles INDEPENDENT of the wobbled region membership:
    // `region_at`'s wobble (±4.8) otherwise punches frontier-grass bays into a corridor's lower
    // ramp — 3–4-class holes in the only walkable ascent up a mesa.
    for reg in active_map().regions {
        if reg.cliffy && reg.peak > 0 {
            if let Some(cls) = pass_class(x, z, reg) {
                return Some((reg.biome, cls));
            }
        }
    }
    if let Some(ri) = region_at(x, z) {
        let reg = &active_map().regions[ri];
        if reg.biome == TB::Swamp && dc < SAFE_R {
            return Some((TB::Grass, 1));
        }
        // (Marsh pools removed: a small carved pool is tiny, so the water shader's shore-distance
        // never reaches "deep" over it — the whole pool renders as the white foam collar, reading as
        // an ugly pale "plate" on the marsh, worse than the flat-disc version it replaced. The swamp
        // reads wet from the algae ground tint + reeds instead; standing water stays for the rivers
        // and the deliberate lake, which are big enough to render as real water.)
        if reg.peak > 0 {
            let h = if reg.cliffy { mesa_height(x, z, reg) } else { mountain_height(x, z, reg) };
            return Some((reg.biome, h));
        }
        // Swamp (map-character overhaul pass 3): a coherent BOG — standing murky water in the
        // authored pool blobs, mud flats (class 1) ringing every shore, dry hummock rises
        // (class 2) only away from the water. Heights key on the SAME `pool_sd` the carve
        // uses, so shores, mud and rises read as one geography.
        if reg.biome == TB::Swamp {
            // (Water itself is carved by the early still-water check above.)
            let sd = pool_sd(x, z);
            let fine = noise_b(x * 0.5 + 1.3, z * 0.5);
            let h = if sd > 2.5 && fine > 0.7 { 2 } else { 1 };
            return Some((TB::Swamp, h));
        }
        // Flat biomes get the inland-hills field: dunes in the desert, wooded rises in the forest.
        // Next to a mesa rim the hills flatten to a contrast APRON — tall reads tall beside flat.
        let max = if near_cliffy_rim(x, z, 8.0) { 1 } else { 4 };
        return Some((reg.biome, inland_hills(x, z, dc, max)));
    }
    let h = if near_cliffy_rim(x, z, 8.0) { 1 } else { inland_hills(x, z, dc, 5) };
    let forest_n = noise_a(x, z) * noise_b(x + 7.0, z - 3.0);
    if forest_n > 0.35 {
        return Some((TB::Forest, h));
    }
    Some((TB::Grass, h))
}

// ── Tile cache (per map) ──────────────────────────────────────────────────────────
type Grid = Vec<Option<(TB, i32)>>;
/// One memoised grid per [`MapId`] that's been generated this process. Keyed by the `u8` id, so
/// switching maps (a New Game on the other world) regenerates once and then returns instantly on
/// switch-back. The one-time regen is fully covered by the loading veil. `tiles()` hands out a
/// cheap `Arc` clone; every reader goes through `tile_at`, so the swap is invisible to them.
static TILES: OnceLock<Mutex<HashMap<u8, Arc<Grid>>>> = OnceLock::new();
fn tiles() -> Arc<Grid> {
    let id = active_id();
    let cache = TILES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().expect("tile cache poisoned");
    guard.entry(id).or_insert_with(build_grid).clone()
}
/// Classify the whole grid for the **active** map, then relax inland cliffs. Sampling runs in
/// BASE space so the island silhouette is unchanged; `active_map()` (via `classify`/the noise
/// phase/the regions) makes it the right world.
fn build_grid() -> Arc<Grid> {
    let mut v = Vec::with_capacity((COLS * ROWS) as usize);
    for iz in 0..ROWS {
        for ix in 0..COLS {
            v.push(classify(ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE));
        }
    }
    prune_stray_islets(&mut v);
    terrace_inland(&mut v);
    Arc::new(v)
}

/// Sink any small land clump the `nw_headland` coast noise pinched off just offshore — the
/// detached, player-unreachable islets that hugged our NW shore. Flood-fills the MAIN landmass
/// (8-connected) from the castle tile; any land NOT attached to it that lies in the NW headland
/// zone is set back to sea, so the snow coast reads as one clean shoreline with no broken-off
/// nubs. Scoped to `nw_headland > 0` so nothing elsewhere (the island proper, the southern
/// Blight, deliberate features) is ever touched — and those landmasses are one connected
/// component with the castle anyway, so the flood-fill keeps them regardless.
fn prune_stray_islets(v: &mut [Option<(TB, i32)>]) {
    let n = (COLS * ROWS) as usize;
    let mut reached = vec![false; n];
    let seed_ix = (CX * MAP_SCALE) as i32;
    let seed_iz = (CZ * MAP_SCALE) as i32;
    let seed = (seed_iz * COLS + seed_ix) as usize;
    if v.get(seed).map(|t| t.is_some()).unwrap_or(false) {
        let mut stack = vec![seed];
        reached[seed] = true;
        while let Some(idx) = stack.pop() {
            let ix = (idx as i32) % COLS;
            let iz = (idx as i32) / COLS;
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }
                    let (nx, nz) = (ix + dx, iz + dz);
                    if nx < 0 || nz < 0 || nx >= COLS || nz >= ROWS {
                        continue;
                    }
                    let ni = (nz * COLS + nx) as usize;
                    if !reached[ni] && v[ni].is_some() {
                        reached[ni] = true;
                        stack.push(ni);
                    }
                }
            }
        }
    }
    for iz in 0..ROWS {
        for ix in 0..COLS {
            let idx = (iz * COLS + ix) as usize;
            if v[idx].is_some() && !reached[idx] {
                let (bx, bz) = (ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE);
                if nw_headland(bx, bz) > 0.0 {
                    v[idx] = None; // detached NW islet → back to sea
                }
            }
        }
    }
}

/// Post-process the heightfield so every **inland** land tile sits at most ONE height class
/// above its lowest land neighbour. NPC movement — the nav-grid (`navgrid::can_step`) and the
/// local steering both ambient wildlife and camp orks use (`steer::can_stand`) — refuses any
/// step across a >1-class face, so a 2+-class cliff between two otherwise-walkable tiles wedges
/// a wandering NPC at its base (it pivots in place with nowhere to go). The per-biome height
/// generators already AIM for ≤1-class terraces, but `mountain_height`/`inland_hills` noise still
/// stamps the occasional sheer pocket; this relaxation enforces the invariant globally, so hills,
/// dunes, plateaus and the mountain bulk are fully climbable instead of penning creatures in.
///
/// The OUTER coastal ridge band (`dist_from_coast ≤ 7`, where `coast_hill_class` lives) is left
/// alone on purpose: those seaward faces are DELIBERATELY sheer (ridge tops reachable from the
/// inland ramp only — see `coast_hill_class`), and terracing them down to the beach would melt
/// the island's coastal silhouette. Relaxation only lowers, never raises, so the forced-flat
/// castle safe-zone / town plots (class 1) and every approach lane stay open.
fn terrace_inland(v: &mut [Option<(TB, i32)>]) {
    // Precompute the coastal-band mask once (dist_from_coast is an 8-ray probe — too costly to
    // recompute per relaxation pass). Cliffy mesa regions are exempt too: their multi-class
    // shelf walls are DELIBERATE (map-character overhaul pass 1) — the authored pass corridors
    // are generated as smooth ≤1-class ramps, so climbability inside a mesa is by construction,
    // not by relaxation.
    let band: Vec<bool> = (0..(COLS * ROWS) as usize)
        .map(|i| {
            let ix = (i as i32) % COLS;
            let iz = (i as i32) / COLS;
            let (bx, bz) = (ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE);
            dist_from_coast(bx, bz) <= 7 || cliff_exempt_base(bx, bz)
        })
        .collect();
    // Iterate to a fixpoint: each pass terraces an offending tile down one class toward its
    // lowest neighbour; a tall sheer pocket converges in ≤peak passes (≤~10).
    loop {
        let mut changed = false;
        for iz in 0..ROWS {
            for ix in 0..COLS {
                let idx = (iz * COLS + ix) as usize;
                let Some((tb, h)) = v[idx] else { continue };
                if h <= 1 || band[idx] {
                    continue;
                }
                let mut min_n = i32::MAX;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, nz) = (ix + dx, iz + dz);
                    if nx < 0 || nz < 0 || nx >= COLS || nz >= ROWS {
                        continue;
                    }
                    if let Some((_, nh)) = v[(nz * COLS + nx) as usize] {
                        min_n = min_n.min(nh);
                    }
                }
                if min_n != i32::MAX && h > min_n + 1 {
                    v[idx] = Some((tb, min_n + 1));
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}
fn tile_at(ix: i32, iz: i32) -> Option<(TB, i32)> {
    if ix < 0 || iz < 0 || ix >= COLS || iz >= ROWS {
        return None;
    }
    tiles()[(iz * COLS + ix) as usize]
}

// World ↔ tile helpers (world is the enlarged tile-space recentred on the origin).
pub fn tile_biome_world(wx: f32, wz: f32) -> Option<Biome> {
    let t = tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32)?;
    match t.0 {
        TB::Forest => Some(Biome::Forest),
        TB::Snow => Some(Biome::Snow),
        TB::Rock => Some(Biome::Rocky),
        TB::Desert => Some(Biome::Desert),
        // The Blight ARE swamp to gameplay: poison + slow (`player::movement`), swamp
        // ambience/weather, swamp wildlife/forage. Only the ground + scatter differ.
        TB::Swamp | TB::Blight => Some(Biome::Swamp),
        _ => None,
    }
}
pub fn is_grass_world(wx: f32, wz: f32) -> bool {
    matches!(tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32).map(|t| t.0), Some(TB::Grass))
}
/// Smoothed terrain height at integer grid CORNER `(cx, cz)` **as seen from a tile of class
/// `h_ref`**: the mean flat-top Y of the corner's land tiles that are CLUSTER-CONNECTED to
/// `h_ref` — chained by ≤1-class links. On terraced ground every corner tile chains together,
/// so this is exactly the old whole-corner mean (continuous slopes, no Minecraft steps). But
/// across a mesa tier wall (≥2-class jump, cliffy regions only — `terrace_inland` forbids it
/// elsewhere) the shelves split into separate clusters: each keeps its own crisp corner height,
/// the surface becomes DISCONTINUOUS across the wall edge, and `build_terrain_chunk` spans the
/// gap with a vertical cliff quad. Without this split the corner averaging melted every
/// authored mesa cliff into a smooth slope (pass-1 verification FAIL).
/// `None` when no land tile at the corner chains to `h_ref` (callers fall back to the tile's
/// own flat top).
fn corner_top_y_for(cx: i32, cz: i32, h_ref: i32) -> Option<f32> {
    let mut hs: [Option<i32>; 4] = [None; 4];
    for (k, (ax, az)) in [(cx - 1, cz - 1), (cx, cz - 1), (cx - 1, cz), (cx, cz)].into_iter().enumerate() {
        hs[k] = tile_at(ax, az).map(|(_, h)| h);
    }
    // Seed with tiles ≤1 class from the reference, then chain outward (≤4 tiles → 3 passes).
    let mut inc = [false; 4];
    for k in 0..4 {
        inc[k] = matches!(hs[k], Some(h) if (h - h_ref).abs() <= 1);
    }
    for _ in 0..3 {
        for k in 0..4 {
            if inc[k] {
                continue;
            }
            let Some(h) = hs[k] else { continue };
            if (0..4).any(|j| inc[j] && matches!(hs[j], Some(hj) if (h - hj).abs() <= 1)) {
                inc[k] = true;
            }
        }
    }
    let mut sum = 0.0_f32;
    let mut n = 0u32;
    for k in 0..4 {
        if inc[k] {
            sum += (hs[k].unwrap() - 1) as f32 * GROUND_STEP;
            n += 1;
        }
    }
    (n > 0).then(|| sum / n as f32)
}

/// Water field at a grid CORNER `(cx, cz)` (grid index → base XZ is `c / MAP_SCALE`) for the bank
/// marching-squares: `(field, is_river)`. `field < 0` = water, `> 0` = land, `0` = shoreline.
///  - off the island → SEA: a moderate negative (so a river-mouth cut follows the real land/water
///    edge and doesn't paint ocean as phantom land), `is_river = false`;
///  - safe-zone / mountains / Blight → land (`+1`), no river;
///  - else the river signed distance, `is_river` when it's under the channel.
/// The mesh only marching-squares a cell that has a RIVER corner, so a plain coast keeps its
/// existing per-tile beach; at a river MOUTH the sea corners still read as water, so the cut is
/// clean there.
fn corner_water(cx: i32, cz: i32) -> (f32, bool) {
    let bx = cx as f32 / MAP_SCALE;
    // The arena is a different island on a different set of water: its own coast + its own lakes.
    // (Every field below is authored in home-island coords, so none of it applies.) Same shape as
    // the campaign branch — sea reads a moderate negative, otherwise the still-water SDF — so the
    // mesh's marching-squares cut treats the arena's shore and lakeshores exactly like the
    // campaign's, i.e. as a smooth sub-tile curve instead of a tile-grid staircase.
    if is_arena() {
        let bz = cz as f32 / MAP_SCALE;
        if arena_sea_field(bx, bz) < 0.0 {
            return (-0.6, false); // ocean
        }
        let sd = arena_lake_sd(bx, bz);
        return (sd, sd < 0.0);
    }
    let bz = cz as f32 / MAP_SCALE;
    // `is_land_shape` is the OLD island ellipse, which does NOT include the Blight (it extends
    // south past the old coast). Without the Blight exception every fortress-interior corner read
    // as sea → `wetn == 4` → `build_terrain_chunk` skipped the cell, leaving the courtyard a
    // textureless void. The Blight is real land, so treat it as such here.
    if !is_land_shape(bx, bz) && crate::ork_fortress::blight_class_base(bx, bz).is_none() {
        return (-0.6, false); // sea
    }
    // Combine the water sources into ONE corner field (nearest water wins) so marching-squares cuts
    // a smooth shore for the lake AND the swamp bog pools too, not just rivers. The lake/pools
    // aren't gated by `river_blocked` (that's a river-only keep-out); they carry their own gating.
    let river = if river_blocked(bx, bz) { f32::INFINITY } else { river_sd(bx, bz) };
    let sd = river.min(lake_sd(bx, bz)).min(pool_or_stream_sd(bx, bz));
    (sd, sd < 0.0)
}

/// Smoothed surface Y at world `(wx, wz)` — bilinear blend of the containing tile's four smoothed
/// corner heights. `None` over water / off the island (the containing tile is water). Every LAND
/// tile contributes to all four of its OWN corners, so the four `corner_top_y` are always `Some`
/// inside a land tile (the `unwrap_or` is just a belt-and-braces fallback). The whole game samples
/// ground height through this (hero/NPC/scatter follow), so the visible mesh and everything resting
/// on it stay in lockstep.
fn smooth_surface_y(wx: f32, wz: f32) -> Option<f32> {
    let gx = wx + GX;
    let gz = wz + GZ;
    // No footing over the real (sub-tile) river/pool surface. The per-tile `tile_at` classifies
    // by tile CENTRE, but the bank is rendered sub-tile via marching-squares — so a boundary
    // tile is land by centre yet half water on screen. Reject the exact water area here (cheap:
    // `river_sd` early-outs outside the rivers' bbox; `pool_sd` outside the swamp interiors) so
    // the hero/NPCs/scatter can't stand on the rendered-water half.
    if is_river(gx / MAP_SCALE, gz / MAP_SCALE) || is_pool(gx / MAP_SCALE, gz / MAP_SCALE) {
        return None;
    }
    let ix = gx.floor() as i32;
    let iz = gz.floor() as i32;
    let (_, h) = tile_at(ix, iz)?;
    let flat = (h - 1) as f32 * GROUND_STEP;
    let (fx, fz) = (gx - ix as f32, gz - iz as f32);
    let c00 = corner_top_y_for(ix, iz, h).unwrap_or(flat);
    let c10 = corner_top_y_for(ix + 1, iz, h).unwrap_or(flat);
    let c01 = corner_top_y_for(ix, iz + 1, h).unwrap_or(flat);
    let c11 = corner_top_y_for(ix + 1, iz + 1, h).unwrap_or(flat);
    let a = c00 + (c10 - c00) * fx;
    let b = c01 + (c11 - c01) * fx;
    Some(a + (b - a) * fz)
}

fn tile_top_y_world(wx: f32, wz: f32) -> f32 {
    smooth_surface_y(wx, wz).unwrap_or(0.0)
}

// ── Public sampling API (wildlife placement + ground-following) ──────────────────
/// Terrain top Y at world `(x, z)`; `None` over water / off the island. Wildlife uses
/// this to sit creatures flush on the ground and to reject water/off-map wander steps.
pub fn ground_at_world(wx: f32, wz: f32) -> Option<f32> {
    smooth_surface_y(wx, wz)
}
/// Biome at world `(x, z)` (`None` = grass / sand / water) — wildlife biome placement.
pub fn biome_at_world(wx: f32, wz: f32) -> Option<Biome> {
    tile_biome_world(wx, wz)
}

/// Is world `(x, z)` over the carved river channel (where the sea plane shows through the
/// terrain)? The real combined-map river — `bridges` scans this to span the actual water.
///
/// Must sample in the SAME space the tiles are classified in: `tile_at` builds each tile from
/// `classify(ix / MAP_SCALE, …)`, so the river that actually carves water lives at base coord
/// `(world + G) / MAP_SCALE`. Feeding the raw `world + G` (missing the `/ MAP_SCALE`) read the
/// formula 1.4× off — bridges spanned dry land far from the real river, and the horizontal
/// river branch produced absurdly long decks.
/// The active map's river centrelines in WORLD XZ (one `Vec` per river), for tests/tools that need
/// to check coverage of the courses. Mirrors the base-space sampling used by the carve.
#[cfg(test)]
pub(crate) fn rivers_world() -> Vec<Vec<(f32, f32)>> {
    active_rivers()
        .iter()
        .map(|def| {
            let seg: Vec<f32> = def.pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).collect();
            let mut out = Vec::new();
            for (si, w) in def.pts.windows(2).enumerate() {
                let (ax, az) = w[0];
                let (bx, bz) = w[1];
                let l = seg[si].max(1e-3);
                let steps = (l / 0.6).ceil() as i32;
                for k in 0..=steps {
                    let s = k as f32 / steps as f32;
                    let cx = ax + (bx - ax) * s;
                    let cz = az + (bz - az) * s;
                    out.push((cx * MAP_SCALE - GX, cz * MAP_SCALE - GZ));
                }
            }
            out
        })
        .collect()
}

pub fn is_river_world(wx: f32, wz: f32) -> bool {
    // The rival fort force-flattens its plateau to dry desert (`classify` checks `fort_flat_zone`
    // BEFORE the river), so the carved channel does NOT exist there. Mirror that here, or the
    // bridge scanner (which reads this raw predicate, not the built terrain) lays a deck on the dry
    // ground next to the rival keep — a bridge over nothing. The castle safe-zone is already
    // excluded inside `is_river`.
    if crate::rival::fort_flat_zone(wx, wz) {
        return false;
    }
    is_river((wx + GX) / MAP_SCALE, (wz + GZ) / MAP_SCALE)
}

/// True ONLY over a real standing-water body — the deliberate **lake** or the open **sea** off the
/// island. Deliberately EXCLUDES rivers (narrow carved channels, incl. the marsh streams that thread
/// the swamp) and all land: `ground_at_world` returns `None` over rivers too, so ambient water life
/// (`fish`) that keyed off "no ground" spawned in the marsh brooks and clipped through the bank.
/// This is the predicate to gate anything that should live in open water, not any wet tile.
pub fn is_open_water_world(wx: f32, wz: f32) -> bool {
    if is_arena() {
        // The arena is a small island on the open sea: anything off its land is ocean.
        return ground_at_world(wx, wz).is_none();
    }
    // The rival fort force-flattens its plateau to dry land (see `is_river_world`); never water.
    if crate::rival::fort_flat_zone(wx, wz) {
        return false;
    }
    let bx = (wx + GX) / MAP_SCALE;
    let bz = (wz + GZ) / MAP_SCALE;
    // The inland lake.
    if is_lake(bx, bz) {
        return true;
    }
    // Open sea: off the island ellipse and not the walkable Blight fortress landmass (which extends
    // past the old coast — mirrors the sea test in `corner_water`).
    !is_land_shape(bx, bz) && crate::ork_fortress::blight_class_base(bx, bz).is_none()
}

// ── Blended ground colour ───────────────────────────────────────────────────────
fn lin3(hex: u32) -> [f32; 3] {
    let l = lin(hex);
    [l[0], l[1], l[2]]
}
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn biome_col(b: TB) -> [f32; 3] {
    let p = &active_map().palette;
    lin3(match b {
        TB::Grass => p.grass,
        TB::Sand => p.sand,
        TB::Forest => p.forest,
        TB::Rock => p.rock,
        TB::Snow => p.snow,
        TB::Desert => p.desert,
        TB::Swamp => p.swamp,
        TB::Blight => p.blight,
    })
}

/// Biome interior colour at base-space (x,z). The region blend used to mix toward one
/// flat `biome_col` per blob, which left big biome interiors (especially the snowfield)
/// a single unbroken tone — only the grass frontier had macro-patches. Snow and forest
/// now carry their own interior mottle; the other biomes keep their flat base (their
/// scatter is dense enough to break the ground up).
fn biome_col_at(b: TB, x: f32, z: f32) -> [f32; 3] {
    let base = biome_col(b);
    let p = &active_map().palette;
    match b {
        TB::Snow => {
            // Broad cool drift-shadow troughs + tighter wind-streak crests (sastrugi).
            let drift = smoothstep(0.15, 1.25, noise_a(x * 3.2 - 7.0, z * 3.2 + 19.0));
            let streak = smoothstep(0.55, 1.35, noise_b(x * 7.5 + 3.0, z * 2.4 - 13.0));
            let col = mix3(base, lin3(p.snow_shade), drift * 0.50);
            mix3(col, lin3(p.snow_bright), streak * 0.55)
        }
        TB::Forest => {
            // Dark moist loam patches under the canopy + dry leaf-litter speckle.
            let moist = smoothstep(0.20, 1.30, noise_a(x * 3.6 + 13.0, z * 3.6 - 5.0));
            let dry = smoothstep(0.50, 1.45, noise_b(x * 8.0 - 23.0, z * 8.0 + 7.0));
            let col = mix3(base, lin3(p.forest_dark), moist * 0.50);
            // Dry leaf-litter kept sparse (player: forest floor too yellow) — narrower band, lower mix.
            mix3(col, lin3(p.forest_dry), dry * 0.20)
        }
        TB::Swamp => {
            // Standing-water muck pools + mossy hummocks + a green algae seep — the marsh
            // floor should read wet and blotchy, not one flat olive (the island swamp had
            // NO interior mottle, so big stretches near the ork mire read dead-flat).
            let pool = smoothstep(0.15, 1.20, noise_a(x * 3.4 + 5.0, z * 3.4 - 11.0));
            let moss = smoothstep(0.45, 1.30, noise_b(x * 7.0 - 17.0, z * 7.0 + 9.0));
            let algae = smoothstep(0.50, 1.25, noise_a(x * 1.9 - 41.0, z * 1.9 + 23.0));
            // Fine high-freq grain: scummy dark fleck + bright damp-moss speckle, breaking
            // the mid-tones into a finer wet tooth so the muck reads textured up close.
            let fleck = smoothstep(0.55, 1.35, noise_b(x * 15.0 + 33.0, z * 15.0 - 19.0));
            let damp = smoothstep(0.62, 1.40, noise_a(x * 11.0 - 61.0, z * 11.0 + 47.0));
            let col = mix3(base, lin3(p.swamp_dark), pool * 0.58);
            let col = mix3(col, lin3(p.swamp_moss), moss * 0.42);
            let col = mix3(col, lin3(p.swamp_algae), algae * 0.32);
            let col = mix3(col, lin3(p.swamp_dark), fleck * 0.20);
            mix3(col, lin3(p.swamp_moss), damp * 0.22)
        }
        TB::Blight => {
            // Churned-black trample troughs, dead-ash patches, a sickly warp-green seep and
            // dried-blood rust stains — the mud should read beaten flat and filthy by ten
            // thousand ork feet. Weights run HOT (an earlier "too busy" pass softened this
            // to one flat tan sheet from gameplay height; the ground near the keep reads
            // blank without the harder contrast).
            let churn = smoothstep(0.08, 1.05, noise_a(x * 4.1 + 11.0, z * 4.1 - 5.0));
            let ash = smoothstep(0.48, 1.22, noise_b(x * 2.6 - 9.0, z * 2.6 + 17.0));
            let seep = smoothstep(0.40, 1.20, noise_a(x * 1.8 + 31.0, z * 1.8 + 3.0));
            let rust = smoothstep(0.55, 1.30, noise_b(x * 5.3 + 19.0, z * 5.3 - 7.0));
            // Fine grain: hairline trample cracks (dark) + dried crust flecks (ash) at high
            // frequency, so the beaten mud has tooth at gameplay height, not flat tan.
            let crack = smoothstep(0.50, 1.30, noise_b(x * 16.0 - 27.0, z * 16.0 + 13.0));
            let crust = smoothstep(0.60, 1.40, noise_a(x * 12.0 + 53.0, z * 12.0 - 37.0));
            let col = mix3(base, lin3(p.blight_dark), churn * 0.78);
            let col = mix3(col, lin3(p.blight_ash), ash * 0.48);
            let col = mix3(col, lin3(p.blight_green), seep * 0.42);
            let col = mix3(col, lin3(p.blight_rust), rust * 0.30);
            let col = mix3(col, lin3(p.blight_dark), crack * 0.22);
            mix3(col, lin3(p.blight_ash), crust * 0.18)
        }
        _ => base,
    }
}

/// Smooth blended ground colour at tile-space (x,z): grass base, each biome blob mixed in
/// over a soft `BLEND` band at its edge, plus a sandy coast fade.
pub(crate) fn ground_color(x: f32, z: f32) -> [f32; 4] {
    if is_arena() {
        return ground_color_arena(x, z);
    }
    let p = &active_map().palette;
    let mut col = lin3(p.grass);
    // Meadow macro-patches on the grass base (before the biome-region blends, so biome
    // interiors keep their own colour): two noise octaves mottle the green between a
    // darker lush tone and a drier warm one. Open grass stops being one flat neon sheet —
    // the patchiness is what makes the ground read as a living meadow at camera distance.
    let p1 = omottle(x, z, 4.0, 31.0); // ~12-world-tile patches
    let p2 = omottle(x, z, 9.0, 53.0); // ~4-tile speckle
    let p3 = omottle(x, z, 1.6, 71.0); // ~30-tile golden sweeps
    col = mix3(col, lin3(p.grass_dark), smoothstep(0.1, 1.3, p1) * 0.55);
    // Warm tones (olive `grass_dry` + yellow `grass_gold`) kept SPARSE — player: too much
    // yellow near the castle + forest. Tightened thresholds + lower mix → a few warm sweeps for
    // variety, not a sheet of it; the lush dark-green stays dominant.
    col = mix3(col, lin3(p.grass_dry), smoothstep(0.4, 1.5, p2) * 0.18);
    col = mix3(col, lin3(p.grass_gold), smoothstep(0.85, 1.6, p3) * 0.10);
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    // Per-vertex WETNESS, carried out in the vertex-colour ALPHA (0 = dry ground, 1 = standing bog).
    // The terrain shader lowers roughness by this, so the marsh's wet sheen FEATHERS across the
    // swamp↔grass edge over the SAME `BLEND` band the colour already blends over — instead of the
    // sheen switching hard per material sheet (grass sheet 1.0 vs the old swamp sheet 0.40), which
    // staircased the boundary into the visible tile "kwadraty" a player reported. Only the Swamp
    // biome is wet; the Blight stays matte (its sheet sits near grass roughness).
    let mut wet = 0.0f32;
    for reg in active_map().regions {
        let fray = if reg.peak > 0 { 0.0 } else { edge_fray(x, z) };
        let d = (x - reg.x).hypot(z - reg.z) + wob + fray;
        let edge = reg.r - d; // >0 inside
        let w = smoothstep(-BLEND, BLEND, edge);
        col = mix3(col, biome_col_at(reg.biome, x, z), w);
        if reg.biome == TB::Swamp {
            wet = wet.max(w);
        }
    }
    // The Blight blends in by its own shape (it's not a `Region` — the landmass lives in
    // `ork_fortress.rs`): the south-swamp green smears into the trampled mud over `BLEND`.
    let blight_w = smoothstep(-BLEND, BLEND, crate::ork_fortress::blight_edge_base(x, z));
    if blight_w > 0.0 {
        col = mix3(col, biome_col_at(TB::Blight, x, z), blight_w);
    }
    // Sandy coast fade — damped inside the Blight: `dist_from_coast` only knows the OLD
    // island shape, so without the damp the whole southern landmass would tint to beach.
    let dco = dist_from_coast(x, z) as f32;
    col = mix3(col, lin3(p.sand), smoothstep(3.5, 0.5, dco) * 0.85 * (1.0 - blight_w));
    // Universal mottle — value jitter + a slow warm/cool hue wander over the *blended*
    // colour, so biome interiors (forest, sand, snow…) get texture too, not just the open
    // grass. Multiplicative and small: it breaks the flat fill without recolouring a biome.
    let m1 = omottle(x, z, 2.2, 17.0); // ~20-tile broad drift
    let m2 = omottle(x, z, 14.0, 91.0); // ~3-tile speckle
    let v = 1.0 + 0.10 * m1 + 0.06 * m2;
    let warm = 0.05 * omottle(x, z, 1.4, 23.0); // +red/−blue ↔ −red/+blue
    col = [
        (col[0] * v * (1.0 + warm)).clamp(0.0, 1.0),
        (col[1] * v).clamp(0.0, 1.0),
        (col[2] * v * (1.0 - warm)).clamp(0.0, 1.0),
    ];
    // Worn dirt baked straight into the ground (NOT raised geometry — just a brown blend in the
    // terrain, like the original game's paths), so it's the SAME surface as the lawn rather than a
    // slab laid on top. The open gate approach-roads OUTSIDE the walls get a light worn tint; the
    // castle yard (plaza + gate paths) INSIDE is more heavily trodden — a darker packed-earth
    // "klepisko" — layered on after, so it dominates and stays continuous through the gates.
    // `ground_color` runs in BASE space (0..BASE_COLS, island centre at CX); the dirt queries are
    // world-space (castle at origin), so convert: world = base·MAP_SCALE − G. (The old code passed
    // `x − GX` here, which dropped the ·MAP_SCALE and mis-placed the worn dirt — the bug that made
    // the baked paths invisible and left the courtyard relying on an overlaid slab.)
    let wx = x * MAP_SCALE - GX;
    let wz = z * MAP_SCALE - GZ;
    let road_s = crate::roads::road_strength(wx, wz);
    if road_s > 0.0 {
        // Rock ground is dark grey-slate, so the warm path tint barely showed — trails through the
        // mountains read as invisible. On rock, blend HARDER and toward a lighter packed-earth so
        // the road pops against the crags; other biomes keep the softer authored tint.
        let rocky = matches!(tile_biome_world(wx, wz), Some(Biome::Rocky));
        let (road_col, road_blend) = if rocky { (0xba9057u32, 0.95) } else { (p.road_dirt, 0.85) };
        // Readability rework (map-character overhaul pass 2): a road reads via its EDGE, not a
        // linear smear. The old `road_s * blend` faded the tint out over the whole falloff, which
        // is exactly why the paths "melted into the biome". Now: a full-strength packed CORE with
        // a hard-ish shoulder (smoothstep over the falloff), plus a faint worn VERGE halo just
        // outside the shoulder — grass thinning toward the road, like a real trodden margin.
        // (Wheel ruts are NOT painted here: ~0.5u grooves are far below the 1-unit vertex-colour
        // resolution — they live in the fragment-resolution rut mask the terrain shader samples,
        // see `roads::bake_rut_mask` + `terrain.wgsl`.)
        let core_w = smoothstep(0.30, 0.55, road_s);
        let verge_w = smoothstep(0.02, 0.14, road_s) * (1.0 - smoothstep(0.18, 0.34, road_s));
        col = mix3(col, lin3(road_col), (verge_w * 0.28 + core_w * road_blend).min(1.0));
    }
    let yard_s = crate::castle::yard_strength(wx, wz);
    if yard_s > 0.0 {
        col = mix3(col, lin3(0x52391f), yard_s * 0.92);
    }
    [col[0], col[1], col[2], wet.clamp(0.0, 1.0)]
}

// ── Shore-distance field (water shader foam / shallows) ─────────────────────────
/// Max encoded shore distance, in tiles — keep in sync with `SHORE_MAX` in
/// `assets/shaders/water.wgsl`.
const SHORE_MAX: f32 = 8.0;

/// Bake an R8 distance-to-land field over the island (1 texel = 1 world unit, with a
/// sea margin all round): 0 on land ramping to 1 at ≥ [`SHORE_MAX`] tiles offshore.
/// The water shader samples it for the shore foam collar + shallow→deep gradient —
/// the terrain has **no underwater geometry** (cliff walls stop at the waterline), so
/// scene depth can't measure shallowness; this baked field is the only source.
/// Returns the image plus the world→UV mapping (`xy` = min corner, `zw` = 1/extent).
fn bake_shore_distance(images: &mut Assets<Image>) -> (Handle<Image>, Handle<Image>, Vec4) {
    // Derived from the grid (was hardcoded 288×384 for MAP_SCALE 1.8 — at 2.2 the island reaches
    // world x ±~156 and the Blight z ~+240, past the old cover, which silently cut the water
    // shader's foam/shallows off at the fringes). +2·16 texels of open-sea margin all round so
    // the distance field ramps fully out to SHORE_MAX before clamp-to-edge takes over.
    let w: usize = COLS as usize + 32; // covers world x ∈ [−GX−16, COLS−GX+16]
    let h: usize = ROWS as usize + 32; // covers world z ∈ [−GZ−16, ROWS−GZ+16]
    let min_x = -GX - 16.0;
    let min_z = -GZ - 16.0;
    #[allow(non_snake_case)]
    let (W, H) = (w, h);
    let idx = |x: usize, z: usize| z * W + x;

    // Land mask → two-pass 8-neighbour chamfer distance transform (≈Euclidean).
    let mut d = vec![f32::INFINITY; W * H];
    for z in 0..H {
        for x in 0..W {
            let wx = min_x + x as f32 + 0.5;
            let wz = min_z + z as f32 + 0.5;
            if ground_at_world(wx, wz).is_some() {
                d[idx(x, z)] = 0.0;
            }
        }
    }
    const DIAG: f32 = 1.414;
    for z in 0..H {
        for x in 0..W {
            let mut v = d[idx(x, z)];
            if x > 0 { v = v.min(d[idx(x - 1, z)] + 1.0); }
            if z > 0 { v = v.min(d[idx(x, z - 1)] + 1.0); }
            if x > 0 && z > 0 { v = v.min(d[idx(x - 1, z - 1)] + DIAG); }
            if x + 1 < W && z > 0 { v = v.min(d[idx(x + 1, z - 1)] + DIAG); }
            d[idx(x, z)] = v;
        }
    }
    for z in (0..H).rev() {
        for x in (0..W).rev() {
            let mut v = d[idx(x, z)];
            if x + 1 < W { v = v.min(d[idx(x + 1, z)] + 1.0); }
            if z + 1 < H { v = v.min(d[idx(x, z + 1)] + 1.0); }
            if x + 1 < W && z + 1 < H { v = v.min(d[idx(x + 1, z + 1)] + DIAG); }
            if x > 0 && z + 1 < H { v = v.min(d[idx(x - 1, z + 1)] + DIAG); }
            d[idx(x, z)] = v;
        }
    }

    let data: Vec<u8> =
        d.iter().map(|v| ((v / SHORE_MAX).clamp(0.0, 1.0) * 255.0) as u8).collect();
    let linear = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    let mut img = Image::new(
        Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Linear filtering smooths the 1-texel bands; the default clamp-to-edge address
    // mode makes off-texture samples read the border (open sea = max distance).
    img.sampler = linear.clone();
    // BOG mask (map-character overhaul pass 3): 1 over the swamp's carved standing POOLS AND over
    // river water inside the swamp regions, same region mapping as the shore field — the water
    // shader swaps the vivid river palette for still dark-olive murk there and kills the foam
    // collar (a bog doesn't lap; a marsh stream shouldn't glow turquoise between pools).
    // NB: keys on `pool_sd` (the olive blobs), NOT `is_pool` — the latter also covers the lake's
    // blue feeder/drain streams (`BLUE_STREAMS`), which must render as CLEAR water, not bog.
    let bog_data: Vec<u8> = (0..W * H)
        .map(|i| {
            let wx = min_x + (i % W) as f32 + 0.5;
            let wz = min_z + (i / W) as f32 + 0.5;
            if pool_sd_world(wx, wz) < 0.0 || (in_swamp_region_world(wx, wz) && is_river_world(wx, wz)) {
                255
            } else {
                0
            }
        })
        .collect();
    let mut bog_img = Image::new(
        Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        bog_data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    bog_img.sampler = linear;
    let region = Vec4::new(min_x, min_z, 1.0 / W as f32, 1.0 / H as f32);
    (images.add(img), images.add(bog_img), region)
}

// ── Build ────────────────────────────────────────────────────────────────────────
#[allow(clippy::too_many_arguments)]
/// Cross-frame state for the chunked build: data made by one phase and consumed by a later one.
/// The build is now spread one phase per frame (see [`build_step`]) so the loading veil can
/// animate, so these can no longer be plain locals on the stack.
#[derive(Default)]
pub struct BuildState {
    /// Shared textured material set from [`crate::castle::build`] (phase 13), reused by the town
    /// plots (phase 19).
    village_mats: Option<crate::castle::Mats>,
    /// Per-biome atmosphere/weather captured during the scatter phases (5–9), inserted as the
    /// [`crate::biome::BiomeAmbiences`] resource at phase 10.
    ambiences: Vec<(Biome, BiomeAmbience)>,
}

/// Number of phases [`build_step`] walks through. The loading veil maps its progress bar onto this.
pub const BUILD_STEPS: u32 = 34;

/// Build the whole world in ONE call (terrain → scatter → castle → fortress → …). Used by the
/// capture harnesses (`FOREST_SHOT`/`FOREST_CLIP`), which want the world up on frame 0. The normal
/// game path drives [`build_step`] one phase per frame instead (see `biome::drive_build`) so the
/// render loop — and the loading screen — can tick between phases.
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
    terrain_mats: &mut Assets<TerrainMaterial>,
    water_mats: &mut Assets<WaterMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
    let mut state = BuildState::default();
    for step in 0..BUILD_STEPS {
        build_step(step, commands, meshes, images, std_mats, terrain_mats, water_mats, creature_mats, &mut state);
    }
}

/// Run ONE phase of the world build. The phases are exactly the sequence the old monolithic `build`
/// ran, in the same order — just one per call, so the render loop can present between them.
/// Cross-phase data rides [`BuildState`]. Steps past the last are no-ops.
#[allow(clippy::too_many_arguments)]
pub fn build_step(
    step: u32,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
    terrain_mats: &mut Assets<TerrainMaterial>,
    water_mats: &mut Assets<WaterMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    state: &mut BuildState,
) {
    // The RTS arena runs a small, dedicated subset of the build: grass terrain, the sea, the
    // decorative fringe trees, ambiences and ground cover. EVERY campaign placement — castle, town
    // plots, camps, ore, chests, wildlife, defenders, ruins/landmarks, vignettes, ork fortress,
    // bridges, distant isles, rival stronghold, meadow, wayside, bog, POIs, vista — is intentionally
    // absent (the wave-2 `rts` modules spawn the arena's own props: bases, workers, deposits). The
    // campaign `roads` network is NOT baked either (its build reads castle gates / biomes / fortress
    // / rival, none of which exist here) — the arena road is painted straight into `ground_color`.
    if is_arena() {
        match step {
            0 => bs_grass_sheet(commands, meshes, images, terrain_mats),
            // Sea plane + shore-distance bake (boats + camp planning are skipped inside).
            1 => bs_sea_and_boats(commands, meshes, images, std_mats, water_mats),
            // Decorative light-forest trees on the `Forest` tiles (the outer fringe + the two
            // dense stands).
            2 => bs_scatter_biome(Biome::Forest, commands, meshes, std_mats, state),
            // The arena's own biome regions get their native props, so each place reads as a place:
            // crags on the iron mountains + rocky bluffs, firs/rime on the snow highlands, cacti and
            // dry rocks on the dune scrub. (`classify_arena` only emits these tiles inside the
            // authored mirrored regions, so the scatter can't stray onto the meadow or the lane.)
            3 => bs_scatter_biome(Biome::Rocky, commands, meshes, std_mats, state),
            4 => bs_scatter_biome(Biome::Snow, commands, meshes, std_mats, state),
            5 => bs_scatter_biome(Biome::Desert, commands, meshes, std_mats, state),
            // BiomeAmbiences resource (atmosphere/particles) — ungated render systems read it.
            6 => bs_insert_ambiences(commands, state),
            // Meadow ground cover (tufts / clover / flowers) on the open grass.
            7 => bs_grass_cover(commands, meshes, std_mats),
            _ => {}
        }
        return;
    }
    match step {
        0 => bs_grass_sheet(commands, meshes, images, terrain_mats),
        1 => bs_swamp_sheet(commands, meshes, images, terrain_mats),
        2 => bs_blight_sheet(commands, meshes, images, terrain_mats),
        // (3 was bs_lava_sheet — dropped with the Ashlands map; Home/Arena grow no lava tiles.)
        4 => bs_sea_and_boats(commands, meshes, images, std_mats, water_mats),
        5 => bs_scatter_biome(Biome::Forest, commands, meshes, std_mats, state),
        6 => bs_scatter_biome(Biome::Snow, commands, meshes, std_mats, state),
        7 => bs_scatter_biome(Biome::Rocky, commands, meshes, std_mats, state),
        8 => bs_scatter_biome(Biome::Desert, commands, meshes, std_mats, state),
        9 => bs_scatter_biome(Biome::Swamp, commands, meshes, std_mats, state),
        10 => bs_insert_ambiences(commands, state),
        11 => bs_snow_drifts(commands, meshes, std_mats),
        12 => bs_grass_cover(commands, meshes, std_mats),
        13 => state.village_mats = Some(crate::castle::build(commands, meshes, images, std_mats)),
        14 => crate::camps::build(commands, meshes, images, std_mats, creature_mats),
        15 => crate::villagers::populate(
            commands,
            meshes,
            state.village_mats.as_ref().expect("castle (phase 13) runs before village props (phase 15)"),
            creature_mats,
        ),
        16 => crate::training_dummies::populate(commands, meshes, std_mats),
        17 => crate::wildlife::populate(commands, meshes, creature_mats),
        18 => crate::verbs::populate_ore(commands, meshes, std_mats),
        19 => crate::town::populate_plots(
            commands,
            meshes,
            state.village_mats.as_ref().expect("castle (phase 13) runs before town plots (phase 19)"),
        ),
        20 => crate::verbs::populate_forage(commands, meshes, std_mats),
        21 => crate::chest::populate_chests(commands, meshes, std_mats),
        22 => crate::defenses::populate_defenders(commands, meshes, std_mats, creature_mats),
        23 => crate::ruins::populate_landmarks(commands, meshes, std_mats),
        24 => crate::vignettes::populate_vignettes(commands, meshes, std_mats),
        25 => crate::ork_fortress::build(commands, meshes, images, std_mats, creature_mats),
        26 => crate::bridges::populate(commands, meshes, std_mats),
        27 => crate::distant_isles::build(commands, meshes, std_mats),
        28 => crate::rival::build(commands, meshes, images, std_mats),
        // (29 was bs_swamp_pools — flat teal water discs, removed: read as ugly "plates"; real
        // carved pools also dropped — too small, the water shader foamed them solid white.)
        // Now: the castle-meadow dressing (hay corner, beehives, rest campfire, tree clumps) —
        // fills the bald safe-zone clearing. After the castle/plots so its placement rules hold.
        29 => crate::meadow::build(commands, meshes, std_mats),
        // Wayside furniture (map-character overhaul pass 2): junction signposts + roadside
        // cairns/fences/shrines. Late — its placement rejects everything the earlier phases own.
        30 => crate::wayside::populate(commands, meshes, std_mats),
        // Bog dressing (pass 3): drowned trees/wisps/glow-mushrooms over the swamp pools + the
        // drowned tower + stilt hut. After bridges (boardwalk keep-out) and roads.
        31 => crate::bog::populate(commands, meshes, std_mats),
        // Micro-POIs + flags (pass 4): story set-pieces along the arteries, the gallows on the
        // Gnashfang approach, smoke + crows over the Hold, farmland around the castle.
        32 => crate::poi::populate(commands, meshes, std_mats),
        // Vista pass (pass 5): the lake waterfall off the mesa wall + three framed overlooks.
        33 => crate::vista::populate(commands, meshes, std_mats),
        _ => {}
    }
}

// ── Build phases (each one [`build_step`] arm; see the old `build` doc for the why of each) ──

/// The island proper: grass blade-grain detail, all tiles that aren't Blight/Swamp.
fn bs_grass_sheet(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, terrain_mats: &mut Assets<TerrainMaterial>) {
    let grass_detail = GroundDetail {
        scale: 0.18,
        strength: 0.52,
        variation: 0.70,
        seed: 1.0,
        dark: 0x356b28,
        base: 0x5d9e44,
        light: 0x95d162,
        grain: 0.72,
        streak: 0.5,
    };
    let ground_mat = crate::terrain::make_material(&grass_detail, 1.0, Some(rut_mask_image(images)), images, terrain_mats);
    spawn_terrain_sheet(commands, meshes, ground_mat, |tb| tb != TB::Blight && tb != TB::Swamp);
}

/// The swamp: bespoke wet blotchy mottle (not the grass blades), lower roughness for a bog sheen.
fn bs_swamp_sheet(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, terrain_mats: &mut Assets<TerrainMaterial>) {
    let swamp_detail = GroundDetail {
        scale: 0.16,
        strength: 0.6, // a touch stronger so the wet/dry blotch reads
        variation: 1.0, // max blotch → standing wet pools vs. exposed muck
        seed: 11.0,
        dark: 0x1f2b18, // deeper near-black-green: the standing bog-water pools (was 0x2c3522)
        base: 0x434f37,
        light: 0x6f8350, // slightly brighter wet-sheen crest (was 0x687a4a)
        grain: 0.7,
        streak: 0.6,
    };
    // Base roughness is now MATTE 1.0 — the same as the grass sheet — on purpose. The wet bog sheen
    // no longer lives in this sheet's constant roughness (which staircased the swamp↔grass boundary,
    // since the grass sheet stayed 1.0 and the step fell exactly on the tile edges — player: visible
    // "kwadraty"). Instead the shader lowers roughness per-fragment by the vertex-colour ALPHA wetness
    // that `ground_color` feathers over the biome BLEND band, so both sheets resolve the SAME
    // roughness at a shared boundary vertex → no seam, while the marsh interior (alpha≈1) still reads
    // as the wet ~0.40-roughness muck it did before (player: "bardziej mokre bagno").
    let swamp_mat = crate::terrain::make_material(&swamp_detail, 1.0, Some(rut_mask_image(images)), images, terrain_mats);
    spawn_terrain_sheet(commands, meshes, swamp_mat, |tb| tb == TB::Swamp);
}

/// The Blight: its own filthy beaten-earth grain so the trampled mud reads near the keep.
fn bs_blight_sheet(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, terrain_mats: &mut Assets<TerrainMaterial>) {
    let blight_detail = GroundDetail {
        scale: 0.26,
        strength: 0.60,
        variation: 0.78,
        seed: 7.0,
        dark: 0x2c2114,
        base: 0x4d3e2a,
        light: 0x6b5c42,
        grain: 0.88,
        streak: 0.55,
    };
    let blight_mat = crate::terrain::make_material(&blight_detail, 0.97, Some(rut_mask_image(images)), images, terrain_mats);
    spawn_terrain_sheet(commands, meshes, blight_mat, |tb| tb == TB::Blight);
}

/// The cart-wheel-rut mask (fragment-resolution road grooves, `roads::bake_rut_mask`) as a GPU
/// image + world→UV region, baked once per process (the road network itself is process-cached)
/// and shared by all terrain sheets.
fn rut_mask_image(images: &mut Assets<Image>) -> (Handle<Image>, Vec4) {
    static CACHE: Mutex<Option<(Handle<Image>, Vec4)>> = Mutex::new(None);
    let mut c = CACHE.lock().expect("rut mask cache poisoned");
    if let Some(v) = c.as_ref() {
        return v.clone();
    }
    let (data, w, h, region) = crate::roads::bake_rut_mask();
    let mut img = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    let v = (images.add(img), region);
    *c = Some(v.clone());
    v
}

/// The sea plane + shore-distance bake + background sailboats, then plan the ork camps (BEFORE
/// scatter, so their clearings can be reserved out of the prop placement).
fn bs_sea_and_boats(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, std_mats: &mut Assets<StandardMaterial>, water_mats: &mut Assets<WaterMaterial>) {
    let sea_mesh = meshes.add(Plane3d::default().mesh().size(900.0, 900.0).subdivisions(8).build());
    let (shore_tex, bog_tex, shore_region) = bake_shore_distance(images);
    let sea = water_mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(0x2f as f32 / 255.0, 0x6f as f32 / 255.0, 0xae as f32 / 255.0, 0.9),
            perceptual_roughness: 0.24,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        },
        extension: WaterExt {
            params: WaterParams {
                params: Vec4::new(0.22, 0.45, 0.4, 0.85),
                sky_tint: Vec4::new(0.70, 0.82, 0.93, 1.0),
                region: shore_region,
            },
            shore: Some(shore_tex),
            bog: Some(bog_tex),
        },
    });
    commands.spawn((Mesh3d(sea_mesh), MeshMaterial3d(sea), Transform::from_xyz(0.0, SEA_Y, 0.0), crate::biome::BiomeEntity));

    // The arena wants the sea plane + shore bake (so its coast foams), but none of the campaign
    // dressing: no background sailboats, and no ork-camp planning (camps never spawn on it).
    if is_arena() {
        return;
    }

    // Island shape is authored in BASE space (CX/CZ, ISLAND_R*); convert to world.
    let isle_c = Vec2::new(CX * MAP_SCALE - GX, CZ * MAP_SCALE - GZ);
    let isle_r = Vec2::new(ISLAND_RX * MAP_SCALE, ISLAND_RZ * MAP_SCALE);
    crate::boats::spawn_boats_island(commands, meshes, std_mats, isle_c, isle_r, SEA_Y);

    crate::camps::plan();
}

/// Scatter one biome's props on its tiles (height-aware), and capture its atmosphere/weather into
/// `state.ambiences` for the [`BiomeAmbiences`] resource (inserted at phase 10).
/// Keep desert props (the tall saguaro cacti especially) off the ragged desert↔rock seam. The two
/// regions overlap and `region_at` resolves the overlap with noise, so a desert tile can sit right
/// against the rock foothills — planting cacti among the mountains. Reject a desert tile within a
/// short margin of any rock tile so the biome edge reads clean.
fn desert_near_rock(x: f32, z: f32) -> bool {
    const M: f32 = 7.0;
    const D: f32 = 4.95; // M * 0.707
    for (dx, dz) in [
        (M, 0.0), (-M, 0.0), (0.0, M), (0.0, -M),
        (D, D), (-D, D), (D, -D), (-D, -D),
    ] {
        if tile_biome_world(x + dx, z + dz) == Some(Biome::Rocky) {
            return true;
        }
    }
    false
}

fn bs_scatter_biome(biome: Biome, commands: &mut Commands, meshes: &mut Assets<Mesh>, std_mats: &mut Assets<StandardMaterial>, state: &mut BuildState) {
    let lo = -GX;
    let hi = GX; // square covers the whole grid; off-map tiles mask out
    let cfg = config_for(biome);
    // On a non-Home map override the per-biome HOME atmosphere with the map's own.
    let atmo = if active_id() == 0 {
        AtmoSample::from_config(&cfg)
    } else {
        let (sky, fog, sun_c, sun_i, amb_c, amb_b, _s) = active_atmosphere();
        AtmoSample::from_raw(sky, sun_c, sun_i, amb_c, amb_b, fog)
    };
    state.ambiences.push((biome, BiomeAmbience { atmo, particle: cfg.particle }));
    scatter_region(
        &cfg,
        commands,
        meshes,
        std_mats,
        lo,
        hi,
        false,
        &move |x, z| {
            let here = tile_biome_world(x, z);
            let biome_match = here == Some(biome);
            biome_match
                && !is_river_world(x, z) // keep props off the (sub-tile) river surface
                && !is_pool_world(x, z) // …and off the swamp bog pools
                && !crate::camps::in_clearing(x, z)
                && !crate::bridges::near_bridge(x, z, 1.0)
                && !crate::ork_fortress::on_gate_approach(x, z)
                && !(crate::ork_fortress::in_blight_world(x, z) && z > 86.0)
                && !crate::rival::near_fort(x, z)
                && !(biome == Biome::Desert && desert_near_rock(x, z))
        },
        &|x, z| tile_top_y_world(x, z),
        // Biome cover classes differ per biome (an index-keyed lean would mis-assign), so the
        // density drift still applies but species stay neutral here — only the home meadow leans.
        &[],
    );
}

/// Insert the [`BiomeAmbiences`] resource: island base + the per-biome list captured during
/// scatter + the Blight's bespoke red-ember mood.
fn bs_insert_ambiences(commands: &mut Commands, state: &mut BuildState) {
    let (sky, fog, sun_c, sun_i, amb_c, amb_b, _sun_p) = active_atmosphere();
    commands.insert_resource(BiomeAmbiences {
        base: BiomeAmbience {
            atmo: AtmoSample::from_raw(sky, sun_c, sun_i, amb_c, amb_b, fog),
            particle: ParticleKind::None,
        },
        list: std::mem::take(&mut state.ambiences),
        blight: crate::ork_fortress::blight_ambience(),
    });
}

/// Snow drifts banked against terrace walls where a snow tile abuts a higher neighbour.
fn bs_snow_drifts(commands: &mut Commands, meshes: &mut Assets<Mesh>, std_mats: &mut Assets<StandardMaterial>) {
    let drift_mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE, // vertex colour carries the hue (mesh contract)
        perceptual_roughness: 0.62,
        reflectance: 0.5,
        ..default()
    });
    let drift_meshes: Vec<Handle<Mesh>> =
        (0..3).map(|v| meshes.add(crate::biome_snow::build_mound_mesh(v))).collect();
    for iz in 0..ROWS {
        for ix in 0..COLS {
            let Some((TB::Snow, h)) = tile_at(ix, iz) else { continue };
            let mut rng = tileworld_core::rng::Mulberry32::new((iz * COLS + ix) as u32 ^ 0x5eed_d81f);
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let Some((_, nh)) = tile_at(ix + dx, iz + dz) else { continue };
                if nh <= h || rng.next() > 0.35 {
                    continue;
                }
                // Pull the mound off the shared edge into THIS (lower) tile so it leans on the wall.
                let wx = ix as f32 - GX + 0.5 - dx as f32 * 0.32 + (rng.next() as f32 - 0.5) * 0.3;
                let wz = iz as f32 - GZ + 0.5 - dz as f32 * 0.32 + (rng.next() as f32 - 0.5) * 0.3;
                if crate::bridges::near_bridge(wx, wz, 0.5) {
                    continue; // keep drift mounds off the plank decks
                }
                let v = (rng.next() * 3.0) as u32;
                commands.spawn((
                    Mesh3d(drift_meshes[(v % 3) as usize].clone()),
                    MeshMaterial3d(drift_mat.clone()),
                    Transform {
                        translation: Vec3::new(wx, tile_top_y_world(wx, wz), wz),
                        rotation: Quat::from_rotation_y(rng.next() as f32 * std::f32::consts::TAU),
                        scale: Vec3::splat(0.9 + rng.next() as f32 * 0.7),
                    },
                    crate::biome::BiomeEntity,
                ));
            }
        }
    }
}

/// Grass frontier cover (tufts/clover/flowers) on grass tiles, around the castle/camps/plots.
fn bs_grass_cover(commands: &mut Commands, meshes: &mut Assets<Mesh>, std_mats: &mut Assets<StandardMaterial>) {
    let lo = -GX;
    let hi = GX;
    let grass_cfg = grass_config();
    // Per-class damp lean, in `frontier_cover()` order [grass, clover, fern, flower, mushroom]:
    // grass + sun-flowers (poppies/buttercups) take the dry/trodden sweeps, while ferns, clover
    // and mushrooms cluster in the damp green hollows — so WHICH plant you see matches the patch
    // it grows on.
    let meadow_affinity: &[f32] = &[-0.6, 0.5, 0.9, -0.15, 1.0];
    scatter_region(
        &grass_cfg,
        commands,
        meshes,
        std_mats,
        lo,
        hi,
        false,
        &|x, z| {
            is_grass_world(x, z)
                && !is_river_world(x, z) // keep cover off the (sub-tile) river surface
                && !crate::castle::in_footprint(x, z)
                && !crate::camps::in_clearing(x, z)
                && !crate::town::near_build_plot(x, z)
                && !crate::bridges::near_bridge(x, z, 1.0)
        },
        &|x, z| tile_top_y_world(x, z),
        meadow_affinity,
    );
}

fn config_for(b: Biome) -> BiomeConfig {
    match b {
        Biome::Forest => crate::biome_forest::config(),
        Biome::Snow => crate::biome_snow::config(),
        Biome::Rocky => crate::biome_rocky::config(),
        Biome::Desert => crate::biome_desert::config(),
        Biome::Swamp => crate::biome_swamp::config(),
    }
}

/// Ground cover for the open frontier: the lush meadow (grass tufts, clover, ferns, flowers,
/// mushrooms — all hard-coded green/floral in `groundcover.rs`).
fn frontier_cover() -> Vec<PropClass> {
    vec![
        PropClass {
            variants: (0..gc::NUM_GRASS_VARIANTS).map(|v| (gc::build_grass_tuft_mesh(v), 1.0)).collect(),
            chance: 0.32,
            scale: (0.6, 1.25),
            tree: false,
            block_radius: 0.0,
        },
        PropClass {
            variants: (0..gc::NUM_CLOVER_VARIANTS).map(|v| (gc::build_clover_mesh(v), 1.0)).collect(),
            chance: 0.30,
            scale: (0.7, 1.2),
            tree: false,
            block_radius: 0.0,
        },
        PropClass {
            variants: (0..gc::NUM_FERN_VARIANTS).map(|v| (gc::build_fern_mesh(v), 1.0)).collect(),
            chance: 0.10,
            scale: (0.5, 0.95),
            tree: false,
            block_radius: 0.0,
        },
        PropClass {
            variants: (0..gc::NUM_FLOWER_VARIANTS).map(|v| (gc::build_flower_mesh(v), 1.0)).collect(),
            chance: 0.16,
            scale: (0.8, 1.4),
            tree: false,
            block_radius: 0.0,
        },
        PropClass {
            variants: (0..2).map(|v| (gc::build_mushroom_mesh(v), 1.0)).collect(),
            chance: 0.05,
            scale: (0.6, 1.0),
            tree: false,
            block_radius: 0.0,
        },
    ]
}

/// A cover-only pseudo-config for the open grass frontier (no trees/rocks).
fn grass_config() -> BiomeConfig {
    BiomeConfig {
        biome: Biome::Forest,
        name: "Grass",
        ground_color: active_map().palette.grass,
        ground_roughness: 0.95,
        detail: GroundDetail {
            scale: 0.18, strength: 0.4, variation: 0.7, seed: 1.0,
            dark: 0x356b28, base: 0x5d9e44, light: 0x95d162, grain: 0.55, streak: 0.5,
        },
        sky: 0xb4d2ec, fog_density: 0.0035, sun_color: 0xffedc7, sun_illuminance: 11_000.0,
        ambient_color: 0xe6edf5, ambient_brightness: 95.0, sun_pos: Vec3::new(80.0, 110.0, 40.0),
        seed: 7777,
        tree_min_dist: 2.0,
        classes: vec![],
        cover: frontier_cover(),
        cover_per_tile: 2,
        river: false,
        river_color: 0x2f8fd6,
        backdrop: Backdrop {
            land_dir: 0.0, land_arc: std::f32::consts::PI, ocean: false, ocean_color: 0x2f6fae,
            hill_body: 0x8f9aa0, hill_cap: 0xb8c2c6, hill_foot: 0x7a8890,
            treeline: false, treeline_dark: 0x2c4a34, treeline_mid: 0x365c3e, hill_h: (40.0, 90.0),
        },
        particle: ParticleKind::None,
    }
}

// ── Terraced terrain mesh ─────────────────────────────────────────────────────────
/// Build the terraced ground mesh over every tile `keep` accepts. Called three times from
/// `build` — island-minus-swamp-minus-Blight (grass detail), the swamp (wet-mottle detail)
/// and the Blight (trampled-mud detail) — so each sheet bakes its own grain. Walls key off
/// the neighbour's `tile_at` height, not `keep`, so a split never opens a seam: the wall is
/// always owned (and drawn) by the higher tile's sheet.
/// Terrain chunk edge, in tiles. ~6×7 chunks per sheet across the 259×295 island — fine enough
/// that a low follow-cam (a narrow near-ground frustum) frustum-culls most of the island, coarse
/// enough that the extra draw calls (≤~40 per sheet) are trivial on the GPU.
const TERRAIN_CHUNK: i32 = 48;

/// Spawn a terrain sheet as frustum-cullable CHUNKS instead of one island-spanning monolith. The
/// monolith carried a single AABB, so the whole sheet (up to ~1.2M verts across all sheets) was
/// submitted every frame even when only a corner was on screen. Splitting into `TERRAIN_CHUNK`-tile
/// blocks — all sharing the one `mat` handle, so they still batch — gives each chunk its own AABB,
/// and Bevy frustum-culls the off-screen majority for free. Empty chunks (e.g. grass over open sea)
/// emit no geometry and aren't spawned.
fn spawn_terrain_sheet(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: Handle<TerrainMaterial>,
    keep: impl Fn(TB) -> bool + Copy,
) {
    let lod = terrain_lod_enabled();
    let mut cz = 0;
    while cz < ROWS {
        let mut cx = 0;
        while cx < COLS {
            let (x1, z1) = ((cx + TERRAIN_CHUNK).min(COLS), (cz + TERRAIN_CHUNK).min(ROWS));
            let mesh = build_terrain_chunk(keep, cx, x1, cz, z1);
            if mesh.count_vertices() > 0 {
                // Attach the chunk's own AABB up front. The chunk entity carries `Transform::default()`
                // with its geometry baked in WORLD space, and the LOD `VisibilityRange` measures the
                // camera→AABB-centre distance. Until Bevy's `calculate_bounds` fills the AABB in, the
                // range check falls back to the entity *translation* — i.e. the world ORIGIN (castle) —
                // so a chunk under a player standing far from the castle reads as "far" and gets pinned
                // to the coarse drape (the "underfoot chunk stuck in low-res" bug), a window that
                // re-opens on every world/biome rebuild. Baking the real AABB here closes that gap.
                use bevy::camera::primitives::MeshAabb;
                let aabb = mesh.compute_aabb();
                let mut e = commands.spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(mat.clone()),
                    Transform::default(),
                    crate::biome::BiomeEntity,
                ));
                if let Some(aabb) = aabb {
                    e.insert(aabb);
                }
                // Terrain LOD: past TERRAIN_LOD the full-res chunk (1 quad/tile + walls +
                // marching-squares banks) hands off to a stride-4 coarse drape — ~1/16th the
                // vertices for the distant majority of the island. The band is a dithered
                // crossfade (only the ring of chunks currently inside it pays the discard
                // cost) so the swap doesn't pop; skirts on the coarse mesh hide the seam.
                if lod {
                    if let Some(coarse) = build_terrain_chunk_coarse(keep, cx, x1, cz, z1) {
                        e.insert(bevy::camera::visibility::VisibilityRange {
                            start_margin: 0.0..0.0,
                            end_margin: TERRAIN_LOD..TERRAIN_LOD + TERRAIN_LOD_BAND,
                            use_aabb: true,
                        });
                        // Same AABB-up-front fix for the coarse sibling: without it, a missing AABB
                        // would fall back to the origin and this drape could flip visible near the
                        // castle / hidden far away — the inverse of the underfoot low-res glitch.
                        let caabb = coarse.compute_aabb();
                        let mut ce = commands.spawn((
                            Mesh3d(meshes.add(coarse)),
                            MeshMaterial3d(mat.clone()),
                            Transform::default(),
                            crate::biome::BiomeEntity,
                            // Shadow cascades stop at ~150 and the fog is thick out there —
                            // the coarse drape is pure fill, never a shadow caster.
                            bevy::light::NotShadowCaster,
                            bevy::camera::visibility::VisibilityRange {
                                start_margin: TERRAIN_LOD..TERRAIN_LOD + TERRAIN_LOD_BAND,
                                end_margin: 1.0e30..1.0e30, // no far cutoff — terrain always draws
                                use_aabb: true,
                            },
                        ));
                        if let Some(caabb) = caabb {
                            ce.insert(caabb);
                        }
                    }
                }
            }
            cx += TERRAIN_CHUNK;
        }
        cz += TERRAIN_CHUNK;
    }
}

/// Where the full-res terrain chunk hands off to its coarse LOD (world units, camera→chunk-AABB
/// distance), and the width of the dithered crossfade band. 110 sits past the base fog start
/// (≈56) and the prop/cover culls (62/75), well before trees vanish (180) — so the geometry
/// swap happens under haze, on chunks the eye reads mostly by silhouette.
/// Pushed out from an earlier 110: the island spans ±150–280u from the castle-origin camera, so at
/// 110 the *entire* coastline sat beyond the swap and always read as the coarse drape — the
/// "blurry edges". 150 keeps the near shore full-res while still swapping under haze on chunks the
/// eye reads by silhouette.
const TERRAIN_LOD: f32 = 150.0;
const TERRAIN_LOD_BAND: f32 = 26.0;
/// Coarse-LOD sampling stride, in tiles (one quad per 2×2 tiles ≈ 1/4 the vertices). Was 4 (1/16),
/// but the ground's hue + biome blend live entirely in per-vertex color, so a stride-4 drape carried
/// its color field at 1/16 res and stretched it across 4-tile quads → the smeared far-terrain look.
/// Stride 2 gives 4× the color/geometry resolution on the drape while staying far cheaper than full.
/// Must divide `TERRAIN_CHUNK` so coarse cell corners line up across chunk seams.
const LOD_STRIDE: i32 = 2;

/// Whether terrain LOD is on. Shares the scatter culling's `FOREST_NOCULL=1` escape hatch —
/// the whole-island map-shot recipe and perf A/B runs want EVERYTHING at full res.
fn terrain_lod_enabled() -> bool {
    // The ARENA is always full-res — no coarse drape at all.
    //
    // The drape deliberately skips the marching-squares shore cut (see `build_terrain_chunk_coarse`),
    // which is exactly what draws the arena's smooth coastline; past the hand-off distance the
    // island's edge therefore snapped back to the tile-grid sawtooth this pass exists to remove.
    // On the campaign island that trade is invisible (its far coast is haze-fogged and you never
    // orbit it), but the RTS camera strips the fog and lives zoomed OUT, so the toothed rim was the
    // first thing you saw. Affordable: the arena's whole land disc (r = `ARENA_LAND_R`) is ~34k
    // tiles — roughly HALF the ~70k the campaign already keeps full-res inside its 150u LOD radius —
    // and the skirmish map carries none of the campaign's scatter load on top.
    //
    // Checked BEFORE the `OnceLock`: the cache must not freeze a campaign answer onto the arena
    // (or vice versa) if the active map ever changes within a process.
    if is_arena() {
        return false;
    }
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_NOCULL").is_err())
}

/// Build the coarse far-LOD drape for one terrain chunk: the smoothed ground surface sampled
/// every `LOD_STRIDE` tiles, no terrace walls, no marching-squares river cuts (the channel just
/// drapes toward `SEA_Y` and the always-drawn water plane reads as the river), plus a short
/// downward SKIRT around the chunk perimeter so LOD seams against neighbouring full-res chunks
/// can't open see-through cracks. Cell→sheet routing matches the full-res pass (centre tile's
/// class through `keep`), so each sheet's coarse drape wears its own material grain.
fn build_terrain_chunk_coarse(
    keep: impl Fn(TB) -> bool,
    ix0: i32,
    ix1: i32,
    iz0: i32,
    iz1: i32,
) -> Option<Mesh> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Ground height at a stride-grid corner (world Y), `None` over water / off the island.
    let corner_y = |cx: i32, cz: i32| -> Option<f32> { smooth_surface_y(cx as f32 - GX, cz as f32 - GZ) };
    // Sea-tucked height for a wet corner: just under the water plane so the drape dips beneath.
    const WET_Y: f32 = SEA_Y - 0.12;
    let nrm3 = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-4);
        [v[0] / l, v[1] / l, v[2] / l]
    };

    let mut push_quad = |p: [[f32; 3]; 4], n: [[f32; 3]; 4], c: [[f32; 4]; 4]| {
        let b = positions.len() as u32;
        for k in 0..4 {
            positions.push(p[k]);
            normals.push(n[k]);
            colors.push(c[k]);
        }
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    };

    let mut cz = iz0;
    while cz < iz1 {
        let z1 = (cz + LOD_STRIDE).min(iz1);
        let mut cx = ix0;
        while cx < ix1 {
            let x1 = (cx + LOD_STRIDE).min(ix1);
            // Route the cell to a sheet by its centre tile (or any land corner on the coast).
            let centre = tile_at((cx + x1) / 2, (cz + z1) / 2);
            let class = centre.or_else(|| {
                [(cx, cz), (x1, cz), (x1, z1), (cx, z1)].iter().find_map(|&(gx, gz)| tile_at(gx, gz))
            });
            let Some((tb, _)) = class else {
                cx += LOD_STRIDE;
                continue; // open sea cell
            };
            if !keep(tb) {
                cx += LOD_STRIDE;
                continue;
            }
            let corners = [(cx, cz), (x1, cz), (x1, z1), (cx, z1)];
            let ys = corners.map(|(gx, gz)| corner_y(gx, gz));
            if ys.iter().all(|y| y.is_none()) {
                cx += LOD_STRIDE;
                continue;
            }
            let y = ys.map(|y| y.unwrap_or(WET_Y));
            // Central-difference normals over the stride grid (wet samples clamp to WET_Y).
            let cn = |gx: i32, gz: i32| {
                let s = LOD_STRIDE;
                let e = corner_y(gx + s, gz).unwrap_or(WET_Y);
                let w = corner_y(gx - s, gz).unwrap_or(WET_Y);
                let so = corner_y(gx, gz + s).unwrap_or(WET_Y);
                let no = corner_y(gx, gz - s).unwrap_or(WET_Y);
                nrm3([w - e, 2.0 * s as f32, no - so])
            };
            let p = corners.map(|(gx, gz)| [gx as f32 - GX, 0.0, gz as f32 - GZ]);
            let p = [
                [p[0][0], y[0], p[0][2]],
                [p[1][0], y[1], p[1][2]],
                [p[2][0], y[2], p[2][2]],
                [p[3][0], y[3], p[3][2]],
            ];
            let n = [cn(cx, cz), cn(x1, cz), cn(x1, z1), cn(cx, z1)];
            let c = corners.map(|(gx, gz)| ground_color(gx as f32 / MAP_SCALE, gz as f32 / MAP_SCALE));
            push_quad(p, n, c);

            // Perimeter skirt: cell edges lying on the CHUNK boundary drop a short apron so a
            // height mismatch against the neighbouring chunk's LOD state can't open a crack.
            const SKIRT: f32 = 1.1;
            let edges: [(usize, usize, bool); 4] = [
                (0, 1, cz == iz0),
                (1, 2, x1 == ix1),
                (2, 3, z1 == iz1),
                (3, 0, cx == ix0),
            ];
            for (a, b, on_boundary) in edges {
                if !on_boundary {
                    continue;
                }
                let dark = |col: [f32; 4]| [col[0] * 0.72, col[1] * 0.70, col[2] * 0.68, col[3]];
                push_quad(
                    [
                        p[a],
                        p[b],
                        [p[b][0], p[b][1] - SKIRT, p[b][2]],
                        [p[a][0], p[a][1] - SKIRT, p[a][2]],
                    ],
                    [n[a], n[b], n[b], n[a]],
                    [c[a], c[b], dark(c[b]), dark(c[a])],
                );
            }
            cx += LOD_STRIDE;
        }
        cz += LOD_STRIDE;
    }

    if positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

/// Build one terrain CHUNK — the tiles in `[ix0,ix1) × [iz0,iz1)` that pass `keep`. Walls still
/// query global `tile_at` for neighbour heights, so a wall spanning a chunk seam is owned by the
/// higher tile and drawn exactly once (no gap, no overlap). `spawn_terrain_sheet` tiles the whole
/// island with these so each chunk gets its own AABB and frustum-culls independently.
fn build_terrain_chunk(keep: impl Fn(TB) -> bool, ix0: i32, ix1: i32, iz0: i32, iz1: i32) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let quad =
        |p: [[f32; 3]; 4], n: [f32; 3], c: [[f32; 4]; 4], idx: &mut Vec<u32>, pos: &mut Vec<[f32; 3]>, nrm: &mut Vec<[f32; 3]>, col: &mut Vec<[f32; 4]>| {
            let b = pos.len() as u32;
            for k in 0..4 {
                pos.push(p[k]);
                nrm.push(n);
                col.push(c[k]);
            }
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        };

    // Like `quad` but with a PER-VERTEX normal — lets the rounded lip shade smoothly (no
    // flat-facet crease) and, by giving a convex corner's two chamfers the SAME diagonal corner
    // normal, kills the diagonal fold artifact at terrace corners.
    let quadn =
        |p: [[f32; 3]; 4], n: [[f32; 3]; 4], c: [[f32; 4]; 4], idx: &mut Vec<u32>, pos: &mut Vec<[f32; 3]>, nrm: &mut Vec<[f32; 3]>, col: &mut Vec<[f32; 4]>| {
            let b = pos.len() as u32;
            for k in 0..4 {
                pos.push(p[k]);
                nrm.push(n[k]);
                col.push(c[k]);
            }
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        };

    let nrm3 = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-4);
        [v[0] / l, v[1] / l, v[2] / l]
    };

    // Direction pointing DOWNHILL at world `(wx, wz)` — a pure function of position, so every
    // wall piece touching a shared corner column (including the PERPENDICULAR pair at a convex
    // cliff corner) computes the identical lean there and the displaced faces stay welded.
    let downhill = |wx: f32, wz: f32| -> (f32, f32) {
        let hcl = |dx: f32, dz: f32| -> f32 {
            tile_at((wx + GX + dx).floor() as i32, (wz + GZ + dz).floor() as i32)
                .map(|(_, h)| h as f32)
                .unwrap_or(0.0) // off-map / sea reads as low → coastal cliffs lean seaward
        };
        let d = 0.9;
        let ox = hcl(-d, 0.0) - hcl(d, 0.0);
        let oz = hcl(0.0, -d) - hcl(0.0, d);
        let l = (ox * ox + oz * oz).sqrt();
        if l < 1e-3 { (0.0, 0.0) } else { (ox / l, oz / l) }
    };

    // Horizontal displacement for a cliff-face vertex. `w` is the lip-relative depth weight
    // (0 at the top edge → 1 from ~1u down): zero at the lip keeps the face glued to the top
    // quad; below, a downhill talus lean (the base swells outward like real scree-footed rock)
    // plus two independent value-noise fields (sampled in a Y-sheared domain so every elevation
    // band bulges differently) break the dead-flat axis-aligned plane into a natural crag.
    // Everything is a pure function of world position + `w`, which shared columns agree on.
    let cliff_disp = |wx: f32, y: f32, wz: f32, w: f32| -> (f32, f32) {
        if w <= 0.001 {
            return (0.0, 0.0);
        }
        let (ox, oz) = downhill(wx, wz);
        let j1 = vnoise(wx * 1.6 + y * 1.1, wz * 1.6 - y * 0.8) - 0.5;
        let j2 = vnoise(wz * 1.7 + y * 0.9 + 37.0, wx * 1.7 + y * 1.3) - 0.5;
        (ox * 0.34 * w + j1 * 0.44 * w, oz * 0.34 * w + j2 * 0.44 * w)
    };

    // One tile-edge wall from the top edge (`ya`→`yb` over `pa`→`pb`) down to the lower shelf
    // (`ba`, `bb`). A low bank keeps the old single flat quad; a real cliff (mean drop ≥
    // CLIFF_MIN_DROP — the ≥2-class mesa/ridge walls) becomes a 3×N lattice of noise-displaced
    // flat-shaded facets. Vertex alpha carries a NEGATIVE "cliffness" so the terrain shader
    // paints procedural rock strata there (positive alpha stays the marsh-wetness lane).
    let cliff_wall = |pa: (f32, f32), pb: (f32, f32), ya: f32, yb: f32, ba: f32, bb: f32,
                      n: [f32; 3], ctop: [f32; 4], cbot: [f32; 4],
                      idx: &mut Vec<u32>, pos: &mut Vec<[f32; 3]>, nrm: &mut Vec<[f32; 3]>, col: &mut Vec<[f32; 4]>| {
        let ba = ba.min(ya);
        let bb = bb.min(yb);
        let drop = ((ya - ba) + (yb - bb)) * 0.5;
        if drop < CLIFF_MIN_DROP {
            let b = pos.len() as u32;
            for (p, c) in [([pa.0, ya, pa.1], ctop), ([pb.0, yb, pb.1], ctop), ([pb.0, bb, pb.1], cbot), ([pa.0, ba, pa.1], cbot)] {
                pos.push(p);
                nrm.push(n);
                col.push(c);
            }
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
            return;
        }
        let cliffk = ((drop - 0.40) / 0.90).clamp(0.0, 1.0);
        let bot_a = ba - CLIFF_SKIRT;
        let bot_b = bb - CLIFF_SKIRT;
        // Interior rows on fixed world-Y levels (see CLIFF_ROW doc) — welded shared columns.
        let lip_hi = ya.max(yb);
        let bot_lo = bot_a.min(bot_b);
        let k_hi = ((lip_hi - 0.10) / CLIFF_ROW).floor() as i32;
        let k_lo = ((bot_lo + 0.10) / CLIFF_ROW).ceil() as i32;
        let mut levels: Vec<f32> = Vec::new();
        let mut k = k_hi;
        while k >= k_lo && levels.len() < 14 {
            levels.push(k as f32 * CLIFF_ROW);
            k -= 1;
        }
        let nrows = levels.len() + 2;
        const NC: usize = 3; // facet columns per 1-tile wall
        let mut grid: Vec<[f32; 3]> = Vec::with_capacity((NC + 1) * nrows);
        let mut gcol: Vec<[f32; 4]> = Vec::with_capacity((NC + 1) * nrows);
        for ci in 0..=NC {
            let t = ci as f32 / NC as f32;
            let x0 = pa.0 + (pb.0 - pa.0) * t;
            let z0 = pa.1 + (pb.1 - pa.1) * t;
            let lip = ya + (yb - ya) * t;
            let bot = bot_a + (bot_b - bot_a) * t;
            for rj in 0..nrows {
                let y = if rj == 0 {
                    lip
                } else if rj == nrows - 1 {
                    bot
                } else {
                    levels[rj - 1].clamp(bot, lip)
                };
                let w = ((lip - y) / 1.1).clamp(0.0, 1.0);
                let (dx, dz) = cliff_disp(x0, y, z0, w);
                // Top→bottom tone gradient + a facet-scale value jitter so a tall face
                // reads as varied rock, not one flat colour ramp.
                let fr = ((lip - y) / (lip - bot).max(0.3)).clamp(0.0, 1.0);
                let tone = 0.85 + (vnoise(x0 * 1.2 + y * 0.8, z0 * 1.2 - y * 0.6) - 0.5) * 0.60;
                grid.push([x0 + dx, y, z0 + dz]);
                gcol.push([
                    (ctop[0] + (cbot[0] - ctop[0]) * fr) * tone,
                    (ctop[1] + (cbot[1] - ctop[1]) * fr) * tone,
                    (ctop[2] + (cbot[2] - ctop[2]) * fr) * tone,
                    // Cliffness fades in over the first ~0.5u below the lip so the rock
                    // albedo melts out of the top-surface colour instead of a hard seam.
                    -(cliffk * ((lip - y) / 0.5).clamp(0.0, 1.0)),
                ]);
            }
        }
        // Flat-shaded facet triangles — the crisp low-poly crag look (per-face normals from
        // the displaced geometry, matching the outward winding of the old wall quad).
        for ci in 0..NC {
            for rj in 0..nrows - 1 {
                let ia = ci * nrows + rj;
                let ib = (ci + 1) * nrows + rj;
                let ic = (ci + 1) * nrows + rj + 1;
                let id = ci * nrows + rj + 1;
                for tri in [[ia, ib, ic], [ia, ic, id]] {
                    let p0 = grid[tri[0]];
                    let p1 = grid[tri[1]];
                    let p2 = grid[tri[2]];
                    let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
                    let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
                    let cr = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                    let l = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt();
                    if l < 1e-6 {
                        continue; // clamped row → degenerate sliver
                    }
                    let fnrm = [cr[0] / l, cr[1] / l, cr[2] / l];
                    let b = pos.len() as u32;
                    for &vi in &tri {
                        pos.push(grid[vi]);
                        nrm.push(fnrm);
                        col.push(gcol[vi]);
                    }
                    idx.extend_from_slice(&[b, b + 1, b + 2]);
                }
            }
        }
    };

    const NB: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

    for iz in iz0..iz1 {
        for ix in ix0..ix1 {
            // River signed-distance at the four corners (a CORNER field; `< 0` = under the river).
            // Bank tiles are cut by MARCHING SQUARES along the `= 0` contour, so the shoreline is a
            // smooth sub-grid curve rather than the tile-grid staircase ("Minecraft squares").
            let cwat = [
                corner_water(ix, iz),
                corner_water(ix + 1, iz),
                corner_water(ix + 1, iz + 1),
                corner_water(ix, iz + 1),
            ];
            let has_river = cwat.iter().any(|c| c.1);
            // Biome of this cell (land tile here, or nearest land neighbour) — drives colour, skirt
            // tone, the material-sheet routing AND the low-coast smooth-shore decision below.
            let here = tile_at(ix, iz);
            let coast_biome = here.or_else(|| NB.iter().find_map(|&(dx, dz)| tile_at(ix + dx, iz + dz)));
            // LOW sea-coast — a flat (h≤1) tile meeting the OPEN SEA — gets the smooth
            // marching-squares shore the rivers already use, instead of the per-tile square skirt
            // that stepped the boundary into visible tile "kwadraty" (player report). Two cases:
            //   • Swamp / Blight mud flats (the original de-squaring), any coast; and
            //   • the NW snow-massif headland fringe (`nw_headland > 0`) — its low class-1 apron
            //     that runs down to the water read as a blocky staircase (player: same "kwadraty"
            //     on the new snow coast). Its TALL flanks (h≥2) still get the faceted cliff face;
            //     only the lowest lip is smoothed here.
            // Gated to h≤1 so deliberate sheer seaward MOUNTAIN cliffs (coast_hill / mesa, h≥2)
            // keep their per-tile face, and skipped at river mouths (already cut clean).
            //   • the whole ARENA coast — the skirmish island is ringed by a class-1 sand
            //     foreshore, and with no smooth cut its waterline was a pure tile-grid sawtooth
            //     (player report: the arena coast read as blocky teeth). It has no tall seaward
            //     cliffs anywhere (`arena_shore_damp` fades every massif flat before the sand), so
            //     the whole coast qualifies and there's nothing here for the h≤1 gate to protect.
            let low_coast = matches!(coast_biome, Some((TB::Swamp | TB::Blight, h)) if h <= 1)
                || matches!(coast_biome, Some((_, h)) if h <= 1
                    && (is_arena()
                        || nw_headland((ix as f32 + 0.5) / MAP_SCALE, (iz as f32 + 0.5) / MAP_SCALE) > 0.0));
            let low_wet_coast = !has_river && low_coast && cwat.iter().any(|c| c.0 < 0.0); // touches sea
            // Corner water field: the real river/lake SDF, OR — on a low wetland coast — a smooth SEA
            // SDF so the contour follows the true shoreline instead of snapping to the tile grid
            // (`corner_water` hands the open sea a flat constant, which is what staircased it).
            let rs = if low_wet_coast {
                let sf = |cx: i32, cz: i32| sea_field(cx as f32 / MAP_SCALE, cz as f32 / MAP_SCALE);
                [sf(ix, iz), sf(ix + 1, iz), sf(ix + 1, iz + 1), sf(ix, iz + 1)]
            } else {
                [cwat[0].0, cwat[1].0, cwat[2].0, cwat[3].0]
            };
            let wetn = rs.iter().filter(|&&s| s < 0.0).count();
            // A saddle (two DIAGONALLY-opposite wet corners) is ambiguous to contour; treat it as
            // solid land — a sub-tile river pinch over-extends land a hair, but it never spikes.
            let saddle = wetn == 2 && (rs[0] < 0.0) == (rs[2] < 0.0);
            // River cells (incl. the mouth, where sea corners also read as water) OR a low wetland
            // sea-coast get the smooth contour cut; a plain coast tile keeps the per-tile beach skirt.
            let cut = (1..=3).contains(&wetn) && !saddle && (has_river || low_wet_coast);

            // Render this cell iff it has land to show: a real land tile here, OR a water cell whose
            // DRY corners form the far bank.
            if wetn == 4 || (here.is_none() && !cut) {
                continue; // fully under the water with no bank here
            }
            let (tb, h) = coast_biome.unwrap_or((TB::Grass, 1));
            if !keep(tb) {
                continue;
            }
            let flat = (h - 1) as f32 * GROUND_STEP;

            // Smoothed corner heights (cluster mean at each corner, as seen from THIS tile's
            // class) → continuous slopes on terraced ground; across a mesa tier wall the
            // clusters split and the gap gets a vertical cliff quad below.
            let cy = [
                corner_top_y_for(ix, iz, h).unwrap_or(flat),
                corner_top_y_for(ix + 1, iz, h).unwrap_or(flat),
                corner_top_y_for(ix + 1, iz + 1, h).unwrap_or(flat),
                corner_top_y_for(ix, iz + 1, h).unwrap_or(flat),
            ];
            let cxz = [
                (ix as f32 - GX, iz as f32 - GZ),
                (ix as f32 + 1.0 - GX, iz as f32 - GZ),
                (ix as f32 + 1.0 - GX, iz as f32 + 1.0 - GZ),
                (ix as f32 - GX, iz as f32 + 1.0 - GZ),
            ];
            let cn = |cx: i32, cz: i32| {
                let e = corner_top_y_for(cx + 1, cz, h).unwrap_or(flat);
                let w = corner_top_y_for(cx - 1, cz, h).unwrap_or(flat);
                let s = corner_top_y_for(cx, cz + 1, h).unwrap_or(flat);
                let n = corner_top_y_for(cx, cz - 1, h).unwrap_or(flat);
                nrm3([w - e, 2.0, n - s])
            };
            let cnn = [cn(ix, iz), cn(ix + 1, iz), cn(ix + 1, iz + 1), cn(ix, iz + 1)];
            let col_at = |i: usize| ground_color((cxz[i].0 + GX) / MAP_SCALE, (cxz[i].1 + GZ) / MAP_SCALE);

            // Bank wall tone: grass shows exposed dirt; snow a cliff lip; else a darkened top.
            // Alpha 0.0 (dry), NOT 1.0 — the shader reads vertex alpha as marsh WETNESS, so the
            // old 1.0 gave every terrace wall a full wet-sheen roughness (plastic-shiny cliffs).
            let top_col = ground_color((ix as f32 + 0.5) / MAP_SCALE, (iz as f32 + 0.5) / MAP_SCALE);
            let (wall_top, wall_bot) = if tb == TB::Grass {
                let j = 0.82 + 0.32 * (noise_b(ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE) * 0.5 + 0.5);
                let d = lin3(active_map().palette.dirt);
                ([d[0] * j, d[1] * j, d[2] * j, 0.0], [d[0] * j * 0.68, d[1] * j * 0.64, d[2] * j * 0.60, 0.0])
            } else if tb == TB::Snow {
                let lip = lin3(active_map().palette.snow_cliff_lip);
                let rock = lin3(active_map().palette.snow_cliff_rock);
                ([lip[0], lip[1], lip[2], 0.0], [rock[0], rock[1], rock[2], 0.0])
            } else {
                ([top_col[0] * 0.80, top_col[1] * 0.78, top_col[2] * 0.76, 0.0], [top_col[0] * 0.58, top_col[1] * 0.56, top_col[2] * 0.54, 0.0])
            };

            if !cut {
                // Full land tile. Skirts drop to the sea ONLY toward non-river water (coast / lake /
                // off-map); a river edge here is owned by the marching-squares neighbour, so skip it.
                quadn(
                    [[cxz[0].0, cy[0], cxz[0].1], [cxz[1].0, cy[1], cxz[1].1], [cxz[2].0, cy[2], cxz[2].1], [cxz[3].0, cy[3], cxz[3].1]],
                    cnn,
                    [col_at(0), col_at(1), col_at(2), col_at(3)],
                    &mut indices, &mut positions, &mut normals, &mut colors,
                );
                for (dx, dz) in NB {
                    // Edge corner indices for this side (CCW corner order 0,1,2,3).
                    let (a, b, n): (usize, usize, [f32; 3]) = match (dx, dz) {
                        (1, 0) => (1, 2, [1.0, 0.0, 0.0]),
                        (-1, 0) => (3, 0, [-1.0, 0.0, 0.0]),
                        (0, 1) => (2, 3, [0.0, 0.0, 1.0]),
                        _ => (0, 1, [0.0, 0.0, -1.0]),
                    };
                    if let Some((_, hn)) = tile_at(ix + dx, iz + dz) {
                        // Land neighbour: on terraced ground the shared cluster corners meet and
                        // no skirt is needed. Across a mesa tier wall the corner clusters SPLIT
                        // (see `corner_top_y_for`) — the higher shelf owns the vertical cliff
                        // face down to the lower shelf's edge.
                        let cg = |i: usize| match i {
                            0 => (ix, iz),
                            1 => (ix + 1, iz),
                            2 => (ix + 1, iz + 1),
                            _ => (ix, iz + 1),
                        };
                        let nflat = (hn - 1) as f32 * GROUND_STEP;
                        let (ga, gb) = (cg(a), cg(b));
                        let na_y = corner_top_y_for(ga.0, ga.1, hn).unwrap_or(nflat);
                        let nb_y = corner_top_y_for(gb.0, gb.1, hn).unwrap_or(nflat);
                        if cy[a] > na_y + 0.01 || cy[b] > nb_y + 0.01 {
                            cliff_wall(
                                cxz[a], cxz[b], cy[a], cy[b], na_y, nb_y,
                                n, wall_top, wall_bot,
                                &mut indices, &mut positions, &mut normals, &mut colors,
                            );
                        }
                        continue;
                    }
                    if is_river((ix + dx) as f32 / MAP_SCALE, (iz + dz) as f32 / MAP_SCALE) {
                        continue; // river edge → the marching-squares cell owns this bank
                    }
                    cliff_wall(
                        cxz[a], cxz[b], cy[a], cy[b], SEA_Y, SEA_Y,
                        n, wall_top, wall_bot,
                        &mut indices, &mut positions, &mut normals, &mut colors,
                    );
                }
            } else {
                // MARCHING SQUARES: the land-side polygon (rs >= 0), clipped to the contour, plus a
                // bank wall along the cut edge. Edge crossings are interpolated to the exact `= 0`
                // point, so the bank follows the river's smooth curve.
                let mut pp: Vec<[f32; 3]> = Vec::new();
                let mut pn: Vec<[f32; 3]> = Vec::new();
                let mut pc: Vec<[f32; 4]> = Vec::new();
                let mut pcross: Vec<bool> = Vec::new();
                for a in 0..4 {
                    let b = (a + 1) % 4;
                    if rs[a] >= 0.0 {
                        pp.push([cxz[a].0, cy[a], cxz[a].1]);
                        pn.push(cnn[a]);
                        pc.push(col_at(a));
                        pcross.push(false);
                    }
                    if (rs[a] >= 0.0) != (rs[b] >= 0.0) {
                        let t = rs[a] / (rs[a] - rs[b]); // zero-crossing fraction along edge a→b
                        let x = cxz[a].0 + (cxz[b].0 - cxz[a].0) * t;
                        let z = cxz[a].1 + (cxz[b].1 - cxz[a].1) * t;
                        // The water-edge vertex sits at WATER LEVEL, not the land height. So the bank
                        // tile slopes from its dry corners (full land height) DOWN to the waterline —
                        // a gradual shore, not a vertical lip cut off like a ruler. (Adjacent cells
                        // interpolate the same crossing on a shared edge, so the slope stays welded.)
                        let y = SEA_Y + 0.06;
                        let nrm = nrm3([
                            cnn[a][0] + (cnn[b][0] - cnn[a][0]) * t,
                            cnn[a][1] + (cnn[b][1] - cnn[a][1]) * t,
                            cnn[a][2] + (cnn[b][2] - cnn[a][2]) * t,
                        ]);
                        pp.push([x, y, z]);
                        pn.push(nrm);
                        pc.push(ground_color((x + GX) / MAP_SCALE, (z + GZ) / MAP_SCALE));
                        pcross.push(true);
                    }
                }
                // Top: triangle-fan from vertex 0 (same CCW winding as the full quad → faces up).
                if pp.len() >= 3 {
                    let base = positions.len() as u32;
                    for k in 0..pp.len() {
                        positions.push(pp[k]);
                        normals.push(pn[k]);
                        colors.push(pc[k]);
                    }
                    for k in 1..pp.len() - 1 {
                        indices.extend_from_slice(&[base, base + k as u32, base + k as u32 + 1]);
                    }
                }
                // Bank wall along the cut edge (the consecutive pair of contour crossings). Normal
                // points toward the water (the wet-corner centroid).
                let (mut wcx, mut wcz, mut wnn) = (0.0_f32, 0.0_f32, 0.0_f32);
                for a in 0..4 {
                    if rs[a] < 0.0 {
                        wcx += cxz[a].0;
                        wcz += cxz[a].1;
                        wnn += 1.0;
                    }
                }
                wcx /= wnn.max(1.0);
                wcz /= wnn.max(1.0);
                let n = pp.len();
                for i in 0..n {
                    let j = (i + 1) % n;
                    if pcross[i] && pcross[j] {
                        let (x0, z0) = (pp[i][0], pp[i][2]);
                        let (x1, z1) = (pp[j][0], pp[j][2]);
                        let (mut nx, mut nz) = (wcx - (x0 + x1) * 0.5, wcz - (z0 + z1) * 0.5);
                        let nl = (nx * nx + nz * nz).sqrt().max(1e-3);
                        nx /= nl;
                        nz /= nl;
                        quad(
                            [[x0, pp[i][1], z0], [x1, pp[j][1], z1], [x1, SEA_Y, z1], [x0, SEA_Y, z0]],
                            [nx, 0.0, nz],
                            [wall_top, wall_top, wall_bot, wall_bot],
                            &mut indices, &mut positions, &mut normals, &mut colors,
                        );
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pair of adjacent LAND tiles outside the coastal ridge band must differ by ≤1
    /// height class. NPC movement (nav-grid `can_step`, local `steer::can_stand`) refuses any
    /// >1-class step, so a 2+-class inland cliff wedges wandering NPCs at its base. Regression
    /// for "NPCs stuck at the mountain near the castle" — guarded by `terrace_inland`.
    #[test]
    fn inland_terrain_is_climbable() {
        let skip = |ix: i32, iz: i32| {
            let (bx, bz) = (ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE);
            // Seaward coastal cliffs and the cliffy mesa regions are DELIBERATELY sheer; the
            // mesas' walkability is guaranteed by their pass corridors instead
            // (`mesa_passes_climb_to_the_summit`).
            dist_from_coast(bx, bz) <= 7 || cliff_exempt_base(bx, bz)
        };
        let mut cliffs: Vec<((i32, i32), (i32, i32), i32, i32)> = Vec::new();
        for iz in 0..ROWS {
            for ix in 0..COLS {
                let Some((_, h)) = tile_at(ix, iz) else { continue };
                // Only +x / +z neighbours so each edge is counted once.
                for (dx, dz) in [(1, 0), (0, 1)] {
                    let (nx, nz) = (ix + dx, iz + dz);
                    let Some((_, nh)) = tile_at(nx, nz) else { continue };
                    if skip(ix, iz) || skip(nx, nz) {
                        continue;
                    }
                    if (h - nh).abs() >= 2 {
                        cliffs.push(((ix, iz), (nx, nz), h, nh));
                    }
                }
            }
        }
        assert!(
            cliffs.is_empty(),
            "{} inland 2+-class cliffs (unclimbable for NPCs), e.g. {:?}",
            cliffs.len(),
            &cliffs[..cliffs.len().min(8)]
        );
    }

    /// Every authored pass corridor must be a continuous ≤1-class staircase from open ground at
    /// the mouth to the region's summit class — the corridors are the ONLY guaranteed ways up a
    /// cliffy mesa, so a single sheer step in one walls off the whole range for NPCs + the hero.
    #[test]
    fn mesa_passes_climb_to_the_summit() {
        for reg in MAP_HOME.regions.iter().filter(|r| r.cliffy && r.peak > 0) {
            for (pi, p) in reg.passes.iter().enumerate() {
                // Sample the corridor centreline FINELY (¼-tile steps) from just outside the
                // mouth to the centre; consecutive distinct grid tiles are then 8-neighbours,
                // matching the nav-grid's 8-direction steps. (A greedy axis-walk is WRONG here —
                // it fronts-loads all the dominant-axis steps and leaves the corridor.)
                let to_grid = |dc: f32| {
                    // The corridor centreline serpentines (`pass_sway`) — walk the same curve.
                    let a = p.ang + pass_sway(dc, reg, p);
                    let bx = reg.x + a.cos() * dc;
                    let bz = reg.z + a.sin() * dc;
                    ((bx * MAP_SCALE) as i32, (bz * MAP_SCALE) as i32)
                };
                let start_dc = reg.r + 2.0;
                let steps = (start_dc * MAP_SCALE * 4.0) as i32;
                let (mut last, mut prev) = {
                    let g = to_grid(start_dc);
                    (g, tile_at(g.0, g.1).map(|(_, h)| h).unwrap_or(1))
                };
                let mut max_seen = prev;
                let mut worst = (0, (0, 0), 0, 0);
                for s in 1..=steps {
                    let dc = start_dc * (1.0 - s as f32 / steps as f32);
                    let (cx, cz) = to_grid(dc);
                    if (cx, cz) == last {
                        continue;
                    }
                    last = (cx, cz);
                    let Some((_, h)) = tile_at(cx, cz) else {
                        panic!("pass {pi} of {:?} region crosses non-land at grid ({cx},{cz})", reg.biome)
                    };
                    if (h - prev).abs() > worst.0 {
                        worst = ((h - prev).abs(), (cx, cz), prev, h);
                    }
                    max_seen = max_seen.max(h);
                    prev = h;
                }
                assert!(
                    worst.0 <= 1,
                    "pass {pi} of {:?} region has a {}-class step ({}→{}) at grid {:?} (base {:.1},{:.1})",
                    reg.biome,
                    worst.0,
                    worst.2,
                    worst.3,
                    worst.1,
                    worst.1.0 as f32 / MAP_SCALE,
                    worst.1.1 as f32 / MAP_SCALE
                );
                assert!(
                    max_seen >= reg.peak - 1,
                    "pass {pi} of {:?} region tops out at class {max_seen} (peak {})",
                    reg.biome,
                    reg.peak
                );
            }
        }
    }

    /// The bog pools must actually EXIST and reach the depths the bog dressing gates on —
    /// regression for two silent failures where a noise-thresholded pool field never fired
    /// inside the swamp regions (0 pools → 0 drowned trees/wisps/tower, unnoticed until a
    /// visual round). Checks the REAL tile grid (post-carve) + the sd field the dressing uses.
    #[test]
    fn swamp_pools_exist_and_run_deep() {
        let mut water_tiles = 0;
        let mut deepest = f32::INFINITY;
        for iz in 0..ROWS {
            for ix in 0..COLS {
                let (bx, bz) = (ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE);
                if tile_at(ix, iz).is_none() && is_pool(bx, bz) {
                    water_tiles += 1;
                }
                let sd = pool_sd(bx, bz);
                if sd < deepest {
                    deepest = sd;
                }
            }
        }
        assert!(
            water_tiles > 400,
            "expected broad carved bog pools, got {water_tiles} pool water tiles"
        );
        assert!(
            deepest < -1.5,
            "deepest pool sd {deepest} — bog dressing gates (trees −0.9, tower −1.1) unreachable"
        );
    }

    /// The mesa summits must be reachable from the castle by the real nav-grid A* — the
    /// end-to-end guarantee that the pass corridors + bridges connect: miners, wardens' hunters
    /// and the hero all route with `can_step`, so an unreachable summit means broken content.
    #[test]
    fn mesa_summits_reachable_from_castle() {
        for reg in MAP_HOME.regions.iter().filter(|r| r.cliffy && r.peak > 0) {
            let summit = Vec2::new(reg.x * MAP_SCALE - GX, reg.z * MAP_SCALE - GZ);
            let path = crate::navgrid::path_to_budget(Vec2::new(0.0, -8.0), summit, 120_000);
            assert!(
                !path.is_empty(),
                "no nav-grid route castle → {:?} summit at {summit:?}",
                reg.biome
            );
        }
    }
}

