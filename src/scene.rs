//! Camera + lighting + post-processing — the polished daytime look. Ports the TS
//! pipeline (AgX, bloom, DoF background blur, fog, warm sun, soft ambient) onto the
//! verified Bevy 0.18 components, plus a procedural gradient-cubemap IBL and SSAO
//! (both adapted from the working tileworld-bevy port's `lighting.rs`).

use bevy::anti_alias::smaa::{Smaa, SmaaPreset};
use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, ShadowFilteringMethod};
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

use crate::siege::{GamePhase, Siege};

/// Sky / fog horizon colour — bright pale daytime blue.
const SKY: Color = Color::srgb(0.70, 0.82, 0.93);
const FOG_DENSITY: f32 = 0.009;
const IBL_INTENSITY: f32 = 620.0;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SKY))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.88, 0.93, 1.0),
                brightness: 85.0,
                affects_lightmapped_meshes: true,
            })
            .insert_resource(DirectionalLightShadowMap { size: 4096 })
            .insert_resource(SkyClock {
                t: start_t(),
                // Freeze the clock for screenshots so frame 90 is deterministic.
                paused: std::env::var("FOREST_SHOT").is_ok(),
                day_secs: day_seconds(),
            })
            .add_systems(Startup, (setup_camera, setup_sun))
            .add_systems(Update, advance_sky);
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
const T_DAWN: f32 = 0.08; // low morning sun (east) at prep start
const T_DUSK: f32 = 0.42; // low evening sun (west) as the prep timer ends (still above horizon)
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

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn advance_sky(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    siege: Option<Res<Siege>>,
    mut clock: ResMut<SkyClock>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun_q: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut fog_q: Query<&mut DistanceFog>,
    mut env_q: Query<&mut GeneratedEnvironmentMapLight>,
    mut grade_q: Query<&mut ColorGrading>,
) {
    let dt = time.delta_secs();
    if keys.just_pressed(KeyCode::KeyP) {
        clock.paused = !clock.paused;
    }
    if !clock.paused {
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

    // Sun direction (from origin toward the sun): +X east, +Y up, −X west, with a small
    // constant +Z tilt so shadows are angled (never axis-perfect even at noon).
    let a = clock.t * std::f32::consts::TAU;
    let sun_dir = Vec3::new(a.cos(), a.sin(), 0.35).normalize();
    let elev = sun_dir.y; // −1 (midnight) .. 1 (noon)

    let day = smoothstep(-0.02, 0.22, elev); // 0 deep night → 1 full day
    let high = smoothstep(0.0, 0.45, elev); // 0 at horizon → 1 overhead
    let horizon = day * (1.0 - high); // peaks at sunrise/sunset
    // Eases in as the sun dips toward/below the horizon — keeps the sunrise/sunset glow
    // bright, then ramps the world into a dark moonlit night.
    let night = 1.0 - smoothstep(-0.22, 0.08, elev);

    for (mut light, mut tf) in &mut sun_q {
        *tf = Transform::from_translation(sun_dir * 120.0).looking_at(Vec3::ZERO, Vec3::Y);
        // A modest moonlight floor (≈300 lux): enough for soft directional moonlight, but
        // low enough that the procedural Atmosphere sky stays dark/moody after dark. Ground
        // visibility comes from ambient + IBL below. Daytime peak (≈11 050) is unchanged.
        light.illuminance = 300.0 + 10_750.0 * day;
        // Warm at the horizon → neutral-warm overhead, then cooled toward moonlit blue as
        // the sun drops below the horizon (so the "moon" doesn't cast an orange glow).
        let warm = lerp_col(Color::srgb(1.0, 0.45, 0.22), Color::srgb(1.0, 0.95, 0.85), high);
        light.color = lerp_col(warm, Color::srgb(0.55, 0.66, 1.0), night * 0.8);
    }

    // Ambient: brightness rides the sun; tint cools to moonlit blue after dark. The night
    // floor is lifted high (≈110) since it — not the dimmed moonlight — is what lights the
    // ground after dark (and ambient doesn't feed the Atmosphere sky, so it brightens the
    // ground without re-brightening the moody sky). It carries the deeper night exposure cut
    // below. (Computed from `day`, never read-back, so it can't compound frame-to-frame.)
    ambient.brightness = 110.0 + 54.0 * day;
    ambient.color = lerp_col(Color::srgb(0.50, 0.60, 0.95), Color::srgb(0.90, 0.93, 1.0), day);

    // IBL (baked daytime) dimmed at night, but kept a strong floor (≈160) so surfaces still
    // catch skylight after dark — the other half of the after-dark ground light.
    for mut env in &mut env_q {
        env.intensity = 160.0 + (IBL_INTENSITY - 160.0) * day;
    }

    // Darken night at the GRADE stage. Camera `Exposure` only scales PBR lighting, but
    // after dark the scene is lit almost entirely by the Atmosphere sky (which bypasses
    // Exposure) — so a final-image stops cut here is what actually makes night read as a
    // dark, blue moonlit night instead of AgX dusk. Depth tunable via FOREST_NIGHT.
    let night_stops = std::env::var("FOREST_NIGHT")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .unwrap_or(2.8);
    for mut g in &mut grade_q {
        g.global.exposure = -night * night_stops;
    }

    // Fog: night navy → day sky-blue, warmed orange at sunrise/sunset. The night navy is
    // lifted off near-black so the world isn't swallowed by black fog after dark.
    let mut fog_col = lerp_col(Color::srgb(0.06, 0.08, 0.15), SKY, day);
    fog_col = lerp_col(fog_col, Color::srgb(1.0, 0.5, 0.3), horizon * 0.6);
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

    // Richer grade: saturation + a touch of contrast (the AgX/flat look needs both).
    let mut grading = ColorGrading::default();
    grading.global.post_saturation = 1.30;
    grading.shadows.contrast = 0.96; // lift crushed shadows (was 1.10 — too dark)
    grading.midtones.contrast = 1.14;
    grading.highlights.contrast = 1.08;

    commands.spawn((
        Camera3d::default(),
        Projection::from(PerspectiveProjection { fov: 50f32.to_radians(), ..default() }),
        cam_tf,
        Hdr,
        Exposure { ev100: 10.1 },
        Tonemapping::AgX,
        // SSAO + SMAA path (mutually exclusive with MSAA). Bevy's built-in DepthOfField is
        // gone — it silently no-op'd next to SSAO and only did a single focal plane. Depth
        // blur is now our own `depth_blur` post pass (below), which only READS the prepass
        // depth, so SSAO + NormalPrepass are safe to keep.
        Msaa::Off,
        Smaa { preset: SmaaPreset::High },
        ScreenSpaceAmbientOcclusion {
            quality_level: ScreenSpaceAmbientOcclusionQualityLevel::High,
            ..default()
        },
        DepthPrepass,
        NormalPrepass,
        Bloom { intensity: 0.16, ..Bloom::NATURAL },
        // Custom distance depth-blur: sharp within `clear` tiles, ramps to a soft far
        // field. Tunable live via FOREST_BLUR="clear,full,radius". Runs after tonemapping.
        crate::depth_blur::settings_from_env(),
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
        ShadowFilteringMethod::Gaussian,
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

fn setup_sun(mut commands: Commands) {
    commands.spawn((
        Sun,
        DirectionalLight {
            color: Color::srgb(1.0, 0.93, 0.78), // warm #ffe6b3-ish
            illuminance: 10_500.0,
            shadows_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            maximum_distance: 110.0,
            first_cascade_far_bound: 10.0,
            ..default()
        }
        .build(),
        // High, slightly-side sun → bright blue daytime sky + soft directional shadows.
        Transform::from_xyz(16.0, 40.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
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
