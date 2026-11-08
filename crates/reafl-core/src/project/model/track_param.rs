//! Parameters a track exposes to automation.
//!
//! A [`TrackParameter`] names one control in a track's parameter namespace:
//! its generator, its MIDI FX or its mixer. Mixer and MIDI FX controls are
//! *group-wide*: a group track's lane for them also drives every descendant
//! that has not given the parameter a lane of its own.

use serde::{Deserialize, Serialize};

use super::{
    ArpMode, Generator, INSERT_SLOTS, InstrumentParameter, MixerParameter, SignalWaveform,
    SynthKind, Track, TrackId, TrackKind,
};

/// Controls of a track's [`MidiFx`](super::MidiFx) block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MidiFxParameter {
    /// Transposition in semitones, rounded to a whole number.
    Transpose,
    /// Velocity scale, `1.0` leaves velocities untouched.
    VelocityScale,
}

impl MidiFxParameter {
    /// Human readable label for menus and tooltips.
    pub fn label(self) -> &'static str {
        match self {
            Self::Transpose => "Transpose",
            Self::VelocityScale => "Velocity scale",
        }
    }

    /// Lowest and highest value this parameter accepts.
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Transpose => (-48.0, 48.0),
            Self::VelocityScale => (0.0, 4.0),
        }
    }
}

/// One automatable control of a track.
///
/// The variant tells which part of the track owns the control, so a group can
/// forward a parameter to its children only when the parameter exists there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackParameter {
    /// A parameter of the track's generator. Groups have none.
    Generator(InstrumentParameter),
    /// A control of the track's MIDI FX block.
    MidiFx(MidiFxParameter),
    /// A control of the track's mixer strip, including sends.
    Mixer(MixerParameter),
}

impl TrackParameter {
    /// Human readable label for menus and tooltips.
    pub fn label(self) -> &'static str {
        match self {
            Self::Generator(parameter) => parameter.label(),
            Self::MidiFx(parameter) => parameter.label(),
            Self::Mixer(parameter) => parameter.label(),
        }
    }

    /// Lowest and highest value this parameter accepts.
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Generator(parameter) => parameter.range(),
            Self::MidiFx(parameter) => parameter.range(),
            Self::Mixer(parameter) => parameter.range(),
        }
    }

    /// True when a group's lane for this parameter also drives its descendants.
    ///
    /// Mixer and MIDI FX controls are group-wide because every track has them;
    /// generator controls only exist on some tracks, so a group must list them
    /// in [`Track::group_targets`] explicitly.
    pub fn is_group_wide(self) -> bool {
        !matches!(self, Self::Generator(_))
    }

    /// The generator parameter, if this names one.
    pub fn generator(self) -> Option<InstrumentParameter> {
        match self {
            Self::Generator(parameter) => Some(parameter),
            _ => None,
        }
    }
}

impl Generator {
    /// True when this generator has the given parameter.
    ///
    /// # Parameters
    ///
    /// - `parameter`: the parameter to look for.
    ///
    /// # Returns
    ///
    /// `false` for a control the current synth model does not use, such as the
    /// pulse width on the FM voice.
    pub fn supports(&self, parameter: InstrumentParameter) -> bool {
        match parameter {
            InstrumentParameter::Gain
            | InstrumentParameter::Pan
            | InstrumentParameter::ArpMode
            | InstrumentParameter::ArpGate
            | InstrumentParameter::ArpOctaves
            | InstrumentParameter::ArpRate
            | InstrumentParameter::VoiceCount => true,
            InstrumentParameter::CutoffHz | InstrumentParameter::Resonance => matches!(
                self.kind.synth_kind(),
                Some(SynthKind::Ms20 | SynthKind::JunoPoly)
            ),
            InstrumentParameter::PulseWidth | InstrumentParameter::SubLevel => {
                matches!(self.kind.synth_kind(), Some(SynthKind::JunoPoly))
            }
            InstrumentParameter::Tone | InstrumentParameter::Decay => {
                matches!(self.kind.synth_kind(), Some(SynthKind::M1Workstation))
            }
            InstrumentParameter::FmRatio | InstrumentParameter::FmIndex => {
                matches!(self.kind.synth_kind(), Some(SynthKind::FmSynth))
            }
            InstrumentParameter::Waveform | InstrumentParameter::SynthNote => {
                matches!(self.kind.synth_kind(), Some(SynthKind::SignalGenerator))
            }
        }
    }
}

impl Track {
    /// True when this track has the given parameter.
    ///
    /// # Parameters
    ///
    /// - `parameter`: the parameter to look for.
    ///
    /// # Returns
    ///
    /// Mixer and MIDI FX controls are present on every track; a generator
    /// control only on a source track whose model uses it.
    pub fn supports(&self, parameter: TrackParameter) -> bool {
        match parameter {
            TrackParameter::Generator(inner) => self
                .generator()
                .is_some_and(|generator| generator.supports(inner)),
            TrackParameter::MidiFx(_) | TrackParameter::Mixer(_) => true,
        }
    }

