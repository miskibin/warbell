//! **Realistic forest** — the Blender-authored PBR kit (`tools/blender_forest/`, Poly Haven CC0
//! scans) swapped into the FOREST biome in place of the low-poly vertex-colour scatter.
//!
//! What it replaces, and how it stays cheap:
//! - **Trees** (`trees.glb`): procedurally grown oaks / beeches / pines / a dead snag with scanned
//!   bark and leaf-spray cards, two LODs each (~1.5k / ~0.35k tris). A tree stays ONE gameplay
//!   entity (`ChopTree` + wind `Sway` on the parent, exactly as before) with its LOD meshes as
//!   children, cross-faded by `VisibilityRange` at [`LOD_NEAR`] and hard-culled at 180u as before.
//! - **Undergrowth + props** (`undergrowth.glb`, `props.glb`): ferns, grass clumps, seed stalks,
//!   scanned stumps / logs / deadfall / mossy rocks. Merged per 16u chunk **per material**, so a
//!   whole chunk of ferns+grass is one draw (they share one atlas), same culling as the old cover.
//! - **Ground**: three scanned forest-floor layers (litter / moss / path) with height maps, splatted
//!   by the terrain shader inside the forest with height-blending and parallax occlusion mapping
//!   (`terrain.wesl`, `FP_*`). The forest/road mask is baked once from the tile map.
//!
//! Placement, spacing, blockers, chop/regrow and determinism are untouched: `biome::scatter_region`
//! still makes every decision and only asks [`Kit`] *what to draw* at the very end.
//! On by default; `FOREST_PBR=0` restores the low-poly forest (A/B and fallback). If any kit file
//! fails to load the game logs a warning and keeps the low-poly forest.

pub mod biomekit;
pub mod foliage;
pub mod impostor;
pub mod load;
pub mod reskin;
pub mod far_cover;

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::camera::visibility::VisibilityRange;
use bevy::prelude::*;

use crate::terrain::TerrainMaterial;

/// LOD0 → LOD1 cross-fade distance (world units, dithered over ±[`LOD_BAND`]).
pub const LOD_NEAR: f32 = 30.0;
/// Abrupt everywhere: Bevy's LOD0/LOD1 dither is a screen-stable 4×4 checkerboard
/// (`pbr_functions::visibility_range_dither`, keyed on `frag_coord` only) that neither SMAA nor
/// TAA can resolve, so every tree parked inside a band wore a crisp grid of "dots" — first fixed
/// in the swamp with a ±0.5 band, then still visible on forest trees at ~30u under TAA. A swap
/// at 30u reads far softer than a held dither pattern (and skips the dither pipeline entirely).
const LOD_BAND: f32 = 0.0;
/// Kept for the swamp's call sites; same abrupt swap as everywhere else.
pub const LOD_BAND_NARROW: f32 = LOD_BAND;
/// Far cull, same as the low-poly trees (fog has them by then).
const TREE_FAR: f32 = 180.0;
/// A kit prop only blocks when it's at least this tall (scaled, ~knee height on the 1.3u hero)…
const SOLID_MIN_H: f32 = 0.35;
/// …and this wide (half-extent).
const SOLID_MIN_R: f32 = 0.25;
/// Kit trees are modelled at real proportions (4-6u); the forest scatter's per-class scale ranges
/// were tuned for the ~1.5u low-poly trees. This brings a kit tree to ~2x the old tree's height
/// (3-6u canopy, 6.5-8u old-growth giants) so the forest keeps its gameplay readability.
const TREE_MODEL_SCALE: f32 = 0.62;

/// Ground-cover / prop chunk cut-offs (mirror `biome::spawn_chunks`), plus the short-range
/// floor clutter (fallen-leaf sheets, pebbles, twigs) that only matters near the camera.
const COVER_FAR: f32 = 58.0;
const PROPS_FAR: f32 = 75.0;
/// Props chunks (scanned rocks / logs / stumps / bushes: the heaviest merged geometry) switch to a
/// meshopt-simplified twin past this distance ([`Tier::PropsFar`]); unscaled by render distance.
const PROPS_LOD: f32 = 30.0;
/// Target triangle share of the far props twin (the simplifier stops earlier if the shape would
/// deviate by more than 2% of the model's size).
const PROPS_LOD_RATIO: f32 = 0.25;
/// Biome static models ([`Kit::attach_static_lod`]) swap to their far twin here (unscaled).
const STATIC_LOD: f32 = 36.0;
const CLUTTER_FAR: f32 = 34.0;
/// The green carpet is density-LOD'd like big open-world games do it: every tuft near the camera,
/// a thinned set of bigger tufts in a ring beyond, nothing past that — where the ground shader's
/// green moss layer under the carpet carries the read with zero geometry.
const LUSH_NEAR: f32 = 22.0;
const LUSH_FAR: f32 = 46.0;
/// Undergrowth chunks thin out tuft by tuft over this many units before their cull distance
/// (and the far lush ring thins in over the same band before [`LUSH_NEAR`], under the near ring's
/// thin-out) instead of popping as whole 16u squares — `foliage::dissolve`.
const DISSOLVE_BAND: f32 = 9.0;

/// Far props tier on? (`FOREST_PROPSLOD=0` draws full props to `PROPS_FAR`, A/B.) Read once.
fn props_lod_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_PROPSLOD").map(|v| v != "0").unwrap_or(true))
}

/// [`DISSOLVE_BAND`], or 0 when `FOREST_DISSOLVE=0`.
fn dissolve_band() -> f32 {
    if std::env::var("FOREST_DISSOLVE").is_ok_and(|v| v == "0") { 0.0 } else { DISSOLVE_BAND }
}

/// Which merge bucket a kit prop lands in - decides its cull distance and landmark clearing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tier {
    /// Bushes / rocks / stumps / logs from the main scatter, `0..PROPS_LOD`.
    Props,
    /// The same props, meshopt-simplified, `PROPS_LOD..PROPS_FAR` (queued alongside every
    /// [`Tier::Props`] push).
    PropsFar,
    /// Ferns / grass / stalks from the cover scatter (cleared around landmarks).
    Cover,
    /// Dense near-camera floor clutter ([`Kit::scatter_clutter`]).
    Clutter,
    /// Green carpet, full density, `0..LUSH_NEAR`.
    LushNear,
    /// Green carpet, thinned + enlarged, `LUSH_NEAR..LUSH_FAR`.
    LushFar,
}

/// Contact-darkening splats (x, z, radius) around trunks and solid props, baked into the forest
/// mask's B channel so the floor darkens where roots and stones meet it. Filled during the forest
/// scatter; [`SCATTER_DONE`] tells the terrain hook the list is complete.
static SPLATS: std::sync::Mutex<Vec<Vec3>> = std::sync::Mutex::new(Vec::new());
/// Softer splats (x, z, radius) under the undergrowth's grass / fern tufts, baked into the same B
/// channel at [`COVER_SPLAT_WEIGHT`]: the soil where a clump roots reads damp and dark (the terrain
/// shader browns it too), so the tufts grow OUT of the floor instead of standing on a green sheet.
static COVER_SPLATS: std::sync::Mutex<Vec<Vec3>> = std::sync::Mutex::new(Vec::new());
const COVER_SPLAT_WEIGHT: f32 = 0.42;
static SCATTER_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called at the top of the forest scatter (also on an in-process world rebuild).
pub fn begin_scatter() {
    SPLATS.lock().expect("splats poisoned").clear();
    COVER_SPLATS.lock().expect("splats poisoned").clear();
    SCATTER_DONE.store(false, std::sync::atomic::Ordering::Release);
    far_cover::reset();
}

/// Register a contact-darkening splat (world XZ + radius in world units).
pub fn splat(x: f32, z: f32, r: f32) {
    SPLATS.lock().expect("splats poisoned").push(Vec3::new(x, z, r));
}
/// Alpha-test cutoff for leaf / blade cards (mips keep this coverage, see `load::mip_chain`).
const CARD_CUTOFF: f32 = 0.42;

/// Is the kit wanted? Read once.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_PBR").map(|v| v != "0").unwrap_or(true))
}

static KIT: OnceLock<Kit> = OnceLock::new();

/// The loaded kit, or `None` when disabled / failed to load / before `PreStartup`.
/// A write-once global for the same reason as `phototrees::get` — the only consumer sits deep
/// inside the `Startup` world-build chain.
pub fn get() -> Option<&'static Kit> {
    KIT.get()
}

/// One mesh primitive of a kit model: CPU copy (for chunk merging) + uploaded handle.
pub struct Part {
    pub mesh: Mesh,
    pub handle: Handle<Mesh>,
    pub mat: usize,
    /// meshopt-simplified twin for far tiers (`autolod`), built on first use.
    lod: OnceLock<Option<Mesh>>,
    /// Its uploaded handle (shared by every instance, so far twins batch like the originals).
    lod_handle: OnceLock<Option<Handle<Mesh>>>,
}

impl Part {
    /// The far-tier mesh: simplified to ~[`PROPS_LOD_RATIO`] of the triangles where that holds
    /// the shape, else the full mesh.
    pub fn lod_mesh(&self) -> &Mesh {
        self.lod.get_or_init(|| crate::autolod::simplify_far(&self.mesh, PROPS_LOD_RATIO, 0.02)).as_ref().unwrap_or(&self.mesh)
    }

    /// Handle of the far twin, or of the full mesh when simplifying didn't pay.
    pub fn lod_handle(&self, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.lod_handle
            .get_or_init(|| {
                self.lod_mesh();
                self.lod.get().and_then(|m| m.as_ref()).map(|m| meshes.add(m.clone()))
            })
            .clone()
            .unwrap_or_else(|| self.handle.clone())
    }
}

pub struct Model {
    pub parts: Vec<Part>,
    /// XZ half-extents of the model about its origin (unscaled).
    pub half: Vec2,
    /// Radius of the trunk base incl. root flare (bark below 0.3u), 0 for non-trees.
    pub trunk: f32,
    /// Rocks / logs / stumps: you walk around them, not through.
    pub solid: bool,
    /// Top of the model above its base (unscaled).
    pub height: f32,
}

pub struct GroundTextures {
    pub albedo: Handle<Image>,
    pub normal: Handle<Image>,
    pub ormh: Handle<Image>,
}

/// A kit material: plain PBR, or a wind-swayed foliage material (leaf cards, undergrowth).
#[derive(Clone)]
pub enum KitMat {
    Std(Handle<StandardMaterial>),
    Foliage(Handle<foliage::FoliageMaterial>),
    /// Snow-dusted twin (`alpine::snowkit`) — the snow biome's trees, rocks and cover.
    Snow(Handle<crate::alpine::snowkit::SnowKitMaterial>),
}

impl KitMat {
    fn insert(&self, e: &mut EntityCommands) {
        match self {
            KitMat::Std(h) => e.insert(MeshMaterial3d(h.clone())),
            KitMat::Foliage(h) => e.insert(MeshMaterial3d(h.clone())),
            KitMat::Snow(h) => e.insert(MeshMaterial3d(h.clone())),
        };
    }
}

pub struct Kit {
    mats: Vec<KitMat>,
    /// Snow-dusted twin per material (`None` = no twin, the plain material is used).
    snow_mats: Vec<Option<KitMat>>,
    models: Vec<Model>,
    by_name: HashMap<String, usize>,
    pub ground: GroundTextures,
    /// Far-tree impostor atlases (`impostor`), `None` when disabled / not packed.
    pub impostors: Option<impostor::Atlas>,
    /// (canopy material index, its range-dissolving twin): the LOD0 crown of a tree that hands
    /// over to an impostor thins out leaf clump by leaf clump over the last
    /// [`impostor::IMP_FADE_IN`] units of its range while the impostor thins in.
    canopy_fade: Option<(usize, KitMat)>,
    /// Per model: how it reads from afar (`far_cover`), computed on first placement.
    cover: Vec<OnceLock<Option<far_cover::Swatch>>>,
}

