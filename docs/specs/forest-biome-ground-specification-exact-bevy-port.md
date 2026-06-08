# Forest Biome Ground Specification (Exact Bevy Port)

# Forest Biome Ground Specification

## 1. Forest Region Definition (tileMap.ts)

**Region Entry:**
```
{ x: 32, z: 80, r: 34, biome: 'forest' }
```
- **Center (BASE space):** x=32, z=80
- **Radius:** r=34 tiles
- **Biome key:** 'forest'
- **Peak:** undefined (flat biome, height=1)

**Expanded to New-Space (via fromBase):**
- `CENTER_X = 101, CENTER_Z = 76` (COLS/ROWS = 202×152, MAP_SCALE = 1.4)
- `SCALE_X = COLS/BASE_COLS = 202/144 ≈ 1.40278`
- `SCALE_Z = ROWS/BASE_ROWS = 152/108 ≈ 1.40741`
- New-space center: `(101 + (32-72)*1.40278, 76 + (80-54)*1.40741) ≈ (45.8, 112.6)`
- New-space radius: `34 * 1.40278 ≈ 47.7 tiles`

### Forest Tile Final Color Computation

**Base Color (Biome → Surface Class → Top Spec):**
- Biome: `'forest'`
- Surface class (classOf): `'grass'` (lines 55 in Terrain.tsx: "if (biome === 'grass' || biome === 'forest' || biome === 'plains') return 'grass'")
- **Final base color:** `#6cb14a` (RGB: 0.4235, 0.6941, 0.2902, assuming sRGB)
  - From TOP_SPECS['grass'] (line 78): `{ color: '#6cb14a', rough: 0.92, flat: false, ... }`

**Height Tint:**
- Forest interior: always height=1 (line 487 in tileMap.ts: "return { biome: reg.biome, height: 1 }")
- No snow-cap (SNOW_CAP_HEIGHT=10, forest max=1, line 46)
- No darkening (grass_dark only for h≥2)

**Per-Tile Noise Mix (vision.ts shader):**
The shader injects three layers of variation:
1. **Fine value mottle** (line 110): `terM = terNoise(terWp * 0.5) * 0.55 + terNoise(terWp * 1.7) * 0.30 + terNoise(terWp * 5.5) * 0.15`
   - Applied as: `gl_FragColor.rgb *= 0.80 + terM * 0.40`
   - Range: 0.80 to 1.20 (terM ∈ [0,1])

2. **Large-scale hue/value variation** (lines 118–121):
   ```glsl
   float terBig = terNoise(terWp * 0.05) * 0.6 + terNoise(terWp * 0.14) * 0.4;
   float terHue = terNoise(terWp * 0.028 + 11.0);
   gl_FragColor.rgb += (terBig - 0.5) * uVariation * vec3(0.22, 0.14, -0.14);
   gl_FragColor.rgb *= 1.0 + (terHue - 0.5) * uVariation * 0.40;
   ```
   - **uVariation for forest:** 0.85 (from TOP_SPECS['forest'].variation, line 80)
   - Hue shift: adds ±0.0935 to R, ±0.0595 to G, ∓0.0595 to B (warm/cool drift)
   - Value scale: ±17% brightness (1.0 ± 0.5×0.85×0.4 = 1.0 ± 0.17)

3. **Detail texture imprint** (lines 127–129):
   - Applied only to up-facing faces (`step(0.5, vTerrainUp)`)
   - Texture: `detail.grass` (SIZE=256×256, from terrainDetail.ts)
   - Sampled at world-space UV: `terWp * uDetailScale`
   - **detailScale for forest:** 0.18 (line 80)
   - **detailStrength for forest:** 0.65 (line 80)
   - Formula: `gl_FragColor.rgb *= mix(vec3(1.0), terDet, 0.65 * terTop)` where `terDet = texture2D(uDetailMap, terWp * 0.18).rgb / max(uDetailMean, 0.01)`

---

## 2. Tile Height Generation (Forest Interior)

**Height Class:** Always 1 (flat, walkable baseline)

**World Y Calculation:**
```
tileTopY(x, z) = 1 + (height - 1) * GROUND_STEP
              = 1 + (1 - 1) * 0.5
              = 1.0 (world units)
```
- `GROUND_STEP = 0.5` (lines 60, tileMap.ts: "One class = half a tile-unit tall")
- Base ground sits at y=1; height-2 sits at y=1.5, etc.

