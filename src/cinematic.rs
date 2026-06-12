//! Trailer staging **director** — triggerable staged scenes/animations for filming a trailer.
//! The user flies their OWN free-cam (` toggles it); this module only stages the WORLD: a fast
//! day→night→dawn sky, custom hero gestures the game never plays, a castle build timelapse, and
//! an ork column marching out of Gnashfang Hold. Everything is fired live from the F1 debug
//! panel's "🎬 Director" section, which mutates [`DirectorState`].
//!
//! Each scene's heavy lifting lives in the module that owns the relevant API (build → `town.rs`,
//! ork march → `siege.rs`, hero gesture → `player/anim.rs`); this module owns the shared state,
//! the self-contained sky timelapse, and the gesture-phase clock.

use bevy::prelude::*;

use crate::scene::SkyClock;

pub struct CinematicPlugin;

/// Hold every director-triggered animation this long after its trigger before motion starts, so
/// the user has time to close the F1 panel and set the free-cam. Consumed by the gesture clock +
/// sky timelapse (here), the build timelapse (`town.rs`), the fortress march (`siege.rs`), the
/// fortress gate swing, and the staged-scene loops (`scenes.rs`).
pub const PRE_ROLL: f32 = 2.0;

/// Staged hero gestures the normal game never plays — for "performance" trailer shots. Held until
/// cleared; looping ones (Wave/Cheer/Work) phase off [`DirectorState::gesture_start`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeroGesture {
    /// Right arm overhead, hand waving side to side.
    Wave,
    /// Right hand snapped to the brow.
    Salute,
    /// Right arm thrust forward, commanding.
    Point,
    /// Both arms folded across the chest (the "supervise" idle).
    ArmsCrossed,
    /// Both arms thrown overhead.
    Cheer,
    /// A looping chop/hammer swing — the "at work" animation.
    Work,
}

/// Tags an ork that's part of a staged fortress march. Driven by `siege::director_march`, and
/// explicitly SKIPPED by the normal camp brain (`orks::ork_brain`) so the two don't fight over it.
#[derive(Component)]
pub struct DirectorMarcher;

/// Shared trailer-staging state. The F1 panel writes it; the per-scene systems read/consume it.
#[derive(Resource)]
pub struct DirectorState {
    /// Day→night→dawn timelapse: while on, the sky clock is driven at `sky_speed` (t units/sec).
    pub sky_run: bool,
    pub sky_speed: f32,
    /// Held hero gesture (None = normal animation); `gesture_start` is the loop-phase origin.
    pub gesture: Option<HeroGesture>,
    pub gesture_start: f32,
    /// Castle build timelapse: raise the whole stronghold piece by piece in real time.
    pub build_run: bool,
    /// Edge triggers consumed once by `siege::director_march`.
    pub march: bool,
    pub clear_marchers: bool,
    /// Fortress gate: swing the leaves open + drop the gate blocker so a sally can path through.
    /// Auto-raised when a march is triggered; can also be toggled on its own from the panel.
    pub gate_open: bool,
    /// Hide the hero's held weapon (sword/axe/maul) — for weapon-free staged gestures (wave/cheer).
    pub hide_weapon: bool,
}

impl Default for DirectorState {
    fn default() -> Self {
        Self {
            sky_run: false,
            sky_speed: 0.06, // ≈17 s for a full day
            gesture: None,
            gesture_start: 0.0,
            build_run: false,
            march: false,
            clear_marchers: false,
            gate_open: false,
            hide_weapon: false,
        }
    }
}

impl HeroGesture {
    /// Gestures meant to read as an *empty* hand — the held weapon looks wrong raised overhead, so
    /// the weapon-visibility system hides it for these (Salute/Point keep the blade, it suits them).
    pub fn wants_empty_hand(self) -> bool {
        matches!(self, HeroGesture::Wave | HeroGesture::Cheer)
    }
}

impl Plugin for CinematicPlugin {
    fn build(&self, app: &mut App) {
        // `FOREST_GESTURE=wave|salute|point|arms|cheer|work` stages a hero gesture at boot (for a
        // screenshot of the pose) — the same staging-hook style as the other `FOREST_*` vars.
        let mut state = DirectorState::default();
        if let Ok(g) = std::env::var("FOREST_GESTURE") {
            state.gesture = match g.trim().to_ascii_lowercase().as_str() {
                "wave" => Some(HeroGesture::Wave),
                "salute" => Some(HeroGesture::Salute),
                "point" => Some(HeroGesture::Point),
                "arms" | "armscrossed" | "cross" => Some(HeroGesture::ArmsCrossed),
                "cheer" => Some(HeroGesture::Cheer),
                "work" => Some(HeroGesture::Work),
                _ => None,
            };
        }
        app.insert_resource(state).add_systems(
            Update,
            (sky_timelapse, track_gesture, animate_fortress_gate, weapon_visibility),
        );
    }
}

