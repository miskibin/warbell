//! **Rival stronghold** — a *Stronghold: Crusader*-style AI opponent. A SECOND castle, raised in a
//! different biome (the NE desert) in a deliberately different **sandstone-and-crimson** style, so
//! it reads at a glance as "not yours". It contests the island for dominance: in later steps it
//! grows its own economy from taxes and buys buildings over time, and its soldiers skirmish with the
//! hero and the player's townsfolk when they meet. It is NOT part of the night siege — the orks
//! ignore it; it is a separate rivalry.
//!
//! Only the **keep** is permanent WORLD geometry (raised once with the island as a
//! `worldmap::build_step` phase, NOT per-run, so it persists across New Game / Continue in-process).
//! Everything else is EARNED: the bailey starts bare AND unwalled — exactly like the player's own
//! castle starts the run — and the rival's autonomous economy raises buildings on the plots and then
//! its curtain wall ([`raise_walls`], gated on [`WALL_AT`]) over time. None of that needs a new save
//! field: it all derives from `RivalState.built`, which already round-trips.
//!
//! Meshes reuse the castle's primitive helpers (`castle::bx`/`cyl`) and its procedural masonry/timber
//! texture generators ([`crate::castle::tex_stone`]/`tex_wood`), but baked in a warm **sandstone**
//! hue — so the fort reads as proper textured ashlar like ours (not flat blocks) while staying a
//! distinct desert colour. The "enemy" read is carried by the crimson banners + the rival's
//! desert-garbed garrison, not by a separate untextured look.

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
/// buildings + the skirmish ground, so the fort never straddles a dune terrace lip. `classify` checks
/// this BEFORE `is_river`, so widening it also keeps the desert river channel out of the fort's
/// grounds — enlarged so the producer buildings + worker gather-sites that now sit OUTSIDE the walls
/// (up to ~radius 28) still land on level desert.
pub const RIVAL_FLAT_R: f32 = 30.0;

/// **LOD radius.** Beyond this distance from the fort the per-frame rival sim (worker/soldier AI,
/// garrison upkeep) is FROZEN to save CPU + frames — out here the NPCs are off-screen or distant
/// specks, and the player can't interact with the garrison anyway (soldier sight/leash are ≤26). The
/// garrison also **lazy-spawns**: no soldiers exist until the hero first comes within this ring.
const RIVAL_LOD_R: f32 = 70.0;

/// Is the hero too far from the fort for its sim to matter this frame? (See [`RIVAL_LOD_R`].)
fn hero_far(p: Vec2) -> bool {
    p.distance(RIVAL_CENTRE) > RIVAL_LOD_R
}

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
/// Curtain-wall height — kept close to the player castle's own wall height (`castle::WALL_H` ≈ 1.35)
/// so the rival's ramparts don't tower over everything; the merlons + corner towers add the rest of
/// the silhouette. (Was 2.8 — read as a giant blank wall.)
const WALL_H: f32 = 1.5;
const WALL_T: f32 = 0.7;
/// Half-width of the south gate gap. Wide + left standing open (the leaves swing back against the
/// jambs in [`wall_parts`]) so the player can see straight into the bailey.
const GATE_HALF: f32 = 3.5;

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
// TEXTURED StandardMaterials (the same procedural masonry/timber generators the player castle uses,
// `castle::tex_stone`/`tex_wood`), but baked in a warm SANDSTONE hue — so the fort reads as proper
// ashlar like ours (not flat untextured blocks) while staying a distinct desert colour. Banners stay
// a flat crimson; the enemy read now comes from the crimson pennants + the rival's desert garrison.
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

/// The rival's textured-sandstone material set. Stored as a Resource so the economy tick can raise
/// new buildings + the curtain wall at runtime with the same look the keep was built with.
#[derive(Resource)]
struct RivalMats {
    sand: Handle<StandardMaterial>,
    sand_dark: Handle<StandardMaterial>,
    crimson: Handle<StandardMaterial>,
    timber: Handle<StandardMaterial>,
    iron: Handle<StandardMaterial>,
}