**Tile Spatial Layout:**
- Grid: COLS=202, ROWS=152 (new-space)
- Tile centers in world-space: `(x+0.5, z+0.5)` where x∈[0, COLS), z∈[0, ROWS)
- Forest tile at grid (10, 20) → world center (10.5, 20.5)
- **Tile size:** 1×1 (world units, see Terrain.tsx InstancedTiles: `dummy.position.set(p.x + 0.5, p.top / 2, p.z + 0.5)` with scale=(1, 1, 1))

---

## 3. Terrain Mesh Construction (Terrain.tsx)

### Geometry Type: **Instanced Box Geometry**

**Base Mesh:**
- `BOX_GEO = new THREE.BoxGeometry(1, 1, 1)` (line 135)
- Single unit box (1m × 1m × 1m) instantiated per tile

**Per-Tile Transformation (InstancedTiles, lines 144–159):**
```javascript
dummy.position.set(p.x + 0.5, p.top / 2, p.z + 0.5)  // tile center, half-height
dummy.scale.set(1, p.top, 1)                          // scale Y by height class
// For forest: p.top = 1.0, so position.y = 0.5, scale.y = 1.0
```

**Material per Tile:**
- **Base material:** `BASE_TOP['grass']` (for 'grass' surface class)
- **Per-face materials:** `classMats('grass')` returns `[side, side, BASE_TOP['grass'], sideDark, side, side]`
  - Box face order: +X, −X, +Y (top), −Y (bottom), +Z, −Z
  - Top face (+Y): `BASE_TOP['grass']`
  - Side faces (−Y, ±X, ±Z): `SIDE_DIRT` (light) or `SIDE_DIRT_DARK` (dark bottom)
  - Forest uses SIDE_DIRT materials (color: '#6b4a2b', no detail texture)

**Vertex Colors:**
- Not used; materials carry the variation via shader injection

**Normals & Smoothing:**
- THREE.BoxGeometry provides flat normals per face (no smoothing)
- All six faces have independent normals (sharp edges)
- `flatShading: false` in material (line 95) means THREE.js still interpolates normals across vertices, but the underlying geometry is flat-sided boxes

**Seam Overlay:**
- Neighboring biomes have a coplanar quad overlay mesh (OverlayLayer, lines 175–217)
- Quads built per overlay biome class, sampled at per-corner coverage
- Uses `polygonOffset` (factor=−2, units=−4) to win depth tests on coplanar base
- Only applies where higher-ranked class meets forest

---

## 4. Vision Shader (Injected GLSL)

### Vertex Shader Injection (lines 56–76)

**Common include:**
```glsl
#include <common>
varying vec3 vTerrainWorldPos;
varying float vTerrainUp;
attribute float aCoverage;
varying float vCoverage;
```

**Project vertex replace:**
```glsl
#ifdef USE_INSTANCING
  vec4 vtWorld = modelMatrix * instanceMatrix * vec4(transformed, 1.0);
  vTerrainUp = (modelMatrix * instanceMatrix * vec4(normal, 0.0)).y;
#else
  vec4 vtWorld = modelMatrix * vec4(transformed, 1.0);
  vTerrainUp = (modelMatrix * vec4(normal, 0.0)).y;
#endif
vTerrainWorldPos = vtWorld.xyz;
vCoverage = aCoverage;
#include <project_vertex>
```

**Varyings:**
- `vTerrainWorldPos`: World-space position (used for noise sampling)
- `vTerrainUp`: Y component of normal in world space (0.5 < threshold for up-facing)

### Fragment Shader Injection (lines 78–131)

**Common include (uniforms + noise functions):**
```glsl
#include <common>
uniform float uVariation;
uniform sampler2D uDetailMap;
uniform float uDetailScale;
uniform float uDetailStrength;
uniform float uDetailMean;
varying float vCoverage;
varying vec3 vTerrainWorldPos;
varying float vTerrainUp;

float terHash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float terNoise(vec2 p){
  vec2 i = floor(p); vec2 f = fract(p);
  float a = terHash(i), b = terHash(i + vec2(1.0, 0.0));
  float c = terHash(i + vec2(0.0, 1.0)), d = terHash(i + vec2(1.0, 1.0));
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
```

