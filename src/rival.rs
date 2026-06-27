//! **Rival stronghold** — a *Stronghold: Crusader*-style AI opponent. A SECOND castle, raised in a
//! different biome (the NE desert) in a deliberately different **sandstone-and-crimson** style, so
//! it reads at a glance as "not yours". It contests the island for dominance: in later steps it
//! grows its own economy from taxes and buys buildings over time, and its soldiers skirmish with the
//! hero and the player's townsfolk when they meet. It is NOT part of the night siege — the orks
//! ignore it; it is a separate rivalry.
//!
//! This step builds the **static fort** only. Like the player castle and the ork fortress, the fort
//! is WORLD geometry: raised once with the island as a `worldmap::build_step` phase (NOT per-run),
//! so it persists across New Game / Continue in-process. The growing economy, garrison and save
//! state arrive in the following steps.
//!
//! Meshes reuse the castle's primitive helpers (`castle::bx`/`gable`/`cyl`) but render against a
//! bespoke plain-material set ([`RivalMats`]) — warm sandstone, dark courses, crimson banners,
//! timber and iron — kept fully decoupled from the player town's textured [`crate::castle::Mats`]
//! so the two strongholds never look alike.

use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::castle::{bx, cyl};
use crate::palette::srgb;
use crate::worldmap::ground_at_world;

/// World-XZ centre of the rival keep, anchored to the NE **desert** at base tile ≈ (102, 14) and
/// derived from `MAP_SCALE` (world = `(base − island-centre)·MAP_SCALE`) so it stays in the desert
/// at any map scale. It sits ~100 units from the player castle at the origin, so the two develop
/// independently and only clash when someone crosses the open ground between them. A forced-flat
/// plateau ([`fort_flat_zone`]) levels the dune terraces under + around it.
pub const RIVAL_CENTRE: Vec2 =
    Vec2::new(30.0 * crate::worldmap::MAP_SCALE, -40.0 * crate::worldmap::MAP_SCALE);

/// Radius of the rival fort's forced-flat desert plateau (world units) — its own "safe-zone": the
/// curtain walls span ±[`WALL_HALF`], this clears a generous flat apron beyond them for the rival's
/// buildings + the skirmish ground, so the fort never straddles a dune terrace lip.
pub const RIVAL_FLAT_R: f32 = 20.0;

/// Is world `(wx, wz)` inside the rival fort's forced-flat plateau? `worldmap::classify` force-sets
/// flat desert here, the mirror of the castle's `SAFE_R` safe-zone (and the town build plots).
pub fn fort_flat_zone(wx: f32, wz: f32) -> bool {
    if crate::worldmap::MapId::from_u8(crate::worldmap::current_map_u8()) != crate::worldmap::MapId::Home {
        return false;
    }
    Vec2::new(wx, wz).distance(RIVAL_CENTRE) < RIVAL_FLAT_R
}

/// Outer half-extent of the curtain-wall ring (square). The gate gap faces +Z (south, toward the
/// player's castle).
const WALL_HALF: f32 = 12.0;
const WALL_H: f32 = 2.8;
const WALL_T: f32 = 0.7;
/// Half-width of the south gate gap.
const GATE_HALF: f32 = 2.4;

/// Tags every part of the rival stronghold — one despawn target for a future rebuild/teardown, and
/// (later) what the destructibility pass will reap. Also `BiomeEntity` so a single-biome viewer swap
/// clears it like the rest of the world dressing.
#[derive(Component)]
pub struct RivalEntity;

/// The central keep mesh root (so later steps can flash/topple it on damage).
#[derive(Component)]
pub struct RivalKeep;

/// Is world `(wx, wz)` inside the rival fort's footprint (+ a small margin)? World scatter
/// (cacti, rocks, props) rejects this so the bailey reads clean — the mirror of
/// `castle::in_footprint` for the player keep. Home map only (the fort is Home-only).
pub fn near_fort(wx: f32, wz: f32) -> bool {
    // The whole forced-flat plateau (walls + apron) reads clean — no cacti against the ramparts.
    fort_flat_zone(wx, wz)
}

// ── Materials ──────────────────────────────────────────────────────────────────────
// Plain solid-colour StandardMaterials (the game's low-poly prop aesthetic). Warm sandstone +
// crimson reads instantly as a desert enemy fort, distinct from the player castle's grey ashlar and
// blue banners.
const SAND: u32 = 0xc9a36a; // warm sandstone courses
const SAND_DARK: u32 = 0x9c7a48; // shadowed banding / battlement caps
const CRIMSON: u32 = 0x9a2420; // enemy banners + pennants
const TIMBER: u32 = 0x3a2618; // gate, beams, poles
const IRON: u32 = 0x55585c; // gate bands, spikes

