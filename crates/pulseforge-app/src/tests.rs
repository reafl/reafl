//! Unit tests for the editor state and its helpers.

use crate::*;
use pulseforge_widgets::fader_math::*;

/// Parameter target of a generator control on a track.
fn generator_target(track_id: usize, parameter: InstrumentParameter) -> ParameterTarget {
    ParameterTarget::Track {
        track_id,
        parameter: TrackParameter::Generator(parameter),
    }
}

#[test]
fn preset_names_become_safe_file_stems() {
    assert_eq!(preset_file_stem("Lead Chain"), "Lead Chain");
    assert_eq!(preset_file_stem("a/b:c*?"), "a_b_c__");
    assert_eq!(preset_file_stem("   "), "Preset");
}

#[test]
fn fader_curve_round_trips_and_snaps_to_silence() {
    assert_eq!(fader_gain(0.0), 0.0);
    assert!(fader_gain(1.0) - 1.0 < 1e-6);
    for position in [0.05, 0.25, 0.5, 0.9, 1.0] {
        let gain = fader_gain(position);
        assert!(
            (fader_position(gain) - position).abs() < 1e-3,
            "position {position} did not round-trip through {gain}"
        );
    }
    assert_eq!(fader_label(0.0), "-\u{221E} dB");
    assert!(gain_to_db(0.0) <= FADER_MIN_DB);
}

#[test]
fn envelope_preview_finds_clip_notes() {
    let mut project = Project::default();
    project.tracks[1].clips = vec![Clip {
        start_step: 0,
        length_steps: 8,
        notes: vec![ClipNote::new(2, 60, 100, 1), ClipNote::new(4, 60, 100, 1)],
        ..Clip::default()
    }];
    assert_eq!(
        automation_note_at(&project.tracks[1], 4.5),
        Some((4.0, 1.0))
    );
    assert!(automation_note_at(&project.tracks[1], 1.0).is_none());
    // A folder owns no notes of its own.
    assert!(automation_note_at(&project.tracks[0], 4.5).is_none());
}

#[test]
fn automation_source_preview_reacts_to_clip_notes() {
    let mut project = Project::default();
    project.tracks[1].clips = vec![Clip {
        start_step: 0,
        length_steps: 16,
        notes: vec![ClipNote::new(0, 60, 100, 1)],
        ..Clip::default()
    }];
    let source = project::AutomationSource::MidiEnvelope {
        track_id: 1,
        attack: project::TimeValue {
            amount: 0.0,
            unit: project::TimeUnit::Steps,
        },
        decay: project::TimeValue {
            amount: 0.0,
            unit: project::TimeUnit::Steps,
        },
        sustain: 1.0,
        release: project::TimeValue {
            amount: 1_000.0,
            unit: project::TimeUnit::Milliseconds,
        },
        base: 0.0,
        depth: 1.0,
    };
    let target = generator_target(1, InstrumentParameter::Gain);
    let level = automation_source_preview(&source, target, &project.tracks, 120.0, 0, 0.25, 0.0);
    assert!(level > 0.9, "envelope preview stayed at {level}");
}

#[test]
fn collapsed_tracks_hide_their_automation_rows() {
    let mut project = Project::default();
    project.automation_tracks.push(AutomationTrack {
        target: generator_target(1, InstrumentParameter::Gain),
        ..AutomationTrack::default()
    });
    let expanded = arrangement_layout(&project);
    assert!(
        expanded
            .iter()
            .any(|span| matches!(span.row, ArrangementRow::Automation(0)))
    );

    project.tracks[1].automation_collapsed = true;
    let collapsed = arrangement_layout(&project);
    assert!(
        !collapsed
            .iter()
            .any(|span| matches!(span.row, ArrangementRow::Automation(0)))
    );
    assert_eq!(collapsed.len(), expanded.len() - 1);
}

#[test]
fn row_heights_offset_and_hit_test_together() {
    let mut project = Project::default();
    project.tracks[0].height = 100.0;
    project.tracks[1].height = 40.0;
    let spans = arrangement_layout(&project);
    assert!((spans[0].height - 100.0).abs() < f32::EPSILON);
    assert!((spans[1].top - 100.0).abs() < f32::EPSILON);
    assert!((spans[1].height - 40.0).abs() < f32::EPSILON);
    assert_eq!(arrangement_row_at(&spans, 10.0), 0);
    assert_eq!(arrangement_row_at(&spans, 120.0), 1);
    // Past the last row the clamp keeps the index inside the layout.
    assert_eq!(arrangement_row_at(&spans, 10_000.0), spans.len() - 1);
}

