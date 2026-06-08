//! Port of `src/world/dustStore.ts` + the render-curve in `src/world/Dust.tsx` —
//! the pooled ground dust kicked up by a footfall, sprint, or hard landing.
//!
//! The original splits into two files: `dustStore.ts` owns the mote POOL +
//! per-frame physics (drag + gentle gravity, settle on the ground), and `Dust.tsx`
//! owns the RENDER curve (a mote blooms in over the first 20% of its life, then
//! shrinks/fades out). Both halves are pure deterministic math with no Three.js or
//! React state, so the whole thing ports into core here; the Bevy `crates/game`
//! side only supplies the spawn TRIGGERS (footstep/landing hooks) and turns the
//! live mote list into instanced meshes each frame.
//!
//! Deviations from the TS, all to keep core dep-free + deterministic:
//!   - The TS used `Math.random()` for the per-mote jitter; here `spawn_dust` takes
//!     an explicit `&mut DustRng` (the same mulberry32 generator shape the rest of
//!     the crate uses) so a puff is reproducible and testable.
//!   - The TS read a Three.js `THREE.Color` to RGB; here the colour is carried as a
//!     plain `(r, g, b)` float triple (0..1, linear of the sRGB hex — the game crate
//!     converts the biome hex once and passes it in).
//!   - `THREE` was only used for colour parsing; nothing visual lives here.

/// Max live motes in the pool — saturating it drops the OLDEST (`dustStore.ts`
/// `MAX = 160`).
pub const MAX_MOTES: usize = 160;

/// Light gravity so a puff drifts instead of plummeting like a spark
/// (`dustStore.ts` `GRAVITY = -1.2`).
pub const GRAVITY: f64 = -1.2;
/// Heavy drag so a puff blooms outward, hangs, and settles within ~half a second
/// (`dustStore.ts` `DRAG = 3.4`).
pub const DRAG: f64 = 3.4;
/// Motes settle (and stop) at this height above the tile top, like the TS
/// `if (s.y < 0.04) { s.y = 0.04; s.vy = 0 }`.
pub const GROUND_Y: f64 = 0.04;

/// The fraction of a mote's life spent blooming IN before it starts shrinking out
/// (`Dust.tsx` `grow = min(1, k / 0.2)`).
pub const GROW_FRACTION: f64 = 0.2;
/// Base render-scale multiplier applied on top of the mote's own `size`
/// (`Dust.tsx` `scale = size * 0.55 * grow * out`).
pub const RENDER_SCALE: f64 = 0.55;

// ─── Per-biome dust tint + "does a plain walk stir it" (dustStore.ts L46-59) ──────

/// A biome's dust look: the earthy tint (sRGB hex) and whether LOOSE ground (so a
/// plain walk puffs underfoot, not just a sprint). Packed ground stays quiet so the
/// effect reads as detail, not noise (`dustStore.ts` `DUST_BY_BIOME` + `DUST_DEFAULT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiomeDust {
    /// Dust tint as an sRGB hex string (matches the TS colour literals exactly).
    pub color: &'static str,
    /// Loose ground → a plain walk stirs dust; packed ground → only a sprint/landing.
    pub loose: bool,
}

/// Default earthy tint for grass / forest / pine dirt (`dustStore.ts` `DUST_DEFAULT`).
pub const DUST_DEFAULT: BiomeDust = BiomeDust { color: "#c9b893", loose: false };

/// The dust look for a biome. Mirrors `dustStore.ts` `dustForBiome`: snow / desert /
/// rock are LOOSE (puff on a plain walk); swamp puffs only on a sprint/landing;
/// everything else (incl. grass / forest / sand / plains) falls to `DUST_DEFAULT`.
///
/// NOTE — faithful to the TS table: `sand` is NOT in the TS `DUST_BY_BIOME` map, so
/// it resolves to the (non-loose) default here too, exactly like the original. Only
/// snow / desert / rock are loose; the prompt's "sand/snow/scree/grass" is a loose
/// paraphrase — the ground-truth table is what's mirrored.
pub fn dust_for_biome(biome: Option<Biome>) -> BiomeDust {
    match biome {
        Some(Biome::Snow) => BiomeDust { color: "#eaf1f7", loose: true },
        Some(Biome::Desert) => BiomeDust { color: "#e3d2a0", loose: true },
        Some(Biome::Rock) => BiomeDust { color: "#bcb8b0", loose: true },
        Some(Biome::Swamp) => BiomeDust { color: "#6f6a4e", loose: false },
        _ => DUST_DEFAULT,
    }
}