/// A material slot in a rival part list — resolved to a real handle by [`spawn_fort`].
#[derive(Clone, Copy)]
enum RM {
    Sand,
    SandDark,
    Crimson,
    Timber,
    Iron,
}

/// The rival's plain-material set. Stored as a Resource so the economy tick can spawn new
/// buildings at runtime with the same look the fort was built with.
#[derive(Resource)]
struct RivalMats {
    sand: Handle<StandardMaterial>,
    sand_dark: Handle<StandardMaterial>,
    crimson: Handle<StandardMaterial>,
    timber: Handle<StandardMaterial>,
    iron: Handle<StandardMaterial>,
}

impl RivalMats {
    fn build(std_mats: &mut Assets<StandardMaterial>) -> Self {
        let mut solid = |hex: u32, rough: f32| {
            std_mats.add(StandardMaterial {
                base_color: srgb(hex),
                perceptual_roughness: rough,
                ..default()
            })
        };
        Self {
            sand: solid(SAND, 0.92),
            sand_dark: solid(SAND_DARK, 0.92),
            crimson: solid(CRIMSON, 0.85),
            timber: solid(TIMBER, 0.9),
            iron: solid(IRON, 0.6),
        }
    }
    fn get(&self, m: RM) -> Handle<StandardMaterial> {
        match m {
            RM::Sand => self.sand.clone(),
            RM::SandDark => self.sand_dark.clone(),
            RM::Crimson => self.crimson.clone(),
            RM::Timber => self.timber.clone(),
            RM::Iron => self.iron.clone(),
        }
    }
}

// ── Mesh assembly (local space; origin = fort centre, base at y = 0) ─────────────────

/// Square ring of merlons (battlement teeth) at height `y`, spaced around a ±`half` square.
fn crenellate(v: &mut Vec<(Mesh, RM)>, half: f32, y: f32) {
    let step = 0.9;
    let n = (half * 2.0 / step) as i32;
    for i in 0..=n {
        let t = -half + i as f32 * step;
        for (x, z) in [(t, -half), (t, half), (-half, t), (half, t)] {
            v.push((bx(0.42, 0.5, 0.42, x, y, z), RM::SandDark));
        }
    }
}

/// A crimson banner on a timber pole at (x, z), cloth hanging to one side.
fn banner(v: &mut Vec<(Mesh, RM)>, x: f32, z: f32, base_y: f32, height: f32) {
    v.push((cyl(0.08, height, x, base_y + height / 2.0, z), RM::Timber));
    // Cloth: a long crimson banner hanging from the pole top (sized to read from across the dunes).
    let cloth_h = (height * 0.55).min(1.8);
    v.push((bx(0.08, cloth_h, 1.1, x, base_y + height - cloth_h / 2.0 - 0.15, z + 0.62), RM::Crimson));
}

/// The stepped sandstone keep — a tapering three-tier tower with a battlemented crown and a tall
/// crimson banner, the silhouette the player reads from across the dunes.
fn keep_parts() -> Vec<(Mesh, RM)> {
    let mut v: Vec<(Mesh, RM)> = Vec::new();
    // Tier 1 (base).
    v.push((bx(6.0, 4.0, 6.0, 0.0, 2.0, 0.0), RM::Sand));
    v.push((bx(6.3, 0.4, 6.3, 0.0, 4.0, 0.0), RM::SandDark)); // course band
    // Tier 2.
    v.push((bx(4.6, 3.0, 4.6, 0.0, 5.7, 0.0), RM::Sand));
    v.push((bx(4.9, 0.4, 4.9, 0.0, 7.2, 0.0), RM::SandDark));
    // Tier 3 (crown).
    v.push((bx(3.4, 2.2, 3.4, 0.0, 8.5, 0.0), RM::Sand));
    v.push((bx(3.7, 0.3, 3.7, 0.0, 9.7, 0.0), RM::SandDark)); // terrace cap
    crenellate(&mut v, 1.75, 10.1);
    // Arrow slits (dark insets) on the base front.
    for x in [-1.4_f32, 1.4] {
        v.push((bx(0.3, 0.9, 0.12, x, 2.6, 3.0), RM::Iron));
    }
    // Crowning banner.
    banner(&mut v, 0.0, 0.0, 9.9, 3.2);
    v
}

