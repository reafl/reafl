//! Tracks: the single entity behind an arrangement row, a mixer strip and a
//! channel-rack slot.
//!
//! A track owns one generator (or is a group), the MIDI processing that feeds
//! it, its mixer settings and its clips. Clips carry their own notes, so the
//! shared pattern pool of the previous model is gone.

use serde::{Deserialize, Serialize};

use super::{
    Arpeggio, DefaultSample, InsertFxSlot, PianoNote, SignalWaveform, SynthKind, ThreeBandEq,
    TrackParameter, VoiceSettings,
};

/// Index of a track inside a project's track list.
pub type TrackId = usize;

/// What a track is: a sound source, or a folder that passes notes down.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TrackKind {
    /// A track with a generator of its own.
    Generator(Generator),
    /// A folder track: sums its children and forwards notes to them.
    Group,
}

impl TrackKind {
    /// The generator of a source track, if this is not a group.
    pub fn generator(&self) -> Option<&Generator> {
        match self {
            Self::Generator(generator) => Some(generator),
            Self::Group => None,
        }
    }

    /// Mutable access to the generator of a source track.
    pub fn generator_mut(&mut self) -> Option<&mut Generator> {
        match self {
            Self::Generator(generator) => Some(generator),
            Self::Group => None,
        }
    }

    /// True when the track is a folder.
    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group)
    }
}

/// Where a generator gets its sound from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorKind {
    /// A built-in synth model. [`SynthKind::External`] hosts a plug-in bundle.
    Synth(SynthKind),
    /// The sample player, using the shipped kit or a loaded file.
    Sample(DefaultSample),
}

impl Default for GeneratorKind {
    fn default() -> Self {
        Self::Synth(SynthKind::Sine)
    }
}

impl GeneratorKind {
    /// The synth model behind this generator, if it is a synth.
    pub fn synth_kind(self) -> Option<SynthKind> {
        match self {
            Self::Synth(kind) => Some(kind),
            Self::Sample(_) => None,
        }
    }
}

/// The sound source of a generator track.
///
/// The synth fields stay side by side, as before, so switching the model never
/// loses the other models' settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Generator {
    /// Which kind of source this is.
    pub kind: GeneratorKind,
    /// Path of a discovered bundle, empty for built-ins.
    pub plugin_path: String,
    /// Path of a loaded sample, empty when a shipped sample is used.
    pub sample_path: String,
    /// Waveform of the simple signal generator.
    pub signal_waveform: SignalWaveform,
    /// MIDI note the signal generator holds.
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
    /// Juno sub oscillator level in 0..=1.
    pub juno_sub_level: f32,
    /// M1 style tone control in 0..=1.
    pub m1_tone: f32,
    /// M1 style decay amount.
    pub m1_decay: f32,
    /// Unit of [`Generator::m1_decay`].
    pub m1_decay_unit: super::TimeUnit,
    /// M1 style decay in steps.
    pub m1_decay_steps: f32,
    /// FM modulator ratio.
    pub fm_ratio: f32,
    /// FM modulation index.
    pub fm_index: f32,
    /// Output gain of the generator in 0..=1.
    pub gain: f32,
    /// Pan of the generator in -1..=1.
    pub pan: f32,
    /// Voice limits and glide settings.
    pub voices: VoiceSettings,
    /// Arpeggiator that turns held notes into a pattern.
    pub arpeggio: Arpeggio,
    /// Opaque plug-in state captured on save and restored on load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugin_state: Vec<u8>,
}

impl Default for Generator {
    fn default() -> Self {
        Self {
            kind: GeneratorKind::Synth(SynthKind::Sine),
            plugin_path: String::new(),
            sample_path: String::new(),
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
            m1_decay_unit: super::TimeUnit::Milliseconds,
            m1_decay_steps: 4.0,
            fm_ratio: 2.0,
            fm_index: 0.35,
            gain: 0.8,
            pan: 0.0,
            voices: VoiceSettings::default(),
            arpeggio: Arpeggio::default(),
            plugin_state: Vec::new(),
        }
    }
}

/// Note processing applied before a track's generator, and — on a group — before
/// the notes are handed to its children.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MidiFx {
    /// Whether the processing runs at all.
    pub enabled: bool,
    /// Transposition in semitones.
    pub transpose: i32,
    /// Velocity scale, `1.0` leaves velocities untouched.
    pub velocity_scale: f32,
    /// Static velocity offset added before the scale.
    pub velocity_offset: i32,
}

