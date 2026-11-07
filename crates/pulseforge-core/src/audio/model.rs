//! Audio-side data: commands and the structures the renderer keeps per voice.

use std::sync::Arc;

use pulseforge_profiling::SlotCounters;

use crate::{
    plugins::BuiltinEffect,
    project::{ClipNote, Project},
};

use super::sample::SampleData;

pub(crate) const VOICE_COUNT: usize = 64;

/// Automation and instrument controls are refreshed once per control block
/// instead of once per sample. Sixteen frames is far below the audible zipper
/// threshold while cutting the automation cost by more than an order of
/// magnitude on dense projects.
pub(crate) const CONTROL_INTERVAL: u32 = 16;

/// Requested output buffer in frames. Small buffers lower monitoring latency;
/// the device default is used when a fixed size is not supported.
pub(crate) const PREFERRED_BUFFER_FRAMES: u32 = 256;

/// Everything the UI can ask the audio thread to do.
///
/// The engine owns all of its state, so a change from the UI is a message on
/// this channel rather than a shared mutable value.
pub enum AudioCommand {
    /// Starts or stops playback.
    SetPlaying(bool),
    /// Moves the transport back to the start of the loop (or zero) without
    /// changing the play state.
    Rewind,
    /// Moves the transport to an absolute step position.
    Seek(u64),
    /// Enables or disables the metronome click.
    SetMetronome(bool),
    /// Selects the pattern the step sequencer plays, `None` for the arrangement.
    SetSelectedPattern(Option<usize>),
    /// Sets the arrangement loop range in steps, `None` to clear it.
    SetLoop(Option<(u64, u64)>),
    /// Sets the project tempo in BPM.
    SetTempo(f32),
    /// Replaces the engine's copy of the project.
    SetProject(Box<Project>),
    /// Caches a decoded sample for one track.
    LoadTrackSample {
        /// Track the sample belongs to.
        track_id: usize,
        /// The decoded sample.
        sample: SampleData,
    },
    /// Caches a decoded WAV by path. Clips resolve their audio from this
    /// cache, so the sample survives the project rebuild that follows an edit
    /// and can arrive before the clip itself exists.
    LoadAudioClip {
        /// Path the sample was decoded from, used as the cache key.
        path: String,
        /// The decoded sample.
        sample: SampleData,
    },
    /// Plays a note on a track immediately, bypassing the sequencer.
    MidiNote {
        /// Track the note is sent to.
        track_id: usize,
        /// MIDI note number.
        note: u8,
        /// MIDI velocity, 1..=127.
        velocity: u8,
        /// `true` for note-on, `false` for note-off.
        on: bool,
    },
    /// Plays a one-shot sample through the master bus for auditioning.
    PreviewSample(Box<SampleData>),
    /// Enables or disables linear parameter ramping and sets its length.
    SetSmoothing {
        /// Whether parameter ramping is active.
        enabled: bool,
        /// Ramp length in milliseconds.
        milliseconds: f32,
    },
    /// Hands a real VST3/CLAP audio half to the engine so it can render there.
    LoadHosted {
        /// Host instance id of the plug-in.
        id: u64,
        /// The audio half of the hosted plug-in.
        audio: Box<pulseforge_hosting::HostedAudio>,
        /// Whether the plug-in is an instrument rather than an effect.
        instrument: bool,
        /// Debug counters owned by the UI's plug-in registry; the slot writes
        /// its block times there so the debug panel can list them.
        counters: Option<Arc<SlotCounters>>,
    },
    /// Drops a hosted plug-in (its editor is closed on the UI thread).
    UnloadHosted {
        /// Host instance id of the plug-in.
        id: u64,
    },
}

pub(crate) struct Voice {
    pub(crate) track_id: usize,
    pub(crate) note: u8,
    pub(crate) phase: f32,
    pub(crate) second_phase: f32,
    pub(crate) sub_phase: f32,
    pub(crate) filter_state: f32,
    pub(crate) age_seconds: f32,
    pub(crate) velocity: f32,
    pub(crate) envelope: f32,
    pub(crate) active: bool,
    pub(crate) released: bool,
    pub(crate) release_at_tick: Option<u64>,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            track_id: 0,
            note: 0,
            phase: 0.0,
            second_phase: 0.0,
            sub_phase: 0.0,
            filter_state: 0.0,
            age_seconds: 0.0,
            velocity: 0.0,
            envelope: 0.0,
            active: false,
            released: false,
            release_at_tick: None,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct InstrumentControls {
    pub(crate) gain: f32,
    pub(crate) pan: f32,
    pub(crate) cutoff_hz: f32,
    pub(crate) resonance: f32,
    pub(crate) pulse_width: f32,
    pub(crate) sub_level: f32,
    pub(crate) tone: f32,
    pub(crate) decay: f32,
    pub(crate) fm_ratio: f32,
    pub(crate) fm_index: f32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct AutomationRuntime {
    pub(crate) start_frame: Option<u64>,
    pub(crate) release_frame: Option<u64>,
    pub(crate) release_level: f32,
}

/// One scheduled arpeggiator note.
#[derive(Clone, Copy)]
pub(crate) struct ArpEvent {
    pub(crate) at_tick: u64,
    pub(crate) release_tick: u64,
    pub(crate) track_id: usize,
    pub(crate) pitch: u8,
    pub(crate) velocity: u8,
}

pub(crate) struct SampleVoice {
    pub(crate) track_id: usize,
    pub(crate) cursor: f64,
    pub(crate) active: bool,
}

impl Default for SampleVoice {
    fn default() -> Self {
        Self {
            track_id: 0,
            cursor: 0.0,
            active: false,
        }
    }
}

pub(crate) struct AudioClipRuntime {
    /// Shared by every clip that references the same file, so sliced or
    /// duplicated audio clips keep playing without re-reading the WAV.
    pub(crate) sample: Option<Arc<SampleData>>,
    pub(crate) cursor: f64,
    pub(crate) active: bool,
    /// Set when the clip has played to its end. Cleared when the transport
    /// leaves the clip, so re-entering it plays again.
    pub(crate) finished: bool,
}

impl Default for AudioClipRuntime {
    fn default() -> Self {
        Self {
            sample: None,
            cursor: 0.0,
            active: false,
            finished: false,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TriggerEvent {
    Note {
        track_id: usize,
        note: ClipNote,
    },
    AudioClip {
        track_id: usize,
        clip_index: usize,
    },
}

/// One insert slot's processor. Built-in effects run their DSP; external
/// plug-in bundles run through the hosting boundary, which passes audio through
/// until a format backend is installed.
pub(crate) enum EffectProcessor {
    Builtin(BuiltinEffect),
    /// Hosted VST3/CLAP insert, addressed by host id in `AudioEngine::hosted`.
    External(u64),
}

impl EffectProcessor {
    /// Identity used to decide whether the processor must be rebuilt.
    pub(crate) fn identity(&self) -> (&'static str, &str) {
        match self {
            Self::Builtin(effect) => ("builtin", effect.effect_name()),
            Self::External(_) => ("external", ""),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct EqState {
    pub(crate) low_250: [f32; 2],
    pub(crate) low_2k: [f32; 2],
}

#[derive(Clone, Copy)]
pub(crate) struct SendEdge {
    pub(crate) source: usize,
    pub(crate) destination: usize,
    pub(crate) gain: f32,
    pub(crate) delayed: [f32; 2],
}
