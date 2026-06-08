# Three.js Rendering Pipeline Spec for Bevy 0.18.1 Port

# Complete Rendering Pipeline Specification: Tileworld

## 1. Canvas / Renderer Settings

### WebGL Context (App.tsx:23-40)
```typescript
Canvas props:
  shadows: 'soft' (CAPTURE_MODE ? false)
  dpr: 1 (pinned in both modes)
  camera: { position: [36, 40, 40], fov: 32 }
  
gl: {
  antialias: false              // SMAA handles this in post
  powerPreference: 'high-performance'
  toneMapping: THREE.AgXToneMapping
  toneMappingExposure: 1.15
}

Fallback clear colour: '#cfd8e2'
```

### Quality Tiers (qualityStore.ts)
- `'low'`: no post-processing stack, no sun shadows (integrated GPU)
- `'medium'`: full post stack (GodRays) + sun shadows
- `'high'`: medium + heavy content extras (reflective water, dense foliage)

---

## 2. EffectComposer Stack (World.tsx:272-305)

### Configuration
```typescript
<EffectComposer multisampling={0} enableNormalPass={false}>
```

### Pass Order (sequential rendering):

#### 1. **GodRays** (Volumetric Sun Shafts)
- source: `sun={sunMesh}` (emissive sphere at sun position)
- blur: enabled
- samples: 36
- resolutionScale: 0.4
- density: 0.96
- decay: 0.92
- weight: 0.4
- exposure: 0.34
- clampMax: 1
- blendFunction: `BlendFunction.SCREEN`
- **Quality gate**: on for medium/high only

#### 2. **Bloom** (Selective Glow)
- mipmapBlur: true
- luminanceThreshold: 1.0
- luminanceSmoothing: 0.3
- intensity: 0.6
- kernelSize: `KernelSize.MEDIUM`

#### 3. **DepthOfField** (Background Blur, Auto-Focus on Player)
- bokehScale: `dofTunables.bokehScale` (live-tunable, ref-driven by DofDriver)
  - Default: 7
  - Range: 0..12
  - bokehScale=0 disables DoF
- height: 480
- **Auto-focus**: camera→player distance (world units)
- focusRange: `dofTunables.focusRange` (sharp band in world units)
  - Default: 70
  - Range: 2..120

#### 4. **HueSaturation** (Color Grade - Reactive)
- saturation: `gradeTunables.baseSaturation` (live-tunable, ref-driven by ReactiveGrade)
  - Default resting: 0.18
  - Range: -0.5 to 0.5
  - Driven down by low HP / on hit

#### 5. **BrightnessContrast** (Fixed Cinematic Grade)
- brightness: -0.02
- contrast: 0.12

#### 6. **Vignette** (Screen-Edge Darkness - Reactive)
- offset: 0.35
- darkness: `gradeTunables.baseDarkness` (live-tunable, ref-driven by ReactiveGrade)
  - Default resting: 0
  - Max at death: 0.97
  - Spikes on hit: +0.13
  - Driven by low-HP dread + "wince" pulse
- eskil: false (squared falloff)

#### 7. **SMAA** (Antialiasing)
- No params (default Subpixel Morphological AA)
- Replaces canvas MSAA (dpr scaling cost)

---

## 3. Fog

### Scene Fog (World.tsx:357)
```typescript
<fogExp2 attach="fog" args={['#d6c6a0', 0.02]} />
```
- Type: **exponential (fogExp2)**
- Default color: `#d6c6a0`
- Default density: 0.02 (leva-tunable, range 0..0.1)
- **Dynamic tinting**: color lerps toward biome on day (DayNight.tsx)

### Biome Fog Colors (DayNight.tsx:23-28)
Per-biome tints applied during day only (faded at night):
- snow: `#cdd8e8`
- desert: `#e7d29a`
- swamp: `#7c8a58`
- forest: `#9fb37a`
- rock: `#b6b4bc`
- **Mix amount**: 0.45 (45% toward biome tint)
- **Ease rate**: 2.2 (frame-to-frame smoothing at biome edges)