impl Default for MidiFx {
    fn default() -> Self {
        Self {
            enabled: true,
            transpose: 0,
            velocity_scale: 1.0,
            velocity_offset: 0,
        }
    }
}

impl MidiFx {
    /// Applies this processing to one note.
    ///
    /// # Parameters
    ///
    /// - `note`: the note to transform.
    ///
    /// # Returns
    ///
    /// The transformed note. Pitch is clamped to the MIDI range and velocity to
    /// `1..=127`, so downstream code never sees an invalid value.
    pub fn process(&self, note: PianoNote) -> PianoNote {
        if !self.enabled {
            return note;
        }
        let pitch = (i32::from(note.pitch) + self.transpose).clamp(0, 127) as u8;
        let velocity = ((f32::from(note.velocity) + self.velocity_offset as f32)
            * self.velocity_scale)
            .round()
            .clamp(1.0, 127.0) as u8;
        PianoNote {
            pitch,
            velocity,
            ..note
        }
    }
}

/// Mixer settings of one track. A group processes the sum of its children with
/// the same controls.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackMixer {
    /// Fader gain in 0..=1.3.
    pub gain: f32,
    /// Pan in -1..=1.
    pub pan: f32,
    /// Stereo width, `0` = mono, `1` = unchanged, `2` = double side signal.
    pub width: f32,
    /// Three-band equaliser.
    pub eq: ThreeBandEq,
    /// Insert effects, in processing order.
    pub inserts: Vec<InsertFxSlot>,
    /// Skips every insert without changing their enabled flags.
    pub fx_bypass: bool,
}

impl Default for TrackMixer {
    fn default() -> Self {
        Self {
            gain: 0.8,
            pan: 0.0,
            width: 1.0,
            eq: ThreeBandEq::default(),
            inserts: vec![InsertFxSlot::default(); super::INSERT_SLOTS],
            fx_bypass: false,
        }
    }
}

/// One note inside a clip, positioned in ticks from the clip start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipNote {
    /// Start position in ticks (`super::TICKS_PER_STEP` ticks per step).
    pub start_tick: u64,
    /// Length in ticks, at least one.
    pub duration_ticks: u64,
    /// MIDI note number.
    pub pitch: u8,
    /// MIDI velocity, 1..=127.
    pub velocity: u8,
    /// Extra triggers after the first one.
    pub repeat_count: u8,
    /// Distance between repeats in ticks; 0 means half the note length.
    pub repeat_ticks: u8,
    /// Velocity lost per repeat.
    pub repeat_decay: u8,
}

impl ClipNote {
    /// A plain note of `steps` steps at `step` inside the clip.
    ///
    /// # Parameters
    ///
    /// - `step`: start step inside the clip.
    /// - `pitch`: MIDI note number.
    /// - `velocity`: MIDI velocity.
    /// - `steps`: length in steps.
    pub fn new(step: u64, pitch: u8, velocity: u8, steps: u64) -> Self {
        Self {
            start_tick: step.saturating_mul(super::TICKS_PER_STEP),
            duration_ticks: (steps.max(1)).saturating_mul(super::TICKS_PER_STEP),
            pitch,
            velocity: velocity.max(1),
            repeat_count: 0,
            repeat_ticks: 0,
            repeat_decay: 0,
        }
    }
}

/// One clip on a track: its own notes, or a reference to an audio file.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Clip {
    /// Display name.
    pub name: String,
    /// Start position in steps.
    pub start_step: u64,
    /// Length in steps.
    pub length_steps: u64,
    /// Material skipped from the clip start, in steps.
    pub offset_steps: u64,
    /// Notes the clip plays. Empty for audio clips.
    pub notes: Vec<ClipNote>,
    /// Path of the WAV for an audio clip, `None` for a note clip.
    pub audio_path: Option<String>,
    /// Per-clip sample gain in 0..=1.
    pub sample_gain: f32,
    /// Per-clip sample pan in -1..=1.
    pub sample_pan: f32,
    /// Per-clip transpose in semitones.
    pub sample_pitch: f32,
}

impl Default for Clip {
    fn default() -> Self {
        Self {
            name: "Clip".to_owned(),
            start_step: 0,
            length_steps: 16,
            offset_steps: 0,
            notes: Vec::new(),
            audio_path: None,
            sample_gain: 0.8,
            sample_pan: 0.0,
            sample_pitch: 0.0,
        }
    }
}

impl Clip {
    /// True when the clip plays an audio file instead of notes.
    pub fn is_audio(&self) -> bool {
        self.audio_path.is_some()
    }

