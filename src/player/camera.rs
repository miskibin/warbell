//! Third-person camera + the free-roam debug toggle. Ported from `MouseLookCamera.tsx`:
//! an over-the-shoulder orbit (azimuth / pitch / dist) with pointer-lock — click to lock the
//! cursor (mouse then rotates the view), Esc to release, wheel to zoom.
//!
//! The backtick key flips [`PlayMode`]: in **FreeRoam** this system yields and
//! `controls::fly_camera` drives the camera instead (for debugging).

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::controls::FlyCam;
use crate::game_state::{AppState, Modal};

use super::{Hero, PlayMode};

/// The "is the cursor interactive / where should the camera frame" inputs, bundled to keep
/// `player_camera` under Bevy's 16-param system cap. Build mode flips the camera up over the town
/// and frees the cursor (`build_mode.active`).
#[derive(SystemParam)]
pub struct CamGate<'w, 's> {
    app: Res<'w, State<AppState>>,
    modal: Option<Res<'w, State<Modal>>>,
    egui_wants: Res<'w, crate::debug_panel::EguiWantsPointer>,
    build_mode: Res<'w, crate::town::BuildMode>,
    /// Set while a warden rears back for a killing blow — eases a tension dolly-out (see below).
    crit_tension: Option<Res<'w, crate::boss::CritTension>>,
    /// The succession beat — when active, the camera is pulled to frame the peasant being possessed.
    succ: Res<'w, crate::succession::Succession>,
    /// Solid buildings the follow boom collides with (see [`super::boom`]).
    occ: super::boom::Occluders<'w, 's>,
    /// Build-cam framing inputs: the producer plots, the next house site (via the town's house
    /// count), the docked palette's laid-out height and the window — see [`CamGate::build_pose`].
    spots: Option<Res<'w, crate::town::PlotSpots>>,
    town: Option<Res<'w, crate::town::TownRes>>,
    palette: Query<'w, 's, &'static ComputedNode, With<crate::town::BuildUi>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    /// The eased build-cam `(eye, look)` (a newly revealed house site or the palette's first layout
    /// pass glides the framing instead of cutting it).
    build_frame: Local<'s, Option<(Vec3, Vec3)>>,
}

impl CamGate<'_, '_> {
    /// The build-mode camera pose, fitted every frame to the REAL town bounds (see
    /// [`build_frame_points`]) for the current lens + window: the whole enceinte, every producer plot
    /// and the next house site fit between the top HUD strip and the docked build palette, at any
    /// aspect / scale factor. `snap` (entering build mode) skips the ease.
    fn build_pose(&mut self, proj: &Projection, dt: f32, snap: bool) -> (Vec3, Vec3) {
        let (fov, aspect) = match proj {
            Projection::Perspective(p) => (p.fov, p.aspect_ratio),
            _ => (50f32.to_radians(), 16.0 / 9.0),
        };
        let win_h = self.windows.single().map_or(720.0, |w| w.height()).max(1.0);
        // The palette root's laid-out height (physical px → logical), plus its bottom inset; before its
        // first layout pass, a measured-once estimate.
        let measured = self
            .palette
            .iter()
            .map(|n| n.size().y * n.inverse_scale_factor())
            .fold(0.0_f32, f32::max);
        let palette_px = if measured > 1.0 { measured + BUILD_PALETTE_INSET_PX } else { BUILD_PALETTE_FALLBACK_PX };
        let bottom = ((palette_px + 8.0) / win_h).min(0.55);
        let top = (BUILD_CAM_TOP_PX / win_h).min(0.2);
        let house = self.town.as_ref().and_then(|t| crate::castle::next_house_site(t.0.houses));
        let plots = self.spots.as_ref().map_or(&[][..], |s| s.0.as_slice());
        let pts = build_frame_points(plots, house);
        let ground = crate::worldmap::ground_at_world(0.0, 0.0).unwrap_or(0.0);
        let target = fit_build_cam(&pts, fov, aspect, bottom, top, ground);
        let k = if snap || self.build_frame.is_none() { 1.0 } else { 1.0 - (-dt * 5.0).exp() };
        let (e, l) = self.build_frame.unwrap_or(target);
        let pose = (e.lerp(target.0, k), l.lerp(target.1, k));
        *self.build_frame = Some(pose);
        pose
    }
}

const SENS_X: f32 = 0.0035;
const SENS_Y: f32 = 0.0016;
const MIN_PITCH: f32 = 0.06;
const MAX_PITCH: f32 = 1.45;
const MIN_DIST: f32 = 1.9;
const MAX_DIST: f32 = 11.0;
const ZOOM_SENS: f32 = 0.55;
/// Witcher-style over-the-shoulder framing: the look target (and with it the whole orbit) is
/// shifted this far to the camera-right, so the hero stands left-of-centre and the view looks
/// past his sword shoulder instead of pinning him dead-centre like a turret. Fades out with the
/// first-person blend (FP aims through the eyes, no shoulder bias).
const SHOULDER_X: f32 = 0.42;
/// Minimum gap kept between the third-person eye and the terrain below it. When the orbit would sink
/// the camera into a hill/slope between the hero and the lens, the eye is lifted to ground + this so
/// it never punches under the world (the "camera dips below the hill" bug). Covers the 0.04 near
/// plane plus a little body so geometry doesn't poke into frame.
const CAM_GROUND_CLEAR: f32 = 0.5;
/// Extra follow distance pulled back (smoothly) while a warden winds up its killing blow — a slow
/// dolly-out that builds tension across the telegraph, then eases back in once the blow resolves.
const CRIT_ZOOM_OUT: f32 = 4.5;
/// Look-target height above the hero's feet. Aims at the knight's upper chest / shoulder line
/// (Witcher-style — drops the horizon lower in frame so more of the world reads over his
/// shoulder), NOT above the helm — an above-the-head target shoves the hero toward the bottom of
/// frame. (Tracks the footman: shoulder sits ≈1.14 world-units up, anchor a touch below it.)
const EYE_H: f32 = 1.03;

/// First-person eye height above the hero's feet — sits right at the helm/eye line. (Scaled ×1.5
/// alongside the `HERO_SCALE` bump; an eye floating above the helm reads as "too tall" and pushes the
/// viewmodel's hands off the bottom of frame.) Original 0.74 × 1.35. Verify with `FOREST_FP`.
const FP_EYE_H: f32 = 1.32;
/// First-person forward eye offset (world units along the look direction): nudges the eye a touch
/// ahead of the (hidden) body centre so it sits at the face, not inside the chest.
const FP_FWD_OFF: f32 = 0.05;
/// How far in front of the rig's head joint the FP eye sits (face, not skull centre — the head
/// itself is hidden in first person).
const FP_HEAD_FWD: f32 = 0.09;
/// How far above the head joint the FP eye sits (world units). The joint is the neck pivot, ~0.11
/// below the visor slit: an eye left on it looked out of the knight's throat, so every foe of his
/// own height stood a head taller in first person.
const FP_HEAD_UP: f32 = 0.11;
/// `(forward, up)` seat of the FP eye off the head joint; `FOREST_FPHEAD="fwd,up"` overrides.
fn fp_head_seat() -> (f32, f32) {
    static CELL: std::sync::OnceLock<(f32, f32)> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| {
        let v: Vec<f32> = std::env::var("FOREST_FPHEAD")
            .ok()
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        (v.first().copied().unwrap_or(FP_HEAD_FWD), v.get(1).copied().unwrap_or(FP_HEAD_UP))
    })
}

