//! Automation sources and the parameter enums that automation addresses.

//! The project data model: values, targets and the structures they fill.

#![allow(unused_imports)]

use serde::{Deserialize, Serialize};

use super::params::*;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCurve {
    #[default]
    Linear,
    Smooth,
    Bezier,
    Curved,
    Pulse,
    Constant,
    Steps,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum AutomationSource {
    #[default]
    Curve,
    /// A drawn curve whose points are offsets added to the parameter's own
    /// value rather than replacing it. Values run from `-1` to `1`, so the lane
    /// can pull a parameter down as well as push it up.
    RelativeCurve,
    MidiEnvelope {
        instrument_id: usize,
        attack: TimeValue,
        decay: TimeValue,
        sustain: f32,
        release: TimeValue,
        base: f32,
        depth: f32,
    },
    Lfo {
        shape: LfoShape,
        rate: TimeValue,
        retrigger: bool,
        base: f32,
        depth: f32,
    },
    Perlin {
        seed: u32,
        rate: TimeValue,
        octaves: u32,
        base: f32,
        depth: f32,
    },
    /// One random value per step of `rate`, held until the next step. Kept out
    /// of [`LfoShape`] so it reads as its own modulation category.
    SampleAndHold {
        seed: u32,
        rate: TimeValue,
        base: f32,
        depth: f32,
    },
}

impl AutomationSource {
    /// True when the source reads `track.points` as a drawn curve.
    pub fn edits_points(&self) -> bool {
        matches!(self, Self::Curve | Self::RelativeCurve)
    }

    /// True when the drawn curve is an offset added to the parameter instead of
    /// replacing it, so its points may be negative.
    pub fn is_relative(&self) -> bool {
        matches!(self, Self::RelativeCurve)
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl TimeSignature {
    pub fn steps_per_bar(self) -> u64 {
        (u64::from(self.numerator.max(1)) * 16 / u64::from(self.denominator.max(1))).max(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepHit {
    pub instrument_id: usize,
    pub step: u64,
}

///
/// `repeat_count` and `repeat_ticks` implement FL-style note repeats: the note
/// re-triggers `repeat_count` extra times, `repeat_ticks` apart (0 = half the
/// note length), with velocity falling off by `repeat_decay` per repeat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PianoNote {
    pub instrument_id: usize,
    pub start_step: u64,
    pub start_substep: u8,
    pub duration_steps: u64,
    pub duration_substeps: u8,
    pub pitch: u8,
    pub velocity: u8,
    pub repeat_count: u8,
    pub repeat_ticks: u8,
    pub repeat_decay: u8,
}

pub const TICKS_PER_STEP: u64 = 16;

/// Pitch used by the merged step sequencer (C5).
pub const STEP_SEQUENCER_PITCH: u8 = 72;

impl PianoNote {
    pub fn start_tick(self) -> u64 {
        self.start_step
            .saturating_mul(TICKS_PER_STEP)
            .saturating_add(u64::from(self.start_substep.min(15)))
    }

    pub fn duration_ticks(self) -> u64 {
        self.duration_steps
            .saturating_mul(TICKS_PER_STEP)
            .saturating_add(u64::from(self.duration_substeps.min(15)))
            .max(1)
    }

    pub fn set_start_tick(&mut self, tick: u64) {
        self.start_step = tick / 16;
        self.start_substep = (tick % TICKS_PER_STEP) as u8;
    }

    pub fn set_duration_ticks(&mut self, ticks: u64) {
        let ticks = ticks.max(1);
        self.duration_steps = ticks / 16;
        self.duration_substeps = (ticks % 16) as u8;
    }

    /// Offsets, in ticks from the note start, at which this note re-triggers.
    /// The first entry is always the note itself.
    pub fn trigger_offsets(self) -> Vec<(u64, u8)> {
        let spacing = if self.repeat_ticks == 0 {
            (self.duration_ticks() / 2).max(1)
        } else {
            u64::from(self.repeat_ticks)
        };
        let mut offsets = Vec::with_capacity(usize::from(self.repeat_count) + 1);
        offsets.push((0, self.velocity.max(1)));
        let mut velocity = self.velocity.max(1);
        for repeat in 1..=u64::from(self.repeat_count) {
            velocity = velocity.saturating_sub(self.repeat_decay).max(1);
            offsets.push((repeat.saturating_mul(spacing), velocity));
        }
        offsets
    }
}
/// Note order used by the arpeggiator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArpMode {
    #[default]
    Up,
    Down,
    UpDown,
    DownUp,
    Random,
    Order,
}

impl ArpMode {
    pub const ALL: [Self; 6] = [
        Self::Up,
        Self::Down,
        Self::UpDown,
        Self::DownUp,
        Self::Random,
        Self::Order,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::UpDown => "Up / Down",
            Self::DownUp => "Down / Up",
            Self::Random => "Random",
            Self::Order => "As played",
        }
    }

    /// Mode for an integer index, clamped to the available range.
    pub fn from_index(index: u32) -> Self {
        Self::ALL[(index as usize).min(Self::ALL.len() - 1)]
    }

    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0) as u32
    }

    /// Reorders `pitches` (already sorted low to high) for this mode.
    pub fn order(self, pitches: &[u8]) -> Vec<u8> {
        let mut values = pitches.to_vec();
        match self {
            Self::Up | Self::Order => values,
            Self::Down => {
                values.reverse();
                values
            }
            Self::UpDown => {
                let mut out = values.clone();
                let mut tail = values;
                tail.pop();
                tail.remove(0);
                tail.reverse();
                out.extend(tail);
                out
            }
            Self::DownUp => {
                values.reverse();
                let mut out = values.clone();
                let mut tail = values;
                tail.pop();
                tail.remove(0);
                tail.reverse();
                out.extend(tail);
                out
            }
            Self::Random => {
                // Deterministic shuffle keeps renders reproducible.
                let len = values.len();
                for index in 0..len {
                    values.swap(index, (index * 7 + len) % len.max(1));
                }
                values
            }
        }
    }
}

/// Per-instrument arpeggiator. Rate is either musical (steps) or free (ms).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Arpeggio {
    pub enabled: bool,
    pub mode: ArpMode,
    pub rate: TimeValue,
    /// Note length as a fraction of the arp step.
    pub gate: f32,
    /// Octaves added above the played range.
    pub octaves: u8,
}

