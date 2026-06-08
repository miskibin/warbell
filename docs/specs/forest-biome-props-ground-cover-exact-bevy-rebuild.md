# Forest Biome Props & Ground Cover — Exact Bevy Rebuild Specs

# Forest Biome Props & Ground Cover — Complete Specification

## Overview
The Three.js forest biome scatters rocks, bushes, and ground cover (mushrooms, flowers, tufts) using a deterministic per-tile RNG. All instances are rendered via `InstancedMesh` with per-instance transforms (position, Y-axis rotation, uniform scale) and per-instance color tinting for foliage.

---

## RNG & Placement Formula

**RNG (Xorshift32):**
```
seed = 2027
state = 0 (unsigned 32-bit)

function rng() {
  state = (state + 0x6d2b79f5) >>> 0
  let t = state
  t = Math.imul(t ^ (t >>> 15), t | 1)
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}
```
Seeded once at startup with `2027` (obstacles.ts:249).

**Per-Tile Placement (lines 251–305, obstacles.ts):**
```
for each tile (x, z) in [0, COLS) × [0, ROWS):
  if tile is reserved (castle, camps, roads, structures): skip
  r = rng()
  pick = first roll where r < roll.until
  if no pick: skip

  // Single prop placement:
  if pick.clusterMin === undefined:
    cx = x + 0.5 + (rng() - 0.5) * 0.4    // ±0.2 from tile center
    cz = z + 0.5 + (rng() - 0.5) * 0.4
    rot = snapToCardinal(rng() * π * 2)    // rounds to nearest 90°
    scale = 0.85 + rng() * 0.45            // [0.85, 1.30]
    variant = floor(rng() * 4)             // 0, 1, 2, or 3
    place(kind, cx, cz, scale, rot, variant)

  // Cluster placement (mushroom, flower, tuft only):
  else:
    count = pick.clusterMin + floor(rng() * (pick.clusterMax - pick.clusterMin + 1))
    for i = 0 to count-1:
      place(kind, x + rng(), z + rng(), 0.7 + rng() * 0.5, snapToCardinal(...), floor(rng() * 4))
```

**Thinning (line 271–277):**
- Trees/birch/snowPine: 65% culled if `rng() < 0.65`
- Forest trees: additional 15% culled if `rng() < 0.15`
- Collidable props (radius > 0): 30% culled if `rng() < 0.3`
- Walk-through (radius = 0): 100% placed

---

## Forest Biome Roll Table

**Source: obstacles.ts:187–195**

Forest is the DENSEST biome (overall scatter until 0.97 = ~97% tile coverage):

| Kind | until | clusterMin | clusterMax | Spawn % |
|------|-------|-----------|-----------|---------|
| tree | 0.34 | — | — | 34% |
| birch | 0.48 | — | — | 14% |
| deadTree | 0.52 | — | — | 4% |
| bush | 0.68 | — | — | 16% |
| mushroom | 0.80 | 2 | 4 | 12% |
| flower | 0.86 | 1 | 3 | 6% |
| tuft | 0.97 | 1 | 3 | 11% |

**After thinning:**
- Trees: 34% × (1 - 0.65) × (1 - 0.15) = ~10% actual
- Birch: 14% × (1 - 0.65) = ~5% actual
- Bush: 16% × (1 - 0.30) = ~11% actual
- Walk-through (mushroom/flower/tuft): 100% as rolled

---

## Rock (walk-through)

**Collision radius:** 0

**Geometry (Scatter.tsx:213):**
- **Single IcosahedronGeometry(radius=0.18, detail=0)**
- Pre-baked translation: [0, 0.05, 0] (sits slightly above ground)

**Material (Scatter.tsx:27):**
- Color: `#d3d3d3` (RGB 211, 211, 211 — light gray)
- Roughness: 0.85
- Metalness: 0 (default)
- flatShading: true

**Per-instance scaling (single random):**
- scale = 0.85 + rng() × 0.45 ∈ [0.85, 1.30]

