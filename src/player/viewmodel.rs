//! **First-person viewmodel** — the knight's own hands, sword and shield, parented to the camera.
//!
//! The old first-person view re-used the third-person rig's arms: mirrored by a handedness hack,
//! posed through Euler angles solved against a tilted hand frame, with the body half-hidden mesh
//! by mesh. Every tweak fought the rig, and the result read as a rod floating in front of the lens.
//!
//! This module replaces all of it. The hands are a small set of purpose-built props (see
//! `model::build_viewmodel`) that are **children of the camera**, so they can never drift from the
//! lens, and each is placed directly in *screen space*:
//!
//! - a pose is `(ndc x, ndc y, depth, blade direction, roll)` — "the grip sits at the lower right
//!   of the frame, 0.6u out, blade pointing up and slightly inward" — so authoring a swing is
//!   authoring where the blade is on screen, not rig joint angles;
//! - forearms are solved by a one-bone IK from the wrist to a fixed elbow anchor behind/below the
//!   lens, so an arm always connects to its hand whatever the swing does;
//! - weight comes from springs: look-inertia (the weapon lags a turn), a recoil spring kicked by
//!   hit feedback, landing/jump response, and stride bob — layered on the authored pose.
//!
//! The render pipeline is single-camera (see CLAUDE.md), so the viewmodel shares the world pass;
//! it is lit by the same sun/IBL and lowers itself when a solid obstacle is within reach so the
//! blade never clips through walls and trunks.
//!
//! The third-person rig is hidden in first person (`camera::fp_body_visibility`) and plays its
//! normal clips untouched, so nothing here is coupled to it.

use std::f32::consts::PI;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::creature::{make_viewmodel_material, CreatureMaterial};
use crate::game_state::{AppState, CampaignOnly};
use crate::inventory::Inventory;
use crate::villagers::NpcHp;
use crate::ui::theme::rgb;

use super::camera::{spring, FirstPerson, OrbitCam, FP_FOV_DEG};
use super::combat::{CHARGE_GRACE, CHARGE_THRESHOLD, HEAVY_VARIANT};
use super::model::{self, FOREARM_LEN};
use super::{Hero, HeroHealth, Health, PlayMode, PlayerRes, HERO_SCALE};

// ── Components ─────────────────────────────────────────────────────────────────────────

/// Root of the viewmodel (child of the main camera). Hidden outside first person.
#[derive(Component)]
pub(crate) struct FpRoot;

/// The four animated pieces. `Weapon` / `Shield` carry their own fist; the forearms are placed by
/// IK from the hand's wrist to the elbow anchor.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FpPart {
    Weapon,
    Shield,
    ForearmR,
    ForearmL,
}

/// Uniform scale of the shield relative to its third-person size. The heater is ~0.63u tall at
/// hero scale — held 0.7u from the eye that would fill three quarters of the frame.
const SHIELD_SCALE: f32 = 0.62;
/// Strength (lux-equivalent) of the camera-locked key light on the viewmodel material — see
/// `creature.wgsl`. The sun is 14 100 lux at noon, so this is a soft fill, not a second sun.
const VM_KEY_LUX: f32 = 3200.0;
/// Wrist attachment points, in each held item's own space (rig units): just below the fist on
/// the sword grip; behind the plate on the shield strap.
const WRIST_SWORD: Vec3 = Vec3::new(0.0, 0.0, -0.02);
const WRIST_SHIELD: Vec3 = Vec3::new(0.0, 0.03, -0.17);
/// Elbow anchors in camera space (x right, y up, -z forward): far below the bottom edge and
/// slightly behind the lens, so the arm always enters the frame from the bottom corners.
const ELBOW_R: Vec3 = Vec3::new(0.34, -0.66, 0.06);
const ELBOW_L: Vec3 = Vec3::new(-0.36, -0.66, 0.06);

// ── Poses ──────────────────────────────────────────────────────────────────────────────

/// A held item's pose, authored on screen: `p = (ndc x, ndc y, depth)` of its origin and `q` its
/// orientation. See [`item_rot`] for how a direction + roll become `q`.
#[derive(Clone, Copy)]
struct Kf {
    p: Vec3,
    q: Quat,
}

impl Kf {
    fn lerp(self, o: Kf, t: f32) -> Kf {
        Kf { p: self.p.lerp(o.p, t), q: self.q.slerp(o.q, t) }
    }
}

