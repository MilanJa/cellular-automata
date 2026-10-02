//! Desktop microphone capture with cpal: samples are mixed to mono into a shared buffer and
//! analysed in blocks when the UI asks for levels.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::analysis::{Analyzer, AudioLevels, FFT_SIZE};

pub struct AudioInput {
    _stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    analyzer: Analyzer,
    pub device_name: String,
}

impl AudioInput {
    /// Opens the default input device. Fails when there is none or it cannot be opened.
    pub fn start() -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or_else(|| anyhow::anyhow!("no audio input device"))?;
        let device_name = device.description().map(|d| d.to_string()).unwrap_or_else(|_| "input".into());
        let config = device.default_input_config()?;
        let channels = config.channels() as usize;
        let sample_rate = config.sample_rate() as f32;
        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(FFT_SIZE * 4)));
        let sink = buffer.clone();
        let err_fn = |e| log::warn!("audio input error: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                config.into(),
                move |data: &[f32], _| push_mono(&sink, data.iter().copied(), channels),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                config.into(),
                move |data: &[i16], _| push_mono(&sink, data.iter().map(|&s| s as f32 / i16::MAX as f32), channels),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                config.into(),
                move |data: &[u16], _| push_mono(&sink, data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0), channels),
                err_fn,
                None,
            )?,
            other => anyhow::bail!("unsupported sample format {other:?}"),
        };
        stream.play()?;
        Ok(AudioInput { _stream: stream, buffer, analyzer: Analyzer::new(sample_rate), device_name })
    }

    /// Analyses the newest block of samples, if a full one has arrived since the last call.
    pub fn levels(&mut self) -> AudioLevels {
        let block: Option<Vec<f32>> = {
            let mut buf = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
            if buf.len() >= FFT_SIZE {
                let start = buf.len() - FFT_SIZE;
                let block = buf[start..].to_vec();
                buf.clear();
                Some(block)
            } else {
                None
            }
        };
        match block {
            Some(b) => self.analyzer.analyze(&b),
            None => self.analyzer.last(),
        }
    }
}

fn push_mono(sink: &Arc<Mutex<Vec<f32>>>, samples: impl Iterator<Item = f32>, channels: usize) {
    let mut buf = sink.lock().unwrap_or_else(|e| e.into_inner());
    let channels = channels.max(1);
    let mut acc = 0.0f32;
    let mut c = 0usize;
    for s in samples {
        acc += s;
        c += 1;
        if c == channels {
            buf.push(acc / channels as f32);
            acc = 0.0;
            c = 0;
        }
    }
    // Keep the buffer bounded if nobody is reading.
    let cap = FFT_SIZE * 8;
    if buf.len() > cap {
        let excess = buf.len() - cap;
        buf.drain(..excess);
    }
}
