use serde::{Deserialize, Serialize};

pub const INITIAL_LANES: usize = 4;
pub const INITIAL_PATTERN_STEPS: u64 = 64;
pub const MIDI_CHANNEL_COUNT: usize = 16;
pub const MAX_SEND_GAIN_SUM: f32 = 0.95;

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
}

impl MixerParameter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gain => "Volume",
            Self::Pan => "Pan",
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
    pub eq: ThreeBandEq,
    pub inserts: Vec<InsertFxSlot>,
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
            eq: ThreeBandEq::default(),
            inserts: vec![InsertFxSlot::default(); 3],
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
            inserts: vec![InsertFxSlot::default(); 3],
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

impl Project {
    /// Clamps the audio-clip playback controls to their documented ranges.
    fn normalize_sample_controls(&mut self) {
        self.sample_gain = finite_clamp(self.sample_gain, 0.0, 1.0, 0.8);
        self.sample_pan = finite_clamp(self.sample_pan, -1.0, 1.0, 0.0);
        self.sample_pitch = finite_clamp(self.sample_pitch, -24.0, 24.0, 0.0);
        for lane in &mut self.lanes {
            for clip in &mut lane.clips {
                clip.sample_gain = finite_clamp(clip.sample_gain, 0.0, 1.0, 0.8);
                clip.sample_pan = finite_clamp(clip.sample_pan, -1.0, 1.0, 0.0);
                clip.sample_pitch = finite_clamp(clip.sample_pitch, -24.0, 24.0, 0.0);
            }
        }
    }

