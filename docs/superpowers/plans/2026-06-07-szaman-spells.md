# Szaman Spells Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the ork shaman a ranged caster — it lobs homing magic bolts at the hero and heals wounded warband allies, instead of clubbing like a grunt.

**Architecture:** A new self-contained `projectile.rs` module owns homing bolts, fed by a `BoltSpawns` queue resource (same "pending channel" idiom as `PendingHeroDamage`). `orks.rs` branches on a new `Ork.shaman` flag: shamans cast bolts from range and run a `shaman_heal` system. Bolt impacts + heal motes reuse the existing `Spark` FX in `player/combat.rs` via small re-exports.

**Tech Stack:** Rust, Bevy 0.18 (`bevy::prelude`, glam `Vec3`/`Vec2`, `bevy::light::NotShadowCaster`).

---

## File structure

- **Create** `src/projectile.rs` — `Bolt` component, `BoltSpawns`/`BoltSpawn` queue, `BoltAssets`, pure `advance_bolt` homing helper, systems, `ProjectilePlugin`. Self-contained projectile unit (mirrors the original `projectileStore.ts`).
- **Modify** `src/main.rs` — declare `mod projectile;`, add `projectile::ProjectilePlugin` to the plugin set.
- **Modify** `src/player/combat.rs` — add a green `heal` material to `CombatFx`; make `spawn_burst` `pub(crate)`; add `pub(crate) fn spawn_heal_burst`.
- **Modify** `src/player/mod.rs` — re-export `Health`, `CombatFx`, `spawn_burst`, `spawn_heal_burst` for `orks.rs` / `projectile.rs`.
- **Modify** `src/orks.rs` — `Ork.shaman` + `Ork.heal_cd` fields; shaman cast branch (push `BoltSpawn`) + cast-range threshold; `shaman_heal` system; cast arm pose; register system.

---

## Task 1: Pure homing-bolt helper (TDD)

**Files:**
- Create: `src/projectile.rs`

- [ ] **Step 1: Write the failing test**

Create `src/projectile.rs` with only the helper + tests:

```rust
//! Homing magic bolts — the ork shaman's ranged spell. A bolt tracks the hero's
//! live position, deals damage on arrival (via `PendingHeroDamage`, so a raised
//! shield mitigates it), and fizzles after a short lifetime or once it has flown
//! its full range. Ported from the original game's `projectileStore.ts`.

use bevy::prelude::*;

/// A bolt within this distance of its target counts as a hit.
pub(crate) const BOLT_HIT_RADIUS: f32 = 0.6;

/// Outcome of advancing a bolt one frame.
#[derive(Debug, PartialEq)]
pub(crate) enum BoltStep {
    /// Still flying — new world position.
    Fly(Vec3),
    /// Reached the target — deal damage.
    Hit,
    /// Flew its full range without connecting — despawn.
    Fizzle,
}

/// Advance a bolt one frame toward `target`, moving `step` units. Returns the
/// outcome and the updated travelled distance.
pub(crate) fn advance_bolt(
    pos: Vec3,
    target: Vec3,
    step: f32,
    traveled: f32,
    max_range: f32,
) -> (BoltStep, f32) {
    let to = target - pos;
    let len = to.length();
    if len < BOLT_HIT_RADIUS {
        return (BoltStep::Hit, traveled);
    }
    let nt = traveled + step;
    if nt >= max_range {
        return (BoltStep::Fizzle, nt);
    }
    (BoltStep::Fly(pos + to / len.max(1e-6) * step), nt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homes_closer_each_step() {
        let (out, tr) = advance_bolt(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 1.0, 0.0, 40.0);
        assert_eq!(out, BoltStep::Fly(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(tr, 1.0);
    }

    #[test]
    fn hits_when_within_radius() {
        let (out, _) = advance_bolt(Vec3::ZERO, Vec3::new(0.3, 0.0, 0.0), 1.0, 0.0, 40.0);
        assert_eq!(out, BoltStep::Hit);
    }

    #[test]
    fn fizzles_past_max_range() {
        let (out, tr) = advance_bolt(Vec3::ZERO, Vec3::new(50.0, 0.0, 0.0), 2.0, 39.0, 40.0);
        assert_eq!(out, BoltStep::Fizzle);
        assert_eq!(tr, 41.0);
    }
}
```

