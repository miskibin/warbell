//! Always-on unit bars — a slim health bar + a type pip floating over every RTS unit, so the player
//! reads a unit's **state** (HP) and **role** (worker / swordsman / archer) at a glance without
//! hovering. Team (friend/foe) is already carried by the ground ring ([`super::teamcolor`]).
//!
//! Each bar is a billboarded root (pooled, reposition-a-pool like the team/selection rings) with
//! three child quads: a dark backing, a coloured fill (green→amber→red by HP, from a small shared
//! material set — no per-bar material churn), and a type pip at the left end. The root copies the
//! camera rotation each frame so the flat quads face the ortho view.

use bevy::prelude::*;

use crate::dying::Dying;
use crate::game_state::AppState;
use crate::player::Health;
use crate::rts::{in_skirmish, RtsUnit, UnitKind};

/// Bar width / height in world units (at the unit's head).
const BAR_W: f32 = 1.4;
const BAR_H: f32 = 0.17;
/// Height above the unit's feet the bar floats.
const BAR_Y: f32 = 2.35;

#[derive(Component)]
struct UnitBar;
/// Marks the fill child (its material + x-scale change per frame).
#[derive(Component)]
struct BarFill;
/// Marks the pip child (its material changes with unit kind).
#[derive(Component)]
struct BarPip;

#[derive(Resource)]
struct BarAssets {
    quad: Handle<Mesh>,
    back: Handle<StandardMaterial>,
    hi: Handle<StandardMaterial>,
    mid: Handle<StandardMaterial>,
    lo: Handle<StandardMaterial>,
    worker: Handle<StandardMaterial>,
    sword: Handle<StandardMaterial>,
    archer: Handle<StandardMaterial>,
}

pub struct RtsUnitBarsPlugin;

impl Plugin for RtsUnitBarsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            sync_unit_bars
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

fn mat(materials: &mut Assets<StandardMaterial>, c: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: c, emissive: LinearRgba::from(c) * 0.4, unlit: true, ..default() })
}

#[allow(clippy::type_complexity)]
fn sync_unit_bars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<BarAssets>>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    units: Query<(&GlobalTransform, &RtsUnit, &Health), Without<Dying>>,
    mut bars: Query<(&mut Transform, &mut Visibility, &Children), With<UnitBar>>,
    mut fills: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), (With<BarFill>, Without<UnitBar>, Without<BarPip>)>,
    mut pips: Query<&mut MeshMaterial3d<StandardMaterial>, (With<BarPip>, Without<UnitBar>, Without<BarFill>)>,
) {
    let a = assets.get_or_insert_with(|| BarAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        back: mat(&mut materials, Color::srgb(0.06, 0.06, 0.06)),
        hi: mat(&mut materials, Color::srgb(0.35, 0.85, 0.35)),
        mid: mat(&mut materials, Color::srgb(0.95, 0.8, 0.25)),
        lo: mat(&mut materials, Color::srgb(0.95, 0.3, 0.25)),
        worker: mat(&mut materials, Color::srgb(0.65, 0.45, 0.25)), // tan
        sword: mat(&mut materials, Color::srgb(0.7, 0.75, 0.82)),   // steel
        archer: mat(&mut materials, Color::srgb(0.4, 0.8, 0.45)),   // green
    });
    let face = cam.single().map(|g| g.rotation()).unwrap_or(Quat::IDENTITY);

    // Desired bars: (head pos, hp fraction, kind).
    let want: Vec<(Vec3, f32, UnitKind)> = units
        .iter()
        .map(|(gt, u, h)| {
            let p = gt.translation();
            let frac = if h.max > 0.0 { (h.hp / h.max).clamp(0.0, 1.0) } else { 0.0 };
            (Vec3::new(p.x, p.y + BAR_Y, p.z), frac, u.kind)
        })
        .collect();

    // Grow the pool: each bar root = back quad + fill quad + pip quad.
    let have = bars.iter().count();
    for _ in have..want.len() {
        commands
            .spawn((Transform::from_xyz(0.0, -100.0, 0.0), Visibility::Hidden, UnitBar))
            .with_children(|p| {
                // Backing.
                p.spawn((
                    Mesh3d(a.quad.clone()),
                    MeshMaterial3d(a.back.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(BAR_W + 0.06, BAR_H + 0.06, 1.0)),
                ));
                // Fill (left-anchored; scaled per frame).
                p.spawn((
                    Mesh3d(a.quad.clone()),
                    MeshMaterial3d(a.hi.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.01).with_scale(Vec3::new(BAR_W, BAR_H, 1.0)),
                    BarFill,
                ));
                // Type pip at the left end, just above the bar.
                p.spawn((
                    Mesh3d(a.quad.clone()),
                    MeshMaterial3d(a.worker.clone()),
                    Transform::from_xyz(-(BAR_W * 0.5) - 0.13, 0.0, 0.01).with_scale(Vec3::splat(BAR_H + 0.09)),
                    BarPip,
                ));
            });
    }

    let mut it = want.into_iter();
    for (mut tf, mut vis, children) in &mut bars {
        match it.next() {
            Some((pos, frac, kind)) => {
                tf.translation = pos;
                tf.rotation = face;
                *vis = Visibility::Visible;
                for &c in children {
                    if let Ok((mut ftf, mut fmat)) = fills.get_mut(c) {
                        // Left-anchor the fill: shrink width to `frac`, slide left so its left edge
                        // stays put (local origin is the bar centre).
                        ftf.scale.x = (BAR_W * frac).max(0.0001);
                        ftf.translation.x = -BAR_W * 0.5 + BAR_W * frac * 0.5;
                        let want_mat = if frac > 0.5 { &a.hi } else if frac > 0.25 { &a.mid } else { &a.lo };
                        if fmat.0.id() != want_mat.id() {
                            fmat.0 = want_mat.clone();
                        }
                    }
                    if let Ok(mut pmat) = pips.get_mut(c) {
                        let want_mat = match kind {
                            UnitKind::Worker => &a.worker,
                            UnitKind::Swordsman => &a.sword,
                            UnitKind::Archer => &a.archer,
                        };
                        if pmat.0.id() != want_mat.id() {
                            pmat.0 = want_mat.clone();
                        }
                    }
                }
            }
            None => *vis = Visibility::Hidden,
        }
    }
}
