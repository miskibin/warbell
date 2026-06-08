# Add-ons contract (water / wind / distant / decor)

You add ONE self-contained Bevy 0.18.1 module to the existing static forest scene. The
scene already has: camera (HDR, AgX, bloom, Bokeh DoF, SSAO, SMAA, IBL, `Atmosphere`
procedural sky, `DistanceFog`), a smooth vision-shader ground, flat-shaded low-poly
trees/bushes/rocks, dense ground cover, and a fly camera. You ADD to it.

## Hard rules
1. **Edit ONLY your assigned new file(s)** (`src/<mod>.rs` + any `assets/shaders/<x>.wgsl`).
   Do NOT touch `main.rs`, `scene.rs`, `terrain.rs`, `scatter.rs`, `Cargo.toml`, or
   another agent's module. The wiring (adding your plugin to `main.rs`, scatter hooks) is
   done by the integrator AFTER you finish.
2. **Do NOT run `cargo build`/`check`/`run`** (shared target dir). Self-review your Rust
   against the verified API doc + the real Bevy source.
3. Read the **real Bevy 0.18.1 source** for exact APIs — grep under
   `C:\Users\skibi\.cargo\registry\src\index.crates.io-*\bevy_*-0.18.1\src` (e.g.
   `bevy_pbr`, `bevy_render`, `bevy_light`, `bevy_mesh`). Also read
   `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md` (verified API doc),
   `assets/shaders/terrain.wgsl` (working `ExtendedMaterial` fragment + `ter_noise`
   pattern), `src/terrain.rs` (how the material is built/registered), `src/scene.rs`
   (component patterns), `src/palette.rs`, `src/trees.rs` (mesh build + flat shading).
4. Each module is a `pub struct <Name>Plugin` implementing `Plugin`, doing all its setup
   in `Startup` (+ `Update` for animation). Self-contained.
5. Meshes: base at y=0, colour via mesh `ATTRIBUTE_COLOR` (linear, `crate::palette::lin`),
   built + merged like `trees.rs` (`tinted` + `Mesh::merge`). Flat-shade low-poly props
   (`duplicate_vertices()` then `compute_flat_normals()`) for the crisp facet look.
6. The scene is centred on the origin; the populated forest spans `[-16, 16]` in X/Z
   (`terrain::HALF` = 16). Place content within ~`[-16,16]` unless it's distant scenery.

## Animation in shaders
Time in a Material WGSL: `#import bevy_pbr::mesh_view_bindings::globals` then use
`globals.time` (seconds). See how `terrain.wgsl` imports `bevy_pbr::...`. For CPU
animation use `Res<Time>` + `time.elapsed_secs()`.

## Coordinate / integration hooks the integrator will use
- Your plugin name (e.g. `WaterPlugin`) — list it in your report so it gets added to
  `main.rs`.
- `water.rs` MUST expose `pub fn on_river(x: f32, z: f32) -> bool` (true where the river
  surface is) and `pub fn river_bank_t(x: f32, z: f32) -> f32` (0 at centerline → 1 a few
  units past the bank) so the scatter + decor can avoid the water and dress the banks.
- `wind.rs` MUST expose `pub struct Sway { pub phase: f32, pub base: Quat }` (a Component)
  — the scatter will insert it on tree entities; your `Update` system animates every
  entity that has it (lean it from `globals`/`Time`, pivoting at the y=0 base).
