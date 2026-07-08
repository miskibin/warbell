//! **Ork camps** — the ambient diorama layer ported from the TS `OrkCamp.tsx` / `Tent.tsx` /
//! `Campfire.tsx` / `CampCage.tsx`. One camp guards each wilderness biome (snow, desert,
//! forest, swamp, rock). Each is a clearing of tents, a flickering campfire, a warband banner,
//! skull-spikes and a prisoner cage with captives, patrolled by a mixed warband (`orks.rs`).
//!
//! This module builds the **camp dioramas + placement**; the warbands that occupy them fight,
//! flee, brawl rival factions and respawn (`orks.rs`), and a cleared warband frees the prisoner
//! cage (`villagers::camp_rescue`, tagged here by [`Cage`]). Camps render only in the Combined
//! world-map view (built from `worldmap::build`, like the castle); single-biome views (keys 1–5)
//! have no island layout, so no camps.
//!
//! Placement ([`plan`]) deterministically reject-samples one flat 7×7 walkable clearing per
//! biome (mountainous snow/rock land on the flat grass apron at the biome edge — what the TS
//! camps did). The plan is cached in a `OnceLock`; [`in_clearing`] lets the scatter + wildlife
//! keep their props/herds out of the camps. Solid props register in [`crate::blockers`] so the
//! orks + wildlife route around them.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::OnceLock;

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::{Biome, BiomeEntity};
use crate::orks::{self, Faction, VARIANTS};
use crate::palette::lin;
use crate::worldmap::{self, GX, GZ};
use crate::meshkit::tinted;

// ── Plugin (campfire flicker + smoke) ────────────────────────────────────────────────

pub struct CampsPlugin;

