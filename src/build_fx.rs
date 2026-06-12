//! **Construction feedback** — every freshly raised structure pops out of the ground on a kick
//! of mortar dust, instead of blinking into existence. Two spawn paths feed it:
//!
//!   * town producer buildings (`town::spawn_building`) get a uniform scale-in **pop** on their
//!     ground-anchored parent;
//!   * castle / decor parts (`castle::sync_castle`, `castle_decor::sync_decor`) are baked in
//!     world space on identity transforms, so they get a y-only **rise** out of the ground
//!     (uniform scaling would slide them toward the world origin).
//!
//! Dust reuses the footstep-fx puff assets ([`footstep_fx`](crate::footstep_fx)) — request one by
//! spawning a bare [`DustBurst`] from any system with `Commands`; no plumbing needed.
//!
//! Like the footstep fades, everything here is visual-only and runs **ungated**, so a panel
//! opened mid-pop still lets the structure settle instead of freezing it half-grown.

use bevy::prelude::*;

/// Initial scale factor a popping structure starts at (not 0 — a degenerate scale can produce
/// NaN normals on some drivers and a dead-zero AABB).
const SCALE0: f32 = 0.04;

pub struct BuildFxPlugin;

impl Plugin for BuildFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (animate_pops, dust_bursts));
    }
}

/// Scale-in animation for a freshly raised structure; removes itself when settled.
/// Assumes the entity's resting scale is `Vec3::ONE` (true for both spawn paths today).
#[derive(Component)]
pub struct BuildPop {
    age: f32,
    dur: f32,
    /// `true`: animate `scale.y` only (baked world-space parts); `false`: uniform scale-in.
    rise: bool,
}

impl BuildPop {
    /// Uniform pop with a small overshoot — for a structure parented at its own ground position.
    pub fn pop() -> Self {
        Self { age: 0.0, dur: 0.45, rise: false }
    }

    /// Y-only rise out of the ground — for meshes baked in world space on identity transforms.
    pub fn rise() -> Self {
        Self { age: 0.0, dur: 0.55, rise: true }
    }

    /// The starting scale. The *inserter* applies this to the transform in the same frame it
    /// reveals the entity, so the structure never flashes full-size before the first anim tick.
    pub fn scale0(&self) -> Vec3 {
        if self.rise { Vec3::new(1.0, SCALE0, 1.0) } else { Vec3::splat(SCALE0) }
    }
}

/// World position a reveal-gated part was authored at. The reveal systems read it to aim the
/// dust burst (the part's mesh is baked in world space, so its `Transform` doesn't know).
#[derive(Component)]
pub struct RevealAt(pub Vec3);

/// A one-shot construction dust request — spawn this bare component anywhere; consumed
/// (despawned) by [`dust_bursts`] the next frame.
#[derive(Component)]
pub struct DustBurst {
    pub pos: Vec3,
    pub n: u32,
    pub spd: f32,
    pub scale0: f32,
    /// Footprint half-width — motes start on this ring so the structure's own mesh doesn't
    /// swallow them (born at the centre they die unseen inside it).
    pub radius: f32,
}

impl DustBurst {
    /// A whole producer building going up on its plot (~2-unit footprint).
    pub fn building(pos: Vec3) -> Self {
        Self { pos, n: 14, spd: 2.2, scale0: 1.2, radius: 1.2 }
    }

    /// One castle / decor part revealing.
    pub fn part(pos: Vec3) -> Self {
        Self { pos, n: 10, spd: 1.7, scale0: 1.0, radius: 0.9 }
    }
}

/// Ease-out-back: starts at 0, overshoots ~5% around t≈0.7, settles at 1 — the "pop".
fn ease_out_back(t: f32) -> f32 {
    const K: f32 = 1.4;
    let u = t - 1.0;
    1.0 + u * u * ((K + 1.0) * u + K)
}

fn animate_pops(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut BuildPop, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut p, mut tf) in &mut q {
        p.age += dt;
        let t = (p.age / p.dur).clamp(0.0, 1.0);
        let s = SCALE0 + (1.0 - SCALE0) * ease_out_back(t);
        if p.rise {
            tf.scale.y = s;
        } else {
            tf.scale = Vec3::splat(s);
        }
        if t >= 1.0 {
            tf.scale = if p.rise { Vec3::new(tf.scale.x, 1.0, tf.scale.z) } else { Vec3::ONE };
            commands.entity(e).try_remove::<BuildPop>();
        }
    }
}

/// Drain pending [`DustBurst`] requests into footstep-fx puffs (their fade system settles them).
fn dust_bursts(
    mut commands: Commands,
    fx: Option<Res<crate::footstep_fx::FxAssets>>,
    q: Query<(Entity, &DustBurst)>,
) {
    let Some(fx) = fx else { return }; // assets still pending — leave requests queued
    for (e, d) in &q {
        crate::footstep_fx::spawn_puffs(
            &mut commands,
            &fx.puff,
            &fx.mortar,
            d.pos,
            d.n,
            d.spd,
            d.scale0,
            d.radius,
        );
        commands.entity(e).try_despawn();
    }
}