/// Material sets by the names Blender exported: (name, PBR set file stem, foliage card?).
const MATERIALS: [(&str, &str, bool); 50] = [
    ("bark_oak", "bark_oak", false),
    ("bark_beech", "bark_beech", false),
    ("bark_pine", "bark_pine", false),
    ("canopy", "canopy", true),
    ("undergrowth", "undergrowth", true),
    ("prop_stump", "prop_stump", false),
    ("prop_log", "prop_log", false),
    ("prop_branches", "prop_branches", false),
    ("prop_rock_a", "prop_rock_a", false),
    ("prop_rock_b", "prop_rock_b", false),
    // Apples: untextured, colour from the mesh's vertex colours (see `pbr_material`).
    ("fruit", "", false),
    // Structure sets (structures.glb: ork camp, mill).
    ("s_planks", "s_planks", false),
    ("s_stone", "s_stone", false),
    ("s_hide", "s_hide", false),
    ("s_thatch", "s_thatch", false),
    ("s_cloth", "s_cloth", false),
    ("s_iron", "s_iron", false),
    // Wayside furniture / story set-pieces (wayside.glb): weathered rock, trodden earth, and
    // nail-free hewn timber (the s_planks scan with its nail rows cut out).
    ("s_rock", "s_rock", false),
    ("s_earth", "s_earth", false),
    ("s_timber", "s_timber", false),
    // Settled snow on the cold-biome story scenes (stories_cold.glb): the Poly Haven snow_02 scan
    // (tools/blender_forest/snow_set.sh); vertex colour adds the blue in the shaded drift folds.
    ("s_snow", "s_snow", false),
    // Cast bronze (the Crown convoy's war bell): untextured true metal, colour from the vertex
    // colours (aged bronze mottled with verdigris), a soft sun highlight.
    ("bronze", "bronze", false),
    // Castle kit (castle_kit.glb; tools/blender_forest/masonry_set.sh, Poly Haven CC0 scans):
    // coursed pale limestone ashlar (our stone tier), grey rubble/fieldstone (the old colony ruins
    // and bell tower), weathered split shingles (keep and tower roofs).
    ("s_ashlar", "s_ashlar", false),
    ("s_rubble", "s_rubble", false),
    ("s_shingle", "s_shingle", false),
    // Creature parts (creatures.glb, animals.glb, people.glb) — never drawn with this material
    // (recoloured CPU meshes on the creature material); the entry only lets the loader accept them.
    ("creature", "", false),
    // Lime-whitewashed wattle-and-daub infill of the town's timber-frame houses (town_kit.glb): the
    // Poly Haven white_rough_plaster scan (tools/blender_forest/plaster_set.sh); vertex colour adds
    // the splash-back grime at the wall feet and soot under the eaves.
    ("s_plaster", "s_plaster", false),
    // Emissive beads and seams on the regional wardens (wardens.glb: the treant's knot eyes, the
    // golem's molten seams): unlit, colour from the vertex colours x an HDR gain so they bloom.
    ("glow", "glow", false),
    // The swamp's own flora (swamp_flora.glb, tools/blender_forest/swamp_flora.py): fibrous
    // bald-cypress bark (Poly Haven metasequoia_bark), furrowed willow bark (bark_willow_02), the
    // same willow scan bleached silver for drowned snags, and the swamp card atlas (cypress
    // fronds, Spanish moss, willow whips, cattails, lily pads) on the foliage wind path.
    ("bark_cypress", "bark_cypress", false),
    ("bark_willow", "bark_willow", false),
    ("bark_snag", "bark_snag", false),
    ("swamp_cards", "swamp_cards", true),
    // Glacier ice (landmarks.glb: the Frozen Spire): untextured, colour from the vertex colours
    // (deep blue core, frosted up-facing facets), glassy and slick.
    ("ice", "ice", false),
    // The desert / forest story sites (sites_warm.glb; tools/blender_forest/biome_sites_set.sh,
    // Poly Haven CC0 scans): wind-scoured sandstone, glazed tile (vertex colour = the glaze),
    // moss-furred rotten wood, clay (mould / furnace / jars), lichen-and-moss stone.
    ("s_sandstone", "s_sandstone", false),
    ("s_tile", "s_tile", false),
    ("s_mosswood", "s_mosswood", false),
    ("s_clay", "s_clay", false),
    ("s_mossrock", "s_mossrock", false),
    // The s_timber scan made matte (no roughness map, low F0) for broad weathered board walls that
    // read slate-grey in shade from the sky reflection: the village barn, the rival gate leaves.
    ("s_timber_matte", "s_timber", false),
    // The swamp / rocky discovery sites (wild_sites.glb, tools/blender_forest/wild_sites.py; scans
    // via wild_set.sh): each biome its own palette. Rocky: lichen-crusted granite (the Old Ones'
    // menhirs, gong stones, rock door) and dry-laid slate (the colony signal tower, bridge piers).
    // Swamp: moss-jointed chapel fieldstone, wet peat, rotting timber, rusted iron, and still black
    // bog water (untextured, glossy; colour from the vertex colours).
    ("s_granite", "s_granite", false),
    ("s_slate", "s_slate", false),
    ("s_chapel", "s_chapel", false),
    ("s_peat", "s_peat", false),
    ("s_rotwood", "s_rotwood", false),
    ("s_rust", "s_rust", false),
    ("bogwater", "bogwater", false),
    // Scanned small props (scanned_props.glb, tools/blender_forest/scanned_props.py; Poly Haven CC0
    // models): the shrub_02 saplings that make the bushes (leaf geometry, drawn double-sided — see
    // `pbr_material`), the sea-chest crates, and the wine barrel (its look baked onto a lathe).
    ("ph_shrub_02", "ph_shrub_02", false),
    ("ph_wine_barrel_01", "ph_wine_barrel_01", false),
    ("ph_wooden_crate_01", "ph_wooden_crate_01", false),
    ("ph_wooden_crate_02", "ph_wooden_crate_02", false),
];

impl Kit {
    fn mat_by_name(&self, name: &str) -> Option<usize> {
        MATERIALS.iter().position(|m| m.0 == name)
    }

    /// Model index by exported name; a LOD-paired model (`bush_a_lod0`/`_lod1`) also answers to its
    /// bare name, so scatter tables can say `"bush_a"` (they did, and silently drew nothing).
    fn model(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied().or_else(|| self.by_name.get(&format!("{name}_lod0")).copied())
    }

    fn pick(&self, names: &[&str], roll: f32) -> Option<usize> {
        let i = ((roll * names.len() as f32) as usize).min(names.len() - 1);
        self.model(names[i])
    }

    /// [`Self::pick`] for other scatter tables (`alpine::snowkit`).
    pub fn pick_named(&self, names: &[&str], roll: f32) -> Option<usize> {
        self.pick(names, roll)
    }

    /// Material `i`, or its snow-dusted twin when `snow` (falls back to the plain one).
    fn mat(&self, i: usize, snow: bool) -> &KitMat {
        match self.snow_mats.get(i) {
            Some(Some(m)) if snow => m,
            _ => &self.mats[i],
        }
    }

    /// Which tree to draw for forest tree class `class` (0 = canopy mix, 1 = ancient giants) and
    /// its SOURCE variant (the variant index before `biome::upload_classes` tint expansion).
    /// The forest canopy class lists five living kinds × `trees::TREE_TINTS` then one dead tree
    /// (`biome_forest::config`), so the source variant encodes the old `TreeKind`.
    /// (Apple trees are NOT scatter picks: they are the harvestable forage orchard, see
    /// [`Self::apple_tree`] — a fruiting tree you can't pick from would read as a bug.)
    pub fn tree_for(&self, class: usize, src_variant: usize) -> Option<usize> {
        let tints = crate::trees::TREE_TINTS.len();
        let name = if class == 1 {
            ["oak_a", "oak_b", "oak_c"][src_variant % 3]
        } else if src_variant >= 5 * tints {
            "dead_a"
        } else {
            let t = src_variant % tints;
            match src_variant / tints {
                0 => ["oak_a", "oak_b", "oak_c"][t % 3],   // Broadleaf
                1 => ["beech_a", "beech_b"][t % 2],        // Birch
                2 => ["pine_a", "pine_b", "pine_c"][t % 3], // Pine
                3 => ["beech_b", "beech_a"][t % 2],        // Poplar → tall slim beech
                _ => ["oak_c", "oak_a", "oak_b"][t % 3],   // Autumn canopy → oak
            }
        };
        self.model(&format!("{name}_lod0"))
    }

    /// What to draw for a forest passive class (props: 2 bush / 3 rock / 4 stump; cover: 0 grass,
    /// 1 clover, 2 mushroom, 3 flowers, 4 fern, 5 litter). `None` = leave the spot bare — a real
    /// forest floor is litter with scattered plants, not a meadow carpet. Returns (model, scale ×).
    pub fn passive_for(&self, class: usize, cover: bool, roll: f32, roll2: f32) -> Option<(usize, f32)> {
        const GRASS: [&str; 4] = ["grass_a", "grass_b", "grass_c", "grass_d"];
        const FERN: [&str; 3] = ["fern_a", "fern_b", "fern_c"];
        const FLOWERS: [&str; 6] = ["flower_daisy", "flower_yellow", "flower_lupine_a", "flower_daisy", "flower_yellow", "flower_lupine_b"];
        const ROCKS: [&str; 13] = [
            "rock_01_a", "rock_01_b", "rock_01_c", "rock_01_d", "rock_01_e", "rock_01_f", "rock_02_a",
            "rock_02_b", "rock_02_c", "rock_02_d", "rock_02_e", "rock_02_f", "rock_02_g",
        ];
        let m = |names: &[&str], s: f32| self.pick(names, roll2).map(|i| (i, s));
        if cover {
            match class {
                0 => m(&GRASS, 1.0),
                1 if roll < 0.35 => m(&GRASS, 1.0),
                1 if roll < 0.55 => m(&FERN, 0.85),
                2 if roll < 0.25 => m(&["branches_a"], 0.5),
                3 if roll < 0.70 => m(&FLOWERS, 1.0),
                3 => m(&["stalks_a"], 1.0),
                4 => m(&FERN, 1.0),
                5 if roll < 0.5 => m(&["branches_a"], 0.7),
                5 => m(&ROCKS, 0.35),
                _ => None,
            }
        } else {
            match class {
                2 => m(&["bush_a", "bush_b"], 1.0),
                3 => m(&ROCKS, 0.9),
                4 if roll < 0.45 => m(&["stump_a"], 1.0),
                4 if roll < 0.8 => m(&["log_a"], 0.8),
                4 => m(&["branches_a"], 1.0),
                _ => None,
            }
        }
    }

    /// What the open grass frontier (castle meadow, village, NW grassland) draws for a cover roll,
    /// in `worldmap::frontier_cover` order [grass, clover, fern, flower, mushroom]: a meadow is grass
    /// and wildflowers — forest ferns, dead branches and mushrooms stay under the trees.
    pub fn meadow_cover(&self, class: usize, roll: f32, roll2: f32) -> Option<(usize, f32)> {
        const GRASS: [&str; 4] = ["grass_a", "grass_b", "grass_c", "grass_d"];
        const FLOWERS: [&str; 6] = ["flower_daisy", "flower_yellow", "flower_daisy", "flower_yellow", "flower_lupine_a", "flower_daisy"];
        let m = |names: &[&str], s: f32| self.pick(names, roll2).map(|i| (i, s));
        match class {
            0 | 1 | 2 => m(&GRASS, 0.9 + roll * 0.4),
            3 if roll < 0.75 => m(&FLOWERS, 1.0),
            3 => m(&["stalks_a"], 1.0),
            _ => None,
        }
    }

    /// Hang tree `model` (its `_lod0`, plus the matching `_lod1`) under an already-spawned
    /// gameplay entity. The parent keeps `Transform` / `Sway` / `ChopTree`; the children only draw.
    pub fn attach_tree(&self, e: &mut EntityCommands, lod0: usize, cull: bool) {
        self.attach_tree_tagged(e, lod0, cull, ());
    }

    /// [`Self::attach_tree`] with the LOD0 → LOD1 dither crossfade squeezed to ±[`LOD_BAND_NARROW`].
    /// The swamp's feathery cypress / willow crowns are LOD0 sprays vs. far fewer, bigger LOD1
    /// cards, so mid-fade most of the crown is covered by only ONE of the two complementary
    /// dither masks: every tree whose base sat 28–32u from the camera wore a crisp, screen-stable
    /// 4×4 checkerboard (SMAA can't resolve it) — the "dots" over the swamp canopy. A 1u band
    /// keeps the hand-off soft for the frame or two a walking camera spends crossing it, and is
    /// no dearer than the wide one (the dither pipeline only runs inside the band).
    pub fn attach_tree_narrow(&self, e: &mut EntityCommands, lod0: usize, cull: bool) {
        self.attach_tree_with(e, lod0, cull, (), false, LOD_BAND_NARROW);
    }

    /// Collision radius for a kit tree `lod0` planted at scale `s` (the trunk you see, not the old
    /// low-poly tree's footprint).
    pub fn trunk_radius(&self, lod0: usize, s: f32) -> f32 {
        (self.models[lod0].trunk * TREE_MODEL_SCALE * s).clamp(0.08, 0.8)
    }