/// FP shield guard: how far the raised shield's shoulder is moved out to the shield side and down
/// (rig units) while blocking. The third-person guard holds the board square in front of the face —
/// from the eye that is a wall of planks (and the fist on its grip) over the middle of the screen.
/// KCD-style, first person carries it in the lower-left: the foe stays in view over its rim.
/// `FOREST_FPGUARD="out,down"` overrides.
fn fp_guard_shift() -> (f32, f32) {
    static CELL: std::sync::OnceLock<(f32, f32)> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| {
        let v: Vec<f32> = std::env::var("FOREST_FPGUARD")
            .ok()
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        (v.first().copied().unwrap_or(0.16), v.get(1).copied().unwrap_or(0.14))
    })
}

/// FP arm reach `(forward, down)` in rig units; `FOREST_FPARM="fwd,down"` overrides.
fn fp_arm_reach() -> (f32, f32) {
    static CELL: std::sync::OnceLock<(f32, f32)> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| {
        let v: Vec<f32> = std::env::var("FOREST_FPARM")
            .ok()
            .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_default();
        (v.first().copied().unwrap_or(0.25), v.get(1).copied().unwrap_or(0.12))
    })
}

/// How far the FP look-pitch bends the rig's torso (so the arms, sword and shield follow the view
/// up and down instead of leaving the frame).
const FP_SPINE_AIM: f32 = 0.55;
/// How much of a sprint's forward trunk lean first person cancels (1 = fully upright).
const FP_SPRINT_UPRIGHT: f32 = 0.9;
/// First-person look-pitch clamp (radians): how far you can crane up/down. Symmetric, unlike the
/// third-person `MIN/MAX_PITCH` (which is camera *elevation*, always tilting the view downward).
const FP_PITCH_LIMIT: f32 = 1.3;
/// First-person close-quarters FOV widen: up to this many degrees as the ringed foe closes from
/// [`FP_CLOSE_RANGE`] to point-blank. Enemies are taller than the 1.32 FP eye and stop at melee
/// range, so without this a big foe is one face filling the lens. Eased slowly (~3/s) so it reads
/// as breathing room, never a zoom pump; scaled by the FP blend so third person is untouched.
const FP_CLOSE_FOV_DEG: f32 = 12.0;
const FP_CLOSE_RANGE: f32 = 3.5;
/// With the widen, the FP eye eases back this far (world units, level, away from the look) as the
/// ringed foe closes — inside the hidden helm, so nothing of the knight comes into view. Lens and
/// step back together keep a club-swinging ork at point-blank about a third smaller on screen
/// without a fish-eye lens.
const FP_CLOSE_PULL: f32 = 0.18;
/// Resting first-person vertical FOV. Wider than the 50° third-person lens: at the eye you want
/// peripheral vision, and the viewmodel's screen-space poses are authored against this value.
pub const FP_FOV_DEG: f32 = 62.0;
/// How fast the FP eye height follows the ground while grounded (1/s). Low on purpose: terraced
/// terrain snaps `hero.y` by 0.5u per tile edge, and a stiff follow turns every step into a hitch.
const FP_VERT_RATE: f32 = 11.0;

/// Build-mode camera: a fixed north-up heading (the eye south of the town, looking north) tilted
/// this far below the horizon (≈60°). The pose itself is NOT a constant — the old hand-placed
/// `(0, 30, 22) → (0, 0, 5)` framed the 17×12 castle and lost most of the enlarged enceinte, its
/// outer plots and the village house sites; [`fit_build_cam`] derives it from the real bounds.
const BUILD_CAM_PITCH: f32 = 1.05;
/// Slack kept between the framed town and the usable band's edges (NDC units, of a 2-unit screen).
const BUILD_CAM_MARGIN: f32 = 0.03;
/// Logical px reserved at the top of the screen for the compass strip.
const BUILD_CAM_TOP_PX: f32 = 64.0;
/// The build palette's bottom inset (`town::spawn_build_strip`'s `bottom: 22px`).
const BUILD_PALETTE_INSET_PX: f32 = 22.0;
/// The palette's height (logical px, inset included) assumed before its first layout pass.
const BUILD_PALETTE_FALLBACK_PX: f32 = 250.0;

/// World points the build camera must keep in frame, each padded by its footprint radius and taken
/// at the ground and at roof height: every enceinte corner tower, the keep, the three gates (and the
/// ballista outside the N postern), every producer plot (`town::PlotSpots`) and the next house site
/// in the street village (`castle::next_house_site`). So the framing follows the castle polygon, the
/// plots and the village rather than a hand-placed pose.
pub(crate) fn build_frame_points(plots: &[Vec2], house: Option<Vec2>) -> Vec<Vec3> {
    use crate::castle;
    let mut pts = Vec::with_capacity(8 * (castle::ENCEINTE.len() + plots.len() + 6));
    let mut push = |c: Vec2, r: f32, h: f32| {
        let g = crate::worldmap::ground_at_world(c.x, c.y).unwrap_or(0.0);
        for o in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let p = c + o * r;
            pts.push(Vec3::new(p.x, g, p.y));
            pts.push(Vec3::new(p.x, g + h, p.y));
        }
    };
    for v in castle::ENCEINTE {
        push(v, 3.5, 8.0); // corner towers (kit towers stand ~7–8u)
    }
    push(castle::KEEP_POS, 5.0, castle::KEEP_ROOF_Y + 1.5);
    for gate in castle::Gate::ALL {
        push(gate.center(), 5.0, 5.0); // gatehouse + the ballista flanking the N postern
    }
    for &p in plots {
        push(p, crate::town::PLOT_CLEAR_R, 4.0);
    }
    if let Some(h) = house {
        push(h, 4.5, 5.0);
    }
    pts
}

