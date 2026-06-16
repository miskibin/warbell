# Combat-feel pass — design

**Goal:** make fighting satisfying. Better impact SFX, better impact visuals, a punchier
hero swing, and — newly — a visible attack animation on every ork and creature (today they
just stand and the only feedback is the hero's HP bar dropping).

Reference for the audio split: the old web game `D:/tileworld/src/audio/sfx.ts`.

## A. SFX split (`audio/sfx.rs`, `audio/mod.rs`, `verbs.rs`)

Restore the old game's rule: **one flesh clip for living things, metallic clips for stone**,
each pitch-jittered so repeats don't sound canned.

Clip mapping (confirmed by file size; `sword-hit.ogg` is byte-identical to `sword-hit-2.ogg`):
- Flesh (creatures): `sword-hit-2.ogg` (old `var-2`).
- Metallic (stone): `sword-hit-1.ogg` + `sword-hit-3.ogg` (old `var-1`/`var-3`).

> **Update (2026-06-16):** the one-clip flesh rule above is superseded. `sword-hit-{1,2,3}.ogg`
> are now three fresh steel-on-target takes and the flesh impact picks one at random (still
> pitch-jittered). The metallic ore clinks moved to `ore-chip-{1,2}.ogg` (the old `var-1`/`var-3`),
> and the shield block now picks between `block-{1,2}.ogg`. The stale `sword-hit.ogg`/`block.ogg`
> duplicates were removed. See `SfxBank` in `src/audio/sfx.rs`.

Changes:
- `SfxBank.hits: Vec<…>` → `flesh: Handle` + `chips: Vec<Handle>`.
- `AudioCue::Impact{kill}` plays ONLY `flesh`, pitch-jitter ±0.08 (kill = ×0.85 lower, ±0.06).
- New `AudioCue::OreChip` → random metallic chip, pitch-jitter ±0.10, non-spatial one-shot.
- `mine_ore`: non-shatter swing emits `OreChip` (was `Impact{kill:false}`); shatter keeps
  the `OreShatter` synth sting (layered).

## B. Impact visuals (`player/combat.rs`) — stylized + blood

New impact-particle code lives in `combat.rs` beside `Spark`/`CombatFx`/`spawn_burst`/
`update_sparks` (its existing role). Extend `CombatFx` with: `blood` mat (dark crimson
`#8a1a1a`, unlit, NO bloom), `chip` mat (grey stone), `slash` mesh+mat (bright, billboard),
`ring` mesh+mat (annulus, bright), `splat` mesh+mat (flat dark-red disc). New short-lived
components + driver systems (timestamp/`now-born` driven so they freeze coherently in
hit-stop), registered in `PlayerPlugin` near `update_sparks`:

- **Directional blood spray** — creature hits: dark-red motes biased ALONG the blow `dir` +
  spread + up. ~8 motes/hit, ~16 on a kill. Replaces the gold spark burst for creatures.
- **Ground splat** — flat dark-red disc at the target's feet on hit/kill (~0.6 m, bigger on
  kill), alpha-fades over ~3.5 s then despawns. (Lifetime-capped so they don't pile up.)
- **Slash-arc flash** — bright camera-facing quad at the hit point, oriented along the blow,
  scale-pops + fades in ~0.12 s.
- **Kill shockwave ring** — flat bright annulus at a kill, expands + fades ~0.25 s.
- **Ore chips** — `mine_ore` spawns a small GREY stone-chip spark burst per swing (reuses the
  spark physics); bigger grey debris burst on shatter. Gold sparks stay for ore-less reuse.

Blood intensity: moderate (dark crimson, no glow). Tunable.

## C. Hero animation (`player/anim.rs`, `player/combat.rs`)

- **Snappier swing** — re-curve `attack_arm_quat`: ease-in windup → fast ease-out sweep so
  peak blade speed lands at `HIT_PHASE`, then settle. Slightly bigger `LIFT`/`SWEEP`.
- **Weapon trail** — during the sweep, sample the blade tip (`ArmR` `GlobalTransform` ×
  local `(0,-0.5,0.96)`) each frame and emit fading translucent ghost quads (a smear). Reuses
  the slash-flash fade component. Isolated/last — droppable if it fights the rig.

## D. Enemy attack animations (`orks.rs`, `wildlife.rs`)

Attackers: 4 ork variants (Grunt/Scout/Berserker melee, Shaman bolt) + 6 predators (Wolf,
PolarBear, Boar, Scorpion, BogCroc, Golem).

**Shared mechanism:** add `atk_anim: f32` (timestamp of last strike, `0` = none) to `Ork` +
`Animal`, set in the existing strike/bite branch. The limb systems (`ork_limbs`/`animal_limbs`)
compute `p = (now − atk_anim)/DUR` clamped 0..1; while `< 1` they overlay a strike pose blended
over the gait. Same virtual clock the limbs already use → freezes coherently in hit-stop.
**Reactive** (plays on the blow) so AI timing/balance is untouched. (Telegraph = later follow-up.)

Strike poses (keyed by the body parts the model already has):
- **Ork club chop** (right arm): raise-back → drive forward past rest → recover over ~0.35 s.
  Berserker bigger/faster.
- **Shaman cast**: raise→jab thrust on cast (replaces the static raised staff) + a small green
  charge-mote at the staff tip.
- **Biters** (Wolf, PolarBear, Boar, BogCroc): head snaps down-forward fast then back, legs
  tuck, + a brief forward body-lunge nudge.
- **Scorpion**: tail whips up-over-forward (sting); no head bite.
- **Golem**: slow heavy overhead — whole-body forward pitch (longer DUR); arm slam if it has
  Arm parts.
- **Fallback**: forward body-lunge for any attacker missing the expected part.

## Files touched
`audio/mod.rs`, `audio/sfx.rs`, `player/combat.rs`, `player/anim.rs`, `verbs.rs`, `orks.rs`,
`wildlife.rs`.

## Verification
Bevy rendering/animation/audio — no meaningful pure-logic unit tests. Verify with
`cargo check`/`cargo build`, then run the app / screenshot harness (`FOREST_SHOT`) and fight.

## Risks
- Blood shifts tone toward gore (opted in).
- Splat accumulation → capped by short fade + despawn.
- Weapon-trail cost / rig-fighting → isolated, cuttable.
