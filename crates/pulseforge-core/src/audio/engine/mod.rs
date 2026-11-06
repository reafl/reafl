//! The render engine: state, rendering and automation.
//!
//! [`AudioEngine`] is one struct spread over several files: the module root
//! keeps construction and the state that the routing is built from, while
//! [`render`], [`effects`] and [`automation`] hold the parts that run every
//! block.

mod automation;
mod commands;
mod effects;
mod render;
mod setup;
mod voices;

use std::{
    collections::HashMap,
    f32::consts::TAU,
    sync::{
        Arc, Mutex,
        mpsc::{self},
    },
};

use crate::{
    hosted::{HostedSlot, instrument_host_id},
    plugins::EffectParameters,
    project::{
        AutomationLaneParameter, AutomationSource, ClipSource, EffectParameter,
        InstrumentParameter, LfoShape, MixerParameter, ParameterTarget, PianoNote, Project,
        SignalWaveform, SynthKind, TimeUnit, TimeValue, finite_clamp, normalize_sends,
    },
    smooth::SmoothedValue,
};

use super::dsp::*;
use super::model::*;

pub(crate) struct AudioEngine {
    pub(crate) sample_rate: f32,
    pub(crate) playing: bool,
    pub(crate) position_steps: u64,
    pub(crate) frames_into_step: f64,
    pub(crate) last_triggered_tick: Option<u64>,
    pub(crate) selected_pattern: Option<usize>,
    pub(crate) loop_range: Option<(u64, u64)>,
    pub(crate) project: Project,
    pub(crate) instrument_samples: Vec<Option<SampleData>>,
    pub(crate) audio_clips: Vec<Vec<AudioClipRuntime>>,
    pub(crate) send_edges: Vec<SendEdge>,
    pub(crate) lane_inputs: Vec<[f32; 2]>,
    pub(crate) meter_levels: Vec<f32>,
    pub(crate) eq_states: Vec<EqState>,
    pub(crate) eq_gains: Vec<[f32; 3]>,
    pub(crate) effect_processors: Vec<Vec<Option<EffectProcessor>>>,
    pub(crate) eq_alpha_250: f32,
    pub(crate) eq_alpha_2k: f32,
    pub(crate) trigger_events: Vec<TriggerEvent>,
    pub(crate) releasing_instruments: Vec<usize>,
    pub(crate) voices: [Voice; VOICE_COUNT],
    pub(crate) sample_voices: [SampleVoice; VOICE_COUNT],
    pub(crate) instrument_controls: Vec<InstrumentControls>,
    pub(crate) automation_runtime: Vec<AutomationRuntime>,
    /// Automation lane indices grouped by the parameter they drive, so
    /// evaluation only walks lanes that actually match the target.
    pub(crate) automation_index: HashMap<ParameterTarget, Vec<usize>>,
    /// Effect parameters cached for the current control block.
    pub(crate) effect_parameters: Vec<Vec<EffectParameters>>,
    pub(crate) control_countdown: u32,
    /// Linear ramps that keep parameter jumps from producing zipper noise.
    pub(crate) lane_gain: Vec<SmoothedValue>,
    pub(crate) lane_pan: Vec<SmoothedValue>,
    pub(crate) lane_width: Vec<SmoothedValue>,
    pub(crate) master_gain: SmoothedValue,
    pub(crate) master_pan: SmoothedValue,
    pub(crate) effect_mix: Vec<Vec<SmoothedValue>>,
    pub(crate) smoothing_enabled: bool,
    pub(crate) smoothing_milliseconds: f32,
    pub(crate) smoothing_samples: u32,
    /// Absolute-tick arpeggiator events produced from played notes.
    pub(crate) arp_queue: Vec<ArpEvent>,
    /// Cleared when the project changes so the next control block snaps instead
    /// of ramping from stale values.
    pub(crate) controls_snapped: bool,
    pub(crate) preview: Option<Box<SampleData>>,
    pub(crate) preview_cursor: f64,
    pub(crate) metronome: bool,
    pub(crate) click_frames_left: u64,
    pub(crate) click_length_frames: u64,
    pub(crate) click_frequency: f32,
    pub(crate) click_phase: f32,
    pub(crate) rendered_frames: u64,
    pub(crate) retired_project: Option<Box<Project>>,
    /// Decoded audio clips keyed by file path, shared by every clip that uses
    /// the same file.
    pub(crate) clip_samples: HashMap<String, Arc<SampleData>>,
    /// Live VST3/CLAP instances, keyed by host id (see `hosted::*_host_id`).
    pub(crate) hosted: HashMap<u64, HostedSlot>,
    /// Instrument ids that currently have a hosted generator.
    pub(crate) hosted_instruments: Vec<usize>,
}

