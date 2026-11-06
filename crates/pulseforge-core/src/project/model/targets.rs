//! Automation sources and the parameter enums that automation addresses.
//!
//! Every automatable parameter is addressed by a [`ParameterTarget`], which
//! carries the owning object and one of the parameter enums below.

use serde::{Deserialize, Serialize};

use super::device::*;
use super::params::*;
use super::piano::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentParameter {
    Gain,
    Pan,
    CutoffHz,
    Resonance,
    PulseWidth,
    SubLevel,
    Tone,
    Decay,
    FmRatio,
    FmIndex,
    /// Index into [`SignalWaveform`]: an enum automated as a plain integer.
    Waveform,
    /// Index into [`ArpMode`]: an enum automated as a plain integer.
    ArpMode,
    /// Arpeggiator note length as a fraction of its step.
    ArpGate,
    /// Arpeggiator octave span, rounded to a whole number.
    ArpOctaves,
    /// Arpeggiator rate amount, in whatever unit the arp rate uses.
    ArpRate,
    /// MIDI note the signal generator holds, rounded to a whole note.
    SynthNote,
}

impl InstrumentParameter {
    /// Number of choices for parameters addressed as integers. `None` means the
    /// parameter is continuous.
    pub fn discrete_count(self) -> Option<u32> {
        match self {
            Self::Waveform => Some(SignalWaveform::ALL.len() as u32),
            Self::ArpMode => Some(ArpMode::ALL.len() as u32),
            Self::ArpOctaves => Some(7),
            Self::SynthNote => Some(128),
            _ => None,
        }
    }

    /// True when the value is rounded to a whole number before it is applied.
    pub fn is_discrete(self) -> bool {
        self.discrete_count().is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectParameter {
    Mix,
    Amount,
    LowDb,
    MidDb,
    HighDb,
    TimeSeconds,
    Feedback,
    RateHz,
    Depth,
    RoomSize,
    Drive,
}

/// Mixer controls that can be automated, independent of any insert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MixerParameter {
    Gain,
    Pan,
    /// Mid/side stereo width, `0` (mono) to `2` (double side).
    Width,
}

impl MixerParameter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gain => "Volume",
            Self::Pan => "Pan",
            Self::Width => "Width",
        }
    }
}

/// Parameters of an automation lane's own source, so modulation can modulate
/// modulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationLaneParameter {
    Base,
    Depth,
    /// The rate amount, in whatever unit the lane's `rate` uses.
    RateAmount,
    /// Index into [`LfoShape`] for LFO lanes.
    Shape,
    /// Sustain level of a MIDI-envelope lane.
    Sustain,
    /// Perlin octave count.
    Octaves,
}

impl AutomationLaneParameter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Base => "Base",
            Self::Depth => "Depth",
            Self::RateAmount => "Rate",
            Self::Shape => "Shape",
            Self::Sustain => "Sustain",
            Self::Octaves => "Octaves",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterTarget {
    Instrument {
        instrument_id: usize,
        parameter: InstrumentParameter,
    },
    InsertEffect {
        channel_id: usize,
        slot_id: usize,
        parameter: EffectParameter,
    },
    /// A parameter of a hosted VST3/CLAP instance. The value is normalized.
    PluginParameter { host_id: u64, parameter_id: u32 },
    /// A mixer channel's own volume or pan.
    MixerChannel {
        channel_id: usize,
        parameter: MixerParameter,
    },
    /// A parameter of another automation lane's source.
    AutomationLane {
        track_id: usize,
        parameter: AutomationLaneParameter,
    },
    /// Project tempo in BPM.
    Tempo,
}

impl Default for ParameterTarget {
    fn default() -> Self {
        Self::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Gain,
        }
    }
}

