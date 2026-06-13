# Creature Detail: Texture + Animation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the hero, orks, and 13 wildlife species procedural surface texture (fur/scale/stone/metal/hide) and richer idle/combat/locomotion animation, without breaking single-material batching or adding UV maps / image textures.

**Architecture:** A new shared `ExtendedMaterial<StandardMaterial, CreatureExt>` (same pattern as `terrain.rs`) replaces the three white `StandardMaterial`s the rigs use. Surface type is packed into the unused vertex-colour **alpha** channel; a fragment shader reads it and applies cheap model-space procedural noise per surface family — subtly. Animation is enriched via a shared module of pure procedural helpers (head-look, idle micro-motion, turn-lean) layered onto the existing sin-gait limb systems. Props/trees/scatter/terrain are untouched.

**Tech Stack:** Bevy 0.18.1, WGSL (`bevy_pbr` extension shaders), Rust. Verification via the `FOREST_SHOT` screenshot harness (Windows, runs `cargo run`).

**Spec:** `docs/superpowers/specs/2026-06-13-creature-detail-texture-and-animation-design.md`

**Verification reality:** This is graphics work — most steps verify with `cargo check` (compiles) then `cargo run` + `FOREST_SHOT` (renders correctly), not unit assertions. Pure-logic helpers (the `Surf`→alpha map, anim math) DO get unit tests. Shader API forms must be checked against `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md` and the real Bevy source under `C:\Users\skibi\.cargo\registry\src\index.crates.io-*\bevy_pbr-0.18.1\src` — the verified doc wins over guesses.

**Key existing facts (verified during planning):**
- `terrain.rs` already ships `ExtendedMaterial<StandardMaterial, ForestExtension>` with `#[uniform(100)]` + `MaterialPlugin` — copy this pattern.
- `dying.rs` is **transform-only** (shrink/sink/tip); it never touches materials. ⇒ forcing the shader to output opaque is safe; no fade carve-out needed.
- `combat_fx.rs` already clones a **per-entity** material for each ork/animal (hurt-flash mutates `.emissive`). These clones + the hero's `HeroMaterial` must retype to `CreatureMaterial`.
- `palette::lin(hex) -> [f32;4]` and `lin_scaled(hex, v) -> [f32;4]` (alpha = 1.0 today).
- Three `Armory::new` callers: `camps.rs:279`, `siege.rs:529`, `ork_fortress.rs:975`. All must pass the new material type or it won't compile.
- Threading path for in-world rigs: `biome.rs:307` (system, has `ResMut` access) → `worldmap::build` (`worldmap.rs:708`) → `camps::build` (`camps.rs:229`), `wildlife::populate` (`wildlife.rs:788`), `ork_fortress::build` (`ork_fortress.rs:336`).

---

## File Structure

- **Create** `src/creature.rs` — `CreatureMaterial` type alias, `CreatureExt` (`AsBindGroup`), `CreatureParams` uniform, `Surf` enum + `surf_code()` + `surf()` mesh helper, `CreaturePlugin` (registers `MaterialPlugin`), `make_creature_material()` builder. One clear responsibility: the shared creature material + surface tagging.
- **Create** `assets/shaders/creature.wgsl` — the fragment extension shader.
- **Create** `src/creature_anim.rs` — pure procedural animation helpers shared by all three rigs (head-look, idle micro-motion, turn-lean, run amplitude).
- **Modify** `src/main.rs` — register `CreaturePlugin`.
- **Modify** `src/player/mod.rs` — `HeroMaterial`, `spawn_hero`, `spawn_hero_meshes`, `reskin_hero` retype.
- **Modify** `src/player/model.rs` — surf tags on knight parts.
- **Modify** `src/orks.rs` — `Armory` retype; surf tags; wire anim helpers into `ork_limbs`.
- **Modify** `src/critters.rs` — surf tags on all 13 species.
- **Modify** `src/wildlife.rs` — `populate`/`spawn_one` retype; wire anim helpers into `animal_limbs`.
- **Modify** `src/camps.rs`, `src/siege.rs`, `src/ork_fortress.rs` — pass `CreatureMaterial` to `Armory::new`.
- **Modify** `src/worldmap.rs`, `src/biome.rs` — thread `&mut Assets<CreatureMaterial>` to the in-world spawners.
- **Modify** `src/combat_fx.rs` — `HurtSkin`, `ensure_ork_skin`, `ensure_animal_skin`, `hurt_flash` retype to `CreatureMaterial`.
- **Modify** `src/dying.rs` — per-variant topple variety (combat polish).

---

# PHASE 0 — Infra spike (NO visual change; prove the pipeline)

Goal: swap the rigs onto `CreatureMaterial` with the shader doing model-space-noise reconstruction but **all surfaces = Skin** (≈ current look). Prove it compiles, batches, flashes, fades, and renders the same before any model edits.

## Task 1: `creature.rs` — material type, `Surf`, helpers

**Files:**
- Create: `src/creature.rs`
- Modify: `src/main.rs` (add `mod creature;` near the other `mod` lines)

- [ ] **Step 1: Write the module**