impl Plugin for CampsPlugin {
    fn build(&self, app: &mut App) {
        // swing_cage_doors + reconcile_cages_on_load are UNGATED: the door swing is cosmetic
        // (a mid-swing door finishing under a pause is harmless), and the load reconcile must
        // fire on the GameLoaded message whenever it lands.
        app.add_systems(Update, (flicker_flames, drift_smoke, swing_cage_doors, reconcile_cages_on_load));
        app.add_systems(
            Update,
            respawn_warbands.run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

/// Ease every cage door toward its `open` target — the rescue "animation": a quick fling that
/// settles (cubic ease-out). The door is a child of the cage frame, so this writes rotation
/// only; the frame (and its blocker) never change.
fn swing_cage_doors(time: Res<Time>, mut q: Query<(&mut Transform, &mut CageDoor)>) {
    let dt = time.delta_secs();
    for (mut tf, mut d) in &mut q {
        let step = dt / CAGE_DOOR_SWING;
        d.t = (d.t + if d.open { step } else { -step }).clamp(0.0, 1.0);
        let s = 1.0 - (1.0 - d.t).powi(3);
        tf.rotation = Quat::from_rotation_y(CAGE_DOOR_OPEN_YAW * s);
    }
}

/// Post-load reconcile: a restored run's cages must match its `rescued_camps` /
/// `blight_captives_freed` flags, whatever state the live world is in (a fresh world rebuild
/// spawns every cage closed + stocked; an in-process Continue may have them open from the dead
/// run). Snap each door to its saved state and despawn the captives of already-freed cages.
fn reconcile_cages_on_load(
    mut ev: MessageReader<crate::savegame::GameLoaded>,
    mut doors: Query<&mut CageDoor>,
    captives: Query<(Entity, &crate::villagers::Captive)>,
    mut commands: Commands,
) {
    let Some(crate::savegame::GameLoaded(data)) = ev.read().last() else { return };
    let freed = |key: CageKey| match key {
        CageKey::Camp(i) => data.rescued_camps.get(i).copied().unwrap_or(false),
        CageKey::Blight(i) => data.blight_captives_freed.get(i).copied().unwrap_or(false),
        CageKey::Decor => false,
    };
    for mut d in &mut doors {
        d.open = freed(d.key);
        d.t = if d.open { 1.0 } else { 0.0 }; // snap — no ghost swing on load
    }
    for (e, c) in &captives {
        if freed(c.key) {
            commands.entity(e).try_despawn(); // rescued long ago — walked home already
        }
    }
}

/// Spawn one prisoner cage: the natural-timber frame (`ork_fortress::cage_mesh`), the separate
/// hinged door child, and `captives` REAL seated peasants inside
/// (`villagers::spawn_cage_captive`, one per `ork_fortress::CAGE_SEATS` straw bed). A rescue
/// opens the door by ANIMATION and walks the peasants out — nothing about the cage is swapped
/// or despawned. Callers register their own collision blocker (footprints differ per site).
#[allow(clippy::too_many_arguments)]
pub fn spawn_cage(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    prop_mat: &Handle<StandardMaterial>,
    tf: Transform,
    key: CageKey,
    captives: usize,
    seed: u32,
    open: bool,
) -> Entity {
    use crate::ork_fortress::{cage_door_mesh, cage_mesh, CAGE_SEATS, CAGE_W};
    let mut rng = seed | 1;
    let root = commands
        .spawn((
            Mesh3d(meshes.add(cage_mesh(&mut rng))),
            MeshMaterial3d(prop_mat.clone()),
            tf,
            Cage { key },
            BiomeEntity,
        ))
        .id();
    // The door hangs just proud of the (+X, +Z) corner post. No BiomeEntity of its own — it
    // despawns with the frame (child cascade), and a second tag would double-despawn.
    let door = commands
        .spawn((
            Mesh3d(meshes.add(cage_door_mesh(&mut rng))),
            MeshMaterial3d(prop_mat.clone()),
            Transform::from_translation(v(CAGE_W / 2.0 + 0.07, 0.0, CAGE_W / 2.0)),
            // `open` pre-opens the door (the `FOREST_CAGETEST` after-state cage) — it eases
            // from shut on the first frames, which is invisible during boot.
            CageDoor { key, open, t: 0.0 },
        ))
        .id();
    commands.entity(root).add_child(door);
    // The captives: real peasants seated on the straw beds (roots of their own — they stand up
    // and WALK OUT on rescue, so they can't be children of the frame).
    for (sx, sz, syaw) in CAGE_SEATS.iter().take(captives) {
        let seat = tf.transform_point(v(*sx, 0.148, *sz)); // on the plank floor
        let facing = tf.rotation * ry(*syaw);
        crate::villagers::spawn_cage_captive(commands, meshes, creature_mats, seat, facing, key, next_u32(&mut rng));
    }
    root
}

/// Campfire flame marker — also the anchor the audio module hangs a spatial campfire loop
/// (+ war-drum sink) on. `pub(crate)` phase: the ork fortress (`ork_fortress.rs`) tags its
/// bonfire with this same component precisely to inherit that audio lifecycle.
#[derive(Component)]
pub struct Flicker {
    pub(crate) phase: f32,
}
#[derive(Component)]
struct CampSmoke {
    base: Vec3,
    phase: f32,
    speed: f32,
}

/// Which prisoner cage an entity belongs to — shared by the cage frame ([`Cage`]), its hinged
/// door ([`CageDoor`]) and the seated peasants inside (`villagers::Captive`), so a rescue can
/// address all three parts of one cage.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CageKey {
    /// A wilderness camp's cage — the enumerate index of [`plan`] (the same index
    /// [`cage_positions`] and `villagers::camp_rescue` use).
    Camp(usize),
    /// A Blight cage outside Gnashfang Hold — the `ork_fortress::BLIGHT_CAGES` slot.
    Blight(usize),
    /// Decorative only (the Hold's own hoard-cage, the `FOREST_CAGETEST` stage) — never freed.
    Decor,
}

/// Tags the prisoner-cage FRAME. The frame persists through a rescue — opening is the hinged
/// [`CageDoor`] child swinging (an animation), never a model swap.
#[derive(Component)]
pub struct Cage {
    pub key: CageKey,
}

/// The cage's hinged door (a child of the frame, hung at the (+X, +Z) corner post). A rescue
/// sets `open`; [`swing_cage_doors`] eases the yaw so the door visibly swings out.
#[derive(Component)]
pub struct CageDoor {
    pub key: CageKey,
    /// Target state — set true by the rescue systems (or snapped by the save-load reconcile).
    pub open: bool,
    /// Swing progress 0 (shut) → 1 (fully open). Snap to 1.0 to open instantly (load reconcile).
    pub t: f32,
}

/// How far the door swings (rad about the hinge post; negative = outward past perpendicular).
const CAGE_DOOR_OPEN_YAW: f32 = -2.05;
/// Full swing duration (s).
const CAGE_DOOR_SWING: f32 = 1.4;

/// Seconds after a camp's warband is wiped before it repopulates. 3× the old 60s (TS
/// `OrkCamp.tsx` used 60) so clearing a camp buys real breathing room instead of an endless
/// treadmill of fresh orks.
const CAMP_RESPAWN_DELAY: f32 = 180.0;
/// The hero must be at least this far (world units) from a camp for it to repopulate, so a cleared
/// warband returns OUT OF SIGHT rather than in front of the player (TS `CAMP_RESPAWN_FAR` = 40).
const CAMP_RESPAWN_FAR: f32 = 40.0;

/// The prebuilt ork [`orks::Armory`] kept alive past [`build`] so a wiped warband can respawn, plus
/// each camp's clear timestamp (`Some(t)` once it has zero living orks, `None` while populated).
/// Captives are deliberately NOT tracked here — a freed cage stays open and empty, matching the
/// original: a camp's prisoners are a one-time rescue, the warband a renewable threat.
#[derive(Resource)]
pub(crate) struct CampWarbands {
    /// The prebuilt ork mesh/material armory — also reused by `landmarks.rs` to summon Rune-Trial
    /// guardians (camp-style orks home-anchored at a landmark).
    pub(crate) armory: orks::Armory,
    cleared_at: Vec<Option<f32>>,
}

fn flicker_flames(time: Res<Time>, mut q: Query<(&Flicker, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (f, mut tf) in &mut q {
        let sx = 1.0 + (t * 7.0 + f.phase).sin() * 0.12 + (t * 14.3 + f.phase).sin() * 0.06;
        let sy = 1.0 + (t * 9.5 + f.phase).sin() * 0.22;
        tf.scale = Vec3::new(sx, sy, sx);
    }
}

fn drift_smoke(time: Res<Time>, mut q: Query<(&CampSmoke, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (s, mut tf) in &mut q {
        let cycle = (t * s.speed + s.phase).rem_euclid(1.0);
        tf.translation.x = s.base.x + (t * 0.7 + s.phase * 6.0).sin() * 0.18 * cycle;
        tf.translation.z = s.base.z + (t * 0.6 + s.phase * 6.0).cos() * 0.18 * cycle;
        tf.translation.y = s.base.y + cycle * 1.6;
        let sc = (0.1 + cycle * 0.4) * (1.0 - cycle).max(0.0);
        tf.scale = Vec3::splat(sc.max(0.001));
    }
}

// ── Placement ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct CampSite {
    centre: Vec2,
    faction: Faction,
    rot: f32,
    seed: u32,
}

static SITES: OnceLock<Vec<CampSite>> = OnceLock::new();

/// Half-extent (world units) of a camp's reserved clearing — scatter + wildlife stay out.
const CLEAR_HALF: f32 = 3.6;

/// Plan one camp per wilderness biome (cached). Pure tile-map queries, so it's deterministic
/// and stable across biome-switch rebuilds. Call before scatter so clearings get reserved.
pub fn plan() -> &'static [CampSite] {
    SITES
        .get_or_init(|| {
            let targets = [Biome::Snow, Biome::Desert, Biome::Forest, Biome::Swamp, Biome::Rocky];
            let mut rng = 0xca37_5eedu32;
            let mut placed: Vec<CampSite> = Vec::new();
            for (i, b) in targets.iter().enumerate() {
                let faction = if i % 2 == 0 { Faction::Red } else { Faction::Blue };
                let mut found = None;
                for _ in 0..8000 {
                    let cx = rng_range(&mut rng, -GX + 8.0, GX - 8.0);
                    let cz = rng_range(&mut rng, -GZ + 8.0, GZ - 8.0);
                    if site_ok(cx, cz, *b, &placed) {
                        found = Some((cx, cz));
                        break;
                    }
                }
                if let Some((cx, cz)) = found {
                    let rot = rng_range(&mut rng, 0.0, TAU);
                    let seed = next_u32(&mut rng);
                    placed.push(CampSite { centre: Vec2::new(cx, cz), faction, rot, seed });
                } else {
                    info!("camps: no flat clearing found for {:?}", b);
                }
            }
            placed
        })
        .as_slice()
}

/// Each camp's `(prisoner-cage world XZ, camp-centre world XZ)` — the rescue interaction walks
/// up to the cage, and the centre is used to test whether the warband guarding it is cleared.
/// The cage sits at camp-local `(-2.2, 2.2)` (see `build`), rotated by the site yaw.
pub fn cage_positions() -> Vec<(Vec2, Vec2)> {
    plan()
        .iter()
        .map(|s| {
            let w = Quat::from_rotation_y(s.rot) * Vec3::new(-2.2, 0.0, 2.2);
            (s.centre + Vec2::new(w.x, w.z), s.centre)
        })
        .collect()
}

/// True if `(wx, wz)` is inside any planned camp's clearing (axis-aligned box around centre).
pub fn in_clearing(wx: f32, wz: f32) -> bool {
    match SITES.get() {
        Some(sites) => sites
            .iter()
            .any(|c| (wx - c.centre.x).abs() <= CLEAR_HALF && (wz - c.centre.y).abs() <= CLEAR_HALF),
        None => false,
    }
}

/// A candidate `(cx, cz)` is a valid camp centre for `biome` if a flat 7×7 clearing of
/// base-height land sits there, clear of the castle + central safe-zone + other camps, and it's
/// in the biome (or on grass within ~8 tiles of it, for the mountainous biomes).
fn site_ok(cx: f32, cz: f32, biome: Biome, placed: &[CampSite]) -> bool {
    let Some(h0) = worldmap::ground_at_world(cx, cz) else { return false };
    if h0 > 0.01 {
        return false; // flat base ground only (avoids plateaus + mountain terraces)
    }
    for dz in -3..=3 {
        for dx in -3..=3 {
            match worldmap::ground_at_world(cx + dx as f32, cz + dz as f32) {
                Some(h) if (h - h0).abs() < 1e-3 => {}
                _ => return false,
            }
        }
    }
    // Keep camps off the castle's doorstep — 24→40 so a biome camp can't reject-sample onto
    // the grass apron right against the keep wall (the biome blobs sit 70+ units out, so this
    // only rejects the too-close apron, never a real biome clearing).
    if crate::castle::in_footprint(cx, cz)
        || Vec2::new(cx, cz).length() < 40.0
        || crate::rival::near_fort(cx, cz)
    {
        return false;
    }
    // The Blight is Gnashfang Hold's own ground (it reads as Swamp to `biome_at_world`) —
    // without this the swamp camp can reject-sample into the fortress's territory.
    if crate::ork_fortress::in_blight_world(cx, cz) {
        return false;
    }
    let on_biome = worldmap::biome_at_world(cx, cz) == Some(biome);
    if !on_biome && !(worldmap::is_grass_world(cx, cz) && biome_within(cx, cz, biome, 8)) {
        return false;
    }
    !placed.iter().any(|c| c.centre.distance(Vec2::new(cx, cz)) < 18.0)
}

/// True if `biome` occurs on any tile within `r` tiles of `(cx, cz)`.
fn biome_within(cx: f32, cz: f32, biome: Biome, r: i32) -> bool {
    for dz in -r..=r {
        for dx in -r..=r {
            if worldmap::biome_at_world(cx + dx as f32, cz + dz as f32) == Some(biome) {
                return true;
            }
        }
    }
    false
}

// ── Build ────────────────────────────────────────────────────────────────────────

/// Warband member offsets around the fire (camp-local), one per `orks::VARIANTS` entry. Spread to
/// four corners of the clearing — clear of the fire, the cage and the big tent (−2.4,0) — so the
/// warband spawns (and idle-orbits, via each ork's `anchor`) fanned out instead of in one knot.
const WARBAND: [(f32, f32); 4] = [(1.6, 1.8), (-1.0, -2.0), (2.2, 0.6), (-0.2, 2.6)];

/// Build every planned camp: props (registering blockers) + the warband. Tagged `BiomeEntity`
/// so the biome switch despawns/rebuilds them. Called from `worldmap::build` after the castle.
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
    let sites = plan();
    if sites.is_empty() {
        return;
    }

    // The orks draw against the shared creature material (per-surface texture from the surf code).
    let mat = crate::creature::make_creature_material(creature_mats);
    // The structural props (tents, cage, banner, fire ring) carry a FAINT grime-grain
    // detail texture multiplied over their vertex colours — same trick as Gnashfang Hold, but
    // kept FAINT (`strength` 0.16) so the camps don't read over-textured against the other
    // biomes' clean flat-shaded props — a tooth on the hide/timber, not a patchwork. One
    // extra batch.
    let grime = crate::biome::GroundDetail {
        scale: 1.0,
        strength: 0.16,
        variation: 0.5,
        seed: 13.0,
        dark: 0x9c968c,
        base: 0xc2bcb0,
        light: 0xe6e0d4,
        grain: 0.6,
        streak: 0.5,
    };
    let (grime_img, _) = crate::terrain::detail_image(&grime);
    let grime_tex = images.add(grime_img);
    let prop_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(grime_tex),
        perceptual_roughness: 0.95,
        ..default()
    });
    // Emissive flame (bloom-lit, like the castle torches) + translucent smoke.
    let flame_mat = materials.add(StandardMaterial {
        base_color: crate::palette::srgb(0xff8a30),
        emissive: crate::palette::srgb(0xff8a30).to_linear() * 4.0,
        ..default()
    });
    let smoke_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.55, 0.55, 0.57, 0.4),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let smoke_puff = meshes.add(Sphere::new(0.5).mesh().ico(1).unwrap());

    let armory = orks::Armory::new(meshes, materials, mat.clone());

    // Screenshot hook: `FOREST_ORKLINE="x,z"` parks one ork of each variant in a line at the
    // given world XZ, each home-anchored on its own spot so it idles in place for the shot —
    // pair with `FOREST_CAM` to frame the warband close up.
    if let Ok(s) = std::env::var("FOREST_ORKLINE") {
        let p: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        if p.len() == 2 {
            for (i, variant) in VARIANTS.iter().enumerate() {
                let pos = Vec2::new(p[0] + i as f32 * 1.8 - 2.7, p[1]);
                armory.spawn(commands, *variant, Faction::Red, pos, pos, 11 + i as u32);
            }
            // + one seated ork at the end of the line (verifies the sit pose on a stump).
            let (sx, sz) = (p[0] + 4.0 * 1.8 - 2.7, p[1]);
            let sy = worldmap::ground_at_world(sx, sz).unwrap_or(0.0);
            armory.spawn_seated(commands, crate::orks::OrkVariant::Grunt, Faction::Red, Vec3::new(sx, sy, sz), 0.0);
        }
    }

    // Screenshot hook: `FOREST_CAGETEST="x,z"` parks the rescue's before/after states side by
    // side at the given world XZ: a CLOSED cage full of seated peasant captives, and 5.5u
    // further +X an OPEN, emptied one — so one still verifies the captive models, the natural
    // frame and the swung door. (The swing ANIMATION itself films best via the real rescue:
    // `FOREST_DEMO=rescue` + `FOREST_CLIP`.) Both doors face +X; frame from the east.
    if let Ok(s) = std::env::var("FOREST_CAGETEST") {
        let p: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
        if p.len() == 2 {
            for (dx, open) in [(0.0, false), (5.5, true)] {
                let (x, z) = (p[0] + dx, p[1]);
                let y = worldmap::ground_at_world(x, z).unwrap_or(0.0);
                spawn_cage(
                    commands,
                    meshes,
                    creature_mats,
                    &prop_mat,
                    Transform::from_xyz(x, y, z),
                    CageKey::Decor,
                    if open { 0 } else { crate::ork_fortress::CAGE_SEATS.len() },
                    0xca6e_7e57 + dx as u32,
                    open,
                );
            }
        }
    }

    for (camp, site) in sites.iter().enumerate() {
        // Logged so staging tools (screenshot framing, debugging) can find each camp.
        info!("camp {camp} at {:.1},{:.1}", site.centre.x, site.centre.y);
        let rot_q = Quat::from_rotation_y(site.rot);
        let cy = worldmap::ground_at_world(site.centre.x, site.centre.y).unwrap_or(0.0);
        let centre3 = Vec3::new(site.centre.x, cy, site.centre.y);
        let place = |local: Vec3| centre3 + rot_q * local;

        // Static props: (mesh, camp-local pos, local yaw, footprint half-extents (hw,hd) in the
        // prop's own frame; (0,0) = no collision). The tent is the FORTRESS model
        // (`ork_fortress::tent_mesh`) so every warband pitches the same gear as Gnashfang Hold;
        // it's SOLID — it registers a blocker box, so the hero and the warband route around it.
        // Tent NW (−z), cage SW (+z), well apart so the bigger fortress models don't overlap
        // (they did at the old −2.4/+2.2 spots). Tent at 0.7× the fortress size to fit the camp.
        let mut prop_rng = site.seed | 1;
        let solids = vec![
            (crate::ork_fortress::tent_mesh(0.7, &mut prop_rng), v(-2.3, 0.0, -2.1), 0.0_f32, (1.3_f32, 1.1_f32)),
            (banner_mesh(site.faction), v(0.0, 0.0, 0.0), 0.0, (0.25, 0.25)),
            (spikes_mesh(), v(0.0, 0.0, 0.0), 0.0, (0.0, 0.0)),
            (fire_base_mesh(), v(0.2, 0.0, 0.0), 0.0, (0.55, 0.55)),
        ];
        for (m, local, lyaw, (hw, hd)) in solids {
            let h = meshes.add(m);
            let world = place(local);
            commands.spawn((
                Mesh3d(h),
                MeshMaterial3d(prop_mat.clone()),
                Transform { translation: world, rotation: rot_q * ry(lyaw), scale: Vec3::ONE },
                BiomeEntity,
            ));
            if hw > 0.0 && hd > 0.0 {
                crate::blockers::add_obb(world.x, world.z, hw, hd, site.rot + lyaw);
            }
        }

        // The prisoner cage: the fortress frame + hinged door + a full cage of real seated
        // peasants (`spawn_cage`). It blocks like any solid prop — you walk UP TO it to rescue,
        // not through it. The blocker set is append-only, so the frame stays solid after the
        // door opens too (an opened cage is a fine wall; the peasants leave through the door
        // mouth, which sits outside the box).
        let cage_world = place(v(-2.4, 0.0, 2.2));
        spawn_cage(
            commands,
            meshes,
            creature_mats,
            &prop_mat,
            Transform { translation: cage_world, rotation: rot_q * ry(0.6), scale: Vec3::ONE },
            CageKey::Camp(camp),
            crate::villagers::CAMP_RESCUE_POP as usize,
            site.seed ^ 0xca6e_0000,
            false,
        );
        crate::blockers::add_obb(cage_world.x, cage_world.z, 1.2, 1.2, site.rot + 0.6);

        // Log sit-stumps ringing the fire (fire is at local (0.2,0,0)) — orks roost on these
        // between musters. Small footprint blockers so the hero bumps them but they don't wall
        // the camp.
        for (i, local) in [v(1.25, 0.0, 0.5), v(-0.5, 0.0, 0.95), v(0.45, 0.0, -1.05)].into_iter().enumerate() {
            let w = place(local);
            let seed = site.seed.wrapping_add(i as u32 * 37).wrapping_add(7) | 1;
            commands.spawn((
                Mesh3d(meshes.add(sit_stump_mesh(seed))),
                MeshMaterial3d(prop_mat.clone()),
                Transform { translation: w, rotation: ry((seed % 628) as f32 / 100.0), scale: Vec3::ONE },
                BiomeEntity,
            ));
            crate::blockers::add(w.x, w.z, 0.26);
        }
        // The banner's cloth — a fluttering faction flag on the pole `banner_mesh` left bare.
        let flag = crate::banner::spawn_flag(
            commands,
            meshes,
            materials,
            place(v(0.0, 2.5, -1.4)),
            0.8,
            0.5,
            site.faction.hex(),
            Some(0x2a2118),
        );
        commands.entity(flag).insert(BiomeEntity);

        // Campfire flame (emissive + flicker) + rising smoke, at the fire.
        let fire = place(v(0.2, 0.0, 0.0));
        let phase = (site.seed % 1000) as f32 * 0.01;
        commands.spawn((
            Mesh3d(meshes.add(flame_mesh())),
            MeshMaterial3d(flame_mat.clone()),
            Transform::from_translation(fire + Vec3::Y * 0.28),
            Flicker { phase },
            BiomeEntity,
            // Pooled flicker-light so the camp reads from across the map at night (the flame
            // mesh alone is emissive-only). The light rides the flame's own transform — its
            // scale wobble doesn't move the light, and it shares the flame's BiomeEntity life.
            crate::firelight::campfire_light(phase),
        ));
        for i in 0..3 {
            commands.spawn((
                Mesh3d(smoke_puff.clone()),
                MeshMaterial3d(smoke_mat.clone()),
                Transform::from_translation(fire).with_scale(Vec3::splat(0.01)),
                CampSmoke { base: fire + Vec3::Y * 0.4, phase: i as f32 / 3.0, speed: 0.3 },
                BiomeEntity,
            ));
        }

        // Warband: one of each variant at its offset, home-anchored to the camp centre.
        for (i, variant) in VARIANTS.iter().enumerate() {
            let (lx, lz) = WARBAND[i];
            let world = place(v(lx, 0.0, lz));
            let seed = site.seed.wrapping_add((i as u32).wrapping_mul(0x9e37_79b1));
            armory.spawn(commands, *variant, site.faction, site.centre, Vec2::new(world.x, world.z), seed);
        }
    }

    // Keep the armory + per-camp clear timers alive so `respawn_warbands` can repopulate a wiped
    // camp off-screen after a cooldown (the cage + its captives are NOT respawned).
    commands.insert_resource(CampWarbands { armory, cleared_at: vec![None; sites.len()] });
}

