//! Time and modulation settings, plus the constants the model starts from.

#![allow(unused_imports)]

use serde::{Deserialize, Serialize};

/// Number of arrangement lanes a new project starts with.
pub const INITIAL_LANES: usize = 4;
/// Length of the pattern a new project starts with, in steps.
pub const INITIAL_PATTERN_STEPS: u64 = 64;
/// Number of MIDI channels the routing table covers.
pub const MIDI_CHANNEL_COUNT: usize = 16;
/// Highest sum of absolute send gains one destination accepts before the
/// matrix is normalised.
pub const MAX_SEND_GAIN_SUM: f32 = 0.95;

/// Insert slots every mixer channel (and the master) provides.
pub const INSERT_SLOTS: usize = 10;

/// Musical time signature, used for bar lines and the metronome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeSignature {
    /// Beats per bar.
    pub numerator: u16,
    /// Note value that counts as one beat.
    pub denominator: u16,
}

/// Unit of a [`TimeValue`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    /// Sixteenth-note steps, so the value follows the tempo.
    Steps,
    /// Milliseconds, so the value is independent of the tempo.
    #[default]
    Milliseconds,
}

/// A time span that can be expressed musically or in real time.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeValue {
    /// Size of the span in [`TimeValue::unit`].
    pub amount: f32,
    /// Unit the amount is expressed in.
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
    /// The span in seconds at the given tempo.
    pub fn seconds(self, bpm: f32) -> f32 {
        match self.unit {
            TimeUnit::Steps => self.amount.max(0.0) * 60.0 / bpm.max(1.0) / 4.0,
            TimeUnit::Milliseconds => self.amount.max(0.0) / 1000.0,
        }
    }
}

/// One octave of Perlin noise in -1..=1, deterministic for a given seed.
pub fn perlin_noise(seed: u32, coordinate: f64) -> f32 {
    perlin_noise_octaves(seed, coordinate, 1)
}

/// Summed Perlin noise octaves in -1..=1, deterministic for a given seed.
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

/// Waveform of an automation LFO.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LfoShape {
    /// Sine wave.
    #[default]
    Sine,
    /// Triangle wave.
    Triangle,
    /// Sawtooth wave.
    Saw,
    /// Square wave.
    Square,
    /// Random value held for one cycle.
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

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl TimeSignature {
    /// Steps that fit in one bar of this signature.
    pub fn steps_per_bar(self) -> u64 {
        (u64::from(self.numerator.max(1)) * 16 / u64::from(self.denominator.max(1))).max(1)
    }
}
