//! **Village life** — the castle courtyard and the street village outside the W gate, lived in.
//!
//! Residents ([`LifeAgent`]: men, women, grey elders and children, each mapped to one of the
//! recorded village voices) follow a day driven by the sky clock: out of the door at dawn with a
//! yawn and a stretch; chores and errands through the morning (draw water at the well and carry
//! the bucket home, sweep the doorstep, scatter grain for the hens, weed the garden on the knees,
//! saw and hammer at the woodpile, hang laundry, buy at the market); the noon meal on a bench
//! (sit down → eat / drink → stand up, the pelvis landing ON the seat and the feet planted by IK);
//! chats in the afternoon (two neighbours meet, face each other and trade talk / listen / nod /
//! laugh beats with a two-line exchange), elders whittling and dozing on the benches, kids playing
//! tag; at dusk everyone heads home through their own door (a mother calls her child in); at
//! night and through a siege the streets are empty.
//!
//! **Smart objects:** every activity is a [`Spot`] — a bench seat, the well winch, a stall
//! counter, a doorstep, a hen coop, a garden bed, a woodpile, the fire — with an approach pose
//! and one occupant. Benches are the town kit's `decor_bench` (new ones round the fire pit, by the
//! well, before the tavern and beside the village doors); the village gets its own well.
//!
//! **Market:** a buyer walks to a counter, browses, haggles (the vendor counters with gestures of
//! her own), pays (coins), and carries a basket or a sack home. Vendors call their wares.
//!
//! **The hero:** standing folk turn their heads to watch him, wave or bow as he passes; kids drop
//! their game to gather and stare. **Voices:** `audio::lines` `Concept::Life(voice, cue)` — spatial
//! at the speaker, only when he is in earshot, globally spaced, and silent until a clip exists.
//!
//! **Perf:** decisions only when a timer runs out (or on a staggered 0.5 s social pass); walking
//! is one steer step per frame; poses are computed only within the biped cull radius; props are
//! spawned once per person and only their `Visibility` flips. No per-frame allocation.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use crate::audio::director::Speak;
use crate::audio::lines::{Concept, LifeCue, LifeVoice};
use crate::biome::BiomeEntity;
use crate::biped::{BipedPart, PoseOverride};
use crate::npc_skin::SkinClip;
use crate::player::anim::village::{self as clip, Act, Carry, SitAct};
use crate::player::anim::Pose;
use crate::player::Joint;
use crate::villagers::Villager;

pub struct VillageLifePlugin;

impl Plugin for VillageLifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LifeWorld>()
            .init_resource::<LifeSfx>()
            .init_resource::<LifeTalk>()
            .init_resource::<Stalls>()
            .add_systems(Startup, load_life_sfx)
            .add_systems(Update, (life_setup, life_houses, life_props, props_to_skin).chain())
            .add_systems(
                Update,
                (life_brain, life_social, vendor_life, guard_barks)
                    .chain()
                    .run_if(in_state(crate::game_state::Modal::None)),
            )
            // Ungated (like `animate_biped`) so a paused world still draws its people posed.
            .add_systems(Update, life_animate.before(crate::biped::animate_biped))
            .add_systems(PostUpdate, seat_fit.before(bevy::transform::TransformSystems::Propagate));
        if std::env::var("FOREST_LIFECHECK").is_ok_and(|v| v != "0") {
            app.add_systems(Update, life_check.after(life_brain));
        }
    }
}

/// Where a seated person's PELVIS joint belongs: over the seat's line, a hand above its top.
const SEAT_PELVIS_UP: f32 = 0.09;

/// Per-person seat correction (world offset applied to the visible body while sitting).
#[derive(Component, Default)]
struct SeatFit(Vec3);

/// **Sit ON the bench, not in it.** The realistic cast's sitting clips put the pelvis wherever
/// their own rig likes (measured by `FOREST_LIFECHECK`: ~0.2 behind the seat line and up to 0.15
/// under the seat top, varying with build and clip), so each sitter's body is eased by a small
/// feedback offset until the skin's real pelvis rests on the seat. The offset is kept per person
/// (the next sit starts from it), faded in over the sit-down and out over the stand-up.
#[allow(clippy::type_complexity)]
fn seat_fit(
    time: Res<Time>,
    mut commands: Commands,
    w: Res<LifeWorld>,
    mut q: Query<(Entity, &LifeAgent, &GlobalTransform, &crate::npc_skin::Skinned, Option<&mut SeatFit>)>,
    joints: Query<&GlobalTransform, Without<LifeAgent>>,
    mut tfs: Query<&mut Transform, Without<LifeAgent>>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.05);
    for (e, a, gt, sk, fit) in &mut q {
        let Some(mut fit) = fit else {
            commands.entity(e).try_insert(SeatFit::default());
            continue;
        };
        // How much of the offset shows: in over the sit-down, out over the stand-up.
        let (weight, learn) = match a.st {
            St::SitDown => (((now - a.t0) / 1.2).clamp(0.0, 1.0), false),
            St::Sit(_) => (1.0, true),
            St::StandUp => ((1.0 - (now - a.t0) / 1.0).clamp(0.0, 1.0), false),
            _ => (0.0, false),
        };
        if learn {
            if let (Some(i), Some(pel)) = (a.spot, sk.pelvis) {
                if let (Some(s), Ok(pg)) = (w.spots.get(i), joints.get(pel)) {
                    let gy = crate::steer::footing(s.pos.x, s.pos.y).unwrap_or(gt.translation().y);
                    let want = Vec3::new(s.pos.x, gy + s.seat_h + SEAT_PELVIS_UP, s.pos.y);
                    let err = want - pg.translation();
                    fit.0 = (fit.0 + err * (6.0 * dt).min(1.0)).clamp(Vec3::splat(-0.4), Vec3::splat(0.4));
                }
            }
        }
        let Ok(mut tf) = tfs.get_mut(sk.inst()) else { continue };
        // World offset → the root's local frame (rotation + uniform scale).
        let (scale, rot, _) = gt.to_scale_rotation_translation();
        let offset = rot.inverse() * (fit.0 * weight) / scale.x.max(1e-3);
        // Only a real change: a write marks the skin root changed and re-propagates its whole rig
        // (~80 joints + meshes) — this ran for every resident every frame, mostly writing zero.
        if tf.translation != offset {
            tf.translation = offset;
        }
    }
}

/// `FOREST_LIFECHECK=1` (+ `FOREST_LIFECHECK_SECS`, default 90): an automated **people collision
/// check** on the live town. Twice a second it audits every resident out of doors:
/// - standing / walking: the body centre must keep 0.08 off every prop and wall (`inside prop`; a
///   person just risen from a bench stands 0.1 in front of it, shins to the plank),
///   and stay 0.4 from everyone else (`overlap`);
/// - seated: the skin's real pelvis must rest ON the seat — 0.0–0.3 above the seat top and within
///   0.22 of the seat line front-to-back (`sits in bench` / `floats` / `off bench`).
/// Prints each violation (first 40) and a tally; at the end logs `LIFECHECK PASS` or
/// `LIFECHECK FAIL n` and quits (exit code 1 on failure), so it runs like a test:
/// `FOREST_LIFECHECK=1 FOREST_DEMO=work FOREST_TIME=0.3 cargo run`.
#[allow(clippy::type_complexity)]
fn life_check(
    time: Res<Time>,
    w: Res<LifeWorld>,
    q: Query<(Entity, &LifeAgent, &Villager, Option<&crate::npc_skin::Skinned>), Without<crate::dying::Dying>>,
    joints: Query<&GlobalTransform>,
    mut st: Local<(f32, u32, u32, std::collections::HashMap<&'static str, u32>)>,
    mut seats: Local<std::collections::BTreeMap<usize, (f32, f32, u32, f32, u8)>>,
    mut exit: MessageWriter<AppExit>,
    mut inside: Local<std::collections::HashMap<Entity, u32>>,
) {
    let now = time.elapsed_secs();
    let secs: f32 = std::env::var("FOREST_LIFECHECK_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(90.0);
    // Let the town settle (houses rise, folk walk out) before judging.
    if now < 20.0 || now < st.0 {
        return;
    }
    st.0 = now + 0.5;
    st.1 += 1;
    let out: Vec<(Entity, Vec2, bool)> = q
        .iter()
        .filter(|(_, a, ..)| !matches!(a.st, St::Indoors))
        .map(|(e, a, v, _)| (e, v.pos, matches!(a.st, St::Sit(_) | St::SitDown | St::StandUp)))
        .collect();
    let view = crate::blockers::read();
    let mut bad = |kind: &'static str, msg: String, st: &mut (f32, u32, u32, std::collections::HashMap<&'static str, u32>)| {
        st.2 += 1;
        *st.3.entry(kind).or_default() += 1;
        if st.2 <= 40 {
            warn!("LIFECHECK {kind}: {msg}");
        }
    };
    for (e, a, v, sk) in &q {
        if matches!(a.st, St::Indoors) {
            continue;
        }
        let p = v.pos;
        match a.st {
            St::Sit(_) => {
                // (the seat fit settles within the first second of the sit)
                if now - a.t0 < 1.5 {
                    continue;
                }
                let (Some(i), Some(pel)) = (a.spot, sk.and_then(|s| s.pelvis)) else { continue };
                let Ok(g) = joints.get(pel) else { continue };
                let s = w.spots[i];
                let gy = crate::steer::footing(s.pos.x, s.pos.y).unwrap_or(0.0);
                let pp = g.translation();
                let up = pp.y - (gy + s.seat_h);
                let f = fwd(s.face);
                let along = (Vec2::new(pp.x, pp.z) - s.pos).dot(f);
                let m = seats.entry(i).or_insert((0.0, 0.0, 0, s.seat_h, a.body as u8));
                m.0 += up;
                m.1 += along;
                m.2 += 1;
                if up < 0.03 {
                    bad("sits in bench", format!("{e:?} seat {i} at {:.1},{:.1}: pelvis {up:.2} under the seat top", s.pos.x, s.pos.y), &mut st);
                } else if up > 0.2 {
                    bad("floats", format!("{e:?} seat {i}: pelvis {up:.2} above the seat top"), &mut st);
                }
                if along.abs() > 0.1 {
                    bad("off bench", format!("{e:?} seat {i} at {:.1},{:.1}: pelvis {along:.2} off the seat line", s.pos.x, s.pos.y), &mut st);
                }
            }
            St::SitDown | St::StandUp => {}
            _ => {
                // (a graze for one sample while sliding past isn't visible; one second inside is)
                let streak = inside.entry(e).or_default();
                *streak = if view.any_within(p.x, p.y, 0.08) { *streak + 1 } else { 0 };
                if *streak >= 2 {
                    bad("inside prop", format!("{e:?} {:?} {:?} at {:.2},{:.2} after {:?} path {}/{} stall {:.1} replans {} until {:.1} spot {:?}", a.body, a.st, p.x, p.y, a.after, a.path_i, a.path.len(), a.stall, a.replans, a.until - now, a.spot) + &{
                    let hs = crate::castle::houses();
                    hs.iter().enumerate().min_by(|x, y| x.1.pos.distance(p).total_cmp(&y.1.pos.distance(p))).map_or(String::new(), |(i, h)| {
                        let l = rot2(p - h.pos, -h.yaw);
                        format!(" | house {i} {} half {:.2},{:.2} local {:.2},{:.2}", h.piece, h.half.x, h.half.y, l.x, l.y)
                    })
                }, &mut st);
                }
                for &(o, op, _) in &out {
                    if o != e && Some(o) != a.partner && o.index_u32() < e.index_u32() && op.distance(p) < 0.4 {
                        bad("overlap", format!("{e:?} {:?} with {o:?} at {:.2},{:.2} ({:.2} apart)", a.st, p.x, p.y, op.distance(p)), &mut st);
                    }
                }
                if a.st == St::Walk {
                    for &(o, op, seated) in &out {
                        // (taking the seat beside them is fine)
                        if seated && o != e && op.distance(p) < 0.45 && a.goal.distance(op) > 1.0 {
                            bad("walks through sitter", format!("{e:?} at {:.2},{:.2}", p.x, p.y), &mut st);
                        }
                    }
                }
            }
        }
    }
    if now >= 20.0 + secs {
        let n = st.2;
        let mut tally: Vec<_> = st.3.iter().map(|(k, v)| format!("{k}={v}")).collect();
        tally.sort();
        if n == 0 {
            info!("LIFECHECK PASS ({} samples)", st.1);
            exit.write(AppExit::Success);
        } else {
            error!("LIFECHECK FAIL {n} over {} samples: {}", st.1, tally.join(" "));
            let mut hist = std::collections::BTreeMap::<String, u32>::new();
            for (_, a, ..) in &q {
                *hist.entry(format!("{:?}", a.st)).or_default() += 1;
            }
            error!("LIFECHECK states now: {hist:?}");
            for (i, (u, al, n, h, b)) in seats.iter() {
                let n = *n as f32;
                info!("LIFECHECK seat {i}: seat_h {h:.2} body {b} pelvis up {:.3} along {:.3}", u / n, al / n);
            }
            exit.write(AppExit::error());
        }
    }
}

// ── Data ─────────────────────────────────────────────────────────────────────────────────

/// A resident's body — picks the model look, the walk speed and the voice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Body {
    Man,
    Woman,
    Elder,
    Kid,
}