/// Respawn a camp's whole warband (one of each variant at its ring offset, home-anchored to the
/// camp centre) — the same layout [`build`] lays down, from the kept-alive armory.
fn spawn_warband(commands: &mut Commands, armory: &orks::Armory, site: &CampSite) {
    let rot_q = Quat::from_rotation_y(site.rot);
    let cy = worldmap::ground_at_world(site.centre.x, site.centre.y).unwrap_or(0.0);
    let centre3 = Vec3::new(site.centre.x, cy, site.centre.y);
    for (i, variant) in VARIANTS.iter().enumerate() {
        let (lx, lz) = WARBAND[i];
        let world = centre3 + rot_q * v(lx, 0.0, lz);
        let seed = site.seed.wrapping_add((i as u32).wrapping_mul(0x9e37_79b1));
        armory.spawn(commands, *variant, site.faction, site.centre, Vec2::new(world.x, world.z), seed);
    }
}

/// Repopulate cleared ork camps — the port of TS `OrkCamp.tsx`'s respawn. A camp whose warband has
/// been wiped starts a [`CAMP_RESPAWN_DELAY`] cooldown; once it elapses AND the hero is at least
/// [`CAMP_RESPAWN_FAR`] away (so the warband returns unseen), the whole warband respawns. Only the
/// orks come back: the prisoner cage stays open, the apples/chests run their own respawn timers
/// (`verbs.rs`), so a re-cleared camp yields no fresh captives — just a renewed fight.
fn respawn_warbands(
    time: Res<Time>,
    hero: Res<crate::player::HeroState>,
    wb: Option<ResMut<CampWarbands>>,
    orks_q: Query<&orks::Ork, (Without<orks::WaveInvader>, Without<crate::dying::Dying>)>,
    mut commands: Commands,
) {
    let Some(mut wb) = wb else { return };
    let sites = plan();
    if sites.is_empty() || wb.cleared_at.len() != sites.len() {
        return;
    }
    let now = time.elapsed_secs();
    // Living warband orks per camp (invaders + fading corpses excluded by the query filter).
    let mut alive = vec![0u32; sites.len()];
    for o in &orks_q {
        let h = o.home();
        if let Some(i) = sites.iter().position(|s| s.centre.distance(h) < 1.0) {
            alive[i] += 1;
        }
    }
    for (i, site) in sites.iter().enumerate() {
        if alive[i] > 0 {
            wb.cleared_at[i] = None; // still populated — reset the clear clock
            continue;
        }
        let Some(cleared) = wb.cleared_at[i] else {
            wb.cleared_at[i] = Some(now); // just went quiet — start the cooldown
            continue;
        };
        if now - cleared >= CAMP_RESPAWN_DELAY && hero.pos.distance(site.centre) > CAMP_RESPAWN_FAR {
            spawn_warband(&mut commands, &wb.armory, site);
            wb.cleared_at[i] = None;
        }
    }
}

