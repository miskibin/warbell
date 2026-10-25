//! Landmark machinery — placement + the animated-set-piece spawn API. The five signature
//! biome set-pieces themselves (old mill, witch's hut, frozen spire, sunken pyramid, standing
//! stones) are authored in [`crate::landmark_models`]; this module owns:
//!
//!  * the shared low-poly mesh helpers (`tinted`/`merged`/`flat_shaded`/`mottle` + primitive
//!    builders) every landmark builder composes from,
//!  * the [`LandmarkModel`]/[`AnimPart`] spawn API — one merged static base mesh on the scene's
//!    shared white vertex-colour material (batches like everything else), plus child entities
//!    for the ANIMATED / glowing sub-parts (mill sails, orbiting ice shards, pulsing windows…),
//!  * [`RuinsFxPlugin`] — the cosmetic part animators (spin / sway / orbit / hover / rise /
//!    glow-pulse). Render-side dressing like `banner.rs`, so the systems stay UNGATED and the
//!    sails keep turning while a panel freezes the sim,
//!  * the flat-spot site picker + `populate_landmarks` world placement.
//!
//! Build contract (verified-APIs doc §9): every part is a primitive `tinted` with a flat linear
//! `ATTRIBUTE_COLOR` BEFORE the merge (the shared white `StandardMaterial` reads colour straight
//! off the vertices), merged via `Mesh::merge`, then `flat_shaded` (`duplicate_vertices` →
//! `compute_flat_normals` — in that order; the latter panics on an indexed mesh).

use bevy::light::NotShadowCaster;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_4, TAU};
use std::sync::OnceLock;

use crate::palette::lin;

// ── Mesh helpers (verified API forms; shared with `landmark_models`) ─────────

// The shared low-poly mesh trio now lives in `crate::meshkit`; re-exported here so the many
// `crate::ruins::{tinted, merged, flat_shaded}` importers (e.g. `landmark_models`) keep working.
pub(crate) use crate::meshkit::{flat_shaded, merged, tinted};

pub(crate) fn yv(v: f32) -> Vec3 {
    Vec3::new(0.0, v, 0.0)
}

/// A box primitive, positioned so its CENTER lands at `center`.
pub(crate) fn box_at(x: f32, y: f32, z: f32, center: Vec3) -> Mesh {
    Cuboid::new(x, y, z).mesh().build().translated_by(center)
}

/// A box rotated by an arbitrary quaternion about its own centre, then translated.
pub(crate) fn box_rot(x: f32, y: f32, z: f32, center: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(Cuboid::new(x, y, z).mesh().build().rotated_by(rot).translated_by(center), lin(c))
}

/// A faceted icosphere blob (ico detail 1), squashed on Y, centred at `center`.
pub(crate) fn ball(r: f32, center: Vec3, squash: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(1)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(center),
        lin(c),
    )
}

/// An upright cylinder centred at `center`. `res` keeps the low-poly facet look.
pub(crate) fn cyl(r: f32, h: f32, center: Vec3, res: u32, c: u32) -> Mesh {
    tinted(Cylinder::new(r, h).mesh().resolution(res).build().translated_by(center), lin(c))
}

/// A cylinder rotated about its centre, then translated (fallen column drums, leaning discs).
pub(crate) fn cyl_rot(r: f32, h: f32, res: u32, center: Vec3, rot: Quat, c: u32) -> Mesh {
    tinted(
        Cylinder::new(r, h).mesh().resolution(res).build().rotated_by(rot).translated_by(center),
        lin(c),
    )
}

/// A truncated-cone shell (tower bodies, thatch caps). `res` low keeps it faceted.
pub(crate) fn frustum(r_bottom: f32, r_top: f32, h: f32, center: Vec3, res: u32, c: u32) -> Mesh {
    tinted(
        ConicalFrustum { radius_top: r_top, radius_bottom: r_bottom, height: h }
            .mesh()
            .resolution(res)
            .build()
            .translated_by(center),
        lin(c),
    )
}

/// An upright cone whose CENTRE sits at `center` (roof peaks, stakes).
pub(crate) fn cone_at(r: f32, h: f32, center: Vec3, res: u32, c: u32) -> Mesh {
    tinted(Cone { radius: r, height: h }.mesh().resolution(res).build().translated_by(center), lin(c))
}

/// Deterministic [0,1) value noise from a position (same sin-hash family as the scatter).
pub(crate) fn hash3(x: f32, y: f32, z: f32) -> f32 {
    let v = (x * 127.1 + y * 311.7 + z * 74.7).sin() * 43758.5453;
    v - v.floor()
}

/// **Surface weathering** — the texture pass. Run LAST (after [`flat_shaded`], which has
/// de-indexed the mesh so vertices come in per-triangle triples). Jitters each facet's colour
/// by a position-seeded noise so big stone/sand faces read mottled and grainy rather than as
/// flat plastic — the project's stand-in for a texture, staying pure vertex-colour so the
/// landmark still batches against the shared white material. `amount` ≈ peak ± fraction.
pub(crate) fn mottle(mut m: Mesh, amount: f32) -> Mesh {
    let pos: Vec<[f32; 3]> = match m.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.clone(),
        _ => return m,
    };
    if let Some(VertexAttributeValues::Float32x4(cols)) = m.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for t in 0..(cols.len() / 3) {
            let i = t * 3;
            // Per-facet centroid → one shade for the whole triangle (keeps crisp flat facets).
            let cx = (pos[i][0] + pos[i + 1][0] + pos[i + 2][0]) / 3.0;
            let cy = (pos[i][1] + pos[i + 1][1] + pos[i + 2][1]) / 3.0;
            let cz = (pos[i][2] + pos[i + 1][2] + pos[i + 2][2]) / 3.0;
            // Two octaves: coarse blotches (weather staining over ~1.4u patches) + fine grain.
            let coarse = hash3((cx * 0.7).floor(), (cy * 0.7).floor(), (cz * 0.7).floor());
            let fine = hash3(cx * 2.3, cy * 2.3, cz * 2.3);
            let n = coarse * 0.6 + fine * 0.4;
            // Biased a touch dark (weathering darkens more than it lightens).
            let f = 1.0 + (n - 0.58) * amount;
            for k in 0..3 {
                for ch in 0..3 {
                    cols[i + k][ch] = (cols[i + k][ch] * f).clamp(0.0, 1.0);
                }
            }
        }
    }
    m
}

