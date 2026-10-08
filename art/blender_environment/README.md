# Warbell Blender environment kit

This is the **campaign static environment** authored in isolated Blender 4.5.9 through the official `mcp-for-blender` `execute_blender_code` bridge on port 9877. It does not include heroes, villagers, enemies, wildlife or RTS buildings.

- `warbell_environment.blend` is the editable source scene, containing every named mesh and packed atlas image. `studio_preview.png` is an earlier Eevee visual QA frame, captured before the 246-model meadow/biome expansion and Poly Haven forest texture; use the current game captures to judge those later changes.
- `assets/models/blender_environment/environment_kit.glb` is **one** GLB with all named meshes and shared material textures embedded once. It is an audit/interchange artifact; Bevy currently reads individual `<name>.json` files.
- Each JSON is the final GLB accessor data for one named mesh, including final split normals and normalized vertex colours. `tools/blender_environment/validate_kit.py` independently checks every JSON against the named GLB node/mesh, indices, unit normals, embedded texture SHA, manifest file hashes and source hashes.
- `surface_atlas.png` has a 4×4 tile grid for dressed limestone, fieldstone, lime plaster, planks, oak beams, clay shingles, slate, thatch, cobble, earth, straw, iron, bronze, granite, cloth and wattle. `surface_orm.png` is linear data with R=AO, G=roughness, B=metallic. `surface_emissive.png` is intentionally nearly black; animated lights stay in game code.
- `vegetation_atlas.png` has cutout leaf/grass/fern/shrub/flower/reed/crop shapes plus full-coverage bark, wood, stone, bone and mud cells. The interchange GLB uses glTF `MASK` with its default cutoff **0.5**; the Bevy runtime uses **0.35** with coverage-preserving mipmaps for thin leaves. The eight main `ground_*_{albedo,normal,roughness}.png` families are tileable 512² biome/path surfaces, authored at approximately 2.5 world units per repeat.

The meshes, material atlases and five ground families are authored procedurally in Blender. Meadow grass and main path albedo come from task-specific OpenAI imagegen source PNGs preserved under `art/blender_environment/imagegen/`; selected inputs are `grass-albedo-v2.png` and `path-albedo-v2.png`. The original images and unused forest imagegen source remain archived there with prompts and provenance. Blender imports the selected diffuse images, resizes them to 512², matches a narrow border for seamless tiling, and derives subtle normal/roughness maps without recolouring the albedo.

Forest ground instead uses the genuine diffuse, OpenGL normal and roughness maps of [Poly Haven leafy_grass](https://polyhaven.com/a/leafy_grass), by **Charlotte Baglioni**. The separate dark `ground_stony_dirt_path_*` alternate triplet uses [Poly Haven stony_dirt_path](https://polyhaven.com/a/stony_dirt_path), by **eye-candy.xyz**; the main path still uses the lighter imagegen texture. Both source sets are [CC0](https://polyhaven.com/license). Original 1k PNGs, download URLs, MD5 and SHA256 checksums are preserved in `art/blender_environment/polyhaven/`. Blender imports the original channels, resizes them to 512², matches only the tile seam and keeps diffuse colours and PBR channels intact. `texture_spec.json` and `manifest.json` record source hashes. The earlier tree experiment in `blender_trees` is a separate frozen asset set with its own MIT leaf-image attribution.

To regenerate, connect an isolated Blender 4.5.9 GUI with the official addon listening on localhost port 9877, set `BLENDER_MCP_COMMAND` to the installed `mcp-for-blender` command if it is not on the Python environment path, and run from the repo root:

```powershell
python tools/blender_environment/mcp_client.py tools/blender_environment/make_atlases.py
python tools/blender_environment/mcp_client.py tools/blender_environment/build_environment.py
python tools/blender_environment/validate_kit.py
python tools/blender_environment/mcp_client.py tools/blender_environment/render_preview.py
```

The bridge appends an auditable result record (with source SHA) to `tools/blender_environment/mcp_calls.jsonl`. The Blender process needs its native NumPy path first. The generator is deterministic at mesh/atlas level; `.blend` and GLB bytes can still vary with Blender save/export metadata across runs. Preserve one coherent export and its manifest for review.