/// Orientation of a held item whose long axis (blade / shield-up) points along `dir` (camera
/// space: x right, y up, -z forward) and whose flat face is turned toward the viewer, rolled by
/// `roll` about the long axis. The face reference leans up a little so a blade pointing straight
/// forward shows its flat to the sky, and its edges face left/right — what a slash needs.
fn item_rot(dir: Vec3, roll: f32) -> Quat {
    let y = dir.normalize();
    let face_ref = Vec3::new(0.0, 0.33, 0.94);
    let mut z = face_ref - y * y.dot(face_ref);
    if z.length_squared() < 1e-4 {
        z = Vec3::Z - y * y.z;
    }
    let z = Quat::from_axis_angle(y, roll) * z.normalize();
    let x = y.cross(z);
    Quat::from_mat3(&Mat3::from_cols(x, y, z))
}

fn kf(x: f32, y: f32, d: f32, dir: [f32; 3], roll: f32) -> Kf {
    Kf { p: Vec3::new(x, y, d), q: item_rot(Vec3::from_array(dir), roll) }
}

// Sword hand.
fn sword_carry() -> Kf {
    kf(0.66, -0.86, 0.60, [0.06, 0.50, -0.86], 0.20)
}
fn sword_ready() -> Kf {
    kf(0.58, -0.68, 0.58, [-0.20, 0.72, -0.66], 0.35)
}
fn sword_sprint() -> Kf {
    kf(0.74, -0.98, 0.54, [0.20, 0.30, -0.93], 0.45)
}
fn sword_guard() -> Kf {
    kf(0.80, -1.0, 0.56, [0.25, 0.35, -0.90], 0.40)
}
/// Sword tucked away for a dodge roll.
fn sword_tuck() -> Kf {
    kf(0.80, -1.45, 0.50, [0.30, 0.20, -0.90], 0.5)
}

// Shield hand.
fn shield_carry() -> Kf {
    kf(-0.90, -0.94, 0.56, [0.22, 1.0, -0.10], -1.00)
}
fn shield_ready() -> Kf {
    kf(-0.78, -0.82, 0.58, [0.18, 1.0, -0.15], -0.78)
}
fn shield_block() -> Kf {
    kf(-0.40, -0.30, 0.76, [0.10, 1.0, 0.10], 0.16)
}
fn shield_tuck() -> Kf {
    kf(-1.05, -1.5, 0.50, [0.25, 1.0, -0.1], -1.0)
}

/// One attack's keyframes plus its camera lean (screen-space radians: x pitch(+up), y yaw(+left),
/// z roll): `lean_wind` pulls opposite the cut during the wind-up, `lean_strike` rides the blade.
struct Swing {
    wind: Kf,
    mid: Kf,
    hit: Kf,
    end: Kf,
    lean_wind: Vec3,
    lean_strike: Vec3,
}

fn swing(variant: u8) -> Swing {
    match variant {
        // Horizontal slash — cocked wide to the right, edge leading across the frame to the left.
        1 => Swing {
            wind: kf(0.92, -0.40, 0.58, [0.78, 0.30, -0.50], -0.30),
            mid: kf(0.25, -0.32, 0.75, [0.05, 0.20, -0.98], 0.0),
            hit: kf(-0.50, -0.36, 0.66, [-0.78, 0.18, -0.60], 0.25),
            end: kf(-0.62, -0.50, 0.60, [-0.85, 0.05, -0.50], 0.30),
            lean_wind: Vec3::new(0.008, -0.020, 0.014),
            lean_strike: Vec3::new(-0.006, 0.032, -0.026),
        },
        // Thrust — the hilt drawn to the ribs with the point levelled at the crosshair, then the
        // whole arm driven out along the view axis.
        2 => Swing {
            wind: kf(0.52, -0.64, 0.50, [-0.10, 0.25, -0.96], 0.30),
            mid: kf(0.32, -0.40, 0.72, [-0.12, 0.20, -0.97], 0.15),
            hit: kf(0.10, -0.14, 1.00, [-0.04, 0.12, -0.99], 0.0),
            end: kf(0.30, -0.40, 0.80, [-0.08, 0.15, -0.98], 0.10),
            lean_wind: Vec3::new(-0.006, -0.006, 0.006),
            lean_strike: Vec3::new(0.012, 0.006, -0.008),
        },
        // Heavy Strike — the overhead chop writ large (also the held charge shape).
        v if v == HEAVY_VARIANT => Swing {
            wind: kf(0.50, 0.50, 0.46, [0.15, 0.88, 0.45], 0.30),
            mid: kf(0.25, 0.20, 0.62, [0.04, 1.0, -0.12], 0.15),
            hit: kf(0.0, -0.55, 0.88, [0.0, 0.06, -1.0], 0.0),
            end: kf(0.12, -0.80, 0.78, [0.02, -0.30, -0.95], 0.10),
            lean_wind: Vec3::new(0.028, 0.0, -0.010),
            lean_strike: Vec3::new(-0.050, 0.006, 0.016),
        },
        // Overhead chop — blade cocked high over the shoulder, driven down the middle.
        _ => Swing {
            wind: kf(0.72, 0.22, 0.55, [0.30, 0.80, 0.52], 0.45),
            mid: kf(0.42, 0.04, 0.62, [0.05, 0.96, -0.25], 0.25),
            hit: kf(0.12, -0.42, 0.80, [-0.04, 0.10, -0.99], 0.05),
            end: kf(0.28, -0.66, 0.70, [0.0, -0.22, -0.97], 0.15),
            lean_wind: Vec3::new(0.016, -0.006, -0.008),
            lean_strike: Vec3::new(-0.034, 0.008, 0.012),
        },
    }
}