impl Body {
    fn voice(self) -> LifeVoice {
        match self {
            Body::Man => LifeVoice::Man,
            Body::Woman => LifeVoice::Woman,
            Body::Elder => LifeVoice::Elder,
            Body::Kid => LifeVoice::Child,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    Courtyard,
    Village,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SpotKind {
    Seat,
    Well,
    Laundry,
    Sweep,
    Hens,
    Garden,
    Wood,
    Fire,
    Play,
    /// Unload the courtyard cart / the hay corner: lift a sack and carry it into the keep.
    Haul,
    /// Guard drill: two men with wooden wasters face each other in the open yard.
    Spar,
}

/// One smart object: where the user's ROOT goes, the yaw it faces while busy, and its occupant.
#[derive(Clone, Copy, Debug)]
struct Spot {
    kind: SpotKind,
    pos: Vec2,
    face: f32,
    zone: Zone,
    /// Bench id (seats on one bench share it → seated neighbours chat).
    bench: u16,
    /// Seat top height above the ground (world units), seats only.
    seat_h: f32,
    /// The house it belongs to (doorstep / garden / hens / woodpile), if any.
    house: Option<u8>,
    /// Only usable once this many houses stand (house-gated castle dressing).
    need_houses: u32,
    taken: Option<Entity>,
}

/// Seat top of the kit's split-log bench (`village_huts.py::decor_bench`: slab centre 0.42 + 0.04).
const BENCH_TOP: f32 = 0.46;
/// Seat top of a plank bench baked into a building (`town_kit.py::bench`: board centre 0.42 + 0.03).
const PLANK_TOP: f32 = 0.45;
/// Seat top of the fire pit's split logs (`town_kit.py::town_firepit`: log axis 0.36 + r 0.165).
const LOG_TOP: f32 = 0.52;
/// Narrowest centre-to-centre spacing of two sitters on one seat (shoulders + elbows clear).
const SEAT_GAP: f32 = 0.6;

/// The world's activity spots + bookkeeping (rebuilt with the world).
#[derive(Resource, Default)]
struct LifeWorld {
    built: bool,
    spots: Vec<Spot>,
    /// Houses already populated (their residents + spots exist).
    houses: u32,
    next_bench: u16,
    seed: u32,
    /// Door (outside), inside (hidden park) and outward yaw per populated house.
    doors: Vec<(Vec2, Vec2, f32)>,
    keep_door: Option<(Vec2, Vec2, f32)>,
    play: [Vec2; 2],
}

/// The market counters: who's buying at each, and what the vendor should be doing.
#[derive(Resource, Default)]
struct Stalls {
    slots: Vec<(Vec2, f32, Vec2)>,
    buyer: [Option<Entity>; 4],
    /// The vendor's cue from its buyer: act + until.
    cue: [(Act, f32); 4],
}

/// Global pacing of the residents' voices (one line at a time, spaced) + the SFX set.
#[derive(Resource, Default)]
struct LifeTalk {
    next: f32,
    sfx_next: f32,
    /// The last few two-handers played (index + 1; 0 = empty), so a pair doesn't replay one.
    recent: [u8; 4],
    recent_i: usize,
    /// The two-hander being played, beat by beat (see [`DlgRun`]).
    run: Option<DlgRun>,
    /// Last moment the voice channel was busy — each beat waits a breath after the last one ends.
    busy_at: f32,
}

/// A scripted conversation in progress. Beats are NOT timed from the transcript: each waits for the
/// voice channel to fall silent (the real clip length), so a slow read never gets talked over or
/// dropped — which matters now that a scene runs 7-11 lines.
#[derive(Clone, Copy)]
struct DlgRun {
    d: u8,
    /// Next beat to speak.
    k: u8,
    /// The chat's lead and partner; `a_opens`: the lead reads role A.
    lead: Entity,
    partner: Entity,
    a_opens: bool,
    /// Earliest time for beat `k` (lets the director start the previous clip first).
    at: f32,
}

/// Pick a recorded two-hander for a chatting pair with voices (`va` lead, `vb` partner), not one
/// played lately. Returns (dialogue, does the lead open it?).
fn pick_dialogue(talk: &LifeTalk, rng: &mut u32, va: LifeVoice, vb: LifeVoice) -> Option<(u8, bool)> {
    use crate::audio::lines::{life_dialogue_recorded, LIFE_DIALOGUES};
    let fits: Vec<(u8, bool)> = LIFE_DIALOGUES
        .iter()
        .enumerate()
        .filter(|(i, _)| life_dialogue_recorded(*i as u8) && !talk.recent.contains(&(*i as u8 + 1)))
        .filter_map(|(i, d)| {
            if d.a == va && d.b == vb {
                Some((i as u8, true))
            } else if d.a == vb && d.b == va {
                Some((i as u8, false))
            } else {
                None
            }
        })
        .collect();
    if fits.is_empty() {
        return None;
    }
    Some(fits[(r01(rng) * fits.len() as f32) as usize % fits.len()])
}

#[derive(Resource, Default)]
struct LifeSfx(Vec<(&'static str, Handle<AudioSource>)>);

const SFX_KEYS: [&str; 23] = [
    "life_sfx_spar",
    "life_sfx_coins",
    "life_sfx_purse",
    "life_sfx_bench_sit",
    "life_sfx_bench_stand",
    "life_sfx_bread",
    "life_sfx_mug",
    "life_sfx_well",
    "life_sfx_bucket",
    "life_sfx_sweep",
    "life_sfx_laundry",
    "life_sfx_hammer",
    "life_sfx_saw",
    "life_sfx_whittle",
    "life_sfx_grain",
    "life_sfx_sack",
    "life_sfx_basket",
    "life_sfx_door",
    "life_sfx_door_shut",
    "life_sfx_shutters",
    "life_sfx_hen",
    "life_sfx_dice",
    "life_sfx_pour",
];

/// Only the SFX actually on disk get a handle — a missing one stays silent without an asset error.
/// Extra takes `<key>_2.ogg` … `<key>_4.ogg` load under the same key; `sfx` picks one at random.
fn load_life_sfx(mut commands: Commands, assets: Res<AssetServer>) {
    let mut v = Vec::new();
    for k in SFX_KEYS {
        for n in 1..=4 {
            let rel = if n == 1 { format!("audio/sfx/life/{k}.ogg") } else { format!("audio/sfx/life/{k}_{n}.ogg") };
            if crate::realforest::load::asset_path(&rel).is_file() {
                v.push((k, assets.load(rel)));
            }
        }
    }
    commands.insert_resource(LifeSfx(v));
}

/// What the brain is doing.
#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    /// Inside the house (hidden, parked within the walls).
    Indoors,
    Walk,
    Idle,
    SitDown,
    Sit(SitAct),
    StandUp,
    Do(Act),
    /// Talking with `partner` (turns alternate).
    Chat,
    /// A wave / bow for the passing knight.
    Greet(Act),
    /// At a market counter: browse → haggle → pay.
    Buy(u8),
    /// A child at play (tag).
    Play,
}

/// What to do on arrival.
#[derive(Clone, Copy, PartialEq, Debug)]
enum After {
    Nothing,
    Spot,
    Enter,
    Chat,
    Buy(u8),
    Stare,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    None,
    Bread,
    Mug,
    Broom,
    Bucket,
    Basket,
    Sack,
    Hammer,
    Saw,
    Stick,
    Waster,
}

/// A village-life resident. Its [`Villager`] carries the shared pose/locomotion fields; this is
/// the daily-life brain on top.
#[derive(Component)]
pub struct LifeAgent {
    body: Body,
    zone: Zone,
    /// (door outside, park point inside, outward yaw)
    home: (Vec2, Vec2, f32),
    house: Option<u8>,
    st: St,
    t0: f32,
    until: f32,
    spot: Option<usize>,
    after: After,
    goal: Vec2,
    path: Vec<Vec2>,
    path_i: usize,
    carry: Carry,
    held: Held,
    rng: u32,
    root_scale: f32,
    rig_off: f32,
    /// Hips height (rig units) that rests the backside on the current seat.
    seat: f32,
    look: f32,
    greet_cd: f32,
    stare_cd: f32,
    want_chat: bool,
    partner: Option<Entity>,
    /// Chat: does this one talk on even beats?
    lead: bool,
    /// Queued lines (replies / later beats of an exchange): each fires at its `.0`.
    vo: Vec<(f32, LifeCue)>,
    /// Kid at play: chasing (it) or fleeing.
    it: bool,
    /// Pending root shift for the animator's cross-fade (rig units, +Z) after a seat move.
    shift: f32,
    /// The yaw a busy (non-walking) resident turns to face — eased, never snapped.
    face: f32,
    /// Walk: seconds spent not advancing (boxed in by a prop or another walker).
    stall: f32,
    /// Next time a passing neighbour may be greeted.
    nod_cd: f32,
    /// Walk: re-plans spent on this trip before giving it up.
    replans: u8,
    inited: bool,
}

/// The pose cross-fade state (written by [`life_animate`]).
#[derive(Component)]
pub(crate) struct LifeAnim {
    key: u32,
    from: Pose,
    last: Pose,
    t0: f32,
    moving: f32,
}

/// The hand props of one resident (children of the rig's joints), shown per activity.
#[derive(Component)]
struct LifeProps {
    items: [(Held, Entity); 10],
    shown: Held,
}

/// A market vendor's little life (call out, haggle, hand over the goods).
#[derive(Component)]
struct VendorLife {
    stall: usize,
    act: Act,
    t0: f32,
    until: f32,
    next_hawk: f32,
    /// A queued answer to the buyer: (at, cue).
    say: Option<(f32, LifeCue)>,
}

/// Marks a bench / well this module placed (world-rebuild detection).
#[derive(Component)]
struct LifeFixture;

// ── Small helpers ───────────────────────────────────────────────────────────────────────

fn hash(s: &mut u32) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}
fn r01(s: &mut u32) -> f32 {
    (hash(s) & 0x00ff_ffff) as f32 / 16_777_216.0
}
fn rr(s: &mut u32, a: f32, b: f32) -> f32 {
    a + (b - a) * r01(s)
}
fn fwd(yaw: f32) -> Vec2 {
    Vec2::new(yaw.sin(), yaw.cos())
}
fn yaw_to(from: Vec2, to: Vec2) -> f32 {
    let d = to - from;
    d.x.atan2(d.y)
}
fn rot2(v: Vec2, yaw: f32) -> Vec2 {
    let r = Quat::from_rotation_y(yaw) * Vec3::new(v.x, 0.0, v.y);
    Vec2::new(r.x, r.z)
}

/// The day, from the sky clock (`t` 0 dawn · 0.25 noon · 0.5 dusk · 0.75 midnight).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Dawn,
    Work,
    Noon,
    Afternoon,
    Dusk,
    Night,
}

fn phase_of(t: f32, wave: bool) -> Phase {
    if wave {
        return Phase::Night;
    }
    match t {
        t if t < 0.07 => Phase::Dawn,
        t if t < 0.19 => Phase::Work,
        t if t < 0.31 => Phase::Noon,
        t if t < 0.45 => Phase::Afternoon,
        t if t < 0.56 => Phase::Dusk,
        t if t < 0.965 => Phase::Night,
        _ => Phase::Dawn,
    }
}

// ── Setup: spots, benches, the village well, residents ──────────────────────────────────

/// Sitters on one piece of furniture: the seat line centred at `c` running along local +X of
/// `yaw`, `offs` along it, each sitter facing `face` with the hips `fwd_off` toward the face off the
/// line (a round log is sat on just forward of its crown). The spot is the HIPS anchor — the pelvis
/// rests on the seat top `h`, the thighs over the seat, the feet planted on the ground in front.
/// A seat whose leg room (the strip the shins and feet need in front) is blocked is dropped.
#[allow(clippy::too_many_arguments)]
fn add_seats(w: &mut LifeWorld, c: Vec2, yaw: f32, face: f32, h: f32, offs: &[f32], fwd_off: f32, zone: Zone, need: u32) {
    let id = w.next_bench;
    w.next_bench += 1;
    let f = fwd(face);
    let view = crate::blockers::read();
    for &o in offs {
        let p = c + rot2(Vec2::new(o, 0.0), yaw) + f * fwd_off;
        if !leg_room(&view, p, face) || !clear_of_market(p, face) {
            info!("village_life: seat at {:.1},{:.1} dropped (leg room blocked)", p.x, p.y);
            continue;
        }
        w.spots.push(Spot { kind: SpotKind::Seat, pos: p, face, zone, bench: id, seat_h: h, house: None, need_houses: need, taken: None });
    }
}

/// The strip in front of a seat the shins and feet occupy (0.3–0.65 ahead, ±0.2 wide) is clear of
/// every solid (walls, houses, plots, yard props, other benches).
fn leg_room(view: &crate::blockers::BlockView, p: Vec2, face: f32) -> bool {
    let f = fwd(face);
    let side = Vec2::new(f.y, -f.x);
    [0.3_f32, 0.48, 0.65].iter().all(|&d| [-0.2_f32, 0.0, 0.2].iter().all(|&s| {
        let q = p + f * d + side * s;
        !view.any_within(q.x, q.y, 0.0)
    }))
}

/// The sitter's body (hips ±0.25) and leg room stay out of every market stall / loose market prop
/// (laid out independently of the tavern's furniture, and possibly after the seats).
fn clear_of_market(p: Vec2, face: f32) -> bool {
    let f = fwd(face);
    let boxes = crate::market::footprints();
    [-0.1_f32, 0.2, 0.5].iter().all(|&d| {
        let q = p + f * d;
        boxes.iter().all(|(c, half, yaw)| {
            let l = rot2(q - *c, -*yaw).abs();
            l.x > half.x + 0.25 || l.y > half.y + 0.25
        })
    })
}

/// Props that are NOT in the blocker grid (walk-through or visual-only) but still own their
/// ground: the fire pit with its log benches and tripod, every bench seat already placed.
fn fixed_props(w: &LifeWorld) -> Vec<(Vec2, f32)> {
    let mut v: Vec<(Vec2, f32)> = w.spots.iter().filter(|s| s.kind == SpotKind::Seat).map(|s| (s.pos, 0.45)).collect();
    if let Some(fire) = crate::castle::town_plot("fire_pit") {
        v.push((fire.pos, 2.45));
    }
    for name in ["well"] {
        if let Some(tp) = crate::castle::town_plot(name) {
            v.push((tp.pos, 0.9));
        }
    }
    v
}

/// Can a NEW split-log bench stand at `pos`/`yaw` (sitters facing `face`)? Its whole footprint
/// (1.2 × 0.3 + a hand's margin) and the sitters' leg room must be clear of every solid and of
/// every walk-through prop (fire pit, seats already placed) — a new bench never goes where
/// something already stands; reuse that thing's own seat instead.
fn bench_fits(w: &LifeWorld, pos: Vec2, yaw: f32, face: f32) -> bool {
    let view = crate::blockers::read();
    let props = fixed_props(w);
    let free = |q: Vec2| !view.any_visual_within(q.x, q.y, 0.05) && props.iter().all(|(c, r)| q.distance(*c) > *r);
    for x in [-0.66_f32, -0.33, 0.0, 0.33, 0.66] {
        for z in [-0.2_f32, 0.0, 0.2] {
            if !free(pos + rot2(Vec2::new(x, z), yaw)) {
                return false;
            }
        }
    }
    let f = fwd(face);
    [-0.3_f32, 0.3].iter().all(|&o| {
        let p = pos + rot2(Vec2::new(o, 0.0), yaw);
        leg_room(&view, p, face) && [0.35_f32, 0.6].iter().all(|&d| free(p + f * d))
    })
}

/// A NEW split-log bench (`decor_bench`) at `pos`/`yaw`, sitters facing `face` — only where it
/// fits ([`bench_fits`]); else nothing. Returns whether it was placed.
#[allow(clippy::too_many_arguments)]
fn add_bench(commands: &mut Commands, w: &mut LifeWorld, pos: Vec2, yaw: f32, face: f32, zone: Zone, need: u32) -> bool {
    if !bench_fits(w, pos, yaw, face) {
        info!("village_life: no bench at {:.1},{:.1} (something already stands there)", pos.x, pos.y);
        return false;
    }
    let y = crate::worldmap::ground_at_world(pos.x, pos.y).unwrap_or(0.0);
    let tf = Transform::from_xyz(pos.x, y - 0.03, pos.y).with_rotation(Quat::from_rotation_y(yaw));
    if let Some(e) = crate::town_kit::spawn_piece(commands, "decor_bench", tf) {
        commands.entity(e).insert((BiomeEntity, LifeFixture));
    }
    crate::blockers::add_obb(pos.x, pos.y, 0.60, 0.17, yaw);
    add_seats(w, pos, yaw, face, BENCH_TOP, &[-0.3, 0.3], 0.0, zone, need);
    true
}

/// The seats a house's own model already has by its door (`village_huts.py`, house-local frame:
/// centre, axis yaw, facing, seat top, offsets along the axis) — `None` for a house without one.
fn house_seats(piece: &str) -> Option<(Vec2, f32, f32, f32, &'static [f32])> {
    if let Some((_, d)) = crate::castle::hut_dims(piece) {
        // Every settler hut: bench(V(-1.0, 0, D/2 + 0.45), L=0.9) left of the door (village_huts.py).
        return Some((Vec2::new(-1.0, d / 2.0 + 0.45), 0.0, 0.0, PLANK_TOP, &[0.0]));
    }
    match piece {
        // bench(V(W/2 + 0.45, 0, -0.9), π/2, L=1.1) built along X, then the whole cottage turned
        // −π/2 (+X → +Z): (x, z) → (−z, x) — so 0.45 before the road-end wall, along X.
        "town_street_house_a" => Some((Vec2::new(0.9, 4.05), 0.0, 0.0, PLANK_TOP, &[-0.28, 0.28])),
        // bench(V(1.05, 0, D/2 + 0.45), L=1.0), the whole cottage then shifted +0.65 (ox).
        "town_house_a" => Some((Vec2::new(1.70, 1.95), 0.0, 0.0, PLANK_TOP, &[0.0])),
        // bench(V(2.0 − W/2 + 0.35, 0, D/2 + 0.45), L=0.9).
        "town_house_c" => Some((Vec2::new(0.65, 1.85), 0.0, 0.0, PLANK_TOP, &[0.0])),
        _ => None,
    }
}

/// Where a house WITHOUT its own bench gets a split-log one (house-local: centre, yaw, facing) —
/// chosen clear of its door, shop counter, barrel, chopping block and muck heap.
fn house_new_bench(piece: &str) -> Option<(Vec2, f32, f32)> {
    match piece {
        // Road end: door x −1.6..−0.8, shop counter x 0.1..1.4, barrel x 1.85 → along the bare −X
        // side wall near the front corner, facing out.
        "town_street_house_b" => Some((Vec2::new(-2.66, 2.3), FRAC_PI_2, -FRAC_PI_2)),
        // Between the door (x −2.0..−1.2) and the barrel (x 0.14..0.66), against the front wall.
        "town_house_b" => Some((Vec2::new(-0.56, 2.03), 0.0, 0.0)),
        // The dug-in hovel: its earth bank runs right round the front — no room for a bench.
        _ => None,
    }
}

fn add_spot(w: &mut LifeWorld, kind: SpotKind, pos: Vec2, face: f32, zone: Zone, house: Option<u8>, need: u32) {
    w.spots.push(Spot { kind, pos, face, zone, bench: u16::MAX, seat_h: 0.0, house, need_houses: need, taken: None });
}

/// A clear spot for the village's own well: on the lane mouth's open ground, off the road and
/// clear of every house plot and reserved plot.
pub(crate) fn village_well_pos() -> Option<Vec2> {
    let houses = crate::castle::houses();
    let clear = |p: Vec2| {
        crate::castle::plot_clear(p.x, p.y, 1.6)
            && crate::castle::village_dist(p.x, p.y) > 3.2
            && crate::castle::village_dist(p.x, p.y) < 6.5
            && crate::castle::outside_dist(p.x, p.y) > 8.0
            && houses.iter().all(|h| {
                let (c, half, yaw) = h.plot();
                let l = rot2(p - c, -yaw);
                l.x.abs() > half.x + 1.4 || l.y.abs() > half.y + 1.4
            })
    };
    let mut s = 20.0;
    while s < 52.0 {
        let (p, dir) = crate::castle::polyline_at(&crate::castle::STREET, s);
        let left = Vec2::new(-dir.y, dir.x);
        for side in [-1.0, 1.0] {
            for off in [4.2, 5.0] {
                let q = p + left * side * off;
                if clear(q) {
                    return Some(q);
                }
            }
        }
        s += 1.0;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn life_setup(
    mut commands: Commands,
    mut w: ResMut<LifeWorld>,
    mut stalls: ResMut<Stalls>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    castle: Query<(), With<crate::castle::CastlePart>>,
    fixtures: Query<(), With<LifeFixture>>,
    agents: Query<Entity, With<LifeAgent>>,
) {
    // A world rebuild cleared our fixtures (BiomeEntity) → lay everything out again.
    if w.built && fixtures.is_empty() && !castle.is_empty() {
        for e in &agents {
            commands.entity(e).try_despawn();
        }
        *w = LifeWorld::default();
    }
    if w.built || castle.is_empty() || crate::realforest::get().filter(|k| k.has("decor_bench")).is_none() {
        return;
    }
    w.built = true;
    w.seed = 0x11fe_5eed;
    use crate::castle::{KEEP_FRONT, KEEP_HALF, KEEP_POS, KEEP_STEPS};
    let c = Zone::Courtyard;
    // The existing keep bench (castle_decor `decor_bench`) — seats only, it already stands.
    add_seats(&mut w, Vec2::new(KEEP_POS.x - KEEP_HALF.x - 1.6, KEEP_FRONT + 0.4), 0.0, 0.0, BENCH_TOP, &[-0.3, 0.3], 0.0, c, 0);
    if let Some(fire) = crate::castle::town_plot("fire_pit") {
        // The fire pit's OWN three split-log benches (`town_kit.py::town_firepit`: centres at
        // r 2.05, angle a = 1.0 + k·2π/3, i.e. (2.05 cos a, 2.05 sin a), log axis tangent) — two
        // sitters on each, facing the fire, hips just forward of the log's crown.
        let f = fire.pos;
        // The tripod's splayed legs (r 1.0) and the hot stones: nobody walks through the fire.
        crate::blockers::add(f.x, f.y, 1.0);
        crate::blockers::add_obb(f.x, f.y, 0.92, 0.92, std::f32::consts::FRAC_PI_4);
        for k in 0..3 {
            let a = 1.0 + k as f32 * TAU / 3.0;
            let lc = f + Vec2::new(a.cos(), a.sin()) * 2.05;
            // rot2(+X, yaw) = (cos yaw, −sin yaw) = the tangent (−sin a, cos a).
            let tangent_yaw = (-a.cos()).atan2(-a.sin());
            // The log itself is solid (folk sit ON it, never walk THROUGH it).
            crate::blockers::add_obb(lc.x, lc.y, 0.62, 0.16, tangent_yaw);
            // One sitter per log and a single standing warmer: three logs × two seats + three standing
            // spots packed nine people round one small fire (player: "za duże skupiska ludzi").
            add_seats(&mut w, lc, tangent_yaw, yaw_to(lc, f), LOG_TOP, &[0.0], 0.04, c, 0);
            // Standing room to warm the hands: in the gap between this log and the next, inside the
            // ring of logs and outside the tripod's legs (r 1.0).
            if k == 0 {
                let g = a + TAU / 6.0;
                let p = f + Vec2::new(g.cos(), g.sin()) * 1.8;
                add_spot(&mut w, SpotKind::Fire, p, yaw_to(p, f), c, None, 0);
            }
        }
    }
    if let Some(well) = crate::castle::town_plot("well") {
        let p = well.pos + Vec2::new(0.0, 1.0);
        add_spot(&mut w, SpotKind::Well, p, PI, c, None, 0);
        add_bench(&mut commands, &mut w, well.pos + Vec2::new(2.3, 0.9), FRAC_PI_2, -FRAC_PI_2, c, 0);
    }
    let wood = Vec2::new(crate::castle::WOOD_YARD.0, crate::castle::WOOD_YARD.1);
    let wy = crate::castle::WOOD_YARD.2;
    let p = wood + rot2(Vec2::new(0.0, 0.85), wy);
    add_spot(&mut w, SpotKind::Wood, p, yaw_to(p, wood), c, None, 0);
    let p = wood + rot2(Vec2::new(1.25, -0.6), wy);
    add_spot(&mut w, SpotKind::Wood, p, yaw_to(p, wood + rot2(Vec2::new(0.0, -0.9), wy)), c, None, 0);
    // The courtyard laundry line + garden (castle_decor, shown once a second house stands).
    for x in [-22.2, -20.8] {
        add_spot(&mut w, SpotKind::Laundry, Vec2::new(x, 11.75), PI, c, None, 2);
    }
    add_spot(&mut w, SpotKind::Garden, Vec2::new(-16.8, 14.1), yaw_to(Vec2::new(-16.8, 14.1), Vec2::new(-17.6, 13.2)), c, None, 2);
    add_spot(&mut w, SpotKind::Sweep, KEEP_STEPS + Vec2::new(1.6, 0.5), FRAC_PI_2, c, None, 0);
    w.keep_door = Some((KEEP_STEPS + Vec2::new(0.0, 0.3), KEEP_POS, 0.0));
    // Unloading: beside the hand-cart's bed (its barrels stand on the other side) and at the hay
    // corner's bales — lift a sack / a truss and carry it across the yard into the keep.
    for ((x, z, yaw), lx, lz) in [(crate::castle::CART_CORNER, -0.2, 0.9), (crate::castle::HAY_CORNER, -0.2, 0.75)] {
        let base = Vec2::new(x, z);
        let p = base + rot2(Vec2::new(lx, lz), yaw);
        add_spot(&mut w, SpotKind::Haul, p, yaw + PI, c, None, 0);
    }
    // The guard drill: two marks 1.7 apart on the open yard east of the keep lane, facing.
    let id = w.next_bench;
    w.next_bench += 1;
    for (p, face) in [(Vec2::new(5.6, 2.6), FRAC_PI_2), (Vec2::new(7.3, 2.6), -FRAC_PI_2)] {
        w.spots.push(Spot { kind: SpotKind::Spar, pos: p, face, zone: c, bench: id, seat_h: 0.0, house: None, need_houses: 0, taken: None });
    }
    // The tavern's OWN benches (`town_kit.py::town_tavern`, tavern-local): the long one against
    // the front (centre (−1.6, HZ + 0.45), L 1.6 — its west end stops short of the barrels) and
    // the two either side of the trestle table (centre (1.15, HZ + 1.45) turned 0.08, benches at
    // ±0.6, legs under the board, clear of the trestle legs at ±0.65).
    if let Some(tav) = crate::castle::outer_plots().iter().find(|p| p.name == "tavern") {
        let at = |x: f32, z: f32| tav.pos + rot2(Vec2::new(x, z), tav.yaw);
        let v = Zone::Village;
        add_seats(&mut w, at(-1.6, 2.75), tav.yaw, tav.yaw, PLANK_TOP, &[-0.32, 0.38], 0.0, v, 0);
        // The trestle table and its two benches are SOLID (folk kept walking through the board);
        // their seats are dropped rather than stepped over — the front bench is the tavern's
        // sitting place. The barrel stack by the west end is solid too.
        let ty = tav.yaw + 0.08;
        let tc = at(1.15, 3.75);
        crate::blockers::add_obb(tc.x, tc.y, 0.9, 0.8, ty);
        let bar = at(-2.72, 2.95);
        crate::blockers::add_obb(bar.x, bar.y, 0.62, 0.38, tav.yaw);
    }
    // The village's own well.
    if let Some(vw) = village_well_pos() {
        let y = crate::worldmap::ground_at_world(vw.x, vw.y).unwrap_or(0.0);
        let tf = Transform::from_xyz(vw.x, y - 0.03, vw.y);
        if let Some(e) = crate::town_kit::spawn_piece(&mut commands, "yard_well", tf) {
            commands.entity(e).insert((BiomeEntity, LifeFixture));
            crate::blockers::add(vw.x, vw.y, 0.62);
            let p = vw + Vec2::new(0.0, 1.0);
            add_spot(&mut w, SpotKind::Well, p, PI, Zone::Village, None, 0);
            info!("village_life: village well at {:.1},{:.1}", vw.x, vw.y);
        }
    }
    w.play = [crate::castle::Gate::South.interior(9.0) + Vec2::new(-5.0, 0.0), crate::castle::LANE[1] + Vec2::new(2.6, 0.0)];
    let play = w.play;
    add_spot(&mut w, SpotKind::Play, play[0], 0.0, c, None, 0);
    add_spot(&mut w, SpotKind::Play, play[1], 0.0, Zone::Village, None, 0);
    stalls.slots = crate::market::stall_slots().to_vec();

    // The courtyard crew: two elders for the benches, two women, two men, two kids — they live
    // in the keep (in and out through its door).
    let crew = [
        Body::Elder, Body::Elder, Body::Woman, Body::Woman, Body::Woman, Body::Man, Body::Man, Body::Man, Body::Man, Body::Man,
        Body::Kid, Body::Kid, Body::Kid,
    ];
    let starts: Vec<Vec2> = w.spots.iter().filter(|s| s.zone == c && s.need_houses == 0).map(|s| s.pos).collect();
    let door = w.keep_door.unwrap();
    for (i, b) in crew.into_iter().enumerate() {
        let seed = hash(&mut w.seed);
        let at = starts[(seed as usize) % starts.len()] + fwd(i as f32 * 2.1) * 1.6;
        spawn_resident(&mut commands, &mut meshes, &mut creature_mats, b, at, c, door, None, seed);
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_resident(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
    body: Body,
    at: Vec2,
    zone: Zone,
    home: (Vec2, Vec2, f32),
    house: Option<u8>,
    seed: u32,
) {
    let (e, root_scale, rig_off) = crate::villagers::spawn_life_peasant(commands, meshes, creature_mats, at, body, seed);
    let mut rng = seed | 1;
    let agent = LifeAgent {
        body,
        zone,
        home,
        house,
        st: St::Idle,
        t0: 0.0,
        until: rr(&mut rng, 0.2, 2.5),
        spot: None,
        after: After::Nothing,
        goal: at,
        path: Vec::with_capacity(32),
        path_i: 0,
        carry: Carry::None,
        held: Held::None,
        rng,
        root_scale,
        rig_off,
        seat: 0.7,
        look: 0.0,
        greet_cd: 0.0,
        stare_cd: 0.0,
        want_chat: false,
        partner: None,
        lead: false,
        vo: Vec::new(),
        it: false,
        shift: 0.0,
        face: 0.0,
        stall: 0.0,
        nod_cd: 0.0,
        replans: 0,
        inited: false,
    };
    let mut ec = commands.entity(e);
    ec.insert((agent, PoseOverride(clip::stand(Act::Stand, 0.0, 0.0))));
    if body == Body::Kid {
        ec.insert(crate::villagers::Kid);
    }
}

/// Populate each newly raised house: its residents, its doorstep / garden / hens / woodpile
/// spots and a bench by the door.
fn life_houses(
    mut commands: Commands,
    mut w: ResMut<LifeWorld>,
    town: Res<crate::town::TownRes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    agents: Query<(Entity, &LifeAgent)>,
) {
    if !w.built {
        return;
    }
    let built = (town.0.houses as usize).min(crate::castle::houses().len()) as u32;
    if built < w.houses {
        // A load / new run shrank the village: drop the residents of the lost houses.
        for (e, a) in &agents {
            if a.house.is_some_and(|h| h as u32 >= built) {
                commands.entity(e).try_despawn();
            }
        }
        w.spots.retain(|s| s.house.is_none_or(|h| (h as u32) < built));
        w.doors.truncate(built as usize);
        w.houses = built;
        return;
    }
    // One house per frame — spreads the spawn cost.
    if built == w.houses {
        return;
    }
    let i = w.houses as usize;
    w.houses += 1;
    let h = crate::castle::houses()[i];
    let v = Zone::Village;
    let flip = if h.dress() < 2 { 1.0 } else { -1.0 };
    let back = -h.half.y;
    let yard = crate::castle::YARD;
    let door = h.local(0.0, h.half.y + 0.55);
    w.doors.push((door, h.pos, h.yaw));
    let hi = Some(i as u8);
    // Sweeping the doorstep, just right of the door (clear of the porch bench and the pots/bucket).
    add_spot(&mut w, SpotKind::Sweep, h.local(0.5, h.half.y + 0.75), h.yaw + FRAC_PI_2, v, hi, 0);
    let g = h.local(-1.1 * flip, back - 0.85);
    add_spot(&mut w, SpotKind::Garden, g, h.yaw + PI, v, hi, 0);
    if h.dress() == 1 {
        add_spot(&mut w, SpotKind::Hens, h.local(1.6 * flip, back - yard + 2.75), h.yaw + PI, v, hi, 0);
        add_spot(&mut w, SpotKind::Wood, h.local(1.5 * flip, back - 1.75), h.yaw, v, hi, 0);
    }
    // A seat by the door: the house's own bench where its model has one, else a split-log bench
    // where nothing of the house stands (never one on top of the other).
    if let Some((c, yaw, face, top, offs)) = house_seats(h.piece) {
        add_seats(&mut w, h.local(c.x, c.y), h.yaw + yaw, h.yaw + face, top, offs, 0.0, v, 0);
        // The model's own porch bench is solid (folk stood inside it, on the sitters' laps).
        let half = if offs.len() > 1 { 0.55 } else { 0.48 };
        let bc = h.local(c.x, c.y);
        crate::blockers::add_obb(bc.x, bc.y, half, 0.17, h.yaw + yaw);
        debug!("village_life: house {i} ({}) own bench at {}", h.piece, h.local(c.x, c.y));
    } else if let Some((c, yaw, face)) = house_new_bench(h.piece) {
        add_bench(&mut commands, &mut w, h.local(c.x, c.y), h.yaw + yaw, h.yaw + face, v, 0);
    }
    // A man and a woman in every house; a child in every third, a grey elder in every fourth.
    let mut folk = vec![Body::Man, Body::Woman];
    if i % 3 == 1 {
        folk.push(Body::Kid);
    }
    if i % 4 == 2 {
        folk.push(Body::Elder);
    }
    for (k, b) in folk.into_iter().enumerate() {
        let seed = hash(&mut w.seed) ^ (i as u32 * 7919);
        let at = door + fwd(h.yaw) * 1.2 + fwd(h.yaw + FRAC_PI_2) * (k as f32 - 1.0) * 0.9;
        spawn_resident(&mut commands, &mut meshes, &mut creature_mats, b, at, v, (door, h.pos, h.yaw), hi, seed);
    }
}

// ── Hand props ───────────────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct PropAssets {
    parts: Vec<(Held, Handle<Mesh>, Handle<StandardMaterial>, Joint, Transform)>,
}

fn prop_assets(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) -> PropAssets {
    let mat = |mats: &mut Assets<StandardMaterial>, hex: u32, rough: f32, metal: f32| {
        mats.add(StandardMaterial {
            base_color: Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8),
            perceptual_roughness: rough,
            metallic: metal,
            ..default()
        })
    };
    let wood = mat(mats, 0x6e4c2c, 0.85, 0.0);
    let straw = mat(mats, 0xa88e52, 0.95, 0.0);
    let bread = mat(mats, 0xa8692e, 0.9, 0.0);
    let clay = mat(mats, 0x7c5236, 0.7, 0.0);
    let cloth = mat(mats, 0x9a8862, 0.95, 0.0);
    let wicker = mat(mats, 0x8a6a3c, 0.9, 0.0);
    let iron = mat(mats, 0x4c4f53, 0.45, 0.7);
    let hand = |x: f32, y: f32, z: f32| Transform::from_xyz(x, y, z);
    // Elbow-joint frame: the forearm runs down local −Y, the fist sits at y −0.25.
    let parts = vec![
        (Held::Bread, meshes.add(Capsule3d::new(0.042, 0.11)), bread.clone(), Joint::ElbowR, hand(0.0, -0.29, 0.03).with_rotation(Quat::from_rotation_z(FRAC_PI_2))),
        (Held::Mug, meshes.add(Cylinder::new(0.042, 0.11)), clay.clone(), Joint::ElbowR, hand(0.0, -0.28, 0.06)),
        // Broom: the handle continues the forearm line down to the floor; the head at its end.
        (Held::Broom, meshes.add(Cylinder::new(0.017, 1.35)), wood.clone(), Joint::ElbowR, hand(0.0, -0.62, 0.0)),
        (Held::Broom, meshes.add(Cone { radius: 0.10, height: 0.32 }), straw.clone(), Joint::ElbowR, hand(0.0, -1.28, 0.0)),
        (Held::Bucket, meshes.add(Cylinder::new(0.12, 0.22)), wood.clone(), Joint::ElbowR, hand(0.0, -0.42, 0.0)),
        (Held::Basket, meshes.add(Cylinder::new(0.15, 0.15)), wicker.clone(), Joint::ElbowL, hand(0.0, -0.14, -0.13).with_rotation(Quat::from_rotation_x(FRAC_PI_2))),
        (Held::Sack, meshes.add(Sphere::new(0.16)), cloth.clone(), Joint::Torso, Transform { translation: Vec3::new(0.16, 0.42, -0.04), scale: Vec3::new(0.9, 0.75, 1.7), ..default() }),
        (Held::Hammer, meshes.add(Cuboid::new(0.03, 0.03, 0.30)), wood.clone(), Joint::ElbowR, hand(0.0, -0.27, 0.12)),
        (Held::Hammer, meshes.add(Cuboid::new(0.05, 0.12, 0.05)), iron.clone(), Joint::ElbowR, hand(0.0, -0.27, 0.27)),
        (Held::Saw, meshes.add(Cuboid::new(0.008, 0.11, 0.55)), iron, Joint::ElbowR, hand(0.0, -0.30, 0.30)),
        // A wooden practice sword (waster): blade along the fist's forward line + a short guard.
        (Held::Waster, meshes.add(Cuboid::new(0.022, 0.045, 0.78)), wood.clone(), Joint::ElbowR, hand(0.0, -0.27, 0.43)),
        (Held::Waster, meshes.add(Cuboid::new(0.16, 0.03, 0.03)), wood.clone(), Joint::ElbowR, hand(0.0, -0.27, 0.05)),
        (Held::Stick, meshes.add(Cylinder::new(0.016, 0.16)), wood, Joint::ElbowL, hand(0.0, -0.28, 0.03).with_rotation(Quat::from_rotation_x(FRAC_PI_2))),
    ];
    let _ = (clay, cloth);
    PropAssets { parts }
}

/// Marks a resident whose hand props were moved onto its realistic skin's hand bones.
#[derive(Component)]
struct PropsOnSkin;

/// Residents with a realistic skin hide the studio rig, props and all: move each hand-prop
/// carrier onto the skin's hand bone (the sack rides the hidden torso; carrying uses the
/// carry clip instead).
fn props_to_skin(
    mut commands: Commands,
    q: Query<(Entity, &LifeProps, &crate::npc_skin::Skinned), Without<PropsOnSkin>>,
) {
    for (e, pr, sk) in &q {
        let Some((hr, hl)) = crate::npc_skin::hands(sk) else { continue };
        for &(h, c) in &pr.items {
            let right = match h {
                Held::None | Held::Sack => continue,
                Held::Basket | Held::Stick => false,
                _ => true,
            };
            commands.entity(if right { hr } else { hl }).add_child(c);
            commands.entity(c).try_insert(crate::npc_skin::elbow_to_hand(right));
        }
        commands.entity(e).try_insert(PropsOnSkin);
    }
}

/// Give every new resident its hand props once its rig exists (hidden until an act shows them).
#[allow(clippy::type_complexity)]
fn life_props(
    mut commands: Commands,
    mut assets: Local<Option<PropAssets>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    pending: Query<Entity, (With<LifeAgent>, Without<LifeProps>)>,
    parts: Query<(Entity, &BipedPart)>,
    mut found: Local<Vec<(Entity, Joint, Entity)>>,
) {
    if pending.is_empty() {
        return;
    }
    let a = assets.get_or_insert_with(|| prop_assets(&mut meshes, &mut mats));
    found.clear();
    for (e, p) in &parts {
        if matches!(p.joint, Joint::ElbowR | Joint::ElbowL | Joint::Torso) && pending.contains(p.root) {
            found.push((p.root, p.joint, e));
        }
    }
    for root in &pending {
        let joint = |j: Joint| found.iter().find(|(r, jj, _)| *r == root && *jj == j).map(|f| f.2);
        let (Some(er), Some(el), Some(to)) = (joint(Joint::ElbowR), joint(Joint::ElbowL), joint(Joint::Torso)) else { continue };
        let mut items = [(Held::None, Entity::PLACEHOLDER); 10];
        let mut n = 0;
        // Group meshes per Held under one carrier entity per joint.
        let mut carriers: Vec<(Held, Joint, Entity)> = Vec::new();
        for (held, mesh, mat, j, tf) in &a.parts {
            let parent = match j {
                Joint::ElbowL => el,
                Joint::Torso => to,
                _ => er,
            };
            let carrier = match carriers.iter().find(|(h, jj, _)| h == held && jj == j) {
                Some(c) => c.2,
                None => {
                    let c = commands.spawn((Transform::default(), Visibility::Hidden)).id();
                    commands.entity(parent).add_child(c);
                    carriers.push((*held, *j, c));
                    if n < items.len() {
                        items[n] = (*held, c);
                        n += 1;
                    }
                    c
                }
            };
            let m = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), *tf)).id();
            commands.entity(carrier).add_child(m);
        }
        commands.entity(root).insert(LifeProps { items, shown: Held::None });
    }
}

// ── The brain ────────────────────────────────────────────────────────────────────────────

/// Where the resident's ROOT goes to sit on a seat spot, and where it stands to start sitting.
fn seat_root(s: &Spot, scale: f32) -> (Vec2, Vec2) {
    let f = fwd(s.face);
    let root = s.pos + f * 0.04 * scale;
    (root, root + f * clip::RISE_DZ * scale)
}

/// The spot's approach point (where the root walks to before the act starts).
fn approach(s: &Spot, scale: f32) -> Vec2 {
    if s.kind == SpotKind::Seat {
        seat_root(s, scale).1
    } else {
        s.pos
    }
}

fn act_for(kind: SpotKind, body: Body, rng: &mut u32) -> (Act, f32, Held) {
    match kind {
        SpotKind::Well => (Act::Draw, rr(rng, 5.5, 8.0), Held::None),
        SpotKind::Laundry => (Act::Hang, rr(rng, 8.0, 14.0), Held::None),
        SpotKind::Sweep => (Act::Sweep, rr(rng, 8.0, 14.0), Held::Broom),
        SpotKind::Hens => (Act::Scatter, rr(rng, 6.0, 10.0), Held::Basket),
        SpotKind::Garden => (Act::Weed, rr(rng, 9.0, 16.0), Held::None),
        SpotKind::Wood => {
            if r01(rng) < 0.5 {
                (Act::Saw, rr(rng, 7.0, 12.0), Held::Saw)
            } else {
                (Act::Hammer, rr(rng, 6.0, 10.0), Held::Hammer)
            }
        }
        SpotKind::Fire => (if body == Body::Kid { Act::Stand } else { Act::Warm }, rr(rng, 8.0, 15.0), Held::None),
        SpotKind::Haul => (Act::Lift, 2.6, Held::None),
        SpotKind::Spar => (Act::Spar, rr(rng, 14.0, 24.0), Held::Waster),
        SpotKind::Play | SpotKind::Seat => (Act::Stand, 3.0, Held::None),
    }
}

/// The resident's chore wish-list for this time of day (weights).
fn chores(body: Body, phase: Phase, zone: Zone) -> &'static [(SpotKind, f32)] {
    use SpotKind::*;
    match (body, phase) {
        (Body::Kid, _) => &[(Play, 1.0)],
        (Body::Elder, _) => &[(Seat, 4.0), (Fire, 1.0)],
        // The keep's own folk keep a little work going through the noon meal (the drill, the cart).
        (Body::Man, Phase::Noon) if zone == Zone::Courtyard => &[(Seat, 2.6), (Fire, 0.5), (Spar, 1.4), (Haul, 1.0)],
        (Body::Woman, Phase::Noon) if zone == Zone::Courtyard => &[(Seat, 3.0), (Well, 1.0), (Laundry, 0.6)],
        (_, Phase::Noon) => &[(Seat, 5.0), (Fire, 0.6)],
        // The keep's own men: the cart and the hay to unload into the keep, the guard drill.
        (Body::Man, _) if zone == Zone::Courtyard => &[(Haul, 1.8), (Spar, 1.6), (Wood, 1.2), (Well, 0.4), (Fire, 0.4), (Seat, 0.3)],
        (Body::Woman, _) if zone == Zone::Courtyard => &[(Well, 1.4), (Sweep, 1.0), (Laundry, 1.0), (Haul, 0.6), (Garden, 0.8), (Seat, 0.5)],
        (Body::Woman, _) => &[(Well, 1.4), (Sweep, 1.2), (Hens, 1.0), (Garden, 1.0), (Laundry, 1.2), (Seat, 0.5)],
        (Body::Man, _) => &[(Wood, 1.6), (Garden, 1.0), (Well, 0.6), (Fire, 0.5), (Seat, 0.4)],
    }
}

