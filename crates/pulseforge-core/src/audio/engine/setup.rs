//! Deriving the engine's cached state after a project change.
//!
//! Everything here rebuilds flattened caches (smoothed values, routing,
//! effect parameter slots, send edges) from the project, so the sample loop
//! reads arrays instead of walking the document.

use super::*;

impl AudioEngine {
    /// Re-creates the ramp lengths after the smoothing setting changes. A
    /// disabled smoother snaps straight to its target, which is exactly what a
    /// one-sample ramp does.
    pub(crate) fn apply_smoothing_settings(&mut self) {
        let ramp = self.smoothing_samples.max(1);
        for value in self
            .lane_gain
            .iter_mut()
            .chain(self.lane_pan.iter_mut())
            .chain(self.lane_width.iter_mut())
            .chain(self.effect_mix.iter_mut().flatten())
        {
            let target = value.target();
            *value = SmoothedValue::new(
                if self.smoothing_enabled {
                    value.value()
                } else {
                    target
                },
                ramp,
            );
            value.set_target(target);
        }
        let master_target = self.master_gain.target();
        self.master_gain = SmoothedValue::new(
            if self.smoothing_enabled {
                self.master_gain.value()
            } else {
                master_target
            },
            ramp,
        );
        self.master_gain.set_target(master_target);
        let pan_target = self.master_pan.target();
        self.master_pan = SmoothedValue::new(
            if self.smoothing_enabled {
                self.master_pan.value()
            } else {
                pan_target
            },
            ramp,
        );
        self.master_pan.set_target(pan_target);
    }