Then add `mod projectile;` to `src/main.rs` (alphabetical, after `mod player;` / before `mod props;`):

```rust
mod player;
mod projectile;
mod props;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin tileworld-bevy-forest projectile`
(If the bin name differs, use `cargo test projectile`.)
Expected: COMPILE FAIL first time only if `mod projectile;` not yet added; once added, the three tests run and PASS (the helper is written alongside them). If you wrote the test first without the impl, expected: FAIL "cannot find function `advance_bolt`".

- [ ] **Step 3: Confirm the helper compiles and tests pass**

The implementation above is already minimal. No extra code needed.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test projectile`
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```bash
git add src/projectile.rs src/main.rs
git commit -m "feat(projectile): homing-bolt advance helper + tests"
```

---

## Task 2: Heal FX + re-exports in the player module

**Files:**
- Modify: `src/player/combat.rs`
- Modify: `src/player/mod.rs`

- [ ] **Step 1: Add a heal material to `CombatFx`**

In `src/player/combat.rs`, extend the resource struct (currently `mesh`, `hit`, `kill`):

```rust
#[derive(Resource)]
pub(crate) struct CombatFx {
    mesh: Handle<Mesh>,
    hit: Handle<StandardMaterial>,
    kill: Handle<StandardMaterial>,
    /// Green motes for the shaman's heal cast.
    heal: Handle<StandardMaterial>,
}
```

In `setup_combat_fx`, build the green material and include it in the inserted resource. Add after the `kill` material:

```rust
    let heal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 1.0, 0.6),
        emissive: LinearRgba::rgb(0.8, 3.0, 1.2),
        unlit: true,
        ..default()
    });
    commands.insert_resource(CombatFx { mesh, hit, kill, heal });
```

(Replace the existing `commands.insert_resource(CombatFx { mesh, hit, kill });` line.)

- [ ] **Step 2: Make `spawn_burst` reusable + add `spawn_heal_burst`**

In `src/player/combat.rs`, change `fn spawn_burst` to `pub(crate) fn spawn_burst` (so `projectile.rs` can reuse it for bolt impacts). Then add the heal-burst helper right after it:

```rust
/// A green sparkle burst when the shaman heals an ally — rising motes.
pub(crate) fn spawn_heal_burst(commands: &mut Commands, fx: &CombatFx, at: Vec3) {
    for i in 0..10u32 {
        let a = i as f32 * 2.399_963_2;
        let mag = 0.4 + ((i * 29 % 10) as f32) * 0.05;
        let vel = Vec3::new(a.cos() * 1.4, 1.8 + (i % 3) as f32 * 0.3, a.sin() * 1.4) * mag;
        commands.spawn((
            Mesh3d(fx.mesh.clone()),
            MeshMaterial3d(fx.heal.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(0.6)),
            Spark { vel, life: 0.5, life0: 0.5, scale0: 0.6 },
            bevy::light::NotShadowCaster,
        ));
    }
}
```

- [ ] **Step 3: Re-export the items `orks.rs` / `projectile.rs` need**

In `src/player/mod.rs`, after the `mod combat;` line (line 13), add:

```rust
pub(crate) use combat::{spawn_burst, spawn_heal_burst, CombatFx, Health};
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds clean. A `dead_code` warning on `spawn_heal_burst` / `CombatFx`-reexport is acceptable until Tasks 3–4 wire them up.

- [ ] **Step 5: Commit**

```bash
git add src/player/combat.rs src/player/mod.rs
git commit -m "feat(combat): green heal-burst FX + re-export Health/CombatFx/spawn_burst"
```

---

## Task 3: Bolt entities, queue, assets, plugin

**Files:**
- Modify: `src/projectile.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add the bolt component, queue, assets, and tuning constants**

Append to `src/projectile.rs` (after the `advance_bolt` helper, before `#[cfg(test)]`):

