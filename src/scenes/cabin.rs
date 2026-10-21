//! Screenplay **P.0 "Na pokładzie" / Below Deck** — the game's first scene, the T2 cutscene that
//! opens the prologue (`docs/superpowers/specs/2026-10-13-kampania-scenariusz-design.md` §P.0). A
//! VIDEO rig like P.3 (`FOREST_SCENE=cabin`, filmed under `FOREST_CLIP`, soundtrack mixed offline by
//! `tools/cutscene/cabin_audio.py`) — no in-game trigger, skip or save hook.
//!
//! Black, a storm on the hull, the card *KELL — three days out*. Three movements, each line set off
//! by something in the room, with the silence between them left in:
//! 1. **The strap.** Ser Godric Harrow seated in his harness, Wat kneeling at his leg doing up a
//!    greave. "Tighter." … "it's still wrong." / "Yes, Ser." — Wat gets it home ("Hnn. There.").
//! 2. **The letter.** Harrow sends him with the letter to the Reeve; Wat takes it off the table and
//!    goes to the door — and stops: below deck the settlers are playing a hymn. Harrow, listening:
//!    "Forty peasants and a Reeve. That's what the Crown sends a man after the eastern campaign."
//! 3. **The city.** The storm in the stern windows. "Do you know what they said about me in the
//!    city?" — lightning — "Before it burned?" Wat turns at the door: "No, Ser." Harrow turns to
//!    him: "Neither do I. Nobody looks at the man behind the shield, Wat. That's the point of you."
//!    Wat lingers; Harrow, back at the window: "Stop hovering." Wat goes out into the dark passage,
//!    and Harrow is left alone with the lantern.
//!
//! The set is moored on the open sea off Wreck Cove, stern window facing away from the island, and
//! rocks as one rigid frame ([`ship_xf`]): set, actors, lights and camera all ride it, so the cabin
//! holds still in frame while the sea through the window, the lantern and the rain move.
//!
//! **Asset slots** (`docs/superpowers/specs/2026-10-20-p0-below-deck-cutscene.md`): every piece that
//! should be a real asset is looked up first and stands in with a blockout only while it's missing:
//! kit models `cs_cabin` / `cs_cabin_table` / `cs_cabin_chair` / `cs_cabin_hammock` /
//! `cs_cabin_lantern`, the NPC looks `harrow` / `wat` (stand-ins: guard / archer), the music
//! `below_deck.ogg` and the cabin SFX in `assets/cutscene/ship_cabin.cues` (missing files are skipped).

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use super::{cube, cylr, env, SceneAssets, SceneId, SceneProp, SceneState, IRON, WOOD, WOOD_DARK};

/// Deep night, the storm's.
pub(super) const SKY: f32 = 0.82;
pub(super) const END: f32 = 86.0;
/// Title card on black, then the picture fades up.
const TITLE: (f32, f32, f32, f32) = (0.9, 1.9, 3.5, 4.4);
const FADE_IN: (f32, f32) = (4.6, 6.4);
const FADE_OUT: (f32, f32) = (83.5, 85.5);

// ── The set, in the ship's frame: +Z aft (the stern windows), +Y up, floor at y = 0 ───────────

const HALF_W: f32 = 2.1;
const HALF_D: f32 = 1.7;
/// Underside of the deck beams.
const BEAMS: f32 = 1.72;
/// Stern window openings (x ranges) between sill and head.
const WINDOWS: [(f32, f32); 3] = [(-1.45, -0.55), (-0.45, 0.45), (0.55, 1.45)];
const SILL: f32 = 0.8;
const HEAD: f32 = 1.48;
/// Cabin floor above the sea.
const FLOOR_UP: f32 = 2.4;

/// Harrow's chair: he sits facing forward-to-port (−X), the stern windows on his left.
const CHAIR: Vec2 = Vec2::new(1.0, 0.75);
const CHAIR_YAW: f32 = -FRAC_PI_2;
const CHAIR_SEAT: f32 = 0.42;
/// How far the seated pelvis rides above the seat top (as `village_life`'s bench fit).
const PELVIS_UP: f32 = 0.09;
/// The table at his right hand, along the starboard hull.
const TABLE: Vec2 = Vec2::new(1.3, -0.35);
const TABLE_TOP: f32 = 0.6;
/// Wat kneels at Harrow's left leg, facing him (+X).
const WAT_AT: Vec2 = Vec2::new(0.02, 0.86);
const WAT_YAW: f32 = FRAC_PI_2;
/// Where Wat stands to take the letter off the table (facing +X), the bend he walks past his
/// master's knees on the way there, and his mark inside the door (forward bulkhead, port of centre).
const TABLE_SPOT: Vec2 = Vec2::new(0.64, -0.3);
const PAST_KNEES: Vec2 = Vec2::new(0.3, 0.3);
const DOOR_X: f32 = -1.0;
const DOOR_SPOT: Vec2 = Vec2::new(-0.95, -1.12);
/// The door's width, its hinge on the port jamb; it swings out into the passage. It hangs a finger
/// off the deck, so the hold's light shows under it.
const DOOR_W: f32 = 0.7;
const DOOR_GAP: f32 = 0.025;
/// The passage forward of the door runs this far; the hold's lamplight comes up at its far end.
const PASSAGE: f32 = 2.3;
/// The candle sconce on the bulkhead starboard of the door — the only light at that end of the cabin.
const SCONCE: Vec3 = Vec3::new(-0.42, 1.24, -HALF_D + 0.06);
/// The letter to the Reeve on the table's near edge (table-top frame), where Wat's right hand
/// comes down for it.
const LETTER_ON_TABLE: Vec3 = Vec3::new(TABLE.x - 0.28, TABLE_TOP + 0.011, TABLE.y + 0.04);
/// The lantern's hook on the beam between them.
const LANTERN_HOOK: Vec3 = Vec3::new(0.45, BEAMS, 0.3);

/// Look-at marks for the coverage, measured off the posed stand-in bodies (head bones; Wat's hands
/// on the greave) — the `Fixing_Kneeling` crouch carries his head low and forward, over the work.
const HARROW_HEAD: Vec3 = Vec3::new(0.87, 1.05, 0.75);
const WAT_HEAD: Vec3 = Vec3::new(0.41, 0.6, 0.82);
const STRAP: Vec3 = Vec3::new(0.45, 0.25, 0.84);
/// Wat on his feet: head height, and his head at the door turned back to his master.
const WAT_STAND_H: f32 = 1.38;
const WAT_DOOR_HEAD: Vec3 = Vec3::new(-0.89, 1.39, -1.05);
/// The helmet on the table's near corner, by his right hand; the shield hung on the hull behind his head.
const HELM_AT: Vec3 = Vec3::new(1.02, TABLE_TOP - 0.02, 0.04);
const SHIELD_AT: Vec3 = Vec3::new(HALF_W - 0.13, 1.12, 1.07);

// ── Lines ───────────────────────────────────────────────────────────────────────────────

/// The VO as `cabin_audio.py` lays it in: (start s, clip, trim from..to s, speaker). The takes are
/// split at their own pauses (silencedetect: `a1_harrow_01` "Tighter." 0–0.81 | 2.47–7.6, `_02`
/// 0–2.52 | 3.13–7.2, `_03` 0.35–2.80 | 3.30–4.43, `_04` 0–1.41 | 3.02–5.89 | 6.50–7.84, `_07`
/// 0–1.16 | 1.75–2.75 | 3.30–5.92) so the beats between phrases are the scene's, not the take's.
/// Read here only for the mouths and the captions.
pub(super) const VO: [(f32, &str, f32, f32, Role); 14] = [
    (12.2, "audio/vo/campaign/a1_harrow_01.ogg", 0.0, 0.9, Role::Harrow), // "Tighter."
    (14.85, "audio/vo/campaign/a1_harrow_01.ogg", 2.4, 7.6, Role::Harrow), // "You've done that strap every day for fifteen years and it's still wrong."
    (21.3, "audio/vo/campaign/a1_wat_01.ogg", 0.0, 0.94, Role::Wat), // "Yes, Ser."
    (23.6, "audio/vo/campaign/a1_wat_05.ogg", 0.0, 2.0, Role::Wat), // (effort) "Hnn." (exhales) "There."
    (26.6, "audio/vo/campaign/a1_harrow_07.ogg", 0.0, 5.92, Role::Harrow), // (sighs) "The letter. To the Reeve. Today, if the sea allows."
    (39.0, "audio/vo/campaign/a1_harrow_02.ogg", 0.0, 2.6, Role::Harrow), // "Forty peasants and a Reeve."
    (42.4, "audio/vo/campaign/a1_harrow_02.ogg", 3.0, 7.2, Role::Harrow), // "That's what the Crown sends a man after the eastern campaign."
    (49.6, "audio/vo/campaign/a1_harrow_03.ogg", 0.0, 2.95, Role::Harrow), // "Do you know what they said about me in the city?"
    (53.6, "audio/vo/campaign/a1_harrow_03.ogg", 3.2, 4.43, Role::Harrow), // "Before it burned?"
    (57.0, "audio/vo/campaign/a1_wat_02.ogg", 0.0, 0.92, Role::Wat), // "No, Ser."
    (59.4, "audio/vo/campaign/a1_harrow_04.ogg", 0.0, 1.5, Role::Harrow), // "Neither do I."
    (62.6, "audio/vo/campaign/a1_harrow_04.ogg", 2.9, 5.95, Role::Harrow), // "Nobody looks at the man behind the shield, Wat."
    (66.6, "audio/vo/campaign/a1_harrow_04.ogg", 6.4, 7.84, Role::Harrow), // "That's the point of you."
    (72.2, "audio/vo/campaign/a1_harrow_08.ogg", 0.0, 1.51, Role::Harrow), // "Stop hovering."
];

