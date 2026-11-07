//! Reading automation values and base parameter values.

use super::*;

impl Project {
    /// True when an enabled lane drives `target`.
    pub fn has_automation(&self, target: ParameterTarget) -> bool {
        self.automation_tracks
            .iter()
            .any(|track| track.enabled && track.target == target)
    }

    /// The enabled lane driving `target`, if any.
    ///
    /// # Parameters
    ///
    /// - `target`: the parameter address to look up.
    pub fn automation_track_for(&self, target: ParameterTarget) -> Option<usize> {
        self.automation_tracks
            .iter()
            .position(|track| track.enabled && track.target == target)
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
            ParameterTarget::Track {
                track_id,
                parameter,
            } => self.track_parameter_value(track_id, parameter),
            ParameterTarget::Clip {
                track_id,
                clip_id,
                parameter,
            } => {
                let clip = self.tracks.get(track_id)?.clips.get(clip_id)?;
                Some(match parameter {
                    ClipParameter::Gain => clip.sample_gain,
                    ClipParameter::Pan => clip.sample_pan,
                    ClipParameter::Pitch => clip.sample_pitch,
                })
            }
            ParameterTarget::SampleDefaults { parameter } => Some(match parameter {
                ClipParameter::Gain => self.sample_gain,
                ClipParameter::Pan => self.sample_pan,
                ClipParameter::Pitch => self.sample_pitch,
            }),
            ParameterTarget::Tempo => Some(self.bpm),
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
                        | AutomationSource::SampleAndHold { base, .. }
                        | AutomationSource::Peak { base, .. },
                        AutomationLaneParameter::Base,
                    ) => Some(*base),
                    (
                        AutomationSource::MidiEnvelope { depth, .. }
                        | AutomationSource::Lfo { depth, .. }
                        | AutomationSource::Perlin { depth, .. }
                        | AutomationSource::SampleAndHold { depth, .. }
                        | AutomationSource::Peak { depth, .. },
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
                        AutomationSource::Curve
                        | AutomationSource::RelativeCurve
                        | AutomationSource::Peak { .. } => return None,
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
                    (
                        AutomationSource::MidiEnvelope { attack, .. }
                        | AutomationSource::Peak { attack, .. },
                        AutomationLaneParameter::Attack,
                    ) => Some(attack.amount),
                    (
                        AutomationSource::MidiEnvelope { decay, .. },
                        AutomationLaneParameter::Decay,
                    ) => Some(decay.amount),
                    (
                        AutomationSource::MidiEnvelope { release, .. }
                        | AutomationSource::Peak { release, .. },
                        AutomationLaneParameter::Release,
                    ) => Some(release.amount),
                    _ => None,
                }
            }
            ParameterTarget::InsertEffect {
                track_id,
                slot_id,
                parameter,
            } => {
                let slots = if track_id == self.tracks.len() {
                    &self.master.inserts
                } else {
                    &self.tracks.get(track_id)?.inserts
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

    /// The stored value of one track control, without automation.
    ///
    /// # Parameters
    ///
    /// - `track_id`: index into [`Project::tracks`], or `tracks.len()` for the
    ///   master bus (mixer controls only).
    /// - `parameter`: which control to read.
    ///
    /// # Returns
    ///
    /// The value, or `None` when the track is missing or the parameter does not
    /// exist on it (for example a generator control on a group).
    pub fn track_parameter_value(
        &self,
        track_id: usize,
        parameter: TrackParameter,
    ) -> Option<f32> {
        let is_master = track_id == self.tracks.len();
        if is_master {
            let TrackParameter::Mixer(parameter) = parameter else {
                return None;
            };
            return Some(match parameter {
                MixerParameter::Gain => self.master.gain,
                MixerParameter::Pan => self.master.pan,
                MixerParameter::Width => 1.0,
                MixerParameter::LowDb => self.master.eq.low_db,
                MixerParameter::MidDb => self.master.eq.mid_db,
                MixerParameter::HighDb => self.master.eq.high_db,
                MixerParameter::Send { destination } => {
                    *self.sends.get(track_id)?.get(destination)?
                }
            });
        }
        let track = self.tracks.get(track_id)?;
        match parameter {
            TrackParameter::Mixer(parameter) => Some(match parameter {
                MixerParameter::Gain => track.gain,
                MixerParameter::Pan => track.pan,
                MixerParameter::Width => track.width,
                MixerParameter::LowDb => track.eq.low_db,
                MixerParameter::MidDb => track.eq.mid_db,
                MixerParameter::HighDb => track.eq.high_db,
                MixerParameter::Send { destination } => {
                    *self.sends.get(track_id)?.get(destination)?
                }
            }),
            TrackParameter::MidiFx(parameter) => Some(match parameter {
                MidiFxParameter::Transpose => track.midi_fx.transpose as f32,
                MidiFxParameter::VelocityScale => track.midi_fx.velocity_scale,
            }),
            TrackParameter::Generator(parameter) => {
                let generator = track.generator()?;
                generator_parameter_value(generator, parameter, self.bpm)
            }
        }
    }

    /// The value a track actually uses at a transport position.
    ///
    /// # Parameters
    ///
    /// - `track_id`: index into [`Project::tracks`].
    /// - `parameter`: which control to resolve.
    /// - `step`: transport position in steps.
    ///
    /// # Returns
    ///
    /// Resolution order: the track's own automation lane, then the lane of the
    /// nearest ancestor group that has this parameter, then the stored value.
    /// This is the single evaluation path shared by the engine and the UI, so
    /// drawn curves and rendered audio cannot drift apart.
    pub fn effective_track_value(
        &self,
        track_id: usize,
        parameter: TrackParameter,
        step: f64,
    ) -> Option<f32> {
        let base = self.track_parameter_value(track_id, parameter)?;
        let own = ParameterTarget::Track {
            track_id,
            parameter,
        };
        if self.has_automation(own) {
            return Some(self.automated_value(own, base, step));
        }
        for ancestor in Track::ancestors(&self.tracks, track_id) {
            if !self.tracks.get(ancestor).is_some_and(|track| track.is_group()) {
                continue;
            }
            if !self.tracks[ancestor].supports(parameter) {
                continue;
            }
            let inherited = ParameterTarget::Track {
                track_id: ancestor,
                parameter,
            };
            if self.has_automation(inherited) {
                let group_base = self.track_parameter_value(ancestor, parameter)?;
                return Some(self.automated_value(inherited, group_base, step));
            }
        }
        Some(base)
    }
}

/// Reads one generator control.
///
/// # Parameters
///
/// - `generator`: the generator to read.
/// - `parameter`: which control to read.
/// - `bpm`: tempo, needed to convert musical decay times to seconds.
///
/// # Returns
///
/// The value, or `None` when the current model has no such control.
fn generator_parameter_value(
    generator: &Generator,
    parameter: InstrumentParameter,
    bpm: f32,
) -> Option<f32> {
    if !generator.supports(parameter) {
        return None;
    }
    Some(match parameter {
        InstrumentParameter::Gain => generator.gain,
        InstrumentParameter::Pan => generator.pan,
        InstrumentParameter::CutoffHz => match generator.kind.synth_kind() {
            Some(SynthKind::JunoPoly) => generator.juno_cutoff_hz,
            _ => generator.ms20_cutoff_hz,
        },
        InstrumentParameter::Resonance => match generator.kind.synth_kind() {
            Some(SynthKind::JunoPoly) => generator.juno_resonance,
            _ => generator.ms20_resonance,
        },
        InstrumentParameter::PulseWidth => generator.juno_pulse_width,
        InstrumentParameter::SubLevel => generator.juno_sub_level,
        InstrumentParameter::Tone => generator.m1_tone,
        InstrumentParameter::Decay => TimeValue {
            amount: match generator.m1_decay_unit {
                TimeUnit::Steps => generator.m1_decay_steps,
                TimeUnit::Milliseconds => generator.m1_decay * 1_000.0,
            },
            unit: generator.m1_decay_unit,
        }
        .seconds(bpm),
        InstrumentParameter::FmRatio => generator.fm_ratio,
        InstrumentParameter::FmIndex => generator.fm_index,
        InstrumentParameter::Waveform => generator.signal_waveform.index() as f32,
        InstrumentParameter::ArpMode => generator.arpeggio.mode.index() as f32,
        InstrumentParameter::ArpGate => generator.arpeggio.gate,
        InstrumentParameter::ArpOctaves => f32::from(generator.arpeggio.octaves),
        InstrumentParameter::ArpRate => generator.arpeggio.rate.amount,
        InstrumentParameter::SynthNote => f32::from(generator.synth_note),
        InstrumentParameter::VoiceCount => f32::from(generator.voices.voice_count),
    })
}