// ── Prop models (vertex-coloured, flat-shaded; ported from the TS components) ─────────

const POLE: u32 = 0x3a2a1a;
const SKULL: u32 = 0xe0d8c0;
const STONE: u32 = 0x6e6e76;
const LOG_LIGHT: u32 = 0x7a4a26;
const LOG_DARK: u32 = 0x3a2a1a;

// (The camps' bespoke A-frame tent + faction tent colours are gone — every warband now
//  pitches the fortress hide tent, `ork_fortress::tent_mesh`; the banner carries the
//  faction colour instead.)

/// Warband banner pole — the faction-coloured flag itself is a fluttering cloth entity
/// (banner.rs), spawned alongside the solids in [`build`].
fn banner_mesh(_f: Faction) -> Mesh {
    group(vec![cyl(0.03, 3.0, v(0.0, 1.5, -1.4), Quat::IDENTITY, lin(POLE))])
}

/// Two skull-topped spikes (decorative; no blocker).
fn spikes_mesh() -> Mesh {
    let spike = |x: f32, z: f32, rot: f32| {
        vec![
            cyl(0.025, 0.8, v(x, 0.4, z), ry(rot), lin(POLE)),
            bx(0.12, 0.13, 0.13, v(x, 0.85, z), lin(SKULL)),
        ]
    };
    let mut parts = spike(-0.9, 1.4, 0.4);
    parts.extend(spike(1.6, 1.2, -0.6));
    group(parts)
}

