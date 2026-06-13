# Chest Tiers, Juice & Gnashfang Mimics — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pull chest logic into its own `src/chest.rs` plugin, add 2 distance-gated tiers (Wood/Relic) with a rarity glow, make every open juicier, and add rooted Gnashfang mimics on Relic chests.

**Architecture:** Move the chest subsystem out of the overloaded `verbs.rs` into a new one-feature `ChestPlugin`. Tier is derived from the existing `forest_frontier` gradient and stored on `Chest`. Mimics reuse the existing combat plumbing — they read the `HeroSwing` broadcast for melee damage (like ore), bite the hero via `PendingHeroDamage` (like orks), and die via `dying::begin_dying`. No new save schema (mimic-vs-real is re-derived from `ChestId`).

**Tech Stack:** Rust, Bevy 0.18, the project's vertex-colour flat-shaded mesh idiom.

Spec: `docs/superpowers/specs/2026-06-13-chest-tiers-juice-mimics-design.md`.

---

## File structure

- **Create** `src/chest.rs` — the whole chest subsystem: `Chest`/`ChestId`/`ChestLid`/`LidSwing`, `ChestTier`/`ChestKind`, `Mimic` + glow/teeth child tags, the open/respawn/mimic systems, `populate_chests`/`spawn_trophy_chest`, the chest+lid+teeth+glow mesh builders, and `ChestPlugin`. Carries its own tiny mesh helpers (`cbx`/`cgroup`/`ctint`/`v`) — matching the per-prop-module convention.
- **Modify** `src/main.rs` — register `chest::ChestPlugin`.
- **Modify** `src/verbs.rs` — delete the moved chest code; drop `chest_interact`/`chest_respawn` from `VerbsPlugin`. Keep `forest_frontier` (`pub(crate)`, already), the `HeroSwing` message, ore/forage/drops, and the verbs-local mesh helpers (ore/apple still use them).
- **Modify** `src/worldmap.rs:931` — `crate::verbs::populate_chests` → `crate::chest::populate_chests`.
- **Modify** `src/ork_fortress.rs:1069` — `crate::verbs::spawn_trophy_chest` → `crate::chest::spawn_trophy_chest`.
- **Modify** `src/savegame.rs:40` — import `Chest`/`ChestId`/`ChestLid`/`CHEST_LID_OPEN` from `crate::chest`; handle a looted mimic on restore.
- **Modify** `src/audio/lines.rs` — add `Concept::ChestRelic` + `Concept::MimicWake` and one `Line` each.

---

## Task 1: Move the chest subsystem into `src/chest.rs` (no behaviour change)

This is a mechanical extraction — the world must look and play identically after it. Do it first so every later task edits one focused file.

**Files:**
- Create: `src/chest.rs`
- Modify: `src/verbs.rs` (remove chest code + plugin lines), `src/main.rs`, `src/worldmap.rs:931`, `src/ork_fortress.rs:1069`, `src/savegame.rs:40`

- [ ] **Step 1: Create `src/chest.rs` and move the code.** Move these items verbatim out of `verbs.rs` into `chest.rs`: the `Chest`, `ChestId`, `ChestLid`, `LidSwing` components; `TROPHY_CHEST_ID`, `CHEST_LID_OPEN`, `CHEST_INTERACT_DIST` consts; `chest_interact`, `chest_respawn`, `populate_chests`, `spawn_trophy_chest` fns; the `lid_swing` driver system (find it near `LidSwing`); `chest_body_mesh`, `chest_lid_mesh`; the chest-local `tile_hash` helper. Add a module doc-comment.

  Add at the top of `chest.rs`:

```rust
//! **Chests** — scattered loot containers (their own subsystem, extracted from `verbs.rs`).
//! Two distance-gated tiers (Wood/Relic), instant juicy opens, and rooted Gnashfang mimics on
//! Relic chests. Mimics reuse combat plumbing: they read `verbs::HeroSwing` for melee damage and
//! bite the hero via `player::PendingHeroDamage`, dying through `dying::begin_dying`.

use bevy::prelude::*;
use tileworld_core::frontier;

use crate::audio::AudioCue;
use crate::combat_fx::{FloatReq, HitFeedback};
use crate::inventory::{try_grant, Inventory, Toasts};
use crate::palette::lin;
use crate::player::{Health, HeroState, PendingHeroDamage, PlayerRes};
use crate::verbs::{forest_frontier, HeroSwing};
use crate::worldmap;

pub struct ChestPlugin;

impl Plugin for ChestPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (chest_interact, chest_respawn, lid_swing)
                .run_if(in_state(crate::game_state::Modal::None)),
        );
    }
}
```

  Add chest-local copies of the mesh helpers (the verbs originals stay for ore/apple):

