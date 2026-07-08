//! Extra forest-floor charm — fallen mossy logs + stumps, berry bushes, riverside
//! reeds + lily pads, hovering fireflies, and mushroom rings. A self-contained
//! plugin with its OWN deterministic placement (does NOT touch `scatter.rs`).
//!
//! Mesh contract (matches the existing mesh modules `trees.rs` / `props.rs` /
//! `groundcover.rs` + the verified-APIs doc §9):
//! - One `pub struct DecorPlugin` doing all setup in `Startup` (+ one `Update` system
//!   that bobs the fireflies).
//! - Every prop mesh is built ONCE, base at y=0 (except fireflies, which hover, and
//!   lily pads, which float on the river surface at y≈0). Parts are coloured via
//!   `crate::palette::lin` + `tinted` and merged with `Mesh::merge`, then flat-shaded
//!   (`duplicate_vertices()` then `compute_flat_normals()`) for the crisp low-poly look.
//! - Props share ONE white vertex-colour `StandardMaterial` so the renderer batches
//!   them; the only exception is the fireflies, which use small per-colour emissive
//!   `StandardMaterial`s (so bloom picks them up).
//! - Placement is a seeded Mulberry32 RNG (const seed, varied by index — no `random()`),
//!   identical to `scatter.rs`'s RNG so the layout is stable across runs.
//!
//! RIVER DEPENDENCY: this module reads `crate::water::on_river` (skip the water when
//! placing ground props) and `crate::water::river_bank_t` (dress the banks with reeds /
//! float lily pads on the surface). It therefore MUST be added to `main.rs` AFTER
//! `WaterPlugin` so the `water` module exists.

// The whole `decor` charm (logs, berry bushes, reeds, lily pads, mushroom rings, fireflies) is
// authored but not wired into the world map yet — kept for future per-region dressing. Only the
// firefly bob system is live (it just has nothing to bob until `build` is called). Allow the
// resulting dead code in the meantime.
#![allow(dead_code)]

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::palette::lin;
use crate::terrain::HALF;
use crate::meshkit::{flat_shaded, merged, tinted};

// ── Decor palette (extends palette.rs; kept local since these tones are decor-only) ──
const LOG_BARK: u32 = 0x4f3320; // damp brown fallen-log bark
const LOG_BARK_DARK: u32 = 0x3c2618; // shadowed underside of the log
const LOG_MOSS: u32 = 0x4f8a3a; // green moss stripe along the log's top
const LOG_END: u32 = 0x7a5636; // lighter sawn/broken end-grain disc

const STUMP_BARK: u32 = 0x5a3a22; // stump bark (matches TREE_TRUNK family)
const STUMP_TOP: u32 = 0x8a6a44; // lighter cut-top wood
const STUMP_RING: u32 = 0xa98456; // pale growth-ring highlight on the cut top

const BERRY_LEAF_DARK: u32 = 0x2f6f30; // berry-bush dark skirt
const BERRY_LEAF_MID: u32 = 0x3f8f3a; // berry-bush body
const BERRY_LEAF_LIGHT: u32 = 0x57a84a; // sunlit crown
const BERRY_RED: u32 = 0xc02a2a; // red berries
const BERRY_BLUE: u32 = 0x3a4fb0; // blue berries

const REED_STALK: u32 = 0x6f8a3a; // cattail/reed green stalk
const REED_STALK_TIP: u32 = 0x9fc066; // lighter reed tip
const REED_HEAD: u32 = 0x6a4326; // brown cattail seed head

const LILY_PAD: u32 = 0x2f7a44; // lily-pad green disc
const LILY_PAD_EDGE: u32 = 0x256238; // darker pad underside / rim ring
const LILY_FLOWER: u32 = 0xf0a8d8; // pink lily bloom
const LILY_FLOWER_CORE: u32 = 0xf6e08a; // pale yellow flower centre

const RING_MUSH_STEM: u32 = 0xeadfca; // pale stem of the ring mushrooms
const RING_MUSH_CAP: u32 = 0xc06a3a; // toadstool cap (warm orange-brown)

// Firefly glow colour (sRGB → linear in the emissive). Warm yellow-green.
const FIREFLY_GLOW: Color = Color::srgb(0.85, 1.0, 0.45);
/// How strongly the firefly emits (drives bloom — high so it blooms).
const FIREFLY_EMISSIVE: f32 = 60.0;

