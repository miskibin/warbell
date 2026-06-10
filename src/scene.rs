//! Camera + lighting + post-processing — the polished daytime look. Ports the TS
//! pipeline (AgX, bloom, DoF background blur, fog, warm sun, soft ambient) onto the
//! verified Bevy 0.18 components, plus a procedural gradient-cubemap IBL and SSAO
//! (both adapted from the working tileworld-bevy port's `lighting.rs`).

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{
    CascadeShadowConfigBuilder, DirectionalLightShadowMap, FogVolume, ShadowFilteringMethod,
    SunDisk, VolumetricFog,
};
use bevy::pbr::{
    Atmosphere, DistanceFog, FogFalloff, ScatteringMedium, ScreenSpaceAmbientOcclusion,
    ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use bevy::render::view::{ColorGrading, Hdr};

use crate::biome::{AtmoSample, BiomeAmbiences};
use crate::game_state::{AppState, Modal};
use crate::player::HeroState;
use crate::siege::{GamePhase, Siege};

/// Sky / fog horizon colour — bright pale daytime blue.
const SKY: Color = Color::srgb(0.70, 0.82, 0.93);
/// Daytime distance-fog colour — warm cream haze (NOT the sky blue: a pale-blue fog reads as
/// milky white-out across the whole frame, a warm haze reads as sunlit atmosphere).
const FOG_DAY: Color = Color::srgb(0.85, 0.80, 0.66);
const FOG_DENSITY: f32 = 0.009;
const IBL_INTENSITY: f32 = 430.0;

/// How strongly the hero's current biome tints the DAYTIME light's mood (0 = none, 1 = the
/// biome's authored colour fully). Scaled by `day`, so night stays the tuned moonlit look.
const BIOME_TINT_W: f32 = 0.7;
/// How fast the biome tint eases as you cross a region boundary (exponential, per second).
const BIOME_ATMO_LERP: f32 = 0.9;
/// Island-wide reference sun lux the per-biome illuminance nudge is measured against.
const BASE_SUN_LUX: f32 = 11_000.0;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.88, 0.93, 1.0),
                brightness: 85.0,
                affects_lightmapped_meshes: true,
            })
            // 2048 (was 4096) — paired with the tighter 75-tile shadow cascade below, each
            // cascade now covers less ground, so per-cascade resolution near the player holds
            // up while the shadow pass costs ~¼ the fill. Far shadows are fogged out anyway.
            .insert_resource(DirectionalLightShadowMap { size: 2048 })
            .insert_resource(SkyClock {
                t: start_t(),
                // Freeze the clock for screenshots so frame 90 is deterministic.
                paused: std::env::var("FOREST_SHOT").is_ok(),
                day_secs: day_seconds(),
            })
            .init_resource::<SmoothBiomeAtmo>()
            .add_systems(Startup, (setup_camera, setup_sun))
            .add_systems(Update, ((track_biome_atmo, advance_sky).chain(), drive_dof_focus));
    }
}

// ── Day / night cycle ────────────────────────────────────────────────────────────
//
// One clock `t ∈ [0,1)` sweeps the sun through the sky. The DirectionalLight IS the
// Atmosphere's sun, so moving it slides the sky gradient + sun disk automatically; we
// just drive its angle/colour/brightness and tint fog + ambient + IBL to match.
//   t=0 dawn (east horizon) · 0.25 noon · 0.5 dusk (west) · 0.75 midnight.
// Knobs (no rebuild): FOREST_DAY="seconds" (full cycle), FOREST_TIME="0..1" (start).
// Keys: P pause/resume, [ / ] scrub time back/forward.

/// Marks the sun so the day/night system can drive only it (not future lights).
#[derive(Component)]
pub struct Sun;

#[derive(Resource)]
pub struct SkyClock {
    pub t: f32,
    pub paused: bool,
    pub day_secs: f32,
}

fn day_seconds() -> f32 {
    std::env::var("FOREST_DAY")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|v| *v > 1.0)
        .unwrap_or(150.0)
}

fn start_t() -> f32 {
    if let Ok(s) = std::env::var("FOREST_TIME") {
        if let Ok(v) = s.trim().parse::<f32>() {
            return v.rem_euclid(1.0);
        }
    }
    // A FOREST_WAVE screenshot boots at midnight so the night assault reads as night.
    if std::env::var("FOREST_WAVE").is_ok() {
        return T_NIGHT;
    }
    0.22 // mid-morning: sun climbing, long-ish shadows
}

