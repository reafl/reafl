//! Audio-side data: samples, commands, voices and the engine state.

use std::{
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
    plugins::BuiltinEffect,
    project::{DefaultSample, PianoNote, Project},
};

pub(crate) const VOICE_COUNT: usize = 64;

/// Automation and instrument controls are refreshed once per control block
/// instead of once per sample. Sixteen frames is far below the audible zipper
/// threshold while cutting the automation cost by more than an order of
/// magnitude on dense projects.
pub(crate) const CONTROL_INTERVAL: u32 = 16;

/// Requested output buffer in frames. Small buffers lower monitoring latency;
/// the device default is used when a fixed size is not supported.
pub(crate) const PREFERRED_BUFFER_FRAMES: u32 = 256;

/// A decoded sample: interleaved stereo frames plus the rate they were captured
/// at.
///
/// Samples are shared between clips by path, so the engine owns one copy per
/// file even when the arrangement uses it in several places.
pub struct SampleData {
    /// Sample rate the frames were captured at.
    pub sample_rate: u32,
    /// Stereo frames.
    pub frames: Vec<[f32; 2]>,
}

impl SampleData {
    /// Synthesises one of the shipped drum samples.
    ///
    /// # Parameters
    ///
    /// - `kind`: which drum sample to build; [`DefaultSample::None`] yields
    ///   `None`.
    /// - `sample_rate`: rate the frames should be generated at.
    ///
    /// # Returns
    ///
    /// The generated sample, or `None` when no sample was requested.
    pub(crate) fn default_drum(kind: DefaultSample, sample_rate: u32) -> Option<Self> {
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

/// Everything the UI can ask the audio thread to do.
///
/// The engine owns all of its state, so a change from the UI is a message on
/// this channel rather than a shared mutable value.
pub enum AudioCommand {
    /// Starts or stops playback.
    SetPlaying(bool),
    /// Moves the transport back to the start of the loop (or zero) without
    /// changing the play state.
    Rewind,
    /// Moves the transport to an absolute step position.
    Seek(u64),
    /// Enables or disables the metronome click.
    SetMetronome(bool),
    /// Selects the pattern the step sequencer plays, `None` for the arrangement.
    SetSelectedPattern(Option<usize>),
    /// Sets the arrangement loop range in steps, `None` to clear it.
    SetLoop(Option<(u64, u64)>),
    /// Sets the project tempo in BPM.
    SetTempo(f32),
    /// Replaces the engine's copy of the project.
    SetProject(Box<Project>),
    /// Caches a decoded sample for one instrument.
    LoadInstrumentSample {
        /// Instrument the sample belongs to.
        instrument_id: usize,
        /// The decoded sample.
        sample: SampleData,
    },
    /// Caches a decoded WAV by path. Clips resolve their audio from this
    /// cache, so the sample survives the project rebuild that follows an edit
    /// and can arrive before the clip itself exists.
    LoadAudioClip {
        /// Path the sample was decoded from, used as the cache key.
        path: String,
        /// The decoded sample.
        sample: SampleData,
    },
    /// Plays a note on an instrument immediately, bypassing the sequencer.
    MidiNote {
        /// Instrument the note is sent to.
        instrument_id: usize,
        /// MIDI note number.
        note: u8,
        /// MIDI velocity, 1..=127.
        velocity: u8,
        /// `true` for note-on, `false` for note-off.
        on: bool,
    },
    /// Plays a one-shot sample through the master bus for auditioning.
    PreviewSample(Box<SampleData>),
    /// Enables or disables linear parameter ramping and sets its length.
    SetSmoothing {
        /// Whether parameter ramping is active.
        enabled: bool,
        /// Ramp length in milliseconds.
        milliseconds: f32,
    },
    /// Hands a real VST3/CLAP audio half to the engine so it can render there.
    LoadHosted {
        /// Host instance id of the plug-in.
        id: u64,
        /// The audio half of the hosted plug-in.
        audio: Box<pulseforge_hosting::HostedAudio>,
        /// Whether the plug-in is an instrument rather than an effect.
        instrument: bool,
        /// Debug counters owned by the UI's plug-in registry; the slot writes
        /// its block times there so the debug panel can list them.
        counters: Option<Arc<SlotCounters>>,
    },
    /// Drops a hosted plug-in (its editor is closed on the UI thread).
    UnloadHosted {
        /// Host instance id of the plug-in.
        id: u64,
    },
}

/// The UI-side handle of a running audio engine.
///
/// Dropping it stops the stream, so the editor keeps one for its lifetime.
pub struct AudioOutput {
    /// Channel to the audio thread.
    pub sender: SyncSender<AudioCommand>,
    /// Meter levels in dB, one per mixer channel plus the master.
    pub meters: Arc<Mutex<Vec<f32>>>,
    /// Integer playhead in steps, published every callback.
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
    /// Project versions waiting to be picked up by the audio thread.
    pub(crate) project_updates: Arc<Mutex<Option<Box<Project>>>>,
    /// Projects that were replaced on the audio thread and can be dropped from
    /// the UI thread, so the callback never frees memory.
    pub(crate) retired_projects: Arc<Mutex<Vec<Project>>>,
    /// The stream itself; kept alive so the device keeps running.
    pub(crate) _stream: Stream,
    /// Sample rate the device is running at.
    pub sample_rate: u32,
}

impl AudioOutput {
    /// Opens the default output device and starts the engine.
    ///
    /// # Parameters
    ///
    /// - `project`: the project the engine starts with.
    ///
    /// # Errors
    ///
    /// Returns a message describing the failing step when no output device
    /// exists, the format cannot be queried or the stream cannot be created.
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
            let mut engine = super::engine::AudioEngine::new(project.clone(), sample_rate);
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

    /// Replaces the engine's copy of the project.
    ///
    /// # Errors
    ///
    /// Returns a message when the pending-update lock is poisoned.
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

pub(crate) struct Voice {
    pub(crate) instrument_id: usize,
    pub(crate) mixer_lane: usize,
    pub(crate) note: u8,
    pub(crate) phase: f32,
    pub(crate) second_phase: f32,
    pub(crate) sub_phase: f32,
    pub(crate) filter_state: f32,
    pub(crate) age_seconds: f32,
    pub(crate) velocity: f32,
    pub(crate) envelope: f32,
    pub(crate) active: bool,
    pub(crate) released: bool,
    pub(crate) release_at_tick: Option<u64>,
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
pub(crate) struct InstrumentControls {
    pub(crate) gain: f32,
    pub(crate) pan: f32,
    pub(crate) cutoff_hz: f32,
    pub(crate) resonance: f32,
    pub(crate) pulse_width: f32,
    pub(crate) sub_level: f32,
    pub(crate) tone: f32,
    pub(crate) decay: f32,
    pub(crate) fm_ratio: f32,
    pub(crate) fm_index: f32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct AutomationRuntime {
    pub(crate) start_frame: Option<u64>,
    pub(crate) release_frame: Option<u64>,
    pub(crate) release_level: f32,
}

/// One scheduled arpeggiator note.
#[derive(Clone, Copy)]
pub(crate) struct ArpEvent {
    pub(crate) at_tick: u64,
    pub(crate) release_tick: u64,
    pub(crate) instrument_id: usize,
    pub(crate) lane_id: usize,
    pub(crate) pitch: u8,
    pub(crate) velocity: u8,
}

pub(crate) struct SampleVoice {
    pub(crate) instrument_id: usize,
    pub(crate) mixer_lane: usize,
    pub(crate) cursor: f64,
    pub(crate) active: bool,
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

pub(crate) struct AudioClipRuntime {
    /// Shared by every clip that references the same file, so sliced or
    /// duplicated audio clips keep playing without re-reading the WAV.
    pub(crate) sample: Option<Arc<SampleData>>,
    pub(crate) cursor: f64,
    pub(crate) active: bool,
    /// Set when the clip has played to its end. Cleared when the transport
    /// leaves the clip, so re-entering it plays again.
    pub(crate) finished: bool,
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
pub(crate) enum TriggerEvent {
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
pub(crate) enum EffectProcessor {
    Builtin(BuiltinEffect),
    /// Hosted VST3/CLAP insert, addressed by host id in `AudioEngine::hosted`.
    External(u64),
}

impl EffectProcessor {
    /// Identity used to decide whether the processor must be rebuilt.
    pub(crate) fn identity(&self) -> (&'static str, &str) {
        match self {
            Self::Builtin(effect) => ("builtin", effect.effect_name()),
            Self::External(_) => ("external", ""),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct EqState {
    pub(crate) low_250: [f32; 2],
    pub(crate) low_2k: [f32; 2],
}

#[derive(Clone, Copy)]
pub(crate) struct SendEdge {
    pub(crate) source: usize,
    pub(crate) destination: usize,
    pub(crate) gain: f32,
    pub(crate) delayed: [f32; 2],
}
