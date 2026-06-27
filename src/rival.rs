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

/// World-XZ centre of the rival keep. The NE **desert** region sits at world ≈ (52, −79) — already
/// ~95 units from the player castle at the origin — so the two develop independently and only clash
/// when someone crosses the open ground between them. Nudged a touch toward the island interior so
/// the fort stands on solid desert rather than the north beach.
pub const RIVAL_CENTRE: Vec2 = Vec2::new(54.0, -72.0);

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
    if crate::worldmap::MapId::from_u8(crate::worldmap::current_map_u8()) != crate::worldmap::MapId::Home {
        return false;
    }
    (wx - RIVAL_CENTRE.x).abs() <= WALL_HALF + 1.5 && (wz - RIVAL_CENTRE.y).abs() <= WALL_HALF + 1.5
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
    // A few starter dwellings inside the bailey (around the keep).
    for (hx, hz) in [(-7.5, -6.0), (7.5, -6.0), (-7.5, 6.5)] {
        rest.extend(desert_house(hx, hz));
    }
    (keep, rest)
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

    // Walls / towers / dwellings root.
    let fort_root = commands
        .spawn((Transform::from_xyz(centre.x, y, centre.y), Visibility::Visible, BiomeEntity, RivalEntity))
        .id();
    commands.entity(fort_root).with_children(|p| {
        for (mesh, slot) in rest {
            p.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats.get(slot)), Transform::default()));
        }
    });
}

// ── Plugin (no systems yet — economy/garrison/save land in later steps) ──────────────

pub struct RivalPlugin;

impl Plugin for RivalPlugin {
    fn build(&self, _app: &mut App) {
        // Step 1 is pure world geometry (built from `worldmap::build_step`). Systems for the
        // autonomous economy, the skirmishing garrison and save/load are added here in later steps.
    }
}
