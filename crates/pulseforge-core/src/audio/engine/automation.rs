//! Evaluating automation: curves, modulators and envelopes.

use super::*;

impl AudioEngine {
    pub(crate) fn update_instrument_controls(&mut self, position: f64) {
        self.instrument_controls.resize(
            self.project.instruments.len(),
            InstrumentControls::default(),
        );
        for instrument_id in 0..self.project.instruments.len() {
            let instrument = &self.project.instruments[instrument_id];
            let mut values = instrument_controls(instrument, self.project.bpm);
            let automated = |parameter, base_value| {
                self.automated_value(
                    ParameterTarget::Instrument {
                        instrument_id,
                        parameter,
                    },
                    base_value,
                    position,
                )
            };
            values.gain = automated(InstrumentParameter::Gain, values.gain);
            values.pan = automated(InstrumentParameter::Pan, values.pan);
            values.cutoff_hz = automated(InstrumentParameter::CutoffHz, values.cutoff_hz);
            values.resonance = automated(InstrumentParameter::Resonance, values.resonance);
            values.pulse_width = automated(InstrumentParameter::PulseWidth, values.pulse_width);
            values.sub_level = automated(InstrumentParameter::SubLevel, values.sub_level);
            values.tone = automated(InstrumentParameter::Tone, values.tone);
            values.decay = automated(InstrumentParameter::Decay, values.decay);
            values.fm_ratio = automated(InstrumentParameter::FmRatio, values.fm_ratio);
            values.fm_index = automated(InstrumentParameter::FmIndex, values.fm_index);
            // Crossfade towards the new control set so knob moves and
            // automation ramps are audible as ramps, not steps. The blend
            // factor is derived from the same smoothing time as the gains.
            if self.smoothing_enabled && self.controls_snapped {
                let previous = self.instrument_controls[instrument_id];
                let blend = self.control_smoothing_blend();
                values = InstrumentControls {
                    gain: blend(previous.gain, values.gain),
                    pan: blend(previous.pan, values.pan),
                    cutoff_hz: blend(previous.cutoff_hz, values.cutoff_hz),
                    resonance: blend(previous.resonance, values.resonance),
                    pulse_width: blend(previous.pulse_width, values.pulse_width),
                    sub_level: blend(previous.sub_level, values.sub_level),
                    tone: blend(previous.tone, values.tone),
                    decay: blend(previous.decay, values.decay),
                    fm_ratio: blend(previous.fm_ratio, values.fm_ratio),
                    fm_index: blend(previous.fm_index, values.fm_index),
                };
            }
            self.instrument_controls[instrument_id] = values;
        }
        self.controls_snapped = true;
    }

