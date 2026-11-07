//! The arpeggiator and the tick grid shared by the sequencers.
//!
//! Notes themselves live in [`ClipNote`](super::ClipNote) inside a clip; this
//! module only holds the timing constants and the [`Arpeggio`] settings that
//! turn held notes into a pattern.

use serde::{Deserialize, Serialize};

use super::params::{TimeUnit, TimeValue};

/// Ticks per step. A step is the smallest grid unit of the sequencers.
pub const TICKS_PER_STEP: u64 = 16;

/// Pitch used by the merged step sequencer (C5).
pub const STEP_SEQUENCER_PITCH: u8 = 72;

/// Note order used by the arpeggiator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArpMode {
    /// Lowest to highest note.
    #[default]
    Up,
    /// Highest to lowest note.
    Down,
    /// Up, then down again.
    UpDown,
    /// Down, then up again.
    DownUp,
    /// Random order.
    Random,
    /// The order the notes were pressed in.
    Order,
}

impl ArpMode {
    /// Every mode, in the order the UI lists them.
    pub const ALL: [Self; 6] = [
        Self::Up,
        Self::Down,
        Self::UpDown,
        Self::DownUp,
        Self::Random,
        Self::Order,
    ];

    /// Human readable label for menus and tooltips.
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

    /// Index of this mode in [`ArpMode::ALL`], used by the automation lanes.
    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0) as u32
    }

    /// Reorders `pitches` (already sorted low to high) for this mode.
    ///
    /// # Parameters
    ///
    /// - `pitches`: the held pitches, sorted ascending.
    ///
    /// # Returns
    ///
    /// The pitches in playing order. The random mode shuffles
    /// deterministically, so renders stay reproducible.
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

/// Per-track arpeggiator. Rate is either musical (steps) or free (ms).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Arpeggio {
    /// Whether held notes are turned into an arpeggio.
    pub enabled: bool,
    /// Order the held notes are played in.
    pub mode: ArpMode,
    /// Time between two arpeggio notes.
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
