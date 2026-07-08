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
                    e.insert(FxRise { rate, range, drift, grow, phase });
                }
            }
            if let Some(p) = pulse {
                e.insert(p);
            }
        });
    }
    root
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

fn fx_pulse(time: Res<Time>, mut mats: ResMut<Assets<StandardMaterial>>, q: Query<&FxPulse>) {
    let t = time.elapsed_secs();
    for p in &q {
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
const SITE_PARAMS: [(crate::biome::Biome, f32, f32); 5] = [
    (crate::biome::Biome::Snow, 1.5, 1.4),
    (crate::biome::Biome::Desert, 1.5, 2.4),
    (crate::biome::Biome::Rocky, 1.45, 2.4),
    (crate::biome::Biome::Forest, 1.45, 1.5),
    (crate::biome::Biome::Swamp, 1.4, 1.5),
];

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
            let mut best: Option<LandmarkSite> = None;
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
                    // sheer walls — a landmark up there is unreachable from its road spur.
                    || crate::worldmap::cliff_shelf_world(x, z)
                {
                    continue;
                }
                // Reject any footprint that runs off-land / over water, then grade by flatness.
                let Some(spread) = footprint_spread(x, z, probe_r) else { continue };
                let y = crate::worldmap::ground_at_world(x, z).unwrap_or(0.0);
                let yaw = crate::wildlife::rng_range(&mut rng, 0.0, TAU);
                if best.map_or(true, |b| spread < b.spread) {
                    best = Some(LandmarkSite { biome, pos: Vec2::new(x, z), y, yaw, spread });
                }
                if spread <= 0.01 {
                    break; // dead flat — done
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
        (Biome::Snow, crate::landmark_models::frozen_spire(), 1.5, vec![(0.0, 0.0, 1.0, 1.0)]),
        (Biome::Desert, crate::landmark_models::sunken_pyramid(), 1.5, vec![(0.0, 0.0, 2.1, 2.1)]),
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
        let id = spawn_landmark_model(commands, meshes, materials, &white, model, xf);
        commands.entity(id).insert(BiomeEntity);
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