    /// Applies automation for targets that are not instrument/insert knobs:
    /// hosted plug-in parameters, mixer volume/pan and the tempo. These are
    /// pushed straight to their owner instead of being read by the renderer.
    pub(crate) fn apply_global_automation(&mut self, position: f64) {
        for index in 0..self.project.automation_tracks.len() {
            let (enabled, target) = {
                let track = &self.project.automation_tracks[index];
                (track.enabled, track.target)
            };
            if !enabled {
                continue;
            }
            match target {
                ParameterTarget::PluginParameter {
                    host_id,
                    parameter_id,
                } => {
                    let base = self
                        .hosted
                        .get(&host_id)
                        .map_or(0.5, |slot| slot.parameter_base(parameter_id));
                    let value = self.automated_value(target, base, position);
                    if let Some(slot) = self.hosted.get_mut(&host_id) {
                        slot.set_parameter(parameter_id, value);
                    }
                }
                ParameterTarget::MixerChannel {
                    channel_id,
                    parameter,
                } => {
                    let base = self.project.parameter_value(target).unwrap_or(
                        if matches!(parameter, MixerParameter::Pan) {
                            0.0
                        } else if matches!(
                            parameter,
                            MixerParameter::LowDb | MixerParameter::MidDb | MixerParameter::HighDb
                        ) {
                            0.0
                        } else {
                            0.8
                        },
                    );
                    let value = self.automated_value(target, base, position);
                    match parameter {
                        MixerParameter::Gain => {
                            if let Some(gain) = self.lane_gain.get_mut(channel_id) {
                                gain.set_target(value);
                            }
                        }
                        MixerParameter::Pan => {
                            if let Some(pan) = self.lane_pan.get_mut(channel_id) {
                                pan.set_target(value);
                            }
                        }
                        MixerParameter::Width => {
                            if let Some(width) = self.lane_width.get_mut(channel_id) {
                                width.set_target(value);
                            }
                        }
                        MixerParameter::LowDb | MixerParameter::MidDb | MixerParameter::HighDb => {
                            // The channel EQ is used as a cached coefficient
                            // set, so the automated values are folded into it
                            // instead of being written back into the project.
                            // All three bands are resolved together, because any
                            // of them can be automated.
                            let stored = self.mixer_eq_bands(channel_id).unwrap_or_default();
                            let low = self.automated_value(
                                ParameterTarget::MixerChannel {
                                    channel_id,
                                    parameter: MixerParameter::LowDb,
                                },
                                stored.low_db,
                                position,
                            );
                            let mid = self.automated_value(
                                ParameterTarget::MixerChannel {
                                    channel_id,
                                    parameter: MixerParameter::MidDb,
                                },
                                stored.mid_db,
                                position,
                            );
                            let high = self.automated_value(
                                ParameterTarget::MixerChannel {
                                    channel_id,
                                    parameter: MixerParameter::HighDb,
                                },
                                stored.high_db,
                                position,
                            );
                            if let Some(gains) = self.eq_gains.get_mut(channel_id) {
                                *gains = eq_gains(low, mid, high);
                            }
                        }
                        MixerParameter::Send { destination } => {
                            let gain = finite_clamp(value, 0.0, MAX_SEND_GAIN_SUM, 0.0);
                            if let Some(edge) = self.send_edges.iter_mut().find(|edge| {
                                edge.source == channel_id && edge.destination == destination
                            }) {
                                edge.gain = gain;
                            } else if gain > f32::EPSILON {
                                // Automation can introduce a send the project
                                // does not store, so the edge is created on the
                                // fly.
                                self.send_edges.push(SendEdge {
                                    source: channel_id,
                                    destination,
                                    gain,
                                    delayed: [0.0; 2],
                                });
                            }
                        }
                    }
                }
                ParameterTarget::Clip {
                    lane_id,
                    clip_id,
                    parameter,
                } => {
                    let Some(base) = self.project.parameter_value(target) else {
                        continue;
                    };
                    let value = self.automated_value(target, base, position);
                    let Some(clip) = self
                        .project
                        .lanes
                        .get_mut(lane_id)
                        .and_then(|lane| lane.clips.get_mut(clip_id))
                    else {
                        continue;
                    };
                    match parameter {
                        ClipParameter::Gain => {
                            clip.sample_gain = finite_clamp(value, 0.0, 1.0, clip.sample_gain)
                        }
                        ClipParameter::Pan => {
                            clip.sample_pan = finite_clamp(value, -1.0, 1.0, clip.sample_pan)
                        }
                        ClipParameter::Pitch => {
                            clip.sample_pitch = finite_clamp(value, -24.0, 24.0, clip.sample_pitch)
                        }
                    }
                }
                ParameterTarget::SampleDefaults { parameter } => {
                    let base = self.project.parameter_value(target).unwrap_or(0.8);
                    let value = self.automated_value(target, base, position);
                    match parameter {
                        ClipParameter::Gain => {
                            self.project.sample_gain = finite_clamp(value, 0.0, 1.0, 0.8)
                        }
                        ClipParameter::Pan => {
                            self.project.sample_pan = finite_clamp(value, -1.0, 1.0, 0.0)
                        }
                        ClipParameter::Pitch => {
                            self.project.sample_pitch = finite_clamp(value, -24.0, 24.0, 0.0)
                        }
                    }
                }
                ParameterTarget::Tempo => {
                    let value = self.automated_value(target, self.project.bpm, position);
                    self.project.bpm = finite_clamp(value, 20.0, 300.0, 120.0);
                }
                ParameterTarget::AutomationLane {
                    track_id,
                    parameter,
                } => {
                    let Some(base) = self.project.parameter_value(target) else {
                        continue;
                    };
                    let value = self.automated_value(target, base, position);
                    // Round first: `Shape` is an enum addressed as an integer.
                    let value = if matches!(parameter, AutomationLaneParameter::Shape) {
                        value.round()
                    } else {
                        value
                    };
                    let Some(track) = self.project.automation_tracks.get_mut(track_id) else {
                        continue;
                    };
                    match (&mut track.source, parameter) {
                        (
                            AutomationSource::MidiEnvelope { base, .. }
                            | AutomationSource::Lfo { base, .. }
                            | AutomationSource::Perlin { base, .. }
                            | AutomationSource::SampleAndHold { base, .. }
                            | AutomationSource::Peak { base, .. },
                            AutomationLaneParameter::Base,
                        ) => *base = finite_clamp(value, 0.0, 1.0, *base),
                        (
                            AutomationSource::MidiEnvelope { depth, .. }
                            | AutomationSource::Lfo { depth, .. }
                            | AutomationSource::Perlin { depth, .. }
                            | AutomationSource::SampleAndHold { depth, .. }
                            | AutomationSource::Peak { depth, .. },
                            AutomationLaneParameter::Depth,
                        ) => *depth = finite_clamp(value, 0.0, 1.0, *depth),
                        (
                            AutomationSource::Lfo { rate, .. }
                            | AutomationSource::Perlin { rate, .. }
                            | AutomationSource::SampleAndHold { rate, .. },
                            AutomationLaneParameter::RateAmount,
                        ) => {
                            // The amount is in the rate's own unit, so the clamp
                            // follows it: a 500 ms LFO must not be squeezed into
                            // 64 ms by the step-rate limit.
                            let limit = match rate.unit {
                                TimeUnit::Steps => 64.0,
                                TimeUnit::Milliseconds => 4_000.0,
                            };
                            rate.amount = finite_clamp(value, 0.1, limit, rate.amount);
                        }
                        (AutomationSource::Lfo { shape, .. }, AutomationLaneParameter::Shape) => {
                            *shape = LfoShape::from_index(value.max(0.0) as u32);
                        }
                        (
                            AutomationSource::MidiEnvelope { attack, .. }
                            | AutomationSource::Peak { attack, .. },
                            AutomationLaneParameter::Attack,
                        ) => attack.amount = finite_clamp(value, 0.1, 4_000.0, attack.amount),
                        (
                            AutomationSource::MidiEnvelope { decay, .. },
                            AutomationLaneParameter::Decay,
                        ) => decay.amount = finite_clamp(value, 0.1, 4_000.0, decay.amount),
                        (
                            AutomationSource::MidiEnvelope { release, .. }
                            | AutomationSource::Peak { release, .. },
                            AutomationLaneParameter::Release,
                        ) => release.amount = finite_clamp(value, 0.1, 4_000.0, release.amount),
                        _ => {}
                    }
                }
                // Parameters that live on the instrument and are read when a
                // note is played are written straight onto the project.
                ParameterTarget::Instrument {
                    instrument_id,
                    parameter,
                } if matches!(
                    parameter,
                    InstrumentParameter::Waveform
                        | InstrumentParameter::ArpMode
                        | InstrumentParameter::ArpGate
                        | InstrumentParameter::ArpOctaves
                        | InstrumentParameter::ArpRate
                        | InstrumentParameter::SynthNote
                        | InstrumentParameter::VoiceCount
                ) =>
                {
                    let Some(base) = self.project.parameter_value(target) else {
                        continue;
                    };
                    let value = self.automated_value(target, base, position);
                    // Enums and octaves are whole numbers.
                    let value = if parameter.is_discrete() {
                        value.round()
                    } else {
                        value
                    };
                    let Some(instrument) = self.project.instruments.get_mut(instrument_id) else {
                        continue;
                    };
                    match parameter {
                        InstrumentParameter::Waveform => {
                            instrument.signal_waveform =
                                crate::project::SignalWaveform::from_index(value.max(0.0) as u32);
                        }
                        InstrumentParameter::ArpMode => {
                            instrument.arpeggio.mode =
                                crate::project::ArpMode::from_index(value.max(0.0) as u32);
                        }
                        InstrumentParameter::ArpGate => {
                            instrument.arpeggio.gate = finite_clamp(value, 0.01, 1.0, 0.6)
                        }
                        InstrumentParameter::ArpOctaves => {
                            instrument.arpeggio.octaves = finite_clamp(value, 1.0, 8.0, 1.0) as u8
                        }
                        InstrumentParameter::ArpRate => {
                            instrument.arpeggio.rate.amount = finite_clamp(value, 0.1, 64.0, 1.0)
                        }
                        InstrumentParameter::SynthNote => {
                            instrument.synth_note = finite_clamp(value, 0.0, 127.0, 60.0) as u8
                        }
                        InstrumentParameter::VoiceCount => {
                            instrument.voices.voice_count =
                                finite_clamp(value, 1.0, 64.0, 16.0) as u8
                        }
                        _ => {}
                    }
                }
                ParameterTarget::Instrument { .. } | ParameterTarget::InsertEffect { .. } => {}
            }
        }
    }