```rust
fn v(x: f32, y: f32, z: f32) -> Vec3 { Vec3::new(x, y, z) }
fn ctint(mut m: Mesh, c: [f32; 4]) -> Mesh {
    let n = m.count_vertices();
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n]);
    m
}
fn cbx(w: f32, h: f32, d: f32, off: Vec3, c: [f32; 4]) -> Mesh {
    ctint(Cuboid::new(w, h, d).mesh().build().translated_by(off), c)
}
fn cgroup(parts: Vec<Mesh>) -> Mesh {
    let mut it = parts.into_iter();
    let mut base = it.next().expect("at least one part");
    for p in it { base.merge(&p).expect("parts share attributes"); }
    base.duplicate_vertices();
    base.compute_flat_normals();
    base
}
```

  Make the moved items `pub(crate)` where another module needs them: `Chest`, `ChestId`, `ChestLid`, `CHEST_LID_OPEN`, `TROPHY_CHEST_ID`, `populate_chests`, `spawn_trophy_chest` (these are already `pub`/`pub(crate)` in verbs — preserve it). The `lid_swing` system + `LidSwing` stay private.

- [ ] **Step 2: Remove the chest code from `verbs.rs`.** Delete the moved items. In `VerbsPlugin::build`, remove `chest_interact,` and `chest_respawn,` from the gated `Update` tuple (lines ~98–99). If `lid_swing` was registered in the impact-juice block, remove it there too. Verify no remaining `verbs.rs` code references the moved symbols.

- [ ] **Step 3: Register the plugin in `main.rs`.** Add `chest::ChestPlugin,` to one of the `add_plugins((...))` tuples that has room (each tuple ≤15). Put it next to `verbs::VerbsPlugin` for readability, e.g. immediately after it in the same tuple if that tuple has <15 entries; otherwise add to the next tuple. Add `mod chest;` with the other `mod` declarations.

```rust
verbs::VerbsPlugin,          // biome verbs: ore mining (HeroSwing) → stone
chest::ChestPlugin,          // scattered loot chests: tiers + juice + Gnashfang mimics
```

- [ ] **Step 4: Update the three call sites.**
  - `src/worldmap.rs:931`: `crate::verbs::populate_chests(...)` → `crate::chest::populate_chests(...)`.
  - `src/ork_fortress.rs:1069`: `crate::verbs::spawn_trophy_chest(` → `crate::chest::spawn_trophy_chest(`.
  - `src/savegame.rs:40`: `use crate::verbs::{Chest, ChestId, ChestLid, CHEST_LID_OPEN};` → `use crate::chest::{Chest, ChestId, ChestLid, CHEST_LID_OPEN};`.

- [ ] **Step 5: Build.**

Run: `cargo check`
Expected: compiles clean (warnings ok). No `unresolved import` / `cannot find` errors.

- [ ] **Step 6: Commit.**

```bash
git add src/chest.rs src/verbs.rs src/main.rs src/worldmap.rs src/ork_fortress.rs src/savegame.rs
git commit -m "refactor(chest): extract chest subsystem from verbs.rs into src/chest.rs" -- src/chest.rs src/verbs.rs src/main.rs src/worldmap.rs src/ork_fortress.rs src/savegame.rs
```

---

## Task 2: ChestTier + `tier_for` (TDD)

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Write the failing test.** Add to a `#[cfg(test)] mod tests` in `chest.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_splits_at_half_frontier() {
        assert_eq!(tier_for(0.0), ChestTier::Wood);
        assert_eq!(tier_for(0.49), ChestTier::Wood);
        assert_eq!(tier_for(0.5), ChestTier::Relic);
        assert_eq!(tier_for(1.0), ChestTier::Relic);
    }
}
```

- [ ] **Step 2: Run it, verify it fails.**

Run: `cargo test -p tileworld_bevy_forest tier_splits` (or `cargo test tier_splits`)
Expected: FAIL — `cannot find type ChestTier` / `function tier_for`.

- [ ] **Step 3: Implement.**

```rust
/// A chest's loot rank, set by distance from the keep (`forest_frontier`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChestTier { Wood, Relic }

/// Wood near the keep (frontier `< 0.5`), Relic out in the deep biomes (`>= 0.5`).
pub(crate) fn tier_for(frontier: f64) -> ChestTier {
    if frontier >= 0.5 { ChestTier::Relic } else { ChestTier::Wood }
}
```

