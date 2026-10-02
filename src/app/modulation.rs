//! Time-driven parameter modulation (LFOs): a wave shape, frequency, amount and phase that
//! move a numeric param around the value the slider holds.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::audio::analysis::AudioLevels;
use crate::shader::params::{ParamSpec, ParamValue};

/// A modulation source: a periodic wave of time, or a live audio level.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Wave {
    Sine,
    Triangle,
    Square,
    Saw,
    Noise,
    AudioLevel,
    AudioLow,
    AudioMid,
    AudioHigh,
}

impl Wave {
    /// The time-based waves.
    pub const ALL: [Wave; 5] = [Wave::Sine, Wave::Triangle, Wave::Square, Wave::Saw, Wave::Noise];
    /// The audio sources (unipolar: 0 in silence, 1 at the recent peak).
    pub const AUDIO: [Wave; 4] = [Wave::AudioLevel, Wave::AudioLow, Wave::AudioMid, Wave::AudioHigh];

    pub fn is_audio(self) -> bool {
        matches!(self, Wave::AudioLevel | Wave::AudioLow | Wave::AudioMid | Wave::AudioHigh)
    }

    pub fn label(self) -> &'static str {
        match self {
            Wave::Sine => "sine",
            Wave::Triangle => "triangle",
            Wave::Square => "square",
            Wave::Saw => "saw",
            Wave::Noise => "noise",
            Wave::AudioLevel => "level",
            Wave::AudioLow => "low",
            Wave::AudioMid => "mid",
            Wave::AudioHigh => "high",
        }
    }

    fn audio_value(self, levels: &AudioLevels) -> f32 {
        match self {
            Wave::AudioLevel => levels.level,
            Wave::AudioLow => levels.low,
            Wave::AudioMid => levels.mid,
            Wave::AudioHigh => levels.high,
            _ => 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Modulation {
    pub wave: Wave,
    /// Cycles per second.
    pub freq: f32,
    /// Peak deviation as a fraction of the param's range (or of the base magnitude without one).
    pub amount: f32,
    /// Phase offset in cycles (0..1).
    pub phase: f32,
    /// Use simulation time (pauses with the simulation) instead of wall-clock time.
    #[serde(default)]
    pub follow_sim: bool,
}

impl Default for Modulation {
    fn default() -> Self {
        Modulation { wave: Wave::Sine, freq: 0.25, amount: 0.3, phase: 0.0, follow_sim: false }
    }
}

/// Wave value in `-1..=1` at `t` cycles.
pub fn wave_value(wave: Wave, t: f32) -> f32 {
    let x = t.rem_euclid(1.0);
    match wave {
        Wave::Sine => (x * std::f32::consts::TAU).sin(),
        Wave::Triangle => {
            if x < 0.25 {
                x * 4.0
            } else if x < 0.75 {
                2.0 - x * 4.0
            } else {
                x * 4.0 - 4.0
            }
        }
        Wave::Square => {
            if x < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Wave::Saw => x * 2.0 - 1.0,
        Wave::Noise => smooth_noise(t),
        // Audio sources have no time dependence; see `modulate_with`.
        Wave::AudioLevel | Wave::AudioLow | Wave::AudioMid | Wave::AudioHigh => 0.0,
    }
}

fn hash01(i: i64) -> f32 {
    let mut v = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    v ^= v >> 31;
    v = v.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    v ^= v >> 29;
    (v >> 40) as f32 / (1u64 << 24) as f32
}

/// Value noise with cubic interpolation: one random value per cycle, smoothly blended.
fn smooth_noise(t: f32) -> f32 {
    let i = t.floor() as i64;
    let f = t - t.floor();
    let a = hash01(i) * 2.0 - 1.0;
    let b = hash01(i + 1) * 2.0 - 1.0;
    let s = f * f * (3.0 - 2.0 * f);
    a + (b - a) * s
}

fn time_for(m: &Modulation, wall: f32, sim: f32) -> f32 {
    if m.follow_sim { sim } else { wall }
}

/// Applies `m` to a numeric value at `time` seconds. Vectors and bools pass through unchanged.
pub fn modulate(base: &ParamValue, spec: &ParamSpec, m: &Modulation, time: f32) -> ParamValue {
    modulate_with(base, spec, m, time, &AudioLevels::default())
}

/// `modulate`, with live audio levels for the audio sources. Waves are bipolar (-1..1), audio
/// sources unipolar (0..1), so a silent input leaves the slider's centre value untouched.
pub fn modulate_with(
    base: &ParamValue,
    spec: &ParamSpec,
    m: &Modulation,
    time: f32,
    levels: &AudioLevels,
) -> ParamValue {
    let w = if m.wave.is_audio() { m.wave.audio_value(levels) } else { wave_value(m.wave, time * m.freq + m.phase) };
    match base {
        ParamValue::F32(v) => {
            let span = spec.range.map(|(lo, hi)| (hi - lo) as f32).unwrap_or(v.abs().max(1.0));
            let mut out = v + m.amount * span * w;
            if let Some((lo, hi)) = spec.range {
                out = out.clamp(lo as f32, hi as f32);
            }
            ParamValue::F32(out)
        }
        ParamValue::I32(v) => {
            let span = spec.range.map(|(lo, hi)| (hi - lo) as f32).unwrap_or((*v as f32).abs().max(1.0));
            let mut out = (*v as f32 + m.amount * span * w).round() as i32;
            if let Some((lo, hi)) = spec.range {
                out = out.clamp(lo as i32, hi as i32);
            }
            ParamValue::I32(out)
        }
        other => other.clone(),
    }
}

/// Values to upload this frame: the slider values with active modulations applied.
pub fn modulated_values(
    specs: &[ParamSpec],
    values: &BTreeMap<String, ParamValue>,
    modulations: &BTreeMap<String, Modulation>,
    wall_time: f32,
    sim_time: f32,
) -> BTreeMap<String, ParamValue> {
    modulated_values_with_audio(specs, values, modulations, wall_time, sim_time, &AudioLevels::default())
}

pub fn modulated_values_with_audio(
    specs: &[ParamSpec],
    values: &BTreeMap<String, ParamValue>,
    modulations: &BTreeMap<String, Modulation>,
    wall_time: f32,
    sim_time: f32,
    levels: &AudioLevels,
) -> BTreeMap<String, ParamValue> {
    let mut out = values.clone();
    for spec in specs {
        if let (Some(m), Some(base)) = (modulations.get(&spec.name), values.get(&spec.name)) {
            out.insert(spec.name.clone(), modulate_with(base, spec, m, time_for(m, wall_time, sim_time), levels));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::params::{parse_params, ParamValue};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn waves_have_the_expected_shape_over_one_cycle() {
        assert!(close(wave_value(Wave::Sine, 0.0), 0.0));
        assert!(close(wave_value(Wave::Sine, 0.25), 1.0));
        assert!(close(wave_value(Wave::Sine, 0.75), -1.0));
        assert!(close(wave_value(Wave::Triangle, 0.0), 0.0));
        assert!(close(wave_value(Wave::Triangle, 0.25), 1.0));
        assert!(close(wave_value(Wave::Triangle, 0.75), -1.0));
        assert!(close(wave_value(Wave::Square, 0.1), 1.0));
        assert!(close(wave_value(Wave::Square, 0.6), -1.0));
        assert!(close(wave_value(Wave::Saw, 0.0), -1.0));
        assert!(close(wave_value(Wave::Saw, 0.5), 0.0));
        assert!(wave_value(Wave::Saw, 0.999) > 0.99);
        // Waves repeat every cycle.
        assert!(close(wave_value(Wave::Sine, 3.25), 1.0));
    }

    #[test]
    fn noise_is_deterministic_bounded_and_smooth() {
        let a = wave_value(Wave::Noise, 1.3);
        let b = wave_value(Wave::Noise, 1.3);
        assert_eq!(a, b);
        for i in 0..200 {
            let t = i as f32 * 0.05;
            let v = wave_value(Wave::Noise, t);
            assert!((-1.0..=1.0).contains(&v), "{v} at {t}");
            let v2 = wave_value(Wave::Noise, t + 0.001);
            assert!((v - v2).abs() < 0.05, "noise should be smooth: {v} vs {v2}");
        }
    }

    #[test]
    fn modulate_scales_by_the_range_and_clamps() {
        let specs = parse_params("// @param a: f32 = 0.5 range 0.0 .. 1.0\n").unwrap();
        let m = Modulation { wave: Wave::Sine, freq: 1.0, amount: 0.5, phase: 0.0, follow_sim: false };
        // t = 0.25 s at 1 Hz: sine peak -> +0.5 * range(1.0) = 0.5 above the centre, clamped to 1.0.
        assert_eq!(modulate(&ParamValue::F32(0.8), &specs[0], &m, 0.25), ParamValue::F32(1.0));
        assert_eq!(modulate(&ParamValue::F32(0.5), &specs[0], &m, 0.25), ParamValue::F32(1.0));
        assert_eq!(modulate(&ParamValue::F32(0.5), &specs[0], &m, 0.75), ParamValue::F32(0.0));
        assert_eq!(modulate(&ParamValue::F32(0.5), &specs[0], &m, 0.0), ParamValue::F32(0.5));
    }

    #[test]
    fn modulate_rounds_integers_and_leaves_vectors_alone() {
        let specs = parse_params("// @param n: i32 = 4 range 0 .. 8\n// @param c: vec3<f32> = (1, 1, 1)\n").unwrap();
        let m = Modulation { wave: Wave::Sine, freq: 1.0, amount: 0.5, phase: 0.0, follow_sim: false };
        assert_eq!(modulate(&ParamValue::I32(4), &specs[0], &m, 0.25), ParamValue::I32(8));
        assert_eq!(modulate(&ParamValue::I32(4), &specs[0], &m, 0.75), ParamValue::I32(0));
        let c = ParamValue::Vec3([1.0, 1.0, 1.0]);
        assert_eq!(modulate(&c, &specs[1], &m, 0.25), c);
    }

    #[test]
    fn params_without_a_range_use_the_base_magnitude() {
        let specs = parse_params("// @param k: f32 = 10.0\n").unwrap();
        let m = Modulation { wave: Wave::Sine, freq: 1.0, amount: 0.5, phase: 0.0, follow_sim: false };
        assert_eq!(modulate(&ParamValue::F32(10.0), &specs[0], &m, 0.25), ParamValue::F32(15.0));
    }

    #[test]
    fn phase_shifts_the_wave() {
        let specs = parse_params("// @param a: f32 = 0.5 range 0.0 .. 1.0\n").unwrap();
        let m = Modulation { wave: Wave::Sine, freq: 1.0, amount: 0.5, phase: 0.25, follow_sim: false };
        assert_eq!(modulate(&ParamValue::F32(0.5), &specs[0], &m, 0.0), ParamValue::F32(1.0));
    }

    #[test]
    fn modulation_round_trips_through_toml() {
        let m = Modulation { wave: Wave::Triangle, freq: 0.5, amount: 0.3, phase: 0.1, follow_sim: true };
        let text = toml::to_string(&m).unwrap();
        assert!(text.contains("wave = \"triangle\""), "{text}");
        let back: Modulation = toml::from_str(&text).unwrap();
        assert_eq!(back, m);
        // follow_sim is optional in files.
        let back: Modulation = toml::from_str("wave = \"sine\"\nfreq = 1.0\namount = 0.2\nphase = 0.0\n").unwrap();
        assert!(!back.follow_sim);
    }

    #[test]
    fn modulated_values_only_touch_modulated_params() {
        let specs = parse_params("// @param a: f32 = 0.5 range 0.0 .. 1.0\n// @param b: f32 = 0.2 range 0.0 .. 1.0\n").unwrap();
        let values: BTreeMap<_, _> =
            [("a".to_string(), ParamValue::F32(0.5)), ("b".to_string(), ParamValue::F32(0.2))].into();
        let mut mods = BTreeMap::new();
        mods.insert("a".to_string(), Modulation { wave: Wave::Square, freq: 1.0, amount: 0.2, phase: 0.0, follow_sim: true });
        // Wall time would give the negative half; sim time (0.1 s) gives the positive half.
        let out = modulated_values(&specs, &values, &mods, 0.6, 0.1);
        assert_eq!(out["a"], ParamValue::F32(0.7));
        assert_eq!(out["b"], ParamValue::F32(0.2));
    }

    #[test]
    fn audio_sources_are_unipolar_and_use_the_live_levels() {
        use crate::audio::analysis::AudioLevels;
        let specs = parse_params("// @param a: f32 = 0.2 range 0.0 .. 1.0\n").unwrap();
        let values: BTreeMap<_, _> = [("a".to_string(), ParamValue::F32(0.2))].into();
        let mut mods = BTreeMap::new();
        mods.insert("a".to_string(), Modulation { wave: Wave::AudioLow, freq: 1.0, amount: 0.5, phase: 0.0, follow_sim: false });
        let levels = AudioLevels { level: 0.9, low: 0.5, mid: 0.1, high: 0.0 };
        let out = modulated_values_with_audio(&specs, &values, &mods, 0.0, 0.0, &levels);
        assert_eq!(out["a"], ParamValue::F32(0.45), "centre + amount * span * low");
        let silent = modulated_values_with_audio(&specs, &values, &mods, 0.0, 0.0, &AudioLevels::default());
        assert_eq!(silent["a"], ParamValue::F32(0.2), "silence leaves the centre value");
        assert!(Wave::AudioLevel.is_audio() && !Wave::Sine.is_audio());
        let text = toml::to_string(&mods["a"]).unwrap();
        assert!(text.contains("wave = \"audiolow\""), "{text}");
    }
}
