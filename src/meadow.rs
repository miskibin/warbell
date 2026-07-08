//! **Castle meadow dressing** — the ring of open grass between the castle walls and the
//! safe-zone treeline (world r ≈ 17..32) used to be a bald, empty clearing. This module fills
//! it with three layers, per the 2026-07 polish pass:
//!
//! * **village life** — hay cart + bales + scarecrow (NE quarter), beehives + firewood rack
//!   (west), stacked crates/barrels by the east wall, short fence runs;
//! * **nature** — tree clumps / bushes / boulders breaking the sightlines (diagonal quarters
//!   only — the four cardinal gate lanes stay open for the night waves + villager paths);
//! * **a rest spot** — a campfire with log stools on the forest side (by the new hero spawn):
//!   standing near it in **Prep** slowly heals the hero ([`rest_by_fire`]).
//!
//! Placement rules honoured (see `worldmap::classify` + `town.rs`): everything keeps ≥3.6 from
//! the 12 build-plot centres (`PLOT_CLEAR_R` 3.4), off the cardinal lanes (|x| or |z| < 3.5
//! near the gates), and inside the grass safe ring (r < 32). Chunky props register
//! [`crate::blockers`] boxes so hero/orks/villagers route around them; thin fences don't (the
//! castle_decor MIN_SOLID reasoning). Trees use their real silhouette footprint.
//!
//! Spawned from `worldmap::build_step` (phase 29), everything tagged [`BiomeEntity`], so the
//! biome-swap despawn/rebuild cycle owns the lifetime — nothing here is run-state (the campfire
//! heal is stateless), so there are no save/reset obligations.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::mesh::MeshBuilder;
use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::palette::lin;
use crate::meshkit::{merged_flat as merged, tinted};

/// Radius around the campfire within which the hero rests (heals) during Prep.
const REST_R: f32 = 3.2;
/// Rest heal per second — deliberately gentle (the shrine/potions stay the real heals).
const REST_HPS: f64 = 3.5;
/// The campfire's world spot — forest side, next to the hero's new spawn (see
/// `player::spawn_point`), clear of plot (-18,17) and the west gate lane.
const FIRE_POS: Vec2 = Vec2::new(-22.0, 14.0);

pub struct MeadowPlugin;

impl Plugin for MeadowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            rest_by_fire.run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}

/// Marks the rest-campfire anchor (queried by [`rest_by_fire`]).
#[derive(Component)]
pub struct RestFire;

/// Gentle out-of-siege heal while the hero warms himself at the meadow campfire. Prep-only —
/// resting mid-wave would trivialise the siege — and it never overheals past max.
fn rest_by_fire(
    time: Res<Time>,
    siege: Option<Res<crate::siege::Siege>>,
    fire_q: Query<&GlobalTransform, With<RestFire>>,
    hero_q: Query<&crate::player::Hero>,
    mut player: ResMut<crate::player::PlayerRes>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut tick: Local<f32>,
) {
    if !siege.is_some_and(|s| s.phase == crate::siege::GamePhase::Prep) {
        return;
    }
    let Ok(hero) = hero_q.single() else { return };
    let Some(fire) = fire_q.iter().next() else { return };
    let fp = fire.translation();
    if hero.pos.distance(Vec2::new(fp.x, fp.z)) > REST_R {
        return;
    }
    if player.0.hp >= player.0.max_hp {
        return;
    }
    player.0.heal(REST_HPS * time.delta_secs() as f64);
    // A soft "+" float every couple seconds so the heal is legible without spamming.
    *tick += time.delta_secs();
    if *tick >= 2.0 {
        *tick = 0.0;
        floats.0.push(crate::combat_fx::FloatReq {
            world: Vec3::new(hero.pos.x, hero.y + 2.3, hero.pos.y),
            text: "+".into(),
            color: Color::srgb(0.55, 0.95, 0.55),
            scale: 0.9,
        });
    }
}

// ── Build (called from `worldmap::build_step` phase 29) ─────────────────────────────