/// Swing timeline as fractions of the swing: wind-up, strike (contact lands near its middle —
/// combat's damage frame is 0.3), follow-through, recovery to the stance.
const WIND_END: f32 = 0.22;
const HIT_AT: f32 = 0.40;
const FOLLOW_END: f32 = 0.58;

fn smooth(t: f32) -> f32 {
    let c = t.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}
fn ease_out(t: f32, pow: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powf(pow)
}

/// Sample a swing at progress `p` (0..1) from the current stance `base`. Returns the pose plus the
/// `(back, fwd)` envelopes the camera lean rides: `back` peaks at the wind-up, `fwd` at the strike.
fn swing_pose(s: &Swing, base: Kf, p: f32) -> (Kf, f32, f32) {
    if p < WIND_END {
        let t = ease_out(p / WIND_END, 3.0);
        (base.lerp(s.wind, t), t, 0.0)
    } else if p < HIT_AT {
        let u = ease_out((p - WIND_END) / (HIT_AT - WIND_END), 2.2);
        // Position bows through `mid` (a crescent, not a straight lerp); orientation goes
        // wind → mid → hit in two slerps.
        let ctrl = s.mid.p * 2.0 - (s.wind.p + s.hit.p) * 0.5;
        let pos = s.wind.p * (1.0 - u) * (1.0 - u) + ctrl * (2.0 * (1.0 - u) * u) + s.hit.p * (u * u);
        let q = if u < 0.5 { s.wind.q.slerp(s.mid.q, u * 2.0) } else { s.mid.q.slerp(s.hit.q, (u - 0.5) * 2.0) };
        (Kf { p: pos, q }, 1.0 - u, u)
    } else if p < FOLLOW_END {
        let t = ease_out((p - HIT_AT) / (FOLLOW_END - HIT_AT), 2.0);
        (s.hit.lerp(s.end, t), 0.0, 1.0 - 0.35 * t)
    } else {
        let t = smooth((p - FOLLOW_END) / (1.0 - FOLLOW_END));
        (s.end.lerp(base, t), 0.0, 0.65 * (1.0 - t))
    }
}

// ── Camera-space solve ─────────────────────────────────────────────────────────────────

/// The lens the poses are authored against: nominal FP vertical FOV and the real aspect (clamped
/// so an ultrawide doesn't fling the hands off the edges).
struct Lens {
    tv: f32,
    th: f32,
}

impl Lens {
    fn new(aspect: f32) -> Self {
        let tv = (FP_FOV_DEG.to_radians() * 0.5).tan();
        Lens { tv, th: tv * aspect.clamp(1.45, 2.1) }
    }
    /// `(ndc x, ndc y, depth)` → camera-space position.
    fn cam(&self, p: Vec3) -> Vec3 {
        Vec3::new(p.x * p.z * self.th, p.y * p.z * self.tv, -p.z)
    }
}

/// Capture hook `FOREST_VMPOSE`: freeze the viewmodel in one pose so a still frames it exactly.
/// `swing:<variant>:<progress>` (0 chop · 1 slash · 2 thrust · 3 heavy; progress 0..1), `block`,
/// `ready` or `sprint`. Read once.
#[derive(Clone, Copy)]
enum Forced {
    Swing(u8, f32),
    Block,
    Ready,
    Sprint,
}

fn forced_pose() -> Option<Forced> {
    static CELL: std::sync::OnceLock<Option<Forced>> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| {
        let v = std::env::var("FOREST_VMPOSE").ok()?;
        let mut it = v.split(':');
        match it.next()? {
            "swing" => {
                let variant = it.next()?.parse().ok()?;
                let p = it.next()?.parse().ok()?;
                Some(Forced::Swing(variant, p))
            }
            "block" => Some(Forced::Block),
            "ready" => Some(Forced::Ready),
            "sprint" => Some(Forced::Sprint),
            _ => None,
        }
    })
}

/// Persistent animation state (smoothed weights + spring memory).
#[derive(Default)]
pub(crate) struct VmState {
    ready: f32,
    block: f32,
    sprint: f32,
    draw: f32,
    air: f32,
    wall: f32,
    look: Vec2,
    look_v: Vec2,
    prev_look: Option<Vec2>,
    look_rate: Vec2,
    kick: f32,
    kick_v: f32,
    last_trauma: f32,
    hop: f32,
    hop_v: f32,
    was_air: bool,
}

