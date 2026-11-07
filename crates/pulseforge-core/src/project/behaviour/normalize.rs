//! Range clamping for every numeric field of a project.

use super::*;

impl Project {
    /// Clamps the audio-clip playback controls to their documented ranges.
    fn normalize_sample_controls(&mut self) {
        self.sample_gain = finite_clamp(self.sample_gain, 0.0, 1.0, 0.8);
        self.sample_pan = finite_clamp(self.sample_pan, -1.0, 1.0, 0.0);
        self.sample_pitch = finite_clamp(self.sample_pitch, -24.0, 24.0, 0.0);
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                clip.sample_gain = finite_clamp(clip.sample_gain, 0.0, 1.0, 0.8);
                clip.sample_pan = finite_clamp(clip.sample_pan, -1.0, 1.0, 0.0);
                clip.sample_pitch = finite_clamp(clip.sample_pitch, -24.0, 24.0, 0.0);
            }
        }
    }

    /// Clamps every numeric field of the project into its documented range.
    ///
    /// Called after loading and after edits that can produce out-of-range
    /// values, so the engine never sees a value it cannot render. It also keeps
    /// the track tree valid: parents must be folders, the tree must be acyclic,
    /// and a group may only forward parameters its children actually have.
    pub fn normalize(&mut self) {
        self.normalize_sample_controls();
        self.bpm = finite_clamp(self.bpm, 20.0, 300.0, 120.0);
        self.snap_ticks = self.snap_ticks.clamp(1, 1024);
        self.time_signature.numerator = self.time_signature.numerator.clamp(1, 128);
        self.time_signature.denominator = self.time_signature.denominator.clamp(1, 128);

        if self.tracks.is_empty() {
            self.tracks.push(Track::group("Folder 1"));
        }
        for track in &mut self.tracks {
            Self::normalize_track(track);
        }
        self.normalize_tree();

        let track_count = self.tracks.len();
        let channel_count = track_count + 1;
        let has_master_routes =
            self.sends.len() > track_count || self.sends.iter().any(|row| row.len() > track_count);
        self.sends.resize(channel_count, Vec::new());
        for row in &mut self.sends {
            row.resize(channel_count, 0.0);
            for gain in row {
                *gain = finite_clamp(*gain, 0.0, 0.95, 0.0);
            }
        }
        if !has_master_routes {
            for row in self.sends.iter_mut().take(track_count) {
                row[track_count] = 0.25;
            }
        }
        self.master.gain = finite_clamp(self.master.gain, 0.0, 1.0, 1.0);
        self.master.pan = finite_clamp(self.master.pan, -1.0, 1.0, 0.0);
        Self::normalize_eq(&mut self.master.eq);
        self.master
            .inserts
            .resize_with(INSERT_SLOTS, InsertFxSlot::default);
        self.master.inserts.truncate(INSERT_SLOTS);
        Self::normalize_inserts(&mut self.master.inserts);
        normalize_sends(&mut self.sends);
        for route in &mut self.midi_routes {
            *route = (*route).min(track_count - 1);
        }
        self.normalize_automation();
    }

    /// Clamps the fields of one track.
    ///
    /// # Parameters
    ///
    /// - `track`: the track to normalise in place.
    fn normalize_track(track: &mut Track) {
        track.height = finite_clamp(track.height, 34.0, 260.0, 54.0);
        track.gain = finite_clamp(track.gain, 0.0, 1.0, 0.8);
        track.pan = finite_clamp(track.pan, -1.0, 1.0, 0.0);
        track.width = finite_clamp(track.width, 0.0, 2.0, 1.0);
        Self::normalize_eq(&mut track.eq);
        track
            .inserts
            .resize_with(INSERT_SLOTS, InsertFxSlot::default);
        track.inserts.truncate(INSERT_SLOTS);
        Self::normalize_inserts(&mut track.inserts);
        track.midi_fx.transpose = track.midi_fx.transpose.clamp(-48, 48);
        track.midi_fx.velocity_offset = track.midi_fx.velocity_offset.clamp(-126, 126);
        track.midi_fx.velocity_scale = finite_clamp(track.midi_fx.velocity_scale, 0.0, 4.0, 1.0);
        if let Some(generator) = track.generator_mut() {
            Self::normalize_generator(generator);
        }
        for clip in &mut track.clips {
            Self::normalize_clip(clip);
        }
        track.clips.sort_by_key(|clip| clip.start_step);
    }

    /// Clamps the fields of one generator.
    ///
    /// # Parameters
    ///
    /// - `generator`: the generator to normalise in place.
    fn normalize_generator(generator: &mut Generator) {
        generator.gain = finite_clamp(generator.gain, 0.0, 1.0, 0.8);
        generator.pan = finite_clamp(generator.pan, -1.0, 1.0, 0.0);
        generator.synth_note = generator.synth_note.min(127);
        generator.ms20_cutoff_hz = finite_clamp(generator.ms20_cutoff_hz, 40.0, 16_000.0, 1_800.0);
        generator.ms20_resonance = finite_clamp(generator.ms20_resonance, 0.0, 0.9, 0.2);
        generator.juno_cutoff_hz = finite_clamp(generator.juno_cutoff_hz, 40.0, 16_000.0, 2_400.0);
        generator.juno_resonance = finite_clamp(generator.juno_resonance, 0.0, 0.9, 0.15);
        generator.juno_pulse_width = finite_clamp(generator.juno_pulse_width, 0.05, 0.95, 0.5);
        generator.juno_sub_level = finite_clamp(generator.juno_sub_level, 0.0, 1.0, 0.3);
        generator.m1_tone = finite_clamp(generator.m1_tone, 0.0, 1.0, 0.55);
        generator.m1_decay = finite_clamp(generator.m1_decay, 0.05, 4.0, 1.2);
        generator.m1_decay_steps = finite_clamp(generator.m1_decay_steps, 0.01, 256.0, 4.0);
        generator.fm_ratio = finite_clamp(generator.fm_ratio, 0.25, 16.0, 2.0);
        generator.fm_index = finite_clamp(generator.fm_index, 0.0, 1.0, 0.35);
        generator.voices.voice_count = generator.voices.voice_count.clamp(1, 64);
        generator.voices.midi_in_channel = generator.voices.midi_in_channel.min(15);
        generator.voices.midi_out_channel = generator.voices.midi_out_channel.min(15);
        generator.voices.audio_in = generator.voices.audio_in.min(15);
        generator.voices.audio_out = generator.voices.audio_out.min(15);
        generator.arpeggio.gate = finite_clamp(generator.arpeggio.gate, 0.05, 1.0, 0.6);
        generator.arpeggio.octaves = generator.arpeggio.octaves.clamp(1, 4);
        generator.arpeggio.rate.amount =
            finite_clamp(generator.arpeggio.rate.amount, 0.01, 64.0, 1.0);
    }

    /// Clamps the fields of one clip and its notes.
    ///
    /// # Parameters
    ///
    /// - `clip`: the clip to normalise in place.
    fn normalize_clip(clip: &mut Clip) {
        clip.length_steps = clip.length_steps.max(1);
        clip.offset_steps = clip.offset_steps.min(clip.length_steps.saturating_sub(1));
        if clip
            .audio_path
            .as_ref()
            .is_some_and(|path| path.trim().is_empty())
        {
            clip.audio_path = None;
        }
        clip.notes.retain_mut(|note| {
            note.start_substep = note.start_substep.min(15);
            note.duration_substeps = note.duration_substeps.min(15);
            note.duration_steps = note.duration_steps.max(1);
            note.pitch = note.pitch.min(127);
            note.velocity = note.velocity.clamp(1, 127);
            note.repeat_count = note.repeat_count.min(32);
            note.repeat_ticks = note.repeat_ticks.min(64);
            note.repeat_decay = note.repeat_decay.min(126);
            true
        });
        clip.notes.sort_by_key(|note| (note.start_step, note.pitch));
    }

    /// Keeps the parent links valid and the group targets meaningful.
    fn normalize_tree(&mut self) {
        let track_count = self.tracks.len();
        let groups: Vec<bool> = self.tracks.iter().map(Track::is_group).collect();
        for index in 0..track_count {
            let parent = self.tracks[index].parent;
            let valid = parent
                .is_some_and(|parent| parent != index && parent < track_count && groups[parent]);
            if !valid {
                self.tracks[index].parent = None;
            }
        }
        // Break cycles by clearing the parent of every track that cannot reach
        // the root, which is cheaper than searching for the cycle itself.
        let mut safe = vec![false; track_count];
        for index in 0..track_count {
            let ancestors = Track::ancestors(&self.tracks, index);
            let rooted = ancestors
                .last()
                .and_then(|last| self.tracks.get(*last))
                .is_some_and(|track| track.parent.is_none());
            if rooted {
                for ancestor in ancestors {
                    safe[ancestor] = true;
                }
            } else if self.tracks[index].parent.is_some() {
                self.tracks[index].parent = None;
            }
        }
        for index in 0..track_count {
            let mut targets = std::mem::take(&mut self.tracks[index].group_targets);
            if !groups[index] {
                continue;
            }
            targets.retain(|parameter| {
                !parameter.is_group_wide()
                    && Track::any_descendant_supports(&self.tracks, index, *parameter)
            });
            targets.dedup();
            self.tracks[index].group_targets = targets;
        }
    }

    /// Drops automation lanes whose target no longer exists.
    fn normalize_automation(&mut self) {
        let track_count = self.tracks.len();
        let automation_count = self.automation_tracks.len();
        // Taken out of `self` so the closure can read the rest of the project
        // while it edits the lanes.
        let mut lanes = std::mem::take(&mut self.automation_tracks);
        lanes.retain_mut(|track| {
            track.length_steps = track.length_steps.max(1);
            let target_is_valid = match track.target {
                ParameterTarget::Track {
                    track_id,
                    parameter,
                } => {
                    track_id <= track_count
                        && self.track_parameter_value(track_id, parameter).is_some()
                }
                ParameterTarget::InsertEffect {
                    track_id, slot_id, ..
                } => {
                    let slots = if track_id == track_count {
                        &self.master.inserts
                    } else if let Some(entry) = self.tracks.get(track_id) {
                        &entry.inserts
                    } else {
                        return false;
                    };
                    slot_id < slots.len() && !slots[slot_id].name.is_empty()
                }
                // Hosted parameters and global targets stay valid as long as
                // the plug-in/engine is alive; nothing to prune here.
                ParameterTarget::PluginParameter { .. } => true,
                // Clip targets follow their clip: removing the clip prunes the lane.
                ParameterTarget::Clip {
                    track_id, clip_id, ..
                } => self
                    .tracks
                    .get(track_id)
                    .is_some_and(|entry| clip_id < entry.clips.len()),
                ParameterTarget::SampleDefaults { .. } => true,
                ParameterTarget::Tempo => true,
                ParameterTarget::AutomationLane { track_id, .. } => track_id < automation_count,
            };
            if !target_is_valid {
                return false;
            }
            for point in &mut track.points {
                // Relative lanes are offsets, so they may go negative; absolute
                // lanes stay inside the normalized parameter range.
                let limit = if track.source.is_relative() { 1.0 } else { 0.0 };
                point.value = finite_clamp(point.value, -limit, 1.0, 0.0);
            }
            for point in &mut track.modulation_points {
                point.value = finite_clamp(point.value, 0.0, 1.0, 1.0);
            }
            track.points.sort_by_key(|point| point.step);
            track.modulation_points.sort_by_key(|point| point.step);
            true
        });
        self.automation_tracks = lanes;
    }
}