/// Dress the meadow. `std_mats` gets one shared white vertex-colour material (auto-batching,
/// the props.rs contract) plus the emissive flame.
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    let ground = |x: f32, z: f32| crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);

    // Spawn one merged prop; `solid` (hw, hd) registers a blocker box (Vec2::ZERO = walk-through).
    let mut prop = |commands: &mut Commands,
                    meshes: &mut Assets<Mesh>,
                    mesh: Mesh,
                    x: f32,
                    z: f32,
                    yaw: f32,
                    solid: Vec2| {
        if solid.x > 0.0 {
            crate::blockers::add_obb(x, z, solid.x, solid.y, yaw);
        }
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, ground(x, z), z).with_rotation(Quat::from_rotation_y(yaw)),
            BiomeEntity,
        ));
    };

    // ── Village life ─────────────────────────────────────────────────────────────
    // Hay-making corner, NE quarter (clear of plots (10,-15.5)/(18,-17) by ≥5).
    prop(commands, meshes, hay_cart_mesh(), 13.0, -20.5, 0.55, Vec2::new(1.1, 0.7));
    prop(commands, meshes, hay_bale_mesh(0), 15.6, -21.8, 0.2, Vec2::new(0.55, 0.4));
    prop(commands, meshes, hay_bale_mesh(1), 14.6, -23.0, 1.1, Vec2::new(0.55, 0.4));
    prop(commands, meshes, scarecrow_mesh(), 17.5, -23.5, -0.4, Vec2::ZERO);
    fence_run(commands, meshes, &mat, Vec2::new(10.5, -18.5), Vec2::new(16.5, -18.0));

    // Beekeeper's west edge (clear of plots (-20,-8)/(-18,-17)).
    for (i, (bx_, bz)) in [(-21.0, -13.0), (-22.4, -12.2), (-21.6, -14.6)].into_iter().enumerate() {
        prop(commands, meshes, beehive_mesh(i as u32), bx_, bz, i as f32 * 0.7, Vec2::new(0.3, 0.3));
    }
    prop(commands, meshes, firewood_rack_mesh(), -24.5, -11.5, FRAC_PI_2 * 0.85, Vec2::new(0.9, 0.35));

    // Supply drop-off by the east wall (clear of plot (20,8), off the z=0 lane).
    prop(commands, meshes, crate_stack_mesh(), 21.0, 4.6, -0.3, Vec2::new(0.75, 0.55));

    // ── The rest campfire, forest side (next to the hero spawn) ────────────────────
    let fy = ground(FIRE_POS.x, FIRE_POS.y);
    prop(commands, meshes, fire_base_mesh(), FIRE_POS.x, FIRE_POS.y, 0.0, Vec2::new(0.45, 0.45));
    for i in 0..3 {
        let a = 0.6 + i as f32 / 3.0 * TAU * 0.6;
        let (sx, sz) = (FIRE_POS.x + a.cos() * 1.15, FIRE_POS.y + a.sin() * 1.15);
        prop(commands, meshes, sit_stump_mesh(i * 37 + 5), sx, sz, a, Vec2::ZERO);
    }
    // Emissive flame + pooled flicker light + the audio anchor (`camps::Flicker` carries the
    // spatial campfire-crackle loop) + the rest marker.
    let flame_mat = materials.add(StandardMaterial {
        base_color: crate::palette::srgb(0xff8a30),
        emissive: crate::palette::srgb(0xff8a30).to_linear() * 4.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(flame_mesh())),
        MeshMaterial3d(flame_mat),
        Transform::from_translation(Vec3::new(FIRE_POS.x, fy + 0.22, FIRE_POS.y)),
        crate::camps::Flicker { phase: 0.35 },
        RestFire,
        BiomeEntity,
        crate::firelight::campfire_light(0.35),
    ));

    // ── Nature clumps (diagonal quarters; cardinal lanes stay open) ────────────────
    let mut tree = |commands: &mut Commands, meshes: &mut Assets<Mesh>, kind, x: f32, z: f32, s: f32, yaw: f32| {
        let m = crate::trees::build_tree_mesh(kind);
        let r = crate::trees::silhouette_block_radius(&m) * s;
        if r > 0.15 {
            crate::blockers::add_box(x, z, r, r);
        }
        commands.spawn((
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(mat.clone()),
            Transform::from_xyz(x, ground(x, z), z)
                .with_rotation(Quat::from_rotation_y(yaw))
                .with_scale(Vec3::splat(s)),
            BiomeEntity,
        ));
    };
    use crate::trees::TreeKind as TK;
    // NE corner clump.
    tree(commands, meshes, TK::Broadleaf, 24.0, -24.0, 1.15, 0.4);
    tree(commands, meshes, TK::Birch, 26.5, -21.5, 1.0, 2.1);
    prop(commands, meshes, boulder_mesh(11), 22.3, -21.9, 0.9, Vec2::new(0.55, 0.5));
    // NW pine corner.
    tree(commands, meshes, TK::Pine, -26.0, -18.0, 1.1, 1.2);
    prop(commands, meshes, boulder_mesh(23), -24.0, -20.3, 2.3, Vec2::new(0.5, 0.45));
    // SE autumn accent.
    tree(commands, meshes, TK::Autumn, 26.0, 18.0, 1.1, 3.6);
    prop(commands, meshes, bush_mesh(7), 23.4, 20.2, 0.0, Vec2::ZERO);
    // Forest-side birches framing the spawn walk-in.
    tree(commands, meshes, TK::Birch, -29.0, 21.5, 1.05, 0.9);
    tree(commands, meshes, TK::Broadleaf, -26.0, 25.5, 1.1, 4.2);
    prop(commands, meshes, bush_mesh(19), -24.6, 18.4, 0.6, Vec2::ZERO);
    // Loose boulders + bushes breaking the remaining sightlines (all off-lane, off-plot).
    prop(commands, meshes, boulder_mesh(41), 8.0, 21.0, 1.4, Vec2::new(0.5, 0.45));
    prop(commands, meshes, bush_mesh(31), 6.8, 22.6, 0.0, Vec2::ZERO);
    prop(commands, meshes, boulder_mesh(57), -7.0, -23.0, 0.4, Vec2::new(0.45, 0.4));
    prop(commands, meshes, bush_mesh(43), -9.0, -21.4, 1.8, Vec2::ZERO);
}

