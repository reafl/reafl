//! Automation sources and the parameter enums that automation addresses.
//!
//! Every automatable parameter is addressed by a [`ParameterTarget`], which
//! carries the owning object and one of the parameter enums below.

use serde::{Deserialize, Serialize};

use super::device::*;
use super::params::*;
use super::piano::*;

/// Continuous and integer parameters of an instrument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstrumentParameter {
    /// Output gain in 0..=1.
    Gain,
    /// Pan in -1..=1.
    Pan,
    /// Filter cutoff in Hz.
    CutoffHz,
    /// Filter resonance.
    Resonance,
    /// Pulse width of the Juno style voice.
    PulseWidth,
    /// Sub oscillator level of the Juno style voice.
    SubLevel,
    /// Tone control of the M1 style voice.
    Tone,
    /// Decay of the M1 style voice.
    Decay,
    /// FM modulator ratio.
    FmRatio,
    /// FM modulation index.
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

/// Parameters of an insert effect slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectParameter {
    /// Dry/wet balance.
    Mix,
    /// Generic amount control.
    Amount,
    /// Low band gain in dB.
    LowDb,
    /// Mid band gain in dB.
    MidDb,
    /// High band gain in dB.
    HighDb,
    /// Delay or release time in seconds.
    TimeSeconds,
    /// Feedback amount.
    Feedback,
    /// Modulation rate in Hz.
    RateHz,
    /// Modulation depth.
    Depth,
    /// Reverb room size.
    RoomSize,
    /// Waveshaper drive.
    Drive,
}

/// Mixer controls that can be automated, independent of any insert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MixerParameter {
    /// Channel fader gain.
    Gain,
    /// Channel pan.
    Pan,
    /// Mid/side stereo width, `0` (mono) to `2` (double side).
    Width,
}

impl MixerParameter {
    /// Human readable label for menus and tooltips.
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
    /// Centre value of the modulation.
    Base,
    /// Amount the modulation swings around the base.
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
    /// Human readable label for menus and tooltips.
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

/// Address of one automatable parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParameterTarget {
    /// A parameter of a channel-rack instrument.
    Instrument {
        /// Index into [`Project::instruments`](super::arrangement::Project::instruments).
        instrument_id: usize,
        /// Which instrument parameter.
        parameter: InstrumentParameter,
    },
    /// A parameter of one insert slot.
    InsertEffect {
        /// Mixer channel, or the master index for the master bus.
        channel_id: usize,
        /// Slot index inside the channel's insert chain.
        slot_id: usize,
        /// Which effect parameter.
        parameter: EffectParameter,
    },
    /// A parameter of a hosted VST3/CLAP instance. The value is normalized.
    PluginParameter {
        /// Host instance id of the plug-in.
        host_id: u64,
        /// Parameter index inside the plug-in.
        parameter_id: u32,
    },
    /// A mixer channel's own volume or pan.
    MixerChannel {
        /// Mixer channel, or the master index for the master bus.
        channel_id: usize,
        /// Which mixer control.
        parameter: MixerParameter,
    },
    /// A parameter of another automation lane's source.
    AutomationLane {
        /// Index into [`Project::automation_tracks`](super::arrangement::Project::automation_tracks).
        track_id: usize,
        /// Which lane source parameter.
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
    /// Lowest and highest value this parameter accepts.
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

    /// Maps a real value into the normalized 0..=1 range automation uses.
    pub fn normalize_value(self, value: f32) -> f32 {
        let (min, max) = self.range();
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }

    /// Maps a normalized 0..=1 automation value back into the real range.
    pub fn denormalize_value(self, value: f32) -> f32 {
        let (min, max) = self.range();
        min + value.clamp(0.0, 1.0) * (max - min)
    }
}

impl InstrumentParameter {
    /// Human readable label for menus and tooltips.
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
    /// Human readable label for menus and tooltips.
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