    pub fn normalize(&mut self) {
        self.normalize_sample_controls();
        self.bpm = finite_clamp(self.bpm, 20.0, 300.0, 120.0);
        self.snap_ticks = self.snap_ticks.clamp(1, 1024);
        self.time_signature.numerator = self.time_signature.numerator.clamp(1, 128);
        self.time_signature.denominator = self.time_signature.denominator.clamp(1, 128);

        if self.patterns.is_empty() {
            self.patterns.push(Pattern::default());
        }

        if self.lanes.is_empty() {
            self.lanes.push(ArrangementLane::default());
        }
        if self.instruments.is_empty() {
            self.instruments.push(Instrument::default());
        }

        for instrument in &mut self.instruments {
            instrument.gain = finite_clamp(instrument.gain, 0.0, 1.0, 0.8);
            instrument.pan = finite_clamp(instrument.pan, -1.0, 1.0, 0.0);
            instrument.synth_note = instrument.synth_note.min(127);
            instrument.ms20_cutoff_hz =
                finite_clamp(instrument.ms20_cutoff_hz, 40.0, 16_000.0, 1_800.0);
            instrument.ms20_resonance = finite_clamp(instrument.ms20_resonance, 0.0, 0.9, 0.2);
            instrument.juno_cutoff_hz =
                finite_clamp(instrument.juno_cutoff_hz, 40.0, 16_000.0, 2_400.0);
            instrument.juno_resonance = finite_clamp(instrument.juno_resonance, 0.0, 0.9, 0.15);
            instrument.juno_pulse_width =
                finite_clamp(instrument.juno_pulse_width, 0.05, 0.95, 0.5);
            instrument.juno_sub_level = finite_clamp(instrument.juno_sub_level, 0.0, 1.0, 0.3);
            instrument.m1_tone = finite_clamp(instrument.m1_tone, 0.0, 1.0, 0.55);
            instrument.m1_decay = finite_clamp(instrument.m1_decay, 0.05, 4.0, 1.2);
            instrument.m1_decay_steps = finite_clamp(instrument.m1_decay_steps, 0.01, 256.0, 4.0);
            instrument.fm_ratio = finite_clamp(instrument.fm_ratio, 0.25, 16.0, 2.0);
            instrument.fm_index = finite_clamp(instrument.fm_index, 0.0, 1.0, 0.35);
            instrument.default_mixer_lane = instrument.default_mixer_lane.min(self.lanes.len() - 1);
            instrument.voices.voice_count = instrument.voices.voice_count.clamp(1, 64);
            instrument.voices.midi_in_channel = instrument.voices.midi_in_channel.min(15);
            instrument.voices.midi_out_channel = instrument.voices.midi_out_channel.min(15);
            instrument.voices.audio_in = instrument.voices.audio_in.min(15);
            instrument.voices.audio_out = instrument.voices.audio_out.min(15);
            instrument.arpeggio.gate = finite_clamp(instrument.arpeggio.gate, 0.05, 1.0, 0.6);
            instrument.arpeggio.octaves = instrument.arpeggio.octaves.clamp(1, 4);
            instrument.arpeggio.rate.amount =
                finite_clamp(instrument.arpeggio.rate.amount, 0.01, 64.0, 1.0);
        }
        for pattern in &mut self.patterns {
            pattern.length_steps = pattern.length_steps.max(1);
            pattern
                .step_hits
                .retain(|hit| hit.instrument_id < self.instruments.len());
            pattern
                .step_hits
                .sort_by_key(|hit| (hit.step, hit.instrument_id));
            pattern
                .notes
                .retain(|note| note.instrument_id < self.instruments.len());
            for note in &mut pattern.notes {
                note.start_substep = note.start_substep.min(15);
                note.duration_substeps = note.duration_substeps.min(15);
                note.duration_steps = note.duration_steps.max(1);
                note.pitch = note.pitch.min(127);
                note.velocity = note.velocity.clamp(1, 127);
                note.repeat_count = note.repeat_count.min(32);
                note.repeat_ticks = note.repeat_ticks.min(64);
                note.repeat_decay = note.repeat_decay.min(126);
            }
            pattern
                .notes
                .sort_by_key(|note| (note.start_step, note.pitch, note.instrument_id));
        }
        for lane in &mut self.lanes {
            lane.height = finite_clamp(lane.height, 34.0, 260.0, 54.0);
            lane.gain = finite_clamp(lane.gain, 0.0, 1.0, 0.8);
            lane.pan = finite_clamp(lane.pan, -1.0, 1.0, 0.0);
            Self::normalize_eq(&mut lane.eq);
            lane.inserts.resize_with(3, InsertFxSlot::default);
            lane.inserts.truncate(3);
            Self::normalize_inserts(&mut lane.inserts);
            lane.clips.retain_mut(|clip| {
                clip.length_steps = clip.length_steps.max(1);
                match &mut clip.source {
                    ClipSource::Pattern { pattern_id } => *pattern_id < self.patterns.len(),
                    ClipSource::Audio { path } => !path.trim().is_empty(),
                }
            });
            lane.clips.sort_by_key(|clip| clip.start_step);
        }

        let lane_count = self.lanes.len();
        let channel_count = lane_count + 1;
        let has_master_routes =
            self.sends.len() > lane_count || self.sends.iter().any(|row| row.len() > lane_count);
        self.sends.resize(channel_count, Vec::new());
        for row in &mut self.sends {
            row.resize(channel_count, 0.0);
            for gain in row {
                *gain = finite_clamp(*gain, 0.0, 0.95, 0.0);
            }
        }
        if !has_master_routes {
            for row in self.sends.iter_mut().take(lane_count) {
                row[lane_count] = 0.25;
            }
        }
        self.master.gain = finite_clamp(self.master.gain, 0.0, 1.0, 1.0);
        self.master.pan = finite_clamp(self.master.pan, -1.0, 1.0, 0.0);
        Self::normalize_eq(&mut self.master.eq);
        self.master.inserts.resize_with(3, InsertFxSlot::default);
        self.master.inserts.truncate(3);
        Self::normalize_inserts(&mut self.master.inserts);
        let totals: Vec<f32> = (0..channel_count)
            .map(|destination| self.sends.iter().map(|row| row[destination].abs()).sum())
            .collect();
        for row in &mut self.sends {
            for (destination, gain) in row.iter_mut().enumerate() {
                if totals[destination] > MAX_SEND_GAIN_SUM {
                    *gain *= MAX_SEND_GAIN_SUM / totals[destination];
                }
            }
        }
        for route in &mut self.midi_routes {
            *route = (*route).min(self.instruments.len() - 1);
        }
        self.automation_tracks.retain_mut(|track| {
            track.length_steps = track.length_steps.max(1);
            let target_is_valid = match track.target {
                ParameterTarget::Instrument { instrument_id, .. } => {
                    instrument_id < self.instruments.len()
                }
                ParameterTarget::InsertEffect {
                    channel_id,
                    slot_id,
                    ..
                } => {
                    let slots = if channel_id == lane_count {
                        &self.master.inserts
                    } else if let Some(lane) = self.lanes.get(channel_id) {
                        &lane.inserts
                    } else {
                        return false;
                    };
                    slot_id < slots.len() && !slots[slot_id].name.is_empty()
                }
                // Hosted parameters and global targets stay valid as long as
                // the plug-in/engine is alive; nothing to prune here.
                ParameterTarget::PluginParameter { .. } => true,
                ParameterTarget::MixerChannel { channel_id, .. } => channel_id <= lane_count,
                ParameterTarget::Tempo => true,
            };
            if !target_is_valid {
                return false;
            }
            for point in &mut track.points {
                point.value = finite_clamp(point.value, 0.0, 1.0, 0.0);
            }
            for point in &mut track.modulation_points {
                point.value = finite_clamp(point.value, 0.0, 1.0, 1.0);
            }
            track.points.sort_by_key(|point| point.step);
            track.modulation_points.sort_by_key(|point| point.step);
            true
        });
    }

    pub fn has_automation(&self, target: ParameterTarget) -> bool {
        self.automation_tracks
            .iter()
            .any(|track| track.enabled && track.target == target)
    }

    pub fn automated_value(&self, target: ParameterTarget, base_value: f32, step: f64) -> f32 {
        let position = step.max(0.0);
        let mut normalized = None;
        for track in &self.automation_tracks {
            if !track.enabled
                || track.target != target
                || !matches!(track.source, AutomationSource::Curve)
                || track.points.is_empty()
                || position < track.start_step as f64
                || position >= track.start_step.saturating_add(track.length_steps) as f64
            {
                continue;
            }
            let value = Self::evaluate_curve(&track.points, position);
            if let Some(sum) = &mut normalized {
                *sum += value - track.points[0].value;
            } else {
                normalized = Some(value);
            }
        }
        normalized.map_or(base_value, |value| {
            target.denormalize_value(value.clamp(0.0, 1.0))
        })
    }

