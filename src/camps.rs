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

// ── Plugin (campfire flicker + smoke) ────────────────────────────────────────────────

pub struct CampsPlugin;

impl Plugin for CampsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (flicker_flames, drift_smoke));
        app.add_systems(
            Update,
            respawn_warbands.run_if(in_state(crate::game_state::Modal::None)),
        );
    }
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

/// Tags JUST the prisoner-cage prop of a camp so the rescue system can despawn it (open the
/// cage) when the warband is cleared. `camp` = the enumerate index of `plan()` — the same index
/// [`cage_positions`] and `villagers::camp_rescue` use.
#[derive(Component)]
pub struct Cage {
    pub camp: usize,
}

/// Index of the cage within the per-camp `solids` vec in [`build`].
const CAGE_SOLID: usize = 3;

/// Seconds after a camp's warband is wiped before it repopulates (TS `OrkCamp.tsx`'s
/// `CAMP_RESPAWN_DELAY`).
const CAMP_RESPAWN_DELAY: f32 = 60.0;
/// The hero must be at least this far (world units) from a camp for it to repopulate, so a cleared
/// warband returns OUT OF SIGHT rather than in front of the player (TS `CAMP_RESPAWN_FAR` = 40).
const CAMP_RESPAWN_FAR: f32 = 40.0;

