//! Device types shared by instruments and insert effects.

use serde::{Deserialize, Serialize};

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
