//! Screenplay **P.1 "Piorun" / The Lightning** — the second scene of the prologue
//! (`docs/superpowers/specs/2026-10-13-kampania-scenariusz-design.md` §P.1). A VIDEO rig like P.0
//! (`FOREST_SCENE=storm`, filmed under `FOREST_CLIP`, soundtrack mixed offline by
//! `tools/cutscene/storm_mix.py`) — no in-game trigger, skip or save hook.
//!
//! No dialogue, no score. Four pictures, each lit by its own lightning and cut out of black, then
//! the beach as the game will play it:
//! 1. **The deck.** Wat wakes in the dark to the hull splitting. A flash: the deck heeled over, the
//!    reef, the crew at the bell, the sea white beyond the rail.
//! 2. **The beach.** On his knees in the sand, coughing up the sea. A flash: orks coming out of
//!    the trees with torches. Then only the torches, getting closer.
//! 3. **Harrow.** A double flash: Ser Godric on his back in the sand, the axe beside him, an ork
//!    standing over him. Maud, off to the side, looking at Wat. Then nothing but rain.
//! 4. **The helmet.** His master's helmet in the sand, the rain on it; behind it, out of focus,
//!    Maud holding a plate to Wat's shoulder. The second flash pulls focus onto them.
//! 5. **Coda.** The knight standing in the surf in Harrow's armour, the first ork charging out of
//!    the dark; Maud, behind him: "Ser Godric's up! Behind him, all of you!" Cut on the clash.
//!
//! Filmed on the real Wreck Cove ([`crate::wreck_cove`]): the beached carrack stands in for the
//! ship on the reef (her deck is heeled, the camera rolls the rest), the beach and the treeline
//! beyond it are the game's own. The lightning is the moon: each flash swings the night's key light
//! round to that shot's bolt and throws it up for a ragged second (`farm_camera` applies [`light`]).

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use super::{cube, cylr, env, SceneAssets, SceneId, SceneProp, SceneState, IRON, WOOD, WOOD_DARK};
use crate::orks::OrkVariant;

/// Deep night.
pub(super) const SKY: f32 = 0.82;

/// Look-dev: `FOREST_STORM_DAY=1` films the same blocking at midday with the storm off (no dome,
/// no haze, no lightning light), to see where everything stands.
pub(super) fn day_dev() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("FOREST_STORM_DAY").is_some())
}
pub(super) const END: f32 = 59.6;

/// The shots, on the scene clock (s). Everything between them is black.
mod at {
    pub const DECK: (f32, f32) = (4.0, 7.7);
    pub const BEACH: (f32, f32) = (12.0, 20.6);
    /// Harrow's last stand, between Wat and them.
    pub const STAND: (f32, f32) = (21.6, 28.0);
    pub const HARROW: (f32, f32) = (28.6, 33.5);
    pub const HELM: (f32, f32) = (37.6, 44.3);
    pub const CODA: (f32, f32) = (46.6, 58.4);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Shot {
    Black,
    Deck,
    Beach,
    Stand,
    Harrow,
    Helm,
    Coda,
}

fn shot(t: f32) -> Shot {
    let inn = |(a, b): (f32, f32)| t >= a && t < b;
    match () {
        _ if inn(at::DECK) => Shot::Deck,
        _ if inn(at::BEACH) => Shot::Beach,
        _ if inn(at::STAND) => Shot::Stand,
        _ if inn(at::HARROW) => Shot::Harrow,
        _ if inn(at::HELM) => Shot::Helm,
        _ if inn(at::CODA) => Shot::Coda,
        _ => Shot::Black,
    }
}

/// The picture the set is dressed for at `t`: the shot on screen, or through the black the one
/// that comes next — actors, props and camera are in place (and the skins posed) before it cuts in.
fn staged(t: f32) -> Shot {
    match () {
        _ if t < at::DECK.1 => Shot::Deck,
        _ if t < at::BEACH.1 => Shot::Beach,
        _ if t < at::STAND.1 => Shot::Stand,
        _ if t < at::HARROW.1 => Shot::Harrow,
        _ if t < at::HELM.1 => Shot::Helm,
        _ => Shot::Coda,
    }
}

/// One moment inside each shot, for the clip warm-up's rehearsal (`scenes::rehearsal`).
pub(super) const REHEARSAL: &[f32] = &[5.0, 14.0, 19.5, 23.0, 25.5, 31.0, 41.0, 47.5, 51.5, 55.0];

/// The lightning: (time, strength). Each shot opens on its first strike (the picture cuts in on
/// the flash); the mix (`storm_mix.py`) puts the thunder under the same list.
pub(super) const STRIKES: &[(f32, f32)] = &[
    (4.05, 1.0),
    (5.95, 0.55),
    (12.05, 1.0),
    (16.6, 0.42),
    (20.22, 0.9),
    (21.62, 1.0),
    (22.62, 0.65),
    (23.35, 0.5),
    (24.95, 0.95),
    (25.75, 0.75),
    (28.65, 1.0),
    (30.9, 0.8),
    (37.65, 0.85),
    (40.9, 0.8),
    (46.65, 0.6),
    (50.0, 0.35),
    (56.7, 0.9),
];

/// Lightning brightness 0..1 at `t`: three ragged pulses and a slow afterglow — long enough for the
/// eye to read the picture it shows, short enough to be gone before it can look round it.
pub(super) fn flash(t: f32) -> f32 {
    STRIKES.iter().map(|&(t0, k)| k * pulse(t - t0)).fold(0.0, f32::max)
}

fn pulse(dt: f32) -> f32 {
    env(dt, &[(-0.01, 0.0), (0.02, 1.0), (0.08, 0.22), (0.13, 0.95), (0.2, 0.35), (0.29, 0.78), (0.46, 0.3), (0.75, 0.12), (1.4, 0.0)])
}

// ── The stage: the beach beside the wreck ─────────────────────────────────────────────────

/// The beach frame: `o` on the sand a little way up the beach from the carrack's bow, `inl` up the
/// beach toward the trees, `along` down the beach away from the wreck.
#[derive(Clone, Copy)]
struct Stage {
    o: Vec2,
    inl: Vec2,
    along: Vec2,
}

fn stage() -> Stage {
    let s = crate::wreck_cove::site();
    let (pos, yaw) = s.map_or((Vec2::new(-161.3, 140.8), 0.0), |s| (s.pos, s.yaw));
    let r = Quat::from_rotation_y(yaw);
    let inl = (r * Vec3::NEG_X).xz().normalize();
    let along = (r * Vec3::Z).xz().normalize();
    // the waterline under her midline, then down the beach past the bow and her boat
    Stage { o: pos + inl * 2.4 + along * 17.0, inl, along }
}

impl Stage {
    fn p(&self, i: f32, a: f32) -> Vec2 {
        self.o + self.inl * i + self.along * a
    }
    /// On the ground (or the sea) at `(i, a)`, `h` above it.
    fn at(&self, i: f32, a: f32, h: f32) -> Vec3 {
        let p = self.p(i, a);
        Vec3::new(p.x, ground(p) + h, p.y)
    }
    /// The world yaw facing along `(di, da)` in the beach frame.
    fn yaw(&self, di: f32, da: f32) -> f32 {
        let d = self.inl * di + self.along * da;
        d.x.atan2(d.y)
    }
}

fn ground(p: Vec2) -> f32 {
    crate::worldmap::ground_at_world(p.x, p.y).unwrap_or(crate::worldmap::SEA_Y)
}

/// A point on the carrack in her own model frame (−X inland, +Z the bow), `h` above her deck where
/// there is deck, else above her base.
fn ship_pt(mx: f32, mz: f32, h: f32) -> Vec3 {
    match crate::wreck_cove::deck_point(mx, mz) {
        Some(p) => p + Vec3::Y * h,
        None => crate::wreck_cove::site().map_or(Vec3::ZERO, |s| s.to_world(Vec3::new(mx, 1.6 + h, mz))),
    }
}

fn ship_yaw() -> f32 {
    crate::wreck_cove::site().map_or(0.0, |s| s.yaw)
}

// ── Components ──────────────────────────────────────────────────────────────────────────

#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(super) enum Role {
    /// A hand at the bell / the rail (index).
    Sailor(u8),
    Harrow,
    Maud,
    Wat,
}

