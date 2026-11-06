//! Step triggering, arpeggios, voice allocation and MIDI note handling.

use super::*;

impl AudioEngine {
    pub(crate) fn trigger_step(&mut self, position_tick: u64) {
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
    pub(crate) fn schedule_arpeggio(
        &mut self,
        note: &PianoNote,
        lane_id: usize,
        tick: u64,
    ) -> bool {
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
    pub(crate) fn drain_arpeggio(&mut self, tick: u64) {
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

    pub(crate) fn trigger_instrument(&mut self, instrument_id: usize, mixer_lane: usize) {
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

    pub(crate) fn next_audio_clips(&mut self, lane_id: usize) -> [f32; 2] {
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
    pub(crate) fn instrument_audible(&self, instrument_id: usize) -> bool {
        let Some(instrument) = self.project.instruments.get(instrument_id) else {
            return false;
        };
        if !instrument.enabled {
            return false;
        }
        !self.project.instrument_solo_active() || instrument.solo
    }

    /// Applies the track power switch and solo state.
    pub(crate) fn lane_audible(&self, lane_id: usize) -> bool {
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
    pub(crate) fn enforce_voice_limit(&mut self, instrument_id: usize) {
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

    pub(crate) fn start_voice(
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

    pub(crate) fn handle_midi_note(
        &mut self,
        instrument_id: usize,
        note: u8,
        velocity: u8,
        on: bool,
    ) {
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
