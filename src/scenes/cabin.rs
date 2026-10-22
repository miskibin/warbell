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
pub(super) const END: f32 = 143.6;
/// Title card on black, then the picture fades up.
const TITLE: (f32, f32, f32, f32) = (0.9, 1.9, 3.5, 4.4);
const FADE_IN: (f32, f32) = (4.6, 6.4);
const FADE_OUT: (f32, f32) = (140.9, 143.4);

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

/// The VO, one clip per spoken beat of the recording (`p0_cabin_NN.ogg`, cut from the one-take
/// ElevenLabs session at its own pauses and laid out by `tools/cutscene/cabin_mix.py`):
/// (start s, clip, trim from..to s, speaker). Read here only for the mouths and the captions; the
/// soundtrack is mixed offline from the same timeline.
pub(super) const VO: &[(f32, &str, f32, f32, Role)] = &[
    (8.97, "audio/vo/campaign/p0_cabin_01.ogg", 0.0, 0.93, Role::Harrow), // "Tighter."
    (10.51, "audio/vo/campaign/p0_cabin_02.ogg", 0.0, 8.49, Role::Harrow), // "Left buckle's always stiff. Fifteen years you've strapped me into this iron, Wat, and you still treat it like it's going to bite you."
    (19.31, "audio/vo/campaign/p0_cabin_03.ogg", 0.0, 3.75, Role::Wat), // "Rust from the salt, Ser. Hnn. There."
    (23.67, "audio/vo/campaign/p0_cabin_04.ogg", 0.0, 4.57, Role::Harrow), // "Stand up. You're making my knees ache just looking at you."
    (29.95, "audio/vo/campaign/p0_cabin_05a.ogg", 0.0, 3.28, Role::Harrow), // "Listen to them. Singing."
    (42.64, "audio/vo/campaign/p0_cabin_05b.ogg", 0.0, 2.27, Role::Harrow), // "Wait till they see the island."
    (45.27, "audio/vo/campaign/p0_cabin_06.ogg", 0.0, 1.46, Role::Wat), // "Have you seen it, Ser? Kell?"
    (46.99, "audio/vo/campaign/p0_cabin_07.ogg", 0.0, 5.53, Role::Harrow), // "Nobody's seen Kell in a hundred years, Wat. That's why the King's giving it to me."
    (52.78, "audio/vo/campaign/p0_cabin_08.ogg", 0.0, 8.77, Role::Harrow), // "You take one city too roughly in the east, and suddenly the Crown discovers a desperate need for your talents on a wet rock at the edge of the map."
    (61.81, "audio/vo/campaign/p0_cabin_09.ogg", 0.0, 3.06, Role::Wat), // "They made you Lord Commissioner, Ser."
    (65.13, "audio/vo/campaign/p0_cabin_10.ogg", 0.0, 5.87, Role::Harrow), // "Over forty seasick turnip-farmers and a village Reeve. A glorious command."
    (71.81, "audio/vo/campaign/p0_cabin_11.ogg", 0.0, 5.48, Role::Harrow), // "Take the parchment to the woman downstairs. The Reeve. Tell her we sight land at dawn."
    (79.80, "audio/vo/campaign/p0_cabin_12.ogg", 0.0, 1.57, Role::Wat), // "Which one, Ser?"
    (81.73, "audio/vo/campaign/p0_cabin_13.ogg", 0.0, 5.53, Role::Harrow), // "The one without the royal wax, Wat. I keep forgetting I didn't hire you for your reading."
    (89.17, "audio/vo/campaign/p0_cabin_14.ogg", 0.0, 3.18, Role::Harrow), // "Did she ask about me? When you brought the water down?"
    (92.81, "audio/vo/campaign/p0_cabin_15.ogg", 0.0, 4.79, Role::Wat), // "She asked why the great Ser Godric Harrow hasn't come out of his cabin since we sailed."
    (97.86, "audio/vo/campaign/p0_cabin_16.ogg", 0.0, 2.86, Role::Harrow), // "And what did my loyal squire tell her?"
    (101.08, "audio/vo/campaign/p0_cabin_17.ogg", 0.0, 2.67, Role::Wat), // "That you were resting after the campaign, Ser."
    (104.56, "audio/vo/campaign/p0_cabin_18.ogg", 0.0, 10.48, Role::Harrow), // "Good man. Let them keep singing to the suit of armour in their heads. The moment I walk down there without that helmet on, I'm just an old butcher with bad knees."
    (115.75, "audio/vo/campaign/p0_cabin_19.ogg", 0.0, 7.93, Role::Harrow), // "Nobody looks at the man inside the iron, Wat. They just want to know the iron is there. Go on. Stop hovering."
];

/// Captions: (start s, duration s, speaker, line) -- verbatim, one per sentence of the [`VO`].
const LINES: &[(f32, f32, &str, &str)] = &[
    (9.00, 0.84, "Harrow", "Tighter."),
    (10.54, 1.72, "Harrow", "Left buckle's always stiff."),
    (12.26, 6.68, "Harrow", "Fifteen years you've strapped me into this iron, Wat, and you still treat it like it's going to bite you."),
    (19.34, 3.66, "Wat", "Rust from the salt, Ser. Hnn. There."),
    (23.70, 4.48, "Harrow", "Stand up. You're making my knees ache just looking at you."),
    (29.98, 3.19, "Harrow", "Listen to them. Singing."),
    (42.67, 2.18, "Harrow", "Wait till they see the island."),
    (45.30, 1.37, "Wat", "Have you seen it, Ser? Kell?"),
    (47.02, 2.89, "Harrow", "Nobody's seen Kell in a hundred years, Wat."),
    (49.91, 2.55, "Harrow", "That's why the King's giving it to me."),
    (52.81, 8.68, "Harrow", "You take one city too roughly in the east, and suddenly the Crown discovers a desperate need for your talents on a wet rock at the edge of the map."),
    (61.84, 2.97, "Wat", "They made you Lord Commissioner, Ser."),
    (65.16, 4.28, "Harrow", "Over forty seasick turnip-farmers and a village Reeve."),
    (69.44, 1.50, "Harrow", "A glorious command."),
    (71.84, 3.42, "Harrow", "Take the parchment to the woman downstairs. The Reeve."),
    (75.26, 1.97, "Harrow", "Tell her we sight land at dawn."),
    (79.83, 1.48, "Wat", "Which one, Ser?"),
    (81.76, 2.16, "Harrow", "The one without the royal wax, Wat."),
    (83.92, 3.28, "Harrow", "I keep forgetting I didn't hire you for your reading."),
    (89.20, 1.22, "Harrow", "Did she ask about me?"),
    (90.42, 1.87, "Harrow", "When you brought the water down?"),
    (92.84, 4.70, "Wat", "She asked why the great Ser Godric Harrow hasn't come out of his cabin since we sailed."),
    (97.89, 2.77, "Harrow", "And what did my loyal squire tell her?"),
    (101.11, 2.58, "Wat", "That you were resting after the campaign, Ser."),
    (104.59, 4.45, "Harrow", "Good man. Let them keep singing to the suit of armour in their heads."),
    (109.04, 5.94, "Harrow", "The moment I walk down there without that helmet on, I'm just an old butcher with bad knees."),
    (115.78, 3.30, "Harrow", "Nobody looks at the man inside the iron, Wat."),
    (119.08, 3.52, "Harrow", "They just want to know the iron is there. Go on."),
    (122.59, 1.03, "Harrow", "Stop hovering."),
];

