//! The project data model: values, targets and the structures they fill.

#![allow(unused_imports)]

use serde::{Deserialize, Serialize};

pub const INITIAL_LANES: usize = 4;
pub const INITIAL_PATTERN_STEPS: u64 = 64;
pub const MIDI_CHANNEL_COUNT: usize = 16;
pub const MAX_SEND_GAIN_SUM: f32 = 0.95;

/// Insert slots every mixer channel (and the master) provides.
pub const INSERT_SLOTS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeSignature {
    pub numerator: u16,
    pub denominator: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    Steps,
    #[default]
    Milliseconds,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeValue {
    pub amount: f32,
    pub unit: TimeUnit,
}

impl Default for TimeValue {
    fn default() -> Self {
        Self {
            amount: 250.0,
            unit: TimeUnit::Milliseconds,
        }
    }
}

impl TimeValue {
    pub fn seconds(self, bpm: f32) -> f32 {
        match self.unit {
            TimeUnit::Steps => self.amount.max(0.0) * 60.0 / bpm.max(1.0) / 4.0,
            TimeUnit::Milliseconds => self.amount.max(0.0) / 1000.0,
        }
    }
}

pub fn perlin_noise(seed: u32, coordinate: f64) -> f32 {
    perlin_noise_octaves(seed, coordinate, 1)
}

pub fn perlin_noise_octaves(seed: u32, coordinate: f64, octaves: u32) -> f32 {
    let octaves = octaves.clamp(1, 16);
    let mut total = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut max_amplitude = 0.0;
    for _ in 0..octaves {
        let lattice = (coordinate * frequency).floor() as i64;
        let fraction = ((coordinate * frequency) - lattice as f64) as f32;
        let fade = fraction * fraction * fraction * (fraction * (fraction * 6.0 - 15.0) + 10.0);
        let gradient = |index: i64, seed: u32| {
            let mut hash = (index as u64).wrapping_add(u64::from(seed));
            hash ^= hash >> 30;
            hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            hash ^= hash >> 27;
            hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
            hash ^= hash >> 31;
            if hash & 1 == 0 { 1.0_f32 } else { -1.0_f32 }
        };
        let left = gradient(lattice, seed) * fraction;
        let right = gradient(lattice.saturating_add(1), seed) * (fraction - 1.0);
        let value = left + (right - left) * fade;
        total += value * amplitude;
        max_amplitude += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    (total / max_amplitude).clamp(-1.0, 1.0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    SampleAndHold,
}

impl LfoShape {
    /// Every shape, in the order the integer-automation index uses.
    pub const ALL: [Self; 5] = [
        Self::Sine,
        Self::Triangle,
        Self::Saw,
        Self::Square,
        Self::SampleAndHold,
    ];

    /// Shape for an integer index, clamped to the available range.
    pub fn from_index(index: u32) -> Self {
        Self::ALL[(index as usize).min(Self::ALL.len() - 1)]
    }

    /// Position of this shape in [`Self::ALL`].
    pub fn index(self) -> u32 {
        Self::ALL
            .iter()
            .position(|shape| *shape == self)
            .unwrap_or(0) as u32
    }
}

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
        Self::ALL
            .iter()
            .position(|mode| *mode == self)
            .unwrap_or(0) as u32
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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationPoint {
    pub step: u64,
    pub value: f32,
    pub curve: AutomationCurve,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationTrack {
    pub name: String,
    pub target: ParameterTarget,
    pub start_step: u64,
    pub length_steps: u64,
    pub points: Vec<AutomationPoint>,
    pub modulation_points: Vec<AutomationPoint>,
    pub enabled: bool,
    pub source: AutomationSource,
}

impl Default for AutomationTrack {
    fn default() -> Self {
        Self {
            name: "Automation".to_owned(),
            target: ParameterTarget::Instrument {
                instrument_id: 0,
                parameter: InstrumentParameter::Gain,
            },
            start_step: 0,
            length_steps: 64,
            points: Vec::new(),
            modulation_points: Vec::new(),
            enabled: true,
            source: AutomationSource::Curve,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct InsertFxSlot {
    pub name: String,
    pub enabled: bool,
    /// Path of a discovered VST/VST3/CLAP bundle, empty for built-in effects.
    pub plugin_path: String,
    /// Locked inserts keep their parameters but ignore edits.
    pub locked: bool,
    /// A soloed insert mutes the other inserts on the same channel, so one
    /// processor can be auditioned on its own.
    pub solo: bool,
    /// Opaque plug-in state captured on save and restored on load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugin_state: Vec<u8>,
    pub mix: f32,
    pub amount: f32,
    pub eq: ThreeBandEq,
    pub time_seconds: f32,
    pub time_unit: TimeUnit,
    pub time_steps: f32,
    pub feedback: f32,
    pub rate_hz: f32,
    pub depth: f32,
    pub room_size: f32,
    pub drive: f32,
}

impl Default for InsertFxSlot {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: false,
            plugin_path: String::new(),
            locked: false,
            solo: false,
            plugin_state: Vec::new(),
            mix: 1.0,
            amount: 0.35,
            eq: ThreeBandEq::default(),
            time_seconds: 0.32,
            time_unit: TimeUnit::Milliseconds,
            time_steps: 1.0,
            feedback: 0.35,
            rate_hz: 0.8,
            depth: 0.5,
            room_size: 0.55,
            drive: 0.5,
        }
    }
}

impl InsertFxSlot {
    pub fn time_amount(&self) -> f32 {
        match self.time_unit {
            TimeUnit::Steps => self.time_steps,
            TimeUnit::Milliseconds => self.time_seconds * 1000.0,
        }
    }

    pub fn set_time_amount(&mut self, amount: f32) {
        match self.time_unit {
            TimeUnit::Steps => self.time_steps = amount,
            TimeUnit::Milliseconds => self.time_seconds = amount / 1000.0,
        }
    }

    pub fn time_seconds_at_tempo(&self, bpm: f32) -> f32 {
        TimeValue {
            amount: self.time_amount(),
            unit: self.time_unit,
        }
        .seconds(bpm)
        .clamp(0.01, 2.0)
    }

    /// Snapshot of the automatable values as the plug-in layer expects them.
    pub fn effect_parameters(&self) -> crate::plugins::EffectParameters {
        crate::plugins::EffectParameters {
            mix: self.mix,
            amount: self.amount,
            low_db: self.eq.low_db,
            mid_db: self.eq.mid_db,
            high_db: self.eq.high_db,
            time_seconds: self.time_seconds,
            feedback: self.feedback,
            rate_hz: self.rate_hz,
            depth: self.depth,
            room_size: self.room_size,
            drive: self.drive,
        }
    }

    /// True when this slot hosts a discovered external plug-in bundle.
    pub fn is_external(&self) -> bool {
        !self.plugin_path.trim().is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Instrument {
    pub name: String,
    /// Path of a discovered external generator bundle, empty for built-ins.
    pub plugin_path: String,
    pub sample_path: String,
    pub default_sample: DefaultSample,
    pub synth_enabled: bool,
    pub synth_kind: SynthKind,
    pub signal_waveform: SignalWaveform,
    pub synth_note: u8,
    pub ms20_cutoff_hz: f32,
    pub ms20_resonance: f32,
    pub juno_cutoff_hz: f32,
    pub juno_resonance: f32,
    pub juno_pulse_width: f32,
    pub juno_sub_level: f32,
    pub m1_tone: f32,
    pub m1_decay: f32,
    pub m1_decay_unit: TimeUnit,
    pub m1_decay_steps: f32,
    pub fm_ratio: f32,
    pub fm_index: f32,
    pub gain: f32,
    pub pan: f32,
    pub default_mixer_lane: usize,
    /// Power switch: a disabled instrument stays loaded but makes no sound.
    pub enabled: bool,
    /// Locked instruments ignore edits from the UI.
    pub locked: bool,
    /// Soloed instruments mute every instrument that is not soloed.
    pub solo: bool,
    pub voices: VoiceSettings,
    pub arpeggio: Arpeggio,
    /// Optional custom channel colour.
    pub color: Option<[u8; 3]>,
    /// Opaque plug-in state captured on save and restored on load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugin_state: Vec<u8>,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            name: "Instrument".to_owned(),
            plugin_path: String::new(),
            sample_path: String::new(),
            default_sample: DefaultSample::None,
            synth_enabled: false,
            synth_kind: SynthKind::Sine,
            signal_waveform: SignalWaveform::Sine,
            synth_note: 60,
            ms20_cutoff_hz: 1_800.0,
            ms20_resonance: 0.2,
            juno_cutoff_hz: 2_400.0,
            juno_resonance: 0.15,
            juno_pulse_width: 0.5,
            juno_sub_level: 0.3,
            m1_tone: 0.55,
            m1_decay: 1.2,
            m1_decay_unit: TimeUnit::Milliseconds,
            m1_decay_steps: 4.0,
            fm_ratio: 2.0,
            fm_index: 0.35,
            gain: 0.8,
            pan: 0.0,
            default_mixer_lane: 0,
            enabled: true,
            locked: false,
            solo: false,
            voices: VoiceSettings::default(),
            arpeggio: Arpeggio::default(),
            color: None,
            plugin_state: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Pattern {
    pub name: String,
    pub length_steps: u64,
    pub step_hits: Vec<StepHit>,
    pub notes: Vec<PianoNote>,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            name: "Pattern 1".to_owned(),
            length_steps: INITIAL_PATTERN_STEPS,
            step_hits: Vec::new(),
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClipSource {
    Pattern { pattern_id: usize },
    Audio { path: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrangementClip {
    pub start_step: u64,
    pub length_steps: u64,
    /// Source material skipped from the clip start, in steps. Trimming or
    /// slicing a clip advances this so audio keeps its position.
    pub offset_steps: u64,
    pub source: ClipSource,
    /// Per-clip sample playback controls, mixed with the project-wide ones.
    pub sample_gain: f32,
    /// Pan in -1..=1.
    pub sample_pan: f32,
    /// Transpose in semitones.
    pub sample_pitch: f32,
}

impl Default for ArrangementClip {
    fn default() -> Self {
        Self {
            start_step: 0,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Pattern { pattern_id: 0 },
            sample_gain: 0.8,
            sample_pan: 0.0,
            sample_pitch: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrangementLane {
    pub name: String,
    pub pinned: bool,
    /// Power switch: a disabled track is silent and its inserts are bypassed.
    pub enabled: bool,
    /// Locked tracks ignore edits such as clip dragging.
    pub locked: bool,
    /// Soloed tracks mute every track that is not soloed.
    pub solo: bool,
    /// Row height in points. Tracks can be resized vertically.
    pub height: f32,
    /// Hides this track's automation rows in the arrangement.
    pub automation_collapsed: bool,
    pub gain: f32,
    pub pan: f32,
    /// Stereo width: 0 = mono, 1 = unchanged, 2 = double side signal.
    pub width: f32,
    pub eq: ThreeBandEq,
    pub inserts: Vec<InsertFxSlot>,
    /// Skips every insert on this channel without touching their enable states,
    /// so the chain can be compared dry against wet.
    pub fx_bypass: bool,
    pub clips: Vec<ArrangementClip>,
}

impl Default for ArrangementLane {
    fn default() -> Self {
        Self {
            name: "Track".to_owned(),
            pinned: false,
            enabled: true,
            locked: false,
            solo: false,
            height: 54.0,
            automation_collapsed: false,
            gain: 0.8,
            pan: 0.0,
            width: 1.0,
            eq: ThreeBandEq::default(),
            inserts: vec![InsertFxSlot::default(); INSERT_SLOTS],
            fx_bypass: false,
            clips: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterChannel {
    pub gain: f32,
    pub pan: f32,
    pub eq: ThreeBandEq,
    pub inserts: Vec<InsertFxSlot>,
}

impl Default for MasterChannel {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            eq: ThreeBandEq::default(),
            inserts: vec![InsertFxSlot::default(); INSERT_SLOTS],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub bpm: f32,
    pub snap_ticks: u64,
    pub time_signature: TimeSignature,
    pub instruments: Vec<Instrument>,
    pub patterns: Vec<Pattern>,
    pub lanes: Vec<ArrangementLane>,
    pub master: MasterChannel,
    pub mixer_collapsed: bool,
    pub automation_tracks: Vec<AutomationTrack>,
    pub sends: Vec<Vec<f32>>,
    pub midi_routes: [usize; MIDI_CHANNEL_COUNT],
    /// Defaults applied to every audio clip: gain, pan (-1..=1) and transpose
    /// in semitones. Each clip multiplies its own values on top of these.
    pub sample_gain: f32,
    pub sample_pan: f32,
    pub sample_pitch: f32,
}

impl Default for Project {
    fn default() -> Self {
        let names = ["Kick", "Snare", "Hat", "Synth"];
        let mut instruments: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(index, name)| Instrument {
                name: (*name).to_owned(),
                synth_enabled: index == 3,
                synth_note: [36, 38, 42, 48][index],
                default_sample: match index {
                    0 => DefaultSample::Kick,
                    1 => DefaultSample::Snare,
                    2 => DefaultSample::HiHat,
                    _ => DefaultSample::None,
                },
                synth_kind: if index < 3 {
                    SynthKind::SamplePlayer
                } else {
                    SynthKind::Sine
                },
                default_mixer_lane: index,
                ..Instrument::default()
            })
            .collect();
        let mut pattern = Pattern::default();
        for step in (0..pattern.length_steps).step_by(4) {
            pattern.step_hits.push(StepHit {
                instrument_id: 0,
                step,
            });
        }
        for step in [4, 12, 20, 28, 36, 44, 52, 60] {
            pattern.step_hits.push(StepHit {
                instrument_id: 1,
                step,
            });
        }
        for step in (0..pattern.length_steps).step_by(2) {
            pattern.step_hits.push(StepHit {
                instrument_id: 2,
                step,
            });
        }
        for (start_step, pitch) in [(0, 60), (16, 64), (32, 67), (48, 72)] {
            pattern.notes.push(PianoNote {
                instrument_id: 3,
                start_step,
                start_substep: 0,
                duration_steps: 12,
                duration_substeps: 0,
                pitch,
                velocity: 100,
                ..PianoNote::default()
            });
        }

        let lanes: Vec<_> = (0..INITIAL_LANES)
            .map(|index| {
                let mut lane = ArrangementLane {
                    name: format!("Track {:02}", index + 1),
                    ..ArrangementLane::default()
                };
                if index == 0 {
                    lane.clips.push(ArrangementClip {
                        start_step: 0,
                        length_steps: pattern.length_steps,
                        source: ClipSource::Pattern { pattern_id: 0 },
                        offset_steps: 0,
                        ..ArrangementClip::default()
                    });
                }
                lane
            })
            .collect();
        let lane_count = lanes.len();
        let channel_count = lane_count + 1;
        let mut sends = vec![vec![0.0; channel_count]; channel_count];
        for lane in sends.iter_mut().take(lane_count) {
            lane[lane_count] = 0.25;
        }
        instruments.shrink_to_fit();
        let mut project = Self {
            bpm: 120.0,
            snap_ticks: 16,
            time_signature: TimeSignature::default(),
            instruments,
            patterns: vec![pattern],
            lanes,
            master: MasterChannel::default(),
            mixer_collapsed: false,
            automation_tracks: Vec::new(),
            sends,
            midi_routes: [0; MIDI_CHANNEL_COUNT],
            sample_gain: 0.8,
            sample_pan: 0.0,
            sample_pitch: 0.0,
        };
        project.normalize();
        project
    }
}