    /// Register the collision of a solid kit prop (rock / stump) — only when it is a real obstacle:
    /// at least [`SOLID_MIN_H`] tall (you step over anything lower — pebbles, twigs, lying logs)
    /// and [`SOLID_MIN_R`] across. The shape is circles, not a box: one for a roundish rock, a row
    /// along the long axis for an elongated one, so the hero's obstacle assist slides him round it
    /// (boxes stop him dead on a diagonal). Returns false for walk-through props.
    pub fn register_solid(&self, model: usize, x: f32, z: f32, scale: f32, rot: Quat) -> bool {
        let m = &self.models[model];
        let half = m.half * scale;
        if !m.solid || m.height * scale < SOLID_MIN_H || half.max_element() < SOLID_MIN_R {
            return false;
        }
        let (major, minor, along) = if half.x >= half.y {
            (half.x, half.y, rot * Vec3::X)
        } else {
            (half.y, half.x, rot * Vec3::Z)
        };
        let r = (minor * 0.8).clamp(0.12, 0.95);
        let reach = (major * 0.85 - r).max(0.0);
        let n = (reach / r).ceil() as i32;
        for k in -n..=n {
            let t = if n == 0 { 0.0 } else { k as f32 / n as f32 * reach };
            crate::blockers::add(x + along.x * t, z + along.z * t, r);
        }
        true
    }

    /// A recoloured CPU copy of creature part `name` (creatures.glb): vertex colour R holds a
    /// region code (region/16), G a baked cavity term. Each region gets `palette[region]` (rgb,
    /// darkened by the cavity) and its creature surface code in alpha.
    pub fn creature_part(&self, name: &str, palette: &[([f32; 4], crate::creature::Surf)]) -> Option<Mesh> {
        use bevy::mesh::VertexAttributeValues;
        let m = self.model(name)?;
        let mut mesh = self.models[m].parts.first()?.mesh.clone();
        if let Some(VertexAttributeValues::Float32x4(cols)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
            for c in cols.iter_mut() {
                let region = ((c[0] * 16.0).floor() as usize).min(palette.len().saturating_sub(1));
                let (rgb, surf) = palette[region];
                let cav = c[1];
                *c = [rgb[0] * cav, rgb[1] * cav, rgb[2] * cav, crate::creature::surf_code(surf)];
            }
        }
        // Not needed on the creature path (triplanar), and the merger-free rigs don't care.
        mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
        Some(mesh)
    }

    /// [`Self::creature_part`] for the townsfolk pieces (people.glb): vertex colour B is a baked
    /// WEATHERING amount (ruddy cheeks and knuckles, grimy hems and knees, dusty boots) that pulls
    /// each region's colour toward `palette[region] * weather[region]`. UV0 / UV1 (the skin
    /// weights) are kept for `peasant_model` to decode.
    pub fn people_part(&self, name: &str, palette: &[([f32; 4], crate::creature::Surf)], weather: &[[f32; 3]]) -> Option<Mesh> {
        use bevy::mesh::VertexAttributeValues;
        let m = self.model(name)?;
        let mut mesh = self.models[m].parts.first()?.mesh.clone();
        if let Some(VertexAttributeValues::Float32x4(cols)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
            for c in cols.iter_mut() {
                let region = ((c[0] * 16.0).floor() as usize).min(palette.len().saturating_sub(1));
                let (rgb, surf) = palette[region];
                let w = weather.get(region).copied().unwrap_or([1.0; 3]);
                let (cav, t) = (c[1], c[2].clamp(0.0, 1.0));
                let k = |i: usize| rgb[i] * (1.0 + (w[i] - 1.0) * t) * cav;
                *c = [k(0), k(1), k(2), crate::creature::surf_code(surf)];
            }
        }
        mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
        Some(mesh)
    }

    /// A CPU copy of kit part `name` with its baked vertex data untouched (region / cavity / tone /
    /// gloss in the colour, skin weights in UV0 / UV1) for a caller that decodes it itself (the
    /// skinned orks, `orks::kit_ork_body`).
    pub fn raw_part(&self, name: &str) -> Option<Mesh> {
        let m = self.model(name)?;
        let mut mesh = self.models[m].parts.first()?.mesh.clone();
        mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
        Some(mesh)
    }

    /// A recoloured CPU copy of animal part `name` (animals.glb, `tools/blender_forest/animals.py`).
    /// Vertex colour R = region (region/16), G = baked cavity / AO, B = coat TONE, A = coat TINT.
    /// Region 0 is fur: its colour walks the `coat` ramp by tone (0 = `coat[0]` dark saddle /
    /// points, 0.5 = `coat[1]` base coat, 1 = `coat[2]` pale belly) and then blends toward
    /// `coat[3]` (the warm tint) by the tint amount. Other regions take `palette[region]`. The
    /// mesh keeps its UV0 — the fur-flow coordinates the creature shader lays the strands along.
    pub fn coat_part(&self, name: &str, coat: &[[f32; 4]; 4], palette: &[([f32; 4], crate::creature::Surf)]) -> Option<Mesh> {
        use bevy::mesh::VertexAttributeValues;
        let m = self.model(name)?;
        let mut mesh = self.models[m].parts.first()?.mesh.clone();
        let mix = |a: [f32; 4], b: [f32; 4], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        if let Some(VertexAttributeValues::Float32x4(cols)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
            for c in cols.iter_mut() {
                let region = ((c[0] * 16.0).floor() as usize).min(palette.len().saturating_sub(1));
                let (flat, surf) = palette[region];
                let (tone, tint, cav) = (c[2].clamp(0.0, 1.0), c[3].clamp(0.0, 1.0), c[1]);
                let rgb = if region == 0 {
                    let ramp = if tone < 0.5 { mix(coat[0], coat[1], tone * 2.0) } else { mix(coat[1], coat[2], tone * 2.0 - 1.0) };
                    mix([ramp[0], ramp[1], ramp[2], 1.0], coat[3], tint)
                } else {
                    [flat[0], flat[1], flat[2]]
                };
                *c = [rgb[0] * cav, rgb[1] * cav, rgb[2] * cav, crate::creature::surf_code(surf)];
            }
        }
        mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
        Some(mesh)
    }

    /// Is there a kit model called `name`?
    pub fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// The parts (one mesh per material) of model `name` — for callers that edit a kit mesh
    /// before spawning it (`castle_kit::spawn_piece_edited`).
    pub fn parts_of(&self, name: &str) -> Option<&[Part]> {
        self.model(name).map(|m| self.models[m].parts.as_slice())
    }

    /// Is part material `mat` the kit material called `name`?
    pub fn mat_is(&self, mat: usize, name: &str) -> bool {
        self.mat_by_name(name) == Some(mat)
    }

    /// Insert kit material `mat` on `e` (pairs with [`Self::parts_of`]).
    pub fn insert_mat(&self, mat: usize, e: &mut EntityCommands) {
        self.mats[mat].insert(e);
    }

    /// (XZ half-extents, height) of kit model `name`, unscaled.
    pub fn extents(&self, name: &str) -> Option<(Vec2, f32)> {
        self.model(name).map(|m| (self.models[m].half, self.models[m].height))
    }

    /// Hang the static kit model `name` (all its material parts) under `e` as children.
    /// Returns false when the kit has no such model.
    pub fn attach_static(&self, e: &mut EntityCommands, name: &str, tf: Transform) -> bool {
        self.attach_static_with(e, name, tf, ())
    }

    /// [`Self::attach_static`], also inserting `extra` on every part (e.g. a `VisibilityRange`,
    /// which Bevy evaluates per mesh entity, not per hierarchy).
    pub fn attach_static_with<B: Bundle + Clone>(&self, e: &mut EntityCommands, name: &str, tf: Transform, extra: B) -> bool {
        let Some(m) = self.model(name) else { return false };
        e.insert(Visibility::default());
        e.with_children(|p| {
            for part in &self.models[m].parts {
                let mut c = p.spawn((Mesh3d(part.handle.clone()), tf, extra.clone()));
                self.mats[part.mat].insert(&mut c);
            }
        });
        true
    }

    /// [`Self::attach_static_with`] in two tiers: the full model to [`STATIC_LOD`], its meshopt
    /// twin from there to `far` (`autolod`). For the biome scatter's big static models (rock spires,
    /// saguaros, krummholz, cypress knees), which the render-distance setting stretches far.
    pub fn attach_static_lod(&self, e: &mut EntityCommands, name: &str, tf: Transform, far: f32, meshes: &mut Assets<Mesh>) -> bool {
        let Some(m) = self.model(name) else { return false };
        let range = |a: f32, b: f32| VisibilityRange { start_margin: a..a, end_margin: b..b, use_aabb: false };
        let split = far > STATIC_LOD && props_lod_enabled();
        e.insert(Visibility::default());
        let handles: Vec<_> = self.models[m].parts.iter().map(|p| split.then(|| p.lod_handle(meshes))).collect();
        e.with_children(|p| {
            for (part, lod) in self.models[m].parts.iter().zip(handles) {
                let near_end = if lod.is_some() { STATIC_LOD } else { far };
                let mut c = p.spawn((Mesh3d(part.handle.clone()), tf, range(0.0, near_end)));
                self.mats[part.mat].insert(&mut c);
                if let Some(h) = lod {
                    let mut c = p.spawn((Mesh3d(h), tf, range(STATIC_LOD, far)));
                    self.mats[part.mat].insert(&mut c);
                }
            }
        });
        true
    }

    /// Spawn the static kit model `name` as a new root entity at `tf`, `extra` on every part.
    pub fn spawn_static_with<B: Bundle + Clone>(&self, commands: &mut Commands, name: &str, tf: Transform, extra: B) -> Option<Entity> {
        self.model(name)?;
        let mut e = commands.spawn((tf, Visibility::default()));
        self.attach_static_with(&mut e, name, Transform::IDENTITY, extra);
        Some(e.id())
    }

    /// Spawn the static kit model `name` as a new root entity at `tf`.
    pub fn spawn_static(&self, commands: &mut Commands, name: &str, tf: Transform) -> Option<Entity> {
        self.model(name)?;
        let mut e = commands.spawn((tf, Visibility::default()));
        self.attach_static(&mut e, name, Transform::IDENTITY);
        Some(e.id())
    }

    /// The wild apple tree used by the forage orchard (`verbs::populate_apple_orchard`).
    pub fn apple_tree(&self, roll: f32) -> Option<usize> {
        self.model(if roll < 0.5 { "apple_a_lod0" } else { "apple_b_lod0" })
    }

    /// [`Self::attach_tree`], also inserting `fruit_tag` on every fruit part (both LODs) so
    /// gameplay can hide / show the apples (harvest + regrow).
    pub fn attach_tree_tagged<B: Bundle + Clone>(&self, e: &mut EntityCommands, lod0: usize, cull: bool, fruit_tag: B) {
        self.attach_tree_with(e, lod0, cull, fruit_tag, false, LOD_BAND);
    }

    /// [`Self::attach_tree`] with the snow-dusted material twins when `snow` (snow biome).
    pub fn attach_tree_snowy(&self, e: &mut EntityCommands, lod0: usize, cull: bool, snow: bool) {
        self.attach_tree_with(e, lod0, cull, (), snow, LOD_BAND);
    }

    fn attach_tree_with<B: Bundle + Clone>(&self, e: &mut EntityCommands, lod0: usize, cull: bool, fruit_tag: B, snow: bool, band: f32) {
        let fruit = self.mat_by_name("fruit");
        e.insert(Visibility::default());
        // Past `IMP_NEAR` a culled tree becomes a merged impostor quad (`impostor`) instead of
        // drawing LOD1 out to `TREE_FAR`.
        let imp = if cull { self.impostors.as_ref().and_then(|a| a.layer_of.get(&lod0).copied()) } else { None };
        if let Some(layer) = imp {
            e.insert(impostor::ImpostorTree { layer, snow: if snow { 0.75 } else { 0.0 } });
        }
        let far_end = if imp.is_some() { impostor::IMP_NEAR } else if cull { TREE_FAR } else { 1.0e6 };
        // An impostor tree keeps LOD0 right up to its impostor (baked from LOD0, so it matches):
        // the hand-built LOD1 has a visibly different crown, and LOD0 → LOD1 at 30u → impostor at
        // 62u read as two drastic crown swaps while running at the trees.
        let lod_near = if imp.is_some() { tree_lod_near().min(far_end) } else { LOD_NEAR };
        let lod1 = if lod_near >= far_end { None } else { self.models.get(lod0 + 1) };
        let near = VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: (lod_near - band)..(lod_near + band),
            use_aabb: false,
        };
        let far = VisibilityRange {
            start_margin: (lod_near - band)..(lod_near + band),
            end_margin: far_end..far_end,
            use_aabb: false,
        };
        let tf = Transform::from_scale(Vec3::splat(TREE_MODEL_SCALE));
        e.with_children(|p| {
            let lods = std::iter::once((&self.models[lod0], near.clone())).chain(lod1.map(|l| (l, far.clone())));
            for (model, range) in lods {
                for part in &model.parts {
                    let mut c = p.spawn((Mesh3d(part.handle.clone()), tf, range.clone()));
                    if imp.is_some() {
                        // The hand-off stretches with the render distance's ground factor only.
                        c.insert(crate::quality::SmallDetail);
                    }
                    match &self.canopy_fade {
                        Some((i, twin)) if imp.is_some() && !snow && *i == part.mat => twin.insert(&mut c),
                        _ => self.mat(part.mat, snow).insert(&mut c),
                    }
                    if Some(part.mat) == fruit {
                        c.insert((fruit_tag.clone(), Visibility::Visible));
                    }
                }
            }
        });
    }
}

