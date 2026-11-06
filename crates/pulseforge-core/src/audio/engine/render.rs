//! Rendering frames, applying commands and playing notes.

use super::*;

impl AudioEngine {
    pub(crate) fn render_frame(
        &mut self,
        receiver: &mpsc::Receiver<AudioCommand>,
        project_updates: &Mutex<Option<Box<Project>>>,
    ) -> [f32; 2] {
        self.apply_pending(receiver, project_updates);
        let automation_position =
            self.position_steps as f64 + self.frames_into_step / self.frames_per_step().max(1.0);
        if self.control_countdown == 0 {
            if self.playing {
                self.update_instrument_controls(automation_position);
                self.apply_global_automation(automation_position);
            } else {
                self.instrument_controls.resize(
                    self.project.instruments.len(),
                    InstrumentControls::default(),
                );
                let bpm = self.project.bpm;
                for (controls, instrument) in self
                    .instrument_controls
                    .iter_mut()
                    .zip(&self.project.instruments)
                {
                    *controls = instrument_controls(instrument, bpm);
                }
            }
            self.update_effect_parameters(automation_position);
        }
        self.control_countdown += 1;
        if self.control_countdown >= CONTROL_INTERVAL {
            self.control_countdown = 0;
        }
        if self.playing {
            let position_tick = self.position_steps.saturating_mul(16)
                + ((self.frames_into_step / self.frames_per_step().max(1.0)) * 16.0).floor() as u64;
            if self.last_triggered_tick != Some(position_tick) {
                self.trigger_step(position_tick);
                self.last_triggered_tick = Some(position_tick);
            }
        }

        let lane_count = self.project.lanes.len();
        let master_index = lane_count;
        self.lane_inputs.fill([0.0; 2]);
        for edge in &self.send_edges {
            for channel in 0..2 {
                self.lane_inputs[edge.destination][channel] += edge.delayed[channel];
            }
        }

        for lane_id in 0..lane_count {
            let audio_samples = self.next_audio_clips(lane_id);
            self.lane_inputs[lane_id][0] += audio_samples[0];
            self.lane_inputs[lane_id][1] += audio_samples[1];
        }

        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let Some(instrument) = self.project.instruments.get(voice.instrument_id) else {
                voice.active = false;
                continue;
            };
            if !instrument.synth_enabled || !instrument.enabled {
                voice.active = false;
                continue;
            }
            if self.project.instrument_solo_active() && !instrument.solo {
                voice.active = false;
                continue;
            }
            let controls = self.instrument_controls[voice.instrument_id];
            let frequency = 440.0 * 2.0_f32.powf((f32::from(voice.note) - 69.0) / 12.0);
            let raw = match instrument.synth_kind {
                SynthKind::SamplePlayer | SynthKind::External => 0.0,
                SynthKind::Sine => voice.phase.sin(),
                SynthKind::SignalGenerator => waveform(instrument.signal_waveform, voice.phase),
                SynthKind::Ms20 => {
                    let oscillator_a = waveform(SignalWaveform::Saw, voice.phase);
                    let oscillator_b = waveform(SignalWaveform::Saw, voice.second_phase);
                    (oscillator_a + oscillator_b * 0.65) * 0.6
                }
                SynthKind::JunoPoly => {
                    let saw = waveform(SignalWaveform::Saw, voice.phase);
                    let pulse = if (voice.phase / TAU).fract() < controls.pulse_width {
                        1.0
                    } else {
                        -1.0
                    };
                    saw * 0.45
                        + pulse * 0.35
                        + waveform(SignalWaveform::Square, voice.sub_phase) * controls.sub_level
                }
                SynthKind::M1Workstation => {
                    let brightness = controls.tone;
                    voice.phase.sin() * 0.65
                        + (voice.phase * 2.0).sin() * (0.25 + brightness * 0.2)
                        + (voice.phase * 3.0).sin() * brightness * 0.1
                }
                SynthKind::FmSynth => {
                    (voice.phase + voice.second_phase.sin() * controls.fm_index * 10.0).sin()
                }
            };
            let signal = if matches!(instrument.synth_kind, SynthKind::Ms20 | SynthKind::JunoPoly) {
                let cutoff = controls.cutoff_hz.min(self.sample_rate * 0.45);
                let alpha = 1.0 - (-TAU * cutoff / self.sample_rate).exp();
                let resonant_input = raw - voice.filter_state * controls.resonance;
                voice.filter_state += alpha * (resonant_input - voice.filter_state);
                let emphasis = if instrument.synth_kind == SynthKind::Ms20 {
                    2.2
                } else {
                    1.5
                };
                (voice.filter_state * emphasis).tanh()
            } else {
                raw
            };
            let decay = if instrument.synth_kind == SynthKind::M1Workstation {
                (-voice.age_seconds / controls.decay).exp()
            } else {
                1.0
            };
            let output = if instrument.synth_kind == SynthKind::SamplePlayer {
                0.0
            } else {
                signal * voice.velocity * voice.envelope * controls.gain * decay * 0.12
            };
            voice.phase = (voice.phase + TAU * frequency / self.sample_rate) % TAU;
            let second_frequency = frequency
                * match instrument.synth_kind {
                    SynthKind::Ms20 => 1.006,
                    SynthKind::FmSynth => controls.fm_ratio,
                    _ => 1.0,
                };
            voice.second_phase =
                (voice.second_phase + TAU * second_frequency / self.sample_rate) % TAU;
            voice.sub_phase = (voice.sub_phase + TAU * frequency * 0.5 / self.sample_rate) % TAU;
            voice.age_seconds += 1.0 / self.sample_rate;
            if voice.released {
                voice.envelope *= 0.9992;
                if voice.envelope < 0.0005 {
                    voice.active = false;
                }
            } else {
                voice.envelope = (voice.envelope + 0.0015).min(1.0);
            }
            let pan = controls.pan;
            let left_gain = ((1.0 - pan) * 0.5).sqrt();
            let right_gain = ((1.0 + pan) * 0.5).sqrt();
            let lane = voice.mixer_lane.min(lane_count.saturating_sub(1));
            self.lane_inputs[lane][0] += output * left_gain;
            self.lane_inputs[lane][1] += output * right_gain;
        }

