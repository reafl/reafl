//! Draining the command queue and adopting a new project.

use super::*;

impl AudioEngine {
    pub(crate) fn apply_pending(
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
                    let slot =
                        HostedSlot::new(*audio, instrument, f64::from(self.sample_rate), counters);
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

    pub(crate) fn apply_project(&mut self, mut project: Project) {
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
}