    /// Points every ramp at the value the project currently asks for.
    ///
    /// Existing ramps retarget smoothly; ramps created for the first time start
    /// at their target so a freshly loaded project never fades in from silence.
    pub(crate) fn retarget_smoothing(&mut self) {
        let ramp = if self.smoothing_enabled {
            self.smoothing_samples.max(1)
        } else {
            1
        };
        let lane_count = self.project.lanes.len();
        let mut gains = Vec::with_capacity(lane_count);
        let mut pans = Vec::with_capacity(lane_count);
        let mut widths = Vec::with_capacity(lane_count);
        let mut mixes: Vec<Vec<SmoothedValue>> = Vec::with_capacity(lane_count + 1);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            let mut gain = self
                .lane_gain
                .get(channel)
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(lane.gain, ramp));
            let mut pan = self
                .lane_pan
                .get(channel)
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(lane.pan, ramp));
            let mut width = self
                .lane_width
                .get(channel)
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(lane.width, ramp));
            gain = gain.with_ramp(ramp);
            pan = pan.with_ramp(ramp);
            width = width.with_ramp(ramp);
            gain.set_target(lane.gain);
            pan.set_target(lane.pan);
            width.set_target(lane.width);
            gains.push(gain);
            pans.push(pan);
            widths.push(width);
            let mut slots = Vec::with_capacity(lane.inserts.len());
            for (slot_id, slot) in lane.inserts.iter().enumerate() {
                let mut value = self
                    .effect_mix
                    .get(channel)
                    .and_then(|existing| existing.get(slot_id))
                    .copied()
                    .unwrap_or_else(|| SmoothedValue::new(slot.mix, ramp));
                value = value.with_ramp(ramp);
                value.set_target(slot.mix);
                slots.push(value);
            }
            mixes.push(slots);
        }
        let master_index = lane_count;
        let mut master_slots = Vec::with_capacity(self.project.master.inserts.len());
        for (slot_id, slot) in self.project.master.inserts.iter().enumerate() {
            let mut value = self
                .effect_mix
                .get(master_index)
                .and_then(|existing| existing.get(slot_id))
                .copied()
                .unwrap_or_else(|| SmoothedValue::new(slot.mix, ramp));
            value = value.with_ramp(ramp);
            value.set_target(slot.mix);
            master_slots.push(value);
        }
        mixes.push(master_slots);
        self.lane_gain = gains;
        self.lane_pan = pans;
        self.lane_width = widths;
        self.effect_mix = mixes;
        self.master_gain = self.master_gain.with_ramp(ramp);
        self.master_gain.set_target(self.project.master.gain);
        self.master_pan = self.master_pan.with_ramp(ramp);
        self.master_pan.set_target(self.project.master.pan);
    }

    pub(crate) fn resize_routing(&mut self) {
        let lane_count = self.project.lanes.len() + 1;
        self.audio_clips
            .resize_with(self.project.lanes.len(), Vec::new);
        for (lane_id, lane) in self.project.lanes.iter().enumerate() {
            self.audio_clips[lane_id].resize_with(lane.clips.len(), AudioClipRuntime::default);
        }
        let event_capacity: usize = self
            .project
            .lanes
            .iter()
            .flat_map(|lane| &lane.clips)
            .map(|clip| match clip.source {
                ClipSource::Pattern { pattern_id } => {
                    self.project.patterns.get(pattern_id).map_or(1, |pattern| {
                        1 + pattern.step_hits.len() + pattern.notes.len()
                    })
                }
                ClipSource::Audio { .. } => 1,
            })
            .sum::<usize>()
            + self
                .project
                .patterns
                .iter()
                .map(|pattern| pattern.step_hits.len() + pattern.notes.len())
                .sum::<usize>();
        self.trigger_events
            .reserve(event_capacity.saturating_sub(self.trigger_events.len()));
        self.lane_inputs.resize(lane_count, [0.0; 2]);
        self.meter_levels.resize(lane_count, 0.0);
        self.eq_states.resize(lane_count, EqState::default());
        self.eq_gains = self
            .project
            .lanes
            .iter()
            .map(|lane| eq_gains(lane.eq.low_db, lane.eq.mid_db, lane.eq.high_db))
            .chain(std::iter::once(eq_gains(
                self.project.master.eq.low_db,
                self.project.master.eq.mid_db,
                self.project.master.eq.high_db,
            )))
            .collect();
        self.effect_processors
            .resize_with(self.project.lanes.len() + 1, Vec::new);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            // A bypassed channel keeps its insert settings but renders with no
            // chain at all, so the dry signal costs nothing.
            let inserts: &[_] = if lane.fx_bypass { &[] } else { &lane.inserts };
            update_effect_chain(
                channel,
                inserts,
                &mut self.effect_processors[channel],
                self.sample_rate as u32,
            );
        }
        update_effect_chain(
            self.project.lanes.len(),
            &self.project.master.inserts,
            &mut self.effect_processors[self.project.lanes.len()],
            self.sample_rate as u32,
        );
        self.instrument_samples
            .resize_with(self.project.instruments.len(), || None);
        self.instrument_controls.resize(
            self.project.instruments.len(),
            InstrumentControls::default(),
        );
        self.automation_runtime.resize(
            self.project.automation_tracks.len(),
            AutomationRuntime::default(),
        );
        self.rebuild_send_edges();
        self.rebuild_automation_index();
        self.resize_effect_parameters();
        self.retarget_smoothing();
        self.control_countdown = 0;
    }

    pub(crate) fn resize_effect_parameters(&mut self) {
        self.effect_parameters.clear();
        self.effect_parameters
            .resize_with(self.project.lanes.len() + 1, Vec::new);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            self.effect_parameters[channel].resize(lane.inserts.len(), EffectParameters::default());
        }
        self.effect_parameters[self.project.lanes.len()].resize(
            self.project.master.inserts.len(),
            EffectParameters::default(),
        );
    }

    /// Refresh the mixer values that can change without resizing anything.
    /// Keeping this path allocation-light avoids audio dropouts while the user
    /// drags knobs or automation points.
    pub(crate) fn refresh_mix_state(&mut self) {
        self.eq_gains.clear();
        self.eq_gains.extend(
            self.project
                .lanes
                .iter()
                .map(|lane| eq_gains(lane.eq.low_db, lane.eq.mid_db, lane.eq.high_db)),
        );
        self.eq_gains.push(eq_gains(
            self.project.master.eq.low_db,
            self.project.master.eq.mid_db,
            self.project.master.eq.high_db,
        ));
        let channel_count = self.project.lanes.len() + 1;
        self.effect_processors.resize_with(channel_count, Vec::new);
        self.effect_processors.truncate(channel_count);
        for (channel, lane) in self.project.lanes.iter().enumerate() {
            // A bypassed channel keeps its insert settings but renders with no
            // chain at all, so the dry signal costs nothing.
            let inserts: &[_] = if lane.fx_bypass { &[] } else { &lane.inserts };
            update_effect_chain(
                channel,
                inserts,
                &mut self.effect_processors[channel],
                self.sample_rate as u32,
            );
        }
        update_effect_chain(
            self.project.lanes.len(),
            &self.project.master.inserts,
            &mut self.effect_processors[self.project.lanes.len()],
            self.sample_rate as u32,
        );
        self.rebuild_automation_index();
        self.resize_effect_parameters();
        self.retarget_smoothing();
        self.control_countdown = 0;
    }

    /// Rebuilds send edges while keeping their one-sample delay history, so
    /// editing the routing does not introduce clicks.
    pub(crate) fn rebuild_send_edges(&mut self) {
        normalize_sends(&mut self.project.sends);
        let mut previous = std::mem::take(&mut self.send_edges);
        self.send_edges.reserve(previous.len());
        for (source, row) in self.project.sends.iter().enumerate() {
            for (destination, gain) in row.iter().copied().enumerate() {
                if gain.abs() <= f32::EPSILON {
                    continue;
                }
                let delayed = previous
                    .iter()
                    .position(|edge| edge.source == source && edge.destination == destination)
                    .map_or([0.0; 2], |index| previous.swap_remove(index).delayed);
                self.send_edges.push(SendEdge {
                    source,
                    destination,
                    gain,
                    delayed,
                });
            }
        }
    }
}
