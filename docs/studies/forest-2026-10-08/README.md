# Forest material review — natural v7

One forest sample for art approval, rendered by Bevy. Buildings, other biomes,
campaign rollout and performance evaluation remain outside this review.
The branch includes main `fb308c8` (vegetation repair and Royal Footman) through
merge `6e849d2`. These screenshots do not establish a full-game performance cost.

## Open the sample

```powershell
cargo build --release --locked
.\scripts\preview-forest-slice.ps1
```

The launcher isolates settings/save data and disables audio playback. It uses
Ultra by default; `-Quality high` uses the lighter preset. Only
`FOREST_FORESTSLICE=1` is needed. The older whole-world/tree switches are separate.
Without preview switches the campaign uses its normal assets and lighting.

Current slice defaults: EV 9.6, sky brightness 1200, ambient multiplier 2.2,
IBL multiplier 2.5, midtone contrast 1.2, saturation 1.0, Tony McMapface tone mapping,
neutral daylight and TAA. These are experimental art settings for this sample.

## Changes since the pastel version

- All 88 simpler background trees use a distance version of the scanned tree,
  giving 129 CC0 trees in a 3,433-instance composition. Positions, path and both
  review cameras are unchanged.
- All 1,601 flat authored grass clumps and 146 oversized white/yellow flowers
  now use scanned grass/flower meshes. Positions and yaw are preserved, with
  grass heights of 0.28–0.43m and smaller flowers. The before-layout snapshot and
  exact mapping are in `art/forest_preview/scanned_groundcover_mapping.json`.
- The path uses real diffuse, OpenGL normal and roughness maps from
  [Poly Haven Stony Dirt Path](https://polyhaven.com/a/stony_dirt_path), CC0.
  Blender prepares seamless 512px maps with local rust-fleck chroma reduction.
  The slice repeats them at 1.79 world units; campaign repeat is unchanged.
  Graphics-quality updates now preserve the material's slice flag: previously
  they reset it, silently restoring the old 7.58-unit path scale and blend rules.
- The scanned trunk receives rougher bark and a gentler baked normal texture.
  The change is stored in Blender sources and exported textures: Bevy 0.19 ignores
  the glTF normal-strength scalar, so that scalar alone cannot implement it.
- Imported glTF textures now receive actual mip levels at load time. Albedo is
  filtered in linear light; cutout alpha coverage is preserved per level and
  normals are renormalized. TAA still helps the thin geometry edges.
- The visible HDR sky also supplies filtered environment lighting. The former
  version used a separate gradient probe. Exposure and color grading were retuned.
- DoF now reads depth by UV proportion at its center and every gather tap. Previous
  pixel coordinates were wrong when Ultra's depth and post-process textures had
  different resolutions. This coordinate correction also applies to the campaign;
  its art settings remain unchanged.

Initial brighter/darker candidates were diagnostic captures. The old NOBLUR
diagnostic overlapped a path texture update and cannot establish an isolated
visual effect from DoF alone.

## Editable source and provenance

- [Blender scene](../../../art/forest_preview/forest_scene.blend) and
  [runtime layout](../../../assets/models/forest_slice/layout.json).
- [Authoring/reproduction notes](../../../art/forest_preview/README.md).
- [Tree manifest](../../../art/forest_preview/cc0_tree_optimization.json),
  [source audit](../../../art/forest_preview/final_scene_audit.json),
  [GLB validation](../../../art/forest_preview/cc0_validation.json).
- [Path inputs/hashes](../../../art/forest_preview/path_report.json).

Tree Small 02, Island Tree 01, scanned understory, stones and stump are Poly Haven
CC0 assets adapted/exported through Blender MCP. Three packed tree libraries use
relative scene links. Eleven runtime GLBs embed PNG maps and masked leaf alpha.
Exact source URLs, licenses, adaptations and hashes are in the linked records.

The sky is [Kloofendal 48d Partly Cloudy (Pure Sky)](https://polyhaven.com/a/kloofendal_48d_partly_cloudy_puresky)
by Greg Zaal and Jarod Guest, CC0. `bake_sky.py` reprojects the preserved 2K HDR
panorama to a 512px RGBA16Float cubemap; `sky_report.json` records the conversion.
Blender's viewport does not reproduce the engine's shaders and post-processing.

## Review evidence

- [Current overview](natural-v7/overview.png) / [record](natural-v7/overview.json).
- [Current detail](natural-v7/detail.png) / [record](natural-v7/detail.json).
- [Previous overview, same camera](natural-v7/before.png), from commit `9711808`.
- [Current validation](natural-v7/review-validation.json) /
  [release build](natural-v7/build.txt).

Top-level `overview.png`, `detail.png`, their records, `build-v6.txt` and
`review-validation.json` are preserved historical v6 evidence. They do not
describe current materials. Images are unmodified engine PNGs, not generated
concept art or edited screenshots.

High/Ultra disable SSAO and retain bloom, contact shadows, DoF and atmosphere.
Ultra renders the 3D view at twice the output resolution. Diagnostic controls:
`FOREST_SLICE_TAA=0`, `FOREST_SLICE_MIPS=0`,
`FOREST_SLICE_CONTACT_SHADOWS=0`, `FOREST_SLICE_SUN_WARM=1`,
`FOREST_SLICE_HDR_IBL=0`. Ordinary campaign/WORLD keep their AA and lighting.

The harness verifies silent playback, a new PNG save, normal exit and no loading
or rendering errors. Independently validate exports with
`python3.12 tools/validate_forest_cc0.py`.

Core tests after the main merge passed (325 unit, one integration). This pass
does not change core gameplay. Art approval, camera-motion quality and performance
acceptance remain pending. The 2,559 scanned grass patches represent about 15.6M
triangles before culling, substantially more than the former simple clumps. This
is an art prototype, not a production cost acceptance or a performance comparison.
Repeated distant trees, airy crowns, pale bark grooves and remaining flat foliage
still differ from the reference. This is a bounded iteration for review, not a
visual-match claim.