```rust
//! Shared creature material: an `ExtendedMaterial<StandardMaterial, CreatureExt>` that all
//! animated rigs (hero, orks, wildlife) draw against — replacing their plain white
//! `StandardMaterial`s. Hue still lives in `ATTRIBUTE_COLOR.rgb`; the **alpha** channel now
//! carries a per-vertex SURFACE CODE that `assets/shaders/creature.wgsl` reads to apply a
//! subtle procedural texture (fur/scale/stone/metal/hide/bone/cloth) in model space, plus a
//! per-surface roughness/spec response. Props/trees/scatter keep their own white material —
//! this is creatures only.

use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

const CREATURE_SHADER: &str = "shaders/creature.wgsl";

pub type CreatureMaterial = ExtendedMaterial<StandardMaterial, CreatureExt>;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct CreatureParams {
    /// x = texture strength (luminance ±), y = micro-relief (normal perturb),
    /// z = metal spec lift, w = spare.
    pub params: Vec4,
}

#[derive(Asset, AsBindGroup, Clone, TypePath, Debug)]
pub struct CreatureExt {
    #[uniform(100)]
    pub params: CreatureParams,
}

impl MaterialExtension for CreatureExt {
    fn fragment_shader() -> ShaderRef {
        CREATURE_SHADER.into()
    }
}

/// Surface family a mesh primitive reads as. Packed into the vertex-colour alpha so the shader
/// can branch its procedural texture. `Skin` is the neutral default (≈ the old flat look).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surf {
    Skin,
    Fur,
    Scale,
    Stone,
    Metal,
    Cloth,
    Bone,
}

/// The alpha value (`0..1`) that encodes a surface family. Mid-band values so the shader can
/// read it with a tolerant `floor(a*7)` style bucket and never sit on a band edge.
pub fn surf_code(s: Surf) -> f32 {
    match s {
        Surf::Skin => 0.07,
        Surf::Fur => 0.21,
        Surf::Scale => 0.36,
        Surf::Stone => 0.50,
        Surf::Metal => 0.64,
        Surf::Cloth => 0.79,
        Surf::Bone => 0.93,
    }
}

/// Rewrite every vertex's colour-alpha of `mesh` to the surface code for `s`, leaving rgb (the
/// hue) untouched. Call AFTER the mesh has its `ATTRIBUTE_COLOR` set and BEFORE it is merged
/// into a group (merge concatenates the attribute, so tagging per-part-primitive then merging
/// preserves per-primitive surfaces). No-op if the mesh has no colour attribute.
pub fn surf(mut mesh: Mesh, s: Surf) -> Mesh {
    use bevy::render::mesh::VertexAttributeValues as V;
    let code = surf_code(s);
    if let Some(V::Float32x4(cols)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for c in cols.iter_mut() {
            c[3] = code;
        }
    }
    mesh
}

/// Build the shared creature material. All rigs call this; combat_fx then clones it per-entity
/// for the hurt-flash (unchanged behaviour, now on this type).
pub fn make_creature_material(mats: &mut Assets<CreatureMaterial>) -> Handle<CreatureMaterial> {
    mats.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE, // vertex colour rgb carries the hue
            perceptual_roughness: 0.7, // per-surface response is applied in the shader
            ..default()
        },
        extension: CreatureExt {
            // strength 0.10 (subtle), relief 0.25, spec-lift 0.35.
            params: CreatureParams { params: Vec4::new(0.10, 0.25, 0.35, 0.0) },
        },
    })
}

pub struct CreaturePlugin;

impl Plugin for CreaturePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CreatureMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surf_codes_are_distinct_and_in_range() {
        let all = [
            Surf::Skin, Surf::Fur, Surf::Scale, Surf::Stone, Surf::Metal, Surf::Cloth, Surf::Bone,
        ];
        let mut codes: Vec<f32> = all.iter().map(|s| surf_code(*s)).collect();
        for c in &codes {
            assert!(*c > 0.0 && *c < 1.0, "code {c} out of (0,1)");
        }
        codes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in codes.windows(2) {
            assert!(w[1] - w[0] > 0.08, "codes {:?} too close to bucket apart", w);
        }
    }

    #[test]
    fn surf_rewrites_only_alpha() {
        let mut m = bevy::prelude::Mesh::from(bevy::math::primitives::Cuboid::new(1.0, 1.0, 1.0));
        let n = m.count_vertices();
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.2, 0.4, 0.6, 1.0]; n]);
        let m = surf(m, Surf::Metal);
        if let Some(bevy::render::mesh::VertexAttributeValues::Float32x4(cols)) =
            m.attribute(Mesh::ATTRIBUTE_COLOR)
        {
            for c in cols {
                assert_eq!(&c[0..3], &[0.2, 0.4, 0.6]); // hue untouched
                assert!((c[3] - surf_code(Surf::Metal)).abs() < 1e-6);
            }
        } else {
            panic!("color attribute lost");
        }
    }
}
```

- [ ] **Step 2: Add the module declaration**

In `src/main.rs`, add `mod creature;` alongside the other top-level `mod` declarations (e.g. near `mod critters;`).

- [ ] **Step 3: Run the unit tests**

Run: `cargo test -p tileworld_bevy_forest creature::tests`
Expected: 2 tests PASS (`surf_codes_are_distinct_and_in_range`, `surf_rewrites_only_alpha`).
(If the crate name differs, use `cargo test creature::tests` — the package is the root crate.)

- [ ] **Step 4: Commit**

```bash
git add src/creature.rs src/main.rs
git commit -m "feat(creature): shared CreatureMaterial type + Surf vertex-alpha encoding"
```

## Task 2: `creature.wgsl` — fragment extension (identity baseline)

**Files:**
- Create: `assets/shaders/creature.wgsl`

This first version reconstructs model-space position and is wired for per-surface branching, but with strength near-zero it renders ≈ the StandardMaterial baseline. Task 9 fills in the per-surface texture.

- [ ] **Step 1: Write the shader**

```wgsl
// Procedural creature surface texturing — a StandardMaterial fragment EXTENSION.
// Reads vertex-colour rgb as hue and vertex-colour alpha as a SURFACE CODE, samples cheap
// model-space value-noise, and subtly perturbs base colour / roughness / normal per surface
// family. PBR lighting (sun/IBL/fog/exposure) stays exact. Output is forced opaque so the
// repurposed alpha never leaks into transparency.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    mesh_functions,
    forward_io::{VertexOutput, FragmentOutput},
}

struct CreatureParams { params: vec4<f32> };
// VERIFIED against bevy_pbr-0.18.1: use the injected bind-group index (NOT @group(2)); terrain.wgsl
// does the same. `mesh_functions::get_world_from_local(instance_index)` exists; `in.instance_index`
// is guaranteed because pbr_fragment.wgsl uses it unconditionally (sets VERTEX_OUTPUT_INSTANCE_INDEX).
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> creature: CreatureParams;

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * 0.3183099 + vec3(0.1, 0.2, 0.3));
    let r = q + dot(q, q.yzx + 19.19);
    return fract((r.x + r.y) * r.z);
}

// Smooth 3D value noise in [0,1].
fn vnoise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let c000 = hash3(i + vec3(0.0, 0.0, 0.0));
    let c100 = hash3(i + vec3(1.0, 0.0, 0.0));
    let c010 = hash3(i + vec3(0.0, 1.0, 0.0));
    let c110 = hash3(i + vec3(1.0, 1.0, 0.0));
    let c001 = hash3(i + vec3(0.0, 0.0, 1.0));
    let c101 = hash3(i + vec3(1.0, 0.0, 1.0));
    let c011 = hash3(i + vec3(0.0, 1.0, 1.0));
    let c111 = hash3(i + vec3(1.0, 1.0, 1.0));
    let x00 = mix(c000, c100, u.x);
    let x10 = mix(c010, c110, u.x);
    let x01 = mix(c001, c101, u.x);
    let x11 = mix(c011, c111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Model-space coordinate locked to this (possibly animated) part: exact for the rigs'
    // rotation + uniform-scale instance transforms, no 4x4 inverse.
    let model = mesh_functions::get_world_from_local(in.instance_index);
    let m = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let s2 = max(dot(m[0], m[0]), 1e-5);
    let origin = model[3].xyz;
    let obj = (transpose(m) * (in.world_position.xyz - origin)) / s2;

    let surf = in.color.a;        // surface code (see Surf::surf_code)
    let strength = creature.params.x;

    // Per-surface texture amount (Task 9 specialises per family; baseline = gentle mottle).
    let n = vnoise(obj * 14.0) - 0.5;
    let lum = 1.0 + n * strength;

    var rgb = pbr_input.material.base_color.rgb * lum;
    pbr_input.material.base_color = vec4<f32>(rgb, 1.0); // force opaque — alpha was the surf code

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
```

