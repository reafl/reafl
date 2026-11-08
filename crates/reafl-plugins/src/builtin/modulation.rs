//! Modulation effects: a chorus with a modulated delay and a phaser ladder.

use super::BuiltinProcessor;
use super::params::EffectParameters;

/// A short modulated delay with an LFO-driven read position.
pub struct Chorus {
    /// Delay buffers, one per channel.
    buffer: [Vec<f32>; 2],
    /// Write position in the buffers.
    cursor: usize,
    /// LFO phase in radians.
    phase: f32,
    /// Sample rate in Hz.
    sample_rate: f32,
}

impl Chorus {
    /// Builds the processor for a sample rate.
    ///
    /// # Parameters
    ///
    /// - `sample_rate`: the engine's sample rate in Hz.
    ///
    /// # Returns
    ///
    /// A chorus with a 50 ms buffer per channel.
    pub fn new(sample_rate: f32) -> Self {
        let length = ((sample_rate * 0.05) as usize).max(4);
        Self {
            buffer: std::array::from_fn(|_| vec![0.0; length]),
            cursor: 0,
            phase: 0.0,
            sample_rate,
        }
    }
}

impl BuiltinProcessor for Chorus {
    fn process(&mut self, input: [f32; 2], parameters: EffectParameters) -> [f32; 2] {
        let length = self.buffer[0].len();
        let modulation = (self.phase.sin() + 1.0) * 0.5;
        let max_delay = parameters.depth * 0.018;
        let delay = (self.sample_rate * (0.012 + modulation * max_delay)) as usize;
        let read = (self.cursor + length - delay.min(length - 1)) % length;
        let output = [
            self.buffer[0][read] * parameters.amount,
            self.buffer[1][read] * parameters.amount,
        ];
        self.buffer[0][self.cursor] = input[0];
        self.buffer[1][self.cursor] = input[1];
        self.cursor = (self.cursor + 1) % length;
        self.phase = (self.phase + std::f32::consts::TAU * parameters.rate_hz / self.sample_rate)
            % std::f32::consts::TAU;
        output
    }
}

/// A first-order all-pass whose coefficient is swept by the LFO.
pub struct Phaser {
    /// LFO phase in radians.
    phase: f32,
    /// Sample rate in Hz.
    sample_rate: f32,
    /// Previous dry input, one per channel.
    previous_input: [f32; 2],
    /// Previous all-pass output, one per channel.
    previous_output: [f32; 2],
}

impl Phaser {
    /// Builds the processor for a sample rate.
    ///
    /// # Parameters
    ///
    /// - `sample_rate`: the engine's sample rate in Hz.
    ///
    /// # Returns
    ///
    /// A phaser with a settled all-pass state.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            sample_rate,
            previous_input: [0.0; 2],
            previous_output: [0.0; 2],
        }
    }
}

impl BuiltinProcessor for Phaser {
    fn process(&mut self, input: [f32; 2], parameters: EffectParameters) -> [f32; 2] {
        let modulation = (self.phase.sin() + 1.0) * 0.5;
        let coefficient = 0.15 + modulation * parameters.depth * 0.75;
        let mut output = [0.0; 2];
        for channel in 0..2 {
            let all_pass = -coefficient * input[channel]
                + self.previous_input[channel]
                + coefficient * self.previous_output[channel];
            self.previous_input[channel] = input[channel];
            self.previous_output[channel] = all_pass;
            output[channel] = all_pass * parameters.amount;
        }
        self.phase = (self.phase + std::f32::consts::TAU * parameters.rate_hz / self.sample_rate)
            % std::f32::consts::TAU;
        output
    }
}