// ── Phase-driven time of day (ported from DayNight.tsx) ───────────────────────────
// The siege phase drives the clock: the prep "day" sweeps the sun across the sky as a
// countdown (a glance tells you how long until night), the night holds through the whole wave,
// and end screens snap to daylight. The clock EASES toward the target so dusk/dawn fall over a
// few seconds.
// The 150s prep day (siege PREP_DURATION) maps to the sun's full arc: ~5:00 low golden
// morning (east) → 13:00 noon (overhead) → ~21:00 low golden evening (west). Wider than
// before so the sun visibly rises and sets across the day instead of hanging mid-high.
const T_DAWN: f32 = 0.03; // ~5:00 — low sunrise sun, east horizon
const T_DUSK: f32 = 0.47; // ~21:00 — low sunset sun, west horizon
const T_NIGHT: f32 = 0.75; // midnight — held for the whole wave
const T_NOON: f32 = 0.25; // end-screen daylight
const DAY_LERP_RATE: f32 = 0.7; // ease speed toward the target (≈ a few-second dusk/dawn)

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_col(a: Color, b: Color, t: f32) -> Color {
    let (a, b) = (a.to_linear(), b.to_linear());
    Color::LinearRgba(LinearRgba {
        red: a.red + (b.red - a.red) * t,
        green: a.green + (b.green - a.green) * t,
        blue: a.blue + (b.blue - a.blue) * t,
        alpha: 1.0,
    })
}

// ── Per-biome atmosphere tint ─────────────────────────────────────────────────────
//
// `advance_sky` owns the day/night light. To make each biome region *feel* distinct, we
// ease a tint toward the biome the hero stands in (captured into `BiomeAmbiences` at world
// build) and blend it into the day/night sun/ambient/fog by `day` — so the desert reads
// warm + bright, the snowfield cool, the swamp dim + green, and grass/coast the island base.

/// The smoothed biome atmosphere the hero is currently in (eased so crossing a region edge
/// fades the mood instead of popping). `None` until the world + `BiomeAmbiences` exist.
#[derive(Resource, Default)]
struct SmoothBiomeAtmo(Option<AtmoSample>);

