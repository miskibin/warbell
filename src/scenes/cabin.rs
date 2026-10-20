//! Screenplay **P.0 "Na pokładzie" / Below Deck** — the game's first scene, the T2 cutscene that
//! opens the prologue (`docs/superpowers/specs/2026-10-13-kampania-scenariusz-design.md` §P.0). A
//! VIDEO rig like P.3 (`FOREST_SCENE=cabin`, filmed under `FOREST_CLIP`, soundtrack mixed offline by
//! `tools/cutscene/cabin_audio.py`) — no in-game trigger, skip or save hook.
//!
//! Black, a storm on the hull, the card *KELL — three days out*. The stern cabin, a lantern swinging:
//! Ser Godric Harrow seated in his harness, helmet on the table, shield on the hull behind him; Wat,
//! his squire, kneeling at his leg doing up a greave strap. Harrow's four lines and Wat's two
//! ("Nobody looks at the man behind the shield, Wat. That's the point of you."), lightning on
//! "Before it burned?", the strap cinched, fade out.
//!
//! The set is moored on the open sea off Wreck Cove, stern window facing away from the island, and
//! rocks as one rigid frame ([`ship_xf`]): set, actors, lights and camera all ride it, so the cabin
//! holds still in frame while the sea through the window, the lantern and the rain move.
//!
//! **Asset slots** (`docs/superpowers/specs/2026-10-20-p0-below-deck-cutscene.md`): every piece that
//! should be a real asset is looked up first and stands in with a blockout only while it's missing:
//! kit models `cs_cabin` / `cs_cabin_table` / `cs_cabin_chair` / `cs_cabin_hammock` /
//! `cs_cabin_lantern`, the NPC looks `harrow` / `wat` (stand-ins: guard / villager), the music
//! `below_deck.ogg` and the cabin SFX in `assets/cutscene/ship_cabin.cues` (missing files are skipped).

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use super::{cube, cylr, env, merged, SceneAssets, SceneId, SceneProp, SceneState, IRON, WOOD, WOOD_DARK};

/// Deep night, the storm's.
pub(super) const SKY: f32 = 0.82;
pub(super) const END: f32 = 49.5;
/// Title card on black, then the picture fades up.
const TITLE: (f32, f32, f32, f32) = (0.9, 1.9, 3.5, 4.4);
const FADE_IN: (f32, f32) = (4.6, 6.4);
const FADE_OUT: (f32, f32) = (46.6, 48.4);

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
const WAT_AT: Vec2 = Vec2::new(0.12, 0.86);
const WAT_YAW: f32 = FRAC_PI_2;
/// The lantern's hook on the beam between them.
const LANTERN_HOOK: Vec3 = Vec3::new(0.45, BEAMS, 0.3);

/// Look-at marks for the coverage (approximate: head heights of the seated / kneeling bodies).
const HARROW_HEAD: Vec3 = Vec3::new(0.95, 1.17, 0.75);
const WAT_HEAD: Vec3 = Vec3::new(0.3, 0.78, 0.86);
const STRAP: Vec3 = Vec3::new(0.55, 0.22, 0.88);

// ── Lines ───────────────────────────────────────────────────────────────────────────────

/// Captions: (start s, duration s, speaker, line) — verbatim from the recorded clips, split at
/// their pauses (silencedetect on the clips: `a1_harrow_01` "Tighter." 0–0.81 | 2.47–7.6, `_02`
/// 0–2.52 | 3.13–7.2, `_03` 0.35–2.80 | 3.30–4.43, `_04` 0–1.41 | 3.02–5.89 | 6.50–7.84).
const LINES: [(f32, f32, &str, &str); 10] = [
    (7.6, 1.0, "Harrow", "Tighter."),
    (10.07, 5.3, "Harrow", "You've done that strap every day for fifteen years and it's still wrong."),
    (16.0, 1.0, "Wat", "Yes, Ser."),
    (17.6, 2.7, "Harrow", "Forty peasants and a Reeve."),
    (20.73, 4.2, "Harrow", "That's what the Crown sends a man after the eastern campaign."),
    (26.15, 2.75, "Harrow", "Do you know what they said about me in the city?"),
    (29.1, 1.5, "Harrow", "Before it burned?"),
    (31.4, 1.0, "Wat", "No, Ser."),
    (33.4, 1.6, "Harrow", "Neither do I."),
    (36.42, 5.0, "Harrow", "Nobody looks at the man behind the shield, Wat. That's the point of you."),
];