/// Harrow's sword (put in his right hand by `harrow_sword` once his skin is ready).
#[derive(Component)]
pub(super) struct HarrowSword;

/// One of the orks: its index in the cast.
#[derive(Component)]
pub(super) struct StormOrk(u8);

#[derive(Component)]
pub(super) struct Bell;

#[derive(Component)]
pub(super) struct Bolt {
    /// The strike it belongs to (an index into [`STRIKES`]).
    strike: usize,
}

/// The storm's cloud dome round the camera, and the glow a strike throws on it.
#[derive(Component)]
pub(super) struct StormSky(Handle<StandardMaterial>);
#[derive(Component)]
pub(super) struct SkyGlow(Handle<StandardMaterial>);

#[derive(Component)]
pub(super) struct Rain {
    mat: Handle<StandardMaterial>,
    phase: f32,
}

#[derive(Component)]
pub(super) struct Flame {
    phase: f32,
    size: f32,
}

#[derive(Component)]
pub(super) struct FlameLight {
    base: f32,
    phase: f32,
}

/// A prop that only stands in one shot (hidden otherwise, so nothing from one picture leaks into
/// another's background).
#[derive(Component)]
pub(super) struct OnlyIn(Shot);

// ── Setup ───────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn setup(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    std_mats: &mut Assets<StandardMaterial>,
    assets: &SceneAssets,
    arm: &crate::orks::Armory,
) {
    use crate::npc_skin as ns;
    let st = stage();
    info!("storm: beach origin ({:.1}, {:.1}) inland {:?}", st.o.x, st.o.y, st.inl);

    // The cast: three hands at the bell, Harrow, Maud, Wat (P.0's stand-ins).
    for i in 0..3u8 {
        let e = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 61 + i as u32 * 11);
        commands.entity(e).insert((ns::SkinRequest(if i == 1 { ns::VILLAGER_M } else { ns::GUARD }), ns::SkinClip("Idle_Rail_Loop"), Role::Sailor(i)));
    }
    let harrow = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 21);
    commands.entity(harrow).insert((ns::SkinRequest(look("harrow", ns::GUARD)), ns::HairShade(2), ns::SkinClip("Death01"), Role::Harrow));
    let maud = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 47);
    commands.entity(maud).insert((ns::SkinRequest(ns::MAUD), ns::SkinClip("Idle_Loop"), Role::Maud, super::HeadLook::default()));
    let wat = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 33);
    commands.entity(wat).insert((ns::SkinRequest(look("wat", ns::ARCHER)), ns::HairShade(1), ns::SkinClip("Fixing_Kneeling"), Role::Wat));

    // The orks: six out of the trees (the beach), one standing over Harrow, and the first three
    // again for the coda's charge. The grunts and berserkers are the game's own torch-bearers
    // (`orks::Armory::spawn` picks them by seed; the seed is searched for one that carries).
    for i in 0..7u8 {
        let v = [OrkVariant::Grunt, OrkVariant::Berserker, OrkVariant::Grunt, OrkVariant::Scout, OrkVariant::Grunt, OrkVariant::Berserker, OrkVariant::Berserker][i as usize];
        let p = st.p(60.0, i as f32 * 3.0);
        let mut seed = 0x5701 + i as u32 * 977;
        while v != OrkVariant::Scout && (seed.wrapping_mul(2_654_435_761) >> 7) % 100 >= 35 {
            seed += 1;
        }
        let e = arm.spawn(commands, v, crate::orks::Faction::Red, p, p, seed);
        // (the coda's blows land for real -- blood on the sand -- but the fight is choreographed,
        // `drive` / `FIGHT`, and the HP bars are hidden for the film)
        commands.entity(e).insert((super::SceneActor, StormOrk(i)));
    }

    // The deck: a ship's bell on a yoke by the mainmast (swung by `drive_fx`).
    let bell_at = ship_pt(0.9, 1.3, 0.0);
    let yoke = commands
        .spawn((Transform::from_translation(bell_at).with_rotation(Quat::from_rotation_y(ship_yaw())), Visibility::default(), OnlyIn(Shot::Deck), SceneProp))
        .id();
    prop_child(commands, meshes, assets, yoke, yoke_mesh());
    let bell = commands.spawn((Transform::from_xyz(0.0, 1.62, 0.0), Visibility::default(), Bell)).id();
    prop_child(commands, meshes, assets, bell, ship_bell_mesh());
    commands.entity(yoke).add_child(bell);
    // A lantern swinging at the stern rail: the only light on deck between the flashes.
    let lamp = ship_pt(-1.6, -3.0, 1.9);
    commands.spawn((
        PointLight { color: Color::srgb(1.0, 0.6, 0.3), intensity: 30_000.0, range: 9.0, radius: 0.1, shadow_maps_enabled: true, ..default() },
        Transform::from_translation(lamp),
        FlameLight { base: 30_000.0, phase: 0.3 },
        OnlyIn(Shot::Deck),
        SceneProp,
    ));

    // Harrow's beach: the axe in the sand beside him, a dropped torch guttering on the sand.
    let axe_p = st.at(5.6, -0.6, 0.0);
    let axe = commands
        .spawn((
            Transform::from_translation(axe_p).with_rotation(Quat::from_rotation_y(st.yaw(0.3, 1.0)) * Quat::from_rotation_x(0.35)),
            Visibility::default(),
            OnlyIn(Shot::Harrow),
            SceneProp,
        ))
        .id();
    prop_child(commands, meshes, assets, axe, axe_mesh());
    let torch_p = st.at(6.4, 2.6, 0.05);
    let dropped = commands
        .spawn((
            // lying in the sand, the head propped a hand off it
            Transform::from_translation(torch_p).with_rotation(Quat::from_rotation_y(st.yaw(1.0, 0.4)) * Quat::from_rotation_z(-1.35)),
            Visibility::default(),
            OnlyIn(Shot::Harrow),
            SceneProp,
        ))
        .id();
    prop_child(commands, meshes, assets, dropped, torch_mesh());
    commands.spawn((
        Mesh3d(assets.flame_mesh.clone()),
        MeshMaterial3d(assets.flame_mat.clone()),
        Transform::from_translation(torch_p + Quat::from_rotation_y(st.yaw(1.0, 0.4)) * Quat::from_rotation_z(-1.35) * Vec3::new(0.0, 0.74, 0.0) + Vec3::Y * 0.06).with_scale(Vec3::splat(0.4)),
        bevy::light::NotShadowCaster,
        Flame { phase: 2.2, size: 0.4 },
        OnlyIn(Shot::Harrow),
        SceneProp,
    ));
    commands.spawn((
        PointLight { color: Color::srgb(1.0, 0.52, 0.2), intensity: 22_000.0, range: 8.0, radius: 0.1, shadow_maps_enabled: true, ..default() },
        Transform::from_translation(torch_p + Vec3::new(0.0, 0.35, 0.0)),
        FlameLight { base: 22_000.0, phase: 2.2 },
        OnlyIn(Shot::Harrow),
        SceneProp,
    ));

    // A torch guttering in the sand past the helmet: between the strikes the only light on it,
    // low and warm, catching the pair's legs behind.
    let gutter = st.at(HELM_I + 1.3, HELM_A + 0.9, 0.05);
    let gr = Quat::from_rotation_y(st.yaw(0.3, -1.0)) * Quat::from_rotation_z(-1.3);
    let g = commands.spawn((Transform::from_translation(gutter).with_rotation(gr), Visibility::default(), OnlyIn(Shot::Helm), SceneProp)).id();
    prop_child(commands, meshes, assets, g, torch_mesh());
    let gf = gutter + gr * Vec3::new(0.0, 0.74, 0.0) + Vec3::Y * 0.05;
    commands.spawn((
        Mesh3d(assets.flame_mesh.clone()),
        MeshMaterial3d(assets.flame_mat.clone()),
        Transform::from_translation(gf).with_scale(Vec3::splat(0.3)),
        bevy::light::NotShadowCaster,
        Flame { phase: 4.4, size: 0.3 },
        OnlyIn(Shot::Helm),
        SceneProp,
    ));
    commands.spawn((
        PointLight { color: Color::srgb(1.0, 0.5, 0.18), intensity: 9_000.0, range: 6.0, radius: 0.08, shadow_maps_enabled: true, ..default() },
        Transform::from_translation(gf + Vec3::Y * 0.2),
        FlameLight { base: 9_000.0, phase: 4.4 },
        OnlyIn(Shot::Helm),
        SceneProp,
    ));

    // The helmet in the sand (the real knight's helm, the one Wat will wear), lying on its side.
    if let Some(helm) = crate::player::spawn_part_prop(commands, meshes, images, creature_mats, "Head") {
        let p = st.at(HELM_I, HELM_A, -0.02);
        commands.entity(helm).insert((
            Name::new("storm_helm"),
            Transform::from_translation(p).with_rotation(Quat::from_rotation_y(st.yaw(-0.4, 1.0)) * Quat::from_rotation_z(1.25)),
            OnlyIn(Shot::Helm),
            SceneProp,
        ));
    }

    // The sky: a dome of low cloud just inside the night sky's (it hides the stars and the moon),
    // mottled dark grey-blue, lit up by every strike; the glow a strike throws on the cloud where
    // its bolt is; and the bolts themselves, for the shots that look at them.
    let sky_mat = std_mats.add(StandardMaterial { base_color: Color::WHITE, unlit: true, fog_enabled: false, cull_mode: None, ..default() });
    commands.spawn((
        Mesh3d(meshes.add(sky_mesh())),
        MeshMaterial3d(sky_mat.clone()),
        Transform::from_scale(Vec3::splat(SKY_R)),
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
        StormSky(sky_mat),
        SceneProp,
    ));
    let glow_mat = std_mats.add(StandardMaterial { base_color: Color::BLACK, unlit: true, fog_enabled: false, alpha_mode: AlphaMode::Add, cull_mode: None, ..default() });
    commands.spawn((
        Mesh3d(meshes.add(glow_mesh())),
        MeshMaterial3d(glow_mat.clone()),
        Transform::IDENTITY,
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
        SkyGlow(glow_mat),
        SceneProp,
    ));
    let bolt_mat = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(30.0, 33.0, 42.0),
        unlit: true,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    });
    for strike in [0usize, 2, 5, 13, 14] {
        commands.spawn((
            Mesh3d(meshes.add(bolt_mesh(strike as u32 * 13 + 7))),
            MeshMaterial3d(bolt_mat.clone()),
            Transform::IDENTITY,
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
            Bolt { strike },
            SceneProp,
        ));
    }

    // Rain round the camera: two stacked volumes falling through it.
    let rain_mat = std_mats.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    let sheet = meshes.add(rain_mesh());
    for i in 0..2 {
        commands.spawn((
            Mesh3d(sheet.clone()),
            MeshMaterial3d(rain_mat.clone()),
            Transform::IDENTITY,
            bevy::light::NotShadowCaster,
            Rain { mat: rain_mat.clone(), phase: i as f32 },
            SceneProp,
        ));
    }

    super::spawn_letterbox(commands);
}