- [ ] **Step 4: Run it, verify it passes.**

Run: `cargo test tier_splits`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git commit -m "feat(chest): add ChestTier + distance-gated tier_for" -- src/chest.rs
```

---

## Task 3: Store tier on `Chest`, wire tier into loot

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Add a `tier` field to `Chest`.**

```rust
#[derive(Component)]
pub(crate) struct Chest {
    pub(crate) cache: bool,
    pub(crate) opened: bool,
    pub(crate) opened_at: f32,
    factor: f64,
    trophy: Option<(i64, &'static [&'static str])>,
    hoard: bool,
    pub(crate) tier: ChestTier,
}
```

- [ ] **Step 2: Set `tier` at every `Chest { .. }` construction.** In `populate_chests` (scatter + hoards), `spawn_trophy_chest`: add `tier: tier_for(forest_frontier(x, z))` (for the trophy, use the chest's world `pos.x`/`pos.z`). Hoards/trophy sit deep, so they resolve to `Relic` naturally.

- [ ] **Step 3: Use tier for the ordinary-treasure loot floor in `chest_interact`.** In the final `else` branch (ordinary treasure), give Relic a guaranteed top-pool floor (mirroring the `hoard` branch's `roll_gear(1.0, ..)` but WITHOUT the heavy purse):

```rust
} else {
    let h = tile_hash(p.x, p.z);
    let items = 1 + (chest.factor * 2.0).round() as i64;
    // Relic chests roll from the top pool (factor pinned to 1.0) so the deep-biome haul is
    // reliably strong; Wood uses the frontier-graded factor. Gold stays the modest curve —
    // exploration pays in GEAR, not purses.
    let roll_factor = if chest.tier == ChestTier::Relic { 1.0 } else { chest.factor };
    let loot = (0..items)
        .map(|i| frontier::roll_gear(roll_factor, (h + i as f64 * 0.37) % 1.0))
        .collect();
    (loot, (5.0 + chest.factor * 55.0 + h * 10.0).round() as i64)
};
```

- [ ] **Step 4: Build.**

Run: `cargo check`
Expected: compiles clean.

- [ ] **Step 5: Commit.**

```bash
git commit -m "feat(chest): store tier on Chest, Relic rolls the top loot pool" -- src/chest.rs
```

---

## Task 4: Tier glow child + open-flare

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Add the glow component + a glow material handle.** The glow is a separate emissive child so the shared white body material keeps batching (CONTRACT mesh rule).

```rust
/// Emissive "loot beam + rim" child on a chest — colour set by `chest_glow` (gold for a Relic,
/// sickly green when a mimic shows its tell). Wood chests get a dim/near-off glow.
#[derive(Component)]
struct ChestGlow {
    /// brightened briefly by `chest_interact` on open, eased back down by `chest_glow`.
    flare: f32,
}