**Scatter in Forest:**
Rocks do NOT appear in forest roll table (0.34–0.97 range has no rock entry). They are exclusive to sand, rock biome, snow, and desert.

---

## Bush (walk-through)

**Collision radius:** 0

**Variants:** 3 color variants (variant % 3)

**Geometry (Scatter.tsx:118–125):**
Each bush is a cluster of 4 IcosahedronGeometry spheres with pre-baked local offsets:

```
bushParts(material) → [
  Part 1: IcosahedronGeometry(radius=0.24, detail=0) at [0, 0.18, 0]
  Part 2: IcosahedronGeometry(radius=0.20, detail=0) at [0.2, 0.15, 0.05]
  Part 3: IcosahedronGeometry(radius=0.18, detail=0) at [-0.17, 0.13, 0.1]
  Part 4: IcosahedronGeometry(radius=0.19, detail=0) at [0.05, 0.22, -0.16]
]
```

**Materials (Scatter.tsx:32–36):**
```
BUSH_MATS[0]: color #3a8a3a (RGB 58, 138, 58 — dark green),   roughness 0.95, flatShading: true
BUSH_MATS[1]: color #4aa84a (RGB 74, 168, 74 — mid green),    roughness 0.95, flatShading: true
BUSH_MATS[2]: color #65bb55 (RGB 101, 187, 85 — light green), roughness 0.95, flatShading: true
```

**Per-instance scaling & tinting:**
- scale = 0.85 + rng() × 0.45 ∈ [0.85, 1.30]
- **Per-instance color tint:** deterministic hash from position:
  ```
  h = abs(sin((x * 12.9898 + z * 78.233) * 43758.5453))
  hh = h - floor(h)
  v = 0.86 + hh * 0.28        // brightness ∈ [0.86, 1.14]
  color = base * v
  color.offsetHSL((hh - 0.5) * 0.04, (hh - 0.5) * 0.10, 0)  // ±hue/sat drift
  ```

**Forest spawn:**
- Roll until 0.68: **16% base, ~11% after 30% thinning**
- Single placement per tile (no clustering)
- Scale: 0.85–1.30

---

## Ground Cover in Forest

### Mushroom (walk-through)

**Collision radius:** 0

**Variants:** 2 visual variants (variant % 2)

**Geometry:**

**Variant 0 (Red cap, Scatter.tsx:127–135):**
```
Part 1: Stem  — CylinderGeometry(radius=0.028 top, 0.04 bottom, height=0.1, segs=6)
           at [0, 0.05, 0]
           Material: STEM_MAT (#f0e8d0, RGB 240, 232, 208)

Part 2: Red cap (dome) — SphereGeometry(radius=0.09, w_segs=8, h_segs=6)
           clipped to upper hemisphere (φ: 0 to π/2)
           at [0, 0.12, 0]
           Material: RED_CAP_MAT (#c83838, RGB 200, 56, 56)

Part 3–5: White dots — 3× SphereGeometry(radius varies)
           Dot 1: radius=0.018 at [0.045, 0.165, 0.02]
           Dot 2: radius=0.014 at [-0.035, 0.155, -0.04]
           Dot 3: radius=0.012 at [0.01, 0.18, 0.05]
           Material: DOT_MAT (#f8f6e8, RGB 248, 246, 232)
```

**Variant 1 (Brown cap, Scatter.tsx:137–142):**
```
Part 1: Stem — same as variant 0
Part 2: Brown cap (dome) — SphereGeometry(radius=0.09, w_segs=8, h_segs=6)
           clipped to upper hemisphere
           at [0, 0.12, 0]
           Material: BROWN_CAP_MAT (#8a5a3a, RGB 138, 90, 58)
           (no white dots)
```

**Forest spawn:**
- Roll until 0.80: **12% base, 100% after thinning (walk-through)**
- **Clustered:** clusterMin=2, clusterMax=4
  - Per tile: 2–4 individual mushrooms
  - Each within [x, x+1) × [z, z+1) randomized
  - Cluster scale: 0.7 + rng() × 0.5 ∈ [0.7, 1.2]

### Flower (walk-through)

**Collision radius:** 0

