//! **Grass underfoot** — the undergrowth answers the people walking through it. Two effects ride
//! one small GPU buffer that every undergrowth card reads in its vertex stage
//! (`foliage_wind.wesl` / `foliage_wind_prepass.wesl`, `trample()`):
//!
//! - **Push**: the hero and the nearest walkers (townsfolk, orks, the rival's men) part the
//!   blades around their legs as they pass — a live, per-frame "pusher" disc each.
//! - **Trample**: every footfall ([`crate::footing::Footfall`]) flattens a patch of a world-locked
//!   grid that follows the hero; the grass stays pressed down for a few seconds, then springs
//!   back up over ~10 s — so a walk through a meadow leaves a visible trodden track behind you.
//!
//! The buffer is a fixed-id [`ShaderBuffer`] ([`TRAMPLE_BUF`]) rewritten in place each frame
//! (Bevy reuses the GPU buffer when the size is unchanged), so no foliage material is ever marked
//! modified — which would re-specialise every foliage mesh (see `realforest::foliage`).
//! Layout (`array<vec4<f32>>`): `[0]` = grid (min x, min z, cell size, cells per side), `[1..=8]`
//! = pushers (x, y, z, radius; radius 0 = unused), then `N*N/4` vec4s of trample cells (0 = upright,
//! ≥ 1 = flat). Only materials flagged by [`crate::realforest::foliage::ground_match`]
//! (`wind.y = 1`: the undergrowth) react; tree crowns and the snow twins ignore it.
//! `FOREST_TRAMPLE=0` turns it off (A/B). `FOREST_TRAMPLETEST="x,z"` keeps a 2.5u disc flattened at a
//! world XZ (within 16u of the hero) for a still of the trodden look.

use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;

use crate::footing::{Footfall, FootfallSet, Stepper, Surface};
use crate::game_state::SimAppExt;
use crate::player::Hero;

/// The shared trample buffer every undergrowth material binds (`FoliageExt::trample`).
pub const TRAMPLE_BUF: Handle<ShaderBuffer> = bevy::asset::uuid_handle!("3f6e2a8c-91d4-4b7a-b2c5-7d0e4f19a6b3");

/// Cells per grid side, and their size (world units): a 32 u square around the hero.
const N: usize = 64;
const CELL: f32 = 0.5;
/// The grid re-centres once the hero is this many cells off its middle (data shifts with it).
const RECENTRE: i32 = 10;
/// Live pusher slots (hero first, then the nearest walkers) — keep in sync with the shaders.
const PUSHERS: usize = 8;
/// Walkers further than this from the hero don't push (too far to see the blades part).
const PUSH_RANGE: f32 = 24.0;
/// Seconds for a fully flattened patch to stand back up…
const RECOVER: f32 = 10.0;
/// …after holding flat for `HOLD × RECOVER` seconds (the stamp writes `1 + HOLD`, the shader
/// clamps at 1).
const HOLD: f32 = 0.45;

/// The CPU side of the trample grid (world-locked cells, min-corner cell index `origin`).
#[derive(Resource)]
struct Field {
    origin: IVec2,
    cells: Vec<f32>,
    /// Any cell above 0 (skip the decay sweep on untouched ground).
    live: bool,
}

impl Default for Field {
    fn default() -> Self {
        Self { origin: IVec2::splat(i32::MIN / 2), cells: vec![0.0; N * N], live: false }
    }
}

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("FOREST_TRAMPLE").map(|v| v != "0").unwrap_or(true))
}

fn buffer_len() -> usize {
    1 + PUSHERS + N * N / 4
}

/// Create the shared buffer (idle). It must exist whether or not trampling is on — every undergrowth
/// material binds it, and an unfilled handle binds a 1-byte placeholder that fails wgpu validation —
/// so the standalone model viewer, which draws foliage without this plugin, calls it too.
pub fn insert_buffer(app: &mut App) {
    let mut buffers = app.world_mut().resource_mut::<Assets<ShaderBuffer>>();
    let mut data = vec![[0.0f32; 4]; buffer_len()];
    data[0] = [0.0, 0.0, CELL, N as f32];
    let _ = buffers.insert(&TRAMPLE_BUF, ShaderBuffer::new(data, bevy::asset::RenderAssetUsages::default()));
}

pub struct GrassTramplePlugin;

impl Plugin for GrassTramplePlugin {
    fn build(&self, app: &mut App) {
        insert_buffer(app);
        if !enabled() {
            return;
        }
        app.init_resource::<Field>().add_sim_systems(update.after(FootfallSet));
    }
}

