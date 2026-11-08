//! Device types shared by instruments and insert effects.

use serde::{Deserialize, Serialize};

/// Routing and voice options shown on a plug-in's settings tab.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    /// MIDI channel the device listens to.
    pub midi_in_channel: u8,
    /// MIDI channel the device sends on.
    pub midi_out_channel: u8,
    /// Audio input index the device is fed from.
    pub audio_in: u8,
    /// Audio output index the device returns to.
    pub audio_out: u8,
    /// Maximum number of simultaneously sounding voices.
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

/// Three-band shelving equaliser shared by channels and effects.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreeBandEq {
    /// Low shelf gain in dB.
    pub low_db: f32,
    /// Mid band gain in dB.
    pub mid_db: f32,
    /// High shelf gain in dB.
    pub high_db: f32,
}

/// Which built-in synth model an instrument uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynthKind {
    /// Instrument backed by a discovered external plug-in bundle. The built-in
    /// engine has no hosting backend, so this produces no built-in voice; the UI
    /// shows a wrapper window for the plug-in instead.
    External,
    /// Plain sine voice.
    #[default]
    Sine,
    /// Plays one of the shipped drum samples.
    SamplePlayer,
    /// Simple oscillator with a selectable waveform.
    SignalGenerator,
    /// MS-20 style subtractive voice.
    Ms20,
    /// Juno style polyphonic voice with sub oscillator.
    JunoPoly,
    /// M1 style workstation voice with a tone control.
    M1Workstation,
    /// Two-operator FM voice.
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

/// Sample of the shipped library an instrument can play.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultSample {
    /// No built-in sample; the instrument stays silent unless a file is loaded.
    #[default]
    None,
    /// Kick drum of the shipped kit.
    Kick,
    /// Snare drum of the shipped kit.
    Snare,
    /// Closed hi-hat of the shipped kit.
    HiHat,
}

/// Waveform of the simple signal generator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalWaveform {
    /// Sine wave.
    #[default]
    Sine,
    /// Triangle wave.
    Triangle,
    /// Sawtooth wave.
    Saw,
    /// Square wave.
    Square,
}

impl SignalWaveform {
    /// Every waveform, in the order the integer-automation index uses.
    pub const ALL: [Self; 4] = [Self::Sine, Self::Triangle, Self::Saw, Self::Square];

    /// Waveform for an integer index, clamped to the available range.
    pub fn from_index(index: u32) -> Self {
        Self::ALL[(index as usize).min(Self::ALL.len() - 1)]
    }

    /// Index of this waveform in [`SignalWaveform::ALL`].
    pub fn index(self) -> u32 {
        Self::ALL
            .iter()
            .position(|waveform| *waveform == self)
            .unwrap_or(0) as u32
    }
}
