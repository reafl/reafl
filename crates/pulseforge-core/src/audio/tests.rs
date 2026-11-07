//! Tests for the engine and its DSP helpers.

use std::{
    f32::consts::TAU,
    sync::{
        Arc, Mutex,
        mpsc::{self},
    },
};

use crate::project::{
    ClipSource, DefaultSample, InstrumentParameter, LfoShape, ParameterTarget, PianoNote, Project,
    SignalWaveform, SynthKind,
};

use super::dsp::*;
use super::engine::*;
use super::model::*;
use super::sample::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ArrangementClip, Pattern, TimeSignature};

    pub(crate) fn empty_project_updates() -> Mutex<Option<Box<Project>>> {
        Mutex::new(None)
    }

    #[test]
    pub(crate) fn sample_and_hold_lfo_changes_only_at_cycle_boundaries() {
        let held = lfo_wave(LfoShape::SampleAndHold, 2.0);
        assert_eq!(held, lfo_wave(LfoShape::SampleAndHold, 2.75));
        assert_ne!(held, lfo_wave(LfoShape::SampleAndHold, 3.0));
    }

    #[test]
    pub(crate) fn selected_pattern_playback_uses_instrument_mixer_lane() {
        let mut project = Project::default();
        project.patterns[0].step_hits.push(crate::project::StepHit {
            track_id: 0,
            step: 0,
        });
        project.instruments[0].default_mixer_lane = 2;
        let mut engine = AudioEngine::new(project, 48_000);
        engine.selected_pattern = Some(0);
        engine.trigger_step(0);
        assert!(engine.trigger_events.iter().any(|event| matches!(
            event,
            TriggerEvent::InstrumentHit {
                track_id: 0,
                track_id: 2
            }
        )));
    }

    #[test]
    pub(crate) fn mixer_eq_automation_moves_the_filter_gains() {
        let target = ParameterTarget::MixerChannel {
            channel_id: 0,
            parameter: crate::project::MixerParameter::LowDb,
        };
        let mut project = Project::default();
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target,
                points: vec![crate::project::AutomationPoint {
                    step: 0,
                    value: 1.0,
                    curve: crate::project::AutomationCurve::Linear,
                }],
                ..Default::default()
            });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        let before = engine.eq_gains[0];
        engine.playing = true;

        engine.apply_global_automation(0.0);

        assert_ne!(
            engine.eq_gains[0], before,
            "the automated EQ band never reached the filter gains"
        );
    }

    #[test]
    pub(crate) fn automation_of_a_lane_parameter_reaches_its_source() {
        use crate::project::{
            AutomationLaneParameter, AutomationSource, LfoShape, MixerParameter, TimeUnit,
            TimeValue,
        };
        let mut project = Project::default();
        // Lane 0 is an LFO whose base is 0 and whose depth is 0.2.
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target: ParameterTarget::Instrument {
                    track_id: 0,
                    parameter: InstrumentParameter::Gain,
                },
                source: AutomationSource::Lfo {
                    shape: LfoShape::Sine,
                    rate: TimeValue {
                        amount: 1.0,
                        unit: TimeUnit::Steps,
                    },
                    retrigger: false,
                    base: 0.0,
                    depth: 0.2,
                },
                ..Default::default()
            });
        // Lane 1 drives that base up to 1.
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target: ParameterTarget::AutomationLane {
                    track_id: 0,
                    parameter: AutomationLaneParameter::Base,
                },
                points: vec![crate::project::AutomationPoint {
                    step: 0,
                    value: 1.0,
                    curve: crate::project::AutomationCurve::Linear,
                }],
                ..Default::default()
            });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;

        engine.apply_global_automation(0.0);

        let base = engine.project.automation_tracks[0]
            .source
            .base()
            .expect("the LFO keeps a base");
        assert!(
            (base - 1.0).abs() < 1e-4,
            "the lane base never followed its automation, got {base}"
        );
        let _ = MixerParameter::Gain;
    }

    #[test]
    pub(crate) fn arrangement_loop_wraps_playhead_at_end_step() {
        let mut engine = AudioEngine::new(Project::default(), 48_000);
        engine.playing = true;
        engine.position_steps = 3;
        engine.loop_range = Some((2, 4));
        engine.frames_into_step = engine.frames_per_step() - 1.0;
        let (_sender, receiver) = mpsc::channel();
        engine.render_frame(&receiver, &empty_project_updates());
        assert_eq!(engine.position_steps, 2);
    }

    #[test]
    pub(crate) fn stopping_playback_releases_ringing_voices() {
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::Sine;
        project.instruments[0].synth_enabled = true;
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        engine.start_voice(0, 0, 60, 110, Some(64));
        assert!(
            engine
                .voices
                .iter()
                .any(|voice| voice.active && !voice.released)
        );

        let (sender, receiver) = mpsc::channel();
        sender.send(AudioCommand::SetPlaying(false)).unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());

        assert!(!engine.playing);
        assert!(
            engine
                .voices
                .iter()
                .filter(|voice| voice.active)
                .all(|voice| voice.released),
            "voices kept ringing after stop"
        );
    }

    #[test]
    pub(crate) fn default_drum_samples_are_non_silent_and_finite() {
        for kind in [
            DefaultSample::Kick,
            DefaultSample::Snare,
            DefaultSample::HiHat,
        ] {
            let sample = SampleData::default_drum(kind, 48_000).unwrap();
            assert!(!sample.frames.is_empty());
            assert!(
                sample
                    .frames
                    .iter()
                    .flatten()
                    .all(|frame| frame.is_finite())
            );
            let peak = sample
                .frames
                .iter()
                .flatten()
                .map(|frame| frame.abs())
                .fold(0.0_f32, f32::max);
            assert!(peak > 0.01, "{kind:?} sample is silent");
        }
        assert!(SampleData::default_drum(DefaultSample::None, 48_000).is_none());
    }

    #[test]
    pub(crate) fn offline_wav_render_writes_a_stereo_file_with_audio() {
        let mut project = Project::default();
        project.tracks[0].clips[0].length_steps = 16;
        let path =
            std::env::temp_dir().join(format!("pulseforge-render-{}.wav", std::process::id()));
        let frames = render_project_to_wav(&project, path.to_str().unwrap()).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 44_100);
        assert_eq!(reader.duration(), frames as u32);
        assert!(
            reader
                .samples::<i16>()
                .take(44_100)
                .any(|sample| sample.unwrap().unsigned_abs() > 0)
        );
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    pub(crate) fn ms20_instrument_reuses_a_single_voice() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.instruments[0].synth_kind = SynthKind::Ms20;
        let mut engine = AudioEngine::new(project, 48_000);

        engine.start_voice(0, 0, 60, 100, None);
        engine.start_voice(0, 0, 67, 100, None);

        let active: Vec<_> = engine
            .voices
            .iter()
            .filter(|voice| voice.active && voice.track_id == 0)
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].note, 67);
    }

    #[test]
    pub(crate) fn classic_synth_models_render_finite_audio() {
        for kind in [
            SynthKind::JunoPoly,
            SynthKind::M1Workstation,
            SynthKind::FmSynth,
        ] {
            let mut project = Project::default();
            project.instruments[0].synth_enabled = true;
            project.instruments[0].synth_kind = kind;
            let mut engine = AudioEngine::new(project, 48_000);
            engine.start_voice(0, 0, 60, 110, None);
            let (_sender, receiver) = mpsc::channel();
            let mut peak = 0.0_f32;
            for _ in 0..100 {
                let output = engine.render_frame(&receiver, &empty_project_updates());
                assert!(output.into_iter().all(f32::is_finite));
                peak = peak.max(output[0].abs()).max(output[1].abs());
            }
            assert!(peak > 0.0001, "{kind:?} produced silence");
            if kind == SynthKind::JunoPoly {
                let voice = engine
                    .voices
                    .iter()
                    .find(|voice| voice.active && voice.track_id == 0)
                    .unwrap();
                assert!((voice.phase - 2.0 * voice.sub_phase).abs() < 0.001);
            }
        }
    }

    #[test]
    pub(crate) fn instrument_automation_updates_audio_rate_controls() {
        let target = ParameterTarget::Instrument {
            track_id: 0,
            parameter: InstrumentParameter::CutoffHz,
        };
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::JunoPoly;
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target,
                points: vec![
                    crate::project::AutomationPoint {
                        step: 0,
                        value: 0.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                    crate::project::AutomationPoint {
                        step: 16,
                        value: 1.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                ],
                ..Default::default()
            });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);

        engine.update_instrument_controls(8.0);

        assert!((engine.instrument_controls[0].cutoff_hz - 8_020.0).abs() < 0.1);
    }

    #[test]
    pub(crate) fn peak_source_follows_the_lane_level() {
        let target = ParameterTarget::Instrument {
            track_id: 0,
            parameter: InstrumentParameter::Gain,
        };
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.instruments[0].synth_kind = SynthKind::Sine;
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target,
                source: crate::project::AutomationSource::Peak {
                    channel_id: 0,
                    post_fx: true,
                    attack: crate::project::TimeValue {
                        amount: 1.0,
                        unit: crate::project::TimeUnit::Milliseconds,
                    },
                    release: crate::project::TimeValue {
                        amount: 50.0,
                        unit: crate::project::TimeUnit::Milliseconds,
                    },
                    base: 0.0,
                    depth: 1.0,
                },
                ..Default::default()
            });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        engine.start_voice(0, 0, 60, 100, None);

        let (_sender, receiver) = mpsc::channel();
        let updates = empty_project_updates();
        for _ in 0..(CONTROL_INTERVAL * 16) {
            engine.render_frame(&receiver, &updates);
        }

        assert!(
            engine.peak_levels[0] > 0.0,
            "the peak follower never saw the track's audio"
        );
        let value = engine.automated_value(target, 0.8, 0.0);
        assert!(
            (value - 0.8).abs() > 1e-3,
            "the peak lane did not drive its parameter"
        );
    }

    #[test]
    pub(crate) fn control_rate_automation_reaches_the_instrument_within_one_block() {
        let target = ParameterTarget::Instrument {
            track_id: 0,
            parameter: InstrumentParameter::CutoffHz,
        };
        let mut project = Project::default();
        project.instruments[0].synth_kind = SynthKind::JunoPoly;
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target,
                points: vec![
                    crate::project::AutomationPoint {
                        step: 0,
                        value: 0.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                    crate::project::AutomationPoint {
                        step: 16,
                        value: 1.0,
                        curve: crate::project::AutomationCurve::Linear,
                    },
                ],
                ..Default::default()
            });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        let (_sender, receiver) = mpsc::channel();
        let resting = engine.instrument_controls[0].cutoff_hz;
        for _ in 0..CONTROL_INTERVAL + 1 {
            engine.render_frame(&receiver, &empty_project_updates());
        }
        let moved = engine.instrument_controls[0].cutoff_hz;
        assert!(
            (moved - resting).abs() > 1.0,
            "control-rate automation did not update the instrument ({resting} -> {moved})"
        );
        assert!(!engine.automation_index.is_empty());
    }

    #[test]
    pub(crate) fn disabled_automation_lanes_are_skipped_by_the_index() {
        let mut project = Project::default();
        project
            .automation_tracks
            .push(crate::project::AutomationTrack {
                target: ParameterTarget::Instrument {
                    track_id: 0,
                    parameter: InstrumentParameter::Gain,
                },
                enabled: false,
                ..Default::default()
            });
        project.normalize();
        let engine = AudioEngine::new(project, 48_000);
        assert!(engine.automation_index.is_empty());
    }

    #[test]
    pub(crate) fn metronome_clicks_without_any_instrument_content() {
        let project = Project::default();
        let mut engine = AudioEngine::new(project, 48_000);
        let (sender, receiver) = mpsc::channel();
        sender.send(AudioCommand::SetMetronome(true)).unwrap();
        sender.send(AudioCommand::SetPlaying(true)).unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        let mut peak = 0.0_f32;
        for _ in 0..64 {
            let output = engine.render_frame(&receiver, &empty_project_updates());
            assert!(output.into_iter().all(f32::is_finite));
            peak = peak.max(output[0].abs());
        }
        assert!(peak > 0.0005, "metronome produced no click");
    }

    #[test]
    pub(crate) fn unchanged_project_updates_keep_audio_runtimes_in_place() {
        let mut project = Project::default();
        project.tracks[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "held.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project.clone(), 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.5, 0.5]],
        }));
        let original_len = engine.audio_clips[0].len();

        let mut edited = project;
        edited.tracks[0].gain = 0.42;
        engine.apply_project(edited);

        assert_eq!(engine.audio_clips[0].len(), original_len);
        assert!(
            engine.audio_clips[0][0].sample.is_some(),
            "a non-structural project edit dropped the loaded sample"
        );
    }

    #[test]
    pub(crate) fn duplicated_audio_clips_share_one_loaded_sample() {
        let mut project = Project::default();
        project.tracks[0].clips.clear();
        project.tracks[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "shared.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project.clone(), 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.1, 0.1]],
        }));

        let mut duplicated = project;
        duplicated.tracks[0].clips.push(ArrangementClip {
            start_step: 16,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "shared.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        engine.apply_project(duplicated);

        assert_eq!(engine.audio_clips[0].len(), 2);
        for runtime in &engine.audio_clips[0] {
            assert!(
                runtime.sample.is_some(),
                "a duplicated audio clip lost its sample"
            );
        }
        assert!(Arc::ptr_eq(
            engine.audio_clips[0][0].sample.as_ref().unwrap(),
            engine.audio_clips[0][1].sample.as_ref().unwrap()
        ));
    }

    #[test]
    pub(crate) fn audio_clips_start_from_their_source_offset() {
        let mut project = Project {
            bpm: 120.0,
            ..Project::default()
        };
        project.tracks[0].clips.clear();
        project.tracks[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            offset_steps: 4,
            source: ClipSource::Audio {
                path: "offset.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.1, 0.1]; 48_000],
        }));

        engine.trigger_step(0);

        assert!(engine.audio_clips[0][0].active);
        // 4 steps at 120 BPM is 0.5 s, which is 24 000 frames at 48 kHz.
        assert!((engine.audio_clips[0][0].cursor - 24_000.0).abs() < 1.0);
    }

    #[test]
    pub(crate) fn note_repeats_queue_one_trigger_per_repeat() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.patterns[0].notes = vec![PianoNote {
            track_id: 0,
            pitch: 60,
            velocity: 100,
            duration_steps: 1,
            repeat_count: 2,
            repeat_ticks: 4,
            repeat_decay: 10,
            ..PianoNote::default()
        }];
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.trigger_step(0);
        let at_zero = engine
            .trigger_events
            .iter()
            .filter(|event| matches!(event, TriggerEvent::Note { .. }))
            .count();
        assert_eq!(at_zero, 1, "the first repeat fires on the note start");

        engine.trigger_step(4);
        let at_four = engine
            .trigger_events
            .iter()
            .filter(|event| matches!(event, TriggerEvent::Note { note, .. } if note.velocity == 90))
            .count();
        assert_eq!(at_four, 1, "the second repeat fires four ticks later");
    }

    #[test]
    pub(crate) fn disabling_smoothing_snaps_mixer_gain_immediately() {
        let project = Project::default();
        let mut engine = AudioEngine::new(project, 48_000);
        assert!(engine.smoothing_enabled);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(AudioCommand::SetSmoothing {
                enabled: false,
                milliseconds: 50.0,
            })
            .unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        assert!(!engine.smoothing_enabled);
        // With smoothing off, one rendered frame reaches the new target.
        let mut project = engine.project.clone();
        project.tracks[0].gain = 0.1;
        engine.apply_project(project);
        engine.render_frame(&receiver, &empty_project_updates());
        assert!((engine.lane_gain[0].value() - 0.1).abs() < 1e-5);
    }

    #[test]
    pub(crate) fn smoothing_ramps_toward_a_new_gain_over_several_frames() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(AudioCommand::SetSmoothing {
                enabled: true,
                milliseconds: 40.0,
            })
            .unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        let mut changed = engine.project.clone();
        changed.tracks[0].gain = 0.2;
        engine.apply_project(changed);
        engine.render_frame(&receiver, &empty_project_updates());
        let after_one = engine.lane_gain[0].value();
        assert!(
            after_one > 0.2 && after_one < 0.8,
            "gain should be mid-ramp, got {after_one}"
        );
        for _ in 0..4_000 {
            engine.render_frame(&receiver, &empty_project_updates());
        }
        assert!((engine.lane_gain[0].value() - 0.2).abs() < 1e-4);
    }

    #[test]
    pub(crate) fn preview_sample_plays_through_the_master_bus() {
        let project = Project::default();
        let mut engine = AudioEngine::new(project, 48_000);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(AudioCommand::PreviewSample(Box::new(SampleData {
                sample_rate: 48_000,
                frames: vec![[0.5, 0.5]; 480],
            })))
            .unwrap();
        engine.apply_pending(&receiver, &empty_project_updates());
        let output = engine.render_frame(&receiver, &empty_project_updates());
        assert!(output[0].abs() > 0.1, "preview sample was inaudible");
        for _ in 0..600 {
            engine.render_frame(&receiver, &empty_project_updates());
        }
        assert!(engine.preview.is_none(), "preview sample did not finish");
    }

    #[test]
    pub(crate) fn audio_clips_play_when_the_transport_starts_inside_them() {
        let mut project = Project {
            bpm: 120.0,
            ..Project::default()
        };
        project.tracks[0].clips.clear();
        project.tracks[0].clips.push(ArrangementClip {
            start_step: 8,
            length_steps: 8,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "mid.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.5, 0.5]; 48_000],
        }));
        engine.playing = true;
        // Transport jumps into the middle of the clip: step 10 of 8..16.
        engine.position_steps = 10;

        let output = engine.next_audio_clips(0);

        assert!(engine.audio_clips[0][0].active);
        assert!(
            output[0].abs() > 0.1,
            "clip was silent when starting mid-clip"
        );
        // Two steps at 120 BPM is 0.25 s, which is 12 000 frames at 48 kHz.
        assert!((engine.audio_clips[0][0].cursor - 12_000.0).abs() < 2.0);
    }

    #[test]
    pub(crate) fn audio_clips_rewind_after_leaving_their_range() {
        let mut project = Project::default();
        project.tracks[0].clips.clear();
        project.tracks[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 4,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "short.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.audio_clips[0][0].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.5, 0.5]; 1_000],
        }));
        engine.playing = true;
        engine.position_steps = 0;
        let _ = engine.next_audio_clips(0);
        engine.position_steps = 9;
        let _ = engine.next_audio_clips(0);
        assert!(!engine.audio_clips[0][0].active);
        assert!(!engine.audio_clips[0][0].finished);
        assert_eq!(engine.audio_clips[0][0].cursor, 0.0);
    }

    #[test]
    pub(crate) fn muted_instruments_and_tracks_stay_silent() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.instruments[0].enabled = false;
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        assert!(!engine.instrument_audible(0));

        let mut project = engine.project.clone();
        project.instruments[0].enabled = true;
        project.tracks[0].enabled = false;
        engine.apply_project(project);
        assert!(engine.instrument_audible(0));
        assert!(!engine.lane_audible(0));
    }

    #[test]
    pub(crate) fn soloing_one_instrument_silences_the_others() {
        let mut project = Project::default();
        project.instruments[1].solo = true;
        project.normalize();
        let engine = AudioEngine::new(project, 48_000);
        assert!(!engine.instrument_audible(0));
        assert!(engine.instrument_audible(1));
    }

    #[test]
    pub(crate) fn arpeggiator_schedules_one_note_per_rate_step() {
        let mut project = Project {
            bpm: 120.0,
            ..Project::default()
        };
        project.instruments[0].synth_enabled = true;
        project.instruments[0].arpeggio.enabled = true;
        project.instruments[0].arpeggio.octaves = 2;
        project.patterns[0].notes = vec![PianoNote {
            track_id: 0,
            pitch: 60,
            velocity: 100,
            duration_steps: 4,
            ..PianoNote::default()
        }];
        project.normalize();
        let mut engine = AudioEngine::new(project, 48_000);
        engine.trigger_step(0);
        // Two octaves over four steps => more than one scheduled event.
        assert!(
            engine.arp_queue.len() > 1,
            "arpeggiator scheduled {} events",
            engine.arp_queue.len()
        );
        assert!(engine.arp_queue.iter().all(|event| event.at_tick % 16 == 0));
    }

    #[test]
    pub(crate) fn external_generators_do_not_fall_back_to_a_builtin_voice() {
        let mut project = Project::default();
        project.instruments[0].synth_enabled = true;
        project.instruments[0].synth_kind = SynthKind::External;
        project.instruments[0].plugin_path = "C:/Plugins/Thing.vst3".to_owned();
        project.normalize();
        assert_eq!(project.instruments[0].synth_kind, SynthKind::External);
        let mut engine = AudioEngine::new(project, 48_000);
        engine.start_voice(0, 0, 60, 110, None);
        assert!(
            !engine
                .voices
                .iter()
                .any(|voice| voice.active && voice.track_id == 0),
            "an external plug-in must not allocate a built-in voice"
        );
    }

    #[test]
    pub(crate) fn signal_generator_waveforms_stay_in_unit_range() {
        for shape in [
            SignalWaveform::Sine,
            SignalWaveform::Triangle,
            SignalWaveform::Saw,
            SignalWaveform::Square,
        ] {
            for step in 0..512 {
                let phase = TAU * step as f32 / 512.0;
                let sample = waveform(shape, phase);
                assert!((-1.0..=1.0).contains(&sample));
            }
        }
    }

    #[test]
    pub(crate) fn pattern_clip_routes_note_to_its_arrangement_lane() {
        let mut project = Project::default();
        project.add_lane();
        project.tracks[1].name = "Second".to_owned();
        project.instruments[3].default_mixer_lane = 0;
        project.patterns = vec![Pattern {
            name: "Notes".to_owned(),
            length_steps: 16,
            notes: vec![PianoNote {
                repeat_count: 0,
                repeat_ticks: 0,
                repeat_decay: 0,
                track_id: 3,
                start_step: 0,
                start_substep: 0,
                duration_steps: 4,
                duration_substeps: 0,
                pitch: 60,
                velocity: 100,
            }],
            ..Pattern::default()
        }];
        project.tracks[1].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 16,
            source: ClipSource::Pattern { pattern_id: 0 },
            offset_steps: 0,
            ..ArrangementClip::default()
        });
        project.normalize();

        let mut engine = AudioEngine::new(project, 48_000);
        engine.playing = true;
        engine.trigger_step(0);
        let voice = engine.voices.iter().find(|voice| voice.active).unwrap();
        assert_eq!(voice.track_id, 1);
    }

    #[test]
    pub(crate) fn master_bus_receives_routed_audio_and_drives_the_output_meter() {
        let project = Project::default();
        let master_index = project.tracks.len();
        let mut engine = AudioEngine::new(project, 48_000);
        let edge = engine
            .send_edges
            .iter_mut()
            .find(|edge| edge.source == 0 && edge.destination == master_index)
            .unwrap();
        edge.delayed = [0.4, -0.3];
        let (_sender, receiver) = mpsc::channel();

        let output = engine.render_frame(&receiver, &empty_project_updates());

        assert!((output[0] - 0.4).abs() < 0.001);
        assert!((output[1] + 0.3).abs() < 0.001);
        assert!(engine.meter_levels[master_index] > 0.39);
    }

    #[test]
    pub(crate) fn project_updates_keep_audio_samples_attached_when_clips_move_lanes() {
        let mut project = Project::default();
        project.tracks[0].clips.push(ArrangementClip {
            start_step: 16,
            length_steps: 16,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "existing.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let mut engine = AudioEngine::new(project.clone(), 48_000);
        let existing_index = project.tracks[0]
            .clips
            .iter()
            .position(
                |clip| matches!(&clip.source, ClipSource::Audio { path } if path == "existing.wav"),
            )
            .unwrap();
        engine.audio_clips[0][existing_index].sample = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            frames: vec![[0.25, 0.25]],
        }));

        let moved_clip = project.tracks[0].clips.remove(existing_index);
        project.tracks[1].clips.push(moved_clip);
        project.tracks[0].clips.push(ArrangementClip {
            start_step: 0,
            length_steps: 8,
            offset_steps: 0,
            source: ClipSource::Audio {
                path: "new.wav".to_owned(),
            },
            ..ArrangementClip::default()
        });
        project.normalize();
        let (sender, receiver) = mpsc::channel();
        assert!(
            sender
                .send(AudioCommand::SetProject(Box::new(project.clone())))
                .is_ok()
        );
        engine.apply_pending(&receiver, &empty_project_updates());

        let moved_index = project.tracks[1]
            .clips
            .iter()
            .position(
                |clip| matches!(&clip.source, ClipSource::Audio { path } if path == "existing.wav"),
            )
            .unwrap();
        assert!(engine.audio_clips[1][moved_index].sample.is_some());
    }

    #[test]
    pub(crate) fn time_signature_changes_bar_steps_not_step_clock() {
        let project = Project {
            time_signature: TimeSignature {
                numerator: 7,
                denominator: 8,
            },
            ..Project::default()
        };
        let engine = AudioEngine::new(project, 48_000);
        assert!((engine.frames_per_step() - 6000.0).abs() < 0.001);
    }

    #[test]
    pub(crate) fn equalizer_is_flat_at_zero_and_changes_tonal_gain() {
        let mut state = EqState::default();
        let flat = process_eq([0.25, 0.25], &mut state, eq_gains(0.0, 0.0, 0.0), 0.1, 0.5);
        assert!((flat[0] - 0.25).abs() < 0.00001);

        let mut state = EqState::default();
        let boosted = process_eq([1.0, 1.0], &mut state, eq_gains(0.0, 0.0, 12.0), 0.1, 0.5);
        assert!(boosted[0] > 2.0);
    }

    #[test]
    pub(crate) fn invalid_sample_values_are_silenced() {
        assert_eq!(finite_sample(f32::NAN), 0.0);
        assert_eq!(finite_sample(f32::INFINITY), 0.0);
        assert_eq!(finite_sample(2.0), 1.0);
    }
}
