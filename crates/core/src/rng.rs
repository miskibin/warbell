//! Shared deterministic RNG (mulberry32).
//!
//! Several FX/combat systems in the game crate each carried a byte-identical copy
//! of this generator (`CombatRng`, `OrbRng`, `SparkRng`). They are unified here so
//! there is one source of truth. The `next` math is copied VERBATIM from those
//! impls so the output is bit-identical to the prior per-crate copies — seeding
//! and the order/number of draws at every call site are unchanged.
//!
//! NOTE: the xorshift RNG in `audio.rs` and the LCG in `player_ctl.rs` are
//! DIFFERENT algorithms and are intentionally NOT represented here.

/// Tiny deterministic RNG (mulberry32) — fast, decent distribution, reproducible.
#[derive(Clone, Copy)]
pub struct Mulberry32 {
    s: u32,
}

impl Mulberry32 {
    /// Construct seeded with the given state.
    pub fn new(seed: u32) -> Self {
        Mulberry32 { s: seed }
    }

    /// Next f64 in [0, 1). mulberry32.
    pub fn next(&mut self) -> f64 {
        self.s = self.s.wrapping_add(0x6D2B_79F5);
        let mut t = self.s;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        let r = (t ^ (t >> 14)) as f64;
        r / 4_294_967_296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first five draws for seed `0x9E37_79B9` (the old `CombatRng` default),
    /// captured from the original per-crate impl. Asserts the merged generator is
    /// bit-identical to what every call site previously produced.
    #[test]
    fn known_sequence() {
        let mut rng = Mulberry32::new(0x9E37_79B9);
        let expected = [
            0.358_889_980_241_656_3,
            0.105_903_261_341_154_58,
            0.675_290_479_324_758_05,
            0.917_934_558_819_979_43,
            0.101_577_150_402_590_63,
        ];
        for (i, e) in expected.iter().enumerate() {
            let got = rng.next();
            assert!(
                (got - e).abs() < 1e-15,
                "draw {i}: got {got}, expected {e}"
            );
        }
    }
}