### Color Palette (timeStore.ts:151-161)
Day-to-night fog color sample:
- FOG_GOLD: `#f0b88a` (golden hour, near horizon)
- FOG_DAY: `#e4d8b8` (high sun)
- FOG_NIGHT: `#141b30` (midnight)

---

## 4. Lighting

### Ambient Light (DayNight.tsx:255)
```typescript
<ambientLight ref={ambRef} intensity={lights.ambient} />
```
- Default intensity: 0.13 (daytime scale, leva: 0..2)
- Color: `gradeTunables.ambientColor` (day/night interpolated)
  - AMB_DAY: `#fff4e0`
  - AMB_NIGHT: `#2c3a63`
- Scale factor: 0.18 + 0.82 × dayAmount (moonlit floor at night → full day)

### Hemisphere Light (DayNight.tsx:254)
```typescript
<hemisphereLight ref={hemiRef} args={['#e7eef8', '#5a6a44', lights.hemi]} />
```
- Default intensity: 0.24 (daytime scale, leva: 0..2)
- Sky color (day): `#e7eef8` → lerps to HEMI_SKY_NIGHT `#1c2740`
- Ground color (day): `#5a6a44` → lerps to HEMI_GND_NIGHT `#181f16`
- Scale factor: 0.22 + 0.78 × dayAmount

### Directional Light (SunShadow.tsx, follows player)
```typescript
<directionalLight
  position={[...]}
  target={target}          // Follows player
  intensity={lights.dir}
  color="#ffe6b3"
  castShadow
  shadow-mapSize-width={1024}
  shadow-mapSize-height={1024}
  shadow-camera-left={-38}
  shadow-camera-right={38}
  shadow-camera-top={38}
  shadow-camera-bottom={-38}
  shadow-camera-near={0.5}
  shadow-camera-far={260}
  shadow-bias={-0.0004}
  shadow-normalBias={0.035}
/>
```
- Default intensity (leva): 2.1 (range 0..4)
- Color (warm golden): `#ffe6b3` (noon) ↔ `#ff8a4d` (sunrise/sunset)
- **Position**: sun direction × 120 world units (SUN_DIST)
- **Direction**: animated from timeStore via `sunDirAt(t)` each frame
  - East→West sweep (X), height variation (Y)
  - South bias (Z): 0.55 (shadows fall southward)
- **Shadow frustum** (follows player, texel-snapped):
  - Size: ±38 world units (SHADOW_HALF)
  - Resolution: 1024×1024 (SHADOW_MAP_SIZE)
  - Auto-update: OFF (on-demand only)
  - Recenter threshold: 9 units (RECENTER_DIST)
  - Texel snap: yes (removes shimmer)
  - Refresh cadence:
    - While moving: every 6 frames (ANIM_REFRESH_INTERVAL)
    - Idle: every 24 frames (IDLE_REFRESH_INTERVAL)
- **Quality gate**: disabled when quality='low' (no shadows)

### Sun Direction Calculation (timeStore.ts:102-105)
```typescript
function sunDirAt(t: number, out: THREE.Vector3): THREE.Vector3 {
  const a = (t - 0.25) * Math.PI * 2
  return out.set(Math.cos(a), Math.sin(a), SOUTH_BIAS).normalize()
}
// t ∈ [0,1): fraction through 24h day
// 0 = midnight, 0.25 = sunrise, 0.5 = noon, 0.75 = sunset
```

### Image-Based Lighting (World.tsx:351-353)
```typescript
<Suspense fallback={null}>
  <Environment files="/hdri/sunset_1k.hdr" environmentIntensity={0.55} />
</Suspense>
```
- HDRI: sunset_1k.hdr (1K resolution)
- Intensity: 0.55
- **Purpose**: ambient + reflection only (background stays Sky dome)

---

## 5. Sky / Background

### Sky Dome (DayNight.tsx:209-217)
```typescript
<Sky
  ref={skyRef}
  distance={4000}
  sunPosition={startPos}    // Updated each frame
  turbidity={9}
  rayleigh={2.4}
  mieCoefficient={0.006}
  mieDirectionalG={0.82}
/>
```
- Atmospheric scattering dome
- Updates sunPosition from day/night cycle
- Covers fallback clear color once mounted

