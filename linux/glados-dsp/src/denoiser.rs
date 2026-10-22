use std::f64::consts::PI;

/// Smoothing of the decision-directed estimate; 0.98 is the classic value.
const ALPHA: f64 = 0.98;

/// Lowest a-priori SNR, about -25 dB. Keeps residual noise smooth instead of twinkling.
const XI_MIN: f64 = 0.003;

/// Share of frames, quietest first, averaged into the noise estimate.
const QUIET_SHARE: f64 = 0.1;

/// Per-frame smoothing of the noise level tracker.
const TRACK_SMOOTHING: f64 = 0.95;

/// The tracker's fixed a-priori SNR for "speech is present", 15 dB (Gerkmann and Hendriks).
const XI_SPEECH: f64 = 31.6;

/// The tracked level sits a little low on average; this brings it back to the true noise level.
const LEVEL_BIAS: f64 = 1.2;

/// The tracker only needs the overall level, so it follows every fourth frequency bin.
const TRACK_STEP: usize = 4;

/// Java's `Integer.highestOneBit`.
fn highest_one_bit(i: i32) -> i32 {
    i & (0x8000_0000u32.wrapping_shr((i as u32).leading_zeros()) as i32)
}

/// Offline noise reduction for steady background noise: hiss, fans, air conditioning, hum.
///
/// The clip is cut into overlapping short-time spectra. The noise spectrum is learned from the clip itself, as the
/// average of its quietest frames (the pauses between words), and its level is then followed through the clip so noise
/// that swells and fades (a fan, a phone's automatic gain) is still removed evenly. Each frequency is then scaled by a
/// Wiener gain whose signal-to-noise estimate uses the decision-directed rule of Ephraim and Malah, which removes noise
/// with far less of the warbling "musical noise" left by plain spectral subtraction, refined by the second step of
/// Plapous, Marro and Scalart's two-step noise reduction so the estimate does not lag a frame behind word onsets and
/// endings. A floor on the gain bounds how much is taken away, so quiet speech is turned down a little rather than
/// gated out.
pub struct Denoiser;

impl Denoiser {
    /// Returns a cleaned copy of `x`. `amount` 0..1 sets how far noise may be pushed down (up to about 30 dB) and how
    /// boldly it is estimated. 0 (or a clip of nothing but digital silence) returns an unchanged copy of `x`. If
    /// `cancelled` turns true the result is incomplete and should be thrown away.
    pub fn process(x: &[f32], sample_rate: i32, amount: f32, cancelled: &dyn Fn() -> bool) -> Vec<f32> {
        let t = amount.clamp(0.0, 1.0);
        if t <= 0.0 || x.is_empty() {
            return x.to_vec();
        }
        let n = highest_one_bit((0.04 * sample_rate as f64) as i32).clamp(256, 4096) as usize;
        let hop = n / 4;
        let bins = n / 2 + 1;
        let fft = Fft::new(n as i32);
        let win: Vec<f32> = (0..n).map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()).sqrt() as f32).collect();

        // Pad by a whole frame at each end so the first and last samples are covered by a full set of frames.
        let mut padded = vec![0f32; x.len() + 2 * n];
        padded[n..n + x.len()].copy_from_slice(x);
        let frames = (padded.len() - n) / hop + 1;

        let Some(profile) = noise_spectrum(&padded, frames, hop, &win, &fft) else {
            return x.to_vec();
        };
        let levels = noise_levels(&padded, frames, hop, &win, &fft, &profile, cancelled);
        let over_subtract = 1.0 + 0.75 * t as f64;
        let floor = 10f64.powf(-30.0 * t as f64 / 20.0);
        let mut noise = vec![0.0; bins];

        let mut re = vec![0.0; n];
        let mut im = vec![0.0; n];
        let mut prev_clean = vec![0.0; bins]; // |S|^2 estimate from the previous frame, before the floor
        let mut out = vec![0f32; padded.len()];
        let mut first = true;
        for (f, &level) in levels.iter().enumerate() {
            if f % 64 == 0 && cancelled() {
                break;
            }
            let start = f * hop;
            for k in 0..bins {
                noise[k] = (profile[k] * level * over_subtract).max(1e-20);
            }
            for i in 0..n {
                re[i] = (padded[start + i] * win[i]) as f64;
                im[i] = 0.0;
            }
            fft.transform(&mut re, &mut im, false);
            for k in 0..bins {
                let power = re[k] * re[k] + im[k] * im[k];
                let gamma = power / noise[k];
                let ml = (gamma - 1.0).max(0.0);
                let xi = if first {
                    ml.max(XI_MIN)
                } else {
                    (ALPHA * prev_clean[k] / noise[k] + (1.0 - ALPHA) * ml).max(XI_MIN)
                };
                let g1 = xi / (1.0 + xi);
                // The floor is left out of the recursion: fed back, it makes the estimate follow the leftover noise,
                // which twinkles at gentle settings.
                prev_clean[k] = g1 * g1 * power;
                // Second step: re-estimate the SNR from this frame's own first estimate, removing the one-frame lag.
                let xi2 = (g1 * g1 * gamma).max(XI_MIN);
                let g = (xi2 / (1.0 + xi2)).max(floor);
                re[k] *= g;
                im[k] *= g;
                if k >= 1 && k < n / 2 {
                    re[n - k] *= g;
                    im[n - k] *= g;
                }
            }
            first = false;
            fft.transform(&mut re, &mut im, true);
            // sqrt-Hann in and out makes Hann overall, which sums to 2 at a quarter-frame hop.
            for i in 0..n {
                out[start + i] += (re[i] * win[i] as f64 * 0.5) as f32;
            }
        }
        out[n..n + x.len()].to_vec()
    }
}