    /// The parameters this track offers to automation.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list, needed for group forwarding.
    /// - `track`: the track whose parameters are listed.
    ///
    /// # Returns
    ///
    /// Generator, MIDI FX and mixer controls of a source track. A group lists
    /// its MIDI FX and mixer controls plus the child parameters it exposes
    /// through [`Track::group_targets`].
    pub fn automation_parameters(tracks: &[Track], track: TrackId) -> Vec<TrackParameter> {
        let Some(entry) = tracks.get(track) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let TrackKind::Generator(generator) = &entry.kind {
            for parameter in InstrumentParameter::ALL {
                if generator.supports(parameter) {
                    out.push(TrackParameter::Generator(parameter));
                }
            }
        }
        for parameter in MidiFxParameter::ALL {
            out.push(TrackParameter::MidiFx(parameter));
        }
        for parameter in MixerParameter::all() {
            out.push(TrackParameter::Mixer(parameter));
        }
        if entry.is_group() {
            for parameter in &entry.group_targets {
                if !out.contains(parameter)
                    && Track::any_descendant_supports(tracks, track, *parameter)
                {
                    out.push(*parameter);
                }
            }
        }
        out
    }

    /// True when at least one track below `group` has the given parameter.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `group`: the folder to search below.
    /// - `parameter`: the parameter to look for.
    pub fn any_descendant_supports(
        tracks: &[Track],
        group: TrackId,
        parameter: TrackParameter,
    ) -> bool {
        Track::descendants(tracks, group).into_iter().any(|child| {
            tracks
                .get(child)
                .is_some_and(|track| track.supports(parameter))
        })
    }

    /// Generator parameters every descendant of `group` has.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `group`: the folder to inspect.
    ///
    /// # Returns
    ///
    /// The intersection over all descendants, in [`InstrumentParameter::ALL`]
    /// order. A folder without descendants returns an empty list, and a folder
    /// whose descendants have an empty intersection returns nothing, so the
    /// picker offers only parameters that can actually be driven. Nested
    /// folders are transparent: they are skipped instead of making the
    /// intersection empty.
    pub fn common_generator_parameters(
        tracks: &[Track],
        group: TrackId,
    ) -> Vec<InstrumentParameter> {
        let sources: Vec<&Track> = Track::descendants(tracks, group)
            .into_iter()
            .filter_map(|child| tracks.get(child))
            .filter(|track| track.generator().is_some())
            .collect();
        if sources.is_empty() {
            return Vec::new();
        }
        InstrumentParameter::ALL
            .into_iter()
            .filter(|parameter| {
                sources.iter().all(|track| {
                    track
                        .generator()
                        .is_some_and(|generator| generator.supports(*parameter))
                })
            })
            .collect()
    }

    /// Every parameter a group may expose from its children.
    ///
    /// # Parameters
    ///
    /// - `tracks`: the project's track list.
    /// - `group`: the folder to inspect.
    ///
    /// # Returns
    ///
    /// The child parameters the user can add to
    /// [`Track::group_targets`]: generator parameters common to every
    /// descendant, followed by insert-slot parameters found on all of them.
    pub fn available_group_targets(tracks: &[Track], group: TrackId) -> Vec<TrackParameter> {
        Track::common_generator_parameters(tracks, group)
            .into_iter()
            .map(TrackParameter::Generator)
            .collect()
    }
}

impl MidiFxParameter {
    /// Every MIDI FX parameter, in picker order.
    pub const ALL: [Self; 2] = [Self::Transpose, Self::VelocityScale];
}

impl MixerParameter {
    /// Every mixer control, with send levels for each insert slot.
    ///
    /// # Returns
    ///
    /// Gain, pan and width first, then the three equaliser bands, then one send
    /// per slot of the mixer's send row.
    pub fn all() -> Vec<Self> {
        let mut out = vec![
            Self::Gain,
            Self::Pan,
            Self::Width,
            Self::LowDb,
            Self::MidDb,
            Self::HighDb,
        ];
        out.extend((0..INSERT_SLOTS).map(|destination| Self::Send { destination }));
        out
    }
}

impl InstrumentParameter {
    /// Every instrument parameter, in picker order.
    pub const ALL: [Self; 17] = [
        Self::Gain,
        Self::Pan,
        Self::CutoffHz,
        Self::Resonance,
        Self::PulseWidth,
        Self::SubLevel,
        Self::Tone,
        Self::Decay,
        Self::FmRatio,
        Self::FmIndex,
        Self::Waveform,
        Self::ArpMode,
        Self::ArpGate,
        Self::ArpOctaves,
        Self::ArpRate,
        Self::SynthNote,
        Self::VoiceCount,
    ];

