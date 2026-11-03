//! Built-in audio processors. Their state is allocated off the audio callback, so
//! construction never happens on the real-time thread.

#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectKind {
    Equalizer,
    Reverb,
    PlateReverb,
    Delay,
    TapeEcho,
    Chorus,
    Phaser,
    Waveshaper,
}

enum EffectState {
    Equalizer {
        low_250: [f32; 2],
        low_2k: [f32; 2],
        alpha_250: f32,
        alpha_2k: f32,
    },
    Delay {
        buffer: [Vec<f32>; 2],
        cursor: usize,
        sample_rate: f32,
    },
    Reverb {
        buffer: [Vec<f32>; 2],
        cursor: usize,
        sample_rate: f32,
    },
    Chorus {
        buffer: [Vec<f32>; 2],
        cursor: usize,
        phase: f32,
        sample_rate: f32,
    },
    Phaser {
        phase: f32,
        sample_rate: f32,
        previous_input: [f32; 2],
        previous_output: [f32; 2],
    },
    TapeEcho {
        buffer: [Vec<f32>; 2],
        cursor: usize,
        sample_rate: f32,
        low_pass: [f32; 2],
    },
    Waveshaper,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EffectParameters {
    pub mix: f32,
    pub amount: f32,
    pub low_db: f32,
    pub mid_db: f32,
    pub high_db: f32,
    pub time_seconds: f32,
    pub feedback: f32,
    pub rate_hz: f32,
    pub depth: f32,
    pub room_size: f32,
    pub drive: f32,
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

pub struct BuiltinEffect {
    kind: EffectKind,
    name: &'static str,
    state: EffectState,
}

impl BuiltinEffect {
    /// Builds a processor for a built-in effect name.
    pub fn new(name: &str, sample_rate: u32) -> Option<Self> {
        let sample_rate = sample_rate.max(1) as f32;
        let kind = match name {
            "3-Band EQ" => EffectKind::Equalizer,
            "Reverb" => EffectKind::Reverb,
            "Plate Reverb" => EffectKind::PlateReverb,
            "Delay" => EffectKind::Delay,
            "Tape Echo" => EffectKind::TapeEcho,
            "Juno Chorus" => EffectKind::Chorus,
            "Phaser" => EffectKind::Phaser,
            "Waveshaper" => EffectKind::Waveshaper,
            _ => return None,
        };
        let state = match kind {
            EffectKind::Equalizer => EffectState::Equalizer {
                low_250: [0.0; 2],
                low_2k: [0.0; 2],
                alpha_250: 1.0 - (-std::f32::consts::TAU * 250.0 / sample_rate).exp(),
                alpha_2k: 1.0 - (-std::f32::consts::TAU * 2_000.0 / sample_rate).exp(),
            },
            EffectKind::Delay => EffectState::Delay {
                buffer: std::array::from_fn(|_| vec![0.0; sample_rate as usize + 1]),
                cursor: 0,
                sample_rate,
            },
            EffectKind::Reverb | EffectKind::PlateReverb => EffectState::Reverb {
                buffer: std::array::from_fn(|_| vec![0.0; sample_rate as usize + 1]),
                cursor: 0,
                sample_rate,
            },
            EffectKind::Chorus => {
                let length = ((sample_rate * 0.05) as usize).max(4);
                EffectState::Chorus {
                    buffer: std::array::from_fn(|_| vec![0.0; length]),
                    cursor: 0,
                    phase: 0.0,
                    sample_rate,
                }
            }
            EffectKind::Phaser => EffectState::Phaser {
                phase: 0.0,
                sample_rate,
                previous_input: [0.0; 2],
                previous_output: [0.0; 2],
            },
            EffectKind::TapeEcho => EffectState::TapeEcho {
                buffer: std::array::from_fn(|_| vec![0.0; sample_rate as usize + 1]),
                cursor: 0,
                sample_rate,
                low_pass: [0.0; 2],
            },
            EffectKind::Waveshaper => EffectState::Waveshaper,
        };
        let name = BUILTIN_EFFECT_NAMES
            .into_iter()
            .find(|candidate| *candidate == name)?;
        Some(Self { kind, name, state })
    }

    /// Display name this processor was built for. Used to avoid rebuilding a
    /// processor while the slot still references the same effect.
    pub fn effect_name(&self) -> &'static str {
        self.name
    }

    pub fn process_with_parameters(
        &mut self,
        input: [f32; 2],
        parameters: EffectParameters,
    ) -> [f32; 2] {
        let parameters = sanitize(parameters);
        let wet = match &mut self.state {
            EffectState::Equalizer {
                low_250,
                low_2k,
                alpha_250,
                alpha_2k,
            } => {
                let gains = [
                    db_gain(parameters.low_db),
                    db_gain(parameters.mid_db),
                    db_gain(parameters.high_db),
                ];
                let mut output = [0.0; 2];
                for channel in 0..2 {
                    low_250[channel] += *alpha_250 * (input[channel] - low_250[channel]);
                    low_2k[channel] += *alpha_2k * (input[channel] - low_2k[channel]);
                    let bands = [
                        low_250[channel],
                        low_2k[channel] - low_250[channel],
                        input[channel] - low_2k[channel],
                    ];
                    output[channel] = (0..3).map(|band| bands[band] * gains[band]).sum();
                }
                output
            }
            EffectState::Delay {
                buffer,
                cursor,
                sample_rate,
            } => {
                let delay_samples = ((parameters.time_seconds * *sample_rate) as usize)
                    .clamp(1, buffer[0].len().saturating_sub(1));
                let read = (*cursor + buffer[0].len() - delay_samples) % buffer[0].len();
                let mut output = [0.0; 2];
                for channel in 0..2 {
                    output[channel] = buffer[channel][read] * parameters.amount;
                    buffer[channel][*cursor] =
                        input[channel] + output[channel] * parameters.feedback;
                }
                *cursor = (*cursor + 1) % buffer[0].len();
                output
            }
            EffectState::Reverb {
                buffer,
                cursor,
                sample_rate,
            } => {
                let room = parameters.room_size;
                let base_delay = if self.kind == EffectKind::PlateReverb {
                    0.035
                } else {
                    0.08
                };
                let delay_samples = ((*sample_rate * (base_delay + room * 0.18)) as usize)
                    .clamp(1, buffer[0].len().saturating_sub(1));
                let read = (*cursor + buffer[0].len() - delay_samples) % buffer[0].len();
                let mut output = [0.0; 2];
                let feedback = (0.25 + parameters.amount * 0.68).min(0.94);
                for channel in 0..2 {
                    output[channel] = buffer[channel][read];
                    buffer[channel][*cursor] = input[channel] + output[channel] * feedback;
                }
                *cursor = (*cursor + 1) % buffer[0].len();
                output
            }
            EffectState::Chorus {
                buffer,
                cursor,
                phase,
                sample_rate,
            } => {
                let length = buffer[0].len();
                let modulation = (phase.sin() + 1.0) * 0.5;
                let max_delay = parameters.depth * 0.018;
                let delay = (*sample_rate * (0.012 + modulation * max_delay)) as usize;
                let read = (*cursor + length - delay.min(length - 1)) % length;
                let output = [
                    buffer[0][read] * parameters.amount,
                    buffer[1][read] * parameters.amount,
                ];
                buffer[0][*cursor] = input[0];
                buffer[1][*cursor] = input[1];
                *cursor = (*cursor + 1) % length;
                *phase = (*phase + std::f32::consts::TAU * parameters.rate_hz / *sample_rate)
                    % std::f32::consts::TAU;
                output
            }
            EffectState::Phaser {
                phase,
                sample_rate,
                previous_input,
                previous_output,
            } => {
                let modulation = (phase.sin() + 1.0) * 0.5;
                let coefficient = 0.15 + modulation * parameters.depth * 0.75;
                let mut output = [0.0; 2];
                for channel in 0..2 {
                    let all_pass = -coefficient * input[channel]
                        + previous_input[channel]
                        + coefficient * previous_output[channel];
                    previous_input[channel] = input[channel];
                    previous_output[channel] = all_pass;
                    output[channel] = all_pass * parameters.amount;
                }
                *phase = (*phase + std::f32::consts::TAU * parameters.rate_hz / *sample_rate)
                    % std::f32::consts::TAU;
                output
            }
            EffectState::TapeEcho {
                buffer,
                cursor,
                sample_rate,
                low_pass,
            } => {
                let delay_samples = ((parameters.time_seconds * *sample_rate) as usize)
                    .clamp(1, buffer[0].len().saturating_sub(1));
                let read = (*cursor + buffer[0].len() - delay_samples) % buffer[0].len();
                let mut output = [0.0; 2];
                for channel in 0..2 {
                    low_pass[channel] += 0.28 * (buffer[channel][read] - low_pass[channel]);
                    output[channel] = low_pass[channel] * parameters.amount;
                    buffer[channel][*cursor] =
                        (input[channel] + low_pass[channel] * parameters.feedback).tanh();
                }
                *cursor = (*cursor + 1) % buffer[0].len();
                output
            }
            EffectState::Waveshaper => {
                let drive = 1.0 + parameters.drive * 19.0;
                let normalizer = drive.tanh().max(f32::EPSILON);
                [
                    (input[0] * drive).tanh() / normalizer,
                    (input[1] * drive).tanh() / normalizer,
                ]
            }
        };
        let mix = parameters.mix;
        [
            input[0] * (1.0 - mix) + wet[0] * mix,
            input[1] * (1.0 - mix) + wet[1] * mix,
        ]
    }
}

fn sanitize(mut parameters: EffectParameters) -> EffectParameters {
    parameters.mix = finite_clamp(parameters.mix, 0.0, 1.0);
    parameters.amount = finite_clamp(parameters.amount, 0.0, 1.0);
    parameters.low_db = finite_clamp(parameters.low_db, -12.0, 12.0);
    parameters.mid_db = finite_clamp(parameters.mid_db, -12.0, 12.0);
    parameters.high_db = finite_clamp(parameters.high_db, -12.0, 12.0);
    parameters.time_seconds = finite_clamp(parameters.time_seconds, 0.01, 1.0);
    parameters.feedback = finite_clamp(parameters.feedback, 0.0, 0.94);
    parameters.rate_hz = finite_clamp(parameters.rate_hz, 0.05, 8.0);
    parameters.depth = finite_clamp(parameters.depth, 0.0, 1.0);
    parameters.room_size = finite_clamp(parameters.room_size, 0.0, 1.0);
    parameters.drive = finite_clamp(parameters.drive, 0.0, 1.0);
    parameters
}

fn finite_clamp(value: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        min
    }
}