    /// Per-control-block interpolation factor for parameters that are not
    /// driven by a `SmoothedValue`. One step of the control block relative to
    /// the configured smoothing time, clamped to a full jump.
    pub(crate) fn control_smoothing_blend(&self) -> impl Fn(f32, f32) -> f32 {
        let ramp = self.smoothing_samples.max(1) as f32;
        let amount = (CONTROL_INTERVAL as f32 / ramp).clamp(0.0, 1.0);
        move |current: f32, target: f32| current + (target - current) * amount
    }

    pub(crate) fn automated_value(
        &self,
        target: ParameterTarget,
        base_value: f32,
        position: f64,
    ) -> f32 {
        let Some(lane_ids) = self.automation_index.get(&target) else {
            return base_value;
        };
        let base_normalized = target.normalize_value(base_value);
        let mut curve_total: Option<f32> = None;
        let mut relative_total = 0.0;
        let mut additive = 0.0;
        for &track_id in lane_ids {
            let Some(track) = self.project.automation_tracks.get(track_id) else {
                continue;
            };
            if position < track.start_step as f64
                || position >= track.start_step.saturating_add(track.length_steps) as f64
            {
                continue;
            }
            if track.source.edits_points() {
                if track.points.is_empty() {
                    continue;
                }
                let value = Project::evaluate_curve(&track.points, position);
                // Relative lanes offset the parameter; their points may be
                // negative, so a lane can pull a value down as well as up.
                if track.source.is_relative() {
                    relative_total += value;
                    continue;
                }
                match &mut curve_total {
                    Some(total) => *total += value - track.points[0].value,
                    None => curve_total = Some(value),
                }
                continue;
            }
            let delta = match &track.source {
                AutomationSource::Curve | AutomationSource::RelativeCurve => 0.0,
                AutomationSource::MidiEnvelope {
                    instrument_id,
                    attack,
                    decay,
                    sustain,
                    release,
                    base,
                    depth,
                } => {
                    let level = self.midi_envelope_level(
                        track_id,
                        *instrument_id,
                        *attack,
                        *decay,
                        *sustain,
                        *release,
                    );
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + level * depth * multiplier - base_normalized
                }
                AutomationSource::Lfo {
                    shape,
                    rate,
                    retrigger,
                    base,
                    depth,
                } => {
                    let phase = self.lfo_phase(track_id, *rate, *retrigger, position);
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + lfo_wave(*shape, phase) * depth * multiplier - base_normalized
                }
                AutomationSource::SampleAndHold {
                    seed,
                    rate,
                    base,
                    depth,
                } => {
                    // One held value per rate cycle, seeded so two lanes read
                    // different noise.
                    let phase = self.lfo_phase(track_id, *rate, false, position);
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + sample_and_hold(phase, *seed) * depth * multiplier - base_normalized
                }
                AutomationSource::Perlin {
                    seed,
                    rate,
                    octaves,
                    base,
                    depth,
                } => {
                    let local_position = (position - track.start_step as f64).max(0.0);
                    let coordinate = match rate.unit {
                        TimeUnit::Steps => local_position / f64::from(rate.amount.max(0.001)),
                        TimeUnit::Milliseconds => {
                            let seconds_per_step = 60.0 / f64::from(self.project.bpm) / 4.0;
                            local_position * seconds_per_step
                                / f64::from((rate.amount / 1_000.0).max(0.001))
                        }
                    };
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + crate::project::perlin_noise_octaves(*seed, coordinate, *octaves)
                        * depth
                        * multiplier
                        - base_normalized
                }
                AutomationSource::Peak { base, depth, .. } => {
                    // The follower was advanced once for this control block by
                    // `update_peak_followers`, so reading it here is cheap.
                    let level = self.peak_levels.get(track_id).copied().unwrap_or(0.0);
                    let multiplier = if track.modulation_points.is_empty() {
                        1.0
                    } else {
                        Project::evaluate_curve(&track.modulation_points, position)
                    };
                    base + level * depth * multiplier - base_normalized
                }
            };
            additive += delta;
        }
        let normalized = curve_total.unwrap_or(base_normalized) + relative_total + additive;
        target.denormalize_value(normalized.clamp(0.0, 1.0))
    }

