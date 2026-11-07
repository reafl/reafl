//! The UI-side half of the audio engine: device setup and the command handle.

use std::{
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
use pulseforge_profiling::{Channel, profile_scope, set_thread_channel};

use crate::project::Project;

use super::*;

/// The UI-side handle of a running audio engine.
///
/// Dropping it stops the stream, so the editor keeps one for its lifetime.
pub struct AudioOutput {
    /// Channel to the audio thread.
    pub sender: SyncSender<AudioCommand>,
    /// Meter levels in dB, one per mixer channel plus the master.
    pub meters: Arc<Mutex<Vec<f32>>>,
    /// Followed level of every automation lane that listens to a track peak,
    /// indexed by lane. Peaks only exist while audio runs, so the UI reads them
    /// here instead of prerendering them.
    pub peaks: Arc<Mutex<Vec<f32>>>,
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

        let meters = Arc::new(Mutex::new(vec![0.0; project.tracks.len() + 1]));
        let peaks = Arc::new(Mutex::new(vec![0.0; project.automation_tracks.len()]));
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
            let callback_peaks = Arc::clone(&peaks);
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
                        if let Ok(mut published) = callback_peaks.try_lock() {
                            published.clone_from(&engine.peak_levels);
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
            peaks,
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

    /// Followed level of one peak lane, as published by the last callback.
    ///
    /// # Parameters
    ///
    /// - `track_id`: automation lane index.
    ///
    /// # Returns
    ///
    /// The follower level in 0..=1, or `None` when the lane has no peak source.
    pub fn peak_level(&self, track_id: usize) -> Option<f32> {
        let peaks = self.peaks.lock().ok()?;
        peaks.get(track_id).copied()
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
