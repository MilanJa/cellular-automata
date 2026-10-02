//! Rule explorer: random Life-like rules (B/S notation) and the rule shader generated for one.
//! The GPU thumbnails live in `App`; this is the pure part.

use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// A Life-like rule as two 9-bit masks indexed by neighbour count (bit n = count n).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LifelikeRule {
    pub birth: u16,
    pub survive: u16,
}

fn digits(mask: u16) -> String {
    (0..9).filter(|n| mask & (1 << n) != 0).map(|n| char::from(b'0' + n as u8)).collect()
}

fn parse_digits(s: &str) -> Option<u16> {
    let mut mask = 0u16;
    for c in s.chars() {
        let d = c.to_digit(10)?;
        if d > 8 {
            return None;
        }
        mask |= 1 << d;
    }
    Some(mask)
}

impl LifelikeRule {
    pub const LIFE: LifelikeRule = LifelikeRule { birth: 1 << 3, survive: (1 << 2) | (1 << 3) };

    /// Golly-style notation, e.g. `B3/S23`.
    pub fn notation(&self) -> String {
        format!("B{}/S{}", digits(self.birth), digits(self.survive))
    }

    pub fn parse(text: &str) -> Option<LifelikeRule> {
        let t = text.trim().to_ascii_uppercase();
        let (b, s) = t.split_once('/')?;
        let b = b.strip_prefix('B')?;
        let s = s.strip_prefix('S')?;
        Some(LifelikeRule { birth: parse_digits(b)?, survive: parse_digits(s)? })
    }

    /// A random rule that can grow (at least one birth count) and does not strobe (no B0).
    pub fn random(seed: u64) -> LifelikeRule {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        loop {
            // Favour sparse masks: each count is included with probability ~0.3.
            let mut birth = 0u16;
            let mut survive = 0u16;
            for n in 1..9 {
                if rng.random_bool(0.3) {
                    birth |= 1 << n;
                }
            }
            for n in 0..9 {
                if rng.random_bool(0.3) {
                    survive |= 1 << n;
                }
            }
            if birth != 0 {
                return LifelikeRule { birth, survive };
            }
        }
    }

    /// A complete rule shader for this rule, with the masks baked in.
    pub fn rule_wgsl(&self) -> String {
        format!(
            "// Life-like rule {notation} (from the rule explorer).\n\
             // Birth and survival neighbour counts are encoded as bitmasks (bit n = n neighbours).\n\
             const BIRTH: u32 = {birth}u;\n\
             const SURVIVE: u32 = {survive}u;\n\
             \n\
             fn rule(pos: vec2<u32>) -> vec4<f32> {{\n\
             \x20   let x = i32(pos.x);\n\
             \x20   let y = i32(pos.y);\n\
             \x20   let n = neighbours(x, y);\n\
             \x20   let me = alive(x, y);\n\
             \x20   let mask = select(BIRTH, SURVIVE, me);\n\
             \x20   let next = ((mask >> n) & 1u) == 1u;\n\
             \x20   let age = select(0.0, cell(x, y).g + 1.0, next && me);\n\
             \x20   return vec4<f32>(f32(next), age, 0.0, 1.0);\n\
             }}\n",
            notation = self.notation(),
            birth = self.birth,
            survive = self.survive,
        )
    }
}

/// `count` distinct random rules derived from `seed`.
pub fn random_batch(seed: u64, count: usize) -> Vec<LifelikeRule> {
    let mut out: Vec<LifelikeRule> = Vec::with_capacity(count);
    let mut i = 0u64;
    while out.len() < count {
        let r = LifelikeRule::random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(i));
        i += 1;
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::assemble::assemble_rule;
    use crate::shader::params::params_wgsl;
    use crate::shader::validate::{validate, ShaderFile};

    #[test]
    fn notation_matches_golly_style() {
        assert_eq!(LifelikeRule { birth: 0b0000_1000, survive: 0b0000_1100 }.notation(), "B3/S23");
        assert_eq!(LifelikeRule { birth: 0b0100_1000, survive: 0b0000_1100 }.notation(), "B36/S23");
        assert_eq!(LifelikeRule { birth: 0b0000_0100, survive: 0 }.notation(), "B2/S");
        assert_eq!(LifelikeRule::parse("B3/S23"), Some(LifelikeRule { birth: 0b0000_1000, survive: 0b0000_1100 }));
        assert_eq!(LifelikeRule::parse("b36/s23"), LifelikeRule::parse("B36/S23"));
        assert_eq!(LifelikeRule::parse("nope"), None);
    }

    #[test]
    fn random_rules_are_deterministic_and_never_dead_on_arrival() {
        let a = LifelikeRule::random(42);
        let b = LifelikeRule::random(42);
        assert_eq!(a, b);
        for seed in 0..200u64 {
            let r = LifelikeRule::random(seed);
            assert!(r.birth != 0, "a rule with no births is boring: seed {seed}");
            assert!(r.birth & 1 == 0, "B0 rules strobe; skip them: seed {seed}");
        }
        let distinct: std::collections::HashSet<_> = (0..64u64).map(LifelikeRule::random).collect();
        assert!(distinct.len() > 40, "random rules should vary: {}", distinct.len());
    }

    #[test]
    fn generated_rule_shader_validates_and_names_the_rule() {
        let r = LifelikeRule::parse("B36/S23").unwrap();
        let src = r.rule_wgsl();
        assert!(src.contains("B36/S23"));
        let a = assemble_rule(&src, &params_wgsl(&[]));
        validate(ShaderFile::Rule, &a).unwrap_or_else(|e| panic!("{e:?}\n{}", a.source));
    }

    #[test]
    fn a_batch_has_distinct_rules() {
        let batch = random_batch(7, 16);
        assert_eq!(batch.len(), 16);
        let distinct: std::collections::HashSet<_> = batch.iter().collect();
        assert_eq!(distinct.len(), 16);
        assert_eq!(random_batch(7, 16), batch, "same seed, same batch");
    }
}