/// Fit the build camera to `pts` for a lens of vertical FOV `fov_y` and `aspect`: the heading and
/// [`BUILD_CAM_PITCH`] are fixed, and the eye is pulled back / slid until every point projects
/// inside the usable band — full width, minus `bottom` (the docked palette) and `top` (the compass)
/// as fractions of the screen height — then centred in its slack. For a fixed orientation the
/// "point inside the frustum band" tests are linear in the eye offset, so this bisects the dolly
/// and reads the slide straight off the tightest constraints. Returns `(eye, look)`, `look` on the
/// plane `ground_y` (so a blend from the follow pose eases sensibly).
pub(crate) fn fit_build_cam(pts: &[Vec3], fov_y: f32, aspect: f32, bottom: f32, top: f32, ground_y: f32) -> (Vec3, Vec3) {
    let (sp, cp) = BUILD_CAM_PITCH.sin_cos();
    let f = Vec3::new(0.0, -sp, -cp); // forward: north and down
    let r = Vec3::X; // forward × up
    let u = Vec3::new(0.0, cp, -sp); // right × forward
    let ty = (fov_y * 0.5).tan();
    let kx = ty * aspect.max(0.1) * (1.0 - BUILD_CAM_MARGIN);
    // NDC band (y up): above the palette, below the top strip — never thinner than 0.4.
    let ndc_top = 1.0 - 2.0 * top - BUILD_CAM_MARGIN;
    let ndc_bot = (-1.0 + 2.0 * bottom + BUILD_CAM_MARGIN).min(ndc_top - 0.4);
    let (ylo, yhi) = (ndc_bot * ty, ndc_top * ty);
    if pts.is_empty() {
        return (Vec3::new(0.0, 30.0, 22.0), Vec3::new(0.0, ground_y, 5.0));
    }
    let centroid = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
    // Eye = e0 + a·r + b·u + t·f, so a point's camera coords are (X − a, Y − b, Z − t).
    let e0 = centroid - f * 2000.0;
    let cam: Vec<Vec3> = pts
        .iter()
        .map(|&p| {
            let v = p - e0;
            Vec3::new(v.dot(r), v.dot(u), v.dot(f))
        })
        .collect();
    let bounds = |t: f32| {
        let (mut alo, mut ahi, mut blo, mut bhi) = (f32::MIN, f32::MAX, f32::MIN, f32::MAX);
        for c in &cam {
            let z = c.z - t;
            alo = alo.max(c.x - kx * z);
            ahi = ahi.min(c.x + kx * z);
            blo = blo.max(c.y - yhi * z);
            bhi = bhi.min(c.y - ylo * z);
        }
        (alo, ahi, blo, bhi)
    };
    let fits = |t: f32| {
        let (alo, ahi, blo, bhi) = bounds(t);
        alo <= ahi && blo <= bhi
    };
    // Bisect the dolly: `lo` always fits (2000u back), `hi` is just short of the nearest point.
    let zmin = cam.iter().map(|c| c.z).fold(f32::INFINITY, f32::min);
    let (mut lo, mut hi) = (0.0_f32, zmin - 1.0);
    if fits(hi) {
        lo = hi;
    } else {
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if fits(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    let (alo, ahi, blo, bhi) = bounds(lo);
    let eye = e0 + r * (0.5 * (alo + ahi)) + u * (0.5 * (blo + bhi)) + f * lo;
    let look = eye + f * ((eye.y - ground_y) / sp);
    (eye, look)
}

#[derive(Resource)]
pub struct OrbitCam {
    pub azimuth: f32,
    pub pitch: f32,
    pub dist: f32,
    pub locked: bool,
    /// Low-passed look-ahead lead (world XZ), eased toward the hero's heading×moving so a direction
    /// change slides the framing instead of snapping. Kept here (not a new system `Local`) because the
    /// camera system is already at Bevy's 16-param ceiling.
    pub lead: Vec3,
    /// Smoothed follow anchor (world, eye-height) — eased toward the hero so the rig glides behind him
    /// instead of rigidly tracking his exact position every frame. Snaps on a big jump (spawn/load).
    pub anchor: Vec3,
    /// Smoothed combat dolly-out (world units) — eased toward the stance × threat-count target so
    /// a foe wandering across the tally line can't pump the zoom. Kept here (not a system `Local`)
    /// because `player_camera` sits at Bevy's 16-param ceiling.
    pub combat: f32,
    /// Boom-collision state (pulled-in length, hold timer, allowed shoulder) — see [`super::boom`].
    pub boom: super::boom::BoomState,
}

impl Default for OrbitCam {
    fn default() -> Self {
        OrbitCam {
            // Boot the orbit looking the way the hero faces (over his shoulder, at the castle):
            // view dir = -(sin az, cos az) so az = facing + π. The old hand-tuned 0.85π matched
            // the retired north-gate spawn; deriving from `spawn_point` keeps the opening frame
            // on the keep if the spawn ever moves again.
            azimuth: crate::player::spawn_point().1 + std::f32::consts::PI,
            // Witcher-style default pose: flatter pitch (near-level, looking along the world
            // rather than down at the hero's back) and a closer follow so he reads large in frame.
            pitch: 0.26,
            dist: 3.3,
            locked: false,
            lead: Vec3::ZERO,
            anchor: Vec3::ZERO,
            combat: 0.0,
            boom: Default::default(),
        }
    }
}

/// Camera-glide ease rate (1/s) for the follow anchor — subtle (flows behind him, not laggy).
const GLIDE_RATE: f32 = 13.0;
/// Separate, much slower ease rate (1/s) for the anchor's **vertical** component. Terrain is
/// terraced (`worldmap::GROUND_STEP` = 0.5), so `hero.y` snaps a half-unit at every tile edge the
/// hero walks up/down. Easing Y at the full `GLIDE_RATE` tracks those snaps near-1:1 and the camera
/// lurches behind him; a slow vertical glide averages the steps into a smooth float. XZ stays at
/// `GLIDE_RATE` so the horizontal follow is still snappy. Only used while grounded — a real jump/fall
/// keeps the fast rate so the camera doesn't lag a genuine vertical move (see `player_camera`).
const VERT_GLIDE_RATE: f32 = 4.0;
/// Sprint "speed feel": how far the camera dollies back (world units) + how much the FOV widens
/// (degrees) at full run, eased by `hero.run_amt`.
const SPRINT_DOLLY: f32 = 0.38;
const SPRINT_FOV_DEG: f32 = 5.0;

// ── Combat framing (the stance's camera) ──
// The cardinal rule (GDC "50 Camera Mistakes"): the camera must NEVER fight the mouse — azimuth
// stays 100% player-owned even in combat (auto-rotation + strafe steering is the classic spin
// feedback loop). Instead the stance gets two gentle, non-rotational biases: the LOOK-TARGET
// slides toward the ringed foe so both fighters share the frame, and the rig DOLLIES BACK a
// touch, more as the scrap grows (the Witcher-mod "camera preset by enemy count" pattern).
/// How far the framing slides toward the foe at full stance (world units; cf. the 0.42 shoulder).
const COMBAT_LEAD: f32 = 0.55;
/// Dolly-out (world units) by threat tier: a duel / a pair / a mob.
const COMBAT_DOLLY_DUEL: f32 = 0.35;
const COMBAT_DOLLY_GROUP: f32 = 0.9;
const COMBAT_DOLLY_MOB: f32 = 1.3;
/// FOV widen (degrees) per world-unit of combat dolly — a slight wide-angle as the fight grows.
const COMBAT_FOV_PER_UNIT: f32 = 2.0;

/// First-person sub-mode of [`PlayMode::Play`] (you still drive the knight). Toggled by the HUD
/// eye button / **V** key. `blend` eases the third⇄first transition (a smooth dolly-in, never a
/// hard cut). `pitch` is the FP look-pitch, kept SEPARATE from [`OrbitCam::pitch`] (which is camera
/// elevation and always tilts the view down); `azimuth` is shared with the orbit so your heading
/// survives the toggle. Transient — not saved (a view preference, like the debug panel).
#[derive(Resource, Default)]
pub struct FirstPerson {
    pub active: bool,
    pub blend: f32,
    pub pitch: f32,
    /// FP swing camera-sway (screen-space radians: x = pitch(+up), y = yaw(+left), z = roll),
    /// written by `viewmodel::animate_viewmodel` from the attack envelopes and applied here on top
    /// of the settled FP pose — one smooth lean per cut (anticipation pulls opposite, the strike
    /// rides the blade), NOT a shake. Zeroed outside FP.
    pub sway: Vec3,
    /// Smoothed eye rig (ground-following height, landing dip, stride bob, strafe lean).
    pub eye: FpEye,
    /// Smoothed FP close-quarters FOV widen (degrees). A `WORLD_BUMP`-scaled ork at melee range
    /// fills the whole first-person frame (face-in-lens); this eases a modest wide-angle in as
    /// the ringed foe closes, buying back its silhouette + HP bar. See `FP_CLOSE_FOV_DEG`.
    pub close_fov: f32,
    /// The hero rig's head joint relative to the hero root (world-space offset, last frame,
    /// low-passed) — first person looks out of the real third-person body's head.
    pub head_off: Option<Vec3>,
}

/// Per-frame memory for the first-person eye: a low-passed ground height (so terrace steps glide),
/// a spring-driven landing dip, the eased stride amount, and the smoothed strafe roll.
#[derive(Default)]
pub struct FpEye {
    y: f32,
    primed: bool,
    land: f32,
    land_v: f32,
    was_air: bool,
    stride: f32,
    roll: f32,
    dive: f32,
}

/// What the eye rig hands back to `player_camera`: where the eye is, and the small extra rotation
/// (x pitch(+up), y yaw(+left), z roll) composed on top of the look direction.
struct FpEyePose {
    pos: Vec3,
    lean: Vec3,
}

/// Critically-damped-ish spring step (semi-implicit Euler), shared with the viewmodel's layers.
pub(super) fn spring(pos: &mut f32, vel: &mut f32, target: f32, omega: f32, zeta: f32, dt: f32) {
    let acc = (target - *pos) * omega * omega - *vel * 2.0 * zeta * omega;
    *vel += acc * dt;
    *pos += *vel * dt;
}

/// Advance the eye rig one frame. Height glides over terraced ground and follows a jump at full
/// rate; landings compress the eye and spring back; stride adds a figure-eight bob; strafing rolls
/// the view a hair into the turn; a dodge roll dips the head and pitches it forward.
fn fp_eye_rig(eye: &mut FpEye, hero: &Hero, yaw: f32, dt: f32, head_off: Option<Vec3>) -> FpEyePose {
    let dt = dt.clamp(1e-4, 0.05);
    let raw_y = hero.y + FP_EYE_H;
    if !eye.primed || (eye.y - raw_y).abs() > 2.5 {
        eye.y = raw_y;
        eye.primed = true;
    } else {
        let rate = if hero.on_ground { FP_VERT_RATE } else { 38.0 };
        eye.y += (raw_y - eye.y) * (1.0 - (-dt * rate).exp());
    }

    if hero.on_ground && eye.was_air {
        eye.land_v -= (hero.vel_y.abs() * 0.04).clamp(0.08, 0.6);
    }
    eye.was_air = !hero.on_ground;
    spring(&mut eye.land, &mut eye.land_v, 0.0, 24.0, 0.42, dt);

    let grounded = if hero.on_ground { 1.0 } else { 0.0 };
    let stride_t = hero.moving_amt.clamp(0.0, 1.0) * grounded;
    eye.stride += (stride_t - eye.stride) * (1.0 - (-dt * 10.0).exp());
    let run = 1.0 + hero.run_amt.clamp(0.0, 1.0) * 0.5;
    let k = eye.stride * run;
    let ph = hero.walk_phase;
    let bob_y = (ph * 2.0).sin() * 0.016 * k;
    let bob_x = ph.sin() * 0.011 * k;
    let bob_roll = ph.sin() * 0.006 * k;
    let bob_pitch = (ph * 2.0 + 0.6).sin() * 0.0035 * k;

    let (ys, yc) = yaw.sin_cos();
    let right = Vec2::new(-yc, ys);
    let lateral = hero.vel.dot(right);
    let roll_t = (-lateral * 0.0042).clamp(-0.03, 0.03);
    eye.roll += (roll_t - eye.roll) * (1.0 - (-dt * 8.0).exp());

    let dive_t = if hero.roll_t >= 0.0 {
        let u = (hero.roll_t / super::movement::ROLL_TIME).clamp(0.0, 1.0);
        let s = |t: f32| {
            let c = t.clamp(0.0, 1.0);
            c * c * (3.0 - 2.0 * c)
        };
        s(u / 0.2) * s((1.0 - u) / 0.25)
    } else {
        0.0
    };
    eye.dive += (dive_t - eye.dive) * (1.0 - (-dt * 14.0).exp());

    if let Some(off) = head_off {
        // Riding the real body's head: its walk / roll / landing already move the eye, so no
        // synthetic bob or dip — only the strafe roll. The terrace low-pass still applies to the
        // root height so stepping up a terrace glides.
        let (fwd, up) = fp_head_seat();
        let pos = Vec3::new(hero.pos.x, eye.y - FP_EYE_H, hero.pos.y) + off + Vec3::new(ys, 0.0, yc) * fwd + Vec3::Y * up;
        return FpEyePose { pos, lean: Vec3::new(0.0, 0.0, eye.roll) };
    }
    let pos = Vec3::new(
        hero.pos.x + ys * FP_FWD_OFF + right.x * bob_x,
        eye.y + bob_y + eye.land - eye.dive * 0.42,
        hero.pos.y + yc * FP_FWD_OFF + right.y * bob_x,
    );
    FpEyePose { pos, lean: Vec3::new(bob_pitch - eye.dive * 0.5, 0.0, eye.roll + bob_roll) }
}

/// Backtick toggles Play ↔ FreeRoam. Leaving Play frees the cursor and syncs the fly-cam's
/// yaw/pitch to the current view so it doesn't snap when it takes over.
pub fn toggle_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<PlayMode>,
    mut orbit: ResMut<OrbitCam>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cam_q: Query<(&Transform, &mut FlyCam)>,
) {
    if !keys.just_pressed(KeyCode::Backquote) {
        return;
    }
    *mode = match *mode {
        PlayMode::Play => PlayMode::FreeRoam,
        PlayMode::FreeRoam => PlayMode::Play,
    };
    if *mode == PlayMode::FreeRoam {
        if let Ok(mut cur) = cursor_q.single_mut() {
            cur.grab_mode = CursorGrabMode::None;
            cur.visible = true;
        }
        orbit.locked = false;
        if let Ok((tf, mut fly)) = cam_q.single_mut() {
            let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
            fly.yaw = yaw;
            fly.pitch = pitch;
            // Sync smoothing targets + clear momentum so the fly-cam takes over at rest
            // instead of easing toward a stale target or drifting from leftover velocity.
            fly.target_yaw = yaw;
            fly.target_pitch = pitch;
            fly.vel = Vec3::ZERO;
        }
    }
}

/// **V** flips first-person on/off (the HUD eye button writes the same flag). Ungated like the
/// other view toggles so it works through pauses/panels; the pose change only takes effect in
/// `player_camera`, which runs only in `PlayMode::Play`.
pub fn toggle_first_person(
    keys: Res<ButtonInput<KeyCode>>,
    mut fp: ResMut<FirstPerson>,
    mut notice: ResMut<crate::ui::notice::Notice>,
    time: Res<Time>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        fp.active = !fp.active;
        notice.push(if fp.active { "First person" } else { "Third person" }, time.elapsed_secs_f64());
    }
}