/// Average power spectrum of the quietest frames, or `None` if there is nothing but digital silence.
fn noise_spectrum(padded: &[f32], frames: usize, hop: usize, win: &[f32], fft: &Fft) -> Option<Vec<f64>> {
    let n = win.len();
    // Frames that lie wholly inside the clip, or every frame if the clip is shorter than one.
    let first_real = (n / hop) as i64;
    let last_real = frames as i64 - 1 - (n / hop) as i64;
    let range = if last_real >= first_real { first_real as usize..last_real as usize + 1 } else { 0..frames };
    let mut energy = vec![0.0; frames];
    for f in range.clone() {
        let mut e = 0.0;
        let s = f * hop;
        for i in 0..n {
            let v = padded[s + i] * win[i];
            e += (v * v) as f64;
        }
        energy[f] = e;
    }
    // Exact digital silence (a recorder warming up, padding) is not the noise to remove, so leave it out.
    let silence = 1e-12 * n as f64;
    let mut idx: Vec<usize> = range.filter(|&f| energy[f] > silence).collect();
    if idx.is_empty() {
        return None;
    }
    let take = 8.min(idx.len()).max((idx.len() as f64 * QUIET_SHARE) as usize);
    // A stable sort, like Kotlin's sortedBy, so equal energies keep their order.
    idx.sort_by(|a, b| energy[*a].total_cmp(&energy[*b]));
    idx.truncate(take);
    quiet_spectrum(padded, &idx, hop, win, fft)
}

/// How loud the noise is in each frame, relative to `profile`. Gerkmann and Hendriks's speech-presence-probability
/// tracker follows the noise in every bin, forwards and then backwards through the clip, starting from the profile;
/// only the overall level is kept (the geometric mean over bins), so the profile's shape stays put, and the smaller
/// of the two directions is used, so a tracker still catching up after a word is not mistaken for louder noise.
/// Frames of exact digital silence leave the level unchanged. Memory is one number per frame per direction.
fn noise_levels(
    padded: &[f32], frames: usize, hop: usize, win: &[f32], fft: &Fft, profile: &[f64], cancelled: &dyn Fn() -> bool,
) -> Vec<f64> {
    let forward = track_levels(padded, 0..frames, frames, hop, win, fft, profile, cancelled);
    let backward = track_levels(padded, (0..frames).rev(), frames, hop, win, fft, profile, cancelled);
    forward.iter().zip(&backward).map(|(a, b)| a.min(*b) * LEVEL_BIAS).collect()
}