/// A short two-rail paddock fence between `a` and `b` (posts every ~1.5). Decorative-only —
/// registering blocker boxes across the meadow just walls off lanes for no gameplay payoff
/// (the castle_decor "thin props snag the hero" lesson).
fn fence_run(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    a: Vec2,
    b: Vec2,
) {
    const WOOD: u32 = 0x6b4a2a;
    const DARK: u32 = 0x54381f;
    let d = b - a;
    let len = d.length();
    let yaw = d.x.atan2(d.y); // mesh runs along +Z; yaw aligns it onto the a→b bearing
    let mut parts: Vec<Mesh> = Vec::new();
    let posts = (len / 1.5).ceil() as i32 + 1;
    for i in 0..posts {
        let t = i as f32 / (posts - 1) as f32;
        parts.push(bx(0.09, 0.85, 0.09, Vec3::new(0.0, 0.42, t * len), DARK));
    }
    parts.push(bx(0.055, 0.07, len, Vec3::new(0.0, 0.62, len * 0.5), WOOD));
    parts.push(bx(0.055, 0.07, len, Vec3::new(0.0, 0.34, len * 0.5), WOOD));
    let y = crate::worldmap::ground_at_world(a.x, a.y).unwrap_or(0.0);
    commands.spawn((
        Mesh3d(meshes.add(merged(parts))),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(a.x, y, a.y).with_rotation(Quat::from_rotation_y(yaw)),
        BiomeEntity,
    ));
}

// ── Prop meshes (feet at y≈0; vertex colours; the props.rs contract) ────────────────

const WOOD: u32 = 0x6b4a2a;
const WOOD_DARK: u32 = 0x54381f;
const STRAW: u32 = 0xcdae5e;
const STRAW_DARK: u32 = 0xb08e46;
const STONE: u32 = 0x8d8d86;
const STONE_DARK: u32 = 0x6f6f68;
const LEAF: u32 = 0x4f7a34;
const LEAF_DARK: u32 = 0x3c6128;