/// First-person body visibility: the entire third-person hero is hidden once the eye is inside it
/// (the hands and weapon you see are the separate [`super::viewmodel`]). Hiding is edge-triggered on
/// `fp.blend` plus a sweep of freshly spawned leaves (an equip reskin rebuilds the rig mid-FP), so
/// the per-frame cost is zero and other systems' visibility writes (the Director's weapon hide) are
/// never fought in third person. The Director's weapon policy runs after this sweep and also
/// respects `fp.blend`, keeping the world sword hidden between transition edges.
pub fn fp_body_visibility(
    fp: Res<FirstPerson>,
    hero_q: Query<&Hero>,
    mut parts: Query<(&super::HeroPart, &mut Visibility), Without<super::HeroMesh>>,
    mut leaves: Query<(&ChildOf, &mut Visibility), With<super::HeroMesh>>,
    joints: Query<&super::HeroPart>,
    mut running: Local<bool>,
) {
    use super::Joint;
    let want = if fp.blend > 0.5 { Visibility::Hidden } else { Visibility::Inherited };
    // At a run (weapons always sheathed) the body pumps through the bottom of the lens: the bare
    // forearms swing forward and up toward the midline, and the cuirass / fauld plates bob up into
    // the lower frame with every stride — dark slabs smeared to grain by motion blur. KCD-style,
    // the body simply leaves the view while running: hide the elbow subtrees (forearm, gauntlet,
    // hand) and the plates hung on the torso + hips. Hysteresis so a jog at the threshold doesn't
    // flicker the chest in and out.
    // Shift in a fight first puts the weapons away (and stopping draws them again): that reach swings
    // the bare forearms, gauntlets and elbow cops up over the shoulder, straight through the lens —
    // hidden for its length too; the kit simply comes up from below the frame when it ends.
    let mut reach = false;
    if let Ok(h) = hero_q.single() {
        let pace = h.gait_speed.max(h.run_amt * 5.0);
        *running = if *running { pace > 2.0 } else { pace > 2.6 };
        reach = h.kit_reach;
    }
    let run_hide = fp.blend > 0.5 && (*running || reach);
    let want_fore = if run_hide { Visibility::Hidden } else { Visibility::Inherited };
    for (p, mut vis) in &mut parts {
        let w = match p.joint {
            Joint::Head => want,
            Joint::ElbowL | Joint::ElbowR => want_fore,
            _ => continue,
        };
        if *vis != w {
            *vis = w;
        }
    }
    // The upper arms + pauldrons swing right past the eye in the third-person clips (a dark slab
    // across half the frame): always dropped in first person. The torso / hips plates only go at
    // a run (see above) — walking, a glance down still finds your own chest.
    for (parent, mut vis) in &mut leaves {
        let w = match joints.get(parent.parent()).map(|j| j.joint) {
            Ok(Joint::ShoulderL | Joint::ShoulderR) => want,
            Ok(Joint::Torso | Joint::Hips) => want_fore,
            _ => continue,
        };
        if *vis != w {
            *vis = w;
        }
    }
}