**Variants:** 4 petal colors (variant % 4)

**Geometry (Scatter.tsx:144–159):**
```
Part 1: Stem — CylinderGeometry(radius=0.008, height=0.14, segs=4)
           at [0, 0.07, 0]
           Material: FLOWER_STEM_MAT (#3a7a2a, RGB 58, 122, 42)

Part 2: Center — SphereGeometry(radius=0.02, w_segs=5, h_segs=4)
           at [0, 0.15, 0]
           Material: FLOWER_CENTER_MAT (#e8c84a, RGB 232, 200, 74)

Parts 3–6: 4× Petals arranged in cardinal cross:
           SphereGeometry(radius=0.028, w_segs=5, h_segs=4)
           Petal i at position [cos(i·π/2)·0.04, 0.15, sin(i·π/2)·0.04]
           Material: FLOWER_PETAL_MATS[variant]
```

**Petal materials (Scatter.tsx:70–75):**
```
FLOWER_PETAL_MATS[0]: color #d63a3a (RGB 214, 58, 58 — red)
FLOWER_PETAL_MATS[1]: color #e6c84a (RGB 230, 200, 74 — yellow)
FLOWER_PETAL_MATS[2]: color #6abadf (RGB 106, 186, 223 — blue)
FLOWER_PETAL_MATS[3]: color #e88ad6 (RGB 232, 138, 214 — magenta)
```
All have roughness 0.9, flatShading false (smooth petals).

**Forest spawn:**
- Roll until 0.86: **6% base, 100% after thinning (walk-through)**
- **Clustered:** clusterMin=1, clusterMax=3
  - Per tile: 1–3 flowers
  - Cluster scale: 0.7 + rng() × 0.5 ∈ [0.7, 1.2]

### Tuft (grass/ground cover, walk-through)

**Collision radius:** 0

**Variant:** single (no variant sub-types)

**Geometry (Scatter.tsx:228–236):**
5 thin cone blades, all rooted near ground, each pre-baked local offset + rotation:

```
Blade 1: ConeGeometry(radius=0.025, height=0.26, segs=4)
         at [0, 0.13, 0]
         rotated [0, 0, 0]

Blade 2: ConeGeometry(radius=0.022, height=0.22, segs=4)
         at [0.06, 0.11, 0.02]
         rotated [0, 0.5, 0.22]

Blade 3: ConeGeometry(radius=0.022, height=0.20, segs=4)
         at [-0.06, 0.1, -0.03]
         rotated [0, -0.4, -0.2]

Blade 4: ConeGeometry(radius=0.02, height=0.18, segs=4)
         at [0.03, 0.09, -0.06]
         rotated [0.25, 0, 0.15]

Blade 5: ConeGeometry(radius=0.02, height=0.17, segs=4)
         at [-0.04, 0.085, 0.05]
         rotated [-0.2, 0, -0.18]
```

**Material (Scatter.tsx:38):**
- Color: `#3aa044` (RGB 58, 160, 68 — grass green)
- Roughness: 1
- flatShading: true
- **Wind sway:** applyWind() injects uniform time-based vertex displacement (see wind.ts)

**Per-instance tinting:**
- Same deterministic hash as bushes (position-based hue/sat/brightness variation)
- Brightness: [0.86, 1.14]
- Hue/sat: ±small drift

**Forest spawn:**
- Roll until 0.97: **11% base, 100% after thinning (walk-through)**
- **Densest ground cover:** clusterMin=1, clusterMax=3
  - Per tile: 1–3 tufts
  - Cluster scale: 0.7 + rng() × 0.5 ∈ [0.7, 1.2]

---

## Collision Radii & Pathing

From obstacles.ts:121–138:

| Kind | Radius | Role |
|------|--------|------|
| rock | 0 | walk-through decor |
| bush | 0 | walk-through decor |
| mushroom | 0 | walk-through decor |
| flower | 0 | walk-through decor |
| tuft | 0 | walk-through decor |
| tree | 0.12 | body-blocking |
| birch | 0.1 | body-blocking |
| deadTree | 0.09 | body-blocking |
| boulder | 0.34 | body-blocking |

