//! Decoded samples shared between the UI thread and the audio thread.

use std::f32::consts::TAU;

use crate::project::DefaultSample;

/// A decoded sample: interleaved stereo frames plus the rate they were captured
/// at.
///
/// Samples are shared between clips by path, so the engine owns one copy per
/// file even when the arrangement uses it in several places.
pub struct SampleData {
    /// Sample rate the frames were captured at.
    pub sample_rate: u32,
    /// Stereo frames.
    pub frames: Vec<[f32; 2]>,
}

impl SampleData {
    /// Synthesises one of the shipped drum samples.
    ///
    /// # Parameters
    ///
    /// - `kind`: which drum sample to build; [`DefaultSample::None`] yields
    ///   `None`.
    /// - `sample_rate`: rate the frames should be generated at.
    ///
    /// # Returns
    ///
    /// The generated sample, or `None` when no sample was requested.
    pub(crate) fn default_drum(kind: DefaultSample, sample_rate: u32) -> Option<Self> {
        let duration = match kind {
            DefaultSample::Kick => 0.48,
            DefaultSample::Snare => 0.32,
            DefaultSample::HiHat => 0.12,
            DefaultSample::None => return None,
        };
        let frame_count = (duration * sample_rate as f32) as usize;
        let mut noise_state: u32 = match kind {
            DefaultSample::Kick => 0x2a31_9d2b,
            DefaultSample::Snare => 0x5a17_02e3,
            DefaultSample::HiHat => 0x7f4a_7c15,
            DefaultSample::None => 0,
        };
        let frames = (0..frame_count)
            .map(|index| {
                let time = index as f32 / sample_rate as f32;
                let progress = time / duration;
                let signal = match kind {
                    DefaultSample::Kick => {
                        let pitch = 42.0 + 105.0 * (-time * 15.0).exp();
                        let envelope = (-time * 9.0).exp();
                        (TAU * pitch * time).sin() * envelope * 0.88
                    }
                    DefaultSample::Snare => {
                        noise_state ^= noise_state << 13;
                        noise_state ^= noise_state >> 17;
                        noise_state ^= noise_state << 5;
                        let noise = noise_state as i32 as f32 / i32::MAX as f32;
                        let envelope = (-progress * 8.5).exp();
                        let body = (TAU * 185.0 * time).sin() * (-time * 18.0).exp() * 0.42;
                        (noise * 0.68 + body) * envelope * 0.78
                    }
                    DefaultSample::HiHat => {
                        noise_state ^= noise_state << 13;
                        noise_state ^= noise_state >> 17;
                        noise_state ^= noise_state << 5;
                        let noise = noise_state as i32 as f32 / i32::MAX as f32;
                        let envelope = (-progress * 13.0).exp();
                        let high_pass = noise
                            - (noise_state.rotate_left(7) as i32 as f32 / i32::MAX as f32) * 0.55;
                        high_pass * envelope * 0.48
                    }
                    DefaultSample::None => 0.0,
                };
                [signal, signal]
            })
            .collect();
        Some(Self {
            sample_rate,
            frames,
        })
    }
}
