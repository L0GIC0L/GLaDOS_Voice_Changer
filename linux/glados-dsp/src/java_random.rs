//! `java.util.Random`, bit for bit, so random speed curves (and the test signals) match the Android version.

// The Gaussians and bounded ints are only used by the tests.
#![cfg_attr(not(test), allow(dead_code))]

const MULTIPLIER: i64 = 0x5DEECE66D;
const ADDEND: i64 = 0xB;
const MASK: i64 = (1 << 48) - 1;
const DOUBLE_UNIT: f64 = 1.0 / (1u64 << 53) as f64;

/// The 48-bit linear congruential generator of `java.util.Random`.
#[derive(Clone, Debug)]
pub(crate) struct JavaRandom {
    seed: i64,
    next_next_gaussian: f64,
    have_next_next_gaussian: bool,
}

impl JavaRandom {
    pub(crate) fn new(seed: i64) -> Self {
        JavaRandom { seed: (seed ^ MULTIPLIER) & MASK, next_next_gaussian: 0.0, have_next_next_gaussian: false }
    }

    pub(crate) fn next(&mut self, bits: i32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub(crate) fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// Uniform in `[0, bound)`.
    pub(crate) fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        let mut r = self.next(31);
        let m = bound - 1;
        if bound & m == 0 {
            // A power of two: take the high bits.
            return ((bound as i64 * r as i64) >> 31) as i32;
        }
        let mut u = r;
        loop {
            r = u % bound;
            if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                return r;
            }
            u = self.next(31);
        }
    }

    pub(crate) fn next_double(&mut self) -> f64 {
        let hi = (self.next(26) as i64) << 27;
        let lo = self.next(27) as i64;
        (hi + lo) as f64 * DOUBLE_UNIT
    }

    pub(crate) fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    /// Marsaglia's polar method with the cached second value, as Java has it (including `StrictMath.log`).
    pub(crate) fn next_gaussian(&mut self) -> f64 {
        if self.have_next_next_gaussian {
            self.have_next_next_gaussian = false;
            return self.next_next_gaussian;
        }
        let (mut v1, mut v2, mut s);
        loop {
            v1 = 2.0 * self.next_double() - 1.0;
            v2 = 2.0 * self.next_double() - 1.0;
            s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                break;
            }
        }
        let multiplier = (-2.0 * fdlibm_log(s) / s).sqrt();
        self.next_next_gaussian = v2 * multiplier;
        self.have_next_next_gaussian = true;
        v1 * multiplier
    }
}

