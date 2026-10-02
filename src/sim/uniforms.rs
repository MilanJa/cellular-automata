//! CPU-side layouts of the uniform buffers. Must match `GLOBALS_WGSL` and `params_wgsl`.

use bytemuck::{Pod, Zeroable};

use crate::shader::params::MAX_PARAMS;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct Globals {
    pub size: [u32; 2],
    pub frame: u32,
    pub seed: u32,
    pub time: f32,
    pub mode: u32,
    pub row: u32,
    pub prev_row: u32,
    /// Crossfade between rule A (0) and rule B (1) when a rule pair is loaded.
    pub blend: f32,
    pub _pad: [u32; 3],
}

/// One 16-byte slot per param, in declaration order.
pub type ParamsData = [[u32; 4]; MAX_PARAMS];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globals_is_48_bytes() {
        assert_eq!(std::mem::size_of::<Globals>(), 48);
    }

    #[test]
    fn params_data_is_16_bytes_per_slot() {
        assert_eq!(std::mem::size_of::<ParamsData>(), 16 * MAX_PARAMS);
    }

    #[test]
    fn globals_carry_a_blend_factor() {
        let g = Globals { blend: 0.25, ..Default::default() };
        assert_eq!(g.blend, 0.25);
        assert_eq!(std::mem::size_of::<Globals>() % 16, 0, "uniform structs are 16-byte aligned");
    }
}
