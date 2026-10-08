# Forest approval slice

This is one authored forest composition, centred at world XZ `(500, 500)`.
The detailed foreground occupies about 48 × 48 units; three rows of lighter
background trees and the terrain extend to 120 × 120. The editable source is
`forest_scene.blend`; runtime transforms and two camera poses are in
`../../assets/models/forest_slice/layout.json`. Its 3,433 instances include
41 optimized Poly Haven CC0 trees and 1,255 scanned CC0 understory instances;
the frozen five-tree study kit appears only deeper in the backdrop. The ground
renderer applies the same six-point S-bend and `slice_height` function as
`build_scene.py`. The Blender source path ribbon is an editing guide; the Bevy
ground and softened path mask are authoritative for visual review.

`tree_atlas_forest.png` is a **slice-only** green grade of the frozen
`assets/models/blender_trees/tree_atlas.png`. `bake_tree_atlas.py` leaves its
alpha and bark quadrant byte-identical. It does not alter the tree study assets
or campaign tree material. `ground_path_{albedo,normal,roughness}.png` is also
slice-only: `bake_path.py` imports the preserved OpenAI imagegen
`art/blender_environment/imagegen/path-albedo-v3.png`, resizes and seam-matches
it, then bakes approximate normal/roughness. The modest albedo exposure
adjustment is recorded in `path_report.json`; the input prompt and provenance
are in `art/blender_environment/imagegen/path-v3-generation.md`.

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
All nine original 1K PBR images per tree are embedded, including distinct
normal/roughness maps; the official separate leaf alpha is merged with the
unchanged diffuse RGB and exported as `MASK` (cutoff 0.45). The island tree
needed deterministic fallback tangents at 404 decimated branch vertices;
`repair_cc0_tangents.py` changes only those tangent values after Blender GLB
export. Blender 4.5's glTF exporter retains JPEG source maps, whereas this
Bevy build has only its PNG decoder enabled. `tools/transcode_forest_glb_textures.py`
re-encodes embedded JPEGs to PNG with exact decoded-pixel equality and
unchanged geometry; `cc0_png_repack.json` records every before/after hash.
Both editable optimized `.blend` libraries pack every used texture
and are linked by relative paths from `forest_scene.blend`, which stays under
100 MB rather than duplicating high-detail meshes for each instance.

The same scene imports Poly Haven CC0 grass, shrub, wildflower, mossy rock,
and stump GLBs prepared in a separate Blender MCP process. Exact source URLs,
license and hashes are in `cc0_understory_downloads.json`; export measurements
are in `cc0_understory_exports.json`. The scene uses 958 scanned grass patches,
including 108 added in twelve irregular near-path clusters, plus broader
low-cost Blender grass clumps farther away. It adds 96 small white, yellow and
purple flower patches in those clusters. Twelve trees replace bright legacy
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
   `apply_final_forest_tune.py`. Open `forest_scene.blend` in Blender MCP and
   run `sync_cc0_tree_scene.py`, `sync_cc0_understory_scene.py`, and
   `audit_final_scene.py`.

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
