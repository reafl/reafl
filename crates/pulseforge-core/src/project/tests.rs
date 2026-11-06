//! Tests for the project model and its behaviour.

use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_synth_kind_and_plugin_path_round_trip_through_json() {
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::External;
        project.instruments[0].plugin_path = "C:/Plugins/Thing.vst3".to_owned();
        let json = serde_json::to_string(&project).unwrap();
        let loaded: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.instruments[0].synth_kind, SynthKind::External);
        assert_eq!(loaded.instruments[0].plugin_path, "C:/Plugins/Thing.vst3");
    }

    #[test]
    fn builtin_synth_list_excludes_the_external_kind() {
        assert!(!SynthKind::BUILTIN.contains(&SynthKind::External));
        assert_eq!(SynthKind::BUILTIN.len(), 7);
    }

    #[test]
    fn step_sequencer_edits_are_c5_piano_notes() {
        let mut project = Project::default();
        project.set_step_note_on_pattern(0, 0, 4, true);
        assert!(project.has_step_note(0, 0, 4));
        let note = project.patterns[0]
            .notes
            .iter()
            .find(|note| note.instrument_id == 0 && note.start_step == 4)
            .copied()
            .expect("step edit should create a note");
        assert_eq!(note.pitch, STEP_SEQUENCER_PITCH);

        project.set_step_note_on_pattern(0, 0, 4, false);
        assert!(!project.has_step_note(0, 0, 4));
    }

    #[test]
    fn arp_modes_reorder_pitches_predictably() {
        let pitches = [60, 64, 67];
        assert_eq!(ArpMode::Up.order(&pitches), vec![60, 64, 67]);
        assert_eq!(ArpMode::Down.order(&pitches), vec![67, 64, 60]);
        assert_eq!(ArpMode::UpDown.order(&pitches), vec![60, 64, 67, 64]);
        assert_eq!(ArpMode::DownUp.order(&pitches), vec![67, 64, 60, 64]);
        assert_eq!(ArpMode::Order.order(&pitches), vec![60, 64, 67]);
        let mut random = ArpMode::Random.order(&pitches);
        random.sort_unstable();
        assert_eq!(random, vec![60, 64, 67]);
    }

    #[test]
    fn solo_and_power_flags_default_to_audible_and_are_reported() {
        let mut project = Project::default();
        assert!(!project.instrument_solo_active());
        assert!(!project.lane_solo_active());
        assert!(project.instruments[0].enabled);
        assert!(project.lanes[0].enabled);
        project.instruments[0].solo = true;
        project.lanes[1].solo = true;
        assert!(project.instrument_solo_active());
        assert!(project.lane_solo_active());
    }

    #[test]
    fn note_repeats_produce_spaced_triggers_with_falling_velocity() {
        let note = PianoNote {
            velocity: 100,
            repeat_count: 3,
            repeat_ticks: 4,
            repeat_decay: 10,
            duration_steps: 1,
            ..PianoNote::default()
        };
        let offsets = note.trigger_offsets();
        assert_eq!(offsets.len(), 4);
        assert_eq!(offsets[0], (0, 100));
        assert_eq!(offsets[1], (4, 90));
        assert_eq!(offsets[2], (8, 80));
        assert_eq!(offsets[3], (12, 70));
    }

    #[test]
    fn repeats_default_to_half_the_note_length() {
        let note = PianoNote {
            velocity: 64,
            repeat_count: 1,
            repeat_ticks: 0,
            duration_steps: 2,
            ..PianoNote::default()
        };
        let offsets = note.trigger_offsets();
        assert_eq!(offsets.len(), 2);
        assert_eq!(offsets[1].0, note.duration_ticks() / 2);
    }

    #[test]
    fn moving_a_lane_keeps_instrument_and_automation_targets_aligned() {
        let mut project = Project::default();
        project.add_lane();
        project.instruments[0].default_mixer_lane = 0;
        project.lanes[0].inserts[0].name = "Reverb".to_owned();
        project.lanes[0].inserts[0].enabled = true;
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::InsertEffect {
                channel_id: 0,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            },
            ..AutomationTrack::default()
        });
        project.normalize();
        let lane_name = project.lanes[0].name.clone();

        project.move_lane(0, 2);

        assert_eq!(project.lanes[2].name, lane_name);
        assert_eq!(project.instruments[0].default_mixer_lane, 2);
        assert_eq!(
            project.automation_tracks[0].target,
            ParameterTarget::InsertEffect {
                channel_id: 2,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            }
        );
    }

    #[test]
    fn lane_height_and_collapse_survive_normalization() {
        let mut project = Project::default();
        project.lanes[0].height = 5.0;
        project.lanes[0].automation_collapsed = true;
        project.normalize();
        assert!((project.lanes[0].height - 34.0).abs() < f32::EPSILON);
        assert!(project.lanes[0].automation_collapsed);
        project.lanes[0].height = 400.0;
        project.normalize();
        assert!((project.lanes[0].height - 260.0).abs() < f32::EPSILON);
    }

    #[test]
    fn extra_automation_lanes_can_target_the_same_parameter() {
        let mut project = Project::default();
        let track = AutomationTrack {
            start_step: 0,
            length_steps: 16,
            ..AutomationTrack::default()
        };
        let mut second = track.clone();
        second.start_step = 16;
        project.add_automation_track(track);
        project.add_automation_track(second);
        project.normalize();
        assert_eq!(project.automation_tracks.len(), 2);
        assert_eq!(
            project.automation_tracks[0].target,
            project.automation_tracks[1].target
        );
    }

    #[test]
    fn insert_slots_distinguish_builtin_and_external_plugins() {
        let mut slot = InsertFxSlot::default();
        assert!(!slot.is_external());
        slot.plugin_path = "C:/Plugins/Thing.vst3".to_owned();
        assert!(slot.is_external());
        // Effect parameters mirror the automatable slot values.
        slot.mix = 0.25;
        slot.feedback = 0.75;
        let parameters = slot.effect_parameters();
        assert!((parameters.mix - 0.25).abs() < f32::EPSILON);
        assert!((parameters.feedback - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn default_project_separates_instruments_patterns_and_lanes() {
        let project = Project::default();
        assert_eq!(project.instruments.len(), 4);
        assert_eq!(project.patterns.len(), 1);
        assert_eq!(project.lanes.len(), INITIAL_LANES);
        assert_eq!(project.patterns[0].length_steps, 64);
        assert_eq!(project.instruments[0].synth_kind, SynthKind::SamplePlayer);
        assert_eq!(project.instruments[0].default_sample, DefaultSample::Kick);
        assert_eq!(project.instruments[1].default_sample, DefaultSample::Snare);
        assert_eq!(project.instruments[2].default_sample, DefaultSample::HiHat);
    }

    #[test]
    fn signature_accepts_plain_numeric_values_and_computes_bar_length() {
        assert_eq!(
            TimeSignature {
                numerator: 7,
                denominator: 8
            }
            .steps_per_bar(),
            14
        );
        assert_eq!(
            TimeSignature {
                numerator: 3,
                denominator: 4
            }
            .steps_per_bar(),
            12
        );
    }

    #[test]
    fn time_values_convert_between_steps_and_milliseconds() {
        let four_steps = TimeValue {
            amount: 4.0,
            unit: TimeUnit::Steps,
        };
        let half_second = TimeValue {
            amount: 500.0,
            unit: TimeUnit::Milliseconds,
        };
        assert!((four_steps.seconds(120.0) - 0.5).abs() < f32::EPSILON);
        assert!((half_second.seconds(120.0) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn arrangement_clips_and_patterns_have_no_fixed_bar_limit() {
        let mut project = Project::default();
        project.patterns[0].length_steps = 4096;
        project.lanes[0].clips[0].length_steps = 4096;
        project.lanes[0].clips[0].start_step = 8192;
        project.normalize();

        assert_eq!(project.patterns[0].length_steps, 4096);
        assert_eq!(project.lanes[0].clips[0].start_step, 8192);
    }

    #[test]
    fn duplicate_clip_repeats_after_it_and_keeps_pattern_linked() {
        let mut project = Project::default();
        let original = project.lanes[0].clips[0].clone();

        let duplicate_id = project.duplicate_clip_after(0, 0).unwrap();
        let duplicate = &project.lanes[0].clips[duplicate_id];

        assert_eq!(
            duplicate.start_step,
            original.start_step + original.length_steps
        );
        assert_eq!(duplicate.length_steps, original.length_steps);
        assert!(matches!(
            &duplicate.source,
            ClipSource::Pattern { pattern_id: 0 }
        ));
        assert_eq!(project.lanes[0].clips[0].start_step, original.start_step);
    }

    #[test]
    fn making_a_pattern_clip_unique_clones_its_pattern_data() {
        let mut project = Project::default();
        project.duplicate_clip_after(0, 0).unwrap();
        let original_pattern = project.patterns[0].clone();
        let original_id = match project.lanes[0].clips[0].source {
            ClipSource::Pattern { pattern_id } => pattern_id,
            ClipSource::Audio { .. } => unreachable!(),
        };

        let unique_id = project.make_clip_pattern_unique(0, 1).unwrap();

        assert_eq!(unique_id, 1);
        assert_eq!(
            project.patterns[unique_id].step_hits.len(),
            original_pattern.step_hits.len()
        );
        assert_eq!(
            project.patterns[unique_id].notes.len(),
            original_pattern.notes.len()
        );
        assert!(matches!(
            &project.lanes[0].clips[0].source,
            ClipSource::Pattern { pattern_id } if *pattern_id == original_id
        ));
        assert!(matches!(
            &project.lanes[0].clips[1].source,
            ClipSource::Pattern { pattern_id } if *pattern_id == unique_id
        ));
    }

    #[test]
    fn adding_lanes_expands_mixer_routes() {
        let mut project = Project::default();
        project.add_lane();
        assert_eq!(project.lanes.len(), 5);
        assert_eq!(project.sends.len(), 6);
        assert!(project.sends.iter().all(|row| row.len() == 6));
        assert!(
            project.sends[..project.lanes.len()]
                .iter()
                .all(|row| row[project.lanes.len()] > 0.0)
        );
    }

    #[test]
    fn legacy_projects_gain_default_master_routes() {
        let mut project = Project::default();
        project.sends = vec![vec![0.0; project.lanes.len()]; project.lanes.len()];
        project.normalize();

        assert_eq!(project.sends.len(), project.lanes.len() + 1);
        assert!(
            project
                .sends
                .iter()
                .all(|row| row.len() == project.sends.len())
        );
        assert!(
            project.sends[..project.lanes.len()]
                .iter()
                .all(|row| row[project.lanes.len()] > 0.0)
        );
    }

    #[test]
    fn older_project_json_uses_defaults_for_new_mixer_and_synth_fields() {
        let mut serialized = serde_json::to_value(Project::default()).unwrap();
        let project = serialized.as_object_mut().unwrap();
        project.remove("mixer_collapsed");
        project["lanes"][0]
            .as_object_mut()
            .unwrap()
            .remove("pinned");
        project["instruments"][0]
            .as_object_mut()
            .unwrap()
            .remove("synth_kind");
        project.remove("automation_tracks");
        let loaded: Project = serde_json::from_value(serialized).unwrap();

        assert!(!loaded.mixer_collapsed);
        assert!(!loaded.lanes[0].pinned);
        assert_eq!(loaded.instruments[0].synth_kind, SynthKind::Sine);
        assert!(loaded.automation_tracks.is_empty());
    }

    #[test]
    fn automation_interpolates_normalized_values_and_holds_its_endpoints() {
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Gain,
        };
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            name: "Gain ramp".to_owned(),
            target,
            start_step: 0,
            length_steps: 64,
            points: vec![
                AutomationPoint {
                    step: 4,
                    value: 0.0,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 12,
                    value: 1.0,
                    curve: AutomationCurve::Linear,
                },
            ],
            modulation_points: Vec::new(),
            enabled: true,
            source: AutomationSource::Curve,
        });
        project.normalize();

        assert_eq!(project.automated_value(target, 0.8, 0.0), 0.0);
        assert!((project.automated_value(target, 0.8, 8.0) - 0.5).abs() < 0.0001);
        assert_eq!(project.automated_value(target, 0.8, 16.0), 1.0);
    }

    #[test]
    fn automation_clip_limits_curve_to_its_arrangement_range() {
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Gain,
        };
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            target,
            start_step: 8,
            length_steps: 8,
            points: vec![
                AutomationPoint {
                    step: 8,
                    value: 0.2,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 15,
                    value: 0.8,
                    curve: AutomationCurve::Linear,
                },
            ],
            ..AutomationTrack::default()
        });
        project.normalize();
        assert_eq!(project.automated_value(target, 0.5, 7.0), 0.5);
        assert_eq!(project.automated_value(target, 0.5, 8.0), 0.2);
        assert_eq!(project.automated_value(target, 0.5, 16.0), 0.5);
    }

    #[test]
    fn relative_automation_offsets_the_parameter_and_may_be_negative() {
        let mut project = Project::default();
        let target = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Gain,
        };
        project.automation_tracks.push(AutomationTrack {
            target,
            source: AutomationSource::RelativeCurve,
            start_step: 0,
            length_steps: 16,
            points: vec![
                AutomationPoint {
                    step: 0,
                    value: -0.5,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 15,
                    value: 0.25,
                    curve: AutomationCurve::Linear,
                },
            ],
            ..AutomationTrack::default()
        });
        project.normalize();
        // The lane is an offset: a base of 0.5 minus 0.5 is silence, and the end
        // of the lane pushes it up to 0.75.
        assert!((project.automated_value(target, 0.5, 0.0) - 0.0).abs() < 1e-4);
        assert!((project.automated_value(target, 0.5, 15.0) - 0.75).abs() < 1e-4);
        // A negative point survives normalization instead of clamping to zero.
        assert!(project.automation_tracks[0].points[0].value < 0.0);
    }

    #[test]
    fn enums_are_addressed_as_integers() {
        let mut project = Project::default();
        project.instruments.clear();
        project.instruments.push(Instrument {
            signal_waveform: SignalWaveform::Saw,
            arpeggio: Arpeggio {
                mode: ArpMode::UpDown,
                ..Arpeggio::default()
            },
            ..Instrument::default()
        });
        let waveform = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::Waveform,
        };
        let arp = ParameterTarget::Instrument {
            instrument_id: 0,
            parameter: InstrumentParameter::ArpMode,
        };
        assert_eq!(project.parameter_value(waveform), Some(2.0));
        assert_eq!(project.parameter_value(arp), Some(2.0));
        // Halfway along the range rounds to a whole index once applied.
        assert_eq!(waveform.denormalize_value(1.0), 3.0);
        assert_eq!(arp.denormalize_value(0.0), 0.0);
        assert!(InstrumentParameter::Waveform.is_discrete());
        assert!(!InstrumentParameter::Gain.is_discrete());
    }

    #[test]
    fn automation_lane_parameters_are_addressable() {
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            source: AutomationSource::Lfo {
                shape: LfoShape::Square,
                rate: TimeValue {
                    amount: 2.0,
                    unit: TimeUnit::Steps,
                },
                retrigger: true,
                base: 0.25,
                depth: 0.5,
            },
            ..AutomationTrack::default()
        });
        let lane = |parameter| ParameterTarget::AutomationLane {
            track_id: 0,
            parameter,
        };
        assert_eq!(
            project.parameter_value(lane(AutomationLaneParameter::Base)),
            Some(0.25)
        );
        assert_eq!(
            project.parameter_value(lane(AutomationLaneParameter::Depth)),
            Some(0.5)
        );
        assert_eq!(
            project.parameter_value(lane(AutomationLaneParameter::RateAmount)),
            Some(2.0)
        );
        assert_eq!(
            project.parameter_value(lane(AutomationLaneParameter::Shape)),
            Some(LfoShape::Square.index() as f32)
        );
        project.normalize();
        assert_eq!(project.automation_tracks.len(), 1);
    }

    #[test]
    fn legacy_automation_lane_gets_default_clip_bounds() {
        let track: AutomationTrack = serde_json::from_str(r#"{"name":"Legacy"}"#).unwrap();
        assert_eq!(track.start_step, 0);
        assert_eq!(track.length_steps, 64);
    }

    #[test]
    fn normalization_prunes_automation_targets_without_a_plugin() {
        let mut project = Project::default();
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::InsertEffect {
                channel_id: 0,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            },
            ..AutomationTrack::default()
        });
        project.normalize();

        assert!(project.automation_tracks.is_empty());
    }

    #[test]
    fn adding_a_lane_keeps_master_effect_automation_attached_to_master() {
        let mut project = Project::default();
        let master_id = project.lanes.len();
        project.master.inserts[0].name = "Delay".to_owned();
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::InsertEffect {
                channel_id: master_id,
                slot_id: 0,
                parameter: EffectParameter::Mix,
            },
            ..AutomationTrack::default()
        });

        project.add_lane();

        assert_eq!(
            project.automation_tracks[0].target,
            ParameterTarget::InsertEffect {
                channel_id: project.lanes.len(),
                slot_id: 0,
                parameter: EffectParameter::Mix,
            }
        );
    }

    #[test]
    fn disconnect_channel_clears_its_inputs_and_outputs() {
        let mut sends = vec![
            vec![0.0, 0.25, 0.5],
            vec![0.4, 0.0, 0.2],
            vec![0.1, 0.3, 0.0],
        ];
        disconnect_channel(&mut sends, 1);
        assert_eq!(sends[0][1], 0.0);
        assert!(sends[1].iter().all(|gain| *gain == 0.0));
        assert_eq!(sends[2][1], 0.0);
        assert_eq!(sends[0][2], 0.5);
        assert_eq!(sends[2][0], 0.1);
    }

    #[test]
    fn normalize_bounds_sends_and_prunes_invalid_references() {
        let mut project = Project::default();
        project.sends[0][1] = 0.9;
        project.sends[2][1] = 0.9;
        project.patterns[0].step_hits.push(StepHit {
            instrument_id: 999,
            step: 0,
        });
        project.normalize();

        let total: f32 = project.sends.iter().map(|row| row[1]).sum();
        assert!(total <= MAX_SEND_GAIN_SUM + 0.00001);
        assert!(
            project.patterns[0]
                .step_hits
                .iter()
                .all(|hit| hit.instrument_id < project.instruments.len())
        );
    }

    #[test]
    fn automation_curve_shapes_have_expected_interpolation() {
        let points = |curve| {
            [
                AutomationPoint {
                    step: 0,
                    value: 0.0,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 16,
                    value: 1.0,
                    curve,
                },
            ]
        };
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Linear), 8.0),
            0.5
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Constant), 8.0),
            0.0
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Pulse), 4.0),
            0.0
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Pulse), 12.0),
            1.0
        );
        assert_eq!(
            Project::evaluate_curve(&points(AutomationCurve::Steps), 4.0),
            0.25
        );
        let bezier = Project::evaluate_curve(&points(AutomationCurve::Bezier), 8.0);
        assert!(bezier > 0.5 && bezier < 1.0);
    }

    #[test]
    fn perlin_noise_is_deterministic_bounded_and_varies_across_positions() {
        let first = perlin_noise(17, 0.25);
        assert_eq!(first, perlin_noise(17, 0.25));
        assert!((-1.0..=1.0).contains(&first));
        assert_ne!(first, perlin_noise(17, 1.25));
    }

    #[test]
    fn perlin_octaves_add_detail_and_stay_bounded() {
        let single = perlin_noise_octaves(3, 0.37, 1);
        let detailed = perlin_noise_octaves(3, 0.37, 8);
        assert_eq!(single, perlin_noise(3, 0.37));
        assert!((-1.0..=1.0).contains(&detailed));
        assert_ne!(single, detailed);
        // Octave counts are clamped into the supported 1-16 range.
        assert_eq!(
            perlin_noise_octaves(3, 0.37, 0),
            perlin_noise_octaves(3, 0.37, 1)
        );
        assert_eq!(
            perlin_noise_octaves(3, 0.37, 99),
            perlin_noise_octaves(3, 0.37, 16)
        );
        // More octaves produce more direction changes across the same span.
        let reversals = |octaves| {
            let samples: Vec<f32> = (0..64)
                .map(|index| perlin_noise_octaves(5, index as f64 * 0.25, octaves))
                .collect();
            samples
                .windows(3)
                .filter(|window| {
                    let first = window[1] - window[0];
                    let second = window[2] - window[1];
                    first * second < 0.0
                })
                .count()
        };
        assert!(reversals(8) > reversals(1));
    }

    #[test]
    fn normalize_sorts_and_clamps_modulation_curve_points() {
        let mut project = Project::default();
        let track = AutomationTrack {
            modulation_points: vec![
                AutomationPoint {
                    step: 8,
                    value: 2.0,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    step: 2,
                    value: -1.0,
                    curve: AutomationCurve::Linear,
                },
            ],
            ..AutomationTrack::default()
        };
        project.automation_tracks.push(track);
        project.normalize();

        let points = &project.automation_tracks[0].modulation_points;
        assert_eq!(points[0].step, 2);
        assert_eq!(points[0].value, 0.0);
        assert_eq!(points[1].step, 8);
        assert_eq!(points[1].value, 1.0);
    }

    #[test]
    fn mixer_eq_send_and_clip_parameters_are_readable() {
        let mut project = Project::default();
        project.lanes[0].eq.low_db = -4.0;
        project.sends[0][1] = 0.3;
        project.lanes[0].clips.push(ArrangementClip {
            source: ClipSource::Audio {
                path: "clip.wav".to_owned(),
            },
            sample_gain: 0.4,
            sample_pan: -0.5,
            sample_pitch: 7.0,
            ..ArrangementClip::default()
        });
        let clip_id = project.lanes[0].clips.len() - 1;

        let eq = ParameterTarget::MixerChannel {
            channel_id: 0,
            parameter: MixerParameter::LowDb,
        };
        assert_eq!(project.parameter_value(eq), Some(-4.0));
        let send = ParameterTarget::MixerChannel {
            channel_id: 0,
            parameter: MixerParameter::Send { destination: 1 },
        };
        assert_eq!(project.parameter_value(send), Some(0.3));
        let pitch = ParameterTarget::Clip {
            lane_id: 0,
            clip_id,
            parameter: ClipParameter::Pitch,
        };
        assert_eq!(project.parameter_value(pitch), Some(7.0));
        assert_eq!(
            project.parameter_value(ParameterTarget::SampleDefaults {
                parameter: ClipParameter::Gain,
            }),
            Some(project.sample_gain)
        );
    }

    #[test]
    fn lanes_for_removed_clips_are_pruned() {
        let mut project = Project::default();
        project.lanes[0].clips.clear();
        project.automation_tracks.push(AutomationTrack {
            target: ParameterTarget::Clip {
                lane_id: 0,
                clip_id: 0,
                parameter: ClipParameter::Gain,
            },
            ..AutomationTrack::default()
        });
        project.normalize();
        assert!(project.automation_tracks.is_empty());
    }
}
