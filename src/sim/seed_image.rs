//! Seeding the grid from an image: PNG decoding and nearest-neighbour resampling into cells.

/// 8-bit RGBA pixels, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// How image pixels become cell values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeedMode {
    /// Pixels brighter than `threshold` (0..1) become `on()`, the rest `off()`.
    Luminance { threshold: f32 },
    /// R, G, B, A map straight onto the four cell channels as 0..1 floats.
    Channels,
}

impl Default for SeedMode {
    fn default() -> Self {
        SeedMode::Luminance { threshold: 0.5 }
    }
}

pub fn decode_png(bytes: &[u8]) -> anyhow::Result<RgbaImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or_else(|| anyhow::anyhow!("image too large"))?];
    let info = reader.next_frame(&mut buf)?;
    let data = &buf[..info.buffer_size()];
    let (width, height) = (info.width, info.height);
    let n = (width * height) as usize;
    let mut rgba = Vec::with_capacity(n * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(data),
        png::ColorType::Rgb => {
            for px in data.as_chunks::<3>().0 {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in data {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in data.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
        }
        other => anyhow::bail!("unsupported PNG colour type {other:?}"),
    }
    anyhow::ensure!(rgba.len() == n * 4, "decoded size mismatch");
    Ok(RgbaImage { width, height, rgba })
}

/// Resamples `img` onto a `grid_w x grid_h` grid (nearest neighbour) and returns the cell
/// values as `grid_w * grid_h * 4` floats.
pub fn image_to_cells(img: &RgbaImage, grid_w: u32, grid_h: u32, mode: SeedMode) -> Vec<f32> {
    let mut out = Vec::with_capacity((grid_w * grid_h * 4) as usize);
    for gy in 0..grid_h {
        let sy = ((gy as u64 * img.height as u64) / grid_h.max(1) as u64) as usize;
        for gx in 0..grid_w {
            let sx = ((gx as u64 * img.width as u64) / grid_w.max(1) as u64) as usize;
            let i = (sy.min(img.height as usize - 1) * img.width as usize + sx.min(img.width as usize - 1)) * 4;
            let px = &img.rgba[i..i + 4];
            match mode {
                SeedMode::Luminance { threshold } => {
                    let lum = (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0;
                    let on = if lum > threshold { 1.0 } else { 0.0 };
                    out.extend_from_slice(&[on, 0.0, 0.0, 1.0]);
                }
                SeedMode::Channels => {
                    out.extend(px.iter().map(|&c| c as f32 / 255.0));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::export::encode_png;

    #[test]
    fn png_bytes_decode_to_rgba8() {
        let rgba: Vec<u8> = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 40];
        let png = encode_png(2, 2, &rgba).unwrap();
        let img = decode_png(&png).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.rgba, rgba);
        assert!(decode_png(b"not a png").is_err());
    }

    #[test]
    fn luminance_mode_thresholds_into_the_red_channel() {
        // 2x1 image: white, black -> 2x1 grid.
        let img = RgbaImage { width: 2, height: 1, rgba: vec![255, 255, 255, 255, 0, 0, 0, 255] };
        let cells = image_to_cells(&img, 2, 1, SeedMode::Luminance { threshold: 0.5 });
        assert_eq!(cells, vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        // Mid grey is below a high threshold and above a low one.
        let grey = RgbaImage { width: 1, height: 1, rgba: vec![128, 128, 128, 255] };
        assert_eq!(image_to_cells(&grey, 1, 1, SeedMode::Luminance { threshold: 0.9 })[0], 0.0);
        assert_eq!(image_to_cells(&grey, 1, 1, SeedMode::Luminance { threshold: 0.1 })[0], 1.0);
    }

    #[test]
    fn channels_mode_copies_rgba_as_floats() {
        let img = RgbaImage { width: 1, height: 1, rgba: vec![255, 0, 51, 102] };
        let cells = image_to_cells(&img, 1, 1, SeedMode::Channels);
        assert!((cells[0] - 1.0).abs() < 1e-6);
        assert_eq!(cells[1], 0.0);
        assert!((cells[2] - 0.2).abs() < 1e-6);
        assert!((cells[3] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn resampling_is_nearest_neighbour_and_fills_the_grid() {
        // 2x2 checkerboard -> 4x4 grid: each source pixel covers a 2x2 block.
        let img = RgbaImage {
            width: 2,
            height: 2,
            rgba: vec![255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255],
        };
        let cells = image_to_cells(&img, 4, 4, SeedMode::Luminance { threshold: 0.5 });
        assert_eq!(cells.len(), 4 * 4 * 4);
        let r = |x: usize, y: usize| cells[(y * 4 + x) * 4];
        assert_eq!(r(0, 0), 1.0);
        assert_eq!(r(1, 1), 1.0);
        assert_eq!(r(2, 0), 0.0);
        assert_eq!(r(3, 3), 1.0);
        assert_eq!(r(0, 3), 0.0);
        // Downsampling a 4x4 into 2x2 also works and keeps the size.
        let small = image_to_cells(&img, 1, 1, SeedMode::Channels);
        assert_eq!(small.len(), 4);
    }
}