/// One straight curtain-wall segment from (x0,z0) to (x1,z1) (axis-aligned), with a crenellated top.
fn wall_run(v: &mut Vec<(Mesh, RM)>, x0: f32, z0: f32, x1: f32, z1: f32) {
    let cx = (x0 + x1) / 2.0;
    let cz = (z0 + z1) / 2.0;
    let len_x = (x1 - x0).abs();
    let len_z = (z1 - z0).abs();
    let (w, d) = if len_x >= len_z { (len_x + WALL_T, WALL_T) } else { (WALL_T, len_z + WALL_T) };
    v.push((bx(w, WALL_H, d, cx, WALL_H / 2.0, cz), RM::Sand));
    v.push((bx(w + 0.12, 0.3, d + 0.12, cx, WALL_H + 0.15, cz), RM::SandDark)); // cap
    // A scatter of merlons along the run.
    let along = len_x.max(len_z);
    let n = (along / 0.9) as i32;
    for i in 0..=n {
        let t = -along / 2.0 + i as f32 * 0.9;
        let (mx, mz) = if len_x >= len_z { (cx + t, cz) } else { (cx, cz + t) };
        v.push((bx(0.42, 0.5, 0.42, mx, WALL_H + 0.45, mz), RM::SandDark));
    }
}

/// A square sandstone corner tower with battlements + a small crimson pennant.
fn corner_tower(v: &mut Vec<(Mesh, RM)>, x: f32, z: f32) {
    v.push((bx(2.8, WALL_H + 2.2, 2.8, x, (WALL_H + 2.2) / 2.0, z), RM::Sand));
    v.push((bx(3.0, 0.3, 3.0, x, WALL_H + 2.2, z), RM::SandDark));
    // Local merlons (a tiny ring around the tower top).
    for (dx, dz) in [(-1.0_f32, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, -1.0), (0.0, 1.0)] {
        v.push((bx(0.42, 0.5, 0.42, x + dx, WALL_H + 2.55, z + dz), RM::SandDark));
    }
    banner(v, x, z, WALL_H + 2.2, 1.8);
}

/// A simple flat-roofed sandstone desert dwelling at (x, z) — the kind the rival's economy will
/// raise more of. Distinct from the player town's gabled timber houses.
fn desert_house(x: f32, z: f32) -> Vec<(Mesh, RM)> {
    vec![
        (bx(2.6, 1.9, 2.4, x, 0.95, z), RM::Sand),
        (bx(2.8, 0.25, 2.6, x, 1.95, z), RM::SandDark), // flat roof slab
        (bx(0.6, 1.0, 0.08, x, 0.5, z + 1.2), RM::Timber), // door
        (bx(0.45, 0.45, 0.1, x - 0.7, 1.2, z + 1.2), RM::Iron), // window inset
    ]
}