/// A low-poly faceted lump (ico detail 0) — the angular chipped-stone look.
pub(crate) fn facet_at(r: f32, off: Vec3, squash: f32, c: u32) -> Mesh {
    facet_tinted(r, off, squash, lin(c))
}

/// Like [`facet_at`] but with an explicit linear RGBA (for baked shadow/highlight scales).
pub(crate) fn facet_tinted(r: f32, off: Vec3, squash: f32, c: [f32; 4]) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(1.0, squash, 1.0))
            .translated_by(off),
        c,
    )
}

/// A faceted lump stretched on X/Y/Z and Z-tilted into a jagged block.
#[allow(dead_code)] // part of the landmark-builder toolkit; not every model uses every primitive
pub(crate) fn block_at(rx: f32, ry: f32, rz: f32, off: Vec3, tilt: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(1.0)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(rx, ry, rz))
            .rotated_by(Quat::from_rotation_z(tilt))
            .translated_by(off),
        lin(c),
    )
}

/// `block_at` with an extra yaw so fracture slabs can lean in any direction.
pub(crate) fn slab_at(rx: f32, ry: f32, rz: f32, off: Vec3, yaw: f32, tilt: f32, c: u32) -> Mesh {
    tinted(
        Sphere::new(1.0)
            .mesh()
            .ico(0)
            .expect("ico detail in range")
            .scaled_by(Vec3::new(rx, ry, rz))
            .rotated_by(Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tilt))
            .translated_by(off),
        lin(c),
    )
}

/// Centre height that grounds a Z-tilted slab's lowest point at y=0.
pub(crate) fn slab_ground(rx: f32, ry: f32, tilt: f32) -> f32 {
    ((rx * tilt.sin()).powi(2) + (ry * tilt.cos()).powi(2)).sqrt()
}

/// A flat lichen splotch pressed onto a stone surface.
pub(crate) fn lichen_at(r: f32, off: Vec3, c: u32) -> Mesh {
    facet_at(r, off, 0.24, c)
}

/// An angular rock chunk: squashed icosphere, yawed + pitched (biome_snow `chunk_at`).
pub(crate) fn chunk_at(r: f32, off: Vec3, scale: Vec3, yaw: f32, pitch: f32, detail: u32, c: u32) -> Mesh {
    tinted(
        Sphere::new(r)
            .mesh()
            .ico(detail)
            .expect("ico detail in range")
            .scaled_by(scale)
            .rotated_by(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch))
            .translated_by(off),
        lin(c),
    )
}

/// A flat cairn stone — thin cuboid slab yawed about Y.
pub(crate) fn flat_stone(w: f32, h: f32, d: f32, off: Vec3, yaw: f32, c: u32) -> Mesh {
    tinted(
        Cuboid::new(w, h, d)
            .mesh()
            .build()
            .rotated_by(Quat::from_rotation_y(yaw))
            .translated_by(off),
        lin(c),
    )
}

/// A cylinder primitive whose BASE sits at the local origin, then optionally pitched
/// about Z and yawed about Y, then translated to its attach point. Order: lift the
/// base-centred cylinder so its base is at origin → rotate (so it swings about its
/// base) → translate to where it attaches. `resolution` keeps the low-poly facet look.
pub(crate) fn limb(radius: f32, height: f32, resolution: u32, pitch_z: f32, yaw_y: f32, attach: Vec3) -> Mesh {
    let rot = Quat::from_rotation_y(yaw_y) * Quat::from_rotation_z(pitch_z);
    Cylinder::new(radius, height)
        .mesh()
        .resolution(resolution)
        .build()
        .translated_by(Vec3::new(0.0, height * 0.5, 0.0)) // base → origin
        .rotated_by(rot)
        .translated_by(attach)
}

// ── The animated-set-piece API ─────────────────────────────────────────────────

/// A glowing sub-part: gets its OWN unlit emissive material (the beacon recipe) instead of the
/// shared white one. `pulse = Some((freq rad/s, ±fraction, phase))` breathes the emissive.
pub struct Glow {
    pub color: Color,
    /// Emissive multiplier (the landmark beacons run ~16; set-piece accents want 3–9).
    pub strength: f32,
    pub pulse: Option<(f32, f32, f32)>,
}

/// The cosmetic motions a landmark sub-part can carry. All are pure `Transform` drives off
/// wall-clock time — deterministic, no state, safe to run ungated.
pub enum Fx {
    /// Static part (used for glow-only parts like lit windows).
    None,
    /// Continuous rotation about a LOCAL axis (mill sails, coin-spinning relics).
    Spin { axis: Vec3, rate: f32 },
    /// Pendulum sway about a local axis through the part's origin (charms, lanterns, vanes).
    Sway { axis: Vec3, amp: f32, freq: f32, phase: f32 },
    /// Circle a model-local centre (the part's rest offset from `centre` sets the radius),
    /// with an optional vertical bob (orbiting ice shards, cauldron wisps).
    Orbit { centre: Vec3, rate: f32, bob_amp: f32, bob_freq: f32, phase: f32 },
    /// Levitate in place: vertical bob + slow yaw spin (hovering keystones, sun-disc relics).
    Hover { bob_amp: f32, bob_freq: f32, spin_rate: f32, phase: f32 },
    /// Loop upward over `range` then wrap, drifting sideways and swelling as it climbs
    /// (chimney smoke). `grow` = extra scale at the top of the run.
    Rise { rate: f32, range: f32, drift: Vec2, grow: f32, phase: f32 },
}

