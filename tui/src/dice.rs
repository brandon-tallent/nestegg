//! Repeatable randomness for invented data: one seed always draws the same numbers

/// FNV-1a over `text`, salted with `seed`
pub fn hash(seed: u64, text: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    /// The multiplier of MurmurHash3's 64-bit finalizer
    const MIX: u64 = 0xff51_afd7_ed55_8ccd;
    const MIX_SHIFT: u32 = 33;
    let mut h = OFFSET ^ seed;
    for byte in text.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(PRIME);
    }
    // Final mix so nearby inputs do not share their high bits
    h ^= h >> MIX_SHIFT;
    h = h.wrapping_mul(MIX);
    h ^ (h >> MIX_SHIFT)
}

/// A value in `[0, 1)` derived from `hash`
pub fn unit(hash: u64) -> f64 {
    /// An f64 holds this many bits exactly
    const MANTISSA_BITS: u32 = 53;
    (hash >> (u64::BITS - MANTISSA_BITS)) as f64 / (1u64 << MANTISSA_BITS) as f64
}

/// A repeatable run of random draws
pub struct Dice {
    seed: u64,
    drawn: u64,
}

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self { seed, drawn: 0 }
    }

    /// A value in `[0, 1)`
    pub fn unit(&mut self) -> f64 {
        self.drawn += 1;
        unit(hash(self.seed, &self.drawn.to_string()))
    }

    pub fn between(&mut self, (low, high): (f64, f64)) -> f64 {
        low + self.unit() * (high - low)
    }

    /// A whole number from `low` to `high`, both included
    pub fn whole(&mut self, (low, high): (i64, i64)) -> i64 {
        low + (self.unit() * (high - low + 1) as f64) as i64
    }

    pub fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_seed_always_draws_the_same_and_another_differently() {
        let draws = |seed: u64| -> Vec<i64> {
            let mut dice = Dice::new(seed);
            (0..20).map(|_| dice.whole((1, 1000))).collect()
        };
        assert_eq!(draws(7), draws(7));
        assert_ne!(draws(7), draws(8));
    }

    #[test]
    fn draws_stay_within_what_was_asked_for() {
        let mut dice = Dice::new(7);
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&dice.unit()));
            assert!((3..=6).contains(&dice.whole((3, 6))));
            assert!((0.25..4.0).contains(&dice.between((0.25, 4.0))));
        }
        let mut seen: Vec<i64> = (0..1000).map(|_| dice.whole((3, 6))).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen, vec![3, 4, 5, 6]);
    }
}
