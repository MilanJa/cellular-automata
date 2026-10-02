//! MIDI controller input: a pure CC-to-param mapping (shared) and a per-platform receiver.

pub mod mapping;

#[cfg(not(target_arch = "wasm32"))]
mod receive_native;
#[cfg(not(target_arch = "wasm32"))]
pub use receive_native::MidiReceiver;

#[cfg(target_arch = "wasm32")]
mod receive_web;
#[cfg(target_arch = "wasm32")]
pub use receive_web::MidiReceiver;