        for voice in &mut self.sample_voices {
            if !voice.active {
                continue;
            }
            let Some(sample) = self
                .instrument_samples
                .get(voice.instrument_id)
                .and_then(Option::as_ref)
            else {
                voice.active = false;
                continue;
            };
            let sample_frame = interpolate_sample(sample, voice.cursor);
            voice.cursor += sample.sample_rate as f64 / self.sample_rate as f64;
            if voice.cursor >= sample.frames.len() as f64 {
                voice.active = false;
            } else {
                let lane = voice.mixer_lane.min(lane_count.saturating_sub(1));
                let controls = self.instrument_controls[voice.instrument_id];
                let left_gain = ((1.0 - controls.pan) * 0.5).sqrt() * controls.gain;
                let right_gain = ((1.0 + controls.pan) * 0.5).sqrt() * controls.gain;
                self.lane_inputs[lane][0] += sample_frame[0] * left_gain;
                self.lane_inputs[lane][1] += sample_frame[1] * right_gain;
            }
        }

        self.render_hosted_instruments(lane_count);

        for lane_id in 0..lane_count {
            if !self.lane_audible(lane_id) {
                self.lane_inputs[lane_id] = [0.0; 2];
                continue;
            }
            let gain = self.lane_gain[lane_id].advance();
            let pan = self.lane_pan[lane_id].advance();
            let left_gain = (1.0 - pan).clamp(0.0, 1.0);
            let right_gain = (1.0 + pan).clamp(0.0, 1.0);
            self.lane_inputs[lane_id][0] *= gain * left_gain;
            self.lane_inputs[lane_id][1] *= gain * right_gain;
            // Stereo width is a mid/side scale, applied before the EQ so the
            // shelves react to the widened signal.
            let width = self.lane_width[lane_id].advance();
            if (width - 1.0).abs() > 1e-4 {
                let frame = self.lane_inputs[lane_id];
                let mid = (frame[0] + frame[1]) * 0.5;
                let side = (frame[0] - frame[1]) * 0.5 * width;
                self.lane_inputs[lane_id] = [mid + side, mid - side];
            }
            self.lane_inputs[lane_id] = process_eq(
                self.lane_inputs[lane_id],
                &mut self.eq_states[lane_id],
                self.eq_gains[lane_id],
                self.eq_alpha_250,
                self.eq_alpha_2k,
            );
            for slot_id in 0..self.effect_processors[lane_id].len() {
                let mut parameters = self.effect_parameters[lane_id][slot_id];
                if let Some(mix) = self
                    .effect_mix
                    .get_mut(lane_id)
                    .and_then(|slots| slots.get_mut(slot_id))
                {
                    parameters.mix = mix.advance();
                }
                self.run_insert(lane_id, slot_id, parameters);
            }
        }