/// Captions: (start s, duration s, speaker, line) — verbatim, one per phrase of the [`VO`].
const LINES: [(f32, f32, &str, &str); 16] = [
    (12.2, 0.9, "Harrow", "Tighter."),
    (14.9, 5.3, "Harrow", "You've done that strap every day for fifteen years and it's still wrong."),
    (21.3, 1.1, "Wat", "Yes, Ser."),
    (23.6, 2.1, "Wat", "Hnn\u{2026} There."),
    (26.6, 1.3, "Harrow", "The letter."),
    (28.3, 1.3, "Harrow", "To the Reeve."),
    (29.85, 2.8, "Harrow", "Today, if the sea allows."),
    (39.0, 2.6, "Harrow", "Forty peasants and a Reeve."),
    (42.5, 4.2, "Harrow", "That's what the Crown sends a man after the eastern campaign."),
    (49.9, 2.6, "Harrow", "Do you know what they said about me in the city?"),
    (53.65, 1.3, "Harrow", "Before it burned?"),
    (57.0, 1.0, "Wat", "No, Ser."),
    (59.4, 1.6, "Harrow", "Neither do I."),
    (62.7, 3.0, "Harrow", "Nobody looks at the man behind the shield, Wat."),
    (66.65, 1.5, "Harrow", "That's the point of you."),
    (72.2, 1.6, "Harrow", "Stop hovering."),
];

/// Lightning strikes outside the stern: (time, strength). A far one over the window insert, the big
/// one just before "Before it burned?", one more over Harrow alone at the end.
const STRIKES: [(f32, f32); 5] = [(5.4, 0.55), (18.9, 0.2), (47.6, 0.35), (53.1, 1.0), (80.6, 0.6)];

// ── Components ──────────────────────────────────────────────────────────────────────────

#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(super) enum Role {
    Harrow,
    Wat,
}

/// The rigid ship frame everything in the set hangs off.
#[derive(Component)]
pub(super) struct Ship;

#[derive(Component)]
pub(super) struct Lantern;

/// The cabin door's pivot on its hinge.
#[derive(Component)]
pub(super) struct Door;

/// The letter to the Reeve: on the table until Wat's hand takes it.
#[derive(Component)]
pub(super) struct Letter;

#[derive(Component)]
pub(super) struct Flame {
    phase: f32,
}

#[derive(Component)]
pub(super) struct FlameLight {
    base: f32,
    phase: f32,
}

#[derive(Component)]
pub(super) struct Lightning {
    base: f32,
}

#[derive(Component)]
pub(super) struct Rain {
    mat: Handle<StandardMaterial>,
    phase: f32,
}

#[derive(Component)]
pub(super) struct Title;

/// The seat fit: world offset that rests Harrow's real pelvis on the chair.
#[derive(Component, Default)]
pub(super) struct SeatFit(Vec3);

// ── The ship's motion ───────────────────────────────────────────────────────────────────

fn roll(t: f32) -> f32 {
    0.042 * (TAU * t / 7.4).sin() + 0.011 * (TAU * t / 3.3 + 1.3).sin()
}
fn pitch(t: f32) -> f32 {
    0.016 * (TAU * t / 5.6 + 0.7).sin()
}
fn heave(t: f32) -> f32 {
    0.07 * (TAU * t / 6.3 + 2.1).sin()
}

/// Where the ship rides: 150 u off Wreck Cove's mouth, bow toward the island, stern to the open sea.
fn anchor() -> (Vec2, f32) {
    let (c, inland) = crate::worldmap::wreck_cove_axis();
    let p = c - inland * 150.0;
    (p, (-inland.x).atan2(-inland.y))
}

/// The ship frame at scene time `t` (ship-local → world).
pub(super) fn ship_xf(t: f32) -> Transform {
    let (p, yaw) = anchor();
    Transform {
        translation: Vec3::new(p.x, crate::worldmap::SEA_Y + FLOOR_UP + heave(t), p.y),
        rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch(t)) * Quat::from_rotation_z(roll(t)),
        scale: Vec3::ONE,
    }
}

fn local(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

// ── Setup ───────────────────────────────────────────────────────────────────────────────

/// What a blockout part is made of: the CC0 scan it wears (`assets/forest_pbr`), or none.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Surf {
    Wood,
    Iron,
    Cloth,
    Plain,
}

impl Surf {
    const ALL: [Surf; 4] = [Surf::Wood, Surf::Iron, Surf::Cloth, Surf::Plain];
    fn set(self) -> &'static str {
        match self {
            Surf::Wood => "s_timber",
            Surf::Iron => "s_iron",
            Surf::Cloth => "s_cloth",
            Surf::Plain => "",
        }
    }
    /// Units per texture repeat.
    fn tile(self) -> f32 {
        match self {
            Surf::Wood => 1.1,
            Surf::Iron => 0.5,
            Surf::Cloth => 0.45,
            Surf::Plain => 1.0,
        }
    }
    /// The blockout colour that wears the scan's own colour × `gain`; others keep their offset
    /// from it, so the plank-to-plank shading survives.
    fn reference(self) -> (u32, f32) {
        match self {
            Surf::Wood => (0x5b3d24, 0.62),
            Surf::Iron => (IRON, 0.75),
            Surf::Cloth => (0xa89a7c, 0.8),
            Surf::Plain => (0xffffff, 1.0),
        }
    }
}

/// The blockouts' materials, one per [`Surf`]: the scans, or the shared vertex-colour material
/// where a set fails to load.
pub(super) struct Skins([Handle<StandardMaterial>; 4]);

fn skins(images: &mut Assets<Image>, std_mats: &mut Assets<StandardMaterial>, assets: &SceneAssets) -> Skins {
    Skins(Surf::ALL.map(|s| match s {
        Surf::Plain => assets.prop_mat.clone(),
        s => match crate::realforest::pbr_material(images, s.set(), false) {
            Ok(m) => std_mats.add(m),
            Err(e) => {
                warn!("cabin: {} scan missing, flat colour instead — {e}", s.set());
                assets.prop_mat.clone()
            }
        },
    }))
}

/// A part's surface from its colour: pale = paper, wax, bone (no scan); grey = iron; else `base`.
fn surf_of(part: &Mesh, base: Surf) -> Surf {
    use bevy::mesh::VertexAttributeValues as V;
    let Ok(V::Float32x4(c)) = part.try_attribute(Mesh::ATTRIBUTE_COLOR) else { return base };
    let s = Vec3::from_slice(&c[0][..3]).powf(1.0 / 2.2);
    let (mx, mn) = (s.max_element(), s.min_element());
    match () {
        _ if base == Surf::Cloth => base,
        _ if mx > 0.6 => Surf::Plain,
        _ if (mx - mn) / mx.max(1e-3) < 0.25 => Surf::Iron,
        _ => base,
    }
}

/// `s_timber`'s layout: grain along v, five boards across u, each clean over this width from its
/// zone start (`tools/blender_forest/wayside_models.py` `TIMBER_ZONES`).
const BOARD_ZONES: [f32; 5] = [0.017, 0.217, 0.417, 0.617, 0.817];
const BOARD_W: f32 = 0.165;
/// Faces narrower than this (planks, beams, legs) sit inside one scanned board; wider ones (a table
/// top, a door) show the scan's boards side by side.
const ONE_BOARD: f32 = 0.3;

/// Facet one part and box-project its UVs: v (the grain) along the part's longest side, from a
/// per-part offset so neighbouring boards don't repeat. Box projection rather than world triplanar:
/// the set rocks with the ship, and the grain must ride it rather than swim across it.
fn skin_part(mut m: Mesh, surf: Surf, seed: u32) -> Mesh {
    use bevy::mesh::VertexAttributeValues as V;
    m.duplicate_vertices();
    m.compute_flat_normals();
    let (Ok(V::Float32x3(pos)), Ok(V::Float32x3(nrm))) = (m.try_attribute(Mesh::ATTRIBUTE_POSITION), m.try_attribute(Mesh::ATTRIBUTE_NORMAL)) else {
        return m;
    };
    let (lo, hi) = pos.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p))));
    let ext = hi - lo;
    let off = Vec2::new(h01(seed), h01(seed ^ 0x5bd1));
    let tile = surf.tile();
    let uv: Vec<[f32; 2]> = pos
        .iter()
        .zip(nrm)
        .map(|(p, n)| {
            let (p, n) = (Vec3::from(*p), Vec3::from(*n).abs());
            let (b, c) = if n.x >= n.y && n.x >= n.z {
                (2, 1)
            } else if n.y >= n.z {
                (0, 2)
            } else {
                (0, 1)
            };
            let (v, u) = if ext[b] >= ext[c] { (b, c) } else { (c, b) };
            let along = p[v] / tile + off.y;
            if surf == Surf::Wood && ext[u] < ONE_BOARD {
                let zone = BOARD_ZONES[(h01(seed ^ 0x1f3) * 5.0) as usize % 5];
                let w = (ext[u] / tile).min(BOARD_W);
                [zone + (BOARD_W - w) * off.x + (p[u] - lo[u]) / ext[u].max(1e-4) * w, along]
            } else {
                [p[u] / tile + off.x, along]
            }
        })
        .collect();
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    if surf != Surf::Plain {
        let (hex, gain) = surf.reference();
        let r = crate::palette::lin(hex);
        if let Ok(V::Float32x4(c)) = m.try_attribute(Mesh::ATTRIBUTE_COLOR) {
            let c: Vec<[f32; 4]> = c
                .iter()
                .map(|c| [0, 1, 2, 3].map(|i| if i == 3 { c[3] } else { (c[i] / r[i].max(1e-4)).clamp(0.3, 1.6) * gain }))
                .collect();
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, c);
        }
    }
    m
}

