//! Desktop MIDI input with midir: every input port is opened and its control changes are queued
//! for the UI thread.

use std::sync::{Arc, Mutex};

use midir::{MidiInput, MidiInputConnection};

use super::mapping::{CcMessage, parse_message};

pub struct MidiReceiver {
    _connections: Vec<MidiInputConnection<()>>,
    queue: Arc<Mutex<Vec<CcMessage>>>,
    pub port_names: Vec<String>,
}

impl MidiReceiver {
    /// Opens all MIDI input ports present right now. Fails when there are none.
    pub fn start() -> anyhow::Result<Self> {
        let probe = MidiInput::new("cellular-automata").map_err(|e| anyhow::anyhow!("{e}"))?;
        let ports = probe.ports();
        if ports.is_empty() {
            anyhow::bail!("no MIDI input devices found");
        }
        let queue: Arc<Mutex<Vec<CcMessage>>> = Arc::default();
        let mut connections = Vec::new();
        let mut port_names = Vec::new();
        for port in ports {
            let input = MidiInput::new("cellular-automata").map_err(|e| anyhow::anyhow!("{e}"))?;
            let name = input.port_name(&port).unwrap_or_else(|_| "MIDI input".into());
            let sink = queue.clone();
            match input.connect(
                &port,
                "cellular-automata-in",
                move |_stamp, bytes, _| {
                    if let Some(msg) = parse_message(bytes) {
                        sink.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
                    }
                },
                (),
            ) {
                Ok(c) => {
                    connections.push(c);
                    port_names.push(name);
                }
                Err(e) => log::warn!("could not open MIDI port {name}: {e}"),
            }
        }
        if connections.is_empty() {
            anyhow::bail!("no MIDI input port could be opened");
        }
        Ok(MidiReceiver { _connections: connections, queue, port_names })
    }

    /// Control changes received since the last call, oldest first.
    pub fn poll(&mut self) -> Vec<CcMessage> {
        std::mem::take(&mut *self.queue.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn error(&self) -> Option<String> {
        None
    }
}