const SEED: u32 = 9173;
const TAU: f32 = std::f32::consts::TAU;
const FRAC_PI_2: f32 = std::f32::consts::FRAC_PI_2;

// ── Mulberry32 deterministic RNG (same as scatter.rs so layouts stay stable) ──
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f32) / 4_294_967_296.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next() * (hi - lo)
    }
}

// ── Mesh helpers (mirror props.rs / groundcover.rs / trees.rs) ──────────────────

fn y(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// A faceted icosphere blob (ico detail 0), optionally squashed, centred at `off`.
fn ball_at(r: f32, off: Vec3, squash: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        lin(c),
    )
}

/// An upright cylinder whose centre sits at `cy` (so a part of height `h` rooted at
/// y=0 uses `cy = h/2`). `res` ≥ 3 (the Cylinder builder debug-asserts resolution > 2).
fn cyl_up(r: f32, h: f32, cy: f32, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(y(cy)), lin(c))
}

// ── Fireflies ────────────────────────────────────────────────────────────────────

/// A hovering glow mote. Stores its drift centre + per-mote phase/speed so the bob is
/// deterministic and each firefly moves independently.
#[derive(Component)]
struct Firefly {
    base: Vec3,
    phase: f32,
    speed: f32,
    bob: f32,
    drift: f32,
}

pub struct DecorPlugin;

impl Plugin for DecorPlugin {
    fn build(&self, app: &mut App) {
        // Only the firefly bob runs as a system; the decor itself is spawned by the
        // forest biome's `landmarks` hook via [`build`].
        app.add_systems(Update, bob_fireflies);
    }
}