/// Spawn blockout parts under `parent`, one textured mesh per surface; returns the meshes.
fn spawn_skinned(commands: &mut Commands, meshes: &mut Assets<Mesh>, skins: &Skins, parent: Entity, parts: Vec<Mesh>, base: Surf) -> Vec<Entity> {
    let mut groups: [Vec<Mesh>; 4] = Default::default();
    for (i, p) in parts.into_iter().enumerate() {
        let s = surf_of(&p, base);
        groups[s as usize].push(skin_part(p, s, i as u32 * 31 + 5));
    }
    let mut out = Vec::new();
    for (s, g) in groups.into_iter().enumerate() {
        if g.is_empty() {
            continue;
        }
        let mut m = crate::meshkit::merged(g);
        if let Err(e) = m.generate_tangents() {
            warn!("cabin: tangents — {e}");
        }
        let e = commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(skins.0[s].clone()), Transform::IDENTITY)).id();
        commands.entity(parent).add_child(e);
        out.push(e);
    }
    out
}

/// A kit model if the kit has it, else the textured blockout — parented to the ship frame.
#[allow(clippy::too_many_arguments)]
fn slot(commands: &mut Commands, meshes: &mut Assets<Mesh>, skins: &Skins, ship: Entity, kit: &str, tf: Transform, blockout: fn() -> Vec<Mesh>, base: Surf) {
    let e = match crate::realforest::get().and_then(|k| k.spawn_static(commands, kit, tf)) {
        Some(e) => e,
        None => {
            let e = commands.spawn((tf, Visibility::default())).id();
            spawn_skinned(commands, meshes, skins, e, blockout(), base);
            e
        }
    };
    commands.entity(ship).add_child(e);
}

fn has_kit(name: &str) -> bool {
    crate::realforest::get().is_some_and(|k| k.has(name))
}

/// The look a cast member plays: their own once it's in the NPC cast, else the stand-in.
fn look(name: &str, stand_in: u8) -> u8 {
    crate::npc_skin::LOOKS.iter().position(|l| *l == name).map_or(stand_in, |i| i as u8)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn setup(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    std_mats: &mut Assets<StandardMaterial>,
    assets: &SceneAssets,
    fonts: Option<&crate::ui::fonts::UiFonts>,
) {
    use crate::npc_skin as ns;
    let (p, _) = anchor();
    info!(
        "cabin: ship moored at ({:.1}, {:.1}), ground there {:?}, kit cabin {}",
        p.x,
        p.y,
        crate::worldmap::ground_at_world(p.x, p.y),
        has_kit("cs_cabin")
    );
    let ship = commands.spawn((ship_xf(0.0), Visibility::default(), Ship, SceneProp)).id();

    // The shell + furniture.
    let sk = skins(images, std_mats, assets);
    slot(commands, meshes, &sk, ship, "cs_cabin", Transform::IDENTITY, shell_mesh, Surf::Wood);
    slot(commands, meshes, &sk, ship, "cs_cabin_table", local(TABLE.x, 0.0, TABLE.y), table_mesh, Surf::Wood);
    slot(
        commands, meshes, &sk, ship, "cs_cabin_chair",
        local(CHAIR.x, 0.0, CHAIR.y).with_rotation(Quat::from_rotation_y(CHAIR_YAW)),
        chair_mesh, Surf::Wood,
    );
    slot(commands, meshes, &sk, ship, "cs_cabin_hammock", Transform::IDENTITY, hammock_mesh, Surf::Cloth);
    // Sea chest under the port hull; casks and a crate in the forward-starboard corner.
    let chest_tf = local(-1.72, 0.0, 0.25).with_rotation(Quat::from_rotation_y(FRAC_PI_2));
    if has_kit("chest_wood_body") {
        for part in ["chest_wood_body", "chest_wood_lid"] {
            if let Some(e) = crate::realforest::get().and_then(|k| k.spawn_static(commands, part, chest_tf)) {
                commands.entity(ship).add_child(e);
            }
        }
    } else {
        slot(commands, meshes, &sk, ship, "-", chest_tf, sea_chest_mesh, Surf::Wood);
    }
    slot(commands, meshes, &sk, ship, "wooden_crate_01", local(1.62, 0.0, -1.25).with_scale(Vec3::splat(0.55)), crate_mesh, Surf::Wood);
    slot(commands, meshes, &sk, ship, "-", local(1.05, 0.0, -1.38), casks_mesh, Surf::Wood);
    slot(commands, meshes, &sk, ship, "-", local(TABLE.x, TABLE_TOP, TABLE.y), table_dressing_mesh, Surf::Plain);
    // The door leaf hangs on its own hinge pivot so it can swing open when Wat goes.
    let door = commands.spawn((local(DOOR_X - DOOR_W * 0.5, DOOR_GAP, -HALF_D + 0.02), Visibility::default(), Door)).id();
    match crate::realforest::get().and_then(|k| k.spawn_static(commands, "cs_cabin_door", Transform::IDENTITY)) {
        Some(e) => {
            commands.entity(door).add_child(e);
        }
        None => {
            spawn_skinned(commands, meshes, &sk, door, door_mesh(), Surf::Wood);
        }
    }
    commands.entity(ship).add_child(door);
    let letter = commands.spawn((Transform::from_translation(LETTER_ON_TABLE), Visibility::default(), Letter)).id();
    spawn_skinned(commands, meshes, &sk, letter, letter_mesh(), Surf::Plain);
    commands.entity(ship).add_child(letter);

    // Harrow's own kit: the helmet on the table, the shield hung on the hull behind his chair.
    if let Some(helm) = crate::player::spawn_part_prop(commands, meshes, images, creature_mats, "Head") {
        commands.entity(helm).insert((Name::new("cabin_helm"), Transform::from_translation(HELM_AT).with_rotation(Quat::from_rotation_y(-2.2))));
        commands.entity(ship).add_child(helm);
    }
    if let Some(shield) = crate::player::spawn_part_prop(commands, meshes, images, creature_mats, "Shield") {
        commands.entity(shield).insert((
            Name::new("cabin_shield"),
            Transform::from_translation(SHIELD_AT).with_rotation(Quat::from_rotation_y(FRAC_PI_2) * Quat::from_rotation_x(-0.1)),
        ));
        commands.entity(ship).add_child(shield);
    }

    // The lantern: a pivot on the hook, swung by `drive` against the ship's roll.
    let glow = std_mats.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.62, 0.3),
        emissive: LinearRgba::rgb(2.6, 1.25, 0.38),
        ..default()
    });
    let pivot = commands
        .spawn((Transform::from_translation(LANTERN_HOOK), Visibility::default(), Lantern))
        .with_children(|l| {
            if has_kit("cs_cabin_lantern") {
                if let Some(k) = crate::realforest::get() {
                    let mut e = l.spawn((Transform::IDENTITY, Visibility::default()));
                    k.attach_static_with(&mut e, "cs_cabin_lantern", Transform::IDENTITY, bevy::light::NotShadowCaster);
                }
            } else {
                l.spawn((Mesh3d(meshes.add(Cuboid::new(0.1, 0.15, 0.1))), MeshMaterial3d(glow.clone()), local(0.0, -0.41, 0.0), bevy::light::NotShadowCaster));
            }
            l.spawn((
                PointLight {
                    color: Color::srgb(1.0, 0.64, 0.33),
                    intensity: 90_000.0,
                    range: 8.0,
                    radius: 0.04,
                    shadow_maps_enabled: true,
                    ..default()
                },
                local(0.0, -0.41, 0.0),
                FlameLight { base: 90_000.0, phase: 0.0 },
            ));
        })
        .id();
    if !has_kit("cs_cabin_lantern") {
        for e in spawn_skinned(commands, meshes, &sk, pivot, lantern_mesh(), Surf::Iron) {
            commands.entity(e).insert(bevy::light::NotShadowCaster);
        }
    }
    commands.entity(ship).add_child(pivot);

    // The candle on the table: a flame card + a small warm light.
    let candle = Vec3::new(TABLE.x + 0.18, TABLE_TOP + 0.15, TABLE.y - 0.3);
    let flame = commands
        .spawn((
            Mesh3d(assets.flame_mesh.clone()),
            MeshMaterial3d(assets.flame_mat.clone()),
            Transform::from_translation(candle).with_scale(Vec3::splat(0.13)),
            bevy::light::NotShadowCaster,
            Flame { phase: 1.7 },
        ))
        .id();
    let candle_light = commands
        .spawn((
            PointLight { color: Color::srgb(1.0, 0.7, 0.4), intensity: 7_000.0, range: 3.0, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(candle + Vec3::Y * 0.05),
            FlameLight { base: 7_000.0, phase: 1.7 },
        ))
        .id();
    commands.entity(ship).add_children(&[flame, candle_light]);

    // The sconce by the door: a candle stub on a tin bracket, so the squire's end of the cabin isn't
    // black. And the hold's lamplight at the far end of the passage, behind the door until it opens.
    let sconce = commands.spawn((local(SCONCE.x, SCONCE.y, SCONCE.z), Visibility::default())).id();
    spawn_skinned(commands, meshes, &sk, sconce, sconce_mesh(), Surf::Iron);
    let sconce_flame = SCONCE + Vec3::new(0.0, 0.105, 0.08);
    let s_flame = commands
        .spawn((
            Mesh3d(assets.flame_mesh.clone()),
            MeshMaterial3d(assets.flame_mat.clone()),
            Transform::from_translation(sconce_flame).with_scale(Vec3::splat(0.13)),
            bevy::light::NotShadowCaster,
            Flame { phase: 4.1 },
        ))
        .id();
    let s_light = commands
        .spawn((
            PointLight { color: Color::srgb(1.0, 0.68, 0.38), intensity: 14_000.0, range: 3.2, radius: 0.03, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(sconce_flame + Vec3::new(0.0, 0.04, 0.05)),
            FlameLight { base: 14_000.0, phase: 4.1 },
        ))
        .id();
    let hold = commands
        .spawn((
            PointLight { color: Color::srgb(1.0, 0.6, 0.3), intensity: 60_000.0, range: 5.0, radius: 0.2, shadow_maps_enabled: true, ..default() },
            local(DOOR_X + 0.25, 0.45, -HALF_D - PASSAGE + 0.3),
            FlameLight { base: 60_000.0, phase: 2.6 },
        ))
        .id();
    commands.entity(ship).add_children(&[sconce, s_flame, s_light, hold]);

    // Night outside: a cold moon fill and the lightning, both through the stern windows.
    let moon = commands
        .spawn((
            SpotLight {
                color: Color::srgb(0.72, 0.8, 1.0),
                intensity: 22_000.0,
                range: 14.0,
                shadow_maps_enabled: true,
                outer_angle: 0.62,
                inner_angle: 0.3,
                ..default()
            },
            local(0.6, 2.8, 4.6).looking_at(Vec3::new(0.3, 0.5, 0.0), Vec3::Y),
        ))
        .id();
    let lightning = commands
        .spawn((
            SpotLight {
                color: Color::srgb(0.78, 0.86, 1.0),
                intensity: 0.0,
                range: 24.0,
                shadow_maps_enabled: true,
                outer_angle: 0.75,
                inner_angle: 0.45,
                ..default()
            },
            local(-1.4, 3.4, 7.5).looking_at(Vec3::new(0.6, 0.6, 0.0), Vec3::Y),
            Lightning { base: 4_000_000.0 },
        ))
        .id();
    // What the glass lets in, soft: models a face turned to the windows.
    let window_fill = commands
        .spawn((
            PointLight { color: Color::srgb(0.74, 0.8, 1.0), intensity: 9_000.0, range: 2.6, radius: 0.5, shadow_maps_enabled: false, ..default() },
            local(0.75, 1.2, HALF_D - 0.12),
        ))
        .id();
    commands.entity(ship).add_children(&[moon, lightning, window_fill]);

    // Rain past the stern windows: two stacked sheets scrolled down (`drive`).
    let rain_mat = std_mats.add(StandardMaterial {
        base_color: Color::srgba(0.72, 0.78, 0.88, 0.22),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let sheet = meshes.add(rain_mesh());
    for i in 0..2 {
        let e = commands
            .spawn((
                Mesh3d(sheet.clone()),
                MeshMaterial3d(rain_mat.clone()),
                Transform::IDENTITY,
                bevy::light::NotShadowCaster,
                Rain { mat: rain_mat.clone(), phase: i as f32 },
            ))
            .id();
        commands.entity(ship).add_child(e);
    }

    // The cast. Their own looks once they exist in the NPC cast; until then a guard (the tallest
    // man in the cast — "Harrow was taller") and the archer (a different face from the guard).
    let harrow = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 21);
    commands.entity(harrow).insert((
        ns::SkinRequest(look("harrow", ns::GUARD)),
        ns::SkinClip("Sitting_Idle_Loop"),
        Role::Harrow,
        super::HeadLook::default(),
        SeatFit::default(),
    ));
    let wat = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 33);
    commands.entity(wat).insert((ns::SkinRequest(look("wat", ns::ARCHER)), ns::SkinClip("Fixing_Kneeling"), Role::Wat, super::HeadLook::default()));

    // Letterbox, the fade and the title card on black.
    super::spawn_letterbox(commands);
    if let Some(f) = fonts {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                GlobalZIndex(39),
                SceneProp,
            ))
            .with_children(|p| {
                p.spawn((crate::ui::fonts::label(&f.display, "KELL \u{2014} three days out", 54.0, Color::srgba(0.9, 0.86, 0.78, 0.0)), Title));
            });
    }
}