fn approach(cur: &mut f32, target: f32, rate: f32, dt: f32) {
    *cur += (target - *cur) * (1.0 - (-rate * dt).exp());
}

// ── Spawn ──────────────────────────────────────────────────────────────────────────────

/// Build (or rebuild, on an equipment change) the viewmodel under the main camera. Lazy + idempotent
/// so it needs no startup ordering against the camera or the hero material, and recovers if the
/// camera entity is ever rebuilt.
pub(crate) fn spawn_viewmodel(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<CreatureMaterial>>,
    inv: Option<Res<Inventory>>,
    cam_q: Query<Entity, With<Camera3d>>,
    root_q: Query<Entity, With<FpRoot>>,
    mut last: Local<Option<(Option<String>, Option<String>)>>,
    mut material: Local<Option<Handle<CreatureMaterial>>>,
) {
    let have = root_q.single().ok();
    let changed = inv.as_ref().is_some_and(|i| i.is_changed());
    if have.is_some() && !changed {
        return;
    }
    let gear = inv
        .as_ref()
        .map(|i| (i.0.equipped_id.clone(), i.0.equipped_armor_id.clone()))
        .unwrap_or_default();
    if have.is_some() && last.as_ref() == Some(&gear) {
        return;
    }
    let Ok(cam) = cam_q.single() else { return };
    if let Some(old) = have {
        commands.entity(old).try_despawn();
    }
    *last = Some(gear.clone());

    let vm = model::build_viewmodel(gear.0.as_deref(), gear.1.as_deref());
    let weapon = meshes.add(vm.weapon);
    let shield = meshes.add(vm.shield);
    let fist = meshes.add(vm.fist);
    let forearm = meshes.add(vm.forearm);
    let m = material
        .get_or_insert_with(|| make_viewmodel_material(&mut mats, VM_KEY_LUX))
        .clone();

    let leaf = |mesh: Handle<Mesh>, xf: Transform| {
        (Mesh3d(mesh), MeshMaterial3d(m.clone()), xf, NotShadowCaster)
    };
    let s = Vec3::splat(HERO_SCALE);
    let root = commands
        .spawn((FpRoot, Transform::IDENTITY, Visibility::Hidden, CampaignOnly))
        .id();
    commands.entity(cam).add_child(root);

    commands
        .spawn((FpPart::Weapon, Transform::from_scale(s), Visibility::Inherited, ChildOf(root)))
        .with_children(|p| {
            p.spawn(leaf(weapon, Transform::IDENTITY));
            p.spawn(leaf(fist.clone(), Transform::IDENTITY));
        });
    commands
        .spawn((FpPart::Shield, Transform::from_scale(s * SHIELD_SCALE), Visibility::Inherited, ChildOf(root)))
        .with_children(|p| {
            p.spawn(leaf(shield, Transform::IDENTITY));
            // The strap fist, behind the plate with its grip axis laid horizontal; counter-scaled
            // so the glove stays full size while the plate is shrunk.
            p.spawn(leaf(
                fist,
                Transform {
                    translation: Vec3::new(0.0, 0.02, -0.13),
                    rotation: Quat::from_rotation_z(PI * 0.5),
                    scale: Vec3::splat(1.0 / SHIELD_SCALE),
                },
            ));
        });
    for part in [FpPart::ForearmR, FpPart::ForearmL] {
        commands.spawn((part, Transform::from_scale(s), Visibility::Inherited, ChildOf(root))).with_children(|p| {
            p.spawn(leaf(forearm.clone(), Transform::IDENTITY));
        });
    }
}

// ── Animate ────────────────────────────────────────────────────────────────────────────

