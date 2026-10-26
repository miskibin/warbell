# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

**"Warbell"** (renamed from "D: Tileworld", June 2026) — a **Bevy 0.19** game (upgraded from 0.18 in
June 2026). This started as a static forest-scene viewer and
has grown into a full playable game: a knight defends a central castle against night-wave
ork sieges, with combat, economy, an upgrade tree, inventory, villagers, succession, and five
biomes on one enlarged island. (It began life as a port of an old TypeScript/three.js game;
that original is **obsolete and gone as a reference** — this repo + `crates/core` are the sole
source of truth. Do not look for or cite "the old game".)

> `README.md` and the `main.rs` module doc-comment now describe the actual game (both were
> truthed-up after an earlier era where they still claimed "no player, no gameplay, just the
> scene"). They're fine as entry points, but the per-subsystem **source of truth** is the parity
> roadmap, `docs/superpowers/specs/2026-06-07-tileworld-parity-port-roadmap.md` (numbers + reuse
> pointers). If you spot a code comment still describing the old static-viewer state, it's stale —
> fix it.

## Models / subagents

**Main session runs Fable** — best model, worth it for the reasoning-heavy work (architecture,
gameplay logic, tricky edits). Fable is expensive, so **don't burn it on grunt work.** Delegate
token-heavy, low-judgement tasks to an **Opus subagent** (cheaper) via the `Agent` tool with
`model: "opus"`:

- **Codebase exploration** — broad "where is X / how does Y work" fan-out reads (use the `Explore`
  agent with `model: "opus"`).
- **Visual debugging** — capture-harness runs (`FOREST_SHOT`/`FOREST_CLIP`), reading back
  screenshots, iterating on a shot until it looks right.
- Any bulk, mechanical, or high-token-low-insight task (log sifting, wide grep sweeps, repetitive
  edits).

Rule of thumb: **Fable decides, Opus fetches.** If the task is mostly "read a lot / look at a lot"
and little judgement, hand it to an Opus subagent and keep the conclusion, not the file dumps.

## Assets: download first, model second

**Standing permission from the user (2026-10-13): whenever an asset can be downloaded instead of
built, download it** — 3D models (creatures, animals, props, buildings), animations, textures, HDRIs,
SFX. Hand-modelling in Blender is the fallback, not the default.

- **Licences:** prefer CC0 (Poly Haven, ambientCG, Quaternius, Kenney, Sketchfab CC0 filter). CC-BY
  is OK — record author + licence + URL in `assets/CREDITS.md`. No NC/ND/editorial-only, no ripped game assets.
- **Fit the game:** realistic look (KCD2-like), PBR, correct real-world scale, reasonable poly count
  (decimate / bake LODs in Blender), retexture to match the scanned palette, export through the
  existing `tools/blender_forest/` pipeline into the kit GLBs.
- **Safety:** each download into its own new empty folder outside the repo (e.g.
  `C:/Users/skibi/Documents/Codex/downloads/<name>/`), never run scripts that come with it, only
  import mesh/texture/anim files.
- Voices are the exception: no TTS / voice-model downloads.

## Commands

```bash
cargo run                       # build + open the game window
cargo test                      # runs the crates/core unit tests (~268; the parity validation spec)
cargo test -p tileworld_core <name>   # a single test by substring
cargo check                     # type-check without the (slow) link
```

`bevy` deps compile at `opt-level = 3` even in dev (see `Cargo.toml`) — otherwise the scene runs
at single-digit FPS — while our own crates stay at low opt for fast rebuilds. The first build is
slow; incremental rebuilds of `src/` are fast.

**Multi-agent caveat:** in a *parallel-dispatch* session (several agents each editing ONE module
against a *shared* `target/`), agents must **not** run `cargo build/check/run` — a concurrent build
corrupts the shared dir; the integrator builds once at the end. That rule is **only** for parallel
dispatch. A normal single Claude session should build and verify.

### Git / agent workflow rules (non-negotiable)

- **NEVER `git reset --hard`** (or any destructive history/working-tree reset: `git clean -fd`,
  force-checkout over dirty files, `git checkout -- <path>` that discards work) **without explicit
  per-instance permission.** Blanket prior approval does not carry over — ask each time, naming what
  would be lost.
- **Push your code after you finish a feature.** Once the feature is complete and verified, commit
  and `git push` — don't leave finished work sitting only in the local working tree.
- **Other agents may be working in parallel.** If your changes overlap with another agent's
  in-flight work, you can simply **wait for them to finish** rather than racing or stomping their
  edits.
- **Push back on bad ideas — don't just agree.** If a request is wrong, risky, or has a clearly
  better alternative, say so plainly and explain why before (or instead of) implementing it. Honest
  technical disagreement is wanted, not reflexive compliance.

### Cloud / remote-container setup (Claude Code on the web, fresh containers)

A fresh Linux cloud container has the Rust toolchain but **not** the system libraries Bevy links
against, so the *first* `cargo check`/`build`/`run` panics in a build script with
`Package wayland-client was not found` (or `xkbcommon` / `alsa` / `libudev`). This is an
environment gap, **not** a code problem — don't "fix" it in `Cargo.toml`. Install the headers once
per container (the container is ephemeral, so this must run every fresh session):

```bash
sudo apt-get update && sudo apt-get install -y \
  libwayland-dev libxkbcommon-dev libudev-dev libasound2-dev
```

(These are Bevy's standard Linux deps: Wayland + XKB for windowing/input, ALSA for the `wav`
audio feature, libudev for gamepad enumeration.) After installing, `cargo check` succeeds (the
first build still recompiles all of `bevy` at `opt-level = 3`, ~2–3 min). On developer machines
(macOS / Windows) none of this applies. To make web sessions build without the manual step, put
the `apt-get` line in a **SessionStart hook** (see the `session-start-hook` skill).

### Screenshot harness (how to verify visuals — the Bevy window can't be captured externally)

```powershell
$env:FOREST_SHOT="shot.png"; cargo run      # warms up (≥240 frames AND ≥10s so cold pipelines/shaders/IBL settle; FOREST_SHOT_WARMUP=<secs> raises the floor), saves PNG, exits
```

**For player-perspective / game-feel shots use `FOREST_TPS=1`, NOT a hand-placed `FOREST_CAM`.**
`FOREST_TPS` boots the real over-the-shoulder **gameplay** camera (`player::camera`), so the frame
matches what a player actually sees — no guessing fly-cam coordinates (which reliably produces
useless god/cloud-cam angles). Just drop the hero with `FOREST_HERO="x,z"`; tune the view with
`FOREST_TPS_AZ`/`_PITCH` (radians) + `_DIST` (units). Pair with `FOREST_DEMO=explore` to film a real
**walking** shot (footsteps / dust kicks / anim) through the follow-cam. Reserve
`FOREST_CAM` for deliberate god/overview framing only.

**A HIGH god-cam to "see the whole island" washes out to flat white — don't fight it.** Three
things compound at altitude and none is your bug: atmospheric **haze** fogs distant terrain to the
sky colour (`FOREST_FOG="clear,full"`, bigger = thinner, only nibbles — it does NOT clear a
whole-island view); the **cloud deck** sits at y≈650 so a camera above it shoots cloud-tops; and
**midday bloom** (`FOREST_TIME≈0.25–0.35`) blows the bright/snow areas white. To actually see
terrain, frame **low + oblique** (camera height **~15–40**, close, looking near-flat along the
ground — the orbit examples below use *height 14* on purpose); a near-top-down at y≈25 over one
feature reads crisply. To verify a **spread-out** feature (rivers, biome layout), take **several
low close shots at the known biome/feature coords** (below), one per region — NOT one doomed
overview. For a true island-wide frame use the **orbit clip** (`FOREST_CLIP_ORBIT`, low height)
and pick a frame; a single high static cam can't do it cleanly.

```powershell
# player-perspective still (hero framed by the real follow-cam):
$env:FOREST_SHOT="shot.png"; $env:FOREST_TPS="1"; $env:FOREST_HERO="-18,24"; cargo run
# real gameplay walk clip (the demo drives the hero; the follow-cam films it):
$env:FOREST_CLIP="target/clips/walk"; $env:FOREST_TPS="1"; $env:FOREST_TPS_PITCH="0.6"; $env:FOREST_DEMO="explore"; cargo run
```

For **GIFs / video** (itch.io promo, motion bugs) use the clip mode instead — it saves a numbered
PNG per frame to a dir, then ffmpeg stitches it. A clamped fixed timestep keeps motion smooth
despite the per-frame encode stall; `FOREST_CLIP_ORBIT` slowly circles a point. `siege_clip_refill`
keeps a `FOREST_WAVE` assault topped up so a long siege actually films a battle.

```powershell
# island/biome flyover (orbit "cx,cy,cz,radius,height,deg_per_sec")
$env:FOREST_CLIP="target/clips/desert"; $env:FOREST_CLIP_ORBIT="71,1.5,-46,22,14,7"; $env:FOREST_TIME="0.24"; cargo run
# sustained night siege
$env:FOREST_CLIP="target/clips/siege"; $env:FOREST_WAVE="1"; $env:FOREST_DEFEND="1"; $env:FOREST_TOWN="1"; $env:FOREST_CAM="0,15,30,0,2,-8"; cargo run
# stitch (per-clip mp4 + gif): ffmpeg -framerate 30 -i frame_%05d.png -pix_fmt yuv420p out.mp4
```
Biome region centres (world XZ, at `MAP_SCALE` 3.9): snow (-179,-117) · desert (145,-133) · rock (176,23) ·
forest (-154,99) · swamp (0,148) (the castle/village stay world-fixed at the origin). Base space (`worldmap::REGIONS`,
144×164, world = base·3.9 − (280.5, 210.6)): snow (26,24) r48 · desert (109,20) r44 · rock (117,60) r38 · forest
(32.5,79.5) r38 · swamp (72,92) r20 — the Oct 2026 biome rebalance grew the desert ~47% over the rock range's old low
northern plateau and dropped the E marsh arm (the lake is a tarn at the rock range's SW foot); the rival stronghold
(`rival::RIVAL_CENTRE`, world (117,-156)) sits in the desert's NW. `BIOME_DUMP=<file.ppm> cargo test --release
biome_footprints` writes a hillshaded biome map (pass trails, warden lairs, rival) + per-biome tile counts.
The grass between the castle meadow and the biomes is the **countryside** (`countryside.rs`: farmstead at (28,66) +
croft (-64,52) with levelled yards, hedges, dry-stone walls, groves, biome-foreland scrub, flower drifts, hay,
paddock horses). Planning diagnostics: `COUNTRY_DUMP=<ppm>` (2 px/u map: roads, fields, plots, village, clearings,
the plan), `FARM_SCAN=1` (open yard sites), `SPOT_WHY="x,z,r"`, `MEADOW_SURVEY=1` (open-grass depth per bearing).
Relief round the castle is keyed on distance past the walls (`worldmap::relief_ease`), not `SAFE_R`.
These scale with `MAP_SCALE` — code with a hand-authored world coord
should route it through `worldmap::world22` (rescales 2.2-era coords) instead of baking the scale in.
**Perf baseline:** `tools/prof/bench.sh <out_dir> [preset ...]` runs the release build through village / forest / siege × presets and prints frame / main+render CPU / GPU pass / draw summaries (`BENCH_SCENES`, `BENCH_SECS`, `BENCH_EXE`); it steals window focus — don't run while the user plays. Background load skews absolute numbers, so A/B interleaved.
Clip knobs: `FOREST_CLIP_FRAMES` (150) · `FOREST_CLIP_FPS` (30) · `FOREST_CLIP_WARMUP` (30). `FOREST_CLIP_ORBIT` is keyed on frames *saved*, so two A/B runs frame identical scenery; `FOREST_CLIP_PAN="x,y,z,pitch_deg,deg_per_sec[,start_yaw]"` holds the eye still and pans the yaw — motion with no parallax, so a rigid-shift-compensated frame diff isolates shimmer/crawl (how the AA and mip-bias A/Bs are measured; `FOREST_MIPBIAS=<b>` overrides TAA's mip bias, default -0.5).
**Clips record the game's OWN audio** into `<clip dir>/audio.wav`, sample-locked to the frames
(`audio_capture.rs`: every started sound is mirrored into an offline rodio mixer that advances
exactly 1/fps per saved frame, copying live volume/fades, pitch, pause and spatial positions; the
sim runs a fixed 1/fps step while it's on). Mux: `ffmpeg -framerate 30 -i frame_%05d.png -i audio.wav
-c:v libx264 -pix_fmt yuv420p -c:a aac -shortest out.mp4` (the in-game mix is quiet, ~-30 dB mean —
`loudnorm` it). `FOREST_CLIP_AUDIO=0` = video only.

Env hooks that stage a scene for a shot (combine with `FOREST_SHOT` **or** `FOREST_CLIP`), all read at startup:

| Var | Effect |
|---|---|
| `FOREST_SHOT=path.png` | single-shot capture-and-exit harness (`capture.rs`) |
| `FOREST_CLIP=dir` (+`_FRAMES`/`_FPS`/`_WARMUP`/`_ORBIT`) | frame-sequence recorder → ffmpeg GIF/video (`capture.rs`) |
| `FOREST_CAM`, `FOREST_TIME`/`FOREST_DAY`/`FOREST_NIGHT` | camera pose / time-of-day for the shot |
| `FOREST_HERO="x,z"` | drop the hero at a world XZ (e.g. deep in a biome region) to stage its reactive atmosphere/weather |
| `FOREST_BIOME` | boot straight into a given biome |
| `FOREST_WAVE` / `FOREST_DEFEND=1` | stage a night siege / arm all defenses + walls |
| `FOREST_MUSTER=1` | rally the whole town into the **war party** at boot (as if pressing `K`) so a shot frames the muster; pair with `FOREST_DEMO=work` (stages a 14-pop town) for a full host (`villagers.rs::stage_muster`) |
| `FOREST_ORKLINE="x,z"` | park one ork of each variant in an idle line at a world XZ (model close-ups) |
| `FOREST_ARCHERS=<n>` | retrain the whole standing militia as **longbow archers** at boot (`villagers.rs::stage_archers`); numeric `n ≥ 2` also raises `town.population` to `n` so a whole squad grows in to retrain. Pair with `FOREST_MUSTER`+`FOREST_HERO` to park a volleying rank anywhere, `FOREST_ORKLINE` for live targets, or `FOREST_WAVE` for a defended siege. (`FOREST_VIEW=peasant:archer` + `FOREST_VIEW_ANIM=bow` previews the model / draw-loose clip in the viewer.) |
| `FOREST_CAGETEST="x,z"` | park the prisoner-cage rescue's before/after states side by side at a world XZ: a CLOSED cage of real seated peasant captives + an OPENED emptied one 5.5u further +X (`camps::spawn_cage`); both doors face +X, so frame from the east. Film the actual door-swing + walk-out with `FOREST_DEMO=rescue` + `FOREST_CLIP` instead |
| `FOREST_BREACH=1` | auto-break the Hold gate on the first sim frame so a shot/clip films the woken garrison + the Warlord boss without a keypress (`ork_fortress::stage_breach`); pair with `FOREST_HERO`/`FOREST_CAM` inside the walls |
| `FOREST_RIVAL=<n>` | instantly raise `n` buildings in the **rival stronghold** (the desert AI opponent, `rival.rs`) so a shot frames a grown rival town instead of waiting out its economy (default: fill the bailey); the rival keep/walls/garrison spawn regardless. Frame it at `rival::RIVAL_CENTRE` ≈ world `(117, -156)` at MAP_SCALE 3.9 (NE desert) |
| `FOREST_TREELINE="x,z"` | park one of each `TreeKind` (broadleaf/birch/pine/poplar/autumn/dead/stump) in a 2× row at a world XZ (tree-model close-ups, `trees.rs`) |
| `FOREST_FISHLINE="x,z"` | park one of each fish variety (silver/blue/gold) frozen mid-leap in a lit row at a world XZ (fish-model close-ups, `fish.rs`) |
| `FOREST_SCENE=farm` (+`FOREST_SCENE_T=<s>`) | stage the **Ch.2 cutscene** (campaign screenplay: dawn farm, Farmer/Wat exchange, ~19 s, `scenes.rs::FarmMorning`): scripted actors, camera cuts, captions, letterbox + fades; `_T` freezes the clock at that second for a still (`FOREST_CLIP` starts it with recording). Camera/hero are scene-driven in PostUpdate, so it works in Play too |
| `FOREST_SCENE=storm` | stage the **P.1 "Piorun" cutscene** (video rig, `scenes/storm.rs`, spec `docs/superpowers/specs/2026-10-26-p1-piorun-cutscene.md`): four lightning-lit pictures at Wreck Cove + a coda; film with `FOREST_TPS=1` + `FOREST_CLIP_WARMUP=260` (the warm-up rehearses every shot so skins/pipelines are compiled before recording), soundtrack `tools/cutscene/storm_mix.py`. Look-dev: `FOREST_SCENE_SHEET="t1,t2,…"` (+`_HOLD=n`) under `FOREST_CLIP` saves one frame per listed scene second (any cutscene); `FOREST_STORM_DAY=1` = same blocking at midday, storm off |
| `FOREST_MENU=1` | shoot the start screen |
| `FOREST_FP=1` | boot straight into first-person (forces Play so the follow-cam eye-view can be captured; `player/camera.rs`). Also shows the FP reticle under a capture (no pointer lock needed) |
| `FOREST_VIEW_FP=1` (+`FOREST_FP_PITCH=<rad>`, `FOREST_VIEW_FOE=grunt\|scout\|berserker\|shaman`, `_FOE_ANIM`, `_FOE_DIST`) | model viewer in **first person**: the eye on the previewed knight's head exactly as in game (`camera::viewer_fp_cam` + the real `fp_head_track`/`fp_body_visibility`), so armour-in-the-lens, the raised shield and a foe's on-screen size are checked in ~25 s instead of a world boot. `FOREST_VIEW_ANIM` drives the hero (`run`, `block`, `drawwalk`…), `_FOE_ANIM` the ork (`smash`, `guard`…); a foe brings in the close-quarters FOV widen + eye step-back. Without `_FP`, `FOREST_VIEW_FOE` + a side `FOREST_CAM` compares heights |
| `FOREST_VIEW_SAY=<clip under assets/>` | model viewer: every previewed townsperson mouths that voice line on a loop (its Rhubarb mouth-shape track, `lipsync.rs`), silently — look-dev for talking faces; frame the head with `FOREST_CAM` (guard: `"0.1,1.33,0.5,0,1.32,0"`). Faces = MakeHuman visemes / jaw / blinks + teeth / tongue / mouth bag baked into `assets/npc/*.glb` by `tools/npc_mpfb/add_face_morphs.py`; tracks in `assets/audio/vo/visemes.txt` from `tools/lipsync/bake_visemes.py` (re-run after adding a voice line) |
| `FOREST_VMPOSE=swing:<variant 0-3>:<progress 0-1>\|block\|ready\|sprint` | freeze the FP viewmodel in one pose so a still frames it exactly (`player/viewmodel.rs`); pair with `FOREST_FP=1`. `FOREST_VMHIDE=1` hides the viewmodel but keeps its camera lean, for A/B diffs |
| `FOREST_DEMO_PATH="x,z;x,z;…"` | explicit waypoint route for `FOREST_DEMO=explore` (overrides the lawn stroll / `FOREST_DEMO_TO` road run) — film footsteps / boot prints on a chosen ground (`demo.rs`). Pick routes with `FOOTING_PROBE="cx,cz,half,step" cargo test --release footing_probe -- --ignored --nocapture` (letter map of `footing::surface_at`) |
| `FOREST_PRINTTEST="x,z"` (+`_SCALE=<k>`) | park rows of boot prints (mud / sand / snow × left, right, sprint) + a blood control mark on the ground at a world XZ ~4 s in, for a still of the print look (`footprints.rs`, shader `ground_mark.wesl` `KIND_BOOT`); `_SCALE` blows them up k× to inspect the relief. Use with `FOREST_CAM` + `FOREST_NOBLUR=1` (DoF smears them). `FOREST_FOOTLOG=1` logs every hero footfall (surface, foot side, position) — check surface switching / left-right alternation from a clip's log (`footing.rs`) |
| `FOREST_DEMO_SPEED=<u/s>` | pace of the `FOREST_DEMO=explore` walk (default = the WASD walk, 1.4; `3.65` = the Shift sprint). The hero's gait (walk/run) is picked from speed, so this films any of them (`demo.rs`) |
| `FOREST_TPS=1` (+`_AZ`/`_PITCH` rad, `_DIST` units) | boot Play + **third-person** real follow-cam so a shot/clip frames the world like actual gameplay (NOT a god-cam `FOREST_CAM`); place the hero with `FOREST_HERO`, film a walk with `FOREST_DEMO=explore` (`player/mod.rs`, `player/camera.rs`) |
| `FOREST_LOADTEST=1` | hold the boot loading veil up (even under a capture) so it can be shot (`loading.rs`); pair with `FOREST_SHOT`+`FOREST_MENU=1` |
| `FOREST_PANEL=tree\|inv` | seed + open the upgrade-tree / satchel panel for a shot |
| `FOREST_EQUIP="sword_gold,gold_armor"` | equip the listed item ids at startup so the hero model shows its weapon/armor |
| `FOREST_FOG="clear,full"` | override fog distances live (no rebuild) |
| `FOREST_QUALITY=ultra\|high\|low` | startup graphics preset (`quality.rs`); `ultra` = demo showcase (visible god rays + maxed AA/AO/shadows) |
| `FOREST_AUDIOTEST` / `FOREST_GRADETEST` | isolate audio / reactive-grade for testing |
| `FOREST_FLOATTEST=1` | continuously stage sample floating combat numbers near the hero (style preview) |
| `FOREST_FLAGTEST=1` | park one cloth banner in open air at `(0, 6, -22)` to frame the flutter in isolation (`banner.rs`). NB the cloth streams along world ≈`(0.9, 0, -0.43)` — shoot from a spot perpendicular to that or it reads edge-on |
| `FOREST_BELLTEST=1` | re-toll the war bell on a ~12s loop (swing + clapper + SFX) so a shot/clip frames the ring without a keypress (`castle::swing_bell`); the bell stands at `castle::BELL_POS` (4.5, 7.5) |
| `FOREST_RIDE=<u/s>[,<turn rad/s>[,<heading rad>]]` | mount the **horse** with no keypress (as if pressing `L`) and drive it at that speed / turn rate from that heading (0 = +Z; gallop ≈ 4.9, Shift ≈ 7.2); under `FOREST_CLIP` the ride starts with the recording. Pair with `FOREST_TPS`+`FOREST_HERO`; a `FOREST_ORKLINE` ahead films the combat dismount + the horse rearing and bolting. `FOREST_HORSELOG=1` logs the horse's step tests and refused calls (`horse.rs`) |
| `FOREST_ROLLTEST=1` | re-arm the hero's **Alt dodge-roll** (forward, along the facing) on a ~1.8s loop so a `FOREST_TPS` shot/clip frames the somersault without a keypress (`player/movement.rs::player_roll`); skips the pointer-lock/stamina gates |
| `FOREST_GAIT=curves` | the hero's walk, run and dodge-roll are **Mixamo clips** by default (jump stays on the hand-built curves). The run = Mixamo "Running" (default settings) downloaded on the knight itself (the T-pose export `footman_obj` test auto-rigged on the Mixamo page, `downloads/mixamo/Knight Running.fbx`; only index-finger chains, so `mx.py` takes the hand frame from the hand bone; baked untouched: only `in_place` strips the travel, no sliders / mirroring / retiming; NOT solved by IK either — `mixamo::direct` puts each Mixamo bone's world turn from bind straight on the matching joint (`RUN_BONES`), the loop grounded once (`run_floor`) and exempt from `hero_anim`'s per-frame grounding, which squashed its flight into a squat); the Shift sprint (`SPRINT_MULT`) is set to exactly its native ~4.7 m/s so it plays 1:1 as on the Mixamo page, the walk fading straight into it; the bake's `sliders` add the rest (knee lift halved — the raw heel kicks to the seat; stride / arm swing / extra arm space, constants at the top of `bake.py`). `FOREST_RUN=curves` puts only the run back on the curves; `FOREST_VIEW=hero FOREST_VIEW_ANIM=run FOREST_VIEW_SPEED=<u/s>` previews any pace (`player/anim/mixamo.rs`, data baked by `tools/mixamo_retarget/bake.py` into `mixamo_clips.rs`, solved onto the footman like the UAL sword clips; the walk is an in-place loop played at ground speed / native stance speed so the boots stay foot-locked, fading into the run above 2.3 m/s). `curves` brings back the curve walk + procedural roll for A/B. **Orks** walk / charge on Mixamo "Orc Walk" / "Mutant Run" (same bake → `ORC_WALK`/`ORC_RUN`; `player/anim/ork.rs::mx_gait`: the clips own legs, pelvis and trunk, foot-locked to the ork's ground speed; the ork's wide toed-out stance, its arms + club and the blows stay procedural); `FOREST_ORK_GAIT=curves` brings back the hand-built ork gait. Independent of this, `hero_anim` ends with a **grounding pass** (the boot nearest the ground is put on it by shifting the hips, ±0.12 m) so no pose/overlay leaves the knight floating |
| `FOREST_SWINGTEST=1` | re-arm the hero's **attack chain** on a ~1.15s loop (inside the combo window, so it steps chop → slash → thrust; every 4th swing is the Heavy, released out of a held charge stance as in play) with no keypress, skipping the pointer-lock gate, so a `FOREST_FP`/`FOREST_TPS` clip frames every swing variant (`player/combat.rs::swing_test`). A number sets the gap between swing starts (`FOREST_SWINGTEST=0.3` = mashed, back-to-back chain). Side view of the sword arm: `FOREST_TPS=1 FOREST_HERO=-12,30 FOREST_TPS_AZ=4.6 FOREST_TPS_PITCH=0.12 FOREST_TPS_DIST=3.2 FOREST_NOBLUR=1 FOREST_CLIP_FPS=60` (no `FOREST_ORKLINE`: ringed foes dolly the follow-cam out). |
| `FOREST_NOSTREAM=1` / `FOREST_MEMREPORT=1` | spawn the kit undergrowth/clutter/props chunks for the WHOLE island at boot instead of streaming them within 95u of the camera (`realforest::stream_cover`; `FOREST_NOCULL=1` also bypasses it) / log one `MEM` breakdown 12 s after `WorldReady` — CPU-retained mesh bytes by owning component, top meshes/images (`perftest::mem_report`; always on under `FOREST_PERFTEST`) |
| `FOREST_SWORDKEEP="sink,stride,turn,lean,gaze,shield"` | look-dev override of how much of the UAL sword clips' body motion the hero's light swings keep (`player/anim/swordplay.rs::keep`; defaults 0.65,0.8,0.75,0.65,0.15,0.15 — lower reads timid, all-arm). Iterate in the viewer: `FOREST_VIEW=hero FOREST_VIEW_ANIM=swing1\|swing2\|swing3\|combo` + `FOREST_CLIP` (~25 s a run, no world build) |
| `FOREST_IMMORTAL=1` | the hero takes hits with full juice (floats/flash/shake) but can't drop below 1 HP, so a filmed melee never trips the **succession beat** — which slow-mos the world and swings the camera to the nearest townsperson, hijacking a combat clip's framing (`player/health.rs::apply_hero_damage`) |
| `FOREST_HURT_ALL=<frac>` | wound every foe / townsfolk to `frac` of max HP, then step it down 9% every 1.3s (4 steps, per-entity phase) so a still frames the world HP bars mid-fight with fresh **damage trails**; holds the hero on a matching 72%→48% staircase for the HUD vitals (`combat_fx::hurt_all_test`). Calm ring shot: `FOREST_TPS=1 FOREST_HERO=-12,30 FOREST_ORKLINE=-12,34 FOREST_TPS_AZ=2.9 FOREST_TPS_PITCH=0.5 FOREST_HURT_ALL=0.6 FOREST_IMMORTAL=1 FOREST_NOBLUR=1` (the hero spawns facing +Z, so the line must sit at +Z for the soft-lock ring) |
| `FOREST_LIFECHECK=1` (+`_SECS`, default 90) | headless-ish **people-collision test** for town life (`village_life.rs::life_check`): skips the menu, samples every villager every 0.5s from t=20s and flags anyone standing inside a prop, sitting inside / floating above / off a bench, overlapping another person, or walking through someone seated; exits `LIFECHECK PASS` (code 0) or `LIFECHECK FAIL n` + tally/states/per-seat pelvis averages. Run: `FOREST_TPS=1 FOREST_LIFECHECK=1 FOREST_DEMO=work cargo run` |
| `FOREST_VERTREPORT=1` | every 5 s log the index counts the main camera actually draws, bucketed by material / ground-cover / shadow / spawn-time `VisibilityRange` (`matcheck.rs`) — finds where the GPU vertex work goes. NB the saved `graphics.json` settings (e.g. render distance **Extreme**) apply to every perf run; force them with `FOREST_VIEWDIST`/`FOREST_QUALITY` for comparable numbers |
| `FOREST_IMPOSTORS=0` / `FOREST_IMPPARALLAX=0` / `FOREST_IMPPUSH=<radii>` / `FOREST_IMPFAR=<x>` | far-tree impostors (`realforest::impostor`, baked by `tools/blender_forest/impostors.py` → `PACK_ONLY=impostor` packer): off = the old LOD1-to-180u trees (A/B) / no depth-parallax step / quad push toward the eye (default 0.55) / draw distance × `TREE_FAR`×render distance (default 2.4 → 432u at Normal: chunks carry `NoFrustumCulling` and are side-plane culled in `cull_chunks`, since the camera far is 230u; `1` = the old bald-past-180u range) |
| `FOREST_PROPSLOD=0` / `FOREST_SKINLOD=0` | turn off the meshopt auto-LOD (`autolod.rs`): kit props chunks past 30u + biome static models past 36u draw simplified twins (`realforest::Tier::PropsFar`, `Kit::attach_static_lod`); skinned townsfolk / animals swap to an index-simplified twin past 30u (`anim_lod::swap_meshes`) — A/B (~-20% GPU in forest / village at Extreme) |
| `FOREST_WATERSSR=<k>` | water screen-space reflections strength (`water_ssr.rs`, High/Ultra; 0 = off): every water surface is the one `SEA_Y` plane, so a post pass finds water from depth and mirrors the lit scene / drawn sky into it, murk-tinted |
| `FOREST_FILL=<x>` / `FOREST_SUNK=<x>` / `FOREST_LOOK="vib,sat,contrast,toe,split,olive"` (+`FOREST_CLARITY`) | look-dev overrides: daylight shadow fill (ambient + IBL; default 1.5) / daylight sun key multiplier (default 1.4 — key vs fill ratio is what models forms instead of flat-lighting them, `scene.rs`) / the day film grade (`filmgrade.rs`; `olive` pulls pure greens toward olive/straw; `FOREST_CLARITY` = mid-radius local contrast, default 0.9 by day, only read inside a `FOREST_LOOK` override) |
| `FOREST_SHADOWLOD=0` | static `StandardMaterial` meshes keep casting sun shadows at any distance again (`shadow_lod.rs`: by default a caster drops its shadow past `radius × 35` (×2 on Extreme), e.g. a saguaro at ~230u — halved the far cascade 7.9k → 3.4k casters in a forest view). The `FOREST_PERFTEST` `farcascade` log lines show who fills the far cascade (avg/max distance + range band + parent tags). Skinned creatures (not the hero) also drop their shadow past 50u (`shadow_lod::cap_creature_shadows`, ×view-distance scale). Material churn per type logs as `MATCHURN` under `FOREST_PERFTEST` (`perftest.rs`) |
| `FOREST_ANIMBUDGET=0` | turn off the **animation budget** (`anim_budget.rs`, Unreal-style URO): each frame skinned players are evaluated every 1/2/3/4 frames by on-screen distance (16/32/55u), off-screen every 2nd (<12u) / 6th, staggered by entity; skipped ones are handed to the vendored `bevy_animation` (`AnimationTickSkip`, WARBELL PATCH in `vendor/bevy_animation`) so clocks keep running. ~-13–15% main thread in the village. Auto-off under `FOREST_LIFECHECK` |
| `FOREST_HEROMERGE=0` | per-part hero draws again (A/B): `player/footman.rs` merges the footman's parts per (joint, material) at load — 233 meshes → 57 draws (mirrored parts get their triangle winding flipped) |
| `FOREST_OCCLUSION=1` | GPU two-phase occlusion culling on the main camera (`quality.rs`, off by default: measured only ~0.2–0.3 ms GPU in the village, and the game is CPU-bound) |
| `FOREST_DYNRES=0` | **NB: on Bevy 0.20 render scale AND dynamic resolution are inert** — `MainPassResolutionOverride` is only read in the render world and nothing extracts it (DLSS inserts it there itself), and no pass upscales a sub-rect main pass anyway; a 0.6 still is exactly as sharp as 1.0. Needs a real offscreen-target + upscale implementation. Original behaviour: turn off **dynamic resolution** (`quality.rs::drive_dynamic_resolution`; also a Graphics-menu toggle, on by default): every 0.5 s, if the GPU pass sum (`RenderDiagnosticsPlugin` timers) exceeds 14.5 ms the main pass scale drops (floor 0.6× of the preset scale), and climbs back when under 75% of budget. Auto-off under `FOREST_SHOT`/`FOREST_CLIP`/`FOREST_RENDERSCALE` |
| `FOREST_TRAMPLE=0` / `FOREST_TRAMPLETEST="x,z"` | grass underfoot off (A/B) / keep a 2.5u disc trodden flat at a world XZ near the hero for a still (`grass_trample.rs`: undergrowth parts round the hero + nearest walkers and lies over where feet came down, one shared GPU buffer read by `foliage_wind*.wesl`) |
| `FOREST_FARCOVER=0\|<k>` | far-cover bake off (A/B) / blend strength (`realforest::far_cover`: every placed cover piece splats its mean card colour + area into an island map; `terrain.wesl` blends the ground toward it past the cards' range so far land isn't bald) |
| `FOREST_DISSOLVE=0` | undergrowth chunks pop at their range ends again instead of thinning out tuft by tuft (`foliage::dissolve`, A/B) |
| `FOREST_VERTSAMPLE=<tag substring>` | with `FOREST_VERTREPORT`: log 4 entities (+ root components) of the matching bucket, to feed `FOREST_WHOIS` (`matcheck.rs`) |
| `FOREST_ALLOCLOG=1` / `FOREST_STREAMLAG=1` / `FOREST_MESHSRC=1` / `FOREST_SPIKEWATCH=<ms>` | walking-hitch diags: log each frame whose render-world mesh upload (`allocate_and_free_meshes`, WARBELL PATCH in `vendor/bevy_render`) takes >2 ms, with MB + slab-growth split / log cover-stream units already inside their drawn range but not landed (`realforest::stream_cover`) / every 5 s group newly added meshes by owning component signature (`matcheck.rs`) / spike-log threshold (default 50). NB Bevy uploads a new mesh at ~0.7 ms/MB on the render thread, so cover streams **per tier** (each at its own drawn range + lead, `stream_radius`), lands ≤2 MB a frame (`LAND_BUDGET_BYTES`) and merges groups into ≤48k-vertex pieces (`PIECE_VERTS`) — landing whole chunks (20–40 MB) was the 15–28 ms hitch every half-second while walking |
| `BEVY_ASSET_ROOT` | point at this dir if running the binary from elsewhere (WGSL loads from `assets/shaders/`) |

## Architecture

### Two crates: pure logic vs. rendering

- **`crates/core` (`tileworld_core`)** — pure, deterministic, **zero-dep** game logic vendored
  from a ditched earlier port. Pathfinding (A*), wave director, upgrade/buff/resource/inventory
  stores, ork config, factions, RNG, shop catalog, etc. **`f64` throughout** to match JS `number`
  semantics for cross-language parity. This is the unit-tested validation spec — `cargo test` runs
  it. It has **no Bevy, no I/O, no rendering**. The Bevy front-end wraps these stores as Resources
  (e.g. `PlayerRes(core::Player)`, `Bank(core::ResourceState)`, `Inventory(core::Bag)`).
- **`tileworld_bevy_forest` (root `src/`)** — the Bevy app: rendering, ECS systems, input, the
  scene. Imports `tileworld_core` for all the numbers/logic that have to stay parity-correct.

When changing gameplay *numbers or rules*, prefer editing `crates/core` (and its tests) so parity
stays test-gated; `src/` should mostly *drive* core, not re-implement it.

### Plugin composition (`src/main.rs`)

Every `src/<feature>.rs` (and `src/player/`, `src/audio/`) is a self-contained `Plugin` that does
its own `Startup` spawn + `Update` systems. `main.rs` is just the assembly list — read it as the
table of contents (each plugin line has a one-line description of what it owns). Plugins are added
in several tuples because the `Plugins` trait maxes out at arity 15.

### The freeze-gate state machine (`src/game_state.rs`)

`AppState` = coarse mode (`StartScreen` / `Playing` / `Paused` / `GameOver`). `Modal` = a substate
that exists **only inside `Playing`** (`None` / `Shop` / `UpgradeTree` / `Inventory`). The entire
world-sim is gated on `run_if(in_state(Modal::None))`: opening any panel — or leaving `Playing` —
freezes the world but keeps rendering. Sim
systems carry the gate; render/camera/anim/audio/HUD systems stay ungated so the frozen world
still draws. New simulation systems **must** carry `.run_if(in_state(Modal::None))` or they'll run
through pauses/panels.

### World layout: biome viewer vs. the real island

Two related map systems share the ground pipeline in `biome.rs`:
- **`biome.rs` + `biome_<name>.rs`** — the biome *framework*. Each biome exposes `config()`
  (declarative ground/atmosphere/scatter/particles) + optional `landmarks()`. Keys **1–5** swap a
  single 32×32 biome patch at runtime (despawns everything tagged `BiomeEntity`, rebuilds).
- **`worldmap.rs`** — the actual playable island: generated at base
  resolution scaled up by `MAP_SCALE = 3.9` (→ `COLS 561 × ROWS 639`). Elliptical island, five
  biome blobs, grass safe-zone (castle), four rivers + lake, coastal mountain ridges, rolling
  terraced knolls, plateaus. `classify` force-flattens grass under every town build plot (and
  chest/cover placement rejects `town::near_build_plot`) so nothing occupies a future building's
  spot. `worldmap::build` also seeds castle/camps/ore/chests placement. **Generation runs in base
  space, drawn over the enlarged grid**, so the island shape is identical, just denser.

### Pathfinding through the castle (`src/navgrid.rs`)

Wraps core's tested A* (`tileworld_core::pathfinding`) onto forest's world-space terrain. Walls
register impassable collision boxes; **gate gaps register none**, so A* threads the gates with no
explicit gate-targeting code. Night-wave invaders follow `InvaderPath` waypoints to the keep.

### Save / load (`src/savegame.rs`) — six slots, snapshot-style

A save is a **logic snapshot, not an ECS dump**: the world is built once at `Startup` and is
persistent within a process, so we serialize the run-state *resources* (hero / economy / town /
upgrades / keep / heirs / night) plus a few world flags (looted chests, rescued camps, discovered
landmarks) to one JSON file per slot, and on load overwrite those resources + mark the
already-spawned entities.

**Slots**: index `0` is the **autosave** (`autosave.json`), `1..=MANUAL_SLOTS` (5) are the player's
manual slots (`save{n}.json`), all in the same OS data dir. A legacy single-slot `save.json` is
renamed into slot 0 once at boot. `SaveSlots` caches a `SlotMeta` per slot (timestamp / night /
playtime / level) so menus can label + order slots without re-reading disk each frame; refresh it
with `refresh_slots` after any write or delete. **New Game only ever deletes slot 0** — manual slots
survive a fresh run and stay loadable from the title's LOAD GAME.

Three write triggers, **all Prep-only**:

- **Dawn autosave** — `autosave_on_dawn` fires on the `Wave → Prep` edge (a cleared night) → slot 0.
- **Periodic autosave** — `autosave_tick` every `AUTOSAVE_INTERVAL` (600s) of `Playtime`, the
  pause-aware play clock (mirrors `siege::GameTime`: sim-gated + frozen while `SkyClock.paused`).
  A trigger during a night marks the save *owed* and lands the moment the night is cleared → slot 0.
- **Manual save** — the pause menu's **SAVE GAME** opens the slot picker, which sends
  `RequestSave(slot)`; `manual_save` writes while in `Prep` (greyed/refused during a siege).
  Allowed on day 1 (`wave_index == -1`).

All build the snapshot from one shared `SaveCtx` SystemParam (`snapshot()`), so there's a single
field list to keep in sync. The **slot picker** overlay (`game_state.rs`: `SlotPicker` +
`sync_slot_picker` / `slot_picker_click`, cloned from the `ConfirmWipe` pattern) serves both saving
(pause) and loading (pause **and** title). Loading: `load_slot` drops the file into `PendingLoad`,
`apply_pending_load` writes it back over the live resources the next `Playing` frame and emits
`GameLoaded`, then the entity-owning modules reconcile from that message (`town.rs` rebuilds
meshes, chests re-open, landmarks re-mark, **bosses despawn already-slain wardens**). Restore always
forces `phase = Prep` at the saved night — which is *why* saving mid-`Wave` is forbidden (a mid-night
`wave_index` would resume one night too late, skipping the fight).

**Invariant — when you add anything a player earns/changes across a run, it MUST round-trip the
save, or it's silently lost on Continue.** Concretely: put it in core stores where you can (those
already serialize via the core `serde` feature and ride `Player`/`Bag`/`Town`/`ResourceState`), and:

1. add the field to `SaveData` (bump `SAVE_VERSION` only on a *breaking* shape change — additive
   fields use `#[serde(default)]` so old saves still load);
2. read it in `SaveCtx::snapshot()` and write it back in `apply_pending_load`;
3. if it lives on **entities** (not a resource), reconcile it from the `GameLoaded` message in the
   owning module — and read the value off the carried `SaveData`, never live `PlayerRes`/etc., which
   `apply_pending_load` may write the same frame in undefined order.

Things deliberately **not** saved (fine — derived/transient): `siege::GameTime` (the night-timeline
clock; `savegame::Playtime` IS saved), timed `Buffs`, pickup `Toasts`, the
battlefield (invaders/bolts/corpses — swept on Continue), warden *levels* (re-level from 1), and
warden *kills* as such (the permanent boon flag on `Player` is the record — `boss::despawn_slain_wardens`
reads it to drop a beaten warden). `Lives.heirs` mirrors `town.population`, so it's saved via `Town`.

## Conventions that bite if you miss them

- **Coordinate frame is world-space with the castle at the ORIGIN** (deliberate — do NOT "fix"
  it). One tile = one world unit;
  `tile = floor(world + G)` where `GX/GZ` (in `worldmap.rs`) recentre the grid onto the origin.
- **Enemy combat numbers are at full core parity (NOT rescaled).** An earlier pass
  scaled ork HP/damage and wildlife damage *down* (~×0.35 HP, ~×0.5 dmg) for the smaller scene,
  but that left enemies far too soft against a hero already at full core power (core
  `PLAYER_BASE_DAMAGE` 25 + weapons/crit/levels). They're now read straight from core:
  `siege::base_hp` → `ork_config(v).hp` (grunt 254 / scout 136 / berserker 306 / shaman 201),
  `orks::ORK_DAMAGE`=24 (variant_melee derives the rest), `SHAMAN_BOLT_DAMAGE`=26, and
  `wildlife::predator_stats` bite damage → `animal_config(s).attack_damage` (wolf 12 … golem 28).
  Wildlife HP already came from core. Per-night HP growth is the `siege::WAVES` `hp_scale` table
  (1.1·1.15^n), unchanged. When wiring a new combat number from core, use it **as-is** — don't
  reintroduce the old rescale.
- **Despawn races are pervasive** — many systems race to reap the same entity (corpses, kills,
  wave clears). Always use `try_despawn` (not `despawn`) and `try_insert` (not `insert`) on
  entities that combat/AI/HP-bar systems might have already removed. This is load-bearing; bare
  `despawn`/`insert` will panic intermittently.
- **Death is a fade, not a pop** (`src/dying.rs`): kills insert `Dying{since}` and crumple over
  ~1.4s. Every AI/targeting/count query must filter `Without<Dying>` so corpses aren't targeted or
  counted — but reward/clear logic fires once pre-fade.
- **Anything a player earns or changes over a run has TWO obligations — persist it AND reset it.**
  Add any new run-state (resource, flag, or earned entity) to *both* paths or it breaks silently:
  1. **Persist** — round-trip it through the save (`src/savegame.rs`): `SaveData` +
     `SaveCtx::snapshot()` + `apply_pending_load` (reconcile from the `GameLoaded` message if it
     lives on entities). Miss this and it's lost on Continue — exactly how a run's level/items, and
     later the quest chain, went missing. See the **Save / load** section for the checklist + the
     deliberately-unsaved list.
  2. **Reset** — make sure **New Game** wipes it back to its fresh-boot value (resources via the
     `OnExit(StartScreen)`/`OnExit(GameOver)` reset systems or the in-process world rebuild;
     world entities via the rebuild's despawn). Miss this and a new run inherits the old run's state.
  Default rule of thumb: prefer holding it in a core store (those already serialize) and a resource
  that a reset system clears, so both obligations are one edit each.
- **Every voice/quote line carries its spoken text in a code comment.** When you wire up *any*
  spoken line (hero `voice.rs`, villager `audio/npc.rs`, ork `audio/ork.rs`, or future speakers),
  put the exact transcript next to where the clip is loaded / keyed, plus its trigger. The audio
  files are opaque `.ogg`s — these comments are our **only** record of what the game actually says,
  so we can retune *when/how often* lines play without re-listening to every clip. Keep them in
  sync when a clip is re-recorded; if a clip's text is unknown (older un-transcribed asset), say so
  explicitly (`[older clip — text not transcribed]`) rather than leaving it blank.
- **Mesh-building contract** (verified API forms in the doc cited below, §9): every prop mesh's
  base sits at
  `y = 0`; **colour lives in the mesh `ATTRIBUTE_COLOR`** (linear RGBA via `crate::palette::lin` /
  `lin_scaled`), because all props share one white `StandardMaterial` so the renderer auto-batches
  thousands of instances. Build parts as primitives, `tinted()` each (add COLOR) before
  `Mesh::merge`, and `duplicate_vertices()` + `compute_flat_normals()` for the crisp low-poly facet
  look (duplicate FIRST — `compute_flat_normals` panics on an indexed mesh). The verified Bevy
  0.18.1 API forms are in `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md` (the
  mesh-building API is unchanged in 0.19, so it still applies); the per-slice spec docs sometimes
  **guess wrong** about the Bevy API — trust the verified doc + the real Bevy source, now under
  `C:\Users\skibi\.cargo\registry\src\index.crates.io-*\bevy_*-0.19.1\src`.
- **Determinism**: scatter/placement uses `mulberry32` (core `rng.rs`) seeded per-tile, so the
  world is reproducible. "Feels the same" parity, not byte-exact RNG/map.
- **Forest's divergences are canonical, not bugs**: `siege.rs`'s wave director is richer than
  core's `wave.rs` and is the one in use; `KEEP_MAX_HP = 1500` / 12-per-s repair; the bespoke
  `Atmosphere`/IBL lighting; the custom CoC bokeh `dof.rs` over a plain DoF. Don't "restore" these
  to match core.
- **The render pipeline is SINGLE-CAMERA — do NOT add a second `Camera3d`.** The "proper" Bevy
  two-camera / `RenderLayers` first-person view-model pattern (the `first_person_view_model` example)
  was tried and **fails three ways here** (June 2026): (1) a 2nd `Camera3d` makes every
  `single()`/`Single<Camera3d>` query in the codebase ambiguous (`player_camera`, `drive_dof_focus`,
  bloom/dof/godrays/outline/grade/atmosphere…), so the main camera silently stops being driven and
  freezes at the boot overview pose; (2) a 2nd HDR+`Tonemapping` camera corrupts the main camera's
  output — Bevy issue #17530 — washing the world to flat sky; (3) the heavy prepass+post stack
  (Depth/Normal/MotionVector prepass + SSAO + DOF + godrays + outline + bloom) throws a hard **wgpu
  Validation Error → the app quits**. Don't re-attempt it. Anything "in front of the lens" (a FP
  weapon/arms view-model, a HUD-in-world overlay) must live in the ONE camera. **Oct 2026: first person now looks out of the REAL third-person rig** (`camera::fp_head_track`:
  eye 0.11 above the head joint — the joint is the neck pivot, and an eye on it made every foe look a
  head taller — head + upper-arm/pauldron meshes hidden, torso bent by look pitch, shoulders
  pushed forward/down in FP, the blocking shield's shoulder moved out/down so the board sits lower-left
  instead of over the screen centre; forearms + kit hidden at a run AND through a draw/sheathe reach
  (`Hero::kit_reach`); tune with `FOREST_FPHEAD="fwd,up"`, `FOREST_FPARM="fwd,down"`,
  `FOREST_FPGUARD="out,down"`, `FOREST_FP_PITCH=<rad>`; preview with `FOREST_VIEW_FP`). The hero root is X-mirrored (`HERO_SCALE_V`; hero materials cull FRONT)
  so the knight is right-handed. The viewmodel described next is legacy, hidden unless `FOREST_FPVM=1`.
  The (legacy) FP
  view-model (**rebuilt from scratch, Sept 2026** — the old one re-used the third-person rig's arms
  through a handedness mirror + Euler angles solved against a tilted hand frame, and was junk) is
  `player/viewmodel.rs`: purpose-built sword / shield / fist / forearm meshes
  (`model::build_viewmodel`) that are **children of the main camera** (`FpRoot`), rendered in the
  one world pass (`NotShadowCaster`) with its OWN material (`creature::make_viewmodel_material`):
  the world's sun/IBL knows nothing about the lens, so facing the sun used to leave near-black
  hands (median luminance 17/255). `creature.wgsl` adds an opt-in **camera-locked key light**
  (`params.w` = `viewmodel::VM_KEY_LUX`, half-lambert from up-left-front of the lens, exposure-scaled)
  and the FP gloves use lighter leather (`model::FP_GLOVE`, not the near-black `PGLOVE`). Any new
  colour used by a viewmodel mesh must be mapped in `model::surf_for` or it defaults to Metal. The whole third-person rig is simply
  **hidden in FP** (`camera::fp_body_visibility`, edge-triggered) and keeps playing its normal clips
  untouched — nothing in `anim::hero_anim` is FP-aware any more, so never re-add FP branches there.
  Poses are authored in **screen space**: `Kf { (ndc x, ndc y, depth), blade direction, roll }`
  (`sword_carry/ready/sprint/guard/tuck`, `shield_*`, and the four `swing()` tables with
  wind → mid → hit → end keys and per-swing camera lean), converted to camera space by `Lens`
  against `camera::FP_FOV_DEG`, so a swing is choreographed by where the blade is *on screen*.
  Roll convention: `roll = 0` shows the blade's broad flat to the sky/viewer (a horizontal slash's
  attitude) — held at rest that reads as a *paddle*, so the carry/ready/thrust/chop keys use a
  **negative roll (~-1.0 rad)**: flat toward screen-left, edges up/forward, crossguard seen end-on,
  fingers wrapping toward the viewer — a right hand's natural grip. Only the horizontal slash rolls
  back toward 0 (edge leading left).
  Forearms are a **rigid one-bone IK** (`solve_arm`: aimed wrist → elbow anchor behind/below the lens,
  never stretched — the sleeve mesh is long enough to stay off-frame), so an arm always connects to its hand. Weight comes from springs layered on the pose: look
  inertia, a hit-recoil kick off `HitFeedback::trauma`, jump/landing hop, stride bob, and a
  **wall-avoid** that tucks the weapon down/back when `blockers` has a solid within reach.
  `camera::fp_eye_rig` owns the eye: terrace-smoothed height (`FP_VERT_RATE`), landing dip spring,
  figure-eight stride bob, strafe roll, dodge-roll dip (the Alt roll now works in FP). The
  near-plane (`scene.rs::setup_camera`, `near: 0.04`) is lowered so a close-held weapon never slices
  it. FP swing lean is `FirstPerson::sway` (written by `animate_viewmodel`, applied post-`look_at`
  in `player_camera`; a lean, never a shake) plus an FP **close-quarters FOV widen**
  (`camera::FP_CLOSE_FOV_DEG`). A centre **reticle** (`viewmodel::spawn_reticle`/`sync_reticle`)
  blooms into a ring when a foe is ringed. Tune with the captures: `FOREST_FP=1` still,
  `FOREST_VMPOSE=swing:<0-3>:<p>|block|ready|sprint` freezes the pose, `FOREST_VMHIDE=1` hides the
  meshes but keeps the lean so `A − A_hidden` diffs isolate the viewmodel. Enemy **HP bars clamp into a
  view cone above the eye** (`combat_fx::HP_BAR_CONE_SLOPE` — the bar slides down toward the chest
  and pulls toward a close camera, shrinking) so a towering foe's bar stays on screen at FP mid-range.
  Inside `HP_BAR_NEAR` (4u) in FP **no world bar is drawn at all** (a bar pulled to the lens filled
  half the frame) — the ringed foe's health is a slim bar under the reticle instead
  (`viewmodel::ReticleTarget` also hides that foe's world bar).
  NB: FP melee inherently puts the enemy in your face — the widen softens it, but no view-model
  trick removes it; third-person is the design's combat view.
- **Capture-harness flakes — confirm the `Screenshot saved` log line, and retry before debugging.**
  For `FOREST_CLIP`, set `FOREST_CLIP_WARMUP=70`+ — the default 30 records the tail of the boot
  **loading veil** (`loading.rs`, lifts ~1.4s of sim time): the first ~50 saved frames render
  dimmed/black *behind an intact HUD* in every scene, which reads exactly like a full-frame
  rendering bug and is invariant to any code change (a whole session was once lost chasing it).
  A `FOREST_SHOT` run can emit a junk frame that is NOT a code bug: a **black** frame (cold pipeline,
  see the bevy-0.19 note) or an **overview/god-cam** frame (the follow-cam hadn't engaged yet under
  `FOREST_FP`/`FOREST_TPS`). And if the run **crashed** (e.g. a wgpu validation error) no new PNG is
  written, so you'll `Read` the **stale** previous file and misread it as "my change did nothing".
  Always grep the run output for `Screenshot saved` / `error[` / `panic` / `Validation` before
  trusting the image; if a frame looks wrong, re-run once before diagnosing it.
  **`FOREST_TPS` is NOT a fixed camera — never use it for an A/B comparison.** It boots the real
  follow-cam, which *orbits*, eases toward its anchor, and dollies OUT when a foe rings the hero
  (`OrbitCam::combat`) — and a hero staged with `FOREST_HERO` in a biome gets attacked during the
  warmup. Two runs with byte-identical env therefore land on different framings, which silently
  turns "before vs after" into "two different shots" (this invalidated a whole comparison round
  before anyone checked). Use `FOREST_CAM` for anything measured, and prove the pair really matches
  by diffing a patch of pure sky — a good pair diffs to <0.001.
  **For any A/B that MEASURES colour or luminance, the hero must be at FULL HP.** `grade.rs`'s
  reactive grade darkens the whole frame and drains ~25% global saturation below `LOW_THRESHOLD`
  (0.35 HP), modulated by a **wall-clock** `sin(now * 5.5)` heartbeat — so two "identical" shots
  taken seconds apart differ everywhere, including in pure sky, and a hurt hero can invert the
  sign of the effect you're measuring. A staged `FOREST_HERO` in a biome gets mauled by wildlife
  within the warmup, which is exactly how this bites. **`FOREST_IMMORTAL=1` does NOT save you** —
  it clamps to 1 HP, i.e. *maximum* dread grade. Use a free-cam (`FOREST_CAM`) with the hero left
  at his safe default spawn, and verify neutrality by checking that a patch of pure sky matches
  between the two frames before trusting any measured delta.

## Reference material

- `docs/superpowers/specs/2026-06-07-tileworld-parity-port-roadmap.md` — the P0–P6 roadmap + locked
  decisions + per-subsystem numbers. Read this before any gameplay-parity work.
- `docs/specs/` — extracted visual specs + the **verified** Bevy 0.18.1 API doc.
- `docs/superpowers/specs/*-design.md` — per-feature design docs (audio, hero, combat feedback,
  shaman spells, biome variety).