impl Default for Arpeggio {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ArpMode::Up,
            rate: TimeValue {
                amount: 1.0,
                unit: TimeUnit::Steps,
            },
            gate: 0.6,
            octaves: 1,
        }
    }
}

/// Routing and voice options shown on a plug-in's settings tab.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    pub midi_in_channel: u8,
    pub midi_out_channel: u8,
    pub audio_in: u8,
    pub audio_out: u8,
    pub voice_count: u8,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            midi_in_channel: 0,
            midi_out_channel: 0,
            audio_in: 0,
            audio_out: 0,
            voice_count: 16,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreeBandEq {
    pub low_db: f32,
    pub mid_db: f32,
    pub high_db: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynthKind {
    /// Instrument backed by a discovered external plug-in bundle. The built-in
    /// engine has no hosting backend, so this produces no built-in voice; the UI
    /// shows a wrapper window for the plug-in instead.
    External,
    #[default]
    Sine,
    SamplePlayer,
    SignalGenerator,
    Ms20,
    JunoPoly,
    M1Workstation,
    FmSynth,
}

impl SynthKind {
    /// Built-in generators offered in the picker, in order.
    pub const BUILTIN: [Self; 7] = [
        Self::SamplePlayer,
        Self::Sine,
        Self::SignalGenerator,
        Self::Ms20,
        Self::JunoPoly,
        Self::M1Workstation,
        Self::FmSynth,
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultSample {
    #[default]
    None,
    Kick,
    Snare,
    HiHat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalWaveform {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
}

impl SignalWaveform {
    /// Every waveform, in the order the integer-automation index uses.
    pub const ALL: [Self; 4] = [Self::Sine, Self::Triangle, Self::Saw, Self::Square];

    pub fn from_index(index: u32) -> Self {
        Self::ALL[(index as usize).min(Self::ALL.len() - 1)]
    }

    pub fn index(self) -> u32 {
        Self::ALL
            .iter()
            .position(|waveform| *waveform == self)
            .unwrap_or(0) as u32
    }
}

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
