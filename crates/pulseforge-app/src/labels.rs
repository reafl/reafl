//! Human-readable names and option lists.

use super::*;

pub(crate) fn synth_kind_label(kind: SynthKind) -> &'static str {
    match kind {
        SynthKind::External => "External plug-in",
        SynthKind::SamplePlayer => "Sample Player",
        SynthKind::Sine => "Sine Synth",
        SynthKind::SignalGenerator => "Signal Generator",
        SynthKind::Ms20 => "MS-20 Mono",
        SynthKind::JunoPoly => "Juno-style Poly",
        SynthKind::M1Workstation => "M1-style Workstation",
        SynthKind::FmSynth => "2-Operator FM",
    }
}

pub(crate) fn waveform_label(waveform: SignalWaveform) -> &'static str {
    match waveform {
        SignalWaveform::Sine => "Sine",
        SignalWaveform::Triangle => "Triangle",
        SignalWaveform::Saw => "Saw",
        SignalWaveform::Square => "Square",
    }
}

pub(crate) fn default_sample_label(sample: DefaultSample) -> &'static str {
    match sample {
        DefaultSample::None => "None",
        DefaultSample::Kick => "Kick",
        DefaultSample::Snare => "Snare",
        DefaultSample::HiHat => "Hi-hat",
    }
}

pub(crate) fn effect_parameter_options(name: &str) -> Vec<EffectParameter> {
    match name {
        "3-Band EQ" => vec![
            EffectParameter::Mix,
            EffectParameter::LowDb,
            EffectParameter::MidDb,
            EffectParameter::HighDb,
        ],
        "Reverb" | "Plate Reverb" => vec![
            EffectParameter::Mix,
            EffectParameter::Amount,
            EffectParameter::RoomSize,
        ],
        "Delay" | "Tape Echo" => vec![
            EffectParameter::Mix,
            EffectParameter::Amount,
            EffectParameter::TimeSeconds,
            EffectParameter::Feedback,
        ],
        "Juno Chorus" | "Phaser" => vec![
            EffectParameter::Mix,
            EffectParameter::Amount,
            EffectParameter::RateHz,
            EffectParameter::Depth,
        ],
        "Waveshaper" => vec![EffectParameter::Mix, EffectParameter::Drive],
        _ => vec![EffectParameter::Mix],
    }
}

/// Display name of a mixer channel, where `lanes.len()` addresses the master.
///
/// # Parameters
///
/// - `project`: the project the channel belongs to.
/// - `channel`: mixer channel index.
///
/// # Returns
///
/// The lane name, `"Master"` for the master bus, or a placeholder when the
/// channel no longer exists.
pub(crate) fn channel_name(project: &Project, channel: usize) -> String {
    if channel == project.tracks.len() {
        "Master".to_owned()
    } else {
        project
            .tracks
            .get(channel)
            .map_or_else(|| "Mixer".to_owned(), |lane| lane.name.clone())
    }
}

pub(crate) fn parameter_target_label(project: &Project, target: ParameterTarget) -> String {
    match target {
        ParameterTarget::Track {
            track_id,
            parameter,
        } => format!(
            "{} \u{00B7} {}",
            if track_id == project.tracks.len() {
                "Master".to_owned()
            } else {
                project
                    .tracks
                    .get(track_id)
                    .map_or_else(|| "Track".to_owned(), |track| track.name.clone())
            },
            parameter.label()
        ),
        ParameterTarget::PluginParameter {
            host_id,
            parameter_id,
        } => format!("Plug-in {host_id} \u{00B7} parameter {parameter_id}"),
        ParameterTarget::Tempo => "Tempo".to_owned(),
        ParameterTarget::Clip {
            track_id,
            clip_id,
            parameter,
        } => format!(
            "{} \u{00B7} clip {} \u{00B7} {}",
            project
                .tracks
                .get(track_id)
                .map_or("Track", |track| track.name.as_str()),
            clip_id + 1,
            parameter.label()
        ),
        ParameterTarget::SampleDefaults { parameter } => {
            format!("All samples \u{00B7} {}", parameter.label())
        }
        ParameterTarget::AutomationLane {
            track_id,
            parameter,
        } => format!("Auto {track_id:02} · {}", parameter.label()),
        ParameterTarget::InsertEffect {
            track_id,
            slot_id,
            parameter,
        } => {
            let channel_name = if track_id == project.tracks.len() {
                "Master"
            } else {
                project
                    .tracks
                    .get(track_id)
                    .map_or("Mixer channel", |lane| lane.name.as_str())
            };
            let slot_name = if track_id == project.tracks.len() {
                project.master.inserts.get(slot_id)
            } else {
                project
                    .tracks
                    .get(track_id)
                    .and_then(|lane| lane.inserts.get(slot_id))
            }
            .map_or("FX", |slot| slot.name.as_str());
            format!("{channel_name} · {slot_name} · {}", parameter.label())
        }
    }
}

/// Semitone offsets of a named scale, relative to its root.
pub(crate) fn scale_steps_for(name: &str) -> &'static [u8] {
    match name {
        "Major" => &[0, 2, 4, 5, 7, 9, 11],
        "Minor" => &[0, 2, 3, 5, 7, 8, 10],
        "Harmonic minor" => &[0, 2, 3, 5, 7, 8, 11],
        "Dorian" => &[0, 2, 3, 5, 7, 9, 10],
        "Phrygian" => &[0, 1, 3, 5, 7, 8, 10],
        "Lydian" => &[0, 2, 4, 6, 7, 9, 11],
        "Mixolydian" => &[0, 2, 4, 5, 7, 9, 10],
        "Pentatonic major" => &[0, 2, 4, 7, 9],
        "Pentatonic minor" => &[0, 3, 5, 7, 10],
        "Blues" => &[0, 3, 5, 6, 7, 10],
        _ => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    }
}

pub(crate) fn scale_contains_pitch(root: u8, name: &str, pitch: u8) -> bool {
    let degree = (pitch + 12 - (root % 12)) % 12;
    scale_steps_for(name).contains(&degree)
}

/// Nearest pitch inside the scale, preferring the lower note on a tie.
pub(crate) fn snap_pitch_into_scale(root: u8, name: &str, pitch: u8) -> u8 {
    if scale_contains_pitch(root, name, pitch) {
        return pitch;
    }
    for distance in 1..=6u8 {
        if pitch >= distance && scale_contains_pitch(root, name, pitch - distance) {
            return pitch - distance;
        }
        if pitch + distance < 128 && scale_contains_pitch(root, name, pitch + distance) {
            return pitch + distance;
        }
    }
    pitch
}

pub(crate) fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(pitch % 12)],
        i16::from(pitch / 12) - 1
    )
}
