//! **Night sky** — stars + a low-poly moon that fade in after dark. The procedural
//! `Atmosphere` sky (scene.rs) is a plain gradient after sunset, so the night half of the
//! game had nothing to look at overhead; this gives the long sieges a sky.
//!
//! How it works: a rig entity follows the CAMERA's translation every frame (rotation is
//! never copied), holding everything at a fixed ~95-unit dome radius — inside the camera's
//! 230 far plane and just past the 85-tile fog-clear radius, so the dome is barely fogged.
//! The stars are ONE merged mesh (220 tiny faceted balls, per-star tint/brightness in
//! `ATTRIBUTE_COLOR`) on an additive unlit material, so the whole field is a single draw
//! call and black contributes nothing — no silhouettes against a dusk sky. The moon rides
//! the point OPPOSITE the sun (computed from [`SkyClock.t`] exactly like `advance_sky`), so
//! it rises in the east as the sun sets and climbs through the night.
//!
//! Fade: everything scales with the same `night` curve `advance_sky` uses, by mutating the
//! shared materials' colours each frame (3 tiny materials — cheap). Fully hidden by day.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::scene::SkyClock;

/// Dome radius (world units) — see module doc for why ~95.
const DOME_R: f32 = 95.0;
const STAR_COUNT: usize = 220;
/// Moon disc radius at [`DOME_R`] — subtends ~3.7°, matching the stylised oversized
/// `SunDisk` (0.060 rad) rather than a realistic pin-prick moon.
const MOON_R: f32 = 3.1;
/// Linear brightness multiplier on the star field at full night (additive, so >1 just
/// pushes the bright stars into Bloom's range).
const STAR_BRIGHT: f32 = 2.2;

pub struct NightSkyPlugin;

impl Plugin for NightSkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_night_sky)
            // Ungated: purely visual, must keep tracking the camera + clock through panels
            // and pauses (the frozen world still draws — see CLAUDE.md freeze-gate notes).
            .add_systems(Update, drive_night_sky);
    }
}

/// Root rig that follows the camera's translation (never its rotation).
#[derive(Component)]
struct NightSkyRig;
/// The merged star-field mesh child (slow-spinning).
#[derive(Component)]
struct StarField;
/// The moon body child (repositioned opposite the sun each frame).
#[derive(Component)]
struct Moon;
/// The additive halo around the moon (slightly larger, repositioned with it).
#[derive(Component)]
struct MoonGlow;

/// The three shared materials the fade system mutates each frame.
#[derive(Resource)]
struct NightSkyMats {
    stars: Handle<StandardMaterial>,
    moon: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
}

// ── Setup ────────────────────────────────────────────────────────────────────────

fn setup_night_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Additive + unlit: per-star tint/brightness lives in the vertex colours, the material
    // base colour is the global fade knob. Black adds nothing, so a faded star field never
    // silhouettes against the dusk gradient.
    let stars = materials.add(StandardMaterial {
        base_color: Color::NONE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    // The moon body is unlit + opaque (it should occlude stars behind it); its crater
    // shading is baked into vertex colours so it survives `unlit`.
    let moon = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,
        ..default()
    });
    let glow = materials.add(StandardMaterial {
        base_color: Color::NONE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });

    let rig = commands
        .spawn((NightSkyRig, Transform::default(), Visibility::Hidden))
        .id();
    let star_field = commands
        .spawn((
            StarField,
            Mesh3d(meshes.add(star_field_mesh())),
            MeshMaterial3d(stars.clone()),
            Transform::default(),
            NotShadowCaster,
        ))
        .id();
    let moon_body = commands
        .spawn((
            Moon,
            Mesh3d(meshes.add(moon_mesh())),
            MeshMaterial3d(moon.clone()),
            Transform::from_xyz(0.0, DOME_R, 0.0),
            NotShadowCaster,
        ))
        .id();
    let moon_glow = commands
        .spawn((
            MoonGlow,
            Mesh3d(meshes.add(Sphere::new(MOON_R * 1.45).mesh().ico(2).expect("ico detail in range"))),
            MeshMaterial3d(glow.clone()),
            Transform::from_xyz(0.0, DOME_R, 0.0),
            NotShadowCaster,
        ))
        .id();
    commands.entity(rig).add_children(&[star_field, moon_body, moon_glow]);
    commands.insert_resource(NightSkyMats { stars, moon, glow });
}

/// One merged mesh of 220 tiny faceted balls scattered over the upper hemisphere.
/// Per-star colour + brightness ride `ATTRIBUTE_COLOR` (the additive material multiplies
/// them in), with a handful of bright "named" stars and a faint majority — the classic
/// low-poly trick of faking density with value range instead of count.
fn star_field_mesh() -> Mesh {
    let mut rng = Rng(0x57a2_715e);
    let mut parts: Vec<Mesh> = Vec::with_capacity(STAR_COUNT);
    for _ in 0..STAR_COUNT {
        // Direction: keep off the horizon band (fog + landmass) — y in 0.08..1.
        let y = rng.range(0.08, 1.0);
        let az = rng.range(0.0, std::f32::consts::TAU);
        let hr = (1.0 - y * y).sqrt();
        let dir = Vec3::new(az.cos() * hr, y, az.sin() * hr);

        // Brightness tiers: ~8% bright beacons, ~30% mid, the rest faint dust.
        let tier = rng.next();
        let (bright, r) = if tier < 0.08 {
            (rng.range(0.85, 1.0), rng.range(0.34, 0.46))
        } else if tier < 0.38 {
            (rng.range(0.45, 0.7), rng.range(0.24, 0.32))
        } else {
            (rng.range(0.16, 0.38), rng.range(0.16, 0.24))
        };
        // Tint: mostly white, a few warm (amber) and cool (blue) stars for life.
        let t = rng.next();
        let tint = if t < 0.12 {
            [1.0, 0.78, 0.55]
        } else if t < 0.28 {
            [0.66, 0.78, 1.0]
        } else {
            [1.0, 1.0, 1.0]
        };
        let col = [tint[0] * bright, tint[1] * bright, tint[2] * bright, 1.0];

        let mut m = Sphere::new(r)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .translated_by(dir * DOME_R);
        let n = m.count_vertices();
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![col; n]);
        parts.push(m);
    }
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one star");
    for p in it {
        base.merge(&p).expect("stars share attributes");
    }
    base
}

