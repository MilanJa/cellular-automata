//! Per-frame statistics: live-cell population and cells changed by the last step, reduced on
//! the GPU, plus a CPU-side detector for grids that have gone static or periodic.

/// One measurement, taken after the last step of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsSample {
    pub step: u32,
    pub population: u32,
    /// Cells whose on/off state differs between the last two generations.
    pub changed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stuck {
    /// Nothing changed for the whole window (includes an empty grid).
    Static,
    /// Population and change counts repeat with this period (1 = constant, e.g. a translating pattern).
    Periodic(u32),
}

/// Largest period the detector looks for.
pub const MAX_PERIOD: usize = 8;

/// Looks at the last `window` samples. Returns `None` while there are fewer than that or while
/// the grid keeps producing new numbers.
pub fn detect_stuck(history: &[StatsSample], window: usize) -> Option<Stuck> {
    if history.len() < window || window == 0 {
        return None;
    }
    let w = &history[history.len() - window..];
    if w.iter().all(|s| s.changed == 0) {
        return Some(Stuck::Static);
    }
    (1..=MAX_PERIOD.min(window / 2)).find_map(|p| {
        let repeats = (p..window).all(|i| w[i].population == w[i - p].population && w[i].changed == w[i - p].changed);
        repeats.then_some(Stuck::Periodic(p as u32))
    })
}

/// Reduces population and change counts into a 2 x u32 storage buffer, one atomic add per workgroup.
pub const STATS_WGSL: &str = r#"@group(0) @binding(0) var cur_tex: texture_2d<f32>;
@group(0) @binding(1) var prev_tex: texture_2d<f32>;
struct Stats { population: atomic<u32>, changed: atomic<u32> }
@group(0) @binding(2) var<storage, read_write> stats: Stats;

var<workgroup> wg_pop: atomic<u32>;
var<workgroup> wg_chg: atomic<u32>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    if (li == 0u) {
        atomicStore(&wg_pop, 0u);
        atomicStore(&wg_chg, 0u);
    }
    workgroupBarrier();
    let dims = textureDimensions(cur_tex);
    if (gid.x < dims.x && gid.y < dims.y) {
        let a = textureLoad(cur_tex, gid.xy, 0).r > 0.5;
        let b = textureLoad(prev_tex, gid.xy, 0).r > 0.5;
        if (a) { atomicAdd(&wg_pop, 1u); }
        if (a != b) { atomicAdd(&wg_chg, 1u); }
    }
    workgroupBarrier();
    if (li == 0u) {
        atomicAdd(&stats.population, atomicLoad(&wg_pop));
        atomicAdd(&stats.changed, atomicLoad(&wg_chg));
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(pop: &[u32], changed: &[u32]) -> Vec<StatsSample> {
        pop.iter()
            .zip(changed)
            .enumerate()
            .map(|(i, (&p, &c))| StatsSample { step: i as u32, population: p, changed: c })
            .collect()
    }

    #[test]
    fn a_static_grid_is_reported_as_static() {
        let s = samples(&[40; 40], &[0; 40]);
        assert_eq!(detect_stuck(&s, 32), Some(Stuck::Static));
    }

    #[test]
    fn a_short_cycle_is_reported_with_its_period() {
        let pop: Vec<u32> = (0..64).map(|i| [10, 12, 10, 14][i % 4]).collect();
        let changed: Vec<u32> = vec![6; 64];
        assert_eq!(detect_stuck(&samples(&pop, &changed), 32), Some(Stuck::Periodic(4)));
        // A blinker: population and change count are constant, so the smallest period is 1.
        let pop: Vec<u32> = vec![3; 64];
        let changed: Vec<u32> = vec![6; 64];
        assert_eq!(detect_stuck(&samples(&pop, &changed), 32), Some(Stuck::Periodic(1)));
    }

    #[test]
    fn a_changing_grid_is_not_stuck_and_short_histories_abstain() {
        let pop: Vec<u32> = (0..64).map(|i| 100 + (i * 7919 % 53) as u32).collect();
        let changed: Vec<u32> = (0..64).map(|i| 1 + (i * 31 % 17) as u32).collect();
        assert_eq!(detect_stuck(&samples(&pop, &changed), 32), None);
        assert_eq!(detect_stuck(&samples(&[5; 10], &[0; 10]), 32), None, "needs a full window");
    }

    #[test]
    fn an_empty_grid_counts_as_static() {
        let s = samples(&[0; 40], &[0; 40]);
        assert_eq!(detect_stuck(&s, 32), Some(Stuck::Static));
    }
}