/// The prebuilt ork [`orks::Armory`] kept alive past [`build`] so a wiped warband can respawn, plus
/// each camp's clear timestamp (`Some(t)` once it has zero living orks, `None` while populated).
/// Captives are deliberately NOT tracked here — a freed cage stays open and empty, matching the
/// original: a camp's prisoners are a one-time rescue, the warband a renewable threat.
#[derive(Resource)]
struct CampWarbands {
    armory: orks::Armory,
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
    if crate::castle::in_footprint(cx, cz) || Vec2::new(cx, cz).length() < 24.0 {
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
) {
    let sites = plan();
    if sites.is_empty() {
        return;
    }

    // Shared vertex-colour material for the ORKS (grime on a face-sized limb is noise).
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    // The structural props (tents, cage, totem, banner, fire ring) carry a neutral grime-grain
    // detail texture multiplied over their vertex colours — same trick as Gnashfang Hold, so
    // the camps read rough/dirty instead of flat-shaded clean. One extra batch.
    let grime = crate::biome::GroundDetail {
        scale: 1.0,
        strength: 0.9,
        variation: 0.5,
        seed: 13.0,
        dark: 0x8e887e,
        base: 0xc2bcb0,
        light: 0xf2ece0,
        grain: 0.9,
        streak: 0.7,
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
        // prop's own frame; (0,0) = no collision). The tent + cage are the FORTRESS models
        // (`ork_fortress::tent_mesh`/`cage_mesh`) so every warband pitches the same gear as
        // Gnashfang Hold; both are SOLID — they register blocker boxes, so the hero and the
        // warband route around them.
        // Tent NW (−z), cage SW (+z), well apart so the bigger fortress models don't overlap
        // (they did at the old −2.4/+2.2 spots). Tent at 0.7× the fortress size to fit the camp.
        let mut prop_rng = site.seed | 1;
        let solids = vec![
            (crate::ork_fortress::tent_mesh(0.7, &mut prop_rng), v(-2.3, 0.0, -2.1), 0.0_f32, (1.3_f32, 1.1_f32)),
            (banner_mesh(site.faction), v(0.0, 0.0, 0.0), 0.0, (0.25, 0.25)),
            (spikes_mesh(), v(0.0, 0.0, 0.0), 0.0, (0.0, 0.0)),
            (crate::ork_fortress::cage_mesh(), v(-2.4, 0.0, 2.2), 0.6, (1.2, 1.2)),
            (fire_base_mesh(), v(0.2, 0.0, 0.0), 0.0, (0.55, 0.55)),
        ];
        for (idx, (m, local, lyaw, (hw, hd))) in solids.into_iter().enumerate() {
            let h = meshes.add(m);
            let world = place(local);
            let mut e = commands.spawn((
                Mesh3d(h),
                MeshMaterial3d(prop_mat.clone()),
                Transform { translation: world, rotation: rot_q * ry(lyaw), scale: Vec3::ONE },
                BiomeEntity,
            ));
            if idx == CAGE_SOLID {
                // The cage: tag it for rescue-despawn. It blocks like any solid prop — you walk
                // UP TO it to rescue, not through it. The blocker set is append-only, so the husk
                // left after the cage opens stays solid too (a closed→open cage is a fine wall).
                e.insert(Cage { camp });
                crate::blockers::add_obb(world.x, world.z, hw, hd, site.rot + lyaw);
            } else if hw > 0.0 && hd > 0.0 {
                crate::blockers::add_obb(world.x, world.z, hw, hd, site.rot + lyaw);
            }
        }

        // A carved war-totem GLARING AT THE CASTLE — wherever you meet a camp, its totem's
        // gaze points home, making the warband's intent legible without a word of UI.
        let totem_world = place(v(2.4, 0.0, -1.6));
        let totem_yaw = (-totem_world.x).atan2(-totem_world.z);
        commands.spawn((
            Mesh3d(meshes.add(totem_mesh(site.faction))),
            MeshMaterial3d(prop_mat.clone()),
            Transform { translation: totem_world, rotation: ry(totem_yaw), scale: Vec3::ONE },
            BiomeEntity,
        ));
        crate::blockers::add(totem_world.x, totem_world.z, 0.3);

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
const WOOD: u32 = 0x4b3724;
const WOOD_DARK: u32 = 0x33271a;
const BAR: u32 = 0x6b6f76;
const CAPTIVE_BODY: u32 = 0x7c6a54;
const CAPTIVE_HEAD: u32 = 0xcaa980;
const STONE: u32 = 0x6e6e76;
const LOG_LIGHT: u32 = 0x7a4a26;
const LOG_DARK: u32 = 0x3a2a1a;

// (The camps' bespoke A-frame tent + faction tent colours are gone — every warband now
//  pitches the fortress hide tent, `ork_fortress::tent_mesh`; the banner + totem carry the
//  faction colour instead.)

/// Warband banner pole — the faction-coloured flag itself is a fluttering cloth entity
/// (banner.rs), spawned alongside the solids in [`build`].
fn banner_mesh(_f: Faction) -> Mesh {
    group(vec![cyl(0.03, 3.0, v(0.0, 1.5, -1.4), Quat::IDENTITY, lin(POLE))])
}

/// War totem — three stacked carved heads on a stump, banded in the warband's paint, horns
/// and a skull on top. Authored facing +Z (the build yaws it toward the castle). Each head
/// twists a little off the one below so the column reads hand-hewn, not lathed.
fn totem_mesh(f: Faction) -> Mesh {
    let paint = lin(f.hex());
    let eye = lin(0x16130f);
    let mut p: Vec<Mesh> = Vec::new();
    // Base stump.
    p.push(cyl(0.15, 0.25, v(0.0, 0.125, 0.0), Quat::IDENTITY, lin(POLE)));
    // A carved head: block + brow ridge + two sunken eyes + a mouth slit, twisted by `yaw`.
    let head = |p: &mut Vec<Mesh>, w: f32, y0: f32, h: f32, yaw: f32, c: u32| {
        let q = ry(yaw);
        let cy = y0 + h / 2.0;
        p.push(bxr(w, h, w * 0.9, v(0.0, cy, 0.0), q, lin(c)));
        p.push(bxr(w * 0.92, 0.07, w * 0.2, q * v(0.0, 0.0, w * 0.40) + v(0.0, y0 + h * 0.78, 0.0), q, lin(WOOD_DARK)));
        for sx in [-1.0_f32, 1.0] {
            p.push(bxr(0.09, 0.09, 0.06, q * v(sx * w * 0.22, 0.0, w * 0.45) + v(0.0, y0 + h * 0.62, 0.0), q, eye));
        }
        p.push(bxr(w * 0.5, 0.05, 0.06, q * v(0.0, 0.0, w * 0.45) + v(0.0, y0 + h * 0.25, 0.0), q, eye));
    };
    head(&mut p, 0.52, 0.25, 0.46, 0.10, WOOD);
    p.push(bx(0.50, 0.08, 0.46, v(0.0, 0.75, 0.0), paint)); // paint band
    head(&mut p, 0.46, 0.79, 0.42, -0.14, WOOD_DARK);
    p.push(bx(0.44, 0.08, 0.40, v(0.0, 1.25, 0.0), paint)); // paint band
    head(&mut p, 0.40, 1.29, 0.38, 0.07, WOOD);
    // Horns flaring off the top head + the skull crowning it.
    for sx in [-1.0_f32, 1.0] {
        let horn = Cone { radius: 0.05, height: 0.28 }
            .mesh()
            .build()
            .translated_by(v(0.0, 0.14, 0.0))
            .rotated_by(rz(sx * 1.1))
            .translated_by(v(sx * 0.22, 1.60, 0.0));
        p.push(tinted(horn, lin(SKULL)));
    }
    p.push(bx(0.18, 0.17, 0.17, v(0.0, 1.78, 0.0), lin(SKULL)));
    group(p)
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

// (The camps' bespoke closed cage is gone too — the closed state is the fortress cage,
//  `ork_fortress::cage_mesh` (W 2.2 / H 1.8); only the opened husk below remains local,
//  scaled up at spawn to match the bigger closed cage it replaces.)

/// The OPENED cage — the fortress cage's frame proportions, the east-face bars gone (door
/// swung out) and no captives inside. Spawned in place of the closed cage on a rescue, so
/// it reads as the cage *opening* rather than vanishing.
fn cage_open_mesh() -> Mesh {
    const W: f32 = 1.7;
    const H: f32 = 1.5;
    const HW: f32 = W / 2.0;
    let wood = lin(WOOD);
    let dark = lin(WOOD_DARK);
    let bar = lin(BAR);
    let mut p: Vec<Mesh> = Vec::new();
    p.push(bx(W + 0.12, 0.12, W + 0.12, v(0.0, 0.06, 0.0), dark)); // floor
    for (sx, sz) in [(-HW, -HW), (HW, -HW), (-HW, HW), (HW, HW)] {
        p.push(bx(0.14, H, 0.14, v(sx, H / 2.0, sz), wood)); // corner posts
    }
    p.push(bx(W, 0.1, 0.1, v(0.0, H - 0.05, -HW), wood));
    p.push(bx(W, 0.1, 0.1, v(0.0, H - 0.05, HW), wood));
    p.push(bx(0.1, 0.1, W, v(-HW, H - 0.05, 0.0), wood));
    p.push(bx(0.1, 0.1, W, v(HW, H - 0.05, 0.0), wood));
    // Bars on N / S / W only — the EAST side is the door, now open.
    for o in [-0.45f32, 0.0, 0.45] {
        p.push(bx(0.07, H - 0.06, 0.07, v(o, H / 2.0, -HW), bar)); // north
        p.push(bx(0.07, H - 0.06, 0.07, v(o, H / 2.0, HW), bar)); // south
        p.push(bx(0.07, H - 0.06, 0.07, v(-HW, H / 2.0, o), bar)); // west
    }
    // The swung-open door panel, hinged at the SE post and flung outward.
    p.push(bxr(0.06, H - 0.1, W - 0.12, v(HW + 0.55, H / 2.0, HW - 0.45), ry(FRAC_PI_2 * 0.85), bar));
    group(p)
}

/// Replace a closed cage with the opened husk at the same pose (called by the rescue path).
pub fn open_cage(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    at: Transform,
) {
    let mat = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    // ×1.25: the open husk is authored at the OLD cage size (W 1.7); the closed cage it
    // replaces is now the fortress one (W 2.2), so scale up to swing open at the same bulk.
    let at = Transform { scale: at.scale * 1.25, ..at };
    commands.spawn((Mesh3d(meshes.add(cage_open_mesh())), MeshMaterial3d(mat), at, BiomeEntity));
}

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
fn rx(a: f32) -> Quat {
    Quat::from_rotation_x(a)
}
fn ry(a: f32) -> Quat {
    Quat::from_rotation_y(a)
}
fn rz(a: f32) -> Quat {
    Quat::from_rotation_z(a)
}
fn xyz(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
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
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: [f32; 4]) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), c)
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
