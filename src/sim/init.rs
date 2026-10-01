//! Seeded initial grid states, generated on the CPU and uploaded to texture A on reset.

use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::preset::{InitPattern, Mode};

/// Returns `width * height * 4` floats (row-major RGBA). Alpha is always 1.
/// In 1D mode only row 0 is populated.
pub fn generate_init(pattern: &InitPattern, mode: Mode, width: u32, height: u32, seed: u32) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![0.0f32; w * h * 4];
    for px in data.chunks_mut(4) {
        px[3] = 1.0;
    }
    let rows = match mode {
        Mode::TwoD => h,
        Mode::OneD => 1,
    };
    match pattern {
        InitPattern::Blank => {}
        InitPattern::Single => {
            let x = w / 2;
            let y = match mode {
                Mode::TwoD => h / 2,
                Mode::OneD => 0,
            };
            data[(y * w + x) * 4] = 1.0;
        }
        InitPattern::Random { density } => {
            let mut rng = ChaCha8Rng::seed_from_u64(seed as u64);
            let p = (*density as f64).clamp(0.0, 1.0);
            for y in 0..rows {
                for x in 0..w {
                    if rng.random_bool(p) {
                        data[(y * w + x) * 4] = 1.0;
                    }
                }
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{InitPattern, Mode};

    #[test]
    fn blank_is_all_zero_with_alpha_one() {
        let v = generate_init(&InitPattern::Blank, Mode::TwoD, 4, 2, 0);
        assert_eq!(v.len(), 4 * 2 * 4);
        for px in v.chunks(4) {
            assert_eq!(px, &[0.0, 0.0, 0.0, 1.0]);
        }
    }

    #[test]
    fn single_2d_sets_centre_only() {
        let v = generate_init(&InitPattern::Single, Mode::TwoD, 8, 6, 0);
        let on: Vec<usize> =
            v.chunks(4).enumerate().filter(|(_, p)| p[0] > 0.5).map(|(i, _)| i).collect();
        assert_eq!(on, vec![3 * 8 + 4]);
    }

    #[test]
    fn single_1d_sets_centre_of_row_zero() {
        let v = generate_init(&InitPattern::Single, Mode::OneD, 8, 6, 0);
        let on: Vec<usize> =
            v.chunks(4).enumerate().filter(|(_, p)| p[0] > 0.5).map(|(i, _)| i).collect();
        assert_eq!(on, vec![4]);
    }

    #[test]
    fn random_is_deterministic_and_respects_density() {
        let a = generate_init(&InitPattern::Random { density: 0.25 }, Mode::TwoD, 100, 100, 9);
        let b = generate_init(&InitPattern::Random { density: 0.25 }, Mode::TwoD, 100, 100, 9);
        let c = generate_init(&InitPattern::Random { density: 0.25 }, Mode::TwoD, 100, 100, 10);
        assert_eq!(a, b);
        assert_ne!(a, c);
        let on = a.chunks(4).filter(|p| p[0] > 0.5).count();
        assert!((2000..3000).contains(&on), "on = {on}");
    }

    #[test]
    fn random_1d_only_fills_row_zero() {
        let v = generate_init(&InitPattern::Random { density: 0.5 }, Mode::OneD, 64, 4, 1);
        let on_later_rows = v[64 * 4..].chunks(4).filter(|p| p[0] > 0.5).count();
        assert_eq!(on_later_rows, 0);
        let on_row0 = v[..64 * 4].chunks(4).filter(|p| p[0] > 0.5).count();
        assert!(on_row0 > 10);
    }
}