**Forest rocks and bush/ground-cover have zero collision:** players/creatures walk through them. Only large trees (0.09–0.12) and boulders (0.34) block movement.

---

## Instanced Rendering (Three.js to Bevy)

**Three.js approach (Scatter.tsx:335–377):**
- Per (material + shadow flag + tint flag) tuple: one InstancedMesh
- Each InstancedMesh holds all obstacles of that sub-kind
- Per-instance matrix set via `setMatrixAt(i, matrix)` where matrix encodes:
  - Position: (o.x, o.y, o.z)
  - Rotation: Y-axis only, o.rot (cardinal: 0°, 90°, 180°, 270°)
  - Scale: uniform, o.scale
- Per-instance color for foliage: `setColorAt(i, color)` with deterministic hash tint

**Bevy equivalent:**
- Use `MaterialMeshBundle` with `InstancedMesh` (or custom `GpuMesh` instancing)
- Transform per instance: mat4 = translate(x, y, z) · rotateY(rot) · scale(uniform)
- Color tint: per-instance attribute (vertex buffer) OR material instance buffer
- For wind sway: inject time into material uniform, apply sine wave to vertex Y

---

## Mesh Merging & Draw Call Optimization

From Scatter.tsx:290–326:

Geometries sharing the same material + shadow flag + tint flag are merged via `mergeGeometries()`. Example forest draw buckets:

| Kind | Material | Shadow | Tint | Merged Into | Instance Count |
|------|----------|--------|------|------------|-----------------|
| bush-0 | BUSH_MATS[0] | true | true | 1 InstancedMesh per tint group | ~130–200 per forest |
| mushroom-0 | RED_CAP_MAT | true | false | separate from stem (STEM_MAT) | ~80–160 |
| flower-0 | FLOWER_PETAL_MATS[0] + center + stem | false/true mixed | false | 3 InstancedMesh (stem+center+petals) | ~40–100 |
| tuft | TUFT_MAT | false | true | 1 InstancedMesh (5 blade cones merged) | ~150–300 |

Total draw calls for forest ground cover: **~20–40** (vs. hundreds if non-instanced).

---

## Forest Map Region

From obstacles.ts:58 (ork camp):
```
forest camp: x=34, z=72 (in enlarged 202×152 map)
```

Forest biome occupies roughly the **SW quadrant** and extends across ~50% of playable tiles. Exact boundary defined by `classifyBiome()` in tileMap.ts (noise-based, continuous transition zone).

**Approximate forest tile count:** ~4,000–5,000 tiles  
**Expected instance counts per prop type** (before thinning):
- Trees: ~1,400–1,700
- Birch: ~560–700
- Bush: ~650–850
- Mushroom clusters: ~480–600
- Flower clusters: ~240–300
- Tuft clusters: ~440–550

---

## Exact Colors (Hex)

**Rock:**
- Base: `#d3d3d3` (rock gray)

**Bushes:**
- Dark: `#3a8a3a`
- Mid: `#4aa84a`
- Light: `#65bb55`

**Mushroom:**
- Stem: `#f0e8d0`
- Red cap: `#c83838`
- Brown cap: `#8a5a3a`
- Dots: `#f8f6e8`

**Flower:**
- Stem: `#3a7a2a`
- Center: `#e8c84a`
- Petals: `#d63a3a`, `#e6c84a`, `#6abadf`, `#e88ad6`

**Tuft:**
- Base: `#3aa044`

---

## Summary Table: Forest Biome Props

| Type | Geometry | Primary Color | Scale Range | Cluster | Walk-Through | Forest % |
|------|----------|---|---|------|-----|------|
| **Rock** | Icosahedron(r=0.18) | #d3d3d3 | [0.85, 1.30] | no | ✓ | — (not in forest) |
| **Bush** | 4× Icosphere clusters | #3a8a3a–#65bb55 | [0.85, 1.30] | no | ✓ | ~11% |
| **Mushroom** | Cylinder stem + sphere cap | #c83838 / #8a5a3a | [0.7, 1.2] | 2–4 | ✓ | ~12% |
| **Flower** | Stem + 4 petal spheres | 4 variants | [0.7, 1.2] | 1–3 | ✓ | ~6% |
| **Tuft** | 5 cone blades | #3aa044 | [0.7, 1.2] | 1–3 | ✓ | ~11% |