```rust
use crate::biome::BiomeEntity;
use crate::player::{spawn_burst, CombatFx, HeroState, PendingHeroDamage};

/// Bolt flight speed (world units/sec).
const BOLT_SPEED: f32 = 9.0;
/// Seconds a bolt lives before it fizzles regardless of distance.
const BOLT_TTL: f32 = 3.0;
/// Distance a bolt may fly before fizzling short of a fleeing target.
const BOLT_MAX_RANGE: f32 = 16.0;

/// One bolt the shaman wants spawned this frame.
pub struct BoltSpawn {
    pub origin: Vec3,
    pub damage: f32,
}

/// Spawn queue — `orks.rs` pushes, `spawn_queued_bolts` drains. Mirrors the
/// `PendingHeroDamage` channel idiom (no `Commands` needed in the ork brain).
#[derive(Resource, Default)]
pub struct BoltSpawns(pub Vec<BoltSpawn>);

/// A live homing bolt flying at the hero.
#[derive(Component)]
struct Bolt {
    damage: f32,
    speed: f32,
    ttl: f32,
    traveled: f32,
    max_range: f32,
}

/// Shared bolt mesh + glowing purple material, built once.
#[derive(Resource)]
struct BoltAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
}

fn setup_bolt_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.16).mesh().ico(2).unwrap());
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.78, 0.61, 1.0),
        emissive: LinearRgba::rgb(2.4, 1.4, 4.0),
        unlit: true,
        ..default()
    });
    commands.insert_resource(BoltAssets { mesh, mat });
}

fn spawn_queued_bolts(
    mut commands: Commands,
    assets: Res<BoltAssets>,
    mut spawns: ResMut<BoltSpawns>,
) {
    for s in spawns.0.drain(..) {
        commands.spawn((
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.mat.clone()),
            Transform::from_translation(s.origin),
            Bolt {
                damage: s.damage,
                speed: BOLT_SPEED,
                ttl: BOLT_TTL,
                traveled: 0.0,
                max_range: BOLT_MAX_RANGE,
            },
            bevy::light::NotShadowCaster,
            BiomeEntity,
        ));
    }
}

fn step_bolts(
    time: Res<Time>,
    hero: Res<HeroState>,
    fx: Option<Res<CombatFx>>,
    mut pending: ResMut<PendingHeroDamage>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Bolt, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    let target = Vec3::new(hero.pos.x, hero.y + 1.0, hero.pos.y);
    for (e, mut b, mut tf) in &mut q {
        b.ttl -= dt;
        if !hero.alive || b.ttl <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let (out, traveled) = advance_bolt(tf.translation, target, b.speed * dt, b.traveled, b.max_range);
        b.traveled = traveled;
        match out {
            BoltStep::Fly(p) => tf.translation = p,
            BoltStep::Hit => {
                pending.0 += b.damage;
                if let Some(fx) = &fx {
                    spawn_burst(&mut commands, fx, tf.translation, false);
                }
                commands.entity(e).despawn();
            }
            BoltStep::Fizzle => commands.entity(e).despawn(),
        }
    }
}

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BoltSpawns>()
            .add_systems(Startup, setup_bolt_assets)
            .add_systems(Update, (spawn_queued_bolts, step_bolts).chain());
    }
}
```

- [ ] **Step 2: Register the plugin**

In `src/main.rs`, add `projectile::ProjectilePlugin` to the second `add_plugins((...))` tuple — place it next to `orks::OrksPlugin`:

```rust
            orks::OrksPlugin,           // camp warbands: idle/patrol AI + biped limb anim
            projectile::ProjectilePlugin, // shaman homing bolts (drains BoltSpawns)
```

(Check the tuple still has ≤15 entries — the file splits plugins across two `add_plugins` calls for exactly this arity limit. The second tuple currently has 9; adding one makes 10. OK.)

- [ ] **Step 3: Verify it compiles + tests still pass**

