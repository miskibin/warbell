//! Hero locomotion — camera-relative WASD, axis-separated terrain + prop collision, jump +
//! gravity. Ported from `Character.tsx`'s movement block; constants are the TS values.
//!
//! Footing uses `steer::can_stand` (the same rule the orks/wildlife walk by: on land, within
//! one terrace class) + `blockers::is_blocked` for solid props — so the hero slides along
//! walls, can't walk into water, and can't climb 2-class cliff faces — plus a slope limit
//! (`MAX_WALK_SLOPE_DEG`) on the continuous ground, so he can't walk up the massif's rock faces.

use bevy::prelude::*;

use crate::audio::AudioCue;
use crate::biome::Biome;
use crate::orks::Ork;
use crate::villagers::Villager;
use crate::wildlife::Animal;
use crate::{blockers, steer, worldmap};

use super::{FirstPerson, Hero, HeroState, PendingHeroDamage, PlayMode, PlayerRes};

/// WASD is a brisk WALK (≈1.8 m/s for this ~1.9 m body) and Shift a SPRINT (≈5.2 m/s — just over
/// the Mixamo run's own pace, ~4.7 m/s, retargeted onto the knight: any faster and the foot-locked
/// clip patters in quick short steps): the gait is cut from ground speed. A sprinting hero
/// (4.06 u/s) outruns everything (wolves hunt at 3.8); a walking one can be caught by ork scouts,
/// bosses and the snowman.
pub(crate) const SPEED: f32 = 1.4;
pub(crate) const SPRINT_MULT: f32 = 2.9;

// ── Combat stance (the Witcher "Alert Near") ──
// With a soft-target near (`hero.soft_pos`, picked by `softlock`), the body stays SQUARE TO THE
// FOE while WASD strafes/backpedals around it — WASD becomes pure velocity, facing tracks the
// enemy. SPRINT breaks the stance (you turn and run, exactly Witcher 3's rule), first person is
// exempt (the view owns facing there). Non-forward movement is slowed so circling/backpedaling
// has weight instead of skating.
/// Facing-track rate (rad/s) onto the foe while moving in the stance — quick enough to hold a
/// circling ork, still under the free-run `TURN_RATE` so it reads as tracking, not a pin.
const STANCE_TURN_RATE: f32 = 10.0;
/// Speed multiplier at a pure SIDEWAYS strafe (±90° off facing)…
const STANCE_STRAFE_MULT: f32 = 0.85;
/// …and at a full BACKPEDAL (research: free-run speed reused backward reads skatey; grounded
/// games run 0.7–0.9).
const STANCE_BACK_MULT: f32 = 0.72;
/// How much of the movement-vs-facing angle the legs actually take (the pelvis twist); the rest
/// is carried by the crossing gait. Clamped to ±`STANCE_TWIST_MAX`.
const STANCE_TWIST_FRAC: f32 = 0.8;
const STANCE_TWIST_MAX: f32 = 0.7;
/// Walking AWAY ends the stance: input pointing this far off the ringed foe (`dir·to_foe` below
/// this, i.e. more than ~110 deg away) held for [`DISENGAGE_HOLD`] s lets the hero turn and leave.
/// A quick tap back is still a guarded backpedal; holding it means "I'm going". (Backpedalling
/// face-on at ×0.72 of a walk, every ork outpaces you — the stance used to pin you to the fight.)
const DISENGAGE_DOT: f32 = -0.35;
const DISENGAGE_HOLD: f32 = 0.3;
/// Stepping back toward the foe (`dir·to_foe` above this) re-engages the stance.
const REENGAGE_DOT: f32 = 0.3;
const GRAVITY: f32 = 20.0;
const JUMP_SPEED: f32 = 6.5;
const TURN_RATE: f32 = 15.0; // snappier facing toward the move direction
/// Velocity-ramp rates (1/s): the hero accelerates IN fast and slides OUT a touch slower, so he has
/// momentum/weight instead of snapping to full speed and stopping dead.
const ACCEL: f32 = 14.0;
/// Ground speed (world u/s) by which the legs are fully cycling rather than blending from idle.
const GAIT_FULL_SPEED: f32 = 1.0;
const DECEL: f32 = 9.0;
pub(super) const PLAYER_R: f32 = 0.22;
/// Which way (+1 / -1 rotation) the hero last slid round a blocker — sticky, so consecutive
/// frames keep curving the same way round an obstacle instead of flip-flopping.
static SLIDE_SIDE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
/// How far outside a trunk's / rock's collision shell the player starts getting the steer assist.
const OBSTACLE_ASSIST_SENSE: f32 = 0.7;
/// Sideways bias mixed into the input near an obstacle (on top of removing the into-obstacle
/// component), so a run straight at a trunk or boulder curves round it instead of stopping.
const OBSTACLE_ASSIST_TANGENT: f32 = 0.45;
/// Tiny outward bias so the player separates from bark while the tangent finds a slide path.
const OBSTACLE_ASSIST_OUTWARD: f32 = 0.06;
/// Walking down a terrace drops the ground a full height class (`worldmap::GROUND_STEP` 0.5) under
/// the body in one tile. Without a snap the body floats above the new-lower ground for a frame or
/// two each step, flicking `on_ground` off→on → the walk/fall anim strobes all the way down a
/// slope (the "mountains are buggy" report). If we were grounded and aren't rising, glue the body
/// to ground for drops within one class; real cliffs/jumps (gap > this, or a jump's `vel_y > 0`)
/// still go airborne and take fall damage.
const STEP_SNAP: f32 = 0.55;

// ── Slope limit ── The ground is one continuous height field (the snow massif's ridges and
// couloirs, terrace ramps, banks), so the per-step `steer::MAX_STEP` cap alone never bit: a
// frame's step is a few centimetres, which rises far under 0.6 on any slope — the hero walked
// straight up rock faces. Uphill moves now also need the ground they land on to be walkable.
/// Steepest ground (deg) the hero walks UP. Above the navgrid's ~31 deg tile limit (the hero is
/// nimbler than an ork column) and well above every authored route (pass ramps rise ≤0.09 per
/// tile, terrace ramps ≈27 deg, mesa relief ≤0.34/u on top of those).
pub(super) const MAX_WALK_SLOPE_DEG: f32 = 48.0;
/// `tan(MAX_WALK_SLOPE_DEG)` — the rise-per-run bound the rule actually compares.
const MAX_WALK_TAN: f32 = 1.1106;
/// Half-span (world u) of the slope probe — about the boot-to-boot footprint, so the slope is
/// that of the ground under the body, not of a sub-tile crease.
const SLOPE_PROBE: f32 = 0.3;
/// Rise (world u) a move may gain on too-steep ground: float slack only — sliding along a face
/// never climbs it.
const SLOPE_RISE_EPS: f32 = 1e-3;
/// Reach (world u) of the "am I standing on a rock face?" probe ([`steep_fall_with`]): wide
/// enough to see past a 1u cliff ledge to the next riser above and the drop below it, which the
/// footprint-sized minmod probe reads as a flat shelf.
const FALL_PROBE: f32 = 0.6;
/// A face only counts if the ground also falls away below the hero by this much — so standing on
/// flat ground at the FOOT of a cliff never slides you off it.
const FALL_MIN_DROP: f32 = 0.2;
/// Slide down a too-steep face: speed the slide kicks in at, its acceleration and its cap (u/s).
const SLIDE_START: f32 = 1.6;
const SLIDE_ACCEL: f32 = 10.0;
const SLIDE_MAX: f32 = 5.5;
/// Airborne, the hero may land on ground at most this far above where he took off + one terrace
/// class (`steer::MAX_STEP`): a hop gets you onto a crate-high ledge, not up a cliff stair.
const AIR_LEDGE: f32 = 0.45;

/// Seconds the Sand-Dash slide takes to travel its whole blink (`arts::DASH_DIST`). Short + ease-out
/// → an explosive launch that glides to a stop, so the dash *moves* the body instead of teleporting.
/// Read by [`anim`] to drive the dash-swipe lunge progress.
pub(crate) const DASH_TIME: f32 = 0.16;

