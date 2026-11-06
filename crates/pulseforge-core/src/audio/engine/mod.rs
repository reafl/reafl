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

    /// Re-creates the ramp lengths after the smoothing setting changes. A
    /// disabled smoother snaps straight to its target, which is exactly what a
    /// one-sample ramp does.
    pub(crate) fn apply_smoothing_settings(&mut self) {
        let ramp = self.smoothing_samples.max(1);
        for value in self
            .lane_gain
            .iter_mut()
            .chain(self.lane_pan.iter_mut())
            .chain(self.lane_width.iter_mut())
            .chain(self.effect_mix.iter_mut().flatten())
        {
            let target = value.target();
            *value = SmoothedValue::new(
                if self.smoothing_enabled {
                    value.value()
                } else {
                    target
                },
                ramp,
            );
            value.set_target(target);
        }
        let master_target = self.master_gain.target();
        self.master_gain = SmoothedValue::new(
            if self.smoothing_enabled {
                self.master_gain.value()
            } else {
                master_target
            },
            ramp,
        );
        self.master_gain.set_target(master_target);
        let pan_target = self.master_pan.target();
        self.master_pan = SmoothedValue::new(
            if self.smoothing_enabled {
                self.master_pan.value()
            } else {
                pan_target
            },
            ramp,
        );
        self.master_pan.set_target(pan_target);
    }

    /// Points every ramp at the value the project currently asks for.
    ///
    /// Existing ramps retarget smoothly; ramps created for the first time start
    /// at their target so a freshly loaded project never fades in from silence.
    pub(crate) fn retarget_smoothing(&mut self) {
        let ramp = if self.smoothing_enabled {
            self.smoothing_samples.max(1)
        } else {
            1
        };
        let lane_count = self.project.lanes.len();
        let mut gains = Vec::with_capacity(lane_count);
        let mut pans = Vec::with_capacity(lane_count);
        let mut widths = Vec::with_capacity(lane_count);
        let mut mixes: Vec<Vec<SmoothedValue>> = Vec::with_capacity(lane_count + 1);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            let mut gain = self
                .lane_gain
                .get(channel)
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(lane.gain, ramp));
            let mut pan = self
                .lane_pan
                .get(channel)
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(lane.pan, ramp));
            let mut width = self
                .lane_width
                .get(channel)
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(lane.width, ramp));
            gain = gain.with_ramp(ramp);
            pan = pan.with_ramp(ramp);
            width = width.with_ramp(ramp);
            gain.set_target(lane.gain);
            pan.set_target(lane.pan);
            width.set_target(lane.width);
            gains.push(gain);
            pans.push(pan);
            widths.push(width);
            let mut slots = Vec::with_capacity(lane.inserts.len());
            for (slot_id, slot) in lane.inserts.iter().enumerate() {
                let mut value = self
                    .effect_mix
                    .get(channel)
                    .and_then(|existing| existing.get(slot_id))
                    .copied()
                    .unwrap_or_else(|| SmoothedValue::new(slot.mix, ramp));
                value = value.with_ramp(ramp);
                value.set_target(slot.mix);
                slots.push(value);
            }
            mixes.push(slots);
        }
        let master_index = lane_count;
        let mut master_slots = Vec::with_capacity(self.project.master.inserts.len());
        for (slot_id, slot) in self.project.master.inserts.iter().enumerate() {
            let mut value = self
                .effect_mix
                .get(master_index)
                .and_then(|existing| existing.get(slot_id))
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(slot.mix, ramp));
            value = value.with_ramp(ramp);
            value.set_target(slot.mix);
            master_slots.push(value);
        }
        mixes.push(master_slots);
        self.lane_gain = gains;
        self.lane_pan = pans;
        self.lane_width = widths;
        self.effect_mix = mixes;
        self.master_gain = self.master_gain.with_ramp(ramp);
        self.master_gain.set_target(self.project.master.gain);
        self.master_pan = self.master_pan.with_ramp(ramp);
        self.master_pan.set_target(self.project.master.pan);
    }

    pub(crate) fn resize_routing(&mut self) {
        let lane_count = self.project.lanes.len() + 1;
        self.audio_clips
            .resize_with(self.project.lanes.len(), Vec::new);
        for (lane_id, lane) in self.project.lanes.iter().enumerate() {
            self.audio_clips[lane_id].resize_with(lane.clips.len(), AudioClipRuntime::default);
        }
        let event_capacity: usize = self
            .project
            .lanes
            .iter()
            .flat_map(|lane| &lane.clips)
            .map(|clip| match clip.source {
                ClipSource::Pattern { pattern_id } => {
                    self.project.patterns.get(pattern_id).map_or(1, |pattern| {
                        1 + pattern.step_hits.len() + pattern.notes.len()
                    })
                }
                ClipSource::Audio { .. } => 1,
            })
            .sum::<usize>()
            + self
                .project
                .patterns
                .iter()
                .map(|pattern| pattern.step_hits.len() + pattern.notes.len())
                .sum::<usize>();
        self.trigger_events
            .reserve(event_capacity.saturating_sub(self.trigger_events.len()));
        self.lane_inputs.resize(lane_count, [0.0; 2]);
        self.meter_levels.resize(lane_count, 0.0);
        self.eq_states.resize(lane_count, EqState::default());
        self.eq_gains = self
            .project
            .lanes
            .iter()
            .map(|lane| eq_gains(lane.eq.low_db, lane.eq.mid_db, lane.eq.high_db))
            .chain(std::iter::once(eq_gains(
                self.project.master.eq.low_db,
                self.project.master.eq.mid_db,
                self.project.master.eq.high_db,
            )))
            .collect();
        self.effect_processors
            .resize_with(self.project.lanes.len() + 1, Vec::new);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            // A bypassed channel keeps its insert settings but renders with no
            // chain at all, so the dry signal costs nothing.
            let inserts: &[_] = if lane.fx_bypass { &[] } else { &lane.inserts };
            update_effect_chain(
                channel,
                inserts,
                &mut self.effect_processors[channel],
                self.sample_rate as u32,
            );
        }
        update_effect_chain(
            self.project.lanes.len(),
            &self.project.master.inserts,
            &mut self.effect_processors[self.project.lanes.len()],
            self.sample_rate as u32,
        );
        self.instrument_samples
            .resize_with(self.project.instruments.len(), || None);
        self.instrument_controls.resize(
            self.project.instruments.len(),
            InstrumentControls::default(),
        );
        self.automation_runtime.resize(
            self.project.automation_tracks.len(),
            AutomationRuntime::default(),
        );
        self.rebuild_send_edges();
        self.rebuild_automation_index();
        self.resize_effect_parameters();
        self.retarget_smoothing();
        self.control_countdown = 0;
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

    pub(crate) fn resize_effect_parameters(&mut self) {
        self.effect_parameters.clear();
        self.effect_parameters
            .resize_with(self.project.lanes.len() + 1, Vec::new);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            self.effect_parameters[channel].resize(lane.inserts.len(), EffectParameters::default());
        }
        self.effect_parameters[self.project.lanes.len()].resize(
            self.project.master.inserts.len(),
            EffectParameters::default(),
        );
    }

    /// Refresh the mixer values that can change without resizing anything.
    /// Keeping this path allocation-light avoids audio dropouts while the user
    /// drags knobs or automation points.
    pub(crate) fn refresh_mix_state(&mut self) {
        self.eq_gains.clear();
        self.eq_gains.extend(
            self.project
                .lanes
                .iter()
                .map(|lane| eq_gains(lane.eq.low_db, lane.eq.mid_db, lane.eq.high_db)),
        );
        self.eq_gains.push(eq_gains(
            self.project.master.eq.low_db,
            self.project.master.eq.mid_db,
            self.project.master.eq.high_db,
        ));
        let channel_count = self.project.lanes.len() + 1;
        self.effect_processors.resize_with(channel_count, Vec::new);
        self.effect_processors.truncate(channel_count);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            // A bypassed channel keeps its insert settings but renders with no
            // chain at all, so the dry signal costs nothing.
            let inserts: &[_] = if lane.fx_bypass { &[] } else { &lane.inserts };
            update_effect_chain(
                channel,
                inserts,
                &mut self.effect_processors[channel],
                self.sample_rate as u32,
            );
        }
        update_effect_chain(
            self.project.lanes.len(),
            &self.project.master.inserts,
            &mut self.effect_processors[self.project.lanes.len()],
            self.sample_rate as u32,
        );
        self.rebuild_automation_index();
        self.resize_effect_parameters();
        self.retarget_smoothing();
        self.control_countdown = 0;
    }

    /// Rebuilds send edges while keeping their one-sample delay history, so
    /// editing the routing does not introduce clicks.
    pub(crate) fn rebuild_send_edges(&mut self) {
        normalize_sends(&mut self.project.sends);
        let mut previous = std::mem::take(&mut self.send_edges);
        self.send_edges.reserve(previous.len());
        for (source, row) in self.project.sends.iter().enumerate() {
            for (destination, gain) in row.iter().copied().enumerate() {
                if gain.abs() <= f32::EPSILON {
                    continue;
                }
                let delayed = previous
                    .iter()
                    .position(|edge| edge.source == source && edge.destination == destination)
                    .map_or([0.0; 2], |index| previous.swap_remove(index).delayed);
                self.send_edges.push(SendEdge {
                    source,
                    destination,
                    gain,
                    delayed,
                });
            }
        }
    }
}
