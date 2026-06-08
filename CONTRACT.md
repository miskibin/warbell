# Model-builder contract (Bevy 0.18.1)

You implement ONE leaf module that builds 3D meshes for the forest scene. The scene,
terrain, lighting, post-processing and scatter/placement are already done — you ONLY
build meshes. Match the TS game's look exactly (or better).

## Hard rules

1. **Visual values** (geometry dims, colours, layer offsets, counts) come from the TS
   spec docs in `docs/specs/`. Those are the source of truth for the LOOK.
2. **Rust / Bevy 0.18 API** comes from this contract +
   `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md` (verified against
   a real compiling 0.18.1 project). The per-slice spec docs sometimes GUESS the Bevy
   API (e.g. `Color::hex`, `shape::Cylinder`, "Mesh has no merge") — those guesses are
   WRONG for 0.18; ignore them and use the verified forms below.
3. **Edit ONLY your assigned file.** Do not touch `main.rs`, `scatter.rs`, `scene.rs`,
   `terrain.rs`, `Cargo.toml`, or another agent's module.
4. **Do NOT run `cargo build`/`cargo run`/`cargo check`.** The target dir is shared and
   a build is run after all agents finish. Self-review your Rust against this contract.
5. Keep the file's existing public function SIGNATURES exactly (the scatter calls them).
   You may add private helpers and extra public consts.

## Coordinate / placement rules

- Each mesh's BASE sits at **y = 0** (trunk bottom / rock bottom on the ground). The
  scatter applies translation, Y-rotation and uniform scale per instance.
- Authoring scale = TS units (1 tile = 1 world unit). A TS tree is ~1.5 units tall; the
  scatter scales trees up. Build at TS proportions; don't pre-scale.
- All meshes render against ONE shared white `StandardMaterial`, so **colour lives in
  the mesh `ATTRIBUTE_COLOR`** (linear RGBA). Use `crate::palette::lin(0xRRGGBB)` for a
  flat part colour and `crate::palette::lin_scaled(hex, v)` to brighten/darken.

## The mesh-building pattern (verified 0.18 API)

```rust
use bevy::prelude::*;
use bevy::mesh::Mesh;            // primitives: Cylinder, Sphere, Cuboid, Cone, etc.

// A primitive → Mesh, positioned/rotated in the model's local space:
let trunk: Mesh = Cylinder::new(radius, height).mesh().resolution(6).build()
    .translated_by(Vec3::new(0.0, 0.25, 0.0));      // consumes + returns Mesh
let canopy: Mesh = Sphere::new(0.46).mesh().ico(1).unwrap()   // ico() returns Result
    .translated_by(Vec3::new(0.0, 0.64, 0.0));

// Tag a part with a flat colour (REQUIRED before merge — all parts need ATTRIBUTE_COLOR):
fn tinted(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}

// Merge parts sharing the same attributes into ONE mesh (so batching holds):
let mut mesh = tinted(trunk, lin(TREE_TRUNK));
mesh.merge(&tinted(canopy, lin(FOLIAGE_DARK))).unwrap();   // Mesh::merge -> Result in 0.18
// ...merge the rest...
mesh                                                       // return it
```

Key facts:
- `Cylinder::new(radius, height)` — a SINGLE radius (no separate top/bottom). For a
  tapered trunk approximate with the average radius, or stack/scale. `.mesh().resolution(n)`
  sets radial segments. `Cone { radius, height }` exists. `Sphere::new(r).mesh().ico(detail).unwrap()`
  (detail 0/1) gives the faceted icosphere look; `.uv(lon,lat)` is the smooth option.
- A primitive mesh has POSITION/NORMAL/UV_0. After merging, all parts must carry the
  same attribute set — so `tinted()` EVERY part (add COLOR) before merging, and don't mix
  parts that have UV with parts that don't (primitives all have UV_0, so fine).