## Exact Constants

**Forest Biome — RNG & Roll Table**
- RNG seed: 2027
- RNG increment: 0x6d2b79f5 (Xorshift32)

**Forest Roll Table (until values)**
- tree: 0.34
- birch: 0.48
- deadTree: 0.52
- bush: 0.68
- mushroom: 0.80 (clusterMin: 2, clusterMax: 4)
- flower: 0.86 (clusterMin: 1, clusterMax: 3)
- tuft: 0.97 (clusterMin: 1, clusterMax: 3)

**Thinning Rates**
- tree/birch/snowPine base: 65% culled
- forest tree extra: 15% culled
- collidable (radius > 0): 30% culled
- walk-through (radius = 0): 0% culled

**Per-Tile Placement**
- Single prop: cx = x + 0.5 + (rng() - 0.5) × 0.4
- Single prop: cz = z + 0.5 + (rng() - 0.5) × 0.4
- Cluster: each item at x + rng(), z + rng()
- Scale (single): 0.85 + rng() × 0.45 ∈ [0.85, 1.30]
- Scale (cluster): 0.7 + rng() × 0.5 ∈ [0.7, 1.2]
- Rotation: snapToCardinal(rng() × π × 2) → {0°, 90°, 180°, 270°}
- Variant: floor(rng() × 4) ∈ {0, 1, 2, 3}

**Rock**
- Geometry: IcosahedronGeometry(radius=0.18, subdivisions=0)
- Baked position: [0, 0.05, 0]
- Color: #d3d3d3
- Roughness: 0.85
- Collision radius: 0
- Scale: [0.85, 1.30]
- castShadow: true

**Bush (3 color variants)**
- Variant 0 color: #3a8a3a
- Variant 1 color: #4aa84a
- Variant 2 color: #65bb55
- Roughness: 0.95
- flatShading: true
- Collision radius: 0
- Scale: [0.85, 1.30]
- castShadow: true
- Parts (radii, offsets):
  - [0.24, 0, 0.18, 0]
  - [0.20, 0.2, 0.15, 0.05]
  - [0.18, -0.17, 0.13, 0.1]
  - [0.19, 0.05, 0.22, -0.16]
- Per-instance tint formula: v = 0.86 + fract(sin(...)) × 0.28

**Mushroom (2 variants)**
- Variant 0 (red):
  - Stem color: #f0e8d0, radius: 0.028–0.04, height: 0.1
  - Cap color: #c83838, radius: 0.09
  - Dots: #f8f6e8, radii: 0.018, 0.014, 0.012
- Variant 1 (brown):
  - Stem color: #f0e8d0, radius: 0.028–0.04, height: 0.1
  - Cap color: #8a5a3a, radius: 0.09
- Roughness: 0.9
- Collision radius: 0
- Cluster scale: [0.7, 1.2]
- Cluster count: 2–4 per tile
- castShadow: true

**Flower (4 petal variants)**
- Stem color: #3a7a2a, radius: 0.008, height: 0.14
- Center color: #e8c84a, radius: 0.02
- Petal colors: #d63a3a (red), #e6c84a (yellow), #6abadf (blue), #e88ad6 (magenta)
- Petal radius: 0.028, offset distance: 0.04 (cardinal cross)
- Roughness: 0.9
- Collision radius: 0
- Cluster scale: [0.7, 1.2]
- Cluster count: 1–3 per tile
- castShadow: false