/// Two-wheel hay cart, shafts resting on the ground, heaped straw load.
fn hay_cart_mesh() -> Mesh {
    let mut p = vec![
        bx(1.7, 0.1, 1.0, Vec3::new(0.0, 0.62, 0.0), WOOD),          // bed
        bx(1.7, 0.28, 0.08, Vec3::new(0.0, 0.8, 0.5), WOOD_DARK),    // side rails
        bx(1.7, 0.28, 0.08, Vec3::new(0.0, 0.8, -0.5), WOOD_DARK),
        // heaped load
        bx(1.35, 0.4, 0.8, Vec3::new(0.0, 0.95, 0.0), STRAW),
        bx(0.95, 0.3, 0.55, Vec3::new(-0.05, 1.25, 0.0), STRAW_DARK),
        // shafts tipped to the ground
        bxr(0.06, 0.06, 1.1, Vec3::new(0.55, 0.3, 0.95), Quat::from_rotation_x(0.5), WOOD_DARK),
        bxr(0.06, 0.06, 1.1, Vec3::new(-0.55, 0.3, 0.95), Quat::from_rotation_x(0.5), WOOD_DARK),
    ];
    // Wheels: chunky 8-gon cylinders on their sides.
    for sx in [-0.88, 0.88] {
        p.push(tinted(
            Cylinder::new(0.42, 0.09)
                .mesh()
                .resolution(8)
                .build()
                .rotated_by(Quat::from_rotation_z(FRAC_PI_2))
                .translated_by(Vec3::new(sx, 0.42, 0.0)),
            lin(WOOD_DARK),
        ));
    }
    merged(p)
}

/// A round hay bale (lying cylinder) with a strap; `seed` jitters the size.
fn hay_bale_mesh(seed: u32) -> Mesh {
    let s = 1.0 + (seed % 3) as f32 * 0.12;
    merged(vec![
        tinted(
            Cylinder::new(0.42 * s, 0.75 * s)
                .mesh()
                .resolution(9)
                .build()
                .rotated_by(Quat::from_rotation_z(FRAC_PI_2))
                .translated_by(Vec3::new(0.0, 0.42 * s, 0.0)),
            lin(STRAW),
        ),
        tinted(
            Cylinder::new(0.43 * s, 0.1)
                .mesh()
                .resolution(9)
                .build()
                .rotated_by(Quat::from_rotation_z(FRAC_PI_2))
                .translated_by(Vec3::new(0.0, 0.42 * s, 0.0)),
            lin(STRAW_DARK),
        ),
    ])
}

/// Cross-pole scarecrow in a ragged coat + straw hat, guarding the hay corner.
fn scarecrow_mesh() -> Mesh {
    const COAT: u32 = 0x7a5a40;
    const HAT: u32 = 0xc2a35a;
    merged(vec![
        bx(0.09, 1.7, 0.09, Vec3::new(0.0, 0.85, 0.0), WOOD_DARK), // post
        bx(1.1, 0.08, 0.08, Vec3::new(0.0, 1.32, 0.0), WOOD),      // arms
        bx(0.5, 0.62, 0.3, Vec3::new(0.0, 1.05, 0.0), COAT),       // coat body
        bx(0.42, 0.1, 0.26, Vec3::new(0.0, 0.72, 0.0), STRAW),     // straw hem
        bx(0.24, 0.24, 0.24, Vec3::new(0.0, 1.5, 0.0), STRAW_DARK), // sack head
        cone_at(0.3, 0.22, Vec3::new(0.0, 1.68, 0.0), Quat::IDENTITY, HAT), // hat
    ])
}