impl AudioEngine {
    pub(crate) fn new(mut project: Project, sample_rate: u32) -> Self {
        project.normalize();
        let instrument_count = project.instruments.len();
        let automation_track_count = project.automation_tracks.len();
        let instrument_samples = project
            .instruments
            .iter()
            .map(|instrument| SampleData::default_drum(instrument.default_sample, sample_rate))
            .collect();
        let channel_count = project.lanes.len() + 1;
        let audio_clips = project
            .lanes
            .iter()
            .map(|lane| {
                lane.clips
                    .iter()
                    .map(|clip| {
                        if matches!(clip.source, ClipSource::Audio { .. }) {
                            AudioClipRuntime::default()
                        } else {
                            AudioClipRuntime {
                                active: false,
                                cursor: 0.0,
                                sample: None,
                                finished: false,
                            }
                        }
                    })
                    .collect()
            })
            .collect();
        let mut engine = Self {
            sample_rate: sample_rate as f32,
            playing: false,
            position_steps: 0,
            frames_into_step: 0.0,
            last_triggered_tick: None,
            selected_pattern: None,
            loop_range: None,
            send_edges: Vec::new(),
            lane_inputs: vec![[0.0; 2]; channel_count],
            meter_levels: vec![0.0; channel_count],
            eq_states: vec![EqState::default(); channel_count],
            eq_gains: Vec::new(),
            effect_processors: Vec::new(),
            eq_alpha_250: 1.0 - (-TAU * 250.0 / sample_rate as f32).exp(),
            eq_alpha_2k: 1.0 - (-TAU * 2_000.0 / sample_rate as f32).exp(),
            project,
            instrument_samples,
            audio_clips,
            trigger_events: Vec::new(),
            releasing_instruments: Vec::with_capacity(VOICE_COUNT),
            voices: std::array::from_fn(|_| Voice::default()),
            sample_voices: std::array::from_fn(|_| SampleVoice::default()),
            instrument_controls: vec![InstrumentControls::default(); instrument_count],
            automation_runtime: vec![AutomationRuntime::default(); automation_track_count],
            automation_index: HashMap::new(),
            effect_parameters: Vec::new(),
            control_countdown: 0,
            lane_gain: Vec::new(),
            lane_pan: Vec::new(),
            lane_width: Vec::new(),
            master_gain: SmoothedValue::new(1.0, 1),
            master_pan: SmoothedValue::new(0.0, 1),
            effect_mix: Vec::new(),
            smoothing_enabled: true,
            smoothing_milliseconds: 12.0,
            smoothing_samples: (sample_rate as f32 * 0.012) as u32,
            arp_queue: Vec::new(),
            preview: None,
            preview_cursor: 0.0,
            metronome: false,
            click_frames_left: 0,
            click_length_frames: (sample_rate as f32 * 0.035) as u64,
            click_frequency: 1_320.0,
            click_phase: 0.0,
            rendered_frames: 0,
            retired_project: None,
            clip_samples: HashMap::new(),
            hosted: HashMap::new(),
            hosted_instruments: Vec::new(),
            controls_snapped: false,
        };
        engine.resize_routing();
        engine
    }

    /// Groups enabled automation lanes by target. Rebuilt only when the
    /// project structure changes, never inside the sample loop.
    pub(crate) fn rebuild_automation_index(&mut self) {
        self.automation_index.clear();
        for (track_id, track) in self.project.automation_tracks.iter().enumerate() {
            if !track.enabled {
                continue;
            }
            self.automation_index
                .entry(track.target)
                .or_default()
                .push(track_id);
        }
    }
}
