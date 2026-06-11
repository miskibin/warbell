---
name: trailer-maker
description: Record and assemble a marketing trailer / promo GIFs for Warbell (the Bevy game). Use when the user asks for a trailer, gameplay video, itch.io clips, GIFs, a release video, or "record/film the game". Covers the in-engine scene director (FOREST_DEMO), the frame-sequence capture harness (FOREST_CLIP), per-scene SFX encoding, and stitching scenes into a titled, scored trailer — plus the gotchas (frame-locked timing, long warmups, SFX-only clips, the amix music-burying bug).
---

# Making a Warbell trailer

The Bevy window can't be screen-captured externally and can't take live input headlessly, so we
**script the actors in-engine** (a "demo director") and record with a **frame-sequence harness**
(one PNG per frame), then stitch with ffmpeg. The hard-won rule: **clips carry SFX only; music,
titles, and crossfades are added in post.** That keeps each clip reusable (GIFs, store page) and
lets the trailer's music breathe across cuts.

Pipeline: **stage a scene → capture frames → encode one scene clip (with SFX) → assemble the
trailer (titles + music) → publish.**

## 0. Where the pieces live

- **Scene director:** `src/demo.rs` (`FOREST_DEMO` modes) + a couple of hooks in `src/town.rs`
  (`demo_build_timelapse`, `demo_work_setup`) and `src/siege.rs` (`siege_clip_refill`).
- **Capture harness:** `src/capture.rs` — `FOREST_CLIP` (frame recorder) and `FOREST_SHOT`
  (single PNG). Exposes the `ClipProgress` resource (`recording: bool`, `frame: u32`).
- **Encode/assembly scripts:** this skill's `scripts/` dir — `build_trailer.ps1` (assembly),
  `encode_scene.ps1` (generic per-scene SFX encoder). Copy them into `promo/` (gitignored) and
  adapt the event tables per clip. `promo/` is the scratch dir for frames, clips, and the trailer.
- **Audio assets:** `assets/audio/*.ogg` (+ `vo/hero`, `vo/npc`, `vo/ork`). See the map in §5.

## 1. The scene director (`FOREST_DEMO`)

Under `FOREST_CLIP` the hero boots in `PlayMode::FreeRoam`, where `player_move`/`player_camera`
early-return — so the demo systems own the hero pose and camera, while the **ungated `hero_anim`
still animates limbs** from the `Hero` fields (`moving`/`walk_phase`/`attacking`/`attack_t`). Set
`FOREST_DEMO` to pick a scenario:

| Mode | What it stages | Pair with |
|---|---|---|
| `explore` | hero walks a scenic path behind a chase-cam; world stays alive | `FOREST_CLIP` |
| `build` | raise the stronghold one piece at a time (walls→gate→towers→defenses + plots/houses) | `FOREST_CLIP FOREST_CLIP_ORBIT=...` |
| `work` | working village (woodcutters fell trees + haul, miners cart stone, farms); over-the-shoulder track on one woodcutter | `FOREST_CLIP` |
| `talk` | cycles the funny sarcastic townsfolk barks as captions (real VO plays) | `FOREST_CLIP` |
| `rescue` | hero frees a caged captive; the real `camp_rescue` line fires | `FOREST_CLIP` |
| `defend` | night siege: courtyard guards + knight + sustained horde; low orbiting battle-cam | `FOREST_CLIP FOREST_WAVE=1 FOREST_DEFEND=1 FOREST_TOWN=1 FOREST_EQUIP=sword_gold,gold_armor` |

To **add a new scene:** add a `Some("name")` arm in `DemoPlugin::build` (`src/demo.rs`) that
registers a `*_drive` system (own the camera + actors), gate sim systems with
`run_if(in_state(Modal::None))`, and put camera/caption writes in `PostUpdate` so the audio
director's ambient barks can't clobber your chosen caption (`Subtitles::force` locks it).

## 2. Capturing frames (`FOREST_CLIP`)

```powershell
$env:FOREST_DEMO="work"; $env:FOREST_CLIP="target/clips/workf"
$env:FOREST_CLIP_WARMUP="2600"; $env:FOREST_CLIP_FRAMES="420"; cargo run
```

Knobs (read once at startup): `FOREST_CLIP_FRAMES` (150), `FOREST_CLIP_FPS` (30),
`FOREST_CLIP_WARMUP` (30), `FOREST_CLIP_ORBIT="cx,cy,cz,radius,height,deg_per_sec"`. A clamped
fixed timestep keeps motion smooth despite the per-frame PNG-encode stall.

### THE TIMING GOTCHA — read this before scripting any scene

Clip "elapsed seconds" is **wall-clocked** (each frame stalls on the PNG encode), so it is **NOT
1:1 with playback time.** Any scripted beat keyed off `time.elapsed_secs()` will fire at the wrong
moment in the stitched clip. **Use the frame-locked counter instead:** read
`ClipProgress.frame` (recorded-frame count, 0 during warmup) and divide by the playback FPS.
`ClipProgress.recording` is false during warmup — hold actors still until it flips true.

### WARMUP must be generous — it's not just shaders

Warmup lets the GPU pipelines and IBL/lighting settle (~30 frames) **but also lets the world
reach the state you want to film:**
- `work` — woodcutters must walk out and start swinging: **~2600** warmup frames.
- `defend` — the night sky has to darken and the horde has to mass: **~1500**.
- `explore`/`talk`/`rescue` — short (150–700) is fine.

If a scene opens "empty" or "too bright", the warmup was too short. (Frames render fast during
warmup — no PNG encode — so a big warmup is cheap.)

### Inspect frames as you go (fail fast)