fn look(name: &str, stand_in: u8) -> u8 {
    crate::npc_skin::LOOKS.iter().position(|l| *l == name).map_or(stand_in, |i| i as u8)
}

/// The helmet's spot on the beach (beach frame).
const HELM_I: f32 = 2.3;
const HELM_A: f32 = 2.4;

// ── Blocking ────────────────────────────────────────────────────────────────────────────

/// Where an actor stands in shot `s` at `t`: (world feet, yaw, clip, held playhead), or `None`
/// (not in this picture: hidden).
fn place(role: Role, s: Shot, t: f32) -> Option<(Vec3, f32, &'static str, Option<f32>)> {
    let st = stage();
    let sy = ship_yaw();
    match (role, s) {
        // The hands at the bell and the rail, braced against the heel.
        (Role::Sailor(0), Shot::Deck) => Some((ship_pt(0.55, 0.85, 0.0), sy + 0.9, "Idle_Rail_Call", None)),
        (Role::Sailor(1), Shot::Deck) => Some((ship_pt(1.45, 2.4, 0.0), sy + FRAC_PI_2 + 0.2, "Idle_Rail_Loop", None)),
        (Role::Sailor(2), Shot::Deck) => Some((ship_pt(1.25, -0.6, 0.0), sy + 1.3, "Idle_Rail_Call", Some(0.9))),
        // Out of the dark past Wat's shoulder at the end of the beach shot, between him and them.
        (Role::Harrow, Shot::Beach) => {
            let k = ((t - 18.7) / 1.1).clamp(0.0, 1.0);
            let p = st.p(-0.6, -2.2).lerp(st.p(2.0, -0.5), k * k * (3.0 - 2.0 * k));
            let clip = if k > 0.0 && k < 1.0 { "Walk_Loop" } else { "Sword_Idle" };
            Some((Vec3::new(p.x, ground(p), p.y), st.yaw(1.0, 0.15), clip, None))
        }
        // His last stand: kills the first, takes the second on the shield, the third catches him.
        (Role::Harrow, Shot::Stand) => {
            let (clip, seek) = harrow_stand(t);
            Some((st.at(HARROW_AT.0, HARROW_AT.1, 0.0), harrow_yaw(), clip, seek))
        }
        // On his back in the sand where he fell.
        (Role::Harrow, Shot::Harrow) => Some((st.at(HARROW_AT.0, HARROW_AT.1, 0.0), harrow_yaw(), "Death01", Some(1.88))),
        // Off to the side, looking at Wat (the camera).
        (Role::Maud, Shot::Harrow) => {
            let p = st.at(5.4, 4.6, 0.0);
            let cam = st.at(1.2, 0.8, 0.0);
            let d = (cam - p).xz();
            Some((p, d.x.atan2(d.y), "Idle_Loop", None))
        }
        // Behind the helmet: Wat on one knee, Maud at his shoulder with the plate.
        (Role::Wat, Shot::Helm) => Some((st.at(5.6, 4.3, 0.0), st.yaw(0.0, -1.0), "Fixing_Kneeling", Some(1.4 + 0.25 * (t * 0.9).sin().abs()))),
        (Role::Maud, Shot::Helm) => Some((st.at(5.75, 3.4, 0.0), st.yaw(-0.15, 1.0), "Interact", Some(0.55 + 0.05 * (t * 1.3).sin()))),
        _ => None,
    }
}

/// Where Harrow makes his stand (beach frame) and the way he faces: down the beach, the way they come.
const HARROW_AT: (f32, f32) = (5.2, 1.0);
const HARROW_FACE: (f32, f32) = (0.25, -1.0);

fn harrow_yaw() -> f32 {
    stage().yaw(HARROW_FACE.0, HARROW_FACE.1)
}

/// A point `d` in front of Harrow's mark and `side` to his right (beach frame).
fn before_harrow(d: f32, side: f32) -> (f32, f32) {
    let f = Vec2::new(HARROW_FACE.0, HARROW_FACE.1).normalize();
    let r = Vec2::new(f.y, -f.x);
    (HARROW_AT.0 + f.x * d + r.x * side, HARROW_AT.1 + f.y * d + r.y * side)
}

/// Harrow's stand, beat by beat (scene seconds): (clip, scrubbed playhead).
const STAND_BEATS: StandBeats = StandBeats { cut: 22.15, block: 23.35, hit: 24.9, fall: 25.55 };
struct StandBeats {
    cut: f32,
    block: f32,
    hit: f32,
    fall: f32,
}

fn harrow_stand(t: f32) -> (&'static str, Option<f32>) {
    let b = STAND_BEATS;
    match t {
        t if t < b.cut => ("Sword_Idle", None),
        t if t < b.block => ("Sword_Attack", Some((t - b.cut).min(1.18))),
        t if t < b.block + 0.95 => ("Sword_Block", Some((t - b.block).min(0.95))),
        t if t < b.hit => ("Sword_Idle", None),
        t if t < b.fall => ("Hit_Knockback", Some((t - b.hit).min(0.66))),
        t => ("Death01", Some((t - b.fall).min(1.88))),
    }
}