/// Lightning outside the stern: (time, strength). Thunder rolls in ~1.2 s behind each (the mix). The
/// big ones: over the ship while the hymn plays, the door, the window before "Nobody looks...", and the
/// reveal as the camera backs out of the stern.
const STRIKES: &[(f32, f32)] = &[(5.4, 0.55), (16.0, 0.25), (36.8, 0.8), (49.0, 0.3), (68.0, 0.35), (89.6, 0.6), (99.5, 0.3), (113.6, 1.0), (126.4, 0.5), (129.9, 0.9), (136.0, 0.3), (141.0, 0.4)];

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

/// The ship seen from outside (CC-BY "Medieval Ship" by Juoda, Sketchfab): shown only for the
/// exterior shots, so it never encloses the cabin the interior is filmed in.
#[derive(Component)]
pub(super) struct ShipModel;

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

/// One dust speck in the lamplight (its index seeds its drift).
#[derive(Component)]
pub(super) struct Mote(u32);
const MOTES: u32 = 140;

/// Where speck `i` drifts at `t` (ship frame) and how bright it is: a slow wander through a ball
/// round the lantern's glass, each on its own phases, fading out toward the ball's edge.
fn mote(i: u32, t: f32) -> (Vec3, f32) {
    let r = |k: u32| h01(i.wrapping_mul(7919).wrapping_add(k));
    let c = LANTERN_HOOK + Vec3::new(0.0, -0.75, 0.0);
    let base = Vec3::new(r(1) - 0.5, r(2) - 0.5, r(3) - 0.5) * Vec3::new(2.2, 1.3, 2.2);
    let wob = Vec3::new(
        (t * (0.07 + 0.05 * r(4)) + r(5) * 6.3).sin(),
        (t * (0.05 + 0.04 * r(6)) + r(7) * 6.3).sin() * 0.6 - (t * 0.01 + r(8)).fract() * 0.0,
        (t * (0.06 + 0.05 * r(9)) + r(10) * 6.3).cos(),
    ) * 0.18;
    let p = c + base + wob;
    let d = (p - (LANTERN_HOOK + Vec3::new(0.0, -0.41, 0.0))).length();
    let lit = (1.0 - d / 1.25).clamp(0.0, 1.0);
    (p, lit * lit * (0.55 + 0.45 * (t * (0.8 + r(11)) + r(12) * 6.3).sin().abs()))
}

/// The moonlight key on Harrow's face through the stern window (`base` lux, x7 at a flash's peak).
#[derive(Component)]
pub(super) struct FaceMoon {
    base: f32,
}

#[derive(Component)]
pub(super) struct Rain {
    mat: Handle<StandardMaterial>,
    phase: f32,
}

/// A hand on the exterior ship's deck: a silhouette against the sky and the stern lantern. Place in the
/// exterior rig's frame, driven by `crew_follow`.
#[derive(Component)]
pub(super) struct Crew {
    at: Vec3,
    yaw: f32,
}

#[derive(Component)]
pub(super) struct ExtRain {
    phase: f32,
}

#[derive(Component)]
pub(super) struct Title;

/// The seat fit: world offset that rests Harrow's real pelvis on the chair.
#[derive(Component, Default)]
pub(super) struct SeatFit(Vec3);

/// The exterior ship (CC-BY "Caravel Ship", Sketchfab) is a different ship from the cabin's, moored
/// out to sea off its stern: the exterior shots are cut to it, so nothing has to line up with the
/// cabin set. Model units: ~74 across the beam, bow toward +Z; scaled to a ~7 m caravel with its
/// waterline (model y 10) on the rig's origin, and centred.
const MODEL_SCALE: f32 = 0.1;
const MODEL_AT: Vec3 = Vec3::new(0.74, -1.0, 1.0);
/// How far astern of the cabin ship the exterior ship lies, and its heading relative to it. Close
/// enough that the cabin's two actors stay inside the NPC anim-cull range (`npc_skin::ANIM_CULL2`, 90 u)
/// and behind the camera, so they are live the moment the picture cuts back (render with
/// `FOREST_ANIMLOD=0 FOREST_SKINSHADOW=all` so off-screen people are not parked either).
const EXT_DIST: f32 = 45.0;
const EXT_YAW: f32 = 0.0;
/// The finale's exterior shot (the camera has backed out of the stern window and cut to the sea).
const EXT_FINAL: f32 = 130.0;

/// The exterior shots: the ship over the hymn, and the pull-back finale.
fn ext_shot(t: f32) -> bool {
    (at::SHIP.0..at::SHIP.1).contains(&t) || t >= EXT_FINAL
}

fn ext_roll(t: f32) -> f32 {
    0.05 * (TAU * t / 6.1).sin() + 0.015 * (TAU * t / 2.7 + 0.4).sin()
}
fn ext_pitch(t: f32) -> f32 {
    0.028 * (TAU * t / 4.8 + 1.1).sin()
}

/// The exterior ship's place on the sea, level (the camera's frame).
fn ext_flat() -> Transform {
    let (p, yaw) = anchor();
    let aft = Quat::from_rotation_y(yaw) * Vec3::Z;
    Transform {
        translation: Vec3::new(p.x + aft.x * EXT_DIST, crate::worldmap::SEA_Y, p.y + aft.z * EXT_DIST),
        rotation: Quat::from_rotation_y(yaw + EXT_YAW),
        scale: Vec3::ONE,
    }
}

