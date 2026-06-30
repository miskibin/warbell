//! The **world map** — a Bevy port of the TS game's island (`src/world/tileMap.ts`),
//! generated in the original BASE space (144×108): an elliptical island with a noisy
//! coast, five biome blobs (snow NW, desert NE, rock E, forest SW, swamp S), a grass
//! centre safe-zone (the castle spot), a grass frontier with scattered forest clumps and
//! rolling terraced knolls, a beach ring backed by patchy coastal mountain ridges, four
//! carved rivers + one lake, and **terraced** stepped heights (flat tile-tops + cliff
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
/// the desert rival, jammed against the north coast) more room. The two world-coord-authored
/// landmarks scale with it automatically: `ork_fortress::BLIGHT_DZ` and `rival::RIVAL_CENTRE` are
/// both derived from `MAP_SCALE`, so bumping it keeps the ork gate on the south coast and the
/// rival fort in the desert with no hand-tuning.
pub const MAP_SCALE: f32 = 2.0;
// The GRID is the enlarged resolution; GENERATION still runs in *base* space — the grid
// loop samples `classify(ix / MAP_SCALE, …)`, so the island shape is identical, just
// drawn over more tiles. `CX/CZ` stay the BASE centre used by all the generation math;
// `GX/GZ` are the GRID centre used for world placement + tile-cache indexing. All four
// derive from MAP_SCALE so bumping the scale stays self-consistent.
const BASE_COLS: f32 = 144.0; // original (pre-enlargement) grid width
const BASE_ROWS: f32 = 164.0; // 108 original island rows + 56 southern Blight extension
pub const COLS: i32 = (BASE_COLS * MAP_SCALE) as i32; // 259 at 1.8
/// Rows: the original island (108 base rows) PLUS the southern **Blight** extension —
/// the walkable ork-fortress landmass (`ork_fortress::blight_class_base`). Everything
/// north of the old south edge is the original map, just denser; the Blight grows south.
pub const ROWS: i32 = (BASE_ROWS * MAP_SCALE) as i32; // 295 at 1.8
const CX: f32 = BASE_COLS / 2.0; // 72 — base generation centre
const CZ: f32 = 54.0; // original island half-height (108/2)
/// Grid centre (enlarged) — world placement recentres the map onto the origin here.
pub const GX: f32 = COLS as f32 / 2.0;
/// NOT `ROWS/2`: the castle stays at the origin (= island centre), so GZ is pinned to the
/// ORIGINAL island's half-height scaled, and the Blight extension grows `ROWS` southward only.
pub const GZ: f32 = CZ * MAP_SCALE; // 97.2 at 1.8
const ISLAND_RX: f32 = 71.0;
const ISLAND_RZ: f32 = 53.0;
const ISLAND_EXP: f32 = 2.6;
// Castle safe-zone radius in BASE space (forced flat grass, no biome scatter). `classify` runs in
// base space, so the WORLD radius is `SAFE_R * MAP_SCALE` (≈32.4 at MAP_SCALE 2.0). Trimmed from
// 18.0 — bumping MAP_SCALE 1.8→2.0 silently grew the *world* safe-zone ~11% (this is base-space, so
// it scales with the map); 16.2 restores the pre-bump world size so the biome-free ring round the
// castle isn't oversized. Town build plots flatten themselves (`near_build_plot`), so this doesn't
// gate their footing.
pub const SAFE_R: f32 = 16.2;
pub const GROUND_STEP: f32 = 0.5; // world-Y per height class
const SEA_Y: f32 = -0.4;
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
// `road_dirt` the baked approach-road tint; `lava_*` the volcanic-map magma field (map 2).
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
    // ── Lava field (map 2 only — see `TB::Lava`) ──
    lava_basalt: u32,
    lava_seam: u32,
    lava_seam_hot: u32,
}

/// The home island's original palette — values unchanged from the pre-second-map era.
const PAL_HOME: Palette = Palette {
    grass: 0x6fb24c,
    grass_dark: 0x4f8c38,
    grass_dry: 0x8fa953,
    grass_gold: 0xa8b048,
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
    lava_basalt: 0x2a2421,
    lava_seam: 0xc8521a,
    lava_seam_hot: 0xffb347,
};