impl ParameterTarget {
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Instrument { parameter, .. } => match parameter {
                InstrumentParameter::Gain => (0.0, 1.0),
                InstrumentParameter::Pan => (-1.0, 1.0),
                InstrumentParameter::CutoffHz => (40.0, 16_000.0),
                InstrumentParameter::Resonance => (0.0, 0.9),
                InstrumentParameter::PulseWidth => (0.05, 0.95),
                InstrumentParameter::SubLevel | InstrumentParameter::FmIndex => (0.0, 1.0),
                InstrumentParameter::Tone => (0.0, 1.0),
                InstrumentParameter::Decay => (0.05, 4.0),
                InstrumentParameter::FmRatio => (0.25, 16.0),
                // Enums are addressed as integers, so their range is the index.
                InstrumentParameter::Waveform => (0.0, (SignalWaveform::ALL.len() - 1) as f32),
                InstrumentParameter::ArpMode => (0.0, (ArpMode::ALL.len() - 1) as f32),
                InstrumentParameter::ArpGate => (0.01, 1.0),
                InstrumentParameter::ArpOctaves => (1.0, 8.0),
                InstrumentParameter::ArpRate => (0.1, 64.0),
                InstrumentParameter::SynthNote => (0.0, 127.0),
            },
            Self::InsertEffect { parameter, .. } => match parameter {
                EffectParameter::Mix
                | EffectParameter::Amount
                | EffectParameter::Feedback
                | EffectParameter::Depth
                | EffectParameter::RoomSize
                | EffectParameter::Drive => (0.0, 1.0),
                EffectParameter::LowDb | EffectParameter::MidDb | EffectParameter::HighDb => {
                    (-12.0, 12.0)
                }
                EffectParameter::TimeSeconds => (0.01, 2.0),
                EffectParameter::RateHz => (0.05, 8.0),
            },
            // Hosted plug-in parameters are always normalized 0..=1.
            Self::PluginParameter { .. } => (0.0, 1.0),
            Self::MixerChannel { parameter, .. } => match parameter {
                MixerParameter::Gain => (0.0, 1.0),
                MixerParameter::Pan => (-1.0, 1.0),
                MixerParameter::Width => (0.0, 2.0),
            },
            Self::AutomationLane { parameter, .. } => match parameter {
                AutomationLaneParameter::Base | AutomationLaneParameter::Depth => (0.0, 1.0),
                AutomationLaneParameter::RateAmount => (0.1, 64.0),
                AutomationLaneParameter::Shape => (0.0, (LfoShape::ALL.len() - 1) as f32),
                AutomationLaneParameter::Sustain => (0.0, 1.0),
                AutomationLaneParameter::Octaves => (1.0, 16.0),
            },
            Self::Tempo => (20.0, 300.0),
        }
    }

    pub fn normalize_value(self, value: f32) -> f32 {
        let (min, max) = self.range();
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }

    pub fn denormalize_value(self, value: f32) -> f32 {
        let (min, max) = self.range();
        min + value.clamp(0.0, 1.0) * (max - min)
    }
}

impl InstrumentParameter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gain => "Gain",
            Self::Pan => "Pan",
            Self::CutoffHz => "Cutoff",
            Self::Resonance => "Resonance",
            Self::PulseWidth => "Pulse width",
            Self::SubLevel => "Sub level",
            Self::Tone => "Tone",
            Self::Decay => "Decay",
            Self::FmRatio => "FM ratio",
            Self::FmIndex => "FM index",
            Self::Waveform => "Waveform",
            Self::ArpMode => "Arp mode",
            Self::ArpGate => "Arp gate",
            Self::ArpOctaves => "Arp octaves",
            Self::ArpRate => "Arp rate",
            Self::SynthNote => "MIDI note",
        }
    }
}

impl EffectParameter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mix => "Mix",
            Self::Amount => "Amount",
            Self::LowDb => "Low",
            Self::MidDb => "Mid",
            Self::HighDb => "High",
            Self::TimeSeconds => "Time",
            Self::Feedback => "Feedback",
            Self::RateHz => "Rate",
            Self::Depth => "Depth",
            Self::RoomSize => "Room size",
            Self::Drive => "Drive",
        }
    }
}
