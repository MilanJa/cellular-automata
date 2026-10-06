//! Browser microphone capture with the Web Audio API: an `AnalyserNode` supplies time-domain
//! samples which the shared analyser turns into levels.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::analysis::{Analyzer, AudioLevels, FFT_SIZE};

struct Graph {
    _context: web_sys::AudioContext,
    _source: web_sys::MediaStreamAudioSourceNode,
    analyser: web_sys::AnalyserNode,
}

pub struct AudioInput {
    graph: Rc<RefCell<Option<Graph>>>,
    error: Rc<RefCell<Option<String>>>,
    analyzer: Analyzer,
    scratch: Vec<f32>,
    pub device_name: String,
}

impl AudioInput {
    /// Asks for microphone permission (must be called from a user gesture) and builds the graph
    /// asynchronously; `levels()` returns zeros until it is ready.
    pub fn start() -> anyhow::Result<Self> {
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
        let devices = window.navigator().media_devices().map_err(|_| anyhow::anyhow!("no media devices"))?;
        let constraints = web_sys::MediaStreamConstraints::new();
        constraints.set_audio(&JsValue::TRUE);
        let promise = devices
            .get_user_media_with_constraints(&constraints)
            .map_err(|_| anyhow::anyhow!("getUserMedia unavailable"))?;
        let graph: Rc<RefCell<Option<Graph>>> = Rc::new(RefCell::new(None));
        let error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let (g, e) = (graph.clone(), error.clone());
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(stream) => {
                    let result = (|| -> Result<Graph, JsValue> {
                        let stream: web_sys::MediaStream = stream.dyn_into()?;
                        let context = web_sys::AudioContext::new()?;
                        let source = context.create_media_stream_source(&stream)?;
                        let analyser = context.create_analyser()?;
                        analyser.set_fft_size(FFT_SIZE as u32 * 2);
                        source.connect_with_audio_node(&analyser)?;
                        Ok(Graph { _context: context, _source: source, analyser })
                    })();
                    match result {
                        Ok(graph) => *g.borrow_mut() = Some(graph),
                        Err(err) => *e.borrow_mut() = Some(format!("audio graph failed: {err:?}")),
                    }
                }
                Err(err) => *e.borrow_mut() = Some(format!("microphone access denied: {err:?}")),
            }
        });
        Ok(AudioInput {
            graph,
            error,
            analyzer: Analyzer::new(48_000.0),
            scratch: vec![0.0; FFT_SIZE * 2],
            device_name: "microphone".into(),
        })
    }

    pub fn levels(&mut self) -> AudioLevels {
        let graph = self.graph.borrow();
        let Some(graph) = graph.as_ref() else { return self.analyzer.last() };
        let rate = graph._context.sample_rate();
        if (rate - 48_000.0).abs() > 1.0 && self.analyzer.last() == AudioLevels::default() {
            self.analyzer = Analyzer::new(rate);
        }
        graph.analyser.get_float_time_domain_data(&mut self.scratch);
        let n = self.scratch.len();
        let block = self.scratch[n - FFT_SIZE..].to_vec();
        self.analyzer.analyze(&block)
    }

    /// A permission or setup error, if one happened.
    pub fn error(&self) -> Option<String> {
        self.error.borrow().clone()
    }
}
