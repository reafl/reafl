//! Range clamping for every numeric field of a project.

use super::*;

impl Project {
    /// Clamps the audio-clip playback controls to their documented ranges.
    fn normalize_sample_controls(&mut self) {
        self.sample_gain = finite_clamp(self.sample_gain, 0.0, 1.0, 0.8);
        self.sample_pan = finite_clamp(self.sample_pan, -1.0, 1.0, 0.0);
        self.sample_pitch = finite_clamp(self.sample_pitch, -24.0, 24.0, 0.0);
        for lane in &mut self.lanes {
            for clip in &mut lane.clips {
                clip.sample_gain = finite_clamp(clip.sample_gain, 0.0, 1.0, 0.8);
                clip.sample_pan = finite_clamp(clip.sample_pan, -1.0, 1.0, 0.0);
                clip.sample_pitch = finite_clamp(clip.sample_pitch, -24.0, 24.0, 0.0);
            }
        }
    }

    pub fn normalize(&mut self) {
        self.normalize_sample_controls();
        self.bpm = finite_clamp(self.bpm, 20.0, 300.0, 120.0);
        self.snap_ticks = self.snap_ticks.clamp(1, 1024);
        self.time_signature.numerator = self.time_signature.numerator.clamp(1, 128);
        self.time_signature.denominator = self.time_signature.denominator.clamp(1, 128);

        if self.patterns.is_empty() {
            self.patterns.push(Pattern::default());
        }

        if self.lanes.is_empty() {
            self.lanes.push(ArrangementLane::default());
        }
        if self.instruments.is_empty() {
            self.instruments.push(Instrument::default());
        }

        for instrument in &mut self.instruments {
            instrument.gain = finite_clamp(instrument.gain, 0.0, 1.0, 0.8);
            instrument.pan = finite_clamp(instrument.pan, -1.0, 1.0, 0.0);
            instrument.synth_note = instrument.synth_note.min(127);
            instrument.ms20_cutoff_hz =
                finite_clamp(instrument.ms20_cutoff_hz, 40.0, 16_000.0, 1_800.0);
            instrument.ms20_resonance = finite_clamp(instrument.ms20_resonance, 0.0, 0.9, 0.2);
            instrument.juno_cutoff_hz =
                finite_clamp(instrument.juno_cutoff_hz, 40.0, 16_000.0, 2_400.0);
            instrument.juno_resonance = finite_clamp(instrument.juno_resonance, 0.0, 0.9, 0.15);
            instrument.juno_pulse_width =
                finite_clamp(instrument.juno_pulse_width, 0.05, 0.95, 0.5);
            instrument.juno_sub_level = finite_clamp(instrument.juno_sub_level, 0.0, 1.0, 0.3);
            instrument.m1_tone = finite_clamp(instrument.m1_tone, 0.0, 1.0, 0.55);
            instrument.m1_decay = finite_clamp(instrument.m1_decay, 0.05, 4.0, 1.2);
            instrument.m1_decay_steps = finite_clamp(instrument.m1_decay_steps, 0.01, 256.0, 4.0);
            instrument.fm_ratio = finite_clamp(instrument.fm_ratio, 0.25, 16.0, 2.0);
            instrument.fm_index = finite_clamp(instrument.fm_index, 0.0, 1.0, 0.35);
            instrument.default_mixer_lane = instrument.default_mixer_lane.min(self.lanes.len() - 1);
            instrument.voices.voice_count = instrument.voices.voice_count.clamp(1, 64);
            instrument.voices.midi_in_channel = instrument.voices.midi_in_channel.min(15);
            instrument.voices.midi_out_channel = instrument.voices.midi_out_channel.min(15);
            instrument.voices.audio_in = instrument.voices.audio_in.min(15);
            instrument.voices.audio_out = instrument.voices.audio_out.min(15);
            instrument.arpeggio.gate = finite_clamp(instrument.arpeggio.gate, 0.05, 1.0, 0.6);
            instrument.arpeggio.octaves = instrument.arpeggio.octaves.clamp(1, 4);
            instrument.arpeggio.rate.amount =
                finite_clamp(instrument.arpeggio.rate.amount, 0.01, 64.0, 1.0);
        }
        for pattern in &mut self.patterns {
            pattern.length_steps = pattern.length_steps.max(1);
            pattern
                .step_hits
                .retain(|hit| hit.instrument_id < self.instruments.len());
            pattern
                .step_hits
                .sort_by_key(|hit| (hit.step, hit.instrument_id));
            pattern
                .notes
                .retain(|note| note.instrument_id < self.instruments.len());
            for note in &mut pattern.notes {
                note.start_substep = note.start_substep.min(15);
                note.duration_substeps = note.duration_substeps.min(15);
                note.duration_steps = note.duration_steps.max(1);
                note.pitch = note.pitch.min(127);
                note.velocity = note.velocity.clamp(1, 127);
                note.repeat_count = note.repeat_count.min(32);
                note.repeat_ticks = note.repeat_ticks.min(64);
                note.repeat_decay = note.repeat_decay.min(126);
            }
            pattern
                .notes
                .sort_by_key(|note| (note.start_step, note.pitch, note.instrument_id));
        }
        for lane in &mut self.lanes {
            lane.height = finite_clamp(lane.height, 34.0, 260.0, 54.0);
            lane.gain = finite_clamp(lane.gain, 0.0, 1.0, 0.8);
            lane.pan = finite_clamp(lane.pan, -1.0, 1.0, 0.0);
            lane.width = finite_clamp(lane.width, 0.0, 2.0, 1.0);
            Self::normalize_eq(&mut lane.eq);
            lane.inserts
                .resize_with(INSERT_SLOTS, InsertFxSlot::default);
            lane.inserts.truncate(INSERT_SLOTS);
            Self::normalize_inserts(&mut lane.inserts);
            lane.clips.retain_mut(|clip| {
                clip.length_steps = clip.length_steps.max(1);
                match &mut clip.source {
                    ClipSource::Pattern { pattern_id } => *pattern_id < self.patterns.len(),
                    ClipSource::Audio { path } => !path.trim().is_empty(),
                }
            });
            lane.clips.sort_by_key(|clip| clip.start_step);
        }

        let lane_count = self.lanes.len();
        let channel_count = lane_count + 1;
        let has_master_routes =
            self.sends.len() > lane_count || self.sends.iter().any(|row| row.len() > lane_count);
        self.sends.resize(channel_count, Vec::new());
        for row in &mut self.sends {
            row.resize(channel_count, 0.0);
            for gain in row {
                *gain = finite_clamp(*gain, 0.0, 0.95, 0.0);
            }
        }
        if !has_master_routes {
            for row in self.sends.iter_mut().take(lane_count) {
                row[lane_count] = 0.25;
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
        let totals: Vec<f32> = (0..channel_count)
            .map(|destination| self.sends.iter().map(|row| row[destination].abs()).sum())
            .collect();
        for row in &mut self.sends {
            for (destination, gain) in row.iter_mut().enumerate() {
                if totals[destination] > MAX_SEND_GAIN_SUM {
                    *gain *= MAX_SEND_GAIN_SUM / totals[destination];
                }
            }
        }
        for route in &mut self.midi_routes {
            *route = (*route).min(self.instruments.len() - 1);
        }
        // Lane-to-lane automation may only point at lanes that exist.
        let track_count = self.automation_tracks.len();
        self.automation_tracks.retain_mut(|track| {
            track.length_steps = track.length_steps.max(1);
            let target_is_valid = match track.target {
                ParameterTarget::Instrument { instrument_id, .. } => {
                    instrument_id < self.instruments.len()
                }
                ParameterTarget::InsertEffect {
                    channel_id,
                    slot_id,
                    ..
                } => {
                    let slots = if channel_id == lane_count {
                        &self.master.inserts
                    } else if let Some(lane) = self.lanes.get(channel_id) {
                        &lane.inserts
                    } else {
                        return false;
                    };
                    slot_id < slots.len() && !slots[slot_id].name.is_empty()
                }
                // Hosted parameters and global targets stay valid as long as
                // the plug-in/engine is alive; nothing to prune here.
                ParameterTarget::PluginParameter { .. } => true,
                ParameterTarget::MixerChannel { channel_id, .. } => channel_id <= lane_count,
                ParameterTarget::Tempo => true,
                ParameterTarget::AutomationLane { track_id, .. } => track_id < track_count,
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
    }
}