/// How the hero's state maps onto the viewmodel, each frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn animate_viewmodel(
    time: Res<Time>,
    mode: Res<PlayMode>,
    player: Res<PlayerRes>,
    orbit: Res<OrbitCam>,
    mut fp: ResMut<FirstPerson>,
    feedback: Option<Res<crate::combat_fx::HitFeedback>>,
    build_mode: Option<Res<crate::town::BuildMode>>,
    succ: Option<Res<crate::succession::Succession>>,
    hero_q: Query<(&Hero, &HeroHealth)>,
    proj_q: Query<&Projection, With<Camera3d>>,
    mut root_q: Query<&mut Visibility, With<FpRoot>>,
    mut parts: Query<(&FpPart, &mut Transform)>,
    mut st: Local<VmState>,
) {
    let Ok((hero, hh)) = hero_q.single() else { return };
    let Ok(mut vis) = root_q.single_mut() else { return };
    let dt = time.delta_secs().clamp(1e-4, 0.05);
    let now = time.elapsed_secs();

    // ── Visibility / draw-in ──
    let busy = build_mode.as_ref().is_some_and(|b| b.active)
        || succ.as_ref().is_some_and(|s| s.active)
        || hero.victory;
    let show = *mode == PlayMode::Play && fp.active && fp.blend > 0.85 && player.0.is_alive() && !busy;
    approach(&mut st.draw, if show { 1.0 } else { 0.0 }, if show { 7.5 } else { 14.0 }, dt);
    let draw = st.draw;
    // `FOREST_VMHIDE=1` keeps every animation/lean running but hides the meshes, for A/B screen diffs.
    let want_vis = if draw > 0.01 && std::env::var_os("FOREST_VMHIDE").is_none() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *vis != want_vis {
        *vis = want_vis;
    }
    if draw <= 0.01 {
        fp.sway = Vec3::ZERO;
        st.prev_look = None;
        return;
    }

    let aspect = proj_q
        .single()
        .ok()
        .and_then(|p| if let Projection::Perspective(pp) = p { Some(pp.aspect_ratio) } else { None })
        .unwrap_or(16.0 / 9.0);
    let lens = Lens::new(aspect);

    // ── State weights ──
    let forced = forced_pose();
    let attack = match forced {
        Some(Forced::Swing(variant, p)) => Some((variant, p.clamp(0.0, 1.0))),
        Some(_) => None,
        None => hero
            .attacking
            .then(|| (hero.attack_variant, (hero.attack_t / hero.attack_dur.max(1e-3)).clamp(0.0, 1.0))),
    };
    let blocking = hh.blocking || matches!(forced, Some(Forced::Block));
    let threat = attack.is_some()
        || hero.charge_t > CHARGE_GRACE
        || blocking
        || hero.threats > 0
        || now < hero.combat_until
        || matches!(forced, Some(Forced::Ready));
    approach(&mut st.ready, if threat { 1.0 } else { 0.0 }, if threat { 12.0 } else { 3.0 }, dt);
    approach(&mut st.block, if blocking { 1.0 } else { 0.0 }, 11.0, dt);
    let sprint_t = if matches!(forced, Some(Forced::Sprint)) { 1.0 } else { hero.run_amt.clamp(0.0, 1.0) };
    let sprint_goal = sprint_t * (1.0 - st.block);
    approach(&mut st.sprint, sprint_goal, 9.0, dt);
    approach(&mut st.air, if hero.on_ground { 0.0 } else { 1.0 }, 9.0, dt);
    let (ready, block, sprint) = (st.ready, st.block, st.sprint);

    // ── Look inertia: the hands lag the view through a turn, then settle ──
    let look = Vec2::new(orbit.azimuth, fp.pitch);
    if let Some(prev) = st.prev_look {
        let raw = Vec2::new(crate::steer::wrap_pi(look.x - prev.x), look.y - prev.y) / dt;
        let k = 1.0 - (-dt * 12.0).exp();
        let cur = st.look_rate;
        st.look_rate = cur + (raw.clamp(Vec2::splat(-9.0), Vec2::splat(9.0)) - cur) * k;
    }
    st.prev_look = Some(look);
    let target = Vec2::new(st.look_rate.x * 0.0115, -st.look_rate.y * 0.0100);
    let (mut lx, mut lvx) = (st.look.x, st.look_v.x);
    let (mut ly, mut lvy) = (st.look.y, st.look_v.y);
    spring(&mut lx, &mut lvx, target.x, 16.0, 0.62, dt);
    spring(&mut ly, &mut lvy, target.y, 16.0, 0.62, dt);
    st.look = Vec2::new(lx, ly);
    st.look_v = Vec2::new(lvx, lvy);

    // ── Recoil: a hit spikes `trauma`; the rise becomes a shove back toward the lens ──
    if let Some(fb) = &feedback {
        let rise = (fb.trauma - st.last_trauma).max(0.0);
        if rise > 0.02 {
            st.kick_v += rise * 1.9;
        }
        st.last_trauma = fb.trauma;
    }
    let (mut kick, mut kick_v) = (st.kick, st.kick_v);
    spring(&mut kick, &mut kick_v, 0.0, 20.0, 0.45, dt);
    st.kick = kick;
    st.kick_v = kick_v;

    // ── Jump / landing response ──
    if hero.on_ground && st.was_air {
        st.hop_v -= (hero.vel_y.abs() * 0.05).clamp(0.12, 0.6);
    }
    st.was_air = !hero.on_ground;
    let (mut hop, mut hop_v) = (st.hop, st.hop_v);
    spring(&mut hop, &mut hop_v, 0.0, 22.0, 0.4, dt);
    st.hop = hop;
    st.hop_v = hop_v;
    let air_lift = if hero.on_ground { 0.0 } else { (hero.vel_y * 0.012).clamp(-0.05, 0.05) };

    // ── Stride bob ──
    let stride = hero.moving_amt.clamp(0.0, 1.0) * (1.0 - st.air);
    let run = 1.0 + hero.run_amt.clamp(0.0, 1.0) * 0.55;
    let ph = hero.walk_phase;
    let breath = (now * 1.7).sin();
    let bob_r = Vec3::new(
        ph.sin() * 0.0085 * stride * run,
        (ph * 2.0 + 0.7).sin() * 0.0105 * stride * run + breath * 0.0028,
        0.0,
    );
    let bob_l = Vec3::new(
        (ph + 0.5).sin() * 0.0075 * stride * run,
        (ph * 2.0 + 1.2).sin() * 0.0095 * stride * run + breath * 0.0028,
        0.0,
    );

    // ── Nearby solid obstacle in reach? Lower + draw the weapon in rather than clip into it ──
    let yaw = orbit.azimuth + PI;
    let fwd = Vec2::new(yaw.sin(), yaw.cos());
    let wall_now = {
        let view = crate::blockers::read();
        let mut nearest = f32::MAX;
        for d in [0.45_f32, 0.75, 1.05, 1.35] {
            if view.is_blocked(hero.pos.x + fwd.x * d, hero.pos.y + fwd.y * d) {
                nearest = d;
                break;
            }
        }
        if nearest == f32::MAX { 0.0 } else { (1.0 - (nearest - 0.45) / 0.9).clamp(0.0, 1.0) }
    };
    let wall_rate = if wall_now > st.wall { 14.0 } else { 6.0 };
    approach(&mut st.wall, wall_now, wall_rate, dt);
    let wall = st.wall;

    // ── Sword hand ──
    let mut right = sword_carry().lerp(sword_ready(), ready);
    right = right.lerp(sword_sprint(), sprint * (1.0 - 0.6 * ready));
    right = right.lerp(sword_guard(), block);

    let mut lean = Vec3::ZERO;
    let roll_w = if hero.roll_t >= 0.0 {
        let u = (hero.roll_t / super::movement::ROLL_TIME).clamp(0.0, 1.0);
        smooth(u / 0.15) * smooth((1.0 - u) / 0.18)
    } else {
        0.0
    };
    if let Some((variant, p)) = attack {
        let sw = swing(variant);
        let (pose, back, fwd_env) = swing_pose(&sw, right, p);
        right = pose;
        lean = sw.lean_wind * back + sw.lean_strike * fwd_env;
    } else if hero.charge_t > CHARGE_GRACE {
        // Holding a Heavy Strike: the blade is hauled into the overhead wind and trembles with effort.
        let frac = ((hero.charge_t - CHARGE_GRACE) / (CHARGE_THRESHOLD - CHARGE_GRACE)).clamp(0.0, 1.0);
        let sw = swing(HEAVY_VARIANT);
        right = right.lerp(sw.wind, smooth(frac * 1.6).min(1.0));
        lean = sw.lean_wind * frac;
        let shake = (now * 41.0).sin() * 0.012 * frac;
        right.p += Vec3::new(shake, (now * 37.0).cos() * 0.012 * frac, 0.0);
    } else if hero.dash_t >= 0.0 {
        let u = (hero.dash_t / super::DASH_TIME).clamp(0.0, 1.0);
        let w = smooth(u * 4.0) * (1.0 - smooth((u - 0.55) / 0.45));
        right = right.lerp(swing(2).hit, w);
    }
    right = right.lerp(sword_tuck(), roll_w);

    // ── Shield hand ──
    let mut left = shield_carry().lerp(shield_ready(), ready);
    left = left.lerp(shield_block(), block);
    if let Some((_, p)) = attack {
        // The off hand rides the swing: pulled back and down through the strike.
        let k = smooth((p - WIND_END) / (HIT_AT - WIND_END)) * (1.0 - smooth((p - FOLLOW_END) / (1.0 - FOLLOW_END)));
        left.p += Vec3::new(-0.05, -0.10, 0.0) * k * (1.0 - block);
    }
    left = left.lerp(shield_tuck(), roll_w);

    // ── Layer offsets (camera space, scaled to read at hand depth) ──
    let inertia = Vec3::new(st.look.x, st.look.y, 0.0);
    let hop_off = Vec3::new(0.0, st.hop * 0.22 + air_lift, 0.0);
    let kick_off = Vec3::new(0.0, st.kick * 0.25, st.kick * 0.9);
    // Wall-avoid: drop and tuck toward the body, swings included (they stay choreographed but
    // can't reach through the obstacle).
    let wall_off = Vec3::new(0.0, -0.20, 0.16) * wall;
    // Draw-in: raise from below the frame on entering first person.
    let draw_off = Vec3::new(0.0, -(1.0 - draw) * 0.55, (1.0 - draw) * 0.25);
    let common = inertia + hop_off + kick_off + wall_off + draw_off;

    // Keep each pose fixed on screen when the FOV widens (sprint / kicks); the hands still shrink
    // a touch, as real objects at the eye would.
    let place = |k: &Kf, extra: Vec3| -> (Vec3, Quat) { (lens.cam(k.p) + extra, k.q) };
    let (rp, rq) = place(&right, common + bob_r);
    let (lp, lq) = place(&left, common * 0.85 + bob_l);

    for (part, mut tf) in &mut parts {
        match part {
            FpPart::Weapon => {
                tf.translation = rp;
                tf.rotation = rq;
            }
            FpPart::Shield => {
                tf.translation = lp;
                tf.rotation = lq;
            }
            FpPart::ForearmR => solve_arm(&mut tf, rp + rq * (WRIST_SWORD * HERO_SCALE), ELBOW_R),
            FpPart::ForearmL => {
                solve_arm(&mut tf, lp + lq * (WRIST_SHIELD * HERO_SCALE * SHIELD_SCALE), ELBOW_L)
            }
        }
    }

    // Camera lean for the swing (applied by `player_camera`; a lean, never a shake).
    fp.sway = lean * draw;
}