/// The same, rocking in the swell (the ship itself).
fn ext_xf(t: f32) -> Transform {
    let f = ext_flat();
    Transform {
        translation: f.translation + Vec3::Y * 0.3 * (TAU * t / 5.3).sin(),
        rotation: f.rotation * Quat::from_rotation_x(ext_pitch(t)) * Quat::from_rotation_z(ext_roll(t)),
        scale: Vec3::ONE,
    }
}

/// The exterior shots in the exterior ship's level frame: (eye, look-at, fov, aperture, focus point).
fn shot_ext(t: f32) -> (Vec3, Vec3, f32, f32, Option<Vec3>) {
    let lin = |t0: f32, t1: f32| ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    let ease = |u: f32| u * u * (3.0 - 2.0 * u);
    let v = Vec3::new;
    if t < EXT_FINAL {
        // The hymn rising through her hull: low off the stern quarter, the dark sail, the stern lantern, a slow drift in.
        let k = ease(lin(at::SHIP.0, at::SHIP.1));
        (v(9.5, 4.6, -19.0).lerp(v(6.5, 5.0, -15.0), k), v(0.0, 3.0, -4.5), 46.0 - 4.0 * k, 0.12, Some(v(0.0, 2.5, -5.0)))
    } else {
        // The pull-back, then a very slow orbit round her (the settlers' hymn carrying the last 14 s): a small lit
        // shape in the dark, the camera drawing away and circling from the port quarter round to her stern.
        let k = lin(EXT_FINAL, END);
        let kk = k * k * (3.0 - 2.0 * k);
        let (r, a, h) = (26.0 + 8.0 * kk, -2.14 + 0.6 * k, 4.0 + 4.0 * kk);
        (v(r * a.cos(), h, r * a.sin()), v(0.0, 4.2, 0.0), 40.0 + 4.0 * kk, 0.12, Some(v(0.0, 3.0, -4.0)))
    }
}

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
    server: &AssetServer,
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
    // Horn panes lit from inside: amber, half see-through, glowing well under the flame (a solid
    // emissive box blew out to a white slab in every shot the lantern hangs in), the flame inside.
    let glow = std_mats.add(StandardMaterial {
        base_color: Color::srgba(0.95, 0.62, 0.3, 0.55),
        emissive: LinearRgba::rgb(0.95, 0.42, 0.11),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.35,
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
                l.spawn((Mesh3d(meshes.add(Cuboid::new(0.118, 0.17, 0.118))), MeshMaterial3d(glow.clone()), local(0.0, -0.405, 0.0), bevy::light::NotShadowCaster));
            }
            l.spawn((
                Mesh3d(assets.flame_mesh.clone()),
                MeshMaterial3d(assets.flame_mat.clone()),
                Transform::from_xyz(0.0, -0.47, 0.0).with_scale(Vec3::splat(0.16)),
                bevy::light::NotShadowCaster,
                Flame { phase: 0.4 },
            ));
            l.spawn((
                PointLight {
                    color: Color::srgb(1.0, 0.64, 0.33),
                    intensity: 70_000.0,
                    range: 8.0,
                    radius: 0.04,
                    shadow_maps_enabled: true,
                    ..default()
                },
                local(0.0, -0.41, 0.0),
                FlameLight { base: 70_000.0, phase: 0.0 },
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
            PointLight { color: Color::srgb(1.0, 0.7, 0.4), intensity: 16_000.0, range: 3.0, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(candle + Vec3::Y * 0.05),
            FlameLight { base: 16_000.0, phase: 1.7 },
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
                intensity: 5_000.0,
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
            Lightning { base: 9_000_000.0 },
        ))
        .id();
    // The exterior ship, out to sea off the stern: the model, a stern lantern, a faint cold moon and a
    // bolt-light so a flash lights her sail and hull; its own rain round the camera's side.
    let rig = commands
        .spawn((ext_xf(0.0), Visibility::default(), ShipModel, SceneProp))
        .with_children(|r| {
            r.spawn((
                WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset("models/ship_caravel.glb"))),
                Transform::from_translation(MODEL_AT).with_scale(Vec3::splat(MODEL_SCALE)),
            ));
            r.spawn((
                SpotLight { color: Color::srgb(0.6, 0.7, 1.0), intensity: 9_000_000.0, range: 120.0, shadow_maps_enabled: true, outer_angle: 0.5, inner_angle: 0.2, ..default() },
                Transform::from_xyz(-16.0, 24.0, -34.0).looking_at(Vec3::new(0.0, 3.0, 0.0), Vec3::Y),
            ));
            r.spawn((
                SpotLight { color: Color::srgb(0.84, 0.88, 1.0), intensity: 0.0, range: 160.0, shadow_maps_enabled: true, outer_angle: 0.6, inner_angle: 0.3, ..default() },
                Transform::from_xyz(10.0, 26.0, -30.0).looking_at(Vec3::new(0.0, 4.0, 0.0), Vec3::Y),
                // (2.5e9 bleached the whole frame: a white ship on a turquoise sea)
                Lightning { base: 220_000_000.0 },
            ));
            r.spawn((
                PointLight { color: Color::srgb(1.0, 0.62, 0.3), intensity: 900_000.0, range: 14.0, radius: 0.2, shadow_maps_enabled: false, ..default() },
                Transform::from_xyz(0.0, 3.2, -9.0),
                FlameLight { base: 900_000.0, phase: 3.3 },
            ));
        })
        .id();
    // What the glass lets in, soft: models a face turned to the windows.
    let window_fill = commands
        .spawn((
            PointLight { color: Color::srgb(0.74, 0.8, 1.0), intensity: 2_500.0, range: 2.6, radius: 0.5, shadow_maps_enabled: false, ..default() },
            local(0.75, 1.2, HALF_D - 0.12),
        ))
        .id();
    // The moon through the middle stern window, low, onto Harrow's face: whenever he turns to the glass
    // ("the city", "bad knees") his face is cold-lit against the warm cabin, and each flash whitens it.
    // It hangs just inside the glass: from outside, the window's horizontal bar cut a black band across his
    // face in every close-up. Still shadowed, so the nose and brow model the face.
    let face_moon = commands
        .spawn((
            SpotLight {
                color: Color::srgb(0.62, 0.72, 1.0),
                intensity: 15_000.0,
                range: 6.0,
                radius: 0.15,
                shadow_maps_enabled: true,
                outer_angle: 0.42,
                inner_angle: 0.12,
                ..default()
            },
            local(0.55, 1.45, HALF_D - 0.2).looking_at(HARROW_HEAD + Vec3::new(0.0, -0.1, 0.0), Vec3::Y),
            FaceMoon { base: 15_000.0 },
        ))
        .id();
    commands.entity(ship).add_children(&[moon, lightning, window_fill, face_moon]);

    // Dust in the lamplight: specks drifting in a ball round the lantern, brightest near it (`drive`).
    let mote_mat = std_mats.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.78, 0.5, 0.0),
        emissive: LinearRgba::rgb(1.6, 1.0, 0.5),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    let speck = meshes.add(Sphere::new(1.0).mesh().ico(0).unwrap());
    for i in 0..MOTES {
        let e = commands
            .spawn((Mesh3d(speck.clone()), MeshMaterial3d(mote_mat.clone()), Transform::from_scale(Vec3::ZERO), bevy::light::NotShadowCaster, Mote(i)))
            .id();
        commands.entity(ship).add_child(e);
    }

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

    // The crew: silhouettes at the tiller and the rail of the exterior ship (stern castle y 2.8, main deck y 1.4
    // in the rig's frame), lit only by her lantern and the flashes.
    for (i, (at, yaw)) in [
        (Vec3::new(-0.7, 2.8, -6.6), 0.4),
        (Vec3::new(0.9, 2.8, -7.9), -2.2),
        (Vec3::new(-1.9, 1.4, -1.0), 1.2),
        (Vec3::new(1.6, 1.4, 1.2), -1.5),
        (Vec3::new(0.2, 1.4, 3.4), 3.0),
    ]
    .into_iter()
    .enumerate()
    {
        let c = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 51 + i as u32 * 7);
        commands.entity(c).insert((ns::SkinRequest(ns::GUARD), ns::SkinClip("Idle_Loop"), Crew { at, yaw }, SceneProp));
    }

    // Rain round the exterior camera: the same sheet blown up, standing in the exterior ship's frame.
    for i in 0..2 {
        let e = commands
            .spawn((
                Mesh3d(sheet.clone()),
                MeshMaterial3d(rain_mat.clone()),
                Transform::IDENTITY,
                bevy::light::NotShadowCaster,
                ExtRain { phase: i as f32 },
            ))
            .id();
        commands.entity(rig).add_child(e);
    }

    // The cast. Their own looks once they exist in the NPC cast; until then a guard (the tallest
    // man in the cast — "Harrow was taller") and the archer (a different face from the guard).
    let harrow = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 21);
    commands.entity(harrow).insert((
        ns::SkinRequest(look("harrow", ns::GUARD)),
        ns::HairShade(2), // dark, not the guard look's ginger: a man of fifty, not a lad
        ns::SkinClip("Sitting_Idle_Loop"),
        Role::Harrow,
        super::HeadLook::default(),
        SeatFit::default(),
    ));
    let wat = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 33);
    commands.entity(wat).insert((ns::SkinRequest(look("wat", ns::ARCHER)), ns::HairShade(1), ns::SkinClip("Fixing_Kneeling"), Role::Wat, super::HeadLook::default()));

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
pub(super) fn flash(t: f32) -> f32 {
    STRIKES
        .iter()
        .map(|&(t0, k)| k * env(t - t0, &[(-0.01, 0.0), (0.03, 1.0), (0.09, 0.15), (0.14, 0.9), (0.22, 0.3), (0.32, 0.6), (0.9, 0.0)]))
        .fold(0.0, f32::max)
}