/// First person looks out of the real body: track the head joint's offset from the hero root
/// (low-passed so the walk's head bob reads as a gentle sway, not a shake), and bend the torso
/// with the look pitch so the arms keep the sword / shield in frame. Runs after the animator.
pub fn fp_head_track(
    time: Res<Time>,
    mut fp: ResMut<FirstPerson>,
    hero_q: Query<&GlobalTransform, With<Hero>>,
    hero_state: Query<(&Hero, &super::HeroHealth)>,
    mut parts: Query<(&super::HeroPart, &mut Transform, &GlobalTransform)>,
    mut guard: Local<f32>,
) {
    let Ok(root) = hero_q.single() else { return };
    // A sprint folds the trunk far forward, which carries the collar, shoulders and chest plate up
    // into the lens (the eye rides the head joint). First person keeps the trunk upright instead:
    // the gait still reads in the legs, bob and stride, the armour stays out of the frame.
    let run = hero_state.single().map_or(0.0, |(h, _)| ((h.gait_speed - 1.8) / 2.4).clamp(0.0, 1.0).max(h.run_amt));
    let dt = time.delta_secs().clamp(1e-4, 0.05);
    // Eased like the animator's block weight, so the board slides aside as it comes up.
    let blocking = hero_state.single().is_ok_and(|(_, hh)| hh.blocking);
    *guard += (if blocking { 1.0 } else { 0.0 } - *guard) * (dt * 10.0).min(1.0);
    let fpb = fp.blend.clamp(0.0, 1.0);
    let mut head = None;
    for (p, mut tf, gt) in &mut parts {
        match p.joint {
            super::Joint::Head => head = Some(gt.translation() - root.translation()),
            super::Joint::Torso if fp.blend > 0.01 => {
                let upright = FP_SPRINT_UPRIGHT * run * fp.blend.clamp(0.0, 1.0);
                if upright > 1e-3 {
                    let (x, y, z) = tf.rotation.to_euler(EulerRot::XYZ);
                    tf.rotation = Quat::from_euler(EulerRot::XYZ, x * (1.0 - upright), y, z);
                }
                tf.rotation *= Quat::from_rotation_x(-fp.pitch * FP_SPINE_AIM * fp.blend.clamp(0.0, 1.0));
            }
            // Arms reach a little further out and lower than the third-person clips author them, so
            // a raised guard / wind-up holds the gauntlet off the lens (KCD-style: the weapon sits
            // in the lower frame, the forearm never fills it).
            super::Joint::ShoulderL | super::Joint::ShoulderR if fp.blend > 0.01 => {
                let (fwd, down) = fp_arm_reach();
                tf.translation += Vec3::new(0.0, -down, fwd) * fpb;
                if p.joint == super::Joint::ShoulderL && *guard > 1e-3 {
                    let (out, low) = fp_guard_shift();
                    let side = tf.translation.x.signum();
                    tf.translation += Vec3::new(side * out, -low, 0.0) * *guard * fpb;
                }
            }
            _ => {}
        }
    }
    if let Some(h) = head {
        let k = 1.0 - (-dt * 10.0).exp();
        fp.head_off = Some(match fp.head_off {
            Some(o) if o.distance(h) < 1.0 => o + (h - o) * k,
            _ => h,
        });
    }
}