/// One animated / glowing sub-part of a landmark: its own child entity with its own mesh.
/// Everything static belongs in the merged base mesh instead (one draw, batches).
pub struct AnimPart {
    pub mesh: Mesh,
    pub xf: Transform,
    pub fx: Fx,
    pub glow: Option<Glow>,
    /// < 1.0 → translucent unlit part (smoke); ignored when `glow` is set.
    pub alpha: f32,
}

impl AnimPart {
    /// A solid vertex-coloured part on the shared white material.
    pub fn solid(mesh: Mesh, xf: Transform, fx: Fx) -> Self {
        Self { mesh, xf, fx, glow: None, alpha: 1.0 }
    }
    /// A glowing part (own unlit emissive material).
    pub fn glowing(mesh: Mesh, xf: Transform, fx: Fx, glow: Glow) -> Self {
        Self { mesh, xf, fx, glow: Some(glow), alpha: 1.0 }
    }
}

/// A complete landmark: one merged static base mesh + the animated sub-parts.
pub struct LandmarkModel {
    pub base: Mesh,
    pub parts: Vec<AnimPart>,
}

/// Rest transform captured at spawn — the FX systems pose relative to this every frame.
#[derive(Component)]
struct RestXf(Transform);

#[derive(Component)]
struct FxSpin {
    axis: Vec3,
    rate: f32,
}

#[derive(Component)]
struct FxSway {
    axis: Vec3,
    amp: f32,
    freq: f32,
    phase: f32,
}

#[derive(Component)]
struct FxOrbit {
    centre: Vec3,
    rate: f32,
    bob_amp: f32,
    bob_freq: f32,
    phase: f32,
}

#[derive(Component)]
struct FxHover {
    bob_amp: f32,
    bob_freq: f32,
    spin_rate: f32,
    phase: f32,
}

#[derive(Component)]
struct FxRise {
    rate: f32,
    range: f32,
    drift: Vec2,
    grow: f32,
    phase: f32,
}

/// Breathes a glow part's emissive. Each pulsing part owns a UNIQUE material instance,
/// so mutating it here can't bleed into other parts.
#[derive(Component)]
struct FxPulse {
    mat: Handle<StandardMaterial>,
    emissive: LinearRgba,
    freq: f32,
    amp: f32,
    phase: f32,
}

/// Spawn a [`LandmarkModel`] at `xf`: the base mesh on `white` (the scene's shared
/// vertex-colour material) with each [`AnimPart`] as a child (children inherit the root
/// scale/yaw, so parts are authored in the same model-local space as the base). Returns the
/// root entity — callers add their own tags (`BiomeEntity`, the POI attach, …).
pub fn spawn_landmark_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    white: &Handle<StandardMaterial>,
    model: LandmarkModel,
    xf: Transform,
) -> Entity {
    spawn_landmark_model_parts(commands, meshes, materials, white, model, xf).0
}

/// [`spawn_landmark_model`], also returning the animated part entities in `model.parts` order.
pub fn spawn_landmark_model_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    white: &Handle<StandardMaterial>,
    model: LandmarkModel,
    xf: Transform,
) -> (Entity, Vec<Entity>) {
    let mut part_ids = Vec::new();
    let root = commands
        .spawn((Mesh3d(meshes.add(model.base)), MeshMaterial3d(white.clone()), xf, Visibility::Inherited))
        .id();
    for part in model.parts {
        // Material: glow → own unlit emissive (beacon recipe); translucent → own blend
        // material; otherwise the shared white.
        let mut pulse = None;
        let (mat, shadowless) = if let Some(g) = &part.glow {
            let emissive = LinearRgba::from(g.color) * g.strength;
            let handle = materials.add(StandardMaterial {
                base_color: g.color,
                emissive,
                unlit: true,
                ..default()
            });
            if let Some((freq, amp, phase)) = g.pulse {
                pulse = Some(FxPulse { mat: handle.clone(), emissive, freq, amp, phase });
            }
            (handle, true)
        } else if part.alpha < 1.0 {
            let handle = materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 1.0, 1.0, part.alpha),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                ..default()
            });
            (handle, true)
        } else {
            (white.clone(), false)
        };
        let mesh = meshes.add(part.mesh);
        commands.entity(root).with_children(|p| {
            let mut e = p.spawn((Mesh3d(mesh), MeshMaterial3d(mat), part.xf, RestXf(part.xf)));
            part_ids.push(e.id());
            if shadowless {
                e.insert(NotShadowCaster);
            }
            match part.fx {
                Fx::None => {}
                Fx::Spin { axis, rate } => {
                    e.insert(FxSpin { axis, rate });
                }
                Fx::Sway { axis, amp, freq, phase } => {
                    e.insert(FxSway { axis, amp, freq, phase });
                }
                Fx::Orbit { centre, rate, bob_amp, bob_freq, phase } => {
                    e.insert(FxOrbit { centre, rate, bob_amp, bob_freq, phase });
                }
                Fx::Hover { bob_amp, bob_freq, spin_rate, phase } => {
                    e.insert(FxHover { bob_amp, bob_freq, spin_rate, phase });
                }
                Fx::Rise { rate, range, drift, grow, phase } => {
                    // Rising parts are smoke puffs: soft round sprites kept facing the camera.
                    e.insert((FxRise { rate, range, drift, grow, phase }, crate::particles::FaceCamera));
                }
            }
            if let Some(p) = pulse {
                e.insert(p);
            }
        });
    }
    (root, part_ids)
}

