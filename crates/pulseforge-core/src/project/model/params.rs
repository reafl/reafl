//! Time, notes, arpeggios and the constants the model starts from.

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

