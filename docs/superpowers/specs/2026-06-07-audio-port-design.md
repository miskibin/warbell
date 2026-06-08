# Audio port — old `D:/tileworld` → `tileworld-bevy-forest`

Port the old web game's full audio bank (music, combat SFX, hero voice lines, ork/monster
voices, UI stings, biome narration, footsteps, campfire/water ambience) into the Bevy forest
viewer, **keeping** the curated wildlife voices + ambience already added to this game.

## Decisions (locked)

- **Scope:** wire every sound that has a real trigger, AND add small new hooks (footsteps,
  campfire ambience, camp alert, biome narration, UI feedback) so otherwise-homeless sounds get
  a home. Genuinely homeless SFX (economy/wave/menu) are copied into the asset bank but parked.
- **Codec:** transcode all `.mp3`/`.wav` → `.ogg` (ffmpeg, libvorbis). Bevy 0.18 default
  features decode ogg only; keeps one codec, matches existing assets.
- **Music:** background bed (`hurdy-gurdy-hymn`) + combat layer (`day-combat`) that swells when
  any ork is hunting/attacking the hero, ducking the bed. No day/night or wave phases exist.
- **Footsteps:** yes — per-step audio tied to hero movement + biome surface (dirt/snow/stone).
- **Architecture:** event-driven `audio/` module split (approach A). Gameplay code emits a
  single `AudioCue` event (or sets a music-state flag); the audio module owns all playback.

## Keep (this game's own — never overwritten)

`camel`, `deer-1/2`, `goat-1/2`, `rabbit`, `dog-1/2`, `cat-1/2`, `bear-growl`, `bear-roar`,
`forest-ambient`, `wind`, `water`. Old animal dupes (`cat-meow-*`, `dog-bark-*`) are NOT ported.

## Module layout (`src/audio/`)

`audio.rs` (240 LOC) becomes a folder:

| file | owns |
|------|------|
| `mod.rs` | `GameAudioPlugin`, `AudioConfig` (mix knobs), `AudioCue` event, `MusicState` resource, `Surface` enum |
| `sfx.rs` | one-shot SFX bank + `play_cues` consumer (combat feedback, UI, footsteps, spatial ork voices) |
| `voice.rs` | hero "one-mouth" voice — swing grunt / hurt / death / biome line; gated so he never talks over himself |
| `music.rs` | bed + combat-swell crossfade |
| `ambience.rs` | biome/water loops (moved) + spatial `campfire-loop` attached to each campfire |
| `footsteps.rs` | emits `AudioCue::Footstep(surface)` on each hero step |
| `wildlife.rs` | existing spatial animal voices (moved verbatim) |

## Event / trigger map

**Faithful to the old game** — conditions/volumes ported from `sfx.ts` + `Character.tsx` +
`playerStore.ts` + `Ork.tsx`, NOT simplified. All sampled bases × `audioMix.voice` (0.6).

| Trigger (file) | Cue | Clip(s) | Spatial | Base vol |
|---|---|---|---|---|
| swing wind-up (`combat.rs`) | `HeroGruntSwing` only | `player-swing-1/2` | no | 0.4 (×voice; **34% chance** + canGrunt) |
| swing resolves (combat.rs) | one of: `Impact{kill}` / `Impact{}` / `Swing` | `sword-hit` (hit/kill) **or** `sword-swing` (whiff only) | no | kill 0.62 / hit 0.50 / whoosh 0.30 |
| hit absorbed by shield (`health.rs`) | `Block` | `block` | no | 0.45 (only when actually blocked) |
| hero jump (`movement.rs`) | `HeroJump` | `player-jump-1` | no | 0.28 (×voice; **40% chance** + canGrunt; no other jump sfx) |
| hero takes dmg (`health.rs`) | `HeroHurt` | `player-hurt-1/2/3` | no | 0.45 (canGrunt-gated) |
| hero dies (health.rs) | `HeroDeath` | `player-death-1/2` | no | 1.0 ×narration |
| ork aggro / Hunt→Attack edge (`orks.rs`) | `OrkGrunt(pos)` | `ork-grunt-1/2/3`, `monster-snarl/growl` | yes | 0.55 ×voice |
| hero enters camp clearing (`orks.rs`) | `OrkRoar(pos)` | `wave-start-roar`, `ork-roar` | yes | 0.50 ×voice |
| any ork Hunt/Attack (orks.rs) | `MusicState.fighting=true` | `music-combat` swell, ducks bed | — | — |
| biome switch 1–5 (`biome.rs`) | `HeroLine(biome)` + `UiSelect` | `vo/{forest,snow,rock,desert,swamp}`; `menu-select` | no | 0.57 / 0.22 |
| gait half-cycle / landing (`footsteps.rs`) | `Footstep{surface,landing}` | `footstep-{dirt,snow,stone}` | no | 0.144, landing ×1.2 |
| campfire present (`ambience.rs`) | (loop) | `campfire-loop` | yes | bed |

`canGrunt` = ≥1.6 s since last grunt AND not mid-line. Whoosh fires ONLY on a whiff (never with
an impact). Footsteps are phase-locked to the gait (sprint-aware), not a fixed timer. Surface
from biome: Snow→snow, Rocky→stone, else→dirt.

## Parked (copied to `assets/audio/`, not loaded — no trigger in this game)

`chest-open`, `gold-pickup`, `shop-open`, `level-up-orchestra`, `menu-theme`,
`orc-march-tallow`, `soot-banner-dread`, `ability-cast`, `player-death-scream`,
`player-attack-grunt`. Documented here so a later milestone can wire them.

## Mix knobs (`AudioConfig`, live-tunable via F1 panel)

Existing: `ambience_vol`, `audible_range`, `call_min/max` (wildlife) — unchanged.
New: `sfx_vol` (0.5), `voice_vol` (0.6), `music_vol` (0.22), `narration_vol` (0.57),
`combat_music` (0.9). All scale their category at the point of playback.

## Non-goals

No economy/wave/menu systems invented to host parked sounds. No new combat mechanics — only
audio reacts to existing triggers. Hero voice stays non-spatial (head-locked), creature voices
stay spatial.