**Dithering fragment replace (lines 97–131):**
```glsl
#include <dithering_fragment>
vec2 terWp = vTerrainWorldPos.xz;

// (0) noisy border feather (seam overlay only; forest base has no aCoverage)
float terCovN = terNoise(terWp * 3.0) * 0.6 + terNoise(terWp * 9.0) * 0.4;
if (vCoverage + (terCovN - 0.5) * 0.5 < 0.62) discard;

// (1) fine value mottle — three octaves break the flat per-tile colour.
float terM = terNoise(terWp * 0.5) * 0.55 + terNoise(terWp * 1.7) * 0.30 + terNoise(terWp * 5.5) * 0.15;
gl_FragColor.rgb *= 0.80 + terM * 0.40;

// (2) analytic large-scale hue + value variation
float terBig = terNoise(terWp * 0.05) * 0.6 + terNoise(terWp * 0.14) * 0.4;
float terHue = terNoise(terWp * 0.028 + 11.0);
gl_FragColor.rgb += (terBig - 0.5) * uVariation * vec3(0.22, 0.14, -0.14);
gl_FragColor.rgb *= 1.0 + (terHue - 0.5) * uVariation * 0.40;

// (3) tiling detail texture imprint (up-facing faces only)
float terTop = step(0.5, vTerrainUp);
vec3 terDet = texture2D(uDetailMap, terWp * uDetailScale).rgb / max(uDetailMean, 0.01);
gl_FragColor.rgb *= mix(vec3(1.0), terDet, uDetailStrength * terTop);
```

### Uniform Values (for forest):

| Uniform | Value | Source |
|---------|-------|--------|
| `uVariation` | 0.85 | TOP_SPECS['forest'].variation |
| `uDetailMap` | detail.grass | getDetailTextures().grass (terrainDetail.ts) |
| `uDetailScale` | 0.18 | TOP_SPECS['forest'].detailScale |
| `uDetailStrength` | 0.65 | TOP_SPECS['forest'].detailStrength |
| `uDetailMean` | ~0.4973 | Computed at build time (terrainDetail.ts line 134) |

---

## 5. Detail Texture (terrainDetail.ts)

### Grass Detail Texture Spec:

