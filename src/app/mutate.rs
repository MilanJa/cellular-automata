//! "Mutate": nudge every param a little, deterministically per seed, staying inside ranges.

use std::collections::BTreeMap;

use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::shader::params::{ParamSpec, ParamValue};

/// Relative size of a nudge: fraction of the range, or of the magnitude without a range.
const RANGED_STEP: f32 = 0.2;
const UNRANGED_STEP: f32 = 0.3;
const COLOR_STEP: f32 = 0.15;
const BOOL_FLIP_PROBABILITY: f64 = 0.3;

fn unit(rng: &mut ChaCha8Rng) -> f32 {
    rng.random::<f32>() * 2.0 - 1.0
}

fn nudge_one(spec: &ParamSpec, value: &ParamValue, rng: &mut ChaCha8Rng) -> ParamValue {
    match value {
        ParamValue::F32(v) => match spec.range {
            Some((lo, hi)) => {
                let span = (hi - lo) as f32;
                ParamValue::F32((v + unit(rng) * RANGED_STEP * span).clamp(lo as f32, hi as f32))
            }
            None => ParamValue::F32(v * (1.0 + unit(rng) * UNRANGED_STEP)),
        },
        ParamValue::I32(v) => match spec.range {
            Some((lo, hi)) => {
                let span = (hi - lo) as f32;
                let n = (*v as f32 + unit(rng) * RANGED_STEP * span).round() as i32;
                ParamValue::I32(n.clamp(lo as i32, hi as i32))
            }
            None => ParamValue::I32((*v as f32 * (1.0 + unit(rng) * UNRANGED_STEP)).round() as i32),
        },
        ParamValue::Bool(b) => ParamValue::Bool(if rng.random_bool(BOOL_FLIP_PROBABILITY) { !b } else { *b }),
        ParamValue::Vec2(a) => ParamValue::Vec2(a.map(|x| x * (1.0 + unit(rng) * UNRANGED_STEP))),
        ParamValue::Vec3(a) => ParamValue::Vec3(if spec.color {
            a.map(|x| (x + unit(rng) * COLOR_STEP).clamp(0.0, 1.0))
        } else {
            a.map(|x| x * (1.0 + unit(rng) * UNRANGED_STEP))
        }),
        ParamValue::Vec4(a) => ParamValue::Vec4(if spec.color {
            a.map(|x| (x + unit(rng) * COLOR_STEP).clamp(0.0, 1.0))
        } else {
            a.map(|x| x * (1.0 + unit(rng) * UNRANGED_STEP))
        }),
    }
}

/// Returns nudged copies of `values` (missing entries start from the spec default). Deterministic
/// for a given `seed`; guaranteed to differ from the input when any param can move.
pub fn mutate_values(
    specs: &[ParamSpec],
    values: &BTreeMap<String, ParamValue>,
    seed: u64,
) -> BTreeMap<String, ParamValue> {
    for attempt in 0..8u64 {
        let mut rng = ChaCha8Rng::seed_from_u64(seed.wrapping_add(attempt.wrapping_mul(0x9E37_79B9)));
        let out: BTreeMap<String, ParamValue> = specs
            .iter()
            .map(|s| {
                let base = values.get(&s.name).filter(|v| v.ty() == s.ty).unwrap_or(&s.default);
                (s.name.clone(), nudge_one(s, base, &mut rng))
            })
            .collect();
        if out != *values || specs.is_empty() {
            return out;
        }
    }
    values.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::params::{parse_params, ParamValue};
    use std::collections::BTreeMap;

    fn setup() -> (Vec<crate::shader::params::ParamSpec>, BTreeMap<String, ParamValue>) {
        let specs = parse_params(
            "// @param a: f32 = 0.5 range 0.0 .. 1.0\n// @param n: i32 = 4 range 0 .. 8\n// @param k: f32 = 10.0\n// @param enabled: bool = true\n// @param col: vec3<f32> = (0.5, 0.5, 0.5) color\n// @param v: vec2<f32> = (2.0, -2.0)\n",
        )
        .unwrap();
        let values = specs.iter().map(|s| (s.name.clone(), s.default.clone())).collect();
        (specs, values)
    }

    #[test]
    fn mutation_is_deterministic_per_seed_and_changes_something() {
        let (specs, values) = setup();
        let a = mutate_values(&specs, &values, 7);
        let b = mutate_values(&specs, &values, 7);
        let c = mutate_values(&specs, &values, 8);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, values, "a mutation must change at least one value");
    }

    #[test]
    fn mutated_values_stay_in_range_and_keep_their_types() {
        let (specs, values) = setup();
        for seed in 0..200u64 {
            let m = mutate_values(&specs, &values, seed);
            match m["a"] {
                ParamValue::F32(x) => assert!((0.0..=1.0).contains(&x), "{x}"),
                ref other => panic!("{other:?}"),
            }
            match m["n"] {
                ParamValue::I32(x) => assert!((0..=8).contains(&x), "{x}"),
                ref other => panic!("{other:?}"),
            }
            match m["k"] {
                ParamValue::F32(x) => assert!((7.0..=13.0).contains(&x), "unranged values move at most 30%: {x}"),
                ref other => panic!("{other:?}"),
            }
            match m["col"] {
                ParamValue::Vec3(c) => assert!(c.iter().all(|x| (0.0..=1.0).contains(x)), "{c:?}"),
                ref other => panic!("{other:?}"),
            }
            assert!(matches!(m["enabled"], ParamValue::Bool(_)));
            assert!(matches!(m["v"], ParamValue::Vec2(_)));
        }
    }

    #[test]
    fn bools_flip_sometimes_but_not_always() {
        let (specs, values) = setup();
        let flips = (0..100u64)
            .filter(|&s| mutate_values(&specs, &values, s)["enabled"] == ParamValue::Bool(false))
            .count();
        assert!((5..95).contains(&flips), "flips = {flips}");
    }
}
