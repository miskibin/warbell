---
name: studio-port
description: Port (or re-port) a character model and/or its animations from the user's three.js "Low-Poly Knight Studio" project into this Bevy game. Use when the user drops a new `low-poly-knight-studio` zip/folder, says "reuse/port the studio walk/run/jump/attack animations", "swap the model", "the model/anim is new again", or asks to update the hero/ork/peasant/guard look or motion to match the studio. Covers the three.js→Bevy geometry+Euler mapping, the rig-joint contract, the per-clip animator, the look gotchas (futuristic "pipes", washed colors, bucket helmet), and the FOREST_VIEW verify loop.
---

# Porting models & animations from the three.js studio

The user authors characters in a separate three.js app ("Low-Poly Knight Studio") and periodically
hands over a new export to port into this Bevy game. The studio is **source of truth**; reproduce it
**precisely** (the #1 complaint is "you changed things / it's uglier — be more precise"). It is a
mechanical transcription, not a redesign — until the user asks for a *natural-look* deviation.

## Where everything lives

| Studio (three.js) — `C:\Users\skibi\Downloads\low-poly-knight-studio (N)\src\` | Bevy target |
|---|---|
| `utils/knightBuilder.ts` — `buildKnight()` (geometry) + `updateKnightAnimation()` (all anims) | `src/player/model.rs` (geometry) + `src/player/anim.rs` (animator) |
| `utils/orcBuilder.ts`, `utils/peasantBuilder.ts`, `utils/sitSeat.ts` | (orks/villagers — port only when asked) |
| `types.ts` — `AnimationType`, `ModelType` | the clip set to support |
| `App.tsx` `useState<CustomizationOptions>` — **default colors** | the colour consts |
| (rig built in `buildKnight` lines ~133-220) | `src/player/mod.rs` `Joint` enum + `spawn_hero_meshes` |

Read the WHOLE builder (it's ~2000 lines, paginate). The knight branch is gated by
`if modelType==='orc' … else if isWorker … else { /* knight */ }`.

## 1. Rig contract (`src/player/mod.rs`) — must match the studio joint frames EXACTLY

The animator numbers only port 1:1 if the joint local-offsets match the studio's `Group` tree.
**Re-read the rig block (`buildKnight` ~lines 155-245) EVERY export — the frames change between
versions** (e.g. one export had head `scale 1.4` / shoulders `(∓0.30,0.25,0)` / hipJoint `(∓0.16,…)` /
foot `scale 1.25`; a later one switched to head `scale 1.0` / shoulders `(∓0.20,0.27,0.01)` / hipJoint
`(∓0.035,…)` close-set legs / foot `scale 0.96`). `head.scale`/`footShape.scale` ride as a **joint
spawn scale** (anim only ever writes rotation/translation, never scale, so it persists). The held
weapon gets its **own `Joint::Sword` pivot** (the studio animates `broadsword.rotation` every attack);
the shield is `Joint::Shield` (position **and** rotation animated, incl. its idle/gait/block offsets).
Calibrate the one `rig` y-offset so feet sit on the ground, then verify visually.

**New `Hero` fields** (e.g. `attack_variant`, `victory`) are transient → no save, but MUST be added to
EVERY `Hero { … }` literal or it won't compile: `spawn_hero` + `reset_player` (mod.rs) **and**
`viewer.rs::setup`. Also wire the preview hooks (`mod.rs::animtest`, `viewer.rs::anim_drive`).

## 2. Geometry (`src/player/model.rs`) — three.js → our helpers

three.js local matrix is `T*R*S`; our `part(mesh, scale, rot, off, color)` is the same order. One Bevy
mesh per joint (`group(vec![...])` merges + flat-shades). Per-part colour via vertex `ATTRIBUTE_COLOR`.

| three.js | helper |
|---|---|
| `CylinderGeometry(rt,rb,h,seg)` | `frustum(rt,rb,h,seg)` (`rt==0`→cone) |
| `BoxGeometry(w,h,d)` | `cuboid(w,h,d)` |
| `SphereGeometry(r,…)` (incl. half-domes) | `ball(r)` (full ico; hidden half is fine) |
| `ConeGeometry(r,h,seg)` | `cone(r,h,seg)` |
| `TorusGeometry(R,t)` | `torus(R,t)` |
| **partial** cylinder (`thetaStart`/`thetaLength`, e.g. a visor) | `arc_shell(rt,rb,h,θ0,θlen,segs)` — custom, double-sided |
| `ExtrudeGeometry(Shape)` (heater shield) | `extrude_poly(&pts, depth)` — sample the quadratic beziers with `quad()` |
| nested `THREE.Group` (fauld, backplate, plume) | `node(m, cscale, crot, coff, prot, poff, c)` |
| `.position` / `.scale` / `.rotation`(EulerXYZ) | `off` / `scale` / `xyz(x,y,z)` (single axis → `rx/ry/rz`) |

Custom meshes (`arc_shell`/`extrude_poly`) must carry POSITION+NORMAL+UV_0 (so `group()`'s `merge`
works; flat-normals recomputes them). Colours: copy App.tsx hexes verbatim into the consts; add each to
`surf_for()` (`Cloth` for cloth/leather/plume, `Skin` for glow, else `Metal`).

## 3. Animation (`src/player/anim.rs`) — `updateKnightAnimation` switch → per-clip `Pose`

The studio resets every joint to rest each frame, then a `switch(animationType)` case sets some. Mirror
exactly: `rest()` seeds a full `Pose` table; each clip fn mutates the fields it touches; the system
writes `Pose.get(joint)` onto the rig. `obj.rotation.set(x,y,z)` → `e3(x,y,z)` (=`Quat::from_euler(XYZ)`).

Clips to port for the knight: idle, walk, run, jump, defend, attack1/2/3, victory. **Game adaptations**
(keep poses verbatim, only re-source the driver):
- walk/run `cycle` ← `walk_phase` (gait locked to movement speed), cross-faded by `moving_amt`/`run_amt`.
- jump: studio fakes height via `hips.y`; here the **physics root owns height**, so recover studio
  `height` from `vel_y` (`h = 1-|vel_y/JUMP_SPEED|`) and feed the airtime formulas.
- attack: one-shot `attack_t/ATTACK_DURATION` split into the studio wind/strike/recovery, strike-start
  aligned to combat `HIT_PHASE`; pick `attack_variant` (0/1/2) randomly in `combat.rs` on swing start.
- defend: eased by a smoothed `block_amt`; **brace UPPER body only** — legs keep walking (else they
  freeze and the body slides; see Gotchas).

## Gotchas (each one bit during a real port)

- **Re-port from scratch each export** — a new version can replace the WHOLE model (one export went
  plate+glowing-eyes → mail hauberk + plate cuirass + surcoat w/ heraldic cross + open-faced bascinet
  with a **human skin face** + exposed skin forearms/hands) and re-proportion the rig. Don't patch the
  last port; re-read `buildKnight` + `updateKnightAnimation` fresh and diff. New materials appear too
  (e.g. `mailMat 0x383c42`, `skinMat 0xb98562`, `darkCoatMat 0x22242b`) — add consts + `surf_for` arms.
- **`ExtrudeGeometry` from a tapered `Shape`** (surcoat/cuirass panels) → `extrude_poly` on the sampled
  outline points (reverse to CCW). A `.rotation.y = PI` panel just faces -Z; `extrude_poly` is a closed
  solid so it shows either way.
- **Be precise** — don't invent lightened/darkened shades or "improve" proportions; the user notices
  immediately. Use the studio's exact hex/values.
- **Visor = front shell, not a full band.** A 360° cylinder for the visor makes an ugly bucket head;
  port the studio's `thetaStart/thetaLength` partial cylinder via `arc_shell`.
- **Block while moving must keep the legs walking** — brace only torso/arms/shield; gate the leg-brace
  by `(1-moving_amt)`. Frozen legs + moving root = "sliding on the ground".
- **"Too futuristic / metal pipes"** = bare cylinder limbs through an open groin. Cover with hanging
  cloth (front+rear fauld/skirt), drop thin cloth-underlayer "piston" tubes, smooth/full-en the thigh.
- **Animated weapon** needs `Joint::Sword`; a static leaf can't sweep through an attack.
- **Flat-shaded ceiling**: all characters share one creature material for batching — hues will match
  the studio but the PBR metalness sheen won't. Say so; only push hue-contrast if the user okays a
  deviation.
- **Windows rebuild "Access is denied (os error 5)"** = a stale capture exe holds the binary;
  `Get-Process tileworld_bevy_forest | Stop-Process -Force` then rebuild.

## Verify loop (REQUIRED — the user judges by sight)

Build, then use the **model-viewer** skill: `FOREST_VIEW=hero` + `FOREST_VIEW_ANIM=`
`idle|walk|run|jump|defend|attack1|attack2|attack3|victory|blockwalk` with `FOREST_SHOT` (still) or
`FOREST_CLIP` (motion). Capture front + a **low angle** (the user's common complaint angle) + side +
each anim, `Read` the PNGs, iterate. Tight cams clip the overhead apex of attacks/jumps — pull back.
Don't commit until the user signs off on the look.