// ── Dodge roll (Alt) — the Witcher-style evade, available from the first minute (no boon gate,
// unlike the Sand-Dash art): a quick ground somersault along the move input (a backward dive with
// none), i-framed through the tuck, priced in stamina so it trades against blocking. ──
/// Seconds the roll takes start-to-finish. (Playtest: 0.42/2.7 read as a stubby hop — now a
/// longer, farther tumble, priced accordingly.)
pub(crate) const ROLL_TIME: f32 = 0.5;
/// World units the roll covers (clipped early at walls/cliffs — see the path walk in
/// [`player_roll`]). Playtest: 3.4 still read short — a roll should clearly reposition you.
const ROLL_DIST: f32 = 4.0;
/// Stamina spent per roll (same pool as block/arts — you can't roll-spam AND hold a guard).
/// Raised from 25: at 25 the roll was a near-free escape; ~2½ rolls now drain a level-1 pool.
const ROLL_STAMINA: f32 = 38.0;
/// Seconds of invulnerability from the roll's start — covers the tuck, not the recovery, so a
/// late roll still eats the follow-up blow (the timing is the skill).
const ROLL_IFRAME: f32 = 0.34;
/// Height (world units) of the tumble pivot — the body's centre of mass while tucked. The root
/// (feet origin) orbits this point through the somersault so the ball rolls in place instead of
/// cartwheeling about the ankles.
const ROLL_PIVOT_H: f32 = 0.5;

// ── Hazards (ported from Character.tsx) ──
/// A fall shorter than this lands free; beyond it hurts.
const FALL_SAFE: f32 = 1.1;
const FALL_DMG_PER_UNIT: f32 = 16.0;
const FALL_DMG_MAX: f32 = 45.0;
/// The swamp drags: move at 75% speed, and poison gnaws 2 HP every 2.5s while standing in it.
const SWAMP_SLOW: f32 = 0.75;
const SWAMP_POISON: f32 = 2.0;
const SWAMP_POISON_INTERVAL: f32 = 2.5;

fn key_axis(keys: &ButtonInput<KeyCode>, pos: KeyCode, neg: KeyCode) -> f32 {
    (keys.pressed(pos) as i32 - keys.pressed(neg) as i32) as f32
}

/// Camera-relative WASD/arrow move direction on the ground plane (world XZ, normalized), or
/// `None` when no move key is held. Shared by [`player_move`] and the [`player_roll`] arm so the
/// roll dives exactly the way the player is steering.
fn move_input(keys: &ButtonInput<KeyCode>, cam: Option<&Transform>) -> Option<Vec2> {
    let mut fwd = cam.map(|c| *c.forward()).unwrap_or(Vec3::NEG_Z);
    fwd.y = 0.0;
    if fwd.length_squared() < 1e-6 {
        fwd = Vec3::NEG_Z;
    }
    fwd = fwd.normalize();
    let right = Vec3::new(-fwd.z, 0.0, fwd.x);
    let fwd_amt = (key_axis(keys, KeyCode::KeyW, KeyCode::KeyS)
        + key_axis(keys, KeyCode::ArrowUp, KeyCode::ArrowDown))
    .clamp(-1.0, 1.0);
    let rgt_amt = (key_axis(keys, KeyCode::KeyD, KeyCode::KeyA)
        + key_axis(keys, KeyCode::ArrowRight, KeyCode::ArrowLeft))
    .clamp(-1.0, 1.0);
    let dir = fwd * fwd_amt + right * rgt_amt;
    (dir.length_squared() > 1e-6).then(|| Vec2::new(dir.x, dir.z).normalize())
}

fn obstacle_assisted_dir(pos: Vec2, input_dir: Vec2, current_vel: Vec2) -> Vec2 {
    let (rx, rz) = blockers::circle_repulsion(pos.x, pos.y, PLAYER_R, OBSTACLE_ASSIST_SENSE);
    let away = Vec2::new(rx, rz);
    if away.length_squared() < 1e-6 {
        return input_dir;
    }
    let away_dir = away.normalize();
    // Only help when the player is pushing into bark; retreating or skimming past stays raw input.
    if input_dir.dot(away_dir) >= -0.08 {
        return input_dir;
    }
    let strength = away.length().min(1.0);
    let mut tangent = Vec2::new(-away_dir.y, away_dir.x);
    let continuity = if current_vel.length_squared() > 1e-4 {
        current_vel.normalize()
    } else {
        input_dir
    };
    if tangent.dot(continuity) < 0.0 {
        tangent = -tangent;
    }
    // Strip the part of the input that drives INTO the obstacle (fully at the shell, fading out
    // across the sense band), then lean along the tangent: the hero flows round the blocker.
    let into = input_dir.dot(away_dir);
    // The outward lean grows toward contact: a pure tangent step at the shell cuts into the curve
    // of a round blocker (and gets rejected), so near contact the path bows slightly out.
    (input_dir - away_dir * (into * strength)
        + tangent * (OBSTACLE_ASSIST_TANGENT * strength)
        + away_dir * ((OBSTACLE_ASSIST_OUTWARD + 0.3 * strength * strength) * strength))
        .normalize_or_zero()
}

/// Arm the **dodge roll** on Alt: pick the dive direction (move input, else a backward dive away
/// from the facing), walk the path for footing/blockers so a wall or cliff-lip clips it short,
/// spend the stamina, grant the i-frames, and cancel any swing/charge — a roll is a full commit.
/// [`player_move`] then owns the slide + tumble while `roll_t >= 0.0`; [`anim`] tucks the limbs.
/// Runs *before* `player_move` so the roll owns locomotion from its very first frame.
#[allow(clippy::too_many_arguments)]
pub fn player_roll(
    time: Res<Time>,
    mode: Res<PlayMode>,
    keys: Res<ButtonInput<KeyCode>>,
    orbit: Res<super::camera::OrbitCam>,
    player: Res<PlayerRes>,
    fx: Option<Res<super::CombatFx>>,
    mut commands: Commands,
    mut cues: MessageWriter<AudioCue>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
    cam_q: Query<&Transform, (With<Camera3d>, Without<Hero>)>,
    mut hero_q: Query<(&mut Hero, &mut super::HeroHealth)>,
    mut next_test: Local<f32>,
) {
    let Ok((mut hero, mut hh)) = hero_q.single_mut() else { return };
    let idle = hero.roll_t < 0.0 && hero.dash_t < 0.0 && hero.on_ground;
    // Debug/capture hook: `FOREST_ROLLTEST=1` re-arms a forward roll on a ~1.8s loop (no keypress
    // under a capture; skips the pointer-lock/stamina gates) so a `FOREST_TPS` clip can frame the
    // somersault — same pattern as `FOREST_BELLTEST`.
    let test_fire = *mode == PlayMode::Play
        && idle
        && std::env::var("FOREST_ROLLTEST").is_ok()
        && time.elapsed_secs() >= *next_test;
    let manual = *mode == PlayMode::Play
        && player.0.is_alive()
        && orbit.locked
        && idle
        && hh.stamina >= ROLL_STAMINA
        && keys.just_pressed(KeyCode::AltLeft);
    if !manual && !test_fire {
        return;
    }
    if test_fire {
        *next_test = time.elapsed_secs() + 1.8;
    }

    // Dive along the live move input; with none, a plain FORWARD roll along the facing (playtest:
    // the old default backward dive was unintuitive — Alt should just roll where you're headed).
    // The backward tumble (eyes kept on the foe) fires only when the dive runs AWAY from the
    // facing while the combat stance holds you square to a target.
    let input = move_input(&keys, cam_q.single().ok());
    let facing_fwd = Vec2::new(hero.facing.sin(), hero.facing.cos());
    let dir = input.unwrap_or(facing_fwd);
    let back = hero.stance_amt > 0.5 && dir.dot(facing_fwd) < -0.3;

    // Walk the path in steps: every sample must have footing within one terrace class of the
    // last (no tumbling off a cliff / into water) and stay clear of solid props — the roll stops
    // at the last good sample, so rolling at a wall gives a short hop, not a clip-through.
    let from = hero.pos;
    let mut to = from;
    let mut ref_y = hero.y;
    for k in 1..=8 {
        let cand = from + dir * (ROLL_DIST * k as f32 / 8.0);
        let footing_ok = hero_can_stand(cand.x, cand.y, PLAYER_R, ref_y) && slope_ok(cand.x, cand.y, ref_y);
        if !footing_ok || blockers::any_within(cand.x, cand.y, PLAYER_R) || crate::traps::blocks_entry(to, cand) {
            break;
        }
        to = cand;
        ref_y = footing(cand.x, cand.y).unwrap_or(ref_y);
    }

    hh.stamina -= ROLL_STAMINA;
    hh.regen_pause = hh.regen_pause.max(0.45);
    hh.iframe_until = time.elapsed_secs() + ROLL_IFRAME;
    hero.roll_from = from;
    hero.roll_to = to;
    hero.roll_t = 0.0;
    hero.roll_back = back;
    // A forward roll faces its travel; the backward (in-stance) dive keeps eyes on the foe.
    if !back {
        hero.facing = dir.x.atan2(dir.y);
    }
    // A roll is a full commit: it cancels the swing, the charge, any buffered chain and the
    // combo clock — you traded offense for the escape.
    hero.attacking = false;
    hero.heavy = false;
    hero.queued = false;
    hero.lock_face = None;
    hero.lunge_left = 0.0;
    hero.charge_t = -1.0;
    hero.combo = 0;
    hero.combo_until = 0.0;
    cues.write(AudioCue::Roll); // the gritty dodge-roll tumble grunt (own clip, not the Dash whoosh)
    crate::combat_fx::add_fov_kick(&mut feedback, 0.8);
    // Kick-off dust where the dive launches from.
    if let Some(fx) = fx {
        let at = Vec3::new(from.x, hero.y + 0.25, from.y);
        super::combat::spawn_dust_puff(&mut commands, &fx, at, 5);
    }
}

