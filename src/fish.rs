//! Ambient water life — a few small fish that glide just under the surface near the hero and
//! **occasionally leap** in an arc when he's beside water. The aquatic twin of `ambient_life`'s
//! butterflies: pure visual charm, no simulation, no save/reset (transient atmosphere like the
//! weather motes / fireflies / butterflies).
//!
//! Design (deliberately SUBTLE — barely-noticed life, a flick of silver, not an aquarium):
//! - A tiny fixed pool follows the hero, but each fish is pinned to a real **open-water body** near
//!   him — the lake or the open sea only (`worldmap::is_open_water_world`), NOT rivers or the marsh
//!   puddles that thread the swamp. Away from open water they dissolve out, exactly like the
//!   butterflies vanish off green country.
//! - Swimming, a fish sits a hair BELOW the translucent surface — a soft gliding shadow at ~half
//!   alpha, tail swishing — so it reads as life under the water, not a card floating on it.
//! - Every few seconds a fish **breaches**: a parabolic leap that clears the surface (full alpha,
//!   nose-up on the rise, nose-down on the fall), then slips back under. That's the moment the
//!   player catches out of the corner of their eye — the "sometimes jump out near you".
//!
//! Gating: unlike the day-only butterflies, fish are out day AND night (a moonlit leap is lovely).
//! Movement/animation are visual so they run ungated; only the one-time spawn read of the hero is
//! gated to `Modal::None` so it doesn't fire mid-panel.

use bevy::prelude::*;

use crate::player::HeroState;
use crate::meshkit::tinted;

/// How many fish haunt the water near the hero. Kept low — a few flickers, not a shoal.
const FISH_COUNT: usize = 3;
/// Water-surface Y (mirrors the private `worldmap::SEA_Y`; sea, rivers and lake all show this one
/// flat sheet). Swim depth and leap height are measured off it.
const SEA_Y: f32 = -0.4;
/// How far below the surface a fish cruises while swimming — a soft submerged shadow.
const SWIM_DEPTH: f32 = 0.14;
/// Max horizontal distance (world units) a fish's home spot sits from the hero.
const TERRITORY_R: f32 = 13.0;
/// Don't seat a fish right under the hero's feet — nearest water home is at least this far out.
const MIN_R: f32 = 3.5;
/// Alpha units per second the whole fish fades in/out as water comes and goes near the hero.
const FADE_RATE: f32 = 0.9;
/// A breach lasts this long (seconds) from leaving the water to splashing back.
const LEAP_DUR: f32 = 1.15;
/// Peak height (world units) a leap clears above the surface.
const LEAP_H: f32 = 0.72;
/// How far (world units) a leap glides forward across its arc. Kept short so the whole arc stays
/// inside the water body (the landing point is water-checked before a leap is allowed).
const LEAP_TRAVEL: f32 = 1.4;
/// How many probe samples we spiral out from the hero looking for a water home each reseat.
const WATER_SAMPLES: usize = 28;

/// One fish. Holds its own drifting water territory + leap state + its material (so alpha can
/// fade independently, like each butterfly owns its wing materials).
#[derive(Component)]
struct Fish {
    /// Per-instance phase so each swims / swishes / leaps out of step.
    phase: f32,
    /// Current world-XZ home spot the fish patrols — kept over water (`valid` says whether the
    /// last reseat actually found water near the hero).
    home: Vec2,
    valid: bool,
    /// Opacity 0..1, eased toward 1 when a water home exists and 0 when it doesn't (or the hero is
    /// dead), so the fish dissolves in/out instead of popping.
    fade: f32,
    /// Silvery material shared by this fish's body + tail, so setting its base-colour alpha fades
    /// the whole fish.
    mat: Handle<StandardMaterial>,
    /// Breach state: `leap_t` runs 0→1 across a leap; between leaps `leap_cd` counts down.
    leaping: bool,
    leap_t: f32,
    leap_cd: f32,
    /// Frozen heading (yaw) + XZ launch point captured at take-off so the arc flies straight from
    /// exactly where the fish was, instead of snapping back to `home`.
    leap_dir: f32,
    leap_from: Vec2,
    /// Counter mixed into the pseudo-random next-leap cooldown so leaps stay staggered.
    leap_n: u32,
}