Run: `cargo build && cargo test projectile`
Expected: builds clean (a `dead_code` warning on `BoltSpawns`/`BoltSpawn` until Task 4 pushes to it is acceptable); 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/projectile.rs src/main.rs
git commit -m "feat(projectile): bolt entities, spawn queue, assets, plugin"
```

---

## Task 4: Shaman casts bolts + heals allies

**Files:**
- Modify: `src/orks.rs`

- [ ] **Step 1: Add shaman tuning constants**

In `src/orks.rs`, after the existing combat constants block (after `const ORK_ATTACK_CD: f32 = 1.1;`, line ~41), add:

```rust
// ── Shaman (ranged caster) — ported from `orkConfig.ts` shaman, scaled to this scene. ──
/// A shaman stands off and casts once the hero is within this range (no melee charge).
const SHAMAN_CAST_RANGE: f32 = 8.0;
/// Seconds between bolt casts.
const SHAMAN_CAST_CD: f32 = 2.1;
/// Bolt damage (kept above the club's `ORK_DAMAGE`, as in the original).
const SHAMAN_BOLT_DAMAGE: f32 = 12.0;
/// A shaman heals the nearest wounded ally within this range.
const SHAMAN_HEAL_RANGE: f32 = 8.0;
/// HP restored per heal.
const SHAMAN_HEAL_AMOUNT: f32 = 20.0;
/// Seconds between heals.
const SHAMAN_HEAL_CD: f32 = 5.0;
```

- [ ] **Step 2: Add `shaman` + `heal_cd` fields to `Ork`**

In the `Ork` struct (line ~108), after `atk_cd: f32,`:

```rust
    /// This ork is the camp shaman (casts bolts + heals instead of clubbing).
    shaman: bool,
    /// Heal cooldown (s) — shamans only.
    heal_cd: f32,
```

In `Armory::spawn`, where the `Ork { ... }` is constructed (line ~472), add after `atk_cd: 0.0,`:

```rust
            shaman: st.shaman,
            heal_cd: rng_range(&mut rng, 0.0, SHAMAN_HEAL_CD),
```

- [ ] **Step 3: Branch the aggro threshold + Attack on `shaman`**

In `ork_brain`, change the aggro mode pick (line ~164). Replace:

```rust
            o.mode = if o.pos.distance(hero.pos) < ORK_ATTACK_RANGE {
                OrkMode::Attack
            } else {
                OrkMode::Hunt
            };
```

with:

```rust
            let atk_range = if o.shaman { SHAMAN_CAST_RANGE } else { ORK_ATTACK_RANGE };
            o.mode = if o.pos.distance(hero.pos) < atk_range {
                OrkMode::Attack
            } else {
                OrkMode::Hunt
            };
```

Add `mut bolts: ResMut<crate::projectile::BoltSpawns>` to `ork_brain`'s parameters (after `mut pending: ResMut<crate::player::PendingHeroDamage>,`).

Then in the `OrkMode::Attack` branch, replace the strike block (line ~227):

```rust
                if o.atk_cd <= 0.0 {
                    o.atk_cd = ORK_ATTACK_CD;
                    pending.0 += ORK_DAMAGE;
                }
```

with:

```rust
                if o.atk_cd <= 0.0 {
                    if o.shaman {
                        o.atk_cd = SHAMAN_CAST_CD;
                        let gy = worldmap::ground_at_world(o.pos.x, o.pos.y).unwrap_or(0.0);
                        bolts.0.push(crate::projectile::BoltSpawn {
                            origin: Vec3::new(o.pos.x, gy + 1.4, o.pos.y),
                            damage: SHAMAN_BOLT_DAMAGE,
                        });
                    } else {
                        o.atk_cd = ORK_ATTACK_CD;
                        pending.0 += ORK_DAMAGE;
                    }
                }
```

- [ ] **Step 4: Add the cast arm pose in `ork_limbs`**

In `ork_limbs`, replace the `PartKind::Arm(sign)` arm of the `match part.kind` (line ~253):

```rust
                PartKind::Arm(sign) => {
                    let s = if o.moving { -(t * o.gait).sin() * 0.42 } else { (t * 0.8).sin() * 0.05 };
                    Quat::from_rotation_x(sign * s)
                }
```

with:

```rust
                PartKind::Arm(sign) => {
                    // Shaman raises its staff (right arm) while casting.
                    if o.shaman && sign > 0.0 && matches!(o.mode, OrkMode::Attack) {
                        Quat::from_rotation_x(-1.3)
                    } else {
                        let s = if o.moving { -(t * o.gait).sin() * 0.42 } else { (t * 0.8).sin() * 0.05 };
                        Quat::from_rotation_x(sign * s)
                    }
                }