    /// Stored equaliser of a mixer channel, the master included.
    ///
    /// # Parameters
    ///
    /// - `channel_id`: mixer channel index; `lanes.len()` addresses the master.
    ///
    /// # Returns
    ///
    /// The channel's three bands, or `None` when the channel no longer exists.
    pub(crate) fn mixer_eq_bands(&self, channel_id: usize) -> Option<crate::project::ThreeBandEq> {
        if channel_id == self.project.lanes.len() {
            Some(self.project.master.eq)
        } else {
            self.project.lanes.get(channel_id).map(|lane| lane.eq)
        }
    }

    /// Advances the envelope follower of every peak lane by one control block,
    /// then clears the per-channel block peaks.    ///
    /// The follower keeps its own attack and release, so two lanes reading the
    /// same channel can react at different speeds.
    pub(crate) fn update_peak_followers(&mut self) {
        let block_seconds = CONTROL_INTERVAL as f32 / self.sample_rate.max(1.0);
        let bpm = self.project.bpm;
        for track_id in 0..self.project.automation_tracks.len() {
            // Copy the settings out first so the project borrow ends before the
            // follower state is written.
            let (channel_id, post_fx, attack, release) =
                match &self.project.automation_tracks[track_id].source {
                    AutomationSource::Peak {
                        channel_id,
                        post_fx,
                        attack,
                        release,
                        ..
                    } => (*channel_id, *post_fx, *attack, *release),
                    _ => continue,
                };
            let peaks = if post_fx {
                &self.lane_peak_post
            } else {
                &self.lane_peak_pre
            };
            let raw = peaks
                .get(channel_id)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let Some(current) = self.peak_levels.get(track_id).copied() else {
                continue;
            };
            // Rise with the attack, fall with the release, like a meter with a
            // hold that never overshoots.
            let time = if raw > current {
                attack.seconds(bpm)
            } else {
                release.seconds(bpm)
            };
            let coefficient = if time <= 0.0 {
                1.0
            } else {
                1.0 - (-block_seconds / time).exp()
            };
            if let Some(level) = self.peak_levels.get_mut(track_id) {
                *level = (current + (raw - current) * coefficient).clamp(0.0, 1.0);
            }
        }
        // Start a fresh block maximum for the next control block.
        self.lane_peak_pre.fill(0.0);
        self.lane_peak_post.fill(0.0);
    }

