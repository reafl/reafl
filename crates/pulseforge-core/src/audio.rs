use std::{
    collections::HashMap,
    f32::consts::TAU,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};

use cpal::{
    BufferSize, SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use pulseforge_profiling::{Channel, SlotCounters, profile_scope, set_thread_channel};

use crate::{
    hosted::{HostedSlot, effect_host_id, instrument_host_id},
    plugins::{BuiltinEffect, EffectParameters},
    project::{
        AutomationSource, ClipSource, DefaultSample, EffectParameter, Instrument,
        InstrumentParameter, LfoShape, MixerParameter, ParameterTarget, PianoNote, Project,
        SignalWaveform, SynthKind, TimeUnit, TimeValue, finite_clamp, normalize_sends,
    },
    smooth::SmoothedValue,
};

const VOICE_COUNT: usize = 64;

/// Automation and instrument controls are refreshed once per control block
/// instead of once per sample. Sixteen frames is far below the audible zipper
/// threshold while cutting the automation cost by more than an order of
/// magnitude on dense projects.
const CONTROL_INTERVAL: u32 = 16;

/// Requested output buffer in frames. Small buffers lower monitoring latency;
/// the device default is used when a fixed size is not supported.
const PREFERRED_BUFFER_FRAMES: u32 = 256;

pub struct SampleData {
    pub sample_rate: u32,
    pub frames: Vec<[f32; 2]>,
}

impl SampleData {
    fn default_drum(kind: DefaultSample, sample_rate: u32) -> Option<Self> {
        let duration = match kind {
            DefaultSample::Kick => 0.48,
            DefaultSample::Snare => 0.32,
            DefaultSample::HiHat => 0.12,
            DefaultSample::None => return None,
        };
        let frame_count = (duration * sample_rate as f32) as usize;
        let mut noise_state: u32 = match kind {
            DefaultSample::Kick => 0x2a31_9d2b,
            DefaultSample::Snare => 0x5a17_02e3,
            DefaultSample::HiHat => 0x7f4a_7c15,
            DefaultSample::None => 0,
        };
        let frames = (0..frame_count)
            .map(|index| {
                let time = index as f32 / sample_rate as f32;
                let progress = time / duration;
                let signal = match kind {
                    DefaultSample::Kick => {
                        let pitch = 42.0 + 105.0 * (-time * 15.0).exp();
                        let envelope = (-time * 9.0).exp();
                        (TAU * pitch * time).sin() * envelope * 0.88
                    }
                    DefaultSample::Snare => {
                        noise_state ^= noise_state << 13;
                        noise_state ^= noise_state >> 17;
                        noise_state ^= noise_state << 5;
                        let noise = noise_state as i32 as f32 / i32::MAX as f32;
                        let envelope = (-progress * 8.5).exp();
                        let body = (TAU * 185.0 * time).sin() * (-time * 18.0).exp() * 0.42;
                        (noise * 0.68 + body) * envelope * 0.78
                    }
                    DefaultSample::HiHat => {
                        noise_state ^= noise_state << 13;
                        noise_state ^= noise_state >> 17;
                        noise_state ^= noise_state << 5;
                        let noise = noise_state as i32 as f32 / i32::MAX as f32;
                        let envelope = (-progress * 13.0).exp();
                        let high_pass = noise
                            - (noise_state.rotate_left(7) as i32 as f32 / i32::MAX as f32) * 0.55;
                        high_pass * envelope * 0.48
                    }
                    DefaultSample::None => 0.0,
                };
                [signal, signal]
            })
            .collect();
        Some(Self {
            sample_rate,
            frames,
        })
    }
}

pub enum AudioCommand {
    SetPlaying(bool),
    /// Moves the transport back to the start of the loop (or zero) without
    /// changing the play state.
    Rewind,
    /// Moves the transport to an absolute step position.
    Seek(u64),
    SetMetronome(bool),
    SetSelectedPattern(Option<usize>),
    SetLoop(Option<(u64, u64)>),
    SetTempo(f32),
    SetProject(Box<Project>),
    LoadInstrumentSample {
        instrument_id: usize,
        sample: SampleData,
    },
    /// Caches a decoded WAV by path. Clips resolve their audio from this
    /// cache, so the sample survives the project rebuild that follows an edit
    /// and can arrive before the clip itself exists.
    LoadAudioClip {
        path: String,
        sample: SampleData,
    },
    MidiNote {
        instrument_id: usize,
        note: u8,
        velocity: u8,
        on: bool,
    },
    /// Plays a one-shot sample through the master bus for auditioning.
    PreviewSample(Box<SampleData>),
    /// Enables or disables linear parameter ramping and sets its length.
    SetSmoothing {
        enabled: bool,
        milliseconds: f32,
    },
    /// Hands a real VST3/CLAP audio half to the engine so it can render there.
    LoadHosted {
        id: u64,
        audio: Box<pulseforge_hosting::HostedAudio>,
        instrument: bool,
        /// Debug counters owned by the UI's plug-in registry; the slot writes
        /// its block times there so the debug panel can list them.
        counters: Option<Arc<SlotCounters>>,
    },
    /// Drops a hosted plug-in (its editor is closed on the UI thread).
    UnloadHosted {
        id: u64,
    },
}

pub struct AudioOutput {
    pub sender: SyncSender<AudioCommand>,
    pub meters: Arc<Mutex<Vec<f32>>>,
    pub playhead_steps: Arc<AtomicU64>,
    /// Fractional playhead in steps, published as `f64` bits so the UI can
    /// animate it smoothly between audio buffers.
    pub playhead_position: Arc<AtomicU64>,
    /// Callback work as a fraction of the callback period, stored as f32 bits.
    /// This is the engine's own real-time load, the number that tells a user
    /// whether a plug-in is about to overrun the buffer.
    pub dsp_load: Arc<AtomicU32>,
    /// Frames the device hands to the callback.
    pub block_frames: Arc<AtomicU32>,
    project_updates: Arc<Mutex<Option<Box<Project>>>>,
    retired_projects: Arc<Mutex<Vec<Project>>>,
    _stream: Stream,
    pub sample_rate: u32,
}

impl AudioOutput {
    pub fn start(project: &Project) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "No default audio output device is available.".to_owned())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("Could not query the default audio format: {error}"))?;

        if supported.sample_format() != SampleFormat::F32 {
            return Err(format!(
                "The default output uses {:?}; this build currently requires f32 output.",
                supported.sample_format()
            ));
        }

        let config = supported.config();
        let sample_rate = config.sample_rate.0;
        let channels = usize::from(config.channels);
        if channels == 0 {
            return Err("The default audio device reports zero output channels.".to_owned());
        }

        let meters = Arc::new(Mutex::new(vec![0.0; project.lanes.len() + 1]));
        let project_updates = Arc::new(Mutex::new(None));
        let retired_projects = Arc::new(Mutex::new(Vec::new()));
        let playhead_steps = Arc::new(AtomicU64::new(0));
        let playhead_position = Arc::new(AtomicU64::new(0.0_f64.to_bits()));
        let dsp_load = Arc::new(AtomicU32::new(0));
        let block_frames = Arc::new(AtomicU32::new(0));
        // A small fixed buffer keeps monitoring latency low. The device default
        // is used when the backend refuses that size.
        let mut sender = None;
        let mut build = |buffer_size: BufferSize| -> Result<Stream, String> {
            let (stream_sender, receiver) = mpsc::sync_channel(512);
            let callback_meters = Arc::clone(&meters);
            let callback_project_updates = Arc::clone(&project_updates);
            let callback_retired_projects = Arc::clone(&retired_projects);
            let callback_playhead_steps = Arc::clone(&playhead_steps);
            let callback_playhead_position = Arc::clone(&playhead_position);
            let callback_dsp_load = Arc::clone(&dsp_load);
            let callback_block_frames = Arc::clone(&block_frames);
            let mut engine = AudioEngine::new(project.clone(), sample_rate);
            let mut stream_config = config.clone();
            stream_config.buffer_size = buffer_size;
            let stream = device
                .build_output_stream(
                    &stream_config,
                    move |output: &mut [f32], _| {
                        // The callback owns this thread, so the profiler is told
                        // once per buffer which channel the spans belong to.
                        set_thread_channel(Channel::Audio);
                        profile_scope!("audio callback");
                        let started = Instant::now();
                        for frame in output.chunks_mut(channels) {
                            let stereo = engine.render_frame(&receiver, &callback_project_updates);
                            match frame {
                                [left, right, rest @ ..] => {
                                    *left = stereo[0];
                                    *right = stereo[1];
                                    rest.fill(0.0);
                                }
                                [mono] => *mono = (stereo[0] + stereo[1]) * 0.5,
                                [] => {}
                            }
                        }
                        // Load against the real-time budget of this buffer: the
                        // single most useful audio number when a plug-in is hot.
                        let frames = output.len() / channels.max(1);
                        let period = frames as f32 / sample_rate as f32;
                        let load = started.elapsed().as_secs_f32() / period.max(f32::EPSILON);
                        callback_dsp_load.store(load.to_bits(), Ordering::Relaxed);
                        callback_block_frames.store(frames as u32, Ordering::Relaxed);
                        if let Ok(mut published) = callback_meters.try_lock() {
                            published.clone_from(&engine.meter_levels);
                        }
                        if let Some(retired) = engine.retired_project.take()
                            && let Ok(mut pool) = callback_retired_projects.try_lock()
                        {
                            pool.push(*retired);
                        }
                        callback_playhead_steps.store(engine.position_steps, Ordering::Relaxed);
                        callback_playhead_position
                            .store(engine.playhead_position().to_bits(), Ordering::Relaxed);
                    },
                    |error| eprintln!("Audio stream error: {error}"),
                    Some(Duration::from_millis(100)),
                )
                .map_err(|error| format!("Could not create the audio output stream: {error}"))?;
            sender = Some(stream_sender);
            Ok(stream)
        };
        let stream = match build(BufferSize::Fixed(PREFERRED_BUFFER_FRAMES)) {
            Ok(stream) => stream,
            Err(_) => build(BufferSize::Default)?,
        };
        stream
            .play()
            .map_err(|error| format!("Could not start the audio output stream: {error}"))?;
        let Some(sender) = sender else {
            return Err("The audio output stream did not start.".to_owned());
        };

        Ok(Self {
            sender,
            meters,
            playhead_steps,
            playhead_position,
            dsp_load,
            block_frames,
            project_updates,
            retired_projects,
            _stream: stream,
            sample_rate,
        })
    }

    /// Latest callback load as a fraction of the buffer period.
    pub fn load(&self) -> f32 {
        f32::from_bits(self.dsp_load.load(Ordering::Relaxed))
    }

    /// Frames per callback the device is currently using.
    pub fn block_frames(&self) -> u32 {
        self.block_frames.load(Ordering::Relaxed)
    }

    pub fn set_project(&self, project: Project) -> Result<(), String> {
        let mut pending = self
            .project_updates
            .lock()
            .map_err(|_| "The pending audio project lock is poisoned.".to_owned())?;
        *pending = Some(Box::new(project));
        Ok(())
    }

    /// Drops replaced project snapshots on the caller's thread so the audio
    /// callback never pays for freeing large buffers.
    pub fn collect_retired_projects(&self) {
        if let Ok(mut pool) = self.retired_projects.try_lock() {
            pool.clear();
        }
    }
}