- [ ] **Step 2: Verify shader imports against real Bevy**

Open `docs/specs/bevy-0-18-1-polished-static-3d-scene-verified-apis.md` and `assets/shaders/terrain.wgsl` (already-working extension) and the registry source `bevy_pbr-0.18.1/src/render/{pbr_fragment.wgsl,mesh_functions.wgsl,forward_io.wgsl}`. Confirm:
  - `pbr_input_from_standard_material(in, is_front)` exists and the signature matches.
  - `mesh_functions::get_world_from_local(instance_index)` exists (else use the name the source defines, e.g. `get_world_from_local`); `in.instance_index` is a real `VertexOutput` field.
  - `FragmentOutput` import path.
Fix import paths/function names inline if the verified source differs. **Do not guess — match the source.**

- [ ] **Step 3: Commit (compiles after Task 5 wires a user of the material)**

```bash
git add assets/shaders/creature.wgsl
git commit -m "feat(creature): procedural surface fragment shader (baseline)"
```

## Task 3: Register `CreaturePlugin`

**Files:**
- Modify: `src/main.rs:134` area (next to `terrain::TerrainPlugin` / `water::WaterPlugin`, which also register materials)

- [ ] **Step 1: Add the plugin to the first `add_plugins` tuple**

In the `.add_plugins((...))` block that contains `terrain::TerrainPlugin` and `water::WaterPlugin`, add:
```rust
            creature::CreaturePlugin, // registers the shared creature ExtendedMaterial
```
(Keep the tuple ≤ 15 entries — this block currently has 14; if it would exceed, add to the next tuple instead.)

- [ ] **Step 2: Type-check**

