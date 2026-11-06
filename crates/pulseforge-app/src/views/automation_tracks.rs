//! The editable list of automation lanes.

use crate::*;

impl PulseForgeApp {
    /// Draws one block per lane: name, target, clip range and the controls of
    /// the selected modulation source.
    ///
    /// # Parameters
    ///
    /// - `ui`: the parent UI.
    pub(crate) fn automation_track_list(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let mut remove_track = None;
        let target_labels: Vec<_> = self
            .project
            .automation_tracks
            .iter()
            .map(|track| parameter_target_label(&self.project, track.target))
            .collect();
        let snap_ticks = self.project.snap_ticks;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (track_id, track) in self.project.automation_tracks.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut track.enabled, "").changed();
                    changed |= ui
                        .add(egui::TextEdit::singleline(&mut track.name).desired_width(180.0))
                        .changed();
                    ui.label(&target_labels[track_id]);
                    if ui.small_button("Remove lane").clicked() {
                        remove_track = Some(track_id);
                    }
                });
                let source_label = match track.source {
                    project::AutomationSource::Curve => "Curve",
                    project::AutomationSource::RelativeCurve => "Relative curve",
                    project::AutomationSource::MidiEnvelope { .. } => "MIDI envelope",
                    project::AutomationSource::Lfo { .. } => "LFO",
                    project::AutomationSource::SampleAndHold { .. } => "Sample & hold",
                    project::AutomationSource::Perlin { .. } => "Perlin",
                };
                egui::ComboBox::from_id_salt(("automation_source", track_id))
                    .selected_text(source_label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Curve),
                                "Curve",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Curve;
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::RelativeCurve),
                                "Relative curve",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::RelativeCurve;
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(
                                    track.source,
                                    project::AutomationSource::MidiEnvelope { .. }
                                ),
                                "MIDI envelope",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::MidiEnvelope {
                                instrument_id: 0,
                                attack: project::TimeValue {
                                    amount: 5.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                decay: project::TimeValue {
                                    amount: 180.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                sustain: 0.7,
                                release: project::TimeValue {
                                    amount: 250.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                base: 0.0,
                                depth: 1.0,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Lfo { .. }),
                                "LFO",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Lfo {
                                shape: project::LfoShape::Sine,
                                rate: project::TimeValue {
                                    amount: 500.0,
                                    unit: project::TimeUnit::Milliseconds,
                                },
                                retrigger: true,
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(
                                    track.source,
                                    project::AutomationSource::SampleAndHold { .. }
                                ),
                                "Sample & hold",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::SampleAndHold {
                                seed: 1,
                                rate: project::TimeValue {
                                    amount: 4.0,
                                    unit: project::TimeUnit::Steps,
                                },
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(track.source, project::AutomationSource::Perlin { .. }),
                                "Perlin noise",
                            )
                            .clicked()
                        {
                            track.source = project::AutomationSource::Perlin {
                                seed: 1,
                                rate: project::TimeValue {
                                    amount: 8.0,
                                    unit: project::TimeUnit::Steps,
                                },
                                octaves: 4,
                                base: 0.5,
                                depth: 0.5,
                            };
                            changed = true;
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("Clip");
                    let old_start = track.start_step;
                    let start_changed = ui
                        .add(
                            egui::DragValue::new(&mut track.start_step)
                                .range(0..=u64::MAX)
                                .prefix("Start "),
                        )
                        .changed();
                    if start_changed {
                        track.start_step = snap_arrangement_step(track.start_step, snap_ticks);
                    }
                    if track.start_step != old_start {
                        let delta = track.start_step as i128 - old_start as i128;
                        for point in track.points.iter_mut().chain(&mut track.modulation_points) {
                            point.step = (point.step as i128 + delta)
                                .clamp(track.start_step as i128, u64::MAX as i128)
                                as u64;
                        }
                        changed = true;
                    }
                    let length_changed = ui
                        .add(
                            egui::DragValue::new(&mut track.length_steps)
                                .range(1..=u64::MAX)
                                .prefix("Length "),
                        )
                        .changed();
                    if length_changed {
                        let snap_steps = snap_ticks.div_ceil(project::TICKS_PER_STEP).max(1);
                        track.length_steps = track.length_steps.saturating_add(snap_steps / 2)
                            / snap_steps
                            * snap_steps;
                        track.length_steps = track.length_steps.max(snap_steps);
                        changed = true;
                    }
                    let clip_end = track.start_step.saturating_add(track.length_steps);
                    for point in track.points.iter_mut().chain(&mut track.modulation_points) {
                        point.step = point.step.min(clip_end.saturating_sub(1));
                    }
                });
                match &mut track.source {
                    project::AutomationSource::Curve | project::AutomationSource::RelativeCurve => {
                    }
                    project::AutomationSource::MidiEnvelope {
                        instrument_id,
                        attack,
                        decay,
                        sustain,
                        release,
                        base,
                        depth,
                    } => {
                        if !self.project.instruments.is_empty() {
                            *instrument_id =
                                (*instrument_id).min(self.project.instruments.len() - 1);
                            egui::ComboBox::from_id_salt(("automation_env_instrument", track_id))
                                .selected_text(&self.project.instruments[*instrument_id].name)
                                .show_ui(ui, |ui| {
                                    for (id, instrument) in
                                        self.project.instruments.iter().enumerate()
                                    {
                                        if ui
                                            .selectable_value(instrument_id, id, &instrument.name)
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                        }
                        ui.horizontal(|ui| {
                            changed |= edit_time_value(ui, track_id, "Attack", attack);
                            changed |= edit_time_value(ui, track_id, "Decay", decay);
                            changed |= lane_parameter_slider(
                                ui,
                                "Sustain",
                                sustain,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Sustain,
                                },
                            );
                            changed |= edit_time_value(ui, track_id, "Release", release);
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                        });
                    }
                    project::AutomationSource::Lfo {
                        shape,
                        rate,
                        retrigger,
                        base,
                        depth,
                    } => {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt(("automation_lfo_shape", track_id))
                                .selected_text(format!("{shape:?}"))
                                .show_ui(ui, |ui| {
                                    // Sample & hold is its own source now, so the LFO
                                    // shape list no longer offers it.
                                    for candidate in [
                                        project::LfoShape::Sine,
                                        project::LfoShape::Triangle,
                                        project::LfoShape::Saw,
                                        project::LfoShape::Square,
                                    ] {
                                        if ui
                                            .selectable_value(
                                                shape,
                                                candidate,
                                                format!("{candidate:?}"),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                            // The shape is an enum, automated as a plain integer.
                            automation_lane_button(
                                ui,
                                "LFO shape",
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Shape,
                                },
                            );
                        });
                        ui.horizontal(|ui| {
                            changed |= edit_time_value_automated(
                                ui,
                                track_id,
                                "Rate",
                                rate,
                                Some(ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::RateAmount,
                                }),
                            );
                            changed |= ui.checkbox(retrigger, "Retrigger").changed();
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                        });
                    }
                    project::AutomationSource::SampleAndHold {
                        seed,
                        rate,
                        base,
                        depth,
                    } => {
                        ui.horizontal(|ui| {
                            changed |= edit_time_value_automated(
                                ui,
                                track_id,
                                "Rate",
                                rate,
                                Some(ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::RateAmount,
                                }),
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                            ui.label("Seed");
                            changed |= ui
                                .add(egui::DragValue::new(seed).range(0..=u32::MAX))
                                .changed();
                        });
                    }
                    project::AutomationSource::Perlin {
                        seed,
                        rate,
                        octaves,
                        base,
                        depth,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label("Seed");
                            changed |= ui
                                .add(egui::DragValue::new(seed).range(0..=u32::MAX))
                                .changed();
                            changed |= edit_time_value_automated(
                                ui,
                                track_id,
                                "Rate",
                                rate,
                                Some(ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::RateAmount,
                                }),
                            );
                            let mut octave_count = *octaves as f32;
                            if lane_parameter_slider(
                                ui,
                                "Octaves",
                                &mut octave_count,
                                1.0..=16.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Octaves,
                                },
                            ) {
                                *octaves = octave_count.round().clamp(1.0, 16.0) as u32;
                            }
                            changed |= lane_parameter_slider(
                                ui,
                                "Base",
                                base,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Base,
                                },
                            );
                            changed |= lane_parameter_slider(
                                ui,
                                "Depth",
                                depth,
                                0.0..=1.0,
                                ParameterTarget::AutomationLane {
                                    track_id,
                                    parameter: project::AutomationLaneParameter::Depth,
                                },
                            );
                        });
                    }
                }
                if track.source.is_relative() {
                    ui.label("Draw offsets from -1 to 1; they are added to the parameter.");
                } else if !track.source.edits_points() {
                    ui.label("Draw a 0-1 modulation multiplier directly over the lane.");
                }
                ui.separator();
            }
        });
        if let Some(track_id) = remove_track {
            self.project.automation_tracks.remove(track_id);
            changed = true;
        }
        if changed {
            self.sync_project();
        }
    }
}