/// Spawn the forest's decor charm (logs, stumps, berry bushes, reeds + lily pads,
/// fireflies, mushroom rings). Called from the Forest biome's `landmarks` hook. Every
/// entity is tagged [`crate::biome::BiomeEntity`] so a biome switch wipes it.
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    use crate::biome::BiomeEntity;
    // One shared white vertex-colour material — same recipe as scatter.rs so all decor
    // props batch with the rest of the forest.
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });

    // Build each prop mesh ONCE; keep cloneable handles.
    let logs: Vec<Handle<Mesh>> = (0..2).map(|v| meshes.add(build_log_mesh(v))).collect();
    let stump = meshes.add(build_stump_mesh());
    let berries: Vec<Handle<Mesh>> = (0..2).map(|v| meshes.add(build_berry_bush_mesh(v))).collect();
    let reeds: Vec<Handle<Mesh>> = (0..2).map(|v| meshes.add(build_reed_clump_mesh(v))).collect();
    let lily_plain = meshes.add(build_lily_pad_mesh(false));
    let lily_flower = meshes.add(build_lily_pad_mesh(true));
    let ring_mushroom = meshes.add(build_ring_mushroom_mesh());

    let lo = -HALF;
    let hi = HALF;
    let mut r = Rng(SEED);

    let spawn = |commands: &mut Commands, mesh: Handle<Mesh>, t: Transform| {
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat.clone()), t, BiomeEntity));
    };
    // Tiny / flat ground dressing doesn't need to cast shadows.
    let spawn_cover = |commands: &mut Commands, mesh: Handle<Mesh>, t: Transform| {
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat.clone()), t, NotShadowCaster, BiomeEntity));
    };

    // ── Fallen logs (+ a small stump beside most of them) ──
    // A handful, spread over the patch on dry ground (skip the river).
    let mut placed_logs = 0;
    for i in 0..40 {
        if placed_logs >= 7 {
            break;
        }
        let x = r.range(lo + 2.0, hi - 2.0);
        let z = r.range(lo + 2.0, hi - 2.0);
        // A log is ~2.6u long; reject if either it's over the river or its midpoint is
        // too close to the centre (keep the open framing in front of the camera clear).
        if crate::water::on_river(x, z) || (x * x + z * z) < 9.0 {
            continue;
        }
        let yaw = r.range(0.0, TAU);
        let s = r.range(0.9, 1.25);
        let variant = i % logs.len();
        let log = logs[variant].clone();
        spawn(
            &mut *commands,
            log,
            Transform {
                translation: Vec3::new(x, 0.0, z),
                rotation: Quat::from_rotation_y(yaw),
                scale: Vec3::splat(s),
            },
        );
        // Solid log — a long thin oriented box along its +X length axis (matching the mesh yaw).
        let log_len = if variant == 0 { 2.6 } else { 2.0 };
        crate::blockers::add_obb(x, z, log_len * 0.5 * s, 0.26 * s, yaw);
        // A short stump just off one end of the log (~70% of the time).
        if r.next() < 0.7 {
            let off = r.range(1.2, 1.7) * s;
            let sx = x + yaw.cos() * off;
            let sz = z - yaw.sin() * off;
            if !crate::water::on_river(sx, sz) {
                let ss = r.range(0.85, 1.2);
                spawn(
                    &mut *commands,
                    stump.clone(),
                    Transform {
                        translation: Vec3::new(sx, 0.0, sz),
                        rotation: Quat::from_rotation_y(r.range(0.0, TAU)),
                        scale: Vec3::splat(ss),
                    },
                );
                crate::blockers::add(sx, sz, 0.32 * ss); // solid stump base
            }
        }
        placed_logs += 1;
    }

    // A few extra lone stumps scattered about.
    let mut placed_stumps = 0;
    for _ in 0..30 {
        if placed_stumps >= 6 {
            break;
        }
        let x = r.range(lo + 1.5, hi - 1.5);
        let z = r.range(lo + 1.5, hi - 1.5);
        if crate::water::on_river(x, z) {
            continue;
        }
        let ss = r.range(0.8, 1.25);
        spawn(
            &mut *commands,
            stump.clone(),
            Transform {
                translation: Vec3::new(x, 0.0, z),
                rotation: Quat::from_rotation_y(r.range(0.0, TAU)),
                scale: Vec3::splat(ss),
            },
        );
        crate::blockers::add(x, z, 0.32 * ss); // solid stump base
        placed_stumps += 1;
    }

    // ── Berry bushes ──
    let mut placed_berries = 0;
    for i in 0..50 {
        if placed_berries >= 10 {
            break;
        }
        let x = r.range(lo + 1.5, hi - 1.5);
        let z = r.range(lo + 1.5, hi - 1.5);
        if crate::water::on_river(x, z) {
            continue;
        }
        spawn(
            &mut *commands,
            berries[i % berries.len()].clone(),
            Transform {
                translation: Vec3::new(x, 0.0, z),
                rotation: Quat::from_rotation_y(r.range(0.0, TAU)),
                scale: Vec3::splat(r.range(0.85, 1.25)),
            },
        );
        placed_berries += 1;
    }

    // ── Mushroom rings ──
    // A ring = a circle of small toadstools. Place a few ring CENTRES on open ground,
    // then dot the mushrooms around each.
    let mut placed_rings = 0;
    for _ in 0..30 {
        if placed_rings >= 4 {
            break;
        }
        let cx = r.range(lo + 3.0, hi - 3.0);
        let cz = r.range(lo + 3.0, hi - 3.0);
        if crate::water::on_river(cx, cz) {
            continue;
        }
        let ring_r = r.range(1.1, 1.7);
        let count = 7 + (r.next() * 4.0) as usize; // 7..10 mushrooms
        let a0 = r.range(0.0, TAU);
        let mut any_on_water = false;
        // Pre-check: if the ring straddles the river, skip it (don't half-drown a ring).
        for k in 0..count {
            let a = a0 + (k as f32 / count as f32) * TAU;
            let mx = cx + a.cos() * ring_r;
            let mz = cz + a.sin() * ring_r;
            if crate::water::on_river(mx, mz) {
                any_on_water = true;
                break;
            }
        }
        if any_on_water {
            continue;
        }
        for k in 0..count {
            let a = a0 + (k as f32 / count as f32) * TAU;
            // Jitter each mushroom slightly off the perfect circle so it reads natural.
            let rr = ring_r + r.range(-0.15, 0.15);
            let mx = cx + a.cos() * rr;
            let mz = cz + a.sin() * rr;
            spawn_cover(
                &mut *commands,
                ring_mushroom.clone(),
                Transform {
                    translation: Vec3::new(mx, 0.0, mz),
                    rotation: Quat::from_rotation_y(r.range(0.0, TAU)),
                    scale: Vec3::splat(r.range(0.85, 1.2)),
                },
            );
        }
        placed_rings += 1;
    }

    // ── Riverside reeds + lily pads ──
    // Sweep a grid; near the bank put reed clumps, on the water float lily pads.
    let mut gx = lo;
    while gx < hi {
        let mut gz = lo;
        while gz < hi {
            // Two samples per tile, jittered.
            for _ in 0..2 {
                let x = gx + r.next();
                let z = gz + r.next();
                let on_water = crate::water::on_river(x, z);
                if on_water {
                    // Float a lily pad on the river surface. ~18% of water samples so the
                    // channel reads dressed but not paved over.
                    if r.next() < 0.18 {
                        let flowered = r.next() < 0.3;
                        let mesh = if flowered { lily_flower.clone() } else { lily_plain.clone() };
                        spawn_cover(
                            &mut *commands,
                            mesh,
                            Transform {
                                // Sit just on the water surface (y≈0); tiny lift avoids
                                // z-fighting with the water plane.
                                translation: Vec3::new(x, 0.01, z),
                                rotation: Quat::from_rotation_y(r.range(0.0, TAU)),
                                scale: Vec3::splat(r.range(0.8, 1.25)),
                            },
                        );
                    }
                } else {
                    // Dry land — `river_bank_t` rises from 0 at the centerline to 1 a few
                    // units past the bank, so a SMALL positive value = the damp strip just
                    // past the waterline. Plant reed clumps there.
                    let bank = crate::water::river_bank_t(x, z);
                    if bank > 0.0 && bank < 0.55 && r.next() < 0.5 {
                        let v = (r.next() * reeds.len() as f32) as usize % reeds.len();
                        spawn_cover(
                            &mut *commands,
                            reeds[v].clone(),
                            Transform {
                                translation: Vec3::new(x, 0.0, z),
                                rotation: Quat::from_rotation_y(r.range(0.0, TAU)),
                                scale: Vec3::splat(r.range(0.85, 1.25)),
                            },
                        );
                    }
                }
            }
            gz += 1.0;
        }
        gx += 1.0;
    }

    // ── Fireflies / glow motes ──
    // Small emissive spheres hovering ~0.6–1.5u up, scattered over the patch. Each gets
    // its own emissive material (so the renderer still batches by colour, and bloom
    // picks the glow up). ~40 motes, gently clustered for a "swarm" feel.
    let firefly_mesh = meshes.add(Sphere::new(0.045).mesh().ico(1).expect("ico detail in range"));
    let firefly_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 1.0, 0.55),
        emissive: LinearRgba::from(FIREFLY_GLOW) * FIREFLY_EMISSIVE,
        unlit: true, // glow regardless of scene lighting → reliable bloom
        ..default()
    });

    // A few loose swarm centres so motes feel like they gather, not a uniform grid.
    let mut centres: Vec<Vec2> = Vec::new();
    for _ in 0..6 {
        centres.push(Vec2::new(r.range(lo + 2.0, hi - 2.0), r.range(lo + 2.0, hi - 2.0)));
    }
    let mut placed_flies = 0;
    for _ in 0..120 {
        if placed_flies >= 40 {
            break;
        }
        // 70% near a swarm centre, 30% free-roaming.
        let (bx, bz) = if r.next() < 0.7 {
            let c = centres[(r.next() * centres.len() as f32) as usize % centres.len()];
            (c.x + r.range(-2.2, 2.2), c.y + r.range(-2.2, 2.2))
        } else {
            (r.range(lo + 1.5, hi - 1.5), r.range(lo + 1.5, hi - 1.5))
        };
        // Keep them out of the river column so they don't hover over open water oddly.
        if crate::water::on_river(bx, bz) {
            continue;
        }
        let by = r.range(0.6, 1.5);
        commands.spawn((
            Mesh3d(firefly_mesh.clone()),
            MeshMaterial3d(firefly_mat.clone()),
            Transform::from_xyz(bx, by, bz),
            NotShadowCaster,
            BiomeEntity,
            Firefly {
                base: Vec3::new(bx, by, bz),
                phase: r.range(0.0, TAU),
                speed: r.range(0.6, 1.4),
                bob: r.range(0.12, 0.30),
                drift: r.range(0.15, 0.45),
            },
        ));
        placed_flies += 1;
    }
}