/// One collision-resolved locomotion step: `(new pos, new vel, slide side)`. Takes the full
/// `step` when `free`; otherwise slides instead of stopping dead — first the classic per-axis
/// slide (walls), then progressively wider deflections of the step (rotated boxes, the curve of a
/// trunk or boulder, a terrace lip at an angle), preferring `side` (the way the hero last slid) so
/// he doesn't jitter between two equally good detours. Speed drops with the deflection (cos): a
/// glancing slide, not a free teleport round the obstacle. Boxed into a corner → stop.
fn slide_step(pos: Vec2, step: Vec2, vel: Vec2, side: f32, free: impl Fn(Vec2) -> bool) -> (Vec2, Vec2, f32) {
    if free(pos + step) {
        return (pos + step, vel, side);
    }
    let len = step.length();
    if len < 1e-6 {
        return (pos, Vec2::ZERO, side);
    }
    let dir = step / len;
    for (axis_step, keep) in [(Vec2::new(step.x, 0.0), Vec2::X), (Vec2::new(0.0, step.y), Vec2::Y)] {
        if axis_step.length() > 0.45 * len && free(pos + axis_step) {
            return (pos + axis_step, vel * keep, side);
        }
    }
    for ang in [0.35f32, 0.7, 1.05, 1.4] {
        for sgn in [side, -side] {
            let (sn, cs) = (sgn * ang).sin_cos();
            let d = Vec2::new(dir.x * cs - dir.y * sn, dir.x * sn + dir.y * cs);
            let k = ang.cos();
            if free(pos + d * (len * k)) {
                return (pos + d * (len * k), d * (vel.length() * k), sgn);
            }
        }
    }
    (pos, Vec2::ZERO, side)
}

#[cfg(test)]
mod slide_tests {
    use super::*;

    /// Walk the slide for `frames` steps toward +Z at 4 u/s through a world of discs.
    fn run(discs: &[(Vec2, f32)], start: Vec2, frames: usize) -> Vec2 {
        let free = |p: Vec2| discs.iter().all(|(c, r)| p.distance(*c) >= r + PLAYER_R);
        let (mut pos, mut vel, mut side) = (start, Vec2::new(0.0, 4.0), 1.0);
        for _ in 0..frames {
            vel = Vec2::new(0.0, 4.0); // the player keeps holding W
            let step = vel / 60.0;
            (pos, vel, side) = slide_step(pos, step, vel, side, &free);
        }
        pos
    }

    #[test]
    fn head_on_into_a_trunk_slides_round_it() {
        // A 0.4 trunk dead ahead: the hero must get past it, not stall at its face.
        let end = run(&[(Vec2::new(0.0, 2.0), 0.4)], Vec2::ZERO, 120);
        assert!(end.y > 3.0, "stalled at the trunk: {end:?}");
    }

    #[test]
    fn diagonal_wall_of_stones_is_followed() {
        // A row of boulders at 45 deg across the path: slide along it and out past its end.
        let wall: Vec<_> = (0..6).map(|i| (Vec2::new(-1.5 + i as f32 * 0.5, 1.5 + i as f32 * 0.5), 0.3)).collect();
        let end = run(&wall, Vec2::ZERO, 240);
        assert!(end.y > 5.0, "stuck on the slanted row: {end:?}");
    }

    #[test]
    fn a_closed_corner_stops_without_tunnelling() {
        // Two overlapping discs forming a wide cup: the hero stops, never passes through.
        let discs = [(Vec2::new(-0.35, 1.0), 0.6), (Vec2::new(0.35, 1.0), 0.6)];
        let free = |p: Vec2| discs.iter().all(|(c, r)| p.distance(*c) >= r + PLAYER_R);
        let end = run(&discs, Vec2::ZERO, 120);
        assert!(free(end), "ended inside a blocker: {end:?}");
    }
}

#[cfg(test)]
mod slope_tests {
    use super::*;

    /// A plane rising along +X at `deg`.
    fn ramp(deg: f32) -> impl Fn(f32, f32) -> Option<f32> + Copy {
        let k = deg.to_radians().tan();
        move |x: f32, _z: f32| Some(x * k)
    }

    /// Hold `input` for `frames` steps at walking pace through the real locomotion step (contour
    /// deflection + `slide_step` with the slope rule); returns the end position.
    fn walk(ground: impl Fn(f32, f32) -> Option<f32> + Copy, input: Vec2, frames: usize) -> Vec2 {
        let dt = 1.0 / 60.0;
        let (mut pos, mut vel, mut side) = (Vec2::ZERO, Vec2::ZERO, 1.0);
        for _ in 0..frames {
            vel = vel + (input * SPEED * SPRINT_MULT - vel) * (dt * ACCEL).min(1.0);
            let y = ground(pos.x, pos.y).unwrap();
            let free = |p: Vec2| slope_ok_with(ground, p.x, p.y, ground(p.x, p.y).unwrap(), y);
            let v = slope_deflect_with(ground, pos, vel, dt, y);
            (pos, vel, side) = slide_step(pos, v * dt, v, side, free);
        }
        pos
    }

    #[test]
    fn trap_lattice_mirrors_the_walk_limit() {
        assert_eq!(crate::traps::WALK_TAN, MAX_WALK_TAN);
        assert!((MAX_WALK_TAN - MAX_WALK_SLOPE_DEG.to_radians().tan()).abs() < 1e-3);
    }

    #[test]
    fn walkable_slopes_climb_steep_ones_block() {
        for deg in [0.0, 20.0, 35.0, 45.0] {
            let g = ramp(deg);
            assert!(slope_ok_with(g, 0.1, 0.0, g(0.1, 0.0).unwrap(), 0.0), "{deg} deg should be walkable");
        }
        for deg in [52.0, 60.0, 75.0, 85.0] {
            let g = ramp(deg);
            assert!(!slope_ok_with(g, 0.01, 0.0, g(0.01, 0.0).unwrap(), 0.0), "{deg} deg must block an uphill step");
            // …but stepping DOWN it, or along its contour, is always fine.
            assert!(slope_ok_with(g, -0.01, 0.0, g(-0.01, 0.0).unwrap(), 0.0), "{deg} deg: walking down");
            assert!(slope_ok_with(g, 0.0, 0.05, g(0.0, 0.05).unwrap(), 0.0), "{deg} deg: along the contour");
        }
    }

    #[test]
    fn sprinting_straight_at_a_rock_face_gains_no_height() {
        let g = ramp(62.0);
        let end = walk(g, Vec2::X, 240);
        assert!(g(end.x, end.y).unwrap() < 0.01, "climbed the face to {end:?}");
    }

    #[test]
    fn a_glancing_run_glides_along_the_face() {
        // 45 deg into a 62 deg face: no climbing, but the hero keeps moving along its foot.
        let g = ramp(62.0);
        let end = walk(g, Vec2::new(1.0, 1.0).normalize(), 240);
        assert!(g(end.x, end.y).unwrap() < 0.01, "climbed the face to {end:?}");
        assert!(end.y > 6.0, "stalled against the face instead of sliding along it: {end:?}");
    }