impl RivalMats {
    fn build(images: &mut Assets<Image>, std_mats: &mut Assets<StandardMaterial>) -> Self {
        // Textured slot: the hue is baked into the masonry/timber texture itself (the generator takes
        // a base hex), so base_color stays WHITE and the relief reads. Double-sided like the castle's
        // (our gable/slab meshes aren't all wound CCW-outward).
        let mut tex = |img: Image, rough: f32| {
            let t = images.add(img);
            std_mats.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: Some(t),
                perceptual_roughness: rough,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        };
        let sand = tex(crate::castle::tex_stone(SAND), 0.84);
        let sand_dark = tex(crate::castle::tex_stone(SAND_DARK), 0.84);
        let timber = tex(crate::castle::tex_wood(TIMBER, 3), 0.9);
        drop(tex); // release the &mut std_mats borrow before the solid-accent closure takes it
        // Flat solid for the small accent bits (banner cloth, iron bands) — no texture wanted there.
        let mut solid = |hex: u32, rough: f32| {
            std_mats.add(StandardMaterial { base_color: srgb(hex), perceptual_roughness: rough, ..default() })
        };
        let crimson = solid(CRIMSON, 0.85);
        let iron = solid(IRON, 0.6);
        Self { sand, sand_dark, crimson, timber, iron }
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
    // Towers stand a touch above the curtain (~+1.6) so they read as towers without the old
    // skyscraper height that the 2.8 wall produced.
    let th = WALL_H + 1.6;
    v.push((bx(2.8, th, 2.8, x, th / 2.0, z), RM::Sand));
    v.push((bx(3.0, 0.3, 3.0, x, th, z), RM::SandDark));
    // Local merlons (a tiny ring around the tower top).
    for (dx, dz) in [(-1.0_f32, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, -1.0), (0.0, 1.0)] {
        v.push((bx(0.42, 0.5, 0.42, x + dx, th + 0.35, z + dz), RM::SandDark));
    }
    banner(v, x, z, th, 1.6);
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

/// The curtain wall + gate + corner towers as a local-space part list (the keep is separate, built
/// statically; this whole ring is *earned* — the rival raises it once its economy is going, see
/// [`raise_walls`]). The bailey itself starts bare; the economy raises buildings on [`PLOT_OFFSETS`].
fn wall_parts() -> Vec<(Mesh, RM)> {
    let mut rest: Vec<(Mesh, RM)> = Vec::new();
    let h = WALL_HALF;
    // North / east / west walls (full runs); south wall split around the gate gap.
    wall_run(&mut rest, -h, -h, h, -h); // north
    wall_run(&mut rest, h, -h, h, h); // east
    wall_run(&mut rest, -h, -h, -h, h); // west
    wall_run(&mut rest, -h, h, -GATE_HALF, h); // south-west of gate
    wall_run(&mut rest, GATE_HALF, h, h, h); // south-east of gate
    // Gate: a wide opening under a sandstone lintel, the two timber leaves swung OPEN against the
    // jambs (narrow panels at the gate edges) so the centre stays clear — the player sees into the
    // bailey instead of a shut door.
    rest.push((bx(GATE_HALF * 2.0 + 0.8, 0.6, WALL_T + 0.3, 0.0, WALL_H + 0.1, h), RM::Sand)); // lintel
    let leaf_w = 0.55;
    for sx in [-1.0_f32, 1.0] {
        let lx = sx * (GATE_HALF - leaf_w * 0.5 - 0.05); // flush to the jamb, center open
        rest.push((bx(leaf_w, WALL_H - 0.2, 0.5, lx, (WALL_H - 0.2) / 2.0, h), RM::Timber)); // open leaf
        rest.push((bx(leaf_w - 0.08, 0.16, 0.56, lx, 0.6, h), RM::Iron)); // iron band on the leaf
    }
    // Corner towers.
    for (sx, sz) in [(-1.0_f32, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        corner_tower(&mut rest, sx * h, sz * h);
    }
    rest
}

// ── Economy buildings (raised on plots by the rival's autonomous economy) ─────────────

/// Building plot offsets (local XZ from [`RIVAL_CENTRE`]), paired by index with [`BUILD_ORDER`].
/// HOUSES sit INSIDE the curtain ring (|x|,|z| < [`WALL_HALF`]=12, clear of the keep ±~4); PRODUCERS
/// sit OUTSIDE the south wall (z > 12, in the enlarged flat apron) — a settlement that spills past
/// its walls like the player's own town, with the workshops/fields out by the resources. The gate
/// faces +Z, so the outside cluster is in front of it where the player approaches and can see it.
const PLOT_OFFSETS: [Vec2; 12] = [
    // Inside the walls — dwellings + the lord's household.
    Vec2::new(-8.0, -6.5),
    Vec2::new(8.0, -6.5),
    Vec2::new(-8.5, 0.5),
    Vec2::new(8.5, 0.5),
    Vec2::new(0.0, -8.5),
    // Outside the walls — producers spread across the southern apron (gate side).
    Vec2::new(-19.0, 15.0),
    Vec2::new(9.0, 21.0),
    Vec2::new(19.0, 15.0),
    Vec2::new(-9.0, 21.0),
    Vec2::new(-22.0, 6.0),
    Vec2::new(22.0, 6.0),
    Vec2::new(0.0, 23.0),
];

/// True if plot `idx` sits OUTSIDE the curtain wall (a producer in the open apron) rather than inside
/// the bailey. Drives the worker gather-loop (outside workers walk out to a resource site and back).
fn plot_is_outside(idx: usize) -> bool {
    PLOT_OFFSETS.get(idx).is_some_and(|o| o.x.abs() >= WALL_HALF || o.y.abs() >= WALL_HALF)
}

/// The kind of building the rival raises. A `House` is a sandstone desert dwelling (lifts the
/// rival's population → more tax income, keeping its bailey cohesive with the sandstone keep/walls);
/// the producers (`Farm`/`Lumber`/`Mine`) reuse the **player town's own models** so the rival reads
/// as a real economic settlement (a Stronghold-style mirror), each staffed by a desert worker.
#[derive(Clone, Copy, PartialEq)]
pub enum RivalKind {
    House,
    Farm,
    Lumber,
    Mine,
}

impl RivalKind {
    fn is_house(self) -> bool {
        matches!(self, RivalKind::House)
    }
    /// The trade of the worker that mans this building (None for a house).
    fn worker_trade(self) -> Option<crate::villagers::Trade> {
        use crate::villagers::Trade;
        match self {
            RivalKind::Farm => Some(Trade::Farmer),
            RivalKind::Lumber => Some(Trade::Woodcutter),
            RivalKind::Mine => Some(Trade::Miner),
            RivalKind::House => None,
        }
    }
    /// Collision half-extents `(hw, hd)` — sized to the structure's footprint (+ a small body
    /// margin). Box is centred on the plot; mirrors the castle/town per-structure boxes.
    fn block(self) -> (f32, f32) {
        match self {
            RivalKind::House => (1.5, 1.4),
            RivalKind::Farm => (1.5, 1.4),
            RivalKind::Lumber => (1.4, 1.3),
            RivalKind::Mine => (1.5, 1.4),
        }
    }
}

/// The fixed order the rival builds in — houses (population/income) interleaved with the three
/// producers so its settlement grows steadily, Stronghold-style. 10 plots.
const BUILD_ORDER: [RivalKind; 12] = [
    // Index-paired with PLOT_OFFSETS: the 5 inside plots are houses, the 7 outside plots producers.
    RivalKind::House,
    RivalKind::House,
    RivalKind::House,
    RivalKind::House,
    RivalKind::House,
    RivalKind::Farm,
    RivalKind::Lumber,
    RivalKind::Mine,
    RivalKind::Farm,
    RivalKind::Lumber,
    RivalKind::Mine,
    RivalKind::Farm,
];

/// Tags a building the rival economy raised on plot `idx` (so later steps can damage/topple it).
#[derive(Component)]
pub struct RivalBuilding {
    pub idx: usize,
}

/// Tags a desert worker the rival raised to man a producer on plot `idx` — cosmetic (it potters
/// near its building; the rival's economy is tax-funded, not producer-fed). Driven by
/// [`rival_workers`], excluded from the town's ambient wander brain.
#[derive(Component)]
pub struct RivalWorker {
    /// The building's yard — where the worker deposits / idles between trips.
    home: Vec2,
    /// The outside resource spot (a "lasek"/"kamieniołom"/field) the worker walks OUT to and gathers
    /// at, further from the fort than `home`. For an inside producer this equals `home` (no trip).
    site: Vec2,
    /// Current destination this trip (toggles between `home` and `site`).
    patrol: Vec2,
    patrol_t: f32,
    /// Cooldown until the next work swing (hoe/chop/pick) while standing at the site.
    work_cd: f32,
    rng: u32,
}

/// Spawn one rival building at plot `idx` (world-snapped, its own entity), plus its desert worker if
/// it's a producer. Shared by the runtime economy tick, the load-restore reconcile, and the
/// screenshot stager. `village` is the player town's textured material set (for the producer models);
/// `rival` the sandstone set (for the desert houses).
fn spawn_building(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    rival: &RivalMats,
    village: &crate::castle::Mats,
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
    // Houses: sandstone (RivalMats + desert_house). Producers: the player town's textured models +
    // material set, so they're literally the same farm/saw-shed/pit-head the player builds.
    commands.entity(parent).with_children(|p| {
        match kind {
            RivalKind::House => {
                for (mesh, slot) in desert_house(0.0, 0.0) {
                    p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(rival.get(slot)), Transform::default()));
                }
            }
            RivalKind::Farm | RivalKind::Lumber | RivalKind::Mine => {
                let parts = match kind {
                    RivalKind::Farm => crate::town_meshes::farm_parts(),
                    RivalKind::Lumber => crate::town_meshes::woodcutter_parts(),
                    _ => crate::town_meshes::mine_parts(),
                };
                for (mesh, m) in parts {
                    p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(village.get(m)), Transform::default()));
                }
            }
        }
    });
    // Make it SOLID. Dedupe first (an in-process Continue reaps + re-raises the same plots, which
    // would otherwise stack duplicate boxes at the same spot).
    let (hw, hd) = kind.block();
    crate::blockers::remove_box_near(wx, wz, 0.05);
    crate::blockers::add_box(wx, wz, hw, hd);
    // A producer is manned by one desert worker, spawned in the working yard (+X side) clear of the
    // building's collision box (half-extent ≤1.5) so it doesn't start stuck inside the wall.
    if let Some(trade) = kind.worker_trade() {
        // `home` (idle/deposit) AND `site` (gather) both sit on the SAME outward radial from the fort
        // centre, OUTSIDE the building. The old `home = (wx+2.3, wz)` was a fixed +X offset while
        // `site = home + out*reach` reached along the radial — for plots not due-east of the centre
        // those directions fought, dropping `site` back onto the building's own collision box, so the
        // worker jammed against the wall and never reached its work spot (it stood there idle). Keeping
        // both on the radial clears the box (half-extent ≤1.5) at every plot, so the worker actually
        // walks out to chop/mine/hoe and back.
        let bpos = Vec2::new(wx, wz);
        let out = {
            let o = (bpos - RIVAL_CENTRE).normalize_or_zero();
            if o == Vec2::ZERO { Vec2::X } else { o }
        };
        let home = bpos + out * 2.6; // deposit/idle spot just clear of the building box
        let reach = if plot_is_outside(idx) {
            match kind {
                RivalKind::Lumber | RivalKind::Mine => 5.0,
                _ => 2.5, // farm field, just past the building
            }
        } else {
            0.0
        };
        let site = home + out * reach;
        let wseed = 0x9a17_0000u32.wrapping_add((idx as u32).wrapping_mul(2654435761)) | 1;
        let e = crate::villagers::spawn_rival_worker(commands, meshes, creature_mats, trade, home, home, wseed);
        commands.entity(e).insert((
            RivalWorker { home, site, patrol: site, patrol_t: 0.0, work_cd: 0.0, rng: wseed },
            // A soft body so the hero can cut the rival's labourers down too (a raiding tactic —
            // starve the enemy economy). Far frailer than a soldier; pays a peasant-tier bounty.
            crate::player::Health { hp: WORKER_HP, max: WORKER_HP },
        ));
    }
}