/// Gentle independent bob + drift on every firefly (CPU animation via `Res<Time>`),
/// pivoting around each mote's stored `base` so they wander but don't run away.
fn bob_fireflies(time: Res<Time>, mut q: Query<(&mut Transform, &Firefly)>) {
    let t = time.elapsed_secs();
    for (mut tf, f) in &mut q {
        let p = f.phase + t * f.speed;
        tf.translation.x = f.base.x + (p * 0.7).sin() * f.drift;
        tf.translation.z = f.base.z + (p * 0.9 + 1.3).cos() * f.drift;
        tf.translation.y = f.base.y + (p).sin() * f.bob;
    }
}

// ── Prop mesh builders ──────────────────────────────────────────────────────────

/// **Fallen log** — a long horizontal cylinder (lying on its side) in brown bark with a
/// green moss stripe along its top, lighter end-grain discs capping each end, plus a
/// couple of broken stub branches. Base flush at y=0 (the log rests on the ground, so
/// its centre is at y = radius). Two variants vary the length / moss coverage.
///
/// The log is authored along **+X** (so a scatter yaw rotates it freely about Y). The
/// Cylinder primitive is built upright (along Y) then rotated 90° about Z to lie down.
fn build_log_mesh(variant: u32) -> Mesh {
    let radius = 0.20;
    let length = if variant == 0 { 2.6 } else { 2.0 };
    // Lie the cylinder down: build along +Y, rotate -90° about Z → axis becomes +X,
    // then lift so its underside rests on y=0.
    let lay = |m: Mesh| -> Mesh {
        m.rotated_by(Quat::from_rotation_z(-FRAC_PI_2)).translated_by(y(radius))
    };

    // Body bark cylinder.
    let body = tinted(
        lay(Cylinder::new(radius, length).mesh().resolution(12).build()),
        lin(LOG_BARK),
    );
    // A darker faceted lump tucked under the log → reads as a shadowed underside.
    let under = ball_at(radius * 0.7, Vec3::new(0.0, radius * 0.35, 0.0), 0.7, LOG_BARK_DARK);

    // Moss stripe — a flattened green box riding the top of the log.
    let moss = tinted(
        Cuboid::new(length * 0.92, radius * 0.34, radius * 1.05)
            .mesh()
            .build()
            .translated_by(Vec3::new(0.0, radius * 1.62, 0.0)),
        lin(LOG_MOSS),
    );
    // A couple of small moss tufts (faceted blobs) on top for irregularity.
    let moss1 = ball_at(radius * 0.45, Vec3::new(length * 0.22, radius * 1.75, 0.04), 0.55, LOG_MOSS);
    let moss2 =
        ball_at(radius * 0.38, Vec3::new(-length * 0.18, radius * 1.72, -0.03), 0.55, LOG_MOSS);

    // Lighter end-grain discs (thin cylinders) capping both ends.
    let end_r = radius * 1.02;
    let end_a = tinted(
        Cylinder::new(end_r, 0.04)
            .mesh()
            .resolution(12)
            .build()
            .rotated_by(Quat::from_rotation_z(-FRAC_PI_2))
            .translated_by(Vec3::new(length * 0.5, radius, 0.0)),
        lin(LOG_END),
    );
    let end_b = tinted(
        Cylinder::new(end_r, 0.04)
            .mesh()
            .resolution(12)
            .build()
            .rotated_by(Quat::from_rotation_z(-FRAC_PI_2))
            .translated_by(Vec3::new(-length * 0.5, radius, 0.0)),
        lin(LOG_END),
    );

    // A short broken stub branch poking up from the body (variant 0 only — keeps the
    // two logs visually distinct).
    let mut parts = vec![body, under, moss, moss1, moss2, end_a, end_b];
    if variant == 0 {
        let stub = tinted(
            Cylinder::new(0.05, 0.34)
                .mesh()
                .resolution(6)
                .build()
                .rotated_by(Quat::from_rotation_x(0.5))
                .translated_by(Vec3::new(length * 0.1, radius + 0.16, 0.10)),
            lin(LOG_BARK),
        );
        parts.push(stub);
    }

    flat_shaded(merged(parts))
}