/// The whole stronghold as a local-space part list.
fn fort_parts() -> (Vec<(Mesh, RM)>, Vec<(Mesh, RM)>) {
    let keep = keep_parts();
    let mut rest: Vec<(Mesh, RM)> = Vec::new();
    let h = WALL_HALF;
    // North / east / west walls (full runs); south wall split around the gate gap.
    wall_run(&mut rest, -h, -h, h, -h); // north
    wall_run(&mut rest, h, -h, h, h); // east
    wall_run(&mut rest, -h, -h, -h, h); // west
    wall_run(&mut rest, -h, h, -GATE_HALF, h); // south-west of gate
    wall_run(&mut rest, GATE_HALF, h, h, h); // south-east of gate
    // Gate: two timber leaves with iron banding under a sandstone lintel.
    rest.push((bx(GATE_HALF * 2.0 + 0.6, 0.6, WALL_T + 0.3, 0.0, WALL_H + 0.1, h), RM::Sand)); // lintel
    for sx in [-1.0_f32, 1.0] {
        rest.push((bx(GATE_HALF - 0.1, WALL_H - 0.2, 0.3, sx * (GATE_HALF / 2.0), (WALL_H - 0.2) / 2.0, h), RM::Timber));
    }
    for sx in [-1.0_f32, 1.0] {
        rest.push((bx(GATE_HALF * 2.0 - 0.2, 0.18, 0.36, 0.0, 0.6 + sx * 0.7 + 0.7, h), RM::Iron)); // bands
    }
    // Corner towers.
    for (sx, sz) in [(-1.0_f32, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        corner_tower(&mut rest, sx * h, sz * h);
    }
    // No dwellings here — the bailey starts bare and the rival's economy raises buildings on the
    // [`PLOT_OFFSETS`] over time (the "watch it grow" core of the rivalry).
    (keep, rest)
}

// ── Economy buildings (raised on plots by the rival's autonomous economy) ─────────────

/// Bailey plot offsets (local XZ from [`RIVAL_CENTRE`]) the rival raises buildings on, in build
/// order. Kept clear of the central keep (±~4) and the south gate lane.
const PLOT_OFFSETS: [Vec2; 10] = [
    Vec2::new(-8.0, -6.5),
    Vec2::new(8.0, -6.5),
    Vec2::new(-8.5, 0.5),
    Vec2::new(8.5, 0.5),
    Vec2::new(-8.0, 7.0),
    Vec2::new(8.0, 7.0),
    Vec2::new(-4.5, -9.0),
    Vec2::new(4.5, -9.0),
    Vec2::new(-4.5, 9.0),
    Vec2::new(4.5, 9.0),
];

/// The kind of building the rival raises. `Dwelling` lifts its population (→ more tax income);
/// the others are economy set-dressing that mark a growing settlement.
#[derive(Clone, Copy, PartialEq)]
pub enum RivalKind {
    Dwelling,
    Granary,
    Workshop,
}

impl RivalKind {
    fn is_house(self) -> bool {
        matches!(self, RivalKind::Dwelling)
    }
}

/// The fixed order the rival builds in — dwellings interleaved with stores/workshops so its
/// population (and thus income) climbs steadily, Stronghold-style.
const BUILD_ORDER: [RivalKind; 10] = [
    RivalKind::Dwelling,
    RivalKind::Granary,
    RivalKind::Dwelling,
    RivalKind::Workshop,
    RivalKind::Dwelling,
    RivalKind::Granary,
    RivalKind::Dwelling,
    RivalKind::Workshop,
    RivalKind::Dwelling,
    RivalKind::Granary,
];

/// A round sandstone granary silo with a domed cap and a dark course band.
fn granary(x: f32, z: f32) -> Vec<(Mesh, RM)> {
    vec![
        (cyl(1.25, 2.2, x, 1.1, z), RM::Sand),
        (cyl(1.32, 0.3, x, 1.9, z), RM::SandDark), // band
        (cyl(0.95, 0.7, x, 2.55, z), RM::SandDark), // domed cap (squat cylinder)
        (bx(0.5, 0.7, 0.08, x, 0.45, z + 1.25), RM::Timber), // hatch door
    ]
}

/// A low sandstone workshop with a timber lean-to awning on iron posts.
fn workshop(x: f32, z: f32) -> Vec<(Mesh, RM)> {
    vec![
        (bx(2.8, 1.5, 2.2, x, 0.75, z), RM::Sand),
        (bx(3.0, 0.22, 2.4, x, 1.5, z), RM::SandDark), // flat roof slab
        (bx(2.4, 0.12, 1.0, x, 1.15, z + 1.6), RM::Timber), // awning
        (bx(0.1, 1.1, 0.1, x - 1.1, 0.55, z + 2.0), RM::Iron), // awning post
        (bx(0.1, 1.1, 0.1, x + 1.1, 0.55, z + 2.0), RM::Iron),
        (bx(0.55, 0.95, 0.08, x, 0.48, z + 1.1), RM::Timber), // door
    ]
}

/// Local-space part list for a building of `kind` at (x, z).
fn building_parts(kind: RivalKind, x: f32, z: f32) -> Vec<(Mesh, RM)> {
    match kind {
        RivalKind::Dwelling => desert_house(x, z),
        RivalKind::Granary => granary(x, z),
        RivalKind::Workshop => workshop(x, z),
    }
}

/// Tags a building the rival economy raised on plot `idx` (so later steps can damage/topple it).
#[derive(Component)]
pub struct RivalBuilding {
    pub idx: usize,
}

/// Spawn one rival building at plot `idx` (world-snapped, its own entity). Shared by the runtime
/// economy tick, the load-restore reconcile, and the screenshot-staging hook.
fn spawn_building(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &RivalMats,
    idx: usize,
    kind: RivalKind,
) {
    let Some(off) = PLOT_OFFSETS.get(idx).copied() else { return };
    let wx = RIVAL_CENTRE.x + off.x;
    let wz = RIVAL_CENTRE.y + off.y;
    let y = ground_at_world(wx, wz).unwrap_or(0.0);
    let parent = commands
        .spawn((
            Transform::from_xyz(wx, y, wz),
            Visibility::Visible,
            BiomeEntity,
            RivalEntity,
            RivalBuilding { idx },
        ))
        .id();
    commands.entity(parent).with_children(|p| {
        for (mesh, slot) in building_parts(kind, 0.0, 0.0) {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.get(slot)), Transform::default()));
        }
    });
}

// ── Build (a worldmap build phase) ──────────────────────────────────────────────────