/// Find a free spot of `kind` for this resident: its own house's first, else the nearest in its
/// zone (a little randomness so the same bench isn't always picked).
fn find_spot(w: &LifeWorld, a: &LifeAgent, pos: Vec2, kind: SpotKind, rng: &mut u32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, s) in w.spots.iter().enumerate() {
        if s.kind != kind || s.taken.is_some() || s.zone != a.zone || s.need_houses > w.houses {
            continue;
        }
        if s.house.is_some() && s.house != a.house && kind != SpotKind::Seat {
            continue; // someone else's garden / hens / doorstep
        }
        if kind == SpotKind::Seat {
            // A seat whose step-in point a prop now covers (a table, a bench, a new building)
            // can't be reached without walking into it — skip it.
            let p = approach(s, a.root_scale);
            if crate::blockers::any_within(p.x, p.y, 0.08) {
                continue;
            }
        }
        let own = if s.house.is_some() && s.house == a.house { -12.0 } else { 0.0 };
        // A sparring partner already waits on the other mark → join him.
        let own = own + if kind == SpotKind::Spar && w.spots.iter().any(|o| o.kind == SpotKind::Spar && o.bench == s.bench && o.taken.is_some()) { -20.0 } else { 0.0 };
        let d = s.pos.distance(pos) + own + rr(rng, 0.0, 6.0);
        if best.is_none_or(|b| d < b.1) {
            best = Some((i, d));
        }
    }
    best.map(|b| b.0)
}