/// When an ork swings its club (`Ork::atk_anim`), and when one goes down (`Dying`), by cast index.
const ORK_SWINGS: &[(u8, f32)] = &[(6, 23.2), (4, 24.75), (6, 25.35), (0, 51.62), (1, 53.68), (2, 54.8), (5, 56.55)];
const ORK_DEATHS: &[(u8, f32)] = &[(3, 22.75), (0, 53.18), (1, 54.2), (2, 56.2), (5, 57.25)];

/// An ork's mark in shot `s` at `t`: (world XZ, facing, moving), or `None` (hidden).
fn ork_place(i: u8, s: Shot, t: f32) -> Option<(Vec2, f32, bool)> {
    let st = stage();
    let track = |keys: &[(f32, f32, f32)]| smooth_track(&st, keys, t, false);
    let charge = |keys: &[(f32, f32, f32)]| smooth_track(&st, keys, t, true);
    match (i, s) {
        // Out of the trees toward the camera (at the waterline): they come down the beach at a walk,
        // the second flash shows them where they are, and they charge — the lead one onto the lens.
        (0, Shot::Beach) => Some(track(&[(12.0, 19.5, -0.6), (16.4, 15.0, -0.4), (20.6, 4.6, -0.2)])),
        (1, Shot::Beach) => Some(track(&[(12.0, 21.0, 3.2), (16.4, 16.5, 2.8), (20.6, 6.5, 2.2)])),
        (2, Shot::Beach) => Some(track(&[(12.0, 20.5, -4.2), (16.6, 16.0, -3.8), (20.6, 6.0, -3.0)])),
        (3, Shot::Beach) => Some(track(&[(12.0, 23.5, 6.5), (16.5, 19.5, 6.0), (20.6, 11.0, 4.6)])),
        (4, Shot::Beach) => Some(track(&[(12.0, 23.0, -7.5), (16.6, 19.0, -7.0), (20.6, 12.0, -5.4)])),
        (5, Shot::Beach) => Some(track(&[(12.0, 25.5, 1.5), (16.4, 21.5, 1.4), (20.6, 14.0, 1.2)])),
        // The stand: the first onto his blade, the big one onto his shield, the third from his flank.
        (3, Shot::Stand) => Some(charge(&[(21.0, HARROW_AT.0 + 2.0, HARROW_AT.1 - 9.0), (22.25, before_harrow(1.25, 0.1).0, before_harrow(1.25, 0.1).1)])),
        (6, Shot::Stand) => Some(charge(&[(21.0, HARROW_AT.0 + 3.5, HARROW_AT.1 - 11.0), (23.05, before_harrow(1.35, -0.45).0, before_harrow(1.35, -0.45).1)])),
        (4, Shot::Stand) => Some(charge(&[(21.0, HARROW_AT.0 - 1.5, HARROW_AT.1 - 12.0), (24.55, before_harrow(0.9, 0.95).0, before_harrow(0.9, 0.95).1)])),
        // Standing over Harrow, his back to the trees.
        (6, Shot::Harrow) => {
            let p = st.p(6.5, 0.2);
            let d = st.p(1.2, 0.8) - p;
            Some((p, d.x.atan2(d.y) + 0.5, false))
        }
        // The coda's charge, at the knight in the surf.
        // one after another onto him: the first onto his shield, the rest as each falls
        (0, Shot::Coda) => Some(charge(&[(46.6, 25.0, -1.0), (51.5, KNIGHT.0 + KNIGHT_TO + 1.35, KNIGHT.1 - 0.15)])),
        (1, Shot::Coda) => Some(charge(&[(46.6, 27.0, 4.0), (53.55, KNIGHT.0 + KNIGHT_TO + 1.45, KNIGHT.1 + 0.9)])),
        (2, Shot::Coda) => Some(charge(&[(46.6, 30.0, -5.0), (54.65, KNIGHT.0 + KNIGHT_TO + 1.4, KNIGHT.1 - 0.9)])),
        (5, Shot::Coda) => Some(charge(&[(46.6, 33.0, 2.5), (56.4, KNIGHT.0 + KNIGHT_TO + 1.45, KNIGHT.1 + 0.7)])),
        _ => None,
    }
}

/// Along keyed marks `(time, inland, along)` (beach frame), held at the ends. The pace eases from
/// one leg's to the next over a second and a half round each key, so a walk breaks into the charge
/// (and the gait blends walk -> run) instead of switching on the spot.
fn smooth_track(st: &Stage, keys: &[(f32, f32, f32)], t: f32, stop: bool) -> (Vec2, f32, bool) {
    const R: f32 = 0.75;
    const DT: f32 = 1.0 / 120.0;
    let pts: Vec<Vec2> = keys.iter().map(|k| st.p(k.1, k.2)).collect();
    let lens: Vec<f32> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
    let total: f32 = lens.iter().sum();
    let speed = |tau: f32| {
        let leg = |i: usize| lens[i] / (keys[i + 1].0 - keys[i].0).max(1e-3);
        let mut v = leg(0);
        for k in 1..keys.len() - 1 {
            let u = ((tau - (keys[k].0 - R)) / (2.0 * R)).clamp(0.0, 1.0);
            v += (leg(k) - leg(k - 1)) * u * u * (3.0 - 2.0 * u);
        }
        v
    };
    let (t0, t1) = (keys[0].0, keys[keys.len() - 1].0);
    // `stop`: pulled up over the last stretch (into a fight), not halted dead from a run
    let brake = |tau: f32| if stop { (((t1 - tau) / 0.9).clamp(0.0, 1.0)).powf(0.7) } else { 1.0 };
    let (mut s_at, mut s_all, mut tau) = (0.0, 0.0, t0);
    while tau < t1 {
        let ds = speed(tau + DT * 0.5) * brake(tau + DT * 0.5) * DT;
        if tau < t {
            s_at += ds.min((t - tau) / DT * ds);
        }
        s_all += ds;
        tau += DT;
    }
    let mut d = s_at * total / s_all.max(1e-4);
    let mut i = 0;
    while i + 1 < lens.len() && d > lens[i] {
        d -= lens[i];
        i += 1;
    }
    let dir = (pts[i + 1] - pts[i]).normalize_or_zero();
    (pts[i] + dir * d.min(lens[i]), dir.x.atan2(dir.y), t > t0 && t < t1)
}

/// The knight's mark in the coda (beach frame), facing up the beach; he steps up to meet them.
const KNIGHT: (f32, f32) = (5.0, 0.0);
const KNIGHT_STEP: (f32, f32) = (48.9, 50.2);
const KNIGHT_TO: f32 = 1.3;