/// Model viewer first person (`FOREST_VIEW_FP=1`, `viewer.rs`): the eye on the previewed knight's
/// head exactly as [`player_camera`] seats it, looking along his facing at `FOREST_FP_PITCH` (rad),
/// at the FP lens — so FP framing (armour in the lens, a raised shield, a foe's size) is checked in
/// seconds instead of a world boot.
pub(crate) fn viewer_fp_cam(
    mut fp: ResMut<FirstPerson>,
    hero_q: Query<(&Hero, &GlobalTransform)>,
    mut cam_q: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    fp.active = true;
    fp.blend = 1.0;
    fp.pitch = std::env::var("FOREST_FP_PITCH").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let (Ok((hero, root)), Ok((mut tf, mut proj)), Some(off)) = (hero_q.single(), cam_q.single_mut(), fp.head_off)
    else {
        return;
    };
    let (ys, yc) = hero.facing.sin_cos();
    let (fwd, up) = fp_head_seat();
    let eye = root.translation() + off + Vec3::new(ys, 0.0, yc) * fwd + Vec3::Y * up;
    // A previewed foe (`FOREST_VIEW_FOE_DIST`) brings in the close-quarters widen + step back.
    let close = std::env::var("FOREST_VIEW_FOE").is_ok().then(|| {
        let d: f32 = std::env::var("FOREST_VIEW_FOE_DIST").ok().and_then(|v| v.parse().ok()).unwrap_or(1.6);
        ((FP_CLOSE_RANGE - d) / FP_CLOSE_RANGE).clamp(0.0, 1.0)
    });
    let close = close.unwrap_or(0.0);
    let eye = eye - Vec3::new(ys, 0.0, yc) * FP_CLOSE_PULL * close;
    let (ps, pc) = fp.pitch.sin_cos();
    tf.translation = eye;
    tf.look_at(eye + Vec3::new(ys * pc, ps, yc * pc), Vec3::Y);
    if let Projection::Perspective(p) = &mut *proj {
        p.fov = (FP_FOV_DEG + FP_CLOSE_FOV_DEG * close).to_radians();
        p.near = 0.04;
    }
}