- `Mesh::translated_by(self, Vec3) -> Mesh` and `Mesh::rotated_by(self, Quat) -> Mesh` and
  `Mesh::scaled_by(self, Vec3) -> Mesh` transform a mesh's vertices (consuming form).
- `Mesh::merge(&mut self, &Mesh) -> Result<(), _>` — `.unwrap()` it.
- For a faceted/low-poly flat-shaded look like the TS icospheres, you can leave the
  smooth normals; the scene's SSAO + directional light already read the facets. If you
  want hard facets: `mesh.duplicate_vertices(); mesh.compute_flat_normals();` (duplicate
  FIRST — compute_flat_normals panics on an indexed mesh).

## Module assignments

### `src/trees.rs`
- `pub enum TreeKind { Broadleaf, Birch, Dead }`
- `pub fn build_tree_mesh(kind: TreeKind) -> Mesh`
- Build all three TS variants (broadleaf = 6 icosphere foliage layers over a tapered
  trunk; birch = pale trunk + 4 rounder foliage; dead = bare trunk + 4 angled branch
  cylinders). EXACT dims/positions/colours in
  `docs/specs/forest-tree-models-exact-bevy-rebuild-recipe.md`. Palette consts already in
  `crate::palette` (TREE_TRUNK, FOLIAGE_DARK/MID/LIGHT, BIRCH_*, DEAD_WOOD*). Use the
  three-tone layered greens so the canopy reads lush, not flat.

### `src/props.rs`
- `pub const NUM_ROCK_VARIANTS: u32;` `pub const NUM_BUSH_VARIANTS: u32;`
- `pub fn build_rock_mesh(variant: u32) -> Mesh` — low-poly boulders (deformed
  icospheres / clustered boxes), mossy grey-green. A few distinct shapes by `variant`.
- `pub fn build_bush_mesh(variant: u32) -> Mesh` — clustered green spheres (2–4 blobs),
  layered light/dark greens like the tree foliage but shorter/rounder.
- Exact specs: `docs/specs/forest-biome-props-ground-cover-exact-bevy-rebuild.md`.

### `src/groundcover.rs`
- `pub fn build_grass_tuft_mesh() -> Mesh` — a few crossed/angled thin blades (tapered
  boxes or thin cones), green with a lighter tip.
- `pub fn build_fern_mesh() -> Mesh` — a low spray of angled fronds, deeper green.
- `pub fn build_mushroom_mesh(variant: u32) -> Mesh` — red-cap amanita: white stem
  (small cylinder) + red dome cap (half-sphere / cone) + optional white speckles.
- `pub fn build_flower_mesh(variant: u32) -> Mesh` — thin green stem + a small bright
  petal head (pink / yellow / white by variant).
- `pub fn build_clover_mesh() -> Mesh` — a tiny tri-leaf clump, low to the ground.
- These are SMALL (mushroom ~0.15u tall, tuft ~0.2u, flower ~0.2u). Exact specs in the
  props/ground-cover doc.

### `src/ruins.rs`
- `pub fn build_trilithon_mesh() -> Mesh` — two upright weathered standing stones with a
  lintel across the top (a Stonehenge-style trilithon), mottled grey, ~3u tall. This is
  the background landmark in the TS forest screenshot.
- `pub fn build_giant_dead_tree_mesh() -> Mesh` — a tall bare gnarled dead tree (~5u)
  with a few thick angled branches, weathered grey-brown.
- Make them read as believable silhouettes against the fog. Use `crate::palette::lin`.

## Palette (already defined in `src/palette.rs`)
`lin(hex)`, `lin_scaled(hex, v)`, `srgb(hex)`, and consts: FOREST_GROUND, TREE_TRUNK,
FOLIAGE_DARK, FOLIAGE_MID, FOLIAGE_LIGHT, BIRCH_TRUNK, BIRCH_MARK, BIRCH_DARK,
BIRCH_LIGHT, DEAD_WOOD, DEAD_WOOD_DARK. Add your own local colour consts as needed.
