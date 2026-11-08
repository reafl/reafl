//! Automation points and tracks, and the insert slots they can drive.

#![allow(unused_imports)]

use serde::{Deserialize, Serialize};

use super::device::*;
use super::params::*;
use super::targets::*;

/// Interpolation used between two [`AutomationPoint`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCurve {
    /// Straight line between the points.
    #[default]
    Linear,
    /// Eased in and out with a smoothstep.
    Smooth,
    /// Cubic curve through the surrounding points.
    Bezier,
    /// Concave/convex curve with an adjustable tension.
    Curved,
    /// Short pulse at the point.
    Pulse,
    /// Holds the value until the next point.
    Constant,
    /// Quantises the ramp to discrete steps.
    Steps,
}

/// Where an automation lane's values come from.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum AutomationSource {
    /// The hand-drawn curve in `track.points`.
    #[default]
    Curve,
    /// A drawn curve whose points are offsets added to the parameter's own
    /// value rather than replacing it. Values run from `-1` to `1`, so the lane
    /// can pull a parameter down as well as push it up.
    RelativeCurve,
    /// An envelope driven by one track's MIDI notes.
    MidiEnvelope {
        /// Track whose notes retrigger the envelope.
        track_id: usize,
        /// Attack time.
        attack: TimeValue,
        /// Decay time.
        decay: TimeValue,
        /// Sustain level in 0..=1.
        sustain: f32,
        /// Release time.
        release: TimeValue,
        /// Value the envelope rests at.
        base: f32,
        /// Amount the envelope swings above `base`.
        depth: f32,
    },
    /// A free-running or retriggered low-frequency oscillator.
    Lfo {
        /// Waveform of the oscillator.
        shape: LfoShape,
        /// Cycle length.
        rate: TimeValue,
        /// Restart the phase whenever playback starts.
        retrigger: bool,
        /// Centre value of the modulation.
        base: f32,
        /// Amount the oscillation swings around `base`.
        depth: f32,
    },
    /// Perlin noise, for organic movement.
    Perlin {
        /// Noise seed.
        seed: u32,
        /// Distance between noise samples.
        rate: TimeValue,
        /// Number of summed noise octaves.
        octaves: u32,
        /// Centre value of the modulation.
        base: f32,
        /// Amount the noise swings around `base`.
        depth: f32,
    },
    /// One random value per step of `rate`, held until the next step. Kept out
    /// of [`LfoShape`] so it reads as its own modulation category.
    SampleAndHold {
        /// Random seed, so a lane repeats exactly.
        seed: u32,
        /// Step length of the hold.
        rate: TimeValue,
        /// Centre value of the modulation.
        base: f32,
        /// Amount the random values swing around `base`.
        depth: f32,
    },
    /// Follows the peak level of a mixer channel, so the audio of a track can
    /// drive any parameter (ducking, envelope following, cross-modulation).
    ///
    /// Unlike the other generated sources this one has no shape of its own: it
    /// depends on what is playing, which is why the lane draws it as a
    /// placeholder clip instead of a curve.
    Peak {
        /// Mixer channel whose level is followed; `lanes.len()` is the master.
        channel_id: usize,
        /// Follow the level after the insert chain instead of before it.
        post_fx: bool,
        /// Attack of the envelope follower.
        attack: TimeValue,
        /// Release of the envelope follower.
        release: TimeValue,
        /// Level the lane rests at while the channel is silent.
        base: f32,
        /// Amount the peak swings above `base`.
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

    /// Short label used by the source picker and the lane captions.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Curve => "Curve",
            Self::RelativeCurve => "Relative curve",
            Self::MidiEnvelope { .. } => "MIDI envelope",
            Self::Lfo { .. } => "LFO",
            Self::Perlin { .. } => "Perlin",
            Self::SampleAndHold { .. } => "Sample & hold",
            Self::Peak { .. } => "Track peak",
        }
    }

    /// True when the lane's own base, depth or rate can be automated, which is
    /// what the automation window lists under the source picker.
    pub fn has_parameters(&self) -> bool {
        !matches!(self, Self::Curve | Self::RelativeCurve)
    }

    /// True when the values only exist while the transport runs, so the lane
    /// cannot draw a curve for them and shows a placeholder clip instead.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Peak { .. })
    }

    /// Centre value of the modulation, when the source has one.
    pub fn base(&self) -> Option<f32> {
        match self {
            Self::Curve | Self::RelativeCurve => None,
            Self::MidiEnvelope { base, .. }
            | Self::Lfo { base, .. }
            | Self::Perlin { base, .. }
            | Self::SampleAndHold { base, .. }
            | Self::Peak { base, .. } => Some(*base),
        }
    }

    /// Amount the modulation swings, when the source has one.
    pub fn depth(&self) -> Option<f32> {
        match self {
            Self::Curve | Self::RelativeCurve => None,
            Self::MidiEnvelope { depth, .. }
            | Self::Lfo { depth, .. }
            | Self::Perlin { depth, .. }
            | Self::SampleAndHold { depth, .. }
            | Self::Peak { depth, .. } => Some(*depth),
        }
    }

    /// Copy of the source with a different base and depth.
    ///
    /// Used by the lane drawing, which modulates those two values with their
    /// own automation before sampling the source.
    pub fn with_base_depth(&self, new_base: f32, new_depth: f32) -> Self {
        let mut copy = self.clone();
        match &mut copy {
            Self::MidiEnvelope { base, depth, .. }
            | Self::Lfo { base, depth, .. }
            | Self::Perlin { base, depth, .. }
            | Self::SampleAndHold { base, depth, .. }
            | Self::Peak { base, depth, .. } => {
                *base = new_base;
                *depth = new_depth;
            }
            Self::Curve | Self::RelativeCurve => {}
        }
        copy
    }
}