#[allow(clippy::too_many_arguments)]
fn track_levels(
    padded: &[f32], order: impl Iterator<Item = usize>, frames: usize, hop: usize, win: &[f32], fft: &Fft,
    profile: &[f64], cancelled: &dyn Fn() -> bool,
) -> Vec<f64> {
    let n = win.len();
    let bins = profile.len();
    let silence = 1e-12 * n as f64;
    let mut noise = profile.to_vec();
    let mut levels = vec![0.0; frames];
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    let mut level = 1.0;
    let mut count = 0;
    for f in order {
        count += 1;
        if count % 64 == 0 && cancelled() {
            break;
        }
        let s = f * hop;
        let mut e = 0.0;
        for i in 0..n {
            let v = padded[s + i] * win[i];
            re[i] = v as f64;
            im[i] = 0.0;
            e += (v * v) as f64;
        }
        if e > silence {
            fft.transform(&mut re, &mut im, false);
            let mut log_sum = 0.0;
            let mut used = 0;
            let mut k = 1;
            while k < bins - 1 {
                let y = re[k] * re[k] + im[k] * im[k];
                // Probability that speech is present in this bin, given how far it stands above the noise.
                let p = 1.0
                    / (1.0 + (1.0 + XI_SPEECH) * (-y / noise[k].max(1e-30) * XI_SPEECH / (1.0 + XI_SPEECH)).exp());
                noise[k] = TRACK_SMOOTHING * noise[k] + (1.0 - TRACK_SMOOTHING) * ((1.0 - p) * y + p * noise[k]);
                if profile[k] > 0.0 {
                    log_sum += (noise[k].max(1e-30) / profile[k]).ln();
                    used += 1;
                }
                k += TRACK_STEP;
            }
            if used > 0 {
                level = (log_sum / used as f64).exp();
            }
        }
        levels[f] = level;
    }
    levels
}

fn quiet_spectrum(padded: &[f32], frames: &[usize], hop: usize, win: &[f32], fft: &Fft) -> Option<Vec<f64>> {
    let n = win.len();
    let bins = n / 2 + 1;
    let mut sum = vec![0.0; bins];
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    for &f in frames {
        let s = f * hop;
        for i in 0..n {
            re[i] = (padded[s + i] * win[i]) as f64;
            im[i] = 0.0;
        }
        fft.transform(&mut re, &mut im, false);
        for k in 0..bins {
            sum[k] += re[k] * re[k] + im[k] * im[k];
        }
    }
    let mut total = 0.0;
    for v in sum.iter_mut() {
        *v /= frames.len() as f64;
        total += *v;
    }
    if total <= 0.0 { None } else { Some(sum) }
}

/// In-place iterative radix-2 complex FFT. The inverse includes the 1/n scaling.
pub(crate) struct Fft {
    n: usize,
    cos_t: Vec<f64>,
    sin_t: Vec<f64>,
    rev: Vec<usize>,
}

impl Fft {
    pub(crate) fn new(n: i32) -> Self {
        assert!(n >= 2 && n & (n - 1) == 0, "FFT size must be a power of two");
        let bits = n.trailing_zeros();
        let nu = n as usize;
        Fft {
            n: nu,
            cos_t: (0..nu / 2).map(|i| (2.0 * PI * i as f64 / n as f64).cos()).collect(),
            sin_t: (0..nu / 2).map(|i| (2.0 * PI * i as f64 / n as f64).sin()).collect(),
            rev: (0..nu).map(|i| ((i as u32).reverse_bits() >> (32 - bits)) as usize).collect(),
        }
    }

    pub(crate) fn transform(&self, re: &mut [f64], im: &mut [f64], inverse: bool) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let sign = if inverse { 1.0 } else { -1.0 };
        let mut size = 2;
        while size <= n {
            let half = size / 2;
            let step = n / size;
            let mut i = 0;
            while i < n {
                let mut k = 0;
                for j in i..i + half {
                    let wr = self.cos_t[k];
                    let wi = sign * self.sin_t[k];
                    let xr = re[j + half] * wr - im[j + half] * wi;
                    let xi = re[j + half] * wi + im[j + half] * wr;
                    re[j + half] = re[j] - xr;
                    im[j + half] = im[j] - xi;
                    re[j] += xr;
                    im[j] += xi;
                    k += step;
                }
                i += size;
            }
            size *= 2;
        }
        if inverse {
            let s = 1.0 / n as f64;
            for i in 0..n {
                re[i] *= s;
                im[i] *= s;
            }
        }
    }
}
