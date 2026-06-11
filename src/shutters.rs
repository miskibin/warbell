//! **Curfew shutters** — the village windows already light up at dusk (`castle::window_glow`);
//! this buttons the town up for the night. Each lit house window gets a pair of hinged wooden
//! leaves that ease from ajar (day) to shut (night), so as the prep day plunges into siege-night
//! the town visibly closes its shutters instead of standing eerily open.
//!
//! The dusk swing falls straight out of the day/night curve — `openness` tracks the sun, so no
//! edge-trigger is needed and it stays in sync if you scrub time. The house meshes bake their
//! window as a flat emissive quad with no animatable sub-entity, so the shutters are independent
//! parent+leaf entities placed over each window. They carry the house's `CastlePart` reveal tag,
//! so `castle::sync_castle` shows a house's shutters exactly when the house itself is built.

use bevy::prelude::*;

use crate::biome::BiomeEntity;
use crate::castle::{CastleKind, CastlePart};

/// How far a leaf swings from shut (0 rad) to fully ajar (radians) — folded back ~131° to the side
/// of the window, a touch proud of the wall, when open.
const OPEN_ANGLE: f32 = 2.3;

/// One shutter leaf. `sign` (+1 right / −1 left) flips the hinge swing so the pair folds out to
/// either side of the window and meets in the middle when shut.
#[derive(Component)]
pub struct Shutter {
    sign: f32,
}

pub struct ShutterPlugin;

impl Plugin for ShutterPlugin {
    fn build(&self, app: &mut App) {
        // Ungated — a visual that tracks the sky clock (keeps swinging while a panel freezes sim).
        app.add_systems(Update, drive_shutters);
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Swing every shutter from ajar (day) to shut (night), following the sun. Openness eases across
/// the dusk band so the leaves close smoothly right as the siege night sets in.
fn drive_shutters(
    clock: Option<Res<crate::scene::SkyClock>>,
    mut q: Query<(&Shutter, &mut Transform)>,
) {
    let night = clock.map(|c| crate::scene::night_of(c.t)).unwrap_or(0.0);
    // Open through the day, latch shut across dusk (night 0.30 → 0.75 of the way to full dark).
    let openness = 1.0 - smoothstep(0.30, 0.75, night);
    for (s, mut tf) in &mut q {
        tf.rotation = Quat::from_rotation_y(s.sign * OPEN_ANGLE * openness);
    }
}

/// Spawn a pair of shutters over one house's front window. `(x, z)` is the house's world plot,
/// `face` its facing yaw, `kind` its reveal tag (so the shutters appear with the house), and
/// `window_local` the archetype's pre-scale front-window centre (`castle::house_window` — the
/// four dwelling silhouettes hang their glass at different spots). The two leaf meshes (built
/// once by the caller against the castle's shared material) are baked at world size, so the
/// house's non-uniform scale folds into the placement here, not the geometry.
#[allow(clippy::too_many_arguments)]
pub fn spawn_house_shutters(
    commands: &mut Commands,
    right_leaf: &Handle<Mesh>,
    left_leaf: &Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    x: f32,
    z: f32,
    face: f32,
    kind: CastleKind,
    window_local: Vec3,
) {
    // Window centre in world: the archetype's local window × the house scale (0.9, 0.74, 0.9),
    // nudged proud of the glass, then oriented to the house front.
    let rot = Quat::from_rotation_y(face);
    let centre = Vec3::new(x, 0.0, z)
        + rot * Vec3::new(window_local.x * 0.9, window_local.y * 0.74, window_local.z * 0.9 + 0.05);
    let vis = if matches!(kind, CastleKind::Always) { Visibility::Inherited } else { Visibility::Hidden };
    let parent = commands
        .spawn((Transform { translation: centre, rotation: rot, scale: Vec3::ONE }, vis, CastlePart { kind }, BiomeEntity))
        .id();
    // Hinge each leaf at its outer edge (± the window half-width, 0.21 × 0.9) so it covers its half
    // when shut and folds out past it when open.
    commands.entity(parent).with_children(|p| {
        p.spawn((
            Mesh3d(right_leaf.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(Vec3::new(0.189, 0.0, 0.0)),
            Shutter { sign: 1.0 },
        ));
        p.spawn((
            Mesh3d(left_leaf.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(Vec3::new(-0.189, 0.0, 0.0)),
            Shutter { sign: -1.0 },
        ));
    });
}