/// Beats of the scene on its clock (s): where the dialogue and the business hang. Mirrors the
/// soundtrack's timeline (`tools/cutscene/cabin_mix.py`).
mod at {
    /// "Stand up." - Wat gets off his knee (the kneel clip's rise).
    pub const RISE: (f32, f32) = (24.5, 25.7);
    /// The ship shot over the hymn (Harrow's "Listen to them. Singing." ends 33.2).
    pub const SHIP: (f32, f32) = (33.17, 42.6);
    /// Wat to the table, his hand over the two envelopes ("Which one, Ser?"), the grab.
    pub const TO_TABLE: (f32, f32) = (77.4, 79.2);
    pub const REACH: (f32, f32) = (79.4, 80.5);
    pub const GRAB: (f32, f32) = (85.4, 86.6);
    pub const TO_DOOR: (f32, f32) = (87.5, 89.5);
    /// "Go on. Stop hovering." - the door opens, Wat goes out, Harrow is left with the lantern.
    pub const DOOR: f32 = 124.0;
    pub const OUT: (f32, f32) = (124.7, 127.9);
    /// The camera backs out through the stern window.
    pub const PULLBACK: (f32, f32) = (128.8, 130.0);
}

/// Harrow's beat: (clip, head yaw/pitch/roll; yaw + = his left, the stern windows). Down at the
/// strap; up for "Stand up"; eyes on the deck while the hymn comes up through it; the letter on the
/// table at his right; after Wat as he goes to the door; round to the windows for the city; back to
/// Wat at the door; and to the glass again for "Nobody looks at the man inside the iron".
fn harrow_beat(t: f32) -> (&'static str, Vec3) {
    // The long speeches get the talking loop; short lines stay on the idle.
    let talking = VO.iter().any(|&(s, _, _, d, r)| r == Role::Harrow && d > 4.5 && t >= s - 0.1 && t < s + d);
    let clip = if talking { "Sitting_Talking_Loop" } else { "Sitting_Idle_Loop" };
    let yaw = env(
        t,
        &[
            (0.0, 0.0), (23.6, 0.0),
            (30.0, -0.12), (44.8, -0.12), (45.4, 0.0), (52.6, 0.0), // the hymn at the deck; Wat; Kell
            (53.6, 1.05), (64.8, 1.05), (65.6, 0.0),                // the Crown's rock: the glass; then the farmers
            (71.2, -1.0), (76.6, -1.0), (77.6, -0.3), (79.2, -0.9), // the letter on the table, Wat crossing to it
            (86.4, -0.9), (88.6, -0.4), (89.8, -0.75), (104.0, -0.75), // "the one without the royal wax" ... Wat at the door
            (104.9, -0.2), (113.4, -0.2), (114.8, 1.05), (127.0, 1.05), // "bad knees" -> the glass, thunder
        ],
    );
    let pitch = env(
        t,
        &[
            (0.0, -0.1), (7.4, -0.1), (8.0, -0.42), (23.4, -0.42), (24.6, -0.08), (29.6, -0.08), (30.6, -0.3), (42.4, -0.3),
            (45.0, -0.1), (52.6, -0.1), (53.6, 0.06), (64.8, 0.06), (66.0, -0.1), (71.0, -0.12), (71.8, -0.3), (77.4, -0.3), (78.0, -0.05),
            (82.0, -0.12), (89.0, -0.05), (104.0, -0.05), (104.9, 0.0), (113.4, 0.0), (114.8, 0.06), (127.0, 0.06),
        ],
    );
    (clip, Vec3::new(yaw, pitch, 0.0))
}

