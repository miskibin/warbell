//! Map-authoring asset foundation for the future in-game visual editor.
//!
//! The editor stores designer intent as small, text-friendly overlay data. The existing world
//! generator still bakes terrain, scatter, blockers and nav; this module only answers "what did the
//! authored map ask for at this base-space coordinate?"

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

const CURRENT_VERSION: u32 = 1;
const DEFAULT_REGION_FEATHER: f32 = 4.0;
const DEFAULT_RIVER_HALF_WIDTH: f32 = 1.2;

static ACTIVE_AUTHORING: OnceLock<RwLock<Option<Arc<MapAuthoring>>>> = OnceLock::new();

fn active_cell() -> &'static RwLock<Option<Arc<MapAuthoring>>> {
    ACTIVE_AUTHORING.get_or_init(|| RwLock::new(None))
}

#[derive(Resource, Default)]
pub(crate) struct ActiveMapAuthoring {
    path: Option<PathBuf>,
    map: Option<Arc<MapAuthoring>>,
}

pub struct MapAuthoringPlugin;

impl Plugin for MapAuthoringPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveMapAuthoring>()
            .add_systems(Startup, load_authoring_from_env);
    }
}

fn load_authoring_from_env(mut active: ResMut<ActiveMapAuthoring>) {
    let Some(path) = std::env::var_os("FOREST_MAP_AUTHORING").map(PathBuf::from) else {
        install_active(None);
        return;
    };
    match MapAuthoring::load_ron(&path) {
        Ok(map) => {
            let map = Arc::new(map);
            info!(
                "loaded map authoring asset {} ({} biome regions, {} river splines)",
                path.display(),
                map.biome_regions.len(),
                map.river_splines.len()
            );
            active.path = Some(path);
            active.map = Some(map.clone());
            install_active(Some(map));
            crate::worldmap::clear_tile_cache();
        }
        Err(err) => {
            warn!(
                "failed to load FOREST_MAP_AUTHORING={}: {err}",
                path.display()
            );
            active.path = Some(path);
            active.map = None;
            install_active(None);
        }
    }
}

fn install_active(map: Option<Arc<MapAuthoring>>) {
    *active_cell().write().expect("map authoring lock poisoned") = map;
}

fn with_active<R>(f: impl FnOnce(&MapAuthoring) -> R) -> Option<R> {
    let guard = active_cell().read().expect("map authoring lock poisoned");
    guard.as_deref().map(f)
}

pub(crate) fn sample_base(x: f32, z: f32) -> AuthoringSample {
    with_active(|map| map.sample_base(x, z)).unwrap_or_default()
}