/// A short upward beam quad + a faint rim disc, built unit-height around local origin.
fn chest_glow_mesh() -> Mesh {
    // a thin tall box reads as a beam from any angle; keep it cheap.
    cgroup(vec![cbx(0.10, 1.2, 0.10, v(0.0, 0.6, 0.0), [1.0, 1.0, 1.0, 1.0])])
}
```

- [ ] **Step 2: Spawn a per-chest glow material + child** in `populate_chests`/`spawn_trophy_chest`. Each chest needs its OWN emissive material instance (colour mutates per-chest), so build it inline (do NOT share the white body `chest_mat`):

```rust
let glow_mesh = meshes.add(chest_glow_mesh());
// ...inside the spawn's with_children:
let glow_mat = materials.add(StandardMaterial {
    base_color: Color::NONE,
    emissive: LinearRgba::BLACK,   // chest_glow drives this
    unlit: true,
    alpha_mode: AlphaMode::Add,
    ..default()
});
p.spawn((
    Mesh3d(glow_mesh.clone()),
    MeshMaterial3d(glow_mat),
    Transform::from_xyz(0.0, 0.0, 0.0),
    ChestGlow { flare: 0.0 },
));
```

- [ ] **Step 3: Add the `chest_glow` system** (register it in `ChestPlugin`'s gated tuple). It sets each glow's emissive from its parent chest's tier/kind/flare. (Mimic-green handled in Task 7 — for now: Wood = off, Relic = gold, plus the flare bump.)

```rust
fn chest_glow(
    time: Res<Time>,
    chests: Query<(&Chest, &Children)>,
    mut glows: Query<(&mut ChestGlow, &MeshMaterial3d<StandardMaterial>)>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    for (chest, children) in &chests {
        for &c in children {
            let Ok((mut glow, math)) = glows.get_mut(c) else { continue };
            glow.flare = (glow.flare - dt * 2.0).max(0.0);
            let base = match chest.tier {
                ChestTier::Relic if !chest.opened => 0.6,
                _ => 0.0,
            };
            let gold = LinearRgba::rgb(2.4, 1.5, 0.4);
            let lvl = base + glow.flare * 3.0;
            if let Some(m) = mats.get_mut(&math.0) {
                m.emissive = gold * lvl;
            }
        }
    }
}
```

- [ ] **Step 4: Flare on open.** In `chest_interact`, after a successful open, bump the glow child's `flare` to `1.0` (find the chest's `ChestGlow` child the same way the lid is found). Add a `glows: Query<&mut ChestGlow>` param and set `glow.flare = 1.0` for the matched child.

- [ ] **Step 5: Build + visual check.**

Run: `cargo check`
Then stage a Relic chest with the screenshot harness (see CLAUDE.md): `$env:FOREST_SHOT="target/relic.png"; $env:FOREST_HERO="-60,39"; cargo run` — eyeball a gold beam on a deep-forest chest.

- [ ] **Step 6: Commit.**

```bash
git commit -m "feat(chest): tier glow child (gold Relic beam) + open flare" -- src/chest.rs
```

---

## Task 5: Juice on open — shake + bigger burst + Relic bark

**Files:**
- Modify: `src/chest.rs`, `src/audio/lines.rs`

- [ ] **Step 1: Add the Relic bark concept + line.** In `src/audio/lines.rs`, add `ChestRelic` to the `Concept` enum (near `ChestOpen`, line ~52) and a `Line` in `LINES` (near line ~155):

```rust
// in enum Concept:
ChestRelic,
// in LINES:
Line { floor: 300.0, priority: 15, ..line("chest_relic", Speaker::Hero, Concept::ChestRelic, "Now THAT'S a haul. Worth the walk.") }, // hero, on opening a Relic-tier chest
```

  Add a candidate-count assertion to the existing lines test (near line 432): `assert_eq!(candidates(Concept::ChestRelic).count(), 1);`.

- [ ] **Step 2: Add screen-shake + a tiered bark to `chest_interact`.** Add `mut feedback: ResMut<HitFeedback>` to the system params. After the existing `floats`/`cues`/`speak` block on a successful open, replace the single `Speak(ChestOpen)` with a tier branch and add trauma:

```rust
let relic = chest.tier == ChestTier::Relic;
feedback.trauma = (feedback.trauma + if relic { 0.35 } else { 0.12 }).min(1.0);
if relic {
    cues.write(AudioCue::Gold); // a second coin chime layers the richer payout
    speak.write(crate::audio::Speak::new(crate::audio::Concept::ChestRelic));
} else {
    speak.write(crate::audio::Speak::new(crate::audio::Concept::ChestOpen));
}
```

  (Remove the now-duplicated original `speak.write(... ChestOpen)` line.)

- [ ] **Step 3: Build + check audio test.**

Run: `cargo test -p tileworld_bevy_forest` then `cargo check`
Expected: the lines tests pass (incl. the new `ChestRelic` count); compiles clean.

- [ ] **Step 4: Commit.**

```bash
git commit -m "feat(chest): tiered open juice — Relic shake + extra chime + bark" -- src/chest.rs src/audio/lines.rs
```

---

## Task 6: Mimic data — `ChestKind` + `is_mimic` (TDD), spawn mimics

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Write the failing tests.**

```rust
#[test]
fn mimics_are_relic_treasure_only() {
    // Caches are never mimics, regardless of id.
    for id in 0..200 {
        assert!(!is_mimic(id, ChestTier::Relic, true), "no cache mimic");
        assert!(!is_mimic(id, ChestTier::Wood, false), "no Wood mimic");
    }
}

#[test]
fn relic_mimic_share_is_about_a_fifth() {
    let n = (0..1000).filter(|&id| is_mimic(id, ChestTier::Relic, false)).count();
    // ~20% ± a deterministic-hash wobble.
    assert!((120..=280).contains(&n), "got {n} mimics of 1000");
}

#[test]
fn is_mimic_is_deterministic() {
    assert_eq!(is_mimic(7, ChestTier::Relic, false), is_mimic(7, ChestTier::Relic, false));
}
```

- [ ] **Step 2: Run, verify fail.**

Run: `cargo test mimic`
Expected: FAIL — `cannot find function is_mimic` / type `ChestKind`.

- [ ] **Step 3: Implement.**

```rust
/// A chest is either a real container or a disguised Gnashfang mimic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ChestKind { Real, Mimic }

