//! Reading automation values and base parameter values.

use super::*;

impl Project {
    /// True when an enabled lane drives `target`.
    pub fn has_automation(&self, target: ParameterTarget) -> bool {
        self.automation_tracks
            .iter()
            .any(|track| track.enabled && track.target == target)
    }

    /// Resolves a parameter at a transport position.
    ///
    /// # Parameters
    ///
    /// - `target`: the parameter to resolve.
    /// - `base_value`: the value to use when no lane drives the parameter.
    /// - `step`: transport position in steps.
    ///
    /// # Returns
    ///
    /// The automated value, or `base_value` when nothing drives the target.
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

    /// Samples a drawn curve at a position, honouring each point's segment type.
    ///
    /// # Parameters
    ///
    /// - `points`: the curve, expected to be sorted by step.
    /// - `position`: position in steps.
    ///
    /// # Returns
    ///
    /// The interpolated value in 0..=1, or -1..=1 for a relative curve.
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

    /// The stored value of a parameter, without automation applied.
    ///
    /// # Parameters
    ///
    /// - `target`: the parameter to read.
    ///
    /// # Returns
    ///
    /// The value, or `None` when the target no longer exists.
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
                    (AutomationSource::Lfo { shape, .. }, AutomationLaneParameter::Shape) => {
                        Some(shape.index() as f32)
                    }
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
}
