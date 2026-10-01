//! Pure index math for 1D (space-time) stepping and grid-size clamping.

use eframe::wgpu;

pub const WORKGROUP: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowPlan {
    /// Scroll the texture up one row before writing (grid is full).
    pub scroll: bool,
    /// Row of `src` the rule reads as the previous generation.
    pub read_row: u32,
    /// Row of `dst` the rule writes.
    pub write_row: u32,
    /// Value of `row` for the next step.
    pub next_row: u32,
}

/// Decide which row a 1D step reads and writes. `row` is the row we want to write next.
pub fn plan_row(row: u32, height: u32) -> RowPlan {
    if height <= 1 {
        return RowPlan { scroll: false, read_row: 0, write_row: 0, next_row: 1 };
    }
    let last = height - 1;
    if row < height {
        RowPlan { scroll: false, read_row: row.saturating_sub(1), write_row: row, next_row: row + 1 }
    } else {
        RowPlan { scroll: true, read_row: last, write_row: last, next_row: height }
    }
}

/// Clamps a requested grid dimension to what the device can allocate and dispatch.
pub fn clamp_size(requested: u32, limits: &wgpu::Limits) -> u32 {
    let by_dispatch = limits.max_compute_workgroups_per_dimension.saturating_mul(WORKGROUP);
    requested.max(1).min(limits.max_texture_dimension_2d).min(by_dispatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_before_bottom_write_next_row_without_scroll() {
        let p = plan_row(1, 8);
        assert!(!p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (0, 1, 2));
        let p = plan_row(7, 8);
        assert!(!p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (6, 7, 8));
    }

    #[test]
    fn at_bottom_scrolls_and_keeps_writing_last_row() {
        let p = plan_row(8, 8);
        assert!(p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (7, 7, 8));
        let p = plan_row(50, 8);
        assert!(p.scroll);
        assert_eq!((p.read_row, p.write_row, p.next_row), (7, 7, 8));
    }

    #[test]
    fn height_one_never_scrolls_nor_reads_negative() {
        let p = plan_row(1, 1);
        assert!(!p.scroll, "a 1-row grid has nothing to scroll");
        assert_eq!((p.read_row, p.write_row, p.next_row), (0, 0, 1));
    }

    #[test]
    fn clamp_size_respects_texture_and_dispatch_limits() {
        let mut limits = wgpu::Limits::default();
        limits.max_texture_dimension_2d = 2048;
        limits.max_compute_workgroups_per_dimension = 100;
        assert_eq!(clamp_size(4096, &limits), 1600);
        limits.max_compute_workgroups_per_dimension = 65535;
        assert_eq!(clamp_size(4096, &limits), 2048);
        assert_eq!(clamp_size(0, &limits), 1);
        assert_eq!(clamp_size(300, &limits), 300);
    }
}