    /// Lowest and highest value this parameter accepts.
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Gain => (0.0, 1.0),
            Self::Pan => (-1.0, 1.0),
            Self::CutoffHz => (40.0, 16_000.0),
            Self::Resonance => (0.0, 0.9),
            Self::PulseWidth => (0.05, 0.95),
            Self::SubLevel | Self::FmIndex => (0.0, 1.0),
            Self::Tone => (0.0, 1.0),
            Self::Decay => (0.05, 4.0),
            Self::FmRatio => (0.25, 16.0),
            // Enums are addressed as integers, so their range is the index.
            Self::Waveform => (0.0, (SignalWaveform::ALL.len() - 1) as f32),
            Self::ArpMode => (0.0, (ArpMode::ALL.len() - 1) as f32),
            Self::ArpGate => (0.01, 1.0),
            Self::ArpOctaves => (1.0, 8.0),
            Self::ArpRate => (0.1, 64.0),
            Self::SynthNote => (0.0, 127.0),
            Self::VoiceCount => (1.0, 64.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::model::{GeneratorKind, SynthKind};

    fn folder(children: &[TrackKind]) -> Vec<Track> {
        let mut tracks = vec![Track {
            kind: TrackKind::Group,
            ..Track::default()
        }];
        for kind in children {
            tracks.push(Track {
                kind: kind.clone(),
                parent: Some(0),
                ..Track::default()
            });
        }
        tracks
    }

    fn synth(kind: SynthKind) -> TrackKind {
        let mut generator = Generator::default();
        generator.kind = GeneratorKind::Synth(kind);
        TrackKind::Generator(generator)
    }

    fn has(tracks: &[Track], track: TrackId, parameter: TrackParameter) -> bool {
        Track::automation_parameters(tracks, track).contains(&parameter)
    }

    #[test]
    fn a_group_offers_its_own_mixer_and_midi_fx_controls() {
        let tracks = folder(&[synth(SynthKind::Sine)]);
        assert!(has(&tracks, 0, TrackParameter::Mixer(MixerParameter::Gain)));
        assert!(has(
            &tracks,
            0,
            TrackParameter::MidiFx(MidiFxParameter::Transpose)
        ));
        assert!(!has(
            &tracks,
            0,
            TrackParameter::Generator(InstrumentParameter::Gain)
        ));
    }

    #[test]
    fn common_child_parameters_are_the_intersection_of_all_descendants() {
        let tracks = folder(&[synth(SynthKind::Ms20), synth(SynthKind::Ms20)]);
        let common = Track::common_generator_parameters(&tracks, 0);
        assert!(common.contains(&InstrumentParameter::CutoffHz));
        assert!(
            !Track::available_group_targets(&tracks, 0)
                .contains(&TrackParameter::Mixer(MixerParameter::Gain))
        );

        // A single child with a different model removes the shared control.
        let mixed = folder(&[synth(SynthKind::Ms20), synth(SynthKind::FmSynth)]);
        assert!(
            !Track::common_generator_parameters(&mixed, 0).contains(&InstrumentParameter::CutoffHz)
        );
        assert!(Track::common_generator_parameters(&mixed, 0).contains(&InstrumentParameter::Gain));
    }

    #[test]
    fn nested_groups_are_inspected_to_the_bottom() {
        let mut tracks = folder(&[synth(SynthKind::Ms20)]);
        tracks.push(Track {
            kind: TrackKind::Group,
            parent: Some(0),
            ..Track::default()
        });
        tracks.push(Track {
            kind: synth(SynthKind::Ms20),
            parent: Some(2),
            ..Track::default()
        });
        assert!(
            Track::common_generator_parameters(&tracks, 0).contains(&InstrumentParameter::CutoffHz)
        );
        assert!(
            Track::common_generator_parameters(&tracks, 2).contains(&InstrumentParameter::CutoffHz)
        );
    }

    #[test]
    fn listed_group_targets_appear_once_a_descendant_can_drive_them() {
        let mut tracks = folder(&[synth(SynthKind::Ms20)]);
        let cutoff = TrackParameter::Generator(InstrumentParameter::CutoffHz);
        tracks[0].group_targets = vec![cutoff];
        assert!(has(&tracks, 0, cutoff));

        // The parameter disappears once no descendant has it any more.
        tracks[1].kind = synth(SynthKind::Sine);
        assert!(!has(&tracks, 0, cutoff));
    }

    #[test]
    fn an_empty_group_exposes_no_child_parameters() {
        let tracks = folder(&[]);
        assert!(Track::common_generator_parameters(&tracks, 0).is_empty());
        assert!(Track::available_group_targets(&tracks, 0).is_empty());
        assert!(has(&tracks, 0, TrackParameter::Mixer(MixerParameter::Pan)));
    }

    #[test]
    fn group_wide_membership_is_decided_by_the_owner() {
        assert!(TrackParameter::Mixer(MixerParameter::Gain).is_group_wide());
        assert!(TrackParameter::MidiFx(MidiFxParameter::VelocityScale).is_group_wide());
        assert!(!TrackParameter::Generator(InstrumentParameter::CutoffHz).is_group_wide());
    }
}