/// Tags the rival's curtain-wall / gate / tower ring — one despawn target, and the marker the
/// economy/restore checks so it raises the walls exactly once. The walls are EARNED (see
/// [`raise_walls`]), so unlike the keep they're dynamic, not permanent world geometry.
#[derive(Component)]
pub struct RivalWalls;

/// How many buildings the rival raises before it can afford to wall its bailey. Below this it stands
/// open like the player's day-1 castle; at/after it, the economy puts up the full curtain + towers.
/// Derived purely from `RivalState.built`, so it round-trips the save for free (no new `SaveData`
/// field) and resets with the economy. At ~75 s/build (+ a rising cost) that's a good ~10+ minutes
/// in — the rival stays an OPEN settlement (houses + producers spilling outside) for most of a
/// campaign, then walls its bailey late.
const WALL_AT: usize = 8;

// ── Build (a worldmap build phase) ──────────────────────────────────────────────────

/// Raise the rival stronghold at [`RIVAL_CENTRE`]. Called once from `worldmap::build_step`.
///
/// Only the KEEP is raised here (permanent world geometry). The bailey starts bare AND wall-less —
/// just like the player's own castle starts unwalled — and the rival's economy raises buildings and
/// then the curtain wall over time ([`maybe_raise_walls`]).
pub fn build(commands: &mut Commands, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, std_mats: &mut Assets<StandardMaterial>) {
    if crate::worldmap::MapId::from_u8(crate::worldmap::current_map_u8()) != crate::worldmap::MapId::Home {
        return;
    }
    let mats = RivalMats::build(images, std_mats);
    let centre = RIVAL_CENTRE;
    let y = ground_at_world(centre.x, centre.y).unwrap_or(0.0);

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
        for (mesh, slot) in keep_parts() {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.get(slot)), Transform::default()));
        }
    });

    // Hand the material set to the economy tick so it can raise buildings / walls at runtime.
    commands.insert_resource(mats);

    // The keep is permanent world geometry → its collision box is registered once and never removed.
    // The walls' boxes are registered later, when the walls are earned (see [`raise_walls`]).
    crate::blockers::add_box(centre.x, centre.y, 3.1, 3.1); // keep (6×6 base)
}

/// Raise the curtain wall + gate + corner towers as one dynamic structure (tagged [`RivalWalls`])
/// and register their collision boxes. Idempotent — a no-op if the walls already stand — so the
/// economy tick, the load-restore, and the screenshot stager can all call it freely. The south gate
/// gap is left open so units sally through it, like the player castle's gate gaps.
fn raise_walls(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &RivalMats, exists: bool) {
    if exists {
        return;
    }
    let centre = RIVAL_CENTRE;
    let y = ground_at_world(centre.x, centre.y).unwrap_or(0.0);
    let root = commands
        .spawn((Transform::from_xyz(centre.x, y, centre.y), Visibility::Visible, BiomeEntity, RivalEntity, RivalWalls))
        .id();
    commands.entity(root).with_children(|p| {
        for (mesh, slot) in wall_parts() {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.get(slot)), Transform::default()));
        }
    });
    register_wall_blockers(centre);
}

/// Curtain-wall / tower collision boxes (axis-aligned — the fort is square). Deduped first so an
/// in-process Continue (which reaps + re-raises the walls) doesn't stack duplicates.
fn register_wall_blockers(centre: Vec2) {
    use crate::blockers::{add_box, remove_box_near};
    let (cx, cz) = (centre.x, centre.y);
    let h = WALL_HALF;
    let t = WALL_T / 2.0 + 0.2; // wall half-depth + a small body margin
    let seg_hw = (h - GATE_HALF) / 2.0;
    let seg_cx = (h + GATE_HALF) / 2.0;
    // (centre, hw, hd) for every wall/tower box.
    let boxes = [
        (cx, cz - h, h, t),            // north wall
        (cx + h, cz, t, h),            // east wall
        (cx - h, cz, t, h),            // west wall
        (cx - seg_cx, cz + h, seg_hw, t), // south-west of gate
        (cx + seg_cx, cz + h, seg_hw, t), // south-east of gate
        (cx - h, cz - h, 1.5, 1.5),    // corner towers
        (cx + h, cz - h, 1.5, 1.5),
        (cx - h, cz + h, 1.5, 1.5),
        (cx + h, cz + h, 1.5, 1.5),
    ];
    for (bx, bz, hw, hd) in boxes {
        remove_box_near(bx, bz, 0.05);
        add_box(bx, bz, hw, hd);
    }
}

/// Drop every economy-building collision box (one per plot). Pairs with [`clear_wall_blockers`] so a
/// reset/Continue rebuilds the rival's blocker set purely from the live `built` count — no phantom
/// boxes at plots the new run hasn't raised yet. `spawn_building` re-adds the live ones.
fn clear_building_blockers() {
    for off in PLOT_OFFSETS {
        crate::blockers::remove_box_near(RIVAL_CENTRE.x + off.x, RIVAL_CENTRE.y + off.y, 0.05);
    }
}

/// Drop the curtain-wall / tower collision boxes (so an in-process reset/Continue that hasn't earned
/// the walls back doesn't leave invisible ramparts standing). The keep box stays — the keep is
/// permanent. Positions mirror [`register_wall_blockers`].
fn clear_wall_blockers(centre: Vec2) {
    let (cx, cz) = (centre.x, centre.y);
    let h = WALL_HALF;
    let seg_cx = (h + GATE_HALF) / 2.0;
    for (bx, bz) in [
        (cx, cz - h), (cx + h, cz), (cx - h, cz),
        (cx - seg_cx, cz + h), (cx + seg_cx, cz + h),
        (cx - h, cz - h), (cx + h, cz - h), (cx - h, cz + h), (cx + h, cz + h),
    ] {
        crate::blockers::remove_box_near(bx, bz, 0.05);
    }
}

// ── Autonomous economy ───────────────────────────────────────────────────────────────
//
// The rival funds itself purely from **taxes** — its producers are staffed by workers for *show*
// (they really swing tools at the yards, see `rival_workers`), but the gold comes from the tithe,
// not from hauled wood/stone (real production is the player's depth). Its population pays a
// per-capita tithe every second. When it has banked the next building's cost AND a minimum interval
// has passed (a SLOW, watchable pace — minutes per build, not seconds), it raises the next building
// in [`BUILD_ORDER`] on the next free plot — houses lift its population, which lifts its income, so
// it grows steadily over the campaign like a Stronghold AI lord.

/// Headcount a fresh rival keep starts with (the garrison + the lord's household).
const RIVAL_BASE_POP: u32 = 4;
/// Each dwelling the rival raises shelters this many more taxpayers.
const RIVAL_POP_PER_HOUSE: u32 = 2;
/// Gold per second per head of population. The AI has no hero income, so taxes are its whole
/// economy. Deliberately MODEST — the rival should grow over a whole campaign, not erect its full
/// fort in two minutes (the earlier 0.85 + cheap builds did exactly that).
const RIVAL_TAX_PER_CAPITA: f64 = 0.55;
/// First building's cost; each subsequent one costs [`RIVAL_BUILD_COST_STEP`] more.
const RIVAL_BUILD_BASE_COST: f64 = 110.0;
const RIVAL_BUILD_COST_STEP: f64 = 45.0;
/// Minimum real seconds between two builds, so even a flush treasury expands at a slow, watchable
/// pace — a building every few minutes, not every ten seconds.
const RIVAL_BUILD_MIN_INTERVAL: f32 = 75.0;

/// The rival lord's run-state: treasury, population, and how many buildings it has raised. Reset on
/// New Game and (in a later step) round-tripped through the save.
/// Hit points of the rival keep — razed by the hero's blows (see [`rival_fort_damage`]). At 0 the
/// whole fort topples, the daily raids stop, and a bounty drops. `keep_hp` is transient (resets to
/// full on load — the *objective* `destroyed` is what round-trips); a war party makes short work of
/// it, a lone hero must commit.
const RIVAL_KEEP_HP: f32 = 1400.0;

#[derive(Resource)]
pub struct RivalState {
    pub gold: f64,
    pub population: u32,
    /// Buildings raised so far == the next free plot index.
    pub built: usize,
    /// Seconds since the last build (paces growth).
    since_build: f32,
    /// Remaining keep HP (transient — not saved; full on a fresh load unless `destroyed`).
    keep_hp: f32,
    /// The fort has been razed by the player: no more buildings/garrison/raids, fort meshes toppled.
    /// Round-trips the save so a destroyed fort stays destroyed.
    pub destroyed: bool,
}

