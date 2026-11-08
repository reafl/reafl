//! The project document: the track tree, the master bus, routing and tempo.
//!
//! A [`Project`] holds one flat list of [`Track`]s plus the parent links that
//! make it a tree. Lanes, mixer channels and rack slots are all the same track,
//! so there is no second list to keep in sync.

use serde::{Deserialize, Serialize};

use super::automation::*;
use super::device::*;
use super::params::*;
use super::track::*;

/// The master bus every track is summed into.
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

/// The whole document: tempo, tracks, routing and automation.
///
/// Everything here is `serde` data, so a project file is a plain serialisation
/// of this struct. Anything derived from it (rendered audio, sample buffers,
/// the arrangement layout) lives outside the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    /// Tempo in beats per minute.
    pub bpm: f32,
    /// Snap grid in ticks; one step is [`crate::project::TICKS_PER_STEP`] ticks.
    pub snap_ticks: u64,
    /// Time signature used for bar lines and the metronome.
    pub time_signature: TimeSignature,
    /// Every track, in display order. Folders are tracks with children.
    pub tracks: Vec<Track>,
    /// The master bus.
    pub master: MasterChannel,
    /// Whether the mixer window shows the collapsed strip layout.
    pub mixer_collapsed: bool,
    /// Automation lanes and clips.
    pub automation_tracks: Vec<AutomationTrack>,
    /// Send matrix: `sends[source][destination]` in 0..=1.
    pub sends: Vec<Vec<f32>>,
    /// Track each of the 16 MIDI channels is routed to.
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
        let mut project = Self {
            bpm: 120.0,
            snap_ticks: 16,
            time_signature: TimeSignature::default(),
            tracks: default_tracks(),
            master: MasterChannel::default(),
            mixer_collapsed: false,
            automation_tracks: Vec::new(),
            sends: Vec::new(),
            midi_routes: [0; MIDI_CHANNEL_COUNT],
            sample_gain: 0.8,
            sample_pan: 0.0,
            sample_pitch: 0.0,
        };
        project.normalize();
        project
    }
}

/// The tracks a fresh project starts with: a drum folder with one track per
/// shipped sample, and a lead synth beside it.
fn default_tracks() -> Vec<Track> {
    const STEPS: u64 = 64;
    let folder = Track::group("Drums");
    let mut kick = Track::source("Kick", sample_generator(DefaultSample::Kick));
    let mut snare = Track::source("Snare", sample_generator(DefaultSample::Snare));
    let mut hat = Track::source("Hat", sample_generator(DefaultSample::HiHat));
    let mut lead = Track::source(
        "Synth",
        Generator {
            kind: GeneratorKind::Synth(SynthKind::Sine),
            ..Generator::default()
        },
    );
    kick.parent = Some(0);
    snare.parent = Some(0);
    hat.parent = Some(0);

    kick.clips = vec![note_clip(
        "Kick",
        (0..STEPS).step_by(4).map(|step| (step, 36)).collect(),
        4,
        STEPS,
    )];
    snare.clips = vec![note_clip(
        "Snare",
        [4, 12, 20, 28, 36, 44, 52, 60]
            .into_iter()
            .map(|step| (step, 38))
            .collect(),
        2,
        STEPS,
    )];
    hat.clips = vec![note_clip(
        "Hat",
        (0..STEPS).step_by(2).map(|step| (step, 42)).collect(),
        1,
        STEPS,
    )];
    lead.clips = vec![note_clip(
        "Synth",
        vec![(0, 60), (16, 64), (32, 67), (48, 72)],
        12,
        STEPS,
    )];

    vec![folder, kick, snare, hat, lead]
}

/// A generator that plays one of the shipped samples.
///
/// # Parameters
///
/// - `sample`: the sample of the shipped kit.
fn sample_generator(sample: DefaultSample) -> Generator {
    Generator {
        kind: GeneratorKind::Sample(sample),
        ..Generator::default()
    }
}

/// A clip holding one note per `(step, pitch)` pair.
///
/// # Parameters
///
/// - `name`: display name of the clip.
/// - `notes`: start step and pitch of every note.
/// - `length`: length of each note in steps.
/// - `total`: length of the clip in steps.
fn note_clip(name: &str, notes: Vec<(u64, u8)>, length: u64, total: u64) -> Clip {
    Clip {
        name: name.to_owned(),
        start_step: 0,
        length_steps: total,
        offset_steps: 0,
        notes: notes
            .into_iter()
            .map(|(step, pitch)| ClipNote::new(step, pitch, 100, length))
            .collect(),
        audio_path: None,
        ..Clip::default()
    }
}
