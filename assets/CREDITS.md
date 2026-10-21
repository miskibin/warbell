# Third-party asset credits

Downloaded models / textures used by the game (CLAUDE.md "Assets: download first"). Each source was
downloaded into its own folder outside the repo and converted by the named Blender script; nothing
bundled with a download is executed.

| Asset | Used for | Author | Licence | Source | Converted by |
|---|---|---|---|---|---|
| Raven (raven_0.blend) | carrion vulture (`oddities.rs`, recoloured to a griffon), crows over the Hold (`poi.rs`) | tehbucket (OpenGameArt) | CC0 1.0 | https://opengameart.org/content/raven-0 | `tools/blender_forest/creatures_dl.py` |
| Rabbit (rabbit.blend) | wild rabbit (`critters.rs`) | CDmir (OpenGameArt) | CC0 1.0 | https://opengameart.org/content/rabbit-0 | `tools/blender_forest/creatures_dl.py` |
| Old Deer Male (deer1.blend) | red deer stag + elk (`assets/animals/deer_stag.glb`, `animal_skin.rs`; walk authored on export) | CDmir (OpenGameArt) | CC0 1.0 | https://opengameart.org/content/old-deer-male | `tools/animals/export_deer.py` |
| Deer Female (doe.blend) | red deer hind (`assets/animals/deer_doe.glb`, `animal_skin.rs`) | CDmir (OpenGameArt) | CC0 1.0 | https://opengameart.org/content/deer-female | `tools/animals/export_deer.py` |
| Grey Wolf Rebuilt | grey wolf (`assets/animals/wolf.glb`, `animal_skin.rs`; leg IK rebuilt, walk / trot / run / bite authored on export) | kenchoo (Sketchfab), rebuilt from "Grey Wolf" by Xfdfgd ("all of life", https://sketchfab.com/3d-models/grey-wolf-6a9f281a9fb344f3a7b8bcab3c6f4c9a) | CC-BY 4.0 | https://sketchfab.com/3d-models/grey-wolf-rebuilt-c0fd1738fa31413b906e2914f5a332b9 | `tools/animals/export_wolf.py` |
| Brown Bear (animated) | brown bear — the forest bruiser (`Species::Boar`; `assets/animals/bear.glb`) and, re-dyed white, the polar bear (`polar_bear.glb`; `animal_skin.rs`; coat warmed, decimated, leg IK + idle / walk / run / swipe authored on export) | Андрей (andrey., Sketchfab) | CC-BY 4.0 | https://sketchfab.com/3d-models/brown-bear-animated-2f59c850fa4e46c08c3c72dd99a63c4b | `tools/animals/export_bear.py` |
| Goat | goat (`assets/animals/goat.glb`, `animal_skin.rs`; decimated, 1K textures, walk / run / graze authored on rebuilt leg IK) | GerardoA400 (Sketchfab) | CC-BY 4.0 | https://sketchfab.com/3d-models/goat | `tools/animals/export_quad.py` |
| Dog | dog / sheepdog (`assets/animals/dog.glb`, `animal_skin.rs`; re-rested, walk / trot / run / graze / idle authored) | MAXDESIGN-3D (Sketchfab) | CC-BY 4.0 | https://sketchfab.com/3d-models/dog | `tools/animals/export_quad.py` |
| Camel | camel (`assets/animals/camel.glb`, `animal_skin.rs`; saddle blanket removed, re-rested, source idles + walk, run / graze authored) | E.A.Cornell (eb78, Sketchfab) | CC-BY 4.0 | https://sketchfab.com/3d-models/camel | `tools/animals/export_quad.py` |
| Esox - Animated Fish | pond / river fish (`fish.rs`) | CDmir (OpenGameArt) | CC0 1.0 | https://opengameart.org/content/esox-animated-fish | `tools/blender_forest/creatures_dl.py` |
| Shrub 02 (shrub_02) | the bushes (`bush_a` / `bush_b`: meadow bushes, forest class-2 slot) | Poly Haven | CC0 1.0 | https://polyhaven.com/a/shrub_02 | `tools/blender_forest/scanned_props.py` |
| Wine Barrel 01 (wine_barrel_01) | market barrels, meadow crate pile (look baked onto a low-poly lathe) | Poly Haven | CC0 1.0 | https://polyhaven.com/a/wine_barrel_01 | `tools/blender_forest/scanned_props.py` |
| Wooden Crate 01 (wooden_crate_01) | market barrels + meadow crate pile | Poly Haven | CC0 1.0 | https://polyhaven.com/a/wooden_crate_01 | `tools/blender_forest/scanned_props.py` |
| Wooden Crate 02 (wooden_crate_02) | market barrels + meadow crate pile | Poly Haven | CC0 1.0 | https://polyhaven.com/a/wooden_crate_02 | `tools/blender_forest/scanned_props.py` |
| Terrain Tiles (terrarium elevation) | horizon islands' height fields (`distant_isles.rs`: Lofoten, Austvågøya, Stromboli, Fogo, La Gomera, Jan Mayen) | Mapzen / AWS Open Data; sources incl. SRTM, GMTED2010, ETOPO1, EU-DEM, ArcticDEM | open data, attribution per source | https://github.com/tilezen/joerd/blob/master/docs/attribution.md | `tools/terrain_isles/fetch_isles.py` |
| Sentinel-2 cloudless 2016 | horizon islands' colour (`assets/isles/*_albedo.png`) | "Sentinel-2 cloudless - https://s2maps.eu by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016 & 2017)" | CC BY 4.0 | https://s2maps.eu | `tools/terrain_isles/fetch_isles.py` |

Photographic textures from Poly Haven (CC0) are listed in `assets/forest_pbr/ATTRIBUTION.txt`.

## Realistic townsfolk (`tools/npc_mpfb/` → `assets/npc/`, `src/npc_skin.rs`)

| Asset | Author | Licence | Source |
|---|---|---|---|
| Bartender (rigged village woman) | CDmir, TinyWorlds | CC0 | https://opengameart.org/content/bartender |
| Universal Animation Library 1 + 2 (Standard) | Quaternius | CC0 | https://opengameart.org/content/universal-animation-library |
| MakeHuman system assets (bodies, skins, hair, eyes) | MakeHuman Community | CC0 | https://static.makehumancommunity.org/assets/assetpacks/makehuman_system_assets.html |
| MPFB 2 (Blender add-on, tool only) | MakeHuman Community | GPL-3.0+ (tool; output is CC0) | https://extensions.blender.org/add-ons/mpfb/ |
| Garments: toigo_wool_pants, toigo_long_full_skirt, culturalibre_male_boots, rehmanpolanski_beard_viking, wdg_scruffy_beard | toigo, culturalibre, rehmanpolanski, wdg | CC0 | MakeHuman asset packs pants01 / skirts01 / shoes01 / bodyparts05 |
| Garments: elvs_male_boho_top1, punkduck_lace_up_blouse, elvs_ruffle_sleeve_peasant_blouse_1, mindfront_male_trousers_2, maciekg_leather_boots, punkduck_medieval_boots, punkduck_female_half-boots, punkduck_riding_boots, maciekg_wizard_robe, grinsegold_full_beard | elvs, punkduck, mindfront, maciekg, grinsegold | CC-BY (credit required) | MakeHuman asset packs shirts02 / shirts03 / pants02 / shoes03 / suits03 / bodyparts06 |
| Maud (campaign cast): hair elvs_braid_bun, garment elvs_ladies_apron | Elvaerwyn | CC-BY (credit required) | MakeHuman community asset library (installed via MPFB) |
| Fabric / leather textures (linen, wool, hessian, leather) | Poly Haven | CC0 | https://polyhaven.com |
| Townsfolk mocap clips (walk, run, idles, sitting, work, death — list in `tools/npc_mpfb/mixamo_private.ps1`) | Adobe Mixamo | Mixamo terms: royalty-free in games, **no redistribution as files** — kept out of this repo (gitignored `assets/npc_private/`, rebuilt per machine) | https://www.mixamo.com |
| Hero locomotion clips: Walking, Dive Roll (X Bot, in place) — baked to numbers in `src/player/anim/mixamo_clips.rs` by `tools/mixamo_retarget/bake.py`; the source FBX are **not** in the repo | Adobe Mixamo | Mixamo terms: royalty-free in games, no redistribution of the FBX files | https://www.mixamo.com |

## Cutscene audio

| Asset | Used for | Source |
|---|---|---|
| `assets/audio/sfx/cutscene/cs_crows_flush_{1,2}.ogg` (crow flock taking flight, wings + caws) | P.3 fort film, the flock flushed at the gate | generated by the project owner (Adobe Firefly audio), loudness-normalised |
| `cs_step_child_*` / `cs_step_adult_*` / `cs_bell_hum` | P.3 fort film: soft footsteps, the bell's hum | cut from the repo's own `footstep-dirt-*` / `war-bell` SFX |

## Footstep SFX (`assets/audio/footsteps/<surface>-<1..4>.ogg`)

Single leather/hiking-boot steps sliced out of longer takes (onset-detected, tail-trimmed, 25 ms fade-out, mono 44.1 kHz Ogg Vorbis, RMS-matched). The Freesound HQ previews were used as the source audio.

| Files | Source | Author | Licence | URL |
|---|---|---|---|---|
| `grass-1..4` | "Hiking Boot Footsteps on Grass" | Fission9 (Freesound) | CC0 1.0 | https://freesound.org/people/Fission9/sounds/521587/ |
| `wood-1..4` | "Hiking Boot Footsteps on Wooden Planks" | Fission9 (Freesound) | CC0 1.0 | https://freesound.org/people/Fission9/sounds/521589/ |
| `stone-1..4` | "Hiking Boot Footsteps on Stone" | Fission9 (Freesound) | CC0 1.0 | https://freesound.org/people/Fission9/sounds/521590/ |
| `dirt-1..4` | "Soil Steps" | Phil25 (Freesound) | CC0 1.0 | https://freesound.org/people/Phil25/sounds/208102/ |
| `snow-1..4` | "Footsteps_Mountain_Boots_Snow_Walk_Mono" | Nox_Sound (Freesound) | CC0 1.0 | https://freesound.org/people/Nox_Sound/sounds/613849/ |
| `sand-1..4` | "Human_Steps_Sand_Dry_Thickererer" | LamaMakesMusic (Freesound) | CC0 1.0 | https://freesound.org/people/LamaMakesMusic/sounds/403604/ |
| `mud-1..4` | "Close-mic footsteps of a person in leather boots" (variation 2), user-generated | generated by the project owner (Adobe Firefly audio) | project-owned | n/a |
| `water-1..4` | "Footsteps wading through ankle-deep shallow water" (variations 1 + 4), user-generated | generated by the project owner (Adobe Firefly audio) | project-owned | n/a |

## Combat audio

| Asset | Used for | Author | Licence | Source |
|---|---|---|---|---|
| `assets/audio/flesh-cut-{1,2,3}.ogg` (from `sword-1a`, `sword-1b`, `sword-arm-2b`), `flesh-heavy-{1,2}.ogg` (from `punch_1b`, `punch_alt-2a`) — "Fleshy fight sounds" | the flesh layer of a connecting sword blow (heavy takes on a kill / crit / Heavy; `audio/sfx.rs`) | Will Leamon | OGA-BY 3.0 (credit required) | https://opengameart.org/content/fleshy-fight-sounds |
| `assets/audio/body-thud-{1,2,3}.ogg` (`impactSoft_medium_000..002`), `body-thud-heavy-{1,2}.ogg` (`impactSoft_heavy_000..001`) — Impact Sounds | the low body-weight thud under a connecting blow | Kenney | CC0 1.0 | https://kenney.nl/assets/impact-sounds |

All peak-normalised to −1 dBFS, mono, with a short fade-out.

| `assets/models/ship_caravel.glb` — "Caravel Ship" (textures downscaled to 1024, spec-gloss converted to metal-rough) | the ship seen from outside in the P.0 cabin cutscene (`scenes/cabin.rs`) | Ginny Sutton (suttonj94) | CC-BY 4.0 | https://sketchfab.com/3d-models/caravel-ship-c9819a0e8a57429f888b74978219fb7a |