/// The coda's fight, on the scene clock: the first ork pulls up on him and swings, he takes it on
/// the shield and answers with the light chain (chop, slash, thrust), it goes down; then the next and
/// the next -- and from the second kill the camera rises and draws away over it.
struct Fight {
    side: f32,
    blocks: [(f32, f32); 2],
    swings: [f32; 10],
    /// The camera's draw-back, up and away over the fight.
    pull: (f32, f32),
}
const FIGHT: Fight = Fight {
    side: 50.72,
    blocks: [(51.5, 52.1), (54.75, 55.2)],
    swings: [52.15, 52.58, 53.0, 53.62, 54.06, 55.25, 55.65, 56.05, 56.7, 57.1],
    pull: (54.3, 58.4),
};

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<SceneState>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    mut actors: Query<(Entity, &mut crate::villagers::Villager, &mut Transform, &mut crate::npc_skin::SkinClip, &Role, &mut Visibility), Without<StormOrk>>,
    mut orks: Query<(Entity, &mut crate::orks::Ork, &mut Transform, &StormOrk, &mut Visibility, Has<crate::dying::Dying>), Without<Role>>,
    mut only: Query<(&OnlyIn, &mut Visibility), (Without<Role>, Without<StormOrk>)>,
    mut fade: Query<&mut BackgroundColor, With<super::FarmFade>>,
    (mut hero_q, mut strike): (Query<&mut crate::player::Hero>, ResMut<crate::player::combat::StagedStrike>),
    mut last_t: Local<f32>,
) {
    if state.active != Some(SceneId::Storm) {
        strike.coil = false;
        strike.block = false;
        return;
    }
    let t = state.clock.max(0.0);
    let prev = std::mem::replace(&mut *last_t, t);
    let crossed = |at: f32| prev < at && t >= at && t - prev < 0.5;
    let now = time.elapsed_secs();
    let s = staged(t);
    let st = stage();

    for (e, mut v, mut tf, mut clip, role, mut vis) in &mut actors {
        match place(*role, s, t) {
            Some((p, yaw, c, seek)) => {
                *vis = Visibility::Inherited;
                v.pos = p.xz();
                v.facing = yaw;
                v.moving = false;
                tf.translation = p;
                tf.rotation = Quat::from_rotation_y(yaw);
                if clip.0 != c {
                    clip.0 = c;
                }
                match seek {
                    Some(a) => commands.entity(e).try_insert(crate::npc_skin::ClipSeek(a)),
                    None => commands.entity(e).try_remove::<crate::npc_skin::ClipSeek>(),
                };
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (e, mut o, mut tf, k, mut vis, dying) in &mut orks {
        if dying {
            continue; // the death fade owns it now
        }
        if ORK_SWINGS.iter().any(|&(i, at)| i == k.0 && crossed(at)) {
            o.atk_anim = now;
        }
        if ORK_DEATHS.iter().any(|&(i, at)| i == k.0 && crossed(at)) {
            // falls away from whoever cut it down
            let by = if s == Shot::Stand { st.p(HARROW_AT.0, HARROW_AT.1) } else { st.p(KNIGHT.0 + KNIGHT_TO, KNIGHT.1) };
            crate::dying::begin_dying_struck(&mut commands, e, now, Vec2::new(tf.translation.x, tf.translation.z) - by, false);
            continue;
        }
        match ork_place(k.0, s, t) {
            Some((p, facing, moving)) => {
                *vis = Visibility::Inherited;
                let stepped = p.distance(o.pos) > 1e-4;
                o.pos = p;
                o.facing = facing;
                o.moving = moving && stepped;
                tf.translation = Vec3::new(p.x, ground(p), p.y);
                tf.rotation = Quat::from_rotation_y(facing);
            }
            None => {
                *vis = Visibility::Hidden;
                o.moving = false;
            }
        }
    }
    for (o, mut vis) in &mut only {
        *vis = if o.0 == s { Visibility::Inherited } else { Visibility::Hidden };
    }

    // The knight stands in the surf for the coda; until then he waits far down the beach.
    // In the coda he gets ready the way the game's knight does: the orks come out of the dark, he
    // draws sword and shield, steps up the beach to meet them, coils the Heavy and lets it go as
    // the first one reaches him. Till then he waits far down the beach, out of every shot.
    let coda = t >= at::CODA.0 - 0.5;
    let step = ((t - KNIGHT_STEP.0) / (KNIGHT_STEP.1 - KNIGHT_STEP.0)).clamp(0.0, 1.0);
    let ki = KNIGHT.0 + KNIGHT_TO * step * step * (3.0 - 2.0 * step);
    let (ki, ka) = if coda { (ki, KNIGHT.1) } else { (2.0, 60.0) };
    state.hero = Some((st.at(ki, ka, 0.0), st.yaw(1.0, 0.0)));
    let walking = coda && step > 0.0 && step < 1.0;
    state.hero_moving = walking;
    if let Ok(mut hero) = hero_q.single_mut() {
        if coda && t >= 47.3 {
            hero.combat_until = now + 1.0;
        }
        if walking {
            crate::player::anim::stage_gait(&mut hero, KNIGHT_TO / (KNIGHT_STEP.1 - KNIGHT_STEP.0), time.delta_secs());
        } else {
            hero.moving_amt *= (-time.delta_secs() * 8.0).exp();
        }
    }
    strike.block = coda && FIGHT.blocks.iter().any(|b| (b.0..b.1).contains(&t));
    if coda && FIGHT.swings.iter().any(|&sw| crossed(sw)) {
        strike.swing = true;
    }

    // Black between the pictures; each one cuts in on its strike.
    let on = shot(t);
    let black = if on == Shot::Black { 1.0 } else { 0.0 };
    let coda_in = env(t, &[(at::CODA.0, 1.0), (at::CODA.0 + 0.9, 0.0), (at::CODA.1 - 1.0, 0.0), (at::CODA.1, 1.0)]);
    let a = if on == Shot::Coda { coda_in } else { black };
    for mut bg in &mut fade {
        bg.0 = Color::srgba(0.0, 0.0, 0.0, a);
    }

    match LINES.iter().find(|(s0, d, ..)| t >= *s0 && t < s0 + d) {
        Some((s0, d, who, text)) => subs.force(now, Some(who), text, s0 + d - t),
        None => subs.force(now, None, "", 0.0),
    }
    if t >= END {
        state.want = None;
    }
}

/// Harrow fights with a sword in his fist: an arming sword, grip in the hand, blade up the haft line.
pub(super) fn harrow_sword(
    mut commands: Commands,
    state: Res<SceneState>,
    mut meshes: ResMut<Assets<Mesh>>,
    assets: Res<SceneAssets>,
    actors: Query<(&Role, &crate::npc_skin::Skinned)>,
    have: Query<(), With<HarrowSword>>,
) {
    if state.active != Some(SceneId::Storm) || !have.is_empty() {
        return;
    }
    let Some(hand) = actors.iter().find(|(r, _)| **r == Role::Harrow).and_then(|(_, sk)| sk.hand_r) else { return };
    let e = commands
        .spawn((Mesh3d(meshes.add(sword_mesh())), MeshMaterial3d(assets.prop_mat.clone()), crate::npc_skin::fist_grip(true), HarrowSword))
        .id();
    commands.entity(hand).add_child(e);
}

/// An arming sword, its origin in the fist, the blade up +Y.
fn sword_mesh() -> Mesh {
    crate::meshkit::merged_flat(vec![
        cube(0.045, 0.82, 0.008, Vec3::new(0.0, 0.47, 0.0), 0x9aa0a8),
        cube(0.2, 0.025, 0.03, Vec3::new(0.0, 0.06, 0.0), 0x55524c),
        cylr(0.016, 0.15, Vec3::new(0.0, -0.02, 0.0), Quat::IDENTITY, 0x3b2a1c),
        cylr(0.026, 0.03, Vec3::new(0.0, -0.105, 0.0), Quat::IDENTITY, 0x55524c),
    ])
}

/// The spoken lines, as captions (timed with `storm_mix.py`).
const LINES: &[(f32, f32, &str, &str)] = &[
    (0.1, 2.1, "Sailor", "Reef! Reef off the bow!"),                       // p1_sailor_01
    (4.5, 2.5, "Sailor", "Ring it! All hands, brace!"),                    // p1_sailor_02
    (29.6, 2.2, "Wat", "Ser...? Ser Godric."),                             // p1_wat_04
    (38.1, 4.4, "Maud", "He's gone, Wat. Put it on. They need a knight standing."), // p1_maud_01
    (47.8, 2.6, "Maud", "Ser Godric's up! Behind him, all of you!"),       // a1_maud_04
];

/// Props that move: the bell swinging on its yoke, the sky and the bolts, the rain, the fires.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn drive_fx(
    time: Res<Time>,
    state: Res<SceneState>,
    mut bell: Query<&mut Transform, (With<Bell>, Without<Bolt>, Without<Rain>, Without<Flame>, Without<StormSky>, Without<SkyGlow>)>,
    mut bolts: Query<(&mut Transform, &mut Visibility, &Bolt), (Without<Bell>, Without<Rain>, Without<Flame>, Without<StormSky>, Without<SkyGlow>)>,
    mut rain: Query<(&mut Transform, &Rain), (Without<Bell>, Without<Bolt>, Without<Flame>, Without<StormSky>, Without<SkyGlow>)>,
    mut flames: Query<(&mut Transform, &Flame), (Without<Bell>, Without<Bolt>, Without<Rain>, Without<StormSky>, Without<SkyGlow>)>,
    mut sky: Query<(&mut Transform, &StormSky), (Without<Bell>, Without<Bolt>, Without<Rain>, Without<Flame>, Without<SkyGlow>)>,
    mut glow: Query<(&mut Transform, &SkyGlow), (Without<Bell>, Without<Bolt>, Without<Rain>, Without<Flame>, Without<StormSky>)>,
    mut lights: Query<(&mut PointLight, &FlameLight)>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if state.active != Some(SceneId::Storm) {
        return;
    }
    let t = state.clock.max(0.0);
    let now = time.elapsed_secs();
    let fl = flash(t);
    let s = staged(t);
    for mut tf in &mut bell {
        // rung hard by hand: a fast uneven swing
        tf.rotation = Quat::from_rotation_x(0.55 * (t * 7.3).sin() + 0.2 * (t * 11.0).sin());
    }
    let ((eye, ..), _) = camera(t).0;
    let face = |p: Vec3| {
        let d = (eye - p).xz();
        Quat::from_rotation_y(d.x.atan2(d.y))
    };
    for (mut tf, sk) in &mut sky {
        tf.translation = eye;
        tf.scale = Vec3::splat(if day_dev() { 0.0 } else { SKY_R });
        if let Some(mut m) = mats.get_mut(&sk.0) {
            let k = sky_gain() * (1.0 + 22.0 * fl);
            m.base_color = Color::linear_rgb(k, k, k * 1.02);
        }
    }
    let bdir = bolt_dir(s);
    for (mut tf, g) in &mut glow {
        let d = bdir.unwrap_or(Vec3::Y);
        let p = eye + d * (SKY_R - 4.0);
        *tf = Transform::from_translation(p).looking_at(eye, Vec3::Y).with_scale(Vec3::splat(if day_dev() { 0.0 } else { SKY_R * 0.6 }));
        if let Some(mut m) = mats.get_mut(&g.0) {
            let k = if bdir.is_some() { 0.45 * fl } else { 0.0 };
            m.base_color = Color::linear_rgb(0.75 * k, 0.8 * k, k);
        }
    }
    for (mut tf, mut vis, b) in &mut bolts {
        let k = pulse(t - STRIKES[b.strike].0);
        let mine = staged(STRIKES[b.strike].0 + 0.1) == s;
        *vis = if mine && k > 0.4 { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(d) = bdir {
            // its foot below the skyline, standing up into the cloud
            let p = eye + Vec3::new(d.x, 0.0, d.z).normalize() * (SKY_R - 8.0) - Vec3::Y * 6.0;
            *tf = Transform::from_translation(p).with_rotation(face(p)).with_scale(Vec3::splat(SKY_R * 0.62));
        }
    }
    // Rain: volumes falling at 11 u/s, slanted by the wind, two stacked round the eye.
    let drop = (t * 11.0).rem_euclid(RAIN_H);
    for (mut tf, r) in &mut rain {
        tf.translation = eye + Vec3::new(0.0, RAIN_H * 0.5 - drop + r.phase * RAIN_H - RAIN_H, 0.0);
        if let Some(mut m) = mats.get_mut(&r.mat) {
            let k = 0.035 + 0.28 * fl;
            m.base_color = Color::linear_rgb(0.75 * k, 0.8 * k, 0.9 * k);
        }
    }
    for (mut tf, f) in &mut flames {
        let s = f.size * (1.0 + 0.14 * (now * 9.0 + f.phase).sin() + 0.07 * (now * 17.3 + f.phase).sin());
        tf.scale = Vec3::new(s, s * (1.0 + 0.15 * (now * 11.1 + f.phase).sin()), s);
    }
    for (mut l, f) in &mut lights {
        l.intensity = f.base * (1.0 + 0.12 * (now * 9.3 + f.phase).sin() + 0.08 * (now * 23.0 + f.phase * 2.0).sin());
    }
}

/// The cloud dome's brightness between strikes (look-dev: `FOREST_STORM_SKY`).
pub(super) fn sky_gain() -> f32 {
    static V: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("FOREST_STORM_SKY").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0))
}

/// The dome's radius: inside the night sky's 95 (stars, moon), so it hides them.
pub(super) const SKY_R: f32 = 88.0;
/// The dome's colour at the horizon (linear, x [`sky_gain`]): what the haze fades into.
pub(super) const SKY_HORIZON: Vec3 = Vec3::new(0.016, 0.018, 0.024);

// ── Light ────────────────────────────────────────────────────────────────────────────────

/// The storm's light at `t`, applied by `farm_camera` over what `advance_sky` wrote.
pub(super) struct Light {
    /// The moon's lux and the direction its light travels: between strikes the dark the storm
    /// makes; on a strike it swings round to that shot's bolt and flares.
    pub moon: f32,
    pub dir: Vec3,
    pub ambient: f32,
    pub ibl: f32,
    /// The flash, 0..1.
    pub flash: f32,
}

pub(super) fn light(t: f32) -> Light {
    let st = stage();
    let fl = flash(t);
    let inl = Vec3::new(st.inl.x, 0.0, st.inl.y);
    let along = Vec3::new(st.along.x, 0.0, st.along.y);
    let s = staged(t);
    let fill = if s == Shot::Coda { 22.0 } else { 6.0 };
    let (base, dir) = match s {
        // from out to sea, over her starboard bow: the hands go black against it
        Shot::Deck => (220.0, inl * 0.8 + along * 0.5 - Vec3::Y * 0.55),
        // across the beach from Wat's side: the orks' shapes, their own torches on their faces
        Shot::Beach => (60.0, inl * 0.6 - along * 0.7 - Vec3::Y * 0.6),
        // from out over the sea behind them: Harrow and the orks black against the white
        Shot::Stand => (650.0, inl * 0.85 + along * 0.2 - Vec3::Y * 0.5),
        // from behind Wat, low: on the faces
        Shot::Harrow => (90.0, inl * 0.75 + along * 0.45 - Vec3::Y * 0.55),
        // from up the beach behind them: a rim on the helmet, the pair against the sky
        Shot::Helm => (70.0, -inl * 0.7 + along * 0.4 - Vec3::Y * 0.5),
        // from behind the camera's shoulder: the light the knight's plate catches
        Shot::Coda => (2600.0, inl * 0.7 - along * 0.35 - Vec3::Y * 0.55),
        Shot::Black => (0.0, -Vec3::Y),
    };
    Light { moon: base + flash_lux() * fl, dir: dir.normalize(), ambient: fill + 45.0 * fl, ibl: if s == Shot::Coda { 90.0 } else { 12.0 } + 60.0 * fl, flash: fl }
}

/// The moon's lux at a full strike (look-dev: `FOREST_FLASH_LUX`).
fn flash_lux() -> f32 {
    static V: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("FOREST_FLASH_LUX").ok().and_then(|v| v.parse().ok()).unwrap_or(9000.0))
}