### Sun Glow Sphere (DayNight.tsx:220-223)
```typescript
<mesh ref={sunRef} position={startPos}>
  <sphereGeometry args={[46, 24, 24]} />
  <meshBasicMaterial color="#fff0cc" toneMapped={false} fog={false} />
</mesh>
```
- Size: radius 46
- Color: `#fff0cc` (warm, emissive)
- toneMapped: false (stays bright post-AgX)
- fog: false (visible on horizon)
- **Purpose**: Bloom + GodRays origin

### Moon (DayNight.tsx:226-236)
```typescript
<mesh ref={moonRef} visible={false}>
  <sphereGeometry args={[34, 20, 20]} />
  <meshBasicMaterial
    color="#cdd6e8"
    toneMapped={false}
    fog={false}
    transparent
    opacity={0}
  />
</mesh>
```
- Position: anti-sun (opposite direction)
- Fades in at night (nightAmount 0→1)

### Star Field (DayNight.tsx:238-251)
```typescript
<points ref={starsRef} geometry={starGeo} visible={false}>
  <pointsMaterial
    color="#cdd6e8"
    size={2}
    sizeAttenuation={false}
    transparent
    opacity={0}
    depthWrite={false}
    fog={false}
    toneMapped={false}
  />
</points>
```
- Count: 420 stars
- Deterministic placement (procedural hash from index)
- Radius: 600 units (upper hemisphere only)
- Fades in at night

### Distant Mountains (DistantMountains.tsx)
- Low-poly silhouettes ringing horizon
- World-space (no fog)
- Two materials: rock body + snowy caps
- Elliptical ring at ocean edge
- **No collision, no pathing**

---

## 6. Water

### Surface (Water.tsx:30-143)
```typescript
<mesh geometry={geo} material={mat} position={[0, 0.9, 0]} receiveShadow />
```
- Plane geometry: 56×48 subdivisions
- Position Y: 0.9 (just below terrain y=1.0)
- Material: MeshStandardMaterial + custom shader
- **Live-tunable (waterTunables)**:
  - color: `#3aa6e0` (vivid blue)
  - metalness: 0.05 (low; sky sheen + sun glint read as reflectivity instead)
  - roughness: 0.3
  - skyStrength: 0.45 (fresnel sky-sheen)
  - sunStrength: 1.6 (sun glint)

### Vertex Displacement (Custom Shader)
```glsl
transformed.y += sin(position.x * 0.55 + uTime * 0.9) * 0.05
                + cos(position.z * 0.7 + uTime * 1.1) * 0.05;
```
- Amplitude: ±0.05 world units
- Two sine/cosine waves (no mesh recomputation)
- Analytic normal computed from partial derivatives

### Water Floor (Water.tsx:148-159)
```typescript
<mesh position={[0, 0.65, 0]}> geometry: planeGeometry
  <meshStandardMaterial color="#0d3a66" roughness={1} />
</mesh>
```
- Darker underwater (no transparency revealed)
- Position: y=0.65 (under surface, above seabed)

### Water Dimensions
- Width: COLS + 280
- Height: ROWS + 280
- (COLS=144, ROWS=108 base map)

---

## 7. Reactive Grade (Screen Juice)

### Vignette + Hue-Saturation (ReactiveGrade driver, World.tsx:150-172)
Updated per-frame via refs (no re-render):

**Low HP Dread** (ratio < 0.35):
- darkness += `lowDarken` (0.16) × (1 - ratio)
- saturation -= `lowDesat` (0.25) × (1 - ratio)
- Heartbeat throb: sin(now × 5.5) × lowDarken amplitude (0.035)

**Hit Wince** (pulse charge, gradeStore.ts:40-52):
- darkness += `winceDarken` (0.13) × pulse
- saturation -= `winceDesat` (0.16) × pulse
- Pulse decay: 3.2 charge/sec