/// The VO as `cabin_audio.py` lays it in: (start s, clip, speaker). Read here only for the mouths.
pub(super) const VO: [(f32, &str, Role); 6] = [
    (7.6, "audio/vo/campaign/a1_harrow_01.ogg", Role::Harrow), // "Tighter. You've done that strap every day for fifteen years and it's still wrong."
    (16.0, "audio/vo/campaign/a1_wat_01.ogg", Role::Wat),      // "Yes, Ser."
    (17.6, "audio/vo/campaign/a1_harrow_02.ogg", Role::Harrow), // "Forty peasants and a Reeve. That's what the Crown sends a man after the eastern campaign."
    (25.8, "audio/vo/campaign/a1_harrow_03.ogg", Role::Harrow), // "Do you know what they said about me in the city? Before it burned?"
    (31.4, "audio/vo/campaign/a1_wat_02.ogg", Role::Wat),      // "No, Ser."
    (33.4, "audio/vo/campaign/a1_harrow_04.ogg", Role::Harrow), // "Neither do I. Nobody looks at the man behind the shield, Wat. That's the point of you."
];

/// Lightning strikes outside the stern: (time, strength). "Before it burned?" gets the big one.
const STRIKES: [(f32, f32); 4] = [(5.4, 0.55), (18.6, 0.22), (29.25, 1.0), (45.3, 0.75)];

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

