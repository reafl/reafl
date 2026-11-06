//! Notes, steps and the arpeggiator.
//!
//! These types describe what the sequencer plays: a [`PianoNote`] in a
//! pattern, a [`StepHit`] from the step sequencer and the [`Arpeggio`]
//! settings that turn held notes into a pattern.

use serde::{Deserialize, Serialize};

use super::params::{TimeUnit, TimeValue};

/// One cell of the step sequencer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepHit {
    /// Instrument the step triggers.
    pub instrument_id: usize,
    /// Step index inside the pattern.
    pub step: u64,
}

/// One note of the piano roll.
///
/// `repeat_count` and `repeat_ticks` implement FL-style note repeats: the note
/// re-triggers `repeat_count` extra times, `repeat_ticks` apart (0 = half the
/// note length), with velocity falling off by `repeat_decay` per repeat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PianoNote {
    /// Instrument the note plays.
    pub instrument_id: usize,
    /// Start step inside the pattern.
    pub start_step: u64,
    /// Sub-step offset in ticks, 0..=15.
    pub start_substep: u8,
    /// Duration in whole steps.
    pub duration_steps: u64,
    /// Extra duration in ticks, 0..15.
    pub duration_substeps: u8,
    /// MIDI note number.
    pub pitch: u8,
    /// MIDI velocity, 1..=127.
    pub velocity: u8,
    /// Number of extra triggers after the first one.
    pub repeat_count: u8,
    /// Distance between repeats in ticks; 0 means half the note length.
    pub repeat_ticks: u8,
    /// Velocity lost per repeat.
    pub repeat_decay: u8,
}

/// Ticks per step. A step is the smallest grid unit of the sequencers.
pub const TICKS_PER_STEP: u64 = 16;

/// Pitch used by the merged step sequencer (C5).
pub const STEP_SEQUENCER_PITCH: u8 = 72;

impl PianoNote {
    /// Start position in ticks from the pattern start.
    pub fn start_tick(self) -> u64 {
        self.start_step
            .saturating_mul(TICKS_PER_STEP)
            .saturating_add(u64::from(self.start_substep.min(15)))
    }

    /// Length in ticks; always at least one tick.
    pub fn duration_ticks(self) -> u64 {
        self.duration_steps
            .saturating_mul(TICKS_PER_STEP)
            .saturating_add(u64::from(self.duration_substeps.min(15)))
            .max(1)
    }

    /// Moves the note to an absolute tick position.
    pub fn set_start_tick(&mut self, tick: u64) {
        self.start_step = tick / 16;
        self.start_substep = (tick % TICKS_PER_STEP) as u8;
    }

    /// Stretches the note to an absolute length in ticks.
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