**Tuft**
- Color: #3aa044
- Roughness: 1
- flatShading: true
- Collision radius: 0
- Blade specs (radius, height, offset [x,y,z], rotation [x,y,z]):
  - 0.025, 0.26, [0, 0.13, 0], [0, 0, 0]
  - 0.022, 0.22, [0.06, 0.11, 0.02], [0, 0.5, 0.22]
  - 0.022, 0.20, [-0.06, 0.1, -0.03], [0, -0.4, -0.2]
  - 0.02, 0.18, [0.03, 0.09, -0.06], [0.25, 0, 0.15]
  - 0.02, 0.17, [-0.04, 0.085, 0.05], [-0.2, 0, -0.18]
- Cluster scale: [0.7, 1.2]
- Cluster count: 1–3 per tile
- Per-instance tint: same hash as bushes
- castShadow: false
- Wind sway: enabled (applyWind uniform)

**Map Dimensions**
- COLS: 202
- ROWS: 152
- Forest ork camp (BASE 34, 72): → enlarged coords ~48, 101

**Collision Radius Table**
- rock: 0
- bush: 0
- mushroom: 0
- flower: 0
- tuft: 0
- tree: 0.12
- birch: 0.10
- deadTree: 0.09
- boulder: 0.34
- cactus: 0.18
- snowPine: 0.12
- iceShard: 0
- bones: 0
- reeds: 0


## Bevy 0.18 Notes
## Bevy 0.18.1 Implementation Notes

### 1. Instanced Mesh Setup
- Use `mesh: Handle<Mesh>` + `material: Handle<StandardMaterial>` (or custom `Material2d` for foliage wind)
- Create custom component `InstanceData` storing per-instance transforms + optional color tint:
  ```rust
  struct InstanceData {
    position: Vec3,
    rotation: f32,  // radians, Y-axis only
    scale: f32,
    color_tint: Color,  // for bushes/tufts
  }
  ```
- Use `InstanceBuffer` (or raw GPU buffer) to upload all transforms at once
- Bind via `MeshUniform` with instancing enabled in shader

### 2. Rock (Simple Single Icosphere)
- Mesh: `IcoSphere { radius: 0.18, subdivisions: 0 }` (20 vertices, 60 indices)
- Bake Y-offset +0.05 into mesh vertices or apply in transform
- Material: `StandardMaterial { base_color: Color::hex("d3d3d3").unwrap(), roughness: 0.85, ... }`
- No per-instance color; uniform material across all rocks
- Collision: radius 0 (no physics body)

### 3. Bush (Clustered Icospheres)
- **Mesh:** Merge 4 icospheres (r=0.24, 0.20, 0.18, 0.19) at baked local offsets into single GPU mesh
  ```rust
  let mut geo = Mesh::new(PrimitiveTopology::TriangleList, ...);
  // Append icosphere 1 (translated [0, 0.18, 0])
  // Append icosphere 2 (translated [0.2, 0.15, 0.05])
  // Append icosphere 3 (translated [-0.17, 0.13, 0.1])
  // Append icosphere 4 (translated [0.05, 0.22, -0.16])
  ```
- **Material:** Create 3 material instances (one per variant color)
- **Per-instance color:** Hash from world position:
  ```rust
  let h = (x * 12.9898 + z * 78.233).sin().abs().fract();
  let v = 0.86 + h * 0.28;
  let color = base_color * v;
  // Also apply hue/sat shift: color.hsl_adjust(...)
  ```
- Instance buffer: 3 separate `InstancedMeshBundle` (one per color variant)
- No wind sway (bushes are rigid)

### 4. Mushroom (Stem + Cap + Dots)
- **Mesh:** Merge all parts into single GPU mesh per variant:
  - **Variant 0 (red):** Cylinder stem (r_top=0.028, r_bot=0.04, h=0.1) + Sphere cap (r=0.09, clipped to upper hemisphere φ∈[0, π/2]) at [0, 0.12, 0] + 3 white dot spheres
  - **Variant 1 (brown):** Cylinder stem + Sphere cap (brown) at [0, 0.12, 0] (no dots)
- **Material:** 
  - Stem: `#f0e8d0`
  - Cap: `#c83838` (red) or `#8a5a3a` (brown)
  - Dots: `#f8f6e8`
  - All: roughness 0.9, flatShading (flat vertex normals in shader)