pub fn player_camera(
    mode: Res<PlayMode>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut orbit: ResMut<OrbitCam>,
    mut fp: ResMut<FirstPerson>,
    mut cursor_q: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut hero_q: Query<&mut Hero>,
    mut cam_q: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<Hero>)>,
    time: Res<Time>,
    feedback: Option<Res<crate::combat_fx::HitFeedback>>,
    mut base_fov: Local<Option<f32>>,
    mut gate: CamGate,
    mut build_blend: Local<f32>,
    mut tension_blend: Local<f32>,
) {
    if *mode != PlayMode::Play {
        return;
    }
    let Ok(mut hero) = hero_q.single_mut() else { return };
    let Ok((mut cam_tf, mut cam_proj)) = cam_q.single_mut() else { return };

    // Cursor only locks while actually playing with no panel up; a modal/menu frees it so its
    // buttons are clickable (and a button-click can't re-grab the view). The debug panel
    // (egui_wants) also blocks the grab so clicking a slider never locks + rotates the view.
    // Build mode is non-interactive too: the cursor stays FREE so the player clicks the palette +
    // plots (and combat/arts already gate on `orbit.locked`, so freeing it disables attacking).
    let interactive = *gate.app.get() == AppState::Playing
        && gate.modal.as_ref().map_or(true, |m| *m.get() == Modal::None)
        && !gate.egui_wants.0
        && !gate.build_mode.active;
    if let Ok(mut cur) = cursor_q.single_mut() {
        if interactive {
            if buttons.just_pressed(MouseButton::Left) && !orbit.locked {
                cur.grab_mode = CursorGrabMode::Locked;
                cur.visible = false;
                orbit.locked = true;
            }
            if keys.just_pressed(KeyCode::Escape) && orbit.locked {
                cur.grab_mode = CursorGrabMode::None;
                cur.visible = true;
                orbit.locked = false;
            }
        } else if orbit.locked {
            cur.grab_mode = CursorGrabMode::None;
            cur.visible = true;
            orbit.locked = false;
        }
    }

    if orbit.locked {
        let d = motion.delta;
        orbit.azimuth -= d.x * SENS_X;
        // Vertical mouse drives the FP look-pitch in first person (crane up/down), the orbit
        // elevation otherwise. `-= d.y` so moving the mouse up looks up (non-inverted).
        if fp.active {
            fp.pitch = (fp.pitch - d.y * SENS_Y).clamp(-FP_PITCH_LIMIT, FP_PITCH_LIMIT);
        } else {
            orbit.pitch = (orbit.pitch + d.y * SENS_Y).clamp(MIN_PITCH, MAX_PITCH);
        }
    }

    let s = scroll.delta.y;
    if s != 0.0 {
        orbit.dist = (orbit.dist - s * ZOOM_SENS).clamp(MIN_DIST, MAX_DIST);
    }

    // Tension dolly: ease a 0→1 blend toward "any warden is winding up a crit", slow enough to read
    // as a deliberate pull-back across the ~1.2s telegraph (and a smooth ease-in on release).
    let want_tension = if gate.crit_tension.as_ref().is_some_and(|t| t.0) { 1.0 } else { 0.0 };
    *tension_blend += (want_tension - *tension_blend) * (1.0 - (-time.delta_secs() * 3.2).exp());
    let r_tension = (*tension_blend).clamp(0.0, 1.0) * CRIT_ZOOM_OUT;

    // Look-ahead lead: shift the framing slightly in the hero's heading while moving, so the camera
    // shows more of where you're going. Subtle — keeps him near centre. The target is low-passed
    // (lead_smooth) so a direction change (e.g. starting a diagonal) eases sideways instead of
    // snapping — an instant facing read jerks the camera the moment you press a strafe key.
    // In the COMBAT STANCE the lead slides toward the ringed foe instead of the heading, so both
    // fighters share the frame — a framing bias only, never a rotation (see the combat constants).
    let heading_lead =
        Vec3::new(hero.facing.sin(), 0.0, hero.facing.cos()) * (0.22 * hero.moving_amt.clamp(0.0, 1.0));
    let lead_target = match hero.soft_pos {
        Some(tp) if hero.stance_amt > 0.01 => {
            let to = Vec3::new(tp.x - hero.pos.x, 0.0, tp.y - hero.pos.y);
            heading_lead.lerp(to.clamp_length_max(1.0) * COMBAT_LEAD, hero.stance_amt.clamp(0.0, 1.0))
        }
        _ => heading_lead,
    };
    let new_lead = orbit.lead + (lead_target - orbit.lead) * (1.0 - (-time.delta_secs() * 4.5).exp());
    orbit.lead = new_lead;
    // Camera glide: ease the followed anchor toward the hero. Snap on a big jump (spawn/respawn/load)
    // so it doesn't sail in from the old position.
    let raw_anchor = Vec3::new(hero.pos.x, hero.y + EYE_H, hero.pos.y);
    if orbit.anchor.distance(raw_anchor) > 5.0 {
        orbit.anchor = raw_anchor;
    } else {
        // Decouple vertical from horizontal: XZ glides fast, Y glides slow so terraced ground steps
        // (`hero.y` snaps 0.5u per tile edge) float by instead of jerking the camera. Airborne, Y
        // tracks at the full rate so a real jump/fall isn't laggy. See `VERT_GLIDE_RATE`.
        let kxz = 1.0 - (-time.delta_secs() * GLIDE_RATE).exp();
        let y_rate = if hero.on_ground { VERT_GLIDE_RATE } else { GLIDE_RATE };
        let ky = 1.0 - (-time.delta_secs() * y_rate).exp();
        orbit.anchor.x += (raw_anchor.x - orbit.anchor.x) * kxz;
        orbit.anchor.z += (raw_anchor.z - orbit.anchor.z) * kxz;
        orbit.anchor.y += (raw_anchor.y - orbit.anchor.y) * ky;
    }
    // Witcher-style shoulder offset: slide the whole framing to the camera-right (right =
    // `(cos az, 0, -sin az)` for this orbit parameterisation) so the hero stands left-of-centre
    // and the view runs past his sword shoulder. The shifted target feeds BOTH the eye and the
    // look-at, so the orbit still circles the hero — the rig just rides offset. First person
    // needs no explicit fade: `fp_eye`/`fp_look` are built from the raw hero position, so the
    // FP blend below washes the offset out on its own.
    let cam_right = Vec3::new(orbit.azimuth.cos(), 0.0, -orbit.azimuth.sin());
    let follow_target = orbit.anchor + orbit.lead + cam_right * SHOULDER_X;
    // Speed feel: sprinting dollies the camera back a touch (eased by `run_amt`).
    let r_speed = hero.run_amt.clamp(0.0, 1.0) * SPRINT_DOLLY;
    // Combat dolly: the stance eases the camera back — a touch for a duel, more as the scrap
    // grows. Smoothed on the orbit so a foe crossing the threat-tally line can't pump the zoom.
    let want_combat = hero.stance_amt.clamp(0.0, 1.0)
        * match hero.threats {
            0 | 1 => COMBAT_DOLLY_DUEL,
            2 => COMBAT_DOLLY_GROUP,
            _ => COMBAT_DOLLY_MOB,
        };
    orbit.combat += (want_combat - orbit.combat) * (1.0 - (-time.delta_secs() * 2.5).exp());
    let (a, p, r) = (orbit.azimuth, orbit.pitch, orbit.dist + r_tension + r_speed + orbit.combat);
    let mut follow_eye =
        follow_target + Vec3::new(a.sin() * p.cos() * r, p.sin() * r, a.cos() * p.cos() * r);
    // Terrain clamp: never let the follow eye sink below the ground under it (a slope/ridge between
    // hero and camera). `smooth_surface_y` is continuous so the lift glides instead of popping; only
    // the third-person eye is clamped (FP rides at head height; build cam is far overhead). `None` —
    // over water / off the island — leaves the eye free.
    if let Some(g) = crate::worldmap::ground_at_world(follow_eye.x, follow_eye.z) {
        follow_eye.y = follow_eye.y.max(g + CAM_GROUND_CLEAR);
    }
    // Boom collision (Witcher-style spring arm): solid buildings tagged `CameraOccluder` pull the
    // eye in toward the hero — fast in, held, slow out — and shrink the shoulder shift when he
    // hugs a wall on that side. The terrain clamp is re-applied to the pulled-in eye.
    let anchor = orbit.anchor;
    let boom = gate.occ.resolve(
        &mut orbit.boom,
        anchor,
        follow_target - anchor,
        follow_eye - follow_target,
        time.delta_secs(),
    );
    let follow_target = boom.target;
    follow_eye = boom.eye;
    if let Some(g) = crate::worldmap::ground_at_world(follow_eye.x, follow_eye.z) {
        follow_eye.y = follow_eye.y.max(g + CAM_GROUND_CLEAR);
    }

    // ── First-person pose ──
    // The orbit's *view* heading (camera-forward yaw) is `azimuth + π` — the look-yaw FP must use so
    // exiting FP doesn't snap your heading. Forward is built from that yaw + the FP look-pitch.
    let look_yaw = a + std::f32::consts::PI;
    let (fpy_sin, fpy_cos) = look_yaw.sin_cos();
    let (fpp_sin, fpp_cos) = fp.pitch.sin_cos();
    let head_off = fp.head_off;
    let fp_pose = fp_eye_rig(&mut fp.eye, &hero, look_yaw, time.delta_secs(), head_off);
    let fp_eye = fp_pose.pos - Vec3::new(fpy_sin, 0.0, fpy_cos) * FP_CLOSE_PULL * (fp.close_fov / FP_CLOSE_FOV_DEG);
    let fp_fwd = Vec3::new(fpy_sin * fpp_cos, fpp_sin, fpy_cos * fpp_cos);
    let fp_look = fp_eye + fp_fwd;

    // Ease the FP blend (smooth dolly-in, no hard cut). The third⇄FP pose is resolved first, then
    // build mode (if active) lerps that overhead — so build framing always wins over FP.
    let want_fp = if fp.active { 1.0 } else { 0.0 };
    fp.blend += (want_fp - fp.blend) * (1.0 - (-time.delta_secs() * 9.0).exp());
    let fpb = fp.blend.clamp(0.0, 1.0);

    // In FP the body owns no facing — the view does (so attacks/arts fire where you look). Movement
    // skips its facing-steer while `fp.active`, so this is the sole writer; coupling on `active` (not
    // `blend`) keeps it consistent through the transition.
    if fp.active {
        hero.facing = look_yaw;
    }
    // (The third-person body is hidden by `fp_body_visibility`; the hands/sword/shield in view are
    // the camera-parented `viewmodel`.)

    // Build mode eases the camera up over the settlement (centred on the castle/origin) so EVERY plot
    // is visible — it doesn't matter which way the hero was facing. Blend back on exit.
    let want = if gate.build_mode.active { 1.0 } else { 0.0 };
    *build_blend += (want - *build_blend) * (1.0 - (-time.delta_secs() * 7.0).exp());
    let blend = (*build_blend).clamp(0.0, 1.0);
    let mut base_eye = follow_eye.lerp(fp_eye, fpb);
    let mut base_look = follow_target.lerp(fp_look, fpb);
    // Succession cinematic: pull the camera to orbit the peasant being possessed (reusing the
    // current azimuth/pitch, pulled back a touch, so it reads as a deliberate move, not a cut).
    // Eased in/out by `cam_blend`; because the risen hero ends up AT `steal_pos`, blending back out
    // hands control smoothly to the normal follow.
    if gate.succ.active && gate.succ.cam_blend > 0.001 {
        let ct = Vec3::new(gate.succ.steal_pos.x, gate.succ.steal_pos.y + EYE_H, gate.succ.steal_pos.z);
        let (ca, cp, cr) = (orbit.azimuth, (orbit.pitch + 0.18).min(MAX_PITCH), orbit.dist + 1.8);
        let ce = ct + Vec3::new(ca.sin() * cp.cos() * cr, cp.sin() * cr, ca.cos() * cp.cos() * cr);
        let k = gate.succ.cam_blend.clamp(0.0, 1.0);
        base_eye = base_eye.lerp(ce, k);
        base_look = base_look.lerp(ct, k);
    }
    let (eye, look) = if gate.build_mode.active || blend > 0.001 {
        let (build_eye, build_look) = gate.build_pose(&cam_proj, time.delta_secs(), blend < 0.02);
        (base_eye.lerp(build_eye, blend), base_look.lerp(build_look, blend))
    } else {
        (base_eye, base_look)
    };
    cam_tf.translation = eye;
    cam_tf.look_at(look, Vec3::Y);

    // FP camera lean: the swing sway (`viewmodel::animate_viewmodel`) plus the eye rig's stride
    // pitch/roll, strafe roll and dodge-roll dip. Composed AFTER look_at so it tilts the settled
    // view; purely cosmetic — `hero.facing`/aim still come from the un-leaned look_yaw above.
    let lean = (fp.sway + fp_pose.lean) * fpb;
    if fpb > 0.0 && lean != Vec3::ZERO {
        cam_tf.rotation *= Quat::from_euler(EulerRot::YXZ, lean.y, lean.x, lean.z);
    }

    // Trauma-based screen shake + FOV punch layered on the settled pose (fed by combat_fx on hits).
    // Damped by ~75% in first person: at eye-scale the raw high-frequency jitter would fill the
    // whole view (a motion-sickness + photosensitive-oscillation hazard) — damped, not removed, so
    // a hit still reads as a hit. `damp` scales with the FP blend so the transition is seamless.
    let damp = 1.0 - 0.75 * fpb;
    if let Some(fb) = &feedback {
        let s = crate::combat_fx::SHAKE_MAX * fb.trauma * fb.trauma * damp;
        if s > 0.0 {
            let t = time.elapsed_secs();
            let chaos = Vec3::new((t * 47.0).sin(), (t * 59.0).sin(), (t * 41.0).sin());
            // Directional bias: a hit site sets `shake_dir` (world XZ) so the camera kicks ALONG
            // it — a swing's `-fwd` reads as recoil. Mix mostly the directed push with some chaos
            // so it still feels organic, not a clean slide. Zero dir → the old unbiased shake.
            let jitter = if fb.shake_dir.length_squared() > 1e-5 {
                let push = Vec3::new(fb.shake_dir.x, 0.0, fb.shake_dir.y) * (t * 53.0).sin();
                push * 0.7 + chaos * 0.5
            } else {
                chaos
            };
            cam_tf.translation += jitter * s;
        }
    }
    // FP close-quarters widen: ramp by how close the ringed foe stands (soft_pos already tracks
    // the engaged hostile). Slow ease both ways so a foe crossing the range can't pump the FOV.
    let want_close = if fp.active {
        hero.soft_pos.map_or(0.0, |tp| {
            ((FP_CLOSE_RANGE - tp.distance(hero.pos)) / FP_CLOSE_RANGE).clamp(0.0, 1.0)
        })
    } else {
        0.0
    };
    fp.close_fov +=
        (want_close * FP_CLOSE_FOV_DEG - fp.close_fov) * (1.0 - (-time.delta_secs() * 3.0).exp());

    // FOV = rest (captured once) + decaying combat punch (damped in FP) + a gentle sprint widen for a
    // sense of speed.
    if let Projection::Perspective(p) = &mut *cam_proj {
        let base = *base_fov.get_or_insert(p.fov);
        let kick = feedback.as_deref().map_or(0.0, |fb| fb.fov_kick) * damp;
        let speed_fov = hero.run_amt.clamp(0.0, 1.0) * SPRINT_FOV_DEG;
        // A slight wide-angle as the combat dolly pulls back, so a mob fight reads the arena.
        let combat_fov = orbit.combat * COMBAT_FOV_PER_UNIT;
        let fp_wide = (FP_FOV_DEG.to_radians() - base).max(0.0) * fpb;
        p.fov = base + fp_wide + (kick + speed_fov + combat_fov + fp.close_fov * fpb).to_radians();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Project `p` through a camera at `eye` looking at `look` (vertical FOV `fov`, `aspect`) to NDC.
    fn ndc(p: Vec3, eye: Vec3, look: Vec3, fov: f32, aspect: f32) -> Vec2 {
        let f = (look - eye).normalize();
        let r = f.cross(Vec3::Y).normalize();
        let u = r.cross(f);
        let v = p - eye;
        let z = v.dot(f);
        let t = (fov * 0.5).tan();
        Vec2::new(v.dot(r) / (z * t * aspect), v.dot(u) / (z * t))
    }

    /// The real town (castle polygon + outer plots + the farthest village house site) fits the band
    /// between the top strip and the palette at 16:9, 4:3 and a tall phone-ish aspect — and fills it
    /// (some point touches an edge, so the framing isn't needlessly far).
    #[test]
    fn build_cam_frames_the_whole_town() {
        let fov = 50f32.to_radians();
        let plots = crate::town::PLOT_OFFSETS.to_vec();
        let house = crate::castle::houses().iter().map(|h| h.pos).min_by(|a, b| a.x.total_cmp(&b.x));
        let pts = build_frame_points(&plots, house);
        for (aspect, bottom, top) in [(16.0 / 9.0, 0.3, 0.06), (4.0 / 3.0, 0.25, 0.05), (0.75, 0.2, 0.05)] {
            let (eye, look) = fit_build_cam(&pts, fov, aspect, bottom, top, 0.0);
            let (lo, hi) = (-1.0 + 2.0 * bottom, 1.0 - 2.0 * top);
            let mut touch = 0.0_f32;
            for &p in &pts {
                let n = ndc(p, eye, look, fov, aspect);
                assert!(n.x.abs() <= 1.0 + 1e-3, "x off-screen: {n} for {p} at aspect {aspect}");
                assert!(n.y >= lo - 1e-3 && n.y <= hi + 1e-3, "y outside band: {n} for {p} at aspect {aspect}");
                touch = touch.max(n.x.abs()).max(((n.y - lo) / (hi - lo) * 2.0 - 1.0).abs());
            }
            assert!(touch > 0.9, "framing too loose at aspect {aspect}: max extent {touch}");
            assert!((look.y).abs() < 1e-3, "look target on the ground plane");
        }
    }
}