    #[test]
    fn a_walkable_ramp_is_climbed_at_speed() {
        let g = ramp(35.0);
        let end = walk(g, Vec2::X, 120);
        assert!(end.x > 5.0, "a 35 deg ramp must stay walkable: {end:?}");
    }

    #[test]
    fn steep_gully_floor_and_mesa_lip_stay_walkable() {
        // A V-gully with 56 deg walls whose floor rises gently (0.2/u) along +Z: walking up the
        // floor, a little off its crease, must not snag on the walls.
        let gully = |x: f32, z: f32| Some(x.abs() * 1.5 + 0.2 * z);
        let end = walk(gully, Vec2::Y, 180);
        assert!(end.y > 6.0, "stuck in the gully: {end:?}");
        // A mesa wall (a 1.0 discontinuity, `MAX_STEP`'s job) reads as flat shelf on either side,
        // so walking along its foot / lip is unaffected by the slope rule.
        let mesa = |x: f32, _z: f32| Some(if x >= 0.0 { 1.0 } else { 0.0 });
        assert!(terrain_slope_with(mesa, -0.1, 0.0).unwrap() < 1e-3);
        assert!(terrain_slope_with(mesa, 0.1, 0.0).unwrap() < 1e-3);
    }

    #[test]
    fn rock_faces_slide_but_cliff_feet_and_lips_hold() {
        // A sheer 62 deg face: standing on it slides you down (-X).
        let face = ramp(62.0);
        let dn = steep_fall_with(face, 0.5, 0.0).expect("62 deg face must slide");
        assert!(dn.x < -0.9, "{dn:?}");
        // A walkable 40 deg ramp holds.
        assert!(steep_fall_with(ramp(40.0), 0.5, 0.0).is_none());
        // A cliff STAIR (1u ledges, 1.4u risers): the minmod footprint probe calls a ledge flat,
        // the face probe must not.
        let stair = |x: f32, _z: f32| Some((x.max(0.0)).floor() * 1.4);
        assert!(steep_fall_with(stair, 2.5, 0.0).is_some(), "stair ledge read as standable");
        // Flat ground at the foot of a wall, and the lip of a mesa: no slide.
        let mesa = |x: f32, _z: f32| Some(if x >= 0.0 { 3.0 } else { 0.0 });
        assert!(steep_fall_with(mesa, -0.3, 0.0).is_none(), "slid off the cliff foot");
        assert!(steep_fall_with(mesa, 0.3, 0.0).is_none(), "slid off the mesa lip");
        // The gully floor stays walkable (its walls rise on both sides, nothing falls away).
        let gully = |x: f32, z: f32| Some(x.abs() * 1.5 + 0.2 * z);
        assert!(steep_fall_with(gully, 0.1, 3.0).is_none());
    }

    /// The real map: every artery road (the mountain-pass trails, castle approaches, canyon road
    /// …) and the hero's spawn must stay walkable under the slope limit.
    #[test]
    fn roads_and_spawn_are_walkable() {
        let g = crate::worldmap::ground_at_world;
        let (spawn, _) = super::super::spawn_point();
        assert!(terrain_slope_with(g, spawn.x, spawn.y).is_none_or(|s| s <= MAX_WALK_TAN), "spawn on a cliff");
        let mut worst = (0.0f32, Vec2::ZERO);
        let mut n = 0;
        for line in crate::roads::artery_polylines() {
            for w in line.windows(2) {
                let steps = (w[0].distance(w[1]) / 0.25).ceil().max(1.0) as usize;
                for k in 0..=steps {
                    let p = w[0].lerp(w[1], k as f32 / steps as f32);
                    // No road may read as a rock face either (the slide would shove you off it).
                    assert!(steep_fall_with(g, p.x, p.y).is_none(), "a road slides the hero off at {p:?}");
                    if let Some(s) = terrain_slope_with(g, p.x, p.y) {
                        n += 1;
                        if s > worst.0 {
                            worst = (s, p);
                        }
                    }
                }
            }
        }
        println!("{n} road samples, steepest {:.1} deg at {:?}", worst.0.atan().to_degrees(), worst.1);
        assert!(n > 1000, "no roads sampled");
        assert!(worst.0 <= MAX_WALK_TAN, "a road climbs {:.1} deg at {:?}", worst.0.atan().to_degrees(), worst.1);
    }

    /// Survey (run by hand: `cargo test --release reach_survey -- --ignored --nocapture`): flood the
    /// island from the hero's spawn on a 0.25u lattice under the old rule (any rise ≤ `MAX_STEP`
    /// per step) and the new one (+ the slope limit), and report what the slope limit cut off —
    /// overall land area and the authored overlook anchors.
    #[test]
    #[ignore]
    fn reach_survey() {
        use crate::worldmap::{COLS, GX, GZ, ROWS};
        let s = 0.25f32;
        let (w, h) = ((COLS as f32 / s) as i32, (ROWS as f32 / s) as i32);
        let at = |i: i32, j: i32| Vec2::new(i as f32 * s - GX, j as f32 * s - GZ);
        let g = crate::worldmap::ground_at_world;
        let n = (w * h) as usize;
        let mut ys = vec![f32::NAN; n];
        let mut slope = vec![0.0f32; n];
        for j in 0..h {
            for i in 0..w {
                let p = at(i, j);
                if let Some(y) = g(p.x, p.y) {
                    ys[(j * w + i) as usize] = y;
                    slope[(j * w + i) as usize] = terrain_slope_with(g, p.x, p.y).unwrap_or(0.0);
                }
            }
        }
        let (spawn, _) = super::super::spawn_point();
        let si = (((spawn.x + GX) / s).round() as i32, ((spawn.y + GZ) / s).round() as i32);
        let flood = |max_tan: f32| {
            let mut seen = vec![false; n];
            let mut q = std::collections::VecDeque::from([si]);
            seen[(si.1 * w + si.0) as usize] = true;
            while let Some((i, j)) = q.pop_front() {
                let fy = ys[(j * w + i) as usize];
                for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (ni, nj) = (i + di, j + dj);
                    if ni < 0 || nj < 0 || ni >= w || nj >= h {
                        continue;
                    }
                    let k = (nj * w + ni) as usize;
                    let ty = ys[k];
                    if seen[k] || ty.is_nan() || ty > fy + steer::MAX_STEP {
                        continue;
                    }
                    if ty > fy + SLOPE_RISE_EPS && slope[k] > max_tan {
                        continue;
                    }
                    seen[k] = true;
                    q.push_back((ni, nj));
                }
            }
            seen
        };
        let (old, new) = (flood(f32::INFINITY), flood(MAX_WALK_TAN));
        for deg in [38.0f32, 40.0, 42.0, 45.0] {
            let f = flood(deg.to_radians().tan());
            let c = f.iter().filter(|b| **b).count();
            let top = (0..n).filter(|k| f[*k]).map(|k| ys[k]).fold(f32::MIN, f32::max);
            println!("  at {deg} deg: {c} cells, top {top:.1}");
        }
        for deg in [30.0f32, 35.0, 38.0, 40.0, 45.0, 48.0, 55.0] {
            let t = deg.to_radians().tan();
            println!("  land steeper than {deg}: {}", slope.iter().zip(&ys).filter(|(s, y)| !y.is_nan() && **s > t).count());
        }
        let (no, nn) = (old.iter().filter(|b| **b).count(), new.iter().filter(|b| **b).count());
        println!("reachable land cells: old {no}, with slope limit {nn} ({:.1}% lost)", 100.0 * (no - nn) as f32 / no as f32);
        let near = |set: &[bool], c: Vec2, r: f32| {
            let (ci, cj) = (((c.x + GX) / s) as i32, ((c.y + GZ) / s) as i32);
            let rr = (r / s) as i32;
            (-rr..=rr).any(|dj| (-rr..=rr).any(|di| {
                let (i, j) = (ci + di, cj + dj);
                i >= 0 && j >= 0 && i < w && j < h && set[(j * w + i) as usize]
            }))
        };
        // The `vista.rs` overlook anchors (derived from the region layout, like vista does).
        for (c, _, label) in crate::vista::overlook_anchors() {
            println!("{label}: old {} new {}", near(&old, c, 5.0), near(&new, c, 5.0));
        }
        // The highest cell each rule reaches (the snow summit's crown).
        let top = |set: &[bool]| (0..n).filter(|k| set[*k]).map(|k| ys[k]).fold(f32::MIN, f32::max);
        println!("highest reachable ground: old {:.1}, new {:.1}", top(&old), top(&new));
    }
}