/// **Volcanic Ashlands** (map 2) — the 5 biomes reskinned for a charred volcanic world, but each
/// kept VISUALLY DISTINCT so a biome still reads as itself under the ember sun (an earlier muddy
/// pass made snow read tan + forest read flat-brown). Snow stays COOL (blue-grey ash) so it never
/// warms to desert; forest keeps a sooty GREEN (g>r) so the terrain shader's foliage layer fires
/// and it reads as scorched woodland, not mud; desert is clearly warm sand; rock is near-black
/// basalt; swamp a sulfur olive.
const PAL_ASH: Palette = Palette {
    grass: 0x575249,       // cinder flats (the neutral "safe" ground)
    grass_dark: 0x3c3931,  // shaded ash hollows
    grass_dry: 0x6f6453,   // dry warm cinder
    grass_gold: 0x8c6838,  // scorched sun-baked sweeps
    sand: 0x837458,        // grey-tan volcanic grit beaches
    forest: 0x424e36,      // sooty GREEN grove floor (g>r → reads as woodland)
    rock: 0x312c27,        // near-black basalt range
    snow: 0xe2e7f0,        // BRIGHT cool ashfall — value (not hue) is what reads as snow vs tan
    desert: 0x9c8a63,      // warm tan dunes (clearly sandy)
    swamp: 0x4a4226,       // sulfur-tainted muck
    swamp_dark: 0x2a2615,  // tar pools
    swamp_moss: 0x726232,  // sulfur crust
    swamp_algae: 0x5c4c22, // brimstone seep
    blight: 0x3a2e1f,      // trampled ash mud
    blight_dark: 0x1f1810,
    blight_ash: 0x6a6258,
    blight_green: 0x55502f,
    blight_rust: 0x6a3318,
    snow_shade: 0xa2aab8,  // cool drift shadow
    snow_bright: 0xdfe6ee, // wind-polished cool ash crest
    forest_dark: 0x313b27, // charred green loam
    forest_dry: 0x586338,  // ash-dusted leaf litter (greenish)
    dirt: 0x3a2c1d,        // cliff soil
    snow_cliff_lip: 0xd0d8e2,
    snow_cliff_rock: 0x565b64,
    road_dirt: 0x4a3a26,
    lava_basalt: 0x2a2421, // cooled crust between the seams
    lava_seam: 0xc8521a,   // glowing crack
    lava_seam_hot: 0xffb347, // white-hot core
};

#[derive(Clone, Copy, PartialEq, Eq)]
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
    /// **Lava field** — the Volcanic Ashlands signature biome (map 2 only). Like the Blight it
    /// draws its own ground (basalt + glowing magma seams) but maps to [`Biome::Swamp`] for
    /// gameplay, so standing in it burns (the swamp poison-and-slow floor, reskinned).
    Lava,
}

struct Region {
    x: f32,
    z: f32,
    r: f32,
    biome: TB,
    /// centre height class for mountain biomes (0 = flat biome).
    peak: i32,
}

const REGIONS: [Region; 6] = [
    Region { x: 26.0, z: 24.0, r: 31.0, biome: TB::Snow, peak: 10 }, // NW snow massif
    // NE dunes — shifted NW (112,28 → 101,10), same radius, so the dune field grows toward the snow
    // massif and fills the wide grass corridor that ran along the top coast (north reach now hits
    // the beach). It keeps a natural grass seam between snow and desert (~13 base at z24) instead of
    // swallowing it. Biased NORTH on purpose: the castle sits at base z54, so a blob centred lower
    // would bulge its southern arc onto the keep — at z10 the dunes bottom out near z28 (x72),
    // leaving the castle safe-ring (south edge ~z36) on clean grass.
    Region { x: 101.0, z: 10.0, r: 34.0, biome: TB::Desert, peak: 0 },
    // E rock range — pulled in toward the castle (122→116: the mine country starts just past
    // the safe-zone fray instead of a 33-unit trek). Raised to a real MOUNTAIN height (the tallest
    // biome by far) — `terrace_inland` relaxes the inland faces to ≤1-class steps so the nav-grid
    // still climbs the taller bulk; the seaward coastal band stays sheer, reading as cliff peaks.
    Region { x: 116.0, z: 57.0, r: 34.0, biome: TB::Rock, peak: 18 },
    Region { x: 32.0, z: 80.0, r: 34.0, biome: TB::Forest, peak: 0 }, // SW forest
    Region { x: 72.0, z: 92.0, r: 26.0, biome: TB::Swamp, peak: 0 }, // S swamp
    // Eastern marsh arm: fills the open grass strip that ran between the S swamp and the
    // rocky mine range (world x +30…+66, z +15…+60), so the marsh laps right up to the
    // foot of the mines instead of leaving a grass corridor. Rock's mountain priority in
    // `region_at` auto-clips the east edge (the mines stay rock), and the castle safe-ring
    // forces grass at the centre, so this only eats the in-between grass.
    Region { x: 102.0, z: 78.0, r: 22.0, biome: TB::Swamp, peak: 0 },
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
/// **Volcanic Ashlands** atmosphere (map 2): a dim red-brown sky + orange fog carry the hellish
/// mood, while the sun is only *warm* (not pure-orange) so biome hues still read — a fully
/// saturated ember sun washed snow to tan and erased every biome's colour. Low sun = long hostile
/// shadows. Same tuple shape as [`ATMOSPHERE`]; fog stays moderate (heavy volumetric fog blacks
/// the Atmosphere sky — see the ultra-graphics note).
const ASH_ATMOSPHERE: (u32, f32, u32, f32, u32, f32, Vec3) =
    (0x5a3b32, 0.0075, 0xffdca8, 10_500.0, 0x6a5e54, 145.0, Vec3::new(90.0, 64.0, 30.0));

/// Ashlands biome layout — the same five biome kinds (reskinned by [`PAL_ASH`]) placed in a
/// deliberately DIFFERENT arrangement from the home island, so the world reads as a new place
/// even before the palette/atmosphere land. Mirror was dropped (it desyncs colour-vs-class +
/// bridges/town-plots); bespoke positions + the noise reseed do the layout work instead.
const ASH_REGIONS: [Region; 6] = [
    Region { x: 24.0, z: 54.0, r: 32.0, biome: TB::Rock, peak: 18 }, // W charcoal massif (home put it E)
    Region { x: 112.0, z: 26.0, r: 28.0, biome: TB::Snow, peak: 9 }, // NE ashfall drifts
    Region { x: 110.0, z: 82.0, r: 30.0, biome: TB::Desert, peak: 0 }, // SE bleached dunes
    Region { x: 58.0, z: 92.0, r: 26.0, biome: TB::Forest, peak: 0 }, // S burnt grove
    Region { x: 96.0, z: 48.0, r: 18.0, biome: TB::Lava, peak: 0 }, // E lava field (the signature biome)
    Region { x: 70.0, z: 14.0, r: 20.0, biome: TB::Swamp, peak: 0 }, // N sulfur seep
];

/// Which world a run generates. `u8` on the wire (0 = Home, default) so it rides the save +
/// the boot global trivially. Add a variant + a [`MapDef`] to ship a third map.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MapId {
    #[default]
    Home = 0,
    Ashlands = 1,
}