/// Where the shot's bolt stands from the camera (unit, horizontal-ish), if the shot looks at it.
fn bolt_dir(s: Shot) -> Option<Vec3> {
    let st = stage();
    let inl = Vec3::new(st.inl.x, 0.0, st.inl.y);
    let along = Vec3::new(st.along.x, 0.0, st.along.y);
    match s {
        // out to sea past the bell, low over the reef
        Shot::Deck => Some((-inl * 0.85 + along * 0.75 + Vec3::Y * 0.05).normalize()),
        // over the trees, right of the orks
        Shot::Beach => Some((inl + along * 0.42 + Vec3::Y * 0.08).normalize()),
        // out to sea behind the fight
        Shot::Stand => Some((-inl + along * 0.25 + Vec3::Y * 0.1).normalize()),
        // up the beach behind the two of them
        Shot::Helm => Some((inl - along * 0.1 + Vec3::Y * 0.12).normalize()),
        Shot::Coda => Some((inl - along * 0.5 + Vec3::Y * 0.06).normalize()),
        _ => None,
    }
}

// ── Coverage ────────────────────────────────────────────────────────────────────────────

/// The shot at `t`: ((eye, look-at, vertical fov, thin-lens aperture), focus distance), and the up
/// vector (the deck shot rolls with the heel).
pub(super) fn camera(t: f32) -> ((super::Cam, Option<f32>), Vec3) {
    let st = stage();
    let lin = |t0: f32, t1: f32| ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    let ease = |u: f32| u * u * (3.0 - 2.0 * u);
    // Wat's breathing, in his eye: heaving, ragged.
    let breath = |amp: f32| Vec3::Y * amp * ((t * TAU * 0.55).sin() + 0.35 * (t * TAU * 1.3 + 0.8).sin());
    let (eye, look, fov, band, focus, roll) = match staged(t) {
        // The deck heeled over: low by the stern castle, along her waist to the bell and the rail,
        // the reef and the white water beyond.
        Shot::Deck => {
            let k = ease(lin(at::DECK.0, at::DECK.1));
            let eye = ship_pt(-1.7, -3.9, 1.35).lerp(ship_pt(-1.55, -3.6, 1.3), k) + breath(0.03);
            let look = ship_pt(1.6, 2.6, 1.15);
            (eye, look, 54.0, 0.35, Some(ship_pt(0.9, 1.0, 1.2)), 0.3 + 0.05 * (t * 1.1).sin())
        }
        // On his knees at the waterline, up the beach to the trees: the orks coming down it.
        Shot::Beach => {
            let k = ease(lin(at::BEACH.0, at::BEACH.1));
            let eye = st.at(0.6, -0.9, 0.95) + breath(0.035);
            let look = st.at(30.0, 0.3, 2.6).lerp(st.at(12.0, 0.0, 1.6), k);
            // focus rides the lead ork in
            let lead = ork_place(0, Shot::Beach, t).map(|(p, ..)| Vec3::new(p.x, ground(p) + 1.4, p.y));
            (eye, look, 46.0, 0.6, lead, -0.04)
        }
        // Harrow's stand, from up the beach: low, the three of them black against the sea and the
        // lightning, a slow drift in.
        Shot::Stand => {
            let k = ease(lin(at::STAND.0, at::STAND.1));
            let c = st.at(HARROW_AT.0, HARROW_AT.1, 0.0);
            let eye = st.at(HARROW_AT.0 + 4.4, HARROW_AT.1 - 1.3, 1.0).lerp(st.at(HARROW_AT.0 + 3.7, HARROW_AT.1 - 1.1, 0.95), k) + breath(0.012);
            let look = st.at(HARROW_AT.0 - 0.6, HARROW_AT.1 - 1.0, 1.15);
            (eye, look, 42.0, 0.45, Some(c + Vec3::Y * 1.2), 0.0)
        }
        // Low from where Wat kneels: Harrow on his back across the sand, the ork over him, Maud.
        Shot::Harrow => {
            let k = lin(at::HARROW.0, at::HARROW.1);
            let eye = st.at(1.2, 0.8, 0.85).lerp(st.at(1.45, 0.85, 0.82), k) + breath(0.02);
            let look = st.at(5.6, 2.0, 0.85);
            (eye, look, 44.0, 0.5, Some(st.at(5.2, 1.6, 0.4)), 0.02)
        }
        // The helmet at the lens, then focus pulled past it onto the two of them.
        Shot::Helm => {
            let eye = st.at(HELM_I - 0.8, HELM_A - 0.35, 0.3);
            let look = st.at(5.6, 3.3, 0.55);
            let helm = st.at(HELM_I, HELM_A, 0.12);
            let pair = st.at(5.65, 3.8, 0.8);
            let k = ease(lin(40.8, 42.0));
            let f = eye.distance(helm) + (eye.distance(pair) - eye.distance(helm)) * k;
            (eye, look, 38.0, 0.45, Some(eye + (look - eye).normalize() * f), 0.0)
        }
        // Behind the knight in the surf, a slow push as the first one comes for him.
        // The fight, from the side: low, the knight left, the orks coming in from the right.
        // ... then rising and drawing away over it, the bodies on the sand round him.
        Shot::Coda if t >= FIGHT.side => {
            let k = ease(lin(FIGHT.side, FIGHT.pull.0));
            let kp = ease(lin(FIGHT.pull.0, FIGHT.pull.1));
            let kx = KNIGHT.0 + KNIGHT_TO;
            let near = st.at(kx + 0.6, KNIGHT.1 - 3.9, 1.2).lerp(st.at(kx + 0.75, KNIGHT.1 - 3.3, 1.15), k);
            let eye = near.lerp(st.at(kx - 6.5, KNIGHT.1 - 9.0, 7.5), kp);
            let look = st.at(kx + 0.9, KNIGHT.1 + 0.35, 1.05).lerp(st.at(kx + 0.8, KNIGHT.1, 0.6), kp);
            let fov = 40.0 + 8.0 * kp;
            (eye, look, fov, 0.5 - 0.3 * kp, Some(st.at(kx + 0.7, KNIGHT.1, 1.0)), 0.0)
        }
        Shot::Coda => {
            let k = ease(lin(at::CODA.0, FIGHT.side));
            let eye = st.at(KNIGHT.0 - 4.4, KNIGHT.1 + 1.7, 1.05).lerp(st.at(KNIGHT.0 - 3.6, KNIGHT.1 + 1.45, 1.0), k);
            let look = st.at(KNIGHT.0 + 14.0, KNIGHT.1 - 1.2, 2.3);
            let knight = st.at(KNIGHT.0, KNIGHT.1, 1.5);
            let lead = ork_place(0, Shot::Coda, t).map_or(knight, |(p, ..)| Vec3::new(p.x, ground(p) + 1.4, p.y));
            let f = knight.lerp(lead, ease(lin(50.0, 50.7)));
            (eye, look, 42.0, 0.5, Some(f), 0.0)
        }
        Shot::Black => unreachable!("staged() never returns Black"),
    };
    let fwd = (look - eye).normalize_or_zero();
    let up = Quat::from_axis_angle(fwd, roll) * Vec3::Y;
    (((eye, look, fov, band), focus.map(|p| eye.distance(p))), up)
}

