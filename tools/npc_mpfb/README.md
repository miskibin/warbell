# Realistic NPC pipeline (MakeHuman / MPFB + Quaternius UAL) — experimental

Blender 4.5 + the MPFB 2 extension build photoreal-ish medieval townsfolk, and every Quaternius
Universal Animation Library clip (UAL1 + UAL2, CC0) is retargeted onto their shared `game_engine` rig.
The exported looks (`assets/npc/npc_<look>.glb`) are the game's townsfolk (`src/npc_skin.rs`), replacing
the procedural 14-joint bodies (`FOREST_NOSKIN=1` brings those back).

Downloads (each in its own folder under `C:/Users/skibi/Documents/Codex/downloads/`, never executed):
`oga_bartender` (CC0 hand-made woman), `ual1` / `ual2` (CC0 clips), `mpfb` (extension zip),
`mh_system_assets` + `mh_packs` (MakeHuman asset packs, copied into MPFB's user data dir).

| Script | What |
|---|---|
| `mpfb_cast.py` | one MPFB human from a spec (macro sliders, race, skin, hair, eyes, rig) |
| `npc_cast.py` | the 14-person cast; community garments are re-textured with Poly Haven linen / wool / leather |
| `build_cast_blend.py` | builds the whole cast into one .blend |
| `bake_anims.py` | UAL clips -> Blender Actions on the MPFB rig (bone-direction swing retarget; any cast member can play them) |
| `town_film.py` | town set from the game's own kits (`town_kit.glb`, `market.glb`, ...) + Poly Haven props + animated cast; renders frames |
| `build_cast_blend.py` / `export_game.py` | `blender -b --python build_cast_blend.py -- <out.blend> [name,...]` builds (part of) the cast; `blender -b <blend> --python export_game.py -- <npc> <out_dir>` bakes + exports one look (per-look `export` options in the cast spec: layer pass / hidden-skin cull / weight inheritance, see its docstring) |
| `bake_mixamo.py` / `export_anims.py` / `mixamo_private.ps1` | Mixamo mocap clips (walk / run / idles / sitting / work…) retargeted onto the rig and exported as one animation-only GLB per look into the **gitignored** `assets/npc_private/` (Mixamo allows the clips in the game, not as public files). The game prefers them by clip name over the UAL clips; `FOREST_UALANIM=1` forces UAL. See the `.ps1` header for the clip list + Mixamo export settings |
| `add_mouth_morph.py` | (superseded by `add_face_morphs.py`) Bakes a `jaw_open` morph target into an exported look GLB **in place** (run on the cast blend, after `export_game.py`; refuses a GLB that already has it): MakeHuman's jaw weights + the lower lip, hinged at its mouth joint. `src/lipsync.rs` opens it to the loudness of the line being spoken. Re-run it after re-exporting a look (all 15 were baked at angle `0.16`) |
| `add_face_morphs.py` | Talking faces, baked into an exported look GLB **in place** (run on the cast blend; idempotent, compacts the buffer): MakeHuman's CC0 `faceunits01` jaw + blinks and the 14 `visemes02` visemes as sparse morph targets (order fixed, read by index in `src/lipsync.rs`), plus the CC0 `teeth_base` / `tongue01` proxies fitted through their `.mhclo` and a dark raycast-fitted mouth bag, as one vertex-coloured `npc_mouth` primitive. `blender -b npc_cast.blend --python add_face_morphs.py -- <npc> <glb> <mh_visemes dir> <mpfb user data dir>` (Maud: `npc_maud.blend`). All 15 looks baked |
| `drop_cornea_glb.py` | Deletes the black cornea shell (outer dark eye shell, the pupil under it kept) from exported GLBs in place — plain Python + `texture2ddecoder`; run before `add_face_morphs.py`. All 15 looks done |
| `dds2png.py` | decodes the game's BC7 DDS textures to PNG so Blender can bind them |
| `retarget_render.py` | single-character version (Bartender) used for the first showcase |

Paths inside the scripts point at the author's scratchpad; adjust `S`/`D` before re-running.