fn db_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters(mix: f32, amount: f32, feedback: f32) -> EffectParameters {
        EffectParameters {
            mix,
            amount,
            feedback,
            depth: 0.5,
            room_size: 0.55,
            drive: 0.5,
            rate_hz: 0.8,
            time_seconds: 0.2,
            ..EffectParameters::default()
        }
    }

    #[test]
    fn all_included_effects_process_finite_audio() {
        for name in BUILTIN_EFFECT_NAMES {
            let mut effect = BuiltinEffect::new(name, 48_000).unwrap();
            let output = effect.process_with_parameters([0.25, -0.25], parameters(1.0, 0.5, 0.5));
            assert!(
                output.into_iter().all(f32::is_finite),
                "{name} produced non-finite audio"
            );
        }
        assert!(BuiltinEffect::new("No Such Effect", 48_000).is_none());
    }

    #[test]
    fn waveshaper_is_finite_and_symmetric() {
        let mut effect = BuiltinEffect::new("Waveshaper", 48_000).unwrap();
        let output = effect.process_with_parameters([0.8, -0.8], parameters(1.0, 0.5, 0.5));
        assert!(output.into_iter().all(f32::is_finite));
        assert!(output[0].abs() <= 1.0);
        assert_eq!(output[0], -output[1]);
    }

    #[test]
    fn delay_emits_an_echo_after_its_configured_time() {
        let mut effect = BuiltinEffect::new("Delay", 1_000).unwrap();
        let parameters = EffectParameters {
            mix: 1.0,
            amount: 0.5,
            time_seconds: 0.32,
            feedback: 0.5,
            ..EffectParameters::default()
        };
        let _ = effect.process_with_parameters([1.0, -1.0], parameters);
        for _ in 0..319 {
            let _ = effect.process_with_parameters([0.0, 0.0], parameters);
        }
        let echo = effect.process_with_parameters([0.0, 0.0], parameters);
        assert!(echo[0] > 0.49);
        assert!(echo[1] < -0.49);
    }
}
