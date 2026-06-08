# Forest Tree Models: Exact Bevy Rebuild Recipe

# Forest Tree Model Specifications

## Overview
The tileworld forest biome uses THREE.js instanced meshes to render trees. There are **three main tree variants** scattered across the forest:
- **tree**: irregular broadleaf (most common, 34% spawn chance in forest)
- **birch**: tall thin-trunk deciduous (15% spawn chance)
- **deadTree**: weathered trunks with broken branches (4% spawn chance)

### Forest Biome Density (from obstacles.ts:187-195)
- **Forest spawn probability**: 0.34 (34% of tiles roll a tree when generating obstacles)
- **Tree thinning**: 65% of rolled trees are culled for playability (to prevent visual/collision overload)
- **Additional forest-specific culling**: Extra 15% of remaining trees dropped (forest reads 15% too thick)
- **Effective spawn density**: ~34% × (1 - 0.65) × (1 - 0.15) ≈ **7.6% actual tree placement** per forest tile
- **Forest region**: center (32, 80) in base coords, radius 34 tiles (≈47.6 tiles post-1.4× scale)
- **Scatter algorithm**: Golden-angle spiral across outer annulus (0.55r to 0.95r radius)
- **Collision radius (walk-through)**:
  - tree: 0.12
  - birch: 0.10
  - deadTree: 0.09
  - (All small; trees are body-blocking but don't impede movement much)

### Per-Instance Variation (obstacles.ts:279-284)
- **Position jitter**: ±0.2 tiles (rand() - 0.5 × 0.4) in x and z within each tile
- **Scale range**: 0.85 to 1.3 (0.85 + rand() × 0.45)
- **Rotation**: Snap to cardinal (4 rotations: 0, π/2, π, 3π/2)
- **Y placement**: Snapped to tile's tileTopY (ground surface per heightmap)
- **Variant index**: 0–3 (4 visual variants per tree type, used for shape randomization)

---

## TREE (Regular Deciduous) - Scatter.tsx:162-170

### Trunk Geometry
```
CylinderGeometry(
  radiusTop: 0.09,
  radiusBottom: 0.12,
  height: 0.5,
  radialSegments: 6
)
Position: [0, 0.25, 0]  // Base at y=0, top at y=0.5
Material: TRUNK_MAT
  Color: #5a3a22 (dark brown)
  Roughness: 1.0
  Metalness: default (0)
  CastShadow: true
```

### Foliage Layers (Icosahedra - detail level 1)
Six spherical foliage layers, offset and sized to create an irregular crown:

#### Layer 1 (Dark base, main mass)
```
IcosahedronGeometry(radius: 0.46, detail: 1)
Position: [0, 0.64, 0]
Material: FOLIAGE_DARK_MAT
  Color: #2f7a36 (cool dark green)
  Roughness: 0.95
  FlatShading: true
  CastShadow: true
  Tint: true (per-instance color variation ±14% brightness)
```

#### Layer 2 (Dark offset right-forward)
```
IcosahedronGeometry(radius: 0.26, detail: 1)
Position: [0.24, 0.6, 0.06]
Material: FOLIAGE_DARK_MAT
  (same as Layer 1)
```

#### Layer 3 (Mid tone, upper)
```
IcosahedronGeometry(radius: 0.4, detail: 1)
Position: [0, 0.86, 0]
Material: FOLIAGE_MID_MAT
  Color: #3a9442 (medium green)
  Roughness: 0.95
  FlatShading: true
  CastShadow: true
  Tint: true
```

#### Layer 4 (Mid offset left-back)
```
IcosahedronGeometry(radius: 0.24, detail: 1)
Position: [-0.22, 0.82, -0.08]
Material: FOLIAGE_MID_MAT
  (same as Layer 3)
```

#### Layer 5 (Light top, main crown cap)
```
IcosahedronGeometry(radius: 0.33, detail: 1)
Position: [0, 1.06, 0]
Material: FOLIAGE_LIGHT_MAT
  Color: #4cb358 (bright green)
  Roughness: 0.95
  FlatShading: true
  CastShadow: true
  Tint: true
```

#### Layer 6 (Light top tip)
```
IcosahedronGeometry(radius: 0.22, detail: 1)
Position: [0, 1.24, 0]
Material: FOLIAGE_LIGHT_MAT
  (same as Layer 5)
```

**Tree height**: ~1.24 units (icosahedron extends 0.22 units above y=1.24, so max reach ≈ 1.46)
**Crown spread**: Main icosphere r=0.46, layered spread creates blobby irregular canopy
**Tint variation formula**: Hash(x × 12.9898 + z × 78.233) × 43758.5453; brightness 0.86–1.14, hue/sat drift ±2% (Scatter.tsx:356-362)

---

## BIRCH (Tall, Thin-Trunk Deciduous) - Scatter.tsx:171-179

### Trunk Geometry
```
CylinderGeometry(
  radiusTop: 0.06,
  radiusBottom: 0.075,
  height: 0.8,
  radialSegments: 6
)
Position: [0, 0.4, 0]  // Base at y=0, top at y=0.8
Material: BIRCH_TRUNK_MAT
  Color: #ece8d8 (pale cream)
  Roughness: 0.9
  CastShadow: true
```

### Trunk Marks (Dark bark stripes)
Two decorative box marks to suggest birch's characteristic peeling stripes:

#### Mark 1 (upper)
```
BoxGeometry(width: 0.005, height: 0.04, depth: 0.08)
Position: [0.075, 0.55, 0]
Material: BIRCH_MARK_MAT
  Color: #2a261e (very dark brown)
  Roughness: 1.0
  (no shadow; detail accent)
```

#### Mark 2 (lower)
```
BoxGeometry(width: 0.005, height: 0.03, depth: 0.06)
Position: [-0.075, 0.32, 0.02]
Material: BIRCH_MARK_MAT
```

### Foliage Layers (Icosahedra - detail level 0, rounder)
Four spherical foliage masses, creating a fuller, rounder canopy than the regular tree:

#### Layer 1 (Dark base crown)
```
IcosahedronGeometry(radius: 0.34, detail: 0)
Position: [0, 0.95, 0]
Material: BIRCH_DARK_MAT
  Color: #3a8c34 (medium green, slightly warmer than tree)
  Roughness: 0.95
  FlatShading: true
  CastShadow: true
```

#### Layer 2 (Light right bump)
```
IcosahedronGeometry(radius: 0.22, detail: 0)
Position: [0.18, 1.05, 0.1]
Material: BIRCH_LIGHT_MAT
  Color: #7dc04a (bright yellow-green)
  Roughness: 0.95
  FlatShading: true
  CastShadow: true
```

#### Layer 3 (Dark left bump)
```
IcosahedronGeometry(radius: 0.24, detail: 0)
Position: [-0.16, 1.0, -0.1]
Material: BIRCH_DARK_MAT
```

#### Layer 4 (Light top tip)
```
IcosahedronGeometry(radius: 0.18, detail: 0)
Position: [0.05, 1.18, 0]
Material: BIRCH_LIGHT_MAT
```

**Birch height**: ~1.36 units (0.8 trunk + foliage extending to 1.18)
**Crown spread**: Rounder than tree (detail:0 icosahedra are less faceted)
**Silhouette**: Taller, thinner trunk with rounder, fuller canopy

---

## DEAD TREE (Weathered, Broken Branches) - Scatter.tsx:190-212

### Main Trunk
```
CylinderGeometry(
  radiusTop: 0.06,
  radiusBottom: 0.095,
  height: 0.9,
  radialSegments: 6
)
Position: [0, 0.45, 0]  // Base at y=0, top at y=0.9
Material: DEAD_MAT
  Color: #6e6258 (dusty gray-brown)
  Roughness: 1.0
  FlatShading: true
  CastShadow: true
```

### Branch 1 (upper right, angled)
```
CylinderGeometry(
  radiusTop: 0.025,
  radiusBottom: 0.04,
  height: 0.42,
  radialSegments: 5
)
Position: [0.2, 0.7, 0.08]
Rotation: [0, 0, -0.8 radians] (rotate Z by -0.8)
Material: DEAD_DARK_MAT
  Color: #4a4238 (darker gray-brown)
  Roughness: 1.0
  FlatShading: true
  CastShadow: true
```

### Branch 2 (upper left, angled)
```
CylinderGeometry(
  radiusTop: 0.022,
  radiusBottom: 0.035,
  height: 0.36,
  radialSegments: 5
)
Position: [-0.17, 0.82, -0.04]
Rotation: [0, 0, 0.7 radians]
Material: DEAD_DARK_MAT
```

### Branch 3 (mid upper right)
```
CylinderGeometry(
  radiusTop: 0.018,
  radiusBottom: 0.028,
  height: 0.3,
  radialSegments: 5
)
Position: [0.06, 1.0, 0.13]
Rotation: [0.4, 0, 0.2 radians]
Material: DEAD_MAT
  Color: #6e6258
  CastShadow: true
```

### Branch 4 (mid upper left)
```
CylinderGeometry(
  radiusTop: 0.016,
  radiusBottom: 0.024,
  height: 0.26,
  radialSegments: 5
)
Position: [-0.08, 1.05, -0.1]
Rotation: [-0.3, 0, -0.4 radians]
Material: DEAD_MAT
  CastShadow: true
```

**Dead tree height**: ~1.05 units (main trunk) + branch reach
**Character**: Skeletal, gnarled; no foliage, only angled branch stubs
**Used in**: Forest (4% roll, survives thinning less often), swamp, and rock biomes

---

## Wind Sway Physics (wind.ts:19-37)

### Vertex Shader Injection
Applied to all foliage materials (tree, birch foliage only; trunks and dead trees remain rigid):

#### Uniforms
```glsl
uniform float uWindTime;      // Elapsed seconds (windTime.value)
uniform float uWindStrength;  // Global multiplier (default 1.0, tunable via leva)
uniform float uWindSpeed;     // Global frequency multiplier (default 1.0)
```

#### Displacement Formula (per-vertex, applied in vertex shader `<begin_vertex>` stage)
```glsl
float t = uWindTime * uWindSpeed;
// Deterministic per-instance phase from world position (no lockstep swaying)
float phase = instanceMatrix[3].x * 0.7 + instanceMatrix[3].z * 0.55;
// Height-weighted sway: base stays planted, crown bends most
float h = max(transformed.y, 0.0);
float k = h * h;  // Square the height (quadratic falloff)
// Two-frequency oscillation for organic motion
transformed.x += (sin(t * 1.5 + phase) + 0.4 * sin(t * 3.1 + phase * 1.7)) * 0.045 * k * uWindStrength;
transformed.z += cos(t * 1.2 + phase * 1.1) * 0.035 * k * uWindStrength;
```

#### Sway Parameters
- **Base amplitude (X)**: 0.045 units (primary at freq 1.5, secondary at freq 3.1)
- **Base amplitude (Z)**: 0.035 units (single wave at freq 1.2)
- **Height scaling**: Quadratic (k = h²) so only the canopy moves significantly
- **Phase offset**: Deterministic per instance: phase = instanceX × 0.7 + instanceZ × 0.55
- **Secondary harmonic**: 40% of primary X wave at double freq with different phase coefficient

#### Runtime Control
```typescript
windTime = { value: 0 }        // Updated by WindDriver every frame to clock.getElapsedTime()
windStrength = { value: 1.0 }  // User-tunable amplitude multiplier
windSpeed = { value: 1.0 }     // User-tunable frequency multiplier
```

---

## Placement Algorithm (obstacles.ts:248-305)

### Deterministic RNG
```typescript
function rng(seed: number) {
  let s = seed >>> 0
  return () => {
    s = (s + 0x6d2b79f5) >>> 0
    let t = s
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}
```
**Seed**: 2027 (fixed, deterministic map generation)

### Per-Tile Placement
For each tile (x, z) in the forest biome:
1. Roll rand() in [0, 1)
2. Check forest roll table (up to 0.34 = tree variant)
3. If tree selected:
   - Check thinning: if rand() < 0.65, skip (65% culled)
   - If forest biome: if rand() < 0.15, skip extra 15%
4. If not skipped:
   - **Tile center jitter**: cx = x + 0.5 + (rand() - 0.5) × 0.4, cz = z + 0.5 + (rand() - 0.5) × 0.4
   - **Scale**: 0.85 + rand() × 0.45 (range [0.85, 1.3])
   - **Rotation**: snapToCardinal(rand() × π × 2) → 0, π/2, π, 3π/2
   - **Variant**: floor(rand() × 4) → 0–3

### Y-Position (Ground Snap)
```typescript
const baseTile = tileAt(Math.floor(x), Math.floor(z))
const y = baseTile ? tileTopY(Math.floor(x), Math.floor(z)) : 1
// tileTopY is height class × GROUND_STEP (0.5); snaps model base to terrain surface
```

---

## THREE.js Instancing Pattern (Scatter.tsx:335-376)

### InstancedMesh Setup per Obstacle Type
```typescript
// One InstancedMesh per (material, castShadow, tint) bucket
// Trees: 4 buckets (4 distinct foliage mats, each cast shadow + tint)
// Birches: 4 buckets
// Dead trees: 2 buckets (trunk + branches share colors)

const m = InstancedMesh(geometry, material, obstacles.length)
for (let i = 0; i < obstacles.length; i++) {
  dummy.position.set(o.x, o.y, o.z)
  dummy.rotation.set(0, o.rot, 0)  // Only Y-axis rotation
  dummy.scale.setScalar(o.scale)
  m.setMatrixAt(i, dummy.matrix)
}
m.instanceMatrix.needsUpdate = true
m.computeBoundingSphere()
```

### Per-Instance Tint Color
For foliage parts (tint: true):
```typescript
const h = Math.abs(Math.sin((o.x * 12.9898 + o.z * 78.233) * 43758.5453))
const hh = h - Math.floor(h)  // fract(h)
const v = 0.86 + hh * 0.28   // brightness: 0.86..1.14
col.copy(base).multiplyScalar(v)
col.offsetHSL((hh - 0.5) * 0.04, (hh - 0.5) * 0.10, 0)  // tiny hue/sat shift
m.setColorAt(i, col)
```

---

## Summary Table

| Aspect | Tree | Birch | Dead Tree |
|--------|------|-------|-----------|
| **Trunk Radii** | 0.09–0.12 | 0.06–0.075 | 0.06–0.095 |
| **Trunk Height** | 0.5 | 0.8 | 0.9 |
| **Trunk Color** | #5a3a22 | #ece8d8 | #6e6258 |
| **Foliage Count** | 6 icosahedra | 4 icosahedra | 4 branches (cylinders) |
| **Max Height** | ~1.46 | ~1.36 | ~1.05 |
| **Roughness** | 0.95–1.0 | 0.9–0.95 | 1.0 |
| **Forest Spawn %** | ~10.2% (34 × 0.35 × 0.85) | ~5.1% (15 × 0.35) | ~1.4% (4 × 0.35) |
| **Collision Radius** | 0.12 | 0.1 | 0.09 |
| **Wind Affected** | Foliage only | Foliage only | No (wood material) |

---

## Hex Color Reference

**Greens (Tree Foliage)**
- Dark: #2f7a36 (RGB 47, 122, 54)
- Mid: #3a9442 (RGB 58, 148, 66)
- Light: #4cb358 (RGB 76, 179, 88)

**Greens (Birch Foliage)**
- Dark: #3a8c34 (RGB 58, 140, 52)
- Light: #7dc04a (RGB 125, 192, 74)

**Browns (Trunks)**
- Standard: #5a3a22 (RGB 90, 58, 34)
- Birch: #ece8d8 (RGB 236, 232, 216)
- Dead: #6e6258 (RGB 110, 98, 88)
- Dead Dark: #4a4238 (RGB 74, 66, 56)
- Birch Marks: #2a261e (RGB 42, 38, 30)


## Exact Constants

**Forest Region (base coords)**: center (32, 80), radius 34 tiles
**Forest Region (post-1.4× scale)**: center ~(37.8, 94.6), radius ~47.6 tiles

**Tree Variant Spawn Chances (Forest)**:
- tree: until 0.34 (34%)
- birch: until 0.48 (14% of remaining)
- deadTree: until 0.52 (4% of remaining)

**Tree Culling (Thinning)**:
- Standard body-blocking culling: 65% dropped
- Forest-specific extra culling: 15% of remainder

**Collision Radii**:
- tree: 0.12
- birch: 0.1
- deadTree: 0.09

**Per-Instance Variation Ranges**:
- Scale: 0.85 to 1.3 (formula: 0.85 + rand() × 0.45)
- Position jitter: ±0.2 tiles per axis (formula: (rand() - 0.5) × 0.4)
- Rotation: 4 cardinal angles (0°, 90°, 180°, 270°)

**TREE Trunk**:
- radiusTop: 0.09
- radiusBottom: 0.12
- height: 0.5
- radialSegments: 6
- y-position: 0.25 (centers trunk)
- color: #5a3a22
- roughness: 1.0

**TREE Foliage (6 layers)**:
- Layer 1: IcosahedronGeometry(0.46, 1), pos [0, 0.64, 0], color #2f7a36
- Layer 2: IcosahedronGeometry(0.26, 1), pos [0.24, 0.6, 0.06], color #2f7a36
- Layer 3: IcosahedronGeometry(0.4, 1), pos [0, 0.86, 0], color #3a9442
- Layer 4: IcosahedronGeometry(0.24, 1), pos [-0.22, 0.82, -0.08], color #3a9442
- Layer 5: IcosahedronGeometry(0.33, 1), pos [0, 1.06, 0], color #4cb358
- Layer 6: IcosahedronGeometry(0.22, 1), pos [0, 1.24, 0], color #4cb358
- All foliage: roughness 0.95, flatShading true

**BIRCH Trunk**:
- radiusTop: 0.06
- radiusBottom: 0.075
- height: 0.8
- radialSegments: 6
- y-position: 0.4
- color: #ece8d8
- roughness: 0.9

**BIRCH Marks (2 boxes)**:
- Mark 1: [0.005, 0.04, 0.08], pos [0.075, 0.55, 0]
- Mark 2: [0.005, 0.03, 0.06], pos [-0.075, 0.32, 0.02]
- color: #2a261e
- roughness: 1.0

**BIRCH Foliage (4 layers)**:
- Layer 1: IcosahedronGeometry(0.34, 0), pos [0, 0.95, 0], color #3a8c34
- Layer 2: IcosahedronGeometry(0.22, 0), pos [0.18, 1.05, 0.1], color #7dc04a
- Layer 3: IcosahedronGeometry(0.24, 0), pos [-0.16, 1.0, -0.1], color #3a8c34
- Layer 4: IcosahedronGeometry(0.18, 0), pos [0.05, 1.18, 0], color #7dc04a
- All foliage: roughness 0.95, flatShading true

**DEAD TREE Trunk**:
- radiusTop: 0.06
- radiusBottom: 0.095
- height: 0.9
- radialSegments: 6
- y-position: 0.45
- color: #6e6258
- roughness: 1.0

**DEAD TREE Branches (4 cylinders)**:
- Branch 1: CylinderGeometry(0.025, 0.04, 0.42, 5), pos [0.2, 0.7, 0.08], rot [0, 0, -0.8], color #4a4238
- Branch 2: CylinderGeometry(0.022, 0.035, 0.36, 5), pos [-0.17, 0.82, -0.04], rot [0, 0, 0.7], color #4a4238
- Branch 3: CylinderGeometry(0.018, 0.028, 0.3, 5), pos [0.06, 1.0, 0.13], rot [0.4, 0, 0.2], color #6e6258
- Branch 4: CylinderGeometry(0.016, 0.024, 0.26, 5), pos [-0.08, 1.05, -0.1], rot [-0.3, 0, -0.4], color #6e6258
- All: roughness 1.0, flatShading true

**Wind Sway (Vertex Shader Constants)**:
- Primary X frequency: 1.5
- Secondary X frequency: 3.1
- Secondary X amplitude factor: 0.4 (40% of primary)
- Primary Z frequency: 1.2
- Base X amplitude: 0.045
- Base Z amplitude: 0.035
- Height-weight function: k = h² (quadratic)
- Phase formula per instance: phase = x × 0.7 + z × 0.55

**Tint Color Variation**:
- Hash input: x × 12.9898 + z × 78.233
- Hash multiplier: 43758.5453
- Brightness range: 0.86 to 1.14 (formula: 0.86 + hh × 0.28)
- Hue shift: (hh - 0.5) × 0.04
- Saturation shift: (hh - 0.5) × 0.10

**RNG Seed**: 2027

**Scatter Inner/Outer Annulus** (for apple trees, also forest biome structure):
- SCATTER_INNER: 0.55 (55% of region radius)
- SCATTER_OUTER: 0.95 (95% of region radius)
- Golden angle: 2.39996323 radians

**Ground Height Unit**: GROUND_STEP = 0.5 (height class = y-position in world)


## Bevy 0.18 Notes

## Bevy 0.18.1 Rebuild Strategy

### 1. Component Structure
```rust
#[derive(Component)]
pub struct TreeModel {
    pub variant: TreeVariant,
    pub scale: f32,
    pub tint: Color,
}

#[derive(Clone, Copy)]
pub enum TreeVariant {
    Tree,
    Birch,
    DeadTree,
}

#[derive(Component)]
pub struct WindAffected;  // Marker for foliage subject to wind
```

### 2. Mesh Building Approach
**Two options** for Bevy:

**Option A: Pre-merged Static Meshes (Recommended for Static Scenes)**
- Build each tree variant as a single merged mesh file (gltf/glb) offline using Blender
- Define trunk + foliage as separate mesh primitives merged per-tree
- Load via Bevy's asset system; spawn via InstancedMesh or Mesh2d if using that
- **Pros**: Clean hierarchy, offline control, single mesh upload per tree type
- **Cons**: Requires external tool, asset pipeline step

**Option B: Runtime Mesh Construction (Pure Bevy)**
```rust
pub fn build_tree_mesh(variant: TreeVariant) -> Mesh {
    let mut tree_mesh = Mesh::new(PrimitiveTopology::TriangleList);
    
    // Trunk (cylinder primitive)
    let trunk = Mesh::from(shape::Cylinder {
        radius_bottom: radius_by_variant(variant).0,
        radius_top: radius_by_variant(variant).1,
        height: trunk_height_by_variant(variant),
        segments: 6,
    });
    
    // Foliage layers (UV sphere/icosphere primitives)
    // For each foliage layer, create icosphere at detail level 0 or 1
    // Apply translations/rotations in local space
    
    // Merge all meshes
    // Note: bevy::mesh::Mesh has no built-in merge; use mesh_to_vertices helper
    // or layer multiple meshes as sub-entities with Transform offsets
}
```

### 3. Material Setup
```rust
use bevy::pbr::{StandardMaterial, MaterialPlugin};

pub fn setup_tree_materials(
    mut materials: ResMut<Assets<StandardMaterial>>,
) -> TreeMaterials {
    TreeMaterials {
        trunk: materials.add(StandardMaterial {
            base_color: Color::hex("#5a3a22").unwrap(),
            roughness: 1.0,
            metallic: 0.0,
            ..default()
        }),
        foliage_dark: materials.add(StandardMaterial {
            base_color: Color::hex("#2f7a36").unwrap(),
            roughness: 0.95,
            ..default()
        }),
        foliage_mid: materials.add(StandardMaterial {
            base_color: Color::hex("#3a9442").unwrap(),
            roughness: 0.95,
            ..default()
        }),
        foliage_light: materials.add(StandardMaterial {
            base_color: Color::hex("#4cb358").unwrap(),
            roughness: 0.95,
            ..default()
        }),
        // ... birch and dead tree materials
    }
}
```

### 4. Wind Animation
**For Static Forest**: Skip per-frame wind if the scene is static. Instead:
- **Option A: Vertex Shader Wind (Compile-time)**
  - Create a custom shader that includes the wind vertex displacement
  - Compile into StandardMaterial's vertex_shader via Material trait
  - Run same formula as THREE.js:
    ```glsl
    // In vertex shader:
    let h = max(vertex.position.y, 0.0);
    let k = h * h;
    vertex.position.x += (sin(time * 1.5 + phase) + 0.4 * sin(time * 3.1 + phase * 1.7)) * 0.045 * k * wind_strength;
    vertex.position.z += cos(time * 1.2 + phase * 1.1) * 0.035 * k * wind_strength;
    ```
  - Bind `time` as a uniform (updated once per frame in a system)
  - Bind `phase` per instance or per mesh

- **Option B: CPU-based Animation**
  - Update Transform.translation each frame for sway
  - Less efficient; good only for small tree counts
  - Formula: offset = displacement from wind equation applied to base position

**Recommended**: Vertex shader with instancing or per-mesh phase tracking.

### 5. Instancing & Placement
```rust
pub fn scatter_trees(
    commands: Commands,
    tree_materials: Res<TreeMaterials>,
    forest_obstacles: Query<&Obstacle>,  // From your obstacles system
) {
    for obstacle in forest_obstacles.iter()
        .filter(|o| o.kind == ObstacleKind::Tree || o.kind == ObstacleKind::Birch) {
        
        let variant = match obstacle.kind {
            ObstacleKind::Tree => TreeVariant::Tree,
            ObstacleKind::Birch => TreeVariant::Birch,
            _ => continue,
        };
        
        commands.spawn((
            Mesh3d(tree_meshes[variant].clone()),
            MeshMaterial3d(tree_materials.by_variant(variant).clone()),
            Transform {
                translation: Vec3::new(obstacle.x, obstacle.y, obstacle.z),
                rotation: Quat::from_rotation_y(obstacle.rot),
                scale: Vec3::splat(obstacle.scale),
            },
            TreeModel {
                variant,
                scale: obstacle.scale,
                tint: compute_tint(obstacle.x, obstacle.z),
            },
            if matches!(variant, TreeVariant::Tree | TreeVariant::Birch) {
                WindAffected
            } else {
                // Dead trees don't sway
                default()
            },
            GlobalTransform::default(),
        ));
    }
}

fn compute_tint(x: f32, z: f32) -> Color {
    let h = ((x * 12.9898 + z * 78.233) * 43758.5453).abs().fract();
    let v = 0.86 + h * 0.28;
    // Apply brightness multiplier to base green color
    Color::srgb(0.3 * v, 0.5 * v, 0.35 * v)  // Rough base green scaled
}
```

### 6. Wind Update System
```rust
pub fn update_wind(
    time: Res<Time>,
    mut wind_uniform: ResMut<WindUniform>,
) {
    wind_uniform.time = time.elapsed_secs();
    wind_uniform.strength = 1.0;  // Tunable
    wind_uniform.speed = 1.0;      // Tunable
}
```

### 7. Collision & Pathing Integration
- Store collision radius per tree variant (0.12, 0.10, 0.09)
- Use Bevy's built-in spatial query system or RAPIER for collision detection
- Same as obstacles.ts: 3×3 tile spatial hash for efficient queries

### 8. Scale & Rotation Per-Instance
- Use Transform for position, rotation (Y-only snap to cardinal), scale
- Deterministic placement: same RNG seed (2027) as THREE.js
- Implement the same `rng()` function in Bevy Rust for consistency

### 9. Key File Locations (Bevy Project Structure)
```
src/
  world/
    trees/
      mod.rs                  # Tree setup, materials, mesh building
      model.rs                # TreeVariant, TreeModel component
      placement.rs            # Scatter logic + RNG
      wind.rs                 # Wind shader & system
```

### 10. Notes on Bevy-Specific Differences
- **No per-instance color tinting natively** in StandardMaterial; must either:
  - Use a custom shader with vertex color attribute
  - Spawn individual meshes (less performant)
  - Use GPU instancing with color buffer
  
- **Icosphere detail**: Bevy's shape crate provides Icosphere; set detail=0 or detail=1
  
- **Cylinder tapering**: Bevy's Cylinder has radiusTop and radiusBottom natively
  
- **Shader compilation**: Use bevy::render::render_resource::ShaderStages to inject wind into vertex stage without full Material trait implementation
  
- **Wind phase per instance**: Store in a custom attribute buffer (or bake into vertex data if instances share exact geometry) or compute in shader from instance ID + world position hash

### 11. Performance Optimization
- Use one InstancedMesh per (mesh, material) pair if Bevy supports InstancedMesh
- Otherwise, use Transform hierarchy with bulk spawning
- Cull trees outside view frustum (Bevy's built-in frustum culling via Visibility)
- Wind shader adds ~20-30 ALU ops per vertex; negligible cost at modern fill rates