const MIMIC_RATE: f64 = 0.20;

/// Deterministic [0,1) hash off a chest's stable `ChestId` — same chests are mimics every run.
fn mimic_hash(id: usize) -> f64 {
    let s = ((id as f64) * 73.13 + 19.19).sin() * 43758.5453;
    s - s.floor()
}

/// Mimics are Relic-tier treasure only (never a cache, never Wood — which keeps them out of the
/// home grass since Wood == near the keep). ~20% of Relic treasure bites back.
pub(crate) fn is_mimic(id: usize, tier: ChestTier, cache: bool) -> bool {
    !cache && tier == ChestTier::Relic && mimic_hash(id) < MIMIC_RATE
}
```

- [ ] **Step 4: Run, verify pass.**

Run: `cargo test mimic`
Expected: PASS (all three).

- [ ] **Step 5: Add the `Mimic` component + teeth child + spawn wiring.**

```rust
/// A disguised mimic. Dormant until provoked (pre-attacked) or opened blind, then it bites on a
/// cooldown and takes `HeroSwing` damage until killed. Rooted — a chest can't chase.
#[derive(Component)]
pub(crate) struct Mimic {
    awake: bool,
    /// hero hit it while still closed → no free bite + bonus loot on death.
    pre_attacked: bool,
    last_bite: f32,
}

/// A jagged row of teeth on the lid seam — nudged out by `mimic_tell` when the hero is near.
#[derive(Component)]
struct MimicTeeth;

const MIMIC_TEETH: u32 = 0xe8e0d0; // bone-white