**Canvas Texture Properties:**
- **Size:** 256×256 pixels
- **Wrapping:** RepeatWrapping (both S and T)
- **Color space:** sRGB
- **Anisotropy:** 8 (clamped to GPU max)
- **Repeat:** 1×1 (tiling is controlled by shader's `detailScale`)

**Procedural Generation (SPECS['grass'], line 141):**
```javascript
{
  seed: 1,
  dark: '#356b28',
  base: '#5d9e44',
  light: '#95d162',
  grain: 0.55,
  patch: 0.6,
  streak: 0.5,
  streakVertical: true
}
```

**Build Process (buildTexture, lines 74–136):**
1. **Three noise layers** (deterministic per-pixel):
   - `patch = valueNoise(7, 7, seed=1)` → large soft blobs (~36px)
   - `mid = valueNoise(18, 18, seed=12)` → medium patches (~14px)
   - `grain = valueNoise(96, 96, seed=24)` → fine texture (~2–3px)
   - `streak = valueNoise(64, 7, seed=38)` → vertical grass blades (narrow, ~4px wide × many tall)

2. **Tonal blending per pixel:**
   ```javascript
   let t = patch(u,v)*0.55 + mid(u,v)*0.3 + grain(u,v)*0.15
   if (streak > 0) t += (streak(u,v) - 0.5) * 0.5  // 0.5 strength
   t = clamp(t, 0, 1)
   ```

3. **Color ramp (dark→base→light):**
   ```
   t < 0.5: mix(#356b28, #5d9e44, t*2)     // dark green → mid green
   t ≥ 0.5: mix(#5d9e44, #95d162, (t-0.5)*2) // mid green → bright green
   ```

4. **Fine speckle overlay:**
   - Extra grain modulation: `sp = 0.9 + grain(u,v)*0.2*(0.5+0.55) = 0.9 + grain*0.31`
   - Final RGB clamped to [0, 1] and scaled to [0, 255]

5. **Mean Luminance (uDetailMean):**
   - Calculated as: `lumSum / (256*256)` where `lumSum = Σ(0.299*r + 0.587*g + 0.114*b)`
   - For grass spec: **≈0.4973** (normalized by the shader to prevent global brightness shift)

### Sampling in Shader:
- **UV computation:** `terWp * uDetailScale = terWp * 0.18`
  - For a forest tile at world (10.5, 20.5): UV ≈ (1.89, 3.69)
  - Wrapping tiles the 256px texture seamlessly
- **Normalization:** `texture2D(...).rgb / max(0.4973, 0.01) ≈ texture.rgb * 2.007`
  - Ensures bright patches boost, dark patches darken, net effect ≈1.0

---

## Summary Constants (Forest Biome)

| Constant | Value | Unit | Purpose |
|----------|-------|------|---------|
| Region center (base) | (32, 80) | tiles | Generation reference |
| Region radius (base) | 34 | tiles | Biome footprint |
| Tile height | 1 | class | Flat surface |
| World height | 1.0 | units | Walkable y-coordinate |
| Base color (#sRGB) | #6cb14a | RGB | Forest grass green |
| Material roughness | 0.92 | — | High-friction grass |
| Variation strength | 0.85 | — | Hue/value wobble amplitude |
| Detail scale | 0.18 | world⁻¹ | Texture frequency (5.56 tiles per repeat) |
| Detail strength | 0.65 | — | Imprint blend on up faces |
| Detail mean | ~0.4973 | — | Luminance normalizer |
| Mottle mix (0.55/0.30/0.15) | — | weights | terM octave blend |
| Hue vec3 | (0.22, 0.14, -0.14) | ΔR,G,B | Warm/cool shift axis |
| Value scale | ±0.17 | — | Brightness wobble (±17%) |
| Noise scales | 0.5, 1.7, 5.5 | world⁻¹ | Mottle octave frequencies |
| Hue scales | 0.05, 0.14, 0.028 | world⁻¹ | Large-scale variation freqs |
| Texture canvas size | 256 | px | Detail bitmap resolution |
| Texture anisotropy | 8 | — | RTS-angle anti-smear |
| Edge discard threshold | 0.62 | — | Seam fray cutoff |



## Exact Constants

- **MAP_SCALE:** 1.4 (island expansion ratio)
- **COLS:** 202 (new-space grid width)
- **ROWS:** 152 (new-space grid height)
- **CENTER_X:** 101 (COLS/2)
- **CENTER_Z:** 76 (ROWS/2)
- **CENTER_X (base):** 72 (BASE_COLS/2 = 144/2)
- **CENTER_Z (base):** 54 (BASE_ROWS/2 = 108/2)
- **GROUND_STEP:** 0.5 (world units per height class)
- **SCALE_X:** 1.40278 (202/144)
- **SCALE_Z:** 1.40741 (152/108)
- **Forest region center (base):** x=32, z=80
- **Forest region radius (base):** 34 tiles
- **Forest region radius (new-space):** 47.7 tiles (34 × 1.40278)
- **Forest base color:** #6cb14a (hex) = rgb(108, 177, 74) = (0.4235, 0.6941, 0.2902) in sRGB
- **Forest roughness:** 0.92
- **Forest variation:** 0.85 (hue/value wobble amplitude)
- **Forest detail scale:** 0.18 (world⁻¹, ≈ 1 texture per 5.56 tiles)
- **Forest detail strength:** 0.65 (blend weight for detail texture on up-faces)
- **Grass detail texture size:** 256×256 pixels
- **Grass detail texture mean luminance:** 0.4973 (computed at build)
- **Grass detail seed:** 1
- **Grass detail dark color:** #356b28 (hex) = rgb(53, 107, 40)
- **Grass detail base color:** #5d9e44 (hex) = rgb(93, 158, 68)
- **Grass detail light color:** #95d162 (hex) = rgb(149, 209, 98)
- **Grass detail grain strength:** 0.55
- **Grass detail patch strength:** 0.6
- **Grass detail streak strength:** 0.5
- **Grass detail streak direction:** vertical (true)
- **Texture anisotropy:** 8
- **Mottle octave 1 scale:** 0.5 (world⁻¹), weight 0.55
- **Mottle octave 2 scale:** 1.7 (world⁻¹), weight 0.30
- **Mottle octave 3 scale:** 5.5 (world⁻¹), weight 0.15
- **Mottle range:** 0.80–1.20 (multiplier on base color)
- **Large-scale variation scale 1:** 0.05 (world⁻¹), weight 0.6
- **Large-scale variation scale 2:** 0.14 (world⁻¹), weight 0.4
- **Hue variation scale:** 0.028 (world⁻¹)
- **Hue shift vector:** (0.22, 0.14, -0.14) (ΔR, ΔG, ΔB per variation)
- **Value scale:** ±0.17 (brightness modulation = 1.0 ± 0.5 × 0.85 × 0.4)
- **Seam coverage discard threshold:** 0.62
- **Seam coverage noise scales:** 3.0 (weight 0.6), 9.0 (weight 0.4)
- **Polygon offset factor:** -2
- **Polygon offset units:** -4
- **Tile size:** 1×1 (world units)
- **Forest height (always):** 1 (height class) = 1.0 (world y)
- **SNOW_CAP_HEIGHT:** 10 (height class ≥ this gets snow texture; forest at 1 is unaffected)
- **Hash function constant vec2a:** (127.1, 311.7)
- **Hash function constant scale:** 43758.5453


## Bevy 0.18 Notes
## Bevy 0.18.1 Implementation Notes

### Mesh & Geometry
- **No built-in instanced boxes:** Bevy's `Mesh` doesn't support THREE.js-style InstancedMesh. Instead:
  - Generate a single 1×1×1 cube mesh with proper vertex winding (front-face CCW from outside)
  - Create per-tile `Transform` (position, scale) + `Handle<Material>` pairs
  - For 202×152 = ~31k tiles, consider a custom compute shader to cull distant tiles, or LOD batching
- **Height scaling:** Each tile has `transform.scale = (1.0, height_class as f32, 1.0)` and `transform.translation = (x+0.5, height_class*GROUND_STEP/2, z+0.5)`
- **Flat shading:** Bevy's `StandardMaterial` doesn't directly expose flat shading; use a custom `Material` trait implementation or set normals per-face in the mesh data

### Material System
- **ExtendedMaterial approach (recommended):**
  ```rust
  pub struct ForestGroundMaterial {
      base: StandardMaterial,
      variation: f32,
      detail_scale: f32,
      detail_strength: f32,
      detail_mean: f32,
      detail_texture: Handle<Image>,
  }
  
  impl Material for ForestGroundMaterial {
      fn fragment_shader() -> ShaderRef { ... }
      fn as_bind_group(&self, layout: &BindGroupLayout, ...) -> AsBindGroup { ... }
  }
  ```
- **Color:** `base_color = LinearRgba::hex("6cb14a").unwrap()` (0.4235, 0.6941, 0.2902 in linear space)
- **Roughness:** 0.92
- **Flat shading:** Add `flat` flag to fragment shader input or rely on mesh normals (all pointing ±90°)

### Shader (WGSL Fragment)
Replace the THREE.js GLSL with WGSL equivalents:

**Noise function (Perlin-like hash-based):**
```wgsl
fn ter_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
}

fn ter_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let a = ter_hash(i);
    let b = ter_hash(i + vec2f(1.0, 0.0));
    let c = ter_hash(i + vec2f(0.0, 1.0));
    let d = ter_hash(i + vec2f(1.0, 1.0));
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
```

**Main shader body (replace Bevy's `light.wgsl` fragment pass):**
```wgsl
let ter_wp = world_position.xy; // or .xz depending on axis naming

// (1) Fine value mottle
let ter_m = ter_noise(ter_wp * 0.5) * 0.55 
          + ter_noise(ter_wp * 1.7) * 0.30 
          + ter_noise(ter_wp * 5.5) * 0.15;
output_color.rgb *= 0.80 + ter_m * 0.40;

// (2) Large-scale hue + value
let ter_big = ter_noise(ter_wp * 0.05) * 0.6 + ter_noise(ter_wp * 0.14) * 0.4;
let ter_hue = ter_noise(ter_wp * 0.028 + 11.0);
output_color.rgb += (ter_big - 0.5) * 0.85 * vec3f(0.22, 0.14, -0.14);
output_color.rgb *= 1.0 + (ter_hue - 0.5) * 0.85 * 0.40;

// (3) Detail texture (up-facing only)
let ter_top = step(0.5, normal_world.y);
let ter_det = textureSample(detail_map, detail_sampler, ter_wp * 0.18).rgb 
            / max(0.4973, 0.01);
output_color.rgb *= mix(vec3f(1.0), ter_det, 0.65 * ter_top);
```

**Uniforms/Bind Group:**
- `detail_map: texture_2d<f32>` (256×256 canvas from terrainDetail.ts equivalent)
- `detail_sampler: sampler` (repeat wrapping)
- Constants inlined (0.85, 0.18, 0.65, 0.4973) or as Material fields

### Seam Overlay
- Similar approach: flat quads (two triangles) per biome seam
- Bind a `coverage` vertex attribute (0..1 per corner)
- In fragment: discard based on coverage + noise (same `ter_noise` function)
- Use `depth_bias` or `polygonOffset` equivalent in RenderState (factor=-2, units=-4)

### Performance Considerations
- **31k tiles × 6 faces = 186k faces:** Consider GPU instancing via `batch_materialize` or `dynamic_batch` render path
- **Procedural detail texture:** Generate once at app startup (headless-safe via feature gates)
- **World-space noise:** Cheap (hash-based); evaluates once per fragment, no texture lookups per noise octave
- **Sampling detail texture:** Single lookup per fragment; anisotropic filtering (ratio 8) helps RTS camera angle

### Testing
- Verify tile heights match `tileTopY(x, z) = 1 + (h-1)*0.5`
- Compare final RGB after all three noise passes to a reference render from Three.js
- Check detail texture mean matches 0.4973 (compute histogram of generated texture)
- Confirm seam coverage blends correctly at biome boundaries (e.g., forest↔swamp)
