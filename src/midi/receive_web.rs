//! Browser MIDI input with the Web MIDI API: after permission, every input port gets a message
//! handler that queues its control changes for the UI.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::mapping::{CcMessage, parse_message};

type Handler = Closure<dyn FnMut(web_sys::MidiMessageEvent)>;

#[derive(Default)]
struct Shared {
    queue: Vec<CcMessage>,
    names: Vec<String>,
    error: Option<String>,
    // Kept alive for as long as the receiver exists.
    access: Option<web_sys::MidiAccess>,
    handlers: Vec<Handler>,
}

pub struct MidiReceiver {
    shared: Rc<RefCell<Shared>>,
    pub port_names: Vec<String>,
}

impl MidiReceiver {
    /// Requests MIDI access (from a user gesture) and attaches to all inputs once granted.
    pub fn start() -> anyhow::Result<Self> {
        let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
        let promise = window
            .navigator()
            .request_midi_access()
            .map_err(|_| anyhow::anyhow!("Web MIDI is not available in this browser"))?;
        let shared: Rc<RefCell<Shared>> = Rc::default();
        let s = shared.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(access) => {
                    if let Err(e) = attach(&s, access) {
                        s.borrow_mut().error = Some(format!("MIDI setup failed: {e:?}"));
                    }
                }
                Err(e) => s.borrow_mut().error = Some(format!("MIDI access denied: {e:?}")),
            }
        });
        Ok(MidiReceiver { shared, port_names: Vec::new() })
    }

    pub fn poll(&mut self) -> Vec<CcMessage> {
        let mut s = self.shared.borrow_mut();
        if s.names.len() != self.port_names.len() {
            self.port_names = s.names.clone();
        }
        std::mem::take(&mut s.queue)
    }

    pub fn error(&self) -> Option<String> {
        self.shared.borrow().error.clone()
    }
}

fn attach(shared: &Rc<RefCell<Shared>>, access: JsValue) -> Result<(), JsValue> {
    let access: web_sys::MidiAccess = access.dyn_into()?;
    let mut handlers = Vec::new();
    let mut names = Vec::new();
    for entry in access.inputs().values() {
        let input: web_sys::MidiInput = entry?.dyn_into()?;
        names.push(input.name().unwrap_or_else(|| "MIDI input".into()));
        let sink = shared.clone();
        let handler: Handler = Closure::new(move |ev: web_sys::MidiMessageEvent| {
            if let Ok(data) = ev.data()
                && let Some(msg) = parse_message(&data)
            {
                sink.borrow_mut().queue.push(msg);
            }
        });
        input.set_onmidimessage(Some(handler.as_ref().unchecked_ref()));
        handlers.push(handler);
    }
    if names.is_empty() {
        return Err(JsValue::from_str("no MIDI inputs connected"));
    }
    let mut s = shared.borrow_mut();
    s.names = names;
    s.handlers = handlers;
    s.access = Some(access);
    Ok(())
}