/// Ease [`SmoothBiomeAtmo`] toward the biome under the hero each frame.
fn track_biome_atmo(
    time: Res<Time>,
    hero: Option<Res<HeroState>>,
    ambiences: Option<Res<BiomeAmbiences>>,
    mut state: ResMut<SmoothBiomeAtmo>,
) {
    let (Some(hero), Some(ambiences)) = (hero, ambiences) else { return };
    let target = ambiences.sample(crate::worldmap::biome_at_world(hero.pos.x, hero.pos.y)).atmo;
    let k = 1.0 - (-time.delta_secs() * BIOME_ATMO_LERP).exp();
    match &mut state.0 {
        None => state.0 = Some(target), // snap on the first frame the world exists
        Some(cur) => {
            cur.sun_color = lerp_col(cur.sun_color, target.sun_color, k);
            cur.ambient_color = lerp_col(cur.ambient_color, target.ambient_color, k);
            cur.sky = lerp_col(cur.sky, target.sky, k);
            cur.sun_illuminance += (target.sun_illuminance - cur.sun_illuminance) * k;
            cur.ambient_brightness += (target.ambient_brightness - cur.ambient_brightness) * k;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn advance_sky(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    app: Res<State<AppState>>,
    modal: Option<Res<State<Modal>>>,
    siege: Option<Res<Siege>>,
    mut clock: ResMut<SkyClock>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun_q: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut fog_q: Query<&mut DistanceFog>,
    mut env_q: Query<&mut GeneratedEnvironmentMapLight>,
    mut grade_q: Query<&mut ColorGrading>,
    biome: Option<Res<SmoothBiomeAtmo>>,
) {
    let dt = time.delta_secs();
    if keys.just_pressed(KeyCode::KeyP) {
        clock.paused = !clock.paused;
    }
    // A paused game (or an open shop/tree/satchel panel) freezes the world — so time-of-day must
    // hold too, not drift on. Mirrors the sim freeze gate. The sun's transform/colour below still
    // applies every frame, so the frozen scene keeps drawing.
    let frozen = *app.get() == AppState::Paused || modal.is_some_and(|m| *m.get() != Modal::None);
    if !clock.paused && !frozen {
        match siege.as_deref() {
            // Phase-driven: the prep day is a sky-as-countdown, night holds through the wave.
            Some(s) => {
                let target = match s.phase {
                    GamePhase::Prep => {
                        let prog = crate::siege::prep_progress(
                            s.prep_seconds_left,
                            crate::siege::mods_for(s.difficulty),
                        );
                        T_DAWN + (T_DUSK - T_DAWN) * prog
                    }
                    GamePhase::Wave => T_NIGHT,
                    GamePhase::Victory | GamePhase::Defeat => T_NOON,
                };
                // Ease along the SHORTEST arc on the [0,1) circle, so the night→dawn wrap goes
                // forward through midnight (sun rises in the east) rather than rewinding.
                let mut diff = target - clock.t;
                diff = (diff + 0.5).rem_euclid(1.0) - 0.5;
                clock.t += diff * (dt * DAY_LERP_RATE).min(1.0);
            }
            // Fallback (siege not yet inserted): the old free-running clock.
            None => clock.t += dt / clock.day_secs,
        }
    }
    // Manual scrub (hold) — handy to jump to a sunrise/sunset.
    if keys.pressed(KeyCode::BracketRight) {
        clock.t += dt * 0.15;
    }
    if keys.pressed(KeyCode::BracketLeft) {
        clock.t -= dt * 0.15;
    }
    clock.t = clock.t.rem_euclid(1.0);

    // Sun direction (from origin toward the sun): +X east, +Y up, −X west, with a strong
    // constant +Z (south) tilt so the sun never passes straight overhead — even at noon the
    // light stays slanted and every tree keeps a visible cast shadow (flat-noon light was a
    // big part of the washed "no depth" look).
    let a = clock.t * std::f32::consts::TAU;
    let sun_dir = Vec3::new(a.cos(), a.sin(), 0.55).normalize();
    let elev = sun_dir.y; // −1 (midnight) .. 1 (noon)

    let day = smoothstep(-0.02, 0.22, elev); // 0 deep night → 1 full day
    let high = smoothstep(0.0, 0.45, elev); // 0 at horizon → 1 overhead
    let horizon = day * (1.0 - high); // peaks at sunrise/sunset
    // Eases in as the sun dips toward/below the horizon — keeps the sunrise/sunset glow
    // bright, then ramps the world into a dark moonlit night.
    let night = 1.0 - smoothstep(-0.22, 0.08, elev);

    // Per-biome mood tint, only in daylight (night stays the tuned moonlit look).
    let tint = biome.and_then(|b| b.0);
    let bw = day * BIOME_TINT_W;

    for (mut light, mut tf) in &mut sun_q {
        *tf = Transform::from_translation(sun_dir * 120.0).looking_at(Vec3::ZERO, Vec3::Y);
        // A modest moonlight floor (≈300 lux): enough for soft directional moonlight, but
        // low enough that the procedural Atmosphere sky stays dark/moody after dark. Ground
        // visibility comes from ambient + IBL below. Daytime peak raised (≈14 100) with the
        // ambient/IBL fill cut below — the sun should dominate the fill so lit vs shadowed
        // ground actually contrasts instead of washing flat.
        light.illuminance = 1100.0 + 13_000.0 * day;
        // Warm at the horizon → warm gold overhead (never neutral-white: the warm key light
        // is what gives the daytime scene its colour depth), then cooled toward moonlit blue
        // as the sun drops below the horizon (so the "moon" doesn't cast an orange glow).
        let warm = lerp_col(Color::srgb(1.0, 0.45, 0.22), Color::srgb(1.0, 0.90, 0.70), high);
        light.color = lerp_col(warm, Color::srgb(0.55, 0.66, 1.0), night * 0.8);
        // Biome tint: warm the desert sun, cool the snow, etc., and nudge brightness toward
        // the biome's authored sun lux (desert brighter, swamp dimmer) — daytime only.
        if let Some(t) = tint {
            light.color = lerp_col(light.color, t.sun_color, bw);
            light.illuminance *= 1.0 + (t.sun_illuminance / BASE_SUN_LUX - 1.0) * bw;
        }
    }

    // Ambient: the night floor stays HIGH (≈215) since it — not the dimmed moonlight — is
    // what lights the ground after dark (and ambient doesn't feed the Atmosphere sky, so it
    // brightens the ground without re-brightening the moody sky). But by DAY the ambient
    // *dips* (≈140): the sun is the day's fill, and a strong cool ambient was flattening the
    // lit/shadow contrast into the washed look. (Computed from `day`, never read-back, so it
    // can't compound frame-to-frame.)
    ambient.brightness = 215.0 - 75.0 * day;
    ambient.color = lerp_col(Color::srgb(0.50, 0.60, 0.95), Color::srgb(1.0, 0.95, 0.86), day);
    // Biome tint on the ambient fill colour (brightness stays on the scene's tuned curve).
    if let Some(t) = tint {
        ambient.color = lerp_col(ambient.color, t.ambient_color, bw);
    }

    // IBL (baked daytime) dimmed at night, but kept a strong floor (≈320) so surfaces still
    // catch skylight after dark — the other half of the after-dark ground light. The daytime
    // value is deliberately modest (430): like ambient above, too much skylight fill kills
    // the sun's shadow contrast.
    for mut env in &mut env_q {
        env.intensity = 320.0 + (IBL_INTENSITY - 320.0) * day;
    }

    // Darken night at the GRADE stage. Camera `Exposure` only scales PBR lighting, but
    // after dark the scene is lit almost entirely by the Atmosphere sky (which bypasses
    // Exposure) — so a final-image stops cut here is what actually makes night read as a
    // dark, blue moonlit night instead of AgX dusk. Depth tunable via FOREST_NIGHT.
    let night_stops = std::env::var("FOREST_NIGHT")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .unwrap_or(1.0);
    for mut g in &mut grade_q {
        g.global.exposure = -night * night_stops;
    }

    // Fog: night navy → day warm-cream haze, warmed orange at sunrise/sunset. The night navy
    // is lifted off near-black so the world isn't swallowed by black fog after dark.
    let mut fog_col = lerp_col(Color::srgb(0.06, 0.08, 0.15), FOG_DAY, day);
    fog_col = lerp_col(fog_col, Color::srgb(1.0, 0.5, 0.3), horizon * 0.6);
    // Biome tint on the daytime fog/haze colour (snow pale-cool, desert warm).
    if let Some(t) = tint {
        fog_col = lerp_col(fog_col, t.sky, bw);
    }
    for mut fog in &mut fog_q {
        fog.color = fog_col;
        // Sun-toward-camera in-scatter glow — warm by day, but faded out into the plain fog
        // colour after dark (else the below-horizon "sun" paints a warm-orange dusk band on
        // the night horizon).
        fog.directional_light_color = lerp_col(light_glow_color(high), fog_col, night);
    }
}

/// The fog's directional in-scatter (sun-toward-camera glow) — warm low, pale high.
fn light_glow_color(high: f32) -> Color {
    lerp_col(Color::srgb(1.0, 0.6, 0.35), Color::srgb(1.0, 0.93, 0.78), high)
}

fn setup_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
) {
    let env = images.add(gradient_env_cubemap());
    let medium = media.add(ScatteringMedium::default());

    // Low, immersive starting pose among the trees; fly controls take over from here.
    // `FOREST_CAM="x,y,z,tx,ty,tz"` overrides it (handy for framing diagnostics).
    // Default pose = an elevated overview of the whole island; `FOREST_CAM` overrides.
    // Pulled back ×1.4 to frame the enlarged island (was 0,44,80 → look -14).
    let cam_tf = env_cam().unwrap_or_else(|| {
        Transform::from_xyz(0.0, 62.0, 112.0).looking_at(Vec3::new(0.0, 0.0, -20.0), Vec3::Y)
    });
    let (yaw, pitch, _) = cam_tf.rotation.to_euler(EulerRot::YXZ);

    let mut grading = ColorGrading::default();
    grading.global.post_saturation = 1.2;
    grading.shadows.contrast = 1.05;
    grading.midtones.contrast = 1.35;
    grading.highlights.contrast = 1.1;

    commands.spawn((
        Camera3d::default(),
        // far=230 (was the 1000 default). The Linear fog reaches full horizon colour by 190
        // tiles (biome.rs), so everything past ~190 is solid fog — invisible but still drawn at
        // full cost. Clipping the frustum at 230 (40-tile margin) lets Bevy's frustum culler
        // drop all that far geometry for free: the opposite island edge / fogged ground-cover
        // stops being submitted when the player roams to a far shore. No visible change.
        Projection::from(PerspectiveProjection { fov: 50f32.to_radians(), far: 230.0, ..default() }),
        cam_tf,
        Hdr,
        Exposure { ev100: 10.60 },
        Tonemapping::AgX,
        // SSAO + SMAA path (mutually exclusive with MSAA). Bevy's built-in DepthOfField is
        // gone — it silently no-op'd next to SSAO and only did a single focal plane. Depth
        // blur is now our own `depth_blur` post pass (below), which only READS the prepass
        // depth, so SSAO + NormalPrepass are safe to keep.
        Msaa::Off,
        Smaa { preset: SmaaPreset::High },
        ScreenSpaceAmbientOcclusion {
            // Medium (was High): AO is a subtle contact-shadow read near the camera; the High→
            // Medium drop is barely perceptible but trims a chunk of the fullscreen prepass cost.
            quality_level: ScreenSpaceAmbientOcclusionQualityLevel::Medium,
            ..default()
        },
        DepthPrepass,
        NormalPrepass,
        Bloom { intensity: 0.30, ..Bloom::NATURAL },
        // Custom CoC **bokeh** depth-of-field (Bevy's built-in no-ops here): a focal plane
        // auto-focused on the player by `drive_dof_focus`, fore/background melting into
        // bokeh. Tunable live in F1 → Blur+Bloom (sharp band / blur radius).
        crate::dof::default_dof(),
        DistanceFog {
            color: SKY,
            directional_light_color: Color::srgb(1.0, 0.93, 0.78),
            directional_light_exponent: 12.0,
            falloff: FogFalloff::ExponentialSquared { density: FOG_DENSITY },
        },
        GeneratedEnvironmentMapLight { environment_map: env, intensity: IBL_INTENSITY, ..default() },
    ))
    // Procedural sky (0.18 headline feature) — real blue sky + sun disk + horizon
    // glow, using the DirectionalLight as the sun. Plus a saturation grade to richen
    // the AgX look back toward the TS palette.
    .insert((
        Atmosphere::earthlike(medium),
        grading,
        // Volumetric sun shafts (god rays) — the modern, integrated replacement for the TS
        // screen-space `GodRays` pass. Lit by the sun's `VolumetricLight` inside the `FogVolume`
        // spawned in `setup_sun`. `jitter = 0` since we run SMAA, not TAA (jitter needs temporal
        // resolve); `ambient_intensity = 0` so only the sun's shafts add brightness — a high
        // value washed distant geometry into a bright white sky-haze. Tunable live in F1 → Fog.
        VolumetricFog { ambient_intensity: 0.0, jitter: 0.0, step_count: 32, ..default() },
        ShadowFilteringMethod::Gaussian,
        // Toon edge-outline (runs before the blur): crisp object silhouettes. Tunable in F1.
        crate::outline::default_outline(),
        crate::controls::FlyCam { yaw, pitch },
        // Listener for spatial wildlife audio (see `audio.rs`). `gap` = ear separation in
        // world units; scaled by the global `SpatialScale` set in `main.rs`.
        SpatialListener::new(4.0),
    ));
}

/// Parse `FOREST_CAM="x,y,z,tx,ty,tz"` into a camera transform, if set.
fn env_cam() -> Option<Transform> {
    let s = std::env::var("FOREST_CAM").ok()?;
    let v: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    if v.len() == 6 {
        Some(Transform::from_xyz(v[0], v[1], v[2]).looking_at(Vec3::new(v[3], v[4], v[5]), Vec3::Y))
    } else {
        None
    }
}

/// Auto-focus the bokeh DoF on the player (Play mode) or a fixed mid-ground plane
/// (free-cam) — mirrors the old game's DofDriver (focusDistance = camera→player distance),
/// so the hero stays sharp while the fore/background melt into bokeh.
fn drive_dof_focus(
    mode: Res<crate::player::PlayMode>,
    mut cam_q: Query<(&GlobalTransform, &mut crate::dof::Dof), With<Camera3d>>,
    hero_q: Query<&crate::player::Hero>,
) {
    let Ok((cam_tf, mut dof)) = cam_q.single_mut() else {
        return;
    };
    let target = if *mode == crate::player::PlayMode::Play {
        hero_q
            .single()
            .map(|h| cam_tf.translation().distance(Vec3::new(h.pos.x, h.y + 1.0, h.pos.y)))
            .unwrap_or(28.0)
    } else {
        28.0 // free-cam: a fixed mid-ground focal plane
    };
    dof.focal = target;
}

fn setup_sun(mut commands: Commands) {
    commands.spawn((
        Sun,
        DirectionalLight {
            color: Color::srgb(1.0, 0.93, 0.78), // warm #ffe6b3-ish
            illuminance: 10_500.0,
            shadows_enabled: true,
            ..default()
        },
        // NB: no `VolumetricLight` here. The sun's half of the god-rays effect (the camera's
        // `VolumetricFog` is the other half) is the *only* runtime switch for the volumetric pass
        // — Bevy tears the pass down when no `VolumetricLight` exists. The God Rays graphics preset
        // (`quality.rs`) inserts it on demand; spawning it here would run the pass for the first
        // frame or two before `apply_quality` removes it, seeding a stale ~20 ms reading that the
        // F2 GPU-passes table then shows forever. Default presets (High/Low) never pay for it.
        // Visible solar disk in the Atmosphere sky. Default is `SunDisk::EARTH` —
        // physically-accurate 0.0093 rad (≈0.5°), a barely-visible dot. The stylized look
        // wants a big warm ball: ~6× earth size, overexposed so Bloom halos it into a glow.
        SunDisk { angular_size: 0.060, intensity: 1.6 },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            // 150 (was 75): with the elevated follow-cam most of the visible frame sits 60–150
            // tiles out, and a 75-tile cutoff left the whole mid/far ground shadowless — flat.
            // Long tree shadows ARE the scene's depth cue; the linear fog only fully wins by
            // ~190, so shadows must reach well past 100 to read in the mid-ground.
            maximum_distance: 150.0,
            first_cascade_far_bound: 12.0,
            ..default()
        }
        .build(),
        // High, slightly-side sun → bright blue daytime sky + soft directional shadows.
        Transform::from_xyz(16.0, 40.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // A single big fog box enclosing the island + the air above it; the sun's shafts only
    // render inside this volume. Low `density_factor` keeps it subtle (gentle haze + shafts,
    // not pea-soup); raised toward the canopy height so beams read between the trees. The
    // "God Rays" graphics preset (src/quality.rs) overrides these for clearly-visible shafts.
    commands.spawn((
        FogVolume {
            density_factor: 0.012, // very thin — just enough to catch shafts, not murk the view
            absorption: 0.05,      // low: don't darken what's seen through the volume
            scattering: 0.5,
            ..default()
        },
        Transform::from_xyz(0.0, 18.0, 0.0).with_scale(Vec3::new(320.0, 50.0, 320.0)),
    ));
}

// ── Procedural gradient-cubemap IBL (adapted from tileworld-bevy lighting.rs) ──

fn gradient_env_cubemap() -> Image {
    const FACE: u32 = 64;
    let sky = Color::srgb_u8(0xe7, 0xee, 0xf8).to_linear();
    let ground = Color::srgb_u8(0x5a, 0x6a, 0x44).to_linear();
    let horizon = Color::srgb_u8(0xc6, 0xcb, 0xc8).to_linear();

    let mut data: Vec<u8> = Vec::with_capacity((FACE * FACE * 6 * 8) as usize);
    for face in 0..6u32 {
        for y in 0..FACE {
            for x in 0..FACE {
                let u = (x as f32 + 0.5) / FACE as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / FACE as f32 * 2.0 - 1.0;
                let dir = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let h = dir.y;
                let lin = if h >= 0.0 {
                    let s = h.clamp(0.0, 1.0);
                    let s = s * s * (3.0 - 2.0 * s);
                    mix_linear(horizon, sky, s)
                } else {
                    let s = (-h).clamp(0.0, 1.0);
                    let s = s * s * (3.0 - 2.0 * s);
                    mix_linear(horizon, ground, s)
                };
                for c in [lin.red, lin.green, lin.blue, 1.0] {
                    data.extend_from_slice(&f32_to_f16_le(c));
                }
            }
        }
    }

    let mut image = Image::new(
        Extent3d { width: FACE, height: FACE, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    image
}

fn mix_linear(a: LinearRgba, b: LinearRgba, s: f32) -> LinearRgba {
    let s = s.clamp(0.0, 1.0);
    LinearRgba {
        red: a.red + (b.red - a.red) * s,
        green: a.green + (b.green - a.green) * s,
        blue: a.blue + (b.blue - a.blue) * s,
        alpha: 1.0,
    }
}

fn f32_to_f16_le(value: f32) -> [u8; 2] {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    let half: u16 = if exp <= 0 {
        sign
    } else if exp >= 0x1f {
        sign | 0x7c00
    } else {
        sign | ((exp as u16) << 10) | ((mantissa >> 13) as u16)
    };
    half.to_le_bytes()
}
