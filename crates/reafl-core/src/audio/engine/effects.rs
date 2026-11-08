//! Insert chains, their parameters and hosted instruments.

use super::*;

impl AudioEngine {
    pub(crate) fn automate_effect_parameters(
        &self,
        track_id: usize,
        slot_id: usize,
        mut parameters: EffectParameters,
        position: f64,
    ) -> EffectParameters {
        let automated = |parameter, value| {
            self.automated_value(
                ParameterTarget::InsertEffect {
                    track_id,
                    slot_id,
                    parameter,
                },
                value,
                position,
            )
        };
        parameters.mix = automated(EffectParameter::Mix, parameters.mix);
        parameters.amount = automated(EffectParameter::Amount, parameters.amount);
        parameters.low_db = automated(EffectParameter::LowDb, parameters.low_db);
        parameters.mid_db = automated(EffectParameter::MidDb, parameters.mid_db);
        parameters.high_db = automated(EffectParameter::HighDb, parameters.high_db);
        parameters.time_seconds = automated(EffectParameter::TimeSeconds, parameters.time_seconds);
        parameters.feedback = automated(EffectParameter::Feedback, parameters.feedback);
        parameters.rate_hz = automated(EffectParameter::RateHz, parameters.rate_hz);
        parameters.depth = automated(EffectParameter::Depth, parameters.depth);
        parameters.room_size = automated(EffectParameter::RoomSize, parameters.room_size);
        parameters.drive = automated(EffectParameter::Drive, parameters.drive);
        parameters
    }

    /// Refreshes the cached insert-effect parameters for the current control
    /// block, so the sample loop only copies values it already has.
    pub(crate) fn update_effect_parameters(&mut self, position: f64) {
        let track_count = self.project.tracks.len();
        for channel in 0..=track_count {
            let slot_count = self.effect_parameters[channel].len();
            for slot_id in 0..slot_count {
                let parameters = {
                    let slot = if channel == track_count {
                        &self.project.master.inserts[slot_id]
                    } else {
                        &self.project.tracks[channel].inserts[slot_id]
                    };
                    let mut parameters = slot.effect_parameters();
                    parameters.time_seconds = slot.time_seconds_at_tempo(self.project.bpm);
                    parameters
                };
                let parameters = if self.playing {
                    self.automate_effect_parameters(channel, slot_id, parameters, position)
                } else {
                    parameters
                };
                // Smooth effect knobs the same way as instrument controls.
                let parameters = if self.smoothing_enabled && self.controls_snapped {
                    let previous = self.effect_parameters[channel][slot_id];
                    let blend = self.control_smoothing_blend();
                    EffectParameters {
                        mix: blend(previous.mix, parameters.mix),
                        amount: blend(previous.amount, parameters.amount),
                        low_db: blend(previous.low_db, parameters.low_db),
                        mid_db: blend(previous.mid_db, parameters.mid_db),
                        high_db: blend(previous.high_db, parameters.high_db),
                        time_seconds: blend(previous.time_seconds, parameters.time_seconds),
                        feedback: blend(previous.feedback, parameters.feedback),
                        rate_hz: blend(previous.rate_hz, parameters.rate_hz),
                        depth: blend(previous.depth, parameters.depth),
                        room_size: blend(previous.room_size, parameters.room_size),
                        drive: blend(previous.drive, parameters.drive),
                    }
                } else {
                    parameters
                };
                self.effect_parameters[channel][slot_id] = parameters;
            }
        }
    }

    /// Runs one insert slot. Hosted inserts render through their block-buffered
    /// slot, built-in ones run their own DSP.
    pub(crate) fn run_insert(
        &mut self,
        track_id: usize,
        slot_id: usize,
        parameters: EffectParameters,
    ) {
        let Some(processor) = self
            .effect_processors
            .get_mut(track_id)
            .and_then(|slots| slots.get_mut(slot_id))
        else {
            return;
        };
        match processor {
            Some(EffectProcessor::External(host_id)) => {
                let id = *host_id;
                let frame = self.track_inputs[track_id];
                if let Some(hosted) = self.hosted.get_mut(&id) {
                    self.track_inputs[track_id] = hosted.render_effect(frame);
                }
            }
            Some(EffectProcessor::Builtin(effect)) => {
                let frame = self.track_inputs[track_id];
                self.track_inputs[track_id] = effect.process_with_parameters(frame, parameters);
            }
            None => {}
        }
    }

    /// Mixes every hosted generator into its instrument's mixer lane.
    pub(crate) fn render_hosted_instruments(&mut self, track_count: usize) {
        for index in 0..self.hosted_instruments.len() {
            let track_id = self.hosted_instruments[index];
            if !self.track_audible(track_id) {
                continue;
            }
            let Some(controls) = self.instrument_controls.get(track_id).copied() else {
                continue;
            };
            let id = instrument_host_id(track_id);
            let Some(frame) = self.hosted.get_mut(&id).map(HostedSlot::render_instrument) else {
                continue;
            };
            let lane = self
                .project
                .tracks
                .get(track_id)
                .map_or(0, |_| track_id)
                .min(track_count.saturating_sub(1));
            let left_gain = ((1.0 - controls.pan) * 0.5).sqrt() * controls.gain;
            let right_gain = ((1.0 + controls.pan) * 0.5).sqrt() * controls.gain;
            self.track_inputs[lane][0] += frame[0] * left_gain;
            self.track_inputs[lane][1] += frame[1] * right_gain;
        }
    }
}