// ── The cosmetic animators ─────────────────────────────────────────────────────

fn fx_spin(time: Res<Time>, mut q: Query<(&FxSpin, &RestXf, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (s, rest, mut xf) in &mut q {
        xf.rotation = rest.0.rotation * Quat::from_axis_angle(s.axis, (t * s.rate) % TAU);
    }
}

fn fx_sway(time: Res<Time>, mut q: Query<(&FxSway, &RestXf, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (s, rest, mut xf) in &mut q {
        // A touch of second-harmonic so the pendulum reads hand-touched, not metronomic.
        let a = (t * s.freq + s.phase).sin() + 0.25 * (t * s.freq * 2.7 + s.phase).sin();
        xf.rotation = rest.0.rotation * Quat::from_axis_angle(s.axis, a * s.amp);
    }
}

fn fx_orbit(time: Res<Time>, mut q: Query<(&FxOrbit, &RestXf, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (o, rest, mut xf) in &mut q {
        let rel = rest.0.translation - o.centre;
        let swing = Quat::from_rotation_y(t * o.rate + o.phase);
        let bob = o.bob_amp * (t * o.bob_freq + o.phase * 1.7).sin();
        xf.translation = o.centre + swing * rel + Vec3::Y * bob;
        // Shards tumble gently as they circle.
        xf.rotation = rest.0.rotation * Quat::from_rotation_y(t * o.rate * 0.6);
    }
}

fn fx_hover(time: Res<Time>, mut q: Query<(&FxHover, &RestXf, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (h, rest, mut xf) in &mut q {
        xf.translation = rest.0.translation + Vec3::Y * (h.bob_amp * (t * h.bob_freq + h.phase).sin());
        xf.rotation = rest.0.rotation * Quat::from_rotation_y((t * h.spin_rate + h.phase) % TAU);
    }
}

fn fx_rise(time: Res<Time>, mut q: Query<(&FxRise, &RestXf, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (r, rest, mut xf) in &mut q {
        let p = (t * r.rate + r.phase).rem_euclid(r.range);
        let f = p / r.range;
        xf.translation =
            rest.0.translation + Vec3::new(r.drift.x * f, p, r.drift.y * f);
        xf.scale = rest.0.scale * (0.55 + r.grow * f);
    }
}

fn fx_pulse(
    time: Res<Time>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    q: Query<(&FxPulse, &ViewVisibility)>,
    mut last_step: Local<i64>,
) {
    // 10 Hz, not every frame: ANY StandardMaterial change in a frame makes Bevy's `AssetChanged`
    // filters walk every StandardMaterial mesh entity on the island (specialization checks for the
    // main, prepass and shadow phases); with no change they short-circuit. These glows breathe at
    // <1 rad/s, so a 0.1 s step is a ~2% brightness change — invisible.
    let step = (time.elapsed_secs_f64() * 10.0).floor() as i64;
    if step == *last_step {
        return;
    }
    *last_step = step;
    let t = step as f32 * 0.1;
    // Only parts on screen: every landmark glow on the island used to rewrite its material each
    // frame (`get_mut` re-uploads it and makes Bevy re-check every mesh entity for specialization).
    for (p, vis) in &q {
        if !vis.get() {
            continue;
        }
        if let Some(mut m) = mats.get_mut(&p.mat) {
            m.emissive = p.emissive * (1.0 + (t * p.freq + p.phase).sin() * p.amp);
        }
    }
}

/// Registers the landmark part animators. Added by the game AND the standalone model viewer,
/// so `FOREST_VIEW=landmark:mill` previews the same motion the island renders.
pub struct RuinsFxPlugin;

impl Plugin for RuinsFxPlugin {
    fn build(&self, app: &mut App) {
        // Ungated (render-side dressing, like banner flutter): the mill keeps turning and the
        // witch-lights keep breathing while a shop panel freezes the sim.
        app.add_systems(Update, (fx_spin, fx_sway, fx_orbit, fx_hover, fx_rise, fx_pulse));
    }
}

// ── Per-biome placement (combined world map) ─────────────────────────────────────

/// A chosen landmark spot, picked once from the TERRAIN (before scatter) so both the landmark
/// placement here and the road network (`roads::build_curves`) can route to the same point.
#[derive(Clone, Copy)]
pub struct LandmarkSite {
    pub biome: crate::biome::Biome,
    /// World XZ.
    pub pos: Vec2,
    /// Ground Y at the spot, and the placed yaw.
    pub y: f32,
    pub yaw: f32,
    /// Footprint height-spread of the chosen spot (0 = dead flat); kept for the uneven-spot warning.
    pub spread: f32,
}

/// (biome, scale, footprint_radius) for each landmark. KEEP IN SYNC with the `specs` table in
/// [`populate_landmarks`] — same scale per biome (that table carries the model + blockers).
/// Each landmark is its biome's skyline FLAG — it has to break the treeline from the road, or
/// the spur leading to it pulls nobody.
/// `(biome, x, z, yaw)` landmarks held at a fixed world spot instead of searched (see `landmark_sites`).
const PINNED_SITES: [(crate::biome::Biome, f32, f32, f32); 3] = [
    (crate::biome::Biome::Snow, -137.60056, -96.46046, 6.1674943),
    (crate::biome::Biome::Forest, -92.89177, 66.46597, 5.913733),
    (crate::biome::Biome::Swamp, -14.323242, 178.35394, 1.9473338),
];

const SITE_PARAMS: [(crate::biome::Biome, f32, f32); 5] = [
    // Snow / Desert: the Blender ice-sheathed pinnacle in its snowfield and the monumental pyramid
    // in its dunes (landmarks.glb) need far more room than the old low-poly pieces did.
    (crate::biome::Biome::Snow, 1.5, 3.5),
    (crate::biome::Biome::Desert, 1.5, 9.0),
    (crate::biome::Biome::Rocky, 1.45, 2.4),
    (crate::biome::Biome::Forest, 1.45, 1.5),
    (crate::biome::Biome::Swamp, 1.4, 1.5),
];

/// Extra collision-free apron around each landmark's real footprint. Scatter can still place
/// walk-through bushes, pebbles and ground cover here; only trunks / blocking boulders stay out.
const LANDMARK_COLLISION_APRON: f32 = 4.0;
/// Tiny visual core that stays empty so scatter does not poke straight through the landmark mesh.
const LANDMARK_VISUAL_APRON: f32 = 0.75;

fn near_site_with_apron(x: f32, z: f32, apron: f32) -> bool {
    let p = Vec2::new(x, z);
    landmark_sites().iter().any(|site| {
        SITE_PARAMS
            .iter()
            .find(|(biome, _, _)| *biome == site.biome)
            .is_some_and(|(_, scale, foot_r)| {
                let r = foot_r * scale + apron;
                site.pos.distance_squared(p) < r * r
            })
    })
}

pub fn near_landmark_collision_buffer(x: f32, z: f32) -> bool {
    near_site_with_apron(x, z, LANDMARK_COLLISION_APRON)
}

pub fn near_landmark_visual_footprint(x: f32, z: f32) -> bool {
    near_site_with_apron(x, z, LANDMARK_VISUAL_APRON)
}

/// The five landmark spots, chosen once (flattest valid candidate per biome) and cached. Decoupled
/// from `blockers` — those aren't populated this early (the road field bakes at the ground pass,
/// long before scatter) — so spots shifted slightly vs. the old post-scatter search; the
/// `LANDMARK_CLEAR_R` sweep fells any scatter that ends up under a landmark anyway. The fortress
/// blob is excluded by radius (its forced-flat plateau would otherwise lure the swamp landmark).
pub fn landmark_sites() -> &'static [LandmarkSite] {
    static SITES: OnceLock<Vec<LandmarkSite>> = OnceLock::new();
    SITES.get_or_init(|| {
        let mut rng: u32 = 0x1a2b_3c4d;
        let mut out = Vec::new();
        for (biome, scale, foot_r) in SITE_PARAMS {
            let probe_r = foot_r * scale;
            // The spire, the mill and the witch hut are pinned where the search left them before the
            // Wreck Cove pass: every search samples the whole island and only draws a yaw for the
            // samples it accepts, so ANY terrain edit (the cove's bay, the smoothed sea cut on the
            // snow coast) re-deals the draws and sends the landmark across the map — off the roads,
            // the river bridge and the wild sites that are laid round it. The spire still replays
            // its legacy draws so the rocks after it keep their stream; swamp is the last search.
            if let Some(&(_, x, z, yaw)) = PINNED_SITES.iter().find(|p| p.0 == biome) {
                if let Some(spread) = footprint_spread(x, z, probe_r) {
                    let ground = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
                    let y = if biome == crate::biome::Biome::Snow {
                        rng = legacy_snow_rng_end(rng, probe_r);
                        footprint_low(x, z, probe_r * 0.55).unwrap_or(ground)
                    } else {
                        ground
                    };
                    out.push(LandmarkSite { biome, pos: Vec2::new(x, z), y, yaw, spread });
                    continue;
                }
                warn!("landmark: pinned {:?} spot is no longer clear — searching", biome);
            }
            // The spire's search changed (real snow), but the four landmarks after it share this rng:
            // replay the OLD spire search's draws so they keep their spots (roads / sites are laid
            // round them), then restore that stream once the new search is done.
            let resume = (biome == crate::biome::Biome::Snow).then(|| legacy_snow_rng_end(rng, probe_r));
            let mut best: Option<LandmarkSite> = None;
            let mut best_score: Option<f32> = None;
            for _ in 0..4000 {
                let x = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GX + 6.0, crate::worldmap::GX - 6.0);
                let z = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GZ + 6.0, crate::worldmap::GZ - 6.0);
                if crate::worldmap::biome_at_world(x, z) != Some(biome)
                    || crate::worldmap::ground_at_world(x, z).is_none()
                    || crate::camps::in_clearing(x, z)
                    || crate::castle::in_footprint(x, z)
                    || crate::rival::near_fort(x, z)
                    || Vec2::new(x, z).distance(crate::ork_fortress::CENTRE) < 30.0
                    // Mesa shelves are flat (the flatness grade LOVES them) but sit behind
                    // sheer walls — a landmark up there is unreachable from its road spur. The snow
                    // massif reads as "shelf" all the way up past the snow line, so the spire skips
                    // this test (its score keeps it low on the massif instead, see below).
                    || (biome != crate::biome::Biome::Snow && crate::worldmap::cliff_shelf_world(x, z))
                {
                    continue;
                }
                // Reject any footprint that runs off-land / over water, then grade by flatness.
                let Some(spread) = footprint_spread(x, z, probe_r) else { continue };
                let mut y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
                let yaw = crate::wildlife::rng_range(&mut rng, 0.0, TAU);
                // The frozen spire belongs up in the snow, not on the green valley floor (the old soft
                // penalty still let a dusted valley spot win: it stood on grass in a white disc).
                // Real snow is altitude-bound, and the massif is never dead flat up there, so the
                // flatness test is relaxed instead: any spot under full cover (snow over the WHOLE
                // footprint) beats every thinner one; among those the flattest wins, and the spire is
                // seated on the footprint's low side so its drifts bank against the slope, never float.
                let score = spread
                    + if biome == crate::biome::Biome::Snow {
                        let c = snow_footprint_min(x, z, probe_r);
                        y = footprint_low(x, z, probe_r * 0.55).unwrap_or(y);
                        (0.9 - c).max(0.0) * 6.0 + if c < crate::alpine::SOLID_SNOW { 100.0 } else { 0.0 } + 0.04 * y
                    } else {
                        0.0
                    };
                if best_score.map_or(true, |b| score < b) {
                    best_score = Some(score);
                    best = Some(LandmarkSite { biome, pos: Vec2::new(x, z), y, yaw, spread });
                }
                if best_score.is_some_and(|b| b <= 0.01) {
                    break; // dead flat (and snowy enough) — done
                }
            }
            if let Some(r) = resume {
                rng = r;
            }
            // The pyramid: the flattest spot was on the desert's rim, under the rock range's walls.
            // It belongs out in the OPEN desert, dunes on every side — a search of its own (its own
            // rng, so the landmarks after it keep their spots).
            if biome == crate::biome::Biome::Desert {
                if let Some(s) = open_desert_site(probe_r) {
                    best = Some(s);
                }
            }
            match best {
                Some(s) => out.push(s),
                None => info!("landmark: no spot found for {:?}", biome),
            }
        }
        out
    })
}

pub fn populate_landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    use crate::biome::{Biome, BiomeEntity};
    let white = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.92, ..default() });
    // (biome, model, scale, blocker OBBs) — one landmark each. Blockers are `(dx, dz, hw, hd)` in
    // MODEL-LOCAL units (rotated into place with the placed yaw, scaled to world): most landmarks
    // block one box hugging the built mass, but the standing stones block PER-STONE so the circle
    // itself stays walkable (the rune trial plays out inside it).
    let specs: [(Biome, LandmarkModel, f32, Vec<(f32, f32, f32, f32)>); 5] = [
        (Biome::Snow, crate::landmark_models::frozen_spire(), 1.5, vec![(0.0, 0.0, 1.5, 1.5), (0.0, 2.2, 1.6, 0.8)]),
        (Biome::Desert, crate::landmark_models::sunken_pyramid(), 1.5, vec![(0.0, 0.0, 6.9, 6.9), (4.6, -4.6, 3.0, 3.0)]),
        (Biome::Rocky, crate::landmark_models::standing_stones(), 1.45, crate::landmark_models::stone_blockers()),
        (Biome::Forest, crate::landmark_models::old_mill(), 1.45, vec![(0.0, 0.0, 1.5, 1.5)]),
        (Biome::Swamp, crate::landmark_models::witch_hut(), 1.4, vec![(0.0, 0.3, 1.25, 1.1), (1.7, 1.2, 0.55, 0.55)]),
    ];
    // Spots are pre-chosen from the terrain (see `landmark_sites`); here we just plant the model.
    for (biome, model, scale, obbs) in specs {
        let Some(s) = landmark_sites().iter().find(|s| s.biome == biome) else {
            continue; // `landmark_sites` already logged the miss.
        };
        let (x, y, z, yaw) = (s.pos.x, s.y, s.pos.y, s.yaw);
        // Greppable placement line — capture/debug sessions frame landmark close-ups from this.
        info!("landmark {:?} placed at ({:.1}, {:.1})", biome, x, z);
        let xf = Transform::from_xyz(x, y, z)
            .with_rotation(Quat::from_rotation_y(yaw))
            .with_scale(Vec3::splat(scale));
        let (id, parts) = spawn_landmark_model_parts(commands, meshes, materials, &white, model, xf);
        commands.entity(id).insert(BiomeEntity);
        // Realistic kit: the Blender body (same footprint, and the same faces / anchors the animated
        // parts sit on) replaces the low-poly base; solid moving parts that have a model swap too
        // (the mill's sail cross + vane, the stones' hovering keystone). Glows, wisps and smoke keep
        // their procedural parts. `landmarks.glb`: tools/blender_forest/landmarks_kit.py.
        let (body, part_swaps): (&str, &[(usize, &str)]) = match biome {
            Biome::Forest => ("mill_body", &[(0, "mill_sails"), (1, "lm_mill_vane")]),
            Biome::Snow => ("lm_frozen_spire", &[]),
            Biome::Desert => ("lm_sunken_pyramid", &[]),
            Biome::Rocky => ("lm_standing_stones", &[(crate::landmark_models::STONE_COUNT, "lm_keystone")]),
            Biome::Swamp => ("lm_witch_hut", &[]),
        };
        if let Some(k) = crate::realforest::get().filter(|k| k.has(body)) {
            let mut root = commands.entity(id);
            root.remove::<Mesh3d>();
            k.attach_static(&mut root, body, Transform::IDENTITY);
            for &(i, name) in part_swaps {
                if let (Some(&part), true) = (parts.get(i), k.has(name)) {
                    let mut s = commands.entity(part);
                    s.remove::<Mesh3d>();
                    k.attach_static(&mut s, name, Transform::IDENTITY);
                }
            }
            // Animated parts re-anchored onto the Blender body (`Some(pos)`) or dropped (`None`):
            // the spire loses its floating splinters (the heart sits in the frozen waterfall), the
            // pyramid its hovering gilt sun-disc (the glyph strips move onto the stair-side face).
            let moves: &[(usize, Option<Vec3>)] = match biome {
                Biome::Snow => &[
                    (0, Some(Vec3::new(0.0, 2.3, 1.42))),
                    (1, None), (2, None), (3, None), (4, None), (5, None), (6, None),
                ],
                Biome::Desert => &[(0, None), (1, Some(Vec3::new(-1.9, 3.35, 4.74))), (2, Some(Vec3::new(1.9, 3.35, 4.74)))],
                _ => &[],
            };
            for &(i, to) in moves {
                let Some(&part) = parts.get(i) else { continue };
                match to {
                    Some(p) => {
                        let tf = Transform::from_translation(p);
                        commands.entity(part).insert((tf, RestXf(tf)));
                    }
                    None => commands.entity(part).despawn(),
                }
            }
        }
        // Solid oriented boxes, offsets rotated by the placed yaw + scaled into world units.
        let (sin, cos) = yaw.sin_cos();
        for (dx, dz, hw, hd) in obbs {
            let (wx, wz) = ((dx * cos + dz * sin) * scale, (-dx * sin + dz * cos) * scale);
            crate::blockers::add_obb(x + wx, z + wz, hw * scale, hd * scale, yaw);
        }
        crate::landmarks::attach(commands, id, biome, Vec3::new(x, y, z), meshes, materials);
        if s.spread > crate::worldmap::GROUND_STEP {
            info!("landmark {:?}: best spot still uneven (spread {:.2})", biome, s.spread);
        }
    }
}

