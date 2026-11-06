//! Instruments, patterns, clips, lanes, the master channel and the project itself.

use serde::{Deserialize, Serialize};

use super::automation::*;
use super::device::*;
use super::params::*;
use super::piano::*;

/// One instrument: a sampler, a built-in synth or a hosted generator plug-in.
///
/// The synth fields are kept side by side instead of in an enum payload so that
/// switching the [`SynthKind`] never loses the other models' settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Instrument {
    /// Display name, also used as the label in the rack and the mixer.
    pub name: String,
    /// Path of a discovered external generator bundle, empty for built-ins.
    pub plugin_path: String,
    /// Path of the user-selected sample, empty when a built-in sample is used.
    pub sample_path: String,
    /// Which sample of the shipped library this instrument plays.
    pub default_sample: DefaultSample,
    /// Whether the synth voice is mixed in beside the sampler.
    pub synth_enabled: bool,
    /// Which built-in synth model the voice uses.
    pub synth_kind: SynthKind,
    /// Waveform of the simple signal generator model.
    pub signal_waveform: SignalWaveform,
    /// MIDI note the synth plays for the step sequencer and the signal model.
    pub synth_note: u8,
    /// MS-20 style low-pass cutoff in Hz.
    pub ms20_cutoff_hz: f32,
    /// MS-20 style resonance in 0..=1.
    pub ms20_resonance: f32,
    /// Juno style low-pass cutoff in Hz.
    pub juno_cutoff_hz: f32,
    /// Juno style resonance in 0..=1.
    pub juno_resonance: f32,
    /// Juno pulse width in 0..=1.
    pub juno_pulse_width: f32,
    /// Level of the Juno sub oscillator in 0..=1.
    pub juno_sub_level: f32,
    /// M1 style tone/colour control in 0..=1.
    pub m1_tone: f32,
    /// M1 style decay amount, interpreted with [`Instrument::m1_decay_unit`].
    pub m1_decay: f32,
    /// Unit of [`Instrument::m1_decay`].
    pub m1_decay_unit: TimeUnit,
    /// M1 style decay in steps, used when the unit is [`TimeUnit::Steps`].
    pub m1_decay_steps: f32,
    /// FM modulator ratio relative to the carrier.
    pub fm_ratio: f32,
    /// FM modulation index.
    pub fm_index: f32,
    /// Output gain of the instrument in 0..=1.
    pub gain: f32,
    /// Pan of the instrument in -1..=1.
    pub pan: f32,
    /// Mixer lane the instrument is routed to by default.
    pub default_mixer_lane: usize,
    /// Power switch: a disabled instrument stays loaded but makes no sound.
    pub enabled: bool,
    /// Locked instruments ignore edits from the UI.
    pub locked: bool,
    /// Soloed instruments mute every instrument that is not soloed.
    pub solo: bool,
    /// Voice allocation limits and glide settings of this instrument.
    pub voices: VoiceSettings,
    /// Arpeggiator that turns held notes into a pattern.
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

/// One pattern: the notes and step hits of a single bar-group.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Pattern {
    /// Display name.
    pub name: String,
    /// Length in steps; the toolbar shows this many columns.
    pub length_steps: u64,
    /// Step-sequencer hits, one per instrument and step.
    pub step_hits: Vec<StepHit>,
    /// Piano-roll notes.
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

/// What an arrangement clip plays.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClipSource {
    /// A pattern clip, referencing an entry of [`Project::patterns`].
    Pattern {
        /// Index into [`Project::patterns`].
        pattern_id: usize,
    },
    /// An audio clip, referencing a WAV file on disk.
    Audio {
        /// Path of the WAV file.
        path: String,
    },
}

/// One clip on an arrangement lane.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrangementClip {
    /// Start position in steps.
    pub start_step: u64,
    /// Length in steps.
    pub length_steps: u64,
    /// Source material skipped from the clip start, in steps. Trimming or
    /// slicing a clip advances this so audio keeps its position.
    pub offset_steps: u64,
    /// What the clip plays.
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

/// One mixer channel and its arrangement row.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ArrangementLane {
    /// Display name, used by the mixer, the routing menu and the arrangement.
    pub name: String,
    /// Pinned lanes stay above the scrolling lanes in the mixer.
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
    /// Fader gain in 0..=1.3.
    pub gain: f32,
    /// Pan in -1..=1.
    pub pan: f32,
    /// Stereo width: 0 = mono, 1 = unchanged, 2 = double side signal.
    pub width: f32,
    /// Three-band equaliser of the channel.
    pub eq: ThreeBandEq,
    /// Insert effects, in processing order.
    pub inserts: Vec<InsertFxSlot>,
    /// Skips every insert on this channel without touching their enable states,
    /// so the chain can be compared dry against wet.
    pub fx_bypass: bool,
    /// Clips placed on this lane.
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

/// The master bus every lane is summed into.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterChannel {
    /// Master fader gain in 0..=1.3.
    pub gain: f32,
    /// Master pan in -1..=1.
    pub pan: f32,
    /// Master three-band equaliser.
    pub eq: ThreeBandEq,
    /// Master insert effects, in processing order.
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

/// The whole document: tempo, instruments, patterns, lanes and routing.
///
/// Everything here is `serde` data, so a project file is a plain serialisation
/// of this struct. Anything derived from it (rendered audio, sample buffers,
/// the arrangement layout) lives outside the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    /// Tempo in beats per minute.
    pub bpm: f32,
    /// Snap grid in ticks; see [`TICKS_PER_STEP`].
    pub snap_ticks: u64,
    /// Time signature used for bar lines and the metronome.
    pub time_signature: TimeSignature,
    /// Instruments of the channel rack.
    pub instruments: Vec<Instrument>,
    /// Patterns the arrangement clips can reference.
    pub patterns: Vec<Pattern>,
    /// Arrangement lanes, which are also the mixer channels.
    pub lanes: Vec<ArrangementLane>,
    /// The master bus.
    pub master: MasterChannel,
    /// Whether the mixer window shows the collapsed strip layout.
    pub mixer_collapsed: bool,
    /// Automation lanes and clips.
    pub automation_tracks: Vec<AutomationTrack>,
    /// Send matrix: `sends[source][destination]` in 0..=1.
    pub sends: Vec<Vec<f32>>,
    /// Mixer lane each of the 16 MIDI channels is routed to.
    pub midi_routes: [usize; MIDI_CHANNEL_COUNT],
    /// Defaults applied to every audio clip: gain, pan (-1..=1) and transpose
    /// in semitones. Each clip multiplies its own values on top of these.
    pub sample_gain: f32,
    /// Default clip pan in -1..=1.
    pub sample_pan: f32,
    /// Default clip transpose in semitones.
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