**Resting Baseline**:
- baseDarkness: 0 (no always-on vignette)
- baseSaturation: 0.18 (richer, compensates AgX desaturation)

---

## 8. Time of Day Constants

### Clock (timeStore.ts:18-45)
- DAY_LENGTH: 120 seconds (2 minutes real time = 1 full day)
- DAY_START_T: 0.30 (golden hour at start; yields sun direction ≈ old static)
- T_DAWN: 0.30
- T_DUSK: 0.70 (low golden west)
- NIGHT_T: 0.0 (midnight, during waves)

### Easing
- DAY_LERP_RATE: 0.7 (dusk/dawn ease, ~1%/frame)
- Phase-driven: day on menu/prep/victory, night during wave

---

## 9. Hex Color Reference

### Renderer & Sky
- Fallback clear: `#cfd8e2`
- Sun glow: `#fff0cc`
- Moon: `#cdd6e8`
- Stars: `#cdd6e8`

### Fog (timeStore.ts:151-161)
- FOG_GOLD: `#f0b88a` (golden hour)
- FOG_DAY: `#e4d8b8` (high sun)
- FOG_NIGHT: `#141b30` (midnight)
- Biome_snow: `#cdd8e8`
- Biome_desert: `#e7d29a`
- Biome_swamp: `#7c8a58`
- Biome_forest: `#9fb37a`
- Biome_rock: `#b6b4bc`

### Lights (timeStore.ts:151-161)
- SUN_LOW: `#ff8a4d` (sunrise/sunset)
- SUN_HIGH: `#ffe6b3` (noon)
- SUN_DIR_SHADOW: `#ffe6b3` (same as high)
- AMB_DAY: `#fff4e0`
- AMB_NIGHT: `#2c3a63`
- HEMI_SKY_DAY: `#e7eef8`
- HEMI_SKY_NIGHT: `#1c2740`
- HEMI_GND_DAY: `#5a6a44`
- HEMI_GND_NIGHT: `#181f16`

### Water
- Color: `#3aa6e0` (vivid blue)
- Sky reflection: `#bcd8f0` (lighter blue, in shader)
- Sun glint: `#fff0cc` (warm)
- Floor: `#0d3a66` (dark blue-black)

### Distant Mountains
- Body: `#8a93a4` (grey, roughness 1.0)
- Snow: `#e6ebf0` (pale, roughness 0.85)

---

## 10. Performance Flags

### Capture Mode (`?capture` or `?lite` URL param)
- Drops entire post stack
- Turns off sun shadows
- dpr pinned to 1
- Used for headless screenshots (npm run shot)

### Perf Mode (`?perf` URL param)
- Shows r3f-perf HUD + console logger
- Only in dev or explicit URL flag

---

## File References

| Component | File | Key Lines |
|-----------|------|-----------|
| Renderer settings | `src/App.tsx` | 23–40 |
| EffectComposer stack | `src/world/World.tsx` | 272–305 |
| Fog (dynamic) | `src/world/World.tsx` | 357 |
| Biome fog tints | `src/world/DayNight.tsx` | 23–34 |
| Lights (ambient, hemi, dir) | `src/world/DayNight.tsx` | 254–255 |
| Sun shadow | `src/world/SunShadow.tsx` | 1–208 |
| Sky dome, sun glow, moon | `src/world/DayNight.tsx` | 206–257 |
| Water | `src/world/Water.tsx` | 1–143 |
| Distant mountains | `src/world/DistantMountains.tsx` | 1–87 |
| Color/light samples | `src/world/timeStore.ts` | 101–186 |
| Grade (vignette, hue) | `src/world/gradeStore.ts` | 1–58 |
| Quality tiers | `src/world/qualityStore.ts` | 1–68 |
| Render mode flags | `src/world/renderMode.ts` | 1–26 |

## Exact Constants
**Renderer Constants**
- toneMapping: THREE.AgXToneMapping
- toneMappingExposure: 1.15
- antialias: false
- dpr: 1
- shadows: 'soft' (or false in capture)
- camera fov: 32
- camera position: [36, 40, 40]
- clear color: #cfd8e2