/// Screenshot cameras on every landmark (`FOREST_SHOT_STORIES=landmarks`, see `capture.rs`): an
/// oblique from the south-east and a lower one from the north-west, framed by the footprint.
pub fn shot_cams() -> Vec<(String, Transform)> {
    let mut out = Vec::new();
    for s in landmark_sites() {
        let Some(&(_, scale, foot)) = SITE_PARAMS.iter().find(|p| p.0 == s.biome) else { continue };
        let r = foot * scale;
        let aim = Vec3::new(s.pos.x, s.y + r * 0.5, s.pos.y);
        let name = format!("{:?}", s.biome).to_lowercase();
        for (tag, off) in [("a", Vec3::new(1.0, 0.55, -1.0)), ("b", Vec3::new(-1.1, 0.3, 0.9))] {
            let eye = aim + off * (r * 1.6 + 6.0);
            info!("landmark cam {name}_{tag}: {:.1},{:.1},{:.1} -> {:.1},{:.1},{:.1}", eye.x, eye.y, eye.z, aim.x, aim.y, aim.z);
            out.push((format!("lm_{name}_{tag}"), Transform::from_translation(eye).looking_at(aim, Vec3::Y)));
        }
    }
    out
}

/// The rng state the pre-2026-10-12 spire search left behind (same draws, filters and early
/// break), so the landmarks seated after it keep their long-standing spots.
fn legacy_snow_rng_end(mut rng: u32, probe_r: f32) -> u32 {
    let mut best: Option<f32> = None;
    for _ in 0..4000 {
        let x = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GX + 6.0, crate::worldmap::GX - 6.0);
        let z = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GZ + 6.0, crate::worldmap::GZ - 6.0);
        if crate::worldmap::biome_at_world(x, z) != Some(crate::biome::Biome::Snow)
            || crate::worldmap::ground_at_world(x, z).is_none()
            || crate::camps::in_clearing(x, z)
            || crate::castle::in_footprint(x, z)
            || crate::rival::near_fort(x, z)
            || Vec2::new(x, z).distance(crate::ork_fortress::CENTRE) < 30.0
            || crate::worldmap::cliff_shelf_world(x, z)
        {
            continue;
        }
        let Some(spread) = footprint_spread(x, z, probe_r) else { continue };
        let _yaw = crate::wildlife::rng_range(&mut rng, 0.0, TAU);
        let c = crate::alpine::snow_cover(x, z);
        let score = spread + (0.75 - c).max(0.0) * 4.0 + if c < 0.45 { 100.0 } else { 0.0 };
        if best.map_or(true, |b| score < b) {
            best = Some(score);
        }
        if best.is_some_and(|b| b <= 0.01) {
            break;
        }
    }
    rng
}