/// Raise the rival stronghold at [`RIVAL_CENTRE`]. Called once from `worldmap::build_step`. Home map
/// only — the desert sits elsewhere on the Ashlands layout, so the fort would land in the wrong
/// biome there; a future step can place an Ashlands variant.
pub fn build(commands: &mut Commands, meshes: &mut Assets<Mesh>, std_mats: &mut Assets<StandardMaterial>) {
    if crate::worldmap::MapId::from_u8(crate::worldmap::current_map_u8()) != crate::worldmap::MapId::Home {
        return;
    }
    let mats = RivalMats::build(std_mats);
    let centre = RIVAL_CENTRE;
    let y = ground_at_world(centre.x, centre.y).unwrap_or(0.0);
    let (keep, rest) = fort_parts();

    // Keep root (own tag so later damage VFX can find it).
    let keep_root = commands
        .spawn((
            Transform::from_xyz(centre.x, y, centre.y),
            Visibility::Visible,
            BiomeEntity,
            RivalEntity,
            RivalKeep,
        ))
        .id();
    commands.entity(keep_root).with_children(|p| {
        for (mesh, slot) in keep {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.get(slot)), Transform::default()));
        }
    });

    // Walls / towers root (the bailey starts bare — the economy raises buildings on plots).
    let fort_root = commands
        .spawn((Transform::from_xyz(centre.x, y, centre.y), Visibility::Visible, BiomeEntity, RivalEntity))
        .id();
    commands.entity(fort_root).with_children(|p| {
        for (mesh, slot) in rest {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.get(slot)), Transform::default()));
        }
    });

    // Hand the material set to the economy tick so it can raise buildings at runtime.
    commands.insert_resource(mats);

    // Make the fort SOLID — register collision boxes for the curtain walls, keep and towers (the
    // mirror of the player castle's wall blockers) so the hero can't walk through the ramparts and
    // world-prop placement avoids them. Registered once; the fort is permanent world geometry, so
    // (like the castle) these are never removed.
    register_blockers(centre);
}

