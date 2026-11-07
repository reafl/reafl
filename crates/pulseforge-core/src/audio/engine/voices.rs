//! Clip scheduling, arpeggios, voice allocation and MIDI note handling.

use super::*;

impl AudioEngine {
    /// Fires every note and audio clip that starts at `position_tick`.
    ///
    /// # Parameters
    ///
    /// - `position_tick`: transport position in sixteenths of a step.
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
                self.releasing_instruments.push(voice.track_id);
            }
        }
        self.releasing_instruments.sort_unstable();
        self.releasing_instruments.dedup();
        for index in 0..self.releasing_instruments.len() {
            let track_id = self.releasing_instruments[index];
            self.release_automation(track_id);
        }
        self.trigger_events.clear();
        for (track_id, track) in self.project.tracks.iter().enumerate() {
            for (clip_index, clip) in track.clips.iter().enumerate() {
                if position < clip.start_step || position >= clip.end_step() {
                    continue;
                }
                if clip.is_audio() {
                    if position == clip.start_step && substep == 0 {
                        self.trigger_events.push(TriggerEvent::AudioClip {
                            track_id,
                            clip_index,
                        });
                    }
                    continue;
                }
                let local_tick = position
                    .saturating_sub(clip.start_step)
                    .saturating_add(clip.offset_steps)
                    .saturating_mul(16)
                    .saturating_add(u64::from(substep));
                for note in &clip.notes {
                    // A note may repeat; every repeat lands on its own tick.
                    let trigger = note
                        .trigger_offsets()
                        .into_iter()
                        .find(|(offset, _)| note.start_tick().saturating_add(*offset) == local_tick)
                        .map(|(_, velocity)| ClipNote { velocity, ..*note });
                    let Some(note) = trigger else {
                        continue;
                    };
                    // A clip on a folder plays through every source track below
                    // it, passing the folder's MIDI FX on the way down.
                    if track.is_group() {
                        for child in Track::descendants(&self.project.tracks, track_id) {
                            let Some(target) = self.project.tracks.get(child) else {
                                continue;
                            };
                            if target.is_group() {
                                continue;
                            }
                            let note = self.midi_chain_note(track_id, child, note);
                            self.trigger_events.push(TriggerEvent::Note {
                                track_id: child,
                                note,
                            });
                        }
                    } else {
                        self.trigger_events
                            .push(TriggerEvent::Note { track_id, note });
                    }
                }
            }
        }
        self.drain_arpeggio(position_tick);
        for index in 0..self.trigger_events.len() {
            match self.trigger_events[index] {
                TriggerEvent::Note { track_id, note } => {
                    if self.schedule_arpeggio(&note, track_id, position_tick) {
                        continue;
                    }
                    self.start_voice(
                        track_id,
                        note.pitch,
                        note.velocity,
                        Some(position_tick.saturating_add(note.duration_ticks())),
                    );
                }
                TriggerEvent::AudioClip {
                    track_id,
                    clip_index,
                } => {
                    let offset_steps = self
                        .project
                        .tracks
                        .get(track_id)
                        .and_then(|track| track.clips.get(clip_index))
                        .map_or(0, |clip| clip.offset_steps);
                    let seconds_per_step = 60.0 / f64::from(self.project.bpm) / 4.0;
                    if let Some(runtime) = self
                        .audio_clips
                        .get_mut(track_id)
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

    /// Applies the MIDI FX of every folder between `source` and `target`, then
    /// the target's own.
    ///
    /// # Parameters
    ///
    /// - `source`: the track owning the clip.
    /// - `target`: the source track that will play the note.
    /// - `note`: the note as written in the clip.
    ///
    /// # Returns
    ///
    /// The note after each folder in the path has processed it.
    pub(crate) fn midi_chain_note(
        &self,
        source: TrackId,
        target: TrackId,
        note: ClipNote,
    ) -> ClipNote {
        let mut note = note;
        let mut chain = Track::ancestors(&self.project.tracks, target);
        chain.reverse();
        for ancestor in chain {
            let stop = ancestor == source;
            if let Some(track) = self.project.tracks.get(ancestor) {
                note = track.midi_fx.process(note);
            }
            if stop {
                break;
            }
        }
        match self.project.tracks.get(target) {
            Some(track) => track.midi_fx.process(note),
            None => note,
        }
    }

    /// Expands a played note into arpeggiator events. Returns true when the
    /// track has an arpeggiator, so the original note must not sound.
    ///
    /// # Parameters
    ///
    /// - `note`: the note that was triggered.
    /// - `track_id`: the track that would play it.
    /// - `tick`: position of the note in sixteenths.
    pub(crate) fn schedule_arpeggio(
        &mut self,
        note: &ClipNote,
        track_id: usize,
        tick: u64,
    ) -> bool {
        let Some(generator) = self.project.tracks.get(track_id).and_then(Track::generator) else {
            return false;
        };
        let arp = generator.arpeggio;
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
                track_id,
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
                event.track_id,
                event.pitch,
                event.velocity,
                Some(event.release_tick),
            );
        }
    }

    /// Starts the one-shot sample of a sample-player track, if it has one.
    ///
    /// # Parameters
    ///
    /// - `track_id`: the track whose sample is played.
    fn start_sample_voice(&mut self, track_id: usize) {
        if !self
            .instrument_samples
            .get(track_id)
            .is_some_and(Option::is_some)
        {
            return;
        }
        let slot = self
            .sample_voices
            .iter()
            .position(|voice| !voice.active)
            .or_else(|| {
                self.sample_voices
                    .iter()
                    .position(|voice| voice.track_id == track_id)
            });
        if let Some(slot) = slot {
            self.sample_voices[slot] = SampleVoice {
                track_id,
                cursor: 0.0,
                active: true,
            };
        }
    }

    /// Streams the audio clips of one track.
    ///
    /// # Parameters
    ///
    /// - `track_id`: the track to render.
    ///
    /// # Returns
    ///
    /// The summed stereo frame of every audio clip on the track.
    pub(crate) fn next_audio_clips(&mut self, track_id: usize) -> [f32; 2] {
        let mut output = [0.0; 2];
        if !self.playing {
            // A stopped transport must not keep streaming clips; rewind so the
            // next start plays from the clip head.
            for runtime in &mut self.audio_clips[track_id] {
                runtime.active = false;
                runtime.finished = false;
                runtime.cursor = 0.0;
            }
            return output;
        }
        let clip_count = self.project.tracks[track_id].clips.len();
        let global_gain = self.project.sample_gain;
        let global_pan = self.project.sample_pan;
        let global_pitch = self.project.sample_pitch;
        // Steps are quarter-beats, so one step is 60/bpm/4 seconds.
        let seconds_per_step = 60.0 / f64::from(self.project.bpm) / 4.0;
        let position = self.position_steps;
        for clip_index in 0..clip_count {
            let (start, length, offset, clip_gain, clip_pan, clip_pitch) = {
                let clip = &self.project.tracks[track_id].clips[clip_index];
                if !clip.is_audio() {
                    continue;
                }
                (
                    clip.start_step,
                    clip.length_steps,
                    clip.offset_steps,
                    clip.sample_gain,
                    clip.sample_pan,
                    clip.sample_pitch,
                )
            };
            let gain = (global_gain * clip_gain).clamp(0.0, 1.0);
            let pan = (global_pan + clip_pan).clamp(-1.0, 1.0);
            let left_gain = ((1.0 - pan) * 0.5).sqrt() * gain;
            let right_gain = ((1.0 + pan) * 0.5).sqrt() * gain;
            // Semitone transpose is a playback-rate change.
            let rate = 2.0_f64.powf(f64::from(global_pitch + clip_pitch) / 12.0);
            let end = start.saturating_add(length);
            let runtime = &mut self.audio_clips[track_id][clip_index];
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

    /// Applies the track power switch and solo state.
    ///
    /// # Parameters
    ///
    /// - `track_id`: the track to test.
    ///
    /// # Returns
    ///
    /// `false` when the track is off, or when some other track is soloed.
    pub(crate) fn track_audible(&self, track_id: usize) -> bool {
        track_audible(&self.project, track_id, self.project.track_solo_active())
    }

    /// Frees the oldest voice for `track_id` when its configured limit is
    /// reached, so a low voice count actually constrains polyphony.
    ///
    /// # Parameters
    ///
    /// - `track_id`: the track whose voices are limited.
    pub(crate) fn enforce_voice_limit(&mut self, track_id: usize) {
        let limit = self
            .project
            .tracks
            .get(track_id)
            .and_then(Track::generator)
            .map_or(VOICE_COUNT, |generator| {
                usize::from(generator.voices.voice_count)
            })
            .clamp(1, VOICE_COUNT);
        let active = self
            .voices
            .iter()
            .filter(|voice| voice.active && voice.track_id == track_id)
            .count();
        if active < limit {
            return;
        }
        let mut to_free = active - limit + 1;
        for voice in self.voices.iter_mut() {
            if to_free == 0 {
                break;
            }
            if voice.active && voice.track_id == track_id {
                voice.active = false;
                to_free -= 1;
            }
        }
    }

    /// Starts a voice, a sample or a hosted note for one track.
    ///
    /// # Parameters
    ///
    /// - `track_id`: the track that plays the note.
    /// - `note`: MIDI note number.
    /// - `velocity`: MIDI velocity, 1..=127.
    /// - `release_at_tick`: position at which the note releases, `None` when it
    ///   is held until a matching note-off.
    pub(crate) fn start_voice(
        &mut self,
        track_id: usize,
        note: u8,
        velocity: u8,
        release_at_tick: Option<u64>,
    ) {
        let Some(generator) = self
            .project
            .tracks
            .get(track_id)
            .and_then(Track::generator)
            .cloned()
        else {
            return;
        };
        if velocity == 0 || !self.track_audible(track_id) {
            return;
        }
        // Sample players ignore the pitch: a note fires the loaded sample.
        if generator.kind.sample().is_some() {
            self.trigger_automation(track_id);
            self.start_sample_voice(track_id);
            return;
        }
        let Some(synth_kind) = generator.kind.synth_kind() else {
            return;
        };
        // External generators are hosted by VST3/CLAP; the note goes to the
        // hosted instance and this engine only mixes its output.
        if synth_kind == SynthKind::External {
            self.trigger_automation(track_id);
            let id = instrument_host_id(track_id);
            if let Some(hosted) = self.hosted.get_mut(&id) {
                hosted.play_note(note, f32::from(velocity) / 127.0);
            }
            return;
        }
        self.enforce_voice_limit(track_id);
        self.trigger_automation(track_id);
        let is_monophonic = matches!(synth_kind, SynthKind::Ms20);
        let slot = if is_monophonic {
            self.voices
                .iter()
                .position(|voice| voice.active && voice.track_id == track_id)
                .or_else(|| self.voices.iter().position(|voice| !voice.active))
        } else {
            self.voices
                .iter()
                .position(|voice| {
                    voice.active
                        && voice.track_id == track_id
                        && voice.note == note
                        && voice.release_at_tick.is_none() == release_at_tick.is_none()
                })
                .or_else(|| self.voices.iter().position(|voice| !voice.active))
        };
        if let Some(slot) = slot {
            self.voices[slot] = Voice {
                track_id,
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

    /// Handles a live MIDI note from the UI.
    ///
    /// # Parameters
    ///
    /// - `track_id`: the track the note was played on.
    /// - `note`: MIDI note number.
    /// - `velocity`: MIDI velocity, 1..=127.
    /// - `on`: `true` for note-on, `false` for note-off.
    pub(crate) fn handle_midi_note(&mut self, track_id: usize, note: u8, velocity: u8, on: bool) {
        if track_id >= self.project.tracks.len() {
            return;
        }
        if on && velocity > 0 {
            self.start_voice(track_id, note, velocity, None);
        } else {
            self.release_automation(track_id);
            let id = instrument_host_id(track_id);
            if let Some(hosted) = self.hosted.get_mut(&id) {
                hosted.note_off(note);
            }
            for voice in &mut self.voices {
                if voice.active
                    && voice.track_id == track_id
                    && voice.note == note
                    && voice.release_at_tick.is_none()
                {
                    voice.released = true;
                }
            }
        }
    }
}