/// Wat's blocking at `t`: (floor spot, facing yaw, clip, scrubbed playhead). Kneeling at the greave
/// until the strap is home ("Hnn. There."); up (the rest of the kneel clip) on "Stand up"; stood at
/// his master's side through the hymn and the talk; round the lantern to the table when sent for the
/// letter, his hand hovering over the two envelopes; the right one lifted; to the door, where the
/// flash stops him; turned back to his master to answer; turned away and out through the door after
/// "Stop hovering."
fn wat_track(t: f32) -> (Vec2, f32, &'static str, Option<f32>) {
    // `Fixing_Kneeling` kneels down, works and stands back up in one 4.1 s cycle; down on one knee
    // 0.75-3.15 s. Held there (eased back and forth so the hands keep working), then let up.
    const DOWN: (f32, f32) = (0.75, 3.15);
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
    // The PickUp_Table playhead: reach, hover over the envelopes, then the grab.
    let pick = {
        let reach = 0.28 * lin(at::REACH.0, at::REACH.1);
        let hover = 0.012 * (t * 5.0).sin() * (1.0 - lin(at::GRAB.0 - 0.3, at::GRAB.0));
        reach + hover + 0.39 * lin(at::GRAB.0, at::GRAB.1)
    };
    match t {
        t if t < at::RISE.0 => (WAT_AT, WAT_YAW, "Fixing_Kneeling", Some(DOWN.0 + span * (0.5 - 0.5 * (PI * t * 0.8 / span).cos()))),
        t if t < at::RISE.1 => (WAT_AT, WAT_YAW, "Fixing_Kneeling", Some(DOWN.1 + (4.1 - DOWN.1) * lin(at::RISE.0, at::RISE.1))),
        t if t < at::TO_TABLE.0 => (WAT_AT, WAT_YAW, "Idle_Loop", None),
        t if t < at::TO_TABLE.1 => {
            let (p, yaw) = walk(&[WAT_AT, PAST_KNEES, TABLE_SPOT], at::TO_TABLE.0, at::TO_TABLE.1);
            (p, WAT_YAW + (yaw - WAT_YAW) * smooth(lin(at::TO_TABLE.0, at::TO_TABLE.0 + 0.5)), "Walk_Loop", None)
        }
        t if t < at::TO_DOOR.0 => {
            let a = TABLE_SPOT - PAST_KNEES;
            let came = a.x.atan2(a.y);
            (TABLE_SPOT, came + (WAT_YAW - came) * smooth(lin(at::TO_TABLE.1, at::TO_TABLE.1 + 0.35)), "PickUp_Table", Some(pick))
        }
        t if t < at::TO_DOOR.1 + 0.1 => {
            let (p, yaw) = walk(&[TABLE_SPOT, DOOR_SPOT], at::TO_DOOR.0, at::TO_DOOR.1);
            (p, yaw, "Walk_Loop", None)
        }
        // At the door: his back to the room as the flash stops him, then round to his master to
        // answer ("She asked why..."), and turned away again after "Stop hovering."
        t if t < at::DOOR - 0.6 => {
            let a = DOOR_SPOT - TABLE_SPOT;
            let came = a.x.atan2(a.y).rem_euclid(TAU);
            let yaw = if t < 91.6 { came + (PI - came) * smooth(lin(at::TO_DOOR.1, at::TO_DOOR.1 + 0.8)) } else { PI + (FACE_HARROW - PI) * smooth(lin(91.6, 92.6)) };
            (DOOR_SPOT, yaw, "Idle_Loop", None)
        }
        t if t < at::OUT.0 => (DOOR_SPOT, FACE_HARROW + (PI - FACE_HARROW) * smooth(lin(at::DOOR - 0.6, at::DOOR + 0.3)), "Idle_Loop", None),
        _ => {
            let (p, yaw) = walk(&[DOOR_SPOT, Vec2::new(DOOR_X, -HALF_D), Vec2::new(DOOR_X, -HALF_D - 1.55)], at::OUT.0, at::OUT.1);
            (p, yaw, if t < at::OUT.1 { "Walk_Loop" } else { "Idle_Loop" }, None)
        }
    }
}

