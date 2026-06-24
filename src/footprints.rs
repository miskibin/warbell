//! **Hero footprints** — a flat ground decal stamped under each footfall, so the hero leaves a
//! trail of tracks behind him. The visual companion to [`footstep_fx`](crate::footstep_fx)'s dust
//! puff: the *same* gait half-cycle gate fires both, but where the puff is a quick mote burst that
//! settles in a beat, a print is a soaked-in decal that lingers (long in pressed snow, briefly on
//! wind-scoured sand) and fades on its own.
//!
//! Surface-aware via [`worldmap::biome_at_world`] (the same branch `footstep_fx::surf_mat` uses):
//! snow takes deep pale prints that the ambient snowfall visually "refills" as they fade; swamp a
//! dark muddy churn; desert shallow tan; forest/grass a faint scuff; bare rock takes none. Left and
//! right feet alternate off the half-cycle parity, each offset to the side of the heading and yawed
//! to point along it.
//!
//! Bounded like [`aftermath`](crate::aftermath): a FIFO cap reaps the oldest print so a long run
//! can't litter the map, and each print owns its own alpha-blended material (freed on despawn) so
//! it can fade solo. Tagged `BiomeEntity`, so a biome swap / world rebuild wipes the trail like any
//! other dressing. Pure transient — not saved.

use std::collections::VecDeque;
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::biome::{Biome, BiomeEntity};
use crate::game_state::Modal;
use crate::player::Hero;
use crate::worldmap;

/// Most prints alive at once (oldest reaped first) — long enough for a believable trailing few
/// strides, bounded so a marathon run never piles up thousands of decals.
const MAX_PRINTS: usize = 56;

/// Lateral offset of each print from the hero's centre line (world units), so left/right tracks
/// read as two separate boot rows rather than one centred smear.
const FOOT_OFFSET: f32 = 0.13;

/// Positive depth bias so the flat decal renders in front of whatever ground/cobble it's stamped on
/// instead of z-fighting it (same fix as `aftermath`'s ground decals vs. the courtyard slab).
const DECAL_DEPTH_BIAS: f32 = 50.0;

/// Per-surface print look: linear-ish RGB tint, peak alpha, plan size, and how long it holds at
/// full before fading + the fade length (s). `None` surfaces (bare rock) take no print.
struct Surface {
    color: Color,
    alpha: f32,
    /// Scale applied to the unit boot mesh.
    size: f32,
    hold: f32,
    fade: f32,
}

/// Pick the print look for the biome under `pos`, or `None` where the ground is too hard/elastic to
/// hold a track (bare rock). Mirrors `footstep_fx::surf_mat`'s biome branch.
fn surface_for(pos: Vec2) -> Option<Surface> {
    // Kept deliberately SUBTLE — a faint pressing, not a stamp. Snow/sand read most because of
    // their contrast; the grass/mud scuffs are barely-there earth tones.
    match worldmap::biome_at_world(pos.x, pos.y) {
        // Pressed snow: pale + long-lived — the falling-snow weather field visually fills the trail
        // back in as it fades.
        Some(Biome::Snow) => Some(snow_look()),
        // Swamp mud: dark, wet, a churned print that sticks around a while.
        Some(Biome::Swamp) => Some(swamp_look()),
        // Desert sand: pale tan, shallow, wind-scoured away fast.
        Some(Biome::Desert) => Some(desert_look()),
        // Bare rock holds no print.
        Some(Biome::Rocky) => None,
        // Forest / grassy castle ground (None = the grass safe-zone): a faint earthy scuff.
        Some(Biome::Forest) | None => Some(dirt_look()),
    }
}

// Per-surface looks, shared by `surface_for` and the `FOREST_FEETTEST` preview so the test row
// matches the real in-world prints exactly.
fn snow_look() -> Surface { Surface { color: Color::srgb(0.74, 0.80, 0.90), alpha: 0.42, size: 1.0, hold: 20.0, fade: 15.0 } }
fn swamp_look() -> Surface { Surface { color: Color::srgb(0.17, 0.15, 0.11), alpha: 0.40, size: 1.05, hold: 14.0, fade: 12.0 } }
fn desert_look() -> Surface { Surface { color: Color::srgb(0.68, 0.59, 0.42), alpha: 0.28, size: 1.0, hold: 6.0, fade: 6.0 } }
fn dirt_look() -> Surface { Surface { color: Color::srgb(0.20, 0.16, 0.10), alpha: 0.28, size: 1.0, hold: 8.0, fade: 7.0 } }