struct Voice {
    instrument_id: usize,
    mixer_lane: usize,
    note: u8,
    phase: f32,
    second_phase: f32,
    sub_phase: f32,
    filter_state: f32,
    age_seconds: f32,
    velocity: f32,
    envelope: f32,
    active: bool,
    released: bool,
    release_at_tick: Option<u64>,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            instrument_id: 0,
            mixer_lane: 0,
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
struct InstrumentControls {
    gain: f32,
    pan: f32,
    cutoff_hz: f32,
    resonance: f32,
    pulse_width: f32,
    sub_level: f32,
    tone: f32,
    decay: f32,
    fm_ratio: f32,
    fm_index: f32,
}

#[derive(Clone, Copy, Default)]
struct AutomationRuntime {
    start_frame: Option<u64>,
    release_frame: Option<u64>,
    release_level: f32,
}

/// One scheduled arpeggiator note.
#[derive(Clone, Copy)]
struct ArpEvent {
    at_tick: u64,
    release_tick: u64,
    instrument_id: usize,
    lane_id: usize,
    pitch: u8,
    velocity: u8,
}

struct SampleVoice {
    instrument_id: usize,
    mixer_lane: usize,
    cursor: f64,
    active: bool,
}

impl Default for SampleVoice {
    fn default() -> Self {
        Self {
            instrument_id: 0,
            mixer_lane: 0,
            cursor: 0.0,
            active: false,
        }
    }
}

struct AudioClipRuntime {
    /// Shared by every clip that references the same file, so sliced or
    /// duplicated audio clips keep playing without re-reading the WAV.
    sample: Option<Arc<SampleData>>,
    cursor: f64,
    active: bool,
    /// Set when the clip has played to its end. Cleared when the transport
    /// leaves the clip, so re-entering it plays again.
    finished: bool,
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
enum TriggerEvent {
    InstrumentHit {
        instrument_id: usize,
        lane_id: usize,
    },
    Note {
        note: PianoNote,
        lane_id: usize,
    },
    AudioClip {
        lane_id: usize,
        clip_index: usize,
    },
}

/// One insert slot's processor. Built-in effects run their DSP; external
/// plug-in bundles run through the hosting boundary, which passes audio through
/// until a format backend is installed.
enum EffectProcessor {
    Builtin(BuiltinEffect),
    /// Hosted VST3/CLAP insert, addressed by host id in `AudioEngine::hosted`.
    External(u64),
}

impl EffectProcessor {
    /// Identity used to decide whether the processor must be rebuilt.
    fn identity(&self) -> (&'static str, &str) {
        match self {
            Self::Builtin(effect) => ("builtin", effect.effect_name()),
            Self::External(_) => ("external", ""),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct EqState {
    low_250: [f32; 2],
    low_2k: [f32; 2],
}

#[derive(Clone, Copy)]
struct SendEdge {
    source: usize,
    destination: usize,
    gain: f32,
    delayed: [f32; 2],
}

struct AudioEngine {
    sample_rate: f32,
    playing: bool,
    position_steps: u64,
    frames_into_step: f64,
    last_triggered_tick: Option<u64>,
    selected_pattern: Option<usize>,
    loop_range: Option<(u64, u64)>,
    project: Project,
    instrument_samples: Vec<Option<SampleData>>,
    audio_clips: Vec<Vec<AudioClipRuntime>>,
    send_edges: Vec<SendEdge>,
    lane_inputs: Vec<[f32; 2]>,
    meter_levels: Vec<f32>,
    eq_states: Vec<EqState>,
    eq_gains: Vec<[f32; 3]>,
    effect_processors: Vec<Vec<Option<EffectProcessor>>>,
    eq_alpha_250: f32,
    eq_alpha_2k: f32,
    trigger_events: Vec<TriggerEvent>,
    releasing_instruments: Vec<usize>,
    voices: [Voice; VOICE_COUNT],
    sample_voices: [SampleVoice; VOICE_COUNT],
    instrument_controls: Vec<InstrumentControls>,
    automation_runtime: Vec<AutomationRuntime>,
    /// Automation lane indices grouped by the parameter they drive, so
    /// evaluation only walks lanes that actually match the target.
    automation_index: HashMap<ParameterTarget, Vec<usize>>,
    /// Effect parameters cached for the current control block.
    effect_parameters: Vec<Vec<EffectParameters>>,
    control_countdown: u32,
    /// Linear ramps that keep parameter jumps from producing zipper noise.
    lane_gain: Vec<SmoothedValue>,
    lane_pan: Vec<SmoothedValue>,
    master_gain: SmoothedValue,
    master_pan: SmoothedValue,
    effect_mix: Vec<Vec<SmoothedValue>>,
    smoothing_enabled: bool,
    smoothing_milliseconds: f32,
    smoothing_samples: u32,
    /// Absolute-tick arpeggiator events produced from played notes.
    arp_queue: Vec<ArpEvent>,
    /// Cleared when the project changes so the next control block snaps instead
    /// of ramping from stale values.
    controls_snapped: bool,
    preview: Option<Box<SampleData>>,
    preview_cursor: f64,
    metronome: bool,
    click_frames_left: u64,
    click_length_frames: u64,
    click_frequency: f32,
    click_phase: f32,
    rendered_frames: u64,
    retired_project: Option<Box<Project>>,
    /// Decoded audio clips keyed by file path, shared by every clip that uses
    /// the same file.
    clip_samples: HashMap<String, Arc<SampleData>>,
    /// Live VST3/CLAP instances, keyed by host id (see `hosted::*_host_id`).
    hosted: HashMap<u64, HostedSlot>,
    /// Instrument ids that currently have a hosted generator.
    hosted_instruments: Vec<usize>,
}

impl AudioEngine {
    fn new(mut project: Project, sample_rate: u32) -> Self {
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
    fn apply_smoothing_settings(&mut self) {
        let ramp = self.smoothing_samples.max(1);
        for value in self
            .lane_gain
            .iter_mut()
            .chain(self.lane_pan.iter_mut())
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
    fn retarget_smoothing(&mut self) {
        let ramp = if self.smoothing_enabled {
            self.smoothing_samples.max(1)
        } else {
            1
        };
        let lane_count = self.project.lanes.len();
        let mut gains = Vec::with_capacity(lane_count);
        let mut pans = Vec::with_capacity(lane_count);
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
            gain = gain.with_ramp(ramp);
            pan = pan.with_ramp(ramp);
            gain.set_target(lane.gain);
            pan.set_target(lane.pan);
            gains.push(gain);
            pans.push(pan);
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
        self.effect_mix = mixes;
        self.master_gain = self.master_gain.with_ramp(ramp);
        self.master_gain.set_target(self.project.master.gain);
        self.master_pan = self.master_pan.with_ramp(ramp);
        self.master_pan.set_target(self.project.master.pan);
    }

    fn resize_routing(&mut self) {
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
    fn rebuild_automation_index(&mut self) {
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

    fn resize_effect_parameters(&mut self) {
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
    fn refresh_mix_state(&mut self) {
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
    fn rebuild_send_edges(&mut self) {
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

    fn update_instrument_controls(&mut self, position: f64) {
        self.instrument_controls.resize(
            self.project.instruments.len(),
            InstrumentControls::default(),
        );
        for instrument_id in 0..self.project.instruments.len() {
            let instrument = &self.project.instruments[instrument_id];
            let mut values = instrument_controls(instrument, self.project.bpm);
            let automated = |parameter, base_value| {
                self.automated_value(
                    ParameterTarget::Instrument {
                        instrument_id,
                        parameter,
                    },
                    base_value,
                    position,
                )
            };
            values.gain = automated(InstrumentParameter::Gain, values.gain);
            values.pan = automated(InstrumentParameter::Pan, values.pan);
            values.cutoff_hz = automated(InstrumentParameter::CutoffHz, values.cutoff_hz);
            values.resonance = automated(InstrumentParameter::Resonance, values.resonance);
            values.pulse_width = automated(InstrumentParameter::PulseWidth, values.pulse_width);
            values.sub_level = automated(InstrumentParameter::SubLevel, values.sub_level);
            values.tone = automated(InstrumentParameter::Tone, values.tone);
            values.decay = automated(InstrumentParameter::Decay, values.decay);
            values.fm_ratio = automated(InstrumentParameter::FmRatio, values.fm_ratio);
            values.fm_index = automated(InstrumentParameter::FmIndex, values.fm_index);
            // Crossfade towards the new control set so knob moves and
            // automation ramps are audible as ramps, not steps. The blend
            // factor is derived from the same smoothing time as the gains.
            if self.smoothing_enabled && self.controls_snapped {
                let previous = self.instrument_controls[instrument_id];
                let blend = self.control_smoothing_blend();
                values = InstrumentControls {
                    gain: blend(previous.gain, values.gain),
                    pan: blend(previous.pan, values.pan),
                    cutoff_hz: blend(previous.cutoff_hz, values.cutoff_hz),
                    resonance: blend(previous.resonance, values.resonance),
                    pulse_width: blend(previous.pulse_width, values.pulse_width),
                    sub_level: blend(previous.sub_level, values.sub_level),
                    tone: blend(previous.tone, values.tone),
                    decay: blend(previous.decay, values.decay),
                    fm_ratio: blend(previous.fm_ratio, values.fm_ratio),
                    fm_index: blend(previous.fm_index, values.fm_index),
                };
            }
            self.instrument_controls[instrument_id] = values;
        }
        self.controls_snapped = true;
    }

    /// Applies automation for targets that are not instrument/insert knobs:
    /// hosted plug-in parameters, mixer volume/pan and the tempo. These are
    /// pushed straight to their owner instead of being read by the renderer.
    fn apply_global_automation(&mut self, position: f64) {
        for index in 0..self.project.automation_tracks.len() {
            let (enabled, target) = {
                let track = &self.project.automation_tracks[index];
                (track.enabled, track.target)
            };
            if !enabled {
                continue;
            }
            match target {
                ParameterTarget::PluginParameter {
                    host_id,
                    parameter_id,
                } => {
                    let base = self
                        .hosted
                        .get(&host_id)
                        .map_or(0.5, |slot| slot.parameter_base(parameter_id));
                    let value = self.automated_value(target, base, position);
                    if let Some(slot) = self.hosted.get_mut(&host_id) {
                        slot.set_parameter(parameter_id, value);
                    }
                }
                ParameterTarget::MixerChannel {
                    channel_id,
                    parameter,
                } => {
                    let base = self.project.parameter_value(target).unwrap_or(
                        if matches!(parameter, MixerParameter::Pan) {
                            0.0
                        } else {
                            0.8
                        },
                    );
                    let value = self.automated_value(target, base, position);
                    match parameter {
                        MixerParameter::Gain => {
                            if let Some(gain) = self.lane_gain.get_mut(channel_id) {
                                gain.set_target(value);
                            }
                        }
                        MixerParameter::Pan => {
                            if let Some(pan) = self.lane_pan.get_mut(channel_id) {
                                pan.set_target(value);
                            }
                        }
                    }
                }
                ParameterTarget::Tempo => {
                    let value = self.automated_value(target, self.project.bpm, position);
                    self.project.bpm = finite_clamp(value, 20.0, 300.0, 120.0);
                }
                ParameterTarget::Instrument { .. } | ParameterTarget::InsertEffect { .. } => {}
            }
        }
    }

    /// Per-control-block interpolation factor for parameters that are not
    /// driven by a `SmoothedValue`. One step of the control block relative to
    /// the configured smoothing time, clamped to a full jump.
    fn control_smoothing_blend(&self) -> impl Fn(f32, f32) -> f32 {
        let ramp = self.smoothing_samples.max(1) as f32;
        let amount = (CONTROL_INTERVAL as f32 / ramp).clamp(0.0, 1.0);
        move |current: f32, target: f32| current + (target - current) * amount
    }

    fn automated_value(&self, target: ParameterTarget, base_value: f32, position: f64) -> f32 {
        let Some(lane_ids) = self.automation_index.get(&target) else {
            return base_value;
        };
        let base_normalized = target.normalize_value(base_value);
        let mut curve_total: Option<f32> = None;
        let mut relative_total = 0.0;
        let mut additive = 0.0;
        for &track_id in lane_ids {
            let Some(track) = self.project.automation_tracks.get(track_id) else {
                continue;
            };
            if position < track.start_step as f64
                || position >= track.start_step.saturating_add(track.length_steps) as f64
            {
                continue;
            }
            if track.source.edits_points() {
                if track.points.is_empty() {
                    continue;
                }
                let value = Project::evaluate_curve(&track.points, position);
                // Relative lanes offset the parameter; their points may be
                // negative, so a lane can pull a value down as well as up.
                if track.source.is_relative() {
                    relative_total += value;
                    continue;
                }
                match &mut curve_total {
                    Some(total) => *total += value - track.points[0].value,
                    None => curve_total = Some(value),
                }
                continue;
            }
            let delta = match &track.source {
                AutomationSource::Curve | AutomationSource::RelativeCurve => 0.0,
                AutomationSource::MidiEnvelope {
                    instrument_id,
                    attack,
                    decay,
                    sustain,
                    release,
                    base,
                    depth,
                } => {
                    let level = self.midi_envelope_level(
                        track_id,
                        *instrument_id,
                        *attack,
                        *decay,
                        *sustain,
                        *release,
                    );
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + level * depth * multiplier - base_normalized
                }
                AutomationSource::Lfo {
                    shape,
                    rate,
                    retrigger,
                    base,
                    depth,
                } => {
                    let phase = self.lfo_phase(track_id, *rate, *retrigger, position);
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + lfo_wave(*shape, phase) * depth * multiplier - base_normalized
                }
                AutomationSource::SampleAndHold {
                    seed,
                    rate,
                    base,
                    depth,
                } => {
                    // One held value per rate cycle, seeded so two lanes read
                    // different noise.
                    let phase = self.lfo_phase(track_id, *rate, false, position);
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + sample_and_hold(phase, *seed) * depth * multiplier - base_normalized
                }
                AutomationSource::Perlin {
                    seed,
                    rate,
                    octaves,
                    base,
                    depth,
                } => {
                    let local_position = (position - track.start_step as f64).max(0.0);
                    let coordinate = match rate.unit {
                        TimeUnit::Steps => local_position / f64::from(rate.amount.max(0.001)),
                        TimeUnit::Milliseconds => {
                            let seconds_per_step = 60.0 / f64::from(self.project.bpm) / 4.0;
                            local_position * seconds_per_step
                                / f64::from((rate.amount / 1_000.0).max(0.001))
                        }
                    };
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + crate::project::perlin_noise_octaves(*seed, coordinate, *octaves)
                        * depth
                        * multiplier
                        - base_normalized
                }
            };
            additive += delta;
        }
        let normalized = curve_total.unwrap_or(base_normalized) + relative_total + additive;
        target.denormalize_value(normalized.clamp(0.0, 1.0))
    }

    fn lfo_phase(&self, track_id: usize, rate: TimeValue, retrigger: bool, position: f64) -> f32 {
        let elapsed = if retrigger {
            let start = self
                .automation_runtime
                .get(track_id)
                .and_then(|state| state.start_frame)
                .unwrap_or(self.rendered_frames);
            self.rendered_frames.saturating_sub(start) as f64 / f64::from(self.sample_rate)
        } else {
            self.rendered_frames as f64 / f64::from(self.sample_rate)
        };
        let cycle_seconds = rate.seconds(self.project.bpm).max(0.001) as f64;
        let cycles = match rate.unit {
            TimeUnit::Steps => position.max(0.0) / rate.amount.max(0.001) as f64,
            TimeUnit::Milliseconds => elapsed / cycle_seconds,
        };
        cycles as f32
    }

    fn midi_envelope_level(
        &self,
        track_id: usize,
        instrument_id: usize,
        attack: TimeValue,
        decay: TimeValue,
        sustain: f32,
        release: TimeValue,
    ) -> f32 {
        let Some(state) = self.automation_runtime.get(track_id) else {
            return 0.0;
        };
        let Some(start_frame) = state.start_frame else {
            return 0.0;
        };
        let Some(track) = self.project.automation_tracks.get(track_id) else {
            return 0.0;
        };
        if !matches!(
            track.source,
            AutomationSource::MidiEnvelope {
                instrument_id: source_id,
                ..
            } if source_id == instrument_id
        ) {
            return 0.0;
        }
        let attack_seconds = attack.seconds(self.project.bpm).max(0.0);
        let decay_seconds = decay.seconds(self.project.bpm).max(0.0);
        let sustain = sustain.clamp(0.0, 1.0);
        let elapsed = self.rendered_frames.saturating_sub(start_frame) as f32 / self.sample_rate;
        let level = if attack_seconds > 0.0 && elapsed < attack_seconds {
            elapsed / attack_seconds
        } else if decay_seconds > 0.0 && elapsed < attack_seconds + decay_seconds {
            1.0 - (1.0 - sustain) * ((elapsed - attack_seconds) / decay_seconds)
        } else {
            sustain
        };
        let Some(release_frame) = state.release_frame else {
            return level.clamp(0.0, 1.0);
        };
        let release_elapsed =
            self.rendered_frames.saturating_sub(release_frame) as f32 / self.sample_rate;
        let release_seconds = release.seconds(self.project.bpm).max(0.001);
        (state.release_level * (1.0 - release_elapsed / release_seconds)).clamp(0.0, 1.0)
    }

    fn trigger_automation(&mut self, instrument_id: usize) {
        self.automation_runtime.resize(
            self.project.automation_tracks.len(),
            AutomationRuntime::default(),
        );
        for (track_id, track) in self.project.automation_tracks.iter().enumerate() {
            match &track.source {
                AutomationSource::MidiEnvelope {
                    instrument_id: source_id,
                    ..
                } if *source_id == instrument_id => {
                    if self.position_steps < track.start_step
                        || self.position_steps
                            >= track.start_step.saturating_add(track.length_steps)
                    {
                        continue;
                    }
                    self.automation_runtime[track_id] = AutomationRuntime {
                        start_frame: Some(self.rendered_frames),
                        ..AutomationRuntime::default()
                    };
                }
                AutomationSource::Lfo {
                    retrigger: true, ..
                } => {
                    if matches!(track.target, ParameterTarget::Instrument { instrument_id: id, .. } if id == instrument_id)
                    {
                        self.automation_runtime[track_id] = AutomationRuntime {
                            start_frame: Some(self.rendered_frames),
                            ..AutomationRuntime::default()
                        };
                    }
                }
                _ => {}
            }
        }
    }

    fn release_automation(&mut self, instrument_id: usize) {
        for track_id in 0..self.project.automation_tracks.len() {
            let source = self.project.automation_tracks[track_id].source.clone();
            let AutomationSource::MidiEnvelope {
                instrument_id: source_id,
                attack,
                decay,
                sustain,
                release,
                ..
            } = source
            else {
                continue;
            };
            if source_id != instrument_id {
                continue;
            }
            let level =
                self.midi_envelope_level(track_id, instrument_id, attack, decay, sustain, release);
            let state = &mut self.automation_runtime[track_id];
            state.release_frame = Some(self.rendered_frames);
            state.release_level = level;
        }
    }

    fn automate_effect_parameters(
        &self,
        channel_id: usize,
        slot_id: usize,
        mut parameters: EffectParameters,
        position: f64,
    ) -> EffectParameters {
        let automated = |parameter, value| {
            self.automated_value(
                ParameterTarget::InsertEffect {
                    channel_id,
                    slot_id,
                    parameter,
                },
                value,
                position,
            )
        };
        parameters.mix = automated(EffectParameter::Mix, parameters.mix);
        parameters.amount = automated(EffectParameter::Amount, parameters.amount);
        parameters.low_db = automated(EffectParameter::LowDb, parameters.low_db);
        parameters.mid_db = automated(EffectParameter::MidDb, parameters.mid_db);
        parameters.high_db = automated(EffectParameter::HighDb, parameters.high_db);
        parameters.time_seconds = automated(EffectParameter::TimeSeconds, parameters.time_seconds);
        parameters.feedback = automated(EffectParameter::Feedback, parameters.feedback);
        parameters.rate_hz = automated(EffectParameter::RateHz, parameters.rate_hz);
        parameters.depth = automated(EffectParameter::Depth, parameters.depth);
        parameters.room_size = automated(EffectParameter::RoomSize, parameters.room_size);
        parameters.drive = automated(EffectParameter::Drive, parameters.drive);
        parameters
    }

    /// Refreshes the cached insert-effect parameters for the current control
    /// block, so the sample loop only copies values it already has.
    fn update_effect_parameters(&mut self, position: f64) {
        let lane_count = self.project.lanes.len();
        for channel in 0..=lane_count {
            let slot_count = self.effect_parameters[channel].len();
            for slot_id in 0..slot_count {
                let parameters = {
                    let slot = if channel == lane_count {
                        &self.project.master.inserts[slot_id]
                    } else {
                        &self.project.lanes[channel].inserts[slot_id]
                    };
                    let mut parameters = slot.effect_parameters();
                    parameters.time_seconds = slot.time_seconds_at_tempo(self.project.bpm);
                    parameters
                };
                let parameters = if self.playing {
                    self.automate_effect_parameters(channel, slot_id, parameters, position)
                } else {
                    parameters
                };
                // Smooth effect knobs the same way as instrument controls.
                let parameters = if self.smoothing_enabled && self.controls_snapped {
                    let previous = self.effect_parameters[channel][slot_id];
                    let blend = self.control_smoothing_blend();
                    EffectParameters {
                        mix: blend(previous.mix, parameters.mix),
                        amount: blend(previous.amount, parameters.amount),
                        low_db: blend(previous.low_db, parameters.low_db),
                        mid_db: blend(previous.mid_db, parameters.mid_db),
                        high_db: blend(previous.high_db, parameters.high_db),
                        time_seconds: blend(previous.time_seconds, parameters.time_seconds),
                        feedback: blend(previous.feedback, parameters.feedback),
                        rate_hz: blend(previous.rate_hz, parameters.rate_hz),
                        depth: blend(previous.depth, parameters.depth),
                        room_size: blend(previous.room_size, parameters.room_size),
                        drive: blend(previous.drive, parameters.drive),
                    }
                } else {
                    parameters
                };
                self.effect_parameters[channel][slot_id] = parameters;
            }
        }
    }

    /// Runs one insert slot. Hosted inserts render through their block-buffered
    /// slot, built-in ones run their own DSP.
    fn run_insert(&mut self, lane_id: usize, slot_id: usize, parameters: EffectParameters) {
        let Some(processor) = self
            .effect_processors
            .get_mut(lane_id)
            .and_then(|slots| slots.get_mut(slot_id))
        else {
            return;
        };
        match processor {
            Some(EffectProcessor::External(host_id)) => {
                let id = *host_id;
                let frame = self.lane_inputs[lane_id];
                if let Some(hosted) = self.hosted.get_mut(&id) {
                    self.lane_inputs[lane_id] = hosted.render_effect(frame);
                }
            }
            Some(EffectProcessor::Builtin(effect)) => {
                let frame = self.lane_inputs[lane_id];
                self.lane_inputs[lane_id] = effect.process_with_parameters(frame, parameters);
            }
            None => {}
        }
    }

    /// Mixes every hosted generator into its instrument's mixer lane.
    fn render_hosted_instruments(&mut self, lane_count: usize) {
        for index in 0..self.hosted_instruments.len() {
            let instrument_id = self.hosted_instruments[index];
            if !self.instrument_audible(instrument_id) {
                continue;
            }
            let Some(controls) = self.instrument_controls.get(instrument_id).copied() else {
                continue;
            };
            let id = instrument_host_id(instrument_id);
            let Some(frame) = self.hosted.get_mut(&id).map(HostedSlot::render_instrument) else {
                continue;
            };
            let lane = self
                .project
                .instruments
                .get(instrument_id)
                .map_or(0, |instrument| instrument.default_mixer_lane)
                .min(lane_count.saturating_sub(1));
            let left_gain = ((1.0 - controls.pan) * 0.5).sqrt() * controls.gain;
            let right_gain = ((1.0 + controls.pan) * 0.5).sqrt() * controls.gain;
            self.lane_inputs[lane][0] += frame[0] * left_gain;
            self.lane_inputs[lane][1] += frame[1] * right_gain;
        }
    }

    fn render_frame(
        &mut self,
        receiver: &mpsc::Receiver<AudioCommand>,
        project_updates: &Mutex<Option<Box<Project>>>,
    ) -> [f32; 2] {
        self.apply_pending(receiver, project_updates);
        let automation_position =
            self.position_steps as f64 + self.frames_into_step / self.frames_per_step().max(1.0);
        if self.control_countdown == 0 {
            if self.playing {
                self.update_instrument_controls(automation_position);
                self.apply_global_automation(automation_position);
            } else {
                self.instrument_controls.resize(
                    self.project.instruments.len(),
                    InstrumentControls::default(),
                );
                let bpm = self.project.bpm;
                for (controls, instrument) in self
                    .instrument_controls
                    .iter_mut()
                    .zip(&self.project.instruments)
                {
                    *controls = instrument_controls(instrument, bpm);
                }
            }
            self.update_effect_parameters(automation_position);
        }
        self.control_countdown += 1;
        if self.control_countdown >= CONTROL_INTERVAL {
            self.control_countdown = 0;
        }
        if self.playing {
            let position_tick = self.position_steps.saturating_mul(16)
                + ((self.frames_into_step / self.frames_per_step().max(1.0)) * 16.0).floor() as u64;
            if self.last_triggered_tick != Some(position_tick) {
                self.trigger_step(position_tick);
                self.last_triggered_tick = Some(position_tick);
            }
        }

        let lane_count = self.project.lanes.len();
        let master_index = lane_count;
        self.lane_inputs.fill([0.0; 2]);
        for edge in &self.send_edges {
            for channel in 0..2 {
                self.lane_inputs[edge.destination][channel] += edge.delayed[channel];
            }
        }

        for lane_id in 0..lane_count {
            let audio_samples = self.next_audio_clips(lane_id);
            self.lane_inputs[lane_id][0] += audio_samples[0];
            self.lane_inputs[lane_id][1] += audio_samples[1];
        }

        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let Some(instrument) = self.project.instruments.get(voice.instrument_id) else {
                voice.active = false;
                continue;
            };
            if !instrument.synth_enabled || !instrument.enabled {
                voice.active = false;
                continue;
            }
            if self.project.instrument_solo_active() && !instrument.solo {
                voice.active = false;
                continue;
            }
            let controls = self.instrument_controls[voice.instrument_id];
            let frequency = 440.0 * 2.0_f32.powf((f32::from(voice.note) - 69.0) / 12.0);
            let raw = match instrument.synth_kind {
                SynthKind::SamplePlayer | SynthKind::External => 0.0,
                SynthKind::Sine => voice.phase.sin(),
                SynthKind::SignalGenerator => waveform(instrument.signal_waveform, voice.phase),
                SynthKind::Ms20 => {
                    let oscillator_a = waveform(SignalWaveform::Saw, voice.phase);
                    let oscillator_b = waveform(SignalWaveform::Saw, voice.second_phase);
                    (oscillator_a + oscillator_b * 0.65) * 0.6
                }
                SynthKind::JunoPoly => {
                    let saw = waveform(SignalWaveform::Saw, voice.phase);
                    let pulse = if (voice.phase / TAU).fract() < controls.pulse_width {
                        1.0
                    } else {
                        -1.0
                    };
                    saw * 0.45
                        + pulse * 0.35
                        + waveform(SignalWaveform::Square, voice.sub_phase) * controls.sub_level
                }
                SynthKind::M1Workstation => {
                    let brightness = controls.tone;
                    voice.phase.sin() * 0.65
                        + (voice.phase * 2.0).sin() * (0.25 + brightness * 0.2)
                        + (voice.phase * 3.0).sin() * brightness * 0.1
                }
                SynthKind::FmSynth => {
                    (voice.phase + voice.second_phase.sin() * controls.fm_index * 10.0).sin()
                }
            };
            let signal = if matches!(instrument.synth_kind, SynthKind::Ms20 | SynthKind::JunoPoly) {
                let cutoff = controls.cutoff_hz.min(self.sample_rate * 0.45);
                let alpha = 1.0 - (-TAU * cutoff / self.sample_rate).exp();
                let resonant_input = raw - voice.filter_state * controls.resonance;
                voice.filter_state += alpha * (resonant_input - voice.filter_state);
                let emphasis = if instrument.synth_kind == SynthKind::Ms20 {
                    2.2
                } else {
                    1.5
                };
                (voice.filter_state * emphasis).tanh()
            } else {
                raw
            };
            let decay = if instrument.synth_kind == SynthKind::M1Workstation {
                (-voice.age_seconds / controls.decay).exp()
            } else {
                1.0
            };
            let output = if instrument.synth_kind == SynthKind::SamplePlayer {
                0.0
            } else {
                signal * voice.velocity * voice.envelope * controls.gain * decay * 0.12
            };
            voice.phase = (voice.phase + TAU * frequency / self.sample_rate) % TAU;
            let second_frequency = frequency
                * match instrument.synth_kind {
                    SynthKind::Ms20 => 1.006,
                    SynthKind::FmSynth => controls.fm_ratio,
                    _ => 1.0,
                };
            voice.second_phase =
                (voice.second_phase + TAU * second_frequency / self.sample_rate) % TAU;
            voice.sub_phase = (voice.sub_phase + TAU * frequency * 0.5 / self.sample_rate) % TAU;
            voice.age_seconds += 1.0 / self.sample_rate;
            if voice.released {
                voice.envelope *= 0.9992;
                if voice.envelope < 0.0005 {
                    voice.active = false;
                }
            } else {
                voice.envelope = (voice.envelope + 0.0015).min(1.0);
            }
            let pan = controls.pan;
            let left_gain = ((1.0 - pan) * 0.5).sqrt();
            let right_gain = ((1.0 + pan) * 0.5).sqrt();
            let lane = voice.mixer_lane.min(lane_count.saturating_sub(1));
            self.lane_inputs[lane][0] += output * left_gain;
            self.lane_inputs[lane][1] += output * right_gain;
        }

        for voice in &mut self.sample_voices {
            if !voice.active {
                continue;
            }
            let Some(sample) = self
                .instrument_samples
                .get(voice.instrument_id)
                .and_then(Option::as_ref)
            else {
                voice.active = false;
                continue;
            };
            let sample_frame = interpolate_sample(sample, voice.cursor);
            voice.cursor += sample.sample_rate as f64 / self.sample_rate as f64;
            if voice.cursor >= sample.frames.len() as f64 {
                voice.active = false;
            } else {
                let lane = voice.mixer_lane.min(lane_count.saturating_sub(1));
                let controls = self.instrument_controls[voice.instrument_id];
                let left_gain = ((1.0 - controls.pan) * 0.5).sqrt() * controls.gain;
                let right_gain = ((1.0 + controls.pan) * 0.5).sqrt() * controls.gain;
                self.lane_inputs[lane][0] += sample_frame[0] * left_gain;
                self.lane_inputs[lane][1] += sample_frame[1] * right_gain;
            }
        }

        self.render_hosted_instruments(lane_count);

        for lane_id in 0..lane_count {
            if !self.lane_audible(lane_id) {
                self.lane_inputs[lane_id] = [0.0; 2];
                continue;
            }
            let gain = self.lane_gain[lane_id].advance();
            let pan = self.lane_pan[lane_id].advance();
            let left_gain = (1.0 - pan).clamp(0.0, 1.0);
            let right_gain = (1.0 + pan).clamp(0.0, 1.0);
            self.lane_inputs[lane_id][0] *= gain * left_gain;
            self.lane_inputs[lane_id][1] *= gain * right_gain;
            // Stereo width is a mid/side scale, applied before the EQ so the
            // shelves react to the widened signal.
            let width = self.project.lanes[lane_id].width;
            if (width - 1.0).abs() > 1e-4 {
                let frame = self.lane_inputs[lane_id];
                let mid = (frame[0] + frame[1]) * 0.5;
                let side = (frame[0] - frame[1]) * 0.5 * width;
                self.lane_inputs[lane_id] = [mid + side, mid - side];
            }
            self.lane_inputs[lane_id] = process_eq(
                self.lane_inputs[lane_id],
                &mut self.eq_states[lane_id],
                self.eq_gains[lane_id],
                self.eq_alpha_250,
                self.eq_alpha_2k,
            );
            for slot_id in 0..self.effect_processors[lane_id].len() {
                let mut parameters = self.effect_parameters[lane_id][slot_id];
                if let Some(mix) = self
                    .effect_mix
                    .get_mut(lane_id)
                    .and_then(|slots| slots.get_mut(slot_id))
                {
                    parameters.mix = mix.advance();
                }
                self.run_insert(lane_id, slot_id, parameters);
            }
        }

        if let Some(sample) = self.preview.as_ref() {
            let frame = interpolate_sample(sample, self.preview_cursor);
            self.preview_cursor += sample.sample_rate as f64 / self.sample_rate as f64;
            if self.preview_cursor >= sample.frames.len() as f64 {
                self.preview = None;
                self.preview_cursor = 0.0;
            }
            self.lane_inputs[master_index][0] += frame[0];
            self.lane_inputs[master_index][1] += frame[1];
        }

        if self.click_frames_left > 0 {
            let envelope = (self.click_frames_left as f32 / self.click_length_frames.max(1) as f32)
                .powi(3)
                * 0.5;
            self.click_phase =
                (self.click_phase + TAU * self.click_frequency / self.sample_rate) % TAU;
            let click = self.click_phase.sin() * envelope;
            self.lane_inputs[master_index][0] += click;
            self.lane_inputs[master_index][1] += click;
            self.click_frames_left -= 1;
        }

        let master_gain = self.master_gain.advance();
        let master_pan = self.master_pan.advance();
        let master_left = (1.0 - master_pan).clamp(0.0, 1.0);
        let master_right = (1.0 + master_pan).clamp(0.0, 1.0);
        self.lane_inputs[master_index][0] *= master_gain * master_left;
        self.lane_inputs[master_index][1] *= master_gain * master_right;
        self.lane_inputs[master_index] = process_eq(
            self.lane_inputs[master_index],
            &mut self.eq_states[master_index],
            self.eq_gains[master_index],
            self.eq_alpha_250,
            self.eq_alpha_2k,
        );
        for slot_id in 0..self.effect_processors[master_index].len() {
            let mut parameters = self.effect_parameters[master_index][slot_id];
            if let Some(mix) = self
                .effect_mix
                .get_mut(master_index)
                .and_then(|slots| slots.get_mut(slot_id))
            {
                parameters.mix = mix.advance();
            }
            self.run_insert(master_index, slot_id, parameters);
        }

        for edge in &mut self.send_edges {
            for channel in 0..2 {
                edge.delayed[channel] = self.lane_inputs[edge.source][channel] * edge.gain;
            }
        }

        for (meter, signal) in self.meter_levels.iter_mut().zip(&self.lane_inputs) {
            let peak = signal[0].abs().max(signal[1].abs()).min(1.0);
            *meter = peak.max(*meter * 0.94);
        }
        let output = [
            self.lane_inputs[master_index][0].clamp(-0.95, 0.95),
            self.lane_inputs[master_index][1].clamp(-0.95, 0.95),
        ];
        if self.playing {
            self.frames_into_step += 1.0;
            if self.frames_into_step >= self.frames_per_step() {
                self.frames_into_step = 0.0;
                self.position_steps = self.position_steps.saturating_add(1);
                if let Some((start, end)) = self.loop_range
                    && self.position_steps >= end
                {
                    self.position_steps = start;
                    self.last_triggered_tick = None;
                }
            }
        }
        self.rendered_frames = self.rendered_frames.saturating_add(1);
        output
    }

    fn apply_pending(
        &mut self,
        receiver: &mpsc::Receiver<AudioCommand>,
        project_updates: &Mutex<Option<Box<Project>>>,
    ) {
        let mut pending_project = None;
        while let Ok(command) = receiver.try_recv() {
            match command {
                AudioCommand::SetPlaying(playing) => {
                    self.playing = playing;
                    if playing {
                        if self.position_steps == 0 {
                            self.last_triggered_tick = None;
                        }
                    } else {
                        self.releasing_instruments.clear();
                        self.click_frames_left = 0;
                        for voice in &mut self.voices {
                            if voice.active && !voice.released {
                                voice.released = true;
                                voice.release_at_tick = None;
                                self.releasing_instruments.push(voice.instrument_id);
                            }
                        }
                        for voice in &mut self.sample_voices {
                            voice.active = false;
                        }
                        for hosted in self.hosted.values_mut() {
                            hosted.reset();
                        }
                        self.releasing_instruments.sort_unstable();
                        self.releasing_instruments.dedup();
                        for index in 0..self.releasing_instruments.len() {
                            let instrument_id = self.releasing_instruments[index];
                            self.release_automation(instrument_id);
                        }
                        self.releasing_instruments.clear();
                    }
                }
                AudioCommand::Rewind => {
                    self.position_steps = self.loop_range.map_or(0, |(start, _)| start);
                    self.frames_into_step = 0.0;
                    self.last_triggered_tick = None;
                    for hosted in self.hosted.values_mut() {
                        hosted.reset();
                    }
                }
                AudioCommand::Seek(position) => {
                    self.position_steps = position;
                    self.frames_into_step = 0.0;
                    self.last_triggered_tick = None;
                }
                AudioCommand::SetMetronome(enabled) => {
                    self.metronome = enabled;
                    if !enabled {
                        self.click_frames_left = 0;
                    }
                }
                AudioCommand::SetSelectedPattern(pattern_id) => {
                    self.selected_pattern =
                        pattern_id.filter(|pattern_id| *pattern_id < self.project.patterns.len());
                    self.position_steps = 0;
                    self.frames_into_step = 0.0;
                    self.last_triggered_tick = None;
                }
                AudioCommand::SetLoop(range) => {
                    self.loop_range = range.filter(|(start, end)| start < end);
                }
                AudioCommand::SetTempo(bpm) => {
                    self.project.bpm = finite_clamp(bpm, 20.0, 300.0, 120.0);
                }
                AudioCommand::SetProject(project) => pending_project = Some(project),
                AudioCommand::LoadInstrumentSample {
                    instrument_id,
                    sample,
                } => {
                    if instrument_id < self.instrument_samples.len() {
                        self.instrument_samples[instrument_id] = Some(sample);
                    }
                }
                AudioCommand::LoadAudioClip { path, sample } => {
                    self.clip_samples.insert(path, Arc::new(sample));
                }
                AudioCommand::MidiNote {
                    instrument_id,
                    note,
                    velocity,
                    on,
                } => self.handle_midi_note(instrument_id, note, velocity, on),
                AudioCommand::PreviewSample(sample) => {
                    self.preview = Some(sample);
                    self.preview_cursor = 0.0;
                }
                AudioCommand::LoadHosted {
                    id,
                    audio,
                    instrument,
                    counters,
                } => {
                    let slot = HostedSlot::new(
                        *audio,
                        instrument,
                        f64::from(self.sample_rate),
                        counters,
                    );
                    if instrument {
                        let instrument_id = id as usize;
                        if !self.hosted_instruments.contains(&instrument_id) {
                            self.hosted_instruments.push(instrument_id);
                        }
                    }
                    self.hosted.insert(id, slot);
                }
                AudioCommand::UnloadHosted { id } => {
                    if let Some(mut slot) = self.hosted.remove(&id) {
                        slot.reset();
                    }
                    self.hosted_instruments
                        .retain(|instrument_id| instrument_host_id(*instrument_id) != id);
                }
                AudioCommand::SetSmoothing {
                    enabled,
                    milliseconds,
                } => {
                    self.smoothing_enabled = enabled;
                    self.smoothing_milliseconds = finite_clamp(milliseconds, 0.0, 500.0, 12.0);
                    self.smoothing_samples =
                        (self.sample_rate * self.smoothing_milliseconds / 1_000.0) as u32;
                    self.apply_smoothing_settings();
                }
            }
        }
        if let Ok(mut latest_project) = project_updates.try_lock()
            && let Some(project) = latest_project.take()
        {
            pending_project = Some(project);
        }
        if let Some(project) = pending_project {
            self.apply_project(*project);
        }
    }

    fn apply_project(&mut self, mut project: Project) {
        project.normalize();
        let old_project = std::mem::replace(&mut self.project, project);
        self.control_countdown = 0;
        self.controls_snapped = false;
        let instruments_changed = old_project.instruments.len() != self.project.instruments.len()
            || old_project
                .instruments
                .iter()
                .zip(&self.project.instruments)
                .any(|(old, new)| {
                    old.sample_path != new.sample_path || old.default_sample != new.default_sample
                });
        if instruments_changed {
            let mut old_instrument_samples = std::mem::take(&mut self.instrument_samples);
            self.instrument_samples = self
                .project
                .instruments
                .iter()
                .enumerate()
                .map(|(instrument_id, instrument)| {
                    let unchanged = old_project
                        .instruments
                        .get(instrument_id)
                        .is_some_and(|old| {
                            old.sample_path == instrument.sample_path
                                && old.default_sample == instrument.default_sample
                        });
                    if unchanged {
                        old_instrument_samples
                            .get_mut(instrument_id)
                            .and_then(Option::take)
                    } else if instrument.sample_path.is_empty() {
                        SampleData::default_drum(instrument.default_sample, self.sample_rate as u32)
                    } else {
                        None
                    }
                })
                .collect();
        }
        // Keep live envelope/LFO trigger state so editing a knob does not restart automation.
        self.automation_runtime.resize_with(
            self.project.automation_tracks.len(),
            AutomationRuntime::default,
        );
        let clips_changed = old_project.lanes.len() != self.project.lanes.len()
            || old_project
                .lanes
                .iter()
                .zip(&self.project.lanes)
                .any(|(old, new)| {
                    old.clips.len() != new.clips.len()
                        || old
                            .clips
                            .iter()
                            .zip(&new.clips)
                            .any(|(old_clip, new_clip)| {
                                !clip_routes_match(&old_clip.source, &new_clip.source)
                            })
                });
        if clips_changed {
            // Clips resolve their audio from the path cache, so a rebuild never
            // loses a sample regardless of the order commands arrived in. The
            // previous runtimes are a fallback for callers that attach audio
            // directly.
            let mut previous: Vec<(String, Arc<SampleData>)> = Vec::new();
            for (lane_id, old_lane) in old_project.lanes.iter().enumerate() {
                let Some(runtimes) = self.audio_clips.get(lane_id) else {
                    continue;
                };
                for (clip, runtime) in old_lane.clips.iter().zip(runtimes) {
                    if let ClipSource::Audio { path } = &clip.source
                        && let Some(sample) = &runtime.sample
                    {
                        previous.push((path.clone(), Arc::clone(sample)));
                    }
                }
            }
            self.audio_clips = self
                .project
                .lanes
                .iter()
                .map(|lane| {
                    lane.clips
                        .iter()
                        .map(|new_clip| {
                            let sample = match &new_clip.source {
                                ClipSource::Audio { path } => {
                                    self.clip_samples.get(path).map(Arc::clone).or_else(|| {
                                        previous
                                            .iter()
                                            .find(|(old_path, _)| old_path == path)
                                            .map(|(_, sample)| Arc::clone(sample))
                                    })
                                }
                                ClipSource::Pattern { .. } => None,
                            };
                            AudioClipRuntime {
                                sample,
                                cursor: 0.0,
                                active: false,
                                finished: false,
                            }
                        })
                        .collect()
                })
                .collect();
        }
        self.meter_levels.resize(self.project.lanes.len() + 1, 0.0);
        let structure_changed = old_project.lanes.len() != self.project.lanes.len()
            || old_project.instruments.len() != self.project.instruments.len()
            || old_project.patterns.len() != self.project.patterns.len()
            || old_project
                .lanes
                .iter()
                .zip(&self.project.lanes)
                .any(|(old, new)| old.clips.len() != new.clips.len());
        let sends_changed = old_project.sends != self.project.sends;
        if structure_changed {
            self.resize_routing();
        } else {
            self.refresh_mix_state();
            if sends_changed {
                self.rebuild_send_edges();
            }
        }
        // Hand the replaced snapshot back to the UI thread for deallocation.
        self.retired_project = Some(Box::new(old_project));
    }

    fn frames_per_step(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / f64::from(self.project.bpm) / 4.0
    }

    /// Fractional transport position in steps, used for smooth playhead
    /// animation on the UI thread.
    fn playhead_position(&self) -> f64 {
        self.position_steps as f64 + self.frames_into_step / self.frames_per_step().max(1.0)
    }

    fn trigger_step(&mut self, position_tick: u64) {
        let position = position_tick / 16;
        let substep = (position_tick % 16) as u8;
        if self.metronome && substep == 0 && position % 4 == 0 {
            self.click_frames_left = self.click_length_frames;
            self.click_phase = 0.0;
            self.click_frequency = if position % 16 == 0 { 1_760.0 } else { 1_320.0 };
        }
        self.releasing_instruments.clear();
        for voice in &mut self.voices {
            if voice.active && voice.release_at_tick == Some(position_tick) {
                voice.released = true;
                self.releasing_instruments.push(voice.instrument_id);
            }
        }
        self.releasing_instruments.sort_unstable();
        self.releasing_instruments.dedup();
        for index in 0..self.releasing_instruments.len() {
            let instrument_id = self.releasing_instruments[index];
            self.release_automation(instrument_id);
        }
        self.trigger_events.clear();
        if let Some(pattern_id) = self.selected_pattern {
            if let Some(pattern) = self.project.patterns.get(pattern_id) {
                let step = position % pattern.length_steps.max(1);
                if substep == 0 {
                    for hit in pattern.step_hits.iter().filter(|hit| hit.step == step) {
                        let lane_id = self
                            .project
                            .instruments
                            .get(hit.instrument_id)
                            .map_or(0, |instrument| instrument.default_mixer_lane)
                            .min(self.project.lanes.len().saturating_sub(1));
                        self.trigger_events.push(TriggerEvent::InstrumentHit {
                            instrument_id: hit.instrument_id,
                            lane_id,
                        });
                    }
                }
                let local_tick = step.saturating_mul(16) + u64::from(substep);
                for note in &pattern.notes {
                    let lane_id = self
                        .project
                        .instruments
                        .get(note.instrument_id)
                        .map_or(0, |instrument| instrument.default_mixer_lane)
                        .min(self.project.lanes.len().saturating_sub(1));
                    push_note_triggers(
                        &mut self.trigger_events,
                        note,
                        local_tick,
                        lane_id,
                        pattern.length_steps,
                    );
                }
            }
        } else {
            for (lane_id, lane) in self.project.lanes.iter().enumerate() {
                for (clip_index, clip) in lane.clips.iter().enumerate() {
                    if position < clip.start_step
                        || position >= clip.start_step.saturating_add(clip.length_steps)
                    {
                        continue;
                    }
                    let local_step = position - clip.start_step;
                    match &clip.source {
                        ClipSource::Pattern { pattern_id } => {
                            let Some(pattern) = self.project.patterns.get(*pattern_id) else {
                                continue;
                            };
                            let step = local_step % pattern.length_steps;
                            if substep == 0 {
                                for hit in pattern.step_hits.iter().filter(|hit| hit.step == step) {
                                    self.trigger_events.push(TriggerEvent::InstrumentHit {
                                        instrument_id: hit.instrument_id,
                                        lane_id,
                                    });
                                }
                            }
                            let local_tick = step.saturating_mul(16) + u64::from(substep);
                            for note in &pattern.notes {
                                push_note_triggers(
                                    &mut self.trigger_events,
                                    note,
                                    local_tick,
                                    lane_id,
                                    pattern.length_steps,
                                );
                            }
                        }
                        ClipSource::Audio { .. } => {
                            if local_step == 0 && substep == 0 {
                                self.trigger_events.push(TriggerEvent::AudioClip {
                                    lane_id,
                                    clip_index,
                                });
                            }
                        }
                    }
                }
            }
        }
        self.drain_arpeggio(position_tick);
        for index in 0..self.trigger_events.len() {
            match self.trigger_events[index] {
                TriggerEvent::InstrumentHit {
                    instrument_id,
                    lane_id,
                } => self.trigger_instrument(instrument_id, lane_id),
                TriggerEvent::Note { note, lane_id } => {
                    if self.schedule_arpeggio(&note, lane_id, position_tick) {
                        continue;
                    }
                    self.start_voice(
                        note.instrument_id,
                        lane_id,
                        note.pitch,
                        note.velocity,
                        Some(position_tick.saturating_add(note.duration_ticks())),
                    );
                }
                TriggerEvent::AudioClip {
                    lane_id,
                    clip_index,
                } => {
                    let offset_steps = self
                        .project
                        .lanes
                        .get(lane_id)
                        .and_then(|lane| lane.clips.get(clip_index))
                        .map_or(0, |clip| clip.offset_steps);
                    let seconds_per_step = 60.0 / f64::from(self.project.bpm) / 4.0;
                    if let Some(runtime) = self
                        .audio_clips
                        .get_mut(lane_id)
                        .and_then(|clips| clips.get_mut(clip_index))
                    {
                        runtime.cursor = runtime.sample.as_ref().map_or(0.0, |sample| {
                            offset_steps as f64 * seconds_per_step * f64::from(sample.sample_rate)
                        });
                        runtime.active = true;
                    }
                }
            }
        }
    }

    /// Expands a played note into arpeggiator events. Returns true when the
    /// instrument has an arpeggiator, so the original note must not sound.
    fn schedule_arpeggio(&mut self, note: &PianoNote, lane_id: usize, tick: u64) -> bool {
        let Some(instrument) = self.project.instruments.get(note.instrument_id) else {
            return false;
        };
        let arp = instrument.arpeggio;
        if !arp.enabled {
            return false;
        }
        // Convert the arp rate to ticks: seconds -> steps -> sixteenths.
        let step_ticks = (f64::from(arp.rate.seconds(self.project.bpm)) * self.sample_rate as f64
            / self.frames_per_step().max(1.0)
            * 16.0)
            .round()
            .max(1.0) as u64;
        let gate_ticks = ((step_ticks as f32 * arp.gate).round() as u64).max(1);
        let mut pitches = vec![note.pitch];
        for octave in 1..arp.octaves {
            pitches.push(note.pitch.saturating_add(12 * octave).min(127));
        }
        pitches.sort_unstable();
        pitches.dedup();
        let ordered = arp.mode.order(&pitches);
        let note_ticks = note.duration_ticks().max(step_ticks);
        let steps = (note_ticks / step_ticks).max(1);
        for step in 0..steps {
            let Some(pitch) = ordered.get(step as usize % ordered.len()).copied() else {
                continue;
            };
            let at_tick = tick.saturating_add(step.saturating_mul(step_ticks));
            self.arp_queue.push(ArpEvent {
                at_tick,
                release_tick: at_tick.saturating_add(gate_ticks),
                instrument_id: note.instrument_id,
                lane_id,
                pitch,
                velocity: note.velocity,
            });
        }
        true
    }

    /// Starts every arpeggiator note due at `tick`.
    fn drain_arpeggio(&mut self, tick: u64) {
        if self.arp_queue.is_empty() {
            return;
        }
        let mut index = 0;
        while index < self.arp_queue.len() {
            let event = self.arp_queue[index];
            if event.at_tick > tick {
                index += 1;
                continue;
            }
            self.arp_queue.swap_remove(index);
            self.start_voice(
                event.instrument_id,
                event.lane_id,
                event.pitch,
                event.velocity,
                Some(event.release_tick),
            );
        }
    }

    fn trigger_instrument(&mut self, instrument_id: usize, mixer_lane: usize) {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            return;
        };
        if !self.instrument_audible(instrument_id) {
            return;
        }
        let synth_enabled = instrument.synth_enabled;
        let synth_note = instrument.synth_note;
        if synth_enabled {
            self.start_voice(
                instrument_id,
                mixer_lane,
                synth_note,
                100,
                Some(self.position_steps.saturating_mul(16).saturating_add(16)),
            );
        } else {
            // Sample-only instruments still drive envelope and LFO automation.
            self.trigger_automation(instrument_id);
        }
        if self
            .instrument_samples
            .get(instrument_id)
            .is_some_and(Option::is_some)
        {
            let slot = self
                .sample_voices
                .iter()
                .position(|voice| !voice.active)
                .or_else(|| {
                    self.sample_voices
                        .iter()
                        .position(|voice| voice.instrument_id == instrument_id)
                });
            if let Some(slot) = slot {
                self.sample_voices[slot] = SampleVoice {
                    instrument_id,
                    mixer_lane,
                    cursor: 0.0,
                    active: true,
                };
            }
        }
    }

    fn next_audio_clips(&mut self, lane_id: usize) -> [f32; 2] {
        let mut output = [0.0; 2];
        if !self.playing {
            // A stopped transport must not keep streaming clips; rewind so the
            // next start plays from the clip head.
            for runtime in &mut self.audio_clips[lane_id] {
                runtime.active = false;
                runtime.finished = false;
                runtime.cursor = 0.0;
            }
            return output;
        }
        let clip_count = self.project.lanes[lane_id].clips.len();
        let global_gain = self.project.sample_gain;
        let global_pan = self.project.sample_pan;
        let global_pitch = self.project.sample_pitch;
        // Steps are quarter-beats, so one step is 60/bpm/4 seconds.
        let seconds_per_step = 60.0 / f64::from(self.project.bpm) / 4.0;
        let position = self.position_steps;
        for clip_index in 0..clip_count {
            let (start, length, offset, clip_gain, clip_pan, clip_pitch) =
                match &self.project.lanes[lane_id].clips[clip_index] {
                    clip if matches!(clip.source, ClipSource::Audio { .. }) => (
                        clip.start_step,
                        clip.length_steps,
                        clip.offset_steps,
                        clip.sample_gain,
                        clip.sample_pan,
                        clip.sample_pitch,
                    ),
                    _ => continue,
                };
            let gain = (global_gain * clip_gain).clamp(0.0, 1.0);
            let pan = (global_pan + clip_pan).clamp(-1.0, 1.0);
            let left_gain = ((1.0 - pan) * 0.5).sqrt() * gain;
            let right_gain = ((1.0 + pan) * 0.5).sqrt() * gain;
            // Semitone transpose is a playback-rate change.
            let rate = 2.0_f64.powf(f64::from(global_pitch + clip_pitch) / 12.0);
            let end = start.saturating_add(length);
            let runtime = &mut self.audio_clips[lane_id][clip_index];
            if position < start || position >= end {
                // Outside the clip: rewind so entering it plays from its start.
                if runtime.active || runtime.finished {
                    runtime.active = false;
                    runtime.finished = false;
                    runtime.cursor = 0.0;
                }
                continue;
            }
            if !runtime.active && !runtime.finished {
                // The transport can start inside a clip (seek, loop wrap or a
                // mid-song play), so begin at the matching sample offset rather
                // than waiting for the clip's first step.
                let Some(sample) = runtime.sample.as_ref() else {
                    continue;
                };
                let skipped_steps = offset.saturating_add(position.saturating_sub(start));
                runtime.cursor =
                    skipped_steps as f64 * seconds_per_step * f64::from(sample.sample_rate);
                runtime.active = true;
            }
            let Some(sample) = runtime.sample.as_ref() else {
                runtime.active = false;
                continue;
            };
            let frame = interpolate_sample(sample, runtime.cursor);
            output[0] += frame[0] * left_gain;
            output[1] += frame[1] * right_gain;
            runtime.cursor += sample.sample_rate as f64 / self.sample_rate as f64 * rate;
            if runtime.cursor >= sample.frames.len() as f64 {
                runtime.active = false;
                runtime.finished = true;
            }
        }
        output
    }

    /// Applies the instrument power switch and solo state.
    fn instrument_audible(&self, instrument_id: usize) -> bool {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            return false;
        };
        if !instrument.enabled {
            return false;
        }
        !self.project.instrument_solo_active() || instrument.solo
    }

    /// Applies the track power switch and solo state.
    fn lane_audible(&self, lane_id: usize) -> bool {
        let Some(lane) = self.project.lanes.get(lane_id) else {
            return false;
        };
        if !lane.enabled {
            return false;
        }
        !self.project.lane_solo_active() || lane.solo
    }

    /// Frees the oldest voice for `instrument_id` when its configured limit is
    /// reached, so a low voice count actually constrains polyphony.
    fn enforce_voice_limit(&mut self, instrument_id: usize) {
        let limit = self
            .project
            .instruments
            .get(instrument_id)
            .map_or(VOICE_COUNT, |instrument| {
                usize::from(instrument.voices.voice_count)
            })
            .clamp(1, VOICE_COUNT);
        let active = self
            .voices
            .iter()
            .filter(|voice| voice.active && voice.instrument_id == instrument_id)
            .count();
        if active < limit {
            return;
        }
        let mut to_free = active - limit + 1;
        for voice in self.voices.iter_mut() {
            if to_free == 0 {
                break;
            }
            if voice.active && voice.instrument_id == instrument_id {
                voice.active = false;
                to_free -= 1;
            }
        }
    }

    fn start_voice(
        &mut self,
        instrument_id: usize,
        mixer_lane: usize,
        note: u8,
        velocity: u8,
        release_at_tick: Option<u64>,
    ) {
        if velocity == 0 || !self.project.instruments[instrument_id].synth_enabled {
            return;
        }
        // External generators are hosted by VST3/CLAP; the note goes to the
        // hosted instance and this engine only mixes its output.
        if self.project.instruments[instrument_id].synth_kind == SynthKind::External {
            self.trigger_automation(instrument_id);
            let id = instrument_host_id(instrument_id);
            if let Some(hosted) = self.hosted.get_mut(&id) {
                hosted.play_note(note, f32::from(velocity) / 127.0);
            }
            return;
        }
        if !self.instrument_audible(instrument_id) {
            return;
        }
        self.enforce_voice_limit(instrument_id);
        self.trigger_automation(instrument_id);
        let is_monophonic = matches!(
            self.project.instruments[instrument_id].synth_kind,
            SynthKind::Ms20
        );
        let slot = if is_monophonic {
            self.voices
                .iter()
                .position(|voice| voice.active && voice.instrument_id == instrument_id)
                .or_else(|| self.voices.iter().position(|voice| !voice.active))
        } else {
            self.voices
                .iter()
                .position(|voice| {
                    voice.active
                        && voice.instrument_id == instrument_id
                        && voice.note == note
                        && voice.release_at_tick.is_none() == release_at_tick.is_none()
                })
                .or_else(|| self.voices.iter().position(|voice| !voice.active))
        };
        if let Some(slot) = slot {
            self.voices[slot] = Voice {
                instrument_id,
                mixer_lane,
                note,
                phase: 0.0,
                second_phase: 0.0,
                sub_phase: 0.0,
                filter_state: 0.0,
                age_seconds: 0.0,
                velocity: f32::from(velocity) / 127.0,
                envelope: 0.0,
                active: true,
                released: false,
                release_at_tick,
            };
        }
    }

    fn handle_midi_note(&mut self, instrument_id: usize, note: u8, velocity: u8, on: bool) {
        if instrument_id >= self.project.instruments.len() {
            return;
        }
        let lane = self.project.instruments[instrument_id].default_mixer_lane;
        if on && velocity > 0 {
            self.start_voice(instrument_id, lane, note, velocity, None);
        } else {
            self.release_automation(instrument_id);
            let id = instrument_host_id(instrument_id);
            if let Some(hosted) = self.hosted.get_mut(&id) {
                hosted.note_off(note);
            }
            for voice in &mut self.voices {
                if voice.active
                    && voice.instrument_id == instrument_id
                    && voice.note == note
                    && voice.release_at_tick.is_none()
                {
                    voice.released = true;
                }
            }
        }
    }
}

fn update_effect_chain(
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

fn instrument_controls(instrument: &Instrument, bpm: f32) -> InstrumentControls {
    let (cutoff_hz, resonance) = match instrument.synth_kind {
        SynthKind::Ms20 => (instrument.ms20_cutoff_hz, instrument.ms20_resonance),
        SynthKind::JunoPoly => (instrument.juno_cutoff_hz, instrument.juno_resonance),
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

fn lfo_wave(shape: LfoShape, phase: f32) -> f32 {
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

/// True when two clip sources resolve to the same cached audio runtime, so a
/// project update can keep the loaded sample in place instead of reloading.
/// Queues every repeat of `note` that lands on `local_tick`.
///
/// Repeats wrap inside the pattern length, so a note repeat never escapes its
/// pattern even when the note sits at the very end.
fn push_note_triggers(
    events: &mut Vec<TriggerEvent>,
    note: &PianoNote,
    local_tick: u64,
    lane_id: usize,
    pattern_length_steps: u64,
) {
    let pattern_ticks = pattern_length_steps.saturating_mul(16).max(16);
    for (offset, velocity) in note.trigger_offsets() {
        let tick = note.start_tick().saturating_add(offset) % pattern_ticks;
        if tick == local_tick {
            let mut triggered = *note;
            triggered.velocity = velocity;
            events.push(TriggerEvent::Note {
                note: triggered,
                lane_id,
            });
        }
    }
}

fn clip_routes_match(left: &ClipSource, right: &ClipSource) -> bool {
    match (left, right) {
        (ClipSource::Pattern { pattern_id: left }, ClipSource::Pattern { pattern_id: right }) => {
            left == right
        }
        (ClipSource::Audio { path: left }, ClipSource::Audio { path: right }) => left == right,
        _ => false,
    }
}

fn interpolate_sample(sample: &SampleData, cursor: f64) -> [f32; 2] {
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

fn waveform(waveform: SignalWaveform, phase: f32) -> f32 {
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

fn eq_gains(low_db: f32, mid_db: f32, high_db: f32) -> [f32; 3] {
    [
        10.0_f32.powf(low_db.clamp(-12.0, 12.0) / 20.0),
        10.0_f32.powf(mid_db.clamp(-12.0, 12.0) / 20.0),
        10.0_f32.powf(high_db.clamp(-12.0, 12.0) / 20.0),
    ]
}

fn process_eq(
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

fn finite_sample(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

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

pub fn render_project_to_wav(project: &Project, path: &str) -> Result<u64, String> {
    const SAMPLE_RATE: u32 = 44_100;
    let mut engine = AudioEngine::new(project.clone(), SAMPLE_RATE);
    for (instrument_id, instrument) in engine.project.instruments.iter().enumerate() {
        if !instrument.sample_path.trim().is_empty() {
            engine.instrument_samples[instrument_id] = Some(read_wav(&instrument.sample_path)?);
        }
    }
    for (lane_id, lane) in engine.project.lanes.iter().enumerate() {
        for (clip_index, clip) in lane.clips.iter().enumerate() {
            if let ClipSource::Audio { path } = &clip.source {
                engine.audio_clips[lane_id][clip_index].sample = Some(Arc::new(read_wav(path)?));
            }
        }
    }
    let end_step = engine
        .project
        .lanes
        .iter()
        .flat_map(|lane| &lane.clips)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ArrangementClip, Pattern, TimeSignature};

    fn empty_project_updates() -> Mutex<Option<Box<Project>>> {
        Mutex::new(None)
    }

    #[test]
    fn sample_and_hold_lfo_changes_only_at_cycle_boundaries() {
        let held = lfo_wave(LfoShape::SampleAndHold, 2.0);
        assert_eq!(held, lfo_wave(LfoShape::SampleAndHold, 2.75));
        assert_ne!(held, lfo_wave(LfoShape::SampleAndHold, 3.0));
    }

    #[test]
    fn selected_pattern_playback_uses_instrument_mixer_lane() {
        let mut project = Project::default();
        project.patterns[0].step_hits.push(crate::project::StepHit {
            instrument_id: 0,
            step: 0,
        });
        project.instruments[0].default_mixer_lane = 2;
        let mut engine = AudioEngine::new(project, 48_000);
        engine.selected_pattern = Some(0);
        engine.trigger_step(0);
        assert!(engine.trigger_events.iter().any(|event| matches!(
            event,
            TriggerEvent::InstrumentHit {
                instrument_id: 0,
                lane_id: 2
            }
        )));
    }

    #[test]
    fn arrangement_loop_wraps_playhead_at_end_step() {
        let mut engine = AudioEngine::new(Project::default(), 48_000);
        engine.playing = true;
        engine.position_steps = 3;
        engine.loop_range = Some((2, 4));
        engine.frames_into_step = engine.frames_per_step() - 1.0;
        let (_sender, receiver) = mpsc::channel();
        engine.render_frame(&receiver, &empty_project_updates());
        assert_eq!(engine.position_steps, 2);
    }

    #[test]
    fn stopping_playback_releases_ringing_voices() {
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::Sine;
        project.instruments[0].synth_enabled = true;
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        engine.start_voice(0, 0, 60, 110, Some(64));
        assert!(
            engine
                .voices
                .iter()
                .any(|voice| voice.active && !voice.released)
        );

        let (sender, receiver) = mpsc::channel();
        sender.send(AudioCommand::SetPlaying(false)).unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());

        assert!(!engine.playing);
        assert!(
            engine
                .voices
                .iter()
                .filter(|voice| voice.active)
                .all(|voice| voice.released),
            "voices kept ringing after stop"
        );
    }

    #[test]
    fn default_drum_samples_are_non_silent_and_finite() {
        for kind in [
            DefaultSample::Kick,
            DefaultSample::Snare,
            DefaultSample::HiHat,
        ] {
            let sample = SampleData::default_drum(kind, 48_000).unwrap();
            assert!(!sample.frames.is_empty());
            assert!(
                sample
                    .frames
                    .iter()
                    .flatten()
                    .all(|frame| frame.is_finite())
            );
            let peak = sample
                .frames
                .iter()
                .flatten()
                .map(|frame| frame.abs())
                .fold(0.0_f32, f32::max);
            assert!(peak > 0.01, "{kind:?} sample is silent");
        }
        assert!(SampleData::default_drum(DefaultSample::None, 48_000).is_none());
    }

    #[test]
    fn offline_wav_render_writes_a_stereo_file_with_audio() {
        let mut project = Project::default();
        project.lanes[0].clips[0].length_steps = 16;
        let path =
            std::env::temp_dir().join(format!("pulseforge-render-{}.wav", std::process::id()));
        let frames = render_project_to_wav(&project, path.to_str().unwrap()).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 44_100);
        assert_eq!(reader.duration(), frames as u32);
        assert!(
            reader
                .samples::<i16>()
                .take(44_100)
                .any(|sample| sample.unwrap().unsigned_abs() > 0)
        );
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn ms20_instrument_reuses_a_single_voice() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.instruments[0].synth_kind = SynthKind::Ms20;
        let mut engine = AudioEngine::new(project, 48_000);

        engine.start_voice(0, 0, 60, 100, None);
        engine.start_voice(0, 0, 67, 100, None);

        let active: Vec<_> = engine
            .voices
            .iter()
            .filter(|voice| voice.active && voice.instrument_id == 0)
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].note, 67);
    }

    #[test]
    fn classic_synth_models_render_finite_audio() {
        for kind in [
            SynthKind::JunoPoly,
            SynthKind::M1Workstation,
            SynthKind::FmSynth,
        ] {
            let mut project = Project::default();
            project.instruments[0].synth_enabled = true;
            project.instruments[0].synth_kind = kind;
            let mut engine = AudioEngine::new(project, 48_000);
            engine.start_voice(0, 0, 60, 110, None);
            let (_sender, receiver) = mpsc::channel();
            let mut peak = 0.0_f32;
            for _ in 0..100 {
                let output = engine.render_frame(&receiver, &empty_project_updates());
                assert!(output.into_iter().all(f32::is_finite));
                peak = peak.max(output[0].abs()).max(output[1].abs());
            }
            assert!(peak > 0.0001, "{kind:?} produced silence");
            if kind == SynthKind::JunoPoly {
                let voice = engine
                    .voices
                    .iter()
                    .find(|voice| voice.active && voice.instrument_id == 0)
                    .unwrap();
                assert!((voice.phase - 2.0 * voice.sub_phase).abs() < 0.001);
            }
        }
    }

    #[test]
    fn instrument_automation_updates_audio_rate_controls() {
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::CutoffHz,
        };
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::JunoPoly;
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target,
                points: vec![
                    crate::project::AutomationPoint {
                        step: 0,
                        value: 0.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                    crate::project::AutomationPoint {
                        step: 16,
                        value: 1.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                ],
                ..Default::default()
            });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);

        engine.update_instrument_controls(8.0);

        assert!((engine.instrument_controls[0].cutoff_hz - 8_020.0).abs() < 0.1);
    }

    #[test]
    fn control_rate_automation_reaches_the_instrument_within_one_block() {
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::CutoffHz,
        };
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::JunoPoly;
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target,
                points: vec![
                    crate::project::AutomationPoint {
                        step: 0,
                        value: 0.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                    crate::project::AutomationPoint {
                        step: 16,
                        value: 1.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                ],
                ..Default::default()
            });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        let (_sender, receiver) = mpsc::channel();
        let resting = engine.instrument_controls[0].cutoff_hz;
        for _ in 0..CONTROL_INTERVAL + 1 {
            engine.render_frame(&receiver, &empty_project_updates());
        }
        let moved = engine.instrument_controls[0].cutoff_hz;
        assert!(
            (moved - resting).abs() > 1.0,
            "control-rate automation did not update the instrument ({resting} -> {moved})"
        );
        assert!(!engine.automation_index.is_empty());
    }

    #[test]
    fn disabled_automation_lanes_are_skipped_by_the_index() {
        let mut project = Project::default();
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target: ParameterTarget::Instrument {
                    instrument_id: 0,
                    parameter: InstrumentParameter::Gain,
                },
                enabled: false,
                ..Default::default()
            });
        project.normalize();
        let engine = AudioEngine::new(project, 48_000);
        assert!(engine.automation_index.is_empty());
    }

    #[test]
    fn metronome_clicks_without_any_instrument_content() {
        let project = Project::default();
        let mut engine = AudioEngine::new(project, 48_000);
        let (sender, receiver) = mpsc::channel();
        sender.send(AudioCommand::SetMetronome(true)).unwrap();
        sender.send(AudioCommand::SetPlaying(true)).unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        let mut peak = 0.0_f32;
        for _ in 0..64 {
            let output = engine.render_frame(&receiver, &empty_project_updates());
            assert!(output.into_iter().all(f32::is_finite));
            peak = peak.max(output[0].abs());
        }
        assert!(peak > 0.0005, "metronome produced no click");
    }

    #[test]
    fn unchanged_project_updates_keep_audio_runtimes_in_place() {
        let mut project = Project::default();
        project.lanes[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "held.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project.clone(), 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.5, 0.5]],
        }));
        let original_len = engine.audio_clips[0].len();

        let mut edited = project;
        edited.lanes[0].gain = 0.42;
        engine.apply_project(edited);

        assert_eq!(engine.audio_clips[0].len(), original_len);
        assert!(
            engine.audio_clips[0][0].sample.is_some(),
            "a non-structural project edit dropped the loaded sample"
        );
    }

    #[test]
    fn duplicated_audio_clips_share_one_loaded_sample() {
        let mut project = Project::default();
        project.lanes[0].clips.clear();
        project.lanes[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "shared.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project.clone(), 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.1, 0.1]],
        }));

        let mut duplicated = project;
        duplicated.lanes[0].clips.push(ArrangementClip {
            start_step: 16,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "shared.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        engine.apply_project(duplicated);

        assert_eq!(engine.audio_clips[0].len(), 2);
        for runtime in &engine.audio_clips[0] {
            assert!(
                runtime.sample.is_some(),
                "a duplicated audio clip lost its sample"
            );
        }
        assert!(Arc::ptr_eq(
            engine.audio_clips[0][0].sample.as_ref().unwrap(),
            engine.audio_clips[0][1].sample.as_ref().unwrap()
        ));
    }

    #[test]
    fn audio_clips_start_from_their_source_offset() {
        let mut project = Project {
            bpm: 120.0,
            ..Project::default()
        };
        project.lanes[0].clips.clear();
        project.lanes[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            offset_steps: 4,
            source: ClipSource::Audio {
                path: "offset.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.1, 0.1]; 48_000],
        }));

        engine.trigger_step(0);

        assert!(engine.audio_clips[0][0].active);
        // 4 steps at 120 BPM is 0.5 s, which is 24 000 frames at 48 kHz.
        assert!((engine.audio_clips[0][0].cursor - 24_000.0).abs() < 1.0);
    }

    #[test]
    fn note_repeats_queue_one_trigger_per_repeat() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.patterns[0].notes = vec![PianoNote {
            instrument_id: 0,
            pitch: 60,
            velocity: 100,
            duration_steps: 1,
            repeat_count: 2,
            repeat_ticks: 4,
            repeat_decay: 10,
            ..PianoNote::default()
        }];
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.trigger_step(0);
        let at_zero = engine
            .trigger_events
            .iter()
            .filter(|event| matches!(event, TriggerEvent::Note { .. }))
            .count();
        assert_eq!(at_zero, 1, "the first repeat fires on the note start");

        engine.trigger_step(4);
        let at_four = engine
            .trigger_events
            .iter()
            .filter(|event| matches!(event, TriggerEvent::Note { note, .. } if note.velocity == 90))
            .count();
        assert_eq!(at_four, 1, "the second repeat fires four ticks later");
    }

    #[test]
    fn disabling_smoothing_snaps_mixer_gain_immediately() {
        let project = Project::default();
        let mut engine = AudioEngine::new(project, 48_000);
        assert!(engine.smoothing_enabled);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(AudioCommand::SetSmoothing {
                enabled: false,
                milliseconds: 50.0,
            })
            .unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        assert!(!engine.smoothing_enabled);
        // With smoothing off, one rendered frame reaches the new target.
        let mut project = engine.project.clone();
        project.lanes[0].gain = 0.1;
        engine.apply_project(project);
        engine.render_frame(&receiver, &empty_project_updates());
        assert!((engine.lane_gain[0].value() - 0.1).abs() < 1e-5);
    }

    #[test]
    fn smoothing_ramps_toward_a_new_gain_over_several_frames() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(AudioCommand::SetSmoothing {
                enabled: true,
                milliseconds: 40.0,
            })
            .unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        let mut changed = engine.project.clone();
        changed.lanes[0].gain = 0.2;
        engine.apply_project(changed);
        engine.render_frame(&receiver, &empty_project_updates());
        let after_one = engine.lane_gain[0].value();
        assert!(
            after_one > 0.2 && after_one < 0.8,
            "gain should be mid-ramp, got {after_one}"
        );
        for _ in 0..4_000 {
            engine.render_frame(&receiver, &empty_project_updates());
        }
        assert!((engine.lane_gain[0].value() - 0.2).abs() < 1e-4);
    }

