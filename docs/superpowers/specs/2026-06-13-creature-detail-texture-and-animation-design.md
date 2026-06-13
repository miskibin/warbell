# Creature Detail: Procedural Surface Texturing + Animation Enrichment

**Date:** 2026-06-13
**Scope:** Hero, orks (8 variant×faction), and the 13 wildlife species — give them
surface texture and richer life/combat/locomotion animation **without** breaking the
single-material batching contract or introducing UV maps / image textures.

Props, trees, scatter, buildings, terrain are **out of scope** — they keep their own
white vertex-colour material and their batching is untouched.

---

## 1. Problem

Every animated actor (hero `player/model.rs`, orks `orks.rs`, wildlife `critters.rs`) is
built from primitive boxes/cones/spheres, merged per articulated part into one flat-shaded
mesh, coloured purely by baked `ATTRIBUTE_COLOR`, and drawn against **one shared white
`StandardMaterial`**. They read as clean low-poly silhouettes but **flat** — no surface
grain (fur, scale, stone, metal, hide all look identical matte plastic), and the animation
is a single sin-gait limb swing + strike pose, so idle creatures feel inert.

Two asks:
- **Surface texture / detail** — fur vs scale vs stone vs metal vs hide should read
  differently, with micro-relief, while staying crisp at gameplay zoom.
- **Animation** — more liveliness (idle), combat polish, and locomotion quality.

Hard constraints (CLAUDE.md / CONTRACT.md):
- Colour lives in the mesh `ATTRIBUTE_COLOR`; all actors share ONE material so the
  renderer batches them. Real per-model image textures need UVs + per-model materials →
  break batching → tank FPS in a 100+ ork siege.
- Meshes are merged, `duplicate_vertices()` + `compute_flat_normals()` for hard facets.
- `combat_fx.rs` already clones a **per-entity** `StandardMaterial` for each ork/animal
  (hurt-flash mutates `.emissive`); the hero has its own `HeroMaterial`.

---

## 2. Decisions (locked with the user)

1. **Detail method:** procedural WGSL shader on a shared `ExtendedMaterial` — *not* real
   image textures, *not* per-surface materials.
2. **Surface-type encoding:** packed into the **unused vertex-colour alpha channel**
   (currently always `1.0`). No custom vertex attribute, no vertex-layout specialization.
3. **Texture intensity:** **subtle / tasteful** — reads up close + in screenshots, stays
   clean at gameplay zoom, preserves the flat-shaded low-poly aesthetic.
4. **Animation:** all three areas — liveliness/idle, combat polish, locomotion.
5. **Order:** infra → hero → orks → 13 animals; visually verify each before the next.

---

## 3. Architecture: the shared procedural material

### 3.1 The material

Mirror the existing terrain pattern (`terrain.rs` already ships
`ExtendedMaterial<StandardMaterial, ForestExtension>`):

```rust
pub type CreatureMaterial = ExtendedMaterial<StandardMaterial, CreatureExt>;

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct CreatureExt {
    #[uniform(100)]
    pub params: CreatureParams, // intensity knobs (texture strength, relief, etc.)
}
// fragment_shader() -> "shaders/creature.wgsl"
```

A new `creature.rs` module owns the type, the `MaterialPlugin::<CreatureMaterial>`, and a
`make_creature_material()` builder. Hero + orks + wildlife each swap their white
`StandardMaterial` creation for `make_creature_material()`. **One handle shared across all
three rigs** → same batching behaviour the white material had today.

Base of the extended material keeps `base_color: WHITE` (vertex colour carries hue),
`perceptual_roughness` ~0.7, opaque.

### 3.2 Surface-type packed in vertex alpha

`ATTRIBUTE_COLOR` is currently linear RGBA with `a = 1.0`. Repurpose alpha as a **surface
code**:

```rust
#[derive(Clone, Copy)]
pub enum Surf { Skin, Fur, Scale, Stone, Metal, Cloth, Bone }
// -> alpha code in {0.05, 0.20, 0.35, 0.50, 0.65, 0.80, 0.95}  (banded, mid-band sampled)
```