    pub fn evaluate_curve(points: &[AutomationPoint], position: f64) -> f32 {
        fn cubic_bezier(t: f32, control_1: f32, control_2: f32) -> f32 {
            let inverse = 1.0 - t;
            3.0 * inverse * inverse * t * control_1 + 3.0 * inverse * t * t * control_2 + t * t * t
        }

        match points {
            [] => 0.0,
            [only] => only.value,
            points if position <= points[0].step as f64 => points[0].value,
            points if position >= points[points.len() - 1].step as f64 => {
                points[points.len() - 1].value
            }
            points => {
                let upper = points.partition_point(|point| point.step as f64 <= position);
                let left = points[upper - 1];
                let right = points[upper];
                let distance = right.step.saturating_sub(left.step).max(1) as f32;
                let fraction = ((position - left.step as f64) as f32 / distance).clamp(0.0, 1.0);
                let shaped = match right.curve {
                    AutomationCurve::Linear => fraction,
                    AutomationCurve::Smooth => fraction * fraction * (3.0 - 2.0 * fraction),
                    AutomationCurve::Bezier => {
                        let (mut low, mut high) = (0.0_f32, 1.0_f32);
                        for _ in 0..12 {
                            let t = (low + high) * 0.5;
                            let x = cubic_bezier(t, 0.25, 0.25);
                            if x < fraction {
                                low = t;
                            } else {
                                high = t;
                            }
                        }
                        cubic_bezier((low + high) * 0.5, 0.1, 1.0)
                    }
                    AutomationCurve::Curved => {
                        (1.0 - (std::f32::consts::PI * fraction).cos()) * 0.5
                    }
                    AutomationCurve::Pulse => {
                        if fraction < 0.5 {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    AutomationCurve::Constant => 0.0,
                    AutomationCurve::Steps => (fraction * 4.0).floor() / 4.0,
                };
                left.value + (right.value - left.value) * shaped
            }
        }
    }

    pub fn parameter_value(&self, target: ParameterTarget) -> Option<f32> {
        match target {
            // Hosted values are injected by the UI layer, which owns the
            // plug-in instance; the engine keeps the automation curve.
            ParameterTarget::PluginParameter { .. } => None,
            ParameterTarget::MixerChannel {
                channel_id,
                parameter,
            } => {
                let (gain, pan) = if channel_id == self.lanes.len() {
                    (self.master.gain, self.master.pan)
                } else {
                    let lane = self.lanes.get(channel_id)?;
                    (lane.gain, lane.pan)
                };
                Some(match parameter {
                    MixerParameter::Gain => gain,
                    MixerParameter::Pan => pan,
                })
            }
            ParameterTarget::Tempo => Some(self.bpm),
            ParameterTarget::Instrument {
                instrument_id,
                parameter,
            } => {
                let instrument = self.instruments.get(instrument_id)?;
                Some(match parameter {
                    InstrumentParameter::Gain => instrument.gain,
                    InstrumentParameter::Pan => instrument.pan,
                    InstrumentParameter::CutoffHz => match instrument.synth_kind {
                        SynthKind::Ms20 => instrument.ms20_cutoff_hz,
                        SynthKind::JunoPoly => instrument.juno_cutoff_hz,
                        _ => return None,
                    },
                    InstrumentParameter::Resonance => match instrument.synth_kind {
                        SynthKind::Ms20 => instrument.ms20_resonance,
                        SynthKind::JunoPoly => instrument.juno_resonance,
                        _ => return None,
                    },
                    InstrumentParameter::PulseWidth => instrument.juno_pulse_width,
                    InstrumentParameter::SubLevel => instrument.juno_sub_level,
                    InstrumentParameter::Tone => instrument.m1_tone,
                    InstrumentParameter::Decay => TimeValue {
                        amount: match instrument.m1_decay_unit {
                            TimeUnit::Steps => instrument.m1_decay_steps,
                            TimeUnit::Milliseconds => instrument.m1_decay * 1_000.0,
                        },
                        unit: instrument.m1_decay_unit,
                    }
                    .seconds(self.bpm),
                    InstrumentParameter::FmRatio => instrument.fm_ratio,
                    InstrumentParameter::FmIndex => instrument.fm_index,
                })
            }
            ParameterTarget::InsertEffect {
                channel_id,
                slot_id,
                parameter,
            } => {
                let slots = if channel_id == self.lanes.len() {
                    &self.master.inserts
                } else {
                    &self.lanes.get(channel_id)?.inserts
                };
                let slot = slots.get(slot_id)?;
                Some(match parameter {
                    EffectParameter::Mix => slot.mix,
                    EffectParameter::Amount => slot.amount,
                    EffectParameter::LowDb => slot.eq.low_db,
                    EffectParameter::MidDb => slot.eq.mid_db,
                    EffectParameter::HighDb => slot.eq.high_db,
                    EffectParameter::TimeSeconds => slot.time_seconds_at_tempo(self.bpm),
                    EffectParameter::Feedback => slot.feedback,
                    EffectParameter::RateHz => slot.rate_hz,
                    EffectParameter::Depth => slot.depth,
                    EffectParameter::RoomSize => slot.room_size,
                    EffectParameter::Drive => slot.drive,
                })
            }
        }
    }

    pub fn add_lane(&mut self) {
        let old_lane_count = self.lanes.len();
        let new_len = old_lane_count + 1;
        for track in &mut self.automation_tracks {
            if let ParameterTarget::InsertEffect { channel_id, .. } = &mut track.target {
                if *channel_id == old_lane_count {
                    *channel_id = new_len;
                }
            }
        }
        self.lanes.push(ArrangementLane {
            name: format!("Track {new_len:02}"),
            ..ArrangementLane::default()
        });
        for row in &mut self.sends {
            row.insert(old_lane_count, 0.0);
        }
        self.sends.insert(old_lane_count, vec![0.0; new_len + 1]);
        self.sends[old_lane_count][new_len] = 0.25;
        self.normalize();
    }

    /// Moves track `from` to position `to`, keeping mixer sends, instrument
    /// routing and insert automation targets pointing at the same channels.
    pub fn move_lane(&mut self, from: usize, to: usize) {
        let lane_count = self.lanes.len();
        if from >= lane_count || to >= lane_count || from == to {
            return;
        }
        let master_index = lane_count;
        let lane = self.lanes.remove(from);
        self.lanes.insert(to, lane);
        // Permute the send matrix the same way, keeping the master row/column
        // last so audio routing still matches the new index order.
        if self.sends.len() == lane_count + 1
            && self.sends.iter().all(|row| row.len() == lane_count + 1)
        {
            let mut rows: Vec<Vec<f32>> = self.sends[..lane_count].to_vec();
            let row = rows.remove(from);
            rows.insert(to, row);
            let mut permuted = vec![vec![0.0; lane_count + 1]; lane_count + 1];
            for (new_row, source_row) in rows.iter().enumerate() {
                let mut values = source_row[..lane_count].to_vec();
                let value = values.remove(from);
                values.insert(to, value);
                permuted[new_row][..lane_count].copy_from_slice(&values);
                // Master column stays the last entry of each lane row.
                permuted[new_row][lane_count] = source_row[master_index];
            }
            // Master row follows the same column permutation.
            let master_row = self.sends[master_index].clone();
            let mut values = master_row[..lane_count].to_vec();
            let value = values.remove(from);
            values.insert(to, value);
            permuted[master_index][..lane_count].copy_from_slice(&values);
            self.sends = permuted;
        }
        let remap = |index: usize| -> usize {
            if index == master_index {
                master_index
            } else if index == from {
                to
            } else if from < to && index > from && index <= to {
                index - 1
            } else if to < from && index >= to && index < from {
                index + 1
            } else {
                index
            }
        };
        for instrument in &mut self.instruments {
            instrument.default_mixer_lane =
                remap(instrument.default_mixer_lane.min(lane_count - 1));
        }
        for track in &mut self.automation_tracks {
            if let ParameterTarget::InsertEffect { channel_id, .. } = &mut track.target {
                *channel_id = remap((*channel_id).min(master_index));
            }
        }
        self.normalize();
    }

    /// Reorders an automation lane so grouped rows can be rearranged.
    pub fn move_automation(&mut self, from: usize, to: usize) {
        if from >= self.automation_tracks.len() || to >= self.automation_tracks.len() || from == to
        {
            return;
        }
        let track = self.automation_tracks.remove(from);
        self.automation_tracks.insert(to, track);
    }

    /// Adds an automation lane, optionally positioned immediately after another
    /// lane so a parameter can carry several adjacent clips.
    pub fn add_automation_track(&mut self, track: AutomationTrack) -> usize {
        self.automation_tracks.push(track);
        self.automation_tracks.len() - 1
    }

    fn normalize_eq(eq: &mut ThreeBandEq) {
        eq.low_db = finite_clamp(eq.low_db, -12.0, 12.0, 0.0);
        eq.mid_db = finite_clamp(eq.mid_db, -12.0, 12.0, 0.0);
        eq.high_db = finite_clamp(eq.high_db, -12.0, 12.0, 0.0);
    }

    fn normalize_inserts(inserts: &mut [InsertFxSlot]) {
        for insert in inserts {
            insert.mix = finite_clamp(insert.mix, 0.0, 1.0, 0.5);
            insert.amount = finite_clamp(insert.amount, 0.0, 1.0, 0.35);
            Self::normalize_eq(&mut insert.eq);
            insert.time_seconds = finite_clamp(insert.time_seconds, 0.01, 1.0, 0.32);
            insert.time_steps = finite_clamp(insert.time_steps, 0.01, 256.0, 1.0);
            insert.feedback = finite_clamp(insert.feedback, 0.0, 1.0, 0.35);
            insert.rate_hz = finite_clamp(insert.rate_hz, 0.05, 8.0, 0.8);
            insert.depth = finite_clamp(insert.depth, 0.0, 1.0, 0.5);
            insert.room_size = finite_clamp(insert.room_size, 0.0, 1.0, 0.55);
            insert.drive = finite_clamp(insert.drive, 0.0, 1.0, 0.5);
            if insert.name.trim().is_empty() {
                insert.enabled = false;
            }
        }
    }

    /// True when some instrument is soloed, so playback should mute the rest.
    pub fn instrument_solo_active(&self) -> bool {
        self.instruments.iter().any(|instrument| instrument.solo)
    }

    /// True when some track is soloed.
    pub fn lane_solo_active(&self) -> bool {
        self.lanes.iter().any(|lane| lane.solo)
    }

    /// Adds or removes the step-sequencer note for `instrument_id` at `step`.
    ///
    /// Step-sequencer edits are stored as ordinary C5 piano-roll notes, so both
    /// editors show and edit the same data.
    pub fn set_step_note(&mut self, instrument_id: usize, step: u64, enabled: bool) {
        self.set_step_note_on_pattern(0, instrument_id, step, enabled);
    }

    /// Step-sequencer edit for a specific pattern.
    pub fn set_step_note_on_pattern(
        &mut self,
        pattern_id: usize,
        instrument_id: usize,
        step: u64,
        enabled: bool,
    ) {
        let pitch = STEP_SEQUENCER_PITCH;
        let Some(pattern) = self.patterns.get_mut(pattern_id) else {
            return;
        };
        let existing = pattern.notes.iter().position(|note| {
            note.instrument_id == instrument_id && note.pitch == pitch && note.start_step == step
        });
        match (enabled, existing) {
            (true, None) => pattern.notes.push(PianoNote {
                instrument_id,
                pitch,
                start_step: step,
                duration_steps: 1,
                velocity: 100,
                ..PianoNote::default()
            }),
            (true, Some(_)) => {}
            (false, Some(index)) => {
                pattern.notes.remove(index);
            }
            (false, None) => {}
        }
        pattern
            .notes
            .sort_by_key(|note| (note.start_step, note.pitch, note.instrument_id));
    }

    /// True when a step-sequencer note exists for this instrument and step.
    pub fn has_step_note(&self, pattern_id: usize, instrument_id: usize, step: u64) -> bool {
        self.patterns.get(pattern_id).is_some_and(|pattern| {
            pattern.notes.iter().any(|note| {
                note.instrument_id == instrument_id
                    && note.pitch == STEP_SEQUENCER_PITCH
                    && note.start_step == step
            })
        })
    }

    pub fn add_pattern(&mut self) -> usize {
        let id = self.patterns.len();
        self.patterns.push(Pattern {
            name: format!("Pattern {}", id + 1),
            ..Pattern::default()
        });
        id
    }

    pub fn duplicate_clip_after(&mut self, lane_id: usize, clip_index: usize) -> Option<usize> {
        let lane = self.lanes.get_mut(lane_id)?;
        let source = lane.clips.get(clip_index)?.clone();
        let start_step = source.start_step.saturating_add(source.length_steps.max(1));
        let insertion_index = lane
            .clips
            .iter()
            .take_while(|clip| clip.start_step <= start_step)
            .count();
        lane.clips.insert(
            insertion_index,
            ArrangementClip {
                start_step,
                length_steps: source.length_steps.max(1),
                source: source.source,
                offset_steps: 0,
                ..ArrangementClip::default()
            },
        );
        Some(insertion_index)
    }

    pub fn make_clip_pattern_unique(&mut self, lane_id: usize, clip_index: usize) -> Option<usize> {
        let pattern_id = match &self.lanes.get(lane_id)?.clips.get(clip_index)?.source {
            ClipSource::Pattern { pattern_id } => *pattern_id,
            ClipSource::Audio { .. } => return None,
        };
        let mut pattern = self.patterns.get(pattern_id)?.clone();
        let unique_id = self.patterns.len();
        pattern.name = format!("{} (unique {})", pattern.name, unique_id + 1);
        self.patterns.push(pattern);
        self.lanes[lane_id].clips[clip_index].source = ClipSource::Pattern {
            pattern_id: unique_id,
        };
        Some(unique_id)
    }
}

pub fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

pub fn normalize_sends(sends: &mut [Vec<f32>]) {
    let lane_count = sends.len();
    for row in sends.iter_mut() {
        row.resize(lane_count, 0.0);
        for gain in row.iter_mut() {
            *gain = finite_clamp(*gain, 0.0, MAX_SEND_GAIN_SUM, 0.0);
        }
    }
    let totals: Vec<f32> = (0..lane_count)
        .map(|destination| sends.iter().map(|row| row[destination].abs()).sum())
        .collect();
    for row in sends {
        for (destination, gain) in row.iter_mut().enumerate() {
            if totals[destination] > MAX_SEND_GAIN_SUM {
                *gain *= MAX_SEND_GAIN_SUM / totals[destination];
            }
        }
    }
}

pub fn disconnect_channel(sends: &mut [Vec<f32>], channel: usize) {
    for row in sends.iter_mut() {
        if let Some(gain) = row.get_mut(channel) {
            *gain = 0.0;
        }
    }
    if let Some(row) = sends.get_mut(channel) {
        row.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_synth_kind_and_plugin_path_round_trip_through_json() {
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::External;
        project.instruments[0].plugin_path = "C:/Plugins/Thing.vst3".to_owned();
        let json = serde_json::to_string(&project).unwrap();
        let loaded: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.instruments[0].synth_kind, SynthKind::External);
        assert_eq!(loaded.instruments[0].plugin_path, "C:/Plugins/Thing.vst3");
    }

    #[test]
    fn builtin_synth_list_excludes_the_external_kind() {
        assert!(!SynthKind::BUILTIN.contains(&SynthKind::External));
        assert_eq!(SynthKind::BUILTIN.len(), 7);
    }

    #[test]
    fn step_sequencer_edits_are_c5_piano_notes() {
        let mut project = Project::default();
        project.set_step_note_on_pattern(0, 0, 4, true);
        assert!(project.has_step_note(0, 0, 4));
        let note = project.patterns[0]
            .notes
            .iter()
            .find(|note| note.instrument_id == 0 && note.start_step == 4)
            .copied()
            .expect("step edit should create a note");
        assert_eq!(note.pitch, STEP_SEQUENCER_PITCH);

        project.set_step_note_on_pattern(0, 0, 4, false);
        assert!(!project.has_step_note(0, 0, 4));
    }

    #[test]
    fn arp_modes_reorder_pitches_predictably() {
        let pitches = [60, 64, 67];
        assert_eq!(ArpMode::Up.order(&pitches), vec![60, 64, 67]);
        assert_eq!(ArpMode::Down.order(&pitches), vec![67, 64, 60]);
        assert_eq!(ArpMode::UpDown.order(&pitches), vec![60, 64, 67, 64]);
        assert_eq!(ArpMode::DownUp.order(&pitches), vec![67, 64, 60, 64]);
        assert_eq!(ArpMode::Order.order(&pitches), vec![60, 64, 67]);
        let mut random = ArpMode::Random.order(&pitches);
        random.sort_unstable();
        assert_eq!(random, vec![60, 64, 67]);
    }

    #[test]
    fn solo_and_power_flags_default_to_audible_and_are_reported() {
        let mut project = Project::default();
        assert!(!project.instrument_solo_active());
        assert!(!project.lane_solo_active());
        assert!(project.instruments[0].enabled);
        assert!(project.lanes[0].enabled);
        project.instruments[0].solo = true;
        project.lanes[1].solo = true;
        assert!(project.instrument_solo_active());
        assert!(project.lane_solo_active());
    }

    #[test]
    fn note_repeats_produce_spaced_triggers_with_falling_velocity() {
        let note = PianoNote {
            velocity: 100,
            repeat_count: 3,
            repeat_ticks: 4,
            repeat_decay: 10,
            duration_steps: 1,
            ..PianoNote::default()
        };
        let offsets = note.trigger_offsets();
        assert_eq!(offsets.len(), 4);
        assert_eq!(offsets[0], (0, 100));
        assert_eq!(offsets[1], (4, 90));
        assert_eq!(offsets[2], (8, 80));
        assert_eq!(offsets[3], (12, 70));
    }

    #[test]
    fn repeats_default_to_half_the_note_length() {
        let note = PianoNote {
            velocity: 64,
            repeat_count: 1,
            repeat_ticks: 0,
            duration_steps: 2,
            ..PianoNote::default()
        };
        let offsets = note.trigger_offsets();
        assert_eq!(offsets.len(), 2);
        assert_eq!(offsets[1].0, note.duration_ticks() / 2);
    }

    #[test]
    fn moving_a_lane_keeps_instrument_and_automation_targets_aligned() {
        let mut project = Project::default();
        project.add_lane();
        project.instruments[0].default_mixer_lane = 0;
        project.lanes[0].inserts[0].name = "Reverb".to_owned();
        project.lanes[0].inserts[0].enabled = true;
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::InsertEffect {
                channel_id: 0,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            },
            ..AutomationTrack::default()
        });
        project.normalize();
        let lane_name = project.lanes[0].name.clone();

        project.move_lane(0, 2);

        assert_eq!(project.lanes[2].name, lane_name);
        assert_eq!(project.instruments[0].default_mixer_lane, 2);
        assert_eq!(
            project.automation_tracks[0].target,
            ParameterTarget::InsertEffect {
                channel_id: 2,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            }
        );
    }