/// The chosen map, as a Bevy resource (start-screen owns it; default Home). `biome::apply_build`
/// reads it and calls [`set_active_map`] before [`build`], so the pure generation functions —
/// which can't read a resource — see the right map.
#[derive(Resource, Clone, Copy, Default)]
pub struct ActiveMap(pub MapId);

/// Everything that differs between maps. All other generation (island ellipse, rivers, lake,
/// plateaus, coast ridges) is shared and merely perturbed by `noise_phase`.
struct MapDef {
    regions: &'static [Region],
    palette: Palette,
    atmosphere: (u32, f32, u32, f32, u32, f32, Vec3),
    /// Added inside `noise_a`/`noise_b` → reseeds the coastline fray, river winding and inland
    /// hills. Home is `0.0` (identical output — the regression anchor).
    noise_phase: f32,
    has_lava: bool,
}

const MAP_HOME: MapDef = MapDef {
    regions: &REGIONS,
    palette: PAL_HOME,
    atmosphere: ATMOSPHERE,
    noise_phase: 0.0,
    has_lava: false,
};
const MAP_ASH: MapDef = MapDef {
    regions: &ASH_REGIONS,
    palette: PAL_ASH,
    atmosphere: ASH_ATMOSPHERE,
    noise_phase: 13.37,
    has_lava: true,
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
            1 => MapId::Ashlands,
            _ => MapId::Home,
        }
    }
}
fn active_map() -> &'static MapDef {
    match active_id() {
        1 => &MAP_ASH,
        _ => &MAP_HOME,
    }
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