**EffectComposer Parameters**
- multisampling: 0
- enableNormalPass: false

**GodRays**
- samples: 36
- resolutionScale: 0.4
- density: 0.96
- decay: 0.92
- weight: 0.4
- exposure: 0.34
- clampMax: 1

**Bloom**
- luminanceThreshold: 1.0
- luminanceSmoothing: 0.3
- intensity: 0.6
- kernelSize: KernelSize.MEDIUM

**DepthOfField**
- bokehScale: 7 (default, 0–12 range)
- focusRange: 70 (default, 2–120 range)
- height: 480

**BrightnessContrast**
- brightness: -0.02
- contrast: 0.12

**Vignette**
- offset: 0.35
- darkness: 0 (default, reactive 0–0.97)
- eskil: false

**Fog**
- type: fogExp2
- color: #d6c6a0 (base)
- density: 0.02 (default, 0–0.1 range)

**Lighting**
- ambient.intensity: 0.13 (default, 0–2 range)
- hemi.intensity: 0.24 (default, 0–2 range)
- hemi.skyColor: #e7eef8 (day)
- hemi.groundColor: #5a6a44 (day)
- dir.intensity: 2.1 (default, 0–4 range)
- dir.color: #ffe6b3 (primary) / #ff8a4d (low)
- dirLight.castShadow: true (except quality='low')

**Shadow Settings**
- mapSize: 1024×1024
- camera.left/right/top/bottom: ±38
- camera.near: 0.5
- camera.far: 260
- bias: -0.0004
- normalBias: 0.035
- follows player, recenter dist: 9 units
- refresh intervals: 6 (moving), 24 (idle) frames

**Sun Direction**
- SUN_DIST: 120 world units
- SOUTH_BIAS: 0.55
- sunDirAt(t) = (cos(a), sin(a), SOUTH_BIAS).normalize() where a = (t - 0.25) × 2π

**Time**
- DAY_LENGTH: 120 seconds
- DAY_START_T: 0.30
- NIGHT_T: 0.0
- T_DAWN: 0.30
- T_DUSK: 0.70
- DAY_LERP_RATE: 0.7

**Sky Dome (Rayleigh Scattering)**
- distance: 4000
- turbidity: 9
- rayleigh: 2.4
- mieCoefficient: 0.006
- mieDirectionalG: 0.82

**Water**
- color: #3aa6e0
- metalness: 0.05
- roughness: 0.3
- skyStrength: 0.45
- sunStrength: 1.6
- position.y: 0.9
- dimensions: (COLS+280) × (ROWS+280)
- displacement: sin/cos waves, ±0.05 amplitude

**Reactive Grade**
- baseDarkness: 0 (default)
- baseSaturation: 0.18 (default)
- lowThreshold: 0.35
- lowDarken: 0.16
- lowDesat: 0.25
- heartbeat: 0.035
- winceDarken: 0.13
- winceDesat: 0.16
- PULSE_DECAY: 3.2 charge/sec

**Biome Fog Mix**
- BIOME_FOG_MIX: 0.45
- BIOME_FOG_EASE: 2.2

**HDRI**
- file: /hdri/sunset_1k.hdr
- environmentIntensity: 0.55

**Distant Mountains**
- ring ellipse: (COLS+280)/2, (ROWS+280)/2 radius
- body color: #8a93a4 (roughness 1.0)
- snow color: #e6ebf0 (roughness 0.85)
- fog: false on material

## Bevy 0.18 Notes
## Bevy 0.18.1 Mapping

### Renderer Setup
```rust
// App::new()
.insert_resource(Msaa::Off)  // No MSAA; use postprocessing SMAA
.insert_resource(DefaultImageSampler::nearest_repeat()) // If mipmaps needed
```

### Camera
```rust
// Use bevy::core_pipeline::bloom::BloomSettings + Tonemapping
Camera3d {
  target_usage: CameraTargetUsage::default(),
}
Transform {
  translation: Vec3::new(36.0, 40.0, 40.0),
  ..default()
}
Projection::Perspective(PerspectiveProjection {
  fov: 32.0_f32.to_radians(),
  ..default()
})
Tonemapping::AgX
ExposureSettings { ev100: 2.2 } // ~1.15 exposure
```