    #[test]
    fn lane_height_and_collapse_survive_normalization() {
        let mut project = Project::default();
        project.lanes[0].height = 5.0;
        project.lanes[0].automation_collapsed = true;
        project.normalize();
        assert!((project.lanes[0].height - 34.0).abs() < f32::EPSILON);
        assert!(project.lanes[0].automation_collapsed);
        project.lanes[0].height = 400.0;
        project.normalize();
        assert!((project.lanes[0].height - 260.0).abs() < f32::EPSILON);
    }

    #[test]
    fn extra_automation_lanes_can_target_the_same_parameter() {
        let mut project = Project::default();
        let track = AutomationTrack {
            start_step: 0,
            length_steps: 16,
            ..AutomationTrack::default()
        };
        let mut second = track.clone();
        second.start_step = 16;
        project.add_automation_track(track);
        project.add_automation_track(second);
        project.normalize();
        assert_eq!(project.automation_tracks.len(), 2);
        assert_eq!(
            project.automation_tracks[0].target,
            project.automation_tracks[1].target
        );
    }

    #[test]
    fn insert_slots_distinguish_builtin_and_external_plugins() {
        let mut slot = InsertFxSlot::default();
        assert!(!slot.is_external());
        slot.plugin_path = "C:/Plugins/Thing.vst3".to_owned();
        assert!(slot.is_external());
        // Effect parameters mirror the automatable slot values.
        slot.mix = 0.25;
        slot.feedback = 0.75;
        let parameters = slot.effect_parameters();
        assert!((parameters.mix - 0.25).abs() < f32::EPSILON);
        assert!((parameters.feedback - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn default_project_separates_instruments_patterns_and_lanes() {
        let project = Project::default();
        assert_eq!(project.instruments.len(), 4);
        assert_eq!(project.patterns.len(), 1);
        assert_eq!(project.lanes.len(), INITIAL_LANES);
        assert_eq!(project.patterns[0].length_steps, 64);
        assert_eq!(project.instruments[0].synth_kind, SynthKind::SamplePlayer);
        assert_eq!(project.instruments[0].default_sample, DefaultSample::Kick);
        assert_eq!(project.instruments[1].default_sample, DefaultSample::Snare);
        assert_eq!(project.instruments[2].default_sample, DefaultSample::HiHat);
    }

    #[test]
    fn signature_accepts_plain_numeric_values_and_computes_bar_length() {
        assert_eq!(
            TimeSignature {
                numerator: 7,
                denominator: 8
            }
            .steps_per_bar(),
            14
        );
        assert_eq!(
            TimeSignature {
                numerator: 3,
                denominator: 4
            }
            .steps_per_bar(),
            12
        );
    }

    #[test]
    fn time_values_convert_between_steps_and_milliseconds() {
        let four_steps = TimeValue {
            amount: 4.0,
            unit: TimeUnit::Steps,
        };
        let half_second = TimeValue {
            amount: 500.0,
            unit: TimeUnit::Milliseconds,
        };
        assert!((four_steps.seconds(120.0) - 0.5).abs() < f32::EPSILON);
        assert!((half_second.seconds(120.0) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn arrangement_clips_and_patterns_have_no_fixed_bar_limit() {
        let mut project = Project::default();
        project.patterns[0].length_steps = 4096;
        project.lanes[0].clips[0].length_steps = 4096;
        project.lanes[0].clips[0].start_step = 8192;
        project.normalize();

        assert_eq!(project.patterns[0].length_steps, 4096);
        assert_eq!(project.lanes[0].clips[0].start_step, 8192);
    }

    #[test]
    fn duplicate_clip_repeats_after_it_and_keeps_pattern_linked() {
        let mut project = Project::default();
        let original = project.lanes[0].clips[0].clone();

        let duplicate_id = project.duplicate_clip_after(0, 0).unwrap();
        let duplicate = &project.lanes[0].clips[duplicate_id];

        assert_eq!(
            duplicate.start_step,
            original.start_step + original.length_steps
        );
        assert_eq!(duplicate.length_steps, original.length_steps);
        assert!(matches!(
            &duplicate.source,
            ClipSource::Pattern { pattern_id: 0 }
        ));
        assert_eq!(project.lanes[0].clips[0].start_step, original.start_step);
    }

    #[test]
    fn making_a_pattern_clip_unique_clones_its_pattern_data() {
        let mut project = Project::default();
        project.duplicate_clip_after(0, 0).unwrap();
        let original_pattern = project.patterns[0].clone();
        let original_id = match project.lanes[0].clips[0].source {
            ClipSource::Pattern { pattern_id } => pattern_id,
            ClipSource::Audio { .. } => unreachable!(),
        };

        let unique_id = project.make_clip_pattern_unique(0, 1).unwrap();

        assert_eq!(unique_id, 1);
        assert_eq!(
            project.patterns[unique_id].step_hits.len(),
            original_pattern.step_hits.len()
        );
        assert_eq!(
            project.patterns[unique_id].notes.len(),
            original_pattern.notes.len()
        );
        assert!(matches!(
            &project.lanes[0].clips[0].source,
            ClipSource::Pattern { pattern_id } if *pattern_id == original_id
        ));
        assert!(matches!(
            &project.lanes[0].clips[1].source,
            ClipSource::Pattern { pattern_id } if *pattern_id == unique_id
        ));
    }

    #[test]
    fn adding_lanes_expands_mixer_routes() {
        let mut project = Project::default();
        project.add_lane();
        assert_eq!(project.lanes.len(), 5);
        assert_eq!(project.sends.len(), 6);
        assert!(project.sends.iter().all(|row| row.len() == 6));
        assert!(
            project.sends[..project.lanes.len()]
                .iter()
                .all(|row| row[project.lanes.len()] > 0.0)
        );
    }

    #[test]
    fn legacy_projects_gain_default_master_routes() {
        let mut project = Project::default();
        project.sends = vec![vec![0.0; project.lanes.len()]; project.lanes.len()];
        project.normalize();

        assert_eq!(project.sends.len(), project.lanes.len() + 1);
        assert!(
            project
                .sends
                .iter()
                .all(|row| row.len() == project.sends.len())
        );
        assert!(
            project.sends[..project.lanes.len()]
                .iter()
                .all(|row| row[project.lanes.len()] > 0.0)
        );
    }

    #[test]
    fn older_project_json_uses_defaults_for_new_mixer_and_synth_fields() {
        let mut serialized = serde_json::to_value(Project::default()).unwrap();
        let project = serialized.as_object_mut().unwrap();
        project.remove("mixer_collapsed");
        project["lanes"][0]
            .as_object_mut()
            .unwrap()
            .remove("pinned");
        project["instruments"][0]
            .as_object_mut()
            .unwrap()
            .remove("synth_kind");
        project.remove("automation_tracks");
        let loaded: Project = serde_json::from_value(serialized).unwrap();

        assert!(!loaded.mixer_collapsed);
        assert!(!loaded.lanes[0].pinned);
        assert_eq!(loaded.instruments[0].synth_kind, SynthKind::Sine);
        assert!(loaded.automation_tracks.is_empty());
    }

    #[test]
    fn automation_interpolates_normalized_values_and_holds_its_endpoints() {
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Gain,
        };
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            name: "Gain ramp".to_owned(),
            target,
            start_step: 0,
            length_steps: 64,
            points: vec![
                AutomationPoint {
                    step: 4,
                    value: 0.0,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 12,
                    value: 1.0,
                    curve: AutomationCurve::Linear,
                },
            ],
            modulation_points: Vec::new(),
            enabled: true,
            source: AutomationSource::Curve,
        });
        project.normalize();

        assert_eq!(project.automated_value(target, 0.8, 0.0), 0.0);
        assert!((project.automated_value(target, 0.8, 8.0) - 0.5).abs() < 0.0001);
        assert_eq!(project.automated_value(target, 0.8, 16.0), 1.0);
    }

    #[test]
    fn automation_clip_limits_curve_to_its_arrangement_range() {
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Gain,
        };
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            target,
            start_step: 8,
            length_steps: 8,
            points: vec![
                AutomationPoint {
                    step: 8,
                    value: 0.2,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 15,
                    value: 0.8,
                    curve: AutomationCurve::Linear,
                },
            ],
            ..AutomationTrack::default()
        });
        project.normalize();
        assert_eq!(project.automated_value(target, 0.5, 7.0), 0.5);
        assert_eq!(project.automated_value(target, 0.5, 8.0), 0.2);
        assert_eq!(project.automated_value(target, 0.5, 16.0), 0.5);
    }

