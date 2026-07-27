//! Value noise + fBM + domain warp + ridged variants.
//!
//! Anti-grid discipline (hard-won in Warbell): never use axis-separable sine fields for
//! anything visible — they form a standing-wave lattice. Everything here is hashed value
//! noise with the domain ROTATED between octaves so no feature aligns to world axes.

use super::rng::hash2;

fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Single-octave value noise in [0, 1).
pub fn vnoise(x: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = x - ix as f32;
    let fz = z - iz as f32;
    let ux = quintic(fx);
    let uz = quintic(fz);
    let a = hash2(ix, iz, seed);
    let b = hash2(ix + 1, iz, seed);
    let c = hash2(ix, iz + 1, seed);
    let d = hash2(ix + 1, iz + 1, seed);
    a + (b - a) * ux + (c - a) * uz + (a - b - c + d) * ux * uz
}

/// Rotation applied between octaves (~37°) so octave lattices never line up.
const ROT_C: f32 = 0.7986355;
const ROT_S: f32 = 0.6018150;

/// Standard fBM, output roughly [0, 1].
pub fn fbm(mut x: f32, mut z: f32, octaves: u32, seed: u32) -> f32 {
    let mut amp = 0.5;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for o in 0..octaves {
        sum += amp * vnoise(x, z, seed.wrapping_add(o * 101));
        norm += amp;
        let (nx, nz) = (
            ROT_C * x - ROT_S * z + 13.7,
            ROT_S * x + ROT_C * z + 71.3,
        );
        x = nx * 2.0;
        z = nz * 2.0;
        amp *= 0.5;
    }
    sum / norm
}

/// Ridged fBM — sharp crests, output roughly [0, 1]. Weights successive octaves by the
/// running ridge value so detail concentrates on crests (classic Musgrave trick).
pub fn ridged(mut x: f32, mut z: f32, octaves: u32, seed: u32) -> f32 {
    let mut amp = 0.5;
    let mut sum = 0.0;
    let mut norm = 0.0;
    let mut weight = 1.0_f32;
    for o in 0..octaves {
        let s = 2.0 * vnoise(x, z, seed.wrapping_add(o * 173)) - 1.0;
        let mut r = 1.0 - s.abs();
        r = r * r;
        r *= weight;
        weight = (r * 2.0).clamp(0.0, 1.0);
        sum += amp * r;
        norm += amp;
        let (nx, nz) = (
            ROT_C * x - ROT_S * z - 4.2,
            ROT_S * x + ROT_C * z + 9.1,
        );
        x = nx * 2.0;
        z = nz * 2.0;
        amp *= 0.5;
    }
    sum / norm
}

/// Domain-warped fBM: offsets the sample point by two independent fBM fields first.
/// `warp` is in the same units as x/z (i.e. noise-space, caller scales).
pub fn warped_fbm(x: f32, z: f32, octaves: u32, warp: f32, seed: u32) -> f32 {
    let wx = fbm(x + 31.4, z + 47.2, 4, seed.wrapping_add(900)) - 0.5;
    let wz = fbm(x - 12.9, z + 88.1, 4, seed.wrapping_add(901)) - 0.5;
    fbm(x + wx * warp, z + wz * warp, octaves, seed)
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vnoise_bounded_and_deterministic() {
        for i in 0..500 {
            let x = i as f32 * 0.37;
            let z = i as f32 * 0.61 - 40.0;
            let v = vnoise(x, z, 5);
            assert!((0.0..=1.0).contains(&v), "v={v}");
            assert_eq!(v, vnoise(x, z, 5));
        }
    }

    #[test]
    fn fbm_bounded() {
        for i in 0..200 {
            let v = fbm(i as f32 * 0.13, i as f32 * 0.29, 6, 9);
            assert!((0.0..=1.0).contains(&v));
            let r = ridged(i as f32 * 0.13, i as f32 * 0.29, 5, 9);
            assert!((0.0..=1.0).contains(&r), "r={r}");
        }
    }

    #[test]
    fn seeds_decorrelate() {
        let a: f32 = (0..100)
            .map(|i| (fbm(i as f32 * 0.31, 0.0, 4, 1) - fbm(i as f32 * 0.31, 0.0, 4, 2)).abs())
            .sum();
        assert!(a > 1.0, "different seeds should differ, sum abs diff={a}");
    }
}