/// **Tree stump** — a short wide cylinder of bark with a lighter cut top and a couple
/// of pale concentric growth-ring discs on the cut face, plus a small root flare. Base
/// at y=0. ~0.3u tall, ~0.45u wide.
fn build_stump_mesh() -> Mesh {
    let r = 0.34;
    let h = 0.30;
    let parts = vec![
        // Bark drum.
        cyl_up(r, h, h * 0.5, 12, STUMP_BARK),
        // Lighter cut-top wood (thin disc sitting flush on the rim).
        cyl_up(r * 0.97, 0.05, h + 0.005, 12, STUMP_TOP),
        // Pale growth-ring highlight (a slightly smaller, slightly higher thin disc).
        cyl_up(r * 0.62, 0.035, h + 0.03, 12, STUMP_RING),
        cyl_up(r * 0.30, 0.03, h + 0.045, 10, STUMP_TOP),
        // A small root flare — a squashed faceted lump at the base on one side.
        ball_at(r * 0.55, Vec3::new(r * 0.8, h * 0.2, 0.0), 0.5, STUMP_BARK),
        ball_at(r * 0.45, Vec3::new(-r * 0.6, h * 0.18, r * 0.5), 0.5, STUMP_BARK),
    ];
    flat_shaded(merged(parts))
}