/// A skep-style beehive: stacked tapering straw rings on a little stand.
fn beehive_mesh(seed: u32) -> Mesh {
    let s = 1.0 + (seed % 2) as f32 * 0.15;
    merged(vec![
        bx(0.5, 0.1, 0.5, Vec3::new(0.0, 0.05, 0.0), WOOD_DARK),
        tinted(Cylinder::new(0.30 * s, 0.16).mesh().resolution(8).build().translated_by(Vec3::new(0.0, 0.18, 0.0)), lin(STRAW)),
        tinted(Cylinder::new(0.27 * s, 0.16).mesh().resolution(8).build().translated_by(Vec3::new(0.0, 0.34, 0.0)), lin(STRAW_DARK)),
        tinted(Cylinder::new(0.22 * s, 0.14).mesh().resolution(8).build().translated_by(Vec3::new(0.0, 0.48, 0.0)), lin(STRAW)),
        tinted(Cylinder::new(0.13 * s, 0.1).mesh().resolution(8).build().translated_by(Vec3::new(0.0, 0.58, 0.0)), lin(STRAW_DARK)),
        // entrance notch
        bx(0.1, 0.07, 0.06, Vec3::new(0.0, 0.14, 0.3 * s), WOOD_DARK),
    ])
}

/// A-frame firewood rack, split logs stacked between the posts.
fn firewood_rack_mesh() -> Mesh {
    let mut p = vec![
        bxr(0.08, 1.0, 0.08, Vec3::new(-0.75, 0.45, 0.18), Quat::from_rotation_x(0.35), WOOD_DARK),
        bxr(0.08, 1.0, 0.08, Vec3::new(-0.75, 0.45, -0.18), Quat::from_rotation_x(-0.35), WOOD_DARK),
        bxr(0.08, 1.0, 0.08, Vec3::new(0.75, 0.45, 0.18), Quat::from_rotation_x(0.35), WOOD_DARK),
        bxr(0.08, 1.0, 0.08, Vec3::new(0.75, 0.45, -0.18), Quat::from_rotation_x(-0.35), WOOD_DARK),
    ];
    // stacked logs (staggered rows)
    let mut seed = 9u32;
    for row in 0..4 {
        let y = 0.16 + row as f32 * 0.15;
        let n = 5 - (row % 2);
        for i in 0..n {
            let x = -0.55 + i as f32 * (1.1 / (n - 1) as f32) + rngf(&mut seed) * 0.04;
            let c = if (i + row) % 2 == 0 { WOOD } else { WOOD_DARK };
            p.push(tinted(
                Cylinder::new(0.075, 1.0 + rngf(&mut seed) * 0.15)
                    .mesh()
                    .resolution(6)
                    .build()
                    .rotated_by(Quat::from_rotation_x(FRAC_PI_2))
                    .translated_by(Vec3::new(x, y, 0.0)),
                lin(c),
            ));
        }
    }
    merged(p)
}

/// Crates + a hooped barrel under a tarp corner — a supply drop-off by the east wall.
fn crate_stack_mesh() -> Mesh {
    const TARP: u32 = 0x8a7a5c;
    const IRON: u32 = 0x4a4a4a;
    merged(vec![
        bx(0.62, 0.62, 0.62, Vec3::new(-0.35, 0.31, 0.1), WOOD),
        bx(0.5, 0.5, 0.5, Vec3::new(0.3, 0.25, -0.25), WOOD_DARK),
        bx(0.44, 0.44, 0.44, Vec3::new(-0.3, 0.84, 0.05), WOOD_DARK),
        // barrel
        tinted(Cylinder::new(0.26, 0.62).mesh().resolution(9).build().translated_by(Vec3::new(0.45, 0.31, 0.42)), lin(WOOD)),
        tinted(Cylinder::new(0.27, 0.06).mesh().resolution(9).build().translated_by(Vec3::new(0.45, 0.15, 0.42)), lin(IRON)),
        tinted(Cylinder::new(0.27, 0.06).mesh().resolution(9).build().translated_by(Vec3::new(0.45, 0.5, 0.42)), lin(IRON)),
        // tarp draped over the tall crate corner
        bxr(0.8, 0.05, 0.75, Vec3::new(-0.32, 1.1, 0.05), Quat::from_rotation_z(0.12), TARP),
    ])
}

/// A low two-tone boulder; `seed` varies proportions/yaw asymmetry.
fn boulder_mesh(mut seed: u32) -> Mesh {
    let s = 0.8 + rngf(&mut seed) * 0.5;
    merged(vec![
        tinted(
            Sphere::new(0.5 * s).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.2, 0.72, 1.0)).translated_by(Vec3::new(0.0, 0.3 * s, 0.0)),
            lin(STONE),
        ),
        tinted(
            Sphere::new(0.3 * s).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.0, 0.7, 1.15)).translated_by(Vec3::new(0.45 * s, 0.18 * s, 0.25 * s)),
            lin(STONE_DARK),
        ),
    ])
}