pub(super) fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    a + steer::wrap_pi(b - a) * t
}

fn write_state(state: &mut HeroState, hero: &Hero) {
    state.pos = hero.pos;
    state.y = hero.y;
    state.facing = hero.facing;
    state.alive = true;
}

/// Hero footing height at `(x, z)`: terrain, or the bridge deck where the river shows through.
/// `worldmap::ground_at_world` is terrain-only (reads `None` over water), so — unlike the orks,
/// who cross on nav-grid waypoints — the hero ORs the deck in here to walk the planks.
fn footing(x: f32, z: f32) -> Option<f32> {
    crate::wreck_cove::footing_over(x, z, worldmap::ground_at_world(x, z).or_else(|| crate::bridges::deck_y_at(x, z)))
}

/// Bridge-aware twin of `steer::can_stand`: the body centre + four footprint edges must all be
/// on footing (terrain or deck) within one terrace class of `cur_y`.
fn hero_can_stand(x: f32, z: f32, r: f32, cur_y: f32) -> bool {
    const OFF: [(f32, f32); 5] = [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];
    OFF.iter().all(|(dx, dz)| {
        matches!(footing(x + dx * r, z + dz * r), Some(y) if (y - cur_y).abs() <= steer::MAX_STEP)
    })
}

/// Asymmetric locomotion rule that lets the hero *drop* off cliffs (the orks/wildlife can't).
/// Every footprint point must still be on footing — no walking into water or off the map — and
/// none may rise more than one terrace class above `ref_y` (can't climb a cliff face). There's
/// no lower bound, so a downward ledge is walkable: the hero steps off, gravity takes over, and
/// the landing code bruises him if the drop cleared `FALL_SAFE`.
///
/// `ref_y` is the hero's *body* height (`hero.y`), not the ground under him: while falling past a
/// ledge that keeps the just-departed high tile in his radius, the high tile stays within
/// `MAX_STEP` of his airborne body, so it doesn't snag him at the lip.
pub(super) fn hero_can_step(x: f32, z: f32, r: f32, ref_y: f32) -> bool {
    const OFF: [(f32, f32); 5] = [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];
    OFF.iter().all(|(dx, dz)| {
        matches!(footing(x + dx * r, z + dz * r), Some(y) if y <= ref_y + steer::MAX_STEP)
    })
}

/// One axis of the slope probe: the gentler of the two one-sided differences, 0 when they
/// disagree in sign (the "minmod" limiter). On a smooth slope both sides agree, so this is the
/// plain gradient; across a mesa WALL (a discontinuity `MAX_STEP` already handles) or a crease
/// (gully floor, ridge crest) it reads the walkable side instead of the jump.
fn minmod(back: f32, fwd: f32) -> f32 {
    if back * fwd <= 0.0 {
        0.0
    } else if back.abs() < fwd.abs() {
        back
    } else {
        fwd
    }
}

/// Terrain slope (rise per run, ≥ 0) under the body at `(x, z)`, sampled across the footprint
/// with [`minmod`]. `None` off the terrain (water / bridge deck / off the island): no slope there.
pub(super) fn terrain_slope_with(ground: impl Fn(f32, f32) -> Option<f32>, x: f32, z: f32) -> Option<f32> {
    let h = SLOPE_PROBE;
    let c = ground(x, z)?;
    let (xm, xp) = (ground(x - h, z)?, ground(x + h, z)?);
    let (zm, zp) = (ground(x, z - h)?, ground(x, z + h)?);
    let gx = minmod((c - xm) / h, (xp - c) / h);
    let gz = minmod((c - zm) / h, (zp - c) / h);
    Some(gx.hypot(gz))
}

/// The slope rule on a ground sampler (unit-testable): may feet at body height `ref_y` move onto
/// `(x, z)`, whose footing is `to_y`? Level and DOWNHILL moves always may (walk down a slope,
/// slide along a face, drop off a ledge); a move that GAINS height needs the ground it lands on to
/// be no steeper than [`MAX_WALK_SLOPE_DEG`].
pub(super) fn slope_ok_with(ground: impl Fn(f32, f32) -> Option<f32>, x: f32, z: f32, to_y: f32, ref_y: f32) -> bool {
    to_y <= ref_y + SLOPE_RISE_EPS || terrain_slope_with(ground, x, z).is_none_or(|g| g <= MAX_WALK_TAN)
}

pub(super) fn slope_ok(x: f32, z: f32, ref_y: f32) -> bool {
    match footing(x, z) {
        Some(to_y) => slope_ok_with(worldmap::ground_at_world, x, z, to_y, ref_y),
        None => true, // no footing at all is `hero_can_step`'s call
    }
}

/// Is the ground under `(x, z)` too steep to stand on and push off (no jumping up a rock face)?
/// Both the footprint slope AND the wider face probe: a stepped cliff's 1u ledges read flat to the
/// footprint probe, which let the hero hop up a sheer wall one ledge at a time.
fn too_steep(x: f32, z: f32) -> bool {
    terrain_slope_with(worldmap::ground_at_world, x, z).is_some_and(|g| g > MAX_WALK_TAN)
        || steep_fall_with(worldmap::ground_at_world, x, z).is_some()
}

/// Standing on a rock FACE (not beside one)? Probes [`FALL_PROBE`] up and down the local fall
/// line: the ground must rise steeper than the walk limit ahead AND drop away behind. Returns the
/// downhill direction to slide along, `None` on walkable ground, at a cliff's foot (no drop) or on
/// a mesa lip (no rise). Unlike the footprint's minmod probe this sees a stepped cliff's narrow
/// ledges as the face they belong to.
pub(super) fn steep_fall_with(ground: impl Fn(f32, f32) -> Option<f32>, x: f32, z: f32) -> Option<Vec2> {
    let c = ground(x, z)?;
    // Fall line over the full probe span (a narrower one reads a ledge's flat top as level).
    let h = FALL_PROBE;
    let g = Vec2::new(ground(x + h, z)? - ground(x - h, z)?, ground(x, z + h)? - ground(x, z - h)?);
    let up = g.normalize_or_zero();
    if up == Vec2::ZERO {
        return None;
    }
    let p = Vec2::new(x, z);
    let (hi, lo) = (p + up * FALL_PROBE, p - up * FALL_PROBE);
    let rise = ground(hi.x, hi.y)? - c;
    let drop = c - ground(lo.x, lo.y)?;
    (rise > FALL_PROBE * MAX_WALK_TAN && drop > FALL_MIN_DROP).then_some(-up)
}

/// Velocity redirected along the CONTOUR when it would carry the body uphill onto too-steep
/// ground: the uphill component is removed (plus a hair of downhill lean so the next step lands
/// level-or-lower and is accepted), so walking into a rock face at an angle glides along its foot
/// instead of stopping dead or chattering against it. Unchanged where the step is walkable.
fn slope_deflect_with(ground: impl Fn(f32, f32) -> Option<f32> + Copy, pos: Vec2, vel: Vec2, dt: f32, ref_y: f32) -> Vec2 {
    let p = pos + vel * dt;
    if vel.length_squared() < 1e-8 {
        return vel;
    }
    match ground(p.x, p.y) {
        Some(to_y) if !slope_ok_with(ground, p.x, p.y, to_y, ref_y) => {}
        _ => return vel,
    }
    // Uphill direction at the candidate (plain central difference — the face's own fall line).
    let h = SLOPE_PROBE;
    let (Some(xm), Some(xp), Some(zm), Some(zp)) =
        (ground(p.x - h, p.y), ground(p.x + h, p.y), ground(p.x, p.y - h), ground(p.x, p.y + h))
    else {
        return vel;
    };
    let up = Vec2::new(xp - xm, zp - zm).normalize_or_zero();
    let into = vel.dot(up);
    if into <= 0.0 {
        return vel;
    }
    vel - up * (into + 0.05 * vel.length())
}

