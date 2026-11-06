//! Delay lines: a clean digital delay and a tape-style echo with a low pass.

use super::params::EffectParameters;
use super::BuiltinProcessor;

/// A stereo delay line, optionally rolled off like tape.
pub struct Delay {
    /// Delay buffers, one per channel.
    buffer: [Vec<f32>; 2],
    /// Write position in the buffers.
    cursor: usize,
    /// Sample rate in Hz.
    sample_rate: f32,
    /// Tape mode keeps the one-pole low pass in the feedback path.
    tape: bool,
    /// Low-pass state for tape mode, one per channel.
    low_pass: [f32; 2],
}

impl Delay {
    /// Builds the processor for a sample rate.
    ///
    /// # Parameters
    ///
    /// - `sample_rate`: the engine's sample rate in Hz.
    /// - `tape`: `true` for the filtered, saturating tape voicing.
    ///
    /// # Returns
    ///
    /// A delay with a one-second buffer per channel.
    pub fn new(sample_rate: f32, tape: bool) -> Self {
        let length = (sample_rate as usize).max(2) + 1;
        Self {
            buffer: std::array::from_fn(|_| vec![0.0; length]),
            cursor: 0,
            sample_rate,
            tape,
            low_pass: [0.0; 2],
        }
    }
}

impl BuiltinProcessor for Delay {
    fn process(&mut self, input: [f32; 2], parameters: EffectParameters) -> [f32; 2] {
        let delay_samples = ((parameters.time_seconds * self.sample_rate) as usize)
            .clamp(1, self.buffer[0].len().saturating_sub(1));
        let read = (self.cursor + self.buffer[0].len() - delay_samples) % self.buffer[0].len();
        let mut output = [0.0; 2];
        for channel in 0..2 {
            if self.tape {
                self.low_pass[channel] += 0.28 * (self.buffer[channel][read] - self.low_pass[channel]);
                output[channel] = self.low_pass[channel] * parameters.amount;
                self.buffer[channel][self.cursor] =
                    (input[channel] + self.low_pass[channel] * parameters.feedback).tanh();
            } else {
                output[channel] = self.buffer[channel][read] * parameters.amount;
                self.buffer[channel][self.cursor] =
                    input[channel] + output[channel] * parameters.feedback;
            }
        }
        self.cursor = (self.cursor + 1) % self.buffer[0].len();
        output
    }
}
