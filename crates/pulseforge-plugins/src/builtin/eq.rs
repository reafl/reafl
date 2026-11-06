//! The three-band equaliser: two one-pole crossovers and three shelf gains.

use super::params::{EffectParameters, db_gain};
use super::BuiltinProcessor;

/// Two crossover filters split the signal into low, mid and high bands.
pub struct Equalizer {
    /// Low band state, one per channel.
    low_250: [f32; 2],
    /// State of the 2 kHz crossover.
    low_2k: [f32; 2],
    /// One-pole coefficient for 250 Hz.
    alpha_250: f32,
    /// One-pole coefficient for 2 kHz.
    alpha_2k: f32,
}

impl Equalizer {
    /// Builds the processor for a sample rate.
    ///
    /// # Parameters
    ///
    /// - `sample_rate`: the engine's sample rate in Hz.
    ///
    /// # Returns
    ///
    /// An equaliser with settled (zero) filter state.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            low_250: [0.0; 2],
            low_2k: [0.0; 2],
            alpha_250: 1.0 - (-std::f32::consts::TAU * 250.0 / sample_rate).exp(),
            alpha_2k: 1.0 - (-std::f32::consts::TAU * 2_000.0 / sample_rate).exp(),
        }
    }
}

impl BuiltinProcessor for Equalizer {
    fn process(&mut self, input: [f32; 2], parameters: EffectParameters) -> [f32; 2] {
        let gains = [
            db_gain(parameters.low_db),
            db_gain(parameters.mid_db),
            db_gain(parameters.high_db),
        ];
        let mut output = [0.0; 2];
        for channel in 0..2 {
            self.low_250[channel] += self.alpha_250 * (input[channel] - self.low_250[channel]);
            self.low_2k[channel] += self.alpha_2k * (input[channel] - self.low_2k[channel]);
            let bands = [
                self.low_250[channel],
                self.low_2k[channel] - self.low_250[channel],
                input[channel] - self.low_2k[channel],
            ];
            output[channel] = (0..3).map(|band| bands[band] * gains[band]).sum();
        }
        output
    }
}