/// A squat leafy bush cluster (two-tone lumps).
fn bush_mesh(mut seed: u32) -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    let n = 3 + (seed % 2);
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU + rngf(&mut seed);
        let r = 0.35 + rngf(&mut seed) * 0.25;
        let c = if i % 2 == 0 { LEAF } else { LEAF_DARK };
        p.push(tinted(
            Sphere::new(r).mesh().ico(1).unwrap().scaled_by(Vec3::new(1.0, 0.78, 1.0)).translated_by(Vec3::new(
                a.cos() * 0.3,
                r * 0.62,
                a.sin() * 0.3,
            )),
            lin(c),
        ));
    }
    merged(p)
}

/// Campfire base — stone ring + crossed logs (the camps.rs recipe, meadow-sized).
fn fire_base_mesh() -> Mesh {
    let mut p: Vec<Mesh> = Vec::new();
    for i in 0..7 {
        let a = (i as f32 / 7.0) * TAU;
        p.push(tinted(
            Sphere::new(0.13).mesh().ico(0).unwrap().translated_by(Vec3::new(a.cos() * 0.42, 0.07, a.sin() * 0.42)),
            lin(STONE),
        ));
    }
    p.push(tinted(
        Cylinder::new(0.045, 0.72).mesh().resolution(6).build().rotated_by(Quat::from_euler(EulerRot::XYZ, FRAC_PI_2, 0.0, 0.6)).translated_by(Vec3::new(0.0, 0.08, 0.0)),
        lin(WOOD),
    ));
    p.push(tinted(
        Cylinder::new(0.045, 0.72).mesh().resolution(6).build().rotated_by(Quat::from_euler(EulerRot::XYZ, FRAC_PI_2, 0.0, -0.6)).translated_by(Vec3::new(0.0, 0.13, 0.0)),
        lin(WOOD_DARK),
    ));
    merged(p)
}

/// Log stool ringing the campfire (the camps.rs sit-stump recipe).
fn sit_stump_mesh(mut seed: u32) -> Mesh {
    let h = 0.36 + rngf(&mut seed) * 0.11;
    merged(vec![
        tinted(Cylinder::new(0.25, h).mesh().resolution(7).build().translated_by(Vec3::new(0.0, h * 0.5, 0.0)), lin(WOOD_DARK)),
        tinted(Cylinder::new(0.30, 0.09).mesh().resolution(7).build().translated_by(Vec3::new(0.0, 0.045, 0.0)), lin(WOOD_DARK)),
        tinted(Cylinder::new(0.235, 0.05).mesh().resolution(7).build().translated_by(Vec3::new(0.0, h, 0.0)), lin(WOOD)),
    ])
}

/// Flame cones (untinted — the emissive material colours them), like camps.rs.
fn flame_mesh() -> Mesh {
    let outer = Cone { radius: 0.17, height: 0.55 }.mesh().build().translated_by(Vec3::new(0.0, 0.27, 0.0));
    let inner = Cone { radius: 0.09, height: 0.35 }.mesh().build().translated_by(Vec3::new(0.0, 0.2, 0.0));
    let mut m = outer;
    m.merge(&inner).expect("cones share attributes");
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

// ── tiny local mesh helpers (the props.rs contract) ────────────────────────────────

fn bx(w: f32, h: f32, d: f32, off: Vec3, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().translated_by(off), lin(c))
}
fn bxr(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), lin(c))
}
fn cone_at(r: f32, h: f32, off: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().build().rotated_by(rot).translated_by(off), lin(c))
}
/// mulberry32-ish jitter, [0,1).
fn rngf(s: &mut u32) -> f32 {
    *s = s.wrapping_add(0x6d2b_79f5);
    let mut t = *s;
    t = (t ^ (t >> 15)).wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    (t ^ (t >> 14)) as f32 / 4_294_967_296.0
}