fn is_land_shape(x: f32, z: f32) -> bool {
    let dx = (x - CX).abs() / ISLAND_RX;
    let dz = (z - CZ).abs() / ISLAND_RZ;
    let r = dx.powf(ISLAND_EXP) + dz.powf(ISLAND_EXP);
    let coast = noise_a(x, z) * 0.08;
    r + coast < 1.0
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

fn river_x(z: f32) -> f32 {
    40.0 + (z * 0.18).sin() * 5.0 + (z * 0.07 + 1.4).sin() * 3.0
}
fn river_z(x: f32) -> f32 {
    20.0 + (x * 0.13 + 0.7).sin() * 4.0
}
/// Southern stream: rises near the castle safe-zone and winds south through the forest to the
/// coast (the safe-zone test in `is_river` clips its head).
fn river_x2(z: f32) -> f32 {
    56.0 + (z * 0.15 + 2.0).sin() * 4.0 + (z * 0.06).sin() * 2.5
}
/// Southern crossways river: spans the island below the castle through forest + swamp.
fn river_z2(x: f32) -> f32 {
    86.0 + (x * 0.11 + 1.0).sin() * 4.0
}
fn in_mountain(x: f32, z: f32) -> bool {
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    active_map().regions.iter().any(|r| r.peak > 0 && (x - r.x).hypot(z - r.z) + wob < r.r + 2.0)
}
fn is_river(x: f32, z: f32) -> bool {
    if dist_from_castle(x, z) < SAFE_R {
        return false;
    }
    if in_mountain(x, z) {
        return false;
    }
    let cx = river_x(z);
    let w = 0.75 + (z * 0.5).sin() * 0.2;
    if (x - cx).abs() < w {
        return true;
    }
    // Northern crossways branch (base-space bound — the old `COLS - 10` was grid-space and
    // never clipped anything; the coast does the clipping anyway).
    if x > 46.0 && x < 134.0 {
        let cz = river_z(x);
        if (z - cz).abs() < 0.7 {
            return true;
        }
    }
    // Southern stream toward the south coast.
    if z > 58.0 && (x - river_x2(z)).abs() < 0.7 {
        return true;
    }
    // Southern crossways river through forest + swamp.
    if x > 14.0 && x < 126.0 && (z - river_z2(x)).abs() < 0.75 {
        return true;
    }
    false
}
fn is_lake(x: f32, z: f32) -> bool {
    let dx = (x - DELIBERATE_LAKE.0) / DELIBERATE_LAKE.2;
    let dz = (z - DELIBERATE_LAKE.1) / DELIBERATE_LAKE.3;
    dx * dx + dz * dz < 1.0
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
    let roll = noise_a(x * 0.3 + 9.0, z * 0.3 - 5.0) + noise_b(x * 0.16 - 4.0, z * 0.16 + 8.0) * 0.6;
    let h = if roll > 1.9 {
        4
    } else if roll > 1.45 {
        3
    } else if roll > 0.95 {
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

fn classify(x: f32, z: f32) -> Option<(TB, i32)> {
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
    let ph = plateau_height(x, z);
    if ph > 0 {
        // A plateau inside a flat biome blob keeps that biome (desert mesa, forest highland);
        // out on the frontier it's a grass plateau.
        let b = match region_at(x, z) {
            Some(ri) if active_map().regions[ri].peak == 0 => active_map().regions[ri].biome,
            _ => TB::Grass,
        };
        return Some((b, ph));
    }
    if let Some(ri) = region_at(x, z) {
        let reg = &active_map().regions[ri];
        if reg.biome == TB::Swamp && dc < SAFE_R {
            return Some((TB::Grass, 1));
        }
        if reg.peak > 0 {
            return Some((reg.biome, mountain_height(x, z, reg)));
        }
        // Flat biomes still get the inland-hills field: dunes in the desert, wooded rises in
        // the forest; the swamp stays marsh-flat.
        let max = if matches!(reg.biome, TB::Swamp | TB::Lava) { 1 } else { 3 };
        return Some((reg.biome, inland_hills(x, z, dc, max)));
    }
    let h = inland_hills(x, z, dc, 4);
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
    terrace_inland(&mut v);
    Arc::new(v)
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
    // recompute per relaxation pass).
    let band: Vec<bool> = (0..(COLS * ROWS) as usize)
        .map(|i| {
            let ix = (i as i32) % COLS;
            let iz = (i as i32) / COLS;
            dist_from_coast(ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE) <= 7
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
        // The Blight + Lava field both ARE swamp to gameplay: poison + slow (`player::movement`),
        // swamp ambience/weather, swamp wildlife/forage. Only the ground + scatter differ. (For
        // the Lava field the swamp DoT reads as a lava burn.)
        TB::Swamp | TB::Blight | TB::Lava => Some(Biome::Swamp),
        _ => None,
    }
}
pub fn is_grass_world(wx: f32, wz: f32) -> bool {
    matches!(tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32).map(|t| t.0), Some(TB::Grass))
}
/// Is world `(x, z)` a Lava-field tile? Lava maps to gameplay [`Biome::Swamp`], so scatter keys
/// off the underlying `TB` directly (swamp props are kept off it; basalt boulders go on it).
fn is_lava_world(wx: f32, wz: f32) -> bool {
    matches!(tile_at((wx + GX).floor() as i32, (wz + GZ).floor() as i32).map(|t| t.0), Some(TB::Lava))
}
/// Smoothed terrain height at integer grid CORNER `(cx, cz)`: the mean flat-top Y of the up-to-four
/// LAND tiles that meet at that corner (water / off-map tiles are skipped, so a coastal corner sits
/// at land height instead of being dragged down toward the sea). `None` when no land tile touches
/// the corner. This is what turns the discrete per-tile height classes into a *continuous*
/// heightfield: adjacent tiles share their boundary corners, so their tops flow into one slope
/// rather than a stepped wall (the old Minecraft-block look).
fn corner_top_y(cx: i32, cz: i32) -> Option<f32> {
    let mut sum = 0.0_f32;
    let mut n = 0u32;
    for (ax, az) in [(cx - 1, cz - 1), (cx, cz - 1), (cx - 1, cz), (cx, cz)] {
        if let Some((_, h)) = tile_at(ax, az) {
            sum += (h - 1) as f32 * GROUND_STEP;
            n += 1;
        }
    }
    (n > 0).then(|| sum / n as f32)
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
    let ix = gx.floor() as i32;
    let iz = gz.floor() as i32;
    let (_, h) = tile_at(ix, iz)?;
    let flat = (h - 1) as f32 * GROUND_STEP;
    let (fx, fz) = (gx - ix as f32, gz - iz as f32);
    let c00 = corner_top_y(ix, iz).unwrap_or(flat);
    let c10 = corner_top_y(ix + 1, iz).unwrap_or(flat);
    let c01 = corner_top_y(ix, iz + 1).unwrap_or(flat);
    let c11 = corner_top_y(ix + 1, iz + 1).unwrap_or(flat);
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
        TB::Lava => p.lava_basalt,
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
            let dry = smoothstep(0.35, 1.40, noise_b(x * 8.0 - 23.0, z * 8.0 + 7.0));
            let col = mix3(base, lin3(p.forest_dark), moist * 0.50);
            mix3(col, lin3(p.forest_dry), dry * 0.35)
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
        TB::Lava => {
            // Cooled black basalt veined with a network of glowing magma seams. Seams sit at the
            // noise zero-crossings (`1 - smoothstep(|n|)` → a thin bright line), with white-hot
            // cores where two seam systems cross.
            let seam = 1.0 - smoothstep(0.0, 0.13, noise_a(x * 2.4 + 9.0, z * 2.4 - 4.0).abs());
            let fine = 1.0 - smoothstep(0.0, 0.09, noise_b(x * 5.5 - 13.0, z * 5.5 + 8.0).abs());
            let col = mix3(base, lin3(p.lava_seam), seam * 0.85);
            let col = mix3(col, lin3(p.lava_seam), fine * 0.40);
            mix3(col, lin3(p.lava_seam_hot), seam * fine * 0.95)
        }
        _ => base,
    }
}

/// Smooth blended ground colour at tile-space (x,z): grass base, each biome blob mixed in
/// over a soft `BLEND` band at its edge, plus a sandy coast fade.
pub(crate) fn ground_color(x: f32, z: f32) -> [f32; 4] {
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
    col = mix3(col, lin3(p.grass_dry), smoothstep(0.25, 1.4, p2) * 0.40);
    col = mix3(col, lin3(p.grass_gold), smoothstep(0.55, 1.5, p3) * 0.45);
    let wob = 2.4 * (x * 0.4 + 1.1).sin() + 2.4 * (z * 0.36 - 0.7).cos();
    for reg in active_map().regions {
        let fray = if reg.peak > 0 { 0.0 } else { edge_fray(x, z) };
        let d = (x - reg.x).hypot(z - reg.z) + wob + fray;
        let edge = reg.r - d; // >0 inside
        let w = smoothstep(-BLEND, BLEND, edge);
        col = mix3(col, biome_col_at(reg.biome, x, z), w);
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
        col = mix3(col, lin3(p.road_dirt), road_s * 0.85);
    }
    let yard_s = crate::castle::yard_strength(wx, wz);
    if yard_s > 0.0 {
        col = mix3(col, lin3(0x52391f), yard_s * 0.92);
    }
    [col[0], col[1], col[2], 1.0]
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
fn bake_shore_distance(images: &mut Assets<Image>) -> (Handle<Image>, Vec4) {
    const W: usize = 288; // covers world x ∈ [-144, 144] (island reaches ±~128 at 1.8 scale)
    const H: usize = 384; // covers world z ∈ [-192, 192] (island ±~95 + the Blight to ~+174)
    let min_x = -(W as f32) / 2.0;
    let min_z = -(H as f32) / 2.0;
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
    let mut img = Image::new(
        Extent3d { width: W as u32, height: H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Linear filtering smooths the 1-texel bands; the default clamp-to-edge address
    // mode makes off-texture samples read the border (open sea = max distance).
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    let region = Vec4::new(min_x, min_z, 1.0 / W as f32, 1.0 / H as f32);
    (images.add(img), region)
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
pub const BUILD_STEPS: u32 = 30;

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
    match step {
        0 => bs_grass_sheet(commands, meshes, images, terrain_mats),
        1 => bs_swamp_sheet(commands, meshes, images, terrain_mats),
        2 => bs_blight_sheet(commands, meshes, images, terrain_mats),
        3 => bs_lava_sheet(commands, meshes, images, terrain_mats),
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
        15 => crate::villagers::populate(commands, meshes, std_mats, creature_mats),
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
        22 => crate::defenses::populate_defenders(commands, meshes, std_mats),
        23 => crate::ruins::populate_landmarks(commands, meshes, std_mats),
        24 => crate::vignettes::populate_vignettes(commands, meshes, std_mats),
        25 => crate::ork_fortress::build(commands, meshes, images, std_mats, creature_mats),
        26 => crate::bridges::populate(commands, meshes, std_mats),
        27 => crate::distant_isles::build(commands, meshes, std_mats),
        28 => crate::rival::build(commands, meshes, images, std_mats),
        29 => bs_swamp_pools(commands, meshes, std_mats),
        _ => {}
    }
}

// ── Build phases (each one [`build_step`] arm; see the old `build` doc for the why of each) ──

/// The island proper: grass blade-grain detail, all tiles that aren't Blight/Swamp/Lava.
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
    let ground_mat = crate::terrain::make_material(&grass_detail, 1.0, images, terrain_mats);
    spawn_terrain_sheet(commands, meshes, ground_mat, |tb| tb != TB::Blight && tb != TB::Swamp && tb != TB::Lava);
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
    // Roughness 0.82 read as DRY matte muck — the single biggest reason the marsh didn't look wet.
    // Dropped to 0.40 so the low (overcast) sun + sky throw a broad damp specular sheen across the
    // bog, reading as standing water / wet mud rather than dry dirt (player: "bardziej mokre bagno").
    let swamp_mat = crate::terrain::make_material(&swamp_detail, 0.40, images, terrain_mats);
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
    let blight_mat = crate::terrain::make_material(&blight_detail, 0.97, images, terrain_mats);
    spawn_terrain_sheet(commands, meshes, blight_mat, |tb| tb == TB::Blight);
}

/// Lava field (map 2 only): basalt crust sheet; magma seams ride the vertex colour. No-op on maps
/// without lava so the home island never builds an empty sheet.
fn bs_lava_sheet(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, terrain_mats: &mut Assets<TerrainMaterial>) {
    if !active_map().has_lava {
        return;
    }
    let lava_detail = GroundDetail {
        scale: 0.30,
        strength: 0.55,
        variation: 0.70,
        seed: 13.0,
        dark: 0x16110d,
        base: 0x2a2421,
        light: 0x4a3a2a,
        grain: 0.80,
        streak: 0.40,
    };
    let lava_mat = crate::terrain::make_material(&lava_detail, 0.90, images, terrain_mats);
    spawn_terrain_sheet(commands, meshes, lava_mat, |tb| tb == TB::Lava);
}

/// The sea plane + shore-distance bake + background sailboats, then plan the ork camps (BEFORE
/// scatter, so their clearings can be reserved out of the prop placement).
fn bs_sea_and_boats(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, std_mats: &mut Assets<StandardMaterial>, water_mats: &mut Assets<WaterMaterial>) {
    let sea_mesh = meshes.add(Plane3d::default().mesh().size(900.0, 900.0).subdivisions(8).build());
    let (shore_tex, shore_region) = bake_shore_distance(images);
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
        },
    });
    commands.spawn((Mesh3d(sea_mesh), MeshMaterial3d(sea), Transform::from_xyz(0.0, SEA_Y, 0.0), crate::biome::BiomeEntity));

    // Island shape is authored in BASE space (CX/CZ, ISLAND_R*); convert to world.
    let isle_c = Vec2::new(CX * MAP_SCALE - GX, CZ * MAP_SCALE - GZ);
    let isle_r = Vec2::new(ISLAND_RX * MAP_SCALE, ISLAND_RZ * MAP_SCALE);
    crate::boats::spawn_boats_island(commands, meshes, std_mats, isle_c, isle_r, SEA_Y);

    crate::camps::plan();
}

/// Scatter one biome's props on its tiles (height-aware), and capture its atmosphere/weather into
/// `state.ambiences` for the [`BiomeAmbiences`] resource (inserted at phase 10).
fn bs_scatter_biome(biome: Biome, commands: &mut Commands, meshes: &mut Assets<Mesh>, std_mats: &mut Assets<StandardMaterial>, state: &mut BuildState) {
    let lo = -GX;
    let hi = GX; // square covers the whole grid; off-map tiles mask out
    let mut cfg = config_for(biome);
    // On a reskinned map (Ashlands) swap every biome's lush green cover for the dead-litter family
    // so the scorched ground doesn't read as blooming meadow. (Trees/rocks left alone.)
    if active_id() != 0 {
        cfg.cover = frontier_cover();
    }
    // On a reskinned map override the per-biome HOME atmosphere with the map's own.
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
            // Lava tiles read as Swamp to gameplay but take ROCK scatter (basalt), never reeds.
            let biome_match = if biome == Biome::Rocky {
                here == Some(Biome::Rocky) || is_lava_world(x, z)
            } else if biome == Biome::Swamp {
                here == Some(Biome::Swamp) && !is_lava_world(x, z)
            } else {
                here == Some(biome)
            };
            biome_match
                && !crate::camps::in_clearing(x, z)
                && !crate::bridges::near_bridge(x, z, 1.0)
                && !crate::ork_fortress::on_gate_approach(x, z)
                && !(crate::ork_fortress::in_blight_world(x, z) && z > 86.0)
                && !crate::rival::near_fort(x, z)
        },
        &|x, z| tile_top_y_world(x, z),
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
                && !crate::castle::in_footprint(x, z)
                && !crate::camps::in_clearing(x, z)
                && !crate::town::near_build_plot(x, z)
                && !crate::bridges::near_bridge(x, z, 1.0)
        },
        &|x, z| tile_top_y_world(x, z),
    );
}