// (The opened-husk cage model is gone — a rescue no longer swaps meshes at all. The one cage
//  model (`ork_fortress::cage_mesh`) persists and its hinged door child swings open by
//  animation: see [`spawn_cage`] / [`swing_cage_doors`].)

/// Campfire base — a ring of stones + two crossed logs (the solid, vertex-coloured part).
fn fire_base_mesh() -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for i in 0..7 {
        let a = (i as f32 / 7.0) * TAU;
        p.push(orb_mesh(0.13, v(a.cos() * 0.42, 0.07, a.sin() * 0.42), lin(STONE)));
    }
    p.push(cyl(0.045, 0.72, v(0.0, 0.08, 0.0), xyz(FRAC_PI_2, 0.0, FRAC_PI_2 / 2.0), lin(LOG_LIGHT)));
    p.push(cyl(0.045, 0.72, v(0.0, 0.13, 0.0), xyz(FRAC_PI_2, 0.0, -FRAC_PI_2 / 2.0), lin(LOG_DARK)));
    group(p)
}

/// A rough log stool ringing the campfire — orks roost on these between musters (the "sit-stumps").
/// A short tapered trunk with a lighter sawn top + heartwood ring and a low root flare; `seed`
/// jitters the height so a ring of them doesn't read as clones.
fn sit_stump_mesh(mut seed: u32) -> Mesh {
    let h = 0.36 + (next_u32(&mut seed) % 110) as f32 / 1000.0; // 0.36..0.47
    group(vec![
        cyl(0.25, h, v(0.0, h * 0.5, 0.0), Quat::IDENTITY, lin(LOG_DARK)), // trunk
        cyl(0.30, 0.09, v(0.0, 0.045, 0.0), Quat::IDENTITY, lin(LOG_DARK)), // root flare
        cyl(0.235, 0.05, v(0.0, h, 0.0), Quat::IDENTITY, lin(LOG_LIGHT)), // sawn top (lighter heartwood)
        cyl(0.12, 0.055, v(0.0, h + 0.004, 0.0), Quat::IDENTITY, lin(LOG_DARK)), // inner growth ring
    ])
}

/// Flame cones (untinted — the emissive flame material colours them). Built about y≈0 so the
/// flicker scale grows it from the fire.
fn flame_mesh() -> Mesh {
    let outer = Cone { radius: 0.17, height: 0.55 }.mesh().build().translated_by(v(0.0, 0.27, 0.0));
    let inner = Cone { radius: 0.09, height: 0.35 }.mesh().build().translated_by(v(0.0, 0.2, 0.0));
    let mut m = outer;
    m.merge(&inner).expect("cones share attributes");
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

// ── Mesh helpers ─────────────────────────────────────────────────────────────────

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("camp parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn cyl(r: f32, h: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(6).build().rotated_by(rot).translated_by(off), c)
}
fn orb_mesh(r: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    tinted(Sphere::new(r).mesh().ico(0).unwrap().translated_by(off), c)
}

// ── Deterministic mulberry32 RNG ─────────────────────────────────────────────────────

fn next_u32(s: &mut u32) -> u32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    t ^ (t >> 14)
}
fn rng01(s: &mut u32) -> f32 {
    next_u32(s) as f32 / 4_294_967_296.0
}
fn rng_range(s: &mut u32, lo: f32, hi: f32) -> f32 {
    lo + rng01(s) * (hi - lo)
}