/// Shove the hero out of a creature's body cylinder (centre `c`, radius `body_r`) so he can't
/// clip through it — a one-way push (the creature holds its ground), sliding along the same
/// standable/blocker rule as locomotion. Shared by the ork + animal collision passes.
fn shove_out_of(hero: &mut Hero, c: Vec2, body_r: f32, cur_y: f32) {
    let min_d = PLAYER_R + body_r;
    let to = hero.pos - c;
    let d = to.length();
    if d <= 1e-4 || d >= min_d {
        return;
    }
    let push = to / d * (min_d - d);
    let nx = hero.pos.x + push.x;
    let nz = hero.pos.y + push.y;
    let ok = |x: f32, z: f32| {
        hero_can_stand(x, z, PLAYER_R, cur_y) && slope_ok(x, z, cur_y) && !blockers::any_within(x, z, PLAYER_R)
    };
    if ok(nx, hero.pos.y) {
        hero.pos.x = nx;
    }
    if ok(hero.pos.x, nz) {
        hero.pos.y = nz;
    }
}

/// The solid bodies the hero is shoved out of so he can't clip through them. Bundled into one
/// [`SystemParam`] to keep [`player_move`] under Bevy's 16-argument ceiling now that the (huge)
/// wardens are solid too.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Bodies<'w, 's> {
    // `Without<Dying>`: a crumpling corpse must NOT keep its body collision, or the hero hits an
    // invisible wall where an enemy just died (the corpse's logical `pos` lingers for the ~1.4s fade).
    orks: Query<'w, 's, &'static Ork, Without<crate::dying::Dying>>,
    animals: Query<'w, 's, &'static Animal, Without<crate::dying::Dying>>,
    villagers: Query<'w, 's, &'static Villager>,
    bosses: Query<'w, 's, &'static crate::boss::Boss, Without<crate::dying::Dying>>,
    /// Present when a demo script owns the hero — `player_move` then yields locomotion to it
    /// (bundled here to keep `player_move` under Bevy's 16-param ceiling).
    scripted: Option<Res<'w, super::ScriptedHero>>,
}

