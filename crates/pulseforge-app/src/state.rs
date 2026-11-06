//! Small state types: windows, selections, gestures and rows.

use crate::*;

#[derive(Default)]
pub(crate) struct EditorWindows {
    pub(crate) channel_rack: bool,
    pub(crate) piano_roll: bool,
    pub(crate) mixer: bool,
    pub(crate) routing: bool,
    pub(crate) midi_routing: bool,
    pub(crate) automation: bool,
    pub(crate) render: bool,
    pub(crate) samples: bool,
    pub(crate) settings: bool,
}

/// Control lane shown underneath the piano-roll grid.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlLane {
    Velocity,
    Repeats,
}

/// Which settings page is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    #[default]
    Shortcuts,
    Plugins,
    Samples,
    Interface,
    Graphics,
    Theming,
    General,
}

/// Which list the sample browser panel shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SampleTab {
    #[default]
    Builtin,
    Folders,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PluginTarget {
    Instrument(usize),
    Effect { channel: usize, slot: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    Note {
        pattern: usize,
        instrument: usize,
        pitch: u8,
        start_step: u64,
    },
    Instrument(usize),
    Clip {
        lane: usize,
        clip: usize,
    },
    MixerChannel(usize),
    /// An automation lane row in the arrangement.
    Automation(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PianoTool {
    Pen,
    Select,
    Brush,
    Join,
    Cut,
}

#[derive(Clone, Copy)]
pub(crate) enum PianoGesture {
    Draw {
        pitch: u8,
        start_tick: u64,
        note_index: usize,
        default_duration_ticks: u64,
    },
    Move {
        note_index: usize,
        origin_tick: u64,
        origin_pitch: u8,
        pointer_tick: u64,
        pointer_pitch: u8,
    },
    Resize {
        note_index: usize,
        origin_duration_ticks: u64,
        pointer_tick: u64,
    },
    ResizeStart {
        note_index: usize,
        origin_start_tick: u64,
        origin_duration_ticks: u64,
        pointer_tick: u64,
    },
    Join {
        pitch: u8,
        start_step: u64,
        end_step: u64,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct RackGesture {
    pub(crate) pattern_id: usize,
    pub(crate) instrument_id: usize,
    pub(crate) enabled: bool,
    pub(crate) last_step: u64,
}

#[derive(Clone)]
pub(crate) struct ArrangementGesture {
    pub(crate) lane: usize,
    pub(crate) start_step: u64,
    pub(crate) pointer_offset_steps: u64,
    pub(crate) source: ClipSource,
    pub(crate) resizing: bool,
    pub(crate) resizing_start: bool,
    pub(crate) resize_end_step: u64,
    pub(crate) changed: bool,
}

/// Drag state for moving an automation clip horizontally.
#[derive(Clone, Copy)]
pub(crate) struct AutomationGesture {
    pub(crate) track_id: usize,
    pub(crate) grab_step: u64,
    pub(crate) changed: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ArrangementRow {
    Lane(usize),
    Automation(usize),
}

#[derive(Clone, Copy)]
pub(crate) struct TimelineGrid {
    pub(crate) timeline_left: f32,
    pub(crate) grid_left: f32,
    pub(crate) start_step: u64,
    pub(crate) step_width: f32,
}

/// Height of one automation row inside a track group.
pub(crate) const AUTOMATION_ROW_HEIGHT: f32 = 30.0;

/// One arrangement row with its resolved vertical position.
#[derive(Clone, Copy)]
pub(crate) struct RowSpan {
    pub(crate) row: ArrangementRow,
    pub(crate) top: f32,
    pub(crate) height: f32,
}