/// One-bone IK: stand the forearm at `wrist`, reaching toward `elbow`, stretched so it always
/// spans the gap (the far end stays off-screen below the lens).
fn solve_arm(tf: &mut Transform, wrist: Vec3, elbow: Vec3) {
    let to = elbow - wrist;
    let dist = to.length().max(1e-3);
    tf.translation = wrist;
    tf.rotation = item_rot(to / dist, 0.0);
    let stretch = (dist / (FOREARM_LEN * HERO_SCALE)).clamp(1.0, 2.6);
    tf.scale = Vec3::new(HERO_SCALE, HERO_SCALE * stretch, HERO_SCALE);
}

// ── Reticle ────────────────────────────────────────────────────────────────────────────

#[derive(Component)]
pub(crate) struct ReticleRoot;
#[derive(Component)]
pub(crate) struct ReticleDot;
#[derive(Component)]
pub(crate) struct ReticleRing;
#[derive(Component)]
pub(crate) struct ReticleHpTrack;
#[derive(Component)]
pub(crate) struct ReticleHpFill;

const RING: f32 = 26.0;

/// `sync_reticle`'s frame-to-frame memory: ring engagement ease, the eased HP fill, which foe that
/// fill belongs to (so a new target snaps rather than sweeping in from the old one), and whether a
/// capture harness is running.
#[derive(Default)]
pub(crate) struct ReticleMem {
    engaged: f32,
    shown_hp: f32,
    shown_for: Option<Entity>,
    capturing: Option<bool>,
}
const HP_TRACK_W: f32 = 70.0;
const HP_TRACK_H: f32 = 6.0;

