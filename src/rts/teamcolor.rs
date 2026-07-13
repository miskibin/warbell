//! Team colours — the friend/foe read the RTS was missing. A pooled flat ground ring sits under
//! every unit and building, tinted by [`Side`] (player = blue, rival = red), so at a glance you can
//! tell your army/town from the enemy's without reading a single label. Buildings get a bigger ring
//! sized to their footprint. The bright green **selection** ring (`select.rs`) still draws on top.
//!
//! Same reposition-a-pool approach as the selection rings (no per-entity child management, so a ring
//! can't orphan when a unit dies mid-frame); one shared ring mesh scaled per entity, two shared
//! unlit materials.

use bevy::prelude::*;

use crate::dying::Dying;
use crate::game_state::AppState;
use crate::rts::{building_def, in_skirmish, RtsBuilding, RtsUnit, Side};

/// Player ring colour (cool blue) and rival ring colour (warm red).
const PLAYER: Color = Color::srgb(0.25, 0.55, 1.0);
const RIVAL: Color = Color::srgb(1.0, 0.3, 0.28);

#[derive(Component)]
struct TeamRing;

/// Cached ring mesh + the two side materials.
#[derive(Resource)]
struct TeamRingAssets {
    mesh: Handle<Mesh>,
    player: Handle<StandardMaterial>,
    rival: Handle<StandardMaterial>,
}

pub struct RtsTeamColorPlugin;

impl Plugin for RtsTeamColorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            sync_team_rings
                .run_if(in_skirmish)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

#[allow(clippy::type_complexity)]
fn sync_team_rings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    units: Query<(&GlobalTransform, &Side), (With<RtsUnit>, Without<Dying>)>,
    buildings: Query<(&GlobalTransform, &Side, &RtsBuilding), Without<Dying>>,
    mut rings: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>), With<TeamRing>>,
    mut assets_l: Local<Option<TeamRingAssets>>,
) {
    let a = assets_l.get_or_insert_with(|| {
        let side_mat = |c: Color| StandardMaterial {
            base_color: c.with_alpha(0.75),
            emissive: LinearRgba::from(c) * 0.6,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        };
        TeamRingAssets {
            // Base ring radius 1.0 — scaled per entity below.
            mesh: meshes.add(Annulus::new(0.8, 1.0).mesh().resolution(28).build()),
            player: materials.add(side_mat(PLAYER)),
            rival: materials.add(side_mat(RIVAL)),
        }
    });

    // Desired rings: (world pos, radius, side).
    let mut want: Vec<(Vec3, f32, Side)> = Vec::new();
    for (gt, side) in &units {
        want.push((gt.translation(), 0.62, *side));
    }
    for (gt, side, b) in &buildings {
        let r = building_def(b.kind).footprint as f32 * 0.5 + 0.5;
        want.push((gt.translation(), r, *side));
    }

    // Grow the pool.
    let have = rings.iter().count();
    for _ in have..want.len() {
        commands.spawn((
            Mesh3d(a.mesh.clone()),
            MeshMaterial3d(a.player.clone()),
            Transform::from_xyz(0.0, -100.0, 0.0)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            Visibility::Hidden,
            TeamRing,
        ));
    }

    let mut it = want.into_iter();
    for (mut tf, mut vis, mut mat) in &mut rings {
        match it.next() {
            Some((p, r, side)) => {
                tf.translation = Vec3::new(p.x, p.y + 0.06, p.z);
                tf.scale = Vec3::splat(r);
                // Keep the flat rotation set at spawn (only translation/scale change).
                let want_mat = match side {
                    Side::Player => &a.player,
                    Side::Rival => &a.rival,
                };
                if mat.0.id() != want_mat.id() {
                    mat.0 = want_mat.clone();
                }
                *vis = Visibility::Visible;
            }
            None => *vis = Visibility::Hidden,
        }
    }
}