#[test]
fn arrangement_rows_group_automation_under_its_track() {
    let mut project = Project::default();
    project.automation_tracks.push(AutomationTrack {
        target: generator_target(0, InstrumentParameter::Gain),
        ..AutomationTrack::default()
    });
    project.automation_tracks.push(AutomationTrack {
        target: ParameterTarget::InsertEffect {
            track_id: 2,
            slot_id: 0,
            parameter: EffectParameter::Mix,
        },
        ..AutomationTrack::default()
    });
    let rows: Vec<ArrangementRow> = arrangement_layout(&project)
        .into_iter()
        .map(|span| span.row)
        .collect();
    assert!(matches!(rows[0], ArrangementRow::Lane(0)));
    assert!(matches!(rows[1], ArrangementRow::Automation(0)));
    assert!(matches!(rows[2], ArrangementRow::Lane(1)));
    assert!(matches!(rows[3], ArrangementRow::Lane(2)));
}

#[test]
fn custom_cursor_svg_provides_shapes_for_each_tool_and_resize_target() {
    for kind in [
        "pen", "brush", "delete", "join", "pan", "resize", "move", "cut",
    ] {
        let points = svg_cursor_points(kind).unwrap();
        assert!(points.len() >= 3, "{kind} cursor has no polygon");
    }
}

#[test]
fn scale_highlighting_and_snapping_follow_the_root() {
    // C major contains C and B but not C sharp.
    assert!(scale_contains_pitch(0, "Major", 60));
    assert!(scale_contains_pitch(0, "Major", 71));
    assert!(!scale_contains_pitch(0, "Major", 61));
    // Transposing the root transposes the membership: D is a whole tone
    // above C, so it is in C major but not in C sharp major.
    assert!(scale_contains_pitch(0, "Major", 62));
    assert!(!scale_contains_pitch(1, "Major", 62));
    assert!(scale_contains_pitch(1, "Major", 61));
    // Chromatic always matches, so snapping is a no-op.
    assert!(scale_contains_pitch(0, "Chromatic", 61));
    // A non-scale note snaps to the closest scale tone.
    assert_eq!(snap_pitch_into_scale(0, "Major", 61), 60);
    assert_eq!(snap_pitch_into_scale(0, "Major", 63), 62);
    assert_eq!(snap_pitch_into_scale(0, "Major", 60), 60);
}

#[test]
fn alt_bypasses_the_piano_snap_setting() {
    assert_eq!(piano_snap_tick(117, 4, false), 116);
    assert_eq!(piano_snap_tick(117, 4, true), 117);
    assert_eq!(piano_snap_tick(16, 1, false), 16);
}

#[test]
fn resizing_the_note_start_keeps_its_end_and_minimum_duration() {
    assert_eq!(resize_note_start(8, 4, 8, 6), (6, 6));
    assert_eq!(resize_note_start(8, 4, 8, 20), (11, 1));
    assert_eq!(resize_note_start(8, 4, 8, 0), (0, 12));
}

#[test]
fn rack_grid_maps_pointer_and_paints_every_crossed_step() {
    assert_eq!(rack_step_at(48.0, 0.0, 32, 24.0, 32), 34);
    assert_eq!(rack_step_at(-10.0, 0.0, 32, 24.0, 32), 32);
    assert_eq!(rack_step_at(900.0, 0.0, 32, 24.0, 32), 63);
    assert_eq!(rack_step_span(37, 42), (37, 42));
    assert_eq!(rack_step_span(42, 37), (37, 42));
}

#[test]
fn routing_helpers_interpolate_colors_and_measure_cable_distance() {
    let red = egui::Color32::from_rgb(255, 0, 0);
    let blue = egui::Color32::from_rgb(0, 0, 255);
    let middle = color_lerp(red, blue, 0.5);
    assert_eq!(middle, egui::Color32::from_rgb(128, 0, 128));
    assert!(
        (point_segment_distance(
            egui::pos2(5.0, 3.0),
            egui::pos2(0.0, 0.0),
            egui::pos2(10.0, 0.0)
        ) - 3.0)
            .abs()
            < 0.001
    );
}

