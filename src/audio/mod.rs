//! Audio input for audio-reactive parameters: capture (per platform) and analysis (shared).

pub mod analysis;

#[cfg(not(target_arch = "wasm32"))]
mod capture_native;
#[cfg(not(target_arch = "wasm32"))]
pub use capture_native::AudioInput;

#[cfg(target_arch = "wasm32")]
mod capture_web;
#[cfg(target_arch = "wasm32")]
pub use capture_web::AudioInput;