/// **Berry bush** — a small rounded leaf blob (three green tiers, like props.rs bushes
/// but smaller) studded with tiny red and blue berry balls. Base at y=0. Two variants
/// emphasise red vs blue berries.
fn build_berry_bush_mesh(variant: u32) -> Mesh {
    let mut parts = vec![
        // Dark skirt.
        ball_at(0.22, y(0.15), 0.82, BERRY_LEAF_DARK),
        ball_at(0.17, Vec3::new(0.17, 0.13, 0.04), 0.82, BERRY_LEAF_DARK),
        ball_at(0.15, Vec3::new(-0.15, 0.12, 0.08), 0.82, BERRY_LEAF_DARK),
        // Mid body.
        ball_at(0.18, y(0.26), 0.86, BERRY_LEAF_MID),
        ball_at(0.14, Vec3::new(0.11, 0.29, -0.11), 0.86, BERRY_LEAF_MID),
        // Bright crown.
        ball_at(0.14, y(0.36), 0.9, BERRY_LEAF_LIGHT),
        ball_at(0.10, Vec3::new(-0.08, 0.39, 0.06), 0.9, BERRY_LEAF_LIGHT),
    ];

    // Berries — tiny faceted balls nestled in the foliage. Variant biases the mix.
    let (n_red, n_blue) = if variant == 0 { (7, 3) } else { (3, 7) };
    // Deterministic placement: walk a golden-angle spiral over the bush surface so the
    // berries spread without an RNG (the mesh is built once, shared by all instances).
    let ga = 2.399_963_2_f32; // golden angle
    let place_berry = |idx: usize, total: usize, parts: &mut Vec<Mesh>, c: u32| {
        let t = (idx as f32 + 0.5) / total as f32;
        let a = idx as f32 * ga;
        let rr = 0.20 * (1.0 - t * 0.4);
        let bx = a.cos() * rr;
        let bz = a.sin() * rr;
        let by = 0.18 + t * 0.20;
        parts.push(ball_at(0.035, Vec3::new(bx, by, bz), 1.0, c));
    };
    for i in 0..n_red {
        place_berry(i, n_red, &mut parts, BERRY_RED);
    }
    for i in 0..n_blue {
        place_berry(i + 1, n_blue, &mut parts, BERRY_BLUE);
    }

    flat_shaded(merged(parts))
}