        if let Some(sample) = self.preview.as_ref() {
            let frame = interpolate_sample(sample, self.preview_cursor);
            self.preview_cursor += sample.sample_rate as f64 / self.sample_rate as f64;
            if self.preview_cursor >= sample.frames.len() as f64 {
                self.preview = None;
                self.preview_cursor = 0.0;
            }
            self.lane_inputs[master_index][0] += frame[0];
            self.lane_inputs[master_index][1] += frame[1];
        }

        if self.click_frames_left > 0 {
            let envelope = (self.click_frames_left as f32 / self.click_length_frames.max(1) as f32)
                .powi(3)
                * 0.5;
            self.click_phase =
                (self.click_phase + TAU * self.click_frequency / self.sample_rate) % TAU;
            let click = self.click_phase.sin() * envelope;
            self.lane_inputs[master_index][0] += click;
            self.lane_inputs[master_index][1] += click;
            self.click_frames_left -= 1;
        }

        let master_gain = self.master_gain.advance();
        let master_pan = self.master_pan.advance();
        let master_left = (1.0 - master_pan).clamp(0.0, 1.0);
        let master_right = (1.0 + master_pan).clamp(0.0, 1.0);
        self.lane_inputs[master_index][0] *= master_gain * master_left;
        self.lane_inputs[master_index][1] *= master_gain * master_right;
        self.lane_inputs[master_index] = process_eq(
            self.lane_inputs[master_index],
            &mut self.eq_states[master_index],
            self.eq_gains[master_index],
            self.eq_alpha_250,
            self.eq_alpha_2k,
        );
        for slot_id in 0..self.effect_processors[master_index].len() {
            let mut parameters = self.effect_parameters[master_index][slot_id];
            if let Some(mix) = self
                .effect_mix
                .get_mut(master_index)
                .and_then(|slots| slots.get_mut(slot_id))
            {
                parameters.mix = mix.advance();
            }
            self.run_insert(master_index, slot_id, parameters);
        }

        for edge in &mut self.send_edges {
            for channel in 0..2 {
                edge.delayed[channel] = self.lane_inputs[edge.source][channel] * edge.gain;
            }
        }

        for (meter, signal) in self.meter_levels.iter_mut().zip(&self.lane_inputs) {
            let peak = signal[0].abs().max(signal[1].abs()).min(1.0);
            *meter = peak.max(*meter * 0.94);
        }
        let output = [
            self.lane_inputs[master_index][0].clamp(-0.95, 0.95),
            self.lane_inputs[master_index][1].clamp(-0.95, 0.95),
        ];
        if self.playing {
            self.frames_into_step += 1.0;
            if self.frames_into_step >= self.frames_per_step() {
                self.frames_into_step = 0.0;
                self.position_steps = self.position_steps.saturating_add(1);
                if let Some((start, end)) = self.loop_range
                    && self.position_steps >= end
                {
                    self.position_steps = start;
                    self.last_triggered_tick = None;
                }
            }
        }
        self.rendered_frames = self.rendered_frames.saturating_add(1);
        output
    }

    pub(crate) fn frames_per_step(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / f64::from(self.project.bpm) / 4.0
    }

    /// Fractional transport position in steps, used for smooth playhead
    /// animation on the UI thread.
    pub(crate) fn playhead_position(&self) -> f64 {
        self.position_steps as f64 + self.frames_into_step / self.frames_per_step().max(1.0)
    }
}