/// Where an impostor tree's LOD0 hands over (`FOREST_TREELOD=<u>` for A/B; default = the impostor
/// start, i.e. no LOD1 at all).
fn tree_lod_near() -> f32 {
    static V: OnceLock<f32> = OnceLock::new();
    *V.get_or_init(|| std::env::var("FOREST_TREELOD").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(impostor::IMP_NEAR))
}

/// Two decorrelated uniform rolls hashed from a world position — the kit's variety picks, kept
/// OFF the scatter's sequential RNG so turning the kit on/off can't shift any later placement.
pub fn rolls(x: f32, z: f32) -> (f32, f32) {
    let h = |s: u32| {
        let mut v = (x * 64.0).round() as i32 as u32;
        v = v.wrapping_mul(0x9E37_79B1) ^ ((z * 64.0).round() as i32 as u32).wrapping_mul(0x85EB_CA77) ^ s;
        v ^= v >> 15;
        v = v.wrapping_mul(0x2C1B_3C6D);
        v ^= v >> 12;
        v = v.wrapping_mul(0x297A_2D39);
        v ^= v >> 15;
        (v >> 8) as f32 / (1u32 << 24) as f32
    };
    (h(0x1234_5678), h(0x0BAD_F00D))
}

impl Kit {
    /// Dense near-camera floor clutter over `[lo, hi]²` - the layer that stops a game forest floor
    /// reading as a flat painted plane: overlapping fallen-leaf sheets, pebbles and twigs lying ON
    /// the ground (real geometry with contact shadow), plus grass tufts colonising trail EDGES (the
    /// main cover scatter keeps a clear apron around roads). Three hashed rolls per tile, all off the
    /// scatter's RNG so the world layout is untouched. Cull distance [`CLUTTER_FAR`].
    pub fn scatter_clutter(
        &self,
        chunks: &mut Chunks,
        lo: f32,
        hi: f32,
        mask: &dyn Fn(f32, f32) -> bool,
        height_fn: &dyn Fn(f32, f32) -> f32,
        meadow: bool,
    ) {
        const LITTER: [&str; 4] = ["litter_a", "litter_b", "litter_c", "litter_d"];
        // Flat ground sheets (leaf litter, twigs) lie ALONG the slope: yawed-only, a 1u card on a
        // 30° knoll hangs its downhill half ~0.4u in the air. Tilt them onto the local normal
        // (central differences of the same `height_fn` the mesh is placed with) and sink a touch.
        let flat: Vec<usize> = LITTER.iter().chain(&["branches_a"]).filter_map(|n| self.model(n)).collect();
        let lie = |m: usize, x: f32, z: f32, yaw: f32| -> (Vec3, Quat) {
            let y = height_fn(x, z);
            if !flat.contains(&m) {
                return (Vec3::new(x, y, z), Quat::from_rotation_y(yaw));
            }
            let e = 0.4;
            let n = Vec3::new(height_fn(x - e, z) - height_fn(x + e, z), 2.0 * e, height_fn(x, z - e) - height_fn(x, z + e)).normalize();
            (Vec3::new(x, y - 0.02, z), Quat::from_rotation_arc(Vec3::Y, n) * Quat::from_rotation_y(yaw))
        };
        const GRASS: [&str; 4] = ["grass_a", "grass_b", "grass_c", "grass_d"];
        const PEBBLES: [&str; 6] = ["rock_01_a", "rock_01_c", "rock_01_e", "rock_02_b", "rock_02_d", "rock_02_f"];
        const FERN: [&str; 3] = ["fern_a", "fern_b", "fern_c"];
        const FLOWERS: [&str; 6] = ["flower_daisy", "flower_yellow", "flower_lupine_a", "flower_daisy", "flower_yellow", "flower_lupine_b"];
        const LUSH: [&str; 3] = ["lush_a", "lush_b", "lush_c"];
        let mut gx = lo;
        while gx < hi {
            let mut gz = lo;
            while gz < hi {
                for k in 0..3u32 {
                    let (a, b) = rolls(gx + k as f32 * 0.37, gz - k as f32 * 0.61);
                    let (c, d) = rolls(gx - k as f32 * 0.53, gz + k as f32 * 0.29);
                    let (x, z) = (gx + a, gz + b);
                    if !mask(x, z) || crate::ruins::near_landmark_visual_footprint(x, z) {
                        continue;
                    }
                    let road = crate::roads::road_strength(x, z);
                    let pick = |names: &[&str], s: f32| self.pick(names, d).map(|m| (m, s));
                    // Open meadow (`meadow`, the grass frontier): no forest floor here — no leaf
                    // sheets, dead branches or ferns away from the trees. Grass tufts thick on the
                    // verges and through the sward, the odd small stone.
                    let choice = if meadow {
                        if road > 0.55 {
                            if road <= 0.9 && c < 0.10 { pick(&GRASS, 0.5 + d * 0.3) } else { None }
                        } else if road > 0.12 {
                            if c < 0.55 { pick(&GRASS, 0.8 + d * 0.5) } else if c < 0.75 { pick(&FLOWERS, 0.85 + d * 0.4) } else { None }
                        } else if c < 0.50 {
                            // Clumped, not sprinkled (`meadow_clump`): `c/0.5` re-uses the roll.
                            let (keep, sc) = crate::biome::meadow_clump_gate(x, z);
                            if c * 2.0 < keep { pick(&GRASS, (0.7 + d * 0.5) * sc) } else { None }
                        } else if c < 0.56 && !crate::blockers::any_within(x, z, 0.35) {
                            pick(&PEBBLES, 0.12 + d * 0.14)
                        } else {
                            None
                        }
                    } else if road > 0.9 {
                        // The grass strip a cart track keeps between its two ruts.
                        if c < 0.14 { pick(&GRASS, 0.5 + d * 0.3) } else { None }
                    } else if road > 0.55 {
                        // Leaves blown onto the track.
                        if c < 0.22 { pick(&LITTER, 0.7 + d * 0.4) } else { None }
                    } else if road > 0.12 && c < 0.025 {
                        // The odd fallen trunk / mossy boulder / bush right at the verge.
                        if d < 0.4 {
                            pick(&["log_a"], 0.75 + d * 0.5)
                        } else if d < 0.75 {
                            pick(&PEBBLES, 0.4 + d * 0.3)
                        } else {
                            pick(&["bush_a", "bush_b"], 1.0 + d * 0.4)
                        }
                    } else if road > 0.12 {
                        // Trail verge: the lush band a forest path grows along — grass, ferns, flowers.
                        if c < 0.42 {
                            pick(&GRASS, 0.8 + d * 0.5)
                        } else if c < 0.68 {
                            pick(&FLOWERS, 0.85 + d * 0.4)
                        } else if c < 0.90 {
                            pick(&FERN, 0.9 + d * 0.5)
                        } else {
                            pick(&["stalks_a"], 0.9)
                        }
                    } else if c < 0.55 {
                        pick(&LITTER, 0.8 + d * 0.5)
                    } else if c < 0.67 && !crate::blockers::any_within(x, z, 0.35) {
                        pick(&PEBBLES, 0.12 + d * 0.16)
                    } else if c < 0.80 {
                        pick(&["branches_a"], 0.35 + d * 0.25)
                    } else if c < 0.86 {
                        pick(&GRASS, 0.6 + d * 0.3)
                    } else if c < 0.93 {
                        pick(&FERN, 0.8 + d * 0.5)
                    } else {
                        None
                    };
                    if let Some((m, s)) = choice {
                        let (pos, rot) = lie(m, x, z, c * 97.0 % std::f32::consts::TAU);
                        if chunks.push(self, m, pos, rot, s, Tier::Clutter) {
                            self.register_solid(m, x, z, s, rot);
                        }
                    }
                }
                // Undergrowth: up to 3 plants per tile off the trails, in DRIFTS — a broad noise field
                // opens real clearings (a forest floor is mostly litter with plant patches, not a
                // lawn), and the ground fertility thins it further on the bare/worn patches. Every
                // 2nd plant also goes into the far ring at 1.35x scale.
                let fert = crate::biome::ground_patch(gx + 0.5, gz + 0.5).fertility;
                // Glades (open, sunlit) grow a grass + flower sward; groves keep the drifts.
                let glade = if meadow { 1.0 } else { 1.0 - crate::biome::fg_smoothstep(0.08, 0.38, crate::biome::forest_grove(gx + 0.5, gz + 0.5)) };
                let drift = crate::biome::forest_drift(gx + 0.5, gz + 0.5).max(glade * 0.85);
                for k in 0..3u32 {
                    let (a, b) = rolls(gx + 0.13 + k as f32 * 0.71, gz + 0.29 - k as f32 * 0.43);
                    let (c, d) = rolls(gx - 0.31 - k as f32 * 0.17, gz + 0.47 + k as f32 * 0.59);
                    let (x, z) = (gx + a, gz + b);
                    // Open meadow: the sward gathers into clumps (tall heart, short rim) with clean
                    // ground between them (`meadow_clump`) instead of an even fertility sprinkle.
                    let (keep, clump_s) = if meadow { crate::biome::meadow_clump_gate(x, z) } else { (fert * drift, 1.0) };
                    if c > keep {
                        continue;
                    }
                    if !mask(x, z)
                        || crate::roads::road_strength(x, z) > 0.3
                        || crate::blockers::any_within(x, z, 0.3)
                        || crate::ruins::near_landmark_visual_footprint(x, z)
                    {
                        continue;
                    }
                    // Variety over volume: grass, ferns, short dry stalks, a low shrub, a fallen-leaf
                    // sheet, the occasional flower — each patch reads as a mixed understorey.
                    let (pick, s) = if meadow {
                        // The sward: lush grass clumps, wildflowers, seed stalks — no ferns.
                        const MEADOW_FLOWERS: [&str; 6] = ["flower_daisy", "flower_yellow", "flower_daisy", "flower_yellow", "flower_lupine_a", "flower_yellow"];
                        if d < 0.70 {
                            (self.pick(&LUSH, c), 0.6 + c * 0.5)
                        } else if d < 0.90 {
                            (self.pick(&MEADOW_FLOWERS, c), 0.8 + c * 0.3)
                        } else {
                            (self.pick(&["stalks_a"], c), 0.9)
                        }
                    } else if glade > 0.5 {
                        if d < 0.62 {
                            (self.pick(&LUSH, c), 0.6 + c * 0.5)
                        } else if d < 0.82 {
                            // Meadow flowers: mostly daisies + yellow, the odd lupine spike.
                            const GLADE_FLOWERS: [&str; 6] = ["flower_daisy", "flower_yellow", "flower_daisy", "flower_yellow", "flower_daisy", "flower_lupine_a"];
                            (self.pick(&GLADE_FLOWERS, c), 0.8 + c * 0.3)
                        } else if d < 0.92 {
                            (self.pick(&["stalks_a"], c), 0.9)
                        } else {
                            (self.pick(&FERN, c), 0.7 + c * 0.4)
                        }
                    } else if d < 0.42 {
                        (self.pick(&LUSH, c), 0.65 + c * 0.5)
                    } else if d < 0.68 {
                        (self.pick(&FERN, c), 0.8 + c * 0.6)
                    } else if d < 0.78 {
                        (self.pick(&["stalks_a"], c), 0.9)
                    } else if d < 0.85 {
                        (self.pick(&["bush_a", "bush_b"], c), 0.45 + c * 0.35)
                    } else if d < 0.94 {
                        (self.pick(&LITTER, c), 0.9 + c * 0.4)
                    } else {
                        (self.pick(&FLOWERS, c), 0.85)
                    };
                    let Some(m) = pick else { continue };
                    let s = s * clump_s;
                    let (pos, rot) = lie(m, x, z, d * 131.0 % std::f32::consts::TAU);
                    chunks.push(self, m, pos, rot, s, Tier::LushNear);
                    COVER_SPLATS.lock().expect("splats poisoned").push(Vec3::new(x, z, 0.30 * s));
                    if k % 2 == 0 && d < 0.85 {
                        chunks.push(self, m, pos, rot, s * 1.35, Tier::LushFar);
                    }
                }
                gz += 1.0;
            }
            gx += 1.0;
        }
    }
}

