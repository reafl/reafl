//! A stateless hyperbolic-tangent waveshaper.

use super::BuiltinProcessor;
use super::params::EffectParameters;

/// Saturates the signal, normalised so full drive stays at unity peak.
pub struct Waveshaper;

impl BuiltinProcessor for Waveshaper {
    fn process(&mut self, input: [f32; 2], parameters: EffectParameters) -> [f32; 2] {
        let drive = 1.0 + parameters.drive * 19.0;
        let normalizer = drive.tanh().max(f32::EPSILON);
        [
            (input[0] * drive).tanh() / normalizer,
            (input[1] * drive).tanh() / normalizer,
        ]
    }
}