### Tonemapping + Exposure
- Use `Tonemapping::AgX` on camera
- Use `ExposureSettings` with ev100 adjusted for 1.15x exposure factor
- Keep color_grading off initially; apply vignette + saturation as post-process

### Core Lighting
```rust
// Ambient
AmbientLight {
  color: Color::srgb_u8(255, 244, 224), // AMB_DAY #fff4e0
  brightness: 0.13,
}

// Hemisphere (use DirectionalLight + custom adjustment)
// Bevy 0.18 has no HemisphereLight, so approximate:
// Option 1: Use DirectionalLight + AmbientLight + custom color adjustment
// Option 2: Create custom light component with similar effect

// Directional (Sun, player-following)
DirectionalLight {
  illuminance: 2.1,
  shadows_enabled: true,
  color: Color::srgb_u8(255, 230, 179), // #ffe6b3
  ..default()
}
CascadeshadowConfigBuilder {
  num_cascades: 1,
  maximum_distance: 260.0,
  ..default()
}.build()

// Transform follows player, frustum size ±38, resolution 1024x1024
```

### Fog
```rust
Fog {
  color: Color::srgb_u8(214, 198, 160), // #d6c6a0
  directional_light_color: Color::WHITE,
  directional_light_exponent: 2.0,
  falloff: FogFalloff::Exponential { density: 0.02 },
}

// Apply biome tinting in custom system each frame
// Lerp fog color toward biome tint by 0.45 × (1 - nightAmount)
```

### PostProcessing
**Use bevy_postprocessing or bevy_enhancement crate (or equivalent fork for 0.18)**:

- **Bloom**: 
  - threshold: 1.0
  - softness: 0.3
  - intensity: 0.6
  - Use MipmapBlur variant
  
- **DepthOfField**: 
  - Auto-focus on player (calculate distance in system)
  - focusRange: 70 world units
  - bokehScale: 7 (or bernsteinBlurRadius: 7)
  - height: 480 (or match render target)
  
- **GodRays** (Volumetric Lighting):
  - source: sun mesh position
  - samples: 36
  - resolution: 0.4x
  - density: 0.96
  - decay: 0.92
  - weight: 0.4
  - exposure: 0.34
  - screen-space blend
  
- **Vignette**:
  - offset: 0.35
  - darkness: reactive (0–0.97)
  - Use squared falloff, not softness param
  
- **ColorGrading** (Hue-Saturation + Brightness-Contrast):
  - saturation: reactive (base 0.18, range -0.5 to 0.5)
  - brightness: -0.02
  - contrast: 0.12
  
- **SMAA**: No parameters (subpixel morphological AA)

### Sky + Atmosphere
```rust
// Sky dome approximation
Mesh: sphere mesh, large radius (4000 distance)
Material: Custom shader (atmospheric scattering)
  - turbidity: 9.0
  - rayleigh: 2.4
  - mieCoefficient: 0.006
  - mieDirectionalG: 0.82
  - sunPosition: animated from timeStore

// Sun glow (emissive source for bloom/godrays)
Mesh: sphere, radius 46
Material: EmissiveMaterial
  color: Color::srgb_u8(255, 240, 204), // #fff0cc
  emissive: Color::srgb_u8(255, 240, 204)
  toneMapped: false
  fog: false
  
// Stars: points material
// Moon: sphere + opacity animation
```

### Water Material
```rust
StandardMaterial {
  base_color: Color::srgb_u8(58, 166, 224), // #3aa6e0
  metallic: 0.05,
  perceptual_roughness: 0.3,
  ..default()
}

// Vertex displacement in custom shader
// sin(x*0.55 + t*0.9)*0.05 + cos(z*0.7 + t*1.1)*0.05
// Analytic normal from partial derivatives

// Water floor: dark plane beneath (no transparency reveal)
Color::srgb_u8(13, 58, 102) // #0d3a66
```

