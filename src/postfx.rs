//! **Cinematic lens** — vignette + chromatic aberration (built-in `bevy_post_process` effects,
//! inserted/removed per graphics preset by `quality::apply_quality`). The chromatic aberration is
//! punched up briefly on a hit so taking damage reads as a "rattled" colour split. Both are
//! live-tunable from the F1 panel via [`LookSettings`].
//!
//! (Film grain used to live here too — removed: it read as crawling static and added nothing.)
//! The per-biome COLOUR grade itself lives in `scene::advance_sky`; saturation rides `LookSettings`
//! so the F1 saturation slider actually sticks (`grade.rs` reads the base from here).

use bevy::post_process::effect_stack::{ChromaticAberration, Vignette};
use bevy::prelude::*;

use crate::combat_fx::HitFeedback;

/// Cinematic edge-darken — a constant lens fall-off (separate from `grade.rs`'s reactive HP
/// vignette, which is a UI overlay). Inserted on the premium presets by `quality::apply_quality`;
/// live-tunable in the F1 panel.
pub fn default_vignette() -> Vignette {
    // 0.26 (was 0.34): the atmospherics haze already frames the image; the heavier vignette
    // stacked on it read as dusk in broad daylight (2026-07 cinematic pass).
    Vignette { intensity: 0.26, radius: 0.78, smoothness: 1.6, ..default() }
}

// Chromatic aberration is disabled by default (user preference) — the component is no longer
// inserted by `quality::apply_quality`, so there's no `default_chromatic()` constructor. The
// `drive_chromatic` system + `LookSettings.chromatic` knob stay so it can be re-enabled later by
// re-inserting a `ChromaticAberration` component on the camera; with none present it's a no-op.

/// Live-tunable "look" knobs. The F1 panel edits these and the per-frame systems READ them, so a
/// slider sticks instead of being stomped each frame (`grade.rs` re-derives `post_saturation`
/// every frame, which is why editing the component directly never held). `saturation` is the big
/// washed-out lever — AgX + the 1.2 base can read flat; raise it here. Not persisted.
#[derive(Resource)]
pub struct LookSettings {
    /// Base `ColorGrading.global.post_saturation` (read by `grade.rs`, then × the hit-drain).
    pub saturation: f32,
    /// Chromatic-aberration baseline intensity (the at-rest fringe; hits add a spike on top).
    pub chromatic: f32,
}

impl Default for LookSettings {
    fn default() -> Self {
        // 0.98 (was 1.1) — 2026-07 cinematic pass: the filmic reference look is gently
        // desaturated; the atmospherics haze now carries the colour mood instead.
        Self { saturation: 0.98, chromatic: 0.0 } // chromatic off by default (component not inserted anyway)
    }
}

pub struct PostFxPlugin;

impl Plugin for PostFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LookSettings>().add_systems(Update, drive_chromatic);
    }
}

/// Lens colour-fringe: a faint constant baseline + a spike on a fresh hit (HitFeedback.flash bleeds
/// down ~1.6/s after a blow), so getting struck reads as a brief rattled split. No-ops on Low (the
/// component is stripped there, so the query is empty).
fn drive_chromatic(look: Res<LookSettings>, fb: Res<HitFeedback>, mut q: Query<&mut ChromaticAberration>) {
    if let Ok(mut ca) = q.single_mut() {
        ca.intensity = look.chromatic + fb.flash * 0.05;
    }
}