fn release(w: &mut LifeWorld, a: &mut LifeAgent, me: Entity) {
    if let Some(i) = a.spot.take() {
        if let Some(s) = w.spots.get_mut(i) {
            if s.taken == Some(me) {
                s.taken = None;
            }
        }
    }
}

/// Body radius the walk planner keeps clear of props (a person's shoulders, plus a hand).
const WALK_R: f32 = 0.3;

/// Plan a walk to `goal`: always an A* route (props and buildings are in the grid), then
/// string-pulled into a few straight legs, so short trips go *around* a cauldron or a cart instead
/// of greedily into it. A free-form goal (a stroll, a meeting point) is first moved off any prop;
/// spot / door goals are exact and left alone (their approach points are checked when claimed).
fn walk_to(a: &mut LifeAgent, from: Vec2, goal: Vec2, after: After, now: f32) {
    let goal = match after {
        After::Spot | After::Enter => goal,
        // body (0.22) + the arrival slack (0.18): a walker stopping short of the goal still clears.
        _ => crate::navgrid::free_point(goal, 0.42),
    };
    a.goal = goal;
    a.after = after;
    a.path.clear();
    a.path_i = 0;
    let mut raw = crate::navgrid::path_to(from, goal);
    raw.push(goal);
    a.path.extend(crate::navgrid::smooth_path(from, &raw, WALK_R * 0.85));
    a.st = St::Walk;
    a.t0 = now;
    a.until = now + 30.0 + from.distance(goal) * 1.5;
    a.stall = 0.0;
}