pub(crate) fn water_signed_distance_base(x: f32, z: f32) -> Option<f32> {
    with_active(|map| map.water_signed_distance_base(x, z)).flatten()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MapAuthoring {
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub target: AuthorMap,
    #[serde(default)]
    pub biome_regions: Vec<BiomeRegion>,
    #[serde(default)]
    pub height_patches: Vec<HeightPatch>,
    #[serde(default)]
    pub river_splines: Vec<RiverSpline>,
    #[serde(default)]
    pub road_strokes: Vec<RoadStroke>,
    #[serde(default)]
    pub scatter_paint: Vec<ScatterPaint>,
    #[serde(default)]
    pub poi_placements: Vec<PoiPlacement>,
    #[serde(default)]
    pub validation: ValidationRules,
}

impl MapAuthoring {
    pub fn load_ron(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path).map_err(|err| format!("read failed: {err}"))?;
        let map: Self = ron::from_str(&src).map_err(|err| format!("RON parse failed: {err}"))?;
        map.validate_schema()?;
        Ok(map)
    }

    fn validate_schema(&self) -> Result<(), String> {
        if self.version != CURRENT_VERSION {
            return Err(format!(
                "unsupported map authoring version {} (expected {CURRENT_VERSION})",
                self.version
            ));
        }
        for region in &self.biome_regions {
            if region.radius_base <= 0.0 {
                return Err(format!(
                    "biome region '{}' has non-positive radius",
                    region.id
                ));
            }
            if region.feather_base < 0.0 {
                return Err(format!("biome region '{}' has negative feather", region.id));
            }
        }
        for river in &self.river_splines {
            if river.points_base.len() < 2 {
                return Err(format!(
                    "river spline '{}' needs at least two points",
                    river.id
                ));
            }
            if river.half_width_base <= 0.0 {
                return Err(format!(
                    "river spline '{}' has non-positive half_width_base",
                    river.id
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn sample_base(&self, x: f32, z: f32) -> AuthoringSample {
        let mut sample = AuthoringSample::default();
        if let Some(sd) = self.water_signed_distance_base(x, z) {
            sample.water = sd < 0.0;
            sample.water_signed_distance = Some(sd);
        }

        let mut best_weight = 0.0;
        for region in &self.biome_regions {
            let weight = region.weight_at(x, z);
            if weight > best_weight {
                best_weight = weight;
                sample.biome = Some(region.biome);
                sample.biome_weight = weight;
            }
        }

        for patch in &self.height_patches {
            if patch.contains(x, z) {
                if let Some(class) = patch.height_class {
                    sample.height_class = Some(class);
                }
                sample.height_delta += patch.height_delta;
            }
        }
        sample
    }

    pub(crate) fn water_signed_distance_base(&self, x: f32, z: f32) -> Option<f32> {
        self.river_splines
            .iter()
            .filter_map(|river| river.signed_distance_base(x, z))
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }
}

fn default_version() -> u32 {
    CURRENT_VERSION
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum AuthorMap {
    #[default]
    Home,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum AuthorBiome {
    Grass,
    Sand,
    Forest,
    Rock,
    Snow,
    Desert,
    Swamp,
    Blight,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct BiomeRegion {
    #[serde(default)]
    pub id: String,
    pub biome: AuthorBiome,
    pub center_base: (f32, f32),
    pub radius_base: f32,
    #[serde(default = "default_region_feather")]
    pub feather_base: f32,
}

impl BiomeRegion {
    fn weight_at(&self, x: f32, z: f32) -> f32 {
        let d = (x - self.center_base.0).hypot(z - self.center_base.1);
        if d <= self.radius_base {
            1.0
        } else if self.feather_base <= 0.0 {
            0.0
        } else {
            (1.0 - (d - self.radius_base) / self.feather_base).clamp(0.0, 1.0)
        }
    }
}

fn default_region_feather() -> f32 {
    DEFAULT_REGION_FEATHER
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct HeightPatch {
    #[serde(default)]
    pub id: String,
    pub center_base: (f32, f32),
    pub radius_base: f32,
    #[serde(default)]
    pub height_delta: i32,
    #[serde(default)]
    pub height_class: Option<i32>,
}

impl HeightPatch {
    fn contains(&self, x: f32, z: f32) -> bool {
        self.radius_base > 0.0
            && (x - self.center_base.0).hypot(z - self.center_base.1) <= self.radius_base
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RiverSpline {
    #[serde(default)]
    pub id: String,
    pub points_base: Vec<(f32, f32)>,
    #[serde(default = "default_river_half_width")]
    pub half_width_base: f32,
}

impl RiverSpline {
    fn signed_distance_base(&self, x: f32, z: f32) -> Option<f32> {
        let p = (x, z);
        self.points_base
            .windows(2)
            .map(|w| segment_distance(p, w[0], w[1]) - self.half_width_base)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }
}

fn default_river_half_width() -> f32 {
    DEFAULT_RIVER_HALF_WIDTH
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let ab = (b.0 - a.0, b.1 - a.1);
    let ap = (p.0 - a.0, p.1 - a.1);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 <= f32::EPSILON {
        return ap.0.hypot(ap.1);
    }
    let t = ((ap.0 * ab.0 + ap.1 * ab.1) / len2).clamp(0.0, 1.0);
    let q = (a.0 + ab.0 * t, a.1 + ab.1 * t);
    (p.0 - q.0).hypot(p.1 - q.1)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum RoadKind {
    MainRoad,
    Trail,
    CampPath,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RoadStroke {
    #[serde(default)]
    pub id: String,
    pub kind: RoadKind,
    pub points_base: Vec<(f32, f32)>,
    pub half_width_base: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum ScatterTool {
    EraseProceduralColliders,
    PaintBiomeProps,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ScatterPaint {
    #[serde(default)]
    pub id: String,
    pub tool: ScatterTool,
    pub biome: Option<AuthorBiome>,
    pub center_base: (f32, f32),
    pub radius_base: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum PoiKind {
    PlayerCastle,
    RivalCastle,
    OrkFortress,
    OrkCamp,
    BossHome,
    Landmark,
    Chest,
    Ore,
    WaterfallVista,
    Wayside,
    MicroPoi,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PoiPlacement {
    #[serde(default)]
    pub id: String,
    pub kind: PoiKind,
    pub position_world: (f32, f32, f32),
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub biome: Option<AuthorBiome>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ValidationRules {
    #[serde(default = "default_true")]
    pub block_save_on_errors: bool,
    #[serde(default = "default_true")]
    pub require_reachability: bool,
    #[serde(default = "default_true")]
    pub require_flat_major_pois: bool,
    #[serde(default = "default_true")]
    pub require_no_collisions: bool,
}

impl Default for ValidationRules {
    fn default() -> Self {
        Self {
            block_save_on_errors: true,
            require_reachability: true,
            require_flat_major_pois: true,
            require_no_collisions: true,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AuthoringSample {
    pub water: bool,
    pub water_signed_distance: Option<f32>,
    pub biome: Option<AuthorBiome>,
    pub biome_weight: f32,
    pub height_class: Option<i32>,
    pub height_delta: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_ron_asset() {
        let map: MapAuthoring = ron::from_str(
            r#"(
                version: 1,
                name: "Home editor foundation",
                target: Home,
            )"#,
        )
        .expect("minimal map authoring asset parses");

        assert_eq!(map.version, CURRENT_VERSION);
        assert_eq!(map.target, AuthorMap::Home);
        assert!(map.biome_regions.is_empty());
        map.validate_schema().expect("minimal schema is valid");
    }

    #[test]
    fn samples_soft_biome_region_and_river_width() {
        let map: MapAuthoring = ron::from_str(
            r#"(
                version: 1,
                name: "Painted test",
                target: Home,
                biome_regions: [
                    (id: "soft-forest", biome: Forest, center_base: (10.0, 10.0), radius_base: 5.0, feather_base: 5.0),
                ],
                river_splines: [
                    (id: "brook", points_base: [(0.0, 0.0), (10.0, 0.0)], half_width_base: 1.5),
                ],
            )"#,
        )
        .expect("full map authoring asset parses");

        let centre = map.sample_base(10.0, 10.0);
        assert_eq!(centre.biome, Some(AuthorBiome::Forest));
        assert_eq!(centre.biome_weight, 1.0);

        let feather = map.sample_base(17.5, 10.0);
        assert_eq!(feather.biome, Some(AuthorBiome::Forest));
        assert!((0.0..1.0).contains(&feather.biome_weight));

        let channel = map.sample_base(4.0, 0.25);
        assert!(channel.water);
        let bank = map.sample_base(4.0, 3.0);
        assert!(!bank.water);
    }
}