/// Hide/show the hero's held weapon. Hidden when the panel's "hide weapon" is on OR the active
/// gesture wants an empty hand (wave/cheer) — so those poses don't brandish a stray sword.
fn weapon_visibility(
    state: Res<DirectorState>,
    mut q: Query<&mut Visibility, With<crate::player::HeroWeapon>>,
) {
    let hide = state.hide_weapon || state.gesture.is_some_and(|g| g.wants_empty_hand());
    let want = if hide { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut q {
        if *v != want {
            *v = want;
        }
    }
}

/// The fortress gate's shut-line OBB blocker (registered in `ork_fortress::build`); dropping it
/// opens the wall gap to A*. Kept in sync with the leaf swing below.
const GATE_BLOCKER: (f32, f32) = (12.0, 80.9);
/// Fully-open leaf swing (radians) — ~95°, the leaves laid back against the wall.
const GATE_OPEN_ANGLE: f32 = 1.65;

/// Swing the fortress gate leaves toward open/shut (eased) and toggle the gate blocker so the
/// nav-grid gap opens the moment the leaves are cracked. State lives on [`DirectorState::gate_open`]
/// (set directly, or auto-raised by a march). A `Local` mirrors the blocker so we add/remove it
/// exactly on the edges, not every frame.
fn animate_fortress_gate(
    state: Res<DirectorState>,
    time: Res<Time>,
    mut leaves: Query<(&mut crate::ork_fortress::FortressGate, &mut Transform)>,
    mut blocker_dropped: Local<bool>,
    mut was_open: Local<bool>,
    mut hold: Local<f32>,
) {
    // Camera-setting grace on the OPENING edge (closing stays immediate): leaves stay shut and the
    // blocker stays up until the swing actually begins.
    if state.gate_open && !*was_open {
        *hold = PRE_ROLL;
    }
    *was_open = state.gate_open;
    if state.gate_open && *hold > 0.0 {
        *hold -= time.delta_secs();
        return;
    }
    let target = if state.gate_open { 1.0 } else { 0.0 };
    // Drop the blocker as soon as we *start* opening; restore it only once fully shut.
    if state.gate_open && !*blocker_dropped {
        crate::blockers::remove_box_near(GATE_BLOCKER.0, GATE_BLOCKER.1, 0.5);
        *blocker_dropped = true;
    }
    let k = 1.0 - (-time.delta_secs() * 3.0).exp(); // ~0.33s settle
    for (mut gate, mut tf) in &mut leaves {
        gate.open += (target - gate.open) * k;
        if gate.open < 0.001 {
            gate.open = 0.0;
        }
        // Swing OUTWARD (−Z): right leaf (+sign) needs a negative Y-rotation, left a positive.
        tf.rotation = Quat::from_rotation_y(-gate.sign * gate.open * GATE_OPEN_ANGLE);
    }
    if !state.gate_open && *blocker_dropped && leaves.iter().all(|(g, _)| g.open == 0.0) {
        crate::blockers::add_obb(GATE_BLOCKER.0, GATE_BLOCKER.1, 3.2, 0.5, 0.0);
        *blocker_dropped = false;
    }
}

/// Drive a fast day/night cycle while `sky_run` is on; hand the clock back to the normal
/// phase-driven sky the moment it's switched off.
fn sky_timelapse(
    state: Res<DirectorState>,
    time: Res<Time>,
    mut clock: ResMut<SkyClock>,
    mut was: Local<bool>,
    mut hold: Local<f32>,
) {
    if state.sky_run {
        if !*was {
            *hold = PRE_ROLL; // camera-setting grace before the sky starts rolling
        }
        clock.paused = true;
        if *hold > 0.0 {
            *hold -= time.delta_secs();
        } else {
            clock.t = (clock.t + state.sky_speed * time.delta_secs()).rem_euclid(1.0);
        }
    } else if *was {
        clock.paused = false;
    }
    *was = state.sky_run;
}

/// Stamp `gesture_start` whenever the active gesture changes, so looping gestures have a phase 0.
/// Stamped [`PRE_ROLL`] into the future: the pose's phase stays ≤ 0 (rest, ease not begun) for the
/// camera-setting grace, then lifts into place.
fn track_gesture(
    mut state: ResMut<DirectorState>,
    time: Res<Time>,
    mut prev: Local<Option<HeroGesture>>,
) {
    if state.gesture != *prev {
        state.gesture_start = time.elapsed_secs() + PRE_ROLL;
        *prev = state.gesture;
    }
}
