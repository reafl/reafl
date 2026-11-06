//! Automation points and tracks, and the insert slots they can drive.

#![allow(unused_imports)]

use serde::{Deserialize, Serialize};

use super::device::*;
use super::params::*;
use super::targets::*;

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