/// A walk that can't finish (boxed in, or out of time): never teleport the walker — drop the
/// errand where they stand and think again. Going home is the exception only when the door is
/// right there (stepping in hides them anyway).
fn give_up(w: &mut LifeWorld, a: &mut LifeAgent, me: Entity, pos: Vec2, now: f32) -> bool {
    let close = pos.distance(a.goal) < 1.2;
    if close && matches!(a.after, After::Enter | After::Spot) {
        return false; // close enough: let `arrive` settle it
    }
    release(w, a, me);
    a.after = After::Nothing;
    a.partner = None;
    a.want_chat = false;
    a.replans = 0;
    a.st = St::Idle;
    a.t0 = now;
    a.until = now + rr(&mut a.rng, 1.0, 3.0);
    true
}

fn go_home(a: &mut LifeAgent, from: Vec2, now: f32) {
    let door = a.home.0;
    walk_to(a, from, door, After::Enter, now);
}

/// Speak a life line at `pos` if the knight is in earshot and the street isn't mid-line.
/// How far (world u) the knight hears the residents talk (was 15: the town read as near-mute).
const HEAR: f32 = 19.0;

fn say(talk: &mut LifeTalk, speak: &mut MessageWriter<Speak>, now: f32, hero: Vec2, pos: Vec2, y: f32, voice: LifeVoice, cue: LifeCue, gap: f32) -> bool {
    if now < talk.next || pos.distance(hero) > HEAR {
        return false;
    }
    speak.write(Speak::at(Concept::Life(voice, cue), Vec3::new(pos.x, y + 1.5, pos.y)));
    talk.next = now + gap;
    true
}

