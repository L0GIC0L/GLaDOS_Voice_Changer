use std::f64::consts::PI;

/// YIN fundamental-frequency detector (de Cheveigné and Kawahara, 2002).
///
/// The search runs on a copy decimated to roughly 12 kHz, then the winning lag is refined on the
/// original samples. Input is read as `x[(start + i) & mask]`, so it works on a ring buffer
/// (mask = size - 1) or a plain array (mask = -1).
#[derive(Clone, Debug)]
pub struct Yin {
    sample_rate: i32,
    decim: i32,
    tau_min: i32,
    tau_max: i32,
    w: i32,
    decimated: Vec<f32>,
    diff: Vec<f64>,
    cmnd: Vec<f64>,
    fir: Vec<f32>,
    refine_cost: Vec<f64>,

    /// Number of input samples read by [`Yin::detect`].
    pub window_length: i32,

    /// Estimated fundamental in Hz. Meaningful when [`Yin::detect`] returned true; best guess otherwise.
    pub f0: f64,

    /// Normalised difference at the chosen lag: 0 is perfectly periodic, 1 is noise.
    pub aperiodicity: f64,
}

#[inline]
fn at(x: &[f32], i: i32) -> f32 {
    x[i as usize]
}

impl Yin {
    pub fn new(sample_rate: i32, fmin: f64, fmax: f64) -> Self {
        let decim = 1.max((sample_rate as f64 / 12000.0) as i32);
        let srd = sample_rate as f64 / decim as f64;
        let tau_min = 2.max((srd / fmax).floor() as i32);
        let tau_max = (srd / fmin).ceil() as i32;
        let w = tau_max;
        Yin {
            sample_rate,
            decim,
            tau_min,
            tau_max,
            w,
            decimated: vec![0f32; (w + tau_max + 1) as usize],
            diff: vec![0.0; (tau_max + 2) as usize],
            cmnd: vec![0.0; (tau_max + 2) as usize],
            fir: design_lowpass(decim),
            refine_cost: vec![0.0; (2 * decim + 4) as usize],
            window_length: decim * (w + tau_max),
            f0: 0.0,
            aperiodicity: 1.0,
        }
    }

    /// `threshold` is the absolute YIN threshold; lower is stricter (typical 0.10 to 0.25).
    /// Returns true if the window is judged periodic.
    pub fn detect(&mut self, x: &[f32], mask: i32, start: i32, threshold: f64) -> bool {
        let tau_max = self.tau_max;
        let w = self.w;
        let n = w + tau_max;
        self.decimate(x, mask, start, n);

        let decimated = &self.decimated;
        for tau in 1..=tau_max {
            let mut s = 0.0;
            for j in 0..w {
                let d = (decimated[j as usize] - decimated[(j + tau) as usize]) as f64;
                s += d * d;
            }
            self.diff[tau as usize] = s;
        }
        let cmnd = &mut self.cmnd;
        let diff = &self.diff;
        cmnd[0] = 1.0;
        let mut running = 0.0;
        for tau in 1..=tau_max as usize {
            running += diff[tau];
            cmnd[tau] = if running > 1e-12 { diff[tau] * tau as f64 / running } else { 1.0 };
        }

        // First dip below the threshold, followed down to its local minimum.
        let mut tau: i32 = -1;
        let mut t = self.tau_min;
        while t < tau_max {
            if cmnd[t as usize] < threshold {
                while t + 1 < tau_max && cmnd[(t + 1) as usize] < cmnd[t as usize] {
                    t += 1;
                }
                tau = t;
                break;
            }
            t += 1;
        }
        if tau < 0 {
            let mut best = self.tau_min;
            for i in self.tau_min..=tau_max {
                if cmnd[i as usize] < cmnd[best as usize] {
                    best = i;
                }
            }
            tau = best;
        }
        self.aperiodicity = cmnd[tau as usize];

        let mut lag = tau as f64;
        if tau > 1 && tau < tau_max {
            let a = cmnd[(tau - 1) as usize];
            let b = cmnd[tau as usize];
            let c = cmnd[(tau + 1) as usize];
            let denom = a - 2.0 * b + c;
            if denom > 1e-12 {
                lag += (0.5 * (a - c) / denom).clamp(-1.0, 1.0);
            }
        }

        let mut lag_full = lag * self.decim as f64;
        if self.decim > 1 {
            lag_full = self.refine(x, mask, start, lag_full);
        }
        self.f0 = self.sample_rate as f64 / lag_full;
        self.aperiodicity < threshold
    }

    fn decimate(&mut self, x: &[f32], mask: i32, start: i32, n: i32) {
        if self.decim == 1 {
            for i in 0..n {
                self.decimated[i as usize] = at(x, start.wrapping_add(i) & mask);
            }
            return;
        }
        let fir = &self.fir;
        let half = (fir.len() / 2) as i32;
        let last = self.window_length - 1;
        for i in 0..n {
            let center = i * self.decim + self.decim / 2;
            let mut acc = 0f32;
            for (k, h) in fir.iter().enumerate() {
                let idx = (center + k as i32 - half).max(0).min(last);
                acc += h * at(x, start.wrapping_add(idx) & mask);
            }
            self.decimated[i as usize] = acc;
        }
    }

    /// Re-search the lag on the original samples around the decimated estimate.
    fn refine(&mut self, x: &[f32], mask: i32, start: i32, lag_guess: f64) -> f64 {
        let lo = 2.max(lag_guess.floor() as i32 - self.decim);
        let hi = lag_guess.ceil() as i32 + self.decim;
        let len = self.window_length - hi;
        if len < lo {
            return lag_guess;
        }
        let count = hi - lo + 1;
        let d = &mut self.refine_cost;
        let mut best_i = 0usize;
        for i in 0..count as usize {
            let lag = lo + i as i32;
            let mut s = 0.0;
            for j in 0..len {
                let e = (at(x, start.wrapping_add(j) & mask) - at(x, start.wrapping_add(j).wrapping_add(lag) & mask)) as f64;
                s += e * e;
            }
            d[i] = s;
            if s < d[best_i] {
                best_i = i;
            }
        }
        let mut lag = (lo + best_i as i32) as f64;
        if best_i >= 1 && (best_i as i32) < count - 1 {
            let a = d[best_i - 1];
            let b = d[best_i];
            let c = d[best_i + 1];
            let denom = a - 2.0 * b + c;
            if denom > 1e-18 {
                lag += (0.5 * (a - c) / denom).clamp(-1.0, 1.0);
            }
        }
        lag
    }
}

fn design_lowpass(decim: i32) -> Vec<f32> {
    if decim == 1 {
        return vec![1f32];
    }
    let half = 2 * decim;
    let cutoff = 0.45 / decim as f64;
    let mut h = vec![0f32; (2 * half + 1) as usize];
    let mut sum = 0.0;
    for k in -half..=half {
        let sinc = if k == 0 { 2.0 * cutoff } else { (2.0 * PI * cutoff * k as f64).sin() / (PI * k as f64) };
        let win = 0.5 + 0.5 * (PI * k as f64 / (half + 1) as f64).cos();
        h[(k + half) as usize] = (sinc * win) as f32;
        sum += h[(k + half) as usize] as f64;
    }
    for v in h.iter_mut() {
        *v = (*v as f64 / sum) as f32;
    }
    h
}