`lin()` / `lin_scaled()` stay unchanged (default → `Skin`/neutral). The three model modules
gain a thin wrapper — e.g. a `surf(mesh, Surf)` that overwrites the alpha of every vertex,
or the `bx/cone/cyl/...` helpers grow an optional surf arg. Each model part is tagged with
the surface family that fits it (plate→Metal, tusk→Bone, fur trim→Fur, …). **Untagged =
the current look**, so the surf pass is purely additive.

Because the model meshes are low-poly (few verts) the alpha is constant across each
primitive — surface type is per-primitive, which is exactly the granularity we want.

### 3.3 The fragment shader (`assets/shaders/creature.wgsl`)

Fragment-only extension (no custom vertex shader — keeps full PBR reuse, lowest risk):

1. Build the standard PBR input from the StandardMaterial base + interpolated vertex data
   (`pbr_input_from_standard_material`), exactly as a normal StandardMaterial would.
2. Read **raw vertex colour**: `in.color.rgb` = hue (use as base), `in.color.a` = surf code.
3. Compute a **model-space sample coordinate** locked to each animated part (so grain does
   NOT swim/crawl as the creature walks or a limb swings). Cheap, no 4×4 inverse, exact for
   our rotation + uniform-scale instance transforms:
   ```wgsl
   let m = mat3x3(world_from_local[0].xyz, world_from_local[1].xyz, world_from_local[2].xyz);
   let s2 = dot(m[0], m[0]);                       // uniform scale²
   let origin = world_from_local[3].xyz;
   let obj = transpose(m) * (in.world_position.xyz - origin) / s2;  // part-local position
   ```
   (`world_from_local` from `bevy_pbr::mesh_functions` via `in.instance_index`.)
4. **Per-surface procedural texture** from cheap 3D value-noise at `obj`:
   - **Fur** — stretched/streaked noise along the part's long axis; soft luminance bands.
   - **Scale** — cellular (Voronoi-ish) cells, slight per-cell darken at edges.
   - **Stone** — broadband mottle + sparse bright speckle, extra roughness.
   - **Metal** — very low-amplitude noise, *reduced* roughness + small spec lift.
   - **Skin/Hide** — soft low-freq mottle, faint pores.
   - **Bone/Cloth** — fine grain / weave.
5. Apply **subtly**: perturb base-colour luminance by a small ± (cap ~±8–12%), nudge
   `perceptual_roughness`, and optionally perturb the normal by a tiny derivative of the
   noise for micro-relief. Then run standard PBR lighting (`apply_pbr_lighting`) so
   IBL/sun/fog/exposure all stay correct.
6. **Force output alpha = 1.0** so the repurposed alpha never leaks into transparency.

Intensity is global via the `CreatureParams` uniform (one knob to dial the whole look,
and to tune from the F1 debug panel during review).

### 3.4 combat_fx + hero plumbing (the integration work)

Retype the per-entity skin clone + flash from `StandardMaterial` to `CreatureMaterial`:
- `combat_fx.rs`: `HurtSkin(Handle<CreatureMaterial>)`; `ensure_ork_skin`,
  `ensure_animal_skin`, `hurt_flash` query/mutate `Assets<CreatureMaterial>` and set
  `m.base.emissive` for the whiten-flash. Eyes keep their own emissive `StandardMaterial`
  (already skipped).
- `player/mod.rs`: `HeroMaterial(Handle<CreatureMaterial>)`; hero spawn + `reskin_hero`
  build/clone the creature material. Weapon/shield children share the hero's handle.
- Any other site that assumes these rigs use `StandardMaterial` (e.g. `combat_fx`
  squash/spring only touches `Transform`, so unaffected; verify `succession_fx`,
  `aftermath`, dying fade — the death fade in `dying.rs` clones a fade material, check its
  type).

**Risk:** the death-fade in `dying.rs` may clone the actor material to fade alpha. If so it
must clone the `CreatureMaterial` and fade `.base.base_color` alpha (and the shader's
forced opaque output must respect base alpha for the dying actor, or dying uses a separate
fade path). Resolve in the infra phase spike.

---

## 4. Animation enrichment (procedural, no skeleton)

All three rigs share the same limb pattern (`ork_limbs`, `animal_limbs`, hero limb system),
so add a small **shared `anim` helper module** of pure functions they each call from their
existing per-frame limb systems. No new ECS systems where avoidable; no skeletal rig.