/// A swishing tail fin child: swings about vertical at the fish's rear. `base` is its rest yaw.
#[derive(Component)]
struct TailFin {
    base: Quat,
    rate: f32,
    phase: f32,
}

/// Marks the one-time spawn done.
#[derive(Resource, Default)]
struct FishSpawned(bool);

pub struct FishPlugin;

impl Plugin for FishPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FishSpawned>().add_systems(
            Update,
            (
                spawn_once.run_if(in_state(crate::game_state::Modal::None)),
                swim_fish,
                swish_tails,
            ),
        );
        // Screenshot hook: park the fish varieties frozen mid-leap in a lit row for model close-ups.
        if std::env::var("FOREST_FISHLINE").is_ok() {
            app.add_systems(Startup, spawn_fishline);
        }
    }
}

// ── Mesh helpers (local copies of the props.rs / critters.rs contract) ───────────────
/// Merge + hard flat-shade for the crisp low-poly facet look (duplicate BEFORE flat-normals —
/// `compute_flat_normals` panics on an indexed mesh).
fn group(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it {
        base.merge(&p).expect("fish parts share attributes");
    }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), crate::palette::lin(c))
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), crate::palette::lin(c))
}

/// A small low-poly fish, nose at +Z (so yaw-from-velocity points it the way it swims). Returns
/// `(body, tail)` — the body is one merged flat-shaded mesh centred on y=0; the tail is a separate
/// child (built in tail-local space, front edge at its origin) that `swish_tails` swings.
/// `back`/`flank`/`fin` are the three tone hexes for this fish variety.
fn build_fish(back: u32, flank: u32, fin: u32) -> (Mesh, Mesh) {
    // Body: a tapered spindle of boxes from the tail peduncle (−Z) to the nose (+Z).
    let body = group(vec![
        bx(0.13, 0.17, 0.22, Vec3::new(0.0, 0.0, 0.0), flank), // mid body
        bx(0.09, 0.12, 0.14, Vec3::new(0.0, 0.005, 0.16), flank), // front taper
        bx(0.05, 0.08, 0.08, Vec3::new(0.0, 0.0, 0.26), flank), // blunt nose
        bx(0.07, 0.11, 0.14, Vec3::new(0.0, 0.0, -0.15), flank), // rear taper
        bx(0.03, 0.06, 0.08, Vec3::new(0.0, 0.0, -0.25), flank), // caudal peduncle
        bx(0.135, 0.03, 0.24, Vec3::new(0.0, 0.075, -0.01), back), // darker dorsal stripe
        bx(0.02, 0.055, 0.10, Vec3::new(0.0, 0.10, -0.03), back), // low dorsal fin (a fin, not a sail)
        // Pectoral fins — thin, splayed back-and-out from just behind the gills.
        bxr(0.09, 0.015, 0.06, Vec3::new(0.07, -0.03, 0.06), Quat::from_rotation_z(0.5), fin),
        bxr(0.09, 0.015, 0.06, Vec3::new(-0.07, -0.03, 0.06), Quat::from_rotation_z(-0.5), fin),
    ]);
    // Tail (caudal) fin — a flat forked V in tail-local space, front at origin fanning to −Z.
    let tail = group(vec![
        bxr(0.02, 0.13, 0.12, Vec3::new(0.0, 0.05, -0.08), Quat::from_rotation_x(-0.35), fin), // upper lobe
        bxr(0.02, 0.13, 0.12, Vec3::new(0.0, -0.05, -0.08), Quat::from_rotation_x(0.35), fin), // lower lobe
    ]);
    (body, tail)
}