/// How far (world units, up to `cap`) the desert runs on every side of `(x, z)`: the nearest
/// non-desert / off-land / water sample on 24 bearings, stepping out 4u at a time.
fn desert_reach(x: f32, z: f32, cap: f32) -> f32 {
    let mut reach = cap;
    for k in 0..24 {
        let a = k as f32 * (TAU / 24.0);
        let mut r = 4.0;
        while r < reach {
            let (px, pz) = (x + a.cos() * r, z + a.sin() * r);
            if crate::worldmap::biome_at_world(px, pz) != Some(crate::biome::Biome::Desert)
                || crate::worldmap::ground_at_world(px, pz).is_none()
                || crate::worldmap::is_river_world(px, pz)
                || crate::worldmap::is_pool_world(px, pz)
            {
                reach = r;
                break;
            }
            r += 4.0;
        }
    }
    reach
}

/// Open desert keeps this far from every biome border (the rock range's walls above all).
const PYRAMID_OPEN: f32 = 52.0;

/// The pyramid's spot: deep in the desert ([`PYRAMID_OPEN`] of sand on every side, so the dunes
/// frame it and no mountain wall stands behind it), on a low, gentle pan, clear of the Pasha's fort
/// and the raiders' camp. Best = flattest, then the deepest in the sand.
fn open_desert_site(probe_r: f32) -> Option<LandmarkSite> {
    use crate::biome::Biome;
    let mut rng: u32 = 0x9e7a_51d3;
    let mut best: Option<(f32, LandmarkSite)> = None;
    let raiders = crate::camps::plan().iter().filter(|c| crate::worldmap::biome_at_world(c.centre.x, c.centre.y) == Some(Biome::Desert)).map(|c| c.centre).collect::<Vec<_>>();
    for _ in 0..6000 {
        let x = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GX + 6.0, crate::worldmap::GX - 6.0);
        let z = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GZ + 6.0, crate::worldmap::GZ - 6.0);
        let yaw = crate::wildlife::rng_range(&mut rng, 0.0, TAU);
        let q = Vec2::new(x, z);
        if crate::worldmap::biome_at_world(x, z) != Some(Biome::Desert)
            || crate::worldmap::cliff_shelf_world(x, z)
            || crate::camps::in_clearing(x, z)
            || crate::rival::near_fort(x, z)
            || q.distance(crate::rival::RIVAL_CENTRE) < 55.0
            || raiders.iter().any(|c| c.distance(q) < 35.0)
        {
            continue;
        }
        let Some(spread) = footprint_spread(x, z, probe_r + 3.0) else { continue };
        if spread > 1.2 {
            continue;
        }
        let reach = desert_reach(x, z, PYRAMID_OPEN + 24.0);
        if reach < PYRAMID_OPEN {
            continue;
        }
        let score = spread - 0.015 * reach;
        if best.as_ref().map_or(true, |(b, _)| score < *b) {
            let y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
            best = Some((score, LandmarkSite { biome: Biome::Desert, pos: q, y, yaw, spread }));
        }
    }
    if best.is_none() {
        info!("landmark: no open-desert spot for the pyramid; keeping the flattest one");
    }
    best.map(|(_, s)| s)
}

