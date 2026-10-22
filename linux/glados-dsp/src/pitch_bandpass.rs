use std::f64::consts::PI;

#[derive(Clone, Debug)]
struct Section {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Section {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Section { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    fn step(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

const Q4: [f64; 2] = [0.5411961, 1.3065630];

/// Band-pass used only to help the pitch tracker; it is never heard. Phone and room recordings carry
/// rumble below the voice and hiss above it, and either one drowns the periodicity the tracker looks
/// for. Four-pole Butterworth high-pass and low-pass, each as two biquads.
///
/// Use [`PitchBandpass::process`] for a stream (it adds a small, frequency-dependent delay) or
/// [`PitchBandpass::zero_phase`] for a whole clip (no delay, so positions line up with the original audio).
#[derive(Clone, Debug)]
pub struct PitchBandpass {
    sections: Vec<Section>,
}

impl PitchBandpass {
    pub fn new(sample_rate: i32, fmin: f64, fmax: f64) -> Self {
        let hp = 40.0f64.max(0.7 * fmin);
        let lp = (sample_rate as f64 * 0.45).min(1200.0f64.max(2.5 * fmax));
        let mut sections = Vec::with_capacity(4);
        for q in Q4 {
            sections.push(design(sample_rate, hp, q, true));
        }
        for q in Q4 {
            sections.push(design(sample_rate, lp, q, false));
        }
        PitchBandpass { sections }
    }

    pub fn reset(&mut self) {
        for s in &mut self.sections {
            s.reset();
        }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let mut v = x as f64;
        for s in &mut self.sections {
            v = s.step(v);
        }
        v as f32
    }

    /// Filters the whole clip forwards and then backwards, so the result has no phase shift.
    pub fn zero_phase(x: &[f32], sample_rate: i32, fmin: f64, fmax: f64) -> Vec<f32> {
        let mut f = PitchBandpass::new(sample_rate, fmin, fmax);
        let mut y = vec![0f32; x.len()];
        for (yi, xi) in y.iter_mut().zip(x) {
            *yi = f.process(*xi);
        }
        f.reset();
        for yi in y.iter_mut().rev() {
            *yi = f.process(*yi);
        }
        y
    }
}

fn design(sr: i32, fc: f64, q: f64, high: bool) -> Section {
    let w = 2.0 * PI * fc / sr as f64;
    let c = w.cos();
    let alpha = w.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    if high {
        Section::new((1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0, -2.0 * c / a0, (1.0 - alpha) / a0)
    } else {
        Section::new((1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0, -2.0 * c / a0, (1.0 - alpha) / a0)
    }
}