/// Standing bog-water pools scattered across the swamp — the surest "wet" signal (the ground sheen
/// alone reads subtly). Flat glossy dark-teal discs laid a hair above the muck: low roughness + the
/// IBL/sun glint make them read as still water between the reeds/lily-pads (player: "mokre bagno").
/// Swamp tiles only (not the Blight, whose mire is trampled dry earth); kept off build plots/bridges.
fn bs_swamp_pools(commands: &mut Commands, meshes: &mut Assets<Mesh>, std_mats: &mut Assets<StandardMaterial>) {
    let water_mat = std_mats.add(StandardMaterial {
        // Dark murky teal; semi-transparent so the muck tints through at the rim like shallow water.
        base_color: Color::srgba(0x21 as f32 / 255.0, 0x33 as f32 / 255.0, 0x2c as f32 / 255.0, 0.86),
        perceptual_roughness: 0.12, // glassy → mirrors the sky/sun = the "wet" read
        reflectance: 0.6,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    // A unit disc lying flat (normal +Y); scaled per-pool. Circle's mesh faces +Z, so tip it up.
    let disc: Vec<Handle<Mesh>> = (0..3)
        .map(|v| meshes.add(Circle::new(1.0).mesh().resolution(10 + v * 2).build()))
        .collect();
    for iz in 0..ROWS {
        for ix in 0..COLS {
            let Some((TB::Swamp, h)) = tile_at(ix, iz) else { continue };
            let mut rng = tileworld_core::rng::Mulberry32::new((iz * COLS + ix) as u32 ^ 0x9a7d_31b1);
            if rng.next() > 0.24 {
                continue; // ~1 pool per ~4 swamp tiles → frequent but not a solid sheet
            }
            let wx = ix as f32 - GX + 0.5 + (rng.next() as f32 - 0.5) * 0.6;
            let wz = iz as f32 - GZ + 0.5 + (rng.next() as f32 - 0.5) * 0.6;
            if crate::bridges::near_bridge(wx, wz, 0.6)
                || crate::camps::in_clearing(wx, wz)
                || crate::town::near_build_plot(wx, wz)
            {
                continue;
            }
            let r = 0.55 + rng.next() as f32 * 0.95;
            let v = (rng.next() * 3.0) as usize % 3;
            commands.spawn((
                Mesh3d(disc[v].clone()),
                MeshMaterial3d(water_mat.clone()),
                Transform {
                    // A hair above the tile top so it films over the muck without z-fighting.
                    translation: Vec3::new(wx, tile_top_y_world(wx, wz) + 0.03, wz),
                    rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
                        * Quat::from_rotation_z(rng.next() as f32 * std::f32::consts::TAU),
                    scale: Vec3::new(r, r * (0.8 + rng.next() as f32 * 0.5), 1.0),
                },
                crate::biome::BiomeEntity,
            ));
        }
    }
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

/// Ground cover for the open frontier, chosen by the active map. The home isle gets the lush
/// meadow (grass tufts, clover, ferns, flowers, mushrooms — all hard-coded green/floral in
/// `groundcover.rs`); a reskinned map like Ashlands instead gets **charred ground litter** (grey
/// pebbles, bark twigs, rust-burnt leaves, pinecones, acorns) so the cinder frontier reads as a
/// scorched waste, not a flower lawn. (Recolouring every cover mesh per-map would be far more
/// invasive — the litter family is already neutral/dead-toned, so we just scatter that instead.)
fn frontier_cover() -> Vec<PropClass> {
    if active_id() != 0 {
        return vec![
            PropClass {
                variants: (0..gc::NUM_LITTER_VARIANTS).map(|v| (gc::build_floor_litter_mesh(v), 1.0)).collect(),
                chance: 0.26,
                scale: (0.7, 1.35),
                tree: false,
                block_radius: 0.0,
            },
        ];
    }
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
    let mut cz = 0;
    while cz < ROWS {
        let mut cx = 0;
        while cx < COLS {
            let mesh = build_terrain_chunk(
                keep,
                cx,
                (cx + TERRAIN_CHUNK).min(COLS),
                cz,
                (cz + TERRAIN_CHUNK).min(ROWS),
            );
            if mesh.count_vertices() > 0 {
                commands.spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(mat.clone()),
                    Transform::default(),
                    crate::biome::BiomeEntity,
                ));
            }
            cx += TERRAIN_CHUNK;
        }
        cz += TERRAIN_CHUNK;
    }
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

    const NB: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

    for iz in iz0..iz1 {
        for ix in ix0..ix1 {
            let Some((tb, h)) = tile_at(ix, iz) else { continue };
            if !keep(tb) {
                continue;
            }
            let flat = (h - 1) as f32 * GROUND_STEP;
            let wx = ix as f32 - GX;
            let wz = iz as f32 - GZ;

            // ── Smoothed corner heights (mean of the LAND tiles meeting at each corner). Adjacent
            //    tiles share their boundary corners, so neighbouring tops meet as ONE continuous
            //    slope instead of a stepped vertical wall — this is what removes the blocky
            //    Minecraft elevation look. The hero, NPCs and scatter all follow the SAME smoothed
            //    surface (`ground_at_world` → `smooth_surface_y`), so nothing floats over or sinks
            //    into the new sloped ground. (`unwrap_or(flat)` is a belt-and-braces fallback;
            //    every land tile contributes to all four of its own corners, so these are `Some`.)
            let cy00 = corner_top_y(ix, iz).unwrap_or(flat);
            let cy10 = corner_top_y(ix + 1, iz).unwrap_or(flat);
            let cy01 = corner_top_y(ix, iz + 1).unwrap_or(flat);
            let cy11 = corner_top_y(ix + 1, iz + 1).unwrap_or(flat);

            // Per-corner normal from the heightfield gradient (central difference over neighbour
            // corners) → shading rolls smoothly across tile seams, no per-facet crease. The `2.0`
            // weights vertical vs. the 1-unit horizontal spacing of the corners on each side.
            let cn = |cx: i32, cz: i32| {
                let e = corner_top_y(cx + 1, cz).unwrap_or(flat);
                let w = corner_top_y(cx - 1, cz).unwrap_or(flat);
                let s = corner_top_y(cx, cz + 1).unwrap_or(flat);
                let n = corner_top_y(cx, cz - 1).unwrap_or(flat);
                nrm3([w - e, 2.0, n - s])
            };

            // Per-corner blended colour. `c` samples in BASE/tile-index space; `cw` is the same in
            // world XZ (tile index = world + G).
            let c = |cx: f32, cz: f32| ground_color(cx / MAP_SCALE, cz / MAP_SCALE);
            let cw = |x: f32, z: f32| c(x + GX, z + GZ);

            // Continuous top: four corner heights + four gradient normals (full tile span — no
            // inset/chamfer, since land seams now join through the shared corners themselves).
            quadn(
                [[wx, cy00, wz], [wx + 1.0, cy10, wz], [wx + 1.0, cy11, wz + 1.0], [wx, cy01, wz + 1.0]],
                [cn(ix, iz), cn(ix + 1, iz), cn(ix + 1, iz + 1), cn(ix, iz + 1)],
                [cw(wx, wz), cw(wx + 1.0, wz), cw(wx + 1.0, wz + 1.0), cw(wx, wz + 1.0)],
                &mut indices, &mut positions, &mut normals, &mut colors,
            );

            // Shoreline skirt — ONLY where a tile edge meets water / off-map. Land–land seams need
            // no wall (their tops already meet through shared corners), so inland terraces are gone:
            // every elevation change inland is now a smooth slope. This drops the smoothed coast
            // edge down to the sea so there's no hole at the shore. Grass coast shows exposed dirt;
            // every other biome darkens its own top colour (the same palette the old walls used).
            let top_col = ground_color((ix as f32 + 0.5) / MAP_SCALE, (iz as f32 + 0.5) / MAP_SCALE);
            let (wall_top, wall_bot) = if tb == TB::Grass {
                let j = 0.82 + 0.32 * (noise_b(ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE) * 0.5 + 0.5);
                let d = lin3(active_map().palette.dirt);
                let t = [d[0] * j, d[1] * j, d[2] * j, 1.0];
                let b = [d[0] * j * 0.68, d[1] * j * 0.64, d[2] * j * 0.60, 1.0];
                (t, b)
            } else if tb == TB::Snow {
                let lip = lin3(active_map().palette.snow_cliff_lip);
                let rock = lin3(active_map().palette.snow_cliff_rock);
                ([lip[0], lip[1], lip[2], 1.0], [rock[0], rock[1], rock[2], 1.0])
            } else {
                let t = [top_col[0] * 0.80, top_col[1] * 0.78, top_col[2] * 0.76, 1.0];
                let b = [top_col[0] * 0.58, top_col[1] * 0.56, top_col[2] * 0.54, 1.0];
                (t, b)
            };
            for (dx, dz) in NB {
                if tile_at(ix + dx, iz + dz).is_some() {
                    continue; // land neighbour → tops already meet, no skirt
                }
                // The two boundary corners (at their smoothed heights) + the outward face normal.
                let (x0, y0, z0, x1, y1, z1, n): (f32, f32, f32, f32, f32, f32, [f32; 3]) = match (dx, dz) {
                    (1, 0) => (wx + 1.0, cy10, wz, wx + 1.0, cy11, wz + 1.0, [1.0, 0.0, 0.0]),
                    (-1, 0) => (wx, cy01, wz + 1.0, wx, cy00, wz, [-1.0, 0.0, 0.0]),
                    (0, 1) => (wx + 1.0, cy11, wz + 1.0, wx, cy01, wz + 1.0, [0.0, 0.0, 1.0]),
                    _ => (wx, cy00, wz, wx + 1.0, cy10, wz, [0.0, 0.0, -1.0]),
                };
                quad(
                    [[x0, y0, z0], [x1, y1, z1], [x1, SEA_Y, z1], [x0, SEA_Y, z0]],
                    n,
                    [wall_top, wall_top, wall_bot, wall_bot],
                    &mut indices, &mut positions, &mut normals, &mut colors,
                );
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
        let band = |ix: i32, iz: i32| dist_from_coast(ix as f32 / MAP_SCALE, iz as f32 / MAP_SCALE) <= 7;
        let mut cliffs: Vec<((i32, i32), (i32, i32), i32, i32)> = Vec::new();
        for iz in 0..ROWS {
            for ix in 0..COLS {
                let Some((_, h)) = tile_at(ix, iz) else { continue };
                // Only +x / +z neighbours so each edge is counted once.
                for (dx, dz) in [(1, 0), (0, 1)] {
                    let (nx, nz) = (ix + dx, iz + dz);
                    let Some((_, nh)) = tile_at(nx, nz) else { continue };
                    if band(ix, iz) || band(nx, nz) {
                        continue; // seaward cliffs are deliberately sheer
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
}