/// Thinnest snow cover over a footprint (centre + a ring at `radius`): a spot is only "in the snow"
/// if the whole apron is, not just its centre.
fn snow_footprint_min(x: f32, z: f32, radius: f32) -> f32 {
    let mut c = crate::alpine::snow_cover(x, z);
    for i in 0..8 {
        let a = i as f32 * FRAC_PI_4;
        c = c.min(crate::alpine::snow_cover(x + a.cos() * radius, z + a.sin() * radius));
    }
    c
}

/// Lowest ground over the centre + a ring at `radius` (seat a landmark into a slope, not onto it).
fn footprint_low(x: f32, z: f32, radius: f32) -> Option<f32> {
    let mut lo = crate::worldmap::ground_at_world(x, z)?;
    for i in 0..8 {
        let a = i as f32 * FRAC_PI_4;
        lo = lo.min(crate::worldmap::ground_at_world(x + a.cos() * radius, z + a.sin() * radius)?);
    }
    Some(lo)
}

/// Terrain height spread (max−min world-Y) over a landmark's footprint: the centre plus two
/// rings of eight samples out to `radius`. Returns `None` if ANY sample is off-land/over water
/// (so a landmark never plants with part of its base over a cliff edge or the sea). A spread of
/// 0 means dead-flat ground; `GROUND_STEP` (0.5) is one terrace step.
fn footprint_spread(x: f32, z: f32, radius: f32) -> Option<f32> {
    let mut lo = crate::worldmap::ground_at_world(x, z)?;
    let mut hi = lo;
    for i in 0..8 {
        let a = i as f32 * FRAC_PI_4;
        let (c, s) = (a.cos(), a.sin());
        for m in [radius, radius * 0.55] {
            let g = crate::worldmap::ground_at_world(x + c * m, z + s * m)?;
            lo = lo.min(g);
            hi = hi.max(g);
        }
    }
    Some(hi - lo)
}