    /// One past the last step the clip covers.
    pub fn end_step(&self) -> u64 {
        self.start_step.saturating_add(self.length_steps)
    }
}

/// One track: generator (or group), MIDI FX, mixer and clips.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    /// Display name.
    pub name: String,
    /// Generator or group.
    pub kind: TrackKind,
    /// Folder this track belongs to, `None` for a top-level track.
    pub parent: Option<TrackId>,
    /// MIDI processing applied before the generator and, on a group, before the
    /// children receive the notes.
    pub midi_fx: MidiFx,
    /// Mixer settings.
    pub mixer: TrackMixer,
    /// Clips on this track, in any order.
    pub clips: Vec<Clip>,
    /// Power switch: a disabled track is silent.
    pub enabled: bool,
    /// Locked tracks ignore edits.
    pub locked: bool,
    /// Soloed tracks mute every track that is not soloed.
    pub solo: bool,
    /// Row height in points.
    pub height: f32,
    /// Hides the children of a group in the arrangement.
    pub collapsed: bool,
    /// Child parameters a group additionally exposes as its own automation
    /// targets.
    ///
    /// Mixer and MIDI FX controls are already group-wide, so this list holds
    /// the generator controls the user picked from the children (for example
    /// the cutoff of every MS-20 below the folder). Entries whose parameter no
    /// descendant has any more are ignored.
    pub group_targets: Vec<TrackParameter>,
    /// Pinned tracks stay at the top of the mixer.
    pub pinned: bool,
    /// Optional custom colour.
    pub color: Option<[u8; 3]>,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            name: "Track".to_owned(),
            kind: TrackKind::Generator(Generator::default()),
            parent: None,
            midi_fx: MidiFx::default(),
            mixer: TrackMixer::default(),
            clips: Vec::new(),
            enabled: true,
            locked: false,
            solo: false,
            height: 54.0,
            collapsed: false,
            group_targets: Vec::new(),
            pinned: false,
            color: None,
        }
    }
}

impl Track {
    /// True when the track is a folder.
    pub fn is_group(&self) -> bool {
        self.kind.is_group()
    }

    /// The generator of a source track.
    pub fn generator(&self) -> Option<&Generator> {
        self.kind.generator()
    }

    /// Mutable access to the generator of a source track.
    pub fn generator_mut(&mut self) -> Option<&mut Generator> {
        self.kind.generator_mut()
    }

    /// The tracks whose `parent` is `parent`, in project order.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `parent`: the group to list the children of.
    ///
    /// # Returns
    ///
    /// The child indices.
    pub fn children(tracks: &[Track], parent: TrackId) -> Vec<TrackId> {
        tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| track.parent == Some(parent))
            .map(|(index, _)| index)
            .collect()
    }

    /// Every track below `track`, depth first.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `track`: the track to descend from.
    ///
    /// # Returns
    ///
    /// The descendants in drawing order. A parent cycle cannot loop the walk.
    pub fn descendants(tracks: &[Track], track: TrackId) -> Vec<TrackId> {
        let mut out = Vec::new();
        let mut visited = vec![false; tracks.len()];
        collect_descendants(tracks, track, &mut visited, &mut out);
        out
    }

    /// The groups above `track`, innermost first.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `track`: the track to walk up from.
    ///
    /// # Returns
    ///
    /// The ancestors, or an empty list for a top-level track.
    pub fn ancestors(tracks: &[Track], track: TrackId) -> Vec<TrackId> {
        let mut out = Vec::new();
        let mut current = tracks.get(track).and_then(|track| track.parent);
        while let Some(parent) = current {
            if out.contains(&parent) || out.len() >= tracks.len() {
                break;
            }
            out.push(parent);
            current = tracks.get(parent).and_then(|track| track.parent);
        }
        out
    }

    /// True when `candidate` sits above `track` in the tree.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `candidate`: the possible ancestor.
    /// - `track`: the track to test.
    pub fn is_ancestor(tracks: &[Track], candidate: TrackId, track: TrackId) -> bool {
        Self::ancestors(tracks, track).contains(&candidate)
    }

    /// Nesting depth, `0` for a top-level track.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `track`: the track to measure.
    pub fn depth(tracks: &[Track], track: TrackId) -> usize {
        Self::ancestors(tracks, track).len()
    }

    /// True when `parent` may become the track's parent.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `track`: the track that would move.
    /// - `parent`: the proposed folder, or `None` to move it to the top level.
    ///
    /// # Returns
    ///
    /// `false` when the move would create a cycle or point at a source track.
    pub fn can_become_child_of(tracks: &[Track], track: TrackId, parent: Option<TrackId>) -> bool {
        let Some(parent) = parent else {
            return true;
        };
        if parent == track || parent >= tracks.len() {
            return false;
        }
        if !tracks[parent].is_group() {
            return false;
        }
        !Self::is_ancestor(tracks, track, parent)
    }
}