fn sfx(commands: &mut Commands, bank: &LifeSfx, talk: &mut LifeTalk, cfg: &crate::audio::AudioConfig, now: f32, hero: Vec2, key: &str, pos: Vec3) {
    if now < talk.sfx_next || pos.xz().distance(hero) > 22.0 {
        return;
    }
    let takes: Vec<_> = bank.0.iter().filter(|(k, _)| *k == key).map(|(_, h)| h).collect();
    if takes.is_empty() {
        return;
    }
    let h = takes[(now * 7919.0) as usize % takes.len()];
    talk.sfx_next = now + 0.25;
    commands.spawn((
        AudioPlayer(h.clone()),
        PlaybackSettings { mode: PlaybackMode::Despawn, volume: Volume::Linear(cfg.sfx_vol * 0.8), spatial: true, ..default() },
        Transform::from_translation(pos),
    ));
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn life_brain(
    time: Res<Time>,
    mut commands: Commands,
    clock: Option<Res<crate::scene::SkyClock>>,
    siege: Option<Res<crate::siege::Siege>>,
    hero_q: Query<&crate::player::Hero>,
    mut w: ResMut<LifeWorld>,
    mut stalls: ResMut<Stalls>,
    mut talk: ResMut<LifeTalk>,
    bank: Res<LifeSfx>,
    cfg: Res<crate::audio::AudioConfig>,
    mut speak: MessageWriter<Speak>,
    mut q: Query<(Entity, &mut LifeAgent, &mut Villager, &mut Transform, &mut Visibility), Without<crate::dying::Dying>>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs().min(0.05);
    let wave = siege.as_ref().is_some_and(|s| s.phase == crate::siege::GamePhase::Wave);
    let phase = phase_of(clock.as_ref().map_or(0.25, |c| c.t), wave);
    let (hero, hero_alive) = hero_q.single().map(|h| (h.pos, true)).unwrap_or((Vec2::splat(1e6), false));
    let w = &mut *w;
    // Everyone out on the street, for personal-space steering (sitters / indoors don't count).
    let others: Vec<(Entity, Vec2, bool)> = q
        .iter()
        .filter(|(_, a, ..)| !matches!(a.st, St::Indoors))
        .map(|(e, a, v, ..)| (e, v.pos, matches!(a.st, St::SitDown | St::Sit(_) | St::StandUp)))
        .collect();

    for (me, mut a, mut v, mut tf, mut vis) in &mut q {
        let a = &mut *a;
        if !a.inited {
            a.inited = true;
            // Match the walk clip's cadence to this person's pace (elders shuffle, kids run).
            if a.body != Body::Kid {
                v.gait = 8.0 * v.speed / 1.4;
            }
            a.face = v.facing;
        }
        let pos = v.pos;
        let gy = crate::steer::footing(pos.x, pos.y).unwrap_or(tf.translation.y);
        let night = matches!(phase, Phase::Night);
        let home_time = night || (phase == Phase::Dusk);

        // ── Queued reply line (the second half of an exchange) ──
        if let Some(i) = a.vo.iter().position(|(at, _)| now >= *at) {
            let (_, cue) = a.vo.swap_remove(i);
            // Replies skip the gap the opener set, but still need the knight in earshot.
            if pos.distance(hero) < HEAR {
                speak.write(Speak::at(Concept::Life(a.body.voice(), cue), Vec3::new(pos.x, gy + 1.5, pos.y)));
                talk.next = talk.next.max(now + 3.0);
            }
        }

        // ── Bedtime: drop whatever is going on and head home (stand up first if seated) ──
        if home_time && !matches!(a.st, St::Indoors | St::SitDown | St::StandUp) && a.after != After::Enter {
            match a.st {
                St::Sit(_) => {
                    a.st = St::StandUp;
                    a.t0 = now;
                }
                _ => {
                    release(w, a, me);
                    if let Some(i) = stalls.buyer.iter().position(|b| *b == Some(me)) {
                        stalls.buyer[i] = None;
                    }
                    a.partner = None;
                    a.carry = Carry::None;
                    a.held = Held::None;
                    // A word as they go: the dusk call, a mother for her child.
                    if phase == Phase::Dusk && r01(&mut a.rng) < 0.35 {
                        let cue = if a.body == Body::Woman && r01(&mut a.rng) < 0.5 { LifeCue::CallKid } else { LifeCue::Dusk };
                        say(&mut talk, &mut speak, now, hero, pos, gy, a.body.voice(), cue, 6.0);
                    }
                    go_home(a, pos, now);
                }
            }
        }

        // ── Hero awareness: heads turn; standing folk wave or bow once as he passes ──
        let hd = pos.distance(hero);
        let watching = hero_alive && hd < 7.0 && !matches!(a.st, St::Indoors | St::Chat);
        let want_look = if watching { crate::steer::wrap_pi(yaw_to(pos, hero) - v.facing).clamp(-1.1, 1.1) } else { 0.0 };
        a.look += crate::steer::wrap_pi(want_look - a.look) * (dt * 4.0).min(1.0);
        if hero_alive && hd < 4.2 && now > a.greet_cd && matches!(a.st, St::Idle | St::Walk) && a.body != Body::Kid && a.carry == Carry::None {
            a.greet_cd = now + rr(&mut a.rng, 70.0, 120.0);
            let act = if a.body == Body::Man && r01(&mut a.rng) < 0.6 { Act::Wave } else { Act::Bow };
            // Resume the walk afterwards (the path is kept).
            a.st = St::Greet(act);
            a.t0 = now;
            a.until = now + 1.9;
            a.face = yaw_to(pos, hero);
            say(&mut talk, &mut speak, now, hero, pos, gy, a.body.voice(), LifeCue::Greet, 5.0);
        }
        // ── Neighbours: passing someone close ahead, a nod or a raised hand, then on their way ──
        if a.st == St::Walk && now > a.nod_cd && a.body != Body::Kid && a.carry == Carry::None {
            let ahead = fwd(v.facing);
            let met = others.iter().find(|(oe, op, _)| {
                let d = *op - pos;
                let l = d.length();
                *oe != me && Some(*oe) != a.partner && l > 0.9 && l < 2.6 && d.dot(ahead) > 0.6 * l
            });
            if let Some(&(_, op, _)) = met {
                a.nod_cd = now + rr(&mut a.rng, 25.0, 50.0);
                if r01(&mut a.rng) < 0.45 {
                    let act = if r01(&mut a.rng) < 0.65 { Act::Nod } else { Act::Wave };
                    a.st = St::Greet(act);
                    a.t0 = now;
                    a.until = now + 1.5;
                    a.face = yaw_to(pos, op);
                }
            }
        }
        // Kids drop their game to come and stare at the knight.
        if a.body == Body::Kid && hero_alive && hd < 9.0 && hd > 2.6 && now > a.stare_cd && matches!(a.st, St::Play | St::Idle) && !home_time {
            a.stare_cd = now + rr(&mut a.rng, 45.0, 80.0);
            let to = hero + (pos - hero).normalize_or(Vec2::X) * 2.3;
            walk_to(a, pos, to, After::Stare, now);
        }

        // ── State machine ──
        match a.st {
            St::Indoors => {
                v.moving = false;
                if !home_time && now >= a.until {
                    // Out of the door: a stretch and a yawn in the morning.
                    v.pos = a.home.0;
                    v.facing = a.home.2;
                    a.face = a.home.2;
                    a.st = if phase == Phase::Dawn || r01(&mut a.rng) < 0.25 { St::Do(Act::Yawn) } else { St::Idle };
                    a.t0 = now;
                    a.until = now + if a.st == St::Idle { 0.5 } else { 3.2 };
                    let p = v.pos;
                    sfx(&mut commands, &bank, &mut talk, &cfg, now, hero, "life_sfx_door", Vec3::new(p.x, gy + 1.0, p.y));
                    if a.st != St::Idle && r01(&mut a.rng) < 0.5 {
                        say(&mut talk, &mut speak, now, hero, p, gy, a.body.voice(), LifeCue::Yawn, 6.0);
                    }
                }
            }
            St::Walk => {
                let target = a.path.get(a.path_i).copied().unwrap_or(a.goal);
                let last = a.path_i + 1 >= a.path.len();
                let reach = if last { 0.18 } else { 0.6 };
                let out_of_time = now > a.until || a.stall > 1.6 && a.replans >= 2;
                if out_of_time {
                    v.moving = false;
                    if !give_up(w, a, me, v.pos, now) {
                        a.replans = 0;
                        arrive(a, &mut v, w, &mut stalls, me, now, phase, hero, gy, &mut talk, &mut speak, &mut commands, &bank, &cfg);
                    }
                } else if v.pos.distance(target) < reach {
                    if !last {
                        a.path_i += 1;
                    } else {
                        v.moving = false;
                        a.replans = 0;
                        arrive(a, &mut v, w, &mut stalls, me, now, phase, hero, gy, &mut talk, &mut speak, &mut commands, &bank, &cfg);
                    }
                } else if a.stall > 1.6 {
                    // Wedged: re-plan from here (the route may have been cut by a new prop or a
                    // crowd), at most twice before the errand is dropped.
                    a.replans += 1;
                    let (goal, after, until) = (a.goal, a.after, a.until);
                    walk_to(a, v.pos, goal, after, now);
                    a.until = until;
                } else {
                    let near = v.pos.distance(hero) < crate::steer::LOD_R;
                    let speed = if a.body == Body::Kid && a.after == After::Nothing && a.it { v.speed * 1.25 } else { v.speed };
                    // Personal space: someone close ahead → bear right round them (both walkers
                    // keep right, so they pass like people); someone right on top → wait a beat.
                    let dir = (target - v.pos).normalize_or_zero();
                    let mut aim = target;
                    let mut pace: f32 = 1.0;
                    for &(oe, op, seated) in &others {
                        if oe == me || Some(oe) == a.partner {
                            continue;
                        }
                        let d = op - v.pos;
                        let dist = d.length();
                        // A sitter's knees and feet stick out in front of the bench: give anyone
                        // seated a wide berth even when merely passing beside them.
                        if seated && dist < 1.0 && dist > 1e-3 && op.distance(target) > 0.6 {
                            aim -= d / dist * (1.0 - dist) * 2.5;
                        }
                        if dist > 1.6 || dist < 1e-3 {
                            continue;
                        }
                        let ahead = d.dot(dir) / dist;
                        if ahead < 0.35 {
                            continue;
                        }
                        let right = Vec2::new(dir.y, -dir.x);
                        let push = (1.6 - dist) / 1.6;
                        aim += right * (1.2 * push) * v.pos.distance(target).min(2.0);
                        if dist < 0.75 && ahead > 0.8 {
                            pace = pace.min(0.25);
                        }
                    }
                    let step = speed * pace * dt;
                    let s = if near {
                        crate::steer::advance_person(v.pos, v.facing, aim, step, v.body_r.max(0.28), gy, 3.0 * dt)
                    } else {
                        Some(crate::steer::advance_direct(v.pos, v.facing, aim, step, v.body_r, gy, 3.0 * dt))
                    };
                    match s {
                        Some(s) => {
                            v.facing = s.facing;
                            v.pos = s.pos;
                            v.moving = s.moving && pace > 0.3;
                            a.stall = if s.moving { (a.stall - dt).max(0.0) } else { a.stall + dt };
                        }
                        None => {
                            // Boxed in: turn toward an opening and count it.
                            v.moving = false;
                            v.facing += 0.6 * dt * 3.0;
                            a.stall += dt;
                        }
                    }
                }
            }
            St::Idle => {
                v.moving = false;
                if now >= a.until {
                    a.want_chat = false;
                    think(a, &v, w, &mut stalls, me, now, phase);
                }
            }
            St::SitDown => {
                v.moving = false;
                if now - a.t0 >= 1.5 {
                    let act = pick_sit_act(a.body, phase, &mut a.rng);
                    a.st = St::Sit(act);
                    a.t0 = now;
                    a.until = now + if a.body == Body::Elder { rr(&mut a.rng, 30.0, 60.0) } else { rr(&mut a.rng, 16.0, 30.0) };
                    a.held = match act {
                        SitAct::Eat => Held::Bread,
                        SitAct::Drink => Held::Mug,
                        SitAct::Whittle => Held::Stick,
                        _ => Held::None,
                    };
                    let p = Vec3::new(v.pos.x, gy + 0.5, v.pos.y);
                    let key = match act {
                        SitAct::Eat => "life_sfx_bread",
                        SitAct::Drink => "life_sfx_mug",
                        SitAct::Whittle => "life_sfx_whittle",
                        _ => "life_sfx_bench_sit",
                    };
                    sfx(&mut commands, &bank, &mut talk, &cfg, now, hero, key, p);
                    let cue = match (a.body, act) {
                        (Body::Elder, SitAct::Doze) => LifeCue::Doze,
                        (Body::Elder, SitAct::Whittle) => LifeCue::Whittle,
                        (Body::Elder, _) => LifeCue::Cough,
                        (_, SitAct::Eat) => LifeCue::Eat,
                        _ => LifeCue::Sit,
                    };
                    if r01(&mut a.rng) < 0.5 {
                        say(&mut talk, &mut speak, now, hero, v.pos, gy, a.body.voice(), cue, 6.0);
                    }
                }
            }
            St::Sit(act) => {
                v.moving = false;
                if now >= a.until {
                    a.st = St::StandUp;
                    a.t0 = now;
                    a.held = Held::None;
                    sfx(&mut commands, &bank, &mut talk, &cfg, now, hero, "life_sfx_bench_stand", Vec3::new(v.pos.x, gy + 0.5, v.pos.y));
                    if r01(&mut a.rng) < 0.35 {
                        say(&mut talk, &mut speak, now, hero, v.pos, gy, a.body.voice(), LifeCue::Stand, 6.0);
                    }
                } else if matches!(act, SitAct::Talk | SitAct::Laugh) && a.partner.is_none() && now - a.t0 > 6.0 {
                    // The neighbour left: settle back.
                    a.st = St::Sit(SitAct::Rest);
                }
            }
            St::StandUp => {
                v.moving = false;
                if now - a.t0 >= 1.5 {
                    // The hips stood RISE_DZ ahead of the seated root: move the root there.
                    let step = fwd(v.facing) * clip::RISE_DZ * a.root_scale;
                    v.pos += step;
                    a.shift -= clip::RISE_DZ;
                    release(w, a, me);
                    a.partner = None;
                    a.st = St::Idle;
                    a.t0 = now;
                    a.until = now + rr(&mut a.rng, 0.5, 2.0);
                }
            }
            St::Do(act) => {
                v.moving = false;
                if act == Act::Stare {
                    a.face = yaw_to(v.pos, hero);
                }
                if now >= a.until {
                    finish_act(a, &v, w, me, now, act, hero, gy, &mut talk, &mut speak, &mut commands, &bank, &cfg);
                }
            }
            St::Greet(_) => {
                v.moving = false;
                if now >= a.until {
                    if a.path_i < a.path.len() {
                        a.st = St::Walk;
                        a.until = now + 30.0 + v.pos.distance(a.goal) * 1.5;
                        a.stall = 0.0;
                    } else {
                        a.st = St::Idle;
                        a.until = now + 0.5;
                    }
                    a.t0 = now;
                }
            }
            St::Chat => {
                v.moving = false;
                if now >= a.until || a.partner.is_none() {
                    a.partner = None;
                    a.st = St::Idle;
                    a.t0 = now;
                    a.until = now + rr(&mut a.rng, 0.5, 2.0);
                }
            }
            St::Buy(stage) => {
                v.moving = false;
                let i = a.spot.unwrap_or(0).min(3);
                a.face = stalls.slots.get(i).map_or(a.face, |s| s.1);
                if now >= a.until {
                    let (voice, vp) = (a.body.voice(), v.pos);
                    match stage {
                        0 => {
                            // Ask the price; the vendor names it, the buyer balks, then pays.
                            a.st = St::Buy(1);
                            a.until = now + rr(&mut a.rng, 6.5, 8.0);
                            stalls.cue[i] = (Act::Haggle, a.until);
                            if say(&mut talk, &mut speak, now, hero, vp, gy, voice, LifeCue::Ask, 2.4) {
                                a.vo.push((now + 5.0, LifeCue::Haggle));
                            }
                        }
                        1 => {
                            a.st = St::Buy(2);
                            a.until = now + 2.4;
                            stalls.cue[i] = (Act::Pay, a.until);
                            sfx(&mut commands, &bank, &mut talk, &cfg, now, hero, "life_sfx_coins", Vec3::new(vp.x, gy + 1.0, vp.y));
                            say(&mut talk, &mut speak, now, hero, vp, gy, voice, LifeCue::Agree, 3.0);
                        }
                        _ => {
                            stalls.buyer[i] = None;
                            stalls.cue[i] = (Act::Stand, now);
                            a.spot = None;
                            a.carry = if a.body == Body::Man || r01(&mut a.rng) < 0.3 { Carry::Sack } else { Carry::Basket };
                            a.held = if a.carry == Carry::Sack { Held::Sack } else { Held::Basket };
                            let key = if a.carry == Carry::Sack { "life_sfx_sack" } else { "life_sfx_basket" };
                            sfx(&mut commands, &bank, &mut talk, &cfg, now, hero, key, Vec3::new(vp.x, gy + 1.0, vp.y));
                            go_home(a, vp, now);
                        }
                    }
                    a.t0 = now;
                }
            }
            St::Play => {
                // The play brain lives in `life_social` (it needs the other kids); here just the
                // timeout back to a fresh think.
                if now >= a.until {
                    a.st = St::Idle;
                    a.until = now;
                }
            }
        }

        // A busy resident turns to its act's facing (seat, counter, partner, the knight).
        if matches!(a.st, St::SitDown | St::Sit(_) | St::Do(_) | St::Chat | St::Greet(_) | St::Buy(_)) {
            v.facing += crate::steer::wrap_pi(a.face - v.facing) * (dt * 7.0).min(1.0);
        }

        // ── Visibility + transform ──
        let want = if a.st == St::Indoors { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
        let bob = if v.moving { ((now + v.phase) * v.gait).sin().abs() * v.bob } else { 0.0 };
        let gy = crate::steer::footing(v.pos.x, v.pos.y).unwrap_or(gy);
        // Only on a real change (a sitter / standing chatter holds still).
        crate::steer::set_pose(&mut tf, Vec3::new(v.pos.x, gy + bob, v.pos.y), Quat::from_rotation_y(v.facing));
    }
}

fn pick_sit_act(body: Body, phase: Phase, rng: &mut u32) -> SitAct {
    let x = r01(rng);
    match (body, phase) {
        (Body::Elder, _) => {
            if x < 0.4 {
                SitAct::Whittle
            } else if x < 0.75 {
                SitAct::Doze
            } else {
                SitAct::Rest
            }
        }
        (_, Phase::Noon) => {
            if x < 0.55 {
                SitAct::Eat
            } else if x < 0.9 {
                SitAct::Drink
            } else {
                SitAct::Rest
            }
        }
        _ => {
            if x < 0.3 {
                SitAct::Drink
            } else if x < 0.5 {
                SitAct::Eat
            } else {
                SitAct::Rest
            }
        }
    }
}

/// Choose the next thing to do (only when a timer runs out — the time-slicing).
fn think(a: &mut LifeAgent, v: &Villager, w: &mut LifeWorld, stalls: &mut Stalls, me: Entity, now: f32, phase: Phase) {
    let pos = v.pos;
    a.t0 = now;
    if matches!(phase, Phase::Night | Phase::Dusk) {
        go_home(a, pos, now);
        return;
    }
    // Carrying the shopping / a bucket: take it home first.
    if a.carry != Carry::None {
        go_home(a, pos, now);
        return;
    }
    if a.body == Body::Kid {
        a.st = St::Play;
        a.until = now + rr(&mut a.rng, 25.0, 50.0);
        return;
    }
    let x = r01(&mut a.rng);
    // A chat with a neighbour (paired up by `life_social`).
    let chatty = match phase {
        Phase::Afternoon => 0.26,
        Phase::Noon => 0.14,
        _ => 0.15,
    };
    if a.body != Body::Kid && x < chatty {
        a.want_chat = true;
        a.st = St::Idle;
        a.until = now + 6.0;
        return;
    }
    // An errand to the market (village folk mostly; courtyard folk now and then).
    let market_p = if a.zone == Zone::Village { 0.22 } else { 0.06 };
    if a.body != Body::Elder && matches!(phase, Phase::Work | Phase::Afternoon) && r01(&mut a.rng) < market_p {
        if let Some(i) = (0..stalls.slots.len().min(4)).find(|i| stalls.buyer[*i].is_none() && (me.to_bits() as usize + *i) % 2 == 0)
            .or_else(|| (0..stalls.slots.len().min(4)).find(|i| stalls.buyer[*i].is_none()))
        {
            stalls.buyer[i] = Some(me);
            a.spot = Some(i);
            let slot = stalls.slots[i].0;
            walk_to(a, pos, slot, After::Buy(i as u8), now);
            return;
        }
    }
    // A chore / a sit from the wish-list.
    let list = chores(a.body, phase, a.zone);
    let total: f32 = list.iter().map(|c| c.1).sum();
    let mut pick = r01(&mut a.rng) * total;
    let mut kind = list[0].0;
    for (k, wgt) in list {
        if pick < *wgt {
            kind = *k;
            break;
        }
        pick -= wgt;
    }
    let mut rng = a.rng;
    let found = find_spot(w, a, pos, kind, &mut rng).or_else(|| find_spot(w, a, pos, SpotKind::Seat, &mut rng));
    a.rng = rng;
    if let Some(i) = found {
        w.spots[i].taken = Some(me);
        a.spot = Some(i);
        let s = &w.spots[i];
        let to = approach(s, a.root_scale);
        // A chore spot drawn against its prop (the woodpile, the coop, a bench) is worked from
        // beside it, never from inside it.
        let to = if s.kind == SpotKind::Seat { to } else { crate::navgrid::free_point(to, 0.42) };
        walk_to(a, pos, to, After::Spot, now);
        return;
    }
    // Nothing free: a stroll near home.
    let to = a.home.0 + fwd(r01(&mut a.rng) * TAU) * rr(&mut a.rng, 2.0, 6.0);
    walk_to(a, pos, to, After::Nothing, now);
}

#[allow(clippy::too_many_arguments)]
fn arrive(
    a: &mut LifeAgent,
    v: &mut Villager,
    w: &mut LifeWorld,
    stalls: &mut Stalls,
    me: Entity,
    now: f32,
    phase: Phase,
    hero: Vec2,
    gy: f32,
    talk: &mut LifeTalk,
    speak: &mut MessageWriter<Speak>,
    commands: &mut Commands,
    bank: &LifeSfx,
    cfg: &crate::audio::AudioConfig,
) {
    a.t0 = now;
    match a.after {
        After::Enter => {
            // In through the door (the shutters bang at dusk).
            let key = if matches!(phase, Phase::Dusk | Phase::Night) { "life_sfx_shutters" } else { "life_sfx_door_shut" };
            sfx(commands, bank, talk, cfg, now, hero, key, Vec3::new(v.pos.x, gy + 1.0, v.pos.y));
            v.pos = a.home.1;
            a.st = St::Indoors;
            a.carry = Carry::None;
            a.held = Held::None;
            a.after = After::Nothing;
            a.until = if matches!(phase, Phase::Dusk | Phase::Night) { now + 5.0 } else { now + rr(&mut a.rng, 6.0, 20.0) };
        }
        After::Spot => {
            a.after = After::Nothing;
            let Some(i) = a.spot else {
                a.st = St::Idle;
                a.until = now + 1.0;
                return;
            };
            let s = w.spots[i];
            // A drill partner who ran out of time short of his mark (usually slowed by the
            // personal-space steering around the man already sparring on the line) would start
            // swinging inside his partner: drop the drill instead of sparring off the mark.
            if s.kind == SpotKind::Spar && v.pos.distance(s.pos) > 0.35 {
                release(w, a, me);
                a.st = St::Idle;
                a.until = now + 1.0;
                return;
            }
            a.face = s.face;
            if s.kind == SpotKind::Seat {
                let (root, _) = seat_root(&s, a.root_scale);
                v.pos = root;
                a.shift += clip::RISE_DZ;
                // Hips height that rests the pelvis on the seat top (the pelvis sits ~0.12 rig
                // units under the hips joint).
                a.seat = s.seat_h / a.root_scale + 0.12 - a.rig_off;
                a.st = St::SitDown;
            } else if s.kind == SpotKind::Play {
                a.st = St::Play;
                a.until = now + 30.0;
            } else {
                let (act, dur, held) = act_for(s.kind, a.body, &mut a.rng);
                a.st = St::Do(act);
                a.until = now + dur;
                a.held = held;
                let key = match act {
                    Act::Draw => Some("life_sfx_well"),
                    Act::Sweep => Some("life_sfx_sweep"),
                    Act::Hang => Some("life_sfx_laundry"),
                    Act::Scatter => Some("life_sfx_grain"),
                    Act::Hammer => Some("life_sfx_hammer"),
                    Act::Saw => Some("life_sfx_saw"),
                    Act::Spar => Some("life_sfx_spar"),
                    Act::Lift => Some("life_sfx_sack"),
                    _ => None,
                };
                if let Some(k) = key {
                    sfx(commands, bank, talk, cfg, now, hero, k, Vec3::new(v.pos.x, gy + 1.0, v.pos.y));
                }
                let cue = match (a.body, act) {
                    (Body::Woman, Act::Sweep) => Some(LifeCue::Sweep),
                    (Body::Woman, Act::Hang) => Some(LifeCue::Laundry),
                    (_, Act::Hammer | Act::Saw | Act::Spar | Act::Lift) => Some(LifeCue::Work),
                    _ => None,
                };
                if let Some(c) = cue {
                    if r01(&mut a.rng) < 0.5 {
                        say(talk, speak, now, hero, v.pos, gy, a.body.voice(), c, 6.0);
                    }
                }
            }
        }
        After::Buy(i) => {
            a.after = After::Nothing;
            let i = i as usize;
            if let Some(s) = stalls.slots.get(i) {
                a.face = s.1;
            }
            a.spot = Some(i);
            a.st = St::Buy(0);
            a.until = now + rr(&mut a.rng, 2.0, 3.5);
            stalls.cue[i] = (Act::Stand, now);
            let _ = me;
        }
        After::Chat => {
            a.after = After::Nothing;
            if let Some(_p) = a.partner {
                a.st = St::Chat;
                a.until = now + rr(&mut a.rng, 12.0, 18.0);
            } else {
                a.st = St::Idle;
                a.until = now + 1.0;
            }
        }
        After::Stare => {
            a.after = After::Nothing;
            a.st = St::Do(Act::Stare);
            a.until = now + rr(&mut a.rng, 5.0, 9.0);
            a.face = yaw_to(v.pos, hero);
            say(talk, speak, now, hero, v.pos, gy, LifeVoice::Child, LifeCue::Knight, 5.0);
        }
        After::Nothing => {
            a.st = St::Idle;
            a.until = now + rr(&mut a.rng, 1.0, 3.5);
        }
    }
}

/// An act ran its course: what comes next (a full bucket goes home, the doorstep is swept…).
#[allow(clippy::too_many_arguments)]
fn finish_act(
    a: &mut LifeAgent,
    v: &Villager,
    w: &mut LifeWorld,
    me: Entity,
    now: f32,
    act: Act,
    hero: Vec2,
    gy: f32,
    talk: &mut LifeTalk,
    speak: &mut MessageWriter<Speak>,
    commands: &mut Commands,
    bank: &LifeSfx,
    cfg: &crate::audio::AudioConfig,
) {
    release(w, a, me);
    a.held = Held::None;
    a.t0 = now;
    match act {
        Act::Draw => {
            a.carry = Carry::Bucket;
            a.held = Held::Bucket;
            sfx(commands, bank, talk, cfg, now, hero, "life_sfx_bucket", Vec3::new(v.pos.x, gy + 0.6, v.pos.y));
            if a.body == Body::Woman && r01(&mut a.rng) < 0.5 {
                say(talk, speak, now, hero, v.pos, gy, a.body.voice(), LifeCue::Water, 6.0);
            }
            go_home(a, v.pos, now);
        }
        Act::Lift => {
            // The sack is up on the shoulder: into the keep with it.
            a.carry = Carry::Sack;
            a.held = Held::Sack;
            go_home(a, v.pos, now);
        }
        Act::Hammer | Act::Saw | Act::Spar if r01(&mut a.rng) < 0.3 => {
            say(talk, speak, now, hero, v.pos, gy, a.body.voice(), LifeCue::WorkDone, 6.0);
            a.st = St::Idle;
            a.until = now + 1.0;
        }
        _ => {
            a.st = St::Idle;
            a.until = now + rr(&mut a.rng, 0.4, 2.0);
        }
    }
}

// ── Social pass: chats, bench neighbours, kids at play ────────────────────────────────

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn life_social(
    time: Res<Time>,
    mut tick: Local<f32>,
    hero_q: Query<&crate::player::Hero>,
    w: Res<LifeWorld>,
    mut talk: ResMut<LifeTalk>,
    mut speak: MessageWriter<Speak>,
    voices: Res<crate::audio::director::VoiceManager>,
    mut q: Query<(Entity, &mut LifeAgent, &Villager), Without<crate::dying::Dying>>,
    mut scratch: Local<Vec<(Entity, Vec2, St, bool, Zone, bool, Option<usize>, Body)>>,
) {
    let now = time.elapsed_secs();
    let hero = hero_q.single().map(|h| h.pos).unwrap_or(Vec2::splat(1e6));
    run_dialogue(&mut talk, &mut speak, &voices, &mut q, now, hero);
    // Kids chase every frame-ish (cheap: a handful); the pairing pass every 0.5 s.
    scratch.clear();
    for (e, a, v) in &q {
        scratch.push((e, v.pos, a.st, a.want_chat, a.zone, a.it, a.spot, a.body));
    }

    // ── Kids: tag. `it` chases the nearest playing kid; the others keep away from it. ──
    for (e, mut a, v) in &mut q {
        if a.st != St::Play && !(a.st == St::Walk && a.after == After::Nothing && a.body == Body::Kid && a.it) {
            continue;
        }
        if a.st == St::Walk && now - a.t0 < 0.35 {
            continue;
        }
        let me_pos = v.pos;
        let play = w.play[if a.zone == Zone::Courtyard { 0 } else { 1 }];
        let mut near: Option<(Entity, Vec2, f32)> = None;
        let mut it_pos: Option<Vec2> = None;
        for &(o, p, st, _, z, it, _, body) in scratch.iter() {
            if o == e || z != a.zone || body != Body::Kid || !matches!(st, St::Play | St::Walk) {
                continue;
            }
            let d = p.distance(me_pos);
            if near.is_none_or(|n| d < n.2) {
                near = Some((o, p, d));
            }
            if it && d < 6.0 {
                it_pos = Some(p);
            }
        }
        let mut rng_v = a.rng;
        let rng = &mut rng_v;
        let target = if a.it {
            match near {
                Some((_, p, d)) if d < 0.75 => {
                    // Tagged! Swap: I flee now (the other kid becomes "it" on its own pass when it
                    // sees nobody else is).
                    a.it = false;
                    say(&mut talk, &mut speak, now, hero, me_pos, 0.0, LifeVoice::Child, LifeCue::Tag, 4.0);
                    me_pos + (me_pos - p).normalize_or(Vec2::X) * 3.0
                }
                Some((_, p, _)) => p,
                None => play + fwd(r01(rng) * TAU) * 2.0,
            }
        } else if let Some(ip) = it_pos {
            let away = (me_pos - ip).normalize_or(Vec2::Y);
            let mut t = me_pos + away * 3.0 + fwd(r01(rng) * TAU) * 0.8;
            if t.distance(play) > 7.0 {
                t = play + (t - play).normalize_or(Vec2::X) * 6.0;
            }
            if r01(rng) < 0.02 {
                say(&mut talk, &mut speak, now, hero, me_pos, 0.0, LifeVoice::Child, LifeCue::Run, 5.0);
            }
            t
        } else {
            // Nobody chasing: one of us becomes "it" (and counts it out loud).
            if r01(rng) < 0.01 {
                a.it = true;
                say(&mut talk, &mut speak, now, hero, me_pos, 0.0, LifeVoice::Child, LifeCue::Count, 5.0);
            }
            play + fwd(r01(rng) * TAU) * rr(rng, 1.0, 5.0)
        };
        a.rng = rng_v;
        let until = a.until;
        if a.st == St::Play || a.goal.distance(target) > 1.0 {
            walk_to(&mut a, me_pos, target, After::Nothing, now);
            a.until = until.max(now + 3.0);
            a.st = St::Walk;
        }
    }

    if now < *tick {
        return;
    }
    *tick = now + 0.5;

    // ── Pair up idle neighbours who want a chat ──
    let n = scratch.len();
    for i in 0..n {
        let (ea, pa, sta, wa, za, ..) = scratch[i];
        if !(wa && sta == St::Idle) {
            continue;
        }
        let mut best: Option<(usize, f32)> = None;
        for (j, &(eb, pb, stb, _, zb, _, _, bb)) in scratch.iter().enumerate() {
            if j == i || eb == ea || zb != za || bb == Body::Kid || !matches!(stb, St::Idle) {
                continue;
            }
            let d = pa.distance(pb);
            if d < 14.0 && best.is_none_or(|b| d < b.1) {
                best = Some((j, d));
            }
        }
        let Some((j, _)) = best else { continue };
        let eb = scratch[j].0;
        let pb = scratch[j].1;
        let Ok([(_, mut a, _), (_, mut b, _)]) = q.get_many_mut([ea, eb]) else { continue };
        let mid = (pa + pb) * 0.5;
        let dir = (pb - pa).normalize_or(Vec2::X);
        a.partner = Some(eb);
        b.partner = Some(ea);
        a.want_chat = false;
        b.want_chat = false;
        a.lead = true;
        b.lead = false;
        walk_to(&mut a, pa, mid - dir * 0.55, After::Chat, now);
        walk_to(&mut b, pb, mid + dir * 0.55, After::Chat, now);
        a.face = dir.x.atan2(dir.y);
        b.face = (-dir.x).atan2(-dir.y);
        scratch[i].2 = St::Walk;
        scratch[j].2 = St::Walk;
    }

    // ── Chat beats: the lead opens with a line, the partner answers a beat later ──
    for i in 0..n {
        let (ea, pa, sta, ..) = scratch[i];
        if sta != St::Chat {
            continue;
        }
        let Ok((_, a, _)) = q.get(ea) else { continue };
        let (partner, lead, started) = (a.partner, a.lead, a.t0);
        let Some(pe) = partner else { continue };
        let Some(&(_, pb, stb, ..)) = scratch.iter().find(|s| s.0 == pe) else {
            if let Ok((_, mut a, _)) = q.get_mut(ea) {
                a.partner = None;
            }
            continue;
        };
        if !(lead && stb == St::Chat && now - started < 0.6 && pa.distance(hero) < HEAR && now >= talk.next && talk.run.is_none()) {
            continue;
        }
        let Ok([(_, mut a, _), (_, mut b, _)]) = q.get_many_mut([ea, pe]) else { continue };
        // A scripted two-hander when one fits the pair's voices (a real question and its answer),
        // else the old pair of unrelated `Chat` lines.
        let (va, vb) = (a.body.voice(), b.body.voice());
        if let Some((d, a_opens)) = pick_dialogue(&talk, &mut a.rng, va, vb) {
            let opener = if a_opens { (pa, va) } else { (pb, vb) };
            if say(&mut talk, &mut speak, now, hero, opener.0, 0.0, opener.1, LifeCue::Dlg(d, 0), 4.5) {
                // `run_dialogue` takes it from beat 1 and holds the pair until the last line.
                talk.run = Some(DlgRun { d, k: 1, lead: ea, partner: pe, a_opens, at: now + 0.8 });
                a.until = a.until.max(now + 12.0);
                b.until = b.until.max(now + 12.0);
                let ri = talk.recent_i;
                talk.recent[ri % 4] = d + 1;
                talk.recent_i = ri + 1;
            }
            continue;
        }
        if say(&mut talk, &mut speak, now, hero, pa, 0.0, a.body.voice(), LifeCue::Chat, 4.5) {
            let reply = if r01(&mut b.rng) < 0.4 { LifeCue::Laugh } else { LifeCue::Chat };
            b.vo.push((now + 4.6, reply));
        }
    }
    // Bench neighbours: two seated on the same bench → one talks, the other laughs / listens.
    for i in 0..n {
        let (ea, pa, sta, _, _, _, sa, _) = scratch[i];
        let St::Sit(_) = sta else { continue };
        let Some(bench) = sa.and_then(|s| w.spots.get(s)).map(|s| s.bench) else { continue };
        if bench == u16::MAX {
            continue;
        }
        let Some(&(eb, ..)) = scratch.iter().find(|s| {
            s.0 != ea && matches!(s.2, St::Sit(_)) && s.6.and_then(|x| w.spots.get(x)).is_some_and(|x| x.bench == bench)
        }) else {
            continue;
        };
        if ea.to_bits() > eb.to_bits() {
            continue; // handle each pair once
        }
        let Ok([(_, mut a, _), (_, mut b, _)]) = q.get_many_mut([ea, eb]) else { continue };
        if a.partner == Some(eb) {
            continue;
        }
        a.partner = Some(eb);
        b.partner = Some(ea);
        let (ka, kb) = (a.st, b.st);
        if matches!(ka, St::Sit(SitAct::Rest | SitAct::Drink)) {
            a.st = St::Sit(SitAct::Talk);
        }
        if matches!(kb, St::Sit(SitAct::Rest)) {
            b.st = St::Sit(SitAct::Laugh);
        }
        if say(&mut talk, &mut speak, now, hero, pa, 0.0, a.body.voice(), LifeCue::Chat, 4.5) {
            b.vo.push((now + 4.6, LifeCue::Laugh));
        }
    }
}

/// Play the two-hander in [`LifeTalk::run`] one beat at a time: a beat waits until the voice channel
/// has been quiet for a breath, is spoken from its speaker's spot, and keeps the pair chatting and
/// the street's other mouths shut. The scene breaks off if the pair stops chatting (bedtime, a
/// siege, a despawn) or the knight wanders out of earshot.
fn run_dialogue(
    talk: &mut LifeTalk,
    speak: &mut MessageWriter<Speak>,
    voices: &crate::audio::director::VoiceManager,
    q: &mut Query<(Entity, &mut LifeAgent, &Villager), Without<crate::dying::Dying>>,
    now: f32,
    hero: Vec2,
) {
    use crate::audio::lines::{BACKGROUND_PRIORITY, LIFE_DIALOGUES};
    /// Pause between one line ending and the reply (s).
    const BREATH: f32 = 0.35;
    let Some(mut run) = talk.run else { return };
    let busy = voices.active.values().any(|x| now < x.ends_at && x.priority > BACKGROUND_PRIORITY);
    if busy {
        talk.busy_at = now;
    }
    let Ok([(_, mut a, va), (_, mut b, vb)]) = q.get_many_mut([run.lead, run.partner]) else {
        talk.run = None;
        talk.next = now + 3.0;
        return;
    };
    let beats = LIFE_DIALOGUES.get(run.d as usize).map_or(0, |d| d.beats);
    let together = a.st == St::Chat && b.st == St::Chat && a.partner == Some(run.partner);
    if !together || run.k >= beats || va.pos.distance(hero) > HEAR + 8.0 {
        talk.run = None;
        talk.next = now + 3.0;
        if together {
            a.until = a.until.min(now + 2.5);
            b.until = b.until.min(now + 2.5);
        }
        return;
    }
    // Hold the pair (and every other village voice) while the scene plays.
    a.until = a.until.max(now + 6.0);
    b.until = b.until.max(now + 6.0);
    talk.next = talk.next.max(now + 2.0);
    if busy || now < run.at || now - talk.busy_at < BREATH {
        return;
    }
    let lead_speaks = (run.k % 2 == 0) == run.a_opens;
    let (pos, voice) = if lead_speaks { (va.pos, a.body.voice()) } else { (vb.pos, b.body.voice()) };
    if pos.distance(hero) < HEAR + 8.0 {
        speak.write(Speak::at(Concept::Life(voice, LifeCue::Dlg(run.d, run.k)), Vec3::new(pos.x, crate::steer::footing(pos.x, pos.y).unwrap_or(0.0) + 1.5, pos.y)));
    }
    run.k += 1;
    run.at = now + 0.6;
    talk.run = Some(run);
}

// ── Vendors ───────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn vendor_life(
    time: Res<Time>,
    mut commands: Commands,
    stalls: Res<Stalls>,
    hero_q: Query<&crate::player::Hero>,
    clock: Option<Res<crate::scene::SkyClock>>,
    mut talk: ResMut<LifeTalk>,
    mut speak: MessageWriter<Speak>,
    fresh: Query<(Entity, &GlobalTransform), (With<crate::market::MarketVendor>, Without<VendorLife>)>,
    mut vendors: Query<(&mut VendorLife, &GlobalTransform, &Visibility)>,
) {
    let now = time.elapsed_secs();
    for (e, gt) in &fresh {
        let p = gt.translation().xz();
        if p == Vec2::ZERO || stalls.slots.is_empty() {
            continue;
        }
        let stall = (0..stalls.slots.len()).min_by(|a, b| stalls.slots[*a].2.distance(p).total_cmp(&stalls.slots[*b].2.distance(p))).unwrap_or(0);
        let seed = e.to_bits() as u32;
        commands.entity(e).insert((
            VendorLife { stall, act: Act::Stand, t0: now, until: now, next_hawk: now + 8.0 + (seed % 23) as f32, say: None },
            PoseOverride(clip::stand(Act::Stand, 0.0, 0.0)),
        ));
    }
    let hero = hero_q.single().map(|h| h.pos).unwrap_or(Vec2::splat(1e6));
    let day = clock.as_ref().is_none_or(|c| c.t < 0.5 || c.t > 0.97);
    for (mut vl, gt, vis) in &mut vendors {
        if *vis == Visibility::Hidden {
            continue;
        }
        let p = gt.translation();
        if let Some((at, c)) = vl.say {
            if now >= at {
                vl.say = None;
                if p.xz().distance(hero) < 15.0 {
                    speak.write(Speak::at(Concept::Life(LifeVoice::Vendor, c), p + Vec3::Y * 1.5));
                    talk.next = talk.next.max(now + 3.0);
                }
            }
        }
        let cue = stalls.cue.get(vl.stall).copied().unwrap_or((Act::Stand, 0.0));
        let busy = stalls.buyer.get(vl.stall).is_some_and(|b| b.is_some());
        if busy && cue.1 > now && cue.0 != vl.act {
            // The buyer haggles → the vendor names her price; pays → she hands the goods over.
            vl.act = cue.0;
            vl.t0 = now;
            vl.until = cue.1;
            // Answer the buyer a beat after she speaks: the price, then (once paid) the close.
            vl.say = match cue.0 {
                Act::Haggle => Some((now + 2.5, LifeCue::Price)),
                Act::Pay => Some((now + 1.6, if (now * 5.3).fract() < 0.4 { LifeCue::Counter } else { LifeCue::Sold })),
                _ => None,
            };
        } else if now >= vl.until && vl.act != Act::Stand {
            vl.act = Act::Stand;
            vl.t0 = now;
        } else if !busy && now >= vl.next_hawk && day {
            vl.next_hawk = now + 25.0 + (now * 7.3).fract() * 30.0;
            vl.act = Act::Call;
            vl.t0 = now;
            vl.until = now + 2.6;
            let near = p.xz().distance(hero);
            let c = if near < 5.0 { LifeCue::Tease } else if (now * 3.1).fract() < 0.15 { LifeCue::Slow } else { LifeCue::Hawk };
            say(&mut talk, &mut speak, now, hero, p.xz(), p.y - 1.5, LifeVoice::Vendor, c, 5.0);
        }
    }
}

/// The wall guards mutter now and then when the knight passes, and call the dusk.
fn guard_barks(
    time: Res<Time>,
    mut tick: Local<f32>,
    mut last_phase: Local<Option<Phase>>,
    clock: Option<Res<crate::scene::SkyClock>>,
    siege: Option<Res<crate::siege::Siege>>,
    hero_q: Query<&crate::player::Hero>,
    guards: Query<&GlobalTransform, (With<crate::villagers::Guard>, Without<crate::dying::Dying>)>,
    mut talk: ResMut<LifeTalk>,
    mut speak: MessageWriter<Speak>,
) {
    let now = time.elapsed_secs();
    if now < *tick {
        return;
    }
    *tick = now + 1.5;
    let Ok(hero) = hero_q.single() else { return };
    let wave = siege.as_ref().is_some_and(|s| s.phase == crate::siege::GamePhase::Wave);
    let phase = phase_of(clock.as_ref().map_or(0.25, |c| c.t), wave);
    let Some(g) = guards.iter().map(|g| g.translation()).min_by(|a, b| a.xz().distance(hero.pos).total_cmp(&b.xz().distance(hero.pos))) else { return };
    let d = g.xz().distance(hero.pos);
    let cue = if *last_phase != Some(phase) && phase == Phase::Dusk {
        Some(if (now * 1.7).fract() < 0.5 { LifeCue::Dusk } else { LifeCue::Gate })
    } else if phase == Phase::Dawn && *last_phase != Some(Phase::Dawn) {
        Some(LifeCue::Yawn)
    } else if d < 5.0 && (now * 0.37).fract() < 0.06 {
        Some(if (now * 2.3).fract() < 0.5 { LifeCue::Greet } else { LifeCue::Patrol })
    } else {
        None
    };
    *last_phase = Some(phase);
    if let Some(c) = cue {
        say(&mut talk, &mut speak, now, hero.pos, g.xz(), g.y, LifeVoice::Guard, c, 6.0);
    }
}

// ── The animator ──────────────────────────────────────────────────────────────────────

/// Squared camera distance past which nobody is posed (= `biped::LIMB_CULL2`).
const POSE_CULL2: f32 = 70.0 * 70.0;
const FADE: f32 = 0.35;

fn state_key(st: St) -> u32 {
    match st {
        St::Indoors => 0,
        St::Walk => 1,
        St::Idle => 2,
        St::SitDown => 3,
        St::Sit(a) => 10 + a as u32,
        St::StandUp => 4,
        St::Do(a) => 40 + a as u32,
        St::Chat => 5,
        St::Greet(a) => 80 + a as u32,
        St::Buy(s) => 120 + s as u32,
        St::Play => 1,
    }
}

/// Chat turns: the lead talks on even 3-second beats, the partner on odd ones; the listener
/// crosses arms, nods, now and then laughs.
fn chat_act(a: &LifeAgent, v: &Villager, age: f32) -> Act {
    let beat = ((age / 3.2) as i32 + if a.lead { 0 } else { 1 }) % 2 == 0;
    let flavour = ((age / 3.2) as u32 + (v.phase * 10.0) as u32) % 4;
    if beat {
        Act::Talk
    } else if flavour == 0 {
        Act::Laugh
    } else if flavour == 1 {
        Act::Nod
    } else {
        Act::Listen
    }
}

/// The realistic-NPC (`npc_skin`) clip standing in for a standing act.
fn act_clip(act: Act) -> &'static str {
    match act {
        Act::Stand | Act::Hide => "",
        Act::Talk | Act::Laugh | Act::Haggle | Act::Browse => "Idle_Talking_Loop",
        Act::Listen | Act::Stare | Act::Yawn => "Idle_FoldArms_Loop",
        Act::Nod => "Yes",
        // A greeting is a respectful nod. (`Idle_Rail_Call` — both arms flung up — read as a
        // frightened hands-up, as if the hero had drawn on them.) Calling out keeps it.
        Act::Wave | Act::Bow => "Yes",
        Act::Call => "Idle_Rail_Call",
        Act::Sweep => "Push_Loop",
        Act::Draw | Act::Hang | Act::Pay => "Interact",
        Act::Scatter => "Farm_PlantSeed",
        Act::Weed => "Farm_Harvest",
        Act::Hammer | Act::Saw => "Fixing_Kneeling",
        Act::Warm => "Idle_Lantern_Loop",
        Act::Lift => "PickUp_Table",
        Act::Spar => "Sword_Regular_B",
    }
}