    #[test]
    fn preview_sample_plays_through_the_master_bus() {
        let project = Project::default();
        let mut engine = AudioEngine::new(project, 48_000);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(AudioCommand::PreviewSample(Box::new(SampleData {
                sample_rate: 48_000,
                frames: vec![[0.5, 0.5]; 480],
            })))
            .unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        let output = engine.render_frame(&receiver, &empty_project_updates());
        assert!(output[0].abs() > 0.1, "preview sample was inaudible");
        for _ in 0..600 {
            engine.render_frame(&receiver, &empty_project_updates());
        }
        assert!(engine.preview.is_none(), "preview sample did not finish");
    }

    #[test]
    fn audio_clips_play_when_the_transport_starts_inside_them() {
        let mut project = Project {
            bpm: 120.0,
            ..Project::default()
        };
        project.lanes[0].clips.clear();
        project.lanes[0].clips.push(ArrangementClip {
            start_step: 8,
            length_steps: 8,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "mid.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.5, 0.5]; 48_000],
        }));
        engine.playing = true;
        // Transport jumps into the middle of the clip: step 10 of 8..16.
        engine.position_steps = 10;

        let output = engine.next_audio_clips(0);

        assert!(engine.audio_clips[0][0].active);
        assert!(
            output[0].abs() > 0.1,
            "clip was silent when starting mid-clip"
        );
        // Two steps at 120 BPM is 0.25 s, which is 12 000 frames at 48 kHz.
        assert!((engine.audio_clips[0][0].cursor - 12_000.0).abs() < 2.0);
    }

