//! Placing, slicing and decoding arrangement clips.

use crate::*;

impl PulseForgeApp {
    /// Places the active arrangement source on a lane, returning the new clip
    /// index. Shared by the pen, brush and drag-and-drop paths.
    pub(crate) fn place_source_clip(
        &mut self,
        lane: usize,
        start_step: u64,
        source: ClipSource,
    ) -> Option<usize> {
        if lane >= self.project.lanes.len() {
            return None;
        }
        // Audio clips take their length from the sample so the clip matches what
        // is heard; the sample is read here once, before the project snapshot is
        // sent, because the engine rebuilds clip runtimes on project updates.
        let mut loaded_sample = None;
        let (length_steps, source) = match source {
            ClipSource::Pattern { pattern_id } => {
                let length = self
                    .project
                    .patterns
                    .get(pattern_id)
                    .map_or(16, |pattern| pattern.length_steps);
                (length, ClipSource::Pattern { pattern_id })
            }
            ClipSource::Audio { path } => match audio::read_wav(&path) {
                Ok(sample) => {
                    let steps = self.steps_for_sample(&sample);
                    loaded_sample = Some(sample);
                    (steps, ClipSource::Audio { path })
                }
                Err(error) => {
                    self.status = format!("WAV could not load: {error}");
                    (
                        self.new_audio_length_steps.max(1),
                        ClipSource::Audio { path },
                    )
                }
            },
        };
        let clip_index = self.add_arrangement_clip(lane, start_step, length_steps, source.clone());
        self.selected_lane = lane;
        self.selection = Some(Selection::Clip {
            lane,
            clip: clip_index,
        });
        self.sync_project();
        if let ClipSource::Audio { path } = &source {
            match loaded_sample {
                Some(sample) => {
                    self.send_audio(AudioCommand::LoadAudioClip {
                        path: path.clone(),
                        sample,
                    });
                    self.status = format!(
                        "Placed {} ({} steps) on this mixer lane.",
                        clip_label(path),
                        length_steps
                    );
                }
                None => {
                    self.status = format!("Placed {path} but the WAV could not be read.");
                }
            }
        } else {
            self.status =
                "Placed pattern clip; its instruments route to this mixer lane.".to_owned();
        }
        Some(clip_index)
    }

    /// Splits the clip under the pointer at `step`, mirroring the FL Studio
    /// slice tool. Returns whether anything was cut.
    pub(crate) fn slice_clip_at(&mut self, lane: usize, step: u64, snap_ticks: u64) -> bool {
        let Some(lane_data) = self.project.lanes.get_mut(lane) else {
            return false;
        };
        let Some(index) = lane_data.clips.iter().position(|clip| {
            clip.start_step < step && step < clip.start_step.saturating_add(clip.length_steps)
        }) else {
            return false;
        };
        let clip = lane_data.clips[index].clone();
        let cut = snap_arrangement_step(step, snap_ticks).max(clip.start_step + 1);
        let end = clip.start_step.saturating_add(clip.length_steps);
        if cut >= end {
            return false;
        }
        lane_data.clips[index].length_steps = cut - clip.start_step;
        lane_data.clips.push(ArrangementClip {
            start_step: cut,
            length_steps: end - cut,
            offset_steps: clip
                .offset_steps
                .saturating_add(cut.saturating_sub(clip.start_step)),
            source: clip.source,
            ..clip
        });
        lane_data.clips.sort_by_key(|clip| clip.start_step);
        self.status = "Sliced clip.".to_owned();
        true
    }

    /// Routes an instrument to the next free mixer insert and renames that
    /// insert after the instrument, mirroring Ctrl+L in the channel rack.
    pub(crate) fn route_instrument_to_next_insert(&mut self, instrument_id: usize) {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            return;
        };
        let name = instrument.name.clone();
        let used: Vec<usize> = self
            .project
            .instruments
            .iter()
            .enumerate()
            .filter(|(id, _)| *id != instrument_id)
            .map(|(_, other)| other.default_mixer_lane)
            .collect();
        let Some(lane) = (0..self.project.lanes.len()).find(|lane| !used.contains(lane)) else {
            self.status = "No free mixer insert is available.".to_owned();
            return;
        };
        self.push_history();
        self.project.instruments[instrument_id].default_mixer_lane = lane;
        self.project.lanes[lane].name = name;
        self.selected_mixer_channel = lane;
        self.sync_project();
        self.status = format!("Routed to mixer insert {}.", lane + 1);
    }

    pub(crate) fn load_instrument_sample(&mut self, instrument_id: usize) {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            self.status = "That instrument no longer exists.".to_owned();
            return;
        };
        let path = instrument.sample_path.clone();
        if path.trim().is_empty() {
            self.status = "Enter a WAV path before loading the instrument sample.".to_owned();
            return;
        }
        // Decoding happens on a worker thread; the audio side is updated when
        // the task lands.
        self.status = format!("Loading {path}…");
        self.tasks
            .decode_sample(path, tasks::SampleUse::Instrument(instrument_id));
    }

    pub(crate) fn add_audio_clip(&mut self, start_step: u64, lane: usize) {
        let path = self.new_audio_path.trim().to_owned();
        if path.is_empty() {
            self.status = "Enter a WAV path for the audio clip.".to_owned();
            return;
        }
        if lane >= self.project.lanes.len() {
            self.status = "The target arrangement lane is unavailable.".to_owned();
            return;
        }
        let clip_index = self.add_arrangement_clip(
            lane,
            start_step,
            self.new_audio_length_steps.max(1),
            ClipSource::Audio { path: path.clone() },
        );
        self.selected_lane = lane;
        self.selection = Some(Selection::Clip {
            lane,
            clip: clip_index,
        });
        self.sync_project();
        self.status = format!("Added audio clip from {path}; decoding…");
        self.tasks.decode_sample(path, tasks::SampleUse::Clip);
    }

    pub(crate) fn add_arrangement_clip(
        &mut self,
        lane: usize,
        start_step: u64,
        length_steps: u64,
        source: ClipSource,
    ) -> usize {
        let insertion_index = self.project.lanes[lane]
            .clips
            .iter()
            .filter(|clip| clip.start_step <= start_step)
            .count();
        self.project.lanes[lane].clips.insert(
            insertion_index,
            ArrangementClip {
                start_step,
                length_steps: length_steps.max(1),
                offset_steps: 0,
                source,
                ..ArrangementClip::default()
            },
        );
        insertion_index
    }

    pub(crate) fn load_audio_clip_samples(&mut self) {
        if self.sender().is_none() {
            self.status = self
                .audio_error
                .clone()
                .unwrap_or_else(|| "Audio output is unavailable.".to_owned());
            return;
        }
        // Each clip decodes on its own worker thread; a project with many
        // audio clips therefore loads in parallel instead of one WAV at a time.
        let mut paths = BTreeSet::new();
        for lane in self.project.lanes.iter() {
            for clip in lane.clips.iter() {
                if let ClipSource::Audio { path } = &clip.source {
                    paths.insert(path.clone());
                }
            }
        }
        for path in paths {
            self.tasks.decode_sample(path, tasks::SampleUse::Clip);
        }
    }
}

