//! Built-in audio processors.
//!
//! Each effect is a small struct in its own module implementing
//! [`BuiltinProcessor`], so adding a new processor means writing one file and
//! adding a line to the table in [`BuiltinEffect::new`]. State is allocated in
//! the constructor, which runs off the audio callback; `process` only does
//! arithmetic, so it stays real-time safe.

mod delay;
mod eq;
mod modulation;
mod params;
mod reverb;
mod waveshaper;

pub use params::EffectParameters;

use delay::Delay;
use eq::Equalizer;
use modulation::{Chorus, Phaser};
use reverb::Reverb;
use waveshaper::Waveshaper;

/// One DSP processor: stereo in, stereo out, plus the block's parameters.
///
/// Implementations must be real-time safe: no allocation, locking or file I/O in
/// [`BuiltinProcessor::process`].
pub trait BuiltinProcessor: Send {
    /// Runs one stereo frame.
    ///
    /// # Parameters
    ///
    /// - `input`: the dry stereo frame.
    /// - `parameters`: the slot's parameters, already sanitised.
    ///
    /// # Returns
    ///
    /// The fully wet stereo frame. Dry/wet mixing is applied by the caller.
    fn process(&mut self, input: [f32; 2], parameters: EffectParameters) -> [f32; 2];
}

/// Every built-in effect name, in the order the plug-in picker lists them.
pub const BUILTIN_EFFECT_NAMES: [&str; 8] = [
    "3-Band EQ",
    "Reverb",
    "Plate Reverb",
    "Delay",
    "Tape Echo",
    "Juno Chorus",
    "Phaser",
    "Waveshaper",
];

/// A host-side wrapper around one built-in processor.
pub struct BuiltinEffect {
    name: &'static str,
    processor: Box<dyn BuiltinProcessor>,
}

impl BuiltinEffect {
    /// Builds a processor for a built-in effect name.
    ///
    /// # Parameters
    ///
    /// - `name`: one of [`BUILTIN_EFFECT_NAMES`].
    /// - `sample_rate`: the engine's sample rate, used to size the buffers.
    ///
    /// # Returns
    ///
    /// The processor, or `None` when the name is not a built-in effect.
    pub fn new(name: &str, sample_rate: u32) -> Option<Self> {
        let rate = sample_rate.max(1) as f32;
        let (name, processor): (&'static str, Box<dyn BuiltinProcessor>) = match name {
            "3-Band EQ" => ("3-Band EQ", Box::new(Equalizer::new(rate))),
            "Reverb" => ("Reverb", Box::new(Reverb::new(rate, false))),
            "Plate Reverb" => ("Plate Reverb", Box::new(Reverb::new(rate, true))),
            "Delay" => ("Delay", Box::new(Delay::new(rate, false))),
            "Tape Echo" => ("Tape Echo", Box::new(Delay::new(rate, true))),
            "Juno Chorus" => ("Juno Chorus", Box::new(Chorus::new(rate))),
            "Phaser" => ("Phaser", Box::new(Phaser::new(rate))),
            "Waveshaper" => ("Waveshaper", Box::new(Waveshaper)),
            _ => return None,
        };
        Some(Self { name, processor })
    }

    /// The effect's display name.
    ///
    /// # Returns
    ///
    /// The name the processor was built from.
    pub fn effect_name(&self) -> &'static str {
        self.name
    }

    /// Processes one frame and applies the slot's dry/wet mix.
    ///
    /// # Parameters
    ///
    /// - `input`: the dry stereo frame.
    /// - `parameters`: the slot's parameters; they are clamped to their ranges.
    ///
    /// # Returns
    ///
    /// The mixed stereo frame.
    pub fn process_with_parameters(
        &mut self,
        input: [f32; 2],
        parameters: EffectParameters,
    ) -> [f32; 2] {
        let parameters = params::sanitize(parameters);
        let wet = self.processor.process(input, parameters);
        let mix = parameters.mix;
        [
            input[0] * (1.0 - mix) + wet[0] * mix,
            input[1] * (1.0 - mix) + wet[1] * mix,
        ]
    }
}