/// Skin clip for a life state (`""` while walking/idling: the locomotion drive picks).
fn skin_clip(st: St, chat: Act) -> &'static str {
    match st {
        St::Indoors | St::Walk | St::Idle | St::Play => "",
        St::SitDown => "Sitting_Enter",
        St::StandUp => "Sitting_Exit",
        St::Sit(SitAct::Talk | SitAct::Laugh) => "Sitting_Talking_Loop",
        St::Sit(_) => "Sitting_Idle_Loop",
        St::Do(act) | St::Greet(act) => act_clip(act),
        St::Chat => act_clip(chat),
        St::Buy(0 | 1) => "Idle_Talking_Loop",
        St::Buy(_) => "Interact",
    }
}

#[allow(clippy::type_complexity)]
fn life_animate(
    time: Res<Time>,
    mut commands: Commands,
    cam: Query<&GlobalTransform, With<Camera3d>>,
    mut q: Query<(Entity, &mut LifeAgent, &Villager, &GlobalTransform, &mut PoseOverride, Option<&mut LifeAnim>, Option<&mut LifeProps>, Option<&mut SkinClip>)>,
    mut vendors: Query<(Entity, &VendorLife, &GlobalTransform, &mut PoseOverride, &crate::villagers::Villager, Option<&mut SkinClip>), Without<LifeAgent>>,
    mut vis: Query<&mut Visibility>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let cam_p = cam.iter().next().map(|g| g.translation());
    for (e, mut a, v, gt, mut over, anim, props, hint) in &mut q {
        if a.st == St::Indoors {
            continue;
        }
        if let (Some(cp), true) = (cam_p, gt.translation() != Vec3::ZERO) {
            if gt.translation().distance_squared(cp) > POSE_CULL2 {
                continue;
            }
        }
        let t = now + v.phase * 2.5;
        let age = now - a.t0;
        let kid = a.body == Body::Kid;
        let pose = match a.st {
            St::Indoors => continue,
            St::Walk | St::Idle | St::Play => {
                let m = anim.as_ref().map_or(0.0, |x| x.moving);
                let run = if kid { 1.0 } else { 0.0 };
                clip::carry_walk(t, (now + v.phase) * v.gait, m, run, a.carry)
            }
            St::SitDown => clip::rise(1.0 - (age / 1.5).clamp(0.0, 1.0), a.seat),
            St::StandUp => clip::rise((age / 1.5).clamp(0.0, 1.0), a.seat),
            St::Sit(act) => clip::seated(act, a.seat, t),
            St::Do(act) => {
                let len = (a.until - a.t0).max(0.5);
                let k = match act {
                    Act::Yawn => clip::envelope(age, len, 0.9),
                    Act::Lift => (age / len).clamp(0.0, 1.0),
                    _ => 1.0,
                };
                clip::stand(act, t, k)
            }
            St::Greet(act) => {
                let k = clip::envelope(age, 1.9, 0.5);
                clip::stand(act, t, if act == Act::Bow { k } else { 1.0 })
            }
            St::Chat => clip::stand(chat_act(&a, v, age), t, 1.0),
            St::Buy(stage) => {
                let act = match stage {
                    0 => Act::Browse,
                    1 => Act::Haggle,
                    _ => Act::Pay,
                };
                clip::stand(act, t, 1.0)
            }
        };
        let mut pose = pose;
        if a.look.abs() > 0.01 && !matches!(a.st, St::SitDown | St::StandUp) {
            clip::look(&mut pose, a.look, -0.05);
        }
        let want = skin_clip(a.st, chat_act(&a, v, age));
        match hint {
            Some(mut h) => {
                if h.0 != want {
                    h.0 = want;
                }
            }
            None => {
                commands.entity(e).try_insert(SkinClip(want));
            }
        }
        let key = state_key(a.st);
        match anim {
            Some(mut an) => {
                let target = if v.moving { 1.0 } else { 0.0 };
                an.moving += (target - an.moving) * (dt * 8.0).min(1.0);
                if a.shift.abs() > 1e-4 {
                    clip::shift(&mut an.last, a.shift);
                    a.shift = 0.0;
                }
                if an.key != key {
                    an.from = an.last;
                    an.t0 = now;
                    an.key = key;
                }
                let s = ((now - an.t0) / FADE).clamp(0.0, 1.0);
                let out = if s < 1.0 { clip::blend(&an.from, &pose, s * s * (3.0 - 2.0 * s)) } else { pose };
                an.last = out;
                over.0 = out;
            }
            None => {
                a.shift = 0.0;
                commands.entity(e).insert(LifeAnim { key, from: pose, last: pose, t0: now, moving: 0.0 });
                over.0 = pose;
            }
        }
        // Hand props follow the act.
        if let Some(mut pr) = props {
            if pr.shown != a.held {
                for (h, pe) in pr.items {
                    if h == Held::None {
                        continue;
                    }
                    if let Ok(mut vv) = vis.get_mut(pe) {
                        let want = if h == a.held { Visibility::Inherited } else { Visibility::Hidden };
                        if *vv != want {
                            *vv = want;
                        }
                    }
                }
                pr.shown = a.held;
            }
        }
    }
    for (ve, vl, gt, mut over, v, hint) in &mut vendors {
        let want = act_clip(vl.act);
        match hint {
            Some(mut h) => {
                if h.0 != want {
                    h.0 = want;
                }
            }
            None => {
                commands.entity(ve).try_insert(SkinClip(want));
            }
        }
        if let Some(cp) = cam_p {
            if gt.translation().distance_squared(cp) > POSE_CULL2 {
                continue;
            }
        }
        let t = now + v.phase * 2.5;
        let len = (vl.until - vl.t0).max(0.5);
        let k = if vl.act == Act::Stand { 1.0 } else { clip::envelope(now - vl.t0, len, 0.4) };
        over.0 = clip::stand(vl.act, t, k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two sitters on one baked-in bench sit ≥ [`SEAT_GAP`] apart and both ON it (half a body
    /// in from either end).
    #[test]
    fn house_bench_sitters_are_spaced_and_on_the_bench() {
        for (piece, len) in [("town_street_house_a", 1.1), ("town_house_a", 1.0), ("town_house_c", 0.9)] {
            let (_, _, _, top, offs) = house_seats(piece).unwrap();
            assert!((0.4..0.6).contains(&top));
            for (i, a) in offs.iter().enumerate() {
                assert!(a.abs() + 0.2 <= len / 2.0 + 1e-3, "{piece}: sitter {a} hangs off the bench");
                for b in &offs[i + 1..] {
                    assert!((a - b).abs() >= SEAT_GAP * 0.9, "{piece}: sitters {a} / {b} overlap");
                }
            }
        }
    }

    /// A new bench by a house never lands on what the house model already has by its door
    /// (`village_huts.py`, house-local after the cottage's turn): doors, the shop counter, barrels.
    #[test]
    fn new_house_benches_clear_the_house_props() {
        // (piece, [(centre, half)]) — door openings + 0.3 swing, counter, barrel, muck heap, bucket.
        let props: [(&str, Vec<(Vec2, Vec2)>); 2] = [
            ("town_street_house_b", vec![(Vec2::new(-1.2, 3.5), Vec2::new(0.4, 0.3)), (Vec2::new(0.75, 3.6), Vec2::new(0.65, 0.35)), (Vec2::new(1.85, 3.75), Vec2::splat(0.26)), (Vec2::new(0.0, 0.0), Vec2::new(2.2, 3.3))]),
            ("town_house_b", vec![(Vec2::new(-1.6, 1.9), Vec2::new(0.4, 0.3)), (Vec2::new(1.75, 1.9), Vec2::new(0.58, 0.3)), (Vec2::new(0.4, 2.05), Vec2::splat(0.26)), (Vec2::new(2.7, 2.95), Vec2::new(0.95, 0.6)), (Vec2::new(0.0, 0.0), Vec2::new(3.0, 1.6))]),
        ];
        for (piece, boxes) in props {
            let (c, yaw, face) = house_new_bench(piece).unwrap();
            let f = fwd(face);
            for x in [-0.6_f32, -0.3, 0.0, 0.3, 0.6] {
                for z in [-0.15_f32, 0.15] {
                    let q = c + rot2(Vec2::new(x, z), yaw);
                    for (bc, bh) in &boxes {
                        let d = (q - *bc).abs();
                        assert!(d.x > bh.x || d.y > bh.y, "{piece}: bench point {q} inside a prop at {bc}");
                    }
                }
            }
            // the sitters face away from the wall, out over open ground
            assert!((c + f * 0.6).length() > c.length(), "{piece}: the sitters face the wall");
        }
    }

    /// The fire pit's log seats: on the logs (r 2.05), facing the fire, ≥ SEAT_GAP apart, and the
    /// standing warm-up spots sit between the logs, clear of both the logs and the sitters' feet.
    #[test]
    fn fire_pit_seats_use_its_own_logs() {
        let f = Vec2::ZERO;
        let mut seats = Vec::new();
        let mut logs = Vec::new();
        for k in 0..3 {
            let a = 1.0 + k as f32 * TAU / 3.0;
            let lc = Vec2::new(a.cos(), a.sin()) * 2.05;
            let ty = (-a.cos()).atan2(-a.sin());
            assert!((rot2(Vec2::X, ty) - Vec2::new(-a.sin(), a.cos())).length() < 1e-4);
            logs.push(lc);
            for o in [-0.4_f32, 0.4] {
                seats.push((lc + rot2(Vec2::new(o, 0.0), ty) + fwd(yaw_to(lc, f)) * 0.04, yaw_to(lc, f)));
            }
        }
        for (i, (p, face)) in seats.iter().enumerate() {
            assert!((fwd(*face) - (f - *p).normalize()).length() < 0.25, "seat {i} doesn't face the fire");
            for (q, _) in &seats[i + 1..] {
                assert!(p.distance(*q) >= SEAT_GAP);
            }
            let feet = *p + fwd(*face) * 0.4;
            for k in 0..3 {
                let g = 1.0 + k as f32 * TAU / 3.0 + TAU / 6.0;
                let stand = Vec2::new(g.cos(), g.sin()) * 1.55;
                assert!(stand.distance(feet) > 0.6, "warm-up spot {k} on seat {i}'s feet");
                assert!(logs.iter().all(|l| l.distance(stand) > 0.95), "warm-up spot {k} in a log");
            }
        }
    }

    #[test]
    fn the_day_runs_dawn_to_night_and_a_siege_is_night() {
        assert_eq!(phase_of(0.02, false), Phase::Dawn);
        assert_eq!(phase_of(0.12, false), Phase::Work);
        assert_eq!(phase_of(0.25, false), Phase::Noon);
        assert_eq!(phase_of(0.4, false), Phase::Afternoon);
        assert_eq!(phase_of(0.5, false), Phase::Dusk);
        assert_eq!(phase_of(0.75, false), Phase::Night);
        assert_eq!(phase_of(0.25, true), Phase::Night);
    }

    #[test]
    fn a_seated_root_stands_up_exactly_where_it_sat_down_from() {
        let s = Spot { kind: SpotKind::Seat, pos: Vec2::new(3.0, -2.0), face: 0.7, zone: Zone::Village, bench: 0, seat_h: BENCH_TOP, house: None, need_houses: 0, taken: None };
        let scale = 0.8;
        let (root, stand) = seat_root(&s, scale);
        let risen = root + fwd(s.face) * clip::RISE_DZ * scale;
        assert!(risen.distance(stand) < 1e-5);
        assert!(root.distance(s.pos) < 0.05);
    }
}
