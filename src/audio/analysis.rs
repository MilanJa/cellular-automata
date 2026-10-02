//! Turns a block of audio samples into four levels in 0..1: overall loudness and the energy in
//! three frequency bands, with an auto-gain so quiet and loud sources both use the full range.

/// Samples per analysis block (also the FFT size).
pub const FFT_SIZE: usize = 1024;

const LOW_HZ: (f32, f32) = (20.0, 250.0);
const MID_HZ: (f32, f32) = (250.0, 2_000.0);
const HIGH_HZ: (f32, f32) = (2_000.0, 20_000.0);

/// Per-call decay of the tracked peak: lower means the gain adapts faster.
const PEAK_DECAY: f32 = 0.985;
const SILENCE: f32 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AudioLevels {
    /// Overall loudness (RMS), 0..1.
    pub level: f32,
    pub low: f32,
    pub mid: f32,
    pub high: f32,
}

/// Magnitude spectrum (N/2 bins) of a real signal whose length is a power of two, via an
/// iterative radix-2 FFT. No window is applied here.
pub fn magnitudes(samples: &[f32]) -> Vec<f32> {
    let n = samples.len().next_power_of_two().min(samples.len().max(1));
    let n = if n.is_power_of_two() { n } else { 1 << (usize::BITS - 1 - n.leading_zeros()) };
    let mut re: Vec<f32> = samples[..n].to_vec();
    let mut im: Vec<f32> = vec![0.0; n];
    // Bit-reversal permutation.
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = (i.reverse_bits() >> (usize::BITS - bits)) & (n - 1);
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len *= 2;
    }
    (0..n / 2).map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt()).collect()
}

/// Stateful analyser: remembers the recent peak for auto-gain and smooths the output.
#[derive(Debug, Clone)]
pub struct Analyzer {
    sample_rate: f32,
    peak_band: f32,
    peak_level: f32,
    last: AudioLevels,
}

impl Analyzer {
    pub fn new(sample_rate: f32) -> Self {
        Analyzer { sample_rate, peak_band: 0.0, peak_level: 0.0, last: AudioLevels::default() }
    }

    /// Analyses one block (ideally `FFT_SIZE` samples, mono, -1..1).
    pub fn analyze(&mut self, samples: &[f32]) -> AudioLevels {
        if samples.is_empty() {
            return self.last;
        }
        let n = samples.len().min(FFT_SIZE);
        let rms = (samples[..n].iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
        // Hann window before the FFT.
        let windowed: Vec<f32> = samples[..n]
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
                s * w
            })
            .collect();
        let mags = magnitudes(&windowed);
        let bin_hz = self.sample_rate / mags.len().max(1) as f32 / 2.0;
        let band = |lo: f32, hi: f32| -> f32 {
            let a = ((lo / bin_hz) as usize).min(mags.len());
            let b = ((hi / bin_hz).ceil() as usize).clamp(a, mags.len());
            if b <= a {
                return 0.0;
            }
            (mags[a..b].iter().map(|m| m * m).sum::<f32>() / (b - a) as f32).sqrt()
        };
        let raw = [band(LOW_HZ.0, LOW_HZ.1), band(MID_HZ.0, MID_HZ.1), band(HIGH_HZ.0, HIGH_HZ.1)];
        let max_band = raw.iter().cloned().fold(0.0f32, f32::max);
        self.peak_band = (self.peak_band * PEAK_DECAY).max(max_band);
        self.peak_level = (self.peak_level * PEAK_DECAY).max(rms);
        let norm = |v: f32, peak: f32| if peak < SILENCE { 0.0 } else { (v / peak).clamp(0.0, 1.0) };
        let levels = AudioLevels {
            level: norm(rms, self.peak_level),
            low: norm(raw[0], self.peak_band),
            mid: norm(raw[1], self.peak_band),
            high: norm(raw[2], self.peak_band),
        };
        self.last = levels;
        levels
    }

    pub fn last(&self) -> AudioLevels {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 / rate * freq * std::f32::consts::TAU).sin() * 0.5).collect()
    }

    #[test]
    fn silence_gives_zero_levels() {
        let mut an = Analyzer::new(48_000.0);
        let l = an.analyze(&vec![0.0; FFT_SIZE]);
        assert_eq!(l, AudioLevels::default());
    }

    #[test]
    fn a_bass_tone_lands_in_the_low_band() {
        let mut an = Analyzer::new(48_000.0);
        let l = an.analyze(&sine(100.0, 48_000.0, FFT_SIZE));
        assert!(l.low > l.mid && l.low > l.high, "{l:?}");
        assert!(l.level > 0.1, "{l:?}");
    }

    #[test]
    fn mid_and_high_tones_land_in_their_bands() {
        let mut an = Analyzer::new(48_000.0);
        let m = an.analyze(&sine(2_000.0, 48_000.0, FFT_SIZE));
        assert!(m.mid > m.low && m.mid > m.high, "{m:?}");
        let mut an = Analyzer::new(48_000.0);
        let h = an.analyze(&sine(9_000.0, 48_000.0, FFT_SIZE));
        assert!(h.high > h.low && h.high > h.mid, "{h:?}");
    }

    #[test]
    fn levels_are_normalised_into_unit_range_with_auto_gain() {
        let mut an = Analyzer::new(48_000.0);
        let loud: Vec<f32> = sine(100.0, 48_000.0, FFT_SIZE).iter().map(|s| s * 8.0).collect();
        let l = an.analyze(&loud);
        for v in [l.level, l.low, l.mid, l.high] {
            assert!((0.0..=1.0).contains(&v), "{l:?}");
        }
        // A quiet signal after a loud one reads lower (the gain tracks the recent peak).
        let q = an.analyze(&sine(100.0, 48_000.0, FFT_SIZE));
        assert!(q.low < l.low, "{q:?} vs {l:?}");
    }

    #[test]
    fn fft_matches_a_known_spectrum() {
        // A pure tone at bin 8 of a 64-point FFT.
        let n = 64;
        let samples: Vec<f32> = (0..n).map(|i| (i as f32 / n as f32 * 8.0 * std::f32::consts::TAU).cos()).collect();
        let mags = magnitudes(&samples);
        let peak = mags.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(peak, 8);
        assert!(mags[8] > 10.0 * mags[9]);
    }
}