impl Kit {
    /// Shoreline dressing on the eroded river / lake / pool banks ([`crate::worldmap::shore_sample`]):
    /// small pebbles strewn along the waterline (half drowned — the slope runs on under the water),
    /// the odd bigger stone down the damp face, and grass / fern tufts hanging over the lip. Five
    /// hashed probes per tile, all off the scatter's RNG; off the banks a probe is one mask lookup.
    /// Lands in the near-clutter merge buckets (same cull distance, no extra draw groups).
    pub fn scatter_shore(
        &self,
        chunks: &mut Chunks,
        lo: f32,
        hi: f32,
        mask: &dyn Fn(f32, f32) -> bool,
    ) {
        const PEBBLES: [&str; 6] = ["rock_01_a", "rock_01_c", "rock_01_e", "rock_02_b", "rock_02_d", "rock_02_f"];
        const GRASS: [&str; 4] = ["grass_a", "grass_b", "grass_c", "grass_d"];
        const FERN: [&str; 3] = ["fern_a", "fern_b", "fern_c"];
        let mut gx = lo;
        while gx < hi {
            let mut gz = lo;
            while gz < hi {
                for k in 0..5u32 {
                    let (a, b) = rolls(gx + 0.71 + k as f32 * 0.23, gz - 0.37 + k as f32 * 0.57);
                    let (c, d) = rolls(gx - 0.19 - k as f32 * 0.41, gz + 0.83 + k as f32 * 0.13);
                    let (x, z) = (gx + a, gz + b);
                    let Some((sd, bank, ground)) = crate::worldmap::shore_sample(x, z) else { continue };
                    if sd < 0.04 || crate::bridges::near_bridge(x, z, 1.0) || crate::ruins::near_landmark_visual_footprint(x, z) {
                        continue;
                    }
                    // The lower slope runs out over the water TILE beside the bank, which belongs to
                    // no biome: judge such a point by its first land neighbour (fixed order, so
                    // exactly one biome pass claims it).
                    let land = |px: f32, pz: f32| crate::worldmap::is_land_tile_world(px, pz);
                    let owner = if land(x, z) {
                        Some((x, z))
                    } else {
                        [(0.9f32, 0.0f32), (-0.9, 0.0), (0.0, 0.9), (0.0, -0.9)]
                            .into_iter()
                            .map(|(ox, oz)| (x + ox, z + oz))
                            .find(|&(px, pz)| land(px, pz))
                    };
                    let Some((ox, oz)) = owner else { continue };
                    if !mask(ox, oz) {
                        continue;
                    }
                    let pick = |names: &[&str], s: f32| self.pick(names, d).map(|m| (m, s));
                    let (choice, sink) = if sd < 0.6 {
                        // Waterline gravel: a loose string of small stones, thinning out of the water.
                        (if c < 0.62 - sd * 0.6 { pick(&PEBBLES, 0.07 + d * 0.11) } else { None }, 0.35)
                    } else if bank > 0.3 && sd < 2.4 {
                        // The odd stone washed out of the face.
                        (if c < 0.07 { pick(&PEBBLES, 0.15 + d * 0.18) } else { None }, 0.3)
                    } else if bank > 0.01 {
                        // The grass / fern fringe overhanging the lip.
                        (
                            if c < 0.30 {
                                pick(&GRASS, 0.55 + d * 0.45)
                            } else if c < 0.40 {
                                pick(&FERN, 0.65 + d * 0.4)
                            } else {
                                None
                            },
                            0.0,
                        )
                    } else {
                        (None, 0.0)
                    };
                    let Some((m, s)) = choice else { continue };
                    let rot = Quat::from_rotation_y(c * 113.0 % std::f32::consts::TAU);
                    // Stones sit IN the ground (a fraction of their own height), not perched on it.
                    // `ground` is the bank surface itself — also where the slope runs out over the
                    // water tile, which the general ground sampler has no footing for.
                    let y = ground - self.models[m].height * s * sink;
                    chunks.push(self, m, Vec3::new(x, y, z), rot, s, Tier::Clutter);
                }
                gz += 1.0;
            }
            gx += 1.0;
        }
    }
}

// ── Chunk merging for passive props / ground cover ───────────────────────────────────────────

/// Queued kit props, grouped by (16u chunk, cover?, material) so each group merges to one mesh.
#[derive(Default)]
pub struct Chunks {
    /// (chunk, tier, density slice, material, snow twin?) -> queued parts.
    groups: HashMap<((i32, i32), Tier, u8, usize, bool), Vec<(usize, usize, Transform)>>,
}

/// Undergrowth density slice a merged chunk belongs to. Every plant / clutter piece is hashed into
/// one of three slices (45% / 27% / 28%) and each slice merges into its own mesh, so the Settings
/// "Undergrowth" level just shows or hides whole slices at runtime — no rebuild, and a hidden
/// slice costs nothing. Props (rocks, logs, stumps, bushes) are always slice 0.
#[derive(Component, Clone, Copy)]
pub struct DensitySlice(pub u8);

fn density_slice(tier: Tier, world: Vec3) -> u8 {
    if matches!(tier, Tier::Props | Tier::PropsFar) {
        return 0;
    }
    let (e, _) = rolls(world.x * 1.7 + 0.13, world.z * 1.3 - 0.71);
    if e < 0.45 {
        0
    } else if e < 0.72 {
        1
    } else {
        2
    }
}

/// Show the slices the Settings undergrowth level asks for (Low 1 / Medium 2 / High 3 slices).
fn apply_undergrowth(
    settings: Res<crate::quality::GraphicsSettings>,
    mut q: Query<(Ref<DensitySlice>, &mut Visibility)>,
) {
    let all = settings.is_changed();
    let keep = settings.undergrowth.slices();
    for (slice, mut vis) in &mut q {
        if !all && !slice.is_added() {
            continue;
        }
        let want = if slice.0 < keep { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

fn chunk_key(x: f32, z: f32) -> (i32, i32) {
    let c = crate::biome::COVER_CHUNK;
    ((x / c).floor() as i32, (z / c).floor() as i32)
}

fn chunk_center(k: (i32, i32)) -> Vec3 {
    let c = crate::biome::COVER_CHUNK;
    Vec3::new(k.0 as f32 * c + c * 0.5, 0.0, k.1 as f32 * c + c * 0.5)
}

/// Kit scatter must stay out of the gameplay set-pieces placed later in the world build: ork
/// camps (tents, cage, fire — plus a margin for guy ropes), roadside story clearings, the planned
/// wayside furniture (signposts / cairns / fences / shrines) and forest vignette, and the
/// landmarks. Merged chunks can't be thinned afterwards the way single trees are, so it is
/// decided here, at placement.
pub fn keep_clear(x: f32, z: f32, r: f32) -> bool {
    (!crate::camps::plan().is_empty() && crate::camps::near_camp(x, z, r + 0.8))
        || crate::poi::overlaps_planned_clearing(x, z, r)
        || crate::wayside::overlaps_planned(x, z, r)
        || crate::vignettes::overlaps_planned(x, z, r)
        || crate::ruins::near_landmark_visual_footprint(x, z)
        || crate::castle::village_keep_clear(x, z, r)
        || crate::countryside::overlaps_planned(x, z, r)
}

impl Chunks {
    /// Queue a kit prop for its chunk merge. Returns false (nothing queued) where the spot must
    /// stay clear of a set-piece ([`keep_clear`]).
    pub fn push(&mut self, kit: &Kit, model: usize, world: Vec3, rot: Quat, scale: f32, tier: Tier) -> bool {
        self.push_snowy(kit, model, world, rot, scale, tier, false)
    }

    /// [`Self::push`], drawn with the snow-dusted material twins when `snow`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_snowy(&mut self, kit: &Kit, model: usize, world: Vec3, rot: Quat, scale: f32, tier: Tier, snow: bool) -> bool {
        if keep_clear(world.x, world.z, kit.models[model].half.max_element() * scale) {
            return false;
        }
        if far_cover::enabled() {
            far_cover::record(far_cover::swatch_for(kit, model), world, scale, tier);
        }
        let key = chunk_key(world.x, world.z);
        let t = Transform { translation: world - chunk_center(key), rotation: rot, scale: Vec3::splat(scale) };
        let slice = density_slice(tier, world);
        for (pi, part) in kit.models[model].parts.iter().enumerate() {
            self.groups.entry((key, tier, slice, part.mat, snow)).or_default().push((model, pi, t));
            if tier == Tier::Props && props_lod_enabled() {
                self.groups.entry((key, Tier::PropsFar, slice, part.mat, snow)).or_default().push((model, pi, t));
            }
        }
        if tier == Tier::Props {
            splat(world.x, world.z, 0.55 * scale);
        }
        true
    }

    pub fn spawn(self, kit: &Kit, commands: &mut Commands, meshes: &mut Assets<Mesh>, cull: bool) {
        SCATTER_DONE.store(true, std::sync::atomic::Ordering::Release);
        // Streamed (the default): park the recipes; `stream_cover` merges + spawns only the chunks
        // near the camera. Every tier is distance-culled at <= PROPS_FAR anyway, so a far chunk
        // was RAM/VRAM and upload time that could never be seen (~39M vertices at MAP_SCALE 3.9).
        if cull && cover_stream_enabled() {
            let mut st = STREAM.lock().expect("cover stream poisoned");
            for ((key, tier, slice, mat, snow), items) in self.groups {
                st.chunks.entry(key).or_default().push(Recipe { tier, slice, mat, snow, items });
            }
            st.dirty = true;
            return;
        }
        // Merge every group in parallel (pure vertex work — this was most of each scatter phase),
        // then spawn serially in the groups' iteration order.
        let groups: Vec<_> = self.groups.into_iter().collect();
        let merged = crate::par::par_map(groups.len(), |gi| merge_group(kit, &groups[gi].1, groups[gi].0 .1 == Tier::PropsFar));
        for (((key, tier, slice, mat, snow), _), base) in groups.into_iter().zip(merged) {
            if let Some(base) = base {
                let r = Recipe { tier, slice, mat, snow, items: Vec::new() };
                spawn_group(kit, commands, meshes, key, &r, base, cull);
            }
        }
    }
}

/// Merge one group's queued kit parts into a single chunk-local mesh (`None` if empty).
fn merge_group(kit: &Kit, items: &[(usize, usize, Transform)], lod: bool) -> Option<Mesh> {
    let mesh = |m: usize, p: usize| {
        let part = &kit.models[m].parts[p];
        if lod { part.lod_mesh() } else { &part.mesh }
    };
    let mut it = items.iter();
    let &(m0, p0, t0) = it.next()?;
    let mut base = mesh(m0, p0).clone().transformed_by(t0);
    for &(m, p, t) in it {
        let part = mesh(m, p).clone().transformed_by(t);
        if base.merge(&part).is_err() {
            warn!("realforest: chunk merge failed (attribute mismatch) — part skipped");
        }
    }
    Some(base)
}

/// Spawn one merged chunk group (mesh already merged). Returns the entity.
fn spawn_group(
    kit: &Kit,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    key: (i32, i32),
    r: &Recipe,
    mut base: Mesh,
    cull: bool,
) -> Entity {
    // GPU-only: the merged chunk is never read back on the CPU (the reskin pass inspects it in
    // PostUpdate of its spawn frame, before extraction), so drop the main-world copy once
    // it's uploaded. These merges were ~3 GiB of resident RAM at MAP_SCALE 3.9.
    base.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    let (near, far) = match r.tier {
        Tier::Props if props_lod_enabled() => (0.0, PROPS_LOD),
        Tier::Props => (0.0, PROPS_FAR),
        Tier::PropsFar => (PROPS_LOD, PROPS_FAR),
        Tier::Cover => (0.0, COVER_FAR),
        Tier::Clutter => (0.0, CLUTTER_FAR),
        Tier::LushNear => (0.0, LUSH_NEAR),
        // Starts a band early: it thins in under the near ring's thin-out (`DISSOLVE_BAND`).
        Tier::LushFar => (LUSH_NEAR - dissolve_band(), LUSH_FAR),
    };
    // Origin on the AABB centre: the range test (`use_aabb: false`) and the shader's dissolve
    // (`foliage_wind_prepass.wesl`, which only sees the entity origin) measure the same distance.
    let centre = match base.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) if !p.is_empty() => {
            let (lo, hi) = p.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| (lo.min(Vec3::from(*v)), hi.max(Vec3::from(*v))));
            (lo + hi) * 0.5
        }
        _ => Vec3::ZERO,
    };
    base.translate_by(-centre);
    let mut e = commands.spawn((
        Mesh3d(meshes.add(base)),
        Transform::from_translation(chunk_center(key) + centre),
        bevy::light::NotShadowCaster,
        crate::biome::BiomeEntity,
        DensitySlice(r.slice),
        Visibility::default(),
    ));
    kit.mat(r.mat, r.snow).insert(&mut e);
    if !matches!(r.tier, Tier::Props | Tier::PropsFar) {
        e.insert(crate::biome::GroundCoverChunk);
    }
    if cull || near > 0.0 {
        let far = if cull { far } else { 1.0e6 };
        e.insert(VisibilityRange { start_margin: near..near, end_margin: far..far, use_aabb: false });
    }
    e.id()
}

