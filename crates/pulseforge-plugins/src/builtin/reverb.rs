//! Feedback reverb, in two voicings: a hall and a shorter plate.

use super::BuiltinProcessor;
use super::params::EffectParameters;

/// A single stereo feedback delay line used as a reverb.
pub struct Reverb {
    /// Delay buffers, one per channel.
    buffer: [Vec<f32>; 2],
    /// Write position in the buffers.
    cursor: usize,
    /// Sample rate in Hz.
    sample_rate: f32,
    /// Plate voicing uses a shorter base delay.
    plate: bool,
}

impl Reverb {
    /// Builds the processor for a sample rate.
    ///
    /// # Parameters
    ///
    /// - `sample_rate`: the engine's sample rate in Hz.
    /// - `plate`: `true` for the short plate voicing.
    ///
    /// # Returns
    ///
    /// A reverb with a one-second delay line per channel.
    pub fn new(sample_rate: f32, plate: bool) -> Self {
        let length = (sample_rate as usize).max(2) + 1;
        Self {
            buffer: std::array::from_fn(|_| vec![0.0; length]),
            cursor: 0,
            sample_rate,
            plate,
        }
    }
}

impl BuiltinProcessor for Reverb {
    fn process(&mut self, input: [f32; 2], parameters: EffectParameters) -> [f32; 2] {
        let room = parameters.room_size;
        let base_delay = if self.plate { 0.035 } else { 0.08 };
        let delay_samples = ((self.sample_rate * (base_delay + room * 0.18)) as usize)
            .clamp(1, self.buffer[0].len().saturating_sub(1));
        let read = (self.cursor + self.buffer[0].len() - delay_samples) % self.buffer[0].len();
        let feedback = (0.25 + parameters.amount * 0.68).min(0.94);
        let mut output = [0.0; 2];
        for channel in 0..2 {
            output[channel] = self.buffer[channel][read];
            self.buffer[channel][self.cursor] = input[channel] + output[channel] * feedback;
        }
        self.cursor = (self.cursor + 1) % self.buffer[0].len();
        output
    }
}