/// **Reed / cattail clump** — a fan of tall thin stalks (cones) leaning slightly out,
/// some topped with a brown cattail seed head (a small capsule/cylinder). Base at y=0,
/// ~0.7–0.9u tall so it reads against the water. Two variants vary the count / height.
fn build_reed_clump_mesh(variant: u32) -> Mesh {
    let count = if variant == 0 { 6 } else { 8 };
    let mut parts: Vec<Mesh> = Vec::new();
    for i in 0..count {
        let a = (i as f32 / count as f32) * TAU;
        // Spread the stalk bases over a small footprint.
        let foot = 0.10;
        let bx = a.cos() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let bz = a.sin() * foot * (0.4 + (i % 3) as f32 * 0.3);
        let h = 0.62 + ((i * 7) % 5) as f32 * 0.07 + variant as f32 * 0.08;
        let tilt = 0.06 + (i % 4) as f32 * 0.04;
        // Stalk: a slender flat-shaded cone leaning out from the base. Build upright at
        // the origin, lean it (Z then Y so `rotated_by` composes to `Ry*Rz`), then shift
        // onto this stalk's footprint `(bx,bz)`.
        let foot_off = Vec3::new(bx, 0.0, bz);
        let lean = Quat::from_rotation_y(a) * Quat::from_rotation_z(tilt);
        let stalk_c = if i % 2 == 0 { REED_STALK } else { REED_STALK_TIP };
        let stalk = Cone { radius: 0.022, height: h }
            .mesh()
            .build()
            .translated_by(y(h / 2.0))
            .rotated_by(Quat::from_rotation_z(tilt))
            .rotated_by(Quat::from_rotation_y(a))
            .translated_by(foot_off);
        parts.push(tinted(stalk, lin(stalk_c)));

        // Roughly half the stalks carry a brown cattail seed head near the tip — at the
        // same leaned-out tip, aligned to the stalk, sharing the footprint offset.
        if i % 2 == 0 {
            let tip = lean * Vec3::new(0.0, h * 0.86, 0.0) + foot_off;
            let head = Cylinder::new(0.035, 0.16)
                .mesh()
                .resolution(7)
                .build()
                .rotated_by(lean) // align the head with the leaning stalk
                .translated_by(tip);
            parts.push(tinted(head, lin(REED_HEAD)));
        }
    }
    flat_shaded(merged(parts))
}

/// **Lily pad** — a flat green disc floating on the water surface, with a darker rim
/// ring and a small radial notch suggestion via a lighter centre; `flowered` adds a
/// tiny pink bloom (petal ring + pale core) sitting just above the pad. Built at y≈0
/// (the pad is essentially flat); the scatter sits it on the water surface.
///
/// The `Circle` mesh lies in the XY plane (normal +Z), so each disc is rotated −90°
/// about X to lie flat on the XZ ground/water plane.
fn build_lily_pad_mesh(flowered: bool) -> Mesh {
    let flat = |m: Mesh| -> Mesh { m.rotated_by(Quat::from_rotation_x(-FRAC_PI_2)) };

    let pad_r = 0.26;
    let mut parts = vec![
        // Darker underside / rim ring (slightly larger, a hair lower).
        tinted(
            flat(Circle::new(pad_r).mesh().resolution(14).build()).translated_by(y(0.004)),
            lin(LILY_PAD_EDGE),
        ),
        // Green pad top (slightly smaller so the darker rim peeks out).
        tinted(
            flat(Circle::new(pad_r * 0.9).mesh().resolution(14).build()).translated_by(y(0.012)),
            lin(LILY_PAD),
        ),
    ];

    if flowered {
        // Pale yellow core (tiny squashed ball) + a ring of pink petals just above pad.
        parts.push(ball_at(0.03, y(0.055), 0.7, LILY_FLOWER_CORE));
        for i in 0..6 {
            let a = (i as f32 / 6.0) * TAU;
            parts.push(ball_at(
                0.035,
                Vec3::new(a.cos() * 0.05, 0.05, a.sin() * 0.05),
                0.5,
                LILY_FLOWER,
            ));
        }
    }

    // Flat-shade the flower blobs; the pad discs already have flat normals, and
    // duplicate_vertices()/compute_flat_normals() over the merged mesh is harmless.
    flat_shaded(merged(parts))
}

/// **Ring mushroom** — a single small toadstool used to build mushroom rings: a pale
/// stem + a domed warm-brown cap. Base at y=0, ~0.16u tall. (Kept distinct from the
/// groundcover amanita so the rings read as their own feature.)
fn build_ring_mushroom_mesh() -> Mesh {
    let stem_h = 0.10;
    let cap_r = 0.075;
    let parts = vec![
        // Pale slender stem.
        cyl_up(0.026, stem_h, stem_h * 0.5, 6, RING_MUSH_STEM),
        // Domed cap (squashed faceted ball) resting on the stem.
        ball_at(cap_r, y(stem_h), 0.6, RING_MUSH_CAP),
    ];
    flat_shaded(merged(parts))
}