fn smooth(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

/// Wat's head (body frame): on the work while he kneels; dipped, listening, at the door; on his
/// master once he turns (looking down at a seated man), eyes dropping after "They just want to know
/// the iron is there."
fn wat_head(t: f32) -> Vec3 {
    let pitch = env(
        t,
        // ("Rust from the salt, Ser.": up from the strap to his master's face for the line, then back to it)
        &[(18.9, 0.0), (19.5, 0.85), (21.4, 0.85), (22.3, 0.0), (26.0, 0.0), (30.0, -0.12), (44.0, -0.12), (45.0, 0.0), (78.0, 0.0), (79.0, -0.3), (86.8, -0.3), (87.6, 0.0), (89.4, 0.0), (90.0, -0.18), (104.0, -0.2), (119.0, -0.2), (120.0, -0.42), (124.0, -0.42), (124.7, 0.0)],
    );
    Vec3::new(0.0, pitch, 0.0)
}

/// The door's swing (radians, out into the passage) as Wat goes.
fn door_open(t: f32) -> f32 {
    1.45 * smooth(((t - at::DOOR) / 1.1).clamp(0.0, 1.0))
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
    mut bolt: Query<(&mut SpotLight, &Lightning), Without<FaceMoon>>,
    (mut face_moon, mut motes): (
        Query<(&mut SpotLight, &FaceMoon), Without<Lightning>>,
        Query<(&mut Transform, &Mote), (Without<Ship>, Without<Role>, Without<Lantern>, Without<Rain>, Without<Flame>, Without<Door>, Without<ShipModel>, Without<ExtRain>)>,
    ),
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut fade: Query<&mut BackgroundColor, With<super::FarmFade>>,
    (mut title, mut rig, mut ext_rain): (
        Query<&mut TextColor, With<Title>>,
        Query<&mut Transform, (With<ShipModel>, Without<Ship>, Without<Role>, Without<Lantern>, Without<Rain>, Without<Flame>, Without<Door>, Without<ExtRain>)>,
        Query<(&mut Transform, &ExtRain), (Without<ShipModel>, Without<Ship>, Without<Role>, Without<Lantern>, Without<Rain>, Without<Flame>, Without<Door>)>,
    ),
) {
    if state.active != Some(SceneId::ShipCabin) {
        return;
    }
    let t = state.clock.max(0.0);
    for mut tf in &mut rig {
        *tf = ext_xf(t);
    }
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
    for (mut s, m) in &mut face_moon {
        s.intensity = m.base * (1.0 + 6.0 * fl);
    }
    for (mut tf, m) in &mut motes {
        let (p, k) = mote(m.0, t);
        tf.translation = p;
        tf.scale = Vec3::splat(0.0016 * k.sqrt());
    }
    // Rain: 5.2 u sheets falling 7 u/s, two stacked so the column never empties.
    let drop = (t * 7.0).rem_euclid(RAIN_H);
    for (mut tf, r) in &mut rain {
        tf.translation = Vec3::new(0.0, RAIN_Y0 - drop + r.phase * RAIN_H, 0.0);
        if let Some(mut m) = mats.get_mut(&r.mat) {
            m.base_color = Color::srgba(0.72 + 0.28 * fl, 0.78 + 0.22 * fl, 0.88 + 0.12 * fl, 0.2 + 0.5 * fl);
        }
    }

    for (mut tf, r) in &mut ext_rain {
        // 6x the sheet, standing on the camera's side of the ship: it falls at the same pace.
        tf.translation = Vec3::new(0.0, RAIN_Y0 * 6.0 - drop * 6.0 + r.phase * RAIN_H * 6.0 + 8.0, -18.0);
        tf.scale = Vec3::new(6.0, 6.0, 6.0);
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
    // The grab: where the lift's playhead passes 0.40 (see `wat_track`).
    const TAKEN: f32 = at::GRAB.0 + (0.12 / 0.39) * (at::GRAB.1 - at::GRAB.0);
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

/// The exterior ship's crew stand on her deck as she rocks.
pub(super) fn crew_follow(state: Res<SceneState>, mut crew: Query<(&Crew, &mut Transform, &mut crate::villagers::Villager)>) {
    if state.active != Some(SceneId::ShipCabin) {
        return;
    }
    let xf = ext_xf(state.clock.max(0.0));
    let (_, ship_yaw, _) = xf.rotation.to_euler(EulerRot::YXZ);
    for (c, mut tf, mut v) in &mut crew {
        let w = xf.transform_point(c.at);
        v.pos = Vec2::new(w.x, w.z);
        v.facing = ship_yaw + c.yaw;
        v.moving = false;
        tf.translation = w;
        tf.rotation = xf.rotation * Quat::from_rotation_y(c.yaw);
    }
}

/// Look-dev: `FOREST_CABIN_LOG=1` logs each actor's head joint in the ship frame (position + the
/// joint's axes), for framing shots on where the faces really are.
pub(super) fn log_heads(state: Res<SceneState>, actors: Query<(&Role, &super::HeadLook)>, globals: Query<&GlobalTransform>) {
    if state.active != Some(SceneId::ShipCabin) || std::env::var_os("FOREST_CABIN_LOG").is_none() {
        return;
    }
    let t = state.clock.max(0.0);
    let inv = ship_xf(t).compute_affine().inverse();
    for (role, hl) in &actors {
        let Some(g) = hl.bone.and_then(|b| globals.get(b).ok()) else { continue };
        let m = inv * g.affine();
        info!("HEAD t={t:.2} {role:?} pos {:.3?} x {:.2?} y {:.2?} z {:.2?}", m.translation, m.matrix3.x_axis.normalize(), m.matrix3.y_axis.normalize(), m.matrix3.z_axis.normalize());
    }
}

/// Harrow's and Wat's faces say their lines: each actor plays the mouth-shape track of the clip he is
/// speaking, at the scene clock (see `lipsync`); a clip without a track opens the jaw to its loudness.
pub(super) fn mouths(
    mut commands: Commands,
    state: Res<SceneState>,
    server: Res<AssetServer>,
    sources: Res<Assets<AudioSource>>,
    tracks: Res<crate::lipsync::Tracks>,
    mut clips: Local<Vec<(Handle<AudioSource>, Option<Vec<f32>>)>>,
    actors: Query<(Entity, &Role)>,
) {
    if state.active != Some(SceneId::ShipCabin) {
        return;
    }
    if clips.is_empty() {
        *clips = VO.iter().map(|vo| (server.load(vo.1), None)).collect();
    }
    for ((h, env), vo) in clips.iter_mut().zip(VO.iter()) {
        if env.is_none() && tracks.get(vo.1).is_none() {
            *env = sources.get(&*h).map(|s| crate::lipsync::envelope(s, vo.2, vo.3));
        }
    }
    let t = state.clock.max(0.0);
    for (e, role) in &actors {
        let line = VO.iter().find(|vo| vo.4 == *role && t >= vo.0 && t < vo.0 + (vo.3 - vo.2));
        let speech = line.filter(|vo| tracks.get(vo.1).is_some()).map(|vo| (vo.1, t - vo.0 + vo.2));
        // no track: the jaw by loudness (smooth ElevenLabs speech barely breathes, so any voiced frame
        // opens it a good way, loud syllables all the way)
        let open = clips
            .iter()
            .zip(VO.iter())
            .filter(|(_, vo)| vo.4 == *role)
            .filter_map(|((_, env), vo)| env.as_deref().map(|env| crate::lipsync::sample(env, t - vo.0)))
            .fold(0.0, f32::max);
        let open = if open > 0.015 { 0.45 + 0.95 * (open * 2.5).min(1.0) } else { 0.0 };
        commands.entity(e).try_insert((crate::lipsync::Speech(speech), crate::lipsync::MouthOpen(open)));
        if std::env::var_os("FOREST_MOUTHLOG").is_some() {
            info!("MOUTH t={t:.2} {role:?} speech={speech:?} open={open:.2}");
        }
    }
}

// ── Coverage ────────────────────────────────────────────────────────────────────────────

/// The shot at `t` in the ship's frame: (eye, look-at, vertical fov deg, thin-lens aperture, focus
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
    // Wat stood at his master's side (his head), for the reaction close-ups.
    let wat_stand = v(WAT_AT.x, WAT_STAND_H, WAT_AT.y);
    let (eye, at, fov, band, focus_on) = match t {
        // -- I. The strap --
        // A: the cabin from the door: the lantern swinging over them, the squire at his master's
        //    leg, the stern windows behind them full of rain. Nobody speaks.
        t if t < 8.9 => (v(-1.05, 1.4, -1.45).lerp(v(-0.82, 1.32, -1.2), ease(lin(4.6, 8.9))), v(0.6, 0.72, 0.7), 58.0, 0.12, Some(HARROW_HEAD)),
        // B: "Tighter." - low over Wat's shoulder, up at Harrow (the squire's eye).
        t if t < 10.5 => (v(-0.25, 0.76, 0.6), HARROW_HEAD, 36.0, 0.9, Some(HARROW_HEAD)),
        // B2: "Left buckle's always stiff. Fifteen years you've strapped me into this iron..." -
        //     pushing in on him; the shield on the hull behind his head.
        t if t < 16.2 => {
            let k = ease(lin(10.5, 16.2));
            (v(0.0, 0.86, 1.24).lerp(v(0.2, 0.88, 1.12), k), HARROW_HEAD + v(0.15, 0.0, 0.12), 34.0 - 4.0 * k, 1.0, Some(HARROW_HEAD))
        }
        // C: the hands at the strap, hauling on it.
        t if t < 19.2 => (v(0.62, 0.5, 1.14).lerp(v(0.6, 0.49, 1.13), lin(16.2, 19.2)), STRAP + v(0.0, 0.03, 0.0), 34.0, 1.0, Some(STRAP)),
        // C2: "Rust from the salt, Ser." - Wat's face, low, eyes on the work.
        //     (from the lantern's side, where his raised face catches its light)
        t if t < 21.8 => {
            let face = v(0.46, 0.64, 0.83);
            (v(0.8, 0.67, 0.44).lerp(v(0.76, 0.66, 0.48), lin(19.2, 21.8)), face, 32.0, 0.9, Some(face))
        }
        // H1: "Hnn. There." - the buckle home.
        t if t < 23.6 => (v(0.6, 0.48, 1.13), STRAP + v(0.0, 0.03, 0.0), 32.0, 1.0, Some(STRAP)),
        // -- II. The hymn --
        // WR: "Stand up. You're making my knees ache just looking at you." - wide from the stern
        //     corner: Wat comes up off his knee.
        t if t < 27.0 => (v(-1.45, 1.45, 1.3), v(0.3, 0.8, 0.7), 50.0, 0.3, Some(wat)),
        // D2: "Listen to them. Singing." - Harrow from the door end, eyes on the deck, the hymn
        //     coming up through it; pushing in.
        t if t < at::SHIP.0 => {
            let k = ease(lin(27.0, at::SHIP.0));
            (v(0.0, 1.25, -1.25).lerp(v(0.1, 1.22, -1.05), k), HARROW_HEAD, 30.0 - 5.0 * k, 0.8, Some(HARROW_HEAD))
        }
        // SHIP: the ship from astern in the storm - the lit stern windows, the rain, the sea heaving,
        //       a flash on the glass; the settlers' hymn rising through the hull. A slow drift in.
        t if t < at::SHIP.1 => {
            let k = ease(lin(at::SHIP.0, at::SHIP.1));
            (v(9.0, 0.9, 15.0).lerp(v(6.2, 1.3, 11.0), k), v(0.2, 1.2, -1.0), 48.0 - 4.0 * k, 0.9, Some(v(0.1, 0.9, 1.7)))
        }
        // -- III. Kell --
        // G2: "Wait till they see the island." - from the door end, Harrow listening.
        t if t < 45.0 => (v(-1.48, 1.45, -1.29), HARROW_HEAD, 36.0, 0.8, Some(HARROW_HEAD)),
        // Wat over his master's shoulder: "Have you seen it, Ser? Kell?"
        t if t < 46.9 => (v(1.15, 1.28, 0.2), wat_stand, 30.0, 0.8, Some(wat_stand)),
        // G1: "Nobody's seen Kell in a hundred years, Wat. That's why the King's giving it to me."
        t if t < 52.6 => (v(-0.3, 1.12, -1.0), HARROW_HEAD, 26.0, 0.8, Some(HARROW_HEAD)),
        // E: "You take one city too roughly in the east..." - his face from the glass.
        t if t < 61.6 => {
            let k = ease(lin(52.6, 61.6));
            (v(0.42, 1.12, 1.46).lerp(v(0.48, 1.11, 1.38), k), HARROW_HEAD, 36.0 - 6.0 * k, 0.6, Some(HARROW_HEAD))
        }
        // "They made you Lord Commissioner, Ser."
        t if t < 65.0 => (v(1.15, 1.28, 0.2), wat_stand, 27.0, 0.8, Some(wat_stand)),
        // G2: "Over forty seasick turnip-farmers and a village Reeve. A glorious command."
        t if t < 71.7 => (v(-1.48, 1.45, -1.29).lerp(v(-1.42, 1.44, -1.24), lin(65.0, 71.7)), HARROW_HEAD, 36.0, 0.8, Some(HARROW_HEAD)),
        // D: "Take the parchment to the woman downstairs." - Harrow from his right, his eyes going to the
        //    table: the helmet, the two envelopes.
        t if t < 77.4 => (v(0.2, 0.92, -0.55).lerp(v(0.24, 0.93, -0.48), lin(71.7, 77.4)), v(0.92, 0.86, 0.45), 40.0, 0.8, Some(HARROW_HEAD)),
        // W1: from the stern corner: Wat round the lantern to the table.
        t if t < 79.4 => (v(-1.45, 1.45, 1.3), wat.lerp(v(0.75, 0.82, -0.25), 0.4), 50.0, 0.3, Some(wat)),
        // His hand over the two envelopes: "Which one, Ser?"
        t if t < 82.0 => {
            let k = ease(lin(79.4, 82.0));
            (v(0.95, 1.55, -0.1).lerp(v(1.0, 1.38, -0.2), k), v(1.08, 0.62, -0.42), 36.0 - 5.0 * k, 1.0, Some(v(1.08, 0.62, -0.42)))
        }
        // "The one without the royal wax, Wat. I keep forgetting I didn't hire you for your reading."
        t if t < 85.0 => (v(-0.3, 1.12, -1.0), HARROW_HEAD, 24.0, 0.8, Some(HARROW_HEAD)),
        // The letter lifted and Wat away with it, to the door - the flash stops him there.
        t if t < 92.6 => (v(-0.6, 1.35, 1.35), wat.lerp(v(-0.9, 1.2, -1.4), 0.6), 44.0, 0.45, Some(wat)),
        // F: "She asked why the great Ser Godric Harrow..." - Wat turned back at the door, past his
        //    master's shoulder.
        t if t < 97.9 => (v(1.05, 1.12, 0.3), WAT_DOOR_HEAD, 30.0, 0.8, Some(WAT_DOOR_HEAD)),
        // "And what did my loyal squire tell her?"
        t if t < 100.9 => (v(-0.3, 1.12, -1.0), HARROW_HEAD, 26.0, 0.8, Some(HARROW_HEAD)),
        // "That you were resting after the campaign, Ser." - Wat, closer.
        t if t < 104.4 => (v(1.0, 1.14, 0.45), WAT_DOOR_HEAD, 25.0, 0.8, Some(WAT_DOOR_HEAD)),
        // E2: "Good man. Let them keep singing... an old butcher with bad knees." - he turns to the glass
        //     (and the big flash); a slow push on his face.
        t if t < 115.4 => {
            let k = ease(lin(104.4, 115.4));
            (v(0.42, 1.12, 1.46).lerp(v(0.5, 1.1, 1.3), k), HARROW_HEAD, 34.0 - 8.0 * k, 0.6, Some(HARROW_HEAD))
        }
        // G2: "Nobody looks at the man inside the iron, Wat." - over Wat's right shoulder.
        t if t < 119.4 => (v(-1.48, 1.45, -1.29).lerp(v(-1.42, 1.44, -1.24), lin(115.4, 119.4)), HARROW_HEAD, 36.0, 0.8, Some(HARROW_HEAD)),
        // H: "They just want to know the iron is there." - Wat, a slow push; his eyes go down.
        t if t < 122.2 => {
            let face = WAT_DOOR_HEAD - v(0.0, 0.08, 0.0);
            (v(-0.25, 1.12, -0.45).lerp(v(-0.4, 1.15, -0.6), ease(lin(119.4, 122.2))), face, 32.0, 0.9, Some(face))
        }
        // I: "Go on. Stop hovering." - Harrow back at the window.
        t if t < 124.4 => (v(0.52, 1.1, 1.32), HARROW_HEAD, 30.0, 0.7, Some(HARROW_HEAD)),
        // J: Wat goes: from behind him, down the passage's length - the door, the hold's lamplight
        //    at the far end, his back against it.
        t if t < at::PULLBACK.0 => (v(-0.72, 1.45, 0.25), v(-1.0, 1.05, -3.0), 40.0, 0.4, Some(wat)),
        // K: Harrow alone with the lantern; the camera backs out through the middle stern window into
        //    the storm (and cuts to the ship from outside, `shot_ext`) - the hymn swelling.
        _ => {
            let k = lin(at::PULLBACK.0, at::PULLBACK.1);
            let kk = k * k * (3.0 - 2.0 * k);
            let eye = v(0.05 + 0.1 * kk, 1.15, 1.2 + 1.2 * kk);
            (eye, v(0.35, 0.9, -1.0).lerp(v(0.0, 1.2, -3.0), kk), 40.0 + 8.0 * kk, 0.3, Some(HARROW_HEAD))
        }
    };
    (eye, at, fov, band, focus_on.map(|p| eye.distance(p)))
}

/// The shot in the world: ((eye, look-at, fov, aperture), focus) + the ship's up for `look_at`.
pub(super) fn camera(t: f32) -> ((super::Cam, Option<f32>), Vec3) {
    if ext_shot(t) {
        let f = ext_flat();
        let (eye, at, fov, band, focus) = shot_ext(t);
        let (e, a) = (f.transform_point(eye), f.transform_point(at));
        return (((e, a, fov, band), focus.map(|p| e.distance(f.transform_point(p)))), Vec3::Y);
    }
    let xf = ship_xf(t);
    let (mut eye, mut at, mut fov, band, focus) = shot_local(t);
    // Look-dev: `FOREST_CABIN_CAM="ex,ey,ez,lx,ly,lz,fov"` (ship frame) replaces the interior shot.
    if let Some(v) = std::env::var("FOREST_CABIN_CAM").ok().and_then(|c| c.split(',').map(|n| n.trim().parse::<f32>().ok()).collect::<Option<Vec<_>>>()).filter(|v| v.len() == 7) {
        (eye, at, fov) = (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]), v[6]);
    }
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
    // The roof's taffrail (the hull below it is the exterior ship model, `ShipModel`).
    p.push(cube(2.0 * HALF_W + 0.5, 0.05, 0.05, Vec3::new(0.0, BEAMS + 0.55, HALF_D + 0.17), 0x2a1c10)); // taffrail
    for k in 0..9u32 {
        let x = -2.0 + k as f32 * 0.5;
        p.push(cube(0.05, 0.36, 0.05, Vec3::new(x, BEAMS + 0.38, HALF_D + 0.17), 0x2a1c10));
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
        cube_r(0.24, 0.014, 0.16, Vec3::new(-0.22, 0.012, -0.2), Quat::from_rotation_y(0.5), 0xe9e1cb), // the other letter, under the Crown's red wax
        cube_r(0.242, 0.004, 0.02, Vec3::new(-0.22, 0.021, -0.2), Quat::from_rotation_y(0.5), 0xb8923a),
        cylr(0.034, 0.01, Vec3::new(-0.22, 0.024, -0.2), Quat::IDENTITY, 0x9a1612),
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
    vec![
        cube(0.22, 0.014, 0.14, Vec3::ZERO, 0xd9cfb4),
        cube(0.23, 0.003, 0.011, Vec3::new(0.0, 0.009, 0.0), 0x6b5a3c), // the string it is tied with
        cylr(0.02, 0.006, Vec3::new(0.0, 0.011, 0.0), Quat::IDENTITY, 0x4f4d4a), // a plain grey seal: no royal wax
    ]
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
