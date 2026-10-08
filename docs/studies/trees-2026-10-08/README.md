# Warbell: first Blender tree study

This is a historical measurement on main `90f8d1b`, before the sparse-vegetation
fix in `fb308c8` (v0.24.1). That old build incorrectly let soft undergrowth exclude
later trees. These timings describe that sparse layout and must not be used as
the performance verdict for the repaired campaign or the new forest prototype.

The Blender MCP pipeline works end to end, but this first tree set exceeded its
predeclared 10% GPU-cost budget. It remains optional. Current work is limited to
one forest art slice for visual approval; broader environment rollout and new
campaign performance evaluation are deferred until the art direction is accepted.

Open [the visual report](index.html) for three real-game before/after comparisons,
repeat ranges, p99, memory and raw measurements. These images were inspected after
capture; none of the six selected images has the cold-pipeline grey/blank failure.

Source: main `90f8d1b1b4b780b481046e76a82c3d931b64d60b`, with the tree import and
measurement corrections preserved in commit `ef680bcf8665b7b69e2689d55dec3d6570d8d2e8`.
Final A/B binary SHA-256:
`fe92afa63846d07dfe07148fea0dee2bc61d651491015908d6a2fd16f637cced`.
The later environment work must not be treated as the measured tree-only version.

On the local i5-12400F / RTX 5060 Ti 16 GB, driver 616.64, 1920×1080 High:

| Scene | Mean frame ms, original → Blender | p95 ms | GPU pass sum ms | GPU change |
|---|---:|---:|---:|---:|
| Close forest | 8.12 → 10.94 | 12.05 → 19.01 | 3.95 → 4.64 | +17.4% |
| Wider forest | 8.15 → 8.33 | 12.11 → 12.88 | 4.37 → 5.04 | +15.2% |
| 96-enemy siege | 11.51 → 11.63 | 16.89 → 17.62 | 3.57 → 3.65 | +2.2% |

Values are medians of three runs per mode. Close-forest frame times vary strongly:
original 7.98–10.41 ms, Blender 8.31–11.38 ms. The median FPS difference therefore
does not isolate the effect of the trees from the remaining system variability.
GPU times are much steadier, with the extra cost primarily in the main opaque
render pass. Overlapping alpha cards are a plausible cause, not yet a separately
isolated experiment. GPU pass sum is a diagnostic proxy, not an exclusive GPU
frame duration. The additional Low pair shows +13.7% GPU cost; one pair is exploratory.

The original unattended harness limited an unfocused window to 60 Hz independently
of VSync. Those initial results were excluded. The final runs use the same release
binary with trees off/on, continuous window scheduling, fixed lighting and active
simulation, with 15 seconds after WorldReady excluded. No runs were removed for
being slow. See [the full protocol](../../tree-study-protocol.md).

Five source models (two oaks, two birches, one pine) replace 118 campaign trees:
112 forest and six meadow. This does not represent a dense forest with thousands
of visible trees. Geometry and branches read more naturally nearby, especially
the pine; distant crowns are thinner, and birches still need palette/density work.
The existing faceted ground and shrubs make that visual mismatch more obvious.

Each model has 1,170–1,668 triangles. Runtime JSON plus the shared 1024² atlas total
2,587,590 bytes. Editable Blender source, a recorded MCP execution trail and matching
GLBs are included. Khronos validation: five GLBs, zero errors and zero warnings.
Release build and 325 core unit tests plus one integration test passed. The GLB,
JSON, normals, UVs, colors and texture parity were independently checked.

Suggested next optimization: reduce alpha-card overlap and preserve canopy mass
with distance LOD; then repeat the same cameras and settings. These data do not
establish performance on an integrated GPU or long-session stability. RTS was
excluded as requested.
