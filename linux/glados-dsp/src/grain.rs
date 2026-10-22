use std::f64::consts::PI;
use std::sync::LazyLock;

const HANN_N: usize = 1024;

static HANN: LazyLock<[f32; HANN_N + 2]> = LazyLock::new(|| {
    let mut h = [0f32; HANN_N + 2];
    for (i, v) in h.iter_mut().enumerate().take(HANN_N + 1) {
        *v = (0.5 - 0.5 * (2.0 * PI * i as f64 / HANN_N as f64).cos()) as f32;
    }
    h[HANN_N + 1] = 0.0;
    h
});

/// Hann window at u in [0, 1]: zero at both edges, one at the middle.
#[inline]
fn window(hann: &[f32; HANN_N + 2], u: f64) -> f32 {
    let pos = u * HANN_N as f64;
    let i = pos as i32;
    let f = (pos - i as f64) as f32;
    let i = i as usize;
    hann[i] + (hann[i + 1] - hann[i]) * f
}

#[inline]
fn at(x: &[f32], i: i32) -> f32 {
    x[i as usize]
}

/// Windowed-grain overlap-add shared by the live and offline PSOLA engines.
pub struct Grain;

impl Grain {
    /// Lower bound for the window-sum divisor. Dividing by the raw sum would turn the faint tails of
    /// widely spaced grains (large downward shifts) back into full-level copies of the neighbouring
    /// pitch pulses, undoing the shift. The floor keeps amplitude flat for moderate shifts and
    /// leaves those tails quiet for large ones.
    pub const NORM_FLOOR: f32 = 0.5;

    /// Catmull-Rom interpolation at fractional index `pos`.
    pub fn cubic(x: &[f32], mask: i32, pos: f64) -> f32 {
        let i = pos.floor() as i32;
        let f = (pos - i as f64) as f32;
        let p0 = at(x, i.wrapping_sub(1) & mask);
        let p1 = at(x, i & mask);
        let p2 = at(x, i.wrapping_add(1) & mask);
        let p3 = at(x, i.wrapping_add(2) & mask);
        let a = -0.5f32 * p0 + 1.5f32 * p1 - 1.5f32 * p2 + 0.5f32 * p3;
        let b = p0 - 2.5f32 * p1 + 2f32 * p2 - 0.5f32 * p3;
        let c = -0.5f32 * p0 + 0.5f32 * p2;
        ((a * f + b) * f + c) * f + p1
    }

    /// Adds one Hann-windowed grain of the source, centred on `center`, into the accumulator at
    /// output position `s`.
    ///
    /// * `half` – grain half-length in source samples (the grain spans center - half .. center + half)
    /// * `rho` – read-speed ratio. Above 1 the grain is shortened, which raises the formants
    ///   without touching the spacing of grains (and so the pitch).
    /// * `gain` – applied to the signal only. The window sum in `norm` is left unscaled so that
    ///   neighbouring grains with different gains crossfade rather than step.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        src: &[f32], smask: i32, center: f64, half: f64,
        acc: &mut [f32], norm: &mut [f32], amask: i32,
        s: f64, rho: f64, gain: f32,
    ) {
        let hann = &*HANN;
        let hs = half / rho;
        let n0 = (s - hs).ceil() as i32;
        let n1 = (s + hs).floor() as i32;
        for n in n0..=n1 {
            let p = (n as f64 - s) * rho;
            let w = window(hann, 0.5 + 0.5 * p / half);
            let v = Grain::cubic(src, smask, center + p) * w;
            let idx = (n & amask) as usize;
            acc[idx] += v * gain;
            norm[idx] += w;
        }
    }
}
