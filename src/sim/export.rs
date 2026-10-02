//! PNG export of the rendered grid: pure helpers for the readback and encoding steps.
//! The GPU side lives on `Simulation` (`start_export` / `poll_export`).

use eframe::wgpu;

use crate::preset::slug;

/// A finished export, ready to be written or downloaded.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportedImage {
    pub filename: String,
    pub png: Vec<u8>,
}

/// wgpu requires `bytes_per_row` of a texture-to-buffer copy to be a multiple of 256.
pub fn padded_bytes_per_row(bytes_per_row: u32) -> u32 {
    bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// Removes the per-row padding from a mapped readback buffer.
pub fn unpad_rows(padded: &[u8], padded_bpr: usize, bpr: usize, rows: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bpr * rows);
    for r in 0..rows {
        let start = r * padded_bpr;
        out.extend_from_slice(&padded[start..start + bpr]);
    }
    out
}

/// Converts 8-bit pixels from the surface format to RGBA in place (BGRA surfaces get swizzled).
pub fn to_rgba(pixels: &mut [u8], format: wgpu::TextureFormat) {
    if matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
    }
}

pub fn image_filename(preset_name: &str, step: u32) -> String {
    format!("{}-step{step}.png", slug(preset_name))
}

/// Largest scale `<= requested` (at least 1) such that the image fits the device's texture limit.
pub fn clamp_scale(requested: u32, width: u32, height: u32, max_dim: u32) -> u32 {
    let mut s = requested.max(1);
    while s > 1 && (width.saturating_mul(s) > max_dim || height.saturating_mul(s) > max_dim) {
        s -= 1;
    }
    s
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(rgba.len() == (width * height * 4) as usize, "pixel buffer size mismatch");
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::wgpu;

    #[test]
    fn unpad_rows_strips_the_alignment_padding() {
        // 2 rows of 3 bytes each, padded to 8 bytes per row.
        let padded = vec![1, 2, 3, 0, 0, 0, 0, 0, 4, 5, 6, 9, 9, 9, 9, 9];
        assert_eq!(unpad_rows(&padded, 8, 3, 2), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn padded_bytes_per_row_rounds_up_to_256() {
        assert_eq!(padded_bytes_per_row(16 * 4), 256);
        assert_eq!(padded_bytes_per_row(100 * 4), 512);
        assert_eq!(padded_bytes_per_row(64 * 4), 256);
    }

    #[test]
    fn bgra_surfaces_are_swizzled_to_rgba() {
        let mut px = vec![10, 20, 30, 255];
        to_rgba(&mut px, wgpu::TextureFormat::Bgra8UnormSrgb);
        assert_eq!(px, vec![30, 20, 10, 255]);
        let mut px = vec![10, 20, 30, 255];
        to_rgba(&mut px, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(px, vec![10, 20, 30, 255]);
    }

    #[test]
    fn image_filename_uses_slug_and_step() {
        assert_eq!(image_filename("Game of Life", 1234), "game_of_life-step1234.png");
    }

    #[test]
    fn scale_is_clamped_to_the_texture_limit() {
        assert_eq!(clamp_scale(4, 512, 512, 8192), 4);
        assert_eq!(clamp_scale(4, 4096, 4096, 8192), 2);
        assert_eq!(clamp_scale(4, 8192, 8192, 8192), 1);
        assert_eq!(clamp_scale(0, 16, 16, 8192), 1);
    }

    #[test]
    fn png_round_trips_pixels() {
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| (i * 7 % 256) as u8).collect();
        let bytes = encode_png(2, 3, &rgba).unwrap();
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(&buf[..info.buffer_size()], &rgba[..]);
    }
}
