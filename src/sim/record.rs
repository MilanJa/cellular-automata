//! Recording the viewport as an animated PNG: pure encoding and budgeting helpers.
//! Frame capture itself reuses the image export path (`Simulation::start_export`).

use crate::preset::slug;

/// Memory allowed for buffered RGBA frames while recording.
pub const FRAME_BUDGET_BYTES: u64 = 256 << 20;

/// What the Record dialog asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingSettings {
    pub frames: u32,
    /// Pixels per cell, as for image export.
    pub scale: u32,
    /// Playback rate written into the file.
    pub fps: u16,
}

impl Default for RecordingSettings {
    fn default() -> Self {
        RecordingSettings { frames: 120, scale: 2, fps: 30 }
    }
}

/// Largest number of RGBA8 frames of `width x height` that fit the budget (at least 1).
pub fn max_frames(width: u32, height: u32, budget_bytes: u64) -> u32 {
    let per = (width as u64 * height as u64 * 4).max(1);
    (budget_bytes / per).clamp(1, u32::MAX as u64) as u32
}

pub fn recording_filename(preset_name: &str) -> String {
    format!("{}-anim.png", slug(preset_name))
}

/// Encodes RGBA8 frames of equal size into a looping animated PNG at `fps` frames per second.
pub fn encode_apng(width: u32, height: u32, frames: &[Vec<u8>], fps: u16) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(!frames.is_empty(), "no frames to encode");
    let expected = (width * height * 4) as usize;
    anyhow::ensure!(frames.iter().all(|f| f.len() == expected), "frame size mismatch");
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(frames.len() as u32, 0)?;
        encoder.set_frame_delay(1, fps.max(1))?;
        let mut writer = encoder.write_header()?;
        for frame in frames {
            writer.write_image_data(frame)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apng_has_the_requested_frames_and_decodes() {
        let frames: Vec<Vec<u8>> =
            (0..3u8).map(|i| (0..2 * 2 * 4).map(|j| (i * 40 + j as u8) % 255).collect()).collect();
        let bytes = encode_apng(2, 2, &frames, 30).unwrap();
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().unwrap();
        let anim = reader.info().animation_control.expect("animated PNG");
        assert_eq!(anim.num_frames, 3);
        assert_eq!(anim.num_plays, 0, "loops forever");
    }

    #[test]
    fn apng_rejects_empty_or_mismatched_input() {
        assert!(encode_apng(2, 2, &[], 30).is_err());
        assert!(encode_apng(2, 2, &[vec![0; 3]], 30).is_err());
    }

    #[test]
    fn frame_budget_caps_long_recordings() {
        // 512x512 RGBA8 = 1 MiB per frame; a 256 MiB budget allows 256 frames.
        assert_eq!(max_frames(512, 512, 256 << 20), 256);
        assert_eq!(max_frames(16, 16, 256 << 20), 262144);
        assert!(max_frames(8192, 8192, 256 << 20) >= 1);
    }

    #[test]
    fn recording_filename_uses_slug() {
        assert_eq!(recording_filename("Neon Life"), "neon_life-anim.png");
    }
}