- Two InstancedMeshBundle (one per cap color)
- No per-instance color tint; static materials
- No wind

### 5. Flower (Stem + 4 Petals + Center)
- **Mesh:** Merge all parts into single GPU mesh:
  - Thin cylinder stem (r=0.008, h=0.14) at [0, 0.07, 0]
  - Sphere center (r=0.02) at [0, 0.15, 0]
  - 4 sphere petals (r=0.028) arranged in cardinal cross
- **Material:**
  - Stem: `#3a7a2a` (green)
  - Center: `#e8c84a` (yellow)
  - Petals: 4 colors (red, yellow, blue, magenta)
  - All: roughness 0.9
  - Petals: **smooth shading** (interpolated normals), not flat
- 4 separate InstancedMeshBundle (one per petal color)
- No per-instance color; static
- No wind

### 6. Tuft (5 Lean Cones)
- **Mesh:** Merge 5 cones (radii 0.025–0.02, heights 0.26–0.17) at baked local offsets + rotations:
  ```rust
  let mut geo = Mesh::new(...);
  // Append cone 1 (r=0.025, h=0.26) at [0, 0.13, 0], rotated identity
  // Append cone 2 (r=0.022, h=0.22) at [0.06, 0.11, 0.02], rotated [0, 0.5, 0.22]
  // ... (5 total)
  ```
- **Material:** `StandardMaterial { base_color: Color::hex("3aa044").unwrap(), ... }` with custom shader for wind sway
- **Per-instance color:** Hash from world position (same as bushes)
  - Brightness: [0.86, 1.14]
  - Hue/sat drift: small randomness
- **Wind sway:** Inject `time: f32` uniform, apply sine wave to vertex Y in shader:
  ```glsl
  float wind_sway = sin(time + position.x * 5.0 + position.z * 3.0) * 0.03;
  position.x += wind_sway;
  ```
  (exact wind formula in src/world/wind.ts, apply same logic)
- Single InstancedMeshBundle (all tufts, one color material with per-instance tint)

### 7. RNG & Deterministic Placement
- Port Xorshift32 to Rust:
  ```rust
  fn rng(state: &mut u32) -> f32 {
    *state = state.wrapping_add(0x6d2b79f5);
    let mut t = *state;
    t ^= t >> 15;
    t = t.wrapping_mul(t | 1);
    t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
    t ^= t >> 14;
    ((t as u32) as f32) / 4294967296.0
  }
  ```
- Seed: `state = 2027` at startup
- Iterate per tile (x, z) in [0, COLS) × [0, ROWS), reusing same RNG state

### 8. Transform & Baking
- Per-instance transform = translate(x, y, z) · rotateY(rot) · scale(uniform_scale)
- Y-offset (e.g., rock +0.05) baked into mesh geometry OR applied uniformly in all instances
- Cardinal rotation: snap rng() * π * 2 to nearest 0°/90°/180°/270°

### 9. Shadow Casting
- Rock/bush/mushroom/flower: `cast_shadow: true`
- Tuft: `cast_shadow: false` (fine ground cover, no shadow needed)
- Receive shadow: all

### 10. Collision
- All forest ground cover (rock, bush, mushroom, flower, tuft): **radius 0**
  - Do NOT add Collider; they are walk-through decor
  - Only trees (0.09–0.12) and boulders (0.34) block movement
- Use spatial grid (as in obstacles.ts) if collision-checking is needed for other systems

### 11. Shader Considerations
- Use `StandardMaterial` for rocks (trivial PBR)
- Use `StandardMaterial` with `flat_shading: true` for bushes, mushrooms, flowers (low-poly faceted look)
- Tuft: custom shader with wind sway uniform
- All: receive shadows, soft shadows if PCSS enabled

### 12. Performance Optimization
- Merge geometries per (material + shadow flag + tint flag) tuple (as in Three.js code)
- Example: all tuft blade cones → single GPU mesh, instanced with per-instance color
- Avoid per-vertex color in large batches; use per-instance color buffer
- LOD: at distance, reduce instance count or cull entirely (optional, depends on game perf target)