Run: `cargo check`
Expected: compiles (no user of `CreatureMaterial` yet besides the plugin — that's fine).

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(creature): register CreaturePlugin material"
```

## Task 4: Retype the hero onto `CreatureMaterial`

**Files:**
- Modify: `src/player/mod.rs` (`HeroMaterial` :107, `spawn_hero` :195, `spawn_hero_meshes` :266, `reskin_hero` :306)

- [ ] **Step 1: Retype `HeroMaterial`**

```rust
#[derive(Resource)]
pub struct HeroMaterial(pub Handle<crate::creature::CreatureMaterial>);
```

- [ ] **Step 2: `spawn_hero` — build the creature material**

Replace the `materials.add(StandardMaterial {...})` block (lines ~202-207) and the `materials: ResMut<Assets<StandardMaterial>>` param:
```rust
    mut materials: ResMut<Assets<crate::creature::CreatureMaterial>>,
```
```rust
    let mat = crate::creature::make_creature_material(&mut materials);
    commands.insert_resource(HeroMaterial(mat.clone()));
```
(The hero's old `metallic: 0.3` sheen is recreated later via the `Metal` surf code in the shader.)

- [ ] **Step 3: `spawn_hero_meshes` — retype the `mat` param**

```rust
    mat: &Handle<crate::creature::CreatureMaterial>,
```
The `MeshMaterial3d(mat.clone())` calls are unchanged (generic over the material type).

- [ ] **Step 4: `reskin_hero` — retype `meshes`/material access**

The `mat: Option<Res<HeroMaterial>>` already holds the new handle type; no body change beyond it compiling. Confirm `spawn_hero_meshes(..., &mat.0)` still type-checks.

- [ ] **Step 5: Type-check**

Run: `cargo check`
Expected: `src/player` compiles. (combat_fx still references `StandardMaterial` for hero? It does not — hero isn't in combat_fx's ork/animal queries. Hero hurt feedback is its own path.)

- [ ] **Step 6: Commit**

```bash
git add src/player/mod.rs
git commit -m "refactor(hero): draw knight against shared CreatureMaterial"
```

## Task 5: Retype the ork `Armory` + its 3 callers + thread the material

**Files:**
- Modify: `src/orks.rs` (`Armory` :968, `Armory::new` :978, `spawn`/`spawn_prop` `MeshMaterial3d` calls)
- Modify: `src/camps.rs` (`build` :229, mat :241, `Armory::new` :279)
- Modify: `src/siege.rs` (`setup_invader_armory` :519)
- Modify: `src/ork_fortress.rs` (`build` :336, `ork_mat` :416, `Armory::new` :975)
- Modify: `src/worldmap.rs` (`build` :708; calls to camps/wildlife/fortress)
- Modify: `src/biome.rs` (`:307` build-world system)

- [ ] **Step 1: `Armory` struct + `new` — retype the body material handle, keep eyes on StandardMaterial**

In `src/orks.rs`:
```rust
pub struct Armory {
    mat: Handle<crate::creature::CreatureMaterial>,
    tmpl: Vec<((OrkVariant, Faction), Template)>,
    eye_mesh: Handle<Mesh>,
    eye_mat: Handle<StandardMaterial>, // glowing eyes stay a plain emissive StandardMaterial
}
```
```rust
    pub fn new(
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>, // still used for the eye material
        mat: Handle<crate::creature::CreatureMaterial>,
    ) -> Armory {
```
The `eye_mat = materials.add(StandardMaterial {...})` block is unchanged. `spawn` / `spawn_prop` keep `MeshMaterial3d(self.mat.clone())` for body parts and `MeshMaterial3d(self.eye_mat.clone())` for eyes — both type-check unchanged.

- [ ] **Step 2: `camps::build` — accept + build the creature material**

```rust
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
```
Replace the ork `let mat = materials.add(StandardMaterial {...})` (camps.rs:241) with:
```rust
    let mat = crate::creature::make_creature_material(creature_mats);
```
(Leave the separate props material at camps.rs:572 as a `StandardMaterial` — it's prop dressing, not orks.)

- [ ] **Step 3: `ork_fortress::build` — same**

```rust
pub fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    std_mats: &mut Assets<StandardMaterial>,
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
```
Replace `let ork_mat = std_mats.add(StandardMaterial {...})` (:416) with:
```rust
    let ork_mat = crate::creature::make_creature_material(creature_mats);
```

- [ ] **Step 4: `worldmap::build` — thread the param through**

Add to the signature (`worldmap.rs:708`):
```rust
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
```
Update the internal calls:
```rust
    crate::wildlife::populate(commands, meshes, std_mats, creature_mats);     // :924
    crate::ork_fortress::build(commands, meshes, images, std_mats, creature_mats); // :945
```
…and the `camps::build(...)` call (find it in `worldmap::build`) gains `creature_mats`.

- [ ] **Step 5: `biome.rs` build-world system — supply the resource**

At `biome.rs:307`, the enclosing system signature gains:
```rust
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
```
and the call becomes:
```rust
    crate::worldmap::build(&mut commands, &mut meshes, &mut images, &mut std_mats,
        &mut terrain_mats, &mut water_mats, &mut creature_mats);
```

- [ ] **Step 6: `siege::setup_invader_armory` — build the creature material**

```rust
fn setup_invader_armory(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut creature_mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
) {
    let mat = crate::creature::make_creature_material(&mut creature_mats);
    commands.insert_resource(InvaderArmory(orks::Armory::new(&mut meshes, &mut materials, mat)));
}
```

- [ ] **Step 7: Type-check**

Run: `cargo check`
Expected: orks + the 3 callers + the threading compile. (combat_fx will still fail until Task 7 — that's expected; if you want a clean check, do Task 7 next before running the app.)

- [ ] **Step 8: Commit**

```bash
git add src/orks.rs src/camps.rs src/siege.rs src/ork_fortress.rs src/worldmap.rs src/biome.rs
git commit -m "refactor(orks): draw warbands/invaders/fortress against CreatureMaterial"
```

## Task 6: Retype wildlife

**Files:**
- Modify: `src/wildlife.rs` (`populate` :788, `WildlifeAssets` :766, `spawn_one` :883, `drain_respawns` :577)

- [ ] **Step 1: `populate` — accept + build the creature material**

```rust
pub fn populate(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,            // (no longer used here; keep or drop)
    creature_mats: &mut Assets<crate::creature::CreatureMaterial>,
) {
    let mat = crate::creature::make_creature_material(creature_mats);
    // ...rest unchanged; `mat` now a CreatureMaterial handle...
```
If `materials` becomes unused, drop it from the signature and from the `worldmap::build` call site (Task 5 step 4).

- [ ] **Step 2: Retype the retained assets + spawners**

```rust
#[derive(Resource)]
struct WildlifeAssets {
    mat: Handle<crate::creature::CreatureMaterial>,
    templates: Vec<(Species, Template)>,
}
```
```rust
fn spawn_one(
    commands: &mut Commands,
    mat: &Handle<crate::creature::CreatureMaterial>,
    // ...
```
`drain_respawns` reads `assets.mat` (now the new type) and passes `&assets.mat` to `spawn_one` — unchanged body.

- [ ] **Step 3: Type-check**

Run: `cargo check`
Expected: wildlife compiles (combat_fx still pending Task 7).

- [ ] **Step 4: Commit**

```bash
git add src/wildlife.rs
git commit -m "refactor(wildlife): draw herds against CreatureMaterial"
```

## Task 7: Retype combat_fx hurt-flash

**Files:**
- Modify: `src/combat_fx.rs` (`HurtSkin` :360, `ensure_ork_skin` :365, `ensure_animal_skin` :399, `hurt_flash` :420)

- [ ] **Step 1: Retype `HurtSkin` + the three systems**

```rust
#[derive(Component)]
struct HurtSkin(Handle<crate::creature::CreatureMaterial>);
```
In `ensure_ork_skin` and `ensure_animal_skin`:
```rust
    mut mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
    child_mats: Query<&MeshMaterial3d<crate::creature::CreatureMaterial>>,
```
The clone logic is identical (`mats.get(&shared.0).cloned()`, `mats.add(base)`, `try_insert(MeshMaterial3d(own.clone()))`). The eye-skip in `ensure_ork_skin` is unchanged (eyes carry a `StandardMaterial`, so `child_mats.get(eye)` now naturally misses — confirm the eye filter still works; it filters by `OrkEye`, not by material type, so it's fine).

- [ ] **Step 2: `hurt_flash` — flash via `.base.emissive`**

```rust
    mut mats: ResMut<Assets<crate::creature::CreatureMaterial>>,
```
```rust
        if let Some(m) = mats.get_mut(&skin.0) {
            m.base.emissive = LinearRgba::BLACK;       // clear
        }
```
```rust
        if let Some(m) = mats.get_mut(&skin.0) {
            let v = k * HURT_FLASH_PEAK;
            m.base.emissive = LinearRgba::rgb(v, v, v); // whiten
        }
```

- [ ] **Step 3: Full type-check**

Run: `cargo check`
Expected: the WHOLE crate compiles.

- [ ] **Step 4: Commit**

```bash
git add src/combat_fx.rs
git commit -m "refactor(combat-fx): hurt-flash on CreatureMaterial"
```

## Task 8: Phase-0 visual + perf gate (identical look)

**Files:** none (verification only)

- [ ] **Step 1: Baseline shot — orks**

Run (PowerShell):
```powershell
$env:FOREST_SHOT="target/p0_orks.png"; $env:FOREST_ORKLINE="0,-6"; $env:FOREST_CAM="0,3,4,0,1.4,-6"; cargo run
```
Expected: a line of orks renders, looking ≈ the same as before this phase (subtle mottle at most). No black/transparent/pink artifacts (pink = shader failed to load — check console for WGSL errors).

- [ ] **Step 2: Baseline shot — hero**

```powershell
$env:FOREST_SHOT="target/p0_hero.png"; $env:FOREST_EQUIP="sword_gold,gold_armor"; cargo run
```
Expected: knight renders correctly with gold blade/armor.

- [ ] **Step 3: Siege perf sanity**

```powershell
$env:FOREST_WAVE="1"; $env:FOREST_DEFEND="1"; cargo run
```
Toggle F2 (perf overlay); confirm frame-rate holds with a full assault on screen (the material is one shared handle + the existing per-entity hurt clones; the procedural noise is a few hashes). Close the window.

- [ ] **Step 4: Confirm hurt-flash + death still work**

In the siege run, attack an ork (LMB): confirm the white hurt-flash fires and the death crumple (shrink/sink/tip) plays. These exercise the retyped combat_fx + the transform-only `dying.rs`.

- [ ] **Step 5: Review the PNGs**

Open `target/p0_orks.png` and `target/p0_hero.png`. They must look essentially unchanged from pre-Phase-0. If anything is pink/black/transparent, the shader failed — fix imports (Task 2 step 2) before proceeding.

- [ ] **Step 6: Commit the gate (docs only, optional)**

```bash
git commit --allow-empty -m "test(creature): phase-0 pipeline verified (no visual change)"
git push
```

---

# PHASE 1 — Per-surface texture + Hero

## Task 9: Fill in per-surface texturing in `creature.wgsl`

**Files:**
- Modify: `assets/shaders/creature.wgsl`

- [ ] **Step 1: Replace the baseline texture block with per-surface branches**

Replace the section from `let surf = in.color.a;` to the `force opaque` line with:
```wgsl
    let surf = in.color.a;
    let strength = creature.params.x;
    let relief = creature.params.y;
    let spec_lift = creature.params.z;

    // Decode the surface family from the alpha bucket (see Surf::surf_code).
    // 0.07 Skin · 0.21 Fur · 0.36 Scale · 0.50 Stone · 0.64 Metal · 0.79 Cloth · 0.93 Bone
    var lum = 1.0;
    var rough_adj = 0.0;
    var rgb = pbr_input.material.base_color.rgb;

    if (surf < 0.14) {
        // Skin / hide — soft low-freq mottle + faint pores.
        let n = vnoise(obj * 9.0) - 0.5;
        let pore = (vnoise(obj * 38.0) - 0.5) * 0.4;
        lum = 1.0 + (n + pore) * strength;
        rough_adj = 0.05;
    } else if (surf < 0.28) {
        // Fur — streaks stretched along the part's long axis (Y in part space).
        let p = obj * vec3(26.0, 7.0, 26.0);
        let f = (vnoise(p) - 0.5) + (vnoise(p * 2.3) - 0.5) * 0.5;
        lum = 1.0 + f * strength * 1.4;
        rough_adj = 0.12;
    } else if (surf < 0.43) {
        // Scale — cellular: quantise position into cells, darken cell edges.
        let cell = floor(obj * 16.0);
        let r = hash3(cell);
        let edge = fract(obj.x * 16.0) * fract(obj.y * 16.0);
        lum = 1.0 + (r - 0.5) * strength * 1.2 - (1.0 - smoothstep(0.05, 0.2, edge)) * strength;
        rough_adj = -0.05;
    } else if (surf < 0.57) {
        // Stone — broadband mottle + sparse bright speckle, rougher.
        let n = (vnoise(obj * 8.0) - 0.5) + (vnoise(obj * 22.0) - 0.5) * 0.5;
        let spk = step(0.92, vnoise(obj * 40.0));
        lum = 1.0 + n * strength * 1.3 + spk * strength * 2.0;
        rough_adj = 0.18;
    } else if (surf < 0.71) {
        // Metal — very low noise, LOWER roughness + spec lift (plate sheen).
        let n = vnoise(obj * 30.0) - 0.5;
        lum = 1.0 + n * strength * 0.4;
        rough_adj = -0.30 * spec_lift - 0.10;
    } else if (surf < 0.86) {
        // Cloth — fine weave grain.
        let weave = (sin(obj.x * 120.0) * sin(obj.y * 120.0)) * 0.5;
        lum = 1.0 + weave * strength * 0.6;
        rough_adj = 0.10;
    } else {
        // Bone — fine grain, slightly polished.
        let n = vnoise(obj * 24.0) - 0.5;
        lum = 1.0 + n * strength * 0.7;
        rough_adj = -0.05;
    }

    rgb = rgb * lum;
    pbr_input.material.base_color = vec4<f32>(rgb, 1.0);
    pbr_input.material.perceptual_roughness =
        clamp(pbr_input.material.perceptual_roughness + rough_adj, 0.05, 1.0);

    // Micro-relief: perturb the normal slightly by the noise gradient (cheap finite diff).
    if (relief > 0.0) {
        let e = 0.02;
        let dx = vnoise(obj * 18.0 + vec3(e, 0.0, 0.0)) - vnoise(obj * 18.0 - vec3(e, 0.0, 0.0));
        let dz = vnoise(obj * 18.0 + vec3(0.0, 0.0, e)) - vnoise(obj * 18.0 - vec3(0.0, 0.0, e));
        let bump = normalize(pbr_input.N + (m * vec3(dx, 0.0, dz)) * relief * 0.5);
        pbr_input.N = bump;
    }
```
(Keep the `apply_pbr_lighting` / `main_pass_post_lighting_processing` tail unchanged.)

- [ ] **Step 2: Visual check — still all Skin (no surf tags yet)**

```powershell
$env:FOREST_SHOT="target/t9_skin.png"; $env:FOREST_ORKLINE="0,-6"; $env:FOREST_CAM="0,3,4,0,1.4,-6"; cargo run
```
Expected: orks (untagged = Skin branch) show a subtle hide mottle, nothing garish. If too strong, lower `params.x` in `make_creature_material` (it's also live-tunable from F1 if exposed).

- [ ] **Step 3: Commit**

```bash
git add assets/shaders/creature.wgsl
git commit -m "feat(creature): per-surface procedural texture (fur/scale/stone/metal/cloth/bone)"
```

## Task 10: Hero surf tags + block stance + idle

**Files:**
- Modify: `src/player/model.rs` (`build_knight` parts)

- [ ] **Step 1: Tag knight meshes by surface in `build_knight`**

Wrap each `group(...)` result (or the per-part meshes) with `crate::creature::surf(mesh, Surf::X)`. Assignment:
  - **Metal**: torso (body/breastplate/backplate/gorget/tassets), head (helm/crown/visor/brow/cheek), both `plate_arm` groups, all `leg()` groups, the shield plate/rim, all `weapon_parts` (iron/gold/frost sword blades + guards, axe head + haft rings, maul head + bands).
  - **Cloth**: the `CHAIN` mail skirt, `BELT`, `PLUME` crest/tail, the shield `SHIELD_FACE`/emblem? (emblem = Metal). Grip wood (`GRIP`) → **Bone**. Club/maul/axe wooden hafts (`CLUB_WOOD`/`STAFF_WOOD` are ork-side, not hero) — hero hafts use `GRIP` → Bone.
  - Apply at the part level: e.g. `let head = crate::creature::surf(group(head_parts), Surf::Metal);` and tag the small cloth/bone primitives individually before merging if a part mixes families (simplest: keep the whole helm Metal, the plume is part of the head group — to split it, build the plume as its own `surf(...)` mesh then `group` together; `surf` runs per-primitive so tag each primitive Mesh before the final group).

  Practical pattern (per-primitive tagging then group):
  ```rust
  let head = group(vec![
      crate::creature::surf(bx(0.32, 0.3, 0.32, v(0.0,0.0,0.0), al), Surf::Metal),
      // ...
      crate::creature::surf(bxr(0.05,0.10,0.26, v(0.0,0.27,-0.02), rx(0.22), PLUME), Surf::Cloth),
  ]);
  ```

- [ ] **Step 2: Visual check — all weapons/armor**

```powershell
$env:FOREST_SHOT="target/t10_hero_steel.png"; cargo run
$env:FOREST_SHOT="target/t10_hero_gold.png"; $env:FOREST_EQUIP="sword_gold,gold_armor"; cargo run
$env:FOREST_SHOT="target/t10_hero_frost.png"; $env:FOREST_EQUIP="blade_frost,dragon_plate"; cargo run
```
Expected: plate reads with a metallic sheen (lower roughness), plume reads cloth-matte, blades glint. Confirm no surface looks plastic-flat.

- [ ] **Step 3: Block stance + idle (combat polish + liveliness)**

The hero anim lives in `src/player/anim.rs` (`hero_anim`) and block in `src/player/block.rs`. Add: when blocking, hold the shield-arm/shield part in a braced pose (the model already exposes `SHIELD_BLOCK_POS`/`shield_block_rot()`); when idle (not moving, not attacking, not blocking) layer a subtle breathing bob via `crate::creature_anim::idle_micro` (Task 17). For this task, wire the block stance; idle micro-motion lands in Task 17 once the helper exists.

- [ ] **Step 4: Commit + push**

```bash
git add src/player/model.rs src/player/anim.rs src/player/block.rs
git commit -m "feat(hero): surface tags (plate=metal, plume=cloth) + block stance"
git push
```

## Task 11: Ork surf tags + head-look/idle

**Files:**
- Modify: `src/orks.rs` (`spec` :796 — per-primitive surf tags)

- [ ] **Step 1: Tag ork meshes by surface**

In `spec()`, tag each primitive before it's grouped:
  - **Skin**: torso body/underbelly, head skull/jaw/brow/ears, arm upper/forearm, legs/shin/foot.
  - **Cloth**: loincloth + hem tatters + war-paint stripes (`fac`/`dark` paint), the `BELT`, shoulder strap, `WRAP` wrist/ankle wraps, faction headband/mohawk/cheek-paint, the shaman faction tassel.
  - **Bone**: tusks (`TUSK`), trophy teeth, `BONE` belt-skull + scout feather + shaman half-skull headdress + horns, the bone claw cradling the orb.
  - **Skin** (wood reads matte) or **Bone** for the club/staff wood (`CLUB_WOOD`/`STAFF_WOOD`) — use **Bone** (grain). Club spikes/bands (`CLUB_BAND`) → **Metal**. The `RING` earring + buckle → **Metal**. The shaman `ORB` → leave **Skin** code (it's smooth) or **Bone**.
  - The glowing eyes are a separate emissive material — NOT tagged (untouched).

- [ ] **Step 2: Visual check — one of each variant**

```powershell
$env:FOREST_SHOT="target/t11_orks.png"; $env:FOREST_ORKLINE="0,-6"; $env:FOREST_CAM="0,3,5,0,1.4,-6"; cargo run
```
Expected: green ork hide reads soft-organic, tusks/bone read polished, loincloth/wraps read cloth-matte, club spikes glint. All 4 variants (grunt/scout/berserker/shaman) distinct.

- [ ] **Step 3: Wire head-look + idle into `ork_limbs`** (after Task 16's helper exists; if doing Task 11 first, leave a `// TODO(anim): head-look` and revisit — but prefer ordering Task 16 before this step).

- [ ] **Step 4: Commit + push**

```bash
git add src/orks.rs
git commit -m "feat(orks): surface tags (hide/bone/cloth/metal) across all variants"
git push
```

---

# PHASE 2 — Wildlife surface tags (13 species)

Tag `critters.rs` per species. Verify per biome with `FOREST_HERO="x,z"` + `FOREST_BIOME`. Biome region centres: snow (-69,-45) · desert (60,-39) · rock (66,4) · forest (-60,39) · swamp (0,57).

## Task 12: Forest/grass mammals — Wolf, Deer, Elk, Boar, Dog, Cat, Rabbit, Goat

**Files:**
- Modify: `src/critters.rs` (the per-species `fn`s)

- [ ] **Step 1: Tag**

Per-primitive surf wrap inside each species fn:
  - **Fur**: wolf body/head/legs/tail (FUR/LIGHT/DARK), deer coat, elk coat, boar hide/bristle, dog coat, cat fur, rabbit fur, goat wool.
  - **Bone**: deer antlers, elk antlers, goat horns, boar tusks, all hooves (`HOOF`/`NOSE`-as-hoof reads bone/keratin).
  - **Skin**: noses/snouts/paw-pads, eyes left untouched-ish (tiny). Antler/horn → Bone; nose → Skin.

- [ ] **Step 2: Visual check (forest)**

```powershell
$env:FOREST_SHOT="target/t12_forest.png"; $env:FOREST_HERO="-60,39"; $env:FOREST_BIOME="forest"; cargo run
```
Expected: fur reads with directional grain; antlers/horns polished. Stage a wolf/deer close-up via `FOREST_CAM` if needed.

- [ ] **Step 3: Commit + push**

```bash
git add src/critters.rs
git commit -m "feat(wildlife): fur/bone surface tags for forest & grass mammals"
git push
```

## Task 13: Snow & desert — PolarBear, Camel, Scorpion

**Files:**
- Modify: `src/critters.rs`

- [ ] **Step 1: Tag**

  - PolarBear: **Fur** (body/head/haunches/paws); claws → **Bone**.
  - Camel: **Fur** (coat/hump/neck); hooves → **Bone**; muzzle → **Skin**.
  - Scorpion: **Scale** (shell/abdomen/pincers/claw tips/legs/tail segments); stinger tip stays its bright red (Scale code is fine — the hue carries it).

- [ ] **Step 2: Visual check (snow + desert)**

```powershell
$env:FOREST_SHOT="target/t13_snow.png"; $env:FOREST_HERO="-69,-45"; $env:FOREST_BIOME="snow"; cargo run
$env:FOREST_SHOT="target/t13_desert.png"; $env:FOREST_HERO="60,-39"; $env:FOREST_BIOME="desert"; cargo run
```
Expected: bear/camel fur grain; scorpion reads chitinous/cellular.

- [ ] **Step 3: Commit + push**

```bash
git add src/critters.rs
git commit -m "feat(wildlife): fur/scale tags for bear, camel, scorpion"
git push
```

## Task 14: Rock & swamp menaces — Golem, BogCroc

**Files:**
- Modify: `src/critters.rs`

- [ ] **Step 1: Tag**

  - Golem: **Stone** (body/shoulders/back-slab/head/crags/arms/fists/legs/feet); moss → **Skin** (organic); the cyan `CORE` glow seams/eyes/chest → leave untagged-as-Skin (they're bright hue; the Stone branch would darken — keep them Skin so they stay clean, OR consider an emissive treatment later — out of scope, keep Skin).
  - BogCroc: **Scale** (hide/snout/jaw/spine ridge/tail/legs); teeth/claws → **Bone**; eyes amber untouched-ish.

- [ ] **Step 2: Visual check (rock + swamp)**

```powershell
$env:FOREST_SHOT="target/t14_rock.png"; $env:FOREST_HERO="66,4"; $env:FOREST_BIOME="rocky"; cargo run
$env:FOREST_SHOT="target/t14_swamp.png"; $env:FOREST_HERO="0,57"; $env:FOREST_BIOME="swamp"; cargo run
```
Expected: golem reads rough/speckled stone with clean glowing core; croc reads scaly with polished teeth.

- [ ] **Step 3: Commit + push**

```bash
git add src/critters.rs
git commit -m "feat(wildlife): stone/scale tags for golem & bog croc"
git push
```

---

# PHASE 3 — Animation enrichment

## Task 15: `creature_anim.rs` — shared pure helpers

**Files:**
- Create: `src/creature_anim.rs`
- Modify: `src/main.rs` (`mod creature_anim;`)

- [ ] **Step 1: Write the helpers (pure, unit-testable)**

```rust
//! Shared procedural animation helpers layered onto the existing sin-gait limb systems
//! (`ork_limbs`, `animal_limbs`, hero `hero_anim`). All pure — no ECS — so each rig calls them
//! and composes the result with its current rotations. No skeleton, no keyframes.

use bevy::prelude::*;

/// Local-yaw the head should add to glance toward `target` from a creature at `pos` facing
/// `facing` (world). Clamped to `max` rad and eased; returns 0 when the target is behind or far.
/// `t` is a small blend used by callers to ease in/out; here we just clamp.
pub fn head_look_yaw(pos: Vec2, facing: f32, target: Vec2, max: f32) -> f32 {
    let to = target - pos;
    if to.length_squared() < 1e-4 {
        return 0.0;
    }
    let want = to.x.atan2(to.y); // same convention as steer (x=sin, y=cos)
    let rel = wrap_pi(want - facing);
    rel.clamp(-max, max)
}

/// A gentle idle breathing/weight-shift offset given a phase `t` (seconds + per-instance phase).
/// Returns (pitch, roll) radians to add to the torso/head while idle. Tiny by design.
pub fn idle_micro(t: f32) -> (f32, f32) {
    let breath = (t * 1.1).sin() * 0.02;
    let shift = (t * 0.37).sin() * 0.015;
    (breath, shift)
}

/// Bank (local Z roll) to lean into a turn, from the per-frame facing delta `dyaw` (rad this
/// frame) and `dt`. Returns a clamped roll; scale by speed at the call site if desired.
pub fn turn_lean(dyaw: f32, dt: f32, max: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    (-dyaw / dt * 0.08).clamp(-max, max)
}

/// Run vs walk swing amplitude multiplier: 1.0 at walk, up to `peak` when `fast`.
pub fn gait_amp(fast: bool, peak: f32) -> f32 {
    if fast {
        peak
    } else {
        1.0
    }
}

/// Wrap an angle to (-PI, PI]. (Local copy so the helper has no cross-module dep.)
pub fn wrap_pi(a: f32) -> f32 {
    let mut x = a;
    while x > std::f32::consts::PI {
        x -= std::f32::consts::TAU;
    }
    while x <= -std::f32::consts::PI {
        x += std::f32::consts::TAU;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_look_clamps_and_centers() {
        // target straight ahead (facing +Z means want = atan2(0,1)=0)
        assert!(head_look_yaw(Vec2::ZERO, 0.0, Vec2::new(0.0, 5.0), 0.6).abs() < 1e-5);
        // target hard left, clamped to max
        let y = head_look_yaw(Vec2::ZERO, 0.0, Vec2::new(-5.0, 0.01), 0.6);
        assert!((y + 0.6).abs() < 1e-3, "got {y}");
    }

    #[test]
    fn turn_lean_opposes_turn_and_clamps() {
        let l = turn_lean(1.0, 0.1, 0.3); // turning +, lean negative
        assert!(l < 0.0 && l >= -0.3);
        assert_eq!(turn_lean(100.0, 0.1, 0.3), -0.3); // clamped
    }

    #[test]
    fn gait_amp_switches() {
        assert_eq!(gait_amp(false, 1.6), 1.0);
        assert_eq!(gait_amp(true, 1.6), 1.6);
    }
}
```

- [ ] **Step 2: Declare the module + run tests**

Add `mod creature_anim;` to `src/main.rs`.
Run: `cargo test creature_anim::tests`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/creature_anim.rs src/main.rs
git commit -m "feat(anim): shared procedural creature-anim helpers + tests"
```

## Task 16: Wire anim helpers into `ork_limbs`

**Files:**
- Modify: `src/orks.rs` (`ork_limbs` :572; the brain already computes `facing`/`moving`/`mode`)

- [ ] **Step 1: Head-look + idle on the head part; run amplitude on legs/arms; turn-lean on root**

In `ork_limbs`, for `PartKind::Head` when idle (not striking, `mode` not Attack), add a yaw toward the hero. The system needs the hero position — add `hero: Res<crate::player::HeroState>` to `ork_limbs`. Compose with the existing bob/scan:
```rust
PartKind::Head => {
    let (breath, _roll) = crate::creature_anim::idle_micro(t);
    let look = if !o.moving && hero.alive {
        crate::creature_anim::head_look_yaw(o.pos, o.facing, hero.pos, 0.5)
    } else { 0.0 };
    let bob = (t * 0.5).sin() * 0.06 + breath;
    Quat::from_euler(EulerRot::XYZ, bob, look, 0.0)
}
```
For run amplitude, multiply the leg/arm swing by `gait_amp(o.mode==Hunt || frenzied-ish, 1.4)` — orks already widen via `o.gait`; keep subtle. Turn-lean: in `ork_brain`, the root rotation is set at `tf.rotation = Quat::from_rotation_y(o.facing) * recoil`; add a small `* Quat::from_rotation_z(turn_lean(dyaw, dt, 0.12))` where `dyaw` is this frame's facing change (store `prev_facing` on `Ork` or compute from the steer result). Keep it tiny.

- [ ] **Step 2: Visual check (motion)**

```powershell
$env:FOREST_WAVE="1"; cargo run
```
Watch idle orks glance toward the hero; charging orks lean into turns. Confirm no jitter/snap (the turn-rate caps already smooth facing).

- [ ] **Step 3: Commit + push**

```bash
git add src/orks.rs
git commit -m "feat(orks): head-look, idle breathing, turn-lean"
git push
```

## Task 17: Wire anim helpers into `animal_limbs` + hero idle

**Files:**
- Modify: `src/wildlife.rs` (`animal_limbs` :511)
- Modify: `src/player/anim.rs` (hero idle micro-motion)

- [ ] **Step 1: Head-look + idle on animals**

Add `hero: Res<crate::player::HeroState>` to `animal_limbs`. For `PartKind::Head`, when grazing/idle and the hero is near, glance toward him (prey nervously, predators with intent) using `head_look_yaw(a.pos, a.facing, hero.pos, 0.45)` composed with the existing bob/scan. Add `idle_micro` breath to the head bob.

- [ ] **Step 2: Hero idle**

In `hero_anim`, when the hero is idle (not moving/attacking/blocking) add `idle_micro(t)` breathing to the torso/head pose (compose, don't replace the existing walk pose).

- [ ] **Step 3: Visual check**

```powershell
$env:FOREST_SHOT="target/t17_deer.png"; $env:FOREST_HERO="-60,39"; $env:FOREST_BIOME="forest"; $env:FOREST_CAM="-58,3,42,-60,1.2,39"; cargo run
```
Plus a live `cargo run` to watch idle creatures breathe + glance.

- [ ] **Step 4: Commit + push**

```bash
git add src/wildlife.rs src/player/anim.rs
git commit -m "feat(anim): head-look + idle breathing for wildlife & hero"
git push
```

## Task 18: Combat polish — strike curves + death variety

**Files:**
- Modify: `src/dying.rs` (`drive_death_fade` :42)
- Modify: `src/orks.rs` (`club_chop_x`/`shaman_cast_x`) and/or `src/wildlife.rs` (`head_bite_x` etc.) — refine anticipation only if the visual review wants it.

- [ ] **Step 1: Vary the death topple per entity**

In `drive_death_fade`, the current crumple always rotates local-Z by a fixed rate (all corpses tip the same way). Vary topple direction + speed by a per-entity hash so deaths don't look identical:
```rust
fn drive_death_fade(time: Res<Time>, mut q: Query<(Entity, &mut Transform), With<Dying>>) {
    let rate = time.delta_secs() / FADE_SECS;
    if rate <= 0.0 {
        return;
    }
    for (e, mut tf) in &mut q {
        // Per-entity topple: direction sign + a small speed jitter from the entity bits.
        let h = (e.to_bits() & 0xff) as f32 / 255.0; // 0..1, stable per corpse
        let dir = if (e.to_bits() & 1) == 0 { 1.0 } else { -1.0 };
        let speed = 1.1 + h * 0.7;
        tf.scale *= 1.0 - 0.85 * rate;
        tf.translation.y -= SINK * rate;
        tf.rotate_local_z(dir * speed * rate);
    }
}
```

- [ ] **Step 2: (Optional, review-driven) refine strike easing**

Only if the Phase-0/1 reviews flagged the strike as stiff: add a touch more anticipation to `club_chop_x` (deeper wind-up before the chop) and a softer recovery. Keep changes small; re-verify in a siege run.

- [ ] **Step 3: Visual check**

```powershell
$env:FOREST_WAVE="1"; $env:FOREST_DEFEND="1"; cargo run
```
Attack a clump of orks; confirm corpses topple in varied directions/speeds.

- [ ] **Step 4: Commit + push**

```bash
git add src/dying.rs src/orks.rs
git commit -m "feat(combat): varied death topple + strike easing polish"
git push
```

---

# Self-Review (completed during planning)

**Spec coverage:**
- §3.1 shared material → Tasks 1, 3 ✓
- §3.2 surf in vertex alpha → Task 1 (`Surf`/`surf`) ✓; applied Tasks 10-14 ✓
- §3.3 fragment shader (model-space noise, per-surface, opaque) → Tasks 2, 9 ✓
- §3.4 combat_fx/hero plumbing → Tasks 4, 5, 6, 7 ✓; dying resolved as transform-only (Task 8 step 4, Task 18) ✓
- §4 animation (head-look, idle, turn-lean, locomotion, combat) → Tasks 15-18 ✓
- §5 phased order + per-phase verify → phase structure ✓
- §6 verification harness → every visual task uses `FOREST_SHOT`/`FOREST_ORKLINE`/`FOREST_HERO`/`FOREST_WAVE` ✓; perf gate Task 8 ✓

**Type consistency:** `CreatureMaterial`, `make_creature_material`, `Surf`, `surf`, `surf_code`, `CreatureParams.params: Vec4` used consistently across Tasks 1-18. `Armory.mat: Handle<CreatureMaterial>` + `eye_mat: Handle<StandardMaterial>` consistent across orks/callers. `HurtSkin`/`HeroMaterial` retyped consistently.

**Placeholder scan:** Task 10 step 3 and Task 11 step 3 intentionally depend on the helper from Task 15/16 — note the ordering preference (do Task 15 before wiring head-look). No unresolved TBD/TODO left in shipped code; the one `// TODO(anim)` is explicitly flagged to avoid by ordering.

**Ordering note:** If executing strictly in number order, Task 16 (helper wiring) precedes the head-look step deferred in Task 11 — revisit Task 11 step 3 after Task 16, or simply do the ork head-look in Task 16 (it edits the same `ork_limbs`). The plan keeps surf-tagging (visual) and anim-wiring (motion) as separate commits per module.
