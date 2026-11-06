//! The audio engine: device handling, rendering and file I/O.
//!
//! [`AudioOutput`] owns the cpal stream and the command queue the UI talks to;
//! [`AudioEngine`] is the renderer that runs on the audio thread. The data they
//! share lives in [`model`], the renderer in [`engine`], and the free DSP
//! helpers in [`dsp`].

mod dsp;
mod engine;
mod model;

#[cfg(test)]
mod tests;

pub use dsp::*;
pub use model::*;
