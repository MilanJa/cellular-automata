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
}

/// One 16-byte slot per param, in declaration order.
pub type ParamsData = [[u32; 4]; MAX_PARAMS];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globals_is_32_bytes() {
        assert_eq!(std::mem::size_of::<Globals>(), 32);
    }

    #[test]
    fn params_data_is_256_bytes() {
        assert_eq!(std::mem::size_of::<ParamsData>(), 256);
    }
}