type Walkers = Or<(
    With<crate::villagers::Villager>,
    With<crate::orks::Ork>,
    With<crate::rival::RivalSoldier>,
    With<crate::rival::RivalWorker>,
    With<crate::rival::RivalRaider>,
)>;

fn update(
    time: Res<Time>,
    mut field: ResMut<Field>,
    mut steps: MessageReader<Footfall>,
    hero_q: Query<&Hero>,
    walkers: Query<&GlobalTransform, (Walkers, Without<crate::dying::Dying>, Without<Hero>)>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let Ok(hero) = hero_q.single() else {
        steps.clear();
        return;
    };
    let dt = time.delta_secs();
    let f = &mut *field;

    // Follow the hero: re-centre (shifting the trodden cells with the window) once he strays.
    let want = (hero.pos / CELL).floor().as_ivec2() - IVec2::splat(N as i32 / 2);
    let off = want - f.origin;
    if off.x.abs() > RECENTRE || off.y.abs() > RECENTRE {
        let mut moved = vec![0.0; N * N];
        if f.live {
            for z in 0..N as i32 {
                for x in 0..N as i32 {
                    let (sx, sz) = (x + off.x, z + off.y);
                    if (0..N as i32).contains(&sx) && (0..N as i32).contains(&sz) {
                        moved[(z as usize) * N + x as usize] = f.cells[(sz as usize) * N + sx as usize];
                    }
                }
            }
        }
        f.cells = moved;
        f.origin = want;
    }

    // Spring back up.
    if f.live {
        let k = dt / RECOVER;
        let mut any = false;
        for v in f.cells.iter_mut() {
            if *v > 0.0 {
                *v = (*v - k).max(0.0);
                any |= *v > 0.0;
            }
        }
        f.live = any;
    }

    if let Some(c) = trample_test() {
        stamp(f, c, 2.5);
    }

    // Press down a patch under every footfall on ground that grows something.
    for s in steps.read() {
        if matches!(s.surface, Surface::Water | Surface::Wood | Surface::Stone) {
            continue;
        }
        let r = match s.who {
            Stepper::Hero => 0.42,
            Stepper::HeroLanding { .. } => 0.6,
            Stepper::Npc { .. } => 0.32,
        };
        stamp(f, s.at.xz(), r);
    }

    // Live pushers: the hero, then the nearest walkers.
    let mut near: Vec<(f32, Vec3)> = walkers
        .iter()
        .map(|g| g.translation())
        .map(|p| (p.xz().distance_squared(hero.pos), p))
        .filter(|(d, _)| *d < PUSH_RANGE * PUSH_RANGE)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut data = Vec::with_capacity(buffer_len());
    data.push([f.origin.x as f32 * CELL, f.origin.y as f32 * CELL, CELL, N as f32]);
    data.push([hero.pos.x, hero.y, hero.pos.y, 0.62]);
    for i in 0..PUSHERS - 1 {
        data.push(near.get(i).map_or([0.0; 4], |(_, p)| [p.x, p.y, p.z, 0.5]));
    }
    for c in f.cells.chunks_exact(4) {
        data.push([c[0], c[1], c[2], c[3]]);
    }
    if let Some(mut buf) = buffers.get_mut(&TRAMPLE_BUF) {
        // Same size / usage / label → the render side rewrites the existing GPU buffer in place.
        *buf = ShaderBuffer::new(data, bevy::asset::RenderAssetUsages::default());
    }
}

fn trample_test() -> Option<Vec2> {
    static V: std::sync::OnceLock<Option<Vec2>> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let v: Vec<f32> = std::env::var("FOREST_TRAMPLETEST").ok()?.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        (v.len() == 2).then(|| Vec2::new(v[0], v[1]))
    })
}

/// Flatten a disc of radius `r` at world XZ `at` (soft edge, never lifting an already-flatter cell).
fn stamp(f: &mut Field, at: Vec2, r: f32) {
    let c = at / CELL - f.origin.as_vec2();
    let rc = r / CELL;
    let (x0, x1) = ((c.x - rc).floor() as i32, (c.x + rc).ceil() as i32);
    let (z0, z1) = ((c.y - rc).floor() as i32, (c.y + rc).ceil() as i32);
    for z in z0.max(0)..=z1.min(N as i32 - 1) {
        for x in x0.max(0)..=x1.min(N as i32 - 1) {
            let d = (Vec2::new(x as f32 + 0.5, z as f32 + 0.5) - c).length() / rc;
            if d >= 1.0 {
                continue;
            }
            let v = (1.0 + HOLD) * (1.0 - d * d).sqrt();
            let cell = &mut f.cells[z as usize * N + x as usize];
            *cell = cell.max(v);
            f.live = true;
        }
    }
}