### Reactive Grade System
```rust
// Vignette + Saturation update each frame via system
pub fn update_reactive_grade(
  mut query: Query<(&mut VignetteEffect, &mut ColorGrading)>,
  player: Res<Player>,
  grade_state: Res<GradeState>,
) {
  let low = (grade_state.low_threshold - player.hp_ratio).max(0.0) / grade_state.low_threshold;
  let beat = if low > 0.0 {
    (now_secs * 5.5).sin() * 0.5 + 0.5 * low * grade_state.heartbeat
  } else { 0.0 };
  
  vignette.darkness = (0.97_f32)
    .min(grade_state.base_darkness + low * grade_state.low_darken + pulse * grade_state.wince_darken + beat);
  
  color_grading.saturation = (-0.8_f32)
    .max(grade_state.base_saturation - low * grade_state.low_desat - pulse * grade_state.wince_desat);
}
```

### Day/Night Cycle
```rust
// Sun direction animation
pub fn sun_dir_at(t: f32, south_bias: f32) -> Vec3 {
  let a = (t - 0.25) * std::f32::consts::TAU;
  Vec3::new(a.cos(), a.sin(), south_bias).normalize()
}

// Lighting sample system
// Interpolate all colors based on sun elevation (e = dir.y)
// Apply to ambient, directional, hemisphere colors each frame
```

### Quality Tiers
```rust
#[derive(Resource, Clone, Copy)]
pub enum Quality { Low, Medium, High }

// Low: no post, no shadows
// Medium: full post + shadows
// High: medium + dense content

pub fn update_quality(
  quality: Res<Quality>,
  mut postprocessing: ResMut<PostprocessingSettings>,
  mut shadow_enabled: Query<&mut CastShadow>,
) {
  match *quality {
    Quality::Low => {
      postprocessing.enabled = false;
      shadow_enabled.iter_mut().for_each(|mut s| s.0 = false);
    }
    Quality::Medium | Quality::High => {
      postprocessing.enabled = true;
      shadow_enabled.iter_mut().for_each(|mut s| s.0 = true);
    }
  }
}
```

### HDRI Lighting
```rust
// Load and apply environment map
EnvironmentMapLight {
  brightness: 0.55,
}
// Load sunset_1k.hdr, apply to scene
```

### Camera Follow (Shadow)
```rust
pub fn update_shadow_camera(
  player: Res<Player>,
  mut light_query: Query<&mut Transform, With<DirectionalLight>>,
  mut shadow_settings: ResMut<ShadowSettings>,
) {
  // Recenter shadow frustum to player with texel snapping
  let texel_size = (SHADOW_HALF * 2.0) / SHADOW_MAP_SIZE as f32;
  let snapped_pos = Vec3::new(
    (player.x / texel_size).round() * texel_size,
    0.0,
    (player.z / texel_size).round() * texel_size,
  );
  // Update light transform & frustum
}
```

### Alternative Postprocessing Crates for Bevy 0.18
- `bevy_contrib_postprocessing` (if available)
- `bevy_shader_utils` + custom passes
- Roll custom post-process with `extract_component`, `RenderGraph`, `BindGroup`
- If using newer `bevy_postprocessing`, check compatibility with 0.18

### Key Compatibility Notes
1. **No HemisphereLight**: Approximate with directional + ambient + color modulation
2. **Cascaded shadows**: Bevy 0.18 uses cascaded shadow maps; adjust to match 1024×1024 frustum-follow behavior
3. **SMAA**: May need a custom integration or crate (bevy_antialiasing if exists)
4. **GodRays**: Likely custom shader; use compute shader or render texture intermediate
5. **Depth-of-field**: Use custom post-process; bevy core doesn't ship with built-in DoF
6. **AgX tonemapping**: Built-in to Bevy 0.18; use `Tonemapping::AgX`
7. **Exposure**: Use `ExposureSettings` resource, not toneMappingExposure prop
8. **Reactive vignette**: Create custom post-process pass with animatable darkness uniform