```

- [ ] **Step 5: Add the `shaman_heal` system**

Add this function to `src/orks.rs` (after `ork_limbs`):

```rust
/// Each shaman, on its heal cooldown, restores HP to the nearest wounded warband
/// ally within range and sparkles it green. Reads the `Health` combat owns.
fn shaman_heal(
    time: Res<Time>,
    fx: Option<Res<crate::player::CombatFx>>,
    mut commands: Commands,
    mut shamans: Query<(Entity, &mut Ork)>,
    mut healths: Query<(Entity, &GlobalTransform, &mut crate::player::Health)>,
) {
    let dt = time.delta_secs().min(0.05);
    // Snapshot ally vitals once (entity, xz, hp, max) so we can scan then mutate.
    let allies: Vec<(Entity, Vec2, f32, f32)> = healths
        .iter()
        .map(|(e, gt, h)| (e, Vec2::new(gt.translation().x, gt.translation().z), h.hp, h.max))
        .collect();

    for (self_e, mut o) in &mut shamans {
        if !o.shaman {
            continue;
        }
        o.heal_cd -= dt;
        if o.heal_cd > 0.0 {
            continue;
        }
        let mut best: Option<Entity> = None;
        let mut best_d = SHAMAN_HEAL_RANGE;
        for (e, p, hp, max) in &allies {
            if *e == self_e || *hp >= *max - 0.5 {
                continue;
            }
            let d = o.pos.distance(*p);
            if d < best_d {
                best_d = d;
                best = Some(*e);
            }
        }
        if let Some(e) = best {
            if let Ok((_, gt, mut h)) = healths.get_mut(e) {
                h.hp = (h.hp + SHAMAN_HEAL_AMOUNT).min(h.max);
                if let Some(fx) = &fx {
                    let at = gt.translation() + Vec3::Y * 1.2;
                    crate::player::spawn_heal_burst(&mut commands, fx, at);
                }
            }
            o.heal_cd = SHAMAN_HEAL_CD;
        } else {
            o.heal_cd = 1.0; // nothing to heal — re-check soon
        }
    }
}
```

- [ ] **Step 6: Register `shaman_heal`**

In `OrksPlugin::build` (line ~139), replace:

```rust
        app.add_systems(Update, (ork_brain, ork_limbs).chain());
```

with:

```rust
        app.add_systems(Update, (ork_brain, ork_limbs).chain());
        app.add_systems(Update, shaman_heal);
```

- [ ] **Step 7: Verify it compiles + tests pass**

Run: `cargo build && cargo test`
Expected: builds clean (no `dead_code` warnings now — `BoltSpawns`, `CombatFx`, `spawn_heal_burst` are all used); all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/orks.rs
git commit -m "feat(orks): shaman casts homing bolts + heals wounded allies"
```

---

## Task 5: Manual verification

**Files:** none (verification only)

- [ ] **Step 1: Run the app and engage a camp**

Run: `cargo run --release` (or use the screenshot harness env vars from memory: `FOREST_SHOT`/`CAM`/`TIME`).

- [ ] **Step 2: Confirm the four behaviours**

1. Walk a camp's warband into aggro. The **shaman holds its distance** (~8 units) and does **not** run in to club.
2. The shaman raises its staff and **purple bolts fly** from it, homing the hero. The hero's HP drops on each hit.
3. **Blocking** mitigates a bolt hit (it routes through `PendingHeroDamage`).
4. Wound a grunt (let it take a hit), then watch a nearby shaman **heal it** — its HP rises and **green motes** sparkle.

- [ ] **Step 3: Note any tuning misses**

If bolts are too fast/slow to dodge, adjust `BOLT_SPEED`; if the shaman casts too rarely, lower `SHAMAN_CAST_CD`. Re-run.

---

## Notes for the executor

- The crate is a **binary** (`src/main.rs`), so tests live inline (`#[cfg(test)] mod tests`) and run via `cargo test`.
- `Ork.mode` and `OrkMode` are private to `orks.rs`; `ork_limbs` and `shaman_heal` are in the same module, so they read `o.mode` directly — no re-export needed.
- Orks get a `GlobalTransform` automatically (Bevy requires it for `Transform`); `player/combat.rs` already queries orks' `&GlobalTransform`, confirming this.
- Damage routing through `PendingHeroDamage` is deliberate: the existing block mitigation in `health::apply_hero_damage` then applies to bolts for free.
```