/// Low-poly moon — a pale faceted ball with a few darker squashed "maria" blobs sunk just
/// proud of the surface (vertex-colour fake detail; the unlit material keeps the contrast).
fn moon_mesh() -> Mesh {
    let pale = [0.92, 0.95, 1.0, 1.0];
    let maria = [0.62, 0.68, 0.82, 1.0];
    let tint = |mut m: Mesh, c: [f32; 4]| -> Mesh {
        let n = m.count_vertices();
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
        m
    };
    let mut body = tint(Sphere::new(MOON_R).mesh().ico(2).expect("ico detail in range"), pale);
    // Crater blobs: squashed darker balls poking ~5% out of the sphere so the silhouette
    // stays round but the face reads mottled.
    for (dir, r) in [
        (Vec3::new(0.5, 0.55, 0.67), 0.95f32),
        (Vec3::new(-0.55, 0.2, 0.81), 0.7),
        (Vec3::new(0.1, -0.45, 0.89), 0.55),
        (Vec3::new(-0.25, 0.75, 0.61), 0.45),
    ] {
        let blob = tint(
            Sphere::new(r)
                .mesh()
                .ico(1)
                .expect("ico detail in range")
                .scaled_by(Vec3::new(1.0, 1.0, 0.35))
                .translated_by(Vec3::new(0.0, 0.0, MOON_R * 0.78))
                .rotated_by(Quat::from_rotation_arc(Vec3::Z, dir.normalize())),
            maria,
        );
        body.merge(&blob).expect("moon parts share attributes");
    }
    body
}

// ── Per-frame drive ──────────────────────────────────────────────────────────────

fn drive_night_sky(
    time: Res<Time>,
    clock: Res<SkyClock>,
    mats: Option<Res<NightSkyMats>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cam_q: Query<&Transform, (With<Camera3d>, Without<NightSkyRig>)>,
    mut rig_q: Query<(&mut Transform, &mut Visibility), (With<NightSkyRig>, Without<Camera3d>)>,
    mut star_q: Query<&mut Transform, (With<StarField>, Without<NightSkyRig>, Without<Camera3d>)>,
    mut moon_q: Query<
        &mut Transform,
        (Or<(With<Moon>, With<MoonGlow>)>, Without<StarField>, Without<NightSkyRig>, Without<Camera3d>),
    >,
) {
    let Some(mats) = mats else { return };
    let (Ok(cam), Ok((mut rig, mut vis))) = (cam_q.single(), rig_q.single_mut()) else { return };

    // Same sun geometry as `advance_sky`: elevation from the clock, `night` eases in as the
    // sun dips below the horizon.
    let a = clock.t * std::f32::consts::TAU;
    let sun_dir = Vec3::new(a.cos(), a.sin(), 0.55).normalize();
    let night = crate::scene::night_of(clock.t);

    if night <= 0.02 {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;
    rig.translation = cam.translation;

    // Slow star-field spin — barely perceptible, just enough that a long siege's sky lives.
    let t = time.elapsed_secs_wrapped();
    if let Ok(mut star_tf) = star_q.single_mut() {
        star_tf.rotation = Quat::from_rotation_y(t * 0.004);
    }
    // The moon rides opposite the sun: rises in the east at sunset, overhead at midnight.
    let moon_dir = -sun_dir;
    for mut tf in &mut moon_q {
        tf.translation = moon_dir * (DOME_R - 1.0);
    }

    // Fade the shared materials with `night` (+ a gentle global twinkle on the stars).
    let twinkle = 1.0 + 0.06 * (t * 2.3).sin() + 0.04 * (t * 5.1).sin();
    let s = STAR_BRIGHT * night * twinkle;
    if let Some(mut m) = materials.get_mut(&mats.stars) {
        let color = Color::linear_rgb(s, s, s);
        if m.base_color != color {
            m.base_color = color;
        }
    }
    if let Some(mut m) = materials.get_mut(&mats.moon) {
        let color = Color::linear_rgb(0.9 * night, 0.93 * night, night);
        if m.base_color != color {
            m.base_color = color;
        }
    }
    if let Some(mut m) = materials.get_mut(&mats.glow) {
        let g = 0.10 * night;
        let color = Color::linear_rgb(g * 0.8, g * 0.9, g);
        if m.base_color != color {
            m.base_color = color;
        }
    }
}

// ── Deterministic mulberry32 RNG (same recipe as decor.rs / camps.rs) ──────────────

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