/// The foe whose vitals the reticle is currently showing (None when the reticle is hidden or the
/// foe is unhurt). `combat_fx::drive_hp_bars` hides that foe's world-space bar — in FP melee it
/// would stripe across the foe's chest right behind the crosshair — so the HP is shown exactly once.
#[derive(Resource, Default)]
pub struct ReticleTarget(pub Option<Entity>);

/// A tiny centre reticle for first person: a soft dot that blooms into a ring when a foe is in
/// swinging range. Melee aims down the view axis in FP, so the player needs to see where "centre" is.
pub(crate) fn spawn_reticle(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                display: Display::None,
                ..default()
            },
            GlobalZIndex(12),
            bevy::ui::FocusPolicy::Pass,
            ReticleRoot,
            CampaignOnly,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-RING * 0.5),
                    top: Val::Px(-RING * 0.5),
                    width: Val::Px(RING),
                    height: Val::Px(RING),
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(Color::NONE),
                ReticleRing,
            ));
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-2.0),
                    top: Val::Px(-2.0),
                    width: Val::Px(4.0),
                    height: Val::Px(4.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(rgb(255, 240, 205)),
                BorderColor::all(Color::srgba(0.05, 0.04, 0.03, 0.75)),
                ReticleDot,
            ));
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-HP_TRACK_W * 0.5),
                    top: Val::Px(RING * 0.5 + 9.0),
                    width: Val::Px(HP_TRACK_W),
                    height: Val::Px(HP_TRACK_H),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.03, 0.03, 0.0)),
                BorderColor::all(Color::NONE),
                ReticleHpTrack,
            ))
            .with_children(|t| {
                t.spawn((
                    Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
                    BackgroundColor(Color::NONE),
                    ReticleHpFill,
                ));
            });
        });
}