/// Spawn the pool once the hero (and so the world) exists.
fn spawn_once(
    mut commands: Commands,
    mut done: ResMut<FishSpawned>,
    hero: Option<Res<HeroState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if done.0 || hero.is_none() {
        return;
    }
    done.0 = true;

    // Three fish varieties [back, flank, fin]: silver trout, blue char, and a golden carp.
    let kinds: [(u32, u32, u32); 3] =
        [(0x4a5a3e, 0xb9c2c8, 0x8a9299), (0x4f6373, 0xc6ccd0, 0x99a1a8), (0x8f6f2f, 0xd6bf74, 0xb39a54)];

    for i in 0..FISH_COUNT {
        let (back, flank, fin) = kinds[i % kinds.len()];
        let (body_mesh, tail_mesh) = build_fish(back, flank, fin);
        let body = meshes.add(body_mesh);
        let tail = meshes.add(tail_mesh);
        // Per-fish material: white (so the mesh's own vertex colours show through) at alpha 0, a
        // touch glossy so a leaping fish catches a wet sun/moon glint. Blend so the dissolve shows.
        let mat = mats.add(StandardMaterial {
            base_color: Color::WHITE.with_alpha(0.0),
            perceptual_roughness: 0.35,
            metallic: 0.2,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let phase = i as f32 * 2.39996; // golden-angle spread
        commands
            .spawn((
                Transform::from_xyz(0.0, -100.0, 0.0), // parked offscreen until first swim update
                Visibility::Hidden,
                Fish {
                    phase,
                    home: Vec2::ZERO,
                    valid: false,
                    fade: 0.0,
                    mat: mat.clone(),
                    leaping: false,
                    leap_t: 0.0,
                    leap_cd: 2.0 + phase, // stagger the first leaps
                    leap_dir: 0.0,
                    leap_from: Vec2::ZERO,
                    leap_n: i as u32,
                },
                bevy::light::NotShadowCaster,
            ))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(mat.clone()),
                    Transform::default(),
                    bevy::light::NotShadowCaster,
                ));
                // Tail pivot at the peduncle; the fin fans back from there and swishes about Y.
                let base = Quat::IDENTITY;
                p.spawn((
                    Mesh3d(tail),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.29),
                    TailFin { base, rate: 9.0 + (phase * 1.3).sin() * 2.0, phase },
                    bevy::light::NotShadowCaster,
                ));
            });
    }
}

/// True ONLY over a real standing-water body — the lake or the open sea. Explicitly NOT rivers
/// (the marsh brooks that thread the swamp) or land, so fish never spawn in / leap out of a
/// narrow channel and clip through the bank. (`ground_at_world` is `None` over rivers too, which is
/// exactly the trap the old check fell into.)
#[inline]
fn is_water(x: f32, z: f32) -> bool {
    crate::worldmap::is_open_water_world(x, z)
}

/// Spiral out from the hero and return the first water spot found (biased by `phase` so each fish
/// scatters to its own patch). `None` when there's no water near the hero at all.
fn find_water_home(hx: f32, hz: f32, phase: f32) -> Option<Vec2> {
    for k in 0..WATER_SAMPLES {
        let f = k as f32 / WATER_SAMPLES as f32;
        let ang = phase * 2.0 + k as f32 * 2.39996; // golden-angle spiral
        let r = MIN_R + f * (TERRITORY_R - MIN_R);
        let x = hx + ang.cos() * r;
        let z = hz + ang.sin() * r;
        if is_water(x, z) {
            return Some(Vec2::new(x, z));
        }
    }
    None
}

/// Cheap deterministic 0..1 hash for staggering the next-leap cooldown (no `Math::random`).
#[inline]
fn frac_hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(747796405).wrapping_add(2891336453);
    x ^= x >> 15;
    x = x.wrapping_mul(2654435769);
    x ^= x >> 13;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Each frame: keep every fish pinned to a water spot near the hero, glide it under the surface,