/// Depth-first child collection with a visited guard, so a parent cycle cannot
/// make the walk loop forever.
fn collect_descendants(
    tracks: &[Track],
    track: TrackId,
    visited: &mut [bool],
    out: &mut Vec<TrackId>,
) {
    for child in Track::children(tracks, track) {
        if visited.get(child).copied().unwrap_or(true) {
            continue;
        }
        visited[child] = true;
        out.push(child);
        collect_descendants(tracks, child, visited, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three tracks: `0` is a group holding `1` (group) and `2`, and `1` holds
    /// `3`.
    fn tree() -> Vec<Track> {
        let group = |name: &str, parent: Option<TrackId>| Track {
            name: name.to_owned(),
            kind: TrackKind::Group,
            parent,
            ..Track::default()
        };
        vec![
            group("folder", None),
            group("inner", Some(0)),
            Track {
                name: "bass".to_owned(),
                parent: Some(0),
                ..Track::default()
            },
            Track {
                name: "pad".to_owned(),
                parent: Some(1),
                ..Track::default()
            },
        ]
    }

    #[test]
    fn children_lists_direct_children_in_project_order() {
        let tracks = tree();
        assert_eq!(Track::children(&tracks, 0), vec![1, 2]);
        assert_eq!(Track::children(&tracks, 1), vec![3]);
        assert!(Track::children(&tracks, 3).is_empty());
    }

    #[test]
    fn descendants_walks_depth_first() {
        let tracks = tree();
        assert_eq!(Track::descendants(&tracks, 0), vec![1, 3, 2]);
        assert_eq!(Track::descendants(&tracks, 1), vec![3]);
        assert!(Track::descendants(&tracks, 3).is_empty());
    }

    #[test]
    fn ancestors_and_depth_follow_the_tree() {
        let tracks = tree();
        assert_eq!(Track::ancestors(&tracks, 3), vec![1, 0]);
        assert_eq!(Track::depth(&tracks, 3), 2);
        assert_eq!(Track::depth(&tracks, 0), 0);
        assert!(Track::is_ancestor(&tracks, 0, 3));
        assert!(!Track::is_ancestor(&tracks, 3, 0));
    }

    #[test]
    fn a_cycle_is_refused_and_cannot_loop_the_walk() {
        let mut tracks = tree();
        // Point the outer folder at its own grandchild: the move is refused.
        assert!(!Track::can_become_child_of(&tracks, 0, Some(3)));
        // Even if such a cycle is forced in, the walks terminate.
        tracks[0].parent = Some(3);
        assert!(Track::ancestors(&tracks, 3).len() <= tracks.len());
        assert!(Track::descendants(&tracks, 3).len() <= tracks.len());
    }

    #[test]
    fn a_source_track_cannot_be_a_folder() {
        let tracks = tree();
        assert!(!Track::can_become_child_of(&tracks, 3, Some(2)));
        assert!(Track::can_become_child_of(&tracks, 3, None));
        assert!(Track::can_become_child_of(&tracks, 3, Some(0)));
    }

    #[test]
    fn midi_fx_transposes_and_scales_velocity() {
        let note = PianoNote {
            pitch: 60,
            velocity: 100,
            ..PianoNote::default()
        };
        let fx = MidiFx {
            transpose: 12,
            velocity_scale: 0.5,
            velocity_offset: 0,
            enabled: true,
        };
        let processed = fx.process(note);
        assert_eq!(processed.pitch, 72);
        assert_eq!(processed.velocity, 50);
        assert_eq!(fx.process(fx.process(note)).pitch, 84);

        let off = MidiFx {
            enabled: false,
            ..fx
        };
        assert_eq!(off.process(note).pitch, 60);
    }

    #[test]
    fn midi_fx_keeps_pitch_and_velocity_in_range() {
        let note = PianoNote {
            pitch: 120,
            velocity: 120,
            ..PianoNote::default()
        };
        let fx = MidiFx {
            transpose: 40,
            velocity_scale: 4.0,
            velocity_offset: 40,
            enabled: true,
        };
        let processed = fx.process(note);
        assert_eq!(processed.pitch, 127);
        assert_eq!(processed.velocity, 127);
    }
}
