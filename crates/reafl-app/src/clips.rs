//! Placing, slicing and decoding clips on tracks.

use crate::*;

impl ReaflApp {
    /// Places the active arrangement source on a track.
    ///
    /// # Parameters
    ///
    /// - `track`: the track the clip lands on.
    /// - `start_step`: position in steps.
    /// - `placement`: what to place.
    ///
    /// # Returns
    ///
    /// The index of the new clip, or `None` when the track or placement is
    /// unusable.
    pub(crate) fn place_source_clip(
        &mut self,
        track: usize,
        start_step: u64,
        placement: ClipPlacement,
    ) -> Option<usize> {
        if track >= self.project.tracks.len() {
            return None;
        }
        // Audio clips take their length from the sample so the clip matches what
        // is heard; the sample is read here once, before the project snapshot is
        // sent, because the engine rebuilds clip runtimes on project updates.
        let (length_steps, audio_path, loaded_sample) = match &placement {
            ClipPlacement::Notes { length_steps } => (*length_steps, None, None),
            ClipPlacement::Audio { path } => match audio::read_wav(path) {
                Ok(sample) => {
                    let steps = self.steps_for_sample(&sample);
                    (steps, Some(path.clone()), Some(sample))
                }
                Err(error) => {
                    self.status = format!("WAV could not load: {error}");
                    (self.new_audio_length_steps.max(1), Some(path.clone()), None)
                }
            },
        };
        let clip_index = self.add_clip(track, start_step, length_steps.max(1), audio_path.clone());
        self.selected_lane = track;
        self.selected_clip = clip_index;
        self.selection = Some(Selection::Clip {
            track,
            clip: clip_index,
        });
        self.sync_project();
        match (&audio_path, loaded_sample) {
            (Some(path), Some(sample)) => {
                self.send_audio(AudioCommand::LoadAudioClip {
                    path: path.clone(),
                    sample,
                });
                self.status = format!(
                    "Placed {} ({} steps) on this track.",
                    clip_label(path),
                    length_steps
                );
            }
            (Some(path), None) => {
                self.status = format!("Placed {path} but the WAV could not be read.");
            }
            (None, _) => {
                self.status = "Placed an empty clip; draw notes in the piano roll.".to_owned();
            }
        }
        Some(clip_index)
    }

    /// Splits the clip under the pointer at `step`.
    ///
    /// # Parameters
    ///
    /// - `track`: track owning the clip.
    /// - `step`: split position in steps.
    /// - `snap_ticks`: snap grid, so the cut lands on a musical position.
    ///
    /// # Returns
    ///
    /// `true` when a clip was cut.
    pub(crate) fn slice_clip_at(&mut self, track: usize, step: u64, snap_ticks: u64) -> bool {
        let Some(track_data) = self.project.tracks.get_mut(track) else {
            return false;
        };
        let Some(index) = track_data.clips.iter().position(|clip| {
            clip.start_step < step && step < clip.start_step.saturating_add(clip.length_steps)
        }) else {
            return false;
        };
        let clip = track_data.clips[index].clone();
        let cut = snap_arrangement_step(step, snap_ticks).max(clip.start_step + 1);
        let end = clip.start_step.saturating_add(clip.length_steps);
        if cut >= end {
            return false;
        }
        let clip_start = clip.start_step;
        track_data.clips[index].length_steps = cut - clip_start;
        let mut tail = clip;
        tail.start_step = cut;
        tail.length_steps = end - cut;
        tail.offset_steps = tail.offset_steps.saturating_add(cut - clip_start);
        track_data.clips.push(tail);
        track_data.clips.sort_by_key(|clip| clip.start_step);
        self.status = "Sliced clip.".to_owned();
        true
    }

    /// Reads the WAV of the `instrument`-th track's sample and hands it to the
    /// engine.
    ///
    /// # Parameters
    ///
    /// - `instrument_id`: the track whose sample is loaded.
    pub(crate) fn load_instrument_sample(&mut self, instrument_id: usize) {
        let Some(generator) = self
            .project
            .tracks
            .get(instrument_id)
            .and_then(|track| track.generator())
        else {
            self.status = "That track has no generator.".to_owned();
            return;
        };
        let path = generator.sample_path.clone();
        if path.trim().is_empty() {
            self.status = "Enter a WAV path before loading the track sample.".to_owned();
            return;
        }
        // Decoding happens on a worker thread; the audio side is updated when
        // the task lands.
        self.status = format!("Loading {path}…");
        self.tasks
            .decode_sample(path, tasks::SampleUse::Instrument(instrument_id));
    }

    /// Adds an audio clip to a track from the audio-clip path field.
    ///
    /// # Parameters
    ///
    /// - `start_step`: position in steps.
    /// - `track`: the target track.
    pub(crate) fn add_audio_clip(&mut self, start_step: u64, track: usize) {
        let path = self.new_audio_path.trim().to_owned();
        if path.is_empty() {
            self.status = "Enter a WAV path for the audio clip.".to_owned();
            return;
        }
        if track >= self.project.tracks.len() {
            self.status = "The target track is unavailable.".to_owned();
            return;
        }
        let clip_index = self.add_clip(
            track,
            start_step,
            self.new_audio_length_steps.max(1),
            Some(path.clone()),
        );
        self.selected_lane = track;
        self.selected_clip = clip_index;
        self.selection = Some(Selection::Clip {
            track,
            clip: clip_index,
        });
        self.sync_project();
        self.status = format!("Added audio clip from {path}; decoding…");
        self.tasks.decode_sample(path, tasks::SampleUse::Clip);
    }

    /// Inserts a clip into a track and returns its index.
    ///
    /// # Parameters
    ///
    /// - `track`: track to insert into.
    /// - `start_step`: position in steps.
    /// - `length_steps`: length in steps, at least one.
    /// - `audio_path`: WAV path for an audio clip, `None` for a note clip.
    pub(crate) fn add_clip(
        &mut self,
        track: usize,
        start_step: u64,
        length_steps: u64,
        audio_path: Option<String>,
    ) -> usize {
        let insertion_index = self.project.tracks[track]
            .clips
            .iter()
            .filter(|clip| clip.start_step <= start_step)
            .count();
        self.project.tracks[track].clips.insert(
            insertion_index,
            Clip {
                name: "Clip".to_owned(),
                start_step,
                length_steps: length_steps.max(1),
                offset_steps: 0,
                notes: Vec::new(),
                audio_path,
                ..Clip::default()
            },
        );
        insertion_index
    }

    /// Decodes the WAV of every audio clip in the project.
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
        for track in self.project.tracks.iter() {
            for clip in track.clips.iter() {
                if let Some(path) = &clip.audio_path {
                    paths.insert(path.clone());
                }
            }
        }
        for path in paths {
            self.tasks.decode_sample(path, tasks::SampleUse::Clip);
        }
    }
}
