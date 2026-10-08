# Forest art approval slice

This is one isolated forest composition for visual review. Buildings, other biomes,
campaign rollout and performance evaluation are paused until its art direction is
accepted. It is a prototype, not a claim that the full game has been rebuilt.

The branch includes main `fb308c8` through merge `6e849d2`, including the vegetation
density fix and Royal Footman. The older tree study measured an earlier sparse
campaign baseline and cannot predict the cost of this composition.

## Open the prototype

```powershell
cargo build --release --locked
.\scripts\preview-forest-slice.ps1
```

The launcher uses a separate settings/save directory and disables audio playback.
It defaults to the reviewed Ultra preset, EV 9.55 and sky brightness 1050;
`-Quality high` keeps that lighting at the lighter preset. Starting the slice by
its environment flag alone uses the earlier EV 10.35 / sky 1800 defaults.
Only `FOREST_FORESTSLICE=1` is needed; do not combine it with the older whole-world
or tree replacement switches. Without preview switches the normal campaign keeps
its own environment.

## Authored scene and renderer

- [Editable Blender scene](../../../art/forest_preview/forest_scene.blend).
- [Blender authoring script](../../../art/forest_preview/build_scene.py).
- [Runtime layout](../../../assets/models/forest_slice/layout.json).
- [Scene export report](../../../art/forest_preview/scene_report.json).

The placement and model source are authored through Blender MCP. Bevy renders the
exported meshes and textures with its real PBR lighting, shadows, tone mapping
and post-processing. The daylight/sky settings are experimental settings for this
slice. The screenshots are actual engine captures, not generated concept images.
The Blender file is editable source; its viewport does not reproduce Bevy shaders.

The foreground and middle distance use two Poly Haven tree models adapted through
Blender MCP: a slender branching tree and a fuller, twisted tree. Their separate
1K PBR materials and leaf masks survive export; they are not reduced to the old
shared atlas. The original tree-study models and atlas stay unchanged and provide
the deeper backdrop. Scanned grass, shrubs, white/yellow flowers, mossy rocks and
a stump are combined with the authored ferns, purple flowers, mushrooms and deadwood.
Earlier architecture assets remain parked in the source kit and are not loaded by
the slice.

The imported source assets are [Tree Small 02](https://polyhaven.com/a/tree_small_02),
[Island Tree 01](https://polyhaven.com/a/island_tree_01),
[Shrub 03](https://polyhaven.com/a/shrub_03),
[Dandelion 01](https://polyhaven.com/a/dandelion_01),
[Flower Heliophila](https://polyhaven.com/a/flower_heliophila),
[Rock Moss Set 01](https://polyhaven.com/a/rock_moss_set_01) and
[Tree Stump 01](https://polyhaven.com/a/tree_stump_01), all CC0.
Exact download URLs, hashes, adaptations and export counts live under
`art/forest_preview/cc0_*`. The standalone GLBs embed their textures and use
depth-tested leaf masks. `tools/validate_forest_cc0.py` independently checks buffer
bounds, indices, normals, tangents, embedded images and actual masked alpha.

Ground inputs include [Poly Haven leafy_grass](https://polyhaven.com/a/leafy_grass)
and [grass_medium_01](https://polyhaven.com/a/grass_medium_01), provided under
[CC0](https://polyhaven.com/license). The warm path uses a generated albedo input;
its exact prompt and generation record are in
[path-v3-generation.md](../../../art/blender_environment/imagegen/path-v3-generation.md).
Its derived normal/roughness maps are approximations, not measured scan channels.

The visible sky is [Kloofendal 48d Partly Cloudy (Pure Sky)](https://polyhaven.com/a/kloofendal_48d_partly_cloudy_puresky)
by Greg Zaal and Jarod Guest (CC0). `art/forest_preview/bake_sky.py` reprojects the
preserved 2K HDR panorama into a 512px RGBA16Float cubemap, aligns its sun azimuth,
and normalizes radiance. It runs with ordinary Python plus NumPy/OpenCV, outside
Blender; `sky_report.json` records its input/output hashes and exact conversion.
The screenshot is still a live 3D render: the panorama supplies only the sky.
The indirect environment lighting still uses the game's separate gradient probe;
it is not a convolution of this HDR sky.

## Validation and limits

The final Blender source audit matches all 3,433 placement rows, including 41
optimized scanned trees. Both linked tree libraries use relative paths and pack
their texture images. The ten CC0 GLBs pass geometry, tangent, embedded PNG and
cutout-alpha validation. Re-run it with:

```powershell
python3.12 tools/validate_forest_cc0.py
```

The slice enables Bevy's stock temporal antialiasing; `FOREST_SLICE_TAA=0` restores
the ordinary SMAA path for comparison. The imported GLB PNG images currently have
one mip level, so fine foliage can still alias. TAA is a static-preview experiment,
not a fix for missing mipmaps; camera-motion blur/ghosting and performance remain
unvalidated. The ordinary campaign and older WORLD mode keep their AA settings.
The current High/Ultra presets disable SSAO; they retain contact shadows, bloom,
depth of field and the game's atmosphere passes. Ultra also supersamples the 3D
view at 2x resolution before downsampling to the output PNG.

The capture harness disables campaign audio playback before sources are started.
Its run record checks the corresponding log marker, successful PNG save, exit code
and loading/render errors. Core logic tests after the main merge passed (325 unit
tests and one integration test). The campaign still initializes outside the remote
slice, so its frame timings would not be an isolated biome benchmark.

## Review evidence

- [Overview](overview.png) / [capture settings and hashes](overview.json).
- [Ground-cover detail](detail.png) / [capture settings and hashes](detail.json).
- [High preset with the earlier lighting](high-default.png) /
  [capture record](high-default.json). This also changes exposure, sky and render
  resolution, so it is not an isolated TAA or performance comparison.
- [Release build](build-v6.txt), [core tests](core-tests.txt), and
  [review validation](review-validation.json).

The chosen two views are unmodified 1600 x 900 Bevy screenshots, using Ultra's 2x
supersampling plus stock TAA. They use the same executable and asset hashes and
the two poses preserved in the layout. Their runtime logs confirm silent audio,
successful saves, normal exits and no asset/render errors. The editable source is
audited separately in `art/forest_preview/final_scene_audit.json`.

This is a first forest composition for review, not a visual match claim. The
canopy is still darker and less sunlit than the reference, the old background
trees remain visibly simpler, and the path lacks the reference's fine stone/soil
detail. Ground-cover density also needs artistic review. These differences should
be judged before expanding the kit across the campaign. Art approval, motion
quality and performance acceptance remain pending.