fn mimic_teeth_mesh() -> Mesh {
    let bone = lin(MIMIC_TEETH);
    let mut parts = Vec::new();
    for i in 0..7 {
        let x = -0.27 + i as f32 * 0.09;
        parts.push(cbx(0.05, 0.10, 0.05, v(x, 0.0, 0.26), bone)); // upper fangs along the front seam
    }
    cgroup(parts)
}
```

  In `populate_chests`, after computing `factor`/`tier` for a scatter chest, compute `kind`:

```rust
let tier = tier_for(factor);
let kind = if is_mimic(placed as usize, tier, cache) { ChestKind::Mimic } else { ChestKind::Real };
```

  Add `tier` + `kind` to the `Chest` (store `kind` too — add a `pub(crate) kind: ChestKind` field to `Chest`, set on every construction; hoards/trophy = `ChestKind::Real`). For a mimic, also insert `Health` + `Mimic` on the parent and add a hidden teeth child:

```rust
if kind == ChestKind::Mimic {
    commands.entity(parent).try_insert((
        Health { hp: 90.0, max: 90.0 },   // dies in a few hero swings
        Mimic { awake: false, pre_attacked: false, last_bite: 0.0 },
    ));
    commands.entity(parent).with_children(|p| {
        p.spawn((
            Mesh3d(meshes.add(mimic_teeth_mesh())),
            MeshMaterial3d(chest_mat.clone()),
            Transform::from_xyz(0.0, 0.30, 0.0),
            Visibility::Hidden,   // shown by mimic_tell within range
            MimicTeeth,
        ));
    });
}
```

- [ ] **Step 6: Build.**

Run: `cargo check`
Expected: compiles clean.

- [ ] **Step 7: Commit.**

```bash
git commit -m "feat(chest): mimic data (ChestKind/is_mimic) + spawn Relic mimics with teeth + Health" -- src/chest.rs
```

---

## Task 7: Mimic tell — teeth wobble + green glow + growl

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Add the tell range const + extend `chest_glow` for mimics.** A dormant mimic within `MIMIC_TELL_DIST` glows sickly green instead of gold; awake mimics drop the disguise glow entirely.

```rust
const MIMIC_TELL_DIST: f32 = 4.0;
```

  In `chest_glow`, query the optional `&Mimic` on the chest entity and override the colour:

```rust
// signature gains: chests: Query<(&Chest, &Transform, Option<&Mimic>, &Children)>,
//                  hero: Res<HeroState>,
let near = Vec2::new(tf.translation.x, tf.translation.z).distance(hero.pos) < MIMIC_TELL_DIST;
let (col, base) = match (mimic, chest.tier) {
    (Some(m), _) if !m.awake && near => (LinearRgba::rgb(0.4, 2.4, 0.5), 0.7), // sickly green tell
    (Some(_), _) => (LinearRgba::BLACK, 0.0),                                   // awake/far mimic: no glow
    (None, ChestTier::Relic) if !chest.opened => (LinearRgba::rgb(2.4, 1.5, 0.4), 0.6), // gold Relic
    _ => (LinearRgba::BLACK, 0.0),
};
// emissive = col * (base + glow.flare * 3.0)
```

- [ ] **Step 2: Add `mimic_tell` system** (register in `ChestPlugin`). Wobble the teeth out of the seam + show them when the hero is within tell range, and fire a one-shot growl on first approach.

```rust
fn mimic_tell(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut cues: MessageWriter<AudioCue>,
    mut mimics: Query<(&mut Mimic, &Transform, &Children)>,
    mut teeth: Query<(&mut Transform, &mut Visibility), (With<MimicTeeth>, Without<Mimic>)>,
    mut growled: Local<std::collections::HashSet<u32>>,
) {
    let t = time.elapsed_secs();
    for (mimic, tf, children) in &mut mimics {
        if mimic.awake { continue; }
        let d = Vec2::new(tf.translation.x, tf.translation.z).distance(hero.pos);
        let near = d < MIMIC_TELL_DIST;
        for &c in children {
            if let Ok((mut ttf, mut vis)) = teeth.get_mut(c) {
                *vis = if near { Visibility::Inherited } else { Visibility::Hidden };
                // small breathing nudge so the teeth "chew" at the seam
                ttf.translation.z = 0.0 + if near { (t * 6.0).sin().max(0.0) * 0.04 } else { 0.0 };
            }
        }
    }
    // one-shot growl handled with the `growled` set keyed by entity bits — push AudioCue::CreatureBite
    // { at, big:false } once per mimic when `near` flips true (see note).
}
```

  Note: track per-entity "has growled while near" with the `growled` `Local<HashSet>` keyed by `entity.to_bits() as u32`; insert on entering range, remove on leaving, and `cues.write(AudioCue::CreatureBite { at: tf.translation, big: false })` on the entering edge (reused as a low chest-growl until a dedicated cue is added). Pass `Entity` into the loop via `mimics: Query<(Entity, &mut Mimic, ...)>`.

- [ ] **Step 3: Build + visual check.**

Run: `cargo check`
Then stage a mimic mid-tell with the harness near a Relic spot and eyeball the green glow + teeth (FOREST_HERO at a deep biome; some Relic chest there will be a mimic).

- [ ] **Step 4: Commit.**

```bash
git commit -m "feat(chest): mimic tell — green glow + chewing teeth + growl on approach" -- src/chest.rs
```

---

## Task 8: Mimic combat — `HeroSwing` damage, pre-attack bonus, blind-open free bite, death loot

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Branch `chest_interact` for a live mimic.** Before the normal open path (after the distance check, before resolving loot), if the chest entity has a live `Mimic`, F is a *blind open*: wake it with a free bite instead of opening. Add `mut mimics: Query<&mut Mimic>` and `mut pending: ResMut<PendingHeroDamage>` params:

```rust
// inside the per-chest loop, after the distance check:
if let Ok(mut mimic) = mimics.get_mut(entity) {
    if !mimic.awake {
        mimic.awake = true;            // it lunges...
        mimic.pre_attacked = false;    // blind open → normal loot
        pending.0 += MIMIC_BITE * 1.5; // a hard free bite for the greedy
        cues.write(AudioCue::CreatureBite { at: head, big: true });
        speak.write(crate::audio::Speak::new(crate::audio::Concept::MimicWake));
        return; // do NOT open / grant loot — kill it first
    }
    continue; // an already-awake mimic isn't "opened" by F
}
```

  (This needs the chest `Entity` in the loop — change the query to `Query<(Entity, &mut Chest, &Transform, &Children), Without<ChestLid>>`.) Add `Concept::MimicWake` + a line in `lines.rs`: `"A trap! Gnashfang's little joke. Right—"`.

```rust
const MIMIC_BITE: f32 = 24.0; // ≈ ork-grunt club (orks::ORK_DAMAGE); the derived feel, not a re-roll
```

- [ ] **Step 2: Add `mimic_damage` system** (register in `ChestPlugin`, gated). Reads `HeroSwing` (exactly like `mine_ore`), damages any mimic in the cone, wakes a dormant one as `pre_attacked` (→ bonus loot), and on death grants loot + `begin_dying`.

```rust
#[allow(clippy::too_many_arguments)]
fn mimic_damage(
    time: Res<Time>,
    mut swings: MessageReader<HeroSwing>,
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    mut toasts: ResMut<Toasts>,
    mut player: ResMut<PlayerRes>,
    mut floats: ResMut<crate::combat_fx::FloatQueue>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(Entity, &mut Mimic, &mut Health, &Chest, &Transform)>,
) {
    let now = time.elapsed_secs();
    const SWING_RANGE: f32 = 2.2; // chest is fat; a touch over the ore reach
    for sw in swings.read() {
        for (e, mut mimic, mut hp, chest, tf) in &mut q {
            if hp.hp <= 0.0 { continue; }
            let p = tf.translation;
            let to = Vec2::new(p.x - sw.origin.x, p.z - sw.origin.y);
            let dist = to.length();
            if dist > SWING_RANGE || dist < 1e-3 { continue; }
            if (to / dist).dot(sw.fwd) < 0.5 { continue; }
            if !mimic.awake { mimic.awake = true; mimic.pre_attacked = true; } // spotted + struck first
            hp.hp -= sw.base_dmg;
            floats.0.push(FloatReq {
                world: Vec3::new(p.x, p.y + 1.2, p.z),
                text: format!("{}", sw.base_dmg.round() as i64),
                color: crate::combat_fx::col_ork_hit(),
                scale: 1.0,
            });
            cues.write(AudioCue::CreatureBite { at: p, big: false });
            if hp.hp <= 0.0 {
                grant_mimic_loot(&mut inv, &mut toasts, &mut player, &mut floats, chest, mimic.pre_attacked, p, now);
                cues.write(AudioCue::ChestOpen);
                crate::dying::begin_dying(&mut commands, e, now);
            }
        }
    }
}
```

- [ ] **Step 2b: Add the shared loot helper** (reused by death). Mirrors `chest_interact`'s Relic roll; `pre_attacked` adds one extra top-pool item + a small purse as the perception bonus.

```rust
#[allow(clippy::too_many_arguments)]
fn grant_mimic_loot(
    inv: &mut Inventory, toasts: &mut Toasts, player: &mut PlayerRes,
    floats: &mut crate::combat_fx::FloatQueue, chest: &Chest, bonus: bool, p: Vec3, now: f32,
) {
    let h = tile_hash(p.x, p.z);
    let items = 3 + if bonus { 1 } else { 0 };
    let loot: Vec<&'static str> = (0..items)
        .map(|i| frontier::roll_gear(1.0, (h + i as f64 * 0.37) % 1.0))
        .collect();
    let gold = (30.0 + chest.factor * 40.0 + if bonus { 30.0 } else { 0.0 }).round() as i64;
    player.0.add_gold(gold);
    for id in &loot { try_grant(&mut inv.0, &mut toasts.0, id, 1, now as f64); }
    floats.0.push(FloatReq {
        world: Vec3::new(p.x, p.y + 1.4, p.z),
        text: format!("+{gold} gold"),
        color: crate::combat_fx::col_kill(), scale: 1.2,
    });
}
```

  (Note: mimic loot is granted on death even if the bag is "full" — combat death shouldn't be blocked by inventory; `try_grant` already no-ops a full bag, dropping overflow. That's acceptable v1.)

- [ ] **Step 3: Build.**

Run: `cargo check`
Expected: compiles clean.

- [ ] **Step 4: Commit.**

```bash
git commit -m "feat(chest): mimic combat — HeroSwing damage, pre-attack bonus, blind-open bite, death loot" -- src/chest.rs src/audio/lines.rs
```

---

## Task 9: Mimic bite — periodic hero damage while awake

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Add the `mimic_bite` system** (register in `ChestPlugin`, gated). An awake, living mimic snaps at the hero on a cooldown when in melee range.

```rust
const MIMIC_BITE_RANGE: f32 = 2.4;
const MIMIC_BITE_CD: f32 = 1.1;