/// and periodically fling it into a breaching arc.
fn swim_fish(
    time: Res<Time>,
    hero: Option<Res<HeroState>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Fish, &mut Transform, &mut Visibility)>,
) {
    let Some(hero) = hero else { return };
    let t = time.elapsed_secs_wrapped();
    let dt = time.delta_secs();
    let hx = hero.pos.x;
    let hz = hero.pos.y; // hero.pos.y is the world Z (movement is XZ)

    for (mut f, mut tf, mut vis) in &mut q {
        // Reseat the home if it's stale: no home yet, drifted out of range, or dried up as the hero
        // walked away from the shore. (Skip while mid-leap so we don't yank the arc.)
        let too_far = f.home.distance_squared(Vec2::new(hx, hz)) > (TERRITORY_R * 1.6).powi(2);
        if !f.leaping && (!f.valid || too_far || !is_water(f.home.x, f.home.y)) {
            match find_water_home(hx, hz, f.phase) {
                Some(h) => {
                    f.home = h;
                    f.valid = true;
                }
                None => f.valid = false,
            }
        }

        // Fade toward visible only when we have a water home and the hero's alive.
        let show = f.valid && hero.alive;
        let target = if show { 1.0 } else { 0.0 };
        f.fade += (target - f.fade).clamp(-FADE_RATE * dt, FADE_RATE * dt);

        if f.fade <= 0.001 {
            *vis = Visibility::Hidden;
            f.leaping = false; // reset any interrupted leap so it re-arms cleanly on return
            f.leap_t = 0.0;
            continue;
        }
        *vis = Visibility::Visible;

        let p = f.phase;
        // Local patrol: small lazy loops around the home spot. If a loop would carry the fish off
        // the water body (small lakes are tight), fall back to the home spot so it never wanders
        // onto land and sinks through the bank.
        let swim = 1.1;
        let mut lx = f.home.x + (t * 0.6 + p).cos() * swim + (t * 0.31 + p * 2.0).sin() * 0.4;
        let mut lz = f.home.y + (t * 0.6 + p).sin() * swim + (t * 0.27 + p * 1.5).cos() * 0.4;
        if !is_water(lx, lz) {
            lx = f.home.x;
            lz = f.home.y;
        }

        // Leap timing: while swimming, count down; when it hits zero (and the fish is well faded-in
        // over live water), take off along the current heading.
        if f.leaping {
            f.leap_t += dt / LEAP_DUR;
            if f.leap_t >= 1.0 {
                f.leaping = false;
                f.leap_t = 0.0;
                f.leap_n = f.leap_n.wrapping_add(1);
                f.leap_cd = 4.0 + frac_hash(f.leap_n) * 6.0; // next breach in ~4..10s
            }
        } else {
            f.leap_cd -= dt;
            if f.leap_cd <= 0.0 && f.fade > 0.6 && show {
                let from = Vec2::new(tf.translation.x, tf.translation.z);
                // Candidate headings: along the swim direction first, then a few fanned + inward
                // (toward home) fallbacks. Take the first whose landing point is still open water so
                // the whole arc stays over the pond/sea and never lands on the bank.
                let swim_dir = {
                    let d = Vec2::new(lx, lz) - from;
                    if d.length_squared() > 1e-6 { d.y.atan2(d.x) } else { p }
                };
                let to_home = {
                    let d = f.home - from;
                    if d.length_squared() > 1e-6 { d.y.atan2(d.x) } else { p }
                };
                let dir = [swim_dir, to_home, swim_dir + 1.2, swim_dir - 1.2, to_home + 0.6]
                    .into_iter()
                    .find(|&a| {
                        let land = from + Vec2::new(a.cos(), a.sin()) * LEAP_TRAVEL;
                        is_water(land.x, land.y)
                    });
                match dir {
                    Some(a) => {
                        f.leaping = true;
                        f.leap_t = 0.0;
                        f.leap_from = from;
                        // Store yaw in the same convention `swim_fish` uses for facing (atan2(x, z)).
                        f.leap_dir = a.cos().atan2(a.sin());
                    }
                    // Boxed in against the shore — skip this breach, try again shortly.
                    None => f.leap_cd = 1.2,
                }
            }
        }

        let (x, z, y, pitch) = if f.leaping {
            // Parabolic breach: rise-and-fall height, plus a little forward travel along the heading.
            let s = f.leap_t;
            let h = (s * core::f32::consts::PI).sin() * LEAP_H; // 0 → peak → 0
            let travel = s * LEAP_TRAVEL; // glides forward across the arc
            let x = f.leap_from.x + f.leap_dir.sin() * travel;
            let z = f.leap_from.y + f.leap_dir.cos() * travel;
            // Nose-up on the way out, nose-down on the way down.
            let pitch = (s * core::f32::consts::PI).cos() * 0.9;
            (x, z, SEA_Y + h, pitch)
        } else {
            // Cruising just under the surface with a soft depth bob.
            let y = SEA_Y - SWIM_DEPTH + (t * 1.3 + p).sin() * 0.03;
            (lx, lz, y, 0.0)
        };

        let prev = tf.translation;
        tf.translation = Vec3::new(x, y, z);
        // Heading: during a leap hold the frozen take-off yaw; else face travel.
        let yaw = if f.leaping {
            f.leap_dir
        } else {
            let d = tf.translation - prev;
            if d.x.abs() + d.z.abs() > 1e-4 { d.x.atan2(d.z) } else { p }
        };
        tf.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);

        // Alpha: a soft ~half-visible shadow while submerged, ramping to full as it clears the
        // surface on a leap (so the breach reads crisp and the underwater glide stays subtle).
        let above = ((y - SEA_Y) / 0.25).clamp(0.0, 1.0);
        let alpha = f.fade * (0.45 + 0.55 * above);
        if let Some(mut m) = mats.get_mut(&f.mat) {
            m.base_color.set_alpha(alpha);
        }
    }
}

