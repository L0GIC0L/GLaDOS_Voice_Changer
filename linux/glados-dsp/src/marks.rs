use crate::kmath::round_f64;

/// Pitch marks: one position per glottal cycle, all at the same phase of the waveform.
/// PSOLA cuts a grain around each mark, so consistency matters more than exact placement.
pub struct Marks;

#[inline]
fn at(x: &[f32], i: i32) -> f32 {
    x[i as usize]
}

impl Marks {
    /// Next mark after `prev`, expected about `period` samples later. Picks the offset whose
    /// surrounding waveform best matches the waveform around `prev` (normalised cross-correlation),
    /// which keeps the chain locked to one phase even as the pitch drifts.
    ///
    /// Reads `x[i & mask]`, so `x` may be a ring buffer or a plain array.
    pub fn next(x: &[f32], mask: i32, prev: i64, period: f64) -> i64 {
        // Positions are folded to 32 bits like the Kotlin Int arithmetic; only differences leave this function.
        let p = prev as i32;
        let t = period.max(8.0);
        let h = 4.max((t * 0.5) as i32);
        let delta = 2.max((t * 0.2) as i32);
        let predicted = p.wrapping_add(round_f64(t));

        let mut er = 0.0;
        for k in -h..=h {
            let v = at(x, p.wrapping_add(k) & mask);
            er += (v * v) as f64;
        }
        if er < 1e-10 {
            return prev + round_f64(t) as i64;
        }

        let mut best = -2.0;
        let mut best_c = predicted;
        for d in -delta..=delta {
            let c = predicted.wrapping_add(d);
            let mut cross = 0.0;
            let mut ec = 0.0;
            for k in -h..=h {
                let a = at(x, p.wrapping_add(k) & mask);
                let b = at(x, c.wrapping_add(k) & mask);
                cross += (a * b) as f64;
                ec += (b * b) as f64;
            }
            let score = cross / (er * ec + 1e-20).sqrt() - 0.002 * d.abs() as f64 / delta as f64;
            if score > best {
                best = score;
                best_c = c;
            }
        }
        prev + best_c.wrapping_sub(p) as i64
    }

    /// Position of the largest absolute sample in `[from, from + length)`.
    pub fn peak(x: &[f32], mask: i32, from: i64, length: i32) -> i64 {
        let f = from as i32;
        let mut best_k = 0;
        let mut best_v = -1f32;
        for k in 0..length {
            let v = at(x, f.wrapping_add(k) & mask).abs();
            if v > best_v {
                best_v = v;
                best_k = k;
            }
        }
        from + best_k as i64
    }
}