/// One control point of a drawn automation curve.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationPoint {
    /// Position in steps from the start of the project.
    pub step: u64,
    /// Normalised value in 0..=1, or -1..=1 for a relative lane.
    pub value: f32,
    /// Interpolation used towards the next point.
    pub curve: AutomationCurve,
}

/// One automation lane: a target, a clip range and its source of values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationTrack {
    /// Display name, defaulting to the target label.
    pub name: String,
    /// The parameter this lane drives.
    pub target: ParameterTarget,
    /// Start of the lane's clip in steps.
    pub start_step: u64,
    /// Length of the lane's clip in steps.
    pub length_steps: u64,
    /// Drawn curve points, used when the source builds on a curve.
    pub points: Vec<AutomationPoint>,
    /// Drawn modulation multiplier points, used by the generated sources.
    pub modulation_points: Vec<AutomationPoint>,
    /// Whether the lane contributes to the parameter at all.
    pub enabled: bool,
    /// Where the values come from.
    pub source: AutomationSource,
}

impl Default for AutomationTrack {
    fn default() -> Self {
        Self {
            name: "Automation".to_owned(),
            target: ParameterTarget::default(),
            start_step: 0,
            length_steps: 64,
            points: Vec::new(),
            modulation_points: Vec::new(),
            enabled: true,
            source: AutomationSource::Curve,
        }
    }
}

/// One slot of a channel's insert chain.
///
/// A slot is either a built-in effect (identified by `name`) or a hosted plug-in
/// bundle (identified by `plugin_path`). The parameter fields cover every
/// built-in processor; each effect reads the subset it needs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct InsertFxSlot {
    /// Name of the built-in effect, empty when the slot is unused.
    pub name: String,
    /// Whether the slot processes audio at all.
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
    /// Dry/wet balance in 0..=1.
    pub mix: f32,
    /// Generic amount control, the meaning depends on the effect.
    pub amount: f32,
    /// Equaliser of the effect, used by the equaliser processors.
    pub eq: ThreeBandEq,
    /// Delay or release time in seconds.
    pub time_seconds: f32,
    /// Unit used by the time control.
    pub time_unit: TimeUnit,
    /// Delay time in steps, used when the unit is [`TimeUnit::Steps`].
    pub time_steps: f32,
    /// Feedback or sustain amount in 0..=1.
    pub feedback: f32,
    /// Modulation rate in Hz.
    pub rate_hz: f32,
    /// Modulation depth in 0..=1.
    pub depth: f32,
    /// Reverb room size in 0..=1.
    pub room_size: f32,
    /// Waveshaper drive in 0..=1.
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
    /// The time control in its own unit: steps or milliseconds.
    pub fn time_amount(&self) -> f32 {
        match self.time_unit {
            TimeUnit::Steps => self.time_steps,
            TimeUnit::Milliseconds => self.time_seconds * 1000.0,
        }
    }

    /// Sets the time control in its own unit: steps or milliseconds.
    pub fn set_time_amount(&mut self, amount: f32) {
        match self.time_unit {
            TimeUnit::Steps => self.time_steps = amount,
            TimeUnit::Milliseconds => self.time_seconds = amount / 1000.0,
        }
    }

    /// The time control expressed in seconds at the given tempo, clamped to a
    /// usable range.
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