// ── Meshes ───────────────────────────────────────────────────────────────────────────────

const RAIN_H: f32 = 16.0;

fn h01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85eb_ca6b;
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 16;
    (x & 0xffff) as f32 / 65535.0
}

/// A volume of rain streaks round the origin (±11 u across, `RAIN_H` tall), slanted by the wind.
fn rain_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (mut pos, mut nrm, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..3600u32 {
        // denser near the eye (but never on the lens): radius biased inward
        let r = 1.8 + 9.5 * h01(i * 5 + 1).powf(1.4);
        let a = TAU * h01(i * 5 + 2);
        let (x, z) = (r * a.cos(), r * a.sin());
        let y = RAIN_H * h01(i * 5 + 3) - RAIN_H * 0.5;
        let (w, l) = (0.0035, 0.45 + 0.35 * h01(i * 7));
        let lean = Vec3::new(0.09, 0.0, 0.04) * l;
        // a vertical streak card turned to face the centre, so it shows its face to an eye inside
        let side = Vec3::new(-a.sin(), 0.0, a.cos()) * w;
        let base = pos.len() as u32;
        let p0 = Vec3::new(x, y, z);
        for q in [p0 - side, p0 + side, p0 + side + Vec3::Y * l + lean, p0 - side + Vec3::Y * l + lean] {
            pos.push(q.to_array());
            nrm.push([-a.cos(), 0.0, -a.sin()]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_indices(Indices::U32(idx));
    m
}

/// The cloud dome (unit sphere, seen from inside): vertex colour carries the ceiling, dark grey
/// overhead, lighter toward the horizon, mottled by layered noise into heavy cloud.
fn sky_mesh() -> Mesh {
    let mut m = Sphere::new(1.0).mesh().ico(6).unwrap();
    let pos: Vec<Vec3> = match m.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) => p.iter().map(|p| Vec3::from(*p)).collect(),
        _ => Vec::new(),
    };
    let zen = Vec3::new(0.007, 0.008, 0.011);
    let col: Vec<[f32; 4]> = pos
        .iter()
        .map(|p| {
            let up = p.y.max(0.0);
            let base = SKY_HORIZON.lerp(zen, (up * 2.2).min(1.0));
            // value noise in three octaves, stretched along the wind
            let q = Vec3::new(p.x * 2.0, p.y * 4.0, p.z * 2.0);
            let n = 0.55 * vnoise3(q * 2.0) + 0.3 * vnoise3(q * 5.0 + 7.0) + 0.15 * vnoise3(q * 13.0 - 3.0);
            let c = base * (0.45 + 1.15 * n);
            [c.x, c.y, c.z, 1.0]
        })
        .collect();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m
}

/// Smooth 3-D value noise, 0..1.
fn vnoise3(p: Vec3) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let h = |x: f32, y: f32, z: f32| h01((x as i32 as u32).wrapping_mul(73_856_093) ^ (y as i32 as u32).wrapping_mul(19_349_663) ^ (z as i32 as u32).wrapping_mul(83_492_791));
    let l = |a: f32, b: f32, k: f32| a + (b - a) * k;
    let (x, y, z) = (i.x, i.y, i.z);
    l(
        l(l(h(x, y, z), h(x + 1.0, y, z), u.x), l(h(x, y + 1.0, z), h(x + 1.0, y + 1.0, z), u.x), u.y),
        l(l(h(x, y, z + 1.0), h(x + 1.0, y, z + 1.0), u.x), l(h(x, y + 1.0, z + 1.0), h(x + 1.0, y + 1.0, z + 1.0), u.x), u.y),
        u.z,
    )
}