#[cfg(test)]
mod spire_tests {
    use super::*;

    /// The frozen spire stands in real snow (the whole drift apron under cover), never on the green
    /// valley floor of the snow region.
    #[test]
    fn frozen_spire_stands_in_snow() {
        let s = landmark_sites().iter().find(|s| s.biome == crate::biome::Biome::Snow).expect("spire site");
        let (mut n, mut snowy, mut maxy) = (0, 0, f32::MIN);
        let mut rng: u32 = 7;
        for _ in 0..20000 {
            let x = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GX, crate::worldmap::GX);
            let z = crate::wildlife::rng_range(&mut rng, -crate::worldmap::GZ, crate::worldmap::GZ);
            if crate::worldmap::biome_at_world(x, z) == Some(crate::biome::Biome::Snow) {
                n += 1;
                let y = crate::worldmap::ground_at_world(x, z).unwrap_or(-9.0);
                maxy = maxy.max(y);
                if crate::alpine::snow_cover(x, z) >= 0.6 {
                    snowy += 1;
                }
            }
        }
        let c = snow_footprint_min(s.pos.x, s.pos.y, 5.25);
        eprintln!("spire at {:?} y {:.2} spread {:.2} cover {c:.2}; snow samples {n}, snowy {snowy}, max y {maxy:.1}", s.pos, s.y, s.spread);
        assert!(c >= crate::alpine::SOLID_SNOW, "spire on bare ground (cover {c:.2})");
        // The landmarks after it keep their long-standing spots (roads and sites are laid round them).
        // The pyramid stands in open desert, sand on every side (no rock wall behind it).
        let d = landmark_sites().iter().find(|s| s.biome == crate::biome::Biome::Desert).expect("pyramid site");
        let reach = desert_reach(d.pos.x, d.pos.y, PYRAMID_OPEN + 24.0);
        eprintln!("pyramid at {:?} y {:.2} spread {:.2} desert reach {reach:.0}", d.pos, d.y, d.spread);
        assert!(reach >= PYRAMID_OPEN, "pyramid near a biome border (reach {reach:.0})");
    }
}