// ── Cover streaming ───────────────────────────────────────────────────────────────────────────
//
// The kit undergrowth / clutter / props (every `Chunks` group) used to be merged and spawned for
// the WHOLE island at boot — ~39M vertices at MAP_SCALE 3.9 — although each tier is culled past
// <= PROPS_FAR (75u). Now `Chunks::spawn` parks compact recipes (model, part, transform) per 16u
// chunk, and [`stream_cover`] merges + spawns only the chunks within the [`stream_radii`] spawn radius of the camera,
// despawning those past its despawn radius (hysteresis). Placement is still decided once at build
// time, so a chunk respawns identically. `FOREST_NOCULL=1` / `FOREST_NOSTREAM=1` restore the
// spawn-everything path.

/// One merge group's recipe: what to merge, and how to draw it.
struct Recipe {
    tier: Tier,
    slice: u8,
    mat: usize,
    snow: bool,
    items: Vec<(usize, usize, Transform)>,
}

#[derive(Default)]
struct CoverStream {
    /// Every chunk's recipes (resident, compact).
    chunks: HashMap<(i32, i32), Vec<Recipe>>,
    /// Chunks currently spawned -> their entities.
    live: HashMap<(i32, i32), Vec<Entity>>,
    /// Chunks whose merge is running on the async compute pool (one mesh per recipe, in order).
    /// Merging off the main thread keeps a moving camera from ever stalling a frame on it.
    pending: HashMap<(i32, i32), bevy::tasks::Task<Vec<Option<Mesh>>>>,
    /// Recipes changed (or the last pass hit its per-frame budget) — re-evaluate even if the
    /// camera hasn't moved.
    dirty: bool,
}

static STREAM: std::sync::LazyLock<std::sync::Mutex<CoverStream>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(CoverStream::default()));

/// (spawn, despawn) radii, camera XZ -> chunk centre. Spawn: the farthest tier culls at PROPS_FAR
/// measured to the chunk AABB, so the centre test gets a chunk half-diagonal (~11.3u) plus slack
/// (95u at Normal). Despawn: past spawn by enough that pacing along the boundary doesn't thrash.
/// The Settings render distance widens the spawn radius (`quality::cover_view_scale`, x1 at
/// Normal); the per-frame budgets below are unchanged, so a wider ring just fills over more frames.
fn stream_radii() -> (f32, f32) {
    let stream_in = PROPS_FAR * crate::quality::cover_view_scale() + 20.0;
    (stream_in, stream_in + 24.0)
}
/// Most chunk KEYS whose async merge is launched per frame (nearest first); the rest follow next
/// frame. The boot radius (~110 keys) is queued within ~4 frames.
const STREAM_BUDGET: usize = 32;
/// Vertices of finished chunk merges landed (spawned + uploaded) per frame — see `stream_cover`.
const LAND_BUDGET_VERTS: usize = 80_000;

/// Is cover streaming on? (`FOREST_NOSTREAM=1` turns it off; `FOREST_NOCULL` bypasses it too.)
pub fn cover_stream_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_NOSTREAM").is_err())
}

/// Forget every recipe + live chunk — the world build calls this at phase 0 (the old chunk
/// entities are `BiomeEntity`s, already wiped by the rebuild).
pub fn reset_cover_stream() {
    let mut st = STREAM.lock().expect("cover stream poisoned");
    st.chunks.clear();
    st.live.clear();
    st.pending.clear(); // dropping a Task cancels it
    st.dirty = false;
}

fn stream_cover(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    ready: Res<crate::biome::WorldReady>,
    mut last: Local<Option<Vec2>>,
    mut last_in: Local<f32>,
) {
    // Idle while a world (re)build is in flight: an in-process rebuild clears `WorldReady` before
    // it wipes the `BiomeEntity`s, so a chunk streamed in from the OLD recipes can never slip past
    // that wipe as an orphan (the build resets the store at phase 0).
    if !ready.0 {
        return;
    }
    let Some(kit) = get() else { return };
    let Some(cam) = cams.iter().next() else { return };
    let t = cam.translation();
    let p = Vec2::new(t.x, t.z);
    let mut guard = STREAM.lock().expect("cover stream poisoned");
    let CoverStream { chunks, live, pending, dirty } = &mut *guard;
    if chunks.is_empty() && live.is_empty() {
        return;
    }
    let centre = |k: &(i32, i32)| {
        let c = chunk_center(*k);
        Vec2::new(c.x, c.z)
    };
    // Land finished merges (spawned whole-chunk at once, so a chunk never appears half-built),
    // nearest first and at most LAND_BUDGET_VERTS per frame (always at least one chunk): landing
    // every finished chunk in one frame put a whole ring of chunks through Bevy's mesh upload at
    // once — 20-40 ms `allocate_and_free_meshes` hitches while walking. The rest land next frame.
    let mut done: Vec<((i32, i32), f32)> =
        pending.iter().filter(|(_, t)| t.is_finished()).map(|(k, _)| (*k, centre(k).distance(p))).collect();
    done.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut landed_verts = 0usize;
    for (key, _) in done {
        if landed_verts >= LAND_BUDGET_VERTS {
            break; // still in `pending`, so next frame lands it (this runs before the idle early-out)
        }
        let Some(task) = pending.remove(&key) else { continue };
        let merged = bevy::tasks::block_on(task);
        let ents = live.entry(key).or_default();
        if let Some(recipes) = chunks.get(&key) {
            for (r, mesh) in recipes.iter().zip(merged) {
                if let Some(mesh) = mesh {
                    landed_verts += mesh.count_vertices();
                    ents.push(spawn_group(kit, &mut commands, &mut meshes, key, r, mesh, true));
                }
            }
        }
    }
    let (stream_in, stream_out) = stream_radii();
    // A render-distance change re-evaluates the ring even with the camera parked.
    if !*dirty && pending.is_empty() && last.is_some_and(|l| l.distance(p) < 2.0) && *last_in == stream_in {
        return;
    }
    *last = Some(p);
    *last_in = stream_in;
    live.retain(|k, ents| {
        if centre(k).distance(p) <= stream_out {
            return true;
        }
        for e in ents.drain(..) {
            commands.entity(e).try_despawn();
        }
        false
    });
    // A merge for a chunk the camera already left is dropped (cancelled).
    pending.retain(|k, _| centre(k).distance(p) <= stream_out);
    let mut need: Vec<((i32, i32), f32)> = chunks
        .keys()
        .filter(|k| !live.contains_key(*k) && !pending.contains_key(*k))
        .map(|k| (*k, centre(k).distance(p)))
        .filter(|(_, d)| *d <= stream_in)
        .collect();
    need.sort_by(|a, b| a.1.total_cmp(&b.1));
    *dirty = need.len() > STREAM_BUDGET;
    need.truncate(STREAM_BUDGET);
    let pool = bevy::tasks::AsyncComputeTaskPool::get();
    for (key, _) in need {
        let items: Vec<(Vec<(usize, usize, Transform)>, bool)> =
            chunks[&key].iter().map(|r| (r.items.clone(), r.tier == Tier::PropsFar)).collect();
        let task = pool.spawn(async move { items.iter().map(|(it, lod)| merge_group(kit, it, *lod)).collect::<Vec<_>>() });
        pending.insert(key, task);
    }
}

// ── Loading ────────────────────────────────────────────────────────────────────────────────────

pub struct RealForestPlugin;

impl Plugin for RealForestPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ForestLook>();
        app.add_plugins((reskin::ReskinPlugin, foliage::FoliagePlugin));
        // Cover streaming (see `stream_cover`): ungated — the camera can move under any modal.
        app.add_systems(Update, stream_cover);
        if !enabled() {
            return;
        }
        // Before the world build (Startup) reads the kit inside `biome::scatter_region`.
        app.add_plugins(impostor::ImpostorPlugin);
        app.add_systems(PreStartup, build_kit)
            .add_systems(Update, (patch_terrain, drive_forest_look, apply_undergrowth, bind_far_cover.after(patch_terrain)));
    }
}

/// 0..1 "the camera is in the realistic forest" weight, eased. The post stack reads it to give the
/// forest its own look — clearer air (`atmospherics`), richer colour (`grade`), stronger sun shafts
/// (`godrays`) — without touching the tuned look of every other biome.
#[derive(Resource, Default)]
pub struct ForestLook(pub f32);

fn drive_forest_look(
    time: Res<Time>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut look: ResMut<ForestLook>,
) {
    let Some(cam) = cams.iter().next() else { return };
    let p = cam.translation();
    let target = if get().is_some()
        && crate::worldmap::tile_biome_world(p.x, p.z) == Some(crate::biome::Biome::Forest)
    {
        1.0
    } else {
        0.0
    };
    let k = (time.delta_secs() * 1.2).min(1.0);
    let v = look.0 + (target - look.0) * k;
    if (v - look.0).abs() > 1e-4 {
        look.0 = v;
    }
}

pub(crate) fn pbr_material(images: &mut Assets<Image>, set: &str, card: bool) -> Result<StandardMaterial, String> {
    if set == "glow" {
        return Ok(StandardMaterial { base_color: Color::LinearRgba(LinearRgba::rgb(2.5, 2.5, 2.5)), unlit: true, ..default() });
    }
    if set == "ice" {
        return Ok(StandardMaterial { perceptual_roughness: 0.22, reflectance: 0.3, ..default() });
    }
    if set == "bronze" {
        return Ok(StandardMaterial { metallic: 1.0, perceptual_roughness: 0.5, reflectance: 0.5, ..default() });
    }
    if set == "bogwater" {
        // Black and still like the swamp's own pools (water.wgsl's tannin bog): a near-black body
        // with a faint, crisp sheen. A rough surface spread the baked grey sky over the whole pool
        // (it read as grey satin); a glossy, low-F0 one stays dark and only glints at grazing.
        // The baked environment is a dim grey sky, so the sheen is tinted down too, or the pool reads
        // as a grey plate at a grazing eye-height view.
        return Ok(StandardMaterial {
            perceptual_roughness: 0.08,
            reflectance: 0.14,
            specular_tint: Color::srgb(0.42, 0.44, 0.46),
            ..default()
        });
    }
    if set.is_empty() {
        // Waxy fruit skin: base colour comes from the vertex colours.
        return Ok(StandardMaterial { perceptual_roughness: 0.42, reflectance: 0.45, ..default() });
    }
    let albedo = images.add(load::texture(&format!("forest_pbr/{set}_albedo.png"), true, card.then_some(CARD_CUTOFF))?);
    let normal = images.add(load::texture(&format!("forest_pbr/{set}_normal.png"), false, None)?);
    if card {
        return Ok(StandardMaterial {
            base_color_texture: Some(albedo),
            normal_map_texture: Some(normal),
            alpha_mode: AlphaMode::Mask(CARD_CUTOFF),
            // Draw both faces but keep ONE normal for both: the cards carry crown-outward bent
            // normals, and `double_sided` would flip them on the back face (dark inner leaves).
            cull_mode: None,
            double_sided: false,
            perceptual_roughness: 0.62,
            reflectance: 0.32,
            // No `diffuse_transmission`: on a crown of overlapping alpha-tested cards (no early-z,
            // every layer shaded) the extra back-lobe light pass cost ~3.6ms of the opaque pass at
            // 1080p (8.5 -> 4.9ms measured). Bent crown normals + vertex AO carry the volume.
            ..default()
        });
    }
    let ormh = images.add(load::texture(&format!("forest_pbr/{set}_ormh.png"), false, None)?);
    // `FOREST_NONORMAL=s_rubble,...`: A/B knob, drop these sets' normal maps.
    let no_normal = std::env::var("FOREST_NONORMAL").is_ok_and(|v| v.split(',').any(|s| s.trim() == set));
    let mut m = StandardMaterial {
        base_color_texture: Some(albedo),
        normal_map_texture: (!no_normal).then_some(normal),
        occlusion_texture: Some(ormh.clone()),
        metallic_roughness_texture: Some(ormh),
        // Both act as multipliers on the texture channels (G roughness, B metal = 0).
        perceptual_roughness: 1.0,
        metallic: 1.0,
        reflectance: 0.35,
        ..default()
    };
    if set == "ph_shrub_02" {
        // The shrub scan is single-layer leaf geometry: draw both faces, lit with the flipped normal.
        m.cull_mode = None;
        m.double_sided = true;
    }
    Ok(m)
}