1. **Head-look** — when idle (and not striking), rotate the `Head` part's yaw/pitch toward
   the hero (or camera if hero absent/far) within a clamp, eased. Biggest liveliness win.
   Applies to orks + wildlife; hero looks toward nearest threat/lock target.
2. **Idle micro-motion** — low-freq weight-shift sway + a breathing pulse (small head/torso
   bob already partly exists; extend to a gentle whole-body breathe). Gated off `HitSquash`
   and `Dying` so it never fights the hit-spring or the death crumple.
3. **Turn-lean** — bank the root Z slightly into facing changes (from the per-frame facing
   delta the brains already compute). Cheap, sells momentum.
4. **Locomotion** — counter-rotate the torso/head against the leg gait; widen swing
   amplitude for run states (flee / charge / berserker frenzy) vs walk. Keep foot-arc from
   hip rotation (no knee joint added).
5. **Combat polish** — refine the strike anticipation/follow-through easing (the
   `club_chop_x` / `head_bite_x` / `arm_slam_x` / `tail_sting_x` curves), add a hero block
   stance hold, and add variety to the `dying.rs` crumple (topple direction / speed by
   variant so deaths don't look identical).

These are layered onto the existing rotations (compose, don't replace) so the current gait
and strike behaviour is preserved and enriched.

---

## 5. Execution plan (phased, each visually verified)

**Phase 0 — Infra + spike.** Add `creature.rs`, `creature.wgsl`, `CreatureMaterial`,
`make_creature_material()`. Swap the three creation sites + retype combat_fx/hero/dying.
Ship with the shader doing the model-space-noise reconstruction but **no surf tags yet**
(all `Skin`), so output ≈ current look. **Goal: it compiles, batches, flashes, fades, and
renders identically — proving the pipeline before any model edits.** Resolve the dying-fade
question here. Verify: `cargo run` + a normal shot + a siege shot (perf sanity).

**Phase 1 — Hero.** Tag `player/model.rs` parts (plate=Metal, blade/axe/maul=Metal,
leather/cloth=Cloth, mail=Metal, hide=Skin, plume=Cloth). Add block stance + idle
micro-motion + head-look. Verify: `FOREST_EQUIP=...` + `FOREST_HERO` shots, each weapon/armor.

**Phase 2 — Orks.** Tag `orks.rs` parts (skin=Skin, tusks/bone=Bone, club wood=Skin/Bone,
loincloth/war-paint=Cloth, metal bits=Metal) for all 4 variants. Add head-look + idle +
turn-lean. Verify: `FOREST_ORKLINE="x,z"` (one of each variant in a line).

**Phase 3 — Wildlife (13 species).** Sweep `critters.rs`: fur for mammals, scale for
croc/scorpion, stone for golem, hide for camel/boar, bone for antlers/horns/tusks. Add
per-species liveliness (head-look, ear/tail life, idle). Verify per biome with `FOREST_SHOT`
+ `FOREST_HERO="x,z"` staged in each biome region, and a wildlife close-up.

Each phase is a commit; push when the phase verifies (CLAUDE.md push rule).

---

## 6. Verification

Windows dev machine → the `FOREST_SHOT` screenshot harness runs locally:
```
$env:FOREST_SHOT="shot.png"; $env:FOREST_ORKLINE="0,-6"; cargo run
```
Staging hooks used: `FOREST_ORKLINE` (ork line-up), `FOREST_EQUIP` + `FOREST_HERO` (hero),
`FOREST_HERO="x,z"` + `FOREST_BIOME` (animals in-biome), `FOREST_WAVE`/`FOREST_DEFEND`
(siege perf sanity). Compare before/after shots each phase. The F1 debug panel exposes the
`CreatureParams` intensity knob for live tuning during review.

Perf gate: a `FOREST_WAVE` siege must hold frame-rate — the extended material is one shared
handle (plus the existing per-entity hurt clones), and the procedural noise is a few hash
ops over the small screen area creatures cover, so cost should be negligible. Confirm with
the F2 perf overlay.

---

## 7. Out of scope / non-goals

- No image textures, no UV unwrapping, no per-model materials.
- No skeletal/keyframe animation rig — procedural limb math only.
- Props/trees/scatter/buildings/terrain unchanged.
- No change to gameplay numbers, HP, AI, or combat balance.