/// One stamped print: fades its own (owned) material's alpha after a hold, then despawns + frees
/// the material so the per-print clones never leak.
#[derive(Component)]
struct Footprint {
    born: f32,
    hold: f32,
    fade: f32,
    alpha0: f32,
    mat: Handle<StandardMaterial>,
}

/// FIFO of live prints for the cap.
#[derive(Resource, Default)]
struct PrintLog(VecDeque<Entity>);

/// The shared boot decal mesh, built once.
#[derive(Resource)]
struct PrintAssets {
    boot: Handle<Mesh>,
}

pub struct FootprintPlugin;

impl Plugin for FootprintPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PrintLog>()
            .add_systems(Startup, setup)
            // Stamping is sim-side (gated with panels/pauses, like the footstep emitter); the fade
            // is visual and keeps settling even while frozen.
            .add_systems(Update, emit.run_if(in_state(Modal::None)))
            .add_systems(Update, (fade, feet_test));
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(PrintAssets { boot: meshes.add(boot_mesh()) });
}

/// A small boot sole lying flat in XZ (normal up): a bigger ball disc toward +Z (the heading) and a
/// smaller heel disc behind it, merged so the print reads as a boot rather than a plain dot.
fn boot_mesh() -> Mesh {
    let lay = Quat::from_rotation_x(-FRAC_PI_2); // Circle is built in XY; lay it into the ground plane
    let ball = Circle::new(0.11).mesh().resolution(12).build().rotated_by(lay).translated_by(Vec3::new(0.0, 0.0, 0.07));
    let heel = Circle::new(0.075).mesh().resolution(10).build().rotated_by(lay).translated_by(Vec3::new(0.0, 0.0, -0.09));
    let mut m = ball;
    m.merge(&heel).expect("boot discs share attributes");
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// On each gait half-cycle (a footfall), stamp a print of the surface underfoot — alternating the
/// left/right foot and offsetting it to the side of the heading.
#[allow(clippy::too_many_arguments)]
fn emit(
    mut last_half: Local<i64>,
    mut last_pos: Local<Option<Vec2>>,
    time: Res<Time>,
    mut commands: Commands,
    assets: Option<Res<PrintAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut log: ResMut<PrintLog>,
    hero_q: Query<&Hero>,
) {
    let (Some(assets), Ok(hero)) = (assets, hero_q.single()) else {
        return;
    };
    // "Walking" = the hero actually translated this frame, NOT the `moving` flag. The flag is reset
    // to false every frame in FreeRoam (the debug/clip camera mode), which races this footfall emit;
    // keying off real displacement makes the trail robust to that AND lets staged demo/clip walks
    // stamp prints for verification. (First frame: seed `last_pos`, never stamp.)
    let moved = last_pos.replace(hero.pos).map_or(0.0, |prev| (hero.pos - prev).length());
    let half = (hero.walk_phase / PI).floor() as i64;
    if !((hero.moving || moved > 1e-4) && hero.on_ground) {
        *last_half = half; // idle/airborne: stay current so resuming doesn't fire a stale print
        return;
    }
    if half == *last_half {
        return;
    }
    *last_half = half;

    // Foot side from the half-cycle parity; offset along the heading's right vector so the two boot
    // rows sit either side of the centre line.
    let side = if half & 1 == 0 { 1.0 } else { -1.0 };
    let right = Vec2::new(hero.facing.cos(), -hero.facing.sin());
    let pos = hero.pos + right * (FOOT_OFFSET * side);

    let Some(surf) = surface_for(pos) else { return }; // bare rock: no print
    let y = worldmap::ground_at_world(pos.x, pos.y).unwrap_or(hero.y);

    // Each print owns an alpha-blended material clone so `fade` can dim it solo.
    let mat = materials.add(StandardMaterial {
        base_color: surf.color.with_alpha(surf.alpha),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        depth_bias: DECAL_DEPTH_BIAS,
        ..default()
    });
    let tf = Transform {
        translation: Vec3::new(pos.x, y + 0.02, pos.y),
        rotation: Quat::from_rotation_y(hero.facing), // +localZ (the ball) points along the heading
        scale: Vec3::splat(surf.size),
    };
    let e = commands
        .spawn((
            Mesh3d(assets.boot.clone()),
            MeshMaterial3d(mat.clone()),
            tf,
            NotShadowCaster,
            BiomeEntity,
            Footprint { born: time.elapsed_secs(), hold: surf.hold, fade: surf.fade, alpha0: surf.alpha, mat },
        ))
        .id();
    log.0.push_back(e);
    while log.0.len() > MAX_PRINTS {
        if let Some(old) = log.0.pop_front() {
            commands.entity(old).try_despawn();
        }
    }
}

/// Screenshot/verify hook: `FOREST_FEETTEST=1` stamps a straight marching row of prints (one per
/// surface kind, repeated, alternating L/R) on the open lawn just north of the gate, ONCE — so a
/// staged `FOREST_SHOT` can frame the decal in isolation (the in-world trail only stamps while the
/// hero actually walks, which the static capture can't drive). No effect in normal play.
fn feet_test(
    mut done: Local<bool>,
    mut commands: Commands,
    assets: Option<Res<PrintAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut log: ResMut<PrintLog>,
) {
    if *done || std::env::var("FOREST_FEETTEST").is_err() {
        return;
    }
    let Some(assets) = assets else { return }; // wait for `setup`
    *done = true;
    // Row origin: `FOREST_FEET_AT="x,z"` (e.g. a biome region) or the lawn at (0,18). Each print is
    // tinted by the REAL `surface_for` at its spot, so the preview shows the true in-world trail for
    // whatever ground it's laid on (snow trail in snow, sand in desert, faint scuff on grass).
    let base = std::env::var("FOREST_FEET_AT").ok().and_then(|s| {
        let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        (v.len() == 2).then(|| Vec2::new(v[0], v[1]))
    }).unwrap_or(Vec2::new(0.0, 18.0));
    for i in 0..16u32 {
        let along = base + Vec2::new(0.0, i as f32 * 0.42); // march +Z
        let side = if i & 1 == 0 { 1.0 } else { -1.0 };
        let pos = along + Vec2::new(FOOT_OFFSET * side, 0.0);
        let Some(s) = surface_for(pos) else { continue }; // bare rock: skip
        let y = worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
        let mat = materials.add(StandardMaterial {
            base_color: s.color.with_alpha(s.alpha),
            perceptual_roughness: 1.0,
            alpha_mode: AlphaMode::Blend,
            depth_bias: DECAL_DEPTH_BIAS,
            ..default()
        });
        let tf = Transform {
            translation: Vec3::new(pos.x, y + 0.02, pos.y),
            rotation: Quat::from_rotation_y(0.0), // ball points +Z (up the row)
            scale: Vec3::splat(s.size),
        };
        let e = commands
            .spawn((Mesh3d(assets.boot.clone()), MeshMaterial3d(mat.clone()), tf, NotShadowCaster, BiomeEntity, Footprint { born: 0.0, hold: 9_000.0, fade: 1.0, alpha0: s.alpha, mat }))
            .id();
        log.0.push_back(e);
    }
}

/// Hold each print at full for `hold`s, then ramp its (owned) material alpha down over `fade`s and
/// despawn + free the material once it's gone.
fn fade(
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    prints: Query<(Entity, &Footprint)>,
) {
    let now = time.elapsed_secs();
    for (e, p) in &prints {
        let age = now - p.born;
        if age < p.hold {
            continue;
        }
        let f = ((age - p.hold) / p.fade).min(1.0); // 0 → 1 across the fade window
        if f >= 1.0 {
            materials.remove(&p.mat);
            commands.entity(e).try_despawn();
        } else if let Some(mut m) = materials.get_mut(&p.mat) {
            // Keep the baked hue, ramp the alpha from alpha0 → 0 (the aftermath blood-fade pattern).
            m.base_color = m.base_color.with_alpha(p.alpha0 * (1.0 - f));
        }
    }
}