fn try_build(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    foliage_mats: &mut Assets<foliage::FoliageMaterial>,
    mut snow_mats: Option<&mut Assets<crate::alpine::snowkit::SnowKitMaterial>>,
    images: &mut Assets<Image>,
) -> Result<Kit, String> {
    let mut mats = Vec::new();
    let mut canopy_fade = None;
    let mut snow_twins = Vec::new();
    let mut mat_index = HashMap::new();
    for (name, set, card) in MATERIALS {
        mat_index.insert(name.to_string(), mats.len());
        let mut m = pbr_material(images, set, card)?;
        if name == "s_timber_matte" {
            m.metallic_roughness_texture = None;
            m.metallic = 0.0;
            m.perceptual_roughness = 0.93;
            m.reflectance = 0.16;
        }
        // Snow-dusted twin for the snow biome (`alpine::snowkit`): same textures, crown tint kept.
        let tint = match name {
            "canopy" => 0.6,
            "undergrowth" => 0.35,
            _ => 0.0,
        };
        snow_twins.push(match (crate::alpine::snowkit::snow_amount(name), snow_mats.as_deref_mut()) {
            (Some(amount), Some(sm)) => Some(KitMat::Snow(sm.add(crate::alpine::snowkit::twin(&m, tint, amount)))),
            _ => None,
        });
        // Leaf cards sway a little at the crown edge, undergrowth blades more at the tips; plus
        // crown / undergrowth colour variety (see `foliage`).
        if name == "canopy" {
            let twin = foliage::dissolve(foliage::material(m.clone(), 0.025, 1.2, 0.6), impostor::IMP_FADE_IN);
            canopy_fade = Some((mats.len(), KitMat::Foliage(foliage_mats.add(twin))));
        }
        mats.push(match name {
            "canopy" => KitMat::Foliage(foliage_mats.add(foliage::material(m, 0.025, 1.2, 0.6))),
            "undergrowth" => KitMat::Foliage(foliage_mats.add(foliage::dissolve(
                // Root lift 0.35 (was 0.8): the ground under the carpet is now earthy, not a lawn, so
                // blade bases stay dark and shaded - a clump grows out of the soil instead of
                // starting in a pale straw halo that read as a card standing on the ground.
                foliage::ground_match(foliage::material(m, 0.035, 2.5, 0.35), 0.35, 0.6),
                DISSOLVE_BAND,
            ))),
            // Hanging moss / willow whips swing a touch more than a crown edge.
            "swamp_cards" => KitMat::Foliage(foliage_mats.add(foliage::material(m, 0.03, 1.6, 0.4))),
            _ => KitMat::Std(materials.add(m)),
        });
    }
    let wind = [mat_index["canopy"], mat_index["undergrowth"], mat_index["swamp_cards"]];
    let mut models = Vec::new();
    let mut by_name = HashMap::new();
    for glb in ["forest_pbr/trees.glb", "forest_pbr/undergrowth.glb", "forest_pbr/props.glb", "forest_pbr/structures.glb", "forest_pbr/wayside.glb", "forest_pbr/stories_warm.glb", "forest_pbr/creatures.glb", "forest_pbr/animals.glb", "forest_pbr/people.glb", "forest_pbr/orks.glb", "forest_pbr/stories_cold.glb", "forest_pbr/snowmen.glb", "forest_pbr/wardens.glb", "forest_pbr/alpine_flora.glb", "forest_pbr/market.glb", "forest_pbr/castle_kit.glb", "forest_pbr/town_kit.glb", "forest_pbr/biome_props.glb", "forest_pbr/swamp_flora.glb", "forest_pbr/ork_camps.glb", "forest_pbr/sites_warm.glb", "forest_pbr/rival_kit.glb", "forest_pbr/stories_castaways.glb", "forest_pbr/landmarks.glb", "forest_pbr/ork_hold.glb", "forest_pbr/wild_sites.glb", "forest_pbr/bridges_kit.glb", "forest_pbr/meadow_kit.glb", "forest_pbr/caravan.glb", "forest_pbr/oddities.glb", "forest_pbr/stashes.glb", "forest_pbr/swamp_folk.glb", "forest_pbr/dl_creatures.glb", "forest_pbr/scanned_props.glb", "forest_pbr/wreck_cove.glb"] {
        // The townsfolk parts (people.glb) and the skinned ork bodies (orks.glb) are optional:
        // without them the villagers keep their box-built bodies (`peasant_model`) and the orks
        // their per-joint creatures.glb parts; the rest of the kit still loads.
        if (glb.ends_with("people.glb") || glb.ends_with("orks.glb") || glb.contains("/stories_") || glb.contains("/sites_") || glb.ends_with("snowmen.glb") || glb.ends_with("wardens.glb") || glb.ends_with("alpine_flora.glb") || glb.ends_with("market.glb") || glb.ends_with("castle_kit.glb") || glb.ends_with("town_kit.glb") || glb.ends_with("biome_props.glb") || glb.ends_with("swamp_flora.glb") || glb.ends_with("ork_camps.glb") || glb.ends_with("rival_kit.glb") || glb.ends_with("landmarks.glb") || glb.ends_with("ork_hold.glb") || glb.ends_with("wild_sites.glb") || glb.ends_with("bridges_kit.glb") || glb.ends_with("meadow_kit.glb") || glb.ends_with("caravan.glb") || glb.ends_with("oddities.glb") || glb.ends_with("stashes.glb") || glb.ends_with("swamp_folk.glb") || glb.ends_with("dl_creatures.glb") || glb.ends_with("scanned_props.glb") || glb.ends_with("wreck_cove.glb"))
            && !load::asset_path(glb).exists()
        {
            warn!("realforest: {glb} missing — falling back to the older bodies");
            continue;
        }
        let mut objs = load::read_glb(glb)?;
        // `attach_tree` finds a tree's LOD1 at `lod0 + 1`, so keep each _lod0 / _lod1 pair adjacent.
        objs.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, prims) in objs {
            let mut raw = Vec::new();
            for p in prims {
                let mat = *mat_index.get(&p.material).ok_or_else(|| format!("{name}: unknown material {}", p.material))?;
                raw.push((p.mesh, mat));
            }
            // Swamp crowns: thinner, gappier, olive / rust — see `biomekit::thin_swamp_crown`.
            for (mesh, mat) in raw.iter_mut() {
                if *mat == mat_index["swamp_cards"] {
                    biomekit::thin_swamp_crown(&name, mesh);
                }
            }
            {
                let mut view: Vec<(&mut Mesh, bool)> = raw.iter_mut().map(|(m, i)| (m, wind.contains(i))).collect();
                foliage::bake_weights(&mut view);
            }
            let parts: Vec<Part> = raw
                .into_iter()
                .map(|(mesh, mat)| Part { handle: meshes.add(mesh.clone()), mesh, mat, lod: OnceLock::new(), lod_handle: OnceLock::new() })
                .collect();
            let (half, trunk, height) = model_extents(&parts, &mats);
            let solid = ["rock_", "log_", "stump_"].iter().any(|p| name.starts_with(p));
            if std::env::var("FOREST_KITDUMP").is_ok() {
                info!("kit model {name}: half {:.2}x{:.2} height {:.2} trunk {:.2}", half.x, half.y, height, trunk);
            }
            by_name.insert(name, models.len());
            models.push(Model { parts, half, trunk, solid, height });
        }
    }
    let ground = GroundTextures {
        albedo: images.add(load::texture_array("forest_pbr/ground_albedo.dds", &LAYERS.map(|l| format!("forest_pbr/ground_{l}_albedo.png")).each_ref().map(|s| s.as_str()), true)?),
        normal: images.add(load::texture_array("forest_pbr/ground_normal.dds", &LAYERS.map(|l| format!("forest_pbr/ground_{l}_normal.png")).each_ref().map(|s| s.as_str()), false)?),
        ormh: images.add(load::texture_array("forest_pbr/ground_ormh.dds", &LAYERS.map(|l| format!("forest_pbr/ground_{l}_ormh.png")).each_ref().map(|s| s.as_str()), false)?),
    };
    // Scanned creature surface layers (orks, villagers, wildlife) — optional.
    let creature = ["albedo", "ormh"].map(|m| {
        let pngs = ["skin", "fur", "leather", "wood", "metal", "cloth", "bone"].map(|l| format!("forest_pbr/creature_{l}_{m}.png"));
        load::texture_array(&format!("forest_pbr/creature_{m}.dds"), &pngs.each_ref().map(|s| s.as_str()), m == "albedo")
    });
    match creature {
        [Ok(a), Ok(o)] => crate::creature::set_detail_layers(images.add(a), images.add(o)),
        _ => warn!("realforest: creature detail layers missing — creatures keep the procedural surfaces"),
    }
    // Townsfolk garment layers (linen, wool, leather, skin) — optional, packed from people_tex.py.
    let people = ["albedo", "ormh"].map(|m| {
        let pngs = ["linen", "wool", "leather", "skin"].map(|l| format!("forest_pbr/people_{l}_{m}.png"));
        load::texture_array(&format!("forest_pbr/people_{m}.dds"), &pngs.each_ref().map(|s| s.as_str()), m == "albedo")
    });
    match people {
        [Ok(a), Ok(o)] => crate::creature::set_people_layers(images.add(a), images.add(o)),
        _ => warn!("realforest: people garment layers missing — townsfolk use the creature cloth layer"),
    }
    let impostors = if impostor::enabled() {
        impostor::load_atlas(images, &by_name)
            .inspect_err(|e| warn!("realforest: no tree impostors ({e}) — far trees stay LOD1 meshes"))
            .ok()
    } else {
        None
    };
    let cover = (0..models.len()).map(|_| OnceLock::new()).collect();
    Ok(Kit { mats, snow_mats: snow_twins, models, by_name, ground, impostors, canopy_fade, cover })
}

/// (XZ half-extent, trunk-base radius) of a model from its CPU meshes. The trunk radius is the
/// widest bark vertex below 0.3u (root flare included) — the kit trees' only plain-PBR part.
fn model_extents(parts: &[Part], mats: &[KitMat]) -> (Vec2, f32, f32) {
    use bevy::mesh::VertexAttributeValues;
    let (mut half, mut trunk, mut height) = (Vec2::ZERO, 0.0f32, 0.0f32);
    for part in parts {
        let Some(VertexAttributeValues::Float32x3(p)) = part.mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        let bark = matches!(mats[part.mat], KitMat::Std(_)) && MATERIALS[part.mat].0.starts_with("bark_");
        for v in p {
            half = half.max(Vec2::new(v[0].abs(), v[2].abs()));
            height = height.max(v[1]);
            if bark && v[1] < 0.3 {
                trunk = trunk.max((v[0] * v[0] + v[2] * v[2]).sqrt());
            }
        }
    }
    (half, trunk, height)
}

/// Ground layers, in texture-array order (`terrain.wesl` indexes them 0/1/2).
const LAYERS: [&str; 3] = ["litter", "moss", "path"];

pub(crate) fn build_kit(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut foliage_mats: ResMut<Assets<foliage::FoliageMaterial>>,
    mut snow_mats: Option<ResMut<Assets<crate::alpine::snowkit::SnowKitMaterial>>>,
    mut images: ResMut<Assets<Image>>,
) {
    let t0 = std::time::Instant::now();
    match try_build(&mut meshes, &mut materials, &mut foliage_mats, snow_mats.as_deref_mut(), &mut images) {
        Ok(kit) => {
            info!("realforest: kit loaded ({} models) in {:.2}s", kit.models.len(), t0.elapsed().as_secs_f32());
            let _ = KIT.set(kit);
        }
        Err(e) => warn!("realforest: kit failed to load, keeping the low-poly forest — {e}"),
    }
}

// ── Terrain hook ───────────────────────────────────────────────────────────────────────────────

/// Bind the forest-floor layers + the forest/road mask into every world terrain material once.
/// Terrain materials are minted inside the world build (and again on every in-process rebuild),
/// so this watches for unpatched ones instead of hooking each creation site.
/// Once the world is built (each `WorldReady` edge — boot and every in-process rebuild), bake the
/// far-cover map from the placed cover (`far_cover`) and bind it to the terrain; keep its blend
/// band on the cover cards' render-distance-scaled range.
fn bind_far_cover(
    ready: Option<Res<crate::biome::WorldReady>>,
    mut mats: ResMut<Assets<TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut state: Local<(bool, Option<Handle<Image>>, f32)>,
) {
    let Some(ready) = ready else { return };
    let (was_ready, baked, band_for) = &mut *state;
    if ready.0 && !*was_ready {
        if let Some(old) = baked.take() {
            images.remove(&old);
        }
        *baked = if far_cover::enabled() { far_cover::bake(&mut images) } else { None };
        *band_for = -1.0;
    }
    *was_ready = ready.0;
    let Some(h) = baked.clone() else { return };
    // Lush carpet ends at LUSH_FAR, sparse cover at COVER_FAR (both stretched by the ground
    // factor): fade in from just inside the carpet's end to the cover's end.
    let gf = crate::quality::ground_factor(crate::quality::view_scale());
    if (gf - *band_for).abs() < 1e-3 {
        return;
    }
    *band_for = gf;
    let (start, end) = ((LUSH_FAR - 10.0) * gf, COVER_FAR * gf);
    let ids: Vec<_> = mats.iter().filter(|(_, m)| m.extension.params.pbr.x > 0.0).map(|(id, _)| id).collect();
    for id in ids {
        let Some(mut m) = mats.get_mut(id) else { continue };
        m.extension.far_cover = h.clone();
        let p = &mut m.extension.params.pbr;
        *p = Vec4::new(p.x, far_cover::shader_strength(), start, end);
    }
}