/// The glow a strike throws on the cloud: a disc (unit radius, facing +Z) bright in the middle,
/// falling off to nothing, broken up by the cloud's own noise.
fn glow_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (rings, segs) = (14u32, 48u32);
    let (mut pos, mut nrm, mut col, mut idx) = (vec![[0.0f32, 0.0, 0.0]], vec![[0.0f32, 0.0, 1.0]], vec![[1.0f32, 1.0, 1.0, 1.0]], Vec::new());
    for r in 1..=rings {
        let rr = r as f32 / rings as f32;
        for k in 0..segs {
            let a = TAU * k as f32 / segs as f32;
            let p = Vec3::new(a.cos() * rr, a.sin() * rr * 0.7, 0.0);
            let n = vnoise3(Vec3::new(p.x * 3.0, p.y * 3.0, 1.7));
            let v = (1.0 - rr).powf(3.2) * (0.55 + 0.9 * n);
            pos.push(p.to_array());
            nrm.push([0.0, 0.0, 1.0]);
            col.push([v, v, v, 1.0]);
        }
    }
    for k in 0..segs {
        idx.extend_from_slice(&[0, 1 + k, 1 + (k + 1) % segs]);
    }
    for r in 1..rings {
        let (a0, b0) = (1 + (r - 1) * segs, 1 + r * segs);
        for k in 0..segs {
            let k1 = (k + 1) % segs;
            idx.extend_from_slice(&[a0 + k, b0 + k, b0 + k1, a0 + k, b0 + k1, a0 + k1]);
        }
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(Indices::U32(idx));
    m
}

/// A forked lightning bolt, 1 u tall from its foot (origin) up, in the XY plane: a ragged main
/// channel and two branches, as a flat ribbon (turned to the camera by `drive_fx`).
fn bolt_mesh(seed: u32) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (mut pos, mut nrm, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let mut ribbon = |pts: &[Vec2], w0: f32| {
        for (k, s) in pts.windows(2).enumerate() {
            let w = w0 * (1.0 - 0.6 * k as f32 / pts.len() as f32);
            let d = (s[1] - s[0]).normalize_or_zero();
            let n = Vec2::new(-d.y, d.x) * w;
            let base = pos.len() as u32;
            for q in [s[0] - n, s[0] + n, s[1] + n, s[1] - n] {
                pos.push([q.x, q.y, 0.0]);
                nrm.push([0.0, 0.0, 1.0]);
            }
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    };
    let walk = |start: Vec2, steps: u32, dy: f32, jit: f32, s: u32| {
        let mut p = start;
        let mut out = vec![p];
        for k in 0..steps {
            p += Vec2::new((h01(s + k * 3) - 0.5) * jit, -dy * (0.6 + 0.8 * h01(s + k * 3 + 1)));
            out.push(p);
        }
        out
    };
    let main = walk(Vec2::new(0.0, 1.0), 22, 0.046, 0.09, seed);
    ribbon(&main, 0.0045);
    for (b, from) in [(1u32, 6usize), (2, 11)] {
        let st = main[from];
        let br = walk(st, 8, 0.03, 0.12, seed + 500 * b);
        let br: Vec<Vec2> = br.iter().map(|p| Vec2::new(st.x + (p.x - st.x) + (p.y - st.y).abs() * if b == 1 { 0.8 } else { -0.7 }, p.y)).collect();
        ribbon(&br, 0.0025);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_indices(Indices::U32(idx));
    m
}

/// A carried torch: a stave with a pitch-wrapped head (origin at the grip, up +Y).
fn torch_mesh() -> Mesh {
    crate::meshkit::merged_flat(vec![
        cylr(0.035, 0.9, Vec3::new(0.0, 0.25, 0.0), Quat::IDENTITY, WOOD_DARK),
        cylr(0.06, 0.16, Vec3::new(0.0, 0.72, 0.0), Quat::IDENTITY, 0x2c2019),
    ])
}

/// The bell's yoke: two posts and a crossbar, lashed to the deck (origin on the deck).
fn yoke_mesh() -> Mesh {
    crate::meshkit::merged_flat(vec![
        cylr(0.06, 1.8, Vec3::new(-0.42, 0.9, 0.0), Quat::IDENTITY, WOOD),
        cylr(0.06, 1.8, Vec3::new(0.42, 0.9, 0.0), Quat::IDENTITY, WOOD),
        cube(1.0, 0.1, 0.12, Vec3::new(0.0, 1.75, 0.0), WOOD_DARK),
    ])
}

/// A ship's bell hanging from its crown (origin at the pivot, the bell below it).
fn ship_bell_mesh() -> Mesh {
    let mut p = vec![cube(0.08, 0.1, 0.08, Vec3::new(0.0, -0.05, 0.0), IRON)];
    // a lathe of stacked rings: shoulder, waist, flared lip
    let prof = [(0.11, -0.1), (0.15, -0.16), (0.16, -0.26), (0.18, -0.36), (0.23, -0.44)];
    for w in prof.windows(2) {
        let (r, y0, y1) = ((w[0].0 + w[1].0) * 0.5, w[0].1, w[1].1);
        p.push(cylr(r, y0 - y1, Vec3::new(0.0, (y0 + y1) * 0.5, 0.0), Quat::IDENTITY, 0xa47a2c));
    }
    p.push(cylr(0.03, 0.2, Vec3::new(0.0, -0.36, 0.0), Quat::IDENTITY, IRON)); // clapper
    p.push(cylr(0.006, 0.6, Vec3::new(0.0, -0.66, 0.0), Quat::IDENTITY, 0x8a7a5e)); // the lanyard
    crate::meshkit::merged_flat(p)
}

/// The axe stuck in the sand (origin where the blade goes in; the haft rising out of it).
fn axe_mesh() -> Mesh {
    crate::meshkit::merged_flat(vec![
        cylr(0.02, 0.62, Vec3::new(0.0, 0.33, 0.0), Quat::IDENTITY, 0x5a4028),
        cube(0.04, 0.15, 0.17, Vec3::new(0.0, 0.05, 0.04), 0x55585e),
        cube(0.025, 0.05, 0.06, Vec3::new(0.0, 0.06, -0.05), 0x44464b),
    ])
}

/// A vertex-coloured prop mesh under `parent` (the shared prop material).
fn prop_child(commands: &mut Commands, meshes: &mut Assets<Mesh>, assets: &SceneAssets, parent: Entity, mesh: Mesh) {
    let e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(assets.prop_mat.clone()), Transform::IDENTITY)).id();
    commands.entity(parent).add_child(e);
}