/// Curtain-wall / keep / tower collision boxes (axis-aligned — the fort is square). The south gate
/// gap is left open so units sally through it, exactly like the player castle's gate gaps.
fn register_blockers(centre: Vec2) {
    use crate::blockers::add_box;
    let (cx, cz) = (centre.x, centre.y);
    let h = WALL_HALF;
    let t = WALL_T / 2.0 + 0.2; // wall half-depth + a small body margin
    add_box(cx, cz - h, h, t); // north wall
    add_box(cx + h, cz, t, h); // east wall
    add_box(cx - h, cz, t, h); // west wall
    // South wall, split around the gate gap (±GATE_HALF stays passable).
    let seg_hw = (h - GATE_HALF) / 2.0;
    let seg_cx = (h + GATE_HALF) / 2.0;
    add_box(cx - seg_cx, cz + h, seg_hw, t); // south-west of gate
    add_box(cx + seg_cx, cz + h, seg_hw, t); // south-east of gate
    add_box(cx, cz, 3.1, 3.1); // keep (6×6 base)
    for (sx, sz) in [(-1.0_f32, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        add_box(cx + sx * h, cz + sz * h, 1.5, 1.5); // corner towers
    }
}

// ── Autonomous economy ───────────────────────────────────────────────────────────────
//
// The rival funds itself purely from **taxes** (no producer/worker simulation — that's the
// player's depth). Its population pays a per-capita tithe every second; the rate is deliberately
// HIGHER than the player's so the AI, which has no hero out earning gold from kills/chests, can
// still afford to expand at a comparable pace. When it has banked the next building's cost (and a
// minimum interval has passed so growth reads as paced, not instant), it raises the next building
// in [`BUILD_ORDER`] on the next free plot — dwellings lift its population, which lifts its income,
// so it snowballs steadily like a Stronghold AI lord.

/// Headcount a fresh rival keep starts with (the garrison + the lord's household).
const RIVAL_BASE_POP: u32 = 4;
/// Each dwelling the rival raises shelters this many more taxpayers.
const RIVAL_POP_PER_HOUSE: u32 = 2;
/// Gold per second per head of population. Higher than the player's tithe — the AI has no hero
/// income, so taxes are its whole economy.
const RIVAL_TAX_PER_CAPITA: f64 = 0.85;
/// First building's cost; each subsequent one costs [`RIVAL_BUILD_COST_STEP`] more.
const RIVAL_BUILD_BASE_COST: f64 = 55.0;
const RIVAL_BUILD_COST_STEP: f64 = 22.0;
/// Minimum real seconds between two builds, so even a flush treasury expands at a watchable pace.
const RIVAL_BUILD_MIN_INTERVAL: f32 = 11.0;

/// The rival lord's run-state: treasury, population, and how many buildings it has raised. Reset on
/// New Game and (in a later step) round-tripped through the save.
#[derive(Resource)]
pub struct RivalState {
    pub gold: f64,
    pub population: u32,
    /// Buildings raised so far == the next free plot index.
    pub built: usize,
    /// Seconds since the last build (paces growth).
    since_build: f32,
}

impl Default for RivalState {
    fn default() -> Self {
        Self { gold: 0.0, population: RIVAL_BASE_POP, built: 0, since_build: RIVAL_BUILD_MIN_INTERVAL }
    }
}

impl RivalState {
    /// Cost to raise the next building.
    fn next_cost(&self) -> f64 {
        RIVAL_BUILD_BASE_COST + self.built as f64 * RIVAL_BUILD_COST_STEP
    }
}

/// Tax accrual + paced building. Gated on `Modal::None` (frozen with the rest of the sim while a
/// panel is open / paused), like every other simulation system.
fn rival_economy(
    time: Res<Time>,
    mut state: ResMut<RivalState>,
    mats: Option<Res<RivalMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let dt = time.delta_secs();
    // Collect taxes.
    state.gold += dt as f64 * state.population as f64 * RIVAL_TAX_PER_CAPITA;
    state.since_build += dt;
    // Try to raise the next building.
    if state.built >= PLOT_OFFSETS.len() {
        return; // bailey full
    }
    let Some(mats) = mats else { return };
    if state.since_build < RIVAL_BUILD_MIN_INTERVAL || state.gold < state.next_cost() {
        return;
    }
    let idx = state.built;
    let kind = BUILD_ORDER[idx % BUILD_ORDER.len()];
    spawn_building(&mut commands, &mut meshes, &mats, idx, kind);
    state.gold -= state.next_cost();
    state.built += 1;
    state.since_build = 0.0;
    if kind.is_house() {
        state.population += RIVAL_POP_PER_HOUSE;
    }
}

/// New run: wipe the rival's treasury/population and reap every building its economy raised (the
/// static fort — keep/walls/towers — is world geometry and stays). Mirrors `town::reset_town`.
fn reset_rival(
    mut state: ResMut<RivalState>,
    mut commands: Commands,
    stale: Query<Entity, Or<(With<RivalBuilding>, With<RivalSoldier>)>>,
) {
    *state = RivalState::default();
    for e in &stale {
        commands.entity(e).try_despawn();
    }
}

/// On a loaded game (`GameLoaded`), restore the rival's treasury/population/build-count from the
/// carried snapshot and reconcile its buildings to match (reap the live ones, raise one per built
/// plot) — the mirror of `town::restore_buildings`. Reads the value off the carried `SaveData`, not
/// the live `RivalState` (which load may write the same frame in undefined order). The static fort
/// (keep/walls/towers) is world geometry and untouched; the garrison re-tops-up on its own.
fn restore_rival(
    mut ev: MessageReader<crate::savegame::GameLoaded>,
    mut state: ResMut<RivalState>,
    mats: Option<Res<RivalMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    // Reap the prior in-process run's buildings AND live soldiers — the garrison is transient (not
    // saved), so a Continue starts from a clean fort and `rival_garrison` re-tops it up. Matches the
    // New-Game `reset_rival` sweep.
    stale: Query<Entity, Or<(With<RivalBuilding>, With<RivalSoldier>)>>,
) {
    let Some(crate::savegame::GameLoaded(data)) = ev.read().last() else { return };
    state.gold = data.rival_gold;
    // Floor population back to the founding base so an old save (which has none) still fields a
    // starter rival that can grow. (Population only ever grows today, so this never shrinks a real
    // value; if losses are added later, gate the floor on the `built == 0` old-save signature.)
    state.population = data.rival_population.max(RIVAL_BASE_POP);
    state.built = data.rival_built.min(PLOT_OFFSETS.len());
    state.since_build = RIVAL_BUILD_MIN_INTERVAL;
    for e in &stale {
        commands.entity(e).try_despawn();
    }
    let Some(mats) = mats else { return };
    for idx in 0..state.built {
        let kind = BUILD_ORDER[idx % BUILD_ORDER.len()];
        spawn_building(&mut commands, &mut meshes, &mats, idx, kind);
    }
}

/// Screenshot staging (`FOREST_RIVAL=<n>`): instantly raise `n` rival buildings (default: fill the
/// bailey) so a shot can frame a grown rival town without waiting out the economy. No-op otherwise.
fn stage_rival_for_shot(
    app: Res<State<crate::game_state::AppState>>,
    mats: Option<Res<RivalMats>>,
    mut state: ResMut<RivalState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut done: Local<bool>,
) {
    if *done || *app.get() != crate::game_state::AppState::Playing {
        return;
    }
    let Ok(val) = std::env::var("FOREST_RIVAL") else { *done = true; return };
    let Some(mats) = mats else { return }; // wait for the fort (and its mats) to exist
    *done = true;
    let n = val.parse::<usize>().unwrap_or(PLOT_OFFSETS.len()).min(PLOT_OFFSETS.len());
    for idx in state.built..n {
        let kind = BUILD_ORDER[idx % BUILD_ORDER.len()];
        spawn_building(&mut commands, &mut meshes, &mats, idx, kind);
        if kind.is_house() {
            state.population += RIVAL_POP_PER_HOUSE;
        }
    }
    state.built = state.built.max(n);
}

// ── Skirmishing garrison ───────────────────────────────────────────────────────────
//
// The rival keeps a small standing garrison of human soldiers (the player's own peasant-guard
// model, reskinned in crimson livery, NOT orks). They patrol near the fort and, the moment the
// hero or any of the player's townsfolk strays within sight AND within leash of the keep, they
// close and fight — bidirectionally: the soldier deals damage through the same channels the orks
// use (`PendingHeroDamage` / `NpcDamage`), and the hero (and the player's militia) cut them down
// through the same `Health` melee path the orks die by (they carry the `RivalSoldier` marker, now
// in the hero/guard target sets, so death routes through the shared `dying` fade). They are wholly
// outside the night siege — the orks never target them; this is a separate rivalry.

const GARRISON_BASE: usize = 4;
const GARRISON_MAX: usize = 8;
/// Seconds between respawns once the garrison has taken losses (slow — a wiped garrison rebuilds
/// over a few minutes, so clearing it out actually means something).
const GARRISON_RESPAWN_DELAY: f32 = 18.0;
/// Soldier hit points — sturdier than a single ork grunt is soft, so a lone hero must commit but a
/// war party makes short work of them.
const SOLDIER_HP: f32 = 120.0;
const SOLDIER_DMG: f32 = 11.0;
const SOLDIER_ATK_CD: f32 = 1.1;
const SOLDIER_MELEE: f32 = 1.7;
/// How near a foe must come before a soldier engages…
const SOLDIER_SIGHT: f32 = 13.0;
/// …and how far from the keep it will chase before breaking off (so they defend home, not roam the
/// island — "they only clash when someone crosses into the other's ground").
const SOLDIER_LEASH: f32 = 26.0;
const SOLDIER_TURN: f32 = 3.2;

/// A rival soldier's brain state. The body is a `villagers` peasant-guard biped (it keeps `Villager`
/// so `villager_drive`/`animate_biped` animate it for free); this carries the bits the town `Villager`
/// doesn't expose — its home anchor, strike cooldown, and a patrol target.
#[derive(Component)]
pub struct RivalSoldier {
    home: Vec2,
    atk_cd: f32,
    patrol: Vec2,
    patrol_t: f32,
    rng: u32,
}

/// Tiny LCG for patrol jitter (no `Math::random` in the deterministic core; this is cosmetic).
fn next_f(s: &mut u32) -> f32 {
    *s = s.wrapping_mul(1664525).wrapping_add(1013904223);
    (*s >> 8) as f32 / (1u32 << 24) as f32
}

/// Keep the garrison topped up to a population-scaled target: fill fast at boot, then replace
/// losses slowly. Only runs where the fort exists (its [`RivalMats`] resource is present — Home map).
#[allow(clippy::too_many_arguments)]
fn rival_garrison(
    time: Res<Time>,
    state: Res<RivalState>,
    mats: Option<Res<RivalMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    soldiers: Query<(), (With<RivalSoldier>, Without<crate::dying::Dying>)>,
    mut timer: Local<f32>,
    mut seed: Local<u32>,
) {
    if mats.is_none() {
        return; // no fort here → no garrison
    }
    let target = (GARRISON_BASE + state.built / 3).min(GARRISON_MAX);
    let have = soldiers.iter().count();
    if have >= target {
        return;
    }
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    // Fill the founding garrison quickly; replace later losses slowly.
    *timer = if have < GARRISON_BASE { 0.3 } else { GARRISON_RESPAWN_DELAY };
    *seed = seed.wrapping_add(1);
    let s = 0x5217_0000u32.wrapping_add(seed.wrapping_mul(2654435761));
    let mut r = s | 1;
    let a = next_f(&mut r) * std::f32::consts::TAU;
    let rad = 5.0 + next_f(&mut r) * 4.0; // ring between the keep (±3.1) and the walls (±12)
    let pos = RIVAL_CENTRE + Vec2::new(a.cos() * rad, a.sin() * rad);
    let e = crate::villagers::spawn_rival_soldier(&mut commands, &mut meshes, &mut creature_mats, RIVAL_CENTRE, pos, s);
    commands.entity(e).insert((
        RivalSoldier { home: RIVAL_CENTRE, atk_cd: 0.0, patrol: pos, patrol_t: 0.0, rng: r },
        crate::player::Health { hp: SOLDIER_HP, max: SOLDIER_HP },
    ));
}

/// The soldier brain: pick the nearest foe (hero or a player townsperson) in sight and within leash
/// of the keep, close to melee and strike on cooldown; otherwise patrol near home. Drives the
/// `Villager` pose fields (position/facing/moving/atk_anim) that `villager_drive` turns into walk +
/// swing clips. Gated on `Modal::None` with the rest of the sim.
#[allow(clippy::type_complexity)]
fn rival_combat(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    mut pending: ResMut<crate::player::PendingHeroDamage>,
    mut npc_dmg: ResMut<crate::villagers::NpcDamage>,
    mut soldiers: Query<(Entity, &mut RivalSoldier, &mut crate::villagers::Villager, &mut Transform), Without<crate::dying::Dying>>,
    townsfolk: Query<(Entity, &Transform), (With<crate::villagers::Townsfolk>, Without<crate::dying::Dying>, Without<RivalSoldier>)>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let tw = time.elapsed_secs_wrapped();
    let folk: Vec<(Entity, Vec2)> =
        townsfolk.iter().map(|(e, t)| (e, Vec2::new(t.translation.x, t.translation.z))).collect();

    for (e, mut sol, mut v, mut tf) in &mut soldiers {
        sol.atk_cd -= dt;
        let vpos = v.pos;
        // Nearest hostile in sight AND within leash of the keep.
        let mut best: Option<(f32, Vec2, Option<Entity>)> = None; // (dist, pos, townsperson victim)
        if hero.alive {
            let d = vpos.distance(hero.pos);
            if d < SOLDIER_SIGHT && sol.home.distance(hero.pos) < SOLDIER_LEASH {
                best = Some((d, hero.pos, None));
            }
        }
        for (fe, fp) in &folk {
            let d = vpos.distance(*fp);
            if d < SOLDIER_SIGHT && sol.home.distance(*fp) < SOLDIER_LEASH && best.map_or(true, |(bd, _, _)| d < bd) {
                best = Some((d, *fp, Some(*fe)));
            }
        }

        let cur_y = crate::steer::footing(vpos.x, vpos.y).unwrap_or(tf.translation.y);
        if let Some((d, tpos, victim)) = best {
            if d <= SOLDIER_MELEE {
                v.moving = false;
                let to = tpos - vpos;
                if to.length_squared() > 1e-4 {
                    let want = to.x.atan2(to.y);
                    v.facing += crate::steer::wrap_pi(want - v.facing).clamp(-SOLDIER_TURN * 2.0 * dt, SOLDIER_TURN * 2.0 * dt);
                }
                if sol.atk_cd <= 0.0 {
                    sol.atk_cd = SOLDIER_ATK_CD;
                    v.atk_anim = now; // fire the swing clip (read by villager_drive)
                    match victim {
                        None => pending.0 += SOLDIER_DMG,
                        Some(ve) => npc_dmg.0.push(crate::villagers::NpcHit { victim: ve, amount: SOLDIER_DMG, attacker: Some(e) }),
                    }
                }
            } else {
                let sp = v.speed * dt;
                step_toward(&mut v, tpos, sp, cur_y, dt);
            }
        } else {
            // Patrol near home.
            sol.patrol_t -= dt;
            if sol.patrol_t <= 0.0 || vpos.distance(sol.patrol) < 0.6 {
                let a = next_f(&mut sol.rng) * std::f32::consts::TAU;
                let rad = 5.0 + next_f(&mut sol.rng) * 4.5; // patrol the open ring inside the walls
                sol.patrol = sol.home + Vec2::new(a.cos() * rad, a.sin() * rad);
                sol.patrol_t = 3.0 + next_f(&mut sol.rng) * 4.0;
            }
            let sp = v.speed * 0.6 * dt;
            step_toward(&mut v, sol.patrol, sp, cur_y, dt);
        }

        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Steer a soldier's `Villager` pose one step toward `target` (shared by chase + patrol).
fn step_toward(v: &mut crate::villagers::Villager, target: Vec2, step: f32, cur_y: f32, dt: f32) {
    match crate::steer::advance(v.pos, v.facing, target, step, v.body_r, cur_y, SOLDIER_TURN * dt) {
        Some(s) => {
            v.facing = s.facing;
            v.pos = s.pos;
            v.moving = s.moving;
        }
        None => v.moving = false,
    }
}

// ── Plugin ─────────────────────────────────────────────────────────────────────────

pub struct RivalPlugin;

impl Plugin for RivalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RivalState>()
            // Fresh run wipes the rival's economy + reaps its raised buildings (the static fort,
            // built from `worldmap::build_step`, persists).
            .add_systems(OnExit(crate::game_state::AppState::StartScreen), reset_rival)
            .add_systems(OnExit(crate::game_state::AppState::GameOver), reset_rival)
            // Tax + paced building, garrison upkeep, and the soldier combat brain (frozen with the
            // sim under any panel / pause).
            .add_systems(
                Update,
                (rival_economy, rival_garrison, rival_combat).run_if(in_state(crate::game_state::Modal::None)),
            )
            // Reconcile the rival's economy + buildings to a loaded save (ungated; fires on a load).
            .add_systems(Update, restore_rival)
            // Screenshot staging (ungated; env-gated inside).
            .add_systems(Update, stage_rival_for_shot);
    }
}
