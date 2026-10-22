use crate::java_random::JavaRandom;
use crate::notes::TimeMap;

/// Where each moment of the edited timeline lands in the rendered output once the speed is changed: a steady tempo
/// plus, optionally, random stretches that rush ahead or drag. The same inputs always give the same warp, so a clip
/// keeps its rhythm when it is re-rendered, and the editor can map the playhead back onto the notes.
#[derive(Clone, Debug)]
pub struct SpeedWarp {
    map: Option<TimeMap>,
    pub rendered_length: i32,
}

impl SpeedWarp {
    pub fn is_identity(&self) -> bool {
        self.map.is_none()
    }

    /// Timeline sample at rendered sample `s`.
    pub fn to_timeline(&self, s: f64) -> f64 {
        match &self.map {
            Some(m) => m.to_input(s),
            None => s,
        }
    }

    /// Rendered sample at timeline sample `u`.
    pub fn to_rendered(&self, u: f64) -> f64 {
        match &self.map {
            Some(m) => m.to_output(u),
            None => u,
        }
    }

    pub fn identity(length: i32) -> SpeedWarp {
        SpeedWarp { map: None, rendered_length: length }
    }

    /// * `tempo` – overall speed, 0.5 to 2 (2 talks twice as fast)
    /// * `random` – 0..1: how often, and how far, the speed jumps away from `tempo` for a moment
    /// * `seed` – the Kotlin default is `length as i64`
    pub fn build(length: i32, sample_rate: i32, tempo: f64, random: f64, seed: i64) -> SpeedWarp {
        if (tempo - 1.0).abs() < 1e-6 && random <= 0.0 {
            return SpeedWarp::identity(length);
        }
        let mut rng = JavaRandom::new(seed ^ 0x5DEECE66D);
        let block = 1.max(sample_rate / 200);
        let glide = 1.0 - (-block as f64 / (0.04 * sample_rate as f64)).exp();
        let mut us: Vec<f64> = Vec::new();
        let mut ss: Vec<f64> = Vec::new();
        let mut u: i32 = 0;
        let mut s = 0.0;
        let mut speed = tempo;
        let mut target = tempo;
        let mut segment_left: i32 = 0;
        while u < length {
            if segment_left <= 0 {
                segment_left = ((0.35 + 0.85 * rng.next_double()) * sample_rate as f64) as i32;
                target = tempo;
                if rng.next_double() < random {
                    let octaves = (0.2 + 0.8 * random) * (0.5 + 0.5 * rng.next_double());
                    target = (tempo * 2f64.powf(if rng.next_boolean() { octaves } else { -octaves })).clamp(0.35, 3.0);
                }
            }
            speed += (target - speed) * glide;
            us.push(u as f64);
            ss.push(s);
            let step = block.min(length - u);
            u += step;
            segment_left -= step;
            s += step as f64 / speed;
        }
        us.push(length as f64);
        ss.push(s);
        SpeedWarp { map: Some(TimeMap::new(us, ss)), rendered_length: 1.max(s.ceil() as i32) }
    }
}
