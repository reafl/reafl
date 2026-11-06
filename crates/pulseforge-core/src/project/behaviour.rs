//! What a [`Project`] can do: normalising, automation lookup, lane edits.

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
            lane.inserts.resize_with(INSERT_SLOTS, InsertFxSlot::default);
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
        self.master.inserts.resize_with(INSERT_SLOTS, InsertFxSlot::default);
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
            track.length_steps = track.length_steps.max(1);            let target_is_valid = match track.target {
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

    pub fn has_automation(&self, target: ParameterTarget) -> bool {
        self.automation_tracks
            .iter()
            .any(|track| track.enabled && track.target == target)
    }

    pub fn automated_value(&self, target: ParameterTarget, base_value: f32, step: f64) -> f32 {
        let position = step.max(0.0);
        let mut normalized = None;
        let mut relative = 0.0;
        let mut touched = false;
        for track in &self.automation_tracks {
            if !track.enabled
                || track.target != target
                || !track.source.edits_points()
                || track.points.is_empty()
                || position < track.start_step as f64
                || position >= track.start_step.saturating_add(track.length_steps) as f64
            {
                continue;
            }
            touched = true;
            let value = Self::evaluate_curve(&track.points, position);
            // Relative lanes offset the parameter instead of replacing it, so
            // several can be stacked and any of them may dip below zero.
            if track.source.is_relative() {
                relative += value;
                continue;
            }
            if let Some(sum) = &mut normalized {
                *sum += value - track.points[0].value;
            } else {
                normalized = Some(value);
            }
        }
        if !touched {
            return base_value;
        }
        let base = normalized.unwrap_or_else(|| target.normalize_value(base_value));
        target.denormalize_value((base + relative).clamp(0.0, 1.0))
    }

    pub fn evaluate_curve(points: &[AutomationPoint], position: f64) -> f32 {
        fn cubic_bezier(t: f32, control_1: f32, control_2: f32) -> f32 {
            let inverse = 1.0 - t;
            3.0 * inverse * inverse * t * control_1 + 3.0 * inverse * t * t * control_2 + t * t * t
        }

        match points {
            [] => 0.0,
            [only] => only.value,
            points if position <= points[0].step as f64 => points[0].value,
            points if position >= points[points.len() - 1].step as f64 => {
                points[points.len() - 1].value
            }
            points => {
                let upper = points.partition_point(|point| point.step as f64 <= position);
                let left = points[upper - 1];
                let right = points[upper];
                let distance = right.step.saturating_sub(left.step).max(1) as f32;
                let fraction = ((position - left.step as f64) as f32 / distance).clamp(0.0, 1.0);
                let shaped = match right.curve {
                    AutomationCurve::Linear => fraction,
                    AutomationCurve::Smooth => fraction * fraction * (3.0 - 2.0 * fraction),
                    AutomationCurve::Bezier => {
                        let (mut low, mut high) = (0.0_f32, 1.0_f32);
                        for _ in 0..12 {
                            let t = (low + high) * 0.5;
                            let x = cubic_bezier(t, 0.25, 0.25);
                            if x < fraction {
                                low = t;
                            } else {
                                high = t;
                            }
                        }
                        cubic_bezier((low + high) * 0.5, 0.1, 1.0)
                    }
                    AutomationCurve::Curved => {
                        (1.0 - (std::f32::consts::PI * fraction).cos()) * 0.5
                    }
                    AutomationCurve::Pulse => {
                        if fraction < 0.5 {
                            0.0
                        } else {
                            1.0
                        }
                    }
                    AutomationCurve::Constant => 0.0,
                    AutomationCurve::Steps => (fraction * 4.0).floor() / 4.0,
                };
                left.value + (right.value - left.value) * shaped
            }
        }
    }

    pub fn parameter_value(&self, target: ParameterTarget) -> Option<f32> {
        match target {
            // Hosted values are injected by the UI layer, which owns the
            // plug-in instance; the engine keeps the automation curve.
            ParameterTarget::PluginParameter { .. } => None,
            ParameterTarget::MixerChannel {
                channel_id,
                parameter,
            } => {
                let (gain, pan, width) = if channel_id == self.lanes.len() {
                    (self.master.gain, self.master.pan, 1.0)
                } else {
                    let lane = self.lanes.get(channel_id)?;
                    (lane.gain, lane.pan, lane.width)
                };
                Some(match parameter {
                    MixerParameter::Gain => gain,
                    MixerParameter::Pan => pan,
                    MixerParameter::Width => width,
                })
            }
            ParameterTarget::Tempo => Some(self.bpm),
            ParameterTarget::Instrument {
                instrument_id,
                parameter,
            } => {
                let instrument = self.instruments.get(instrument_id)?;
                Some(match parameter {
                    InstrumentParameter::Gain => instrument.gain,
                    InstrumentParameter::Pan => instrument.pan,
                    InstrumentParameter::CutoffHz => match instrument.synth_kind {
                        SynthKind::Ms20 => instrument.ms20_cutoff_hz,
                        SynthKind::JunoPoly => instrument.juno_cutoff_hz,
                        _ => return None,
                    },
                    InstrumentParameter::Resonance => match instrument.synth_kind {
                        SynthKind::Ms20 => instrument.ms20_resonance,
                        SynthKind::JunoPoly => instrument.juno_resonance,
                        _ => return None,
                    },
                    InstrumentParameter::PulseWidth => instrument.juno_pulse_width,
                    InstrumentParameter::SubLevel => instrument.juno_sub_level,
                    InstrumentParameter::Tone => instrument.m1_tone,
                    InstrumentParameter::Decay => TimeValue {
                        amount: match instrument.m1_decay_unit {
                            TimeUnit::Steps => instrument.m1_decay_steps,
                            TimeUnit::Milliseconds => instrument.m1_decay * 1_000.0,
                        },
                        unit: instrument.m1_decay_unit,
                    }
                    .seconds(self.bpm),
                    InstrumentParameter::FmRatio => instrument.fm_ratio,
                    InstrumentParameter::FmIndex => instrument.fm_index,
                    InstrumentParameter::Waveform => instrument.signal_waveform.index() as f32,
                    InstrumentParameter::ArpMode => instrument.arpeggio.mode.index() as f32,
                    InstrumentParameter::ArpGate => instrument.arpeggio.gate,
                    InstrumentParameter::ArpOctaves => f32::from(instrument.arpeggio.octaves),
                    InstrumentParameter::ArpRate => instrument.arpeggio.rate.amount,
                    InstrumentParameter::SynthNote => f32::from(instrument.synth_note),
                })
            }
            ParameterTarget::AutomationLane {
                track_id,
                parameter,
            } => {
                let track = self.automation_tracks.get(track_id)?;
                match (&track.source, parameter) {
                    (
                        AutomationSource::MidiEnvelope { base, .. }
                        | AutomationSource::Lfo { base, .. }
                        | AutomationSource::Perlin { base, .. }
                        | AutomationSource::SampleAndHold { base, .. },
                        AutomationLaneParameter::Base,
                    ) => Some(*base),
                    (
                        AutomationSource::MidiEnvelope { depth, .. }
                        | AutomationSource::Lfo { depth, .. }
                        | AutomationSource::Perlin { depth, .. }
                        | AutomationSource::SampleAndHold { depth, .. },
                        AutomationLaneParameter::Depth,
                    ) => Some(*depth),
                    (
                        AutomationSource::MidiEnvelope { .. }
                        | AutomationSource::Lfo { .. }
                        | AutomationSource::Perlin { .. }
                        | AutomationSource::SampleAndHold { .. },
                        AutomationLaneParameter::RateAmount,
                    ) => Some(match &track.source {
                        AutomationSource::MidiEnvelope { .. } => return None,
                        AutomationSource::Lfo { rate, .. }
                        | AutomationSource::Perlin { rate, .. }
                        | AutomationSource::SampleAndHold { rate, .. } => rate.amount,
                        AutomationSource::Curve | AutomationSource::RelativeCurve => return None,
                    }),
                    (
                        AutomationSource::Lfo { shape, .. },
                        AutomationLaneParameter::Shape,
                    ) => Some(shape.index() as f32),
                    (
                        AutomationSource::MidiEnvelope { sustain, .. },
                        AutomationLaneParameter::Sustain,
                    ) => Some(*sustain),
                    (
                        AutomationSource::Perlin { octaves, .. },
                        AutomationLaneParameter::Octaves,
                    ) => Some(*octaves as f32),
                    _ => None,
                }
            }
            ParameterTarget::InsertEffect {
                channel_id,
                slot_id,
                parameter,
            } => {
                let slots = if channel_id == self.lanes.len() {
                    &self.master.inserts
                } else {
                    &self.lanes.get(channel_id)?.inserts
                };
                let slot = slots.get(slot_id)?;
                Some(match parameter {
                    EffectParameter::Mix => slot.mix,
                    EffectParameter::Amount => slot.amount,
                    EffectParameter::LowDb => slot.eq.low_db,
                    EffectParameter::MidDb => slot.eq.mid_db,
                    EffectParameter::HighDb => slot.eq.high_db,
                    EffectParameter::TimeSeconds => slot.time_seconds_at_tempo(self.bpm),
                    EffectParameter::Feedback => slot.feedback,
                    EffectParameter::RateHz => slot.rate_hz,
                    EffectParameter::Depth => slot.depth,
                    EffectParameter::RoomSize => slot.room_size,
                    EffectParameter::Drive => slot.drive,
                })
            }
        }
    }

    pub fn add_lane(&mut self) {
        let old_lane_count = self.lanes.len();
        let new_len = old_lane_count + 1;
        for track in &mut self.automation_tracks {
            if let ParameterTarget::InsertEffect { channel_id, .. } = &mut track.target {
                if *channel_id == old_lane_count {
                    *channel_id = new_len;
                }
            }
        }
        self.lanes.push(ArrangementLane {
            name: format!("Track {new_len:02}"),
            ..ArrangementLane::default()
        });
        for row in &mut self.sends {
            row.insert(old_lane_count, 0.0);
        }
        self.sends.insert(old_lane_count, vec![0.0; new_len + 1]);
        self.sends[old_lane_count][new_len] = 0.25;
        self.normalize();
    }

    /// Moves track `from` to position `to`, keeping mixer sends, instrument
    /// routing and insert automation targets pointing at the same channels.
    pub fn move_lane(&mut self, from: usize, to: usize) {
        let lane_count = self.lanes.len();
        if from >= lane_count || to >= lane_count || from == to {
            return;
        }
        let master_index = lane_count;
        let lane = self.lanes.remove(from);
        self.lanes.insert(to, lane);
        // Permute the send matrix the same way, keeping the master row/column
        // last so audio routing still matches the new index order.
        if self.sends.len() == lane_count + 1
            && self.sends.iter().all(|row| row.len() == lane_count + 1)
        {
            let mut rows: Vec<Vec<f32>> = self.sends[..lane_count].to_vec();
            let row = rows.remove(from);
            rows.insert(to, row);
            let mut permuted = vec![vec![0.0; lane_count + 1]; lane_count + 1];
            for (new_row, source_row) in rows.iter().enumerate() {
                let mut values = source_row[..lane_count].to_vec();
                let value = values.remove(from);
                values.insert(to, value);
                permuted[new_row][..lane_count].copy_from_slice(&values);
                // Master column stays the last entry of each lane row.
                permuted[new_row][lane_count] = source_row[master_index];
            }
            // Master row follows the same column permutation.
            let master_row = self.sends[master_index].clone();
            let mut values = master_row[..lane_count].to_vec();
            let value = values.remove(from);
            values.insert(to, value);
            permuted[master_index][..lane_count].copy_from_slice(&values);
            self.sends = permuted;
        }
        let remap = |index: usize| -> usize {
            if index == master_index {
                master_index
            } else if index == from {
                to
            } else if from < to && index > from && index <= to {
                index - 1
            } else if to < from && index >= to && index < from {
                index + 1
            } else {
                index
            }
        };
        for instrument in &mut self.instruments {
            instrument.default_mixer_lane =
                remap(instrument.default_mixer_lane.min(lane_count - 1));
        }
        for track in &mut self.automation_tracks {
            if let ParameterTarget::InsertEffect { channel_id, .. } = &mut track.target {
                *channel_id = remap((*channel_id).min(master_index));
            }
        }
        self.normalize();
    }

    /// Reorders an automation lane so grouped rows can be rearranged.
    pub fn move_automation(&mut self, from: usize, to: usize) {
        if from >= self.automation_tracks.len() || to >= self.automation_tracks.len() || from == to
        {
            return;
        }
        let track = self.automation_tracks.remove(from);
        self.automation_tracks.insert(to, track);
    }

    /// Adds an automation lane, optionally positioned immediately after another
    /// lane so a parameter can carry several adjacent clips.
    pub fn add_automation_track(&mut self, track: AutomationTrack) -> usize {
        self.automation_tracks.push(track);
        self.automation_tracks.len() - 1
    }

    fn normalize_eq(eq: &mut ThreeBandEq) {
        eq.low_db = finite_clamp(eq.low_db, -12.0, 12.0, 0.0);
        eq.mid_db = finite_clamp(eq.mid_db, -12.0, 12.0, 0.0);
        eq.high_db = finite_clamp(eq.high_db, -12.0, 12.0, 0.0);
    }

    fn normalize_inserts(inserts: &mut [InsertFxSlot]) {
        for insert in inserts {
            insert.mix = finite_clamp(insert.mix, 0.0, 1.0, 0.5);
            insert.amount = finite_clamp(insert.amount, 0.0, 1.0, 0.35);
            Self::normalize_eq(&mut insert.eq);
            insert.time_seconds = finite_clamp(insert.time_seconds, 0.01, 1.0, 0.32);
            insert.time_steps = finite_clamp(insert.time_steps, 0.01, 256.0, 1.0);
            insert.feedback = finite_clamp(insert.feedback, 0.0, 1.0, 0.35);
            insert.rate_hz = finite_clamp(insert.rate_hz, 0.05, 8.0, 0.8);
            insert.depth = finite_clamp(insert.depth, 0.0, 1.0, 0.5);
            insert.room_size = finite_clamp(insert.room_size, 0.0, 1.0, 0.55);
            insert.drive = finite_clamp(insert.drive, 0.0, 1.0, 0.5);
            if insert.name.trim().is_empty() {
                insert.enabled = false;
            }
        }
    }

    /// True when some instrument is soloed, so playback should mute the rest.
    pub fn instrument_solo_active(&self) -> bool {
        self.instruments.iter().any(|instrument| instrument.solo)
    }

    /// True when some track is soloed.
    pub fn lane_solo_active(&self) -> bool {
        self.lanes.iter().any(|lane| lane.solo)
    }

    /// Adds or removes the step-sequencer note for `instrument_id` at `step`.
    ///
    /// Step-sequencer edits are stored as ordinary C5 piano-roll notes, so both
    /// editors show and edit the same data.
    pub fn set_step_note(&mut self, instrument_id: usize, step: u64, enabled: bool) {
        self.set_step_note_on_pattern(0, instrument_id, step, enabled);
    }

    /// Step-sequencer edit for a specific pattern.
    pub fn set_step_note_on_pattern(
        &mut self,
        pattern_id: usize,
        instrument_id: usize,
        step: u64,
        enabled: bool,
    ) {
        let pitch = STEP_SEQUENCER_PITCH;
        let Some(pattern) = self.patterns.get_mut(pattern_id) else {
            return;
        };
        let existing = pattern.notes.iter().position(|note| {
            note.instrument_id == instrument_id && note.pitch == pitch && note.start_step == step
        });
        match (enabled, existing) {
            (true, None) => pattern.notes.push(PianoNote {
                instrument_id,
                pitch,
                start_step: step,
                duration_steps: 1,
                velocity: 100,
                ..PianoNote::default()
            }),
            (true, Some(_)) => {}
            (false, Some(index)) => {
                pattern.notes.remove(index);
            }
            (false, None) => {}
        }
        pattern
            .notes
            .sort_by_key(|note| (note.start_step, note.pitch, note.instrument_id));
    }

    /// True when a step-sequencer note exists for this instrument and step.
    pub fn has_step_note(&self, pattern_id: usize, instrument_id: usize, step: u64) -> bool {
        self.patterns.get(pattern_id).is_some_and(|pattern| {
            pattern.notes.iter().any(|note| {
                note.instrument_id == instrument_id
                    && note.pitch == STEP_SEQUENCER_PITCH
                    && note.start_step == step
            })
        })
    }

    pub fn add_pattern(&mut self) -> usize {
        let id = self.patterns.len();
        self.patterns.push(Pattern {
            name: format!("Pattern {}", id + 1),
            ..Pattern::default()
        });
        id
    }

    pub fn duplicate_clip_after(&mut self, lane_id: usize, clip_index: usize) -> Option<usize> {
        let lane = self.lanes.get_mut(lane_id)?;
        let source = lane.clips.get(clip_index)?.clone();
        let start_step = source.start_step.saturating_add(source.length_steps.max(1));
        let insertion_index = lane
            .clips
            .iter()
            .take_while(|clip| clip.start_step <= start_step)
            .count();
        lane.clips.insert(
            insertion_index,
            ArrangementClip {
                start_step,
                length_steps: source.length_steps.max(1),
                source: source.source,
                offset_steps: 0,
                ..ArrangementClip::default()
            },
        );
        Some(insertion_index)
    }

    pub fn make_clip_pattern_unique(&mut self, lane_id: usize, clip_index: usize) -> Option<usize> {
        let pattern_id = match &self.lanes.get(lane_id)?.clips.get(clip_index)?.source {
            ClipSource::Pattern { pattern_id } => *pattern_id,
            ClipSource::Audio { .. } => return None,
        };
        let mut pattern = self.patterns.get(pattern_id)?.clone();
        let unique_id = self.patterns.len();
        pattern.name = format!("{} (unique {})", pattern.name, unique_id + 1);
        self.patterns.push(pattern);
        self.lanes[lane_id].clips[clip_index].source = ClipSource::Pattern {
            pattern_id: unique_id,
        };
        Some(unique_id)
    }
}

pub fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

pub fn normalize_sends(sends: &mut [Vec<f32>]) {
    let lane_count = sends.len();
    for row in sends.iter_mut() {
        row.resize(lane_count, 0.0);
        for gain in row.iter_mut() {
            *gain = finite_clamp(*gain, 0.0, MAX_SEND_GAIN_SUM, 0.0);
        }
    }
    let totals: Vec<f32> = (0..lane_count)
        .map(|destination| sends.iter().map(|row| row[destination].abs()).sum())
        .collect();
    for row in sends {
        for (destination, gain) in row.iter_mut().enumerate() {
            if totals[destination] > MAX_SEND_GAIN_SUM {
                *gain *= MAX_SEND_GAIN_SUM / totals[destination];
            }
        }
    }
}

pub fn disconnect_channel(sends: &mut [Vec<f32>], channel: usize) {
    for row in sends.iter_mut() {
        if let Some(gain) = row.get_mut(channel) {
            *gain = 0.0;
        }
    }
    if let Some(row) = sends.get_mut(channel) {
        row.fill(0.0);
    }
}