// ── Per-frame ───────────────────────────────────────────────────────────────────────────

/// Lightning brightness 0..1 at `t`: a double flicker and a ragged decay per strike.
fn flash(t: f32) -> f32 {
    STRIKES
        .iter()
        .map(|&(t0, k)| k * env(t - t0, &[(-0.01, 0.0), (0.03, 1.0), (0.09, 0.15), (0.14, 0.9), (0.22, 0.3), (0.32, 0.6), (0.9, 0.0)]))
        .fold(0.0, f32::max)
}

/// Harrow's beat: (clip, head yaw/pitch/roll; yaw + = his left, the stern windows). Down at the
/// strap; a glance at the letter on the table at his right; after Wat as he goes to the door; eyes on
/// the deck while the hymn comes up through it; round to the windows for the city; back to Wat at
/// the door for the last lines; away to the windows again to send him off.
fn harrow_beat(t: f32) -> (&'static str, Vec3) {
    let clip = if (39.0..46.4).contains(&t) { "Sitting_Talking_Loop" } else { "Sitting_Idle_Loop" };
    let yaw = env(
        t,
        &[
            (26.0, 0.0), (26.7, -1.0), (29.2, -1.0), (29.9, -0.25), (31.2, -0.35), (32.0, -0.9), (33.6, -0.9),
            (35.4, -0.75), (36.4, -0.75), (37.4, -0.1), (46.6, -0.1), (47.4, 1.05), (58.8, 1.05), (59.5, -0.75),
            (69.6, -0.75), (70.6, 1.05),
        ],
    );
    let pitch = env(
        t,
        &[
            (7.4, -0.32), (8.0, -0.42), (25.6, -0.42), (26.7, -0.3), (29.2, -0.3), (29.9, -0.05), (36.4, 0.0),
            (37.4, -0.28), (39.0, -0.18), (46.6, -0.12), (47.4, 0.06), (58.8, 0.06), (59.5, 0.05), (70.6, 0.06),
        ],
    );
    (clip, Vec3::new(yaw, pitch, 0.0))
}

/// Wat's blocking at `t`: (floor spot, facing yaw, clip, scrubbed playhead). Kneeling at the greave
/// until the strap is home; up (the rest of the kneel clip) on "Today…"; round the lantern to the
/// table; the letter lifted; to the door, where the hymn stops him; turned back to his master on
/// "Before it burned?"; turned away and out through the door after "Stop hovering."
fn wat_track(t: f32) -> (Vec2, f32, &'static str, Option<f32>) {
    // `Fixing_Kneeling` kneels down, works and stands back up in one 4.1 s cycle; down on one knee
    // 0.75–3.15 s. Held there (eased back and forth so the hands keep working), then let up.
    const DOWN: (f32, f32) = (0.75, 3.15);
    const RISE: (f32, f32) = (29.4, 30.4);
    const PICK: (f32, f32) = (32.2, 33.5);
    const FACE_HARROW: f32 = 0.79;
    let span = DOWN.1 - DOWN.0;
    let lin = |t0: f32, t1: f32| ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    // Along a polyline at constant pace; facing along a short chord of it, so he turns through a
    // corner instead of snapping round it.
    let walk = |pts: &[Vec2], t0: f32, t1: f32| {
        let n = pts.len() - 1;
        let total: f32 = (0..n).map(|i| pts[i].distance(pts[i + 1])).sum();
        let at = |mut d: f32| {
            d = d.clamp(0.0, total);
            let mut i = 0;
            while i + 1 < n && d > pts[i].distance(pts[i + 1]) {
                d -= pts[i].distance(pts[i + 1]);
                i += 1;
            }
            pts[i] + (pts[i + 1] - pts[i]).normalize_or_zero() * d
        };
        let d = total * lin(t0, t1);
        let c = at(d + 0.25) - at(d - 0.25);
        (at(d), c.x.atan2(c.y))
    };
    match t {
        t if t < RISE.0 => (WAT_AT, WAT_YAW, "Fixing_Kneeling", Some(DOWN.0 + span * (0.5 - 0.5 * (PI * t * 0.8 / span).cos()))),
        t if t < RISE.1 => (WAT_AT, WAT_YAW, "Fixing_Kneeling", Some(DOWN.1 + (4.1 - DOWN.1) * lin(RISE.0, RISE.1))),
        t if t < PICK.0 => {
            let (p, yaw) = walk(&[WAT_AT, PAST_KNEES, TABLE_SPOT], RISE.1, PICK.0 - 0.1);
            (p, WAT_YAW + (yaw - WAT_YAW) * smooth(lin(RISE.1, RISE.1 + 0.5)), "Walk_Loop", None)
        }
        t if t < PICK.1 => {
            let a = TABLE_SPOT - PAST_KNEES;
            let came = a.x.atan2(a.y);
            (TABLE_SPOT, came + (WAT_YAW - came) * smooth(lin(PICK.0, PICK.0 + 0.35)), "PickUp_Table", Some(0.67 * lin(PICK.0, PICK.1 - 0.3)))
        }
        t if t < 35.6 => {
            let (p, yaw) = walk(&[TABLE_SPOT, DOOR_SPOT], PICK.1, 35.5);
            (p, yaw, "Walk_Loop", None)
        }
        // At the door, his back to the room; turned back on "Before it burned?".
        t if t < 74.2 => {
            let a = DOOR_SPOT - TABLE_SPOT;
            let came = a.x.atan2(a.y).rem_euclid(TAU);
            let yaw = if t < 45.0 { came + (PI - came) * smooth(lin(35.5, 36.3)) } else { PI + (FACE_HARROW - PI) * smooth(lin(55.0, 55.9)) };
            (DOOR_SPOT, yaw, "Idle_Loop", None)
        }
        t if t < 75.0 => (DOOR_SPOT, FACE_HARROW + (PI - FACE_HARROW) * smooth(lin(74.2, 74.9)), "Idle_Loop", None),
        _ => {
            let (p, yaw) = walk(&[DOOR_SPOT, Vec2::new(DOOR_X, -HALF_D), Vec2::new(DOOR_X, -HALF_D - 1.55)], 75.0, 78.1);
            (p, yaw, if t < 78.1 { "Walk_Loop" } else { "Idle_Loop" }, None)
        }
    }
}

