//! Building health bars — a billboarded HP bar over any building that has taken damage (hp < max),
//! both sides, always visible while hurt (you need to see a building being torn down). Full-health
//! buildings show nothing, so an untouched town stays clean. Same pooled billboard machinery as
//! `unitbars.rs`, but its own system: different visibility rule (damaged, not selected), wider bar,
//! and a per-kind height (a town hall's bar rides much higher than a wall's).

use bevy::prelude::*;

use crate::dying::Dying;
use crate::game_state::AppState;
use crate::player::Health;
use crate::rts::{building_def, in_skirmish, BuildingKind, RtsBuilding};

const BAR_H: f32 = 0.2;

/// Half the footprint in world units.
fn fp_half(kind: BuildingKind) -> f32 {
    building_def(kind).footprint as f32 * 0.5
}

#[derive(Component)]
struct BldgBar;
#[derive(Component)]
struct BldgBarFill;

#[derive(Resource)]
struct BldgBarAssets {
    quad: Handle<Mesh>,
    back: Handle<StandardMaterial>,
    hi: Handle<StandardMaterial>,
    mid: Handle<StandardMaterial>,
    lo: Handle<StandardMaterial>,
}

pub struct RtsBuildingBarsPlugin;

impl Plugin for RtsBuildingBarsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            sync_building_bars
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

fn mat(m: &mut Assets<StandardMaterial>, c: Color) -> Handle<StandardMaterial> {
    m.add(StandardMaterial { base_color: c, emissive: LinearRgba::from(c) * 0.4, unlit: true, ..default() })
}

/// Bar height (world units above the building base) — scales with footprint, with the two tall
/// towers lifted so the bar clears their roofs.
fn bar_y(kind: BuildingKind) -> f32 {
    // Buildings render at BUILD_SCALE, so lift the bar the same so it still clears the roof.
    let base = match kind {
        BuildingKind::TownHall => 5.1,
        BuildingKind::Watchtower => 5.9,
        other => fp_half(other) * 1.1 + 2.3,
    };
    base * crate::rts::build::BUILD_SCALE
}

#[allow(clippy::type_complexity)]
fn sync_building_bars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<BldgBarAssets>>,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    buildings: Query<(&GlobalTransform, &RtsBuilding, &Health), Without<Dying>>,
    mut bars: Query<(&mut Transform, &mut Visibility, &Children), With<BldgBar>>,
    mut fills: Query<(&mut Transform, &mut MeshMaterial3d<StandardMaterial>), (With<BldgBarFill>, Without<BldgBar>)>,
) {
    let a = assets.get_or_insert_with(|| BldgBarAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        back: mat(&mut materials, Color::srgb(0.06, 0.06, 0.06)),
        hi: mat(&mut materials, Color::srgb(0.35, 0.85, 0.35)),
        mid: mat(&mut materials, Color::srgb(0.95, 0.8, 0.25)),
        lo: mat(&mut materials, Color::srgb(0.95, 0.3, 0.25)),
    });
    let face = cam.single().map(|g| g.rotation()).unwrap_or(Quat::IDENTITY);

    // Only DAMAGED buildings want a bar: (head pos, hp fraction, bar width).
    let want: Vec<(Vec3, f32, f32)> = buildings
        .iter()
        .filter(|(_, _, h)| h.hp > 0.0 && h.hp < h.max - 0.5)
        .map(|(gt, b, h)| {
            let p = gt.translation();
            let frac = (h.hp / h.max).clamp(0.0, 1.0);
            let w = fp_half(b.kind) * 1.3 + 0.7;
            (Vec3::new(p.x, p.y + bar_y(b.kind), p.z), frac, w)
        })
        .collect();

    // Grow the pool: each bar = a backing quad + a fill quad.
    let have = bars.iter().count();
    for _ in have..want.len() {
        commands
            .spawn((Transform::from_xyz(0.0, -100.0, 0.0), Visibility::Hidden, BldgBar))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(a.quad.clone()),
                    MeshMaterial3d(a.back.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(1.06, BAR_H + 0.06, 1.0)),
                ));
                p.spawn((
                    Mesh3d(a.quad.clone()),
                    MeshMaterial3d(a.hi.clone()),
                    Transform::from_xyz(0.0, 0.0, 0.01).with_scale(Vec3::new(1.0, BAR_H, 1.0)),
                    BldgBarFill,
                ));
            });
    }

    let mut it = want.into_iter();
    for (mut tf, mut vis, children) in &mut bars {
        match it.next() {
            Some((pos, frac, w)) => {
                tf.translation = pos;
                tf.rotation = face;
                // Scale the whole bar so the backing width = w (children are unit-width).
                tf.scale = Vec3::new(w, 1.0, 1.0);
                *vis = Visibility::Visible;
                for &c in children {
                    if let Ok((mut ftf, mut fmat)) = fills.get_mut(c) {
                        // Left-anchor the fill within the (unit-width) bar, scaled by frac.
                        ftf.scale.x = frac.max(0.0001);
                        ftf.translation.x = -0.5 + frac * 0.5;
                        let want_mat = if frac > 0.5 { &a.hi } else if frac > 0.25 { &a.mid } else { &a.lo };
                        if fmat.0.id() != want_mat.id() {
                            fmat.0 = want_mat.clone();
                        }
                    }
                }
            }
            None => *vis = Visibility::Hidden,
        }
    }
}