pub fn player_move(
    time: Res<Time>,
    mode: Res<PlayMode>,
    build_mode: Res<crate::town::BuildMode>,
    player: Res<PlayerRes>,
    buffs: Res<crate::inventory::Buffs>,
    keys: Res<ButtonInput<KeyCode>>,
    mut hero_q: Query<(&mut Hero, &mut Transform), Without<Camera3d>>,
    cam_q: Query<&Transform, (With<Camera3d>, Without<Hero>)>,
    bodies: Bodies,
    mut state: ResMut<HeroState>,
    mut pending: ResMut<PendingHeroDamage>,
    mut feedback: ResMut<crate::combat_fx::HitFeedback>,
    mut cues: MessageWriter<AudioCue>,
    mut poison_acc: Local<f32>,
    mut was_swamp: Local<bool>,
    fp: Res<FirstPerson>,
) {
    let Ok((mut hero, mut tf)) = hero_q.single_mut() else { return };
    let t = time.elapsed_secs();

    // A scripted demo (`FOREST_DEMO=explore`) owns the hero's pos/facing/anim — yield locomotion to
    // it (don't fight it with input-driven movement) but still mirror the pose into `HeroState` so
    // the follow-cam / weather / audio track the scripted walk.
    if bodies.scripted.is_some() {
        write_state(&mut state, &hero);
        state.alive = player.0.is_alive();
        return;
    }

    // Hold still in FreeRoam (fly-cam drives the view), while down (awaiting respawn), or in build
    // mode (WASD then drives the build palette, not the knight). Keep the mirror current; `alive=false`
    // when down so orks stop chasing the corpse.
    if *mode != PlayMode::Play || !player.0.is_alive() || build_mode.active {
        hero.moving = false;
        hero.vel = Vec2::ZERO;
        hero.dash_t = -1.0; // cancel any in-flight dash (no corpse / frozen-hero skating)
        hero.roll_t = -1.0; // cancel any in-flight roll (same rule; rotation rights next Play frame)
        hero.stance_amt = 0.0; // drop the combat stance (and its gait twist) with it
        hero.strafe_twist = 0.0;
        hero.back_amt = 0.0;
        // The rig (hips joint in `anim`) owns the idle/walk bob — keep the root on the ground.
        tf.translation = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
        write_state(&mut state, &hero);
        state.alive = player.0.is_alive();
        return;
    }

    let dt = time.delta_secs().min(0.05);

    // ── Dodge roll: while a roll is armed (by `player_roll`), the roll OWNS locomotion — the body
    // slides `roll_from → roll_to` while the ROOT tumbles through a full somersault about the tucked
    // centre of mass (the feet-origin orbits `ROLL_PIVOT_H`, so the ball rolls in place instead of
    // cartwheeling about the ankles). `anim` tucks the limbs; the tumble spins backward for the
    // no-input backward dive. ──
    if hero.roll_t >= 0.0 {
        hero.roll_t += dt;
        let u = (hero.roll_t / ROLL_TIME).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - u) * (1.0 - u); // mild ease-out: springs off the line, settles to land
        hero.pos = hero.roll_from.lerp(hero.roll_to, e);
        hero.y = footing(hero.pos.x, hero.pos.y).unwrap_or(hero.y);
        hero.vel_y = 0.0;
        hero.on_ground = true;
        hero.moving = false;
        // The somersault: a full 2π pitch swept between 10%..90% of the roll (flat at both ends so
        // the tuck-in / stand-up carry the transition), spun the other way for a backward dive.
        let spin = {
            let s = ((u - 0.10) / 0.80).clamp(0.0, 1.0);
            s * s * (3.0 - 2.0 * s) // smoothstep — no snap at either end
        };
        let theta = std::f32::consts::TAU * spin * if hero.roll_back { -1.0 } else { 1.0 };
        let rot = Quat::from_rotation_y(hero.facing) * Quat::from_rotation_x(theta);
        let center = Vec3::new(hero.pos.x, hero.y + ROLL_PIVOT_H, hero.pos.y);
        tf.translation = center - rot * (Vec3::Y * ROLL_PIVOT_H);
        tf.rotation = rot;
        if u >= 1.0 {
            hero.roll_t = -1.0; // done — hand locomotion back to input next frame
            // Exit with travel momentum so a forward roll flows straight into the run.
            let dir = (hero.roll_to - hero.roll_from).normalize_or_zero();
            hero.vel = dir * SPEED * if hero.roll_back { 0.3 } else { 0.85 };
        } else {
            hero.vel = Vec2::ZERO;
        }
        write_state(&mut state, &hero);
        return;
    }

    // ── Sand Dash slide: while a dash is armed (by `arts::player_arts`), the dash OWNS locomotion —
    // the body skates `dash_from → dash_to` over DASH_TIME with an ease-out (explosive launch, glide
    // to a stop) instead of teleporting. Input is ignored for the blink; the path was pre-validated
    // standable in `arts`, so we just ride it and snap Y to the ground. ──
    if hero.dash_t >= 0.0 {
        hero.dash_t += dt;
        let u = (hero.dash_t / DASH_TIME).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - u).powi(3); // ease-out: fast off the line, settle at the end
        hero.pos = hero.dash_from.lerp(hero.dash_to, e);
        hero.y = footing(hero.pos.x, hero.pos.y).unwrap_or(hero.y);
        hero.vel = Vec2::ZERO;
        hero.vel_y = 0.0;
        hero.on_ground = true;
        hero.moving = false;
        if u >= 1.0 {
            hero.dash_t = -1.0; // blink done — hand locomotion back to input next frame
        }
        tf.translation = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
        tf.rotation = Quat::from_rotation_y(hero.facing);
        write_state(&mut state, &hero);
        return;
    }

    // ── Swamp poison: gnaws 2 HP every 2.5s while standing in the marsh, even idle (the first
    // tick fires the instant you step in — `swampPoisonAt` starts at 0 in the TS). ──
    let in_swamp = worldmap::biome_at_world(hero.pos.x, hero.pos.y) == Some(Biome::Swamp);
    if in_swamp {
        if !*was_swamp {
            *poison_acc = SWAMP_POISON_INTERVAL; // arm an immediate first tick on entry
        }
        *poison_acc += dt;
        if *poison_acc >= SWAMP_POISON_INTERVAL {
            *poison_acc -= SWAMP_POISON_INTERVAL;
            pending.0 += SWAMP_POISON;
        }
    }
    *was_swamp = in_swamp;

    // ── Camera-relative move vector, flattened to the ground plane ──
    let input = move_input(&keys, cam_q.single().ok());
    let move_dir = input.unwrap_or(Vec2::ZERO);
    let moving = input.is_some();
    hero.moving = moving;

    // ── Horizontal motion with axis-separated terrain + prop collision ──
    // No sprinting with sword and shield drawn: holding Shift in a fight first puts the weapons
    // away (`anim`'s sheathe, a moment's commitment), and only then does the hero break into a run.
    hero.sprint_held = moving && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    let sprinting = hero.sprint_held && !hero.weapons_drawn;
    // Smooth walk⇄run blend so the run pose eases in/out instead of snapping (anim reads `run_amt`).
    let run_target = if sprinting { 1.0 } else { 0.0 };
    hero.run_amt += (run_target - hero.run_amt) * (dt * 8.0).min(1.0);
    let cur_y = footing(hero.pos.x, hero.pos.y).unwrap_or(hero.y);

    // ── Combat stance: engaged while a soft-target is ringed AND blows have actually been traded
    // (`combat_until` — landing a hit or taking one starts it, and it lingers a few seconds past
    // the last exchange, so walking PAST a camp never squares you up). Sprint breaks it (turn and
    // run); first person is exempt. Smoothed so the anim twist and the camera's combat framing
    // fade in/out instead of snapping. ──
    // Disengage: the player holding a direction away from the ringed foe means leave, not backpedal.
    // A swing / block / charge, stopping, stepping back at the foe, or the fight going cold
    // re-engages.
    let to_foe = hero.soft_pos.map(|tp| (tp - hero.pos).normalize_or_zero());
    let toward = to_foe.map_or(0.0, |d| move_dir.dot(d));
    let away = moving && to_foe.is_some() && toward < DISENGAGE_DOT;
    hero.away_for = if away { hero.away_for + dt } else { 0.0 };
    if hero.away_for > DISENGAGE_HOLD {
        hero.disengaged = true;
    }
    let committed = hero.attacking || state.blocking || hero.charge_t >= 0.0;
    if committed || !moving || t >= hero.combat_until || (to_foe.is_some() && toward > REENGAGE_DOT) {
        hero.disengaged = false;
    }
    let stance = hero.soft_pos.is_some() && t < hero.combat_until && !sprinting && !fp.active && !hero.disengaged;
    let stance_target = if stance { 1.0 } else { 0.0 };
    hero.stance_amt += (stance_target - hero.stance_amt) * (dt * 8.0).min(1.0);
    // How the movement points relative to the body: 1 = with the facing, −1 = straight back.
    let facing_fwd = Vec2::new(hero.facing.sin(), hero.facing.cos());
    let along = if moving { move_dir.dot(facing_fwd) } else { 1.0 };
    // Non-forward movement is slower in the stance (strafe ×0.85 → backpedal ×0.72), scaled by
    // the stance blend so the penalty eases with it.
    let dir_mult = if along >= 0.0 {
        STANCE_STRAFE_MULT + (1.0 - STANCE_STRAFE_MULT) * along
    } else {
        STANCE_STRAFE_MULT + (STANCE_STRAFE_MULT - STANCE_BACK_MULT) * along
    };
    let stance_speed_mult = 1.0 + (dir_mult - 1.0) * hero.stance_amt;

    // ── Directional-gait drives for the animator: in the stance the pelvis+legs aim along the
    // MOVEMENT (torso/head counter-rotate onto the foe in `anim`), wrapping at ±90° — past that
    // the legs stay forward and the gait plays in REVERSE (a true backpedal; nobody twists their
    // knees 180°). Smoothed here so `anim` stays stateless. ──
    let (twist_target, back_target) = if stance && moving {
        let rel = steer::wrap_pi(move_dir.x.atan2(move_dir.y) - hero.facing);
        let (leg_rel, back) = if rel.abs() > std::f32::consts::FRAC_PI_2 {
            (steer::wrap_pi(rel - std::f32::consts::PI), 1.0)
        } else {
            (rel, 0.0)
        };
        ((leg_rel * STANCE_TWIST_FRAC).clamp(-STANCE_TWIST_MAX, STANCE_TWIST_MAX), back)
    } else {
        (0.0, 0.0)
    };
    hero.strafe_twist += (twist_target - hero.strafe_twist) * (dt * 9.0).min(1.0);
    hero.back_amt += (back_target - hero.back_amt) * (dt * 9.0).min(1.0);

    // ── Velocity ramp (momentum): accelerate toward the input target, slide to a stop on release —
    // instead of snapping to full speed / dead stop. ──
    let want_speed = SPEED
        * if sprinting { SPRINT_MULT } else { 1.0 }
        * player.0.move_speed_mult as f32
        * buffs.0.speed_mult(t as f64) as f32 // active Haste buff (1.0 = none)
        * if in_swamp { SWAMP_SLOW } else { 1.0 }
        // Travelling by road is a little quicker than bushwhacking (baked road-field lookup).
        * crate::roads::speed_mult(hero.pos.x, hero.pos.y)
        // Winding up a Heavy Strike commits you: slowed feet while the charge builds.
        * if hero.charge_t > super::combat::CHARGE_GRACE { super::combat::CHARGE_MOVE_MULT } else { 1.0 }
        // Combat stance: strafing/backpedaling around the ringed foe is slower than advancing.
        * stance_speed_mult;
    let travel_dir = if moving && hero.stance_amt < 0.5 {
        obstacle_assisted_dir(hero.pos, move_dir, hero.vel)
    } else {
        move_dir
    };
    let desired = if moving { travel_dir * want_speed } else { Vec2::ZERO };
    let ramp = if moving { ACCEL } else { DECEL }; // faster to start than to stop
    let new_vel = hero.vel + (desired - hero.vel) * (dt * ramp).min(1.0);
    hero.vel = new_vel;
    if hero.vel.length_squared() < 1e-4 {
        hero.vel = Vec2::ZERO; // settle exactly so a stopped hero doesn't creep
    }
    // No standing on a rock face: the hero slides down its fall line (input only steers him across
    // it, a little) until the ground is walkable again, or drops off where it turns sheer.
    if hero.on_ground && !crate::traps::in_trap(hero.pos) && crate::bridges::deck_y_at(hero.pos.x, hero.pos.y).is_none() {
        if let Some(dn) = steep_fall_with(worldmap::ground_at_world, hero.pos.x, hero.pos.y) {
            let down = hero.vel.dot(dn);
            let across = hero.vel - dn * down;
            hero.vel = across * 0.6 + dn * (down.max(SLIDE_START) + SLIDE_ACCEL * dt).min(SLIDE_MAX);
        }
    }

    // Anim weight tracks ACTUAL speed so the legs keep striding through the stop-slide (no foot-slide
    // while static, no snap to idle). Saturates at a slow walk: the gait itself is foot-locked at
    // every speed, so any idle blended in above that would only drag the planted boot.
    let speed_frac = (hero.vel.length() / GAIT_FULL_SPEED).clamp(0.0, 1.0);
    hero.moving_amt += (speed_frac - hero.moving_amt) * (dt * 13.0).min(1.0);

    if hero.vel.length_squared() > 1e-6 {
        let nx = hero.pos.x + hero.vel.x * dt;
        let nz = hero.pos.y + hero.vel.y * dt;
        // Collide the hero's whole BODY (radius `PLAYER_R`), not just his centre point: test the
        // blocker margin so he stops with his shoulder at the surface instead of sinking ~0.22u
        // into every wall/stone/prop (the "clip a bit / collision is unreliable" feel).
        // `escaping` lets a hero who is ALREADY overlapping a solid slide back out. Normal
        // locomotion halts him at the surface — an accepted step always leaves his centre
        // ≥ PLAYER_R from every face (where `any_within` reads false) — so this only trips when a
        // blocker was *registered on top of a stationary hero*: a wall/tower/ballista bought at the
        // War Table, or (the common case) a producer building raised in build mode on the very plot
        // he's standing on. The old test was `is_blocked` — centre strictly INSIDE a box — which
        // missed the penetration SHELL: centre just outside a face but within PLAYER_R of it. There
        // `is_blocked` is false, every axis move is rejected by `any_within`, and `vel.{x,y}=0` on
        // each rejection stops him ever building the single-frame displacement needed to clear the
        // inflated box — permanently wedged "inside the structure". Testing body-overlap frees him
        // from the shell too, and still can't be abused to clip a wall in play (he never legally
        // rests overlapping one, so it stays false except when a solid materialised around him).
        let escaping = blockers::any_within(hero.pos.x, hero.pos.y, PLAYER_R);
        // In the air, the climb reference is capped at the take-off ground (+ a hop's ledge): the
        // body's apex used to admit ground up to ~1.7u above the feet, so jump-and-drift scaled
        // cliff stairs ledge by ledge.
        let hy = if hero.on_ground { hero.y } else { hero.y.min(hero.air_takeoff_y + AIR_LEDGE) };
        // Trap pockets (`traps`): ground you could drop into but never climb out of. Its rim is
        // an edge you can't step over; a hero already inside one (old save, knock-back, the mask
        // still baking) scrambles out past the slope limit and up to a hop's height of ledge.
        let from = hero.pos;
        let trapped = crate::traps::in_trap(from);
        let free = |p: Vec2| {
            let climb_ok = if trapped {
                hero_can_step(p.x, p.y, PLAYER_R, hy + 1.0)
            } else {
                hero_can_step(p.x, p.y, PLAYER_R, hy) && slope_ok(p.x, p.y, hy) && !crate::traps::blocks_entry(from, p)
            };
            climb_ok && (escaping || !blockers::any_within(p.x, p.y, PLAYER_R))
        };
        // Too-steep ground ahead: glide along its contour rather than up it (see `slope_deflect_with`).
        let vel_in = if trapped || crate::bridges::deck_y_at(nx, nz).is_some() {
            hero.vel // on/onto a bridge deck: the planks are footing, not a slope
        } else {
            slope_deflect_with(worldmap::ground_at_world, hero.pos, hero.vel, dt, hy)
        };
        let (nx, nz) = if vel_in == hero.vel { (nx, nz) } else { (hero.pos.x + vel_in.x * dt, hero.pos.y + vel_in.y * dt) };
        let side = SLIDE_SIDE.load(std::sync::atomic::Ordering::Relaxed) as f32;
        let (pos, vel, new_side) = slide_step(hero.pos, Vec2::new(nx, nz) - hero.pos, vel_in, side, free);
        hero.pos = pos;
        hero.vel = vel;
        SLIDE_SIDE.store(new_side as i32, std::sync::atomic::Ordering::Relaxed);
        // Steer the facing. Free-run: toward the INPUT direction while pressing (hold the last
        // facing through the slide). Combat stance: onto the RINGED FOE — the body stays square
        // to it while WASD circles/backpedals (the movement itself is untouched; only the yaw
        // target changes). In first person the *view* owns facing (set in `player_camera`) so
        // attacks fire where you aim. This stays live DURING a swing — `player_attack` only
        // *gently* nudges facing toward the locked foe, and the steer here overpowers it.
        if moving && !fp.active {
            let (want, rate) = match hero.soft_pos {
                Some(tp) if stance && tp.distance_squared(hero.pos) > 1e-6 => {
                    let to = tp - hero.pos;
                    (to.x.atan2(to.y), STANCE_TURN_RATE)
                }
                _ => (move_dir.x.atan2(move_dir.y), TURN_RATE),
            };
            hero.facing = lerp_angle(hero.facing, want, (dt * rate).min(1.0));
        }
    }

    // ── Body-collision vs creatures: shove the hero out of any overlap so he can't clip through
    // an ork or animal (one-way push — the creature holds its ground). ──
    // The hero's shield state — a raised shield extends the keep-out out front (directional, in
    // `hero_guard_radius`) so a guarded attacker is held off the shield, not the torso. Read from
    // `HeroState` (the same source `wildlife`/`orks` use for the lunge clamp) so the render and the
    // collision shove agree on the keep-out line every frame.
    let blocking = state.blocking;
    for o in &bodies.orks {
        // Hold the hero's SHIELD/torso out of the ork's body, not just his slim nav centre: reserve
        // the visible guard half-width in place of `PLAYER_R` so a pressed-in knight can't bury his
        // shield in the ork (`shove_out_of` adds `PLAYER_R` back internally, so inflate `body_r` by
        // the difference → min gap = body_r + guard). `guard` grows out front while blocking.
        let guard = crate::orks::hero_guard_radius(hero.pos, hero.facing, blocking, o.pos);
        shove_out_of(&mut hero, o.pos, o.body_r + (guard - PLAYER_R), cur_y);
    }
    for a in &bodies.animals {
        // Hunting predators reserve a head-reach margin in front of the torso so the jaws they snap
        // forward on a bite land on the hero's front, not inside his chest (the strike-lunge render
        // is held to the same line). Use the BARE guard (never the blocking shield-reach extension):
        // a charging beast stops ~1.2 out to bite, and the extra shield reach pushed that keep-out
        // line PAST the bite stop, so a blocking hero got bulldozed backwards ("sliding") instead of
        // standing his ground while the beast bit his shield. The render-side clamp still uses the
        // full shield guard, so the snout is visually held off a raised shield regardless.
        let r = a.body_r + crate::wildlife::head_reach(a.species, a.body_r)
            + (crate::orks::HERO_GUARD_R - PLAYER_R);
        shove_out_of(&mut hero, a.pos, r, cur_y);
    }
    // Townsfolk are solid too — you bump them, you don't walk through them.
    for v in &bodies.villagers {
        let (p, r) = v.body();
        shove_out_of(&mut hero, p, r, cur_y);
    }
    // Wardens are huge — shove the hero out of their bulk (skip a dying one so a fading corpse
    // doesn't wall you off).
    for b in &bodies.bosses {
        let (c, r) = b.footprint();
        shove_out_of(&mut hero, c, r, cur_y);
    }

    // ── Vertical: jump + gravity + ground snap ──
    let ground_y = footing(hero.pos.x, hero.pos.y).unwrap_or(0.0);
    let was_on_ground = hero.on_ground;
    // No pushing off a too-steep face: a hop per landing would otherwise climb any rock wall.
    if keys.just_pressed(KeyCode::Space)
        && hero.on_ground
        && (!too_steep(hero.pos.x, hero.pos.y) || crate::traps::in_trap(hero.pos))
    {
        hero.vel_y = JUMP_SPEED;
        hero.on_ground = false;
        cues.write(AudioCue::HeroJump); // voice decides (~40% + canGrunt); no other jump sfx
    }
    hero.vel_y -= GRAVITY * dt;
    hero.y += hero.vel_y * dt;
    // Step-down snap (see STEP_SNAP): stepping off a terrace while already grounded and not jumping
    // keeps the hero glued to the ground instead of micro-falling, so the gait anim doesn't strobe.
    let snap_down = was_on_ground && hero.vel_y <= 0.0 && hero.y - ground_y <= STEP_SNAP;
    if hero.y <= ground_y || snap_down {
        // Just touched down: a long drop (cliff/jump) bruises on landing.
        if !was_on_ground {
            let fall = hero.air_takeoff_y - ground_y;
            if fall > FALL_SAFE {
                pending.0 += (((fall - FALL_SAFE) * FALL_DMG_PER_UNIT).round()).min(FALL_DMG_MAX);
                crate::combat_fx::add_fov_kick(&mut feedback, crate::combat_fx::FOV_KICK_LAND);
            }
        }
        hero.y = ground_y;
        hero.vel_y = 0.0;
        hero.on_ground = true;
    } else {
        if was_on_ground {
            hero.air_takeoff_y = hero.y;
        }
        hero.on_ground = false;
    }

    // ── Walk phase + body bob ──
    // Advance the gait by ACTUAL speed so footfalls match the accel/slide (and ease out, not cut).
    // The rate is the animator's foot-lock contract, so the planted boot never skates.
    let spd = hero.vel.length();
    hero.gait_speed = spd;
    hero.walk_phase += dt * super::anim::gait_phase_rate(spd);
    // Vertical bob is owned entirely by the rig (the hips joint in `anim`) so it's applied exactly
    // once — stacking a second bob here (at a different frequency) is what made the gait read
    // jittery/uncoordinated. The root just tracks the ground.
    tf.translation = Vec3::new(hero.pos.x, hero.y, hero.pos.y);
    tf.rotation = Quat::from_rotation_y(hero.facing);

    write_state(&mut state, &hero);
}