impl Default for RivalState {
    fn default() -> Self {
        Self {
            gold: 0.0,
            population: RIVAL_BASE_POP,
            built: 0,
            since_build: RIVAL_BUILD_MIN_INTERVAL,
            keep_hp: RIVAL_KEEP_HP,
            destroyed: false,
        }
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
    village: Option<Res<crate::castle::VillageMats>>,
    walls: Query<(), With<RivalWalls>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
) {
    if state.destroyed {
        return; // razed — the lord is gone, no more tax or building
    }
    let dt = time.delta_secs();
    // Collect taxes.
    state.gold += dt as f64 * state.population as f64 * RIVAL_TAX_PER_CAPITA;
    state.since_build += dt;
    // Try to raise the next building.
    if state.built >= PLOT_OFFSETS.len() {
        return; // bailey full
    }
    let (Some(mats), Some(village)) = (mats, village) else { return }; // wait for the fort + town models
    if state.since_build < RIVAL_BUILD_MIN_INTERVAL || state.gold < state.next_cost() {
        return;
    }
    let idx = state.built;
    let kind = BUILD_ORDER[idx % BUILD_ORDER.len()];
    spawn_building(&mut commands, &mut meshes, &mut creature_mats, &mats, &village.0, idx, kind);
    state.gold -= state.next_cost();
    state.built += 1;
    state.since_build = 0.0;
    if kind.is_house() {
        state.population += RIVAL_POP_PER_HOUSE;
    }
    // Once it's a few buildings deep, the rival walls its bailey (once).
    if state.built >= WALL_AT {
        raise_walls(&mut commands, &mut meshes, &mats, !walls.is_empty());
    }
}

/// New run: wipe the rival's treasury/population and reap every building its economy raised AND its
/// earned curtain wall (only the keep is permanent world geometry). Mirrors `town::reset_town`.
fn reset_rival(
    mut state: ResMut<RivalState>,
    mut commands: Commands,
    stale: Query<Entity, Or<(With<RivalBuilding>, With<RivalSoldier>, With<RivalWorker>, With<RivalWalls>)>>,
) {
    *state = RivalState::default();
    for e in &stale {
        commands.entity(e).try_despawn();
    }
    clear_building_blockers();
    clear_wall_blockers(RIVAL_CENTRE);
}

/// On a loaded game (`GameLoaded`), restore the rival's treasury/population/build-count from the
/// carried snapshot and reconcile its buildings to match (reap the live ones, raise one per built
/// plot, and re-raise the curtain wall iff the save earned it) — the mirror of
/// `town::restore_buildings`. Reads the value off the carried `SaveData`, not the live `RivalState`
/// (which load may write the same frame in undefined order). The keep is permanent world geometry and
/// untouched; the garrison re-tops-up on its own.
fn restore_rival(
    mut ev: MessageReader<crate::savegame::GameLoaded>,
    mut state: ResMut<RivalState>,
    mats: Option<Res<RivalMats>>,
    village: Option<Res<crate::castle::VillageMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    // Reap the prior in-process run's buildings, workers AND live soldiers — the garrison + workers
    // are transient (not saved), so a Continue starts clean and `spawn_building`/`rival_garrison`
    // re-raise them. Matches the New-Game `reset_rival` sweep.
    stale: Query<Entity, Or<(With<RivalBuilding>, With<RivalSoldier>, With<RivalWorker>, With<RivalWalls>)>>,
    // The keep is normally permanent (built once in `worldmap::build`), so it's reaped ONLY when the
    // save says the fort was razed — never on an ordinary Continue.
    keep: Query<Entity, With<RivalKeep>>,
) {
    let Some(crate::savegame::GameLoaded(data)) = ev.read().last() else { return };
    state.gold = data.rival_gold;
    // Floor population back to the founding base so an old save (which has none) still fields a
    // starter rival that can grow. (Population only ever grows today, so this never shrinks a real
    // value; if losses are added later, gate the floor on the `built == 0` old-save signature.)
    state.population = data.rival_population.max(RIVAL_BASE_POP);
    state.built = data.rival_built.min(PLOT_OFFSETS.len());
    state.since_build = RIVAL_BUILD_MIN_INTERVAL;
    state.destroyed = data.rival_destroyed;
    state.keep_hp = RIVAL_KEEP_HP; // transient — full again on load unless razed
    // Razed save: reap the whole fort (the boot rebuild raised it; tear it down) + clear ALL its
    // collision, and skip the normal rebuild below.
    if data.rival_destroyed {
        for e in &stale {
            commands.entity(e).try_despawn();
        }
        for e in &keep {
            commands.entity(e).try_despawn();
        }
        clear_building_blockers();
        clear_wall_blockers(RIVAL_CENTRE);
        crate::blockers::remove_box_near(RIVAL_CENTRE.x, RIVAL_CENTRE.y, 0.3);
        return;
    }
    for e in &stale {
        commands.entity(e).try_despawn();
    }
    // Drop the old run's blocker boxes; we re-add live ones below (buildings per plot, walls only if
    // this save earned the curtain).
    clear_building_blockers();
    clear_wall_blockers(RIVAL_CENTRE);
    let (Some(mats), Some(village)) = (mats, village) else { return };
    for idx in 0..state.built {
        let kind = BUILD_ORDER[idx % BUILD_ORDER.len()];
        spawn_building(&mut commands, &mut meshes, &mut creature_mats, &mats, &village.0, idx, kind);
    }
    // Walls are derived from `built` (no separate save field), so re-raise them iff this save is past
    // the threshold. `exists = false` — we just despawned the live ring above.
    if state.built >= WALL_AT {
        raise_walls(&mut commands, &mut meshes, &mats, false);
    }
}

/// Screenshot staging (`FOREST_RIVAL=<n>`): instantly raise `n` rival buildings (default: fill the
/// bailey) so a shot can frame a grown rival town without waiting out the economy. No-op otherwise.
fn stage_rival_for_shot(
    app: Res<State<crate::game_state::AppState>>,
    mats: Option<Res<RivalMats>>,
    village: Option<Res<crate::castle::VillageMats>>,
    walls: Query<(), With<RivalWalls>>,
    mut state: ResMut<RivalState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    mut done: Local<bool>,
) {
    if *done || *app.get() != crate::game_state::AppState::Playing {
        return;
    }
    let Ok(val) = std::env::var("FOREST_RIVAL") else { *done = true; return };
    let (Some(mats), Some(village)) = (mats, village) else { return }; // wait for the fort + town models
    *done = true;
    let n = val.parse::<usize>().unwrap_or(PLOT_OFFSETS.len()).min(PLOT_OFFSETS.len());
    for idx in state.built..n {
        let kind = BUILD_ORDER[idx % BUILD_ORDER.len()];
        spawn_building(&mut commands, &mut meshes, &mut creature_mats, &mats, &village.0, idx, kind);
        if kind.is_house() {
            state.population += RIVAL_POP_PER_HOUSE;
        }
    }
    state.built = state.built.max(n);
    // Match the runtime rule: a staged grown rival also shows its earned curtain wall.
    if state.built >= WALL_AT {
        raise_walls(&mut commands, &mut meshes, &mats, !walls.is_empty());
    }
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
/// war party makes short work of them. (Was 120/11 — too soft, the garrison folded instantly.)
const SOLDIER_HP: f32 = 220.0;
const SOLDIER_DMG: f32 = 16.0;
/// Bounty for cutting down a rival soldier — gold (scaled by the hero's bounty boon) + flat XP, on
/// par with a tough ork so the skirmish actually rewards the hero. Read in `player::combat`.
pub const SOLDIER_BOUNTY_GOLD: i64 = 12;
pub const SOLDIER_BOUNTY_XP: i64 = 25;
const SOLDIER_ATK_CD: f32 = 1.1;
/// A rival labourer (farmer/woodcutter/miner) — an unarmed non-combatant, so it dies in a hit or two
/// and pays only a token peasant bounty. Read in `player::combat`.
const WORKER_HP: f32 = 40.0;
pub const WORKER_BOUNTY_GOLD: i64 = 3;
pub const WORKER_BOUNTY_XP: i64 = 8;
const SOLDIER_MELEE: f32 = 1.7;
/// A rival bowman's per-shaft hit — the soldier blade × the same 1.5 mark-up the town's
/// `archer_damage` puts on its guards' swords (slower cadence, paid back per hit).
const SOLDIER_ARROW_DMG: f32 = 24.0;
/// Nock-to-nock cadence — matches the town archers' `ARCHER_ATTACK_CD`.
const RIVAL_ARCHER_CD: f32 = 2.5;
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

/// A rival soldier who is a **desert bowman** — the rival mirror of the town's `villagers::Archer`
/// trait: about a third of the garrison and of each raid party ([`crate::villagers::is_bowman`] on
/// the spawn seed, the town's own split). Carriers fight at [`crate::villagers::BOW_RANGE`] instead
/// of closing to melee; their brains ([`rival_combat`] / [`rival_raid_brain`]) drive the same
/// draw-and-loose clip and release a crimson-fletched [`crate::projectile::ArrowSpawn`] on the
/// clip's release frame via [`bow_cycle`].
#[derive(Component)]
pub struct RivalBow {
    /// The in-flight shot's arrow has left the string (guards against double-spawning while the
    /// release-frame window is crossed over several frames) — same latch as `villagers::Archer`.
    loosed: bool,
}

/// One tick of a rival bowman's draw-and-loose state machine (mirrors the archer branch of
/// `villagers::guard_combat`): starts a new draw when `atk_cd` allows (stamping `v.atk_anim`, which
/// `villager_drive` turns into the bow clip), and returns `Some(bow_position)` exactly ONCE at the
/// clip's release frame — the caller then spawns the arrow / applies its damage. `dir` is the flat
/// aim direction (for the half-step the loose point leads toward the target); `base_y` the body's
/// current ground height.
fn bow_cycle(
    now: f32,
    v: &mut crate::villagers::Villager,
    atk_cd: &mut f32,
    bw: &mut RivalBow,
    dir: Vec2,
    base_y: f32,
) -> Option<Vec3> {
    let secs = crate::villagers::BOW_SHOT_SECS;
    if v.atk_anim > 0.0 && now - v.atk_anim < secs {
        let p = (now - v.atk_anim) / secs;
        if p >= crate::player::anim::BOW_RELEASE_P && !bw.loosed {
            bw.loosed = true;
            let d3 = Vec3::new(dir.x, 0.0, dir.y).normalize_or_zero();
            // Loose from the bow: chest height, half a step toward the mark (as the town archers).
            return Some(Vec3::new(v.pos.x, base_y + 1.3, v.pos.y) + d3 * 0.45);
        }
    } else if *atk_cd <= 0.0 {
        *atk_cd = RIVAL_ARCHER_CD;
        v.atk_anim = now; // start the draw (villager_drive plays the bow clip off this)
        bw.loosed = false;
    }
    None
}

/// Is this bowman still mid-draw? (The engagement logic must stay planted through the whole clip
/// even if the mark drifts out of range — same stickiness as the town archers' `mid_shot`.)
fn mid_shot(v: &crate::villagers::Villager, now: f32) -> bool {
    v.atk_anim > 0.0 && now - v.atk_anim < crate::villagers::BOW_SHOT_SECS
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
    hero: Res<crate::player::HeroState>,
    mats: Option<Res<RivalMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    soldiers: Query<(), (With<RivalSoldier>, Without<RivalRaider>, Without<crate::dying::Dying>)>,
    mut timer: Local<f32>,
    mut seed: Local<u32>,
) {
    if mats.is_none() || state.destroyed {
        return; // no fort here (or it's been razed) → no garrison
    }
    // LOD / lazy-spawn: don't field (or top up) the garrison until the hero is near the fort. Far
    // away the soldiers would just be unseen bodies burning CPU on their patrol brain.
    if hero_far(hero.pos) {
        return;
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
    // About a third of the garrison are desert bowmen — the same seed-hashed one-in-three split
    // the player's militia uses (`villagers::is_bowman`), so the two towns mirror each other.
    let e = if crate::villagers::is_bowman(s) {
        let e = crate::villagers::spawn_rival_archer(&mut commands, &mut meshes, &mut creature_mats, RIVAL_CENTRE, pos, s);
        commands.entity(e).insert(RivalBow { loosed: true });
        e
    } else {
        crate::villagers::spawn_rival_soldier(&mut commands, &mut meshes, &mut creature_mats, RIVAL_CENTRE, pos, s)
    };
    commands.entity(e).insert((
        RivalSoldier { home: RIVAL_CENTRE, atk_cd: 0.0, patrol: pos, patrol_t: 0.0, rng: r },
        crate::player::Health { hp: SOLDIER_HP, max: SOLDIER_HP },
    ));
}

/// The soldier brain: pick the nearest foe (hero or a player townsperson) in sight and within leash
/// of the keep, close to melee and strike on cooldown; otherwise patrol near home. A **bowman**
/// ([`RivalBow`]) spots from farther ([`crate::villagers::BOW_RANGE`]) and never closes — he plants,
/// tracks, and looses crimson-fletched arrows on the town archers' cadence. Drives the
/// `Villager` pose fields (position/facing/moving/atk_anim) that `villager_drive` turns into walk +
/// swing/draw clips. Gated on `Modal::None` with the rest of the sim.
#[allow(clippy::type_complexity)]
fn rival_combat(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    mut pending: ResMut<crate::player::PendingHeroDamage>,
    mut npc_dmg: ResMut<crate::villagers::NpcDamage>,
    mut arrows: ResMut<crate::projectile::ArrowSpawns>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    hero_ent: Query<Entity, With<crate::player::Hero>>,
    mut soldiers: Query<(Entity, &mut RivalSoldier, Option<&mut RivalBow>, &mut crate::villagers::Villager, &mut Transform), (Without<crate::dying::Dying>, Without<RivalRaider>)>,
    townsfolk: Query<(Entity, &Transform), (With<crate::villagers::Townsfolk>, Without<crate::dying::Dying>, Without<RivalSoldier>)>,
) {
    // LOD: with the hero far, nothing can reach a soldier (sight/leash ≤26) and they're off-screen —
    // freeze the whole brain to save CPU/frames. (Townsfolk live ~100 from the fort, beyond leash, so
    // the hero's distance alone decides this.)
    if hero_far(hero.pos) {
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let tw = time.elapsed_secs_wrapped();
    let hero_e = hero_ent.iter().next();
    // (entity, flat pos, chest height) — the height feeds a bowman's aim point.
    let folk: Vec<(Entity, Vec2, f32)> =
        townsfolk.iter().map(|(e, t)| (e, Vec2::new(t.translation.x, t.translation.z), t.translation.y + 1.0)).collect();

    for (e, mut sol, mut bow, mut v, mut tf) in &mut soldiers {
        sol.atk_cd -= dt;
        let vpos = v.pos;
        let sight = if bow.is_some() { crate::villagers::BOW_RANGE } else { SOLDIER_SIGHT };
        // Nearest hostile in sight AND within leash of the keep. (aim_y = the mark's chest height.)
        let mut best: Option<(f32, Vec2, f32, Option<Entity>)> = None; // (dist, pos, aim_y, townsperson victim)
        if hero.alive {
            let d = vpos.distance(hero.pos);
            if d < sight && sol.home.distance(hero.pos) < SOLDIER_LEASH {
                best = Some((d, hero.pos, hero.y + 1.0, None));
            }
        }
        for (fe, fp, fy) in &folk {
            let d = vpos.distance(*fp);
            if d < sight && sol.home.distance(*fp) < SOLDIER_LEASH && best.map_or(true, |(bd, ..)| d < bd) {
                best = Some((d, *fp, *fy, Some(*fe)));
            }
        }

        let cur_y = crate::steer::footing(vpos.x, vpos.y).unwrap_or(tf.translation.y);
        if let Some((d, tpos, aim_y, victim)) = best {
            // A bowman never closes: inside bow range (or mid-draw already) he plants, tracks the
            // mark, and shoots on his cadence — the mirror of the town archers' engagement.
            if let Some(bw) = bow.as_deref_mut().filter(|_| d < crate::villagers::BOW_RANGE || mid_shot(&v, now)) {
                v.moving = false;
                let to = tpos - vpos;
                if to.length_squared() > 1e-4 {
                    let want = to.x.atan2(to.y);
                    v.facing += crate::steer::wrap_pi(want - v.facing).clamp(-SOLDIER_TURN * 2.0 * dt, SOLDIER_TURN * 2.0 * dt);
                }
                if let Some(from) = bow_cycle(now, &mut v, &mut sol.atk_cd, bw, to, tf.translation.y) {
                    // Aim at the mark's live chest; the hero-aimed shaft still needs a target
                    // entity for the spawn — the hero body works (the rival hit test finds the
                    // hero by position, never through this handle).
                    let target = victim.or(hero_e);
                    if let Some(te) = target {
                        arrows.0.push(crate::projectile::ArrowSpawn {
                            from,
                            aim: Vec3::new(tpos.x, aim_y, tpos.y),
                            target: te,
                            shooter: e,
                            damage: SOLDIER_ARROW_DMG,
                            rival: true,
                        });
                        // The string's twang, on the same earshot the town archers use.
                        if vpos.distance(hero.pos) < crate::villagers::BOW_SFX_RADIUS {
                            cues.write(crate::audio::AudioCue::BowShot(from));
                        }
                    }
                }
            } else if d <= SOLDIER_MELEE {
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
                        None => {
                            pending.0 += SOLDIER_DMG;
                            pending.1 = to.normalize_or_zero(); // directional hit-shake
                        }
                        Some(ve) => npc_dmg.0.push(crate::villagers::NpcHit { victim: ve, amount: SOLDIER_DMG, attacker: Some(e) }),
                    }
                }
            } else {
                let sp = v.speed * dt;
                step_toward(&mut v, tpos, sp, cur_y, dt);
            }
        } else {
            // Hold post: stand guard most of the time, with the occasional short repositioning step,
            // so the garrison reads as *guarding* the fort — not endlessly circling it. (Long pauses
            // + a tight radius; once at the spot, `step_toward` reports not-moving → an idle stance.)
            sol.patrol_t -= dt;
            if sol.patrol_t <= 0.0 {
                let a = next_f(&mut sol.rng) * std::f32::consts::TAU;
                let rad = 4.0 + next_f(&mut sol.rng) * 4.0; // a post somewhere inside the walls
                sol.patrol = sol.home + Vec2::new(a.cos() * rad, a.sin() * rad);
                sol.patrol_t = 9.0 + next_f(&mut sol.rng) * 10.0; // then stand there a good while
            }
            let sp = v.speed * 0.5 * dt;
            step_toward(&mut v, sol.patrol, sp, cur_y, dt);
        }

        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
}

/// Worker brain: man the producer's yard — stand at a work spot and swing the trade's tool
/// (hoe/chop/pick) on a cooldown, with the occasional short repositioning step. They don't circle
/// the fort and they don't idly wander — they read as *working*. (The rival's economy is still
/// tax-funded; the labour is visual, but it's real on-screen work, not aimless milling.) The swing
/// reuses the `Villager.atk_anim` path `villager_drive` turns into an overhead strike. LOD-frozen
/// when the hero is far. Gated on `Modal::None` with the rest of the sim.
fn rival_workers(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    mut workers: Query<(&mut RivalWorker, &mut crate::villagers::Villager, &mut Transform), Without<crate::dying::Dying>>,
) {
    if hero_far(hero.pos) {
        return; // off-screen — freeze the labour (LOD)
    }
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    let tw = time.elapsed_secs_wrapped();
    for (mut w, mut v, mut tf) in &mut workers {
        w.patrol_t -= dt;
        let vpos = v.pos;
        let cur_y = crate::steer::footing(vpos.x, vpos.y).unwrap_or(tf.translation.y);
        let at_site = w.patrol.distance(w.site) < 0.05; // current destination IS the work site
        if vpos.distance(w.patrol) < 0.4 {
            // Arrived. At the site, work (swing the tool); at home, just deposit/idle. After a dwell,
            // walk back the other way — an out-to-gather, back-to-deposit loop past the open gate.
            v.moving = false;
            // Work at the site. INSIDE-bailey producers have no outside field (`site == home`, see
            // `spawn_building`'s reach=0), so they'd otherwise stand idle forever — let them work in
            // place at the building instead of only when site differs from home.
            if at_site {
                w.work_cd -= dt;
                if w.work_cd <= 0.0 {
                    w.work_cd = 1.2 + next_f(&mut w.rng) * 1.0;
                    v.atk_anim = now; // fire a hoe/chop/pick swing (read by villager_drive)
                }
            }
            if w.patrol_t <= 0.0 {
                // Toggle destination. Linger longer at the site (working) than at home (depositing).
                if at_site {
                    w.patrol = w.home;
                    w.patrol_t = 2.5 + next_f(&mut w.rng) * 2.0;
                } else {
                    w.patrol = w.site;
                    w.patrol_t = 6.0 + next_f(&mut w.rng) * 4.0;
                }
            }
        } else {
            let sp = v.speed * 0.5 * dt;
            step_toward(&mut v, w.patrol, sp, cur_y, dt);
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

// ── Daily raids (the rival marches on the player's castle) ───────────────────────────
//
// From day 2, each dawn the rival musters a SMALL, soft army (easy to wipe) that marches from the
// fort to the player's keep, trading blows with any guard/hero on the way and chipping the keep.
// They carry `RivalSoldier` (so the hero + town guards already target/kill them) PLUS `RivalRaider`
// (which swaps the leashed garrison brain for a march-on-the-keep one). At nightfall survivors
// retreat, so each raid lives one day. The chip is modest — pressure, not a death sentence (the
// keep-0 defeat stays night-gated in `siege.rs`).

const RAIDER_HP: f32 = 70.0;
/// A raid bowman's per-shaft hit — the raider's townsperson blade × the same 1.5 archer mark-up
/// as everywhere else (raiders are deliberately soft pressure, not snipers).
const RAIDER_ARROW_DMG: f32 = 13.0;
/// A raid bowman stops and volleys the keep from here (a bowshot, not at the wall's foot).
const RAIDER_KEEP_RANGE_BOW: f32 = 12.0;
/// Sticky-notice id for the "Rival raiders are attacking!" banner (persists while any raider lives).
const RAID_ALERT_KEY: u32 = 0x5a1d_a1e7;
const RAIDER_HERO_DMG: f32 = 8.0;
const RAIDER_NPC_DMG: f32 = 9.0;
/// Per-strike keep chip. Modest: a full party (~5) just out-paces the prep-time repair, so an
/// ignored raid softens you for the night without razing the keep by itself.
const RAIDER_KEEP_DMG: f32 = 3.5;
const RAIDER_ATK_CD: f32 = 1.2;
const RAIDER_MELEE: f32 = 1.8;
const RAIDER_SIGHT: f32 = 11.0;
const RAIDER_KEEP_RANGE: f32 = 5.0;
const RAIDER_SPEED: f32 = 2.6;
const RAIDER_TURN: f32 = 3.0;
/// Largest raid party (grows with the day, capped here). Kept small — these are a daily nuisance.
const RAID_MAX: usize = 5;

/// A rival raider: a `RivalSoldier`-bodied attacker with its own march-on-the-keep brain.
#[derive(Component)]
pub struct RivalRaider {
    atk_cd: f32,
}

/// Once per day from day 2 (the dawn edge Wave→Prep, `wave_index >= 0`), muster a raid party at the
/// gate and send it on the player's keep; at nightfall (edge into Wave) survivors retreat. No-op
/// while the fort is razed or on a map without the fort.
#[allow(clippy::too_many_arguments)]
fn rival_raid_director(
    time: Res<Time>,
    siege: Res<crate::siege::Siege>,
    state: Res<RivalState>,
    mats: Option<Res<RivalMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    mut speak: MessageWriter<crate::audio::Speak>,
    raiders: Query<Entity, With<RivalRaider>>,
    mut prev: Local<Option<crate::siege::GamePhase>>,
    mut seed: Local<u32>,
) {
    use crate::siege::GamePhase;
    let cur = siege.phase;
    let was = prev.replace(cur);
    if mats.is_none() {
        return; // no fort on this map
    }
    // Nightfall: surviving raiders retreat for the night.
    if cur == GamePhase::Wave && was != Some(GamePhase::Wave) {
        for e in &raiders {
            commands.entity(e).try_despawn();
        }
        return;
    }
    // Dawn. `Some(Wave)` skips the boot Prep; `wave_index >= 0` = at least one night cleared (day 2+).
    if cur == GamePhase::Prep && matches!(was, Some(GamePhase::Wave)) && !state.destroyed && siege.wave_index >= 0 {
        for e in &raiders {
            commands.entity(e).try_despawn(); // clear any stragglers before mustering a fresh party
        }
        let count = (2 + (siege.wave_index as usize) / 2).min(RAID_MAX);
        for i in 0..count {
            *seed = seed.wrapping_add(1);
            let s = 0x5a1d_0000u32.wrapping_add(seed.wrapping_mul(2654435761)) | 1;
            let spread = (i as f32 - (count as f32 - 1.0) * 0.5) * 1.6; // form up across the gate
            let pos = RIVAL_CENTRE + Vec2::new(spread, WALL_HALF + 3.0);
            spawn_raider(&mut commands, &mut meshes, &mut creature_mats, pos, s);
        }
        notice.push("Rival raiders march on the keep!", time.elapsed_secs_f64());
        // A raider barks the march cue from the gate as the party sets out.
        speak.write(crate::audio::Speak::at(
            crate::audio::Concept::RivalRaidMarch,
            Vec3::new(RIVAL_CENTRE.x, 1.6, RIVAL_CENTRE.y + WALL_HALF + 3.0),
        ));
    }
}

/// Spawn one raider at `pos`, marching on the player keep. Carries `RivalSoldier` (so the hero + town
/// guards already target/kill it) + `RivalRaider` (the march-on-the-keep brain). Shared by the daily
/// director and the screenshot stager.
fn spawn_raider(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    pos: Vec2,
    seed: u32,
) {
    // Raid parties march with the same one-in-three bowman split as the garrison (and as the
    // player's own militia) — `is_bowman` on the spawn seed.
    let e = if crate::villagers::is_bowman(seed) {
        let e = crate::villagers::spawn_rival_archer(commands, meshes, creature_mats, crate::siege::KEEP_POS, pos, seed);
        commands.entity(e).insert(RivalBow { loosed: true });
        e
    } else {
        crate::villagers::spawn_rival_soldier(commands, meshes, creature_mats, crate::siege::KEEP_POS, pos, seed)
    };
    commands.entity(e).insert((
        RivalSoldier { home: crate::siege::KEEP_POS, atk_cd: 0.0, patrol: pos, patrol_t: 0.0, rng: seed },
        RivalRaider { atk_cd: 0.0 },
        crate::player::Health { hp: RAIDER_HP, max: RAIDER_HP },
    ));
}

/// Screenshot/test staging (`FOREST_RAID=1`): muster a raid party right outside the castle at boot so
/// a shot/clip frames the attack + the town's defence immediately (the real raids only come at dawn
/// of day 2+). No-op otherwise.
fn stage_raid_for_shot(
    app: Res<State<crate::game_state::AppState>>,
    mats: Option<Res<RivalMats>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    mut done: Local<bool>,
) {
    if *done || *app.get() != crate::game_state::AppState::Playing {
        return;
    }
    if std::env::var("FOREST_RAID").is_err() {
        *done = true;
        return;
    }
    if mats.is_none() {
        return; // wait for the fort/map to exist
    }
    *done = true;
    for i in 0..4 {
        let s = 0x5a1d_7000u32.wrapping_add((i as u32).wrapping_mul(2654435761)) | 1;
        let pos = crate::siege::KEEP_POS + Vec2::new((i as f32 - 1.5) * 1.6, 16.0);
        spawn_raider(&mut commands, &mut meshes, &mut creature_mats, pos, s);
    }
}

/// Drive each raider toward the player's keep: engage the nearest guard/townsperson/hero in sight,
/// else press to the keep and batter it (modest chip). A raid **bowman** ([`RivalBow`]) fights the
/// same brain at range: he spots foes from [`crate::villagers::BOW_RANGE`], plants and volleys them,
/// and bombards the keep from a bowshot out instead of hammering its wall. Gated on `Modal::None`
/// with the rest of the sim. A raid is a handful of bodies alive for one day, heading to where the
/// player lives — no LOD.
#[allow(clippy::type_complexity)]
fn rival_raid_brain(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    mut pending: ResMut<crate::player::PendingHeroDamage>,
    mut npc_dmg: ResMut<crate::villagers::NpcDamage>,
    mut keep: ResMut<crate::siege::KeepHp>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    mut arrows: ResMut<crate::projectile::ArrowSpawns>,
    mut cues: MessageWriter<crate::audio::AudioCue>,
    hero_ent: Query<Entity, With<crate::player::Hero>>,
    mut raiders: Query<(Entity, &mut RivalRaider, Option<&mut RivalBow>, &mut crate::villagers::Villager, &mut Transform), Without<crate::dying::Dying>>,
    townsfolk: Query<(Entity, &Transform), (With<crate::villagers::Townsfolk>, Without<crate::dying::Dying>, Without<RivalSoldier>, Without<RivalRaider>)>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    // Set when any raider lands a blow this frame → throttled on-screen "under attack" cue, so a
    // daytime raid the player isn't watching still announces itself (the siege keep-alert is night-only).
    let mut struck = false;
    let tw = time.elapsed_secs_wrapped();
    let goal = crate::siege::KEEP_POS;
    let hero_e = hero_ent.iter().next();
    // (entity, flat pos, chest height) — the height feeds a bowman's aim point.
    let folk: Vec<(Entity, Vec2, f32)> =
        townsfolk.iter().map(|(e, t)| (e, Vec2::new(t.translation.x, t.translation.z), t.translation.y + 1.0)).collect();
    for (re, mut rd, mut bow, mut v, mut tf) in &mut raiders {
        rd.atk_cd -= dt;
        let vpos = v.pos;
        let is_archer = bow.is_some();
        let sight = if is_archer { crate::villagers::BOW_RANGE } else { RAIDER_SIGHT };
        // Nearest of our people (hero or townsperson) in sight. (aim_y = the mark's chest height.)
        let mut best: Option<(f32, Vec2, f32, Option<Entity>)> = None;
        if hero.alive {
            let d = vpos.distance(hero.pos);
            if d < sight {
                best = Some((d, hero.pos, hero.y + 1.0, None));
            }
        }
        for (fe, fp, fy) in &folk {
            let d = vpos.distance(*fp);
            if d < sight && best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, *fp, *fy, Some(*fe)));
            }
        }
        let cur_y = crate::steer::footing(vpos.x, vpos.y).unwrap_or(tf.translation.y);
        let turn = RAIDER_TURN * 2.0 * dt;
        // The keep-assault distance: a swordsman batters the wall, a bowman volleys from a bowshot.
        let keep_range = if is_archer { RAIDER_KEEP_RANGE_BOW } else { RAIDER_KEEP_RANGE };
        if let Some((d, tpos, aim_y, victim)) = best {
            // A raid bowman plants and volleys instead of closing — mirror of the garrison archer.
            if let Some(bw) = bow.as_deref_mut().filter(|_| d < crate::villagers::BOW_RANGE || mid_shot(&v, now)) {
                v.moving = false;
                let to = tpos - vpos;
                if to.length_squared() > 1e-4 {
                    let want = to.x.atan2(to.y);
                    v.facing += crate::steer::wrap_pi(want - v.facing).clamp(-turn, turn);
                }
                if let Some(from) = bow_cycle(now, &mut v, &mut rd.atk_cd, bw, to, tf.translation.y) {
                    struck = true;
                    if let Some(te) = victim.or(hero_e) {
                        arrows.0.push(crate::projectile::ArrowSpawn {
                            from,
                            aim: Vec3::new(tpos.x, aim_y, tpos.y),
                            target: te,
                            shooter: re,
                            damage: RAIDER_ARROW_DMG,
                            rival: true,
                        });
                        if vpos.distance(hero.pos) < crate::villagers::BOW_SFX_RADIUS {
                            cues.write(crate::audio::AudioCue::BowShot(from));
                        }
                    }
                }
            } else if d <= RAIDER_MELEE {
                v.moving = false;
                let to = tpos - vpos;
                if to.length_squared() > 1e-4 {
                    let want = to.x.atan2(to.y);
                    v.facing += crate::steer::wrap_pi(want - v.facing).clamp(-turn, turn);
                }
                if rd.atk_cd <= 0.0 {
                    rd.atk_cd = RAIDER_ATK_CD;
                    v.atk_anim = now;
                    struck = true;
                    match victim {
                        None => {
                            pending.0 += RAIDER_HERO_DMG;
                            pending.1 = to.normalize_or_zero(); // directional hit-shake
                        }
                        // `attacker: Some(re)` so the struck townsperson fights back against the raider
                        // like it does every other foe (raiders carry `RivalSoldier`, now in the
                        // `npc_fight_back` hostile set) — mirrors `rival_combat`'s garrison strike.
                        Some(ve) => npc_dmg.0.push(crate::villagers::NpcHit { victim: ve, amount: RAIDER_NPC_DMG, attacker: Some(re) }),
                    }
                }
            } else {
                step_toward(&mut v, tpos, RAIDER_SPEED * dt, cur_y, dt);
            }
        } else if vpos.distance(goal) <= keep_range {
            v.moving = false;
            let to = goal - vpos;
            if to.length_squared() > 1e-4 {
                let want = to.x.atan2(to.y);
                v.facing += crate::steer::wrap_pi(want - v.facing).clamp(-turn, turn);
            }
            if let Some(bw) = bow.as_deref_mut() {
                // Volley the keep: the chip lands on the release frame (as the melee chip lands on
                // the strike frame), and the shaft itself flies for real, arcing into the
                // battlements — usually planting in masonry, but a defender crossing the arc
                // catches an honest raid-arrow hit.
                if let Some(from) = bow_cycle(now, &mut v, &mut rd.atk_cd, bw, to, tf.translation.y) {
                    struck = true;
                    keep.hp = (keep.hp - RAIDER_KEEP_DMG).max(0.0);
                    let keep_y = crate::worldmap::ground_at_world(goal.x, goal.y).unwrap_or(tf.translation.y);
                    arrows.0.push(crate::projectile::ArrowSpawn {
                        from,
                        aim: Vec3::new(goal.x, keep_y + 4.0, goal.y),
                        target: re, // no living mark (masonry) — never matched by the rival hit test
                        shooter: re,
                        damage: RAIDER_ARROW_DMG,
                        rival: true,
                    });
                    if vpos.distance(hero.pos) < crate::villagers::BOW_SFX_RADIUS {
                        cues.write(crate::audio::AudioCue::BowShot(from));
                    }
                }
            } else if rd.atk_cd <= 0.0 {
                rd.atk_cd = RAIDER_ATK_CD;
                v.atk_anim = now;
                struck = true;
                keep.hp = (keep.hp - RAIDER_KEEP_DMG).max(0.0);
            }
        } else {
            step_toward(&mut v, goal, RAIDER_SPEED * dt, cur_y, dt);
        }
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(tf.translation.y);
        let bob = if v.moving { (tw * v.gait + v.phase).sin().abs() * v.bob } else { 0.0 };
        tf.translation = Vec3::new(v.pos.x, gy + bob, v.pos.y);
        tf.rotation = Quat::from_rotation_y(v.facing);
    }
    // Keep a persistent "under attack" banner up for the WHOLE life of the raid — it must not lapse
    // while any raider is still alive (the night-only siege keep-alert doesn't cover daytime raids).
    // Raise it once the raid lands its first blow, and drop it the moment the last raider falls
    // (the query already excludes `Dying`, so a downed raider stops counting immediately).
    let alive = raiders.iter().count();
    if alive == 0 {
        notice.clear_sticky(RAID_ALERT_KEY);
    } else if struck {
        notice.set_sticky(RAID_ALERT_KEY, "Rival raiders are attacking!");
    }
}

// ── Razing the rival fort (the player destroys the stronghold) ───────────────────────

/// Reach the hero's swing must land within of the keep centre to chip it (keep half-girth ≈3.1 +
/// melee reach). And the swing-cone dot (≈70°, matching the combat cone).
const FORT_HIT_REACH: f32 = 5.0;
const FORT_CONE_DOT: f32 = 0.35;

/// The player razes the fort: every hero swing whose cone falls on the keep chips its HP (using the
/// shared [`crate::verbs::HeroSwing`] cone that ore/dummies read). At 0 the whole fort topples (keep,
/// walls, buildings, garrison + workers + raiders), its collision is cleared, the daily raids stop,
/// a bounty drops and a notice fires. A SECONDARY objective — the campaign win is still the Warlord.
#[allow(clippy::too_many_arguments)]
fn rival_fort_damage(
    time: Res<Time>,
    mut swings: MessageReader<crate::verbs::HeroSwing>,
    mut state: ResMut<RivalState>,
    mut commands: Commands,
    mut notice: ResMut<crate::ui::notice::Notice>,
    mut rewards: ResMut<crate::orbs::RewardBursts>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut speak: MessageWriter<crate::audio::Speak>,
    keep: Query<(), With<RivalKeep>>,
    parts: Query<Entity, Or<(With<RivalEntity>, With<RivalSoldier>, With<RivalWorker>)>>,
) {
    if state.destroyed || keep.is_empty() {
        swings.clear(); // no live fort → drain so the reader doesn't backlog
        return;
    }
    let now = time.elapsed_secs();
    let mut chipped = false;
    for sw in swings.read() {
        let to = RIVAL_CENTRE - sw.origin;
        let d = to.length();
        if d > FORT_HIT_REACH {
            continue;
        }
        if d > 1e-3 && (to / d).dot(sw.fwd) < FORT_CONE_DOT {
            continue;
        }
        state.keep_hp = (state.keep_hp - sw.base_dmg).max(0.0);
        chipped = true;
    }
    if !chipped {
        return;
    }
    // Chip number over the keep.
    let gy = ground_at_world(RIVAL_CENTRE.x, RIVAL_CENTRE.y).unwrap_or(0.0);
    floats.0.push(crate::combat_fx::FloatReq {
        world: Vec3::new(RIVAL_CENTRE.x, gy + 5.5, RIVAL_CENTRE.y),
        text: format!("{}", state.keep_hp.ceil() as i32),
        color: crate::combat_fx::col_ork_hit(),
        scale: 1.1,
    });
    if state.keep_hp <= 0.0 {
        state.destroyed = true;
        // Topple everything rival (keep + walls + buildings + workers + garrison + raiders).
        for e in &parts {
            crate::dying::begin_dying(&mut commands, e, now);
        }
        // Drop ALL its collision — buildings, curtain wall, and the otherwise-permanent keep box —
        // so the razed ground is walkable, not a field of invisible boxes.
        clear_building_blockers();
        clear_wall_blockers(RIVAL_CENTRE);
        crate::blockers::remove_box_near(RIVAL_CENTRE.x, RIVAL_CENTRE.y, 0.3);
        rewards.0.push(crate::orbs::RewardBurst {
            at: Vec3::new(RIVAL_CENTRE.x, gy + 1.0, RIVAL_CENTRE.y),
            gold: 400,
            xp: 300,
        });
        notice.push("The rival stronghold has fallen — the raids cease.", time.elapsed_secs_f64());
        // Hero's relief (head-locked) + the garrison's dying lament (spatial, over the ruin).
        let cry = Vec3::new(RIVAL_CENTRE.x, gy + 1.0, RIVAL_CENTRE.y);
        speak.write(crate::audio::Speak::new(crate::audio::Concept::RivalFell));
        speak.write(crate::audio::Speak::at(crate::audio::Concept::RivalLament, cry));
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
                (
                    rival_economy,
                    rival_garrison,
                    rival_combat,
                    rival_workers,
                    rival_raid_director,
                    rival_raid_brain,
                    rival_fort_damage,
                )
                    .run_if(in_state(crate::game_state::Modal::None)),
            )
            // Reconcile the rival's economy + buildings to a loaded save (ungated; fires on a load).
            .add_systems(Update, restore_rival)
            // Screenshot staging (ungated; env-gated inside).
            .add_systems(Update, (stage_rival_for_shot, stage_raid_for_shot));
    }
}