Read sample PNGs from the clip dir (`frame_00010.png`, mid, last) with the Read tool **before**
encoding 400 of them into a clip. Cheaper to recapture than to re-edit.

## 3. Encoding one scene (SFX only)

Use `scripts/encode_scene.ps1` as a template: it loops a bed (ambient + fires/wind) and lays
discrete SFX events on a timeline (`@{f=...; t=seconds; v=gain}`). **No music here.** Keep the
mix `amix=...:normalize=0` (see §4) and end with `alimiter=limit=0.9`.

Per-scene SFX recipes that worked:
- **explore** — `forest-ambient` bed, footsteps optional (the chase-cam reads as ambient).
- **work** — `forest-ambient` + faint `castle-ambient`; `chop-wood-{1,2,3}` on the woodcutter's
  ~2.1s cadence until the tree topples, then a `dog`/`goat` for village life.
- **defend** — `wind` + `campfire-loop` bed; `wave-start-roar` at the cut; **real ork VO**
  (`vo/ork/charge`, `vo/ork/taunt`, `vo/ork/death_2`, `vo/ork/gate`), `ork-grunt-{1,2,3}`,
  `ork-roar`, and knight `sword-swing`→`sword-hit-{1,2,3}` pairs on his ~0.67s cadence + a
  `block` + `player-attack-grunt`. Layer ~20 events so it reads as a battle, not three clinks.

Stitch frames to mp4 inside the encode script:
`ffmpeg -framerate 30 -i frames/frame_%05d.png ... -c:v libx264 -crf 19 -pix_fmt yuv420p`.

## 4. Assembling the trailer (`scripts/build_trailer.ps1`)

Takes the per-scene mp4s (in a fixed order), and adds: **xfade** crossfades, **drawtext**
titles (font `assets/fonts/EBGaramond.ttf`), and the **music stem** — a calm bed
(`music-bed.ogg`) under the early scenes that crossfades into the night siege track
(`soot-banner-dread.ogg`) for the finale.

Two things that WILL bite:

1. **`amix` normalization buries the music.** ffmpeg's `amix` default `normalize=1` divides every
   input by the input count, so a 2-input SFX+music mix halves both — the music vanishes under the
   SFX. **Always `amix=inputs=N:normalize=0`** and set explicit per-stem `volume=`. This was the
   single cause of "there is no music in this scene." After building, **verify**:
   `ffmpeg -ss <end-10> -i trailer.mp4 -af volumedetect -f null NUL` — the last scene's
   `mean_volume` should be roughly **−10 to −14 dB**, not −25.
2. **`soot-banner-dread` is quiet at the head.** Its loudest stretch is **~69.5s in**; trim to
   that (`atrim=69.5:...`, `asetpts=PTS-STARTPTS`) or the night music is inaudible even at
   `normalize=0`.
3. **xfade offsets are cumulative:** scene *i* starts at `Σ(dur[0..i]) − i·XF`. The script
   computes this; don't hand-tune.
4. **drawtext font path on Windows:** escape the drive colon (`C\:/Users/...`) inside the filter
   arg or ffmpeg parses it as an option separator.

The default scene order is `explore → build → work → talk → rescue → defend` with titles
"Explore the Wilds / Forge a Stronghold / Tend the Realm / Mind the Locals / Free the Captured /
Hold the Night".

## 5. Audio asset map

| File | Use |
|---|---|
| `music-bed.ogg` | calm day music (trailer bed under scenes 1–N-1) |
| `music-combat.ogg` | **day** combat layer — NOT the night track (a past mistake) |
| `soot-banner-dread.ogg` | **night siege** track; loudest ~69.5s in — this is the finale music |
| `orc-march-tallow.ogg` | boss march |
| `wave-start-roar.ogg` / `war-bell.ogg` | siege start stingers |
| `wind`/`forest-ambient`/`castle-ambient`/`campfire-loop` | ambience beds |
| `chop-wood-{1,2,3}` | axe on tree |
| `sword-swing` + `sword-hit-{1,2,3}` + `block` | knight melee |
| `ork-grunt-{1,2,3}` / `ork-roar` / `monster-snarl` | ork punctuation |
| `vo/ork/*` | real ork barks (charge/taunt/gate/meat/death…) — transcripts in `src/audio/lines.rs` |
| `vo/npc/*` | townsfolk VO (the sarcastic talk-scene lines: `screaming`/`statue`/`optimist`) |

Spoken-line transcripts live as code comments next to where each clip is keyed
(`src/audio/lines.rs`) — match captions to the actual VO so subtitle and audio agree.

## 6. Publishing

```powershell
gh release upload <tag> promo/tileworld-trailer.mp4 --clobber -R miskibin/tileworld-bevy-forest
```

Remote preview (this box = Tailscale `mskibin-newera`): serve `promo/` over a loopback HTTP
server and `tailscale serve --bg 8099` it, share the `…ts.net/` URL. `promo/` is gitignored —
the deliverables live on the release + that URL, not in the repo.

GIFs for itch.io: `ffmpeg -i scene.mp4 -vf "fps=15,scale=480:-1:flags=lanczos" -loop 0 scene.gif`.

## Checklist for a fresh trailer

1. `cargo build` (capture needs the real binary).
2. For each scene: stage env → `FOREST_CLIP` capture → **Read sample frames** → encode with SFX.
3. Long warmups for `work`/`defend`; frame-lock every scripted beat.
4. Assemble: titles + xfade + music with `normalize=0`; **volumedetect-verify the last scene**.
5. Eyeball the final by extracting frames (`ffmpeg -ss <t> -i trailer.mp4 -frames:v 1 chk.png`).
6. Upload to the release; refresh the Tailscale preview.
