# Warbell: Blender campaign world preview

> **Paused for art approval.** The user narrowed the work to one forest slice.
> Use `scripts/preview-forest-slice.ps1` and the forest review captures instead.
> Buildings, other biomes, the full campaign gallery and performance evaluation
> remain unfinished experiments. The branch now includes main `fb308c8` through
> merge `6e849d2`, including the vegetation-density repair and Royal Footman.
> The text below records the earlier draft scope and is not a completion report.

This is an opt-in visual study of Warbell's **campaign environment** on top of main
`90f8d1b1b4b780b481046e76a82c3d931b64d60b`. It extends the separate
[tree-only study](../trees-2026-10-08/README.md) so the authored trees can be
judged with matching ground, vegetation, buildings and props. It is not a new
gameplay map or a replacement for the measured tree-only build.

`FOREST_BLENDERWORLD=1` selects the environment kit and also enables the Blender
trees. The preview covers campaign terrain materials and groundcover across the
biomes; castle keep tiers, walls, gates, towers, houses and courtyard details;
town production buildings and their construction/ruin states; bridges, ruins,
shrines, landmarks, camps and the ork fortress; and selected static or
interactive prop shells such as chests, boats, ballistae and training dummies.
Existing terrain height and island topology, paths, object placement, blockers,
combat, upgrades, harvesting and interactions remain game-owned. Animated
attachments and live effects retain their existing systems. RTS buildings,
characters (hero, villagers, enemies and wildlife), and effect redesign are
outside this pass. Hero PR #80 was initially a visual reference; its Royal Footman
changes are now included through the latest-main merge.

The meshes and shared opaque/cutout material atlases were authored in Blender
through Blender MCP. One `environment_kit.glb` is the review/interchange asset;
Bevy loads the corresponding named mesh JSON files and shared textures. Grass and
path ground albedo use task-specific **AI-generated image inputs**. Forest-floor
channels and grass cards now also use CC0 Poly Haven photographs. Blender imports
and tiles the generated inputs and derives approximate normal/roughness maps;
the scanned forest floor retains its original PBR channels. Other ground families
and atlases are procedurally authored. Source images, prompts and generation IDs are retained in
[the image-input record](../../../art/blender_environment/imagegen/README.md).
The [editable source and export record](../../../art/blender_environment/README.md)
and [asset contract](../../../tools/blender_environment/CONTRACT.md) document the
Blender scene, GLB/JSON parity checks, material channels and input hashes.

To open the current optional preview on Windows from the repository root:

```powershell
.\scripts\preview-blender-world.ps1
.\scripts\preview-blender-world.ps1 -OriginalWorld
```

The second command launches the original environment for visual comparison.
The launcher uses an isolated user-data directory and the saved preview binary
when available, falling back to a local release build. If neither executable
exists, build with `cargo build --release --locked` first. The study does not
change the default game unless the opt-in flag is set.

Early in-game QA found and corrected malformed JSON index numbers, inward-facing
roof triangles, and several architecture/landmark bodies whose height or pivot
missed existing animated attachments (notably the windmill sails and castle
flag). Ground and asset work then exposed a larger visual issue: a matched-camera
forest pair showed that sparse card shrubs and collapsed flower/litter/mushroom
variants left much more bare ground than the original scene. The snow birch,
swamp cypress stump and rocky wind-bent pine also need distinct models and
variant mappings; mapping them to generic dead or full trees removes recognizable
biome silhouettes. Those richness corrections are being integrated and require
new game captures before they can be called resolved.

## Verification still to complete

The final asset export, release build, matched-camera gallery and campaign
performance measurements are pending. The gallery will show both original and
Blender-world captures with the same camera, lighting and game state; the report
should retain visible failures as well as improvements. No final frame-time,
GPU-cost, memory or model-count claim belongs to this README until those checks
finish. The tree-only [results](../trees-2026-10-08/README.md) do not measure
this broader environment pass.