fn patch_terrain(
    mut mats: ResMut<Assets<TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut mask: Local<Option<(Handle<Image>, Vec4)>>,
) {
    let Some(kit) = get() else { return };
    // The mask's contact-AO channel needs the forest scatter's splats - wait for it.
    if !SCATTER_DONE.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let todo: Vec<_> = mats.iter().filter(|(_, m)| m.extension.params.pbr.x == 0.0).map(|(id, _)| id).collect();
    for id in todo {
        let Some(mut m) = mats.get_mut(id) else { continue };
        // Ground without a road network = a single-biome viewer / ground test: leave it alone.
        // `FOREST_PBR_GROUND=0` keeps the procedural floor (perf isolation / A-B).
        if m.extension.params.rut_region.z <= 0.0 || std::env::var("FOREST_PBR_GROUND").is_ok_and(|v| v == "0") {
            m.extension.params.pbr.x = -1.0;
            continue;
        }
        let (mask_h, region) = mask.get_or_insert_with(|| bake_mask(&mut images)).clone();
        let ext = &mut m.extension;
        ext.fp_albedo = kit.ground.albedo.clone();
        ext.fp_normal = kit.ground.normal.clone();
        ext.fp_ormh = kit.ground.ormh.clone();
        ext.fp_mask = mask_h;
        ext.params.pbr = Vec4::new(1.0, 0.0, 0.0, 0.0);
        ext.params.pbr_region = region;
    }
}

/// RGBA8 mask over the whole tile grid at 2 texels / unit: R = forest tile (soft-edged),
/// G = road strength (worn trail -> the path layer), B = contact darkening around trunks and
/// solid props (the root/stone <-> ground junction). World->UV = `(w - xy) * zw`.
fn bake_mask(images: &mut Assets<Image>) -> (Handle<Image>, Vec4) {
    use crate::worldmap::{COLS, GX, GZ, ROWS};
    const RES: f32 = 2.0;
    let (x0, z0) = (-GX, -GZ);
    let (w, h) = ((COLS as f32 * RES) as usize, (ROWS as f32 * RES) as usize);
    // `forest` = where the scanned floor lies: the forest AND the open grass (castle meadow, the
    // village lobe, the NW grassland). The grass used to keep the procedural vertex-green lawn
    // (+ shader twigs / soil blotches) — the flat neon-green "low-poly grass" patches the player
    // saw beside the scanned forest floor. `woods` is the forest alone: it gates the grove field
    // (mask A), so open meadow reads as the grass / clover layer, never as leaf litter.
    let mut forest = vec![0.0f32; w * h];
    let mut woods = vec![0.0f32; w * h];
    let mut land = vec![false; w * h];
    let mut road = vec![0u8; w * h];
    for j in 0..h {
        let wz = z0 + (j as f32 + 0.5) / RES;
        for i in 0..w {
            let wx = x0 + (i as f32 + 0.5) / RES;
            let k = j * w + i;
            land[k] = crate::worldmap::is_land_tile_world(wx, wz);
            let is_forest = crate::worldmap::tile_biome_world(wx, wz) == Some(crate::biome::Biome::Forest);
            woods[k] = is_forest as u8 as f32;
            forest[k] = (is_forest || crate::worldmap::is_grass_world(wx, wz)) as u8 as f32;
            road[k] = (crate::roads::road_strength(wx, wz).clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    // Carry the land's forest weight a few units OUT over the water tiles first: an eroded river /
    // lake bank runs down into the water tile beside it, and without this the floor faded to bare
    // vertex green along every forest stream (the bright strip on the bank lip). Each water texel
    // takes the mean of its already-filled neighbours, ring by ring (nearest land wins), so a
    // stream between forest and meadow splits down the middle instead of one side taking both.
    let mut filled = land;
    for _ in 0..6 {
        let (prev, prev_filled) = (forest.clone(), filled.clone());
        for j in 0..h {
            for i in 0..w {
                let k = j * w + i;
                if prev_filled[k] {
                    continue;
                }
                let (mut s, mut n) = (0.0, 0u32);
                for (ok, kk) in [(i > 0, k.wrapping_sub(1)), (i + 1 < w, k + 1), (j > 0, k.wrapping_sub(w)), (j + 1 < h, k + w)] {
                    if ok && prev_filled[kk] {
                        s += prev[kk];
                        n += 1;
                    }
                }
                if n > 0 {
                    forest[k] = s / n as f32;
                    filled[k] = true;
                }
            }
        }
    }
    // Separable box blur (radius 12 texels = 6u): a wide ramp the shader's edge noise can push the
    // floor's ragged edge back and forth across, so litter and mud interleave in tongues and
    // islands over several units instead of meeting on one line.
    let r = 12i32;
    let blur = |src: &[f32], horiz: bool| -> Vec<f32> {
        let mut out = vec![0.0f32; w * h];
        for j in 0..h as i32 {
            for i in 0..w as i32 {
                let mut s = 0.0;
                for d in -r..=r {
                    let (x, y) = if horiz { ((i + d).clamp(0, w as i32 - 1), j) } else { (i, (j + d).clamp(0, h as i32 - 1)) };
                    s += src[y as usize * w + x as usize];
                }
                out[j as usize * w + i as usize] = s / (2 * r + 1) as f32;
            }
        }
        out
    };
    // The feather straddles the (now ragged, `worldmap::region_noise`) forest / meadow edge: ~0.5
    // at the border, full ~2u in, gone ~2u out, and the shader's edge noise breaks that into
    // tongues of litter and mud. An earlier remap kept it wholly INSIDE the edge (litter crept onto
    // the swamp along a straight seam), but that bared a lime strip of raw vertex grass between the
    // litter and the mud — the seam read as a glowing line.
    let inward = |v: Vec<f32>| -> Vec<f32> {
        v.into_iter()
            .map(|x| {
                let t = ((x - 0.25) / 0.5).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            })
            .collect()
    };
    let forest = inward(blur(&blur(&forest, true), false));
    let woods = blur(&blur(&woods, true), false);
    let mut occl = vec![0.0f32; w * h];
    let solid = SPLATS.lock().expect("splats poisoned").clone();
    let cover = COVER_SPLATS.lock().expect("splats poisoned").clone();
    let all = solid.iter().map(|s| (*s, 1.0)).chain(cover.iter().map(|s| (*s, COVER_SPLAT_WEIGHT)));
    for (sp, wt) in all {
        let (cx, cz, r) = (sp.x, sp.y, sp.z.max(0.2));
        let reach = r * 2.6;
        let i0 = ((cx - reach - x0) * RES).floor().max(0.0) as usize;
        let i1 = (((cx + reach - x0) * RES).ceil().max(0.0) as usize).min(w);
        let j0 = ((cz - reach - z0) * RES).floor().max(0.0) as usize;
        let j1 = (((cz + reach - z0) * RES).ceil().max(0.0) as usize).min(h);
        for j in j0..j1 {
            let dz = z0 + (j as f32 + 0.5) / RES - cz;
            for i in i0..i1 {
                let dx = x0 + (i as f32 + 0.5) / RES - cx;
                let v = (-(dx * dx + dz * dz) / (r * r * 1.6)).exp() * wt;
                let o = &mut occl[j * w + i];
                *o = 1.0 - (1.0 - *o) * (1.0 - v);
            }
        }
    }
    let mut grove = vec![0u8; w * h];
    for j in 0..h {
        let wz = z0 + (j as f32 + 0.5) / RES;
        for i in 0..w {
            let wx = x0 + (i as f32 + 0.5) / RES;
            let k = j * w + i;
            grove[k] = (crate::biome::forest_grove(wx, wz) * woods[k] * 255.0).round() as u8;
        }
    }
    let mut data = vec![0u8; w * h * 4];
    for k in 0..w * h {
        data[k * 4] = (forest[k] * 255.0).round() as u8;
        data[k * 4 + 1] = road[k];
        data[k * 4 + 2] = (occl[k].clamp(0.0, 1.0) * 255.0).round() as u8;
        data[k * 4 + 3] = grove[k];
    }
    // The castle yard's worn ground (lanes, plaza, wall paths, working corners) rides the same
    // channels: scanned floor + trail + debris — see `castle_ground`.
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (i0, i1) = (((-crate::castle_ground::REACH_X - x0) * RES).max(0.0) as usize, (((crate::castle_ground::REACH_X - x0) * RES) as usize).min(w));
    let (j0, j1) = (((-crate::castle_ground::REACH_Z - z0) * RES).max(0.0) as usize, (((crate::castle_ground::REACH_Z - z0) * RES) as usize).min(h));
    for j in j0..j1 {
        let wz = z0 + (j as f32 + 0.5) / RES;
        for i in i0..i1 {
            let wx = x0 + (i as f32 + 0.5) / RES;
            let g = crate::castle_ground::sample(wx, wz);
            if g.floor <= 0.0 {
                continue;
            }
            let k = (j * w + i) * 4;
            data[k] = data[k].max(to_u8(g.floor));
            data[k + 1] = data[k + 1].max(to_u8(g.path));
            data[k + 3] = data[k + 3].max(to_u8(g.debris));
        }
    }
    // Ork camps' trampled ground (`camps::ground_wear`): same channels, noisy feathered blots.
    for c in crate::camps::wear_centres() {
        let r = crate::camps::WEAR_REACH;
        let ib = |v: f32, n: usize| ((v * RES).max(0.0) as usize).min(n);
        let (i0, i1) = (ib(c.x - r - x0, w), ib(c.x + r - x0, w));
        let (j0, j1) = (ib(c.y - r - z0, h), ib(c.y + r - z0, h));
        for j in j0..j1 {
            let wz = z0 + (j as f32 + 0.5) / RES;
            for i in i0..i1 {
                let wx = x0 + (i as f32 + 0.5) / RES;
                let Some((f, p, d)) = crate::camps::ground_wear(wx, wz) else { continue };
                let k = (j * w + i) * 4;
                data[k] = data[k].max(to_u8(f));
                data[k + 1] = data[k + 1].max(to_u8(p));
                data[k + 3] = data[k + 3].max(to_u8(d));
            }
        }
    }
    // The field strips (`castle_ground::field_at`): scanned soil (the path layer) on
    // the ploughed strips, a soil/grass mix on the stubble ones, no litter. Furrows ride the rut mask.
    let (flo, fhi) = crate::castle_ground::field_bounds();
    let (i0, i1) = (((flo.x - x0) * RES).max(0.0) as usize, (((fhi.x - x0) * RES) as usize).min(w));
    let (j0, j1) = (((flo.y - z0) * RES).max(0.0) as usize, (((fhi.y - z0) * RES) as usize).min(h));
    for j in j0..j1 {
        let wz = z0 + (j as f32 + 0.5) / RES;
        for i in i0..i1 {
            let wx = x0 + (i as f32 + 0.5) / RES;
            let Some((fw, plough)) = crate::castle_ground::field_at(wx, wz) else { continue };
            let k = (j * w + i) * 4;
            data[k] = data[k].max(to_u8(fw));
            data[k + 1] = data[k + 1].max(to_u8((0.30 + 0.54 * plough) * fw));
            data[k + 3] = (data[k + 3] as f32 * (1.0 - fw)) as u8;
        }
    }
    // The meadow sites (fold, drill yard, beacon): trampled mud and sheep tracks — `meadow`.
    for (c, r, wear) in crate::meadow::ground_wear_passes() {
        let (i0, i1) = (((c.x - r - x0) * RES).max(0.0) as usize, (((c.x + r - x0) * RES).max(0.0) as usize).min(w));
        let (j0, j1) = (((c.y - r - z0) * RES).max(0.0) as usize, (((c.y + r - z0) * RES).max(0.0) as usize).min(h));
        for j in j0..j1 {
            let wz = z0 + (j as f32 + 0.5) / RES;
            for i in i0..i1 {
                let Some((fl, pa, de)) = wear(x0 + (i as f32 + 0.5) / RES, wz) else { continue };
                let k = (j * w + i) * 4;
                data[k] = data[k].max(to_u8(fl));
                data[k + 1] = data[k + 1].max(to_u8(pa));
                data[k + 3] = data[k + 3].max(to_u8(de));
            }
        }
    }
    let mut img = Image::new(
        bevy::render::render_resource::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8Unorm,
        bevy::asset::RenderAssetUsages::default(),
    );
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        ..default()
    });
    (images.add(img), Vec4::new(x0, z0, 1.0 / COLS as f32, 1.0 / ROWS as f32))
}