#[allow(clippy::type_complexity)]
pub(crate) fn sync_reticle(
    time: Res<Time>,
    mode: Res<PlayMode>,
    app: Res<State<AppState>>,
    fp: Res<FirstPerson>,
    orbit: Res<OrbitCam>,
    player: Res<PlayerRes>,
    hero_q: Query<(&Hero, &HeroHealth)>,
    mut root_q: Query<&mut Node, (With<ReticleRoot>, Without<ReticleRing>, Without<ReticleDot>)>,
    mut ring_q: Query<(&mut Node, &mut BorderColor), (With<ReticleRing>, Without<ReticleDot>)>,
    mut dot_q: Query<&mut BackgroundColor, (With<ReticleDot>, Without<ReticleHpTrack>, Without<ReticleHpFill>)>,
    mut track_q: Query<(&mut BackgroundColor, &mut BorderColor), (With<ReticleHpTrack>, Without<ReticleRing>)>,
    mut fill_q: Query<(&mut Node, &mut BackgroundColor), (With<ReticleHpFill>, Without<ReticleHpTrack>, Without<ReticleRing>, Without<ReticleDot>, Without<ReticleRoot>)>,
    vitals_q: Query<(Option<&Health>, Option<&NpcHp>)>,
    mut target: ResMut<ReticleTarget>,
    mut mem: Local<ReticleMem>,
) {
    target.0 = None;
    let Ok(mut root) = root_q.single_mut() else { return };
    // A capture harness can't pointer-lock, so it shows the reticle regardless.
    let capturing = *mem
        .capturing
        .get_or_insert_with(|| std::env::var_os("FOREST_SHOT").is_some() || std::env::var_os("FOREST_CLIP").is_some());
    let Ok((hero, hh)) = hero_q.single() else { return };
    let on = *mode == PlayMode::Play
        && *app.get() == AppState::Playing
        && fp.active
        && fp.blend > 0.9
        && (orbit.locked || capturing)
        && player.0.is_alive();
    root.display = if on { Display::Flex } else { Display::None };
    if !on {
        mem.engaged = 0.0;
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let want = if hero.soft_pos.is_some() { 1.0 } else { 0.0 };
    mem.engaged += (want - mem.engaged) * (1.0 - (-dt * 12.0).exp());
    // The ring snaps in on a swing and contracts onto the dot as the blow lands.
    let swing_pulse = if hero.attacking {
        let p = (hero.attack_t / hero.attack_dur.max(1e-3)).clamp(0.0, 1.0);
        (1.0 - (p / 0.45).clamp(0.0, 1.0)) * 0.35
    } else {
        0.0
    };
    if let Ok((mut node, mut border)) = ring_q.single_mut() {
        let size = RING * (0.72 + 0.28 * mem.engaged + swing_pulse);
        node.width = Val::Px(size);
        node.height = Val::Px(size);
        node.left = Val::Px(-size * 0.5);
        node.top = Val::Px(-size * 0.5);
        let a = 0.85 * mem.engaged;
        *border = BorderColor::all(Color::srgba(1.0, 0.42, 0.30, a));
    }
    if let Ok(mut bg) = dot_q.single_mut() {
        bg.0 = if hh.blocking { rgb(190, 215, 255) } else { rgb(255, 240, 205) }.with_alpha(0.92);
    }

    // The ringed foe's health, once it's hurt: a slim bar under the ring that eases down with each
    // blow and fades with the ring.
    let ratio = hero.soft_target.and_then(|e| {
        let (health, npc) = vitals_q.get(e).ok()?;
        let (hp, max) = match (health, npc) {
            (Some(h), _) => (h.hp, h.max),
            (_, Some(n)) => (n.hp, n.max),
            _ => return None,
        };
        (max > 0.0 && hp < max).then(|| (hp / max).clamp(0.0, 1.0))
    });
    let show = ratio.is_some() && mem.engaged > 0.5;
    if show {
        target.0 = hero.soft_target;
    }
    let a = if show { ((mem.engaged - 0.5) * 2.0).clamp(0.0, 1.0) } else { 0.0 };
    if let Some(r) = ratio {
        if mem.shown_for != hero.soft_target {
            mem.shown_for = hero.soft_target;
            mem.shown_hp = r;
        }
        mem.shown_hp += (r - mem.shown_hp) * (1.0 - (-dt * 14.0).exp());
    } else {
        mem.shown_for = None;
    }
    if let Ok((mut bg, mut border)) = track_q.single_mut() {
        bg.0 = Color::srgba(0.05, 0.03, 0.03, 0.62 * a);
        *border = BorderColor::all(Color::srgba(0.02, 0.01, 0.01, 0.85 * a));
    }
    if let Ok((mut node, mut bg)) = fill_q.single_mut() {
        node.width = Val::Percent(mem.shown_hp * 100.0);
        bg.0 = Color::srgba(0.86, 0.22, 0.16, a);
    }
}