#[test]
fn lane_preview_follows_automation_of_its_own_rate() {
    use project::{AutomationLaneParameter, AutomationSource, TimeUnit, TimeValue};
    let mut project = Project::default();
    project.automation_tracks.push(AutomationTrack {
        target: generator_target(1, InstrumentParameter::Gain),
        source: AutomationSource::Lfo {
            shape: project::LfoShape::Sine,
            rate: TimeValue {
                amount: 500.0,
                unit: TimeUnit::Milliseconds,
            },
            retrigger: false,
            base: 0.5,
            depth: 0.5,
        },
        ..AutomationTrack::default()
    });
    project.automation_tracks.push(AutomationTrack {
        target: ParameterTarget::AutomationLane {
            track_id: 0,
            parameter: AutomationLaneParameter::RateAmount,
        },
        points: vec![AutomationPoint {
            step: 0,
            value: 0.0,
            curve: AutomationCurve::Linear,
        }],
        ..AutomationTrack::default()
    });
    project.normalize();

    let driven = lane_driven_parameters(&project, 0);
    assert_eq!(driven, vec![AutomationLaneParameter::RateAmount]);
    let resolved = resolved_lane_source(&project, 0, 0.0, 120.0, &|_| 0.0, 0).expect("lane exists");
    let AutomationSource::Lfo { rate, .. } = resolved else {
        panic!("source should stay an LFO");
    };
    assert!(
        (rate.amount - 0.1).abs() < 1e-3,
        "preview kept the stored rate {} instead of the automated one",
        rate.amount
    );
}

#[test]
fn peak_lane_drives_another_lane_parameter() {
    use project::{AutomationLaneParameter, AutomationSource, TimeUnit, TimeValue};
    let mut project = Project::default();
    project.automation_tracks.push(AutomationTrack {
        target: generator_target(1, InstrumentParameter::Gain),
        source: AutomationSource::Lfo {
            shape: project::LfoShape::Sine,
            rate: TimeValue {
                amount: 500.0,
                unit: TimeUnit::Milliseconds,
            },
            retrigger: false,
            base: 0.5,
            depth: 0.5,
        },
        ..AutomationTrack::default()
    });
    project.automation_tracks.push(AutomationTrack {
        target: ParameterTarget::AutomationLane {
            track_id: 0,
            parameter: AutomationLaneParameter::RateAmount,
        },
        source: AutomationSource::Peak {
            channel_id: 0,
            post_fx: true,
            attack: TimeValue {
                amount: 5.0,
                unit: TimeUnit::Milliseconds,
            },
            release: TimeValue {
                amount: 180.0,
                unit: TimeUnit::Milliseconds,
            },
            base: 0.0,
            depth: 1.0,
        },
        ..AutomationTrack::default()
    });
    project.normalize();

    // A peak lane writes its follower into lane 0's rate; silent means the
    // follower sits at the bottom of its range, so the two must differ.
    let quiet = resolved_lane_source(&project, 0, 0.0, 120.0, &|_| 0.0, 0).expect("lane exists");
    let loud = resolved_lane_source(&project, 0, 0.0, 120.0, &|_| 1.0, 0).expect("lane exists");
    let AutomationSource::Lfo { rate: quiet, .. } = quiet else {
        panic!("source should stay an LFO");
    };
    let AutomationSource::Lfo { rate: loud, .. } = loud else {
        panic!("source should stay an LFO");
    };
    assert!(
        (loud.amount - quiet.amount).abs() > 1.0,
        "peak level did not reach the LFO rate: {} vs {}",
        quiet.amount,
        loud.amount
    );
}

#[test]
fn peak_source_preview_uses_the_live_level() {
    use project::{AutomationSource, TimeUnit, TimeValue};
    let project = Project::default();
    let source = AutomationSource::Peak {
        channel_id: 0,
        post_fx: true,
        attack: TimeValue {
            amount: 5.0,
            unit: TimeUnit::Milliseconds,
        },
        release: TimeValue {
            amount: 180.0,
            unit: TimeUnit::Milliseconds,
        },
        base: 0.25,
        depth: 0.5,
    };
    let target = generator_target(0, InstrumentParameter::Gain);
    let silent = automation_source_preview(&source, target, &project.tracks, 120.0, 0, 0.0, 0.0);
    let loud = automation_source_preview(&source, target, &project.tracks, 120.0, 0, 0.0, 1.0);
    assert!((silent - 0.25).abs() < 1e-4, "silent level gave {silent}");
    assert!((loud - 0.75).abs() < 1e-4, "full level gave {loud}");
}
