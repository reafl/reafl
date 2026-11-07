//! Free DSP helpers: oscillators, EQ, sample interpolation and WAV I/O.

use std::{
    f32::consts::TAU,
    sync::{
        Arc, Mutex,
        mpsc::{self},
    },
};

use crate::{
    hosted::effect_host_id,
    plugins::BuiltinEffect,
    project::{Clip, Generator, LfoShape, Project, SignalWaveform, SynthKind, TimeUnit, TimeValue},
};

use super::engine::*;
use super::model::*;
use super::sample::*;

pub(crate) fn update_effect_chain(
    lane: usize,
    slots: &[crate::project::InsertFxSlot],
    processors: &mut Vec<Option<EffectProcessor>>,
    sample_rate: u32,
) {
    processors.resize_with(slots.len(), || None);
    processors.truncate(slots.len());
    // A soloed insert silences the rest of the chain so it can be auditioned
    // on its own, mirroring the track-level solo rule.
    let any_solo = slots.iter().any(|slot| slot.solo && slot.enabled);
    for (slot_index, (processor, slot)) in processors.iter_mut().zip(slots).enumerate() {
        let wanted = if !slot.enabled || (any_solo && !slot.solo) {
            None
        } else if slot.is_external() {
            Some(EffectProcessor::External(effect_host_id(lane, slot_index)))
        } else {
            BuiltinEffect::new(&slot.name, sample_rate).map(EffectProcessor::Builtin)
        };
        let same = match (&*processor, &wanted) {
            (Some(current), Some(next)) => current.identity() == next.identity(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            *processor = wanted;
        }
    }
}

/// True when a track should sound: its power switch is on and, if anything is
/// soloed, it is one of the soloed tracks.
///
/// # Parameters
///
/// - `project`: the project the track belongs to.
/// - `track_id`: the track to test.
/// - `solo_active`: whether any track is soloed, hoisted by the caller so a
///   block scans the track list once.
pub(crate) fn track_audible(project: &Project, track_id: usize, solo_active: bool) -> bool {
    let Some(track) = project.tracks.get(track_id) else {
        return false;
    };
    track.enabled && (!solo_active || track.solo)
}

pub(crate) fn instrument_controls(instrument: &Generator, bpm: f32) -> InstrumentControls {
    let (cutoff_hz, resonance) = match instrument.kind.synth_kind() {
        Some(SynthKind::Ms20) => (instrument.ms20_cutoff_hz, instrument.ms20_resonance),
        Some(SynthKind::JunoPoly) => (instrument.juno_cutoff_hz, instrument.juno_resonance),
        _ => (1_800.0, 0.2),
    };
    InstrumentControls {
        gain: instrument.gain,
        pan: instrument.pan,
        cutoff_hz,
        resonance,
        pulse_width: instrument.juno_pulse_width,
        sub_level: instrument.juno_sub_level,
        tone: instrument.m1_tone,
        decay: TimeValue {
            amount: match instrument.m1_decay_unit {
                TimeUnit::Steps => instrument.m1_decay_steps,
                TimeUnit::Milliseconds => instrument.m1_decay * 1000.0,
            },
            unit: instrument.m1_decay_unit,
        }
        .seconds(bpm)
        .clamp(0.05, 4.0),
        fm_ratio: instrument.fm_ratio,
        fm_index: instrument.fm_index,
    }
}

/// One random value in `-1..=1` per whole step of `phase`, held in between.
/// `seed` lets independent lanes produce independent noise.
pub fn sample_and_hold(phase: f32, seed: u32) -> f32 {
    let cycle = phase.floor() as u32;
    let mut value = cycle.wrapping_mul(0x9e37_79b9).wrapping_add(seed);
    value ^= value >> 16;
    value = value.wrapping_mul(0x85eb_ca6b);
    value ^= value >> 13;
    (value as f32 / u32::MAX as f32) * 2.0 - 1.0
}

pub(crate) fn lfo_wave(shape: LfoShape, phase: f32) -> f32 {
    match shape {
        LfoShape::Sine => (TAU * phase.fract()).sin(),
        LfoShape::Triangle => 1.0 - 4.0 * (phase.fract() - 0.5).abs(),
        LfoShape::Saw => phase.fract() * 2.0 - 1.0,
        LfoShape::Square => {
            if phase.fract() < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        LfoShape::SampleAndHold => sample_and_hold(phase, 0x7f4a_7c15),
    }
}

/// True when two clips resolve to the same cached audio runtime, so a project
/// update can keep the loaded sample in place instead of reloading.
pub(crate) fn clip_routes_match(left: &Clip, right: &Clip) -> bool {
    match (&left.audio_path, &right.audio_path) {
        (Some(left), Some(right)) => left == right,
        (None, None) => true,
        _ => false,
    }
}

pub(crate) fn interpolate_sample(sample: &SampleData, cursor: f64) -> [f32; 2] {
    let index = cursor as usize;
    if index >= sample.frames.len() {
        return [0.0; 2];
    }
    let next_index = (index + 1).min(sample.frames.len() - 1);
    let fraction = (cursor - index as f64) as f32;
    let current = sample.frames[index];
    let next = sample.frames[next_index];
    [
        finite_sample(current[0] + (next[0] - current[0]) * fraction),
        finite_sample(current[1] + (next[1] - current[1]) * fraction),
    ]
}

pub(crate) fn waveform(waveform: SignalWaveform, phase: f32) -> f32 {
    let cycle = (phase / TAU).fract();
    match waveform {
        SignalWaveform::Sine => phase.sin(),
        SignalWaveform::Triangle => 1.0 - 4.0 * (cycle - 0.5).abs(),
        SignalWaveform::Saw => cycle * 2.0 - 1.0,
        SignalWaveform::Square => {
            if cycle < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

pub(crate) fn eq_gains(low_db: f32, mid_db: f32, high_db: f32) -> [f32; 3] {
    [
        10.0_f32.powf(low_db.clamp(-12.0, 12.0) / 20.0),
        10.0_f32.powf(mid_db.clamp(-12.0, 12.0) / 20.0),
        10.0_f32.powf(high_db.clamp(-12.0, 12.0) / 20.0),
    ]
}

pub(crate) fn process_eq(
    input: [f32; 2],
    state: &mut EqState,
    gains: [f32; 3],
    alpha_250: f32,
    alpha_2k: f32,
) -> [f32; 2] {
    let mut output = [0.0; 2];
    for channel in 0..2 {
        state.low_250[channel] += alpha_250 * (input[channel] - state.low_250[channel]);
        state.low_2k[channel] += alpha_2k * (input[channel] - state.low_2k[channel]);
        let low = state.low_250[channel];
        let mid = state.low_2k[channel] - low;
        let high = input[channel] - state.low_2k[channel];
        let value = low * gains[0] + mid * gains[1] + high * gains[2];
        output[channel] = if value.is_finite() { value } else { 0.0 };
    }
    output
}

pub(crate) fn finite_sample(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Decodes a WAV file into the engine's sample representation.
///
/// # Parameters
///
/// - `path`: file to read.
///
/// # Errors
///
/// Returns a message when the file cannot be opened, decoded or read.
pub fn read_wav(path: &str) -> Result<SampleData, String> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|error| format!("Could not open WAV file: {error}"))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err("The WAV file has no channels.".to_owned());
    }

    let values = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Could not decode WAV samples: {error}"))?,
        hound::SampleFormat::Int => {
            let scale = 2.0_f32.powi(i32::from(spec.bits_per_sample.saturating_sub(1)));
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|value| value as f32 / scale)
                        .map_err(|error| format!("Could not decode WAV samples: {error}"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };

    let channels = usize::from(spec.channels);
    let mut frames = Vec::with_capacity(values.len().div_ceil(channels));
    for frame in values.chunks(channels) {
        let left = finite_sample(frame[0]);
        let right = if channels > 1 {
            finite_sample(frame[1])
        } else {
            left
        };
        frames.push([left, right]);
    }
    if frames.is_empty() {
        return Err("The WAV file contains no audio frames.".to_owned());
    }
    Ok(SampleData {
        sample_rate: spec.sample_rate,
        frames,
    })
}

/// Renders the whole arrangement offline into a stereo 16-bit WAV file.
///
/// # Parameters
///
/// - `project`: the project to render; it is cloned, not mutated.
/// - `path`: output file to write.
///
/// # Returns
///
/// The number of frames written.
///
/// # Errors
///
/// Returns a message when the file cannot be created or written.
pub fn render_project_to_wav(project: &Project, path: &str) -> Result<u64, String> {
    const SAMPLE_RATE: u32 = 44_100;
    let mut engine = AudioEngine::new(project.clone(), SAMPLE_RATE);
    for (track_id, track) in engine.project.tracks.iter().enumerate() {
        let Some(generator) = track.generator() else {
            continue;
        };
        if !generator.sample_path.trim().is_empty() {
            engine.instrument_samples[track_id] = Some(read_wav(&generator.sample_path)?);
        }
    }
    for (track_id, track) in engine.project.tracks.iter().enumerate() {
        for (clip_index, clip) in track.clips.iter().enumerate() {
            if let Some(path) = &clip.audio_path {
                engine.audio_clips[track_id][clip_index].sample = Some(Arc::new(read_wav(path)?));
            }
        }
    }
    let end_step = engine
        .project
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .map(|clip| clip.start_step.saturating_add(clip.length_steps))
        .max()
        .ok_or_else(|| "Add an arrangement clip before rendering.".to_owned())?;
    let frames_per_step = engine.frames_per_step();
    let render_frames = (end_step as f64 * frames_per_step).ceil() + f64::from(SAMPLE_RATE) * 2.0;
    if !render_frames.is_finite() || render_frames > usize::MAX as f64 {
        return Err("The arrangement is too long to render on this system.".to_owned());
    }
    let render_frames = render_frames as usize;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|error| error.to_string())?;
    let (_sender, receiver) = mpsc::channel();
    let project_updates = Mutex::new(None);
    engine.playing = true;
    for _ in 0..render_frames {
        let frame = engine.render_frame(&receiver, &project_updates);
        for sample in frame {
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            writer
                .write_sample((sample * f32::from(i16::MAX)) as i16)
                .map_err(|error| error.to_string())?;
        }
    }
    writer.finalize().map_err(|error| error.to_string())?;
    Ok(render_frames as u64)
}