use crate::tilemap::Biome;

// ─── Deterministic mote RNG (mulberry32, the crate-wide generator shape) ──────────

/// mulberry32 — the same generator shape as `CombatRng`/`OrbRng`, so the puff
/// fan-out is reproducible (and self-contained, no `rand` dep). Replaces the TS
/// `Math.random()` calls in `spawnDust`.
#[derive(Debug, Clone)]
pub struct DustRng {
    s: u32,
}

impl DustRng {
    /// Seed the generator. Any non-zero seed gives a full-period stream.
    pub fn new(seed: u32) -> Self {
        DustRng { s: seed }
    }

    /// Next f64 in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        self.s = self.s.wrapping_add(0x6D2B_79F5);
        let mut t = self.s;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        let r = (t ^ (t >> 14)) as f64;
        r / 4_294_967_296.0
    }
}

// ─── A single mote + the pool (dustStore.ts `Mote` + `motes[]`) ───────────────────

/// One dust mote. Lives in grid coords (the game crate bakes the world-centre offset
/// at render time, like orks/orbs/sparks). 1:1 with the TS `Mote` interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mote {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    /// Seconds elapsed.
    pub age: f64,
    /// Seconds total before the mote is pruned.
    pub life: f64,
    pub size: f64,
    /// Colour (linear or sRGB float triple — the game crate decides; carried as-is).
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Mote {
    /// Normalised life progress 0..1 (`age / life`), clamped.
    pub fn k(&self) -> f64 {
        if self.life > 0.0 {
            (self.age / self.life).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// The mote's RENDER scale this instant — the `Dust.tsx` fade-by-scale curve:
    /// blooms in over the first `GROW_FRACTION` of life (`grow = min(1, k/0.2)`),
    /// then shrinks away (`out = 1 - k`), times the base `size * RENDER_SCALE`.
    /// Floored at a hair above 0 so a degenerate mote never inverts.
    pub fn render_scale(&self) -> f64 {
        let k = self.k();
        let grow = (k / GROW_FRACTION).min(1.0);
        let out = 1.0 - k;
        (self.size * RENDER_SCALE * grow * out).max(0.0001)
    }
}

/// The pooled ground-dust system. Holds the live motes; `spawn`/`step` mirror the
/// TS `spawnDust`/`stepDust`. Bounded at `MAX_MOTES` (oldest dropped on overflow).
#[derive(Debug, Clone, Default)]
pub struct DustPool {
    motes: Vec<Mote>,
}

/// Options for one puff (`dustStore.ts` `DustOpts`), with the same defaults.
#[derive(Debug, Clone, Copy)]
pub struct DustOpts {
    /// Number of motes in the puff (TS default 5).
    pub count: usize,
    /// Outward speed magnitude (TS default 0.9).
    pub spread: f64,
    /// Mote size multiplier (TS default 1.0).
    pub size: f64,
    /// Upward launch bias (TS default 0.5).
    pub up: f64,
    /// Base mote colour as a linear/sRGB float triple (the TS parsed `opts.color`).
    pub color: (f32, f32, f32),
}

impl Default for DustOpts {
    fn default() -> Self {
        // The TS `spawnDust` defaults: count 5, spread 0.9, size 1, up 0.5, and the
        // DUST_DEFAULT grass tint (carried as a float triple by the caller).
        DustOpts { count: 5, spread: 0.9, size: 1.0, up: 0.5, color: (0.0, 0.0, 0.0) }
    }
}

impl DustPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Live motes (read-only; the render layer iterates these each frame).
    pub fn motes(&self) -> &[Mote] {
        &self.motes
    }

    /// Number of live motes.
    pub fn len(&self) -> usize {
        self.motes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.motes.is_empty()
    }

    /// Clear the pool (the TS `resetDust`; a fresh run / world remount).
    pub fn reset(&mut self) {
        self.motes.clear();
    }

    /// Emit a soft puff of motes at a grid point — the port of `dustStore.ts`
    /// `spawnDust`. Saturating the pool drops the OLDEST mote (TS `motes.shift()`).
    /// Per-mote jitter comes from `rng` (replacing the TS `Math.random()`).
    pub fn spawn(&mut self, x: f64, y: f64, z: f64, opts: DustOpts, rng: &mut DustRng) {
        let DustOpts { count, spread, size, up, color } = opts;
        for i in 0..count {
            if self.motes.len() >= MAX_MOTES {
                self.motes.remove(0); // drop the oldest (TS `motes.shift()`)
            }
            // Fan around a ring, biased outward (TS L72-79).
            let a = (i as f64 / count.max(1) as f64) * std::f64::consts::TAU + rng.next_f64() * 1.6;
            let sp = spread * (0.4 + rng.next_f64() * 0.8);
            self.motes.push(Mote {
                x: x + (rng.next_f64() * 2.0 - 1.0) * 0.12,
                y: y + rng.next_f64() * 0.08,
                z: z + (rng.next_f64() * 2.0 - 1.0) * 0.12,
                vx: a.cos() * sp,
                vy: up * (0.5 + rng.next_f64() * 0.8),
                vz: a.sin() * sp,
                age: 0.0,
                life: 0.45 + rng.next_f64() * 0.4,
                size: size * (0.7 + rng.next_f64() * 0.7),
                r: color.0,
                g: color.1,
                b: color.2,
            });
        }
    }

    /// Advance every mote (drag + gentle gravity, settling on the ground) and prune
    /// the dead — the port of `dustStore.ts` `stepDust`. `dt` is the frame delta.
    pub fn step(&mut self, dt: f64) {
        let d = (1.0 - DRAG * dt).max(0.0);
        let mut i = 0;
        while i < self.motes.len() {
            let s = &mut self.motes[i];
            s.age += dt;
            if s.age >= s.life {
                self.motes.remove(i);
                continue; // don't advance i — the next mote slid into this slot
            }
            s.vx *= d;
            s.vz *= d;
            s.vy = s.vy * d + GRAVITY * dt;
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            s.z += s.vz * dt;
            if s.y < GROUND_Y {
                s.y = GROUND_Y;
                s.vy = 0.0;
            }
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(count: usize) -> DustOpts {
        DustOpts { count, color: (0.5, 0.4, 0.3), ..Default::default() }
    }

    #[test]
    fn dust_for_biome_matches_the_ts_table() {
        // Loose biomes (a plain walk stirs them) — snow / desert / rock.
        assert_eq!(dust_for_biome(Some(Biome::Snow)), BiomeDust { color: "#eaf1f7", loose: true });
        assert_eq!(dust_for_biome(Some(Biome::Desert)), BiomeDust { color: "#e3d2a0", loose: true });
        assert_eq!(dust_for_biome(Some(Biome::Rock)), BiomeDust { color: "#bcb8b0", loose: true });
        // Swamp puffs but is NOT loose (sprint/landing only).
        assert_eq!(dust_for_biome(Some(Biome::Swamp)), BiomeDust { color: "#6f6a4e", loose: false });
        // Grass / forest / plains / sand / off-map all fall to the default tint.
        assert_eq!(dust_for_biome(Some(Biome::Grass)), DUST_DEFAULT);
        assert_eq!(dust_for_biome(Some(Biome::Forest)), DUST_DEFAULT);
        assert_eq!(dust_for_biome(Some(Biome::Plains)), DUST_DEFAULT);
        // Faithful to the TS: sand is not in DUST_BY_BIOME, so it's the default (not loose).
        assert_eq!(dust_for_biome(Some(Biome::Sand)), DUST_DEFAULT);
        assert_eq!(dust_for_biome(None), DUST_DEFAULT);
    }

    #[test]
    fn spawn_pushes_count_motes_with_the_colour() {
        let mut pool = DustPool::new();
        let mut rng = DustRng::new(1);
        pool.spawn(10.0, 1.0, 12.0, opts(5), &mut rng);
        assert_eq!(pool.len(), 5, "five motes in a default puff");
        for m in pool.motes() {
            assert_eq!((m.r, m.g, m.b), (0.5, 0.4, 0.3), "every mote carries the puff colour");
            // Lives are in the TS band 0.45..0.85.
            assert!((0.45..=0.85).contains(&m.life), "life {} in band", m.life);
            // Spawned near the puff origin (±0.12 jitter in x/z).
            assert!((m.x - 10.0).abs() <= 0.13 && (m.z - 12.0).abs() <= 0.13);
        }
    }

    #[test]
    fn pool_is_bounded_and_drops_the_oldest() {
        let mut pool = DustPool::new();
        let mut rng = DustRng::new(7);
        // Mark the very first mote so we can prove it's the one dropped on overflow.
        pool.spawn(0.0, 1.0, 0.0, opts(1), &mut rng);
        let first = pool.motes()[0];
        // Now flood well past MAX_MOTES.
        for _ in 0..(MAX_MOTES + 50) {
            pool.spawn(1.0, 1.0, 1.0, opts(1), &mut rng);
        }
        assert_eq!(pool.len(), MAX_MOTES, "pool never exceeds MAX_MOTES");
        assert!(
            !pool.motes().iter().any(|m| *m == first),
            "the oldest mote was shifted out (FIFO), not kept"
        );
    }

    #[test]
    fn step_ages_settles_and_prunes() {
        let mut pool = DustPool::new();
        let mut rng = DustRng::new(3);
        pool.spawn(5.0, 1.0, 5.0, opts(8), &mut rng);
        assert_eq!(pool.len(), 8);

        // Step well past the longest possible life (0.85 s) — every mote prunes.
        for _ in 0..120 {
            pool.step(1.0 / 60.0);
        }
        assert!(pool.is_empty(), "all motes pruned once past their life");
    }

    #[test]
    fn step_settles_a_mote_on_the_ground() {
        // A mote launched downward never sinks below GROUND_Y, and its vy is zeroed
        // when it lands (the TS floor clamp).
        let mut pool = DustPool::new();
        pool.motes.push(Mote {
            x: 0.0,
            y: 0.5,
            z: 0.0,
            vx: 0.0,
            vy: -5.0,
            vz: 0.0,
            age: 0.0,
            life: 1.0,
            size: 1.0,
            r: 0.0,
            g: 0.0,
            b: 0.0,
        });
        for _ in 0..10 {
            pool.step(1.0 / 60.0);
        }
        let m = &pool.motes()[0];
        assert!(m.y >= GROUND_Y - 1e-9, "mote settled at/above the ground, y = {}", m.y);
        assert_eq!(m.vy, 0.0, "downward velocity zeroed on landing");
    }

    #[test]
    fn render_scale_grows_in_then_shrinks_out() {
        // The Dust.tsx fade-by-scale curve: ~0 at birth, peaks near the end of the
        // 20% grow window, then decays back toward 0 at end of life.
        let mut m = Mote {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            age: 0.0,
            life: 1.0,
            size: 1.0,
            r: 0.0,
            g: 0.0,
            b: 0.0,
        };
        // Birth: grow≈0 → scale near the 0.0001 floor.
        m.age = 0.0;
        assert!(m.render_scale() <= 0.001, "tiny at birth");
        // End of the grow window (k = 0.2): grow = 1, out = 0.8 → size*0.55*0.8.
        m.age = 0.2;
        let peak = m.render_scale();
        assert!((peak - RENDER_SCALE * 0.8).abs() < 1e-9, "peak at end of grow, got {peak}");
        // Mid-life is past the peak and shrinking.
        m.age = 0.6;
        assert!(m.render_scale() < peak, "shrinking after the peak");
        // End of life: out = 0 → floored at ~0.
        m.age = 1.0;
        assert!(m.render_scale() <= 0.001, "gone by end of life");
    }

    #[test]
    fn spawn_is_deterministic_for_a_seed() {
        // Same seed + same call → identical motes (the determinism the RNG buys us).
        let mut a = DustPool::new();
        let mut b = DustPool::new();
        let mut ra = DustRng::new(42);
        let mut rb = DustRng::new(42);
        a.spawn(3.0, 1.0, 4.0, opts(6), &mut ra);
        b.spawn(3.0, 1.0, 4.0, opts(6), &mut rb);
        assert_eq!(a.motes(), b.motes(), "deterministic puff for a fixed seed");
    }
}