    pub(crate) fn lfo_phase(
        &self,
        track_id: usize,
        rate: TimeValue,
        retrigger: bool,
        position: f64,
    ) -> f32 {
        let elapsed = if retrigger {
            let start = self
                .automation_runtime
                .get(track_id)
                .and_then(|state| state.start_frame)
                .unwrap_or(self.rendered_frames);
            self.rendered_frames.saturating_sub(start) as f64 / f64::from(self.sample_rate)
        } else {
            self.rendered_frames as f64 / f64::from(self.sample_rate)
        };
        let cycle_seconds = rate.seconds(self.project.bpm).max(0.001) as f64;
        let cycles = match rate.unit {
            TimeUnit::Steps => position.max(0.0) / rate.amount.max(0.001) as f64,
            TimeUnit::Milliseconds => elapsed / cycle_seconds,
        };
        cycles as f32
    }

    pub(crate) fn midi_envelope_level(
        &self,
        track_id: usize,
        instrument_id: usize,
        attack: TimeValue,
        decay: TimeValue,
        sustain: f32,
        release: TimeValue,
    ) -> f32 {
        let Some(state) = self.automation_runtime.get(track_id) else {
            return 0.0;
        };
        let Some(start_frame) = state.start_frame else {
            return 0.0;
        };
        let Some(track) = self.project.automation_tracks.get(track_id) else {
            return 0.0;
        };
        if !matches!(
            track.source,
            AutomationSource::MidiEnvelope {
                instrument_id: source_id,
                ..
            } if source_id == instrument_id
        ) {
            return 0.0;
        }
        let attack_seconds = attack.seconds(self.project.bpm).max(0.0);
        let decay_seconds = decay.seconds(self.project.bpm).max(0.0);
        let sustain = sustain.clamp(0.0, 1.0);
        let elapsed = self.rendered_frames.saturating_sub(start_frame) as f32 / self.sample_rate;
        let level = if attack_seconds > 0.0 && elapsed < attack_seconds {
            elapsed / attack_seconds
        } else if decay_seconds > 0.0 && elapsed < attack_seconds + decay_seconds {
            1.0 - (1.0 - sustain) * ((elapsed - attack_seconds) / decay_seconds)
        } else {
            sustain
        };
        let Some(release_frame) = state.release_frame else {
            return level.clamp(0.0, 1.0);
        };
        let release_elapsed =
            self.rendered_frames.saturating_sub(release_frame) as f32 / self.sample_rate;
        let release_seconds = release.seconds(self.project.bpm).max(0.001);
        (state.release_level * (1.0 - release_elapsed / release_seconds)).clamp(0.0, 1.0)
    }