    #[test]
    fn legacy_automation_lane_gets_default_clip_bounds() {
        let track: AutomationTrack = serde_json::from_str(r#"{"name":"Legacy"}"#).unwrap();
        assert_eq!(track.start_step, 0);
        assert_eq!(track.length_steps, 64);
    }

    #[test]
    fn normalization_prunes_automation_targets_without_a_plugin() {
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::InsertEffect {
                channel_id: 0,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            },
            ..AutomationTrack::default()
        });
        project.normalize();

        assert!(project.automation_tracks.is_empty());
    }

    #[test]
    fn adding_a_lane_keeps_master_effect_automation_attached_to_master() {
        let mut project = Project::default();
        let master_id = project.lanes.len();
        project.master.inserts[0].name = "Delay".to_owned();
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::InsertEffect {
                channel_id: master_id,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            },
            ..AutomationTrack::default()
        });

        project.add_lane();

        assert_eq!(
            project.automation_tracks[0].target,
            ParameterTarget::InsertEffect {
                channel_id: project.lanes.len(),
                slot_id: 0,
                parameter: EffectParameter::Mix,
            }
        );
    }

    #[test]
    fn disconnect_channel_clears_its_inputs_and_outputs() {
        let mut sends = vec![
            vec![0.0, 0.25, 0.5],
            vec![0.4, 0.0, 0.2],
            vec![0.1, 0.3, 0.0],
        ];
        disconnect_channel(&mut sends, 1);
        assert_eq!(sends[0][1], 0.0);
        assert!(sends[1].iter().all(|gain| *gain == 0.0));
        assert_eq!(sends[2][1], 0.0);
        assert_eq!(sends[0][2], 0.5);
        assert_eq!(sends[2][0], 0.1);
    }

    #[test]
    fn normalize_bounds_sends_and_prunes_invalid_references() {
        let mut project = Project::default();
        project.sends[0][1] = 0.9;
        project.sends[2][1] = 0.9;
        project.patterns[0].step_hits.push(StepHit {
            instrument_id: 999,
            step: 0,
        });
        project.normalize();

        let total: f32 = project.sends.iter().map(|row| row[1]).sum();
        assert!(total <= MAX_SEND_GAIN_SUM + 0.00001);
        assert!(
            project.patterns[0]
                .step_hits
                .iter()
                .all(|hit| hit.instrument_id < project.instruments.len())
        );
    }

    #[test]
    fn automation_curve_shapes_have_expected_interpolation() {
        let points = |curve| {
            [
                AutomationPoint {
                    step: 0,
                    value: 0.0,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 16,
                    value: 1.0,
                    curve,
                },
            ]
        };
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Linear), 8.0),
            0.5
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Constant), 8.0),
            0.0
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Pulse), 4.0),
            0.0
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Pulse), 12.0),
            1.0
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Steps), 4.0),
            0.25
        );
        let bezier = Project::evaluate_curve(&points(AutomationCurve::Bezier), 8.0);
        assert!(bezier > 0.5 && bezier < 1.0);
    }

    #[test]
    fn perlin_noise_is_deterministic_bounded_and_varies_across_positions() {
        let first = perlin_noise(17, 0.25);
        assert_eq!(first, perlin_noise(17, 0.25));
        assert!((-1.0..=1.0).contains(&first));
        assert_ne!(first, perlin_noise(17, 1.25));
    }

    #[test]
    fn perlin_octaves_add_detail_and_stay_bounded() {
        let single = perlin_noise_octaves(3, 0.37, 1);
        let detailed = perlin_noise_octaves(3, 0.37, 8);
        assert_eq!(single, perlin_noise(3, 0.37));
        assert!((-1.0..=1.0).contains(&detailed));
        assert_ne!(single, detailed);
        // Octave counts are clamped into the supported 1-16 range.
        assert_eq!(
            perlin_noise_octaves(3, 0.37, 0),
            perlin_noise_octaves(3, 0.37, 1)
        );
        assert_eq!(
            perlin_noise_octaves(3, 0.37, 99),
            perlin_noise_octaves(3, 0.37, 16)
        );
        // More octaves produce more direction changes across the same span.
        let reversals = |octaves| {
            let samples: Vec<f32> = (0..64)
                .map(|index| perlin_noise_octaves(5, index as f64 * 0.25, octaves))
                .collect();
            samples
                .windows(3)
                .filter(|window| {
                    let first = window[1] - window[0];
                    let second = window[2] - window[1];
                    first * second < 0.0
                })
                .count()
        };
        assert!(reversals(8) > reversals(1));
    }

    #[test]
    fn normalize_sorts_and_clamps_modulation_curve_points() {
        let mut project = Project::default();
        let track = AutomationTrack {
            modulation_points: vec![
                AutomationPoint {
                    step: 8,
                    value: 2.0,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 2,
                    value: -1.0,
                    curve: AutomationCurve::Linear,
                },
            ],
            ..AutomationTrack::default()
        };
        project.automation_tracks.push(track);
        project.normalize();

        let points = &project.automation_tracks[0].modulation_points;
        assert_eq!(points[0].step, 2);
        assert_eq!(points[0].value, 0.0);
        assert_eq!(points[1].step, 8);
        assert_eq!(points[1].value, 1.0);
    }
}