    #[test]
    fn audio_clips_rewind_after_leaving_their_range() {
        let mut project = Project::default();
        project.lanes[0].clips.clear();
        project.lanes[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 4,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "short.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.5, 0.5]; 1_000],
        }));
        engine.playing = true;
        engine.position_steps = 0;
        let _ = engine.next_audio_clips(0);
        engine.position_steps = 9;
        let _ = engine.next_audio_clips(0);
        assert!(!engine.audio_clips[0][0].active);
        assert!(!engine.audio_clips[0][0].finished);
        assert_eq!(engine.audio_clips[0][0].cursor, 0.0);
    }

    #[test]
    fn muted_instruments_and_tracks_stay_silent() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.instruments[0].enabled = false;
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        assert!(!engine.instrument_audible(0));

        let mut project = engine.project.clone();
        project.instruments[0].enabled = true;
        project.lanes[0].enabled = false;
        engine.apply_project(project);
        assert!(engine.instrument_audible(0));
        assert!(!engine.lane_audible(0));
    }

    #[test]
    fn soloing_one_instrument_silences_the_others() {
        let mut project = Project::default();
        project.instruments[1].solo = true;
        project.normalize();
        let engine = AudioEngine::new(project, 48_000);
        assert!(!engine.instrument_audible(0));
        assert!(engine.instrument_audible(1));
    }

    #[test]
    fn arpeggiator_schedules_one_note_per_rate_step() {
        let mut project = Project {
            bpm: 120.0,
            ..Project::default()
        };
        project.instruments[0].synth_enabled = true;
        project.instruments[0].arpeggio.enabled = true;
        project.instruments[0].arpeggio.octaves = 2;
        project.patterns[0].notes = vec![PianoNote {
            instrument_id: 0,
            pitch: 60,
            velocity: 100,
            duration_steps: 4,
            ..PianoNote::default()
        }];
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.trigger_step(0);
        // Two octaves over four steps => more than one scheduled event.
        assert!(
            engine.arp_queue.len() > 1,
            "arpeggiator scheduled {} events",
            engine.arp_queue.len()
        );
        assert!(engine.arp_queue.iter().all(|event| event.at_tick % 16 == 0));
    }

    #[test]
    fn external_generators_do_not_fall_back_to_a_builtin_voice() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.instruments[0].synth_kind = SynthKind::External;
        project.instruments[0].plugin_path = "C:/Plugins/Thing.vst3".to_owned();
        project.normalize();
        assert_eq!(project.instruments[0].synth_kind, SynthKind::External);
        let mut engine = AudioEngine::new(project, 48_000);
        engine.start_voice(0, 0, 60, 110, None);
        assert!(
            !engine
                .voices
                .iter()
                .any(|voice| voice.active && voice.instrument_id == 0),
            "an external plug-in must not allocate a built-in voice"
        );
    }

    #[test]
    fn signal_generator_waveforms_stay_in_unit_range() {
        for shape in [
            SignalWaveform::Sine,
            SignalWaveform::Triangle,
            SignalWaveform::Saw,
            SignalWaveform::Square,
        ] {
            for step in 0..512 {
                let phase = TAU * step as f32 / 512.0;
                let sample = waveform(shape, phase);
                assert!((-1.0..=1.0).contains(&sample));
            }
        }
    }

    #[test]
    fn pattern_clip_routes_note_to_its_arrangement_lane() {
        let mut project = Project::default();
        project.add_lane();
        project.lanes[1].name = "Second".to_owned();
        project.instruments[3].default_mixer_lane = 0;
        project.patterns = vec![Pattern {
            name: "Notes".to_owned(),
            length_steps: 16,
            notes: vec![PianoNote {
                repeat_count: 0,
                repeat_ticks: 0,
                repeat_decay: 0,
                instrument_id: 3,
                start_step: 0,
                start_substep: 0,
                duration_steps: 4,
                duration_substeps: 0,
                pitch: 60,
                velocity: 100,
            }],
            ..Pattern::default()
        }];
        project.lanes[1].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            source: ClipSource::Pattern { pattern_id: 0 },
            offset_steps: 0,
            ..ArrangementClip::default()
        });
        project.normalize();

        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        engine.trigger_step(0);
        let voice = engine.voices.iter().find(|voice| voice.active).unwrap();
        assert_eq!(voice.mixer_lane, 1);
    }

    #[test]
    fn master_bus_receives_routed_audio_and_drives_the_output_meter() {
        let project = Project::default();
        let master_index = project.lanes.len();
        let mut engine = AudioEngine::new(project, 48_000);
        let edge = engine
            .send_edges
            .iter_mut()
            .find(|edge| edge.source == 0 && edge.destination == master_index)
            .unwrap();
        edge.delayed = [0.4, -0.3];
        let (_sender, receiver) = mpsc::channel();

        let output = engine.render_frame(&receiver, &empty_project_updates());

        assert!((output[0] - 0.4).abs() < 0.001);
        assert!((output[1] + 0.3).abs() < 0.001);
        assert!(engine.meter_levels[master_index] > 0.39);
    }

    #[test]
    fn project_updates_keep_audio_samples_attached_when_clips_move_lanes() {
        let mut project = Project::default();
        project.lanes[0].clips.push(ArrangementClip {
            start_step: 16,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "existing.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project.clone(), 48_000);
        let existing_index = project.lanes[0]
            .clips
            .iter()
            .position(
                |clip| matches!(&clip.source, ClipSource::Audio { path } if path == "existing.wav"),
            )
            .unwrap();
        engine.audio_clips[0][existing_index].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.25, 0.25]],
        }));

        let moved_clip = project.lanes[0].clips.remove(existing_index);
        project.lanes[1].clips.push(moved_clip);
        project.lanes[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 8,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "new.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let (sender, receiver) = mpsc::channel();
        assert!(
            sender
                .send(AudioCommand::SetProject(Box::new(project.clone())))
                .is_ok()
        );
        engine.apply_pending(&receiver, &empty_project_updates());

        let moved_index = project.lanes[1]
            .clips
            .iter()
            .position(
                |clip| matches!(&clip.source, ClipSource::Audio { path } if path == "existing.wav"),
            )
            .unwrap();
        assert!(engine.audio_clips[1][moved_index].sample.is_some());
    }

    #[test]
    fn time_signature_changes_bar_steps_not_step_clock() {
        let project = Project {
            time_signature: TimeSignature {
                numerator: 7,
                denominator: 8,
            },
            ..Project::default()
        };
        let engine = AudioEngine::new(project, 48_000);
        assert!((engine.frames_per_step() - 6000.0).abs() < 0.001);
    }

    #[test]
    fn equalizer_is_flat_at_zero_and_changes_tonal_gain() {
        let mut state = EqState::default();
        let flat = process_eq([0.25, 0.25], &mut state, eq_gains(0.0, 0.0, 0.0), 0.1, 0.5);
        assert!((flat[0] - 0.25).abs() < 0.00001);

        let mut state = EqState::default();
        let boosted = process_eq([1.0, 1.0], &mut state, eq_gains(0.0, 0.0, 12.0), 0.1, 0.5);
        assert!(boosted[0] > 2.0);
    }

    #[test]
    fn invalid_sample_values_are_silenced() {
        assert_eq!(finite_sample(f32::NAN), 0.0);
        assert_eq!(finite_sample(f32::INFINITY), 0.0);
        assert_eq!(finite_sample(2.0), 1.0);
    }
}