/// fdlibm's `__ieee754_log`, which `StrictMath.log` is defined to be. The system `ln` can differ in the last bit.
/// The constants are written exactly as in fdlibm's e_log.c.
#[allow(clippy::excessive_precision)]
pub(crate) fn fdlibm_log(x: f64) -> f64 {
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const TWO54: f64 = 1.80143985094819840000e+16;
    const LG1: f64 = 6.666666666666735130e-01;
    const LG2: f64 = 3.999999999940941908e-01;
    const LG3: f64 = 2.857142874366239149e-01;
    const LG4: f64 = 2.222219843214978396e-01;
    const LG5: f64 = 1.818357216161805012e-01;
    const LG6: f64 = 1.531383769920937332e-01;
    const LG7: f64 = 1.479819860511658591e-01;

    let mut x = x;
    let bits = x.to_bits();
    let mut hx = (bits >> 32) as i32;
    let lx = bits as u32;
    let mut k: i32 = 0;
    if hx < 0x00100000 {
        // x < 2^-1022
        if ((hx & 0x7fffffff) as u32 | lx) == 0 {
            return f64::NEG_INFINITY; // log(+-0) = -inf
        }
        if hx < 0 {
            return f64::NAN; // log(-#) = NaN
        }
        k -= 54;
        x *= TWO54; // subnormal number, scale up x
        hx = (x.to_bits() >> 32) as i32;
    }
    if hx >= 0x7ff00000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000fffff;
    let i = (hx + 0x95f64) & 0x100000;
    // Normalise x or x/2.
    x = f64::from_bits((((hx | (i ^ 0x3ff00000)) as u32 as u64) << 32) | (x.to_bits() & 0xffff_ffff));
    k += i >> 20;
    let f = x - 1.0;
    if (0x000fffff & (2 + hx)) < 3 {
        // |f| < 2^-20
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = k as f64;
            return dk * LN2_HI + dk * LN2_LO;
        }
        let r = f * f * (0.5 - 0.33333333333333333 * f);
        if k == 0 {
            return f - r;
        }
        let dk = k as f64;
        return dk * LN2_HI - ((r - dk * LN2_LO) - f);
    }
    let s = f / (2.0 + f);
    let dk = k as f64;
    let z = s * s;
    let mut i = hx - 0x6147a;
    let w = z * z;
    let j = 0x6b851 - hx;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    i |= j;
    let r = t2 + t1;
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
        }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::excessive_precision)]
    fn fdlibm_constants_and_accuracy() {
        assert_eq!(0x3FE5555555555593, 6.666666666666735130e-01f64.to_bits());
        assert_eq!(0x3FC2F112DF3E5244, 1.479819860511658591e-01f64.to_bits());
        let mut r = JavaRandom::new(11);
        for _ in 0..10000 {
            let x = r.next_double() * 4.0 + 1e-9;
            let a = fdlibm_log(x);
            let b = x.ln();
            assert!((a - b).abs() <= 2.0 * f64::EPSILON * b.abs().max(1e-300), "{x}: {a} vs {b}");
        }
        assert_eq!(0.0, fdlibm_log(1.0));
        assert!((fdlibm_log(std::f64::consts::E) - 1.0).abs() < 1e-15);
        assert!((fdlibm_log(1e-310) - 1e-310f64.ln()).abs() < 1e-12);
    }

    /// Values printed by `java.util.Random` on OpenJDK 21: nextInt(), nextInt(10), nextInt(2^20),
    /// nextInt(1000000007), nextDouble(), nextBoolean(), then three nextGaussian(), in that order.
    #[allow(clippy::type_complexity)]
    const REFERENCE: [(i64, [i32; 4], u64, bool, [u64; 3]); 7] = [
        (0, [-1155484576, 8, 252220, 302116440], 0x3fe465b93a78ef81, true, [0xbfd732068a7ed544, 0x3fd10cfbc79228fe, 0xbff6b7e8d84fb1af]),
        (1, [-1155869325, 8, 430000, 874970313], 0x3fca96666128d71c, false, [0x3feecbe39f5fdca6, 0x3ff46d0b7ff51745, 0xbfce7b196f0e17f9]),
        (42, [-1170105035, 3, 716411, 102948884], 0x3fd3c20f3f12bbb4, false, [0x3fd11618765ec424, 0xbfe0cf112d5e46a3, 0xbfec72c6e2101ffd]),
        (3, [-1155099828, 0, 74440, 729262574], 0x3fb12ec6e53d49e0, true, [0x3fce92b86c1e9348, 0x3ff1eb0c78a66be2, 0xbfc86f9796db83aa]),
        (-7, [1155869324, 7, 705881, 119890471], 0x3fc2be14fb429624, false, [0xbfeb4b1027ae7a1c, 0xbfb4f4f5c7e29d24, 0x3fff0ff714015f35]),
        (123456789012345, [-1421006947, 4, 906057, 308132645], 0x3fec35bbf6b7031b, false, [0x3fe62845b5a22152, 0xbfc59afd01261d91, 0x3fc64e093fd63fd0]),
        (25214616429, [-861466467, 0, 267914, 518734901], 0x3febab0413879577, true, [0x3fef18660d5b6c15, 0xbfd3d7e5960360ea, 0xbfe541071f847788]),
    ];

    #[test]
    fn matches_java_util_random() {
        for (seed, ints, double, boolean, gaussians) in REFERENCE {
            let mut r = JavaRandom::new(seed);
            assert_eq!(ints[0], r.next_int(), "seed {seed}");
            assert_eq!(ints[1], r.next_int_bound(10), "seed {seed}");
            assert_eq!(ints[2], r.next_int_bound(1 << 20), "seed {seed}");
            assert_eq!(ints[3], r.next_int_bound(1000000007), "seed {seed}");
            assert_eq!(double, r.next_double().to_bits(), "seed {seed}");
            assert_eq!(boolean, r.next_boolean(), "seed {seed}");
            for g in gaussians {
                assert_eq!(g, r.next_gaussian().to_bits(), "seed {seed}");
            }
        }
    }

    /// Checksums printed by the same Java program: a million Gaussians, and StrictMath.log of a million doubles.
    #[test]
    fn a_million_gaussians_and_logs_match_java_bit_for_bit() {
        let mut r = JavaRandom::new(5);
        let mut x: i64 = 0;
        for _ in 0..1_000_000 {
            x = x.wrapping_mul(31).wrapping_add(r.next_gaussian().to_bits() as i64);
        }
        assert_eq!(-4596470278305693184, x);
        let mut q = JavaRandom::new(9);
        let mut y: i64 = 0;
        for _ in 0..1_000_000 {
            y = y.wrapping_mul(31).wrapping_add(fdlibm_log(q.next_double() * 1e3 + 1e-300).to_bits() as i64);
        }
        assert_eq!(5331171110237992592, y);
    }
}