fn smooth(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

/// Wat's head (body frame): on the work while he kneels; dipped, listening, at the door; on his
/// master once he turns (looking down at a seated man), eyes dropping after "That's the point of you."
fn wat_head(t: f32) -> Vec3 {
    let pitch = env(t, &[(35.6, 0.0), (36.4, -0.18), (55.0, -0.18), (55.9, -0.2), (67.8, -0.2), (68.8, -0.42), (74.2, -0.42), (75.0, 0.0)]);
    Vec3::new(0.0, pitch, 0.0)
}

/// The door's swing (radians, out into the passage) as Wat goes.
fn door_open(t: f32) -> f32 {
    1.45 * smooth(((t - 74.5) / 1.1).clamp(0.0, 1.0))
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<SceneState>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    mut ship: Query<&mut Transform, (With<Ship>, Without<Role>, Without<Lantern>, Without<Rain>, Without<Flame>, Without<Door>)>,
    mut actors: Query<
        (Entity, &mut crate::villagers::Villager, &mut Transform, &mut crate::npc_skin::SkinClip, &Role, &mut super::HeadLook),
        (Without<Ship>, Without<Lantern>, Without<Rain>, Without<Flame>, Without<Door>),
    >,
    mut door: Query<&mut Transform, (With<Door>, Without<Ship>, Without<Role>, Without<Lantern>, Without<Rain>, Without<Flame>)>,
    mut lantern: Query<&mut Transform, (With<Lantern>, Without<Ship>, Without<Role>, Without<Rain>, Without<Flame>, Without<Door>)>,
    mut rain: Query<(&mut Transform, &Rain), (Without<Ship>, Without<Role>, Without<Lantern>, Without<Flame>, Without<Door>)>,
    mut flames: Query<(&mut Transform, &Flame), (Without<Ship>, Without<Role>, Without<Lantern>, Without<Rain>, Without<Door>)>,
    mut lights: Query<(&mut PointLight, &FlameLight)>,
    mut bolt: Query<(&mut SpotLight, &Lightning)>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut fade: Query<&mut BackgroundColor, With<super::FarmFade>>,
    mut title: Query<&mut TextColor, With<Title>>,
) {
    if state.active != Some(SceneId::ShipCabin) {
        return;
    }
    let t = state.clock.max(0.0);
    let (dt, now) = (time.delta_secs(), time.elapsed_secs());
    let xf = ship_xf(t);
    for mut tf in &mut ship {
        *tf = xf;
    }
    let (_, ship_yaw, _) = xf.rotation.to_euler(EulerRot::YXZ);
    let k_head = 1.0 - (-dt * 4.0).exp();

    for (e, mut v, mut tf, mut clip, role, mut head) in &mut actors {
        let (at, yaw, want_clip, want_head, seek) = match role {
            Role::Harrow => {
                let (c, h) = harrow_beat(t);
                (CHAIR, CHAIR_YAW, c, h, None)
            }
            Role::Wat => {
                let (p, yaw, c, seek) = wat_track(t);
                (p, yaw, c, wat_head(t), seek)
            }
        };
        match seek {
            Some(at) => commands.entity(e).try_insert(crate::npc_skin::ClipSeek(at)),
            None => commands.entity(e).try_remove::<crate::npc_skin::ClipSeek>(),
        };
        let w = xf.transform_point(Vec3::new(at.x, 0.0, at.y));
        v.pos = Vec2::new(w.x, w.z);
        v.facing = ship_yaw + yaw;
        v.moving = false;
        tf.translation = w;
        tf.rotation = xf.rotation * Quat::from_rotation_y(yaw);
        if clip.0 != want_clip {
            clip.0 = want_clip;
        }
        head.want = want_head;
        let cur = head.cur;
        head.cur = cur + (head.want - cur) * k_head;
    }

    for mut tf in &mut door {
        tf.rotation = Quat::from_rotation_y(door_open(t));
    }
    // A pendulum hangs plumb in the world: against the cabin it swings opposite the roll, late.
    for mut tf in &mut lantern {
        tf.rotation = Quat::from_rotation_z(-1.6 * roll(t - 0.55)) * Quat::from_rotation_x(-1.6 * pitch(t - 0.45));
    }
    for (mut tf, f) in &mut flames {
        let s = 0.13 * (1.0 + 0.1 * (now * 8.0 + f.phase).sin() + 0.05 * (now * 15.3 + f.phase).sin());
        tf.scale = Vec3::new(s, s * (1.0 + 0.12 * (now * 10.1 + f.phase).sin()), s);
    }
    for (mut l, f) in &mut lights {
        l.intensity = f.base * (1.0 + 0.06 * (now * 9.3 + f.phase).sin() + 0.04 * (now * 23.0 + f.phase * 2.0).sin());
    }
    let fl = flash(t);
    for (mut s, l) in &mut bolt {
        s.intensity = l.base * fl;
    }
    // Rain: 5.2 u sheets falling 7 u/s, two stacked so the column never empties.
    let drop = (t * 7.0).rem_euclid(RAIN_H);
    for (mut tf, r) in &mut rain {
        tf.translation = Vec3::new(0.0, RAIN_Y0 - drop + r.phase * RAIN_H, 0.0);
        if let Some(mut m) = mats.get_mut(&r.mat) {
            m.base_color = Color::srgba(0.72 + 0.28 * fl, 0.78 + 0.22 * fl, 0.88 + 0.12 * fl, 0.2 + 0.5 * fl);
        }
    }

    match LINES.iter().find(|(s, d, ..)| t >= *s && t < s + d) {
        Some((s, d, who, text)) => subs.force(now, Some(who), text, s + d - t),
        None => subs.force(now, None, "", 0.0),
    }

    let a = env(t, &[(FADE_IN.0, 1.0), (FADE_IN.1, 0.0), (FADE_OUT.0, 0.0), (FADE_OUT.1, 1.0)]);
    for mut bg in &mut fade {
        bg.0 = Color::srgba(0.0, 0.0, 0.0, a);
    }
    let ta = env(t, &[(TITLE.0, 0.0), (TITLE.1, 1.0), (TITLE.2, 1.0), (TITLE.3, 0.0)]);
    for mut c in &mut title {
        c.0 = c.0.with_alpha(ta);
    }

    if t >= END {
        state.want = None;
    }
}

/// Rest Harrow's real pelvis on the chair: the `Sitting_*` clips are authored for a bench height
/// the chair doesn't share, so the skin instance is nudged (in the root's frame) until the pelvis
/// sits on the seat — `village_life::fit_seats`' trick.
pub(super) fn seat_fit(
    time: Res<Time>,
    state: Res<SceneState>,
    mut q: Query<(&Role, &crate::npc_skin::Skinned, &GlobalTransform, &mut SeatFit)>,
    joints: Query<&GlobalTransform, Without<Role>>,
    mut tfs: Query<&mut Transform, Without<Role>>,
) {
    if state.active != Some(SceneId::ShipCabin) {
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let xf = ship_xf(state.clock.max(0.0));
    for (role, sk, gt, mut fit) in &mut q {
        if *role != Role::Harrow {
            continue;
        }
        let (Some(pel), Ok(_)) = (sk.pelvis, tfs.get(sk.inst())) else { continue };
        let Ok(pg) = joints.get(pel) else { continue };
        let want = xf.transform_point(Vec3::new(CHAIR.x + 0.04, CHAIR_SEAT + PELVIS_UP, CHAIR.y));
        fit.0 = (fit.0 + (want - pg.translation()) * (6.0 * dt).min(1.0)).clamp(Vec3::splat(-0.4), Vec3::splat(0.4));
        let (scale, rot, _) = gt.to_scale_rotation_translation();
        if let Ok(mut tf) = tfs.get_mut(sk.inst()) {
            tf.translation = rot.inverse() * fit.0 / scale.x.max(1e-3);
        }
    }
}

/// The letter lies on the table until the lift's grab, then rides Wat's right hand.
pub(super) fn letter_follow(
    state: Res<SceneState>,
    actors: Query<(&Role, &crate::npc_skin::Skinned)>,
    joints: Query<&GlobalTransform>,
    mut letter: Query<&mut Transform, With<Letter>>,
) {
    const TAKEN: f32 = 32.75;
    if state.active != Some(SceneId::ShipCabin) {
        return;
    }
    let t = state.clock.max(0.0);
    let Ok(mut tf) = letter.single_mut() else { return };
    let hand = actors.iter().find(|(r, _)| **r == Role::Wat).and_then(|(_, sk)| sk.hand_r).and_then(|h| joints.get(h).ok());
    match hand {
        Some(g) if t >= TAKEN => {
            let xf = ship_xf(t);
            let (_, rot, pos) = g.to_scale_rotation_translation();
            tf.translation = xf.compute_affine().inverse().transform_point3(pos);
            tf.rotation = xf.rotation.inverse() * rot * Quat::from_rotation_z(FRAC_PI_2);
        }
        _ => *tf = Transform::from_translation(LETTER_ON_TABLE).with_rotation(Quat::from_rotation_y(-0.35)),
    }
}

/// Harrow's and Wat's mouths follow their lines' loudness (see `lipsync`).
pub(super) fn mouths(
    mut commands: Commands,
    state: Res<SceneState>,
    server: Res<AssetServer>,
    sources: Res<Assets<AudioSource>>,
    mut clips: Local<Vec<(Handle<AudioSource>, Option<Vec<f32>>)>>,
    actors: Query<(Entity, &Role)>,
) {
    if state.active != Some(SceneId::ShipCabin) {
        return;
    }
    if clips.is_empty() {
        *clips = VO.iter().map(|vo| (server.load(vo.1), None)).collect();
    }
    for ((h, env), vo) in clips.iter_mut().zip(&VO) {
        if env.is_none() {
            *env = sources.get(&*h).map(|s| crate::lipsync::envelope(s, vo.2, vo.3));
        }
    }
    let t = state.clock.max(0.0);
    for (e, role) in &actors {
        let open = clips
            .iter()
            .zip(&VO)
            .filter(|(_, vo)| vo.4 == *role)
            .filter_map(|((_, env), vo)| env.as_deref().map(|env| crate::lipsync::sample(env, t - vo.0)))
            .fold(0.0, f32::max);
        commands.entity(e).try_insert(crate::lipsync::MouthOpen(open));
    }
}

// ── Coverage ────────────────────────────────────────────────────────────────────────────

/// The shot at `t` in the ship's frame: (eye, look-at, vertical fov°, thin-lens aperture, focus
/// distance). The camera rides the ship, so the cabin holds still and the sea moves.
fn shot_local(t: f32) -> (Vec3, Vec3, f32, f32, Option<f32>) {
    let lin = |t0: f32, t1: f32| ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    let ease = |u: f32| u * u * (3.0 - 2.0 * u);
    let v = Vec3::new;
    // Wat on his feet, wherever his track has him.
    let wat = {
        let (p, ..) = wat_track(t);
        v(p.x, WAT_STAND_H, p.y)
    };
    let (eye, at, fov, band, focus_on) = match t {
        // ── I. The strap ──
        // A · the cabin from the door: the lantern swinging over them, the squire at his master's
        //     leg, the stern windows behind them full of rain. Nobody speaks.
        t if t < 10.0 => (v(-1.05, 1.4, -1.45).lerp(v(-0.82, 1.32, -1.2), ease(lin(4.6, 10.0))), v(0.6, 0.72, 0.7), 58.0, 0.12, Some(HARROW_HEAD)),
        // C · the hands at the strap: a pull, the buckle won't sit.
        t if t < 12.0 => (v(0.66, 0.52, 1.2).lerp(v(0.64, 0.5, 1.15), lin(10.0, 12.0)), STRAP + v(0.0, 0.04, 0.0), 36.0, 1.0, Some(STRAP)),
        // B · "Tighter." — low over Wat's shoulder, up at Harrow (the squire's eye).
        t if t < 13.2 => (v(-0.25, 0.76, 0.6), HARROW_HEAD, 36.0, 0.9, Some(HARROW_HEAD)),
        // C · Wat hauls on it.
        t if t < 14.8 => (v(0.62, 0.5, 1.14), STRAP + v(0.0, 0.03, 0.0), 34.0, 1.0, Some(STRAP)),
        // B2 · "You've done that strap every day for fifteen years and it's still wrong." — pushing
        //      in on him; the shield on the hull behind his head.
        t if t < 20.4 => {
            let k = ease(lin(14.8, 20.4));
            (v(0.0, 0.86, 1.24).lerp(v(0.2, 0.88, 1.12), k), HARROW_HEAD + v(0.15, 0.0, 0.12), 34.0 - 4.0 * k, 1.0, Some(HARROW_HEAD))
        }
        // C2 · "Yes, Ser." — Wat's face, low, eyes on the work.
        t if t < 23.4 => (v(0.66, 0.66, 1.46).lerp(v(0.63, 0.66, 1.4), lin(20.4, 23.4)), WAT_HEAD, 34.0, 0.9, Some(WAT_HEAD)),
        // H1 · "Hnn. There." — the buckle home.
        t if t < 26.0 => (v(0.6, 0.48, 1.13), STRAP + v(0.0, 0.03, 0.0), 32.0, 1.0, Some(STRAP)),
        // ── II. The letter ──
        // D · "The letter. To the Reeve. Today, if the sea allows." — Harrow from his right, his eyes
        //     going to the table: the helmet, the sealed letter by it.
        t if t < 29.4 => (v(0.2, 0.92, -0.55).lerp(v(0.24, 0.93, -0.48), lin(26.0, 29.4)), v(0.92, 0.86, 0.45), 40.0, 0.8, Some(HARROW_HEAD)),
        // W1 · from the stern corner: Wat gets up, goes round the lantern to the table, takes the letter.
        t if t < 33.6 => {
            let k = ease(lin(29.6, 32.4));
            (v(-1.45, 1.45, 1.3), v(0.25, 0.75, 0.6).lerp(v(0.75, 0.82, -0.25), k), 50.0, 0.3, Some(wat))
        }
        // W2 · from the stern windows: Wat to the door with it — and he stops there; the hymn
        //      coming up through the deck.
        t if t < 38.8 => (v(-0.6, 1.35, 1.35), wat.lerp(v(-0.9, 1.2, -1.4), 0.6), 44.0, 0.45, Some(wat)),
        // D2 · "Forty peasants and a Reeve. That's what the Crown sends a man after the eastern
        //      campaign." — Harrow from the door end, listening down to the hymn, pushing in. (The
        //      lantern hangs on the straight line from Wat to him: the lens sits starboard of it.)
        t if t < 46.6 => {
            let k = ease(lin(38.8, 46.6));
            (v(0.0, 1.25, -1.25).lerp(v(0.1, 1.22, -1.05), k), HARROW_HEAD, 28.0 - 4.0 * k, 0.8, Some(HARROW_HEAD))
        }
        // ── III. The city ──
        // The storm in the stern windows, a far flash.
        t if t < 49.4 => (v(0.95, 1.18, 1.25), v(1.25, 0.8, 6.0), 50.0, 0.25, None),
        // E · "Do you know what they said about me in the city?" — he's turned to the windows; from
        //     the glass, his face, and the lightning on "Before it burned?" behind the lens.
        t if t < 55.4 => (v(0.42, 1.12, 1.46).lerp(v(0.46, 1.11, 1.4), lin(49.4, 55.4)), HARROW_HEAD, 34.0, 0.6, Some(HARROW_HEAD)),
        // F · "No, Ser." — Wat turned back at the door, past his master's shoulder.
        t if t < 58.6 => (v(1.05, 1.12, 0.3), WAT_DOOR_HEAD, 30.0, 0.8, Some(WAT_DOOR_HEAD)),
        // G1 · "Neither do I." — Harrow turns from the glass to him: low, from Wat's side of the lantern.
        t if t < 62.4 => (v(-0.3, 1.12, -1.0), HARROW_HEAD, 26.0, 0.8, Some(HARROW_HEAD)),
        // G2 · "Nobody looks at the man behind the shield, Wat." — over Wat's right shoulder.
        t if t < 66.4 => (v(-1.48, 1.45, -1.29).lerp(v(-1.42, 1.44, -1.24), lin(62.4, 66.4)), HARROW_HEAD, 36.0, 0.8, Some(HARROW_HEAD)),
        // H · "That's the point of you." — on Wat, a slow push; his eyes go down.
        t if t < 70.6 => {
            let face = WAT_DOOR_HEAD - v(0.0, 0.08, 0.0);
            (v(-0.25, 1.12, -0.45).lerp(v(-0.4, 1.15, -0.6), ease(lin(66.4, 70.6))), face, 32.0, 0.9, Some(face))
        }
        // I · "Stop hovering." — Harrow back at the window.
        t if t < 74.2 => (v(0.52, 1.1, 1.32), HARROW_HEAD, 30.0, 0.7, Some(HARROW_HEAD)),
        // J · Wat goes: from behind him, down the passage's length — the door, the hold's lamplight
        //     at the far end, his back against it.
        t if t < 78.6 => (v(-0.72, 1.45, 0.25), v(-1.0, 1.05, -3.0), 40.0, 0.4, Some(wat)),
        // K · Harrow alone with the lantern, from the door he left by; into black.
        _ => (v(-0.85, 1.34, -1.25).lerp(v(-1.0, 1.4, -1.48), ease(lin(78.6, END))), v(0.6, 0.72, 0.7), 46.0 + 10.0 * lin(78.6, END), 0.15, Some(HARROW_HEAD)),
    };
    (eye, at, fov, band, focus_on.map(|p| eye.distance(p)))
}

/// The shot in the world: ((eye, look-at, fov, aperture), focus) + the ship's up for `look_at`.
pub(super) fn camera(t: f32) -> ((super::Cam, Option<f32>), Vec3) {
    let xf = ship_xf(t);
    let (eye, at, fov, band, focus) = shot_local(t);
    (((xf.transform_point(eye), xf.transform_point(at), fov, band), focus), xf.rotation * Vec3::Y)
}

// ── Blockout meshes (vertex-coloured, flat-shaded — stand-ins for the kit slots) ───────────

/// Rain sheet extent: falls through `RAIN_Y0 - RAIN_H .. RAIN_Y0`.
const RAIN_H: f32 = 5.2;
const RAIN_Y0: f32 = 0.0;

/// A cheap stable hash → 0..1 (plank shades, rain scatter).
fn h01(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85eb_ca6b;
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 16;
    (x & 0xffff) as f32 / 65535.0
}

fn plank(i: u32) -> u32 {
    [0x5b3d24, 0x654429, 0x553923, 0x5f412a, 0x4f3520][(h01(i) * 5.0) as usize % 5]
}

fn cube_r(w: f32, h: f32, d: f32, off: Vec3, rot: Quat, hex: u32) -> Mesh {
    crate::meshkit::tinted_hex(Cuboid::new(w, h, d).mesh().build().rotated_by(rot).translated_by(off), hex)
}

/// The great cabin: plank floor, deck beams and deckhead, the hull's ceiling planks and frames each
/// side, the forward bulkhead with its door, the stern wall with three leaded windows over a bench.
fn shell_mesh() -> Vec<Mesh> {
    let mut p = Vec::new();
    // floor
    p.push(cube(2.0 * HALF_W + 0.3, 0.1, 2.0 * HALF_D + 0.3, Vec3::new(0.0, -0.1, 0.0), 0x2b1d12));
    let n = (2.0 * HALF_W / 0.21) as u32;
    for i in 0..n {
        let x = -HALF_W + 0.105 + i as f32 * 0.21;
        p.push(cube(0.2, 0.05, 2.0 * HALF_D, Vec3::new(x, -0.025, 0.0), plank(i)));
    }
    // deck beams + deckhead
    p.push(cube(2.0 * HALF_W + 0.3, 0.06, 2.0 * HALF_D + 0.3, Vec3::new(0.0, BEAMS + 0.17, 0.0), 0x3a2817));
    for z in [-1.5f32, -0.9, -0.3, 0.3, 0.9, 1.5] {
        p.push(cube(2.0 * HALF_W, 0.14, 0.13, Vec3::new(0.0, BEAMS + 0.07, z), 0x4a3320));
    }
    p.push(cube(0.12, 0.08, 2.0 * HALF_D, Vec3::new(0.0, BEAMS + 0.1, 0.0), 0x412d1c)); // carling
    // hull sides: strakes of ceiling planking, frames standing proud of them
    for (s, side) in [(0u32, -1.0f32), (1, 1.0)] {
        for k in 0..10u32 {
            let y = k as f32 * 0.19 + 0.095;
            p.push(cube(0.06, 0.18, 2.0 * HALF_D, Vec3::new(side * (HALF_W + 0.03), y, 0.0), plank(100 + s * 20 + k)));
        }
        for z in [-1.4f32, -0.85, -0.3, 0.25, 0.8, 1.35] {
            p.push(cube(0.1, BEAMS + 0.14, 0.12, Vec3::new(side * (HALF_W - 0.04), (BEAMS + 0.14) * 0.5, z), 0x45301d));
        }
        // the shelf along the top of the frames
        p.push(cube(0.14, 0.08, 2.0 * HALF_D, Vec3::new(side * (HALF_W - 0.06), BEAMS - 0.04, 0.0), 0x3f2b1a));
    }
    // forward bulkhead: vertical boards round the door opening (the leaf is its own piece, `door_mesh`)
    let door_x = DOOR_X;
    let (open0, open1, door_h) = (door_x - DOOR_W * 0.5, door_x + DOOR_W * 0.5, 1.46);
    let tall = BEAMS + 0.14;
    for i in 0..n {
        let x = -HALF_W + 0.105 + i as f32 * 0.21;
        let (b0, b1) = (x - 0.1, x + 0.1);
        let z = -HALF_D - 0.03;
        if b1 <= open0 || b0 >= open1 {
            p.push(cube(0.2, tall, 0.06, Vec3::new(x, tall * 0.5, z), plank(200 + i)));
            continue;
        }
        // over the opening: the board's head above the door; beside it, the strip it leaves open
        p.push(cube(0.2, tall - door_h, 0.06, Vec3::new(x, (tall + door_h) * 0.5, z), plank(200 + i)));
        for (a, b) in [(b0, b1.min(open0)), (b0.max(open1), b1)] {
            if b - a > 0.005 {
                p.push(cube(b - a, door_h, 0.06, Vec3::new((a + b) * 0.5, door_h * 0.5, z), plank(200 + i)));
            }
        }
    }
    for x in [door_x - 0.38, door_x + 0.38] {
        p.push(cube(0.07, 1.52, 0.08, Vec3::new(x, 0.76, -HALF_D + 0.02), 0x3b2817));
    }
    p.push(cube(0.83, 0.07, 0.08, Vec3::new(door_x, 1.5, -HALF_D + 0.02), 0x3b2817));
    // the passage forward of the door: a short boarded tunnel Wat goes out into, lit only from the
    // hold at its far end
    let (pz0, pz1) = (-HALF_D - 0.06, -HALF_D - PASSAGE);
    let (pc, pl) = ((pz0 + pz1) * 0.5, pz0 - pz1);
    p.push(cube(1.0, 0.06, pl, Vec3::new(door_x, -0.03, pc), 0x2b1d12));
    p.push(cube(1.0, 0.06, pl, Vec3::new(door_x, 1.83, pc), 0x2b1d12));
    for x in [door_x - 0.53, door_x + 0.53] {
        p.push(cube(0.06, 1.86, pl, Vec3::new(x, 0.93, pc), 0x30200f));
    }
    p.push(cube(1.0, 1.86, 0.06, Vec3::new(door_x, 0.93, pz1), 0x30200f));
    // stern wall around the windows
    let zs = HALF_D + 0.03;
    for i in 0..n {
        let x = -HALF_W + 0.105 + i as f32 * 0.21;
        p.push(cube(0.2, SILL, 0.06, Vec3::new(x, SILL * 0.5, zs), plank(300 + i)));
    }
    p.push(cube(2.0 * HALF_W, BEAMS + 0.14 - HEAD, 0.06, Vec3::new(0.0, (HEAD + BEAMS + 0.14) * 0.5, zs), 0x4d3420));
    let mut piers = vec![(-HALF_W, WINDOWS[0].0)];
    for w in WINDOWS.windows(2) {
        piers.push((w[0].1, w[1].0));
    }
    piers.push((WINDOWS[2].1, HALF_W));
    for (a, b) in piers {
        p.push(cube(b - a, HEAD - SILL, 0.08, Vec3::new((a + b) * 0.5, (SILL + HEAD) * 0.5, zs), 0x4a321e));
    }
    // window frames + leading (open lights: the storm is real, beyond them)
    for (a, b) in WINDOWS {
        let (cx, w, h, cy) = ((a + b) * 0.5, b - a, HEAD - SILL, (SILL + HEAD) * 0.5);
        p.push(cube(w, 0.05, 0.1, Vec3::new(cx, SILL + 0.025, zs), 0x2e2116));
        p.push(cube(w, 0.05, 0.1, Vec3::new(cx, HEAD - 0.025, zs), 0x2e2116));
        p.push(cube(0.05, h, 0.1, Vec3::new(a + 0.025, cy, zs), 0x2e2116));
        p.push(cube(0.05, h, 0.1, Vec3::new(b - 0.025, cy, zs), 0x2e2116));
        p.push(cube(0.02, h, 0.03, Vec3::new(cx, cy, zs), 0x262422));
        for y in [SILL + h / 3.0, SILL + 2.0 * h / 3.0] {
            p.push(cube(w, 0.02, 0.03, Vec3::new(cx, y, zs), 0x262422));
        }
    }
    // the stern bench under the windows
    p.push(cube(3.1, 0.07, 0.44, Vec3::new(0.0, 0.42, HALF_D - 0.22), 0x5c3f27));
    p.push(cube(3.1, 0.38, 0.04, Vec3::new(0.0, 0.2, HALF_D - 0.44), 0x4e3520));
    p.push(cube(1.2, 0.08, 0.38, Vec3::new(-0.9, 0.49, HALF_D - 0.22), 0x6a3a2e)); // a cushion
    p
}

/// A heavy trestle table (origin on the floor under its centre).
fn table_mesh() -> Vec<Mesh> {
    let mut p = vec![cube(0.72, 0.05, 1.0, Vec3::new(0.0, TABLE_TOP - 0.025, 0.0), 0x6b4a2c)];
    for (x, z) in [(-0.3f32, -0.42f32), (0.3, -0.42), (-0.3, 0.42), (0.3, 0.42)] {
        p.push(cube(0.06, TABLE_TOP - 0.05, 0.06, Vec3::new(x, (TABLE_TOP - 0.05) * 0.5, z), WOOD_DARK));
    }
    p.push(cube(0.05, 0.05, 0.84, Vec3::new(0.0, 0.14, 0.0), WOOD_DARK));
    p
}

/// Harrow's chair (origin on the floor under the seat, facing +Z: the back at −Z).
fn chair_mesh() -> Vec<Mesh> {
    let mut p = vec![
        cube(0.46, 0.05, 0.44, Vec3::new(0.0, CHAIR_SEAT - 0.025, 0.0), 0x5a3b22),
        cube(0.46, 0.62, 0.05, Vec3::new(0.0, CHAIR_SEAT + 0.31, -0.22), 0x4f331e),
    ];
    for (x, z) in [(-0.2f32, -0.19f32), (0.2, -0.19), (-0.2, 0.19), (0.2, 0.19)] {
        p.push(cube(0.05, CHAIR_SEAT - 0.05, 0.05, Vec3::new(x, (CHAIR_SEAT - 0.05) * 0.5, z), WOOD_DARK));
    }
    for x in [-0.22f32, 0.22] {
        p.push(cube(0.05, 0.04, 0.42, Vec3::new(x, CHAIR_SEAT + 0.22, 0.0), WOOD_DARK)); // arms
        p.push(cube(0.04, 0.22, 0.04, Vec3::new(x, CHAIR_SEAT + 0.11, 0.18), WOOD_DARK));
    }
    p
}

/// Wat's hammock, slung along the port hull in the forward corner, ropes to the beams.
fn hammock_mesh() -> Vec<Mesh> {
    let (x, z0, z1, top, sag) = (-HALF_W + 0.4, -HALF_D + 0.15, -0.05, 1.5, 0.36);
    let mut p = Vec::new();
    let n = 12;
    for i in 0..n {
        let (u0, u1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let y = |u: f32| top - sag * (PI * u).sin();
        let (a, b) = (Vec3::new(x, y(u0), z0 + (z1 - z0) * u0), Vec3::new(x, y(u1), z0 + (z1 - z0) * u1));
        let d = b - a;
        let rot = Quat::from_rotation_x(-d.y.atan2(d.z));
        p.push(cube_r(0.5, 0.03, d.length() + 0.01, (a + b) * 0.5, rot, 0xa89a7c));
    }
    p.push(cube(0.5, 0.12, 0.5, Vec3::new(x, top - sag + 0.06, (z0 + z1) * 0.5), 0x8a7a5e)); // a rolled blanket
    for z in [z0, z1] {
        p.push(cylr(0.012, BEAMS - top, Vec3::new(x, (BEAMS + top) * 0.5, z), Quat::IDENTITY, 0x9a8a68));
    }
    p
}

fn sea_chest_mesh() -> Vec<Mesh> {
    vec![
        cube(0.8, 0.42, 0.46, Vec3::new(0.0, 0.21, 0.0), 0x5a3a20),
        cube(0.82, 0.08, 0.48, Vec3::new(0.0, 0.46, 0.0), 0x4a301b),
        cube(0.84, 0.04, 0.03, Vec3::new(0.0, 0.3, 0.24), IRON),
        cube(0.84, 0.04, 0.03, Vec3::new(0.0, 0.3, -0.24), IRON),
    ]
}

fn crate_mesh() -> Vec<Mesh> {
    vec![cube(0.8, 0.8, 0.8, Vec3::new(0.0, 0.4, 0.0), 0x7a5a36)]
}

/// Two casks lashed in the forward corner.
fn casks_mesh() -> Vec<Mesh> {
    let mut p = Vec::new();
    for (i, x) in [-0.22f32, 0.22].into_iter().enumerate() {
        p.push(cylr(0.19, 0.5, Vec3::new(x, 0.25, 0.0), Quat::IDENTITY, if i == 0 { WOOD } else { 0x5e4026 }));
        for y in [0.08f32, 0.42] {
            p.push(cylr(0.2, 0.03, Vec3::new(x, y, 0.0), Quat::IDENTITY, IRON));
        }
    }
    p
}

/// What's on the table (origin = table top centre): a sea chart weighted open, a pewter cup, an
/// inkwell, the candle in its holder (its flame is a separate card). The letter is its own piece.
fn table_dressing_mesh() -> Vec<Mesh> {
    vec![
        cube_r(0.4, 0.004, 0.32, Vec3::new(-0.08, 0.002, -0.12), Quat::from_rotation_y(0.2), 0xcbb98f),
        cylr(0.025, 0.36, Vec3::new(-0.05, 0.025, 0.08), Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_x(0.2), 0xbfa97a),
        cylr(0.032, 0.09, Vec3::new(-0.2, 0.045, 0.3), Quat::IDENTITY, 0x8d8f92),
        cylr(0.022, 0.04, Vec3::new(0.05, 0.02, -0.33), Quat::IDENTITY, 0x1c1a1a),
        cylr(0.045, 0.012, Vec3::new(0.18, 0.006, -0.3), Quat::IDENTITY, 0x6f5a3c),
        cylr(0.02, 0.13, Vec3::new(0.18, 0.075, -0.3), Quat::IDENTITY, 0xe6dcc0),
    ]
}

/// The door sconce (origin on the bulkhead face, +Z into the cabin): back plate, arm, drip dish, candle.
fn sconce_mesh() -> Vec<Mesh> {
    vec![
        cube(0.08, 0.2, 0.012, Vec3::new(0.0, 0.04, 0.0), 0x3a3a3e),
        cube(0.018, 0.018, 0.08, Vec3::new(0.0, -0.02, 0.04), IRON),
        cylr(0.042, 0.012, Vec3::new(0.0, -0.01, 0.08), Quat::IDENTITY, 0x3a3a3e),
        cylr(0.017, 0.09, Vec3::new(0.0, 0.05, 0.08), Quat::IDENTITY, 0xe6dcc0),
    ]
}

/// The sealed letter to the Reeve (origin at its centre): folded paper, the red wax seal.
fn letter_mesh() -> Vec<Mesh> {
    vec![cube(0.16, 0.012, 0.1, Vec3::ZERO, 0xd9cfb4), cylr(0.018, 0.006, Vec3::new(0.0, 0.008, 0.0), Quat::IDENTITY, 0x8a1d18)]
}

/// The cabin door's leaf (origin on its hinge, the port jamb; the leaf runs +X): boards, two iron
/// strap hinges, the ring pull on the latch side.
fn door_mesh() -> Vec<Mesh> {
    let mut p = vec![cube(DOOR_W, 1.46, 0.05, Vec3::new(DOOR_W * 0.5, 0.73, 0.0), 0x4a311c)];
    for y in [0.3f32, 1.15] {
        p.push(cube(0.56, 0.04, 0.015, Vec3::new(0.3, y, 0.03), IRON));
    }
    p.push(cylr(0.03, 0.02, Vec3::new(DOOR_W - 0.11, 0.78, 0.04), Quat::from_rotation_x(FRAC_PI_2), IRON));
    p
}

/// The hanging lantern (origin at the hook, hanging down −Y): chain, cap, cage, base. The glowing
/// horn panes are a separate emissive box.
fn lantern_mesh() -> Vec<Mesh> {
    let mut p = vec![
        cylr(0.008, 0.22, Vec3::new(0.0, -0.11, 0.0), Quat::IDENTITY, IRON),
        super::cone_m(0.1, 0.09, Vec3::new(0.0, -0.27, 0.0), Quat::IDENTITY, 0x2f2f33),
        cube(0.15, 0.02, 0.15, Vec3::new(0.0, -0.31, 0.0), 0x2f2f33),
        cube(0.15, 0.025, 0.15, Vec3::new(0.0, -0.5, 0.0), 0x2f2f33),
    ];
    for (x, z) in [(-0.065f32, -0.065f32), (0.065, -0.065), (-0.065, 0.065), (0.065, 0.065)] {
        p.push(cylr(0.007, 0.19, Vec3::new(x, -0.405, z), Quat::IDENTITY, IRON));
    }
    p
}

/// One rain sheet: thin falling streaks in the air beyond the stern windows.
fn rain_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let (mut pos, mut nrm, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..340u32 {
        let x = -2.8 + 5.6 * h01(i * 3 + 1);
        let z = HALF_D + 0.35 + 4.5 * h01(i * 3 + 2).powf(1.4);
        let y = RAIN_H * h01(i * 3 + 3) - 2.2;
        let (w, l, lean) = (0.004, 0.16 + 0.12 * h01(i * 7), 0.03);
        let base = pos.len() as u32;
        for (dx, dy) in [(-w, 0.0), (w, 0.0), (w + lean, l), (-w + lean, l)] {
            pos.push([x + dx, y + dy, z]);
            nrm.push([0.0, 0.0, -1.0]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_indices(Indices::U32(idx));
    m
}
