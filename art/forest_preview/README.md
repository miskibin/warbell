# Forest approval slice

This is one authored forest composition, centred at world XZ `(500, 500)`.
The detailed foreground occupies about 48 × 48 units; three rows of lighter
background trees and the terrain extend to 120 × 120. The editable source is
`forest_scene.blend`; runtime transforms and two camera poses are in
`../../assets/models/forest_slice/layout.json`. Its 3,433 instances include
129 optimized Poly Haven CC0 trees and 1,255 scanned CC0 understory instances.
The frozen five-tree study kit remains in the repository, but its pastel
silhouettes are no longer used in this approval slice. The ground
renderer applies the same six-point S-bend and `slice_height` function as
`build_scene.py`. The Blender source path ribbon is an editing guide; the Bevy
ground and softened path mask are authoritative for visual review.

`tree_atlas_forest.png` is a **slice-only** green grade of the frozen
`assets/models/blender_trees/tree_atlas.png`. `bake_tree_atlas.py` leaves its
alpha and bark quadrant byte-identical. It does not alter the tree study assets
or campaign tree material. `ground_path_{albedo,normal,roughness}.png` is also
slice-only: `bake_path.py` imports the preserved [Poly Haven stony dirt path](https://polyhaven.com/a/stony_dirt_path)
CC0 scan, including its OpenGL normal and roughness channels. Blender resizes
the aligned 1k sources to 512 pixels, closes a narrow tile seam, reduces the
scan's saturated rust-orange mineral flakes and lifts the albedo equally across
RGB. The slice repeats this fine gravel at about 1.8 world units per tile; the
input and output hashes are in `path_report.json`, and the verified Blender MCP
call/source hash is in `path_mcp_provenance.json`;
the source PNGs and download checksums are under `art/blender_environment/polyhaven/`.

The grass-card atlas cell uses a cropped Diffuse + Alpha tussock from Poly
Haven's [grass_medium_01](https://polyhaven.com/a/grass_medium_01), CC0. Exact
download URLs and hashes are in
`art/blender_environment/polyhaven/grass_medium_01/downloads.json`. Other
environment texture sources and their credits are in
`art/blender_environment/README.md`. The GLB export explicitly selects named
`Color` vertex attributes; `validate_kit.py` checks distinct yellow, white and
purple petal palettes as well as GLB/JSON parity, preventing the Blender
exporter's silent white fallback.

The slice-only `hero_forest_kit.glb` and twelve matching JSON meshes are
authored in `hero_kit.py`: two earlier detailed oaks (retained as reproducible
alternatives), fine-blade grasses, leafy shrubs, three modeled wildflower
colours, and low road pebble clusters. All use one 2048-pixel
`hero_tree_{atlas,normal,orm}.png` PBR triplet.
The oak trunk and flared roots use Poly Haven's
[Jolcham oak bark](https://polyhaven.com/a/jolcham_oak_bark_01) scan by Charlotte
Baglioni (CC0); source URLs and checksums are in
`polyhaven/jolcham_oak_bark_01/downloads.json`. Its 1024 × 2048 scan keeps its
1:2 proportions across two vertical atlas cells. Small oak-leaf sprays come
from the project's MIT `assets/textures/leaves/oakL.png` and retain source
alpha. The sixteen atlas cells are mip-isolated. The hero GLB exports named
vertex colors and tangents; `validate_hero.py` checks all attributes, the
embedded texture hashes and source/output hashes independently.

The foreground review now uses **actual imported Poly Haven CC0 models**:
[tree_small_02](https://polyhaven.com/a/tree_small_02) (2,062,487 original →
164,608 triangles, 8.0 m tall, 38 instances) and
[island_tree_01](https://polyhaven.com/a/island_tree_01) (1,599,403 original →
439,378 triangles, 8.7 m tall, three near-frame anchors). Blender MCP
separates their original trunk/branch/leaf materials, simplifies geometry,
and exports self-contained GLBs under `assets/models/forest_slice/cc0/`.
Nine 1K PBR images per tree are embedded, including distinct trunk, branch
and leaf normal/roughness maps; the official separate leaf alpha is merged
with the unchanged diffuse RGB and exported as `MASK` (cutoff 0.45). The
two small-tree exports use derived trunk maps for the final matte-bark review:
roughness G is multiplied by 1.25 and clamped (median 0.690 to 0.863),
while tangent-normal XY is scaled by 0.6 and renormalized. AO R, metallic B,
trunk diffuse, and all branch/leaf maps retain their decoded source pixels.
The original Poly Haven maps remain unchanged in the verified source cache;
derived PNGs and their recipe scripts are in `cc0_prepared/` and this folder.
The normal change is baked into pixels because Bevy 0.19.1 currently ignores
glTF `normalTexture.scale`; Blender Normal Map Strength and exported scale are
both 1.0. `matte_bark_glb_audit.json` compares both final GLBs against the
pre-correction exports and confirms identical positions, normals, UVs and
indices. The island tree
needed deterministic fallback tangents at 404 decimated branch vertices;
`repair_cc0_tangents.py` changes only those tangent values after Blender GLB
export. Blender 4.5's glTF exporter retains JPEG source maps, whereas this
Bevy build has only its PNG decoder enabled. `tools/transcode_forest_glb_textures.py`
re-encodes embedded JPEGs to PNG with exact decoded-pixel equality and
unchanged geometry; `cc0_png_repack.json` records every before/after hash.
All three editable tree `.blend` libraries pack every used texture
and are linked by relative paths from `forest_scene.blend`, which stays under
100 MB rather than duplicating high-detail meshes for each instance.

The remaining 88 oak, birch and pine study trees in the v6 backdrop were
replaced by `tree_small_02_backdrop.glb`, a 71,283-triangle distance LOD of
the same scanned CC0 tree (down from the 164,608-triangle foreground export).
Blender MCP saved `tree_small_02_backdrop.blend` with the corrected matte bark,
original branch and leaf PBR materials, packed 1K maps and real cutout alpha.
One degenerate tangent was repaired; six embedded JPEGs were re-encoded as PNG
without changing decoded pixels or geometry. `apply_cc0_backdrop_layout.py`
keeps all 88 coordinates, rotations, camera and path, matches each earlier
tree's approximate canopy width, and sinks the scan rim by 14 cm.

The same scene imports Poly Haven CC0 grass, shrub, wildflower, mossy rock,
and stump GLBs prepared in a separate Blender MCP process. Exact source URLs,
license and hashes are in `cc0_understory_downloads.json`; export measurements
are in `cc0_understory_exports.json`. The final review replaces all 1,601
rigid authored grass clumps with the existing scanned grass patches, for 2,559
scanned grass instances at 0.28–0.43 m height. It replaces all 146 oversized
authored white/yellow flower heads with 101 scanned white heliophila patches
(0.31–0.41 m) and 45 small yellow dandelion patches (0.15–0.23 m); 32 authored
purple accents remain. `layout_v7_before_scanned_groundcover.json` preserves
the prior layout, and `scanned_groundcover_mapping.json` records every changed
row with unchanged position and yaw. This increases scanned grass geometry and
needs a separate runtime performance measurement. Twelve trees replace bright legacy
crowns in the central background. Forty-one CC0 tree bases sit 14 cm lower to
conceal the flat scan skirt, and the review camera is 1.85 m high.
`apply_final_forest_tune.py` derives this bounded composition from the tracked
v5 layout snapshot, so reruns cannot accumulate extra plants. Scanned shrubs
and flowers are varied with the original authored species. Source glTF
dependencies live in ignored
`target/world-preview/cc0-sources/` and are re-downloadable from tracked
`polyhaven_api/*-files.json` and the download manifests.

To reproduce with Blender 4.5.9 and the official `mcp-for-blender` bridge
connected to the isolated add-on on port 9877, run these scripts from the repo
root through `tools/blender_environment/mcp_client.py` in order:

1. `tools/blender_environment/make_atlases.py`
2. `art/forest_preview/bake_tree_atlas.py`
3. `art/forest_preview/bake_path.py`
4. `art/forest_preview/bake_hero_atlas.py`
5. `art/forest_preview/hero_kit.py`
6. `tools/blender_environment/build_environment.py`
7. `art/forest_preview/build_scene.py` (uses kit objects left in Blender)

Then prepare the imported CC0 foreground (normal Python for download/alpha,
official Blender MCP for model import and export):

1. `download_cc0.py tree_small_02` and `download_cc0.py island_tree_01`
2. `prepare_cc0_leaf_alpha.py tree_small_02` and the same for `island_tree_01`
3. Through Blender MCP on port 9877: `preview_cc0.py`, `separate_cc0.py`,
   `optimize_cc0_tree.py`; then `preview_cc0_island.py` and
   `optimize_cc0_island.py`
4. `repair_cc0_tangents.py island_tree_01_optimized`
5. Through Blender MCP: `pack_cc0_blend_sources.py`
6. For understory: `download_cc0_understory.py`,
   `cc0_understory_make_rgba.py`, then `cc0_understory_build.py` through its
   isolated Blender MCP on port 9878
7. `tools/transcode_forest_glb_textures.py` (normal Python; converts all ten
   embedded PBR texture sets to runtime-compatible PNG without pixel changes),
   then `cc0_understory_finalize.py` to refresh the understory export hashes
8. `apply_cc0_layout.py`, `apply_cc0_understory_layout.py`, then
   `apply_final_forest_tune.py`. Open `tree_small_02_optimized.blend` in Blender
   MCP and run `author_cc0_backdrop_lod.py`, then `export_cc0_backdrop_lod.py`.
   Run `repair_cc0_tangents.py tree_small_02_backdrop` and
   `repack_cc0_backdrop_lod.py` with normal Python. For the final matte-bark
   variant, run `make_matte_bark_arm.py` and `make_soft_bark_normal.py` with
   normal Python. Open each of `tree_small_02_optimized.blend` and
   `tree_small_02_backdrop.blend` through Blender MCP; run
   `apply_matte_bark_blend.py`, then `apply_soft_bark_normal_blend.py`, then
   `export_matte_bark_tree.py`. Run `repair_cc0_tangents.py` for each export,
   followed by `repack_matte_bark_trees.py` and
   `audit_matte_bark_glbs.py` with normal Python.
9. Run `apply_cc0_backdrop_layout.py`, then
   `replace_legacy_groundcover.py` with normal Python. Open
   `forest_scene.blend` in Blender MCP and run `sync_cc0_tree_scene.py`,
   `sync_cc0_understory_scene.py`, and
   `resave_matte_bark_scene.py`, then `audit_final_scene.py`.

The portable Blender 4.5.9 Windows x64 ZIP is listed in the
[Blender Foundation release index](https://download.blender.org/release/Blender4.5/);
its official ZIP SHA-256 is
`41da973b9bf95bb312cbeff4d1982feb13259b43c821686b9bafea4dfe5477cf`.
The `mcp-for-blender==2.1.9` Python package bundles the matching add-on as
`blender_mcp/bundled/addon.py`. Restored private tools and source downloads are
kept under ignored `target/`; packed `.blend` libraries are tracked here.

Then run `tools/blender_environment/validate_kit.py`,
`tools/blender_environment/audit_refs.py`, and
`art/forest_preview/validate_hero.py` with normal Python. MCP call/source
hash evidence is appended to `tools/blender_environment/mcp_calls.jsonl`; the
current output hashes and scene counts are in `tree_atlas_report.json`,
`path_report.json`, `hero_atlas_report.json`, `hero_manifest.json` and
`scene_report.json`. Independently run `tools/validate_forest_cc0.py` to check
every `gltf:` runtime ID, actual alpha in `MASK` images, embedded PBR images,
mesh/index bounds, normals and tangents. Final source/output hashes are in
`cc0_tree_optimization.json`. `final_scene_audit.json` confirms that the
saved `.blend` matches all 3,433 runtime rows in position, scale and yaw and
links its tree libraries through relative paths.

The preserved MCP call ledgers contain historical absolute Windows paths and
earlier tool errors. These are provenance records, not runtime dependencies;
use the repository-relative reproduction steps above on a fresh machine.

The scene is a visual direction prototype. Review the actual Bevy screenshots
rather than the Blender viewport for final lighting and path appearance.