    pub(crate) fn trigger_automation(&mut self, instrument_id: usize) {
        self.automation_runtime.resize(
            self.project.automation_tracks.len(),
            AutomationRuntime::default(),
        );
        for (track_id, track) in self.project.automation_tracks.iter().enumerate() {
            match &track.source {
                AutomationSource::MidiEnvelope {
                    instrument_id: source_id,
                    ..
                } if *source_id == instrument_id => {
                    if self.position_steps < track.start_step
                        || self.position_steps
                            >= track.start_step.saturating_add(track.length_steps)
                    {
                        continue;
                    }
                    self.automation_runtime[track_id] = AutomationRuntime {
                        start_frame: Some(self.rendered_frames),
                        ..AutomationRuntime::default()
                    };
                }
                AutomationSource::Lfo {
                    retrigger: true, ..
                } => {
                    if matches!(track.target, ParameterTarget::Instrument { instrument_id: id, .. } if id == instrument_id)
                    {
                        self.automation_runtime[track_id] = AutomationRuntime {
                            start_frame: Some(self.rendered_frames),
                            ..AutomationRuntime::default()
                        };
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn release_automation(&mut self, instrument_id: usize) {
        for track_id in 0..self.project.automation_tracks.len() {
            let source = self.project.automation_tracks[track_id].source.clone();
            let AutomationSource::MidiEnvelope {
                instrument_id: source_id,
                attack,
                decay,
                sustain,
                release,
                ..
            } = source
            else {
                continue;
            };
            if source_id != instrument_id {
                continue;
            }
            let level =
                self.midi_envelope_level(track_id, instrument_id, attack, decay, sustain, release);
            let state = &mut self.automation_runtime[track_id];
            state.release_frame = Some(self.rendered_frames);
            state.release_level = level;
        }
    }
}