/// A kit model if the kit has it, else the blockout — parented to the ship frame.
#[allow(clippy::too_many_arguments)]
fn slot(commands: &mut Commands, meshes: &mut Assets<Mesh>, assets: &SceneAssets, ship: Entity, kit: &str, tf: Transform, blockout: fn() -> Mesh) {
    let e = match crate::realforest::get().and_then(|k| k.spawn_static(commands, kit, tf)) {
        Some(e) => e,
        None => commands.spawn((Mesh3d(meshes.add(blockout())), MeshMaterial3d(assets.prop_mat.clone()), tf)).id(),
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
    slot(commands, meshes, assets, ship, "cs_cabin", Transform::IDENTITY, shell_mesh);
    slot(commands, meshes, assets, ship, "cs_cabin_table", local(TABLE.x, 0.0, TABLE.y), table_mesh);
    slot(
        commands, meshes, assets, ship, "cs_cabin_chair",
        local(CHAIR.x, 0.0, CHAIR.y).with_rotation(Quat::from_rotation_y(CHAIR_YAW)),
        chair_mesh,
    );
    slot(commands, meshes, assets, ship, "cs_cabin_hammock", Transform::IDENTITY, hammock_mesh);
    // Sea chest under the port hull; casks and a crate in the forward-starboard corner.
    let chest_tf = local(-1.72, 0.0, 0.25).with_rotation(Quat::from_rotation_y(FRAC_PI_2));
    if has_kit("chest_wood_body") {
        for part in ["chest_wood_body", "chest_wood_lid"] {
            if let Some(e) = crate::realforest::get().and_then(|k| k.spawn_static(commands, part, chest_tf)) {
                commands.entity(ship).add_child(e);
            }
        }
    } else {
        slot(commands, meshes, assets, ship, "-", chest_tf, sea_chest_mesh);
    }
    slot(commands, meshes, assets, ship, "wooden_crate_01", local(1.62, 0.0, -1.25).with_scale(Vec3::splat(0.55)), crate_mesh);
    slot(commands, meshes, assets, ship, "-", local(1.05, 0.0, -1.38), casks_mesh);
    slot(commands, meshes, assets, ship, "-", local(TABLE.x, TABLE_TOP, TABLE.y), table_dressing_mesh);

    // Harrow's own kit: the helmet on the table, the shield hung on the hull behind his chair.
    if let Some(helm) = crate::player::spawn_part_prop(commands, meshes, images, creature_mats, "Head") {
        commands.entity(helm).insert(local(TABLE.x - 0.08, TABLE_TOP - 0.02, TABLE.y + 0.18).with_rotation(Quat::from_rotation_y(-2.2)));
        commands.entity(ship).add_child(helm);
    }
    if let Some(shield) = crate::player::spawn_part_prop(commands, meshes, images, creature_mats, "Shield") {
        commands.entity(shield).insert(
            local(HALF_W - 0.12, 0.62, CHAIR.y + 0.1).with_rotation(Quat::from_rotation_y(-FRAC_PI_2) * Quat::from_rotation_x(0.12)),
        );
        commands.entity(ship).add_child(shield);
    }

    // The lantern: a pivot on the hook, swung by `drive` against the ship's roll.
    let glow = std_mats.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.62, 0.3),
        emissive: LinearRgba::rgb(6.0, 3.1, 1.0),
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
                l.spawn((Mesh3d(meshes.add(lantern_mesh())), MeshMaterial3d(assets.prop_mat.clone()), Transform::IDENTITY, bevy::light::NotShadowCaster));
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

    // Night outside: a cold moon fill and the lightning, both through the stern windows.
    let moon = commands
        .spawn((
            SpotLight {
                color: Color::srgb(0.6, 0.7, 1.0),
                intensity: 60_000.0,
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
    commands.entity(ship).add_children(&[moon, lightning]);

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
    // man in the cast — "Harrow was taller") and a plain villager.
    let harrow = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 21);
    commands.entity(harrow).insert((
        ns::SkinRequest(look("harrow", ns::GUARD)),
        ns::SkinClip("Sitting_Idle_Loop"),
        Role::Harrow,
        super::HeadLook::default(),
        SeatFit::default(),
    ));
    let wat = crate::villagers::spawn_scene_peasant(commands, meshes, creature_mats, Vec2::ZERO, 0.0, None, 33);
    commands.entity(wat).insert((ns::SkinRequest(look("wat", ns::VILLAGER_M)), ns::SkinClip("Fixing_Kneeling"), Role::Wat, super::HeadLook::default()));

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

/// Harrow's beat: (clip, head yaw/pitch/roll). Down at the strap; off into the middle distance for
/// the Crown; round to the windows for the city; back down to his squire for the last line.
fn harrow_beat(t: f32) -> (&'static str, Vec3) {
    let clip = if (17.6..24.9).contains(&t) { "Sitting_Talking_Loop" } else { "Sitting_Idle_Loop" };
    let yaw = env(t, &[(17.2, 0.0), (18.0, 0.2), (24.9, 0.25), (25.6, 1.05), (35.9, 1.05), (36.6, 0.05)]);
    let pitch = env(t, &[(7.4, -0.32), (8.0, -0.42), (16.8, -0.42), (17.6, -0.05), (24.9, -0.05), (25.6, 0.06), (35.9, 0.06), (36.6, -0.38)]);
    (clip, Vec3::new(yaw, pitch, 0.0))
}

/// Wat never stops working; he looks up twice — "No, Ser." and the last line.
fn wat_head(t: f32) -> Vec3 {
    let up = env(t, &[(30.7, 0.0), (31.2, 0.5), (32.6, 0.5), (33.2, 0.1), (36.4, 0.1), (36.9, 0.45), (41.6, 0.45), (42.2, 0.0)]);
    Vec3::new(0.0, up, 0.0)
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn drive(
    time: Res<Time>,
    mut state: ResMut<SceneState>,
    mut subs: ResMut<crate::subtitles::Subtitles>,
    mut ship: Query<&mut Transform, (With<Ship>, Without<Role>, Without<Lantern>, Without<Rain>, Without<Flame>)>,
    mut actors: Query<
        (&mut crate::villagers::Villager, &mut Transform, &mut crate::npc_skin::SkinClip, &Role, &mut super::HeadLook),
        (Without<Ship>, Without<Lantern>, Without<Rain>, Without<Flame>),
    >,
    mut lantern: Query<&mut Transform, (With<Lantern>, Without<Ship>, Without<Role>, Without<Rain>, Without<Flame>)>,
    mut rain: Query<(&mut Transform, &Rain), (Without<Ship>, Without<Role>, Without<Lantern>, Without<Flame>)>,
    mut flames: Query<(&mut Transform, &Flame), (Without<Ship>, Without<Role>, Without<Lantern>, Without<Rain>)>,
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

    for (mut v, mut tf, mut clip, role, mut head) in &mut actors {
        let (at, yaw, want_clip, want_head) = match role {
            Role::Harrow => {
                let (c, h) = harrow_beat(t);
                (CHAIR, CHAIR_YAW, c, h)
            }
            Role::Wat => (WAT_AT, WAT_YAW, "Fixing_Kneeling", wat_head(t)),
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
    for (h, env) in clips.iter_mut() {
        if env.is_none() {
            *env = sources.get(&*h).map(|s| crate::lipsync::envelope(s, 0.0, 30.0));
        }
    }
    let t = state.clock.max(0.0);
    for (e, role) in &actors {
        let open = clips
            .iter()
            .zip(&VO)
            .filter(|(_, vo)| vo.2 == *role)
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
    let (eye, at, fov, band, focus_on) = match t {
        // A · the cabin: lantern swinging in the foreground, the squire at his master's leg, the
        //     stern windows behind them full of rain.
        t if t < 7.5 => (v(-1.85, 1.42, -1.45).lerp(v(-1.62, 1.36, -1.22), ease(lin(4.6, 7.5))), v(0.6, 0.85, 0.65), 60.0, 0.12, Some(HARROW_HEAD)),
        // B · "Tighter." — low over Wat's shoulder, up at Harrow (the squire's eye).
        t if t < 10.0 => (v(-0.72, 0.98, 0.62), HARROW_HEAD, 34.0, 0.9, Some(HARROW_HEAD)),
        // C · "…every day for fifteen years…" — the hands at the strap.
        t if t < 13.0 => (v(0.12, 0.5, 1.55).lerp(v(0.2, 0.47, 1.45), lin(10.0, 13.0)), STRAP, 28.0, 1.0, Some(STRAP)),
        // C2 · "…and it's still wrong." / "Yes, Ser." — Wat's face, three-quarter, eyes on the work.
        t if t < 17.0 => (v(0.78, 0.72, 1.48).lerp(v(0.74, 0.72, 1.4), lin(13.0, 17.0)), WAT_HEAD, 30.0, 0.9, Some(WAT_HEAD)),
        // D · "Forty peasants and a Reeve." — Harrow from his right, the helmet on the table below.
        t if t < 25.2 => (v(0.35, 0.92, -0.95).lerp(v(0.42, 0.95, -0.82), lin(17.0, 25.2)), HARROW_HEAD, 36.0, 0.8, Some(HARROW_HEAD)),
        // E · "Do you know what they said about me in the city?" — he turns to the windows; his
        //     profile against the rain, the lightning on "Before it burned?".
        t if t < 30.8 => (v(-1.1, 1.12, 0.52).lerp(v(-0.92, 1.12, 0.58), lin(25.2, 30.8)), v(1.0, 1.15, 0.95), 34.0, 0.6, Some(HARROW_HEAD)),
        // F · "No, Ser." — down on Wat from beside his master's head.
        t if t < 33.2 => (v(0.88, 1.2, 0.6), WAT_HEAD, 34.0, 0.9, Some(WAT_HEAD)),
        // G1 · "Neither do I." — the profile again, tighter.
        t if t < 36.2 => (v(-0.62, 1.12, 0.62), v(1.0, 1.15, 0.85), 28.0, 0.7, Some(HARROW_HEAD)),
        // G2 · "Nobody looks at the man behind the shield, Wat. That's the point of you." — the
        //      squire's angle, pushing in; the shield on the hull behind his head.
        t if t < 41.8 => {
            let k = ease(lin(36.2, 41.8));
            (v(-0.28, 0.82, 0.6).lerp(v(0.02, 0.85, 0.66), k), HARROW_HEAD, 32.0 - 5.0 * k, 1.0, Some(HARROW_HEAD))
        }
        // H1 · the strap cinched.
        t if t < 44.6 => (v(0.22, 0.48, 1.5), STRAP, 26.0, 1.0, Some(STRAP)),
        // H2 · the cabin from the stern corner, the lantern swinging, into black.
        _ => (v(-1.6, 1.45, 1.35).lerp(v(-1.68, 1.5, 1.2), lin(44.6, END)), v(0.6, 0.7, 0.2), 58.0, 0.12, Some(HARROW_HEAD)),
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
fn shell_mesh() -> Mesh {
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
    // forward bulkhead: vertical boards, a door with iron strap hinges
    for i in 0..n {
        let x = -HALF_W + 0.105 + i as f32 * 0.21;
        p.push(cube(0.2, BEAMS + 0.14, 0.06, Vec3::new(x, (BEAMS + 0.14) * 0.5, -HALF_D - 0.03), plank(200 + i)));
    }
    let door_x = -1.0;
    p.push(cube(0.7, 1.46, 0.05, Vec3::new(door_x, 0.73, -HALF_D + 0.02), 0x4a311c));
    for y in [0.3f32, 1.15] {
        p.push(cube(0.56, 0.04, 0.015, Vec3::new(door_x + 0.05, y, -HALF_D + 0.05), IRON));
    }
    p.push(cylr(0.03, 0.02, Vec3::new(door_x - 0.24, 0.78, -HALF_D + 0.06), Quat::from_rotation_x(FRAC_PI_2), IRON));
    for x in [door_x - 0.38, door_x + 0.38] {
        p.push(cube(0.07, 1.52, 0.08, Vec3::new(x, 0.76, -HALF_D + 0.02), 0x3b2817));
    }
    p.push(cube(0.83, 0.07, 0.08, Vec3::new(door_x, 1.5, -HALF_D + 0.02), 0x3b2817));
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
    merged(p)
}

/// A heavy trestle table (origin on the floor under its centre).
fn table_mesh() -> Mesh {
    let mut p = vec![cube(0.72, 0.05, 1.0, Vec3::new(0.0, TABLE_TOP - 0.025, 0.0), 0x6b4a2c)];
    for (x, z) in [(-0.3f32, -0.42f32), (0.3, -0.42), (-0.3, 0.42), (0.3, 0.42)] {
        p.push(cube(0.06, TABLE_TOP - 0.05, 0.06, Vec3::new(x, (TABLE_TOP - 0.05) * 0.5, z), WOOD_DARK));
    }
    p.push(cube(0.05, 0.05, 0.84, Vec3::new(0.0, 0.14, 0.0), WOOD_DARK));
    merged(p)
}

/// Harrow's chair (origin on the floor under the seat, facing +Z: the back at −Z).
fn chair_mesh() -> Mesh {
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
    merged(p)
}

/// Wat's hammock, slung along the port hull in the forward corner, ropes to the beams.
fn hammock_mesh() -> Mesh {
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
    merged(p)
}

fn sea_chest_mesh() -> Mesh {
    merged(vec![
        cube(0.8, 0.42, 0.46, Vec3::new(0.0, 0.21, 0.0), 0x5a3a20),
        cube(0.82, 0.08, 0.48, Vec3::new(0.0, 0.46, 0.0), 0x4a301b),
        cube(0.84, 0.04, 0.03, Vec3::new(0.0, 0.3, 0.24), IRON),
        cube(0.84, 0.04, 0.03, Vec3::new(0.0, 0.3, -0.24), IRON),
    ])
}

fn crate_mesh() -> Mesh {
    merged(vec![cube(0.8, 0.8, 0.8, Vec3::new(0.0, 0.4, 0.0), 0x7a5a36)])
}

/// Two casks lashed in the forward corner.
fn casks_mesh() -> Mesh {
    let mut p = Vec::new();
    for (i, x) in [-0.22f32, 0.22].into_iter().enumerate() {
        p.push(cylr(0.19, 0.5, Vec3::new(x, 0.25, 0.0), Quat::IDENTITY, if i == 0 { WOOD } else { 0x5e4026 }));
        for y in [0.08f32, 0.42] {
            p.push(cylr(0.2, 0.03, Vec3::new(x, y, 0.0), Quat::IDENTITY, IRON));
        }
    }
    merged(p)
}

/// What's on the table (origin = table top centre): a sea chart weighted open, the sealed letter to
/// the Reeve, a pewter cup, an inkwell, the candle in its holder (its flame is a separate card).
fn table_dressing_mesh() -> Mesh {
    merged(vec![
        cube_r(0.4, 0.004, 0.32, Vec3::new(-0.08, 0.002, -0.12), Quat::from_rotation_y(0.2), 0xcbb98f),
        cylr(0.025, 0.36, Vec3::new(-0.05, 0.025, 0.08), Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_x(0.2), 0xbfa97a),
        cube_r(0.16, 0.012, 0.1, Vec3::new(0.2, 0.006, 0.12), Quat::from_rotation_y(-0.35), 0xd9cfb4),
        cylr(0.018, 0.006, Vec3::new(0.2, 0.014, 0.12), Quat::IDENTITY, 0x8a1d18),
        cylr(0.032, 0.09, Vec3::new(-0.2, 0.045, 0.3), Quat::IDENTITY, 0x8d8f92),
        cylr(0.022, 0.04, Vec3::new(0.05, 0.02, -0.33), Quat::IDENTITY, 0x1c1a1a),
        cylr(0.045, 0.012, Vec3::new(0.18, 0.006, -0.3), Quat::IDENTITY, 0x6f5a3c),
        cylr(0.02, 0.13, Vec3::new(0.18, 0.075, -0.3), Quat::IDENTITY, 0xe6dcc0),
    ])
}

/// The hanging lantern (origin at the hook, hanging down −Y): chain, cap, cage, base. The glowing
/// horn panes are a separate emissive box.
fn lantern_mesh() -> Mesh {
    let mut p = vec![
        cylr(0.008, 0.22, Vec3::new(0.0, -0.11, 0.0), Quat::IDENTITY, IRON),
        super::cone_m(0.1, 0.09, Vec3::new(0.0, -0.27, 0.0), Quat::IDENTITY, 0x2f2f33),
        cube(0.15, 0.02, 0.15, Vec3::new(0.0, -0.31, 0.0), 0x2f2f33),
        cube(0.15, 0.025, 0.15, Vec3::new(0.0, -0.5, 0.0), 0x2f2f33),
    ];
    for (x, z) in [(-0.065f32, -0.065f32), (0.065, -0.065), (-0.065, 0.065), (0.065, 0.065)] {
        p.push(cylr(0.007, 0.19, Vec3::new(x, -0.405, z), Quat::IDENTITY, IRON));
    }
    merged(p)
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