fn mimic_bite(
    time: Res<Time>,
    hero: Res<HeroState>,
    mut pending: ResMut<PendingHeroDamage>,
    mut cues: MessageWriter<AudioCue>,
    mut q: Query<(&mut Mimic, &Health, &Transform)>,
) {
    if !hero.alive { return; }
    let now = time.elapsed_secs();
    for (mut mimic, hp, tf) in &mut q {
        if !mimic.awake || hp.hp <= 0.0 { continue; }
        let d = Vec2::new(tf.translation.x, tf.translation.z).distance(hero.pos);
        if d <= MIMIC_BITE_RANGE && now - mimic.last_bite >= MIMIC_BITE_CD {
            mimic.last_bite = now;
            pending.0 += MIMIC_BITE;
            cues.write(AudioCue::CreatureBite { at: tf.translation, big: true });
        }
    }
}
```

- [ ] **Step 2: Build + play-verify.**

Run: `cargo check`, then `cargo run` — find a deep Relic chest, confirm: a real one opens with the gold flare; a mimic shows green teeth, bites if F'd blind, and dies to a few swings dropping loot (extra if you hit it first).

- [ ] **Step 3: Commit.**

```bash
git commit -m "feat(chest): awake mimics bite the hero on a cooldown" -- src/chest.rs
```

---

## Task 10: More chests — `CHEST_COUNT` 12 → 24

**Files:**
- Modify: `src/chest.rs`

- [ ] **Step 1: Bump the count.** In `populate_chests`: `const CHEST_COUNT: u32 = 24;`. The `attempts` cap already scales off `CHEST_COUNT` (`CHEST_COUNT * 500 + 1000`), so reject-sampling still has headroom.

- [ ] **Step 2: Build + play-verify density.**

Run: `cargo check`, then `cargo run` — confirm noticeably more chests spread across biomes, none in the courtyard/water/plots.

- [ ] **Step 3: Commit.**

```bash
git commit -m "feat(chest): scatter 24 chests (was 12) for denser exploration loot" -- src/chest.rs
```

---

## Task 11: Save/load — a looted mimic stays dead

**Files:**
- Modify: `src/savegame.rs`

- [ ] **Step 1: Confirm capture covers mimics.** `capture` (savegame.rs ~193) flags `opened_chests[id]` for `!chest.cache && chest.opened`. A killed mimic's `Chest.opened` is currently never set (it dies, it doesn't "open"). Set `chest.opened = true` on mimic death in `grant_mimic_loot` (add `chest: &mut Chest` and set `chest.opened = true`; thread a `&mut Chest` through `mimic_damage`'s query) so the kill persists like an opened treasure.

- [ ] **Step 2: Restore — despawn a looted mimic instead of leaving a live one.** In `restore_opened_chests` (savegame.rs ~328): for each chest whose `opened_chests[id]` is true, if it has a `Mimic`, `try_despawn` the whole chest entity (a looted mimic is gone, not a re-closable chest); otherwise keep the existing lid-open restore. Add a `mimics: Query<(), With<crate::chest::Mimic>>` (make `Mimic` `pub(crate)`) and branch:

```rust
// inside the per-id restore loop, when opened_chests[id.0] is true:
if mimics.get(entity).is_ok() {
    commands.entity(entity).try_despawn();
    continue;
}
// ...existing lid-open restore...
```

- [ ] **Step 3: Build.**

Run: `cargo check`
Expected: compiles clean.

- [ ] **Step 4: Commit.**

```bash
git commit -m "fix(chest): persist mimic kills — a looted mimic stays gone on reload" -- src/chest.rs src/savegame.rs
```

---

## Task 12: Full verification

**Files:** none (verify + final commit if needed)

- [ ] **Step 1: Unit tests.**

Run: `cargo test`
Expected: all pass — incl. the new `chest` tier/mimic tests and the `lines` candidate-count assertions. Report the count.

- [ ] **Step 2: Visual proof.** Use the screenshot harness (CLAUDE.md) to capture: a gold Relic chest, and a mimic mid-tell (green glow + teeth). Save PNGs under `target/` and eyeball them.

- [ ] **Step 3: Play smoke test.** `cargo run`: open Wood + Relic chests (juice scales), provoke a mimic both ways (blind F = free bite; pre-attack = bonus loot on death), confirm panels/pause freeze mimics (Modal gate), confirm a killed mimic doesn't reappear after save/continue.

- [ ] **Step 4: Push.** Once verified, `git push` (per CLAUDE.md "push after a feature").

---

## Self-review notes (addressed inline)

- **Spec coverage:** more chests (T10), 2 tiers (T2–T3), glow tell (T4,T7), juicy open (T5), mimics: tell (T7) / fair-tell-punishing-bite (T8–T9) / pre-attack bonus (T8) / Relic-only (T6) / prep-only via Modal gate (every system `run_if(Modal::None)`), new plugin (T1), save (T11). All covered.
- **Type consistency:** `ChestTier`/`ChestKind`/`Mimic`/`ChestGlow`/`MimicTeeth`, `tier_for`/`is_mimic`/`grant_mimic_loot`, `MIMIC_BITE`/`MIMIC_TELL_DIST`/`MIMIC_BITE_RANGE`/`MIMIC_BITE_CD` are used consistently across tasks. `Chest` gains `tier` (T3) + `kind` (T6) fields — set at every construction site.
- **No new save fields:** tier/kind re-derived from position + `ChestId`; only the existing `opened_chests` bool vec is used (T11).
