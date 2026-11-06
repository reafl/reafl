//! Instruments, patterns, clips, lanes, the master channel and the project itself.

use serde::{Deserialize, Serialize};

use super::automation::*;
use super::device::*;
use super::params::*;
use super::piano::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Instrument {
    pub name: String,
    /// Path of a discovered external generator bundle, empty for built-ins.
    pub plugin_path: String,
    pub sample_path: String,
    pub default_sample: DefaultSample,
    pub synth_enabled: bool,
    pub synth_kind: SynthKind,
    pub signal_waveform: SignalWaveform,
    pub synth_note: u8,
    pub ms20_cutoff_hz: f32,
    pub ms20_resonance: f32,
    pub juno_cutoff_hz: f32,
    pub juno_resonance: f32,
    pub juno_pulse_width: f32,
    pub juno_sub_level: f32,
    pub m1_tone: f32,
    pub m1_decay: f32,
    pub m1_decay_unit: TimeUnit,
    pub m1_decay_steps: f32,
    pub fm_ratio: f32,
    pub fm_index: f32,
    pub gain: f32,
    pub pan: f32,
    pub default_mixer_lane: usize,
    /// Power switch: a disabled instrument stays loaded but makes no sound.
    pub enabled: bool,
    /// Locked instruments ignore edits from the UI.
    pub locked: bool,
    /// Soloed instruments mute every instrument that is not soloed.
    pub solo: bool,
    pub voices: VoiceSettings,
    pub arpeggio: Arpeggio,
    /// Optional custom channel colour.
    pub color: Option<[u8; 3]>,
    /// Opaque plug-in state captured on save and restored on load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugin_state: Vec<u8>,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            name: "Instrument".to_owned(),
            plugin_path: String::new(),
            sample_path: String::new(),
            default_sample: DefaultSample::None,
            synth_enabled: false,
            synth_kind: SynthKind::Sine,
            signal_waveform: SignalWaveform::Sine,
            synth_note: 60,
            ms20_cutoff_hz: 1_800.0,
            ms20_resonance: 0.2,
            juno_cutoff_hz: 2_400.0,
            juno_resonance: 0.15,
            juno_pulse_width: 0.5,
            juno_sub_level: 0.3,
            m1_tone: 0.55,
            m1_decay: 1.2,
            m1_decay_unit: TimeUnit::Milliseconds,
            m1_decay_steps: 4.0,
            fm_ratio: 2.0,
            fm_index: 0.35,
            gain: 0.8,
            pan: 0.0,
            default_mixer_lane: 0,
            enabled: true,
            locked: false,
            solo: false,
            voices: VoiceSettings::default(),
            arpeggio: Arpeggio::default(),
            color: None,
            plugin_state: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Pattern {
    pub name: String,
    pub length_steps: u64,
    pub step_hits: Vec<StepHit>,
    pub notes: Vec<PianoNote>,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            name: "Pattern 1".to_owned(),
            length_steps: INITIAL_PATTERN_STEPS,
            step_hits: Vec::new(),
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClipSource {
    Pattern { pattern_id: usize },
    Audio { path: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrangementClip {
    pub start_step: u64,
    pub length_steps: u64,
    /// Source material skipped from the clip start, in steps. Trimming or
    /// slicing a clip advances this so audio keeps its position.
    pub offset_steps: u64,
    pub source: ClipSource,
    /// Per-clip sample playback controls, mixed with the project-wide ones.
    pub sample_gain: f32,
    /// Pan in -1..=1.
    pub sample_pan: f32,
    /// Transpose in semitones.
    pub sample_pitch: f32,
}

impl Default for ArrangementClip {
    fn default() -> Self {
        Self {
            start_step: 0,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Pattern { pattern_id: 0 },
            sample_gain: 0.8,
            sample_pan: 0.0,
            sample_pitch: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrangementLane {
    pub name: String,
    pub pinned: bool,
    /// Power switch: a disabled track is silent and its inserts are bypassed.
    pub enabled: bool,
    /// Locked tracks ignore edits such as clip dragging.
    pub locked: bool,
    /// Soloed tracks mute every track that is not soloed.
    pub solo: bool,
    /// Row height in points. Tracks can be resized vertically.
    pub height: f32,
    /// Hides this track's automation rows in the arrangement.
    pub automation_collapsed: bool,
    pub gain: f32,
    pub pan: f32,
    /// Stereo width: 0 = mono, 1 = unchanged, 2 = double side signal.
    pub width: f32,
    pub eq: ThreeBandEq,
    pub inserts: Vec<InsertFxSlot>,
    /// Skips every insert on this channel without touching their enable states,
    /// so the chain can be compared dry against wet.
    pub fx_bypass: bool,
    pub clips: Vec<ArrangementClip>,
}

impl Default for ArrangementLane {
    fn default() -> Self {
        Self {
            name: "Track".to_owned(),
            pinned: false,
            enabled: true,
            locked: false,
            solo: false,
            height: 54.0,
            automation_collapsed: false,
            gain: 0.8,
            pan: 0.0,
            width: 1.0,
            eq: ThreeBandEq::default(),
            inserts: vec![InsertFxSlot::default(); INSERT_SLOTS],
            fx_bypass: false,
            clips: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterChannel {
    pub gain: f32,
    pub pan: f32,
    pub eq: ThreeBandEq,
    pub inserts: Vec<InsertFxSlot>,
}

impl Default for MasterChannel {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            eq: ThreeBandEq::default(),
            inserts: vec![InsertFxSlot::default(); INSERT_SLOTS],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub bpm: f32,
    pub snap_ticks: u64,
    pub time_signature: TimeSignature,
    pub instruments: Vec<Instrument>,
    pub patterns: Vec<Pattern>,
    pub lanes: Vec<ArrangementLane>,
    pub master: MasterChannel,
    pub mixer_collapsed: bool,
    pub automation_tracks: Vec<AutomationTrack>,
    pub sends: Vec<Vec<f32>>,
    pub midi_routes: [usize; MIDI_CHANNEL_COUNT],
    /// Defaults applied to every audio clip: gain, pan (-1..=1) and transpose
    /// in semitones. Each clip multiplies its own values on top of these.
    pub sample_gain: f32,
    pub sample_pan: f32,
    pub sample_pitch: f32,
}

impl Default for Project {
    fn default() -> Self {
        let names = ["Kick", "Snare", "Hat", "Synth"];
        let mut instruments: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(index, name)| Instrument {
                name: (*name).to_owned(),
                synth_enabled: index == 3,
                synth_note: [36, 38, 42, 48][index],
                default_sample: match index {
                    0 => DefaultSample::Kick,
                    1 => DefaultSample::Snare,
                    2 => DefaultSample::HiHat,
                    _ => DefaultSample::None,
                },
                synth_kind: if index < 3 {
                    SynthKind::SamplePlayer
                } else {
                    SynthKind::Sine
                },
                default_mixer_lane: index,
                ..Instrument::default()
            })
            .collect();
        let mut pattern = Pattern::default();
        for step in (0..pattern.length_steps).step_by(4) {
            pattern.step_hits.push(StepHit {
                instrument_id: 0,
                step,
            });
        }
        for step in [4, 12, 20, 28, 36, 44, 52, 60] {
            pattern.step_hits.push(StepHit {
                instrument_id: 1,
                step,
            });
        }
        for step in (0..pattern.length_steps).step_by(2) {
            pattern.step_hits.push(StepHit {
                instrument_id: 2,
                step,
            });
        }
        for (start_step, pitch) in [(0, 60), (16, 64), (32, 67), (48, 72)] {
            pattern.notes.push(PianoNote {
                instrument_id: 3,
                start_step,
                start_substep: 0,
                duration_steps: 12,
                duration_substeps: 0,
                pitch,
                velocity: 100,
                ..PianoNote::default()
            });
        }

        let lanes: Vec<_> = (0..INITIAL_LANES)
            .map(|index| {
                let mut lane = ArrangementLane {
                    name: format!("Track {:02}", index + 1),
                    ..ArrangementLane::default()
                };
                if index == 0 {
                    lane.clips.push(ArrangementClip {
                        start_step: 0,
                        length_steps: pattern.length_steps,
                        source: ClipSource::Pattern { pattern_id: 0 },
                        offset_steps: 0,
                        ..ArrangementClip::default()
                    });
                }
                lane
            })
            .collect();
        let lane_count = lanes.len();
        let channel_count = lane_count + 1;
        let mut sends = vec![vec![0.0; channel_count]; channel_count];
        for lane in sends.iter_mut().take(lane_count) {
            lane[lane_count] = 0.25;
        }
        instruments.shrink_to_fit();
        let mut project = Self {
            bpm: 120.0,
            snap_ticks: 16,
            time_signature: TimeSignature::default(),
            instruments,
            patterns: vec![pattern],
            lanes,
            master: MasterChannel::default(),
            mixer_collapsed: false,
            automation_tracks: Vec::new(),
            sends,
            midi_routes: [0; MIDI_CHANNEL_COUNT],
            sample_gain: 0.8,
            sample_pan: 0.0,
            sample_pitch: 0.0,
        };
        project.normalize();
        project
    }
}