/// Debug screenshot hook: `FOREST_FISHLINE="x,z"` parks one of every fish variety in a lit row at
/// the given world XZ, each frozen at a leap apex (nose-up), for model close-ups (mirrors
/// `FOREST_TREELINE`/`FOREST_ORKLINE`). No `Fish`/`TailFin` components, so the swim/swish systems
/// leave them planted.
fn spawn_fishline(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let Ok(s) = std::env::var("FOREST_FISHLINE") else { return };
    let p: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    if p.len() != 2 {
        return;
    }
    let kinds: [(u32, u32, u32); 3] =
        [(0x4a5a3e, 0xb9c2c8, 0x8a9299), (0x4f6373, 0xc6ccd0, 0x99a1a8), (0x8f6f2f, 0xd6bf74, 0xb39a54)];
    for (i, (back, flank, fin)) in kinds.into_iter().enumerate() {
        let (body_mesh, tail_mesh) = build_fish(back, flank, fin);
        let body = meshes.add(body_mesh);
        let tail = meshes.add(tail_mesh);
        let mat = mats.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.35,
            metallic: 0.2,
            ..default()
        });
        let x = p[0] + i as f32 * 1.2 - (kinds.len() as f32 - 1.0) * 0.6;
        commands
            .spawn((
                // Broadside to +X (yaw 90°) so the fish silhouette faces the camera, pitched nose-up.
                Transform::from_xyz(x, 1.2, p[1])
                    .with_rotation(Quat::from_rotation_y(core::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(0.6))
                    .with_scale(Vec3::splat(2.5)),
                Visibility::Visible,
                bevy::light::NotShadowCaster,
            ))
            .with_children(|c| {
                c.spawn((Mesh3d(body), MeshMaterial3d(mat.clone()), Transform::default(), bevy::light::NotShadowCaster));
                c.spawn((
                    Mesh3d(tail),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.29),
                    bevy::light::NotShadowCaster,
                ));
            });
    }
}

/// Swish every tail fin about vertical — a steady side-to-side beat, staggered per fish.
fn swish_tails(time: Res<Time>, mut q: Query<(&TailFin, &mut Transform)>) {
    let t = time.elapsed_secs_wrapped();
    for (fin, mut tf) in &mut q {
        let swing = (t * fin.rate + fin.phase).sin() * 0.5; // ~±0.5 rad sweep
        tf.rotation = Quat::from_rotation_y(swing) * fin.base;
    }
}
